// MCP-004 — Catálogo de comandos (meta-tool `enola_list_commands`).
//
// Lee el árbol tipado generado por build.rs (assets/mcp_tools.json) y expone
// el descubrimiento de comandos: sin filtro → resumen de familias; por familia
// → sus subcomandos; por texto → coincidencias en nombre/descripción.

use crate::application::mcp::registry::{ToolHandler, ToolRegistry};
use crate::domain::error::{EnolaError, Result};
use crate::domain::tool::{ToolKind, ToolSpec};
use serde_json::{json, Value};
use std::sync::Arc;

const TREE_JSON: &str = include_str!("../../../assets/mcp_tools.json");

pub(crate) fn load_tree() -> Value {
    serde_json::from_str(TREE_JSON).unwrap_or_else(|_| json!({ "families": [] }))
}

/// Devuelve el descubrimiento de comandos según los filtros opcionales.
pub(crate) fn list_commands(family: Option<&str>, query: Option<&str>) -> Value {
    let tree = load_tree();
    let families = tree["families"].as_array().cloned().unwrap_or_default();

    if let Some(fam) = family {
        let subcommands = families
            .iter()
            .find(|f| f["name"].as_str() == Some(fam))
            .and_then(|f| f["subcommands"].as_array().cloned())
            .unwrap_or_default();
        return json!({ "family": fam, "subcommands": subcommands });
    }

    if let Some(q) = query {
        let q = q.to_lowercase();
        let mut matches: Vec<Value> = Vec::new();
        for fam in &families {
            // Recorrido recursivo por niveles (L2, L3…): el path
            // `parent child` (p.ej. "model list") es directamente usable
            // como `subcommand` de `enola_run_command` (Fase 11.0, R13).
            let mut stack: Vec<(String, Value)> = fam["subcommands"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .map(|s| (String::new(), s))
                .collect();
            while let Some((prefix, sub)) = stack.pop() {
                let name = sub["name"].as_str().unwrap_or("");
                let desc = sub["description"].as_str().unwrap_or("");
                let path = if prefix.is_empty() {
                    name.to_string()
                } else {
                    format!("{prefix} {name}")
                };
                if name.to_lowercase().contains(&q) || desc.to_lowercase().contains(&q) {
                    matches.push(json!({
                        "family": fam["name"],
                        "subcommand": path,
                        "description": desc,
                    }));
                }
                for child in sub["subcommands"].as_array().cloned().unwrap_or_default() {
                    stack.push((path.clone(), child));
                }
            }
        }
        return json!({ "matches": matches });
    }

    let summary: Vec<Value> = families
        .iter()
        .map(|f| {
            json!({
                "name": f["name"],
                "description": f["description"],
                "subcommand_count": f["subcommands"].as_array().map(|s| s.len()).unwrap_or(0),
            })
        })
        .collect();
    json!({ "families": summary })
}

struct ListCommandsHandler;

#[async_trait::async_trait]
impl ToolHandler for ListCommandsHandler {
    async fn call(&self, args: Value) -> Result<Value> {
        let family = args.get("family").and_then(|v| v.as_str());
        let query = args.get("query").and_then(|v| v.as_str());
        Ok(list_commands(family, query))
    }
}

/// Registra la meta-tool `enola_list_commands` en el registry.
pub(crate) fn register(registry: &mut ToolRegistry) -> Result<()> {
    let spec = ToolSpec::new(
        "enola_list_commands",
        "Discover the commands available to enola_run_command. Filter by family or text.",
        json!({
            "type": "object",
            "properties": {
                "family": {"type": "string", "maxLength": 64},
                "query": {"type": "string", "maxLength": 256}
            },
            "additionalProperties": false
        }),
        ToolKind::ReadOnly,
    )
    .map_err(EnolaError::ValidationError)?;

    registry.register(spec, Arc::new(ListCommandsHandler))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_without_filters_returns_families_summary() {
        let out = list_commands(None, None);
        let families = out["families"].as_array().unwrap();
        assert!(families.len() >= 30);
        assert!(families.iter().all(|f| f["name"].is_string()));
    }

    #[test]
    fn list_by_family_returns_its_subcommands() {
        let out = list_commands(Some("wp"), None);
        assert_eq!(out["family"], "wp");
        let subs = out["subcommands"].as_array().unwrap();
        assert!(subs.iter().any(|s| s["name"] == "create"));
    }

    #[test]
    fn list_by_text_returns_matches() {
        let out = list_commands(None, Some("publish"));
        let matches = out["matches"].as_array().unwrap();
        assert!(!matches.is_empty());
        // El término coincide por nombre Y por descripción: al menos los que
        // se llaman exactamente "publish" deben estar.
        assert!(matches.iter().any(|m| m["subcommand"] == "publish"));
    }

    #[test]
    fn list_by_text_finds_level3_subcommands() {
        // Fase 11.0: la búsqueda por texto debe recorrer los subcomandos
        // anidados (L3). `ai model list` se expone como path "model list",
        // directamente usable en `enola_run_command.subcommand`.
        let out = list_commands(None, Some("model"));
        let matches = out["matches"].as_array().unwrap();
        assert!(
            matches
                .iter()
                .any(|m| m["family"] == "ai" && m["subcommand"] == "model list"),
            "L3 path 'ai model list' must appear in text search: {matches:?}"
        );
    }

    #[test]
    fn list_unknown_family_returns_empty() {
        let out = list_commands(Some("bogus"), None);
        assert_eq!(out["family"], "bogus");
        assert_eq!(out["subcommands"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn register_adds_enola_list_commands() {
        let mut registry = ToolRegistry::new();
        register(&mut registry).unwrap();
        let tools = registry.list_tools();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "enola_list_commands");
    }
}
