// Fase 14 — Casos de uso `ai finetune` (prepare/validate/start/status).
//
// Hexagonal: ports (`AiAdapterPort`, `HardwareProbePort`, `ContainerPort`,
// `AiFinetuneJobsPort`, `FinetuneWorkspacePort` para los ficheros del host)
// + dominio puro (`ai_finetune`). El script LoRA
// embebido corre DENTRO del contenedor de entrenamiento — nunca en el host.

use crate::domain::ai::validate_ai_instance_name;
use crate::domain::ai_finetune::{
    finetune_warnings, records_to_jsonl, split_dataset, validate_dataset, DatasetFormat,
    FinetuneJob, FinetuneJobLayout, FinetuneJobStatus,
};
use crate::domain::ai_model;
use crate::domain::error::{EnolaError, Result};
use crate::ports::ai::{
    AiAdapterPort, AiModelLibraryPort, AiModelRegistryPort, TokenizerDownloadPort,
    TokenizerDownloadRequest,
};
use crate::ports::container::{ContainerConfig, ContainerPort};
use crate::ports::finetune::{AiFinetuneJobsPort, FinetuneWorkspacePort};
use crate::ports::hardware::HardwareProbePort;
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Imagen de entrenamiento (CUDA runtime con transformers/peft/datasets).
const TRAIN_IMAGE: &str = "enola-finetune:latest";
/// A25-12: UID/GID sin privilegios del proceso de entrenamiento. No el 1000:
/// en el host suele ser el usuario invocador, que podría manipular los
/// resultados (el GGUF que root importa después con `register`).
pub(crate) const FINETUNE_UID: u32 = 65532;
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
    /// Registro Ollama (`/api/create`): materializa el modelo resultante.
    registry: Arc<dyn AiModelRegistryPort + Send + Sync>,
    /// Descarga bajo demanda del tokenizer HF (`--tokenizer`), host-side.
    tokenizer: Option<Arc<dyn TokenizerDownloadPort + Send + Sync>>,
    /// Ficheros del host: datasets, workspace del job y GGUF exportado (H-09).
    workspace: Arc<dyn FinetuneWorkspacePort + Send + Sync>,
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
        registry: Arc<dyn AiModelRegistryPort + Send + Sync>,
        tokenizer: Option<Arc<dyn TokenizerDownloadPort + Send + Sync>>,
        base_dir: PathBuf,
        workspace: Arc<dyn FinetuneWorkspacePort + Send + Sync>,
    ) -> Self {
        Self {
            instances,
            hardware,
            containers,
            jobs,
            library,
            registry,
            tokenizer,
            workspace,
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
        let content = self.read_dataset(dataset).await?;
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
        let (train_path, val_path) = self
            .workspace
            .write_split(&out, &records_to_jsonl(&train), &records_to_jsonl(&val))
            .await?;
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
        let content = self.read_dataset(dataset).await?;
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
        let content = self.read_dataset(dataset).await?;
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
                if !self
                    .workspace
                    .is_file(&dir.join("tokenizer_config.json"))
                    .await
                {
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

        // Id único y monótono por milisegundos (sin `% 100_000`: evita reuso y
        // colisión de ids en jobs que viven > ~27,8 h en `jobs.json`).
        let job_id = format!(
            "ft{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0)
        );
        let container_name = format!("enola-ai-{instance}-finetune-{job_id}");
        // El contenedor monta el rootfs en read-only (SEC-019) y solo escribe
        // en /out: /tmp y la caché HF van a subdirectorios de ese volumen.
        let layout = FinetuneJobLayout::new(&self.base_dir, instance, &job_id);
        // Directorios + scripts embebidos (nunca /tmp, SEC-001) + copia del dataset.
        self.workspace
            .prepare_job_workspace(&layout, Path::new(dataset))
            .await?;

        let mut env = HashMap::new();
        env.insert("LOW_MEMORY".into(), low_memory.to_string());
        // Fase 14.5 — export del modelo resultante a GGUF en /out. Solo si el
        // job se lanza sin --no-register; el fichero se llama
        // `<instance>-ft-<job_id>.gguf` (lo que `register` buscará luego).
        if !no_register {
            let export_name = format!("{instance}-ft");
            env.insert("EXPORT_FILE".into(), format!("{export_name}-{job_id}.gguf"));
        }
        // El rootfs es read-only: directorios de escritura dentro del volumen /out.
        env.insert("TMPDIR".into(), "/out/tmp".to_string());
        env.insert("HF_HOME".into(), "/out/.hf".to_string());
        // A25-12: el UID sin privilegios no tiene home en la imagen.
        env.insert("HOME".into(), "/out".to_string());
        // Sin red (`network_mode: none`): fallar rápido si se pide un modelo remoto,
        // en vez de colgarse en un timeout de red.
        env.insert("HF_HUB_OFFLINE".into(), "1".to_string());
        env.insert("TRANSFORMERS_OFFLINE".into(), "1".to_string());
        // H-17: torch >= 2.14 enruta ops en modo eager (p.ej. el `bmm` del RoPE)
        // a kernels Triton que compilan C en runtime y cachean en `$HOME`: con
        // rootfs read-only (y sin compilador en la imagen) el job falla. Con
        // esto torch usa sus kernels CUDA precompilados.
        env.insert("TORCH_DISABLE_NATIVE_JIT".into(), "1".to_string());
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
        volumes.insert(
            layout.data_dir.to_string_lossy().to_string(),
            "/data".to_string(),
        );
        volumes.insert(
            layout.scripts_dir.to_string_lossy().to_string(),
            "/opt/finetune".to_string(),
        );
        volumes.insert(
            layout.out_dir.to_string_lossy().to_string(),
            "/out".to_string(),
        );
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
            // Sin red real (`network_mode: none`): `None` sería el bridge por defecto (H-04).
            network: Some("none".to_string()),
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
            // A25-12: nunca root dentro del contenedor de entrenamiento.
            user: Some(format!("{FINETUNE_UID}:{FINETUNE_UID}")),
        };
        self.containers.create_container(cfg).await?;
        self.containers.start_container(&container_name).await?;

        let job = FinetuneJob {
            id: job_id,
            instance: instance.to_string(),
            base_model,
            dataset: dataset.to_string(),
            low_memory,
            status: FinetuneJobStatus::Running,
            container: container_name,
            created_at: now_iso8601(),
            finished_at: None,
            error: None,
            result_model: if no_register {
                None
            } else {
                Some(format!("{instance}-ft"))
            },
            registered: false,
        };
        self.jobs.add_job(job.clone()).await?;
        Ok(job)
    }

    /// `ai finetune status <instance>` — jobs registrados + estado del
    /// contenedor si sigue corriendo. Los jobs `Running` se reconcilian con
    /// el exit real del contenedor (`Completed`/`Failed` persistidos).
    pub async fn status(&self, instance: &str) -> Result<Vec<FinetuneJob>> {
        validate_ai_instance_name(instance)?;
        // La instancia debe existir: `status` sobre una instancia inexistente
        // es un error, no una lista vacía (verificado por test_finetune.sh #11).
        self.instances.get_instance(instance).await?;
        let mut jobs = self.jobs.list_jobs(instance).await?;
        for job in jobs.iter_mut() {
            if job.status == FinetuneJobStatus::Running {
                self.reconcile_job(instance, job).await?;
            }
        }
        Ok(jobs)
    }

    /// `ai finetune register <instance> <job-id>` — materializa el modelo
    /// resultante de un job completado: importa el `.gguf` exportado por el
    /// entrenamiento a la biblioteca de la instancia y lo registra en Ollama
    /// (`/api/create`) con el nombre `result_model` (`<instance>-ft`).
    pub async fn register(&self, instance: &str, job_id: &str) -> Result<FinetuneJob> {
        validate_ai_instance_name(instance)?;
        // La instancia debe existir (mismo contrato que `status`).
        self.instances.get_instance(instance).await?;

        let mut jobs = self.jobs.list_jobs(instance).await?;
        let idx = jobs.iter().position(|j| j.id == job_id).ok_or_else(|| {
            EnolaError::NotFound(format!("job '{job_id}' no existe en '{instance}'"))
        })?;
        // Si el job aún figura Running, reconciliar con el exit real del
        // contenedor antes de decidir (el usuario puede registrar sin pasar
        // por `status`).
        if jobs[idx].status == FinetuneJobStatus::Running {
            self.reconcile_job(instance, &mut jobs[idx]).await?;
        }
        let job = jobs[idx].clone();

        if job.status != FinetuneJobStatus::Completed {
            return Err(EnolaError::ValidationError(format!(
                "el job '{job_id}' no está completado (estado {:?}) — espera a que el entrenamiento termine con exit 0",
                job.status
            )));
        }
        let Some(result_name) = job.result_model.clone() else {
            return Err(EnolaError::ValidationError(format!(
                "el job '{job_id}' se lanzó con --no-register: no hay modelo que registrar"
            )));
        };

        // El GGUF exportado vive en `<base>/<instance>/finetune/<job>/out/`.
        let filename = format!("{result_name}-{job_id}.gguf");
        let gguf_path = FinetuneJobLayout::new(&self.base_dir, instance, job_id)
            .out_dir
            .join(&filename);
        if !self.workspace.is_file(&gguf_path).await {
            return Err(EnolaError::NotFound(format!(
                "no se encontró '{}' en la salida del job — el entrenamiento debe exportar el GGUF (¿falló la conversión?)",
                gguf_path.display()
            )));
        }

        // Importa a la biblioteca (copia + SHA-256 + validación de header) y
        // registra en Ollama (blob + `/api/create`, H-16).
        let model = self
            .library
            .import_library_file(instance, &gguf_path)
            .await?;
        self.registry
            .register_library_model(instance, &model.filename, &result_name)
            .await?;
        // Persistir el estado "registrado" para que `status` no muestre el
        // modelo como pendiente de registro.
        let mut updated = job.clone();
        updated.registered = true;
        self.jobs.update_job(instance, &updated).await?;
        Ok(updated)
    }

    /// Lee el dataset con el límite defensivo de tamaño (`MAX_DATASET_BYTES`).
    async fn read_dataset(&self, dataset: &str) -> Result<String> {
        self.workspace
            .read_dataset(Path::new(dataset), MAX_DATASET_BYTES)
            .await
    }

    /// Reconcilia un job `Running` con el estado real de su contenedor.
    /// `exited`/`dead` → `Completed` (exit 0) o `Failed` (resto). Si el
    /// inspect falla (daemon caído, contenedor borrado) se deja `Running`:
    /// no se puede distinguir transitorio de desaparecido y marcar `Failed`
    /// a ciegas corrompería jobs vivos.
    async fn reconcile_job(&self, instance: &str, job: &mut FinetuneJob) -> Result<()> {
        let Ok(info) = self.containers.inspect_container(&job.container).await else {
            return Ok(());
        };
        match info.get("status").map(String::as_str) {
            Some("exited") | Some("dead") => {
                let code = info
                    .get("exit_code")
                    .and_then(|s| s.parse::<i64>().ok())
                    .unwrap_or(1);
                job.finished_at = Some(now_iso8601());
                if code == 0 {
                    job.status = FinetuneJobStatus::Completed;
                } else {
                    job.status = FinetuneJobStatus::Failed;
                    job.error = Some(match info.get("error") {
                        Some(d) => format!("exit {code}: {d}"),
                        None => format!("el contenedor terminó con exit {code}"),
                    });
                }
                self.jobs.update_job(instance, job).await?;
            }
            _ => {}
        }
        Ok(())
    }
}

