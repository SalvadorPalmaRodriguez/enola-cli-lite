// Fase 11.1 — Dominio puro para la biblioteca de modelos GGUF (`ai model`).
//
// Regla de capa (`domain/`): SIN dependencias de I/O (`tokio`, `reqwest`,
// `std::fs`). Toda la lógica aquí es determinista y testeable sin tocar disco
// ni red: parsing del header GGUF, validación de nombres/paths y construcción
// de URLs de descarga.

use crate::domain::ai::validate_ai_instance_name;
use crate::domain::error::{EnolaError, Result};
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

/// Magic bytes de un fichero GGUF.
pub const GGUF_MAGIC: [u8; 4] = *b"GGUF";

/// Longitud mínima del header GGUF v2/v3: magic(4) + version(4)
/// + tensor_count(8) + metadata_kv_count(8).
pub const GGUF_MIN_HEADER_LEN: usize = 24;

/// Versiones GGUF soportadas (v1 usa contadores u32 y está deprecada).
pub const SUPPORTED_GGUF_VERSIONS: [u32; 2] = [2, 3];

/// Cota de cordura para contadores del header (anti ficheros basura).
pub const MAX_GGUF_COUNT: u64 = 1_000_000;

/// Subdirectorio de la biblioteca GGUF dentro del volumen Ollama de la
/// instancia (`<base>/<instance>/ollama/models-gguf`).
pub const GGUF_LIBRARY_DIR: &str = "models-gguf";

/// Ruta de la biblioteca DENTRO del contenedor Ollama (volumen `/root/.ollama`).
pub const CONTAINER_MODELS_DIR: &str = "/root/.ollama/models-gguf";

/// Base URL de HuggingFace para resolver `org/repo` + `--file`.
/// Servicio público de terceros (no dominio del proyecto).
pub const HUGGINGFACE_BASE_URL: &str = "https://huggingface.co";

/// Longitud máxima de un nombre de fichero de modelo.
pub const MAX_MODEL_FILENAME_LEN: usize = 255;

/// Longitud máxima de un nombre de modelo en el registro Ollama.
pub const MAX_MODEL_NAME_LEN: usize = 256;

/// Metadatos del header de un fichero GGUF.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GgufMetadata {
    /// Versión del formato GGUF (2 o 3).
    pub version: u32,
    /// Número de tensores declarados.
    pub tensor_count: u64,
    /// Número de pares clave-valor de metadatos.
    pub metadata_kv_count: u64,
}

/// Origen de un modelo en la biblioteca.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelSource {
    /// Descargado desde HuggingFace (o URL directa).
    Downloaded,
    /// Importado desde una ruta local del host.
    Imported,
}

/// Estado de un fichero de la biblioteca.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AiModelStatus {
    /// Header GGUF válido.
    Ready,
    /// Header inválido o ilegible.
    Corrupt,
}

/// Modelo de la biblioteca GGUF de una instancia.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiModel {
    /// Nombre sugerido para el registro Ollama (alias o filename sin `.gguf`).
    pub name: String,
    /// Nombre del fichero `.gguf` dentro de la biblioteca.
    pub filename: String,
    /// Tamaño en bytes.
    pub size_bytes: u64,
    /// Cuantización inferida del nombre (`Q4_K_M`, `Q8_0`, `F16`, `Unknown`).
    pub quantization: String,
    /// Familia inferida del nombre (`Llama 3`, `Phi 3`, `Mistral`, `Gemma`, `Unknown`).
    pub family: String,
    /// Estado del fichero.
    pub status: AiModelStatus,
    /// Origen del fichero.
    pub source: ModelSource,
    /// SHA-256 registrado en la descarga/importación (si se verificó).
    pub sha256: Option<String>,
}

