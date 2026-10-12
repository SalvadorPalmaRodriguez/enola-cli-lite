// MCP-004 — Tools de lectura de primera clase (mapeo 1:1 a casos de uso).
//
// Cada handler llama al caso de uso de `application/` y devuelve un resultado
// JSON estructurado (no una cadena formateada). Los handlers con dependencias
// externas reciben el PORT inyectado desde la composición (no importan adapters).

use crate::application::list_tor_services::ListTorServices;
use crate::application::mcp::registry::{ToolHandler, ToolRegistry};
use crate::domain::error::{EnolaError, Result};
use crate::domain::tool::{ToolKind, ToolSpec};
use crate::ports::tor::TorManagerPort;
use serde_json::{json, Value};
use std::sync::Arc;

/// Registra las tools de lectura de primera clase en el registry.
pub fn register(
    registry: &mut ToolRegistry,
    tor: Arc<dyn TorManagerPort + Send + Sync>,
) -> Result<()> {
    register_tool(
        registry,
        "tor_list",
        "List configured Tor hidden services.",
        json!({"type": "object"}),
        ToolKind::ReadOnly,
        Arc::new(TorListHandler { tor }),
    )?;
    register_tool(
        registry,
        "config_show",
        "Show the resolved configuration (flag > env > file > default).",
        json!({"type": "object"}),
        ToolKind::ReadOnly,
        Arc::new(ConfigShowHandler),
    )?;
    register_tool(
        registry,
        "doctor",
        "Run system diagnostics and return the full report.",
        json!({"type": "object"}),
        ToolKind::ReadOnly,
        Arc::new(DoctorHandler),
    )?;
    Ok(())
}

fn register_tool(
    registry: &mut ToolRegistry,
    name: &str,
    description: &str,
    schema: Value,
    kind: ToolKind,
    handler: Arc<dyn ToolHandler>,
) -> Result<()> {
    let spec =
        ToolSpec::new(name, description, schema, kind).map_err(EnolaError::ValidationError)?;
    registry.register(spec, handler)
}

struct TorListHandler {
    tor: Arc<dyn TorManagerPort + Send + Sync>,
}

#[async_trait::async_trait]
impl ToolHandler for TorListHandler {
    async fn call(&self, _args: Value) -> Result<Value> {
        let services = ListTorServices::new(self.tor.clone()).execute().await?;
        Ok(json!({ "services": services }))
    }
}

struct ConfigShowHandler;

#[async_trait::async_trait]
impl ToolHandler for ConfigShowHandler {
    async fn call(&self, _args: Value) -> Result<Value> {
        let entries = crate::application::config_inspector::resolve_all();
        Ok(json!({ "entries": entries }))
    }
}

struct DoctorHandler;

#[async_trait::async_trait]
impl ToolHandler for DoctorHandler {
    async fn call(&self, _args: Value) -> Result<Value> {
        let report = crate::application::system_doctor::full_report().await;
        Ok(json!({ "report": report }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::tor::{MockTorManagerPort, TorServiceInfo};

    #[tokio::test]
    async fn tor_list_returns_services() {
        let mut mock = MockTorManagerPort::new();
        mock.expect_list_hidden_services().returning(|| {
            Ok(vec![TorServiceInfo {
                name: "svc".into(),
                hostname: "abc.onion".into(),
                hidden_service_dir: "/var/lib/tor/svc".into(),
                ports: vec![(80, "127.0.0.1:8080".into())],
                clients: vec![],
                active: true,
                auth_enabled: false,
            }])
        });

        let handler = TorListHandler {
            tor: Arc::new(mock),
        };
        let out = handler.call(json!({})).await.unwrap();
        assert_eq!(out["services"][0]["name"], "svc");
        assert_eq!(out["services"][0]["hostname"], "abc.onion");
    }

    #[tokio::test]
    async fn config_show_returns_entries() {
        let handler = ConfigShowHandler;
        let out = handler.call(json!({})).await.unwrap();
        assert!(out["entries"].is_array());
    }

    #[test]
    fn register_adds_three_readonly_tools() {
        let mut registry = ToolRegistry::new();
        register(&mut registry, Arc::new(MockTorManagerPort::new())).unwrap();
        let tools = registry.list_tools();
        assert_eq!(tools.len(), 3);
        assert!(tools.iter().all(|t| t.kind == ToolKind::ReadOnly));
        assert!(tools.iter().any(|t| t.name == "tor_list"));
        assert!(tools.iter().any(|t| t.name == "config_show"));
        assert!(tools.iter().any(|t| t.name == "doctor"));
    }
}
