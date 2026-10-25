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

use crate::adapters::web::web_api;

pub struct AppState {
    pub token: String,
    /// T12: contador de fallos de auth consecutivos para el backoff anti fuerza bruta.
    pub auth_failures: AtomicU64,
}

/// F-06 residual (R-10): persiste el token del dashboard en
/// `~/.enola/web-token` (0600, escritura atómica) del usuario invocador —
/// imprimirlo por stderr lo dejaba en journald bajo systemd/nohup, legible
/// por `journalctl` (y por la tool `logs view --source system`, ReadOnly).
/// Devuelve la ruta si se persistió; el valor del token nunca va a stderr.
#[cfg(unix)]
fn persist_web_token_in(
    dir: &std::path::Path,
    token: &str,
    owner: Option<(u32, u32)>,
) -> Option<std::path::PathBuf> {
    let sd = crate::infrastructure::safe_dir::open_user_config_dir(dir, owner).ok()?;
    sd.write_atomic("web-token", token.as_bytes(), 0o600, owner)
        .ok()?;
    Some(dir.join("web-token"))
}

#[cfg(unix)]
fn persist_web_token(token: &str) -> Option<std::path::PathBuf> {
    use crate::infrastructure::drop_privs::resolve_user_home;

    let home = resolve_user_home(None)?;
    let dir = std::path::Path::new(&home).join(".enola");
    // SEC-A6: el fichero nace del usuario invocador (fchown sobre el fd antes
    // del rename) para que pueda leerlo sin sudo; nunca chown por ruta.
    persist_web_token_in(
        &dir,
        token,
        crate::infrastructure::safe_dir::invoker_owner(),
    )
}

pub async fn start_server(port: u16) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if !is_root() {
        eprintln!("Error: enola-cli web requires root privileges.");
        eprintln!("Try: sudo enola-cli web --port {}", port);
        return Err("not running as root".into());
    }

    // SEC-A7 (D3): la web gestiona todos los servicios; corregir al arrancar
    // los directorios base de datos que ya existan.
    crate::infrastructure::safe_dir::secure_existing_service_bases(
        &crate::infrastructure::safe_dir::SERVICE_DATA_BASES,
    );

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

    // F-06 residual: el token se persiste en ~/.enola/web-token (0600) del
    // usuario invocador y por stderr solo va la RUTA — nunca el valor
    // (journald/`logs view` son legibles sin privilegio equivalente).
    // Fallback: si no se puede persistir (p.ej. root puro sin home resoluble),
    // se imprime el token — residual documentado.
    let token_display = {
        #[cfg(unix)]
        {
            match persist_web_token(&token) {
                Some(path) => format!("{}", path.display()),
                None => {
                    eprintln!(
                        "  ⚠️  Could not persist token to ~/.enola/web-token — printing it here (goes to journald under systemd/nohup)."
                    );
                    token.clone()
                }
            }
        }
        #[cfg(not(unix))]
        {
            token.clone()
        }
    };

    eprintln!("════════════════════════════════════════════════════════════════");
    eprintln!("  Enola Web Dashboard");
    eprintln!("  Listening: http://127.0.0.1:{}", port);
    if std::env::var("ENOLA_WEB_TOKEN").is_ok() {
        // Modo test: el operador ya conoce el valor (lo fijó él mismo).
        eprintln!("  Token:    {}", token);
        eprintln!("  ⚠️  Token fijado via ENOLA_WEB_TOKEN (modo test).");
        eprintln!("     No use esto en producción — omita la variable para");
        eprintln!("     obtener un token aleatorio en cada inicio.");
    } else {
        eprintln!("  Token file: {}", token_display);
        eprintln!("  Read it with: cat {}", token_display);
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
        include_str!("../../../assets/index.html"),
    )
}

async fn style_css() -> impl IntoResponse {
    (
        [(axum::http::header::CONTENT_TYPE, "text/css; charset=utf-8")],
        include_str!("../../../assets/style.css"),
    )
}

async fn app_js() -> impl IntoResponse {
    (
        [(
            axum::http::header::CONTENT_TYPE,
            "application/javascript; charset=utf-8",
        )],
        include_str!("../../../assets/app.js"),
    )
}

async fn console_commands_json() -> impl IntoResponse {
    (
        [(
            axum::http::header::CONTENT_TYPE,
            "application/json; charset=utf-8",
        )],
        include_str!("../../../assets/console_commands.json"),
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
            let err = crate::adapters::web::web_errors::ApiError {
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

    // SEC-A6: el token se escribe dentro de ~/.enola validado y nunca sigue
    // un symlink plantado en `web-token`.
    #[test]
    fn persist_web_token_does_not_follow_symlink() {
        use std::os::unix::fs::PermissionsExt;
        let home = tempfile::tempdir().unwrap();
        let dir = home.path().join(".enola");
        std::fs::create_dir(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        let victim = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(victim.path(), "keep").unwrap();
        std::os::unix::fs::symlink(victim.path(), dir.join("web-token")).unwrap();

        let p = persist_web_token_in(&dir, "abcdefghijklmnopqrstuvwxyzabcdef", None)
            .expect("token persisted");

        assert_eq!(std::fs::read_to_string(victim.path()).unwrap(), "keep");
        let md = std::fs::symlink_metadata(&p).unwrap();
        assert!(md.file_type().is_file());
        assert_eq!(md.permissions().mode() & 0o777, 0o600);
        assert_eq!(
            std::fs::read_to_string(&p).unwrap(),
            "abcdefghijklmnopqrstuvwxyzabcdef"
        );
    }

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
