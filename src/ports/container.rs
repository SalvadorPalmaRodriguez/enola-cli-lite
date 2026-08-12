use crate::domain::error::Result;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct ContainerInfo {
    pub id: String,
    pub name: String,
    pub image: String,
    pub status: String,     // Up, Exited, etc.
    pub ports: Vec<String>, // "80:80", etc.
}

#[derive(Debug, Clone)]
pub struct ContainerConfig {
    pub name: String,
    pub image: String,
    pub command: Option<Vec<String>>, // Added command
    pub env: HashMap<String, String>,
    pub ports: HashMap<u16, u16>,         // Host Port -> Container Port
    pub volumes: HashMap<String, String>, // Host Path -> Container Path
    pub network: Option<String>,          // Added network field
    pub restart_policy: Option<String>,
    pub gpu_support: bool,           // Added GPU request flag
    pub auto_remove: bool,           // --rm flag for ephemeral containers
    pub working_dir: Option<String>, // Container working directory
    /// AA-002: AppArmor / security-opt strings (e.g. "apparmor=enola-git-myservice")
    pub security_opt: Vec<String>,
    /// DK-002: Memory limit in bytes (e.g. 512 * 1024 * 1024 for 512MB). 0 = unlimited.
    pub memory_limit: Option<i64>,
    /// DK-002: CPU quota as nanoCPUs (e.g. 1_000_000_000 = 1 CPU). None = unlimited.
    pub nano_cpus: Option<i64>,
    /// DK-002: Max number of PIDs inside the container. None = unlimited.
    pub pids_limit: Option<i64>,
    /// SEC-005: Docker secrets as host file mounts.
    /// Key = secret name (mounted at /run/secrets/{name}), Value = absolute host file path.
    /// Files are mounted read-only inside the container.
    pub secrets: HashMap<String, String>,
    /// SEC-019: Mount container root filesystem as read-only.
    /// Default: true for services that don't need write access.
    /// For services that need write access, use explicit volumes.
    pub read_only_rootfs: bool,
    /// SEC-019: Prevent container from acquiring new privileges (e.g. via setuid binaries).
    /// Default: true for security hardening.
    pub no_new_privileges: bool,
    /// SEC-012: Linux capabilities to drop (e.g. ["ALL"] or ["NET_RAW", "SYS_ADMIN"]).
    /// Default: ["ALL"] — all capabilities dropped, services must add back only what they need.
    pub cap_drop: Vec<String>,
    /// SEC-012: Linux capabilities to add back after cap_drop (e.g. ["SETUID", "SETGID"]).
    /// Default: empty — no capabilities added back.
    pub cap_add: Vec<String>,
    /// SEC-013: Optional image digest (sha256) for immutable pinning.
    /// When set, the container is created with `image@sha256:...` instead of `image:tag`.
    pub image_digest: Option<String>,
}

impl Default for ContainerConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            image: String::new(),
            command: None,
            env: HashMap::new(),
            ports: HashMap::new(),
            volumes: HashMap::new(),
            network: None,
            restart_policy: None,
            gpu_support: false,
            auto_remove: false,
            working_dir: None,
            security_opt: Vec::new(),
            memory_limit: None,
            nano_cpus: None,
            pids_limit: None,
            secrets: HashMap::new(),
            // SEC-019: hardened defaults
            read_only_rootfs: true,
            no_new_privileges: true,
            // SEC-012: drop all capabilities by default
            cap_drop: vec!["ALL".to_string()],
            // SEC-012: add back CHOWN/SETUID/SETGID — most container images
            // (Forgejo, Ghost, MariaDB, WordPress, Drupal) have entrypoint
            // scripts that use chown/su-exec to set file ownership and drop
            // privileges, which requires these. Safe because
            // no-new-privileges:true is also set.
            cap_add: vec![
                "CHOWN".to_string(),
                "SETUID".to_string(),
                "SETGID".to_string(),
                "DAC_OVERRIDE".to_string(),
            ],
            // SEC-013: no digest pinning by default
            image_digest: None,
        }
    }
}

/// Configuration for building a Docker image
#[derive(Debug, Clone)]
pub struct ImageBuildConfig {
    pub dockerfile_path: PathBuf,
    pub context_path: PathBuf,
    pub tag: String,
    pub build_args: HashMap<String, String>,
}

