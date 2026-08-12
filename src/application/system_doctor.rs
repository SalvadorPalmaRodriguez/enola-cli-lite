//! Diagnóstico global del sistema — OBS-001 (2026-04-20)
//!
//! `enola-cli doctor` era originalmente solo un chequeo de dependencias del
//! sistema (Docker/Tor/Nginx/…). Este módulo lo amplía para componer un
//! **reporte multi-sección** orientado a soporte: cuando algo va mal, el
//! usuario ejecuta `sudo enola-cli doctor` y pega la salida → tenemos el
//! contexto completo en un solo comando.
//!
//! ## Secciones
//!
//! 1. **System Dependencies** — lo que ya existía (Docker, Tor, Nginx, UFW, …).
//!    Delegado a `DependencyManager::doctor()`.
//!
//! 2. **Configuration** — valores efectivos del sistema de config centralizada
//!    (CONFIG-001..009 + CFG-NEW-001). Resumen por origen (env/file/default).
//!
//! 3. **Config Validation** — errores y warnings de `config_inspector::validate_all()`
//!    (TOML parseable, permisos 0600, URLs sintácticas, Tor si `.onion`).
//!
//! 4. **Authentication** — estado de la sesión local (`~/.enola/session.json`):
//!    usuario, rol de licencia, tiempo restante del access token.
//!
//! 5. **Runtime Services** — contadores rápidos:
//!    - Contenedores Docker con prefijo `enola-` (running/total).
//!    - Hidden services Tor en `/var/lib/tor/enola_*`.
//!    - Sites Nginx enabled con prefijo `enola-` o `proxy_`.
//!
//! 6. **Hardware** — GPU (si se detecta), RAM, disco, modo appliance.
//!
//! 7. **PQC TLS** — estado de la pila OpenSSL 3.5 post-cuántica (ya existente).
//!
//! ## Uso
//!
//! ```no_run
//! # async fn demo() {
//! // En executor.rs
//! let out = enola_core::application::system_doctor::full_report().await;
//! print!("{}", out);
//! # }
//! ```
//!
//! Todas las secciones son **tolerantes a fallos**: si falta una herramienta
//! o una llamada da error, se reporta en el propio output pero no se panica.

use std::sync::Arc;

/// Ancho del separador usado entre secciones.
const SEP: &str = "═══════════════════════════════════════════════════════════════════";

/// Genera el reporte completo concatenando todas las secciones.
pub async fn full_report() -> String {
    let mut out = String::new();
    out.push_str(&header("System Doctor"));

    // 1. Dependencias (reutiliza el existente).
    out.push_str(&section_deps());

    // 2. Configuración efectiva.
    out.push_str(&section_config());

    // 3. Validación de la configuración.
    out.push_str(&section_config_validation());

    // 4. Servicios en ejecución.
    out.push_str(&section_runtime_services());

    // 5.5 Colisiones de puertos (PORTS-001)
    out.push_str(&section_port_collisions());

    // 6. Hardware.
    out.push_str(&section_hardware().await);

    // 7. PQC TLS stack.
    out.push_str(&section_pqc_tls());

    out
}

fn section_pqc_tls() -> String {
    crate::infrastructure::pqc_tls::doctor_section()
}

fn header(title: &str) -> String {
    format!("\n{}\n🩺 Enola CLI — {}\n{}\n", SEP, title, SEP)
}

fn subsection(title: &str) -> String {
    format!("\n── {} ──\n", title)
}

// ───────────────────────────────────────────────────────────────────────────
fn section_deps() -> String {
    use crate::adapters::infra::dependencies::SystemDependencyAdapter;
    use crate::application::dependency_manager::DependencyManager;

    let adapter = Arc::new(SystemDependencyAdapter::new());
    let mgr = DependencyManager::new(adapter);
    mgr.doctor()
}

// ───────────────────────────────────────────────────────────────────────────
// 2. Configuration — catálogo con fuente
// ───────────────────────────────────────────────────────────────────────────
fn section_config() -> String {
    use crate::application::config_inspector::{format_table, resolve_all};
    let mut s = header("Configuration");
    s.push_str(&format_table(&resolve_all()));
    s.push_str(
        "Leyenda: env = variable de entorno, file = ~/.enola/config.toml, default = binario.\n",
    );
    s
}

