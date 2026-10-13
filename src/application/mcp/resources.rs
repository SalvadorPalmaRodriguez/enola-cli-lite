// MCP-005 — Resources: documentación de usuario embebida.
//
// Expone el catálogo cerrado `domain::docs::DOC_CATALOG` vía JSON-RPC:
// `resources/list` → [{uri, name, description, mimeType}]
// `resources/read` → {contents: [{uri, mimeType, text}]}
// La resolución de URIs es estricta: solo `enola://docs/<path>` del catálogo
// (sin traversal — `doc_by_uri` solo hace match exacto sobre paths fijos).

use crate::application::mcp::registry::ToolHandler;
use crate::domain::docs::{doc_by_uri, doc_title, search_docs, DOC_CATALOG};
use crate::domain::error::{EnolaError, Result};
use serde_json::{json, Value};
use std::sync::Arc;

/// Resultado de `resources/list`: todos los documentos embebidos.
pub(crate) fn list_resources() -> Value {
    let resources: Vec<Value> = DOC_CATALOG
        .iter()
        .map(|d| {
            json!({
                "uri": d.uri(),
                "name": d.path,
                "description": doc_title(d),
                "mimeType": "text/markdown",
            })
        })
        .collect();
    json!({ "resources": resources })
}

/// Resultado de `resources/read` para una URI `enola://docs/<path>`.
///
/// Solo resuelve URIs del catálogo cerrado; cualquier otra (incluido
/// traversal `..`) devuelve `EnolaError::NotFound`/`ValidationError`.
pub(crate) fn read_resource(uri: &str) -> Result<Value> {
    if uri.is_empty() {
        return Err(EnolaError::ValidationError("missing 'uri'".into()));
    }
    let doc =
        doc_by_uri(uri).ok_or_else(|| EnolaError::NotFound(format!("unknown resource '{uri}'")))?;
    Ok(json!({
        "contents": [{
            "uri": doc.uri(),
            "mimeType": "text/markdown",
            "text": doc.content,
        }]
    }))
}

/// Tool `docs_search`: búsqueda de texto sobre todo el catálogo.
struct DocsSearchHandler;

#[async_trait::async_trait]
impl ToolHandler for DocsSearchHandler {
    async fn call(&self, args: Value) -> Result<Value> {
        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EnolaError::ValidationError("missing 'query'".into()))?;
        let matches: Vec<Value> = search_docs(query)
            .iter()
            .map(|(doc, excerpt)| {
                json!({
                    "uri": doc.uri(),
                    "doc": doc.path,
                    "excerpt": excerpt,
                })
            })
            .collect();
        Ok(json!({ "query": query, "matches": matches }))
    }
}

/// Registra la tool `docs_search` en el registry.
pub(crate) fn register_search_tool(
    registry: &mut crate::application::mcp::registry::ToolRegistry,
) -> Result<()> {
    use crate::domain::tool::{ToolKind, ToolSpec};
    let spec = ToolSpec::new(
        "docs_search",
        "Search the embedded user documentation (all enola-cli docs).",
        json!({
            "type": "object",
            "properties": {"query": {"type": "string"}},
            "required": ["query"]
        }),
        ToolKind::ReadOnly,
    )
    .map_err(EnolaError::ValidationError)?;
    registry.register(spec, Arc::new(DocsSearchHandler))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_resources_exposes_every_doc() {
        let out = list_resources();
        let resources = out["resources"].as_array().unwrap();
        assert_eq!(resources.len(), DOC_CATALOG.len());
        assert!(resources.iter().all(|r| r["mimeType"] == "text/markdown"));
        assert!(resources
            .iter()
            .any(|r| r["uri"] == "enola://docs/wp/commands-wp.md"));
    }

    #[test]
    fn read_resource_returns_markdown() {
        let out = read_resource("enola://docs/wp/commands-wp.md").unwrap();
        let text = out["contents"][0]["text"].as_str().unwrap();
        assert!(text.len() > 100);
        assert_eq!(out["contents"][0]["mimeType"], "text/markdown");
    }

    #[test]
    fn read_resource_rejects_traversal_and_unknown() {
        assert!(read_resource("enola://docs/../Cargo.toml").is_err());
        assert!(read_resource("enola://docs/nope.md").is_err());
        assert!(read_resource("file:///etc/passwd").is_err());
        assert!(read_resource("").is_err());
    }

    #[tokio::test]
    async fn docs_search_returns_matching_docs() {
        let h = DocsSearchHandler;
        let out = h.call(json!({"query": "onion"})).await.unwrap();
        assert!(!out["matches"].as_array().unwrap().is_empty());
        assert!(h.call(json!({})).await.is_err());
    }
}
