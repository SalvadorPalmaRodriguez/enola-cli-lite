use crate::domain::error::Result;
use crate::ports::service::ServiceManagerPort;
use crate::ports::web::NginxManagerPort;
use reqwest::Client;
use serde::Serialize;
use std::sync::Arc;

#[derive(Serialize)]
pub struct NginxStatusReport {
    pub active: bool,
    pub sites_enabled: Vec<String>,
    pub check_http_status: u16, // 0 if failed
    pub version: String,        // Optional
}

pub struct NginxStatusChecker {
    service_manager: Arc<dyn ServiceManagerPort + Send + Sync>,
    nginx_manager: Arc<dyn NginxManagerPort + Send + Sync>,
}

impl NginxStatusChecker {
    pub fn new(
        service_manager: Arc<dyn ServiceManagerPort + Send + Sync>,
        nginx_manager: Arc<dyn NginxManagerPort + Send + Sync>,
    ) -> Self {
        Self {
            service_manager,
            nginx_manager,
        }
    }

    pub async fn execute(&self) -> Result<NginxStatusReport> {
        // 1. Service Status
        let active = self
            .service_manager
            .is_active("nginx")
            .await
            .unwrap_or(false);

        // 2. Sites Enabled
        let sites_enabled = self
            .nginx_manager
            .list_enabled_sites()
            .await
            .unwrap_or_default();

        // 3. HTTP Check (Localhost)
        // We assume port 80? Or read default config?
        // Legacy script tries to find port.
        // Let's assume 80 for basic check or try to connect.

        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(2))
            .build()
            .unwrap_or_default();
        let status = match client.get("http://127.0.0.1").send().await {
            Ok(resp) => resp.status().as_u16(),
            Err(_) => 0,
        };

        // 4. Version?
        let version = "unknown".to_string(); // Requires command execution

