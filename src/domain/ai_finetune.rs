// Fase 14.1 — Dominio puro de datasets de fine-tuning (`ai finetune prepare`).
//
// Formatos soportados: Alpaca, ChatML, ShareGPT y Texto plano — detectados
// por inspección de la primera línea JSONL. Lógica 100% pura: validación de
// esquema, conversión canónica a ChatML y split determinista train/validación.

use crate::domain::error::{EnolaError, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Formato de dataset JSONL soportado.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DatasetFormat {
    /// `{"instruction","input","output"}` — Alpaca.
    Alpaca,
    /// `{"messages":[{"role","content"}]}` — ChatML canónico.
    ChatML,
    /// `{"conversations":[{"from","value"}]}` — ShareGPT.
    ShareGPT,
    /// `{"text":"…"}` — texto plano (pre-entrenamiento).
    Text,
}

impl DatasetFormat {
    /// Parse case-insensitive del nombre de formato.
    pub fn from_str_ci(s: &str) -> Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "alpaca" => Ok(DatasetFormat::Alpaca),
            "chatml" | "chat" => Ok(DatasetFormat::ChatML),
            "sharegpt" | "share-gpt" => Ok(DatasetFormat::ShareGPT),
            "text" | "txt" | "raw" => Ok(DatasetFormat::Text),
            other => Err(EnolaError::ValidationError(format!(
                "Formato de dataset '{other}' desconocido — válidos: alpaca, chatml, sharegpt, text"
            ))),
        }
    }

    /// Nombre canónico del formato.
    pub fn as_str(&self) -> &'static str {
        match self {
            DatasetFormat::Alpaca => "alpaca",
            DatasetFormat::ChatML => "chatml",
            DatasetFormat::ShareGPT => "sharegpt",
            DatasetFormat::Text => "text",
        }
    }
}

/// Mensaje ChatML canónico (forma interna de conversión).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatMlMessage {
    /// Rol: `system` | `user` | `assistant`.
    pub role: String,
    /// Contenido textual.
    pub content: String,
}

/// Un registro JSONL convertido a ChatML.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatMlRecord {
    /// Secuencia de mensajes del ejemplo.
    pub messages: Vec<ChatMlMessage>,
}

/// Estadísticas de validación de un dataset.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DatasetStats {
    /// Formato detectado.
    pub format: DatasetFormat,
    /// Registros válidos.
    pub valid_records: usize,
    /// Líneas totales no vacías inspeccionadas.
    pub total_lines: usize,
    /// Errores de esquema (línea → descripción, sin contenido).
    pub errors: Vec<String>,
}

/// Límite defensivo de líneas JSONL por dataset.
pub const MAX_DATASET_LINES: usize = 200_000;
/// Límite defensivo de bytes por línea JSONL.
pub const MAX_LINE_BYTES: usize = 4 * 1024 * 1024;

/// Detecta el formato inspeccionando la primera línea JSONL válida.
pub fn detect_format(content: &str) -> Result<DatasetFormat> {
    let line = content
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .ok_or_else(|| EnolaError::ValidationError("El dataset está vacío".to_string()))?;
    let v: Value = serde_json::from_str(line)
        .map_err(|e| EnolaError::ValidationError(format!("Primera línea JSONL inválida: {e}")))?;
    if v.get("messages").is_some() {
        return Ok(DatasetFormat::ChatML);
    }
    if v.get("conversations").is_some() {
        return Ok(DatasetFormat::ShareGPT);
    }
    if v.get("instruction").is_some() {
        return Ok(DatasetFormat::Alpaca);
    }
    if v.get("text").is_some() {
        return Ok(DatasetFormat::Text);
    }
    Err(EnolaError::ValidationError(
        "Formato no reconocido: se esperaba 'messages' (chatml), 'conversations' (sharegpt), 'instruction' (alpaca) o 'text'".into(),
    ))
}

/// Valida y convierte una línea individual a `ChatMlRecord` según su formato.
/// Devuelve `Err` descriptivo (nunca panic) para líneas malformadas.
pub fn convert_line(format: DatasetFormat, line: &str) -> Result<ChatMlRecord> {
    let v: Value = serde_json::from_str(line.trim())
        .map_err(|e| EnolaError::ValidationError(format!("JSON inválido: {e}")))?;
    match format {
        DatasetFormat::ChatML => convert_chatml(&v),
        DatasetFormat::Alpaca => convert_alpaca(&v),
        DatasetFormat::ShareGPT => convert_sharegpt(&v),
        DatasetFormat::Text => convert_text(&v),
    }
}

