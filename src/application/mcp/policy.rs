// MCP-007 — Política de escritura del servidor MCP.
//
// Regla (decisión D4): las tools `ReadOnly` siempre pasan; las `Mutating`
// y `Destructive` exigen `allow_write` y, si la allowlist no está vacía,
// figurar en ella. Allowlist vacía + `allow_write` → se permiten todas
// las mutantes (decisión explícita, documentada en la skill `mcp`).
// Las `Destructive` exigen ADEMÁS la confirmación de dos pasos (MCP-009),
// que se aplica en el servidor/handler, no en esta función.
//
// La política es pura y se evalúa ANTES del dispatch: si `allows` falla, el
// caso de uso no se invoca nunca.
//
// Resolución de configuración (precedencia alta → baja):
//   flag CLI (`mcp --allow-write --allow-tools …`) > env `ENOLA_MCP_*` >
//   `~/.enola/config.toml` `[mcp]` > defaults seguros (`allow_write=false`).

use crate::domain::error::{EnolaError, Result};
use crate::domain::tool::ToolKind;
use std::collections::HashMap;

/// Política efectiva del servidor MCP (ya resuelta).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct McpPolicy {
    /// Permite ejecutar tools `Mutating`.
    pub(crate) allow_write: bool,
    /// Allowlist de nombres de tool (`wp_create`) o rutas de comando
    /// (`family:sub` p.ej. `wp:create`; `family` pelado para hojas).
    /// Vacía = todas las mutantes.
    pub(crate) allowed_tools: Vec<String>,
    /// Límite de bytes para stdout/stderr devueltos por tools.
    pub(crate) max_output_bytes: usize,
    /// Path del log de auditoría (None = stderr).
    pub(crate) audit_log: Option<String>,
    /// Timeout por llamada a tool, en segundos.
    pub(crate) request_timeout_secs: u64,
}

impl Default for McpPolicy {
    fn default() -> Self {
        Self {
            allow_write: false,
            allowed_tools: Vec::new(),
            max_output_bytes: 64 * 1024,
            audit_log: None,
            request_timeout_secs: 60,
        }
    }
}

impl McpPolicy {
    /// Evalúa si `tool_name` (de clase `kind`) puede ejecutarse.
    ///
    /// Para `enola_run_command` el nombre evaluado es `family:sub` (p.ej.
    /// `wp:create`; `family` pelado para familias hoja como `doctor`), así
    /// una allowlist puede limitar la meta-tool por comando.
    pub(crate) fn allows(&self, kind: ToolKind, tool_name: &str) -> Result<()> {
        match kind {
            ToolKind::ReadOnly => Ok(()),
            // Destructive es un subtipo de Mutating: exige la misma política
            // de escritura y, además, la confirmación de dos pasos que aplica
            // el servidor (fuera del alcance de esta función).
            ToolKind::Mutating | ToolKind::Destructive => {
                if !self.allow_write {
                    return Err(EnolaError::SecurityError(format!(
                        "tool '{tool_name}' is mutating; start the server with --allow-write \
                         (or [mcp].allow_write = true) to enable it"
                    )));
                }
                if !self.allowed_tools.is_empty()
                    && !self.allowed_tools.iter().any(|t| t == tool_name)
                {
                    return Err(EnolaError::SecurityError(format!(
                        "tool '{tool_name}' is not in the MCP allowlist ([mcp].allowed_tools)"
                    )));
                }
                Ok(())
            }
        }
    }
}

/// Overrides de flags CLI (None = no aportado → baja al siguiente nivel).
#[derive(Debug, Default, Clone)]
pub(crate) struct McpFlagOverrides {
    /// `--allow-write` (flag booleano; `Some(true)` si se pasó).
    pub(crate) allow_write: Option<bool>,
    /// `--allow-tools` CSV ya troceado.
    pub(crate) allow_tools: Option<Vec<String>>,
    /// `--max-output-bytes`.
    pub(crate) max_output_bytes: Option<usize>,
    /// `--audit-log` path.
    pub(crate) audit_log: Option<String>,
    /// `--request-timeout-secs`.
    pub(crate) request_timeout_secs: Option<u64>,
}

