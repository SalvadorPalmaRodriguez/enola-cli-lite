/// Trait inyectable para gestión de firewall UFW.
/// Mockeable con mockall. Tarea UFW-002 (190).
///
/// Implementación concreta: `src/adapters/infra/ufw.rs`
use crate::domain::error::EnolaError;
use crate::domain::firewall::{FirewallProtocol, FirewallStatus};

pub type Result<T> = std::result::Result<T, EnolaError>;

#[cfg_attr(test, mockall::automock)]
pub trait FirewallPort: Send + Sync {
    /// Indica si UFW está instalado en el sistema
    fn is_installed(&self) -> bool;

    /// Indica si UFW está activo (running)
    fn is_active(&self) -> Result<bool>;

    /// Activa UFW con política segura por defecto:
    /// deny incoming, allow outgoing, allow ssh_port/tcp
    fn enable_with_default_policy(&self, ssh_port: u16) -> Result<()>;

    /// Permite tráfico en un puerto
    fn allow_port(&self, port: u16, protocol: FirewallProtocol, from: Option<String>)
        -> Result<()>;

    /// Deniega tráfico en un puerto
    fn deny_port(&self, port: u16, protocol: FirewallProtocol) -> Result<()>;

    /// Elimina una regla por número (de `ufw status numbered`)
    fn delete_rule(&self, rule_number: u32) -> Result<()>;

    /// Estado completo del firewall
    fn status(&self) -> Result<FirewallStatus>;

    /// Indica si la cadena DOCKER-USER ya está configurada en /etc/ufw/after.rules
    fn is_docker_user_configured(&self) -> Result<bool>;

    /// Añade las reglas DOCKER-USER a /etc/ufw/after.rules y recarga UFW.
    /// Sin esto, Docker bypasea TODAS las reglas UFW vía iptables.
    fn configure_docker_user_chain(&self) -> Result<()>;

    /// Permite tráfico desde loopback (127.0.0.1) a un puerto TCP.
    /// Usado internamente por los comandos create/edit para que la cadena
    /// Tor→Nginx→Docker funcione cuando UFW está activo.
    ///
    /// Regla generada: `ufw allow from 127.0.0.1 to any port PORT proto tcp`
    fn allow_loopback_port(&self, port: u16) -> Result<()>;

    /// Elimina la regla loopback para un puerto.
    /// Usado internamente por delete/edit cuando un puerto se libera.
    fn remove_loopback_rule(&self, port: u16) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::firewall::{FirewallAction, FirewallProtocol, FirewallStatus};

    #[test]
    fn test_mock_is_installed_true() {
        let mut mock = MockFirewallPort::new();
        mock.expect_is_installed().returning(|| true);
        assert!(mock.is_installed());
    }

    #[test]
    fn test_mock_is_active_ok() {
        let mut mock = MockFirewallPort::new();
        mock.expect_is_active().returning(|| Ok(true));
        assert!(mock.is_active().unwrap());
    }

    #[test]
    fn test_mock_enable_with_default_policy() {
        let mut mock = MockFirewallPort::new();
        mock.expect_enable_with_default_policy()
            .withf(|port| *port == 22)
            .returning(|_| Ok(()));
        assert!(mock.enable_with_default_policy(22).is_ok());
    }

    #[test]
    fn test_mock_allow_port() {
        let mut mock = MockFirewallPort::new();
        mock.expect_allow_port().returning(|_, _, _| Ok(()));
        assert!(mock.allow_port(80, FirewallProtocol::Tcp, None).is_ok());
    }

    #[test]
    fn test_mock_deny_port() {
        let mut mock = MockFirewallPort::new();
        mock.expect_deny_port().returning(|_, _| Ok(()));
        assert!(mock.deny_port(23, FirewallProtocol::Tcp).is_ok());
    }

    #[test]
    fn test_mock_status() {
        let mut mock = MockFirewallPort::new();
        mock.expect_status().returning(|| {
            Ok(FirewallStatus {
                active: true,
                default_incoming: FirewallAction::Deny,
                default_outgoing: FirewallAction::Allow,
                rules: vec![],
                docker_user_configured: true,
            })
        });
        let s = mock.status().unwrap();
        assert!(s.active);
        assert!(s.is_secure());
    }

    #[test]
    fn test_mock_docker_user_not_configured() {
        let mut mock = MockFirewallPort::new();
        mock.expect_is_docker_user_configured()
            .returning(|| Ok(false));
        assert!(!mock.is_docker_user_configured().unwrap());
    }

    #[test]
    fn test_mock_configure_docker_user_chain() {
        let mut mock = MockFirewallPort::new();
        mock.expect_configure_docker_user_chain()
            .returning(|| Ok(()));
        assert!(mock.configure_docker_user_chain().is_ok());
    }

    #[test]
    fn test_mock_allow_loopback_port() {
        let mut mock = MockFirewallPort::new();
        mock.expect_allow_loopback_port()
            .withf(|port| *port == 8080)
            .returning(|_| Ok(()));
        assert!(mock.allow_loopback_port(8080).is_ok());
    }

    #[test]
    fn test_mock_remove_loopback_rule() {
        let mut mock = MockFirewallPort::new();
        mock.expect_remove_loopback_rule()
            .withf(|port| *port == 8080)
            .returning(|_| Ok(()));
        assert!(mock.remove_loopback_rule(8080).is_ok());
    }
}
