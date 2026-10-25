// Fase 10.3 + 10.4 — Adapter de ciclo de vida de instancias de IA (Ollama + Open WebUI)
// reescrito íntegramente sobre `ContainerPort` (`BollardDockerAdapter`).
//
// Reglas inviolables aplicadas:
// - CERO `docker compose`, CERO `Command::new("docker")`.
// - Todos los puertos host bindean exclusivamente a `127.0.0.1` (`build_localhost_port_maps`
//   en `BollardDockerAdapter` + verificación explícita).
// - Reserva anti-TOCTOU de puertos con `infrastructure::port_lock::acquire_port_lock` (flock).
// - Hardening centralizado con `build_default_security_opt`, `no_new_privileges: true`,
//   `cap_drop: ["ALL"]` y soporte de `image_digest`.
// - Límites R20 calibrados para IA: `memory_limit` >= 8 GiB en Ollama (evita OOM exit 137
//   del default de 2 GiB), `pids_limit` >= 4096 (hilos BLAS/CUDA) y volúmenes persistentes.
// - Soporte GPU (Paso 10.4, R21) reutilizando `HardwareProbePort` (`EnolaHardwareProbe`),
//   sin duplicar sondas NVML/`nvidia-smi`, degradando a CPU con aviso claro si no hay GPU.

