// Fase 12.4 — Casos de uso de `ai rag` (índice vectorial por instancia).
//
// Hexagonal: solo traits de `crate::ports::*` y tipos de `crate::domain::*`.
// El I/O de las fuentes (ficheros, git, WordPress) va por `RagSourcePort` y la
// política pura por `domain::rag` (`validate_rag_source_path`, extensiones).

use crate::domain::error::{EnolaError, Result};
use crate::domain::rag::{
    self, AiSource, AiSourceType, EmbeddingMode, RagConfig, VectorStoreBackend,
};
use crate::ports::ai::AiAdapterPort;
use crate::ports::rag::{
    AddDocumentsOutcome, AiRagStorePort, AiVectorStorePort, RagDocument, RagSourceFile,
    RagSourcePort, RagVectorStorePort, VectorStoreInfo,
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

/// Timeout de espera de readiness del vector store tras su aprovisionado.
const VECTOR_READY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// Reintentos máximos ofrecidos cuando Ollama no devuelve embeddings (D6).
pub(crate) const MAX_EMBED_RETRIES: u32 = 3;

/// Decisión cuando Ollama no devuelve embeddings durante una ingesta (D6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbeddingFallbackDecision {
    /// Volver a intentarlo con Ollama.
    Retry,
    /// Indexar con embeddings por hash (la colección queda marcada `hash`).
    UseHash,
    /// Abortar sin indexar nada.
    Cancel,
}

/// Decide qué hacer si Ollama no responde (inyectable: TTY real o tests).
pub trait EmbeddingFallbackGate: Send + Sync {
    /// `attempt` = nº de fallos consecutivos (1..); `Retry` solo vale hasta
    /// [`MAX_EMBED_RETRIES`].
    fn decide(&self, detail: &str, attempt: u32) -> EmbeddingFallbackDecision;
}

/// Gate sin interacción: siempre cancela (MCP, scripts, sin TTY) → falla cerrado.
pub struct FailClosedEmbeddingGate;

impl EmbeddingFallbackGate for FailClosedEmbeddingGate {
    fn decide(&self, _detail: &str, _attempt: u32) -> EmbeddingFallbackDecision {
        EmbeddingFallbackDecision::Cancel
    }
}

/// Gate interactivo: `[r]eintentar / [h]ash / [C]ancelar` en TTY; sin TTY cancela.
pub struct TtyEmbeddingFallbackGate;

impl EmbeddingFallbackGate for TtyEmbeddingFallbackGate {
    fn decide(&self, detail: &str, attempt: u32) -> EmbeddingFallbackDecision {
        #[allow(unsafe_code)]
        let is_tty = unsafe { libc::isatty(libc::STDIN_FILENO) != 0 };
        if !is_tty {
            return EmbeddingFallbackDecision::Cancel;
        }
        let can_retry = attempt <= MAX_EMBED_RETRIES;
        eprintln!("\n⚠️  {detail}.");
        eprintln!(
            "   [h]ash = búsqueda NO semántica; la colección queda marcada `hash` \
             y no se podrá mezclar con embeddings de Ollama."
        );
        if can_retry {
            eprint!(
                "¿Qué hacer? [r]eintentar ({attempt}/{MAX_EMBED_RETRIES}) / [h]ash / [C]ancelar: "
            );
        } else {
            eprint!("¿Qué hacer? [h]ash / [C]ancelar: ");
        }
        let mut line = String::new();
        if std::io::stdin().read_line(&mut line).is_err() {
            return EmbeddingFallbackDecision::Cancel;
        }
        parse_fallback_answer(&line, can_retry)
    }
}

/// Interpreta la respuesta del prompt de [`TtyEmbeddingFallbackGate`]
/// (por defecto, cancelar).
pub(crate) fn parse_fallback_answer(input: &str, can_retry: bool) -> EmbeddingFallbackDecision {
    match input.trim().to_lowercase().as_str() {
        "r" | "reintentar" | "retry" if can_retry => EmbeddingFallbackDecision::Retry,
        "h" | "hash" => EmbeddingFallbackDecision::UseHash,
        _ => EmbeddingFallbackDecision::Cancel,
    }
}

/// Orquestador de `ai rag <sub>`.
pub struct ManageAiRag {
    instances: Arc<dyn AiAdapterPort + Send + Sync>,
    store: Arc<dyn AiRagStorePort + Send + Sync>,
    provision: Arc<dyn AiVectorStorePort + Send + Sync>,
    /// Lectura de las fuentes (ficheros locales, git, WordPress) — H-09.
    sources: Arc<dyn RagSourcePort + Send + Sync>,
    /// Tamaño máximo por fichero ingerido.
    max_file_bytes: u64,
    /// Qué hacer si Ollama no devuelve embeddings (D6). Por defecto, falla cerrado.
    fallback_gate: Arc<dyn EmbeddingFallbackGate>,
    /// `--embedding-fallback hash`: indexar por hash sin consultar Ollama.
    hash_fallback: bool,
    /// Espera entre reintentos de embeddings.
    retry_delay: std::time::Duration,
}

