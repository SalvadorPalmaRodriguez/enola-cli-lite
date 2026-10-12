// MCP-004 — Tools de lectura de primera clase (mapeo 1:1 a casos de uso).
//
// Cada handler llama al caso de uso de `application/` y devuelve un resultado
// JSON estructurado (no una cadena formateada). Los handlers con dependencias
// externas reciben el PORT inyectado desde la composición (no importan adapters).

use crate::application::deploy_fileserver::{DeployFileServer, DeployFileServerRequest};
use crate::application::deploy_tor_service::{DeployTorService, DeployTorServiceRequest};
use crate::application::deploy_wordpress::DeployWordPress;
use crate::application::list_ports::ListPorts;
use crate::application::list_tor_services::ListTorServices;
use crate::application::list_wordpress_sites::ListWordPressSites;
use crate::application::mcp::registry::{ToolHandler, ToolRegistry};
use crate::application::nginx_status_checker::NginxStatusChecker;
use crate::application::plan_service::PlanService;
use crate::application::publish_wordpress::PublishWordPress;
use crate::application::remove_tor_service::RemoveTorService;
use crate::application::system_health_check::SystemHealthCheck;
use crate::application::verify_onion::VerifyOnion;
use crate::application::wordpress_status_check::WordPressStatusCheck;
use crate::domain::error::{EnolaError, Result};
use crate::domain::tool::{ToolKind, ToolSpec};
use crate::ports::container::ContainerPort;
use crate::ports::file::FileManagerPort;
use crate::ports::http_probe::HttpProbePort;
use crate::ports::manifest::ManifestPort;
use crate::ports::port_checker::PortCheckerPort;
use crate::ports::service::ServiceManagerPort;
use crate::ports::tor::TorManagerPort;
use crate::ports::web::NginxManagerPort;
use serde_json::{json, Value};
use std::sync::Arc;

