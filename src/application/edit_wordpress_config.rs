use crate::domain::error::{EnolaError, Result};
use crate::ports::file::FileManagerPort;
use regex::Regex;
use std::path::PathBuf;
use std::sync::Arc;

pub struct EditWordPressConfig {
    file_manager: Arc<dyn FileManagerPort + Send + Sync>,
}

impl EditWordPressConfig {
    pub fn new(file_manager: Arc<dyn FileManagerPort + Send + Sync>) -> Self {
        Self { file_manager }
    }

    pub async fn execute(&self, blog_name: &str, key: &str, value: &str) -> Result<()> {
        let wp_config_path =
            PathBuf::from(format!("/srv/enola-wordpress/{}/wp-config.php", blog_name));

        if !wp_config_path.exists() {
            return Err(EnolaError::NotFound(format!(
                "wp-config.php not found for {}",
                blog_name
            )));
        }

        let content = self.file_manager.read_file(&wp_config_path).await?;
        let new_content = apply_config_edit(&content, key, value)?;
        self.file_manager
            .write_file(&wp_config_path, &new_content)
            .await
    }
}
/// Pure function: apply a config edit to wp-config.php content.
/// Replaces `define( 'KEY', '...' );` if found, otherwise inserts before the stop line.
pub fn apply_config_edit(content: &str, key: &str, value: &str) -> Result<String> {
    let escaped_key = regex::escape(key);
    let pattern = format!(r#"define\(\s*['"]{}['"]\s*,\s*.+?\)\s*;"#, escaped_key);
    let regex = Regex::new(&pattern)
        .map_err(|e| EnolaError::ValidationError(format!("Invalid regex: {}", e)))?;

    let replacement = format!("define( '{}', '{}' );", key, value);

    let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
    let mut found = false;

    for line in lines.iter_mut() {
        if regex.is_match(line) {
            *line = regex.replace(line, &replacement).to_string();
            found = true;
            break;
        }
    }

    if !found {
        let stop_line = "/* That's all, stop editing! Happy publishing. */";
        let mut insert_pos = None;
        for (i, line) in lines.iter().enumerate() {
            if line.contains(stop_line) {
                insert_pos = Some(i);
                break;
            }
        }

        if let Some(pos) = insert_pos {
            lines.insert(pos, replacement);
        } else {
            lines.push(replacement);
        }
    }

    Ok(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_replace_simple_define() {
        let content = "<?php\ndefine( 'DB_NAME', 'old_value' );\ndefine( 'DB_USER', 'root' );\n";
        let result = apply_config_edit(content, "DB_NAME", "new_db").unwrap();
        assert!(result.contains("define( 'DB_NAME', 'new_db' );"));
        assert!(result.contains("define( 'DB_USER', 'root' );"));
        assert!(!result.contains("old_value"));
    }

    #[test]
    fn test_replace_getenv_docker_format() {
        let content =
            "<?php\ndefine( 'DB_NAME', getenv_docker('WORDPRESS_DB_NAME', 'wordpress') );\n";
        let result = apply_config_edit(content, "DB_NAME", "mydb").unwrap();
        assert!(result.contains("define( 'DB_NAME', 'mydb' );"));
        assert!(!result.contains("getenv_docker"));
    }

    #[test]
    fn test_replace_double_quotes() {
        let content = "<?php\ndefine( \"DB_NAME\", \"old\" );\n";
        let result = apply_config_edit(content, "DB_NAME", "new").unwrap();
        assert!(result.contains("define( 'DB_NAME', 'new' );"));
    }

    #[test]
    fn test_replace_with_extra_whitespace() {
        let content = "<?php\ndefine(    'DB_NAME'   ,   'old'   );\n";
        let result = apply_config_edit(content, "DB_NAME", "new").unwrap();
        assert!(result.contains("define( 'DB_NAME', 'new' );"));
    }

    #[test]
    fn test_insert_new_key_before_stop_line() {
        let content = "<?php\n/* That's all, stop editing! Happy publishing. */\n\n";
        let result = apply_config_edit(content, "WP_DEBUG", "true").unwrap();
        let lines: Vec<&str> = result.lines().collect();
        let define_line = lines.iter().find(|l| l.contains("WP_DEBUG"));
        let stop_line = lines.iter().find(|l| l.contains("That's all"));
        assert!(define_line.is_some());
        assert!(stop_line.is_some());
        let define_idx = lines.iter().position(|l| l.contains("WP_DEBUG")).unwrap();
        let stop_idx = lines.iter().position(|l| l.contains("That's all")).unwrap();
        assert!(define_idx < stop_idx, "define should be before stop line");
    }

    #[test]
    fn test_insert_new_key_appended_if_no_stop_line() {
        let content = "<?php\necho 'hello';\n";
        let result = apply_config_edit(content, "NEW_KEY", "value").unwrap();
        assert!(result.contains("define( 'NEW_KEY', 'value' );"));
        let lines: Vec<&str> = result.lines().collect();
        assert!(lines.last().unwrap().contains("NEW_KEY"));
    }

    #[test]
    fn test_key_with_regex_special_chars_escaped() {
        let content = "<?php\ndefine( 'WP_CACHE', 'old' );\n";
        let result = apply_config_edit(content, "WP_CACHE", "new").unwrap();
        assert!(result.contains("define( 'WP_CACHE', 'new' );"));
    }

    #[test]
    fn test_replace_preserves_other_defines() {
        let content = "<?php\ndefine( 'DB_NAME', 'old' );\ndefine( 'DB_USER', 'root' );\ndefine( 'DB_PASSWORD', 'secret' );\n";
        let result = apply_config_edit(content, "DB_USER", "admin").unwrap();
        assert!(result.contains("define( 'DB_NAME', 'old' );"));
        assert!(result.contains("define( 'DB_USER', 'admin' );"));
        assert!(result.contains("define( 'DB_PASSWORD', 'secret' );"));
    }

    #[test]
    fn test_struct_creation() {
        let mock = std::sync::Arc::new(crate::ports::file::MockFileManagerPort::new());
        let _svc = EditWordPressConfig::new(mock);
    }
}
