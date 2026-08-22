use crate::application::backup_system::BackupSystem;
use crate::application::wordpress_status_check::WordPressStatusCheck;
use crate::domain::error::{EnolaError, Result};
use crate::ports::container::ContainerPort;
use std::path::PathBuf;
use std::sync::Arc;

pub struct SecureWordPressUpdate {
    container_manager: Arc<dyn ContainerPort + Send + Sync>,
    backup_system: Arc<BackupSystem>,
    status_check: Arc<WordPressStatusCheck>,
}

/// Directories used by a WordPress instance, relative to /srv/enola-wordpress/
const WP_BASE_DIR: &str = "/srv/enola-wordpress";

impl SecureWordPressUpdate {
    pub fn new(
        container_manager: Arc<dyn ContainerPort + Send + Sync>,
        backup_system: Arc<BackupSystem>,
        status_check: Arc<WordPressStatusCheck>,
    ) -> Self {
        Self {
            container_manager,
            backup_system,
            status_check,
        }
    }

    pub async fn execute(&self, blog_name: &str) -> Result<()> {
        tracing::info!(
            "Starting secure update for WordPress instance: {}",
            blog_name
        );

        // 1. Pre-update Backup: archive _db and _wp directories
        let base_dir = PathBuf::from(WP_BASE_DIR);
        let db_dir = base_dir.join(format!("{}_db", blog_name));
        let wp_dir = base_dir.join(format!("{}_wp", blog_name));

        let backup_db_path = if db_dir.exists() {
            Some(
                self.backup_system
                    .create_archive_backup(&db_dir, &format!("pre-update-{}-db", blog_name))
                    .await?,
            )
        } else {
            None
        };

        let backup_wp_path = if wp_dir.exists() {
            Some(
                self.backup_system
                    .create_archive_backup(&wp_dir, &format!("pre-update-{}-wp", blog_name))
                    .await?,
            )
        } else {
            None
        };

        // 2. Pull latest images
        self.container_manager.pull_image("mariadb:10.6").await?;
        self.container_manager
            .pull_image("wordpress:latest")
            .await?;

        // 3. Restart Containers
        let db_container = format!("db-{}", blog_name);
        let wp_container = format!("wp-{}", blog_name);

        self.container_manager
            .restart_container(&db_container)
            .await?;
        self.container_manager
            .restart_container(&wp_container)
            .await?;

        // 4. Health Check
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        let status = self.status_check.execute(blog_name).await?;

        if !status.is_healthy {
            tracing::error!(
                "Update health check failed for {}. Rolling back...",
                blog_name
            );
            self.rollback(
                blog_name,
                backup_db_path.as_deref(),
                backup_wp_path.as_deref(),
            )
            .await?;
            return Err(EnolaError::InfrastructureError(
                "Update failed health check, rolled back".to_string(),
            ));
        }

        tracing::info!("Secure update completed successfully for {}", blog_name);
        Ok(())
    }

    async fn rollback(
        &self,
        blog_name: &str,
        backup_db_path: Option<&std::path::Path>,
        backup_wp_path: Option<&std::path::Path>,
    ) -> Result<()> {
        tracing::warn!("Rollback initiated for {}", blog_name);

        let base_dir = PathBuf::from(WP_BASE_DIR);
        let db_container = format!("db-{}", blog_name);
        let wp_container = format!("wp-{}", blog_name);
        let db_dir = base_dir.join(format!("{}_db", blog_name));
        let wp_dir = base_dir.join(format!("{}_wp", blog_name));

        // 1. Stop containers before restoring data
        let _ = self.container_manager.stop_container(&wp_container).await;
        let _ = self.container_manager.stop_container(&db_container).await;

        // 2. Restore data from backups
        if let Some(db_backup) = backup_db_path {
            if db_backup.exists() {
                tracing::info!("Restoring DB data from {:?}", db_backup);
                self.backup_system
                    .restore_directory(db_backup, &db_dir)
                    .await?;
            }
        }

        if let Some(wp_backup) = backup_wp_path {
            if wp_backup.exists() {
                tracing::info!("Restoring WP data from {:?}", wp_backup);
                self.backup_system
                    .restore_directory(wp_backup, &wp_dir)
                    .await?;
            }
        }

        // 3. Restart containers
        self.container_manager
            .start_container(&db_container)
            .await?;
        self.container_manager
            .start_container(&wp_container)
            .await?;

        tracing::info!("Rollback completed for {}", blog_name);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::container::MockContainerPort;
    use crate::ports::file::MockFileManagerPort;

    #[test]
    fn test_struct_creation() {
        let mock_main = MockContainerPort::new();
        let mock_status = MockContainerPort::new();

        let mut mock_file = MockFileManagerPort::new();
        mock_file.expect_ensure_dir().returning(|_| Ok(()));

        let _svc = SecureWordPressUpdate::new(
            Arc::new(mock_main),
            Arc::new(BackupSystem::new(Arc::new(mock_file))),
            Arc::new(WordPressStatusCheck::new(Arc::new(mock_status))),
        );
    }

    #[tokio::test]
    async fn test_execute_skips_backup_if_no_data_dir() {
        let mut mock_main = MockContainerPort::new();
        mock_main.expect_pull_image().returning(|_| Ok(()));
        mock_main.expect_restart_container().returning(|_| Ok(()));
        // Rollback expectations: stop + start (rollback triggered by failed health check)
        mock_main.expect_stop_container().returning(|_| Ok(()));
        mock_main.expect_start_container().returning(|_| Ok(()));

        let mut mock_status = MockContainerPort::new();
        mock_status
            .expect_list_containers()
            .returning(|_| Ok(vec![]));

        let mut mock_file = MockFileManagerPort::new();
        mock_file.expect_ensure_dir().returning(|_| Ok(()));

        let svc = SecureWordPressUpdate::new(
            Arc::new(mock_main),
            Arc::new(BackupSystem::new(Arc::new(mock_file))),
            Arc::new(WordPressStatusCheck::new(Arc::new(mock_status))),
        );

        // /srv/enola-wordpress/nonexistent_db and _wp don't exist → skips backup
        // Status check returns unhealthy (no containers) → triggers rollback → returns error
        let result = svc.execute("nonexistent").await;
        assert!(result.is_err());
    }
}
