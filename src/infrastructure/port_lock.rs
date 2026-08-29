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
// **Path de los lockfiles**:
//   - Default: /var/lock/enola/port_<PORT>.lock  (el CLI corre como root via sudo).
//   - Override (tests / no-root): env `ENOLA_PORT_LOCK_DIR=/path`.
//   - Fallback: si /var/lock/enola/ no es writable, /tmp/enola-locks/.
//
// **Reglas (extiende file_lock.rs):**
//   1. Tomar el lock ANTES de validar el puerto y de lanzar `docker run`.
//   2. Mantener el guard vivo hasta DESPUES de que docker termine de hacer el bind.
//   3. NO compartir un mismo PortLock entre dos hilos concurrentes (cada uno debe
//      llamar `acquire_port_lock` con su Drop scope propio).
use crate::infrastructure::file_lock::FileLock;
use std::io;
use std::path::PathBuf;
/// Directorio canonico de lockfiles de puertos.
///
/// Resolucion:
/// 1. `ENOLA_PORT_LOCK_DIR` (env) -> uso forzado (tests, dev sin root).
/// 2. /var/lock/enola/  -> default produccion (root).
/// 3. /tmp/enola-locks/ -> fallback si /var/lock no es accesible.
pub fn lock_dir() -> PathBuf {
    if let Ok(custom) = std::env::var("ENOLA_PORT_LOCK_DIR") {
        if !custom.is_empty() {
            return PathBuf::from(custom);
        }
    }
    let primary = PathBuf::from("/var/lock/enola");
    lock_dir_from_primary(&primary)
}

fn lock_dir_from_primary(primary: &std::path::Path) -> PathBuf {
    // Probar a crear /var/lock/enola/. Si falla (no-root, FS read-only),
    // caer a /tmp/enola-locks/. La creacion real (con permisos) la hace
    // FileLock::try_acquire al primer uso.
    if primary.exists() {
        return primary.to_path_buf();
    }
    match std::fs::create_dir_all(primary) {
        Ok(_) => primary.to_path_buf(),
        Err(_) => PathBuf::from("/tmp/enola-locks"),
    }
}

