// Fase 14 — Casos de uso `ai finetune` (prepare/validate/start/status).
//
// Hexagonal: ports (`AiAdapterPort`, `HardwareProbePort`, `ContainerPort`,
// `AiFinetuneJobsPort`) + dominio puro (`ai_finetune`). El script LoRA
// embebido corre DENTRO del contenedor de entrenamiento — nunca en el host.

use crate::domain::ai::validate_ai_instance_name;
use crate::domain::ai_finetune::{
    finetune_warnings, records_to_jsonl, split_dataset, validate_dataset, DatasetFormat,
    FinetuneJob, FinetuneJobStatus,
};
use crate::domain::ai_model;
use crate::domain::error::{EnolaError, Result};
use crate::ports::ai::{
    AiAdapterPort, AiModelLibraryPort, TokenizerDownloadPort, TokenizerDownloadRequest,
};
use crate::ports::container::{ContainerConfig, ContainerPort};
use crate::ports::finetune::AiFinetuneJobsPort;
use crate::ports::hardware::HardwareProbePort;
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Imagen de entrenamiento (CUDA runtime con transformers/peft/datasets).
const TRAIN_IMAGE: &str = "enola-finetune:latest";
/// Límites del contenedor de entrenamiento (R20: muy por encima del default 2 GiB).
const TRAIN_MEMORY_BYTES: i64 = 16 * 1024 * 1024 * 1024; // 16 GiB
const TRAIN_PIDS_LIMIT: i64 = 4096;
/// Máximo defensivo de tamaño de dataset leído (32 MiB).
const MAX_DATASET_BYTES: u64 = 32 * 1024 * 1024;

/// Informe de `ai finetune prepare`.
#[derive(Debug, Clone, Serialize)]
pub struct FinetunePrepareReport {
    /// Formato detectado/forzado.
    pub format: DatasetFormat,
    /// Registros válidos convertidos.
    pub valid_records: usize,
    /// Registros en train.
    pub train_records: usize,
    /// Registros en validación.
    pub val_records: usize,
    /// Errores de esquema por línea.
    pub errors: Vec<String>,
    /// Ruta del train.jsonl generado.
    pub train_path: String,
    /// Ruta del val.jsonl generado.
    pub val_path: String,
}

/// Chequeo individual del pre-flight `ai finetune validate`.
#[derive(Debug, Clone, Serialize)]
pub struct FinetuneCheck {
    /// Nombre del chequeo.
    pub name: String,
    /// `true` si pasa.
    pub ok: bool,
    /// Detalle accionable.
    pub detail: String,
}

/// Informe de `ai finetune validate`.
#[derive(Debug, Clone, Serialize)]
pub struct FinetuneValidateReport {
    /// Dataset válido (≥1 registro).
    pub dataset_ok: bool,
    /// Registros válidos.
    pub valid_records: usize,
    /// Errores de esquema.
    pub schema_errors: Vec<String>,
    /// Chequeos ejecutados (GPU, tamaño, compatibilidad).
    pub checks: Vec<FinetuneCheck>,
    /// Advertencias no bloqueantes.
    pub warnings: Vec<String>,
}

/// Orquestador de `ai finetune <sub>`.
pub struct ManageFinetune {
    instances: Arc<dyn AiAdapterPort + Send + Sync>,
    hardware: Arc<dyn HardwareProbePort + Send + Sync>,
    containers: Arc<dyn ContainerPort + Send + Sync>,
    jobs: Arc<dyn AiFinetuneJobsPort + Send + Sync>,
    /// Biblioteca de modelos GGUF: resuelve el modelo base y el tokenizer.
    library: Arc<dyn AiModelLibraryPort + Send + Sync>,
    /// Descarga bajo demanda del tokenizer HF (`--tokenizer`), host-side.
    tokenizer: Option<Arc<dyn TokenizerDownloadPort + Send + Sync>>,
    /// Directorio base de instancias (`/srv/enola-ai`) — datasets y scripts.
    base_dir: PathBuf,
}

