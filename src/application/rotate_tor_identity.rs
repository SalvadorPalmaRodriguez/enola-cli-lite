use crate::domain::error::Result;
use crate::ports::tor::TorManagerPort;
use std::sync::Arc;

pub struct RotateTorIdentity {
    tor_manager: Arc<dyn TorManagerPort + Send + Sync>,
}

impl RotateTorIdentity {
    pub fn new(tor_manager: Arc<dyn TorManagerPort + Send + Sync>) -> Self {
        Self { tor_manager }
    }

    pub async fn execute(&self, service_name: &str) -> Result<String> {
        self.tor_manager
            .rotate_hidden_service_identity(service_name)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::error::EnolaError;
    use crate::ports::tor::TorServiceInfo;
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicBool, Ordering};

    struct MockTorManager {
        rotate_called: AtomicBool,
        should_fail: bool,
        new_address: String,
    }

    impl MockTorManager {
        fn new() -> Self {
            Self {
                rotate_called: AtomicBool::new(false),
                should_fail: false,
                new_address: "newaddress123.onion".to_string(),
            }
        }

        fn failing() -> Self {
            Self {
                rotate_called: AtomicBool::new(false),
                should_fail: true,
                new_address: String::new(),
            }
        }
    }

    #[async_trait]
    impl TorManagerPort for MockTorManager {
        async fn list_hidden_services(&self) -> Result<Vec<TorServiceInfo>> {
            Ok(vec![])
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
            self.rotate_called.store(true, Ordering::SeqCst);
            if self.should_fail {
                Err(EnolaError::InfrastructureError("Rotate failed".into()))
            } else {
                Ok(self.new_address.clone())
            }
        }
    }

    #[tokio::test]
    async fn test_rotate_identity_success() {
        let tor = Arc::new(MockTorManager::new());
        let use_case = RotateTorIdentity::new(tor.clone());

        let result = use_case.execute("myservice").await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "newaddress123.onion");
        assert!(tor.rotate_called.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn test_rotate_identity_failure() {
        let tor = Arc::new(MockTorManager::failing());
        let use_case = RotateTorIdentity::new(tor);

        let result = use_case.execute("myservice").await;

        assert!(result.is_err());
    }
}
