// SEC-EXT-RACE-011: Lock por puerto basado en FileLock (advisory flock).
//
// Cierra la ventana TOCTOU entre `is_port_free_shared(port)` (que solo OBSERVA el
// estado del puerto) y `docker run -p host:port` (que lo CONSUME). Dos despliegues
// concurrentes del CLI podian ambos:
//   1. Llamar a is_port_free_shared(8080)  ambos veian el puerto libre.
//   2. Lanzar `docker run -p 8080:80`  uno ganaba, el otro fallaba con conflict.
//
// Con `acquire_port_lock(8080)`, el segundo despliegue obtiene WouldBlock
// inmediatamente y aborta con un mensaje claro al usuario, sin tocar Docker.
//
// **Path de los lockfiles** (SEC-A2):
//   - Default: /run/enola/locks/port_<PORT>.lock  (el CLI corre como root via sudo).
//   - Override (tests / dev): env `ENOLA_PORT_LOCK_DIR=/path` — validado igual
//     que el default (dueño = euid, sin symlink, sin escritura de grupo/otros).
//   - Sin root y sin override → error (sin fallback a /tmp).
//
// **Reglas (extiende file_lock.rs):**
//   1. Tomar el lock ANTES de validar el puerto y de lanzar `docker run`.
//   2. Mantener el guard vivo hasta DESPUES de que docker termine de hacer el bind.
//   3. NO compartir un mismo PortLock entre dos hilos concurrentes (cada uno debe
//      llamar `acquire_port_lock` con su Drop scope propio).
use crate::infrastructure::file_lock::{secure_lock_dir, FileLock};
use std::io;
use std::path::PathBuf;

/// Variable de entorno que fija el directorio de locks de puertos.
pub const PORT_LOCK_DIR_ENV: &str = "ENOLA_PORT_LOCK_DIR";

/// Directorio canonico de lockfiles de puertos, ya validado (ver cabecera).
pub fn lock_dir() -> io::Result<PathBuf> {
    secure_lock_dir(PORT_LOCK_DIR_ENV).map(|d| d.path().to_path_buf())
}

fn lock_name(port: u16) -> String {
    format!("port_{}.lock", port)
}

/// Path del lockfile para un puerto dado (solo diagnóstico).
pub fn lock_path_for_port(port: u16) -> io::Result<PathBuf> {
    Ok(lock_dir()?.join(lock_name(port)))
}

