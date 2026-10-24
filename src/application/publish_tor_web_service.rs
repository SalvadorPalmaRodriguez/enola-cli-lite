//! Caso de uso unificado de publicación `.onion` (MEGAPLAN-PUBLISH-HARMONIZATION, D12).
//!
//! Orquesta la secuencia *cert autofirmado → config Nginx → hidden service Tor*
//! para todos los dominios publicables, eliminando las copias duplicadas de la
//! lógica en `cli/commands.rs` y en casos de uso concretos.
//!
//! Convenciones (§5 del plan):
//! - Servicio Tor: nombre canónico `<dominio>_<name>` (`ai_dev`, `wp_blog`,
//!   `fileserver_docs`, `proxy_web1` para `tor create web`).
//! - Site Nginx: `proxy_<canonical>` para dominios proxy, `fileserver_<name>`
//!   para FileServer, `proxy_<name>` para `tor create web`.
//! - Certificado: `/etc/nginx/ssl/{name_canonical}.{crt,key}` (D7).
//! - Auth: siempre vía `ManageClientAuth` (D6), nunca `enable_client_auth`
//!   directo.
//! - Los puertos llegan ya resueltos por el caller (D8: los locks viven en CLI).

use crate::domain::error::{EnolaError, Result};
use crate::domain::naming::{
    find_legacy_v09, legacy_v09_hint, validate_service_name, validate_tor_service_name,
    LegacyMatch, ServiceName, ServiceType,
};
use crate::ports::manifest::ManifestPort;
use crate::ports::tor::TorManagerPort;
use crate::ports::web::{
    NginxFileServerConfig, NginxFileServerConfigWithSsl, NginxManagerPort, NginxProxyConfig,
    NginxProxyConfigWithSsl,
};
use std::sync::Arc;

/// Petición del caso de uso unificado de publicación `.onion`.
pub struct PublishTorWebServiceRequest {
    /// Dominio del servicio (`ServiceType::Git`, `WordPress`, `Drupal`,
    /// `Ghost`, `Magnolia`, `Strapi`, `Wagtail`, `FileServer`, `Ai`, `Proxy`).
    pub service_kind: ServiceType,
    /// Nombre canónico del servicio Tor (`<dominio>_<name>`).
    pub name_canonical: String,
    /// Puerto local del backend al que Nginx hace proxy (ignorado en FileServer).
    pub backend_port: u16,
    /// Puerto HTTP de escucha de Nginx (bind 127.0.0.1).
    pub http_port: u16,
    /// Puerto HTTPS de escucha de Nginx (obligatorio si `ssl`).
    pub https_port: Option<u16>,
    /// Mapeos extra del hidden service (p.ej. SSH `22→22` en `git publish`).
    pub extra_ports: Vec<(u16, u16)>,
    /// Genera cert autofirmado canónico y publica `.onion:443` → Nginx HTTPS.
    pub ssl: bool,
    /// Activa Tor client auth tras publicar (vía `ManageClientAuth`, D6).
    pub auth: bool,
    /// Directorio raíz del file server (obligatorio si `service_kind` es
    /// `FileServer`; p.ej. `/srv/enola-files/<name>`).
    pub root_dir: Option<String>,
}

/// Resultado de la publicación.
#[derive(Debug)]
pub struct PublishTorWebResult {
    pub onion_address: String,
    /// Nombre canónico del servicio Tor desplegado.
    pub service_name: String,
    /// Nombre del site Nginx habilitado.
    pub nginx_site: String,
    pub http_port: u16,
    pub https_port: Option<u16>,
    pub auth_enabled: bool,
    /// Avisos no bloqueantes (p.ej. un posible servicio de v0.9.0 con el
    /// nombre anterior que también usan otros comandos).
    pub warnings: Vec<String>,
}

/// Caso de uso unificado: publica un servicio web tras Nginx + Tor `.onion`.
pub struct PublishTorWebService {
    nginx_manager: Arc<dyn NginxManagerPort + Send + Sync>,
    tor_manager: Arc<dyn TorManagerPort + Send + Sync>,
    manifest: Arc<dyn ManifestPort + Send + Sync>,
}

