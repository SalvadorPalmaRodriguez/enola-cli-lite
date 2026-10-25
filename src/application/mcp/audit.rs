// MCP-007 — Log de auditoría de escrituras MCP.
//
// Cada `tools/call` mutante deja una línea JSON con timestamp, tool,
// argumentos SANEADOS y resultado. El log va a fichero (atomic append) o a
// stderr — NUNCA a stdout (el canal stdout es solo JSON-RPC, §3.1).

use serde_json::{json, Value};
use std::path::PathBuf;

/// Claves de argumentos cuyos valores se redactan en el log y en el argv
/// devuelto por `run_command`/`try_run` (F-12).
pub(crate) const SECRET_KEYS: &[&str] = &[
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
    /// `outcome` describe el estado: `"requested"`, `"rejected"`,
    /// `"simulated"`, `"confirmation_requested"`, `"authorized"`, `"ok"` o
    /// `"error: <msg>"`. Errores de escritura del propio log no rompen la
    /// llamada (best-effort, a stderr).
    pub(crate) fn log_call(&self, tool: &str, args: &Value, outcome: &str) {
        self.log_call_op(tool, args, outcome, None);
    }

    /// Igual que `log_call` pero correlaciona la entrada con `op_id`
    /// (típicamente el `confirm_token` de una operación destructiva), de modo
    /// que request → authorized → ok/error compartan identificador.
    ///
    /// `outcome` se trunca a 500 chars: los mensajes de error pueden arrastrar
    /// fragmentos de salida y el log debe mantenerse manejable.
    pub(crate) fn log_call_op(&self, tool: &str, args: &Value, outcome: &str, op_id: Option<&str>) {
        let outcome = if outcome.len() > 500 {
            crate::domain::tool::truncate_output(outcome, 500).0
        } else {
            outcome.to_string()
        };
        let mut entry = json!({
            "ts": chrono::Utc::now().to_rfc3339(),
            "tool": tool,
            "args": sanitize_args(args),
            "result": outcome,
        });
        if let Some(op) = op_id {
            // Solo el prefijo: el token completo no debe quedar en el log
            // (bastaría para confirmar la operación pendiente). 12 chars
            // de un token de 32 bastan para correlacionar estados.
            let prefix: String = op.chars().take(12).collect();
            entry["op"] = json!(format!("{prefix}…"));
        }
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
                // SAFETY: geteuid no tiene precondiciones.
                #[cfg(unix)]
                let euid = unsafe { libc::geteuid() };
                #[cfg(not(unix))]
                let euid = 1;
                validate_audit_path(path, euid)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::PermissionDenied, e))?;
                let name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .ok_or_else(|| std::io::Error::other("audit log path has no file name"))?;
                let dir = open_audit_dir(path, euid)?;
                // R-4: rotación simple por tamaño — un flood de rechazos no
                // debe hacer crecer el log sin límite (10 MiB → .1, se pierde
                // la rotación anterior). El directorio ya está validado (dueño
                // y modo), así que el rename por ruta no lo controla un tercero.
                const MAX_AUDIT_BYTES: u64 = 10 * 1024 * 1024;
                if let Ok(meta) = std::fs::symlink_metadata(path) {
                    if meta.is_file() && meta.len() >= MAX_AUDIT_BYTES {
                        let rotated = path.with_file_name(format!("{name}.1"));
                        let _ = std::fs::rename(path, &rotated);
                    }
                }
                // 0600 en creación + O_NOFOLLOW relativo al fd del directorio:
                // un symlink plantado no redirige la escritura (posiblemente root).
                let mut f = dir.open_append_nofollow(name, 0o600)?;
                // Si ya existía con perms laxos, endurecerlo sobre el fd (fchmod).
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = f.set_permissions(std::fs::Permissions::from_mode(0o600));
                }
                writeln!(f, "{line}")?;
                // fsync: la entrada de audit debe sobrevivir a un crash.
                f.sync_all()
            }
        }
    }
}

/// A25-04 (P2.5): directorios donde root puede escribir el log de auditoría
/// MCP. Una ruta elegida por terceros (`/usr/bin/x`, `/etc/sudoers.d/x`)
/// hacía que root cambiara permisos del padre y escribiera ahí.
pub(crate) const ROOT_AUDIT_DIRS: &[&str] = &["/var/log/enola", "/var/lib/enola"];

