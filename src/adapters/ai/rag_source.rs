// Fase 12 / H-09 — Adapter de lectura de fuentes RAG (`RagSourcePort`).
//
// I/O que antes vivía en `application/ai_rag.rs`: canonicalización y recorrido
// de ficheros locales, `git pull` de repos ligados (como usuario invocador,
// check 24) y REST de WordPress (`build_http_client`, SOCKS5h para `.onion`).
// La política (rutas permitidas, extensiones) es de `domain::rag`.

use crate::domain::error::{EnolaError, Result};
use crate::domain::rag;
use crate::ports::rag::{RagSourceFile, RagSourcePort, WordpressPost};
use std::path::{Path, PathBuf};

/// Lee fuentes RAG del sistema de ficheros, git y WordPress.
#[derive(Debug, Default, Clone, Copy)]
pub struct FsRagSource;

impl FsRagSource {
    /// Construye el adapter (sin estado).
    pub fn new() -> Self {
        Self
    }
}

/// Recorrido síncrono de `root` (ver `RagSourcePort::read_tree`).
fn read_tree_blocking(root: &Path, max_file_bytes: u64) -> Result<Vec<RagSourceFile>> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(path) = stack.pop() {
        // Cada entrada se canonicaliza y se comprueba que sigue bajo root.
        let canon = std::fs::canonicalize(&path).map_err(|e| {
            EnolaError::InfrastructureError(format!(
                "No se pudo canonicalizar '{}': {e}",
                path.display()
            ))
        })?;
        if canon != *root && !canon.starts_with(root) {
            tracing::warn!("skip {:?}: escapa del root de ingesta", canon);
            continue;
        }
        if canon.is_dir() {
            let entries = std::fs::read_dir(&canon).map_err(|e| {
                EnolaError::InfrastructureError(format!(
                    "No se pudo leer '{}': {e}",
                    canon.display()
                ))
            })?;
            for entry in entries.flatten() {
                let ep = entry.path();
                if let Some(name) = ep.file_name().and_then(|n| n.to_str()) {
                    if name.starts_with('.') {
                        continue; // .git, .vscode, dotfiles
                    }
                }
                stack.push(ep);
            }
        } else if canon.is_file() && rag::is_supported_rag_file(&canon) {
            match read_limited(&canon, max_file_bytes) {
                Ok(content) => files.push(RagSourceFile {
                    path: canon,
                    content,
                }),
                Err(e) => tracing::warn!("skip {:?}: {e}", canon),
            }
        }
    }
    Ok(files)
}

/// Lee `path` como UTF-8 si no supera `max_bytes`.
fn read_limited(path: &Path, max_bytes: u64) -> Result<String> {
    let meta = std::fs::metadata(path)
        .map_err(|e| EnolaError::InfrastructureError(format!("stat '{}': {e}", path.display())))?;
    if meta.len() > max_bytes {
        return Err(EnolaError::ValidationError(format!(
            "'{}' excede el máximo de {} bytes",
            path.display(),
            max_bytes
        )));
    }
    std::fs::read_to_string(path)
        .map_err(|e| EnolaError::InfrastructureError(format!("leer '{}': {e}", path.display())))
}

#[async_trait::async_trait]
impl RagSourcePort for FsRagSource {
    #[tracing::instrument(skip(self))]
    async fn canonicalize(&self, location: &str) -> Result<PathBuf> {
        std::fs::canonicalize(location).map_err(|e| {
            EnolaError::ValidationError(format!("No se pudo resolver '{location}': {e}"))
        })
    }

    async fn is_git_repo(&self, path: &Path) -> bool {
        path.join(".git").exists()
    }

    #[tracing::instrument(skip(self))]
    async fn read_tree(&self, root: &Path, max_file_bytes: u64) -> Result<Vec<RagSourceFile>> {
        let root = root.to_path_buf();
        tokio::task::spawn_blocking(move || read_tree_blocking(&root, max_file_bytes))
            .await
            .map_err(|e| {
                EnolaError::InfrastructureError(format!("Lectura de la fuente RAG abortada: {e}"))
            })?
    }