impl PublishTorWebService {
    pub fn new(
        nginx_manager: Arc<dyn NginxManagerPort + Send + Sync>,
        tor_manager: Arc<dyn TorManagerPort + Send + Sync>,
        manifest: Arc<dyn ManifestPort + Send + Sync>,
    ) -> Self {
        Self {
            nginx_manager,
            tor_manager,
            manifest,
        }
    }

    /// Deriva `(base_name, site_name, adapter_service_name)` a partir del
    /// dominio y el nombre canónico.
    ///
    /// Los adapters fijan el prefijo del fichero (`proxy_{x}` /
    /// `fileserver_{x}`), así que el `service_name` que reciben es:
    /// - dominios proxy (Git/CMS/Ai): el nombre canónico (`ai_dev` → site
    ///   `proxy_ai_dev`);
    /// - `Proxy` (`tor create web`): el nombre base (`web1` → site `proxy_web1`,
    ///   nunca `proxy_proxy_web1`);
    /// - `FileServer`: el nombre base (`docs` → site `fileserver_docs`).
    fn resolve_names(kind: ServiceType, name_canonical: &str) -> Result<(&str, String, String)> {
        if matches!(kind, ServiceType::StaticSite | ServiceType::Raw) {
            return Err(EnolaError::ValidationError(format!(
                "El dominio '{kind}' no se publica vía PublishTorWebService"
            )));
        }
        let base = name_canonical
            .strip_prefix(kind.tor_config_prefix())
            .filter(|b| !b.is_empty())
            .ok_or_else(|| {
                EnolaError::ValidationError(format!(
                    "El nombre canónico '{name_canonical}' no corresponde al dominio '{kind}'"
                ))
            })?;
        // El base es el nombre del usuario: mismas reglas que `execute_typed`
        // (sin `_`, que es el separador de dominio y permitiría suplantar otro).
        validate_service_name(base)?;
        let site_name = ServiceName::new(base, kind)
            .nginx_config_name()
            .ok_or_else(|| {
                EnolaError::ValidationError(format!(
                    "El dominio '{kind}' no tiene site Nginx asociado"
                ))
            })?;
        let adapter_name = match kind {
            ServiceType::Proxy | ServiceType::FileServer => base.to_string(),
            _ => name_canonical.to_string(),
        };
        Ok((base, site_name, adapter_name))
    }

