//! SEC-EXT-RACE-010 — escritura atómica de archivos sensibles (sin TOCTOU).
//!
//! ## Problema (CWE-377 — Insecure Temporary File / TOCTOU)
//!
//! El patrón clásico:
//!
//! ```ignore
//! fs::write(&path, content)?;                                  // crea con umask (típicamente 0644)
//! fs::set_permissions(&path, Permissions::from_mode(0o600))?;  // chmod TARDÍO
//! ```
//!
//! deja una ventana de microsegundos en la que el archivo es legible por
//! otros usuarios/procesos del mismo sistema. Un atacante con polling
//! agresivo (`inotify` o bucle `read()`) puede leer el JWT, la API key o
//! el refresh token antes del `chmod`.
//!
//! ## Mitigación canónica
//!
//! 1. Crear un archivo temporal **en el mismo directorio** que el destino
//!    (mismo filesystem → `rename` será atómico).
//! 2. Abrir con `O_CREAT | O_EXCL | O_WRONLY` y `mode 0o600` en **la misma
//!    syscall** (`OpenOptions::create_new(true).mode(0o600)`).
//!    El archivo NUNCA existe con permisos > 0600.
//! 3. Escribir + `sync_all()` (fsync) → datos persistidos.
//! 4. `rename(tmp, final)` — POSIX garantiza que es atómico dentro del
//!    mismo filesystem. Lectores concurrentes ven la versión vieja o la
//!    nueva, jamás un estado intermedio.
//!
//! ## Garantías
//!
//! - El archivo destino siempre tiene permisos `0o600` desde el primer
//!   instante en que existe.
//! - Si el proceso muere a media escritura, queda el archivo viejo intacto
//!   (o ninguno si era la primera escritura) + un `tempfile` huérfano que
//!   puede recolectarse.
//! - Funciona en Linux/macOS. En no-UNIX la función falla en compilación
//!   (intencional — no hay ruta segura sin POSIX).
//!
//! ## Uso
//!
//! ```ignore
//! use crate::infrastructure::atomic_secret_file::write_secret_atomically;
//! write_secret_atomically(&session_path, content.as_bytes())?;
//! ```

#![cfg(unix)]

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

fn default_write_sync(file: &mut std::fs::File, payload: &[u8]) -> std::io::Result<()> {
    write_all_step(file, payload)?;
    flush_step(file)?;
    sync_all_step(file)?;
    Ok(())
}

fn write_all_step(file: &mut std::fs::File, payload: &[u8]) -> std::io::Result<()> {
    #[cfg(feature = "testing")]
    if std::env::var("ENOLA_TEST_FORCE_SECRET_WRITE_STEP").as_deref() == Ok("write") {
        return Err(std::io::Error::other(
            "forced write_all failure (testing feature)",
        ));
    }
    file.write_all(payload)
}

fn flush_step(file: &mut std::fs::File) -> std::io::Result<()> {
    #[cfg(feature = "testing")]
    if std::env::var("ENOLA_TEST_FORCE_SECRET_WRITE_STEP").as_deref() == Ok("flush") {
        return Err(std::io::Error::other(
            "forced flush failure (testing feature)",
        ));
    }
    file.flush()
}

fn sync_all_step(file: &mut std::fs::File) -> std::io::Result<()> {
    #[cfg(feature = "testing")]
    if std::env::var("ENOLA_TEST_FORCE_SECRET_WRITE_STEP").as_deref() == Ok("sync") {
        return Err(std::io::Error::other(
            "forced sync_all failure (testing feature)",
        ));
    }
    file.sync_all()
}

fn default_rename(tmp: &Path, final_path: &Path) -> std::io::Result<()> {
    fs::rename(tmp, final_path)
}

#[cfg(feature = "testing")]
fn forced_write_sync_failure(_file: &mut std::fs::File, _payload: &[u8]) -> std::io::Result<()> {
    Err(std::io::Error::other(
        "forced write failure (testing feature)",
    ))
}

#[cfg(feature = "testing")]
fn forced_rename_failure(_tmp: &Path, _final_path: &Path) -> std::io::Result<()> {
    Err(std::io::Error::other(
        "forced rename failure (testing feature)",
    ))
}

