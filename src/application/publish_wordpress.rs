// MCP-007 — publish_wordpress: publica un sitio WordPress como hidden service.
//
// Caso de uso tipado para el paso 4 del caso canónico MCP: devuelve
// `{"name","onion_address"}` estructurado (nada de cadenas formateadas).
// Reutiliza `ListWordPressSites` para el puerto del sitio y
// `DeployTorService` para el hidden service (80 → puerto del sitio).

use crate::application::list_wordpress_sites::ListWordPressSites;
use crate::application::publish_tor_web_service::{
    PublishTorWebService, PublishTorWebServiceRequest,
};
use crate::domain::error::{EnolaError, Result};
use crate::domain::naming::ServiceType;
use crate::ports::container::ContainerPort;
use crate::ports::manifest::ManifestPort;
use crate::ports::service::ServiceManagerPort;
use crate::ports::tor::TorManagerPort;
use crate::ports::web::NginxManagerPort;
use serde::Serialize;
use std::sync::Arc;

/// Resultado estructurado de la publicación.
#[derive(Debug, Clone, Serialize)]
pub struct PublishResult {
    /// Nombre del sitio publicado.
    pub name: String,
    /// Dirección .onion asignada al hidden service.
    pub onion_address: String,
}

/// Publica un WordPress existente en Tor (crea el hidden service 80→site).
/// MEGAPLAN-PUBLISH: vía `PublishTorWebService` (canonical `wp_<name>`,
/// Nginx proxy, `--ssl`/`--auth` en paridad con el CLI).
pub struct PublishWordPress {
    container: Arc<dyn ContainerPort + Send + Sync>,
    tor: Arc<dyn TorManagerPort + Send + Sync>,
    #[allow(dead_code)]
    service_manager: Arc<dyn ServiceManagerPort + Send + Sync>,
    manifest: Arc<dyn ManifestPort + Send + Sync>,
    nginx_manager: Arc<dyn NginxManagerPort + Send + Sync>,
}

impl PublishWordPress {
    /// Inyecta los ports del despliegue.
    pub fn new(
        container: Arc<dyn ContainerPort + Send + Sync>,
        tor: Arc<dyn TorManagerPort + Send + Sync>,
        service_manager: Arc<dyn ServiceManagerPort + Send + Sync>,
        manifest: Arc<dyn ManifestPort + Send + Sync>,
        nginx_manager: Arc<dyn NginxManagerPort + Send + Sync>,
    ) -> Self {
        Self {
            container,
            tor,
            service_manager,
            manifest,
            nginx_manager,
        }
    }