fn convert_chatml(v: &Value) -> Result<ChatMlRecord> {
    let msgs = v["messages"]
        .as_array()
        .ok_or_else(|| EnolaError::ValidationError("'messages' debe ser un array".into()))?;
    if msgs.is_empty() {
        return Err(EnolaError::ValidationError("'messages' vacío".into()));
    }
    let mut out = Vec::with_capacity(msgs.len());
    for m in msgs {
        let role = m["role"]
            .as_str()
            .ok_or_else(|| EnolaError::ValidationError("mensaje sin 'role'".into()))?;
        let content = m["content"]
            .as_str()
            .ok_or_else(|| EnolaError::ValidationError("mensaje sin 'content'".into()))?;
        if !matches!(role, "system" | "user" | "assistant") {
            return Err(EnolaError::ValidationError(format!(
                "rol inválido '{role}'"
            )));
        }
        out.push(ChatMlMessage {
            role: role.to_string(),
            content: content.to_string(),
        });
    }
    Ok(ChatMlRecord { messages: out })
}

fn convert_alpaca(v: &Value) -> Result<ChatMlRecord> {
    let instruction = v["instruction"]
        .as_str()
        .ok_or_else(|| EnolaError::ValidationError("registro sin 'instruction'".into()))?;
    let output = v["output"]
        .as_str()
        .ok_or_else(|| EnolaError::ValidationError("registro sin 'output'".into()))?;
    if instruction.trim().is_empty() || output.trim().is_empty() {
        return Err(EnolaError::ValidationError(
            "instruction/output no pueden estar vacíos".into(),
        ));
    }
    let input = v["input"].as_str().unwrap_or("").trim();
    let user = if input.is_empty() {
        instruction.to_string()
    } else {
        format!("{instruction}\n\n{input}")
    };
    Ok(ChatMlRecord {
        messages: vec![
            ChatMlMessage {
                role: "user".into(),
                content: user,
            },
            ChatMlMessage {
                role: "assistant".into(),
                content: output.to_string(),
            },
        ],
    })
}

fn convert_sharegpt(v: &Value) -> Result<ChatMlRecord> {
    let convs = v["conversations"]
        .as_array()
        .ok_or_else(|| EnolaError::ValidationError("'conversations' debe ser un array".into()))?;
    if convs.is_empty() {
        return Err(EnolaError::ValidationError("'conversations' vacío".into()));
    }
    let mut out = Vec::with_capacity(convs.len());
    for c in convs {
        let from = c["from"]
            .as_str()
            .ok_or_else(|| EnolaError::ValidationError("turno sin 'from'".into()))?;
        let value = c["value"]
            .as_str()
            .ok_or_else(|| EnolaError::ValidationError("turno sin 'value'".into()))?;
        let role = match from {
            "human" | "user" => "user",
            "gpt" | "assistant" => "assistant",
            "system" => "system",
            other => {
                return Err(EnolaError::ValidationError(format!(
                    "rol sharegpt desconocido '{other}'"
                )))
            }
        };
        out.push(ChatMlMessage {
            role: role.to_string(),
            content: value.to_string(),
        });
    }
    Ok(ChatMlRecord { messages: out })
}

fn convert_text(v: &Value) -> Result<ChatMlRecord> {
    let text = v["text"]
        .as_str()
        .ok_or_else(|| EnolaError::ValidationError("registro sin 'text'".into()))?;
    if text.trim().is_empty() {
        return Err(EnolaError::ValidationError("'text' vacío".into()));
    }
    // Texto plano se envuelve como un único turno user→assistant? No:
    // para pre-training se emite como user único (el trainer lo trata raw).
    Ok(ChatMlRecord {
        messages: vec![ChatMlMessage {
            role: "user".into(),
            content: text.to_string(),
        }],
    })
}

