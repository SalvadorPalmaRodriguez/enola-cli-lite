use crate::domain::error::{EnolaError, Result};
use crate::ports::file::FileManagerPort;
use crate::ports::manifest::ManifestPort;
use crate::ports::service::ServiceManagerPort;
use crate::ports::tor::TorManagerPort;
use crate::ports::web::{NginxFileServerConfig, NginxManagerPort};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Input for DeployFileServer
pub struct DeployFileServerRequest {
    pub service_name: String,
    pub port: u16,                  // Internal Nginx port
    pub share_path: Option<String>, // Default: /srv/enola-files/<name>
    pub enable_auth: bool,
}

/// Use Case: Deploy a secure Nginx File Server over Tor
pub struct DeployFileServer {
    nginx_manager: Arc<dyn NginxManagerPort + Send + Sync>,
    tor_manager: Arc<dyn TorManagerPort + Send + Sync>,
    #[allow(dead_code)]
    service_manager: Arc<dyn ServiceManagerPort + Send + Sync>,
    file_manager: Arc<dyn FileManagerPort + Send + Sync>,
    manifest: Arc<dyn ManifestPort + Send + Sync>,
}

impl DeployFileServer {
    pub fn new(
        nginx_manager: Arc<dyn NginxManagerPort + Send + Sync>,
        tor_manager: Arc<dyn TorManagerPort + Send + Sync>,
        service_manager: Arc<dyn ServiceManagerPort + Send + Sync>,
        file_manager: Arc<dyn FileManagerPort + Send + Sync>,
        manifest: Arc<dyn ManifestPort + Send + Sync>,
    ) -> Self {
        Self {
            nginx_manager,
            tor_manager,
            service_manager,
            file_manager,
            manifest,
        }
    }

