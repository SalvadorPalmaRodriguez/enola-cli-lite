use crate::domain::error::Result;
use crate::ports::tor::TorManagerPort;
use crate::ports::tor::TorServiceInfo;
use std::sync::Arc;

/// Use Case: List all active Tor Hidden Services
pub struct ListTorServices {
    tor_manager: Arc<dyn TorManagerPort + Send + Sync>,
}

impl ListTorServices {
    pub fn new(tor_manager: Arc<dyn TorManagerPort + Send + Sync>) -> Self {
        Self { tor_manager }
    }

    pub async fn execute(&self) -> Result<Vec<TorServiceInfo>> {
        // En un mundo ideal, el Port tendría un método `list_services`.
        // Pero el Adapter actual de MVP no parsea el torrc completo para listar.
        // Como workaround para MVP, podemos escanear el directorio /var/lib/tor/
        // Pero eso es detalle de implementación del Adapter.

        // Vamos a asumir que el Port puede listar. Oh espera, en ports/tor.rs
        // NO definí `list_hidden_services`.
        // Necesito actualizar el Port o el UseCase.

        // Revisando TASKS.md Task 6.2: "Leer /var/lib/tor/hidden_service/hostname".
        // Esto implica que necesitamos un metodo en el Port para descubrir servicios. -- YA ESTA IMPLEMENTADO

        self.tor_manager.list_hidden_services().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::error::EnolaError;
    use async_trait::async_trait;

    struct MockTorManager {
        services: Vec<TorServiceInfo>,
        should_fail: bool,
    }

    impl MockTorManager {
        fn new() -> Self {
            Self {
                services: vec![],
                should_fail: false,
            }
        }

        fn with_services(services: Vec<TorServiceInfo>) -> Self {
            Self {
                services,
                should_fail: false,
            }
        }

        fn failing() -> Self {
            Self {
                services: vec![],
                should_fail: true,
            }
        }
    }

    #[async_trait]
    impl TorManagerPort for MockTorManager {
        async fn list_hidden_services(&self) -> Result<Vec<TorServiceInfo>> {
            if self.should_fail {
                Err(EnolaError::InfrastructureError("List failed".into()))
            } else {
                Ok(self.services.clone())
            }
        }
        async fn deploy_hidden_service(&self, _: &str, _: Vec<(u16, u16)>) -> Result<String> {
            Ok("test.onion".into())
        }
        async fn remove_hidden_service(&self, _: &str) -> Result<()> {
            Ok(())
        }
        async fn get_onion_address(&self, _: &str) -> Result<String> {
            Ok("test.onion".into())
        }
        async fn reload_tor(&self) -> Result<()> {
            Ok(())
        }
        async fn generate_client_keys(&self, _: &str) -> Result<(String, String)> {
            Ok(("pub".into(), "priv".into()))
        }
        async fn add_client_auth(&self, _: &str, _: &str, _: &str) -> Result<()> {
            Ok(())
        }
        async fn disable_client_auth(&self, _: &str) -> Result<()> {
            Ok(())
        }
        async fn revoke_client_auth(&self, _: &str, _: &str) -> Result<()> {
            Ok(())
        }
        async fn enable_client_auth(&self, _: &str) -> Result<()> {
            Ok(())
        }
        async fn stop_hidden_service(&self, _: &str) -> Result<()> {
            Ok(())
        }
        async fn start_hidden_service(&self, _: &str) -> Result<()> {
            Ok(())
        }
        async fn rotate_hidden_service_identity(&self, _: &str) -> Result<String> {
            Ok("new.onion".into())
        }
    }

    #[tokio::test]
    async fn test_list_services_empty() {
        let tor = Arc::new(MockTorManager::new());
        let use_case = ListTorServices::new(tor);

        let result = use_case.execute().await;

        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_list_services_with_services() {
        let services = vec![
            TorServiceInfo {
                name: "service1".into(),
                hostname: "abc123.onion".into(),
                hidden_service_dir: "/var/lib/tor/enola_service1".into(),
                ports: vec![(80, "127.0.0.1:8080".into())],
                active: true,
                auth_enabled: false,
                clients: vec![],
            },
            TorServiceInfo {
                name: "service2".into(),
                hostname: "def456.onion".into(),
                hidden_service_dir: "/var/lib/tor/enola_service2".into(),
                ports: vec![(443, "127.0.0.1:8443".into())],
                active: true,
                auth_enabled: true,
                clients: vec!["client1".into()],
            },
        ];
        let tor = Arc::new(MockTorManager::with_services(services));
        let use_case = ListTorServices::new(tor);

        let result = use_case.execute().await;

        assert!(result.is_ok());
        let services = result.unwrap();
        assert_eq!(services.len(), 2);
        assert_eq!(services[0].name, "service1");
        assert_eq!(services[1].name, "service2");
    }

    #[tokio::test]
    async fn test_list_services_failure() {
        let tor = Arc::new(MockTorManager::failing());
        let use_case = ListTorServices::new(tor);

        let result = use_case.execute().await;

        assert!(result.is_err());
    }
}