/// Valida un dataset completo: detecta formato, convierte cada línea y
/// recolecta errores por línea (sin abortar el análisis completo).
/// Devuelve `(stats, registros_chatml_validos)`.
pub fn validate_dataset(content: &str) -> Result<(DatasetStats, Vec<ChatMlRecord>)> {
    let format = detect_format(content)?;
    let mut stats = DatasetStats {
        format,
        valid_records: 0,
        total_lines: 0,
        errors: Vec::new(),
    };
    let mut records = Vec::new();
    for (n, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        stats.total_lines += 1;
        if stats.total_lines > MAX_DATASET_LINES {
            stats.errors.push(format!(
                "dataset excede el máximo de {MAX_DATASET_LINES} líneas"
            ));
            break;
        }
        if line.len() > MAX_LINE_BYTES {
            stats
                .errors
                .push(format!("línea {}: excede {} bytes", n + 1, MAX_LINE_BYTES));
            continue;
        }
        match convert_line(format, line) {
            Ok(rec) => {
                stats.valid_records += 1;
                records.push(rec);
            }
            Err(e) => stats.errors.push(format!("línea {}: {e}", n + 1)),
        }
    }
    Ok((stats, records))
}

/// Split determinista train/validación: cada N-ésima línea válida va a
/// `val` (ratio `1/val_every`). Determinista (sin RNG) para reproducibilidad.
/// `val_ratio_percent` típico: 5-20. Devuelve `(train, validation)`.
pub fn split_dataset<T: Clone>(records: &[T], val_ratio_percent: usize) -> (Vec<T>, Vec<T>) {
    let ratio = val_ratio_percent.clamp(1, 50);
    let every = (100 / ratio).max(2); // p.ej. 10% → cada 10
    let mut train = Vec::new();
    let mut val = Vec::new();
    for (i, r) in records.iter().enumerate() {
        if (i + 1) % every == 0 {
            val.push(r.clone());
        } else {
            train.push(r.clone());
        }
    }
    (train, val)
}