    /// Busca el puerto del sitio `name` y publica vía el caso de uso unificado.
    /// `ssl`/`auth` en paridad con `wp publish --ssl --auth` del CLI (D13/D14).
    pub async fn execute(&self, name: &str, ssl: bool, auth: bool) -> Result<PublishResult> {
        let sites = ListWordPressSites::new(self.container.clone())
            .execute()
            .await?;
        let site = sites
            .iter()
            .find(|s| s.name == name)
            .ok_or_else(|| EnolaError::NotFound(format!("WordPress site '{name}' not found")))?;
        let port = site.port.ok_or_else(|| {
            EnolaError::ValidationError(format!(
                "site '{name}' has no mapped port (is it running?)"
            ))
        })?;

        let (http_port, https_port) = if ssl {
            (
                self.nginx_manager.find_available_port(10000, 15000).await?,
                Some(self.nginx_manager.find_available_port(15001, 20000).await?),
            )
        } else {
            (
                self.nginx_manager.find_available_port(10000, 15000).await?,
                None,
            )
        };

        // Nombre canónico `wp_<name>` (ServiceName::new + tor_config_name).
        let name_canonical =
            crate::domain::naming::ServiceName::new(name, ServiceType::WordPress).tor_config_name();
        let res = PublishTorWebService::new(
            self.nginx_manager.clone(),
            self.tor.clone(),
            self.manifest.clone(),
        )
        .execute(PublishTorWebServiceRequest {
            service_kind: ServiceType::WordPress,
            name_canonical,
            backend_port: port,
            http_port,
            https_port,
            extra_ports: vec![],
            ssl,
            auth,
            root_dir: None,
        })
        .await?;

        Ok(PublishResult {
            name: name.to_string(),
            onion_address: res.onion_address,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::container::{ContainerInfo, MockContainerPort};
    use crate::ports::manifest::MockManifestPort;
    use crate::ports::service::MockServiceManagerPort;
    use crate::ports::tor::MockTorManagerPort;

    fn nginx_for_publish() -> crate::ports::web::MockNginxManagerPort {
        let mut nginx = crate::ports::web::MockNginxManagerPort::new();
        nginx
            .expect_find_available_port()
            .returning(|_, _| Ok(10081));
        nginx.expect_create_proxy_config().returning(|_| Ok(()));
        nginx.expect_validate_config().returning(|| Ok(true));
        nginx.expect_enable_site().returning(|_| Ok(()));
        nginx.expect_reload().returning(|| Ok(()));
        nginx
    }

    fn site_container(name: &str, port: &str) -> ContainerInfo {
        ContainerInfo {
            id: "id".into(),
            name: name.into(),
            image: "img".into(),
            status: "Up".into(),
            ports: vec![port.to_string()],
        }
    }

    fn ports_for(tor: MockTorManagerPort) -> PublishWordPress {
        let mut container = MockContainerPort::new();
        container.expect_list_containers().returning(|_| {
            Ok(vec![
                site_container("wp-blog", "127.0.0.1:8080->80/tcp"),
                site_container("db-blog", "3306/tcp"),
            ])
        });
        let mut sm = MockServiceManagerPort::new();
        sm.expect_is_active().returning(|_| Ok(true));
        let mut manifest = MockManifestPort::new();
        manifest.expect_append().returning(|_, _| Ok(())).times(0..);
        PublishWordPress::new(
            Arc::new(container),
            Arc::new(tor),
            Arc::new(sm),
            Arc::new(manifest),
            Arc::new(nginx_for_publish()),
        )
    }

    #[tokio::test]
    async fn publishes_site_and_returns_structured_onion() {
        let mut tor = MockTorManagerPort::new();
        // Sin servicios de v0.9.0 con el nombre anterior.
        tor.expect_list_hidden_services().returning(|| Ok(vec![]));
        tor.expect_deploy_hidden_service()
            .withf(|name, ports| name == "wp_blog" && *ports == vec![(80u16, 10081u16)])
            .returning(|_, _| Ok("abc123.onion".to_string()));
        tor.expect_reload_tor().returning(|| Ok(()));
        let r = ports_for(tor).execute("blog", false, false).await.unwrap();
        assert_eq!(r.name, "blog");
        assert_eq!(r.onion_address, "abc123.onion");
    }

    #[tokio::test]
    async fn unknown_site_returns_not_found() {
        let tor = MockTorManagerPort::new();
        let r = ports_for(tor).execute("ghost-site", false, false).await;
        assert!(r.is_err());
    }

    #[tokio::test]
    async fn site_without_port_is_rejected() {
        let mut container = MockContainerPort::new();
        container
            .expect_list_containers()
            .returning(|_| Ok(vec![site_container("wp-blog", "")]));
        let tor = MockTorManagerPort::new();
        let mut sm = MockServiceManagerPort::new();
        sm.expect_is_active().returning(|_| Ok(true));
        let mut manifest = MockManifestPort::new();
        manifest.expect_append().returning(|_, _| Ok(())).times(0..);
        let r = PublishWordPress::new(
            Arc::new(container),
            Arc::new(tor),
            Arc::new(sm),
            Arc::new(manifest),
            Arc::new(nginx_for_publish()),
        )
        .execute("blog", false, false)
        .await;
        assert!(r.is_err());
    }
}