impl ManageFinetune {
    /// Construye el caso de uso con los puertos inyectados.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        instances: Arc<dyn AiAdapterPort + Send + Sync>,
        hardware: Arc<dyn HardwareProbePort + Send + Sync>,
        containers: Arc<dyn ContainerPort + Send + Sync>,
        jobs: Arc<dyn AiFinetuneJobsPort + Send + Sync>,
        library: Arc<dyn AiModelLibraryPort + Send + Sync>,
        tokenizer: Option<Arc<dyn TokenizerDownloadPort + Send + Sync>>,
        base_dir: PathBuf,
    ) -> Self {
        Self {
            instances,
            hardware,
            containers,
            jobs,
            library,
            tokenizer,
            base_dir,
        }
    }

    /// `ai finetune prepare <file>` — valida, convierte a ChatML y genera
    /// `train.jsonl`/`val.jsonl` con split determinista.
    pub async fn prepare(
        &self,
        dataset: &str,
        format: Option<DatasetFormat>,
        out_dir: Option<&str>,
        val_ratio: usize,
    ) -> Result<FinetunePrepareReport> {
        let content = read_limited(dataset)?;
        let (detected, records) = match format {
            Some(f) => {
                // Formato forzado: convertir cada línea con ese formato.
                let (mut stats_errs, mut recs) = (Vec::new(), Vec::new());
                for (n, line) in content.lines().enumerate() {
                    let line = line.trim();
                    if line.is_empty() {
                        continue;
                    }
                    match crate::domain::ai_finetune::convert_line(f, line) {
                        Ok(r) => recs.push(r),
                        Err(e) => stats_errs.push(format!("línea {}: {e}", n + 1)),
                    }
                }
                (
                    crate::domain::ai_finetune::DatasetStats {
                        format: f,
                        valid_records: recs.len(),
                        total_lines: recs.len() + stats_errs.len(),
                        errors: stats_errs,
                    },
                    recs,
                )
            }
            None => validate_dataset(&content)?,
        };
        if records.is_empty() {
            return Err(EnolaError::ValidationError(format!(
                "El dataset '{dataset}' no tiene registros válidos ({:?} formato)",
                detected.format
            )));
        }
        let (train, val) = split_dataset(&records, val_ratio);
        let out = out_dir.map(PathBuf::from).unwrap_or_else(|| {
            Path::new(dataset)
                .parent()
                .unwrap_or(Path::new("."))
                .to_path_buf()
        });
        std::fs::create_dir_all(&out).map_err(|e| {
            EnolaError::InfrastructureError(format!("No se pudo crear '{}': {e}", out.display()))
        })?;
        let train_path = out.join("train.jsonl");
        let val_path = out.join("val.jsonl");
        std::fs::write(&train_path, records_to_jsonl(&train)).map_err(|e| {
            EnolaError::InfrastructureError(format!(
                "No se pudo escribir '{}': {e}",
                train_path.display()
            ))
        })?;
        std::fs::write(&val_path, records_to_jsonl(&val)).map_err(|e| {
            EnolaError::InfrastructureError(format!(
                "No se pudo escribir '{}': {e}",
                val_path.display()
            ))
        })?;
        Ok(FinetunePrepareReport {
            format: detected.format,
            valid_records: detected.valid_records,
            train_records: train.len(),
            val_records: val.len(),
            errors: detected.errors,
            train_path: train_path.display().to_string(),
            val_path: val_path.display().to_string(),
        })
    }

    /// `ai finetune validate <file>` — pre-flight: esquema + tamaño +
    /// VRAM (HardwareProbePort) + modelo base, ANTES de gastar cómputo.
    pub async fn validate(
        &self,
        dataset: &str,
        base_model: Option<&str>,
    ) -> Result<FinetuneValidateReport> {
        let content = read_limited(dataset)?;
        let (stats, records) = validate_dataset(&content)?;
        let mut checks = Vec::new();
        checks.push(FinetuneCheck {
            name: "dataset".into(),
            ok: stats.valid_records > 0,
            detail: format!(
                "{} registros válidos de {} líneas ({:?})",
                stats.valid_records, stats.total_lines, stats.format
            ),
        });
        checks.push(FinetuneCheck {
            name: "schema_errors".into(),
            ok: stats.errors.is_empty(),
            detail: format!("{} líneas con error de esquema", stats.errors.len()),
        });

        // GPU/VRAM vía HardwareProbePort (no nvidia-smi directo — R21).
        let specs = self.hardware.probe().await;
        let (gpu_check, free_vram) = match &specs {
            Ok(s) if !s.gpus.is_empty() => {
                let free = s.gpus.iter().map(|g| g.vram_free_mb).max();
                (
                    FinetuneCheck {
                        name: "gpu".into(),
                        ok: true,
                        detail: format!(
                            "{} GPU(s), max VRAM libre {} MB",
                            s.gpus.len(),
                            free.unwrap_or(0)
                        ),
                    },
                    free,
                )
            }
            _ => (
                FinetuneCheck {
                    name: "gpu".into(),
                    ok: false,
                    detail: "sin GPU NVIDIA detectada (entrenamiento CPU — muy lento)".into(),
                },
                None,
            ),
        };
        checks.push(gpu_check);
        if let Some(bm) = base_model {
            checks.push(FinetuneCheck {
                name: "base_model".into(),
                ok: !bm.trim().is_empty(),
                detail: format!("modelo base '{bm}' (se verificará en el registro al iniciar)"),
            });
        }
        let warnings = finetune_warnings(content.len() as u64, records.len(), free_vram);
        Ok(FinetuneValidateReport {
            dataset_ok: stats.valid_records > 0,
            valid_records: stats.valid_records,
            schema_errors: stats.errors,
            checks,
            warnings,
        })
    }

    /// `ai finetune start <instance>` — aprovisiona el contenedor de
    /// entrenamiento (`enola-ai-<n>-finetune-<job>`) sobre `ContainerPort`
    /// con GPU + volúmenes de datos/scripts/out, registra el job y arranca.
    #[allow(clippy::too_many_arguments)]
    pub async fn start(
        &self,
        instance: &str,
        dataset: &str,
        base_model: Option<&str>,
        tokenizer: Option<&str>,
        low_memory: bool,
        no_register: bool,
        force: bool,
    ) -> Result<FinetuneJob> {
        if !force {
            return Err(EnolaError::ValidationError(
                "`ai finetune start` lanza horas de cómputo GPU: requiere --force".into(),
            ));
        }
        validate_ai_instance_name(instance)?;
        let inst = self.instances.get_instance(instance).await?;

        // El dataset debe estar en formato ChatML preparado (fail-fast barato:
        // antes de resolver el modelo o descargar tokenizer).
        let content = read_limited(dataset)?;
        let (stats, _records) = validate_dataset(&content)?;
        if stats.format != DatasetFormat::ChatML {
            return Err(EnolaError::ValidationError(format!(
                "El dataset debe estar en formato ChatML ({:?} detectado) — prepara con `ai finetune prepare`",
                stats.format
            )));
        }

        // S4: resolver el modelo base contra la biblioteca de GGUF de la
        // instancia. `--base-model` acepta `name` o `.gguf`; por defecto se
        // usa `inst.model` emparejado por `name`.
        let models = self.library.list_library_models(instance).await?;
        let resolved = ai_model::resolve_base_model(&models, base_model, &inst.model)?;

        // Pre-validación con el HEADER GGUF real: arquitectura y cuantización
        // deben ser cargables por `transformers` (gguf_file). El contenedor no
        // tiene red, así que esto evita lanzar un job que fallará al cargar.
        let gguf_meta = self
            .library
            .read_gguf_metadata(instance, &resolved.filename)
            .await?;
        if !ai_model::gguf_load_supported(gguf_meta.architecture.as_deref(), gguf_meta.file_type) {
            return Err(EnolaError::ValidationError(format!(
                "El GGUF '{}' no se puede cargar para fine-tuning: arquitectura '{}' o cuantización '{}' no soportada — cuantizaciones soportadas: {}",
                resolved.filename,
                gguf_meta.architecture.as_deref().unwrap_or("(desconocida)"),
                gguf_meta
                    .file_type
                    .map(|t| t.to_string())
                    .unwrap_or_else(|| "(desconocida)".to_string()),
                ai_model::GGUF_SUPPORTED_QUANT_HINT,
            )));
        }

        // Tokenizer HF (S6): `--tokenizer <org/repo>` sobreescribe el repo del
        // modelo base. Debe estar descargado en el host para montarlo en el
        // contenedor sin red; si falta, se descarga bajo demanda (host-side).
        let repo = match tokenizer.map(str::trim).filter(|s| !s.is_empty()) {
            Some(repo) => Some(ai_model::validate_hf_repo(repo)?),
            None => resolved.hf_repo.clone(),
        };
        let tokenizer_dir = match repo {
            Some(repo) => {
                let dir = self.library.resolve_tokenizer_dir(instance, &repo).await?;
                if !dir.join("tokenizer_config.json").is_file() {
                    let downloader = self.tokenizer.as_ref().ok_or_else(|| {
                        EnolaError::ValidationError(format!(
                            "El tokenizer de '{repo}' no está descargado y la descarga no está disponible — descárgalo con `ai model download {instance} {repo}`"
                        ))
                    })?;
                    downloader
                        .download_tokenizer(TokenizerDownloadRequest {
                            repo: repo.clone(),
                            destination: dir.clone(),
                            use_tor: false,
                        })
                        .await
                        .map_err(|e| {
                            EnolaError::ValidationError(format!(
                                "No se pudo descargar el tokenizer de '{repo}': {e}"
                            ))
                        })?;
                }
                dir
            }
            None => {
                return Err(EnolaError::ValidationError(format!(
                    "El modelo '{}' no tiene repositorio HuggingFace asociado (sin tokenizer) — indica uno con --tokenizer <org/repo>",
                    resolved.name
                )));
            }
        };
        // Ruta host del GGUF para montarlo como volumen (single-file bind).
        let gguf_host =
            ai_model::resolve_library_path(&self.base_dir, instance, &resolved.filename)?;
        let base_model = resolved.name.clone();

        let job_id = format!(
            "ft{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() % 100_000)
                .unwrap_or(0)
        );
        let container_name = format!("enola-ai-{instance}-finetune-{job_id}");
        let ft_dir = self.base_dir.join(instance).join("finetune").join(&job_id);
        let data_dir = ft_dir.join("data");
        let scripts_dir = ft_dir.join("scripts");
        let out_dir = ft_dir.join("out");
        // El contenedor monta el rootfs en read-only (SEC-019) y solo escribe
        // en /out: /tmp y la caché HF van a subdirectorios de ese volumen.
        let tmp_dir = out_dir.join("tmp");
        let hf_home = out_dir.join(".hf");
        for d in [&data_dir, &scripts_dir, &out_dir, &tmp_dir, &hf_home] {
            std::fs::create_dir_all(d).map_err(|e| {
                EnolaError::InfrastructureError(format!("mkdir '{}': {e}", d.display()))
            })?;
        }
        // Dataset copiado al volumen del job (aislado del origen).
        std::fs::copy(dataset, data_dir.join("train.jsonl")).map_err(|e| {
            EnolaError::InfrastructureError(format!("copiar dataset a '{dataset}': {e}"))
        })?;
        // Script LoRA embebido → runtime dir root-owned (no /tmp — SEC-001).
        crate::infrastructure::finetune_scripts::ensure_finetune_scripts(&scripts_dir)?;

        let mut env = HashMap::new();
        env.insert("LOW_MEMORY".into(), low_memory.to_string());
        // El rootfs es read-only: directorios de escritura dentro del volumen /out.
        env.insert("TMPDIR".into(), "/out/tmp".to_string());
        env.insert("HF_HOME".into(), "/out/.hf".to_string());
        // Sin red (`network: None`): fallar rápido si se pide un modelo remoto,
        // en vez de colgarse en un timeout de red.
        env.insert("HF_HUB_OFFLINE".into(), "1".to_string());
        env.insert("TRANSFORMERS_OFFLINE".into(), "1".to_string());
        let mut args = vec![
            "python3".to_string(),
            "/opt/finetune/train_lora.py".to_string(),
            "--train".to_string(),
            "/data/train.jsonl".to_string(),
            "--out".to_string(),
            "/out".to_string(),
            "--gguf".to_string(),
            "/model.gguf".to_string(),
            "--tokenizer".to_string(),
            "/tokenizer".to_string(),
        ];
        if low_memory {
            args.push("--low-memory".to_string());
        }
        let mut volumes = HashMap::new();
        volumes.insert(data_dir.to_string_lossy().to_string(), "/data".to_string());
        volumes.insert(
            scripts_dir.to_string_lossy().to_string(),
            "/opt/finetune".to_string(),
        );
        volumes.insert(out_dir.to_string_lossy().to_string(), "/out".to_string());
        // GGUF (single-file bind) + tokenizer (directorio) montados read-only de facto.
        volumes.insert(
            gguf_host.to_string_lossy().to_string(),
            "/model.gguf".to_string(),
        );
        volumes.insert(
            tokenizer_dir.to_string_lossy().to_string(),
            "/tokenizer".to_string(),
        );

        let mut gpu = false;
        if let Ok(specs) = self.hardware.probe().await {
            gpu = !specs.gpus.is_empty();
        }
        let cfg = ContainerConfig {
            name: container_name.clone(),
            image: TRAIN_IMAGE.to_string(),
            command: Some(args),
            env,
            ports: HashMap::new(), // sin puertos: solo cómputo
            volumes,
            network: None, // sin red: entrenamiento aislado del stack
            restart_policy: Some("no".to_string()),
            gpu_support: gpu,
            auto_remove: false,
            working_dir: None,
            security_opt: crate::infrastructure::security_opt::build_default_security_opt(vec![]),
            memory_limit: Some(TRAIN_MEMORY_BYTES),
            nano_cpus: None,
            pids_limit: Some(TRAIN_PIDS_LIMIT),
            secrets: HashMap::new(),
            read_only_rootfs: true, // el train escribe solo en /out y /tmp
            no_new_privileges: true,
            cap_drop: vec!["ALL".to_string()],
            cap_add: Vec::new(),
            image_digest: None,
        };
        self.containers.create_container(cfg).await?;
        self.containers.start_container(&container_name).await?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs().to_string())
            .unwrap_or_default();
        let job = FinetuneJob {
            id: job_id,
            instance: instance.to_string(),
            base_model,
            dataset: dataset.to_string(),
            low_memory,
            status: FinetuneJobStatus::Running,
            container: container_name,
            created_at: now,
            finished_at: None,
            error: None,
            result_model: if no_register {
                None
            } else {
                Some(format!("{instance}-ft"))
            },
        };
        self.jobs.add_job(job.clone()).await?;
        Ok(job)
    }

    /// `ai finetune status <instance>` — jobs registrados + estado del
    /// contenedor si sigue corriendo.
    pub async fn status(&self, instance: &str) -> Result<Vec<FinetuneJob>> {
        validate_ai_instance_name(instance)?;
        // La instancia debe existir: `status` sobre una instancia inexistente
        // es un error, no una lista vacía (verificado por test_finetune.sh #11).
        self.instances.get_instance(instance).await?;
        self.jobs.list_jobs(instance).await
    }
}

