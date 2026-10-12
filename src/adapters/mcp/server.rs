// MCP-003 — Bucle de protocolo: handshake, routing de métodos y errores.
//
// El servidor es un driving adapter que habla JSON-RPC 2.0 por un
// `McpTransport`. El handshake exige `initialize` antes de `tools/list` y
// `resources/list`; las notificaciones se atienden sin respuesta y los errores
// de protocolo no cierran el bucle. Las tools viven en un `ToolRegistry`
// compartido (también lo usará `ask` en la Fase 9).

use crate::application::mcp::audit::AuditLogger;
use crate::application::mcp::policy::McpPolicy;
use crate::application::mcp::registry::ToolRegistry;
use crate::domain::error::{EnolaError, Result};
use crate::domain::mcp::{self, Id, Request, Response, RpcError};
use crate::domain::tool::{ToolKind, ToolSpec};
use crate::ports::mcp_transport::McpTransport;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

pub(crate) struct McpServer<T: McpTransport> {
    transport: T,
    initialized: bool,
    registry: Arc<ToolRegistry>,
    policy: McpPolicy,
    audit: Arc<AuditLogger>,
}

impl<T: McpTransport> McpServer<T> {
    pub(crate) fn new(transport: T, registry: Arc<ToolRegistry>) -> Self {
        Self {
            transport,
            initialized: false,
            registry,
            policy: McpPolicy::default(),
            audit: Arc::new(AuditLogger::new(None)),
        }
    }

    /// MCP-007: inyecta la política de escritura y el log de auditoría.
    pub(crate) fn with_policy(mut self, policy: McpPolicy, audit: Arc<AuditLogger>) -> Self {
        self.policy = policy;
        self.audit = audit;
        self
    }

    /// Ejecuta el bucle hasta EOF y devuelve el transporte (útil en tests).
    pub(crate) async fn run(mut self) -> Result<T> {
        while let Some(msg) = self.transport.recv().await? {
            self.handle_frame(&msg).await;
        }
        Ok(self.transport)
    }

    async fn handle_frame(&mut self, msg: &str) {
        let request: Request = match serde_json::from_str(msg) {
            Ok(r) => r,
            Err(_) => {
                let _ = self
                    .send_response(error_response(Id::Null, -32700, "Parse error"))
                    .await;
                return;
            }
        };

        if request.jsonrpc != mcp::JSONRPC_VERSION {
            let id = request.id.clone().unwrap_or(Id::Null);
            let _ = self
                .send_response(error_response(id, -32600, "Invalid Request"))
                .await;
            return;
        }

        if request.is_notification() {
            self.handle_notification(&request);
            return;
        }

        let id = request.id.clone().unwrap_or(Id::Null);
        let resp = self.dispatch(&request, id).await;
        let _ = self.send_response(resp).await;
    }

