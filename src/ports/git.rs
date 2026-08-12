use crate::domain::error::Result;
use std::path::Path;

#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait GitPort {
    /// Clone a repository to a target directory
    async fn clone_repo(&self, repo_url: &str, target_dir: &Path) -> Result<()>;

    /// Check if a directory is a git repo
    async fn is_repo(&self, dir: &Path) -> Result<bool>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[tokio::test]
    async fn test_mock_git_clone() {
        let mut mock = MockGitPort::new();
        mock.expect_clone_repo().returning(|_, _| Ok(()));
        assert!(mock
            .clone_repo("http://git.local/repo", Path::new("/tmp/repo"))
            .await
            .is_ok());
    }

    #[tokio::test]
    async fn test_mock_git_is_repo() {
        let mut mock = MockGitPort::new();
        mock.expect_is_repo().returning(|_| Ok(true));
        assert!(mock.is_repo(Path::new("/tmp/repo")).await.unwrap());
    }
}
