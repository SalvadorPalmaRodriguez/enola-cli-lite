// Fase 13.2 — Dominio puro de providers IA externos (BYOK cloud).
//
// ⚠️ PRIVACIDAD: estos providers son el ÚNICO punto del CLI donde los
// datos del usuario salen de su máquina. Desactivados por defecto; el
// consentimiento explícito (prompt TTY o --accept-risks) se aplica en
// application/cli, no aquí. Los secretos NUNCA se serializan en logs,
// errores ni salidas — ver `redact_key` y `validate_api_key`.

use crate::domain::error::{EnolaError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Provider IA externo soportado (BYOK — Bring Your Own Key).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExternalProvider {
    /// OpenAI (api.openai.com) — formato OpenAI-compatible.
    OpenAi,
    /// Anthropic (api.anthropic.com) — formato Messages propio.
    Anthropic,
    /// xAI Grok (api.x.ai) — OpenAI-compatible.
    Grok,
    /// Groq (api.groq.com) — OpenAI-compatible.
    Groq,
    /// OpenRouter (openrouter.ai) — OpenAI-compatible, gateway multimodelo.
    OpenRouter,
}

impl ExternalProvider {
    /// Todos los providers conocidos (orden estable para `ai provider list`).
    pub fn all() -> &'static [ExternalProvider] {
        &[
            ExternalProvider::OpenAi,
            ExternalProvider::Anthropic,
            ExternalProvider::Grok,
            ExternalProvider::Groq,
            ExternalProvider::OpenRouter,
        ]
    }

    /// Parse case-insensitive del nombre del provider.
    pub fn from_str_ci(s: &str) -> Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "openai" | "open-ai" | "gpt" => Ok(ExternalProvider::OpenAi),
            "anthropic" | "claude" => Ok(ExternalProvider::Anthropic),
            "grok" | "xai" | "x.ai" => Ok(ExternalProvider::Grok),
            "groq" => Ok(ExternalProvider::Groq),
            "openrouter" | "open-router" | "or" => Ok(ExternalProvider::OpenRouter),
            other => Err(EnolaError::ValidationError(format!(
                "Provider '{other}' desconocido — válidos: {}",
                ExternalProvider::all()
                    .iter()
                    .map(|p| p.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))),
        }
    }

    /// Nombre canónico en minúsculas (clave del archivo/env).
    pub fn as_str(&self) -> &'static str {
        match self {
            ExternalProvider::OpenAi => "openai",
            ExternalProvider::Anthropic => "anthropic",
            ExternalProvider::Grok => "grok",
            ExternalProvider::Groq => "groq",
            ExternalProvider::OpenRouter => "openrouter",
        }
    }

    /// Endpoint base por defecto (API pública del provider).
    pub fn default_endpoint(&self) -> &'static str {
        match self {
            ExternalProvider::OpenAi => "https://api.openai.com/v1",
            ExternalProvider::Anthropic => "https://api.anthropic.com/v1",
            ExternalProvider::Grok => "https://api.x.ai/v1",
            ExternalProvider::Groq => "https://api.groq.com/openai/v1",
            ExternalProvider::OpenRouter => "https://openrouter.ai/api/v1",
        }
    }

    /// Modelo por defecto razonable para el provider.
    pub fn default_model(&self) -> &'static str {
        match self {
            ExternalProvider::OpenAi => "gpt-4o-mini",
            ExternalProvider::Anthropic => "claude-haiku-4-5",
            ExternalProvider::Grok => "grok-3-mini",
            ExternalProvider::Groq => "llama-3.3-70b-versatile",
            ExternalProvider::OpenRouter => "openai/gpt-4o-mini",
        }
    }

    /// `true` si el provider habla el formato OpenAI `/chat/completions`.
    /// Anthropic usa su API Messages propia (`/messages`).
    pub fn is_openai_compatible(&self) -> bool {
        !matches!(self, ExternalProvider::Anthropic)
    }
}