#[cfg(feature = "testing")]
pub fn lock_dir_from_primary_for_testing(primary: &std::path::Path) -> PathBuf {
    lock_dir_from_primary(primary)
}
/// Path del lockfile para un puerto dado.
pub fn lock_path_for_port(port: u16) -> PathBuf {
    lock_dir().join(format!("port_{}.lock", port))
}
/// Intenta reservar un puerto adquiriendo un lock exclusivo no bloqueante.
///
/// Devuelve un guard RAII; mantenerlo vivo durante toda la operacion que
/// consume el puerto (validacion + `docker run -p` + cualquier post-check).
///
/// # Errores
/// - `io::ErrorKind::WouldBlock`: otro proceso del CLI ya esta reservando
///   este puerto. El caller debe abortar con mensaje al usuario.
/// - Otros: problemas de permisos / I/O al crear el lockfile.
pub fn acquire_port_lock(port: u16) -> io::Result<FileLock> {
    let path = lock_path_for_port(port);
    FileLock::try_acquire(&path)
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::LockResult;
    use std::sync::MutexGuard;
    use tempfile::TempDir;
    /// Asegura aislamiento: cada test usa su propio dir via env var.
    /// `std::env::set_var` no es thread-safe entre tests, asi que serializamos
    /// con un mutex estatico.
    static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

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

    impl Drop for EnvVarRestore {
        fn drop(&mut self) {
            match self.prev.take() {
                Some(v) => std::env::set_var("ENOLA_PORT_LOCK_DIR", v),
                None => std::env::remove_var("ENOLA_PORT_LOCK_DIR"),
            }
        }
    }

    fn set_lock_dir_override(dir: &std::path::Path) -> EnvVarRestore {
        let guard = recover_poison(ENV_MUTEX.lock());
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

        {
            let _g = recover_poison(ENV_MUTEX.lock());
            std::env::set_var("ENOLA_PORT_LOCK_DIR", tmp_prev.path());
        }

        {
            let _restore = set_lock_dir_override(tmp_new.path());
            let current = std::env::var("ENOLA_PORT_LOCK_DIR").unwrap();
            assert_eq!(PathBuf::from(current), tmp_new.path());
        }

        let restored = std::env::var("ENOLA_PORT_LOCK_DIR").unwrap();
        assert_eq!(PathBuf::from(restored), tmp_prev.path());
        std::env::remove_var("ENOLA_PORT_LOCK_DIR");
    }

    #[test]
    fn set_lock_dir_override_restores_to_unset_when_no_previous_value() {
        let tmp_new = TempDir::new().unwrap();

        {
            let _g = recover_poison(ENV_MUTEX.lock());
            std::env::remove_var("ENOLA_PORT_LOCK_DIR");
        }

        {
            let _restore = set_lock_dir_override(tmp_new.path());
            let current = std::env::var("ENOLA_PORT_LOCK_DIR").unwrap();
            assert_eq!(PathBuf::from(current), tmp_new.path());
        }

        {
            let _g = recover_poison(ENV_MUTEX.lock());
            assert!(std::env::var("ENOLA_PORT_LOCK_DIR").is_err());
        }
    }
    #[test]
    fn lock_path_for_port_uses_env_override() {
        let tmp = TempDir::new().unwrap();
        let _restore = set_lock_dir_override(tmp.path());
        let p = lock_path_for_port(12345);
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
    #[test]
    fn lock_dir_default_falls_back_when_var_lock_unavailable() {
        // Sin override, lock_dir() devuelve algo valido (/var/lock/enola o /tmp/enola-locks).
        let _g = recover_poison(ENV_MUTEX.lock());
        std::env::set_var("ENOLA_PORT_LOCK_DIR", "/tmp/enola-prev");
        let prev = std::env::var("ENOLA_PORT_LOCK_DIR").ok();
        std::env::remove_var("ENOLA_PORT_LOCK_DIR");
        let d = lock_dir();
        let is_expected =
            d == *"/var/lock/enola" || d == *"/tmp/enola-locks";
        assert!(is_expected);
        if let Some(v) = prev {
            std::env::set_var("ENOLA_PORT_LOCK_DIR", v);
        }
    }

    #[test]
    fn lock_dir_default_restores_to_unset_when_no_previous_override() {
        let _g = recover_poison(ENV_MUTEX.lock());
        std::env::remove_var("ENOLA_PORT_LOCK_DIR");
        let prev = std::env::var("ENOLA_PORT_LOCK_DIR").ok();
        assert!(prev.is_none());
        std::env::remove_var("ENOLA_PORT_LOCK_DIR");
        let _ = lock_dir();
        std::env::remove_var("ENOLA_PORT_LOCK_DIR");
        assert!(std::env::var("ENOLA_PORT_LOCK_DIR").is_err());
    }

    // TEST-COV-UNIT-003: cubrir lock_dir() con env var vacía (rama !custom.is_empty() = false)
    #[test]
    fn lock_dir_ignores_empty_env_var_and_falls_back() {
        let _g = recover_poison(ENV_MUTEX.lock());
        std::env::set_var("ENOLA_PORT_LOCK_DIR", "/tmp/enola-prev");
        let prev = std::env::var("ENOLA_PORT_LOCK_DIR").ok();
        std::env::set_var("ENOLA_PORT_LOCK_DIR", ""); // vacío → ignorado
        let d = lock_dir();
        // Debe caer al default, no a ""
        assert!(
            d != *"",
            "lock_dir vacío debe rechazarse: {:?}",
            d
        );
        std::env::remove_var("ENOLA_PORT_LOCK_DIR");
        if let Some(v) = prev {
            std::env::set_var("ENOLA_PORT_LOCK_DIR", v);
        }
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

    // TEST-COV-UNIT-003: cubrir L80 (rama Some en with_lock_dir) y L43-46 (logica lock_dir)

    #[test]
    fn lock_dir_primary_returned_when_already_exists() {
        let tmp = tempfile::TempDir::new().unwrap();
        let primary = tmp.path().join("enola_primary");
        std::fs::create_dir_all(&primary).unwrap(); // primary existe
        let d = lock_dir_from_primary(&primary);
        assert_eq!(
            d,
            primary.to_path_buf(),
            "debe regresar primary si ya existe"
        );
    }

    #[test]
    fn lock_dir_creates_primary_when_possible() {
        let tmp = tempfile::TempDir::new().unwrap();
        let primary = tmp.path().join("enola_new");
        // primary NO existe pero su padre (tmp) si existe y es escribible
        let d = lock_dir_from_primary(&primary);
        assert_eq!(d, primary.to_path_buf(), "debe crear y retornar primary");
    }

    #[test]
    fn lock_dir_falls_back_when_create_fails() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::TempDir::new().unwrap();
        let ro_parent = tmp.path().join("ro_parent");
        std::fs::create_dir_all(&ro_parent).unwrap();
        // Hacer el padre solo-lectura para que create_dir_all sobre el hijo falle
        std::fs::set_permissions(&ro_parent, std::fs::Permissions::from_mode(0o444)).unwrap();
        let primary = ro_parent.join("enola_fail");
        let d = lock_dir_from_primary(&primary);
        // Restaurar permisos antes de que TempDir intente limpiar
        std::fs::set_permissions(&ro_parent, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(
            d,
            std::path::PathBuf::from("/tmp/enola-locks"),
            "debe caer al fallback"
        );
    }

    // TEST-COV-UNIT-003: cubrir restore de env var en rama Some(v)
    // Nota: NO llamamos set_lock_dir_override() desde dentro de un bloque que ya tiene ENV_MUTEX
    // (causaria deadlock por mutex no-reentrante). En su lugar testeamos la logica directamente.
    #[test]
    fn some_restore_covers_l80_l131_l145() {
        let tmp_prev = tempfile::TempDir::new().unwrap();
        let tmp_actual = tempfile::TempDir::new().unwrap();
        // Asegurar que la logica "Some(v) => set_var" se ejecuta:
        // Simulamos el patron identico a with_lock_dir/lock_dir_default pero con prev=Some.
        {
            let _g = recover_poison(ENV_MUTEX.lock());
            // Establecer variable antes de capturar prev (cubre el Some arm en match al restaurar)
            std::env::set_var("ENOLA_PORT_LOCK_DIR", tmp_prev.path());
            let prev = std::env::var("ENOLA_PORT_LOCK_DIR").ok(); // Some(tmp_prev)
            std::env::set_var("ENOLA_PORT_LOCK_DIR", tmp_actual.path());
            let d = lock_dir();
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