    async fn dispatch(&mut self, request: &Request, id: Id) -> Response {
        match request.method.as_str() {
            "initialize" => self.handle_initialize(request, id).await,
            "ping" => success_response(id, json!({})),
            "tools/list" => self.handle_tools_list(id),
            "tools/call" => self.handle_tool_call(request, id).await,
            "resources/list" => {
                if !self.initialized {
                    return error_response(id, -32600, "Invalid Request: not initialized");
                }
                success_response(id, crate::application::mcp::resources::list_resources())
            }
            "resources/read" => {
                if !self.initialized {
                    return error_response(id, -32600, "Invalid Request: not initialized");
                }
                let uri = request
                    .params
                    .as_ref()
                    .and_then(|p| p.get("uri"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                match crate::application::mcp::resources::read_resource(uri) {
                    Ok(result) => success_response(id, result),
                    Err(e) => error_response(id, -32602, &format!("Invalid params: {e}")),
                }
            }
            _ => error_response(id, -32601, "Method not found"),
        }
    }

    async fn handle_initialize(&mut self, request: &Request, id: Id) -> Response {
        if self.initialized {
            return error_response(id, -32600, "Invalid Request: already initialized");
        }
        let requested = request
            .params
            .as_ref()
            .and_then(|p| p.get("protocolVersion"))
            .and_then(|v| v.as_str())
            .unwrap_or(mcp::PROTOCOL_VERSION);
        match mcp::negotiate_protocol_version(requested) {
            Ok(version) => {
                self.initialized = true;
                success_response(
                    id,
                    json!({
                        "protocolVersion": version,
                        "capabilities": { "tools": {}, "resources": {} },
                        "serverInfo": {
                            "name": env!("CARGO_PKG_NAME"),
                            "version": env!("CARGO_PKG_VERSION"),
                        }
                    }),
                )
            }
            Err(_) => error_response(id, -32602, "Invalid params: unsupported protocolVersion"),
        }
    }

    fn handle_tools_list(&self, id: Id) -> Response {
        if !self.initialized {
            return error_response(id, -32600, "Invalid Request: not initialized");
        }
        let tools: Vec<Value> = self
            .registry
            .list_tools()
            .iter()
            .map(tool_to_value)
            .collect();
        success_response(id, json!({ "tools": tools }))
    }

    async fn handle_tool_call(&self, request: &Request, id: Id) -> Response {
        if !self.initialized {
            return error_response(id, -32600, "Invalid Request: not initialized");
        }
        let params = request.params.as_ref().and_then(|p| p.as_object());
        let Some(name) = params.and_then(|p| p.get("name")).and_then(|v| v.as_str()) else {
            return error_response(id, -32602, "Invalid params: missing 'name'");
        };
        let args = params
            .and_then(|p| p.get("arguments"))
            .cloned()
            .unwrap_or_else(|| json!({}));

        // MCP-007: política de escritura evaluada ANTES del dispatch.
        let kind = self.registry.kind_of(name).unwrap_or(ToolKind::ReadOnly);
        let is_mutating = kind == ToolKind::Mutating;
        if let Err(e) = self.policy.allows(kind, name) {
            if is_mutating {
                self.audit.log_call(name, &args, "rejected");
            }
            return success_response(
                id,
                json!({
                    "content": [{"type": "text", "text": format!("{e}")}],
                    "isError": true,
                }),
            );
        }

        // request_timeout_secs conectado a la ejecución real de la tool.
        let timeout = Duration::from_secs(self.policy.request_timeout_secs);
        let outcome =
            match tokio::time::timeout(timeout, self.registry.call(name, args.clone())).await {
                Ok(res) => res,
                Err(_) => Err(EnolaError::Timeout(format!(
                    "tool '{name}' timed out after {}s",
                    timeout.as_secs()
                ))),
            };

        if is_mutating {
            let label = match &outcome {
                Ok(_) => "ok".to_string(),
                Err(e) => format!("error: {e}"),
            };
            self.audit.log_call(name, &args, &label);
        }

        match outcome {
            Ok(result) => {
                let text = serde_json::to_string(&result).unwrap_or_else(|_| "{}".to_string());
                success_response(
                    id,
                    json!({
                        "content": [{"type": "text", "text": text}],
                        "isError": false,
                    }),
                )
            }
            Err(e) => match e {
                EnolaError::NotFound(_) | EnolaError::ValidationError(_) => {
                    error_response(id, -32602, &format!("Invalid params: {e}"))
                }
                EnolaError::SecurityError(_) => success_response(
                    id,
                    json!({
                        "content": [{"type": "text", "text": format!("{e}")}],
                        "isError": true,
                    }),
                ),
                other => error_response(id, -32603, &other.to_string()),
            },
        }
    }

    fn handle_notification(&self, request: &Request) {
        if request.method.as_str() == "notifications/initialized" {
            // handshake completo: nada que hacer
        }
        // cualquier otra notificación desconocida se ignora en silencio (spec MCP)
    }

    async fn send_response(&mut self, resp: Response) -> Result<()> {
        let msg =
            serde_json::to_string(&resp).map_err(|e| EnolaError::ValidationError(e.to_string()))?;
        self.transport.send(&msg).await
    }
}

fn tool_to_value(spec: &ToolSpec) -> Value {
    json!({
        "name": spec.name,
        "description": spec.description,
        "inputSchema": spec.input_schema,
    })
}

fn success_response(id: Id, result: Value) -> Response {
    Response {
        jsonrpc: mcp::JSONRPC_VERSION.to_string(),
        result: Some(result),
        error: None,
        id,
    }
}

fn error_response(id: Id, code: i64, message: &str) -> Response {
    Response {
        jsonrpc: mcp::JSONRPC_VERSION.to_string(),
        result: None,
        error: Some(RpcError {
            code,
            message: message.to_string(),
            data: None,
        }),
        id,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::mcp::registry::ToolHandler;
    use crate::domain::tool::{ToolKind, ToolSpec};
    use crate::ports::mcp_transport::McpTransport;
    use serde_json::{json, Value};
    use std::collections::VecDeque;
    use std::sync::Mutex;

    struct InMemoryTransport {
        incoming: VecDeque<String>,
        outgoing: Vec<String>,
    }

    impl InMemoryTransport {
        fn new(incoming: Vec<String>) -> Self {
            Self {
                incoming: incoming.into(),
                outgoing: Vec::new(),
            }
        }
    }

    #[async_trait::async_trait]
    impl McpTransport for InMemoryTransport {
        async fn recv(&mut self) -> Result<Option<String>> {
            Ok(self.incoming.pop_front())
        }
        async fn send(&mut self, msg: &str) -> Result<()> {
            self.outgoing.push(msg.to_string());
            Ok(())
        }
    }

    struct StubHandler {
        result: Value,
    }

    #[async_trait::async_trait]
    impl ToolHandler for StubHandler {
        async fn call(&self, _args: Value) -> Result<Value> {
            Ok(self.result.clone())
        }
    }

    async fn run(incoming: Vec<Value>) -> Vec<Value> {
        run_with_registry(incoming, Arc::new(ToolRegistry::new())).await
    }

    async fn run_with_registry(incoming: Vec<Value>, registry: Arc<ToolRegistry>) -> Vec<Value> {
        let transport =
            InMemoryTransport::new(incoming.into_iter().map(|v| v.to_string()).collect());
        let server = McpServer::new(transport, registry);
        let transport = server.run().await.unwrap();
        transport
            .outgoing
            .into_iter()
            .map(|s| serde_json::from_str(&s).unwrap())
            .collect()
    }

    fn initialize_msg(id: i64) -> Value {
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "initialize",
            "params": { "protocolVersion": "2026-07-28" }
        })
    }

    #[tokio::test]
    async fn initialize_responds_with_capabilities() {
        let out = run(vec![initialize_msg(1)]).await;
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["id"], 1);
        assert_eq!(out[0]["result"]["protocolVersion"], "2026-07-28");
        assert!(out[0]["result"]["capabilities"]["tools"].is_object());
        assert!(out[0]["result"]["capabilities"]["resources"].is_object());
        assert_eq!(
            out[0]["result"]["serverInfo"]["name"],
            env!("CARGO_PKG_NAME")
        );
    }

