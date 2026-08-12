use crate::domain::error::Result;
use crate::ports::container::ContainerPort;
use crate::ports::tor::TorManagerPort;
use crate::ports::web::NginxManagerPort;
use std::sync::Arc;

pub struct EditPortConfig {
    tor_manager: Arc<dyn TorManagerPort + Send + Sync>,
    nginx_manager: Option<Arc<dyn NginxManagerPort + Send + Sync>>,
    container_manager: Option<Arc<dyn ContainerPort + Send + Sync>>,
}

impl EditPortConfig {
    pub fn new(
        tor_manager: Arc<dyn TorManagerPort + Send + Sync>,
        nginx_manager: Option<Arc<dyn NginxManagerPort + Send + Sync>>,
    ) -> Self {
        Self {
            tor_manager,
            nginx_manager,
            container_manager: None,
        }
    }

    pub fn with_container_manager(
        mut self,
        container_manager: Arc<dyn ContainerPort + Send + Sync>,
    ) -> Self {
        self.container_manager = Some(container_manager);
        self
    }

    pub async fn execute(
        &self,
        service_name: &str,
        onion_port: u16,
        nginx_listen_port: u16,
        backend_port: u16,
    ) -> Result<()> {
        // 1. Update Tor Configuration
        // Map Onion Port -> Nginx Listen Port (Always local)
        let ports = vec![(onion_port, nginx_listen_port)];
        self.tor_manager
            .deploy_hidden_service(service_name, ports)
            .await?;
        self.tor_manager.reload_tor().await?;

        // 2. Update Nginx Configuration
        if let Some(nginx) = &self.nginx_manager {
            nginx
                .update_proxy_ports(service_name, nginx_listen_port, backend_port)
                .await?;
        }

        // 3. Handle WordPress Container recreation if necessary
        if let Some(container) = &self.container_manager {
            let wordpress_container = format!("wp-{}", service_name);
            let containers = container.list_containers(true).await?;

            // Check if this is a WordPress service by looking for the container
            let is_wordpress = containers.iter().any(|c| c.name == wordpress_container);

            if is_wordpress {
                // Restart the WordPress container to pick up any network changes
                // Note: Port changes in WordPress typically require container recreation
                // but for simple proxy port changes, a restart should suffice
                if container
                    .restart_container(&wordpress_container)
                    .await
                    .is_err()
                {
                    // Container might not be running, try to start it
                    let _ = container.start_container(&wordpress_container).await;
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::error::EnolaError;
    use crate::ports::container::{ContainerConfig, ContainerInfo};
    use crate::ports::tor::TorServiceInfo;
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicBool, Ordering};

    // Mock TorManagerPort
    struct MockTorManager {
        deploy_called: AtomicBool,
        reload_called: AtomicBool,
        should_fail: bool,
    }

    impl MockTorManager {
        fn new() -> Self {
            Self {
                deploy_called: AtomicBool::new(false),
                reload_called: AtomicBool::new(false),
                should_fail: false,
            }
        }

        fn with_failure() -> Self {
            Self {
                deploy_called: AtomicBool::new(false),
                reload_called: AtomicBool::new(false),
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
                Err(EnolaError::InfrastructureError("Mock failure".into()))
            } else {
                Ok("test.onion".to_string())
            }
        }
        async fn remove_hidden_service(&self, _: &str) -> Result<()> {
            Ok(())
        }
        async fn get_onion_address(&self, _: &str) -> Result<String> {
            Ok("test.onion".into())
        }
        async fn reload_tor(&self) -> Result<()> {
            self.reload_called.store(true, Ordering::SeqCst);
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

    // Mock NginxManagerPort
    struct MockNginxManager {
        update_called: AtomicBool,
        should_fail_proxy: bool,
    }

    impl MockNginxManager {
        fn new() -> Self {
            Self {
                update_called: AtomicBool::new(false),
                should_fail_proxy: false,
            }
        }

        fn with_proxy_failure() -> Self {
            Self {
                update_called: AtomicBool::new(false),
                should_fail_proxy: true,
            }
        }
    }

    #[async_trait]
    impl NginxManagerPort for MockNginxManager {
        async fn create_site_config(&self, _: crate::ports::web::NginxSiteConfig) -> Result<()> {
            Ok(())
        }
        async fn create_fileserver_config(
            &self,
            _: crate::ports::web::NginxFileServerConfig,
        ) -> Result<()> {
            Ok(())
        }
        async fn create_proxy_config(&self, _: crate::ports::web::NginxProxyConfig) -> Result<()> {
            Ok(())
        }
        async fn enable_site(&self, _: &str) -> Result<()> {
            Ok(())
        }
        async fn disable_site(&self, _: &str) -> Result<()> {
            Ok(())
        }
        async fn delete_site_config(&self, _: &str) -> Result<()> {
            Ok(())
        }
        async fn validate_config(&self) -> Result<bool> {
            Ok(true)
        }
        async fn reload(&self) -> Result<()> {
            Ok(())
        }
        async fn update_proxy_ports(&self, _domain: &str, _: u16, _: u16) -> Result<()> {
            self.update_called.store(true, Ordering::SeqCst);
            if self.should_fail_proxy {
                Err(EnolaError::InfrastructureError(
                    "Proxy config not found".into(),
                ))
            } else {
                Ok(())
            }
        }
        async fn list_enabled_sites(&self) -> Result<Vec<String>> {
            Ok(vec![])
        }

        async fn create_proxy_config_with_ssl(
            &self,
            _config: crate::ports::web::NginxProxyConfigWithSsl,
        ) -> Result<()> {
            Ok(())
        }

        async fn generate_self_signed_cert(&self, _service_name: &str) -> Result<(String, String)> {
            Ok(("cert.pem".to_string(), "key.pem".to_string()))
        }

        async fn update_proxy_ports_with_ssl(
            &self,
            _domain: &str,
            _http_listen_port: u16,
            _https_listen_port: Option<u16>,
            _backend_port: u16,
        ) -> Result<()> {
            Ok(())
        }

        async fn find_available_port(&self, _range_start: u16, _range_end: u16) -> Result<u16> {
            Ok(8080)
        }

        async fn is_port_available(&self, _port: u16) -> bool {
            true
        }
    }

    // Mock ContainerPort
    struct MockContainerManager {
        containers: Vec<ContainerInfo>,
        restart_called: AtomicBool,
    }

    impl MockContainerManager {
        fn new() -> Self {
            Self {
                containers: vec![],
                restart_called: AtomicBool::new(false),
            }
        }

        fn with_wordpress(service_name: &str) -> Self {
            Self {
                containers: vec![ContainerInfo {
                    id: "container123".into(),
                    name: format!("wp-{}", service_name),
                    image: "wordpress:latest".into(),
                    status: "running".into(),
                    ports: vec!["80:80".into()],
                }],
                restart_called: AtomicBool::new(false),
            }
        }
    }

    #[async_trait]
    impl ContainerPort for MockContainerManager {
        async fn list_containers(&self, _all: bool) -> Result<Vec<ContainerInfo>> {
            Ok(self.containers.clone())
        }
        async fn create_container(&self, _: ContainerConfig) -> Result<String> {
            Ok("new_container".into())
        }
        async fn start_container(&self, _: &str) -> Result<()> {
            Ok(())
        }
        async fn stop_container(&self, _: &str) -> Result<()> {
            Ok(())
        }
        async fn remove_container(&self, _: &str) -> Result<()> {
            Ok(())
        }
        async fn restart_container(&self, _: &str) -> Result<()> {
            self.restart_called.store(true, Ordering::SeqCst);
            Ok(())
        }
        async fn get_logs(&self, _: &str, _: usize) -> Result<String> {
            Ok("logs".into())
        }
        async fn inspect_container(&self, _: &str) -> Result<HashMap<String, String>> {
            Ok(HashMap::new())
        }
        async fn execute_command(&self, _: &str, _: Vec<String>) -> Result<String> {
            Ok("output".into())
        }
        async fn create_network(&self, _: &str) -> Result<()> {
            Ok(())
        }
        async fn remove_network(&self, _: &str) -> Result<()> {
            Ok(())
        }
        async fn connect_container_to_network(&self, _: &str, _: &str) -> Result<()> {
            Ok(())
        }
        async fn image_exists(&self, _: &str) -> Result<bool> {
            Ok(true)
        }
        async fn build_image(
            &self,
            _: crate::ports::container::ImageBuildConfig,
        ) -> Result<String> {
            Ok("mock:latest".into())
        }
        async fn run_ephemeral_container(&self, _: ContainerConfig) -> Result<(i64, String)> {
            Ok((0, "success".into()))
        }
        async fn prune_system(&self) -> Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_edit_port_config_basic() {
        let tor_manager = Arc::new(MockTorManager::new());
        let config = EditPortConfig::new(tor_manager.clone(), None);

        let result = config.execute("test_service", 80, 8080, 3000).await;
        assert!(result.is_ok());
        assert!(tor_manager.deploy_called.load(Ordering::SeqCst));
        assert!(tor_manager.reload_called.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn test_edit_port_config_with_nginx() {
        let tor_manager = Arc::new(MockTorManager::new());
        let nginx_manager = Arc::new(MockNginxManager::new());
        let config = EditPortConfig::new(tor_manager, Some(nginx_manager.clone()));

        let result = config.execute("test_service", 80, 8080, 3000).await;
        assert!(result.is_ok());
        assert!(nginx_manager.update_called.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn test_edit_port_config_nginx_failure() {
        let tor_manager = Arc::new(MockTorManager::new());
        let nginx_manager = Arc::new(MockNginxManager::with_proxy_failure());
        let config = EditPortConfig::new(tor_manager, Some(nginx_manager.clone()));

        // Should fail because nginx update fails
        let result = config.execute("test_service", 80, 8080, 3000).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_edit_port_config_with_wordpress_container() {
        let tor_manager = Arc::new(MockTorManager::new());
        let container_manager = Arc::new(MockContainerManager::with_wordpress("myblog"));

        let config = EditPortConfig::new(tor_manager, None)
            .with_container_manager(container_manager.clone());

        let result = config.execute("myblog", 80, 8080, 3000).await;
        assert!(result.is_ok());
        assert!(container_manager.restart_called.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn test_edit_port_config_no_wordpress_container() {
        let tor_manager = Arc::new(MockTorManager::new());
        let container_manager = Arc::new(MockContainerManager::new());

        let config = EditPortConfig::new(tor_manager, None)
            .with_container_manager(container_manager.clone());

        let result = config.execute("other_service", 80, 8080, 3000).await;
        assert!(result.is_ok());
        // No restart should be called for non-wordpress service
        assert!(!container_manager.restart_called.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn test_edit_port_config_tor_failure() {
        let tor_manager = Arc::new(MockTorManager::with_failure());
        let config = EditPortConfig::new(tor_manager, None);

        let result = config.execute("test_service", 80, 8080, 3000).await;
        assert!(result.is_err());
    }
}
