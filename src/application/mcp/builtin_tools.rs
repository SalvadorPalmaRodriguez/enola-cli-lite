// MCP-004 — Tools de lectura de primera clase (mapeo 1:1 a casos de uso).
//
// Cada handler llama al caso de uso de `application/` y devuelve un resultado
// JSON estructurado (no una cadena formateada). Los handlers con dependencias
// externas reciben el PORT inyectado desde la composición (no importan adapters).

use crate::application::list_tor_services::ListTorServices;
use crate::application::mcp::registry::{ToolHandler, ToolRegistry};
use crate::application::plan_service::PlanService;
use crate::application::system_health_check::SystemHealthCheck;
use crate::domain::error::{EnolaError, Result};
use crate::domain::tool::{ToolKind, ToolSpec};
use crate::ports::port_checker::PortCheckerPort;
use crate::ports::tor::TorManagerPort;
use serde_json::{json, Value};
use std::sync::Arc;

/// Registra las tools de lectura de primera clase en el registry.
pub fn register(
    registry: &mut ToolRegistry,
    tor: Arc<dyn TorManagerPort + Send + Sync>,
    port_checker: Arc<dyn PortCheckerPort>,
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
    register_tool(
        registry,
        "diag_summary",
        "System resource health (CPU, memory, disk).",
        json!({"type": "object"}),
        ToolKind::ReadOnly,
        Arc::new(DiagSummaryHandler),
    )?;
    register_tool(
        registry,
        "update_check",
        "Check for available updates and security advisories.",
        json!({"type": "object"}),
        ToolKind::ReadOnly,
        Arc::new(UpdateCheckHandler),
    )?;
    register_tool(
        registry,
        "plan_wp_create",
        "Dry-run a WordPress site creation (0 side effects).",
        json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "http_port": {"type": "integer"}
            },
            "required": ["name"]
        }),
        ToolKind::ReadOnly,
        Arc::new(PlanWpCreateHandler {
            checker: port_checker.clone(),
        }),
    )?;
    register_tool(
        registry,
        "plan_tor_create",
        "Dry-run a Tor hidden service creation (0 side effects).",
        json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "service_type": {"type": "string"},
                "virtual_port": {"type": "integer"},
                "target_port": {"type": "integer"},
                "ssl": {"type": "boolean"}
            },
            "required": ["name", "service_type", "virtual_port"]
        }),
        ToolKind::ReadOnly,
        Arc::new(PlanTorCreateHandler {
            checker: port_checker,
        }),
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

fn required_string<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| EnolaError::ValidationError(format!("missing '{key}'")))
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

struct DiagSummaryHandler;

#[async_trait::async_trait]
impl ToolHandler for DiagSummaryHandler {
    async fn call(&self, _args: Value) -> Result<Value> {
        let health = SystemHealthCheck::new().execute().await?;
        Ok(json!({ "health": health }))
    }
}

struct UpdateCheckHandler;

#[async_trait::async_trait]
impl ToolHandler for UpdateCheckHandler {
    async fn call(&self, _args: Value) -> Result<Value> {
        let url = crate::application::update_checker::feed_url();
        let report = crate::application::update_checker::check_for_updates(&url).await;
        Ok(json!({ "report": report }))
    }
}

struct PlanWpCreateHandler {
    checker: Arc<dyn PortCheckerPort>,
}

#[async_trait::async_trait]
impl ToolHandler for PlanWpCreateHandler {
    async fn call(&self, args: Value) -> Result<Value> {
        let name = required_string(&args, "name")?;
        let http_port = args
            .get("http_port")
            .and_then(|v| v.as_u64())
            .map(|p| p as u16);
        let plan = PlanService::new(self.checker.clone()).plan_wordpress(name, http_port)?;
        Ok(json!({ "plan": plan }))
    }
}

struct PlanTorCreateHandler {
    checker: Arc<dyn PortCheckerPort>,
}

#[async_trait::async_trait]
impl ToolHandler for PlanTorCreateHandler {
    async fn call(&self, args: Value) -> Result<Value> {
        let name = required_string(&args, "name")?;
        let service_type = required_string(&args, "service_type")?;
        let virtual_port = args
            .get("virtual_port")
            .and_then(|v| v.as_u64())
            .map(|p| p as u16)
            .ok_or_else(|| EnolaError::ValidationError("missing 'virtual_port'".into()))?;
        let target_port = args
            .get("target_port")
            .and_then(|v| v.as_u64())
            .map(|p| p as u16);
        let ssl = args.get("ssl").and_then(|v| v.as_bool()).unwrap_or(false);
        let plan = PlanService::new(self.checker.clone()).plan_tor(
            name,
            service_type,
            virtual_port,
            target_port,
            ssl,
        )?;
        Ok(json!({ "plan": plan }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::port_checker::{MockPortCheckerPort, PortCheckResult};
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

    #[tokio::test]
    async fn plan_wp_create_returns_plan() {
        let mut mock = MockPortCheckerPort::new();
        mock.expect_find_free_port().returning(|_, _| Ok(8080));
        mock.expect_check_port().returning(|port| {
            Ok(PortCheckResult {
                port,
                free_os: true,
                free_docker: true,
            })
        });
        let handler = PlanWpCreateHandler {
            checker: Arc::new(mock),
        };
        let out = handler.call(json!({"name": "blog"})).await.unwrap();
        assert!(out["plan"].is_object());
    }

    #[tokio::test]
    async fn plan_wp_create_requires_name() {
        let mock = MockPortCheckerPort::new();
        let handler = PlanWpCreateHandler {
            checker: Arc::new(mock),
        };
        assert!(handler.call(json!({})).await.is_err());
    }

    #[test]
    fn register_adds_seven_readonly_tools() {
        let mut registry = ToolRegistry::new();
        register(
            &mut registry,
            Arc::new(MockTorManagerPort::new()),
            Arc::new(MockPortCheckerPort::new()),
        )
        .unwrap();
        let tools = registry.list_tools();
        assert_eq!(tools.len(), 7);
        assert!(tools.iter().all(|t| t.kind == ToolKind::ReadOnly));
        for name in [
            "tor_list",
            "config_show",
            "doctor",
            "diag_summary",
            "update_check",
            "plan_wp_create",
            "plan_tor_create",
        ] {
            assert!(tools.iter().any(|t| t.name == name), "missing {name}");
        }
    }
}
