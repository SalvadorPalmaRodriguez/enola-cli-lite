use crate::domain::error::Result;
pub use crate::domain::tor::TorServiceInfo;

#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait TorManagerPort {
    /// Creates or updates a Tor Hidden Service configuration
    async fn deploy_hidden_service(&self, name: &str, ports: Vec<(u16, u16)>) -> Result<String>;

    /// Removes a hidden service
    async fn remove_hidden_service(&self, name: &str) -> Result<()>;

    /// formatted as xxxxxxxxx.onion
    async fn get_onion_address(&self, name: &str) -> Result<String>;

    /// Reloads Tor configuration to apply changes
    async fn reload_tor(&self) -> Result<()>;

    /// Generate new client x25519 authorization keys
    async fn generate_client_keys(&self, client_name: &str) -> Result<(String, String)>; // (Priv, Pub)

    /// Add client authorization to a service
    async fn add_client_auth(
        &self,
        service_name: &str,
        client_name: &str,
        public_key: &str,
    ) -> Result<()>;

    /// Revoke client authorization from a service
    /// Disable client authorization (removes or renames directory)
    async fn disable_client_auth(&self, service_name: &str) -> Result<()>;

    async fn revoke_client_auth(&self, service_name: &str, client_name: &str) -> Result<()>;

    /// Enable client authorization (creates necessary directory)
    async fn enable_client_auth(&self, service_name: &str) -> Result<()>;

    /// List all configured hidden services
    async fn list_hidden_services(&self) -> Result<Vec<TorServiceInfo>>;

    /// Stop a hidden service temporarily (disable config)
    async fn stop_hidden_service(&self, name: &str) -> Result<()>;

    /// Start a hidden service (enable config)
    async fn start_hidden_service(&self, name: &str) -> Result<()>;

    /// Rotate Identity (Regenerate Onion Address), preserving clients
    async fn rotate_hidden_service_identity(&self, name: &str) -> Result<String>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_tor_deploy_hidden_service() {
        let mut mock = MockTorManagerPort::new();
        mock.expect_deploy_hidden_service()
            .returning(|_, _| Ok("abc.onion".into()));
        let result = mock.deploy_hidden_service("svc", vec![(80, 8080)]).await;
        assert_eq!(result.unwrap(), "abc.onion");
    }

    #[tokio::test]
    async fn test_mock_tor_reload() {
        let mut mock = MockTorManagerPort::new();
        mock.expect_reload_tor().returning(|| Ok(()));
        assert!(mock.reload_tor().await.is_ok());
    }
}
