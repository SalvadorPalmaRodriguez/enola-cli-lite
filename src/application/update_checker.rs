// MED-01: Prevent unwrap/expect in non-test code — panics in critical paths
// can leave the system in a corrupted state (e.g. mid-apply_update).
#![warn(clippy::unwrap_used, clippy::expect_used)]
// UPD-CLI-001/002 (2026-05-06) — Verificador de actualizaciones y feed de advisories.
//
// ## Feed — esquema de advisories.json v1 (UPD-FEED-001)
//   URL: env ENOLA_UPDATE_FEED_URL → [update].feed_url en config.toml → DEFAULT_FEED_URL
//
// ## Privacidad
//   Solo hace GET sin datos del usuario. User-Agent: enola-cli/<version>.
//   Si la URL es .onion se enruta automáticamente por Tor (infrastructure::http).
//
// ## Caché — ~/.enola/update_cache.json (TTL 24h)
use crate::domain::error::Result;
use crate::infrastructure::config_loader;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Command;
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const CACHE_TTL_SECS: i64 = 86_400;
const EMBEDDED_MINISIGN_PUBKEY_FILE: &str = include_str!("../../minisign.pub");
const UPDATE_CACHE_VERSION: u32 = 2;
pub(crate) const UPDATE_EXIT_OK: i32 = 0;
pub(crate) const UPDATE_EXIT_CRITICAL_ADVISORY: i32 = 11;
pub(crate) const UPDATE_EXIT_BELOW_MIN_SUPPORTED: i32 = 12;
pub(crate) const UPDATE_EXIT_REQUIRED: i32 = 13;
pub(crate) const UPDATE_EXIT_FEED_INVALID: i32 = 20;
pub(crate) const UPDATE_EXIT_SIGNATURE_INVALID: i32 = 21;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UpdateFeedErrorKind {
    FeedInvalid,
    SignatureInvalid,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum UpdateCheckStatus {
    Ok,
    UpdateAvailable,
    UpdateRequired,
    CriticalAdvisory,
    BelowMinSupported,
    FeedInvalid,
    SignatureInvalid,
    FeedNotConfigured,
}

#[derive(Debug)]
struct UpdateFeedError {
    kind: UpdateFeedErrorKind,
    message: String,
}

impl UpdateFeedError {
    fn new(kind: UpdateFeedErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for UpdateFeedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}
// ══════════════════ Tipos del feed (UPD-FEED-001 v1) ══════════════════
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UpdateFeed {
    #[serde(default)]
    pub schema_version: String,
    #[serde(default)]
    pub latest: String,
    #[serde(default)]
    pub min_supported: String,
    #[serde(default)]
    pub download_url: String,
    #[serde(default)]
    pub published_at: String,
    #[serde(default)]
    pub docs_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub severity_summary: Option<SeveritySummary>,
    #[serde(default)]
    pub signature_urls: Vec<String>,
    #[serde(default)]
    pub advisories: Vec<Advisory>,
    #[serde(default)]
    pub pqc_milestones: Vec<PqcMilestone>,
    #[serde(default)]
    pub enforce_updates: bool,
    /// MED-02: Rotación de clave minisign vía feed.
    /// Si el operador rota la clave minisign, anuncia la nueva aquí,
    /// firmada con la clave ACTUAL. El cliente la persiste tras verificar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_pubkey: Option<NextPubkey>,
}

/// Nueva clave minisign anunciada en el feed, firmada con la clave actual.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NextPubkey {
    /// Nueva clave pública minisign (formato RW...).
    pub key: String,
    /// Firma minisign de la nueva clave, hecha con la clave ACTUAL.
    /// El mensaje firmado es la nueva clave en texto plano.
    pub signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SeveritySummary {
    #[serde(default)]
    pub critical: u32,
    #[serde(default)]
    pub high: u32,
    #[serde(default)]
    pub medium: u32,
    #[serde(default)]
    pub low: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Advisory {
    pub id: String,
    #[serde(default)]
    pub severity: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub affected_versions: Vec<String>,
    #[serde(default)]
    pub fixed_in: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PqcMilestone {
    pub id: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub target_version: String,
}
// ══════════════════ Caché local ══════════════════
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UpdateCache {
    #[serde(default)]
    pub cache_version: u32,
    pub checked_at: i64,
    #[serde(default)]
    pub source_fingerprint: String,
    #[serde(default)]
    pub schema_version: String,
    #[serde(default)]
    pub latest: String,
    #[serde(default)]
    pub min_supported: String,
    #[serde(default)]
    pub download_url: String,
    #[serde(default)]
    pub advisories_for_current: Vec<String>,
    #[serde(default)]
    pub pqc_milestones_pending: usize,
    #[serde(default)]
    pub advisories: Vec<Advisory>,
    #[serde(default)]
    pub pqc_milestones: Vec<PqcMilestone>,
    #[serde(default)]
    pub feed_verified: bool,
    #[serde(default)]
    pub enforce_updates: bool,
}
// ══════════════════ Resultado de comprobación ══════════════════
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateReport {
    pub current_version: String,
    pub latest_version: String,
    pub update_available: bool,
    pub below_min_supported: bool,
    pub enforce_updates: bool,
    pub download_url: String,
    pub active_advisories: Vec<Advisory>,
    pub pqc_milestones_pending: Vec<PqcMilestone>,
    pub feed_not_configured: bool,
    pub feed_verified: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feed_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feed_error_kind: Option<UpdateFeedErrorKind>,
    pub checked_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedVerificationReport {
    pub source: String,
    pub signature_source: String,
    pub report: UpdateReport,
}

impl FeedVerificationReport {
    pub(crate) fn exit_code(&self) -> i32 {
        self.report.exit_code()
    }

    pub(crate) fn human_summary(&self) -> String {
        if let Some(err) = &self.report.feed_error {
            return format!(
                "⚠️  verify-feed falló.\nsource: {}\nsignature: {}\nMotivo: {}",
                self.source, self.signature_source, err
            );
        }

        format!(
            "✅ verify-feed OK.\nsource: {}\nsignature: {}\n{}",
            self.source,
            self.signature_source,
            self.report.human_summary()
        )
    }

    pub(crate) fn json_value(&self) -> std::result::Result<serde_json::Value, serde_json::Error> {
        let mut value = self.report.json_value()?;
        if let serde_json::Value::Object(ref mut map) = value {
            map.insert("source".to_string(), serde_json::json!(self.source));
            map.insert(
                "signature_source".to_string(),
                serde_json::json!(self.signature_source),
            );
        }
        Ok(value)
    }
}
impl UpdateReport {
    pub fn human_summary(&self) -> String {
        if self.feed_not_configured {
            return "ℹ️  No hay feed de actualizaciones configurado.\n\
                     Configura ENOLA_UPDATE_FEED_URL o [update].feed_url en ~/.enola/config.toml"
                .to_string();
        }
        if let Some(err) = &self.feed_error {
            return format!(
                "⚠️  No se pudo verificar el feed de actualizaciones.\n\
                 Motivo: {}\n\
                 El CLI ignora el feed hasta que la firma sea válida.",
                err
            );
        }
        let mut lines: Vec<String> = Vec::new();
        let highest_severity = self.highest_advisory_severity();
        if self.below_min_supported {
            lines.push(format!(
                "🚨 Tu versión ({}) es inferior a la mínima soportada. ¡Actualiza urgentemente!",
                self.current_version
            ));
        } else if self.update_available {
            let dl = if self.download_url.is_empty() {
                String::new()
            } else {
                format!("\n   📦 Descarga: {}", self.download_url)
            };
            if self.enforce_updates {
                lines.push(format!(
                    "� Actualización OBLIGATORIA: {} → {}{}\n   El CLI no funcionará hasta que actualices.",
                    self.current_version, self.latest_version, dl
                ));
            } else {
                lines.push(format!(
                    "�💡 Nueva versión disponible: {} → {}{}",
                    self.current_version, self.latest_version, dl
                ));
            }
        } else {
            lines.push(format!(
                "✅ Estás usando la última versión ({}).",
                self.current_version
            ));
        }
        if !self.active_advisories.is_empty() {
            lines.push(String::new());
            lines.push(format!(
                "{} {} aviso(s) de seguridad afectan a tu versión{}:",
                if highest_severity.as_deref() == Some("critical") {
                    "🚨"
                } else {
                    "⚠️ "
                },
                self.active_advisories.len(),
                highest_severity
                    .as_deref()
                    .map(|s| format!(" (máxima severidad: {})", s))
                    .unwrap_or_default()
            ));
            for adv in &self.active_advisories {
                lines.push(format!(
                    "   [{:^8}] {} — {} (corregido en: {})",
                    adv.severity.to_uppercase(),
                    adv.id,
                    adv.title,
                    adv.fixed_in
                ));
            }
        }
        if !self.pqc_milestones_pending.is_empty() {
            lines.push(String::new());
            lines.push(format!(
                "🔬 {} hito(s) PQC pendientes:",
                self.pqc_milestones_pending.len()
            ));
            for m in &self.pqc_milestones_pending {
                lines.push(format!("   [{}] {}", m.id, m.description));
            }
        }
        lines.join("\n")
    }

    pub(crate) fn highest_advisory_severity(&self) -> Option<String> {
        highest_advisory_severity_for(&self.active_advisories)
    }

    pub(crate) fn status(&self) -> UpdateCheckStatus {
        if self.feed_not_configured {
            return UpdateCheckStatus::FeedNotConfigured;
        }
        if let Some(kind) = self.feed_error_kind {
            return match kind {
                UpdateFeedErrorKind::FeedInvalid => UpdateCheckStatus::FeedInvalid,
                UpdateFeedErrorKind::SignatureInvalid => UpdateCheckStatus::SignatureInvalid,
            };
        }
        if self.below_min_supported {
            return UpdateCheckStatus::BelowMinSupported;
        }
        if self.highest_advisory_severity().as_deref() == Some("critical") {
            return UpdateCheckStatus::CriticalAdvisory;
        }
        if self.update_available {
            if self.enforce_updates {
                return UpdateCheckStatus::UpdateRequired;
            }
            return UpdateCheckStatus::UpdateAvailable;
        }
        UpdateCheckStatus::Ok
    }

    pub(crate) fn exit_code(&self) -> i32 {
        match self.status() {
            UpdateCheckStatus::Ok
            | UpdateCheckStatus::UpdateAvailable
            | UpdateCheckStatus::FeedNotConfigured => UPDATE_EXIT_OK,
            UpdateCheckStatus::UpdateRequired => UPDATE_EXIT_REQUIRED,
            UpdateCheckStatus::CriticalAdvisory => UPDATE_EXIT_CRITICAL_ADVISORY,
            UpdateCheckStatus::BelowMinSupported => UPDATE_EXIT_BELOW_MIN_SUPPORTED,
            UpdateCheckStatus::FeedInvalid => UPDATE_EXIT_FEED_INVALID,
            UpdateCheckStatus::SignatureInvalid => UPDATE_EXIT_SIGNATURE_INVALID,
        }
    }

    pub(crate) fn json_value(&self) -> std::result::Result<serde_json::Value, serde_json::Error> {
        let mut value = serde_json::to_value(self)?;
        if let serde_json::Value::Object(ref mut map) = value {
            map.insert("status".to_string(), serde_json::to_value(self.status())?);
            map.insert("exit_code".to_string(), serde_json::json!(self.exit_code()));
            if let Some(severity) = self.highest_advisory_severity() {
                map.insert(
                    "highest_advisory_severity".to_string(),
                    serde_json::json!(severity),
                );
            }
        }
        Ok(value)
    }
}
// ══════════════════ Helpers ══════════════════

const DEFAULT_FEED_URL: &str =
    "https://salvadorpalmarodriguez.github.io/enola-cli-lite/feed/advisories.json";

pub fn feed_url() -> String {
    if let Ok(url) = std::env::var("ENOLA_UPDATE_FEED_URL") {
        if !url.is_empty() {
            return url;
        }
    }
    let section = config_loader::load_section("update");
    section
        .get("feed_url")
        .cloned()
        .filter(|u| !u.is_empty())
        .unwrap_or_else(|| DEFAULT_FEED_URL.to_string())
}
pub fn signature_url(feed_url: &str) -> String {
    if let Ok(url) = std::env::var("ENOLA_UPDATE_SIGNATURE_URL") {
        if !url.is_empty() {
            return url;
        }
    }
    let section = config_loader::load_section("update");
    if let Some(url) = section.get("signature_url") {
        if !url.is_empty() {
            return url.clone();
        }
    }
    if feed_url.is_empty() {
        String::new()
    } else {
        format!("{}.minisig", feed_url)
    }
}
/// MED-02: Path al archivo de claves minisign confiables persistidas.
/// Formato: JSON array de strings (claves públicas RW...).
fn trusted_minisign_keys_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    PathBuf::from(home)
        .join(".enola")
        .join("trusted_minisign_keys.json")
}

/// MED-02: Carga las claves minisign confiables persistidas localmente.
/// Devuelve un vector de claves públicas (strings RW...).
fn load_trusted_minisign_keys() -> Vec<String> {
    let path = trusted_minisign_keys_path();
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return vec![],
    };
    let keys: Vec<String> = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return vec![],
    };
    keys.into_iter().filter(|k| !k.trim().is_empty()).collect()
}

/// MED-02: Persiste una nueva clave minisign confiable (0600, atómico).
/// No duplica si la clave ya existe.
fn persist_trusted_minisign_key(new_key: &str) -> std::io::Result<()> {
    let trimmed = new_key.trim();
    if trimmed.is_empty() {
        return Ok(());
    }
    let mut keys = load_trusted_minisign_keys();
    if keys.iter().any(|k| k.trim() == trimmed) {
        return Ok(()); // Already persisted
    }
    keys.push(trimmed.to_string());
    let content = serde_json::to_string_pretty(&keys).unwrap_or_else(|_| "[]".to_string());
    let path = trusted_minisign_keys_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    crate::infrastructure::atomic_secret_file::write_secret_atomically(&path, content.as_bytes())
}

/// MED-02: Verifica que la firma de `next_pubkey` es válida con la clave actual.
/// La firma se verifica sobre el texto plano de la nueva clave.
fn verify_next_pubkey_signature(next: &NextPubkey, current_pubkey: &str) -> bool {
    if current_pubkey.trim().is_empty()
        || next.key.trim().is_empty()
        || next.signature.trim().is_empty()
    {
        return false;
    }
    // The message being signed is the new key text itself.
    verify_minisign_signature(next.key.as_bytes(), &next.signature, current_pubkey).is_ok()
}

pub fn update_minisign_pubkey() -> String {
    // 1. Env var override (highest priority)
    if let Ok(key) = std::env::var("ENOLA_UPDATE_MINISIGN_PUBKEY") {
        if !key.trim().is_empty() {
            return key.trim().to_string();
        }
    }
    // 2. config.toml [update].minisign_pubkey
    let section = config_loader::load_section("update");
    if let Some(key) = section.get("minisign_pubkey") {
        if !key.trim().is_empty() {
            return key.trim().to_string();
        }
    }
    // 3. MED-02: Persisted trusted keys (from feed rotation)
    let trusted = load_trusted_minisign_keys();
    if let Some(first) = trusted.first() {
        return first.trim().to_string();
    }
    // 4. Embedded key (lowest priority)
    EMBEDDED_MINISIGN_PUBKEY_FILE
        .lines()
        .find(|line| line.starts_with("RW"))
        .unwrap_or_default()
        .trim()
        .to_string()
}
pub fn cache_path() -> Result<PathBuf> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    Ok(PathBuf::from(home).join(".enola").join("update_cache.json"))
}

fn now_ts() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn source_fingerprint(feed_url: &str) -> String {
    use sha2::{Digest, Sha256};

    let material = format!(
        "feed={}|sig={}|key={}",
        feed_url,
        signature_url(feed_url),
        update_minisign_pubkey()
    );
    format!("{:x}", Sha256::digest(material.as_bytes()))
}

fn cache_is_fresh_for(cache: &UpdateCache) -> bool {
    if cache.checked_at == 0 {
        return false;
    }
    (now_ts() - cache.checked_at) < CACHE_TTL_SECS
}

fn normalized_advisory_severity(severity: &str) -> &'static str {
    let lower = severity.trim().to_ascii_lowercase();
    match lower.as_str() {
        "critical" => "critical",
        "high" => "high",
        "medium" => "medium",
        "low" => "low",
        _ => "unknown",
    }
}

