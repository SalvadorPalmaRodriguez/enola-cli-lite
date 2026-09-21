use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use rand::Rng;
use std::net::SocketAddr;
use std::sync::Arc;

use crate::application::web_api;

#[derive(Clone)]
pub struct AppState {
    pub token: String,
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
        Some(h) if token_matches(h, &state.token) => next.run(req).await,
        _ => {
            let err = crate::application::web_errors::ApiError {
                error: "Unauthorized: invalid or missing token".to_string(),
                code: 401,
            };
            err.into_response()
        }
    }
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
}
