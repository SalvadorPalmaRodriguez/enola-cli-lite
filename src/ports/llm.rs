//! Fase 9 (9.1) — Puerto `LlmEngine` para motores LLM locales (Ollama).
//!
//! Define el contrato inyectable entre `application::ai_assistant` y el
//! adapter concreto (`adapters::ai::ollama`), permitiendo probar todo el
//! bucle de tool-calling sin levantar Ollama ni red.

use crate::domain::error::Result;
use crate::domain::tool::ToolSpec;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Rol de un mensaje en la conversación con el LLM.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatRole {
    System,
    User,
    Assistant,
    Tool,
}

/// Llamada a tool solicitada por el modelo en un turno `assistant`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LlmToolCall {
    /// Nombre de la tool (p.ej. `tor_list`, `enola_run_command`).
    pub name: String,
    /// Argumentos JSON estructurados para la tool.
    pub arguments: Value,
}

/// Mensaje individual del historial de chat enviado al motor LLM.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatMessage {
    /// Rol del emisor (`system`, `user`, `assistant`, `tool`).
    pub role: ChatRole,
    /// Contenido textual del mensaje (o serialización JSON del resultado de tool).
    pub content: String,
    /// Llamadas a tools emitidas por el asistente en este turno (vacío para otros roles).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<LlmToolCall>,
    /// Nombre de la tool cuando `role == ChatRole::Tool`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
}

impl ChatMessage {
    /// Construye un mensaje de sistema.
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: ChatRole::System,
            content: content.into(),
            tool_calls: Vec::new(),
            tool_name: None,
        }
    }

    /// Construye un mensaje del usuario.
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: ChatRole::User,
            content: content.into(),
            tool_calls: Vec::new(),
            tool_name: None,
        }
    }

    /// Construye una respuesta de texto del asistente (sin tool calls).
    pub fn assistant_text(content: impl Into<String>) -> Self {
        Self {
            role: ChatRole::Assistant,
            content: content.into(),
            tool_calls: Vec::new(),
            tool_name: None,
        }
    }

    /// Construye un turno del asistente que solicita ejecutar una o más tools.
    pub fn assistant_tools(content: impl Into<String>, tool_calls: Vec<LlmToolCall>) -> Self {
        Self {
            role: ChatRole::Assistant,
            content: content.into(),
            tool_calls,
            tool_name: None,
        }
    }

    /// Construye el mensaje con el resultado de una tool ejecutada.
    pub fn tool_result(tool_name: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: ChatRole::Tool,
            content: content.into(),
            tool_calls: Vec::new(),
            tool_name: Some(tool_name.into()),
        }
    }
}

/// Meta-propiedades exclusivas del protocolo MCP que se ocultan en los schemas
/// enviados al LLM local (R11).
pub const MCP_SCHEMA_META_KEYS: &[&str] = &["confirm_token", "try_run"];

/// Definición de tool expuesta al motor LLM (formato function-calling).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LlmToolDefinition {
    /// Nombre único de la tool.
    pub name: String,
    /// Descripción orientada al modelo.
    pub description: String,
    /// JSON Schema de los parámetros (`type: "object"`).
    pub parameters: Value,
}

impl LlmToolDefinition {
    /// Convierte un `ToolSpec` del `ToolRegistry` en definición para el LLM,
    /// filtrando las meta-propiedades exclusivas de MCP (`confirm_token`, `try_run`)
    /// para no confundir a modelos pequeños (R11).
    pub(crate) fn from_tool_spec(spec: &ToolSpec) -> Self {
        let mut parameters = spec.input_schema.clone();
        if let Some(obj) = parameters.as_object_mut() {
            if let Some(props) = obj.get_mut("properties").and_then(|p| p.as_object_mut()) {
                for meta in MCP_SCHEMA_META_KEYS {
                    props.remove(*meta);
                }
            }
            if let Some(req) = obj.get_mut("required").and_then(|r| r.as_array_mut()) {
                req.retain(|v| {
                    v.as_str()
                        .map(|s| !MCP_SCHEMA_META_KEYS.contains(&s))
                        .unwrap_or(true)
                });
            }
        }
        Self {
            name: spec.name.clone(),
            description: spec.description.clone(),
            parameters,
        }
    }
}

