// MCP-007 — Log de auditoría de escrituras MCP.
//
// Cada `tools/call` mutante deja una línea JSON con timestamp, tool,
// argumentos SANEADOS y resultado. El log va a fichero (atomic append) o a
// stderr — NUNCA a stdout (el canal stdout es solo JSON-RPC, §3.1).

use serde_json::{json, Value};
use std::path::PathBuf;

/// Claves de argumentos cuyos valores se redactan en el log.
const SECRET_KEYS: &[&str] = &[
    "pass",
    "password",
    "passwd",
    "secret",
    "token",
    "key",
    "apikey",
    "api_key",
    "credential",
    "auth",
    "private",
    "cert",
    "pqc",
];

/// Sustituye por `[REDACTED]` el valor de cualquier clave cuyo nombre
/// contenga un patrón de secreto (case-insensitive). Recursivo en objetos.
pub(crate) fn sanitize_args(args: &Value) -> Value {
    match args {
        Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (k, v) in map {
                let lower = k.to_lowercase();
                if SECRET_KEYS.iter().any(|p| lower.contains(p)) {
                    out.insert(k.clone(), json!("[REDACTED]"));
                } else {
                    out.insert(k.clone(), sanitize_args(v));
                }
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(items.iter().map(sanitize_args).collect()),
        other => other.clone(),
    }
}

/// Destino del log de auditoría.
#[derive(Debug, Clone)]
pub(crate) enum AuditSink {
    /// `eprintln!` — nunca stdout.
    Stderr,
    /// Fichero append-only (una línea JSON por entrada).
    File(PathBuf),
}

/// Logger de auditoría para llamadas mutantes.
pub(crate) struct AuditLogger {
    sink: AuditSink,
}

impl AuditLogger {
    /// Construye el logger. `path` vacío/None/`"stderr"` → stderr.
    pub(crate) fn new(path: Option<&str>) -> Self {
        let sink = match path {
            Some(p) if !p.trim().is_empty() && p.trim() != "stderr" => {
                AuditSink::File(PathBuf::from(p))
            }
            _ => AuditSink::Stderr,
        };
        Self { sink }
    }

    /// Registra una llamada `tools/call`: tool, args saneados y resultado.
    ///
    /// `outcome` es `"ok"`, `"rejected"` o `"error: <msg>"`. Errores de
    /// escritura del propio log no rompen la llamada (best-effort, a stderr).
    pub(crate) fn log_call(&self, tool: &str, args: &Value, outcome: &str) {
        let entry = json!({
            "ts": chrono::Utc::now().to_rfc3339(),
            "tool": tool,
            "args": sanitize_args(args),
            "result": outcome,
        });
        let line = entry.to_string();
        if let Err(e) = self.write(&line) {
            eprintln!("mcp audit: failed to write log entry: {e}");
        }
    }

    fn write(&self, line: &str) -> std::io::Result<()> {
        match &self.sink {
            AuditSink::Stderr => {
                eprintln!("mcp audit: {line}");
                Ok(())
            }
            AuditSink::File(path) => {
                use std::io::Write;
                if let Some(parent) = path.parent() {
                    if !parent.as_os_str().is_empty() {
                        std::fs::create_dir_all(parent)?;
                    }
                }
                let mut f = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)?;
                writeln!(f, "{line}")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_secret_looking_keys() {
        let args = json!({
            "name": "blog",
            "db_pass": "s3cret!",
            "nested": {"api_key": "k", "plain": "ok"},
            "list": [{"token": "t"}],
        });
        let s = sanitize_args(&args);
        assert_eq!(s["name"], "blog");
        assert_eq!(s["db_pass"], "[REDACTED]");
        assert_eq!(s["nested"]["api_key"], "[REDACTED]");
        assert_eq!(s["nested"]["plain"], "ok");
        assert_eq!(s["list"][0]["token"], "[REDACTED]");
    }

    #[test]
    fn non_object_args_pass_through() {
        assert_eq!(sanitize_args(&json!("x")), json!("x"));
        assert_eq!(sanitize_args(&json!(null)), json!(null));
    }

    #[test]
    fn file_sink_appends_json_lines() {
        let dir = std::env::temp_dir().join(format!("mcp-audit-{}", std::process::id()));
        let path = dir.join("audit.log");
        let logger = AuditLogger::new(Some(path.to_str().unwrap()));
        logger.log_call("wp_create", &json!({"name": "a", "db_pass": "x"}), "ok");
        logger.log_call("wp_create", &json!({"name": "b"}), "rejected");
        let content = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2);
        let first: Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(first["tool"], "wp_create");
        assert_eq!(first["args"]["db_pass"], "[REDACTED]");
        assert_eq!(first["result"], "ok");
        assert!(first["ts"].as_str().unwrap().contains('T'));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn empty_path_falls_back_to_stderr() {
        let logger = AuditLogger::new(None);
        assert!(matches!(logger.sink, AuditSink::Stderr));
        let logger = AuditLogger::new(Some("   "));
        assert!(matches!(logger.sink, AuditSink::Stderr));
    }
}
