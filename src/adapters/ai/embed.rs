//! Fase 12.3 — Embeddings compartidos por los adapters de vector store.
//!
//! `POST {ollama}/api/embed {model, input}` → `embeddings[0]`. La vía la decide
//! el caso de uso (D6): con `EmbeddingMode::Ollama`, si Ollama no responde se
//! devuelve `None` (nunca se cae a hash en silencio); con `EmbeddingMode::Hash`
//! se usa el embedding determinista por hash, solo por elección explícita.

use crate::domain::error::{EnolaError, Result};
use crate::domain::rag::EmbeddingMode;
use serde_json::json;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// Dimensión del embedding de fallback por hash.
pub const FALLBACK_EMBED_DIM: usize = 384;

/// Timeout de petición a `/api/embed` (embeddings son rápidos).
const EMBED_TIMEOUT_SECS: u64 = 60;

/// Endpoint de embeddings dado el `host_port` de la API Ollama.
pub fn ollama_embed_url(ollama_port: u16) -> String {
    format!("http://127.0.0.1:{ollama_port}/api/embed")
}

/// Genera un embedding por Ollama para `text` (`None` si falla).
async fn try_embed(
    client: &reqwest::Client,
    ollama_port: u16,
    model: &str,
    text: &str,
) -> Option<Vec<f32>> {
    let url = ollama_embed_url(ollama_port);
    let payload = json!({ "model": model, "input": text });
    let resp = client.post(&url).json(&payload).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let body: serde_json::Value = resp.json().await.ok()?;
    let emb = body.get("embeddings")?.as_array()?.first()?.as_array()?;
    let vec: Vec<f32> = emb
        .iter()
        .filter_map(|v| v.as_f64().map(|f| f as f32))
        .collect();
    if vec.is_empty() {
        None
    } else {
        Some(vec)
    }
}

/// Embedding determinista por hash (fallback sin Ollama).
/// Misma dimensión para todos los textos; solo sirve para no bloquear.
pub fn hash_embedding(text: &str) -> Vec<f32> {
    let mut v = vec![0.0f32; FALLBACK_EMBED_DIM];
    let words: Vec<&str> = text.split_whitespace().collect();
    for w in &words {
        let mut h = DefaultHasher::new();
        w.hash(&mut h);
        let slot = (h.finish() as usize) % FALLBACK_EMBED_DIM;
        v[slot] += 1.0;
    }
    // Normalizar a vector unitario (distancia coseno coherente).
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in &mut v {
            *x /= norm;
        }
    }
    v
}

/// Genera embeddings para `texts` por la vía `mode`.
///
/// - `Hash`: embedding determinista por hash para todos (sin red).
/// - `Ollama`: `Some` solo si TODOS los textos obtienen embedding; `None` si
///   Ollama falla en alguno (no se indexa nada a medias ni se mezcla con hash).
pub async fn embed_texts(
    ollama_port: u16,
    model: &str,
    texts: &[String],
    mode: EmbeddingMode,
) -> Result<Option<Vec<Vec<f32>>>> {
    if texts.is_empty() {
        return Ok(Some(Vec::new()));
    }
    if mode == EmbeddingMode::Hash {
        return Ok(Some(texts.iter().map(|t| hash_embedding(t)).collect()));
    }
    let ollama_base = format!("http://127.0.0.1:{ollama_port}");
    let client = crate::infrastructure::http::http_client_builder(&ollama_base).and_then(|b| {
        b.timeout(std::time::Duration::from_secs(EMBED_TIMEOUT_SECS))
            .build()
            .map_err(|e| EnolaError::InfrastructureError(format!("HTTP client (embed): {e}")))
    })?;

    let mut out: Vec<Vec<f32>> = Vec::with_capacity(texts.len());
    for t in texts {
        match try_embed(&client, ollama_port, model, t).await {
            Some(v) => out.push(v),
            None => return Ok(None),
        }
    }
    Ok(Some(out))
}

/// Mensaje accionable cuando Ollama no devuelve embeddings.
pub fn embeddings_unavailable_detail(ollama_port: u16, model: &str) -> String {
    format!(
        "Ollama no devolvió embeddings en 127.0.0.1:{ollama_port} (modelo '{model}'): \
         instancia parada o modelo de embeddings no descargado"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_embedding_is_deterministic_and_unit() {
        let a = hash_embedding("hola mundo enola");
        let b = hash_embedding("hola mundo enola");
        assert_eq!(a, b);
        assert_eq!(a.len(), FALLBACK_EMBED_DIM);
        let norm: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-5);
    }

    #[test]
    fn hash_embedding_empty_is_zero() {
        let v = hash_embedding("");
        assert!(v.iter().all(|&x| x == 0.0));
    }

    #[tokio::test]
    async fn embed_ollama_down_returns_none_instead_of_hash() {
        // D6: puerto sin servidor → None (sin respaldo silencioso por hash).
        let out = embed_texts(
            1,
            "nomic-embed-text",
            &["hola".to_string()],
            EmbeddingMode::Ollama,
        )
        .await
        .unwrap();
        assert!(out.is_none());
    }

    #[tokio::test]
    async fn embed_hash_mode_never_calls_ollama() {
        let out = embed_texts(
            1,
            "nomic-embed-text",
            &["hola".to_string()],
            EmbeddingMode::Hash,
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].len(), FALLBACK_EMBED_DIM);
    }
}
