// Fase 11 — Casos de uso de `ai model` (biblioteca GGUF + registro Ollama).
//
// Hexagonal: solo traits de `crate::ports::*` y tipos de `crate::domain::*`.
//
// Dos vías de instalación de modelos conviven por instancia:
// - `ai model download|import|add` (esta capa): fichero `.gguf` en la
//   biblioteca `<base>/<instance>/ollama/models-gguf/` + registro vía
//   `/api/create` (Modelfile FROM), o `/api/pull` para tags del registro.
//   Proveniencia completa: fichero auditable + SHA-256 registrado.
// - Pull desde el panel de Open WebUI (Admin → Models): NO atraviesa esta
//   capa. El modelo queda solo en el store interno de Ollama y `list()` lo
//   muestra bajo `registered`, nunca en `library` (sin SHA-256 ni origen).

use crate::domain::ai_model::{self, AiModel};
use crate::domain::error::{EnolaError, Result};
use crate::ports::ai::{
    AiAdapterPort, AiModelLibraryPort, AiModelRegistryPort, ModelDownloadPort,
    ModelDownloadRequest, TokenizerDownloadPort, TokenizerDownloadRequest,
};
use crate::ports::llm::LlmModelInfo;
use serde::Serialize;
use std::path::Path;
use std::sync::Arc;

/// Modelos de una instancia: biblioteca de ficheros + registro Ollama.
#[derive(Debug, Clone, Serialize)]
pub struct InstanceModelReport {
    /// Nombre de la instancia.
    pub instance: String,
    /// Ficheros `.gguf` en la biblioteca de la instancia.
    pub library: Vec<AiModel>,
    /// Modelos registrados en Ollama (`/api/tags`).
    pub registered: Vec<LlmModelInfo>,
    /// Nota operativa (p.ej. instancia parada → registro no consultado).
    pub note: Option<String>,
}

/// Resultado de `ai model download`/`import`.
#[derive(Debug, Clone, Serialize)]
pub struct ModelInstallReport {
    /// Instancia destino.
    pub instance: String,
    /// Fichero `.gguf` en la biblioteca.
    pub filename: String,
    /// Bytes escritos.
    pub bytes_written: u64,
    /// SHA-256 real del fichero.
    pub sha256: String,
    /// `true` si se reanudó una descarga parcial.
    pub resumed: bool,
    /// `true` si la descarga salió por Tor SOCKS5h.
    pub via_tor: bool,
    /// Nombre con el que quedó registrado en Ollama (si se registró).
    pub registered_as: Option<String>,
    /// Directorio (host) donde se guardó el tokenizer HF (si se descargó).
    pub tokenizer_dir: Option<String>,
    /// Aviso si el tokenizer no pudo descargarse (el GGUF siguió OK).
    pub tokenizer_warning: Option<String>,
}

/// Orquestador de `ai model <sub>`: instancias + biblioteca + registro + descarga.
pub struct ManageAiModels {
    instances: Arc<dyn AiAdapterPort + Send + Sync>,
    library: Arc<dyn AiModelLibraryPort + Send + Sync>,
    registry: Arc<dyn AiModelRegistryPort + Send + Sync>,
    downloader: Option<Arc<dyn ModelDownloadPort + Send + Sync>>,
    tokenizer: Option<Arc<dyn TokenizerDownloadPort + Send + Sync>>,
}

impl ManageAiModels {
    /// Construye el caso de uso con los puertos inyectados.
    pub fn new(
        instances: Arc<dyn AiAdapterPort + Send + Sync>,
        library: Arc<dyn AiModelLibraryPort + Send + Sync>,
        registry: Arc<dyn AiModelRegistryPort + Send + Sync>,
        downloader: Option<Arc<dyn ModelDownloadPort + Send + Sync>>,
        tokenizer: Option<Arc<dyn TokenizerDownloadPort + Send + Sync>>,
    ) -> Self {
        Self {
            instances,
            library,
            registry,
            downloader,
            tokenizer,
        }
    }

    async fn require_instance(&self, instance: &str) -> Result<()> {
        crate::domain::ai::validate_ai_instance_name(instance)?;
        let inst = self.instances.get_instance(instance).await?;
        if inst.api_port.is_none() {
            return Err(EnolaError::ValidationError(format!(
                "La instancia '{instance}' no tiene la API de Ollama activa (¿`ai start {instance}`?)"
            )));
        }
        Ok(())
    }

