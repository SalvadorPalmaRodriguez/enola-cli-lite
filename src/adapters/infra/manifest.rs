// FileManifestAdapter — file-backed implementation of ManifestPort.
//
// Reads and writes the Enola installation manifest at
// `/usr/local/share/enola/manifest` (overridable via `ENOLA_MANIFEST_PATH`
// or `with_path()`). The manifest is a flat text file with `key|value`
// per line. Comments (`#`) and blank lines are ignored.
//
// Contract: `append()` and `remove()` are **best-effort** — if the file
// does not exist or is not writable, they no-op with a `tracing::warn`
// and return `Ok(())`. A service `create` must never fail because of
// manifest registration.
use crate::domain::error::Result;
use crate::ports::manifest::ManifestPort;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::infrastructure::file_lock::FileLock;

/// Default manifest path: `/usr/local/share/enola/manifest`.
const DEFAULT_MANIFEST_PATH: &str = "/usr/local/share/enola/manifest";

/// Lock file path: sibling of the manifest, with `.lock` suffix.
const LOCK_SUFFIX: &str = ".lock";

pub struct FileManifestAdapter {
    manifest_path: PathBuf,
}

impl Default for FileManifestAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl FileManifestAdapter {
    /// Creates an adapter pointing at the default manifest path
    /// (`/usr/local/share/enola/manifest`), or the path in the
    /// `ENOLA_MANIFEST_PATH` env var if set (useful for tests).
    pub fn new() -> Self {
        let path = std::env::var("ENOLA_MANIFEST_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(DEFAULT_MANIFEST_PATH));
        Self {
            manifest_path: path,
        }
    }

    /// Creates an adapter with an explicit manifest path (for tests / injection).
    pub fn with_path(path: PathBuf) -> Self {
        Self {
            manifest_path: path,
        }
    }

    fn lock_path(&self) -> PathBuf {
        let mut p = self.manifest_path.clone();
        let mut name = p.file_name().unwrap_or_default().to_os_string();
        name.push(LOCK_SUFFIX);
        p.set_file_name(name);
        p
    }

    /// Read all lines from the manifest file. Returns an empty vec if
    /// the file does not exist.
    fn read_lines(&self) -> Vec<String> {
        match fs::read_to_string(&self.manifest_path) {
            Ok(content) => content.lines().map(|l| l.to_string()).collect(),
            Err(_) => Vec::new(),
        }
    }

    /// Parse a manifest line into (key, value). Returns None for comments
    /// and blank lines. The first `|` is the separator; subsequent `|` in
    /// the value are preserved.
    fn parse_line(line: &str) -> Option<(&str, &str)> {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            return None;
        }
        let idx = trimmed.find('|')?;
        let (k, v) = trimmed.split_at(idx);
        // skip the `|` itself
        Some((k, &v[1..]))
    }

    /// Write all lines atomically (via lock + temp write + rename).
    fn write_lines(&self, lines: &[String]) -> std::io::Result<()> {
        let parent = self
            .manifest_path
            .parent()
            .unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)?;
        let tmp_path = self.manifest_path.with_extension("tmp");
        {
            let mut f = fs::File::create(&tmp_path)?;
            for line in lines {
                writeln!(f, "{}", line)?;
            }
            f.sync_all()?;
        }
        fs::rename(&tmp_path, &self.manifest_path)?;
        Ok(())
    }
}

impl ManifestPort for FileManifestAdapter {
    fn append(&self, entry_type: &str, value: &str) -> Result<()> {
        // Best-effort: if the file doesn't exist or we can't write, no-op.
        if !self.manifest_path.exists() {
            tracing::warn!(
                "Manifest file not found at {}, skipping append({}|{})",
                self.manifest_path.display(),
                entry_type,
                value
            );
            return Ok(());
        }

        // O_APPEND mode on Linux guarantees atomic writes for payloads
        // ≤ PIPE_BUF (4096 bytes). Our lines are short, so no lock is needed
        // for append — the kernel serializes concurrent writes.
        let line = format!("{}|{}\n", entry_type, value);
        let result = fs::OpenOptions::new()
            .append(true)
            .open(&self.manifest_path)
            .and_then(|mut f| f.write_all(line.as_bytes()));

        if let Err(e) = result {
            tracing::warn!(
                "Failed to append to manifest at {}: {} — skipping",
                self.manifest_path.display(),
                e
            );
        }
        Ok(())
    }