fn env_truthy(key: &str) -> Option<bool> {
    std::env::var(key).ok().and_then(|v| truthy(&v))
}

fn env_u64(key: &str) -> Option<u64> {
    std::env::var(key).ok().and_then(|v| v.trim().parse().ok())
}

fn env_string(key: &str) -> Option<String> {
    std::env::var(key).ok().and_then(|v| {
        let v = v.trim().to_string();
        if v.is_empty() {
            None
        } else {
            Some(v)
        }
    })
}

fn env_csv(key: &str) -> Option<Vec<String>> {
    env_string(key).map(|v| {
        v.split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect()
    })
}

fn truthy(v: &str) -> Option<bool> {
    match v.trim().to_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn file_bool(section: &HashMap<String, String>, key: &str) -> Option<bool> {
    section.get(key).and_then(|v| truthy(v))
}

fn file_u64(section: &HashMap<String, String>, key: &str) -> Option<u64> {
    section.get(key).and_then(|v| v.trim().parse().ok())
}

fn file_string(section: &HashMap<String, String>, key: &str) -> Option<String> {
    section.get(key).map(String::from).and_then(|v| {
        let v = v.trim().to_string();
        if v.is_empty() {
            None
        } else {
            Some(v)
        }
    })
}

/// `allowed_tools` en el fichero: admite array TOML (`["a","b"]`) o CSV.
fn file_csv(section: &HashMap<String, String>, key: &str) -> Option<Vec<String>> {
    let raw = section.get(key)?.trim().to_string();
    if raw.is_empty() {
        return None;
    }
    if raw.starts_with('[') {
        if let Ok(arr) = serde_json::from_str::<Vec<String>>(&raw) {
            return Some(arr);
        }
        // TOML usa comillas simples a veces: normalizar a JSON.
        let normalized = raw.replace('\'', "\"");
        if let Ok(arr) = serde_json::from_str::<Vec<String>>(&normalized) {
            return Some(arr);
        }
    }
    Some(
        raw.split(',')
            .map(str::trim)
            .filter(|x| !x.is_empty())
            .map(String::from)
            .collect(),
    )
}

/// Resuelve la política efectiva: flag > env > `~/.enola/config.toml [mcp]` >
/// defaults seguros. `config_section` es la sección `[mcp]` del fichero
/// (`infrastructure::config_loader::load_section("mcp")`, vacío si no existe).
pub(crate) fn resolve_policy(
    flags: &McpFlagOverrides,
    config_section: &HashMap<String, String>,
) -> McpPolicy {
    let default = McpPolicy::default();

    McpPolicy {
        allow_write: flags
            .allow_write
            .or_else(|| env_truthy("ENOLA_MCP_ALLOW_WRITE"))
            .or_else(|| file_bool(config_section, "allow_write"))
            .unwrap_or(default.allow_write),
        allowed_tools: flags
            .allow_tools
            .clone()
            .or_else(|| env_csv("ENOLA_MCP_ALLOWED_TOOLS"))
            .or_else(|| file_csv(config_section, "allowed_tools"))
            .unwrap_or_default(),
        max_output_bytes: flags
            .max_output_bytes
            .or_else(|| env_u64("ENOLA_MCP_MAX_OUTPUT_BYTES").map(|v| v as usize))
            .or_else(|| file_u64(config_section, "max_output_bytes").map(|v| v as usize))
            .unwrap_or(default.max_output_bytes),
        audit_log: flags
            .audit_log
            .clone()
            .or_else(|| env_string("ENOLA_MCP_AUDIT_LOG"))
            .or_else(|| file_string(config_section, "audit_log"))
            .or(default.audit_log),
        request_timeout_secs: flags
            .request_timeout_secs
            .or_else(|| env_u64("ENOLA_MCP_REQUEST_TIMEOUT_SECS"))
            .or_else(|| file_u64(config_section, "request_timeout_secs"))
            .unwrap_or(default.request_timeout_secs),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn mutating() -> ToolKind {
        ToolKind::Mutating
    }

    #[test]
    fn readonly_always_allowed() {
        let p = McpPolicy::default();
        assert!(p.allows(ToolKind::ReadOnly, "tor_list").is_ok());
    }

    #[test]
    fn mutating_rejected_without_allow_write() {
        let p = McpPolicy::default();
        let e = p.allows(mutating(), "wp_create").unwrap_err();
        assert!(e.to_string().contains("allow-write"));
    }

    #[test]
    fn mutating_allowed_with_allow_write_and_empty_allowlist() {
        let p = McpPolicy {
            allow_write: true,
            ..Default::default()
        };
        assert!(p.allows(mutating(), "wp_create").is_ok());
        assert!(p.allows(mutating(), "tor_remove").is_ok());
    }

    #[test]
    fn mutating_rejected_when_not_in_allowlist() {
        let p = McpPolicy {
            allow_write: true,
            allowed_tools: vec!["wp_create".to_string()],
            ..Default::default()
        };
        assert!(p.allows(mutating(), "wp_create").is_ok());
        let e = p.allows(mutating(), "wp_delete").unwrap_err();
        assert!(e.to_string().contains("allowlist"));
    }

    #[test]
    fn command_paths_match_allowlist() {
        let p = McpPolicy {
            allow_write: true,
            allowed_tools: vec!["wp:create".to_string()],
            ..Default::default()
        };
        assert!(p.allows(mutating(), "wp:create").is_ok());
        assert!(p.allows(mutating(), "tor:remove").is_err());
    }

    #[test]
    fn destructive_follows_write_policy() {
        let p = McpPolicy::default();
        assert!(p.allows(ToolKind::Destructive, "tor_remove").is_err());
        let p = McpPolicy {
            allow_write: true,
            allowed_tools: vec!["wp_hide".to_string()],
            ..Default::default()
        };
        assert!(p.allows(ToolKind::Destructive, "wp_hide").is_ok());
        assert!(p.allows(ToolKind::Destructive, "tor_remove").is_err());
    }

    #[test]
    fn resolve_defaults_are_safe_without_any_source() {
        let _g = ENV_LOCK.lock().unwrap();
        for k in [
            "ENOLA_MCP_ALLOW_WRITE",
            "ENOLA_MCP_ALLOWED_TOOLS",
            "ENOLA_MCP_MAX_OUTPUT_BYTES",
            "ENOLA_MCP_AUDIT_LOG",
            "ENOLA_MCP_REQUEST_TIMEOUT_SECS",
        ] {
            std::env::remove_var(k);
        }
        let p = resolve_policy(&McpFlagOverrides::default(), &HashMap::new());
        assert!(!p.allow_write);
        assert!(p.allowed_tools.is_empty());
        assert_eq!(p.max_output_bytes, 64 * 1024);
        assert_eq!(p.request_timeout_secs, 60);
        assert_eq!(p.audit_log, None);
    }

    #[test]
    fn precedence_flag_beats_env_and_file() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::set_var("ENOLA_MCP_ALLOW_WRITE", "false");
        std::env::set_var("ENOLA_MCP_REQUEST_TIMEOUT_SECS", "45");
        let mut file = HashMap::new();
        file.insert("allow_write".to_string(), "false".to_string());
        file.insert("request_timeout_secs".to_string(), "90".to_string());
        file.insert("allowed_tools".to_string(), "[\"a\"]".to_string());
        let flags = McpFlagOverrides {
            allow_write: Some(true),
            ..Default::default()
        };
        let p = resolve_policy(&flags, &file);
        assert!(p.allow_write); // flag gana
        assert_eq!(p.request_timeout_secs, 45); // env gana a file
        assert_eq!(p.allowed_tools, vec!["a".to_string()]); // file gana a default
        std::env::remove_var("ENOLA_MCP_ALLOW_WRITE");
        std::env::remove_var("ENOLA_MCP_REQUEST_TIMEOUT_SECS");
    }
}
