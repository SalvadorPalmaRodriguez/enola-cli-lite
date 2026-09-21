use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use rand::Rng;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::application::web_api;

pub struct AppState {
    pub token: String,
    /// T12: contador de fallos de auth consecutivos para el backoff anti fuerza bruta.
    pub auth_failures: AtomicU64,
}

pub async fn start_server(port: u16) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if !is_root() {
        eprintln!("Error: enola-cli web requires root privileges.");
        eprintln!("Try: sudo enola-cli web --port {}", port);
        return Err("not running as root".into());
    }

    // Token: randomly generated unless ENOLA_WEB_TOKEN env var is set.
    // ENOLA_WEB_TOKEN is intended for automated testing only.
    // In production, omit it to get a random token printed on startup.
    let token: String = std::env::var("ENOLA_WEB_TOKEN").unwrap_or_else(|_| {
        (0..32)
            .map(|_| {
                let mut rng = rand::thread_rng();
                (b'a' + rng.gen_range(0..26)) as char
            })
            .collect()
    });

    let state = Arc::new(AppState {
        token: token.clone(),
        auth_failures: AtomicU64::new(0),
    });

    let addr: SocketAddr = format!("127.0.0.1:{}", port).parse()?;

    let api_routes = web_api::api_routes(state.clone());

    let app = Router::new()
        .route("/", get(index_html))
        .route("/style.css", get(style_css))
        .route("/app.js", get(app_js))
        .route("/console_commands.json", get(console_commands_json))
        .nest("/api", api_routes)
        .with_state(state);

    eprintln!("════════════════════════════════════════════════════════════════");
    eprintln!("  Enola Web Dashboard");
    eprintln!("  Listening: http://127.0.0.1:{}", port);
    eprintln!("  Token:    {}", token);
    if std::env::var("ENOLA_WEB_TOKEN").is_ok() {
        eprintln!("  ⚠️  Token fijado via ENOLA_WEB_TOKEN (modo test).");
        eprintln!("     No use esto en producción — omita la variable para");
        eprintln!("     obtener un token aleatorio en cada inicio.");
    }
    eprintln!("  Open in browser and enter the token when prompted.");
    eprintln!("════════════════════════════════════════════════════════════════");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

fn is_root() -> bool {
    #[allow(unsafe_code)]
    unsafe {
        libc::geteuid() == 0
    }
}

async fn index_html() -> impl IntoResponse {
    (
        [(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")],
        include_str!("../../assets/index.html"),
    )
}

async fn style_css() -> impl IntoResponse {
    (
        [(axum::http::header::CONTENT_TYPE, "text/css; charset=utf-8")],
        include_str!("../../assets/style.css"),
    )
}

async fn app_js() -> impl IntoResponse {
    (
        [(
            axum::http::header::CONTENT_TYPE,
            "application/javascript; charset=utf-8",
        )],
        include_str!("../../assets/app.js"),
    )
}

async fn console_commands_json() -> impl IntoResponse {
    (
        [(
            axum::http::header::CONTENT_TYPE,
            "application/json; charset=utf-8",
        )],
        include_str!("../../assets/console_commands.json"),
    )
}

pub async fn auth_middleware(
    State(state): State<Arc<AppState>>,
    req: Request,
    next: Next,
) -> Response {
    let auth_header = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());

    match auth_header {
        Some(h) if token_matches(h, &state.token) => {
            state.auth_failures.store(0, Ordering::Relaxed);
            next.run(req).await
        }
        _ => {
            // T12: backoff exponencial ante fuerza bruta sobre el token.
            let fails = state.auth_failures.fetch_add(1, Ordering::Relaxed);
            tokio::time::sleep(auth_backoff_delay(fails)).await;
            let err = crate::application::web_errors::ApiError {
                error: "Unauthorized: invalid or missing token".to_string(),
                code: 401,
            };
            err.into_response()
        }
    }
}

/// T12: delay de backoff exponencial para respuestas 401 (anti fuerza bruta).
/// Crece 100ms·2^n hasta un tope de 2000ms.
fn auth_backoff_delay(failures: u64) -> Duration {
    const BASE_MS: u64 = 100;
    const MAX_MS: u64 = 2_000;
    let shift = failures.min(6) as u32;
    let ms = BASE_MS.saturating_mul(1u64 << shift);
    Duration::from_millis(ms.min(MAX_MS))
}

/// T5: comparación en tiempo constante del token bearer (anti timing-attack).
/// Evita el cortocircuito de `==` que filtra longitud y prefijo por tiempo.
fn token_matches(provided: &str, expected: &str) -> bool {
    let a = provided.as_bytes();
    let b = expected.as_bytes();
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_matches_equal_tokens() {
        assert!(token_matches("secret-token", "secret-token"));
    }

    #[test]
    fn token_matches_rejects_different_tokens() {
        assert!(!token_matches("secret-token", "other-token"));
    }

    #[test]
    fn token_matches_rejects_same_prefix_different_length() {
        assert!(!token_matches("secret", "secret-token"));
        assert!(!token_matches("secret-token", "secret"));
    }

    #[test]
    fn token_matches_rejects_empty_vs_nonempty() {
        assert!(!token_matches("", "token"));
        assert!(token_matches("", ""));
    }

    #[test]
    fn auth_backoff_delay_grows_then_caps() {
        use std::time::Duration;
        assert_eq!(auth_backoff_delay(0), Duration::from_millis(100));
        assert_eq!(auth_backoff_delay(1), Duration::from_millis(200));
        assert_eq!(auth_backoff_delay(2), Duration::from_millis(400));
        assert_eq!(auth_backoff_delay(3), Duration::from_millis(800));
        assert_eq!(auth_backoff_delay(10), Duration::from_millis(2000));
        assert_eq!(auth_backoff_delay(100), Duration::from_millis(2000));
    }
}
