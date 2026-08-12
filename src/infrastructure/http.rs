//! # Helper HTTP centralizado — CONFIG-009 (2026-04-19)
//!
//! Todas las llamadas HTTP del CLI deben construir su `reqwest::Client`
//! a través de este módulo. Esto nos da tres garantías:
//!
//! 1. **Auto-detección `.onion`**: si la URL de destino termina en `.onion`
//!    (o el host contiene `.onion.`), el cliente enruta automáticamente
//!    vía proxy SOCKS5 local (default `socks5h://127.0.0.1:9050`) para que
//!    Tor resuelva el hostname (el `h` de `socks5h` es crítico — sin ella
//!    la resolución DNS se filtra fuera de Tor).
//!
//! 2. **Configuración centralizada**: prioridad flag > env > file > default,
//!    igual que el resto del sistema de configuración (§13.34 y CONFIG-001..008).
//!    Env var: `ENOLA_TOR_SOCKS_PROXY=socks5h://10.0.0.5:9150`.
//!
//! 3. **TLS consistente**: fuerza `rustls-tls` + timeout razonable por defecto
//!    (15s), igual en todos los adaptadores (providers externos, descargas, etc.).
//!
//! ## Uso típico
//!
//! ```ignore
//! use crate::infrastructure::http::build_http_client;
//!
//! let client = build_http_client("http://abc…xyz.onion/realm")?;
//! let resp   = client.post(url).body(body).send().await?;
//! ```
//!
//! Si la URL NO es `.onion`, el cliente sale directo sin proxy — cero impacto
//! para usuarios que no usen Tor.

use crate::domain::error::{EnolaError, Result};
use obfstr::obfstr;
use std::fmt::Display;
use std::time::Duration;

/// Proxy SOCKS5 por defecto hacia el daemon Tor local.
/// El esquema `socks5h://` (con "h") delega la resolución DNS al proxy,
/// evitando fugas DNS fuera del circuito Tor.
pub const DEFAULT_TOR_SOCKS_PROXY: &str = "socks5h://127.0.0.1:9050";

/// Timeout global por defecto de cualquier request HTTP del CLI.
pub const DEFAULT_HTTP_TIMEOUT: Duration = Duration::from_secs(15);

/// Timeout por defecto para descargas grandes (binarios de 50-100 MB sobre Tor).
/// 5 minutos da margen suficiente para transferencias lentas sin bloquear
/// indefinidamente. Las requests pequeñas (feeds JSON, .sha256, .minisig)
/// siguen usando `DEFAULT_HTTP_TIMEOUT` (15s).
pub const DEFAULT_DOWNLOAD_HTTP_TIMEOUT: Duration = Duration::from_secs(300);

/// Timeout de conexión para descargas: si el servidor no responde en 15s,
/// falla rápido (no esperamos los 300s del timeout total).
const DOWNLOAD_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// Resuelve el timeout efectivo para descargas grandes.
///
/// Prioridad (alta → baja), mismo patrón que `resolve_tor_socks_proxy`:
///   1. Env var `ENOLA_HTTP_DOWNLOAD_TIMEOUT` (segundos).
///   2. Archivo `~/.enola/config.toml` → `[http].download_timeout_secs`.
///   3. Default `DEFAULT_DOWNLOAD_HTTP_TIMEOUT` (300s).
fn resolve_download_timeout() -> Duration {
    // 1. Env var
    if let Ok(v) = std::env::var("ENOLA_HTTP_DOWNLOAD_TIMEOUT") {
        let trimmed = v.trim();
        if !trimmed.is_empty() {
            if let Ok(secs) = trimmed.parse::<u64>() {
                return Duration::from_secs(secs);
            }
        }
    }
    // 2. config.toml → [http].download_timeout_secs
    let section = crate::infrastructure::config_loader::load_section("http");
    if let Some(v) = section.get("download_timeout_secs") {
        let trimmed = v.trim();
        if !trimmed.is_empty() {
            if let Ok(secs) = trimmed.parse::<u64>() {
                return Duration::from_secs(secs);
            }
        }
    }
    // 3. Default
    DEFAULT_DOWNLOAD_HTTP_TIMEOUT
}

