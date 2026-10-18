// Fase 13.1 — Caso de uso `ai benchmark`: medición de tokens/s, latencia
// y tamaño de modelo en una instancia Ollama local.
//
// Hexagonal: usa SOLO `AiAdapterPort` (resolución de instancia) +
// `LlmEngine` (generación). El motor concreto lo inyecta el executor.

use crate::domain::ai::validate_ai_instance_name;
use crate::domain::ai_benchmark::{aggregate_samples, BenchmarkReport, BenchmarkSample};
use crate::domain::error::{EnolaError, Result};
use crate::ports::ai::AiAdapterPort;
use crate::ports::llm::{ChatMessage, LlmEngine};
use std::sync::Arc;
use std::time::Instant;

/// Prompt por defecto del benchmark (suficiente para generar tokens medibles).
pub const DEFAULT_BENCHMARK_PROMPT: &str =
    "Explain how a hash map works in detail, with an example implementation.";

/// Número de corridas por defecto.
pub const DEFAULT_BENCHMARK_RUNS: usize = 3;

/// Tope defensivo de corridas por benchmark.
pub const MAX_BENCHMARK_RUNS: usize = 20;

/// Orquestador de `ai benchmark <instance>`.
pub struct RunAiBenchmark {
    instances: Arc<dyn AiAdapterPort + Send + Sync>,
    engine: Arc<dyn LlmEngine + Send + Sync>,
}

impl RunAiBenchmark {
    /// Construye el caso de uso con el motor LLM ya resuelto para la
    /// instancia (el executor lo construye desde `api_port`).
    pub fn new(
        instances: Arc<dyn AiAdapterPort + Send + Sync>,
        engine: Arc<dyn LlmEngine + Send + Sync>,
    ) -> Self {
        Self { instances, engine }
    }

    /// Ejecuta el benchmark sobre `instance` con `runs` corridas.
    ///
    /// - `model`: modelo a evaluar; si `None` se usa el modelo de la instancia.
    /// - `prompt`: prompt de entrada (vacío → `DEFAULT_BENCHMARK_PROMPT`).
    /// - `runs`: número de corridas (`1..=MAX_BENCHMARK_RUNS`).
    pub async fn run(
        &self,
        instance: &str,
        model: Option<&str>,
        prompt: &str,
        runs: usize,
    ) -> Result<BenchmarkReport> {
        validate_ai_instance_name(instance)?;
        let inst = self.instances.get_instance(instance).await?;
        if inst.api_port.is_none() {
            return Err(EnolaError::ValidationError(format!(
                "La instancia '{instance}' no tiene la API Ollama activa (¿`ai start {instance}`?)"
            )));
        }
        let model = match model {
            Some(m) if !m.trim().is_empty() => m.trim().to_string(),
            _ => inst.model.clone(),
        };
        let prompt = if prompt.trim().is_empty() {
            DEFAULT_BENCHMARK_PROMPT
        } else {
            prompt.trim()
        };
        let runs = runs.clamp(1, MAX_BENCHMARK_RUNS);

        // Tamaño del modelo: del registro si está disponible (best-effort).
        let model_size_bytes = self
            .engine
            .list_models()
            .await
            .unwrap_or_default()
            .into_iter()
            .find(|m| m.name == model || m.name == format!("{model}:latest"))
            .and_then(|m| m.size_bytes);

        let mut samples = Vec::with_capacity(runs);
        for _ in 0..runs {
            let started = Instant::now();
            let resp = self
                .engine
                .chat_tools(
                    &model,
                    &[ChatMessage::user(prompt)],
                    &[], // benchmark mide generación pura, sin tools
                )
                .await?;
            let latency_ms = started.elapsed().as_millis() as u64;
            samples.push(BenchmarkSample {
                latency_ms,
                eval_count: resp.eval_count.or_else(|| {
                    // Fallback honesto: estimar por heurística si no hay métricas.
                    Some(crate::domain::ai_benchmark::estimate_tokens(&resp.content))
                }),
                prompt_eval_count: resp.prompt_eval_count,
                eval_duration_ns: resp.eval_duration_ns,
            });
        }

        Ok(aggregate_samples(
            instance,
            &model,
            samples,
            model_size_bytes,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ai::{AiInstance, AiInstanceStatus};
    use crate::ports::ai::MockAiAdapterPort;
    use crate::ports::llm::{LlmChatResponse, MockLlmEngine};

    fn inst() -> AiInstance {
        AiInstance {
            name: "dev".into(),
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

    #[tokio::test]
    async fn benchmark_runs_n_times_and_aggregates() {
        let mut insts = MockAiAdapterPort::new();
        insts.expect_get_instance().returning(|_| Ok(inst()));
        let mut eng = MockLlmEngine::new();
        eng.expect_list_models().returning(|| Ok(vec![]));
        eng.expect_chat_tools().times(3).returning(|_, _, _| {
            Ok(LlmChatResponse {
                content: "respuesta".into(),
                tool_calls: vec![],
                eval_count: Some(42),
                prompt_eval_count: Some(10),
                eval_duration_ns: Some(1_000_000_000),
            })
        });
        let bench = RunAiBenchmark::new(Arc::new(insts), Arc::new(eng));
        let rep = bench.run("dev", None, "", 3).await.unwrap();
        assert_eq!(rep.model, "llama3.1:8b");
        assert_eq!(rep.samples.len(), 3);
        assert_eq!(rep.total_eval_tokens, 126);
        assert!((rep.tokens_per_sec.unwrap() - 42.0).abs() < 0.01);
    }

    #[tokio::test]
    async fn benchmark_uses_model_override_and_estimates_without_metrics() {
        let mut insts = MockAiAdapterPort::new();
        insts.expect_get_instance().returning(|_| Ok(inst()));
        let mut eng = MockLlmEngine::new();
        eng.expect_list_models().returning(|| Ok(vec![]));
        eng.expect_chat_tools()
            .withf(|model, _, _| model == "qwen3:4b")
            .returning(|_, _, _| {
                Ok(LlmChatResponse {
                    content: "abcd".into(),
                    ..Default::default()
                })
            });
        let bench = RunAiBenchmark::new(Arc::new(insts), Arc::new(eng));
        let rep = bench.run("dev", Some("qwen3:4b"), "hola", 1).await.unwrap();
        assert_eq!(rep.model, "qwen3:4b");
        // Sin eval_count → estimación por heurística (4 chars → 1 token).
        assert_eq!(rep.total_eval_tokens, 1);
    }

    #[tokio::test]
    async fn benchmark_rejects_instance_without_api() {
        let mut insts = MockAiAdapterPort::new();
        insts.expect_get_instance().returning(|_| {
            let mut i = inst();
            i.api_port = None;
            Ok(i)
        });
        let eng = MockLlmEngine::new();
        let bench = RunAiBenchmark::new(Arc::new(insts), Arc::new(eng));
        assert!(bench.run("dev", None, "", 1).await.is_err());
    }
}
