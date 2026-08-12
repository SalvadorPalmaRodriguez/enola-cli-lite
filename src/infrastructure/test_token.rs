// ═══════════════════════════════════════════════════════════════════════════
// Test Token Module  — SOLO DISPONIBLE CON FEATURE FLAG `testing`
// ═══════════════════════════════════════════════════════════════════════════
//
// Este módulo implementa un mecanismo seguro para saltar la verificación de
// autenticación en tests E2E. Reemplaza la antigua variable de entorno
// `ENOLA_SKIP_AUTH=1` que era explotable por cualquier proceso.
//
// SEGURIDAD:
//   - Este módulo SOLO COMPILA si se activa `--features testing`.
//   - Los binarios de producción (`cargo build --release`) NO contienen
//     este código. Es imposible activar el bypass en producción.
//   - El token es HMAC-SHA256 con timestamp embebido (TTL = 5 minutos).
//   - Requiere acceso al archivo de clave local `~/.enola/test.key`.
//     Sin ese archivo, no se puede generar un token válido.
//
// FLUJO:
//   1. CI/Dev genera el token: `sudo enola-cli dev test-token`
//      → Lee ~/.enola/test.key (o lo crea si no existe)
//      → Genera HMAC-SHA256(timestamp_unix + secret)
//      → Imprime: "enola_test_<timestamp>_<hmac_hex>"
//   2. El script de test exporta: `export ENOLA_TEST_TOKEN=<token>`
//   3. El executor verifica: `verify_test_token_from_env()`
//      → Extrae timestamp del token
//      → Verifica que no han pasado más de 5 minutos
//      → Verifica el HMAC con la clave local
//      → Solo si todo es correcto → salta la verificación de licencia

#![cfg(feature = "testing")]

use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::time::{SystemTime, UNIX_EPOCH};

type HmacSha256 = Hmac<Sha256>;

/// TTL del token de test: 5 minutos (300 segundos)
const TEST_TOKEN_TTL_SECS: u64 = 300;

/// Variable de entorno que transporta el token de test
pub const ENV_TEST_TOKEN: &str = "ENOLA_TEST_TOKEN";

/// Prefijo del token para identificación rápida
const TOKEN_PREFIX: &str = "enola_test_";

/// Error del sistema de test tokens
#[derive(Debug, thiserror::Error)]
pub enum TestTokenError {
    #[error(
        "Token de test expirado (TTL: {0}s). Genera uno nuevo con: sudo enola-cli dev test-token"
    )]
    Expired(u64),

    #[error("Token de test inválido o manipulado")]
    InvalidSignature,

    #[error("Formato de token incorrecto")]
    MalformedToken,

    #[error("No se encontró la clave de test: {0}. Ejecuta: sudo enola-cli dev setup-test-key")]
    KeyNotFound(String),

    #[error("Error al leer/escribir la clave de test: {0}")]
    KeyIo(String),

    #[error("Error interno de test token: {0}")]
    Internal(String),
}

/// Obtiene la ruta del archivo de clave de test
fn get_key_path() -> std::path::PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("/root"));
    home.join(".enola").join("test.key")
}

/// Crea o lee la clave secreta de test.
///
/// La clave se genera aleatoriamente en la primera invocación y se guarda
/// en `~/.enola/test.key` con permisos 0600.
///
/// LOW-03: Si la clave tiene más de 30 días (según mtime), se regenera
/// automáticamente. No compartir ni backupear `test.key`.
///
/// # Errors
/// Retorna error si no se puede leer ni crear el archivo.
pub fn get_or_create_test_key() -> Result<Vec<u8>, TestTokenError> {
    use rand::RngCore;
    use std::os::unix::fs::OpenOptionsExt;

    let key_path = get_key_path();
    const KEY_MAX_AGE_SECS: u64 = 30 * 24 * 60 * 60; // 30 days

    if key_path.exists() {
        let raw = std::fs::read(&key_path)
            .map_err(|e| TestTokenError::KeyIo(format!("read {}: {}", key_path.display(), e)))?;
        if raw.len() < 32 {
            return Err(TestTokenError::KeyIo(
                "Clave de test corrupta (< 32 bytes). Borra ~/.enola/test.key y regenera.".into(),
            ));
        }
        // LOW-03: Check if key is older than 30 days — regenerate if so.
        if let Ok(metadata) = std::fs::metadata(&key_path) {
            if let Ok(mtime) = metadata.modified() {
                if let Ok(elapsed) = mtime.elapsed() {
                    if elapsed.as_secs() > KEY_MAX_AGE_SECS {
                        tracing::warn!(
                            "Test key en {} tiene >30 días — regenerando.",
                            key_path.display()
                        );
                        // Fall through to regeneration below.
                    } else {
                        return Ok(raw);
                    }
                } else {
                    return Ok(raw);
                }
            } else {
                return Ok(raw);
            }
        } else {
            return Ok(raw);
        }
    }

    // Crear directorio si no existe
    if let Some(parent) = key_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| TestTokenError::KeyIo(format!("mkdir {}: {}", parent.display(), e)))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700)).map_err(
                |e| TestTokenError::KeyIo(format!("chmod 700 {}: {}", parent.display(), e)),
            )?;
        }
    }

    // Generar 32 bytes aleatorios
    let mut key = vec![0u8; 32];
    rand::thread_rng().fill_bytes(&mut key);

    // Escribir con permisos 0600 (solo propietario)
    std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&key_path)
        .and_then(|mut f| {
            use std::io::Write;
            f.write_all(&key)
        })
        .map_err(|e| TestTokenError::KeyIo(format!("write {}: {}", key_path.display(), e)))?;

    tracing::info!("Test key creada en {}", key_path.display());
    Ok(key)
}

