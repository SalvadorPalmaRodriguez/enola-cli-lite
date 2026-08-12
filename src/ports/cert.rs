use crate::domain::error::Result;
use std::path::PathBuf;

#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait CertManagerPort {
    /// Generate a self-signed certificate for a hostname (e.g. onion address)
    /// Returns (Path to Cert, Path to Key)
    async fn generate_self_signed_cert(
        &self,
        hostname: &str,
        output_dir: &std::path::Path,
    ) -> Result<(PathBuf, PathBuf)>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[tokio::test]
    async fn test_mock_cert_generate() {
        let mut mock = MockCertManagerPort::new();
        mock.expect_generate_self_signed_cert().returning(|_, _| {
            Ok((
                PathBuf::from("/tmp/cert.pem"),
                PathBuf::from("/tmp/key.pem"),
            ))
        });
        let (cert, key) = mock
            .generate_self_signed_cert("test.onion", Path::new("/tmp"))
            .await
            .unwrap();
        assert!(cert.to_string_lossy().contains("cert.pem"));
        assert!(key.to_string_lossy().contains("key.pem"));
    }
}