/// Marca de tiempo ISO-8601 UTC (consistente con `created_at`/`finished_at`,
/// que el dominio documenta como ISO-8601).
fn now_iso8601() -> String {
    chrono::Utc::now().to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ai::{AiInstance, AiInstanceStatus};
    use crate::domain::ai_model::{AiModel, AiModelStatus, GgufMetadata, ModelSource};
    use crate::ports::ai::{
        MockAiAdapterPort, MockAiModelLibraryPort, MockAiModelRegistryPort,
        MockTokenizerDownloadPort, TokenizerDownloadOutcome,
    };
    use crate::ports::container::MockContainerPort;
    use crate::ports::finetune::MockAiFinetuneJobsPort;
    use crate::ports::hardware::{GpuInfo, MockHardwareProbePort, SystemHardwareSpecs};

    /// Sistema de ficheros en memoria para los tests (sin disco): implementa
    /// `FinetuneWorkspacePort` con la misma semántica que `FsFinetuneWorkspace`.
    #[derive(Default)]
    struct FakeWorkspace {
        files: std::sync::Mutex<HashMap<PathBuf, String>>,
    }

    impl FakeWorkspace {
        fn write(&self, path: impl AsRef<Path>, content: impl AsRef<[u8]>) -> std::io::Result<()> {
            self.files.lock().unwrap().insert(
                path.as_ref().to_path_buf(),
                String::from_utf8_lossy(content.as_ref()).to_string(),
            );
            Ok(())
        }

        fn mkdir(&self, _path: impl AsRef<Path>) -> std::io::Result<()> {
            Ok(())
        }

        fn read(&self, path: impl AsRef<Path>) -> std::io::Result<String> {
            self.files
                .lock()
                .unwrap()
                .get(path.as_ref())
                .cloned()
                .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))
        }

        fn contains(&self, path: impl AsRef<Path>) -> bool {
            self.files.lock().unwrap().contains_key(path.as_ref())
        }
    }

    #[async_trait::async_trait]
    impl FinetuneWorkspacePort for FakeWorkspace {
        async fn read_dataset(&self, path: &Path, max_bytes: u64) -> Result<String> {
            let content = self.read(path).map_err(|e| {
                EnolaError::ValidationError(format!("No se pudo leer '{}': {e}", path.display()))
            })?;
            if content.len() as u64 > max_bytes {
                return Err(EnolaError::ValidationError(format!(
                    "Dataset '{}' excede el máximo",
                    path.display()
                )));
            }
            Ok(content)
        }

        async fn write_split(
            &self,
            out_dir: &Path,
            train_jsonl: &str,
            val_jsonl: &str,
        ) -> Result<(PathBuf, PathBuf)> {
            let (t, v) = (out_dir.join("train.jsonl"), out_dir.join("val.jsonl"));
            let _ = self.write(&t, train_jsonl);
            let _ = self.write(&v, val_jsonl);
            Ok((t, v))
        }

        async fn prepare_job_workspace(
            &self,
            layout: &FinetuneJobLayout,
            dataset: &Path,
        ) -> Result<()> {
            let content = self.read(dataset).map_err(|e| {
                EnolaError::InfrastructureError(format!(
                    "copiar dataset a '{}': {e}",
                    dataset.display()
                ))
            })?;
            let _ = self.write(layout.dataset_file(), content);
            Ok(())
        }

        async fn is_file(&self, path: &Path) -> bool {
            self.contains(path)
        }
    }

    /// Raíz ficticia (`/fake/enola-ai`) + su workspace en memoria.
    struct FakeDir {
        root: PathBuf,
        ws: Arc<FakeWorkspace>,
    }

    impl FakeDir {
        fn new() -> Self {
            Self {
                root: PathBuf::from("/fake/enola-ai"),
                ws: Arc::new(FakeWorkspace::default()),
            }
        }

        fn path(&self) -> &Path {
            &self.root
        }
    }

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
    /// y header GGUF soportado (llama + Q4_K_M = file_type 15).
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
                    file_type: Some(15),
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
        let dir = FakeDir::new();
        let src = dir.path().join("in.jsonl");
        dir.ws
            .write(
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
            Arc::new(MockAiModelRegistryPort::new()),
            None,
            dir.path().to_path_buf(),
            dir.ws.clone(),
        );
        let rep = svc
            .prepare(src.to_str().unwrap(), None, None, 20)
            .await
            .unwrap();
        assert_eq!(rep.format, DatasetFormat::Alpaca);
        assert_eq!(rep.valid_records, 10);
        assert_eq!(rep.train_records + rep.val_records, 10);
        assert!(dir.ws.contains(&rep.train_path));
        // El train.jsonl es ChatML válido
        let back = dir.ws.read(&rep.train_path).unwrap();
        assert!(back.contains("\"role\":\"user\""));
    }

    #[tokio::test]
    async fn validate_reports_gpu_and_schema() {
        let dir = FakeDir::new();
        let src = dir.path().join("in.jsonl");
        dir.ws
            .write(
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
            Arc::new(MockAiModelRegistryPort::new()),
            None,
            dir.path().to_path_buf(),
            dir.ws.clone(),
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
        let dir = FakeDir::new();
        let ds = dir.path().join("train.jsonl");
        dir.ws
            .write(
                &ds,
                "{\"messages\":[{\"role\":\"user\",\"content\":\"h\"}]}\n",
            )
            .unwrap();
        // El tokenizer debe existir en el host (S3) para montarse.
        let tok_dir = dir
            .path()
            .join("dev/ollama/models-gguf/tokenizer/org__repo");
        dir.ws.mkdir(&tok_dir).unwrap();
        dir.ws
            .write(tok_dir.join("tokenizer_config.json"), "{}")
            .unwrap();

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
                    // H-04: sin red real (`none`), no el bridge por defecto.
                    && cfg.network.as_deref() == Some("none")
                    // A25-12: nunca root dentro del contenedor.
                    && cfg.user.as_deref() == Some("65532:65532")
                    && cfg.env.get("HOME").map(String::as_str) == Some("/out")
                    && cfg.gpu_support
                    && cfg.memory_limit == Some(TRAIN_MEMORY_BYTES)
                    && cfg.read_only_rootfs
                    // H-17: sin JIT nativo de torch (Triton compila C en runtime
                    // y escribe en $HOME, incompatible con rootfs read-only).
                    && cfg.env.get("TORCH_DISABLE_NATIVE_JIT").map(String::as_str) == Some("1")
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
            Arc::new(MockAiModelRegistryPort::new()),
            None,
            dir.path().to_path_buf(),
            dir.ws.clone(),
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
        let dir = FakeDir::new();
        let ds = dir.path().join("train.jsonl");
        dir.ws
            .write(
                &ds,
                "{\"messages\":[{\"role\":\"user\",\"content\":\"h\"}]}\n",
            )
            .unwrap();
        let tok_dir = dir
            .path()
            .join("dev/ollama/models-gguf/tokenizer/org__repo");
        dir.ws.mkdir(&tok_dir).unwrap();
        dir.ws
            .write(tok_dir.join("tokenizer_config.json"), "{}")
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
                file_type: Some(15),
            })
        });

        let svc = ManageFinetune::new(
            Arc::new(insts),
            Arc::new(hw),
            Arc::new(cont),
            Arc::new(jobs),
            Arc::new(lib),
            Arc::new(MockAiModelRegistryPort::new()),
            None,
            dir.path().to_path_buf(),
            dir.ws.clone(),
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
        let dir = FakeDir::new();
        let ds = dir.path().join("train.jsonl");
        dir.ws
            .write(
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
            Arc::new(MockAiModelRegistryPort::new()),
            Some(Arc::new(tok)),
            dir.path().to_path_buf(),
            dir.ws.clone(),
        );
        let job = svc
            .start("dev", ds.to_str().unwrap(), None, None, false, false, true)
            .await
            .unwrap();
        assert_eq!(job.status, FinetuneJobStatus::Running);
    }

    #[tokio::test]
    async fn start_errors_without_tokenizer_repo_or_override() {
        let dir = FakeDir::new();
        let ds = dir.path().join("train.jsonl");
        dir.ws
            .write(
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
                file_type: Some(15),
            })
        });

        let svc = ManageFinetune::new(
            Arc::new(insts),
            Arc::new(hw),
            Arc::new(cont),
            Arc::new(jobs),
            Arc::new(lib),
            Arc::new(MockAiModelRegistryPort::new()),
            None,
            dir.path().to_path_buf(),
            dir.ws.clone(),
        );
        let err = svc
            .start("dev", ds.to_str().unwrap(), None, None, false, false, true)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("--tokenizer"));
    }

    #[tokio::test]
    async fn start_tokenizer_override_repo_takes_precedence() {
        let dir = FakeDir::new();
        let ds = dir.path().join("train.jsonl");
        dir.ws
            .write(
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
                file_type: Some(15),
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
            Arc::new(MockAiModelRegistryPort::new()),
            Some(Arc::new(tok)),
            dir.path().to_path_buf(),
            dir.ws.clone(),
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
        let dir = FakeDir::new();
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
            Arc::new(MockAiModelRegistryPort::new()),
            None,
            dir.path().to_path_buf(),
            dir.ws.clone(),
        );
        assert!(svc.status("inexistente-xyz").await.is_err());
    }

    #[tokio::test]
    async fn status_lists_jobs_of_existing_instance() {
        let dir = FakeDir::new();
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
            Arc::new(MockAiModelRegistryPort::new()),
            None,
            dir.path().to_path_buf(),
            dir.ws.clone(),
        );
        assert!(svc.status("dev").await.is_ok());
    }

    fn running_job(id: &str) -> FinetuneJob {
        FinetuneJob {
            id: id.into(),
            instance: "dev".into(),
            base_model: "llama3.1:8b".into(),
            dataset: "/data/train.jsonl".into(),
            low_memory: true,
            status: FinetuneJobStatus::Running,
            container: format!("enola-ai-dev-finetune-{id}"),
            created_at: "1".into(),
            finished_at: None,
            error: None,
            result_model: Some("dev-ft".into()),
            registered: false,
        }
    }

    fn inspect_info(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[tokio::test]
    async fn status_reconciles_exit_zero_to_completed() {
        let dir = FakeDir::new();
        let mut insts = MockAiAdapterPort::new();
        insts.expect_get_instance().returning(|_| Ok(inst()));
        let mut jobs = MockAiFinetuneJobsPort::new();
        jobs.expect_list_jobs()
            .returning(|_| Ok(vec![running_job("j1")]));
        jobs.expect_update_job()
            .withf(|inst, j| {
                inst == "dev"
                    && j.id == "j1"
                    && j.status == FinetuneJobStatus::Completed
                    && j.finished_at.is_some()
                    && j.error.is_none()
            })
            .returning(|_, _| Ok(()));
        let mut containers = MockContainerPort::new();
        containers
            .expect_inspect_container()
            .withf(|c| c == "enola-ai-dev-finetune-j1")
            .returning(|_| Ok(inspect_info(&[("status", "exited"), ("exit_code", "0")])));
        let svc = ManageFinetune::new(
            Arc::new(insts),
            Arc::new(MockHardwareProbePort::new()),
            Arc::new(containers),
            Arc::new(jobs),
            Arc::new(MockAiModelLibraryPort::new()),
            Arc::new(MockAiModelRegistryPort::new()),
            None,
            dir.path().to_path_buf(),
            dir.ws.clone(),
        );
        let jobs = svc.status("dev").await.unwrap();
        assert_eq!(jobs[0].status, FinetuneJobStatus::Completed);
    }

    #[tokio::test]
    async fn status_reconciles_nonzero_exit_to_failed() {
        let dir = FakeDir::new();
        let mut insts = MockAiAdapterPort::new();
        insts.expect_get_instance().returning(|_| Ok(inst()));
        let mut jobs = MockAiFinetuneJobsPort::new();
        jobs.expect_list_jobs()
            .returning(|_| Ok(vec![running_job("j2")]));
        jobs.expect_update_job()
            .withf(|inst, j| {
                inst == "dev"
                    && j.id == "j2"
                    && j.status == FinetuneJobStatus::Failed
                    && j.error.as_deref().is_some_and(|e| e.contains("exit 137"))
            })
            .returning(|_, _| Ok(()));
        let mut containers = MockContainerPort::new();
        containers
            .expect_inspect_container()
            .returning(|_| Ok(inspect_info(&[("status", "exited"), ("exit_code", "137")])));
        let svc = ManageFinetune::new(
            Arc::new(insts),
            Arc::new(MockHardwareProbePort::new()),
            Arc::new(containers),
            Arc::new(jobs),
            Arc::new(MockAiModelLibraryPort::new()),
            Arc::new(MockAiModelRegistryPort::new()),
            None,
            dir.path().to_path_buf(),
            dir.ws.clone(),
        );
        let jobs = svc.status("dev").await.unwrap();
        assert_eq!(jobs[0].status, FinetuneJobStatus::Failed);
    }

    #[tokio::test]
    async fn status_keeps_running_while_container_up() {
        let dir = FakeDir::new();
        let mut insts = MockAiAdapterPort::new();
        insts.expect_get_instance().returning(|_| Ok(inst()));
        let mut jobs = MockAiFinetuneJobsPort::new();
        jobs.expect_list_jobs()
            .returning(|_| Ok(vec![running_job("j3")]));
        // Sin expect_update_job: mockall entra en pánico si se llama.
        let mut containers = MockContainerPort::new();
        containers
            .expect_inspect_container()
            .returning(|_| Ok(inspect_info(&[("status", "running")])));
        let svc = ManageFinetune::new(
            Arc::new(insts),
            Arc::new(MockHardwareProbePort::new()),
            Arc::new(containers),
            Arc::new(jobs),
            Arc::new(MockAiModelLibraryPort::new()),
            Arc::new(MockAiModelRegistryPort::new()),
            None,
            dir.path().to_path_buf(),
            dir.ws.clone(),
        );
        let jobs = svc.status("dev").await.unwrap();
        assert_eq!(jobs[0].status, FinetuneJobStatus::Running);
    }

    #[tokio::test]
    async fn status_keeps_running_when_inspect_fails() {
        // Daemon caído o contenedor borrado: no se puede distinguir transitorio
        // de desaparecido — el job se queda Running (conservador).
        let dir = FakeDir::new();
        let mut insts = MockAiAdapterPort::new();
        insts.expect_get_instance().returning(|_| Ok(inst()));
        let mut jobs = MockAiFinetuneJobsPort::new();
        jobs.expect_list_jobs()
            .returning(|_| Ok(vec![running_job("j4")]));
        let mut containers = MockContainerPort::new();
        containers
            .expect_inspect_container()
            .returning(|_| Err(EnolaError::InfrastructureError("docker down".into())));
        let svc = ManageFinetune::new(
            Arc::new(insts),
            Arc::new(MockHardwareProbePort::new()),
            Arc::new(containers),
            Arc::new(jobs),
            Arc::new(MockAiModelLibraryPort::new()),
            Arc::new(MockAiModelRegistryPort::new()),
            None,
            dir.path().to_path_buf(),
            dir.ws.clone(),
        );
        let jobs = svc.status("dev").await.unwrap();
        assert_eq!(jobs[0].status, FinetuneJobStatus::Running);
    }

    fn completed_job(id: &str, result_model: Option<&str>) -> FinetuneJob {
        FinetuneJob {
            id: id.into(),
            instance: "dev".into(),
            base_model: "llama3.1:8b".into(),
            dataset: "/data/train.jsonl".into(),
            low_memory: true,
            status: FinetuneJobStatus::Completed,
            container: format!("enola-ai-dev-finetune-{id}"),
            created_at: "1".into(),
            finished_at: Some("2".into()),
            error: None,
            result_model: result_model.map(str::to_string),
            registered: false,
        }
    }

    #[tokio::test]
    async fn register_imports_and_registers_completed_job() {
        let dir = FakeDir::new();
        // GGUF exportado por el entrenamiento en /out.
        let out = dir.path().join("dev/finetune/ft1/out");
        dir.ws.mkdir(&out).unwrap();
        dir.ws
            .write(out.join("dev-ft-ft1.gguf"), b"not-a-real-gguf")
            .unwrap();

        let mut insts = MockAiAdapterPort::new();
        insts.expect_get_instance().returning(|_| Ok(inst()));
        let mut jobs = MockAiFinetuneJobsPort::new();
        jobs.expect_list_jobs()
            .returning(|_| Ok(vec![completed_job("ft1", Some("dev-ft"))]));
        jobs.expect_update_job()
            .withf(|i, j| i == "dev" && j.id == "ft1" && j.registered)
            .returning(|_, _| Ok(()));
        let mut lib = MockAiModelLibraryPort::new();
        lib.expect_import_library_file()
            .withf(|i, p| i == "dev" && p.ends_with("dev-ft-ft1.gguf"))
            .returning(|_, _| Ok(lib_model("dev-ft", "dev-ft-ft1.gguf", None)));
        let mut reg = MockAiModelRegistryPort::new();
        reg.expect_register_library_model()
            .withf(|i, f, n| i == "dev" && f == "dev-ft-ft1.gguf" && n == "dev-ft")
            .returning(|_, _, _| Ok(()));
        let svc = ManageFinetune::new(
            Arc::new(insts),
            Arc::new(MockHardwareProbePort::new()),
            Arc::new(MockContainerPort::new()),
            Arc::new(jobs),
            Arc::new(lib),
            Arc::new(reg),
            None,
            dir.path().to_path_buf(),
            dir.ws.clone(),
        );
        let job = svc.register("dev", "ft1").await.unwrap();
        assert_eq!(job.result_model.as_deref(), Some("dev-ft"));
        assert!(job.registered);
    }

    #[tokio::test]
    async fn register_rejects_job_launched_with_no_register() {
        let dir = FakeDir::new();
        let mut insts = MockAiAdapterPort::new();
        insts.expect_get_instance().returning(|_| Ok(inst()));
        let mut jobs = MockAiFinetuneJobsPort::new();
        jobs.expect_list_jobs()
            .returning(|_| Ok(vec![completed_job("ft2", None)]));
        let svc = ManageFinetune::new(
            Arc::new(insts),
            Arc::new(MockHardwareProbePort::new()),
            Arc::new(MockContainerPort::new()),
            Arc::new(jobs),
            Arc::new(MockAiModelLibraryPort::new()),
            Arc::new(MockAiModelRegistryPort::new()),
            None,
            dir.path().to_path_buf(),
            dir.ws.clone(),
        );
        let err = svc.register("dev", "ft2").await.unwrap_err();
        assert!(err.to_string().contains("--no-register"));
    }

    #[tokio::test]
    async fn register_rejects_missing_exported_gguf() {
        let dir = FakeDir::new();
        let mut insts = MockAiAdapterPort::new();
        insts.expect_get_instance().returning(|_| Ok(inst()));
        let mut jobs = MockAiFinetuneJobsPort::new();
        jobs.expect_list_jobs()
            .returning(|_| Ok(vec![completed_job("ft3", Some("dev-ft"))]));
        let svc = ManageFinetune::new(
            Arc::new(insts),
            Arc::new(MockHardwareProbePort::new()),
            Arc::new(MockContainerPort::new()),
            Arc::new(jobs),
            Arc::new(MockAiModelLibraryPort::new()),
            Arc::new(MockAiModelRegistryPort::new()),
            None,
            dir.path().to_path_buf(),
            dir.ws.clone(),
        );
        let err = svc.register("dev", "ft3").await.unwrap_err();
        assert!(err.to_string().contains("no se encontró"));
    }

    #[tokio::test]
    async fn register_rejects_unfinished_job() {
        let dir = FakeDir::new();
        let mut insts = MockAiAdapterPort::new();
        insts.expect_get_instance().returning(|_| Ok(inst()));
        let mut jobs = MockAiFinetuneJobsPort::new();
        jobs.expect_list_jobs()
            .returning(|_| Ok(vec![running_job("ft4")]));
        let mut containers = MockContainerPort::new();
        containers
            .expect_inspect_container()
            .returning(|_| Ok(inspect_info(&[("status", "running")])));
        let svc = ManageFinetune::new(
            Arc::new(insts),
            Arc::new(MockHardwareProbePort::new()),
            Arc::new(containers),
            Arc::new(jobs),
            Arc::new(MockAiModelLibraryPort::new()),
            Arc::new(MockAiModelRegistryPort::new()),
            None,
            dir.path().to_path_buf(),
            dir.ws.clone(),
        );
        let err = svc.register("dev", "ft4").await.unwrap_err();
        assert!(err.to_string().contains("no está completado"));
    }

    #[test]
    fn finetune_dockerfile_pins_base_image_and_every_package() {
        // H-05: base por digest y cada paquete pip con `==` (reproducibilidad).
        let dockerfile = include_str!("../../assets/finetune/Dockerfile");
        let from = dockerfile
            .lines()
            .find(|l| l.starts_with("FROM "))
            .unwrap_or_default();
        assert!(from.contains("@sha256:"), "FROM sin digest: {from}");
        for line in dockerfile.lines().filter(|l| l.contains("pip install")) {
            // Argumentos de `pip install` hasta el siguiente separador de
            // shell; las rutas (`-r /tmp/<lock>`) las cubre el test del lock.
            let pkgs = line
                .split_whitespace()
                .skip_while(|t| *t != "install")
                .skip(1)
                .take_while(|t| !matches!(*t, "&&" | ";" | ";;" | "\\"))
                .map(|t| t.trim_matches('"'))
                .filter(|t| !t.starts_with('-') && !t.starts_with("${"))
                .filter(|t| !t.starts_with("http") && !t.starts_with('/'));
            for pkg in pkgs {
                let pkg = pkg.trim_matches('"');
                assert!(pkg.contains("=="), "paquete sin pinear: {pkg}");
            }
        }
    }

    #[test]
    fn finetune_dockerfile_locks_every_transitive_dependency() {
        // Lock completo (también transitivas), sacado del `pip freeze` de la
        // imagen validada en el E2E real: común + CUDA (solo build cu126).
        let dockerfile = include_str!("../../assets/finetune/Dockerfile");
        let common = include_str!("../../assets/finetune/requirements.lock");
        let cuda = include_str!("../../assets/finetune/requirements-cu126.lock");
        let entries = |lock: &'static str| {
            lock.lines()
                .map(str::trim)
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .map(|l| {
                    let (name, ver) = l.split_once("==").unwrap_or_else(|| panic!("sin ==: {l}"));
                    assert!(
                        !ver.is_empty() && !ver.contains(['<', '>', '*', ' ']),
                        "{l}"
                    );
                    (name.to_ascii_lowercase().replace('_', "-"), ver.to_string())
                })
                .collect::<Vec<_>>()
        };
        let common = entries(common);
        let cuda = entries(cuda);
        assert!(common.len() > 40 && !cuda.is_empty());
        // torch lo fija su propia línea (variante CPU/cu126 según el índice).
        assert!(common.iter().chain(&cuda).all(|(n, _)| n != "torch"));
        assert!(cuda
            .iter()
            .all(|(n, _)| n.starts_with("nvidia-") || n.starts_with("cuda-") || n == "triton"));
        assert!(common
            .iter()
            .all(|(n, _)| !n.starts_with("nvidia-") && !n.starts_with("cuda-") && n != "triton"));
        // Se instala exactamente el lock y se verifica la coherencia.
        assert!(dockerfile.contains("--no-deps -r /tmp/requirements.lock"));
        assert!(dockerfile.contains("--no-deps -r /tmp/requirements-cu126.lock"));
        assert!(dockerfile.contains("pip check"));
        // Los pins directos del Dockerfile coinciden con el lock.
        let code = dockerfile
            .lines()
            .filter(|l| !l.trim_start().starts_with('#'));
        for tok in code
            .flat_map(str::split_whitespace)
            .filter(|t| t.contains("=="))
        {
            let (name, ver) = tok.split_once("==").unwrap_or_default();
            let name = name.to_ascii_lowercase().replace('_', "-");
            if name == "torch" {
                continue;
            }
            let locked = common.iter().find(|(n, _)| *n == name);
            assert_eq!(
                locked.map(|(_, v)| v.as_str()),
                Some(ver),
                "{name} difiere del lock"
            );
        }
    }

    #[test]
    fn finetune_dockerfile_pins_are_mutually_installable() {
        // H-18: gguf 0.16.0 de PyPI no tiene `MODEL_ARCH.GEMMA3`, que el
        // conversor b4879 usa al importarse; 0.16.2 es la primera versión
        // publicada que sí (verificado: conversión Qwen2 completa). Exige
        // `sentencepiece>=0.1.98,<=0.2.0`; otro pin rompe el `pip install`.
        let dockerfile = include_str!("../../assets/finetune/Dockerfile");
        let pin = |name: &str| {
            dockerfile
                .split_whitespace()
                .find_map(|t| t.strip_prefix(&format!("{name}==")))
                .map(str::to_owned)
        };
        assert_eq!(pin("gguf").as_deref(), Some("0.16.2"));
        let sp = pin("sentencepiece").unwrap_or_default();
        let parts: Vec<u32> = sp.split('.').filter_map(|p| p.parse().ok()).collect();
        assert!(
            parts.as_slice() >= [0, 1, 98].as_slice() && parts.as_slice() <= [0, 2, 0].as_slice(),
            "sentencepiece=={sp} incompatible con gguf==0.16.2 (<=0.2.0)"
        );
    }
}