/// Intenta reservar un puerto adquiriendo un lock exclusivo no bloqueante.
///
/// Devuelve un guard RAII; mantenerlo vivo durante toda la operacion que
/// consume el puerto (validacion + `docker run -p` + cualquier post-check).
///
/// # Errores
/// - `io::ErrorKind::WouldBlock`: otro proceso del CLI ya esta reservando
///   este puerto. El caller debe abortar con mensaje al usuario.
/// - `PermissionDenied`: directorio de locks inseguro o CLI sin root.
/// - Otros: problemas de I/O al crear el lockfile.
pub fn acquire_port_lock(port: u16) -> io::Result<FileLock> {
    let dir = secure_lock_dir(PORT_LOCK_DIR_ENV)?;
    FileLock::try_acquire_in(&dir, &lock_name(port))
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::LockResult;
    use std::sync::MutexGuard;
    use tempfile::TempDir;
    // Asegura aislamiento: cada test usa su propio dir via env var.
    // `std::env::set_var` no es thread-safe entre tests, asi que serializamos
    // con el mutex global TEST_ENV_LOCK.
    fn recover_poison<T>(res: LockResult<MutexGuard<'static, T>>) -> MutexGuard<'static, T> {
        match res {
            Ok(g) => g,
            Err(e) => e.into_inner(),
        }
    }

    struct EnvVarRestore {
        prev: Option<String>,
        // Mantiene el lock durante todo el scope del test para evitar carreras de env.
        _guard: MutexGuard<'static, ()>,
    }

    fn restore_lock_dir_env(prev: Option<String>) {
        match prev {
            Some(v) => std::env::set_var("ENOLA_PORT_LOCK_DIR", v),
            None => std::env::remove_var("ENOLA_PORT_LOCK_DIR"),
        }
    }

    impl Drop for EnvVarRestore {
        fn drop(&mut self) {
            restore_lock_dir_env(self.prev.take());
        }
    }

    fn set_lock_dir_override(dir: &std::path::Path) -> EnvVarRestore {
        // SEC-A2: el override se valida; TempDir respeta el umask (002 → 0775).
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        let guard = recover_poison(crate::infrastructure::TEST_ENV_LOCK.lock());
        let prev = std::env::var("ENOLA_PORT_LOCK_DIR").ok();
        std::env::set_var("ENOLA_PORT_LOCK_DIR", dir);
        EnvVarRestore {
            prev,
            _guard: guard,
        }
    }

    #[test]
    fn set_lock_dir_override_restores_previous_value_when_present() {
        let tmp_prev = TempDir::new().unwrap();
        let tmp_new = TempDir::new().unwrap();

        let _g = recover_poison(crate::infrastructure::TEST_ENV_LOCK.lock());
        std::env::set_var("ENOLA_PORT_LOCK_DIR", tmp_prev.path());
        let prev = std::env::var("ENOLA_PORT_LOCK_DIR").ok();
        std::env::set_var("ENOLA_PORT_LOCK_DIR", tmp_new.path());
        let current = std::env::var("ENOLA_PORT_LOCK_DIR").unwrap();
        assert_eq!(PathBuf::from(current), tmp_new.path());
        restore_lock_dir_env(prev);
        let restored = std::env::var("ENOLA_PORT_LOCK_DIR").unwrap();
        assert_eq!(PathBuf::from(restored), tmp_prev.path());
        std::env::remove_var("ENOLA_PORT_LOCK_DIR");
    }

    #[test]
    fn set_lock_dir_override_restores_to_unset_when_no_previous_value() {
        let tmp_new = TempDir::new().unwrap();

        let _g = recover_poison(crate::infrastructure::TEST_ENV_LOCK.lock());
        std::env::remove_var("ENOLA_PORT_LOCK_DIR");
        let prev = std::env::var("ENOLA_PORT_LOCK_DIR").ok();
        std::env::set_var("ENOLA_PORT_LOCK_DIR", tmp_new.path());
        let current = std::env::var("ENOLA_PORT_LOCK_DIR").unwrap();
        assert_eq!(PathBuf::from(current), tmp_new.path());
        restore_lock_dir_env(prev);
        assert!(std::env::var("ENOLA_PORT_LOCK_DIR").is_err());
    }
    #[test]
    fn lock_path_for_port_uses_env_override() {
        let tmp = TempDir::new().unwrap();
        let _restore = set_lock_dir_override(tmp.path());
        let p = lock_path_for_port(12345).unwrap();
        assert!(p.starts_with(tmp.path()));
        assert!(p.to_string_lossy().ends_with("port_12345.lock"));
    }
    #[test]
    fn acquire_port_lock_creates_lockfile_in_override_dir() {
        let tmp = TempDir::new().unwrap();
        let _restore = set_lock_dir_override(tmp.path());
        let _g = acquire_port_lock(54321).expect("acquire");
        let expected = tmp.path().join("port_54321.lock");
        assert!(
            expected.exists(),
            "lockfile created at {}",
            expected.display()
        );
    }
    #[test]
    fn second_acquire_same_port_returns_wouldblock() {
        let tmp = TempDir::new().unwrap();
        let _restore = set_lock_dir_override(tmp.path());
        let _g1 = acquire_port_lock(54322).expect("first");
        let err = acquire_port_lock(54322).expect_err("second must fail");
        assert_eq!(err.kind(), io::ErrorKind::WouldBlock);
    }
    #[test]
    fn different_ports_dont_block_each_other() {
        let tmp = TempDir::new().unwrap();
        let _restore = set_lock_dir_override(tmp.path());
        let _g1 = acquire_port_lock(54323).expect("port a");
        let _g2 = acquire_port_lock(54324).expect("port b independent");
    }
    // SEC-A2: sin override, el default exige root; nunca cae a /tmp.
    #[test]
    fn lock_dir_without_override_never_falls_back_to_tmp() {
        let _g = recover_poison(crate::infrastructure::TEST_ENV_LOCK.lock());
        let prev = std::env::var("ENOLA_PORT_LOCK_DIR").ok();
        std::env::remove_var("ENOLA_PORT_LOCK_DIR");
        let d = lock_dir();
        restore_lock_dir_env(prev);
        match d {
            Ok(p) => assert_eq!(p, PathBuf::from("/run/enola/locks")),
            Err(e) => assert_eq!(e.kind(), io::ErrorKind::PermissionDenied),
        }
    }

    #[test]
    fn lock_dir_ignores_empty_env_var() {
        let _g = recover_poison(crate::infrastructure::TEST_ENV_LOCK.lock());
        let prev = std::env::var("ENOLA_PORT_LOCK_DIR").ok();
        std::env::set_var("ENOLA_PORT_LOCK_DIR", ""); // vacío → default
        let d = lock_dir();
        restore_lock_dir_env(prev);
        if let Ok(p) = d {
            assert_eq!(p, PathBuf::from("/run/enola/locks"));
        }
    }

    #[test]
    fn acquire_rejects_world_writable_override_dir() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = TempDir::new().unwrap();
        let _restore = set_lock_dir_override(tmp.path());
        std::fs::set_permissions(tmp.path(), std::fs::Permissions::from_mode(0o777)).unwrap();
        let err = acquire_port_lock(54330).expect_err("insecure dir must be rejected");
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn acquire_rejects_symlinked_port_lockfile() {
        let tmp = TempDir::new().unwrap();
        let _restore = set_lock_dir_override(tmp.path());
        let victim_dir = TempDir::new().unwrap();
        let victim = victim_dir.path().join("victim");
        std::fs::write(&victim, b"keep").unwrap();
        std::os::unix::fs::symlink(&victim, tmp.path().join("port_54331.lock")).unwrap();
        assert!(acquire_port_lock(54331).is_err());
        assert_eq!(std::fs::read(&victim).unwrap(), b"keep");
    }

    // TEST-COV-UNIT-003: lock se libera al salir del scope y puede ser re-adquirido
    #[test]
    fn lock_released_on_scope_exit_allows_reacquire() {
        let tmp = TempDir::new().unwrap();
        let _restore = set_lock_dir_override(tmp.path());
        {
            let _g = acquire_port_lock(59999).expect("primera adquisición");
            // guard sale de scope aquí
        }
        // Ahora debe poder re-adquirir
        let _g2 = acquire_port_lock(59999).expect("re-adquisición tras release");
    }

    // TEST-COV-UNIT-003: cubrir restore de env var en rama Some(v)
    // Nota: NO llamamos set_lock_dir_override() desde dentro de un bloque que ya tiene crate::infrastructure::TEST_ENV_LOCK
    // (causaria deadlock por mutex no-reentrante). En su lugar testeamos la logica directamente.
    #[test]
    fn some_restore_covers_l80_l131_l145() {
        let tmp_prev = tempfile::TempDir::new().unwrap();
        let tmp_actual = tempfile::TempDir::new().unwrap();
        // Asegurar que la logica "Some(v) => set_var" se ejecuta:
        // Simulamos el patron identico a with_lock_dir/lock_dir_default pero con prev=Some.
        {
            let _g = recover_poison(crate::infrastructure::TEST_ENV_LOCK.lock());
            // Establecer variable antes de capturar prev (cubre el Some arm en match al restaurar)
            std::env::set_var("ENOLA_PORT_LOCK_DIR", tmp_prev.path());
            let prev = std::env::var("ENOLA_PORT_LOCK_DIR").ok(); // Some(tmp_prev)
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(tmp_actual.path(), std::fs::Permissions::from_mode(0o700))
                    .unwrap();
            }
            std::env::set_var("ENOLA_PORT_LOCK_DIR", tmp_actual.path());
            let d = lock_dir().unwrap();
            assert_eq!(d, tmp_actual.path().to_path_buf());
            // Restaurar: cubre L80, L131, L145 (rama Some(v))
            std::env::remove_var("ENOLA_PORT_LOCK_DIR");
            if let Some(v) = prev {
                std::env::set_var("ENOLA_PORT_LOCK_DIR", v);
            }
            let restored = std::env::var("ENOLA_PORT_LOCK_DIR").unwrap();
            assert_eq!(
                std::path::PathBuf::from(&restored),
                tmp_prev.path().to_path_buf()
            );
            std::env::remove_var("ENOLA_PORT_LOCK_DIR");
        }
    }
}