use crate::domain::ai::{
    ai_network_name, ollama_container_name, validate_ai_instance_name, webui_container_name,
    AiInstance, AiInstanceStatus, AiStackConfig, DEFAULT_AI_BASE_DIR, DEFAULT_AI_MODEL,
    DEFAULT_WEBUI_MEMORY_BYTES, MIN_AI_PIDS_LIMIT, MIN_OLLAMA_MEMORY_BYTES, OLLAMA_INTERNAL_PORT,
    WEBUI_INTERNAL_PORT,
};
use crate::domain::error::{EnolaError, Result};
use crate::infrastructure::file_lock::FileLock;
use crate::infrastructure::port_lock::acquire_port_lock;
use crate::infrastructure::security_opt::{apparmor_profile_opt, build_default_security_opt};
use crate::ports::ai::AiAdapterPort;
use crate::ports::container::{ContainerConfig, ContainerInfo, ContainerPort};
use crate::ports::hardware::{GpuBrand, HardwareProbePort};
use crate::ports::llm::LlmEngine;
use crate::ports::manifest::ManifestPort;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Metadatos persistidos en `<base_dir>/<name>/instance.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredInstanceMeta {
    name: String,
    model: String,
    api_port: u16,
    webui_port: u16,
    gpu_enabled: bool,
}

/// Adapter de infraestructura para gestionar stacks Ollama + Open WebUI vía `ContainerPort`.
pub struct EnolaAiAdapter {
    container_manager: Arc<dyn ContainerPort + Send + Sync>,
    hardware_probe: Arc<dyn HardwareProbePort + Send + Sync>,
    manifest: Arc<dyn ManifestPort + Send + Sync>,
    base_dir: PathBuf,
}

impl EnolaAiAdapter {
    /// Crea un nuevo `EnolaAiAdapter` con el directorio de datos por defecto (`/srv/enola-ai`).
    pub fn new(
        container_manager: Arc<dyn ContainerPort + Send + Sync>,
        hardware_probe: Arc<dyn HardwareProbePort + Send + Sync>,
        manifest: Arc<dyn ManifestPort + Send + Sync>,
    ) -> Self {
        Self {
            container_manager,
            hardware_probe,
            manifest,
            base_dir: PathBuf::from(DEFAULT_AI_BASE_DIR),
        }
    }

    /// Constructor con `base_dir` inyectable (tests unitarios sin tocar `/srv/enola-ai`).
    pub fn new_with_base(
        container_manager: Arc<dyn ContainerPort + Send + Sync>,
        hardware_probe: Arc<dyn HardwareProbePort + Send + Sync>,
        manifest: Arc<dyn ManifestPort + Send + Sync>,
        base_dir: PathBuf,
    ) -> Self {
        Self {
            container_manager,
            hardware_probe,
            manifest,
            base_dir,
        }
    }

    /// Directorio en disco de una instancia: `<base_dir>/<name>`.
    pub fn instance_dir(&self, name: &str) -> PathBuf {
        self.base_dir.join(name)
    }

    fn meta_file_path(&self, name: &str) -> PathBuf {
        self.instance_dir(name).join("instance.json")
    }

    fn read_meta(&self, name: &str) -> Option<StoredInstanceMeta> {
        let path = self.meta_file_path(name);
        let raw = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&raw).ok()
    }

    fn write_meta(&self, meta: &StoredInstanceMeta) -> Result<()> {
        let dir = self.instance_dir(&meta.name);
        std::fs::create_dir_all(&dir).map_err(|e| {
            EnolaError::InfrastructureError(format!(
                "No se pudo crear el directorio de instancia IA '{}': {e}",
                dir.display()
            ))
        })?;
        let json = serde_json::to_string_pretty(meta).map_err(|e| {
            EnolaError::InfrastructureError(format!("No se pudo serializar metadatos IA: {e}"))
        })?;
        std::fs::write(self.meta_file_path(&meta.name), json).map_err(|e| {
            EnolaError::InfrastructureError(format!("No se pudo escribir instance.json: {e}"))
        })
    }

    /// Reserva un puerto con `acquire_port_lock` (flock exclusivo no bloqueante).
    fn reserve_port(port: u16, label: &str) -> Result<FileLock> {
        acquire_port_lock(port).map_err(|e| {
            if e.kind() == ErrorKind::WouldBlock {
                EnolaError::ValidationError(format!(
                    "El puerto {port} ({label}) está reservado por otra operación concurrente de enola-cli"
                ))
            } else {
                EnolaError::InfrastructureError(format!(
                    "No se pudo adquirir el lockfile del puerto {port} ({label}): {e}"
                ))
            }
        })
    }

    /// Resuelve si hay GPU NVIDIA disponible a través de `HardwareProbePort` (Paso 10.4).
    /// Si el usuario solicitó GPU pero no hay GPU NVIDIA disponible, degrada a CPU con warning.
    pub async fn resolve_effective_gpu(&self, requested_gpu: bool, instance_name: &str) -> bool {
        if !requested_gpu {
            return false;
        }
        match self.hardware_probe.probe().await {
            Ok(specs) => {
                let has_nvidia = specs.gpu_brand == GpuBrand::Nvidia
                    || specs.gpus.iter().any(|g| g.brand == GpuBrand::Nvidia);
                if has_nvidia {
                    true
                } else {
                    tracing::warn!(
                        instance = instance_name,
                        "No se detectó GPU NVIDIA disponible; degradando instancia IA a modo CPU"
                    );
                    eprintln!(
                        "⚠️  No se detectó GPU NVIDIA disponible para '{}'; degradando a modo CPU.",
                        instance_name
                    );
                    false
                }
            }
            Err(err) => {
                tracing::warn!(
                    instance = instance_name,
                    error = %err,
                    "Fallo al sondear hardware GPU; degradando instancia IA a modo CPU"
                );
                eprintln!(
                    "⚠️  No se pudo verificar GPU NVIDIA ({}); degradando '{}' a modo CPU.",
                    err, instance_name
                );
                false
            }
        }
    }

    /// Construye el `ContainerConfig` endurecido y calibrado (R20) para Ollama.
    pub fn build_ollama_container_config(
        config: &AiStackConfig,
        effective_gpu: bool,
        inst_dir: &Path,
    ) -> ContainerConfig {
        let container_name = ollama_container_name(&config.name);
        let net_name = ai_network_name(&config.name);
        let ollama_data = inst_dir.join("ollama");

        let mut env = HashMap::new();
        env.insert("OLLAMA_HOST".to_string(), "0.0.0.0:11434".to_string());
        env.insert(
            "OLLAMA_MODELS".to_string(),
            "/root/.ollama/models".to_string(),
        );

        let mut ports = HashMap::new();
        ports.insert(config.api_port, OLLAMA_INTERNAL_PORT);

        let mut volumes = HashMap::new();
        volumes.insert(
            ollama_data.to_string_lossy().to_string(),
            "/root/.ollama".to_string(),
        );

        let mut user_sec_opts = Vec::new();
        if let Some(profile) = apparmor_profile_opt(&container_name) {
            user_sec_opts.push(profile);
        }
        let security_opt = build_default_security_opt(user_sec_opts);

        ContainerConfig {
            name: container_name,
            image: config.ollama_image.clone(),
            command: None,
            env,
            ports,
            volumes,
            network: Some(net_name),
            restart_policy: Some("unless-stopped".to_string()),
            gpu_support: effective_gpu,
            auto_remove: false,
            working_dir: None,
            security_opt,
            memory_limit: Some(config.memory_limit_bytes.max(MIN_OLLAMA_MEMORY_BYTES)),
            nano_cpus: Some(config.nano_cpus),
            pids_limit: Some(config.pids_limit.max(MIN_AI_PIDS_LIMIT)),
            secrets: HashMap::new(),
            // R20: Ollama escribe cachés temporales y metadatos de runtime además de `/root/.ollama`.
            read_only_rootfs: false,
            no_new_privileges: true,
            cap_drop: vec!["ALL".to_string()],
            cap_add: vec![
                "CHOWN".to_string(),
                "SETUID".to_string(),
                "SETGID".to_string(),
                "DAC_OVERRIDE".to_string(),
            ],
            image_digest: config.ollama_image_digest.clone(),
        }
    }

    /// Construye el `ContainerConfig` endurecido y calibrado (R20) para Open WebUI.
    pub fn build_webui_container_config(
        config: &AiStackConfig,
        inst_dir: &Path,
    ) -> ContainerConfig {
        let container_name = webui_container_name(&config.name);
        let ollama_name = ollama_container_name(&config.name);
        let net_name = ai_network_name(&config.name);
        let webui_data = inst_dir.join("webui");

        let mut env = HashMap::new();
        env.insert(
            "OLLAMA_BASE_URL".to_string(),
            format!("http://{ollama_name}:{OLLAMA_INTERNAL_PORT}"),
        );
        env.insert("DEFAULT_MODELS".to_string(), config.model.clone());
        env.insert("WEBUI_AUTH".to_string(), "true".to_string());
        env.insert("SCARF_NO_ANALYTICS".to_string(), "true".to_string());
        env.insert("DO_NOT_TRACK".to_string(), "true".to_string());
        env.insert("ANONYMIZED_TELEMETRY".to_string(), "false".to_string());

        let mut ports = HashMap::new();
        ports.insert(config.webui_port, WEBUI_INTERNAL_PORT);

        let mut volumes = HashMap::new();
        volumes.insert(
            webui_data.to_string_lossy().to_string(),
            "/app/backend/data".to_string(),
        );

        let mut user_sec_opts = Vec::new();
        if let Some(profile) = apparmor_profile_opt(&container_name) {
            user_sec_opts.push(profile);
        }
        let security_opt = build_default_security_opt(user_sec_opts);

        ContainerConfig {
            name: container_name,
            image: config.webui_image.clone(),
            command: None,
            env,
            ports,
            volumes,
            network: Some(net_name),
            restart_policy: Some("unless-stopped".to_string()),
            gpu_support: false,
            auto_remove: false,
            working_dir: None,
            security_opt,
            memory_limit: Some(
                config
                    .webui_memory_limit_bytes
                    .max(DEFAULT_WEBUI_MEMORY_BYTES),
            ),
            nano_cpus: Some(config.nano_cpus),
            pids_limit: Some(config.pids_limit.max(MIN_AI_PIDS_LIMIT)),
            secrets: HashMap::new(),
            // R20: Open WebUI inicializa migraciones SQLite y cachés en `/app/backend/data` y `/tmp`.
            read_only_rootfs: false,
            no_new_privileges: true,
            cap_drop: vec!["ALL".to_string()],
            cap_add: vec![
                "CHOWN".to_string(),
                "SETUID".to_string(),
                "SETGID".to_string(),
                "DAC_OVERRIDE".to_string(),
            ],
            image_digest: config.webui_image_digest.clone(),
        }
    }

    /// Extrae el puerto host mapeado a `internal_port` desde las cadenas de `ContainerInfo::ports`.
    fn parse_host_port(ports: &[String], internal_port: u16) -> Option<u16> {
        let needle = format!("->{internal_port}/tcp");
        for entry in ports {
            if let Some(prefix) = entry.split(needle.as_str()).next() {
                if prefix.len() == entry.len() {
                    continue;
                }
                let host_part = prefix.rsplit(':').next().unwrap_or(prefix);
                if let Ok(port) = host_part.trim().parse::<u16>() {
                    return Some(port);
                }
            }
        }
        None
    }

    fn is_running_status(status: &str) -> bool {
        let lower = status.to_ascii_lowercase();
        lower.contains("up") || lower.contains("running")
    }

    fn build_instance_from_state(
        &self,
        name: &str,
        ollama: Option<&ContainerInfo>,
        webui: Option<&ContainerInfo>,
    ) -> Option<AiInstance> {
        let meta = self.read_meta(name);
        if ollama.is_none() && webui.is_none() && meta.is_none() {
            return None;
        }

        let ollama_running = ollama.map(|c| Self::is_running_status(&c.status));
        let webui_running = webui.map(|c| Self::is_running_status(&c.status));
        let status = AiInstanceStatus::from_container_states(ollama_running, webui_running);

        let api_port = ollama
            .and_then(|c| Self::parse_host_port(&c.ports, OLLAMA_INTERNAL_PORT))
            .or_else(|| meta.as_ref().map(|m| m.api_port));
        let webui_port = webui
            .and_then(|c| Self::parse_host_port(&c.ports, WEBUI_INTERNAL_PORT))
            .or_else(|| meta.as_ref().map(|m| m.webui_port));

        let model = meta
            .as_ref()
            .map(|m| m.model.clone())
            .unwrap_or_else(|| DEFAULT_AI_MODEL.to_string());
        let gpu_enabled = meta.as_ref().map(|m| m.gpu_enabled).unwrap_or(false);

        Some(AiInstance {
            name: name.to_string(),
            model,
            api_port,
            webui_port,
            status,
            ollama_status: ollama
                .map(|c| c.status.clone())
                .unwrap_or_else(|| "missing".to_string()),
            webui_status: webui
                .map(|c| c.status.clone())
                .unwrap_or_else(|| "missing".to_string()),
            gpu_enabled,
            onion_address: None,
        })
    }

    async fn rollback_partial_create(
        &self,
        name: &str,
        ollama_created: bool,
        webui_created: bool,
        inst_dir: &Path,
    ) {
        let webui_name = webui_container_name(name);
        let ollama_name = ollama_container_name(name);
        let net_name = ai_network_name(name);

        if webui_created {
            let _ = self.container_manager.stop_container(&webui_name).await;
            let _ = self.container_manager.remove_container(&webui_name).await;
        }
        if ollama_created {
            let _ = self.container_manager.stop_container(&ollama_name).await;
            let _ = self.container_manager.remove_container(&ollama_name).await;
        }
        let _ = self.container_manager.remove_network(&net_name).await;
        if inst_dir.exists() {
            let _ = std::fs::remove_dir_all(inst_dir);
        }
    }
}

#[async_trait::async_trait]
impl AiAdapterPort for EnolaAiAdapter {
    async fn create_instance(&self, config: AiStackConfig) -> Result<AiInstance> {
        validate_ai_instance_name(&config.name)?;

        // 1. Adquirir locks exclusivos de puerto ANTES de tocar Docker (SEC-EXT-RACE-011).
        //    Los guards viven hasta que ambos contenedores se han creado y arrancado.
        let _api_port_lock = Self::reserve_port(config.api_port, "api-port")?;
        let _webui_port_lock = Self::reserve_port(config.webui_port, "webui-port")?;

        // SEC-A7: directorio base de datos en 0700 (webui.db, claves de sesión).
        crate::infrastructure::safe_dir::secure_service_base_dir(&self.base_dir).map_err(|e| {
            EnolaError::InfrastructureError(format!(
                "Directorio de datos inseguro {}: {}",
                self.base_dir.display(),
                e
            ))
        })?;

        // 2. Comprobar que la instancia no exista ya.
        let inst_dir = self.instance_dir(&config.name);
        let ollama_name = ollama_container_name(&config.name);
        let webui_name = webui_container_name(&config.name);
        let existing_containers = self.container_manager.list_containers(true).await?;
        if existing_containers
            .iter()
            .any(|c| c.name == ollama_name || c.name == webui_name)
            || self.meta_file_path(&config.name).exists()
        {
            return Err(EnolaError::ValidationError(format!(
                "La instancia IA '{}' ya existe",
                config.name
            )));
        }

        // 3. Resolver soporte GPU vía HardwareProbePort (Paso 10.4 — R21).
        let effective_gpu = self
            .resolve_effective_gpu(config.gpu_enabled, &config.name)
            .await;

        // 4. Preparar directorios de datos persistentes.
        let ollama_data = inst_dir.join("ollama");
        let webui_data = inst_dir.join("webui");
        std::fs::create_dir_all(&ollama_data).map_err(|e| {
            EnolaError::InfrastructureError(format!(
                "No se pudo crear directorio de datos Ollama '{}': {e}",
                ollama_data.display()
            ))
        })?;
        std::fs::create_dir_all(&webui_data).map_err(|e| {
            EnolaError::InfrastructureError(format!(
                "No se pudo crear directorio de datos WebUI '{}': {e}",
                webui_data.display()
            ))
        })?;

        // 5. Crear red Docker aislada.
        let net_name = ai_network_name(&config.name);
        if let Err(err) = self.container_manager.create_network(&net_name).await {
            // La red puede ya existir (limpieza previa incompleta — hallazgo de
            // la E2E real): es nuestra por naming `enola_net_ai_<n>` → reutilizar.
            if !err.to_string().to_lowercase().contains("already exists") {
                let _ = std::fs::remove_dir_all(&inst_dir);
                return Err(err);
            }
            tracing::warn!(
                network = %net_name,
                "La red ya existe; se reutiliza (restos de una instancia anterior)"
            );
        }

        // 6. Crear y arrancar contenedor Ollama.
        let ollama_cfg = Self::build_ollama_container_config(&config, effective_gpu, &inst_dir);
        if let Err(err) = self.container_manager.create_container(ollama_cfg).await {
            self.rollback_partial_create(&config.name, false, false, &inst_dir)
                .await;
            return Err(err);
        }
        if let Err(err) = self.container_manager.start_container(&ollama_name).await {
            self.rollback_partial_create(&config.name, true, false, &inst_dir)
                .await;
            return Err(err);
        }

        // 7. Crear y arrancar contenedor Open WebUI.
        let webui_cfg = Self::build_webui_container_config(&config, &inst_dir);
        if let Err(err) = self.container_manager.create_container(webui_cfg).await {
            self.rollback_partial_create(&config.name, true, false, &inst_dir)
                .await;
            return Err(err);
        }
        if let Err(err) = self.container_manager.start_container(&webui_name).await {
            self.rollback_partial_create(&config.name, true, true, &inst_dir)
                .await;
            return Err(err);
        }

        // 8. Persistir metadatos y registrar en el manifest de desinstalación.
        let meta = StoredInstanceMeta {
            name: config.name.clone(),
            model: config.model.clone(),
            api_port: config.api_port,
            webui_port: config.webui_port,
            gpu_enabled: effective_gpu,
        };
        if let Err(err) = self.write_meta(&meta) {
            self.rollback_partial_create(&config.name, true, true, &inst_dir)
                .await;
            return Err(err);
        }

        let _ = self.manifest.append("docker_network", &net_name);
        let _ = self.manifest.append("docker_container", &ollama_name);
        let _ = self.manifest.append("docker_container", &webui_name);
        let _ = self.manifest.append("dir", &inst_dir.to_string_lossy());

        Ok(AiInstance {
            name: config.name,
            model: config.model,
            api_port: Some(config.api_port),
            webui_port: Some(config.webui_port),
            status: AiInstanceStatus::Running,
            ollama_status: "Up".to_string(),
            webui_status: "Up".to_string(),
            gpu_enabled: effective_gpu,
            onion_address: None,
        })
    }

    async fn list_instances(&self) -> Result<Vec<AiInstance>> {
        let containers = self.container_manager.list_containers(true).await?;
        let mut names = BTreeSet::new();

        for c in &containers {
            if let Some(rest) = c.name.strip_prefix("enola-ai-") {
                if let Some(name) = rest.strip_suffix("-ollama") {
                    if validate_ai_instance_name(name).is_ok() {
                        names.insert(name.to_string());
                    }
                } else if let Some(name) = rest.strip_suffix("-webui") {
                    if validate_ai_instance_name(name).is_ok() {
                        names.insert(name.to_string());
                    }
                }
            }
        }

        if let Ok(entries) = std::fs::read_dir(&self.base_dir) {
            for entry in entries.flatten() {
                if entry.path().is_dir() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if validate_ai_instance_name(&name).is_ok()
                        && self.meta_file_path(&name).exists()
                    {
                        names.insert(name);
                    }
                }
            }
        }

        let mut instances = Vec::new();
        for name in names {
            let ollama_name = ollama_container_name(&name);
            let webui_name = webui_container_name(&name);
            let ollama = containers.iter().find(|c| c.name == ollama_name);
            let webui = containers.iter().find(|c| c.name == webui_name);
            if let Some(inst) = self.build_instance_from_state(&name, ollama, webui) {
                instances.push(inst);
            }
        }
        Ok(instances)
    }

    async fn get_instance(&self, name: &str) -> Result<AiInstance> {
        validate_ai_instance_name(name)?;
        let containers = self.container_manager.list_containers(true).await?;
        let ollama_name = ollama_container_name(name);
        let webui_name = webui_container_name(name);
        let ollama = containers.iter().find(|c| c.name == ollama_name);
        let webui = containers.iter().find(|c| c.name == webui_name);

        self.build_instance_from_state(name, ollama, webui)
            .ok_or_else(|| EnolaError::NotFound(format!("Instancia IA '{}' no encontrada", name)))
    }

    async fn start_instance(&self, name: &str) -> Result<()> {
        validate_ai_instance_name(name)?;
        let _ = self.get_instance(name).await?;
        let ollama_name = ollama_container_name(name);
        let webui_name = webui_container_name(name);
        self.container_manager.start_container(&ollama_name).await?;
        self.container_manager.start_container(&webui_name).await?;
        Ok(())
    }

    async fn stop_instance(&self, name: &str) -> Result<()> {
        validate_ai_instance_name(name)?;
        let _ = self.get_instance(name).await?;
        let webui_name = webui_container_name(name);
        let ollama_name = ollama_container_name(name);
        let _ = self.container_manager.stop_container(&webui_name).await;
        self.container_manager.stop_container(&ollama_name).await?;
        Ok(())
    }

    async fn delete_instance(&self, name: &str, force: bool) -> Result<()> {
        validate_ai_instance_name(name)?;
        if !force {
            return Err(EnolaError::ValidationError(format!(
                "Eliminar la instancia IA '{name}' requiere confirmar con --force"
            )));
        }

        let _ = self.get_instance(name).await?;
        let net_name = ai_network_name(name);

        // Borrar TODOS los contenedores de la instancia (ollama, webui,
        // vector store, finetune) — la E2E real mostró huérfanos
        // (`enola-ai-<n>-chromadb`) que bloqueaban el borrado de la red.
        // Sufijo exacto, no prefijo: `dev-test` no es de `dev` (H-02).
        let all = self
            .container_manager
            .list_containers(true)
            .await
            .unwrap_or_default();
        for c in all
            .iter()
            .filter(|c| crate::domain::ai::is_instance_container(name, &c.name))
        {
            let _ = self.container_manager.stop_container(&c.name).await;
            let _ = self.container_manager.remove_container(&c.name).await;
        }
        let _ = self.container_manager.remove_network(&net_name).await;

        let inst_dir = self.instance_dir(name);
        if inst_dir.exists() {
            std::fs::remove_dir_all(&inst_dir).map_err(|e| {
                EnolaError::InfrastructureError(format!(
                    "No se pudo eliminar el directorio '{}': {e}",
                    inst_dir.display()
                ))
            })?;
        }
        Ok(())
    }

    async fn get_instance_logs(&self, name: &str, lines: usize) -> Result<String> {
        validate_ai_instance_name(name)?;
        let _ = self.get_instance(name).await?;
        let ollama_name = ollama_container_name(name);
        let webui_name = webui_container_name(name);

        let ollama_logs = self
            .container_manager
            .get_logs(&ollama_name, lines)
            .await
            .unwrap_or_else(|e| format!("(no logs from {ollama_name}: {e})"));
        let webui_logs = self
            .container_manager
            .get_logs(&webui_name, lines)
            .await
            .unwrap_or_else(|e| format!("(no logs from {webui_name}: {e})"));

        Ok(format!(
            "=== {ollama_name} ===\n{}\n=== {webui_name} ===\n{}",
            ollama_logs.trim_end(),
            webui_logs.trim_end()
        ))
    }
}

// ── Fase 11.2/11.3 — biblioteca de modelos GGUF + registro Ollama ──────────
//
// La biblioteca vive en `<base>/<instance>/ollama/models-gguf/`, dentro del
// volumen que Ollama monta en `/root/.ollama`: el contenedor ve los ficheros
// como `/root/.ollama/models-gguf/<file>`. El registro en Ollama no usa esa
// ruta: sube el fichero como blob (`LlmEngine::create_model_from_gguf`, H-16).
// `index.json` (modo 0640) guarda proveniencia (sha256, origen) por fichero.

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ModelIndexEntry {
    #[serde(default)]
    sha256: Option<String>,
    #[serde(default)]
    source: Option<String>,
    /// Repositorio HF (`org/repo`) de origen, para recuperar el tokenizer.
    #[serde(default)]
    hf_repo: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ModelIndex {
    #[serde(default)]
    files: HashMap<String, ModelIndexEntry>,
}

impl EnolaAiAdapter {
    fn library_dir(&self, instance: &str) -> Result<PathBuf> {
        crate::domain::ai_model::library_dir_for_instance(&self.base_dir, instance)
    }

    fn index_path(&self, instance: &str) -> Result<PathBuf> {
        Ok(self.library_dir(instance)?.join("index.json"))
    }

    fn read_index(&self, instance: &str) -> ModelIndex {
        let Ok(path) = self.index_path(instance) else {
            return ModelIndex::default();
        };
        std::fs::read_to_string(path)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default()
    }

    fn write_index(&self, instance: &str, index: &ModelIndex) -> Result<()> {
        let path = self.index_path(instance)?;
        let json = serde_json::to_string_pretty(index).map_err(|e| {
            EnolaError::InfrastructureError(format!("No se pudo serializar index.json: {e}"))
        })?;
        std::fs::write(&path, json).map_err(|e| {
            EnolaError::InfrastructureError(format!("No se pudo escribir index.json: {e}"))
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640));
        }
        Ok(())
    }

    fn index_entry_update(
        &self,
        instance: &str,
        filename: &str,
        sha256: Option<String>,
        source: &str,
        hf_repo: Option<String>,
    ) -> Result<()> {
        let mut idx = self.read_index(instance);
        idx.files.insert(
            filename.to_string(),
            ModelIndexEntry {
                sha256,
                source: Some(source.to_string()),
                hf_repo,
            },
        );
        self.write_index(instance, &idx)
    }

    fn index_entry_remove(&self, instance: &str, filename: &str) -> Result<()> {
        let mut idx = self.read_index(instance);
        idx.files.remove(filename);
        self.write_index(instance, &idx)
    }

    /// Endpoint HTTP del Ollama de la instancia (`http://127.0.0.1:<api_port>`).
    async fn ollama_endpoint(&self, instance: &str) -> Result<String> {
        let inst = self.get_instance(instance).await?;
        let port = inst.api_port.ok_or_else(|| {
            EnolaError::ValidationError(format!(
                "La instancia IA '{instance}' no tiene puerto API activo (¿está en ejecución?)"
            ))
        })?;
        Ok(format!("http://127.0.0.1:{port}"))
    }

    fn ollama_engine(&self, endpoint: &str) -> crate::adapters::ai::ollama::OllamaAdapter {
        crate::adapters::ai::ollama::OllamaAdapter::new(
            endpoint,
            std::time::Duration::from_secs(600),
        )
    }

    fn model_from_path(
        &self,
        _instance: &str,
        path: &Path,
        index: &ModelIndex,
    ) -> Result<crate::domain::ai_model::AiModel> {
        use crate::domain::ai_model::{
            default_registry_name, parse_filename_heuristics, AiModel, AiModelStatus, ModelSource,
        };
        let filename = path
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_default();
        let size_bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        let head = std::fs::File::open(path)
            .and_then(|mut f| {
                use std::io::Read;
                let mut buf = vec![0u8; crate::domain::ai_model::GGUF_MIN_HEADER_LEN];
                let n = f.read(&mut buf)?;
                buf.truncate(n);
                Ok(buf)
            })
            .unwrap_or_default();
        let status = match crate::domain::ai_model::validate_gguf_header(&head) {
            Ok(_) => AiModelStatus::Ready,
            Err(_) => AiModelStatus::Corrupt,
        };
        let (family, quantization) = parse_filename_heuristics(&filename);
        let entry = index.files.get(&filename);
        Ok(AiModel {
            name: default_registry_name(&filename).unwrap_or_else(|_| filename.clone()),
            filename,
            size_bytes,
            quantization,
            family,
            status,
            source: match entry.and_then(|e| e.source.as_deref()) {
                Some("imported") => ModelSource::Imported,
                _ => ModelSource::Downloaded,
            },
            sha256: entry.and_then(|e| e.sha256.clone()),
            hf_repo: entry.and_then(|e| e.hf_repo.clone()),
        })
    }
}

