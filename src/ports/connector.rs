use crate::domain::connector::ConnectorEntry;
use crate::domain::error::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Information about a content source
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceInfo {
    pub name: String,
    pub source_type: String, // "wordpress", "git", "web"
    pub status: SourceStatus,
    pub entry_count: usize,
    pub last_sync: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SourceStatus {
    Available,
    Unavailable,
    Error(String),
}

/// Result of content extraction
#[derive(Debug, Clone)]
pub struct ExtractionResult {
    pub entries: Vec<ConnectorEntry>,
    pub output_file: Option<PathBuf>,
    pub stats: ExtractionStats,
}

#[derive(Debug, Clone, Default)]
pub struct ExtractionStats {
    pub total_entries: usize,
    pub posts_extracted: usize,
    pub pages_extracted: usize,
    pub errors: usize,
}

/// Port for content connectors (WordPress, Git, Web)
/// Follows the pattern of connector_wordpress.sh with validate/info/extract actions
#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait ContentConnectorPort: Send + Sync {
    /// Validate that the source is accessible and properly configured
    async fn validate(&self, source_name: &str) -> Result<bool>;

    /// Get information about the content source
    async fn info(&self, source_name: &str) -> Result<SourceInfo>;

    /// Extract content from the source and return structured entries
    async fn extract(&self, source_name: &str) -> Result<ExtractionResult>;

    /// Extract content and write directly to JSONL file
    async fn extract_to_file(&self, source_name: &str, output_path: &Path) -> Result<PathBuf>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_connector_validate() {
        let mut mock = MockContentConnectorPort::new();
        mock.expect_validate().returning(|_| Ok(true));
        assert!(mock.validate("wp_blog").await.unwrap());
    }

    #[tokio::test]
    async fn test_mock_connector_info() {
        let mut mock = MockContentConnectorPort::new();
        mock.expect_info().returning(|_| {
            Ok(SourceInfo {
                name: "blog".into(),
                source_type: "wordpress".into(),
                status: SourceStatus::Available,
                entry_count: 10,
                last_sync: Some("2026-03-03".into()),
            })
        });
        let info = mock.info("blog").await.unwrap();
        assert_eq!(info.entry_count, 10);
        assert_eq!(info.status, SourceStatus::Available);
    }

    #[test]
    fn test_source_status_eq() {
        assert_eq!(SourceStatus::Available, SourceStatus::Available);
        assert_ne!(SourceStatus::Available, SourceStatus::Unavailable);
    }

    #[test]
    fn test_extraction_stats_default() {
        let stats = ExtractionStats::default();
        assert_eq!(stats.total_entries, 0);
        assert_eq!(stats.errors, 0);
    }
}
