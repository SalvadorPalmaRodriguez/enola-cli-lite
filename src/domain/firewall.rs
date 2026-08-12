/// Tipos de dominio para gestión de firewall UFW.
/// Sin dependencias externas — lógica pura.
///
/// Tarea UFW-001 (189)
///
/// Protocolo de red para una regla de firewall
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FirewallProtocol {
    Tcp,
    Udp,
    Both,
}

impl FirewallProtocol {
    pub fn as_str(&self) -> &str {
        match self {
            FirewallProtocol::Tcp => "tcp",
            FirewallProtocol::Udp => "udp",
            FirewallProtocol::Both => "any",
        }
    }
}

impl std::fmt::Display for FirewallProtocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for FirewallProtocol {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "tcp" => Ok(FirewallProtocol::Tcp),
            "udp" => Ok(FirewallProtocol::Udp),
            "both" | "any" => Ok(FirewallProtocol::Both),
            other => Err(format!("Unknown protocol '{}'. Use: tcp, udp, both", other)),
        }
    }
}

/// Acción de una regla de firewall
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FirewallAction {
    Allow,
    Deny,
    Reject,
}

impl std::fmt::Display for FirewallAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FirewallAction::Allow => write!(f, "ALLOW"),
            FirewallAction::Deny => write!(f, "DENY"),
            FirewallAction::Reject => write!(f, "REJECT"),
        }
    }
}

/// Una regla de firewall
#[derive(Debug, Clone, serde::Serialize)]
pub struct FirewallRule {
    /// Puerto afectado por la regla
    pub port: u16,
    /// Protocolo (tcp/udp/both)
    pub protocol: FirewallProtocol,
    /// IP o CIDR de origen. None = anywhere
    pub from: Option<String>,
    /// Acción (allow/deny/reject)
    pub action: FirewallAction,
}

/// Estado completo del firewall
#[derive(Debug, Clone, serde::Serialize)]
pub struct FirewallStatus {
    /// UFW está activo
    pub active: bool,
    /// Política por defecto para tráfico entrante
    pub default_incoming: FirewallAction,
    /// Política por defecto para tráfico saliente
    pub default_outgoing: FirewallAction,
    /// Reglas activas
    pub rules: Vec<FirewallRule>,
    /// La cadena DOCKER-USER está configurada en /etc/ufw/after.rules
    pub docker_user_configured: bool,
}

impl FirewallStatus {
    /// Indica si la configuración es segura (activo + DOCKER-USER configurado)
    pub fn is_secure(&self) -> bool {
        self.active && self.docker_user_configured
    }

    /// Devuelve un resumen legible del estado
    pub fn summary(&self) -> String {
        format!(
            "UFW: {} | Incoming: {} | Outgoing: {} | Rules: {} | Docker-User: {}",
            if self.active { "active" } else { "inactive" },
            self.default_incoming,
            self.default_outgoing,
            self.rules.len(),
            if self.docker_user_configured {
                "✅"
            } else {
                "❌"
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_display() {
        assert_eq!(FirewallProtocol::Tcp.to_string(), "tcp");
        assert_eq!(FirewallProtocol::Udp.to_string(), "udp");
        assert_eq!(FirewallProtocol::Both.to_string(), "any");
    }

    #[test]
    fn test_protocol_from_str() {
        assert_eq!(
            "tcp".parse::<FirewallProtocol>().unwrap(),
            FirewallProtocol::Tcp
        );
        assert_eq!(
            "UDP".parse::<FirewallProtocol>().unwrap(),
            FirewallProtocol::Udp
        );
        assert_eq!(
            "both".parse::<FirewallProtocol>().unwrap(),
            FirewallProtocol::Both
        );
        assert!("invalid".parse::<FirewallProtocol>().is_err());
    }

    #[test]
    fn test_action_display() {
        assert_eq!(FirewallAction::Allow.to_string(), "ALLOW");
        assert_eq!(FirewallAction::Deny.to_string(), "DENY");
    }

    #[test]
    fn test_status_is_secure() {
        let status = FirewallStatus {
            active: true,
            default_incoming: FirewallAction::Deny,
            default_outgoing: FirewallAction::Allow,
            rules: vec![],
            docker_user_configured: true,
        };
        assert!(status.is_secure());
    }

    #[test]
    fn test_status_not_secure_without_docker_user() {
        let status = FirewallStatus {
            active: true,
            default_incoming: FirewallAction::Deny,
            default_outgoing: FirewallAction::Allow,
            rules: vec![],
            docker_user_configured: false,
        };
        assert!(!status.is_secure());
    }

    #[test]
    fn test_status_not_secure_when_inactive() {
        let status = FirewallStatus {
            active: false,
            default_incoming: FirewallAction::Allow,
            default_outgoing: FirewallAction::Allow,
            rules: vec![],
            docker_user_configured: true,
        };
        assert!(!status.is_secure());
    }

    #[test]
    fn test_status_summary_contains_key_info() {
        let status = FirewallStatus {
            active: true,
            default_incoming: FirewallAction::Deny,
            default_outgoing: FirewallAction::Allow,
            rules: vec![],
            docker_user_configured: true,
        };
        let s = status.summary();
        assert!(s.contains("active"));
        assert!(s.contains("DENY"));
    }

    // ── Error-path tests ──

    #[test]
    fn test_action_reject_display() {
        assert_eq!(FirewallAction::Reject.to_string(), "REJECT");
    }

    #[test]
    fn test_protocol_from_str_empty() {
        assert!("".parse::<FirewallProtocol>().is_err());
    }

    #[test]
    fn test_protocol_from_str_invalid() {
        let result = "icmp".parse::<FirewallProtocol>();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("Unknown protocol"));
        assert!(err.contains("icmp"));
    }

    #[test]
    fn test_status_summary_when_inactive() {
        let status = FirewallStatus {
            active: false,
            default_incoming: FirewallAction::Deny,
            default_outgoing: FirewallAction::Allow,
            rules: vec![],
            docker_user_configured: false,
        };
        let s = status.summary();
        assert!(s.contains("inactive"));
        assert!(s.contains("❌"));
    }

    #[test]
    fn test_is_secure_false_when_both_false() {
        let status = FirewallStatus {
            active: false,
            default_incoming: FirewallAction::Allow,
            default_outgoing: FirewallAction::Allow,
            rules: vec![],
            docker_user_configured: false,
        };
        assert!(!status.is_secure());
    }

    #[test]
    fn test_summary_with_rules_count() {
        let status = FirewallStatus {
            active: true,
            default_incoming: FirewallAction::Deny,
            default_outgoing: FirewallAction::Allow,
            rules: vec![
                FirewallRule {
                    port: 80,
                    protocol: FirewallProtocol::Tcp,
                    from: None,
                    action: FirewallAction::Allow,
                },
                FirewallRule {
                    port: 443,
                    protocol: FirewallProtocol::Tcp,
                    from: None,
                    action: FirewallAction::Allow,
                },
            ],
            docker_user_configured: true,
        };
        let s = status.summary();
        assert!(s.contains("Rules: 2"));
    }

    #[test]
    fn test_protocol_any_alias() {
        assert_eq!(
            "any".parse::<FirewallProtocol>().unwrap(),
            FirewallProtocol::Both
        );
    }
}
