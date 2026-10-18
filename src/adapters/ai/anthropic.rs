//! Fase 13.2 — Adapter `ExternalAiPort` para la API Messages de Anthropic.
//!
//! `POST {endpoint}/messages` con `x-api-key` + `anthropic-version`.
//! Formato distinto al de OpenAI: `system` es campo de nivel superior y
//! los mensajes no incluyen roles `system`. La key NUNCA se loggea.

use crate::domain::ai_providers::ExternalProvider;
use crate::domain::error::{EnolaError, Result};
use crate::ports::external_ai::{ExternalAiPort, ProviderTestResult};
use crate::ports::llm::ChatMessage;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

/// Versión de la API Messages de Anthropic.
const ANTHROPIC_VERSION: &str = "2023-06-01";
/// Timeout defensivo para llamadas a Anthropic.
const PROVIDER_TIMEOUT_SECS: u64 = 120;

/// Adapter de la API Messages de Anthropic.
pub struct AnthropicAdapter {
    client: reqwest::Client,
    /// `true` si el cliente saldrá por el proxy SOCKS5h de Tor.
    via_tor: bool,
}

impl AnthropicAdapter {
    /// Construye el adapter; `endpoint` decide el cliente HTTP (Tor o directo).
    pub fn new(endpoint: &str, use_tor: bool) -> Result<Self> {
        let client = if use_tor {
            crate::infrastructure::http::build_download_client_via_tor(endpoint)?
        } else {
            crate::infrastructure::http::build_http_client(endpoint)?
        };
        Ok(Self {
            client,
            via_tor: use_tor,
        })
    }

    /// `true` si las peticiones salen por SOCKS5h.
    pub fn is_via_tor(&self) -> bool {
        self.via_tor
    }

    fn messages_url(endpoint: &str) -> String {
        format!("{}/messages", endpoint.trim_end_matches('/'))
    }

    fn http_err(ctx: &str, status: reqwest::StatusCode, body: &str) -> EnolaError {
        let detail = serde_json::from_str::<Value>(body)
            .ok()
            .and_then(|v| {
                v.pointer("/error/message")
                    .and_then(|m| m.as_str().map(str::to_string))
            })
            .unwrap_or_else(|| body.trim().chars().take(200).collect());
        EnolaError::InfrastructureError(format!(
            "Anthropic {ctx} → HTTP {}: {detail}",
            status.as_u16()
        ))
    }
}