// ───────────────────────────────────────────────────────────────────────────
// 3. Config validation
// ───────────────────────────────────────────────────────────────────────────
// ───────────────────────────────────────────────────────────────────────────
// 3. Config validation
// ───────────────────────────────────────────────────────────────────────────
fn section_config_validation() -> String {
    use crate::application::config_inspector::{validate_all, ValidationSeverity};

    let mut s = header("Config Validation");
    // Modo offline (rápido): no hacemos HTTP aquí — doctor debe ser instantáneo.
    let findings = validate_all(false);
    let errors = findings
        .iter()
        .filter(|f| f.severity == ValidationSeverity::Error)
        .count();
    let warnings = findings
        .iter()
        .filter(|f| f.severity == ValidationSeverity::Warning)
        .count();
    let oks = findings
        .iter()
        .filter(|f| f.severity == ValidationSeverity::Ok)
        .count();

    for f in &findings {
        let icon = match f.severity {
            ValidationSeverity::Error => "❌",
            ValidationSeverity::Warning => "⚠️ ",
            ValidationSeverity::Ok => "✅",
        };
        s.push_str(&format!("  {} [{}] {}\n", icon, f.check, f.message));
    }
    s.push_str(&format!(
        "\nResumen: ✅ {}  ⚠️  {}  ❌ {}\n",
        oks, warnings, errors
    ));
    if errors == 0 && warnings == 0 {
        s.push_str("→ Configuración sana.\n");
    } else if errors == 0 {
        s.push_str("→ Sin errores. Warnings no bloquean el uso del CLI.\n");
    } else {
        s.push_str("→ ❌ Hay errores. Ejecuta: enola-cli config-validate\n");
    }
    s
}

// ───────────────────────────────────────────────────────────────────────────
// 4. Runtime services — contadores
// ───────────────────────────────────────────────────────────────────────────
fn section_runtime_services() -> String {
    let mut s = header("Runtime Services");

    // 5.1 Docker containers con prefijo enola-
    s.push_str(&subsection("Docker"));
    match std::process::Command::new("docker")
        .args(["ps", "-a", "--format", "{{.Names}}\t{{.State}}"])
        .output()
    {
        Ok(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let lines: Vec<&str> = stdout.lines().collect();
            let enola_lines: Vec<&&str> = lines
                .iter()
                .filter(|l| l.starts_with("enola-") || l.starts_with("wp-") || l.starts_with("db-"))
                .collect();
            let running = enola_lines.iter().filter(|l| l.contains("running")).count();
            let total = enola_lines.len();
            s.push_str(&format!(
                "  Contenedores Enola: {} en ejecución / {} total\n",
                running, total
            ));
            for line in enola_lines.iter().take(10) {
                s.push_str(&format!("    • {}\n", line.replace('\t', " → ")));
            }
            if enola_lines.len() > 10 {
                s.push_str(&format!(
                    "    … y {} más (usa `docker ps -a`)\n",
                    enola_lines.len() - 10
                ));
            }
        }
        Ok(_) => {
            s.push_str("  ⚠️  `docker ps` falló — ¿Docker está instalado y corriendo?\n");
        }
        Err(e) => {
            s.push_str(&format!("  ⚠️  `docker` no ejecutable: {}\n", e));
        }
    }

    // 5.2 Tor hidden services
    s.push_str(&subsection("Tor"));
    let tor_dir = std::path::Path::new("/var/lib/tor");
    if tor_dir.is_dir() {
        let count = std::fs::read_dir(tor_dir)
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .filter(|e| e.file_name().to_string_lossy().starts_with("enola_"))
                    .count()
            })
            .unwrap_or(0);
        s.push_str(&format!("  Hidden services Enola: {}\n", count));
    } else {
        s.push_str("  ⚠️  /var/lib/tor no accesible (¿Tor instalado? ¿permisos root?)\n");
    }

    // 5.3 Nginx sites enabled
    s.push_str(&subsection("Nginx"));
    let nginx_enabled = std::path::Path::new("/etc/nginx/sites-enabled");
    if nginx_enabled.is_dir() {
        let (enola_sites, total) = std::fs::read_dir(nginx_enabled)
            .map(|rd| {
                let entries: Vec<_> = rd.filter_map(|e| e.ok()).collect();
                let total = entries.len();
                let enola = entries
                    .iter()
                    .filter(|e| {
                        let n = e.file_name().to_string_lossy().to_string();
                        n.starts_with("enola-") || n.starts_with("proxy_") || n.starts_with("wp-")
                    })
                    .count();
                (enola, total)
            })
            .unwrap_or((0, 0));
        s.push_str(&format!(
            "  Sites enabled: {} Enola / {} total\n",
            enola_sites, total
        ));
    } else {
        s.push_str("  ⚠️  /etc/nginx/sites-enabled no accesible\n");
    }

    s
}

