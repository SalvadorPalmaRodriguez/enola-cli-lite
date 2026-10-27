// Fase 14 — Ports de fine-tuning por instancia.
//
// - `AiFinetuneJobsPort`: estado en `<base>/<instance>/finetune/jobs.json`
//   (atómico + 0640).
// - `FinetuneWorkspacePort`: ficheros del host (datasets, workspace del job,
//   GGUF exportado) — H-09: `application/` no toca el disco.

use crate::domain::ai_finetune::{FinetuneJob, FinetuneJobLayout};
use crate::domain::error::Result;
use std::path::{Path, PathBuf};

/// Port del registro de jobs de fine-tuning.
#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait AiFinetuneJobsPort: Send + Sync {
    /// Lista los jobs de una instancia (vacío si no hay).
    async fn list_jobs(&self, instance: &str) -> Result<Vec<FinetuneJob>>;

    /// Registra un job nuevo (id único; error si ya existe).
    async fn add_job(&self, job: FinetuneJob) -> Result<()>;

    /// Actualiza el estado (y error/finished/result) de un job existente.
    async fn update_job(&self, instance: &str, job: &FinetuneJob) -> Result<()>;
}

/// Port de los ficheros de fine-tuning en el host.
#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait FinetuneWorkspacePort: Send + Sync {
    /// Lee un dataset JSONL. Error de validación si no se puede leer o si
    /// supera `max_bytes`.
    async fn read_dataset(&self, path: &Path, max_bytes: u64) -> Result<String>;

    /// Escribe `train.jsonl` y `val.jsonl` en `out_dir` (lo crea si falta) y
    /// devuelve sus rutas.
    async fn write_split(
        &self,
        out_dir: &Path,
        train_jsonl: &str,
        val_jsonl: &str,
    ) -> Result<(PathBuf, PathBuf)>;

    /// Crea los directorios del job, despliega los scripts embebidos en
    /// `scripts/` y copia `dataset` a `layout.dataset_file()`.
    async fn prepare_job_workspace(&self, layout: &FinetuneJobLayout, dataset: &Path)
        -> Result<()>;

    /// A25-12: da al UID sin privilegios del entrenamiento acceso de SOLO
    /// lectura (grupo `FINETUNE_UID`, `0640`/`0750`) al tokenizer y al GGUF
    /// base que se montan en el contenedor; el resto de usuarios sigue sin acceso.
    async fn grant_model_read(&self, tokenizer_dir: &Path, gguf: &Path) -> Result<()>;

    /// `true` si `path` es un fichero existente.
    async fn is_file(&self, path: &Path) -> bool;
}
