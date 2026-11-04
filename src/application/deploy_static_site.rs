use crate::domain::error::{EnolaError, Result};
use crate::ports::manifest::ManifestPort;
use crate::ports::web::{NginxManagerPort, NginxSiteConfig};
use std::sync::Arc;

pub struct DeployStaticSite {
    nginx_manager: Arc<dyn NginxManagerPort + Send + Sync>,
    manifest: Arc<dyn ManifestPort + Send + Sync>,
}

impl DeployStaticSite {
    pub fn new(
        nginx_manager: Arc<dyn NginxManagerPort + Send + Sync>,
        manifest: Arc<dyn ManifestPort + Send + Sync>,
    ) -> Self {
        Self {
            nginx_manager,
            manifest,
        }
    }

    /// Publica el site estático. Primero crea el directorio raíz (vacío,
    /// `root:www-data` 750): si ya existe, se niega **sin tocar nada más**
    /// (Nginx, Tor ni el directorio ajeno). Si un paso posterior falla, retira
    /// el directorio vacío que acaba de crear para que reintentar con el mismo
    /// nombre no choque con su propio resto.
    pub async fn execute(&self, domain: &str, root_dir: &str, port: u16) -> Result<()> {
        self.nginx_manager.create_site_docroot(root_dir).await?;
        if let Err(e) = self.configure_site(domain, root_dir, port).await {
            let _ = self.nginx_manager.remove_empty_site_docroot(root_dir).await;
            return Err(e);
        }
        Ok(())
    }

    async fn configure_site(&self, domain: &str, root_dir: &str, port: u16) -> Result<()> {
        // 1. Create Config
        let config = NginxSiteConfig {
            domain: domain.to_string(),
            listen_port: port,
            root_dir: root_dir.to_string(),
            index_files: vec!["index.html".to_string(), "index.htm".to_string()],
            autoindex: false,
        };

        self.nginx_manager.create_site_config(config).await?;
        let _ = self.manifest.append("nginx_config", domain);

        // 2. Validate
        if !self.nginx_manager.validate_config().await? {
            return Err(EnolaError::InfrastructureError(
                "Generated Nginx config is invalid".to_string(),
            ));
        }

        // 3. Enable
        self.nginx_manager.enable_site(domain).await?;

        // 4. Reload
        self.nginx_manager.reload().await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::manifest::MockManifestPort;
    use crate::ports::web::MockNginxManagerPort;

    fn mock_manifest() -> MockManifestPort {
        let mut m = MockManifestPort::new();
        m.expect_append().returning(|_, _| Ok(())).times(0..);
        m.expect_remove().returning(|_, _| Ok(())).times(0..);
        m
    }

    #[tokio::test]
    async fn test_deploy_static_site_success() {
        let mut mock = MockNginxManagerPort::new();
        mock.expect_create_site_docroot().returning(|_| Ok(()));
        mock.expect_create_site_config().returning(|_| Ok(()));
        mock.expect_validate_config().returning(|| Ok(true));
        mock.expect_enable_site().returning(|_| Ok(()));
        mock.expect_reload().returning(|| Ok(()));

        let deployer = DeployStaticSite::new(Arc::new(mock), Arc::new(mock_manifest()));
        let result = deployer
            .execute("example.com", "/var/www/example", 8080)
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_deploy_static_site_invalid_config_returns_error() {
        let mut mock = MockNginxManagerPort::new();
        mock.expect_create_site_docroot().returning(|_| Ok(()));
        mock.expect_remove_empty_site_docroot()
            .times(1)
            .returning(|p| Ok(crate::ports::web::DocRootCleanup::Removed(p.to_string())));
        mock.expect_create_site_config().returning(|_| Ok(()));
        mock.expect_validate_config().returning(|| Ok(false)); // Invalid config

        let deployer = DeployStaticSite::new(Arc::new(mock), Arc::new(mock_manifest()));
        let result = deployer.execute("bad.com", "/var/www/bad", 9090).await;
        assert!(result.is_err(), "Invalid nginx config should return error");
    }

    #[tokio::test]
    async fn test_deploy_static_site_create_config_failure() {
        let mut mock = MockNginxManagerPort::new();
        mock.expect_create_site_docroot().returning(|_| Ok(()));
        // El directorio recién creado se retira al fallar un paso posterior.
        mock.expect_remove_empty_site_docroot()
            .times(1)
            .returning(|p| Ok(crate::ports::web::DocRootCleanup::Removed(p.to_string())));
        mock.expect_create_site_config().returning(|_| {
            Err(crate::domain::error::EnolaError::InfrastructureError(
                "write failed".into(),
            ))
        });

        let deployer = DeployStaticSite::new(Arc::new(mock), Arc::new(mock_manifest()));
        let result = deployer.execute("fail.com", "/var/www/fail", 8080).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn existing_docroot_refuses_and_touches_nothing_else() {
        let mut mock = MockNginxManagerPort::new();
        mock.expect_create_site_docroot().times(1).returning(|p| {
            Err(crate::domain::error::EnolaError::ValidationError(format!(
                "'{p}' ya existe"
            )))
        });
        // Sin expectativas para create_site_config / enable_site / reload /
        // remove_empty_site_docroot: si se llamaran, mockall haría panic.
        let deployer = DeployStaticSite::new(Arc::new(mock), Arc::new(mock_manifest()));
        let result = deployer.execute("html", "/var/www/html", 8080).await;
        assert!(matches!(
            result,
            Err(crate::domain::error::EnolaError::ValidationError(_))
        ));
    }

    #[tokio::test]
    async fn docroot_is_created_before_nginx_config() {
        let mut seq = mockall::Sequence::new();
        let mut mock = MockNginxManagerPort::new();
        mock.expect_create_site_docroot()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_| Ok(()));
        mock.expect_create_site_config()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_| Ok(()));
        mock.expect_validate_config().returning(|| Ok(true));
        mock.expect_enable_site().returning(|_| Ok(()));
        mock.expect_reload().returning(|| Ok(()));
        let deployer = DeployStaticSite::new(Arc::new(mock), Arc::new(mock_manifest()));
        assert!(deployer
            .execute("sitio", "/var/www/sitio", 8080)
            .await
            .is_ok());
    }
}
