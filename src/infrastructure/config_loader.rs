//! Loader genérico del archivo `~/.enola/config.toml` (CONFIG-001 + CONFIG-002 + QUAL-001).
//!
//! Lee una sección del `config.toml` y devuelve un mapa `clave → valor` con
//! los valores stringificados (strings sin comillas, enteros/bool a su
//! representación textual, arrays en su forma TOML).
//!
//! # QUAL-001 (2026-04-20)
//!
//! Anteriormente este módulo parseaba TOML "a mano" con `split_once('=')`,
//! lo que impedía soportar strings multilínea, arrays, tablas anidadas o
//! escapes. Ahora delega en la crate `toml` (ya en `Cargo.toml`). La firma
//! pública `load_section(&str) -> HashMap<String,String>` se mantiene
//! intacta para que los callers existentes no cambien.
//!
//! Formato esperado:
//! ```toml
//! [web]
//! web_public_url = "https://enola.example.com"
//!
//! [distribution]
//! binary_base_url = "https://releases.example.com"
//! ```
//!
//! Comportamiento tolerante (contrato):
//! - Archivo inexistente / ilegible → `HashMap` vacío.
//! - TOML inválido → `HashMap` vacío (no propaga error).
//! - Sección inexistente → `HashMap` vacío.
use std::collections::HashMap;
use std::path::PathBuf;
/// Devuelve la ruta por defecto al `config.toml` del usuario.
pub fn config_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".enola").join("config.toml"))
}
/// Lee una sección concreta del `config.toml` y devuelve sus pares clave-valor.
///
/// Todos los valores escalares se convierten a `String` (ver [`value_to_string`]).
/// Las sub-tablas anidadas (`[section.sub]`) NO se expanden aquí: pide la
/// sub-sección con otra llamada usando el dot-path completo (`"section.sub"`).
pub fn load_section(section: &str) -> HashMap<String, String> {
    load_section_from_path(config_path(), section)
}