    /// `ai model list [--instance <name>]`.
    pub async fn list(&self, instance: Option<&str>) -> Result<Vec<InstanceModelReport>> {
        let names: Vec<String> = match instance {
            Some(name) => {
                crate::domain::ai::validate_ai_instance_name(name)?;
                vec![name.to_string()]
            }
            None => self
                .instances
                .list_instances()
                .await?
                .into_iter()
                .map(|i| i.name)
                .collect(),
        };

        let mut out = Vec::with_capacity(names.len());
        for name in names {
            let library = self
                .library
                .list_library_models(&name)
                .await
                .unwrap_or_default();
            let (registered, note) = match self.registry.list_registered_models(&name).await {
                Ok(models) => (models, None),
                Err(e) => (
                    Vec::new(),
                    Some(format!("registro no consultado (¿instancia parada?): {e}")),
                ),
            };
            out.push(InstanceModelReport {
                instance: name,
                library,
                registered,
                note,
            });
        }
        Ok(out)
    }

    /// `ai model download <instance> <source> [--file] [--sha256] [--name] [--tor] [--no-register]`.
    pub async fn download(
        &self,
        instance: &str,
        source: &str,
        file: Option<&str>,
        expected_sha256: Option<&str>,
        name: Option<&str>,
        use_tor: bool,
        register: bool,
    ) -> Result<ModelInstallReport> {
        self.require_instance(instance).await?;
        let resolved = ai_model::resolve_download_source(source, file)?;
        let downloader = self.downloader.as_ref().ok_or_else(|| {
            EnolaError::InfrastructureError("Descarga de modelos no disponible".into())
        })?;
        let dest = self
            .library
            .resolve_library_target(instance, &resolved.filename)
            .await?;

        let outcome = downloader
            .download(ModelDownloadRequest {
                url: resolved.url,
                destination: dest,
                expected_sha256: expected_sha256.map(str::to_string),
                use_tor,
            })
            .await?;

        // Validar header GGUF real tras la descarga (un HTML de error no es modelo).
        self.library
            .read_gguf_metadata(instance, &resolved.filename)
            .await?;

        // S2: persistir proveniencia (sha256 + origen + hf_repo) en index.json.
        self.library
            .record_download_provenance(
                instance,
                &resolved.filename,
                &outcome.sha256,
                resolved.hf_repo.clone(),
            )
            .await?;

        // S3: tokenizer HF host-side (best-effort: si falla se avisa, no aborta).
        let (tokenizer_dir, tokenizer_warning) = self
            .download_tokenizer_best_effort(instance, resolved.hf_repo.as_deref(), use_tor)
            .await;

        let registered_as = if register {
            let reg_name = match name {
                Some(n) => {
                    ai_model::validate_ollama_model_name(n)?;
                    n.to_string()
                }
                None => ai_model::default_registry_name(&resolved.filename)?,
            };
            self.registry
                .register_library_model(instance, &resolved.filename, &reg_name)
                .await?;
            Some(reg_name)
        } else {
            None
        };

        Ok(ModelInstallReport {
            instance: instance.to_string(),
            filename: resolved.filename,
            bytes_written: outcome.bytes_written,
            sha256: outcome.sha256,
            resumed: outcome.resumed,
            via_tor: outcome.via_tor,
            registered_as,
            tokenizer_dir,
            tokenizer_warning,
        })
    }

    /// Descarga (best-effort) el tokenizer HF del repo y devuelve
    /// `(directorio_guardado, aviso)`. Nunca falla: un problema del tokenizer
    /// se convierte en aviso para no tumbar una descarga de GGUF ya completada.
    async fn download_tokenizer_best_effort(
        &self,
        instance: &str,
        repo: Option<&str>,
        use_tor: bool,
    ) -> (Option<String>, Option<String>) {
        let Some(repo) = repo else {
            return (None, None);
        };
        let dest = match self.library.resolve_tokenizer_dir(instance, repo).await {
            Ok(d) => d,
            Err(e) => {
                return (
                    None,
                    Some(format!(
                        "no se pudo resolver el directorio del tokenizer: {e}"
                    )),
                )
            }
        };
        let Some(downloader) = self.tokenizer.as_ref() else {
            return (
                None,
                Some("descarga de tokenizer no disponible en este entorno".to_string()),
            );
        };
        match downloader
            .download_tokenizer(TokenizerDownloadRequest {
                repo: repo.to_string(),
                destination: dest,
                use_tor,
            })
            .await
        {
            Ok(outcome) => (Some(outcome.destination.display().to_string()), None),
            Err(e) => (
                None,
                Some(format!(
                    "tokenizer HF no descargado de '{repo}' (usa --tokenizer al afinar): {e}"
                )),
            ),
        }
    }

