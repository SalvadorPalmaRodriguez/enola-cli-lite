// MCP-006 — verify_onion: sonda de alcanzabilidad de servicios .onion.
//
// Valida la dirección onion ANTES de salir a la red, sondea vía
// `HttpProbePort` (que enruta .onion por SOCKS5h en infrastructure::http) y
// devuelve un resultado estructurado. Reintenta con backoff porque un hidden
// service recién publicado tarda en propagarse por la red Tor.

use crate::domain::error::{EnolaError, Result};
use crate::ports::http_probe::{HttpProbePort, ProbeError};
use serde::Serialize;
use std::sync::Arc;
use std::time::Duration;

/// Timeout por intento de sonda.
const PROBE_TIMEOUT: Duration = Duration::from_secs(20);
/// Intentos por defecto (el primero + reintentos con backoff).
const DEFAULT_ATTEMPTS: u32 = 4;
/// Backoff base entre reintentos (intento n espera n·BACKOFF).
const RETRY_BACKOFF: Duration = Duration::from_secs(3);

/// Resultado estructurado de la verificación de una URL .onion.
#[derive(Debug, Clone, Serialize)]
pub struct OnionVerification {
    /// URL sondeada (normalizada con esquema).
    pub url: String,
    /// `true` si el servicio respondió algo (cualquier status HTTP).
    pub reachable: bool,
    /// Código HTTP devuelto (si hubo respuesta).
    pub status: Option<u16>,
    /// Latencia de la última petición en ms.
    pub latency_ms: Option<u64>,
    /// `<title>` del HTML servido (si lo hay).
    pub title: Option<String>,
    /// Motivo del fallo: `timeout`, `proxy_unavailable`, `site_unreachable`
    /// u `other`. `None` cuando `reachable` es true.
    pub reason: Option<String>,
    /// Número de intentos realizados.
    pub attempts: u32,
}

/// Verifica que una dirección .onion responde vía Tor (SOCKS5h).
pub struct VerifyOnion {
    probe: Arc<dyn HttpProbePort + Send + Sync>,
    attempts: u32,
    backoff: Duration,
}

impl VerifyOnion {
    /// Configura la sonda con el port HTTP inyectado.
    pub fn new(probe: Arc<dyn HttpProbePort + Send + Sync>) -> Self {
        Self {
            probe,
            attempts: DEFAULT_ATTEMPTS,
            backoff: RETRY_BACKOFF,
        }
    }

    /// Ajusta intentos/backoff (tests usan `backoff` 0 para no dormir).
    pub fn with_retry(mut self, attempts: u32, backoff: Duration) -> Self {
        self.attempts = attempts.max(1);
        self.backoff = backoff;
        self
    }

    /// Sondea `address` (admite `xxx…onion`, `http://xxx…onion`,
    /// `https://…`). La dirección se valida antes de cualquier acceso a red.
    #[tracing::instrument(skip(self), fields(address = %address))]
    pub async fn execute(&self, address: &str) -> Result<OnionVerification> {
        let url = normalize_onion_url(address)?;
        let mut last_reason = "other".to_string();
        let mut attempts_done = 0u32;

        for i in 0..self.attempts {
            attempts_done = i + 1;
            match self.probe.get(&url, PROBE_TIMEOUT).await {
                Ok(outcome) => {
                    return Ok(OnionVerification {
                        url,
                        reachable: true,
                        status: Some(outcome.status),
                        latency_ms: Some(outcome.latency_ms),
                        title: outcome.title,
                        reason: None,
                        attempts: attempts_done,
                    });
                }
                Err(e) => {
                    last_reason = reason_of(&e).to_string();
                    // No reintentar si el proxy Tor está caído: no se arregla
                    // esperando y el diagnóstico es distinto al de sitio caído.
                    if matches!(e, ProbeError::ProxyUnavailable(_)) || i + 1 == self.attempts {
                        break;
                    }
                    tokio::time::sleep(self.backoff).await;
                }
            }
        }

        Ok(OnionVerification {
            url,
            reachable: false,
            status: None,
            latency_ms: None,
            title: None,
            reason: Some(last_reason),
            attempts: attempts_done,
        })
    }
}

fn reason_of(e: &ProbeError) -> &'static str {
    match e {
        ProbeError::Timeout => "timeout",
        ProbeError::ProxyUnavailable(_) => "proxy_unavailable",
        ProbeError::SiteUnreachable(_) => "site_unreachable",
        ProbeError::Other(_) => "other",
    }
}

