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

/// Bytes de cabecera que se leen para validar un GGUF y extraer su metadata
/// (`general.architecture`, `general.file_type`). La metadata incluye el
/// vocabulario del tokenizer: Qwen2.5 ocupa 5.66 MiB (151k tokens + merges)
/// y los modelos con vocabularios de 256k tokens rondan los 10 MiB. 32 MiB
/// cubre esos casos y sigue siendo una cota defensiva contra ficheros
/// malformados. Normalmente basta mucho menos: `parse_gguf_header` para en
/// cuanto tiene las dos claves, que los conversores escriben al principio.
pub const GGUF_HEADER_READ_LEN: usize = 32 * 1024 * 1024;

/// Tipos de valor del bloque de metadatos GGUF (gguf_value_type).
const GGUF_T_UINT8: u32 = 0;
const GGUF_T_INT8: u32 = 1;
const GGUF_T_UINT16: u32 = 2;
const GGUF_T_INT16: u32 = 3;
const GGUF_T_UINT32: u32 = 4;
const GGUF_T_INT32: u32 = 5;
const GGUF_T_FLOAT32: u32 = 6;
const GGUF_T_BOOL: u32 = 7;
const GGUF_T_STRING: u32 = 8;
const GGUF_T_ARRAY: u32 = 9;
const GGUF_T_UINT64: u32 = 10;
const GGUF_T_INT64: u32 = 11;
const GGUF_T_FLOAT64: u32 = 12;

/// Versiones GGUF soportadas (v1 usa contadores u32 y está deprecada).
pub const SUPPORTED_GGUF_VERSIONS: [u32; 2] = [2, 3];

/// Cota de cordura para contadores del header (anti ficheros basura).
pub const MAX_GGUF_COUNT: u64 = 1_000_000;

/// Subdirectorio de la biblioteca GGUF dentro del volumen Ollama de la
/// instancia (`<base>/<instance>/ollama/models-gguf`).
pub const GGUF_LIBRARY_DIR: &str = "models-gguf";

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
    /// Arquitectura del modelo (`general.architecture` del header), p.ej.
    /// `"llama"`, `"mistral"`, `"qwen2"`. `None` si el header no la expone.
    #[serde(default)]
    pub architecture: Option<String>,
    /// Tipo de fichero GGUF (`general.file_type` del header, u32 de GGUF),
    /// p.ej. `1` (F16), `2` (Q4_0), `12` (Q4_K). `None` si no está presente.
    #[serde(default)]
    pub file_type: Option<u32>,
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
    /// Repositorio HuggingFace (`org/repo`) de origen, cuando se conoce.
    /// Se usa para recuperar el tokenizer HF del fine-tuning offline.
    #[serde(default)]
    pub hf_repo: Option<String>,
}

/// Valida el prefijo fijo del header GGUF (magic + version + contadores) y
/// devuelve `(version, tensor_count, metadata_kv_count)`.
///
/// Esta es la comprobación estructural mínima: suficiente para decidir si un
/// fichero *es* un GGUF válido sin necesidad de parsear su metadata completa.
fn header_prefix(bytes: &[u8]) -> Result<(u32, u64, u64)> {
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
    let tensor_count = u64::from_le_bytes([
        bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
    ]);
    let metadata_kv_count = u64::from_le_bytes([
        bytes[16], bytes[17], bytes[18], bytes[19], bytes[20], bytes[21], bytes[22], bytes[23],
    ]);
    if tensor_count > MAX_GGUF_COUNT || metadata_kv_count > MAX_GGUF_COUNT {
        return Err(EnolaError::ValidationError(format!(
            "Contadores del header GGUF fuera de rango (tensors={tensor_count}, kv={metadata_kv_count})"
        )));
    }
    Ok((version, tensor_count, metadata_kv_count))
}

/// Valida únicamente el prefijo fijo del header GGUF (magic + version +
/// contadores). No parsea la metadata; pensado para los chequeos baratos de
/// "¿es esto un GGUF?" previos a copiar/registrar.
pub fn validate_gguf_header(bytes: &[u8]) -> Result<()> {
    header_prefix(bytes).map(|_| ())
}

