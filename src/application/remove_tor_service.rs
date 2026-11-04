use crate::domain::error::{EnolaError, Result};
use crate::ports::tor::TorManagerPort;
use crate::ports::web::{DocRootCleanup, NginxManagerPort};
use std::sync::Arc;

/// Resultado de [`RemoveTorService::remove_published`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoveOutcome {
    /// Se eliminó el servicio canónico.
    Removed,
    /// El servicio no estaba publicado (nada que borrar).
    NotPublished,
}

/// Retira un servicio Tor publicado: exactamente lo que creó su publicación.
pub struct RemoveTorService {
    tor_manager: Arc<dyn TorManagerPort + Send + Sync>,
    nginx_manager: Option<Arc<dyn NginxManagerPort + Send + Sync>>,
}

impl RemoveTorService {
    /// Sin `nginx_manager` solo se quita el servicio Tor (lo usan los `edit`
    /// que republican: conservan site y certificado).
    pub fn new(
        tor_manager: Arc<dyn TorManagerPort + Send + Sync>,
        nginx_manager: Option<Arc<dyn NginxManagerPort + Send + Sync>>,
    ) -> Self {
        Self {
            tor_manager,
            nginx_manager,
        }
    }

    /// Quita el servicio Tor `service_name` (nombre exacto, sin variantes) y,
    /// con `nginx_manager`, su site y su certificado `<service_name>.{crt,key}`.
    /// El site es el canónico del tipo (`proxy_wp_<n>` para `wp_<n>`,
    /// `fileserver_<n>` para `fileserver_<n>`; ninguno para `raw_`/`ai_`…);
    /// un nombre sin prefijo de tipo (`tor create raw|static <n>`) usa el site
    /// `<n>`. El certificado se borra también si el servicio Tor ya no existía
    /// (`NotFound`, que se devuelve igualmente); ante otros errores de Tor no se toca.
    ///
    /// Un nombre sin prefijo de tipo (`tor create static <n>`) puede tener un
    /// directorio raíz propio (`/var/www/<n>`): se elimina **solo si está
    /// vacío**; con ficheros del usuario se conserva y se avisa de dónde queda.
    pub async fn execute(&self, service_name: &str) -> Result<()> {
        let typed = crate::domain::naming::ServiceName::from_tor_config(service_name);
        let site = match &typed {
            Some(sn) => sn.nginx_config_name(),
            None => Some(service_name.to_string()),
        };

        // El `root` se lee ANTES de borrar el site (es de donde sale).
        let docroot = match (&self.nginx_manager, &site, &typed) {
            (Some(nginx), Some(site), None) => nginx.site_root_dir(site).await.unwrap_or(None),
            _ => None,
        };

        if let (Some(nginx), Some(site)) = (&self.nginx_manager, &site) {
            let _ = nginx.disable_site(site).await;
            let _ = nginx.delete_site_config(site).await;
            let _ = nginx.reload().await;
        }

        let removed = self.tor_manager.remove_hidden_service(service_name).await;
        match removed {
            Ok(()) => {}
            Err(EnolaError::NotFound(msg)) => {
                // El servicio Tor ya no existe pero el certificado propio puede
                // haber quedado: se borra igualmente y se devuelve el NotFound.
                if let Some(nginx) = &self.nginx_manager {
                    nginx.delete_ssl_cert(service_name).await?;
                }
                self.cleanup_docroot(docroot.as_deref()).await;
                return Err(EnolaError::NotFound(msg));
            }
            Err(e) => return Err(e),
        }

        if let Some(nginx) = &self.nginx_manager {
            nginx.delete_ssl_cert(service_name).await?;
        }
        self.cleanup_docroot(docroot.as_deref()).await;
        Ok(())
    }

    /// Retira el directorio raíz del site si está vacío y avisa del resultado.
    /// Nunca falla la retirada del servicio por esto (ya está hecha).
    async fn cleanup_docroot(&self, docroot: Option<&str>) {
        let (Some(nginx), Some(root)) = (&self.nginx_manager, docroot) else {
            return;
        };
        match nginx.remove_empty_site_docroot(root).await {
            Ok(DocRootCleanup::Removed(path)) => {
                eprintln!("🗑️  Directorio vacío eliminado: {path}");
            }
            Ok(DocRootCleanup::KeptNotEmpty(path)) => {
                tracing::warn!("Directorio del sitio NO borrado (tiene ficheros): {path}");
                eprintln!(
                    "⚠️  El directorio {path} contiene ficheros y se conserva. Revísalo y elimínalo a mano si procede."
                );
            }
            Ok(DocRootCleanup::Skipped { path, reason }) => {
                tracing::warn!("Directorio del sitio NO borrado: {path}: {reason}");
                eprintln!(
                    "⚠️  Directorio del sitio NO borrado ({path}): {reason}. Solo se borran directorios vacíos bajo /var/www/."
                );
            }
            Ok(DocRootCleanup::NotApplicable) => {}
            Err(e) => {
                tracing::warn!("No se pudo retirar el directorio del sitio {root}: {e}");
                eprintln!("⚠️  No se pudo retirar el directorio {root}: {e}");
            }
        }
    }

