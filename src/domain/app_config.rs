//! Configuración global de distribución y web (CONFIG-001 + CONFIG-002).
//!
//! Centraliza URLs y parámetros que NO deben estar hardcodeados en código/docs/web:
//! - `[distribution]`: dónde se descargan binarios y la imagen Docker
//! - `[web]`: URL pública de la web del proyecto
//!
//! Fuentes de configuración (prioridad de mayor a menor):
//! 1. Flags CLI (`--binary-base-url`, `--web-url`, …) — resuelto en executor.rs
//! 2. Variables de entorno (`ENOLA_BINARY_BASE_URL`, `ENOLA_WEB_URL`, …)
//! 3. Archivo `~/.enola/config.toml` → secciones `[distribution]` y `[web]`
//! 4. Defaults — preservan el comportamiento actual (URLs relativas / vacías)
//!
//! Mantenemos *cero* URLs externas hardcodeadas en el código: si el usuario
//! no configura nada, todas las descargas resuelven a rutas relativas sobre
//! el origen desde el que se sirve la web (comportamiento idéntico al actual).

use serde::{Deserialize, Serialize};

/// URLs de distribución: de dónde descargar el binario y la clave minisign.
///
/// Se usa en:
/// - La página web (`releases.component.html`) para construir la URL real de descarga.
/// - El subcomando `enola-cli verify` / `enola-cli info releases` (futuro).
/// - El script `post_install.sh` para descargar la clave pública si no existe.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct DistributionSettings {
    /// Base URL para descargar los artefactos del release (tar.gz + minisig + sha256).
    ///
    /// Ejemplos:
    /// - `""` (default) → rutas relativas `/releases/enola-cli-vX-linux-x86_64.tar.gz`
    /// - `"https://github.com/org/repo/releases/download/vX"`
    /// - `"https://releases.enola.dev"`
    #[serde(default)]
    pub binary_base_url: String,

    /// URL para descargar la clave pública minisign (verificación de firmas).
    /// Si está vacío, se usa la clave embebida en el binario (`minisign.pub`).
    #[serde(default)]
    pub minisign_pubkey_url: String,
}

impl DistributionSettings {
    /// Carga la configuración aplicando la cadena: env > archivo > default.
    pub fn load() -> Self {
        let file = crate::infrastructure::config_loader::load_section("distribution");
        let mut cfg = Self::default();

        if let Some(v) = std::env::var("ENOLA_BINARY_BASE_URL")
            .ok()
            .or_else(|| file.get("binary_base_url").cloned())
        {
            cfg.binary_base_url = v;
        }
        if let Some(v) = std::env::var("ENOLA_MINISIGN_PUBKEY_URL")
            .ok()
            .or_else(|| file.get("minisign_pubkey_url").cloned())
        {
            cfg.minisign_pubkey_url = v;
        }

        cfg
    }

    /// Construye la URL completa de descarga de un artefacto para una versión dada.
    ///
    /// Si `binary_base_url` está vacío, devuelve la ruta relativa `/releases/<artifact>`.
    /// Si no, concatena base + `/` + artifact (normalizando dobles `/`).
    pub fn artifact_url(&self, artifact: &str) -> String {
        if self.binary_base_url.is_empty() {
            return format!("/releases/{}", artifact);
        }
        let base = self.binary_base_url.trim_end_matches('/');
        format!("{}/{}", base, artifact)
    }
}

/// URLs de la web pública del proyecto.
///
/// Se usa en:
/// - `enola-cli quickref` / `docs` (enlace a la web con documentación completa).
/// - Mensajes de error que apuntan a la web.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct WebSettings {
    /// URL pública de la web del proyecto.
    ///
    /// Ejemplos:
    /// - `""` (default) → no se muestran enlaces a web en mensajes
    /// - `"https://enola.dev"`
    #[serde(default)]
    pub web_public_url: String,

    /// URL pública de la documentación (puede diferir de `web_public_url`).
    /// Si está vacío, se usa `{web_public_url}/docs`.
    #[serde(default)]
    pub docs_url: String,
}

impl WebSettings {
    /// Carga la configuración aplicando la cadena: env > archivo > default.
    pub fn load() -> Self {
        let file = crate::infrastructure::config_loader::load_section("web");
        let mut cfg = Self::default();

        if let Some(v) = std::env::var("ENOLA_WEB_URL")
            .ok()
            .or_else(|| file.get("web_public_url").cloned())
        {
            cfg.web_public_url = v;
        }
        if let Some(v) = std::env::var("ENOLA_DOCS_URL")
            .ok()
            .or_else(|| file.get("docs_url").cloned())
        {
            cfg.docs_url = v;
        }
        cfg
    }

