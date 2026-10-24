use crate::domain::error::Result;
use crate::ports::tor::TorManagerPort;
use crate::ports::web::NginxManagerPort;
use std::sync::Arc;

/// Resultado de [`RemoveTorService::remove_published`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoveOutcome {
    /// Se eliminó el servicio canónico.
    Removed,
    /// El servicio no estaba publicado (nada que borrar).
    NotPublished,
    /// No estaba el canónico, pero sí un servicio de v0.9.0 con el nombre
    /// anterior: **no se borra** (podría ser otro servicio) y se devuelve el
    /// aviso común con el comando para eliminarlo.
    LegacyV09(String),
}

pub struct RemoveTorService {
    tor_manager: Arc<dyn TorManagerPort + Send + Sync>,
    nginx_manager: Option<Arc<dyn NginxManagerPort + Send + Sync>>,
}

impl RemoveTorService {
    pub fn new(
        tor_manager: Arc<dyn TorManagerPort + Send + Sync>,
        nginx_manager: Option<Arc<dyn NginxManagerPort + Send + Sync>>,
    ) -> Self {
        Self {
            tor_manager,
            nginx_manager,
        }
    }

    pub async fn execute(&self, service_name: &str) -> Result<()> {
        // Try to find the correct service name
        // Services can be named directly (raw) or with proxy_ prefix (web)
        let names_to_try = if service_name.starts_with("proxy_") {
            vec![service_name.to_string()]
        } else {
            vec![service_name.to_string(), format!("proxy_{}", service_name)]
        };

        // Sites Nginx candidatos: derivación canónica vía ServiceName
        // (`proxy_ai_dev` para `ai_dev`, `fileserver_docs` para FileServer,
        // `proxy_wp_blog` para `wp_blog`, ...) + variantes legacy.
        let mut sites_to_try: Vec<String> = Vec::new();
        if let Some(sn) = crate::domain::naming::ServiceName::from_tor_config(service_name) {
            if let Some(site) = sn.nginx_config_name() {
                sites_to_try.push(site);
            }
        }
        for n in &names_to_try {
            sites_to_try.push(n.clone());
            sites_to_try.push(format!("proxy_{}", n));
        }
        sites_to_try.sort();
        sites_to_try.dedup();

        let mut last_error = None;
        let mut tor_removed = false;

        // 1. Remove Nginx site config first (canonical site + legacy variants)
        if let Some(nginx) = &self.nginx_manager {
            for name in &sites_to_try {
                let _ = nginx.disable_site(name).await;
                let _ = nginx.delete_site_config(name).await;
            }
            let _ = nginx.reload().await;
        }

        // 2. Try to remove Tor service with each possible name
        for name in &names_to_try {
            match self.tor_manager.remove_hidden_service(name).await {
                Ok(_) => {
                    tor_removed = true;
                    break;
                }
                Err(e) => {
                    last_error = Some(e);
                }
            }
        }

        if !tor_removed {
            // Si en su lugar existe el servicio de v0.9.0 (nombre anterior),
            // el error dice cómo eliminarlo (sin compatibilidad desde v0.10.0).
            if let Ok(services) = self.tor_manager.list_hidden_services().await {
                let existing: Vec<&str> = services.iter().map(|s| s.name.as_str()).collect();
                if let Some((legacy, _)) =
                    crate::domain::naming::find_legacy_v09(service_name, &existing)
                {
                    return Err(crate::domain::error::EnolaError::NotFound(
                        crate::domain::naming::legacy_v09_hint(service_name, &legacy),
                    ));
                }
            }
            return Err(last_error.unwrap_or_else(|| {
                crate::domain::error::EnolaError::NotFound(format!(
                    "Service '{}' not found",
                    service_name
                ))
            }));
        }

        Ok(())
    }