    /// `ai model import <instance> <path> [--name] [--no-register]`.
    pub async fn import(
        &self,
        instance: &str,
        path: &Path,
        name: Option<&str>,
        register: bool,
    ) -> Result<ModelInstallReport> {
        self.require_instance(instance).await?;
        let model = self.library.import_library_file(instance, path).await?;

        let registered_as = if register {
            let reg_name = match name {
                Some(n) => {
                    ai_model::validate_ollama_model_name(n)?;
                    n.to_string()
                }
                None => model.name.clone(),
            };
            self.registry
                .register_library_model(instance, &model.filename, &reg_name)
                .await?;
            Some(reg_name)
        } else {
            None
        };

        Ok(ModelInstallReport {
            instance: instance.to_string(),
            filename: model.filename,
            bytes_written: model.size_bytes,
            sha256: model.sha256.unwrap_or_default(),
            resumed: false,
            via_tor: false,
            registered_as,
            tokenizer_dir: None,
            tokenizer_warning: None,
        })
    }

    /// `ai model add <instance> <model> [--name]`:
    /// `.gguf` → registrar fichero de biblioteca (`/api/create`);
    /// otro → pull del registro Ollama (`/api/pull`).
    pub async fn add(&self, instance: &str, model: &str, name: Option<&str>) -> Result<String> {
        self.require_instance(instance).await?;
        if model.to_ascii_lowercase().ends_with(".gguf") {
            ai_model::validate_gguf_filename(model)?;
            let reg_name = match name {
                Some(n) => {
                    ai_model::validate_ollama_model_name(n)?;
                    n.to_string()
                }
                None => ai_model::default_registry_name(model)?,
            };
            self.registry
                .register_library_model(instance, model, &reg_name)
                .await?;
            Ok(format!(
                "'{model}' registrado en '{instance}' como '{reg_name}'"
            ))
        } else {
            if name.is_some() {
                return Err(EnolaError::ValidationError(
                    "--name solo aplica al registrar un .gguf de la biblioteca".into(),
                ));
            }
            ai_model::validate_ollama_model_name(model)?;
            self.registry.pull_registry_model(instance, model).await?;
            Ok(format!(
                "'{model}' descargado en '{instance}' vía /api/pull"
            ))
        }
    }

    /// `ai model delete <instance> <model|file.gguf> --force`.
    pub async fn delete(&self, instance: &str, model: &str, force: bool) -> Result<String> {
        crate::domain::ai::validate_ai_instance_name(instance)?;
        if !force {
            return Err(EnolaError::ValidationError(format!(
                "Eliminar '{model}' de '{instance}' requiere confirmar con --force"
            )));
        }
        if model.to_ascii_lowercase().ends_with(".gguf") {
            ai_model::validate_gguf_filename(model)?;
            self.library.delete_library_file(instance, model).await?;
            Ok(format!(
                "fichero '{model}' eliminado de la biblioteca de '{instance}'"
            ))
        } else {
            ai_model::validate_ollama_model_name(model)?;
            self.registry
                .delete_registered_model(instance, model)
                .await?;
            Ok(format!(
                "modelo '{model}' eliminado del registro de '{instance}'"
            ))
        }
    }

