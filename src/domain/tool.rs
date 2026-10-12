// MCP-004 — Tipos puros de tools MCP + clasificación lectura/escritura.

use serde::{Deserialize, Serialize};

/// Clasificación de seguridad de una tool MCP.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolKind {
    /// No muta el sistema: listar, consultar, planificar, verificar.
    ReadOnly,
    /// Puede mutar el sistema: crear, borrar, publicar, configurar.
    Mutating,
}

/// Especificación de una tool MCP (nombre, descripción, schema y clase).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
    pub kind: ToolKind,
}

impl ToolSpec {
    /// Construye una `ToolSpec` validando que el schema es un JSON Schema de
    /// tipo `object` (lo que los clientes MCP esperan para `inputSchema`).
    pub fn new(
        name: &str,
        description: &str,
        input_schema: serde_json::Value,
        kind: ToolKind,
    ) -> Result<Self, String> {
        if name.trim().is_empty() {
            return Err("tool name must not be empty".to_string());
        }
        let is_object_schema = input_schema
            .get("type")
            .and_then(|t| t.as_str())
            .map(|t| t == "object")
            .unwrap_or(false);
        if !is_object_schema {
            return Err("inputSchema must be a JSON Schema object with type=object".to_string());
        }
        Ok(Self {
            name: name.to_string(),
            description: description.to_string(),
            input_schema,
            kind,
        })
    }
}

/// Resultado de una ejecución de tool, con marca de truncado.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolOutcome {
    pub content: String,
    pub truncated: bool,
}

/// Trunca `text` a `max` bytes respetando límites de caracteres UTF-8.
/// Devuelve `(texto, si_truncó)`.
pub fn truncate_output(text: &str, max: usize) -> (String, bool) {
    if text.len() <= max {
        return (text.to_string(), false);
    }
    let mut end = max;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    (text[..end].to_string(), true)
}

/// Subcomandos de LECTURA por familia (allowlist explícita).
///
/// Default-deny: cualquier (familia, subcomando) no listado aquí se clasifica
/// como `Mutating`. Un comando nuevo NUNCA se cuela como lectura por descuido.
pub const READ_ONLY_SUBCOMMANDS: &[(&str, &str)] = &[
    ("tor", "list"),
    ("git", "list"),
    ("git", "status"),
    ("wp", "list"),
    ("wp", "status"),
    ("drupal", "list"),
    ("drupal", "status"),
    ("ghost", "list"),
    ("ghost", "status"),
    ("magnolia", "list"),
    ("magnolia", "status"),
    ("strapi", "list"),
    ("strapi", "status"),
    ("wagtail", "list"),
    ("wagtail", "status"),
    ("files", "list"),
    ("maintenance", "status"),
    ("maintenance", "timer-status"),
    ("diag", "summary"),
    ("diag", "nginx"),
    ("diag", "tor"),
    ("diag", "ssh"),
    ("diag", "word-press"),
    ("diag", "wp-sync"),
    ("diag", "nginx-test"),
    ("diag", "resources"),
    ("test", "list"),
    ("test", "results"),
    ("logs", "list"),
    ("logs", "view"),
    ("ports", "list"),
    ("firewall", "status"),
    ("apparmor", "status"),
    ("vpn", "list"),
    ("vpn", "status"),
    ("docs", "quickstart"),
    ("docs", "commands"),
    ("docs", "concepts"),
    ("docs", "faq"),
    ("docs", "examples"),
    ("docs", "search"),
    ("docs", "quantum-security"),
    ("docs", "verify-downloads"),
    ("docs", "security"),
    ("docs", "install-from-iso"),
    ("update", "check"),
    ("update", "schema"),
    ("update", "verify-feed"),
    ("plan", "wp"),
    ("plan", "tor"),
    ("plan", "git"),
];

/// Familias HOJA (sin subcomando) que son de LECTURA.
pub const READ_ONLY_LEAF_FAMILIES: &[&str] = &[
    "doctor",
    "quickref",
    "license",
    "verify",
    "config-show",
    "config-validate",
];

/// Clasifica un comando como lectura o mutación (default-deny).
pub fn classify(family: &str, subcommand: Option<&str>) -> ToolKind {
    match subcommand {
        Some(sub) => {
            if READ_ONLY_SUBCOMMANDS.contains(&(family, sub)) {
                ToolKind::ReadOnly
            } else {
                ToolKind::Mutating
            }
        }
        None => {
            if READ_ONLY_LEAF_FAMILIES.contains(&family) {
                ToolKind::ReadOnly
            } else {
                ToolKind::Mutating
            }
        }
    }
}

/// Valida `args` contra un subconjunto de JSON Schema: `type: object`,
/// `required`, `properties` (con `type` y `enum`). Devuelve error descriptivo.
pub fn validate_input(schema: &serde_json::Value, args: &serde_json::Value) -> Result<(), String> {
    if schema.get("type").and_then(|t| t.as_str()) != Some("object") {
        return Err("schema must declare type: object".to_string());
    }
    let obj = args
        .as_object()
        .ok_or_else(|| "arguments must be a JSON object".to_string())?;

    if let Some(required) = schema.get("required").and_then(|r| r.as_array()) {
        for name in required {
            let name = name
                .as_str()
                .ok_or_else(|| "required must be strings".to_string())?;
            if !obj.contains_key(name) {
                return Err(format!("missing required argument: {name}"));
            }
        }
    }

    if let Some(props) = schema.get("properties").and_then(|p| p.as_object()) {
        for (name, prop_schema) in props {
            let Some(value) = obj.get(name) else { continue };
            validate_property(name, prop_schema, value)?;
        }
    }
    Ok(())
}