/// Entrada de configuración de UN provider en `providers.env`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderEntry {
    /// Si el provider está habilitado para uso.
    pub enabled: bool,
    /// Si tiene API key configurada (la key NUNCA aparece en salidas).
    #[serde(skip_serializing)]
    pub api_key: Option<String>,
    /// Modelo a usar (default del provider si `None`).
    pub model: Option<String>,
    /// Endpoint personalizado (default del provider si `None`).
    pub endpoint: Option<String>,
}

impl ProviderEntry {
    /// Modelo efectivo (config o default del provider).
    pub fn effective_model(&self, provider: ExternalProvider) -> String {
        self.model
            .clone()
            .unwrap_or_else(|| provider.default_model().to_string())
    }

    /// Endpoint efectivo (config o default del provider).
    pub fn effective_endpoint(&self, provider: ExternalProvider) -> String {
        self.endpoint
            .clone()
            .unwrap_or_else(|| provider.default_endpoint().to_string())
    }

    /// `true` si el provider puede usarse (habilitado + con clave).
    pub fn is_usable(&self) -> bool {
        self.enabled && self.api_key.is_some()
    }
}

/// Estado completo parseado de `~/.enola/providers.env`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProvidersFile {
    /// Entradas por provider.
    pub entries: HashMap<ExternalProvider, ProviderEntry>,
}

/// Máximo de caracteres de una API key (defensivo).
const MAX_API_KEY_LEN: usize = 512;

/// Prefijo de las variables del archivo de providers.
const ENV_PREFIX: &str = "ENOLA_PROVIDER_";

/// Hosts a los que se admite `http://` (SEC-M8, D10): la clave viajaría en
/// claro, así que solo se permite sin cifrar hacia la propia máquina
/// (p. ej. un proxy o gateway local).
const HTTP_LOOPBACK_HOSTS: &[&str] = &["127.0.0.1", "::1", "localhost"];

/// SEC-M8: valida el endpoint de un provider. `https://` con host; `http://`
/// solo hacia loopback. Se rechaza userinfo (`http://127.0.0.1@evil.com`
/// apunta a `evil.com`) y cualquier carácter de control o espacio.
pub fn validate_endpoint(ep: &str) -> Result<()> {
    let bad = |why: &str| EnolaError::ValidationError(format!("Endpoint no válido '{ep}': {why}"));
    if ep.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err(bad("contiene espacios o caracteres de control"));
    }
    let (rest, is_http) = if let Some(r) = ep.strip_prefix("https://") {
        (r, false)
    } else if let Some(r) = ep.strip_prefix("http://") {
        (r, true)
    } else {
        return Err(bad(
            "debe empezar por https:// (o http:// hacia esta máquina)",
        ));
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.contains('@') {
        return Err(bad("no admite usuario/contraseña en la URL"));
    }
    let host = if let Some(v6) = authority.strip_prefix('[') {
        v6.split(']').next().unwrap_or_default()
    } else {
        authority.split(':').next().unwrap_or_default()
    };
    if host.is_empty() {
        return Err(bad("falta el host"));
    }
    if is_http
        && !HTTP_LOOPBACK_HOSTS
            .iter()
            .any(|h| h.eq_ignore_ascii_case(host))
    {
        return Err(bad(
            "http:// solo se admite hacia esta máquina (127.0.0.1, ::1, localhost); usa https://",
        ));
    }
    Ok(())
}

/// Valida una API key: no vacía, sin `\n`/`\r`/`\0` (T9 — inyección en
/// archivo env), dentro del máximo defensivo.
pub fn validate_api_key(key: &str) -> Result<()> {
    if key.is_empty() {
        return Err(EnolaError::ValidationError("La API key está vacía".into()));
    }
    if key.len() > MAX_API_KEY_LEN {
        return Err(EnolaError::ValidationError(format!(
            "API key demasiado larga (máx. {MAX_API_KEY_LEN} caracteres)"
        )));
    }
    if key
        .chars()
        .any(|c| c == '\n' || c == '\r' || c == '\0' || c.is_control())
    {
        return Err(EnolaError::ValidationError(
            "La API key contiene caracteres de control no permitidos".into(),
        ));
    }
    Ok(())
}