fn truncated_header(what: &str) -> EnolaError {
    EnolaError::ValidationError(format!("Header GGUF truncado ({what})"))
}

/// Lee un `u32` little-endian en `pos`.
fn read_u32_at(bytes: &[u8], pos: usize) -> Result<u32> {
    let end = pos.checked_add(4).ok_or_else(|| truncated_header("u32"))?;
    let s = bytes.get(pos..end).ok_or_else(|| truncated_header("u32"))?;
    Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

/// Lee un `u64` little-endian en `pos`.
fn read_u64_at(bytes: &[u8], pos: usize) -> Result<u64> {
    let end = pos.checked_add(8).ok_or_else(|| truncated_header("u64"))?;
    let s = bytes.get(pos..end).ok_or_else(|| truncated_header("u64"))?;
    Ok(u64::from_le_bytes([
        s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7],
    ]))
}

/// Lee una `gguf_string` (u64 longitud + bytes UTF-8) en `pos`, devolviendo
/// `(cadena, siguiente_posición)`.
fn read_gguf_string(bytes: &[u8], pos: usize) -> Result<(String, usize)> {
    let len = read_u64_at(bytes, pos)? as usize;
    let start = pos
        .checked_add(8)
        .ok_or_else(|| truncated_header("string"))?;
    let end = start
        .checked_add(len)
        .ok_or_else(|| truncated_header("string"))?;
    let s = bytes
        .get(start..end)
        .ok_or_else(|| truncated_header("string"))?;
    Ok((String::from_utf8_lossy(s).into_owned(), end))
}

/// Avanza sobre el valor GGUF de tipo `value_type` en `pos`, devolviendo la
/// posición siguiente. Soporta escalares, strings y arrays (anidados a un
/// nivel, como prescribe el formato).
fn skip_gguf_value(bytes: &[u8], pos: usize, value_type: u32) -> Result<usize> {
    let scalar_size = match value_type {
        GGUF_T_UINT8 | GGUF_T_INT8 | GGUF_T_BOOL => Some(1),
        GGUF_T_UINT16 | GGUF_T_INT16 => Some(2),
        GGUF_T_UINT32 | GGUF_T_INT32 | GGUF_T_FLOAT32 => Some(4),
        GGUF_T_UINT64 | GGUF_T_INT64 | GGUF_T_FLOAT64 => Some(8),
        _ => None,
    };
    if let Some(size) = scalar_size {
        let end = pos
            .checked_add(size)
            .ok_or_else(|| truncated_header("valor"))?;
        if end > bytes.len() {
            return Err(truncated_header("valor"));
        }
        return Ok(end);
    }
    match value_type {
        GGUF_T_STRING => {
            let (_s, end) = read_gguf_string(bytes, pos)?;
            Ok(end)
        }
        GGUF_T_ARRAY => {
            let elem_type = read_u32_at(bytes, pos)?;
            let count = read_u64_at(bytes, pos + 4)? as usize;
            let mut p = pos + 12;
            for _ in 0..count {
                p = skip_gguf_value(bytes, p, elem_type)?;
            }
            Ok(p)
        }
        other => Err(EnolaError::ValidationError(format!(
            "Tipo de valor GGUF desconocido ({other}) en el header"
        ))),
    }
}

