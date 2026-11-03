//! Port de espacio en disco para el backup (PLAN-BACKUP-RESTORE, Fase 1):
//! comprobar que hay sitio ANTES de crear un archivo que quedaría a medias.
//!
//! Implementación concreta: `src/adapters/infra/disk_space.rs`.

use crate::domain::error::Result;
use std::path::{Path, PathBuf};

/// Consulta de tamaños y espacio libre.
#[cfg_attr(test, mockall::automock)]
pub trait DiskSpacePort: Send + Sync {
    /// Bytes libres para un usuario no privilegiado en el sistema de ficheros que
    /// contiene `path` (o su primer ancestro existente).
    fn available_bytes(&self, path: &Path) -> Result<u64>;

    /// Suma del tamaño de los ficheros regulares bajo `paths` (sin seguir enlaces).
    fn tree_size(&self, paths: &[PathBuf]) -> Result<u64>;
}