/// Devuelve una representación segura de la key para salidas:
/// `None` → `(not set)`; otra → `(set, <len> chars)` — jamás el valor.
pub fn key_status(api_key: &Option<String>) -> &'static str {
    match api_key {
        Some(_) => "(set)",
        None => "(not set)",
    }
}

/// Parsea el contenido de `providers.env` (formato `KEY=value`, `#` comentario).
/// Las keys NUNCA salen de aquí hacia logs — solo se almacenan en memoria.
pub fn parse_providers_env(content: &str) -> Result<ProvidersFile> {
    let mut file = ProvidersFile::default();
    for (n, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line.split_once('=').ok_or_else(|| {
            EnolaError::ValidationError(format!(
                "providers.env línea {}: sin '=' — '{}'",
                n + 1,
                line.chars().take(40).collect::<String>()
            ))
        })?;
        let key = key.trim();
        let value = value.trim();
        if !key.starts_with(ENV_PREFIX) {
            continue; // variables ajenas al prefijo se ignoran
        }
        let rest = &key[ENV_PREFIX.len()..];
        // El campo va como sufijo: `_API_KEY`, `_MODEL`, `_ENDPOINT` o `_ENABLED`
        // (API_KEY contiene '_' → sufijo literal, no rsplit).
        let (provider_name, field) = if let Some(name) = rest.strip_suffix("_API_KEY") {
            (name, "API_KEY")
        } else if let Some((name, f)) = rest.rsplit_once('_') {
            (name, f)
        } else {
            return Err(EnolaError::ValidationError(format!(
                "providers.env: clave malformada '{key}'"
            )));
        };
        let provider =
            ExternalProvider::from_str_ci(&provider_name.replace('_', "-")).map_err(|_| {
                EnolaError::ValidationError(format!(
                    "providers.env: provider desconocido en '{key}'"
                ))
            })?;
        let entry = file
            .entries
            .entry(provider)
            .or_insert_with(|| ProviderEntry {
                enabled: false,
                api_key: None,
                model: None,
                endpoint: None,
            });
        match field {
            "API_KEY" => entry.api_key = Some(value.to_string()),
            "MODEL" => entry.model = Some(value.to_string()),
            "ENDPOINT" => entry.endpoint = Some(value.to_string()),
            "ENABLED" => entry.enabled = value.eq_ignore_ascii_case("true") || value == "1",
            _ => { /* campo desconocido → ignorar */ }
        }
    }
    Ok(file)
}

