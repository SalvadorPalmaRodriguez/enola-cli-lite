use crate::domain::error::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ServiceState {
    Active,
    Inactive,
    Failed,
    Unknown,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ServiceMetrics {
    pub cpu_percent: f32,
    pub memory_bytes: u64,
}

#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait ServiceManagerPort {
    async fn start_service(&self, name: &str) -> Result<()>;
    async fn stop_service(&self, name: &str) -> Result<()>;
    async fn restart_service(&self, name: &str) -> Result<()>;
    async fn enable_service(&self, name: &str) -> Result<()>;
    async fn disable_service(&self, name: &str) -> Result<()>;
    async fn is_active(&self, name: &str) -> Result<bool>;
    async fn get_service_metrics(&self, name: &str) -> Result<ServiceMetrics>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_service_start_stop() {
        let mut mock = MockServiceManagerPort::new();
        mock.expect_start_service().returning(|_| Ok(()));
        mock.expect_stop_service().returning(|_| Ok(()));
        assert!(mock.start_service("nginx").await.is_ok());
        assert!(mock.stop_service("nginx").await.is_ok());
    }

    #[tokio::test]
    async fn test_mock_service_is_active() {
        let mut mock = MockServiceManagerPort::new();
        mock.expect_is_active().returning(|_| Ok(true));
        assert!(mock.is_active("tor").await.unwrap());
    }

    #[test]
    fn test_service_state_variants() {
        assert_eq!(ServiceState::Active, ServiceState::Active);
        assert_ne!(ServiceState::Active, ServiceState::Failed);
    }

    #[test]
    fn test_service_metrics_default() {
        let m = ServiceMetrics::default();
        assert_eq!(m.cpu_percent, 0.0);
        assert_eq!(m.memory_bytes, 0);
    }
}
