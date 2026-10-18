// Fase 10.6 — Casos de uso de aplicación para instancias de IA (`ai`):
// - Creación con pull de modelo por HTTP (`/api/pull` vía `LlmEngine`, R12)
// - Publicación de Open WebUI en `.onion` (Nginx + Tor + verificación opcional con `VerifyOnion`)
// - Ocultación (`hide`) de Open WebUI en Tor + Nginx
// - Enriquecimiento de `AiInstance` con su `onion_address` activa
//
// Regla hexagonal inviolable (§2 AGENTS.md): este módulo importa ÚNICAMENTE traits de
// `crate::ports::*` y tipos de `crate::domain::*`. CERO imports de `crate::adapters::*`.

use crate::application::verify_onion::{OnionVerification, VerifyOnion};
use crate::domain::ai::{AiInstance, AiStackConfig};
use crate::domain::error::{EnolaError, Result};
use crate::ports::ai::AiAdapterPort;
use crate::ports::http_probe::HttpProbePort;
use crate::ports::llm::LlmEngine;
use crate::ports::manifest::ManifestPort;
use crate::ports::service::ServiceManagerPort;
use crate::ports::tor::TorManagerPort;
use crate::ports::web::NginxManagerPort;
use serde::Serialize;
use std::sync::Arc;
use std::time::Duration;

/// Resultado estructurado de `ai publish <name>`.
#[derive(Debug, Clone, Serialize)]
pub struct PublishAiResult {
    /// Nombre lógico de la instancia IA.
    pub name: String,
    /// Dirección `.onion` donde se publicó Open WebUI.
    pub onion_address: String,
    /// Puerto local (`127.0.0.1`) de Open WebUI.
    pub webui_port: u16,
    /// Puerto local (`127.0.0.1`) donde escucha el reverse proxy Nginx.
    pub nginx_port: u16,
    /// Resultado de la verificación `.onion` (si se solicitó `verify`).
    pub verification: Option<OnionVerification>,
}

/// Caso de uso: publicar la interfaz Open WebUI de una instancia IA en Tor (`.onion`)
/// a través de Nginx reverse proxy, con verificación opcional (`VerifyOnion`).
pub struct PublishAiInstance {
    ai_adapter: Arc<dyn AiAdapterPort + Send + Sync>,
    nginx_manager: Arc<dyn NginxManagerPort + Send + Sync>,
    tor_manager: Arc<dyn TorManagerPort + Send + Sync>,
    service_manager: Arc<dyn ServiceManagerPort + Send + Sync>,
    manifest: Arc<dyn ManifestPort + Send + Sync>,
    http_probe: Option<Arc<dyn HttpProbePort + Send + Sync>>,
}

impl PublishAiInstance {
    /// Construye el caso de uso inyectando los puertos de infraestructura.
    pub fn new(
        ai_adapter: Arc<dyn AiAdapterPort + Send + Sync>,
        nginx_manager: Arc<dyn NginxManagerPort + Send + Sync>,
        tor_manager: Arc<dyn TorManagerPort + Send + Sync>,
        service_manager: Arc<dyn ServiceManagerPort + Send + Sync>,
        manifest: Arc<dyn ManifestPort + Send + Sync>,
    ) -> Self {
        Self {
            ai_adapter,
            nginx_manager,
            tor_manager,
            service_manager,
            manifest,
            http_probe: None,
        }
    }

    /// Inyecta la sonda HTTP/SOCKS5h para verificar la `.onion` tras publicar (Fase 6).
    pub fn with_probe(mut self, probe: Arc<dyn HttpProbePort + Send + Sync>) -> Self {
        self.http_probe = Some(probe);
        self
    }

