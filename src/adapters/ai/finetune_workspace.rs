// Fase 14 / H-09 — Adapter de ficheros de fine-tuning (`FinetuneWorkspacePort`).
//
// I/O que antes vivía en `application/ai_finetune.rs`: lectura acotada de
// datasets, escritura del split train/val y preparación del workspace del job
// (directorios, scripts embebidos — nunca en `/tmp`, SEC-001 — y copia del
// dataset). Las rutas del job las define `domain::ai_finetune::FinetuneJobLayout`.

use crate::domain::ai_finetune::FinetuneJobLayout;
use crate::domain::error::{EnolaError, Result};
use crate::ports::finetune::FinetuneWorkspacePort;
use std::path::{Path, PathBuf};

/// Workspace de fine-tuning sobre el sistema de ficheros del host.
#[derive(Debug, Default, Clone, Copy)]
pub struct FsFinetuneWorkspace;

impl FsFinetuneWorkspace {
    /// Construye el adapter (sin estado).
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl FinetuneWorkspacePort for FsFinetuneWorkspace {
    #[tracing::instrument(skip(self))]
    async fn read_dataset(&self, path: &Path, max_bytes: u64) -> Result<String> {
        let shown = path.display();
        let meta = tokio::fs::metadata(path)
            .await
            .map_err(|e| EnolaError::ValidationError(format!("No se pudo leer '{shown}': {e}")))?;
        if meta.len() > max_bytes {
            return Err(EnolaError::ValidationError(format!(
                "Dataset '{shown}' excede el máximo de {} MiB",
                max_bytes / (1024 * 1024)
            )));
        }
        tokio::fs::read_to_string(path)
            .await
            .map_err(|e| EnolaError::ValidationError(format!("No se pudo leer '{shown}': {e}")))
    }

    #[tracing::instrument(skip(self, train_jsonl, val_jsonl))]
    async fn write_split(
        &self,
        out_dir: &Path,
        train_jsonl: &str,
        val_jsonl: &str,
    ) -> Result<(PathBuf, PathBuf)> {
        tokio::fs::create_dir_all(out_dir).await.map_err(|e| {
            EnolaError::InfrastructureError(format!(
                "No se pudo crear '{}': {e}",
                out_dir.display()
            ))
        })?;
        let train_path = out_dir.join("train.jsonl");
        let val_path = out_dir.join("val.jsonl");
        for (path, content) in [(&train_path, train_jsonl), (&val_path, val_jsonl)] {
            tokio::fs::write(path, content).await.map_err(|e| {
                EnolaError::InfrastructureError(format!(
                    "No se pudo escribir '{}': {e}",
                    path.display()
                ))
            })?;
        }
        Ok((train_path, val_path))
    }

    #[tracing::instrument(skip(self))]
    async fn prepare_job_workspace(
        &self,
        layout: &FinetuneJobLayout,
        dataset: &Path,
    ) -> Result<()> {
        for d in layout.dirs() {
            tokio::fs::create_dir_all(d).await.map_err(|e| {
                EnolaError::InfrastructureError(format!("mkdir '{}': {e}", d.display()))
            })?;
        }
        // Dataset copiado al volumen del job (aislado del origen).
        tokio::fs::copy(dataset, layout.dataset_file())
            .await
            .map_err(|e| {
                EnolaError::InfrastructureError(format!(
                    "copiar dataset a '{}': {e}",
                    dataset.display()
                ))
            })?;
        // Script LoRA embebido → runtime dir root-owned (no /tmp — SEC-001).
        crate::infrastructure::finetune_scripts::ensure_finetune_scripts(&layout.scripts_dir)
            .map(|_| ())
    }

    async fn is_file(&self, path: &Path) -> bool {
        tokio::fs::metadata(path)
            .await
            .map(|m| m.is_file())
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn read_dataset_reads_and_enforces_size_limit() {
        let dir = tempfile::tempdir().unwrap();
        let ds = dir.path().join("d.jsonl");
        std::fs::write(&ds, "{\"a\":1}\n").unwrap();
        let ws = FsFinetuneWorkspace::new();
        assert_eq!(ws.read_dataset(&ds, 1024).await.unwrap(), "{\"a\":1}\n");
        let err = ws.read_dataset(&ds, 2).await.unwrap_err();
        assert!(err.to_string().contains("excede el máximo"), "{err}");
        let err = ws
            .read_dataset(&dir.path().join("nope.jsonl"), 1024)
            .await
            .unwrap_err();
        assert!(matches!(err, EnolaError::ValidationError(_)), "{err}");
        assert!(err.to_string().contains("No se pudo leer"), "{err}");
    }

    #[tokio::test]
    async fn write_split_creates_dir_and_both_files() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("nuevo/sub");
        let (train, val) = FsFinetuneWorkspace::new()
            .write_split(&out, "T\n", "V\n")
            .await
            .unwrap();
        assert_eq!(train, out.join("train.jsonl"));
        assert_eq!(val, out.join("val.jsonl"));
        assert_eq!(std::fs::read_to_string(train).unwrap(), "T\n");
        assert_eq!(std::fs::read_to_string(val).unwrap(), "V\n");
    }

    #[tokio::test]
    async fn prepare_job_workspace_creates_layout_scripts_and_dataset_copy() {
        let dir = tempfile::tempdir().unwrap();
        let ds = dir.path().join("train.jsonl");
        std::fs::write(&ds, "{\"messages\":[]}\n").unwrap();
        let layout = FinetuneJobLayout::new(dir.path(), "dev", "ft1");
        FsFinetuneWorkspace::new()
            .prepare_job_workspace(&layout, &ds)
            .await
            .unwrap();
        for d in layout.dirs() {
            assert!(d.is_dir(), "falta {}", d.display());
        }
        assert_eq!(
            std::fs::read_to_string(layout.dataset_file()).unwrap(),
            "{\"messages\":[]}\n"
        );
        assert!(layout.scripts_dir.join("train_lora.py").is_file());
        assert!(layout.scripts_dir.join("convert_hf_to_gguf.py").is_file());
    }

    #[tokio::test]
    async fn prepare_job_workspace_fails_on_missing_dataset() {
        let dir = tempfile::tempdir().unwrap();
        let layout = FinetuneJobLayout::new(dir.path(), "dev", "ft1");
        let err = FsFinetuneWorkspace::new()
            .prepare_job_workspace(&layout, &dir.path().join("nope.jsonl"))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("copiar dataset"), "{err}");
    }

    #[tokio::test]
    async fn is_file_distinguishes_files_dirs_and_missing() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("f");
        std::fs::write(&f, "x").unwrap();
        let ws = FsFinetuneWorkspace::new();
        assert!(ws.is_file(&f).await);
        assert!(!ws.is_file(dir.path()).await);
        assert!(!ws.is_file(&dir.path().join("nope")).await);
    }
}
