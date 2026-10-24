use crate::application::deploy_tor_service::{DeployTorService, DeployTorServiceRequest};
use crate::domain::error::{EnolaError, Result};
use crate::ports::manifest::ManifestPort;
use crate::ports::service::ServiceManagerPort;
use crate::ports::tor::TorManagerPort;
use std::sync::Arc;

pub struct DeploySshHiddenService {
    tor_manager: Arc<dyn TorManagerPort + Send + Sync>,
    service_manager: Arc<dyn ServiceManagerPort + Send + Sync>,
    manifest: Arc<dyn ManifestPort + Send + Sync>,
}

impl DeploySshHiddenService {
    pub fn new(
        tor_manager: Arc<dyn TorManagerPort + Send + Sync>,
        service_manager: Arc<dyn ServiceManagerPort + Send + Sync>,
        manifest: Arc<dyn ManifestPort + Send + Sync>,
    ) -> Self {
        Self {
            tor_manager,
            service_manager,
            manifest,
        }
    }

    pub async fn execute(
        &self,
        service_name: &str,
        onion_port: u16,
        local_ssh_port: u16,
    ) -> Result<String> {
        // 0. Validate service name — se interpola en rutas/config de Tor
        // vía DeployTorService (R-12; defensa en profundidad aunque el
        // caso de uso delegado también valida).
        // Es el nombre de servicio canónico (`<dominio>_<name>`): admite `_`.
        crate::domain::naming::validate_tor_service_name(service_name)?;

        // 1. Ensure SSH Server is running on localhost
        // Try 'ssh' or 'sshd'. Ubuntu usually uses 'ssh'.
        if !self.service_manager.is_active("ssh").await? {
            // Try to start it
            self.service_manager
                .start_service("ssh")
                .await
                .map_err(|_| {
                    EnolaError::InfrastructureError(
                        "Failed to start SSH service on host".to_string(),
                    )
                })?;
        }

        // 2. Deploy Hidden Service mapping onion_port -> local_ssh_port
        let deploy_tor = DeployTorService::new(
            self.tor_manager.clone(),
            self.service_manager.clone(),
            self.manifest.clone(),
        );

        let request = DeployTorServiceRequest {
            service_name: service_name.to_string(),
            ports: vec![(onion_port, local_ssh_port)],
        };

        let onion = deploy_tor.execute(request).await?;

        Ok(onion)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::manifest::MockManifestPort;
    use crate::ports::service::MockServiceManagerPort;
    use crate::ports::tor::MockTorManagerPort;
    use mockall::predicate::*;

    fn mock_manifest() -> MockManifestPort {
        let mut m = MockManifestPort::new();
        m.expect_append().returning(|_, _| Ok(())).times(0..);
        m.expect_remove().returning(|_, _| Ok(())).times(0..);
        m
    }

    #[tokio::test]
    async fn test_deploy_ssh_success() {
        let mut mock_tor = MockTorManagerPort::new();
        let mut mock_service = MockServiceManagerPort::new();

        // Expect SSH check
        mock_service
            .expect_is_active()
            .with(eq("ssh"))
            .times(1)
            .returning(|_| Ok(true));

        // Expect Tor Service check (called by DeployTorService)
        mock_service
            .expect_is_active()
            .with(eq("tor"))
            .times(1)
            .returning(|_| Ok(true));

        // Expect Deploy
        mock_tor
            .expect_deploy_hidden_service()
            .with(eq("my-ssh"), eq(vec![(22, 2222)]))
            .times(1)
            .returning(|_, _| Ok("ssh_onion.onion".to_string()));

        let use_case = DeploySshHiddenService::new(
            Arc::new(mock_tor),
            Arc::new(mock_service),
            Arc::new(mock_manifest()),
        );

        let result = use_case.execute("my-ssh", 22, 2222).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "ssh_onion.onion");
    }

    #[tokio::test]
    async fn test_deploy_ssh_starts_service() {
        let mut mock_tor = MockTorManagerPort::new();
        let mut mock_service = MockServiceManagerPort::new();

        // SSH not active, should start
        mock_service
            .expect_is_active()
            .with(eq("ssh"))
            .times(1)
            .returning(|_| Ok(false));

        mock_service
            .expect_start_service()
            .with(eq("ssh"))
            .times(1)
            .returning(|_| Ok(()));

        // Tor check
        mock_service
            .expect_is_active()
            .with(eq("tor"))
            .returning(|_| Ok(true));

        mock_tor
            .expect_deploy_hidden_service()
            .returning(|_, _| Ok("onion".to_string()));

        let use_case = DeploySshHiddenService::new(
            Arc::new(mock_tor),
            Arc::new(mock_service),
            Arc::new(mock_manifest()),
        );

        let result = use_case.execute("test", 22, 22).await;
        assert!(result.is_ok());
    }

    /// R-12: el nombre se interpola en rutas/config Tor vía DeployTorService —
    /// el caso de uso valida también en su entrada (defensa en profundidad).
    #[tokio::test]
    async fn test_deploy_ssh_hidden_rejects_unsafe_names() {
        for name in ["../x", "/etc/ssh/sshd_config.d/00-x", "a\nb", ""] {
            let use_case = DeploySshHiddenService::new(
                Arc::new(MockTorManagerPort::new()),
                Arc::new(MockServiceManagerPort::new()),
                Arc::new(mock_manifest()),
            );
            assert!(
                use_case.execute(name, 22, 22).await.is_err(),
                "name {name:?} debe ser rechazado"
            );
        }
    }
}
