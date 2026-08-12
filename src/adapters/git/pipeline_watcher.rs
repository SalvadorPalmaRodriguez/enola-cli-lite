use crate::domain::error::{EnolaError, Result};
use crate::ports::pipeline::PipelineWatcherPort;
use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use tokio::sync::mpsc;
use tracing::{error, info, warn};

pub struct GitPipelineWatcher {
    watcher: Option<RecommendedWatcher>,
    trigger_path: PathBuf,
    rx: Option<mpsc::Receiver<Result<Event>>>,
}

#[derive(Debug, serde::Deserialize)]
struct GitTriggerSignal {
    repo: String,
    branch: String,
    // commit: String, // unused for now
}

impl GitPipelineWatcher {
    pub fn new(trigger_path: &Path) -> Self {
        Self {
            watcher: None,
            trigger_path: trigger_path.to_path_buf(),
            rx: None,
        }
    }

    /// Initialize the watcher
    pub fn init(&mut self) -> Result<()> {
        if !self.trigger_path.exists() {
            std::fs::create_dir_all(&self.trigger_path).map_err(EnolaError::IoError)?;
        }

        let (tx, rx) = mpsc::channel(100);

        let tx_clone = tx.clone();

        // Use notify::recommended_watcher instead of RecommendedWatcher::new if new is not available or signature mismatch
        // notify 6.1: RecommendedWatcher::new(tx, config)
        let watcher = RecommendedWatcher::new(
            move |res: std::result::Result<Event, notify::Error>| {
                let _ = tx_clone
                    .blocking_send(res.map_err(|e| EnolaError::InfrastructureError(e.to_string())));
            },
            Config::default(),
        )
        .map_err(|e| EnolaError::InfrastructureError(e.to_string()))?;

        self.watcher = Some(watcher);
        self.rx = Some(rx);

        self.watcher
            .as_mut()
            .ok_or_else(|| EnolaError::InfrastructureError("Watcher not initialized".to_string()))?
            .watch(&self.trigger_path, RecursiveMode::NonRecursive)
            .map_err(|e| EnolaError::InfrastructureError(e.to_string()))?;

        info!("👀 Git Pipeline Watcher started on {:?}", self.trigger_path);
        Ok(())
    }

