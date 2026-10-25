use crate::domain::error::{EnolaError, Result};
use crate::ports::file::{UserDirFilePort, UserOwner};
use std::sync::Arc;

pub struct AddSshKey {
    user_dir: Arc<dyn UserDirFilePort + Send + Sync>,
}

impl AddSshKey {
    pub fn new(user_dir: Arc<dyn UserDirFilePort + Send + Sync>) -> Self {
        Self { user_dir }
    }

    /// Add a key to `<home>/.ssh/authorized_keys` of the user `owner`.
    ///
    /// The target home directory and owner are resolved by the CLI layer
    /// (`--user`, `SUDO_USER`, `$HOME`), not read from the environment here, so
    /// running via `sudo` targets the intended user instead of `/root`.
    /// SEC-A8: `~/.ssh` and `authorized_keys` are never followed if they are
    /// symlinks; the file keeps `owner` as its owner.
    pub async fn execute_for_home(
        &self,
        home: &std::path::Path,
        owner: UserOwner,
        pubkey: &str,
        comment: Option<&str>,
    ) -> Result<()> {
        let pubkey = pubkey.trim();

        // 1. Validate Public Key Format
        self.validate_key(pubkey)?;

        // 2. Determine target file (<home>/.ssh/authorized_keys)
        let ssh_dir = home.join(".ssh");

        self.execute_with_ssh_dir(&ssh_dir, owner, pubkey, comment)
            .await
    }

    /// Execute with an explicit `.ssh` directory (testable — avoids std::env::set_var).
    pub async fn execute_with_ssh_dir(
        &self,
        ssh_dir: &std::path::Path,
        owner: UserOwner,
        pubkey: &str,
        comment: Option<&str>,
    ) -> Result<()> {
        let pubkey = pubkey.to_string();
        let comment = comment.map(str::to_string);
        // 3-7. Lock + read + duplicate check + append + atomic write, all inside
        // the validated directory (UserDirFilePort, SEC-A8).
        self.user_dir
            .edit_locked(
                ssh_dir,
                owner,
                0o700,
                ".authorized_keys.lock",
                "authorized_keys",
                0o600,
                Box::new(move |content: &str| append_key(content, &pubkey, comment.as_deref())),
            )
            .await
    }

    fn validate_key(&self, key: &str) -> Result<()> {
        if key.contains('\n') || key.contains('\r') {
            return Err(EnolaError::ValidationError(
                "Key contains newlines".to_string(),
            ));
        }

        let parts: Vec<&str> = key.split_whitespace().collect();
        if parts.len() < 2 {
            return Err(EnolaError::ValidationError(
                "Key too short or invalid format".to_string(),
            ));
        }

        let key_type = parts[0];
        let valid_types = [
            "ssh-rsa",
            "ssh-ed25519",
            "ecdsa-sha2-nistp256",
            "ecdsa-sha2-nistp384",
            "ecdsa-sha2-nistp521",
            "sk-ssh-ed25519@openssh.com",
            "sk-ecdsa-sha2-nistp256@openssh.com",
        ];

        if !valid_types.contains(&key_type) {
            // It might be a valid type I missed, but let's be strict for now or allow if starts with ssh-
            if !key_type.starts_with("ssh-") && !key_type.starts_with("ecdsa-") {
                return Err(EnolaError::ValidationError(format!(
                    "Unsupported key type: {}",
                    key_type
                )));
            }
        }

        // Validate base64 body (parts[1])
        // Basic char check
        if !parts[1].chars().all(|c| {
            c.is_alphanumeric() || c == '+' || c == '/' || c == '=' || c == '-' || c == '_'
        }) {
            return Err(EnolaError::ValidationError(
                "Invalid characters in key body".to_string(),
            ));
        }

        Ok(())
    }
}

/// Pure append logic: `None` if the key body is already present (duplicate).
fn append_key(content: &str, pubkey: &str, comment: Option<&str>) -> Result<Option<String>> {
    // Simple duplicate check (exact match or key body match)
    // Key format: "type body comment"
    let key_parts: Vec<&str> = pubkey.split_whitespace().collect();
    if key_parts.len() < 2 {
        return Err(EnolaError::ValidationError(
            "Invalid key format".to_string(),
        ));
    }
    let key_body = key_parts[1]; // The base64 part

    if content.contains(key_body) {
        // Already exists
        return Ok(None);
    }

    let comment_str = comment.unwrap_or("enola-managed");
    let new_line = if key_parts.len() >= 3 {
        pubkey.to_string()
    } else {
        format!("{} {}", pubkey, comment_str)
    };

    let new_content = if content.ends_with('\n') || content.is_empty() {
        format!("{}{}\n", content, new_line)
    } else {
        format!("{}\n{}\n", content, new_line)
    };
    Ok(Some(new_content))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::file::MockUserDirFilePort;

    const OWNER: UserOwner = UserOwner {
        uid: 1000,
        gid: 1000,
    };

    #[tokio::test]
    async fn test_add_ssh_key_validation_failure() {
        let use_case = AddSshKey::new(Arc::new(MockUserDirFilePort::new()));
        let home_dir = tempfile::tempdir().unwrap();

        let result = use_case
            .execute_for_home(home_dir.path(), OWNER, "invalid-key", None)
            .await;
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_key_accepts_fido2_ecdsa() {
        let use_case = AddSshKey::new(Arc::new(MockUserDirFilePort::new()));

        // FIDO2 ECDSA keys were previously rejected (they don't start with
        // `ssh-`/`ecdsa-`). The explicit whitelist must accept them.
        let key = "sk-ecdsa-sha2-nistp256@openssh.com AAAAB3NzaC1yc2EAAAADAQABAAABAQ";
        assert!(use_case.validate_key(key).is_ok());
    }

    // SEC-A8: el caso de uso delega en UserDirFilePort con el dueño, el
    // directorio y los modos correctos (la validación de symlinks vive en el adapter).
    #[tokio::test]
    async fn add_key_edits_authorized_keys_in_user_dir_with_owner() {
        let home_dir = tempfile::tempdir().unwrap();
        let expected_dir = home_dir.path().join(".ssh");
        let mut mock = MockUserDirFilePort::new();
        mock.expect_edit_locked()
            .withf(move |dir, owner, dir_mode, lock, name, file_mode, _| {
                dir == expected_dir.as_path()
                    && *owner == OWNER
                    && *dir_mode == 0o700
                    && lock == ".authorized_keys.lock"
                    && name == "authorized_keys"
                    && *file_mode == 0o600
            })
            .times(1)
            .returning(|_, _, _, _, _, _, edit| {
                let out = edit("")?.expect("new key must be written");
                assert_eq!(out, "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIBPQL laptop\n");
                Ok(())
            });
        let use_case = AddSshKey::new(Arc::new(mock));
        use_case
            .execute_for_home(
                home_dir.path(),
                OWNER,
                "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIBPQL",
                Some("laptop"),
            )
            .await
            .expect("add key");
    }

    #[test]
    fn append_key_skips_duplicates() {
        let existing = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIBPQL existing\n";
        let r = append_key(existing, "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIBPQL", None).unwrap();
        assert!(r.is_none(), "duplicate key must not be written");
    }

    #[test]
    fn append_key_adds_newline_separator() {
        let r = append_key("ssh-rsa AAAAB3Nza old", "ssh-ed25519 AAAAC3NzaC1 new", None)
            .unwrap()
            .unwrap();
        assert_eq!(r, "ssh-rsa AAAAB3Nza old\nssh-ed25519 AAAAC3NzaC1 new\n");
    }
}
