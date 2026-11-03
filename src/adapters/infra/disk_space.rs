//! PLAN-BACKUP-RESTORE (Fase 1) — Adapter de `DiskSpacePort`: espacio libre vía
//! `sysinfo` (sin `unsafe`) y tamaño de árboles con `symlink_metadata`.

use crate::domain::error::{EnolaError, Result};
use crate::ports::disk_space::DiskSpacePort;
use std::path::{Path, PathBuf};

/// Implementación del sistema.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemDiskSpace;

impl SystemDiskSpace {
    /// Constructor.
    pub fn new() -> Self {
        Self
    }
}

/// Primer ancestro de `path` que existe (el destino aún puede no existir).
fn existing_ancestor(path: &Path) -> PathBuf {
    let mut p = path.to_path_buf();
    while !p.exists() {
        if !p.pop() {
            return PathBuf::from("/");
        }
    }
    p
}

impl DiskSpacePort for SystemDiskSpace {
    fn available_bytes(&self, path: &Path) -> Result<u64> {
        let real = std::fs::canonicalize(existing_ancestor(path)).map_err(|e| {
            EnolaError::InfrastructureError(format!("Cannot resolve {}: {e}", path.display()))
        })?;
        let disks = sysinfo::Disks::new_with_refreshed_list();
        // El punto de montaje más específico (más largo) que contiene la ruta.
        disks
            .list()
            .iter()
            .filter(|d| real.starts_with(d.mount_point()))
            .max_by_key(|d| d.mount_point().as_os_str().len())
            .map(|d| d.available_space())
            .ok_or_else(|| {
                EnolaError::InfrastructureError(format!(
                    "Cannot determine free space for {}",
                    real.display()
                ))
            })
    }

    fn tree_size(&self, paths: &[PathBuf]) -> Result<u64> {
        fn walk(p: &Path, total: &mut u64) -> std::io::Result<()> {
            let md = std::fs::symlink_metadata(p)?;
            if md.is_dir() {
                // Entradas ilegibles (p. ej. sin permiso): se saltan; tar fallará
                // con un error claro si de verdad no puede leerlas.
                for e in std::fs::read_dir(p)?.flatten() {
                    let _ = walk(&e.path(), total);
                }
            } else if md.is_file() {
                *total = total.saturating_add(md.len());
            }
            Ok(())
        }
        let mut total = 0u64;
        for p in paths {
            walk(p, &mut total).map_err(|e| {
                EnolaError::InfrastructureError(format!("Cannot measure {}: {e}", p.display()))
            })?;
        }
        Ok(total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree_size_sums_regular_files_and_ignores_symlinks() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a"), vec![0u8; 100]).unwrap();
        std::fs::create_dir(tmp.path().join("d")).unwrap();
        std::fs::write(tmp.path().join("d/b"), vec![0u8; 50]).unwrap();
        std::os::unix::fs::symlink(tmp.path().join("a"), tmp.path().join("link")).unwrap();
        let n = SystemDiskSpace
            .tree_size(&[tmp.path().to_path_buf()])
            .unwrap();
        assert_eq!(n, 150);
    }

    #[test]
    fn available_bytes_works_for_a_path_that_does_not_exist_yet() {
        let tmp = tempfile::tempdir().unwrap();
        let free = SystemDiskSpace
            .available_bytes(&tmp.path().join("no/existe/todavia"))
            .unwrap();
        assert!(free > 0);
    }
}