    /// Ejecuta la publicación completa.
    pub async fn execute(
        &self,
        request: PublishTorWebServiceRequest,
    ) -> Result<PublishTorWebResult> {
        validate_tor_service_name(&request.name_canonical)?;

        let (_base, site_name, adapter_name) =
            Self::resolve_names(request.service_kind, &request.name_canonical)?;
        let kind = request.service_kind;
        let https_port = match (request.ssl, request.https_port) {
            (true, Some(p)) => Some(p),
            (true, None) => {
                return Err(EnolaError::ValidationError(
                    "--ssl requiere un puerto HTTPS (https_port)".into(),
                ))
            }
            (false, p) => p,
        };

        let root_dir = if kind == ServiceType::FileServer {
            Some(request.root_dir.clone().ok_or_else(|| {
                EnolaError::ValidationError("FileServer requiere root_dir".into())
            })?)
        } else {
            None
        };

        // 0. Servicio de v0.9.0 con el nombre anterior (antes de cualquier efecto).
        let warnings = self.check_v09_legacy(&request.name_canonical).await?;

        // 1. Certificado autofirmado canónico (D7) si ssl
        let cert = if request.ssl {
            let paths = self
                .nginx_manager
                .generate_self_signed_cert(&request.name_canonical)
                .await?;
            let _ = self.manifest.append("ssl_cert", &paths.0);
            let _ = self.manifest.append("ssl_key", &paths.1);
            Some(paths)
        } else {
            None
        };
        let (cert_path, key_path) = cert
            .as_ref()
            .map(|(c, k)| (c.clone(), k.clone()))
            .unwrap_or_default();

        // 2. Config Nginx según dominio y modo
        match kind {
            ServiceType::FileServer => {
                let root_dir = root_dir.clone().unwrap_or_default();
                if request.ssl {
                    self.nginx_manager
                        .create_fileserver_config_with_ssl(NginxFileServerConfigWithSsl {
                            service_name: adapter_name.clone(),
                            http_port: request.http_port,
                            https_port: https_port.unwrap_or_default(),
                            root_dir,
                            ssl_cert_path: cert_path,
                            ssl_key_path: key_path,
                        })
                        .await?;
                } else {
                    self.nginx_manager
                        .create_fileserver_config(NginxFileServerConfig {
                            service_name: adapter_name.clone(),
                            listen_port: request.http_port,
                            root_dir,
                            disable_symlinks: true,
                            allow_upload: false,
                        })
                        .await?;
                }
            }
            _ => {
                if request.ssl {
                    self.nginx_manager
                        .create_proxy_config_with_ssl(NginxProxyConfigWithSsl {
                            service_name: adapter_name.clone(),
                            http_port: request.http_port,
                            https_port: https_port.unwrap_or_default(),
                            backend_port: request.backend_port,
                            server_name: "localhost".into(),
                            ssl_cert_path: cert_path,
                            ssl_key_path: key_path,
                            rate_limit: None,
                        })
                        .await?;
                } else {
                    self.nginx_manager
                        .create_proxy_config(NginxProxyConfig {
                            service_name: adapter_name.clone(),
                            listen_port: request.http_port,
                            backend_port: request.backend_port,
                            server_name: "localhost".into(),
                            rate_limit: None,
                        })
                        .await?;
                }
            }
        }
        let _ = self.manifest.append("nginx_config", &site_name);

        // 3. Validar, habilitar y recargar Nginx
        if !self.nginx_manager.validate_config().await? {
            return Err(EnolaError::InfrastructureError(
                "Generated Nginx config is invalid".into(),
            ));
        }
        self.nginx_manager.enable_site(&site_name).await?;
        self.nginx_manager.reload().await?;

        // 4. Hidden service Tor con nombre canónico
        let mut ports = vec![(80u16, request.http_port)];
        if request.ssl {
            ports.push((443, https_port.unwrap_or_default()));
        }
        ports.extend(request.extra_ports.iter().copied());
        let onion_address = self
            .tor_manager
            .deploy_hidden_service(&request.name_canonical, ports)
            .await?;
        let _ = self.manifest.append("tor_service", &request.name_canonical);

        // 5. Auth vía ManageClientAuth (D6) — nunca enable_client_auth directo
        if request.auth {
            crate::application::manage_client_auth::ManageClientAuth::new(self.tor_manager.clone())
                .toggle_auth(&request.name_canonical, true)
                .await?;
        }

        self.tor_manager.reload_tor().await?;

        Ok(PublishTorWebResult {
            onion_address,
            service_name: request.name_canonical,
            nginx_site: site_name,
            http_port: request.http_port,
            https_port: if request.ssl { https_port } else { None },
            auth_enabled: request.auth,
            warnings,
        })
    }

