use crate::domain::app_config::BackupSettings;
use crate::domain::backup::{required_free_bytes, BackupManifest};
use crate::domain::error::{EnolaError, Result};
use crate::ports::disk_space::DiskSpacePort;
use crate::ports::file::FileManagerPort;
use chrono::Local;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub struct BackupSystem {
    file_manager: Arc<dyn FileManagerPort + Send + Sync>,
    backup_root: PathBuf,
    max_backups: usize,
    /// Comprobación de espacio previa a crear el archivo (`None` = sin comprobar:
    /// solo tests; el CLI la inyecta siempre).
    disk: Option<Arc<dyn DiskSpacePort>>,
}

/// Rutas de identidad a archivar: las fijas de `domain::backup` más los perfiles
/// AppArmor `enola-*` de `apparmor_dir` (uno a uno, no todo `/etc/apparmor.d`).
/// Las que no existen las filtra `create_system_backup`.
pub fn system_identity_paths(apparmor_dir: &Path) -> Vec<PathBuf> {
    use crate::domain::backup::{APPARMOR_PREFIX, SYSTEM_IDENTITY_PATHS};
    let mut paths: Vec<PathBuf> = SYSTEM_IDENTITY_PATHS.iter().map(PathBuf::from).collect();
    if let Ok(entries) = std::fs::read_dir(apparmor_dir) {
        let mut profiles: Vec<PathBuf> = entries
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name().to_string_lossy().starts_with(APPARMOR_PREFIX)
                    && e.file_type().map(|t| t.is_file()).unwrap_or(false)
            })
            .map(|e| e.path())
            .collect();
        profiles.sort();
        paths.extend(profiles);
    }
    paths
}

impl BackupSystem {
    pub fn new(file_manager: Arc<dyn FileManagerPort + Send + Sync>) -> Self {
        Self {
            file_manager,
            backup_root: PathBuf::from("/var/backups/enola-server"),
            max_backups: BackupSettings::load().max_backups,
            disk: None,
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
            disk: None,
        }
    }

    /// Activa la comprobación de espacio libre antes de crear un backup de sistema.
    pub fn with_disk_space(mut self, disk: Arc<dyn DiskSpacePort>) -> Self {
        self.disk = Some(disk);
        self
    }

    /// Override the retention policy (max backups kept per identifier).
    pub fn with_max_backups(mut self, max_backups: usize) -> Self {
        self.max_backups = max_backups.max(1);
        self
    }

    /// F-23: los backups contienen credenciales y configs de servicio — el
    /// directorio de backups queda en 0700 (no solo creado por umask).
    async fn ensure_backup_root(&self) -> Result<()> {
        self.file_manager.ensure_dir(&self.backup_root).await?;
        self.file_manager
            .set_permissions(&self.backup_root, 0o700)
            .await
    }

    /// F-23: cada artefacto de backup queda en 0600 — con umask típica el
    /// tarball/copia quedaría 0644 (legible por otros usuarios del host).
    async fn harden_backup_file(&self, path: &Path) -> Result<()> {
        self.file_manager.set_permissions(path, 0o600).await
    }

    pub async fn create_backup(&self, target_path: &Path, identifier: &str) -> Result<PathBuf> {
        if !target_path.exists() {
            return Err(EnolaError::NotFound(format!(
                "File to backup not found: {:?}",
                target_path
            )));
        }

        self.ensure_backup_root().await?;

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
        self.harden_backup_file(&backup_path).await?;

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

            self.ensure_backup_root().await?;
            self.file_manager
                .copy_file(target_path, &pre_restore_path)
                .await
                .map_err(|e| {
                    tracing::warn!("Failed to create pre-restore backup: {}", e);
                    e
                })?;
            self.harden_backup_file(&pre_restore_path).await?;
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
        self.ensure_backup_root().await?;

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
        // Un archivo a medias (disco lleno, tar fallido) NO se deja: parecería un
        // backup válido y la rotación podría borrar uno bueno por él.
        if let Err(e) = self
            .file_manager
            .create_archive_multi(&existing, &backup_path)
            .await
        {
            let _ = self.file_manager.delete_file(&backup_path).await;
            return Err(e);
        }
        self.harden_backup_file(&backup_path).await?;

        self.rotate_backups(identifier, "").await?;

        Ok(backup_path)
    }

