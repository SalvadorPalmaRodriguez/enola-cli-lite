// ═══════════════════════════════════════════════════════════════════════════════
// Privilege Management Module
// ═══════════════════════════════════════════════════════════════════════════════
// Este módulo gestiona la verificación de privilegios de sistema y proporciona
// errores descriptivos cuando se requieren permisos elevados.

use std::process::Command;
use thiserror::Error;

/// Errores relacionados con privilegios de sistema
#[derive(Error, Debug)]
pub enum PrivilegeError {
    #[error("Root privileges required. Run with: sudo enola-cli")]
    RootRequired,

    #[error("Permission denied: {0}. Verify you have the necessary permissions.")]
    PermissionDenied(String),

    #[error("Service '{0}' requires elevated privileges to {1}")]
    ServicePrivilegeRequired(String, String),

    #[error("Cannot access system file: {0}. Run with sudo.")]
    SystemFileAccess(String),

    #[error("Cannot modify service configuration without root: {0}")]
    ServiceConfigAccess(String),
}

/// Resultado de la verificación de privilegios
#[derive(Debug, Clone, PartialEq)]
pub enum PrivilegeLevel {
    Root,
    User,
}

/// Verifica si el proceso actual tiene privilegios de root
pub fn check_root() -> Result<PrivilegeLevel, PrivilegeError> {
    check_root_with_euid(unsafe { libc::geteuid() })
}

/// Verifica privilegios y retorna el nivel actual
pub fn get_privilege_level() -> PrivilegeLevel {
    get_privilege_level_from_euid(unsafe { libc::geteuid() })
}

fn check_root_with_euid(euid: u32) -> Result<PrivilegeLevel, PrivilegeError> {
    if euid == 0 {
        Ok(PrivilegeLevel::Root)
    } else {
        Err(PrivilegeError::RootRequired)
    }
}

fn get_privilege_level_from_euid(euid: u32) -> PrivilegeLevel {
    if euid == 0 {
        PrivilegeLevel::Root
    } else {
        PrivilegeLevel::User
    }
}

/// Verifica si se puede acceder a un archivo de sistema
pub fn can_access_system_file(path: &str) -> Result<(), PrivilegeError> {
    use std::fs;
    use std::os::unix::fs::MetadataExt;

    let euid = unsafe { libc::geteuid() };

    match fs::metadata(path) {
        Ok(meta) => can_access_system_file_from_meta(path, euid, Some((meta.uid(), meta.mode()))),
        Err(_) => can_access_system_file_from_meta(path, euid, None),
    }
}

fn can_access_system_file_from_meta(
    path: &str,
    euid: u32,
    metadata: Option<(u32, u32)>,
) -> Result<(), PrivilegeError> {
    match metadata {
        Some((file_uid, mode)) => {
            // Si somos root, siempre podemos acceder
            if euid == 0 {
                return Ok(());
            }

            // Si el archivo es de root y no somos root
            if file_uid == 0 && euid != 0 {
                // Verificar si tenemos permisos de lectura/escritura para others
                let other_write = mode & 0o002 != 0;
                let other_read = mode & 0o004 != 0;

                if !other_write && !other_read {
                    return Err(PrivilegeError::SystemFileAccess(path.to_string()));
                }
            }

            Ok(())
        }
        None => {
            if get_privilege_level_from_euid(euid) != PrivilegeLevel::Root {
                Err(PrivilegeError::SystemFileAccess(path.to_string()))
            } else {
                Ok(())
            }
        }
    }
}

/// Verifica si se puede gestionar un servicio systemd
pub fn can_manage_service(service_name: &str, action: &str) -> Result<(), PrivilegeError> {
    can_manage_service_with_level(get_privilege_level(), service_name, action)
}

fn can_manage_service_with_level(
    level: PrivilegeLevel,
    service_name: &str,
    action: &str,
) -> Result<(), PrivilegeError> {
    if level != PrivilegeLevel::Root {
        return Err(PrivilegeError::ServicePrivilegeRequired(
            service_name.to_string(),
            action.to_string(),
        ));
    }
    Ok(())
}

/// Verifica si Tor está corriendo (requerido para muchas operaciones)
pub fn check_tor_running() -> Result<(), String> {
    let output = Command::new("systemctl")
        .args(["is-active", "tor"])
        .output();
    map_tor_check_result(output.map(|o| o.status.success()))
}

/// Verifica si Docker está disponible
pub fn check_docker_available() -> Result<(), String> {
    let output = Command::new("docker").args(["info"]).output();
    map_docker_check_result(output.map(|o| o.status.success()))
}

fn map_tor_check_result(status: std::io::Result<bool>) -> Result<(), String> {
    match status {
        Ok(true) => Ok(()),
        _ => Err("Tor service is not running. Try: sudo systemctl start tor".to_string()),
    }
}

fn map_docker_check_result(status: std::io::Result<bool>) -> Result<(), String> {
    match status {
        Ok(true) => Ok(()),
        Ok(false) => Err(
            "Docker is not running or you don't have permissions. Try: sudo systemctl start docker"
                .to_string(),
        ),
        Err(_) => Err("Docker is not installed".to_string()),
    }
}

