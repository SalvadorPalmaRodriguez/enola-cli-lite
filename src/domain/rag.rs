// Fase 12.1 — Dominio puro del índice RAG (`ai rag`).
//
// Regla de capa (`domain/`): SIN dependencias de I/O ni externas pesadas.
// Todo aquí es determinista: presets, validación de nombres, chunking con
// overlap, limpieza de texto para embeddings y política de rutas sobre paths
// YA canonicalizados.

use crate::domain::error::{EnolaError, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Rango de puertos host para el vector store (`enola-ai-<name>-<backend>`),
/// sin colisionar con la API Ollama (11435-12000) ni WebUI (8081-8200).
pub const AI_VECTOR_PORT_RANGE: (u16, u16) = (12001, 12500);

/// Backend de vector store soportado.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum VectorStoreBackend {
    /// ChromaDB — setup sencillo, API REST v2 (default).
    #[default]
    Chroma,
    /// Qdrant — ligero (~130 MB), filtros avanzados.
    Qdrant,
}

impl VectorStoreBackend {
    /// Parsea desde string CLI (case-insensitive).
    pub fn from_str_ci(s: &str) -> Result<Self> {
        match s.to_ascii_lowercase().as_str() {
            "chroma" | "chromadb" => Ok(Self::Chroma),
            "qdrant" => Ok(Self::Qdrant),
            other => Err(EnolaError::ValidationError(format!(
                "Vector store '{other}' desconocido. Válidos: chroma, qdrant"
            ))),
        }
    }

    /// Imagen Docker del backend.
    pub fn docker_image(&self) -> &'static str {
        match self {
            Self::Chroma => "chromadb/chroma:latest",
            Self::Qdrant => "qdrant/qdrant:latest",
        }
    }

    /// Puerto interno del contenedor.
    pub fn container_port(&self) -> u16 {
        match self {
            Self::Chroma => 8000,
            Self::Qdrant => 6333,
        }
    }

    /// Sufijo del nombre del contenedor (`enola-ai-<name>-<suffix>`).
    pub fn container_suffix(&self) -> &'static str {
        match self {
            Self::Chroma => "chromadb",
            Self::Qdrant => "qdrant",
        }
    }

    /// Path de datos persistentes DENTRO del contenedor.
    pub fn container_data_path(&self) -> &'static str {
        match self {
            Self::Chroma => "/chroma/chroma",
            Self::Qdrant => "/qdrant/storage",
        }
    }
}

/// Configuración RAG por instancia (`/srv/enola-ai/<name>/rag_config.json`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RagConfig {
    /// Tamaño máximo de chunk en caracteres.
    pub chunk_size: usize,
    /// Overlap entre chunks (contexto).
    pub chunk_overlap: usize,
    /// Eliminar bloques de código antes de indexar.
    pub remove_code_blocks: bool,
    /// Convertir headers markdown a marcadores de sección.
    pub extract_structure: bool,
    /// Modelo de embeddings en Ollama (p.ej. `nomic-embed-text`).
    pub embedding_model: String,
    /// Backend del vector store.
    pub vector_store: VectorStoreBackend,
    /// Nombre del preset aplicado (informativo).
    pub preset: String,
}

impl Default for RagConfig {
    fn default() -> Self {
        Self {
            chunk_size: 3000,
            chunk_overlap: 200,
            remove_code_blocks: false,
            extract_structure: true,
            embedding_model: "nomic-embed-text".to_string(),
            vector_store: VectorStoreBackend::Chroma,
            preset: "docs".to_string(),
        }
    }
}

impl RagConfig {
    /// Construye la config desde un preset: `docs|code|faq|large`.
    pub fn from_preset(preset: &str) -> Result<Self> {
        let mut cfg = Self {
            preset: preset.to_ascii_lowercase(),
            ..Self::default()
        };
        match cfg.preset.as_str() {
            "docs" => {}
            "code" => {
                cfg.chunk_size = 2000;
                cfg.chunk_overlap = 150;
            }
            "faq" => {
                cfg.chunk_size = 1000;
                cfg.chunk_overlap = 50;
                cfg.remove_code_blocks = true;
            }
            "large" => {
                cfg.chunk_size = 4000;
                cfg.chunk_overlap = 400;
            }
            other => {
                return Err(EnolaError::ValidationError(format!(
                    "Preset RAG '{other}' desconocido. Válidos: docs, code, faq, large"
                )))
            }
        }
        Ok(cfg)
    }