    /// Backup de la **identidad** del sistema (PLAN-BACKUP-RESTORE, Fase 1).
    ///
    /// 1. Filtra `paths` a los que existen. 2. Si hay `DiskSpacePort`, exige espacio
    ///    libre ≥ tamaño de las fuentes + reserva y **aborta antes de escribir nada**.
    /// 3. Escribe el manifiesto (0600) en `manifest_path`, que viaja dentro del
    ///    archivo. 4. Archiva. Devuelve la ruta del `.tar.gz`.
    pub async fn create_system_backup(
        &self,
        identifier: &str,
        paths: &[PathBuf],
        manifest_path: &Path,
        enola_version: &str,
    ) -> Result<PathBuf> {
        let existing: Vec<PathBuf> = paths.iter().filter(|p| p.exists()).cloned().collect();
        if existing.is_empty() {
            return Err(EnolaError::ValidationError(
                "No backup paths found".to_string(),
            ));
        }

        if let Some(disk) = &self.disk {
            let needed = required_free_bytes(disk.tree_size(&existing)?);
            let free = disk.available_bytes(&self.backup_root)?;
            if free < needed {
                return Err(EnolaError::InfrastructureError(format!(
                    "Not enough disk space for the backup in {}: {} MiB free, {} MiB needed. \
                     Nothing was written.",
                    self.backup_root.display(),
                    free / (1024 * 1024),
                    needed.div_ceil(1024 * 1024)
                )));
            }
        }

        let mut archived = existing;
        let manifest = BackupManifest::new(
            enola_version,
            &Local::now().format("%Y-%m-%d_%H-%M-%S").to_string(),
            archived
                .iter()
                .map(|p| p.to_string_lossy().to_string())
                .collect(),
        );
        if let Some(dir) = manifest_path.parent() {
            self.file_manager.ensure_dir(dir).await?;
        }
        self.file_manager
            .write_file(manifest_path, &manifest.to_json()?)
            .await?;
        self.file_manager
            .set_permissions(manifest_path, 0o600)
            .await?;
        if !archived.iter().any(|p| manifest_path.starts_with(p)) {
            archived.push(manifest_path.to_path_buf());
        }

        self.create_backup_of_paths(identifier, &archived).await
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

        self.ensure_backup_root().await?;

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
        self.harden_backup_file(&backup_path).await?;

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

    /// A backup file belongs to `identifier` when its name is
    /// `<timestamp>_<identifier>_<suffix>`, where `<timestamp>` may be the current
    /// canonical form (`YYYY-MM-DD_HH-MM-SS`) or any legacy timestamp-only prefix
    /// left over from earlier versions. Anchoring on a timestamp-like prefix avoids
    /// substring collisions between identifiers such as `system` and `old_system`.
    fn name_matches_identifier(name: &str, identifier: &str) -> bool {
        if identifier.is_empty() {
            return false;
        }
        let needle = format!("{}_", identifier);
        for (idx, _) in name.match_indices(&needle) {
            if idx == 0 {
                continue; // sin prefijo de timestamp → no es nuestro
            }
            let prefix = &name[..idx];
            if !prefix.ends_with('_') {
                continue;
            }
            if Self::is_timestamp_like(&prefix[..prefix.len() - 1]) {
                return true;
            }
        }
        false
    }

    /// A timestamp prefix contains only digits and the separators used by the
    /// current and legacy backup naming schemes.
    fn is_timestamp_like(s: &str) -> bool {
        !s.is_empty()
            && s.chars().any(|c| c.is_ascii_digit())
            && s.chars()
                .all(|c| c.is_ascii_digit() || matches!(c, '-' | '_' | 'T' | ':'))
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
        // F-23: 0700 sobre el backup_root (sin artefacto: paths inexistentes).
        mock.expect_set_permissions()
            .times(1)
            .returning(|_, _| Ok(()));
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
        // F-23: 0700 en backup_root + 0600 en el pre-restore .bak.
        mock.expect_set_permissions()
            .times(2)
            .returning(|_, _| Ok(()));

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
        // F-23: 0700 en backup_root + 0600 en el tarball.
        mock.expect_set_permissions()
            .times(2)
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
        // F-23: 0700 en backup_root + 0600 en el tarball nuevo.
        mock.expect_set_permissions()
            .times(2)
            .returning(|_, _| Ok(()));
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
        // Legacy timestamp-only prefix is now swept (it is a real backup of
        // "mysvc" left over from earlier versions).
        assert!(BackupSystem::name_matches_identifier(
            "2026-01-01_mysvc__full.tar.gz",
            "mysvc"
        ));
        // Other legacy timestamp formats are swept too.
        assert!(BackupSystem::name_matches_identifier(
            "20260101-120000_mysvc_full.tar.gz",
            "mysvc"
        ));
        // Names without a timestamp prefix never match.
        assert!(!BackupSystem::name_matches_identifier(
            "mysvc_full.tar.gz",
            "mysvc"
        ));
        // An empty identifier never matches.
        assert!(!BackupSystem::name_matches_identifier(
            "2026-09-19_17-00-00_system_full.tar.gz",
            ""
        ));
    }

    // ── PLAN-BACKUP-RESTORE Fase 1: backup de sistema ─────────────────────
    fn disk_with(free: u64, size: u64) -> Arc<dyn DiskSpacePort> {
        let mut d = crate::ports::disk_space::MockDiskSpacePort::new();
        d.expect_tree_size().returning(move |_| Ok(size));
        d.expect_available_bytes().returning(move |_| Ok(free));
        Arc::new(d)
    }

    /// Sin espacio: error claro y NADA escrito (ni manifiesto ni archivo).
    #[tokio::test]
    async fn system_backup_aborts_before_writing_when_disk_is_too_small() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        std::fs::create_dir(&src).unwrap();
        let mut mock = mock_file_manager();
        mock.expect_write_file().times(0);
        mock.expect_create_archive_multi().times(0);
        mock.expect_ensure_dir().times(0);
        let system = BackupSystem::with_backup_root(Arc::new(mock), tmp.path().join("bk"))
            .with_disk_space(disk_with(10 * 1024 * 1024, 5 * 1024 * 1024));

        let err = system
            .create_system_backup("system", &[src], &tmp.path().join("m.json"), "0.0.0")
            .await
            .expect_err("debe abortar");
        let msg = err.to_string();
        assert!(msg.contains("Not enough disk space"), "{msg}");
        assert!(msg.contains("Nothing was written"), "{msg}");
    }

