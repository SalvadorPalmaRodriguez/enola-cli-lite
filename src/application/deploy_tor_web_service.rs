use crate::domain::error::{EnolaError, Result};
use crate::ports::manifest::ManifestPort;
use crate::ports::service::ServiceManagerPort;
use crate::ports::tor::TorManagerPort;
use crate::ports::web::{NginxManagerPort, NginxProxyConfig};
use std::sync::Arc;

/// Input for DeployTorWebService
pub struct DeployTorWebServiceRequest {
    pub service_name: String,
    pub backend_port: u16, // Local app port (e.g. 3000)
    pub nginx_port: u16,   // Nginx listen port (e.g. 8081)
    pub enable_auth: bool,
}

/// Use Case: Deploy a Web Service (Reverse Proxy) via Tor
pub struct DeployTorWebService {
    nginx_manager: Arc<dyn NginxManagerPort + Send + Sync>,
    tor_manager: Arc<dyn TorManagerPort + Send + Sync>,
    #[allow(dead_code)]
    service_manager: Arc<dyn ServiceManagerPort + Send + Sync>,
    manifest: Arc<dyn ManifestPort + Send + Sync>,
}

impl DeployTorWebService {
    pub fn new(
        nginx_manager: Arc<dyn NginxManagerPort + Send + Sync>,
        tor_manager: Arc<dyn TorManagerPort + Send + Sync>,
        service_manager: Arc<dyn ServiceManagerPort + Send + Sync>,
        manifest: Arc<dyn ManifestPort + Send + Sync>,
    ) -> Self {
        Self {
            nginx_manager,
            tor_manager,
            service_manager,
            manifest,
        }
    }

    pub async fn execute(&self, request: DeployTorWebServiceRequest) -> Result<String> {
        // 1. Validate Input — el nombre se interpola en configs Nginx
        // (`proxy_{name}.conf`) y en rutas Tor: validación estricta (R-12).
        // Es el nombre de servicio canónico (`<dominio>_<name>`), no el nombre
        // base del usuario: admite `_` como separador de dominio.
        crate::domain::naming::validate_tor_service_name(&request.service_name)?;

        // 2. Create Nginx Reverse Proxy Config
        eprintln!("   📝 Creating Nginx proxy config...");
        let _ = std::io::Write::flush(&mut std::io::stderr());

        let nginx_config = NginxProxyConfig {
            service_name: request.service_name.clone(),
            listen_port: request.nginx_port,
            backend_port: request.backend_port,
            server_name: "localhost".to_string(),
            rate_limit: None,
        };

        self.nginx_manager.create_proxy_config(nginx_config).await?;
        let _ = self
            .manifest
            .append("nginx_config", &format!("proxy_{}", request.service_name));
        eprintln!("   ✓ Nginx config created");
        let _ = std::io::Write::flush(&mut std::io::stderr());

        // 3. Validate & Enable Nginx Config
        eprintln!("   🔍 Validating Nginx config...");
        let _ = std::io::Write::flush(&mut std::io::stderr());

        if !self.nginx_manager.validate_config().await? {
            // Ideally rollback creation
            return Err(EnolaError::InfrastructureError(
                "Generated Nginx config is invalid".to_string(),
            ));
        }
        eprintln!("   ✓ Nginx config valid");
        let _ = std::io::Write::flush(&mut std::io::stderr());

        let site_name = format!("proxy_{}", request.service_name);
        eprintln!("   🔗 Enabling Nginx site '{}'...", site_name);
        let _ = std::io::Write::flush(&mut std::io::stderr());

        self.nginx_manager.enable_site(&site_name).await?;
        eprintln!("   ✓ Nginx site enabled");
        let _ = std::io::Write::flush(&mut std::io::stderr());

        eprintln!("   🔄 Reloading Nginx...");
        let _ = std::io::Write::flush(&mut std::io::stderr());

        self.nginx_manager.reload().await?;
        eprintln!("   ✓ Nginx reloaded");
        let _ = std::io::Write::flush(&mut std::io::stderr());

        // 4. Deploy Hidden Service
        // Map port 80 (public onion) -> request.nginx_port (internal nginx)
        let ports = vec![(80, request.nginx_port)];
        // Nombre canónico (MEGAPLAN-PUBLISH, D12): el servicio Tor ES
        // `service_name` (`<dominio>_<name>`); el site Nginx es `proxy_{name}`.
        let tor_service_name = request.service_name.clone();

        eprintln!(
            "   🧅 Deploying Tor hidden service '{}'...",
            tor_service_name
        );
        let _ = std::io::Write::flush(&mut std::io::stderr());

        let onion_address = self
            .tor_manager
            .deploy_hidden_service(&tor_service_name, ports)
            .await?;
        let _ = self.manifest.append("tor_service", &tor_service_name);

        // 5. Handle Auth (Optional) — siempre vía ManageClientAuth (D6)
        if request.enable_auth {
            crate::application::manage_client_auth::ManageClientAuth::new(self.tor_manager.clone())
                .toggle_auth(&tor_service_name, true)
                .await?;
        }

        // Reload Tor to ensure auth/service is active
        self.tor_manager.reload_tor().await?;

        Ok(onion_address)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::manifest::MockManifestPort;
    use crate::ports::service::MockServiceManagerPort;
    use crate::ports::tor::MockTorManagerPort;
    use crate::ports::web::MockNginxManagerPort;

    fn mock_manifest() -> MockManifestPort {
        let mut m = MockManifestPort::new();
        m.expect_append().returning(|_, _| Ok(())).times(0..);
        m.expect_remove().returning(|_, _| Ok(())).times(0..);
        m
    }

    fn setup_mocks() -> (
        MockNginxManagerPort,
        MockTorManagerPort,
        MockServiceManagerPort,
    ) {
        (
            MockNginxManagerPort::new(),
            MockTorManagerPort::new(),
            MockServiceManagerPort::new(),
        )
    }

    #[tokio::test]
    async fn test_deploy_empty_name_returns_validation_error() {
        let (nginx, tor, svc) = setup_mocks();
        let service = DeployTorWebService::new(
            Arc::new(nginx),
            Arc::new(tor),
            Arc::new(svc),
            Arc::new(mock_manifest()),
        );
        let request = DeployTorWebServiceRequest {
            service_name: "".to_string(),
            backend_port: 3000,
            nginx_port: 8081,
            enable_auth: false,
        };
        let result = service.execute(request).await;
        assert!(result.is_err());
        match result {
            Err(EnolaError::ValidationError(_)) => {}
            _ => panic!("Expected ValidationError"),
        }
    }

    #[tokio::test]
    async fn test_deploy_success_without_auth() {
        let (mut nginx, mut tor, svc) = setup_mocks();

        nginx.expect_create_proxy_config().returning(|_| Ok(()));
        nginx.expect_validate_config().returning(|| Ok(true));
        nginx.expect_enable_site().returning(|_| Ok(()));
        nginx.expect_reload().returning(|| Ok(()));

        tor.expect_deploy_hidden_service()
            .returning(|_, _| Ok("abc123.onion".to_string()));
        tor.expect_reload_tor().returning(|| Ok(()));

        let service = DeployTorWebService::new(
            Arc::new(nginx),
            Arc::new(tor),
            Arc::new(svc),
            Arc::new(mock_manifest()),
        );
        let request = DeployTorWebServiceRequest {
            service_name: "myapp".to_string(),
            backend_port: 3000,
            nginx_port: 8081,
            enable_auth: false,
        };
        let result = service.execute(request).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "abc123.onion");
    }

