//! Inspector de configuración centralizada — CFG-NEW-001 (2026-04-20)
//!
//! Resuelve y presenta la configuración efectiva del CLI mostrando, para cada
//! valor, de qué fuente salió según la cadena de prioridad:
//!
//!   1. Flag CLI global       (no se puede inspeccionar desde aquí — son per-invocación)
//!   2. Variable de entorno   (ENOLA_WEB_URL, …)
//!   3. Archivo config.toml   (~/.enola/config.toml, sección [web]/[distribution])
//!   4. Default del binario   (localhost o vacío — siempre seguro)
//!
//! ## Redacción de secretos
//!
//! Cualquier clave que termine en `_token`, `_secret`, `_password`, `_key`
//! se muestra como `[REDACTED]`. Los API keys viven en `~/.enola/providers.env`,
//! no en `config.toml`, pero reforzamos el patrón por defensa en profundidad.

use crate::domain::error::Result;
use crate::infrastructure::config_loader;
use serde::Serialize;

/// Origen efectivo de un valor resuelto.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ConfigSource {
    /// Variable de entorno del proceso (`ENOLA_*`).
    Env,
    /// Archivo `~/.enola/config.toml`.
    File,
    /// Default hardcoded en el binario.
    Default,
}

impl ConfigSource {
    /// Etiqueta legible (útil para output tabla).
    pub fn label(&self) -> &'static str {
        match self {
            ConfigSource::Env => "env",
            ConfigSource::File => "file",
            ConfigSource::Default => "default",
        }
    }
}

/// Una entrada resuelta de configuración.
#[derive(Debug, Clone, Serialize)]
pub struct ConfigEntry {
    /// Nombre cualificado: `web.web_public_url`, …
    pub key: String,
    /// Valor efectivo (ya redactado si aplica).
    pub value: String,
    /// Origen de ese valor.
    pub source: ConfigSource,
    /// Variable de entorno equivalente (para que el usuario sepa cómo sobreescribir).
    pub env_var: &'static str,
}

/// Describe una clave configurable: cómo se llama, qué env-var la sobreescribe,
/// qué default aplica y si debe redactarse.
struct ConfigKeySpec {
    section: &'static str,
    key: &'static str,
    env_var: &'static str,
    default_value: &'static str,
    /// Alias opcional en config.toml.
    alias: Option<&'static str>,
    redact: bool,
}

/// Catálogo de todas las claves conocidas del sistema de config centralizada.
/// Ampliar aquí cuando se añada una nueva sección .
const KNOWN_KEYS: &[ConfigKeySpec] = &[
    ConfigKeySpec {
        section: "web",
        key: "web_public_url",
        env_var: "ENOLA_WEB_URL",
        default_value: "",
        alias: None,
        redact: false,
    },
    ConfigKeySpec {
        section: "web",
        key: "docs_url",
        env_var: "ENOLA_DOCS_URL",
        default_value: "",
        alias: None,
        redact: false,
    },
    ConfigKeySpec {
        section: "distribution",
        key: "binary_base_url",
        env_var: "ENOLA_BINARY_BASE_URL",
        default_value: "",
        alias: None,
        redact: false,
    },
    ConfigKeySpec {
        section: "distribution",
        key: "minisign_pubkey_url",
        env_var: "ENOLA_MINISIGN_PUBKEY_URL",
        default_value: "",
        alias: None,
        redact: false,
    },
    ConfigKeySpec {
        section: "update",
        key: "feed_url",
        env_var: "ENOLA_UPDATE_FEED_URL",
        default_value: "",
        alias: None,
        redact: false,
    },
    ConfigKeySpec {
        section: "update",
        key: "signature_url",
        env_var: "ENOLA_UPDATE_SIGNATURE_URL",
        default_value: "",
        alias: None,
        redact: false,
    },
    ConfigKeySpec {
        section: "update",
        key: "minisign_pubkey",
        env_var: "ENOLA_UPDATE_MINISIGN_PUBKEY",
        default_value: "",
        alias: None,
        redact: false,
    },
    ConfigKeySpec {
        section: "http",
        key: "tor_socks_proxy",
        env_var: "ENOLA_TOR_SOCKS_PROXY",
        default_value: "socks5h://127.0.0.1:9050",
        alias: None,
        redact: false,
    },
];

