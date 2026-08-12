use crate::domain::error::Result;
use crate::ports::tor::TorManagerPort;
use crate::ports::web::{NginxManagerPort, NginxProxyConfig};
use std::sync::Arc;

pub struct TorServiceManager {
    tor_manager: Arc<dyn TorManagerPort + Send + Sync>,
    nginx_manager: Arc<dyn NginxManagerPort + Send + Sync>,
}

impl TorServiceManager {
    pub fn new(
        tor_manager: Arc<dyn TorManagerPort + Send + Sync>,
        nginx_manager: Arc<dyn NginxManagerPort + Send + Sync>,
    ) -> Self {
        Self {
            tor_manager,
            nginx_manager,
        }
    }

    /// Creates a Tor hidden service and configures Nginx reverse proxy
    /// name: Service name (e.g. "git", "wordpress")
    /// onion_port: Public port on .onion (e.g. 80)
    /// target_port: Destination port where the app is running (e.g. 3000)
    /// ssl: Whether to configure SSL (not yet fully implemented in this sketch)
    pub async fn create_hidden_service(
        &self,
        name: &str,
        onion_port: u16,
        target_port: u16,
        _ssl: bool,
    ) -> Result<String> {
        // 1. Determine Nginx listen port (Tor -> Nginx -> App)
        // For simplicity, we might assume Tor talks to Nginx on a specific port.
        // Or we might dynamically assign one.
        // For now, let's assume we use a convention or the user provides it.
        // But the prompt signature implies we figure it out.

        // However, looking at existing DeployTorWebService, it takes `nginx_port`.
        // Let's assume for now 127.0.0.1:8080 range for internal Nginx binding?
        // Or we can just bind to the same as target_port if Nginx is main entry?
        // No, Nginx is proxy.
        // Let's find an available port for Nginx to listen on.

        let nginx_listen_port = self.nginx_manager.find_available_port(8000, 9000).await?;

        // 2. Deploy Hidden Service pointing to Nginx
        // Tor map: onion_port -> 127.0.0.1:nginx_listen_port
        let ports_map = vec![(onion_port, nginx_listen_port)];
        let onion_addr = self
            .tor_manager
            .deploy_hidden_service(name, ports_map)
            .await?;

        // 3. Configure Nginx to proxy to target_port
        let nginx_config = NginxProxyConfig {
            service_name: name.to_string(),
            listen_port: nginx_listen_port,
            backend_port: target_port,
            server_name: "localhost".to_string(),
            rate_limit: None,
        };

        self.nginx_manager.create_proxy_config(nginx_config).await?;

        Ok(onion_addr)
    }

    /// Expose service (Start Tor + Enable Nginx)
    pub async fn expose_service(&self, name: &str) -> Result<()> {
        self.tor_manager.start_hidden_service(name).await?;
        self.nginx_manager.enable_site(name).await?;
        self.nginx_manager.reload().await?;
        self.tor_manager.reload_tor().await?;
        Ok(())
    }

    /// Hide service (Stop Tor + Disable Nginx)
    pub async fn hide_service(&self, name: &str) -> Result<()> {
        // Ignore errors if already stopped?
        let _ = self.tor_manager.stop_hidden_service(name).await;
        let _ = self.nginx_manager.disable_site(name).await;

        // Reloads
        let _ = self.nginx_manager.reload().await;
        // Tor reload might not be strictly necessary if we just commented out lines, but good practice
        let _ = self.tor_manager.reload_tor().await;

        Ok(())
    }

    /// Delete service
    pub async fn delete_service(&self, name: &str) -> Result<()> {
        self.hide_service(name).await?;
        self.tor_manager.remove_hidden_service(name).await?;
        self.nginx_manager.delete_site_config(name).await?;
        Ok(())
    }

