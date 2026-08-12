use crate::domain::error::Result;

/// Port for reading and writing the Enola installation manifest.
///
/// The manifest is a flat file (`key|value` per line) that tracks every
/// component Enola creates at runtime (Docker containers, Nginx configs,
/// Tor hidden services, etc.) so that `uninstall.sh` can remove exactly
/// what Enola created — nothing more, nothing less.
///
/// Contract: `append()` and `remove()` are **best-effort**. If the manifest
/// file does not exist (e.g. dev/dogfooding via `cargo run` without
/// `install.sh`) or is not writable (e.g. running without sudo), they
/// silently no-op with a `tracing::warn` — a service `create` must never
/// fail because of manifest registration.
#[cfg_attr(test, mockall::automock)]
pub trait ManifestPort: Send + Sync {
    /// Append a `type|value` entry to the manifest.
    /// Best-effort: no-op (with warn) if the file is missing or not writable.
    fn append(&self, entry_type: &str, value: &str) -> Result<()>;

    /// Read all values for a given type. Returns an empty vec if the
    /// manifest does not exist or the type has no entries.
    fn get_all(&self, entry_type: &str) -> Result<Vec<String>>;

    /// Read the first value for a given type, or `None` if not found.
    fn get(&self, entry_type: &str) -> Result<Option<String>>;

    /// Returns `true` if the manifest file exists on disk.
    fn exists(&self) -> bool;

    /// Remove a specific `type|value` entry from the manifest.
    /// Best-effort: no-op (with warn) if the file is missing or not writable.
    fn remove(&self, entry_type: &str, value: &str) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_append() {
        let mut mock = MockManifestPort::new();
        mock.expect_append().returning(|_, _| Ok(()));
        assert!(mock.append("docker_container", "wp-mysite").is_ok());
    }

    #[test]
    fn test_mock_get_all() {
        let mut mock = MockManifestPort::new();
        mock.expect_get_all()
            .returning(|_| Ok(vec!["wp-mysite".into()]));
        let result = mock.get_all("docker_container").unwrap();
        assert_eq!(result, vec!["wp-mysite"]);
    }

    #[test]
    fn test_mock_get() {
        let mut mock = MockManifestPort::new();
        mock.expect_get()
            .returning(|_| Ok(Some("/usr/local/bin/enola-cli".into())));
        let result = mock.get("binary").unwrap();
        assert_eq!(result, Some("/usr/local/bin/enola-cli".to_string()));
    }

    #[test]
    fn test_mock_exists() {
        let mut mock = MockManifestPort::new();
        mock.expect_exists().returning(|| true);
        assert!(mock.exists());
    }

    #[test]
    fn test_mock_remove() {
        let mut mock = MockManifestPort::new();
        mock.expect_remove().returning(|_, _| Ok(()));
        assert!(mock.remove("docker_container", "wp-mysite").is_ok());
    }
}