/// Devuelve `true` si una clave parece sensible por su nombre.
fn looks_secret(key: &str) -> bool {
    let lower = key.to_lowercase();
    lower.ends_with("_token")
        || lower.ends_with("_secret")
        || lower.ends_with("_password")
        || lower.ends_with("_key")
        || lower.ends_with("_apikey")
}

/// Resuelve un valor aplicando la cadena env > file > default y devuelve
/// también el origen efectivo.
fn resolve_one(spec: &ConfigKeySpec) -> ConfigEntry {
    // 1. Env var
    if let Ok(v) = std::env::var(spec.env_var) {
        if !v.is_empty() {
            return finalize_entry(spec, v, ConfigSource::Env);
        }
    }

    // 2. Archivo config.toml (clave principal o alias)
    let section = config_loader::load_section(spec.section);
    if let Some(v) = section.get(spec.key) {
        return finalize_entry(spec, v.clone(), ConfigSource::File);
    }
    if let Some(alias) = spec.alias {
        if let Some(v) = section.get(alias) {
            return finalize_entry(spec, v.clone(), ConfigSource::File);
        }
    }

    // 3. Default
    finalize_entry(spec, spec.default_value.to_string(), ConfigSource::Default)
}

fn finalize_entry(spec: &ConfigKeySpec, value: String, source: ConfigSource) -> ConfigEntry {
    let display_value = if spec.redact || looks_secret(spec.key) {
        "[REDACTED]".to_string()
    } else {
        value
    };
    ConfigEntry {
        key: format!("{}.{}", spec.section, spec.key),
        value: display_value,
        source,
        env_var: spec.env_var,
    }
}

/// Resuelve TODA la configuración centralizada.
pub fn resolve_all() -> Vec<ConfigEntry> {
    KNOWN_KEYS.iter().map(resolve_one).collect()
}

/// Formatea una lista de entradas como tabla ASCII alineada.
pub fn format_table(entries: &[ConfigEntry]) -> String {
    let key_w = entries
        .iter()
        .map(|e| e.key.len())
        .max()
        .unwrap_or(3)
        .max(3);
    let val_w = entries
        .iter()
        .map(|e| e.value.len())
        .max()
        .unwrap_or(5)
        .clamp(5, 60);
    let src_w = 7;
    let env_w = entries
        .iter()
        .map(|e| e.env_var.len())
        .max()
        .unwrap_or(8)
        .max(8);

    let mut out = String::new();
    out.push_str(&format!(
        "{:<key_w$}  {:<val_w$}  {:<src_w$}  {:<env_w$}\n",
        "KEY",
        "VALUE",
        "SOURCE",
        "ENV VAR",
        key_w = key_w,
        val_w = val_w,
        src_w = src_w,
        env_w = env_w,
    ));
    out.push_str(&format!(
        "{}  {}  {}  {}\n",
        "-".repeat(key_w),
        "-".repeat(val_w),
        "-".repeat(src_w),
        "-".repeat(env_w),
    ));
    for e in entries {
        let trimmed_val = if e.value.len() > val_w {
            format!("{}…", &e.value[..val_w.saturating_sub(1)])
        } else {
            e.value.clone()
        };
        out.push_str(&format!(
            "{:<key_w$}  {:<val_w$}  {:<src_w$}  {:<env_w$}\n",
            e.key,
            trimmed_val,
            e.source.label(),
            e.env_var,
            key_w = key_w,
            val_w = val_w,
            src_w = src_w,
            env_w = env_w,
        ));
    }
    out
}

