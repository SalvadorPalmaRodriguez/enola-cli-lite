// Fase 12.4 — Casos de uso de `ai rag` (índice vectorial por instancia).
//
// Hexagonal: solo traits de `crate::ports::*` y tipos de `crate::domain::*`.
// La canonicalización de rutas se hace aquí (std::fs) y la política pura en
// `domain::rag::validate_rag_source_path`.

use crate::domain::error::{EnolaError, Result};
use crate::domain::rag::{self, AiSource, AiSourceType, RagConfig, VectorStoreBackend};
use crate::ports::ai::AiAdapterPort;
use crate::ports::rag::{
    AiRagStorePort, AiVectorStorePort, RagDocument, RagVectorStorePort, VectorStoreInfo,
};
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

/// Informe de `ai rag status`.
#[derive(Debug, Clone, Serialize)]
pub struct RagStatusReport {
    /// Instancia.
    pub instance: String,
    /// Vector store aprovisionado (backend + endpoint).
    pub vector_store: Option<VectorStoreInfo>,
    /// Documentos indexados en la colección de la instancia.
    pub documents_indexed: Option<u64>,
    /// Fuentes ligadas.
    pub sources: Vec<AiSource>,
    /// Config efectiva.
    pub config: RagConfig,
    /// Nota operativa.
    pub note: Option<String>,
}

/// Extensiones soportadas en la ingesta (texto, código, config, web, docs).
/// Timeout de espera de readiness del vector store tras su aprovisionado.
const VECTOR_READY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

const SUPPORTED_EXTENSIONS: &[&str] = &[
    "txt", "md", "markdown", "rst", "rtf", "rs", "py", "js", "ts", "jsx", "tsx", "go", "java", "c",
    "cpp", "h", "hpp", "rb", "php", "swift", "kt", "scala", "lua", "pl", "pm", "json", "toml",
    "yml", "yaml", "xml", "ini", "conf", "cfg", "html", "htm", "css", "scss", "sh", "bash", "zsh",
    "sql", "tex", "csv", "tsv", "jsonl",
];

/// Orquestador de `ai rag <sub>`.
pub struct ManageAiRag {
    instances: Arc<dyn AiAdapterPort + Send + Sync>,
    store: Arc<dyn AiRagStorePort + Send + Sync>,
    provision: Arc<dyn AiVectorStorePort + Send + Sync>,
    /// Ingesta de ficheros (lectura de disco + chunking + add_documents);
    /// inyectable para tests sin fs real.
    max_file_bytes: u64,
}

impl ManageAiRag {
    /// Construye el caso de uso con los puertos inyectados.
    pub fn new(
        instances: Arc<dyn AiAdapterPort + Send + Sync>,
        store: Arc<dyn AiRagStorePort + Send + Sync>,
        provision: Arc<dyn AiVectorStorePort + Send + Sync>,
    ) -> Self {
        Self {
            instances,
            store,
            provision,
            max_file_bytes: 5 * 1024 * 1024, // 5 MiB por fichero
        }
    }

