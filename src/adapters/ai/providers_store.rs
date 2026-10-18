//! Fase 13.2 — Adapter `AiProvidersStorePort` sobre disco.
//!
//! Persiste `~/.enola/providers.env` (vía `effective_home()` que respeta
//! `SUDO_USER`), escritura atómica (tmp + rename) y permisos `0600`.
//! Las keys solo viven en este fichero y en memoria — jamás en logs.

use crate::domain::ai_providers::{
    parse_providers_env, serialize_providers_env, validate_api_key, ExternalProvider,
    ProviderEntry, ProvidersFile,
};
use crate::domain::error::{EnolaError, Result};
use crate::ports::external_ai::AiProvidersStorePort;
use std::path::PathBuf;

/// Adapter de persistencia de providers en `~/.enola/providers.env`.
pub struct FsProvidersStore {
    /// Ruta del fichero (inyectable para tests).
    path: PathBuf,
}

impl FsProvidersStore {
    /// Store por defecto en `~/.enola/providers.env` (respeta SUDO_USER).
    pub fn default() -> Result<Self> {
        let home = crate::infrastructure::config_loader::effective_home().ok_or_else(|| {
            EnolaError::InfrastructureError(
                "No se pudo resolver el home del usuario invocador".into(),
            )
        })?;
        Ok(Self {
            path: home.join(".enola").join("providers.env"),
        })
    }

    /// Store sobre una ruta arbitraria (tests).
    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }

    /// Escritura atómica con permisos `0600` (el fichero contiene secretos).
    fn write_atomic(&self, content: &str) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                EnolaError::InfrastructureError(format!(
                    "No se pudo crear '{}': {e}",
                    parent.display()
                ))
            })?;
        }
        let tmp = self.path.with_extension("tmp");
        std::fs::write(&tmp, content).map_err(|e| {
            EnolaError::InfrastructureError(format!("No se pudo escribir '{}': {e}", tmp.display()))
        })?;
        std::fs::rename(&tmp, &self.path).map_err(|e| {
            EnolaError::InfrastructureError(format!(
                "No se pudo renombrar '{}': {e}",
                self.path.display()
            ))
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&self.path, std::fs::Permissions::from_mode(0o600)).map_err(
                |e| {
                    EnolaError::InfrastructureError(format!(
                        "No se pudieron fijar permisos 0600 en '{}': {e}",
                        self.path.display()
                    ))
                },
            )?;
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl AiProvidersStorePort for FsProvidersStore {
    async fn load(&self) -> Result<ProvidersFile> {
        match std::fs::read_to_string(&self.path) {
            Ok(raw) => parse_providers_env(&raw),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ProvidersFile::default()),
            Err(e) => Err(EnolaError::InfrastructureError(format!(
                "No se pudo leer providers.env: {e}"
            ))),
        }
    }

    async fn save(&self, file: &ProvidersFile) -> Result<()> {
        self.write_atomic(&serialize_providers_env(file))
    }

    async fn get(&self, provider: ExternalProvider) -> Result<Option<ProviderEntry>> {
        Ok(self.load().await?.entries.get(&provider).cloned())
    }

    async fn set(&self, provider: ExternalProvider, entry: ProviderEntry) -> Result<()> {
        if let Some(k) = &entry.api_key {
            validate_api_key(k)?; // T9 — inyección de control en el env
        }
        let mut file = self.load().await?;
        file.entries.insert(provider, entry);
        self.save(&file).await
    }

    async fn remove(&self, provider: ExternalProvider) -> Result<bool> {
        let mut file = self.load().await?;
        let existed = file.entries.remove(&provider).is_some();
        if existed {
            self.save(&file).await?;
        }
        Ok(existed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[tokio::test]
    async fn roundtrip_set_get_remove_and_perms() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsProvidersStore::at(dir.path().join("providers.env"));

        assert!(store.load().await.unwrap().entries.is_empty());
        let entry = ProviderEntry {
            enabled: true,
            api_key: Some("test_k1".into()),
            model: Some("gpt-4o".into()),
            endpoint: None,
        };
        store
            .set(ExternalProvider::OpenAi, entry.clone())
            .await
            .unwrap();

        // Permisos 0600
        let mode = std::fs::metadata(dir.path().join("providers.env"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "permisos={mode:o}");

        let got = store.get(ExternalProvider::OpenAi).await.unwrap().unwrap();
        assert_eq!(got, entry);
        assert!(store.remove(ExternalProvider::OpenAi).await.unwrap());
        assert!(!store.remove(ExternalProvider::OpenAi).await.unwrap());
        assert!(store.get(ExternalProvider::OpenAi).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn set_rejects_control_chars_in_key() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsProvidersStore::at(dir.path().join("providers.env"));
        let bad = ProviderEntry {
            enabled: true,
            api_key: Some("bad\nkey".into()),
            model: None,
            endpoint: None,
        };
        assert!(store.set(ExternalProvider::Groq, bad).await.is_err());
        // Nada persistido
        assert!(store.get(ExternalProvider::Groq).await.unwrap().is_none());
    }
}
