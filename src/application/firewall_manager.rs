use crate::domain::error::EnolaError;
use crate::domain::firewall::{FirewallProtocol, FirewallStatus};
use crate::ports::firewall::FirewallPort;
/// Caso de uso para gestión de firewall UFW.
/// Tarea UFW-004 (192).
///
/// Usa solo el trait FirewallPort (inversión de dependencia).
/// NUNCA importa UfwAdapter directamente.
use std::sync::Arc;

pub type Result<T> = std::result::Result<T, EnolaError>;

/// Orquestador de operaciones de firewall.
/// Punto de entrada para todos los comandos CLI de firewall.
pub struct FirewallManager {
    firewall: Arc<dyn FirewallPort>,
}

impl FirewallManager {
    pub fn new(firewall: Arc<dyn FirewallPort>) -> Self {
        Self { firewall }
    }

    /// Configura UFW con política segura por defecto.
    /// Flujo:
    ///   1. Verificar que UFW está instalado
    ///   2. Aplicar política: deny incoming, allow outgoing
    ///   3. Abrir SSH (anti-lockout)
    ///   4. Activar UFW
    ///   5. Configurar cadena DOCKER-USER
    ///   6. FW-004: Reconciliar servicios existentes (loopback rules)
    ///
    /// Retorna resumen de lo que se hizo.
    pub fn setup_secure_defaults(&self, ssh_port: u16, existing_ports: &[u16]) -> Result<String> {
        if !self.firewall.is_installed() {
            return Err(EnolaError::InfrastructureError(
                "UFW is not installed. Install it with:\n  sudo apt install ufw".to_string(),
            ));
        }

        self.firewall.enable_with_default_policy(ssh_port)?;
        self.firewall.configure_docker_user_chain()?;

        // FW-004: Reconciliar servicios existentes — crear reglas loopback
        let reconciled = self.reconcile_existing_services(existing_ports);

        let status = self.firewall.status()?;
        let reconcile_msg = if reconciled > 0 {
            format!(
                "\n• Existing services reconciled: {} loopback rules added",
                reconciled
            )
        } else {
            String::new()
        };
        Ok(format!(
            "✅ Firewall configured securely\n\
             {}\n\
             \n\
             Rules applied:\n\
             • Default: deny incoming, allow outgoing\n\
             • SSH ({}/tcp): ALLOWED\n\
             • DOCKER-USER chain: {}{}\n\
             \n\
             ⚠️  Enola services bind to 127.0.0.1 — no UFW rules needed for them.\n\
             Use 'enola-cli firewall allow --port X' to open additional ports.",
            status.summary(),
            ssh_port,
            if status.docker_user_configured {
                "✅ configured"
            } else {
                "❌ failed — run setup again"
            },
            reconcile_msg
        ))
    }

    /// FW-004: Reconcilia servicios ya existentes al activar UFW.
    /// Para cada puerto de servicio Enola activo, crea una regla loopback
    /// `allow from 127.0.0.1 to any port PORT proto tcp` para que la cadena
    /// Tor→Nginx→Docker no se rompa.
    ///
    /// Retorna cuántas reglas se añadieron correctamente.
    pub fn reconcile_existing_services(&self, ports: &[u16]) -> usize {
        let mut added = 0;
        for &port in ports {
            if port == 0 {
                continue;
            }
            match self.firewall.allow_loopback_port(port) {
                Ok(()) => added += 1,
                Err(e) => eprintln!("⚠️  Could not add loopback rule for port {}: {}", port, e),
            }
        }
        added
    }

    /// Permite tráfico en un puerto.
    pub fn add_rule(
        &self,
        port: u16,
        protocol: FirewallProtocol,
        from: Option<String>,
    ) -> Result<String> {
        self.validate_port(port)?;
        let label = format!("{}/{}", port, protocol);
        self.firewall.allow_port(port, protocol, from.clone())?;
        Ok(format!(
            "✅ Port {} allowed{}",
            label,
            from.map(|f| format!(" from {}", f)).unwrap_or_default()
        ))
    }

    /// Deniega tráfico en un puerto.
    pub fn deny_rule(&self, port: u16, protocol: FirewallProtocol) -> Result<String> {
        self.validate_port(port)?;
        let warning = if self.is_enola_internal_port(port) {
            "\n⚠️  Note: This port is used by an Enola service bound to 127.0.0.1.\n   UFW rules don't affect localhost traffic, but the rule has been added."
        } else {
            ""
        };
        let label = format!("{}/{}", port, protocol);
        self.firewall.deny_port(port, protocol)?;
        Ok(format!("✅ Port {} denied{}", label, warning))
    }

    /// Devuelve el estado completo del firewall.
    pub fn get_status(&self) -> Result<FirewallStatus> {
        self.firewall.status()
    }

    /// Devuelve un mensaje de advertencia si UFW no está activo.
    /// Usado al crear servicios para avisar al usuario (no bloqueante).
    pub fn inactive_warning(&self) -> Option<String> {
        if !self.firewall.is_installed() {
            return None; // No instalado → no avisamos
        }
        match self.firewall.is_active() {
            Ok(false) => Some(
                "⚠️  UFW firewall is not active. Your services are protected by 127.0.0.1 binding,\n   but enabling the firewall is recommended:\n   sudo enola-cli firewall setup".to_string()
            ),
            _ => None,
        }
    }

    // ─── helpers ─────────────────────────────────────────────────────────────

