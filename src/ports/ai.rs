// Fase 10.2 — Puerto de gestión de instancias de IA local (Ollama + Open WebUI).
//
// Adaptado a la arquitectura hexagonal de `enola-cli-lite`:
// - Sin dependencias de `docker compose` ni `Command::new("docker")`.
// - Opera con tipos puros de `crate::domain::ai` (`AiInstance`, `AiStackConfig`).

pub use crate::domain::ai::{AiInstance, AiInstanceInfo, AiInstanceStatus, AiStackConfig};
use crate::domain::error::Result;

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
}
