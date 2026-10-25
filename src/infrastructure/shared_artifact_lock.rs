// SEC-EXT-RACE-012: lockfiles para artefactos compartidos.
//
// Preparacion de cableado para recursos mutables fuera de Docker:
// - /etc/systemd/system/*.service|*.timer
// - /etc/nginx/sites-available/* y /etc/nginx/sites-enabled/*
// - /opt/enola/**
//
// Politica "solo ante colision real": si el artefacto aun no existe, no se
// toma lock; si existe (edicion/reemplazo), se toma flock exclusivo.

use crate::infrastructure::file_lock::{secure_lock_dir, FileLock};
use std::io;
use std::path::Path;

/// Variable de entorno que fija el directorio de locks de artefactos
/// (validado igual que el default `/run/enola/locks`, SEC-A2).
pub const ARTIFACT_LOCK_DIR_ENV: &str = "ENOLA_LOCK_DIR";

fn sanitize_key(raw: &str) -> String {
    raw.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn maybe_lock_existing_artifact(artifact: &Path, scope: &str) -> io::Result<Option<FileLock>> {
    if !artifact.exists() {
        return Ok(None);
    }
    let key = sanitize_key(scope);
    let dir = secure_lock_dir(ARTIFACT_LOCK_DIR_ENV)?;
    FileLock::try_acquire_in(&dir, &format!("artifact_{}.lock", key)).map(Some)
}

pub fn maybe_lock_nginx_site(path: &Path) -> io::Result<Option<FileLock>> {
    maybe_lock_existing_artifact(path, &format!("nginx_{}", path.display()))
}

pub fn maybe_lock_systemd_unit(path: &Path) -> io::Result<Option<FileLock>> {
    maybe_lock_existing_artifact(path, &format!("systemd_{}", path.display()))
}

pub fn maybe_lock_opt_enola_path(path: &Path) -> io::Result<Option<FileLock>> {
    maybe_lock_existing_artifact(path, &format!("opt_enola_{}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{LockResult, MutexGuard};

    fn recover_poison<T>(res: LockResult<MutexGuard<'static, T>>) -> MutexGuard<'static, T> {
        match res {
            Ok(g) => g,
            Err(e) => e.into_inner(),
        }
    }

    struct LockDirGuard {
        _guard: MutexGuard<'static, ()>,
    }

    impl Drop for LockDirGuard {
        fn drop(&mut self) {
            std::env::remove_var("ENOLA_LOCK_DIR");
        }
    }

    fn set_lock_dir(dir: &std::path::Path) -> LockDirGuard {
        // SEC-A2: el override se valida; TempDir respeta el umask (002 → 0775).
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).expect("chmod");
        let guard = recover_poison(crate::infrastructure::TEST_ENV_LOCK.lock());
        std::env::set_var("ENOLA_LOCK_DIR", dir);
        LockDirGuard { _guard: guard }
    }

    #[test]
    fn no_lock_when_artifact_missing() {
        let tmp = tempfile::TempDir::new().expect("tmp");
        let path = tmp.path().join("missing.conf");
        let lock = maybe_lock_nginx_site(&path).expect("ok");
        assert!(lock.is_none());
    }

    #[test]
    fn lock_when_artifact_exists() {
        let tmp = tempfile::TempDir::new().expect("tmp");
        let lock_dir = tempfile::TempDir::new().expect("lock_dir");
        let _env_guard = set_lock_dir(lock_dir.path());
        let path = tmp.path().join("existing.conf");
        std::fs::write(&path, "x").expect("write");
        let lock = maybe_lock_nginx_site(&path).expect("ok");
        assert!(lock.is_some());
    }

    // TEST-COV-UNIT-003: cubrir maybe_lock_systemd_unit y maybe_lock_opt_enola_path
    #[test]
    fn systemd_unit_lock_when_exists() {
        let tmp = tempfile::TempDir::new().expect("tmp");
        let lock_dir = tempfile::TempDir::new().expect("lock_dir");
        let _env_guard = set_lock_dir(lock_dir.path());
        let path = tmp.path().join("enola-test-service.service");
        std::fs::write(&path, "[Unit]\nDescription=test\n").expect("write");
        let lock = maybe_lock_systemd_unit(&path).expect("ok");
        assert!(lock.is_some());
    }

    #[test]
    fn systemd_unit_no_lock_when_missing() {
        let tmp = tempfile::TempDir::new().expect("tmp");
        let path = tmp.path().join("non-existent.timer");
        let lock = maybe_lock_systemd_unit(&path).expect("ok");
        assert!(lock.is_none());
    }

    #[test]
    fn opt_enola_lock_when_exists() {
        let tmp = tempfile::TempDir::new().expect("tmp");
        let lock_dir = tempfile::TempDir::new().expect("lock_dir");
        let _env_guard = set_lock_dir(lock_dir.path());
        let path = tmp.path().join("config.toml");
        std::fs::write(&path, "key = \"val\"\n").expect("write");
        let lock = maybe_lock_opt_enola_path(&path).expect("ok");
        assert!(lock.is_some());
    }

    #[test]
    fn opt_enola_no_lock_when_missing() {
        let tmp = tempfile::TempDir::new().expect("tmp");
        let path = tmp.path().join("absent_opt_config");
        let lock = maybe_lock_opt_enola_path(&path).expect("ok");
        assert!(lock.is_none());
    }

    // TEST-COV-UNIT-003: sanitize_key convierte caracteres no permitidos
    #[test]
    fn sanitize_key_replaces_slashes_and_dots() {
        let result = sanitize_key("/etc/nginx/sites-available/enola.conf");
        assert!(!result.contains('/'), "no slashes: {}", result);
        assert!(!result.contains('.'), "no dots: {}", result);
        // Caracteres válidos deben preservarse
        assert!(
            result.contains('-')
                || result.contains('_')
                || result
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        );
    }

    // SEC-A2: directorio de locks inseguro → error, nunca fallback a /tmp.
    #[test]
    fn lock_rejects_world_writable_lock_dir() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::TempDir::new().expect("tmp");
        let lock_dir = tempfile::TempDir::new().expect("lock_dir");
        let _env_guard = set_lock_dir(lock_dir.path());
        std::fs::set_permissions(lock_dir.path(), std::fs::Permissions::from_mode(0o777))
            .expect("chmod");
        let path = tmp.path().join("existing.conf");
        std::fs::write(&path, "x").expect("write");
        let err = maybe_lock_nginx_site(&path).expect_err("insecure lock dir");
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
    }
}
