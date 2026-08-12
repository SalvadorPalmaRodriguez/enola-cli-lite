use thiserror::Error;

#[derive(Error, Debug)]
pub enum EnolaError {
    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("Authentication failed: {0}")]
    AuthError(String),

    #[error("{0}")]
    InfrastructureError(String),

    #[error("{0}")]
    ValidationError(String),

    #[error("{0}")]
    NotFound(String),

    #[error("Operation timed out: {0}")]
    Timeout(String),

    #[error("Security violation: {0}")]
    SecurityError(String),

    #[error("{0}")]
    FileSystemError(String),

    #[error("{0}")]
    IoError(#[from] std::io::Error),

    #[error("{0}")]
    Unknown(String),

    #[error("External provider error: {0}")]
    ExternalProviderError(String),
}

pub type Result<T> = std::result::Result<T, EnolaError>;
