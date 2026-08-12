use crate::cli::commands::CliError;
use crate::domain::error::EnolaError;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct ApiError {
    pub error: String,
    pub code: u16,
}

impl From<EnolaError> for ApiError {
    fn from(e: EnolaError) -> Self {
        ApiError::from(CliError::from(e))
    }
}

impl From<CliError> for ApiError {
    fn from(e: CliError) -> Self {
        let (status, msg) = match &e {
            CliError::InvalidInput(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            CliError::NotImplemented(msg) => (StatusCode::NOT_IMPLEMENTED, msg.clone()),
            CliError::Io(err) => {
                if err.kind() == std::io::ErrorKind::PermissionDenied {
                    (
                        StatusCode::FORBIDDEN,
                        "Permission denied. Run with sudo.".to_string(),
                    )
                } else {
                    (StatusCode::INTERNAL_SERVER_ERROR, format!("{}", err))
                }
            }
            CliError::Domain(err) => (StatusCode::INTERNAL_SERVER_ERROR, format!("{}", err)),
            CliError::Generic(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.clone()),
            CliError::ControlledExit { code, stderr, .. } => {
                let msg = stderr.as_deref().unwrap_or("controlled exit").to_string();
                let status = match code {
                    13 => StatusCode::FORBIDDEN,
                    21 => StatusCode::UNPROCESSABLE_ENTITY,
                    _ => StatusCode::INTERNAL_SERVER_ERROR,
                };
                return ApiError {
                    error: msg,
                    code: status.as_u16(),
                };
            }
        };
        ApiError {
            error: msg,
            code: status.as_u16(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = StatusCode::from_u16(self.code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        (status, axum::Json(self)).into_response()
    }
}

pub type ApiResult<T> = Result<axum::Json<T>, ApiError>;
