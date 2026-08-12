// RELEASE-VERIFY (PQC-030): verificación post-cuántica de releases desde enola-cli.
//
// Permite al USUARIO comprobar que el archivo descargado (tarball o binario) es
// legítimo, usando SOLO el binario distribuido `enola-cli`. La clave pública
// ML-DSA-65 (FIPS 204) se embebe en compilación (`include_str!` de
// `pqc_sign.pub`), por lo que la verificación:
//   - No requiere red, ni login, ni dependencias del sistema.
//   - No necesita el binario de desarrollo `enola-sign-pqc` (que pasa a ser
//     herramienta dev-only, feature `dev-tools`, DEV-TOOLS-001).
//
// Capas verificadas:
//   1. Firma ML-DSA-65 (`<file>.pqsig`)  — autenticidad post-cuántica (OBLIGATORIA).
//   2. SHA-256 (`<file>.sha256` hermano) — integridad (si existe; best-effort).
//
// Esta lógica vive en `application/` (orquestación) y usa los crates de cripto
// directamente, igual que `update_checker` usa `sha2`/`minisign`. La clave
// privada y la generación/firma siguen siendo exclusivas de `enola-sign-pqc`.

use ml_dsa::signature::Verifier;
use ml_dsa::{EncodedSignature, EncodedVerifyingKey, MlDsa65, Signature, VerifyingKey};
use serde_json::json;
use std::convert::TryFrom;
use std::fs;
use std::path::{Path, PathBuf};

/// Clave pública ML-DSA-65 del proyecto, embebida en compilación (PQC-030).
/// Es la misma `pqc_sign.pub` versionada en el repositorio.
const EMBEDDED_PQC_PUBKEY: &str = include_str!("../../pqc_sign.pub");

const SIG_EXTENSION: &str = "pqsig";
const SHA256_EXTENSION: &str = "sha256";

/// Resultado de la verificación de un release.
#[derive(Debug, Clone)]
pub struct VerifyReport {
    pub file: String,
    /// Firma ML-DSA-65 verificada correctamente.
    pub pqc_ok: bool,
    pub pqc_detail: String,
    /// Si se usó la clave pública embebida (`true`) o una externa (`false`).
    pub pqc_embedded_key: bool,
    /// Si existía un `<file>.sha256` y se comprobó.
    pub sha256_checked: bool,
    pub sha256_ok: bool,
    pub sha256_detail: String,
}

impl VerifyReport {
    /// La verificación se considera exitosa si la firma PQC es válida y, en caso
    /// de existir un `.sha256`, también coincide.
    pub fn success(&self) -> bool {
        self.pqc_ok && (!self.sha256_checked || self.sha256_ok)
    }

    pub fn json_value(&self) -> serde_json::Value {
        json!({
            "file": self.file,
            "verified": self.success(),
            "pqc": {
                "algorithm": "ML-DSA-65 (FIPS 204)",
                "ok": self.pqc_ok,
                "embedded_key": self.pqc_embedded_key,
                "detail": self.pqc_detail,
            },
            "sha256": {
                "checked": self.sha256_checked,
                "ok": self.sha256_ok,
                "detail": self.sha256_detail,
            },
        })
    }

    pub fn human_summary(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("Verificación de release: {}\n", self.file));

        let pqc_mark = if self.pqc_ok { "✅" } else { "❌" };
        let key_src = if self.pqc_embedded_key {
            "clave pública embebida"
        } else {
            "clave pública externa"
        };
        out.push_str(&format!(
            "  {} Firma post-cuántica ML-DSA-65 ({}) — {}\n",
            pqc_mark, key_src, self.pqc_detail
        ));

        if self.sha256_checked {
            let sha_mark = if self.sha256_ok { "✅" } else { "❌" };
            out.push_str(&format!(
                "  {} Integridad SHA-256 — {}\n",
                sha_mark, self.sha256_detail
            ));
        } else {
            out.push_str(&format!(
                "  ➖ Integridad SHA-256 — {}\n",
                self.sha256_detail
            ));
        }

        if self.success() {
            out.push_str("\n✅ El archivo es legítimo: firmado por el proyecto Enola.");
        } else {
            out.push_str("\n❌ NO se pudo verificar la legitimidad del archivo. NO lo ejecutes.");
        }
        out
    }
}

/// Extrae los datos hex de un archivo en formato minisign-like.
/// Ignora las líneas `untrusted comment:` y las vacías, y concatena el resto
/// (robusto frente a claves/firmas escritas en una o varias líneas).
fn parse_hex_body(content: &str) -> Result<Vec<u8>, String> {
    let body: String = content
        .lines()
        .filter(|l| !l.starts_with("untrusted comment") && !l.trim().is_empty())
        .map(|l| l.trim())
        .collect();
    if body.is_empty() {
        return Err("formato inválido (faltan datos hex)".to_string());
    }
    hex::decode(&body).map_err(|e| format!("hex inválido: {}", e))
}

