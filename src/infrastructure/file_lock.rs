// SEC-EXT-RACE-011: Bloqueo exclusivo de fichero (advisory lock) basado en `flock`.
//
// **Propósito**: serializar operaciones que en `enola-cli` tienen una ventana TOCTOU
// entre "validar disponibilidad de un recurso" y "consumirlo" (típicamente un puerto
// asignado a `docker run -p`, pero también artefactos compartidos en `/opt/enola`,
// `/etc/systemd/system`, `/etc/nginx`). Sin esto, dos invocaciones concurrentes del
// CLI pueden ambas validar el mismo recurso como libre y luego pisarse mutuamente.
//
// **Diseño**:
// - Lock advisory POSIX `flock(LOCK_EX | LOCK_NB)` (no bloqueante por defecto).
// - El lock se libera automáticamente al cerrar el fd (Drop) — incluso si el
//   proceso panica o muere abruptamente, el kernel libera el lock.
// - Fichero vacío (solo el lock importa). Permisos 0644 (no contiene secretos).
// - Directorio padre con `create_dir_all` y permisos 0755.
//
// **Reglas para uso correcto** (§13.69):
// 1. Adquirir el lock ANTES de cualquier validación del recurso.
// 2. Mantener el `FileLock` vivo hasta DESPUÉS de consumir el recurso.
// 3. NO usar `LOCK_SH` (shared) sin justificación: la idea es serializar.
// 4. `try_acquire` (no bloqueante) es el modo preferido — falla rápido con error claro.
use crate::infrastructure::safe_dir::{OwnerPolicy, SafeDir};
use std::fs::{File, OpenOptions};
use std::io;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};

fn map_flock_error(err: io::Error, path_ref: &Path) -> io::Error {
    if err.raw_os_error() == Some(libc::EWOULDBLOCK) {
        io::Error::new(
            io::ErrorKind::WouldBlock,
            format!("another process holds the lock for {}", path_ref.display()),
        )
    } else {
        err
    }
}
/// Guard RAII de un lock exclusivo sobre un fichero.
///
/// Mientras este valor esté vivo, ningún otro proceso (en la misma máquina,
/// mismo kernel) podrá adquirir un `FileLock` sobre el mismo path. Al hacer
/// `drop` (o al morir el proceso) el lock se libera automáticamente porque el
/// kernel cierra el fd y `flock` se desvincula.
#[must_use = "FileLock must be kept alive until the protected operation finishes; \
               dropping it immediately releases the lock and reintroduces the TOCTOU window"]
#[derive(Debug)]
pub struct FileLock {
    // Mantener el `File` vivo conserva el fd abierto, y por tanto el flock.
    _file: File,
    path: PathBuf,
}
impl FileLock {
    /// Intenta adquirir un lock exclusivo NO BLOQUEANTE sobre `path`.
    ///
    /// Crea el fichero (y su directorio padre) si no existen. Si otro proceso
    /// ya tiene el lock, devuelve `io::ErrorKind::WouldBlock` inmediatamente.
    pub fn try_acquire<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let path_ref = path.as_ref();
        if let Some(parent) = path_ref.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent)?;
                let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o755));
            }
        }
        // SEC-A2: O_NOFOLLOW — un lockfile sustituido por un symlink no se sigue.
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o644)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path_ref)?;
        if !file.metadata()?.file_type().is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{}: lockfile is not a regular file", path_ref.display()),
            ));
        }
        Self::lock_file(file, path_ref.to_path_buf())
    }
    /// SEC-A2: adquiere el lock `name` dentro de un directorio ya validado,
    /// sin seguir symlinks (el lockfile debe ser un fichero regular).
    pub fn try_acquire_in(dir: &SafeDir, name: &str) -> io::Result<Self> {
        let path = dir.path().join(name);
        let file = dir.open_file_nofollow(name, 0o644)?;
        Self::lock_file(file, path)
    }

    fn lock_file(file: File, path: PathBuf) -> io::Result<Self> {
        // SAFETY: `flock` con un fd válido no tiene UB. Los flags están bien
        // documentados en flock(2). El fd vive mientras `file` esté vivo.
        let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if rc != 0 {
            let err = io::Error::last_os_error();
            return Err(map_flock_error(err, &path));
        }
        Ok(FileLock { _file: file, path })
    }

    /// Path del lockfile (útil para mensajes de error de diagnóstico).
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        // Desbloquea explícitamente la open file description en el kernel antes de
        // cerrar el fd: si otro hilo hizo fork() y aún no ejecutó execve() (O_CLOEXEC),
        // el refcount del struct file sería >1 y close(fd) por sí solo no liberaría el
        // flock hasta que el hijo completara execve.
        unsafe {
            let _ = libc::flock(self._file.as_raw_fd(), libc::LOCK_UN);
        }
    }
}
/// SEC-A2 (D1): directorio canónico de locks cuando el CLI corre como root.
/// `/run` es root 0755 (nadie más puede pre-crear `/run/enola`); `/run/lock`
/// (1777) permitía pre-crear o sustituir el directorio tras cada arranque.
pub const ROOT_LOCK_PARENT: &str = "/run/enola";
/// Ver [`ROOT_LOCK_PARENT`].
pub const ROOT_LOCK_DIR: &str = "/run/enola/locks";

