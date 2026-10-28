//! SEC-M10 (D6) — Usuarios de sistema dedicados para los procesos de los
//! contenedores que no corren como root (Forgejo, Ghost).
//!
//! Sin user namespaces, el uid del proceso del contenedor es el mismo en el
//! host: con uid 1000 (el primer usuario humano en Ubuntu/Debian) un escape
//! del contenedor caería en la cuenta del operador, que suele tener sudo. Con
//! un usuario de sistema propio (`useradd --system`, sin shell ni home) cae
//! en una cuenta sin privilegios ni ficheros propios.
//!
//! Implementación concreta: `src/adapters/infra/service_user.rs`.

use crate::domain::error::Result;
use std::path::Path;

/// Gestión de usuarios de sistema de servicio.
#[cfg_attr(test, mockall::automock)]
pub trait ServiceUserPort: Send + Sync {
    /// Garantiza que existe el usuario de sistema `name` (lo crea si falta)
    /// y devuelve su `(uid, gid)`.
    fn ensure(&self, name: &str) -> Result<(u32, u32)>;

    /// `chown -R uid:gid` sobre `path` sin seguir symlinks.
    fn chown_tree(&self, path: &Path, uid: u32, gid: u32) -> Result<()>;
}