/// Verifica la firma ML-DSA-65 de `file` contra `pubkey_content` (texto del
/// archivo de clave pública). Devuelve `Ok(())` si la firma es válida.
fn verify_pqc_signature(file: &Path, sig_path: &Path, pubkey_content: &str) -> Result<(), String> {
    let vk_bytes = parse_hex_body(pubkey_content)?;
    let enc_vk = <&EncodedVerifyingKey<MlDsa65>>::try_from(vk_bytes.as_slice()).map_err(|_| {
        format!(
            "tamaño de clave pública incorrecto ({} bytes)",
            vk_bytes.len()
        )
    })?;
    let vk = VerifyingKey::<MlDsa65>::decode(enc_vk);

    let content =
        fs::read(file).map_err(|e| format!("no se pudo leer {}: {}", file.display(), e))?;

    let sig_content = fs::read_to_string(sig_path)
        .map_err(|e| format!("firma .pqsig no encontrada ({}): {}", sig_path.display(), e))?;
    let sig_bytes = parse_hex_body(&sig_content)?;
    let enc_sig = <&EncodedSignature<MlDsa65>>::try_from(sig_bytes.as_slice())
        .map_err(|_| format!("tamaño de firma incorrecto ({} bytes)", sig_bytes.len()))?;
    let sig = Signature::<MlDsa65>::decode(enc_sig)
        .ok_or_else(|| "firma malformada (decode falló)".to_string())?;

    vk.verify(&content, &sig)
        .map_err(|_| "firma inválida o archivo modificado".to_string())
}

fn sha256_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(data))
}

/// Comprueba la integridad SHA-256 leyendo el primer token de `sha_file`
/// (formato estándar `<hash>  <nombre>`) y comparándolo con el hash real.
fn verify_sha256(file: &Path, sha_file: &Path) -> Result<(), String> {
    let expected_raw = fs::read_to_string(sha_file)
        .map_err(|e| format!("no se pudo leer {}: {}", sha_file.display(), e))?;
    let expected = expected_raw
        .split_whitespace()
        .next()
        .map(|s| s.to_lowercase())
        .ok_or_else(|| "archivo .sha256 vacío".to_string())?;
    let data = fs::read(file).map_err(|e| format!("no se pudo leer {}: {}", file.display(), e))?;
    let actual = sha256_hex(&data);
    if actual == expected {
        Ok(())
    } else {
        Err(format!(
            "hash no coincide (esperado {}, real {})",
            expected, actual
        ))
    }
}

/// Construye la ruta `<file>.<ext>` añadiendo la extensión al nombre completo.
fn sibling_with_suffix(file: &Path, ext: &str) -> PathBuf {
    let mut name = file.as_os_str().to_os_string();
    name.push(".");
    name.push(ext);
    PathBuf::from(name)
}

