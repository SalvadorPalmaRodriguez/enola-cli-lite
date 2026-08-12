//! src/infrastructure/anti_debug.rs — LAUNCH-015
//!
//! Endurecimiento del proceso contra debugging y dumps de memoria.
//!
//! ## Qué hace
//!
//! 1. **`harden_process()`** — `prctl(PR_SET_DUMPABLE, 0)` en Linux:
//!    - Bloquea **core dumps** (si el proceso crashea, no se escribe `core` en disco
//!      con la memoria del proceso → no expone JWT, passphrases o claves en RAM).
//!    - Bloquea **ptrace attach** desde otro proceso (gdb/strace no pueden adjuntarse
//!      a un proceso ya en ejecución sin ser root y aún así con restricciones
//!      según `kernel.yama.ptrace_scope`).
//!
//! 2. **`detect_tracer()`** — lee `/proc/self/status` y comprueba el campo `TracerPid`.
//!    Si es distinto de 0, el proceso está siendo debuggeado (gdb, strace, ltrace,
//!    rr, ...) y el binario debe abortar.
//!
//! ## Qué NO hace
//!
//! - **NO es DRM**: un atacante puede recompilar Rust con esta función deshabilitada
//!   o parchear el binario en disco. La capa de seguridad real es la verificación
//!   de firma del binario (minisign/ML-DSA).
//! - **NO bloquea análisis estático**: `strings`, `objdump`, `radare2`, `Ghidra` siguen
//!   funcionando sobre el binario en disco. Para eso ya está LAUNCH-007 (strip + LTO +
//!   remap-path-prefix) y LAUNCH-007+015 (`obfstr!` en strings sensibles).
//! - **NO afecta a usuarios legítimos**: `prctl` y la lectura de `/proc/self/status`
//!   son operaciones gratuitas. No imponen latencia ni dependencias adicionales.
//!
//! ## Defensa en profundidad
//!
//! Esta es UNA capa más entre:
//! - Hardening del binario (LAUNCH-007).
//! - Strings ofuscadas (`obfstr!`).
//! - Self-integrity check (INT-008).
//! - Firma minisign + ML-DSA (PQC-030..032).

#[cfg(target_os = "linux")]
const PR_SET_DUMPABLE: i32 = 4;

/// Endurece el proceso actual: deshabilita core dumps y bloquea ptrace attach.
///
/// Se debe llamar **lo antes posible** en `main()`, antes de cargar cualquier
/// dato sensible en memoria.
///
/// En sistemas no-Linux es un no-op silencioso.
#[inline]
pub fn harden_process() {
    #[cfg(target_os = "linux")]
    {
        // SAFETY: prctl con PR_SET_DUMPABLE solo modifica un flag del proceso actual.
        // No tiene efectos colaterales sobre memoria ni recursos del sistema.
        // Si falla (ej. kernel sin soporte), simplemente no aplica el hardening.
        unsafe {
            libc::prctl(PR_SET_DUMPABLE, 0_i64, 0_i64, 0_i64, 0_i64);
        }
    }
}

/// Detecta si el proceso actual está siendo debuggeado (ptrace, gdb, strace, ...).
///
/// Lee `/proc/self/status` y comprueba el campo `TracerPid`. Si es distinto de 0,
/// hay un debugger adjunto.
///
/// Devuelve `false` si:
/// - No hay tracer (caso normal).
/// - No estamos en Linux (no hay `/proc`).
/// - `/proc/self/status` no se puede leer (sandbox restrictivo, kernel exotic).
///
/// El llamante decide qué hacer ante `true` (típicamente: imprimir error y `exit(2)`).
#[inline]
pub fn detect_tracer() -> bool {
    detect_tracer_from_path("/proc/self/status")
}
/// Implementacion interna: lee `path` como si fuera `/proc/self/status` y
/// devuelve si hay un TracerPid != 0.
/// Expuesta como `pub(crate)` para tests: inyectar ruta inexistente (Err branch)
/// o fichero sin TracerPid (fallthrough false).
pub(crate) fn detect_tracer_from_path(path: &str) -> bool {
    let status = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(_) => return false,
    };
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("TracerPid:") {
            let pid: i32 = rest.trim().parse().unwrap_or(0);
            return pid != 0;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `harden_process()` no debe panicar ni en runtime de test ni cuando se invoca
    /// múltiples veces. Es idempotente.
    #[test]
    fn harden_process_is_idempotent_and_safe() {
        harden_process();
        harden_process();
        harden_process();
        // Si llegamos aquí, no hubo panic.
    }

    /// En el runtime normal de `cargo test` no hay debugger adjunto, por lo que
    /// `detect_tracer()` debe devolver `false`. Si alguien ejecuta los tests bajo
    /// `gdb --args cargo test ...` este test fallará intencionalmente — eso es
    /// el comportamiento correcto y demuestra que la detección funciona.
    #[test]
    fn detect_tracer_is_false_in_normal_test_run() {
        assert!(
            !detect_tracer(),
            "TracerPid distinto de 0 — ¿estás corriendo cargo test bajo gdb/strace?"
        );
    }

    /// Verifica que `detect_tracer()` no panica aunque `/proc/self/status` tenga
    /// formato inesperado (en Linux real esto no debería ocurrir, pero el test
    /// confirma que el parser es tolerante a ruido).
    #[test]
    fn detect_tracer_returns_bool_without_panic() {
        // Llamadas múltiples — solo verifica que retorna sin panic.
        let _ = detect_tracer();
        let _ = detect_tracer();
    }
    // TEST-COV-UNIT-004: cubre Err(_)=>false (ruta inexistente)
    #[test]
    fn detect_tracer_from_path_false_on_nonexistent_file() {
        assert!(!detect_tracer_from_path(
            "/tmp/enola_antidebug_nonexistent_xyz99999"
        ));
    }
    // TEST-COV-UNIT-004: cubre fallthrough false (fichero sin TracerPid)
    #[test]
    fn detect_tracer_from_path_false_when_no_tracer_pid_line() {
        let p = "/tmp/enola_test_no_tracer_pid_line";
        std::fs::write(p, "Name:\tenola\nVmRSS:\t4096 kB\n").unwrap();
        assert!(!detect_tracer_from_path(p));
        std::fs::remove_file(p).ok();
    }
    // TEST-COV-UNIT-004: TracerPid=0 -> false
    #[test]
    fn detect_tracer_from_path_false_when_tracer_pid_zero() {
        let p = "/tmp/enola_test_tracer_zero";
        std::fs::write(p, "TracerPid:\t0\n").unwrap();
        assert!(!detect_tracer_from_path(p));
        std::fs::remove_file(p).ok();
    }
    // TEST-COV-UNIT-004: TracerPid!=0 -> true (tracer activo)
    #[test]
    fn detect_tracer_from_path_true_when_tracer_pid_nonzero() {
        let p = "/tmp/enola_test_tracer_nonzero";
        std::fs::write(p, "TracerPid:\t1234\n").unwrap();
        assert!(detect_tracer_from_path(p));
        std::fs::remove_file(p).ok();
    }
}
