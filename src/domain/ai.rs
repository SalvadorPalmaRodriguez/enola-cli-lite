// Fase 10.1 — Dominio puro para instancias de IA local (Ollama + Open WebUI).
//
// Regla de capa (`domain/`): SIN dependencias externas de I/O (`tokio`, `reqwest`,
// `std::net::TcpListener`, etc.). Toda validación y cálculo de puertos/límites es
// determinista y puro.

use crate::domain::error::{EnolaError, Result};
use crate::domain::naming::validate_service_name;
use crate::domain::rag::VectorStoreBackend;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fmt;

/// Imagen oficial por defecto para el motor Ollama.
/// Tag de versión fijo (no `latest`) para reproducibilidad: `latest` es
/// mutable y ya cambió el soporte de LoRA adapters que condiciona Fase 14.5.
pub const DEFAULT_OLLAMA_IMAGE: &str = "ollama/ollama:0.40.6";

/// Imagen oficial por defecto para la interfaz Open WebUI.
/// Tag de versión fijo (`vX.Y.Z` no cambia; `main` es rolling y muta).
pub const DEFAULT_WEBUI_IMAGE: &str = "ghcr.io/open-webui/open-webui:v0.12.3";

/// Modelo por defecto al crear una instancia si el usuario no especifica otro.
pub const DEFAULT_AI_MODEL: &str = "llama3.1:8b";

/// Puerto interno de Ollama dentro del contenedor.
pub const OLLAMA_INTERNAL_PORT: u16 = 11434;

/// Puerto interno de Open WebUI dentro del contenedor.
pub const WEBUI_INTERNAL_PORT: u16 = 8080;

/// Rango de puertos host para la API HTTP de Ollama.
pub const AI_API_PORT_RANGE: (u16, u16) = (11435, 12000);

/// Rango de puertos host para la interfaz web de Open WebUI.
pub const AI_WEBUI_PORT_RANGE: (u16, u16) = (8081, 8200);

/// Límite mínimo de RAM para el contenedor Ollama (8 GiB — R20).
/// El default general de 2 GiB de `ContainerConfig` mata Ollama por OOM (exit 137)
/// al cargar cualquier modelo >= 3B.
pub const MIN_OLLAMA_MEMORY_BYTES: i64 = 8 * 1024 * 1024 * 1024;

/// Límite de RAM por defecto para el contenedor Open WebUI (4 GiB — R20).
pub const DEFAULT_WEBUI_MEMORY_BYTES: i64 = 4 * 1024 * 1024 * 1024;

/// Límite mínimo de PIDs para contenedores de IA (4096 — R20: hilos BLAS/CUDA).
pub const MIN_AI_PIDS_LIMIT: i64 = 4096;

/// Cuota de CPU por defecto para contenedores de IA (4 CPUs en nanoCPUs).
pub const DEFAULT_AI_NANO_CPUS: i64 = 4_000_000_000;

/// Directorio base en el host para metadatos y volúmenes de instancias IA.
pub const DEFAULT_AI_BASE_DIR: &str = "/srv/enola-ai";

/// Estado agregado de una instancia de IA (Ollama + Open WebUI).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AiInstanceStatus {
    /// Ambos contenedores (`ollama` y `webui`) están en ejecución.
    Running,
    /// Uno de los dos contenedores está activo y el otro detenido/ausente.
    Degraded,
    /// Los contenedores existen pero están detenidos.
    Stopped,
    /// La instancia no existe.
    NotFound,
}

impl AiInstanceStatus {
    /// Representación canónica en minúsculas.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Degraded => "degraded",
            Self::Stopped => "stopped",
            Self::NotFound => "not_found",
        }
    }

    /// Calcula el estado agregado a partir del estado de los contenedores Ollama y WebUI.
    pub fn from_container_states(
        ollama_running: Option<bool>,
        webui_running: Option<bool>,
    ) -> Self {
        match (ollama_running, webui_running) {
            (Some(true), Some(true)) => Self::Running,
            (Some(true), _) | (_, Some(true)) => Self::Degraded,
            (Some(false), _) | (_, Some(false)) => Self::Stopped,
            (None, None) => Self::NotFound,
        }
    }
}

