use crate::domain::error::{EnolaError, Result};
use crate::ports::git::GitPort;
use std::path::Path;
use tokio::process::Command;

pub struct GitClientAdapter;

impl Default for GitClientAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl GitClientAdapter {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl GitPort for GitClientAdapter {
    async fn clone_repo(&self, repo_url: &str, target_dir: &Path) -> Result<()> {
        if target_dir.exists() {
            // If exists, try pull
            let status = Command::new("git")
                .arg("-C")
                .arg(target_dir)
                .arg("pull")
                .status()
                .await
                .map_err(|e| EnolaError::InfrastructureError(format!("Git pull failed: {}", e)))?;

            if !status.success() {
                return Err(EnolaError::InfrastructureError(
                    "Git pull returned non-zero".to_string(),
                ));
            }
        } else {
            // Clone
            let status = Command::new("git")
                .arg("clone")
                .arg("--depth")
                .arg("1") // Shallow clone for speed
                .arg(repo_url)
                .arg(target_dir)
                .status()
                .await
                .map_err(|e| EnolaError::InfrastructureError(format!("Git clone failed: {}", e)))?;

            if !status.success() {
                return Err(EnolaError::InfrastructureError(
                    "Git clone returned non-zero".to_string(),
                ));
            }
        }
        Ok(())
    }

    async fn is_repo(&self, dir: &Path) -> Result<bool> {
        Ok(dir.join(".git").exists())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::git::GitPort;
    use tempfile::TempDir;

    #[test]
    fn test_default_constructor() {
        let adapter = GitClientAdapter::default();
        let _ = adapter;
    }

    #[tokio::test]
    async fn test_is_repo_with_git_dir() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".git")).unwrap();
        let adapter = GitClientAdapter::new();
        assert!(adapter.is_repo(dir.path()).await.unwrap());
    }

    #[tokio::test]
    async fn test_is_repo_without_git_dir() {
        let dir = TempDir::new().unwrap();
        let adapter = GitClientAdapter::new();
        assert!(!adapter.is_repo(dir.path()).await.unwrap());
    }
}