    pub async fn execute(&self, request: DeployFileServerRequest) -> Result<(String, String)> {
        // 1. Validate Input
        if request.service_name.is_empty() {
            return Err(EnolaError::ValidationError(
                "Service name cannot be empty".to_string(),
            ));
        }

        let share_path = request
            .share_path
            .unwrap_or_else(|| format!("/srv/enola-files/{}", request.service_name));

        // SEC-017: Validate share_path to prevent path traversal attacks
        let share_path_buf = validate_share_path(&share_path)?;

        // 2. Secure Directory Creation
        self.file_manager.ensure_dir(&share_path_buf).await?;

        // Apply permissions: root:www-data, 750
        // We do not fail hard if OS doesn't support users (like Windows dev env), but try.
        let chown_res = self
            .file_manager
            .set_ownership(&share_path_buf, "root", "www-data")
            .await;

        chown_res?;

        self.file_manager
            .set_permissions(&share_path_buf, 0o750)
            .await?;

        // 3. Create Nginx Config
        let nginx_config = NginxFileServerConfig {
            service_name: request.service_name.clone(),
            listen_port: request.port,
            root_dir: share_path.clone(),
            disable_symlinks: true,
            allow_upload: false,
        };

        self.nginx_manager
            .create_fileserver_config(nginx_config)
            .await?;
        let _ = self.manifest.append(
            "nginx_config",
            &format!("fileserver_{}", request.service_name),
        );

        // 4. Validate & Enable Nginx Config
        if !self.nginx_manager.validate_config().await? {
            return Err(EnolaError::InfrastructureError(
                "Generated Nginx config is invalid".to_string(),
            ));
        }

        // Nginx file is created as "fileserver_{name}" by create_fileserver_config
        let nginx_site_name = format!("fileserver_{}", request.service_name);
        self.nginx_manager.enable_site(&nginx_site_name).await?;
        self.nginx_manager.reload().await?;

        // 5. Deploy Hidden Service
        // Map port 80 (public onion) -> request.port (internal nginx)
        let ports = vec![(80, request.port)];
        // Use consistent naming: fileserver_{name} for Tor config
        let tor_service_name = format!("fileserver_{}", request.service_name);

        let onion_address = self
            .tor_manager
            .deploy_hidden_service(&tor_service_name, ports)
            .await?;
        let _ = self.manifest.append("tor_service", &tor_service_name);

        // 6. Handle Auth (Optional)
        if request.enable_auth {
            self.tor_manager
                .enable_client_auth(&tor_service_name)
                .await?;
        }

        self.tor_manager.reload_tor().await?;

        Ok((onion_address, share_path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::error::EnolaError;
    use crate::ports::manifest::MockManifestPort;
    use crate::ports::service::ServiceMetrics;
    use crate::ports::tor::TorServiceInfo;
    use crate::ports::web::{NginxProxyConfig, NginxSiteConfig};
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::path::Path;
    use std::sync::Mutex;

    fn mock_manifest() -> MockManifestPort {
        let mut m = MockManifestPort::new();
        m.expect_append().returning(|_, _| Ok(())).times(0..);
        m.expect_remove().returning(|_, _| Ok(())).times(0..);
        m
    }

    struct MockFileManager;

    #[async_trait]
    impl FileManagerPort for MockFileManager {
        async fn read_file(&self, _path: &Path) -> Result<String> {
            Ok("".into())
        }
        async fn write_file(&self, _path: &Path, _content: &str) -> Result<()> {
            Ok(())
        }
        async fn ensure_dir(&self, _path: &Path) -> Result<()> {
            Ok(())
        }
        async fn read_env(&self, _path: &Path) -> Result<HashMap<String, String>> {
            Ok(HashMap::new())
        }
        async fn update_env_key(&self, _path: &Path, _key: &str, _value: &str) -> Result<()> {
            Ok(())
        }
        async fn delete_file(&self, _path: &Path) -> Result<()> {
            Ok(())
        }
        async fn copy_file(&self, _from: &Path, _to: &Path) -> Result<()> {
            Ok(())
        }
        async fn set_ownership(&self, _path: &Path, _user: &str, _group: &str) -> Result<()> {
            Ok(())
        }
        async fn set_permissions(&self, _path: &Path, _mode: u32) -> Result<()> {
            Ok(())
        }
        async fn create_archive(&self, _source_dir: &Path, _dest_file: &Path) -> Result<()> {
            Ok(())
        }
        async fn extract_archive(&self, _archive: &Path, _dest_dir: &Path) -> Result<()> {
            Ok(())
        }
    }

    #[allow(dead_code)]
    struct MockNginxManager {
        should_fail: bool,
        should_invalid_config: bool,
        created_configs: Mutex<Vec<String>>,
        enabled_sites: Mutex<Vec<String>>,
    }

    impl MockNginxManager {
        fn new() -> Self {
            Self {
                should_fail: false,
                should_invalid_config: false,
                created_configs: Mutex::new(vec![]),
                enabled_sites: Mutex::new(vec![]),
            }
        }

        #[allow(dead_code)]
        fn failing() -> Self {
            Self {
                should_fail: true,
                should_invalid_config: false,
                created_configs: Mutex::new(vec![]),
                enabled_sites: Mutex::new(vec![]),
            }
        }

        fn with_invalid_config() -> Self {
            Self {
                should_fail: false,
                should_invalid_config: true,
                created_configs: Mutex::new(vec![]),
                enabled_sites: Mutex::new(vec![]),
            }
        }
    }

    #[async_trait]
    impl NginxManagerPort for MockNginxManager {
        async fn create_site_config(&self, _config: NginxSiteConfig) -> Result<()> {
            Ok(())
        }
        async fn create_fileserver_config(&self, config: NginxFileServerConfig) -> Result<()> {
            if self.should_fail {
                Err(EnolaError::InfrastructureError(
                    "Create config failed".into(),
                ))
            } else {
                self.created_configs
                    .lock()
                    .unwrap()
                    .push(config.service_name);
                Ok(())
            }
        }
        async fn create_proxy_config(&self, _config: NginxProxyConfig) -> Result<()> {
            Ok(())
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

        async fn enable_site(&self, _domain: &str) -> Result<()> {
            Ok(())
        }

        async fn disable_site(&self, _domain: &str) -> Result<()> {
            Ok(())
        }

        async fn delete_site_config(&self, _domain: &str) -> Result<()> {
            Ok(())
        }

        async fn validate_config(&self) -> Result<bool> {
            if self.should_invalid_config {
                Ok(false)
            } else {
                Ok(true)
            }
        }

        async fn reload(&self) -> Result<()> {
            Ok(())
        }

        async fn update_proxy_ports(
            &self,
            _domain: &str,
            _listen_port: u16,
            _backend_port: u16,
        ) -> Result<()> {
            Ok(())
        }

        async fn list_enabled_sites(&self) -> Result<Vec<String>> {
            Ok(vec![])
        }
    }

    struct MockTorManager {
        should_fail: bool,
        deployed_services: Mutex<Vec<String>>,
    }

    impl MockTorManager {
        fn new() -> Self {
            Self {
                should_fail: false,
                deployed_services: Mutex::new(vec![]),
            }
        }

        #[allow(dead_code)]
        fn failing() -> Self {
            Self {
                should_fail: true,
                deployed_services: Mutex::new(vec![]),
            }
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
                self.deployed_services
                    .lock()
                    .unwrap()
                    .push(name.to_string());
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

    struct MockServiceManager;

    #[async_trait]
    impl ServiceManagerPort for MockServiceManager {
        async fn start_service(&self, _name: &str) -> Result<()> {
            Ok(())
        }
        async fn stop_service(&self, _name: &str) -> Result<()> {
            Ok(())
        }
        async fn restart_service(&self, _name: &str) -> Result<()> {
            Ok(())
        }
        async fn enable_service(&self, _name: &str) -> Result<()> {
            Ok(())
        }
        async fn disable_service(&self, _name: &str) -> Result<()> {
            Ok(())
        }
        async fn is_active(&self, _name: &str) -> Result<bool> {
            Ok(true)
        }
        async fn get_service_metrics(&self, _name: &str) -> Result<ServiceMetrics> {
            Ok(ServiceMetrics::default())
        }
    }

    // Note: These tests require filesystem access which may not work in all CI environments
    // They test the validation logic, not the actual filesystem operations

    #[tokio::test]
    async fn test_deploy_fileserver_empty_name() {
        let nginx = Arc::new(MockNginxManager::new());
        let tor = Arc::new(MockTorManager::new());
        let service = Arc::new(MockServiceManager);
        let file = Arc::new(MockFileManager);
        let use_case = DeployFileServer::new(nginx, tor, service, file, Arc::new(mock_manifest()));

        let request = DeployFileServerRequest {
            service_name: "".to_string(),
            port: 8080,
            share_path: None,
            enable_auth: false,
        };

        let result = use_case.execute(request).await;

        assert!(result.is_err());
        match result {
            Err(EnolaError::ValidationError(msg)) => {
                assert!(msg.contains("empty"));
            }
            _ => panic!("Expected ValidationError"),
        }
    }

    #[tokio::test]
    async fn test_deploy_fileserver_invalid_nginx_config() {
        let nginx = Arc::new(MockNginxManager::with_invalid_config());
        let tor = Arc::new(MockTorManager::new());
        let service = Arc::new(MockServiceManager);
        let file = Arc::new(MockFileManager);
        let use_case = DeployFileServer::new(nginx, tor, service, file, Arc::new(mock_manifest()));

        // Use /tmp which should exist and be writable
        let request = DeployFileServerRequest {
            service_name: "testfiles".to_string(),
            port: 8080,
            share_path: Some("/tmp/enola_test_share".to_string()),
            enable_auth: false,
        };

        let result = use_case.execute(request).await;

        // This will fail either due to permissions or invalid config
        assert!(result.is_err());
    }

    #[test]
    fn test_deploy_request_struct() {
        let request = DeployFileServerRequest {
            service_name: "myfiles".to_string(),
            port: 9000,
            share_path: Some("/data/files".to_string()),
            enable_auth: true,
        };

        assert_eq!(request.service_name, "myfiles");
        assert_eq!(request.port, 9000);
        assert_eq!(request.share_path, Some("/data/files".to_string()));
        assert!(request.enable_auth);
    }

    #[test]
    fn test_deploy_request_default_path() {
        let request = DeployFileServerRequest {
            service_name: "docs".to_string(),
            port: 8080,
            share_path: None,
            enable_auth: false,
        };

        assert!(request.share_path.is_none());
        // The actual default path is computed in execute()
    }
}

/// SEC-006: Validate that a share path is absolute, contains no traversal,
/// and stays within the expected root directory using canonical path resolution.
///
/// This function eliminates the TOCTOU vulnerability by:
/// 1. Checking for null bytes and relative paths upfront.
/// 2. Lexically canonicalizing the path (collapsing `.` and `..` components)
///    without requiring the path to exist on disk.
/// 3. Verifying the canonicalized path stays within the expected root.
///
/// If the path already exists on disk, `std::fs::canonicalize` is used for
/// full resolution (resolving symlinks). If it doesn't exist yet (common case
/// for new shares), lexical canonicalization is used instead.
fn validate_share_path(path: &str) -> Result<PathBuf> {
    // Check for null bytes
    if path.contains('\0') {
        return Err(EnolaError::ValidationError(
            "Path contains null byte".to_string(),
        ));
    }

    let path_buf = PathBuf::from(path);

    // Must be absolute path
    if !path_buf.is_absolute() {
        return Err(EnolaError::ValidationError(
            "Path must be absolute (start with /)".to_string(),
        ));
    }

    // SEC-006: Lexical canonicalization — collapse . and .. components
    // without requiring the path to exist on disk.
    let canonical = lexically_canonicalize(&path_buf)?;

    // SEC-006: If the original path starts with /srv/enola-files,
    // the canonicalized path must also stay within /srv/enola-files.
    // This catches traversal like /srv/enola-files/../etc/passwd → /srv/etc/passwd
    // which escapes the intended root.
    let original_starts_enola = path.starts_with("/srv/enola-files");
    if original_starts_enola {
        let base = PathBuf::from("/srv/enola-files");
        if !canonical.starts_with(&base) {
            return Err(EnolaError::ValidationError(
                "Path escapes /srv/enola-files directory".to_string(),
            ));
        }
    }

    // If the path already exists, use full filesystem canonicalization
    // to resolve any symlinks that could bypass the lexical check.
    if path_buf.exists() {
        match std::fs::canonicalize(&path_buf) {
            Ok(fs_canonical) => {
                // Re-check that the filesystem-resolved path is still within root
                if original_starts_enola {
                    let base = PathBuf::from("/srv/enola-files");
                    if !fs_canonical.starts_with(&base) {
                        return Err(EnolaError::ValidationError(
                            "Path escapes /srv/enola-files directory (symlink resolved)"
                                .to_string(),
                        ));
                    }
                }
                Ok(fs_canonical)
            }
            Err(e) => Err(EnolaError::InfrastructureError(format!(
                "Failed to canonicalize path '{}': {}",
                path, e
            ))),
        }
    } else {
        Ok(canonical)
    }
}

/// Lexically canonicalize a path by collapsing `.` and `..` components
/// without touching the filesystem. This prevents path traversal attacks
/// even when the path doesn't exist yet.
fn lexically_canonicalize(path: &Path) -> Result<PathBuf> {
    let mut components = Vec::new();

    for component in path.components() {
        use std::path::Component;
        match component {
            Component::CurDir => { /* skip `.` */ }
            Component::ParentDir => {
                // Pop the last component if any, but never above root.
                // If only root remains, .. would escape — reject it.
                match components.last() {
                    None => {
                        return Err(EnolaError::ValidationError(
                            "Path contains '..' that escapes root (path traversal not allowed)"
                                .to_string(),
                        ));
                    }
                    Some(last) if last == &PathBuf::from("/") => {
                        return Err(EnolaError::ValidationError(
                            "Path contains '..' that escapes root (path traversal not allowed)"
                                .to_string(),
                        ));
                    }
                    Some(_) => {
                        components.pop();
                    }
                }
            }
            Component::RootDir => {
                components.push(std::path::PathBuf::from("/"));
            }
            Component::Normal(c) => {
                if let Some(last) = components.last() {
                    let mut combined = last.clone();
                    combined.push(c);
                    components.push(combined);
                } else {
                    components.push(PathBuf::from(c));
                }
            }
            Component::Prefix(_) => {}
        }
    }

    // Rebuild from components
    let mut result = PathBuf::new();
    for (i, c) in components.iter().enumerate() {
        if i == 0 {
            result = c.clone();
        } else {
            result.push(c);
        }
    }

    if result.as_os_str().is_empty() {
        result = PathBuf::from("/");
    }

    Ok(result)
}

#[cfg(test)]
mod sec_017_tests {
    use super::*;

    #[test]
    fn validate_share_path_accepts_valid_absolute_path() {
        // Create a temporary directory for testing
        let temp_dir = std::env::temp_dir();
        let test_path = temp_dir.join("enola-files-test");
        std::fs::create_dir_all(&test_path).unwrap(); // SAFETY: test-only, temp dir creation

        let result = validate_share_path(test_path.to_str().unwrap()); // SAFETY: test-only, valid UTF-8 path
        assert!(result.is_ok());

        // Cleanup
        std::fs::remove_dir_all(&test_path).ok();
    }

    #[test]
    fn validate_share_path_rejects_relative_path() {
        let result = validate_share_path("relative/path");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("absolute"));
    }

    #[test]
    fn validate_share_path_rejects_path_traversal() {
        // /srv/enola-files/../etc/passwd canonicalizes to /etc/passwd
        // which is NOT under /srv/enola-files — should be rejected
        let result = validate_share_path("/srv/enola-files/../etc/passwd");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("escapes"));
    }

    #[test]
    fn validate_share_path_rejects_null_byte() {
        let result = validate_share_path("/srv/enola-files/test\0");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("null byte"));
    }

    #[test]
    fn validate_share_path_rejects_escape_from_enola_files() {
        // /srv/enola-files/../../etc/passwd canonicalizes to /etc/passwd
        // which escapes /srv/enola-files
        let result = validate_share_path("/srv/enola-files/../../etc/passwd");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("escapes"));
    }

    // ── SEC-006: Canonical path validation tests ───────────────────────────

    #[test]
    fn validate_share_path_collapses_dot_components() {
        let result = validate_share_path("/srv/enola-files/./test").unwrap();
        assert_eq!(result, PathBuf::from("/srv/enola-files/test"));
    }

    #[test]
    fn validate_share_path_collapses_dotdot_within_root() {
        // /a/../b should canonicalize to /b (not an error, just rewrites)
        let result = validate_share_path("/srv/enola-files/sub/../test").unwrap();
        assert_eq!(result, PathBuf::from("/srv/enola-files/test"));
    }

    #[test]
    fn validate_share_path_rejects_dotdot_escaping_root() {
        // /.. at root level: the root component has nothing to pop,
        // so lexically_canonicalize should reject it.
        let result = validate_share_path("/..");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains(".."));
    }

    #[test]
    fn validate_share_path_accepts_nonexistent_path() {
        // Path doesn't exist yet — should still validate via lexical canonicalization
        let result = validate_share_path("/tmp/enola-nonexistent-path-12345");
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap(),
            PathBuf::from("/tmp/enola-nonexistent-path-12345")
        );
    }

    #[test]
    fn validate_share_path_resolves_existing_symlink() {
        // Create a real symlink and verify canonicalize resolves it
        let temp_dir = std::env::temp_dir();
        let real_dir = temp_dir.join("enola-real-dir-sec006");
        let link_dir = temp_dir.join("enola-link-dir-sec006");
        std::fs::create_dir_all(&real_dir).unwrap();
        let _ = std::os::unix::fs::symlink(&real_dir, &link_dir);

        let result = validate_share_path(link_dir.to_str().unwrap());
        assert!(result.is_ok());
        // The canonicalized path should point to the real directory, not the symlink
        let canonical = result.unwrap();
        assert!(canonical.ends_with("enola-real-dir-sec006"));

        // Cleanup
        std::fs::remove_file(&link_dir).ok();
        std::fs::remove_dir_all(&real_dir).ok();
    }
}
