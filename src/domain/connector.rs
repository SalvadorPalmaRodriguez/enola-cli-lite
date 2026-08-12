use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectorEntry {
    pub id: String,
    pub source: String, // e.g. "wordpress", "git"
    pub title: String,
    pub content: String,
    pub created_at: DateTime<Utc>,
    pub url: String,
    #[serde(default)]
    pub metadata: HashMap<String, String>,
}

impl ConnectorEntry {
    pub fn new(id: &str, source: &str, title: &str, content: &str, url: &str) -> Self {
        Self {
            id: id.to_string(),
            source: source.to_string(),
            title: title.to_string(),
            content: content.to_string(),
            created_at: Utc::now(),
            url: url.to_string(),
            metadata: HashMap::new(),
        }
    }

    pub fn with_metadata(mut self, key: &str, value: &str) -> Self {
        self.metadata.insert(key.to_string(), value.to_string());
        self
    }

    pub fn to_jsonl(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }
}

pub struct ConnectorSharedUtils;

impl ConnectorSharedUtils {
    pub fn write_jsonl_entry<T: serde::Serialize>(
        target_file: &std::path::Path,
        entry: &T,
    ) -> crate::domain::error::Result<()> {
        let json = serde_json::to_string(entry)
            .map_err(|e| crate::domain::error::EnolaError::ValidationError(e.to_string()))?;

        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(target_file)
            .map_err(|e| crate::domain::error::EnolaError::InfrastructureError(e.to_string()))?;

        writeln!(file, "{}", json)
            .map_err(|e| crate::domain::error::EnolaError::InfrastructureError(e.to_string()))?;

        Ok(())
    }
}
