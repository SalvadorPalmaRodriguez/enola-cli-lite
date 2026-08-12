// DRUPAL-001 (2026-04-29) — Tipos de dominio del catálogo CMS.
//
// Define el contrato neutral entre los CMS soportados (WordPress, Drupal, Ghost,
// Strapi, Wagtail, Magnolia, etc.). Los adapters concretos viven en
// `src/adapters/cms/` y deben implementar los traits de `src/ports/cms.rs` sobre
// estos tipos.
//
// Reglas permanentes
//   - `DbStack` se incluye desde el día 1 (no diferir a refactor posterior).
//   - Cualquier CMS nuevo añade UNA variante a `CmsKind` + UN descriptor estático.
//   - Los descriptores son `pub const` o función `const` siempre que sea posible.

use std::fmt;

/// Stack de base de datos requerido por un CMS.
///
/// El valor `None` indica que el CMS funciona sin BD externa (p.ej. Hugo estático
/// o sites SQLite embebido manejado por el propio contenedor).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DbStack {
    /// MySQL/MariaDB. Imagen por defecto: `mariadb:10.11`.
    MariaDB,
    /// PostgreSQL. Imagen por defecto: `postgres:16-alpine`.
    Postgres,
    /// SQLite embebido en el contenedor del CMS (sin BD externa).
    Sqlite,
    /// MongoDB (uso raro: KeystoneJS legacy, Strapi opcional).
    MongoDB,
    /// Sin BD (sites estáticos o CMS que la integran internamente sin exponerla).
    None,
}

impl DbStack {
    /// Imagen Docker por defecto para este stack, o `None` si no aplica BD externa.
    pub fn default_image(self) -> Option<&'static str> {
        match self {
            DbStack::MariaDB => Some("mariadb:10.11"),
            DbStack::Postgres => Some("postgres:16-alpine"),
            DbStack::MongoDB => Some("mongo:7"),
            DbStack::Sqlite | DbStack::None => None,
        }
    }

    /// `true` si el stack requiere un contenedor de BD adicional.
    pub fn requires_external_container(self) -> bool {
        matches!(
            self,
            DbStack::MariaDB | DbStack::Postgres | DbStack::MongoDB
        )
    }
}

impl fmt::Display for DbStack {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            DbStack::MariaDB => "mariadb",
            DbStack::Postgres => "postgres",
            DbStack::Sqlite => "sqlite",
            DbStack::MongoDB => "mongodb",
            DbStack::None => "none",
        };
        f.write_str(s)
    }
}

/// Identidad estable del CMS dentro del catálogo (1 variante = 1 CMS).
///
/// Añadir un CMS = añadir UNA variante aquí + UN descriptor en su adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CmsKind {
    Wordpress,
    Drupal,
    Ghost,
    Strapi,
    Wagtail,
    Magnolia,
    DotCms,
    DjangoCms,
    Keystone,
    OpenCms,
}

impl CmsKind {
    /// Identificador kebab-case usado en CLI, paths y logs.
    pub fn slug(self) -> &'static str {
        match self {
            CmsKind::Wordpress => "wordpress",
            CmsKind::Drupal => "drupal",
            CmsKind::Ghost => "ghost",
            CmsKind::Strapi => "strapi",
            CmsKind::Wagtail => "wagtail",
            CmsKind::Magnolia => "magnolia",
            CmsKind::DotCms => "dotcms",
            CmsKind::DjangoCms => "django-cms",
            CmsKind::Keystone => "keystone",
            CmsKind::OpenCms => "opencms",
        }
    }
}

impl fmt::Display for CmsKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.slug())
    }
}

/// Metadata estática de un CMS en el catálogo. Datos puros, sin I/O.
///
/// Cualquier adapter (WordPress, Drupal, Ghost…) expone esto vía
/// `CmsAdapter::descriptor()`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CmsDescriptor {
    /// Identidad enum (clave estable).
    pub kind: CmsKind,
    /// Nombre legible para humanos ("WordPress", "Drupal", "Ghost").
    pub display_name: &'static str,
    /// Imagen Docker por defecto (p.ej. `wordpress:6-php8.2-apache`).
    pub default_image: &'static str,
    /// Stack de BD requerido. Si != `None`, el adapter debe gestionar el
    /// contenedor de BD vinculado.
    pub db_stack: DbStack,
    /// Códigos HTTP que el setup wizard devuelve antes de completarse
    /// manualmente. Los tests E2E DEBEN aceptarlos como PASS (§13.1).
    /// Vacío si el CMS no tiene wizard web.
    pub setup_wizard_status_codes: &'static [u16],
    /// Prefijo del contenedor principal del CMS (p.ej. `wp-`, `drupal-`,
    /// `ghost-`). El nombre final = `{prefix}{instance_name}`.
    pub container_prefix: &'static str,
    /// Directorio base donde el adapter guarda datos persistentes
    /// (`/srv/enola-{slug}/{name}/`).
    pub data_root: &'static str,
    /// Rango de puertos HTTP que el adapter puede asignar dinámicamente.
    pub http_port_range: (u16, u16),
}

