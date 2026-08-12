use crate::domain::error::{EnolaError, Result};
use crate::ports::container::{ContainerConfig, ContainerPort, ImageBuildConfig};
use crate::ports::file::FileManagerPort;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::{error, info, warn};

/// Application Service for deploying user applications from Git repositories
pub struct AppDeployer {
    apps_base_dir: PathBuf,
    container_manager: Arc<dyn ContainerPort + Send + Sync>,
    file_manager: Arc<dyn FileManagerPort + Send + Sync>,
}

#[derive(Debug, Clone)]
pub struct DeployResult {
    pub success: bool,
    pub strategy: DeployStrategy,
    pub app_port: Option<u16>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DeployStrategy {
    Script,     // deploy.sh
    Dockerfile, // Dockerfile
    Compose,    // docker-compose.yml
    Static,     // Static files (index.html)
    None,       // No deployment method found
}

impl AppDeployer {
    pub fn new(
        container_manager: Arc<dyn ContainerPort + Send + Sync>,
        file_manager: Arc<dyn FileManagerPort + Send + Sync>,
    ) -> Self {
        Self {
            apps_base_dir: PathBuf::from("/opt/enola/apps"),
            container_manager,
            file_manager,
        }
    }

    /// Execute deployment for a project from a cloned repo directory
    pub async fn execute(&self, project_name: &str, repo_path: &Path) -> Result<DeployResult> {
        info!(
            "[Deploy: {}] Starting deployment from {:?}",
            project_name, repo_path
        );

        // Ensure repo exists
        if !repo_path.exists() {
            return Err(EnolaError::NotFound(format!(
                "Repository path not found: {:?}",
                repo_path
            )));
        }

        // Detect deployment strategy
        let strategy = self.detect_strategy(repo_path).await;
        info!(
            "[Deploy: {}] Detected strategy: {:?}",
            project_name, strategy
        );

        // Execute deployment based on strategy
        let result = match strategy {
            DeployStrategy::Script => self.deploy_with_script(project_name, repo_path).await,
            DeployStrategy::Dockerfile => {
                self.deploy_with_dockerfile(project_name, repo_path).await
            }
            DeployStrategy::Compose => self.deploy_with_compose(project_name, repo_path).await,
            DeployStrategy::Static => self.deploy_static(project_name, repo_path).await,
            DeployStrategy::None => {
                warn!(
                    "[Deploy: {}] No deployment method found, copying files only",
                    project_name
                );
                self.deploy_static(project_name, repo_path).await
            }
        };

        result
    }

    /// Detect which deployment strategy to use based on files in the repo
    async fn detect_strategy(&self, repo_path: &Path) -> DeployStrategy {
        // Check for deploy.sh first (highest priority)
        if repo_path.join("deploy.sh").exists() {
            return DeployStrategy::Script;
        }

        // Check for docker-compose.yml
        if repo_path.join("docker-compose.yml").exists()
            || repo_path.join("docker-compose.yaml").exists()
        {
            return DeployStrategy::Compose;
        }

        // Check for Dockerfile
        if repo_path.join("Dockerfile").exists() {
            return DeployStrategy::Dockerfile;
        }

        // Check for static files
        if repo_path.join("index.html").exists()
            || repo_path.join("public/index.html").exists()
            || repo_path.join("dist/index.html").exists()
        {
            return DeployStrategy::Static;
        }

        DeployStrategy::None
    }

    /// Deploy using deploy.sh script
    async fn deploy_with_script(
        &self,
        project_name: &str,
        repo_path: &Path,
    ) -> Result<DeployResult> {
        let script_path = repo_path.join("deploy.sh");
        info!("[Deploy: {}] Executing deploy.sh", project_name);

        // Make script executable via FileManagerPort (no direct Command::new)
        self.file_manager
            .set_permissions(&script_path, 0o755)
            .await?;

        // Execute the script via ContainerPort run_ephemeral_container
        let app_dir = self
            .apps_base_dir
            .join(project_name)
            .to_string_lossy()
            .to_string();
        let config = ContainerConfig {
            name: format!("enola-deploy-{}", project_name),
            image: "bash:5".to_string(),
            volumes: std::collections::HashMap::from([
                (
                    repo_path.to_string_lossy().to_string(),
                    "/repo:ro".to_string(),
                ),
                (app_dir.clone(), "/app".to_string()),
            ]),
            command: Some(vec!["/repo/deploy.sh".into()]),
            env: std::collections::HashMap::from([
                ("PROJECT_NAME".into(), project_name.into()),
                ("APP_DIR".into(), app_dir.clone()),
            ]),
            auto_remove: true,
            ..Default::default()
        };

        let (exit_code, stdout) = tokio::time::timeout(
            std::time::Duration::from_secs(300),
            self.container_manager.run_ephemeral_container(config),
        )
        .await
        .map_err(|_| {
            EnolaError::InfrastructureError("Deploy script timeout (5min)".to_string())
        })??;

        if exit_code == 0 {
            let port = self.detect_port_from_output(&stdout, repo_path).await;
            info!("[Deploy: {}] Script completed successfully", project_name);
            Ok(DeployResult {
                success: true,
                strategy: DeployStrategy::Script,
                app_port: port,
                message: format!("Deployed with deploy.sh\n{}", stdout),
            })
        } else {
            error!(
                "[Deploy: {}] Script failed (exit {})",
                project_name, exit_code
            );
            Err(EnolaError::InfrastructureError(format!(
                "Deploy script failed (exit {})",
                exit_code
            )))
        }
    }

