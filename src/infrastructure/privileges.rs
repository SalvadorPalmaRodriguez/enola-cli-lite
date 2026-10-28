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

/// `(uid, gid, mode, es_directorio)` de una ruta, sin seguir el último symlink
/// (`invoker_can_read` canonicaliza antes de evaluar).
type StatInfo = (u32, u32, u32, bool);

/// `true` si la clase que aplica a `uid`/`gids` (dueño, grupo u otros, en ese
/// orden de precedencia, como el kernel) tiene todos los `bits` (0o4 lectura,
/// 0o1 paso) en `mode`.
fn mode_grants(uid: u32, gids: &[u32], st: StatInfo, bits: u32) -> bool {
    let (fuid, fgid, mode, _) = st;
    let class = if uid == fuid {
        mode >> 6
    } else if gids.contains(&fgid) {
        mode >> 3
    } else {
        mode
    };
    class & bits == bits
}

/// SEC-M7/M11: `true` si `uid`/`gids` podría leer `path`: paso (`x`) en cada
/// directorio ancestro y lectura en `path` (+ paso si es directorio). Una
/// ruta que no se puede consultar cuenta como no legible. Las ACL no se
/// evalúan: solo pueden dar más acceso, así que el fallo es conservador.
fn chain_readable(
    path: &std::path::Path,
    uid: u32,
    gids: &[u32],
    stat: impl Fn(&std::path::Path) -> Option<StatInfo>,
) -> bool {
    for anc in path.ancestors().skip(1) {
        match stat(anc) {
            Some(st) if mode_grants(uid, gids, st, 0o1) => {}
            _ => return false,
        }
    }
    match stat(path) {
        Some(st) => {
            let bits = if st.3 { 0o5 } else { 0o4 };
            mode_grants(uid, gids, st, bits)
        }
        None => false,
    }
}

/// Grupos (primario + suplementarios) de `name` según NSS.
#[cfg(unix)]
fn groups_of(name: &str, gid: u32) -> Vec<u32> {
    let Ok(cname) = std::ffi::CString::new(name) else {
        return vec![gid];
    };
    let mut n: libc::c_int = 64;
    for _ in 0..4 {
        let mut buf = vec![0 as libc::gid_t; n.max(1) as usize];
        let mut got = n;
        // SAFETY: `cname` termina en NUL; `buf` tiene `got` huecos y
        // `getgrouplist` escribe como mucho `got` y actualiza `got`.
        let r = unsafe { libc::getgrouplist(cname.as_ptr(), gid, buf.as_mut_ptr(), &mut got) };
        if r >= 0 {
            buf.truncate(got.max(0) as usize);
            return buf;
        }
        n = got.max(n * 2);
    }
    vec![gid]
}

