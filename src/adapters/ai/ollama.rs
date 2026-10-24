//! Fase 9 (9.2) — Adapter HTTP para Ollama (`/api/chat`, `/api/tags`, `/api/pull`,
//! `/api/blobs` + `/api/create`).
//!
//! Construye su cliente a través de `crate::infrastructure::http::http_client_builder`,
//! soportando tanto endpoints locales (`http://127.0.0.1:11434`) como endpoints
//! remotos `.onion` enrutados automáticamente por `socks5h://`.

use crate::domain::error::{EnolaError, Result};
use crate::ports::llm::{
    ChatMessage, LlmChatResponse, LlmEngine, LlmModelInfo, LlmToolCall, LlmToolDefinition,
};
use serde_json::{json, Value};
use std::time::Duration;

/// Timeout por defecto para peticiones a Ollama (inferencia local puede tardar).
pub const DEFAULT_OLLAMA_TIMEOUT_SECS: u64 = 120;

/// Endpoint local por defecto de Ollama.
pub const DEFAULT_OLLAMA_ENDPOINT: &str = "http://127.0.0.1:11434";

/// Modelo por defecto para `enola-cli ask`.
pub const DEFAULT_OLLAMA_MODEL: &str = "llama3.1:8b";

/// Adapter HTTP contra un servidor Ollama local o `.onion`.
#[derive(Debug, Clone)]
pub struct OllamaAdapter {
    endpoint: String,
    timeout: Duration,
}

impl OllamaAdapter {
    /// Crea un adapter apuntando a `endpoint` con `timeout`.
    pub fn new(endpoint: impl Into<String>, timeout: Duration) -> Self {
        let raw = endpoint.into();
        let trimmed = raw.trim().trim_end_matches('/').to_string();
        Self {
            endpoint: if trimmed.is_empty() {
                DEFAULT_OLLAMA_ENDPOINT.to_string()
            } else {
                trimmed
            },
            timeout,
        }
    }

    /// Crea un adapter con los valores por defecto (`http://127.0.0.1:11434`, 120 s).
    pub fn with_defaults() -> Self {
        Self::new(
            DEFAULT_OLLAMA_ENDPOINT,
            Duration::from_secs(DEFAULT_OLLAMA_TIMEOUT_SECS),
        )
    }

    /// Endpoint base normalizado (sin `/` final).
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// Timeout configurado por petición.
    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    fn build_client(&self) -> Result<reqwest::Client> {
        let builder = crate::infrastructure::http::http_client_builder(&self.endpoint)?;
        builder.timeout(self.timeout).build().map_err(|e| {
            EnolaError::InfrastructureError(format!(
                "Failed to build HTTP client for Ollama ({}): {e}",
                self.endpoint
            ))
        })
    }

    /// Sube `path` al almacén de blobs de Ollama y devuelve su digest
    /// (`sha256:<hex>`). Calcula el SHA-256 en streaming y, si el blob ya
    /// existe (`HEAD` 200), no lo vuelve a subir. Cliente bloqueante en
    /// `spawn_blocking`: el GGUF puede pesar varios GiB y no se carga en memoria.
    async fn push_blob(&self, path: &std::path::Path) -> Result<String> {
        let endpoint = self.endpoint.clone();
        let timeout = self.timeout;
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || push_blob_blocking(&endpoint, timeout, &path))
            .await
            .map_err(|e| {
                EnolaError::InfrastructureError(format!("Subida del blob a Ollama abortada: {e}"))
            })?
    }

    fn map_reqwest_error(&self, op: &str, err: &reqwest::Error) -> EnolaError {
        if err.is_timeout() {
            return EnolaError::Timeout(format!(
                "Ollama {op} agotó el tiempo de espera ({}s) en {}: {err}",
                self.timeout.as_secs(),
                self.endpoint
            ));
        }
        if err.is_connect() {
            return EnolaError::InfrastructureError(format!(
                "No se pudo conectar con Ollama en '{}' ({err}). \
                 Asegúrate de tener Ollama activo (`ollama serve`), instálalo o levanta una instancia \
                 gestionada con `sudo enola-cli ai create` (o ajusta --endpoint / ENOLA_AI_ENDPOINT / [ai].endpoint).",
                self.endpoint
            ));
        }
        EnolaError::InfrastructureError(format!(
            "Error HTTP en Ollama ({op} contra {}): {err}",
            self.endpoint
        ))
    }
}