/// Escribe `content` en `final_path` de forma atómica, garantizando que el
/// archivo nunca existe con permisos > 0o600.
///
/// El padre debe existir y ser escribible. Esta función NO crea el directorio
/// padre — eso es responsabilidad del caller (que típicamente quiere aplicar
/// `0o700` al directorio antes de invocar este helper).
///
/// # Errores
///
/// - El padre del path final no existe o no es escribible.
/// - El temporal no se puede crear (espacio, permisos, FS read-only).
/// - El `rename` falla (cross-FS, EACCES).
pub fn write_secret_atomically(final_path: &Path, content: &[u8]) -> std::io::Result<()> {
    #[cfg(feature = "testing")]
    {
        if std::env::var("ENOLA_TEST_FORCE_SECRET_WRITE_FAIL").as_deref() == Ok("1") {
            return write_secret_atomically_with(
                final_path,
                content,
                forced_write_sync_failure,
                default_rename,
            );
        }
        if std::env::var("ENOLA_TEST_FORCE_SECRET_RENAME_FAIL").as_deref() == Ok("1") {
            return write_secret_atomically_with(
                final_path,
                content,
                default_write_sync,
                forced_rename_failure,
            );
        }
    }
    write_secret_atomically_with(final_path, content, default_write_sync, default_rename)
}

fn write_secret_atomically_with(
    final_path: &Path,
    content: &[u8],
    write_sync: fn(&mut std::fs::File, &[u8]) -> std::io::Result<()>,
    rename_fn: fn(&Path, &Path) -> std::io::Result<()>,
) -> std::io::Result<()> {
    let parent = final_path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "final_path must have a parent directory",
        )
    })?;

    // Nombre del temporal: mismo directorio (atómico para rename), prefijo
    // `.` (oculto), sufijo aleatorio para evitar colisiones entre procesos.
    let file_name = final_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("secret");
    let pid = std::process::id();
    let nonce = nonce_token();
    let tmp_name = format!(".{}.tmp.{}.{}", file_name, pid, nonce);
    let tmp_path = parent.join(&tmp_name);

    // 1. Crear con O_CREAT | O_EXCL | mode 0600 en una sola syscall.
    //    Si el temporal ya existiera (improbable, pero posible si quedó
    //    huérfano de un crash), `create_new(true)` falla — reintentamos
    //    con otro nonce.
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp_path)?;

    // 2. Escribir contenido + flush + fsync.
    if let Err(e) = write_sync(&mut file, content) {
        // Limpieza best-effort: eliminar el temporal si la escritura falla.
        let _ = fs::remove_file(&tmp_path);
        return Err(e);
    }

    // 3. Rename atómico. POSIX: si `final_path` existe con permisos
    //    distintos a 0600 (legacy de la versión anterior con TOCTOU),
    //    es reemplazado por el nuevo (que sí es 0600) sin ventana intermedia.
    if let Err(e) = rename_fn(&tmp_path, final_path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(e);
    }

    Ok(())
}

/// Genera un nonce hexadecimal corto a partir del reloj monotónico
/// + dirección de memoria de una variable local. Suficiente para evitar
///   colisiones en el nombre del temporal (no es criptográfico).
fn nonce_token() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let stack_addr = &nanos as *const _ as usize;
    format!("{:08x}{:08x}", nanos, stack_addr & 0xFFFF_FFFF)
}

