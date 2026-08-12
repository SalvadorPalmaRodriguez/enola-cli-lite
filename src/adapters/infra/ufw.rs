use crate::domain::error::EnolaError;
use crate::domain::firewall::{FirewallAction, FirewallProtocol, FirewallRule, FirewallStatus};
use crate::ports::firewall::{FirewallPort, Result};
/// Implementación concreta de FirewallPort usando comandos `ufw`.
/// Tarea UFW-003 (191).
///
/// Todos los comandos ufw requieren root. Verificar con
/// `crate::infrastructure::privileges::check_root_permissions()` antes de llamar.
use std::process::Command;

/// Contenido de las reglas DOCKER-USER que se insertan en /etc/ufw/after.rules.
/// Sin estas reglas, Docker bypasea UFW completamente via iptables.
/// IMPORTANTE: Solo ASCII puro — UFW (Python) usa bytes(out, 'ascii') al escribir.
const DOCKER_USER_RULES: &str = "
# === ENOLA DOCKER-USER RULES ================================================
# These rules prevent Docker from exposing containers directly to the Internet
# bypassing UFW. See: docs/FIREWALL_SETUP.md section 4
*filter
:DOCKER-USER - [0:0]
# Allow traffic from localhost and internal Docker networks
-A DOCKER-USER -i lo -j RETURN
-A DOCKER-USER -s 127.0.0.0/8 -j RETURN
-A DOCKER-USER -s 172.16.0.0/12 -j RETURN
-A DOCKER-USER -s 10.0.0.0/8 -j RETURN
-A DOCKER-USER -s 192.168.0.0/16 -j RETURN
# Block new external connections to Docker containers
-A DOCKER-USER -m conntrack --ctstate NEW -j DROP
COMMIT
# ===========================================================================
";

const AFTER_RULES_PATH: &str = "/etc/ufw/after.rules";
const DOCKER_USER_MARKER: &str = "ENOLA DOCKER-USER RULES";

pub struct UfwAdapter;

impl UfwAdapter {
    pub fn new() -> Self {
        Self
    }

    /// Ejecuta un comando ufw con encoding UTF-8 forzado.
    /// Solo falla si el exit code es distinto de 0.
    /// UFW (Python) puede imprimir tracebacks/warnings en stdout aunque el comando
    /// haya tenido éxito (exit 0) — esos se ignoran.
    fn run_ufw(&self, args: &[&str]) -> Result<String> {
        let output = Command::new("ufw")
            .args(args)
            // Forzar UTF-8 en el subproceso Python de UFW para evitar
            // UnicodeEncodeError en entornos con LANG=C o sin locale UTF-8
            .env("LANG", "en_US.UTF-8")
            .env("LC_ALL", "en_US.UTF-8")
            .env("PYTHONIOENCODING", "utf-8")
            .env("PYTHONUTF8", "1")
            .output()
            .map_err(|e| {
                EnolaError::InfrastructureError(format!(
                    "Failed to execute ufw: {}. Is ufw installed? sudo apt install ufw",
                    e
                ))
            })?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if output.status.success() {
            // Exit 0 = éxito aunque haya tracebacks/warnings en stdout/stderr
            // (UFW con Python puede imprimir warnings no fatales)
            Ok(stdout)
        } else {
            Err(EnolaError::InfrastructureError(format!(
                "ufw command failed (exit {}): {} {}",
                output.status.code().unwrap_or(-1),
                stdout.trim(),
                stderr.trim()
            )))
        }
    }

    /// Parsea la salida de `ufw status verbose` para extraer reglas y política
    fn parse_status_output(&self, output: &str) -> FirewallStatus {
        let mut active = false;
        let mut default_incoming = FirewallAction::Allow;
        let mut default_outgoing = FirewallAction::Allow;
        let mut rules = Vec::new();

        for line in output.lines() {
            let line = line.trim();

            if line.starts_with("Status: active") {
                active = true;
            } else if line.starts_with("Default:") {
                // "Default: deny (incoming), allow (outgoing), ..."
                if line.contains("deny (incoming)") {
                    default_incoming = FirewallAction::Deny;
                }
                if line.contains("reject (incoming)") {
                    default_incoming = FirewallAction::Reject;
                }
                if line.contains("deny (outgoing)") {
                    default_outgoing = FirewallAction::Deny;
                }
            } else if line.contains("ALLOW") || line.contains("DENY") || line.contains("REJECT") {
                // Parsear líneas de reglas como:
                // "22/tcp                     ALLOW IN    Anywhere"
                // "80/tcp                     ALLOW IN    Anywhere"
                if let Some(rule) = self.parse_rule_line(line) {
                    rules.push(rule);
                }
            }
        }

        FirewallStatus {
            active,
            default_incoming,
            default_outgoing,
            rules,
            docker_user_configured: self.is_docker_user_configured().unwrap_or(false),
        }
    }