fn serialize_message(msg: &ChatMessage) -> Value {
    let mut obj = serde_json::Map::new();
    let role = match msg.role {
        crate::ports::llm::ChatRole::System => "system",
        crate::ports::llm::ChatRole::User => "user",
        crate::ports::llm::ChatRole::Assistant => "assistant",
        crate::ports::llm::ChatRole::Tool => "tool",
    };
    obj.insert("role".to_string(), json!(role));
    obj.insert("content".to_string(), json!(msg.content));
    if !msg.tool_calls.is_empty() {
        let calls: Vec<Value> = msg
            .tool_calls
            .iter()
            .map(|tc| {
                json!({
                    "function": {
                        "name": tc.name,
                        "arguments": tc.arguments,
                    }
                })
            })
            .collect();
        obj.insert("tool_calls".to_string(), Value::Array(calls));
    }
    if let Some(ref tool_name) = msg.tool_name {
        obj.insert("tool_name".to_string(), json!(tool_name));
    }
    Value::Object(obj)
}

fn serialize_tool(def: &LlmToolDefinition) -> Value {
    json!({
        "type": "function",
        "function": {
            "name": def.name,
            "description": def.description,
            "parameters": def.parameters,
        }
    })
}

/// Extrae el mensaje de error si el cuerpo JSON (o línea NDJSON) de Ollama incluye `"error"`.
/// SHA-256 en streaming de `path`, como digest de blob Ollama (`sha256:<hex>`).
fn blob_digest(path: &std::path::Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut file = std::fs::File::open(path).map_err(|e| {
        EnolaError::InfrastructureError(format!("No se pudo abrir '{}': {e}", path.display()))
    })?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut buf).map_err(|e| {
            EnolaError::InfrastructureError(format!("No se pudo leer '{}': {e}", path.display()))
        })?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

/// Cuerpo síncrono de `OllamaAdapter::push_blob` (ver allí).
fn push_blob_blocking(endpoint: &str, timeout: Duration, path: &std::path::Path) -> Result<String> {
    let digest = blob_digest(path)?;
    let client = crate::infrastructure::http::build_blocking_http_client(endpoint, timeout)?;
    let url = format!("{endpoint}/api/blobs/{digest}");
    let http_err = |op: &str, e: reqwest::Error| {
        EnolaError::InfrastructureError(format!(
            "Error HTTP en Ollama ({op} contra {endpoint}): {e}"
        ))
    };

    let head = client
        .head(&url)
        .send()
        .map_err(|e| http_err("HEAD /api/blobs", e))?;
    if head.status().is_success() {
        return Ok(digest);
    }

    let file = std::fs::File::open(path).map_err(|e| {
        EnolaError::InfrastructureError(format!("No se pudo abrir '{}': {e}", path.display()))
    })?;
    let len = file
        .metadata()
        .map_err(|e| {
            EnolaError::InfrastructureError(format!("No se pudo leer '{}': {e}", path.display()))
        })?
        .len();
    let resp = client
        .post(&url)
        .body(reqwest::blocking::Body::sized(file, len))
        .send()
        .map_err(|e| http_err("POST /api/blobs", e))?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().unwrap_or_default();
        let detail = extract_ollama_error(&body).unwrap_or_else(|| body.trim().to_string());
        return Err(EnolaError::InfrastructureError(format!(
            "Ollama /api/blobs devolvió HTTP {}: {}",
            status.as_u16(),
            detail
        )));
    }
    Ok(digest)
}

fn extract_ollama_error(body: &str) -> Option<String> {
    if let Ok(v) = serde_json::from_str::<Value>(body) {
        if let Some(err) = v.get("error").and_then(|e| e.as_str()) {
            return Some(err.to_string());
        }
    }
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
            if let Some(err) = v.get("error").and_then(|e| e.as_str()) {
                return Some(err.to_string());
            }
        }
    }
    None
}

fn parse_tool_calls_from_message(msg: &Value) -> Vec<LlmToolCall> {
    let Some(arr) = msg.get("tool_calls").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        let func = item.get("function").unwrap_or(item);
        let Some(name) = func.get("name").and_then(|v| v.as_str()) else {
            continue;
        };
        let raw_args = func.get("arguments").cloned().unwrap_or_else(|| json!({}));
        let arguments = match raw_args {
            Value::String(s) => serde_json::from_str::<Value>(&s).unwrap_or_else(|_| json!({})),
            Value::Null => json!({}),
            other => other,
        };
        out.push(LlmToolCall {
            name: name.to_string(),
            arguments,
        });
    }
    out
}

