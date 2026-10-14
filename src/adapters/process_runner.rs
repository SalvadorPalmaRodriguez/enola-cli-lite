//! MCP-004 — Adapter que ejecuta el propio binario `enola-cli` como subproceso.
//!
//! Implementa `ProcessRunnerPort` con `tokio::process::Command` sobre
//! `std::env::current_exe()`: argv array (sin shell), `stdin` cerrado,
//! `kill_on_drop` para que un timeout mate al hijo.

use crate::domain::error::{EnolaError, Result};
use crate::ports::process_runner::{ProcessOutput, ProcessRunnerPort};
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

/// Variables permitidas en hijos MCP (R-3): el hijo arranca con
/// `env_clear()` y solo recibe estas — una denylist de `ENOLA_*` sería
/// porosa (cualquier var nueva reabriría el agujero; ver auditoría F-15:
/// `ENOLA_UPDATE_MINISIGN_PUBKEY` reemplaza el trust anchor de firmas).
///
/// `HOME`/`USER`/`SUDO_USER` se conservan: el hijo resuelve su config y
/// estado desde ellos (`effective_home()`); la confianza sobre `[mcp]` en
/// homes de usuario se trata en R-5, no aquí. `ENOLA_ACCEPT_LICENSE` se
/// conserva: es el gate legal, no de seguridad.
const CHILD_ENV_EXACT: &[&str] = &[
    "PATH",
    "HOME",
    "USER",
    "SUDO_USER",
    "LANG",
    "TERM",
    "ENOLA_ACCEPT_LICENSE",
];
const CHILD_ENV_PREFIXES: &[&str] = &["LC_"];

/// Entorno filtrado para el hijo: solo la allowlist + `ENOLA_MCP_CHILD=1`
/// (marca que desactiva preflight automático en el hijo — R-3).
fn child_env() -> Vec<(std::ffi::OsString, std::ffi::OsString)> {
    let mut env: Vec<(std::ffi::OsString, std::ffi::OsString)> = std::env::vars_os()
        .filter(|(k, _)| {
            k.to_str()
                .map(|k| {
                    CHILD_ENV_EXACT.contains(&k)
                        || CHILD_ENV_PREFIXES.iter().any(|p| k.starts_with(p))
                })
                .unwrap_or(false)
        })
        .collect();
    env.push((
        std::ffi::OsString::from("ENOLA_MCP_CHILD"),
        std::ffi::OsString::from("1"),
    ));
    env
}

/// Ejecuta `enola-cli` como subproceso con argv tipado (sin shell).
pub struct SubprocessRunner {
    program: PathBuf,
}

impl SubprocessRunner {
    /// Usa el ejecutable actual (`std::env::current_exe`).
    pub fn new() -> Result<Self> {
        let program = std::env::current_exe()?;
        Ok(Self { program })
    }
}

#[async_trait::async_trait]
impl ProcessRunnerPort for SubprocessRunner {
    async fn run(&self, argv: &[String], timeout: Duration) -> Result<ProcessOutput> {
        let child = tokio::process::Command::new(&self.program)
            .args(argv)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            // R-3: entorno mínimo — ningún ENOLA_* del padre se hereda
            // salvo ENOLA_ACCEPT_LICENSE; ENOLA_MCP_CHILD marca al hijo.
            .env_clear()
            .envs(child_env())
            .spawn()
            .map_err(|e| {
                EnolaError::InfrastructureError(format!(
                    "cannot spawn {}: {e}",
                    self.program.display()
                ))
            })?;
        match tokio::time::timeout(timeout, child.wait_with_output()).await {
            Ok(Ok(out)) => Ok(ProcessOutput {
                exit_code: out.status.code().unwrap_or(-1),
                stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
            }),
            Ok(Err(e)) => Err(e.into()),
            Err(_) => Err(EnolaError::Timeout(format!(
                "command timed out after {}s",
                timeout.as_secs()
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// R-3: el entorno del hijo es `env_clear()` + allowlist explícita —
    /// ningún `ENOLA_*` peligroso se hereda, `ENOLA_MCP_CHILD` se marca.
    #[test]
    fn child_env_strips_enola_secrets_and_marks_child() {
        // Simular un env envenenado para la comprobación.
        std::env::set_var("ENOLA_UPDATE_MINISIGN_PUBKEY", "evil");
        std::env::set_var("ENOLA_ALLOW_UNSIGNED_UPDATE", "1");
        std::env::set_var("ENOLA_WEB_TOKEN", "x");
        let env = child_env();
        let keys: Vec<&str> = env.iter().filter_map(|(k, _)| k.to_str()).collect();

        assert!(keys.contains(&"ENOLA_MCP_CHILD"));
        for poisoned in [
            "ENOLA_UPDATE_MINISIGN_PUBKEY",
            "ENOLA_ALLOW_UNSIGNED_UPDATE",
            "ENOLA_WEB_TOKEN",
            "ENOLA_SKIP_ROOT_CHECK",
            "ENOLA_TOR_SOCKS_PROXY",
            "ENOLA_MANIFEST_PATH",
            "ENOLA_TEST_TOKEN",
        ] {
            assert!(
                !keys.contains(&poisoned),
                "{poisoned} no debe llegar al hijo MCP"
            );
        }
        assert!(
            keys.contains(&"PATH"),
            "PATH debe preservarse para que el hijo funcione"
        );
        std::env::remove_var("ENOLA_UPDATE_MINISIGN_PUBKEY");
        std::env::remove_var("ENOLA_ALLOW_UNSIGNED_UPDATE");
        std::env::remove_var("ENOLA_WEB_TOKEN");
    }
}