/// Valida la ruta del log: absoluta y sin `..`; bajo euid 0 además su
/// directorio padre debe ser uno de `ROOT_AUDIT_DIRS`.
pub(crate) fn validate_audit_path(path: &std::path::Path, euid: u32) -> Result<(), String> {
    use std::path::Component;
    if !path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
    {
        return Err(format!(
            "audit log '{}' must be an absolute path without '.'/'..'",
            path.display()
        ));
    }
    if path.file_name().is_none() {
        return Err(format!("audit log '{}' has no file name", path.display()));
    }
    if euid == 0 {
        let parent = path.parent().and_then(|p| p.to_str()).unwrap_or("");
        if !ROOT_AUDIT_DIRS.contains(&parent) {
            return Err(format!(
                "as root the audit log must live directly in {} (got '{}')",
                ROOT_AUDIT_DIRS.join(" or "),
                path.display()
            ));
        }
    }
    Ok(())
}

/// Abre (creándolo si falta) el directorio del log sin seguir symlinks y sin
/// `chmod` por ruta: bajo root se crea `0700` root-owned (rechaza uno ajeno o
/// escribible por grupo/otros); sin root se reducen sus permisos a `0700`
/// sobre el fd (nunca se añaden).
#[cfg(unix)]
fn open_audit_dir(
    path: &std::path::Path,
    euid: u32,
) -> std::io::Result<crate::infrastructure::safe_dir::SafeDir> {
    use crate::infrastructure::safe_dir::{open_user_dir, OwnerPolicy, SafeDir};
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("audit log path has no parent"))?;
    if euid == 0 {
        return SafeDir::open_or_create(parent, 0o700, OwnerPolicy::Euid);
    }
    if let Some(grand) = parent.parent() {
        std::fs::create_dir_all(grand)?;
    }
    // SAFETY: getegid no tiene precondiciones.
    let egid = unsafe { libc::getegid() };
    open_user_dir(parent, 0o700, (euid, egid), OwnerPolicy::Euid)
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

    /// A25-04 (P2.5): bajo root solo /var/log/enola o /var/lib/enola.
    #[test]
    fn root_audit_path_must_be_in_canonical_dir() {
        use std::path::Path;
        for bad in [
            "/usr/bin/x",
            "/etc/sudoers.d/x",
            "/var/log/enola/../../etc/x",
            "/var/log/enola/sub/x",
            "relative.log",
        ] {
            assert!(validate_audit_path(Path::new(bad), 0).is_err(), "{bad}");
        }
        assert!(validate_audit_path(Path::new("/var/log/enola/mcp-audit.log"), 0).is_ok());
        assert!(validate_audit_path(Path::new("/var/lib/enola/mcp-audit.log"), 0).is_ok());
        // Sin root: cualquier ruta absoluta propia.
        assert!(validate_audit_path(Path::new("/tmp/x/audit.log"), 1000).is_ok());
        assert!(validate_audit_path(Path::new("rel.log"), 1000).is_err());
    }

    #[test]
    fn audit_does_not_follow_planted_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let victim = dir.path().join("victim");
        std::fs::write(&victim, "keep").unwrap();
        let path = dir.path().join("logs").join("audit.log");
        std::fs::create_dir(dir.path().join("logs")).unwrap();
        std::os::unix::fs::symlink(&victim, &path).unwrap();
        let logger = AuditLogger::new(Some(path.to_str().unwrap()));
        logger.log_call("wp_create", &serde_json::json!({}), "ok");
        assert_eq!(std::fs::read_to_string(&victim).unwrap(), "keep");
    }

    #[test]
    fn empty_path_falls_back_to_stderr() {
        let logger = AuditLogger::new(None);
        assert!(matches!(logger.sink, AuditSink::Stderr));
        let logger = AuditLogger::new(Some("   "));
        assert!(matches!(logger.sink, AuditSink::Stderr));
    }

    /// R-4: el fichero de auditoría se crea con 0600 y el directorio con 0700.
    #[cfg(unix)]
    #[test]
    fn audit_file_created_with_restrictive_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("enola-audit-test-{}", std::process::id()));
        let path = dir.join("audit.log");
        let logger = AuditLogger::new(Some(path.to_str().unwrap()));
        logger.log_call("t", &json!({}), "ok");

        let file_mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(file_mode, 0o600, "audit log debe ser 0600");
        let dir_mode = std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(dir_mode, 0o700, "directorio de audit debe ser 0700");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
