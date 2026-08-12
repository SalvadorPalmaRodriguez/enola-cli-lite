use crate::domain::error::Result;

/// Port para observar eventos de push en repositorios Git.
///
/// Usa patrón `next_event()` en lugar de closure para ser mockeable con mockall.
/// El watcher lee señales depositadas por el hook post-receive de Forgejo/Git.
#[async_trait::async_trait]
#[cfg_attr(test, mockall::automock)]
pub trait PipelineWatcherPort: Send + Sync {
    /// Inicializa el watcher (crea directorios de triggers, registra observador).
    fn init(&mut self) -> Result<()>;

    /// Espera al siguiente evento de pipeline.
    ///
    /// Retorna `Some((silo_id, branch))` cuando llega un evento,
    /// o `None` si el canal se ha cerrado (shutdown).
    async fn next_event(&mut self) -> Option<Result<(String, String)>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_pipeline_init() {
        let mut mock = MockPipelineWatcherPort::new();
        mock.expect_init().returning(|| Ok(()));
        assert!(mock.init().is_ok());
    }

    #[test]
    fn test_mock_pipeline_init_error() {
        let mut mock = MockPipelineWatcherPort::new();
        mock.expect_init().returning(|| {
            Err(crate::domain::error::EnolaError::InfrastructureError(
                "no dir".into(),
            ))
        });
        assert!(mock.init().is_err());
    }
}
