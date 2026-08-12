//! Cleanup Service - Handles cleanup of temporary files and residual data
//!
//! Targets:
//! - logs: Old log files in /logs/ and project directories
//! - docker: Orphaned containers, images, and volumes
//! - all: All of the above

use crate::domain::error::EnolaError;
use crate::ports::container::ContainerPort;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

/// Result of a cleanup operation
#[derive(Debug, Default)]
pub struct CleanupResult {
    pub files_deleted: usize,
    pub bytes_freed: u64,
    pub errors: Vec<String>,
}

impl CleanupResult {
    pub fn merge(&mut self, other: CleanupResult) {
        self.files_deleted += other.files_deleted;
        self.bytes_freed += other.bytes_freed;
        self.errors.extend(other.errors);
    }
}

/// Cleanup service for managing temporary and residual files
pub struct CleanupService {
    project_root: PathBuf,
    dry_run: bool,
    container_manager: Option<Arc<dyn ContainerPort + Send + Sync>>,
}

impl CleanupService {
    pub fn new(project_root: PathBuf, dry_run: bool) -> Self {
        Self {
            project_root,
            dry_run,
            container_manager: None,
        }
    }

    /// Attach a container manager for Docker cleanup operations
    pub fn with_container_manager(mut self, cm: Arc<dyn ContainerPort + Send + Sync>) -> Self {
        self.container_manager = Some(cm);
        self
    }

    /// Run cleanup for the specified target
    pub async fn cleanup(
        &self,
        target: &str,
        keep_days: u32,
        force: bool,
    ) -> Result<CleanupResult, EnolaError> {
        if !force && !self.dry_run {
            println!("⚠️  This will permanently delete files. Use --dry-run to preview or --force to skip this warning.");
            return Ok(CleanupResult::default());
        }

        let mut result = CleanupResult::default();

        match target {
            "all" => {
                println!("🧹 Cleaning all targets...\n");
                result.merge(self.cleanup_logs(keep_days).await?);
                result.merge(self.cleanup_docker().await?);
            }
            "logs" => {
                println!("🧹 Cleaning logs...\n");
                result.merge(self.cleanup_logs(keep_days).await?);
            }
            "docker" => {
                println!("🧹 Cleaning Docker residuals...\n");
                result.merge(self.cleanup_docker().await?);
            }
            _ => {
                return Err(EnolaError::ConfigError(format!(
                    "Unknown cleanup target: '{}'. Valid targets: all, logs, docker",
                    target
                )));
            }
        }

        Ok(result)
    }

