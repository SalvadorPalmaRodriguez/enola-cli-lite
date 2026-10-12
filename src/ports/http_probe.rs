//! MCP-006 — Port para sondear URLs HTTP(S)/.onion.
//!
//! `verify_onion` necesita distinguir *por qué* falla una URL para que el
//! agente diagnostique bien: no es lo mismo «el proxy Tor está caído» que
//! «el servicio no responde». El adapter clasifica el error de reqwest.

use std::fmt;
use std::time::Duration;

/// Resultado de una sonda HTTP exitosa (2xx-4xx cuentan como respuesta).
#[derive(Debug, Clone, PartialEq)]
pub struct ProbeOutcome {
    /// Código de estado HTTP devuelto por el servidor.
    pub status: u16,
    /// Latencia total de la petición en milisegundos.
    pub latency_ms: u64,
    /// Contenido de `<title>` si el cuerpo es HTML (opcional).
    pub title: Option<String>,
}

/// Por qué falló la sonda — clasificación accionable para el diagnóstico.
#[derive(Debug, Clone, PartialEq)]
pub enum ProbeError {
    /// La petición agotó su timeout sin respuesta.
    Timeout,
    /// El proxy SOCKS5h (Tor) no acepta conexiones o está mal configurado.
    ProxyUnavailable(String),
    /// El proxy responde pero el sitio (.onion) no contesta / rechaza.
    SiteUnreachable(String),
    /// Otro fallo (TLS, parseo de URL, error HTTP de transporte).
    Other(String),
}

impl fmt::Display for ProbeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProbeError::Timeout => write!(f, "timeout"),
            ProbeError::ProxyUnavailable(m) => write!(f, "tor proxy unavailable: {m}"),
            ProbeError::SiteUnreachable(m) => write!(f, "site unreachable: {m}"),
            ProbeError::Other(m) => write!(f, "{m}"),
        }
    }
}

/// Sonda HTTP/HTTPS/.onion inyectable.
#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait HttpProbePort {
    /// GET a `url` con `timeout` dado. Devuelve `ProbeOutcome` ante cualquier
    /// respuesta del servidor (incluido 4xx/5xx) o `ProbeError` si no llega.
    async fn get(&self, url: &str, timeout: Duration) -> Result<ProbeOutcome, ProbeError>;
}
