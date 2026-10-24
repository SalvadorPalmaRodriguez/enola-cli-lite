//! Caso de uso `tor ssl` — activa SSL post-hoc sobre un servicio `.onion`
//! ya publicado en HTTP (MEGAPLAN-PUBLISH-HARMONIZATION, D11).
//!
//! Secuencia: `detect_ssl_config` (trait, D15) → error si ya es SSL →
//! cert canónico → reescritura del site Nginx a HTTP+HTTPS →
//! `deploy_hidden_service` con `[(80,http),(443,https)]` → reload Nginx+Tor.
//!
//! Soporta los dominios proxy (git, wp, cms×5, ai, `tor create web`) y
//! FileServer (requiere `root_dir`). `static`/`raw` no tienen site proxy y
//! se rechazan con error claro.

use crate::domain::error::{EnolaError, Result};
use crate::domain::naming::{
    find_legacy_v09, legacy_v09_hint, validate_tor_service_name, ServiceName, ServiceType,
};
use crate::ports::manifest::ManifestPort;
use crate::ports::tor::TorManagerPort;
use crate::ports::web::{NginxFileServerConfigWithSsl, NginxManagerPort, NginxProxyConfigWithSsl};
use std::sync::Arc;

/// Petición de `tor ssl`. Los puertos llegan ya resueltos por el caller (D8).
pub struct TorSslRequest {
    /// Nombre canónico del servicio Tor (`ai_dev`, `wp_blog`, `proxy_web1`,
    /// `fileserver_docs`).
    pub service_name: String,
    /// Puerto HTTP actual de Nginx (listener existente del site).
    pub http_port: u16,
    /// Puerto HTTPS nuevo de Nginx.
    pub https_port: u16,
    /// Puerto backend al que hace proxy (dominios proxy).
    pub backend_port: u16,
    /// Directorio raíz (obligatorio si el site es FileServer).
    pub root_dir: Option<String>,
}

/// Resultado del retrofit SSL.
#[derive(Debug)]
pub struct TorSslResult {
    pub onion_address: String,
    pub https_port: u16,
}

/// Caso de uso: retrofit SSL sobre un servicio `.onion` HTTP existente.
pub struct TorSsl {
    nginx_manager: Arc<dyn NginxManagerPort + Send + Sync>,
    tor_manager: Arc<dyn TorManagerPort + Send + Sync>,
    manifest: Arc<dyn ManifestPort + Send + Sync>,
}

/// Parsea una config de site Nginx y devuelve
/// `(http_port, backend_port, root_dir)`:
/// - `http_port`: primer `listen 127.0.0.1:P;` no-SSL;
/// - `backend_port`: `proxy_pass http://127.0.0.1:P;`;
/// - `root_dir`: directiva `root P;` (sites FileServer).
///
/// Lógica pura: el caller (CLI/MCP) lee el fichero.
pub fn parse_site_config(content: &str) -> (Option<u16>, Option<u16>, Option<String>) {
    let mut http_port = None;
    let mut backend_port = None;
    let mut root_dir = None;
    for line in content.lines() {
        let t = line.trim();
        if t.starts_with("listen") && !t.contains("ssl") && t.contains("127.0.0.1:") {
            http_port = http_port.or_else(|| {
                t.split("127.0.0.1:")
                    .nth(1)
                    .and_then(|v| v.trim_end_matches(';').split_whitespace().next())
                    .and_then(|v| v.parse::<u16>().ok())
            });
        }
        if t.starts_with("proxy_pass") && t.contains("127.0.0.1:") {
            backend_port = t
                .split("127.0.0.1:")
                .nth(1)
                .and_then(|v| v.trim_end_matches(';').split_whitespace().next())
                .and_then(|v| v.parse::<u16>().ok());
        }
        if t.starts_with("root ") {
            root_dir = t
                .trim_start_matches("root")
                .trim()
                .trim_end_matches(';')
                .split_whitespace()
                .next()
                .map(|v| v.to_string());
        }
    }
    (http_port, backend_port, root_dir)
}

impl TorSsl {
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