    /// Servicio de v0.9.0 con el nombre anterior (sin compatibilidad, v0.10.0):
    /// si solo pudo crearlo este dominio, publicar dejaría dos `.onion` → error
    /// accionable; si el nombre es ambiguo, se publica y se avisa. Si Tor no
    /// puede listar servicios, no se bloquea la publicación.
    async fn check_v09_legacy(&self, canonical: &str) -> Result<Vec<String>> {
        let Ok(services) = self.tor_manager.list_hidden_services().await else {
            return Ok(Vec::new());
        };
        let existing: Vec<&str> = services.iter().map(|s| s.name.as_str()).collect();
        match find_legacy_v09(canonical, &existing) {
            Some((legacy, LegacyMatch::Unambiguous)) => Err(EnolaError::ValidationError(
                legacy_v09_hint(canonical, &legacy),
            )),
            Some((legacy, LegacyMatch::Ambiguous)) => {
                let hint = legacy_v09_hint(canonical, &legacy);
                tracing::warn!("{hint}");
                Ok(vec![hint])
            }
            None => Ok(Vec::new()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::manifest::MockManifestPort;
    use crate::ports::tor::MockTorManagerPort;
    use crate::ports::web::MockNginxManagerPort;

    fn mock_manifest() -> Arc<dyn ManifestPort + Send + Sync> {
        let mut m = MockManifestPort::new();
        m.expect_append().returning(|_, _| Ok(())).times(0..);
        m.expect_remove().returning(|_, _| Ok(())).times(0..);
        Arc::new(m)
    }

    fn req(kind: ServiceType, name: &str) -> PublishTorWebServiceRequest {
        PublishTorWebServiceRequest {
            service_kind: kind,
            name_canonical: name.to_string(),
            backend_port: 3000,
            http_port: 10081,
            https_port: None,
            extra_ports: vec![],
            ssl: false,
            auth: false,
            root_dir: None,
        }
    }

    #[tokio::test]
    async fn publish_rejects_invalid_base_name_before_any_side_effect() {
        // El nombre compuesto es válido para Tor (`proxy_x_y`), pero el base
        // (`x_y`) no: con `_` en el base se podría suplantar otro dominio
        // (`proxy_ai_dev` ≠ `ai_dev`). Se valida igual que `execute_typed`.
        let nginx = MockNginxManagerPort::new();
        let tor = MockTorManagerPort::new();
        let uc = PublishTorWebService::new(Arc::new(nginx), Arc::new(tor), mock_manifest());
        for (kind, canonical) in [
            (ServiceType::Proxy, "proxy_x_y"),
            (ServiceType::Ai, "ai_bad_name"),
            (ServiceType::FileServer, "fileserver_-docs"),
        ] {
            let err = uc.execute(req(kind, canonical)).await.unwrap_err();
            assert!(
                err.to_string().contains("Nombre de servicio inválido"),
                "{canonical}: {err}"
            );
        }
    }

    fn tor_service(name: &str) -> crate::ports::tor::TorServiceInfo {
        crate::ports::tor::TorServiceInfo {
            name: name.into(),
            hostname: format!("{name}.onion"),
            hidden_service_dir: format!("/var/lib/tor/enola_{name}"),
            ports: vec![],
            clients: vec![],
            active: true,
            auth_enabled: false,
        }
    }

    #[tokio::test]
    async fn publish_refuses_when_v09_service_of_same_cms_exists() {
        // `proxy_drupal-sitio` solo pudo crearlo `drupal publish sitio` en
        // v0.9.0: publicar `drupal_sitio` dejaría dos .onion → error accionable.
        let nginx = MockNginxManagerPort::new(); // ninguna llamada
        let mut tor = MockTorManagerPort::new();
        tor.expect_list_hidden_services()
            .returning(|| Ok(vec![tor_service("proxy_drupal-sitio")]));
        let uc = PublishTorWebService::new(Arc::new(nginx), Arc::new(tor), mock_manifest());
        let err = uc
            .execute(req(ServiceType::Drupal, "drupal_sitio"))
            .await
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("sudo enola-cli tor remove proxy_drupal-sitio"),
            "{err}"
        );
    }

    #[tokio::test]
    async fn publish_warns_but_continues_on_ambiguous_v09_name() {
        // `proxy_blog` puede ser un `wp publish blog` de v0.9.0 o un
        // `tor create web blog`: se publica `wp_blog` y se avisa.
        let mut nginx = MockNginxManagerPort::new();
        nginx.expect_create_proxy_config().returning(|_| Ok(()));
        nginx.expect_validate_config().returning(|| Ok(true));
        nginx.expect_enable_site().returning(|_| Ok(()));
        nginx.expect_reload().returning(|| Ok(()));
        let mut tor = MockTorManagerPort::new();
        tor.expect_list_hidden_services()
            .returning(|| Ok(vec![tor_service("proxy_blog")]));
        tor.expect_deploy_hidden_service()
            .withf(|name, _| name == "wp_blog")
            .returning(|_, _| Ok("wp.onion".to_string()));
        tor.expect_reload_tor().returning(|| Ok(()));
        let uc = PublishTorWebService::new(Arc::new(nginx), Arc::new(tor), mock_manifest());
        let res = uc
            .execute(req(ServiceType::WordPress, "wp_blog"))
            .await
            .unwrap();
        assert_eq!(res.service_name, "wp_blog");
        assert_eq!(res.warnings.len(), 1);
        assert!(
            res.warnings[0].contains("tor remove proxy_blog"),
            "{:?}",
            res.warnings
        );
    }

    #[tokio::test]
    async fn publish_proxy_web_http_uses_canonical_names() {
        // `tor create web web1`: tor `proxy_web1`, site `proxy_web1` (no proxy_proxy_)
        let mut nginx = MockNginxManagerPort::new();
        nginx
            .expect_create_proxy_config()
            .withf(|cfg| cfg.service_name == "web1" && cfg.listen_port == 10081)
            .times(1)
            .returning(|_| Ok(()));
        nginx.expect_validate_config().returning(|| Ok(true));
        nginx
            .expect_enable_site()
            .withf(|s| s == "proxy_web1")
            .times(1)
            .returning(|_| Ok(()));
        nginx.expect_reload().times(1).returning(|| Ok(()));

        let mut tor = MockTorManagerPort::new();
        tor.expect_list_hidden_services().returning(|| Ok(vec![]));
        tor.expect_deploy_hidden_service()
            .withf(|name, ports| name == "proxy_web1" && *ports == vec![(80u16, 10081u16)])
            .times(1)
            .returning(|_, _| Ok("web1.onion".to_string()));
        tor.expect_reload_tor().times(1).returning(|| Ok(()));

        let uc = PublishTorWebService::new(Arc::new(nginx), Arc::new(tor), mock_manifest());
        let res = uc
            .execute(req(ServiceType::Proxy, "proxy_web1"))
            .await
            .unwrap();
        assert_eq!(res.onion_address, "web1.onion");
        assert_eq!(res.service_name, "proxy_web1");
        assert_eq!(res.nginx_site, "proxy_web1");
    }

    #[tokio::test]
    async fn publish_ai_with_ssl_and_auth_orchestrates_cert_nginx_tor_auth() {
        let mut nginx = MockNginxManagerPort::new();
        nginx
            .expect_generate_self_signed_cert()
            .withf(|s| s == "ai_dev")
            .times(1)
            .returning(|_| {
                Ok((
                    "/etc/nginx/ssl/ai_dev.crt".to_string(),
                    "/etc/nginx/ssl/ai_dev.key".to_string(),
                ))
            });
        nginx
            .expect_create_proxy_config_with_ssl()
            .withf(|cfg| {
                cfg.service_name == "ai_dev"
                    && cfg.http_port == 10081
                    && cfg.https_port == 15002
                    && cfg.backend_port == 3000
                    && cfg.ssl_cert_path == "/etc/nginx/ssl/ai_dev.crt"
            })
            .times(1)
            .returning(|_| Ok(()));
        nginx.expect_validate_config().returning(|| Ok(true));
        nginx
            .expect_enable_site()
            .withf(|s| s == "proxy_ai_dev")
            .times(1)
            .returning(|_| Ok(()));
        nginx.expect_reload().times(1).returning(|| Ok(()));

        let mut tor = MockTorManagerPort::new();
        tor.expect_list_hidden_services().returning(|| Ok(vec![]));
        tor.expect_deploy_hidden_service()
            .withf(|name, ports| {
                name == "ai_dev" && *ports == vec![(80u16, 10081u16), (443u16, 15002u16)]
            })
            .times(1)
            .returning(|_, _| Ok("aidev.onion".to_string()));
        // auth SIEMPRE vía ManageClientAuth → enable_client_auth en el port
        tor.expect_enable_client_auth()
            .withf(|name| name == "ai_dev")
            .times(1)
            .returning(|_| Ok(()));
        tor.expect_reload_tor().times(1).returning(|| Ok(()));

        let uc = PublishTorWebService::new(Arc::new(nginx), Arc::new(tor), mock_manifest());
        let mut r = req(ServiceType::Ai, "ai_dev");
        r.ssl = true;
        r.https_port = Some(15002);
        r.auth = true;
        let res = uc.execute(r).await.unwrap();
        assert_eq!(res.onion_address, "aidev.onion");
        assert_eq!(res.https_port, Some(15002));
        assert!(res.auth_enabled);
    }

    #[tokio::test]
    async fn publish_fileserver_http_writes_fileserver_site() {
        let mut nginx = MockNginxManagerPort::new();
        nginx
            .expect_create_fileserver_config()
            .withf(|cfg| {
                cfg.service_name == "docs"
                    && cfg.listen_port == 20001
                    && cfg.root_dir == "/srv/enola-files/docs"
            })
            .times(1)
            .returning(|_| Ok(()));
        nginx.expect_validate_config().returning(|| Ok(true));
        nginx
            .expect_enable_site()
            .withf(|s| s == "fileserver_docs")
            .times(1)
            .returning(|_| Ok(()));
        nginx.expect_reload().times(1).returning(|| Ok(()));

        let mut tor = MockTorManagerPort::new();
        tor.expect_list_hidden_services().returning(|| Ok(vec![]));
        tor.expect_deploy_hidden_service()
            .withf(|name, ports| name == "fileserver_docs" && *ports == vec![(80u16, 20001u16)])
            .times(1)
            .returning(|_, _| Ok("files.onion".to_string()));
        tor.expect_reload_tor().times(1).returning(|| Ok(()));

        let uc = PublishTorWebService::new(Arc::new(nginx), Arc::new(tor), mock_manifest());
        let mut r = req(ServiceType::FileServer, "fileserver_docs");
        r.http_port = 20001;
        r.root_dir = Some("/srv/enola-files/docs".to_string());
        let res = uc.execute(r).await.unwrap();
        assert_eq!(res.nginx_site, "fileserver_docs");
    }

    #[tokio::test]
    async fn publish_fileserver_ssl_uses_fileserver_ssl_config() {
        let mut nginx = MockNginxManagerPort::new();
        nginx
            .expect_generate_self_signed_cert()
            .withf(|s| s == "fileserver_docs")
            .times(1)
            .returning(|_| {
                Ok((
                    "/etc/nginx/ssl/fileserver_docs.crt".to_string(),
                    "/etc/nginx/ssl/fileserver_docs.key".to_string(),
                ))
            });
        nginx
            .expect_create_fileserver_config_with_ssl()
            .withf(|cfg| {
                cfg.service_name == "docs"
                    && cfg.http_port == 12001
                    && cfg.https_port == 15002
                    && cfg.root_dir == "/srv/enola-files/docs"
            })
            .times(1)
            .returning(|_| Ok(()));
        nginx.expect_validate_config().returning(|| Ok(true));
        nginx
            .expect_enable_site()
            .withf(|s| s == "fileserver_docs")
            .times(1)
            .returning(|_| Ok(()));
        nginx.expect_reload().times(1).returning(|| Ok(()));

        let mut tor = MockTorManagerPort::new();
        tor.expect_list_hidden_services().returning(|| Ok(vec![]));
        tor.expect_deploy_hidden_service()
            .withf(|name, ports| {
                name == "fileserver_docs" && *ports == vec![(80u16, 12001u16), (443u16, 15002u16)]
            })
            .times(1)
            .returning(|_, _| Ok("files2.onion".to_string()));
        tor.expect_reload_tor().times(1).returning(|| Ok(()));

        let uc = PublishTorWebService::new(Arc::new(nginx), Arc::new(tor), mock_manifest());
        let mut r = req(ServiceType::FileServer, "fileserver_docs");
        r.http_port = 12001;
        r.ssl = true;
        r.https_port = Some(15002);
        r.root_dir = Some("/srv/enola-files/docs".to_string());
        let res = uc.execute(r).await.unwrap();
        assert_eq!(res.service_name, "fileserver_docs");
    }

    #[tokio::test]
    async fn publish_drupal_uses_backend_prefix() {
        let mut nginx = MockNginxManagerPort::new();
        nginx
            .expect_create_proxy_config()
            .withf(|cfg| cfg.service_name == "drupal_site")
            .times(1)
            .returning(|_| Ok(()));
        nginx.expect_validate_config().returning(|| Ok(true));
        nginx
            .expect_enable_site()
            .withf(|s| s == "proxy_drupal_site")
            .times(1)
            .returning(|_| Ok(()));
        nginx.expect_reload().times(1).returning(|| Ok(()));

        let mut tor = MockTorManagerPort::new();
        tor.expect_list_hidden_services().returning(|| Ok(vec![]));
        tor.expect_deploy_hidden_service()
            .withf(|name, _| name == "drupal_site")
            .times(1)
            .returning(|_, _| Ok("d.onion".to_string()));
        tor.expect_reload_tor().times(1).returning(|| Ok(()));

        let uc = PublishTorWebService::new(Arc::new(nginx), Arc::new(tor), mock_manifest());
        uc.execute(req(ServiceType::Drupal, "drupal_site"))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn publish_rejects_mismatched_canonical_and_missing_params() {
        let nginx = MockNginxManagerPort::new();
        let tor = MockTorManagerPort::new();
        let uc = PublishTorWebService::new(Arc::new(nginx), Arc::new(tor), mock_manifest());

        // nombre canónico que no corresponde al dominio declarado
        let r = req(ServiceType::Ai, "wp_blog");
        assert!(uc.execute(r).await.is_err());

        // ssl sin https_port
        let mut r = req(ServiceType::Ai, "ai_dev");
        r.ssl = true;
        assert!(uc.execute(r).await.is_err());

        // FileServer sin root_dir
        let r = req(ServiceType::FileServer, "fileserver_docs");
        assert!(uc.execute(r).await.is_err());

        // dominio no publicable vía este caso de uso
        let r = req(ServiceType::Raw, "raw_tcp1");
        assert!(uc.execute(r).await.is_err());
    }

    #[tokio::test]
    async fn publish_git_publish_appends_extra_ports() {
        // git publish añade SSH 22→22 como extra_port
        let mut nginx = MockNginxManagerPort::new();
        nginx
            .expect_create_proxy_config()
            .times(1)
            .returning(|_| Ok(()));
        nginx.expect_validate_config().returning(|| Ok(true));
        nginx
            .expect_enable_site()
            .withf(|s| s == "proxy_git_repo")
            .times(1)
            .returning(|_| Ok(()));
        nginx.expect_reload().times(1).returning(|| Ok(()));

        let mut tor = MockTorManagerPort::new();
        tor.expect_list_hidden_services().returning(|| Ok(vec![]));
        tor.expect_deploy_hidden_service()
            .withf(|name, ports| {
                name == "git_repo" && *ports == vec![(80u16, 10081u16), (22u16, 22u16)]
            })
            .times(1)
            .returning(|_, _| Ok("git.onion".to_string()));
        tor.expect_reload_tor().times(1).returning(|| Ok(()));

        let uc = PublishTorWebService::new(Arc::new(nginx), Arc::new(tor), mock_manifest());
        let mut r = req(ServiceType::Git, "git_repo");
        r.extra_ports = vec![(22, 22)];
        uc.execute(r).await.unwrap();
    }
}