/// Valida y normaliza una dirección .onion.
///
/// Acepta `abc…xyz.onion`, `http://abc…xyz.onion` y `https://…`, y devuelve la
/// URL con esquema (`http://` por defecto). Rechaza hosts mal formados antes
/// de cualquier acceso a red: v3 = 56 chars base32 `[a-z2-7]`, v2 legacy = 16.
fn normalize_onion_url(address: &str) -> Result<String> {
    let trimmed = address.trim();
    if trimmed.is_empty() {
        return Err(EnolaError::ValidationError("empty onion address".into()));
    }
    let (scheme, rest) = match trimmed.split_once("://") {
        Some((s, r)) => (Some(s), r),
        None => (None, trimmed),
    };
    if let Some(s) = scheme {
        if s != "http" && s != "https" {
            return Err(EnolaError::ValidationError(format!(
                "unsupported scheme '{s}' (expected http/https)"
            )));
        }
    }
    let host = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("")
        .trim_end_matches('/')
        .to_ascii_lowercase();
    let label = host
        .strip_suffix(".onion")
        .ok_or_else(|| EnolaError::ValidationError(format!("'{host}' is not a .onion address")))?;
    let valid = matches!(label.len(), 16 | 56)
        && label
            .chars()
            .all(|c| c.is_ascii_lowercase() || ('2'..='7').contains(&c));
    if !valid {
        return Err(EnolaError::ValidationError(format!(
            "malformed onion address '{host}' (v3 = 56 base32 chars, v2 = 16)"
        )));
    }
    Ok(format!("{}://{}", scheme.unwrap_or("http"), host))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::http_probe::{MockHttpProbePort, ProbeOutcome};

    const V3: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.onion";

    fn probe_ok() -> MockHttpProbePort {
        let mut m = MockHttpProbePort::new();
        m.expect_get().returning(|_, _| {
            Ok(ProbeOutcome {
                status: 200,
                latency_ms: 420,
                title: Some("My Blog".to_string()),
            })
        });
        m
    }

    #[test]
    fn validates_v3_and_v2_addresses() {
        assert_eq!(normalize_onion_url(V3).unwrap(), format!("http://{V3}"));
        assert_eq!(
            normalize_onion_url(&format!("https://{V3}")).unwrap(),
            format!("https://{V3}")
        );
        let v2 = "abcdefghijklmnop.onion";
        assert_eq!(normalize_onion_url(v2).unwrap(), format!("http://{v2}"));
    }

    #[test]
    fn rejects_malformed_addresses_before_network() {
        for bad in [
            "",
            "not-an-onion.com",
            "http://abc.onion", // too short
            "ABC!.onion",       // bad chars
            "ftp://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.onion",
        ] {
            assert!(
                normalize_onion_url(bad).is_err(),
                "'{bad}' should be rejected"
            );
        }
    }

    #[tokio::test]
    async fn reachable_onion_returns_status_latency_title() {
        let v = VerifyOnion::new(Arc::new(probe_ok()));
        let r = v.execute(V3).await.unwrap();
        assert!(r.reachable);
        assert_eq!(r.status, Some(200));
        assert_eq!(r.latency_ms, Some(420));
        assert_eq!(r.title.as_deref(), Some("My Blog"));
        assert_eq!(r.attempts, 1);
    }

    #[tokio::test]
    async fn timeout_returns_unreachable_without_hard_error() {
        let mut m = MockHttpProbePort::new();
        m.expect_get()
            .times(2)
            .returning(|_, _| Err(ProbeError::Timeout));
        let v = VerifyOnion::new(Arc::new(m)).with_retry(2, Duration::ZERO);
        let r = v.execute(V3).await.unwrap();
        assert!(!r.reachable);
        assert_eq!(r.reason.as_deref(), Some("timeout"));
        assert_eq!(r.attempts, 2);
    }

    #[tokio::test]
    async fn proxy_failure_is_distinguished_and_not_retried() {
        let mut m = MockHttpProbePort::new();
        m.expect_get()
            .times(1) // proxy caído → sin reintentos
            .returning(|_, _| Err(ProbeError::ProxyUnavailable("conn refused".into())));
        let v = VerifyOnion::new(Arc::new(m)).with_retry(4, Duration::ZERO);
        let r = v.execute(V3).await.unwrap();
        assert!(!r.reachable);
        assert_eq!(r.reason.as_deref(), Some("proxy_unavailable"));
        assert_eq!(r.attempts, 1);
    }

    #[tokio::test]
    async fn site_down_retries_then_reports_unreachable() {
        let mut m = MockHttpProbePort::new();
        m.expect_get()
            .times(3)
            .returning(|_, _| Err(ProbeError::SiteUnreachable("host unreachable".into())));
        let v = VerifyOnion::new(Arc::new(m)).with_retry(3, Duration::ZERO);
        let r = v.execute(V3).await.unwrap();
        assert!(!r.reachable);
        assert_eq!(r.reason.as_deref(), Some("site_unreachable"));
        assert_eq!(r.attempts, 3);
    }

    #[tokio::test]
    async fn malformed_address_never_reaches_probe() {
        let m = MockHttpProbePort::new(); // sin expectativas → falla si se llama
        let v = VerifyOnion::new(Arc::new(m));
        assert!(v.execute("not.onion.bad!!").await.is_err());
    }
}
