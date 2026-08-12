// DRUPAL-001 (2026-04-29) — WordPress descriptor en el catálogo CMS.
//
// Implementación mínima y NO destructiva: solo expone la metadata estática de
// WordPress vía el trait `CmsAdapter`. La lógica lifecycle real (create / start
// / stop / delete / status) sigue viviendo en `src/application/deploy_wordpress.rs`,
// `toggle_wordpress.rs`, etc., sin tocarse en esta tarea.
//
// El trait `CmsLifecycle` se implementará incrementalmente cuando aparezca un
// segundo CMS (DRUPAL-002) que valide la abstracción contra una implementación
// real distinta — evita refactor especulativo sobre WordPress que rompería
// los ~395 tests E2E existentes.
//
// Constantes alineadas con el código existente:
//   - container_prefix `wp-`           → §13.3 (naming) y `toggle_wordpress.rs`
//   - data_root `/srv/enola-wordpress` → §13.2 (paths)  y `deploy_wordpress.rs`
//   - http_port_range 8000-9999        → `domain/wordpress.rs::WordPressPortManager`
//   - setup_wizard codes 200/302/500   → §13.1 (HTTP 500 = wizard pendiente)

use crate::domain::cms::{CmsDescriptor, CmsKind, DbStack};
use crate::ports::cms::CmsAdapter;

/// Adapter zero-sized para registrar WordPress en el catálogo CMS.
#[derive(Debug, Clone, Copy, Default)]
pub struct WordPressCmsAdapter;

impl CmsAdapter for WordPressCmsAdapter {
    fn descriptor(&self) -> CmsDescriptor {
        CmsDescriptor {
            kind: CmsKind::Wordpress,
            display_name: "WordPress",
            default_image: "wordpress:6-php8.2-apache",
            db_stack: DbStack::MariaDB,
            setup_wizard_status_codes: &[200, 301, 302, 304, 500],
            container_prefix: "wp-",
            data_root: "/srv/enola-wordpress",
            http_port_range: (8000, 9999),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wordpress_descriptor_matches_existing_constants() {
        let d = WordPressCmsAdapter.descriptor();
        assert_eq!(d.kind, CmsKind::Wordpress);
        assert_eq!(d.kind.slug(), "wordpress");
        assert_eq!(d.container_prefix, "wp-");
        assert_eq!(d.data_root, "/srv/enola-wordpress");
        assert_eq!(d.http_port_range, (8000, 9999));
        assert_eq!(d.db_stack, DbStack::MariaDB);
        assert!(d.requires_db());
    }

    #[test]
    fn wordpress_setup_wizard_accepts_500_and_302() {
        // §13.1: HTTP 500 y 302 son válidos hasta que el usuario completa
        // el wizard manual. Los tests E2E DEBEN aceptarlos como PASS.
        let codes = WordPressCmsAdapter.setup_wizard_status_codes();
        assert!(codes.contains(&500));
        assert!(codes.contains(&302));
        assert!(codes.contains(&200));
    }

    #[test]
    fn wordpress_default_image_is_lts_php82() {
        assert_eq!(
            WordPressCmsAdapter.default_image(),
            "wordpress:6-php8.2-apache"
        );
    }
}