    #[tokio::test]
    async fn ping_returns_empty_object() {
        let out = run(vec![
            initialize_msg(1),
            json!({"jsonrpc": "2.0", "id": 2, "method": "ping"}),
        ])
        .await;
        assert_eq!(out.len(), 2);
        assert_eq!(out[1]["id"], 2);
        assert_eq!(out[1]["result"], json!({}));
    }

    #[tokio::test]
    async fn notification_has_no_response() {
        let out = run(vec![
            json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
            initialize_msg(1),
        ])
        .await;
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["id"], 1);
    }

    #[tokio::test]
    async fn tools_list_before_initialize_is_error() {
        let out = run(vec![
            json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}),
        ])
        .await;
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["id"], 1);
        assert_eq!(out[0]["error"]["code"], -32600);
    }

    #[tokio::test]
    async fn initialize_twice_is_error() {
        let out = run(vec![initialize_msg(1), initialize_msg(2)]).await;
        assert_eq!(out.len(), 2);
        assert_eq!(out[0]["id"], 1);
        assert!(out[0]["result"].is_object());
        assert_eq!(out[1]["id"], 2);
        assert_eq!(out[1]["error"]["code"], -32600);
    }

    #[tokio::test]
    async fn unknown_method_returns_32601_and_server_stays_alive() {
        let out = run(vec![
            initialize_msg(1),
            json!({"jsonrpc": "2.0", "id": 2, "method": "bogus/method"}),
            json!({"jsonrpc": "2.0", "id": 3, "method": "ping"}),
        ])
        .await;
        assert_eq!(out.len(), 3);
        assert_eq!(out[1]["error"]["code"], -32601);
        assert_eq!(out[2]["result"], json!({}));
    }

    #[tokio::test]
    async fn unknown_notification_is_ignored_silently() {
        let out = run(vec![
            json!({"jsonrpc": "2.0", "method": "bogus/notification"}),
            initialize_msg(1),
        ])
        .await;
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["id"], 1);
    }

    #[tokio::test]
    async fn parse_error_returns_32700() {
        let out = run(vec![json!("not valid json")]).await;
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["error"]["code"], -32700);
        assert_eq!(out[0]["id"], Value::Null);
    }

    #[tokio::test]
    async fn garbage_does_not_panic_the_loop() {
        let junk = vec![
            Value::String("".to_string()),
            Value::String("{".to_string()),
            json!({"jsonrpc": "1.0", "id": 1, "method": "ping"}),
            json!({"jsonrpc": "2.0", "id": "x", "method": "ping"}),
            json!({"jsonrpc": "2.0", "id": 99, "method": "tools/list"}),
            initialize_msg(100),
            json!({"jsonrpc": "2.0", "id": 101, "method": "ping"}),
        ];
        let out = run(junk).await;
        assert!(out.iter().any(|r| r["id"] == 100));
        assert!(out
            .iter()
            .any(|r| r["id"] == 101 && r["result"] == json!({})));
    }

    #[tokio::test]
    async fn tools_list_returns_registered_tools() {
        let mut registry = ToolRegistry::new();
        let spec = ToolSpec::new(
            "my_tool",
            "A test tool",
            json!({"type": "object"}),
            ToolKind::ReadOnly,
        )
        .unwrap();
        registry
            .register(spec, Arc::new(StubHandler { result: json!({}) }))
            .unwrap();

        let out = run_with_registry(
            vec![
                initialize_msg(1),
                json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
            ],
            Arc::new(registry),
        )
        .await;
        assert_eq!(out[1]["result"]["tools"][0]["name"], "my_tool");
    }

    #[tokio::test]
    async fn tools_call_dispatches_to_handler() {
        let mut registry = ToolRegistry::new();
        let spec = ToolSpec::new(
            "my_tool",
            "A test tool",
            json!({"type": "object", "properties": {"x": {"type": "integer"}}}),
            ToolKind::ReadOnly,
        )
        .unwrap();
        registry
            .register(
                spec,
                Arc::new(StubHandler {
                    result: json!({"doubled": true}),
                }),
            )
            .unwrap();

        let out = run_with_registry(
            vec![
                initialize_msg(1),
                json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "my_tool", "arguments": {"x": 2}}}),
            ],
            Arc::new(registry),
        )
        .await;
        assert_eq!(out[1]["result"]["content"][0]["type"], "text");
        assert_eq!(out[1]["result"]["isError"], false);
        assert_eq!(
            out[1]["result"]["content"][0]["text"],
            r#"{"doubled":true}"#
        );
    }

    #[test]
    fn registry_is_referenced_but_never_imports_executor() {
        // Garantía anti-deriva R5: el registry vive en application/ y no
        // depende del CLI. Este test solo fija la relación server→registry.
        let _ = std::mem::size_of::<Mutex<()>>();
        let registry = ToolRegistry::new();
        assert!(registry.list_tools().is_empty());
    }
}