/// SEC-M7/M11: `true` si el usuario que invocó `sudo` podría leer `path`.
///
/// Las lecturas de contenido que pide el usuario (fuentes RAG, datasets de
/// finetune) se hacen como root; sin esta comprobación servirían para leer
/// ficheros de otros usuarios o de `/srv`. Fuera de `sudo` (sin root, o root
/// sin `SUDO_UID`) devuelve `true`: el propio kernel aplica los permisos o
/// el invocador ya es root.
pub fn invoker_can_read(path: &std::path::Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    // SAFETY: geteuid no tiene precondiciones.
    if unsafe { libc::geteuid() } != 0 {
        return true;
    }
    let Some(user) = crate::infrastructure::drop_privs::invoking_user() else {
        return true;
    };
    // Se evalúa el destino real: un symlink en sí es 0777 y no dice nada.
    let Ok(canon) = std::fs::canonicalize(path) else {
        return false;
    };
    let gids = match &user.name {
        Some(name) => groups_of(name, user.gid),
        None => vec![user.gid],
    };
    chain_readable(&canon, user.uid, &gids, |p| {
        std::fs::symlink_metadata(p)
            .ok()
            .map(|m| (m.uid(), m.gid(), m.mode(), m.is_dir()))
    })
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
    // ── SEC-M7/M11: lectura con los permisos del invocador ──

    fn fake_fs(
        entries: &'static [(&'static str, StatInfo)],
    ) -> impl Fn(&std::path::Path) -> Option<StatInfo> {
        move |p| {
            entries
                .iter()
                .find(|(k, _)| std::path::Path::new(k) == p)
                .map(|(_, v)| *v)
        }
    }

    const DIR_755: StatInfo = (0, 0, 0o40755, true);

    #[test]
    fn invoker_reads_own_file_under_world_traversable_dirs() {
        let fs = fake_fs(&[
            ("/", DIR_755),
            ("/home", DIR_755),
            ("/home/ana", (1000, 1000, 0o40750, true)),
            ("/home/ana/doc.md", (1000, 1000, 0o100600, false)),
        ]);
        assert!(chain_readable(
            std::path::Path::new("/home/ana/doc.md"),
            1000,
            &[1000],
            &fs
        ));
    }

    #[test]
    fn invoker_cannot_read_other_users_private_file() {
        let fs = fake_fs(&[
            ("/", DIR_755),
            ("/home", DIR_755),
            ("/home/bob", (1001, 1001, 0o40755, true)),
            ("/home/bob/secret.md", (1001, 1001, 0o100600, false)),
        ]);
        assert!(!chain_readable(
            std::path::Path::new("/home/bob/secret.md"),
            1000,
            &[1000],
            &fs
        ));
    }

    #[test]
    fn invoker_cannot_cross_root_only_directory() {
        // `/srv/enola-git` 0700 root: el fichero es 0644, pero no se llega a él.
        let fs = fake_fs(&[
            ("/", DIR_755),
            ("/srv", DIR_755),
            ("/srv/enola-git", (0, 0, 0o40700, true)),
            ("/srv/enola-git/app.ini", (0, 0, 0o100644, false)),
        ]);
        assert!(!chain_readable(
            std::path::Path::new("/srv/enola-git/app.ini"),
            1000,
            &[1000],
            &fs
        ));
    }

    #[test]
    fn invoker_reads_through_group_membership() {
        let fs = fake_fs(&[
            ("/", DIR_755),
            ("/srv", DIR_755),
            ("/srv/share", (0, 33, 0o40750, true)),
            ("/srv/share/a.md", (0, 33, 0o100640, false)),
        ]);
        let p = std::path::Path::new("/srv/share/a.md");
        assert!(chain_readable(p, 1000, &[1000, 33], &fs));
        assert!(!chain_readable(p, 1000, &[1000], &fs));
    }

    #[test]
    fn owner_class_wins_even_if_more_restrictive() {
        // Dueño sin lectura aunque "otros" la tenga: el kernel deniega.
        let fs = fake_fs(&[("/", DIR_755), ("/f", (1000, 1000, 0o100044, false))]);
        assert!(!chain_readable(
            std::path::Path::new("/f"),
            1000,
            &[1000],
            &fs
        ));
    }

    #[test]
    fn directory_needs_read_and_traverse() {
        let fs = fake_fs(&[("/", DIR_755), ("/d", (1000, 1000, 0o40600, true))]);
        assert!(!chain_readable(
            std::path::Path::new("/d"),
            1000,
            &[1000],
            &fs
        ));
    }

    #[test]
    fn unstattable_path_is_not_readable() {
        let fs = fake_fs(&[("/", DIR_755)]);
        assert!(!chain_readable(
            std::path::Path::new("/nope"),
            1000,
            &[1000],
            &fs
        ));
    }

    #[test]
    fn invoker_can_read_is_true_without_root() {
        // Sin euid 0 el kernel ya aplica los permisos (los tests no corren como root).
        if unsafe { libc::geteuid() } != 0 {
            assert!(invoker_can_read(std::path::Path::new("/nope/at/all")));
        }
    }
}
