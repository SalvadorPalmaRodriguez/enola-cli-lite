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
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o644)
            .open(path_ref)?;
        // SAFETY: `flock` con un fd válido no tiene UB. Los flags están bien
        // documentados en flock(2). El fd vive mientras `file` esté vivo.
        let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if rc != 0 {
            let err = io::Error::last_os_error();
            return Err(map_flock_error(err, path_ref));
        }
        Ok(FileLock {
            _file: file,
            path: path_ref.to_path_buf(),
        })
    }
    /// Path del lockfile (útil para mensajes de error de diagnóstico).
    pub fn path(&self) -> &Path {
        &self.path
    }
}
// `Drop` no necesita explícitamente `flock(LOCK_UN)`: cerrar el fd libera el lock.
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
}