/// Serializa registros ChatML a JSONL (una línea `{"messages":[…]}` por registro).
pub fn records_to_jsonl(records: &[ChatMlRecord]) -> String {
    let mut out = String::new();
    for r in records {
        out.push_str(&serde_json::to_string(r).unwrap_or_default());
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_format_recognizes_all_four() {
        assert_eq!(
            detect_format(r#"{"instruction":"a","output":"b"}"#).unwrap(),
            DatasetFormat::Alpaca
        );
        assert_eq!(
            detect_format(r#"{"messages":[{"role":"user","content":"x"}]}"#).unwrap(),
            DatasetFormat::ChatML
        );
        assert_eq!(
            detect_format(r#"{"conversations":[{"from":"human","value":"x"}]}"#).unwrap(),
            DatasetFormat::ShareGPT
        );
        assert_eq!(
            detect_format(r#"{"text":"raw text"}"#).unwrap(),
            DatasetFormat::Text
        );
        assert!(detect_format("").is_err());
        assert!(detect_format(r#"{"other":1}"#).is_err());
    }

    #[test]
    fn alpaca_merges_input_into_user_turn() {
        let r = convert_line(
            DatasetFormat::Alpaca,
            r#"{"instruction":"Explain X","input":"Given Y","output":"Z"}"#,
        )
        .unwrap();
        assert_eq!(r.messages.len(), 2);
        assert_eq!(r.messages[0].role, "user");
        assert_eq!(r.messages[0].content, "Explain X\n\nGiven Y");
        assert_eq!(r.messages[1].role, "assistant");
        assert_eq!(r.messages[1].content, "Z");
    }

    #[test]
    fn alpaca_without_input_uses_instruction_alone() {
        let r = convert_line(
            DatasetFormat::Alpaca,
            r#"{"instruction":"Do it","output":"Done"}"#,
        )
        .unwrap();
        assert_eq!(r.messages[0].content, "Do it");
    }

    #[test]
    fn sharegpt_maps_roles() {
        let r = convert_line(
            DatasetFormat::ShareGPT,
            r#"{"conversations":[{"from":"system","value":"s"},{"from":"human","value":"h"},{"from":"gpt","value":"g"}]}"#,
        )
        .unwrap();
        assert_eq!(r.messages[0].role, "system");
        assert_eq!(r.messages[1].role, "user");
        assert_eq!(r.messages[2].role, "assistant");
    }

    #[test]
    fn malformed_lines_are_rejected_not_panic() {
        assert!(convert_line(DatasetFormat::Alpaca, r#"{"output":"no instruction"}"#).is_err());
        assert!(convert_line(DatasetFormat::ChatML, r#"{"messages":[]}"#).is_err());
        assert!(convert_line(
            DatasetFormat::ChatML,
            r#"{"messages":[{"role":"nope","content":"x"}]}"#
        )
        .is_err());
        assert!(convert_line(DatasetFormat::Text, r#"{"text":"  "}"#).is_err());
        assert!(convert_line(
            DatasetFormat::ShareGPT,
            r#"{"conversations":[{"from":"weird","value":"x"}]}"#
        )
        .is_err());
    }

    #[test]
    fn validate_dataset_collects_errors_and_valid() {
        let content = r#"{"instruction":"a","output":"b"}
{"output":"missing instruction"}
{"instruction":"c","output":"d"}
not json at all"#;
        let (stats, records) = validate_dataset(content).unwrap();
        assert_eq!(stats.format, DatasetFormat::Alpaca);
        assert_eq!(stats.total_lines, 4);
        assert_eq!(stats.valid_records, 2);
        assert_eq!(stats.errors.len(), 2);
        assert_eq!(records.len(), 2);
    }

    #[test]
    fn split_is_deterministic_and_respects_ratio() {
        let recs: Vec<ChatMlRecord> = (0..100)
            .map(|i| ChatMlRecord {
                messages: vec![ChatMlMessage {
                    role: "user".into(),
                    content: format!("r{i}"),
                }],
            })
            .collect();
        let (train, val) = split_dataset(&recs, 10);
        assert_eq!(train.len(), 90);
        assert_eq!(val.len(), 10);
        // determinista: mismo input → mismo split
        let (train2, val2) = split_dataset(&recs, 10);
        assert_eq!(train, train2);
        assert_eq!(val, val2);
        // ratios límite
        let (_t, v) = split_dataset(&recs, 50);
        assert_eq!(v.len(), 50);
        let (t2, _) = split_dataset(&recs, 1);
        assert!(t2.len() >= 98);
    }

    #[test]
    fn records_roundtrip_through_jsonl() {
        let recs = vec![ChatMlRecord {
            messages: vec![ChatMlMessage {
                role: "user".into(),
                content: "hola".into(),
            }],
        }];
        let jl = records_to_jsonl(&recs);
        let (stats, back) = validate_dataset(&jl).unwrap();
        assert_eq!(stats.valid_records, 1);
        assert_eq!(back, recs);
    }
}

/// Estado de un job de fine-tuning.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FinetuneJobStatus {
    /// Aprovisionado, pendiente de arrancar.
    Pending,
    /// Entrenando en el contenedor.
    Running,
    /// Completado con éxito (adapter en `/out`).
    Completed,
    /// Falló (ver `error`).
    Failed,
}

/// Job de fine-tuning registrado por instancia (`finetune/jobs.json`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FinetuneJob {
    /// Id único del job.
    pub id: String,
    /// Instancia IA asociada.
    pub instance: String,
    /// Modelo base.
    pub base_model: String,
    /// Ruta del dataset ChatML (host).
    pub dataset: String,
    /// Modo low-memory.
    pub low_memory: bool,
    /// Estado actual.
    pub status: FinetuneJobStatus,
    /// Contenedor de entrenamiento (`enola-ai-<n>-finetune-<job>`).
    pub container: String,
    /// ISO-8601 de creación.
    pub created_at: String,
    /// ISO-8601 de finalización (si aplica).
    pub finished_at: Option<String>,
    /// Error si `Failed`.
    pub error: Option<String>,
    /// Nombre del modelo resultante previsto (`<instance>-ft`), salvo que el
    /// job se lanzara con `--no-register`.
    pub result_model: Option<String>,
    /// `true` cuando `ai finetune register` ya materializó el modelo en la
    /// biblioteca y en Ollama. `#[serde(default)]` tolera `jobs.json` previos.
    #[serde(default)]
    pub registered: bool,
}

/// Validación previa de un job: tamaño del dataset vs. VRAM disponible
/// (heurística honesta: ~1.5× el tamaño del dataset en bytes + ~6 GiB de
/// base LoRA mínimo). Devuelve advertencias accionables, no errores.
pub fn finetune_warnings(
    dataset_bytes: u64,
    records: usize,
    free_vram_mb: Option<u64>,
) -> Vec<String> {
    let mut out = Vec::new();
    if records < 50 {
        out.push(format!(
            "dataset pequeño ({records} registros): LoRA necesita típicamente ≥100"
        ));
    }
    let est_mb = (dataset_bytes * 3 / 2) / (1024 * 1024) + 6144;
    match free_vram_mb {
        Some(free) if free < est_mb => out.push(format!(
            "VRAM libre (~{} MB) puede ser insuficiente (est. ~{est_mb} MB): usa --low-memory",
            free
        )),
        None => {
            out.push("sin GPU detectada: el entrenamiento correrá en CPU (muy lento)".to_string())
        }
        _ => {}
    }
    out
}
