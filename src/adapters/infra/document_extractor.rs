/// Adapter for DocumentExtractorPort — uses pdftotext and python3
use crate::domain::error::{EnolaError, Result};
use crate::ports::document::DocumentExtractorPort;
use std::path::Path;

pub struct ShellDocumentExtractor;

#[async_trait::async_trait]
impl DocumentExtractorPort for ShellDocumentExtractor {
    async fn pdf_to_text(&self, pdf_path: &Path) -> Result<String> {
        let output = tokio::process::Command::new("pdftotext")
            .args([pdf_path.to_string_lossy().as_ref(), "-"])
            .output()
            .await
            .map_err(|e| EnolaError::InfrastructureError(format!("pdftotext failed: {}", e)))?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            Err(EnolaError::InfrastructureError(
                String::from_utf8_lossy(&output.stderr).to_string(),
            ))
        }
    }

    async fn run_python_script(&self, script_path: &Path, args: Vec<String>) -> Result<String> {
        let mut cmd = tokio::process::Command::new("python3");
        cmd.arg(script_path.to_string_lossy().as_ref());
        for arg in &args {
            cmd.arg(arg);
        }
        let output = cmd
            .output()
            .await
            .map_err(|e| EnolaError::InfrastructureError(format!("python3 failed: {}", e)))?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            Err(EnolaError::InfrastructureError(
                String::from_utf8_lossy(&output.stderr).to_string(),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_struct_creation() {
        let _ = ShellDocumentExtractor;
    }

    // Real tests would need pdftotext and python3 installed
    // We test that missing commands return errors gracefully
    #[tokio::test]
    async fn test_pdf_to_text_nonexistent_file() {
        let extractor = ShellDocumentExtractor;
        let result = extractor
            .pdf_to_text(std::path::Path::new("/nonexistent/file.pdf"))
            .await;
        // pdftotext should error on nonexistent file
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_run_python_script_nonexistent() {
        let extractor = ShellDocumentExtractor;
        let result = extractor
            .run_python_script(std::path::Path::new("/nonexistent/script.py"), vec![])
            .await;
        assert!(result.is_err());
    }
}