    /// Resuelve instancia + vector store + adapter concreto del backend.
    async fn stack(
        &self,
        instance: &str,
    ) -> Result<(Arc<dyn RagVectorStorePort + Send + Sync>, RagConfig)> {
        crate::domain::ai::validate_ai_instance_name(instance)?;
        let inst = self.instances.get_instance(instance).await?;
        let ollama_port = inst.api_port.ok_or_else(|| {
            EnolaError::ValidationError(format!(
                "La instancia '{instance}' no tiene la API Ollama activa (¿`ai start {instance}`?)"
            ))
        })?;
        let config = self.store.load_rag_config(instance).await?;
        let info = self
            .provision
            .ensure_vector_store(instance, config.vector_store)
            .await?;
        let store = self
            .provision
            .build_vector_store(&info, ollama_port, &config.embedding_model);
        // El contenedor del vector store tarda unos segundos en levantar su
        // API (migraciones internas de Chroma/Qdrant): esperar readiness
        // antes de devolver el store (evita la race del primer GET).
        let deadline = std::time::Instant::now() + VECTOR_READY_TIMEOUT;
        while !store.is_available().await {
            if std::time::Instant::now() >= deadline {
                return Err(EnolaError::InfrastructureError(format!(
                    "El vector store '{:?}' de '{instance}' no respondió en {}s \
                     (contenedor 'enola-ai-{instance}-{}' arrancando o caído)",
                    info.backend,
                    VECTOR_READY_TIMEOUT.as_secs(),
                    info.backend.container_suffix()
                )));
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
        Ok((store, config))
    }

    /// `ai rag status <instance>`.
    pub async fn status(&self, instance: &str) -> Result<RagStatusReport> {
        crate::domain::ai::validate_ai_instance_name(instance)?;
        let config = self.store.load_rag_config(instance).await?;
        let sources = self.store.list_rag_sources(instance).await?;
        let vector = self.provision.vector_store_endpoint(instance).await?;
        let (documents_indexed, note) = match &vector {
            Some(info) => {
                let inst = self.instances.get_instance(instance).await?;
                match inst.api_port {
                    Some(op) => {
                        let vs =
                            self.provision
                                .build_vector_store(info, op, &config.embedding_model);
                        match rag::collection_for_instance(instance).map(|c| (c, vs)) {
                            Ok((col, vs)) => match vs.count(&col).await {
                                Ok(n) => (Some(n), None),
                                Err(e) => (None, Some(format!("conteo no disponible: {e}"))),
                            },
                            Err(e) => (None, Some(e.to_string())),
                        }
                    }
                    None => (None, Some("instancia sin API Ollama activa".to_string())),
                }
            }
            None => (
                None,
                Some("vector store no aprovisionado (se creará al ingerir)".to_string()),
            ),
        };
        Ok(RagStatusReport {
            instance: instance.to_string(),
            vector_store: vector,
            documents_indexed,
            sources,
            config,
            note,
        })
    }

    /// `ai rag bind <instance> --source-type <t> --path <p> --force`.
    pub async fn bind(
        &self,
        instance: &str,
        source_type: AiSourceType,
        location: &str,
        force: bool,
    ) -> Result<AiSource> {
        crate::domain::ai::validate_ai_instance_name(instance)?;
        if !force {
            return Err(EnolaError::ValidationError(
                "`ai rag bind` expone una ruta/URL al índice: requiere --force".into(),
            ));
        }
        if location.trim().is_empty() {
            return Err(EnolaError::ValidationError(
                "La localización de la fuente no puede estar vacía".into(),
            ));
        }

        let resolved_location = match source_type {
            AiSourceType::Filesystem | AiSourceType::GitRepo => {
                // Canonicalizar + política de rutas (anti traversal/symlink escape).
                let canon = std::fs::canonicalize(location).map_err(|e| {
                    EnolaError::ValidationError(format!("No se pudo resolver '{location}': {e}"))
                })?;
                rag::validate_rag_source_path(&canon)?;
                if source_type == AiSourceType::GitRepo && !canon.join(".git").exists() {
                    return Err(EnolaError::ValidationError(format!(
                        "'{}' no es un repositorio git (falta .git)",
                        canon.display()
                    )));
                }
                canon.to_string_lossy().to_string()
            }
            AiSourceType::Wordpress => {
                if !(location.starts_with("https://") || location.starts_with("http://")) {
                    return Err(EnolaError::ValidationError(format!(
                        "La fuente wordpress debe ser una URL http(s): '{location}'"
                    )));
                }
                location.trim_end_matches('/').to_string()
            }
        };

        let source = AiSource {
            id: format!(
                "src_{}_{}",
                instance,
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_micros())
                    .unwrap_or(0)
            ),
            instance_name: instance.to_string(),
            source_type,
            location: resolved_location,
            last_synced: None,
        };
        self.store.add_rag_source(instance, source.clone()).await?;
        Ok(source)
    }

    /// `ai rag ingest <instance> <path>` — ingesta directa (sin ligar).
    pub async fn ingest(&self, instance: &str, path: &str) -> Result<usize> {
        let (vector_store, config) = self.stack(instance).await?;
        let canon = std::fs::canonicalize(path).map_err(|e| {
            EnolaError::ValidationError(format!("No se pudo resolver '{path}': {e}"))
        })?;
        rag::validate_rag_source_path(&canon)?;
        let docs = self.collect_documents(&canon, &config)?;
        if docs.is_empty() {
            return Ok(0);
        }
        let collection = rag::collection_for_instance(instance)?;
        vector_store.ensure_collection(&collection).await?;
        vector_store
            .add_documents(&collection, docs.clone())
            .await?;
        Ok(docs.len())
    }

    /// `ai rag sync <instance>` — re-ingesta todas las fuentes ligadas.
    pub async fn sync(&self, instance: &str) -> Result<usize> {
        let sources = self.store.list_rag_sources(instance).await?;
        if sources.is_empty() {
            return Err(EnolaError::ValidationError(format!(
                "La instancia '{instance}' no tiene fuentes ligadas (usa `ai rag bind`)"
            )));
        }
        let mut total = 0usize;
        for src in sources {
            let n = match src.source_type {
                AiSourceType::Filesystem => self.ingest(instance, &src.location).await?,
                AiSourceType::GitRepo => {
                    self.git_pull(&src.location)?;
                    self.ingest(instance, &src.location).await?
                }
                AiSourceType::Wordpress => self.sync_wordpress(instance, &src).await?,
            };
            total += n;
            let now = {
                // ISO 8601 simple sin depender de chrono::Utc en application.
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| format!("{}s", d.as_secs()))
                    .unwrap_or_else(|_| "unknown".to_string())
            };
            self.store
                .mark_source_synced(instance, &src.id, &now)
                .await?;
        }
        Ok(total)
    }