/// Parsea el header GGUF de `bytes` (magic + version + contadores v2/v3).
pub fn parse_gguf_header(bytes: &[u8]) -> Result<GgufMetadata> {
    if bytes.len() < GGUF_MIN_HEADER_LEN {
        return Err(EnolaError::ValidationError(format!(
            "Fichero demasiado corto para un header GGUF ({} bytes < {GGUF_MIN_HEADER_LEN})",
            bytes.len()
        )));
    }
    if bytes[..4] != GGUF_MAGIC {
        return Err(EnolaError::ValidationError(
            "Magic inválido: el fichero no empieza por 'GGUF'".into(),
        ));
    }
    let version = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    if !SUPPORTED_GGUF_VERSIONS.contains(&version) {
        return Err(EnolaError::ValidationError(format!(
            "Versión GGUF {version} no soportada (soportadas: {:?})",
            SUPPORTED_GGUF_VERSIONS
        )));
    }
    let tensor_count =
        u64::from_le_bytes(bytes[8..16].try_into().map_err(|_| {
            EnolaError::ValidationError("Header GGUF truncado (tensor_count)".into())
        })?);
    let metadata_kv_count = u64::from_le_bytes(bytes[16..24].try_into().map_err(|_| {
        EnolaError::ValidationError("Header GGUF truncado (metadata_kv_count)".into())
    })?);
    if tensor_count > MAX_GGUF_COUNT || metadata_kv_count > MAX_GGUF_COUNT {
        return Err(EnolaError::ValidationError(format!(
            "Contadores del header GGUF fuera de rango (tensors={tensor_count}, kv={metadata_kv_count})"
        )));
    }
    Ok(GgufMetadata {
        version,
        tensor_count,
        metadata_kv_count,
    })
}

/// `true` si el nombre es un único componente de fichero seguro (sin `/`,
/// `\`, NUL, `..` ni caracteres de control).
fn is_single_component(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_MODEL_FILENAME_LEN
        && !name.contains('/')
        && !name.contains('\\')
        && !name.contains('\0')
        && !name.contains("..")
        && !name.chars().any(char::is_control)
        && Path::new(name)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
        && Path::new(name).components().count() == 1
}

/// Valida un nombre de fichero `.gguf` para la biblioteca (anti path traversal).
pub fn validate_gguf_filename(name: &str) -> Result<()> {
    if !is_single_component(name) {
        return Err(EnolaError::ValidationError(format!(
            "Nombre de fichero de modelo inválido: '{name}' (un solo componente, sin separadores ni '..')"
        )));
    }
    if !name.to_ascii_lowercase().ends_with(".gguf") {
        return Err(EnolaError::ValidationError(format!(
            "El fichero de modelo '{name}' debe tener extensión .gguf"
        )));
    }
    Ok(())
}

