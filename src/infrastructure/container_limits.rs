// ═══════════════════════════════════════════════════════════════════════════════
// SEC-EXT-DOCKER-040 — Default container resource limits
// ═══════════════════════════════════════════════════════════════════════════════
//
// Por defecto Docker NO impone límites de CPU/RAM/PIDs a los contenedores.
// Eso significa que un contenedor comprometido (CVE en WordPress, fork-bomb,
// runaway GC, etc.) puede agotar el host:
//   - OOM-killer al kernel → un servicio crítico puede caer junto con los demás.
//   - 100% CPU a 16 cores → toda la máquina queda inutilizable.
//   - Fork-bomb → PID exhaustion → no se puede ni `ssh` para mitigar.
//
// Este módulo centraliza los límites por defecto que SE APLICAN a todo
// `ContainerConfig` cuyo caller no haya establecido un valor explícito.
// El cliente que sabe que su contenedor necesita más recursos los
// sobreescribe en el `ContainerConfig` antes de llamar a
// `create_container` / `run_ephemeral_container`.
//
// El operador puede personalizar globalmente vía env vars:
//   - `ENOLA_DOCKER_DEFAULT_MEMORY_BYTES` (default: 2 GiB)
//   - `ENOLA_DOCKER_DEFAULT_NANO_CPUS`    (default: 2 CPU = 2_000_000_000)
//   - `ENOLA_DOCKER_DEFAULT_PIDS`         (default: 1024)
//
// Valor especial 0 (cero) = "ilimitado" — útil para hosts dedicados a una
// única instancia que quieran usar todos los recursos.

use crate::ports::container::ContainerConfig;

/// Límite de memoria por defecto: 2 GiB (en bytes).
///
/// Justificación: cubre WordPress + DB, Drupal, Ghost y Forgejo.
pub const DEFAULT_MEMORY_LIMIT_BYTES: i64 = 2 * 1024 * 1024 * 1024;

/// CPU por defecto: 2 CPU (2_000_000_000 nanoCPUs).
///
/// Cualquier servicio web razonable se sirve sobradamente con 2 CPU.
pub const DEFAULT_NANO_CPUS: i64 = 2_000_000_000;

/// PIDs por defecto: 1024.
///
/// Mata fork-bombs sin estorbar a workloads legítimos (Forgejo arranca
/// ~50 procesos y WordPress ~10).
pub const DEFAULT_PIDS_LIMIT: i64 = 1024;

/// Lee un override entero positivo de una variable de entorno.
/// Devuelve `None` si la var no está, está vacía o no se parsea.
/// Acepta `0` como valor explícito "ilimitado".
fn env_override(var: &str) -> Option<i64> {
    let raw = std::env::var(var).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    trimmed.parse::<i64>().ok().filter(|v| *v >= 0)
}

/// Resuelve el límite efectivo para `memory_limit`. Devuelve `Some(bytes)` o
/// `None` si el override es 0 (ilimitado explícito).
fn resolve_memory() -> Option<i64> {
    let v = env_override("ENOLA_DOCKER_DEFAULT_MEMORY_BYTES").unwrap_or(DEFAULT_MEMORY_LIMIT_BYTES);
    if v == 0 {
        None
    } else {
        Some(v)
    }
}

fn resolve_nano_cpus() -> Option<i64> {
    let v = env_override("ENOLA_DOCKER_DEFAULT_NANO_CPUS").unwrap_or(DEFAULT_NANO_CPUS);
    if v == 0 {
        None
    } else {
        Some(v)
    }
}

fn resolve_pids() -> Option<i64> {
    let v = env_override("ENOLA_DOCKER_DEFAULT_PIDS").unwrap_or(DEFAULT_PIDS_LIMIT);
    if v == 0 {
        None
    } else {
        Some(v)
    }
}

