//! MCP-004 — Adapter que ejecuta el propio binario `enola-cli` como subproceso.
//!
//! Implementa `ProcessRunnerPort` con `tokio::process::Command` sobre
//! `std::env::current_exe()`: argv array (sin shell), `stdin` cerrado,
//! `kill_on_drop` para que un timeout mate al hijo.

use crate::domain::error::{EnolaError, Result};
use crate::infrastructure::child_env::child_env;
use crate::ports::process_runner::{ProcessOutput, ProcessRunnerPort};
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

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
