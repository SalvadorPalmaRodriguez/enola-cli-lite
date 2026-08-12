// MED-01: Prevent unwrap/expect in non-test code — panics in FS operations
// with privileges can leave files/dirs in inconsistent state.
#![warn(clippy::unwrap_used, clippy::expect_used)]
use crate::domain::error::{EnolaError, Result};
use crate::ports::file::FileManagerPort;
use std::collections::HashMap;
use std::path::Path;
use tokio::fs;

pub struct EnolaFileAdapter;

impl Default for EnolaFileAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl EnolaFileAdapter {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl FileManagerPort for EnolaFileAdapter {
    async fn read_file(&self, path: &Path) -> Result<String> {
        fs::read_to_string(path).await.map_err(|e| {
            EnolaError::InfrastructureError(format!("Failed to read file {:?}: {}", path, e))
        })
    }

    async fn write_file(&self, path: &Path, content: &str) -> Result<()> {
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent).await.map_err(|e| {
                    EnolaError::InfrastructureError(format!(
                        "Failed to create dir {:?}: {}",
                        parent, e
                    ))
                })?;
            }
        }

        fs::write(path, content).await.map_err(|e| {
            EnolaError::InfrastructureError(format!("Failed to write file {:?}: {}", path, e))
        })
    }

    async fn ensure_dir(&self, path: &Path) -> Result<()> {
        if !path.exists() {
            fs::create_dir_all(path).await.map_err(|e| {
                EnolaError::InfrastructureError(format!("Failed to create dir {:?}: {}", path, e))
            })?;
        }
        Ok(())
    }

    async fn read_env(&self, path: &Path) -> Result<HashMap<String, String>> {
        if !path.exists() {
            return Ok(HashMap::new());
        }

        let content = self.read_file(path).await.unwrap_or_default();

        let mut map = HashMap::new();
        for line in content.lines() {
            if let Some((k, v)) = line.split_once('=') {
                let key = k.trim().to_string();
                let val = v.trim().trim_matches('"').to_string();
                if !key.starts_with('#') && !key.is_empty() {
                    map.insert(key, val);
                }
            }
        }
        Ok(map)
    }

    async fn update_env_key(&self, path: &Path, key: &str, value: &str) -> Result<()> {
        let content = if path.exists() {
            self.read_file(path).await?
        } else {
            String::new()
        };

        let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
        let mut found = false;

        for line in &mut lines {
            if line.starts_with(key) && line.contains('=') {
                if let Some((k, _)) = line.split_once('=') {
                    if k.trim() == key {
                        *line = format!("{}={}", key, value);
                        found = true;
                        break;
                    }
                }
            }
        }

        if !found {
            lines.push(format!("{}={}", key, value));
        }

        let new_content = lines.join("\n") + "\n";
        self.write_file(path, &new_content).await
    }

    async fn delete_file(&self, path: &Path) -> Result<()> {
        if path.exists() {
            fs::remove_file(path).await.map_err(|e| {
                EnolaError::InfrastructureError(format!("Failed to delete file {:?}: {}", path, e))
            })?;
        }
        Ok(())
    }

    async fn copy_file(&self, from: &Path, to: &Path) -> Result<()> {
        if let Some(parent) = to.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent).await.map_err(|e| {
                    EnolaError::InfrastructureError(format!(
                        "Failed to create dir {:?}: {}",
                        parent, e
                    ))
                })?;
            }
        }

        fs::copy(from, to).await.map(|_| ()).map_err(|e| {
            EnolaError::InfrastructureError(format!(
                "Failed to copy file from {:?} to {:?}: {}",
                from, to, e
            ))
        })
    }

    async fn set_ownership(&self, path: &Path, user: &str, group: &str) -> Result<()> {
        let arg = format!("{}:{}", user, group);
        let status = tokio::process::Command::new("chown")
            .arg(&arg)
            .arg(path)
            .status()
            .await
            .map_err(|e| EnolaError::InfrastructureError(format!("Failed to exec chown: {}", e)))?;

        if status.success() {
            Ok(())
        } else {
            Err(EnolaError::InfrastructureError(format!(
                "chown failed for {:?}",
                path
            )))
        }
    }

    async fn set_permissions(&self, path: &Path, mode: u32) -> Result<()> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(mode);
            fs::set_permissions(path, perms).await.map_err(|e| {
                EnolaError::InfrastructureError(format!("Failed to chmod {:?}: {}", path, e))
            })
        }
        #[cfg(not(unix))]
        {
            Ok(())
        }
    }

    async fn create_archive(&self, source_dir: &Path, dest_file: &Path) -> Result<()> {
        let status = tokio::process::Command::new("tar")
            .args([
                "-czf",
                &dest_file.to_string_lossy(),
                "-C",
                &source_dir.parent().unwrap_or(source_dir).to_string_lossy(),
                &source_dir
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| ".".to_string()),
            ])
            .status()
            .await
            .map_err(|e| EnolaError::InfrastructureError(format!("tar create failed: {}", e)))?;

        if status.success() {
            Ok(())
        } else {
            Err(EnolaError::InfrastructureError(format!(
                "tar -czf failed for {:?}",
                dest_file
            )))
        }
    }

    async fn extract_archive(&self, archive: &Path, dest_dir: &Path) -> Result<()> {
        fs::create_dir_all(dest_dir).await.map_err(|e| {
            EnolaError::InfrastructureError(format!("Cannot create dest dir: {}", e))
        })?;

        let status = tokio::process::Command::new("tar")
            .args([
                "-xzf",
                &archive.to_string_lossy(),
                "-C",
                &dest_dir.to_string_lossy(),
            ])
            .status()
            .await
            .map_err(|e| EnolaError::InfrastructureError(format!("tar extract failed: {}", e)))?;

        if status.success() {
            Ok(())
        } else {
            Err(EnolaError::InfrastructureError(format!(
                "tar -xzf failed for {:?}",
                archive
            )))
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::ports::file::FileManagerPort;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_write_and_read_file() {
        let dir = TempDir::new().unwrap();
        let adapter = EnolaFileAdapter::new();
        let path = dir.path().join("test.txt");
        adapter.write_file(&path, "hello world").await.unwrap();
        let content = adapter.read_file(&path).await.unwrap();
        assert_eq!(content, "hello world");
    }

    #[tokio::test]
    async fn test_write_creates_parent_dirs() {
        let dir = TempDir::new().unwrap();
        let adapter = EnolaFileAdapter::new();
        let path = dir.path().join("a/b/c/file.txt");
        adapter.write_file(&path, "nested").await.unwrap();
        assert!(path.exists());
    }

    #[tokio::test]
    async fn test_read_file_not_found() {
        let adapter = EnolaFileAdapter::new();
        let result = adapter
            .read_file(std::path::Path::new("/nonexistent/file.txt"))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_ensure_dir() {
        let dir = TempDir::new().unwrap();
        let adapter = EnolaFileAdapter::new();
        let new_dir = dir.path().join("new_dir");
        adapter.ensure_dir(&new_dir).await.unwrap();
        assert!(new_dir.exists());
    }

    #[tokio::test]
    async fn test_ensure_dir_already_exists() {
        let dir = TempDir::new().unwrap();
        let adapter = EnolaFileAdapter::new();
        adapter.ensure_dir(dir.path()).await.unwrap();
    }

    #[tokio::test]
    async fn test_read_env() {
        let dir = TempDir::new().unwrap();
        let adapter = EnolaFileAdapter::new();
        let path = dir.path().join(".env");
        adapter
            .write_file(
                &path,
                "KEY1=value1\nKEY2=\"value2\"\n# comment\n\nKEY3=value3",
            )
            .await
            .unwrap();
        let env = adapter.read_env(&path).await.unwrap();
        assert_eq!(env.get("KEY1").unwrap(), "value1");
        assert_eq!(env.get("KEY2").unwrap(), "value2");
        assert_eq!(env.get("KEY3").unwrap(), "value3");
        assert!(!env.contains_key("# comment"));
    }

    #[tokio::test]
    async fn test_read_env_nonexistent() {
        let adapter = EnolaFileAdapter::new();
        let env = adapter
            .read_env(std::path::Path::new("/nonexistent/.env"))
            .await
            .unwrap();
        assert!(env.is_empty());
    }

    #[tokio::test]
    async fn test_update_env_key_new() {
        let dir = TempDir::new().unwrap();
        let adapter = EnolaFileAdapter::new();
        let path = dir.path().join(".env");
        adapter.update_env_key(&path, "PORT", "8080").await.unwrap();
        let env = adapter.read_env(&path).await.unwrap();
        assert_eq!(env.get("PORT").unwrap(), "8080");
    }

    #[tokio::test]
    async fn test_update_env_key_existing() {
        let dir = TempDir::new().unwrap();
        let adapter = EnolaFileAdapter::new();
        let path = dir.path().join(".env");
        adapter
            .write_file(&path, "PORT=3000\nHOST=localhost\n")
            .await
            .unwrap();
        adapter.update_env_key(&path, "PORT", "8080").await.unwrap();
        let env = adapter.read_env(&path).await.unwrap();
        assert_eq!(env.get("PORT").unwrap(), "8080");
        assert_eq!(env.get("HOST").unwrap(), "localhost");
    }

    #[tokio::test]
    async fn test_delete_file() {
        let dir = TempDir::new().unwrap();
        let adapter = EnolaFileAdapter::new();
        let path = dir.path().join("deleteme.txt");
        adapter.write_file(&path, "temp").await.unwrap();
        assert!(path.exists());
        adapter.delete_file(&path).await.unwrap();
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn test_delete_file_nonexistent_is_ok() {
        let adapter = EnolaFileAdapter::new();
        let result = adapter
            .delete_file(std::path::Path::new("/tmp/enola_test_nonexist_42"))
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_copy_file() {
        let dir = TempDir::new().unwrap();
        let adapter = EnolaFileAdapter::new();
        let src = dir.path().join("source.txt");
        let dst = dir.path().join("dest.txt");
        adapter.write_file(&src, "copy me").await.unwrap();
        adapter.copy_file(&src, &dst).await.unwrap();
        let content = adapter.read_file(&dst).await.unwrap();
        assert_eq!(content, "copy me");
    }

    #[tokio::test]
    async fn test_default_constructor() {
        let adapter = EnolaFileAdapter::default();
        let _ = adapter;
    }
}
