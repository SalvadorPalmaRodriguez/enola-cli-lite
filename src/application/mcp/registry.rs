// MCP-004 — ToolRegistry: catálogo de tools + dispatch a handlers.
//
// El registry es la pieza compartida entre el servidor MCP y `ask` (Fase 9).
// Valida los argumentos contra el JSON Schema ANTES de invocar el handler, de
// modo que ningún caso de uso recibe entradas inválidas.

use crate::domain::error::{EnolaError, Result};
use crate::domain::tool::{validate_input, ToolSpec};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

/// Handler de una tool MCP: recibe los argumentos ya validados y devuelve un
/// resultado JSON estructurado.
#[async_trait::async_trait]
pub trait ToolHandler: Send + Sync {
    async fn call(&self, args: Value) -> Result<Value>;
}

struct RegisteredTool {
    spec: ToolSpec,
    handler: Arc<dyn ToolHandler>,
}

/// Catálogo de tools + dispatch a handlers.
#[derive(Default)]
pub struct ToolRegistry {
    tools: HashMap<String, RegisteredTool>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registra una tool. Rechaza nombres duplicados.
    pub fn register(&mut self, spec: ToolSpec, handler: Arc<dyn ToolHandler>) -> Result<()> {
        if self.tools.contains_key(&spec.name) {
            return Err(EnolaError::ValidationError(format!(
                "duplicate tool: {}",
                spec.name
            )));
        }
        self.tools
            .insert(spec.name.clone(), RegisteredTool { spec, handler });
        Ok(())
    }

    /// Devuelve las tools registradas, ordenadas por nombre.
    pub fn list_tools(&self) -> Vec<ToolSpec> {
        let mut specs: Vec<ToolSpec> = self.tools.values().map(|t| t.spec.clone()).collect();
        specs.sort_by(|a, b| a.name.cmp(&b.name));
        specs
    }

    /// Despacha `name` con `args`, validando antes de invocar el handler.
    pub async fn call(&self, name: &str, args: Value) -> Result<Value> {
        let tool = self
            .tools
            .get(name)
            .ok_or_else(|| EnolaError::NotFound(format!("unknown tool: {name}")))?;
        validate_input(&tool.spec.input_schema, &args).map_err(EnolaError::ValidationError)?;
        tool.handler.call(args).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::tool::ToolKind;
    use serde_json::json;
    use std::sync::Mutex;

    struct StubHandler {
        calls: Mutex<Vec<Value>>,
        result: Value,
    }

    #[async_trait::async_trait]
    impl ToolHandler for StubHandler {
        async fn call(&self, args: Value) -> Result<Value> {
            self.calls.lock().unwrap().push(args);
            Ok(self.result.clone())
        }
    }

    fn spec(name: &str) -> ToolSpec {
        ToolSpec::new(
            name,
            "desc",
            json!({
                "type": "object",
                "properties": {"name": {"type": "string"}},
                "required": ["name"]
            }),
            ToolKind::ReadOnly,
        )
        .unwrap()
    }

    fn stub(result: Value) -> Arc<StubHandler> {
        Arc::new(StubHandler {
            calls: Mutex::new(Vec::new()),
            result,
        })
    }

    #[tokio::test]
    async fn list_tools_returns_registered_tools_sorted() {
        let mut reg = ToolRegistry::new();
        reg.register(spec("b"), stub(json!({}))).unwrap();
        reg.register(spec("a"), stub(json!({}))).unwrap();
        let tools = reg.list_tools();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].name, "a");
        assert_eq!(tools[1].name, "b");
    }

    #[tokio::test]
    async fn register_duplicate_is_error() {
        let mut reg = ToolRegistry::new();
        reg.register(spec("a"), stub(json!({}))).unwrap();
        assert!(reg.register(spec("a"), stub(json!({}))).is_err());
    }

    #[tokio::test]
    async fn call_unknown_tool_is_error() {
        let reg = ToolRegistry::new();
        assert!(matches!(
            reg.call("nope", json!({})).await,
            Err(EnolaError::NotFound(_))
        ));
    }

    #[tokio::test]
    async fn call_invalid_args_does_not_invoke_handler() {
        let handler = stub(json!({}));
        let mut reg = ToolRegistry::new();
        reg.register(spec("a"), handler.clone()).unwrap();

        // Falta el argumento requerido "name" → error de validación, handler NO invocado.
        assert!(matches!(
            reg.call("a", json!({})).await,
            Err(EnolaError::ValidationError(_))
        ));
        assert!(handler.calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn call_valid_args_invokes_handler_with_args() {
        let handler = stub(json!({"ok": true}));
        let mut reg = ToolRegistry::new();
        reg.register(spec("a"), handler.clone()).unwrap();

        let out = reg.call("a", json!({"name": "foo"})).await.unwrap();
        assert_eq!(out, json!({"ok": true}));
        assert_eq!(*handler.calls.lock().unwrap(), vec![json!({"name": "foo"})]);
    }
}