    /// Publica Open WebUI de la instancia `name` en Tor vía Nginx y opcionalmente verifica.
    pub async fn execute(&self, name: &str, verify: bool) -> Result<PublishAiResult> {
        crate::domain::naming::validate_service_name(name)?;

        let inst = self.ai_adapter.get_instance(name).await?;
        let webui_port = inst.webui_port.ok_or_else(|| {
            EnolaError::ValidationError(format!(
                "La instancia IA '{name}' no tiene un puerto WebUI activo (¿está en ejecución?)"
            ))
        })?;

        let nginx_port = self.nginx_manager.find_available_port(10000, 15000).await?;

        let service_name = crate::domain::ai::ai_tor_service_name(name);
        let deploy_web = crate::application::deploy_tor_web_service::DeployTorWebService::new(
            self.nginx_manager.clone(),
            self.tor_manager.clone(),
            self.service_manager.clone(),
            self.manifest.clone(),
        );
        let onion_address = deploy_web
            .execute(
                crate::application::deploy_tor_web_service::DeployTorWebServiceRequest {
                    service_name,
                    backend_port: webui_port,
                    nginx_port,
                    enable_auth: false,
                },
            )
            .await?;

        let verification = if verify {
            if let Some(probe) = &self.http_probe {
                let verifier =
                    VerifyOnion::new(probe.clone()).with_retry(2, Duration::from_millis(50));
                Some(verifier.execute(&onion_address).await?)
            } else {
                None
            }
        } else {
            None
        };

        Ok(PublishAiResult {
            name: name.to_string(),
            onion_address,
            webui_port,
            nginx_port,
            verification,
        })
    }
}

/// Caso de uso: retirar la exposición `.onion` (Tor + Nginx) de una instancia IA.
pub struct HideAiInstance {
    tor_manager: Arc<dyn TorManagerPort + Send + Sync>,
    nginx_manager: Option<Arc<dyn NginxManagerPort + Send + Sync>>,
}

impl HideAiInstance {
    pub fn new(
        tor_manager: Arc<dyn TorManagerPort + Send + Sync>,
        nginx_manager: Option<Arc<dyn NginxManagerPort + Send + Sync>>,
    ) -> Self {
        Self {
            tor_manager,
            nginx_manager,
        }
    }

    pub async fn execute(&self, name: &str) -> Result<()> {
        crate::domain::naming::validate_service_name(name)?;
        let service_name = crate::domain::ai::ai_tor_service_name(name);
        let remover = crate::application::remove_tor_service::RemoveTorService::new(
            self.tor_manager.clone(),
            self.nginx_manager.clone(),
        );
        remover.execute(&service_name).await
    }
}

/// Caso de uso: crear instancia IA y opcionalmente descargar el modelo vía `LlmEngine::pull_model` (R12).
pub struct CreateAiInstance {
    ai_adapter: Arc<dyn AiAdapterPort + Send + Sync>,
    llm_engine: Option<Arc<dyn LlmEngine + Send + Sync>>,
    /// Timeout de espera de readiness de la API Ollama antes del pull inicial.
    ready_timeout: std::time::Duration,
}

/// Espera a que la API de Ollama responda (`/api/tags`) hasta `timeout`.
async fn wait_for_ollama_api(engine: &dyn LlmEngine, timeout: std::time::Duration) {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if engine.list_models().await.is_ok() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    tracing::warn!(
        timeout_secs = timeout.as_secs(),
        "La API de Ollama no respondió a tiempo; el pull inicial puede fallar"
    );
}

impl CreateAiInstance {
    pub fn new(ai_adapter: Arc<dyn AiAdapterPort + Send + Sync>) -> Self {
        Self {
            ai_adapter,
            llm_engine: None,
            ready_timeout: std::time::Duration::from_secs(60),
        }
    }

    /// Override del timeout de readiness (tests).
    pub fn with_ready_timeout(mut self, timeout: std::time::Duration) -> Self {
        self.ready_timeout = timeout;
        self
    }

    pub fn with_llm_engine(mut self, engine: Arc<dyn LlmEngine + Send + Sync>) -> Self {
        self.llm_engine = Some(engine);
        self
    }

    pub async fn execute(&self, config: AiStackConfig) -> Result<AiInstance> {
        let pull_model = config.pull_model_on_create;
        let model_name = config.model.clone();
        let inst = self.ai_adapter.create_instance(config).await?;
        if pull_model {
            if let Some(engine) = &self.llm_engine {
                // El contenedor Ollama recién arrancado tarda unos segundos en
                // bindear su API — esperar readiness antes del pull inicial
                // (evita la race "error sending request" de la E2E real).
                wait_for_ollama_api(engine.as_ref(), self.ready_timeout).await;
                if let Err(err) = engine.pull_model(&model_name).await {
                    tracing::warn!(
                        instance = %inst.name,
                        model = %model_name,
                        error = %err,
                        "No se pudo completar el pull inicial del modelo; la instancia sigue activa"
                    );
                    eprintln!(
                        "⚠️  Instancia '{}' creada, pero la descarga inicial del modelo '{}' falló ({}). \
                         Puedes reintentarlo más tarde.",
                        inst.name, model_name, err
                    );
                }
            }
        }
        Ok(inst)
    }
}

