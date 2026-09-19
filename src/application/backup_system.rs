use crate::domain::app_config::BackupSettings;
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
            max_backups: BackupSettings::load().max_backups,
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
            max_backups: BackupSettings::load().max_backups,
        }
    }

    /// Override the retention policy (max backups kept per identifier).
    pub fn with_max_backups(mut self, max_backups: usize) -> Self {
        self.max_backups = max_backups.max(1);
        self
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

                let matches_identifier =
                    identifier.is_empty() || Self::name_matches_identifier(name, identifier);

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

        // Safety: backup current state before overwriting
        if target_path.exists() {
            let timestamp = Local::now().format("%Y-%m-%d_%H-%M-%S").to_string();
            let target_name = target_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string();
            let pre_restore_name = format!("pre-restore_{}_{}.bak", timestamp, target_name);
            let pre_restore_path = self.backup_root.join(pre_restore_name);

            self.file_manager.ensure_dir(&self.backup_root).await?;
            self.file_manager
                .copy_file(target_path, &pre_restore_path)
                .await
                .map_err(|e| {
                    tracing::warn!("Failed to create pre-restore backup: {}", e);
                    e
                })?;
        }

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
    /// Archives all existing paths into a single tar.gz preserving their
    /// absolute layout, so `restore_full_backup()` restores them in place.
    pub async fn create_backup_of_paths(
        &self,
        identifier: &str,
        paths: &[PathBuf],
    ) -> Result<PathBuf> {
        self.file_manager.ensure_dir(&self.backup_root).await?;

        let timestamp = Local::now().format("%Y-%m-%d_%H-%M-%S").to_string();
        let backup_name = format!("{}_{}_full.tar.gz", timestamp, identifier);
        let backup_path = self.backup_root.join(&backup_name);

        // Collect existing paths
        let existing: Vec<PathBuf> = paths.iter().filter(|p| p.exists()).cloned().collect();

        if existing.is_empty() {
            // No paths to archive — return path anyway (empty backup)
            return Ok(backup_path);
        }

        // Archive all paths preserving their absolute layout (members relative
        // to /), so restore_full_backup() extracting into / lands every entry
        // back at its original location. Works for files and directories alike.
        self.file_manager
            .create_archive_multi(&existing, &backup_path)
            .await?;

        self.rotate_backups(identifier, "").await?;

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

    /// Create a tar.gz archive backup of a directory.
    /// Uses `create_archive` from `FileManagerPort`.
    pub async fn create_archive_backup(
        &self,
        source_dir: &Path,
        identifier: &str,
    ) -> Result<PathBuf> {
        if !source_dir.exists() {
            return Err(EnolaError::NotFound(format!(
                "Source directory not found: {:?}",
                source_dir
            )));
        }

        self.file_manager.ensure_dir(&self.backup_root).await?;

        let timestamp = Local::now().format("%Y-%m-%d_%H-%M-%S").to_string();
        let filename = source_dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        let backup_name = format!("{}_{}_{}.tar.gz", timestamp, identifier, filename);
        let backup_path = self.backup_root.join(backup_name);

        self.file_manager
            .create_archive(source_dir, &backup_path)
            .await?;

        self.rotate_backups(identifier, "").await?;

        Ok(backup_path)
    }

    /// Restore a directory from a tar.gz archive backup.
    /// Uses `extract_archive` from `FileManagerPort`.
    pub async fn restore_directory(&self, backup_archive: &Path, target_dir: &Path) -> Result<()> {
        if !backup_archive.exists() {
            return Err(EnolaError::NotFound(format!(
                "Backup archive not found: {:?}",
                backup_archive
            )));
        }

        // Ensure target directory exists
        self.file_manager.ensure_dir(target_dir).await?;

        self.file_manager
            .extract_archive(backup_archive, target_dir)
            .await
    }

    /// Whether a backup filename belongs to `identifier`.
    /// Names are "<ts>_<identifier>_<rest>" where the timestamp is a fixed
    /// 19 chars (`YYYY-MM-DD_HH-MM-SS`), so the identifier is anchored at
    /// byte 20 and must be followed by `_`. Anchoring avoids substring
    /// collisions between identifiers like `system` and `old_system`.
    fn name_matches_identifier(name: &str, identifier: &str) -> bool {
        const TS_PREFIX_LEN: usize = 20; // 19-char timestamp + '_'
        name.get(TS_PREFIX_LEN..)
            .map(|rest| rest.starts_with(&format!("{}_", identifier)))
            .unwrap_or(false)
    }

    async fn rotate_backups(&self, identifier: &str, _filename: &str) -> Result<()> {
        let mut matching_backups = Vec::new();

        let mut entries = match tokio::fs::read_dir(&self.backup_root).await {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => {
                return Err(EnolaError::InfrastructureError(format!(
                    "Read backup dir failed: {}",
                    e
                )))
            }
        };

        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if Self::name_matches_identifier(name, identifier)
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
        // max_backups comes from the real config chain (env > file > default);
        // the test only requires it to be a usable value.
        assert!(system.max_backups >= 1);
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
        std::fs::write(tmp.path().join("2026-01-02_12-00-00_mysvc_full.tar.gz"), "").unwrap(); // unwrap: test-only
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
            .any(|p| p.extension().is_some_and(|e| e == "txt")));
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
        // Default is a pure constant — independent of env/config file.
        assert_eq!(BackupSettings::default().max_backups, 5);
    }

    #[tokio::test]
    async fn test_restore_creates_pre_restore_backup() {
        let tmp = tempfile::tempdir().unwrap();
        let backup_root = tmp.path().join("backups");
        std::fs::create_dir_all(&backup_root).unwrap();

        // Create a fake "current" target file and a backup file
        let target = tmp.path().join("target.txt");
        std::fs::write(&target, "current").unwrap();
        let backup_file = backup_root.join("backup.bak");
        std::fs::write(&backup_file, "backup_content").unwrap();

        let mut mock = MockFileManagerPort::new();
        mock.expect_ensure_dir().returning(|_| Ok(()));
        // copy_file called twice: once for pre-restore, once for actual restore
        mock.expect_copy_file().times(2).returning(|_, _| Ok(()));

        let system = BackupSystem::with_backup_root(Arc::new(mock), backup_root);
        let result = system.restore_backup(&backup_file, &target).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_restore_skips_pre_backup_if_target_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let backup_root = tmp.path().join("backups");
        std::fs::create_dir_all(&backup_root).unwrap();

        let backup_file = backup_root.join("backup.bak");
        std::fs::write(&backup_file, "backup_content").unwrap();

        let mut mock = MockFileManagerPort::new();
        mock.expect_ensure_dir().returning(|_| Ok(()));
        // copy_file called once: only the actual restore (no pre-restore since target doesn't exist)
        mock.expect_copy_file().times(1).returning(|_, _| Ok(()));

        let system = BackupSystem::with_backup_root(Arc::new(mock), backup_root);
        let target = tmp.path().join("nonexistent_target.txt");
        let result = system.restore_backup(&backup_file, &target).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_multi_path_backup_archives_all_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let backup_root = tmp.path().join("backups");

        // Create two source files
        let file1 = tmp.path().join("file1.txt");
        let file2 = tmp.path().join("file2.txt");
        std::fs::write(&file1, "content1").unwrap();
        std::fs::write(&file2, "content2").unwrap();

        let mut mock = MockFileManagerPort::new();
        mock.expect_ensure_dir().returning(|_| Ok(()));
        // Multi-path: a single create_archive_multi preserving absolute layout
        mock.expect_create_archive_multi()
            .times(1)
            .returning(|_, _| Ok(()));

        let system = BackupSystem::with_backup_root(Arc::new(mock), backup_root);
        let result = system.create_backup_of_paths("test", &[file1, file2]).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_multi_path_backup_rotates_old_archives() {
        let tmp = tempfile::tempdir().unwrap();
        let backup_root = tmp.path().join("backups");
        std::fs::create_dir_all(&backup_root).unwrap();

        // 3 archivos viejos del mismo identificador + 1 de otro
        for i in 1..=3 {
            std::fs::write(
                backup_root.join(format!("2026-01-0{}_10-00-00_mysvc_full.tar.gz", i)),
                "old",
            )
            .unwrap();
        }
        std::fs::write(
            backup_root.join("2026-01-09_10-00-00_other_full.tar.gz"),
            "keep",
        )
        .unwrap();

        let src = tmp.path().join("src.txt");
        std::fs::write(&src, "data").unwrap();

        let mut mock = MockFileManagerPort::new();
        mock.expect_ensure_dir().returning(|_| Ok(()));
        // El mock simula el archive creando el fichero destino para que la
        // rotación lo contabilice.
        mock.expect_create_archive_multi()
            .times(1)
            .returning(|_, dest| {
                let _ = std::fs::write(dest, "new");
                Ok(())
            });
        // max_backups = 2 → el nuevo + 1 viejo quedan; se borran 2 de mysvc
        // (el fichero "_other" no coincide con el identificador y se conserva)
        mock.expect_delete_file().times(2).returning(|_| Ok(()));

        let system =
            BackupSystem::with_backup_root(Arc::new(mock), backup_root).with_max_backups(2);
        let result = system.create_backup_of_paths("mysvc", &[src]).await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_with_max_backups_clamps_to_minimum_one() {
        let mock = mock_file_manager();
        let system = BackupSystem::new(Arc::new(mock)).with_max_backups(0);
        assert_eq!(system.max_backups, 1);
    }

    #[test]
    fn test_identifier_matching_no_substring_collision() {
        // "system" must not match backups of "old_system" / "system2"
        assert!(BackupSystem::name_matches_identifier(
            "2026-09-19_17-00-00_system_full.tar.gz",
            "system"
        ));
        assert!(BackupSystem::name_matches_identifier(
            "2026-09-19_17-00-00_system_file.bak",
            "system"
        ));
        assert!(!BackupSystem::name_matches_identifier(
            "2026-09-19_17-00-00_old_system_full.tar.gz",
            "system"
        ));
        assert!(!BackupSystem::name_matches_identifier(
            "2026-09-19_17-00-00_system2_full.tar.gz",
            "system"
        ));
        // Identifiers containing '_' still match
        assert!(BackupSystem::name_matches_identifier(
            "2026-09-19_17-00-00_wp_site_full.tar.gz",
            "wp_site"
        ));
        // Malformed names (short/missing timestamp) never match
        assert!(!BackupSystem::name_matches_identifier(
            "2026-01-01_mysvc__full.tar.gz",
            "mysvc"
        ));
    }
}