    /// `ai model validate <instance> <file.gguf>`: header + heurísticas.
    pub async fn validate(&self, instance: &str, filename: &str) -> Result<AiModel> {
        crate::domain::ai::validate_ai_instance_name(instance)?;
        ai_model::validate_gguf_filename(filename)?;
        let meta = self.library.read_gguf_metadata(instance, filename).await?;
        let mut models = self.library.list_library_models(instance).await?;
        let model = models
            .drain(..)
            .find(|m| m.filename == filename)
            .unwrap_or_else(|| {
                let (family, quantization) = ai_model::parse_filename_heuristics(filename);
                AiModel {
                    name: ai_model::default_registry_name(filename)
                        .unwrap_or_else(|_| filename.to_string()),
                    filename: filename.to_string(),
                    size_bytes: 0,
                    quantization,
                    family,
                    status: ai_model::AiModelStatus::Ready,
                    source: ai_model::ModelSource::Downloaded,
                    sha256: None,
                    hf_repo: None,
                }
            });
        debug_assert!(meta.tensor_count <= ai_model::MAX_GGUF_COUNT);
        Ok(model)
    }

    /// `ai model refresh <instance>`: re-escaneo de biblioteca + registro.
    pub async fn refresh(&self, instance: &str) -> Result<InstanceModelReport> {
        self.require_instance(instance).await?;
        let library = self.library.list_library_models(instance).await?;
        let registered = self.registry.list_registered_models(instance).await?;
        Ok(InstanceModelReport {
            instance: instance.to_string(),
            library,
            registered,
            note: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ai::AiInstanceStatus;
    use crate::ports::ai::{
        MockAiAdapterPort, MockAiModelLibraryPort, MockAiModelRegistryPort, MockModelDownloadPort,
        MockTokenizerDownloadPort, ModelDownloadOutcome, TokenizerDownloadOutcome,
    };
    use std::path::PathBuf;

    fn inst(name: &str) -> crate::domain::ai::AiInstance {
        crate::domain::ai::AiInstance {
            name: name.into(),
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

    fn svc(
        instances: MockAiAdapterPort,
        library: MockAiModelLibraryPort,
        registry: MockAiModelRegistryPort,
        downloader: MockModelDownloadPort,
    ) -> ManageAiModels {
        svc_with_tok(instances, library, registry, downloader, None)
    }

    fn svc_with_tok(
        instances: MockAiAdapterPort,
        library: MockAiModelLibraryPort,
        registry: MockAiModelRegistryPort,
        downloader: MockModelDownloadPort,
        tokenizer: Option<MockTokenizerDownloadPort>,
    ) -> ManageAiModels {
        ManageAiModels::new(
            Arc::new(instances),
            Arc::new(library),
            Arc::new(registry),
            Some(Arc::new(downloader)),
            tokenizer.map(|t| Arc::new(t) as Arc<dyn TokenizerDownloadPort + Send + Sync>),
        )
    }

    #[tokio::test]
    async fn download_resolves_target_downloads_and_registers() {
        let mut insts = MockAiAdapterPort::new();
        insts.expect_get_instance().returning(|n| Ok(inst(n)));

        let mut lib = MockAiModelLibraryPort::new();
        lib.expect_resolve_library_target()
            .withf(|i, f| i == "dev" && f == "m.gguf")
            .returning(|_, f| {
                Ok(PathBuf::from(format!(
                    "/srv/enola-ai/dev/ollama/models-gguf/{f}"
                )))
            });
        lib.expect_read_gguf_metadata().returning(|_, _| {
            Ok(ai_model::GgufMetadata {
                version: 3,
                tensor_count: 1,
                metadata_kv_count: 0,
                architecture: None,
                file_type: None,
            })
        });
        lib.expect_record_download_provenance()
            .withf(|i, f, sha, repo| {
                i == "dev" && f == "m.gguf" && sha == "ab12" && repo.as_deref() == Some("org/repo")
            })
            .times(1)
            .returning(|_, _, _, _| Ok(()));
        // hf_repo presente → se resuelve el directorio del tokenizer; al no
        // haber descargador de tokenizer, el flujo avisa (best-effort).
        lib.expect_resolve_tokenizer_dir()
            .withf(|i, r| i == "dev" && r == "org/repo")
            .times(1)
            .returning(|_, r| {
                Ok(PathBuf::from(format!(
                    "/srv/enola-ai/dev/ollama/models-gguf/tokenizer/{}",
                    r.replace('/', "__")
                )))
            });

        let mut reg = MockAiModelRegistryPort::new();
        reg.expect_register_library_model()
            .withf(|i, f, n| i == "dev" && f == "m.gguf" && n == "custom:q4")
            .times(1)
            .returning(|_, _, _| Ok(()));

        let mut dl = MockModelDownloadPort::new();
        dl.expect_download()
            .withf(|r| {
                r.url == "https://huggingface.co/org/repo/resolve/main/m.gguf"
                    && !r.use_tor
                    && r.expected_sha256.as_deref() == Some("ab12")
            })
            .times(1)
            .returning(|r| {
                Ok(ModelDownloadOutcome {
                    destination: r.destination,
                    bytes_written: 128,
                    sha256: "ab12".into(),
                    resumed: false,
                    via_tor: r.use_tor,
                })
            });

        let s = svc(insts, lib, reg, dl);
        let out = s
            .download(
                "dev",
                "org/repo",
                Some("m.gguf"),
                Some("ab12"),
                Some("custom:q4"),
                false,
                true,
            )
            .await
            .unwrap();
        assert_eq!(out.filename, "m.gguf");
        assert_eq!(out.registered_as.as_deref(), Some("custom:q4"));
        assert!(!out.via_tor);
        // Sin descargador de tokenizer inyectado, se avisa pero no falla.
        assert_eq!(out.tokenizer_dir, None);
        assert!(out.tokenizer_warning.is_some());
    }

    #[tokio::test]
    async fn download_tor_flag_propagates_to_downloader() {
        let mut insts = MockAiAdapterPort::new();
        insts.expect_get_instance().returning(|n| Ok(inst(n)));
        let mut lib = MockAiModelLibraryPort::new();
        lib.expect_resolve_library_target()
            .returning(|_, f| Ok(PathBuf::from(f)));
        lib.expect_read_gguf_metadata().returning(|_, _| {
            Ok(ai_model::GgufMetadata {
                version: 3,
                tensor_count: 1,
                metadata_kv_count: 0,
                architecture: None,
                file_type: None,
            })
        });
        lib.expect_record_download_provenance()
            .withf(|i, f, _sha, repo| i == "dev" && f == "m.gguf" && repo.is_none())
            .times(1)
            .returning(|_, _, _, _| Ok(()));
        let mut reg = MockAiModelRegistryPort::new();
        reg.expect_register_library_model().times(0);
        let mut dl = MockModelDownloadPort::new();
        dl.expect_download()
            .withf(|r| r.use_tor && r.url == "https://h.example/m.gguf")
            .times(1)
            .returning(|r| {
                Ok(ModelDownloadOutcome {
                    destination: r.destination,
                    bytes_written: 1,
                    sha256: "x".into(),
                    resumed: false,
                    via_tor: true,
                })
            });
        let s = svc(insts, lib, reg, dl);
        let out = s
            .download(
                "dev",
                "https://h.example/m.gguf",
                None,
                None,
                None,
                true,
                false,
            )
            .await
            .unwrap();
        assert!(out.via_tor);
        assert!(out.registered_as.is_none());
        // URL directa → sin hf_repo → sin tokenizer ni aviso.
        assert_eq!(out.tokenizer_dir, None);
        assert_eq!(out.tokenizer_warning, None);
    }

    #[tokio::test]
    async fn download_with_hf_repo_fetches_tokenizer() {
        let mut insts = MockAiAdapterPort::new();
        insts.expect_get_instance().returning(|n| Ok(inst(n)));
        let mut lib = MockAiModelLibraryPort::new();
        lib.expect_resolve_library_target()
            .returning(|_, f| Ok(PathBuf::from(f)));
        lib.expect_read_gguf_metadata().returning(|_, _| {
            Ok(ai_model::GgufMetadata {
                version: 3,
                tensor_count: 1,
                metadata_kv_count: 0,
                architecture: None,
                file_type: None,
            })
        });
        lib.expect_record_download_provenance()
            .returning(|_, _, _, _| Ok(()));
        lib.expect_resolve_tokenizer_dir()
            .withf(|i, r| i == "dev" && r == "org/repo")
            .times(1)
            .returning(|_, r| {
                Ok(PathBuf::from(format!(
                    "/srv/enola-ai/dev/ollama/models-gguf/tokenizer/{}",
                    r.replace('/', "__")
                )))
            });
        let mut reg = MockAiModelRegistryPort::new();
        reg.expect_register_library_model().times(0);
        let mut dl = MockModelDownloadPort::new();
        dl.expect_download().times(1).returning(|r| {
            Ok(ModelDownloadOutcome {
                destination: r.destination,
                bytes_written: 1,
                sha256: "x".into(),
                resumed: false,
                via_tor: false,
            })
        });
        let mut tok = MockTokenizerDownloadPort::new();
        tok.expect_download_tokenizer()
            .withf(|req| req.repo == "org/repo" && !req.use_tor)
            .times(1)
            .returning(|req| {
                Ok(TokenizerDownloadOutcome {
                    destination: req.destination,
                    files_downloaded: 2,
                })
            });

        let s = svc_with_tok(insts, lib, reg, dl, Some(tok));
        let out = s
            .download("dev", "org/repo", Some("m.gguf"), None, None, false, false)
            .await
            .unwrap();
        assert_eq!(
            out.tokenizer_dir.as_deref(),
            Some("/srv/enola-ai/dev/ollama/models-gguf/tokenizer/org__repo")
        );
        assert_eq!(out.tokenizer_warning, None);
    }

    #[tokio::test]
    async fn download_tokenizer_failure_is_warning() {
        let mut insts = MockAiAdapterPort::new();
        insts.expect_get_instance().returning(|n| Ok(inst(n)));
        let mut lib = MockAiModelLibraryPort::new();
        lib.expect_resolve_library_target()
            .returning(|_, f| Ok(PathBuf::from(f)));
        lib.expect_read_gguf_metadata().returning(|_, _| {
            Ok(ai_model::GgufMetadata {
                version: 3,
                tensor_count: 1,
                metadata_kv_count: 0,
                architecture: None,
                file_type: None,
            })
        });
        lib.expect_record_download_provenance()
            .returning(|_, _, _, _| Ok(()));
        lib.expect_resolve_tokenizer_dir()
            .returning(|_, r| Ok(PathBuf::from(r)));
        let mut reg = MockAiModelRegistryPort::new();
        reg.expect_register_library_model().times(0);
        let mut dl = MockModelDownloadPort::new();
        dl.expect_download().times(1).returning(|r| {
            Ok(ModelDownloadOutcome {
                destination: r.destination,
                bytes_written: 1,
                sha256: "x".into(),
                resumed: false,
                via_tor: false,
            })
        });
        let mut tok = MockTokenizerDownloadPort::new();
        tok.expect_download_tokenizer()
            .times(1)
            .returning(|_| Err(EnolaError::NotFound("no tokenizer".into())));

        let s = svc_with_tok(insts, lib, reg, dl, Some(tok));
        let out = s
            .download("dev", "org/repo", Some("m.gguf"), None, None, false, false)
            .await
            .unwrap();
        assert_eq!(out.tokenizer_dir, None);
        assert!(out.tokenizer_warning.unwrap().contains("tokenizer"));
    }

    #[tokio::test]
    async fn download_rejects_invalid_source_before_network() {
        let mut insts = MockAiAdapterPort::new();
        insts.expect_get_instance().returning(|n| Ok(inst(n)));
        let lib = MockAiModelLibraryPort::new();
        let reg = MockAiModelRegistryPort::new();
        let mut dl = MockModelDownloadPort::new();
        dl.expect_download().times(0);
        let s = svc(insts, lib, reg, dl);
        assert!(s
            .download("dev", "org/repo", None, None, None, false, true)
            .await
            .is_err());
        assert!(s
            .download("dev", "ftp://x/m.gguf", None, None, None, false, true)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn add_dispatches_pull_for_registry_names_and_create_for_gguf() {
        let mut insts = MockAiAdapterPort::new();
        insts.expect_get_instance().returning(|n| Ok(inst(n)));
        let lib = MockAiModelLibraryPort::new();
        let mut reg = MockAiModelRegistryPort::new();
        reg.expect_pull_registry_model()
            .withf(|i, m| i == "dev" && m == "llama3.1:8b")
            .times(1)
            .returning(|_, _| Ok(()));
        reg.expect_register_library_model()
            .withf(|i, f, n| i == "dev" && f == "m.gguf" && n == "m")
            .times(1)
            .returning(|_, _, _| Ok(()));
        let s = svc(insts, lib, reg, MockModelDownloadPort::new());
        s.add("dev", "llama3.1:8b", None).await.unwrap();
        s.add("dev", "m.gguf", None).await.unwrap();
        assert!(s.add("dev", "llama3.1:8b", Some("alias")).await.is_err());
    }

    #[tokio::test]
    async fn delete_requires_force_and_routes_by_suffix() {
        let mut insts = MockAiAdapterPort::new();
        insts.expect_get_instance().returning(|n| Ok(inst(n)));
        let mut lib = MockAiModelLibraryPort::new();
        lib.expect_delete_library_file()
            .withf(|i, f| i == "dev" && f == "m.gguf")
            .times(1)
            .returning(|_, _| Ok(()));
        let mut reg = MockAiModelRegistryPort::new();
        reg.expect_delete_registered_model()
            .withf(|i, m| i == "dev" && m == "llama3.1:8b")
            .times(1)
            .returning(|_, _| Ok(()));
        let s = svc(insts, lib, reg, MockModelDownloadPort::new());
        assert!(s.delete("dev", "m.gguf", false).await.is_err());
        assert!(s.delete("dev", "llama3.1:8b", false).await.is_err());
        s.delete("dev", "m.gguf", true).await.unwrap();
        s.delete("dev", "llama3.1:8b", true).await.unwrap();
    }

    #[tokio::test]
    async fn list_aggregates_library_and_registry_per_instance() {
        let mut insts = MockAiAdapterPort::new();
        insts
            .expect_list_instances()
            .returning(|| Ok(vec![inst("dev"), inst("gpu1")]));
        let mut lib = MockAiModelLibraryPort::new();
        lib.expect_list_library_models().returning(|_| Ok(vec![]));
        let mut reg = MockAiModelRegistryPort::new();
        reg.expect_list_registered_models().returning(|_| {
            Ok(vec![LlmModelInfo {
                name: "llama3.1:8b".into(),
                size_bytes: Some(1),
                modified_at: None,
            }])
        });
        let s = svc(insts, lib, reg, MockModelDownloadPort::new());
        let reports = s.list(None).await.unwrap();
        assert_eq!(reports.len(), 2);
        assert_eq!(reports[0].registered.len(), 1);
        // Con --instance concreto solo se consulta esa.
        let one = s.list(Some("dev")).await.unwrap();
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].instance, "dev");
    }

    #[tokio::test]
    async fn validate_uses_library_metadata() {
        let mut insts = MockAiAdapterPort::new();
        insts.expect_get_instance().returning(|n| Ok(inst(n)));
        let mut lib = MockAiModelLibraryPort::new();
        lib.expect_read_gguf_metadata()
            .withf(|i, f| i == "dev" && f == "m.gguf")
            .returning(|_, _| {
                Ok(ai_model::GgufMetadata {
                    version: 3,
                    tensor_count: 7,
                    metadata_kv_count: 2,
                    architecture: None,
                    file_type: None,
                })
            });
        lib.expect_list_library_models().returning(|_| {
            Ok(vec![AiModel {
                name: "m".into(),
                filename: "m.gguf".into(),
                size_bytes: 42,
                quantization: "Q4_K_M".into(),
                family: "Llama 3".into(),
                status: ai_model::AiModelStatus::Ready,
                source: ai_model::ModelSource::Imported,
                sha256: Some("x".into()),
                hf_repo: None,
            }])
        });
        let s = svc(
            insts,
            lib,
            MockAiModelRegistryPort::new(),
            MockModelDownloadPort::new(),
        );
        let m = s.validate("dev", "m.gguf").await.unwrap();
        assert_eq!(m.filename, "m.gguf");
        assert_eq!(m.quantization, "Q4_K_M");
        // Rechaza traversal antes de tocar el port.
        assert!(s.validate("dev", "../x.gguf").await.is_err());
    }

    #[tokio::test]
    async fn refresh_returns_fresh_scan() {
        let mut insts = MockAiAdapterPort::new();
        insts.expect_get_instance().returning(|n| Ok(inst(n)));
        let mut lib = MockAiModelLibraryPort::new();
        lib.expect_list_library_models()
            .times(1)
            .returning(|_| Ok(vec![]));
        let mut reg = MockAiModelRegistryPort::new();
        reg.expect_list_registered_models()
            .times(1)
            .returning(|_| Ok(vec![]));
        let s = svc(insts, lib, reg, MockModelDownloadPort::new());
        let rep = s.refresh("dev").await.unwrap();
        assert_eq!(rep.instance, "dev");
    }
}
