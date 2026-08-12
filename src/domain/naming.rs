//! Service Naming Convention Module
//!
//! This module centralizes all naming conventions for Enola services.
//! It ensures consistency across Tor, Docker, Nginx, and file system.
//!
//! ## Naming Convention:
//!
//! | Service Type | Tor Dir Prefix | Tor Config Name | Docker Container | Nginx Config |
//! |--------------|----------------|-----------------|------------------|--------------|
//! | Git          | enola_git_     | git_{name}      | enola-git-{name} | proxy_git_{name} |
//! | WordPress    | enola_wp_      | wp_{name}       | wp-{name}        | proxy_wp_{name} |
//! | Static Site  | enola_static_  | static_{name}   | N/A              | static_{name} |
//! | File Server  | enola_files_   | files_{name}    | N/A              | files_{name} |
//! | Proxy/Web    | enola_proxy_   | proxy_{name}    | N/A              | proxy_{name} |
//! | Raw TCP      | enola_raw_     | raw_{name}      | N/A              | N/A |

use super::error::EnolaError;
use std::fmt;

// ─── Validación de nombres de servicio (SEC-002) ───────────────────────────
// Los nombres se usan en paths del sistema, nombres de contenedores Docker,
// configs de Nginx y Tor. Deben ser estrictamente alfanuméricos + guiones.

/// Comprueba que un nombre de servicio solo contenga minúsculas, dígitos y guiones,
/// empiece por letra o dígito y no exceda 63 caracteres.
/// No requiere regex para evitar expect() en producción (HARD-PRO-004).
fn is_valid_service_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 63 {
        return false;
    }
    let bytes = name.as_bytes();
    let first = bytes[0];
    if !first.is_ascii_lowercase() && !first.is_ascii_digit() {
        return false;
    }
    bytes[1..]
        .iter()
        .all(|&b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Valida que un nombre de servicio sea seguro para usar en:
/// - Nombres de contenedores Docker
/// - Rutas de sistema (`/srv/enola-git/{name}/`)
/// - Configs de Nginx y Tor
///
/// Reglas:
/// - Solo minúsculas, dígitos y guiones
/// - Empieza con letra o dígito (no guión)
/// - Máximo 63 caracteres
/// - No puede ser vacío
///
/// # Errors
/// Retorna `EnolaError::ValidationError` si el nombre no cumple las reglas.
pub fn validate_service_name(name: &str) -> Result<(), EnolaError> {
    if name.is_empty() {
        return Err(EnolaError::ValidationError(
            "El nombre del servicio no puede estar vacío".into(),
        ));
    }
    if !is_valid_service_name(name) {
        return Err(EnolaError::ValidationError(format!(
            "Nombre de servicio inválido: '{}'\n\
             Reglas: solo minúsculas, dígitos y guiones; empieza con letra o dígito; máx 63 chars.\n\
             Ejemplo válido: mi-servicio",
            name
        )));
    }
    Ok(())
}

/// Service types supported by Enola
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceType {
    Git,
    WordPress,
    StaticSite,
    FileServer,
    Proxy,
    Raw,
}

impl ServiceType {
    /// Get the prefix used for Tor hidden service directories
    /// e.g., /var/lib/tor/enola_git_{name}
    pub fn tor_dir_prefix(&self) -> &'static str {
        match self {
            ServiceType::Git => "enola_git_",
            ServiceType::WordPress => "enola_wp_",
            ServiceType::StaticSite => "enola_static_",
            ServiceType::FileServer => "enola_fileserver_",
            ServiceType::Proxy => "enola_proxy_",
            ServiceType::Raw => "enola_raw_",
        }
    }

    /// Get the prefix used for Tor configuration files
    /// e.g., /etc/tor/enola.d/git_{name}.conf
    pub fn tor_config_prefix(&self) -> &'static str {
        match self {
            ServiceType::Git => "git_",
            ServiceType::WordPress => "wp_",
            ServiceType::StaticSite => "static_",
            ServiceType::FileServer => "fileserver_",
            ServiceType::Proxy => "proxy_",
            ServiceType::Raw => "raw_",
        }
    }

    /// Get the prefix used for Docker containers
    /// e.g., enola-git-{name}
    pub fn docker_prefix(&self) -> Option<&'static str> {
        match self {
            ServiceType::Git => Some("enola-git-"),
            ServiceType::WordPress => Some("wp-"),
            ServiceType::StaticSite => None,
            ServiceType::FileServer => None,
            ServiceType::Proxy => None,
            ServiceType::Raw => None,
        }
    }

    /// Get the prefix used for Nginx configuration files
    /// e.g., /etc/nginx/sites-available/proxy_git_{name}
    pub fn nginx_prefix(&self) -> Option<&'static str> {
        match self {
            ServiceType::Git => Some("proxy_git_"),
            ServiceType::WordPress => Some("proxy_wp_"),
            ServiceType::StaticSite => Some("static_"),
            ServiceType::FileServer => Some("fileserver_"),
            ServiceType::Proxy => Some("proxy_"),
            ServiceType::Raw => None,
        }
    }

    /// Parse service type from a string
    pub fn parse_from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "git" | "forgejo" => Some(ServiceType::Git),
            "wordpress" | "wp" => Some(ServiceType::WordPress),
            "static" | "site" => Some(ServiceType::StaticSite),
            "files" | "fileserver" => Some(ServiceType::FileServer),
            "proxy" | "web" | "http" => Some(ServiceType::Proxy),
            "raw" | "tcp" => Some(ServiceType::Raw),
            _ => None,
        }
    }
}

