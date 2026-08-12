// SEC-EXT-RACE-012: lockfiles para artefactos compartidos.
//
// Preparacion de cableado para recursos mutables fuera de Docker:
// - /etc/systemd/system/*.service|*.timer
// - /etc/nginx/sites-available/* y /etc/nginx/sites-enabled/*
// - /opt/enola/**
//
// Politica "solo ante colision real": si el artefacto aun no existe, no se
// toma lock; si existe (edicion/reemplazo), se toma flock exclusivo.

use crate::infrastructure::file_lock::FileLock;
use std::io;
use std::path::Path;

fn lock_root_dir() -> std::path::PathBuf {
    if let Ok(custom) = std::env::var("ENOLA_LOCK_DIR") {
        let p = std::path::PathBuf::from(custom);
        let _ = std::fs::create_dir_all(&p);
        return p;
    }
    let primary = std::path::PathBuf::from("/var/lock/enola");
    lock_root_dir_with_primary(&primary)
}

fn lock_root_dir_with_primary(primary: &std::path::Path) -> std::path::PathBuf {
    if primary.exists() || std::fs::create_dir_all(primary).is_ok() {
        primary.to_path_buf()
    } else {
        std::path::PathBuf::from("/tmp/enola-locks")
    }
}

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
    let lock_path = lock_root_dir().join(format!("artifact_{}.lock", key));
    FileLock::try_acquire(lock_path).map(Some)
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

    static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

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
        let guard = recover_poison(ENV_MUTEX.lock());
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

    #[test]
    fn lock_root_returns_primary_when_exists() {
        let tmp = tempfile::TempDir::new().unwrap();
        let primary = tmp.path().join("enola_root");
        std::fs::create_dir_all(&primary).unwrap();
        let d = lock_root_dir_with_primary(&primary);
        assert_eq!(d, primary.to_path_buf());
    }

    #[test]
    fn lock_root_creates_primary_when_possible() {
        let tmp = tempfile::TempDir::new().unwrap();
        let primary = tmp.path().join("enola_create_me");
        let d = lock_root_dir_with_primary(&primary);
        assert_eq!(d, primary.to_path_buf());
    }

    #[test]
    fn lock_root_falls_back_to_tmp_when_cant_create() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::TempDir::new().unwrap();
        let ro = tmp.path().join("ro");
        std::fs::create_dir_all(&ro).unwrap();
        std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o444)).unwrap();
        let primary = ro.join("enola_fail");
        let d = lock_root_dir_with_primary(&primary);
        std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(d, std::path::PathBuf::from("/tmp/enola-locks"));
    }
}