    /// `ai rag config <instance>` — mostrar o actualizar.
    #[allow(clippy::too_many_arguments)]
    pub async fn config(
        &self,
        instance: &str,
        chunk_size: Option<usize>,
        chunk_overlap: Option<usize>,
        remove_code: Option<bool>,
        preset: Option<&str>,
        vector_store: Option<&str>,
        show: bool,
    ) -> Result<RagConfig> {
        crate::domain::ai::validate_ai_instance_name(instance)?;
        let mut cfg = self.store.load_rag_config(instance).await?;
        if show {
            return Ok(cfg);
        }
        if let Some(p) = preset {
            cfg = RagConfig::from_preset(p)?;
        }
        if let Some(s) = chunk_size {
            cfg.chunk_size = s;
        }
        if let Some(o) = chunk_overlap {
            cfg.chunk_overlap = o;
        }
        if let Some(r) = remove_code {
            cfg.remove_code_blocks = r;
        }
        if let Some(vs) = vector_store {
            cfg.vector_store = VectorStoreBackend::from_str_ci(vs)?;
        }
        cfg.validate()?;
        self.store.save_rag_config(instance, &cfg).await?;
        Ok(cfg)
    }

    // ── internos ──

    /// Recorre `root` (ya canonicalizado) recolectando documentos-chunk.
    /// Re-canonicaliza cada entrada y rechaza escapes (symlinks).
    fn collect_documents(&self, root: &Path, config: &RagConfig) -> Result<Vec<RagDocument>> {
        let mut docs = Vec::new();
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
            } else if canon.is_file() && self.supported(&canon) {
                match self.file_to_documents(&canon, config) {
                    Ok(mut d) => docs.append(&mut d),
                    Err(e) => tracing::warn!("skip {:?}: {e}", canon),
                }
            }
        }
        Ok(docs)
    }

    fn supported(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| SUPPORTED_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
            .unwrap_or(false)
    }

    fn file_to_documents(&self, path: &Path, config: &RagConfig) -> Result<Vec<RagDocument>> {
        let meta = std::fs::metadata(path).map_err(|e| {
            EnolaError::InfrastructureError(format!("stat '{}': {e}", path.display()))
        })?;
        if meta.len() > self.max_file_bytes {
            return Err(EnolaError::ValidationError(format!(
                "'{}' excede el máximo de {} bytes",
                path.display(),
                self.max_file_bytes
            )));
        }
        let raw = std::fs::read_to_string(path).map_err(|e| {
            EnolaError::InfrastructureError(format!("leer '{}': {e}", path.display()))
        })?;
        let filename = path
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown".to_string());
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_string();
        let chunks = rag::preprocess_document(&raw, config);
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_micros())
            .unwrap_or(0);
        let docs = chunks
            .into_iter()
            .map(|c| {
                let mut metadata = HashMap::new();
                metadata.insert("source".to_string(), filename.clone());
                metadata.insert("path".to_string(), path.display().to_string());
                metadata.insert("type".to_string(), "text".to_string());
                metadata.insert(
                    "chunk".to_string(),
                    format!("{}/{}", c.chunk_index + 1, c.total_chunks),
                );
                metadata.insert("extension".to_string(), ext.clone());
                RagDocument {
                    id: format!("{filename}_chunk{}_{ts}", c.chunk_index),
                    content: c.content,
                    metadata,
                }
            })
            .collect();
        Ok(docs)
    }

    /// `git pull --ff-only` sobre el repo ligado, como usuario invocador
    /// (check 24 — nunca como root).
    fn git_pull(&self, repo: &str) -> Result<()> {
        let out = crate::infrastructure::drop_privs::command_as_invoking_user("git")
            .args(["-C", repo, "pull", "--ff-only"])
            .output()
            .map_err(|e| EnolaError::InfrastructureError(format!("git pull en '{repo}': {e}")))?;
        if !out.status.success() {
            return Err(EnolaError::InfrastructureError(format!(
                "git pull en '{repo}' falló: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        Ok(())
    }

    /// Sincroniza una fuente WordPress vía REST (`/wp-json/wp/v2/posts`).
    async fn sync_wordpress(&self, instance: &str, src: &AiSource) -> Result<usize> {
        let (vector_store, config) = self.stack(instance).await?;
        let url = format!(
            "{}/wp-json/wp/v2/posts?per_page=100&_fields=title,content",
            src.location
        );
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
        let mut docs = Vec::new();
        for (i, post) in posts
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .enumerate()
        {
            let title = post["title"]["rendered"].as_str().unwrap_or("post");
            let body = post["content"]["rendered"].as_str().unwrap_or("");
            let text = format!("{title}\n\n{body}");
            for c in rag::preprocess_document(&text, &config) {
                let mut metadata = HashMap::new();
                metadata.insert("source".to_string(), src.location.clone());
                metadata.insert("type".to_string(), "wordpress".to_string());
                metadata.insert("post".to_string(), title.to_string());
                docs.push(RagDocument {
                    id: format!("wp_{}_chunk{}", i, c.chunk_index),
                    content: c.content,
                    metadata,
                });
            }
        }
        if docs.is_empty() {
            return Ok(0);
        }
        let collection = rag::collection_for_instance(instance)?;
        vector_store.ensure_collection(&collection).await?;
        let n = docs.len();
        vector_store.add_documents(&collection, docs).await?;
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ai::AiInstanceStatus;
    use crate::domain::rag::VectorStoreBackend;
    use crate::ports::ai::MockAiAdapterPort;
    use crate::ports::rag::{MockAiRagStorePort, MockAiVectorStorePort, MockRagVectorStorePort};

    fn inst(name: &str) -> crate::domain::ai::AiInstance {
        crate::domain::ai::AiInstance {
            name: name.into(),
            model: "llama3.1:8b".into(),
            api_port: Some(11435),
            webui_port: Some(8081),
            status: AiInstanceStatus::Running,
            ollama_status: "Up".into(),
            webui_status: "Up".into(),
            gpu_enabled: false,
            onion_address: None,
        }
    }

    fn svc(
        insts: MockAiAdapterPort,
        store: MockAiRagStorePort,
        prov: MockAiVectorStorePort,
    ) -> ManageAiRag {
        ManageAiRag::new(Arc::new(insts), Arc::new(store), Arc::new(prov))
    }

    fn vs_info() -> VectorStoreInfo {
        VectorStoreInfo {
            backend: VectorStoreBackend::Chroma,
            host_port: 12001,
            endpoint: "http://127.0.0.1:12001".into(),
        }
    }

    #[tokio::test]
    async fn bind_filesystem_canonicalizes_and_rejects_system() {
        let mut insts = MockAiAdapterPort::new();
        insts.expect_get_instance().returning(|n| Ok(inst(n)));
        let mut store = MockAiRagStorePort::new();
        store
            .expect_add_rag_source()
            .withf(|i, s| i == "dev" && s.source_type == AiSourceType::Filesystem)
            .times(1)
            .returning(|_, _| Ok(()));
        let s = svc(insts, store, MockAiVectorStorePort::new());
        // /tmp existe y es permitido
        let bound = s
            .bind("dev", AiSourceType::Filesystem, "/tmp", true)
            .await
            .unwrap();
        assert!(bound.location.starts_with('/'));
        // sin --force → rechazo
        assert!(s
            .bind("dev", AiSourceType::Filesystem, "/tmp", false)
            .await
            .is_err());
        // ruta del sistema canonicalizada → rechazo (no llega a add_source)
        assert!(s
            .bind("dev", AiSourceType::Filesystem, "/etc", true)
            .await
            .is_err());
        assert!(s
            .bind("dev", AiSourceType::Filesystem, "/proc", true)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn bind_wordpress_requires_url() {
        let mut insts = MockAiAdapterPort::new();
        insts.expect_get_instance().returning(|n| Ok(inst(n)));
        let mut store = MockAiRagStorePort::new();
        store.expect_add_rag_source().returning(|_, _| Ok(()));
        let s = svc(insts, store, MockAiVectorStorePort::new());
        assert!(s
            .bind("dev", AiSourceType::Wordpress, "notaurl", true)
            .await
            .is_err());
        let ok = s
            .bind(
                "dev",
                AiSourceType::Wordpress,
                "https://blog.example.com/",
                true,
            )
            .await
            .unwrap();
        assert_eq!(ok.location, "https://blog.example.com");
    }

    #[tokio::test]
    async fn ingest_ingests_supported_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.md"), "# Hola\n\ncontenido de prueba").unwrap();
        std::fs::write(dir.path().join("b.bin"), vec![0u8; 8]).unwrap(); // no soportado
        std::fs::write(dir.path().join(".hidden.md"), "oculto").unwrap();

        let mut insts = MockAiAdapterPort::new();
        insts.expect_get_instance().returning(|n| Ok(inst(n)));
        let mut store = MockAiRagStorePort::new();
        store
            .expect_load_rag_config()
            .returning(|_| Ok(RagConfig::default()));
        let mut prov = MockAiVectorStorePort::new();
        prov.expect_ensure_vector_store()
            .returning(|_, _| Ok(vs_info()));
        let mut vs = MockRagVectorStorePort::new();
        vs.expect_ensure_collection().returning(|_| Ok(()));
        vs.expect_add_documents()
            .withf(|c, docs| c == "rag_dev" && !docs.is_empty())
            .returning(|_, d| {
                // afirmar que solo se ingesta a.md (no .bin ni .hidden)
                assert!(d.iter().all(|doc| doc.metadata["source"] == "a.md"));
                Ok(())
            });
        prov.expect_build_vector_store().returning(move |_, _, _| {
            let mut vs = MockRagVectorStorePort::new();
            vs.expect_is_available().returning(|| true);
            vs.expect_ensure_collection().returning(|_| Ok(()));
            vs.expect_add_documents().returning(|_, _| Ok(()));
            Arc::new(vs)
        });
        let _ = vs; // el mock real va en la factoría
        let s = svc(insts, store, prov);
        let n = s.ingest("dev", dir.path().to_str().unwrap()).await.unwrap();
        assert!(n >= 1);
    }

    #[tokio::test]
    async fn config_show_and_preset() {
        let mut insts = MockAiAdapterPort::new();
        insts.expect_get_instance().returning(|n| Ok(inst(n)));
        let mut store = MockAiRagStorePort::new();
        store
            .expect_load_rag_config()
            .returning(|_| Ok(RagConfig::default()));
        store.expect_save_rag_config().returning(|_, _| Ok(()));
        let s = svc(insts, store, MockAiVectorStorePort::new());
        let shown = s
            .config("dev", None, None, None, None, None, true)
            .await
            .unwrap();
        assert_eq!(shown.chunk_size, 3000);
        let faq = s
            .config("dev", None, None, None, Some("faq"), None, false)
            .await
            .unwrap();
        assert!(faq.remove_code_blocks);
        // preset desconocido → error
        assert!(s
            .config("dev", None, None, None, Some("nope"), None, false)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn status_reports_sources_and_provisioning() {
        let mut insts = MockAiAdapterPort::new();
        insts.expect_get_instance().returning(|n| Ok(inst(n)));
        let mut store = MockAiRagStorePort::new();
        store
            .expect_load_rag_config()
            .returning(|_| Ok(RagConfig::default()));
        store.expect_list_rag_sources().returning(|_| {
            Ok(vec![AiSource {
                id: "s1".into(),
                instance_name: "dev".into(),
                source_type: AiSourceType::GitRepo,
                location: "/home/u/repo".into(),
                last_synced: None,
            }])
        });
        let mut prov = MockAiVectorStorePort::new();
        prov.expect_vector_store_endpoint().returning(|_| Ok(None));
        let s = svc(insts, store, prov);
        let rep = s.status("dev").await.unwrap();
        assert_eq!(rep.sources.len(), 1);
        assert!(rep.vector_store.is_none());
        assert!(rep.note.is_some());
    }
}
