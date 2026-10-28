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
//! | Drupal       | enola_drupal_  | drupal_{name}   | drupal-{name}    | proxy_drupal_{name} |
//! | Ghost        | enola_ghost_   | ghost_{name}    | ghost-{name}     | proxy_ghost_{name} |
//! | Magnolia     | enola_magnolia_| magnolia_{name} | magnolia-{name}  | proxy_magnolia_{name} |
//! | Strapi       | enola_strapi_  | strapi_{name}   | strapi-{name}    | proxy_strapi_{name} |
//! | Wagtail      | enola_wagtail_ | wagtail_{name}  | wagtail-{name}   | proxy_wagtail_{name} |
//! | Static Site  | enola_static_  | static_{name}   | N/A              | static_{name} |
//! | File Server  | enola_fileserver_ | fileserver_{name} | N/A         | fileserver_{name} |
//! | Proxy/Web    | enola_proxy_   | proxy_{name}    | N/A              | proxy_{name} |
//! | Raw TCP      | enola_raw_     | raw_{name}      | N/A              | N/A |
//! | AI           | enola_ai_      | ai_{name}       | enola-ai-{name}  | proxy_ai_{name} |

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

/// Comprueba que un nombre de servicio Tor/Nginx **canónico** solo contenga
/// minúsculas, dígitos, guiones y guiones bajos, empiece por letra o dígito
/// y no exceda 80 caracteres (63 de base + prefijo de dominio, p.ej.
/// `fileserver_` = 11 + margen).
fn is_valid_tor_service_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 80 {
        return false;
    }
    let bytes = name.as_bytes();
    let first = bytes[0];
    if !first.is_ascii_lowercase() && !first.is_ascii_digit() {
        return false;
    }
    bytes[1..]
        .iter()
        .all(|&b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

/// Valida un nombre de servicio **ya compuesto** en la capa Tor/Nginx
/// (canonical `<dominio>_<name>`, p.ej. `ai_dev`, `drupal_blog`,
/// `fileserver_share`, `proxy_web1`).
///
/// A diferencia de [`validate_service_name`] —que valida el nombre base
/// elegido por el usuario— aquí se admite `_` porque es el separador
/// canónico de dominio. Las reglas de seguridad son las mismas: sin
/// metacaracteres de shell, sin traversal, sin mayúsculas ni espacios.
///
/// # Errors
/// Retorna `EnolaError::ValidationError` si el nombre no cumple las reglas.
pub fn validate_tor_service_name(name: &str) -> Result<(), EnolaError> {
    if name.is_empty() {
        return Err(EnolaError::ValidationError(
            "El nombre del servicio no puede estar vacío".into(),
        ));
    }
    if !is_valid_tor_service_name(name) {
        return Err(EnolaError::ValidationError(format!(
            "Nombre de servicio inválido: '{}'\n\
             Reglas: solo minúsculas, dígitos, guiones y guiones bajos; empieza con letra o dígito; máx 80 chars.\n\
             Ejemplo válido: ai_mi-instancia",
            name
        )));
    }
    Ok(())
}

/// SEC-M10 (D6): usuario de sistema del proceso Forgejo (`USER_UID`/`USER_GID`).
pub const GIT_SERVICE_USER: &str = "enola-git";
/// SEC-M10 (D6): usuario de sistema del proceso Ghost (`--user`).
pub const GHOST_SERVICE_USER: &str = "enola-ghost";
/// SEC-M10: directorio root-only con el hash admin de cada servidor Forgejo.
/// Fuera de `/srv/enola-git/<name>`, que el contenedor monta en `/data`.
pub const GIT_ADMIN_CREDS_DIR: &str = "/etc/enola/git";

/// Ruta del hash admin del servidor Forgejo `server` (ver [`GIT_ADMIN_CREDS_DIR`]).
pub fn git_admin_creds_path(server: &str) -> std::path::PathBuf {
    std::path::Path::new(GIT_ADMIN_CREDS_DIR).join(format!("{server}.creds"))
}

/// Service types supported by Enola
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceType {
    Git,
    WordPress,
    Drupal,
    Ghost,
    Magnolia,
    Strapi,
    Wagtail,
    StaticSite,
    FileServer,
    Proxy,
    Raw,
    Ai,
}

impl ServiceType {
    /// Todas las variantes de `ServiceType` (D16).
    ///
    /// Fuente única de verdad para iterar tipos en `from_tor_dir`,
    /// `from_tor_config` y `possible_names_for_lookup`. Al añadir una
    /// variante al enum hay que incluirla aquí — el test
    /// `test_service_type_all_covers_every_variant` lo exige.
    pub const ALL: [ServiceType; 12] = [
        ServiceType::Git,
        ServiceType::WordPress,
        ServiceType::Drupal,
        ServiceType::Ghost,
        ServiceType::Magnolia,
        ServiceType::Strapi,
        ServiceType::Wagtail,
        ServiceType::StaticSite,
        ServiceType::FileServer,
        ServiceType::Proxy,
        ServiceType::Raw,
        ServiceType::Ai,
    ];

    /// Get the prefix used for Tor hidden service directories
    /// e.g., /var/lib/tor/enola_git_{name}
    pub fn tor_dir_prefix(&self) -> &'static str {
        match self {
            ServiceType::Git => "enola_git_",
            ServiceType::WordPress => "enola_wp_",
            ServiceType::Drupal => "enola_drupal_",
            ServiceType::Ghost => "enola_ghost_",
            ServiceType::Magnolia => "enola_magnolia_",
            ServiceType::Strapi => "enola_strapi_",
            ServiceType::Wagtail => "enola_wagtail_",
            ServiceType::StaticSite => "enola_static_",
            ServiceType::FileServer => "enola_fileserver_",
            ServiceType::Proxy => "enola_proxy_",
            ServiceType::Raw => "enola_raw_",
            ServiceType::Ai => "enola_ai_",
        }
    }

    /// Get the prefix used for Tor configuration files
    /// e.g., /etc/tor/enola.d/git_{name}.conf
    pub fn tor_config_prefix(&self) -> &'static str {
        match self {
            ServiceType::Git => "git_",
            ServiceType::WordPress => "wp_",
            ServiceType::Drupal => "drupal_",
            ServiceType::Ghost => "ghost_",
            ServiceType::Magnolia => "magnolia_",
            ServiceType::Strapi => "strapi_",
            ServiceType::Wagtail => "wagtail_",
            ServiceType::StaticSite => "static_",
            ServiceType::FileServer => "fileserver_",
            ServiceType::Proxy => "proxy_",
            ServiceType::Raw => "raw_",
            ServiceType::Ai => "ai_",
        }
    }

    /// Get the prefix used for Docker containers
    /// e.g., enola-git-{name}
    pub fn docker_prefix(&self) -> Option<&'static str> {
        match self {
            ServiceType::Git => Some("enola-git-"),
            ServiceType::WordPress => Some("wp-"),
            ServiceType::Drupal => Some("drupal-"),
            ServiceType::Ghost => Some("ghost-"),
            ServiceType::Magnolia => Some("magnolia-"),
            ServiceType::Strapi => Some("strapi-"),
            ServiceType::Wagtail => Some("wagtail-"),
            ServiceType::StaticSite => None,
            ServiceType::FileServer => None,
            ServiceType::Proxy => None,
            ServiceType::Raw => None,
            ServiceType::Ai => Some("enola-ai-"),
        }
    }

    /// Get the prefix used for Nginx configuration files
    /// e.g., /etc/nginx/sites-available/proxy_git_{name}
    pub fn nginx_prefix(&self) -> Option<&'static str> {
        match self {
            ServiceType::Git => Some("proxy_git_"),
            ServiceType::WordPress => Some("proxy_wp_"),
            ServiceType::Drupal => Some("proxy_drupal_"),
            ServiceType::Ghost => Some("proxy_ghost_"),
            ServiceType::Magnolia => Some("proxy_magnolia_"),
            ServiceType::Strapi => Some("proxy_strapi_"),
            ServiceType::Wagtail => Some("proxy_wagtail_"),
            ServiceType::StaticSite => Some("static_"),
            ServiceType::FileServer => Some("fileserver_"),
            ServiceType::Proxy => Some("proxy_"),
            ServiceType::Raw => None,
            ServiceType::Ai => Some("proxy_ai_"),
        }
    }

    /// Parse service type from a string
    pub fn parse_from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "git" | "forgejo" => Some(ServiceType::Git),
            "wordpress" | "wp" => Some(ServiceType::WordPress),
            "drupal" => Some(ServiceType::Drupal),
            "ghost" => Some(ServiceType::Ghost),
            "magnolia" => Some(ServiceType::Magnolia),
            "strapi" => Some(ServiceType::Strapi),
            "wagtail" => Some(ServiceType::Wagtail),
            "static" | "site" => Some(ServiceType::StaticSite),
            "files" | "fileserver" => Some(ServiceType::FileServer),
            "proxy" | "web" | "http" => Some(ServiceType::Proxy),
            "raw" | "tcp" => Some(ServiceType::Raw),
            "ai" | "ollama" | "webui" => Some(ServiceType::Ai),
            _ => None,
        }
    }
}

