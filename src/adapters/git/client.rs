use crate::domain::error::{EnolaError, Result};
use crate::ports::git::GitPort;
use std::path::Path;
use tokio::fs;
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

    // ... existing flatten_repo ... unfortunately trait doesn't have it yet,
    // maybe we should move flatten_repo to trait or keep as helper?
    // User task "GitCodeFlattening Conector ... #domain" implies it's a capability.

    // ... helper methods ...
    fn is_text_file(&self, ext: &str) -> bool {
        matches!(
            ext,
            "rs" | "toml"
                | "md"
                | "txt"
                | "json"
                | "js"
                | "ts"
                | "py"
                | "sh"
                | "html"
                | "css"
                | "yml"
                | "yaml"
        )
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

impl GitClientAdapter {
    /// Flattens a repository into a single text file (context window optimized)
    pub async fn flatten_repo(&self, repo_dir: &Path) -> Result<String> {
        // Walkdir
        let mut content = String::new();

        // Recursive walker needed.
        // Since we are in async adapter, and recursive async fns need BoxFuture, let's use a stack or `walkdir` crate if added (not in cargo.toml).
        // I'll implement simple recursive walker helper.

        self.visit_dir(repo_dir, &mut content).await?;

        Ok(content)
    }

    #[async_recursion::async_recursion]
    // Wait, async_recursion crate not in Cargo.toml.
    // I will use Iterative Stack approach or just 1 level deep for MVP?
    // Git repos are deep.
    // `tokio::fs::read_dir` returns `ReadDir`.

    // I'll modify logic to use `find` command if available for speed and simplicity in Linux?
    // Or just simple recursion without `async_recursion` macro using Box::pin.

    // Using `find` command is actually robust for "Enola Server" (Linux env).
    // `find . -maxdepth 5 -not -path '*/.*' -type f`

    // Let's stick to safe Rust recursion.
    // Since I can't add crate easily without editing cargo.toml and network, I'll use a `Vec` stack.

    async fn visit_dir(&self, dir: &Path, content: &mut String) -> Result<()> {
        // This function would be better iterative for async without recursion trait.
        // But let's try `find` command approach for "flattening" to avoid re-implementing walkdir.

        // Actually, let's just implement `SyncSource` assuming `GitCodeFlattening` allows `process_repo`.
        // The task calls for "Clonar repo y generar contexto plano".

        // Let's implement iterative walk.
        let mut stack = vec![dir.to_path_buf()];

        while let Some(path) = stack.pop() {
            let mut entries = match fs::read_dir(&path).await {
                Ok(e) => e,
                Err(_) => continue, // Permission denied etc
            };

            while let Ok(Some(entry)) = entries.next_entry().await {
                let entry_path = entry.path();
                let file_name = entry.file_name().to_string_lossy().to_string();

                // Ignore .git, target, node_modules
                if file_name.starts_with('.')
                    || file_name == "target"
                    || file_name == "node_modules"
                {
                    continue;
                }

                if entry_path.is_dir() {
                    stack.push(entry_path);
                } else {
                    // Check extension
                    let ext = entry_path
                        .extension()
                        .and_then(|s| s.to_str())
                        .unwrap_or("");
                    if self.is_text_file(ext) {
                        content.push_str(&format!(
                            "\n\n--- FILE: {} ---\n",
                            entry_path.to_string_lossy()
                        ));
                        if let Ok(text) = fs::read_to_string(&entry_path).await {
                            content.push_str(&text);
                        }
                    }
                }
            }
        }
        Ok(())
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

    #[test]
    fn test_is_text_file() {
        let adapter = GitClientAdapter::new();
        assert!(adapter.is_text_file("rs"));
        assert!(adapter.is_text_file("toml"));
        assert!(adapter.is_text_file("md"));
        assert!(adapter.is_text_file("py"));
        assert!(adapter.is_text_file("json"));
        assert!(!adapter.is_text_file("exe"));
        assert!(!adapter.is_text_file("bin"));
        assert!(!adapter.is_text_file("png"));
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

    #[tokio::test]
    async fn test_flatten_repo_empty_dir() {
        let dir = TempDir::new().unwrap();
        let adapter = GitClientAdapter::new();
        let content = adapter.flatten_repo(dir.path()).await.unwrap();
        assert!(content.is_empty());
    }

    #[tokio::test]
    async fn test_flatten_repo_with_files() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("main.rs"), "fn main() {}").unwrap();
        std::fs::write(dir.path().join("README.md"), "# Hello").unwrap();
        std::fs::write(dir.path().join("photo.png"), "binary").unwrap();
        let adapter = GitClientAdapter::new();
        let content = adapter.flatten_repo(dir.path()).await.unwrap();
        assert!(content.contains("fn main()"));
        assert!(content.contains("# Hello"));
        assert!(!content.contains("binary")); // png should be excluded
    }

    #[tokio::test]
    async fn test_flatten_repo_skips_dotdirs() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".git")).unwrap();
        std::fs::write(dir.path().join(".git/config"), "secret").unwrap();
        std::fs::write(dir.path().join("lib.rs"), "pub mod lib;").unwrap();
        let adapter = GitClientAdapter::new();
        let content = adapter.flatten_repo(dir.path()).await.unwrap();
        assert!(!content.contains("secret"));
        assert!(content.contains("pub mod lib"));
    }
}