    /// Edit service ports
    pub async fn edit_service_ports(
        &self,
        name: &str,
        onion_port: u16,
        target_port: u16,
    ) -> Result<()> {
        // 1. Get current config or infer?
        // This is tricky without state. But Nginx config exists.

        // For Tor, we redeploy.
        let nginx_listen_port = self.nginx_manager.find_available_port(8000, 9000).await?; // Or try to keep existing?

        // Update Tor
        let ports_map = vec![(onion_port, nginx_listen_port)];
        self.tor_manager
            .deploy_hidden_service(name, ports_map)
            .await?;

        // Update Nginx
        self.nginx_manager
            .update_proxy_ports(name, nginx_listen_port, target_port)
            .await?;

        // Reload
        self.nginx_manager.reload().await?;
        self.tor_manager.reload_tor().await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::tor::MockTorManagerPort;
    use crate::ports::web::MockNginxManagerPort;
    use mockall::predicate::*;

    #[tokio::test]
    async fn test_create_hidden_service() {
        let mut mock_tor = MockTorManagerPort::new();
        let mut mock_nginx = MockNginxManagerPort::new();

        // Expect finding an available port
        mock_nginx
            .expect_find_available_port()
            .with(eq(8000), eq(9000))
            .times(1)
            .returning(|_, _| Ok(8080));

        // Expect deploying hidden service
        mock_tor
            .expect_deploy_hidden_service()
            .with(eq("test_service"), eq(vec![(80, 8080)]))
            .times(1)
            .returning(|_, _| Ok("testaddress.onion".to_string()));

        // Expect creating proxy config
        mock_nginx
            .expect_create_proxy_config()
            .withf(|config| {
                config.service_name == "test_service"
                    && config.listen_port == 8080
                    && config.backend_port == 3000
            })
            .times(1)
            .returning(|_| Ok(()));

        let manager = TorServiceManager::new(Arc::new(mock_tor), Arc::new(mock_nginx));

        let result = manager
            .create_hidden_service("test_service", 80, 3000, false)
            .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "testaddress.onion");
    }

    #[tokio::test]
    async fn test_expose_service() {
        let mut mock_tor = MockTorManagerPort::new();
        let mut mock_nginx = MockNginxManagerPort::new();

        mock_tor
            .expect_start_hidden_service()
            .with(eq("test_service"))
            .times(1)
            .returning(|_| Ok(()));

        mock_nginx
            .expect_enable_site()
            .with(eq("test_service"))
            .times(1)
            .returning(|_| Ok(()));

        mock_nginx.expect_reload().times(1).returning(|| Ok(()));
        mock_tor.expect_reload_tor().times(1).returning(|| Ok(()));

        let manager = TorServiceManager::new(Arc::new(mock_tor), Arc::new(mock_nginx));

        let result = manager.expose_service("test_service").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_hide_service() {
        let mut mock_tor = MockTorManagerPort::new();
        let mut mock_nginx = MockNginxManagerPort::new();

        mock_tor
            .expect_stop_hidden_service()
            .with(eq("test_service"))
            .times(1)
            .returning(|_| Ok(()));

        mock_nginx
            .expect_disable_site()
            .with(eq("test_service"))
            .times(1)
            .returning(|_| Ok(()));

        mock_nginx.expect_reload().times(1).returning(|| Ok(()));
        mock_tor.expect_reload_tor().times(1).returning(|| Ok(()));

        let manager = TorServiceManager::new(Arc::new(mock_tor), Arc::new(mock_nginx));

        let result = manager.hide_service("test_service").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_create_hidden_service_failure_no_port() {
        let mock_tor = MockTorManagerPort::new();
        let mut mock_nginx = MockNginxManagerPort::new();

        mock_nginx
            .expect_find_available_port()
            .with(eq(8000), eq(9000))
            .times(1)
            .returning(|_, _| {
                Err(crate::domain::error::EnolaError::InfrastructureError(
                    "No ports".into(),
                ))
            });

        let manager = TorServiceManager::new(Arc::new(mock_tor), Arc::new(mock_nginx));

        let result = manager
            .create_hidden_service("test_service", 80, 3000, false)
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_create_hidden_service_tor_failure() {
        let mut mock_tor = MockTorManagerPort::new();
        let mut mock_nginx = MockNginxManagerPort::new();

        mock_nginx
            .expect_find_available_port()
            .returning(|_, _| Ok(8080));

        mock_tor.expect_deploy_hidden_service().returning(|_, _| {
            Err(crate::domain::error::EnolaError::InfrastructureError(
                "Tor failed".into(),
            ))
        });

        let manager = TorServiceManager::new(Arc::new(mock_tor), Arc::new(mock_nginx));

        let result = manager
            .create_hidden_service("test_service", 80, 3000, false)
            .await;

        assert!(result.is_err());
    }
}