#[async_trait::async_trait]
impl ExternalAiPort for AnthropicAdapter {
    async fn chat(
        &self,
        provider: ExternalProvider,
        endpoint: &str,
        model: &str,
        api_key: &str,
        messages: &[ChatMessage],
        system_prompt: Option<String>,
    ) -> Result<String> {
        if provider != ExternalProvider::Anthropic {
            return Err(EnolaError::ValidationError(format!(
                "{:?} no usa la API Messages de Anthropic",
                provider
            )));
        }
        let msgs: Vec<Value> = messages
            .iter()
            .map(|m| json!({"role": m.role, "content": m.content}))
            .collect();
        let mut body = json!({
            "model": model,
            "max_tokens": 1024,
            "messages": msgs,
        });
        if let Some(sys) = system_prompt {
            body["system"] = json!(sys);
        }
        let resp = self
            .client
            .post(Self::messages_url(endpoint))
            .header("x-api-key", api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .timeout(Duration::from_secs(PROVIDER_TIMEOUT_SECS))
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                EnolaError::InfrastructureError(format!(
                    "Anthropic chat falló (¿red/endpoint?): {e}"
                ))
            })?;
        let status = resp.status();
        let raw = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(Self::http_err("chat", status, &raw));
        }
        let v: Value = serde_json::from_str(&raw).map_err(|e| {
            EnolaError::InfrastructureError(format!("Anthropic JSON inválido: {e}"))
        })?;
        // Formato Messages: content es un array de bloques {type:"text", text:…}
        let text: String = v["content"]
            .as_array()
            .map(|blocks| {
                blocks
                    .iter()
                    .filter_map(|b| b["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("")
            })
            .unwrap_or_default();
        if text.is_empty() {
            return Err(EnolaError::InfrastructureError(
                "Anthropic devolvió contenido vacío".to_string(),
            ));
        }
        Ok(text)
    }

    async fn test_connection(
        &self,
        provider: ExternalProvider,
        endpoint: &str,
        model: &str,
        api_key: &str,
    ) -> Result<ProviderTestResult> {
        let started = Instant::now();
        let resp = self
            .client
            .post(Self::messages_url(endpoint))
            .header("x-api-key", api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .timeout(Duration::from_secs(30))
            .json(&json!({
                "model": model,
                "max_tokens": 5,
                "messages": [{"role": "user", "content": "Reply with: ok"}],
            }))
            .send()
            .await;
        let latency_ms = started.elapsed().as_millis() as u64;
        match resp {
            Ok(r) if r.status().is_success() => Ok(ProviderTestResult {
                provider: provider.as_str().to_string(),
                model: model.to_string(),
                latency_ms,
                success: true,
                error: None,
            }),
            Ok(r) => {
                let status = r.status();
                let body = r.text().await.unwrap_or_default();
                Ok(ProviderTestResult {
                    provider: provider.as_str().to_string(),
                    model: model.to_string(),
                    latency_ms,
                    success: false,
                    error: Some(Self::http_err("test", status, &body).to_string()),
                })
            }
            Err(e) => Ok(ProviderTestResult {
                provider: provider.as_str().to_string(),
                model: model.to_string(),
                latency_ms,
                success: false,
                error: Some(format!("connection error: {e}")),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    async fn spawn_anthropic_stub(expected_key: String) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let expected = expected_key.clone();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 16384];
                    let n = socket.read(&mut buf).await.unwrap_or(0);
                    let raw = String::from_utf8_lossy(&buf[..n]);
                    let lower = raw.to_lowercase();
                    let (status, body) = if lower
                        .contains(&format!("x-api-key: {}", expected.to_lowercase()))
                        && lower.contains("anthropic-version:")
                    {
                        (
                            "200 OK",
                            r#"{"content":[{"type":"text","text":"pong"}],"role":"assistant"}"#
                                .to_string(),
                        )
                    } else {
                        (
                            "401 Unauthorized",
                            r#"{"error":{"message":"invalid x-api-key"}}"#.to_string(),
                        )
                    };
                    let resp = format!(
                        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = socket.write_all(resp.as_bytes()).await;
                    let _ = socket.shutdown().await;
                });
            }
        });
        port
    }

    #[tokio::test]
    async fn chat_sends_x_api_key_and_parses_content_blocks() {
        let port = spawn_anthropic_stub("sk-ant-test".to_string()).await;
        let a = AnthropicAdapter::new(&format!("http://127.0.0.1:{port}"), false).unwrap();
        let out = a
            .chat(
                ExternalProvider::Anthropic,
                &format!("http://127.0.0.1:{port}"),
                "claude-haiku-4-5",
                "sk-ant-test",
                &[ChatMessage::user("hi")],
                Some("be brief".to_string()),
            )
            .await
            .unwrap();
        assert_eq!(out, "pong");
    }

    #[tokio::test]
    async fn chat_401_never_leaks_key() {
        let port = spawn_anthropic_stub("sk-ant-test".to_string()).await;
        let a = AnthropicAdapter::new(&format!("http://127.0.0.1:{port}"), false).unwrap();
        let err = a
            .chat(
                ExternalProvider::Anthropic,
                &format!("http://127.0.0.1:{port}"),
                "m",
                "WRONG_KEY_ABC",
                &[ChatMessage::user("hi")],
                None,
            )
            .await
            .unwrap_err();
        assert!(!err.to_string().contains("WRONG_KEY_ABC"));
    }
}