impl CmsDescriptor {
    /// Conveniencia: ¿requiere un contenedor de BD adicional?
    pub fn requires_db(&self) -> bool {
        self.db_stack.requires_external_container()
    }
}

/// Petición genérica de creación de una instancia CMS.
///
/// Los adapters concretos pueden ignorar campos que no apliquen a su CMS.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CmsCreateRequest {
    /// Nombre lógico de la instancia (slug, sin prefijo). Ej: `myblog`.
    pub name: String,
    /// Puerto HTTP a forzar. `None` ⇒ el adapter asigna uno libre del rango.
    pub http_port: Option<u16>,
    /// Password de BD a forzar. `None` ⇒ el adapter genera uno aleatorio.
    pub db_password: Option<String>,
}

/// Estado runtime de una instancia CMS, devuelto por `status()`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CmsInstance {
    pub kind: CmsKind,
    pub name: String,
    pub status: CmsStatus,
    /// Puerto HTTP local (`127.0.0.1:{http_port}`).
    pub http_port: Option<u16>,
    /// Puerto BD local si aplica.
    pub db_port: Option<u16>,
    /// `.onion` publicado (si está expuesto vía Tor).
    pub onion_address: Option<String>,
}

/// Estado de ciclo de vida.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CmsStatus {
    Running,
    Stopped,
    NotFound,
    /// El contenedor existe pero no responde aún (setup wizard pendiente).
    Initializing,
}

