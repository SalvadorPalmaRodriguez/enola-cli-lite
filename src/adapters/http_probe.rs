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
    classify_failure(&msg, &e.to_string(), e.is_connect() || e.is_request())
}

/// Clasifica el texto (en minúsculas) de la cadena de errores de reqwest.
///
/// Distingue dos situaciones que ambas mencionan «socks proxy»:
/// - **Proxy inalcanzable** (`failed to create underlying connection`): el
///   daemon Tor no escucha. No se arregla esperando → `ProxyUnavailable`.
/// - **Tor respondió con un error SOCKS** (`SOCKS error: host unreachable`,
///   `ttl expired`, …): el proxy funciona pero el destino no se alcanza, p. ej.
///   una `.onion` recién creada cuyo descriptor aún no se ha propagado. Tor
///   contesta al instante y reintentar SÍ lo arregla → `SiteUnreachable`.
fn classify_failure(msg_lower: &str, display: &str, connect_like: bool) -> ProbeError {
    if msg_lower.contains("failed to create underlying connection") {
        ProbeError::ProxyUnavailable(display.to_string())
    } else if msg_lower.contains("socks error") {
        ProbeError::SiteUnreachable(display.to_string())
    } else if msg_lower.contains("proxy") || msg_lower.contains("socks") {
        ProbeError::ProxyUnavailable(display.to_string())
    } else if connect_like {
        ProbeError::SiteUnreachable(display.to_string())
    } else {
        ProbeError::Other(display.to_string())
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

    // Textos reales medidos contra Tor (reqwest 0.12 + socks).
    const TOR_HOST_UNREACHABLE: &str = "error sending request for url (http://x.onion/) | client error (connect) | error connecting to socks proxy | socks error: host unreachable";
    const TOR_PROXY_DOWN: &str = "error sending request for url (http://x.onion/) | client error (connect) | error connecting to socks proxy | socks error: failed to create underlying connection";

    #[test]
    fn tor_host_unreachable_is_retryable_site_error_not_proxy_down() {
        let e = classify_failure(TOR_HOST_UNREACHABLE, "x", true);
        assert!(matches!(e, ProbeError::SiteUnreachable(_)), "{e:?}");
    }

    #[test]
    fn socks_replies_from_a_live_tor_are_site_errors() {
        for reply in [
            "ttl expired",
            "connection refused",
            "network unreachable",
            "general failure",
        ] {
            let msg = format!(
                "client error (connect) | error connecting to socks proxy | socks error: {reply}"
            );
            let e = classify_failure(&msg, "x", true);
            assert!(
                matches!(e, ProbeError::SiteUnreachable(_)),
                "{reply}: {e:?}"
            );
        }
    }

    #[test]
    fn unreachable_proxy_stays_proxy_unavailable() {
        let e = classify_failure(TOR_PROXY_DOWN, "x", true);
        assert!(matches!(e, ProbeError::ProxyUnavailable(_)), "{e:?}");
    }

    #[test]
    fn other_proxy_mentions_stay_proxy_unavailable_and_plain_connect_is_site() {
        assert!(matches!(
            classify_failure("invalid proxy uri", "x", false),
            ProbeError::ProxyUnavailable(_)
        ));
        assert!(matches!(
            classify_failure("tcp connect error: connection refused", "x", true),
            ProbeError::SiteUnreachable(_)
        ));
        assert!(matches!(
            classify_failure("bad body", "x", false),
            ProbeError::Other(_)
        ));
    }
}
