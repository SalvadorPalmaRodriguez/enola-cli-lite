use crate::domain::error::Result;
use crate::domain::port_config::ServicePortConfig;
use async_trait::async_trait;

#[derive(Debug, Clone)]
pub struct PortConsistencyReport {
    pub service_name: String,
    pub is_consistent: bool,
    pub tor_status: ComponentStatus,
    pub nginx_status: ComponentStatus,
    pub backend_status: ComponentStatus,
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComponentStatus {
    Ok,
    ConfigMismatch,
    PortClosed,
    NotConfigured,
    Unknown,
    Error(String),
}

/// Trait para servicios que pueden configurar/editar puertos
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait PortConfigurable: Send + Sync {
    /// Obtener configuración actual de puertos
    async fn get_port_config(&self, service_name: &str) -> Result<ServicePortConfig>;

    /// Actualizar configuración de puertos
    /// Esta operación debe ser atómica y coordinar Tor, Nginx y el servicio Backend
    async fn update_port_config(&self, config: ServicePortConfig) -> Result<()>;

    /// Validar consistencia entre Tor, Nginx y backend
    async fn validate_port_consistency(&self, service_name: &str) -> Result<PortConsistencyReport>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_port_configurable_get() {
        let mut mock = MockPortConfigurable::new();
        mock.expect_get_port_config().returning(|_| {
            Ok(ServicePortConfig::new(
                "test",
                crate::domain::port_config::ServiceType::Web,
            ))
        });
        let config = mock.get_port_config("test").await.unwrap();
        assert_eq!(config.service_name, "test");
    }

    #[tokio::test]
    async fn test_mock_port_configurable_validate() {
        let mut mock = MockPortConfigurable::new();
        mock.expect_validate_port_consistency().returning(|_| {
            Ok(PortConsistencyReport {
                service_name: "svc".into(),
                is_consistent: true,
                tor_status: ComponentStatus::Ok,
                nginx_status: ComponentStatus::Ok,
                backend_status: ComponentStatus::Ok,
                issues: vec![],
            })
        });
        let report = mock.validate_port_consistency("svc").await.unwrap();
        assert!(report.is_consistent);
        assert!(report.issues.is_empty());
    }

    #[test]
    fn test_component_status_eq() {
        assert_eq!(ComponentStatus::Ok, ComponentStatus::Ok);
        assert_ne!(ComponentStatus::Ok, ComponentStatus::PortClosed);
    }
}