        Ok(NginxStatusReport {
            active,
            sites_enabled,
            check_http_status: status,
            version,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::error::EnolaError;
    use crate::ports::service::ServiceMetrics;
    use crate::ports::web::{NginxFileServerConfig, NginxProxyConfig, NginxSiteConfig};
    use async_trait::async_trait;

    struct MockServiceManager {
        is_active: bool,
        should_fail: bool,
    }

    impl MockServiceManager {
        fn new(is_active: bool) -> Self {
            Self {
                is_active,
                should_fail: false,
            }
        }

        fn failing() -> Self {
            Self {
                is_active: false,
                should_fail: true,
            }
        }
    }

    #[async_trait]
    impl ServiceManagerPort for MockServiceManager {
        async fn start_service(&self, _name: &str) -> Result<()> {
            Ok(())
        }
        async fn stop_service(&self, _name: &str) -> Result<()> {
            Ok(())
        }
        async fn restart_service(&self, _name: &str) -> Result<()> {
            Ok(())
        }
        async fn enable_service(&self, _name: &str) -> Result<()> {
            Ok(())
        }
        async fn disable_service(&self, _name: &str) -> Result<()> {
            Ok(())
        }
        async fn is_active(&self, _name: &str) -> Result<bool> {
            if self.should_fail {
                Err(EnolaError::InfrastructureError(
                    "Service check failed".into(),
                ))
            } else {
                Ok(self.is_active)
            }
        }
        async fn get_service_metrics(&self, _name: &str) -> Result<ServiceMetrics> {
            Ok(ServiceMetrics::default())
        }
    }

    struct MockNginxManager {
        sites: Vec<String>,
        should_fail: bool,
    }

    impl MockNginxManager {
        fn new() -> Self {
            Self {
                sites: vec![],
                should_fail: false,
            }
        }

        fn with_sites(sites: Vec<String>) -> Self {
            Self {
                sites,
                should_fail: false,
            }
        }

        fn failing() -> Self {
            Self {
                sites: vec![],
                should_fail: true,
            }
        }
    }

    #[async_trait]
    impl NginxManagerPort for MockNginxManager {
        async fn create_site_config(&self, _config: NginxSiteConfig) -> Result<()> {
            Ok(())
        }
        async fn create_fileserver_config(&self, _config: NginxFileServerConfig) -> Result<()> {
            Ok(())
        }
        async fn create_proxy_config(&self, _config: NginxProxyConfig) -> Result<()> {
            Ok(())
        }
        async fn enable_site(&self, _domain: &str) -> Result<()> {
            Ok(())
        }
        async fn disable_site(&self, _domain: &str) -> Result<()> {
            Ok(())
        }
        async fn delete_site_config(&self, _domain: &str) -> Result<()> {
            Ok(())
        }
        async fn validate_config(&self) -> Result<bool> {
            Ok(true)
        }
        async fn reload(&self) -> Result<()> {
            Ok(())
        }
        async fn update_proxy_ports(
            &self,
            _domain: &str,
            _listen_port: u16,
            _backend_port: u16,
        ) -> Result<()> {
            Ok(())
        }
        async fn list_enabled_sites(&self) -> Result<Vec<String>> {
            if self.should_fail {
                Err(EnolaError::InfrastructureError("List sites failed".into()))
            } else {
                Ok(self.sites.clone())
            }
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
    async fn test_nginx_status_checker_active_service() {
        let service = Arc::new(MockServiceManager::new(true));
        let nginx = Arc::new(MockNginxManager::new());
        let checker = NginxStatusChecker::new(service, nginx);

        let result = checker.execute().await;

        assert!(result.is_ok());
        let report = result.unwrap();
        assert!(report.active);
    }

    #[tokio::test]
    async fn test_nginx_status_checker_inactive_service() {
        let service = Arc::new(MockServiceManager::new(false));
        let nginx = Arc::new(MockNginxManager::new());
        let checker = NginxStatusChecker::new(service, nginx);

        let result = checker.execute().await;

        assert!(result.is_ok());
        let report = result.unwrap();
        assert!(!report.active);
    }

    #[tokio::test]
    async fn test_nginx_status_checker_with_sites() {
        let service = Arc::new(MockServiceManager::new(true));
        let sites = vec![
            "site1".to_string(),
            "site2".to_string(),
            "site3".to_string(),
        ];
        let nginx = Arc::new(MockNginxManager::with_sites(sites.clone()));
        let checker = NginxStatusChecker::new(service, nginx);

        let result = checker.execute().await;

        assert!(result.is_ok());
        let report = result.unwrap();
        assert_eq!(report.sites_enabled.len(), 3);
        assert!(report.sites_enabled.contains(&"site1".to_string()));
        assert!(report.sites_enabled.contains(&"site2".to_string()));
        assert!(report.sites_enabled.contains(&"site3".to_string()));
    }

    #[tokio::test]
    async fn test_nginx_status_checker_empty_sites() {
        let service = Arc::new(MockServiceManager::new(true));
        let nginx = Arc::new(MockNginxManager::new());
        let checker = NginxStatusChecker::new(service, nginx);

        let result = checker.execute().await;

        assert!(result.is_ok());
        let report = result.unwrap();
        assert!(report.sites_enabled.is_empty());
    }

    #[tokio::test]
    async fn test_nginx_status_checker_service_check_failure_returns_inactive() {
        let service = Arc::new(MockServiceManager::failing());
        let nginx = Arc::new(MockNginxManager::new());
        let checker = NginxStatusChecker::new(service, nginx);

        let result = checker.execute().await;

        // Should still return ok but with active=false since we use unwrap_or(false)
        assert!(result.is_ok());
        let report = result.unwrap();
        assert!(!report.active);
    }

    #[tokio::test]
    async fn test_nginx_status_checker_sites_failure_returns_empty() {
        let service = Arc::new(MockServiceManager::new(true));
        let nginx = Arc::new(MockNginxManager::failing());
        let checker = NginxStatusChecker::new(service, nginx);

        let result = checker.execute().await;

        // Should still return ok but with empty sites since we use unwrap_or_default()
        assert!(result.is_ok());
        let report = result.unwrap();
        assert!(report.sites_enabled.is_empty());
    }

    #[tokio::test]
    async fn test_nginx_status_checker_returns_unknown_version() {
        let service = Arc::new(MockServiceManager::new(true));
        let nginx = Arc::new(MockNginxManager::new());
        let checker = NginxStatusChecker::new(service, nginx);

        let result = checker.execute().await;

        assert!(result.is_ok());
        let report = result.unwrap();
        assert_eq!(report.version, "unknown");
    }

    #[test]
    fn test_nginx_status_report_serialization() {
        let report = NginxStatusReport {
            active: true,
            sites_enabled: vec!["site1".to_string()],
            check_http_status: 200,
            version: "1.18.0".to_string(),
        };

        let json = serde_json::to_string(&report);
        assert!(json.is_ok());
        let json_str = json.unwrap();
        assert!(json_str.contains("\"active\":true"));
        assert!(json_str.contains("\"check_http_status\":200"));
        assert!(json_str.contains("\"version\":\"1.18.0\""));
    }
}
