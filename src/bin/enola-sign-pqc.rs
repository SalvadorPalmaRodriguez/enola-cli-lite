// enola-sign-pqc — Herramienta de firma post-cuántica (PQC-030)
//
// Implementa la segunda capa de firma digital: ML-DSA-65 (FIPS 204).
// La primera capa (minisign/Ed25519) ya existe en release.sh.
// Juntas forman la firma dual requerida por PQC-030.
//
// Uso:
//   enola-sign-pqc keygen               # Genera keypair ML-DSA-65
//   enola-sign-pqc pubkey               # Re-exporta la clave pública desde la privada existente
//   enola-sign-pqc sign <file>          # Firma archivo → <file>.pqsig
//   enola-sign-pqc verify <file> <pub>  # Verifica <file>.pqsig contra clave pública
//
// Almacenamiento de claves:
//   Semilla privada: ~/.enola/pqc_signing.key   (base64, permisos 0600, 32 bytes)
//   Clave pública:   pqc_sign.pub               (hex, commitear en repo)

use base64::Engine as _;
use ml_dsa::signature::{Signer, Verifier};
use ml_dsa::{
    EncodedSignature, EncodedVerifyingKey, ExpandedSigningKey, MlDsa65, Seed, Signature,
    VerifyingKey,
};
use std::convert::TryFrom;
use std::fs;
use std::io::{self, Read};
use std::path::PathBuf;
use std::process;

const PRIVATE_KEY_FILE: &str = ".enola/pqc_signing.key";
const SIG_EXTENSION: &str = ".pqsig";
const VK_HEADER: &str = "untrusted comment: ML-DSA-65 public key (PQC-030, FIPS 204)";
const SIG_HEADER: &str = "untrusted comment: ML-DSA-65 signature (PQC-030, FIPS 204)";

fn private_key_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    PathBuf::from(home).join(PRIVATE_KEY_FILE)
}

fn b64_enc(data: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(data)
}

fn b64_dec(s: &str) -> Result<Vec<u8>, String> {
    base64::engine::general_purpose::STANDARD
        .decode(s.trim())
        .map_err(|e| e.to_string())
}

/// Helper: convierte hybrid_array::Array a Vec<u8> de forma no ambigua
fn array_to_vec<U: hybrid_array::ArraySize>(arr: &hybrid_array::Array<u8, U>) -> Vec<u8> {
    let slice: &[u8] = arr.as_slice();
    slice.to_vec()
}

fn usage() -> ! {
    eprintln!(
        "Enola PQC Signing Tool — ML-DSA-65 (FIPS 204)\n\
         \n\
         Uso:\n\
           enola-sign-pqc keygen              Genera keypair ML-DSA-65\n\
           enola-sign-pqc pubkey              Re-exporta la clave pública (1 línea) desde ~/.enola/pqc_signing.key\n\
           enola-sign-pqc sign <file>         Firma archivo (→ <file>.pqsig)\n\
           enola-sign-pqc verify <file> <pub> Verifica con clave pública\n\
         \n\
         Archivos:\n\
           Semilla privada: ~/.enola/pqc_signing.key  (0600, 32 bytes en base64)\n\
           Clave pública:   pqc_sign.pub               (commitear en repo)\n\
           Firma:           <archivo>.pqsig"
    );
    process::exit(1);
}

