//! Fase 12.3 — Embeddings Ollama compartidos por los adapters de vector store.
//!
//! `POST {ollama}/api/embed {model, input}` → `embeddings[0]`. Si Ollama no
//! responde o devuelve vectores vacíos, se usa un embedding determinista por
//! hash (dimensión fija) para que la ingesta nunca quede bloqueada por el
//! modelo de embeddings — con la precisión semántica reducida que ello implica.

use crate::domain::error::{EnolaError, Result};
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

/// Genera embeddings para `texts` vía Ollama; por cada texto que falle usa
/// el fallback por hash (misma dimensión garantizada solo si TODOS usan la
/// misma vía — si Ollama devuelve dims distintas del fallback se fuerza
/// fallback global para no mezclar espacios vectoriales).
pub async fn embed_texts(ollama_port: u16, model: &str, texts: &[String]) -> Result<Vec<Vec<f32>>> {
    if texts.is_empty() {
        return Ok(Vec::new());
    }
    let ollama_base = format!("http://127.0.0.1:{ollama_port}");
    let client = crate::infrastructure::http::http_client_builder(&ollama_base).and_then(|b| {
        b.timeout(std::time::Duration::from_secs(EMBED_TIMEOUT_SECS))
            .build()
            .map_err(|e| EnolaError::InfrastructureError(format!("HTTP client (embed): {e}")))
    })?;

    let mut out: Vec<Vec<f32>> = Vec::with_capacity(texts.len());
    let mut all_ok = true;
    for t in texts {
        match try_embed(&client, ollama_port, model, t).await {
            Some(v) => out.push(v),
            None => {
                all_ok = false;
                break;
            }
        }
    }
    if all_ok && out.len() == texts.len() {
        return Ok(out);
    }
    // Fallback global: hash determinista para TODOS (espacio homogéneo).
    tracing::warn!("Ollama /api/embed no disponible (:{ollama_port}); usando embeddings por hash");
    Ok(texts.iter().map(|t| hash_embedding(t)).collect())
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
    async fn embed_falls_back_when_ollama_down() {
        // Puerto sin servidor → fallback por hash.
        let out = embed_texts(1, "nomic-embed-text", &["hola".to_string()])
            .await
            .unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].len(), FALLBACK_EMBED_DIM);
    }
}
