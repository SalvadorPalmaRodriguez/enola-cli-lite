// MCP-003 — Bucle de protocolo: handshake, routing de métodos y errores.
//
// El servidor es un driving adapter que habla JSON-RPC 2.0 por un
// `McpTransport`. El handshake exige `initialize` antes de `tools/list` y
// `resources/list`; las notificaciones se atienden sin respuesta y los errores
// de protocolo no cierran el bucle.

use crate::domain::error::Result;
use crate::domain::mcp::{self, Id, Request, Response, RpcError};
use crate::ports::mcp_transport::McpTransport;
use serde_json::{json, Value};

pub(crate) struct McpServer<T: McpTransport> {
    transport: T,
    initialized: bool,
}

impl<T: McpTransport> McpServer<T> {
    pub(crate) fn new(transport: T) -> Self {
        Self {
            transport,
            initialized: false,
        }
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
            "initialize" => {
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
                    Err(_) => {
                        error_response(id, -32602, "Invalid params: unsupported protocolVersion")
                    }
                }
            }
            "ping" => success_response(id, json!({})),
            "tools/list" => {
                if !self.initialized {
                    return error_response(id, -32600, "Invalid Request: not initialized");
                }
                success_response(id, json!({ "tools": [] }))
            }
            "resources/list" => {
                if !self.initialized {
                    return error_response(id, -32600, "Invalid Request: not initialized");
                }
                success_response(id, json!({ "resources": [] }))
            }
            _ => error_response(id, -32601, "Method not found"),
        }
    }

    fn handle_notification(&self, request: &Request) {
        match request.method.as_str() {
            "notifications/initialized" => {}
            _ => {} // notificación desconocida → ignorada en silencio (spec)
        }
    }

    async fn send_response(&mut self, resp: Response) -> Result<()> {
        let msg = serde_json::to_string(&resp)
            .map_err(|e| crate::domain::error::EnolaError::ValidationError(e.to_string()))?;
        self.transport.send(&msg).await
    }
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
    use crate::ports::mcp_transport::McpTransport;
    use serde_json::{json, Value};
    use std::collections::VecDeque;

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
        async fn recv(&mut self) -> crate::domain::error::Result<Option<String>> {
            Ok(self.incoming.pop_front())
        }
        async fn send(&mut self, msg: &str) -> crate::domain::error::Result<()> {
            self.outgoing.push(msg.to_string());
            Ok(())
        }
    }

    async fn run(incoming: Vec<Value>) -> Vec<Value> {
        let transport =
            InMemoryTransport::new(incoming.into_iter().map(|v| v.to_string()).collect());
        let server = McpServer::new(transport);
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
        // No debe panickear; el bucle sobrevive a todo.
        let out = run(junk).await;
        assert!(out.iter().any(|r| r["id"] == 100));
        assert!(out
            .iter()
            .any(|r| r["id"] == 101 && r["result"] == json!({})));
    }
}