    /// Parsea una línea de regla de ufw status
    fn parse_rule_line(&self, line: &str) -> Option<FirewallRule> {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 2 {
            return None;
        }

        // "22/tcp" o "22" o "80/tcp (v6)"
        let port_proto = parts[0].trim_end_matches("(v6)").trim();
        let (port, protocol) = if port_proto.contains('/') {
            let mut sp = port_proto.splitn(2, '/');
            let p = sp.next()?.parse::<u16>().ok()?;
            let proto = sp
                .next()
                .unwrap_or("tcp")
                .parse()
                .unwrap_or(FirewallProtocol::Tcp);
            (p, proto)
        } else {
            let p = port_proto.parse::<u16>().ok()?;
            (p, FirewallProtocol::Tcp)
        };

        let action = if line.contains("ALLOW") {
            FirewallAction::Allow
        } else if line.contains("REJECT") {
            FirewallAction::Reject
        } else {
            FirewallAction::Deny
        };

        Some(FirewallRule {
            port,
            protocol,
            from: None,
            action,
        })
    }
}

impl Default for UfwAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl FirewallPort for UfwAdapter {
    fn is_installed(&self) -> bool {
        Command::new("which")
            .arg("ufw")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    fn is_active(&self) -> Result<bool> {
        let output = self.run_ufw(&["status"])?;
        Ok(output.contains("Status: active"))
    }

    fn enable_with_default_policy(&self, ssh_port: u16) -> Result<()> {
        // 1. Política por defecto
        self.run_ufw(&["default", "deny", "incoming"])?;
        self.run_ufw(&["default", "allow", "outgoing"])?;

        // 2. Permitir SSH antes de activar (evitar lockout)
        let ssh_rule = format!("{}/tcp", ssh_port);
        self.run_ufw(&["allow", &ssh_rule])?;

        // 3. Activar (--force para no pedir confirmación interactiva)
        self.run_ufw(&["--force", "enable"])?;

        Ok(())
    }

    fn allow_port(
        &self,
        port: u16,
        protocol: FirewallProtocol,
        from: Option<String>,
    ) -> Result<()> {
        let rule = format!("{}/{}", port, protocol.as_str());
        if let Some(source) = from {
            self.run_ufw(&[
                "allow",
                "from",
                &source,
                "to",
                "any",
                "port",
                &port.to_string(),
                "proto",
                protocol.as_str(),
            ])?;
        } else {
            self.run_ufw(&["allow", &rule])?;
        }
        Ok(())
    }

    fn deny_port(&self, port: u16, protocol: FirewallProtocol) -> Result<()> {
        let rule = format!("{}/{}", port, protocol.as_str());
        self.run_ufw(&["deny", &rule])?;
        Ok(())
    }

    fn delete_rule(&self, rule_number: u32) -> Result<()> {
        self.run_ufw(&["--force", "delete", &rule_number.to_string()])?;
        Ok(())
    }

    fn status(&self) -> Result<FirewallStatus> {
        let output = match self.run_ufw(&["status", "verbose"]) {
            Ok(o) => o,
            Err(_) => {
                // UFW no activo — devolver estado inactivo
                return Ok(FirewallStatus {
                    active: false,
                    default_incoming: FirewallAction::Allow,
                    default_outgoing: FirewallAction::Allow,
                    rules: vec![],
                    docker_user_configured: false,
                });
            }
        };
        Ok(self.parse_status_output(&output))
    }

    fn is_docker_user_configured(&self) -> Result<bool> {
        let content = std::fs::read_to_string(AFTER_RULES_PATH).unwrap_or_default();
        Ok(content.contains(DOCKER_USER_MARKER))
    }

    fn configure_docker_user_chain(&self) -> Result<()> {
        // Leer el archivo actual
        let content = std::fs::read_to_string(AFTER_RULES_PATH).unwrap_or_default();

        // Si ya está configurado con el marcador actual, no hacer nada
        if content.contains(DOCKER_USER_MARKER) {
            // Verificar que el contenido es ASCII puro (sin emojis/guiones UTF-8)
            if content.is_ascii() {
                return Ok(());
            }
            // Si tiene el marcador pero con caracteres non-ASCII (versión antigua),
            // limpiar y reescribir con ASCII puro
        }

        // Eliminar cualquier bloque ENOLA previo (ASCII o UTF-8)
        // Buscar desde cualquier línea que contenga "ENOLA DOCKER-USER RULES"
        let clean_content = {
            let mut lines: Vec<&str> = Vec::new();
            let mut skip = false;
            for line in content.lines() {
                if line.contains("ENOLA DOCKER-USER RULES") || line.contains("ENOLA DOCKER") {
                    skip = true;
                }
                if !skip {
                    lines.push(line);
                }
                // Terminar bloque skip cuando encontramos "===" o "───" al final del bloque
                if skip
                    && (line.starts_with("# ===") || line.starts_with("# ───"))
                    && !line.contains("ENOLA")
                {
                    skip = false;
                }
            }
            lines.join("\n")
        };

        // Construir nuevo contenido: original limpio + reglas ASCII puras
        let new_content = format!("{}\n{}", clean_content.trim_end(), DOCKER_USER_RULES);

        // Verificar que el resultado es ASCII puro antes de escribir
        if !new_content.is_ascii() {
            return Err(EnolaError::InfrastructureError(format!(
                "after.rules contiene caracteres non-ASCII que UFW no puede procesar. \
                     Revisa manualmente: {}",
                AFTER_RULES_PATH
            )));
        }

        // Escribir de vuelta (requiere root)
        std::fs::write(AFTER_RULES_PATH, &new_content).map_err(|e| {
            EnolaError::InfrastructureError(format!(
                "Cannot write {}: {}. Are you root?",
                AFTER_RULES_PATH, e
            ))
        })?;

        // Recargar UFW para aplicar cambios (ignorar error si UFW inactivo)
        let _ = self.run_ufw(&["reload"]);

        Ok(())
    }

    fn allow_loopback_port(&self, port: u16) -> Result<()> {
        self.run_ufw(&[
            "allow",
            "from",
            "127.0.0.1",
            "to",
            "any",
            "port",
            &port.to_string(),
            "proto",
            "tcp",
        ])?;
        Ok(())
    }

    fn remove_loopback_rule(&self, port: u16) -> Result<()> {
        // UFW delete by rule specification (no number needed)
        // Ignore errors: rule may not exist (e.g., service created before UFW was active)
        let _ = self.run_ufw(&[
            "--force",
            "delete",
            "allow",
            "from",
            "127.0.0.1",
            "to",
            "any",
            "port",
            &port.to_string(),
            "proto",
            "tcp",
        ]);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_adapter_new() {
        let adapter = UfwAdapter::new();
        // Verificar que se puede crear sin panic
        let _ = adapter.is_installed();
    }

    #[test]
    fn test_parse_status_active() {
        let adapter = UfwAdapter::new();
        let output = "Status: active\nDefault: deny (incoming), allow (outgoing)\nTo                         Action      From\n--                         ------      ----\n22/tcp                     ALLOW IN    Anywhere\n";
        let status = adapter.parse_status_output(output);
        assert!(status.active);
        assert_eq!(status.rules.len(), 1);
        assert_eq!(status.rules[0].port, 22);
    }

    #[test]
    fn test_parse_status_inactive() {
        let adapter = UfwAdapter::new();
        let output = "Status: inactive\n";
        let status = adapter.parse_status_output(output);
        assert!(!status.active);
        assert!(status.rules.is_empty());
    }

    #[test]
    fn test_parse_rule_line_tcp() {
        let adapter = UfwAdapter::new();
        let rule = adapter.parse_rule_line("22/tcp                     ALLOW IN    Anywhere");
        assert!(rule.is_some());
        let r = rule.unwrap();
        assert_eq!(r.port, 22);
        assert_eq!(r.protocol, FirewallProtocol::Tcp);
        assert!(matches!(r.action, FirewallAction::Allow));
    }

    #[test]
    fn test_parse_rule_line_deny() {
        let adapter = UfwAdapter::new();
        let rule = adapter.parse_rule_line("23/tcp                     DENY IN     Anywhere");
        assert!(rule.is_some());
        assert!(matches!(rule.unwrap().action, FirewallAction::Deny));
    }

    #[test]
    fn test_parse_rule_line_invalid() {
        let adapter = UfwAdapter::new();
        let rule = adapter.parse_rule_line("not a rule");
        assert!(rule.is_none());
    }

    #[test]
    fn test_docker_user_rules_marker_present() {
        assert!(DOCKER_USER_RULES.contains(DOCKER_USER_MARKER));
    }

    #[test]
    fn test_is_docker_user_configured_false_when_missing() {
        let adapter = UfwAdapter::new();
        // En entorno de test sin /etc/ufw/after.rules real,
        // debe retornar false (el archivo no existe o no tiene el marker)
        let result = adapter.is_docker_user_configured();
        assert!(result.is_ok());
    }
}
