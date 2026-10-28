//! Fase 13.2 — Adapter `AiProvidersStorePort` sobre disco.
//!
//! Persiste `~/.enola/providers.env` (vía `effective_home()` que respeta
//! `SUDO_USER`), escritura atómica `0600` sin ventana de permisos
//! (`infrastructure::atomic_secret_file`).
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
    pub fn for_invoking_user() -> Result<Self> {
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

    /// `(directorio, nombre)` del fichero.
    fn split(&self) -> Result<(&std::path::Path, &str)> {
        let bad = || {
            EnolaError::InfrastructureError(format!(
                "Ruta de providers no válida: '{}'",
                self.path.display()
            ))
        };
        let parent = self.path.parent().ok_or_else(bad)?;
        let name = self
            .path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(bad)?;
        Ok((parent, name))
    }

    /// SEC-B6: escritura atómica `0600` dentro de `~/.enola` abierto sin
    /// seguir symlinks (`SafeDir`, como `config.toml`, SEC-A6) y con el
    /// invocador de `sudo` como dueño del directorio y del fichero (antes
    /// quedaban de root en su home). Temporal `O_EXCL|O_NOFOLLOW` con nombre
    /// aleatorio + `renameat` (H-03).
    fn write_atomic(&self, content: &str) -> Result<()> {
        use crate::infrastructure::safe_dir::{invoker_owner, open_user_config_dir};
        let (parent, name) = self.split()?;
        let owner = invoker_owner();
        let io = |e: std::io::Error| {
            EnolaError::InfrastructureError(format!(
                "No se pudo escribir '{}': {e}",
                self.path.display()
            ))
        };
        open_user_config_dir(parent, owner)
            .map_err(io)?
            .write_atomic(name, content.as_bytes(), 0o600, owner)
            .map_err(io)
    }
}

/// Tope de lectura de `providers.env` (unas pocas claves).
const MAX_PROVIDERS_FILE_BYTES: u64 = 64 * 1024;

#[async_trait::async_trait]
impl AiProvidersStorePort for FsProvidersStore {
    async fn load(&self) -> Result<ProvidersFile> {
        use crate::infrastructure::safe_dir::{invoker_owner, open_user_config_dir};
        let (parent, name) = self.split()?;
        if !parent.exists() {
            return Ok(ProvidersFile::default());
        }
        // SEC-B6: como root no se sigue un `providers.env` symlink (p. ej. a
        // un fichero de otro usuario).
        let raw = open_user_config_dir(parent, invoker_owner())
            .and_then(|d| d.read_to_string_nofollow(name, MAX_PROVIDERS_FILE_BYTES))
            .map_err(|e| {
                EnolaError::InfrastructureError(format!("No se pudo leer providers.env: {e}"))
            })?;
        match raw {
            Some(raw) => parse_providers_env(&raw),
            None => Ok(ProvidersFile::default()),
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
    async fn save_uses_secure_atomic_write_without_predictable_tmp() {
        // H-03: el temporal no puede tener nombre predecible ni seguir symlinks,
        // y el directorio nuevo se crea 0700.
        let dir = tempfile::tempdir().unwrap();
        let enola_dir = dir.path().join(".enola");
        let store = FsProvidersStore::at(enola_dir.join("providers.env"));

        // Primer `set`: crea `.enola` con 0700.
        let entry = ProviderEntry {
            enabled: true,
            api_key: Some("test_k1".into()),
            model: None,
            endpoint: None,
        };
        store
            .set(ExternalProvider::OpenAi, entry.clone())
            .await
            .unwrap();
        let dir_mode = std::fs::metadata(&enola_dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(dir_mode, 0o700, "permisos dir={dir_mode:o}");

        // Un symlink plantado en el antiguo nombre de temporal no se sigue.
        let victim = dir.path().join("victim");
        std::fs::write(&victim, "intacto").unwrap();
        std::os::unix::fs::symlink(&victim, enola_dir.join("providers.tmp")).unwrap();
        store.set(ExternalProvider::Groq, entry).await.unwrap();
        assert_eq!(std::fs::read_to_string(&victim).unwrap(), "intacto");

        // Sin temporales huérfanos y destino 0600.
        let leftovers: Vec<String> = std::fs::read_dir(&enola_dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.contains(".tmp."))
            .collect();
        assert!(leftovers.is_empty(), "temporales: {leftovers:?}");
        let mode = std::fs::metadata(enola_dir.join("providers.env"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "permisos={mode:o}");
    }

    #[tokio::test]
    async fn load_does_not_follow_symlinked_providers_file() {
        // SEC-B6: un `providers.env` symlink no se lee (ni se filtra su contenido).
        let dir = tempfile::tempdir().unwrap();
        let enola_dir = dir.path().join(".enola");
        std::fs::create_dir(&enola_dir).unwrap();
        let victim = dir.path().join("secret");
        std::fs::write(&victim, "ENOLA_PROVIDER_OPENAI_API_KEY=ajena\n").unwrap();
        std::os::unix::fs::symlink(&victim, enola_dir.join("providers.env")).unwrap();
        let store = FsProvidersStore::at(enola_dir.join("providers.env"));
        assert!(store.load().await.is_err());
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