fn load_section_from_path(path: Option<PathBuf>, section: &str) -> HashMap<String, String> {
    let Some(path) = path else {
        return HashMap::new();
    };
    let Ok(content) = std::fs::read_to_string(&path) else {
        return HashMap::new();
    };
    parse_section(&content, section)
}
/// Parsea la sección `section` de un texto TOML real.
///
/// `section` puede ser simple (`"misc"`) o dot-path (`"misc.extra"`).
/// Separado para ser testeable sin filesystem.
pub fn parse_section(content: &str, section: &str) -> HashMap<String, String> {
    let Ok(root) = toml::from_str::<toml::Value>(content) else {
        return HashMap::new();
    };
    // Navegar `a.b.c` descendiendo por tablas.
    let mut current: &toml::Value = &root;
    for part in section.split('.') {
        match current.get(part) {
            Some(v) => current = v,
            None => return HashMap::new(),
        }
    }
    let Some(table) = current.as_table() else {
        return HashMap::new();
    };
    let mut out = HashMap::new();
    for (k, v) in table {
        if let Some(s) = value_to_string(v) {
            out.insert(k.clone(), s);
        }
    }
    out
}
/// Convierte un `toml::Value` escalar a `String`.
///
/// - `String` → su contenido sin comillas.
/// - `Integer` / `Float` / `Boolean` / `Datetime` → `to_string()`.
/// - `Array` → representación TOML (`[a, b]`), útil para inspección/logging.
/// - `Table` → `None` (las sub-tablas se leen con otra llamada a `load_section`).
fn value_to_string(v: &toml::Value) -> Option<String> {
    match v {
        toml::Value::String(s) => Some(s.clone()),
        toml::Value::Integer(i) => Some(i.to_string()),
        toml::Value::Float(f) => Some(f.to_string()),
        toml::Value::Boolean(b) => Some(b.to_string()),
        toml::Value::Datetime(d) => Some(d.to_string()),
        toml::Value::Array(_) => Some(v.to_string()),
        toml::Value::Table(_) => None,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn empty_content_empty_map() {
        let m = parse_section("", "distribution");
        assert!(m.is_empty());
    }
    #[test]
    fn reads_target_section_only() {
        let toml = r#"
[misc]
key = "https://example.com"
[distribution]
binary_base_url = "https://dl.example.com"
minisign_pubkey_url = "https://example.com/pub"
[web]
web_public_url = "https://example.com"
"#;
        let d = parse_section(toml, "distribution");
        assert_eq!(d.get("binary_base_url").unwrap(), "https://dl.example.com");
        assert_eq!(
            d.get("minisign_pubkey_url").unwrap(),
            "https://example.com/pub"
        );
        assert_eq!(d.len(), 2);
        let w = parse_section(toml, "web");
        assert_eq!(w.get("web_public_url").unwrap(), "https://example.com");
    }
    #[test]
    fn ignores_comments_and_blank_lines() {
        let toml = r#"
# un comentario
[distribution]
# otro comentario
binary_base_url = "https://dl.example.com"  # inline
"#;
        let d = parse_section(toml, "distribution");
        assert_eq!(d.get("binary_base_url").unwrap(), "https://dl.example.com");
    }
    #[test]
    fn unknown_section_returns_empty() {
        let toml = "[misc]\nkey = \"x\"\n";
        assert!(parse_section(toml, "distribution").is_empty());
    }
    #[test]
    fn invalid_toml_returns_empty() {
        // QUAL-001: contrato tolerante — TOML malformado devuelve mapa vacío,
        // nunca propaga error ni hace panic.
        let bad = "[misc\nkey = unquoted";
        assert!(parse_section(bad, "misc").is_empty());
    }

    #[test]
    fn hostile_toml_corpus_never_panics_and_keeps_tolerant_contract() {
        // SEC-EXT-DEV-070: corpus hostil para parser TOML. Debe devolver mapa
        // (vacío o con datos) pero NUNCA panic ni error propagado.
        let corpus = vec![
            "[misc\nkey='broken'",                          // tabla sin cierre
            "[misc]\nkey = \"\\u0000\"",                    // NUL escaped
            "[misc]\nkey = \"http://example.com\n",         // string sin cerrar
            "[misc]\narr = [1,2,3,",                        // array truncado
            "[misc.extra]\nvalue = { nested = { x = 1 } }", // inline nested
            "[a.b.c.d.e]\nvalue='ok'",                      // dot-path profundo
            "not even toml",                                // basura total
        ];

        for sample in corpus {
            let _ = parse_section(sample, "misc");
            let _ = parse_section(sample, "misc.extra");
            let _ = parse_section(sample, "a.b.c.d.e");
        }
    }

    // TEST-COV-UNIT-003: cubrir value_to_string Datetime (L87).
    #[test]
    fn value_to_string_datetime_branch() {
        let toml = "[section]\nsome_date = 2024-01-15\n";
        let m = parse_section(toml, "section");
        assert!(
            m.contains_key("some_date"),
            "fecha debe estar presente: {:?}",
            m
        );
        assert!(m.get("some_date").unwrap().contains("2024"));
    }

    // TEST-COV-UNIT-003: cubrir value_to_string Array (L88).
    #[test]
    fn value_to_string_array_branch() {
        let toml = "[section]\nvals = [1, 2, 3]\n";
        let m = parse_section(toml, "section");
        assert!(m.contains_key("vals"), "array debe estar presente: {:?}", m);
    }

    // TEST-COV-UNIT-003: cubrir L65 (dot-path a escalar, no tabla).
    #[test]
    fn section_dot_path_to_scalar_returns_empty() {
        let toml = "[misc]\nurl = \"https://example.com\"\n";
        let m = parse_section(toml, "misc.url");
        assert!(m.is_empty(), "escalar no es tabla -> mapa vacio");
    }

    #[test]
    fn load_section_from_none_path_returns_empty() {
        let m = load_section_from_path(None, "misc");
        assert!(m.is_empty());
    }
}
