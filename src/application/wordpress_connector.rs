use crate::domain::connector::ConnectorEntry;
use crate::domain::error::{EnolaError, Result};
use crate::ports::connector::{
    ContentConnectorPort, ExtractionResult, ExtractionStats, SourceInfo, SourceStatus,
};
use crate::ports::container::ContainerPort;
use crate::ports::file::FileManagerPort;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug, Serialize, Deserialize)]
pub struct WordPressPost {
    pub id: String,
    pub date: String,
    pub post_type: String,
    pub title: String,
    pub content: String,
}

pub struct WordPressConnector {
    container_manager: Arc<dyn ContainerPort + Send + Sync>,
    file_manager: Arc<dyn FileManagerPort + Send + Sync>,
    output_base_dir: PathBuf,
}

impl WordPressConnector {
    pub fn new(
        container_manager: Arc<dyn ContainerPort + Send + Sync>,
        file_manager: Arc<dyn FileManagerPort + Send + Sync>,
    ) -> Self {
        Self {
            container_manager,
            file_manager,
            output_base_dir: PathBuf::from("/opt/enola/ia/data/connectors/wordpress"),
        }
    }

    /// Check if the database container for a WordPress instance is running
    async fn is_container_running(&self, instance_name: &str) -> Result<bool> {
        let db_container = format!("db-{}", instance_name);
        let containers = self.container_manager.list_containers(false).await?;
        Ok(containers
            .iter()
            .any(|c| c.name == db_container || c.name.contains(&db_container)))
    }

    /// Get the database container name for an instance
    fn db_container_name(&self, instance_name: &str) -> String {
        format!("db-{}", instance_name)
    }

    /// Clean HTML tags from content
    pub fn clean_html(&self, html: &str) -> String {
        let mut result = String::new();
        let mut in_tag = false;

        for c in html.chars() {
            if c == '<' {
                in_tag = true;
            } else if c == '>' {
                in_tag = false;
                result.push(' '); // Add space after tag for word separation
            } else if !in_tag {
                result.push(c);
            }
        }

        // Cleanup whitespace
        result.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// Execute SQL query in the WordPress database container
    async fn execute_sql(&self, instance_name: &str, query: &str) -> Result<String> {
        let db_container = self.db_container_name(instance_name);

        let cmd = vec![
            "mariadb".to_string(),
            "-uwordpress".to_string(),
            "-ppassword".to_string(),
            "wordpress".to_string(),
            "-e".to_string(),
            query.to_string(),
            "-B".to_string(),
        ];

        self.container_manager
            .execute_command(&db_container, cmd)
            .await
    }

    /// Parse TSV output from database query into entries
    fn parse_tsv_to_entries(
        &self,
        output: &str,
        instance_name: &str,
    ) -> (Vec<ConnectorEntry>, ExtractionStats) {
        let mut entries = Vec::new();
        let mut stats = ExtractionStats::default();

        let mut lines = output.lines();
        // Skip header
        let _header = lines.next();

        for line in lines {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 5 {
                let id = parts[0].to_string();
                let date = parts[1].to_string();
                let title = parts[2].to_string();
                let content = self.clean_html(parts[3]);
                let post_type = parts[4].to_string();

                let entry = ConnectorEntry::new(
                    &id,
                    "wordpress",
                    &title,
                    &content,
                    &format!("wordpress://{}/{}", instance_name, id),
                )
                .with_metadata("post_type", &post_type)
                .with_metadata("original_date", &date)
                .with_metadata("instance", instance_name);

                entries.push(entry);

                match post_type.as_str() {
                    "post" => stats.posts_extracted += 1,
                    "page" => stats.pages_extracted += 1,
                    _ => {}
                }
            } else {
                stats.errors += 1;
            }
        }

        stats.total_entries = entries.len();
        (entries, stats)
    }

    /// Legacy method for backward compatibility
    pub async fn extract_content(&self, instance_name: &str) -> Result<PathBuf> {
        let result = self
            .extract_to_file(instance_name, &self.output_base_dir.join(instance_name))
            .await?;
        Ok(result)
    }
}

#[async_trait::async_trait]
impl ContentConnectorPort for WordPressConnector {
    async fn validate(&self, source_name: &str) -> Result<bool> {
        self.is_container_running(source_name).await
    }