    /// Valida los rangos de configuración.
    pub fn validate(&self) -> Result<()> {
        if self.chunk_size < 500 {
            return Err(EnolaError::ValidationError(
                "chunk_size debe ser >= 500".into(),
            ));
        }
        if self.chunk_size > 8000 {
            return Err(EnolaError::ValidationError(
                "chunk_size debe ser <= 8000".into(),
            ));
        }
        if self.chunk_overlap >= self.chunk_size / 2 {
            return Err(EnolaError::ValidationError(
                "chunk_overlap debe ser < 50% de chunk_size".into(),
            ));
        }
        if self.embedding_model.trim().is_empty() {
            return Err(EnolaError::ValidationError(
                "embedding_model no puede estar vacío".into(),
            ));
        }
        Ok(())
    }
}

/// Tipo de fuente ligada al índice RAG.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AiSourceType {
    /// Repositorio git local (sync hace `git pull` + re-ingesta).
    GitRepo,
    /// Directorio local (sync re-ingesta el árbol).
    Filesystem,
    /// Sitio WordPress (sync consulta la REST API `/wp-json/wp/v2/posts`).
    Wordpress,
}

impl AiSourceType {
    /// Parsea `--source-type` del CLI.
    pub fn from_str_ci(s: &str) -> Result<Self> {
        match s.to_ascii_lowercase().as_str() {
            "git" | "gitrepo" => Ok(Self::GitRepo),
            "filesystem" | "dir" | "local" => Ok(Self::Filesystem),
            "wordpress" | "wp" | "web" | "weburl" => Ok(Self::Wordpress),
            other => Err(EnolaError::ValidationError(format!(
                "Tipo de fuente '{other}' desconocido. Válidos: git, filesystem, wordpress"
            ))),
        }
    }
}

/// Fuente ligada a una instancia (`/srv/enola-ai/<name>/rag_sources.json`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiSource {
    /// Identificador único (`src_<instance>_<micros>`).
    pub id: String,
    /// Instancia propietaria.
    pub instance_name: String,
    /// Tipo de fuente.
    pub source_type: AiSourceType,
    /// Path local o URL base.
    pub location: String,
    /// Última sincronización (ISO 8601) o `None`.
    pub last_synced: Option<String>,
}

/// Chunk preprocesado listo para embedding.
#[derive(Debug, Clone, PartialEq)]
pub struct RagChunk {
    /// Contenido del chunk.
    pub content: String,
    /// Índice del chunk dentro del documento.
    pub chunk_index: usize,
    /// Total de chunks del documento.
    pub total_chunks: usize,
    /// Longitud original del documento.
    pub original_length: usize,
}

// ── Limpieza de texto (sin regex — solo operaciones de string) ────────────

/// `true` si el carácter es decorativo de ASCII-art/box-drawing.
fn is_decorative(c: char) -> bool {
    matches!(
        c,
        '═' | '║'
            | '╔'
            | '╗'
            | '╚'
            | '╝'
            | '╠'
            | '╣'
            | '╦'
            | '╩'
            | '╬'
            | '┌'
            | '┐'
            | '└'
            | '┘'
            | '├'
            | '┤'
            | '┬'
            | '┴'
            | '─'
            | '│'
            | '█'
            | '▓'
            | '▒'
            | '░'
    )
}

/// Elimina etiquetas HTML sencillas (`<tag ...>`) con un escáner de chars.
fn strip_html_tags(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_tag = false;
    let mut prev = '\0';
    for c in input.chars() {
        if in_tag {
            if c == '>' {
                in_tag = false;
            }
        } else if c == '<'
            && (prev.is_alphanumeric() || prev == ' ' || prev == '\n' || prev == '\0')
        {
            // Solo si parece el inicio de una etiqueta: el siguiente char útil
            // no se conoce aquí; aproximación — verificaremos al cerrar.
            in_tag = true;
        } else {
            out.push(c);
        }
        prev = c;
    }
    // Si quedó un '<' sin cerrar al final, no importa: el contenido visible
    // se conservó. Aproximación deliberadamente simple.
    out
}

