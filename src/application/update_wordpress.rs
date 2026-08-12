use crate::domain::error::Result;
use crate::ports::container::ContainerPort;
use std::sync::Arc;

pub struct UpdateWordPress {
    container_manager: Arc<dyn ContainerPort + Send + Sync>,
}

impl UpdateWordPress {
    pub fn new(container_manager: Arc<dyn ContainerPort + Send + Sync>) -> Self {
        Self { container_manager }
    }

    pub async fn execute(&self, blog_name: &str) -> Result<()> {
        let db_container = format!("db-{}", blog_name);
        let wp_container = format!("wp-{}", blog_name);

        // 1. Pull latest images (optional, depends if we want to force update)
        // ContainerPort should have a pull_image method for this.
        // Assuming we want to update the WordPress image.

        // 2. Restart Containers
        self.container_manager
            .restart_container(&db_container)
            .await?;
        self.container_manager
            .restart_container(&wp_container)
            .await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::container::MockContainerPort;

    #[tokio::test]
    async fn test_update_restarts_both_containers() {
        let mut mock = MockContainerPort::new();
        mock.expect_restart_container()
            .withf(|id| id == "db-myblog")
            .times(1)
            .returning(|_| Ok(()));
        mock.expect_restart_container()
            .withf(|id| id == "wp-myblog")
            .times(1)
            .returning(|_| Ok(()));

        let service = UpdateWordPress::new(Arc::new(mock));
        let result = service.execute("myblog").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_update_fails_if_db_restart_fails() {
        let mut mock = MockContainerPort::new();
        mock.expect_restart_container()
            .withf(|id| id == "db-myblog")
            .returning(|_| {
                Err(crate::domain::error::EnolaError::InfrastructureError(
                    "Docker not running".to_string(),
                ))
            });

        let service = UpdateWordPress::new(Arc::new(mock));
        let result = service.execute("myblog").await;
        assert!(result.is_err());
    }
}
