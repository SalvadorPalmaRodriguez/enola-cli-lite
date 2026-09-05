use crate::domain::error::{EnolaError, Result};
use crate::ports::container::{ContainerConfig, ContainerPort};
use crate::ports::manifest::ManifestPort;
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
}

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
        }
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
        let secrets_dir = base_dir.join(format!("{}_secrets", service_name));
        let db_root_pass_path = write_secret_file(&secrets_dir, "db_root_password", &db_root_pass)?;
        let db_pass_path = write_secret_file(&secrets_dir, "db_password", &db_pass)?;

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
            image: "mariadb:10.6".to_string(),
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
        let mut wp_volumes = HashMap::new();
        wp_volumes.insert(
            wp_volume_path.to_string_lossy().to_string(),
            "/var/www/html".to_string(),
        );

        let mut wp_ports = HashMap::new();
        wp_ports.insert(http_port, 80);

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
            db_pass_path.to_string_lossy().to_string(),
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
            image: "wordpress:latest".to_string(),
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
            // SEC-019: WordPress needs write access for uploads/plugins
            read_only_rootfs: false,
            no_new_privileges: true,
            ..Default::default()
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

/// SEC-005: Write a secret value to a file with restricted permissions (0644).
/// Creates the parent directory with mode 0700 if it doesn't exist.
/// Returns the absolute path to the created file.
///
/// Permissions are 0644 (not 0600) because Docker bind-mounts preserve host
/// permissions. Inside the WordPress container, Apache runs as www-data (UID 33).
/// With 0600, PHP cannot read the secret file → "Permission denied" →
/// "Error establishing a database connection". The secrets directory itself
/// is 0700 root:root, so only root can traverse into it from the host.
/// Inside the container, the bind-mount makes the file readable by www-data.
fn write_secret_file(dir: &Path, name: &str, value: &str) -> Result<PathBuf> {
    use std::os::unix::fs::PermissionsExt;

    // Create secrets directory with restricted permissions
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
    // Escritura atómica con 0o644 desde el primer instante.
    // 0o644 es intencional: el contenedor Docker lee via bind-mount como www-data.
    crate::infrastructure::atomic_secret_file::write_atomic(&file_path, value.as_bytes(), 0o644)
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

    /// Mutex to serialize tests that modify `ENOLA_WORDPRESS_BASE_DIR`.
    /// Prevents race conditions when parallel tests read/write the same env var.
    static WP_BASE_DIR_LOCK: Mutex<()> = Mutex::new(());

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
    }

    impl MockContainerManager {
        fn new() -> Self {
            Self {
                should_fail: false,
                created_containers: Mutex::new(vec![]),
                started_containers: Mutex::new(vec![]),
            }
        }

        fn failing() -> Self {
            Self {
                should_fail: true,
                created_containers: Mutex::new(vec![]),
                started_containers: Mutex::new(vec![]),
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
            Ok(true)
        }
        async fn build_image(
            &self,
            _config: crate::ports::container::ImageBuildConfig,
        ) -> Result<String> {
            Ok("mock:latest".into())
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
        async fn generate_client_keys(&self, _: &str) -> Result<(String, String)> {
            Ok(("priv".into(), "pub".into()))
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

    #[tokio::test]
    async fn test_deploy_wordpress_success() {
        let _guard = WP_BASE_DIR_LOCK.lock().unwrap();
        let _tmp = setup_test_base_dir();

        let container = Arc::new(MockContainerManager::new());
        let tor = Arc::new(MockTorManager::new());
        let use_case = DeployWordPress::new(container.clone(), tor, Arc::new(mock_manifest()));

        let result = use_case.execute("myblog", 8080).await;

        teardown_test_base_dir();

        assert!(result.is_ok());
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
        let use_case = DeployWordPress::new(container, tor, Arc::new(mock_manifest()));

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
        let use_case = DeployWordPress::new(container, tor, Arc::new(mock_manifest()));

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
        let _guard = WP_BASE_DIR_LOCK.lock().unwrap();
        let _tmp = setup_test_base_dir();

        let container = Arc::new(MockContainerManager::failing());
        let tor = Arc::new(MockTorManager::new());
        let use_case = DeployWordPress::new(container, tor, Arc::new(mock_manifest()));

        let result = use_case.execute("myblog", 8080).await;

        teardown_test_base_dir();

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_deploy_wordpress_tor_failure_is_graceful() {
        let _guard = WP_BASE_DIR_LOCK.lock().unwrap();
        let _tmp = setup_test_base_dir();

        // Tor is not called during create (manual publish model).
        // A failing Tor manager should not affect deployment at all.
        let container = Arc::new(MockContainerManager::new());
        let tor = Arc::new(MockTorManager::failing());
        let use_case = DeployWordPress::new(container, tor, Arc::new(mock_manifest()));

        let result = use_case.execute("myblog", 8080).await;

        teardown_test_base_dir();

        // Should succeed — Tor is not invoked during create
        assert!(result.is_ok());
        let address = result.unwrap();
        assert!(address.contains("localhost"));
    }

    #[tokio::test]
    async fn test_deploy_wordpress_valid_names() {
        let _guard = WP_BASE_DIR_LOCK.lock().unwrap();
        let _tmp = setup_test_base_dir();

        let container = Arc::new(MockContainerManager::new());
        let tor = Arc::new(MockTorManager::new());
        let use_case = DeployWordPress::new(container, tor, Arc::new(mock_manifest()));

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