/// Elimina imágenes `![alt](src)` y convierte enlaces `[text](url)` en `text`.
fn strip_markdown_links(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        // Imagen: ![...](...)
        if chars[i] == '!' && i + 1 < chars.len() && chars[i + 1] == '[' {
            if let Some(close) = find_closing(&chars, i + 1, ']') {
                if close + 1 < chars.len() && chars[close + 1] == '(' {
                    if let Some(end) = find_closing(&chars, close + 1, ')') {
                        i = end + 1;
                        continue;
                    }
                }
                i = close + 1;
                continue;
            }
        }
        // Enlace: [text](url) → text
        if chars[i] == '[' {
            if let Some(close) = find_closing(&chars, i, ']') {
                if close + 1 < chars.len() && chars[close + 1] == '(' {
                    if let Some(end) = find_closing(&chars, close + 1, ')') {
                        for c in &chars[i + 1..close] {
                            out.push(*c);
                        }
                        i = end + 1;
                        continue;
                    }
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// Encuentra el índice de `target` a partir de `start` (sin anidado).
fn find_closing(chars: &[char], start: usize, target: char) -> Option<usize> {
    (start + 1..chars.len()).find(|&i| chars[i] == target)
}

/// Limpia el contenido para embeddings: estructura markdown, decoradores,
/// código (opcional), enlaces, HTML y whitespace.
pub fn clean_rag_text(content: &str, remove_code_blocks: bool, extract_structure: bool) -> String {
    let mut out = String::with_capacity(content.len());
    let mut in_fence = false;
    let mut code_buffer = String::new();

    for raw_line in content.lines() {
        let line = raw_line.trim_end();
        let trimmed = line.trim_start();

        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            if in_fence {
                // Cierre de bloque
                in_fence = false;
                if remove_code_blocks {
                    out.push_str("[código omitido]\n");
                } else {
                    out.push_str("[CÓDIGO]\n");
                    out.push_str(&code_buffer);
                    out.push_str("[/CÓDIGO]\n");
                }
                code_buffer.clear();
            } else {
                in_fence = true;
                code_buffer.clear();
            }
            continue;
        }
        if in_fence {
            if !remove_code_blocks {
                code_buffer.push_str(line);
                code_buffer.push('\n');
            }
            continue;
        }

        // Filtrar líneas puramente decorativas (solo símbolos/box-drawing).
        if !line.is_empty()
            && line
                .chars()
                .all(|c| is_decorative(c) || matches!(c, '-' | '_' | '=' | '*' | '#'))
        {
            continue;
        }

        let mut line_out = String::with_capacity(line.len());
        if extract_structure {
            if let Some(h) = trimmed.strip_prefix("### ") {
                line_out.push_str(&format!("[{h}]\n"));
            } else if let Some(h) = trimmed.strip_prefix("## ") {
                line_out.push_str(&format!("[SECCIÓN: {h}]\n"));
            } else if let Some(h) = trimmed.strip_prefix("# ") {
                line_out.push_str(&format!("[TÍTULO: {h}]\n"));
            }
        }

        // Bullets y listas numeradas
        let body = if let Some(b) = trimmed.strip_prefix("- ") {
            format!("• {b}")
        } else if let Some(b) = trimmed.strip_prefix("* ") {
            format!("• {b}")
        } else {
            // "1. texto" → "texto"
            let mut it = trimmed;
            if let Some(pos) = it.find(". ") {
                let (num, rest) = it.split_at(pos);
                if !num.is_empty() && num.chars().all(|c| c.is_ascii_digit()) {
                    it = &rest[1..];
                }
            }
            it.to_string()
        };
        line_out.push_str(&body);
        line_out.push('\n');

        // Quitar caracteres decorativos sueltos y no-ASCII de control/emoji
        let filtered: String = line_out
            .chars()
            .filter(|&c| !is_decorative(c) && (c.is_ascii() || c.is_alphanumeric() || c == '•'))
            .collect();
        out.push_str(&filtered);
    }

    if in_fence && !remove_code_blocks {
        // Bloque sin cerrar: volcar el buffer tal cual.
        out.push_str("[CÓDIGO]\n");
        out.push_str(&code_buffer);
    }

    let cleaned = strip_markdown_links(&out);
    let cleaned = strip_html_tags(&cleaned);

    // Normalizar whitespace: colapsar runs de espacios/tabs y >2 newlines.
    let mut final_out = String::with_capacity(cleaned.len());
    let mut space_run = 0usize;
    let mut nl_run = 0usize;
    for c in cleaned.chars() {
        match c {
            ' ' | '\t' => {
                space_run += 1;
                nl_run = 0;
                if space_run == 1 {
                    final_out.push(' ');
                }
            }
            '\n' => {
                nl_run += 1;
                space_run = 0;
                if nl_run <= 2 {
                    final_out.push('\n');
                }
            }
            _ => {
                space_run = 0;
                nl_run = 0;
                final_out.push(c);
            }
        }
    }

    final_out
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

// ── Chunking con overlap respetando fronteras UTF-8 ───────────────────────

/// Divide `content` en chunks de `max_size` caracteres con `overlap`,
/// preferiendo cortes en párrafo → frase → salto de línea.
pub fn chunk_text(content: &str, max_size: usize, overlap: usize) -> Vec<String> {
    if max_size == 0 {
        return if content.is_empty() {
            Vec::new()
        } else {
            vec![content.to_string()]
        };
    }
    if content.len() <= max_size {
        return if content.is_empty() {
            Vec::new()
        } else {
            vec![content.to_string()]
        };
    }

    let mut chunks = Vec::new();
    let mut start = 0usize;

    while start < content.len() {
        while start < content.len() && !content.is_char_boundary(start) {
            start += 1;
        }
        if start >= content.len() {
            break;
        }

        let mut end = (start + max_size).min(content.len());
        while end > start && !content.is_char_boundary(end) {
            end -= 1;
        }
        if end <= start {
            break;
        }

        if end < content.len() {
            let slice = &content[start..end];
            for sep in ["\n\n", ". ", "\n"] {
                if let Some(pos) = slice.rfind(sep) {
                    let new_end = start + pos + sep.len();
                    if new_end > start && content.is_char_boundary(new_end) {
                        end = new_end;
                        break;
                    }
                }
            }
        }

        let chunk = content[start..end].trim();
        if !chunk.is_empty() {
            chunks.push(chunk.to_string());
        }
        if end >= content.len() {
            break;
        }
        // Siguiente inicio con overlap, sin retroceder (anti-bucle).
        let candidate = end.saturating_sub(overlap);
        start = if candidate <= start { end } else { candidate };
        while start < content.len() && !content.is_char_boundary(start) {
            start += 1;
        }
    }

    if chunks.is_empty() && !content.is_empty() {
        chunks.push(content.to_string());
    }
    chunks
}

/// Preprocesa un documento para indexar: limpieza + chunking.
pub fn preprocess_document(content: &str, config: &RagConfig) -> Vec<RagChunk> {
    let cleaned = clean_rag_text(content, config.remove_code_blocks, config.extract_structure);
    let chunks = chunk_text(&cleaned, config.chunk_size, config.chunk_overlap);
    let total = chunks.len();
    let original_length = content.len();
    chunks
        .into_iter()
        .enumerate()
        .map(|(i, c)| RagChunk {
            content: c,
            chunk_index: i,
            total_chunks: total,
            original_length,
        })
        .collect()
}

// ── Política de rutas sobre paths CANONICALIZADOS (pure check) ────────────

/// Prefijos absolutos del sistema que NUNCA pueden ser fuente RAG
/// (protege `/etc`, `/root`, `/proc`, …). La canonicalización (symlinks)
/// se resuelve en la capa de aplicación con `std::fs::canonicalize`; este
/// chequeo opera sobre el resultado ya resuelto.
pub const RAG_DENIED_PREFIXES: &[&str] = &[
    "/etc",
    "/root",
    "/proc",
    "/sys",
    "/dev",
    "/boot",
    "/run",
    "/usr",
    "/bin",
    "/sbin",
    "/lib",
    "/lib64",
    "/snap",
    "/var/lib",
    "/var/log",
    "/var/run",
    "/opt/enola",
];

/// Rechaza paths canonicalizados que apunten a zonas del sistema.
/// El canon debe ser absoluto, sin `..`, y no vivir bajo `RAG_DENIED_PREFIXES`.
pub fn validate_rag_source_path(canon: &Path) -> Result<()> {
    if !canon.is_absolute() {
        return Err(EnolaError::ValidationError(format!(
            "La fuente RAG debe ser una ruta absoluta: '{}'",
            canon.display()
        )));
    }
    let s = canon.to_string_lossy();
    for denied in RAG_DENIED_PREFIXES {
        if s == *denied || s.starts_with(&format!("{denied}/")) {
            return Err(EnolaError::ValidationError(format!(
                "La fuente RAG '{s}' apunta a una ruta del sistema ({denied}) — no permitida"
            )));
        }
    }
    Ok(())
}

/// Valida un nombre de colección (Chroma: 3-63 chars, alnum + `._-`,
/// empieza y acaba en alfanumérico).
pub fn validate_collection_name(name: &str) -> Result<()> {
    let ok = name.len() >= 3
        && name.len() <= 63
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        && name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric())
        && name
            .chars()
            .last()
            .is_some_and(|c| c.is_ascii_alphanumeric());
    if !ok {
        return Err(EnolaError::ValidationError(format!(
            "Nombre de colección inválido: '{name}' (3-63, alfanumérico + '._-', bordes alfanuméricos)"
        )));
    }
    Ok(())
}

/// Nombre de colección RAG por defecto para una instancia (`rag_<name>`).
pub fn collection_for_instance(instance: &str) -> Result<String> {
    crate::domain::ai::validate_ai_instance_name(instance)?;
    let name = format!("rag_{instance}");
    validate_collection_name(&name)?;
    Ok(name)
}

/// Nombre de colección para el lookup de documentación (`docs_lookup`).
pub const DOCS_COLLECTION: &str = "enola_docs";

#[cfg(test)]
mod tests {
    use super::*;

    // ── VectorStoreBackend ──

    #[test]
    fn backend_parses_case_insensitive() {
        assert_eq!(
            VectorStoreBackend::from_str_ci("Chroma").unwrap(),
            VectorStoreBackend::Chroma
        );
        assert_eq!(
            VectorStoreBackend::from_str_ci("qdrant").unwrap(),
            VectorStoreBackend::Qdrant
        );
        assert!(VectorStoreBackend::from_str_ci("weaviate").is_err());
    }

    #[test]
    fn backend_container_facts() {
        assert_eq!(VectorStoreBackend::Chroma.container_port(), 8000);
        assert_eq!(VectorStoreBackend::Qdrant.container_port(), 6333);
        assert!(VectorStoreBackend::Chroma.docker_image().contains("chroma"));
        assert_eq!(VectorStoreBackend::Chroma.container_suffix(), "chromadb");
    }

    // ── RagConfig presets + validate ──

    #[test]
    fn preset_docs_is_default() {
        let c = RagConfig::from_preset("docs").unwrap();
        assert_eq!(c.chunk_size, 3000);
        assert!(!c.remove_code_blocks);
    }

    #[test]
    fn preset_faq_removes_code_and_small_chunks() {
        let c = RagConfig::from_preset("faq").unwrap();
        assert!(c.remove_code_blocks);
        assert_eq!(c.chunk_size, 1000);
        assert_eq!(c.chunk_overlap, 50);
    }

    #[test]
    fn preset_rejects_unknown() {
        assert!(RagConfig::from_preset("nope").is_err());
    }

    #[test]
    fn config_validate_bounds() {
        assert!(RagConfig {
            chunk_size: 100,
            ..RagConfig::default()
        }
        .validate()
        .is_err());
        assert!(RagConfig {
            chunk_size: 9000,
            ..RagConfig::default()
        }
        .validate()
        .is_err());
        assert!(RagConfig {
            chunk_size: 1000,
            chunk_overlap: 600,
            ..RagConfig::default()
        }
        .validate()
        .is_err());
        assert!(RagConfig::default().validate().is_ok());
    }

    // ── AiSourceType ──

    #[test]
    fn source_type_parses() {
        assert_eq!(
            AiSourceType::from_str_ci("git").unwrap(),
            AiSourceType::GitRepo
        );
        assert_eq!(
            AiSourceType::from_str_ci("filesystem").unwrap(),
            AiSourceType::Filesystem
        );
        assert_eq!(
            AiSourceType::from_str_ci("wordpress").unwrap(),
            AiSourceType::Wordpress
        );
        assert!(AiSourceType::from_str_ci("ftp").is_err());
    }

    // ── clean_rag_text ──

    #[test]
    fn clean_strips_headers_and_decorators() {
        let input = "# Título\n═══════════════\n## Sección\nTexto normal.\n- item uno\n1. first";
        let out = clean_rag_text(input, false, true);
        assert!(out.contains("[TÍTULO: Título]"), "got: {out}");
        assert!(out.contains("[SECCIÓN: Sección]"), "got: {out}");
        assert!(!out.contains('═'));
        assert!(out.contains("• item uno"));
        assert!(out.contains("first"));
    }

    #[test]
    fn clean_removes_code_blocks_when_configured() {
        let input = "Antes\n```rust\nfn main() {}\n```\nDespués";
        let out = clean_rag_text(input, true, true);
        assert!(out.contains("[código omitido]"));
        assert!(!out.contains("fn main"));
        let kept = clean_rag_text(input, false, true);
        assert!(kept.contains("fn main"), "got: {kept}");
    }

    #[test]
    fn clean_converts_links_and_strips_images_html() {
        let input = "Ver [documento](https://x.io/d) y ![img](x.png). <p>texto</p>";
        let out = clean_rag_text(input, false, true);
        assert!(out.contains("documento"), "got: {out}");
        assert!(!out.contains("https://x.io"), "got: {out}");
        assert!(!out.contains("x.png"));
        assert!(!out.contains("<p>"), "got: {out}");
    }

    #[test]
    fn clean_collapses_whitespace() {
        let input = "a   b\t\tc\n\n\n\n\nd";
        let out = clean_rag_text(input, false, false);
        assert_eq!(out, "a b c\nd");
    }

    // ── chunk_text ──

    #[test]
    fn chunk_respects_size_and_overlap() {
        let cfg_body = "palabra ".repeat(400); // ~3600 chars
        let chunks = chunk_text(&cfg_body, 1000, 200);
        assert!(chunks.len() >= 4, "got {}", chunks.len());
        for c in &chunks[..chunks.len() - 1] {
            assert!(c.len() <= 1000, "chunk too big: {}", c.len());
        }
    }

    #[test]
    fn chunk_prefers_paragraph_boundaries() {
        let input = format!("{} \n\n {}", "a".repeat(400), "b".repeat(400));
        let chunks = chunk_text(&input, 500, 50);
        assert!(
            chunks[0].ends_with('a'),
            "debe cortar en \\n\\n: {:?}",
            chunks
        );
    }

    #[test]
    fn chunk_handles_utf8_and_small_inputs() {
        let utf8 = "áéíóú ".repeat(300);
        let chunks = chunk_text(&utf8, 500, 100);
        assert!(!chunks.is_empty());
        for c in &chunks {
            assert!(c.is_char_boundary(0) && c.is_char_boundary(c.len()));
        }
        assert_eq!(chunk_text("", 10, 0), Vec::<String>::new());
        assert_eq!(chunk_text("corto", 100, 0), vec!["corto".to_string()]);
    }

    #[test]
    fn preprocess_returns_indexed_chunks() {
        let cfg = RagConfig {
            chunk_size: 500,
            chunk_overlap: 50,
            ..RagConfig::default()
        };
        let chunks = preprocess_document(&"x".repeat(1400), &cfg);
        assert!(chunks.len() >= 3);
        assert_eq!(chunks[0].chunk_index, 0);
        assert_eq!(chunks[0].total_chunks, chunks.len());
        assert_eq!(chunks[0].original_length, 1400);
    }

    // ── política de rutas canonicalizadas ──

    #[test]
    fn rag_path_denies_system_dirs() {
        for bad in [
            "/etc/nginx",
            "/root",
            "/proc/self",
            "/sys/kernel",
            "/var/lib/x",
        ] {
            assert!(
                validate_rag_source_path(Path::new(bad)).is_err(),
                "debe rechazar {bad}"
            );
        }
    }

    #[test]
    fn rag_path_accepts_user_dirs() {
        assert!(validate_rag_source_path(Path::new("/home/user/docs")).is_ok());
        assert!(validate_rag_source_path(Path::new("/srv/enola-files/x")).is_ok());
        assert!(validate_rag_source_path(Path::new("/tmp/data")).is_ok());
    }

    #[test]
    fn rag_path_rejects_relative() {
        assert!(validate_rag_source_path(Path::new("docs/sub")).is_err());
    }

    // ── colecciones ──

    #[test]
    fn collection_name_rules() {
        assert!(validate_collection_name("rag_dev").is_ok());
        assert!(validate_collection_name("enola_docs").is_ok());
        assert!(validate_collection_name("ab").is_err()); // <3
        assert!(validate_collection_name("_lead").is_err());
        assert!(validate_collection_name("trail-").is_err());
        assert!(validate_collection_name("a b").is_err());
        assert!(validate_collection_name(&"x".repeat(64)).is_err());
    }

    #[test]
    fn collection_for_instance_derives_valid_name() {
        assert_eq!(collection_for_instance("dev").unwrap(), "rag_dev");
        assert!(collection_for_instance("Bad_Name").is_err());
    }
}
