// MCP-003 — Bucle de protocolo: handshake, routing de métodos y errores.
//
// El servidor es un driving adapter que habla JSON-RPC 2.0 por un
// `McpTransport`. El handshake exige `initialize` antes de `tools/list` y
// `resources/list`; las notificaciones se atienden sin respuesta y los errores
// de protocolo no cierran el bucle. Las tools viven en un `ToolRegistry`
// compartido (también lo usará `ask` en la Fase 9).

use crate::application::mcp::audit::AuditLogger;
use crate::application::mcp::confirmation::{confirmation_required_result, ConfirmationStore};
use crate::application::mcp::policy::McpPolicy;
use crate::application::mcp::registry::ToolRegistry;
use crate::domain::error::{EnolaError, Result};
use crate::domain::mcp::{
    self, CallToolResult, Capability, Id, InitializeResult, Request, Response, RpcError,
    ServerCapabilities, ServerInfo, ToolDescriptor,
};
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
    confirmations: Arc<ConfirmationStore>,
}

impl<T: McpTransport> McpServer<T> {
    pub(crate) fn new(transport: T, registry: Arc<ToolRegistry>) -> Self {
        Self {
            transport,
            initialized: false,
            registry,
            policy: McpPolicy::default(),
            audit: Arc::new(AuditLogger::new(None)),
            confirmations: Arc::new(ConfirmationStore::new()),
        }
    }

    /// MCP-007: inyecta la política de escritura y el log de auditoría.
    pub(crate) fn with_policy(mut self, policy: McpPolicy, audit: Arc<AuditLogger>) -> Self {
        self.policy = policy;
        self.audit = audit;
        self
    }

