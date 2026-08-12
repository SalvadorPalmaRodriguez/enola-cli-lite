use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Tipo de servicio para determinar la estrategia de puertos
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ServiceType {
    Web,        // WordPress, Static Sites (HTTP/HTTPS)
    Git,        // Forgejo (HTTP/HTTPS + SSH)
    Api,        // Generic API services (HTTP/HTTPS)
    FileServer, // Nginx File Server
    Raw,        // Direct TCP (SSH, Custom)
}

/// Configuración para puerto HTTP
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpPortConfig {
    pub tor_virtual_port: u16,  // Puerto expuesto en .onion (típicamente 80)
    pub nginx_listen_port: u16, // Puerto donde escucha Nginx
    pub backend_port: u16,      // Puerto de la aplicación
}

/// Configuración para puerto HTTPS
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpsPortConfig {
    pub tor_virtual_port: u16,  // Puerto expuesto en .onion (típicamente 443)
    pub nginx_listen_port: u16, // Puerto donde escucha Nginx con SSL
    pub backend_port: u16,      // Puerto de la aplicación
    pub cert_path: PathBuf,
    pub key_path: PathBuf,
}

/// Configuración para puerto SSH (usado en Git)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshPortConfig {
    pub tor_virtual_port: u16, // Puerto expuesto en .onion (típicamente 22)
    pub host_port: u16,        // Puerto SSH del host mapeado al contenedor
    pub target_port: u16,      // Puerto destino (generalmente 22 dentro del contenedor)
}

/// Configuración para puertos personalizados adicionales
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomPortConfig {
    pub name: String,
    pub tor_virtual_port: u16,
    pub target_port: u16,
}

/// Configuración unificada de puertos para cualquier servicio
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServicePortConfig {
    pub service_name: String,
    pub service_type: ServiceType,
    pub http: Option<HttpPortConfig>,
    pub https: Option<HttpsPortConfig>,
    pub ssh: Option<SshPortConfig>,
    pub custom_ports: Vec<CustomPortConfig>,
}

impl ServicePortConfig {
    pub fn new(service_name: &str, service_type: ServiceType) -> Self {
        Self {
            service_name: service_name.to_string(),
            service_type,
            http: None,
            https: None,
            ssh: None,
            custom_ports: Vec::new(),
        }
    }

    pub fn with_http(mut self, virtual_p: u16, nginx_p: u16, backend_p: u16) -> Self {
        self.http = Some(HttpPortConfig {
            tor_virtual_port: virtual_p,
            nginx_listen_port: nginx_p,
            backend_port: backend_p,
        });
        self
    }

    pub fn with_https(
        mut self,
        virtual_p: u16,
        nginx_p: u16,
        backend_p: u16,
        cert: PathBuf,
        key: PathBuf,
    ) -> Self {
        self.https = Some(HttpsPortConfig {
            tor_virtual_port: virtual_p,
            nginx_listen_port: nginx_p,
            backend_port: backend_p,
            cert_path: cert,
            key_path: key,
        });
        self
    }

    pub fn with_ssh(mut self, virtual_p: u16, host_p: u16, target_p: u16) -> Self {
        self.ssh = Some(SshPortConfig {
            tor_virtual_port: virtual_p,
            host_port: host_p,
            target_port: target_p,
        });
        self
    }

    pub fn add_custom_port(mut self, name: &str, virtual_p: u16, target_p: u16) -> Self {
        self.custom_ports.push(CustomPortConfig {
            name: name.to_string(),
            tor_virtual_port: virtual_p,
            target_port: target_p,
        });
        self
    }

    pub fn has_ssl(&self) -> bool {
        self.https.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_service_port_config_builder() {
        let config = ServicePortConfig::new("test-service", ServiceType::Web)
            .with_http(80, 10080, 8080)
            .with_https(
                443,
                10443,
                8080,
                PathBuf::from("/etc/ssl/cert.pem"),
                PathBuf::from("/etc/ssl/key.pem"),
            );

        assert_eq!(config.service_name, "test-service");
        assert_eq!(config.service_type, ServiceType::Web);
        assert!(config.has_ssl());

        let http = config.http.unwrap();
        assert_eq!(http.tor_virtual_port, 80);
        assert_eq!(http.nginx_listen_port, 10080);
        assert_eq!(http.backend_port, 8080);

        let https = config.https.unwrap();
        assert_eq!(https.tor_virtual_port, 443);
        assert_eq!(https.backend_port, 8080);
    }
}