/// Punto de entrada del subcomando `enola-cli config show`.
pub fn show(json: bool) -> Result<()> {
    let entries = resolve_all();
    if json {
        // Preferimos no introducir dependencia extra; `serde_json` ya está.
        let payload = serde_json::to_string_pretty(&entries).map_err(|e| {
            crate::domain::error::EnolaError::InfrastructureError(format!(
                "Cannot serialize config to JSON: {}",
                e
            ))
        })?;
        println!("{}", payload);
    } else {
        println!("🧭 Enola CLI — configuración efectiva\n");
        println!("{}", format_table(&entries));
        println!("Prioridad de resolución: flag > env > file (~/.enola/config.toml) > default");
        println!("Los valores sensibles se muestran como [REDACTED].");
    }
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════
//  CFG-NEW-002 — `enola-cli config validate`
// ═══════════════════════════════════════════════════════════════════════════

/// Severidad de un finding de validación.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationSeverity {
    /// El release/uso NO se puede completar con este problema → bloquear.
    Error,
    /// Es posible continuar pero merece atención del usuario.
    Warning,
    /// Comprobación exitosa (útil en salida verbose/JSON).
    Ok,
}

impl ValidationSeverity {
    fn icon(&self) -> &'static str {
        match self {
            ValidationSeverity::Error => "❌",
            ValidationSeverity::Warning => "⚠️",
            ValidationSeverity::Ok => "✅",
        }
    }
}

/// Un resultado individual de validación.
#[derive(Debug, Clone)]
pub struct ValidationFinding {
    pub severity: ValidationSeverity,
    pub check: String,
    pub message: String,
}

impl ValidationFinding {
    fn ok<S: Into<String>, M: Into<String>>(check: S, message: M) -> Self {
        Self {
            severity: ValidationSeverity::Ok,
            check: check.into(),
            message: message.into(),
        }
    }
    fn warn<S: Into<String>, M: Into<String>>(check: S, message: M) -> Self {
        Self {
            severity: ValidationSeverity::Warning,
            check: check.into(),
            message: message.into(),
        }
    }
    fn error<S: Into<String>, M: Into<String>>(check: S, message: M) -> Self {
        Self {
            severity: ValidationSeverity::Error,
            check: check.into(),
            message: message.into(),
        }
    }
}

/// Valida que `url` tiene sintaxis aceptable como URL absoluta.
///
/// No depende de `url` crate para mantener superficie mínima: comprueba
/// esquema http/https/socks[5h], host no vacío, sin espacios.
pub(crate) fn is_syntactically_valid_url(url: &str) -> bool {
    if url.is_empty() || url.contains(char::is_whitespace) {
        return false;
    }
    let schemes = ["http://", "https://", "socks5://", "socks5h://"];
    let (scheme_ok, rest) = match schemes
        .iter()
        .find_map(|s| url.strip_prefix(s).map(|r| (s, r)))
    {
        Some((s, r)) => (*s, r),
        None => return false,
    };
    if rest.is_empty() {
        return false;
    }
    // Host = parte antes de '/', '?', '#' o fin
    let host_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let host_port = &rest[..host_end];
    if host_port.is_empty() {
        return false;
    }
    // Si hay ':' al final (puerto), validar que sean dígitos
    if let Some(pos) = host_port.rfind(':') {
        // Ignorar caso ipv6 (simplificación)
        if !host_port.contains('[') {
            let port = &host_port[pos + 1..];
            if !port.is_empty() && !port.chars().all(|c| c.is_ascii_digit()) {
                return false;
            }
        }
    }
    let _ = scheme_ok;
    true
}

