// DRUPAL-001 (2026-04-29) — Adapters concretos del catálogo CMS.
//
// Cada CMS soportado vive en su propio módulo (`wordpress.rs`, `drupal.rs`,
// `ghost.rs`…) e implementa `crate::ports::cms::CmsAdapter` y opcionalmente
// `CmsLifecycle`.
//
// El registro vive aquí para que la CLI y futura UI puedan iterar sobre
// el catálogo sin acoplarse a cada adapter.

pub mod drupal; // DRUPAL-002 — Adapter Drupal con CmsLifecycle completo
pub mod ghost; // CMS-GHOST-001 — Adapter Ghost (SQLite, sin contenedor BD)
pub mod magnolia;
pub mod strapi; // CMS-STRAPI-001 — Adapter Strapi v4 (Node.js headless + Postgres)
pub mod wagtail; // CMS-WAGTAIL-001 — Adapter Wagtail (Python/Django + Postgres)
pub mod wordpress; // CMS-MAGNOLIA-001 — Adapter Magnolia Community (Java/Tomcat + H2 embebido)

use crate::domain::cms::{CmsDescriptor, CmsKind};
use crate::ports::cms::CmsAdapter;

/// Reads the Tor onion address for a given Tor service name.
///
/// Tor service directories are at `/var/lib/tor/enola_{tor_service_name}/hostname`.
/// Returns `None` if the file doesn't exist (service not published on Tor).
pub fn read_onion_address(tor_service_name: &str) -> Option<String> {
    let hostname_path = format!("/var/lib/tor/enola_{}/hostname", tor_service_name);
    std::fs::read_to_string(&hostname_path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Devuelve el catálogo de adapters CMS registrados (descriptors únicamente,
/// sin instanciar lifecycle). Útil para `cms list` y la web de docs.
///
/// Mantener ordenado por nombre para output estable.
pub fn catalog_descriptors() -> Vec<CmsDescriptor> {
    // Nota: para descriptors basta con structs zero-sized aunque algunos
    // adapters reales (Drupal, Ghost, Wagtail, futuras impls) lleven dependencias
    // inyectadas para su CmsLifecycle. Aquí solo necesitamos la metadata.
    let descriptors: Vec<CmsDescriptor> = vec![
        wordpress::WordPressCmsAdapter.descriptor(),
        drupal_descriptor_static(),
        ghost::ghost_descriptor(),
        magnolia::magnolia_descriptor(),
        strapi::strapi_descriptor(),
        wagtail::wagtail_descriptor(),
    ];
    let mut out = descriptors;
    out.sort_by_key(|d| d.kind.slug());
    out
}

/// Descriptor estático de Drupal (sin necesidad de instanciar el adapter
/// completo, que requiere `Arc<dyn ContainerPort>`).
///
/// Debe permanecer sincronizado con `DrupalCmsAdapter::descriptor()`. El test
/// `drupal_descriptor_static_matches_adapter` lo verifica.
fn drupal_descriptor_static() -> CmsDescriptor {
    CmsDescriptor {
        kind: CmsKind::Drupal,
        display_name: "Drupal",
        default_image: "drupal:10-apache",
        db_stack: crate::domain::cms::DbStack::MariaDB,
        // §13.1: Drupal wizard web acepta 200/301/302/304/403 hasta completarse.
        // 403 puede ocurrir por permisos o configuración Apache inicial (TEST-COV-DRUPAL-019).
        // 500 incluido por simetría con WP (errores transitorios durante boot DB).
        setup_wizard_status_codes: &[200, 301, 302, 304, 403, 500],
        container_prefix: "drupal-",
        data_root: "/srv/enola-drupal",
        http_port_range: (8000, 9999),
    }
}

/// Localiza un descriptor por kind. `None` si el CMS aún no está registrado.
pub fn descriptor_by_kind(kind: CmsKind) -> Option<CmsDescriptor> {
    catalog_descriptors().into_iter().find(|d| d.kind == kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_contains_wordpress() {
        let d = descriptor_by_kind(CmsKind::Wordpress);
        assert!(d.is_some(), "WordPress must be registered");
        let d = d.unwrap();
        assert_eq!(d.container_prefix, "wp-");
        assert_eq!(d.data_root, "/srv/enola-wordpress");
    }

    #[test]
    fn catalog_descriptors_are_sorted_and_unique() {
        let descriptors = catalog_descriptors();
        let slugs: Vec<&str> = descriptors.iter().map(|d| d.kind.slug()).collect();
        let mut sorted = slugs.clone();
        sorted.sort();
        assert_eq!(slugs, sorted, "catalog must be sorted by slug");

        let mut deduped = slugs.clone();
        deduped.dedup();
        assert_eq!(slugs.len(), deduped.len(), "no duplicate kinds");
    }

    #[test]
    fn catalog_contains_drupal() {
        let d = descriptor_by_kind(CmsKind::Drupal).expect("Drupal must be registered");
        assert_eq!(d.default_image, "drupal:10-apache");
        assert_eq!(d.container_prefix, "drupal-");
    }

    #[test]
    fn drupal_descriptor_static_matches_adapter() {
        // Garantía: el descriptor estático del catálogo NO se desincroniza del
        // que devuelve DrupalCmsAdapter::descriptor() en runtime.
        use crate::ports::container::MockContainerPort;
        use crate::ports::manifest::MockManifestPort;
        use std::sync::Arc;
        let mock = MockContainerPort::new();
        let adapter =
            drupal::DrupalCmsAdapter::new(Arc::new(mock), Arc::new(MockManifestPort::new()));
        let from_adapter = adapter.descriptor();
        let from_static = drupal_descriptor_static();
        assert_eq!(from_adapter.kind, from_static.kind);
        assert_eq!(from_adapter.default_image, from_static.default_image);
        assert_eq!(from_adapter.db_stack, from_static.db_stack);
        assert_eq!(from_adapter.container_prefix, from_static.container_prefix);
        assert_eq!(from_adapter.data_root, from_static.data_root);
        assert_eq!(from_adapter.http_port_range, from_static.http_port_range);
        assert_eq!(
            from_adapter.setup_wizard_status_codes,
            from_static.setup_wizard_status_codes
        );
    }

    #[test]
    fn catalog_contains_ghost() {
        // CMS-GHOST-001: Ghost registrado con SQLite (sin BD externa).
        let d = descriptor_by_kind(CmsKind::Ghost).expect("Ghost must be registered");
        assert_eq!(d.default_image, "ghost:5-alpine");
        assert_eq!(d.container_prefix, "ghost-");
        assert_eq!(d.data_root, "/srv/enola-ghost");
        assert!(
            !d.requires_db(),
            "Ghost SQLite must NOT require external DB"
        );
    }

    #[test]
    fn ghost_descriptor_static_matches_adapter() {
        // Garantía: el descriptor estático del catálogo NO se desincroniza del
        // que devuelve GhostCmsAdapter::descriptor() en runtime.
        use crate::ports::container::MockContainerPort;
        use crate::ports::manifest::MockManifestPort;
        use std::sync::Arc;
        let mock = MockContainerPort::new();
        let adapter =
            ghost::GhostCmsAdapter::new(Arc::new(mock), Arc::new(MockManifestPort::new()));
        let from_adapter = adapter.descriptor();
        let from_static = ghost::ghost_descriptor();
        assert_eq!(from_adapter.kind, from_static.kind);
        assert_eq!(from_adapter.default_image, from_static.default_image);
        assert_eq!(from_adapter.db_stack, from_static.db_stack);
        assert_eq!(from_adapter.container_prefix, from_static.container_prefix);
        assert_eq!(from_adapter.data_root, from_static.data_root);
        assert_eq!(from_adapter.http_port_range, from_static.http_port_range);
        assert_eq!(
            from_adapter.setup_wizard_status_codes,
            from_static.setup_wizard_status_codes
        );
    }

    #[test]
    fn catalog_contains_wagtail() {
        // CMS-WAGTAIL-001: Wagtail registrado con Postgres (primer Postgres del catálogo).
        let d = descriptor_by_kind(CmsKind::Wagtail).expect("Wagtail must be registered");
        assert_eq!(d.default_image, "wagtail/bakerydemo:latest");
        assert_eq!(d.container_prefix, "wagtail-");
        assert_eq!(d.data_root, "/srv/enola-wagtail");
        assert_eq!(d.db_stack, crate::domain::cms::DbStack::Postgres);
        assert!(d.requires_db(), "Wagtail+Postgres requires external DB");
    }

    #[test]
    fn wagtail_descriptor_static_matches_adapter() {
        // Garantía: el descriptor estático del catálogo NO se desincroniza del
        // que devuelve WagtailCmsAdapter::descriptor() en runtime.
        use crate::ports::container::MockContainerPort;
        use crate::ports::manifest::MockManifestPort;
        use std::sync::Arc;
        let mock = MockContainerPort::new();
        let adapter =
            wagtail::WagtailCmsAdapter::new(Arc::new(mock), Arc::new(MockManifestPort::new()));
        let from_adapter = adapter.descriptor();
        let from_static = wagtail::wagtail_descriptor();
        assert_eq!(from_adapter.kind, from_static.kind);
        assert_eq!(from_adapter.default_image, from_static.default_image);
        assert_eq!(from_adapter.db_stack, from_static.db_stack);
        assert_eq!(from_adapter.container_prefix, from_static.container_prefix);
        assert_eq!(from_adapter.data_root, from_static.data_root);
        assert_eq!(from_adapter.http_port_range, from_static.http_port_range);
        assert_eq!(
            from_adapter.setup_wizard_status_codes,
            from_static.setup_wizard_status_codes
        );
    }

    #[test]
    fn catalog_contains_strapi() {
        // CMS-STRAPI-001: Strapi v5 registrado con Postgres (Node.js headless).
        let d = descriptor_by_kind(CmsKind::Strapi).expect("Strapi must be registered");
        assert_eq!(d.default_image, "enola/strapi:5.49.0");
        assert_eq!(d.container_prefix, "strapi-");
        assert_eq!(d.data_root, "/srv/enola-strapi");
        assert_eq!(d.db_stack, crate::domain::cms::DbStack::Postgres);
        assert!(d.requires_db(), "Strapi+Postgres requires external DB");
    }

    #[test]
    fn strapi_descriptor_static_matches_adapter() {
        // Garantía: el descriptor estático del catálogo NO se desincroniza del
        // que devuelve StrapiCmsAdapter::descriptor() en runtime.
        use crate::ports::container::MockContainerPort;
        use crate::ports::manifest::MockManifestPort;
        use std::sync::Arc;
        let mock = MockContainerPort::new();
        let adapter =
            strapi::StrapiCmsAdapter::new(Arc::new(mock), Arc::new(MockManifestPort::new()));
        let from_adapter = adapter.descriptor();
        let from_static = strapi::strapi_descriptor();
        assert_eq!(from_adapter.kind, from_static.kind);
        assert_eq!(from_adapter.default_image, from_static.default_image);
        assert_eq!(from_adapter.db_stack, from_static.db_stack);
        assert_eq!(from_adapter.container_prefix, from_static.container_prefix);
        assert_eq!(from_adapter.data_root, from_static.data_root);
        assert_eq!(from_adapter.http_port_range, from_static.http_port_range);
        assert_eq!(
            from_adapter.setup_wizard_status_codes,
            from_static.setup_wizard_status_codes
        );
    }

    #[test]
    fn catalog_contains_magnolia() {
        // CMS-MAGNOLIA-001: Magnolia registrado con H2 embebido (sin BD externa).
        let d = descriptor_by_kind(CmsKind::Magnolia).expect("Magnolia must be registered");
        assert_eq!(
            d.default_image,
            "ghcr.io/magnolia-sre/magnolia-docker/magnolia-docker:latest"
        );
        assert_eq!(d.container_prefix, "magnolia-");
        assert_eq!(d.data_root, "/srv/enola-magnolia");
        assert_eq!(d.db_stack, crate::domain::cms::DbStack::None);
        assert!(
            !d.requires_db(),
            "Magnolia+H2 embedded does NOT require external DB"
        );
    }

    #[test]
    fn magnolia_descriptor_static_matches_adapter() {
        // Garantía: el descriptor estático del catálogo NO se desincroniza del
        // que devuelve MagnoliaCmsAdapter::descriptor() en runtime.
        use crate::ports::container::MockContainerPort;
        use crate::ports::manifest::MockManifestPort;
        use std::sync::Arc;
        let mock = MockContainerPort::new();
        let adapter =
            magnolia::MagnoliaCmsAdapter::new(Arc::new(mock), Arc::new(MockManifestPort::new()));
        let from_adapter = adapter.descriptor();
        let from_static = magnolia::magnolia_descriptor();
        assert_eq!(from_adapter.kind, from_static.kind);
        assert_eq!(from_adapter.default_image, from_static.default_image);
        assert_eq!(from_adapter.db_stack, from_static.db_stack);
        assert_eq!(from_adapter.container_prefix, from_static.container_prefix);
        assert_eq!(from_adapter.data_root, from_static.data_root);
        assert_eq!(from_adapter.http_port_range, from_static.http_port_range);
        assert_eq!(
            from_adapter.setup_wizard_status_codes,
            from_static.setup_wizard_status_codes
        );
    }
}
