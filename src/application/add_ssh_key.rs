use crate::domain::error::{EnolaError, Result};
use crate::ports::file::FileManagerPort;
use std::path::PathBuf;
use std::sync::Arc;

pub struct AddSshKey {
    file_manager: Arc<dyn FileManagerPort + Send + Sync>,
}

impl AddSshKey {
    pub fn new(file_manager: Arc<dyn FileManagerPort + Send + Sync>) -> Self {
        Self { file_manager }
    }

    pub async fn execute(&self, pubkey: &str, comment: Option<&str>) -> Result<()> {
        let pubkey = pubkey.trim();

        // 1. Validate Public Key Format
        self.validate_key(pubkey)?;

        // 2. Determine target file (~/.ssh/authorized_keys)
        let home = std::env::var("HOME")
            .map_err(|_| EnolaError::InfrastructureError("HOME not set".to_string()))?;
        let ssh_dir = PathBuf::from(home).join(".ssh");
        let auth_keys_path = ssh_dir.join("authorized_keys");

        // 3. Ensure .ssh dir exists
        self.file_manager.ensure_dir(&ssh_dir).await?;

        // 4. Read existing keys to prevent duplicates
        // Note: FileManagerPort currently reads entire file.
        // For very large authorized_keys this is inefficient, but for this use case it's fine.
        let content = self
            .file_manager
            .read_file(&auth_keys_path)
            .await
            .unwrap_or_default();

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
            return Ok(());
        }

        // 5. Append Key
        let comment_str = comment.unwrap_or("enola-managed");
        // Construct line if pubkey doesn't already have comment or we want to override/append logic?
        // Usually pubkey string passed includes the type and body.

        // If the input pubkey already serves as a full line:
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

        self.file_manager
            .write_file(&auth_keys_path, &new_content)
            .await?;

        Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::file::MockFileManagerPort;

    #[tokio::test]
    async fn test_add_ssh_key_validation_failure() {
        let mock_file = MockFileManagerPort::new();
        let use_case = AddSshKey::new(Arc::new(mock_file));

        let result = use_case.execute("invalid-key", None).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_add_ssh_key_duplicate() {
        // Set HOME for the test environment
        std::env::set_var("HOME", "/tmp/test_home");

        let mut mock_file = MockFileManagerPort::new();

        mock_file.expect_ensure_dir().times(1).returning(|_| Ok(()));

        // Use a realistic looking SSH key body (valid base64 chars)
        mock_file
            .expect_read_file()
            .times(1)
            .returning(|_| Ok("ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIBPQL existing\n".to_string()));

        let use_case = AddSshKey::new(Arc::new(mock_file));
        // Use the same key body to trigger duplicate detection
        let key = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIBPQL";
        // Should succeed and do nothing because key already exists
        let result = use_case.execute(key, None).await;
        assert!(result.is_ok());
    }

    /*
    // This test is hard because we can't easily mock env::var("HOME")
    // We would need to refactor paths to be injectable or use a crate for path providers.
    #[tokio::test]
    async fn test_add_ssh_key_success() {
         ...
    }
    */
}