    /// Borra **solo** el servicio canónico `canonical` (`wp_<n>`, ...) para la
    /// limpieza de un `delete`, sin probar otros nombres: un `proxy_<n>` o un
    /// `<n>` podrían ser servicios ajenos (`tor create`).
    pub async fn remove_published(&self, canonical: &str) -> RemoveOutcome {
        match self.execute(canonical).await {
            Ok(()) => RemoveOutcome::Removed,
            Err(_) => RemoveOutcome::NotPublished,
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
        /// Si falla, lo hace con `NotFound` en vez de error de infraestructura.
        not_found: bool,
        listed: Vec<String>,
        /// Nombres que se intentaron borrar (en orden).
        removed: std::sync::Mutex<Vec<String>>,
    }

    impl MockTorManager {
        fn new() -> Self {
            Self {
                remove_called: AtomicBool::new(false),
                should_fail: false,
                not_found: false,
                listed: Vec::new(),
                removed: std::sync::Mutex::new(Vec::new()),
            }
        }
        fn not_found() -> Self {
            Self {
                not_found: true,
                ..Self::failing()
            }
        }
        fn failing() -> Self {
            Self {
                remove_called: AtomicBool::new(false),
                should_fail: true,
                not_found: false,
                listed: Vec::new(),
                removed: std::sync::Mutex::new(Vec::new()),
            }
        }
        /// Falla al borrar (el canónico no existe) y lista `services`.
        fn missing_with(services: &[&str]) -> Self {
            Self {
                remove_called: AtomicBool::new(false),
                should_fail: true,
                not_found: false,
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
            if self.should_fail && self.not_found {
                Err(EnolaError::NotFound("no existe".into()))
            } else if self.should_fail {
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
        deleted_sites: std::sync::Mutex<Vec<String>>,
        deleted_certs: std::sync::Mutex<Vec<String>>,
        /// `root` que declara el site (lo que devuelve `site_root_dir`).
        site_root: Option<String>,
        /// Directorios raíz que se pidió retirar (en orden).
        docroot_cleanups: std::sync::Mutex<Vec<String>>,
    }

    impl MockNginxManager {
        fn new() -> Self {
            Self {
                delete_called: AtomicBool::new(false),
                deleted_sites: std::sync::Mutex::new(Vec::new()),
                deleted_certs: std::sync::Mutex::new(Vec::new()),
                site_root: None,
                docroot_cleanups: std::sync::Mutex::new(Vec::new()),
            }
        }
        fn with_root(root: &str) -> Self {
            Self {
                site_root: Some(root.to_string()),
                ..Self::new()
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
        async fn delete_ssl_cert(&self, name: &str) -> Result<()> {
            self.deleted_certs.lock().unwrap().push(name.to_string());
            Ok(())
        }
        async fn delete_site_config(&self, name: &str) -> Result<()> {
            self.delete_called.store(true, Ordering::SeqCst);
            self.deleted_sites.lock().unwrap().push(name.to_string());
            Ok(())
        }
        async fn validate_config(&self) -> Result<bool> {
            Ok(true)
        }
        async fn site_root_dir(&self, _: &str) -> Result<Option<String>> {
            Ok(self.site_root.clone())
        }
        async fn remove_empty_site_docroot(&self, root: &str) -> Result<DocRootCleanup> {
            self.docroot_cleanups.lock().unwrap().push(root.to_string());
            Ok(DocRootCleanup::Removed(root.to_string()))
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
        // Sin prefijo de tipo (`tor create static myservice`): site `myservice`.
        assert_eq!(
            *nginx.deleted_sites.lock().unwrap(),
            vec!["myservice".to_string()]
        );
        assert_eq!(
            *nginx.deleted_certs.lock().unwrap(),
            vec!["myservice".to_string()]
        );
    }

    #[tokio::test]
    async fn bare_static_service_cleans_its_docroot() {
        let tor = Arc::new(MockTorManager::new());
        let nginx = Arc::new(MockNginxManager::with_root("/var/www/myservice"));
        let use_case = RemoveTorService::new(tor, Some(nginx.clone()));
        use_case.execute("myservice").await.expect("ok");
        assert_eq!(
            *nginx.docroot_cleanups.lock().unwrap(),
            vec!["/var/www/myservice".to_string()]
        );
    }

    #[tokio::test]
    async fn typed_services_never_touch_a_docroot() {
        // wp_/fileserver_/git_… tienen site propio con prefijo: nunca se
        // intenta retirar un directorio raíz.
        for name in ["wp_blog", "fileserver_docs", "git_dev"] {
            let tor = Arc::new(MockTorManager::new());
            let nginx = Arc::new(MockNginxManager::with_root("/var/www/x"));
            let use_case = RemoveTorService::new(tor, Some(nginx.clone()));
            use_case.execute(name).await.expect("ok");
            assert!(nginx.docroot_cleanups.lock().unwrap().is_empty(), "{name}");
        }
    }

    #[tokio::test]
    async fn without_nginx_no_docroot_is_touched() {
        let tor = Arc::new(MockTorManager::new());
        let use_case = RemoveTorService::new(tor, None);
        assert!(use_case.execute("myservice").await.is_ok());
    }

    #[tokio::test]
    async fn infra_failure_keeps_docroot() {
        let tor = Arc::new(MockTorManager::failing());
        let nginx = Arc::new(MockNginxManager::with_root("/var/www/myservice"));
        let use_case = RemoveTorService::new(tor, Some(nginx.clone()));
        assert!(use_case.execute("myservice").await.is_err());
        assert!(nginx.docroot_cleanups.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn not_found_service_still_deletes_own_cert_and_returns_not_found() {
        let tor = Arc::new(MockTorManager::not_found());
        let nginx = Arc::new(MockNginxManager::new());
        let use_case = RemoveTorService::new(tor, Some(nginx.clone()));
        let result = use_case.execute("wp_blog").await;
        assert!(matches!(result, Err(EnolaError::NotFound(_))));
        assert_eq!(
            *nginx.deleted_certs.lock().unwrap(),
            vec!["wp_blog".to_string()]
        );
    }

    #[tokio::test]
    async fn infra_failure_keeps_cert() {
        let tor = Arc::new(MockTorManager::failing());
        let nginx = Arc::new(MockNginxManager::new());
        let use_case = RemoveTorService::new(tor, Some(nginx.clone()));
        assert!(use_case.execute("wp_blog").await.is_err());
        assert!(nginx.deleted_certs.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_remove_tor_service_tor_failure() {
        let tor = Arc::new(MockTorManager::failing());
        let use_case = RemoveTorService::new(tor, None);

        let result = use_case.execute("myservice").await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn remove_removes_only_the_given_name_and_its_canonical_site() {
        // `wp_blog`: servicio Tor `wp_blog`, site `proxy_wp_blog` y cert
        // `wp_blog`; nunca `proxy_wp_blog` como servicio ni el site `wp_blog`.
        let tor = Arc::new(MockTorManager::new());
        let nginx = Arc::new(MockNginxManager::new());
        let use_case = RemoveTorService::new(tor.clone(), Some(nginx.clone()));
        use_case.execute("wp_blog").await.unwrap();
        assert_eq!(*tor.removed.lock().unwrap(), vec!["wp_blog".to_string()]);
        assert_eq!(
            *nginx.deleted_sites.lock().unwrap(),
            vec!["proxy_wp_blog".to_string()]
        );
        assert_eq!(
            *nginx.deleted_certs.lock().unwrap(),
            vec!["wp_blog".to_string()]
        );
    }

    #[tokio::test]
    async fn remove_does_not_try_proxy_variant() {
        // Antes se probaba también `proxy_<n>`, que puede ser otro servicio.
        let tor = Arc::new(MockTorManager::missing_with(&["proxy_blog"]));
        let use_case = RemoveTorService::new(tor.clone(), None);
        assert!(use_case.execute("blog").await.is_err());
        assert_eq!(*tor.removed.lock().unwrap(), vec!["blog".to_string()]);
    }

    #[tokio::test]
    async fn remove_keeps_cert_when_tor_removal_fails() {
        let tor = Arc::new(MockTorManager::failing());
        let nginx = Arc::new(MockNginxManager::new());
        let use_case = RemoveTorService::new(tor, Some(nginx.clone()));
        assert!(use_case.execute("wp_blog").await.is_err());
        assert!(nginx.deleted_certs.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn remove_keeps_original_error() {
        let tor = Arc::new(MockTorManager::missing_with(&["proxy_drupal-sitio"]));
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
        // se intenta el canónico, nunca `blog`/`proxy_blog`.
        let tor = Arc::new(MockTorManager::missing_with(&["proxy_blog", "blog"]));
        let use_case = RemoveTorService::new(tor.clone(), None);
        assert_eq!(
            use_case.remove_published("wp_blog").await,
            RemoveOutcome::NotPublished
        );
        assert_eq!(*tor.removed.lock().unwrap(), vec!["wp_blog".to_string()]);
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