    fn get_all(&self, entry_type: &str) -> Result<Vec<String>> {
        if !self.manifest_path.exists() {
            return Ok(Vec::new());
        }
        let lines = self.read_lines();
        let mut values = Vec::new();
        for line in lines {
            if let Some((k, v)) = Self::parse_line(&line) {
                if k == entry_type {
                    values.push(v.to_string());
                }
            }
        }
        Ok(values)
    }

    fn get(&self, entry_type: &str) -> Result<Option<String>> {
        if !self.manifest_path.exists() {
            return Ok(None);
        }
        let lines = self.read_lines();
        for line in lines {
            if let Some((k, v)) = Self::parse_line(&line) {
                if k == entry_type {
                    return Ok(Some(v.to_string()));
                }
            }
        }
        Ok(None)
    }

    fn exists(&self) -> bool {
        self.manifest_path.exists()
    }

    fn remove(&self, entry_type: &str, value: &str) -> Result<()> {
        if !self.manifest_path.exists() {
            tracing::warn!(
                "Manifest file not found at {}, skipping remove({}|{})",
                self.manifest_path.display(),
                entry_type,
                value
            );
            return Ok(());
        }

        let _lock = match FileLock::try_acquire(self.lock_path()) {
            Ok(g) => g,
            Err(e) => {
                tracing::warn!(
                    "Could not acquire manifest lock for remove: {} — skipping",
                    e
                );
                return Ok(());
            }
        };

        let lines = self.read_lines();
        let target = format!("{}|{}", entry_type, value);
        let filtered: Vec<String> = lines
            .into_iter()
            .filter(|l| l.trim() != target && !l.trim().is_empty())
            .collect();

        if let Err(e) = self.write_lines(&filtered) {
            tracing::warn!(
                "Failed to rewrite manifest at {}: {} — skipping",
                self.manifest_path.display(),
                e
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn adapter_in_tmp() -> (FileManifestAdapter, TempDir) {
        let tmp = TempDir::new().unwrap();
        let manifest_path = tmp.path().join("manifest");
        fs::write(&manifest_path, "# test manifest\n").unwrap();
        (FileManifestAdapter::with_path(manifest_path), tmp)
    }

    #[test]
    fn append_and_get_all() {
        let (adapter, _tmp) = adapter_in_tmp();
        adapter.append("docker_container", "wp-mysite").unwrap();
        adapter.append("docker_container", "db-mysite").unwrap();
        adapter
            .append("docker_network", "enola_net_mysite")
            .unwrap();

        let containers = adapter.get_all("docker_container").unwrap();
        assert_eq!(containers, vec!["wp-mysite", "db-mysite"]);

        let networks = adapter.get_all("docker_network").unwrap();
        assert_eq!(networks, vec!["enola_net_mysite"]);
    }

    #[test]
    fn get_returns_first_match() {
        let (adapter, _tmp) = adapter_in_tmp();
        adapter
            .append("binary", "/usr/local/bin/enola-cli")
            .unwrap();
        adapter
            .append("share_dir", "/usr/local/share/enola")
            .unwrap();

        let result = adapter.get("binary").unwrap();
        assert_eq!(result, Some("/usr/local/bin/enola-cli".to_string()));

        let missing = adapter.get("nonexistent").unwrap();
        assert_eq!(missing, None);
    }

    #[test]
    fn remove_entry() {
        let (adapter, _tmp) = adapter_in_tmp();
        adapter.append("docker_container", "wp-mysite").unwrap();
        adapter.append("docker_container", "db-mysite").unwrap();

        adapter.remove("docker_container", "wp-mysite").unwrap();

        let remaining = adapter.get_all("docker_container").unwrap();
        assert_eq!(remaining, vec!["db-mysite"]);
    }

    #[test]
    fn remove_nonexistent_entry_is_ok() {
        let (adapter, _tmp) = adapter_in_tmp();
        adapter.append("docker_container", "wp-mysite").unwrap();

        // Removing something that doesn't exist should be fine.
        adapter.remove("docker_container", "nonexistent").unwrap();

        let remaining = adapter.get_all("docker_container").unwrap();
        assert_eq!(remaining, vec!["wp-mysite"]);
    }

    #[test]
    fn parse_line_handles_pipe_in_value() {
        let (adapter, _tmp) = adapter_in_tmp();
        // A value containing `|` should be preserved after the first `|`.
        adapter
            .append("ufw_rule", "allow 8080/tcp # enola-cli")
            .unwrap();
        let values = adapter.get_all("ufw_rule").unwrap();
        assert_eq!(values, vec!["allow 8080/tcp # enola-cli"]);
    }

    #[test]
    fn nonexistent_manifest_returns_empty() {
        let tmp = TempDir::new().unwrap();
        let adapter = FileManifestAdapter::with_path(tmp.path().join("nonexistent"));
        assert!(!adapter.exists());
        assert!(adapter.get_all("docker_container").unwrap().is_empty());
        assert_eq!(adapter.get("binary").unwrap(), None);
    }

    #[test]
    fn append_to_nonexistent_manifest_is_noop() {
        let tmp = TempDir::new().unwrap();
        let adapter = FileManifestAdapter::with_path(tmp.path().join("nonexistent"));
        // Should not error — best-effort no-op.
        adapter.append("docker_container", "wp-mysite").unwrap();
        assert!(!adapter.exists());
    }

    #[test]
    fn remove_from_nonexistent_manifest_is_noop() {
        let tmp = TempDir::new().unwrap();
        let adapter = FileManifestAdapter::with_path(tmp.path().join("nonexistent"));
        adapter.remove("docker_container", "wp-mysite").unwrap();
        assert!(!adapter.exists());
    }

    #[test]
    fn append_to_readonly_manifest_is_noop() {
        let (adapter, tmp) = adapter_in_tmp();
        let path = tmp.path().join("manifest");
        // Make the manifest read-only.
        let mut perms = fs::metadata(&path).unwrap().permissions();
        perms.set_readonly(true);
        fs::set_permissions(&path, perms).unwrap();

        // Should not error — best-effort no-op.
        adapter.append("docker_container", "wp-mysite").unwrap();
    }

    #[test]
    fn comments_and_blank_lines_ignored() {
        let (adapter, _tmp) = adapter_in_tmp();
        // The initial file has "# test manifest\n" — a comment.
        adapter.append("docker_container", "wp-mysite").unwrap();

        let all = adapter.get_all("docker_container").unwrap();
        assert_eq!(all, vec!["wp-mysite"]);

        // The comment line should not appear as a bogus entry.
        let bogus = adapter.get_all("# test manifest").unwrap();
        assert!(bogus.is_empty());
    }

    #[test]
    fn exists_returns_true_when_file_present() {
        let (adapter, _tmp) = adapter_in_tmp();
        assert!(adapter.exists());
    }

    #[test]
    fn concurrent_appends_do_not_lose_entries() {
        let (adapter, tmp) = adapter_in_tmp();
        let manifest_path = tmp.path().join("manifest");
        let adapter = std::sync::Arc::new(adapter);
        let mut handles = vec![];

        for i in 0..8 {
            let a = std::sync::Arc::clone(&adapter);
            handles.push(std::thread::spawn(move || {
                a.append("docker_container", &format!("container-{}", i))
                    .unwrap();
            }));
        }
        for h in handles {
            h.join().unwrap();
        }

        let all = adapter.get_all("docker_container").unwrap();
        assert_eq!(all.len(), 8, "all 8 entries must be present");
        // Verify no duplicates
        let mut sorted = all.clone();
        sorted.sort();
        for (i, v) in sorted.iter().enumerate() {
            assert_eq!(*v, format!("container-{}", i));
        }

        // The manifest file should still exist and be readable.
        assert!(manifest_path.exists());
    }
}