/// Lee un fichero JSONL con límite defensivo de tamaño.
fn read_limited(path: &str) -> Result<String> {
    let meta = std::fs::metadata(path)
        .map_err(|e| EnolaError::ValidationError(format!("No se pudo leer '{path}': {e}")))?;
    if meta.len() > MAX_DATASET_BYTES {
        return Err(EnolaError::ValidationError(format!(
            "Dataset '{path}' excede el máximo de {} MiB",
            MAX_DATASET_BYTES / (1024 * 1024)
        )));
    }
    std::fs::read_to_string(path)
        .map_err(|e| EnolaError::ValidationError(format!("No se pudo leer '{path}': {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ai::{AiInstance, AiInstanceStatus};
    use crate::domain::ai_model::{AiModel, AiModelStatus, GgufMetadata, ModelSource};
    use crate::ports::ai::{
        MockAiAdapterPort, MockAiModelLibraryPort, MockTokenizerDownloadPort,
        TokenizerDownloadOutcome,
    };
    use crate::ports::container::MockContainerPort;
    use crate::ports::finetune::MockAiFinetuneJobsPort;
    use crate::ports::hardware::{GpuInfo, MockHardwareProbePort, SystemHardwareSpecs};

    fn inst() -> AiInstance {
        AiInstance {
            name: "dev".into(),
            model: "llama3.1:8b".into(),
            api_port: Some(11435),
            webui_port: Some(8081),
            status: AiInstanceStatus::Running,
            ollama_status: "Up".into(),
            webui_status: "Up".into(),
            gpu_enabled: true,
            onion_address: None,
        }
    }

    fn lib_model(name: &str, filename: &str, repo: Option<&str>) -> AiModel {
        AiModel {
            name: name.to_string(),
            filename: filename.to_string(),
            size_bytes: 0,
            quantization: "Q4_K_M".into(),
            family: "Llama 3".into(),
            status: AiModelStatus::Ready,
            source: ModelSource::Downloaded,
            sha256: None,
            hf_repo: repo.map(str::to_string),
        }
    }

    /// Mock de biblioteca con un GGUF `llama3.1:8b` → `m.gguf` (repo `org/repo`)
    /// y header GGUF soportado (llama + Q4_K = file_type 12).
    fn library_with_base_model(base: &Path) -> MockAiModelLibraryPort {
        let base = base.to_path_buf();
        let mut lib = MockAiModelLibraryPort::new();
        lib.expect_list_library_models()
            .returning(|_| Ok(vec![lib_model("llama3.1:8b", "m.gguf", Some("org/repo"))]));
        lib.expect_read_gguf_metadata()
            .withf(|i, f| i == "dev" && f == "m.gguf")
            .returning(|_, _| {
                Ok(GgufMetadata {
                    version: 3,
                    tensor_count: 1,
                    metadata_kv_count: 2,
                    architecture: Some("llama".into()),
                    file_type: Some(12),
                })
            });
        lib.expect_resolve_tokenizer_dir()
            .withf(|i, r| i == "dev" && r == "org/repo")
            .returning(move |_, r| {
                Ok(base
                    .join("dev/ollama/models-gguf/tokenizer")
                    .join(r.replace('/', "__")))
            });
        lib
    }

    fn specs_gpu() -> SystemHardwareSpecs {
        SystemHardwareSpecs {
            cpu_cores: 8,
            ram_total_mb: 32000,
            ram_available_mb: 16000,
            ram_used_mb: 16000,
            gpu_brand: crate::ports::hardware::GpuBrand::Nvidia,
            gpus: vec![GpuInfo {
                index: 0,
                name: "RTX".into(),
                brand: crate::ports::hardware::GpuBrand::Nvidia,
                vram_total_mb: 12288,
                vram_used_mb: 1000,
                vram_free_mb: 11288,
                temperature_c: None,
                utilization_gpu: None,
                driver_version: None,
            }],
            battery_status: crate::ports::hardware::BatteryState::AcConnected,
            platform: "linux".into(),
        }
    }

    #[tokio::test]
    async fn prepare_converts_and_splits() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("in.jsonl");
        std::fs::write(
            &src,
            (0..10)
                .map(|i| format!("{{\"instruction\":\"q{i}\",\"output\":\"a{i}\"}}"))
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();
        let svc = ManageFinetune::new(
            Arc::new(MockAiAdapterPort::new()),
            Arc::new(MockHardwareProbePort::new()),
            Arc::new(MockContainerPort::new()),
            Arc::new(MockAiFinetuneJobsPort::new()),
            Arc::new(MockAiModelLibraryPort::new()),
            None,
            dir.path().to_path_buf(),
        );
        let rep = svc
            .prepare(src.to_str().unwrap(), None, None, 20)
            .await
            .unwrap();
        assert_eq!(rep.format, DatasetFormat::Alpaca);
        assert_eq!(rep.valid_records, 10);
        assert_eq!(rep.train_records + rep.val_records, 10);
        assert!(Path::new(&rep.train_path).exists());
        // El train.jsonl es ChatML válido
        let back = std::fs::read_to_string(&rep.train_path).unwrap();
        assert!(back.contains("\"role\":\"user\""));
    }

    #[tokio::test]
    async fn validate_reports_gpu_and_schema() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("in.jsonl");
        std::fs::write(
            &src,
            "{\"messages\":[{\"role\":\"user\",\"content\":\"h\"}]}\n",
        )
        .unwrap();
        let mut hw = MockHardwareProbePort::new();
        hw.expect_probe().returning(|| Ok(specs_gpu()));
        let svc = ManageFinetune::new(
            Arc::new(MockAiAdapterPort::new()),
            Arc::new(hw),
            Arc::new(MockContainerPort::new()),
            Arc::new(MockAiFinetuneJobsPort::new()),
            Arc::new(MockAiModelLibraryPort::new()),
            None,
            dir.path().to_path_buf(),
        );
        let rep = svc
            .validate(src.to_str().unwrap(), Some("llama3.1:8b"))
            .await
            .unwrap();
        assert!(rep.dataset_ok);
        assert!(rep.checks.iter().any(|c| c.name == "gpu" && c.ok));
        assert!(rep.checks.iter().any(|c| c.name == "base_model"));
    }

    #[tokio::test]
    async fn start_requires_force_and_creates_container_and_job() {
        let dir = tempfile::tempdir().unwrap();
        let ds = dir.path().join("train.jsonl");
        std::fs::write(
            &ds,
            "{\"messages\":[{\"role\":\"user\",\"content\":\"h\"}]}\n",
        )
        .unwrap();
        // El tokenizer debe existir en el host (S3) para montarse.
        let tok_dir = dir
            .path()
            .join("dev/ollama/models-gguf/tokenizer/org__repo");
        std::fs::create_dir_all(&tok_dir).unwrap();
        std::fs::write(tok_dir.join("tokenizer_config.json"), "{}").unwrap();

        let mut insts = MockAiAdapterPort::new();
        insts.expect_get_instance().returning(|_| Ok(inst()));
        let mut hw = MockHardwareProbePort::new();
        hw.expect_probe().returning(|| Ok(specs_gpu()));
        let mut cont = MockContainerPort::new();
        cont.expect_create_container()
            .withf(|cfg: &ContainerConfig| {
                let args = cfg.command.as_ref().unwrap();
                let has_gguf_arg = args
                    .windows(2)
                    .any(|w| w[0] == "--gguf" && w[1] == "/model.gguf");
                let has_tokenizer_arg = args
                    .windows(2)
                    .any(|w| w[0] == "--tokenizer" && w[1] == "/tokenizer");
                cfg.name.starts_with("enola-ai-dev-finetune-")
                    && cfg.gpu_support
                    && cfg.memory_limit == Some(TRAIN_MEMORY_BYTES)
                    && cfg.read_only_rootfs
                    && has_gguf_arg
                    && has_tokenizer_arg
                    && cfg.volumes.values().any(|v| v == "/model.gguf")
                    && cfg.volumes.values().any(|v| v == "/tokenizer")
            })
            .returning(|cfg| Ok(cfg.name.clone()));
        cont.expect_start_container().returning(|_| Ok(()));
        let mut jobs = MockAiFinetuneJobsPort::new();
        jobs.expect_add_job().returning(|_| Ok(()));

        let svc = ManageFinetune::new(
            Arc::new(insts),
            Arc::new(hw),
            Arc::new(cont),
            Arc::new(jobs),
            Arc::new(library_with_base_model(dir.path())),
            None,
            dir.path().to_path_buf(),
        );
        // Sin --force → rechazo.
        assert!(svc
            .start("dev", ds.to_str().unwrap(), None, None, false, false, false)
            .await
            .is_err());
        let job = svc
            .start("dev", ds.to_str().unwrap(), None, None, false, false, true)
            .await
            .unwrap();
        assert_eq!(job.status, FinetuneJobStatus::Running);
        assert!(job.container.contains("finetune"));
        // El job registra el modelo base resuelto por `name` desde la biblioteca.
        assert_eq!(job.base_model, "llama3.1:8b");
    }

    #[tokio::test]
    async fn start_errors_when_base_model_unsupported_gguf() {
        let dir = tempfile::tempdir().unwrap();
        let ds = dir.path().join("train.jsonl");
        std::fs::write(
            &ds,
            "{\"messages\":[{\"role\":\"user\",\"content\":\"h\"}]}\n",
        )
        .unwrap();
        let tok_dir = dir
            .path()
            .join("dev/ollama/models-gguf/tokenizer/org__repo");
        std::fs::create_dir_all(&tok_dir).unwrap();
        std::fs::write(tok_dir.join("tokenizer_config.json"), "{}").unwrap();

        let mut insts = MockAiAdapterPort::new();
        insts.expect_get_instance().returning(|_| Ok(inst()));
        let mut hw = MockHardwareProbePort::new();
        hw.expect_probe().returning(|| Ok(specs_gpu()));
        let mut cont = MockContainerPort::new();
        cont.expect_create_container().returning(|_| Ok("x".into()));
        cont.expect_start_container().returning(|_| Ok(()));
        let mut jobs = MockAiFinetuneJobsPort::new();
        jobs.expect_add_job().returning(|_| Ok(()));

        // Header GGUF con arquitectura no soportada (arch "falcon" no está en
        // la lista de soportadas).
        let mut lib = MockAiModelLibraryPort::new();
        lib.expect_list_library_models()
            .returning(|_| Ok(vec![lib_model("llama3.1:8b", "m.gguf", Some("org/repo"))]));
        lib.expect_read_gguf_metadata().returning(|_, _| {
            Ok(GgufMetadata {
                version: 3,
                tensor_count: 1,
                metadata_kv_count: 2,
                architecture: Some("falcon".into()),
                file_type: Some(12),
            })
        });

        let svc = ManageFinetune::new(
            Arc::new(insts),
            Arc::new(hw),
            Arc::new(cont),
            Arc::new(jobs),
            Arc::new(lib),
            None,
            dir.path().to_path_buf(),
        );
        let err = svc
            .start("dev", ds.to_str().unwrap(), None, None, false, false, true)
            .await
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("no se puede cargar para fine-tuning"));
        assert!(msg.contains(crate::domain::ai_model::GGUF_SUPPORTED_QUANT_HINT));
    }

    #[tokio::test]
    async fn start_downloads_tokenizer_on_demand_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        let ds = dir.path().join("train.jsonl");
        std::fs::write(
            &ds,
            "{\"messages\":[{\"role\":\"user\",\"content\":\"h\"}]}\n",
        )
        .unwrap();
        // No creamos tokenizer_config.json → dispara la descarga bajo demanda.

        let mut insts = MockAiAdapterPort::new();
        insts.expect_get_instance().returning(|_| Ok(inst()));
        let mut hw = MockHardwareProbePort::new();
        hw.expect_probe().returning(|| Ok(specs_gpu()));
        let mut cont = MockContainerPort::new();
        cont.expect_create_container().returning(|_| Ok("x".into()));
        cont.expect_start_container().returning(|_| Ok(()));
        let mut jobs = MockAiFinetuneJobsPort::new();
        jobs.expect_add_job().returning(|_| Ok(()));

        let mut tok = MockTokenizerDownloadPort::new();
        tok.expect_download_tokenizer()
            .withf(|r| r.repo == "org/repo" && !r.use_tor)
            .times(1)
            .returning(|r| {
                Ok(TokenizerDownloadOutcome {
                    destination: r.destination,
                    files_downloaded: 1,
                })
            });

        let svc = ManageFinetune::new(
            Arc::new(insts),
            Arc::new(hw),
            Arc::new(cont),
            Arc::new(jobs),
            Arc::new(library_with_base_model(dir.path())),
            Some(Arc::new(tok)),
            dir.path().to_path_buf(),
        );
        let job = svc
            .start("dev", ds.to_str().unwrap(), None, None, false, false, true)
            .await
            .unwrap();
        assert_eq!(job.status, FinetuneJobStatus::Running);
    }

    #[tokio::test]
    async fn start_errors_without_tokenizer_repo_or_override() {
        let dir = tempfile::tempdir().unwrap();
        let ds = dir.path().join("train.jsonl");
        std::fs::write(
            &ds,
            "{\"messages\":[{\"role\":\"user\",\"content\":\"h\"}]}\n",
        )
        .unwrap();

        let mut insts = MockAiAdapterPort::new();
        insts.expect_get_instance().returning(|_| Ok(inst()));
        let mut hw = MockHardwareProbePort::new();
        hw.expect_probe().returning(|| Ok(specs_gpu()));
        let mut cont = MockContainerPort::new();
        cont.expect_create_container().returning(|_| Ok("x".into()));
        cont.expect_start_container().returning(|_| Ok(()));
        let mut jobs = MockAiFinetuneJobsPort::new();
        jobs.expect_add_job().returning(|_| Ok(()));

        // Modelo importado sin repo HF (hf_repo = None) y sin --tokenizer.
        let mut lib = MockAiModelLibraryPort::new();
        lib.expect_list_library_models()
            .returning(|_| Ok(vec![lib_model("llama3.1:8b", "m.gguf", None)]));
        lib.expect_read_gguf_metadata().returning(|_, _| {
            Ok(GgufMetadata {
                version: 3,
                tensor_count: 1,
                metadata_kv_count: 2,
                architecture: Some("llama".into()),
                file_type: Some(12),
            })
        });

        let svc = ManageFinetune::new(
            Arc::new(insts),
            Arc::new(hw),
            Arc::new(cont),
            Arc::new(jobs),
            Arc::new(lib),
            None,
            dir.path().to_path_buf(),
        );
        let err = svc
            .start("dev", ds.to_str().unwrap(), None, None, false, false, true)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("--tokenizer"));
    }

    #[tokio::test]
    async fn start_tokenizer_override_repo_takes_precedence() {
        let dir = tempfile::tempdir().unwrap();
        let ds = dir.path().join("train.jsonl");
        std::fs::write(
            &ds,
            "{\"messages\":[{\"role\":\"user\",\"content\":\"h\"}]}\n",
        )
        .unwrap();

        let mut insts = MockAiAdapterPort::new();
        insts.expect_get_instance().returning(|_| Ok(inst()));
        let mut hw = MockHardwareProbePort::new();
        hw.expect_probe().returning(|| Ok(specs_gpu()));
        let mut cont = MockContainerPort::new();
        cont.expect_create_container().returning(|_| Ok("x".into()));
        cont.expect_start_container().returning(|_| Ok(()));
        let mut jobs = MockAiFinetuneJobsPort::new();
        jobs.expect_add_job().returning(|_| Ok(()));

        let base = dir.path().to_path_buf();
        let mut lib = MockAiModelLibraryPort::new();
        lib.expect_list_library_models()
            .returning(|_| Ok(vec![lib_model("llama3.1:8b", "m.gguf", Some("org/repo"))]));
        lib.expect_read_gguf_metadata().returning(|_, _| {
            Ok(GgufMetadata {
                version: 3,
                tensor_count: 1,
                metadata_kv_count: 2,
                architecture: Some("llama".into()),
                file_type: Some(12),
            })
        });
        lib.expect_resolve_tokenizer_dir()
            .withf(|i, r| i == "dev" && r == "other/repo")
            .times(1)
            .returning(move |_, r| Ok(base.join("tokenizer").join(r.replace('/', "__"))));

        let mut tok = MockTokenizerDownloadPort::new();
        tok.expect_download_tokenizer()
            .withf(|r| r.repo == "other/repo")
            .times(1)
            .returning(|r| {
                Ok(TokenizerDownloadOutcome {
                    destination: r.destination,
                    files_downloaded: 1,
                })
            });

        let svc = ManageFinetune::new(
            Arc::new(insts),
            Arc::new(hw),
            Arc::new(cont),
            Arc::new(jobs),
            Arc::new(lib),
            Some(Arc::new(tok)),
            dir.path().to_path_buf(),
        );
        let job = svc
            .start(
                "dev",
                ds.to_str().unwrap(),
                None,
                Some("other/repo"),
                false,
                false,
                true,
            )
            .await
            .unwrap();
        assert_eq!(job.status, FinetuneJobStatus::Running);
    }

    #[tokio::test]
    async fn status_errors_on_nonexistent_instance() {
        let dir = tempfile::tempdir().unwrap();
        let mut insts = MockAiAdapterPort::new();
        insts.expect_get_instance().returning(|name| {
            Err(EnolaError::ValidationError(format!(
                "instancia '{name}' no existe"
            )))
        });
        let svc = ManageFinetune::new(
            Arc::new(insts),
            Arc::new(MockHardwareProbePort::new()),
            Arc::new(MockContainerPort::new()),
            Arc::new(MockAiFinetuneJobsPort::new()),
            Arc::new(MockAiModelLibraryPort::new()),
            None,
            dir.path().to_path_buf(),
        );
        assert!(svc.status("inexistente-xyz").await.is_err());
    }

    #[tokio::test]
    async fn status_lists_jobs_of_existing_instance() {
        let dir = tempfile::tempdir().unwrap();
        let mut insts = MockAiAdapterPort::new();
        insts.expect_get_instance().returning(|_| Ok(inst()));
        let mut jobs = MockAiFinetuneJobsPort::new();
        jobs.expect_list_jobs().returning(|_| Ok(vec![]));
        let svc = ManageFinetune::new(
            Arc::new(insts),
            Arc::new(MockHardwareProbePort::new()),
            Arc::new(MockContainerPort::new()),
            Arc::new(jobs),
            Arc::new(MockAiModelLibraryPort::new()),
            None,
            dir.path().to_path_buf(),
        );
        assert!(svc.status("dev").await.is_ok());
    }
}
