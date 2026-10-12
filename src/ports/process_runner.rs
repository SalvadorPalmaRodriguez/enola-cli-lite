//! MCP-004 — Port para ejecutar el propio binario `enola-cli` como subproceso.
//!
//! `enola_run_command` despacha comandos del árbol generado sin construir
//! cadenas de shell: el adapter recibe un `argv` tipado y lo pasa a
//! `Command::new().args()` directamente.

use crate::domain::error::Result;
use std::time::Duration;

/// Salida capturada de un subproceso `enola-cli`.
#[derive(Debug, Clone, PartialEq)]
pub struct ProcessOutput {
    /// Código de salida del proceso (-1 si fue terminado por señal).
    pub exit_code: i32,
    /// stdout capturado (con `--format json` debe ser JSON o vacío).
    pub stdout: String,
    /// stderr capturado (diagnósticos, licencias, warnings).
    pub stderr: String,
}

/// Ejecuta `enola-cli` con un argv ya validado. Nunca invoca una shell.
#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait ProcessRunnerPort {
    /// Lanza el proceso con `argv`, espera hasta `timeout` y captura salida.
    ///
    /// En timeout el proceso hijo se mata (`kill_on_drop`) antes de devolver
    /// `EnolaError::Timeout`.
    async fn run(&self, argv: &[String], timeout: Duration) -> Result<ProcessOutput>;
}