#[async_trait::async_trait]
impl crate::ports::ai::AiModelLibraryPort for EnolaAiAdapter {
    async fn resolve_library_target(&self, instance: &str, filename: &str) -> Result<PathBuf> {
        let path =
            crate::domain::ai_model::resolve_library_path(&self.base_dir, instance, filename)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                EnolaError::InfrastructureError(format!(
                    "No se pudo crear la biblioteca de modelos '{}': {e}",
                    parent.display()
                ))
            })?;
        }
        Ok(path)
    }

    async fn list_library_models(
        &self,
        instance: &str,
    ) -> Result<Vec<crate::domain::ai_model::AiModel>> {
        let dir = self.library_dir(instance)?;
        let index = self.read_index(instance);
        let mut models = Vec::new();
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(models),
            Err(e) => {
                return Err(EnolaError::InfrastructureError(format!(
                    "No se pudo leer la biblioteca '{}': {e}",
                    dir.display()
                )))
            }
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.to_ascii_lowercase().ends_with(".gguf") {
                continue;
            }
            models.push(self.model_from_path(instance, &path, &index)?);
        }
        models.sort_by(|a, b| a.filename.cmp(&b.filename));
        Ok(models)
    }

    async fn read_gguf_metadata(
        &self,
        instance: &str,
        filename: &str,
    ) -> Result<crate::domain::ai_model::GgufMetadata> {
        let path =
            crate::domain::ai_model::resolve_library_path(&self.base_dir, instance, filename)?;
        let file = std::fs::File::open(&path).map_err(|e| {
            EnolaError::NotFound(format!(
                "Fichero '{filename}' no encontrado en la biblioteca de '{instance}': {e}"
            ))
        })?;
        // Leer el header completo (metadata incluida) para extraer
        // `general.architecture` y `general.file_type`. `read_to_end` sobre
        // `take`: un único `read` puede devolver menos bytes de los pedidos.
        use std::io::Read;
        let mut buf = Vec::new();
        file.take(crate::domain::ai_model::GGUF_HEADER_READ_LEN as u64)
            .read_to_end(&mut buf)
            .map_err(|e| {
                EnolaError::InfrastructureError(format!("No se pudo leer '{filename}': {e}"))
            })?;
        crate::domain::ai_model::parse_gguf_header(&buf)
    }

    async fn import_library_file(
        &self,
        instance: &str,
        source: &Path,
    ) -> Result<crate::domain::ai_model::AiModel> {
        let filename = source
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .ok_or_else(|| {
                EnolaError::ValidationError(format!(
                    "La ruta '{}' no tiene nombre de fichero válido",
                    source.display()
                ))
            })?;
        crate::domain::ai_model::validate_gguf_filename(&filename)?;

        // Validar el header ANTES de copiar (rechaza falsos .gguf).
        let mut src = std::fs::File::open(source).map_err(|e| {
            EnolaError::InfrastructureError(format!("No se pudo abrir '{}': {e}", source.display()))
        })?;
        let mut head = vec![0u8; crate::domain::ai_model::GGUF_MIN_HEADER_LEN];
        use std::io::Read;
        let n = src.read(&mut head).map_err(|e| {
            EnolaError::InfrastructureError(format!("No se pudo leer '{}': {e}", source.display()))
        })?;
        head.truncate(n);
        crate::domain::ai_model::validate_gguf_header(&head)?;
        // El `read` anterior avanzó el cursor: rebobinar para copiar el fichero
        // COMPLETO (header incluido), no desde el byte 24.
        use std::io::Seek;
        src.seek(std::io::SeekFrom::Start(0)).map_err(|e| {
            EnolaError::InfrastructureError(format!(
                "No se pudo rebobinar '{}': {e}",
                source.display()
            ))
        })?;

        let dest = self.resolve_library_target(instance, &filename).await?;
        // Copia con hash SHA-256 en streaming (los GGUF pueden ser GiB).
        let mut dst = std::fs::File::create(&dest).map_err(|e| {
            EnolaError::InfrastructureError(format!("No se pudo crear '{}': {e}", dest.display()))
        })?;
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        let mut buf = [0u8; 65536];
        loop {
            let n = src.read(&mut buf).map_err(|e| {
                EnolaError::InfrastructureError(format!("Leyendo '{}': {e}", source.display()))
            })?;
            if n == 0 {
                break;
            }
            use std::io::Write;
            dst.write_all(&buf[..n]).map_err(|e| {
                EnolaError::InfrastructureError(format!("Escribiendo '{}': {e}", dest.display()))
            })?;
            hasher.update(&buf[..n]);
        }
        drop(dst);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o640)).map_err(
                |e| {
                    EnolaError::InfrastructureError(format!(
                        "Permisos 0640 en '{}': {e}",
                        dest.display()
                    ))
                },
            )?;
        }
        let sha256 = format!("{:x}", hasher.finalize());
        self.index_entry_update(instance, &filename, Some(sha256), "imported", None)?;
        let index = self.read_index(instance);
        self.model_from_path(instance, &dest, &index)
    }

    async fn delete_library_file(&self, instance: &str, filename: &str) -> Result<()> {
        let path =
            crate::domain::ai_model::resolve_library_path(&self.base_dir, instance, filename)?;
        std::fs::remove_file(&path).map_err(|e| {
            if e.kind() == ErrorKind::NotFound {
                EnolaError::NotFound(format!(
                    "Fichero '{filename}' no encontrado en la biblioteca de '{instance}'"
                ))
            } else {
                EnolaError::InfrastructureError(format!(
                    "No se pudo eliminar '{}': {e}",
                    path.display()
                ))
            }
        })?;
        let _ = self.index_entry_remove(instance, filename);
        Ok(())
    }

    async fn record_download_provenance(
        &self,
        instance: &str,
        filename: &str,
        sha256: &str,
        hf_repo: Option<String>,
    ) -> Result<()> {
        crate::domain::ai_model::validate_gguf_filename(filename)?;
        self.index_entry_update(
            instance,
            filename,
            Some(sha256.to_string()),
            "downloaded",
            hf_repo,
        )
    }

    async fn resolve_tokenizer_dir(&self, instance: &str, repo: &str) -> Result<PathBuf> {
        crate::domain::ai_model::tokenizer_dir_for_repo(&self.base_dir, instance, repo)
    }
}

