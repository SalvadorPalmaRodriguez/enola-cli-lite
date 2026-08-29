// CMS-STRAPI-001 (2026-05-05) — Adapter Strapi v5 (Node.js/TypeScript) del catálogo CMS.
//
// Cuarto CMS validando la abstracción `CmsLifecycle` (DRUPAL-001 13.56).
// Primer representante Node.js headless (REST API + GraphQL). Headless CMS
// trending 2024-2026 para JAMstack, SPAs y apps móviles.
//
// Stack:
//   - Web: `enola/strapi:5.49.0` (Strapi v5, Node 20, production)  — TypeScript + panel admin precompilado.
//   - BD:  `postgres:16-alpine` en contenedor separado.
//          (PostgreSQL recomendado en producción; SQLite solo para dev).
//
// Naming (13.3 — prefijos distintos garantizan aislamiento Tor/Nginx):
//   - Web:        `strapi-{name}`
//   - DB:         `db-{name}-strapi`
//   - Network:    `enola_net_strapi_{name}`
//
// Paths (13.2):
//   - App:        `/srv/enola-strapi/{name}/app/`     → `/srv/app`
//   - DB:         `/srv/enola-strapi/{name}/db/`      → `/var/lib/postgresql/data`
//   - Secrets:    `/srv/enola-strapi/{name}/secrets/` → montados como archivos (modo 0700/0600)
//
// Secretos generados (sec-SEC-005):
//   - `db_password`      → password de Postgres (20 chars alfanumérico)
//   - `app_keys`         → 2 keys hex separadas por coma (requerido por Strapi)
//   - `api_token_salt`   → 32-char hex salt (seguridad JWT API tokens)
//   - `admin_jwt_secret` → 32-char hex secret (sesiones de panel admin)
//   - `jwt_secret`       → 32-char hex secret (autenticación de usuarios)
//
// Setup wizard (13.1): Strapi v4 muestra un wizard de creación de admin
// en el primer arranque. Antes de completar `/admin/`, puede devolver 200/302/500.
//
// Puerto interno: Strapi escucha en **1337** (puerto por defecto de Strapi).
// El binding host:contenedor mapea http_port (host) → 1337 (container).
//
// Docker binding (13.16): SIEMPRE `127.0.0.1` (lo aplica `DockerAdapter`).
//
// RAM mínima: 512 MB (Node.js + TypeScript runtime + panel admin generado).

use crate::domain::cms::{
    CmsCreateRequest, CmsDescriptor, CmsInstance, CmsKind, CmsStatus, DbStack,
};
use crate::domain::error::{EnolaError, Result};
use crate::ports::cms::{CmsAdapter, CmsLifecycle};
use crate::ports::container::{ContainerConfig, ContainerPort};
use crate::ports::manifest::ManifestPort;
use rand::Rng;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Puerto interno donde Strapi escucha dentro del contenedor.
const STRAPI_INTERNAL_PORT: u16 = 1337;

/// Directorio base por defecto para datos persistentes de Strapi.
const DEFAULT_STRAPI_BASE_DIR: &str = "/srv/enola-strapi";

/// Resuelve el directorio base. Tests pueden sobreescribir con
/// `ENOLA_STRAPI_BASE_DIR` (mismo patrón que Drupal/Ghost/Wagtail).
fn strapi_base_dir() -> PathBuf {
    #[cfg(test)]
    {
        if let Ok(dir) = std::env::var("ENOLA_STRAPI_BASE_DIR") {
            return PathBuf::from(dir);
        }
    }
    PathBuf::from(DEFAULT_STRAPI_BASE_DIR)
}

/// Adapter Strapi v4. Igual que Wagtail, solo necesita `ContainerPort`.
pub struct StrapiCmsAdapter {
    container_manager: Arc<dyn ContainerPort + Send + Sync>,
    manifest: Arc<dyn ManifestPort + Send + Sync>,
}

impl StrapiCmsAdapter {
    pub fn new(
        container_manager: Arc<dyn ContainerPort + Send + Sync>,
        manifest: Arc<dyn ManifestPort + Send + Sync>,
    ) -> Self {
        Self {
            container_manager,
            manifest,
        }
    }

    fn web_container_name(name: &str) -> String {
        format!("strapi-{}", name)
    }

    fn db_container_name(name: &str) -> String {
        format!("db-{}-strapi", name)
    }

    fn network_name(name: &str) -> String {
        format!("enola_net_strapi_{}", name)
    }