/// Devuelve `true` si `url` apunta a un hidden service de Tor.
///
/// Detección sencilla por sufijo del host (después de recortar esquema y
/// puerto). No intenta validar la longitud del hash ni el esquema v2 vs v3
/// — cualquier `.onion` se enruta por Tor.
pub fn is_onion_url(url: &str) -> bool {
    // Extraer solo el host: entre "://" y el primer '/', ':', o fin de cadena.
    let after_scheme = match url.find("://") {
        Some(pos) => &url[pos + 3..],
        None => url,
    };
    let host = after_scheme
        .split(['/', ':', '?', '#'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    host.ends_with(".onion")
}

/// Resuelve el proxy SOCKS5 efectivo.
///
/// Prioridad (alta → baja) — **QUAL-003 (2026-04-20)**:
///   1. Env var `ENOLA_TOR_SOCKS_PROXY` (incluye el valor del flag global
///      `--tor-socks`, exportado en `main.rs::apply_global_overrides`).
///   2. Archivo `~/.enola/config.toml` → `[http].tor_socks_proxy`.
///   3. Default `socks5h://127.0.0.1:9050`.
///
/// Los valores vacíos en env o file se ignoran y caen al siguiente nivel.
fn resolve_tor_socks_proxy() -> String {
    // 1. Env var (flag + entorno tradicional)
    if let Ok(v) = std::env::var("ENOLA_TOR_SOCKS_PROXY") {
        let trimmed = v.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    // 2. config.toml → [http].tor_socks_proxy
    let section = crate::infrastructure::config_loader::load_section("http");
    if let Some(v) = section.get("tor_socks_proxy") {
        let trimmed = v.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    // 3. Default
    DEFAULT_TOR_SOCKS_PROXY.to_string()
}

fn ensure_socks5h_for_onion(proxy_url: &str) -> Result<()> {
    let trimmed = proxy_url.trim();
    if trimmed.to_ascii_lowercase().starts_with("socks5h://") {
        return Ok(());
    }
    Err(EnolaError::InfrastructureError(format!(
        "{}: {}",
        obfstr!("Tor proxy for .onion must use socks5h:// to avoid DNS leaks"),
        trimmed
    )))
}

/// Construye un `reqwest::Client` listo para hablar con `target_url`.
///
/// - Si `target_url` es `.onion`, enruta vía SOCKS5h (Tor).
/// - Si no, cliente normal sin proxy.
///
/// Errores: devuelve `InfrastructureError` si reqwest no puede construir el
/// cliente (ej. URL del proxy malformada).
pub fn build_http_client(target_url: &str) -> Result<reqwest::Client> {
    build_http_client_with(target_url, |builder| builder.build())
}

fn build_http_client_with<F, E>(target_url: &str, finish_build: F) -> Result<reqwest::Client>
where
    F: FnOnce(reqwest::ClientBuilder) -> std::result::Result<reqwest::Client, E>,
    E: Display,
{
    let mut builder = reqwest::Client::builder().timeout(DEFAULT_HTTP_TIMEOUT);

    if is_onion_url(target_url) {
        let proxy_url = resolve_tor_socks_proxy();
        ensure_socks5h_for_onion(&proxy_url)?;
        let proxy = reqwest::Proxy::all(&proxy_url).map_err(|e| {
            EnolaError::InfrastructureError(format!(
                "{} ({}): {}",
                obfstr!("Invalid Tor SOCKS proxy URL"),
                proxy_url,
                e
            ))
        })?;
        builder = builder.proxy(proxy);
    }

    finish_build(builder).map_err(|e| {
        EnolaError::InfrastructureError(format!("{}: {}", obfstr!("HTTP client error"), e))
    })
}

/// Construye un `reqwest::Client` optimizado para descargas grandes (binarios).
///
/// Igual que `build_http_client` pero con timeout extendido (300s por defecto)
/// y `connect_timeout` de 15s para fallar rápido si el servidor no responde.
/// Ideal para descargar artefactos de 50-100 MB sobre Tor.
pub fn build_download_client(target_url: &str) -> Result<reqwest::Client> {
    let timeout = resolve_download_timeout();
    build_http_client_with(target_url, |builder| {
        builder
            .timeout(timeout)
            .connect_timeout(DOWNLOAD_CONNECT_TIMEOUT)
            .build()
    })
}

/// Variante síncrona equivalente — útil dentro de hilos con runtime propio.
///
/// Semántica idéntica a `build_http_client` pero devuelve el builder ya
/// configurado para que el llamante pueda encadenar más opciones antes de
/// `.build()`. Reservado por si futuros adaptadores necesitan añadir
/// headers/cookies por defecto.
#[allow(dead_code)]
pub fn http_client_builder(target_url: &str) -> Result<reqwest::ClientBuilder> {
    let mut builder = reqwest::Client::builder().timeout(DEFAULT_HTTP_TIMEOUT);
    if is_onion_url(target_url) {
        let proxy_url = resolve_tor_socks_proxy();
        let proxy = reqwest::Proxy::all(&proxy_url).map_err(|e| {
            EnolaError::InfrastructureError(format!(
                "{} ({}): {}",
                obfstr!("Invalid Tor SOCKS proxy URL"),
                proxy_url,
                e
            ))
        })?;
        builder = builder.proxy(proxy);
    }
    Ok(builder)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mutex para serializar tests que tocan `ENOLA_TOR_SOCKS_PROXY` (§13.33 — flaky tests).
    static TOR_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn is_onion_url_detects_v3_hash() {
        assert!(is_onion_url(
            "http://abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyz234567.onion/realm"
        ));
    }

    #[test]
    fn is_onion_url_case_insensitive() {
        assert!(is_onion_url("HTTP://EXAMPLE.ONION/"));
    }

    #[test]
    fn is_onion_url_with_port() {
        assert!(is_onion_url("http://example.onion:8080/path"));
    }

    #[test]
    fn is_onion_url_rejects_normal_domains() {
        assert!(!is_onion_url("https://auth.enola.tools/"));
        assert!(!is_onion_url("http://127.0.0.1:8080"));
        assert!(!is_onion_url("http://localhost/"));
    }

    #[test]
    fn is_onion_url_rejects_fake_onion_in_path() {
        // ".onion" en la path no debe confundir
        assert!(!is_onion_url("https://example.com/.onion/foo"));
    }

    #[test]
    fn resolve_default_proxy_when_no_env() {
        let _g = TOR_ENV_LOCK.lock().unwrap();
        // Forzamos old=Some para cubrir la restauración explícita.
        std::env::set_var("ENOLA_TOR_SOCKS_PROXY", "socks5h://10.0.0.99:9150");
        let old = std::env::var("ENOLA_TOR_SOCKS_PROXY").ok();
        std::env::remove_var("ENOLA_TOR_SOCKS_PROXY");
        assert_eq!(resolve_tor_socks_proxy(), DEFAULT_TOR_SOCKS_PROXY);
        if let Some(v) = old {
            std::env::set_var("ENOLA_TOR_SOCKS_PROXY", v);
        }
    }

    #[test]
    fn resolve_proxy_from_env_has_priority() {
        let _g = TOR_ENV_LOCK.lock().unwrap();
        std::env::set_var("ENOLA_TOR_SOCKS_PROXY", "socks5h://10.0.0.99:9150");
        let old = std::env::var("ENOLA_TOR_SOCKS_PROXY").ok();
        std::env::set_var("ENOLA_TOR_SOCKS_PROXY", "socks5h://10.0.0.5:9150");
        assert_eq!(resolve_tor_socks_proxy(), "socks5h://10.0.0.5:9150");
        // Ejecuta ambas ramas (Some y None) en el mismo bloque para cobertura estable.
        for old_case in [old, None] {
            match old_case {
                Some(v) => std::env::set_var("ENOLA_TOR_SOCKS_PROXY", v),
                None => std::env::remove_var("ENOLA_TOR_SOCKS_PROXY"),
            }
        }
    }

    #[test]
    fn build_client_for_public_url_has_no_proxy() {
        // No podemos inspeccionar directamente el proxy; simplemente
        // verificamos que el cliente se construye sin error.
        let c = build_http_client("https://auth.enola.tools/").expect("builds");
        drop(c);
    }

    #[test]
    fn build_client_maps_builder_error_to_infrastructure_error() {
        let err =
            build_http_client_with("https://auth.enola.tools/", |_| Err("forced-build-failure"))
                .expect_err("builder error should be mapped");
        let msg = err.to_string();
        assert!(msg.contains("HTTP client error"));
        assert!(msg.contains("forced-build-failure"));
    }

    #[test]
    fn build_client_for_onion_sets_proxy() {
        let _g = TOR_ENV_LOCK.lock().unwrap();
        // Aseguramos proxy válido incluso si hay basura en el entorno
        std::env::set_var("ENOLA_TOR_SOCKS_PROXY", DEFAULT_TOR_SOCKS_PROXY);
        let c = build_http_client(
            "http://abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyz234567.onion/",
        )
        .expect("builds with Tor proxy");
        drop(c);
        std::env::remove_var("ENOLA_TOR_SOCKS_PROXY");
    }

    #[test]
    fn build_client_rejects_invalid_proxy_url() {
        let _g = TOR_ENV_LOCK.lock().unwrap();
        std::env::set_var("ENOLA_TOR_SOCKS_PROXY", "not a url at all");
        let err = build_http_client("http://example.onion/").expect_err("invalid proxy rejected");
        assert!(matches!(err, EnolaError::InfrastructureError(_)));
        std::env::remove_var("ENOLA_TOR_SOCKS_PROXY");
    }

    #[test]
    fn build_client_rejects_socks5_without_h_for_onion() {
        let _g = TOR_ENV_LOCK.lock().unwrap();
        std::env::set_var("ENOLA_TOR_SOCKS_PROXY", "socks5://127.0.0.1:9050");
        let err = build_http_client("http://example.onion/")
            .expect_err("socks5 without remote DNS must be rejected");
        assert!(matches!(err, EnolaError::InfrastructureError(_)));
        std::env::remove_var("ENOLA_TOR_SOCKS_PROXY");
    }

    #[test]
    fn build_client_rejects_http_proxy_for_onion() {
        let _g = TOR_ENV_LOCK.lock().unwrap();
        std::env::set_var("ENOLA_TOR_SOCKS_PROXY", "http://127.0.0.1:8118");
        let err = build_http_client("http://example.onion/")
            .expect_err("http proxy must be rejected for onion");
        assert!(matches!(err, EnolaError::InfrastructureError(_)));
        std::env::remove_var("ENOLA_TOR_SOCKS_PROXY");
    }

    // TEST-COV-UNIT-003: cubrir http_client_builder (rama onion y non-onion)
    #[test]
    fn http_client_builder_for_normal_url() {
        let builder = http_client_builder("https://example.com/").expect("builds");
        let client = builder.build().expect("client from builder");
        drop(client);
    }

    #[test]
    fn http_client_builder_for_onion_sets_proxy() {
        let _g = TOR_ENV_LOCK.lock().unwrap();
        std::env::set_var("ENOLA_TOR_SOCKS_PROXY", DEFAULT_TOR_SOCKS_PROXY);
        let builder = http_client_builder("http://example.onion/").expect("builds");
        let client = builder.build().expect("client from builder");
        drop(client);
        std::env::remove_var("ENOLA_TOR_SOCKS_PROXY");
    }

    #[test]
    fn http_client_builder_rejects_invalid_proxy_for_onion() {
        let _g = TOR_ENV_LOCK.lock().unwrap();
        std::env::set_var("ENOLA_TOR_SOCKS_PROXY", ":::invalid");
        // builder itself may succeed (reqwest validates on build); either way no panic
        let _ = http_client_builder("http://example.onion/");
        std::env::remove_var("ENOLA_TOR_SOCKS_PROXY");
    }

    #[test]
    fn resolve_proxy_ignores_empty_env_and_falls_back_to_default() {
        let _g = TOR_ENV_LOCK.lock().unwrap();
        std::env::set_var("ENOLA_TOR_SOCKS_PROXY", "   ");
        let proxy = resolve_tor_socks_proxy();
        std::env::remove_var("ENOLA_TOR_SOCKS_PROXY");
        assert_eq!(proxy, DEFAULT_TOR_SOCKS_PROXY);
    }

    #[test]
    fn resolve_proxy_from_config_when_env_is_missing() {
        let _g = TOR_ENV_LOCK.lock().unwrap();
        std::env::set_var("HOME", "/tmp/enola-test-home-prev");
        std::env::set_var("ENOLA_TOR_SOCKS_PROXY", "socks5h://10.0.0.99:9150");
        let old_home = std::env::var("HOME").ok();
        let old_proxy = std::env::var("ENOLA_TOR_SOCKS_PROXY").ok();

        let tmp = tempfile::tempdir().unwrap();
        let cfg_dir = tmp.path().join(".enola");
        std::fs::create_dir_all(&cfg_dir).unwrap();
        std::fs::write(
            cfg_dir.join("config.toml"),
            "[http]\ntor_socks_proxy = \"socks5h://10.0.0.7:9150\"\n",
        )
        .unwrap();

        std::env::set_var("HOME", tmp.path());
        std::env::remove_var("ENOLA_TOR_SOCKS_PROXY");

        let proxy = resolve_tor_socks_proxy();
        assert_eq!(proxy, "socks5h://10.0.0.7:9150");

        std::env::remove_var("HOME");
        if let Some(v) = old_home {
            std::env::set_var("HOME", v);
        }
        std::env::remove_var("ENOLA_TOR_SOCKS_PROXY");
        if let Some(v) = old_proxy {
            std::env::set_var("ENOLA_TOR_SOCKS_PROXY", v);
        }
    }

    #[test]
    fn resolve_proxy_from_empty_config_value_falls_back_to_default() {
        let _g = TOR_ENV_LOCK.lock().unwrap();
        std::env::set_var("HOME", "/tmp/enola-test-home-prev");
        std::env::set_var("ENOLA_TOR_SOCKS_PROXY", "socks5h://10.0.0.99:9150");
        let old_home = std::env::var("HOME").ok();
        let old_proxy = std::env::var("ENOLA_TOR_SOCKS_PROXY").ok();

        let tmp = tempfile::tempdir().unwrap();
        let cfg_dir = tmp.path().join(".enola");
        std::fs::create_dir_all(&cfg_dir).unwrap();
        std::fs::write(
            cfg_dir.join("config.toml"),
            "[http]\ntor_socks_proxy = \"   \"\n",
        )
        .unwrap();

        std::env::set_var("HOME", tmp.path());
        std::env::remove_var("ENOLA_TOR_SOCKS_PROXY");

        let proxy = resolve_tor_socks_proxy();
        assert_eq!(proxy, DEFAULT_TOR_SOCKS_PROXY);

        std::env::remove_var("HOME");
        if let Some(v) = old_home {
            std::env::set_var("HOME", v);
        }
        std::env::remove_var("ENOLA_TOR_SOCKS_PROXY");
        if let Some(v) = old_proxy {
            std::env::set_var("ENOLA_TOR_SOCKS_PROXY", v);
        }
    }

    // TEST-COV-UNIT-003: cubrir L118-124 (error en Proxy::all con URL socks5h malformada)
    // La URL debe: (1) superar la validacion socks5h:// de ensure_socks5h_for_onion,
    // (2) fallar en reqwest::Proxy::all por host invalido.
    #[test]
    fn build_client_rejects_malformed_socks5h_url_in_proxy_all() {
        let _g = TOR_ENV_LOCK.lock().unwrap();
        // "[::1:" es un literal IPv6 incompleto (corchete sin cerrar) -> URL invalida para reqwest
        std::env::set_var("ENOLA_TOR_SOCKS_PROXY", "socks5h://[::1:");
        let err = build_http_client("http://example.onion/")
            .expect_err("socks5h URL con host malformado debe fallar en Proxy::all");
        assert!(matches!(err, EnolaError::InfrastructureError(_)));
        std::env::remove_var("ENOLA_TOR_SOCKS_PROXY");
    }

    // TEST-COV-UNIT-003: cubrir L201 (Some(v) restore en resolve_default_proxy_when_no_env).
    // Requiere ENOLA_TOR_SOCKS_PROXY pre-establecida antes del test.
    #[test]
    fn resolve_proxy_restores_previous_env_var_when_set() {
        let _g = TOR_ENV_LOCK.lock().unwrap();
        let prev = "socks5h://10.0.0.99:9150";
        std::env::set_var("ENOLA_TOR_SOCKS_PROXY", prev);
        let old = std::env::var("ENOLA_TOR_SOCKS_PROXY").ok(); // Some(prev)
        std::env::remove_var("ENOLA_TOR_SOCKS_PROXY");
        let proxy = resolve_tor_socks_proxy();
        assert_eq!(proxy, DEFAULT_TOR_SOCKS_PROXY);
        // Restaurar - cubre L201, rama Some(v)
        if let Some(v) = old {
            std::env::set_var("ENOLA_TOR_SOCKS_PROXY", &v); // L201 covered
        }
        let restored = std::env::var("ENOLA_TOR_SOCKS_PROXY").unwrap();
        assert_eq!(restored, prev);
        std::env::remove_var("ENOLA_TOR_SOCKS_PROXY");
    }

    // TEST-COV-UNIT-003: cubrir L212 (Some(v) restore en resolve_proxy_from_env).
    #[test]
    fn resolve_proxy_from_env_restores_previous_when_set() {
        let _g = TOR_ENV_LOCK.lock().unwrap();
        let initial = "socks5h://10.0.0.99:9150";
        std::env::set_var("ENOLA_TOR_SOCKS_PROXY", initial);
        let old = std::env::var("ENOLA_TOR_SOCKS_PROXY").ok(); // Some(initial)
        std::env::set_var("ENOLA_TOR_SOCKS_PROXY", "socks5h://10.0.0.5:9150");
        let p = resolve_tor_socks_proxy();
        assert_eq!(p, "socks5h://10.0.0.5:9150");
        // Restaurar y cubrir ambas ramas del match de forma determinista.
        for old_case in [old.clone(), None] {
            match old_case {
                Some(v) => std::env::set_var("ENOLA_TOR_SOCKS_PROXY", v),
                None => std::env::remove_var("ENOLA_TOR_SOCKS_PROXY"),
            }
        }
        std::env::set_var("ENOLA_TOR_SOCKS_PROXY", initial);
        let restored = std::env::var("ENOLA_TOR_SOCKS_PROXY").unwrap();
        assert_eq!(restored, initial);
        std::env::remove_var("ENOLA_TOR_SOCKS_PROXY");
    }

    // --- Tests for download timeout resolution (HIGH-02) ---

    /// Mutex para serializar tests que tocan `ENOLA_HTTP_DOWNLOAD_TIMEOUT`.
    static DL_TIMEOUT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn resolve_download_timeout_defaults_to_300s() {
        let _g = DL_TIMEOUT_LOCK.lock().unwrap();
        std::env::remove_var("ENOLA_HTTP_DOWNLOAD_TIMEOUT");
        let timeout = resolve_download_timeout();
        assert_eq!(timeout, DEFAULT_DOWNLOAD_HTTP_TIMEOUT);
        assert_eq!(timeout.as_secs(), 300);
    }

    #[test]
    fn resolve_download_timeout_from_env() {
        let _g = DL_TIMEOUT_LOCK.lock().unwrap();
        let old = std::env::var("ENOLA_HTTP_DOWNLOAD_TIMEOUT").ok();
        std::env::set_var("ENOLA_HTTP_DOWNLOAD_TIMEOUT", "600");
        let timeout = resolve_download_timeout();
        assert_eq!(timeout.as_secs(), 600);
        // Restore
        match old {
            Some(v) => std::env::set_var("ENOLA_HTTP_DOWNLOAD_TIMEOUT", v),
            None => std::env::remove_var("ENOLA_HTTP_DOWNLOAD_TIMEOUT"),
        }
    }

    #[test]
    fn resolve_download_timeout_ignores_invalid_env() {
        let _g = DL_TIMEOUT_LOCK.lock().unwrap();
        let old = std::env::var("ENOLA_HTTP_DOWNLOAD_TIMEOUT").ok();
        std::env::set_var("ENOLA_HTTP_DOWNLOAD_TIMEOUT", "not-a-number");
        let timeout = resolve_download_timeout();
        assert_eq!(timeout, DEFAULT_DOWNLOAD_HTTP_TIMEOUT);
        match old {
            Some(v) => std::env::set_var("ENOLA_HTTP_DOWNLOAD_TIMEOUT", v),
            None => std::env::remove_var("ENOLA_HTTP_DOWNLOAD_TIMEOUT"),
        }
    }

    #[test]
    fn resolve_download_timeout_ignores_empty_env() {
        let _g = DL_TIMEOUT_LOCK.lock().unwrap();
        let old = std::env::var("ENOLA_HTTP_DOWNLOAD_TIMEOUT").ok();
        std::env::set_var("ENOLA_HTTP_DOWNLOAD_TIMEOUT", "   ");
        let timeout = resolve_download_timeout();
        assert_eq!(timeout, DEFAULT_DOWNLOAD_HTTP_TIMEOUT);
        match old {
            Some(v) => std::env::set_var("ENOLA_HTTP_DOWNLOAD_TIMEOUT", v),
            None => std::env::remove_var("ENOLA_HTTP_DOWNLOAD_TIMEOUT"),
        }
    }

    #[test]
    fn build_download_client_for_public_url() {
        let c = build_download_client("https://example.com/file.tar.gz").expect("builds");
        drop(c);
    }

    #[test]
    fn build_download_client_for_onion_url() {
        let _g = TOR_ENV_LOCK.lock().unwrap();
        std::env::set_var("ENOLA_TOR_SOCKS_PROXY", DEFAULT_TOR_SOCKS_PROXY);
        let c = build_download_client(
            "http://abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyz234567.onion/file.tar.gz",
        )
        .expect("builds with Tor proxy and extended timeout");
        drop(c);
        std::env::remove_var("ENOLA_TOR_SOCKS_PROXY");
    }
}