#[async_trait::async_trait]
impl crate::ports::ai::AiModelRegistryPort for EnolaAiAdapter {
    async fn list_registered_models(
        &self,
        instance: &str,
    ) -> Result<Vec<crate::ports::llm::LlmModelInfo>> {
        let endpoint = self.ollama_endpoint(instance).await?;
        self.ollama_engine(&endpoint).list_models().await
    }

    async fn pull_registry_model(&self, instance: &str, model: &str) -> Result<()> {
        crate::domain::ai_model::validate_ollama_model_name(model)?;
        let endpoint = self.ollama_endpoint(instance).await?;
        self.ollama_engine(&endpoint).pull_model(model).await
    }

    async fn register_library_model(
        &self,
        instance: &str,
        filename: &str,
        name: &str,
    ) -> Result<()> {
        crate::domain::ai_model::validate_ollama_model_name(name)?;
        // El fichero debe existir en la biblioteca de la instancia.
        let path =
            crate::domain::ai_model::resolve_library_path(&self.base_dir, instance, filename)?;
        if !path.exists() {
            return Err(EnolaError::NotFound(format!(
                "El fichero '{filename}' no está en la biblioteca de '{instance}' \
                 (usa `ai model download` o `ai model import` primero)"
            )));
        }
        // Validar header antes de registrar en Ollama.
        let mut file = std::fs::File::open(&path).map_err(|e| {
            EnolaError::InfrastructureError(format!("No se pudo abrir '{filename}': {e}"))
        })?;
        let mut head = vec![0u8; crate::domain::ai_model::GGUF_MIN_HEADER_LEN];
        use std::io::Read;
        let n = file.read(&mut head).map_err(|e| {
            EnolaError::InfrastructureError(format!("No se pudo leer '{filename}': {e}"))
        })?;
        head.truncate(n);
        crate::domain::ai_model::validate_gguf_header(&head)?;

        let endpoint = self.ollama_endpoint(instance).await?;
        self.ollama_engine(&endpoint)
            .create_model_from_gguf(name, &path)
            .await
    }