    fn validate_name(name: &str) -> Result<()> {
        if name.is_empty()
            || !name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Err(EnolaError::ValidationError(format!(
                "Invalid Strapi instance name '{}': only alphanumeric, '_' and '-' allowed",
                name
            )));
        }
        Ok(())
    }

    /// Genera una cadena alfanumérica aleatoria de `length` caracteres.
    fn generate_password(length: usize) -> String {
        rand::thread_rng()
            .sample_iter(&rand::distributions::Alphanumeric)
            .take(length)
            .map(char::from)
            .collect()
    }

    /// Genera una cadena hex aleatoria de `bytes` bytes (2*bytes caracteres).
    fn generate_hex_secret(bytes: usize) -> String {
        let raw: Vec<u8> = rand::thread_rng()
            .sample_iter(&rand::distributions::Standard)
            .take(bytes)
            .collect();
        raw.iter().map(|b| format!("{:02x}", b)).collect()
    }

    /// Genera el valor `APP_KEYS` de Strapi: 2 hex keys separadas por coma.
    /// Strapi v4 requiere mínimo 2 keys en APP_KEYS.
    fn generate_app_keys() -> String {
        let key1 = Self::generate_hex_secret(24);
        let key2 = Self::generate_hex_secret(24);
        format!("{},{}", key1, key2)
    }

    /// Extrae el puerto host HTTP desde la lista de puertos de Docker.
    /// Acepta `127.0.0.1:8080->1337/tcp` o `8080->1337/tcp`.
    /// NO matchea con 80 (WP/Drupal), 2368 (Ghost) ni 8000 (Wagtail).
    fn parse_http_port(ports: &[String]) -> Option<u16> {
        let needle = format!("->{}/tcp", STRAPI_INTERNAL_PORT);
        for entry in ports {
            if let Some(prefix) = entry.split(needle.as_str()).next() {
                if prefix == entry.as_str() {
                    continue;
                }
                let host_part = prefix.rsplit(':').next().unwrap_or(prefix);
                if let Ok(p) = host_part.trim().parse::<u16>() {
                    return Some(p);
                }
            }
        }
        None
    }
}

impl CmsAdapter for StrapiCmsAdapter {
    fn descriptor(&self) -> CmsDescriptor {
        strapi_descriptor()
    }
}

/// Descriptor estático compartido entre el adapter en runtime y el catálogo
/// (`adapters::cms::catalog_descriptors`). Mantener sincronizado.
pub(crate) fn strapi_descriptor() -> CmsDescriptor {
    CmsDescriptor {
        kind: CmsKind::Strapi,
        display_name: "Strapi",
        default_image: "enola/strapi:5.49.0",
        db_stack: DbStack::Postgres,
        // 13.1: Strapi en el primer arranque puede devolver 500 mientras
        // aplica migraciones de BD; el admin wizard vive en `/admin/` (302).
        setup_wizard_status_codes: &[200, 301, 302, 304, 500],
        container_prefix: "strapi-",
        data_root: "/srv/enola-strapi",
        http_port_range: (8000, 9999),
    }
}

