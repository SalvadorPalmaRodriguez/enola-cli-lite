// MCP-001 — Tipos puros JSON-RPC 2.0 + Model Context Protocol.
// Sin I/O, sin tokio, sin reqwest. Solo serde/serde_json (regla §2 de AGENTS.md).

use serde::{Deserialize, Deserializer, Serialize};

/// Versión del protocolo JSON-RPC (fija en la especificación 2.0).
pub(crate) const JSONRPC_VERSION: &str = "2.0";

/// Revisión del protocolo MCP que implementa este servidor.
pub(crate) const PROTOCOL_VERSION: &str = "2026-07-28";

/// Revisiones conocidas del protocolo MCP (anteriores + la nuestra).
const KNOWN_PROTOCOL_VERSIONS: &[&str] =
    &["2024-11-05", "2025-03-26", "2025-06-18", PROTOCOL_VERSION];

/// Identificador JSON-RPC: cadena, número o nulo.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum Id {
    Str(String),
    Num(i64),
    Null,
}

/// Deserializa `Option<T>` distinguiendo ausencia de `null`.
///
/// `serde` mapea `null` a `None` por defecto, lo que haría indistinguible un
/// `id: null` (petición con id nulo) de un `id` ausente (notificación). Con
/// este helper, `null` deserializa como `Some(Id::Null)` y la ausencia como
/// `None`.
fn deserialize_some<'de, T, D>(deserializer: D) -> Result<Option<T>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    T::deserialize(deserializer).map(Some)
}

/// Petición JSON-RPC 2.0. Un `id` ausente (`None`) es una notificación.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Request {
    pub(crate) jsonrpc: String,
    pub(crate) method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) params: Option<serde_json::Value>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "deserialize_some"
    )]
    pub(crate) id: Option<Id>,
}

impl Request {
    /// `true` si la petición no lleva `id` (notificación JSON-RPC).
    pub(crate) fn is_notification(&self) -> bool {
        self.id.is_none()
    }
}

/// Respuesta JSON-RPC 2.0. Exactamente uno de `result`/`error` debe ir presente.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Response {
    pub(crate) jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error: Option<RpcError>,
    pub(crate) id: Id,
}

/// Objeto de error JSON-RPC 2.0 (`code`, `message`, `data` opcional).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct RpcError {
    pub(crate) code: i64,
    pub(crate) message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) data: Option<serde_json::Value>,
}

/// Capacidades del servidor MCP anunciadas en `initialize`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct ServerCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) tools: Option<Capability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) resources: Option<Capability>,
}

/// Capacidad MCP (objeto vacío o con flags opcionales).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Capability {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) list_changed: Option<bool>,
}

/// Información del servidor (`name`, `version`) en `initialize`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct ServerInfo {
    pub(crate) name: String,
    pub(crate) version: String,
}

/// Resultado de `initialize`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InitializeResult {
    pub(crate) protocol_version: String,
    pub(crate) capabilities: ServerCapabilities,
    pub(crate) server_info: ServerInfo,
}

/// Descriptor de una tool MCP (para `tools/list`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ToolDescriptor {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) input_schema: serde_json::Value,
}

/// Resultado de `tools/call`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CallToolResult {
    pub(crate) content: Vec<ContentBlock>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) is_error: Option<bool>,
}

/// Bloque de contenido (texto) dentro de `CallToolResult`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct ContentBlock {
    #[serde(rename = "type")]
    pub(crate) content_type: String,
    pub(crate) text: String,
}

impl CallToolResult {
    /// Resultado de `tools/call` con un único bloque de texto.
    pub(crate) fn text(text: impl Into<String>, is_error: bool) -> Self {
        Self {
            content: vec![ContentBlock {
                content_type: "text".to_string(),
                text: text.into(),
            }],
            is_error: Some(is_error),
        }
    }
}

