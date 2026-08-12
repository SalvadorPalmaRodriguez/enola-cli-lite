use crate::domain::error::{EnolaError, Result};
use crate::ports::tor::TorManagerPort;
use std::sync::Arc;
pub struct ManageClientAuth {
    tor_manager: Arc<dyn TorManagerPort + Send + Sync>,
}
impl ManageClientAuth {
    pub fn new(tor_manager: Arc<dyn TorManagerPort + Send + Sync>) -> Self {
        Self { tor_manager }
    }
    pub async fn add_client(
        &self,
        service_name: &str,
        client_name: &str,
        public_key: &str,
    ) -> Result<()> {
        if client_name.is_empty() {
            return Err(EnolaError::ValidationError(
                "Client name cannot be empty".to_string(),
            ));
        }
        if public_key.len() != 52 {
            // Tor x25519 base32 keys are 52 chars
            return Err(EnolaError::ValidationError(
                "Invalid key length. Must be 52 chars base32".to_string(),
            ));
        }
        // Ensure auth is enabled
        self.tor_manager.enable_client_auth(service_name).await?;
        self.tor_manager
            .add_client_auth(service_name, client_name, public_key)
            .await
    }
    pub async fn list_clients(&self, service_name: &str) -> Result<Vec<String>> {
        let services = self.tor_manager.list_hidden_services().await?;
        let service = services
            .into_iter()
            .find(|s| s.name == service_name)
            .ok_or_else(|| EnolaError::NotFound(format!("Service {} not found", service_name)))?;
        Ok(service.clients)
    }
    pub async fn toggle_auth(&self, service_name: &str, enable: bool) -> Result<()> {
        if enable {
            self.tor_manager.enable_client_auth(service_name).await
        } else {
            self.tor_manager.disable_client_auth(service_name).await
        }
    }
    pub async fn revoke_client(&self, service_name: &str, client_name: &str) -> Result<()> {
        self.tor_manager
            .revoke_client_auth(service_name, client_name)
            .await
    }
    pub async fn generate_keys(&self, client_name: &str) -> Result<(String, String)> {
        self.tor_manager.generate_client_keys(client_name).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::error::EnolaError;
    use crate::ports::tor::TorServiceInfo;
    use async_trait::async_trait;
    use std::sync::Mutex;

    struct MockTorManager {
        services: Vec<TorServiceInfo>,
        should_fail: bool,
        enable_auth_called: Mutex<bool>,
    }

    impl MockTorManager {
        fn new() -> Self {
            Self {
                services: vec![],
                should_fail: false,
                enable_auth_called: Mutex::new(false),
            }
        }

        fn with_services(services: Vec<TorServiceInfo>) -> Self {
            Self {
                services,
                should_fail: false,
                enable_auth_called: Mutex::new(false),
            }
        }

        fn failing() -> Self {
            Self {
                services: vec![],
                should_fail: true,
                enable_auth_called: Mutex::new(false),
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
            if self.should_fail {
                Err(EnolaError::InfrastructureError("Generate failed".into()))
            } else {
                Ok((
                    "PRIVKEY1234567890123456789012345678901234567890AB".into(),
                    "PUBKEY12345678901234567890123456789012345678901234".into(),
                ))
            }
        }
        async fn add_client_auth(&self, _: &str, _: &str, _: &str) -> Result<()> {
            if self.should_fail {
                Err(EnolaError::InfrastructureError("Add failed".into()))
            } else {
                Ok(())
            }
        }
        async fn disable_client_auth(&self, _: &str) -> Result<()> {
            Ok(())
        }
        async fn revoke_client_auth(&self, _: &str, _: &str) -> Result<()> {
            if self.should_fail {
                Err(EnolaError::InfrastructureError("Revoke failed".into()))
            } else {
                Ok(())
            }
        }
        async fn enable_client_auth(&self, _: &str) -> Result<()> {
            *self.enable_auth_called.lock().unwrap() = true;
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
    async fn test_add_client_success() {
        let tor = Arc::new(MockTorManager::new());
        let use_case = ManageClientAuth::new(tor.clone());
        // Valid 52 char base32 key
        let key = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567ABCDEFGHIJKLMNOPQRST";

        let result = use_case.add_client("myservice", "client1", key).await;

        assert!(result.is_ok());
        assert!(*tor.enable_auth_called.lock().unwrap());
    }

    #[tokio::test]
    async fn test_add_client_empty_name() {
        let tor = Arc::new(MockTorManager::new());
        let use_case = ManageClientAuth::new(tor);
        let key = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567ABCDEFGHIJKLMNOPQRST";

        let result = use_case.add_client("myservice", "", key).await;

        assert!(result.is_err());
        match result {
            Err(EnolaError::ValidationError(msg)) => {
                assert!(msg.contains("empty"));
            }
            _ => panic!("Expected ValidationError"),
        }
    }

    #[tokio::test]
    async fn test_add_client_invalid_key_length() {
        let tor = Arc::new(MockTorManager::new());
        let use_case = ManageClientAuth::new(tor);
        let key = "TOOSHORT";

        let result = use_case.add_client("myservice", "client1", key).await;

        assert!(result.is_err());
        match result {
            Err(EnolaError::ValidationError(msg)) => {
                assert!(msg.contains("52 chars"));
            }
            _ => panic!("Expected ValidationError"),
        }
    }

    #[tokio::test]
    async fn test_list_clients_success() {
        let services = vec![TorServiceInfo {
            name: "myservice".into(),
            hostname: "abc.onion".into(),
            hidden_service_dir: "/var/lib/tor/enola_myservice".into(),
            ports: vec![(80, "127.0.0.1:8080".into())],
            active: true,
            auth_enabled: true,
            clients: vec!["client1".into(), "client2".into()],
        }];
        let tor = Arc::new(MockTorManager::with_services(services));
        let use_case = ManageClientAuth::new(tor);

        let result = use_case.list_clients("myservice").await;

        assert!(result.is_ok());
        let clients = result.unwrap();
        assert_eq!(clients.len(), 2);
        assert_eq!(clients[0], "client1");
        assert_eq!(clients[1], "client2");
    }

    #[tokio::test]
    async fn test_list_clients_service_not_found() {
        let tor = Arc::new(MockTorManager::new());
        let use_case = ManageClientAuth::new(tor);

        let result = use_case.list_clients("nonexistent").await;

        assert!(result.is_err());
        match result {
            Err(EnolaError::NotFound(msg)) => {
                assert!(msg.contains("nonexistent"));
            }
            _ => panic!("Expected NotFound"),
        }
    }

    #[tokio::test]
    async fn test_toggle_auth_enable() {
        let tor = Arc::new(MockTorManager::new());
        let use_case = ManageClientAuth::new(tor.clone());

        let result = use_case.toggle_auth("myservice", true).await;

        assert!(result.is_ok());
        assert!(*tor.enable_auth_called.lock().unwrap());
    }

    #[tokio::test]
    async fn test_toggle_auth_disable() {
        let tor = Arc::new(MockTorManager::new());
        let use_case = ManageClientAuth::new(tor);

        let result = use_case.toggle_auth("myservice", false).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_revoke_client_success() {
        let tor = Arc::new(MockTorManager::new());
        let use_case = ManageClientAuth::new(tor);

        let result = use_case.revoke_client("myservice", "client1").await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_revoke_client_failure() {
        let tor = Arc::new(MockTorManager::failing());
        let use_case = ManageClientAuth::new(tor);

        let result = use_case.revoke_client("myservice", "client1").await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_generate_keys_success() {
        let tor = Arc::new(MockTorManager::new());
        let use_case = ManageClientAuth::new(tor);

        let result = use_case.generate_keys("client1").await;

        assert!(result.is_ok());
        let (priv_key, pub_key) = result.unwrap();
        assert!(!priv_key.is_empty());
        assert!(!pub_key.is_empty());
    }

    #[tokio::test]
    async fn test_generate_keys_failure() {
        let tor = Arc::new(MockTorManager::failing());
        let use_case = ManageClientAuth::new(tor);

        let result = use_case.generate_keys("client1").await;

        assert!(result.is_err());
    }
}
