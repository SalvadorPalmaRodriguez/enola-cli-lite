use crate::domain::error::EnolaError;
use crate::ports::port_checker::PortCheckerPort;
/// Caso de uso para validación de puertos antes de crear/editar servicios.
/// Tarea PORTS-001 (175).
///
/// Responsabilidad única: verificar que un conjunto de puertos están disponibles
/// ANTES de que comience cualquier operación que modifique el sistema.
/// Si falla → error inmediato, 0 residuos en el sistema.
use std::sync::Arc;

pub type Result<T> = std::result::Result<T, EnolaError>;

/// Rangos de puertos por defecto para auto-asignación
pub struct PortRanges;

impl PortRanges {
    /// Rango para puertos de escucha Nginx (interno, entre Tor y Nginx)
    pub const NGINX_LISTEN: (u16, u16) = (10000, 20000);
    /// Rango para puertos backend WordPress
    pub const WORDPRESS_BACKEND: (u16, u16) = (8080, 9000);
    /// Rango para puertos HTTP de Git/Forgejo
    /// Rango alto (10000-15000) para evitar conflictos con servicios comunes:
    /// - Puerto 3000: Forgejo por defecto
    /// - Puertos <8000: muchas aplicaciones web estándar
    pub const GIT_HTTP: (u16, u16) = (10000, 15000);
    /// Rango para puertos SSH de Git/Forgejo
    /// Rango alto (30000-35000) para evitar conflictos con:
    /// - Puerto 22: SSH del sistema
    /// - Puerto 2222: SSH alternativo común
    pub const GIT_SSH: (u16, u16) = (30000, 35000);
    /// Puerto SSH del sistema (no tocar)
    pub const RESERVED_SSH: u16 = 22;
}

/// Valida puertos y auto-asigna libres.
pub struct PortValidator {
    checker: Arc<dyn PortCheckerPort>,
}

impl PortValidator {
    pub fn new(checker: Arc<dyn PortCheckerPort>) -> Self {
        Self { checker }
    }

    /// Valida que un puerto específico está libre.
    /// Si no lo está, devuelve error descriptivo con instrucciones.
    pub fn validate_port(&self, port: u16, label: &str) -> Result<()> {
        // Puertos reservados por el sistema
        if port < 1024 && port != 80 && port != 443 {
            return Err(EnolaError::ValidationError(format!(
                "Port {} ({}) is a privileged port (<1024). Use ports ≥1024 or the standard 80/443.",
                port, label
            )));
        }

        let result = self.checker.check_port(port)?;
        if !result.is_free() {
            return Err(EnolaError::ValidationError(format!(
                "Port {} ({}) is not available.\n{}\n\nChoose a different port with --{}-port <PORT>",
                port,
                label,
                result.error_message().unwrap_or_default(),
                label.to_lowercase().replace(' ', "-")
            )));
        }
        Ok(())
    }

    /// Valida un conjunto de puertos de una vez.
    /// Falla en el primer puerto ocupado (fail-fast).
    pub fn validate_ports(&self, ports: &[(u16, &str)]) -> Result<()> {
        for (port, label) in ports {
            self.validate_port(*port, label)?;
        }
        Ok(())
    }

    /// Auto-asigna un puerto libre en el rango dado.
    /// Devuelve el primero libre o error si el rango está agotado.
    pub fn auto_assign(&self, range: (u16, u16), label: &str) -> Result<u16> {
        self.checker.find_free_port(range.0, range.1).map_err(|_| {
            EnolaError::ValidationError(format!(
                "No free port available in range {}-{} for {}.\n\
                 Free up ports or specify one manually with --{}-port <PORT>",
                range.0,
                range.1,
                label,
                label.to_lowercase().replace(' ', "-")
            ))
        })
    }

    /// Devuelve el puerto si se especificó manualmente, o auto-asigna uno libre.
    pub fn resolve_port(&self, manual: Option<u16>, range: (u16, u16), label: &str) -> Result<u16> {
        match manual {
            Some(port) => {
                self.validate_port(port, label)?;
                Ok(port)
            }
            None => self.auto_assign(range, label),
        }
    }

