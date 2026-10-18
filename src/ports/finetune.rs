// Fase 14 — Port de persistencia de jobs de fine-tuning por instancia.
//
// Estado en `<base>/<instance>/finetune/jobs.json` (atómico + 0640).

use crate::domain::ai_finetune::FinetuneJob;
use crate::domain::error::Result;

/// Port del registro de jobs de fine-tuning.
#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait AiFinetuneJobsPort: Send + Sync {
    /// Lista los jobs de una instancia (vacío si no hay).
    async fn list_jobs(&self, instance: &str) -> Result<Vec<FinetuneJob>>;

    /// Registra un job nuevo (id único; error si ya existe).
    async fn add_job(&self, job: FinetuneJob) -> Result<()>;

    /// Actualiza el estado (y error/finished/result) de un job existente.
    async fn update_job(&self, instance: &str, job: &FinetuneJob) -> Result<()>;
}