fn validate_property(
    name: &str,
    schema: &serde_json::Value,
    value: &serde_json::Value,
) -> Result<(), String> {
    if let Some(allowed) = schema.get("enum").and_then(|e| e.as_array()) {
        if !allowed.iter().any(|v| v == value) {
            let choices: Vec<String> = allowed
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect();
            return Err(format!(
                "argument '{name}' must be one of: {}",
                choices.join(", ")
            ));
        }
    }

    if let Some(ty) = schema.get("type").and_then(|t| t.as_str()) {
        let ok = match ty {
            "string" => value.is_string(),
            "integer" => value.is_i64() || value.is_u64(),
            "number" => value.is_number(),
            "boolean" => value.is_boolean(),
            "array" => value.is_array(),
            "object" => value.is_object(),
            _ => true,
        };
        if !ok {
            return Err(format!("argument '{name}' must be of type {ty}"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn load_tree() -> serde_json::Value {
        serde_json::from_str(include_str!("../../assets/mcp_tools.json")).unwrap()
    }

    // ── 4.2 ToolSpec / ToolKind / truncado ──

    #[test]
    fn tool_spec_rejects_invalid_schema() {
        assert!(ToolSpec::new("x", "d", json!({"type": "array"}), ToolKind::ReadOnly).is_err());
        assert!(ToolSpec::new("x", "d", json!(42), ToolKind::ReadOnly).is_err());
    }

    #[test]
    fn tool_spec_accepts_object_schema() {
        let spec = ToolSpec::new("x", "d", json!({"type": "object"}), ToolKind::ReadOnly).unwrap();
        assert_eq!(spec.kind, ToolKind::ReadOnly);
        assert_eq!(spec.name, "x");
    }

    #[test]
    fn truncate_output_respects_limit_and_marks_truncation() {
        assert_eq!(truncate_output("abc", 5), ("abc".to_string(), false));
        assert_eq!(truncate_output("abc", 3), ("abc".to_string(), false));
        assert_eq!(truncate_output("abcdef", 3), ("abc".to_string(), true));
    }

    #[test]
    fn truncate_output_never_panics_on_utf8_boundary() {
        // 'é' ocupa 2 bytes: cortar en medio no debe panickear.
        let (out, truncated) = truncate_output("aé", 1);
        assert_eq!(out, "a");
        assert!(truncated);
    }

    // ── 4.3 Clasificación read/write (default-deny) ──

    #[test]
    fn classify_semantics_sample() {
        assert_eq!(classify("wp", Some("list")), ToolKind::ReadOnly);
        assert_eq!(classify("wp", Some("create")), ToolKind::Mutating);
        assert_eq!(classify("firewall", Some("allow")), ToolKind::Mutating);
        assert_eq!(classify("config-validate", None), ToolKind::ReadOnly);
    }

    #[test]
    fn default_deny_unlisted_is_mutating() {
        assert_eq!(classify("wp", Some("delete")), ToolKind::Mutating);
        assert_eq!(classify("bogus", Some("bogus")), ToolKind::Mutating);
        assert_eq!(classify("uninstall", None), ToolKind::Mutating);
    }

    #[test]
    fn read_allowlist_matches_real_tree() {
        let tree = load_tree();
        let families = tree["families"].as_array().unwrap();

        for (fam, sub) in READ_ONLY_SUBCOMMANDS {
            let node = families
                .iter()
                .find(|f| f["name"].as_str() == Some(*fam))
                .unwrap_or_else(|| panic!("allowlist family '{fam}' not in tree"));
            let subs: Vec<&str> = node["subcommands"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| s["name"].as_str().unwrap())
                .collect();
            assert!(
                subs.contains(sub),
                "allowlist subcommand '{fam} {sub}' not in tree"
            );
        }

        for fam in READ_ONLY_LEAF_FAMILIES {
            assert!(
                families.iter().any(|f| f["name"].as_str() == Some(*fam)),
                "allowlist leaf family '{fam}' not in tree"
            );
        }
    }

    #[test]
    fn every_tree_subcommand_is_classified() {
        let tree = load_tree();
        for fam in tree["families"].as_array().unwrap() {
            let subs = fam["subcommands"].as_array().unwrap();
            for sub in subs {
                let kind = classify(
                    fam["name"].as_str().unwrap(),
                    Some(sub["name"].as_str().unwrap()),
                );
                assert!(kind == ToolKind::ReadOnly || kind == ToolKind::Mutating);
            }
        }
    }

    // ── validate_input (subconjunto JSON Schema) ──

    #[test]
    fn validate_input_accepts_valid_args() {
        let schema = json!({
            "type": "object",
            "properties": {"name": {"type": "string"}},
            "required": ["name"]
        });
        assert!(validate_input(&schema, &json!({"name": "foo"})).is_ok());
    }

    #[test]
    fn validate_input_rejects_missing_required() {
        let schema = json!({"type": "object", "required": ["name"]});
        let err = validate_input(&schema, &json!({})).unwrap_err();
        assert!(err.contains("missing required argument: name"));
    }

    #[test]
    fn validate_input_rejects_wrong_type() {
        let schema = json!({"type": "object", "properties": {"name": {"type": "string"}}});
        let err = validate_input(&schema, &json!({"name": 42})).unwrap_err();
        assert!(err.contains("must be of type string"));
    }

    #[test]
    fn validate_input_rejects_enum_violation() {
        let schema = json!({"type": "object", "properties": {"mode": {"enum": ["a", "b"]}}});
        let err = validate_input(&schema, &json!({"mode": "z"})).unwrap_err();
        assert!(err.contains("must be one of"));
    }

    #[test]
    fn validate_input_rejects_non_object_args() {
        let schema = json!({"type": "object"});
        assert!(validate_input(&schema, &json!(42)).is_err());
    }
}
