use crate::domain::error::Result;

#[derive(Debug, Clone)]
pub struct NginxSiteConfig {
    pub domain: String,           // e.g. "mysite" (used for filename)
    pub listen_port: u16,         // e.g. 8080 (internal port)
    pub root_dir: String,         // e.g. "/var/www/mysite"
    pub index_files: Vec<String>, // e.g. ["index.html", "index.htm"]
    pub autoindex: bool,
}

#[derive(Debug, Clone)]
pub struct NginxFileServerConfig {
    pub service_name: String, // Used for filename and logging
    pub listen_port: u16,
    pub root_dir: String,
    pub disable_symlinks: bool, // Default true for security
    pub allow_upload: bool,     // If false, restrict to GET/HEAD
}

#[derive(Debug, Clone)]
pub struct NginxProxyConfig {
    pub service_name: String,
    pub listen_port: u16,           // Nginx listens here (e.g. 8080)
    pub backend_port: u16,          // Nginx proxies to localhost:backend_port
    pub server_name: String,        // usually "localhost"
    pub rate_limit: Option<String>, // e.g. "10r/s"
}

/// Configuración de file server con SSL/HTTPS (autoindex + TLS + PQC).
/// El site Nginx se escribe como `fileserver_{service_name}` (nombre canónico).
#[derive(Debug, Clone)]
pub struct NginxFileServerConfigWithSsl {
    pub service_name: String,
    pub http_port: u16,   // Nginx HTTP listen port
    pub https_port: u16,  // Nginx HTTPS listen port
    pub root_dir: String, // e.g. "/srv/enola-files/{name}"
    pub ssl_cert_path: String,
    pub ssl_key_path: String,
}

#[derive(Debug, Clone)]
pub struct NginxProxyConfigWithSsl {
    pub service_name: String,
    pub http_port: u16,    // Nginx HTTP port (for redirect or direct access)
    pub https_port: u16,   // Nginx HTTPS port
    pub backend_port: u16, // Backend application port
    pub server_name: String,
    pub ssl_cert_path: String, // Path to SSL certificate
    pub ssl_key_path: String,  // Path to SSL private key
    pub rate_limit: Option<String>,
}

#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait NginxManagerPort {
    /// Create a standard static site configuration
    async fn create_site_config(&self, config: NginxSiteConfig) -> Result<()>;

    /// Create a secure file server configuration
    async fn create_fileserver_config(&self, config: NginxFileServerConfig) -> Result<()>;

    /// Create a reverse proxy configuration
    async fn create_proxy_config(&self, config: NginxProxyConfig) -> Result<()>;

    /// Create a reverse proxy configuration with SSL/HTTPS support
    async fn create_proxy_config_with_ssl(&self, config: NginxProxyConfigWithSsl) -> Result<()>;

    /// Create a file server configuration with SSL/HTTPS (autoindex + TLS + PQC)
    async fn create_fileserver_config_with_ssl(
        &self,
        config: NginxFileServerConfigWithSsl,
    ) -> Result<()>;

    /// Detect whether a site config already has SSL enabled.
    /// Returns `(has_ssl, https_port, cert_path)`.
    async fn detect_ssl_config(&self, domain: &str) -> Result<(bool, Option<u16>, Option<String>)>;

    /// Generate a self-signed SSL certificate for a service
    /// Returns (cert_path, key_path)
    async fn generate_self_signed_cert(&self, service_name: &str) -> Result<(String, String)>;

    /// Borra el certificado TLS `<service_name>.{crt,key}` del servicio.
    /// Sin certificado devuelve `Ok(())` (idempotente).
    async fn delete_ssl_cert(&self, service_name: &str) -> Result<()>;

    /// Enable a site (symlink available -> enabled)
    async fn enable_site(&self, domain: &str) -> Result<()>;

    /// Disable a site (remove symlink)
    async fn disable_site(&self, domain: &str) -> Result<()>;

    /// Delete site configuration (remove file from sites-available)
    async fn delete_site_config(&self, domain: &str) -> Result<()>;

    /// Check if configuration syntax is valid (nginx -t)
    async fn validate_config(&self) -> Result<bool>;

    /// Reload Nginx service
    async fn reload(&self) -> Result<()>;

    /// Update ports for a proxy site (listen_port and backend_port)
    async fn update_proxy_ports(
        &self,
        domain: &str,
        listen_port: u16,
        backend_port: u16,
    ) -> Result<()>;

    /// Update ports for a proxy site, supporting both HTTP and HTTPS
    async fn update_proxy_ports_with_ssl(
        &self,
        domain: &str,
        http_listen_port: u16,
        https_listen_port: Option<u16>,
        backend_port: u16,
    ) -> Result<()>;

    /// List enabled sites (filenames in sites-enabled)
    async fn list_enabled_sites(&self) -> Result<Vec<String>>;

    /// Find an available port within a range
    /// Returns the port number if found, or an error if no port is available
    async fn find_available_port(&self, range_start: u16, range_end: u16) -> Result<u16>;

    /// Check if a specific port is available
    async fn is_port_available(&self, port: u16) -> bool;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_nginx_create_proxy_config() {
        let mut mock = MockNginxManagerPort::new();
        mock.expect_create_proxy_config().returning(|_| Ok(()));
        let config = NginxProxyConfig {
            service_name: "test".into(),
            listen_port: 8080,
            backend_port: 3000,
            server_name: "localhost".into(),
            rate_limit: None,
        };
        assert!(mock.create_proxy_config(config).await.is_ok());
    }

    #[tokio::test]
    async fn test_mock_nginx_validate_and_reload() {
        let mut mock = MockNginxManagerPort::new();
        mock.expect_validate_config().returning(|| Ok(true));
        mock.expect_reload().returning(|| Ok(()));
        assert!(mock.validate_config().await.unwrap());
        assert!(mock.reload().await.is_ok());
    }

    #[test]
    fn test_nginx_site_config_struct() {
        let config = NginxSiteConfig {
            domain: "mysite".into(),
            listen_port: 8080,
            root_dir: "/var/www/mysite".into(),
            index_files: vec!["index.html".into()],
            autoindex: false,
        };
        assert_eq!(config.domain, "mysite");
    }

    #[test]
    fn test_nginx_fileserver_config_struct() {
        let config = NginxFileServerConfig {
            service_name: "files".into(),
            listen_port: 9000,
            root_dir: "/data".into(),
            disable_symlinks: true,
            allow_upload: false,
        };
        assert!(config.disable_symlinks);
        assert!(!config.allow_upload);
    }
}