    async fn delete_registered_model(&self, instance: &str, model: &str) -> Result<()> {
        crate::domain::ai_model::validate_ollama_model_name(model)?;
        let endpoint = self.ollama_endpoint(instance).await?;
        self.ollama_engine(&endpoint).delete_model(model).await
    }
}

// ── Fase 12.3 — vector store por instancia (ChromaDB / Qdrant) ────────────
//
// El contenedor `enola-ai-<name>-<chromadb|qdrant>` se aprovisiona PEREZOSO
// (la primera vez que `ai rag` lo necesita) sobre `ContainerPort` en la red
// `enola_net_ai_<name>`, bind 127.0.0.1, puerto host de AI_VECTOR_PORT_RANGE
// con `acquire_port_lock`, volumen persistente `<inst>/<backend>` →
// `container_data_path()` y los mismos guardarraíles que el resto del stack.

/// Límites calibrados del vector store (R20-lite: no necesita 8 GiB).
const VECTOR_STORE_MEMORY_BYTES: i64 = 1024 * 1024 * 1024; // 1 GiB
const VECTOR_STORE_PIDS_LIMIT: i64 = 512;

#[async_trait::async_trait]
impl crate::ports::rag::AiVectorStorePort for EnolaAiAdapter {
    async fn ensure_vector_store(
        &self,
        instance: &str,
        backend: crate::domain::rag::VectorStoreBackend,
    ) -> Result<crate::ports::rag::VectorStoreInfo> {
        validate_ai_instance_name(instance)?;
        // La instancia debe existir (meta o contenedores).
        let inst_dir = self.instance_dir(instance);
        if !self.meta_file_path(instance).exists() && !inst_dir.exists() {
            return Err(EnolaError::NotFound(format!(
                "La instancia IA '{instance}' no existe (crea con `ai create`)"
            )));
        }
        if let Some(info) = self.vector_store_endpoint(instance).await? {
            if info.backend == backend {
                return Ok(info);
            }
            return Err(EnolaError::ValidationError(format!(
                "La instancia '{instance}' ya usa {:?}; para cambiar de backend borra el contenedor 'enola-ai-{instance}-{}'",
                info.backend,
                info.backend.container_suffix()
            )));
        }

        let suffix = backend.container_suffix();
        let container_name = format!("enola-ai-{instance}-{suffix}");
        let data_dir = inst_dir.join(suffix);
        std::fs::create_dir_all(&data_dir).map_err(|e| {
            EnolaError::InfrastructureError(format!(
                "No se pudo crear el volumen del vector store '{}': {e}",
                data_dir.display()
            ))
        })?;

        // Puerto libre dentro del rango dedicado con flock.
        let range = crate::domain::rag::AI_VECTOR_PORT_RANGE;
        let mut port: Option<u16> = None;
        let mut locks: Vec<FileLock> = Vec::new();
        {
            let existing = self.container_manager.list_containers(true).await?;
            'scan: for p in range.0..=range.1 {
                if existing
                    .iter()
                    .any(|c| c.ports.iter().any(|s| s.contains(&format!(":{p}->"))))
                {
                    continue;
                }
                match acquire_port_lock(p) {
                    Ok(lock) => {
                        locks.push(lock);
                        port = Some(p);
                        break 'scan;
                    }
                    Err(_) => continue,
                }
            }
        }
        let host_port = port.ok_or_else(|| {
            EnolaError::InfrastructureError(format!(
                "No hay puerto libre para el vector store en {}-{}",
                range.0, range.1
            ))
        })?;