/// Enriquece una lista de `AiInstance` con sus direcciones `.onion` activas en Tor.
pub async fn enrich_instances_with_onion(
    instances: &mut [AiInstance],
    tor_manager: &(dyn TorManagerPort + Send + Sync),
) {
    let Ok(tor_services) = tor_manager.list_hidden_services().await else {
        return;
    };
    for inst in instances.iter_mut() {
        let candidates = [
            format!("proxy_ai-{}", inst.name),
            format!("ai-{}", inst.name),
            format!("ai_{}", inst.name),
            format!("proxy_ai_{}", inst.name),
        ];
        if let Some(found) = tor_services
            .iter()
            .find(|s| candidates.iter().any(|c| c == &s.name))
        {
            if !found.hostname.is_empty() && !found.hostname.contains("pending") {
                inst.onion_address = Some(found.hostname.clone());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ai::AiInstanceStatus;
    use crate::ports::ai::MockAiAdapterPort;
    use crate::ports::http_probe::{MockHttpProbePort, ProbeOutcome};
    use crate::ports::llm::MockLlmEngine;
    use crate::ports::manifest::MockManifestPort;
    use crate::ports::service::MockServiceManagerPort;
    use crate::ports::tor::{MockTorManagerPort, TorServiceInfo};
    use crate::ports::web::MockNginxManagerPort;

    fn mock_manifest() -> Arc<dyn ManifestPort + Send + Sync> {
        let mut m = MockManifestPort::new();
        m.expect_append().returning(|_, _| Ok(())).times(0..);
        m.expect_remove().returning(|_, _| Ok(())).times(0..);
        Arc::new(m)
    }

    #[tokio::test]
    async fn publish_ai_instance_configures_nginx_and_tor_and_verifies_onion() {
        let mut ai = MockAiAdapterPort::new();
        ai.expect_get_instance()
            .withf(|n| n == "dev")
            .returning(|_| {
                Ok(AiInstance {
                    name: "dev".into(),
                    model: "llama3.1:8b".into(),
                    api_port: Some(11435),
                    webui_port: Some(8081),
                    status: AiInstanceStatus::Running,
                    ollama_status: "Up".into(),
                    webui_status: "Up".into(),
                    gpu_enabled: true,
                    onion_address: None,
                })
            });

        let mut nginx = MockNginxManagerPort::new();
        nginx
            .expect_find_available_port()
            .returning(|_, _| Ok(10081));
        nginx
            .expect_create_proxy_config()
            .withf(|cfg| {
                cfg.service_name == "ai-dev" && cfg.listen_port == 10081 && cfg.backend_port == 8081
            })
            .times(1)
            .returning(|_| Ok(()));
        nginx.expect_validate_config().returning(|| Ok(true));
        nginx
            .expect_enable_site()
            .withf(|s| s == "proxy_ai-dev")
            .times(1)
            .returning(|_| Ok(()));
        nginx.expect_reload().times(1).returning(|| Ok(()));

        let onion_addr =
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.onion".to_string();
        let onion_clone = onion_addr.clone();
        let mut tor = MockTorManagerPort::new();
        tor.expect_deploy_hidden_service()
            .withf(|name, ports| name == "proxy_ai-dev" && *ports == vec![(80u16, 10081u16)])
            .times(1)
            .returning(move |_, _| Ok(onion_clone.clone()));
        tor.expect_reload_tor().times(1).returning(|| Ok(()));

        let mut sm = MockServiceManagerPort::new();
        sm.expect_is_active().returning(|_| Ok(true));

        let mut probe = MockHttpProbePort::new();
        probe.expect_get().times(1).returning(|_, _| {
            Ok(ProbeOutcome {
                status: 200,
                latency_ms: 42,
                title: Some("Open WebUI".into()),
            })
        });

        let use_case = PublishAiInstance::new(
            Arc::new(ai),
            Arc::new(nginx),
            Arc::new(tor),
            Arc::new(sm),
            mock_manifest(),
        )
        .with_probe(Arc::new(probe));

        let result = use_case.execute("dev", true).await.unwrap();
        assert_eq!(result.name, "dev");
        assert_eq!(result.webui_port, 8081);
        assert_eq!(result.nginx_port, 10081);
        assert_eq!(result.onion_address, onion_addr);
        let ver = result.verification.expect("verification present");
        assert!(ver.reachable);
        assert_eq!(ver.status, Some(200));
    }

    #[tokio::test]
    async fn hide_ai_instance_removes_tor_and_nginx_config() {
        let mut tor = MockTorManagerPort::new();
        tor.expect_remove_hidden_service()
            .withf(|name| name == "ai-dev" || name == "proxy_ai-dev")
            .times(1)
            .returning(|_| Ok(()));

        let mut nginx = MockNginxManagerPort::new();
        nginx.expect_disable_site().returning(|_| Ok(()));
        nginx.expect_delete_site_config().returning(|_| Ok(()));
        nginx.expect_reload().returning(|| Ok(()));

        let use_case = HideAiInstance::new(Arc::new(tor), Some(Arc::new(nginx)));
        use_case.execute("dev").await.unwrap();
    }

    #[tokio::test]
    async fn create_ai_instance_pulls_model_via_llm_engine_when_enabled() {
        let mut ai = MockAiAdapterPort::new();
        ai.expect_create_instance().times(1).returning(|cfg| {
            Ok(AiInstance {
                name: cfg.name,
                model: cfg.model,
                api_port: Some(cfg.api_port),
                webui_port: Some(cfg.webui_port),
                status: AiInstanceStatus::Running,
                ollama_status: "Up".into(),
                webui_status: "Up".into(),
                gpu_enabled: cfg.gpu_enabled,
                onion_address: None,
            })
        });

        let mut llm = MockLlmEngine::new();
        // El wait-for-readiness consulta /api/tags antes del pull.
        llm.expect_list_models().returning(|| Ok(Vec::new()));
        llm.expect_pull_model()
            .withf(|m| m == "llama3.1:8b")
            .times(1)
            .returning(|_| Ok(()));

        let use_case = CreateAiInstance::new(Arc::new(ai)).with_llm_engine(Arc::new(llm));
        let cfg = AiStackConfig::new("dev", "llama3.1:8b", 11435, 8081, true).unwrap();
        let inst = use_case.execute(cfg).await.unwrap();
        assert_eq!(inst.name, "dev");
    }

    #[tokio::test]
    async fn enrich_instances_with_onion_matches_proxy_ai_service() {
        let mut tor = MockTorManagerPort::new();
        tor.expect_list_hidden_services().returning(|| {
            Ok(vec![TorServiceInfo {
                name: "proxy_ai-dev".into(),
                hostname: "devonion.onion".into(),
                hidden_service_dir: "/var/lib/tor/enola_proxy_ai-dev".into(),
                ports: vec![(80, "127.0.0.1:10081".to_string())],
                clients: vec![],
                active: true,
                auth_enabled: false,
            }])
        });

        let mut instances = vec![AiInstance {
            name: "dev".into(),
            model: "llama3.1:8b".into(),
            api_port: Some(11435),
            webui_port: Some(8081),
            status: AiInstanceStatus::Running,
            ollama_status: "Up".into(),
            webui_status: "Up".into(),
            gpu_enabled: false,
            onion_address: None,
        }];

        enrich_instances_with_onion(&mut instances, &tor).await;
        assert_eq!(
            instances[0].onion_address.as_deref(),
            Some("devonion.onion")
        );
    }

    #[tokio::test]
    async fn ai_create_and_publish_integrates_with_ask_instance_resolution() {
        use crate::application::ai_assistant::resolve_instance_endpoint;
        use crate::ports::container::ContainerInfo;

        // Criterio de aceptación Fase 10: una instancia creada por `ai create`
        // (`enola-ai-<name>-ollama` en `127.0.0.1`) es resuelta por `ask --instance <name>`.
        let containers = vec![
            ContainerInfo {
                id: "c-ollama".into(),
                name: "enola-ai-dev-ollama".into(),
                image: "ollama/ollama:latest".into(),
                status: "Up 2 minutes".into(),
                ports: vec!["127.0.0.1:11435->11434/tcp".into()],
            },
            ContainerInfo {
                id: "c-webui".into(),
                name: "enola-ai-dev-webui".into(),
                image: "ghcr.io/open-webui/open-webui:main".into(),
                status: "Up 2 minutes".into(),
                ports: vec!["127.0.0.1:8081->8080/tcp".into()],
            },
        ];

        let endpoint = resolve_instance_endpoint(&containers, Some("dev"))
            .expect("ask --instance dev must resolve the created AI instance");
        assert_eq!(endpoint.as_deref(), Some("http://127.0.0.1:11435"));
    }
}
