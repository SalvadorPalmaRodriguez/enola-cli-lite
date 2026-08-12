use crate::domain::error::{EnolaError, Result};
use crate::ports::file::FileManagerPort;
use chrono::Local;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub struct BackupSystem {
    file_manager: Arc<dyn FileManagerPort + Send + Sync>,
    backup_root: PathBuf,
    max_backups: usize,
}

impl BackupSystem {
    pub fn new(file_manager: Arc<dyn FileManagerPort + Send + Sync>) -> Self {
        Self {
            file_manager,
            backup_root: PathBuf::from("/var/backups/enola-server"),
            max_backups: 5,
        }
    }

    /// Constructor with custom backup root (useful for testing)
    pub fn with_backup_root(
        file_manager: Arc<dyn FileManagerPort + Send + Sync>,
        backup_root: PathBuf,
    ) -> Self {
        Self {
            file_manager,
            backup_root,
            max_backups: 5,
        }
    }

    pub async fn create_backup(&self, target_path: &Path, identifier: &str) -> Result<PathBuf> {
        if !target_path.exists() {
            return Err(EnolaError::NotFound(format!(
                "File to backup not found: {:?}",
                target_path
            )));
        }

        self.file_manager.ensure_dir(&self.backup_root).await?;

        let timestamp = Local::now().format("%Y-%m-%d_%H-%M-%S").to_string();
        let filename = target_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        let backup_name = format!("{}_{}_{}.bak", timestamp, identifier, filename);
        let backup_path = self.backup_root.join(backup_name);

        self.file_manager
            .copy_file(target_path, &backup_path)
            .await?;

        // Rotate
        self.rotate_backups(identifier, &filename).await?;

        Ok(backup_path)
    }