impl fmt::Display for ServiceType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ServiceType::Git => write!(f, "git"),
            ServiceType::WordPress => write!(f, "wordpress"),
            ServiceType::StaticSite => write!(f, "static"),
            ServiceType::FileServer => write!(f, "fileserver"),
            ServiceType::Proxy => write!(f, "proxy"),
            ServiceType::Raw => write!(f, "raw"),
        }
    }
}

/// Service name builder - creates consistent names across all systems
#[derive(Debug, Clone)]
pub struct ServiceName {
    pub base_name: String,
    pub service_type: ServiceType,
}

impl ServiceName {
    pub fn new(base_name: &str, service_type: ServiceType) -> Self {
        Self {
            base_name: base_name.to_string(),
            service_type,
        }
    }

    /// Get the full Tor hidden service directory name
    /// e.g., "enola_git_myserver"
    pub fn tor_dir_name(&self) -> String {
        format!("{}{}", self.service_type.tor_dir_prefix(), self.base_name)
    }

    /// Get the full Tor hidden service directory path
    /// e.g., "/var/lib/tor/enola_git_myserver"
    pub fn tor_dir_path(&self) -> String {
        format!("/var/lib/tor/{}", self.tor_dir_name())
    }

    /// Get the Tor configuration file name (without extension)
    /// e.g., "git_myserver"
    pub fn tor_config_name(&self) -> String {
        format!(
            "{}{}",
            self.service_type.tor_config_prefix(),
            self.base_name
        )
    }

    /// Get the full Tor configuration file path
    /// e.g., "/etc/tor/enola.d/git_myserver.conf"
    pub fn tor_config_path(&self) -> String {
        format!("/etc/tor/enola.d/{}.conf", self.tor_config_name())
    }

    /// Get the Docker container name (if applicable)
    /// e.g., "enola-git-myserver"
    pub fn docker_container_name(&self) -> Option<String> {
        self.service_type
            .docker_prefix()
            .map(|prefix| format!("{}{}", prefix, self.base_name))
    }
    /// Get the Nginx configuration file name (if applicable)
    /// e.g., "proxy_git_myserver"
    pub fn nginx_config_name(&self) -> Option<String> {
        self.service_type
            .nginx_prefix()
            .map(|prefix| format!("{}{}", prefix, self.base_name))
    }

    /// Get the full Nginx configuration file path (if applicable)
    /// e.g., "/etc/nginx/sites-available/proxy_git_myserver"
    pub fn nginx_config_path(&self) -> Option<String> {
        self.nginx_config_name()
            .map(|name| format!("/etc/nginx/sites-available/{}", name))
    }

    /// Parse a service name from a Tor directory name
    /// e.g., "enola_git_myserver" -> Some(ServiceName { base_name: "myserver", service_type: Git })
    pub fn from_tor_dir(dir_name: &str) -> Option<Self> {
        for service_type in [
            ServiceType::Git,
            ServiceType::WordPress,
            ServiceType::StaticSite,
            ServiceType::FileServer,
            ServiceType::Proxy,
            ServiceType::Raw,
        ] {
            let prefix = service_type.tor_dir_prefix();
            if dir_name.starts_with(prefix) {
                let base_name = dir_name.strip_prefix(prefix)?;
                return Some(Self::new(base_name, service_type));
            }
        }

        // Legacy support: handle old "enola_" prefix without service type
        if dir_name.starts_with("enola_") {
            let base_name = dir_name.strip_prefix("enola_")?;
            // Try to infer type from base_name
            if base_name.starts_with("proxy_") {
                let actual_name = base_name.strip_prefix("proxy_")?;
                return Some(Self::new(actual_name, ServiceType::Proxy));
            }
            // Default to proxy for backwards compatibility
            return Some(Self::new(base_name, ServiceType::Proxy));
        }

        None
    }