        let mut ports = HashMap::new();
        ports.insert(host_port, backend.container_port());
        let mut volumes = HashMap::new();
        volumes.insert(
            data_dir.to_string_lossy().to_string(),
            backend.container_data_path().to_string(),
        );
        let mut user_sec_opts = Vec::new();
        if let Some(profile) = apparmor_profile_opt(&container_name) {
            user_sec_opts.push(profile);
        }
        let cfg = ContainerConfig {
            name: container_name.clone(),
            image: backend.docker_image().to_string(),
            command: None,
            env: HashMap::new(),
            ports,
            volumes,
            network: Some(ai_network_name(instance)),
            restart_policy: Some("unless-stopped".to_string()),
            gpu_support: false,
            auto_remove: false,
            working_dir: None,
            security_opt: build_default_security_opt(user_sec_opts),
            memory_limit: Some(VECTOR_STORE_MEMORY_BYTES),
            nano_cpus: None,
            pids_limit: Some(VECTOR_STORE_PIDS_LIMIT),
            secrets: HashMap::new(),
            // El vector store escribe su base de datos en el volumen.
            read_only_rootfs: false,
            no_new_privileges: true,
            cap_drop: vec!["ALL".to_string()],
            cap_add: vec![
                "CHOWN".to_string(),
                "SETUID".to_string(),
                "SETGID".to_string(),
                "DAC_OVERRIDE".to_string(),
            ],
            image_digest: None,
        };
        self.container_manager.create_container(cfg).await?;
        self.container_manager
            .start_container(&container_name)
            .await?;
        drop(locks); // liberar el lock de puerto tras el arranque
        Ok(crate::ports::rag::VectorStoreInfo {
            backend,
            host_port,
            endpoint: format!("http://127.0.0.1:{host_port}"),
        })
    }

    async fn vector_store_endpoint(
        &self,
        instance: &str,
    ) -> Result<Option<crate::ports::rag::VectorStoreInfo>> {
        validate_ai_instance_name(instance)?;
        let containers = self.container_manager.list_containers(true).await?;
        for backend in [
            crate::domain::rag::VectorStoreBackend::Chroma,
            crate::domain::rag::VectorStoreBackend::Qdrant,
        ] {
            let name = format!("enola-ai-{instance}-{}", backend.container_suffix());
            if let Some(c) = containers.iter().find(|c| c.name == name) {
                let host_port = c.ports.iter().find_map(|s| {
                    // Formato "127.0.0.1:PORT->INTERNAL/tcp" o "PORT:INTERNAL"
                    s.split("->")
                        .next()
                        .and_then(|left| left.rsplit(':').next()?.parse::<u16>().ok())
                });
                let running = c.status.to_lowercase().contains("up")
                    || c.status.to_lowercase().contains("running");
                if let (Some(p), true) = (host_port, running) {
                    return Ok(Some(crate::ports::rag::VectorStoreInfo {
                        backend,
                        host_port: p,
                        endpoint: format!("http://127.0.0.1:{p}"),
                    }));
                }
            }
        }
        Ok(None)
    }

    fn build_vector_store(
        &self,
        info: &crate::ports::rag::VectorStoreInfo,
        ollama_port: u16,
        embedding_model: &str,
    ) -> std::sync::Arc<dyn crate::ports::rag::RagVectorStorePort + Send + Sync> {
        use crate::domain::rag::VectorStoreBackend;
        match info.backend {
            VectorStoreBackend::Chroma => {
                std::sync::Arc::new(crate::adapters::ai::chroma::ChromaAdapter::new(
                    info.host_port,
                    ollama_port,
                    embedding_model,
                ))
            }
            VectorStoreBackend::Qdrant => {
                std::sync::Arc::new(crate::adapters::ai::qdrant::QdrantAdapter::new(
                    info.host_port,
                    ollama_port,
                    embedding_model,
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::infra::docker::BollardDockerAdapter;
    use crate::ports::ai::AiModelLibraryPort;
    use crate::ports::container::MockContainerPort;
    use crate::ports::hardware::{
        BatteryState, GpuInfo, MockHardwareProbePort, SystemHardwareSpecs,
    };
    use crate::ports::manifest::MockManifestPort;
    use std::sync::Mutex;
    use tempfile::TempDir;

    fn lock_env_dir(tmp: &TempDir) -> std::sync::MutexGuard<'static, ()> {
        let guard = crate::infrastructure::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        // SEC-A2: el directorio de locks se valida (sin escritura de grupo/otros);
        // TempDir respeta el umask (002 → 0775).
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(tmp.path(), std::fs::Permissions::from_mode(0o700))
            .expect("chmod lock dir");
        std::env::set_var("ENOLA_PORT_LOCK_DIR", tmp.path());
        guard
    }

    fn mock_manifest() -> Arc<dyn ManifestPort + Send + Sync> {
        let mut m = MockManifestPort::new();
        m.expect_append().returning(|_, _| Ok(())).times(0..);
        Arc::new(m)
    }

    fn mock_hw_with_nvidia(has_nvidia: bool) -> Arc<dyn HardwareProbePort + Send + Sync> {
        let mut hw = MockHardwareProbePort::new();
        hw.expect_probe().returning(move || {
            let gpus = if has_nvidia {
                vec![GpuInfo {
                    index: 0,
                    name: "NVIDIA RTX 4060".into(),
                    brand: GpuBrand::Nvidia,
                    vram_total_mb: 8192,
                    vram_used_mb: 512,
                    vram_free_mb: 7680,
                    temperature_c: Some(40),
                    utilization_gpu: Some(5),
                    driver_version: Some("550.0".into()),
                }]
            } else {
                vec![]
            };
            Ok(SystemHardwareSpecs {
                cpu_cores: 8,
                ram_total_mb: 16384,
                ram_available_mb: 12000,
                ram_used_mb: 4384,
                gpu_brand: if has_nvidia {
                    GpuBrand::Nvidia
                } else {
                    GpuBrand::None
                },
                gpus,
                battery_status: BatteryState::AcConnected,
                platform: "Linux".into(),
            })
        });
        Arc::new(hw)
    }

    /// GGUF real con más de 4 MiB de metadata y `general.file_type` DESPUÉS
    /// del vocabulario: `read_gguf_metadata` debe leer el header completo.
    #[tokio::test]
    async fn read_gguf_metadata_handles_metadata_larger_than_4_mib() {
        fn kv_header(b: &mut Vec<u8>, key: &str, value_type: u32) {
            b.extend_from_slice(&(key.len() as u64).to_le_bytes());
            b.extend_from_slice(key.as_bytes());
            b.extend_from_slice(&value_type.to_le_bytes());
        }
        let mut b = Vec::new();
        b.extend_from_slice(b"GGUF");
        b.extend_from_slice(&3u32.to_le_bytes());
        b.extend_from_slice(&0u64.to_le_bytes()); // tensor_count
        b.extend_from_slice(&3u64.to_le_bytes()); // metadata_kv_count
        kv_header(&mut b, "general.architecture", 8);
        b.extend_from_slice(&5u64.to_le_bytes());
        b.extend_from_slice(b"qwen2");
        kv_header(&mut b, "tokenizer.ggml.tokens", 9);
        b.extend_from_slice(&8u32.to_le_bytes()); // array de strings
        let token = b"token-de-relleno";
        let count = (5 * 1024 * 1024) / (8 + token.len());
        b.extend_from_slice(&(count as u64).to_le_bytes());
        for _ in 0..count {
            b.extend_from_slice(&(token.len() as u64).to_le_bytes());
            b.extend_from_slice(token);
        }
        kv_header(&mut b, "general.file_type", 4);
        b.extend_from_slice(&15u32.to_le_bytes());
        assert!(b.len() > 5 * 1024 * 1024);

        let tmp = TempDir::new().unwrap();
        let lib = crate::domain::ai_model::library_dir_for_instance(tmp.path(), "dev").unwrap();
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::write(lib.join("big-vocab.gguf"), &b).unwrap();
        let adapter = EnolaAiAdapter::new_with_base(
            Arc::new(MockContainerPort::new()),
            mock_hw_with_nvidia(false),
            mock_manifest(),
            tmp.path().to_path_buf(),
        );

        let meta = adapter
            .read_gguf_metadata("dev", "big-vocab.gguf")
            .await
            .unwrap();
        assert_eq!(meta.architecture.as_deref(), Some("qwen2"));
        assert_eq!(meta.file_type, Some(15));
    }

    #[tokio::test]
    async fn create_instance_creates_ollama_and_webui_with_r20_limits_and_localhost_ports() {
        let tmp = TempDir::new().unwrap();
        let _env_guard = lock_env_dir(&tmp);

        let captured_configs: Arc<Mutex<Vec<ContainerConfig>>> = Arc::new(Mutex::new(Vec::new()));
        let captured_clone = captured_configs.clone();

        let mut docker = MockContainerPort::new();
        docker.expect_list_containers().returning(|_| Ok(vec![]));
        docker
            .expect_create_network()
            .withf(|net| net == "enola_net_ai_dev")
            .times(1)
            .returning(|_| Ok(()));
        docker
            .expect_create_container()
            .times(2)
            .returning(move |cfg| {
                let name = cfg.name.clone();
                captured_clone.lock().unwrap().push(cfg);
                Ok(format!("id-{name}"))
            });
        docker
            .expect_start_container()
            .times(2)
            .returning(|_| Ok(()));

        let adapter = EnolaAiAdapter::new_with_base(
            Arc::new(docker),
            mock_hw_with_nvidia(true),
            mock_manifest(),
            tmp.path().join("ai_data"),
        );

        let mut stack_cfg = AiStackConfig::new("dev", "llama3.1:8b", 11435, 8081, true).unwrap();
        stack_cfg.ollama_image_digest = Some("sha256:111122223333".into());

        let inst = adapter.create_instance(stack_cfg).await.unwrap();
        // SEC-A7: el directorio base de datos queda 0700.
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(tmp.path().join("ai_data"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o700, "ai base must be 0700, got {:o}", mode);
        }
        assert_eq!(inst.name, "dev");
        assert_eq!(inst.api_port, Some(11435));
        assert_eq!(inst.webui_port, Some(8081));
        assert_eq!(inst.status, AiInstanceStatus::Running);
        assert!(inst.gpu_enabled);

        let configs = captured_configs.lock().unwrap();
        assert_eq!(configs.len(), 2);

        let ollama_cfg = configs
            .iter()
            .find(|c| c.name == "enola-ai-dev-ollama")
            .expect("ollama container created");
        assert_eq!(ollama_cfg.ports.get(&11435), Some(&11434));
        assert!(
            ollama_cfg.gpu_support,
            "NVIDIA GPU detected -> gpu_support=true"
        );
        assert!(
            ollama_cfg.memory_limit.unwrap_or(0) >= MIN_OLLAMA_MEMORY_BYTES,
            "R20: memory_limit must be >= 8 GiB, never 2 GiB default"
        );
        assert!(
            ollama_cfg.pids_limit.unwrap_or(0) >= MIN_AI_PIDS_LIMIT,
            "R20: pids_limit must be >= 4096"
        );
        assert!(ollama_cfg.no_new_privileges);
        assert_eq!(ollama_cfg.cap_drop, vec!["ALL".to_string()]);
        assert!(ollama_cfg
            .security_opt
            .iter()
            .any(|o| o == "no-new-privileges:true"));
        assert_eq!(
            ollama_cfg.image_digest.as_deref(),
            Some("sha256:111122223333")
        );

        // Regla inviolable skill docker: TODOS los port bindings en 127.0.0.1
        let (_, ollama_bindings) =
            BollardDockerAdapter::build_localhost_port_maps(&ollama_cfg.ports);
        for bindings in ollama_bindings.values().flatten() {
            for b in bindings {
                assert_eq!(b.host_ip.as_deref(), Some("127.0.0.1"));
            }
        }

        let webui_cfg = configs
            .iter()
            .find(|c| c.name == "enola-ai-dev-webui")
            .expect("webui container created");
        assert_eq!(webui_cfg.ports.get(&8081), Some(&8080));
        assert!(webui_cfg.memory_limit.unwrap_or(0) > 2 * 1024 * 1024 * 1024);
        assert!(webui_cfg.pids_limit.unwrap_or(0) >= MIN_AI_PIDS_LIMIT);
        assert!(webui_cfg.no_new_privileges);
        assert_eq!(webui_cfg.cap_drop, vec!["ALL".to_string()]);
        assert_eq!(
            webui_cfg.env.get("OLLAMA_BASE_URL").map(String::as_str),
            Some("http://enola-ai-dev-ollama:11434")
        );

        let (_, webui_bindings) = BollardDockerAdapter::build_localhost_port_maps(&webui_cfg.ports);
        for bindings in webui_bindings.values().flatten() {
            for b in bindings {
                assert_eq!(b.host_ip.as_deref(), Some("127.0.0.1"));
            }
        }
    }

    #[tokio::test]
    async fn create_instance_degrades_to_cpu_when_no_nvidia_gpu_available() {
        let tmp = TempDir::new().unwrap();
        let _env_guard = lock_env_dir(&tmp);

        let captured_configs: Arc<Mutex<Vec<ContainerConfig>>> = Arc::new(Mutex::new(Vec::new()));
        let captured_clone = captured_configs.clone();

        let mut docker = MockContainerPort::new();
        docker.expect_list_containers().returning(|_| Ok(vec![]));
        docker.expect_create_network().returning(|_| Ok(()));
        docker.expect_create_container().returning(move |cfg| {
            let name = cfg.name.clone();
            captured_clone.lock().unwrap().push(cfg);
            Ok(format!("id-{name}"))
        });
        docker.expect_start_container().returning(|_| Ok(()));

        let adapter = EnolaAiAdapter::new_with_base(
            Arc::new(docker),
            mock_hw_with_nvidia(false),
            mock_manifest(),
            tmp.path().join("ai_data"),
        );

        let stack_cfg = AiStackConfig::new("cpuinst", "llama3.1:8b", 11440, 8085, true).unwrap();
        let inst = adapter.create_instance(stack_cfg).await.unwrap();
        assert!(
            !inst.gpu_enabled,
            "should degrade to CPU when no GPU is found"
        );

        let configs = captured_configs.lock().unwrap();
        let ollama_cfg = configs
            .iter()
            .find(|c| c.name == "enola-ai-cpuinst-ollama")
            .unwrap();
        assert!(!ollama_cfg.gpu_support);
    }

    #[tokio::test]
    async fn create_instance_rolls_back_ollama_and_network_when_webui_fails() {
        let tmp = TempDir::new().unwrap();
        let _env_guard = lock_env_dir(&tmp);

        let mut docker = MockContainerPort::new();
        docker.expect_list_containers().returning(|_| Ok(vec![]));
        docker
            .expect_create_network()
            .withf(|net| net == "enola_net_ai_rollback")
            .times(1)
            .returning(|_| Ok(()));

        // 1er create_container (ollama) tiene éxito; 2º (webui) falla
        let mut call_count = 0usize;
        docker
            .expect_create_container()
            .times(2)
            .returning(move |cfg| {
                call_count += 1;
                if call_count == 1 {
                    assert_eq!(cfg.name, "enola-ai-rollback-ollama");
                    Ok("id-ollama".into())
                } else {
                    Err(EnolaError::InfrastructureError(
                        "simulated webui creation failure".into(),
                    ))
                }
            });
        docker
            .expect_start_container()
            .withf(|id| id == "enola-ai-rollback-ollama" || id == "id-ollama")
            .times(1)
            .returning(|_| Ok(()));

        // Rollback esperado: stop + remove de ollama + remove_network
        docker
            .expect_stop_container()
            .withf(|name| name == "enola-ai-rollback-ollama")
            .times(1)
            .returning(|_| Ok(()));
        docker
            .expect_remove_container()
            .withf(|name| name == "enola-ai-rollback-ollama")
            .times(1)
            .returning(|_| Ok(()));
        docker
            .expect_remove_network()
            .withf(|net| net == "enola_net_ai_rollback")
            .times(1)
            .returning(|_| Ok(()));

        let base_dir = tmp.path().join("ai_data");
        let adapter = EnolaAiAdapter::new_with_base(
            Arc::new(docker),
            mock_hw_with_nvidia(false),
            mock_manifest(),
            base_dir.clone(),
        );

        let stack_cfg = AiStackConfig::new("rollback", "llama3.1:8b", 11445, 8086, false).unwrap();
        let res = adapter.create_instance(stack_cfg).await;
        assert!(res.is_err());
        assert!(
            !base_dir.join("rollback").exists(),
            "instance directory must be cleaned up on rollback"
        );
    }

    #[tokio::test]
    async fn create_instance_rejects_port_already_locked_by_flock() {
        let tmp = TempDir::new().unwrap();
        let _env_guard = lock_env_dir(&tmp);

        // Pre-bloqueamos el puerto 11450 con acquire_port_lock
        let _held_lock = acquire_port_lock(11450).expect("hold port lock");

        let mut docker = MockContainerPort::new();
        // No debe invocar create_container si el lock de puerto falla
        docker.expect_create_container().times(0);

        let adapter = EnolaAiAdapter::new_with_base(
            Arc::new(docker),
            mock_hw_with_nvidia(false),
            mock_manifest(),
            tmp.path().join("ai_data"),
        );

        let stack_cfg = AiStackConfig::new("locked", "llama3.1:8b", 11450, 8087, false).unwrap();
        let err = adapter.create_instance(stack_cfg).await.unwrap_err();
        assert!(err.to_string().contains("11450"));
    }

    #[tokio::test]
    async fn lifecycle_list_status_start_stop_delete_and_logs_work_via_container_port() {
        let tmp = TempDir::new().unwrap();
        let _env_guard = lock_env_dir(&tmp);

        let mut docker = MockContainerPort::new();
        docker.expect_list_containers().returning(|_| {
            Ok(vec![
                ContainerInfo {
                    id: "c1".into(),
                    name: "enola-ai-dev-ollama".into(),
                    image: "ollama/ollama:latest".into(),
                    status: "Up 5 minutes".into(),
                    ports: vec!["127.0.0.1:11435->11434/tcp".into()],
                },
                ContainerInfo {
                    id: "c2".into(),
                    name: "enola-ai-dev-webui".into(),
                    image: "ghcr.io/open-webui/open-webui:main".into(),
                    status: "Up 5 minutes".into(),
                    ports: vec!["127.0.0.1:8081->8080/tcp".into()],
                },
            ])
        });
        docker
            .expect_start_container()
            .times(2)
            .returning(|_| Ok(()));
        docker
            .expect_stop_container()
            .times(4)
            .returning(|_| Ok(()));
        docker
            .expect_remove_container()
            .times(2)
            .returning(|_| Ok(()));
        docker
            .expect_remove_network()
            .withf(|n| n == "enola_net_ai_dev")
            .times(1)
            .returning(|_| Ok(()));
        docker
            .expect_get_logs()
            .times(2)
            .returning(|name, _| Ok(format!("log line from {name}")));

        let adapter = EnolaAiAdapter::new_with_base(
            Arc::new(docker),
            mock_hw_with_nvidia(false),
            mock_manifest(),
            tmp.path().join("ai_data"),
        );

        let list = adapter.list_instances().await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "dev");
        assert_eq!(list[0].api_port, Some(11435));
        assert_eq!(list[0].webui_port, Some(8081));
        assert_eq!(list[0].status, AiInstanceStatus::Running);

        let status = adapter.get_instance("dev").await.unwrap();
        assert_eq!(status.status, AiInstanceStatus::Running);

        adapter.start_instance("dev").await.unwrap();
        adapter.stop_instance("dev").await.unwrap();

        let logs = adapter.get_instance_logs("dev", 50).await.unwrap();
        assert!(logs.contains("enola-ai-dev-ollama"));
        assert!(logs.contains("enola-ai-dev-webui"));

        // delete sin --force debe fallar
        assert!(adapter.delete_instance("dev", false).await.is_err());
        // delete con --force elimina ambos contenedores y la red
        adapter.delete_instance("dev", true).await.unwrap();
    }

    #[tokio::test]
    async fn delete_instance_does_not_touch_instances_sharing_name_prefix() {
        let tmp = TempDir::new().unwrap();
        let _env_guard = lock_env_dir(&tmp);

        let container = |name: &str| ContainerInfo {
            id: name.into(),
            name: name.into(),
            image: "img".into(),
            status: "Up 5 minutes".into(),
            ports: vec![],
        };
        let mut docker = MockContainerPort::new();
        docker.expect_list_containers().returning(move |_| {
            Ok(vec![
                container("enola-ai-dev-ollama"),
                container("enola-ai-dev-webui"),
                container("enola-ai-dev-chromadb"),
                container("enola-ai-dev-finetune-ft123"),
                container("enola-ai-dev-test-ollama"),
                container("enola-ai-dev-test-webui"),
                container("enola-ai-dev-test-qdrant"),
            ])
        });
        let removed = Arc::new(Mutex::new(Vec::<String>::new()));
        let removed_c = removed.clone();
        docker
            .expect_stop_container()
            .withf(|n| !n.starts_with("enola-ai-dev-test-"))
            .returning(|_| Ok(()));
        docker
            .expect_remove_container()
            .withf(|n| !n.starts_with("enola-ai-dev-test-"))
            .returning(move |n| {
                removed_c.lock().unwrap().push(n.to_string());
                Ok(())
            });
        docker
            .expect_remove_network()
            .withf(|n| n == "enola_net_ai_dev")
            .times(1)
            .returning(|_| Ok(()));

        let adapter = EnolaAiAdapter::new_with_base(
            Arc::new(docker),
            mock_hw_with_nvidia(false),
            mock_manifest(),
            tmp.path().join("ai_data"),
        );

        adapter.delete_instance("dev", true).await.unwrap();

        let mut removed = removed.lock().unwrap().clone();
        removed.sort();
        assert_eq!(
            removed,
            vec![
                "enola-ai-dev-chromadb",
                "enola-ai-dev-finetune-ft123",
                "enola-ai-dev-ollama",
                "enola-ai-dev-webui",
            ]
        );
    }

    #[tokio::test]
    async fn import_library_file_preserves_full_file_bytes() {
        let tmp = TempDir::new().unwrap();
        let _env_guard = lock_env_dir(&tmp);

        // GGUF mínimo válido: magic + version(3) + tensor_count(0) + kv(0) + payload.
        let mut src_bytes = Vec::new();
        src_bytes.extend_from_slice(b"GGUF");
        src_bytes.extend_from_slice(&3u32.to_le_bytes());
        src_bytes.extend_from_slice(&0u64.to_le_bytes());
        src_bytes.extend_from_slice(&0u64.to_le_bytes());
        src_bytes.extend_from_slice(b"PAYLOAD-AFTER-HEADER");

        let src = tmp.path().join("model.gguf");
        std::fs::write(&src, &src_bytes).unwrap();

        let adapter = EnolaAiAdapter::new_with_base(
            Arc::new(MockContainerPort::new()),
            Arc::new(MockHardwareProbePort::new()),
            mock_manifest(),
            tmp.path().join("ai_data"),
        );

        let model = adapter.import_library_file("dev", &src).await.unwrap();
        assert_eq!(model.filename, "model.gguf");

        let dest = crate::domain::ai_model::resolve_library_path(
            &tmp.path().join("ai_data"),
            "dev",
            "model.gguf",
        )
        .unwrap();
        let dest_bytes = std::fs::read(&dest).unwrap();
        assert_eq!(
            dest_bytes, src_bytes,
            "el fichero importado debe ser byte-idéntico al origen (header incluido)"
        );
    }
}