/// Serializa el estado a `providers.env`. La key se escribe en claro en el
/// fichero (propósito del fichero: secreto local 0600), jamás en salidas.
pub fn serialize_providers_env(file: &ProvidersFile) -> String {
    let mut out = String::from(
        "# enola-cli — API keys de providers IA externos (BYOK). Permisos 0600.\n\
         # Generado por `ai provider set|remove`. No compartir.\n",
    );
    for p in ExternalProvider::all() {
        if let Some(e) = file.entries.get(p) {
            let name = p.as_str().to_uppercase().replace('-', "_");
            if let Some(k) = &e.api_key {
                out.push_str(&format!("ENOLA_PROVIDER_{name}_API_KEY={k}\n"));
            }
            if let Some(m) = &e.model {
                out.push_str(&format!("ENOLA_PROVIDER_{name}_MODEL={m}\n"));
            }
            if let Some(ep) = &e.endpoint {
                out.push_str(&format!("ENOLA_PROVIDER_{name}_ENDPOINT={ep}\n"));
            }
            if e.enabled {
                out.push_str(&format!("ENOLA_PROVIDER_{name}_ENABLED=true\n"));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_parses_and_has_defaults() {
        assert_eq!(
            ExternalProvider::from_str_ci("OpenAI").unwrap(),
            ExternalProvider::OpenAi
        );
        assert_eq!(
            ExternalProvider::from_str_ci("claude").unwrap(),
            ExternalProvider::Anthropic
        );
        assert!(ExternalProvider::from_str_ci("x.ai").is_ok());
        assert!(ExternalProvider::from_str_ci("nope").is_err());
        for p in ExternalProvider::all() {
            assert!(p.default_endpoint().starts_with("https://"));
            assert!(!p.default_model().is_empty());
        }
        assert!(ExternalProvider::OpenAi.is_openai_compatible());
        assert!(!ExternalProvider::Anthropic.is_openai_compatible());
    }

    #[test]
    fn api_key_validation_rejects_control_chars() {
        assert!(validate_api_key("sk-test_123").is_ok());
        assert!(validate_api_key("").is_err());
        assert!(validate_api_key("sk-with\nnewline").is_err());
        assert!(validate_api_key("sk-with\rCR").is_err());
        assert!(validate_api_key("sk-with\0null").is_err());
        assert!(validate_api_key(&"x".repeat(600)).is_err());
    }

    #[test]
    fn env_parse_and_serialize_roundtrip() {
        let content = "ENOLA_PROVIDER_OPENAI_API_KEY=test_k1\n\
                       ENOLA_PROVIDER_OPENAI_MODEL=gpt-4o\n\
                       ENOLA_PROVIDER_OPENAI_ENABLED=true\n\
                       ENOLA_PROVIDER_GROQ_API_KEY=test_k2\n\
                       # comentario\n\
                       OTHER_VAR=ignorada\n";
        let f = parse_providers_env(content).unwrap();
        let oai = &f.entries[&ExternalProvider::OpenAi];
        assert!(oai.enabled && oai.api_key.as_deref() == Some("test_k1"));
        assert_eq!(oai.model.as_deref(), Some("gpt-4o"));
        let groq = &f.entries[&ExternalProvider::Groq];
        assert!(!groq.enabled && !groq.is_usable());
        assert!(oai.is_usable());
        // roundtrip
        let back = parse_providers_env(&serialize_providers_env(&f)).unwrap();
        assert_eq!(f, back);
    }

    #[test]
    fn key_status_never_reveals_value() {
        assert_eq!(key_status(&Some("sk-secret".into())), "(set)");
        assert_eq!(key_status(&None), "(not set)");
    }

    // ── SEC-M8: http:// solo hacia loopback ──

    #[test]
    fn endpoint_http_remote_rejected() {
        for ep in [
            "http://api.openai.com/v1",
            "http://10.0.0.5:8080",
            "http://127.0.0.1@evil.com/v1",
            "http://127.0.0.1.evil.com",
            "http://[2001:db8::1]:80",
            "ftp://127.0.0.1",
            "api.openai.com",
            "https://",
            "https://user:pw@api.openai.com",
            "https://api.openai.com/v1 x",
        ] {
            assert!(validate_endpoint(ep).is_err(), "{ep}");
        }
    }

    #[test]
    fn endpoint_http_loopback_ok() {
        for ep in [
            "http://127.0.0.1:4000/v1",
            "http://localhost:8080",
            "http://LOCALHOST/v1",
            "http://[::1]:11434/v1",
            "https://api.openai.com/v1",
            "https://openrouter.ai/api/v1?x=1",
        ] {
            assert!(validate_endpoint(ep).is_ok(), "{ep}");
        }
    }

    #[test]
    fn default_endpoints_are_valid() {
        for p in ExternalProvider::all() {
            assert!(validate_endpoint(p.default_endpoint()).is_ok(), "{p:?}");
        }
    }
}