#[async_trait::async_trait]
impl CmsLifecycle for StrapiCmsAdapter {
    async fn create(&self, request: CmsCreateRequest) -> Result<CmsInstance> {
        Self::validate_name(&request.name)?;

        let http_port = request.http_port.ok_or_else(|| {
            EnolaError::ValidationError(
                "StrapiCmsAdapter.create() requires an explicit http_port \
                 (use PortValidator at the CLI layer)"
                    .to_string(),
            )
        })?;

        let web_name = Self::web_container_name(&request.name);
        let db_name = Self::db_container_name(&request.name);
        let net_name = Self::network_name(&request.name);

        // 1. Network (idempotente).
        let _ = self.container_manager.create_network(&net_name).await;
        let _ = self.manifest.append("docker_network", &net_name);

        // 2. Secretos (mismo patrón SEC-005 que WP/Drupal/Wagtail).
        let base = strapi_base_dir();
        let inst_dir = base.join(&request.name);
        let secrets_dir = inst_dir.join("secrets");

        let db_pass = request
            .db_password
            .clone()
            .unwrap_or_else(|| Self::generate_password(20));
        let app_keys = Self::generate_app_keys();
        let api_token_salt = Self::generate_hex_secret(16);
        let admin_jwt_secret = Self::generate_hex_secret(16);
        let jwt_secret = Self::generate_hex_secret(16);
        let transfer_token_salt = Self::generate_hex_secret(16);

        // Write secrets to disk for user audit (not consumed by container).
        let db_pass_path = write_secret_file(&secrets_dir, "db_password", &db_pass)?;
        let _app_keys_path = write_secret_file(&secrets_dir, "app_keys", &app_keys)?;
        let _api_token_salt_path =
            write_secret_file(&secrets_dir, "api_token_salt", &api_token_salt)?;
        let _admin_jwt_path =
            write_secret_file(&secrets_dir, "admin_jwt_secret", &admin_jwt_secret)?;
        let _jwt_secret_path = write_secret_file(&secrets_dir, "jwt_secret", &jwt_secret)?;
        let _transfer_token_path =
            write_secret_file(&secrets_dir, "transfer_token_salt", &transfer_token_salt)?;

        // 3. Volúmenes persistentes.
        let db_volume = inst_dir.join("db");

        // 4. Contenedor BD (Postgres 16).
        let mut db_env = HashMap::new();
        db_env.insert("POSTGRES_DB".to_string(), "strapi".to_string());
        db_env.insert("POSTGRES_USER".to_string(), "strapi".to_string());
        db_env.insert(
            "POSTGRES_PASSWORD_FILE".to_string(),
            "/run/secrets/db_password".to_string(),
        );

        let mut db_volumes = HashMap::new();
        db_volumes.insert(
            db_volume.to_string_lossy().to_string(),
            "/var/lib/postgresql/data".to_string(),
        );

        let mut db_secrets = HashMap::new();
        db_secrets.insert(
            "db_password".to_string(),
            db_pass_path.to_string_lossy().to_string(),
        );

        let db_image = self
            .descriptor()
            .db_stack
            .default_image()
            .ok_or_else(|| {
                EnolaError::InfrastructureError(
                    "Strapi descriptor missing DB image (DbStack::Postgres)".to_string(),
                )
            })?
            .to_string();

        let db_config = ContainerConfig {
            name: db_name.clone(),
            image: db_image,
            command: None,
            env: db_env,
            ports: HashMap::new(),
            volumes: db_volumes,
            network: Some(net_name.clone()),
            restart_policy: Some("unless-stopped".to_string()),
            gpu_support: false,
            auto_remove: false,
            working_dir: None,
            security_opt: Vec::new(),
            memory_limit: None,
            nano_cpus: None,
            pids_limit: None,
            secrets: db_secrets,
            // SEC-019: DB needs write access
            read_only_rootfs: false,
            no_new_privileges: true,
            ..Default::default()
        };
        self.container_manager.create_container(db_config).await?;
        self.container_manager.start_container(&db_name).await?;
        let _ = self.manifest.append("docker_container", &db_name);

        // 5. Contenedor Strapi.
        // The production image bakes the app at /srv/app. We must NOT mount a
        // volume over it (that would hide the baked-in code). Instead, mount
        // only public/uploads for persistent user-uploaded media files.
        let uploads_volume = inst_dir.join("uploads");

        let mut web_ports = HashMap::new();
        web_ports.insert(http_port, STRAPI_INTERNAL_PORT);

        // Strapi image expects env vars directly (not _FILE pattern).
        // SEC-001: Secrets are mounted as Docker secrets at /run/secrets/ and
        // injected via an entrypoint wrapper script that reads them and exports
        // as env vars before starting Strapi. This avoids plaintext secrets in
        // the container's env (visible via `docker inspect`).
        let mut web_env = HashMap::new();
        web_env.insert("DATABASE_CLIENT".to_string(), "postgres".to_string());
        web_env.insert("DATABASE_HOST".to_string(), db_name.clone());
        web_env.insert("DATABASE_PORT".to_string(), "5432".to_string());
        web_env.insert("DATABASE_NAME".to_string(), "strapi".to_string());
        web_env.insert("DATABASE_USERNAME".to_string(), "strapi".to_string());
        web_env.insert("DATABASE_SSL".to_string(), "false".to_string());
        web_env.insert("NODE_ENV".to_string(), "production".to_string());
        web_env.insert("STRAPI_TELEMETRY_DISABLED".to_string(), "true".to_string());

        // SEC-001: Mount secrets as Docker secrets (read-only at /run/secrets/)
        let mut web_secrets = HashMap::new();
        web_secrets.insert(
            "db_password".to_string(),
            db_pass_path.to_string_lossy().to_string(),
        );
        web_secrets.insert(
            "app_keys".to_string(),
            _app_keys_path.to_string_lossy().to_string(),
        );
        web_secrets.insert(
            "api_token_salt".to_string(),
            _api_token_salt_path.to_string_lossy().to_string(),
        );
        web_secrets.insert(
            "admin_jwt_secret".to_string(),
            _admin_jwt_path.to_string_lossy().to_string(),
        );
        web_secrets.insert(
            "jwt_secret".to_string(),
            _jwt_secret_path.to_string_lossy().to_string(),
        );
        web_secrets.insert(
            "transfer_token_salt".to_string(),
            _transfer_token_path.to_string_lossy().to_string(),
        );

        // SEC-001: Entrypoint wrapper that reads secrets from /run/secrets/ and
        // exports them as env vars before exec'ing the original Strapi entrypoint.
        let entrypoint_script = r#"#!/bin/sh
# SEC-001: Read secrets from /run/secrets/ and export as env vars
export DATABASE_PASSWORD="$(cat /run/secrets/db_password)"
export APP_KEYS="$(cat /run/secrets/app_keys)"
export API_TOKEN_SALT="$(cat /run/secrets/api_token_salt)"
export ADMIN_JWT_SECRET="$(cat /run/secrets/admin_jwt_secret)"
export JWT_SECRET="$(cat /run/secrets/jwt_secret)"
export TRANSFER_TOKEN_SALT="$(cat /run/secrets/transfer_token_salt)"
exec "$@"
"#;
        let entrypoint_path = inst_dir.join("entrypoint.sh");
        std::fs::write(&entrypoint_path, entrypoint_script).map_err(|e| {
            EnolaError::InfrastructureError(format!("Cannot write entrypoint script: {}", e))
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&entrypoint_path, std::fs::Permissions::from_mode(0o755))
                .map_err(|e| {
                    EnolaError::InfrastructureError(format!(
                        "Cannot set entrypoint permissions: {}",
                        e
                    ))
                })?;
        }

        // Mount the entrypoint wrapper into the container
        let mut web_volumes = HashMap::new();
        web_volumes.insert(
            uploads_volume.to_string_lossy().to_string(),
            "/srv/app/public/uploads".to_string(),
        );
        web_volumes.insert(
            entrypoint_path.to_string_lossy().to_string(),
            "/entrypoint.sh".to_string(),
        );

        let web_config = ContainerConfig {
            name: web_name.clone(),
            image: self.descriptor().default_image.to_string(),
            // SEC-001: Use entrypoint wrapper to inject secrets from files
            command: Some(vec![
                "/bin/sh".to_string(),
                "/entrypoint.sh".to_string(),
                "npm".to_string(),
                "start".to_string(),
            ]),
            env: web_env,
            ports: web_ports,
            volumes: web_volumes,
            network: Some(net_name),
            restart_policy: Some("unless-stopped".to_string()),
            gpu_support: false,
            auto_remove: false,
            working_dir: None,
            security_opt: Vec::new(),
            memory_limit: None,
            nano_cpus: None,
            pids_limit: None,
            secrets: web_secrets,
            // SEC-019: Strapi needs write access for content
            read_only_rootfs: false,
            no_new_privileges: true,
            ..Default::default()
        };
        self.container_manager.create_container(web_config).await?;
        self.container_manager.start_container(&web_name).await?;
        let _ = self.manifest.append("docker_container", &web_name);

        Ok(CmsInstance {
            kind: CmsKind::Strapi,
            name: request.name,
            status: CmsStatus::Initializing, // Strapi migraciones + build admin
            http_port: Some(http_port),
            db_port: None,
            onion_address: None,
        })
    }