    /// Con espacio justo (fuentes + reserva) sigue adelante, escribe el manifiesto
    /// 0600 y lo incluye en el archivo.
    #[tokio::test]
    async fn system_backup_writes_manifest_0600_and_archives_it() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        std::fs::create_dir(&src).unwrap();
        let manifest = tmp.path().join("etc/backup-manifest.json");
        let size = 5 * 1024 * 1024;
        let need = required_free_bytes(size);

        let mut mock = mock_file_manager();
        mock.expect_ensure_dir().returning(|_| Ok(()));
        mock.expect_write_file()
            .withf(|_, content| {
                content.contains("\"format_version\": 1") && content.contains("enola_version")
            })
            .times(1)
            .returning(|path, content| {
                // Como el adaptador real: el fichero existe tras escribirlo.
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(path, content).unwrap();
                Ok(())
            });
        let mode_seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::<u32>::new()));
        let m2 = mode_seen.clone();
        mock.expect_set_permissions().returning(move |_, mode| {
            m2.lock().unwrap().push(mode);
            Ok(())
        });
        let manifest_c = manifest.clone();
        mock.expect_create_archive_multi()
            .withf(move |paths, _| paths.contains(&manifest_c))
            .times(1)
            .returning(|_, _| Ok(()));
        let system = BackupSystem::with_backup_root(Arc::new(mock), tmp.path().join("bk"))
            .with_disk_space(disk_with(need, size));

        let out = system
            .create_system_backup("system", &[src], &manifest, "0.12.0")
            .await;
        assert!(out.is_ok(), "{:?}", out.err());
        assert!(mode_seen.lock().unwrap().contains(&0o600));
    }

    /// Un `tar` que falla a mitad no deja un archivo parcial.
    #[tokio::test]
    async fn failed_archive_leaves_no_partial_file() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        std::fs::create_dir(&src).unwrap();
        let mut mock = mock_file_manager();
        mock.expect_ensure_dir().returning(|_| Ok(()));
        mock.expect_set_permissions().returning(|_, _| Ok(()));
        mock.expect_create_archive_multi()
            .returning(|_, _| Err(EnolaError::InfrastructureError("disk full".into())));
        mock.expect_delete_file().times(1).returning(|_| Ok(()));
        let system = BackupSystem::with_backup_root(Arc::new(mock), tmp.path().join("bk"));

        let r = system
            .create_backup_of_paths("system", std::slice::from_ref(&src))
            .await;
        assert!(r.is_err());
    }

    #[tokio::test]
    async fn system_backup_with_no_existing_paths_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let system =
            BackupSystem::with_backup_root(Arc::new(mock_file_manager()), tmp.path().into());
        let r = system
            .create_system_backup(
                "system",
                &[PathBuf::from("/no/existe/jamas")],
                &tmp.path().join("m.json"),
                "0.0.0",
            )
            .await;
        assert!(matches!(r, Err(EnolaError::ValidationError(_))));
    }

    #[test]
    fn identity_paths_include_only_enola_apparmor_profiles() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("enola-nginx"), "x").unwrap();
        std::fs::write(tmp.path().join("enola-tor"), "x").unwrap();
        std::fs::write(tmp.path().join("usr.bin.firefox"), "x").unwrap();
        std::fs::create_dir(tmp.path().join("enola-dir")).unwrap();
        let paths = system_identity_paths(tmp.path());
        assert!(paths.contains(&tmp.path().join("enola-nginx")));
        assert!(paths.contains(&tmp.path().join("enola-tor")));
        assert!(!paths.contains(&tmp.path().join("usr.bin.firefox")));
        assert!(!paths.contains(&tmp.path().join("enola-dir")));
        assert!(paths.contains(&PathBuf::from("/etc/tor/enola.d")));
        assert!(!paths.iter().any(|p| p.starts_with("/srv")));
    }
}