fn advisory_severity_rank(severity: &str) -> u8 {
    match normalized_advisory_severity(severity) {
        "critical" => 4,
        "high" => 3,
        "medium" => 2,
        "low" => 1,
        _ => 0,
    }
}

fn highest_advisory_severity_for(advisories: &[Advisory]) -> Option<String> {
    advisories
        .iter()
        .max_by_key(|adv| advisory_severity_rank(&adv.severity))
        .map(|adv| normalized_advisory_severity(&adv.severity).to_string())
}

fn reusable_cache(feed_url: &str) -> Option<UpdateCache> {
    let cache = read_cache();
    if cache.cache_version != UPDATE_CACHE_VERSION {
        return None;
    }
    if !cache.feed_verified {
        return None;
    }
    if !cache_is_fresh_for(&cache) {
        return None;
    }
    if cache.source_fingerprint != source_fingerprint(feed_url) {
        return None;
    }
    Some(cache)
}

fn build_report_from_feed(feed: &UpdateFeed, checked_at: i64) -> UpdateReport {
    let update_available = is_newer(CURRENT_VERSION, &feed.latest);
    let below_min =
        !feed.min_supported.is_empty() && is_newer(CURRENT_VERSION, &feed.min_supported);

    let active_advisories: Vec<Advisory> = feed
        .advisories
        .iter()
        .filter(|a| version_is_affected(CURRENT_VERSION, &a.affected_versions))
        .cloned()
        .collect();

    let pqc_pending: Vec<PqcMilestone> = feed
        .pqc_milestones
        .iter()
        .filter(|m| m.status != "released")
        .cloned()
        .collect();

    UpdateReport {
        current_version: CURRENT_VERSION.to_string(),
        latest_version: feed.latest.clone(),
        update_available,
        below_min_supported: below_min,
        enforce_updates: feed.enforce_updates,
        download_url: feed.download_url.clone(),
        active_advisories,
        pqc_milestones_pending: pqc_pending,
        feed_not_configured: false,
        feed_verified: true,
        feed_error: None,
        feed_error_kind: None,
        checked_at,
    }
}

fn build_report_from_cache(cache: &UpdateCache) -> UpdateReport {
    let feed = UpdateFeed {
        schema_version: cache.schema_version.clone(),
        latest: cache.latest.clone(),
        min_supported: cache.min_supported.clone(),
        download_url: cache.download_url.clone(),
        published_at: String::new(),
        docs_url: String::new(),
        severity_summary: None,
        signature_urls: vec![],
        advisories: cache.advisories.clone(),
        pqc_milestones: cache.pqc_milestones.clone(),
        enforce_updates: cache.enforce_updates,
        next_pubkey: None,
    };
    build_report_from_feed(&feed, cache.checked_at)
}