impl fmt::Display for CmsStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            CmsStatus::Running => "running",
            CmsStatus::Stopped => "stopped",
            CmsStatus::NotFound => "not_found",
            CmsStatus::Initializing => "initializing",
        };
        f.write_str(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_stack_default_images_are_consistent() {
        assert_eq!(DbStack::MariaDB.default_image(), Some("mariadb:10.11"));
        assert_eq!(
            DbStack::Postgres.default_image(),
            Some("postgres:16-alpine")
        );
        assert_eq!(DbStack::MongoDB.default_image(), Some("mongo:7"));
        assert_eq!(DbStack::Sqlite.default_image(), None);
        assert_eq!(DbStack::None.default_image(), None);
    }

    #[test]
    fn db_stack_requires_external_container_only_for_real_dbs() {
        assert!(DbStack::MariaDB.requires_external_container());
        assert!(DbStack::Postgres.requires_external_container());
        assert!(DbStack::MongoDB.requires_external_container());
        assert!(!DbStack::Sqlite.requires_external_container());
        assert!(!DbStack::None.requires_external_container());
    }

    #[test]
    fn cms_kind_slugs_are_kebab_case_and_unique() {
        let all = [
            CmsKind::Wordpress,
            CmsKind::Drupal,
            CmsKind::Ghost,
            CmsKind::Strapi,
            CmsKind::Wagtail,
            CmsKind::Magnolia,
            CmsKind::DotCms,
            CmsKind::DjangoCms,
            CmsKind::Keystone,
            CmsKind::OpenCms,
        ];
        let mut slugs: Vec<&str> = all.iter().map(|k| k.slug()).collect();
        slugs.sort();
        let len = slugs.len();
        slugs.dedup();
        assert_eq!(slugs.len(), len, "duplicate CmsKind slugs");
        for s in slugs {
            assert!(!s.is_empty());
            assert!(s.chars().all(|c| c.is_ascii_lowercase() || c == '-'));
        }
    }

    #[test]
    fn descriptor_requires_db_matches_db_stack() {
        let d = CmsDescriptor {
            kind: CmsKind::Wordpress,
            display_name: "WordPress",
            default_image: "wordpress:6-php8.2-apache",
            db_stack: DbStack::MariaDB,
            setup_wizard_status_codes: &[200, 302, 500],
            container_prefix: "wp-",
            data_root: "/srv/enola-wordpress",
            http_port_range: (8000, 9999),
        };
        assert!(d.requires_db());

        let static_site = CmsDescriptor {
            kind: CmsKind::Ghost,
            display_name: "Ghost",
            default_image: "ghost:5-alpine",
            db_stack: DbStack::Sqlite,
            setup_wizard_status_codes: &[200, 302],
            container_prefix: "ghost-",
            data_root: "/srv/enola-ghost",
            http_port_range: (8000, 9999),
        };
        assert!(!static_site.requires_db());
    }

    #[test]
    fn cms_status_renders_lowercase() {
        assert_eq!(CmsStatus::Running.to_string(), "running");
        assert_eq!(CmsStatus::Stopped.to_string(), "stopped");
        assert_eq!(CmsStatus::NotFound.to_string(), "not_found");
        assert_eq!(CmsStatus::Initializing.to_string(), "initializing");
    }

    // TEST-COV-UNIT-002: cubrir Display for DbStack (líneas 51-60 sin cobertura)
    #[test]
    fn db_stack_display_renders_correctly() {
        assert_eq!(DbStack::MariaDB.to_string(), "mariadb");
        assert_eq!(DbStack::Postgres.to_string(), "postgres");
        assert_eq!(DbStack::Sqlite.to_string(), "sqlite");
        assert_eq!(DbStack::MongoDB.to_string(), "mongodb");
        assert_eq!(DbStack::None.to_string(), "none");
    }

    // TEST-COV-UNIT-002: cubrir Display for CmsKind (líneas 99-101 sin cobertura)
    #[test]
    fn cms_kind_display_matches_slug() {
        let all = [
            CmsKind::Wordpress,
            CmsKind::Drupal,
            CmsKind::Ghost,
            CmsKind::Strapi,
            CmsKind::Wagtail,
            CmsKind::Magnolia,
            CmsKind::DotCms,
            CmsKind::DjangoCms,
            CmsKind::Keystone,
            CmsKind::OpenCms,
        ];
        for kind in all {
            assert_eq!(
                kind.to_string(),
                kind.slug(),
                "Display de {:?} debe coincidir con slug()",
                kind
            );
        }
    }

    // TEST-COV-UNIT-002: verificar CmsInstance y CmsCreateRequest se construyen sin panic
    #[test]
    fn cms_instance_fields_are_accessible() {
        let inst = CmsInstance {
            kind: CmsKind::Drupal,
            name: "myblog".to_string(),
            status: CmsStatus::Running,
            http_port: Some(8080),
            db_port: Some(3306),
            onion_address: Some("abc.onion".to_string()),
        };
        assert_eq!(inst.kind, CmsKind::Drupal);
        assert_eq!(inst.name, "myblog");
        assert_eq!(inst.status, CmsStatus::Running);
        assert_eq!(inst.http_port, Some(8080));
    }

    #[test]
    fn cms_create_request_fields_are_accessible() {
        let req = CmsCreateRequest {
            name: "mysite".to_string(),
            http_port: Some(9090),
            db_password: Some("s3cr3t".to_string()),
        };
        assert_eq!(req.name, "mysite");
        assert_eq!(req.http_port, Some(9090));
        assert_eq!(req.db_password.as_deref(), Some("s3cr3t"));

        // Sin puerto ni password (valores None)
        let req2 = CmsCreateRequest {
            name: "minimal".to_string(),
            http_port: None,
            db_password: None,
        };
        assert!(req2.http_port.is_none());
        assert!(req2.db_password.is_none());
    }

    // ── Error-path / edge-case tests ──

    #[test]
    fn cms_status_not_found_display() {
        assert_eq!(CmsStatus::NotFound.to_string(), "not_found");
    }

    #[test]
    fn cms_status_initializing_display() {
        assert_eq!(CmsStatus::Initializing.to_string(), "initializing");
    }

    #[test]
    fn descriptor_with_none_db_stack_does_not_require_db() {
        let d = CmsDescriptor {
            kind: CmsKind::Ghost,
            display_name: "Ghost",
            default_image: "ghost:5-alpine",
            db_stack: DbStack::None,
            setup_wizard_status_codes: &[],
            container_prefix: "ghost-",
            data_root: "/srv/enola-ghost",
            http_port_range: (8000, 9999),
        };
        assert!(!d.requires_db());
    }

    #[test]
    fn descriptor_with_mongodb_requires_db() {
        let d = CmsDescriptor {
            kind: CmsKind::Strapi,
            display_name: "Strapi",
            default_image: "strapi/strapi",
            db_stack: DbStack::MongoDB,
            setup_wizard_status_codes: &[200],
            container_prefix: "strapi-",
            data_root: "/srv/enola-strapi",
            http_port_range: (8000, 9999),
        };
        assert!(d.requires_db());
    }

    #[test]
    fn cms_instance_with_no_ports_and_no_onion() {
        let inst = CmsInstance {
            kind: CmsKind::Drupal,
            name: "unconfigured".to_string(),
            status: CmsStatus::NotFound,
            http_port: None,
            db_port: None,
            onion_address: None,
        };
        assert_eq!(inst.status, CmsStatus::NotFound);
        assert!(inst.http_port.is_none());
        assert!(inst.db_port.is_none());
        assert!(inst.onion_address.is_none());
    }

    #[test]
    fn cms_create_request_with_empty_name() {
        let req = CmsCreateRequest {
            name: "".to_string(),
            http_port: None,
            db_password: None,
        };
        assert_eq!(req.name, "");
    }

    #[test]
    fn db_stack_none_does_not_require_external_container() {
        assert!(!DbStack::None.requires_external_container());
    }

    #[test]
    fn db_stack_sqlite_does_not_require_external_container() {
        assert!(!DbStack::Sqlite.requires_external_container());
    }
}