/// Genera un token de test válido para los próximos `TEST_TOKEN_TTL_SECS` segundos.
///
/// Formato del token: `enola_test_<timestamp_unix>_<hmac_hex>`
///
/// # Errors
/// Retorna error si no se puede leer/crear la clave.
pub fn generate_test_token() -> Result<String, TestTokenError> {
    let key = get_or_create_test_key()?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let msg = format!("{}", now);
    let mut mac = HmacSha256::new_from_slice(&key)
        .map_err(|e| TestTokenError::Internal(format!("HMAC: {}", e)))?;
    mac.update(msg.as_bytes());
    let hmac_bytes = mac.finalize().into_bytes();
    let hmac_hex = hex_encode(&hmac_bytes);

    Ok(format!("{}{}_{}", TOKEN_PREFIX, now, hmac_hex))
}

/// Verifica un token de test.
///
/// Comprueba:
///   1. Formato correcto
///   2. No ha expirado (TTL = 5 minutos)
///   3. HMAC válido con la clave local
///
/// # Returns
/// `Ok(())` si el token es válido y no ha expirado.
pub fn verify_test_token(token: &str) -> Result<(), TestTokenError> {
    // Verificar prefijo
    let rest = token
        .strip_prefix(TOKEN_PREFIX)
        .ok_or(TestTokenError::MalformedToken)?;

    // Separar timestamp y hmac
    let sep = rest.rfind('_').ok_or(TestTokenError::MalformedToken)?;
    let timestamp_str = &rest[..sep];
    let received_hmac = &rest[sep + 1..];

    // Parsear timestamp
    let token_ts: u64 = timestamp_str
        .parse()
        .map_err(|_| TestTokenError::MalformedToken)?;

    // Verificar TTL
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let age = now.saturating_sub(token_ts);
    if age > TEST_TOKEN_TTL_SECS {
        return Err(TestTokenError::Expired(age));
    }

    // Verificar HMAC
    let key = get_or_create_test_key()?;
    let msg = format!("{}", token_ts);
    let mut mac = HmacSha256::new_from_slice(&key)
        .map_err(|e| TestTokenError::Internal(format!("HMAC: {}", e)))?;
    mac.update(msg.as_bytes());
    let expected_hmac_bytes = mac.finalize().into_bytes();
    let expected_hmac = hex_encode(&expected_hmac_bytes);

    // Comparación en tiempo constante (evita timing attacks)
    if !constant_time_eq(received_hmac.as_bytes(), expected_hmac.as_bytes()) {
        return Err(TestTokenError::InvalidSignature);
    }

    Ok(())
}

/// Lee `ENOLA_TEST_TOKEN` del entorno y lo verifica.
///
/// Esta función es la que llama `executor.rs` cuando se compila con
/// `--features testing`. Si no hay token o es inválido, retorna `false`.
///
/// En producción (sin feature `testing`) esta función NO EXISTE.
pub fn verify_test_token_from_env() -> bool {
    match std::env::var(ENV_TEST_TOKEN) {
        Ok(token) => match verify_test_token(&token) {
            Ok(()) => {
                tracing::debug!("Token de test válido — auth bypass activo");
                true
            }
            Err(e) => {
                tracing::warn!("Token de test inválido: {}", e);
                eprintln!("\x1b[1;33m⚠ ENOLA_TEST_TOKEN inválido: {}\x1b[0m", e);
                false
            }
        },
        Err(_) => false,
    }
}