    /// Verifica todos los puertos de un servicio Tor+Nginx+Docker de una vez.
    /// Incluye: nginx_listen y backend.
    /// Los puertos virtuales (.onion) no necesitan verificación — no son sockets reales.
    pub fn validate_service_ports(&self, nginx_listen: u16, backend: u16) -> Result<()> {
        self.validate_ports(&[(nginx_listen, "nginx-listen"), (backend, "backend")])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::port_checker::{MockPortCheckerPort, PortCheckResult};

    fn free_result(port: u16) -> PortCheckResult {
        PortCheckResult {
            port,
            free_os: true,
            free_docker: true,
        }
    }

    fn busy_result(port: u16) -> PortCheckResult {
        PortCheckResult {
            port,
            free_os: false,
            free_docker: true,
        }
    }

    #[test]
    fn test_validate_port_free() {
        let mut mock = MockPortCheckerPort::new();
        mock.expect_check_port().returning(|p| Ok(free_result(p)));
        let v = PortValidator::new(Arc::new(mock));
        assert!(v.validate_port(8080, "backend").is_ok());
    }

    #[test]
    fn test_validate_port_busy() {
        let mut mock = MockPortCheckerPort::new();
        mock.expect_check_port().returning(|p| Ok(busy_result(p)));
        let v = PortValidator::new(Arc::new(mock));
        let err = v.validate_port(8080, "backend").unwrap_err().to_string();
        assert!(err.contains("8080"));
        assert!(err.contains("not available") || err.contains("in use"));
    }

    #[test]
    fn test_validate_port_privileged_blocked() {
        let mock = MockPortCheckerPort::new();
        let v = PortValidator::new(Arc::new(mock));
        assert!(v.validate_port(25, "smtp").is_err());
    }

    #[test]
    fn test_validate_port_80_allowed() {
        let mut mock = MockPortCheckerPort::new();
        mock.expect_check_port().returning(|p| Ok(free_result(p)));
        let v = PortValidator::new(Arc::new(mock));
        // 80 es especial — permitido aunque sea <1024
        assert!(v.validate_port(80, "onion-http").is_ok());
    }

    #[test]
    fn test_validate_ports_fails_on_first_busy() {
        let mut mock = MockPortCheckerPort::new();
        mock.expect_check_port()
            .withf(|p| *p == 8080)
            .returning(|p| Ok(busy_result(p)));
        let v = PortValidator::new(Arc::new(mock));
        let result = v.validate_ports(&[(8080, "backend")]);
        assert!(result.is_err());
    }

    #[test]
    fn test_auto_assign_ok() {
        let mut mock = MockPortCheckerPort::new();
        mock.expect_find_free_port().returning(|start, _| Ok(start));
        let v = PortValidator::new(Arc::new(mock));
        let port = v.auto_assign(PortRanges::GIT_HTTP, "git-http").unwrap();
        assert_eq!(port, PortRanges::GIT_HTTP.0);
    }

    #[test]
    fn test_auto_assign_no_free_port_error() {
        let mut mock = MockPortCheckerPort::new();
        mock.expect_find_free_port()
            .returning(|_, _| Err(EnolaError::ValidationError("no free port".to_string())));
        let v = PortValidator::new(Arc::new(mock));
        let err = v
            .auto_assign(PortRanges::GIT_HTTP, "git-http")
            .unwrap_err()
            .to_string();
        assert!(err.contains("No free port") || err.contains("no free port"));
    }

    #[test]
    fn test_resolve_port_manual() {
        let mut mock = MockPortCheckerPort::new();
        mock.expect_check_port().returning(|p| Ok(free_result(p)));
        let v = PortValidator::new(Arc::new(mock));
        assert_eq!(
            v.resolve_port(Some(3500), PortRanges::GIT_HTTP, "git-http")
                .unwrap(),
            3500
        );
    }

    #[test]
    fn test_resolve_port_auto() {
        let mut mock = MockPortCheckerPort::new();
        mock.expect_find_free_port().returning(|start, _| Ok(start));
        let v = PortValidator::new(Arc::new(mock));
        let port = v
            .resolve_port(None, PortRanges::GIT_HTTP, "git-http")
            .unwrap();
        assert_eq!(port, PortRanges::GIT_HTTP.0);
    }

    #[test]
    fn test_validate_service_ports_ok() {
        let mut mock = MockPortCheckerPort::new();
        mock.expect_check_port().returning(|p| Ok(free_result(p)));
        let v = PortValidator::new(Arc::new(mock));
        assert!(v.validate_service_ports(15000, 8080).is_ok());
    }

    #[test]
    fn test_port_ranges_defined() {
        assert!(PortRanges::NGINX_LISTEN.0 < PortRanges::NGINX_LISTEN.1);
        assert!(PortRanges::GIT_HTTP.0 < PortRanges::GIT_HTTP.1);
        assert!(PortRanges::GIT_SSH.0 < PortRanges::GIT_SSH.1);
    }
}