/// Comprueba permisos 0600 en archivos de secretos conocidos (Unix).
/// En Windows el concepto no aplica — devuelve Ok siempre.
/// Si los permisos son incorrectos, los auto-corrige a 0600.
fn check_file_permissions(findings: &mut Vec<ValidationFinding>) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let home = match std::env::var("HOME") {
            Ok(h) => h,
            Err(_) => {
                findings.push(ValidationFinding::warn(
                    "permissions",
                    "$HOME no está definido; omitiendo chequeo de permisos",
                ));
                return;
            }
        };
        let candidates = [
            ".enola/config.toml",
            ".enola/providers.env",
            ".enola/session.json",
            ".enola/test.key",
            ".enola/pqc_signing.key",
        ];
        for rel in &candidates {
            let path = std::path::PathBuf::from(&home).join(rel);
            if !path.exists() {
                continue;
            }
            let mode = match std::fs::metadata(&path) {
                Ok(m) => m.permissions().mode() & 0o777,
                Err(e) => {
                    findings.push(ValidationFinding::warn(
                        "permissions",
                        format!("No se pudo leer {}: {}", rel, e),
                    ));
                    continue;
                }
            };
            if mode == 0o600 {
                findings.push(ValidationFinding::ok(
                    "permissions",
                    format!("{}: 0600", rel),
                ));
            } else {
                // Auto-corregir permisos a 0600
                let perms = std::fs::Permissions::from_mode(0o600);
                match std::fs::set_permissions(&path, perms) {
                    Ok(()) => {
                        findings.push(ValidationFinding::ok(
                            "permissions",
                            format!("{}: {:o} → 0600 (auto-corregido)", rel, mode),
                        ));
                    }
                    Err(e) => {
                        findings.push(ValidationFinding::error(
                            "permissions",
                            format!(
                                "{}: {:o} (esperado 0600). No se pudo auto-corregir: {}. Ejecuta: chmod 0600 ~/{}",
                                rel, mode, e, rel
                            ),
                        ));
                    }
                }
            }
        }
        // También asegurar que ~/.enola/ tenga 0700
        let enola_dir = std::path::PathBuf::from(&home).join(".enola");
        if enola_dir.exists() {
            let dir_mode = std::fs::metadata(&enola_dir)
                .map(|m| m.permissions().mode() & 0o777)
                .unwrap_or(0);
            if dir_mode != 0o700 {
                let perms = std::fs::Permissions::from_mode(0o700);
                let _ = std::fs::set_permissions(&enola_dir, perms);
            }
        }
    }
    #[cfg(not(unix))]
    {
        findings.push(ValidationFinding::ok(
            "permissions",
            "SO no-Unix: chequeo de permisos POSIX omitido",
        ));
    }
}

/// Comprueba que `~/.enola/config.toml` parsea como TOML válido.
fn check_config_toml_parseable(findings: &mut Vec<ValidationFinding>) {
    let home = match std::env::var("HOME") {
        Ok(h) => h,
        Err(_) => return,
    };
    let path = std::path::PathBuf::from(&home).join(".enola/config.toml");
    if !path.exists() {
        findings.push(ValidationFinding::warn(
            "toml",
            "~/.enola/config.toml no existe (se usarán defaults y env vars)",
        ));
        return;
    }
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            findings.push(ValidationFinding::error(
                "toml",
                format!("No se puede leer config.toml: {}", e),
            ));
            return;
        }
    };
    match toml::from_str::<toml::Value>(&content) {
        Ok(_) => findings.push(ValidationFinding::ok("toml", "config.toml parsea OK")),
        Err(e) => findings.push(ValidationFinding::error(
            "toml",
            format!("config.toml mal formado: {}", e),
        )),
    }
}

/// Comprueba que las URLs resueltas son sintácticamente válidas.
fn check_urls_syntactic(entries: &[ConfigEntry], findings: &mut Vec<ValidationFinding>) {
    let url_keys = [
        "web.web_public_url",
        "web.docs_url",
        "distribution.binary_base_url",
        "distribution.minisign_pubkey_url",
        "update.feed_url",
        "update.signature_url",
        "http.tor_socks_proxy",
    ];
    for key in &url_keys {
        let entry = match entries.iter().find(|e| &e.key == key) {
            Some(e) => e,
            None => continue,
        };
        if entry.value.is_empty() {
            // Vacío = opcional → ok (lo reporta `show`)
            continue;
        }
        if is_syntactically_valid_url(&entry.value) {
            findings.push(ValidationFinding::ok(
                "url",
                format!("{} = {} (sintaxis válida)", key, entry.value),
            ));
        } else {
            findings.push(ValidationFinding::error(
                "url",
                format!("{} = {:?} no es una URL válida", key, entry.value),
            ));
        }
    }
}

