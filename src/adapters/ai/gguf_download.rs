//! Fase 11.2 — Adapter de descarga de modelos GGUF (`ai model download`).
//!
//! - Streaming con `reqwest` + `chunk()` (sin cargar el fichero en memoria).
//! - Se descarga a `<destino>.part`. Reanudación solo desde ese `.part`
//!   (`Range: bytes=<len>-` → `206`); si el servidor ignora el rango
//!   (`200 OK`), se reinicia el parcial. Un `.gguf` final previo nunca se
//!   trata como parcial (H-06).
//! - SHA-256 incremental durante la escritura y validación del header GGUF
//!   completo (prefijo + metadata) sobre el `.part`; solo si ambos pasan se
//!   hace `rename` atómico al destino. En fallo se borra el `.part` y el
//!   modelo previo (si existía) queda intacto.
//! - Egress: clearnet por defecto (`build_download_client`), Tor SOCKS5h
//!   forzado con `use_tor` (`build_download_client_via_tor`, R22).
//! - Permisos restrictivos `0640` sobre el fichero final.

use crate::domain::error::{EnolaError, Result};
use crate::ports::ai::{ModelDownloadOutcome, ModelDownloadPort, ModelDownloadRequest};
use reqwest::header::RANGE;
use reqwest::StatusCode;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;

/// Adapter de descarga GGUF sobre HTTP (`reqwest`).
pub struct ReqwestModelDownloader;

impl ReqwestModelDownloader {
    /// Crea el adapter de descarga.
    pub fn new() -> Self {
        Self
    }

    fn build_client(&self, url: &str, use_tor: bool) -> Result<reqwest::Client> {
        if use_tor {
            crate::infrastructure::http::build_download_client_via_tor(url)
        } else {
            crate::infrastructure::http::build_download_client(url)
        }
    }

    fn actionable_error(op: &str, url: &str, use_tor: bool, err: &reqwest::Error) -> EnolaError {
        let route = if use_tor {
            "vía Tor SOCKS5h ([http].tor_socks_proxy)"
        } else {
            "por clearnet"
        };
        if err.is_timeout() {
            return EnolaError::Timeout(format!(
                "La descarga {op} de '{url}' {route} agotó el tiempo de espera. \
                 Aumenta ENOLA_HTTP_DOWNLOAD_TIMEOUT o reintenta (la descarga es reanudable)."
            ));
        }
        if err.is_connect() {
            let hint = if use_tor {
                "Verifica que el daemon Tor está activo y que [http].tor_socks_proxy es alcanzable."
            } else {
                "Verifica la conectividad de red o usa --tor para salir por el proxy Tor."
            };
            return EnolaError::InfrastructureError(format!(
                "No se pudo conectar para la descarga {op} de '{url}' {route}: {err}. {hint}"
            ));
        }
        EnolaError::InfrastructureError(format!(
            "Error de red en la descarga {op} de '{url}' {route}: {err}"
        ))
    }
}

impl Default for ReqwestModelDownloader {
    fn default() -> Self {
        Self::new()
    }
}

