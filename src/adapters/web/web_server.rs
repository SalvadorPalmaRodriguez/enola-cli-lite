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

/// A25-02 (P1.2): máximo de comandos de consola concurrentes.
pub(crate) const CONSOLE_MAX_CONCURRENT: usize = 4;

/// A25-06 (P5.2): TTL por defecto del token del dashboard (decisión del
/// operador: 12 h, renovación silenciosa re-persistida en `~/.enola/web-token`).
pub(crate) const DEFAULT_TOKEN_TTL_SECS: u64 = 12 * 3600;
/// Límites de `[web].token_ttl_secs` (5 min … 7 días).
const TOKEN_TTL_BOUNDS: (u64, u64) = (300, 7 * 24 * 3600);

/// Token vigente y cuándo se emitió.
pub(crate) struct WebToken {
    pub(crate) value: String,
    pub(crate) issued: std::time::Instant,
}

/// Persiste un token nuevo; `true` si quedó guardado (si no, no se rota).
pub(crate) type TokenPersister = Arc<dyn Fn(&str) -> bool + Send + Sync>;

/// Token aleatorio de 32 letras (CSPRNG del hilo, `rand::thread_rng`).
pub(crate) fn generate_token() -> String {
    let mut rng = rand::thread_rng();
    (0..32)
        .map(|_| (b'a' + rng.gen_range(0..26)) as char)
        .collect()
}

pub struct AppState {
    /// A25-06: token vigente (rota al caducar o con `POST /api/token/rotate`).
    pub(crate) token: std::sync::RwLock<WebToken>,
    /// A25-06: vida máxima del token.
    pub(crate) token_ttl: Duration,
    /// A25-06: cómo se re-persiste el token al rotar.
    pub(crate) persist_token: TokenPersister,
    /// T12: contador de fallos de auth consecutivos para el backoff anti fuerza bruta.
    pub auth_failures: AtomicU64,
    /// A25-02 (P1.2): semáforo de `/api/console/run` (procesos root acotados).
    pub(crate) console_slots: tokio::sync::Semaphore,
    /// A25-03 (P1.5): job PQC TLS en curso (uno a la vez).
    pub(crate) pqc_job: std::sync::Mutex<Option<Arc<crate::adapters::web::web_jobs::JobLog>>>,
    /// A25-I5 (P1.5): tickets monouso del SSE (el EventSource no envía token).
    pub(crate) sse_tickets: crate::adapters::web::web_jobs::TicketStore,
}

impl AppState {
    /// Estado inicial con `token` recién emitido.
    pub(crate) fn new(token: String, token_ttl: Duration, persist_token: TokenPersister) -> Self {
        Self {
            token: std::sync::RwLock::new(WebToken {
                value: token,
                issued: std::time::Instant::now(),
            }),
            token_ttl,
            persist_token,
            auth_failures: AtomicU64::new(0),
            console_slots: tokio::sync::Semaphore::new(CONSOLE_MAX_CONCURRENT),
            pqc_job: std::sync::Mutex::new(None),
            sse_tickets: Default::default(),
        }
    }

    /// Valor del token vigente.
    pub(crate) fn token_value(&self) -> String {
        self.token
            .read()
            .map(|t| t.value.clone())
            .unwrap_or_else(|p| p.into_inner().value.clone())
    }

    /// Emite, persiste y activa un token nuevo. `None` (y se conserva el
    /// actual) si no se pudo persistir: el operador no podría leerlo.
    pub(crate) fn rotate_token(&self) -> Option<String> {
        let mut guard = self.token.write().unwrap_or_else(|p| p.into_inner());
        Self::rotate_locked(&self.persist_token, &mut guard)
    }

