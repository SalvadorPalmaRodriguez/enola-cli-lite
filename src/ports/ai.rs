// Fase 10.2 — Puerto de gestión de instancias de IA local (Ollama + Open WebUI).
//
// Adaptado a la arquitectura hexagonal de `enola-cli-lite`:
// - Sin dependencias de `docker compose` ni `Command::new("docker")`.
// - Opera con tipos puros de `crate::domain::ai` (`AiInstance`, `AiStackConfig`).

pub use crate::domain::ai::{AiInstance, AiInstanceInfo, AiInstanceStatus, AiStackConfig};
pub use crate::domain::ai_model::{AiModel, AiModelStatus, GgufMetadata, ModelSource};
use crate::domain::error::Result;
use crate::ports::llm::LlmModelInfo;
use std::path::{Path, PathBuf};

/// Puerto de ciclo de vida para instancias de IA local (Ollama + Open WebUI).
#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait AiAdapterPort: Send + Sync {
    /// Despliega una nueva instancia de IA (contenedores `enola-ai-<name>-ollama`
    /// y `enola-ai-<name>-webui` en `127.0.0.1`).
    async fn create_instance(&self, config: AiStackConfig) -> Result<AiInstance>;

    /// Lista todas las instancias de IA gestionadas por Enola.
    async fn list_instances(&self) -> Result<Vec<AiInstance>>;

    /// Obtiene el estado detallado de una instancia de IA por nombre.
    async fn get_instance(&self, name: &str) -> Result<AiInstance>;

    /// Inicia los contenedores de una instancia detenida.
    async fn start_instance(&self, name: &str) -> Result<()>;

    /// Detiene los contenedores de una instancia en ejecución.
    async fn stop_instance(&self, name: &str) -> Result<()>;

    /// Elimina los contenedores, red y directorio de datos de una instancia.
    /// Exige `force = true` para confirmar el borrado destructivo.
    async fn delete_instance(&self, name: &str, force: bool) -> Result<()>;

    /// Recupera las últimas `lines` líneas de logs combinados (Ollama + WebUI).
    async fn get_instance_logs(&self, name: &str, lines: usize) -> Result<String>;
}

/// Alias de compatibilidad con `AiManagerPort`.
pub use AiAdapterPort as AiManagerPort;

// ── Fase 11 — biblioteca de modelos GGUF (`ai model`) ─────────────────────

/// Petición de descarga de un fichero GGUF.
#[derive(Debug, Clone)]
pub struct ModelDownloadRequest {
    /// URL http(s) del fichero `.gguf`.
    pub url: String,
    /// Ruta destino en el host (ya resuelta dentro de la biblioteca).
    pub destination: PathBuf,
    /// SHA-256 esperado (hex) para verificación post-descarga.
    pub expected_sha256: Option<String>,
    /// `true` para forzar la salida por el proxy Tor SOCKS5h (R22).
    pub use_tor: bool,
}

/// Resultado de una descarga GGUF completada.
#[derive(Debug, Clone)]
pub struct ModelDownloadOutcome {
    /// Ruta final del fichero.
    pub destination: PathBuf,
    /// Bytes escritos en disco.
    pub bytes_written: u64,
    /// SHA-256 real del fichero descargado (hex).
    pub sha256: String,
    /// `true` si la descarga reanudó un fichero parcial previo (Range 206).
    pub resumed: bool,
    /// `true` si el tráfico salió por el proxy Tor SOCKS5h.
    pub via_tor: bool,
}

/// Puerto de descarga de modelos GGUF (streaming + reanudación + SHA-256).
#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait ModelDownloadPort: Send + Sync {
    /// Descarga `req.url` a `req.destination` con streaming, reanudación por
    /// `Range` y verificación SHA-256 opcional. Si `req.use_tor` es `true` el
    /// tráfico sale por el proxy SOCKS5h resuelto (`resolve_tor_socks_proxy`).
    async fn download(&self, req: ModelDownloadRequest) -> Result<ModelDownloadOutcome>;
}

/// Puerto de la biblioteca de ficheros `.gguf` de una instancia
/// (`<base>/<instance>/ollama/models-gguf/`).
#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait AiModelLibraryPort: Send + Sync {
    /// Resuelve (y crea si hace falta) la ruta destino segura de `filename`
    /// en la biblioteca de la instancia. Rechaza path traversal.
    async fn resolve_library_target(&self, instance: &str, filename: &str) -> Result<PathBuf>;

    /// Lista los ficheros `.gguf` de la biblioteca con metadatos e índice.
    async fn list_library_models(&self, instance: &str) -> Result<Vec<AiModel>>;

    /// Lee y parsea el header GGUF de un fichero de la biblioteca.
    async fn read_gguf_metadata(&self, instance: &str, filename: &str) -> Result<GgufMetadata>;

    /// Importa un `.gguf` local a la biblioteca (valida header antes de copiar).
    async fn import_library_file(&self, instance: &str, source: &Path) -> Result<AiModel>;

    /// Borra un `.gguf` de la biblioteca.
    async fn delete_library_file(&self, instance: &str, filename: &str) -> Result<()>;

    /// Registra la proveniencia de una descarga en `index.json` (source,
    /// sha256 y repositorio HF `org/repo` de origen).
    async fn record_download_provenance(
        &self,
        instance: &str,
        filename: &str,
        sha256: &str,
        hf_repo: Option<String>,
    ) -> Result<()>;

    /// Resuelve el directorio destino del tokenizer HF de un repositorio
    /// (`org/repo`) dentro de la biblioteca de la instancia
    /// (`<library>/tokenizer/<org__repo>/`). No lo crea: lo crea el
    /// descargador.
    async fn resolve_tokenizer_dir(&self, instance: &str, repo: &str) -> Result<PathBuf>;
}