/// Respuesta de un turno de `chat_tools`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LlmChatResponse {
    /// Respuesta textual del asistente (puede ser vacía si solo emite `tool_calls`).
    pub content: String,
    /// Llamadas a tools solicitadas por el modelo en este turno.
    pub tool_calls: Vec<LlmToolCall>,
}

/// Metadatos básicos de un modelo disponible en el motor LLM (`/api/tags`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LlmModelInfo {
    /// Nombre/tag del modelo (p.ej. `llama3.1:8b`).
    pub name: String,
    /// Tamaño en bytes en disco, si el motor lo reporta.
    pub size_bytes: Option<u64>,
    /// Fecha de modificación ISO-8601, si el motor la reporta.
    pub modified_at: Option<String>,
}

/// Puerto de motor LLM con soporte de tool-calling y gestión básica de modelos.
#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait LlmEngine: Send + Sync {
    /// Envía el historial `messages` y el catálogo `tools` al modelo `model`.
    async fn chat_tools(
        &self,
        model: &str,
        messages: &[ChatMessage],
        tools: &[LlmToolDefinition],
    ) -> Result<LlmChatResponse>;

    /// Lista los modelos descargados localmente en el motor.
    async fn list_models(&self) -> Result<Vec<LlmModelInfo>>;

    /// Descarga/actualiza un modelo en el motor (`/api/pull`).
    async fn pull_model(&self, model: &str) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::mcp::registry::{ToolHandler, ToolRegistry};
    use crate::domain::error::EnolaError;
    use crate::domain::tool::ToolKind;
    use serde_json::json;
    use std::sync::{Arc, Mutex};

    struct SpyHandler {
        called: Mutex<Vec<Value>>,
    }

    #[async_trait::async_trait]
    impl ToolHandler for SpyHandler {
        async fn call(&self, args: Value) -> Result<Value> {
            self.called.lock().unwrap().push(args);
            Ok(json!({"status": "ok"}))
        }
    }

    #[tokio::test]
    async fn mock_llm_engine_returns_plain_text_response() {
        let mut mock = MockLlmEngine::new();
        mock.expect_chat_tools()
            .withf(|model, msgs, tools| {
                model == "llama3.1:8b"
                    && msgs.len() == 1
                    && msgs[0].role == ChatRole::User
                    && msgs[0].content == "hola"
                    && tools.is_empty()
            })
            .times(1)
            .returning(|_, _, _| {
                Ok(LlmChatResponse {
                    content: "Hola, ¿en qué puedo ayudarte?".to_string(),
                    tool_calls: vec![],
                })
            });

        let resp = mock
            .chat_tools("llama3.1:8b", &[ChatMessage::user("hola")], &[])
            .await
            .unwrap();
        assert_eq!(resp.content, "Hola, ¿en qué puedo ayudarte?");
        assert!(resp.tool_calls.is_empty());
    }

    #[tokio::test]
    async fn mock_llm_engine_returns_tool_call_and_dispatches_via_registry() {
        let mut reg = ToolRegistry::new();
        let spec = ToolSpec::new(
            "wp_status",
            "Get WordPress status",
            json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "minLength": 1 }
                },
                "required": ["name"],
                "additionalProperties": false
            }),
            ToolKind::ReadOnly,
        )
        .unwrap();
        let spy = Arc::new(SpyHandler {
            called: Mutex::new(Vec::new()),
        });
        reg.register(spec.clone(), spy.clone()).unwrap();

        let tool_def = LlmToolDefinition::from_tool_spec(&spec);
        assert_eq!(tool_def.name, "wp_status");

        let mut mock = MockLlmEngine::new();
        mock.expect_chat_tools().times(1).returning(|_, _, _| {
            Ok(LlmChatResponse {
                content: String::new(),
                tool_calls: vec![LlmToolCall {
                    name: "wp_status".to_string(),
                    arguments: json!({"name": "myblog"}),
                }],
            })
        });

        let resp = mock
            .chat_tools(
                "llama3.1:8b",
                &[ChatMessage::user("estado de myblog")],
                &[tool_def],
            )
            .await
            .unwrap();
        assert_eq!(resp.tool_calls.len(), 1);
        let tc = &resp.tool_calls[0];
        let out = reg.call(&tc.name, tc.arguments.clone()).await.unwrap();
        assert_eq!(out, json!({"status": "ok"}));
        assert_eq!(spy.called.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn tool_call_with_invalid_arguments_is_rejected_by_registry_before_handler() {
        let mut reg = ToolRegistry::new();
        let spec = ToolSpec::new(
            "wp_create",
            "Create WordPress site",
            json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "minLength": 1 },
                    "http_port": { "type": "integer", "minimum": 8080, "maximum": 9000 }
                },
                "required": ["name"],
                "additionalProperties": false
            }),
            ToolKind::Mutating,
        )
        .unwrap();
        let spy = Arc::new(SpyHandler {
            called: Mutex::new(Vec::new()),
        });
        reg.register(spec, spy.clone()).unwrap();

        let mut mock = MockLlmEngine::new();
        mock.expect_chat_tools().times(1).returning(|_, _, _| {
            Ok(LlmChatResponse {
                content: String::new(),
                tool_calls: vec![LlmToolCall {
                    name: "wp_create".to_string(),
                    // http_port fuera de rango y falta "name" requerido
                    arguments: json!({"http_port": 80}),
                }],
            })
        });

        let resp = mock
            .chat_tools("llama3.1:8b", &[ChatMessage::user("crea wp")], &[])
            .await
            .unwrap();
        let tc = &resp.tool_calls[0];
        let err = reg.call(&tc.name, tc.arguments.clone()).await.unwrap_err();
        assert!(matches!(err, EnolaError::ValidationError(_)));
        assert!(
            spy.called.lock().unwrap().is_empty(),
            "el handler NO debe invocarse cuando el LLM emite argumentos inválidos"
        );
    }

    #[test]
    fn from_tool_spec_strips_mcp_meta_properties_confirm_token_and_try_run() {
        let spec = ToolSpec::new(
            "tor_remove",
            "Remove Tor service",
            json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string" },
                    "force": { "type": "boolean" },
                    "confirm_token": { "type": "string" },
                    "try_run": { "type": "boolean" }
                },
                "required": ["name", "force"],
                "additionalProperties": false
            }),
            ToolKind::Destructive,
        )
        .unwrap();

        let def = LlmToolDefinition::from_tool_spec(&spec);
        assert_eq!(def.name, "tor_remove");
        assert_eq!(def.description, "Remove Tor service");
        let props = def.parameters["properties"].as_object().unwrap();
        assert!(props.contains_key("name"));
        assert!(props.contains_key("force"));
        assert!(!props.contains_key("confirm_token"));
        assert!(!props.contains_key("try_run"));
    }

    #[tokio::test]
    async fn mock_list_and_pull_models_contract() {
        let mut mock = MockLlmEngine::new();
        mock.expect_list_models().times(1).returning(|| {
            Ok(vec![LlmModelInfo {
                name: "llama3.1:8b".to_string(),
                size_bytes: Some(4_700_000_000),
                modified_at: Some("2026-10-18T00:00:00Z".to_string()),
            }])
        });
        mock.expect_pull_model()
            .withf(|m| m == "llama3.1:8b")
            .times(1)
            .returning(|_| Ok(()));

        let models = mock.list_models().await.unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].name, "llama3.1:8b");
        mock.pull_model("llama3.1:8b").await.unwrap();
    }

    #[test]
    fn chat_message_constructors_set_roles_and_fields() {
        let sys = ChatMessage::system("sys");
        assert_eq!(sys.role, ChatRole::System);
        assert_eq!(sys.content, "sys");

        let txt = ChatMessage::assistant_text("ok");
        assert_eq!(txt.role, ChatRole::Assistant);
        assert_eq!(txt.content, "ok");
        assert!(txt.tool_calls.is_empty());

        let tc = LlmToolCall {
            name: "tor_list".to_string(),
            arguments: json!({}),
        };
        let tools_msg = ChatMessage::assistant_tools("", vec![tc.clone()]);
        assert_eq!(tools_msg.role, ChatRole::Assistant);
        assert_eq!(tools_msg.tool_calls, vec![tc]);

        let res = ChatMessage::tool_result("tor_list", "[]");
        assert_eq!(res.role, ChatRole::Tool);
        assert_eq!(res.tool_name.as_deref(), Some("tor_list"));
        assert_eq!(res.content, "[]");
    }
}