fn write_verified_cache(feed_url: &str, feed: &UpdateFeed, checked_at: i64) {
    let report = build_report_from_feed(feed, checked_at);
    let cache = UpdateCache {
        cache_version: UPDATE_CACHE_VERSION,
        checked_at,
        source_fingerprint: source_fingerprint(feed_url),
        schema_version: feed.schema_version.clone(),
        latest: feed.latest.clone(),
        min_supported: feed.min_supported.clone(),
        download_url: feed.download_url.clone(),
        advisories_for_current: report
            .active_advisories
            .iter()
            .map(|a| a.id.clone())
            .collect(),
        pqc_milestones_pending: report.pqc_milestones_pending.len(),
        advisories: feed.advisories.clone(),
        pqc_milestones: feed.pqc_milestones.clone(),
        feed_verified: true,
        enforce_updates: feed.enforce_updates,
    };
    write_cache(&cache);
}
fn read_cache() -> UpdateCache {
    let path = match cache_path() {
        Ok(p) => p,
        Err(_) => return UpdateCache::default(),
    };
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return UpdateCache::default(),
    };
    serde_json::from_str(&content).unwrap_or_default()
}
fn write_cache(cache: &UpdateCache) {
    let path = match cache_path() {
        Ok(p) => p,
        Err(_) => return,
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(cache) {
        let _ = std::fs::write(&path, json);
    }
}
pub fn cache_is_fresh() -> bool {
    cache_is_fresh_for(&read_cache())
}
fn is_newer(current: &str, other: &str) -> bool {
    parse_semver(other) > parse_semver(current)
}
fn parse_semver(v: &str) -> (u32, u32, u32) {
    let parts: Vec<&str> = v.trim_start_matches('v').splitn(3, '.').collect();
    let major = parts.first().and_then(|s| s.parse().ok()).unwrap_or(0);
    let minor = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    let patch = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
    (major, minor, patch)
}
fn version_is_affected(version: &str, ranges: &[String]) -> bool {
    if ranges.is_empty() {
        return false;
    }
    let cv = parse_semver(version);
    for range in ranges {
        let range = range.trim();
        if let Some(rest) = range.strip_prefix(">=") {
            let rv = parse_semver(rest);
            if cv < rv {
                return false;
            }
        } else if let Some(rest) = range.strip_prefix('<') {
            let rv = parse_semver(rest);
            if cv >= rv {
                return false;
            }
        }
    }
    true
}
type UpdateFeedResult<T> = std::result::Result<T, UpdateFeedError>;

async fn fetch_feed_bytes(url: &str) -> UpdateFeedResult<Vec<u8>> {
    use crate::infrastructure::http::build_http_client;
    let client = build_http_client(url).map_err(|e| {
        UpdateFeedError::new(
            UpdateFeedErrorKind::FeedInvalid,
            format!("update feed client: {}", e),
        )
    })?;
    let resp = client
        .get(url)
        .header("User-Agent", format!("enola-cli/{}", CURRENT_VERSION))
        .send()
        .await
        .map_err(|e| {
            UpdateFeedError::new(
                UpdateFeedErrorKind::FeedInvalid,
                format!("update feed fetch: {}", e),
            )
        })?;
    if !resp.status().is_success() {
        return Err(UpdateFeedError::new(
            UpdateFeedErrorKind::FeedInvalid,
            format!("update feed returned HTTP {}", resp.status()),
        ));
    }
    resp.bytes().await.map(|b| b.to_vec()).map_err(|e| {
        UpdateFeedError::new(
            UpdateFeedErrorKind::FeedInvalid,
            format!("update feed bytes: {}", e),
        )
    })
}
async fn fetch_feed_signature(url: &str) -> UpdateFeedResult<String> {
    use crate::infrastructure::http::build_http_client;
    let client = build_http_client(url).map_err(|e| {
        UpdateFeedError::new(
            UpdateFeedErrorKind::SignatureInvalid,
            format!("update signature client: {}", e),
        )
    })?;
    let resp = client
        .get(url)
        .header("User-Agent", format!("enola-cli/{}", CURRENT_VERSION))
        .send()
        .await
        .map_err(|e| {
            UpdateFeedError::new(
                UpdateFeedErrorKind::SignatureInvalid,
                format!("update signature fetch: {}", e),
            )
        })?;
    if !resp.status().is_success() {
        return Err(UpdateFeedError::new(
            UpdateFeedErrorKind::SignatureInvalid,
            format!("update signature returned HTTP {}", resp.status()),
        ));
    }
    resp.text().await.map_err(|e| {
        UpdateFeedError::new(
            UpdateFeedErrorKind::SignatureInvalid,
            format!("update signature parse: {}", e),
        )
    })
}

fn source_is_http(source: &str) -> bool {
    source.starts_with("http://") || source.starts_with("https://")
}

async fn fetch_feed_bytes_from_source(source: &str) -> UpdateFeedResult<Vec<u8>> {
    if source_is_http(source) {
        return fetch_feed_bytes(source).await;
    }
    std::fs::read(source).map_err(|e| {
        UpdateFeedError::new(
            UpdateFeedErrorKind::FeedInvalid,
            format!("update feed read '{}': {}", source, e),
        )
    })
}

async fn fetch_feed_signature_from_source(source: &str) -> UpdateFeedResult<String> {
    if source_is_http(source) {
        return fetch_feed_signature(source).await;
    }
    std::fs::read_to_string(source).map_err(|e| {
        UpdateFeedError::new(
            UpdateFeedErrorKind::SignatureInvalid,
            format!("update signature read '{}': {}", source, e),
        )
    })
}

fn resolve_signature_source(source: &str, signature_override: Option<&str>) -> String {
    if let Some(override_source) = signature_override {
        if !override_source.trim().is_empty() {
            return override_source.trim().to_string();
        }
    }
    format!("{}.minisig", source)
}
/// SEC-003: Resolve and validate the minisign binary path.
///
/// If `ENOLA_MINISIGN_BIN` is set to an absolute path, validates that:
/// 1. The binary exists and is executable.
/// 2. Running `<bin> -V` outputs something containing "minisign" (identity check).
///
/// If validation fails, emits a warning and falls back to "minisign" from PATH.
/// If set to a simple name (no `/`), uses it directly (PATH lookup).
/// If unset, defaults to "minisign".
fn resolve_minisign_binary() -> String {
    let raw = std::env::var("ENOLA_MINISIGN_BIN").unwrap_or_else(|_| "minisign".to_string());

    // Simple name (no path separator) — use directly via PATH lookup.
    if !raw.contains('/') {
        return raw;
    }

    // Absolute or relative path — validate identity.
    let path = std::path::Path::new(&raw);
    if !path.exists() {
        eprintln!(
            "⚠️  ENOLA_MINISIGN_BIN points to non-existent path '{}'; falling back to 'minisign' from PATH.",
            raw
        );
        return "minisign".to_string();
    }

    // Identity check: run `<bin> -V` and verify output contains "minisign".
    match Command::new(&raw).arg("-V").output() {
        Ok(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let stderr = String::from_utf8_lossy(&out.stderr);
            let combined = format!("{}{}", stdout, stderr);
            if combined.contains("minisign") {
                raw
            } else {
                eprintln!(
                    "⚠️  ENOLA_MINISIGN_BIN '{}' does not appear to be minisign (identity check failed); falling back to 'minisign' from PATH.",
                    raw
                );
                "minisign".to_string()
            }
        }
        _ => {
            eprintln!(
                "⚠️  ENOLA_MINISIGN_BIN '{}' could not be executed; falling back to 'minisign' from PATH.",
                raw
            );
            "minisign".to_string()
        }
    }
}

fn verify_minisign_signature(
    feed_bytes: &[u8],
    signature: &str,
    pubkey: &str,
) -> UpdateFeedResult<()> {
    if pubkey.trim().is_empty() {
        return Err(UpdateFeedError::new(
            UpdateFeedErrorKind::SignatureInvalid,
            "empty minisign public key for update feed",
        ));
    }
    let minisign = resolve_minisign_binary();
    let dir = tempfile::tempdir().map_err(|e| {
        UpdateFeedError::new(
            UpdateFeedErrorKind::SignatureInvalid,
            format!("tempdir for update verify: {}", e),
        )
    })?;
    let msg_path = dir.path().join("advisories.json");
    let sig_path = dir.path().join("advisories.json.minisig");
    std::fs::write(&msg_path, feed_bytes).map_err(|e| {
        UpdateFeedError::new(
            UpdateFeedErrorKind::SignatureInvalid,
            format!("write temp feed: {}", e),
        )
    })?;
    std::fs::write(&sig_path, signature).map_err(|e| {
        UpdateFeedError::new(
            UpdateFeedErrorKind::SignatureInvalid,
            format!("write temp signature: {}", e),
        )
    })?;

    let output = Command::new(&minisign)
        .args([
            "-V",
            "-m",
            &msg_path.to_string_lossy(),
            "-x",
            &sig_path.to_string_lossy(),
            "-P",
            pubkey,
        ])
        .output()
        .map_err(|e| {
            UpdateFeedError::new(
                UpdateFeedErrorKind::SignatureInvalid,
                format!(
                    "failed to execute minisign for update feed verification: {}",
                    e
                ),
            )
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let detail = if !stderr.is_empty() {
            stderr
        } else if !stdout.is_empty() {
            stdout
        } else {
            "invalid minisign signature".to_string()
        };
        return Err(UpdateFeedError::new(
            UpdateFeedErrorKind::SignatureInvalid,
            format!("update feed signature verification failed: {}", detail),
        ));
    }
    Ok(())
}
async fn fetch_feed(url: &str) -> UpdateFeedResult<UpdateFeed> {
    let feed_bytes = fetch_feed_bytes(url).await?;
    let signature = fetch_feed_signature(&signature_url(url)).await?;
    let current_pubkey = update_minisign_pubkey();
    verify_minisign_signature(&feed_bytes, &signature, &current_pubkey)?;
    let feed = serde_json::from_slice::<UpdateFeed>(&feed_bytes).map_err(|e| {
        UpdateFeedError::new(
            UpdateFeedErrorKind::FeedInvalid,
            format!("update feed parse: {}", e),
        )
    })?;
    // MED-02: Process key rotation if announced in the feed.
    if let Some(ref next) = feed.next_pubkey {
        if verify_next_pubkey_signature(next, &current_pubkey) {
            if let Err(e) = persist_trusted_minisign_key(&next.key) {
                eprintln!("⚠️  Warning: failed to persist rotated minisign key: {}", e);
            }
        } else {
            eprintln!("⚠️  Warning: feed announced next_pubkey but signature verification failed — ignoring.");
        }
    }
    Ok(feed)
}

async fn fetch_feed_for_verify(
    source: &str,
    signature_source: &str,
) -> UpdateFeedResult<UpdateFeed> {
    let feed_bytes = fetch_feed_bytes_from_source(source).await?;
    let signature = fetch_feed_signature_from_source(signature_source).await?;
    verify_minisign_signature(&feed_bytes, &signature, &update_minisign_pubkey())?;
    serde_json::from_slice::<UpdateFeed>(&feed_bytes).map_err(|e| {
        UpdateFeedError::new(
            UpdateFeedErrorKind::FeedInvalid,
            format!("update feed parse: {}", e),
        )
    })
}

pub async fn verify_feed_source(
    source: &str,
    signature_override: Option<&str>,
) -> FeedVerificationReport {
    let now = now_ts();
    let signature_source = resolve_signature_source(source, signature_override);

    if source.trim().is_empty() {
        return FeedVerificationReport {
            source: source.to_string(),
            signature_source,
            report: UpdateReport {
                current_version: CURRENT_VERSION.to_string(),
                latest_version: String::new(),
                update_available: false,
                below_min_supported: false,
                enforce_updates: false,
                download_url: String::new(),
                active_advisories: vec![],
                pqc_milestones_pending: vec![],
                feed_not_configured: false,
                feed_verified: false,
                feed_error: Some("empty feed source".to_string()),
                feed_error_kind: Some(UpdateFeedErrorKind::FeedInvalid),
                checked_at: now,
            },
        };
    }

    match fetch_feed_for_verify(source, &signature_source).await {
        Ok(feed) => FeedVerificationReport {
            source: source.to_string(),
            signature_source,
            report: build_report_from_feed(&feed, now),
        },
        Err(e) => FeedVerificationReport {
            source: source.to_string(),
            signature_source,
            report: UpdateReport {
                current_version: CURRENT_VERSION.to_string(),
                latest_version: String::new(),
                update_available: false,
                below_min_supported: false,
                enforce_updates: false,
                download_url: String::new(),
                active_advisories: vec![],
                pqc_milestones_pending: vec![],
                feed_not_configured: false,
                feed_verified: false,
                feed_error: Some(e.to_string()),
                feed_error_kind: Some(e.kind),
                checked_at: now,
            },
        },
    }
}
pub async fn check_for_updates_with_options(url: &str, force: bool) -> UpdateReport {
    let now = now_ts();
    if url.is_empty() {
        return UpdateReport {
            current_version: CURRENT_VERSION.to_string(),
            latest_version: CURRENT_VERSION.to_string(),
            update_available: false,
            below_min_supported: false,
            enforce_updates: false,
            download_url: String::new(),
            active_advisories: vec![],
            pqc_milestones_pending: vec![],
            feed_not_configured: true,
            feed_verified: false,
            feed_error: None,
            feed_error_kind: None,
            checked_at: now,
        };
    }

    if !force {
        if let Some(cache) = reusable_cache(url) {
            return build_report_from_cache(&cache);
        }
    }

    match fetch_feed(url).await {
        Ok(feed) => {
            write_verified_cache(url, &feed, now);
            build_report_from_feed(&feed, now)
        }
        Err(e) => {
            tracing::warn!("update check failed (non-fatal): {}", e);
            UpdateReport {
                current_version: CURRENT_VERSION.to_string(),
                latest_version: String::new(),
                update_available: false,
                below_min_supported: false,
                enforce_updates: false,
                download_url: String::new(),
                active_advisories: vec![],
                pqc_milestones_pending: vec![],
                feed_not_configured: false,
                feed_verified: false,
                feed_error: Some(e.to_string()),
                feed_error_kind: Some(e.kind),
                checked_at: now,
            }
        }
    }
}

pub async fn check_for_updates(url: &str) -> UpdateReport {
    check_for_updates_with_options(url, false).await
}
/// Comprobación silenciosa en background.
/// Reutiliza caché verificada y fresca cuando existe; si no, intenta red.
/// Retorna Some(mensaje) si hay novedad relevante, None si todo está OK.
pub async fn background_update_hint() -> Option<String> {
    let url = feed_url();
    if url.is_empty() {
        return None;
    }
    let report = check_for_updates(&url).await;
    if report.feed_verified && (report.update_available || !report.active_advisories.is_empty()) {
        Some(report.human_summary())
    } else {
        None
    }
}

// ══════════════════ SELFUPDATE-001: Download + Apply ══════════════════

/// Result of a download operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadResult {
    pub current_version: String,
    pub latest_version: String,
    pub download_url: String,
    /// Path where the downloaded binary was saved.
    pub binary_path: String,
    /// SHA256 of the downloaded binary (verified).
    pub sha256: String,
    /// Whether minisign signature was verified.
    pub signature_verified: bool,
    /// Whether the binary was also applied.
    pub applied: bool,
    /// Whether this was a dry-run (no download).
    pub dry_run: bool,
    /// Error message if something failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Path for the last downloaded binary metadata.
fn last_download_meta_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    PathBuf::from(home)
        .join(".enola")
        .join("last_download.json")
}

/// Directory where downloaded binaries are persisted between `download` and `apply`.
fn downloads_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    PathBuf::from(home).join(".enola").join("downloads")
}