    /// Borra **solo** el servicio canónico `canonical` (`wp_<n>`, ...) para la
    /// limpieza de un `delete`, sin probar otros nombres: un `proxy_<n>` o un
    /// `<n>` podrían ser servicios ajenos (`tor create`). Si falta el canónico
    /// pero hay uno de v0.9.0, lo indica en vez de borrarlo.
    pub async fn remove_published(&self, canonical: &str) -> RemoveOutcome {
        if self.execute(canonical).await.is_ok() {
            return RemoveOutcome::Removed;
        }
        let Ok(services) = self.tor_manager.list_hidden_services().await else {
            return RemoveOutcome::NotPublished;
        };
        let existing: Vec<&str> = services.iter().map(|s| s.name.as_str()).collect();
        match crate::domain::naming::find_legacy_v09(canonical, &existing) {
            Some((legacy, _)) => {
                RemoveOutcome::LegacyV09(crate::domain::naming::legacy_v09_hint(canonical, &legacy))
            }
            None => RemoveOutcome::NotPublished,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::error::EnolaError;
    use crate::ports::tor::TorServiceInfo;
    use crate::ports::web::{NginxFileServerConfig, NginxProxyConfig, NginxSiteConfig};
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicBool, Ordering};

    struct MockTorManager {
        remove_called: AtomicBool,
        should_fail: bool,
        listed: Vec<String>,
        /// Nombres que se intentaron borrar (en orden).
        removed: std::sync::Mutex<Vec<String>>,
    }

    impl MockTorManager {
        fn new() -> Self {
            Self {
                remove_called: AtomicBool::new(false),
                should_fail: false,
                listed: Vec::new(),
                removed: std::sync::Mutex::new(Vec::new()),
            }
        }
        fn failing() -> Self {
            Self {
                remove_called: AtomicBool::new(false),
                should_fail: true,
                listed: Vec::new(),
                removed: std::sync::Mutex::new(Vec::new()),
            }
        }
        /// Falla al borrar (el canónico no existe) y lista `services`.
        fn missing_with(services: &[&str]) -> Self {
            Self {
                remove_called: AtomicBool::new(false),
                should_fail: true,
                listed: services.iter().map(|s| s.to_string()).collect(),
                removed: std::sync::Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl TorManagerPort for MockTorManager {
        async fn list_hidden_services(&self) -> Result<Vec<TorServiceInfo>> {
            Ok(self
                .listed
                .iter()
                .map(|n| TorServiceInfo {
                    name: n.clone(),
                    hostname: format!("{n}.onion"),
                    hidden_service_dir: format!("/var/lib/tor/enola_{n}"),
                    ports: vec![],
                    clients: vec![],
                    active: true,
                    auth_enabled: false,
                })
                .collect())
        }
        async fn deploy_hidden_service(&self, _: &str, _: Vec<(u16, u16)>) -> Result<String> {
            Ok("test.onion".into())
        }
        async fn remove_hidden_service(&self, name: &str) -> Result<()> {
            self.remove_called.store(true, Ordering::SeqCst);
            self.removed.lock().unwrap().push(name.to_string());
            if self.should_fail {
                Err(EnolaError::InfrastructureError("Remove failed".into()))
            } else {
                Ok(())
            }
        }
        async fn get_onion_address(&self, _: &str) -> Result<String> {
            Ok("test.onion".into())
        }
        async fn reload_tor(&self) -> Result<()> {
            Ok(())
        }
        async fn generate_client_keys(&self, _: &str) -> Result<crate::ports::tor::ClientKeypair> {
            Ok(crate::ports::tor::ClientKeypair {
                public_key: "pub".into(),
                private_key: "priv".into(),
            })
        }
        async fn add_client_auth(&self, _: &str, _: &str, _: &str) -> Result<()> {
            Ok(())
        }
        async fn disable_client_auth(&self, _: &str) -> Result<()> {
            Ok(())
        }
        async fn revoke_client_auth(&self, _: &str, _: &str) -> Result<()> {
            Ok(())
        }
        async fn enable_client_auth(&self, _: &str) -> Result<()> {
            Ok(())
        }
        async fn stop_hidden_service(&self, _: &str) -> Result<()> {
            Ok(())
        }
        async fn start_hidden_service(&self, _: &str) -> Result<()> {
            Ok(())
        }
        async fn rotate_hidden_service_identity(&self, _: &str) -> Result<String> {
            Ok("new.onion".into())
        }
    }

    struct MockNginxManager {
        delete_called: AtomicBool,
    }

    impl MockNginxManager {
        fn new() -> Self {
            Self {
                delete_called: AtomicBool::new(false),
            }
        }
    }

    #[async_trait]
    impl NginxManagerPort for MockNginxManager {
        async fn create_site_config(&self, _: NginxSiteConfig) -> Result<()> {
            Ok(())
        }
        async fn create_fileserver_config(&self, _: NginxFileServerConfig) -> Result<()> {
            Ok(())
        }
        async fn create_proxy_config(&self, _: NginxProxyConfig) -> Result<()> {
            Ok(())
        }
        async fn enable_site(&self, _: &str) -> Result<()> {
            Ok(())
        }
        async fn disable_site(&self, _: &str) -> Result<()> {
            Ok(())
        }
        async fn delete_site_config(&self, _: &str) -> Result<()> {
            self.delete_called.store(true, Ordering::SeqCst);
            Ok(())
        }
        async fn validate_config(&self) -> Result<bool> {
            Ok(true)
        }
        async fn reload(&self) -> Result<()> {
            Ok(())
        }
        async fn update_proxy_ports(&self, _: &str, _: u16, _: u16) -> Result<()> {
            Ok(())
        }
        async fn list_enabled_sites(&self) -> Result<Vec<String>> {
            Ok(vec![])
        }

        async fn create_proxy_config_with_ssl(
            &self,
            _config: crate::ports::web::NginxProxyConfigWithSsl,
        ) -> Result<()> {
            Ok(())
        }

        async fn generate_self_signed_cert(&self, _service_name: &str) -> Result<(String, String)> {
            Ok(("cert.pem".to_string(), "key.pem".to_string()))
        }

        async fn update_proxy_ports_with_ssl(
            &self,
            _domain: &str,
            _http_listen_port: u16,
            _https_listen_port: Option<u16>,
            _backend_port: u16,
        ) -> Result<()> {
            Ok(())
        }

        async fn find_available_port(&self, _range_start: u16, _range_end: u16) -> Result<u16> {
            Ok(8080)
        }

        async fn is_port_available(&self, _port: u16) -> bool {
            true
        }

        async fn create_fileserver_config_with_ssl(
            &self,
            _config: crate::ports::web::NginxFileServerConfigWithSsl,
        ) -> Result<()> {
            Ok(())
        }

        async fn detect_ssl_config(
            &self,
            _domain: &str,
        ) -> Result<(bool, Option<u16>, Option<String>)> {
            Ok((false, None, None))
        }
    }

    #[tokio::test]
    async fn test_remove_tor_service_success() {
        let tor = Arc::new(MockTorManager::new());
        let use_case = RemoveTorService::new(tor.clone(), None);

        let result = use_case.execute("myservice").await;

        assert!(result.is_ok());
        assert!(tor.remove_called.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn test_remove_tor_service_with_nginx() {
        let tor = Arc::new(MockTorManager::new());
        let nginx = Arc::new(MockNginxManager::new());
        let use_case = RemoveTorService::new(tor.clone(), Some(nginx.clone()));

        let result = use_case.execute("myservice").await;

        assert!(result.is_ok());
        assert!(tor.remove_called.load(Ordering::SeqCst));
        assert!(nginx.delete_called.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn test_remove_tor_service_tor_failure() {
        let tor = Arc::new(MockTorManager::failing());
        let use_case = RemoveTorService::new(tor, None);

        let result = use_case.execute("myservice").await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn remove_reports_v09_service_when_canonical_is_missing() {
        // `drupal hide sitio` busca `drupal_sitio`; si solo existe el de
        // v0.9.0 (`proxy_drupal-sitio`), el error dice cómo eliminarlo.
        let tor = Arc::new(MockTorManager::missing_with(&["proxy_drupal-sitio"]));
        let use_case = RemoveTorService::new(tor, None);
        let err = use_case
            .execute("drupal_sitio")
            .await
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("sudo enola-cli tor remove proxy_drupal-sitio"),
            "{err}"
        );
    }

    #[tokio::test]
    async fn remove_keeps_original_error_without_v09_service() {
        let tor = Arc::new(MockTorManager::missing_with(&["git_otro"]));
        let use_case = RemoveTorService::new(tor, None);
        let err = use_case
            .execute("drupal_sitio")
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("Remove failed"), "{err}");
    }

    // ── remove_published (`wp delete` y similares) ──

    #[tokio::test]
    async fn remove_published_removes_canonical_service() {
        let tor = Arc::new(MockTorManager::new());
        let use_case = RemoveTorService::new(tor.clone(), None);
        assert_eq!(
            use_case.remove_published("wp_blog").await,
            RemoveOutcome::Removed
        );
        assert_eq!(
            tor.removed.lock().unwrap().first().map(String::as_str),
            Some("wp_blog")
        );
    }

    #[tokio::test]
    async fn remove_published_never_touches_other_names() {
        // `proxy_blog` puede ser un `tor create web blog` sin relación: solo
        // se intenta el canónico (y su site), nunca `blog`/`proxy_blog`.
        let tor = Arc::new(MockTorManager::missing_with(&["proxy_blog", "blog"]));
        let use_case = RemoveTorService::new(tor.clone(), None);
        let outcome = use_case.remove_published("wp_blog").await;
        let removed = tor.removed.lock().unwrap().clone();
        assert!(
            removed
                .iter()
                .all(|n| n == "wp_blog" || n == "proxy_wp_blog"),
            "{removed:?}"
        );
        // Y avisa del posible servicio de v0.9.0 con el mensaje común.
        match outcome {
            RemoveOutcome::LegacyV09(hint) => {
                assert!(
                    hint.contains("sudo enola-cli tor remove proxy_blog"),
                    "{hint}"
                )
            }
            other => panic!("se esperaba LegacyV09, no {other:?}"),
        }
    }

    #[tokio::test]
    async fn remove_published_reports_not_published() {
        let tor = Arc::new(MockTorManager::missing_with(&["git_otro"]));
        let use_case = RemoveTorService::new(tor, None);
        assert_eq!(
            use_case.remove_published("wp_blog").await,
            RemoveOutcome::NotPublished
        );
    }
}