fn parse_chat_response_body(body: &str) -> Result<LlmChatResponse> {
    if let Some(err) = extract_ollama_error(body) {
        return Err(EnolaError::InfrastructureError(format!(
            "Ollama error: {err}"
        )));
    }

    // Caso 1: JSON único (`stream: false`).
    if let Ok(v) = serde_json::from_str::<Value>(body) {
        let msg = v.get("message").unwrap_or(&v);
        let content = msg
            .get("content")
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string();
        let tool_calls = parse_tool_calls_from_message(msg);
        // Fase 13.1 — métricas de generación para `ai benchmark`.
        let eval_count = v.get("eval_count").and_then(|n| n.as_u64());
        let prompt_eval_count = v.get("prompt_eval_count").and_then(|n| n.as_u64());
        let eval_duration_ns = v.get("eval_duration").and_then(|n| n.as_u64());
        return Ok(LlmChatResponse {
            content,
            tool_calls,
            eval_count,
            prompt_eval_count,
            eval_duration_ns,
        });
    }

    // Caso 2: NDJSON línea a línea (por si el servidor responde en modo stream).
    let mut content = String::new();
    let mut tool_calls = Vec::new();
    let mut parsed_any = false;
    // Fase 13.1 — métricas: en NDJSON viven en la última línea (done:true).
    let mut eval_count = None;
    let mut prompt_eval_count = None;
    let mut eval_duration_ns = None;
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let v: Value = serde_json::from_str(trimmed).map_err(|e| {
            EnolaError::InfrastructureError(format!(
                "Respuesta JSON inválida de Ollama (/api/chat): {e}"
            ))
        })?;
        parsed_any = true;
        eval_count = v.get("eval_count").and_then(|n| n.as_u64()).or(eval_count);
        prompt_eval_count = v
            .get("prompt_eval_count")
            .and_then(|n| n.as_u64())
            .or(prompt_eval_count);
        eval_duration_ns = v
            .get("eval_duration")
            .and_then(|n| n.as_u64())
            .or(eval_duration_ns);
        let msg = v.get("message").unwrap_or(&v);
        if let Some(chunk) = msg.get("content").and_then(|c| c.as_str()) {
            content.push_str(chunk);
        }
        tool_calls.extend(parse_tool_calls_from_message(msg));
    }

    if !parsed_any {
        return Err(EnolaError::InfrastructureError(
            "Respuesta vacía de Ollama (/api/chat)".to_string(),
        ));
    }

    Ok(LlmChatResponse {
        content,
        tool_calls,
        eval_count,
        prompt_eval_count,
        eval_duration_ns,
    })
}

#[async_trait::async_trait]
impl LlmEngine for OllamaAdapter {
    #[tracing::instrument(skip(self, messages, tools), fields(endpoint = %self.endpoint, model = %model))]
    async fn chat_tools(
        &self,
        model: &str,
        messages: &[ChatMessage],
        tools: &[LlmToolDefinition],
    ) -> Result<LlmChatResponse> {
        let client = self.build_client()?;
        let url = format!("{}/api/chat", self.endpoint);

        let mut payload = serde_json::Map::new();
        payload.insert("model".to_string(), json!(model));
        payload.insert(
            "messages".to_string(),
            Value::Array(messages.iter().map(serialize_message).collect()),
        );
        payload.insert("stream".to_string(), json!(false));
        if !tools.is_empty() {
            payload.insert(
                "tools".to_string(),
                Value::Array(tools.iter().map(serialize_tool).collect()),
            );
        }

        let resp = client
            .post(&url)
            .json(&Value::Object(payload))
            .send()
            .await
            .map_err(|e| self.map_reqwest_error("/api/chat", &e))?;

        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| self.map_reqwest_error("/api/chat body", &e))?;

        if !status.is_success() {
            let detail = extract_ollama_error(&body).unwrap_or_else(|| body.trim().to_string());
            return Err(EnolaError::InfrastructureError(format!(
                "Ollama /api/chat devolvió HTTP {}: {}",
                status.as_u16(),
                detail
            )));
        }