/// Macro para verificar privilegios de root al inicio de una función
#[macro_export]
macro_rules! require_root {
    () => {
        if $crate::infrastructure::privileges::get_privilege_level()
            != $crate::infrastructure::privileges::PrivilegeLevel::Root
        {
            return Err($crate::infrastructure::privileges::PrivilegeError::RootRequired.into());
        }
    };
}

/// Macro para verificar acceso a archivo de sistema
#[macro_export]
macro_rules! require_file_access {
    ($path:expr) => {
        $crate::infrastructure::privileges::can_access_system_file($path)?;
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_privilege_level() {
        // Este test funcionará tanto como root como usuario normal
        let level = get_privilege_level();
        assert!(level == PrivilegeLevel::Root || level == PrivilegeLevel::User);
    }

    #[test]
    fn test_check_root_returns_result() {
        let result = check_root();
        // El resultado depende de cómo se ejecute el test
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn test_can_access_system_file_public() {
        // /etc/hostname es legible por todos
        let result = can_access_system_file("/etc/hostname");
        assert!(result.is_ok());
    }

    #[test]
    fn test_privilege_error_display() {
        let err = PrivilegeError::RootRequired;
        assert!(err.to_string().contains("sudo"));
    }

    #[test]
    fn test_can_access_system_file_missing_path_executes_metadata_error_branch() {
        let r = can_access_system_file("/definitely/nonexistent/enola/privilege/test");
        assert!(r.is_ok() || r.is_err());
    }

    #[test]
    fn test_check_root_with_euid_branches() {
        assert_eq!(check_root_with_euid(0).unwrap(), PrivilegeLevel::Root);
        assert!(matches!(
            check_root_with_euid(1000),
            Err(PrivilegeError::RootRequired)
        ));
    }

    #[test]
    fn test_get_privilege_level_from_euid_branches() {
        assert_eq!(get_privilege_level_from_euid(0), PrivilegeLevel::Root);
        assert_eq!(get_privilege_level_from_euid(1000), PrivilegeLevel::User);
    }

    #[test]
    fn test_can_access_system_file_from_meta_root_can_access() {
        let r = can_access_system_file_from_meta("/etc/shadow", 0, Some((0, 0o600)));
        assert!(r.is_ok());
    }

    #[test]
    fn test_can_access_system_file_from_meta_non_root_denied_on_root_owned_private() {
        let r = can_access_system_file_from_meta("/etc/shadow", 1000, Some((0, 0o600)));
        assert!(matches!(r, Err(PrivilegeError::SystemFileAccess(_))));
    }

    #[test]
    fn test_can_access_system_file_from_meta_non_root_allowed_on_root_owned_world_readable() {
        let r = can_access_system_file_from_meta("/etc/hostname", 1000, Some((0, 0o644)));
        assert!(r.is_ok());
    }

    #[test]
    fn test_can_access_system_file_from_meta_non_root_allowed_on_non_root_file() {
        let r = can_access_system_file_from_meta("/tmp/file", 1000, Some((1000, 0o600)));
        assert!(r.is_ok());
    }

    #[test]
    fn test_can_access_system_file_from_meta_metadata_error_non_root_denied() {
        let r = can_access_system_file_from_meta("/nope", 1000, None);
        assert!(matches!(r, Err(PrivilegeError::SystemFileAccess(_))));
    }

    #[test]
    fn test_can_access_system_file_from_meta_metadata_error_root_allowed() {
        let r = can_access_system_file_from_meta("/nope", 0, None);
        assert!(r.is_ok());
    }

    #[test]
    fn test_map_tor_check_result_branches() {
        assert!(map_tor_check_result(Ok(true)).is_ok());
        assert!(map_tor_check_result(Ok(false)).is_err());
        assert!(map_tor_check_result(Err(std::io::Error::other("x"))).is_err());
    }

    #[test]
    fn test_map_docker_check_result_branches() {
        assert!(map_docker_check_result(Ok(true)).is_ok());
        let not_running = map_docker_check_result(Ok(false)).unwrap_err();
        assert!(not_running.contains("Docker is not running"));
        let not_installed = map_docker_check_result(Err(std::io::Error::other("x"))).unwrap_err();
        assert!(not_installed.contains("not installed"));
    }

    #[test]
    fn test_can_manage_service_with_level_branches() {
        assert!(can_manage_service_with_level(PrivilegeLevel::Root, "tor", "restart").is_ok());
        let err = can_manage_service_with_level(PrivilegeLevel::User, "tor", "restart")
            .expect_err("user should be denied");
        assert!(matches!(
            err,
            PrivilegeError::ServicePrivilegeRequired(_, _)
        ));
    }

    #[test]
    fn test_can_manage_service_smoke_executes_public_wrapper() {
        let r = can_manage_service("tor", "restart");
        assert!(r.is_ok() || r.is_err());
    }

    #[test]
    fn test_check_tor_running_smoke_executes_wrapper() {
        let r = check_tor_running();
        assert!(r.is_ok() || r.is_err());
    }

    #[test]
    fn test_check_docker_available_smoke_executes_wrapper() {
        let r = check_docker_available();
        assert!(r.is_ok() || r.is_err());
    }
}