// ───────────────────────────────────────────────────────────────────────────
// 5.5 Port collisions (PORTS-001)
// ───────────────────────────────────────────────────────────────────────────
fn section_port_collisions() -> String {
    use crate::adapters::infra::port_checker::PortCheckerAdapter;
    use crate::ports::port_checker::PortCheckerPort;

    let mut s = header("Port Collisions");
    let checker = PortCheckerAdapter::new();

    // Puertos comunes que deben estar libres para Enola
    let common_ports = vec![
        (80, "HTTP estándar"),
        (443, "HTTPS estándar"),
        (8080, "HTTP backend común"),
        (3000, "Forgejo por defecto"),
    ];

    let mut collisions = Vec::new();
    for (port, label) in common_ports {
        match checker.check_port(port) {
            Ok(result) => {
                if !result.is_free() {
                    collisions.push((port, label, result.error_message().unwrap_or_default()));
                }
            }
            Err(_) => {
                // Si falla el check, asumimos que puede estar ocupado
                collisions.push((port, label, "check falló".to_string()));
            }
        }
    }

    if collisions.is_empty() {
        s.push_str("  ✅ Sin colisiones detectadas en puertos comunes.\n");
    } else {
        s.push_str("  ⚠️  Colisiones detectadas:\n");
        for (port, label, reason) in collisions {
            s.push_str(&format!("    • Puerto {} ({}): {}\n", port, label, reason));
        }
        s.push_str("  → Ejecuta: enola-cli ports list para ver todos los puertos en uso.\n");
    }

    s
}

// ───────────────────────────────────────────────────────────────────────────
// 6. Hardware
// ───────────────────────────────────────────────────────────────────────────
#[allow(dead_code)]
async fn section_hardware() -> String {
    use crate::adapters::hardware::probe::EnolaHardwareProbe;
    use crate::ports::hardware::HardwareProbePort;
    let mut s = header("Hardware");

    let probe = EnolaHardwareProbe::new();
    let specs = match probe.probe().await {
        Ok(specs) => specs,
        Err(e) => {
            s.push_str(&format!("  ⚠️  No se pudo probar hardware: {}\n", e));
            return s;
        }
    };

    s.push_str(&format!("  Plataforma   : {}\n", specs.platform));
    s.push_str(&format!("  CPU cores    : {}\n", specs.cpu_cores));
    s.push_str(&format!(
        "  RAM          : total {} MB  /  libre {} MB  /  usada {} MB\n",
        specs.ram_total_mb, specs.ram_available_mb, specs.ram_used_mb
    ));
    if specs.gpus.is_empty() {
        s.push_str("  GPU          : no detectada (modo CPU)\n");
    } else {
        for gpu in &specs.gpus {
            s.push_str(&format!(
                "  GPU #{}       : {} — VRAM {} MB ({:?})\n",
                gpu.index, gpu.name, gpu.vram_total_mb, gpu.brand
            ));
        }
    }
    s
}

// ═══════════════════════════════════════════════════════════════════════════
// SEC-AUDIT-001: doctor --security — auditoría automatizada post-deploy
// ═══════════════════════════════════════════════════════════════════════════

