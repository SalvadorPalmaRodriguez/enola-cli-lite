use crate::domain::error::{EnolaError, Result};
use crate::ports::manifest::ManifestPort;
use crate::ports::service::ServiceManagerPort;
use crate::ports::tor::TorManagerPort;
use std::sync::Arc;

/// Input for DeployTorService
pub struct DeployTorServiceRequest {
    pub service_name: String,
    pub ports: Vec<(u16, u16)>, // (Public, Target)
}

/// Use Case: Deploy a new Tor Hidden Service
pub struct DeployTorService {
    tor_manager: Arc<dyn TorManagerPort + Send + Sync>,
    service_manager: Arc<dyn ServiceManagerPort + Send + Sync>,
    manifest: Arc<dyn ManifestPort + Send + Sync>,
}

impl DeployTorService {
    pub fn new(
        tor_manager: Arc<dyn TorManagerPort + Send + Sync>,
        service_manager: Arc<dyn ServiceManagerPort + Send + Sync>,
        manifest: Arc<dyn ManifestPort + Send + Sync>,
    ) -> Self {
        Self {
            tor_manager,
            service_manager,
            manifest,
        }
    }

    pub async fn execute(&self, request: DeployTorServiceRequest) -> Result<String> {
        // 1. Validate Input
        if request.service_name.is_empty() {
            return Err(EnolaError::ValidationError(
                "Service name cannot be empty".to_string(),
            ));
        }
        if request.ports.is_empty() {
            return Err(EnolaError::ValidationError(
                "At least one port mapping is required".to_string(),
            ));
        }

        // 2. Ensure Tor Service is running (Systemd)
        if !self.service_manager.is_active("tor").await? {
            // Attempt to start if not active
            self.service_manager.start_service("tor").await?;
            // Wait a bit? Assuming start_service waits for command completion.
        }

        // 3. Deploy Hidden Service Config
        let onion_address = self
            .tor_manager
            .deploy_hidden_service(&request.service_name, request.ports)
            .await?;
        let _ = self.manifest.append("tor_service", &request.service_name);

        Ok(onion_address)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::manifest::MockManifestPort;
    use crate::ports::tor::TorServiceInfo;
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicBool, Ordering};

    fn mock_manifest() -> MockManifestPort {
        let mut m = MockManifestPort::new();
        m.expect_append().returning(|_, _| Ok(())).times(0..);
        m.expect_remove().returning(|_, _| Ok(())).times(0..);
        m
    }

    struct MockTorManager {
        deploy_called: AtomicBool,
        should_fail: bool,
    }

    impl MockTorManager {
        fn new() -> Self {
            Self {
                deploy_called: AtomicBool::new(false),
                should_fail: false,
            }
        }

        fn failing() -> Self {
            Self {
                deploy_called: AtomicBool::new(false),
                should_fail: true,
            }
        }
    }

    #[async_trait]
    impl TorManagerPort for MockTorManager {
        async fn list_hidden_services(&self) -> Result<Vec<TorServiceInfo>> {
            Ok(vec![])
        }
        async fn deploy_hidden_service(&self, _: &str, _: Vec<(u16, u16)>) -> Result<String> {
            self.deploy_called.store(true, Ordering::SeqCst);
            if self.should_fail {
                Err(EnolaError::InfrastructureError("Deploy failed".into()))
            } else {
                Ok("abc123xyz.onion".to_string())
            }
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

    struct MockServiceManager {
        is_active: bool,
        start_called: AtomicBool,
    }

    impl MockServiceManager {
        fn active() -> Self {
            Self {
                is_active: true,
                start_called: AtomicBool::new(false),
            }
        }
        fn inactive() -> Self {
            Self {
                is_active: false,
                start_called: AtomicBool::new(false),
            }
        }
    }

    #[async_trait]
    impl ServiceManagerPort for MockServiceManager {
        async fn start_service(&self, _: &str) -> Result<()> {
            self.start_called.store(true, Ordering::SeqCst);
            Ok(())
        }
        async fn stop_service(&self, _: &str) -> Result<()> {
            Ok(())
        }
        async fn restart_service(&self, _: &str) -> Result<()> {
            Ok(())
        }
        async fn enable_service(&self, _: &str) -> Result<()> {
            Ok(())
        }
        async fn disable_service(&self, _: &str) -> Result<()> {
            Ok(())
        }
        async fn is_active(&self, _: &str) -> Result<bool> {
            Ok(self.is_active)
        }
        async fn get_service_metrics(
            &self,
            _: &str,
        ) -> Result<crate::ports::service::ServiceMetrics> {
            Ok(crate::ports::service::ServiceMetrics::default())
        }
    }

    #[tokio::test]
    async fn test_deploy_tor_service_success() {
        let tor = Arc::new(MockTorManager::new());
        let svc = Arc::new(MockServiceManager::active());
        let use_case = DeployTorService::new(tor.clone(), svc, Arc::new(mock_manifest()));

        let result = use_case
            .execute(DeployTorServiceRequest {
                service_name: "myservice".into(),
                ports: vec![(80, 8080)],
            })
            .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "abc123xyz.onion");
        assert!(tor.deploy_called.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn test_deploy_tor_service_starts_tor_if_inactive() {
        let tor = Arc::new(MockTorManager::new());
        let svc = Arc::new(MockServiceManager::inactive());
        let use_case = DeployTorService::new(tor, svc.clone(), Arc::new(mock_manifest()));

        let result = use_case
            .execute(DeployTorServiceRequest {
                service_name: "myservice".into(),
                ports: vec![(80, 8080)],
            })
            .await;

        assert!(result.is_ok());
        assert!(svc.start_called.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn test_deploy_tor_service_empty_name_error() {
        let tor = Arc::new(MockTorManager::new());
        let svc = Arc::new(MockServiceManager::active());
        let use_case = DeployTorService::new(tor, svc, Arc::new(mock_manifest()));

        let result = use_case
            .execute(DeployTorServiceRequest {
                service_name: "".into(),
                ports: vec![(80, 8080)],
            })
            .await;

        assert!(result.is_err());
        match result.unwrap_err() {
            EnolaError::ValidationError(msg) => assert!(msg.contains("empty")),
            _ => panic!("Expected ValidationError"),
        }
    }

    #[tokio::test]
    async fn test_deploy_tor_service_empty_ports_error() {
        let tor = Arc::new(MockTorManager::new());
        let svc = Arc::new(MockServiceManager::active());
        let use_case = DeployTorService::new(tor, svc, Arc::new(mock_manifest()));

        let result = use_case
            .execute(DeployTorServiceRequest {
                service_name: "myservice".into(),
                ports: vec![],
            })
            .await;

        assert!(result.is_err());
        match result.unwrap_err() {
            EnolaError::ValidationError(msg) => assert!(msg.contains("port")),
            _ => panic!("Expected ValidationError"),
        }
    }

    #[tokio::test]
    async fn test_deploy_tor_service_tor_failure() {
        let tor = Arc::new(MockTorManager::failing());
        let svc = Arc::new(MockServiceManager::active());
        let use_case = DeployTorService::new(tor, svc, Arc::new(mock_manifest()));

        let result = use_case
            .execute(DeployTorServiceRequest {
                service_name: "myservice".into(),
                ports: vec![(80, 8080)],
            })
            .await;

        assert!(result.is_err());
    }
}