    async fn info(&self, source_name: &str) -> Result<SourceInfo> {
        let is_running = self.is_container_running(source_name).await?;

        if !is_running {
            return Ok(SourceInfo {
                name: source_name.to_string(),
                source_type: "wordpress".to_string(),
                status: SourceStatus::Unavailable,
                entry_count: 0,
                last_sync: None,
            });
        }

        // Get post count
        let count_query = "SELECT COUNT(*) as count FROM wp_posts WHERE post_status='publish' AND post_type IN ('post', 'page');";
        let output = self.execute_sql(source_name, count_query).await?;

        let entry_count = output
            .lines()
            .nth(1)
            .and_then(|line| line.trim().parse::<usize>().ok())
            .unwrap_or(0);

        Ok(SourceInfo {
            name: source_name.to_string(),
            source_type: "wordpress".to_string(),
            status: SourceStatus::Available,
            entry_count,
            last_sync: None,
        })
    }

    async fn extract(&self, source_name: &str) -> Result<ExtractionResult> {
        // Validate container is running
        if !self.is_container_running(source_name).await? {
            return Err(EnolaError::NotFound(format!(
                "Database container {} not found or not running",
                self.db_container_name(source_name)
            )));
        }

        // Execute extraction query
        let sql_query = "SELECT ID, post_date, post_title, post_content, post_type FROM wp_posts WHERE post_status='publish' AND post_type IN ('post', 'page');";
        let output = self.execute_sql(source_name, sql_query).await?;

        // Parse results
        let (entries, stats) = self.parse_tsv_to_entries(&output, source_name);

        // Write to file
        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
        let instance_dir = self.output_base_dir.join(source_name);
        self.file_manager.ensure_dir(&instance_dir).await?;

        let output_file = instance_dir.join(format!("wp_content_{}.jsonl", timestamp));
        let mut jsonl_content = String::new();

        for entry in &entries {
            jsonl_content.push_str(&entry.to_jsonl());
            jsonl_content.push('\n');
        }

        self.file_manager
            .write_file(&output_file, &jsonl_content)
            .await?;

        Ok(ExtractionResult {
            entries,
            output_file: Some(output_file),
            stats,
        })
    }

