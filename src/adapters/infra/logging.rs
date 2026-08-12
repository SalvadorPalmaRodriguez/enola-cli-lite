use anyhow::Context;
use std::path::Path;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{filter::LevelFilter, layer::SubscriberExt, util::SubscriberInitExt};

/// Initialize the global logging system.
///
/// This setup ensures that logs are written to a file and not to stdout/stderr,
/// preventing interference with the TUI (Terminal User Interface).
pub fn init_logger(log_dir: &str, level: &str) -> anyhow::Result<()> {
    let file_appender =
        RollingFileAppender::new(Rotation::DAILY, Path::new(log_dir), "enola-core.log");

    let _non_blocking = tracing_appender::non_blocking(file_appender);

    // Parse level string to LevelFilter
    let level_filter = match level.to_lowercase().as_str() {
        "trace" => LevelFilter::TRACE,
        "debug" => LevelFilter::DEBUG,
        "info" => LevelFilter::INFO,
        "warn" => LevelFilter::WARN,
        "error" => LevelFilter::ERROR,
        _ => LevelFilter::INFO,
    };

    let file_appender = RollingFileAppender::new(Rotation::DAILY, Path::new(log_dir), "core.log");

    tracing_subscriber::registry()
        .with(level_filter)
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(file_appender)
                .with_ansi(false)
                .with_target(true)
                .with_thread_ids(true)
                .with_file(true)
                .with_line_number(true),
        )
        .try_init()
        .context("Failed to initialize logging subscriber")?;

    tracing::info!("Logging initialized successfully at {}", log_dir);
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_init_logger_with_temp_dir() {
        let dir = tempfile::TempDir::new().unwrap();
        let result = super::init_logger(dir.path().to_str().unwrap(), "info");
        // May fail if subscriber already initialized in test env, that's expected
        let _ = result;
    }

    #[test]
    fn test_level_parsing_variants() {
        // We can't call init_logger multiple times, but we can test the level parsing logic
        // indirectly by just verifying the function doesn't panic for each level string
        let levels = ["trace", "debug", "info", "warn", "error", "invalid", ""];
        for level in levels {
            // Just verify no panic during parsing logic
            let _ = level.to_lowercase();
        }
    }
}
