/// Trait inyectable para verificar disponibilidad de puertos.
/// Tarea PORTS-002 (176).
///
/// Implementación concreta: `src/adapters/infra/port_checker.rs`
/// Mockeable con mockall para tests unitarios.
use crate::domain::error::EnolaError;

pub type Result<T> = std::result::Result<T, EnolaError>;

/// Resultado de la verificación de un puerto
#[derive(Debug, Clone, serde::Serialize)]
pub struct PortCheckResult {
    pub port: u16,
    /// Puerto libre a nivel del sistema operativo (TcpListener::bind)
    pub free_os: bool,
    /// Puerto libre a nivel Docker (contenedores parados también retienen el binding)
    pub free_docker: bool,
}

impl PortCheckResult {
    /// El puerto está completamente libre (OS + Docker)
    pub fn is_free(&self) -> bool {
        self.free_os && self.free_docker
    }

    pub fn error_message(&self) -> Option<String> {
        if self.free_os && self.free_docker {
            return None;
        }
        let who = if !self.free_os && !self.free_docker {
            "the OS and a Docker container"
        } else if !self.free_os {
            "another process on the OS"
        } else {
            "a Docker container (stopped containers retain port bindings)"
        };
        Some(format!("Port {} is already in use by {}", self.port, who))
    }
}

#[cfg_attr(test, mockall::automock)]
pub trait PortCheckerPort: Send + Sync {
    /// Verifica si un puerto está libre tanto en el OS como en Docker
    fn check_port(&self, port: u16) -> Result<PortCheckResult>;

    /// Encuentra el primer puerto libre en un rango dado
    fn find_free_port(&self, start: u16, end: u16) -> Result<u16>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_port_check_result_is_free() {
        let r = PortCheckResult {
            port: 8080,
            free_os: true,
            free_docker: true,
        };
        assert!(r.is_free());
        assert!(r.error_message().is_none());
    }

    #[test]
    fn test_port_check_result_os_busy() {
        let r = PortCheckResult {
            port: 8080,
            free_os: false,
            free_docker: true,
        };
        assert!(!r.is_free());
        let msg = r.error_message().unwrap();
        assert!(msg.contains("8080"));
        assert!(msg.contains("OS") || msg.contains("process"));
    }

    #[test]
    fn test_port_check_result_docker_busy() {
        let r = PortCheckResult {
            port: 3000,
            free_os: true,
            free_docker: false,
        };
        assert!(!r.is_free());
        let msg = r.error_message().unwrap();
        assert!(msg.contains("3000"));
        assert!(msg.contains("Docker") || msg.contains("container"));
    }

    #[test]
    fn test_mock_check_port_free() {
        let mut mock = MockPortCheckerPort::new();
        mock.expect_check_port().returning(|p| {
            Ok(PortCheckResult {
                port: p,
                free_os: true,
                free_docker: true,
            })
        });
        let result = mock.check_port(8080).unwrap();
        assert!(result.is_free());
    }

    #[test]
    fn test_mock_find_free_port() {
        let mut mock = MockPortCheckerPort::new();
        mock.expect_find_free_port()
            .returning(|start, _end| Ok(start));
        assert_eq!(mock.find_free_port(3000, 4000).unwrap(), 3000);
    }
}