/// UPD-RESIDUE-001: remove leftover download artifacts after a successful apply.
///
/// Once the new binary is installed, the staged downloads in `~/.enola/downloads/`
/// and the `last_download.json` pointer serve no purpose and would otherwise
/// accumulate (~50-100 MB each) on the end user's machine. The rollback backup
/// (`enola-cli.bak`) is intentionally preserved. Best-effort: cleanup failures
/// never fail the update.
fn prune_download_residuals() {
    let dir = downloads_dir();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let is_staged_binary = path
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with("enola-cli-"))
                .unwrap_or(false);
            if is_staged_binary {
                let _ = std::fs::remove_file(&path);
            }
        }
        // Remove the directory if it ended up empty.
        let _ = std::fs::remove_dir(&dir);
    }
    let _ = std::fs::remove_file(last_download_meta_path());
}

/// Save metadata about the last download so `update apply` can find it.
fn save_last_download(path: &str, sha256: &str, signature_verified: bool) {
    let meta = serde_json::json!({
        "binary_path": path,
        "sha256": sha256,
        "signature_verified": signature_verified,
        "saved_at": now_ts(),
    });
    let meta_path = last_download_meta_path();
    if let Some(parent) = meta_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(
        &meta_path,
        serde_json::to_string_pretty(&meta).unwrap_or_default(),
    );
}

/// Load metadata about the last download.
/// Returns (binary_path, sha256, signature_verified).
fn load_last_download() -> Option<(String, String, bool)> {
    let meta_path = last_download_meta_path();
    let content = std::fs::read_to_string(&meta_path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&content).ok()?;
    let path = v.get("binary_path")?.as_str()?.to_string();
    let sha = v.get("sha256")?.as_str()?.to_string();
    let sig_verified = v
        .get("signature_verified")
        .and_then(|b| b.as_bool())
        .unwrap_or(false);
    Some((path, sha, sig_verified))
}

/// Download the latest binary from the update feed.
/// Verifies SHA256 and minisign signature.
/// Saves the binary to a temp directory and returns the path.
pub async fn download_update(force_feed: bool) -> std::result::Result<DownloadResult, String> {
    let url = feed_url();
    if url.is_empty() {
        return Err("No update feed configured. Set ENOLA_UPDATE_FEED_URL or [update].feed_url in config.toml".to_string());
    }

    let report = check_for_updates_with_options(&url, force_feed).await;
    if !report.feed_verified {
        return Err(report
            .feed_error
            .unwrap_or_else(|| "Feed verification failed".to_string()));
    }
    if !report.update_available {
        return Err(format!(
            "Already on latest version ({}). No update needed.",
            report.current_version
        ));
    }
    if report.download_url.is_empty() {
        return Err("Update feed does not include a download_url".to_string());
    }

    let dl_url = &report.download_url;
    let sha_url = format!("{}.sha256", dl_url);
    let sig_url = format!("{}.minisig", dl_url);

    // Download binary
    let tmp = tempfile::tempdir().map_err(|e| format!("tempdir: {}", e))?;
    let binary_path = tmp.path().join("enola-cli-new");
    let sha_path = tmp.path().join("enola-cli-new.sha256");
    let sig_path = tmp.path().join("enola-cli-new.minisig");

    // Use extended timeout for the binary download (large file, possibly over Tor).
    // Small files (.sha256, .minisig) use the standard 15s timeout.
    let download_client = crate::infrastructure::http::build_download_client(dl_url)
        .map_err(|e| format!("HTTP download client: {}", e))?;
    let client = crate::infrastructure::http::build_http_client(dl_url)
        .map_err(|e| format!("HTTP client: {}", e))?;

    // Download binary (extended timeout)
    let resp = download_client
        .get(dl_url)
        .header("User-Agent", format!("enola-cli/{}", CURRENT_VERSION))
        .send()
        .await
        .map_err(|e| format!("Download binary: {}", e))?;
    if !resp.status().is_success() {
        return Err(format!("Download binary: HTTP {}", resp.status()));
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("Download binary bytes: {}", e))?;
    std::fs::write(&binary_path, &bytes).map_err(|e| format!("Write binary: {}", e))?;

    // Download SHA256 (small file, standard timeout)
    let resp = client
        .get(&sha_url)
        .header("User-Agent", format!("enola-cli/{}", CURRENT_VERSION))
        .send()
        .await
        .map_err(|e| format!("Download SHA256: {}", e))?;
    if !resp.status().is_success() {
        return Err(format!("Download SHA256: HTTP {}", resp.status()));
    }
    let sha_content = resp
        .text()
        .await
        .map_err(|e| format!("Download SHA256 text: {}", e))?;
    std::fs::write(&sha_path, &sha_content).map_err(|e| format!("Write SHA256: {}", e))?;

    // Verify SHA256
    let expected_sha = sha_content
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_string();
    let actual_sha = {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        format!("{:x}", hasher.finalize())
    };
    if expected_sha.is_empty() || expected_sha != actual_sha {
        return Err(format!(
            "SHA256 mismatch: expected {}, got {}",
            expected_sha, actual_sha
        ));
    }

    // Download and verify minisign signature
    let mut signature_verified = false;
    let resp = client
        .get(&sig_url)
        .header("User-Agent", format!("enola-cli/{}", CURRENT_VERSION))
        .send()
        .await;
    if let Ok(resp) = resp {
        if resp.status().is_success() {
            if let Ok(sig_content) = resp.text().await {
                std::fs::write(&sig_path, &sig_content).ok();
                let pubkey = update_minisign_pubkey();
                if !pubkey.is_empty() {
                    let minisign_bin = resolve_minisign_binary();
                    let output = Command::new(&minisign_bin)
                        .args([
                            "-V",
                            "-m",
                            &binary_path.to_string_lossy(),
                            "-x",
                            &sig_path.to_string_lossy(),
                            "-P",
                            &pubkey,
                        ])
                        .output();
                    if let Ok(out) = output {
                        signature_verified = out.status.success();
                    }
                }
            }
        }
    }

    // Persist binary to a stable location (not tempdir which gets cleaned up)
    let persist_dir = downloads_dir();
    std::fs::create_dir_all(&persist_dir).ok();
    let persist_path = persist_dir.join(format!("enola-cli-{}", report.latest_version));
    std::fs::copy(&binary_path, &persist_path).map_err(|e| format!("Persist binary: {}", e))?;
    let persist_str = persist_path.to_string_lossy().to_string();

    // MED-05: fail if signature was not verified, unless escape hatch is set.
    if !signature_verified {
        if std::env::var("ENOLA_ALLOW_UNSIGNED_UPDATE").as_deref() != Ok("1") {
            // Still save metadata so user can inspect the binary, but return error.
            save_last_download(&persist_str, &actual_sha, false);
            return Err(format!(
                "Signature verification failed or minisign unavailable. \
                 Binary saved at: {}. \
                 To apply anyway, set ENOLA_ALLOW_UNSIGNED_UPDATE=1 or use --allow-unsigned.",
                persist_str
            ));
        }
        eprintln!("⚠️  WARNING: applying update without minisign signature verification.");
    }

    save_last_download(&persist_str, &actual_sha, signature_verified);

    Ok(DownloadResult {
        current_version: report.current_version,
        latest_version: report.latest_version,
        download_url: dl_url.clone(),
        binary_path: persist_str,
        sha256: actual_sha,
        signature_verified,
        applied: false,
        dry_run: false,
        error: None,
    })
}