        parse_chat_response_body(&body)
    }

    #[tracing::instrument(skip(self), fields(endpoint = %self.endpoint))]
    async fn list_models(&self) -> Result<Vec<LlmModelInfo>> {
        let client = self.build_client()?;
        let url = format!("{}/api/tags", self.endpoint);

        let resp = client
            .get(&url)
            .send()
            .await
            .map_err(|e| self.map_reqwest_error("/api/tags", &e))?;

        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| self.map_reqwest_error("/api/tags body", &e))?;

        if !status.is_success() {
            let detail = extract_ollama_error(&body).unwrap_or_else(|| body.trim().to_string());
            return Err(EnolaError::InfrastructureError(format!(
                "Ollama /api/tags devolvió HTTP {}: {}",
                status.as_u16(),
                detail
            )));
        }

        let parsed: Value = serde_json::from_str(&body).map_err(|e| {
            EnolaError::InfrastructureError(format!(
                "Respuesta JSON inválida de Ollama (/api/tags): {e}"
            ))
        })?;
        let models = parsed
            .get("models")
            .and_then(|m| m.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| {
                        let name = item.get("name")?.as_str()?.to_string();
                        let size_bytes = item.get("size").and_then(|s| s.as_u64());
                        let modified_at = item
                            .get("modified_at")
                            .and_then(|m| m.as_str())
                            .map(String::from);
                        Some(LlmModelInfo {
                            name,
                            size_bytes,
                            modified_at,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(models)
    }

    #[tracing::instrument(skip(self), fields(endpoint = %self.endpoint, model = %model))]
    async fn pull_model(&self, model: &str) -> Result<()> {
        let client = self.build_client()?;
        let url = format!("{}/api/pull", self.endpoint);
        let payload = json!({
            "name": model,
            "stream": false,
        });

        let resp = client
            .post(&url)
            .json(&payload)
            .send()
            .await
            .map_err(|e| self.map_reqwest_error("/api/pull", &e))?;

        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| self.map_reqwest_error("/api/pull body", &e))?;

        if !status.is_success() {
            let detail = extract_ollama_error(&body).unwrap_or_else(|| body.trim().to_string());
            return Err(EnolaError::InfrastructureError(format!(
                "Ollama /api/pull devolvió HTTP {}: {}",
                status.as_u16(),
                detail
            )));
        }
        if let Some(err) = extract_ollama_error(&body) {
            return Err(EnolaError::InfrastructureError(format!(
                "Ollama /api/pull error: {err}"
            )));
        }

        Ok(())
    }

    #[tracing::instrument(skip(self), fields(endpoint = %self.endpoint, model = %name))]
    async fn create_model_from_gguf(&self, name: &str, gguf: &std::path::Path) -> Result<()> {
        let filename = gguf
            .file_name()
            .and_then(|f| f.to_str())
            .ok_or_else(|| {
                EnolaError::ValidationError(format!(
                    "Ruta GGUF sin nombre de fichero válido: '{}'",
                    gguf.display()
                ))
            })?
            .to_string();
        let digest = self.push_blob(gguf).await?;

        let client = self.build_client()?;
        let url = format!("{}/api/create", self.endpoint);
        let payload = json!({
            "model": name,
            "files": { filename: digest },
            "stream": false,
        });

        let resp = client
            .post(&url)
            .json(&payload)
            .send()
            .await
            .map_err(|e| self.map_reqwest_error("/api/create", &e))?;

        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| self.map_reqwest_error("/api/create body", &e))?;

        if !status.is_success() {
            let detail = extract_ollama_error(&body).unwrap_or_else(|| body.trim().to_string());
            return Err(EnolaError::InfrastructureError(format!(
                "Ollama /api/create devolvió HTTP {}: {}",
                status.as_u16(),
                detail
            )));
        }
        if let Some(err) = extract_ollama_error(&body) {
            return Err(EnolaError::InfrastructureError(format!(
                "Ollama /api/create error: {err}"
            )));
        }
        Ok(())
    }

    #[tracing::instrument(skip(self), fields(endpoint = %self.endpoint, model = %name))]
    async fn delete_model(&self, name: &str) -> Result<()> {
        let client = self.build_client()?;
        let url = format!("{}/api/delete", self.endpoint);
        let payload = json!({ "name": name });

        let resp = client
            .delete(&url)
            .json(&payload)
            .send()
            .await
            .map_err(|e| self.map_reqwest_error("/api/delete", &e))?;

        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| self.map_reqwest_error("/api/delete body", &e))?;

        if !status.is_success() {
            let detail = extract_ollama_error(&body).unwrap_or_else(|| body.trim().to_string());
            return Err(EnolaError::InfrastructureError(format!(
                "Ollama /api/delete devolvió HTTP {}: {}",
                status.as_u16(),
                detail
            )));
        }
        if let Some(err) = extract_ollama_error(&body) {
            return Err(EnolaError::InfrastructureError(format!(
                "Ollama /api/delete error: {err}"
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::llm::ChatRole;
    use serde_json::{json, Value};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// Levanta un servidor HTTP efímero en `127.0.0.1:0` que responde una petición
    /// con `status_line` y `body`, devolviendo la URL base y un handle que captura
    /// la petición cruda recibida.
    async fn spawn_one_shot_http(
        status_line: &'static str,
        body: String,
        delay: Duration,
    ) -> (String, tokio::task::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let url = format!("http://127.0.0.1:{}", addr.port());
        let handle = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 16384];
            let mut raw = String::new();
            let mut header_end = None;
            let mut content_len = 0usize;

            loop {
                let n = socket.read(&mut buf).await.unwrap_or(0);
                if n == 0 {
                    break;
                }
                raw.push_str(&String::from_utf8_lossy(&buf[..n]));
                if header_end.is_none() {
                    if let Some(pos) = raw.find("\r\n\r\n") {
                        header_end = Some(pos + 4);
                        for line in raw[..pos].lines() {
                            if let Some((k, v)) = line.split_once(':') {
                                if k.trim().eq_ignore_ascii_case("content-length") {
                                    content_len = v.trim().parse().unwrap_or(0);
                                }
                            }
                        }
                    }
                }
                if let Some(end) = header_end {
                    if raw.len() >= end + content_len {
                        break;
                    }
                }
            }

            if !delay.is_zero() {
                tokio::time::sleep(delay).await;
            }
            let resp = format!(
                "HTTP/1.1 {status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = socket.write_all(resp.as_bytes()).await;
            let _ = socket.shutdown().await;
            raw
        });
        (url, handle)
    }

    #[tokio::test]
    async fn chat_tools_parses_plain_text_response_and_sends_tools() {
        let body = json!({
            "model": "llama3.1:8b",
            "message": {
                "role": "assistant",
                "content": "Tienes 2 servicios Tor activos."
            },
            "done": true
        })
        .to_string();
        let (url, handle) = spawn_one_shot_http("200 OK", body, Duration::ZERO).await;
        let adapter = OllamaAdapter::new(&url, Duration::from_secs(5));
        let tools = vec![LlmToolDefinition {
            name: "tor_list".to_string(),
            description: "List Tor hidden services".to_string(),
            parameters: json!({"type": "object", "properties": {}}),
        }];
        let resp = adapter
            .chat_tools("llama3.1:8b", &[ChatMessage::user("lista tor")], &tools)
            .await
            .unwrap();

        assert_eq!(resp.content, "Tienes 2 servicios Tor activos.");
        assert!(resp.tool_calls.is_empty());

        let raw_req = handle.await.unwrap();
        assert!(raw_req.starts_with("POST /api/chat HTTP/1.1"));
        let req_body = raw_req.split("\r\n\r\n").nth(1).unwrap();
        let parsed: Value = serde_json::from_str(req_body).unwrap();
        assert_eq!(parsed["model"], "llama3.1:8b");
        assert_eq!(parsed["stream"], false);
        assert_eq!(parsed["tools"][0]["type"], "function");
        assert_eq!(parsed["tools"][0]["function"]["name"], "tor_list");
    }

    #[tokio::test]
    async fn chat_tools_parses_tool_calls_from_assistant_message() {
        let body = json!({
            "model": "llama3.1:8b",
            "message": {
                "role": "assistant",
                "content": "",
                "tool_calls": [
                    {
                        "function": {
                            "name": "wp_status",
                            "arguments": { "name": "miblog" }
                        }
                    }
                ]
            },
            "done": true
        })
        .to_string();
        let (url, _handle) = spawn_one_shot_http("200 OK", body, Duration::ZERO).await;
        let adapter = OllamaAdapter::new(&url, Duration::from_secs(5));
        let resp = adapter
            .chat_tools(
                "llama3.1:8b",
                &[
                    ChatMessage::system("Eres el asistente de Enola"),
                    ChatMessage::user("estado de miblog"),
                ],
                &[],
            )
            .await
            .unwrap();

        assert_eq!(resp.tool_calls.len(), 1);
        assert_eq!(resp.tool_calls[0].name, "wp_status");
        assert_eq!(resp.tool_calls[0].arguments, json!({"name": "miblog"}));
    }

    #[tokio::test]
    async fn chat_tools_handles_streaming_ndjson_chunks() {
        let ndjson = format!(
            "{}\n{}\n",
            json!({
                "model": "llama3.1:8b",
                "message": { "role": "assistant", "content": "Parte 1 " },
                "done": false
            }),
            json!({
                "model": "llama3.1:8b",
                "message": { "role": "assistant", "content": "y Parte 2." },
                "done": true
            })
        );
        let (url, _handle) = spawn_one_shot_http("200 OK", ndjson, Duration::ZERO).await;
        let adapter = OllamaAdapter::new(&url, Duration::from_secs(5));
        let resp = adapter
            .chat_tools(
                "llama3.1:8b",
                &[ChatMessage {
                    role: ChatRole::User,
                    content: "hola".to_string(),
                    tool_calls: vec![],
                    tool_name: None,
                }],
                &[],
            )
            .await
            .unwrap();

        assert_eq!(resp.content, "Parte 1 y Parte 2.");
    }

    #[tokio::test]
    async fn list_models_parses_api_tags_response() {
        let body = json!({
            "models": [
                {
                    "name": "llama3.1:8b",
                    "size": 4_700_000_000u64,
                    "modified_at": "2026-10-18T10:00:00Z"
                },
                {
                    "name": "qwen2.5:7b",
                    "size": 4_200_000_000u64
                }
            ]
        })
        .to_string();
        let (url, handle) = spawn_one_shot_http("200 OK", body, Duration::ZERO).await;
        let adapter = OllamaAdapter::new(&url, Duration::from_secs(5));
        let models = adapter.list_models().await.unwrap();
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].name, "llama3.1:8b");
        assert_eq!(models[0].size_bytes, Some(4_700_000_000));
        assert_eq!(models[1].name, "qwen2.5:7b");

        let raw_req = handle.await.unwrap();
        assert!(raw_req.starts_with("GET /api/tags HTTP/1.1"));
    }

    #[tokio::test]
    async fn pull_model_succeeds_on_valid_stream_or_json() {
        let body = format!(
            "{}\n{}\n",
            json!({"status": "pulling manifest"}),
            json!({"status": "success"})
        );
        let (url, handle) = spawn_one_shot_http("200 OK", body, Duration::ZERO).await;
        let adapter = OllamaAdapter::new(&url, Duration::from_secs(5));
        adapter.pull_model("llama3.1:8b").await.unwrap();

        let raw_req = handle.await.unwrap();
        assert!(raw_req.starts_with("POST /api/pull HTTP/1.1"));
    }

    #[tokio::test]
    async fn chat_tools_returns_error_on_http_error_or_ollama_error_field() {
        let body =
            json!({"error": "model 'missing:7b' not found, try pulling it first"}).to_string();
        let (url, _handle) = spawn_one_shot_http("404 Not Found", body, Duration::ZERO).await;
        let adapter = OllamaAdapter::new(&url, Duration::from_secs(5));
        let err = adapter
            .chat_tools("missing:7b", &[ChatMessage::user("hola")], &[])
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("missing:7b"),
            "error debe incluir el detalle de Ollama: {err}"
        );
    }

    #[tokio::test]
    async fn chat_tools_returns_timeout_when_server_exceeds_deadline() {
        let body =
            json!({"message": {"role": "assistant", "content": "tarde"}, "done": true}).to_string();
        let (url, _handle) = spawn_one_shot_http("200 OK", body, Duration::from_millis(350)).await;
        let adapter = OllamaAdapter::new(&url, Duration::from_millis(80));
        let err = adapter
            .chat_tools("llama3.1:8b", &[ChatMessage::user("hola")], &[])
            .await
            .unwrap_err();
        assert!(
            matches!(err, EnolaError::Timeout(_)),
            "esperado EnolaError::Timeout, recibido: {err:?}"
        );
    }

    #[tokio::test]
    async fn onion_endpoint_uses_centralized_http_builder_with_socks5h_guard() {
        let (proxy_val, res) = {
            let _g = crate::infrastructure::TEST_ENV_LOCK.lock().unwrap();
            let old = std::env::var("ENOLA_TOR_SOCKS_PROXY").ok();
            std::env::set_var("ENOLA_TOR_SOCKS_PROXY", "socks5://127.0.0.1:9050");
            let adapter =
                OllamaAdapter::new("http://ollamaabcdef.onion:11434", Duration::from_secs(2));
            // Construimos el cliente síncronamente bajo el lock para verificar el guard socks5h.
            let client_res = crate::infrastructure::http::http_client_builder(adapter.endpoint());
            match old {
                Some(v) => std::env::set_var("ENOLA_TOR_SOCKS_PROXY", v),
                None => std::env::remove_var("ENOLA_TOR_SOCKS_PROXY"),
            }
            (adapter.endpoint().to_string(), client_res)
        };
        assert_eq!(proxy_val, "http://ollamaabcdef.onion:11434");
        assert!(res.is_err(), "socks5:// sin h debe rechazarse para .onion");
    }

    /// Petición capturada por `spawn_routed_http`: método, ruta y cuerpo crudo.
    #[derive(Debug, Clone)]
    struct CapturedRequest {
        method: String,
        path: String,
        body: Vec<u8>,
    }

    /// Servidor HTTP efímero que atiende `routes.len()` conexiones en orden
    /// (una petición por conexión, `Connection: close`). Cada ruta es
    /// `(status_line, body)` y responde a la petición de esa posición.
    async fn spawn_routed_http(
        routes: Vec<(&'static str, String)>,
    ) -> (String, tokio::task::JoinHandle<Vec<CapturedRequest>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let handle = tokio::spawn(async move {
            let mut captured = Vec::new();
            for (status_line, body) in routes {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut raw: Vec<u8> = Vec::new();
                let mut buf = vec![0u8; 65536];
                let (header_end, content_len) = loop {
                    let n = socket.read(&mut buf).await.unwrap_or(0);
                    assert!(n > 0, "conexión cerrada antes de las cabeceras");
                    raw.extend_from_slice(&buf[..n]);
                    if let Some(pos) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&raw[..pos]).to_string();
                        let len = head
                            .lines()
                            .filter_map(|l| l.split_once(':'))
                            .find(|(k, _)| k.trim().eq_ignore_ascii_case("content-length"))
                            .and_then(|(_, v)| v.trim().parse::<usize>().ok())
                            .unwrap_or(0);
                        break (pos + 4, len);
                    }
                };
                while raw.len() < header_end + content_len {
                    let n = socket.read(&mut buf).await.unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    raw.extend_from_slice(&buf[..n]);
                }
                let head = String::from_utf8_lossy(&raw[..header_end]).to_string();
                let mut first = head.lines().next().unwrap_or("").split_whitespace();
                captured.push(CapturedRequest {
                    method: first.next().unwrap_or("").to_string(),
                    path: first.next().unwrap_or("").to_string(),
                    body: raw[header_end..].to_vec(),
                });
                let resp = format!(
                    "HTTP/1.1 {status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = socket.write_all(resp.as_bytes()).await;
                let _ = socket.shutdown().await;
            }
            captured
        });
        (url, handle)
    }

    fn write_fake_gguf(dir: &std::path::Path, name: &str) -> (std::path::PathBuf, Vec<u8>, String) {
        use sha2::{Digest, Sha256};
        let bytes = b"GGUF\x03\x00\x00\x00 fake model bytes".repeat(1000);
        let path = dir.join(name);
        std::fs::write(&path, &bytes).unwrap();
        let digest = format!("sha256:{:x}", Sha256::digest(&bytes));
        (path, bytes, digest)
    }

    #[tokio::test]
    async fn create_model_from_gguf_uploads_blob_then_creates_with_files() {
        let dir = tempfile::tempdir().unwrap();
        let (path, bytes, digest) = write_fake_gguf(dir.path(), "m.gguf");
        let (url, handle) = spawn_routed_http(vec![
            ("404 Not Found", String::new()),
            ("201 Created", String::new()),
            ("200 OK", json!({"status": "success"}).to_string()),
        ])
        .await;
        let adapter = OllamaAdapter::new(url, Duration::from_secs(10));
        adapter
            .create_model_from_gguf("dev-ft", &path)
            .await
            .expect("registro vía blob + files");

        let reqs = handle.await.unwrap();
        let blob_path = format!("/api/blobs/{digest}");
        assert_eq!(
            (reqs[0].method.as_str(), reqs[0].path.as_str()),
            ("HEAD", blob_path.as_str())
        );
        assert_eq!(
            (reqs[1].method.as_str(), reqs[1].path.as_str()),
            ("POST", blob_path.as_str())
        );
        assert_eq!(
            reqs[1].body, bytes,
            "el blob subido debe ser el fichero íntegro"
        );
        assert_eq!(
            (reqs[2].method.as_str(), reqs[2].path.as_str()),
            ("POST", "/api/create")
        );
        let payload: Value = serde_json::from_slice(&reqs[2].body).unwrap();
        assert_eq!(payload["model"], "dev-ft");
        assert_eq!(payload["files"], json!({ "m.gguf": digest }));
        assert_eq!(payload["stream"], false);
        assert!(
            payload.get("modelfile").is_none(),
            "Ollama >= 0.5 rechaza `modelfile` (H-16)"
        );
    }

    #[tokio::test]
    async fn create_model_from_gguf_skips_upload_when_blob_exists() {
        let dir = tempfile::tempdir().unwrap();
        let (path, _bytes, digest) = write_fake_gguf(dir.path(), "m.gguf");
        let (url, handle) = spawn_routed_http(vec![
            ("200 OK", String::new()),
            ("200 OK", json!({"status": "success"}).to_string()),
        ])
        .await;
        let adapter = OllamaAdapter::new(url, Duration::from_secs(10));
        adapter
            .create_model_from_gguf("dev-ft", &path)
            .await
            .unwrap();

        let reqs = handle.await.unwrap();
        assert_eq!(reqs.len(), 2);
        assert_eq!(reqs[0].method, "HEAD");
        assert_eq!(reqs[0].path, format!("/api/blobs/{digest}"));
        assert_eq!(reqs[1].path, "/api/create");
    }

    #[tokio::test]
    async fn create_model_from_gguf_surfaces_ollama_error() {
        let dir = tempfile::tempdir().unwrap();
        let (path, _bytes, _digest) = write_fake_gguf(dir.path(), "m.gguf");
        let (url, _handle) = spawn_routed_http(vec![
            ("200 OK", String::new()),
            (
                "400 Bad Request",
                json!({"error": "invalid model"}).to_string(),
            ),
        ])
        .await;
        let adapter = OllamaAdapter::new(url, Duration::from_secs(10));
        let err = adapter
            .create_model_from_gguf("dev-ft", &path)
            .await
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("HTTP 400") && err.contains("invalid model"),
            "{err}"
        );
    }

    #[tokio::test]
    async fn create_model_from_gguf_fails_on_blob_upload_error() {
        let dir = tempfile::tempdir().unwrap();
        let (path, _bytes, _digest) = write_fake_gguf(dir.path(), "m.gguf");
        let (url, handle) = spawn_routed_http(vec![
            ("404 Not Found", String::new()),
            (
                "400 Bad Request",
                json!({"error": "digest mismatch"}).to_string(),
            ),
        ])
        .await;
        let adapter = OllamaAdapter::new(url, Duration::from_secs(10));
        let err = adapter
            .create_model_from_gguf("dev-ft", &path)
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("digest mismatch"), "{err}");
        assert_eq!(
            handle.await.unwrap().len(),
            2,
            "no debe llamar a /api/create"
        );
    }

    /// Integración contra un Ollama real (H-16): `ENOLA_TEST_OLLAMA_ENDPOINT`
    /// (p.ej. `http://127.0.0.1:11999`) y `ENOLA_TEST_GGUF` (GGUF válido).
    #[tokio::test]
    #[ignore = "requires real Ollama + GGUF (ENOLA_TEST_OLLAMA_ENDPOINT, ENOLA_TEST_GGUF)"]
    async fn create_model_from_gguf_against_real_ollama() {
        let endpoint = std::env::var("ENOLA_TEST_OLLAMA_ENDPOINT").unwrap();
        let gguf = std::path::PathBuf::from(std::env::var("ENOLA_TEST_GGUF").unwrap());
        let adapter = OllamaAdapter::new(endpoint, Duration::from_secs(120));
        adapter
            .create_model_from_gguf("enola-h16-test", &gguf)
            .await
            .expect("registro real vía blob + files");
        // Idempotente: la segunda vez el blob ya existe y no se re-sube.
        adapter
            .create_model_from_gguf("enola-h16-test", &gguf)
            .await
            .expect("re-registro con blob existente");
        let models = adapter.list_models().await.unwrap();
        assert!(
            models.iter().any(|m| m.name.starts_with("enola-h16-test")),
            "{models:?}"
        );
    }

    #[test]
    fn with_defaults_sets_expected_endpoint_and_timeout() {
        let adapter = OllamaAdapter::with_defaults();
        assert_eq!(adapter.endpoint(), DEFAULT_OLLAMA_ENDPOINT);
        assert_eq!(
            adapter.timeout(),
            Duration::from_secs(DEFAULT_OLLAMA_TIMEOUT_SECS)
        );
        assert_eq!(DEFAULT_OLLAMA_MODEL, "llama3.1:8b");
    }
}
