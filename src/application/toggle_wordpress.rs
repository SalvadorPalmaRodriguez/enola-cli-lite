use crate::domain::error::Result;
use crate::ports::container::ContainerPort;
use std::sync::Arc;

pub struct ToggleWordPress {
    docker_adapter: Arc<dyn ContainerPort + Send + Sync>,
}

pub enum ToggleAction {
    Start,
    Stop,
    Remove,
}

impl ToggleWordPress {
    pub fn new(docker_adapter: Arc<dyn ContainerPort + Send + Sync>) -> Self {
        Self { docker_adapter }
    }

    pub async fn execute(&self, service_name: &str, action: ToggleAction) -> Result<()> {
        // Enola naming convention for WordPress containers: "wp-{service_name}" and "db-{service_name}"
        let wp_container_name = format!("wp-{}", service_name);
        let db_container_name = format!("db-{}", service_name);

        match action {
            ToggleAction::Start => {
                // Ensure db is started first
                self.docker_adapter
                    .start_container(&db_container_name)
                    .await?;
                self.docker_adapter
                    .start_container(&wp_container_name)
                    .await?;

                // Enable tor service if hidden service exists?
                // Tor service management is usually separate (StopTorService), but maybe linked here?
                // Usually "ToggleWordPress" implies the WP instance itself.
                // However, if we stop the container, the hidden service will just point to nothing.
            }
            ToggleAction::Stop => {
                self.docker_adapter
                    .stop_container(&wp_container_name)
                    .await?;
                self.docker_adapter
                    .stop_container(&db_container_name)
                    .await?;
            }
            ToggleAction::Remove => {
                // Stop and Remove
                self.docker_adapter
                    .stop_container(&wp_container_name)
                    .await
                    .ok();
                self.docker_adapter
                    .remove_container(&wp_container_name)
                    .await?;

                self.docker_adapter
                    .stop_container(&db_container_name)
                    .await
                    .ok();
                self.docker_adapter
                    .remove_container(&db_container_name)
                    .await?;

                // Also remove Tor Service config?
                // The prompt says "Enable/Disable/Remove".
                // If we remove the WP instance, we should likely check if we should remove the Tor service too.
                // But usually that's handled by `RemoveTorService`.
                // Let's assume this UseCase handles the underlying compute resources (Docker).
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::container::MockContainerPort;
    use mockall::predicate::*;

    #[tokio::test]
    async fn test_toggle_wordpress_start() {
        let mut mock_docker = MockContainerPort::new();

        mock_docker
            .expect_start_container()
            .with(eq("db-testblog"))
            .times(1)
            .returning(|_| Ok(()));

        mock_docker
            .expect_start_container()
            .with(eq("wp-testblog"))
            .times(1)
            .returning(|_| Ok(()));

        let use_case = ToggleWordPress::new(Arc::new(mock_docker));
        let result = use_case.execute("testblog", ToggleAction::Start).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_toggle_wordpress_stop() {
        let mut mock_docker = MockContainerPort::new();

        mock_docker
            .expect_stop_container()
            .with(eq("wp-testblog")) // Stop WP first
            .times(1)
            .returning(|_| Ok(()));

        mock_docker
            .expect_stop_container()
            .with(eq("db-testblog"))
            .times(1)
            .returning(|_| Ok(()));

        let use_case = ToggleWordPress::new(Arc::new(mock_docker));
        let result = use_case.execute("testblog", ToggleAction::Stop).await;

        assert!(result.is_ok());
    }
}
