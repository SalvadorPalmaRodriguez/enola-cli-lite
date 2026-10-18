// Fase 13.2 — Ports para providers IA externos (BYOK cloud).
//
// Segregación (ISP): `LlmEngine` sigue siendo el motor LOCAL (Ollama).
// `ExternalAiPort` envía mensajes a APIs externas; el consentimiento de
// privacidad es responsabilidad del caller (application/cli) — la API key
// JAMÁS se loggea ni aparece en errores/salidas (check 01).

use crate::domain::ai_providers::{ExternalProvider, ProviderEntry, ProvidersFile};
use crate::domain::error::Result;
use crate::ports::llm::ChatMessage;
use serde::Serialize;

/// Resultado de `ai provider test <provider>`.
#[derive(Debug, Clone, Serialize)]
pub struct ProviderTestResult {
    /// Nombre del provider.
    pub provider: String,
    /// Modelo usado en el test.
    pub model: String,
    /// Latencia de la llamada en ms.
    pub latency_ms: u64,
    /// `true` si el provider respondió correctamente.
    pub success: bool,
    /// Error si falló (NUNCA contiene la API key).
    pub error: Option<String>,
}

/// Puerto de comunicación con un provider IA externo.
///
/// Implementaciones: `OpenAiCompatAdapter` (openai/grok/groq/openrouter)
/// y `AnthropicAdapter` (API Messages propia).
#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait ExternalAiPort: Send + Sync {
    /// Envía mensajes de chat al provider y devuelve el texto de respuesta.
    ///
    /// `api_key` viaja solo en la cabecera `Authorization`/`x-api-key` de la
    /// petición; NUNCA se registra ni se incluye en errores.
    async fn chat(
        &self,
        provider: ExternalProvider,
        endpoint: &str,
        model: &str,
        api_key: &str,
        messages: &[ChatMessage],
        system_prompt: Option<String>,
    ) -> Result<String>;

    /// Verifica conectividad + validez de la key con una llamada mínima.
    async fn test_connection(
        &self,
        provider: ExternalProvider,
        endpoint: &str,
        model: &str,
        api_key: &str,
    ) -> Result<ProviderTestResult>;
}

/// Puerto del almacén local de claves/providers (`~/.enola/providers.env`,
/// atómico + `0600`, respetando `effective_home()`/`SUDO_USER`).
#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait AiProvidersStorePort: Send + Sync {
    /// Carga el fichero de providers (vacío si no existe).
    async fn load(&self) -> Result<ProvidersFile>;

    /// Persiste el fichero completo (escritura atómica, `0600`).
    async fn save(&self, file: &ProvidersFile) -> Result<()>;

    /// Devuelve la entrada de un provider (o defaults deshabilitados).
    async fn get(&self, provider: ExternalProvider) -> Result<Option<ProviderEntry>>;

    /// Configura/actualiza una entrada de provider.
    async fn set(&self, provider: ExternalProvider, entry: ProviderEntry) -> Result<()>;

    /// Elimina la entrada de un provider. Devuelve `false` si no existía.
    async fn remove(&self, provider: ExternalProvider) -> Result<bool>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ai_providers::ExternalProvider as P;

    #[tokio::test]
    async fn mock_store_contract() {
        let mut m = MockAiProvidersStorePort::new();
        m.expect_load().returning(|| Ok(ProvidersFile::default()));
        m.expect_get().withf(|p| *p == P::OpenAi).returning(|_| {
            Ok(Some(ProviderEntry {
                enabled: true,
                api_key: Some("test_k".into()),
                model: None,
                endpoint: None,
            }))
        });
        assert!(m.load().await.unwrap().entries.is_empty());
        let e = m.get(P::OpenAi).await.unwrap().unwrap();
        assert!(e.is_usable());
    }

    #[tokio::test]
    async fn mock_external_port_contract() {
        let mut m = MockExternalAiPort::new();
        m.expect_chat()
            .returning(|_, _, _, _, _, _| Ok("hi".into()));
        let r = m
            .chat(P::Groq, "https://x", "m", "k", &[], None)
            .await
            .unwrap();
        assert_eq!(r, "hi");
    }
}