/// Parsea el header GGUF de `bytes`: prefijo fijo (magic + version +
/// contadores) y bloque de metadatos KV, extrayendo `general.architecture` y
/// `general.file_type` del header real (no del nombre del fichero).
pub fn parse_gguf_header(bytes: &[u8]) -> Result<GgufMetadata> {
    let (version, tensor_count, metadata_kv_count) = header_prefix(bytes)?;
    let mut pos = GGUF_MIN_HEADER_LEN;
    let mut architecture = None;
    let mut file_type = None;
    for _ in 0..metadata_kv_count {
        let (key, next) = read_gguf_string(bytes, pos)?;
        pos = next;
        let value_type = read_u32_at(bytes, pos)?;
        pos += 4;
        match key.as_str() {
            "general.architecture" if value_type == GGUF_T_STRING => {
                let (val, next) = read_gguf_string(bytes, pos)?;
                architecture = Some(val);
                pos = next;
            }
            "general.file_type" if value_type == GGUF_T_UINT32 => {
                file_type = Some(read_u32_at(bytes, pos)?);
                pos += 4;
            }
            _ => {
                pos = skip_gguf_value(bytes, pos, value_type)?;
            }
        }
        // Lo que sigue (vocabulario, merges...) no se necesita y puede
        // ocupar varios MiB: no se exige leerlo.
        if architecture.is_some() && file_type.is_some() {
            break;
        }
    }
    Ok(GgufMetadata {
        version,
        tensor_count,
        metadata_kv_count,
        architecture,
        file_type,
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

/// Arquitecturas (`general.architecture` del header GGUF) cuya carga nativa
/// está soportada por `transformers` (vía
/// `AutoModelForCausalLM.from_pretrained(..., gguf_file=...)`).
const GGUF_SUPPORTED_ARCHITECTURES: &[&str] =
    &["llama", "mistral", "gemma", "gemma2", "qwen2", "phi3"];

/// Cuantizaciones (`general.file_type` del header GGUF) que `transformers`
/// puede descuantizar al cargar un GGUF.
///
/// `general.file_type` usa la numeración `LlamaFileType` de llama.cpp, que
/// distingue `_S`/`_M` y NO coincide con los tipos de tensor GGML (H-14).
/// Valores: 0 F32, 1 F16, 2 Q4_0, 7 Q8_0, 14 Q4_K_S, 15 Q4_K_M, 16 Q5_K_S,
/// 17 Q5_K_M, 18 Q6_K.
const GGUF_SUPPORTED_FILE_TYPES: &[u32] = &[0, 1, 2, 7, 14, 15, 16, 17, 18];

/// Cuantizaciones soportadas (legibles) para el hint de error de
/// `ai finetune start`.
pub const GGUF_SUPPORTED_QUANT_HINT: &str =
    "F16, F32, Q4_0, Q4_K_S, Q4_K_M, Q5_K_S, Q5_K_M, Q6_K, Q8_0";

/// Pre-validación de `ai finetune start`: ¿puede `transformers` cargar este
/// GGUF nativo (`gguf_file`) con la arquitectura y el `general.file_type`
/// leídos del header real (no del nombre del fichero)?
///
/// El contenedor de entrenamiento no tiene red, así que esta comprobación
/// evita lanzar un job que fallará al descuantizar/mapear un GGUF no soportado.
pub fn gguf_load_supported(architecture: Option<&str>, file_type: Option<u32>) -> bool {
    let arch_ok = architecture.is_some_and(|a| GGUF_SUPPORTED_ARCHITECTURES.contains(&a));
    let quant_ok = file_type.is_some_and(|t| GGUF_SUPPORTED_FILE_TYPES.contains(&t));
    arch_ok && quant_ok
}

/// Valida un repositorio HuggingFace `org/repo` (dos segmentos no vacíos con
/// caracteres seguros) y lo devuelve normalizado (sin espacios iniciales/finales).
pub fn validate_hf_repo(repo: &str) -> Result<String> {
    let src = repo.trim();
    let parts: Vec<&str> = src.split('/').collect();
    let valid = parts.len() == 2
        && parts.iter().all(|p| {
            !p.is_empty()
                && *p != "."
                && *p != ".."
                && p.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        });
    if !valid {
        return Err(EnolaError::ValidationError(format!(
            "Repositorio HuggingFace '{repo}' inválido: usa 'org/repo'"
        )));
    }
    Ok(src.to_string())
}

/// Nombre de directorio seguro para un repo HF (`org/repo` → `org__repo`),
/// usado como subdirectorio de `tokenizer/` en la biblioteca.
pub fn hf_repo_dir_name(repo: &str) -> Result<String> {
    Ok(validate_hf_repo(repo)?.replace('/', "__"))
}

/// Ruta host del directorio del tokenizer HF de un repositorio:
/// `<base>/<instance>/ollama/models-gguf/tokenizer/<org__repo>/`.
pub fn tokenizer_dir_for_repo(base: &Path, instance: &str, repo: &str) -> Result<PathBuf> {
    let dir = library_dir_for_instance(base, instance)?;
    let name = hf_repo_dir_name(repo)?;
    let joined = dir.join("tokenizer").join(&name);
    if !joined.starts_with(&dir) {
        return Err(EnolaError::SecurityError(format!(
            "La ruta del tokenizer de '{repo}' escapa del directorio de modelos"
        )));
    }
    Ok(joined)
}

/// Resuelve el modelo base de `ai finetune start` contra la biblioteca de la
/// instancia.
///
/// - `requested = Some(id)`: `id` puede ser el `name` del índice (alias de
///   registro) o un fichero `.gguf` (detectado por extensión, que empareja por
///   `filename`).
/// - `requested = None`: se usa `default_model` (el modelo de la instancia),
///   emparejado por `name`.
///
/// Devuelve la entrada de la biblioteca o un error accionable.
pub fn resolve_base_model<'a>(
    models: &'a [AiModel],
    requested: Option<&str>,
    default_model: &str,
) -> Result<&'a AiModel> {
    match requested.map(str::trim).filter(|s| !s.is_empty()) {
        Some(id) if id.to_ascii_lowercase().ends_with(".gguf") => models
            .iter()
            .find(|m| m.filename == id)
            .ok_or_else(|| {
                EnolaError::ValidationError(format!(
                    "El fichero '{id}' no está en la biblioteca de GGUF (búscalo con `ai model list`)"
                ))
            }),
        Some(id) => models
            .iter()
            .find(|m| m.name == id)
            .ok_or_else(|| {
                EnolaError::ValidationError(format!(
                    "El modelo base '{id}' no está en la biblioteca de GGUF (búscalo con `ai model list`)"
                ))
            }),
        None => {
            let dm = default_model.trim();
            models
                .iter()
                .find(|m| m.name == dm)
                .ok_or_else(|| {
                    EnolaError::ValidationError(format!(
                        "El modelo base por defecto '{dm}' de la instancia no está en la biblioteca — importa primero un GGUF de ese modelo (`ai model download` o `ai model import`)"
                    ))
                })
        }
    }
}

/// Resultado de resolver la fuente de una descarga.
pub struct ResolvedDownload {
    /// URL final de descarga.
    pub url: String,
    /// Nombre de fichero `.gguf` validado para la biblioteca.
    pub filename: String,
    /// Repositorio HuggingFace (`org/repo`) de origen, cuando la fuente es un
    /// repo (None para URLs directas). Permite recuperar el tokenizer HF.
    pub hf_repo: Option<String>,
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
            hf_repo: None,
        });
    }
    // Modo repo HuggingFace: `org/repo` exactamente un separador.
    let repo = validate_hf_repo(src).map_err(|_| {
        EnolaError::ValidationError(format!(
            "Fuente '{src}' inválida: usa una URL http(s) a un .gguf o un repo 'org/repo' de HuggingFace"
        ))
    })?;
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
        url: format!("{HUGGINGFACE_BASE_URL}/{repo}/resolve/main/{file}"),
        filename: file.to_string(),
        hf_repo: Some(repo),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Prefijo fijo del header (magic + version + contadores), sin metadata.
    fn gguf_prefix(version: u32, tensors: u64, kv: u64) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(b"GGUF");
        b.extend_from_slice(&version.to_le_bytes());
        b.extend_from_slice(&tensors.to_le_bytes());
        b.extend_from_slice(&kv.to_le_bytes());
        b
    }

    /// Entrada de metadatos KV tipo `STRING`.
    fn kv_string(key: &str, value: &str) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(&(key.len() as u64).to_le_bytes());
        b.extend_from_slice(key.as_bytes());
        b.extend_from_slice(&GGUF_T_STRING.to_le_bytes());
        b.extend_from_slice(&(value.len() as u64).to_le_bytes());
        b.extend_from_slice(value.as_bytes());
        b
    }

    /// Entrada de metadatos KV tipo `UINT32`.
    fn kv_u32(key: &str, value: u32) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(&(key.len() as u64).to_le_bytes());
        b.extend_from_slice(key.as_bytes());
        b.extend_from_slice(&GGUF_T_UINT32.to_le_bytes());
        b.extend_from_slice(&value.to_le_bytes());
        b
    }

    /// Header completo con el prefijo y `kv_entries` pares de metadatos.
    fn gguf_with_metadata(version: u32, kv_entries: Vec<Vec<u8>>) -> Vec<u8> {
        let mut b = gguf_prefix(version, 0, kv_entries.len() as u64);
        for e in kv_entries {
            b.extend_from_slice(&e);
        }
        b
    }

    // ── parse_gguf_header / validate_gguf_header ──

    #[test]
    fn gguf_header_extracts_architecture_and_file_type() {
        let bytes = gguf_with_metadata(
            3,
            vec![
                kv_string("general.architecture", "llama"),
                kv_u32("general.file_type", 12),
                kv_string("general.name", "llama-3"),
            ],
        );
        let meta = parse_gguf_header(&bytes).unwrap();
        assert_eq!(meta.version, 3);
        assert_eq!(meta.metadata_kv_count, 3);
        assert_eq!(meta.architecture.as_deref(), Some("llama"));
        assert_eq!(meta.file_type, Some(12));
    }

    #[test]
    fn gguf_header_skips_other_metadata_types() {
        // Un array de u32 no debe romper el parseo posterior.
        let mut arr = Vec::new();
        arr.extend_from_slice(&GGUF_T_UINT32.to_le_bytes());
        arr.extend_from_slice(&2u64.to_le_bytes());
        arr.extend_from_slice(&1u32.to_le_bytes());
        arr.extend_from_slice(&2u32.to_le_bytes());
        let mut kv = Vec::new();
        kv.extend_from_slice(&(b"tokenizer.list".len() as u64).to_le_bytes());
        kv.extend_from_slice(b"tokenizer.list");
        kv.extend_from_slice(&GGUF_T_ARRAY.to_le_bytes());
        kv.extend_from_slice(&arr);

        let bytes = gguf_with_metadata(
            2,
            vec![
                kv_string("general.architecture", "mistral"),
                kv,
                kv_u32("general.file_type", 1),
            ],
        );
        let meta = parse_gguf_header(&bytes).unwrap();
        assert_eq!(meta.architecture.as_deref(), Some("mistral"));
        assert_eq!(meta.file_type, Some(1));
    }

    /// Modelos con vocabulario grande (Qwen2.5: 151k tokens + merges, 5.66 MiB
    /// de metadata) no caben en el buffer leído; las claves que se necesitan
    /// van al principio, así que el parseo no debe exigir llegar al final.
    #[test]
    fn gguf_header_stops_once_architecture_and_file_type_are_known() {
        let mut vocab = Vec::new();
        vocab.extend_from_slice(&(b"tokenizer.ggml.tokens".len() as u64).to_le_bytes());
        vocab.extend_from_slice(b"tokenizer.ggml.tokens");
        vocab.extend_from_slice(&GGUF_T_ARRAY.to_le_bytes());
        vocab.extend_from_slice(&GGUF_T_STRING.to_le_bytes());
        vocab.extend_from_slice(&151_936u64.to_le_bytes()); // declarado, no incluido
        let bytes = gguf_with_metadata(
            3,
            vec![
                kv_string("general.architecture", "qwen2"),
                kv_u32("general.file_type", 2),
                vocab,
                kv_u32("general.quantization_version", 2),
            ],
        );
        let meta = parse_gguf_header(&bytes).unwrap();
        assert_eq!(meta.architecture.as_deref(), Some("qwen2"));
        assert_eq!(meta.file_type, Some(2));
        assert_eq!(meta.metadata_kv_count, 4);
    }

    #[test]
    fn gguf_header_parses_without_metadata() {
        let meta = parse_gguf_header(&gguf_prefix(2, 10, 0)).unwrap();
        assert_eq!(meta.version, 2);
        assert_eq!(meta.tensor_count, 10);
        assert_eq!(meta.architecture, None);
        assert_eq!(meta.file_type, None);
    }

    #[test]
    fn validate_gguf_header_checks_prefix_only() {
        // Con metadata declarada pero no incluida, el prefijo sigue siendo
        // válido para el chequeo barato, mientras que parse_gguf_header falla.
        assert!(validate_gguf_header(&gguf_prefix(3, 0, 41)).is_ok());
        assert!(parse_gguf_header(&gguf_prefix(3, 0, 41)).is_err());
    }

    #[test]
    fn gguf_header_rejects_bad_magic() {
        let mut b = gguf_prefix(3, 1, 0);
        b[0] = b'X';
        assert!(parse_gguf_header(&b).is_err());
    }

    // ── gguf_load_supported ──

    #[test]
    fn gguf_load_supported_accepts_common_models() {
        assert!(gguf_load_supported(Some("llama"), Some(15))); // Q4_K_M
        assert!(gguf_load_supported(Some("mistral"), Some(7))); // Q8_0
        assert!(gguf_load_supported(Some("gemma"), Some(1))); // F16
        assert!(gguf_load_supported(Some("qwen2"), Some(2))); // Q4_0
        assert!(gguf_load_supported(Some("phi3"), Some(17))); // Q5_K_M
        assert!(gguf_load_supported(Some("llama"), Some(18))); // Q6_K
    }

    #[test]
    fn gguf_load_supported_rejects_unknown_or_unsupported() {
        assert!(!gguf_load_supported(None, Some(15)));
        assert!(!gguf_load_supported(Some("llama"), None));
        assert!(!gguf_load_supported(Some("llama"), Some(10))); // Q2_K
        assert!(!gguf_load_supported(Some("falcon"), Some(7)));
    }

    /// H-14: `general.file_type` usa la numeración `LlamaFileType`, no la de
    /// tipos de tensor GGML. Valores leídos de headers reales de
    /// `Qwen/Qwen2.5-0.5B-Instruct-GGUF` (q4_k_m → 15, q8_0 → 7, q5_0 → 8).
    #[test]
    fn gguf_load_supported_uses_real_llama_file_type_values() {
        assert!(gguf_load_supported(Some("qwen2"), Some(15))); // Q4_K_M real
        assert!(gguf_load_supported(Some("qwen2"), Some(7))); // Q8_0 real
        assert!(!gguf_load_supported(Some("qwen2"), Some(8))); // Q5_0 real: fuera de la lista
        assert!(!gguf_load_supported(Some("qwen2"), Some(12))); // Q3_K_M real
        assert!(!gguf_load_supported(Some("qwen2"), Some(13))); // Q3_K_L real
    }

    #[test]
    fn gguf_header_rejects_truncated() {
        assert!(parse_gguf_header(b"GGUF").is_err());
        assert!(parse_gguf_header(&gguf_prefix(3, 1, 0)[..16]).is_err());
        assert!(parse_gguf_header(&[]).is_err());
    }

    #[test]
    fn gguf_header_rejects_unsupported_version() {
        assert!(parse_gguf_header(&gguf_prefix(1, 1, 0)).is_err());
        assert!(parse_gguf_header(&gguf_prefix(99, 1, 0)).is_err());
    }

    #[test]
    fn gguf_header_rejects_absurd_counts() {
        assert!(parse_gguf_header(&gguf_prefix(3, u64::MAX, 0)).is_err());
        assert!(parse_gguf_header(&gguf_prefix(3, 0, u64::MAX)).is_err());
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
        assert_eq!(r.hf_repo, None);
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
        assert_eq!(
            r.hf_repo.as_deref(),
            Some("bartowski/Llama-3.2-1B-Instruct-GGUF")
        );
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

    // ── validación de repo HF + directorio tokenizer ──

    #[test]
    fn hf_repo_validation_and_dir_name() {
        assert_eq!(validate_hf_repo("org/repo").unwrap(), "org/repo");
        assert_eq!(validate_hf_repo("  org/repo  ").unwrap(), "org/repo");
        assert!(validate_hf_repo("org").is_err());
        assert!(validate_hf_repo("a/b/c").is_err());
        assert!(validate_hf_repo("a//b").is_err());
        assert!(validate_hf_repo("org/re po").is_err());
        assert!(validate_hf_repo("../escape").is_err());
        assert!(validate_hf_repo("org/..").is_err());
        assert_eq!(hf_repo_dir_name("org/repo").unwrap(), "org__repo");
    }

    #[test]
    fn tokenizer_dir_stays_inside_library() {
        let base = Path::new("/srv/enola-ai");
        let p = tokenizer_dir_for_repo(base, "dev", "org/repo").unwrap();
        assert_eq!(
            p,
            PathBuf::from("/srv/enola-ai/dev/ollama/models-gguf/tokenizer/org__repo")
        );
        assert!(tokenizer_dir_for_repo(base, "dev", "../escape").is_err());
        assert!(tokenizer_dir_for_repo(base, "dev", "org/re/po").is_err());
    }

    // ── resolve_base_model ──

    fn model_entry(name: &str, filename: &str) -> AiModel {
        AiModel {
            name: name.to_string(),
            filename: filename.to_string(),
            size_bytes: 0,
            quantization: "Q4_K_M".into(),
            family: "Unknown".into(),
            status: AiModelStatus::Ready,
            source: ModelSource::Downloaded,
            sha256: None,
            hf_repo: None,
        }
    }

    #[test]
    fn resolve_base_model_default_matches_by_name() {
        let lib = vec![
            model_entry("llama-3.2-1b.q4_k_m", "llama-3.2-1b.Q4_K_M.gguf"),
            model_entry("mistral-7b.q4_0", "mistral-7b.Q4_0.gguf"),
        ];
        let m = resolve_base_model(&lib, None, "llama-3.2-1b.q4_k_m").unwrap();
        assert_eq!(m.filename, "llama-3.2-1b.Q4_K_M.gguf");
    }

    #[test]
    fn resolve_base_model_requested_by_name() {
        let lib = vec![
            model_entry("a.q4_k_m", "a.gguf"),
            model_entry("b.q4_0", "b.gguf"),
        ];
        let m = resolve_base_model(&lib, Some("b.q4_0"), "a.q4_k_m").unwrap();
        assert_eq!(m.filename, "b.gguf");
    }

    #[test]
    fn resolve_base_model_requested_by_gguf_filename() {
        let lib = vec![
            model_entry("a.q4_k_m", "a.gguf"),
            model_entry("b.q4_0", "b.gguf"),
        ];
        let m = resolve_base_model(&lib, Some("b.gguf"), "a.q4_k_m").unwrap();
        assert_eq!(m.name, "b.q4_0");
    }

    #[test]
    fn resolve_base_model_default_not_found_mentions_import() {
        let lib = vec![model_entry("a.q4_k_m", "a.gguf")];
        let err = resolve_base_model(&lib, None, "missing:model").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("importa primero un GGUF de ese modelo"));
    }

    #[test]
    fn resolve_base_model_requested_not_found_is_clear() {
        let lib = vec![model_entry("a.q4_k_m", "a.gguf")];
        assert!(resolve_base_model(&lib, Some("nope"), "a.q4_k_m").is_err());
        assert!(resolve_base_model(&lib, Some("nope.gguf"), "a.q4_k_m").is_err());
    }
}