    async fn start(&self, name: &str) -> Result<()> {
        Self::validate_name(name)?;
        let db_name = Self::db_container_name(name);
        let web_name = Self::web_container_name(name);
        // Postgres primero para que Strapi no falle el connect inicial.
        self.container_manager.start_container(&db_name).await?;
        self.container_manager.start_container(&web_name).await?;
        Ok(())
    }

    async fn stop(&self, name: &str) -> Result<()> {
        Self::validate_name(name)?;
        let web_name = Self::web_container_name(name);
        let db_name = Self::db_container_name(name);
        let containers = self.container_manager.list_containers(true).await?;
        let exists = containers
            .iter()
            .any(|c| c.name == web_name || c.name == db_name);
        if !exists {
            return Err(EnolaError::NotFound(format!(
                "Strapi instance '{}' not found. Use `strapi list` to see existing instances.",
                name
            )));
        }
        // Strapi primero para no dejar conexiones colgadas a Postgres.
        let _ = self.container_manager.stop_container(&web_name).await;
        let _ = self.container_manager.stop_container(&db_name).await;
        Ok(())
    }

    async fn delete(&self, name: &str, force: bool) -> Result<()> {
        Self::validate_name(name)?;
        let web_name = Self::web_container_name(name);
        let db_name = Self::db_container_name(name);

        if !force {
            let containers = self.container_manager.list_containers(true).await?;
            let running: Vec<&str> = containers
                .iter()
                .filter(|c| {
                    (c.name == web_name || c.name == db_name)
                        && c.status.to_lowercase().starts_with("up")
                })
                .map(|c| c.name.as_str())
                .collect();
            if !running.is_empty() {
                return Err(EnolaError::ValidationError(format!(
                    "Cannot delete Strapi '{}': containers still running ({}). \
                     Use --force or stop first.",
                    name,
                    running.join(", ")
                )));
            }
        }

        let _ = self.container_manager.stop_container(&web_name).await;
        let _ = self.container_manager.stop_container(&db_name).await;
        let _ = self.container_manager.remove_container(&web_name).await;
        let _ = self.container_manager.remove_container(&db_name).await;
        let _ = self.manifest.remove("docker_container", &web_name);
        let _ = self.manifest.remove("docker_container", &db_name);
        // Remove the Docker network (same pattern as Drupal/Wagtail).
        let net_name = Self::network_name(name);
        let _ = self.container_manager.remove_network(&net_name).await;
        let _ = self.manifest.remove("docker_network", &net_name);
        // Clean up /srv data directory.
        let base = strapi_base_dir();
        let inst_dir = base.join(name);
        let _ = std::fs::remove_dir_all(&inst_dir);
        Ok(())
    }