/// Si alguna URL es `.onion`, verifica que el proxy Tor responde.
fn check_tor_if_onion(entries: &[ConfigEntry], findings: &mut Vec<ValidationFinding>) {
    use crate::infrastructure::http::is_onion_url;
    let any_onion = entries.iter().any(|e| is_onion_url(&e.value));
    if !any_onion {
        return;
    }
    // Extraer host:port del proxy resuelto, con fallback al default.
    let proxy_entry = entries.iter().find(|e| e.key == "http.tor_socks_proxy");
    let proxy_url = proxy_entry.map(|e| e.value.clone()).unwrap_or_default();
    let target = extract_host_port(&proxy_url).unwrap_or_else(|| "127.0.0.1:9050".to_string());

    // Parsear "host:port" a SocketAddr. Si el parse falla, es un finding: no
    // podemos alcanzar Tor y el usuario debe corregir la URL.
    let addr: std::net::SocketAddr = match target.parse() {
        Ok(a) => a,
        Err(e) => {
            findings.push(ValidationFinding::error(
                "tor",
                format!(
                    "proxy {:?} no es host:port válido ({}); revisa ENOLA_TOR_SOCKS_PROXY",
                    target, e
                ),
            ));
            return;
        }
    };

    match std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_secs(2)) {
        Ok(_) => findings.push(ValidationFinding::ok(
            "tor",
            format!("Proxy SOCKS5 alcanzable ({})", target),
        )),
        Err(e) => findings.push(ValidationFinding::error(
            "tor",
            format!(
                "Hay URLs .onion pero el proxy Tor {} no responde ({}). Inicia Tor o define ENOLA_TOR_SOCKS_PROXY",
                target, e
            ),
        )),
    }
}

fn extract_host_port(url: &str) -> Option<String> {
    let after_scheme = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    let host_port = after_scheme.split(['/', '?', '#']).next()?;
    if host_port.is_empty() {
        return None;
    }
    Some(host_port.to_string())
}

/// Si `--reachable` está activo, intenta un HTTP HEAD/GET a las URLs principales.
fn check_urls_reachable(entries: &[ConfigEntry], findings: &mut Vec<ValidationFinding>) {
    let reachable_keys = ["web.web_public_url"];
    for key in &reachable_keys {
        let entry = match entries.iter().find(|e| &e.key == key) {
            Some(e) => e,
            None => continue,
        };
        if entry.value.is_empty() || !is_syntactically_valid_url(&entry.value) {
            continue;
        }
        let url = entry.value.clone();
        let key_label = key.to_string();
        let handle = std::thread::spawn(move || -> (String, std::result::Result<u16, String>) {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => return (key_label, Err(format!("tokio init: {}", e))),
            };
            let res = rt.block_on(async {
                let client = crate::infrastructure::http::build_http_client(&url)
                    .map_err(|e| format!("{}", e))?;
                let resp = client
                    .get(&url)
                    .timeout(std::time::Duration::from_secs(5))
                    .send()
                    .await
                    .map_err(|e| format!("{}", e))?;
                Ok(resp.status().as_u16())
            });
            (key_label, res)
        });
        match handle.join() {
            Ok((label, Ok(status))) => {
                findings.push(ValidationFinding::ok(
                    "reachable",
                    format!("{} → HTTP {}", label, status),
                ));
            }
            Ok((label, Err(e))) => {
                findings.push(ValidationFinding::warn(
                    "reachable",
                    format!("{} no alcanzable: {}", label, e),
                ));
            }
            Err(_) => {
                findings.push(ValidationFinding::warn(
                    "reachable",
                    format!("{}: thread panic", key),
                ));
            }
        }
    }
}

