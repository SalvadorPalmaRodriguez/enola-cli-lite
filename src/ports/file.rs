use crate::domain::error::Result;
use std::collections::HashMap;
use std::path::Path;

#[async_trait::async_trait]
pub trait FileManagerPort {
    /// Read entire file as string
    async fn read_file(&self, path: &Path) -> Result<String>;

    /// Write string to file (atomic move preferred)
    async fn write_file(&self, path: &Path, content: &str) -> Result<()>;

    /// Ensure directory exists
    async fn ensure_dir(&self, path: &Path) -> Result<()>;

    /// Read .env style file into Map
    async fn read_env(&self, path: &Path) -> Result<HashMap<String, String>>;

    /// Update or Insert a key in a .env file, preserving other lines/comments if possible
    async fn update_env_key(&self, path: &Path, key: &str, value: &str) -> Result<()>;

    /// Securely delete a file
    async fn delete_file(&self, path: &Path) -> Result<()>;

    /// Copy a file
    async fn copy_file(&self, from: &Path, to: &Path) -> Result<()>;

    /// Set file ownership (chown) - user:group
    async fn set_ownership(&self, path: &Path, user: &str, group: &str) -> Result<()>;

    /// Set file permissions (chmod) - octal mode e.g. 0o755
    async fn set_permissions(&self, path: &Path, mode: u32) -> Result<()>;

    /// Create a tar.gz archive from source_dir into dest_file
    async fn create_archive(&self, source_dir: &Path, dest_file: &Path) -> Result<()>;

    /// Extract a tar.gz archive into dest_dir
    async fn extract_archive(&self, archive: &Path, dest_dir: &Path) -> Result<()>;
}

#[cfg(test)]
mockall::mock! {
    pub FileManagerPort {}
    #[async_trait::async_trait]
    impl FileManagerPort for FileManagerPort {
        async fn read_file(&self, path: &Path) -> Result<String>;
        async fn write_file(&self, path: &Path, content: &str) -> Result<()>;
        async fn ensure_dir(&self, path: &Path) -> Result<()>;
        async fn read_env(&self, path: &Path) -> Result<HashMap<String, String>>;
        async fn update_env_key(&self, path: &Path, key: &str, value: &str) -> Result<()>;
        async fn delete_file(&self, path: &Path) -> Result<()>;
        async fn copy_file(&self, from: &Path, to: &Path) -> Result<()>;
        async fn set_ownership(&self, path: &Path, user: &str, group: &str) -> Result<()>;
        async fn set_permissions(&self, path: &Path, mode: u32) -> Result<()>;
        async fn create_archive(&self, source_dir: &Path, dest_file: &Path) -> Result<()>;
        async fn extract_archive(&self, archive: &Path, dest_dir: &Path) -> Result<()>;
    }
}