    async fn status(&self, name: &str) -> Result<CmsInstance> {
        Self::validate_name(name)?;
        let web_name = Self::web_container_name(name);
        let db_name = Self::db_container_name(name);

        let containers = self.container_manager.list_containers(true).await?;
        let web = containers.iter().find(|c| c.name == web_name);
        let db = containers.iter().find(|c| c.name == db_name);

        // Estado agregado (13.5): Running solo si AMBOS contenedores están up.
        let status = match (web, db) {
            (None, None) => CmsStatus::NotFound,
            (Some(w), Some(d)) => {
                let w_up = w.status.to_lowercase().starts_with("up");
                let d_up = d.status.to_lowercase().starts_with("up");
                if w_up && d_up {
                    CmsStatus::Running
                } else {
                    CmsStatus::Stopped
                }
            }
            _ => CmsStatus::Stopped,
        };

        let http_port = web.and_then(|c| Self::parse_http_port(&c.ports));

        Ok(CmsInstance {
            kind: CmsKind::Strapi,
            name: name.to_string(),
            status,
            http_port,
            db_port: None,
            onion_address: super::read_onion_address(name),
        })
    }
}

/// Escribe un secreto en disco con permisos 0600 (mismo patrón SEC-005 que WP/Drupal/Wagtail).
/// Crea el directorio padre con modo 0700 si no existe.
fn write_secret_file(dir: &Path, name: &str, value: &str) -> Result<PathBuf> {
    use std::os::unix::fs::PermissionsExt;

    if !dir.exists() {
        std::fs::create_dir_all(dir).map_err(|e| {
            EnolaError::InfrastructureError(format!(
                "Failed to create Strapi secrets dir {}: {}",
                dir.display(),
                e
            ))
        })?;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).map_err(|e| {
            EnolaError::InfrastructureError(format!(
                "Failed to chmod Strapi secrets dir {}: {}",
                dir.display(),
                e
            ))
        })?;
    }
    let path = dir.join(name);
    std::fs::write(&path, value).map_err(|e| {
        EnolaError::InfrastructureError(format!(
            "Failed to write Strapi secret {}: {}",
            path.display(),
            e
        ))
    })?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).map_err(|e| {
        EnolaError::InfrastructureError(format!(
            "Failed to chmod Strapi secret {}: {}",
            path.display(),
            e
        ))
    })?;
    Ok(path)
}

#[cfg(test)]
#[allow(clippy::await_holding_lock)]
mod tests {
    use super::*;
    use crate::ports::container::{ContainerInfo, MockContainerPort};
    use crate::ports::manifest::MockManifestPort;
    use std::sync::Mutex;
    use tempfile::TempDir;

    fn mock_manifest() -> MockManifestPort {
        let mut m = MockManifestPort::new();
        m.expect_append().returning(|_, _| Ok(())).times(0..);
        m.expect_remove().returning(|_, _| Ok(())).times(0..);
        m
    }

    /// Mutex global para serializar mutaciones de ENOLA_STRAPI_BASE_DIR (13.33).
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn setup_test_base() -> (TempDir, std::sync::MutexGuard<'static, ()>) {
        let guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("ENOLA_STRAPI_BASE_DIR", tmp.path());
        (tmp, guard)
    }

    fn teardown_test_base(_tmp: TempDir, _guard: std::sync::MutexGuard<'static, ()>) {
        std::env::remove_var("ENOLA_STRAPI_BASE_DIR");
    }

    #[test]
    fn descriptor_matches_strapi_metadata() {
        let mock = MockContainerPort::new();
        let adapter = StrapiCmsAdapter::new(Arc::new(mock), Arc::new(mock_manifest()));
        let d = adapter.descriptor();
        assert_eq!(d.kind, CmsKind::Strapi);
        assert_eq!(d.kind.slug(), "strapi");
        assert_eq!(d.default_image, "enola/strapi:5.49.0");
        assert_eq!(d.db_stack, DbStack::Postgres);
        assert_eq!(d.container_prefix, "strapi-");
        assert_eq!(d.data_root, "/srv/enola-strapi");
        assert!(d.requires_db(), "Strapi+Postgres requires external DB");
        assert!(d.setup_wizard_status_codes.contains(&200));
        assert!(d.setup_wizard_status_codes.contains(&302));
    }

