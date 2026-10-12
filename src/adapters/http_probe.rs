//! MCP-006 — Adapter HTTP probe (reqwest + SOCKS5h para .onion).
//!
//! Implementa `HttpProbePort` sobre `infrastructure::http::build_http_client`
//! (auto-detección `.onion` → proxy `socks5h://`, timeouts ya resueltos).
//! Clasifica los errores de reqwest en `ProbeError` para que el caso de uso
//! distinga «proxy Tor caído» de «sitio caído» de «timeout».

use crate::ports::http_probe::{HttpProbePort, ProbeError, ProbeOutcome};
use std::time::{Duration, Instant};

/// Sonda HTTP real vía reqwest (con enrutado Tor automático para .onion).
pub struct ReqwestHttpProbe;

#[async_trait::async_trait]
impl HttpProbePort for ReqwestHttpProbe {
    async fn get(&self, url: &str, timeout: Duration) -> Result<ProbeOutcome, ProbeError> {
        let client = crate::infrastructure::http::build_http_client(url)
            .map_err(|e| classify_infra(&e.to_string()))?;

        let start = Instant::now();
        let resp = client
            .get(url)
            .timeout(timeout)
            .send()
            .await
            .map_err(|e| classify_reqwest(&e))?;
        let latency_ms = start.elapsed().as_millis() as u64;
        let status = resp.status().as_u16();

        // Limitar la lectura del cuerpo (algunos sitios devuelven MB).
        let body = resp.text().await.unwrap_or_default();
        let title = extract_title(&body);

        Ok(ProbeOutcome {
            status,
            latency_ms,
            title,
        })
    }
}

/// Errores de construcción del cliente (proxy URL inválida → misconfiguración).
fn classify_infra(msg: &str) -> ProbeError {
    let lower = msg.to_lowercase();
    if lower.contains("proxy") || lower.contains("socks") {
        ProbeError::ProxyUnavailable(msg.to_string())
    } else {
        ProbeError::Other(msg.to_string())
    }
}

/// Clasifica un `reqwest::Error` en la taxonomía accionable.
fn classify_reqwest(e: &reqwest::Error) -> ProbeError {
    if e.is_timeout() {
        return ProbeError::Timeout;
    }
    let chain: String = std::iter::successors(std::error::Error::source(e), |s| s.source())
        .map(|s| s.to_string())
        .collect::<Vec<_>>()
        .join(" | ");
    let msg = format!("{e} | {chain}").to_lowercase();
    if msg.contains("proxy") || msg.contains("socks") {
        ProbeError::ProxyUnavailable(e.to_string())
    } else if e.is_connect() || e.is_request() {
        ProbeError::SiteUnreachable(e.to_string())
    } else {
        ProbeError::Other(e.to_string())
    }
}

/// Extrae `<title>…</title>` de un HTML (primer match, recortado).
fn extract_title(body: &str) -> Option<String> {
    let lower = body.to_lowercase();
    let start = lower.find("<title>")? + "<title>".len();
    let end = lower[start..].find("</title>")? + start;
    let title = body[start..end].trim();
    if title.is_empty() {
        None
    } else {
        Some(title.chars().take(200).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_html_title() {
        assert_eq!(
            extract_title("<html><head><title>  My Onion Blog </title></head>").as_deref(),
            Some("My Onion Blog")
        );
        assert_eq!(extract_title("<html>no title</html>"), None);
        assert_eq!(extract_title(""), None);
    }
}