/// Negocia la versión de protocolo MCP.
///
/// Acepta la nuestra (`2026-07-28`) y cualquier revisión anterior conocida
/// (respondiendo siempre con la nuestra); rechaza versiones desconocidas con
/// un error claro, sin panic.
pub(crate) fn negotiate_protocol_version(
    requested: &str,
) -> Result<&'static str, crate::domain::error::EnolaError> {
    if KNOWN_PROTOCOL_VERSIONS.contains(&requested) {
        Ok(PROTOCOL_VERSION)
    } else {
        Err(crate::domain::error::EnolaError::ValidationError(format!(
            "Unsupported MCP protocol version: {requested}"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::error::EnolaError;
    use serde_json::{json, Value};

    // ═════════════════════════════════════════════════════════════════════
    // 1.1 Núcleo JSON-RPC 2.0
    // ═════════════════════════════════════════════════════════════════════

    #[test]
    fn request_roundtrips_with_string_id() {
        let req = Request {
            jsonrpc: JSONRPC_VERSION.to_string(),
            method: "tools/list".to_string(),
            params: Some(json!({})),
            id: Some(Id::Str("req-1".to_string())),
        };
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(v["jsonrpc"], "2.0");
        assert_eq!(v["method"], "tools/list");
        assert_eq!(v["id"], "req-1");
        let back: Request = serde_json::from_value(v).unwrap();
        assert_eq!(back, req);
    }

    #[test]
    fn request_roundtrips_with_numeric_id() {
        let req = Request {
            jsonrpc: JSONRPC_VERSION.to_string(),
            method: "ping".to_string(),
            params: None,
            id: Some(Id::Num(7)),
        };
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(v["id"], 7);
        let back: Request = serde_json::from_value(v).unwrap();
        assert_eq!(back, req);
    }

    #[test]
    fn request_roundtrips_with_null_id() {
        let req = Request {
            jsonrpc: JSONRPC_VERSION.to_string(),
            method: "ping".to_string(),
            params: None,
            id: Some(Id::Null),
        };
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(v["id"], Value::Null);
        let back: Request = serde_json::from_value(v).unwrap();
        assert_eq!(back, req);
    }

    #[test]
    fn request_omits_params_key_when_absent() {
        let req = Request {
            jsonrpc: JSONRPC_VERSION.to_string(),
            method: "ping".to_string(),
            params: None,
            id: Some(Id::Num(1)),
        };
        let v = serde_json::to_value(&req).unwrap();
        assert!(v.get("params").is_none());
    }

    #[test]
    fn notification_is_request_without_id() {
        let req = Request {
            jsonrpc: JSONRPC_VERSION.to_string(),
            method: "notifications/initialized".to_string(),
            params: None,
            id: None,
        };
        assert!(req.is_notification());
        let v = serde_json::to_value(&req).unwrap();
        assert!(v.get("id").is_none());
        let back: Request = serde_json::from_value(v).unwrap();
        assert!(back.is_notification());
    }

    #[test]
    fn response_success_roundtrips() {
        let resp = Response {
            jsonrpc: JSONRPC_VERSION.to_string(),
            result: Some(json!({"ok": true})),
            error: None,
            id: Id::Num(1),
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["jsonrpc"], "2.0");
        assert_eq!(v["id"], 1);
        assert_eq!(v["result"], json!({"ok": true}));
        assert!(v.get("error").is_none());
        let back: Response = serde_json::from_value(v).unwrap();
        assert_eq!(back, resp);
    }

    #[test]
    fn response_error_roundtrips_with_spec_codes() {
        for (code, msg) in [
            (-32700, "Parse error"),
            (-32601, "Method not found"),
            (-32602, "Invalid params"),
        ] {
            let resp = Response {
                jsonrpc: JSONRPC_VERSION.to_string(),
                result: None,
                error: Some(RpcError {
                    code,
                    message: msg.to_string(),
                    data: None,
                }),
                id: Id::Null,
            };
            let v = serde_json::to_value(&resp).unwrap();
            assert_eq!(v["error"]["code"], code);
            assert_eq!(v["error"]["message"], msg);
            assert_eq!(v["id"], Value::Null);
            let back: Response = serde_json::from_value(v).unwrap();
            assert_eq!(back, resp);
        }
    }

    #[test]
    fn serialized_response_never_emits_null_result_or_error() {
        let ok = Response {
            jsonrpc: JSONRPC_VERSION.to_string(),
            result: Some(json!(1)),
            error: None,
            id: Id::Num(1),
        };
        let s = serde_json::to_string(&ok).unwrap();
        assert!(
            !s.contains("\"error\""),
            "success response must not contain error key"
        );

        let err = Response {
            jsonrpc: JSONRPC_VERSION.to_string(),
            result: None,
            error: Some(RpcError {
                code: -32601,
                message: "Method not found".to_string(),
                data: None,
            }),
            id: Id::Null,
        };
        let s = serde_json::to_string(&err).unwrap();
        assert!(
            !s.contains("\"result\""),
            "error response must not contain result key"
        );
    }

    // ═════════════════════════════════════════════════════════════════════
    // 1.2 Tipos MCP (golden tests contra fixtures literales de la spec)
    // ═════════════════════════════════════════════════════════════════════

    #[test]
    fn initialize_result_golden_json() {
        let result = InitializeResult {
            protocol_version: PROTOCOL_VERSION.to_string(),
            capabilities: ServerCapabilities {
                tools: Some(Capability::default()),
                resources: Some(Capability::default()),
            },
            server_info: ServerInfo {
                name: env!("CARGO_PKG_NAME").to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
            },
        };
        let v = serde_json::to_value(&result).unwrap();
        assert_eq!(
            v,
            json!({
                "protocolVersion": "2026-07-28",
                "capabilities": { "tools": {}, "resources": {} },
                "serverInfo": {
                    "name": env!("CARGO_PKG_NAME"),
                    "version": env!("CARGO_PKG_VERSION")
                }
            })
        );
    }

    #[test]
    fn tool_descriptor_golden_json() {
        let tool = ToolDescriptor {
            name: "tor_list".to_string(),
            description: "List Tor hidden services".to_string(),
            input_schema: json!({"type": "object", "properties": {}}),
        };
        let v = serde_json::to_value(&tool).unwrap();
        assert_eq!(
            v,
            json!({
                "name": "tor_list",
                "description": "List Tor hidden services",
                "inputSchema": {"type": "object", "properties": {}}
            })
        );
    }

    #[test]
    fn call_tool_result_golden_json() {
        let result = CallToolResult {
            content: vec![ContentBlock {
                content_type: "text".to_string(),
                text: "onion: abcdef.onion".to_string(),
            }],
            is_error: Some(false),
        };
        let v = serde_json::to_value(&result).unwrap();
        assert_eq!(
            v,
            json!({
                "content": [{"type": "text", "text": "onion: abcdef.onion"}],
                "isError": false
            })
        );
    }

    // ═════════════════════════════════════════════════════════════════════
    // 1.3 Negociación de versión de protocolo
    // ═════════════════════════════════════════════════════════════════════

    #[test]
    fn negotiate_accepts_current_version() {
        assert_eq!(
            negotiate_protocol_version("2026-07-28").unwrap(),
            "2026-07-28"
        );
    }

    #[test]
    fn negotiate_responds_with_ours_for_known_older() {
        assert_eq!(
            negotiate_protocol_version("2025-06-18").unwrap(),
            "2026-07-28"
        );
    }

    #[test]
    fn negotiate_rejects_unknown_version() {
        match negotiate_protocol_version("2099-01-01") {
            Err(EnolaError::ValidationError(msg)) => {
                assert!(msg.contains("Unsupported"), "unclear error message: {msg}");
            }
            other => panic!("expected ValidationError, got {other:?}"),
        }
    }
}