    fn validate_port(&self, port: u16) -> Result<()> {
        if port == 0 {
            return Err(EnolaError::ValidationError(
                "Port 0 is not valid".to_string(),
            ));
        }
        Ok(())
    }

    /// Puertos que Enola usa internamente (bindeados a 127.0.0.1)
    fn is_enola_internal_port(&self, port: u16) -> bool {
        // Nginx listen range (10000-20000)
        if (10000..=20000).contains(&port) {
            return true;
        }
        // WordPress backend (8080-9000)
        if (8080..=9000).contains(&port) {
            return true;
        }
        // Git/Forgejo (3000-4000)
        if (3000..=4000).contains(&port) {
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::firewall::{FirewallAction, FirewallStatus};
    use crate::ports::firewall::MockFirewallPort;

    fn secure_status() -> FirewallStatus {
        FirewallStatus {
            active: true,
            default_incoming: FirewallAction::Deny,
            default_outgoing: FirewallAction::Allow,
            rules: vec![],
            docker_user_configured: true,
        }
    }

    #[test]
    fn test_setup_secure_defaults_ok() {
        let mut mock = MockFirewallPort::new();
        mock.expect_is_installed().returning(|| true);
        mock.expect_enable_with_default_policy()
            .returning(|_| Ok(()));
        mock.expect_configure_docker_user_chain()
            .returning(|| Ok(()));
        mock.expect_status().returning(|| Ok(secure_status()));

        let mgr = FirewallManager::new(Arc::new(mock));
        let result = mgr.setup_secure_defaults(22, &[]);
        assert!(result.is_ok());
        let msg = result.unwrap();
        assert!(msg.contains("configured securely") || msg.contains("Firewall"));
    }

    #[test]
    fn test_setup_fails_if_ufw_not_installed() {
        let mut mock = MockFirewallPort::new();
        mock.expect_is_installed().returning(|| false);

        let mgr = FirewallManager::new(Arc::new(mock));
        let result = mgr.setup_secure_defaults(22, &[]);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("not installed") || err.contains("apt install ufw"));
    }

    #[test]
    fn test_setup_with_existing_services_reconciles() {
        let mut mock = MockFirewallPort::new();
        mock.expect_is_installed().returning(|| true);
        mock.expect_enable_with_default_policy()
            .returning(|_| Ok(()));
        mock.expect_configure_docker_user_chain()
            .returning(|| Ok(()));
        mock.expect_allow_loopback_port()
            .times(2)
            .returning(|_| Ok(()));
        mock.expect_status().returning(|| Ok(secure_status()));

        let mgr = FirewallManager::new(Arc::new(mock));
        let result = mgr.setup_secure_defaults(22, &[8080, 11435]);
        assert!(result.is_ok());
        let msg = result.unwrap();
        assert!(msg.contains("2 loopback rules added"));
    }

    #[test]
    fn test_reconcile_skips_port_zero() {
        let mut mock = MockFirewallPort::new();
        // allow_loopback_port should NOT be called for port 0
        mock.expect_allow_loopback_port()
            .times(1)
            .returning(|_| Ok(()));

        let mgr = FirewallManager::new(Arc::new(mock));
        let added = mgr.reconcile_existing_services(&[0, 8080]);
        assert_eq!(added, 1);
    }

    #[test]
    fn test_add_rule_ok() {
        let mut mock = MockFirewallPort::new();
        mock.expect_allow_port().returning(|_, _, _| Ok(()));

        let mgr = FirewallManager::new(Arc::new(mock));
        let result = mgr.add_rule(80, FirewallProtocol::Tcp, None);
        assert!(result.is_ok());
        assert!(result.unwrap().contains("80"));
    }

    #[test]
    fn test_add_rule_port_zero_fails() {
        let mock = MockFirewallPort::new();
        let mgr = FirewallManager::new(Arc::new(mock));
        assert!(mgr.add_rule(0, FirewallProtocol::Tcp, None).is_err());
    }

    #[test]
    fn test_deny_rule_with_enola_port_shows_warning() {
        let mut mock = MockFirewallPort::new();
        mock.expect_deny_port().returning(|_, _| Ok(()));

        let mgr = FirewallManager::new(Arc::new(mock));
        let result = mgr.deny_rule(8080, FirewallProtocol::Tcp);
        assert!(result.is_ok());
        let msg = result.unwrap();
        assert!(msg.contains("127.0.0.1") || msg.contains("localhost"));
    }

    #[test]
    fn test_inactive_warning_shown_when_ufw_inactive() {
        let mut mock = MockFirewallPort::new();
        mock.expect_is_installed().returning(|| true);
        mock.expect_is_active().returning(|| Ok(false));

        let mgr = FirewallManager::new(Arc::new(mock));
        let warn = mgr.inactive_warning();
        assert!(warn.is_some());
        assert!(warn.unwrap().contains("UFW"));
    }

    #[test]
    fn test_no_warning_when_ufw_not_installed() {
        let mut mock = MockFirewallPort::new();
        mock.expect_is_installed().returning(|| false);

        let mgr = FirewallManager::new(Arc::new(mock));
        assert!(mgr.inactive_warning().is_none());
    }

    #[test]
    fn test_no_warning_when_ufw_active() {
        let mut mock = MockFirewallPort::new();
        mock.expect_is_installed().returning(|| true);
        mock.expect_is_active().returning(|| Ok(true));

        let mgr = FirewallManager::new(Arc::new(mock));
        assert!(mgr.inactive_warning().is_none());
    }
}
