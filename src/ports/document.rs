/// Port for extracting text content from binary documents (PDF, Office, etc.)
/// Segregated from FileManagerPort per ISP — not all file consumers need extraction.
use crate::domain::error::Result;
use std::path::Path;

#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait DocumentExtractorPort {
    /// Extract plain text from a PDF file using pdftotext or equivalent
    async fn pdf_to_text(&self, pdf_path: &Path) -> Result<String>;

    /// Run a Python script to extract/process content, returns stdout
    async fn run_python_script(&self, script_path: &Path, args: Vec<String>) -> Result<String>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_pdf_to_text() {
        let mut mock = MockDocumentExtractorPort::new();
        mock.expect_pdf_to_text()
            .returning(|_| Ok("extracted text".to_string()));
        let result = mock.pdf_to_text(std::path::Path::new("test.pdf")).await;
        assert_eq!(result.unwrap(), "extracted text");
    }

    #[tokio::test]
    async fn test_mock_run_python_script() {
        let mut mock = MockDocumentExtractorPort::new();
        mock.expect_run_python_script()
            .returning(|_, _| Ok("script output".to_string()));
        let result = mock
            .run_python_script(std::path::Path::new("script.py"), vec!["arg1".to_string()])
            .await;
        assert_eq!(result.unwrap(), "script output");
    }
}
