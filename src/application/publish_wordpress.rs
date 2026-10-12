// MCP-007 — publish_wordpress: publica un sitio WordPress como hidden service.
//
// Caso de uso tipado para el paso 4 del caso canónico MCP: devuelve
// `{"name","onion_address"}` estructurado (nada de cadenas formateadas).
// Reutiliza `ListWordPressSites` para el puerto del sitio y
// `DeployTorService` para el hidden service (80 → puerto del sitio).

use crate::application::deploy_tor_service::{DeployTorService, DeployTorServiceRequest};
use crate::application::list_wordpress_sites::ListWordPressSites;
use crate::domain::error::{EnolaError, Result};
use crate::ports::container::ContainerPort;
use crate::ports::manifest::ManifestPort;
use crate::ports::service::ServiceManagerPort;
use crate::ports::tor::TorManagerPort;
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
pub struct PublishWordPress {
    container: Arc<dyn ContainerPort + Send + Sync>,
    tor: Arc<dyn TorManagerPort + Send + Sync>,
    service_manager: Arc<dyn ServiceManagerPort + Send + Sync>,
    manifest: Arc<dyn ManifestPort + Send + Sync>,
}

impl PublishWordPress {
    /// Inyecta los ports del despliegue.
    pub fn new(
        container: Arc<dyn ContainerPort + Send + Sync>,
        tor: Arc<dyn TorManagerPort + Send + Sync>,
        service_manager: Arc<dyn ServiceManagerPort + Send + Sync>,
        manifest: Arc<dyn ManifestPort + Send + Sync>,
    ) -> Self {
        Self {
            container,
            tor,
            service_manager,
            manifest,
        }
    }

    /// Busca el puerto del sitio `name` y crea el hidden service.
    pub async fn execute(&self, name: &str) -> Result<PublishResult> {
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

        // Nombre del hidden service = nombre del sitio, consistente con
        // `wp create`/`wp publish` del CLI (ver commands::wordpress::publish).
        let onion_address = DeployTorService::new(
            self.tor.clone(),
            self.service_manager.clone(),
            self.manifest.clone(),
        )
        .execute(DeployTorServiceRequest {
            service_name: name.to_string(),
            ports: vec![(80, port)],
        })
        .await?;

        Ok(PublishResult {
            name: name.to_string(),
            onion_address,
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
        )
    }

    #[tokio::test]
    async fn publishes_site_and_returns_structured_onion() {
        let mut tor = MockTorManagerPort::new();
        tor.expect_deploy_hidden_service()
            .withf(|name, ports| name == "blog" && *ports == vec![(80u16, 8080u16)])
            .returning(|_, _| Ok("abc123.onion".to_string()));
        let r = ports_for(tor).execute("blog").await.unwrap();
        assert_eq!(r.name, "blog");
        assert_eq!(r.onion_address, "abc123.onion");
    }

    #[tokio::test]
    async fn unknown_site_returns_not_found() {
        let tor = MockTorManagerPort::new();
        let r = ports_for(tor).execute("ghost-site").await;
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
        )
        .execute("blog")
        .await;
        assert!(r.is_err());
    }
}