impl fmt::Display for AiInstanceStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Configuración validada para desplegar un stack de IA local (Ollama + Open WebUI).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiStackConfig {
    /// Nombre lógico de la instancia (validado con `validate_service_name`).
    pub name: String,
    /// Modelo inicial de Ollama (ej. `llama3.1:8b`).
    pub model: String,
    /// Puerto en `127.0.0.1` mapeado al puerto 11434 del contenedor Ollama.
    pub api_port: u16,
    /// Puerto en `127.0.0.1` mapeado al puerto 8080 del contenedor Open WebUI.
    pub webui_port: u16,
    /// Si se solicita passthrough de GPU NVIDIA (`DeviceRequest`).
    pub gpu_enabled: bool,
    /// Límite de memoria RAM en bytes para el contenedor Ollama (mínimo 8 GiB, R20).
    pub memory_limit_bytes: i64,
    /// Límite de memoria RAM en bytes para el contenedor Open WebUI.
    pub webui_memory_limit_bytes: i64,
    /// Límite de procesos/hilos para los contenedores de IA (mínimo 4096, R20).
    pub pids_limit: i64,
    /// Cuota de CPU en nanoCPUs.
    pub nano_cpus: i64,
    /// Imagen Docker de Ollama.
    pub ollama_image: String,
    /// Imagen Docker de Open WebUI.
    pub webui_image: String,
    /// Digest SHA-256 opcional para fijar la imagen de Ollama (`SEC-013`).
    pub ollama_image_digest: Option<String>,
    /// Digest SHA-256 opcional para fijar la imagen de Open WebUI (`SEC-013`).
    pub webui_image_digest: Option<String>,
    /// Si debe descargarse el modelo (`/api/pull`) tras iniciar el contenedor.
    pub pull_model_on_create: bool,
}

impl AiStackConfig {
    /// Construye y valida una configuración de stack IA con límites seguros para R20.
    pub fn new(
        name: &str,
        model: &str,
        api_port: u16,
        webui_port: u16,
        gpu_enabled: bool,
    ) -> Result<Self> {
        validate_ai_instance_name(name)?;

        let trimmed_model = model.trim();
        if trimmed_model.is_empty() {
            return Err(EnolaError::ValidationError(
                "El nombre del modelo de IA no puede estar vacío".into(),
            ));
        }
        if trimmed_model.contains('\0')
            || trimmed_model.contains('\n')
            || trimmed_model.contains('\r')
        {
            return Err(EnolaError::ValidationError(
                "El nombre del modelo contiene caracteres de control inválidos".into(),
            ));
        }

        if api_port == 0 || webui_port == 0 {
            return Err(EnolaError::ValidationError(
                "Los puertos API y WebUI deben ser mayores que 0".into(),
            ));
        }
        if api_port == webui_port {
            return Err(EnolaError::ValidationError(format!(
                "El puerto API ({api_port}) y el puerto WebUI ({webui_port}) no pueden ser iguales"
            )));
        }

        Ok(Self {
            name: name.to_string(),
            model: trimmed_model.to_string(),
            api_port,
            webui_port,
            gpu_enabled,
            memory_limit_bytes: MIN_OLLAMA_MEMORY_BYTES,
            webui_memory_limit_bytes: DEFAULT_WEBUI_MEMORY_BYTES,
            pids_limit: MIN_AI_PIDS_LIMIT,
            nano_cpus: DEFAULT_AI_NANO_CPUS,
            ollama_image: DEFAULT_OLLAMA_IMAGE.to_string(),
            webui_image: DEFAULT_WEBUI_IMAGE.to_string(),
            ollama_image_digest: None,
            webui_image_digest: None,
            pull_model_on_create: true,
        })
    }

    /// Ajusta el límite de memoria de Ollama (en bytes), rechazando valores < 8 GiB (R20).
    pub fn with_memory_limit_bytes(mut self, bytes: i64) -> Result<Self> {
        if bytes < MIN_OLLAMA_MEMORY_BYTES {
            return Err(EnolaError::ValidationError(format!(
                "El límite de memoria de Ollama ({bytes} bytes) debe ser al menos {} bytes (8 GiB, R20)",
                MIN_OLLAMA_MEMORY_BYTES
            )));
        }
        self.memory_limit_bytes = bytes;
        Ok(self)
    }
}