async fn file_len(path: &Path) -> Result<u64> {
    match tokio::fs::metadata(path).await {
        Ok(m) => Ok(m.len()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(e) => Err(EnolaError::InfrastructureError(format!(
            "No se pudo leer metadata de '{}': {e}",
            path.display()
        ))),
    }
}

/// Ruta del fichero parcial de una descarga: `<destino>.part` (mismo directorio,
/// así el `rename` final es atómico).
fn part_path(destination: &Path) -> PathBuf {
    let mut name = destination.as_os_str().to_owned();
    name.push(".part");
    PathBuf::from(name)
}

/// Valida el parcial como GGUF completo **antes** de publicarlo: prefijo
/// (magic + versión + contadores) y metadata hasta `general.architecture` /
/// `general.file_type`, con la misma lectura que `read_gguf_metadata`. Así un
/// fichero que luego no se podría leer nunca llega a la biblioteca.
async fn validate_part_header(part: &Path) -> Result<()> {
    use tokio::io::AsyncReadExt;
    let f = tokio::fs::File::open(part).await.map_err(|e| {
        EnolaError::InfrastructureError(format!("No se pudo leer '{}': {e}", part.display()))
    })?;
    let mut buf = Vec::new();
    f.take(crate::domain::ai_model::GGUF_HEADER_READ_LEN as u64)
        .read_to_end(&mut buf)
        .await
        .map_err(|e| {
            EnolaError::InfrastructureError(format!("No se pudo leer '{}': {e}", part.display()))
        })?;
    crate::domain::ai_model::parse_gguf_header(&buf)
        .map(|_| ())
        .map_err(|e| {
            EnolaError::ValidationError(format!(
                "El fichero descargado no es un modelo GGUF válido ({e}); se descartó."
            ))
        })
}

fn apply_restrictive_permissions(path: &Path) -> Result<()> {
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
impl ModelDownloadPort for ReqwestModelDownloader {
    async fn download(&self, req: ModelDownloadRequest) -> Result<ModelDownloadOutcome> {
        if req.destination.as_os_str().is_empty() {
            return Err(EnolaError::ValidationError(
                "El destino de la descarga no puede estar vacío".into(),
            ));
        }
        if let Some(parent) = req.destination.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                EnolaError::InfrastructureError(format!(
                    "No se pudo crear el directorio destino '{}': {e}",
                    parent.display()
                ))
            })?;
        }

        // H-06: el parcial vive en `<fichero>.part`; solo se reanuda desde ahí,
        // nunca contra un `.gguf` final previo (puede ser otro modelo).
        let part = part_path(&req.destination);
        let client = self.build_client(&req.url, req.use_tor)?;
        let existing_len = file_len(&part).await?;

        let mut request = client.get(&req.url);
        if existing_len > 0 {
            request = request.header(RANGE, format!("bytes={existing_len}-"));
        }
        let mut response = request
            .send()
            .await
            .map_err(|e| Self::actionable_error("inicial", &req.url, req.use_tor, &e))?;

        let status = response.status();
        let resumed = status == StatusCode::PARTIAL_CONTENT && existing_len > 0;
        let mut hasher = Sha256::new();

        // Si reanudamos, el hash debe incluir los bytes ya presentes
        // (streaming por bloques: los GGUF pueden pesar varios GiB).
        if resumed {
            let mut prev = tokio::fs::File::open(&part).await.map_err(|e| {
                EnolaError::InfrastructureError(format!(
                    "No se pudo reabrir '{}' para reanudar: {e}",
                    part.display()
                ))
            })?;
            let mut buf = vec![0u8; 65536];
            loop {
                use tokio::io::AsyncReadExt;
                let n = prev.read(&mut buf).await.map_err(|e| {
                    EnolaError::InfrastructureError(format!("Relectura previa: {e}"))
                })?;
                if n == 0 {
                    break;
                }
                hasher.update(&buf[..n]);
            }
        }

        let mut file = if resumed {
            tokio::fs::OpenOptions::new()
                .append(true)
                .open(&part)
                .await
                .map_err(|e| {
                    EnolaError::InfrastructureError(format!(
                        "No se pudo abrir '{}' en modo append: {e}",
                        part.display()
                    ))
                })?
        } else {
            if !status.is_success() {
                return Err(EnolaError::InfrastructureError(format!(
                    "La descarga de '{}' falló con HTTP {}. Comprueba la URL/repo y el fichero (--file).",
                    req.url,
                    status.as_u16()
                )));
            }
            tokio::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&part)
                .await
                .map_err(|e| {
                    EnolaError::InfrastructureError(format!(
                        "No se pudo crear '{}': {e}",
                        part.display()
                    ))
                })?
        };

        let mut written: u64 = if resumed { existing_len } else { 0 };
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|e| Self::actionable_error("streaming", &req.url, req.use_tor, &e))?
        {
            file.write_all(&chunk).await.map_err(|e| {
                EnolaError::InfrastructureError(format!(
                    "Error de escritura en '{}': {e}",
                    part.display()
                ))
            })?;
            hasher.update(&chunk);
            written += chunk.len() as u64;
        }
        file.flush().await.map_err(|e| {
            EnolaError::InfrastructureError(format!("Error al volcar '{}': {e}", part.display()))
        })?;
        drop(file);

        apply_restrictive_permissions(&part)?;

        let sha256 = format!("{:x}", hasher.finalize());
        if let Some(expected) = &req.expected_sha256 {
            let expected = expected.trim();
            if !expected.is_empty() && !sha256.eq_ignore_ascii_case(expected) {
                let _ = std::fs::remove_file(&part);
                return Err(EnolaError::ValidationError(format!(
                    "SHA-256 mismatch: esperado {expected}, obtenido {sha256}. \
                     La descarga se descartó; el modelo previo (si existía) no se tocó."
                )));
            }
        }

        // Un HTML de error no es un modelo: validar el header antes de publicar.
        if let Err(e) = validate_part_header(&part).await {
            let _ = std::fs::remove_file(&part);
            return Err(e);
        }

        tokio::fs::rename(&part, &req.destination)
            .await
            .map_err(|e| {
                EnolaError::InfrastructureError(format!(
                    "No se pudo mover '{}' a '{}': {e}",
                    part.display(),
                    req.destination.display()
                ))
            })?;

        Ok(ModelDownloadOutcome {
            destination: req.destination,
            bytes_written: written,
            sha256,
            resumed,
            via_tor: req.use_tor,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// Cuerpo con prefijo GGUF válido (magic + v3 + 0 tensores + 0 kv) + `payload`.
    fn gguf_body(payload: &[u8]) -> Vec<u8> {
        let mut b = b"GGUF".to_vec();
        b.extend_from_slice(&3u32.to_le_bytes());
        b.extend_from_slice(&0u64.to_le_bytes());
        b.extend_from_slice(&0u64.to_le_bytes());
        b.extend_from_slice(payload);
        b
    }

    /// Servidor HTTP mínimo con soporte de `Range` para tests de descarga.
    async fn spawn_download_server(body: Vec<u8>, status_full: &'static str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let url = format!("http://127.0.0.1:{}", addr.port());
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let body = body.clone();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 8192];
                    let n = socket.read(&mut buf).await.unwrap_or(0);
                    let raw = String::from_utf8_lossy(&buf[..n]).to_string();
                    let range_start = raw
                        .lines()
                        .find(|l| l.to_ascii_lowercase().starts_with("range:"))
                        .and_then(|l| l.split_once('='))
                        .and_then(|(_, v)| v.trim().trim_end_matches('-').parse::<usize>().ok());
                    let (status, payload) = match range_start {
                        Some(start) if start < body.len() => {
                            ("HTTP/1.1 206 Partial Content", body[start..].to_vec())
                        }
                        _ => (status_full, body),
                    };
                    let resp = format!(
                        "{status}\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        payload.len()
                    );
                    let _ = socket.write_all(resp.as_bytes()).await;
                    let _ = socket.write_all(&payload).await;
                    let _ = socket.shutdown().await;
                });
            }
        });
        url
    }

    #[tokio::test]
    async fn download_writes_stream_and_reports_sha256() {
        let body = gguf_body(b"fake model bytes for test");
        let url = spawn_download_server(body.clone(), "HTTP/1.1 200 OK").await;
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("model.gguf");

        let d = ReqwestModelDownloader::new();
        let out = d
            .download(ModelDownloadRequest {
                url,
                destination: dest.clone(),
                expected_sha256: None,
                use_tor: false,
            })
            .await
            .unwrap();

        assert_eq!(out.bytes_written as usize, body.len());
        assert_eq!(std::fs::read(&dest).unwrap(), body);
        assert_eq!(out.sha256.len(), 64);
        assert!(!out.resumed && !out.via_tor);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&dest).unwrap().permissions().mode() & 0o777,
                0o640
            );
        }
    }

    #[tokio::test]
    async fn download_verifies_sha256_and_rejects_mismatch() {
        let body = gguf_body(b"payload");
        let url = spawn_download_server(body.clone(), "HTTP/1.1 200 OK").await;
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("model.gguf");
        let good = format!("{:x}", Sha256::digest(&body));

        let d = ReqwestModelDownloader::new();
        let ok = d
            .download(ModelDownloadRequest {
                url: url.clone(),
                destination: dest.clone(),
                expected_sha256: Some(good),
                use_tor: false,
            })
            .await;
        assert!(ok.is_ok());

        let bad = d
            .download(ModelDownloadRequest {
                url,
                destination: dest.clone(),
                expected_sha256: Some("0".repeat(64)),
                use_tor: false,
            })
            .await;
        assert!(bad.is_err());
        // H-06: el modelo previo válido sigue intacto; solo se descarta el `.part`.
        assert_eq!(std::fs::read(&dest).unwrap(), body);
        assert!(!dir.path().join("model.gguf.part").exists());
    }

    #[tokio::test]
    async fn download_resumes_partial_file_with_range() {
        let body = gguf_body(b"0123456789abcdef");
        let url = spawn_download_server(body.clone(), "HTTP/1.1 200 OK").await;
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("partial.gguf");
        let part = dir.path().join("partial.gguf.part");
        std::fs::write(&part, &body[..30]).unwrap();

        let d = ReqwestModelDownloader::new();
        let out = d
            .download(ModelDownloadRequest {
                url,
                destination: dest.clone(),
                expected_sha256: None,
                use_tor: false,
            })
            .await
            .unwrap();

        assert!(out.resumed, "must resume via Range/206");
        assert_eq!(std::fs::read(&dest).unwrap(), body);
        assert!(!part.exists(), "el .part se renombra al destino final");
        assert_eq!(out.bytes_written as usize, body.len());
    }

    #[tokio::test]
    async fn download_http_error_is_actionable() {
        let url = spawn_download_server(Vec::new(), "HTTP/1.1 404 Not Found").await;
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("missing.gguf");

        let d = ReqwestModelDownloader::new();
        let err = d
            .download(ModelDownloadRequest {
                url,
                destination: dest,
                expected_sha256: None,
                use_tor: false,
            })
            .await
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("404"), "debe indicar el status: {msg}");
    }

    #[tokio::test]
    async fn download_connect_failure_is_actionable() {
        let d = ReqwestModelDownloader::new();
        let err = d
            .download(ModelDownloadRequest {
                url: "http://127.0.0.1:1/never.gguf".to_string(),
                destination: std::path::PathBuf::from("/tmp/enola-never.gguf"),
                expected_sha256: None,
                use_tor: false,
            })
            .await
            .unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("clearnet") || msg.contains("conectar"),
            "error accionable: {msg}"
        );
    }

    #[tokio::test]
    async fn download_never_resumes_against_existing_final_file() {
        // H-06: un `.gguf` previo con el mismo nombre (otro modelo) no es un
        // parcial: no se envía `Range` ni se le pega la cola del nuevo.
        let body = gguf_body(b"new model with a longer payload");
        let url = spawn_download_server(body.clone(), "HTTP/1.1 200 OK").await;
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("model.gguf");
        std::fs::write(&dest, gguf_body(b"old")).unwrap();

        let d = ReqwestModelDownloader::new();
        let out = d
            .download(ModelDownloadRequest {
                url,
                destination: dest.clone(),
                expected_sha256: None,
                use_tor: false,
            })
            .await
            .unwrap();

        assert!(!out.resumed);
        assert_eq!(std::fs::read(&dest).unwrap(), body);
        assert_eq!(out.sha256, format!("{:x}", Sha256::digest(&body)));
    }

    #[tokio::test]
    async fn download_rejects_non_gguf_without_touching_previous_model() {
        // Una página de error HTML no reemplaza el modelo previo.
        let html = b"<html>404 rate limited</html>".to_vec();
        let url = spawn_download_server(html, "HTTP/1.1 200 OK").await;
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("model.gguf");
        let previous = gguf_body(b"previous valid model");
        std::fs::write(&dest, &previous).unwrap();

        let d = ReqwestModelDownloader::new();
        let err = d
            .download(ModelDownloadRequest {
                url,
                destination: dest.clone(),
                expected_sha256: None,
                use_tor: false,
            })
            .await
            .unwrap_err();

        assert!(err.to_string().contains("GGUF"), "{err}");
        assert_eq!(std::fs::read(&dest).unwrap(), previous);
        assert!(!dir.path().join("model.gguf.part").exists());
    }

    #[tokio::test]
    async fn download_rejects_unparseable_metadata_before_publishing() {
        // Prefijo GGUF válido que declara 41 entradas de metadata ausentes:
        // la validación completa debe ocurrir sobre el `.part`, antes del
        // `rename`, para no dejar en la biblioteca un fichero que luego no
        // se puede leer (ni pisar el modelo previo).
        let mut body = b"GGUF".to_vec();
        body.extend_from_slice(&3u32.to_le_bytes());
        body.extend_from_slice(&0u64.to_le_bytes());
        body.extend_from_slice(&41u64.to_le_bytes());
        let url = spawn_download_server(body, "HTTP/1.1 200 OK").await;
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("model.gguf");
        let previous = gguf_body(b"previous valid model");
        std::fs::write(&dest, &previous).unwrap();

        let d = ReqwestModelDownloader::new();
        let err = d
            .download(ModelDownloadRequest {
                url,
                destination: dest.clone(),
                expected_sha256: None,
                use_tor: false,
            })
            .await
            .unwrap_err();

        assert!(err.to_string().contains("GGUF"), "{err}");
        assert_eq!(std::fs::read(&dest).unwrap(), previous);
        assert!(!dir.path().join("model.gguf.part").exists());
    }
}