    #[test]
    fn validate_name_rejects_invalid_chars() {
        assert!(StrapiCmsAdapter::validate_name("").is_err());
        assert!(StrapiCmsAdapter::validate_name("my site").is_err());
        assert!(StrapiCmsAdapter::validate_name("my/api").is_err());
        assert!(StrapiCmsAdapter::validate_name("my$api").is_err());
        assert!(StrapiCmsAdapter::validate_name("myapi").is_ok());
        assert!(StrapiCmsAdapter::validate_name("my_api-1").is_ok());
    }

    #[test]
    fn naming_uses_strapi_prefix_and_does_not_collide_with_other_cms() {
        // 13.3 + 13.41 — prefijos distintos garantizan aislamiento Tor/Nginx.
        assert_eq!(StrapiCmsAdapter::web_container_name("blog"), "strapi-blog");
        assert_eq!(
            StrapiCmsAdapter::db_container_name("blog"),
            "db-blog-strapi"
        );
        assert_eq!(
            StrapiCmsAdapter::network_name("blog"),
            "enola_net_strapi_blog"
        );
        // Anti-regresión: strapi-{name} != wp/drupal/ghost/wagtail-{name}.
        let api = "api";
        assert_ne!(
            StrapiCmsAdapter::web_container_name(api),
            format!("wp-{}", api)
        );
        assert_ne!(
            StrapiCmsAdapter::web_container_name(api),
            format!("drupal-{}", api)
        );
        assert_ne!(
            StrapiCmsAdapter::web_container_name(api),
            format!("ghost-{}", api)
        );
        assert_ne!(
            StrapiCmsAdapter::web_container_name(api),
            format!("wagtail-{}", api)
        );
        // El sufijo `-strapi` evita colisión con db-{name}-drupal/wagtail.
        assert_ne!(
            StrapiCmsAdapter::db_container_name(api),
            format!("db-{}-drupal", api)
        );
        assert_ne!(
            StrapiCmsAdapter::db_container_name(api),
            format!("db-{}-wagtail", api)
        );
    }

    #[test]
    fn parse_http_port_handles_127001_format() {
        let ports = vec!["127.0.0.1:8085->1337/tcp".to_string()];
        assert_eq!(StrapiCmsAdapter::parse_http_port(&ports), Some(8085));
    }

    #[test]
    fn parse_http_port_handles_short_format() {
        let ports = vec!["8090->1337/tcp".to_string()];
        assert_eq!(StrapiCmsAdapter::parse_http_port(&ports), Some(8090));
    }

    #[test]
    fn parse_http_port_returns_none_for_unrelated_ports() {
        // Puerto 80 (WP/Drupal) NO debe matchear con Strapi (1337).
        let ports = vec!["8080->80/tcp".to_string()];
        assert_eq!(StrapiCmsAdapter::parse_http_port(&ports), None);
        // Puerto 2368 (Ghost) tampoco.
        let ports = vec!["8080->2368/tcp".to_string()];
        assert_eq!(StrapiCmsAdapter::parse_http_port(&ports), None);
        // Puerto 8000 (Wagtail) tampoco.
        let ports = vec!["8080->8000/tcp".to_string()];
        assert_eq!(StrapiCmsAdapter::parse_http_port(&ports), None);
    }

    #[test]
    fn generate_app_keys_produces_two_comma_separated_keys() {
        let keys = StrapiCmsAdapter::generate_app_keys();
        let parts: Vec<&str> = keys.split(',').collect();
        assert_eq!(parts.len(), 2, "APP_KEYS must have exactly 2 parts");
        assert!(!parts[0].is_empty());
        assert!(!parts[1].is_empty());
        // Las dos keys deben ser distintas.
        assert_ne!(parts[0], parts[1]);
    }

    #[test]
    fn generate_hex_secret_returns_hex_string_of_expected_length() {
        let s = StrapiCmsAdapter::generate_hex_secret(16);
        assert_eq!(s.len(), 32, "16 bytes = 32 hex chars");
        assert!(
            s.chars().all(|c| c.is_ascii_hexdigit()),
            "must be hex: {}",
            s
        );
    }

    #[tokio::test]
    async fn create_rejects_missing_http_port() {
        let mock = MockContainerPort::new();
        let adapter = StrapiCmsAdapter::new(Arc::new(mock), Arc::new(mock_manifest()));
        let req = CmsCreateRequest {
            name: "myapi".to_string(),
            http_port: None,
            db_password: None,
        };
        let r = adapter.create(req).await;
        assert!(r.is_err());
        let err = r.unwrap_err().to_string().to_lowercase();
        assert!(err.contains("http_port"), "got: {}", err);
    }