/// Apply a downloaded update: replace the current binary atomically.
/// Requires root. Backs up old binary to enola-cli.bak.
pub fn apply_update(binary_path: Option<&str>) -> std::result::Result<DownloadResult, String> {
    use std::os::unix::fs::PermissionsExt;

    // Determine which binary to apply
    let (bin_path, expected_sha, sig_verified) = match binary_path {
        Some(p) => {
            let sha = {
                use sha2::{Digest, Sha256};
                let data = std::fs::read(p).map_err(|e| format!("Read binary: {}", e))?;
                let mut hasher = Sha256::new();
                hasher.update(&data);
                format!("{:x}", hasher.finalize())
            };
            // When applying by explicit path, we can't know if signature was verified.
            // Warn but allow — the user chose this path explicitly.
            eprintln!("⚠️  WARNING: applying binary by explicit path — signature verification status unknown.");
            (p.to_string(), sha, true)
        }
        None => {
            let (p, s, sig) = load_last_download().ok_or_else(|| {
                "No previous download found. Run 'enola-cli update download' first.".to_string()
            })?;
            // MED-05: reject if signature was not verified, unless escape hatch is set.
            if !sig {
                if std::env::var("ENOLA_ALLOW_UNSIGNED_UPDATE").as_deref() != Ok("1") {
                    return Err(
                        "Cannot apply update: the downloaded binary was not signature-verified. \
                         To override, set ENOLA_ALLOW_UNSIGNED_UPDATE=1 or use --allow-unsigned."
                            .to_string(),
                    );
                }
                eprintln!("⚠️  WARNING: applying update without minisign signature verification.");
            }
            (p, s, sig)
        }
    };

    // Verify the binary exists
    if !std::path::Path::new(&bin_path).exists() {
        return Err(format!("Binary not found: {}", bin_path));
    }

    // Find current binary
    let current_exe = std::env::current_exe()
        .map_err(|e| format!("Cannot determine current binary path: {}", e))?;
    let install_path = current_exe.to_string_lossy().to_string();

    // Determine share dir for backup and sha256 file
    let share_dir = current_exe
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.join("share").join("enola"))
        .unwrap_or_else(|| PathBuf::from("/usr/local/share/enola"));
    let backup_path = share_dir.join("enola-cli.bak");
    let sha256_path = share_dir.join("cli.sha256");

    // Backup current binary
    if std::path::Path::new(&install_path).exists() {
        std::fs::copy(&install_path, &backup_path)
            .map_err(|e| format!("Backup current binary: {}", e))?;
    }

    // Make new binary executable
    let perms = std::fs::Permissions::from_mode(0o755);
    std::fs::set_permissions(&bin_path, perms)
        .map_err(|e| format!("Set permissions on new binary: {}", e))?;

    // Atomic replace: rename new binary to install path.
    // If rename fails (cross-device, common in WSL2), fall back to copy + remove.
    if let Err(e) = std::fs::rename(&bin_path, &install_path) {
        std::fs::copy(&bin_path, &install_path)
            .map_err(|e2| format!("Rename failed: {}, copy also failed: {}", e, e2))?;
        let _ = std::fs::remove_file(&bin_path);
    }

    // Update cli.sha256
    if let Some(parent) = sha256_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&sha256_path, format!("{}  enola-cli", expected_sha));

    // UPD-RESIDUE-001: clean up staged downloads now that the new binary is live.
    // The rollback backup (enola-cli.bak) is kept on purpose.
    prune_download_residuals();

    Ok(DownloadResult {
        current_version: CURRENT_VERSION.to_string(),
        latest_version: String::new(),
        download_url: String::new(),
        binary_path: install_path,
        sha256: expected_sha,
        signature_verified: sig_verified,
        applied: true,
        dry_run: false,
        error: None,
    })
}

