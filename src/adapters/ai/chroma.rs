//! Fase 12.3 — Adapter ChromaDB para `RagVectorStorePort`.
//!
//! Chroma v2 (`/api/v2/tenants/{t}/databases/{d}/collections…`). Los
//! embeddings se generan vía Ollama (`embed.rs`) con fallback por hash.
//! Solo escucha en `127.0.0.1` (puerto host reservado por `ensure_vector_store`).

use crate::domain::error::{EnolaError, Result};
use crate::domain::rag::validate_collection_name;
use crate::ports::rag::{RagDocument, RagSearchResult, RagVectorStorePort};
use serde_json::{json, Value};
use std::time::Duration;

/// Tenant/database por defecto de Chroma v2.
const TENANT: &str = "default_tenant";
const DATABASE: &str = "default_database";
/// Timeout de llamadas a la API Chroma.
const CHROMA_TIMEOUT_SECS: u64 = 120;

/// Adapter HTTP de ChromaDB v2.
pub struct ChromaAdapter {
    /// Puerto host del vector store (`127.0.0.1:<port>`).
    port: u16,
    /// Puerto host de la API Ollama de la instancia (embeddings).
    ollama_port: u16,
    /// Modelo de embeddings (p.ej. `nomic-embed-text`).
    embedding_model: String,
}

impl ChromaAdapter {
    /// Crea el adapter para `127.0.0.1:<port>` con embeddings en `:<ollama_port>`.
    pub fn new(port: u16, ollama_port: u16, embedding_model: &str) -> Self {
        Self {
            port,
            ollama_port,
            embedding_model: embedding_model.to_string(),
        }
    }

    fn base(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    fn api(&self) -> String {
        format!(
            "{}/api/v2/tenants/{TENANT}/databases/{DATABASE}",
            self.base()
        )
    }

    fn client(&self) -> Result<reqwest::Client> {
        crate::infrastructure::http::http_client_builder(&self.base()).and_then(|b| {
            b.timeout(Duration::from_secs(CHROMA_TIMEOUT_SECS))
                .build()
                .map_err(|e| EnolaError::InfrastructureError(format!("Chroma HTTP client: {e}")))
        })
    }

    fn chroma_err(ctx: &str, status: reqwest::StatusCode, body: &str) -> EnolaError {
        let detail = serde_json::from_str::<Value>(body)
            .ok()
            .and_then(|v| {
                v.get("error")
                    .and_then(|e| e.as_str().map(str::to_string))
                    .or_else(|| {
                        v.get("message")
                            .and_then(|m| m.as_str().map(str::to_string))
                    })
            })
            .unwrap_or_else(|| body.trim().chars().take(200).collect());
        EnolaError::InfrastructureError(format!(
            "Chroma {ctx} → HTTP {}: {detail}",
            status.as_u16()
        ))
    }

    /// ID interno de una colección por nombre (Chroma usa UUIDs internos).
    async fn collection_id(&self, name: &str) -> Result<Option<String>> {
        let url = format!("{}/collections/{name}", self.api());
        let resp =
            self.client()?.get(&url).send().await.map_err(|e| {
                EnolaError::InfrastructureError(format!("Chroma GET collection: {e}"))
            })?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(Self::chroma_err("GET collection", status, &body));
        }
        let v: Value = serde_json::from_str(&body)
            .map_err(|e| EnolaError::InfrastructureError(format!("Chroma collection JSON: {e}")))?;
        Ok(v.get("id").and_then(|i| i.as_str()).map(str::to_string))
    }
}

#[async_trait::async_trait]
impl RagVectorStorePort for ChromaAdapter {
    async fn ensure_collection(&self, collection: &str) -> Result<()> {
        validate_collection_name(collection)?;
        if self.collection_id(collection).await?.is_some() {
            return Ok(());
        }
        let url = format!("{}/collections", self.api());
        let resp = self
            .client()?
            .post(&url)
            .json(&json!({ "name": collection }))
            .send()
            .await
            .map_err(|e| {
                EnolaError::InfrastructureError(format!("Chroma create collection: {e}"))
            })?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        // 409/exists → la colección ya estaba: idempotente.
        if !status.is_success()
            && !body.contains("already exists")
            && status != reqwest::StatusCode::CONFLICT
        {
            return Err(Self::chroma_err("create collection", status, &body));
        }
        Ok(())
    }