/// Devuelve el directorio de locks validado (dueño = euid, sin symlink, sin
/// escritura de grupo/otros). `override_env` permite fijarlo (tests, dev);
/// el override pasa por las mismas comprobaciones. Sin override y sin root →
/// error explícito (D2: sin fallback a `/tmp`).
pub fn secure_lock_dir(override_env: &str) -> io::Result<SafeDir> {
    if let Ok(custom) = std::env::var(override_env) {
        if !custom.is_empty() {
            return SafeDir::open_or_create(Path::new(&custom), 0o755, OwnerPolicy::Euid);
        }
    }
    // SAFETY: geteuid no tiene precondiciones.
    let euid = unsafe { libc::geteuid() };
    secure_lock_dir_at(Path::new(ROOT_LOCK_PARENT), Path::new(ROOT_LOCK_DIR), euid)
}

/// Núcleo testeable de [`secure_lock_dir`] sin override.
fn secure_lock_dir_at(parent: &Path, dir: &Path, euid: u32) -> io::Result<SafeDir> {
    if euid != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "port/artifact locks live in {} and require root (run with sudo)",
                dir.display()
            ),
        ));
    }
    SafeDir::open_or_create(parent, 0o755, OwnerPolicy::Euid)?;
    SafeDir::open_or_create(dir, 0o755, OwnerPolicy::Euid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;
    use tempfile::TempDir;
    fn lock_path(dir: &TempDir, name: &str) -> PathBuf {
        dir.path().join(format!("{}.lock", name))
    }
    #[test]
    fn acquires_lock_and_creates_file() {
        let tmp = TempDir::new().unwrap();
        let p = lock_path(&tmp, "a");
        let _g = FileLock::try_acquire(&p).expect("acquire");
        assert!(p.exists(), "lockfile must be created");
        let mode = std::fs::metadata(&p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o644, "lockfile mode must be 0644");
    }
    #[test]
    fn second_acquire_fails_with_wouldblock_while_first_alive() {
        let tmp = TempDir::new().unwrap();
        let p = lock_path(&tmp, "b");
        let _g1 = FileLock::try_acquire(&p).expect("first acquire");
        let err = FileLock::try_acquire(&p).expect_err("second must fail");
        assert_eq!(err.kind(), io::ErrorKind::WouldBlock);
    }
    #[test]
    fn lock_releases_on_drop_and_can_be_reacquired() {
        let tmp = TempDir::new().unwrap();
        let p = lock_path(&tmp, "c");
        {
            let _g = FileLock::try_acquire(&p).expect("acquire");
        } // drop here releases
        let _g2 = FileLock::try_acquire(&p).expect("re-acquire after drop");
    }
    #[test]
    fn creates_parent_directory_on_demand() {
        let tmp = TempDir::new().unwrap();
        let nested = tmp.path().join("a/b/c/deep.lock");
        let _g = FileLock::try_acquire(&nested).expect("nested dir created");
        assert!(nested.parent().unwrap().exists());
    }

    #[test]
    fn empty_path_returns_error_without_parent_handling() {
        let err = FileLock::try_acquire(Path::new(""));
        assert!(err.is_err());
    }

    #[test]
    fn concurrent_threads_only_one_holds_lock_at_a_time() {
        // 8 hilos contienden; verificamos que la sección crítica nunca tiene > 1
        // hilo simultáneo. Si flock funciona, max_in_flight == 1.
        let tmp = TempDir::new().unwrap();
        let p = Arc::new(lock_path(&tmp, "concurrent"));
        let max_in_flight = Arc::new(AtomicUsize::new(0));
        let in_flight = Arc::new(AtomicUsize::new(0));
        let succeeded = Arc::new(AtomicUsize::new(0));
        let mut handles = vec![];
        for _ in 0..8 {
            let p = Arc::clone(&p);
            let in_flight = Arc::clone(&in_flight);
            let max_in_flight = Arc::clone(&max_in_flight);
            let succeeded = Arc::clone(&succeeded);
            handles.push(thread::spawn(move || {
                let deadline = std::time::Instant::now() + Duration::from_secs(5);
                let mut acquired = false;
                while std::time::Instant::now() < deadline {
                    match FileLock::try_acquire(&*p) {
                        Ok(_g) => {
                            let cur = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
                            max_in_flight.fetch_max(cur, Ordering::SeqCst);
                            thread::sleep(Duration::from_millis(20));
                            in_flight.fetch_sub(1, Ordering::SeqCst);
                            succeeded.fetch_add(1, Ordering::SeqCst);
                            acquired = true;
                            break;
                        }
                        Err(_e) => thread::sleep(Duration::from_millis(5)),
                    }
                }
                acquired
            }));
        }
        for h in handles {
            assert!(
                h.join().unwrap(),
                "thread did not acquire lock within deadline"
            );
        }
        assert_eq!(
            succeeded.load(Ordering::SeqCst),
            8,
            "all 8 threads must have entered the CS"
        );
        assert_eq!(
            max_in_flight.load(Ordering::SeqCst),
            1,
            "flock must guarantee at most 1 holder at a time"
        );
    }

    // TEST-COV-UNIT-003: cubrir el metodo path() (L76-78)
    #[test]
    fn path_method_returns_lockfile_path() {
        let tmp = TempDir::new().unwrap();
        let p = lock_path(&tmp, "path_method");
        let lock = FileLock::try_acquire(&p).expect("acquire");
        assert_eq!(
            lock.path(),
            p.as_path(),
            "path() debe devolver la ruta del lockfile"
        );
    }

    #[test]
    fn map_flock_error_maps_ewouldblock_to_wouldblock() {
        let p = PathBuf::from("/tmp/test.lock");
        let err = io::Error::from_raw_os_error(libc::EWOULDBLOCK);
        let mapped = map_flock_error(err, &p);
        assert_eq!(mapped.kind(), io::ErrorKind::WouldBlock);
        assert!(mapped
            .to_string()
            .contains("another process holds the lock"));
    }

    #[test]
    fn map_flock_error_preserves_non_ewouldblock_errors() {
        let p = PathBuf::from("/tmp/test.lock");
        let err = io::Error::from_raw_os_error(libc::EACCES);
        let mapped = map_flock_error(err, &p);
        assert_eq!(mapped.raw_os_error(), Some(libc::EACCES));
    }

    // ── SEC-A2 ────────────────────────────────────────────────────────────
    #[test]
    fn try_acquire_rejects_symlinked_lockfile() {
        let tmp = TempDir::new().unwrap();
        let victim = tmp.path().join("victim");
        std::fs::write(&victim, b"keep").unwrap();
        let p = lock_path(&tmp, "sym");
        std::os::unix::fs::symlink(&victim, &p).unwrap();
        assert!(
            FileLock::try_acquire(&p).is_err(),
            "must not follow symlink"
        );
        assert_eq!(std::fs::read(&victim).unwrap(), b"keep");
    }

    #[test]
    fn try_acquire_in_locks_exclusively_inside_safe_dir() {
        let tmp = TempDir::new().unwrap();
        // TempDir respeta el umask (p. ej. 002 → 0775): fijar un modo privado.
        std::fs::set_permissions(tmp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let dir = SafeDir::open(tmp.path(), OwnerPolicy::Euid).unwrap();
        let _g = FileLock::try_acquire_in(&dir, "port_1.lock").expect("acquire");
        let err = FileLock::try_acquire_in(&dir, "port_1.lock").expect_err("second");
        assert_eq!(err.kind(), io::ErrorKind::WouldBlock);
        assert!(tmp.path().join("port_1.lock").is_file());
    }

    #[test]
    fn try_acquire_in_rejects_symlinked_lockfile() {
        let tmp = TempDir::new().unwrap();
        let victim = tmp.path().join("victim");
        std::fs::write(&victim, b"keep").unwrap();
        let locks = tmp.path().join("locks");
        std::fs::create_dir(&locks).unwrap();
        std::fs::set_permissions(&locks, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::os::unix::fs::symlink(&victim, locks.join("port_2.lock")).unwrap();
        let dir = SafeDir::open(&locks, OwnerPolicy::Euid).unwrap();
        assert!(FileLock::try_acquire_in(&dir, "port_2.lock").is_err());
        assert_eq!(std::fs::read(&victim).unwrap(), b"keep");
    }

    #[test]
    fn secure_lock_dir_at_requires_root_without_override() {
        let tmp = TempDir::new().unwrap();
        let err = secure_lock_dir_at(&tmp.path().join("p"), &tmp.path().join("p/l"), 1000)
            .expect_err("non-root without override must fail (D2)");
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
        assert!(!tmp.path().join("p").exists(), "nothing created on failure");
    }

    #[test]
    fn secure_lock_dir_at_creates_parent_and_dir_0755() {
        let tmp = TempDir::new().unwrap();
        let parent = tmp.path().join("enola");
        let dir = parent.join("locks");
        let _d = secure_lock_dir_at(&parent, &dir, 0).expect("create");
        for p in [&parent, &dir] {
            let md = std::fs::symlink_metadata(p).unwrap();
            assert!(md.is_dir());
            assert_eq!(md.permissions().mode() & 0o777, 0o755);
        }
    }

    #[test]
    fn secure_lock_dir_at_rejects_symlinked_lock_dir() {
        let tmp = TempDir::new().unwrap();
        let parent = tmp.path().join("enola");
        std::fs::create_dir(&parent).unwrap();
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o755)).unwrap();
        let elsewhere = tmp.path().join("elsewhere");
        std::fs::create_dir(&elsewhere).unwrap();
        std::os::unix::fs::symlink(&elsewhere, parent.join("locks")).unwrap();
        assert!(secure_lock_dir_at(&parent, &parent.join("locks"), 0).is_err());
    }

    #[test]
    fn secure_lock_dir_at_rejects_world_writable_dirs() {
        let tmp = TempDir::new().unwrap();
        let parent = tmp.path().join("enola");
        std::fs::create_dir(&parent).unwrap();
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o777)).unwrap();
        assert!(secure_lock_dir_at(&parent, &parent.join("locks"), 0).is_err());
    }

    #[test]
    fn secure_lock_dir_override_is_validated() {
        let _g = crate::infrastructure::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = TempDir::new().unwrap();
        let ww = tmp.path().join("ww");
        std::fs::create_dir(&ww).unwrap();
        std::fs::set_permissions(&ww, std::fs::Permissions::from_mode(0o777)).unwrap();
        std::env::set_var("ENOLA_TEST_SEC_LOCK_DIR", &ww);
        let rejected = secure_lock_dir("ENOLA_TEST_SEC_LOCK_DIR").is_err();
        std::fs::set_permissions(tmp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        std::env::set_var("ENOLA_TEST_SEC_LOCK_DIR", tmp.path());
        let accepted = secure_lock_dir("ENOLA_TEST_SEC_LOCK_DIR").is_ok();
        std::env::remove_var("ENOLA_TEST_SEC_LOCK_DIR");
        assert!(rejected, "world-writable override must be rejected");
        assert!(accepted, "own private override must be accepted");
    }
}