/// Registra las tools de lectura de primera clase en el registry.
#[allow(clippy::too_many_arguments)]
pub fn register(
    registry: &mut ToolRegistry,
    tor: Arc<dyn TorManagerPort + Send + Sync>,
    port_checker: Arc<dyn PortCheckerPort>,
    container: Arc<dyn ContainerPort + Send + Sync>,
    service_manager: Arc<dyn ServiceManagerPort + Send + Sync>,
    nginx: Arc<dyn NginxManagerPort + Send + Sync>,
    probe: Arc<dyn HttpProbePort + Send + Sync>,
    manifest: Arc<dyn ManifestPort + Send + Sync>,
    file_manager: Arc<dyn FileManagerPort + Send + Sync>,
) -> Result<()> {
    let tor_handler: Arc<dyn ToolHandler> = Arc::new(TorListHandler { tor: tor.clone() });
    register_tool(
        registry,
        "tor_list",
        "List configured Tor hidden services.",
        json!({"type": "object"}),
        ToolKind::ReadOnly,
        tor_handler.clone(),
    )?;
    register_tool(
        registry,
        "diag_tor",
        "Tor hidden service status (same as tor_list).",
        json!({"type": "object"}),
        ToolKind::ReadOnly,
        tor_handler,
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
        "diag_nginx",
        "Nginx status (active, sites, HTTP check, version).",
        json!({"type": "object"}),
        ToolKind::ReadOnly,
        Arc::new(DiagNginxHandler {
            service_manager: service_manager.clone(),
            nginx: nginx.clone(),
        }),
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
        "wp_list",
        "List WordPress sites (containers, port, onion address).",
        json!({"type": "object"}),
        ToolKind::ReadOnly,
        Arc::new(WpListHandler {
            container: container.clone(),
        }),
    )?;
    register_tool(
        registry,
        "wp_status",
        "Status of a WordPress site (wp + db containers, HTTP, health).",
        json!({
            "type": "object",
            "properties": {"name": {"type": "string"}},
            "required": ["name"]
        }),
        ToolKind::ReadOnly,
        Arc::new(WpStatusHandler {
            container: container.clone(),
        }),
    )?;
    register_tool(
        registry,
        "ports_list",
        "List all host/virtual port bindings managed by enola-cli.",
        json!({"type": "object"}),
        ToolKind::ReadOnly,
        Arc::new(PortsListHandler {
            container: container.clone(),
        }),
    )?;
    register_tool(
        registry,
        "verify_onion",
        "Probe a .onion address through Tor (SOCKS5h): reachability, HTTP status, latency, failure reason.",
        json!({
            "type": "object",
            "properties": {"address": {"type": "string"}},
            "required": ["address"]
        }),
        ToolKind::ReadOnly,
        Arc::new(VerifyOnionHandler { probe }),
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
            checker: port_checker.clone(),
        }),
    )?;

    // ── Tools mutantes (Fase 7) — rechazadas por la política salvo
    //    --allow-write + allowlist. Se registran con ToolKind::Mutating. ──
    register_tool(
        registry,
        "wp_create",
        "Create a WordPress site (containers, network, secrets).",
        json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "http_port": {"type": "integer"}
            },
            "required": ["name"]
        }),
        ToolKind::Mutating,
        Arc::new(WpCreateHandler {
            container: container.clone(),
            tor: tor.clone(),
            manifest: manifest.clone(),
            checker: port_checker.clone(),
        }),
    )?;
    register_tool(
        registry,
        "wp_publish",
        "Publish an existing WordPress site as a Tor hidden service (returns onion_address).",
        json!({
            "type": "object",
            "properties": {"name": {"type": "string"}},
            "required": ["name"]
        }),
        ToolKind::Mutating,
        Arc::new(WpPublishHandler {
            container,
            tor: tor.clone(),
            service_manager: service_manager.clone(),
            manifest: manifest.clone(),
        }),
    )?;
    register_tool(
        registry,
        "wp_hide",
        "Remove the Tor hidden service of a WordPress site (requires force=true).",
        json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "force": {"type": "boolean"}
            },
            "required": ["name", "force"]
        }),
        ToolKind::Mutating,
        Arc::new(TorRemoveHandler {
            tor: tor.clone(),
            tool_label: "wp_hide",
        }),
    )?;
    register_tool(
        registry,
        "tor_create",
        "Create a Tor hidden service (virtual port → local target).",
        json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "virtual_port": {"type": "integer"},
                "target_port": {"type": "integer"}
            },
            "required": ["name", "virtual_port"]
        }),
        ToolKind::Mutating,
        Arc::new(TorCreateHandler {
            tor: tor.clone(),
            service_manager: service_manager.clone(),
            manifest: manifest.clone(),
        }),
    )?;
    register_tool(
        registry,
        "tor_remove",
        "Remove a Tor hidden service (requires force=true).",
        json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "force": {"type": "boolean"}
            },
            "required": ["name", "force"]
        }),
        ToolKind::Mutating,
        Arc::new(TorRemoveHandler {
            tor: tor.clone(),
            tool_label: "tor_remove",
        }),
    )?;
    register_tool(
        registry,
        "files_create",
        "Create a file sharing service over Tor (Nginx + hidden service).",
        json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "port": {"type": "integer"},
                "share_path": {"type": "string"},
                "enable_auth": {"type": "boolean"}
            },
            "required": ["name"]
        }),
        ToolKind::Mutating,
        Arc::new(FilesCreateHandler {
            nginx,
            tor,
            service_manager,
            file_manager,
            manifest,
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

struct DiagNginxHandler {
    service_manager: Arc<dyn ServiceManagerPort + Send + Sync>,
    nginx: Arc<dyn NginxManagerPort + Send + Sync>,
}

#[async_trait::async_trait]
impl ToolHandler for DiagNginxHandler {
    async fn call(&self, _args: Value) -> Result<Value> {
        let report = NginxStatusChecker::new(self.service_manager.clone(), self.nginx.clone())
            .execute()
            .await?;
        Ok(json!({ "report": report }))
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

struct WpListHandler {
    container: Arc<dyn ContainerPort + Send + Sync>,
}

#[async_trait::async_trait]
impl ToolHandler for WpListHandler {
    async fn call(&self, _args: Value) -> Result<Value> {
        let sites = ListWordPressSites::new(self.container.clone())
            .execute()
            .await?;
        Ok(json!({ "sites": sites }))
    }
}

struct PortsListHandler {
    container: Arc<dyn ContainerPort + Send + Sync>,
}

#[async_trait::async_trait]
impl ToolHandler for PortsListHandler {
    async fn call(&self, _args: Value) -> Result<Value> {
        let ports = ListPorts::new(self.container.clone()).execute().await?;
        Ok(json!({ "ports": ports }))
    }
}

struct VerifyOnionHandler {
    probe: Arc<dyn HttpProbePort + Send + Sync>,
}

#[async_trait::async_trait]
impl ToolHandler for VerifyOnionHandler {
    async fn call(&self, args: Value) -> Result<Value> {
        let address = required_string(&args, "address")?;
        let verification = VerifyOnion::new(self.probe.clone())
            .execute(address)
            .await?;
        Ok(serde_json::to_value(verification)
            .map_err(|e| EnolaError::Unknown(format!("verify_onion serialization: {e}")))?)
    }
}

struct WpStatusHandler {
    container: Arc<dyn ContainerPort + Send + Sync>,
}

#[async_trait::async_trait]
impl ToolHandler for WpStatusHandler {
    async fn call(&self, args: Value) -> Result<Value> {
        let name = required_string(&args, "name")?;
        let status = WordPressStatusCheck::new(self.container.clone())
            .execute(name)
            .await?;
        Ok(json!({ "status": status }))
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

// ═══════════════════════════════════════════════════════════════════════════
// Handlers mutantes (Fase 7) — solo alcanzables con --allow-write + allowlist.
// ═══════════════════════════════════════════════════════════════════════════

struct WpCreateHandler {
    container: Arc<dyn ContainerPort + Send + Sync>,
    tor: Arc<dyn TorManagerPort + Send + Sync>,
    manifest: Arc<dyn ManifestPort + Send + Sync>,
    checker: Arc<dyn PortCheckerPort>,
}

#[async_trait::async_trait]
impl ToolHandler for WpCreateHandler {
    async fn call(&self, args: Value) -> Result<Value> {
        let name = required_string(&args, "name")?;
        let http_port = match args.get("http_port").and_then(|v| v.as_u64()) {
            Some(p) => p as u16,
            None => self
                .checker
                .find_free_port(8080, 9000)
                .map_err(|e| EnolaError::InfrastructureError(e.to_string()))?,
        };
        let output = DeployWordPress::new(
            self.container.clone(),
            self.tor.clone(),
            self.manifest.clone(),
        )
        .execute(name, http_port)
        .await?;
        Ok(json!({ "name": name, "http_port": http_port, "output": output }))
    }
}

struct WpPublishHandler {
    container: Arc<dyn ContainerPort + Send + Sync>,
    tor: Arc<dyn TorManagerPort + Send + Sync>,
    service_manager: Arc<dyn ServiceManagerPort + Send + Sync>,
    manifest: Arc<dyn ManifestPort + Send + Sync>,
}

#[async_trait::async_trait]
impl ToolHandler for WpPublishHandler {
    async fn call(&self, args: Value) -> Result<Value> {
        let name = required_string(&args, "name")?;
        let result = PublishWordPress::new(
            self.container.clone(),
            self.tor.clone(),
            self.service_manager.clone(),
            self.manifest.clone(),
        )
        .execute(name)
        .await?;
        Ok(serde_json::to_value(result)
            .map_err(|e| EnolaError::Unknown(format!("wp_publish serialization: {e}")))?)
    }
}

/// Quita un hidden service (`RemoveTorService`). Destructivo: exige
/// `force: true` explícito en los argumentos (la tool nunca lo presupone).
struct TorRemoveHandler {
    tor: Arc<dyn TorManagerPort + Send + Sync>,
    tool_label: &'static str,
}

#[async_trait::async_trait]
impl ToolHandler for TorRemoveHandler {
    async fn call(&self, args: Value) -> Result<Value> {
        let name = required_string(&args, "name")?;
        if args.get("force").and_then(|v| v.as_bool()) != Some(true) {
            return Err(EnolaError::ValidationError(format!(
                "'{}' is destructive; pass \"force\": true explicitly",
                self.tool_label
            )));
        }
        let nginx: Option<Arc<dyn NginxManagerPort + Send + Sync>> = None;
        RemoveTorService::new(self.tor.clone(), nginx)
            .execute(name)
            .await?;
        Ok(json!({ "name": name, "removed": true }))
    }
}

struct TorCreateHandler {
    tor: Arc<dyn TorManagerPort + Send + Sync>,
    service_manager: Arc<dyn ServiceManagerPort + Send + Sync>,
    manifest: Arc<dyn ManifestPort + Send + Sync>,
}

#[async_trait::async_trait]
impl ToolHandler for TorCreateHandler {
    async fn call(&self, args: Value) -> Result<Value> {
        let name = required_string(&args, "name")?;
        let virtual_port = args
            .get("virtual_port")
            .and_then(|v| v.as_u64())
            .map(|p| p as u16)
            .ok_or_else(|| EnolaError::ValidationError("missing 'virtual_port'".into()))?;
        let target_port = args
            .get("target_port")
            .and_then(|v| v.as_u64())
            .map(|p| p as u16)
            .unwrap_or(virtual_port);
        let onion = DeployTorService::new(
            self.tor.clone(),
            self.service_manager.clone(),
            self.manifest.clone(),
        )
        .execute(DeployTorServiceRequest {
            service_name: name.to_string(),
            ports: vec![(virtual_port, target_port)],
        })
        .await?;
        Ok(json!({ "name": name, "onion_address": onion }))
    }
}

struct FilesCreateHandler {
    nginx: Arc<dyn NginxManagerPort + Send + Sync>,
    tor: Arc<dyn TorManagerPort + Send + Sync>,
    service_manager: Arc<dyn ServiceManagerPort + Send + Sync>,
    file_manager: Arc<dyn FileManagerPort + Send + Sync>,
    manifest: Arc<dyn ManifestPort + Send + Sync>,
}

#[async_trait::async_trait]
impl ToolHandler for FilesCreateHandler {
    async fn call(&self, args: Value) -> Result<Value> {
        let name = required_string(&args, "name")?;
        let (onion, share_path) = DeployFileServer::new(
            self.nginx.clone(),
            self.tor.clone(),
            self.service_manager.clone(),
            self.file_manager.clone(),
            self.manifest.clone(),
        )
        .execute(DeployFileServerRequest {
            service_name: name.to_string(),
            port: args
                .get("port")
                .and_then(|v| v.as_u64())
                .map(|p| p as u16)
                .unwrap_or(8080),
            share_path: args
                .get("share_path")
                .and_then(|v| v.as_str())
                .map(String::from),
            enable_auth: args
                .get("enable_auth")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
        })
        .await?;
        Ok(json!({ "name": name, "onion_address": onion, "share_path": share_path }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::container::MockContainerPort;
    use crate::ports::port_checker::{MockPortCheckerPort, PortCheckResult};
    use crate::ports::service::MockServiceManagerPort;
    use crate::ports::tor::{MockTorManagerPort, TorServiceInfo};
    use crate::ports::web::MockNginxManagerPort;

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
    fn register_adds_thirteen_readonly_and_six_mutating_tools() {
        let mut registry = ToolRegistry::new();
        register(
            &mut registry,
            Arc::new(MockTorManagerPort::new()),
            Arc::new(MockPortCheckerPort::new()),
            Arc::new(MockContainerPort::new()),
            Arc::new(MockServiceManagerPort::new()),
            Arc::new(MockNginxManagerPort::new()),
            Arc::new(crate::ports::http_probe::MockHttpProbePort::new()),
            Arc::new(crate::ports::manifest::MockManifestPort::new()),
            Arc::new(crate::ports::file::MockFileManagerPort::new()),
        )
        .unwrap();
        let tools = registry.list_tools();
        assert_eq!(tools.len(), 19);
        let readonly: Vec<_> = tools
            .iter()
            .filter(|t| t.kind == ToolKind::ReadOnly)
            .collect();
        let mutating: Vec<_> = tools
            .iter()
            .filter(|t| t.kind == ToolKind::Mutating)
            .collect();
        assert_eq!(readonly.len(), 13);
        assert_eq!(mutating.len(), 6);
        for name in [
            "tor_list",
            "diag_tor",
            "config_show",
            "doctor",
            "diag_summary",
            "diag_nginx",
            "update_check",
            "wp_list",
            "wp_status",
            "ports_list",
            "verify_onion",
            "plan_wp_create",
            "plan_tor_create",
        ] {
            assert!(tools.iter().any(|t| t.name == name), "missing {name}");
        }
        for name in [
            "wp_create",
            "wp_publish",
            "wp_hide",
            "tor_create",
            "tor_remove",
            "files_create",
        ] {
            assert!(mutating.iter().any(|t| t.name == name), "missing {name}");
        }
    }

    /// MCP-007 (7.5) — toda tool mutante con equivalente `plan` debe exponer
    /// su dry-run de lectura. Falla si se añade una mutante planificable sin
    /// registrar su `plan_*`.
    #[test]
    fn mutating_tools_with_plan_expose_dry_run() {
        let mut registry = ToolRegistry::new();
        register(
            &mut registry,
            Arc::new(MockTorManagerPort::new()),
            Arc::new(MockPortCheckerPort::new()),
            Arc::new(MockContainerPort::new()),
            Arc::new(MockServiceManagerPort::new()),
            Arc::new(MockNginxManagerPort::new()),
            Arc::new(crate::ports::http_probe::MockHttpProbePort::new()),
            Arc::new(crate::ports::manifest::MockManifestPort::new()),
            Arc::new(crate::ports::file::MockFileManagerPort::new()),
        )
        .unwrap();
        let tools = registry.list_tools();
        // mutante → su plan de lectura esperado (PlanCommands: wp/tor/git create)
        for (mutating, plan) in [
            ("wp_create", "plan_wp_create"),
            ("tor_create", "plan_tor_create"),
        ] {
            assert!(tools.iter().any(|t| t.name == mutating));
            assert!(
                tools.iter().any(|t| t.name == plan),
                "mutating '{mutating}' must expose '{plan}' dry-run"
            );
        }
    }

    /// MCP-007 (7.3) — handlers mutantes: una prueba por tool con ports mockeados.
    #[tokio::test]
    async fn tor_remove_requires_explicit_force() {
        let tor = MockTorManagerPort::new(); // no llamada esperada sin force
        let h = TorRemoveHandler {
            tor: Arc::new(tor),
            tool_label: "tor_remove",
        };
        let r = h.call(json!({"name": "svc"})).await;
        assert!(r.is_err());
        assert!(r.unwrap_err().to_string().contains("force"));
    }

    #[tokio::test]
    async fn tor_remove_with_force_calls_use_case() {
        let mut tor = MockTorManagerPort::new();
        tor.expect_remove_hidden_service()
            .withf(|name| name == "svc")
            .returning(|_| Ok(()));
        let h = TorRemoveHandler {
            tor: Arc::new(tor),
            tool_label: "tor_remove",
        };
        let r = h.call(json!({"name": "svc", "force": true})).await.unwrap();
        assert_eq!(r["removed"], true);
    }

    #[tokio::test]
    async fn tor_create_requires_virtual_port() {
        let h = TorCreateHandler {
            tor: Arc::new(MockTorManagerPort::new()),
            service_manager: Arc::new(MockServiceManagerPort::new()),
            manifest: Arc::new(crate::ports::manifest::MockManifestPort::new()),
        };
        assert!(h.call(json!({"name": "svc"})).await.is_err());
    }

    #[tokio::test]
    async fn tor_create_deploys_hidden_service() {
        let mut tor = MockTorManagerPort::new();
        tor.expect_deploy_hidden_service()
            .withf(|name, ports| name == "svc" && *ports == vec![(80u16, 8080u16)])
            .returning(|_, _| Ok("xyz.onion".to_string()));
        let mut sm = MockServiceManagerPort::new();
        sm.expect_is_active().returning(|_| Ok(true));
        let mut manifest = crate::ports::manifest::MockManifestPort::new();
        manifest.expect_append().returning(|_, _| Ok(())).times(0..);
        let h = TorCreateHandler {
            tor: Arc::new(tor),
            service_manager: Arc::new(sm),
            manifest: Arc::new(manifest),
        };
        let r = h
            .call(json!({"name": "svc", "virtual_port": 80, "target_port": 8080}))
            .await
            .unwrap();
        assert_eq!(r["onion_address"], "xyz.onion");
    }

    #[tokio::test]
    async fn wp_create_requires_name() {
        let h = WpCreateHandler {
            container: Arc::new(MockContainerPort::new()),
            tor: Arc::new(MockTorManagerPort::new()),
            manifest: Arc::new(crate::ports::manifest::MockManifestPort::new()),
            checker: Arc::new(MockPortCheckerPort::new()),
        };
        assert!(h.call(json!({})).await.is_err());
    }

    #[tokio::test]
    async fn wp_hide_requires_force() {
        let tor = MockTorManagerPort::new();
        let h = TorRemoveHandler {
            tor: Arc::new(tor),
            tool_label: "wp_hide",
        };
        assert!(h.call(json!({"name": "site"})).await.is_err());
    }

    #[tokio::test]
    async fn files_create_requires_name() {
        let h = FilesCreateHandler {
            nginx: Arc::new(MockNginxManagerPort::new()),
            tor: Arc::new(MockTorManagerPort::new()),
            service_manager: Arc::new(MockServiceManagerPort::new()),
            file_manager: Arc::new(crate::ports::file::MockFileManagerPort::new()),
            manifest: Arc::new(crate::ports::manifest::MockManifestPort::new()),
        };
        assert!(h.call(json!({})).await.is_err());
    }

    #[tokio::test]
    async fn wp_publish_reports_unknown_site() {
        let mut container = MockContainerPort::new();
        container.expect_list_containers().returning(|_| Ok(vec![]));
        let h = WpPublishHandler {
            container: Arc::new(container),
            tor: Arc::new(MockTorManagerPort::new()),
            service_manager: Arc::new(MockServiceManagerPort::new()),
            manifest: Arc::new(crate::ports::manifest::MockManifestPort::new()),
        };
        assert!(h.call(json!({"name": "ghost"})).await.is_err());
    }

    /// MCP-004 (4.8) — coherencia schemas de tools ↔ árbol clap generado.
    ///
    /// Anti-deriva: cada propiedad del `inputSchema` de una tool debe existir
    /// como flag del subcomando clap equivalente, y todo flag `required` de
    /// clap debe estar en `required` del schema. Falla si se renombra un flag
    /// en `defs.rs` sin actualizar el schema de la tool.
    #[test]
    fn tool_schemas_match_clap_tree() {
        use crate::application::mcp::command_catalog::load_tree;

        fn leaf<'a>(tree: &'a Value, path: &[&str]) -> &'a Value {
            let mut node = tree["families"]
                .as_array()
                .and_then(|f| f.iter().find(|f| f["name"].as_str() == Some(path[0])))
                .unwrap_or_else(|| panic!("family '{}' missing from tree", path[0]));
            for tok in &path[1..] {
                node = node["subcommands"]
                    .as_array()
                    .and_then(|s| s.iter().find(|s| s["name"].as_str() == Some(*tok)))
                    .unwrap_or_else(|| panic!("'{}' missing under '{}'", tok, path[0]));
            }
            node
        }

        // tool → ruta en el árbol clap
        let mapping: &[(&str, &[&str])] = &[
            ("tor_list", &["tor", "list"]),
            ("diag_tor", &["diag", "tor"]),
            ("config_show", &["config-show"]),
            ("doctor", &["doctor"]),
            ("diag_summary", &["diag", "summary"]),
            ("diag_nginx", &["diag", "nginx"]),
            ("update_check", &["update", "check"]),
            ("wp_list", &["wp", "list"]),
            ("wp_status", &["wp", "status"]),
            ("ports_list", &["ports", "list"]),
            ("verify_onion", &["diag", "onion"]),
            ("plan_wp_create", &["plan", "wp", "create"]),
            ("plan_tor_create", &["plan", "tor", "create"]),
        ];

        let mut registry = ToolRegistry::new();
        register(
            &mut registry,
            Arc::new(MockTorManagerPort::new()),
            Arc::new(MockPortCheckerPort::new()),
            Arc::new(MockContainerPort::new()),
            Arc::new(MockServiceManagerPort::new()),
            Arc::new(MockNginxManagerPort::new()),
            Arc::new(crate::ports::http_probe::MockHttpProbePort::new()),
            Arc::new(crate::ports::manifest::MockManifestPort::new()),
            Arc::new(crate::ports::file::MockFileManagerPort::new()),
        )
        .unwrap();
        let tools = registry.list_tools();
        let tree = load_tree();

        for (tool_name, path) in mapping {
            let spec = tools
                .iter()
                .find(|t| t.name == *tool_name)
                .unwrap_or_else(|| panic!("tool '{tool_name}' not registered"));
            let node = leaf(&tree, path);
            let clap_flags: Vec<&str> = node["flags"]
                .as_array()
                .map(|f| f.iter().filter_map(|x| x["name"].as_str()).collect())
                .unwrap_or_default();

            // Toda propiedad del schema existe como flag clap (los args MCP
            // usan snake_case; las flags clap, kebab-case → se normaliza).
            if let Some(props) = spec.input_schema["properties"].as_object() {
                for key in props.keys() {
                    let flag = key.replace('_', "-");
                    assert!(
                        clap_flags.contains(&flag.as_str()),
                        "tool '{tool_name}' schema has '{key}' but '{}' has no --{flag} flag",
                        path.join(" ")
                    );
                }
            }
            // Todo flag required de clap está en required del schema.
            let schema_required: Vec<&str> = spec.input_schema["required"]
                .as_array()
                .map(|r| r.iter().filter_map(|v| v.as_str()).collect())
                .unwrap_or_default();
            for f in node["flags"].as_array().cloned().unwrap_or_default() {
                if f["required"].as_bool() == Some(true) {
                    let name = f["name"].as_str().unwrap_or("");
                    assert!(
                        schema_required.contains(&name),
                        "tool '{tool_name}' misses required flag '--{name}' ('{}' requires it)",
                        path.join(" ")
                    );
                }
            }
        }
    }
}
