//! Fase 14.2 — Adapter de descarga del tokenizer HF de un repositorio
//! (`ai model download`, host-side, previo al entrenamiento offline).
//!
//! El contenedor de entrenamiento corre SIN red y con el rootfs en read-only,
//! así que el tokenizer debe bajarse en el HOST y montarse como volumen. Aquí
//! se descargan los ficheros canónicos del tokenizer de un repo HF a un
//! directorio local; los ficheros ausentes del repo (HTTP 404) se saltan.
//!
//! - `tokenizer_config.json` es el mínimo imprescindible para que
//!   `AutoTokenizer.from_pretrained(dir)` instancie el tokenizer.
//! - Egress: clearnet por defecto o Tor SOCKS5h con `use_tor` (R22).

use crate::domain::error::{EnolaError, Result};
use crate::ports::ai::{TokenizerDownloadOutcome, TokenizerDownloadPort, TokenizerDownloadRequest};

/// Ficheros canónicos de un tokenizer HF. Se descargan los que existan en el
/// repo (un 404 se salta). El orden antepone `tokenizer_config.json`, que es
/// el fichero crítico.
const TOKENIZER_FILES: &[&str] = &[
    "tokenizer_config.json",
    "tokenizer.json",
    "special_tokens_map.json",
    "added_tokens.json",
    "vocab.json",
    "merges.txt",
    "tokenizer.model",
];

/// Adapter de descarga del tokenizer HF sobre HTTP (`reqwest`).
pub struct ReqwestTokenizerDownloader {
    base_url: String,
}

impl ReqwestTokenizerDownloader {
    /// Crea el adapter apuntando a HuggingFace.
    pub fn new() -> Self {
        Self {
            base_url: crate::domain::ai_model::HUGGINGFACE_BASE_URL.to_string(),
        }
    }

    /// Constructor con URL base inyectable (tests contra un servidor local).
    #[cfg(test)]
    fn new_with_base(base_url: String) -> Self {
        Self { base_url }
    }

    fn file_url(&self, repo: &str, file: &str) -> String {
        format!(
            "{}/{repo}/resolve/main/{file}",
            self.base_url.trim_end_matches('/')
        )
    }

    fn build_client(&self, use_tor: bool) -> Result<reqwest::Client> {
        if use_tor {
            crate::infrastructure::http::build_download_client_via_tor(&self.base_url)
        } else {
            crate::infrastructure::http::build_download_client(&self.base_url)
        }
    }
}

impl Default for ReqwestTokenizerDownloader {
    fn default() -> Self {
        Self::new()
    }
}

fn apply_restrictive_permissions(path: &std::path::Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o640)).map_err(|e| {
            EnolaError::InfrastructureError(format!(
                "No se pudieron aplicar permisos 0640 a '{}': {e}",
                path.display()
            ))
        })?;
    }
    Ok(())
}