    /// Persiste y activa un token nuevo con el write-lock YA tomado: fichero
    /// y memoria cambian como una unidad. Dos rotaciones concurrentes (tick
    /// de TTL + `POST /api/token/rotate`, o dos requests tras caducar) no
    /// pueden intercalarse y dejar en `~/.enola/web-token` un token distinto
    /// del vigente (el operador quedaría fuera hasta la siguiente rotación).
    fn rotate_locked(persist: &TokenPersister, guard: &mut WebToken) -> Option<String> {
        let new = generate_token();
        if !persist(&new) {
            eprintln!(
                "⚠️  Could not persist the rotated dashboard token; keeping the current one."
            );
            return None;
        }
        *guard = WebToken {
            value: new.clone(),
            issued: std::time::Instant::now(),
        };
        Some(new)
    }

    /// Rota en silencio si el token superó su TTL. `true` si rotó.
    ///
    /// La caducidad se re-comprueba bajo el write-lock: si otra rotación se
    /// adelantó, el token ya es fresco y esta no rota de nuevo.
    pub(crate) fn rotate_if_expired(&self) -> bool {
        let is_expired = |t: &WebToken| t.issued.elapsed() >= self.token_ttl;
        let expired = self.token.read().map(|t| is_expired(&t)).unwrap_or(true);
        if !expired {
            return false;
        }
        let mut guard = self.token.write().unwrap_or_else(|p| p.into_inner());
        is_expired(&guard) && Self::rotate_locked(&self.persist_token, &mut guard).is_some()
    }
}

/// `[web].token_ttl_secs` (clave de seguridad: bajo root solo de
/// `/etc/enola/config.toml`), acotado a 5 min … 7 días.
fn token_ttl_from_config() -> Duration {
    let section =
        crate::infrastructure::config_loader::load_security_section("web", &["token_ttl_secs"]);
    let secs = section
        .get("token_ttl_secs")
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_TOKEN_TTL_SECS)
        .clamp(TOKEN_TTL_BOUNDS.0, TOKEN_TTL_BOUNDS.1);
    Duration::from_secs(secs)
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

    // A25-04/06 (P2.3, SD-7): el token se genera SIEMPRE con CSPRNG en cada
    // arranque. `ENOLA_WEB_TOKEN` ya no existe: bajo `sudo -E` permitía fijar
    // el token del dashboard root a un valor conocido por el atacante. Los
    // tests leen `~/.enola/web-token`, como el usuario real.
    let token = generate_token();
    let token_ttl = token_ttl_from_config();
    #[cfg(unix)]
    let persister: TokenPersister = Arc::new(|t: &str| persist_web_token(t).is_some());
    #[cfg(not(unix))]
    let persister: TokenPersister = Arc::new(|_t: &str| false);
    let state = Arc::new(AppState::new(token.clone(), token_ttl, persister));

    // A25-06: renovación silenciosa — al caducar, el token se rota y se
    // re-persiste en ~/.enola/web-token (el navegador pedirá el nuevo).
    {
        let st = state.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(60));
            loop {
                tick.tick().await;
                st.rotate_if_expired();
            }
        });
    }

    let addr: SocketAddr = format!("127.0.0.1:{}", port).parse()?;

    let api_routes = web_api::api_routes(state.clone());

    let app = Router::new()
        .route("/", get(index_html))
        .route("/style.css", get(style_css))
        .route("/app.js", get(app_js))
        .route("/console_commands.json", get(console_commands_json))
        // A25-I5: el SSE de jobs va FUERA del middleware de token (lo
        // autentica su ticket monouso); el resto de /api exige el bearer.
        .nest("/api", api_routes.merge(web_api::sse_routes()))
        // A25-05 (P5.1): Host de loopback + cabeceras de seguridad en TODA respuesta.
        .layer(axum::middleware::from_fn(security_headers_middleware))
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
    eprintln!("  Token file: {}", token_display);
    eprintln!("  Read it with: cat {}", token_display);
    eprintln!(
        "  The token rotates every {} and is rewritten to that file.",
        humantime_secs(token_ttl.as_secs())
    );
    eprintln!("  Open in browser and enter the token when prompted.");
    eprintln!("════════════════════════════════════════════════════════════════");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

