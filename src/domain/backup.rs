//! PLAN-BACKUP-RESTORE-ROLLBACK (Fase 1) — Tipos puros del backup «de sistema»:
//! qué rutas forman la **identidad** de una máquina Enola, el manifiesto que
//! viaja dentro del archivo y la regla de espacio en disco.
//!
//! Identidad = lo que hace falta para reconstruir los servicios `.onion` (claves
//! de Tor, fragmentos torrc, Nginx, certificados, configuración de Enola). Los
//! datos de cada servicio (`/srv/enola-*`) quedan fuera por decisión del operador
//! (D2). Dominio puro: sin I/O.

use crate::domain::error::{EnolaError, Result};
use serde::{Deserialize, Serialize};

/// Versión del formato del archivo de backup. `restore` rechaza cualquier otra
/// (alpha sin retrocompatibilidad).
pub const BACKUP_FORMAT_VERSION: u32 = 1;

/// Dónde se escribe el manifiesto antes de archivar (dentro de `/etc/enola`, que
/// forma parte de la identidad: así viaja en el propio archivo).
pub const BACKUP_MANIFEST_PATH: &str = "/etc/enola/backup-manifest.json";

/// Rutas fijas de identidad. Solo root las escribe, salvo `/var/lib/tor`
/// (`debian-tor`). Se excluyen a propósito `~/.enola` (con `sudo -E` sería el home
/// del invocador, SEC-A5) y `/var/lib/enola/updates` (restaurar claves minisign
/// antiguas desharía una rotación de clave).
pub const SYSTEM_IDENTITY_PATHS: &[&str] = &[
    "/var/lib/tor",
    "/etc/tor/enola.d",
    "/etc/nginx/sites-available",
    "/etc/nginx/sites-enabled",
    "/etc/nginx/ssl",
    "/etc/enola",
    "/opt/enola",
    "/usr/local/share/enola/manifest",
];

/// Directorio y prefijo de los perfiles AppArmor de Enola (`enola-*`), que se
/// añaden uno a uno (no se archiva todo `/etc/apparmor.d`).
pub const APPARMOR_DIR: &str = "/etc/apparmor.d";
/// Prefijo de los perfiles AppArmor propios.
pub const APPARMOR_PREFIX: &str = "enola-";

/// Reserva fija de espacio libre sobre el tamaño de las fuentes (copias
/// `pre-restore`, rotación, metadatos del sistema de ficheros).
pub const DISK_RESERVE_BYTES: u64 = 100 * 1024 * 1024;

/// Espacio libre mínimo para crear un backup: tamaño de las fuentes (cota
/// superior del `.tar.gz`) más la reserva fija.
pub fn required_free_bytes(source_bytes: u64) -> u64 {
    source_bytes.saturating_add(DISK_RESERVE_BYTES)
}

/// Manifiesto del archivo de backup (`etc/enola/backup-manifest.json` dentro del tar).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupManifest {
    /// Versión del formato (`BACKUP_FORMAT_VERSION`).
    pub format_version: u32,
    /// Versión de `enola-cli` que lo creó.
    pub enola_version: String,
    /// Fecha de creación (`YYYY-MM-DD_HH-MM-SS`, hora local).
    pub created_at: String,
    /// Rutas absolutas archivadas.
    pub paths: Vec<String>,
}

impl BackupManifest {
    /// Manifiesto nuevo con el formato vigente.
    pub fn new(enola_version: &str, created_at: &str, paths: Vec<String>) -> Self {
        Self {
            format_version: BACKUP_FORMAT_VERSION,
            enola_version: enola_version.to_string(),
            created_at: created_at.to_string(),
            paths,
        }
    }

    /// Serializa a JSON legible.
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string_pretty(self)
            .map_err(|e| EnolaError::ValidationError(format!("backup manifest: {e}")))
    }

    /// Lee un manifiesto y **rechaza** otra versión de formato.
    pub fn parse(json: &str) -> Result<Self> {
        let m: Self = serde_json::from_str(json)
            .map_err(|e| EnolaError::ValidationError(format!("invalid backup manifest: {e}")))?;
        if m.format_version != BACKUP_FORMAT_VERSION {
            return Err(EnolaError::ValidationError(format!(
                "unsupported backup format {} (this enola-cli reads format {})",
                m.format_version, BACKUP_FORMAT_VERSION
            )));
        }
        Ok(m)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_paths_are_absolute_normalized_and_exclude_service_data() {
        for p in SYSTEM_IDENTITY_PATHS {
            assert!(p.starts_with('/'), "{p}");
            assert!(!p.split('/').any(|s| s == ".." || s == "."), "{p}");
            assert!(
                !p.starts_with("/srv/enola-"),
                "datos de servicio fuera (D2): {p}"
            );
            assert!(!p.contains(".enola"), "~/.enola excluido: {p}");
            assert!(
                !p.starts_with("/var/lib/enola/updates"),
                "claves de confianza fuera: {p}"
            );
        }
        for must in [
            "/var/lib/tor",
            "/etc/tor/enola.d",
            "/etc/nginx/ssl",
            "/etc/nginx/sites-enabled",
            "/etc/enola",
        ] {
            assert!(SYSTEM_IDENTITY_PATHS.contains(&must), "falta {must}");
        }
    }

    #[test]
    fn manifest_lives_inside_an_archived_path() {
        assert!(SYSTEM_IDENTITY_PATHS
            .iter()
            .any(|p| BACKUP_MANIFEST_PATH.starts_with(&format!("{p}/"))));
    }

    #[test]
    fn manifest_roundtrips_and_rejects_other_format_versions() {
        let m = BackupManifest::new("0.12.0", "2026-11-03_20-00-00", vec!["/etc/enola".into()]);
        let json = m.to_json().unwrap();
        assert_eq!(BackupManifest::parse(&json).unwrap(), m);

        let other = json.replace("\"format_version\": 1", "\"format_version\": 99");
        assert!(BackupManifest::parse(&other).is_err());
        assert!(BackupManifest::parse("no es json").is_err());
        assert!(BackupManifest::parse("{}").is_err());
    }

    #[test]
    fn required_free_bytes_adds_reserve_and_never_overflows() {
        assert_eq!(required_free_bytes(0), DISK_RESERVE_BYTES);
        assert_eq!(required_free_bytes(1000), 1000 + DISK_RESERVE_BYTES);
        assert_eq!(required_free_bytes(u64::MAX), u64::MAX);
    }
}