/// Genera un nuevo keypair ML-DSA-65 y lo guarda en disco.
fn cmd_keygen() {
    // Generar semilla aleatoria (32 bytes criptográficamente seguros via rand/OsRng)
    let seed_bytes: [u8; 32] = rand::random();
    let seed_ref: &Seed = match <&Seed>::try_from(seed_bytes.as_slice()) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("ERROR: invalid seed length: {}", e);
            process::exit(1);
        }
    };
    let sk = ExpandedSigningKey::<MlDsa65>::from_seed(seed_ref);
    let vk = sk.verifying_key();
    let encoded_vk: EncodedVerifyingKey<MlDsa65> = vk.encode();

    // Guardar semilla privada
    let sk_path = private_key_path();
    if let Some(parent) = sk_path.parent() {
        fs::create_dir_all(parent).unwrap_or_else(|e| {
            eprintln!("Error creando directorio: {}", e);
            process::exit(1);
        });
    }
    let seed_b64 = b64_enc(&seed_bytes);
    fs::write(&sk_path, &seed_b64).unwrap_or_else(|e| {
        eprintln!("Error escribiendo clave privada: {}", e);
        process::exit(1);
    });
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&sk_path, fs::Permissions::from_mode(0o600));
    }

    // Clave pública en hex
    let vk_hex = hex::encode(array_to_vec(&encoded_vk));

    println!("✅ Keypair ML-DSA-65 generado");
    println!("   Semilla privada: {}", sk_path.display());
    println!();
    println!("=== CLAVE PÚBLICA — copiar a pqc_sign.pub y commitear ===");
    println!("{}", VK_HEADER);
    println!("{}", vk_hex);
    println!("=========================================================");
    println!();
    println!("⚠️  NUNCA compartas ni commitees ~/.enola/pqc_signing.key");
}

/// Re-exporta la clave pública (formato pqc_sign.pub, hex en UNA línea) a partir
/// de la clave privada existente. Útil para regenerar `pqc_sign.pub` sin crear
/// un keypair nuevo (no invalida firmas previas). Redirige a un archivo:
///   enola-sign-pqc pubkey > pqc_sign.pub
fn cmd_pubkey() {
    let sk = load_signing_key();
    let vk = sk.verifying_key();
    let encoded_vk: EncodedVerifyingKey<MlDsa65> = vk.encode();
    let vk_hex = hex::encode(array_to_vec(&encoded_vk));
    println!("{}", VK_HEADER);
    println!("{}", vk_hex);
}

/// Carga la clave de firma expandida desde la semilla guardada.
fn load_signing_key() -> ExpandedSigningKey<MlDsa65> {
    let sk_path = private_key_path();
    let seed_b64 = fs::read_to_string(&sk_path).unwrap_or_else(|_| {
        eprintln!("❌ Clave privada no encontrada: {}", sk_path.display());
        eprintln!("   Genera un keypair primero: enola-sign-pqc keygen");
        process::exit(1);
    });
    let seed_bytes = b64_dec(&seed_b64).unwrap_or_else(|e| {
        eprintln!("❌ Error decodificando semilla: {}", e);
        process::exit(1);
    });
    let seed_ref: &Seed = <&Seed>::try_from(seed_bytes.as_slice()).unwrap_or_else(|_| {
        eprintln!(
            "❌ Semilla inválida: se esperan 32 bytes, hay {}",
            seed_bytes.len()
        );
        process::exit(1);
    });
    ExpandedSigningKey::<MlDsa65>::from_seed(seed_ref)
}

/// Firma un archivo con ML-DSA-65.
fn cmd_sign(file: &str) {
    let sk = load_signing_key();

    let mut content = Vec::new();
    if file == "-" {
        io::stdin().read_to_end(&mut content).unwrap_or_else(|e| {
            eprintln!("❌ Error leyendo stdin: {}", e);
            process::exit(1);
        });
    } else {
        fs::File::open(file)
            .and_then(|mut f| f.read_to_end(&mut content))
            .unwrap_or_else(|e| {
                eprintln!("❌ Error leyendo {}: {}", file, e);
                process::exit(1);
            });
    }

    let sig: Signature<MlDsa65> = sk.sign(&content);
    let encoded_sig: EncodedSignature<MlDsa65> = sig.encode();
    let sig_hex = hex::encode(array_to_vec(&encoded_sig));

    let sig_path = format!("{}{}", file, SIG_EXTENSION);
    let sig_content = format!("{}\n{}\n", SIG_HEADER, sig_hex);
    fs::write(&sig_path, &sig_content).unwrap_or_else(|e| {
        eprintln!("❌ Error escribiendo firma: {}", e);
        process::exit(1);
    });
    println!("✅ Firma ML-DSA-65 generada: {}", sig_path);
}

