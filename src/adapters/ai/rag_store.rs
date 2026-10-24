//! Fase 12.3 — Adapter de estado RAG por instancia (disco).
//!
//! Persiste bajo `<base>/<instance>/`:
//! - `rag_config.json`  → `RagConfig` (presets, chunking, backend).
//! - `rag_sources.json` → `Vec<AiSource>` (fuentes ligadas).
//!
//! Escritura atómica (tmp + rename) y permisos `0640`.

use crate::domain::ai::validate_ai_instance_name;
use crate::domain::error::{EnolaError, Result};
use crate::domain::rag::{AiSource, RagConfig};
use crate::ports::rag::AiRagStorePort;
use std::path::{Path, PathBuf};

/// Adapter de persistencia RAG por instancia.
pub struct FsRagStoreAdapter {
    /// Directorio base de instancias (`/srv/enola-ai`).
    base_dir: PathBuf,
}

impl FsRagStoreAdapter {
    /// Crea el adapter sobre el directorio base de instancias IA.
    pub fn new(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    fn instance_dir(&self, instance: &str) -> Result<PathBuf> {
        validate_ai_instance_name(instance)?;
        Ok(self.base_dir.join(instance))
    }

    fn config_path(&self, instance: &str) -> Result<PathBuf> {
        Ok(self.instance_dir(instance)?.join("rag_config.json"))
    }

    fn sources_path(&self, instance: &str) -> Result<PathBuf> {
        Ok(self.instance_dir(instance)?.join("rag_sources.json"))
    }

    /// Escritura atómica + permisos 0640.
    fn write_json(path: &Path, value: &impl serde::Serialize) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                EnolaError::InfrastructureError(format!(
                    "No se pudo crear '{}': {e}",
                    parent.display()
                ))
            })?;
        }
        let json = serde_json::to_string_pretty(value).map_err(|e| {
            EnolaError::InfrastructureError(format!("Serialización RAG fallida: {e}"))
        })?;
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, json).map_err(|e| {
            EnolaError::InfrastructureError(format!("No se pudo escribir '{}': {e}", tmp.display()))
        })?;
        std::fs::rename(&tmp, path).map_err(|e| {
            EnolaError::InfrastructureError(format!(
                "No se pudo renombrar '{}': {e}",
                path.display()
            ))
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o640));
        }
        Ok(())
    }

    fn read_sources(&self, instance: &str) -> Result<Vec<AiSource>> {
        let path = self.sources_path(instance)?;
        match std::fs::read_to_string(&path) {
            Ok(raw) => serde_json::from_str(&raw).map_err(|e| {
                EnolaError::InfrastructureError(format!(
                    "rag_sources.json de '{instance}' corrupto: {e}"
                ))
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(EnolaError::InfrastructureError(format!(
                "No se pudo leer rag_sources.json de '{instance}': {e}"
            ))),
        }
    }
}

#[async_trait::async_trait]
impl AiRagStorePort for FsRagStoreAdapter {
    async fn load_rag_config(&self, instance: &str) -> Result<RagConfig> {
        let path = self.config_path(instance)?;
        match std::fs::read_to_string(&path) {
            Ok(raw) => serde_json::from_str(&raw).map_err(|e| {
                EnolaError::InfrastructureError(format!(
                    "rag_config.json de '{instance}' corrupto: {e}"
                ))
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(RagConfig::default()),
            Err(e) => Err(EnolaError::InfrastructureError(format!(
                "No se pudo leer rag_config.json de '{instance}': {e}"
            ))),
        }
    }

    async fn save_rag_config(&self, instance: &str, config: &RagConfig) -> Result<()> {
        config.validate()?;
        Self::write_json(&self.config_path(instance)?, config)
    }

    async fn list_rag_sources(&self, instance: &str) -> Result<Vec<AiSource>> {
        self.read_sources(instance)
    }

    async fn add_rag_source(&self, instance: &str, source: AiSource) -> Result<()> {
        let mut sources = self.read_sources(instance)?;
        if sources
            .iter()
            .any(|s| s.location == source.location && s.source_type == source.source_type)
        {
            return Err(EnolaError::ValidationError(format!(
                "La fuente '{}' ({:?}) ya está ligada a '{instance}'",
                source.location, source.source_type
            )));
        }
        sources.push(source);
        Self::write_json(&self.sources_path(instance)?, &sources)
    }

    async fn mark_source_synced(
        &self,
        instance: &str,
        source_id: &str,
        iso_time: &str,
    ) -> Result<()> {
        let mut sources = self.read_sources(instance)?;
        let found = sources
            .iter_mut()
            .find(|s| s.id == source_id)
            .ok_or_else(|| {
                EnolaError::NotFound(format!(
                    "Fuente '{source_id}' no encontrada en '{instance}'"
                ))
            })?;
        found.last_synced = Some(iso_time.to_string());
        Self::write_json(&self.sources_path(instance)?, &sources)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::rag::AiSourceType;

    #[tokio::test]
    async fn config_roundtrip_and_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsRagStoreAdapter::new(dir.path().to_path_buf());
        let cfg = store.load_rag_config("dev").await.unwrap();
        assert_eq!(cfg, RagConfig::default());
        let mut custom = RagConfig::from_preset("faq").unwrap();
        custom.embedding_model = "mxbai-embed-large".into();
        store.save_rag_config("dev", &custom).await.unwrap();
        assert_eq!(store.load_rag_config("dev").await.unwrap(), custom);
    }

    #[tokio::test]
    async fn sources_add_list_dedupe_synced() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsRagStoreAdapter::new(dir.path().to_path_buf());
        assert!(store.list_rag_sources("dev").await.unwrap().is_empty());
        let src = AiSource {
            id: "s1".into(),
            instance_name: "dev".into(),
            source_type: AiSourceType::Filesystem,
            location: "/home/u/docs".into(),
            last_synced: None,
        };
        store.add_rag_source("dev", src.clone()).await.unwrap();
        assert!(store.add_rag_source("dev", src.clone()).await.is_err());
        store
            .mark_source_synced("dev", "s1", "2026-10-18T00:00:00Z")
            .await
            .unwrap();
        let list = store.list_rag_sources("dev").await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].last_synced.as_deref(), Some("2026-10-18T00:00:00Z"));
    }

    #[tokio::test]
    async fn rejects_invalid_instance_name() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsRagStoreAdapter::new(dir.path().to_path_buf());
        assert!(store.load_rag_config("../x").await.is_err());
    }
}
