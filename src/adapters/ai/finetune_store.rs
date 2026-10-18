//! Fase 14 — Adapter `AiFinetuneJobsPort` en disco.
//!
//! `<base>/<instance>/finetune/jobs.json` — escritura atómica + `0640`.

use crate::domain::ai::validate_ai_instance_name;
use crate::domain::ai_finetune::FinetuneJob;
use crate::domain::error::{EnolaError, Result};
use crate::ports::finetune::AiFinetuneJobsPort;
use std::path::PathBuf;

/// Store fs de jobs de fine-tuning.
pub struct FsFinetuneJobsStore {
    /// Directorio base de instancias (`/srv/enola-ai`).
    base_dir: PathBuf,
}

impl FsFinetuneJobsStore {
    /// Crea el store sobre el directorio base de instancias.
    pub fn new(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    fn jobs_path(&self, instance: &str) -> Result<PathBuf> {
        validate_ai_instance_name(instance)?;
        Ok(self
            .base_dir
            .join(instance)
            .join("finetune")
            .join("jobs.json"))
    }

    fn read(&self, instance: &str) -> Result<Vec<FinetuneJob>> {
        let path = self.jobs_path(instance)?;
        match std::fs::read_to_string(&path) {
            Ok(raw) => serde_json::from_str(&raw).map_err(|e| {
                EnolaError::InfrastructureError(format!("jobs.json de '{instance}' corrupto: {e}"))
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(EnolaError::InfrastructureError(format!(
                "No se pudo leer jobs.json de '{instance}': {e}"
            ))),
        }
    }

    fn write(&self, instance: &str, jobs: &[FinetuneJob]) -> Result<()> {
        let path = self.jobs_path(instance)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                EnolaError::InfrastructureError(format!("mkdir '{}': {e}", parent.display()))
            })?;
        }
        let json = serde_json::to_string_pretty(jobs)
            .map_err(|e| EnolaError::InfrastructureError(format!("serializar jobs: {e}")))?;
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, json).map_err(|e| {
            EnolaError::InfrastructureError(format!("escribir '{}': {e}", tmp.display()))
        })?;
        std::fs::rename(&tmp, &path).map_err(|e| {
            EnolaError::InfrastructureError(format!("rename '{}': {e}", path.display()))
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640));
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl AiFinetuneJobsPort for FsFinetuneJobsStore {
    async fn list_jobs(&self, instance: &str) -> Result<Vec<FinetuneJob>> {
        self.read(instance)
    }

    async fn add_job(&self, job: FinetuneJob) -> Result<()> {
        let mut jobs = self.read(&job.instance)?;
        if jobs.iter().any(|j| j.id == job.id) {
            return Err(EnolaError::ValidationError(format!(
                "job '{}' duplicado en '{}'",
                job.id, job.instance
            )));
        }
        jobs.push(job);
        self.write(&jobs[0].instance, &jobs)
    }

    async fn update_job(&self, instance: &str, job: &FinetuneJob) -> Result<()> {
        let mut jobs = self.read(instance)?;
        let found = jobs.iter_mut().find(|j| j.id == job.id).ok_or_else(|| {
            EnolaError::NotFound(format!("job '{}' no existe en '{instance}'", job.id))
        })?;
        *found = job.clone();
        self.write(instance, &jobs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ai_finetune::FinetuneJobStatus;

    fn job(id: &str, inst: &str) -> FinetuneJob {
        FinetuneJob {
            id: id.into(),
            instance: inst.into(),
            base_model: "llama3.1:8b".into(),
            dataset: "/data/x.jsonl".into(),
            low_memory: false,
            status: FinetuneJobStatus::Pending,
            container: "enola-ai-dev-finetune-j1".into(),
            created_at: "t0".into(),
            finished_at: None,
            error: None,
            result_model: None,
        }
    }

    #[tokio::test]
    async fn add_list_update_jobs() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsFinetuneJobsStore::new(dir.path().to_path_buf());
        assert!(store.list_jobs("dev").await.unwrap().is_empty());
        store.add_job(job("j1", "dev")).await.unwrap();
        assert!(store.add_job(job("j1", "dev")).await.is_err());
        let mut j = job("j1", "dev");
        j.status = FinetuneJobStatus::Completed;
        j.result_model = Some("dev-ft".into());
        store.update_job("dev", &j).await.unwrap();
        let list = store.list_jobs("dev").await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].status, FinetuneJobStatus::Completed);
        assert_eq!(list[0].result_model.as_deref(), Some("dev-ft"));
    }
}
