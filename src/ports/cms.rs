// DRUPAL-001 (2026-04-29) — Trait `CmsAdapter` + `CmsLifecycle`.
//
// Contrato neutral entre el catálogo de CMS y la capa de aplicación. Cada CMS
// concreto (WordPress, Drupal, Ghost, Strapi, Wagtail, Magnolia…) vive en
// `src/adapters/cms/<slug>.rs` e implementa estos traits.
//
// Diseño en dos traits:
//   - `CmsAdapter`   → metadata estática (sync, barata, sin I/O).
//   - `CmsLifecycle` → ciclo de vida (async, requiere infraestructura).
//
// La separación permite que un CMS recién registrado en el catálogo (descriptor
// declarado) pueda exponerse en `cms-catalog.html` ANTES de tener implementación
// lifecycle completa, evitando vaporware.
//
// Reglas:
//   - El descriptor es estático y barato → seguro llamar desde cualquier listado.
//   - `CmsLifecycle` requiere `CmsAdapter` (super-trait) → un solo adapter por CMS.
//   - Errores propagan vía `Result<T, EnolaError>` (igual que el resto de ports).

use crate::domain::cms::{CmsCreateRequest, CmsDescriptor, CmsInstance};
use crate::domain::error::Result;

/// Metadata estática de un CMS del catálogo.
///
/// Implementaciones típicas son structs zero-sized o singletons. La operación
/// `descriptor()` debe ser barata: NO hace I/O ni asume Docker disponible.
pub trait CmsAdapter: Send + Sync {
    /// Devuelve el descriptor estático del CMS.
    fn descriptor(&self) -> CmsDescriptor;

    /// Imagen Docker por defecto (atajo a `descriptor().default_image`).
    fn default_image(&self) -> &'static str {
        self.descriptor().default_image
    }

    /// `true` si el CMS necesita un contenedor de BD adicional.
    fn requires_db(&self) -> bool {
        self.descriptor().requires_db()
    }

    /// Códigos HTTP que el wizard del CMS puede devolver hasta que el usuario
    /// completa la instalación inicial. Los tests E2E DEBEN aceptarlos como PASS.
    fn setup_wizard_status_codes(&self) -> &'static [u16] {
        self.descriptor().setup_wizard_status_codes
    }
}

/// Ciclo de vida de instancias de un CMS.
///
/// Este trait es opcional: un adapter puede registrar su `descriptor()` en el
/// catálogo antes de tener lifecycle implementado (útil para `cms-catalog.html`
/// sin anunciar vaporware con backend real).
#[async_trait::async_trait]
pub trait CmsLifecycle: CmsAdapter {
    /// Crea una instancia nueva (contenedor + BD si aplica + datos persistentes).
    async fn create(&self, request: CmsCreateRequest) -> Result<CmsInstance>;

    /// Arranca una instancia ya creada (todos los contenedores asociados).
    async fn start(&self, name: &str) -> Result<()>;

    /// Detiene una instancia (sin borrar datos).
    async fn stop(&self, name: &str) -> Result<()>;

    /// Elimina la instancia. Si `force=false`, falla si la instancia está running.
    async fn delete(&self, name: &str, force: bool) -> Result<()>;

    /// Devuelve el estado runtime actual.
    async fn status(&self, name: &str) -> Result<CmsInstance>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::cms::{CmsKind, DbStack};

    /// Adapter de prueba que solo implementa el descriptor.
    struct DummyCms;

    impl CmsAdapter for DummyCms {
        fn descriptor(&self) -> CmsDescriptor {
            CmsDescriptor {
                kind: CmsKind::Ghost,
                display_name: "Ghost",
                default_image: "ghost:5-alpine",
                db_stack: DbStack::Sqlite,
                setup_wizard_status_codes: &[200, 302],
                container_prefix: "ghost-",
                data_root: "/srv/enola-ghost",
                http_port_range: (8000, 9999),
            }
        }
    }

    #[test]
    fn default_methods_delegate_to_descriptor() {
        let a = DummyCms;
        assert_eq!(a.default_image(), "ghost:5-alpine");
        assert!(!a.requires_db()); // sqlite ⇒ false
        assert_eq!(a.setup_wizard_status_codes(), &[200, 302]);
    }

    #[test]
    fn descriptor_kind_is_stable() {
        assert_eq!(DummyCms.descriptor().kind, CmsKind::Ghost);
    }
}
