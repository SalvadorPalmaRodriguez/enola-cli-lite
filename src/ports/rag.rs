// Fase 12.2 — Ports del índice RAG (`ai rag`).
//
// Fronteras hexagonales:
// - `RagVectorStorePort`: el almacén vectorial de una instancia (Chroma/Qdrant
//   HTTP); el adapter resuelve embeddings internamente vía Ollama.
// - `AiRagStorePort`: estado por instancia en disco (fuentes ligadas +
//   `rag_config.json` bajo `/srv/enola-ai/<name>/`).
// - `AiVectorStorePort`: aprovisiona el contenedor del vector store vía
//   `ContainerPort` (NUNCA docker compose / CLI docker).

use crate::domain::error::Result;
pub use crate::domain::rag::{AiSource, AiSourceType, RagConfig, VectorStoreBackend};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

/// Documento a indexar en el vector store.
#[derive(Debug, Clone, PartialEq)]
pub struct RagDocument {
    /// Identificador único del documento/chunk.
    pub id: String,
    /// Texto del chunk.
    pub content: String,
    /// Metadatos (source, path, chunk i/n, extension…).
    pub metadata: HashMap<String, String>,
}

/// Resultado de una consulta semántica.
#[derive(Debug, Clone, PartialEq)]
pub struct RagSearchResult {
    /// Documento recuperado.
    pub document: RagDocument,
    /// Distancia (menor = más similar; semántica del backend).
    pub distance: f32,
}

/// Puerto del vector store de una instancia (Chroma/Qdrant vía HTTP local).
#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait RagVectorStorePort: Send + Sync {
    /// Crea la colección si no existe (idempotente).
    async fn ensure_collection(&self, collection: &str) -> Result<()>;

    /// Añade documentos a una colección (genera embeddings internamente).
    async fn add_documents(&self, collection: &str, docs: Vec<RagDocument>) -> Result<()>;

    /// Consulta semántica: devuelve los `n_results` más cercanos a `query`.
    async fn query(
        &self,
        collection: &str,
        query: &str,
        n_results: usize,
    ) -> Result<Vec<RagSearchResult>>;

    /// Elimina una colección completa.
    async fn delete_collection(&self, collection: &str) -> Result<()>;

    /// Número aproximado de documentos indexados en la colección.
    async fn count(&self, collection: &str) -> Result<u64>;

    /// `true` si el backend responde (health check ligero).
    async fn is_available(&self) -> bool;
}

/// Endpoint del vector store ya aprovisionado para una instancia.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VectorStoreInfo {
    /// Backend elegido.
    pub backend: VectorStoreBackend,
    /// Puerto host `127.0.0.1:<port>`.
    pub host_port: u16,
    /// URL base (`http://127.0.0.1:<port>`).
    pub endpoint: String,
}

/// Puerto de aprovisionamiento del contenedor de vector store por instancia.
#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait AiVectorStorePort: Send + Sync {
    /// Asegura que el contenedor del vector store de la instancia existe y
    /// está en ejecución (lo crea vía `ContainerPort` si falta).
    async fn ensure_vector_store(
        &self,
        instance: &str,
        backend: VectorStoreBackend,
    ) -> Result<VectorStoreInfo>;

    /// Devuelve el endpoint del vector store si está aprovisionado y activo.
    async fn vector_store_endpoint(&self, instance: &str) -> Result<Option<VectorStoreInfo>>;

    /// Construye el `RagVectorStorePort` concreto para el endpoint dado.
    /// Factoría dentro del port para que `application/` nunca importe
    /// `adapters/` (frontera hexagonal).
    fn build_vector_store(
        &self,
        info: &VectorStoreInfo,
        ollama_port: u16,
        embedding_model: &str,
    ) -> Arc<dyn RagVectorStorePort + Send + Sync>;
}

/// Puerto del estado RAG por instancia en disco:
/// `rag_sources.json` (fuentes ligadas) y `rag_config.json` (configuración).
#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait AiRagStorePort: Send + Sync {
    /// Carga `rag_config.json` de la instancia (defaults si no existe).
    async fn load_rag_config(&self, instance: &str) -> Result<RagConfig>;

    /// Persiste `rag_config.json` de la instancia.
    async fn save_rag_config(&self, instance: &str, config: &RagConfig) -> Result<()>;

    /// Lista las fuentes ligadas a la instancia.
    async fn list_rag_sources(&self, instance: &str) -> Result<Vec<AiSource>>;

    /// Registra una nueva fuente ligada.
    async fn add_rag_source(&self, instance: &str, source: AiSource) -> Result<()>;

    /// Marca una fuente como sincronizada (ISO 8601 en `last_synced`).
    async fn mark_source_synced(
        &self,
        instance: &str,
        source_id: &str,
        iso_time: &str,
    ) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mock_vector_store_contract() {
        let mut mock = MockRagVectorStorePort::new();
        mock.expect_ensure_collection().returning(|_| Ok(()));
        mock.expect_add_documents().returning(|_, _| Ok(()));
        mock.expect_query().returning(|_, _, _| Ok(vec![]));
        mock.expect_count().returning(|_| Ok(7));
        mock.expect_is_available().returning(|| true);

        mock.ensure_collection("rag_dev").await.unwrap();
        mock.add_documents(
            "rag_dev",
            vec![RagDocument {
                id: "d1".into(),
                content: "hola".into(),
                metadata: HashMap::new(),
            }],
        )
        .await
        .unwrap();
        assert!(mock.query("rag_dev", "hola", 3).await.unwrap().is_empty());
        assert_eq!(mock.count("rag_dev").await.unwrap(), 7);
        assert!(mock.is_available().await);
    }

    #[tokio::test]
    async fn mock_rag_store_contract() {
        let mut mock = MockAiRagStorePort::new();
        mock.expect_load_rag_config()
            .returning(|_| Ok(RagConfig::default()));
        mock.expect_save_rag_config().returning(|_, _| Ok(()));
        mock.expect_list_rag_sources().returning(|_| Ok(vec![]));
        mock.expect_add_rag_source().returning(|_, _| Ok(()));
        mock.expect_mark_source_synced().returning(|_, _, _| Ok(()));

        let cfg = mock.load_rag_config("dev").await.unwrap();
        assert_eq!(cfg.chunk_size, 3000);
        mock.save_rag_config("dev", &cfg).await.unwrap();
        assert!(mock.list_rag_sources("dev").await.unwrap().is_empty());
        mock.add_rag_source(
            "dev",
            AiSource {
                id: "s1".into(),
                instance_name: "dev".into(),
                source_type: AiSourceType::Filesystem,
                location: "/home/u/docs".into(),
                last_synced: None,
            },
        )
        .await
        .unwrap();
        mock.mark_source_synced("dev", "s1", "2026-10-18T00:00:00Z")
            .await
            .unwrap();
    }
}