#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait ContainerPort {
    /// List all containers (or only running ones if all=false)
    async fn list_containers(&self, all: bool) -> Result<Vec<ContainerInfo>>;
    async fn create_container(&self, config: ContainerConfig) -> Result<String>;
    async fn start_container(&self, id: &str) -> Result<()>;
    async fn stop_container(&self, id: &str) -> Result<()>;
    async fn remove_container(&self, id: &str) -> Result<()>;
    async fn restart_container(&self, id: &str) -> Result<()>;
    async fn get_logs(&self, id: &str, tail: usize) -> Result<String>;
    async fn inspect_container(&self, id: &str) -> Result<HashMap<String, String>>;
    async fn execute_command(&self, id: &str, cmd: Vec<String>) -> Result<String>;
    async fn create_network(&self, name: &str) -> Result<()>;
    async fn remove_network(&self, name: &str) -> Result<()>;
    async fn connect_container_to_network(&self, network: &str, container: &str) -> Result<()>;

    // Image management methods
    /// Check if a Docker image exists locally
    async fn image_exists(&self, image: &str) -> Result<bool>;

    /// Build a Docker image from a Dockerfile
    async fn build_image(&self, config: ImageBuildConfig) -> Result<String>;

    /// Run an ephemeral container (with --rm) and wait for completion
    /// Returns (exit_code, logs)
    async fn run_ephemeral_container(&self, config: ContainerConfig) -> Result<(i64, String)>;

    /// Prune stopped containers, dangling images, and unused volumes
    async fn prune_system(&self) -> Result<()>;
}
// But `impl Trait` in traits is tricky for dynamic dispatch (dyn ContainerPort).
// We likely want `dyn ContainerPort` for mocking.
// So `async_trait` crate is still the best way to get object safety for dyn traits.

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_container_list_empty() {
        let mut mock = MockContainerPort::new();
        mock.expect_list_containers().returning(|_| Ok(vec![]));
        assert!(mock.list_containers(false).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_mock_container_restart() {
        let mut mock = MockContainerPort::new();
        mock.expect_restart_container().returning(|_| Ok(()));
        assert!(mock.restart_container("test-container").await.is_ok());
    }

    #[test]
    fn test_container_config_default() {
        let config = ContainerConfig::default();
        assert!(config.name.is_empty());
        assert!(!config.gpu_support);
        assert!(!config.auto_remove);
        assert!(config.security_opt.is_empty());
        assert!(config.memory_limit.is_none());
        assert!(config.nano_cpus.is_none());
        assert!(config.pids_limit.is_none());
        assert!(config.secrets.is_empty());
        // SEC-019: defaults should be hardened
        assert!(config.read_only_rootfs);
        assert!(config.no_new_privileges);
        // SEC-012: default should drop ALL capabilities
        assert_eq!(config.cap_drop, vec!["ALL".to_string()]);
        // SEC-012: default should add back CHOWN/SETUID/SETGID/DAC_OVERRIDE for container entrypoints
        assert_eq!(
            config.cap_add,
            vec![
                "CHOWN".to_string(),
                "SETUID".to_string(),
                "SETGID".to_string(),
                "DAC_OVERRIDE".to_string()
            ]
        );
        // SEC-013: no digest by default
        assert!(config.image_digest.is_none());
    }

    #[test]
    fn test_container_config_with_security_and_limits() {
        let config = ContainerConfig {
            name: "test-secured".into(),
            image: "nginx:latest".into(),
            security_opt: vec!["apparmor=enola-git-test".into()],
            memory_limit: Some(512 * 1024 * 1024), // 512MB
            nano_cpus: Some(1_000_000_000),        // 1 CPU
            pids_limit: Some(256),
            ..Default::default()
        };
        assert_eq!(config.security_opt.len(), 1);
        assert_eq!(config.security_opt[0], "apparmor=enola-git-test");
        assert_eq!(config.memory_limit, Some(536_870_912));
        assert_eq!(config.nano_cpus, Some(1_000_000_000));
        assert_eq!(config.pids_limit, Some(256));
    }

    #[test]
    fn test_container_config_cap_drop_default() {
        let config = ContainerConfig::default();
        assert_eq!(config.cap_drop, vec!["ALL".to_string()]);
    }

    #[test]
    fn test_container_config_cap_drop_custom() {
        let config = ContainerConfig {
            name: "test-cap".into(),
            image: "nginx:latest".into(),
            cap_drop: vec!["ALL".to_string(), "NET_RAW".to_string()],
            ..Default::default()
        };
        assert_eq!(config.cap_drop.len(), 2);
        assert_eq!(config.cap_drop[0], "ALL");
    }

    #[test]
    fn test_container_config_image_digest() {
        let config = ContainerConfig {
            name: "test-digest".into(),
            image: "nginx:latest".into(),
            image_digest: Some("sha256:abcdef1234567890".to_string()),
            ..Default::default()
        };
        assert!(config.image_digest.is_some());
        assert!(config.image_digest.as_ref().unwrap().starts_with("sha256:"));
    }

    #[test]
    fn test_container_info_struct() {
        let info = ContainerInfo {
            id: "abc123".into(),
            name: "test".into(),
            image: "nginx:latest".into(),
            status: "Up".into(),
            ports: vec!["80:80".into()],
        };
        assert_eq!(info.name, "test");
    }
}