    async fn add_documents(&self, collection: &str, docs: Vec<RagDocument>) -> Result<()> {
        if docs.is_empty() {
            return Ok(());
        }
        self.ensure_collection(collection).await?;
        let id = self.collection_id(collection).await?.ok_or_else(|| {
            EnolaError::NotFound(format!(
                "Colección '{collection}' no encontrada tras crearla"
            ))
        })?;

        let texts: Vec<String> = docs.iter().map(|d| d.content.clone()).collect();
        let embeddings =
            super::embed::embed_texts(self.ollama_port, &self.embedding_model, &texts).await?;

        let ids: Vec<&str> = docs.iter().map(|d| d.id.as_str()).collect();
        let metadatas: Vec<&std::collections::HashMap<String, String>> =
            docs.iter().map(|d| &d.metadata).collect();

        let url = format!("{}/collections/{id}/add", self.api());
        let resp = self
            .client()?
            .post(&url)
            .json(&json!({
                "ids": ids,
                "embeddings": embeddings,
                "documents": texts,
                "metadatas": metadatas,
            }))
            .send()
            .await
            .map_err(|e| EnolaError::InfrastructureError(format!("Chroma add: {e}")))?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(Self::chroma_err("add", status, &body));
        }
        Ok(())
    }

    async fn query(
        &self,
        collection: &str,
        query: &str,
        n_results: usize,
    ) -> Result<Vec<RagSearchResult>> {
        validate_collection_name(collection)?;
        let id = self.collection_id(collection).await?.ok_or_else(|| {
            EnolaError::NotFound(format!("Colección '{collection}' no existe en el índice"))
        })?;

        let emb = super::embed::embed_texts(
            self.ollama_port,
            &self.embedding_model,
            &[query.to_string()],
        )
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| EnolaError::InfrastructureError("Sin embedding para la consulta".into()))?;

        let url = format!("{}/collections/{id}/query", self.api());
        let resp = self
            .client()?
            .post(&url)
            .json(&json!({
                "query_embeddings": [emb],
                "n_results": n_results.max(1),
                "include": ["documents", "metadatas", "distances"],
            }))
            .send()
            .await
            .map_err(|e| EnolaError::InfrastructureError(format!("Chroma query: {e}")))?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(Self::chroma_err("query", status, &body));
        }
        let v: Value = serde_json::from_str(&body)
            .map_err(|e| EnolaError::InfrastructureError(format!("Chroma query JSON: {e}")))?;

        let ids = v["ids"][0].as_array().cloned().unwrap_or_default();
        let docs = v["documents"][0].as_array().cloned().unwrap_or_default();
        let metas = v["metadatas"][0].as_array().cloned().unwrap_or_default();
        let dists = v["distances"][0].as_array().cloned().unwrap_or_default();

        let mut out = Vec::with_capacity(ids.len());
        for (i, idv) in ids.iter().enumerate() {
            let content = docs
                .get(i)
                .and_then(|d| d.as_str())
                .unwrap_or("")
                .to_string();
            let metadata = metas
                .get(i)
                .and_then(|m| serde_json::from_value(m.clone()).ok())
                .unwrap_or_default();
            let distance = dists.get(i).and_then(|d| d.as_f64()).unwrap_or(f64::MAX) as f32;
            out.push(RagSearchResult {
                document: RagDocument {
                    id: idv.as_str().unwrap_or("").to_string(),
                    content,
                    metadata,
                },
                distance,
            });
        }
        Ok(out)
    }

    async fn delete_collection(&self, collection: &str) -> Result<()> {
        validate_collection_name(collection)?;
        let url = format!("{}/collections/{collection}", self.api());
        let resp = self
            .client()?
            .delete(&url)
            .send()
            .await
            .map_err(|e| EnolaError::InfrastructureError(format!("Chroma delete: {e}")))?;
        let status = resp.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Err(EnolaError::NotFound(format!(
                "Colección '{collection}' no existe"
            )));
        }
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(Self::chroma_err("delete collection", status, &body));
        }
        Ok(())
    }

    async fn count(&self, collection: &str) -> Result<u64> {
        validate_collection_name(collection)?;
        let id = self
            .collection_id(collection)
            .await?
            .ok_or_else(|| EnolaError::NotFound(format!("Colección '{collection}' no existe")))?;
        let url = format!("{}/collections/{id}/count", self.api());
        let resp = self
            .client()?
            .get(&url)
            .send()
            .await
            .map_err(|e| EnolaError::InfrastructureError(format!("Chroma count: {e}")))?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(Self::chroma_err("count", status, &body));
        }
        body.trim().parse::<u64>().map_err(|e| {
            EnolaError::InfrastructureError(format!("Chroma count parse '{body}': {e}"))
        })
    }

    async fn is_available(&self) -> bool {
        let url = format!("{}/api/v2/heartbeat", self.base());
        match self.client() {
            Ok(c) => c
                .get(&url)
                .send()
                .await
                .map(|r| r.status().is_success())
                .unwrap_or(false),
            Err(_) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// Mini servidor Chroma de prueba: colección fija, add/query/count.
    async fn spawn_chroma_stub() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 16384];
                    let n = socket.read(&mut buf).await.unwrap_or(0);
                    let raw = String::from_utf8_lossy(&buf[..n]).to_string();
                    let first = raw.lines().next().unwrap_or("").to_string();
                    let (status, body) = if first.contains("/collections/rag_dev")
                        && first.starts_with("GET")
                    {
                        (
                            "200 OK",
                            r#"{"id":"coll-123","name":"rag_dev"}"#.to_string(),
                        )
                    } else if first.contains("/collections")
                        && first.starts_with("POST")
                        && !first.contains("/query")
                        && !first.contains("/add")
                    {
                        (
                            "200 OK",
                            r#"{"id":"coll-123","name":"rag_dev"}"#.to_string(),
                        )
                    } else if first.contains("/add") {
                        ("201 Created", "{}".to_string())
                    } else if first.contains("/query") {
                        ("200 OK", r#"{"ids":[["d1"]],"documents":[["texto uno"]],"metadatas":[[{"source":"a.md"}]],"distances":[[0.42]]}"#.to_string())
                    } else if first.contains("/count") {
                        ("200 OK", "5".to_string())
                    } else if first.contains("/heartbeat") {
                        ("200 OK", r#"{"nanosecond heartbeat":1}"#.to_string())
                    } else if first.starts_with("DELETE") {
                        ("200 OK", "{}".to_string())
                    } else {
                        ("404 Not Found", r#"{"error":"not found"}"#.to_string())
                    };
                    let resp = format!(
                        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = socket.write_all(resp.as_bytes()).await;
                    let _ = socket.shutdown().await;
                });
            }
        });
        format!("http://127.0.0.1:{port}")
    }

    fn adapter_for(port: u16) -> ChromaAdapter {
        // ollama_port=1 → fallback por hash en embeddings (server ausente)
        ChromaAdapter::new(port, 1, "nomic-embed-text")
    }

    fn port_of(url: &str) -> u16 {
        url.rsplit(':').next().unwrap_or("0").parse().unwrap_or(0)
    }

    #[tokio::test]
    async fn chroma_ensure_and_add_and_query_and_count() {
        let url = spawn_chroma_stub().await;
        let a = adapter_for(port_of(&url));
        a.ensure_collection("rag_dev").await.unwrap();
        a.add_documents(
            "rag_dev",
            vec![RagDocument {
                id: "d1".into(),
                content: "texto uno".into(),
                metadata: std::collections::HashMap::from([("source".into(), "a.md".into())]),
            }],
        )
        .await
        .unwrap();
        let res = a.query("rag_dev", "texto", 5).await.unwrap();
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].document.id, "d1");
        assert!((res[0].distance - 0.42).abs() < 1e-6);
        assert_eq!(a.count("rag_dev").await.unwrap(), 5);
        assert!(a.is_available().await);
    }

    #[tokio::test]
    async fn chroma_query_missing_collection_is_not_found() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 8192];
                    let _ = socket.read(&mut buf).await;
                    let body = r#"{"error":"not found"}"#;
                    let resp = format!("HTTP/1.1 404 Not Found\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                    let _ = socket.write_all(resp.as_bytes()).await;
                    let _ = socket.shutdown().await;
                });
            }
        });
        let a = adapter_for(port);
        let err = a.query("rag_dev", "x", 3).await.unwrap_err();
        assert!(matches!(err, EnolaError::NotFound(_)), "got {err}");
    }
}
