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
use crate::domain::error::{EnolaError, Result};
use crate::ports::ai::AiAdapterPort;
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
    /// Directorio base de instancias (`/srv/enola-ai`) — datasets y scripts.
    base_dir: PathBuf,
}

impl ManageFinetune {
    /// Construye el caso de uso con los puertos inyectados.
    pub fn new(
        instances: Arc<dyn AiAdapterPort + Send + Sync>,
        hardware: Arc<dyn HardwareProbePort + Send + Sync>,
        containers: Arc<dyn ContainerPort + Send + Sync>,
        jobs: Arc<dyn AiFinetuneJobsPort + Send + Sync>,
        base_dir: PathBuf,
    ) -> Self {
        Self {
            instances,
            hardware,
            containers,
            jobs,
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
        let base_model = base_model
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| inst.model.clone());

        // El dataset debe estar en formato ChatML preparado.
        let content = read_limited(dataset)?;
        let (stats, _records) = validate_dataset(&content)?;
        if stats.format != DatasetFormat::ChatML {
            return Err(EnolaError::ValidationError(format!(
                "El dataset debe estar en formato ChatML ({:?} detectado) — prepara con `ai finetune prepare`",
                stats.format
            )));
        }

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
            "--base-model".to_string(),
            base_model.clone(),
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
    use crate::ports::ai::MockAiAdapterPort;
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

        let mut insts = MockAiAdapterPort::new();
        insts.expect_get_instance().returning(|_| Ok(inst()));
        let mut hw = MockHardwareProbePort::new();
        hw.expect_probe().returning(|| Ok(specs_gpu()));
        let mut cont = MockContainerPort::new();
        cont.expect_create_container()
            .withf(|cfg: &ContainerConfig| {
                cfg.name.starts_with("enola-ai-dev-finetune-")
                    && cfg.gpu_support
                    && cfg.memory_limit == Some(TRAIN_MEMORY_BYTES)
                    && cfg.read_only_rootfs
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
            dir.path().to_path_buf(),
        );
        // Sin --force → rechazo.
        assert!(svc
            .start("dev", ds.to_str().unwrap(), None, false, false, false)
            .await
            .is_err());
        let job = svc
            .start("dev", ds.to_str().unwrap(), None, false, false, true)
            .await
            .unwrap();
        assert_eq!(job.status, FinetuneJobStatus::Running);
        assert!(job.container.contains("finetune"));
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
            dir.path().to_path_buf(),
        );
        assert!(svc.status("dev").await.is_ok());
    }
}
