// SEC-M3 — Extractores de segmentos de ruta validados.
//
// Los handlers de `web_api.rs` reciben nombres en la URL (`/wp/{name}/…`,
// `/cms/{cms_type}/{name}/…`, `/vpn/{interface}/…`) y los pasan a comandos
// que operan como root. Estos extractores aplican la validación de dominio
// ANTES del handler y responden 400 si no la cumple: defensa en profundidad
// aunque cada comando valide después.
//
// - `ValidName` / `ValidNames`: `validate_tor_service_name` (minúsculas,
//   dígitos, `-`, `_`; empieza por letra o dígito; máx. 80). Es el superconjunto
//   de nombres base, nombres Tor canónicos (`wp_blog`, `proxy_web1`), tipos de
//   servicio/CMS y familias de comando.
// - `ValidIface`: `validate_vpn_name` (reglas de interfaz Linux).

use axum::extract::{FromRequestParts, Path};
use axum::http::request::Parts;
use axum::http::StatusCode;

use crate::adapters::web::web_errors::ApiError;
use crate::domain::naming::validate_tor_service_name;
use crate::domain::vpn::validate_vpn_name;

fn bad_request(error: String) -> ApiError {
    ApiError {
        error,
        code: StatusCode::BAD_REQUEST.as_u16(),
    }
}

fn check_name(s: &str) -> Result<(), ApiError> {
    validate_tor_service_name(s).map_err(|e| bad_request(e.to_string()))
}

/// Un segmento `{name}` validado con `validate_tor_service_name`.
#[derive(Debug, Clone)]
pub struct ValidName(pub String);

impl<S: Send + Sync> FromRequestParts<S> for ValidName {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let Path(name) = Path::<String>::from_request_parts(parts, state)
            .await
            .map_err(|e| bad_request(e.body_text()))?;
        check_name(&name)?;
        Ok(Self(name))
    }
}

/// Dos segmentos (`{tipo}/{name}`), ambos validados como `ValidName`.
#[derive(Debug, Clone)]
pub struct ValidNames(pub String, pub String);

impl<S: Send + Sync> FromRequestParts<S> for ValidNames {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let Path((a, b)) = Path::<(String, String)>::from_request_parts(parts, state)
            .await
            .map_err(|e| bad_request(e.body_text()))?;
        check_name(&a)?;
        check_name(&b)?;
        Ok(Self(a, b))
    }
}

/// Un segmento `{interface}` validado con `validate_vpn_name`.
#[derive(Debug, Clone)]
pub struct ValidIface(pub String);

impl<S: Send + Sync> FromRequestParts<S> for ValidIface {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let Path(iface) = Path::<String>::from_request_parts(parts, state)
            .await
            .map_err(|e| bad_request(e.body_text()))?;
        validate_vpn_name(&iface).map_err(bad_request)?;
        Ok(Self(iface))
    }
}