/// Información de estado de una instancia de IA desplegada.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiInstance {
    /// Nombre lógico de la instancia.
    pub name: String,
    /// Modelo configurado en la instancia.
    pub model: String,
    /// Puerto local (`127.0.0.1`) de la API de Ollama.
    pub api_port: Option<u16>,
    /// Puerto local (`127.0.0.1`) de Open WebUI.
    pub webui_port: Option<u16>,
    /// Estado agregado de la instancia.
    pub status: AiInstanceStatus,
    /// Estado reportado del contenedor Ollama.
    pub ollama_status: String,
    /// Estado reportado del contenedor Open WebUI.
    pub webui_status: String,
    /// Si la instancia tiene soporte GPU activo.
    pub gpu_enabled: bool,
    /// Dirección `.onion` si Open WebUI está publicado en Tor.
    pub onion_address: Option<String>,
}

/// Alias de compatibilidad con la nomenclatura de standalone.
pub type AiInstanceInfo = AiInstance;

/// Valida el nombre de una instancia IA aplicando las reglas canónicas de `naming`.
pub fn validate_ai_instance_name(name: &str) -> Result<()> {
    validate_service_name(name)
}

/// Nombre canónico del contenedor Ollama: `enola-ai-<name>-ollama`.
pub fn ollama_container_name(name: &str) -> String {
    format!("enola-ai-{name}-ollama")
}

/// Nombre canónico del contenedor Open WebUI: `enola-ai-<name>-webui`.
pub fn webui_container_name(name: &str) -> String {
    format!("enola-ai-{name}-webui")
}

/// Indica si `container` pertenece a la instancia IA `instance`.
///
/// Solo acepta los sufijos exactos que crea `enola-cli` tras `enola-ai-<instance>-`:
/// `ollama`, `webui`, el del vector store (`chromadb`/`qdrant`) y `finetune-ft<dígitos>`.
/// Un prefijo no basta: `validate_service_name` admite `-`, así que
/// `enola-ai-dev-test-ollama` es de `dev-test`, no de `dev` (H-02).
pub fn is_instance_container(instance: &str, container: &str) -> bool {
    let Some(suffix) = container
        .strip_prefix("enola-ai-")
        .and_then(|rest| rest.strip_prefix(instance))
        .and_then(|rest| rest.strip_prefix('-'))
    else {
        return false;
    };
    match suffix {
        "ollama" | "webui" => true,
        s if s == VectorStoreBackend::Chroma.container_suffix()
            || s == VectorStoreBackend::Qdrant.container_suffix() =>
        {
            true
        }
        s => s
            .strip_prefix("finetune-ft")
            .is_some_and(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit())),
    }
}

/// Nombre canónico de la red Docker aislada para la instancia IA: `enola_net_ai_<name>`.
pub fn ai_network_name(name: &str) -> String {
    format!("enola_net_ai_{name}")
}

/// Nombre canónico del servicio Tor para publicar Open WebUI: `ai-<name>`.
pub fn ai_tor_service_name(name: &str) -> String {
    format!("ai-{name}")
}