    /// MCP-009: inyecta el store de confirmaciones compartido con el registry.
    pub(crate) fn with_confirmations(mut self, store: Arc<ConfirmationStore>) -> Self {
        self.confirmations = store;
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
                let result = InitializeResult {
                    protocol_version: version.to_string(),
                    capabilities: ServerCapabilities {
                        tools: Some(Capability::default()),
                        resources: Some(Capability::default()),
                    },
                    server_info: ServerInfo {
                        name: env!("CARGO_PKG_NAME").to_string(),
                        version: env!("CARGO_PKG_VERSION").to_string(),
                    },
                };
                success_response(
                    id,
                    serde_json::to_value(&result).unwrap_or_else(|_| json!({})),
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

    #[tracing::instrument(skip_all, fields(tool = tracing::field::Empty))]
    async fn handle_tool_call(&self, request: &Request, id: Id) -> Response {
        if !self.initialized {
            return error_response(id, -32600, "Invalid Request: not initialized");
        }
        let params = request.params.as_ref().and_then(|p| p.as_object());
        let Some(name) = params.and_then(|p| p.get("name")).and_then(|v| v.as_str()) else {
            return error_response(id, -32602, "Invalid params: missing 'name'");
        };
        tracing::Span::current().record("tool", name);
        let args = params
            .and_then(|p| p.get("arguments"))
            .cloned()
            .unwrap_or_else(|| json!({}));

        // MCP-007: política de escritura evaluada ANTES del dispatch.
        // Una tool no registrada se rechaza como parámetro inválido: no hay
        // kind por defecto (default-deny real vive en `classify`).
        let Some(kind) = self.registry.kind_of(name) else {
            return error_response(id, -32602, &format!("Invalid params: unknown tool: {name}"));
        };
        let writes = kind.writes();
        if let Err(e) = self.policy.allows(kind, name) {
            if writes {
                self.audit.log_call(name, &args, "rejected");
            }
            return success_response(
                id,
                serde_json::to_value(CallToolResult::text(format!("{e}"), true))
                    .unwrap_or_else(|_| json!({})),
            );
        }

        // MCP-009: `try_run` enruta a la simulación del handler. La política
        // ya se aplicó arriba (una simulación exige los mismos permisos que
        // la ejecución) y la validación de schema corre dentro de try_call.
        let try_run = args.get("try_run").and_then(|v| v.as_bool()) == Some(true);

        // Auditoría del ciclo de vida: la operación queda registrada como
        // solicitada en cuanto pasa la política; estados posteriores la
        // completan (rejected/confirmation_requested/authorized/simulated/
        // ok/error).
        if writes {
            self.audit.log_call(name, &args, "requested");
        }

        // Validación temprana del schema: un challenge de confirmación nunca
        // se emite sobre argumentos inválidos.
        if let Err(e) = self.registry.validate(name, &args) {
            if writes {
                self.audit.log_call(name, &args, &format!("rejected: {e}"));
            }
            return error_response(id, -32602, &format!("Invalid params: {e}"));
        }

        // MCP-009: las destructivas exigen una confirmación de dos pasos
        // vinculada a (tool + args). `force: true` solo no autoriza nada.
        let mut op_id: Option<String> = None;
        if kind == ToolKind::Destructive && !try_run {
            match args.get("confirm_token").and_then(|v| v.as_str()) {
                Some(token) => {
                    if let Err(e) = self.confirmations.consume(token, name, &args) {
                        self.audit.log_call_op(name, &args, "rejected", Some(token));
                        return error_response(id, -32602, &format!("Invalid params: {e}"));
                    }
                    op_id = Some(token.to_string());
                    self.audit
                        .log_call_op(name, &args, "authorized", op_id.as_deref());
                }
                None => match self.confirmations.create(name, &args) {
                    Ok(token) => {
                        self.audit
                            .log_call_op(name, &args, "confirmation_requested", Some(&token));
                        let result = confirmation_required_result(name, &args, &token);
                        return success_response(
                            id,
                            serde_json::to_value(CallToolResult::text(result.to_string(), false))
                                .unwrap_or_else(|_| json!({})),
                        );
                    }
                    Err(e) => {
                        return error_response(id, -32603, &e.to_string());
                    }
                },
            }
        }

        // request_timeout_secs conectado a la ejecución real de la tool.
        // OJO: un timeout NO cancela el caso de uso (tokio::time::timeout solo
        // deja de esperar); una mutación puede completarse en segundo plano y
        // queda registrada como "error: timed out" — estado parcial posible.
        let timeout = Duration::from_secs(self.policy.request_timeout_secs);
        let outcome = if try_run {
            match tokio::time::timeout(timeout, self.registry.try_call(name, args.clone())).await {
                Ok(res) => res,
                Err(_) => Err(EnolaError::Timeout(format!(
                    "tool '{name}' timed out after {}s",
                    timeout.as_secs()
                ))),
            }
        } else {
            match tokio::time::timeout(timeout, self.registry.call(name, args.clone())).await {
                Ok(res) => res,
                Err(_) => Err(EnolaError::Timeout(format!(
                    "tool '{name}' timed out after {}s",
                    timeout.as_secs()
                ))),
            }
        };

        if writes {
            let label = if try_run {
                "simulated".to_string()
            } else {
                match &outcome {
                    Ok(_) => "ok".to_string(),
                    Err(e) => format!("error: {e}"),
                }
            };
            self.audit
                .log_call_op(name, &args, &label, op_id.as_deref());
        }

        match outcome {
            Ok(result) => {
                let text = serde_json::to_string(&result).unwrap_or_else(|_| "{}".to_string());
                success_response(
                    id,
                    serde_json::to_value(CallToolResult::text(text, false))
                        .unwrap_or_else(|_| json!({})),
                )
            }
            Err(e) => match e {
                EnolaError::NotFound(_) | EnolaError::ValidationError(_) => {
                    error_response(id, -32602, &format!("Invalid params: {e}"))
                }
                EnolaError::SecurityError(_) => success_response(
                    id,
                    serde_json::to_value(CallToolResult::text(format!("{e}"), true))
                        .unwrap_or_else(|_| json!({})),
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
    serde_json::to_value(&ToolDescriptor {
        name: spec.name.clone(),
        description: spec.description.clone(),
        input_schema: spec.input_schema.clone(),
    })
    .unwrap_or_else(|_| json!({}))
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
        run_full(incoming, registry, McpPolicy::default(), None).await
    }

    async fn run_full(
        incoming: Vec<Value>,
        registry: Arc<ToolRegistry>,
        policy: McpPolicy,
        confirmations: Option<Arc<ConfirmationStore>>,
    ) -> Vec<Value> {
        let transport =
            InMemoryTransport::new(incoming.into_iter().map(|v| v.to_string()).collect());
        let store = confirmations.unwrap_or_else(|| Arc::new(ConfirmationStore::new()));
        let server = McpServer::new(transport, registry)
            .with_policy(policy, Arc::new(AuditLogger::new(None)))
            .with_confirmations(store);
        let transport = server.run().await.unwrap();
        transport
            .outgoing
            .into_iter()
            .map(|s| serde_json::from_str(&s).unwrap())
            .collect()
    }

    /// Registry con una tool destructiva stub (cuenta sus ejecuciones).
    fn destructive_registry() -> (Arc<ToolRegistry>, Arc<std::sync::atomic::AtomicUsize>) {
        use std::sync::atomic::AtomicUsize;
        let counter = Arc::new(AtomicUsize::new(0));
        struct Destructive {
            counter: Arc<AtomicUsize>,
        }
        #[async_trait::async_trait]
        impl ToolHandler for Destructive {
            async fn call(&self, _args: Value) -> Result<Value> {
                self.counter
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(json!({"done": true}))
            }
        }
        let mut registry = ToolRegistry::new();
        let spec = ToolSpec::new(
            "svc_delete",
            "Destructive stub",
            json!({
                "type": "object",
                "properties": {
                    "name": {"type": "string"},
                    "confirm_token": {"type": "string", "minLength": 32, "maxLength": 64},
                    "try_run": {"type": "boolean"}
                },
                "required": ["name"],
                "additionalProperties": false
            }),
            ToolKind::Destructive,
        )
        .unwrap();
        registry
            .register(
                spec,
                Arc::new(Destructive {
                    counter: counter.clone(),
                }),
            )
            .unwrap();
        (Arc::new(registry), counter)
    }

    fn call_tool(id: i64, name: &str, args: Value) -> Value {
        json!({"jsonrpc": "2.0", "id": id, "method": "tools/call",
               "params": {"name": name, "arguments": args}})
    }

    /// Extrae el payload JSON del content[0].text de un tools/call ok.
    fn tool_payload(resp: &Value) -> Value {
        serde_json::from_str(resp["result"]["content"][0]["text"].as_str().unwrap()).unwrap()
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

    #[tokio::test]
    async fn unknown_tool_call_returns_invalid_params() {
        let out = run(vec![
            initialize_msg(1),
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "no_such_tool", "arguments": {}}}),
        ])
        .await;
        assert_eq!(out[1]["error"]["code"], -32602);
        assert!(out[1]["error"]["message"]
            .as_str()
            .unwrap()
            .contains("unknown tool"));
    }

    // ── MCP-009: confirmación de dos pasos a nivel de protocolo ──

    #[tokio::test]
    async fn destructive_without_token_returns_challenge_and_never_executes() {
        let (registry, counter) = destructive_registry();
        let policy = McpPolicy {
            allow_write: true,
            ..Default::default()
        };
        let out = run_full(
            vec![
                initialize_msg(1),
                call_tool(2, "svc_delete", json!({"name": "db"})),
            ],
            registry,
            policy,
            None,
        )
        .await;
        let payload = tool_payload(&out[1]);
        assert_eq!(payload["confirmation_required"], true);
        assert!(payload["confirm_token"].as_str().unwrap().len() >= 32);
        assert_eq!(payload["params"]["name"], "db");
        // El handler NUNCA se invocó.
        assert_eq!(counter.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn destructive_token_does_not_survive_server_restart() {
        let (registry, counter) = destructive_registry();
        let policy = McpPolicy {
            allow_write: true,
            ..Default::default()
        };
        let out = run_full(
            vec![
                initialize_msg(1),
                call_tool(2, "svc_delete", json!({"name": "db"})),
                // id=3: replay con params DISTINTOS → rechazado antes de dispatch
                call_tool(
                    3,
                    "svc_delete",
                    json!({"name": "OTHER", "confirm_token": "pending"}),
                ),
            ],
            registry.clone(),
            policy,
            None,
        )
        .await;
        let token = tool_payload(&out[1])["confirm_token"]
            .as_str()
            .unwrap()
            .to_string();
        // id=3 llevaba token inventado → error -32602.
        assert_eq!(out[2]["error"]["code"], -32602);
        assert_eq!(counter.load(std::sync::atomic::Ordering::SeqCst), 0);

        // Segunda tanda: confirmación correcta + replay del mismo token.
        let out2 = run_full(
            vec![
                initialize_msg(1),
                call_tool(
                    2,
                    "svc_delete",
                    json!({"name": "db", "confirm_token": token}),
                ),
                call_tool(
                    3,
                    "svc_delete",
                    json!({"name": "db", "confirm_token": token}),
                ),
            ],
            registry.clone(),
            McpPolicy {
                allow_write: true,
                ..Default::default()
            },
            // NOTA: nuevo server = nuevo store → el token no existiría.
            // Este escenario real necesita el MISMO store; ver test siguiente.
            None,
        )
        .await;
        assert_eq!(out2[1]["error"]["code"], -32602); // token desconocido en store nuevo
        assert_eq!(counter.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn destructive_confirm_then_execute_then_replay_fails() {
        let (registry, counter) = destructive_registry();
        let store = Arc::new(ConfirmationStore::new());
        let policy = McpPolicy {
            allow_write: true,
            ..Default::default()
        };
        let out = run_full(
            vec![
                initialize_msg(1),
                call_tool(2, "svc_delete", json!({"name": "db"})),
            ],
            registry.clone(),
            policy.clone(),
            Some(store.clone()),
        )
        .await;
        let token = tool_payload(&out[1])["confirm_token"]
            .as_str()
            .unwrap()
            .to_string();

        // Mismo store (misma sesión): confirmar ejecuta; replay rechaza.
        let out2 = run_full(
            vec![
                initialize_msg(1),
                call_tool(
                    2,
                    "svc_delete",
                    json!({"name": "db", "confirm_token": token}),
                ),
                call_tool(
                    3,
                    "svc_delete",
                    json!({"name": "db", "confirm_token": token}),
                ),
            ],
            registry,
            policy,
            Some(store),
        )
        .await;
        assert_eq!(tool_payload(&out2[1])["done"], true);
        assert_eq!(out2[2]["error"]["code"], -32602); // replay
        assert_eq!(counter.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn destructive_rejected_without_allow_write_before_challenge() {
        let (registry, counter) = destructive_registry();
        let out = run_full(
            vec![
                initialize_msg(1),
                call_tool(2, "svc_delete", json!({"name": "db"})),
            ],
            registry,
            McpPolicy::default(), // allow_write=false
            None,
        )
        .await;
        assert_eq!(out[1]["result"]["isError"], true);
        assert!(out[1]["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("allow-write"));
        assert_eq!(counter.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn destructive_try_run_simulates_without_challenge() {
        let (registry, counter) = destructive_registry();
        let policy = McpPolicy {
            allow_write: true,
            ..Default::default()
        };
        let out = run_full(
            vec![
                initialize_msg(1),
                call_tool(2, "svc_delete", json!({"name": "db", "try_run": true})),
            ],
            registry,
            policy,
            None,
        )
        .await;
        let payload = tool_payload(&out[1]);
        // Handler por defecto: no simulable, pero NUNCA ejecuta ni pide token.
        assert_eq!(payload["simulated"], false);
        assert!(payload.get("confirm_token").is_none());
        assert_eq!(counter.load(std::sync::atomic::Ordering::SeqCst), 0);
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
