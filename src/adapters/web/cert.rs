use crate::domain::error::{EnolaError, Result};
use crate::ports::cert::CertManagerPort;
use openssl::asn1::Asn1Time;
use openssl::hash::MessageDigest;
use openssl::pkey::PKey;
use openssl::rsa::Rsa;
use openssl::x509::extension::{BasicConstraints, SubjectAlternativeName};
use openssl::x509::{X509Name, X509};
use std::path::{Path, PathBuf};

pub struct OpenSslCertAdapter;

impl Default for OpenSslCertAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl OpenSslCertAdapter {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl CertManagerPort for OpenSslCertAdapter {
    async fn generate_self_signed_cert(
        &self,
        hostname: &str,
        output_dir: &Path,
    ) -> Result<(PathBuf, PathBuf)> {
        // Output filenames
        let cert_name = if hostname.ends_with(".onion") {
            "onion"
        } else {
            hostname
        };
        let cert_path = output_dir.join(format!("{}.crt", cert_name));
        let key_path = output_dir.join(format!("{}.key", cert_name));

        if cert_path.exists() && key_path.exists() {
            return Ok((cert_path, key_path));
        }

        // Generate Key
        let rsa = Rsa::generate(4096).map_err(|e| {
            EnolaError::InfrastructureError(format!("Failed to generate RSA key: {}", e))
        })?;
        let pkey = PKey::from_rsa(rsa).map_err(|e| {
            EnolaError::InfrastructureError(format!("Failed to create PKey: {}", e))
        })?;

        // Generate Cert
        let mut builder = X509::builder().map_err(|e| {
            EnolaError::InfrastructureError(format!("Failed to create X509 builder: {}", e))
        })?;

        builder
            .set_version(2)
            .map_err(|e| EnolaError::InfrastructureError(format!("set_version failed: {}", e)))?;

        let mut name = X509Name::builder().map_err(|e| {
            EnolaError::InfrastructureError(format!("X509Name::builder failed: {}", e))
        })?;
        name.append_entry_by_text("C", "XX")
            .map_err(|e| EnolaError::InfrastructureError(format!("set country failed: {}", e)))?;
        name.append_entry_by_text("O", "Tor Hidden Service")
            .map_err(|e| EnolaError::InfrastructureError(format!("set org failed: {}", e)))?;
        name.append_entry_by_text("CN", hostname)
            .map_err(|e| EnolaError::InfrastructureError(format!("set CN failed: {}", e)))?;
        let name = name.build();

        builder.set_subject_name(&name).map_err(|e| {
            EnolaError::InfrastructureError(format!("set_subject_name failed: {}", e))
        })?;
        builder.set_issuer_name(&name).map_err(|e| {
            EnolaError::InfrastructureError(format!("set_issuer_name failed: {}", e))
        })?;

        // Validity: 10 years
        let not_before = Asn1Time::days_from_now(0).map_err(|e| {
            EnolaError::InfrastructureError(format!("Asn1Time not_before failed: {}", e))
        })?;
        let not_after = Asn1Time::days_from_now(3650).map_err(|e| {
            EnolaError::InfrastructureError(format!("Asn1Time not_after failed: {}", e))
        })?;
        builder.set_not_before(&not_before).map_err(|e| {
            EnolaError::InfrastructureError(format!("set_not_before failed: {}", e))
        })?;
        builder
            .set_not_after(&not_after)
            .map_err(|e| EnolaError::InfrastructureError(format!("set_not_after failed: {}", e)))?;

        builder
            .set_pubkey(&pkey)
            .map_err(|e| EnolaError::InfrastructureError(format!("set_pubkey failed: {}", e)))?;

        // Extensions
        let basic_constraints = BasicConstraints::new()
            .critical()
            .ca()
            .build()
            .map_err(|e| {
                EnolaError::InfrastructureError(format!("BasicConstraints failed: {}", e))
            })?;
        builder.append_extension(basic_constraints).map_err(|e| {
            EnolaError::InfrastructureError(format!("append BasicConstraints failed: {}", e))
        })?;

        let san = SubjectAlternativeName::new()
            .dns(hostname)
            .dns(&format!("www.{}", hostname))
            .build(&builder.x509v3_context(None, None))
            .map_err(|e| {
                EnolaError::InfrastructureError(format!("SubjectAlternativeName failed: {}", e))
            })?;
        builder
            .append_extension(san)
            .map_err(|e| EnolaError::InfrastructureError(format!("append SAN failed: {}", e)))?;

        // Sign
        builder
            .sign(&pkey, MessageDigest::sha256())
            .map_err(|e| EnolaError::InfrastructureError(format!("Failed to sign cert: {}", e)))?;

        let cert = builder.build();

        // Write files
        let cert_pem = cert.to_pem().map_err(|e| {
            EnolaError::InfrastructureError(format!("Failed to encode cert PEM: {}", e))
        })?;
        let key_pem = pkey.private_key_to_pem_pkcs8().map_err(|e| {
            EnolaError::InfrastructureError(format!("Failed to encode key PEM: {}", e))
        })?;

        if !output_dir.exists() {
            tokio::fs::create_dir_all(output_dir).await.map_err(|e| {
                EnolaError::InfrastructureError(format!("Failed to create output dir: {}", e))
            })?;
        }

        // Atomic writes with correct permissions from inception (anti-TOCTOU)
        let cert_path_owned = cert_path.clone();
        tokio::task::spawn_blocking(move || {
            crate::infrastructure::atomic_secret_file::write_atomic(
                &cert_path_owned,
                &cert_pem,
                0o644,
            )
        })
        .await
        .map_err(|e| EnolaError::InfrastructureError(format!("spawn_blocking: {}", e)))?
        .map_err(|e| EnolaError::InfrastructureError(format!("Failed to write cert: {}", e)))?;

        let key_path_owned = key_path.clone();
        tokio::task::spawn_blocking(move || {
            crate::infrastructure::atomic_secret_file::write_atomic(
                &key_path_owned,
                &key_pem,
                0o600,
            )
        })
        .await
        .map_err(|e| EnolaError::InfrastructureError(format!("spawn_blocking: {}", e)))?
        .map_err(|e| EnolaError::InfrastructureError(format!("Failed to write key: {}", e)))?;

        Ok((cert_path, key_path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::cert::CertManagerPort;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_generate_self_signed_cert() {
        let dir = TempDir::new().unwrap();
        let adapter = OpenSslCertAdapter::new();
        let (cert_path, key_path) = adapter
            .generate_self_signed_cert("test.local", dir.path())
            .await
            .unwrap();
        assert!(cert_path.exists());
        assert!(key_path.exists());
        assert!(cert_path.to_string_lossy().contains("test.local"));
    }

    #[tokio::test]
    async fn test_generate_onion_cert() {
        let dir = TempDir::new().unwrap();
        let adapter = OpenSslCertAdapter::new();
        let (cert_path, key_path) = adapter
            .generate_self_signed_cert("abc123.onion", dir.path())
            .await
            .unwrap();
        assert!(cert_path.exists());
        assert!(key_path.exists());
        // Onion certs use "onion" as name prefix
        assert!(cert_path.to_string_lossy().contains("onion.crt"));
    }

    #[tokio::test]
    async fn test_cert_already_exists_returns_existing() {
        let dir = TempDir::new().unwrap();
        let adapter = OpenSslCertAdapter::new();
        let (cert1, key1) = adapter
            .generate_self_signed_cert("existing.local", dir.path())
            .await
            .unwrap();
        // Call again — should return same paths
        let (cert2, key2) = adapter
            .generate_self_signed_cert("existing.local", dir.path())
            .await
            .unwrap();
        assert_eq!(cert1, cert2);
        assert_eq!(key1, key2);
    }

    #[test]
    fn test_default_constructor() {
        let adapter = OpenSslCertAdapter;
        let _ = adapter;
    }
}