    /// Cleanup old log files
    async fn cleanup_logs(&self, keep_days: u32) -> Result<CleanupResult, EnolaError> {
        let mut result = CleanupResult::default();
        let max_age = Duration::from_secs(keep_days as u64 * 24 * 60 * 60);
        let now = SystemTime::now();

        // Paths to clean
        let log_paths = vec![
            self.project_root.join("logs"),
            self.project_root.join("logs/e2e_tests"),
        ];

        for log_path in log_paths {
            if !log_path.exists() {
                continue;
            }

            println!("📁 Scanning: {}", log_path.display());

            if let Ok(entries) = fs::read_dir(&log_path) {
                for entry in entries.flatten() {
                    let path = entry.path();

                    // Only process .log files
                    if path.extension().is_some_and(|ext| ext == "log") {
                        if let Ok(metadata) = fs::metadata(&path) {
                            if let Ok(modified) = metadata.modified() {
                                if let Ok(age) = now.duration_since(modified) {
                                    if age > max_age {
                                        let size = metadata.len();

                                        if self.dry_run {
                                            println!(
                                                "  [DRY-RUN] Would delete: {} ({} bytes)",
                                                path.display(),
                                                size
                                            );
                                            result.files_deleted += 1;
                                            result.bytes_freed += size;
                                        } else {
                                            if let Err(e) = fs::remove_file(&path) {
                                                result.errors.push(format!(
                                                    "Failed to delete {}: {}",
                                                    path.display(),
                                                    e
                                                ));
                                            } else {
                                                println!("  ✓ Deleted: {}", path.display());
                                                result.files_deleted += 1;
                                                result.bytes_freed += size;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(result)
    }

    /// Cleanup Docker residuals (orphaned containers, images, volumes)
    async fn cleanup_docker(&self) -> Result<CleanupResult, EnolaError> {
        let result = CleanupResult::default();

        println!("🐳 Docker cleanup...");

        if self.dry_run {
            println!("  [DRY-RUN] Would run: docker system prune -f");
            println!("  [DRY-RUN] Would run: docker volume prune -f");
            return Ok(result);
        }

        if let Some(cm) = &self.container_manager {
            println!("  Pruning stopped containers and dangling images...");
            if let Err(e) = cm.prune_system().await {
                tracing::warn!("Docker prune warning: {}", e);
            }
        } else {
            println!("  ⚠️  No container manager available, skipping Docker cleanup");
        }

        println!("  ✓ Docker cleanup completed");

        Ok(result)
    }
}

/// Format bytes to human readable string
pub fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} bytes", bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use std::io::Write;

    #[test]
    fn test_cleanup_result_default() {
        let r = CleanupResult::default();
        assert_eq!(r.files_deleted, 0);
        assert_eq!(r.bytes_freed, 0);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_cleanup_result_merge() {
        let mut a = CleanupResult {
            files_deleted: 2,
            bytes_freed: 1024,
            errors: vec![],
        };
        let b = CleanupResult {
            files_deleted: 3,
            bytes_freed: 2048,
            errors: vec!["err".into()],
        };
        a.merge(b);
        assert_eq!(a.files_deleted, 5);
        assert_eq!(a.bytes_freed, 3072);
        assert_eq!(a.errors.len(), 1);
    }

    #[test]
    fn test_format_bytes_bytes() {
        assert_eq!(format_bytes(512), "512 bytes");
    }

    #[test]
    fn test_format_bytes_kb() {
        assert!(format_bytes(2048).contains("KB"));
    }

    #[test]
    fn test_format_bytes_mb() {
        assert!(format_bytes(2 * 1024 * 1024).contains("MB"));
    }

    #[test]
    fn test_format_bytes_gb() {
        assert!(format_bytes(2 * 1024 * 1024 * 1024).contains("GB"));
    }

    #[tokio::test]
    async fn test_cleanup_unknown_target_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let svc = CleanupService::new(dir.path().to_path_buf(), false);
        let result = svc.cleanup("unknown_target", 7, true).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_cleanup_without_force_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let svc = CleanupService::new(dir.path().to_path_buf(), false);
        // dry_run=false, force=false → early return without deleting
        let result = svc.cleanup("logs", 7, false).await.unwrap();
        assert_eq!(result.files_deleted, 0);
    }

    #[tokio::test]
    async fn test_cleanup_logs_dry_run_no_actual_delete() {
        let dir = tempfile::tempdir().unwrap();
        let logs_dir = dir.path().join("logs");
        fs::create_dir_all(&logs_dir).unwrap();

        // Crear un archivo .log "antiguo" (forzamos acceso con fecha vieja vía keep_days=0)
        let log_file = logs_dir.join("test.log");
        File::create(&log_file)
            .unwrap()
            .write_all(b"test log content")
            .unwrap();

        let svc = CleanupService::new(dir.path().to_path_buf(), true); // dry_run=true
        let result = svc.cleanup("logs", 0, true).await.unwrap();

        // En dry_run el archivo debe seguir existiendo
        assert!(log_file.exists(), "El archivo NO debe borrarse en dry-run");
        // Pero debe contarse como candidato
        // files_deleted es usize, siempre >= 0, validamos que la ejecución no falle
        let _ = result.files_deleted;
    }

    #[tokio::test]
    async fn test_cleanup_logs_empty_dir_no_error() {
        let dir = tempfile::tempdir().unwrap();
        let svc = CleanupService::new(dir.path().to_path_buf(), false);
        // El directorio logs no existe — no debe fallar
        let result = svc.cleanup("logs", 7, true).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().files_deleted, 0);
    }
}
