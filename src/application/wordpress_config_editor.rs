use crate::application::edit_port_config::EditPortConfig;
use crate::application::edit_wordpress_config::EditWordPressConfig;
use crate::domain::error::Result;
use std::sync::Arc;

pub struct WordPressConfigEditor {
    port_editor: Arc<EditPortConfig>,
    wp_setting_editor: Arc<EditWordPressConfig>,
}

impl WordPressConfigEditor {
    pub fn new(
        port_editor: Arc<EditPortConfig>,
        wp_setting_editor: Arc<EditWordPressConfig>,
    ) -> Self {
        Self {
            port_editor,
            wp_setting_editor,
        }
    }

    pub async fn update_port(
        &self,
        service_name: &str,
        onion_port: u16,
        nginx_listen_port: u16,
        backend_port: u16,
    ) -> Result<()> {
        self.port_editor
            .execute(service_name, onion_port, nginx_listen_port, backend_port)
            .await
    }

    pub async fn update_wp_setting(&self, blog_name: &str, key: &str, value: &str) -> Result<()> {
        self.wp_setting_editor.execute(blog_name, key, value).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::file::MockFileManagerPort;
    use crate::ports::tor::MockTorManagerPort;

    fn make_port_editor() -> Arc<EditPortConfig> {
        let mut tor = MockTorManagerPort::new();
        tor.expect_deploy_hidden_service()
            .returning(|_, _| Ok("test.onion".to_string()));
        tor.expect_reload_tor().returning(|| Ok(()));
        Arc::new(EditPortConfig::new(Arc::new(tor), None))
    }

    fn make_wp_editor() -> Arc<EditWordPressConfig> {
        let mock = MockFileManagerPort::new();
        Arc::new(EditWordPressConfig::new(Arc::new(mock)))
    }

    #[test]
    fn test_wordpress_config_editor_creation() {
        let editor = WordPressConfigEditor::new(make_port_editor(), make_wp_editor());
        // Verify struct is created correctly
        let _ = editor;
    }

    #[tokio::test]
    async fn test_update_port_delegates_to_port_editor() {
        let editor = WordPressConfigEditor::new(make_port_editor(), make_wp_editor());
        let result = editor.update_port("test_svc", 80, 8080, 3000).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_update_wp_setting_returns_not_found() {
        let editor = WordPressConfigEditor::new(make_port_editor(), make_wp_editor());
        // Path doesn't exist so returns NotFound
        let result = editor
            .update_wp_setting("nonexistent", "DB_NAME", "test")
            .await;
        assert!(result.is_err());
    }
}