    /// Deploy using Dockerfile
    async fn deploy_with_dockerfile(
        &self,
        project_name: &str,
        repo_path: &Path,
    ) -> Result<DeployResult> {
        let image_name = format!("enola-app-{}", project_name);
        let container_name = format!("enola-app-{}", project_name);
        info!(
            "[Deploy: {}] Building Docker image: {}",
            project_name, image_name
        );

        // Build image via ContainerPort
        let build_config = ImageBuildConfig {
            dockerfile_path: repo_path.join("Dockerfile"),
            context_path: repo_path.to_path_buf(),
            tag: image_name.clone(),
            build_args: std::collections::HashMap::new(),
        };
        self.container_manager
            .build_image(build_config)
            .await
            .map_err(|e| EnolaError::InfrastructureError(format!("Docker build failed: {}", e)))?;

        // Stop and remove existing container
        let _ = self.container_manager.stop_container(&container_name).await;
        let _ = self
            .container_manager
            .remove_container(&container_name)
            .await;

        // Detect port from Dockerfile
        let port = self
            .detect_port_from_dockerfile(repo_path)
            .await
            .unwrap_or(3000);
        info!(
            "[Deploy: {}] Starting container on port {}",
            project_name, port
        );

        // Run container
        let config = ContainerConfig {
            name: container_name.clone(),
            image: image_name,
            ports: std::collections::HashMap::from([(port, port)]),
            restart_policy: Some("unless-stopped".into()),
            ..Default::default()
        };
        self.container_manager
            .create_container(config)
            .await
            .map_err(|e| EnolaError::InfrastructureError(format!("Docker run failed: {}", e)))?;
        self.container_manager
            .start_container(&container_name)
            .await?;

        Ok(DeployResult {
            success: true,
            strategy: DeployStrategy::Dockerfile,
            app_port: Some(port),
            message: format!("Deployed with Dockerfile, container: {}", container_name),
        })
    }

    /// Deploy using Docker Compose via ContainerPort ephemeral container
    async fn deploy_with_compose(
        &self,
        project_name: &str,
        repo_path: &Path,
    ) -> Result<DeployResult> {
        info!("[Deploy: {}] Running docker compose up", project_name);

        // Use run_ephemeral_container with docker/compose image to run Docker Compose
        let config = ContainerConfig {
            name: format!("enola-compose-{}", project_name),
            image: "docker/compose:latest".to_string(),
            volumes: std::collections::HashMap::from([
                (
                    repo_path.to_string_lossy().to_string(),
                    "/workspace".to_string(),
                ),
                (
                    "/var/run/docker.sock".to_string(),
                    "/var/run/docker.sock".to_string(),
                ),
            ]),
            command: Some(vec![
                "-p".into(),
                project_name.into(),
                "up".into(),
                "-d".into(),
                "--build".into(),
            ]),
            working_dir: Some("/workspace".into()),
            auto_remove: true,
            ..Default::default()
        };

        let (exit_code, _logs) = self
            .container_manager
            .run_ephemeral_container(config)
            .await?;

        if exit_code == 0 {
            let port = self.detect_port_from_compose(repo_path).await;
            Ok(DeployResult {
                success: true,
                strategy: DeployStrategy::Compose,
                app_port: port,
                message: format!("Deployed with docker compose, project: {}", project_name),
            })
        } else {
            Err(EnolaError::InfrastructureError(format!(
                "docker compose failed (exit {})",
                exit_code
            )))
        }
    }

