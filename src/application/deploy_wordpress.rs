use crate::domain::error::{EnolaError, Result};
use crate::ports::container::{ContainerConfig, ContainerPort, ImageBuildConfig};
use crate::ports::manifest::ManifestPort;
use crate::ports::service_user::ServiceUserPort;
use crate::ports::tor::TorManagerPort;
use rand::Rng;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Default base directory for WordPress data and secrets in production.
const DEFAULT_WORDPRESS_BASE_DIR: &str = "/srv/enola-wordpress";

/// Resolve the base directory for WordPress data/secrets.
///
/// - **Production**: always returns `/srv/enola-wordpress`.
/// - **Tests** (`#[cfg(test)]`): reads `ENOLA_WORDPRESS_BASE_DIR` env var, falling
///   back to the production default. This allows unit tests to point at a
///   `tempfile::tempdir()` so they don't need root privileges.
fn wordpress_base_dir() -> PathBuf {
    #[cfg(test)]
    {
        if let Ok(dir) = std::env::var("ENOLA_WORDPRESS_BASE_DIR") {
            return PathBuf::from(dir);
        }
    }
    PathBuf::from(DEFAULT_WORDPRESS_BASE_DIR)
}

/// Application Service for deploying WordPress + MySQL
/// Port logic from: scripts/wordpress/generate_wordpress.sh
pub struct DeployWordPress {
    container_manager: Arc<dyn ContainerPort + Send + Sync>,
    #[allow(dead_code)]
    tor_manager: Arc<dyn TorManagerPort + Send + Sync>,
    manifest: Arc<dyn ManifestPort + Send + Sync>,
    /// Usuario de sistema `enola-wordpress` (proceso Apache sin root).
    service_user: Arc<dyn ServiceUserPort + Send + Sync>,
    /// Directorio de extracción del contexto de build (solo tests; `None` = canónico).
    build_dir: Option<PathBuf>,
}

/// uid/gid del usuario `mysql` de la imagen oficial de MariaDB: su entrypoint
/// baja a él antes de leer los secretos `*_FILE`.
const MARIADB_UID_GID: u32 = 999;
/// Puerto interno de Apache en la imagen propia (>1024: sin `NET_BIND_SERVICE`).
pub(crate) const WORDPRESS_INTERNAL_PORT: u16 = 8080;

#[derive(Debug, Clone)]
pub struct DeployWordPressRequest {
    pub service_name: String, // e.g. "myblog"
    pub db_pass: Option<String>,
}

impl DeployWordPress {
    pub fn new(
        container_manager: Arc<dyn ContainerPort + Send + Sync>,
        tor_manager: Arc<dyn TorManagerPort + Send + Sync>,
        manifest: Arc<dyn ManifestPort + Send + Sync>,
    ) -> Self {
        Self {
            container_manager,
            tor_manager,
            manifest,
            service_user: Arc::new(crate::adapters::infra::service_user::SystemServiceUser::new()),
            build_dir: None,
        }
    }

    /// Constructor para tests: usuario de servicio simulado y directorio de build propio.
    #[cfg(test)]
    pub fn new_with_service_user(
        container_manager: Arc<dyn ContainerPort + Send + Sync>,
        tor_manager: Arc<dyn TorManagerPort + Send + Sync>,
        manifest: Arc<dyn ManifestPort + Send + Sync>,
        service_user: Arc<dyn ServiceUserPort + Send + Sync>,
        build_dir: PathBuf,
    ) -> Self {
        Self {
            container_manager,
            tor_manager,
            manifest,
            service_user,
            build_dir: Some(build_dir),
        }
    }