    #[tokio::test]
    async fn create_provisions_two_containers_postgres_and_returns_initializing() {
        let (tmp, guard) = setup_test_base();
        let mut mock = MockContainerPort::new();
        mock.expect_create_network().returning(|_| Ok(()));
        // Postgres ⇒ DOS create_container (web + db).
        mock.expect_create_container()
            .times(2)
            .returning(|c| Ok(c.name));
        mock.expect_start_container().times(2).returning(|_| Ok(()));

        let adapter = StrapiCmsAdapter::new(Arc::new(mock), Arc::new(mock_manifest()));
        let req = CmsCreateRequest {
            name: "myapi".to_string(),
            http_port: Some(8085),
            db_password: Some("custompass".to_string()),
        };
        let inst = adapter.create(req).await.expect("create should succeed");
        assert_eq!(inst.kind, CmsKind::Strapi);
        assert_eq!(inst.name, "myapi");
        assert_eq!(inst.status, CmsStatus::Initializing);
        assert_eq!(inst.http_port, Some(8085));
        assert_eq!(inst.db_port, None);
        assert!(inst.onion_address.is_none());
        teardown_test_base(tmp, guard);
    }

    #[tokio::test]
    async fn create_generates_all_six_secret_files_and_entrypoint() {
        let (tmp, guard) = setup_test_base();
        let mut mock = MockContainerPort::new();
        mock.expect_create_network().returning(|_| Ok(()));
        mock.expect_create_container()
            .times(2)
            .returning(|c| Ok(c.name));
        mock.expect_start_container().times(2).returning(|_| Ok(()));

        let adapter = StrapiCmsAdapter::new(Arc::new(mock), Arc::new(mock_manifest()));
        let req = CmsCreateRequest {
            name: "sectest".to_string(),
            http_port: Some(8086),
            db_password: None,
        };
        adapter.create(req).await.expect("create should succeed");

        // SEC-001: Verificar que los 6 archivos de secreto se escribieron correctamente.
        let secrets_dir = tmp.path().join("sectest").join("secrets");
        for secret_name in &[
            "db_password",
            "app_keys",
            "api_token_salt",
            "admin_jwt_secret",
            "jwt_secret",
            "transfer_token_salt",
        ] {
            let path = secrets_dir.join(secret_name);
            assert!(path.exists(), "Secret file missing: {}", secret_name);
            let content = std::fs::read_to_string(&path).unwrap();
            assert!(!content.is_empty(), "Secret file empty: {}", secret_name);
        }

        // SEC-001: Verificar que el entrypoint wrapper se generó y es ejecutable.
        let entrypoint = tmp.path().join("sectest").join("entrypoint.sh");
        assert!(entrypoint.exists(), "entrypoint.sh missing");
        let script = std::fs::read_to_string(&entrypoint).unwrap();
        assert!(
            script.contains("/run/secrets/db_password"),
            "entrypoint missing db_password"
        );
        assert!(
            script.contains("/run/secrets/transfer_token_salt"),
            "entrypoint missing transfer_token_salt"
        );
        assert!(script.contains("exec \"$@\""), "entrypoint missing exec");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&entrypoint).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o755, "entrypoint.sh should be 0755, got {:o}", mode);
        }

        teardown_test_base(tmp, guard);
    }

    #[tokio::test]
    async fn status_returns_not_found_when_no_containers() {
        let mut mock = MockContainerPort::new();
        mock.expect_list_containers().returning(|_| Ok(vec![]));
        let adapter = StrapiCmsAdapter::new(Arc::new(mock), Arc::new(mock_manifest()));
        let inst = adapter.status("myapi").await.unwrap();
        assert_eq!(inst.status, CmsStatus::NotFound);
        assert!(inst.http_port.is_none());
    }

    #[tokio::test]
    async fn status_returns_running_only_when_both_up() {
        let mut mock = MockContainerPort::new();
        mock.expect_list_containers().returning(|_| {
            Ok(vec![
                ContainerInfo {
                    id: "1".into(),
                    name: "strapi-myapi".into(),
                    image: "enola/strapi:5.49.0".into(),
                    status: "Up 2 minutes".into(),
                    ports: vec!["127.0.0.1:8085->1337/tcp".into()],
                },
                ContainerInfo {
                    id: "2".into(),
                    name: "db-myapi-strapi".into(),
                    image: "postgres:16-alpine".into(),
                    status: "Up 2 minutes".into(),
                    ports: vec![],
                },
            ])
        });
        let adapter = StrapiCmsAdapter::new(Arc::new(mock), Arc::new(mock_manifest()));
        let inst = adapter.status("myapi").await.unwrap();
        assert_eq!(inst.status, CmsStatus::Running);
        assert_eq!(inst.http_port, Some(8085));
    }

    #[tokio::test]
    async fn status_returns_stopped_when_only_web_up() {
        // Si Postgres está caído, Strapi no funciona — mostramos Stopped.
        let mut mock = MockContainerPort::new();
        mock.expect_list_containers().returning(|_| {
            Ok(vec![
                ContainerInfo {
                    id: "1".into(),
                    name: "strapi-myapi".into(),
                    image: "enola/strapi:5.49.0".into(),
                    status: "Up 2 minutes".into(),
                    ports: vec!["127.0.0.1:8085->1337/tcp".into()],
                },
                ContainerInfo {
                    id: "2".into(),
                    name: "db-myapi-strapi".into(),
                    image: "postgres:16-alpine".into(),
                    status: "Exited (0) 1 minute ago".into(),
                    ports: vec![],
                },
            ])
        });
        let adapter = StrapiCmsAdapter::new(Arc::new(mock), Arc::new(mock_manifest()));
        let inst = adapter.status("myapi").await.unwrap();
        assert_eq!(inst.status, CmsStatus::Stopped);
    }

    #[tokio::test]
    async fn delete_without_force_fails_if_running() {
        let mut mock = MockContainerPort::new();
        mock.expect_list_containers().returning(|_| {
            Ok(vec![ContainerInfo {
                id: "1".into(),
                name: "strapi-myapi".into(),
                image: "enola/strapi:5.49.0".into(),
                status: "Up 1 minute".into(),
                ports: vec!["127.0.0.1:8085->1337/tcp".into()],
            }])
        });
        let adapter = StrapiCmsAdapter::new(Arc::new(mock), Arc::new(mock_manifest()));
        let r = adapter.delete("myapi", false).await;
        assert!(r.is_err());
        let msg = r.unwrap_err().to_string().to_lowercase();
        assert!(msg.contains("running") || msg.contains("--force"));
    }

    #[tokio::test]
    async fn delete_with_force_removes_both_containers() {
        let mut mock = MockContainerPort::new();
        mock.expect_list_containers().times(0);
        mock.expect_stop_container().times(2).returning(|_| Ok(()));
        mock.expect_remove_container()
            .times(2)
            .returning(|_| Ok(()));
        mock.expect_remove_network().returning(|_| Ok(()));
        let adapter = StrapiCmsAdapter::new(Arc::new(mock), Arc::new(mock_manifest()));
        let r = adapter.delete("myapi", true).await;
        assert!(r.is_ok());
    }

    #[tokio::test]
    async fn start_starts_db_before_web() {
        let mut mock = MockContainerPort::new();
        let calls = std::sync::Arc::new(Mutex::new(Vec::<String>::new()));
        let calls_clone = calls.clone();
        mock.expect_start_container().returning(move |id| {
            calls_clone.lock().unwrap().push(id.to_string());
            Ok(())
        });
        let adapter = StrapiCmsAdapter::new(Arc::new(mock), Arc::new(mock_manifest()));
        adapter.start("myapi").await.unwrap();
        let recorded = calls.lock().unwrap().clone();
        // Postgres primero para que Strapi no falle el connect inicial.
        assert_eq!(recorded.len(), 2);
        assert_eq!(recorded[0], "db-myapi-strapi");
        assert_eq!(recorded[1], "strapi-myapi");
    }

    #[tokio::test]
    async fn stop_stops_web_before_db() {
        let mut mock = MockContainerPort::new();
        let calls = std::sync::Arc::new(Mutex::new(Vec::<String>::new()));
        let calls_clone = calls.clone();
        mock.expect_list_containers().returning(|_| {
            Ok(vec![
                crate::ports::container::ContainerInfo {
                    id: "a".into(),
                    name: "strapi-myapi".into(),
                    image: "strapi".into(),
                    status: "Up".into(),
                    ports: vec![],
                },
                crate::ports::container::ContainerInfo {
                    id: "b".into(),
                    name: "db-myapi-strapi".into(),
                    image: "postgres".into(),
                    status: "Up".into(),
                    ports: vec![],
                },
            ])
        });
        mock.expect_stop_container().returning(move |id| {
            calls_clone.lock().unwrap().push(id.to_string());
            Ok(())
        });
        let adapter = StrapiCmsAdapter::new(Arc::new(mock), Arc::new(mock_manifest()));
        adapter.stop("myapi").await.unwrap();
        let recorded = calls.lock().unwrap().clone();
        // Strapi primero para no dejar conexiones colgadas a Postgres.
        assert_eq!(recorded[0], "strapi-myapi");
        assert_eq!(recorded[1], "db-myapi-strapi");
    }
}