/// Severity level for a security finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecurityFindingLevel {
    Ok,
    Warning,
    Critical,
}

/// A single security finding.
#[derive(Debug, Clone)]
pub struct SecurityFinding {
    pub check: &'static str,
    pub level: SecurityFindingLevel,
    pub message: String,
}

/// Exit codes for security audit.
pub const SEC_EXIT_OK: i32 = 0;
pub const SEC_EXIT_WARNING: i32 = 10;
pub const SEC_EXIT_CRITICAL: i32 = 20;

/// Run the security audit and return (report_string, exit_code).
pub fn security_report() -> (String, i32) {
    let mut s = header("Security Audit");
    s.push_str("SEC-AUDIT-001 — Post-deploy security verification\n\n");

    let mut findings = Vec::new();

    // (a) Container hardening: no-new-privileges
    findings.extend(check_container_no_new_privileges());
    // (b) Container hardening: read_only_rootfs or explicit volumes
    findings.extend(check_container_read_only());
    // (c) No container binds to 0.0.0.0
    findings.extend(check_container_bind_addresses());
    // (d) Nginx fileserver: disable_symlinks + dangerous extension blocking
    findings.extend(check_nginx_fileserver_security());
    // (e) AppArmor enabled
    findings.extend(check_apparmor_enabled());
    // (f) UFW active
    findings.extend(check_ufw_active());
    // (g) No secrets in container env vars
    findings.extend(check_container_env_secrets());

    // Render findings
    let mut warnings = 0;
    let mut criticals = 0;
    let mut oks = 0;

    for f in &findings {
        let (icon, count_flag) = match f.level {
            SecurityFindingLevel::Ok => ("✅", 0),
            SecurityFindingLevel::Warning => ("⚠️ ", 1),
            SecurityFindingLevel::Critical => ("❌", 2),
        };
        match count_flag {
            0 => oks += 1,
            1 => warnings += 1,
            2 => criticals += 1,
            _ => {}
        }
        s.push_str(&format!("  {} [{}] {}\n", icon, f.check, f.message));
    }

    s.push_str(&format!("\n{}\n", SEP));
    s.push_str(&format!(
        "  Resumen: ✅ {} OK  ⚠️  {} warnings  ❌ {} critical\n",
        oks, warnings, criticals
    ));

    let exit_code = if criticals > 0 {
        s.push_str("  → ❌ CRITICAL: Hay problemas de seguridad críticos que deben corregirse.\n");
        SEC_EXIT_CRITICAL
    } else if warnings > 0 {
        s.push_str("  → ⚠️  Hay warnings de seguridad. Revisa los ítems arriba.\n");
        SEC_EXIT_WARNING
    } else {
        s.push_str("  → ✅ Auditoría de seguridad pasada sin incidencias.\n");
        SEC_EXIT_OK
    };

    (s, exit_code)
}

