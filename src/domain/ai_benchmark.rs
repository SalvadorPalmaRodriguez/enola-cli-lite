// Fase 13.1 — Tipos puros del benchmark local (`ai benchmark`).
//
// Dominio PURO: sin I/O, sin tokio, sin reqwest. Solo agregación
// determinista de muestras de generación.

use serde::{Deserialize, Serialize};

/// Muestra individual de una corrida de benchmark.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BenchmarkSample {
    /// Latencia de pared en milisegundos de la llamada completa.
    pub latency_ms: u64,
    /// Tokens generados reportados por el motor (`eval_count`).
    pub eval_count: Option<u64>,
    /// Tokens del prompt evaluados (`prompt_eval_count`).
    pub prompt_eval_count: Option<u64>,
    /// Nanosegundos de generación reportados por el motor (`eval_duration`).
    pub eval_duration_ns: Option<u64>,
}

/// Informe agregado de `ai benchmark`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BenchmarkReport {
    /// Instancia IA evaluada.
    pub instance: String,
    /// Modelo evaluado.
    pub model: String,
    /// Muestras individuales (una por corrida).
    pub samples: Vec<BenchmarkSample>,
    /// Tokens/segundo de generación agregados (suma eval_tokens / suma eval_duration).
    /// `None` si el motor no reporta métricas (fallback a wall-clock).
    pub tokens_per_sec: Option<f64>,
    /// Tokens/segundo calculados con wall-clock (fallback si no hay eval_duration).
    pub wall_tokens_per_sec: Option<f64>,
    /// Latencia media de pared en milisegundos.
    pub avg_latency_ms: u64,
    /// Suma total de tokens generados en todas las corridas.
    pub total_eval_tokens: u64,
    /// Tamaño del modelo en bytes (del registro, si se conoce).
    pub model_size_bytes: Option<u64>,
    /// Nota operativa (p.ej. motor sin métricas de eval).
    pub note: Option<String>,
}

/// Estimación aproximada de tokens cuando el motor no reporta `eval_count`.
/// Heurística: ~4 caracteres por token (promedio de tokenizadores BPE latinos).
pub fn estimate_tokens(text: &str) -> u64 {
    (text.len() as u64).div_ceil(4)
}

/// Agrega muestras en un informe. Lógica pura y determinista.
///
/// - `tokens_per_sec` usa `eval_duration` (tiempo real de generación) si
///   TODAS las muestras lo reportan; si no, se calcula `wall_tokens_per_sec`
///   con la latencia de pared (menos preciso: incluye red y encolado).
pub fn aggregate_samples(
    instance: &str,
    model: &str,
    samples: Vec<BenchmarkSample>,
    model_size_bytes: Option<u64>,
) -> BenchmarkReport {
    let runs = samples.len().max(1) as u64;
    let total_latency: u64 = samples.iter().map(|s| s.latency_ms).sum();
    let avg_latency_ms = total_latency / runs;

    let all_have_eval = !samples.is_empty()
        && samples
            .iter()
            .all(|s| s.eval_count.is_some() && s.eval_duration_ns.is_some());
    let total_eval_tokens: u64 = samples.iter().filter_map(|s| s.eval_count).sum();
    let total_eval_ns: u64 = samples.iter().filter_map(|s| s.eval_duration_ns).sum();

    let tokens_per_sec = if all_have_eval && total_eval_ns > 0 {
        Some(total_eval_tokens as f64 / (total_eval_ns as f64 / 1e9))
    } else {
        None
    };

    // Fallback con wall-clock si el motor no reporta duración.
    let wall_tokens_per_sec = if tokens_per_sec.is_none()
        && !samples.is_empty()
        && total_latency > 0
        && total_eval_tokens > 0
    {
        Some(total_eval_tokens as f64 / (total_latency as f64 / 1000.0))
    } else {
        None
    };

    let note = if all_have_eval {
        None
    } else if wall_tokens_per_sec.is_some() {
        Some(
            "motor sin eval_duration: tokens/s calculados con wall-clock (incluye latencia de red)"
                .to_string(),
        )
    } else {
        Some("motor sin métricas de evaluación: no se pudo calcular tokens/s".to_string())
    };

    BenchmarkReport {
        instance: instance.to_string(),
        model: model.to_string(),
        samples,
        tokens_per_sec,
        wall_tokens_per_sec,
        avg_latency_ms,
        total_eval_tokens,
        model_size_bytes,
        note,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(lat: u64, eval: Option<u64>, dur_ns: Option<u64>) -> BenchmarkSample {
        BenchmarkSample {
            latency_ms: lat,
            eval_count: eval,
            prompt_eval_count: Some(10),
            eval_duration_ns: dur_ns,
        }
    }

    #[test]
    fn aggregate_uses_eval_duration_for_tokens_per_sec() {
        // 2 corridas: 100 tok en 2s y 100 tok en 1s → 200 tok / 3s = 66.67 t/s
        let rep = aggregate_samples(
            "dev",
            "llama3.1:8b",
            vec![
                sample(2100, Some(100), Some(2_000_000_000)),
                sample(1100, Some(100), Some(1_000_000_000)),
            ],
            Some(4_900_000_000),
        );
        let tps = rep.tokens_per_sec.unwrap();
        assert!((tps - 66.666).abs() < 0.1, "tps={tps}");
        assert!(rep.wall_tokens_per_sec.is_none());
        assert_eq!(rep.total_eval_tokens, 200);
        assert_eq!(rep.avg_latency_ms, 1600);
        assert_eq!(rep.model_size_bytes, Some(4_900_000_000));
        assert!(rep.note.is_none());
    }

    #[test]
    fn aggregate_falls_back_to_wall_clock_without_eval_duration() {
        let rep = aggregate_samples(
            "dev",
            "m",
            vec![sample(2000, Some(50), None), sample(2000, Some(50), None)],
            None,
        );
        assert!(rep.tokens_per_sec.is_none());
        let wtps = rep.wall_tokens_per_sec.unwrap();
        assert!((wtps - 25.0).abs() < 0.01, "wtps={wtps}"); // 100 tok / 4s
        assert!(rep.note.unwrap().contains("wall-clock"));
    }

    #[test]
    fn aggregate_without_any_metrics_notes_missing() {
        let rep = aggregate_samples("dev", "m", vec![sample(100, None, None)], None);
        assert!(rep.tokens_per_sec.is_none());
        assert!(rep.wall_tokens_per_sec.is_none());
        assert!(rep.note.unwrap().contains("sin métricas"));
    }

    #[test]
    fn estimate_tokens_uses_four_char_heuristic() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcdefghi"), 3); // ceil(9/4)
    }
}