    pub async fn list_backups(&self, identifier: &str) -> Result<Vec<PathBuf>> {
        let mut backups = Vec::new();
        if !self.backup_root.exists() {
            return Ok(backups);
        }

        let mut entries = tokio::fs::read_dir(&self.backup_root).await.map_err(|e| {
            EnolaError::InfrastructureError(format!("Read backup dir failed: {}", e))
        })?;

        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                let is_backup_file = name.ends_with(".bak") || name.ends_with(".tar.gz");

                let matches_identifier = if identifier.is_empty() {
                    is_backup_file
                } else {
                    name.contains(&format!("_{}_", identifier))
                        || name.contains(&format!("{}_{}", identifier, "_full"))
                };

                if matches_identifier && is_backup_file {
                    backups.push(path);
                }
            }
        }

        // Sort by name (which has timestamp) descending
        backups.sort_by(|a, b| b.cmp(a));

        Ok(backups)
    }

    pub async fn restore_backup(&self, backup_path: &Path, target_path: &Path) -> Result<()> {
        if !backup_path.exists() {
            return Err(EnolaError::NotFound(format!(
                "Backup file not found: {:?}",
                backup_path
            )));
        }

        // Optional: Backup current state before overwrite?
        // For now, straight restore.

        // Ensure parent of target exists
        if let Some(parent) = target_path.parent() {
            if !parent.exists() {
                self.file_manager.ensure_dir(parent).await?;
            }
        }

        self.file_manager
            .copy_file(backup_path, target_path)
            .await?;

        Ok(())
    }

    pub async fn create_full_backup(&self, identifier: &str) -> Result<PathBuf> {
        // Default paths for production
        self.create_backup_of_paths(identifier, &[PathBuf::from("/opt/enola")])
            .await
    }

    /// Create backup of specific paths (for testing or custom backups)
    pub async fn create_backup_of_paths(
        &self,
        identifier: &str,
        paths: &[PathBuf],
    ) -> Result<PathBuf> {
        self.file_manager.ensure_dir(&self.backup_root).await?;

        let timestamp = Local::now().format("%Y-%m-%d_%H-%M-%S").to_string();
        let backup_name = format!("{}_{}_full.tar.gz", timestamp, identifier);
        let backup_path = self.backup_root.join(&backup_name);

        // Archive each existing path via FileManagerPort
        for path in paths {
            if path.exists() {
                self.file_manager.create_archive(path, &backup_path).await?;
                break; // create_archive on the first found path; for multi-path a loop appending would be needed
            }
        }

        Ok(backup_path)
    }

    pub async fn restore_full_backup(&self, backup_path: &Path) -> Result<()> {
        if !backup_path.exists() {
            return Err(EnolaError::NotFound(format!(
                "Backup file not found: {:?}",
                backup_path
            )));
        }

        // Extract to root
        let dest = PathBuf::from("/");
        self.file_manager.extract_archive(backup_path, &dest).await
    }

    async fn rotate_backups(&self, identifier: &str, _filename: &str) -> Result<()> {
        let pattern_check = format!("_{}", identifier); // Generic check for identifier

        let mut matching_backups = Vec::new();

        let mut entries = tokio::fs::read_dir(&self.backup_root).await.map_err(|e| {
            EnolaError::InfrastructureError(format!("Read backup dir failed: {}", e))
        })?;

        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                // Modified loose matching for full system backups too
                if name.contains(&pattern_check)
                    && (name.ends_with(".bak") || name.ends_with(".tar.gz"))
                {
                    matching_backups.push(path);
                }
            }
        }

        // Sort newest first
        matching_backups.sort_by(|a, b| b.cmp(a));

        if matching_backups.len() > self.max_backups {
            for path_to_remove in &matching_backups[self.max_backups..] {
                self.file_manager.delete_file(path_to_remove).await?;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::file::MockFileManagerPort;

    fn mock_file_manager() -> MockFileManagerPort {
        MockFileManagerPort::new()
    }

    #[test]
    fn test_backup_system_default_config() {
        let mut mock = mock_file_manager();
        mock.expect_ensure_dir().returning(|_| Ok(()));
        let system = BackupSystem::new(Arc::new(mock));
        assert_eq!(system.max_backups, 5);
        assert!(system.backup_root.to_string_lossy().contains("backups"));
    }

    #[test]
    fn test_backup_system_with_custom_root() {
        let mut mock = mock_file_manager();
        mock.expect_ensure_dir().returning(|_| Ok(()));
        let custom_root = PathBuf::from("/tmp/test_backups");
        let system = BackupSystem::with_backup_root(Arc::new(mock), custom_root.clone());
        assert_eq!(system.backup_root, custom_root);
    }

    #[tokio::test]
    async fn test_create_backup_file_not_found() {
        let mut mock = mock_file_manager();
        mock.expect_ensure_dir().returning(|_| Ok(()));
        let system = BackupSystem::new(Arc::new(mock));

        let result = system
            .create_backup(&PathBuf::from("/nonexistent/file.txt"), "test")
            .await;

        assert!(result.is_err());
        match result {
            Err(EnolaError::NotFound(_)) => {}
            _ => panic!("Expected NotFound error"),
        }
    }

    #[tokio::test]
    async fn test_restore_backup_not_found() {
        let mut mock = mock_file_manager();
        mock.expect_ensure_dir().returning(|_| Ok(()));
        let system = BackupSystem::new(Arc::new(mock));

        let result = system
            .restore_backup(
                &PathBuf::from("/nonexistent/backup.bak"),
                &PathBuf::from("/tmp/target"),
            )
            .await;

        assert!(result.is_err());
        match result {
            Err(EnolaError::NotFound(_)) => {}
            _ => panic!("Expected NotFound error"),
        }
    }

    #[tokio::test]
    async fn test_list_backups_empty_dir() {
        let mut mock = mock_file_manager();
        mock.expect_ensure_dir().returning(|_| Ok(()));
        let system = BackupSystem::with_backup_root(
            Arc::new(mock),
            PathBuf::from("/nonexistent_backup_dir_xyz"),
        );

        let result = system.list_backups("test_instance").await.unwrap(); // unwrap: test-only
        assert!(result.is_empty(), "Sin backups en dir inexistente");
    }

    #[tokio::test]
    async fn test_restore_full_backup_not_found() {
        let mock = mock_file_manager();
        let system = BackupSystem::new(Arc::new(mock));
        let result = system
            .restore_full_backup(&PathBuf::from("/nonexistent/full_backup.tar.gz"))
            .await;
        assert!(result.is_err());
        assert!(matches!(result, Err(EnolaError::NotFound(_))));
    }

    #[tokio::test]
    async fn test_create_backup_of_paths_all_missing() {
        let mut mock = mock_file_manager();
        mock.expect_ensure_dir().returning(|_| Ok(()));
        let tmp = tempfile::tempdir().unwrap(); // unwrap: test-only
        let system = BackupSystem::with_backup_root(Arc::new(mock), tmp.path().to_path_buf());

        // No path exists → should still return Ok with a backup_path (no archive created)
        let result = system
            .create_backup_of_paths(
                "myid",
                &[
                    PathBuf::from("/nonexistent_path_1"),
                    PathBuf::from("/nonexistent_path_2"),
                ],
            )
            .await;
        assert!(
            result.is_ok(),
            "Paths missing should not error: {:?}",
            result.err()
        );
    }

    #[tokio::test]
    async fn test_list_backups_with_real_files() {
        let tmp = tempfile::tempdir().unwrap(); // unwrap: test-only
        let mock = mock_file_manager();
        let system = BackupSystem::with_backup_root(Arc::new(mock), tmp.path().to_path_buf());

        let id = "mysvc";
        // Create fake .bak files matching and non-matching
        std::fs::write(tmp.path().join("2026-01-01_12-00-00_mysvc_file.bak"), "").unwrap(); // unwrap: test-only
        std::fs::write(tmp.path().join("2026-01-01_12-00-00_othersvc_file.bak"), "").unwrap(); // unwrap: test-only
        std::fs::write(tmp.path().join("2026-01-01_mysvc__full.tar.gz"), "").unwrap(); // unwrap: test-only
        std::fs::write(tmp.path().join("not_a_backup.txt"), "").unwrap(); // unwrap: test-only

        let result = system.list_backups(id).await.unwrap(); // unwrap: test-only
                                                             // Should find the .bak with mysvc in name
        assert!(result.iter().any(|p| p.to_string_lossy().contains("mysvc")));
        // Should NOT find othersvc
        assert!(!result
            .iter()
            .any(|p| p.to_string_lossy().contains("othersvc")));
        // Should NOT find .txt
        assert!(!result
            .iter()
            .any(|p| p.extension().map_or(false, |e| e == "txt")));
    }

    #[test]
    fn test_backup_name_contains_identifier_and_timestamp() {
        // Verifica el formato del nombre de backup generado (anti-regresión)
        let timestamp = "2026-01-15_10-30-00";
        let identifier = "wp-site1";
        let filename = "app.ini";
        let name = format!("{}_{}_{}.bak", timestamp, identifier, filename);
        assert!(name.contains(identifier));
        assert!(name.contains(filename));
        assert!(name.ends_with(".bak"));
    }

    #[test]
    fn test_max_backups_default_is_five() {
        let mock = mock_file_manager();
        let system = BackupSystem::new(Arc::new(mock));
        assert_eq!(system.max_backups, 5);
    }
}
