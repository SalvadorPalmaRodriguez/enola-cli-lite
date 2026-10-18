//! Fase 12.3 — Adapter Qdrant para `RagVectorStorePort`.
//!
//! API REST estable (`/collections/{name}`, `/collections/{name}/points…`).
//! Los IDs de punto son `u64`: se derivan del `id` del documento por hash
//! estable (FNV-1a), y el id textual queda en el payload (`doc_id`).
//! Solo escucha en `127.0.0.1`.

use crate::domain::error::{EnolaError, Result};
use crate::domain::rag::validate_collection_name;
use crate::ports::rag::{RagDocument, RagSearchResult, RagVectorStorePort};
use serde_json::{json, Value};
use std::time::Duration;

/// Timeout de llamadas a la API Qdrant.
const QDRANT_TIMEOUT_SECS: u64 = 120;

/// Adapter HTTP de Qdrant.
pub struct QdrantAdapter {
    /// Puerto host del vector store (`127.0.0.1:<port>`).
    port: u16,
    /// Puerto host de la API Ollama de la instancia (embeddings).
    ollama_port: u16,
    /// Modelo de embeddings (p.ej. `nomic-embed-text`).
    embedding_model: String,
}

impl QdrantAdapter {
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

    fn collections(&self) -> String {
        format!("{}/collections", self.base())
    }

    fn client(&self) -> Result<reqwest::Client> {
        crate::infrastructure::http::http_client_builder(&self.base()).and_then(|b| {
            b.timeout(Duration::from_secs(QDRANT_TIMEOUT_SECS))
                .build()
                .map_err(|e| EnolaError::InfrastructureError(format!("Qdrant HTTP client: {e}")))
        })
    }

    fn qdrant_err(ctx: &str, status: reqwest::StatusCode, body: &str) -> EnolaError {
        let detail = serde_json::from_str::<Value>(body)
            .ok()
            .and_then(|v| {
                v.pointer("/status/error")
                    .and_then(|e| e.as_str().map(str::to_string))
            })
            .unwrap_or_else(|| body.trim().chars().take(200).collect());
        EnolaError::InfrastructureError(format!(
            "Qdrant {ctx} → HTTP {}: {detail}",
            status.as_u16()
        ))
    }

    /// `u64` estable desde un id de documento (FNV-1a 64).
    fn point_id(doc_id: &str) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for b in doc_id.as_bytes() {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x100000001b3);
        }
        h
    }

    /// `true` si la colección existe (GET 200).
    async fn collection_exists(&self, name: &str) -> Result<bool> {
        let url = format!("{}/{name}", self.collections());
        let resp =
            self.client()?.get(&url).send().await.map_err(|e| {
                EnolaError::InfrastructureError(format!("Qdrant GET collection: {e}"))
            })?;
        let status = resp.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Ok(false);
        }
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(Self::qdrant_err("GET collection", status, &body));
        }
        Ok(true)
    }
}