/// Selecciona un par de puertos `(api_port, webui_port)` libres para una instancia IA.
pub fn select_ai_ports<F>(
    existing: &[AiInstance],
    requested_api: Option<u16>,
    requested_webui: Option<u16>,
    is_port_free: F,
) -> Result<(u16, u16)>
where
    F: Fn(u16) -> bool,
{
    let mut used = HashSet::new();
    for inst in existing {
        if let Some(p) = inst.api_port {
            used.insert(p);
        }
        if let Some(p) = inst.webui_port {
            used.insert(p);
        }
    }

    let api_port = match requested_api {
        Some(p) => {
            if p == 0 || used.contains(&p) || !is_port_free(p) {
                return Err(EnolaError::ValidationError(format!(
                    "Puerto API {p} no está disponible"
                )));
            }
            p
        }
        None => (AI_API_PORT_RANGE.0..=AI_API_PORT_RANGE.1)
            .find(|&p| !used.contains(&p) && is_port_free(p))
            .ok_or_else(|| {
                EnolaError::InfrastructureError(format!(
                    "No hay puertos API disponibles en el rango {}-{}",
                    AI_API_PORT_RANGE.0, AI_API_PORT_RANGE.1
                ))
            })?,
    };
    used.insert(api_port);

    let webui_port = match requested_webui {
        Some(p) => {
            if p == 0 || used.contains(&p) || !is_port_free(p) {
                return Err(EnolaError::ValidationError(format!(
                    "Puerto WebUI {p} no está disponible"
                )));
            }
            p
        }
        None => (AI_WEBUI_PORT_RANGE.0..=AI_WEBUI_PORT_RANGE.1)
            .find(|&p| !used.contains(&p) && is_port_free(p))
            .ok_or_else(|| {
                EnolaError::InfrastructureError(format!(
                    "No hay puertos WebUI disponibles en el rango {}-{}",
                    AI_WEBUI_PORT_RANGE.0, AI_WEBUI_PORT_RANGE.1
                ))
            })?,
    };

    Ok((api_port, webui_port))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn naming_helpers_follow_enola_ai_conventions() {
        assert_eq!(ollama_container_name("dev"), "enola-ai-dev-ollama");
        assert_eq!(webui_container_name("dev"), "enola-ai-dev-webui");
        assert_eq!(ai_network_name("dev"), "enola_net_ai_dev");
        assert_eq!(ai_tor_service_name("dev"), "ai-dev");
    }

    #[test]
    fn is_instance_container_matches_only_own_exact_suffixes() {
        // Contenedores propios de `dev`.
        assert!(is_instance_container("dev", "enola-ai-dev-ollama"));
        assert!(is_instance_container("dev", "enola-ai-dev-webui"));
        assert!(is_instance_container("dev", "enola-ai-dev-chromadb"));
        assert!(is_instance_container("dev", "enola-ai-dev-qdrant"));
        assert!(is_instance_container(
            "dev",
            "enola-ai-dev-finetune-ft1729000000000"
        ));

        // Instancias cuyo nombre empieza por `dev-` NO son de `dev` (H-02).
        assert!(!is_instance_container("dev", "enola-ai-dev-test-ollama"));
        assert!(!is_instance_container("dev", "enola-ai-dev-test-webui"));
        assert!(!is_instance_container("dev", "enola-ai-dev-test-chromadb"));
        assert!(!is_instance_container(
            "dev",
            "enola-ai-dev-test-finetune-ft1"
        ));
        assert!(!is_instance_container(
            "dev",
            "enola-ai-dev-finetune-ft1-ollama"
        ));

        // Sufijos o ids de job no canónicos.
        assert!(!is_instance_container("dev", "enola-ai-dev-finetune-"));
        assert!(!is_instance_container("dev", "enola-ai-dev-finetune-ft"));
        assert!(!is_instance_container("dev", "enola-ai-dev-finetune-ftabc"));
        assert!(!is_instance_container("dev", "enola-ai-dev-other"));
        assert!(!is_instance_container("dev", "enola-ai-dev"));
        assert!(!is_instance_container("dev", "enola-ai-devx-ollama"));
        assert!(!is_instance_container("dev", "piloto-ollama"));
    }

    #[test]
    fn stack_config_validates_service_name_and_enforces_r20_limits() {
        // Nombres inválidos rechazados por validate_service_name
        assert!(AiStackConfig::new("", "llama3.1:8b", 11435, 8081, false).is_err());
        assert!(AiStackConfig::new("Bad_Name", "llama3.1:8b", 11435, 8081, false).is_err());
        assert!(AiStackConfig::new("-leading", "llama3.1:8b", 11435, 8081, false).is_err());
        assert!(AiStackConfig::new("../escape", "llama3.1:8b", 11435, 8081, false).is_err());

        // Puertos colisionados o en 0 rechazados
        assert!(AiStackConfig::new("dev", "llama3.1:8b", 8081, 8081, false).is_err());
        assert!(AiStackConfig::new("dev", "llama3.1:8b", 0, 8081, false).is_err());
        assert!(AiStackConfig::new("dev", "llama3.1:8b", 11435, 0, false).is_err());

        // Modelo vacío rechazado
        assert!(AiStackConfig::new("dev", "   ", 11435, 8081, false).is_err());

        // Configuración válida incluye límites R20 (>= 8 GiB RAM, >= 4096 PIDs)
        let cfg = AiStackConfig::new("dev", "llama3.1:8b", 11435, 8081, true).unwrap();
        assert_eq!(cfg.name, "dev");
        assert_eq!(cfg.model, "llama3.1:8b");
        assert_eq!(cfg.api_port, 11435);
        assert_eq!(cfg.webui_port, 8081);
        assert!(cfg.gpu_enabled);
        assert!(cfg.memory_limit_bytes >= MIN_OLLAMA_MEMORY_BYTES);
        assert!(cfg.memory_limit_bytes > 2 * 1024 * 1024 * 1024);
        assert!(cfg.pids_limit >= MIN_AI_PIDS_LIMIT);
    }

    #[test]
    fn stack_config_rejects_memory_limit_below_minimum() {
        let cfg = AiStackConfig::new("dev", "llama3.1:8b", 11435, 8081, false).unwrap();
        // 2 GiB debe rechazarse (causaría OOM exit 137 en Ollama — R20)
        assert!(cfg
            .clone()
            .with_memory_limit_bytes(2 * 1024 * 1024 * 1024)
            .is_err());
        // 12 GiB se acepta
        let updated = cfg
            .with_memory_limit_bytes(12 * 1024 * 1024 * 1024)
            .unwrap();
        assert_eq!(updated.memory_limit_bytes, 12 * 1024 * 1024 * 1024);
    }

    #[test]
    fn instance_status_aggregates_ollama_and_webui_states() {
        assert_eq!(
            AiInstanceStatus::from_container_states(Some(true), Some(true)),
            AiInstanceStatus::Running
        );
        assert_eq!(
            AiInstanceStatus::from_container_states(Some(true), Some(false)),
            AiInstanceStatus::Degraded
        );
        assert_eq!(
            AiInstanceStatus::from_container_states(Some(false), Some(true)),
            AiInstanceStatus::Degraded
        );
        assert_eq!(
            AiInstanceStatus::from_container_states(Some(true), None),
            AiInstanceStatus::Degraded
        );
        assert_eq!(
            AiInstanceStatus::from_container_states(Some(false), Some(false)),
            AiInstanceStatus::Stopped
        );
        assert_eq!(
            AiInstanceStatus::from_container_states(None, None),
            AiInstanceStatus::NotFound
        );
    }

    #[test]
    fn select_ai_ports_skips_used_and_busy_ports() {
        let existing = vec![AiInstance {
            name: "first".into(),
            model: "llama3.1:8b".into(),
            api_port: Some(11435),
            webui_port: Some(8081),
            status: AiInstanceStatus::Running,
            ollama_status: "running".into(),
            webui_status: "running".into(),
            gpu_enabled: false,
            onion_address: None,
        }];

        // Simulamos que 11436 y 8082 también están ocupados en el SO
        let (api, webui) = select_ai_ports(&existing, None, None, |p| p != 11436 && p != 8082)
            .expect("should allocate next free ports");
        assert_eq!(api, 11437);
        assert_eq!(webui, 8083);
    }

    #[test]
    fn select_ai_ports_honors_explicit_ports_and_rejects_conflicts() {
        let existing = vec![];
        let (api, webui) = select_ai_ports(&existing, Some(11500), Some(8090), |_| true).unwrap();
        assert_eq!(api, 11500);
        assert_eq!(webui, 8090);

        // Colisión entre api y webui
        assert!(select_ai_ports(&existing, Some(8090), Some(8090), |_| true).is_err());

        // Puerto ocupado según el predicado
        assert!(select_ai_ports(&existing, Some(11500), Some(8090), |p| p != 11500).is_err());
    }
}