    /// Construye `enola/wordpress:<ver>` desde el contexto embebido si la imagen
    /// no existe. Se llama ANTES de tocar red, secretos o contenedores: un build
    /// fallido no deja residuos.
    async fn ensure_image(&self) -> Result<()> {
        use crate::infrastructure::embedded_scripts as emb;
        let tag = emb::WORDPRESS_IMAGE_TAG;
        if self.container_manager.image_exists(tag).await? {
            return Ok(());
        }
        let ctx = match &self.build_dir {
            Some(dir) => emb::ensure_wordpress_context_in(dir),
            None => emb::ensure_wordpress_context(),
        }
        .map_err(|e| {
            EnolaError::InfrastructureError(format!(
                "Cannot prepare the WordPress build context: {e}"
            ))
        })?;
        eprintln!(
            "🏗️  Building the WordPress image {tag} (first time only, ~1-3 min, needs Internet)..."
        );
        self.container_manager
            .build_image(ImageBuildConfig {
                dockerfile_path: ctx.join(emb::WORDPRESS_DOCKERFILE_NAME),
                context_path: ctx,
                tag: tag.to_string(),
                build_args: HashMap::new(),
            })
            .await?;
        Ok(())
    }

    pub async fn execute(&self, service_name: &str, http_port: u16) -> Result<String> {
        // 1. Validate Service Name
        if service_name.is_empty()
            || !service_name
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
        {
            return Err(EnolaError::ValidationError(format!(
                "Invalid service name: {}",
                service_name
            )));
        }

        // 1b. Imagen propia (build la primera vez), antes de crear nada.
        self.ensure_image().await?;

        let network_name = format!("enola_net_{}", service_name);

        // 2. Create Network
        // We attempt to create it. If it fails (exists), it's fine for now, or check specific error.
        // Docker adapter handles check_duplicate=true
        let _ = self.container_manager.create_network(&network_name).await;
        let _ = self.manifest.append("docker_network", &network_name);

        // 3. Prepare Database Container (MySQL/MariaDB)
        let db_pass = self.generate_password(16);
        let db_root_pass = self.generate_password(20);
        let db_name = format!("wordpress_{}", service_name);
        let db_user = format!("wp_{}", service_name);
        let db_host = format!("db-{}", service_name); // Use hyphen for container name

        // SEC-005: Write secrets to host files (mode 0600) instead of env vars.
        // MariaDB and WordPress official images support the _FILE suffix convention.
        let base_dir = wordpress_base_dir();
        // SEC-A7: directorio base de datos en 0700 (wp-config.php lo escribe el
        // contenedor con 0644).
        crate::infrastructure::safe_dir::secure_service_base_dir(&base_dir).map_err(|e| {
            EnolaError::InfrastructureError(format!(
                "Insecure data directory {}: {}",
                base_dir.display(),
                e
            ))
        })?;
        let secrets_dir = base_dir.join(format!("{}_secrets", service_name));
        // Cada secreto es 0600 y pertenece al usuario que lo lee dentro del contenedor:
        // MariaDB (uid 999) sus dos claves; Apache (`enola-wordpress`) su propia copia.
        let (wp_uid, wp_gid) = self
            .service_user
            .ensure(crate::domain::naming::WORDPRESS_SERVICE_USER)?;
        let db_root_pass_path = write_secret_file(&secrets_dir, "db_root_password", &db_root_pass)?;
        let db_pass_path = write_secret_file(&secrets_dir, "db_password", &db_pass)?;
        let wp_pass_path = write_secret_file(&secrets_dir, "wp_db_password", &db_pass)?;
        self.service_user
            .chown_tree(&db_root_pass_path, MARIADB_UID_GID, MARIADB_UID_GID)?;
        self.service_user
            .chown_tree(&db_pass_path, MARIADB_UID_GID, MARIADB_UID_GID)?;
        self.service_user
            .chown_tree(&wp_pass_path, wp_uid, wp_gid)?;

        let mut db_env = HashMap::new();
        // SEC-005: Use _FILE convention — MariaDB reads the secret from the mounted file
        db_env.insert(
            "MYSQL_ROOT_PASSWORD_FILE".to_string(),
            "/run/secrets/db_root_password".to_string(),
        );
        db_env.insert("MYSQL_DATABASE".to_string(), db_name.clone());
        db_env.insert("MYSQL_USER".to_string(), db_user.clone());
        db_env.insert(
            "MYSQL_PASSWORD_FILE".to_string(),
            "/run/secrets/db_password".to_string(),
        );

        // SEC-005: secrets as read-only bind mounts
        let mut db_secrets = HashMap::new();
        db_secrets.insert(
            "db_root_password".to_string(),
            db_root_pass_path.to_string_lossy().to_string(),
        );
        db_secrets.insert(
            "db_password".to_string(),
            db_pass_path.to_string_lossy().to_string(),
        );

        // Volume for DB
        let db_volume_path = base_dir.join(format!("{}_db", service_name));
        let mut db_volumes = HashMap::new();
        db_volumes.insert(
            db_volume_path.to_string_lossy().to_string(),
            "/var/lib/mysql".to_string(),
        );

        let db_config = ContainerConfig {
            name: db_host.clone(),
            image: crate::domain::images::MARIADB_WORDPRESS.image.to_string(),
            // A25-07/08: tag fijo + digest + cap_drop ALL / cap_add mínimo (domain::images).
            image_digest: crate::domain::images::MARIADB_WORDPRESS.digest_string(),
            cap_drop: crate::domain::images::ImageSpec::cap_drop_all(),
            cap_add: crate::domain::images::MARIADB_WORDPRESS.cap_add_vec(),
            command: None,
            env: db_env,
            ports: HashMap::new(), // Not exposed to host
            volumes: db_volumes,
            network: Some(network_name.clone()),
            restart_policy: Some("unless-stopped".to_string()),
            gpu_support: false,
            auto_remove: false,
            working_dir: None,
            security_opt: Vec::new(),
            memory_limit: None,
            nano_cpus: None,
            pids_limit: None,
            secrets: db_secrets,
            // SEC-019: MariaDB needs write access for DB
            read_only_rootfs: false,
            no_new_privileges: true,
            ..Default::default()
        };

        self.container_manager.create_container(db_config).await?;
        self.container_manager.start_container(&db_host).await?;
        let _ = self.manifest.append("docker_container", &db_host);

        // 4. Prepare WordPress Container
        let wp_volume_path = base_dir.join(format!("{}_wp", service_name));
        // El volumen lo escribe Apache (núcleo, plugins, temas, uploads): debe ser
        // del usuario de servicio antes de que Docker lo cree como root.
        std::fs::create_dir_all(&wp_volume_path).map_err(|e| {
            EnolaError::InfrastructureError(format!(
                "Cannot create {}: {}",
                wp_volume_path.display(),
                e
            ))
        })?;
        self.service_user
            .chown_tree(&wp_volume_path, wp_uid, wp_gid)?;
        let mut wp_volumes = HashMap::new();
        wp_volumes.insert(
            wp_volume_path.to_string_lossy().to_string(),
            "/var/www/html".to_string(),
        );

        let mut wp_ports = HashMap::new();
        wp_ports.insert(http_port, WORDPRESS_INTERNAL_PORT);

        let mut wp_env = HashMap::new();
        wp_env.insert("WORDPRESS_DB_HOST".to_string(), db_host);
        wp_env.insert("WORDPRESS_DB_USER".to_string(), db_user);
        // SEC-005: WordPress also supports _FILE convention
        wp_env.insert(
            "WORDPRESS_DB_PASSWORD_FILE".to_string(),
            "/run/secrets/db_password".to_string(),
        );
        wp_env.insert("WORDPRESS_DB_NAME".to_string(), db_name);

        // SEC-005: WordPress needs the same db_password secret
        let mut wp_secrets = HashMap::new();
        wp_secrets.insert(
            "db_password".to_string(),
            wp_pass_path.to_string_lossy().to_string(),
        );

        let wp_container_name = format!("wp-{}", service_name); // Use hyphen for container name

        // AA-002: inyectar perfil AppArmor si está activo en el kernel y cargado.
        let aa_profile = format!("enola-wp-{}", service_name);
        let mut wp_security_opt = Vec::new();
        if let Some(aa_opt) = crate::infrastructure::security_opt::apparmor_profile_opt(&aa_profile)
        {
            wp_security_opt.push(aa_opt);
        }

        let wp_config = ContainerConfig {
            name: wp_container_name.clone(),
            image: crate::domain::images::WORDPRESS.image.to_string(),
            // A25-07/08: tag fijo + digest + cap_drop ALL / cap_add mínimo (domain::images).
            image_digest: crate::domain::images::WORDPRESS.digest_string(),
            cap_drop: crate::domain::images::ImageSpec::cap_drop_all(),
            cap_add: crate::domain::images::WORDPRESS.cap_add_vec(),
            command: None,
            env: wp_env,
            ports: wp_ports,
            volumes: wp_volumes,
            network: Some(network_name),
            restart_policy: Some("unless-stopped".to_string()),
            gpu_support: false,
            auto_remove: false,
            working_dir: None,
            security_opt: wp_security_opt,
            memory_limit: None,
            nano_cpus: None,
            pids_limit: None,
            secrets: wp_secrets,
            user: Some(format!("{wp_uid}:{wp_gid}")),
            // SEC-019: WordPress needs write access for uploads/plugins
            read_only_rootfs: false,
            no_new_privileges: true,
        };

        self.container_manager.create_container(wp_config).await?;
        self.container_manager
            .start_container(&wp_container_name)
            .await?;
        let _ = self.manifest.append("docker_container", &wp_container_name);

        // Tor hidden service is NOT auto-published on create for security:
        // the WordPress setup wizard would be exposed on Tor before the user
        // configures the admin account. The user must run `wp publish <name>`
        // after completing the setup at http://127.0.0.1:{http_port}/.
        Ok(format!("localhost:{}", http_port))
    }