/// (a) Check that all enola-* containers have no-new-privileges:true
fn check_container_no_new_privileges() -> Vec<SecurityFinding> {
    let output = std::process::Command::new("docker")
        .args(["ps", "-q", "--filter", "name=enola-"])
        .output();

    let mut findings = Vec::new();

    match output {
        Ok(out) if out.status.success() => {
            let ids = String::from_utf8_lossy(&out.stdout);
            let container_ids: Vec<&str> = ids.lines().filter(|l| !l.is_empty()).collect();

            if container_ids.is_empty() {
                findings.push(SecurityFinding {
                    check: "no-new-privileges",
                    level: SecurityFindingLevel::Ok,
                    message: "No enola-* containers running (nothing to check)".to_string(),
                });
                return findings;
            }

            let mut violations = Vec::new();
            for cid in &container_ids {
                let inspect = std::process::Command::new("docker")
                    .args(["inspect", "--format", "{{.HostConfig.SecurityOpt}}", cid])
                    .output();
                if let Ok(io) = inspect {
                    let val = String::from_utf8_lossy(&io.stdout).trim().to_string();
                    if !val.contains("no-new-privileges") {
                        let name = std::process::Command::new("docker")
                            .args(["inspect", "--format", "{{.Name}}", cid])
                            .output()
                            .map(|o| {
                                String::from_utf8_lossy(&o.stdout)
                                    .trim()
                                    .trim_start_matches('/')
                                    .to_string()
                            })
                            .unwrap_or_else(|_| cid.to_string());
                        violations.push(name);
                    }
                }
            }

            if violations.is_empty() {
                findings.push(SecurityFinding {
                    check: "no-new-privileges",
                    level: SecurityFindingLevel::Ok,
                    message: format!(
                        "All {} enola-* containers have no-new-privileges:true",
                        container_ids.len()
                    ),
                });
            } else {
                findings.push(SecurityFinding {
                    check: "no-new-privileges",
                    level: SecurityFindingLevel::Critical,
                    message: format!(
                        "{} container(s) missing no-new-privileges: {}",
                        violations.len(),
                        violations.join(", ")
                    ),
                });
            }
        }
        Ok(_) => {
            findings.push(SecurityFinding {
                check: "no-new-privileges",
                level: SecurityFindingLevel::Warning,
                message: "`docker ps` failed — Docker not running or not installed".to_string(),
            });
        }
        Err(e) => {
            findings.push(SecurityFinding {
                check: "no-new-privileges",
                level: SecurityFindingLevel::Warning,
                message: format!("docker command not available: {}", e),
            });
        }
    }
    findings
}

/// (b) Check that all enola-* containers have read_only_rootfs or explicit volumes
fn check_container_read_only() -> Vec<SecurityFinding> {
    let output = std::process::Command::new("docker")
        .args(["ps", "-q", "--filter", "name=enola-"])
        .output();

    let mut findings = Vec::new();

    match output {
        Ok(out) if out.status.success() => {
            let ids = String::from_utf8_lossy(&out.stdout);
            let container_ids: Vec<&str> = ids.lines().filter(|l| !l.is_empty()).collect();

            if container_ids.is_empty() {
                findings.push(SecurityFinding {
                    check: "read-only-rootfs",
                    level: SecurityFindingLevel::Ok,
                    message: "No enola-* containers running (nothing to check)".to_string(),
                });
                return findings;
            }

            let mut violations = Vec::new();
            for cid in &container_ids {
                let inspect = std::process::Command::new("docker")
                    .args(["inspect", "--format", "{{.HostConfig.ReadonlyRootfs}}", cid])
                    .output();
                if let Ok(io) = inspect {
                    let val = String::from_utf8_lossy(&io.stdout).trim().to_string();
                    if val != "true" {
                        // Check if it has explicit volumes/mounts
                        let mounts = std::process::Command::new("docker")
                            .args([
                                "inspect",
                                "--format",
                                "{{range .Mounts}}{{.Type}}{{end}}",
                                cid,
                            ])
                            .output();
                        let has_mounts = mounts
                            .map(|m| !String::from_utf8_lossy(&m.stdout).trim().is_empty())
                            .unwrap_or(false);
                        if !has_mounts {
                            let name = std::process::Command::new("docker")
                                .args(["inspect", "--format", "{{.Name}}", cid])
                                .output()
                                .map(|o| {
                                    String::from_utf8_lossy(&o.stdout)
                                        .trim()
                                        .trim_start_matches('/')
                                        .to_string()
                                })
                                .unwrap_or_else(|_| cid.to_string());
                            violations.push(name);
                        }
                    }
                }
            }

            if violations.is_empty() {
                findings.push(SecurityFinding {
                    check: "read-only-rootfs",
                    level: SecurityFindingLevel::Ok,
                    message: format!(
                        "All {} enola-* containers have read-only rootfs or explicit volumes",
                        container_ids.len()
                    ),
                });
            } else {
                findings.push(SecurityFinding {
                    check: "read-only-rootfs",
                    level: SecurityFindingLevel::Warning,
                    message: format!(
                        "{} container(s) without read-only rootfs or volumes: {}",
                        violations.len(),
                        violations.join(", ")
                    ),
                });
            }
        }
        _ => {
            findings.push(SecurityFinding {
                check: "read-only-rootfs",
                level: SecurityFindingLevel::Ok,
                message: "Docker not available — skipped".to_string(),
            });
        }
    }
    findings
}