    /// Deploy static files via FileManagerPort
    async fn deploy_static(&self, project_name: &str, repo_path: &Path) -> Result<DeployResult> {
        let app_dir = self.apps_base_dir.join(project_name);
        info!(
            "[Deploy: {}] Copying static files to {:?}",
            project_name, app_dir
        );

        self.file_manager.ensure_dir(&app_dir).await?;

        // Determine source directory
        let source_dir = if repo_path.join("dist").exists() {
            repo_path.join("dist")
        } else if repo_path.join("public").exists() {
            repo_path.join("public")
        } else if repo_path.join("build").exists() {
            repo_path.join("build")
        } else {
            repo_path.to_path_buf()
        };

        // Copy each entry via FileManagerPort
        if let Ok(mut entries) = tokio::fs::read_dir(&source_dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let dest = app_dir.join(entry.file_name());
                self.file_manager
                    .copy_file(&entry.path(), &dest)
                    .await
                    .unwrap_or_else(|e| warn!("Copy failed for {:?}: {}", entry.path(), e));
            }
        }

        Ok(DeployResult {
            success: true,
            strategy: DeployStrategy::Static,
            app_port: Some(80),
            message: format!("Static files deployed to {:?}", app_dir),
        })
    }

    /// Try to detect port from script output or .env file
    async fn detect_port_from_output(&self, output: &str, repo_path: &Path) -> Option<u16> {
        // Check output for PORT=XXXX pattern
        for line in output.lines() {
            if let Some(port_str) = line.strip_prefix("PORT=") {
                if let Ok(port) = port_str.trim().parse::<u16>() {
                    return Some(port);
                }
            }
        }

        // Check .env file
        self.detect_port_from_env(repo_path).await
    }

    /// Detect port from .env file
    async fn detect_port_from_env(&self, repo_path: &Path) -> Option<u16> {
        let env_path = repo_path.join(".env");
        if let Ok(content) = tokio::fs::read_to_string(&env_path).await {
            for line in content.lines() {
                let line = line.trim();
                if line.starts_with("PORT=") || line.starts_with("APP_PORT=") {
                    if let Some(port_str) = line.split('=').nth(1) {
                        if let Ok(port) = port_str.trim().parse::<u16>() {
                            return Some(port);
                        }
                    }
                }
            }
        }
        None
    }

    /// Detect port from Dockerfile EXPOSE instruction
    async fn detect_port_from_dockerfile(&self, repo_path: &Path) -> Option<u16> {
        let dockerfile_path = repo_path.join("Dockerfile");
        if let Ok(content) = tokio::fs::read_to_string(&dockerfile_path).await {
            for line in content.lines() {
                let line = line.trim().to_uppercase();
                if line.starts_with("EXPOSE ") {
                    if let Some(port_str) = line.strip_prefix("EXPOSE ") {
                        // Handle "EXPOSE 3000" or "EXPOSE 3000/tcp"
                        let port_str = port_str.split('/').next().unwrap_or(port_str);
                        if let Ok(port) = port_str.trim().parse::<u16>() {
                            return Some(port);
                        }
                    }
                }
            }
        }
        None
    }