/// Dry-run: show what would happen without downloading.
pub async fn dry_run_update(force_feed: bool) -> std::result::Result<DownloadResult, String> {
    let url = feed_url();
    if url.is_empty() {
        return Err("No update feed configured. Set ENOLA_UPDATE_FEED_URL or [update].feed_url in config.toml".to_string());
    }

    let report = check_for_updates_with_options(&url, force_feed).await;
    if !report.feed_verified {
        return Err(report
            .feed_error
            .unwrap_or_else(|| "Feed verification failed".to_string()));
    }

    Ok(DownloadResult {
        current_version: report.current_version,
        latest_version: report.latest_version,
        download_url: report.download_url,
        binary_path: String::new(),
        sha256: String::new(),
        signature_verified: false,
        applied: false,
        dry_run: true,
        error: None,
    })
}
// ══════════════════ Tests ══════════════════
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::await_holding_lock)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static UPDATE_ENV_LOCK: Mutex<()> = Mutex::new(());

    fn setup_test_home() -> (tempfile::TempDir, std::sync::MutexGuard<'static, ()>) {
        let guard = UPDATE_ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let tmp = tempfile::tempdir().expect("tempdir must be created");
        std::env::set_var("HOME", tmp.path());
        std::env::remove_var("ENOLA_UPDATE_FEED_URL");
        std::env::remove_var("ENOLA_UPDATE_SIGNATURE_URL");
        std::env::remove_var("ENOLA_UPDATE_MINISIGN_PUBKEY");
        (tmp, guard)
    }

    fn teardown_test_home(_tmp: tempfile::TempDir, _guard: std::sync::MutexGuard<'static, ()>) {
        std::env::remove_var("HOME");
        std::env::remove_var("ENOLA_UPDATE_FEED_URL");
        std::env::remove_var("ENOLA_UPDATE_SIGNATURE_URL");
        std::env::remove_var("ENOLA_UPDATE_MINISIGN_PUBKEY");
    }

    // ── SEC-003: Tests for resolve_minisign_binary ──────────────────────────

    #[test]
    fn resolve_minisign_binary_defaults_to_minisign_when_unset() {
        let _guard = UPDATE_ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        std::env::remove_var("ENOLA_MINISIGN_BIN");
        let result = resolve_minisign_binary();
        assert_eq!(result, "minisign");
    }

    #[test]
    fn resolve_minisign_binary_uses_simple_name_directly() {
        let _guard = UPDATE_ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        std::env::set_var("ENOLA_MINISIGN_BIN", "my-minisign");
        let result = resolve_minisign_binary();
        assert_eq!(result, "my-minisign");
        std::env::remove_var("ENOLA_MINISIGN_BIN");
    }

    #[test]
    fn resolve_minisign_binary_falls_back_for_nonexistent_path() {
        let _guard = UPDATE_ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        std::env::set_var("ENOLA_MINISIGN_BIN", "/nonexistent/path/to/minisign");
        let result = resolve_minisign_binary();
        assert_eq!(result, "minisign");
        std::env::remove_var("ENOLA_MINISIGN_BIN");
    }

    #[test]
    fn resolve_minisign_binary_falls_back_for_non_minisign_binary() {
        let _guard = UPDATE_ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        // /bin/echo exists and runs but is NOT minisign
        std::env::set_var("ENOLA_MINISIGN_BIN", "/bin/echo");
        let result = resolve_minisign_binary();
        assert_eq!(result, "minisign");
        std::env::remove_var("ENOLA_MINISIGN_BIN");
    }

    #[test]
    fn resolve_minisign_binary_falls_back_for_unexecutable_path() {
        let _guard = UPDATE_ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        // /etc/hostname exists but is not executable
        std::env::set_var("ENOLA_MINISIGN_BIN", "/etc/hostname");
        let result = resolve_minisign_binary();
        assert_eq!(result, "minisign");
        std::env::remove_var("ENOLA_MINISIGN_BIN");
    }

    #[test]
    fn is_newer_correctly_compares_semver() {
        assert!(is_newer("1.4.0", "1.5.0"));
        assert!(is_newer("1.4.9", "1.5.0"));
        assert!(!is_newer("1.5.0", "1.4.0"));
        assert!(!is_newer("1.5.0", "1.5.0"));
        assert!(is_newer("1.5.0", "2.0.0"));
        assert!(is_newer("0.9.9", "1.0.0"));
    }
    #[test]
    fn version_is_affected_handles_less_than_range() {
        let ranges = vec!["<1.4.1".to_string()];
        assert!(version_is_affected("1.4.0", &ranges));
        assert!(!version_is_affected("1.4.1", &ranges));
        assert!(!version_is_affected("1.5.0", &ranges));
    }
    #[test]
    fn version_is_affected_handles_gte_and_lt_range() {
        let ranges = vec![">=1.3.0".to_string(), "<1.4.1".to_string()];
        assert!(version_is_affected("1.3.5", &ranges));
        assert!(!version_is_affected("1.2.9", &ranges));
        assert!(!version_is_affected("1.4.1", &ranges));
    }
    #[test]
    fn version_is_affected_empty_ranges_returns_false() {
        assert!(!version_is_affected("1.4.0", &[]));
    }
    #[test]
    fn update_report_not_configured_shows_hint() {
        let report = UpdateReport {
            current_version: "1.4.0".to_string(),
            latest_version: "1.4.0".to_string(),
            update_available: false,
            below_min_supported: false,
            enforce_updates: false,
            download_url: String::new(),
            active_advisories: vec![],
            pqc_milestones_pending: vec![],
            feed_not_configured: true,
            feed_verified: false,
            feed_error: None,
            feed_error_kind: None,
            checked_at: 0,
        };
        let summary = report.human_summary();
        assert!(summary.contains("ENOLA_UPDATE_FEED_URL") || summary.contains("feed_url"));
    }
    #[test]
    fn update_report_shows_new_version_with_url() {
        let report = UpdateReport {
            current_version: "1.4.0".to_string(),
            latest_version: "1.5.0".to_string(),
            update_available: true,
            below_min_supported: false,
            enforce_updates: false,
            download_url: "download-placeholder".to_string(),
            active_advisories: vec![],
            pqc_milestones_pending: vec![],
            feed_not_configured: false,
            feed_verified: true,
            feed_error: None,
            feed_error_kind: None,
            checked_at: 0,
        };
        let summary = report.human_summary();
        assert!(summary.contains("1.4.0"));
        assert!(summary.contains("1.5.0"));
        assert!(summary.contains("download-placeholder"));
    }
    #[test]
    fn update_report_shows_advisory_info() {
        let report = UpdateReport {
            current_version: "1.4.0".to_string(),
            latest_version: "1.5.0".to_string(),
            update_available: true,
            below_min_supported: false,
            enforce_updates: false,
            download_url: String::new(),
            active_advisories: vec![Advisory {
                id: "ENOLA-ADV-2026-001".to_string(),
                severity: "high".to_string(),
                title: "JWT bypass".to_string(),
                description: String::new(),
                affected_versions: vec!["<1.5.0".to_string()],
                fixed_in: "1.5.0".to_string(),
            }],
            pqc_milestones_pending: vec![],
            feed_not_configured: false,
            feed_verified: true,
            feed_error: None,
            feed_error_kind: None,
            checked_at: 0,
        };
        let summary = report.human_summary();
        assert!(summary.contains("ENOLA-ADV-2026-001"));
        assert!(summary.contains("HIGH"));
        assert!(summary.contains("JWT bypass"));
    }
    #[test]
    fn update_report_up_to_date_shows_ok() {
        let report = UpdateReport {
            current_version: "1.5.0".to_string(),
            latest_version: "1.5.0".to_string(),
            update_available: false,
            below_min_supported: false,
            enforce_updates: false,
            download_url: String::new(),
            active_advisories: vec![],
            pqc_milestones_pending: vec![],
            feed_not_configured: false,
            feed_verified: true,
            feed_error: None,
            feed_error_kind: None,
            checked_at: 0,
        };
        let summary = report.human_summary();
        assert!(summary.contains("✅"));
        assert!(summary.contains("1.5.0"));
    }
    #[test]
    fn update_feed_deserializes_with_defaults() {
        let json = r#"{"latest":"1.5.0","download_url":"https://example.com"}"#;
        let feed: UpdateFeed = serde_json::from_str(json).unwrap();
        assert_eq!(feed.latest, "1.5.0");
        assert!(feed.published_at.is_empty());
        assert!(feed.docs_url.is_empty());
        assert!(feed.severity_summary.is_none());
        assert!(feed.signature_urls.is_empty());
        assert!(feed.advisories.is_empty());
        assert!(feed.pqc_milestones.is_empty());
    }

    #[test]
    fn update_feed_deserializes_optional_v1_fields() {
        let json = r#"{
            "schema_version": "1",
            "latest": "1.5.0",
            "min_supported": "1.4.0",
            "download_url": "https://example.com/release.tar.gz",
            "published_at": "2026-05-07T12:00:00Z",
            "docs_url": "https://example.com/advisories/2026-05-07",
            "severity_summary": {"critical": 1, "high": 2, "medium": 3, "low": 4},
            "signature_urls": ["https://example.com/advisories.json.minisig"],
            "advisories": [],
            "pqc_milestones": []
        }"#;
        let feed: UpdateFeed = serde_json::from_str(json).unwrap();
        assert_eq!(feed.published_at, "2026-05-07T12:00:00Z");
        assert_eq!(feed.docs_url, "https://example.com/advisories/2026-05-07");
        assert_eq!(feed.signature_urls.len(), 1);
        let summary = feed
            .severity_summary
            .expect("severity_summary should be present");
        assert_eq!(summary.critical, 1);
        assert_eq!(summary.high, 2);
        assert_eq!(summary.medium, 3);
        assert_eq!(summary.low, 4);
    }
    #[test]
    fn parse_semver_handles_v_prefix() {
        assert_eq!(parse_semver("v1.5.0"), (1, 5, 0));
        assert_eq!(parse_semver("1.5.0"), (1, 5, 0));
        assert_eq!(parse_semver("2.0.0"), (2, 0, 0));
    }

    #[test]
    fn signature_url_defaults_to_sidecar_minisig() {
        assert_eq!(
            signature_url("https://example.com/advisories.json"),
            "https://example.com/advisories.json.minisig"
        );
    }

    #[test]
    fn embedded_minisign_pubkey_is_loaded() {
        let key = update_minisign_pubkey();
        assert!(key.starts_with("RW"));
        assert!(key.len() > 20);
    }

    #[test]
    fn resolve_signature_source_prefers_explicit_override() {
        let resolved = resolve_signature_source(
            "https://example.com/advisories.json",
            Some("https://example.com/sig.asc"),
        );
        assert_eq!(resolved, "https://example.com/sig.asc");
    }

    #[test]
    fn resolve_signature_source_defaults_to_sidecar() {
        let resolved = resolve_signature_source("/tmp/advisories.json", None);
        assert_eq!(resolved, "/tmp/advisories.json.minisig");
    }

    #[test]
    fn verify_feed_report_json_includes_sources() {
        let report = FeedVerificationReport {
            source: "feed-placeholder".to_string(),
            signature_source: "sig-placeholder".to_string(),
            report: UpdateReport {
                current_version: "1.4.0".to_string(),
                latest_version: "1.4.0".to_string(),
                update_available: false,
                below_min_supported: false,
                enforce_updates: false,
                download_url: String::new(),
                active_advisories: vec![],
                pqc_milestones_pending: vec![],
                feed_not_configured: false,
                feed_verified: true,
                feed_error: None,
                feed_error_kind: None,
                checked_at: 0,
            },
        };

        let json = report.json_value().expect("json must serialize");
        assert_eq!(
            json.get("source").and_then(|v| v.as_str()),
            Some("feed-placeholder")
        );
        assert_eq!(
            json.get("signature_source").and_then(|v| v.as_str()),
            Some("sig-placeholder")
        );
    }

    #[test]
    fn update_report_with_feed_error_shows_verification_warning() {
        let report = UpdateReport {
            current_version: "1.4.0".to_string(),
            latest_version: String::new(),
            update_available: false,
            below_min_supported: false,
            enforce_updates: false,
            download_url: String::new(),
            active_advisories: vec![],
            pqc_milestones_pending: vec![],
            feed_not_configured: false,
            feed_verified: false,
            feed_error: Some("signature invalid".to_string()),
            feed_error_kind: Some(UpdateFeedErrorKind::SignatureInvalid),
            checked_at: 0,
        };
        let summary = report.human_summary();
        assert!(summary.contains("No se pudo verificar el feed"));
        assert!(summary.contains("signature invalid"));
    }

    #[test]
    fn highest_advisory_severity_prefers_critical() {
        let report = UpdateReport {
            current_version: "1.4.0".to_string(),
            latest_version: "1.5.0".to_string(),
            update_available: true,
            below_min_supported: false,
            enforce_updates: false,
            download_url: String::new(),
            active_advisories: vec![
                Advisory {
                    id: "ENOLA-ADV-2026-LOW".to_string(),
                    severity: "low".to_string(),
                    title: "Minor".to_string(),
                    description: String::new(),
                    affected_versions: vec![">=1.0.0".to_string(), "<2.0.0".to_string()],
                    fixed_in: "2.0.0".to_string(),
                },
                Advisory {
                    id: "ENOLA-ADV-2026-CRIT".to_string(),
                    severity: "critical".to_string(),
                    title: "Critical".to_string(),
                    description: String::new(),
                    affected_versions: vec![">=1.0.0".to_string(), "<2.0.0".to_string()],
                    fixed_in: "2.0.0".to_string(),
                },
            ],
            pqc_milestones_pending: vec![],
            feed_not_configured: false,
            feed_verified: true,
            feed_error: None,
            feed_error_kind: None,
            checked_at: 0,
        };
        assert_eq!(
            report.highest_advisory_severity().as_deref(),
            Some("critical")
        );
        assert_eq!(report.status(), UpdateCheckStatus::CriticalAdvisory);
        assert_eq!(report.exit_code(), UPDATE_EXIT_CRITICAL_ADVISORY);
    }

    #[test]
    fn below_min_supported_has_priority_over_advisories() {
        let report = UpdateReport {
            current_version: "1.3.0".to_string(),
            latest_version: "1.5.0".to_string(),
            update_available: true,
            below_min_supported: true,
            enforce_updates: false,
            download_url: String::new(),
            active_advisories: vec![Advisory {
                id: "ENOLA-ADV-2026-CRIT".to_string(),
                severity: "critical".to_string(),
                title: "Critical".to_string(),
                description: String::new(),
                affected_versions: vec![">=1.0.0".to_string(), "<2.0.0".to_string()],
                fixed_in: "2.0.0".to_string(),
            }],
            pqc_milestones_pending: vec![],
            feed_not_configured: false,
            feed_verified: true,
            feed_error: None,
            feed_error_kind: None,
            checked_at: 0,
        };
        assert_eq!(report.status(), UpdateCheckStatus::BelowMinSupported);
        assert_eq!(report.exit_code(), UPDATE_EXIT_BELOW_MIN_SUPPORTED);
    }

    #[test]
    fn feed_error_kinds_map_to_distinct_exit_codes() {
        let feed_invalid = UpdateReport {
            current_version: "1.4.0".to_string(),
            latest_version: String::new(),
            update_available: false,
            below_min_supported: false,
            enforce_updates: false,
            download_url: String::new(),
            active_advisories: vec![],
            pqc_milestones_pending: vec![],
            feed_not_configured: false,
            feed_verified: false,
            feed_error: Some("bad json".to_string()),
            feed_error_kind: Some(UpdateFeedErrorKind::FeedInvalid),
            checked_at: 0,
        };
        let signature_invalid = UpdateReport {
            feed_error_kind: Some(UpdateFeedErrorKind::SignatureInvalid),
            feed_error: Some("bad signature".to_string()),
            ..feed_invalid.clone()
        };
        assert_eq!(feed_invalid.status(), UpdateCheckStatus::FeedInvalid);
        assert_eq!(feed_invalid.exit_code(), UPDATE_EXIT_FEED_INVALID);
        assert_eq!(
            signature_invalid.status(),
            UpdateCheckStatus::SignatureInvalid
        );
        assert_eq!(signature_invalid.exit_code(), UPDATE_EXIT_SIGNATURE_INVALID);
    }

    #[test]
    fn json_value_contains_status_and_exit_code() {
        let report = UpdateReport {
            current_version: "1.4.0".to_string(),
            latest_version: "1.5.0".to_string(),
            update_available: true,
            below_min_supported: false,
            enforce_updates: false,
            download_url: String::new(),
            active_advisories: vec![Advisory {
                id: "ENOLA-ADV-2026-CRIT".to_string(),
                severity: "critical".to_string(),
                title: "Critical".to_string(),
                description: String::new(),
                affected_versions: vec![">=1.0.0".to_string(), "<2.0.0".to_string()],
                fixed_in: "2.0.0".to_string(),
            }],
            pqc_milestones_pending: vec![],
            feed_not_configured: false,
            feed_verified: true,
            feed_error: None,
            feed_error_kind: None,
            checked_at: 0,
        };
        let value = report.json_value().expect("json serialization must work");
        assert_eq!(
            value.get("status").and_then(|v| v.as_str()),
            Some("critical_advisory")
        );
        assert_eq!(value.get("exit_code").and_then(|v| v.as_i64()), Some(11));
        assert_eq!(
            value
                .get("highest_advisory_severity")
                .and_then(|v| v.as_str()),
            Some("critical")
        );
    }

    #[test]
    fn enforce_updates_true_with_update_returns_required_exit_13() {
        let report = UpdateReport {
            current_version: "1.4.0".to_string(),
            latest_version: "1.5.0".to_string(),
            update_available: true,
            below_min_supported: false,
            enforce_updates: true,
            download_url: "https://example.com/enola-cli".to_string(),
            active_advisories: vec![],
            pqc_milestones_pending: vec![],
            feed_not_configured: false,
            feed_verified: true,
            feed_error: None,
            feed_error_kind: None,
            checked_at: 0,
        };
        assert_eq!(report.status(), UpdateCheckStatus::UpdateRequired);
        assert_eq!(report.exit_code(), UPDATE_EXIT_REQUIRED);
        assert_eq!(report.exit_code(), 13);
    }

    #[test]
    fn enforce_updates_false_with_update_returns_available_exit_0() {
        let report = UpdateReport {
            current_version: "1.4.0".to_string(),
            latest_version: "1.5.0".to_string(),
            update_available: true,
            below_min_supported: false,
            enforce_updates: false,
            download_url: "https://example.com/enola-cli".to_string(),
            active_advisories: vec![],
            pqc_milestones_pending: vec![],
            feed_not_configured: false,
            feed_verified: true,
            feed_error: None,
            feed_error_kind: None,
            checked_at: 0,
        };
        assert_eq!(report.status(), UpdateCheckStatus::UpdateAvailable);
        assert_eq!(report.exit_code(), UPDATE_EXIT_OK);
        assert_eq!(report.exit_code(), 0);
    }

    #[test]
    fn minisign_verifies_known_release_artifact_when_available() {
        let minisign = resolve_minisign_binary();
        if Command::new(&minisign).arg("-v").output().is_err() {
            return;
        }

        let project_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let artifact_path = project_root.join("web/releases/enola-cli-v1.4.0-linux-x86_64.tar.gz");
        let signature_path =
            project_root.join("web/releases/enola-cli-v1.4.0-linux-x86_64.tar.gz.minisig");
        if !artifact_path.exists() || !signature_path.exists() {
            return;
        }

        let bytes = std::fs::read(&artifact_path).expect("release artifact should be readable");
        let sig = std::fs::read_to_string(&signature_path).expect("minisig should be readable");
        let pubkey = update_minisign_pubkey();
        verify_minisign_signature(&bytes, &sig, &pubkey)
            .expect("known release artifact should verify with embedded minisign key");
    }

    #[test]
    fn reusable_cache_accepts_fresh_verified_matching_source() {
        let (tmp, guard) = setup_test_home();
        let feed = "feed-placeholder";
        let cache = UpdateCache {
            cache_version: UPDATE_CACHE_VERSION,
            checked_at: now_ts(),
            source_fingerprint: source_fingerprint(feed),
            schema_version: "1".to_string(),
            latest: "1.5.0".to_string(),
            min_supported: "1.4.0".to_string(),
            download_url: "release-placeholder".to_string(),
            advisories_for_current: vec!["ENOLA-ADV-1".to_string()],
            pqc_milestones_pending: 1,
            advisories: vec![Advisory {
                id: "ENOLA-ADV-1".to_string(),
                severity: "high".to_string(),
                title: "Test".to_string(),
                description: String::new(),
                affected_versions: vec![">=1.0.0".to_string(), "<2.0.0".to_string()],
                fixed_in: "2.0.0".to_string(),
            }],
            pqc_milestones: vec![],
            feed_verified: true,
            enforce_updates: false,
        };
        write_cache(&cache);

        let reused = reusable_cache(feed);
        assert!(reused.is_some());
        assert_eq!(reused.unwrap().latest, "1.5.0");
        teardown_test_home(tmp, guard);
    }

    #[tokio::test]
    async fn check_for_updates_uses_fresh_cache_without_network() {
        let (tmp, guard) = setup_test_home();
        let feed = "http://127.0.0.1:9/advisories.json";
        std::env::set_var("ENOLA_UPDATE_FEED_URL", feed);

        let cache = UpdateCache {
            cache_version: UPDATE_CACHE_VERSION,
            checked_at: now_ts(),
            source_fingerprint: source_fingerprint(feed),
            schema_version: "1".to_string(),
            latest: "9.9.9".to_string(),
            min_supported: "1.0.0".to_string(),
            download_url: "cached-release-placeholder".to_string(),
            advisories_for_current: vec![],
            pqc_milestones_pending: 0,
            advisories: vec![],
            pqc_milestones: vec![],
            feed_verified: true,
            enforce_updates: false,
        };
        write_cache(&cache);

        let report = check_for_updates_with_options(feed, false).await;
        assert_eq!(report.latest_version, "9.9.9");
        assert!(report.feed_verified);
        assert!(report.feed_error.is_none());
        teardown_test_home(tmp, guard);
    }

    #[tokio::test]
    async fn force_bypasses_cache_and_hits_network() {
        let (tmp, guard) = setup_test_home();
        let feed = "http://127.0.0.1:9/advisories.json";
        std::env::set_var("ENOLA_UPDATE_FEED_URL", feed);

        let cache = UpdateCache {
            cache_version: UPDATE_CACHE_VERSION,
            checked_at: now_ts(),
            source_fingerprint: source_fingerprint(feed),
            schema_version: "1".to_string(),
            latest: "9.9.9".to_string(),
            min_supported: "1.0.0".to_string(),
            download_url: "cached-release-placeholder".to_string(),
            advisories_for_current: vec![],
            pqc_milestones_pending: 0,
            advisories: vec![],
            pqc_milestones: vec![],
            feed_verified: true,
            enforce_updates: false,
        };
        write_cache(&cache);

        let report = check_for_updates_with_options(feed, true).await;
        assert!(!report.feed_verified);
        assert!(report.feed_error.is_some());
        assert_ne!(report.latest_version, "9.9.9");
        teardown_test_home(tmp, guard);
    }

    #[tokio::test]
    async fn background_hint_uses_cached_update() {
        let (tmp, guard) = setup_test_home();
        let feed = "http://127.0.0.1:9/advisories.json";
        std::env::set_var("ENOLA_UPDATE_FEED_URL", feed);

        let cache = UpdateCache {
            cache_version: UPDATE_CACHE_VERSION,
            checked_at: now_ts(),
            source_fingerprint: source_fingerprint(feed),
            schema_version: "1".to_string(),
            latest: "9.9.9".to_string(),
            min_supported: "0.0.1".to_string(),
            download_url: "cached-release-placeholder".to_string(),
            advisories_for_current: vec![],
            pqc_milestones_pending: 0,
            advisories: vec![],
            pqc_milestones: vec![],
            feed_verified: true,
            enforce_updates: false,
        };
        write_cache(&cache);

        let hint = background_update_hint().await;
        assert!(hint.is_some());
        assert!(hint.unwrap().contains("9.9.9"));
        teardown_test_home(tmp, guard);
    }

    #[test]
    fn verify_minisign_empty_pubkey_returns_error() {
        // Rotación: clave vacía (antes de cargar la nueva) → error controlado
        let err = verify_minisign_signature(b"some feed content", "fake sig", "");
        assert!(err.is_err());
        let msg = err.unwrap_err().to_string();
        assert!(
            msg.contains("empty") || msg.contains("key"),
            "mensaje: {}",
            msg
        );
    }

    #[test]
    fn verify_minisign_whitespace_only_pubkey_returns_error() {
        // Clave con solo espacios (caso edge de rotación incompleta)
        let err = verify_minisign_signature(b"feed", "sig", "   \n  ");
        assert!(err.is_err());
    }

    #[test]
    fn update_minisign_pubkey_env_override_replaces_embedded() {
        // Simula rotación de clave: el operador pone la nueva clave en env var
        // El cliente con la env var nueva usa esa clave en lugar de la embebida.
        let (tmp, guard) = setup_test_home();
        std::env::remove_var("ENOLA_UPDATE_MINISIGN_PUBKEY");
        let embedded = update_minisign_pubkey();
        assert!(
            embedded.starts_with("RW"),
            "embedded key debe empezar con RW"
        );

        // Override con clave de rotación ficticia
        std::env::set_var("ENOLA_UPDATE_MINISIGN_PUBKEY", "RWTestRotatedKeyXYZ");
        let rotated = update_minisign_pubkey();
        assert_eq!(rotated, "RWTestRotatedKeyXYZ");
        assert_ne!(rotated, embedded, "key rotada debe diferir de la embebida");
        teardown_test_home(tmp, guard);
    }

    #[test]
    fn update_minisign_pubkey_blank_env_falls_back_to_embedded() {
        // Si la env var existe pero está vacía, usa la clave embebida (seguro)
        let (tmp, guard) = setup_test_home();
        std::env::set_var("ENOLA_UPDATE_MINISIGN_PUBKEY", "   ");
        let key = update_minisign_pubkey();
        assert!(key.starts_with("RW"), "fallback a embedded: {}", key);
        teardown_test_home(tmp, guard);
    }

    #[test]
    fn update_minisign_pubkey_env_trimmed() {
        // La clave con espacios en env se devuelve trimmed
        let (tmp, guard) = setup_test_home();
        std::env::set_var("ENOLA_UPDATE_MINISIGN_PUBKEY", "  RWTrimmedKey123  ");
        let key = update_minisign_pubkey();
        assert_eq!(key, "RWTrimmedKey123");
        teardown_test_home(tmp, guard);
    }

    #[test]
    fn verify_minisign_wrong_pubkey_returns_signature_error() {
        // Simula cliente con clave vieja tras rotación: el feed nuevo firmado
        // con clave nueva NO verifica con clave vieja → error controlado, no panic.
        // Solo ejecuta si minisign está disponible.
        let minisign = resolve_minisign_binary();
        if Command::new(&minisign).arg("-v").output().is_err() {
            return; // minisign no disponible en este entorno → skip
        }
        let fake_feed = b"latest: 2.0.0\n";
        let fake_sig = "untrusted comment: signature from minisign\nRWRfakebase64sigXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX\n";
        let old_pubkey = "RWOldKeyBase64XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX";
        let err = verify_minisign_signature(fake_feed, fake_sig, old_pubkey);
        assert!(err.is_err(), "clave vieja no debe verificar firma nueva");
    }

    // Rotación de secretos: secreto viejo rechaza, secreto nuevo acepta
    // Aquí testamos la lógica de rotación a nivel de update_checker:
    // UpdateFeedError display + error kind mapping.
    #[test]
    fn update_feed_error_signature_invalid_display() {
        let err = UpdateFeedError::new(
            UpdateFeedErrorKind::SignatureInvalid,
            "webhook secret rotated: old HMAC rejected",
        );
        let msg = err.to_string();
        assert!(msg.contains("old HMAC rejected"), "msg: {}", msg);
    }

    #[test]
    fn update_feed_error_kinds_are_distinct() {
        let kinds = [
            UpdateFeedErrorKind::FeedInvalid,
            UpdateFeedErrorKind::SignatureInvalid,
        ];
        // Todos son distintos (enum sin repetición)
        let strs: Vec<String> = kinds.iter().map(|k| format!("{:?}", k)).collect();
        let unique: std::collections::HashSet<_> = strs.iter().collect();
        assert_eq!(
            unique.len(),
            kinds.len(),
            "UpdateFeedErrorKind deben ser distintos"
        );
    }

    #[test]
    fn hostile_update_feed_json_corpus_never_panics() {
        // SEC-EXT-DEV-070: parser del advisory feed debe tolerar input hostil
        // devolviendo Err o estructura por defecto, sin panic.
        let corpus: Vec<&[u8]> = vec![
            b"{",                                                          // truncado
            b"[]",                                                         // tipo raiz incorrecto
            b"null",                                                       // null
            br#"{\"latest\":123,\"advisories\":\"oops\"}"#,                // tipos incorrectos
            br#"{\"latest\":\"1.5.0\",\"advisories\":[{\"id\":\"A\"}] }"#, // parcial
            br#"{\"schema_version\":\"1\",\"latest\":\"1.5.0\"}"#,         // mínimo válido
            &[0, 159, 146, 150],                                           // bytes no UTF-8
        ];

        for sample in corpus {
            let _ = serde_json::from_slice::<UpdateFeed>(sample);
        }
    }

    #[test]
    fn prune_download_residuals_removes_staged_binaries_and_meta() {
        // UPD-RESIDUE-001: tras aplicar, los binarios descargados y el puntero
        // last_download.json deben desaparecer para no acumular residuos en el
        // equipo del usuario final. El backup enola-cli.bak NO lo gestiona esta
        // función (vive en el share dir del sistema).
        let (tmp, guard) = setup_test_home();

        let dir = downloads_dir();
        std::fs::create_dir_all(&dir).expect("create downloads dir");
        std::fs::write(dir.join("enola-cli-1.4.0"), b"old").expect("write old binary");
        std::fs::write(dir.join("enola-cli-1.5.0"), b"new").expect("write new binary");
        // Fichero ajeno: no debe eliminarse (solo prefijo enola-cli-).
        std::fs::write(dir.join("README.txt"), b"keep").expect("write foreign file");
        save_last_download(
            &dir.join("enola-cli-1.5.0").to_string_lossy(),
            "deadbeef",
            true,
        );
        assert!(last_download_meta_path().exists());

        prune_download_residuals();

        assert!(
            !dir.join("enola-cli-1.4.0").exists(),
            "old binary debe borrarse"
        );
        assert!(
            !dir.join("enola-cli-1.5.0").exists(),
            "new binary debe borrarse"
        );
        assert!(
            !last_download_meta_path().exists(),
            "last_download.json debe borrarse"
        );
        // Como quedó un fichero ajeno, el directorio NO se elimina.
        assert!(
            dir.join("README.txt").exists(),
            "fichero ajeno debe preservarse"
        );

        teardown_test_home(tmp, guard);
    }

    #[test]
    fn prune_download_residuals_removes_empty_dir() {
        let (tmp, guard) = setup_test_home();

        let dir = downloads_dir();
        std::fs::create_dir_all(&dir).expect("create downloads dir");
        std::fs::write(dir.join("enola-cli-2.0.0"), b"bin").expect("write binary");

        prune_download_residuals();

        assert!(!dir.exists(), "downloads dir vacío debe eliminarse");

        teardown_test_home(tmp, guard);
    }

    // --- MED-05: apply_update exige firma minisign ---

    #[test]
    fn apply_update_rejects_unsigned_download() {
        let (tmp, guard) = setup_test_home();
        std::env::remove_var("ENOLA_ALLOW_UNSIGNED_UPDATE");

        // Simulate a download metadata with signature_verified = false
        let fake_bin = tmp.path().join("fake-binary");
        std::fs::write(&fake_bin, b"fake binary content").expect("write fake bin");
        save_last_download(
            &fake_bin.to_string_lossy(),
            "abc123",
            false, // signature NOT verified
        );

        let result = apply_update(None);
        assert!(
            result.is_err(),
            "apply_update must reject unsigned download"
        );
        let err = result.unwrap_err();
        assert!(
            err.contains("not signature-verified"),
            "error must mention signature verification: {}",
            err
        );

        teardown_test_home(tmp, guard);
    }

    #[test]
    fn apply_update_allows_unsigned_with_escape_hatch() {
        let (tmp, guard) = setup_test_home();
        std::env::set_var("ENOLA_ALLOW_UNSIGNED_UPDATE", "1");

        // Simulate a download metadata with signature_verified = false
        let fake_bin = tmp.path().join("fake-binary-escape");
        std::fs::write(&fake_bin, b"fake binary content").expect("write fake bin");
        save_last_download(
            &fake_bin.to_string_lossy(),
            "abc123",
            false, // signature NOT verified
        );

        // apply_update should proceed past the signature check (it will fail later
        // because the binary path doesn't match current_exe, but the error should
        // NOT be about signature verification).
        let result = apply_update(None);
        // It will fail at some point (e.g. binary not found at install path),
        // but the error should NOT mention "not signature-verified".
        if let Err(e) = &result {
            assert!(
                !e.contains("not signature-verified"),
                "escape hatch should bypass signature check: {}",
                e
            );
        }

        std::env::remove_var("ENOLA_ALLOW_UNSIGNED_UPDATE");
        teardown_test_home(tmp, guard);
    }

    #[test]
    fn load_last_download_preserves_signature_flag() {
        let (tmp, guard) = setup_test_home();

        save_last_download("/tmp/fake", "deadbeef", true);
        let (path, sha, sig) = load_last_download().expect("metadata should load");
        assert_eq!(path, "/tmp/fake");
        assert_eq!(sha, "deadbeef");
        assert!(sig, "signature_verified must be true");

        save_last_download("/tmp/fake2", "cafe", false);
        let (path2, sha2, sig2) = load_last_download().expect("metadata should load");
        assert_eq!(path2, "/tmp/fake2");
        assert_eq!(sha2, "cafe");
        assert!(!sig2, "signature_verified must be false");

        teardown_test_home(tmp, guard);
    }

    // --- MED-02: Rotación de clave minisign vía feed ---

    #[test]
    fn trusted_minisign_keys_persist_and_load() {
        let (tmp, guard) = setup_test_home();

        // Initially empty
        assert!(load_trusted_minisign_keys().is_empty());

        // Persist a key
        persist_trusted_minisign_key("RWTTestKey1234567890").expect("persist should work");
        let keys = load_trusted_minisign_keys();
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0], "RWTTestKey1234567890");

        // Persist same key again — should not duplicate
        persist_trusted_minisign_key("RWTTestKey1234567890").expect("persist duplicate");
        let keys = load_trusted_minisign_keys();
        assert_eq!(keys.len(), 1, "duplicate key should not be added");

        // Persist a different key
        persist_trusted_minisign_key("RWTAnotherKey987654321").expect("persist second key");
        let keys = load_trusted_minisign_keys();
        assert_eq!(keys.len(), 2);

        // File should have 0600 permissions
        let path = trusted_minisign_keys_path();
        let perms = std::fs::metadata(&path).expect("metadata").permissions();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                perms.mode() & 0o777,
                0o600,
                "trusted keys file must be 0600"
            );
        }

        teardown_test_home(tmp, guard);
    }

    #[test]
    fn update_minisign_pubkey_prefers_trusted_over_embedded() {
        let (tmp, guard) = setup_test_home();
        // Ensure env and config don't interfere
        std::env::remove_var("ENOLA_UPDATE_MINISIGN_PUBKEY");

        // Without trusted keys, should fall back to embedded
        let embedded = update_minisign_pubkey();
        assert!(!embedded.is_empty(), "embedded key should not be empty");

        // With a trusted key, should use it instead
        persist_trusted_minisign_key("RWPrioritizedTrustedKey").expect("persist");
        let resolved = update_minisign_pubkey();
        assert_eq!(resolved, "RWPrioritizedTrustedKey");

        teardown_test_home(tmp, guard);
    }

    #[test]
    fn next_pubkey_deserializes_from_feed() {
        let json = r#"{
            "schema_version": "1",
            "latest": "1.5.0",
            "min_supported": "1.0.0",
            "download_url": "https://example.com/enola-cli-1.5.0.tar.gz",
            "advisories": [],
            "pqc_milestones": [],
            "enforce_updates": false,
            "next_pubkey": {
                "key": "RWRotatedNewKey123",
                "signature": "untrusted comment: sig\nRWQ...signature..."
            }
        }"#;
        let feed: UpdateFeed = serde_json::from_str(json).expect("parse feed with next_pubkey");
        assert!(feed.next_pubkey.is_some());
        let next = feed.next_pubkey.unwrap();
        assert_eq!(next.key, "RWRotatedNewKey123");
        assert!(next.signature.contains("signature"));
    }

    #[test]
    fn next_pubkey_absent_in_feed_without_it() {
        let json = r#"{
            "schema_version": "1",
            "latest": "1.5.0",
            "min_supported": "1.0.0",
            "download_url": "https://example.com/enola-cli-1.5.0.tar.gz",
            "advisories": [],
            "pqc_milestones": [],
            "enforce_updates": false
        }"#;
        let feed: UpdateFeed = serde_json::from_str(json).expect("parse feed without next_pubkey");
        assert!(feed.next_pubkey.is_none());
    }

    #[test]
    fn verify_next_pubkey_rejects_empty_fields() {
        let empty_key = NextPubkey {
            key: String::new(),
            signature: String::new(),
        };
        assert!(!verify_next_pubkey_signature(
            &empty_key,
            "RWSomeCurrentKey"
        ));

        let empty_sig = NextPubkey {
            key: "RWNewKey".to_string(),
            signature: String::new(),
        };
        assert!(!verify_next_pubkey_signature(
            &empty_sig,
            "RWSomeCurrentKey"
        ));

        let empty_current = NextPubkey {
            key: "RWNewKey".to_string(),
            signature: "sig".to_string(),
        };
        assert!(!verify_next_pubkey_signature(&empty_current, ""));
    }
}