    /// Process events loop (async)
    /// Runs until cancelled.
    pub async fn run_loop<F, Fut>(&mut self, handler: F) -> Result<()>
    where
        F: Fn(String, String) -> Fut,
        Fut: std::future::Future<Output = Result<()>>,
    {
        // We cannot iterate over self.rx directly if we want to call methods on self inside the loop
        // because receiving borrows self.rx mutably (and thus self), but calling methods borrows self.
        // Solution: Take rx out of self temporarily or loop differently.
        let mut rx = self.rx.take().ok_or_else(|| {
            EnolaError::InfrastructureError("Watcher not initialized".to_string())
        })?;

        while let Some(res) = rx.recv().await {
            match res {
                Ok(event) => {
                    // Handle Create or Modify events
                    match event.kind {
                        notify::EventKind::Create(_) | notify::EventKind::Modify(_) => {
                            for path in event.paths {
                                if let Some(filename) = path.file_name().and_then(|s| s.to_str()) {
                                    if filename.starts_with("git_push_")
                                        && filename.ends_with(".sig")
                                    {
                                        info!("🔔 Detected signal: {}", filename);

                                        // Handling signal
                                        match Self::handle_signal_static(&path, &handler).await {
                                            Ok(_) => {
                                                // Cleanup signal file
                                                if let Err(e) = tokio::fs::remove_file(&path).await
                                                {
                                                    warn!("Failed to remove signal file: {}", e);
                                                }
                                            }
                                            Err(e) => {
                                                error!(
                                                    "Failed to process signal {:?}: {}",
                                                    path, e
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
                Err(e) => error!("Watcher error: {}", e),
            }
        }

        // Put rx back (though loop implies we are done or channel closed)
        self.rx = Some(rx);
        Ok(())
    }

    // Made static (associated function) to avoid borrowing self
    async fn handle_signal_static<F, Fut>(path: &Path, handler: &F) -> Result<()>
    where
        F: Fn(String, String) -> Fut,
        Fut: std::future::Future<Output = Result<()>>,
    {
        // Wait briefly for file write to complete (debounce hack)
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // Read signal content
        let content = tokio::fs::read_to_string(path)
            .await
            .map_err(EnolaError::IoError)?;

        if content.trim().is_empty() {
            return Err(EnolaError::InfrastructureError(
                "Signal file empty".to_string(),
            ));
        }

        // Parse JSON
        let signal: GitTriggerSignal = serde_json::from_str(&content)
            .map_err(|e| EnolaError::InfrastructureError(format!("Invalid signal JSON: {}", e)))?;

        info!(
            "🚀 Pipeline Triggered: Repo={}, Branch={}",
            signal.repo, signal.branch
        );

        // Execute Handler
        handler(signal.repo.clone(), signal.branch.clone()).await?;

        Ok(())
    }
}

#[async_trait::async_trait]
impl PipelineWatcherPort for GitPipelineWatcher {
    fn init(&mut self) -> Result<()> {
        GitPipelineWatcher::init(self)
    }

    /// Lee el siguiente evento del canal de señales.
    /// Procesa la señal del archivo, retorna (silo_id, branch) o error.
    async fn next_event(&mut self) -> Option<Result<(String, String)>> {
        let rx = self.rx.as_mut()?;

        loop {
            let res = rx.recv().await?;

            match res {
                Ok(event) => {
                    // Solo procesar Create/Modify
                    match event.kind {
                        notify::EventKind::Create(_) | notify::EventKind::Modify(_) => {}
                        _ => continue,
                    }

                    for path in event.paths {
                        let filename = match path.file_name().and_then(|s| s.to_str()) {
                            Some(f) => f.to_string(),
                            None => continue,
                        };

                        if !filename.starts_with("git_push_") || !filename.ends_with(".sig") {
                            continue;
                        }

                        info!("🔔 PipelineWatcherPort: señal detectada: {}", filename);

                        // Debounce
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

                        // Leer y parsear señal
                        let content = match tokio::fs::read_to_string(&path).await {
                            Ok(c) => c,
                            Err(e) => return Some(Err(EnolaError::IoError(e))),
                        };

                        if content.trim().is_empty() {
                            return Some(Err(EnolaError::InfrastructureError(
                                "Signal file empty".to_string(),
                            )));
                        }

                        #[derive(serde::Deserialize)]
                        struct Sig {
                            repo: String,
                            branch: String,
                        }

                        let sig: Sig = match serde_json::from_str(content.trim()) {
                            Ok(s) => s,
                            Err(e) => {
                                return Some(Err(EnolaError::InfrastructureError(format!(
                                    "Invalid signal JSON: {}",
                                    e
                                ))))
                            }
                        };

                        // Limpiar archivo de señal
                        if let Err(e) = tokio::fs::remove_file(&path).await {
                            warn!("No se pudo eliminar signal file: {}", e);
                        }

                        return Some(Ok((sig.repo, sig.branch)));
                    }
                }
                Err(e) => {
                    error!("Watcher error: {}", e);
                    return Some(Err(e));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_new_watcher() {
        let dir = TempDir::new().unwrap();
        let watcher = GitPipelineWatcher::new(dir.path());
        assert!(watcher.watcher.is_none());
        assert!(watcher.rx.is_none());
    }

    #[test]
    fn test_init_creates_dir_if_missing() {
        let dir = TempDir::new().unwrap();
        let trigger = dir.path().join("triggers");
        let mut watcher = GitPipelineWatcher::new(&trigger);
        watcher.init().unwrap();
        assert!(trigger.exists());
    }

    #[test]
    fn test_init_with_existing_dir() {
        let dir = TempDir::new().unwrap();
        let mut watcher = GitPipelineWatcher::new(dir.path());
        let result = watcher.init();
        assert!(result.is_ok());
    }
}