    pub async fn execute(&self, request: TorSslRequest) -> Result<TorSslResult> {
        validate_tor_service_name(&request.service_name)?;

        // Resolver dominio y nombre base desde el nombre canónico
        let parsed = ServiceName::from_tor_config(&request.service_name).ok_or_else(|| {
            EnolaError::ValidationError(format!(
                "El servicio '{}' no sigue la nomenclatura canónica <dominio>_<name>",
                request.service_name
            ))
        })?;
        let kind = parsed.service_type;
        if matches!(kind, ServiceType::StaticSite | ServiceType::Raw) {
            return Err(EnolaError::ValidationError(format!(
                "El servicio '{}' ({kind}) no tiene site Nginx convertible a SSL",
                request.service_name
            )));
        }
        let base = parsed.base_name.as_str();
        let site_name = ServiceName::new(base, kind)
            .nginx_config_name()
            .ok_or_else(|| {
                EnolaError::ValidationError(format!(
                    "El dominio '{kind}' no tiene site Nginx asociado"
                ))
            })?;

        // 0. El servicio debe existir: `deploy_hidden_service` crearía uno nuevo.
        //    Si solo está el de v0.9.0 (nombre anterior), se dice cómo migrar.
        let services = self.tor_manager.list_hidden_services().await?;
        let existing: Vec<&str> = services.iter().map(|s| s.name.as_str()).collect();
        if !existing.contains(&request.service_name.as_str()) {
            return Err(EnolaError::NotFound(
                match find_legacy_v09(&request.service_name, &existing) {
                    Some((legacy, _)) => legacy_v09_hint(&request.service_name, &legacy),
                    None => format!("El servicio '{}' no existe", request.service_name),
                },
            ));
        }

        // 1. Idempotencia explícita: error claro si ya es SSL (D11)
        let (has_ssl, _, _) = self
            .nginx_manager
            .detect_ssl_config(&request.service_name)
            .await?;
        if has_ssl {
            return Err(EnolaError::ValidationError(format!(
                "El servicio '{}' already has SSL enabled",
                request.service_name
            )));
        }

        // 2. Certificado autofirmado canónico (D7)
        let (cert_path, key_path) = self
            .nginx_manager
            .generate_self_signed_cert(&request.service_name)
            .await?;
        let _ = self.manifest.append("ssl_cert", &cert_path);
        let _ = self.manifest.append("ssl_key", &key_path);

        // 3. Reescribir el site como HTTP+HTTPS
        //    `service_name` para el adapter: base en Proxy/FileServer,
        //    canónico en el resto (los adapters prefijan proxy_/fileserver_).
        match kind {
            ServiceType::FileServer => {
                let root_dir = request.root_dir.clone().ok_or_else(|| {
                    EnolaError::ValidationError(format!(
                        "El servicio '{}' es FileServer: root_dir requerido",
                        request.service_name
                    ))
                })?;
                self.nginx_manager
                    .create_fileserver_config_with_ssl(NginxFileServerConfigWithSsl {
                        service_name: base.to_string(),
                        http_port: request.http_port,
                        https_port: request.https_port,
                        root_dir,
                        ssl_cert_path: cert_path.clone(),
                        ssl_key_path: key_path.clone(),
                    })
                    .await?;
            }
            _ => {
                let adapter_name = if kind == ServiceType::Proxy {
                    base.to_string()
                } else {
                    request.service_name.clone()
                };
                self.nginx_manager
                    .create_proxy_config_with_ssl(NginxProxyConfigWithSsl {
                        service_name: adapter_name,
                        http_port: request.http_port,
                        https_port: request.https_port,
                        backend_port: request.backend_port,
                        server_name: "localhost".into(),
                        ssl_cert_path: cert_path.clone(),
                        ssl_key_path: key_path.clone(),
                        rate_limit: None,
                    })
                    .await?;
            }
        }
        let _ = self.manifest.append("nginx_config", &site_name);

        // 4. Validar, habilitar y recargar Nginx
        if !self.nginx_manager.validate_config().await? {
            return Err(EnolaError::InfrastructureError(
                "Generated Nginx SSL config is invalid".into(),
            ));
        }
        self.nginx_manager.enable_site(&site_name).await?;
        self.nginx_manager.reload().await?;

        // 5. Añadir :443 al hidden service existente
        let onion_address = self
            .tor_manager
            .deploy_hidden_service(
                &request.service_name,
                vec![(80, request.http_port), (443, request.https_port)],
            )
            .await?;
        self.tor_manager.reload_tor().await?;

        Ok(TorSslResult {
            onion_address,
            https_port: request.https_port,
        })
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
    async fn tor_ssl_requires_an_existing_service() {
        // Sin servicio Tor no hay nada que convertir: nunca debe crear uno
        // nuevo con `deploy_hidden_service`.
        let nginx = MockNginxManagerPort::new();
        let mut tor = MockTorManagerPort::new();
        tor.expect_list_hidden_services().returning(|| Ok(vec![]));
        let uc = TorSsl::new(Arc::new(nginx), Arc::new(tor), mock_manifest());
        let err = uc.execute(req("wp_blog")).await.unwrap_err();
        assert!(matches!(err, EnolaError::NotFound(_)), "{err}");
    }

    #[tokio::test]
    async fn tor_ssl_reports_v09_service_with_previous_name() {
        let nginx = MockNginxManagerPort::new();
        let mut tor = MockTorManagerPort::new();
        tor.expect_list_hidden_services()
            .returning(|| Ok(vec![tor_service("proxy_ghost-blog")]));
        let uc = TorSsl::new(Arc::new(nginx), Arc::new(tor), mock_manifest());
        let err = uc.execute(req("ghost_blog")).await.unwrap_err().to_string();
        assert!(
            err.contains("sudo enola-cli tor remove proxy_ghost-blog"),
            "{err}"
        );
    }

    fn req(name: &str) -> TorSslRequest {
        TorSslRequest {
            service_name: name.to_string(),
            http_port: 10081,
            https_port: 15002,
            backend_port: 3000,
            root_dir: None,
        }
    }

    #[tokio::test]
    async fn tor_ssl_retrofits_proxy_service() {
        let mut nginx = MockNginxManagerPort::new();
        nginx
            .expect_detect_ssl_config()
            .withf(|d| d == "ai_dev")
            .times(1)
            .returning(|_| Ok((false, None, None)));
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
        tor.expect_list_hidden_services()
            .returning(|| Ok(vec![tor_service("ai_dev")]));
        tor.expect_deploy_hidden_service()
            .withf(|name, ports| {
                name == "ai_dev" && *ports == vec![(80u16, 10081u16), (443u16, 15002u16)]
            })
            .times(1)
            .returning(|_, _| Ok("aidev.onion".to_string()));
        tor.expect_reload_tor().times(1).returning(|| Ok(()));

        let uc = TorSsl::new(Arc::new(nginx), Arc::new(tor), mock_manifest());
        let res = uc.execute(req("ai_dev")).await.unwrap();
        assert_eq!(res.onion_address, "aidev.onion");
        assert_eq!(res.https_port, 15002);
    }

    #[tokio::test]
    async fn tor_ssl_retrofits_tor_create_web_service() {
        // `proxy_web1`: site `proxy_web1`, adapter service_name = base `web1`
        let mut nginx = MockNginxManagerPort::new();
        nginx
            .expect_detect_ssl_config()
            .returning(|_| Ok((false, None, None)));
        nginx
            .expect_generate_self_signed_cert()
            .withf(|s| s == "proxy_web1")
            .times(1)
            .returning(|_| {
                Ok((
                    "/etc/nginx/ssl/proxy_web1.crt".to_string(),
                    "/etc/nginx/ssl/proxy_web1.key".to_string(),
                ))
            });
        nginx
            .expect_create_proxy_config_with_ssl()
            .withf(|cfg| cfg.service_name == "web1")
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
        tor.expect_list_hidden_services()
            .returning(|| Ok(vec![tor_service("proxy_web1")]));
        tor.expect_deploy_hidden_service()
            .withf(|name, _| name == "proxy_web1")
            .times(1)
            .returning(|_, _| Ok("w.onion".to_string()));
        tor.expect_reload_tor().times(1).returning(|| Ok(()));

        let uc = TorSsl::new(Arc::new(nginx), Arc::new(tor), mock_manifest());
        uc.execute(req("proxy_web1")).await.unwrap();
    }

    #[tokio::test]
    async fn tor_ssl_errors_if_already_ssl_without_side_effects() {
        let mut nginx = MockNginxManagerPort::new();
        nginx.expect_detect_ssl_config().times(1).returning(|_| {
            Ok((
                true,
                Some(15002),
                Some("/etc/nginx/ssl/ai_dev.crt".to_string()),
            ))
        });
        // Ninguna otra expectativa: cualquier efecto extra rompe el mock.

        let mut tor = MockTorManagerPort::new();
        tor.expect_list_hidden_services()
            .returning(|| Ok(vec![tor_service("ai_dev")]));
        let uc = TorSsl::new(Arc::new(nginx), Arc::new(tor), mock_manifest());
        let res = uc.execute(req("ai_dev")).await;
        assert!(res.is_err());
        let msg = res.unwrap_err().to_string();
        assert!(msg.contains("already has SSL"), "msg: {msg}");
    }

    #[tokio::test]
    async fn tor_ssl_retrofits_fileserver_with_root_dir() {
        let mut nginx = MockNginxManagerPort::new();
        nginx
            .expect_detect_ssl_config()
            .returning(|_| Ok((false, None, None)));
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
            .withf(|cfg| cfg.service_name == "docs" && cfg.root_dir == "/srv/enola-files/docs")
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
        tor.expect_list_hidden_services()
            .returning(|| Ok(vec![tor_service("fileserver_docs")]));
        tor.expect_deploy_hidden_service()
            .withf(|name, _| name == "fileserver_docs")
            .times(1)
            .returning(|_, _| Ok("f.onion".to_string()));
        tor.expect_reload_tor().times(1).returning(|| Ok(()));

        let uc = TorSsl::new(Arc::new(nginx), Arc::new(tor), mock_manifest());
        let mut r = req("fileserver_docs");
        r.root_dir = Some("/srv/enola-files/docs".to_string());
        uc.execute(r).await.unwrap();
    }

    #[tokio::test]
    async fn tor_ssl_rejects_non_canonical_and_unsupported() {
        let nginx = MockNginxManagerPort::new();
        let tor = MockTorManagerPort::new();
        let uc = TorSsl::new(Arc::new(nginx), Arc::new(tor), mock_manifest());

        assert!(uc.execute(req("raw_tcp1")).await.is_err());
        assert!(uc.execute(req("not-a-service!")).await.is_err());
        assert!(uc.execute(req("myservice-plain")).await.is_err());
    }
}