impl fmt::Display for ServiceType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ServiceType::Git => write!(f, "git"),
            ServiceType::WordPress => write!(f, "wordpress"),
            ServiceType::Drupal => write!(f, "drupal"),
            ServiceType::Ghost => write!(f, "ghost"),
            ServiceType::Magnolia => write!(f, "magnolia"),
            ServiceType::Strapi => write!(f, "strapi"),
            ServiceType::Wagtail => write!(f, "wagtail"),
            ServiceType::StaticSite => write!(f, "static"),
            ServiceType::FileServer => write!(f, "fileserver"),
            ServiceType::Proxy => write!(f, "proxy"),
            ServiceType::Raw => write!(f, "raw"),
            ServiceType::Ai => write!(f, "ai"),
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
        for service_type in ServiceType::ALL {
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
        for service_type in ServiceType::ALL {
            let prefix = service_type.tor_config_prefix();
            if config_name.starts_with(prefix) {
                let base_name = config_name.strip_prefix(prefix)?;
                return Some(Self::new(base_name, service_type));
            }
        }
        None
    }

    /// Resuelve `(domain, instance)` al nombre canónico del servicio Tor (D10).
    ///
    /// `domain` acepta los alias de [`ServiceType::parse_from_str`]
    /// (`"git"`, `"wp"`, `"drupal"`, `"files"`, `"ai"`, `"web"`, ...).
    /// Devuelve `None` si el dominio no corresponde a un `ServiceType`.
    ///
    /// ```text
    /// resolve("ai", "dev")  -> Some("ai_dev")
    /// resolve("wp", "blog") -> Some("wp_blog")
    /// ```
    pub fn resolve(domain: &str, instance: &str) -> Option<String> {
        ServiceType::parse_from_str(domain)
            .map(|service_type| Self::new(instance, service_type).tor_config_name())
    }

    /// Try to find a service by base name, checking all possible prefixes
    /// Returns the list of possible names to search for
    pub fn possible_names_for_lookup(base_name: &str) -> Vec<String> {
        let mut names = vec![base_name.to_string()];

        for service_type in ServiceType::ALL {
            names.push(format!("{}{}", service_type.tor_config_prefix(), base_name));
        }

        names
    }

    /// Resuelve el nombre de servicio que escribe el usuario contra los
    /// servicios Tor existentes (`existing`), sin depender de su orden:
    ///
    /// 1. Coincidencia exacta → ese servicio (el usuario dio el nombre real).
    /// 2. Si no, variantes de [`Self::possible_names_for_lookup`]: una sola
    ///    coincidencia → ese servicio; varias → [`ServiceLookup::Ambiguous`] con
    ///    todas, ordenadas por tipo (orden de las variantes) y sin duplicados,
    ///    para que el usuario elija. Nunca se escoge una al azar.
    pub fn lookup_service(input: &str, existing: &[&str]) -> ServiceLookup {
        if existing.contains(&input) {
            return ServiceLookup::Found(input.to_string());
        }
        let mut matches: Vec<String> = Vec::new();
        for candidate in Self::possible_names_for_lookup(input) {
            if existing.contains(&candidate.as_str()) && !matches.contains(&candidate) {
                matches.push(candidate);
            }
        }
        match matches.len() {
            0 => ServiceLookup::NotFound,
            1 => ServiceLookup::Found(matches.remove(0)),
            _ => ServiceLookup::Ambiguous(matches),
        }
    }
}