impl ManageAiRag {
    /// Construye el caso de uso con los puertos inyectados.
    pub fn new(
        instances: Arc<dyn AiAdapterPort + Send + Sync>,
        store: Arc<dyn AiRagStorePort + Send + Sync>,
        provision: Arc<dyn AiVectorStorePort + Send + Sync>,
        sources: Arc<dyn RagSourcePort + Send + Sync>,
    ) -> Self {
        Self {
            instances,
            store,
            provision,
            sources,
            max_file_bytes: 5 * 1024 * 1024, // 5 MiB por fichero
            fallback_gate: Arc::new(FailClosedEmbeddingGate),
            hash_fallback: false,
            retry_delay: std::time::Duration::from_secs(2),
        }
    }

    /// Gate a usar si Ollama no devuelve embeddings (CLI: TTY; resto: falla cerrado).
    pub fn with_fallback_gate(mut self, gate: Arc<dyn EmbeddingFallbackGate>) -> Self {
        self.fallback_gate = gate;
        self
    }

    /// `--embedding-fallback hash`: indexar por hash por elección explícita.
    pub fn with_hash_fallback(mut self, enabled: bool) -> Self {
        self.hash_fallback = enabled;
        self
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
                let canon = self.sources.canonicalize(location).await?;
                rag::validate_rag_source_path(&canon)?;
                if source_type == AiSourceType::GitRepo && !self.sources.is_git_repo(&canon).await {
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
        let canon = self.sources.canonicalize(path).await?;
        rag::validate_rag_source_path(&canon)?;
        let docs = self.collect_documents(&canon, &config).await?;
        if docs.is_empty() {
            return Ok(0);
        }
        let collection = rag::collection_for_instance(instance)?;
        vector_store.ensure_collection(&collection).await?;
        let n = docs.len();
        self.add_with_policy(instance, vector_store.as_ref(), config, &collection, docs)
            .await?;
        Ok(n)
    }

    /// `ai rag ingest-docs <instance>` — (re)indexa en `enola_docs` la
    /// documentación embebida en el binario (`DOC_CATALOG`, la fuente de
    /// `docs_search`), para `docs_lookup` (H-12). Nunca toca `rag_<instancia>`.
    ///
    /// Reconstruye la colección (Q8): olvida la vía registrada, borra la
    /// colección y la indexa entera con la política D6. Así no quedan chunks
    /// de documentos retirados tras actualizar el binario y se puede cambiar
    /// de vía (hash ↔ Ollama). Si la indexación falla, la colección queda
    /// vacía y `docs_lookup` degrada sin error.
    pub async fn ingest_docs(&self, instance: &str) -> Result<usize> {
        let (vector_store, mut config) = self.stack(instance).await?;
        let collection = rag::DOCS_COLLECTION;
        let docs = catalog_documents();
        if docs.is_empty() {
            return Ok(0);
        }
        let key = rag::embedding_kind_key(config.vector_store, collection);
        if config.embedding_kinds.remove(&key).is_some() {
            self.store.save_rag_config(instance, &config).await?;
        }
        match vector_store.delete_collection(collection).await {
            Ok(()) | Err(EnolaError::NotFound(_)) => {}
            Err(e) => return Err(e),
        }
        vector_store.ensure_collection(collection).await?;
        let n = docs.len();
        self.add_with_policy(instance, vector_store.as_ref(), config, collection, docs)
            .await?;
        Ok(n)
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
                    self.sources.git_pull(&src.location).await?;
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
            // El preset reinicia el chunking, no el registro de vías (D6).
            let kinds = std::mem::take(&mut cfg.embedding_kinds);
            cfg = RagConfig::from_preset(p)?;
            cfg.embedding_kinds = kinds;
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

    /// Lee los ficheros admitidos bajo `root` (ya canonicalizado) y los trocea.
    async fn collect_documents(&self, root: &Path, config: &RagConfig) -> Result<Vec<RagDocument>> {
        let files = self.sources.read_tree(root, self.max_file_bytes).await?;
        Ok(files
            .iter()
            .flat_map(|f| file_to_documents(f, config))
            .collect())
    }

    /// Sincroniza una fuente WordPress vía REST (`/wp-json/wp/v2/posts`).
    async fn sync_wordpress(&self, instance: &str, src: &AiSource) -> Result<usize> {
        let (vector_store, config) = self.stack(instance).await?;
        let posts = self.sources.fetch_wordpress_posts(&src.location).await?;
        let mut docs = Vec::new();
        for (i, post) in posts.iter().enumerate() {
            let title = post.title.as_str();
            let text = format!("{title}\n\n{}", post.body);
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
        self.add_with_policy(instance, vector_store.as_ref(), config, &collection, docs)
            .await?;
        Ok(n)
    }

    /// Indexa `docs` aplicando D6: vía Ollama por defecto (o hash si se pidió),
    /// sin mezclar con la vía ya registrada para la colección; si Ollama no
    /// responde, el gate decide (reintentar / hash / cancelar → error).
    async fn add_with_policy(
        &self,
        instance: &str,
        vector_store: &(dyn RagVectorStorePort + Send + Sync),
        config: RagConfig,
        collection: &str,
        docs: Vec<RagDocument>,
    ) -> Result<()> {
        let mut config = config;
        let key = rag::embedding_kind_key(config.vector_store, collection);
        let stored = config.embedding_kinds.get(&key).cloned();
        let mut mode = if self.hash_fallback {
            EmbeddingMode::Hash
        } else {
            EmbeddingMode::Ollama
        };
        let mut failures = 0u32;
        loop {
            let label = rag::embedding_kind_label(mode, &config.embedding_model);
            rag::check_embedding_compat(stored.as_deref(), &label, collection)?;
            match vector_store
                .add_documents(collection, docs.clone(), mode)
                .await?
            {
                AddDocumentsOutcome::Added => {
                    if stored.is_none() {
                        config.embedding_kinds.insert(key, label);
                        self.store.save_rag_config(instance, &config).await?;
                    }
                    return Ok(());
                }
                AddDocumentsOutcome::EmbeddingsUnavailable(detail) => {
                    failures += 1;
                    match self.fallback_gate.decide(&detail, failures) {
                        EmbeddingFallbackDecision::Retry if failures <= MAX_EMBED_RETRIES => {
                            tokio::time::sleep(self.retry_delay).await;
                        }
                        EmbeddingFallbackDecision::UseHash => mode = EmbeddingMode::Hash,
                        _ => {
                            return Err(EnolaError::InfrastructureError(format!(
                                "{detail}. No se indexó nada. Opciones: `ai start {instance}` \
                                 y reintentar, o `--embedding-fallback hash` (búsqueda NO \
                                 semántica; la colección queda marcada `hash` y no admite \
                                 mezclar con embeddings de Ollama)."
                            )))
                        }
                    }
                }
            }
        }
    }
}

/// Trocea un fichero leído de una fuente local en documentos para el índice.
fn file_to_documents(file: &RagSourceFile, config: &RagConfig) -> Vec<RagDocument> {
    let path = &file.path;
    let filename = path
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_string();
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_micros())
        .unwrap_or(0);
    rag::preprocess_document(&file.content, config)
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
        .collect()
}

/// Documentación embebida (`DOC_CATALOG`) troceada para `enola_docs`.
///
/// Usa el preset por defecto (no la config RAG del usuario, pensada para su
/// corpus): los bloques de código de la documentación son los comandos.
/// Ids deterministas `enola://docs/<path>#<n>`: reingestar no duplica.
fn catalog_documents() -> Vec<RagDocument> {
    let config = RagConfig::default();
    crate::domain::docs::DOC_CATALOG
        .iter()
        .flat_map(|doc| {
            rag::preprocess_document(doc.content, &config)
                .into_iter()
                .map(move |c| {
                    let mut metadata = HashMap::new();
                    metadata.insert("source".to_string(), "enola-docs".to_string());
                    metadata.insert("path".to_string(), doc.path.to_string());
                    metadata.insert("type".to_string(), "docs".to_string());
                    metadata.insert(
                        "chunk".to_string(),
                        format!("{}/{}", c.chunk_index + 1, c.total_chunks),
                    );
                    metadata.insert("version".to_string(), env!("CARGO_PKG_VERSION").to_string());
                    RagDocument {
                        id: format!("{}#{}", doc.uri(), c.chunk_index),
                        content: c.content,
                        metadata,
                    }
                })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ai::AiInstanceStatus;
    use crate::domain::rag::VectorStoreBackend;
    use crate::ports::ai::MockAiAdapterPort;
    use crate::ports::rag::{
        MockAiRagStorePort, MockAiVectorStorePort, MockRagSourcePort, MockRagVectorStorePort,
    };
    use std::path::PathBuf;

    /// Ruta de fuente ficticia: el disco lo simula `MockRagSourcePort`.
    const SRC: &str = "/tmp/rag-src";

    /// Fuente simulada: canonicalización identidad, sin `.git` y un único
    /// fichero `a.md` bajo el root pedido.
    fn sources() -> MockRagSourcePort {
        let mut m = MockRagSourcePort::new();
        m.expect_canonicalize().returning(|l| Ok(PathBuf::from(l)));
        m.expect_is_git_repo().returning(|_| false);
        m.expect_read_tree().returning(|root, _| {
            Ok(vec![RagSourceFile {
                path: root.join("a.md"),
                content: "# Hola\n\ncontenido de prueba".into(),
            }])
        });
        m
    }

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
        svc_with(insts, store, prov, sources())
    }

    fn svc_with(
        insts: MockAiAdapterPort,
        store: MockAiRagStorePort,
        prov: MockAiVectorStorePort,
        src: MockRagSourcePort,
    ) -> ManageAiRag {
        ManageAiRag::new(
            Arc::new(insts),
            Arc::new(store),
            Arc::new(prov),
            Arc::new(src),
        )
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
    async fn ingest_indexes_files_read_from_source() {
        // El filtrado (extensiones, dotfiles, symlinks) lo prueba el adapter
        // `FsRagSource`; aquí, que se ingiere lo que devuelve el port.
        let mut src = MockRagSourcePort::new();
        src.expect_canonicalize()
            .withf(|l| l == SRC)
            .returning(|l| Ok(PathBuf::from(l)));
        src.expect_read_tree()
            .withf(|root, max| root == Path::new(SRC) && *max == 5 * 1024 * 1024)
            .times(1)
            .returning(|root, _| {
                Ok(vec![RagSourceFile {
                    path: root.join("a.md"),
                    content: "# Hola\n\ncontenido de prueba".into(),
                }])
            });

        let mut insts = MockAiAdapterPort::new();
        insts.expect_get_instance().returning(|n| Ok(inst(n)));
        let mut store = MockAiRagStorePort::new();
        store
            .expect_load_rag_config()
            .returning(|_| Ok(RagConfig::default()));
        let mut prov = MockAiVectorStorePort::new();
        prov.expect_ensure_vector_store()
            .returning(|_, _| Ok(vs_info()));
        store.expect_save_rag_config().returning(|_, _| Ok(()));
        prov.expect_build_vector_store().returning(move |_, _, _| {
            let mut vs = MockRagVectorStorePort::new();
            vs.expect_is_available().returning(|| true);
            vs.expect_ensure_collection().returning(|_| Ok(()));
            vs.expect_add_documents()
                .withf(|c, docs, _| c == "rag_dev" && !docs.is_empty())
                .returning(|_, d, _| {
                    // afirmar que solo se ingesta a.md (no .bin ni .hidden)
                    assert!(d.iter().all(|doc| doc.metadata["source"] == "a.md"));
                    Ok(AddDocumentsOutcome::Added)
                });
            Arc::new(vs)
        });
        let s = svc_with(insts, store, prov, src);
        let n = s.ingest("dev", SRC).await.unwrap();
        assert!(n >= 1);
    }

    #[tokio::test]
    async fn bind_git_repo_requires_dot_git() {
        let mut insts = MockAiAdapterPort::new();
        insts.expect_get_instance().returning(|n| Ok(inst(n)));
        let mut store = MockAiRagStorePort::new();
        store.expect_add_rag_source().times(0);
        let s = svc(insts, store, MockAiVectorStorePort::new());
        let err = s
            .bind("dev", AiSourceType::GitRepo, SRC, true)
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("no es un repositorio git"),
            "{err}"
        );
    }

    /// Stack de ingesta que acepta cualquier documento y cuenta los añadidos.
    fn ingest_stack(added: Arc<std::sync::Mutex<Vec<RagDocument>>>) -> MockAiVectorStorePort {
        let mut prov = MockAiVectorStorePort::new();
        prov.expect_ensure_vector_store()
            .returning(|_, _| Ok(vs_info()));
        prov.expect_build_vector_store().returning(move |_, _, _| {
            let mut vs = MockRagVectorStorePort::new();
            vs.expect_is_available().returning(|| true);
            vs.expect_ensure_collection().returning(|_| Ok(()));
            let added = added.clone();
            vs.expect_add_documents().returning(move |_, d, _| {
                added.lock().unwrap().extend(d);
                Ok(AddDocumentsOutcome::Added)
            });
            Arc::new(vs)
        });
        prov
    }

    fn store_with_sources(sources: Vec<AiSource>) -> MockAiRagStorePort {
        let mut store = MockAiRagStorePort::new();
        store
            .expect_load_rag_config()
            .returning(|_| Ok(RagConfig::default()));
        store.expect_save_rag_config().returning(|_, _| Ok(()));
        store
            .expect_list_rag_sources()
            .returning(move |_| Ok(sources.clone()));
        store
            .expect_mark_source_synced()
            .times(1)
            .returning(|_, _, _| Ok(()));
        store
    }

    fn source(t: AiSourceType, location: &str) -> AiSource {
        AiSource {
            id: "src_dev_1".into(),
            instance_name: "dev".into(),
            source_type: t,
            location: location.into(),
            last_synced: None,
        }
    }

    #[tokio::test]
    async fn sync_git_repo_pulls_before_ingesting() {
        let mut insts = MockAiAdapterPort::new();
        insts.expect_get_instance().returning(|n| Ok(inst(n)));
        let mut src = sources();
        src.expect_git_pull()
            .withf(|r| r == SRC)
            .times(1)
            .returning(|_| Ok(()));
        let added = Arc::new(std::sync::Mutex::new(Vec::new()));
        let s = svc_with(
            insts,
            store_with_sources(vec![source(AiSourceType::GitRepo, SRC)]),
            ingest_stack(added.clone()),
            src,
        );
        assert!(s.sync("dev").await.unwrap() >= 1);
        assert!(added
            .lock()
            .unwrap()
            .iter()
            .all(|d| d.metadata["source"] == "a.md"));
    }

    #[tokio::test]
    async fn sync_git_repo_stops_when_pull_fails() {
        let mut insts = MockAiAdapterPort::new();
        insts.expect_get_instance().returning(|n| Ok(inst(n)));
        let mut src = MockRagSourcePort::new();
        src.expect_git_pull().returning(|r| {
            Err(EnolaError::InfrastructureError(format!(
                "git pull en '{r}' falló: conflicto"
            )))
        });
        src.expect_read_tree().times(0);
        let mut store = MockAiRagStorePort::new();
        store
            .expect_list_rag_sources()
            .returning(|_| Ok(vec![source(AiSourceType::GitRepo, SRC)]));
        store.expect_mark_source_synced().times(0);
        let s = svc_with(insts, store, MockAiVectorStorePort::new(), src);
        let err = s.sync("dev").await.unwrap_err();
        assert!(err.to_string().contains("git pull"), "{err}");
    }

    #[tokio::test]
    async fn sync_wordpress_indexes_posts_from_source() {
        let mut insts = MockAiAdapterPort::new();
        insts.expect_get_instance().returning(|n| Ok(inst(n)));
        let mut src = MockRagSourcePort::new();
        src.expect_fetch_wordpress_posts()
            .withf(|u| u == "https://blog.example.com")
            .returning(|_| {
                Ok(vec![crate::ports::rag::WordpressPost {
                    title: "Hola".into(),
                    body: "<p>cuerpo del post</p>".into(),
                }])
            });
        let added = Arc::new(std::sync::Mutex::new(Vec::new()));
        let s = svc_with(
            insts,
            store_with_sources(vec![source(
                AiSourceType::Wordpress,
                "https://blog.example.com",
            )]),
            ingest_stack(added.clone()),
            src,
        );
        assert!(s.sync("dev").await.unwrap() >= 1);
        let added = added.lock().unwrap();
        assert!(added
            .iter()
            .all(|d| d.metadata["type"] == "wordpress" && d.metadata["post"] == "Hola"));
        assert!(added[0].id.starts_with("wp_0_chunk"));
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

    // ── D6: Ollama sin embeddings ──

    /// Gate de test con respuestas prefijadas (y registro de llamadas).
    struct ScriptedGate(std::sync::Mutex<Vec<EmbeddingFallbackDecision>>);

    impl EmbeddingFallbackGate for ScriptedGate {
        fn decide(&self, _detail: &str, _attempt: u32) -> EmbeddingFallbackDecision {
            let mut v = self.0.lock().unwrap();
            if v.is_empty() {
                EmbeddingFallbackDecision::Cancel
            } else {
                v.remove(0)
            }
        }
    }

    type Calls = Arc<std::sync::Mutex<Vec<EmbeddingMode>>>;

    /// Monta un `ManageAiRag` cuya `add_documents` responde con `outcomes`
    /// (en orden) y registra la vía usada en cada llamada.
    fn rag_with_outcomes(
        stored: Option<&str>,
        outcomes: Vec<AddDocumentsOutcome>,
        saved: Arc<std::sync::Mutex<Option<RagConfig>>>,
    ) -> (ManageAiRag, Calls) {
        let mut insts = MockAiAdapterPort::new();
        insts.expect_get_instance().returning(|n| Ok(inst(n)));
        let mut cfg = RagConfig::default();
        if let Some(k) = stored {
            cfg.embedding_kinds
                .insert("chromadb/rag_dev".to_string(), k.to_string());
        }
        let mut store = MockAiRagStorePort::new();
        store
            .expect_load_rag_config()
            .returning(move |_| Ok(cfg.clone()));
        store.expect_save_rag_config().returning(move |_, c| {
            *saved.lock().unwrap() = Some(c.clone());
            Ok(())
        });
        let calls: Calls = Arc::new(std::sync::Mutex::new(Vec::new()));
        let outcomes = Arc::new(std::sync::Mutex::new(outcomes));
        let mut prov = MockAiVectorStorePort::new();
        prov.expect_ensure_vector_store()
            .returning(|_, _| Ok(vs_info()));
        let calls_f = calls.clone();
        prov.expect_build_vector_store().returning(move |_, _, _| {
            let mut vs = MockRagVectorStorePort::new();
            vs.expect_is_available().returning(|| true);
            vs.expect_ensure_collection().returning(|_| Ok(()));
            let calls = calls_f.clone();
            let outcomes = outcomes.clone();
            vs.expect_add_documents().returning(move |_, _, mode| {
                calls.lock().unwrap().push(mode);
                let mut o = outcomes.lock().unwrap();
                Ok(if o.is_empty() {
                    AddDocumentsOutcome::Added
                } else {
                    o.remove(0)
                })
            });
            Arc::new(vs)
        });
        let mut s = svc(insts, store, prov);
        s.retry_delay = std::time::Duration::ZERO;
        (s, calls)
    }

    fn unavailable() -> AddDocumentsOutcome {
        AddDocumentsOutcome::EmbeddingsUnavailable("Ollama no devolvió embeddings".into())
    }

    #[tokio::test]
    async fn ingest_fails_closed_when_ollama_down_without_tty() {
        let saved = Arc::new(std::sync::Mutex::new(None));
        let (s, calls) = rag_with_outcomes(None, vec![unavailable()], saved.clone());
        let err = s.ingest("dev", SRC).await.unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("--embedding-fallback hash"), "{msg}");
        assert!(msg.contains("ai start dev"), "{msg}");
        assert_eq!(*calls.lock().unwrap(), vec![EmbeddingMode::Ollama]);
        assert!(
            saved.lock().unwrap().is_none(),
            "no se registra vía si no se indexó"
        );
    }

    #[tokio::test]
    async fn ingest_retry_then_ollama_records_ollama_kind() {
        let saved = Arc::new(std::sync::Mutex::new(None));
        let (s, calls) = rag_with_outcomes(None, vec![unavailable()], saved.clone());
        let s = s.with_fallback_gate(Arc::new(ScriptedGate(std::sync::Mutex::new(vec![
            EmbeddingFallbackDecision::Retry,
        ]))));
        s.ingest("dev", SRC).await.unwrap();
        assert_eq!(
            *calls.lock().unwrap(),
            vec![EmbeddingMode::Ollama, EmbeddingMode::Ollama]
        );
        let cfg = saved.lock().unwrap().clone().unwrap();
        assert_eq!(
            cfg.embedding_kinds["chromadb/rag_dev"],
            "ollama:nomic-embed-text"
        );
    }

    #[tokio::test]
    async fn ingest_user_chooses_hash_records_hash_kind() {
        let saved = Arc::new(std::sync::Mutex::new(None));
        let (s, calls) = rag_with_outcomes(None, vec![unavailable()], saved.clone());
        let s = s.with_fallback_gate(Arc::new(ScriptedGate(std::sync::Mutex::new(vec![
            EmbeddingFallbackDecision::UseHash,
        ]))));
        s.ingest("dev", SRC).await.unwrap();
        assert_eq!(
            *calls.lock().unwrap(),
            vec![EmbeddingMode::Ollama, EmbeddingMode::Hash]
        );
        let cfg = saved.lock().unwrap().clone().unwrap();
        assert_eq!(cfg.embedding_kinds["chromadb/rag_dev"], "hash");
    }

    #[tokio::test]
    async fn ingest_retries_are_bounded() {
        let saved = Arc::new(std::sync::Mutex::new(None));
        let outcomes = (0..10).map(|_| unavailable()).collect();
        let (s, calls) = rag_with_outcomes(None, outcomes, saved);
        let s = s.with_fallback_gate(Arc::new(ScriptedGate(std::sync::Mutex::new(
            (0..10).map(|_| EmbeddingFallbackDecision::Retry).collect(),
        ))));
        assert!(s.ingest("dev", SRC).await.is_err());
        // 1 intento + MAX_EMBED_RETRIES reintentos.
        assert_eq!(calls.lock().unwrap().len() as u32, 1 + MAX_EMBED_RETRIES);
    }

    #[tokio::test]
    async fn hash_flag_indexes_by_hash_without_asking_ollama() {
        let saved = Arc::new(std::sync::Mutex::new(None));
        let (s, calls) = rag_with_outcomes(None, vec![], saved.clone());
        let s = s.with_hash_fallback(true);
        s.ingest("dev", SRC).await.unwrap();
        assert_eq!(*calls.lock().unwrap(), vec![EmbeddingMode::Hash]);
        let cfg = saved.lock().unwrap().clone().unwrap();
        assert_eq!(cfg.embedding_kinds["chromadb/rag_dev"], "hash");
    }

    #[tokio::test]
    async fn ingest_refuses_to_mix_with_recorded_kind() {
        // Colección indexada por hash: la vía Ollama no puede añadirle nada.
        let saved = Arc::new(std::sync::Mutex::new(None));
        let (s, calls) = rag_with_outcomes(Some("hash"), vec![], saved.clone());
        let err = s.ingest("dev", SRC).await.unwrap_err();
        assert!(err.to_string().contains("no se mezclan"), "{err}");
        assert!(
            calls.lock().unwrap().is_empty(),
            "no debe llegar al vector store"
        );
        // Y la elección de hash tampoco mezcla con una colección Ollama.
        let saved2 = Arc::new(std::sync::Mutex::new(None));
        let (s2, calls2) = rag_with_outcomes(Some("ollama:nomic-embed-text"), vec![], saved2);
        let s2 = s2.with_hash_fallback(true);
        assert!(s2.ingest("dev", SRC).await.is_err());
        assert!(calls2.lock().unwrap().is_empty());
    }

    #[test]
    fn fallback_answer_parsing_defaults_to_cancel() {
        use EmbeddingFallbackDecision::*;
        assert_eq!(parse_fallback_answer("r\n", true), Retry);
        assert_eq!(parse_fallback_answer("r", false), Cancel);
        assert_eq!(parse_fallback_answer("H", true), UseHash);
        assert_eq!(parse_fallback_answer("", true), Cancel);
        assert_eq!(parse_fallback_answer("loquesea", true), Cancel);
    }

    // ── H-12: `ai rag ingest-docs` (documentación embebida → `enola_docs`) ──

    /// Llamadas a `add_documents`: (colección, vía, ids).
    type DocsCalls = Arc<std::sync::Mutex<Vec<(String, EmbeddingMode, Vec<String>)>>>;

    /// `ManageAiRag` para `ingest_docs`: registra borrados y altas, responde
    /// con `outcomes` en orden y guarda el último `RagConfig` persistido.
    fn docs_rag(
        stored_docs_kind: Option<&str>,
        outcomes: Vec<AddDocumentsOutcome>,
        collection_exists: bool,
        saved: Arc<std::sync::Mutex<Option<RagConfig>>>,
    ) -> (ManageAiRag, DocsCalls, Arc<std::sync::Mutex<Vec<String>>>) {
        let mut insts = MockAiAdapterPort::new();
        insts.expect_get_instance().returning(|n| Ok(inst(n)));
        let mut cfg = RagConfig::default();
        cfg.embedding_kinds
            .insert("chromadb/rag_dev".into(), "ollama:nomic-embed-text".into());
        if let Some(k) = stored_docs_kind {
            cfg.embedding_kinds
                .insert("chromadb/enola_docs".into(), k.to_string());
        }
        let mut store = MockAiRagStorePort::new();
        store
            .expect_load_rag_config()
            .returning(move |_| Ok(cfg.clone()));
        store.expect_save_rag_config().returning(move |_, c| {
            *saved.lock().unwrap() = Some(c.clone());
            Ok(())
        });
        let calls: DocsCalls = Arc::new(std::sync::Mutex::new(Vec::new()));
        let deletes = Arc::new(std::sync::Mutex::new(Vec::new()));
        let outcomes = Arc::new(std::sync::Mutex::new(outcomes));
        let mut prov = MockAiVectorStorePort::new();
        prov.expect_ensure_vector_store()
            .returning(|_, _| Ok(vs_info()));
        let (calls_f, deletes_f) = (calls.clone(), deletes.clone());
        prov.expect_build_vector_store().returning(move |_, _, _| {
            let mut vs = MockRagVectorStorePort::new();
            vs.expect_is_available().returning(|| true);
            vs.expect_ensure_collection().returning(|_| Ok(()));
            let deletes = deletes_f.clone();
            vs.expect_delete_collection().returning(move |c| {
                deletes.lock().unwrap().push(c.to_string());
                if collection_exists {
                    Ok(())
                } else {
                    Err(EnolaError::NotFound(format!("Colección '{c}' no existe")))
                }
            });
            let calls = calls_f.clone();
            let outcomes = outcomes.clone();
            vs.expect_add_documents().returning(move |c, docs, mode| {
                let ids = docs.iter().map(|d| d.id.clone()).collect();
                calls.lock().unwrap().push((c.to_string(), mode, ids));
                let mut o = outcomes.lock().unwrap();
                Ok(if o.is_empty() {
                    AddDocumentsOutcome::Added
                } else {
                    o.remove(0)
                })
            });
            Arc::new(vs)
        });
        let mut s = svc(insts, store, prov);
        s.retry_delay = std::time::Duration::ZERO;
        (s, calls, deletes)
    }

    #[tokio::test]
    async fn ingest_docs_rebuilds_enola_docs_from_embedded_catalog() {
        let saved = Arc::new(std::sync::Mutex::new(None));
        let (s, calls, deletes) = docs_rag(None, vec![], true, saved.clone());
        let n = s.ingest_docs("dev").await.unwrap();
        assert_eq!(*deletes.lock().unwrap(), vec!["enola_docs".to_string()]);
        let calls = calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        let (collection, mode, ids) = &calls[0];
        assert_eq!(collection, "enola_docs", "nunca escribe en rag_<instancia>");
        assert_eq!(*mode, EmbeddingMode::Ollama);
        assert_eq!(ids.len(), n);
        // Todos los documentos del catálogo, con ids deterministas y únicos.
        for doc in crate::domain::docs::DOC_CATALOG {
            let prefix = format!("{}#", doc.uri());
            assert!(
                ids.iter().any(|id| id.starts_with(&prefix)),
                "falta {}",
                doc.path
            );
        }
        let unique: std::collections::HashSet<_> = ids.iter().collect();
        assert_eq!(unique.len(), ids.len());
        let cfg = saved.lock().unwrap().clone().unwrap();
        assert_eq!(
            cfg.embedding_kinds["chromadb/enola_docs"],
            "ollama:nomic-embed-text"
        );
        assert_eq!(
            cfg.embedding_kinds["chromadb/rag_dev"],
            "ollama:nomic-embed-text"
        );
    }

    #[tokio::test]
    async fn ingest_docs_reingest_does_not_duplicate_chunks() {
        let saved = Arc::new(std::sync::Mutex::new(None));
        let (s, calls, deletes) = docs_rag(None, vec![], true, saved);
        s.ingest_docs("dev").await.unwrap();
        s.ingest_docs("dev").await.unwrap();
        // Q8: cada ingesta reconstruye la colección y los ids son estables.
        assert_eq!(deletes.lock().unwrap().len(), 2);
        let calls = calls.lock().unwrap();
        assert_eq!(calls[0].2, calls[1].2);
    }

    #[tokio::test]
    async fn ingest_docs_tolerates_missing_collection() {
        let saved = Arc::new(std::sync::Mutex::new(None));
        let (s, calls, _) = docs_rag(None, vec![], false, saved);
        assert!(s.ingest_docs("dev").await.unwrap() > 0);
        assert_eq!(calls.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn ingest_docs_fails_closed_without_ollama_and_forgets_kind() {
        let saved = Arc::new(std::sync::Mutex::new(None));
        let (s, calls, _) = docs_rag(
            Some("ollama:nomic-embed-text"),
            vec![unavailable()],
            true,
            saved.clone(),
        );
        let err = s.ingest_docs("dev").await.unwrap_err();
        assert!(
            err.to_string().contains("--embedding-fallback hash"),
            "{err}"
        );
        assert_eq!(calls.lock().unwrap().len(), 1);
        // La colección se reconstruía: su vía previa ya no la describe.
        let cfg = saved.lock().unwrap().clone().unwrap();
        assert!(!cfg.embedding_kinds.contains_key("chromadb/enola_docs"));
        assert!(cfg.embedding_kinds.contains_key("chromadb/rag_dev"));
    }

    #[tokio::test]
    async fn ingest_docs_hash_flag_records_hash_and_may_switch_kind() {
        let saved = Arc::new(std::sync::Mutex::new(None));
        let (s, calls, _) = docs_rag(Some("ollama:nomic-embed-text"), vec![], true, saved.clone());
        let s = s.with_hash_fallback(true);
        s.ingest_docs("dev").await.unwrap();
        assert_eq!(calls.lock().unwrap()[0].1, EmbeddingMode::Hash);
        let cfg = saved.lock().unwrap().clone().unwrap();
        assert_eq!(cfg.embedding_kinds["chromadb/enola_docs"], "hash");
    }

    #[tokio::test]
    async fn config_preset_keeps_recorded_embedding_kinds() {
        let mut insts = MockAiAdapterPort::new();
        insts.expect_get_instance().returning(|n| Ok(inst(n)));
        let mut store = MockAiRagStorePort::new();
        store.expect_load_rag_config().returning(|_| {
            let mut c = RagConfig::default();
            c.embedding_kinds
                .insert("chromadb/rag_dev".into(), "hash".into());
            Ok(c)
        });
        store.expect_save_rag_config().returning(|_, _| Ok(()));
        let s = svc(insts, store, MockAiVectorStorePort::new());
        let cfg = s
            .config("dev", None, None, None, Some("faq"), None, false)
            .await
            .unwrap();
        assert_eq!(cfg.embedding_kinds["chromadb/rag_dev"], "hash");
    }
}