/// `43200` → `12h`, `90` → `90s` (solo para el banner).
fn humantime_secs(secs: u64) -> String {
    if secs.is_multiple_of(3600) {
        format!("{}h", secs / 3600)
    } else if secs.is_multiple_of(60) {
        format!("{}min", secs / 60)
    } else {
        format!("{secs}s")
    }
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

/// P1.7: el catálogo de la consola se sirve con los vetos del gate
/// (`forbidden`) para que la UI marque como no disponibles las familias y
/// rutas que la consola rechazaría con 403.
async fn console_commands_json() -> impl IntoResponse {
    let raw = include_str!("../../../assets/console_commands.json");
    let body = match serde_json::from_str::<serde_json::Value>(raw) {
        Ok(mut v) => {
            v["forbidden"] = crate::application::command_gate::forbidden_for_ui();
            v.to_string()
        }
        Err(_) => raw.to_string(),
    };
    (
        [(
            axum::http::header::CONTENT_TYPE,
            "application/json; charset=utf-8",
        )],
        body,
    )
}

/// A25-05: hosts aceptados en la cabecera `Host` (anti DNS-rebinding). El puerto
/// no se compara: un túnel SSH (`ssh -L 9000:127.0.0.1:8090`) llega como
/// `localhost:9000` y es uso legítimo; el rebinding usa un dominio del atacante.
fn host_is_loopback(host: &str) -> bool {
    let h = host.trim();
    let name = if let Some(rest) = h.strip_prefix('[') {
        rest.split(']').next().unwrap_or("")
    } else {
        h.rsplit_once(':').map(|(n, _)| n).unwrap_or(h)
    };
    matches!(
        name.to_ascii_lowercase().as_str(),
        "127.0.0.1" | "localhost" | "::1"
    )
}

/// A25-05 (P5.1): rechaza `Host` ajeno con 421 y añade cabeceras de seguridad.
///
/// CSP: `'unsafe-inline'` en `script-src`/`style-src` es un residual temporal —
/// `index.html` usa ~80 manejadores `onclick=` inline; quitarlo exige migrarlos a
/// `addEventListener`. El resto (`default-src 'self'`, `frame-ancestors 'none'`,
/// `base-uri`/`form-action`) sí se aplica.
pub(crate) async fn security_headers_middleware(req: Request, next: Next) -> Response {
    use axum::http::{header, HeaderValue, StatusCode};
    let host_ok = req
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .map(host_is_loopback)
        .unwrap_or(false);
    if !host_ok {
        return (
            StatusCode::MISDIRECTED_REQUEST,
            "Misdirected request: the dashboard only answers on 127.0.0.1/localhost",
        )
            .into_response();
    }
    let is_api = req.uri().path().starts_with("/api");
    let mut resp = next.run(req).await;
    let h = resp.headers_mut();
    h.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'self'; script-src 'self' 'unsafe-inline'; \
             style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self'; \
             frame-ancestors 'none'; base-uri 'none'; form-action 'self'",
        ),
    );
    h.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    h.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    if is_api {
        h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    resp
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
        // A25-06: un token caducado se rota aquí mismo (el viejo deja de valer).
        Some(h) if !state.rotate_if_expired() && token_matches(h, &state.token_value()) => {
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
pub(crate) fn auth_backoff_delay(failures: u64) -> Duration {
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

    /// A25-05: el Host debe ser loopback (anti DNS-rebinding); el puerto no
    /// importa (túneles SSH).
    #[test]
    fn host_validation_accepts_only_loopback_names() {
        for ok in [
            "127.0.0.1:8090",
            "localhost:9000",
            "LOCALHOST",
            "[::1]:8090",
            "127.0.0.1",
        ] {
            assert!(host_is_loopback(ok), "{ok}");
        }
        for bad in [
            "evil.com",
            "evil.com:8090",
            "127.0.0.1.evil.com:8090",
            "10.0.0.5:8090",
            "",
        ] {
            assert!(!host_is_loopback(bad), "{bad}");
        }
    }

    #[tokio::test]
    async fn middleware_sets_headers_and_rejects_foreign_host() {
        use axum::routing::get;
        let app = axum::Router::new()
            .route("/api/x", get(|| async { "ok" }))
            .layer(axum::middleware::from_fn(security_headers_middleware));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        let client = reqwest::Client::new();
        let url = format!("http://{addr}/api/x");
        let r = client
            .get(&url)
            .header("Host", "evil.com")
            .send()
            .await
            .unwrap();
        assert_eq!(r.status().as_u16(), 421);
        let r = client.get(&url).send().await.unwrap();
        assert_eq!(r.status().as_u16(), 200);
        let h = r.headers();
        assert_eq!(h["x-frame-options"], "DENY");
        assert_eq!(h["x-content-type-options"], "nosniff");
        assert_eq!(h["cache-control"], "no-store");
        assert_eq!(h["referrer-policy"], "no-referrer");
        assert!(h["content-security-policy"]
            .to_str()
            .unwrap()
            .contains("frame-ancestors 'none'"));
    }

    fn state_with(ttl: Duration, persist_ok: bool) -> AppState {
        AppState::new("a".repeat(32), ttl, Arc::new(move |_t: &str| persist_ok))
    }

    /// A25-06: rotación manual y por TTL; sin persistir no se rota.
    #[test]
    fn token_rotates_on_demand_and_on_expiry() {
        let st = state_with(Duration::from_secs(3600), true);
        let old = st.token_value();
        assert!(!st.rotate_if_expired());
        let new = st.rotate_token().unwrap();
        assert_ne!(old, new);
        assert_eq!(st.token_value(), new);
        assert_eq!(new.len(), 32);

        let st = state_with(Duration::from_secs(0), true);
        let old = st.token_value();
        assert!(st.rotate_if_expired());
        assert_ne!(st.token_value(), old);

        let st = state_with(Duration::from_secs(0), false);
        let old = st.token_value();
        assert!(!st.rotate_if_expired());
        assert_eq!(st.token_value(), old, "sin persistir se conserva el token");
    }

    /// Re-verificación A25-06: persistir y activar son una unidad bajo el
    /// write-lock — tras rotaciones concurrentes (manual + por TTL) el último
    /// token escrito en el fichero es exactamente el vigente en memoria.
    #[test]
    fn concurrent_rotations_keep_file_and_memory_in_sync() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let persisted = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let calls = Arc::new(AtomicUsize::new(0));
        let (sink, n) = (persisted.clone(), calls.clone());
        let st = Arc::new(AppState::new(
            "a".repeat(32),
            Duration::from_secs(0),
            Arc::new(move |t: &str| {
                // Intercalado determinista: el PRIMER persist no termina
                // hasta que un segundo hilo haya persistido y (sin lock)
                // swapeado el suyo. Sin el write-lock alrededor de
                // persist+swap se produce persist(A) → persist(B) → swap(B)
                // → swap(A): fichero=B, memoria=A. Con el lock, el segundo
                // hilo no entra a persistir hasta que el primero termina, así
                // que el primero sigue al agotar el deadline (50 ms).
                let me = n.fetch_add(1, Ordering::SeqCst);
                sink.lock().unwrap().push(t.to_string());
                if me == 0 {
                    let deadline = std::time::Instant::now() + Duration::from_millis(50);
                    while n.load(Ordering::SeqCst) < 2 && std::time::Instant::now() < deadline {
                        std::thread::yield_now();
                    }
                    // Deja al segundo hilo completar su swap (sin lock).
                    std::thread::sleep(Duration::from_millis(5));
                }
                true
            }),
        ));
        let a = {
            let st = st.clone();
            std::thread::spawn(move || st.rotate_token())
        };
        std::thread::sleep(Duration::from_millis(5));
        let b = {
            let st = st.clone();
            std::thread::spawn(move || st.rotate_if_expired())
        };
        a.join().unwrap();
        b.join().unwrap();
        let last = persisted.lock().unwrap().last().cloned().unwrap();
        assert_eq!(
            st.token_value(),
            last,
            "el fichero debe contener el token vigente"
        );
    }

    #[test]
    fn ttl_banner_format() {
        assert_eq!(humantime_secs(43200), "12h");
        assert_eq!(humantime_secs(300), "5min");
        assert_eq!(humantime_secs(90), "90s");
    }
}