#[async_trait::async_trait]
impl TokenizerDownloadPort for ReqwestTokenizerDownloader {
    async fn download_tokenizer(
        &self,
        req: TokenizerDownloadRequest,
    ) -> Result<TokenizerDownloadOutcome> {
        crate::domain::ai_model::validate_hf_repo(&req.repo)?;
        tokio::fs::create_dir_all(&req.destination)
            .await
            .map_err(|e| {
                EnolaError::InfrastructureError(format!(
                    "No se pudo crear el directorio del tokenizer '{}': {e}",
                    req.destination.display()
                ))
            })?;

        let client = self.build_client(req.use_tor)?;
        let mut files_downloaded = 0usize;
        let mut config_downloaded = false;
        let mut last_error: Option<String> = None;

        for file in TOKENIZER_FILES {
            let url = self.file_url(&req.repo, file);
            let dest = req.destination.join(file);
            let result = client.get(&url).send().await;
            match result {
                Ok(resp) if resp.status().is_success() => {
                    let bytes = resp.bytes().await.map_err(|e| {
                        EnolaError::InfrastructureError(format!(
                            "Error leyendo '{file}' de '{url}': {e}"
                        ))
                    })?;
                    tokio::fs::write(&dest, &bytes).await.map_err(|e| {
                        EnolaError::InfrastructureError(format!(
                            "Error escribiendo '{}': {e}",
                            dest.display()
                        ))
                    })?;
                    apply_restrictive_permissions(&dest)?;
                    files_downloaded += 1;
                    if *file == "tokenizer_config.json" {
                        config_downloaded = true;
                    }
                }
                Ok(resp) if resp.status() == reqwest::StatusCode::NOT_FOUND => {
                    // Fichero no presente en este repo: se salta.
                }
                Ok(resp) => {
                    last_error = Some(format!("HTTP {} para '{file}'", resp.status().as_u16()));
                }
                Err(e) => {
                    last_error = Some(e.to_string());
                }
            }
        }

        if !config_downloaded {
            return Err(EnolaError::NotFound(format!(
                "No se pudo descargar 'tokenizer_config.json' del repo '{}'{}",
                req.repo,
                last_error.map(|e| format!(": {e}")).unwrap_or_default()
            )));
        }

        Ok(TokenizerDownloadOutcome {
            destination: req.destination,
            files_downloaded,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// Servidor HTTP mínimo que sirve un subconjunto de ficheros del tokenizer
    /// y devuelve 404 para el resto.
    async fn spawn_tokenizer_server(
        files: std::collections::HashMap<&'static str, &'static [u8]>,
    ) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let files: std::collections::HashMap<String, Vec<u8>> = files
            .into_iter()
            .map(|(k, v)| (format!("/org/repo/resolve/main/{k}"), v.to_vec()))
            .collect();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let files = files.clone();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 8192];
                    let n = socket.read(&mut buf).await.unwrap_or(0);
                    let raw = String::from_utf8_lossy(&buf[..n]).to_string();
                    let path = raw
                        .lines()
                        .next()
                        .and_then(|l| l.split_whitespace().nth(1))
                        .unwrap_or("/")
                        .to_string();
                    match files.get(&path) {
                        Some(body) => {
                            let resp = format!(
                                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                body.len()
                            );
                            let _ = socket.write_all(resp.as_bytes()).await;
                            let _ = socket.write_all(body).await;
                        }
                        None => {
                            let resp = "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                            let _ = socket.write_all(resp.as_bytes()).await;
                        }
                    }
                    let _ = socket.shutdown().await;
                });
            }
        });
        format!("http://127.0.0.1:{}", addr.port())
    }

    #[tokio::test]
    async fn downloads_tokenizer_files_and_skips_absent() {
        let base = spawn_tokenizer_server(
            [
                (
                    "tokenizer_config.json",
                    b"{\"model_type\":\"llama\"}" as &[u8],
                ),
                ("tokenizer.json", b"{\"version\":\"1.0\"}" as &[u8]),
            ]
            .into_iter()
            .collect(),
        )
        .await;
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("tok");

        let d = ReqwestTokenizerDownloader::new_with_base(base);
        let out = d
            .download_tokenizer(TokenizerDownloadRequest {
                repo: "org/repo".into(),
                destination: dest.clone(),
                use_tor: false,
            })
            .await
            .unwrap();

        assert_eq!(out.files_downloaded, 2);
        assert!(dest.join("tokenizer_config.json").exists());
        assert!(dest.join("tokenizer.json").exists());
        // Ficheros no servidos (404) no se materializan.
        assert!(!dest.join("vocab.json").exists());
    }

    #[tokio::test]
    async fn fails_without_tokenizer_config() {
        // Servidor vacío → todo 404 → sin tokenizer_config.json.
        let base = spawn_tokenizer_server(std::collections::HashMap::new()).await;
        let dir = tempfile::tempdir().unwrap();

        let d = ReqwestTokenizerDownloader::new_with_base(base);
        let err = d
            .download_tokenizer(TokenizerDownloadRequest {
                repo: "org/repo".into(),
                destination: dir.path().join("tok"),
                use_tor: false,
            })
            .await
            .unwrap_err();
        assert!(err.to_string().contains("tokenizer_config.json"));
    }

    #[tokio::test]
    async fn rejects_invalid_repo() {
        let d = ReqwestTokenizerDownloader::new_with_base("http://127.0.0.1:1".to_string());
        let err = d
            .download_tokenizer(TokenizerDownloadRequest {
                repo: "not-a-repo".into(),
                destination: std::path::PathBuf::from("/tmp/tok"),
                use_tor: false,
            })
            .await
            .unwrap_err();
        assert!(err.to_string().contains("org/repo"));
    }
}
