use crate::domain::error::Result;
use crate::ports::tor::TorManagerPort;
use crate::ports::web::NginxManagerPort;
use std::sync::Arc;

pub struct StartTorService {
    tor_manager: Arc<dyn TorManagerPort + Send + Sync>,
    nginx_manager: Option<Arc<dyn NginxManagerPort + Send + Sync>>,
}

impl StartTorService {
    pub fn new(
        tor_manager: Arc<dyn TorManagerPort + Send + Sync>,
        nginx_manager: Option<Arc<dyn NginxManagerPort + Send + Sync>>,
    ) -> Self {
        Self {
            tor_manager,
            nginx_manager,
        }
    }

    pub async fn execute(&self, service_name: &str) -> Result<()> {
        // Try to find the correct service name
        // Services can be named directly (raw) or with proxy_ prefix (web)
        let names_to_try = if service_name.starts_with("proxy_") {
            vec![service_name.to_string()]
        } else {
            vec![service_name.to_string(), format!("proxy_{}", service_name)]
        };

        let mut last_error = None;
        let mut tor_started = false;
        let mut actual_name = String::new();

        // 1. Try to start Tor service with each possible name
        for name in &names_to_try {
            match self.tor_manager.start_hidden_service(name).await {
                Ok(_) => {
                    tor_started = true;
                    actual_name = name.clone();
                    break;
                }
                Err(e) => {
                    last_error = Some(e);
                }
            }
        }

        if !tor_started {
            return Err(last_error.unwrap_or_else(|| {
                crate::domain::error::EnolaError::NotFound(format!(
                    "Service '{}' not found",
                    service_name
                ))
            }));
        }

        // 2. Try to enable Nginx site (only for proxy services)
        if let Some(nginx) = &self.nginx_manager {
            // Try with the actual name that worked for Tor
            if nginx.enable_site(&actual_name).await.is_ok() {
                let _ = nginx.reload().await;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::error::EnolaError;
    use crate::ports::tor::TorServiceInfo;
    use crate::ports::web::{NginxFileServerConfig, NginxProxyConfig, NginxSiteConfig};
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicBool, Ordering};

    struct MockTorManager {
        start_called: AtomicBool,
        should_fail: bool,
    }

    impl MockTorManager {
        fn new() -> Self {
            Self {
                start_called: AtomicBool::new(false),
                should_fail: false,
            }
        }
        fn failing() -> Self {
            Self {
                start_called: AtomicBool::new(false),
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
            self.start_called.store(true, Ordering::SeqCst);
            if self.should_fail {
                Err(EnolaError::InfrastructureError("Start failed".into()))
            } else {
                Ok(())
            }
        }
        async fn rotate_hidden_service_identity(&self, _: &str) -> Result<String> {
            Ok("new.onion".into())
        }
    }

    struct MockNginxManager {
        enable_called: AtomicBool,
    }

    impl MockNginxManager {
        fn new() -> Self {
            Self {
                enable_called: AtomicBool::new(false),
            }
        }
    }

    #[async_trait]
    impl NginxManagerPort for MockNginxManager {
        async fn create_site_config(&self, _: NginxSiteConfig) -> Result<()> {
            Ok(())
        }
        async fn create_fileserver_config(&self, _: NginxFileServerConfig) -> Result<()> {
            Ok(())
        }
        async fn create_proxy_config(&self, _: NginxProxyConfig) -> Result<()> {
            Ok(())
        }
        async fn enable_site(&self, _: &str) -> Result<()> {
            self.enable_called.store(true, Ordering::SeqCst);
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
        async fn update_proxy_ports(&self, _: &str, _: u16, _: u16) -> Result<()> {
            Ok(())
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

    #[tokio::test]
    async fn test_start_tor_service_success() {
        let tor = Arc::new(MockTorManager::new());
        let use_case = StartTorService::new(tor.clone(), None);

        let result = use_case.execute("myservice").await;

        assert!(result.is_ok());
        assert!(tor.start_called.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn test_start_tor_service_with_nginx() {
        let tor = Arc::new(MockTorManager::new());
        let nginx = Arc::new(MockNginxManager::new());
        let use_case = StartTorService::new(tor.clone(), Some(nginx.clone()));

        let result = use_case.execute("myservice").await;

        assert!(result.is_ok());
        assert!(tor.start_called.load(Ordering::SeqCst));
        assert!(nginx.enable_called.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn test_start_tor_service_tor_failure() {
        let tor = Arc::new(MockTorManager::failing());
        let use_case = StartTorService::new(tor, None);

        let result = use_case.execute("myservice").await;

        assert!(result.is_err());
    }
}