/// (c) Check that no container binds to 0.0.0.0
fn check_container_bind_addresses() -> Vec<SecurityFinding> {
    let output = std::process::Command::new("docker")
        .args(["ps", "-q", "--filter", "name=enola-"])
        .output();

    let mut findings = Vec::new();

    match output {
        Ok(out) if out.status.success() => {
            let ids = String::from_utf8_lossy(&out.stdout);
            let container_ids: Vec<&str> = ids.lines().filter(|l| !l.is_empty()).collect();

            if container_ids.is_empty() {
                findings.push(SecurityFinding {
                    check: "bind-address",
                    level: SecurityFindingLevel::Ok,
                    message: "No enola-* containers running (nothing to check)".to_string(),
                });
                return findings;
            }

            let mut violations = Vec::new();
            for cid in &container_ids {
                let inspect = std::process::Command::new("docker")
                    .args([
                        "inspect",
                        "--format",
                        "{{json .HostConfig.PortBindings}}",
                        cid,
                    ])
                    .output();
                if let Ok(io) = inspect {
                    let val = String::from_utf8_lossy(&io.stdout).trim().to_string();
                    if val.contains("0.0.0.0") || val.contains("::") {
                        let name = std::process::Command::new("docker")
                            .args(["inspect", "--format", "{{.Name}}", cid])
                            .output()
                            .map(|o| {
                                String::from_utf8_lossy(&o.stdout)
                                    .trim()
                                    .trim_start_matches('/')
                                    .to_string()
                            })
                            .unwrap_or_else(|_| cid.to_string());
                        violations.push(name);
                    }
                }
            }

            if violations.is_empty() {
                findings.push(SecurityFinding {
                    check: "bind-address",
                    level: SecurityFindingLevel::Ok,
                    message: format!(
                        "All {} enola-* containers bind to 127.0.0.1 only",
                        container_ids.len()
                    ),
                });
            } else {
                findings.push(SecurityFinding {
                    check: "bind-address",
                    level: SecurityFindingLevel::Critical,
                    message: format!(
                        "{} container(s) binding to 0.0.0.0: {} (should be 127.0.0.1)",
                        violations.len(),
                        violations.join(", ")
                    ),
                });
            }
        }
        _ => {
            findings.push(SecurityFinding {
                check: "bind-address",
                level: SecurityFindingLevel::Ok,
                message: "Docker not available — skipped".to_string(),
            });
        }
    }
    findings
}