/// Codificación hexadecimal simple (sin dependencia hex crate)
fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Comparación en tiempo constante para evitar timing attacks
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut result = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        result |= x ^ y;
    }
    result == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static HOME_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn test_generate_and_verify_token() {
        let token = generate_test_token().expect("debe generar token");
        assert!(token.starts_with(TOKEN_PREFIX));
        assert!(
            verify_test_token(&token).is_ok(),
            "token recién generado debe ser válido"
        );
    }

    #[test]
    fn test_expired_token_rejected() {
        // Generar token con timestamp en el pasado (hace 10 minutos)
        let key = get_or_create_test_key().unwrap();
        let old_ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            - 601; // 601s > TTL de 300s
        let msg = format!("{}", old_ts);
        let mut mac = HmacSha256::new_from_slice(&key).unwrap();
        mac.update(msg.as_bytes());
        let hmac_hex = hex_encode(&mac.finalize().into_bytes());
        let old_token = format!("{}{}_{}", TOKEN_PREFIX, old_ts, hmac_hex);

        match verify_test_token(&old_token) {
            Err(TestTokenError::Expired(_)) => {} // correcto
            other => panic!("debería ser Expired, got {:?}", other),
        }
    }

    #[test]
    fn test_tampered_token_rejected() {
        let token = generate_test_token().unwrap();
        // Cambiar último char del HMAC
        let mut tampered = token.clone();
        let last = tampered.pop().unwrap();
        tampered.push(if last == 'a' { 'b' } else { 'a' });

        match verify_test_token(&tampered) {
            Err(TestTokenError::InvalidSignature) => {} // correcto
            other => panic!("debería ser InvalidSignature, got {:?}", other),
        }
    }

    #[test]
    fn test_malformed_token_rejected() {
        assert!(verify_test_token("not_a_token").is_err());
        assert!(verify_test_token("enola_test_abc_xyz").is_err());
    }

    #[test]
    fn test_key_creation_enforces_0700_dir_and_0600_file_owner() {
        let _g = HOME_LOCK.lock().unwrap();
        let old_home = std::env::var("HOME").ok();
        let tmp = tempfile::TempDir::new().expect("tempdir");
        std::env::set_var("HOME", tmp.path());

        let key = get_or_create_test_key().expect("key creation");
        assert!(key.len() >= 32);

        #[cfg(unix)]
        {
            use std::os::unix::fs::{MetadataExt, PermissionsExt};
            let dir = tmp.path().join(".enola");
            let key_path = dir.join("test.key");

            let dir_meta = std::fs::metadata(&dir).expect("dir metadata");
            let dir_mode = dir_meta.permissions().mode() & 0o777;
            assert_eq!(dir_mode, 0o700, "~/.enola must be 0700");

            let file_meta = std::fs::metadata(&key_path).expect("key metadata");
            let file_mode = file_meta.permissions().mode() & 0o777;
            assert_eq!(file_mode, 0o600, "test.key must be 0600");

            let uid = unsafe { libc::geteuid() };
            assert_eq!(dir_meta.uid(), uid, "dir owner mismatch");
            assert_eq!(file_meta.uid(), uid, "key owner mismatch");
        }

        match old_home {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
    }

    #[test]
    fn test_old_key_is_regenerated() {
        let _g = HOME_LOCK.lock().unwrap();
        let old_home = std::env::var("HOME").ok();
        let tmp = tempfile::TempDir::new().expect("tempdir");
        std::env::set_var("HOME", tmp.path());

        // Create a key first
        let key1 = get_or_create_test_key().expect("first key creation");
        assert!(key1.len() >= 32);

        // Set the key's mtime to 31 days ago
        let key_path = tmp.path().join(".enola").join("test.key");
        let old_time =
            std::time::SystemTime::now() - std::time::Duration::from_secs(31 * 24 * 60 * 60);
        let file = std::fs::File::open(&key_path).expect("open key file");
        let _ = file.set_times(std::fs::FileTimes::new().set_modified(old_time));

        // Call again — should regenerate because mtime > 30 days
        let key2 = get_or_create_test_key().expect("key regeneration");
        assert!(key2.len() >= 32);
        // Keys should differ (new random key)
        assert_ne!(
            key1, key2,
            "old key should be regenerated with new random bytes"
        );

        match old_home {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
    }
}