    #[tokio::test]
    async fn test_deploy_invalid_nginx_config_returns_error() {
        let (mut nginx, tor, svc) = setup_mocks();

        nginx.expect_create_proxy_config().returning(|_| Ok(()));
        nginx.expect_validate_config().returning(|| Ok(false));

        let service = DeployTorWebService::new(
            Arc::new(nginx),
            Arc::new(tor),
            Arc::new(svc),
            Arc::new(mock_manifest()),
        );
        let request = DeployTorWebServiceRequest {
            service_name: "myapp".to_string(),
            backend_port: 3000,
            nginx_port: 8081,
            enable_auth: false,
        };
        let result = service.execute(request).await;
        assert!(result.is_err());
        match result {
            Err(EnolaError::InfrastructureError(msg)) => assert!(msg.contains("invalid")),
            _ => panic!("Expected InfrastructureError for invalid nginx"),
        }
    }

    #[tokio::test]
    async fn test_deploy_with_auth_enables_client_auth() {
        let (mut nginx, mut tor, svc) = setup_mocks();

        nginx.expect_create_proxy_config().returning(|_| Ok(()));
        nginx.expect_validate_config().returning(|| Ok(true));
        nginx.expect_enable_site().returning(|_| Ok(()));
        nginx.expect_reload().returning(|| Ok(()));

        tor.expect_deploy_hidden_service()
            .returning(|_, _| Ok("xyz.onion".to_string()));
        tor.expect_enable_client_auth()
            .times(1)
            .returning(|_| Ok(()));
        tor.expect_reload_tor().returning(|| Ok(()));

        let service = DeployTorWebService::new(
            Arc::new(nginx),
            Arc::new(tor),
            Arc::new(svc),
            Arc::new(mock_manifest()),
        );
        let request = DeployTorWebServiceRequest {
            service_name: "secure".to_string(),
            backend_port: 3000,
            nginx_port: 8081,
            enable_auth: true,
        };
        let result = service.execute(request).await;
        assert!(result.is_ok());
    }

    /// R-12: el nombre llega a `proxy_{name}.conf` de Nginx — rechazar
    /// traversal/absoluto/newline antes de crear la config.
    #[tokio::test]
    async fn test_deploy_tor_web_service_rejects_unsafe_names() {
        for name in ["../x", "/etc/nginx/x", "a\nb", ""] {
            let service = DeployTorWebService::new(
                Arc::new(MockNginxManagerPort::new()),
                Arc::new(MockTorManagerPort::new()),
                Arc::new(MockServiceManagerPort::new()),
                Arc::new(mock_manifest()),
            );
            let result = service
                .execute(DeployTorWebServiceRequest {
                    service_name: name.to_string(),
                    backend_port: 3000,
                    nginx_port: 8081,
                    enable_auth: false,
                })
                .await;
            assert!(result.is_err(), "name {name:?} debe ser rechazado");
        }
    }
}
