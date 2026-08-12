// Infrastructure Layer
// System-level utilities and cross-cutting concerns

pub mod anti_debug; // LAUNCH-015 — prctl PR_SET_DUMPABLE + TracerPid detection
#[cfg(unix)]
pub mod atomic_secret_file; // SEC-EXT-RACE-010 — escritura atómica O_EXCL+0600+rename (anti-TOCTOU §13.x)
pub mod config_loader; // CONFIG-001 + CONFIG-002 — generic TOML section loader
pub mod container_limits; // SEC-EXT-DOCKER-040 — límites CPU/RAM/PIDs por defecto
pub mod drop_privs; // SEC-EXT-PRIV-020 — drop EUID 0 antes de exec en subprocesos auxiliares
pub mod embedded_scripts; // Dockerfiles embebidos en el binario (Strapi, etc.)
#[cfg(unix)]
pub mod file_lock; // SEC-EXT-RACE-011  generic flock-based exclusive file lock (RAII)
pub mod http; // CONFIG-009 — centralized reqwest builder with Tor SOCKS5 auto-detection
#[cfg(unix)]
pub mod port_lock; // SEC-EXT-RACE-011  per-port flock to close TOCTOU between is_port_free and docker run
pub mod pqc_tls;
pub mod privileges;
pub mod safe_args; // SEC-EXT-PRIV-021 — sanitización de inputs de usuario para Command::new + sh -c
pub mod security_opt; // SEC-007 — default Docker --security-opt hardening (WSL2-safe)
#[cfg(unix)]
pub mod shared_artifact_lock; // SEC-EXT-RACE-012  lock condicional para artefactos compartidos (/etc/nginx, /etc/systemd, /opt/enola)
/// Módulo de tokens de test seguros.
/// Solo disponible con feature flag `testing` — no existe en builds de producción.
#[cfg(feature = "testing")]
pub mod test_token;