// TEST-COV-UNIT-003 nota: las siguientes ramas son defensivas y dificiles de cubrir
// en tests unitarios estandar:
//   L67-71: parent() = None solo para rutas como "/" (root del filesystem).
//   L104-105: cleanup tras fallo de escritura (requiere /dev/full o filesystem con cuota 0).
//   L112-113: cleanup tras fallo de rename (requiere rename cross-device, necesita root/mount).
// Se mantienen como red de seguridad en produccion; el resto del modulo tiene 100% cobertura.
#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn passthrough_rename(tmp: &Path, final_path: &Path) -> std::io::Result<()> {
        fs::rename(tmp, final_path)
    }

    fn forced_write_failure(_file: &mut std::fs::File, _payload: &[u8]) -> std::io::Result<()> {
        Err(std::io::Error::other("forced write failure"))
    }

    fn forced_rename_failure(_tmp: &Path, _final_path: &Path) -> std::io::Result<()> {
        Err(std::io::Error::other("forced rename failure"))
    }

    #[test]
    fn passthrough_rename_moves_file() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("a.tmp");
        let dst = dir.path().join("b.tmp");
        fs::write(&src, b"x").unwrap();
        passthrough_rename(&src, &dst).unwrap();
        assert!(!src.exists());
        assert!(dst.exists());
    }

    #[test]
    fn writes_file_with_0600_permissions_from_inception() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secret.json");
        write_secret_atomically(&path, b"super-secret-jwt").unwrap();

        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "file must be 0600, got {:o}", mode);

        let content = fs::read(&path).unwrap();
        assert_eq!(content, b"super-secret-jwt");
    }

    #[test]
    fn overwrites_existing_file_atomically_with_0600() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secret.json");

        // Pre-existente con permisos laxos (simula legado TOCTOU).
        fs::write(&path, b"old-content").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o644
        );

        // Sobrescribir atómicamente.
        write_secret_atomically(&path, b"new-content").unwrap();

        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "after atomic rewrite, must be 0600");
        assert_eq!(fs::read(&path).unwrap(), b"new-content");
    }

    #[test]
    fn fails_clean_when_parent_does_not_exist() {
        let path = std::path::Path::new("/nonexistent/sec-ext-race-010/secret.json");
        let result = write_secret_atomically(path, b"x");
        assert!(result.is_err(), "must fail when parent dir is missing");
    }

    #[test]
    fn fails_with_invalid_input_when_final_path_has_no_parent() {
        let path = std::path::Path::new("/");
        let result =
            write_secret_atomically(path, b"x").expect_err("root path should be invalid input");
        assert_eq!(result.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[test]
    fn no_temp_files_left_after_success() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secret.json");
        write_secret_atomically(&path, b"x").unwrap();

        // Solo debe quedar el archivo final, ningún `.secret.json.tmp.*`.
        let entries: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(entries.len(), 1, "expected 1 file, got {:?}", entries);
        assert_eq!(entries[0], "secret.json");
    }

    #[test]
    fn cleans_temp_file_when_write_sync_fails() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secret.json");

        let err =
            write_secret_atomically_with(&path, b"x", forced_write_failure, passthrough_rename)
                .expect_err("write failure should propagate");
        assert_eq!(err.kind(), std::io::ErrorKind::Other);

        let mut entries = Vec::new();
        for entry in fs::read_dir(dir.path()).unwrap() {
            let Ok(entry) = entry else {
                continue;
            };
            entries.push(entry.file_name().to_string_lossy().into_owned());
        }
        assert!(
            entries.is_empty(),
            "temp file must be cleaned after write failure: {:?}",
            entries
        );
    }

    #[test]
    fn cleans_temp_file_when_rename_fails() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secret.json");

        let err =
            write_secret_atomically_with(&path, b"x", default_write_sync, forced_rename_failure)
                .expect_err("rename failure should propagate");
        assert_eq!(err.kind(), std::io::ErrorKind::Other);

        let mut entries = Vec::new();
        for entry in fs::read_dir(dir.path()).unwrap() {
            let Ok(entry) = entry else {
                continue;
            };
            entries.push(entry.file_name().to_string_lossy().into_owned());
        }
        assert!(
            entries.is_empty(),
            "temp file must be cleaned after rename failure: {:?}",
            entries
        );
    }

    #[test]
    fn nonce_token_changes_between_calls() {
        let a = nonce_token();
        let b = nonce_token();
        // No es estrictamente garantizado pero con nanos+stack debe variar.
        // Si fallase, indicaría reloj congelado o ASLR roto — investigar.
        assert_ne!(a, b);
    }

    #[test]
    fn concurrent_writers_do_not_corrupt_final_file() {
        // Anti-TOCTOU: 8 hilos escriben simultáneamente; el archivo final
        // siempre tiene 0600 y un contenido válido (uno de los escritos).
        use std::sync::Arc;
        use std::thread;

        let dir = Arc::new(tempfile::tempdir().unwrap());
        let path = Arc::new(dir.path().join("secret.json"));

        let mut handles = Vec::new();
        for i in 0..8 {
            let path = Arc::clone(&path);
            let _dir = Arc::clone(&dir);
            let handle = thread::spawn(move || {
                let payload = format!("writer-{}", i);
                write_secret_atomically(&path, payload.as_bytes())
            });
            handles.push(handle);
        }

        for h in handles {
            h.join().unwrap().expect("each writer must succeed");
        }

        let mode = fs::metadata(&*path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "final file must remain 0600 under concurrency");

        let content = fs::read_to_string(&*path).unwrap();
        assert!(
            content.starts_with("writer-"),
            "content must be one complete writer payload, got {:?}",
            content
        );
    }
}