    fn generate_password(&self, length: usize) -> String {
        let rng = rand::thread_rng();
        rng.sample_iter(&rand::distributions::Alphanumeric)
            .take(length)
            .map(char::from)
            .collect()
    }
}

/// SEC-005: Write a secret value to a file with mode 0600.
/// Creates the parent directory with mode 0700 if it doesn't exist.
/// Returns the absolute path to the created file.
///
/// El llamador hace `chown` al usuario que lo lee dentro del contenedor
/// (MariaDB uid 999 / `enola-wordpress`): con 0600 nadie más puede leerlo.
fn write_secret_file(dir: &Path, name: &str, value: &str) -> Result<PathBuf> {
    use std::os::unix::fs::PermissionsExt;

    if !dir.exists() {
        std::fs::create_dir_all(dir).map_err(|e| {
            EnolaError::InfrastructureError(format!(
                "Failed to create secrets directory {}: {}",
                dir.display(),
                e
            ))
        })?;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).map_err(|e| {
            EnolaError::InfrastructureError(format!(
                "Failed to set permissions on {}: {}",
                dir.display(),
                e
            ))
        })?;
    }

    let file_path = dir.join(name);
    crate::infrastructure::atomic_secret_file::write_atomic(&file_path, value.as_bytes(), 0o600)
        .map_err(|e| {
            EnolaError::InfrastructureError(format!(
                "Failed to write secret file {}: {}",
                file_path.display(),
                e
            ))
        })?;

    Ok(file_path)
}