/// Orquesta la verificación de un release.
///
/// - `file`: ruta al archivo descargado.
/// - `pqsig`: ruta a la firma `.pqsig` (por defecto `<file>.pqsig`).
/// - `pubkey`: ruta a una clave pública ML-DSA alternativa (por defecto, la
///   clave embebida en el binario).
pub fn run(file: &str, pqsig: Option<&str>, pubkey: Option<&str>) -> VerifyReport {
    let file_path = PathBuf::from(file);

    let sig_path = match pqsig {
        Some(p) if !p.trim().is_empty() => PathBuf::from(p),
        _ => sibling_with_suffix(&file_path, SIG_EXTENSION),
    };

    // Resolver clave pública: externa (si se pasa) o embebida.
    let (pubkey_content, embedded) = match pubkey {
        Some(p) if !p.trim().is_empty() => match fs::read_to_string(p) {
            Ok(c) => (c, false),
            Err(e) => {
                return VerifyReport {
                    file: file.to_string(),
                    pqc_ok: false,
                    pqc_detail: format!("no se pudo leer la clave pública {}: {}", p, e),
                    pqc_embedded_key: false,
                    sha256_checked: false,
                    sha256_ok: false,
                    sha256_detail: "no comprobado".to_string(),
                };
            }
        },
        _ => (EMBEDDED_PQC_PUBKEY.to_string(), true),
    };

    let (pqc_ok, pqc_detail) = match verify_pqc_signature(&file_path, &sig_path, &pubkey_content) {
        Ok(()) => (true, "firma válida".to_string()),
        Err(e) => (false, e),
    };

    // SHA-256 opcional (best-effort): solo si existe `<file>.sha256`.
    let sha_path = sibling_with_suffix(&file_path, SHA256_EXTENSION);
    let (sha256_checked, sha256_ok, sha256_detail) = if sha_path.exists() {
        match verify_sha256(&file_path, &sha_path) {
            Ok(()) => (true, true, "hash coincide".to_string()),
            Err(e) => (true, false, e),
        }
    } else {
        (
            false,
            false,
            "sin archivo .sha256 junto al release".to_string(),
        )
    };

    VerifyReport {
        file: file.to_string(),
        pqc_ok,
        pqc_detail,
        pqc_embedded_key: embedded,
        sha256_checked,
        sha256_ok,
        sha256_detail,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ml_dsa::signature::Signer;
    use ml_dsa::{EncodedVerifyingKey, ExpandedSigningKey, MlDsa65, Seed};
    use std::convert::TryFrom;

    /// Genera un keypair de prueba y devuelve (texto pubkey, signing key).
    fn test_keypair() -> (String, ExpandedSigningKey<MlDsa65>) {
        let seed_bytes: [u8; 32] = [7u8; 32];
        // SAFETY: seed_bytes es exactamente 32 bytes, el tamaño requerido para Seed
        let seed_ref: &Seed = <&Seed>::try_from(seed_bytes.as_slice()).unwrap();
        let sk = ExpandedSigningKey::<MlDsa65>::from_seed(seed_ref);
        let vk = sk.verifying_key();
        let encoded_vk: EncodedVerifyingKey<MlDsa65> = vk.encode();
        let vk_hex = hex::encode(encoded_vk.as_slice());
        let pubkey = format!("untrusted comment: test key\n{}\n", vk_hex);
        (pubkey, sk)
    }

    fn write_pqsig(sk: &ExpandedSigningKey<MlDsa65>, content: &[u8], sig_path: &Path) {
        let sig: Signature<MlDsa65> = sk.sign(content);
        let sig_hex = hex::encode(sig.encode().as_slice());
        // SAFETY: test-only, tempfile::tempdir() garantiza directorio válido
        fs::write(
            sig_path,
            format!("untrusted comment: test sig\n{}\n", sig_hex),
        )
        .unwrap();
    }

    #[test]
    fn verifies_valid_signature() {
        // SAFETY: test-only, tempfile::tempdir() garantiza directorio válido
        let dir = tempfile::tempdir().unwrap();
        let (pubkey, sk) = test_keypair();
        let file = dir.path().join("artifact.tar.gz");
        let payload = b"contenido del release legitimo";
        // SAFETY: test-only, directorio temporal garantizado
        fs::write(&file, payload).unwrap();
        let sig_path = sibling_with_suffix(&file, SIG_EXTENSION);
        write_pqsig(&sk, payload, &sig_path);

        let res = verify_pqc_signature(&file, &sig_path, &pubkey);
        assert!(res.is_ok(), "firma válida debe verificar: {:?}", res);
    }

    #[test]
    fn rejects_tampered_file() {
        // SAFETY: test-only, tempfile::tempdir() garantiza directorio válido
        let dir = tempfile::tempdir().unwrap();
        let (pubkey, sk) = test_keypair();
        let file = dir.path().join("artifact.bin");
        // SAFETY: test-only, directorio temporal garantizado
        fs::write(&file, b"original").unwrap();
        let sig_path = sibling_with_suffix(&file, SIG_EXTENSION);
        write_pqsig(&sk, b"original", &sig_path);
        // Modificar el archivo tras firmar.
        // SAFETY: test-only, directorio temporal garantizado
        fs::write(&file, b"modificado por atacante").unwrap();

        assert!(verify_pqc_signature(&file, &sig_path, &pubkey).is_err());
    }

    #[test]
    fn run_reports_success_with_sha256() {
        // SAFETY: test-only, tempfile::tempdir() garantiza directorio válido
        let dir = tempfile::tempdir().unwrap();
        let (pubkey, sk) = test_keypair();
        let file = dir.path().join("rel.tar.gz");
        let payload = b"payload con sha";
        // SAFETY: test-only, directorio temporal garantizado
        fs::write(&file, payload).unwrap();
        let sig_path = sibling_with_suffix(&file, SIG_EXTENSION);
        write_pqsig(&sk, payload, &sig_path);
        // sha256 hermano
        let sha_path = sibling_with_suffix(&file, SHA256_EXTENSION);
        // SAFETY: test-only, directorio temporal garantizado
        fs::write(&sha_path, format!("{}  rel.tar.gz\n", sha256_hex(payload))).unwrap();
        // pubkey externa
        let pub_path = dir.path().join("test.pub");
        // SAFETY: test-only, directorio temporal garantizado
        fs::write(&pub_path, &pubkey).unwrap();

        // SAFETY: test-only, paths son UTF-8 válidos en entorno de prueba
        let report = run(
            file.to_str().unwrap(),
            None,
            Some(pub_path.to_str().unwrap()),
        );
        assert!(report.pqc_ok);
        assert!(report.sha256_checked && report.sha256_ok);
        assert!(report.success());
        assert!(!report.pqc_embedded_key);
    }

    #[test]
    fn embedded_pubkey_is_hex() {
        // La clave embebida debe, como mínimo, parsear como hex.
        let bytes = parse_hex_body(EMBEDDED_PQC_PUBKEY).expect("embedded pubkey parses as hex");
        assert!(!bytes.is_empty(), "embedded pubkey must not be empty");
    }

    #[test]
    fn embedded_pubkey_is_valid_mldsa65() {
        let bytes = parse_hex_body(EMBEDDED_PQC_PUBKEY).expect("embedded pubkey parses");
        assert!(
            <&EncodedVerifyingKey<MlDsa65>>::try_from(bytes.as_slice()).is_ok(),
            "embedded pubkey must be a valid ML-DSA-65 verifying key (got {} bytes)",
            bytes.len()
        );
    }
}
