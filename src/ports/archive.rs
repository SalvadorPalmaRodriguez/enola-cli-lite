//! Port de lectura/extracción de archivos de backup (PLAN-BACKUP-RESTORE, Fase 2).
//!
//! El restore corre como root sobre `/`; el listado, la lectura del manifiesto y
//! la extracción pasan por este port para poder validar TODO antes de escribir y
//! para testear sin tocar el sistema. Implementación: `adapters/infra/archive.rs`.

use crate::domain::backup::{ArchiveEntry, RestoreItem};
use crate::domain::error::Result;
use std::path::Path;

/// Acceso a un `.tar.gz` de backup.
#[cfg_attr(test, mockall::automock)]
pub trait ArchiveReaderPort: Send + Sync {
    /// Lista todas las entradas (tipo, enlaces, propietario) sin extraer nada.
    fn list(&self, archive: &Path) -> Result<Vec<ArchiveEntry>>;

    /// Lee un miembro de texto (máx. 1 MiB). `None` si no existe.
    fn read_text(&self, archive: &Path, member: &str) -> Result<Option<String>>;

    /// Extrae SOLO las entradas de `items` (ya validadas) bajo `root`. Nunca sigue
    /// enlaces simbólicos existentes en el destino, escribe de forma atómica y
    /// repone modo y propietario permitidos. Falla antes de escribir si un
    /// propietario necesario no existe en el sistema.
    fn extract(&self, archive: &Path, root: &Path, items: &[RestoreItem]) -> Result<()>;
}