#[cfg(test)]
#[allow(clippy::await_holding_lock)]
mod tests {
    use super::*;
    use crate::domain::error::EnolaError;
    use crate::ports::container::{ContainerInfo, ContainerStats};
    use crate::ports::manifest::MockManifestPort;
    use crate::ports::tor::TorServiceInfo;
    use async_trait::async_trait;
    use std::sync::Mutex;

    fn mock_manifest() -> MockManifestPort {
        let mut m = MockManifestPort::new();
        m.expect_append().returning(|_, _| Ok(())).times(0..);
        m.expect_remove().returning(|_, _| Ok(())).times(0..);
        m
    }

    /// Helper: create a temporary directory and set `ENOLA_WORDPRESS_BASE_DIR` to it.
    /// Returns the `TempDir` handle — the directory is deleted when it goes out of scope.
    fn setup_test_base_dir() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().expect("failed to create temp dir for test");
        std::env::set_var("ENOLA_WORDPRESS_BASE_DIR", tmp.path());
        tmp
    }

    /// Helper: remove `ENOLA_WORDPRESS_BASE_DIR` after a test finishes.
    fn teardown_test_base_dir() {
        std::env::remove_var("ENOLA_WORDPRESS_BASE_DIR");
    }

    struct MockContainerManager {
        should_fail: bool,
        created_containers: Mutex<Vec<String>>,
        started_containers: Mutex<Vec<String>>,
        configs: Mutex<Vec<ContainerConfig>>,
        built_tags: Mutex<Vec<String>>,
        image_present: bool,
    }

    impl MockContainerManager {
        fn new() -> Self {
            Self {
                should_fail: false,
                created_containers: Mutex::new(vec![]),
                started_containers: Mutex::new(vec![]),
                configs: Mutex::new(vec![]),
                built_tags: Mutex::new(vec![]),
                image_present: true,
            }
        }

        fn failing() -> Self {
            Self {
                should_fail: true,
                created_containers: Mutex::new(vec![]),
                started_containers: Mutex::new(vec![]),
                configs: Mutex::new(vec![]),
                built_tags: Mutex::new(vec![]),
                image_present: true,
            }
        }
    }

    #[async_trait]
    impl ContainerPort for MockContainerManager {
        async fn list_containers(&self, _all: bool) -> Result<Vec<ContainerInfo>> {
            Ok(vec![])
        }
        async fn create_container(&self, config: ContainerConfig) -> Result<String> {
            if self.should_fail {
                Err(EnolaError::InfrastructureError("Create failed".into()))
            } else {
                self.created_containers
                    .lock()
                    .unwrap()
                    .push(config.name.clone());
                self.configs.lock().unwrap().push(config.clone());
                Ok(config.name)
            }
        }
        async fn start_container(&self, id: &str) -> Result<()> {
            if self.should_fail {
                Err(EnolaError::InfrastructureError("Start failed".into()))
            } else {
                self.started_containers.lock().unwrap().push(id.to_string());
                Ok(())
            }
        }
        async fn stop_container(&self, _id: &str) -> Result<()> {
            Ok(())
        }
        async fn remove_container(&self, _id: &str) -> Result<()> {
            Ok(())
        }
        async fn restart_container(&self, _id: &str) -> Result<()> {
            Ok(())
        }
        async fn get_logs(&self, _id: &str, _tail: usize) -> Result<String> {
            Ok("logs".into())
        }
        async fn inspect_container(&self, _id: &str) -> Result<HashMap<String, String>> {
            Ok(HashMap::new())
        }
        async fn execute_command(&self, _id: &str, _cmd: Vec<String>) -> Result<String> {
            Ok("output".into())
        }
        async fn create_network(&self, _name: &str) -> Result<()> {
            Ok(())
        }
        async fn remove_network(&self, _name: &str) -> Result<()> {
            Ok(())
        }
        async fn connect_container_to_network(
            &self,
            _network: &str,
            _container: &str,
        ) -> Result<()> {
            Ok(())
        }
        async fn image_exists(&self, _image: &str) -> Result<bool> {
            Ok(self.image_present)
        }
        async fn build_image(
            &self,
            config: crate::ports::container::ImageBuildConfig,
        ) -> Result<String> {
            self.built_tags.lock().unwrap().push(config.tag.clone());
            Ok(config.tag)
        }
        async fn run_ephemeral_container(&self, _config: ContainerConfig) -> Result<(i64, String)> {
            Ok((0, "success".into()))
        }
        async fn prune_system(&self) -> Result<()> {
            Ok(())
        }
        async fn pull_image(&self, _image: &str) -> Result<()> {
            Ok(())
        }
        async fn get_container_stats(&self, _id: &str) -> Result<ContainerStats> {
            Ok(ContainerStats::default())
        }
    }

    struct MockTorManager {
        should_fail: bool,
    }

    impl MockTorManager {
        fn new() -> Self {
            Self { should_fail: false }
        }

        fn failing() -> Self {
            Self { should_fail: true }
        }
    }

    #[async_trait]
    impl TorManagerPort for MockTorManager {
        async fn list_hidden_services(&self) -> Result<Vec<TorServiceInfo>> {
            Ok(vec![])
        }
        async fn deploy_hidden_service(&self, name: &str, _: Vec<(u16, u16)>) -> Result<String> {
            if self.should_fail {
                Err(EnolaError::InfrastructureError("Deploy failed".into()))
            } else {
                Ok(format!("{}.onion", name))
            }
        }
        async fn remove_hidden_service(&self, _: &str) -> Result<()> {
            Ok(())
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

    /// Caso de uso con usuario de servicio simulado (uid/gid 10042) y build en tmp.
    fn make_use_case(
        container: Arc<MockContainerManager>,
        tor: Arc<MockTorManager>,
    ) -> DeployWordPress {
        let mut user = crate::ports::service_user::MockServiceUserPort::new();
        user.expect_ensure()
            .returning(|_| Ok((10042, 10042)))
            .times(0..);
        user.expect_chown_tree()
            .returning(|_, _, _| Ok(()))
            .times(0..);
        let build = std::env::temp_dir().join("enola-wp-build-test");
        DeployWordPress::new_with_service_user(
            container,
            tor,
            Arc::new(mock_manifest()),
            Arc::new(user),
            build,
        )
    }

    /// PLAN-WORDPRESS-7-1: WordPress corre con el uid de servicio, sin root,
    /// en el puerto interno 8080 y con secretos 0600 propios de cada contenedor.
    #[tokio::test]
    async fn test_deploy_wordpress_runs_non_root_on_8080_with_0600_secrets() {
        use std::os::unix::fs::PermissionsExt;
        let _guard = crate::infrastructure::TEST_ENV_LOCK.lock().unwrap();
        let tmp = setup_test_base_dir();
        let container = Arc::new(MockContainerManager::new());
        let use_case = make_use_case(container.clone(), Arc::new(MockTorManager::new()));

        let result = use_case.execute("myblog", 8090).await;
        teardown_test_base_dir();
        assert!(result.is_ok());

        let configs = container.configs.lock().unwrap();
        let wp = configs.iter().find(|c| c.name == "wp-myblog").unwrap();
        assert_eq!(wp.user.as_deref(), Some("10042:10042"));
        assert_eq!(wp.ports.get(&8090), Some(&8080));
        assert!(wp.cap_add.is_empty());
        assert!(wp.no_new_privileges);
        assert_eq!(wp.cap_drop, vec!["ALL".to_string()]);
        assert_eq!(wp.image, crate::domain::images::WORDPRESS.image);
        // WordPress lee SU copia del secreto; MariaDB las suyas.
        let wp_secret = wp.secrets.get("db_password").unwrap();
        assert!(wp_secret.ends_with("wp_db_password"));
        let db = configs.iter().find(|c| c.name == "db-myblog").unwrap();
        assert!(db
            .secrets
            .get("db_password")
            .unwrap()
            .ends_with("/db_password"));
        for f in ["db_root_password", "db_password", "wp_db_password"] {
            let m = std::fs::metadata(tmp.path().join("myblog_secrets").join(f)).unwrap();
            assert_eq!(m.permissions().mode() & 0o777, 0o600, "{f}");
        }
    }

    /// La imagen propia se construye una sola vez, antes de crear nada.
    #[tokio::test]
    async fn test_deploy_wordpress_builds_missing_image_before_creating() {
        let _guard = crate::infrastructure::TEST_ENV_LOCK.lock().unwrap();
        let _tmp = setup_test_base_dir();
        let mut mock = MockContainerManager::new();
        mock.image_present = false;
        let container = Arc::new(mock);
        let use_case = make_use_case(container.clone(), Arc::new(MockTorManager::new()));

        let result = use_case.execute("myblog", 8090).await;
        teardown_test_base_dir();
        assert!(result.is_ok());
        assert_eq!(
            container.built_tags.lock().unwrap().as_slice(),
            [crate::domain::images::WORDPRESS.image.to_string()]
        );
    }

    #[tokio::test]
    async fn test_deploy_wordpress_success() {
        let _guard = crate::infrastructure::TEST_ENV_LOCK.lock().unwrap();
        let tmp = setup_test_base_dir();

        let container = Arc::new(MockContainerManager::new());
        let tor = Arc::new(MockTorManager::new());
        let use_case = make_use_case(container.clone(), tor);

        let result = use_case.execute("myblog", 8080).await;

        teardown_test_base_dir();

        assert!(result.is_ok());
        // SEC-A7: el directorio base de datos queda 0700.
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(tmp.path()).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o700, "wordpress base must be 0700, got {:o}", mode);
        }
        let address = result.unwrap();
        assert!(address.contains("localhost"));
        // Should have created 2 containers: db and wp
        let created = container.created_containers.lock().unwrap();
        assert_eq!(created.len(), 2);
        assert!(created.iter().any(|c| c.contains("db-")));
        assert!(created.iter().any(|c| c.contains("wp-")));
    }

    #[tokio::test]
    async fn test_deploy_wordpress_empty_name() {
        let container = Arc::new(MockContainerManager::new());
        let tor = Arc::new(MockTorManager::new());
        let use_case = make_use_case(container, tor);

        let result = use_case.execute("", 8080).await;

        assert!(result.is_err());
        match result {
            Err(EnolaError::ValidationError(msg)) => {
                assert!(msg.contains("Invalid service name"));
            }
            _ => panic!("Expected ValidationError"),
        }
    }

    #[tokio::test]
    async fn test_deploy_wordpress_invalid_name() {
        let container = Arc::new(MockContainerManager::new());
        let tor = Arc::new(MockTorManager::new());
        let use_case = make_use_case(container, tor);

        let result = use_case.execute("my blog!", 8080).await;

        assert!(result.is_err());
        match result {
            Err(EnolaError::ValidationError(msg)) => {
                assert!(msg.contains("Invalid service name"));
            }
            _ => panic!("Expected ValidationError"),
        }
    }

    #[tokio::test]
    async fn test_deploy_wordpress_container_failure() {
        let _guard = crate::infrastructure::TEST_ENV_LOCK.lock().unwrap();
        let _tmp = setup_test_base_dir();

        let container = Arc::new(MockContainerManager::failing());
        let tor = Arc::new(MockTorManager::new());
        let use_case = make_use_case(container, tor);

        let result = use_case.execute("myblog", 8080).await;

        teardown_test_base_dir();

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_deploy_wordpress_tor_failure_is_graceful() {
        let _guard = crate::infrastructure::TEST_ENV_LOCK.lock().unwrap();
        let _tmp = setup_test_base_dir();

        // Tor is not called during create (manual publish model).
        // A failing Tor manager should not affect deployment at all.
        let container = Arc::new(MockContainerManager::new());
        let tor = Arc::new(MockTorManager::failing());
        let use_case = make_use_case(container, tor);

        let result = use_case.execute("myblog", 8080).await;

        teardown_test_base_dir();

        // Should succeed — Tor is not invoked during create
        assert!(result.is_ok());
        let address = result.unwrap();
        assert!(address.contains("localhost"));
    }

    #[tokio::test]
    async fn test_deploy_wordpress_valid_names() {
        let _guard = crate::infrastructure::TEST_ENV_LOCK.lock().unwrap();
        let _tmp = setup_test_base_dir();

        let container = Arc::new(MockContainerManager::new());
        let tor = Arc::new(MockTorManager::new());
        let use_case = make_use_case(container, tor);

        // Test valid names with underscores and hyphens
        let result = use_case.execute("my_blog-1", 8080).await;

        teardown_test_base_dir();

        assert!(result.is_ok());
    }

    #[test]
    fn test_deploy_request_struct() {
        let request = DeployWordPressRequest {
            service_name: "test".to_string(),
            db_pass: Some("secret123".to_string()),
        };

        assert_eq!(request.service_name, "test");
        assert_eq!(request.db_pass, Some("secret123".to_string()));
    }
}