    /// Parse a service name from a Tor config name
    /// e.g., "git_myserver" -> Some(ServiceName { base_name: "myserver", service_type: Git })
    pub fn from_tor_config(config_name: &str) -> Option<Self> {
        for service_type in [
            ServiceType::Git,
            ServiceType::WordPress,
            ServiceType::StaticSite,
            ServiceType::FileServer,
            ServiceType::Proxy,
            ServiceType::Raw,
        ] {
            let prefix = service_type.tor_config_prefix();
            if config_name.starts_with(prefix) {
                let base_name = config_name.strip_prefix(prefix)?;
                return Some(Self::new(base_name, service_type));
            }
        }
        None
    }

    /// Try to find a service by base name, checking all possible prefixes
    /// Returns the list of possible names to search for
    pub fn possible_names_for_lookup(base_name: &str) -> Vec<String> {
        let mut names = vec![base_name.to_string()];

        for service_type in [
            ServiceType::Git,
            ServiceType::WordPress,
            ServiceType::StaticSite,
            ServiceType::FileServer,
            ServiceType::Proxy,
            ServiceType::Raw,
        ] {
            names.push(format!("{}{}", service_type.tor_config_prefix(), base_name));
        }

        // Legacy names
        names.push(format!("proxy_{}", base_name));

        names
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_service_name_git() {
        let name = ServiceName::new("myrepo", ServiceType::Git);

        assert_eq!(name.tor_dir_name(), "enola_git_myrepo");
        assert_eq!(name.tor_dir_path(), "/var/lib/tor/enola_git_myrepo");
        assert_eq!(name.tor_config_name(), "git_myrepo");
        assert_eq!(name.tor_config_path(), "/etc/tor/enola.d/git_myrepo.conf");
        assert_eq!(
            name.docker_container_name(),
            Some("enola-git-myrepo".to_string())
        );
        assert_eq!(
            name.nginx_config_name(),
            Some("proxy_git_myrepo".to_string())
        );
    }

    #[test]
    fn test_service_name_wordpress() {
        let name = ServiceName::new("myblog", ServiceType::WordPress);

        assert_eq!(name.tor_dir_name(), "enola_wp_myblog");
        assert_eq!(name.tor_config_name(), "wp_myblog");
        assert_eq!(name.docker_container_name(), Some("wp-myblog".to_string()));
    }

    #[test]
    fn test_service_name_static() {
        let name = ServiceName::new("mysite", ServiceType::StaticSite);

        assert_eq!(name.tor_dir_name(), "enola_static_mysite");
        assert_eq!(name.tor_config_name(), "static_mysite");
        assert_eq!(name.docker_container_name(), None);
    }

    #[test]
    fn test_service_name_fileserver() {
        let name = ServiceName::new("myshare", ServiceType::FileServer);

        assert_eq!(name.tor_dir_name(), "enola_fileserver_myshare");
        assert_eq!(name.tor_config_name(), "fileserver_myshare");
        assert_eq!(
            name.nginx_config_name(),
            Some("fileserver_myshare".to_string())
        );
        assert_eq!(name.docker_container_name(), None);
    }

    #[test]
    fn test_from_tor_dir() {
        let parsed = ServiceName::from_tor_dir("enola_git_myrepo").unwrap();
        assert_eq!(parsed.base_name, "myrepo");
        assert_eq!(parsed.service_type, ServiceType::Git);

        let parsed = ServiceName::from_tor_dir("enola_wp_myblog").unwrap();
        assert_eq!(parsed.base_name, "myblog");
        assert_eq!(parsed.service_type, ServiceType::WordPress);
    }

    #[test]
    fn test_from_tor_dir_legacy() {
        // Legacy format: enola_proxy_xxx
        let parsed = ServiceName::from_tor_dir("enola_proxy_myservice").unwrap();
        assert_eq!(parsed.base_name, "myservice");
        assert_eq!(parsed.service_type, ServiceType::Proxy);
    }

    #[test]
    fn test_possible_names_for_lookup() {
        let names = ServiceName::possible_names_for_lookup("myservice");

        assert!(names.contains(&"myservice".to_string()));
        assert!(names.contains(&"git_myservice".to_string()));
        assert!(names.contains(&"wp_myservice".to_string()));
        assert!(names.contains(&"proxy_myservice".to_string()));
        assert!(names.contains(&"fileserver_myservice".to_string()));
    }

    // ── TEST-COV-NEG-001: Args inválidos — validate_service_name ─────────────
    // Cada caso verifica: error preciso + tipo EnolaError::ValidationError.
    // §13.74 (safe_args), §13.3 (naming conventions)

    #[test]
    fn neg001_validate_service_name_accepts_valid_names() {
        assert!(validate_service_name("myservice").is_ok());
        assert!(validate_service_name("my-service").is_ok());
        assert!(validate_service_name("my-service-123").is_ok());
        assert!(validate_service_name("a").is_ok());
        assert!(validate_service_name("1service").is_ok());
        assert!(validate_service_name(&"a".repeat(63)).is_ok());
    }

    #[test]
    fn neg001_validate_service_name_rejects_empty() {
        let err = validate_service_name("").unwrap_err();
        assert!(matches!(err, EnolaError::ValidationError(_)));
        let msg = err.to_string();
        assert!(
            msg.contains("vac") || msg.contains("empty"),
            "must mention empty: {}",
            msg
        );
    }

    #[test]
    fn neg001_validate_service_name_rejects_shell_metacharacters() {
        // TEST-COV-NEG-001: metacaracteres de shell → rechazo con ValidationError
        // Previene inyección en sh -c (§13.74 safe_args)
        let bad = [
            "test;rm",
            "test|head",
            "test&&bad",
            "test'quote",
            "test\"quote",
            "test$cmd",
            "test>redir",
            "test<redir",
            "test(paren",
            "test{brace}",
            "test*glob",
            "test?glob",
            "test[bracket]",
            "test~tilde",
            "test!bang",
            "test space",
        ];
        for name in bad {
            let result = validate_service_name(name);
            assert!(
                result.is_err(),
                "Se esperaba rechazo de metacaracter: {:?}",
                name
            );
            assert!(matches!(
                result.unwrap_err(),
                EnolaError::ValidationError(_)
            ));
        }
    }

    #[test]
    fn neg001_validate_service_name_rejects_path_traversal() {
        // TEST-COV-NEG-001: path traversal en nombres → rechazo
        let bad = [
            "../etc",
            "../../etc/passwd",
            "./relative",
            "/absolute",
            "test/slash",
        ];
        for name in bad {
            let result = validate_service_name(name);
            assert!(
                result.is_err(),
                "Se esperaba rechazo de path traversal: {:?}",
                name
            );
        }
    }

    #[test]
    fn neg001_validate_service_name_rejects_uppercase() {
        // Naming convention: solo minúsculas (Docker/DNS compatibility)
        assert!(validate_service_name("TestName").is_err());
        assert!(validate_service_name("MyService").is_err());
        assert!(validate_service_name("SERVICE").is_err());
    }

    #[test]
    fn neg001_validate_service_name_rejects_leading_dash() {
        assert!(validate_service_name("-myservice").is_err());
        assert!(validate_service_name("--option").is_err());
    }

    #[test]
    fn neg001_validate_service_name_rejects_too_long() {
        // Máximo 63 chars (hostname DNS + Docker container name limit)
        let long = "a".repeat(64);
        let err = validate_service_name(&long).unwrap_err();
        assert!(matches!(err, EnolaError::ValidationError(_)));
    }

    #[test]
    fn neg001_validate_service_name_rejects_whitespace() {
        assert!(validate_service_name("my service").is_err());
        assert!(validate_service_name("my\tservice").is_err());
        assert!(validate_service_name("my\nservice").is_err());
    }

    #[test]
    fn neg001_validate_service_name_error_message_is_helpful() {
        // El mensaje de error debe ser orientativo (guía al usuario)
        let err = validate_service_name("Bad;Name").unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("min")
                || msg.contains("inv")
                || msg.contains("Ejemplo")
                || msg.contains("guion")
                || msg.contains("letra"),
            "Error message debe ser orientativo: {}",
            msg
        );
    }
}