/// Verifica la firma .pqsig de un archivo con la clave pública proporcionada.
fn cmd_verify(file: &str, pubkey_file: &str) {
    // Leer y parsear clave pública (hex)
    let vk_content = fs::read_to_string(pubkey_file).unwrap_or_else(|e| {
        eprintln!("❌ Error leyendo clave pública {}: {}", pubkey_file, e);
        process::exit(1);
    });
    let vk_hex = vk_content
        .lines()
        .find(|l| !l.starts_with("untrusted comment") && !l.trim().is_empty())
        .unwrap_or_else(|| {
            eprintln!("❌ Formato de clave pública inválido (se esperan 2 líneas)");
            process::exit(1);
        });
    let vk_bytes = hex::decode(vk_hex.trim()).unwrap_or_else(|e| {
        eprintln!("❌ Error decodificando clave pública hex: {}", e);
        process::exit(1);
    });
    let enc_vk: &EncodedVerifyingKey<MlDsa65> =
        <&EncodedVerifyingKey<MlDsa65>>::try_from(vk_bytes.as_slice()).unwrap_or_else(|_| {
            eprintln!(
                "❌ Tamaño de clave pública incorrecto ({} bytes)",
                vk_bytes.len()
            );
            process::exit(1);
        });
    let vk = VerifyingKey::<MlDsa65>::decode(enc_vk);

    // Leer archivo original
    let mut content = Vec::new();
    fs::File::open(file)
        .and_then(|mut f| f.read_to_end(&mut content))
        .unwrap_or_else(|e| {
            eprintln!("❌ Error leyendo {}: {}", file, e);
            process::exit(1);
        });

    // Leer y parsear firma
    let sig_path = format!("{}{}", file, SIG_EXTENSION);
    let sig_content = fs::read_to_string(&sig_path).unwrap_or_else(|e| {
        eprintln!("❌ Firma no encontrada {}: {}", sig_path, e);
        process::exit(1);
    });
    let sig_hex = sig_content
        .lines()
        .find(|l| !l.starts_with("untrusted comment") && !l.trim().is_empty())
        .unwrap_or_else(|| {
            eprintln!("❌ Formato de firma inválido");
            process::exit(1);
        });
    let sig_bytes = hex::decode(sig_hex.trim()).unwrap_or_else(|e| {
        eprintln!("❌ Error decodificando firma hex: {}", e);
        process::exit(1);
    });
    let enc_sig: &EncodedSignature<MlDsa65> =
        <&EncodedSignature<MlDsa65>>::try_from(sig_bytes.as_slice()).unwrap_or_else(|_| {
            eprintln!("❌ Tamaño de firma incorrecto ({} bytes)", sig_bytes.len());
            process::exit(1);
        });
    let sig = Signature::<MlDsa65>::decode(enc_sig).unwrap_or_else(|| {
        eprintln!("❌ Firma malformada (decode falló)");
        process::exit(1);
    });

    // Verificar
    match vk.verify(&content, &sig) {
        Ok(()) => println!(
            "✅ Firma ML-DSA-65 verificada correctamente\n   Archivo: {}",
            file
        ),
        Err(_) => {
            eprintln!("❌ Verificación FALLIDA — firma inválida o archivo modificado");
            process::exit(2);
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(|s| s.as_str()) {
        Some("keygen") => cmd_keygen(),
        Some("pubkey") => cmd_pubkey(),
        Some("sign") => {
            let file = args.get(2).map(|s| s.as_str()).unwrap_or_else(|| {
                eprintln!("Uso: enola-sign-pqc sign <archivo>");
                process::exit(1);
            });
            cmd_sign(file);
        }
        Some("verify") => {
            let file = args.get(2).map(|s| s.as_str()).unwrap_or_else(|| {
                eprintln!("Uso: enola-sign-pqc verify <archivo> <clave-publica>");
                process::exit(1);
            });
            let pubkey = args.get(3).map(|s| s.as_str()).unwrap_or_else(|| {
                eprintln!("Uso: enola-sign-pqc verify <archivo> <clave-publica>");
                process::exit(1);
            });
            cmd_verify(file, pubkey);
        }
        _ => usage(),
    }
}