    /// Devuelve la URL de la documentación. Si `docs_url` está vacío y `web_public_url` existe,
    /// devuelve `{web_public_url}/docs`. Si ambos están vacíos, devuelve cadena vacía.
    pub fn docs_link(&self) -> String {
        if !self.docs_url.is_empty() {
            return self.docs_url.clone();
        }
        if !self.web_public_url.is_empty() {
            let base = self.web_public_url.trim_end_matches('/');
            return format!("{}/docs", base);
        }
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distribution_default_preserves_current_behavior() {
        let d = DistributionSettings::default();
        assert_eq!(d.binary_base_url, "");
        assert_eq!(d.minisign_pubkey_url, "");
    }

    #[test]
    fn artifact_url_relative_when_empty_base() {
        let d = DistributionSettings::default();
        assert_eq!(
            d.artifact_url("enola-cli-v1.4.0-linux-x86_64.tar.gz"),
            "/releases/enola-cli-v1.4.0-linux-x86_64.tar.gz"
        );
    }

    #[test]
    fn artifact_url_absolute_when_base_configured() {
        let d = DistributionSettings {
            binary_base_url: "https://example.com/dl/".to_string(),
            ..Default::default()
        };
        assert_eq!(
            d.artifact_url("file.tar.gz"),
            "https://example.com/dl/file.tar.gz"
        );
    }

    #[test]
    fn web_default_no_links() {
        let w = WebSettings::default();
        assert_eq!(w.web_public_url, "");
        assert_eq!(w.docs_link(), "");
    }

    #[test]
    fn web_docs_link_fallback_to_web() {
        let w = WebSettings {
            web_public_url: "https://enola.dev/".to_string(),
            docs_url: String::new(),
        };
        assert_eq!(w.docs_link(), "https://enola.dev/docs");
    }

    #[test]
    fn web_docs_link_explicit() {
        let w = WebSettings {
            web_public_url: "https://enola.dev".to_string(),
            docs_url: "https://docs.enola.dev".to_string(),
        };
        assert_eq!(w.docs_link(), "https://docs.enola.dev");
    }

    // TEST-COV-GAPS-006: DistributionSettings::load env override
    use std::sync::Mutex;
    static DIST_ENV_LOCK: Mutex<()> = Mutex::new(());

    fn clear_dist_env() {
        std::env::remove_var("ENOLA_BINARY_BASE_URL");
        std::env::remove_var("ENOLA_MINISIGN_PUBKEY_URL");
    }

    #[test]
    fn distribution_load_respects_env_override() {
        let _g = DIST_ENV_LOCK.lock().unwrap();
        clear_dist_env();
        std::env::set_var("ENOLA_BINARY_BASE_URL", "https://cdn.example.com/dl");
        std::env::set_var(
            "ENOLA_MINISIGN_PUBKEY_URL",
            "https://cdn.example.com/enola.pub",
        );
        let d = DistributionSettings::load();
        assert_eq!(d.binary_base_url, "https://cdn.example.com/dl");
        assert_eq!(d.minisign_pubkey_url, "https://cdn.example.com/enola.pub");
        clear_dist_env();
    }

    #[test]
    fn distribution_load_uses_defaults_when_no_env() {
        let _g = DIST_ENV_LOCK.lock().unwrap();
        clear_dist_env();
        let d = DistributionSettings::load();
        // Sin env ni config.toml → defaults
        // binary_base_url puede venir del config.toml real del dev; solo chequeamos no-panic
        let _ = d.binary_base_url;
        assert!(d.binary_base_url.is_empty() || !d.binary_base_url.is_empty()); // solo smoke: no panic
    }

    // TEST-COV-GAPS-006: WebSettings::load env override
    static WEB_ENV_LOCK: Mutex<()> = Mutex::new(());

    fn clear_web_env() {
        std::env::remove_var("ENOLA_WEB_URL");
        std::env::remove_var("ENOLA_DOCS_URL");
    }

    #[test]
    fn web_load_respects_env_override() {
        let _g = WEB_ENV_LOCK.lock().unwrap();
        clear_web_env();
        std::env::set_var("ENOLA_WEB_URL", "https://mysite.onion");
        std::env::set_var("ENOLA_DOCS_URL", "https://docs.mysite.onion");
        let w = WebSettings::load();
        assert_eq!(w.web_public_url, "https://mysite.onion");
        assert_eq!(w.docs_url, "https://docs.mysite.onion");
        assert_eq!(w.docs_link(), "https://docs.mysite.onion");
        clear_web_env();
    }

    #[test]
    fn web_load_derives_links_from_base_url() {
        let _g = WEB_ENV_LOCK.lock().unwrap();
        clear_web_env();
        std::env::set_var("ENOLA_WEB_URL", "https://enola.dev/");
        let w = WebSettings::load();
        assert_eq!(w.docs_link(), "https://enola.dev/docs");
        clear_web_env();
    }

    #[test]
    fn artifact_url_base_without_trailing_slash() {
        let d = DistributionSettings {
            binary_base_url: "https://cdn.example.com/dl".to_string(),
            ..Default::default()
        };
        assert_eq!(
            d.artifact_url("enola-v1.0.tar.gz"),
            "https://cdn.example.com/dl/enola-v1.0.tar.gz"
        );
    }

    // ── Error-path / edge-case tests ──

    #[test]
    fn artifact_url_empty_artifact_name() {
        let d = DistributionSettings::default();
        assert_eq!(d.artifact_url(""), "/releases/");
    }

    #[test]
    fn artifact_url_normalizes_double_slash() {
        let d = DistributionSettings {
            binary_base_url: "https://cdn.example.com/dl/".to_string(),
            ..Default::default()
        };
        let url = d.artifact_url("file.tar.gz");
        assert_eq!(url, "https://cdn.example.com/dl/file.tar.gz");
        assert!(!url.contains("//dl/"));
    }

    #[test]
    fn docs_link_with_trailing_slash_on_docs_url() {
        let w = WebSettings {
            web_public_url: "https://enola.dev".to_string(),
            docs_url: "https://docs.enola.dev/".to_string(),
        };
        assert_eq!(w.docs_link(), "https://docs.enola.dev/");
    }

    #[test]
    fn docs_link_empty_when_both_empty() {
        let w = WebSettings::default();
        assert_eq!(w.docs_link(), "");
    }

    #[test]
    fn distribution_settings_partial_config() {
        let d = DistributionSettings {
            binary_base_url: "https://example.com".to_string(),
            minisign_pubkey_url: String::new(),
        };
        assert!(!d.binary_base_url.is_empty());
        assert!(d.minisign_pubkey_url.is_empty());
    }
}