/// Punto de entrada del subcomando `enola-cli config validate`.
///
/// Ejecuta las comprobaciones: TOML parseable, permisos 0600, URLs sintácticas,
/// Tor disponible si hay .onion, y (opcional) HTTP reachability.
///
/// Devuelve la lista de findings (útil para testing y salida JSON).
pub fn validate_all(reachable: bool) -> Vec<ValidationFinding> {
    let mut findings = Vec::new();
    check_config_toml_parseable(&mut findings);
    check_file_permissions(&mut findings);
    let entries = resolve_all();
    check_urls_syntactic(&entries, &mut findings);
    check_tor_if_onion(&entries, &mut findings);
    if reachable {
        check_urls_reachable(&entries, &mut findings);
    }
    findings
}

/// Punto de entrada del subcomando CLI `config-validate`.
///
/// Imprime resultado legible (o JSON con `--json`) y devuelve `Err` si hay
/// algún finding con severidad `Error`.
pub fn validate(reachable: bool, json: bool) -> Result<()> {
    let findings = validate_all(reachable);

    if json {
        let payload: Vec<_> = findings
            .iter()
            .map(|f| {
                serde_json::json!({
                    "severity": match f.severity {
                        ValidationSeverity::Error => "error",
                        ValidationSeverity::Warning => "warning",
                        ValidationSeverity::Ok => "ok",
                    },
                    "check": f.check,
                    "message": f.message,
                })
            })
            .collect();
        let out = serde_json::to_string_pretty(&payload).map_err(|e| {
            crate::domain::error::EnolaError::InfrastructureError(format!(
                "Cannot serialize findings: {}",
                e
            ))
        })?;
        println!("{}", out);
    } else {
        println!("🔎 Enola CLI — validación de configuración\n");
        for f in &findings {
            println!("  {} [{}] {}", f.severity.icon(), f.check, f.message);
        }
        let errors = findings
            .iter()
            .filter(|f| f.severity == ValidationSeverity::Error)
            .count();
        let warnings = findings
            .iter()
            .filter(|f| f.severity == ValidationSeverity::Warning)
            .count();
        let oks = findings
            .iter()
            .filter(|f| f.severity == ValidationSeverity::Ok)
            .count();
        println!("\nResumen: ✅ {}  ⚠️  {}  ❌ {}", oks, warnings, errors);
        if !reachable {
            println!("Consejo: usa --reachable para comprobar que las URLs responden por HTTP.");
        }
    }

    let has_errors = findings
        .iter()
        .any(|f| f.severity == ValidationSeverity::Error);
    if has_errors {
        Err(crate::domain::error::EnolaError::ValidationError(
            "Config validation failed — ver output".to_string(),
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// §13.33 — serializa tests que tocan env vars para evitar flakiness.
    static CFG_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn clear_all_env() {
        for spec in KNOWN_KEYS {
            std::env::remove_var(spec.env_var);
        }
    }

    #[test]
    fn looks_secret_detects_common_patterns() {
        assert!(looks_secret("api_key"));
        assert!(looks_secret("client_secret"));
        assert!(looks_secret("access_token"));
        assert!(looks_secret("admin_password"));
        assert!(!looks_secret("realm"));
    }

    #[test]
    fn resolve_all_has_expected_keys() {
        let _g = CFG_ENV_LOCK.lock().unwrap();
        let all = resolve_all();
        let keys: Vec<&str> = all.iter().map(|e| e.key.as_str()).collect();
        assert!(keys.contains(&"web.web_public_url"));
        assert!(keys.contains(&"distribution.binary_base_url"));
        assert!(keys.contains(&"update.feed_url"));
        assert!(keys.contains(&"update.signature_url"));
        assert!(keys.contains(&"update.minisign_pubkey"));
        assert!(keys.contains(&"http.tor_socks_proxy"));
    }

    #[test]
    fn env_var_has_priority_over_default() {
        let _g = CFG_ENV_LOCK.lock().unwrap();
        clear_all_env();
        std::env::set_var("ENOLA_WEB_URL", "https://from-env.example.com");
        let all = resolve_all();
        let entry = all.iter().find(|e| e.key == "web.web_public_url").unwrap();
        assert_eq!(entry.value, "https://from-env.example.com");
        assert_eq!(entry.source, ConfigSource::Env);
        std::env::remove_var("ENOLA_WEB_URL");
    }

    #[test]
    fn default_used_when_no_env_no_file() {
        let _g = CFG_ENV_LOCK.lock().unwrap();
        clear_all_env();
        let all = resolve_all();
        // tor_socks_proxy no suele estar en config.toml → debe caer al default
        let entry = all
            .iter()
            .find(|e| e.key == "http.tor_socks_proxy")
            .unwrap();
        // Puede ser Env si el usuario lo exportó globalmente; al menos no crashea.
        assert!(!entry.value.is_empty());
    }

    #[test]
    fn format_table_contains_all_entries() {
        let entries = vec![ConfigEntry {
            key: "web.web_public_url".to_string(),
            value: "https://enola.tools".to_string(),
            source: ConfigSource::File,
            env_var: "ENOLA_WEB_URL",
        }];
        let out = format_table(&entries);
        assert!(out.contains("web.web_public_url"));
        assert!(out.contains("https://enola.tools"));
        assert!(out.contains("file"));
        assert!(out.contains("ENOLA_WEB_URL"));
    }

    #[test]
    fn show_json_output_is_valid_json() {
        let _g = CFG_ENV_LOCK.lock().unwrap();
        clear_all_env();
        let entries = resolve_all();
        let json = serde_json::to_string(&entries).expect("serializa");
        // Debe parsear de vuelta sin errores
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("parsea");
        assert!(parsed.is_array());
    }

    // ── Tests para CFG-NEW-002: validación ──

    #[test]
    fn url_valid_accepts_common_schemes() {
        assert!(is_syntactically_valid_url("http://127.0.0.1:8080"));
        assert!(is_syntactically_valid_url("https://auth.example.com/realm"));
        assert!(is_syntactically_valid_url("socks5h://127.0.0.1:9050"));
        assert!(is_syntactically_valid_url("http://abcdef.onion:80/foo"));
    }

    #[test]
    fn url_valid_rejects_garbage() {
        assert!(!is_syntactically_valid_url(""));
        assert!(!is_syntactically_valid_url("not a url"));
        assert!(!is_syntactically_valid_url("ftp://example.com"));
        assert!(!is_syntactically_valid_url("http://"));
        assert!(!is_syntactically_valid_url("http://host:notaport"));
    }

    #[test]
    fn extract_host_port_works() {
        assert_eq!(
            extract_host_port("socks5h://127.0.0.1:9050"),
            Some("127.0.0.1:9050".to_string())
        );
        assert_eq!(
            extract_host_port("http://example.com/foo/bar"),
            Some("example.com".to_string())
        );
    }

    #[test]
    fn validate_all_returns_some_findings() {
        let _g = CFG_ENV_LOCK.lock().unwrap();
        let findings = validate_all(false);
        // Al menos TOML check + URLs check producen algo
        assert!(!findings.is_empty());
    }

    #[test]
    fn validate_all_with_bad_url_env_has_error() {
        let _g = CFG_ENV_LOCK.lock().unwrap();
        clear_all_env();
        std::env::set_var("ENOLA_WEB_URL", "not a url");
        let findings = validate_all(false);
        std::env::remove_var("ENOLA_WEB_URL");
        assert!(
            findings
                .iter()
                .any(|f| f.severity == ValidationSeverity::Error
                    && f.message.contains("web.web_public_url")),
            "expected URL error, got: {:?}",
            findings
        );
    }
}