/// Resultado de [`find_published`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublishedLookup {
    /// El servicio canónico (`<dominio>_<name>`) existe: su nombre.
    Found(String),
    /// No está publicado.
    NotPublished,
}

/// Localiza el servicio publicado de `base` en el dominio `kind` **solo** por
/// su nombre canónico, sin probar variantes (que podrían ser de otro servicio,
/// p.ej. `proxy_<n>` de `tor create web`). Lo usan `hide`, `edit` y `status`
/// de cada dominio para que resuelvan lo mismo que crea `publish`.
pub fn find_published(base: &str, kind: ServiceType, existing: &[&str]) -> PublishedLookup {
    let canonical = ServiceName::new(base, kind).tor_config_name();
    if existing.contains(&canonical.as_str()) {
        PublishedLookup::Found(canonical)
    } else {
        PublishedLookup::NotPublished
    }
}

/// Resultado de [`ServiceName::lookup_service`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceLookup {
    /// Un único servicio coincide: su nombre real en Tor.
    Found(String),
    /// Ningún servicio coincide.
    NotFound,
    /// Varios servicios coinciden, ordenados por tipo: el usuario debe elegir.
    Ambiguous(Vec<String>),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_published_uses_only_the_canonical_name() {
        // `wp publish blog` crea `wp_blog`: es lo único que se usa.
        assert_eq!(
            find_published("blog", ServiceType::WordPress, &["proxy_blog", "wp_blog"]),
            PublishedLookup::Found("wp_blog".into())
        );
        assert_eq!(
            find_published("repo", ServiceType::Git, &["git_repo"]),
            PublishedLookup::Found("git_repo".into())
        );
        // Un `proxy_blog` (posible `tor create web blog`) nunca se toma por el WP.
        assert_eq!(
            find_published("blog", ServiceType::WordPress, &["proxy_blog"]),
            PublishedLookup::NotPublished
        );
        assert_eq!(
            find_published("repo", ServiceType::Git, &["otro"]),
            PublishedLookup::NotPublished
        );
    }

    #[test]
    fn lookup_service_exact_name_wins_even_if_variants_exist() {
        // El usuario escribió el nombre real: no hay ambigüedad posible.
        let existing = ["wp_dev", "git_dev", "dev"];
        assert_eq!(
            ServiceName::lookup_service("wp_dev", &existing),
            ServiceLookup::Found("wp_dev".to_string())
        );
        assert_eq!(
            ServiceName::lookup_service("dev", &existing),
            ServiceLookup::Found("dev".to_string())
        );
    }

    #[test]
    fn lookup_service_single_variant_is_found() {
        let existing = ["wp_blog", "git_repo"];
        assert_eq!(
            ServiceName::lookup_service("repo", &existing),
            ServiceLookup::Found("git_repo".to_string())
        );
    }

    #[test]
    fn lookup_service_multiple_variants_is_ambiguous_and_ordered_by_type() {
        // Nunca se elige una al azar (ni por el orden en que Tor las liste):
        // se devuelven todas, ordenadas por tipo y sin duplicados.
        let existing = ["wp_dev", "proxy_dev", "git_dev"];
        assert_eq!(
            ServiceName::lookup_service("dev", &existing),
            ServiceLookup::Ambiguous(vec![
                "git_dev".to_string(),
                "wp_dev".to_string(),
                "proxy_dev".to_string(),
            ])
        );
    }

    #[test]
    fn lookup_service_not_found() {
        assert_eq!(
            ServiceName::lookup_service("nada", &["wp_dev"]),
            ServiceLookup::NotFound
        );
    }

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
        assert!(names.contains(&"drupal_myservice".to_string()));
        assert!(names.contains(&"ghost_myservice".to_string()));
        assert!(names.contains(&"magnolia_myservice".to_string()));
        assert!(names.contains(&"strapi_myservice".to_string()));
        assert!(names.contains(&"wagtail_myservice".to_string()));
        // Sin variantes antiguas (`ai-<n>`, `proxy_ai-<n>`).
        assert!(!names.contains(&"ai-myservice".to_string()));
    }

    #[test]
    fn test_service_name_ai() {
        let parsed_type = ServiceType::parse_from_str("ai").expect("ServiceType::Ai");
        let name = ServiceName::new("dev", parsed_type);
        assert_eq!(name.tor_dir_name(), "enola_ai_dev");
        assert_eq!(name.tor_config_name(), "ai_dev");
        assert_eq!(
            name.docker_container_name(),
            Some("enola-ai-dev".to_string())
        );
        assert_eq!(name.nginx_config_name(), Some("proxy_ai_dev".to_string()));
        let lookup = ServiceName::possible_names_for_lookup("dev");
        assert!(lookup.contains(&"ai_dev".to_string()));
    }

    // ── MEGAPLAN-PUBLISH-HARMONIZATION: nomenclatura canónica (D3/D10/D16) ──

    #[test]
    fn test_service_type_cms_prefixes() {
        // D3: prefijo por backend para los 5 CMS
        let cases: [(ServiceType, &str, &str, &str, &str); 5] = [
            (
                ServiceType::Drupal,
                "enola_drupal_",
                "drupal_",
                "proxy_drupal_",
                "drupal-",
            ),
            (
                ServiceType::Ghost,
                "enola_ghost_",
                "ghost_",
                "proxy_ghost_",
                "ghost-",
            ),
            (
                ServiceType::Magnolia,
                "enola_magnolia_",
                "magnolia_",
                "proxy_magnolia_",
                "magnolia-",
            ),
            (
                ServiceType::Strapi,
                "enola_strapi_",
                "strapi_",
                "proxy_strapi_",
                "strapi-",
            ),
            (
                ServiceType::Wagtail,
                "enola_wagtail_",
                "wagtail_",
                "proxy_wagtail_",
                "wagtail-",
            ),
        ];
        for (ty, dir_p, cfg_p, ngx_p, docker_p) in cases {
            let name = ServiceName::new("sitio", ty);
            assert_eq!(name.tor_dir_name(), format!("{}sitio", dir_p));
            assert_eq!(name.tor_config_name(), format!("{}sitio", cfg_p));
            assert_eq!(name.nginx_config_name(), Some(format!("{}sitio", ngx_p)));
            assert_eq!(
                name.docker_container_name(),
                Some(format!("{}sitio", docker_p))
            );
        }
    }

    #[test]
    fn test_service_type_parse_and_display_cms() {
        for (s, ty) in [
            ("drupal", ServiceType::Drupal),
            ("ghost", ServiceType::Ghost),
            ("magnolia", ServiceType::Magnolia),
            ("strapi", ServiceType::Strapi),
            ("wagtail", ServiceType::Wagtail),
        ] {
            assert_eq!(ServiceType::parse_from_str(s), Some(ty));
            assert_eq!(ty.to_string(), s);
        }
    }

    #[test]
    fn test_service_type_all_covers_every_variant() {
        // D16: ALL es la fuente única de verdad de variantes
        assert_eq!(ServiceType::ALL.len(), 12);
        for ty in [
            ServiceType::Git,
            ServiceType::WordPress,
            ServiceType::Drupal,
            ServiceType::Ghost,
            ServiceType::Magnolia,
            ServiceType::Strapi,
            ServiceType::Wagtail,
            ServiceType::StaticSite,
            ServiceType::FileServer,
            ServiceType::Proxy,
            ServiceType::Raw,
            ServiceType::Ai,
        ] {
            assert!(ServiceType::ALL.contains(&ty), "ALL debe contener {:?}", ty);
        }
    }

    #[test]
    fn test_from_tor_dir_and_config_cms() {
        let parsed = ServiceName::from_tor_dir("enola_drupal_blog").unwrap();
        assert_eq!(parsed.base_name, "blog");
        assert_eq!(parsed.service_type, ServiceType::Drupal);

        let parsed = ServiceName::from_tor_config("wagtail_site").unwrap();
        assert_eq!(parsed.base_name, "site");
        assert_eq!(parsed.service_type, ServiceType::Wagtail);
    }

    #[test]
    fn test_service_name_resolve() {
        // D10: resolución amigable (domain, instance) → nombre canónico
        assert_eq!(
            ServiceName::resolve("ai", "dev"),
            Some("ai_dev".to_string())
        );
        assert_eq!(
            ServiceName::resolve("git", "repo"),
            Some("git_repo".to_string())
        );
        assert_eq!(
            ServiceName::resolve("wp", "blog"),
            Some("wp_blog".to_string())
        );
        assert_eq!(
            ServiceName::resolve("wordpress", "blog"),
            Some("wp_blog".to_string())
        );
        assert_eq!(
            ServiceName::resolve("drupal", "site"),
            Some("drupal_site".to_string())
        );
        assert_eq!(
            ServiceName::resolve("files", "share"),
            Some("fileserver_share".to_string())
        );
        assert_eq!(
            ServiceName::resolve("web", "proxy1"),
            Some("proxy_proxy1".to_string())
        );
        assert_eq!(ServiceName::resolve("bogus", "x"), None);
    }

    #[test]
    fn test_validate_tor_service_name_accepts_canonical() {
        // Nombres canónicos `<dominio>_<name>` y nombres planos válidos
        for name in [
            "ai_dev",
            "drupal_blog",
            "fileserver_uitest-files",
            "proxy_ai_dev",
            "proxy_web1",
            "git_my-repo",
            "wagtail_x",
            "raw_tcp1",
            "a",
        ] {
            assert!(
                validate_tor_service_name(name).is_ok(),
                "debe aceptar: {name}"
            );
        }
    }

    #[test]
    fn test_validate_tor_service_name_rejects_unsafe() {
        // Mismas defensas que validate_service_name: metacaracteres, traversal,
        // mayúsculas, espacios, vacío, líder no alfanumérico.
        for name in [
            "",
            "../etc",
            "/abs",
            "a/b",
            "Bad_Name",
            "name;rm",
            "name space",
            "_leading",
            "-leading",
            &"a".repeat(81),
        ] {
            assert!(
                validate_tor_service_name(name).is_err(),
                "debe rechazar: {name:?}"
            );
        }
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