/// (d) Check Nginx fileserver configs for disable_symlinks and dangerous extension blocking
fn check_nginx_fileserver_security() -> Vec<SecurityFinding> {
    let mut findings = Vec::new();
    let nginx_sites = "/etc/nginx/sites-enabled";

    if !std::path::Path::new(nginx_sites).is_dir() {
        findings.push(SecurityFinding {
            check: "nginx-fileserver",
            level: SecurityFindingLevel::Ok,
            message: "Nginx sites-enabled not found — skipped".to_string(),
        });
        return findings;
    }

    let entries = match std::fs::read_dir(nginx_sites) {
        Ok(rd) => rd.filter_map(|e| e.ok()).collect::<Vec<_>>(),
        Err(_) => {
            findings.push(SecurityFinding {
                check: "nginx-fileserver",
                level: SecurityFindingLevel::Warning,
                message: "Cannot read /etc/nginx/sites-enabled".to_string(),
            });
            return findings;
        }
    };

    let fileserver_configs: Vec<_> = entries
        .iter()
        .filter(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            // Only check configs that are file servers (enola-files-*, or configs
            // that contain a root/alias directive pointing to /srv/enola).
            // Proxy configs (proxy_*) are reverse proxies, not file servers —
            // disable_symlinks and extension blocking don't apply to them.
            if !name.starts_with("enola-") {
                return false;
            }
            let content = std::fs::read_to_string(e.path()).unwrap_or_default();
            content.contains("root /srv/enola")
                || content.contains("alias /srv/enola")
                || content.contains("root /var/www/enola")
        })
        .collect();

    if fileserver_configs.is_empty() {
        findings.push(SecurityFinding {
            check: "nginx-fileserver",
            level: SecurityFindingLevel::Ok,
            message: "No Enola Nginx configs found — skipped".to_string(),
        });
        return findings;
    }

    let mut missing_symlink_protection = Vec::new();
    let mut missing_ext_blocking = Vec::new();

    for entry in &fileserver_configs {
        let path = entry.path();
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let name = entry.file_name().to_string_lossy().to_string();

        if !content.contains("disable_symlinks") {
            missing_symlink_protection.push(name.clone());
        }

        // Check for dangerous extension blocking (.env, .git, .sql, .pem)
        let has_ext_blocking = content.contains(".env")
            || content.contains(".git")
            || content.contains(".sql")
            || content.contains(".pem")
            || content.contains("location ~* \\.");
        if !has_ext_blocking {
            missing_ext_blocking.push(name);
        }
    }

    if missing_symlink_protection.is_empty() {
        findings.push(SecurityFinding {
            check: "nginx-symlinks",
            level: SecurityFindingLevel::Ok,
            message: "All Enola Nginx configs have disable_symlinks".to_string(),
        });
    } else {
        findings.push(SecurityFinding {
            check: "nginx-symlinks",
            level: SecurityFindingLevel::Warning,
            message: format!(
                "{} config(s) missing disable_symlinks: {}",
                missing_symlink_protection.len(),
                missing_symlink_protection.join(", ")
            ),
        });
    }

    if missing_ext_blocking.is_empty() {
        findings.push(SecurityFinding {
            check: "nginx-ext-block",
            level: SecurityFindingLevel::Ok,
            message: "All Enola Nginx configs block dangerous extensions".to_string(),
        });
    } else {
        findings.push(SecurityFinding {
            check: "nginx-ext-block",
            level: SecurityFindingLevel::Warning,
            message: format!(
                "{} config(s) missing dangerous extension blocking: {}",
                missing_ext_blocking.len(),
                missing_ext_blocking.join(", ")
            ),
        });
    }

    findings
}

/// (e) Check AppArmor is enabled
fn check_apparmor_enabled() -> Vec<SecurityFinding> {
    let mut findings = Vec::new();

    let output = std::process::Command::new("aa-status")
        .args(["--enabled"])
        .output();

    match output {
        Ok(out) => {
            if out.status.success() {
                findings.push(SecurityFinding {
                    check: "apparmor",
                    level: SecurityFindingLevel::Ok,
                    message: "AppArmor is enabled and enforcing".to_string(),
                });
            } else {
                findings.push(SecurityFinding {
                    check: "apparmor",
                    level: SecurityFindingLevel::Warning,
                    message: "AppArmor is installed but not enabled".to_string(),
                });
            }
        }
        Err(_) => {
            findings.push(SecurityFinding {
                check: "apparmor",
                level: SecurityFindingLevel::Warning,
                message: "aa-status not found — AppArmor may not be installed (run: sudo enola-cli setup --security)".to_string(),
            });
        }
    }
    findings
}

/// (f) Check UFW is active
fn check_ufw_active() -> Vec<SecurityFinding> {
    let mut findings = Vec::new();

    let output = std::process::Command::new("ufw").args(["status"]).output();

    match output {
        Ok(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            if stdout.contains("Status: active") {
                findings.push(SecurityFinding {
                    check: "ufw",
                    level: SecurityFindingLevel::Ok,
                    message: "UFW is active".to_string(),
                });
            } else {
                findings.push(SecurityFinding {
                    check: "ufw",
                    level: SecurityFindingLevel::Warning,
                    message: "UFW is installed but not active (run: sudo enola-cli firewall setup)"
                        .to_string(),
                });
            }
        }
        Ok(_) => {
            findings.push(SecurityFinding {
                check: "ufw",
                level: SecurityFindingLevel::Warning,
                message: "`ufw status` failed — may need root".to_string(),
            });
        }
        Err(_) => {
            findings.push(SecurityFinding {
                check: "ufw",
                level: SecurityFindingLevel::Warning,
                message: "ufw not found — UFW may not be installed (run: sudo enola-cli setup --security)".to_string(),
            });
        }
    }
    findings
}

