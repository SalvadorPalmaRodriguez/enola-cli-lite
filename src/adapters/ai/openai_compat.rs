//! Fase 13.2 — Adapter `ExternalAiPort` para APIs OpenAI-compatibles.
//!
//! Cubre OpenAI, xAI Grok, Groq y OpenRouter — todos hablan
//! `POST {endpoint}/chat/completions` con `Authorization: Bearer <key>`.
//! ⚠️ La key NUNCA se registra ni aparece en mensajes de error.

use crate::domain::ai_providers::ExternalProvider;
use crate::domain::error::{EnolaError, Result};
use crate::ports::external_ai::{ExternalAiPort, ProviderTestResult};
use crate::ports::llm::ChatMessage;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

/// Timeout defensivo para llamadas a providers externos.
const PROVIDER_TIMEOUT_SECS: u64 = 120;

/// Adapter OpenAI-compatible (openai/grok/groq/openrouter/custom).
/// `use_tor` enruta por SOCKS5h para que la IP de origen no se exponga.
pub struct OpenAiCompatAdapter {
    client: reqwest::Client,
    /// `true` si el cliente saldrá por el proxy SOCKS5h de Tor.
    via_tor: bool,
}

impl OpenAiCompatAdapter {
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

    fn chat_url(endpoint: &str) -> String {
        format!("{}/chat/completions", endpoint.trim_end_matches('/'))
    }

    /// Error que NUNCA incluye la API key ni el cuerpo completo (recortado).
    fn http_err(ctx: &str, status: reqwest::StatusCode, body: &str) -> EnolaError {
        let detail = serde_json::from_str::<Value>(body)
            .ok()
            .and_then(|v| {
                v.pointer("/error/message")
                    .and_then(|m| m.as_str().map(str::to_string))
            })
            .unwrap_or_else(|| body.trim().chars().take(200).collect());
        EnolaError::InfrastructureError(format!(
            "Provider {ctx} → HTTP {}: {detail}",
            status.as_u16()
        ))
    }
}

#[async_trait::async_trait]
impl ExternalAiPort for OpenAiCompatAdapter {
    async fn chat(
        &self,
        provider: ExternalProvider,
        endpoint: &str,
        model: &str,
        api_key: &str,
        messages: &[ChatMessage],
        system_prompt: Option<String>,
    ) -> Result<String> {
        if !provider.is_openai_compatible() {
            return Err(EnolaError::ValidationError(format!(
                "{:?} no habla el formato OpenAI-compatible",
                provider
            )));
        }
        let mut msgs: Vec<Value> = Vec::with_capacity(messages.len() + 1);
        if let Some(sys) = system_prompt {
            msgs.push(json!({"role": "system", "content": sys}));
        }
        for m in messages {
            msgs.push(json!({"role": m.role, "content": m.content}));
        }
        let resp = self
            .client
            .post(Self::chat_url(endpoint))
            .bearer_auth(api_key)
            .timeout(Duration::from_secs(PROVIDER_TIMEOUT_SECS))
            .json(&json!({
                "model": model,
                "messages": msgs,
                "stream": false,
            }))
            .send()
            .await
            .map_err(|e| {
                EnolaError::InfrastructureError(format!(
                    "Provider {} chat falló (¿red/endpoint?): {e}",
                    provider.as_str()
                ))
            })?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(Self::http_err("chat", status, &body));
        }
        let v: Value = serde_json::from_str(&body).map_err(|e| {
            EnolaError::InfrastructureError(format!("Provider JSON chat inválido: {e}"))
        })?;
        v.pointer("/choices/0/message/content")
            .and_then(|c| c.as_str())
            .map(str::to_string)
            .ok_or_else(|| {
                EnolaError::InfrastructureError(
                    "Provider sin content en la respuesta chat".to_string(),
                )
            })
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
            .post(Self::chat_url(endpoint))
            .bearer_auth(api_key)
            .timeout(Duration::from_secs(30))
            .json(&json!({
                "model": model,
                "messages": [{"role": "user", "content": "Reply with: ok"}],
                "max_tokens": 5,
                "stream": false,
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
    use crate::domain::ai_providers::ExternalProvider as P;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// Stub HTTP que valida el Bearer y responde un chat/completions mínimo.
    async fn spawn_openai_stub(expected_auth: String) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let expected = expected_auth.clone();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 16384];
                    let n = socket.read(&mut buf).await.unwrap_or(0);
                    let raw = String::from_utf8_lossy(&buf[..n]);
                    let (status, body) = if raw.to_lowercase().contains(&format!(
                        "authorization: bearer {}",
                        expected.to_lowercase()
                    )) {
                        (
                            "200 OK",
                            r#"{"choices":[{"message":{"content":"pong"}}]}"#.to_string(),
                        )
                    } else {
                        (
                            "401 Unauthorized",
                            r#"{"error":{"message":"invalid api key"}}"#.to_string(),
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
    async fn chat_sends_bearer_and_parses_content() {
        let port = spawn_openai_stub("test_k9".to_string()).await;
        let a = OpenAiCompatAdapter::new(&format!("http://127.0.0.1:{port}"), false).unwrap();
        let out = a
            .chat(
                P::OpenAi,
                &format!("http://127.0.0.1:{port}"),
                "gpt-4o-mini",
                "test_k9",
                &[ChatMessage::user("hi")],
                None,
            )
            .await
            .unwrap();
        assert_eq!(out, "pong");
    }

    #[tokio::test]
    async fn chat_401_never_leaks_key_in_error() {
        let port = spawn_openai_stub("test_k9".to_string()).await;
        let a = OpenAiCompatAdapter::new(&format!("http://127.0.0.1:{port}"), false).unwrap();
        let err = a
            .chat(
                P::OpenAi,
                &format!("http://127.0.0.1:{port}"),
                "gpt-4o-mini",
                "WRONG_SECRET_XYZ",
                &[ChatMessage::user("hi")],
                None,
            )
            .await
            .unwrap_err();
        let msg = err.to_string();
        assert!(!msg.contains("WRONG_SECRET_XYZ"), "key leaked: {msg}");
        assert!(msg.contains("401"));
    }

    #[tokio::test]
    async fn test_connection_reports_success_and_failure() {
        let port = spawn_openai_stub("test_k9".to_string()).await;
        let a = OpenAiCompatAdapter::new(&format!("http://127.0.0.1:{port}"), false).unwrap();
        let ok = a
            .test_connection(P::Groq, &format!("http://127.0.0.1:{port}"), "m", "test_k9")
            .await
            .unwrap();
        assert!(ok.success);
        let bad = a
            .test_connection(P::Groq, &format!("http://127.0.0.1:{port}"), "m", "wrong")
            .await
            .unwrap();
        assert!(!bad.success && bad.error.is_some());
        assert!(!bad.error.unwrap().contains("wrong"));
    }

    #[tokio::test]
    async fn anthropic_is_rejected_by_openai_adapter() {
        let a = OpenAiCompatAdapter::new("http://127.0.0.1:1", false).unwrap();
        assert!(a
            .chat(P::Anthropic, "http://x", "m", "k", &[], None)
            .await
            .is_err());
    }
}