/// Valida un nombre de modelo del registro Ollama (`llama3.1:8b`,
/// `hf.co/org/repo:Q4_K_M`, `mi-modelo`).
pub fn validate_ollama_model_name(name: &str) -> Result<()> {
    let ok = !name.is_empty()
        && name.len() <= MAX_MODEL_NAME_LEN
        && !name.contains("..")
        && !name
            .chars()
            .any(|c| !(c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/' | ':')))
        && !name.starts_with(['-', '/', ':', '.'])
        && !name.ends_with(['/', ':', '.'])
        && !name.contains("//")
        && !name.contains("::");
    if !ok {
        return Err(EnolaError::ValidationError(format!(
            "Nombre de modelo Ollama inválido: '{name}' (alfanumérico + '._-/:', sin '..' ni segmentos vacíos)"
        )));
    }
    Ok(())
}

/// Ruta host de la biblioteca GGUF de una instancia:
/// `<base>/<instance>/ollama/models-gguf/`.
pub fn library_dir_for_instance(base: &Path, instance: &str) -> Result<PathBuf> {
    validate_ai_instance_name(instance)?;
    Ok(base.join(instance).join("ollama").join(GGUF_LIBRARY_DIR))
}

/// Resuelve la ruta host de un fichero `.gguf` dentro de la biblioteca de la
/// instancia, rechazando cualquier escape (path traversal).
pub fn resolve_library_path(base: &Path, instance: &str, filename: &str) -> Result<PathBuf> {
    validate_gguf_filename(filename)?;
    let dir = library_dir_for_instance(base, instance)?;
    let joined = dir.join(filename);
    if !joined.starts_with(&dir) {
        return Err(EnolaError::SecurityError(format!(
            "La ruta '{filename}' escapa del directorio de modelos"
        )));
    }
    Ok(joined)
}

/// Ruta del fichero `.gguf` DENTRO del contenedor Ollama
/// (`/root/.ollama/models-gguf/<filename>`), usable en `FROM` de Modelfile.
pub fn container_gguf_path(filename: &str) -> Result<String> {
    validate_gguf_filename(filename)?;
    Ok(format!("{CONTAINER_MODELS_DIR}/{filename}"))
}

/// Nombre de registro por defecto: filename sin `.gguf`, minúsculas, con los
/// caracteres no admitidos sustituidos por `-`.
pub fn default_registry_name(filename: &str) -> Result<String> {
    validate_gguf_filename(filename)?;
    let stem = &filename[..filename.len() - ".gguf".len()];
    let mut out = String::with_capacity(stem.len());
    for c in stem.chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
            out.push(c.to_ascii_lowercase());
        } else {
            out.push('-');
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() {
        return Err(EnolaError::ValidationError(format!(
            "No se puede derivar un nombre de modelo desde '{filename}'"
        )));
    }
    validate_ollama_model_name(&out)?;
    Ok(out)
}

/// Heurísticas de familia y cuantización a partir del nombre del fichero.
/// Devuelve `(family, quantization)`.
pub fn parse_filename_heuristics(filename: &str) -> (String, String) {
    let lower = filename.to_ascii_lowercase();
    let family = if lower.contains("llama-3") || lower.contains("llama3") {
        "Llama 3"
    } else if lower.contains("phi-3") || lower.contains("phi3") {
        "Phi 3"
    } else if lower.contains("mistral") {
        "Mistral"
    } else if lower.contains("gemma") {
        "Gemma"
    } else if lower.contains("qwen") {
        "Qwen"
    } else {
        "Unknown"
    };
    let quant = if lower.contains("q4_k_m") {
        "Q4_K_M"
    } else if lower.contains("q4_0") {
        "Q4_0"
    } else if lower.contains("q5_k_m") {
        "Q5_K_M"
    } else if lower.contains("q6_k") {
        "Q6_K"
    } else if lower.contains("q8_0") {
        "Q8_0"
    } else if lower.contains("f16") {
        "F16"
    } else if lower.contains("f32") {
        "F32"
    } else {
        "Unknown"
    };
    (family.to_string(), quant.to_string())
}

/// Resultado de resolver la fuente de una descarga: `(url, filename)`.
pub struct ResolvedDownload {
    /// URL final de descarga.
    pub url: String,
    /// Nombre de fichero `.gguf` validado para la biblioteca.
    pub filename: String,
}

/// Resuelve `--source` de `ai model download`:
/// - `http(s)://…/file.gguf` → URL directa (filename = último segmento).
/// - `org/repo` → HuggingFace `resolve/main` (exige `--file`).
pub fn resolve_download_source(source: &str, file: Option<&str>) -> Result<ResolvedDownload> {
    let src = source.trim();
    if src.is_empty() {
        return Err(EnolaError::ValidationError(
            "La fuente de descarga no puede estar vacía".into(),
        ));
    }
    if src.starts_with("http://") || src.starts_with("https://") {
        let path_part = src.split(['?', '#']).next().unwrap_or(src);
        let filename = path_part.rsplit('/').next().unwrap_or("").to_string();
        validate_gguf_filename(&filename).map_err(|_| {
            EnolaError::ValidationError(format!(
                "La URL debe terminar en un fichero .gguf válido: '{src}'"
            ))
        })?;
        return Ok(ResolvedDownload {
            url: src.to_string(),
            filename,
        });
    }
    // Modo repo HuggingFace: `org/repo` exactamente un separador.
    let parts: Vec<&str> = src.split('/').collect();
    let valid_repo = parts.len() == 2
        && parts.iter().all(|p| {
            !p.is_empty()
                && p.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        });
    if !valid_repo {
        return Err(EnolaError::ValidationError(format!(
            "Fuente '{src}' inválida: usa una URL http(s) a un .gguf o un repo 'org/repo' de HuggingFace"
        )));
    }
    let file = file
        .map(str::trim)
        .filter(|f| !f.is_empty())
        .ok_or_else(|| {
            EnolaError::ValidationError(
                "Con un repo 'org/repo' de HuggingFace es obligatorio indicar --file <fichero.gguf>"
                    .into(),
            )
        })?;
    validate_gguf_filename(file)?;
    Ok(ResolvedDownload {
        url: format!("{HUGGINGFACE_BASE_URL}/{src}/resolve/main/{file}"),
        filename: file.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gguf_bytes(version: u32, tensors: u64, kv: u64) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(b"GGUF");
        b.extend_from_slice(&version.to_le_bytes());
        b.extend_from_slice(&tensors.to_le_bytes());
        b.extend_from_slice(&kv.to_le_bytes());
        b.extend_from_slice(&[0u8; 8]);
        b
    }

    // ── parse_gguf_header ──

    #[test]
    fn gguf_header_parses_valid_v3() {
        let meta = parse_gguf_header(&gguf_bytes(3, 292, 41)).unwrap();
        assert_eq!(meta.version, 3);
        assert_eq!(meta.tensor_count, 292);
        assert_eq!(meta.metadata_kv_count, 41);
    }

    #[test]
    fn gguf_header_parses_valid_v2() {
        let meta = parse_gguf_header(&gguf_bytes(2, 10, 5)).unwrap();
        assert_eq!(meta.version, 2);
    }

    #[test]
    fn gguf_header_rejects_bad_magic() {
        let mut b = gguf_bytes(3, 1, 0);
        b[0] = b'X';
        assert!(parse_gguf_header(&b).is_err());
    }

    #[test]
    fn gguf_header_rejects_truncated() {
        assert!(parse_gguf_header(b"GGUF").is_err());
        assert!(parse_gguf_header(&gguf_bytes(3, 1, 0)[..16]).is_err());
        assert!(parse_gguf_header(&[]).is_err());
    }

    #[test]
    fn gguf_header_rejects_unsupported_version() {
        assert!(parse_gguf_header(&gguf_bytes(1, 1, 0)).is_err());
        assert!(parse_gguf_header(&gguf_bytes(99, 1, 0)).is_err());
    }

    #[test]
    fn gguf_header_rejects_absurd_counts() {
        assert!(parse_gguf_header(&gguf_bytes(3, u64::MAX, 0)).is_err());
        assert!(parse_gguf_header(&gguf_bytes(3, 0, u64::MAX)).is_err());
    }

    // ── validate_gguf_filename (anti path traversal) ──

    #[test]
    fn gguf_filename_accepts_simple_names() {
        assert!(validate_gguf_filename("model.gguf").is_ok());
        assert!(validate_gguf_filename("Llama-3.2-1B-Q4_K_M.gguf").is_ok());
        assert!(validate_gguf_filename("A.GGUF").is_ok());
    }

    #[test]
    fn gguf_filename_rejects_traversal_and_separators() {
        assert!(validate_gguf_filename("../x.gguf").is_err());
        assert!(validate_gguf_filename("a/../b.gguf").is_err());
        assert!(validate_gguf_filename("dir/x.gguf").is_err());
        assert!(validate_gguf_filename("dir\\x.gguf").is_err());
        assert!(validate_gguf_filename("/abs/x.gguf").is_err());
        assert!(validate_gguf_filename("x.gguf\0evil").is_err());
        assert!(validate_gguf_filename("").is_err());
        assert!(validate_gguf_filename(".").is_err());
        assert!(validate_gguf_filename("..").is_err());
    }

    #[test]
    fn gguf_filename_requires_gguf_extension() {
        assert!(validate_gguf_filename("model.bin").is_err());
        assert!(validate_gguf_filename("modelgguf").is_err());
        assert!(validate_gguf_filename("model.gguf.bak").is_err());
    }

    // ── validate_ollama_model_name ──

    #[test]
    fn ollama_model_name_accepts_registry_forms() {
        assert!(validate_ollama_model_name("llama3.1:8b").is_ok());
        assert!(validate_ollama_model_name("hf.co/org/repo:Q4_K_M").is_ok());
        assert!(validate_ollama_model_name("mi-modelo_v2").is_ok());
    }

    #[test]
    fn ollama_model_name_rejects_dangerous_forms() {
        assert!(validate_ollama_model_name("").is_err());
        assert!(validate_ollama_model_name("a b").is_err());
        assert!(validate_ollama_model_name("../x").is_err());
        assert!(validate_ollama_model_name("a//b").is_err());
        assert!(validate_ollama_model_name("a::b").is_err());
        assert!(validate_ollama_model_name("-lead").is_err());
        assert!(validate_ollama_model_name("x;rm -rf").is_err());
        assert!(validate_ollama_model_name("a\nb").is_err());
    }

    // ── resolución de rutas ──

    #[test]
    fn library_path_stays_inside_instance_dir() {
        let base = Path::new("/srv/enola-ai");
        let p = resolve_library_path(base, "dev", "m.gguf").unwrap();
        assert_eq!(
            p,
            PathBuf::from("/srv/enola-ai/dev/ollama/models-gguf/m.gguf")
        );
        assert!(resolve_library_path(base, "dev", "../escape.gguf").is_err());
        assert!(resolve_library_path(base, "dev", "sub/dir.gguf").is_err());
        assert!(resolve_library_path(base, "Bad_Name", "m.gguf").is_err());
    }

    #[test]
    fn container_path_maps_inside_ollama_volume() {
        let p = container_gguf_path("m.gguf").unwrap();
        assert_eq!(p, "/root/.ollama/models-gguf/m.gguf");
        assert!(container_gguf_path("../m.gguf").is_err());
    }

    #[test]
    fn default_registry_name_derives_safe_lowercase() {
        assert_eq!(
            default_registry_name("Llama-3.2-1B-Instruct-Q4_K_M.gguf").unwrap(),
            "llama-3.2-1b-instruct-q4_k_m"
        );
        assert!(default_registry_name("../x.gguf").is_err());
    }

    // ── heurísticas de filename ──

    #[test]
    fn filename_heuristics_detect_family_and_quant() {
        assert_eq!(
            parse_filename_heuristics("llama-3-8b-instruct-q4_k_m.gguf"),
            ("Llama 3".to_string(), "Q4_K_M".to_string())
        );
        assert_eq!(parse_filename_heuristics("mistral-7b-f16.gguf").1, "F16");
        assert_eq!(
            parse_filename_heuristics("random.gguf"),
            ("Unknown".to_string(), "Unknown".to_string())
        );
    }

    // ── resolve_download_source ──

    #[test]
    fn download_source_parses_direct_url() {
        let r = resolve_download_source(
            "https://huggingface.co/org/repo/resolve/main/model-Q4_K_M.gguf?download=true",
            None,
        )
        .unwrap();
        assert_eq!(r.filename, "model-Q4_K_M.gguf");
        assert!(r.url.starts_with("https://huggingface.co/"));
    }

    #[test]
    fn download_source_parses_hf_repo_with_file() {
        let r = resolve_download_source("bartowski/Llama-3.2-1B-Instruct-GGUF", Some("m.gguf"))
            .unwrap();
        assert_eq!(
            r.url,
            "https://huggingface.co/bartowski/Llama-3.2-1B-Instruct-GGUF/resolve/main/m.gguf"
        );
        assert_eq!(r.filename, "m.gguf");
    }

    #[test]
    fn download_source_rejects_repo_without_file() {
        assert!(resolve_download_source("org/repo", None).is_err());
    }

    #[test]
    fn download_source_rejects_invalid_forms() {
        assert!(resolve_download_source("", None).is_err());
        assert!(resolve_download_source("ftp://x/m.gguf", None).is_err());
        assert!(resolve_download_source("org/repo", Some("../evil.gguf")).is_err());
        assert!(resolve_download_source("org/repo", Some("m.bin")).is_err());
        assert!(resolve_download_source("a/b/c", Some("m.gguf")).is_err());
        assert!(resolve_download_source("https://x/path/", None).is_err());
        assert!(resolve_download_source("https://x/m.bin", None).is_err());
    }
}