/// Aplica los límites por defecto a un `ContainerConfig` mutable. Solo
/// rellena campos que el caller dejó en `None`; valores explícitos del
/// caller se preservan intactos.
///
/// Idempotente: ejecutarlo dos veces da el mismo resultado.
///
/// **CONTRATO**: este helper se invoca UNA vez en
/// `DockerAdapter::create_container` / `run_ephemeral_container` antes de
/// construir `HostConfig`. Ningún call-site fuera del adapter Docker debe
/// invocarlo (sería redundante).
pub fn apply_default_limits(config: &mut ContainerConfig) {
    if config.memory_limit.is_none() {
        config.memory_limit = resolve_memory();
    }
    if config.nano_cpus.is_none() {
        config.nano_cpus = resolve_nano_cpus();
    }
    if config.pids_limit.is_none() {
        config.pids_limit = resolve_pids();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn clear_env() {
        std::env::remove_var("ENOLA_DOCKER_DEFAULT_MEMORY_BYTES");
        std::env::remove_var("ENOLA_DOCKER_DEFAULT_NANO_CPUS");
        std::env::remove_var("ENOLA_DOCKER_DEFAULT_PIDS");
    }

    #[test]
    fn applies_defaults_when_none() {
        let _g = ENV_LOCK.lock().unwrap();
        clear_env();

        let mut cfg = ContainerConfig {
            name: "x".into(),
            image: "y".into(),
            ..Default::default()
        };
        apply_default_limits(&mut cfg);
        assert_eq!(cfg.memory_limit, Some(DEFAULT_MEMORY_LIMIT_BYTES));
        assert_eq!(cfg.nano_cpus, Some(DEFAULT_NANO_CPUS));
        assert_eq!(cfg.pids_limit, Some(DEFAULT_PIDS_LIMIT));
    }

    #[test]
    fn preserves_explicit_caller_values() {
        let _g = ENV_LOCK.lock().unwrap();
        clear_env();

        let mut cfg = ContainerConfig {
            name: "x".into(),
            image: "y".into(),
            memory_limit: Some(8 * 1024 * 1024 * 1024), // AI: 8 GiB
            nano_cpus: Some(8_000_000_000),             // AI: 8 CPU
            pids_limit: Some(4096),
            ..Default::default()
        };
        apply_default_limits(&mut cfg);
        assert_eq!(cfg.memory_limit, Some(8 * 1024 * 1024 * 1024));
        assert_eq!(cfg.nano_cpus, Some(8_000_000_000));
        assert_eq!(cfg.pids_limit, Some(4096));
    }

    #[test]
    fn env_override_changes_defaults() {
        let _g = ENV_LOCK.lock().unwrap();
        clear_env();
        std::env::set_var("ENOLA_DOCKER_DEFAULT_MEMORY_BYTES", "536870912"); // 512 MiB
        std::env::set_var("ENOLA_DOCKER_DEFAULT_NANO_CPUS", "500000000"); // 0.5 CPU
        std::env::set_var("ENOLA_DOCKER_DEFAULT_PIDS", "256");

        let mut cfg = ContainerConfig {
            name: "x".into(),
            image: "y".into(),
            ..Default::default()
        };
        apply_default_limits(&mut cfg);
        assert_eq!(cfg.memory_limit, Some(536_870_912));
        assert_eq!(cfg.nano_cpus, Some(500_000_000));
        assert_eq!(cfg.pids_limit, Some(256));

        clear_env();
    }

    #[test]
    fn env_override_zero_means_unlimited() {
        let _g = ENV_LOCK.lock().unwrap();
        clear_env();
        std::env::set_var("ENOLA_DOCKER_DEFAULT_MEMORY_BYTES", "0");
        std::env::set_var("ENOLA_DOCKER_DEFAULT_NANO_CPUS", "0");
        std::env::set_var("ENOLA_DOCKER_DEFAULT_PIDS", "0");

        let mut cfg = ContainerConfig {
            name: "x".into(),
            image: "y".into(),
            ..Default::default()
        };
        apply_default_limits(&mut cfg);
        assert!(cfg.memory_limit.is_none(), "0 = unlimited");
        assert!(cfg.nano_cpus.is_none());
        assert!(cfg.pids_limit.is_none());

        clear_env();
    }

    #[test]
    fn env_override_corrupted_falls_back_to_default() {
        let _g = ENV_LOCK.lock().unwrap();
        clear_env();
        std::env::set_var("ENOLA_DOCKER_DEFAULT_MEMORY_BYTES", "not-a-number");
        std::env::set_var("ENOLA_DOCKER_DEFAULT_NANO_CPUS", "");
        std::env::set_var("ENOLA_DOCKER_DEFAULT_PIDS", "-1");

        let mut cfg = ContainerConfig {
            name: "x".into(),
            image: "y".into(),
            ..Default::default()
        };
        apply_default_limits(&mut cfg);
        assert_eq!(cfg.memory_limit, Some(DEFAULT_MEMORY_LIMIT_BYTES));
        assert_eq!(cfg.nano_cpus, Some(DEFAULT_NANO_CPUS));
        assert_eq!(cfg.pids_limit, Some(DEFAULT_PIDS_LIMIT));

        clear_env();
    }

    #[test]
    fn idempotent_double_apply() {
        let _g = ENV_LOCK.lock().unwrap();
        clear_env();

        let mut cfg = ContainerConfig {
            name: "x".into(),
            image: "y".into(),
            ..Default::default()
        };
        apply_default_limits(&mut cfg);
        let snapshot = (cfg.memory_limit, cfg.nano_cpus, cfg.pids_limit);
        apply_default_limits(&mut cfg);
        assert_eq!((cfg.memory_limit, cfg.nano_cpus, cfg.pids_limit), snapshot);
    }
}