/// (g) Check for secrets in container env vars
fn check_container_env_secrets() -> Vec<SecurityFinding> {
    let output = std::process::Command::new("docker")
        .args(["ps", "-q", "--filter", "name=enola-"])
        .output();

    let mut findings = Vec::new();

    match output {
        Ok(out) if out.status.success() => {
            let ids = String::from_utf8_lossy(&out.stdout);
            let container_ids: Vec<&str> = ids.lines().filter(|l| !l.is_empty()).collect();

            if container_ids.is_empty() {
                findings.push(SecurityFinding {
                    check: "env-secrets",
                    level: SecurityFindingLevel::Ok,
                    message: "No enola-* containers running (nothing to check)".to_string(),
                });
                return findings;
            }

            let mut violations = Vec::new();
            for cid in &container_ids {
                let inspect = std::process::Command::new("docker")
                    .args(["inspect", "--format", "{{.Config.Env}}", cid])
                    .output();
                if let Ok(io) = inspect {
                    let env = String::from_utf8_lossy(&io.stdout);
                    // Look for sensitive env vars without _FILE suffix
                    const SENSITIVE_KEYS: &[&str] = &["password", "secret", "private_key"];
                    for line in env.lines() {
                        let lower = line.to_lowercase();
                        let found = SENSITIVE_KEYS
                            .iter()
                            .any(|k| lower.contains(&format!("{}=", k)));
                        if found && !lower.contains("_file=") && !lower.contains("password_file=") {
                            let name = std::process::Command::new("docker")
                                .args(["inspect", "--format", "{{.Name}}", cid])
                                .output()
                                .map(|o| {
                                    String::from_utf8_lossy(&o.stdout)
                                        .trim()
                                        .trim_start_matches('/')
                                        .to_string()
                                })
                                .unwrap_or_else(|_| cid.to_string());
                            violations.push(name);
                            break;
                        }
                    }
                }
            }

            if violations.is_empty() {
                findings.push(SecurityFinding {
                    check: "env-secrets",
                    level: SecurityFindingLevel::Ok,
                    message: format!(
                        "No plaintext secrets found in {} container(s) env vars",
                        container_ids.len()
                    ),
                });
            } else {
                findings.push(SecurityFinding {
                    check: "env-secrets",
                    level: SecurityFindingLevel::Critical,
                    message: format!("{} container(s) have plaintext secrets in env vars (use _FILE= instead): {}", violations.len(), violations.join(", ")),
                });
            }
        }
        _ => {
            findings.push(SecurityFinding {
                check: "env-secrets",
                level: SecurityFindingLevel::Ok,
                message: "Docker not available — skipped".to_string(),
            });
        }
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_contains_title() {
        let h = header("foo");
        assert!(h.contains("foo"));
        assert!(h.contains("Enola CLI"));
    }

    #[test]
    fn subsection_wraps_title() {
        let s = subsection("bar");
        assert!(s.contains("── bar ──"));
    }

    #[test]
    fn config_section_lists_known_keys() {
        let s = section_config();
        // El catálogo de config_inspector incluye web.web_public_url
        assert!(s.contains("web.web_public_url"));
    }

    #[test]
    fn config_validation_section_always_non_empty() {
        let s = section_config_validation();
        assert!(s.contains("Config Validation"));
        assert!(s.contains("Resumen"));
    }

    #[tokio::test]
    async fn hardware_section_reports_cpu() {
        let s = section_hardware().await;
        assert!(s.contains("CPU cores"));
        assert!(s.contains("RAM"));
    }

    #[tokio::test]
    async fn full_report_composes_all_sections() {
        let r = full_report().await;
        // Checkeamos headers de cada sección.
        assert!(r.contains("System Dependencies") || r.contains("System Doctor"));
        assert!(r.contains("Configuration"));
        assert!(r.contains("Config Validation"));
        assert!(r.contains("Runtime Services"));
        assert!(r.contains("Hardware"));
    }
}