    /// Detect port from docker-compose.yml
    async fn detect_port_from_compose(&self, repo_path: &Path) -> Option<u16> {
        let compose_path = if repo_path.join("docker-compose.yml").exists() {
            repo_path.join("docker-compose.yml")
        } else {
            repo_path.join("docker-compose.yaml")
        };

        if let Ok(content) = tokio::fs::read_to_string(&compose_path).await {
            // Simple regex-like search for ports: pattern
            for line in content.lines() {
                let line = line.trim();
                // Look for patterns like "- 3000:3000" or "- \"8080:8080\""
                if line.starts_with("- ") && line.contains(':') {
                    let port_mapping = line.trim_start_matches("- ").trim_matches('"');
                    if let Some(host_port) = port_mapping.split(':').next() {
                        if let Ok(port) = host_port.trim().parse::<u16>() {
                            return Some(port);
                        }
                    }
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn test_deploy_strategy_equality() {
        assert_eq!(DeployStrategy::Script, DeployStrategy::Script);
        assert_eq!(DeployStrategy::Dockerfile, DeployStrategy::Dockerfile);
        assert_eq!(DeployStrategy::Compose, DeployStrategy::Compose);
        assert_eq!(DeployStrategy::Static, DeployStrategy::Static);
        assert_eq!(DeployStrategy::None, DeployStrategy::None);
        assert_ne!(DeployStrategy::Script, DeployStrategy::None);
    }

    #[test]
    fn test_deploy_result_default_fields() {
        let result = DeployResult {
            success: true,
            strategy: DeployStrategy::Script,
            app_port: Some(3000),
            message: "OK".to_string(),
        };
        assert!(result.success);
        assert_eq!(result.app_port, Some(3000));
    }

    #[test]
    fn test_deploy_result_no_port() {
        let result = DeployResult {
            success: false,
            strategy: DeployStrategy::None,
            app_port: None,
            message: "No deployment method".to_string(),
        };
        assert!(!result.success);
        assert!(result.app_port.is_none());
    }

    use crate::ports::container::MockContainerPort;
    use crate::ports::file::MockFileManagerPort;

    fn make_deployer() -> AppDeployer {
        let mut cm = MockContainerPort::new();
        cm.expect_run_ephemeral_container()
            .returning(|_| Ok((0, "ok".into())));
        cm.expect_build_image().returning(|_| Ok("img".into()));
        cm.expect_stop_container().returning(|_| Ok(()));
        cm.expect_remove_container().returning(|_| Ok(()));
        cm.expect_create_container().returning(|_| Ok("cid".into()));
        cm.expect_start_container().returning(|_| Ok(()));
        let mut fm = MockFileManagerPort::new();
        fm.expect_set_permissions().returning(|_, _| Ok(()));
        fm.expect_ensure_dir().returning(|_| Ok(()));
        fm.expect_copy_file().returning(|_, _| Ok(()));
        AppDeployer::new(Arc::new(cm), Arc::new(fm))
    }

    #[tokio::test]
    async fn test_detect_strategy_deploy_sh() {
        let dir = tempfile::tempdir().unwrap(); // unwrap: test-only
        std::fs::write(dir.path().join("deploy.sh"), "#!/bin/bash\necho ok").unwrap(); // unwrap: test-only

        let deployer = make_deployer();
        let strategy = deployer.detect_strategy(dir.path()).await;
        assert_eq!(strategy, DeployStrategy::Script);
    }

    #[tokio::test]
    async fn test_detect_strategy_dockerfile() {
        let dir = tempfile::tempdir().unwrap(); // unwrap: test-only
        std::fs::write(dir.path().join("Dockerfile"), "FROM alpine").unwrap(); // unwrap: test-only

        let deployer = make_deployer();
        let strategy = deployer.detect_strategy(dir.path()).await;
        assert_eq!(strategy, DeployStrategy::Dockerfile);
    }

    #[tokio::test]
    async fn test_detect_strategy_compose() {
        let dir = tempfile::tempdir().unwrap(); // unwrap: test-only
        std::fs::write(dir.path().join("docker-compose.yml"), "version: '3'").unwrap(); // unwrap: test-only

        let deployer = make_deployer();
        let strategy = deployer.detect_strategy(dir.path()).await;
        assert_eq!(strategy, DeployStrategy::Compose);
    }

    #[tokio::test]
    async fn test_detect_strategy_static_index() {
        let dir = tempfile::tempdir().unwrap(); // unwrap: test-only
        std::fs::write(dir.path().join("index.html"), "<html></html>").unwrap(); // unwrap: test-only

        let deployer = make_deployer();
        let strategy = deployer.detect_strategy(dir.path()).await;
        assert_eq!(strategy, DeployStrategy::Static);
    }

    #[tokio::test]
    async fn test_detect_strategy_none() {
        let dir = tempfile::tempdir().unwrap(); // unwrap: test-only
        let deployer = make_deployer();
        let strategy = deployer.detect_strategy(dir.path()).await;
        assert_eq!(strategy, DeployStrategy::None);
    }

    #[tokio::test]
    async fn test_detect_strategy_priority_script_over_dockerfile() {
        let dir = tempfile::tempdir().unwrap(); // unwrap: test-only
        std::fs::write(dir.path().join("deploy.sh"), "#!/bin/bash").unwrap(); // unwrap: test-only
        std::fs::write(dir.path().join("Dockerfile"), "FROM alpine").unwrap(); // unwrap: test-only

        let deployer = make_deployer();
        let strategy = deployer.detect_strategy(dir.path()).await;
        assert_eq!(strategy, DeployStrategy::Script);
    }

    #[tokio::test]
    async fn test_execute_repo_not_found() {
        let deployer = make_deployer();
        let result = deployer
            .execute("test", Path::new("/nonexistent/path/abc123"))
            .await;
        assert!(result.is_err());
    }

    // ── deploy_with_script error paths ──────────────────────────────────────

    #[tokio::test]
    async fn test_deploy_script_set_permissions_fails() {
        let dir = tempfile::tempdir().unwrap(); // unwrap: test-only
        std::fs::write(dir.path().join("deploy.sh"), "#!/bin/bash").unwrap(); // unwrap: test-only

        let cm = MockContainerPort::new();
        let mut fm = MockFileManagerPort::new();
        fm.expect_set_permissions().returning(|_, _| {
            Err(crate::domain::error::EnolaError::InfrastructureError(
                "perm denied".into(),
            ))
        });

        let deployer = AppDeployer::new(Arc::new(cm), Arc::new(fm));
        let result = deployer.execute("test", dir.path()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_deploy_script_container_error() {
        let dir = tempfile::tempdir().unwrap(); // unwrap: test-only
        std::fs::write(dir.path().join("deploy.sh"), "#!/bin/bash").unwrap(); // unwrap: test-only

        let mut cm = MockContainerPort::new();
        cm.expect_run_ephemeral_container().returning(|_| {
            Err(crate::domain::error::EnolaError::InfrastructureError(
                "container error".into(),
            ))
        });
        let mut fm = MockFileManagerPort::new();
        fm.expect_set_permissions().returning(|_, _| Ok(()));

        let deployer = AppDeployer::new(Arc::new(cm), Arc::new(fm));
        let result = deployer.execute("test", dir.path()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_deploy_script_nonzero_exit() {
        let dir = tempfile::tempdir().unwrap(); // unwrap: test-only
        std::fs::write(dir.path().join("deploy.sh"), "#!/bin/bash\nexit 1").unwrap(); // unwrap: test-only

        let mut cm = MockContainerPort::new();
        cm.expect_run_ephemeral_container()
            .returning(|_| Ok((1, "error".into())));
        let mut fm = MockFileManagerPort::new();
        fm.expect_set_permissions().returning(|_, _| Ok(()));

        let deployer = AppDeployer::new(Arc::new(cm), Arc::new(fm));
        let result = deployer.execute("test", dir.path()).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Deploy script failed"));
    }

    #[tokio::test]
    async fn test_deploy_script_success_port_from_output() {
        let dir = tempfile::tempdir().unwrap(); // unwrap: test-only
        std::fs::write(dir.path().join("deploy.sh"), "#!/bin/bash").unwrap(); // unwrap: test-only

        let mut cm = MockContainerPort::new();
        cm.expect_run_ephemeral_container()
            .returning(|_| Ok((0, "starting\nPORT=8080\nready".into())));
        let mut fm = MockFileManagerPort::new();
        fm.expect_set_permissions().returning(|_, _| Ok(()));

        let deployer = AppDeployer::new(Arc::new(cm), Arc::new(fm));
        let result = deployer.execute("test", dir.path()).await.unwrap(); // unwrap: test-only
        assert!(result.success);
        assert_eq!(result.app_port, Some(8080));
        assert_eq!(result.strategy, DeployStrategy::Script);
    }

    // ── deploy_with_dockerfile error paths ──────────────────────────────────

    #[tokio::test]
    async fn test_deploy_dockerfile_build_fails() {
        let dir = tempfile::tempdir().unwrap(); // unwrap: test-only
        std::fs::write(dir.path().join("Dockerfile"), "FROM alpine").unwrap(); // unwrap: test-only

        let mut cm = MockContainerPort::new();
        cm.expect_build_image().returning(|_| {
            Err(crate::domain::error::EnolaError::InfrastructureError(
                "build failed".into(),
            ))
        });
        let fm = MockFileManagerPort::new();

        let deployer = AppDeployer::new(Arc::new(cm), Arc::new(fm));
        let result = deployer.execute("test", dir.path()).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Docker build failed"));
    }

    #[tokio::test]
    async fn test_deploy_dockerfile_create_container_fails() {
        let dir = tempfile::tempdir().unwrap(); // unwrap: test-only
        std::fs::write(dir.path().join("Dockerfile"), "FROM alpine\nEXPOSE 3000").unwrap(); // unwrap: test-only

        let mut cm = MockContainerPort::new();
        cm.expect_build_image().returning(|_| Ok("img".into()));
        cm.expect_stop_container().returning(|_| Ok(()));
        cm.expect_remove_container().returning(|_| Ok(()));
        cm.expect_create_container().returning(|_| {
            Err(crate::domain::error::EnolaError::InfrastructureError(
                "no space".into(),
            ))
        });
        let fm = MockFileManagerPort::new();

        let deployer = AppDeployer::new(Arc::new(cm), Arc::new(fm));
        let result = deployer.execute("test", dir.path()).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Docker run failed"));
    }

    #[tokio::test]
    async fn test_deploy_dockerfile_start_container_fails() {
        let dir = tempfile::tempdir().unwrap(); // unwrap: test-only
        std::fs::write(dir.path().join("Dockerfile"), "FROM alpine").unwrap(); // unwrap: test-only

        let mut cm = MockContainerPort::new();
        cm.expect_build_image().returning(|_| Ok("img".into()));
        cm.expect_stop_container().returning(|_| Ok(()));
        cm.expect_remove_container().returning(|_| Ok(()));
        cm.expect_create_container().returning(|_| Ok("cid".into()));
        cm.expect_start_container().returning(|_| {
            Err(crate::domain::error::EnolaError::InfrastructureError(
                "start failed".into(),
            ))
        });
        let fm = MockFileManagerPort::new();

        let deployer = AppDeployer::new(Arc::new(cm), Arc::new(fm));
        let result = deployer.execute("test", dir.path()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_deploy_dockerfile_success_with_expose_tcp() {
        let dir = tempfile::tempdir().unwrap(); // unwrap: test-only
        std::fs::write(
            dir.path().join("Dockerfile"),
            "FROM node:18\nEXPOSE 4000/tcp",
        )
        .unwrap(); // unwrap: test-only

        let mut cm = MockContainerPort::new();
        cm.expect_build_image().returning(|_| Ok("img".into()));
        cm.expect_stop_container().returning(|_| Ok(()));
        cm.expect_remove_container().returning(|_| Ok(()));
        cm.expect_create_container().returning(|_| Ok("cid".into()));
        cm.expect_start_container().returning(|_| Ok(()));
        let fm = MockFileManagerPort::new();

        let deployer = AppDeployer::new(Arc::new(cm), Arc::new(fm));
        let result = deployer.execute("test", dir.path()).await.unwrap(); // unwrap: test-only
        assert!(result.success);
        assert_eq!(result.app_port, Some(4000));
        assert_eq!(result.strategy, DeployStrategy::Dockerfile);
    }

    #[tokio::test]
    async fn test_deploy_dockerfile_success_fallback_port_3000() {
        let dir = tempfile::tempdir().unwrap(); // unwrap: test-only
        std::fs::write(dir.path().join("Dockerfile"), "FROM alpine\nRUN echo hi").unwrap(); // unwrap: test-only

        let mut cm = MockContainerPort::new();
        cm.expect_build_image().returning(|_| Ok("img".into()));
        cm.expect_stop_container().returning(|_| Ok(()));
        cm.expect_remove_container().returning(|_| Ok(()));
        cm.expect_create_container().returning(|_| Ok("cid".into()));
        cm.expect_start_container().returning(|_| Ok(()));
        let fm = MockFileManagerPort::new();

        let deployer = AppDeployer::new(Arc::new(cm), Arc::new(fm));
        let result = deployer.execute("test", dir.path()).await.unwrap(); // unwrap: test-only
        assert!(result.success);
        assert_eq!(result.app_port, Some(3000));
    }

    // ── deploy_with_compose error paths ─────────────────────────────────────

    #[tokio::test]
    async fn test_deploy_compose_container_error() {
        let dir = tempfile::tempdir().unwrap(); // unwrap: test-only
        std::fs::write(dir.path().join("docker-compose.yml"), "version: '3'").unwrap(); // unwrap: test-only

        let mut cm = MockContainerPort::new();
        cm.expect_run_ephemeral_container().returning(|_| {
            Err(crate::domain::error::EnolaError::InfrastructureError(
                "docker err".into(),
            ))
        });
        let fm = MockFileManagerPort::new();

        let deployer = AppDeployer::new(Arc::new(cm), Arc::new(fm));
        let result = deployer.execute("test", dir.path()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_deploy_compose_nonzero_exit() {
        let dir = tempfile::tempdir().unwrap(); // unwrap: test-only
        std::fs::write(dir.path().join("docker-compose.yml"), "version: '3'").unwrap(); // unwrap: test-only

        let mut cm = MockContainerPort::new();
        cm.expect_run_ephemeral_container()
            .returning(|_| Ok((1, "compose error".into())));
        let fm = MockFileManagerPort::new();

        let deployer = AppDeployer::new(Arc::new(cm), Arc::new(fm));
        let result = deployer.execute("test", dir.path()).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("docker compose failed"));
    }

    #[tokio::test]
    async fn test_deploy_compose_success_with_port() {
        let dir = tempfile::tempdir().unwrap(); // unwrap: test-only
        std::fs::write(
            dir.path().join("docker-compose.yml"),
            "services:\n  web:\n    ports:\n      - 5000:5000",
        )
        .unwrap(); // unwrap: test-only

        let mut cm = MockContainerPort::new();
        cm.expect_run_ephemeral_container()
            .returning(|_| Ok((0, "".into())));
        let fm = MockFileManagerPort::new();

        let deployer = AppDeployer::new(Arc::new(cm), Arc::new(fm));
        let result = deployer.execute("test", dir.path()).await.unwrap(); // unwrap: test-only
        assert!(result.success);
        assert_eq!(result.app_port, Some(5000));
        assert_eq!(result.strategy, DeployStrategy::Compose);
    }

    #[tokio::test]
    async fn test_deploy_compose_yaml_extension() {
        let dir = tempfile::tempdir().unwrap(); // unwrap: test-only
        std::fs::write(
            dir.path().join("docker-compose.yaml"),
            "services:\n  web:\n    ports:\n      - 4321:4321",
        )
        .unwrap(); // unwrap: test-only

        let mut cm = MockContainerPort::new();
        cm.expect_run_ephemeral_container()
            .returning(|_| Ok((0, "".into())));
        let fm = MockFileManagerPort::new();

        let deployer = AppDeployer::new(Arc::new(cm), Arc::new(fm));
        let result = deployer.execute("test", dir.path()).await.unwrap(); // unwrap: test-only
        assert!(result.success);
        assert_eq!(result.app_port, Some(4321));
    }

    // ── deploy_static error path + source dir selection ──────────────────────

    #[tokio::test]
    async fn test_deploy_static_ensure_dir_fails() {
        let dir = tempfile::tempdir().unwrap(); // unwrap: test-only
        std::fs::write(dir.path().join("index.html"), "<html>").unwrap(); // unwrap: test-only

        let cm = MockContainerPort::new();
        let mut fm = MockFileManagerPort::new();
        fm.expect_ensure_dir().returning(|_| {
            Err(crate::domain::error::EnolaError::InfrastructureError(
                "disk full".into(),
            ))
        });

        let deployer = AppDeployer::new(Arc::new(cm), Arc::new(fm));
        let result = deployer.execute("test", dir.path()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_deploy_static_uses_dist_dir() {
        let dir = tempfile::tempdir().unwrap(); // unwrap: test-only
        let dist = dir.path().join("dist");
        std::fs::create_dir(&dist).unwrap(); // unwrap: test-only
        std::fs::write(dist.join("index.html"), "<html>").unwrap(); // unwrap: test-only

        let cm = MockContainerPort::new();
        let mut fm = MockFileManagerPort::new();
        fm.expect_ensure_dir().returning(|_| Ok(()));
        fm.expect_copy_file().returning(|_, _| Ok(()));

        let deployer = AppDeployer::new(Arc::new(cm), Arc::new(fm));
        let result = deployer.execute("test", dir.path()).await.unwrap(); // unwrap: test-only
        assert!(result.success);
        assert_eq!(result.strategy, DeployStrategy::Static);
    }

    #[tokio::test]
    async fn test_deploy_static_uses_public_dir() {
        let dir = tempfile::tempdir().unwrap(); // unwrap: test-only
        let public = dir.path().join("public");
        std::fs::create_dir(&public).unwrap(); // unwrap: test-only
        std::fs::write(public.join("index.html"), "<html>").unwrap(); // unwrap: test-only

        let cm = MockContainerPort::new();
        let mut fm = MockFileManagerPort::new();
        fm.expect_ensure_dir().returning(|_| Ok(()));
        fm.expect_copy_file().returning(|_, _| Ok(()));

        let deployer = AppDeployer::new(Arc::new(cm), Arc::new(fm));
        let result = deployer.execute("test", dir.path()).await.unwrap(); // unwrap: test-only
        assert!(result.success);
    }

    #[tokio::test]
    async fn test_deploy_static_uses_build_dir() {
        let dir = tempfile::tempdir().unwrap(); // unwrap: test-only
        let build = dir.path().join("build");
        std::fs::create_dir(&build).unwrap(); // unwrap: test-only
        std::fs::write(build.join("app.js"), "console.log('hi')").unwrap(); // unwrap: test-only

        let cm = MockContainerPort::new();
        let mut fm = MockFileManagerPort::new();
        fm.expect_ensure_dir().returning(|_| Ok(()));
        fm.expect_copy_file().returning(|_, _| Ok(()));

        let deployer = AppDeployer::new(Arc::new(cm), Arc::new(fm));
        let result = deployer.execute("test", dir.path()).await.unwrap(); // unwrap: test-only
        assert!(result.success);
        assert_eq!(result.app_port, Some(80));
    }

    #[tokio::test]
    async fn test_execute_strategy_none_fallback_static() {
        let dir = tempfile::tempdir().unwrap(); // unwrap: test-only

        let cm = MockContainerPort::new();
        let mut fm = MockFileManagerPort::new();
        fm.expect_ensure_dir().returning(|_| Ok(()));

        let deployer = AppDeployer::new(Arc::new(cm), Arc::new(fm));
        let result = deployer.execute("test", dir.path()).await.unwrap(); // unwrap: test-only
        assert!(result.success);
        assert_eq!(result.strategy, DeployStrategy::Static);
    }

    // ── port detection ───────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_detect_port_from_output_port_prefix() {
        let dir = tempfile::tempdir().unwrap(); // unwrap: test-only
        let deployer = make_deployer();
        let port = deployer
            .detect_port_from_output("starting\nPORT=9000\nready", dir.path())
            .await;
        assert_eq!(port, Some(9000));
    }

    #[tokio::test]
    async fn test_detect_port_from_output_fallback_to_env() {
        let dir = tempfile::tempdir().unwrap(); // unwrap: test-only
        std::fs::write(dir.path().join(".env"), "APP_PORT=7777\n").unwrap(); // unwrap: test-only
        let deployer = make_deployer();
        let port = deployer
            .detect_port_from_output("no port here", dir.path())
            .await;
        assert_eq!(port, Some(7777));
    }

    #[tokio::test]
    async fn test_detect_port_from_env_port_key() {
        let dir = tempfile::tempdir().unwrap(); // unwrap: test-only
        std::fs::write(dir.path().join(".env"), "PORT=6543\n").unwrap(); // unwrap: test-only
        let deployer = make_deployer();
        let port = deployer.detect_port_from_env(dir.path()).await;
        assert_eq!(port, Some(6543));
    }

    #[tokio::test]
    async fn test_detect_port_from_env_app_port_key() {
        let dir = tempfile::tempdir().unwrap(); // unwrap: test-only
        std::fs::write(
            dir.path().join(".env"),
            "DB_HOST=localhost\nAPP_PORT=2345\n",
        )
        .unwrap(); // unwrap: test-only
        let deployer = make_deployer();
        let port = deployer.detect_port_from_env(dir.path()).await;
        assert_eq!(port, Some(2345));
    }

    #[tokio::test]
    async fn test_detect_port_from_env_missing_file() {
        let dir = tempfile::tempdir().unwrap(); // unwrap: test-only
        let deployer = make_deployer();
        let port = deployer.detect_port_from_env(dir.path()).await;
        assert_eq!(port, None);
    }

    #[tokio::test]
    async fn test_detect_port_from_dockerfile_expose_plain() {
        let dir = tempfile::tempdir().unwrap(); // unwrap: test-only
        std::fs::write(
            dir.path().join("Dockerfile"),
            "FROM node:18\nEXPOSE 3001\nCMD node app.js",
        )
        .unwrap(); // unwrap: test-only
        let deployer = make_deployer();
        let port = deployer.detect_port_from_dockerfile(dir.path()).await;
        assert_eq!(port, Some(3001));
    }

    #[tokio::test]
    async fn test_detect_port_from_dockerfile_expose_with_proto() {
        let dir = tempfile::tempdir().unwrap(); // unwrap: test-only
        std::fs::write(
            dir.path().join("Dockerfile"),
            "FROM alpine\nEXPOSE 8443/tcp",
        )
        .unwrap(); // unwrap: test-only
        let deployer = make_deployer();
        let port = deployer.detect_port_from_dockerfile(dir.path()).await;
        assert_eq!(port, Some(8443));
    }

    #[tokio::test]
    async fn test_detect_port_from_dockerfile_no_expose() {
        let dir = tempfile::tempdir().unwrap(); // unwrap: test-only
        std::fs::write(dir.path().join("Dockerfile"), "FROM alpine\nRUN echo hello").unwrap(); // unwrap: test-only
        let deployer = make_deployer();
        let port = deployer.detect_port_from_dockerfile(dir.path()).await;
        assert_eq!(port, None);
    }

    #[tokio::test]
    async fn test_detect_port_from_compose_quoted_mapping() {
        let dir = tempfile::tempdir().unwrap(); // unwrap: test-only
        std::fs::write(
            dir.path().join("docker-compose.yml"),
            "services:\n  web:\n    ports:\n      - \"8080:8080\"",
        )
        .unwrap(); // unwrap: test-only
        let deployer = make_deployer();
        let port = deployer.detect_port_from_compose(dir.path()).await;
        assert_eq!(port, Some(8080));
    }

    #[tokio::test]
    async fn test_detect_port_from_compose_yaml_ext() {
        let dir = tempfile::tempdir().unwrap(); // unwrap: test-only
        std::fs::write(
            dir.path().join("docker-compose.yaml"),
            "services:\n  web:\n    ports:\n      - 9876:9876",
        )
        .unwrap(); // unwrap: test-only
        let deployer = make_deployer();
        let port = deployer.detect_port_from_compose(dir.path()).await;
        assert_eq!(port, Some(9876));
    }
}