#[async_trait::async_trait]
impl RagVectorStorePort for QdrantAdapter {
    async fn ensure_collection(&self, collection: &str) -> Result<()> {
        validate_collection_name(collection)?;
        if self.collection_exists(collection).await? {
            return Ok(());
        }
        // Dimensión: embeddings de Ollama varían por modelo; fallback usa
        // FALLBACK_EMBED_DIM. Se crea con la dimensión del primer embedding
        // real vía `create` perezoso: aquí usamos la dim del fallback y si el
        // primer upsert real difiere, Qdrant rechazará con error claro.
        let url = format!("{}/{collection}", self.collections());
        let resp = self
            .client()?
            .put(&url)
            .json(&json!({
                "vectors": {
                    "size": super::embed::FALLBACK_EMBED_DIM,
                    "distance": "Cosine",
                }
            }))
            .send()
            .await
            .map_err(|e| {
                EnolaError::InfrastructureError(format!("Qdrant create collection: {e}"))
            })?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            // "already exists" es idempotente.
            if !body.contains("already exists") {
                return Err(Self::qdrant_err("create collection", status, &body));
            }
        }
        Ok(())
    }

    async fn add_documents(&self, collection: &str, docs: Vec<RagDocument>) -> Result<()> {
        if docs.is_empty() {
            return Ok(());
        }
        self.ensure_collection(collection).await?;

        let texts: Vec<String> = docs.iter().map(|d| d.content.clone()).collect();
        let embeddings =
            super::embed::embed_texts(self.ollama_port, &self.embedding_model, &texts).await?;

        let points: Vec<Value> = docs
            .iter()
            .zip(embeddings.iter())
            .map(|(d, emb)| {
                let mut payload = serde_json::Map::new();
                payload.insert("doc_id".to_string(), Value::String(d.id.clone()));
                payload.insert("content".to_string(), Value::String(d.content.clone()));
                for (k, v) in &d.metadata {
                    payload.insert(k.clone(), Value::String(v.clone()));
                }
                json!({
                    "id": Self::point_id(&d.id),
                    "vector": emb,
                    "payload": Value::Object(payload),
                })
            })
            .collect();

        let url = format!("{}/{collection}/points?wait=true", self.collections());
        let resp = self
            .client()?
            .put(&url)
            .json(&json!({ "points": points }))
            .send()
            .await
            .map_err(|e| EnolaError::InfrastructureError(format!("Qdrant upsert: {e}")))?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(Self::qdrant_err("upsert points", status, &body));
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
        if !self.collection_exists(collection).await? {
            return Err(EnolaError::NotFound(format!(
                "Colección '{collection}' no existe en el índice"
            )));
        }
        let emb = super::embed::embed_texts(
            self.ollama_port,
            &self.embedding_model,
            &[query.to_string()],
        )
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| EnolaError::InfrastructureError("Sin embedding para la consulta".into()))?;

        let url = format!("{}/{collection}/points/search", self.collections());
        let resp = self
            .client()?
            .post(&url)
            .json(&json!({
                "vector": emb,
                "limit": n_results.max(1),
                "with_payload": true,
            }))
            .send()
            .await
            .map_err(|e| EnolaError::InfrastructureError(format!("Qdrant search: {e}")))?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(Self::qdrant_err("search", status, &body));
        }
        let v: Value = serde_json::from_str(&body)
            .map_err(|e| EnolaError::InfrastructureError(format!("Qdrant search JSON: {e}")))?;

        let mut out = Vec::new();
        for hit in v["result"].as_array().cloned().unwrap_or_default() {
            let payload = hit["payload"].clone();
            let doc_id = payload
                .get("doc_id")
                .and_then(|d| d.as_str())
                .unwrap_or("")
                .to_string();
            let content = payload
                .get("content")
                .and_then(|c| c.as_str())
                .unwrap_or("")
                .to_string();
            let metadata = payload
                .as_object()
                .map(|m| {
                    m.iter()
                        .filter(|(k, _)| k.as_str() != "content")
                        .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                        .collect()
                })
                .unwrap_or_default();
            // Qdrant devuelve score (mayor = más similar); lo exponemos como
            // distancia = 1 - score para mantener la semántica del port.
            let score = hit["score"].as_f64().unwrap_or(0.0) as f32;
            out.push(RagSearchResult {
                document: RagDocument {
                    id: doc_id,
                    content,
                    metadata,
                },
                distance: 1.0 - score,
            });
        }
        Ok(out)
    }

    async fn delete_collection(&self, collection: &str) -> Result<()> {
        validate_collection_name(collection)?;
        let url = format!("{}/{collection}", self.collections());
        let resp = self
            .client()?
            .delete(&url)
            .send()
            .await
            .map_err(|e| EnolaError::InfrastructureError(format!("Qdrant delete: {e}")))?;
        let status = resp.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Err(EnolaError::NotFound(format!(
                "Colección '{collection}' no existe"
            )));
        }
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(Self::qdrant_err("delete collection", status, &body));
        }
        Ok(())
    }

    async fn count(&self, collection: &str) -> Result<u64> {
        validate_collection_name(collection)?;
        let url = format!("{}/{collection}", self.collections());
        let resp = self
            .client()?
            .get(&url)
            .send()
            .await
            .map_err(|e| EnolaError::InfrastructureError(format!("Qdrant count: {e}")))?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            if status == reqwest::StatusCode::NOT_FOUND {
                return Err(EnolaError::NotFound(format!(
                    "Colección '{collection}' no existe"
                )));
            }
            return Err(Self::qdrant_err("count", status, &body));
        }
        let v: Value = serde_json::from_str(&body)
            .map_err(|e| EnolaError::InfrastructureError(format!("Qdrant count JSON: {e}")))?;
        v.pointer("/result/points_count")
            .and_then(|n| n.as_u64())
            .ok_or_else(|| EnolaError::InfrastructureError("Qdrant count sin points_count".into()))
    }

    async fn is_available(&self) -> bool {
        match self.client() {
            Ok(c) => c
                .get(&self.collections())
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

    /// Mini servidor Qdrant de prueba.
    async fn spawn_qdrant_stub() -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 16384];
                    let n = socket.read(&mut buf).await.unwrap_or(0);
                    let raw = String::from_utf8_lossy(&buf[..n]).to_string();
                    let first = raw.lines().next().unwrap_or("").to_string();
                    let (status, body) = if first.starts_with("GET /collections/rag_dev") {
                        ("200 OK", r#"{"result":{"points_count":3}}"#.to_string())
                    } else if first.starts_with("PUT /collections/rag_dev/points") {
                        ("200 OK", r#"{"result":{"status":"completed"}}"#.to_string())
                    } else if first.starts_with("PUT /collections/rag_dev") {
                        ("200 OK", r#"{"result":true}"#.to_string())
                    } else if first.contains("/points/search") {
                        ("200 OK", r#"{"result":[{"id":7,"score":0.91,"payload":{"doc_id":"d1","content":"texto","source":"a.md"}}]}"#.to_string())
                    } else if first.starts_with("GET /collections") {
                        ("200 OK", r#"{"result":{"collections":[]}}"#.to_string())
                    } else {
                        (
                            "404 Not Found",
                            r#"{"status":{"error":"not found"}}"#.to_string(),
                        )
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
        port
    }

    #[tokio::test]
    async fn qdrant_add_query_count() {
        let port = spawn_qdrant_stub().await;
        let a = QdrantAdapter::new(port, 1, "nomic-embed-text");
        a.ensure_collection("rag_dev").await.unwrap();
        a.add_documents(
            "rag_dev",
            vec![RagDocument {
                id: "d1".into(),
                content: "texto".into(),
                metadata: std::collections::HashMap::from([("source".into(), "a.md".into())]),
            }],
        )
        .await
        .unwrap();
        let res = a.query("rag_dev", "texto", 5).await.unwrap();
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].document.id, "d1");
        assert!((res[0].distance - 0.09).abs() < 1e-5); // 1 - 0.91
        assert_eq!(a.count("rag_dev").await.unwrap(), 3);
    }

    #[tokio::test]
    async fn qdrant_missing_collection_404() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 8192];
                    let _ = socket.read(&mut buf).await;
                    let body = r#"{"status":{"error":"not found"}}"#;
                    let resp = format!("HTTP/1.1 404 Not Found\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                    let _ = socket.write_all(resp.as_bytes()).await;
                    let _ = socket.shutdown().await;
                });
            }
        });
        let a = QdrantAdapter::new(port, 1, "m");
        let err = a.query("rag_dev", "x", 3).await.unwrap_err();
        assert!(matches!(err, EnolaError::NotFound(_)));
    }
}