    async fn extract_to_file(&self, source_name: &str, _output_path: &Path) -> Result<PathBuf> {
        let result = self.extract(source_name).await?;

        if let Some(file) = result.output_file {
            Ok(file)
        } else {
            Err(EnolaError::InfrastructureError(
                "Failed to create output file".to_string(),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::container::MockContainerPort;
    use crate::ports::file::MockFileManagerPort;
    use mockall::predicate::*;
    use std::sync::Arc;

    fn create_container_info(name: &str) -> crate::ports::container::ContainerInfo {
        crate::ports::container::ContainerInfo {
            id: "abc123".to_string(),
            name: name.to_string(),
            image: "mariadb:latest".to_string(),
            status: "Up 2 hours".to_string(),
            ports: vec![],
        }
    }

    #[tokio::test]
    async fn test_validate_returns_false_when_container_not_found() {
        let mut mock_container = MockContainerPort::new();
        let mock_file = MockFileManagerPort::new();

        mock_container
            .expect_list_containers()
            .with(eq(false))
            .times(1)
            .returning(|_| Ok(vec![]));

        let connector = WordPressConnector::new(Arc::new(mock_container), Arc::new(mock_file));

        let result = connector.validate("testblog").await;
        assert!(result.is_ok());
        assert!(
            !result.unwrap(),
            "Should return false when container not found"
        );
    }

    #[tokio::test]
    async fn test_validate_returns_true_when_container_running() {
        let mut mock_container = MockContainerPort::new();
        let mock_file = MockFileManagerPort::new();

        mock_container
            .expect_list_containers()
            .with(eq(false))
            .times(1)
            .returning(|_| Ok(vec![create_container_info("db-testblog")]));

        let connector = WordPressConnector::new(Arc::new(mock_container), Arc::new(mock_file));

        let result = connector.validate("testblog").await;
        assert!(result.is_ok());
        assert!(
            result.unwrap(),
            "Should return true when container is running"
        );
    }

    #[tokio::test]
    async fn test_info_returns_source_info_available() {
        let mut mock_container = MockContainerPort::new();
        let mock_file = MockFileManagerPort::new();

        mock_container
            .expect_list_containers()
            .with(eq(false))
            .times(1)
            .returning(|_| Ok(vec![create_container_info("db-testblog")]));

        mock_container
            .expect_execute_command()
            .times(1)
            .returning(|_, _| Ok("count\n5\n".to_string()));

        let connector = WordPressConnector::new(Arc::new(mock_container), Arc::new(mock_file));

        let result = connector.info("testblog").await;
        assert!(result.is_ok());

        let info = result.unwrap();
        assert_eq!(info.name, "testblog");
        assert_eq!(info.source_type, "wordpress");
        assert_eq!(info.status, SourceStatus::Available);
        assert_eq!(info.entry_count, 5);
    }

    #[tokio::test]
    async fn test_info_returns_unavailable_when_container_not_running() {
        let mut mock_container = MockContainerPort::new();
        let mock_file = MockFileManagerPort::new();

        mock_container
            .expect_list_containers()
            .with(eq(false))
            .times(1)
            .returning(|_| Ok(vec![]));

        let connector = WordPressConnector::new(Arc::new(mock_container), Arc::new(mock_file));

        let result = connector.info("testblog").await;
        assert!(result.is_ok());

        let info = result.unwrap();
        assert_eq!(info.status, SourceStatus::Unavailable);
        assert_eq!(info.entry_count, 0);
    }

    #[tokio::test]
    async fn test_extract_returns_connector_entries() {
        let mut mock_container = MockContainerPort::new();
        let mut mock_file = MockFileManagerPort::new();

        mock_container
            .expect_list_containers()
            .with(eq(false))
            .times(1)
            .returning(|_| Ok(vec![create_container_info("db-testblog")]));

        let tsv_output = "ID\tpost_date\tpost_title\tpost_content\tpost_type\n\
                          1\t2024-01-15 10:00:00\tFirst Post\t<p>Hello World</p>\tpost\n\
                          2\t2024-01-16 11:00:00\tAbout Page\t<h1>About Us</h1>\tpage\n";

        mock_container
            .expect_execute_command()
            .times(1)
            .returning(move |_, _| Ok(tsv_output.to_string()));

        mock_file.expect_ensure_dir().times(1).returning(|_| Ok(()));

        mock_file
            .expect_write_file()
            .times(1)
            .returning(|_, _| Ok(()));

        let connector = WordPressConnector::new(Arc::new(mock_container), Arc::new(mock_file));

        let result = connector.extract("testblog").await;
        assert!(result.is_ok());

        let extraction = result.unwrap();
        assert_eq!(extraction.entries.len(), 2);
        assert_eq!(extraction.stats.posts_extracted, 1);
        assert_eq!(extraction.stats.pages_extracted, 1);

        let first = &extraction.entries[0];
        assert_eq!(first.id, "1");
        assert_eq!(first.source, "wordpress");
        assert_eq!(first.title, "First Post");
        assert!(!first.content.contains("<p>"), "HTML should be cleaned");
    }

    #[tokio::test]
    async fn test_extract_handles_empty_posts() {
        let mut mock_container = MockContainerPort::new();
        let mut mock_file = MockFileManagerPort::new();

        mock_container
            .expect_list_containers()
            .with(eq(false))
            .times(1)
            .returning(|_| Ok(vec![create_container_info("db-testblog")]));

        mock_container
            .expect_execute_command()
            .times(1)
            .returning(|_, _| {
                Ok("ID\tpost_date\tpost_title\tpost_content\tpost_type\n".to_string())
            });

        mock_file.expect_ensure_dir().times(1).returning(|_| Ok(()));

        mock_file
            .expect_write_file()
            .times(1)
            .returning(|_, _| Ok(()));

        let connector = WordPressConnector::new(Arc::new(mock_container), Arc::new(mock_file));

        let result = connector.extract("testblog").await;
        assert!(result.is_ok());

        let extraction = result.unwrap();
        assert_eq!(extraction.entries.len(), 0);
        assert_eq!(extraction.stats.total_entries, 0);
    }

    #[tokio::test]
    async fn test_extract_fails_when_container_not_found() {
        let mut mock_container = MockContainerPort::new();
        let mock_file = MockFileManagerPort::new();

        mock_container
            .expect_list_containers()
            .with(eq(false))
            .times(1)
            .returning(|_| Ok(vec![]));

        let connector = WordPressConnector::new(Arc::new(mock_container), Arc::new(mock_file));

        let result = connector.extract("nonexistent").await;
        assert!(result.is_err());

        match result.unwrap_err() {
            EnolaError::NotFound(msg) => {
                assert!(
                    msg.contains("db-nonexistent"),
                    "Error should mention container name"
                );
            }
            _ => panic!("Expected NotFound error"),
        }
    }

    #[test]
    fn test_html_cleaning() {
        let mock_container = MockContainerPort::new();
        let mock_file = MockFileManagerPort::new();

        let connector = WordPressConnector::new(Arc::new(mock_container), Arc::new(mock_file));

        // Test various HTML patterns
        assert_eq!(
            connector.clean_html("<p>Simple paragraph</p>"),
            "Simple paragraph"
        );
        assert_eq!(
            connector.clean_html("<h1>Title</h1><p>Content</p>"),
            "Title Content"
        );
        assert_eq!(connector.clean_html("Plain text"), "Plain text");
        assert_eq!(
            connector.clean_html("<div class=\"test\">Nested <span>content</span></div>"),
            "Nested content"
        );
    }
}