/// Puerto del registro de modelos de una instancia Ollama (HTTP `/api/*`).
#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait AiModelRegistryPort: Send + Sync {
    /// Lista los modelos registrados en el Ollama de la instancia (`/api/tags`).
    async fn list_registered_models(&self, instance: &str) -> Result<Vec<LlmModelInfo>>;

    /// Descarga un modelo del registro Ollama (`/api/pull`, p.ej. `llama3.1:8b`).
    async fn pull_registry_model(&self, instance: &str, model: &str) -> Result<()>;

    /// Registra un `.gguf` de la biblioteca en Ollama: sube el fichero como
    /// blob y lo crea con `/api/create` + `files` (H-16).
    async fn register_library_model(
        &self,
        instance: &str,
        filename: &str,
        name: &str,
    ) -> Result<()>;

    /// Elimina un modelo del registro Ollama (`DELETE /api/delete`).
    async fn delete_registered_model(&self, instance: &str, model: &str) -> Result<()>;
}

// ── Fase 14 — descarga del tokenizer HF (host-side, previa al entrenamiento) ──

/// Petición de descarga del tokenizer HF de un repositorio.
#[derive(Debug, Clone)]
pub struct TokenizerDownloadRequest {
    /// Repositorio HuggingFace (`org/repo`).
    pub repo: String,
    /// Directorio destino (host) donde guardar los ficheros del tokenizer.
    pub destination: PathBuf,
    /// `true` para forzar la salida por el proxy Tor SOCKS5h.
    pub use_tor: bool,
}

/// Resultado de una descarga de tokenizer HF.
#[derive(Debug, Clone)]
pub struct TokenizerDownloadOutcome {
    /// Directorio destino donde se guardaron los ficheros.
    pub destination: PathBuf,
    /// Número de ficheros descargados.
    pub files_downloaded: usize,
}

/// Puerto de descarga del tokenizer HF de un repositorio (en el host, para
/// que el contenedor de entrenamiento sin red pueda montarlo).
#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait TokenizerDownloadPort: Send + Sync {
    /// Descarga los ficheros del tokenizer de `req.repo` a `req.destination`.
    async fn download_tokenizer(
        &self,
        req: TokenizerDownloadRequest,
    ) -> Result<TokenizerDownloadOutcome>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::error::EnolaError;

    #[tokio::test]
    async fn mock_ai_adapter_port_create_and_list_contract() {
        let mut mock = MockAiAdapterPort::new();
        mock.expect_create_instance().returning(|cfg| {
            Ok(AiInstance {
                name: cfg.name,
                model: cfg.model,
                api_port: Some(cfg.api_port),
                webui_port: Some(cfg.webui_port),
                status: AiInstanceStatus::Running,
                ollama_status: "Up 1 second".into(),
                webui_status: "Up 1 second".into(),
                gpu_enabled: cfg.gpu_enabled,
                onion_address: None,
            })
        });
        mock.expect_list_instances().returning(|| Ok(vec![]));

        let cfg = AiStackConfig::new("dev", "llama3.1:8b", 11435, 8081, true).unwrap();
        let inst = mock.create_instance(cfg).await.unwrap();
        assert_eq!(inst.name, "dev");
        assert_eq!(inst.status, AiInstanceStatus::Running);
        assert!(mock.list_instances().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn mock_ai_adapter_port_delete_requires_force_contract() {
        let mut mock = MockAiAdapterPort::new();
        mock.expect_delete_instance().returning(|name, force| {
            if !force {
                Err(EnolaError::ValidationError(format!(
                    "Deleting AI instance '{name}' requires --force"
                )))
            } else {
                Ok(())
            }
        });

        assert!(mock.delete_instance("dev", false).await.is_err());
        assert!(mock.delete_instance("dev", true).await.is_ok());
    }

    #[tokio::test]
    async fn mock_tokenizer_download_port_contract() {
        let mut mock = MockTokenizerDownloadPort::new();
        mock.expect_download_tokenizer()
            .withf(|req| req.repo == "org/repo" && !req.use_tor)
            .returning(|req| {
                Ok(TokenizerDownloadOutcome {
                    destination: req.destination,
                    files_downloaded: 2,
                })
            });
        let out = mock
            .download_tokenizer(TokenizerDownloadRequest {
                repo: "org/repo".into(),
                destination: std::path::PathBuf::from("/tmp/tok"),
                use_tor: false,
            })
            .await
            .unwrap();
        assert_eq!(out.files_downloaded, 2);
    }
}