    #[tracing::instrument(skip(self))]
    async fn git_pull(&self, repo: &str) -> Result<()> {
        // Proceso bloqueante fuera de los hilos del runtime async.
        let repo_arg = repo.to_string();
        let out = tokio::task::spawn_blocking(move || {
            crate::infrastructure::drop_privs::command_as_invoking_user("git")
                .args(["-C", &repo_arg, "pull", "--ff-only"])
                .output()
        })
        .await
        .map_err(|e| EnolaError::InfrastructureError(format!("git pull en '{repo}': {e}")))?
        .map_err(|e| EnolaError::InfrastructureError(format!("git pull en '{repo}': {e}")))?;
        if !out.status.success() {
            return Err(EnolaError::InfrastructureError(format!(
                "git pull en '{repo}' falló: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        Ok(())
    }

    #[tracing::instrument(skip(self))]
    async fn fetch_wordpress_posts(&self, base_url: &str) -> Result<Vec<WordpressPost>> {
        let url = format!("{base_url}/wp-json/wp/v2/posts?per_page=100&_fields=title,content");
        let client = crate::infrastructure::http::build_http_client(&url)?;
        let resp = client
            .get(&url)
            .send()
            .await
            .map_err(|e| EnolaError::InfrastructureError(format!("WP REST '{url}': {e}")))?;
        if !resp.status().is_success() {
            return Err(EnolaError::InfrastructureError(format!(
                "WP REST '{url}' → HTTP {}",
                resp.status().as_u16()
            )));
        }
        let posts: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| EnolaError::InfrastructureError(format!("WP REST JSON '{url}': {e}")))?;
        Ok(posts
            .as_array()
            .map(|arr| {
                arr.iter()
                    .map(|post| WordpressPost {
                        title: post["title"]["rendered"]
                            .as_str()
                            .unwrap_or("post")
                            .to_string(),
                        body: post["content"]["rendered"]
                            .as_str()
                            .unwrap_or("")
                            .to_string(),
                    })
                    .collect()
            })
            .unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn names(files: &[RagSourceFile]) -> Vec<String> {
        let mut n: Vec<String> = files
            .iter()
            .map(|f| f.path.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        n.sort();
        n
    }

    #[tokio::test]
    async fn read_tree_keeps_supported_files_and_skips_dotfiles() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.md"), "# Hola").unwrap();
        std::fs::write(dir.path().join("b.bin"), vec![0u8; 8]).unwrap();
        std::fs::write(dir.path().join(".hidden.md"), "oculto").unwrap();
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        std::fs::write(dir.path().join(".git").join("x.md"), "git").unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub").join("c.RS"), "fn main() {}").unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();

        let files = FsRagSource::new().read_tree(&root, 1024).await.unwrap();
        assert_eq!(names(&files), vec!["a.md", "c.RS"]);
        let a = files.iter().find(|f| f.path.ends_with("a.md")).unwrap();
        assert_eq!(a.content, "# Hola");
    }

    #[tokio::test]
    async fn read_tree_skips_oversized_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("big.txt"), "x".repeat(64)).unwrap();
        std::fs::write(dir.path().join("ok.txt"), "x").unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        let files = FsRagSource::new().read_tree(&root, 16).await.unwrap();
        assert_eq!(names(&files), vec!["ok.txt"]);
    }

    #[tokio::test]
    async fn read_tree_skips_symlinks_escaping_root() {
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret.md"), "fuera").unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path().join("secret.md"), dir.path().join("link.md"))
            .unwrap();
        std::fs::write(dir.path().join("in.md"), "dentro").unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        let files = FsRagSource::new().read_tree(&root, 1024).await.unwrap();
        assert_eq!(names(&files), vec!["in.md"]);
    }

    #[tokio::test]
    async fn read_tree_accepts_a_single_file_root() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("solo.md"), "uno").unwrap();
        let root = std::fs::canonicalize(dir.path().join("solo.md")).unwrap();
        let files = FsRagSource::new().read_tree(&root, 1024).await.unwrap();
        assert_eq!(names(&files), vec!["solo.md"]);
    }

    #[tokio::test]
    async fn canonicalize_resolves_and_reports_missing_paths() {
        let dir = tempfile::tempdir().unwrap();
        let src = FsRagSource::new();
        let canon = src
            .canonicalize(dir.path().to_str().unwrap())
            .await
            .unwrap();
        assert_eq!(canon, std::fs::canonicalize(dir.path()).unwrap());
        let err = src.canonicalize("/no/existe/xyz").await.unwrap_err();
        assert!(err.to_string().contains("No se pudo resolver"), "{err}");
    }

    #[tokio::test]
    async fn is_git_repo_checks_dot_git() {
        let dir = tempfile::tempdir().unwrap();
        let src = FsRagSource::new();
        assert!(!src.is_git_repo(dir.path()).await);
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        assert!(src.is_git_repo(dir.path()).await);
    }

    #[tokio::test]
    async fn fetch_wordpress_posts_parses_title_and_body() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 4096];
            let n = socket.read(&mut buf).await.unwrap();
            let request = String::from_utf8_lossy(&buf[..n]).to_string();
            let body =
                r#"[{"title":{"rendered":"Hola"},"content":{"rendered":"<p>cuerpo</p>"}},{}]"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(resp.as_bytes()).await.unwrap();
            request
        });
        let posts = FsRagSource::new()
            .fetch_wordpress_posts(&url)
            .await
            .unwrap();
        assert_eq!(
            posts,
            vec![
                WordpressPost {
                    title: "Hola".into(),
                    body: "<p>cuerpo</p>".into()
                },
                WordpressPost {
                    title: "post".into(),
                    body: String::new()
                },
            ]
        );
        let request = server.await.unwrap();
        assert!(
            request.starts_with("GET /wp-json/wp/v2/posts?per_page=100"),
            "{request}"
        );
    }
}
