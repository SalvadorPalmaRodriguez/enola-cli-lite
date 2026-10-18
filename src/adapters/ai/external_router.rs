//! Fase 13.2 — Router `ExternalAiPort`: despacha por provider al formato
//! correcto (OpenAI-compatible vs Anthropic Messages) sin factories en
//! `application/` (frontera hexagonal limpia).

use crate::domain::ai_providers::ExternalProvider;
use crate::domain::error::Result;
use crate::ports::external_ai::{ExternalAiPort, ProviderTestResult};
use crate::ports::llm::ChatMessage;

/// Adapter único que despacha al formato del provider.
/// `use_tor` enruta todas las peticiones por SOCKS5h.
pub struct ExternalAiAdapter {
    openai: super::openai_compat::OpenAiCompatAdapter,
    anthropic: super::anthropic::AnthropicAdapter,
    via_tor: bool,
}

impl ExternalAiAdapter {
    /// Construye el router; `use_tor` fuerza SOCKS5h en todas las salidas.
    pub fn new(use_tor: bool) -> Result<Self> {
        Ok(Self {
            openai: super::openai_compat::OpenAiCompatAdapter::new(
                "https://api.openai.com/v1",
                use_tor,
            )?,
            anthropic: super::anthropic::AnthropicAdapter::new(
                "https://api.anthropic.com/v1",
                use_tor,
            )?,
            via_tor: use_tor,
        })
    }

    /// `true` si todas las peticiones salen por SOCKS5h.
    pub fn is_via_tor(&self) -> bool {
        self.via_tor
    }
}

#[async_trait::async_trait]
impl ExternalAiPort for ExternalAiAdapter {
    async fn chat(
        &self,
        provider: ExternalProvider,
        endpoint: &str,
        model: &str,
        api_key: &str,
        messages: &[ChatMessage],
        system_prompt: Option<String>,
    ) -> Result<String> {
        if provider.is_openai_compatible() {
            self.openai
                .chat(provider, endpoint, model, api_key, messages, system_prompt)
                .await
        } else {
            self.anthropic
                .chat(provider, endpoint, model, api_key, messages, system_prompt)
                .await
        }
    }

    async fn test_connection(
        &self,
        provider: ExternalProvider,
        endpoint: &str,
        model: &str,
        api_key: &str,
    ) -> Result<ProviderTestResult> {
        if provider.is_openai_compatible() {
            self.openai
                .test_connection(provider, endpoint, model, api_key)
                .await
        } else {
            self.anthropic
                .test_connection(provider, endpoint, model, api_key)
                .await
        }
    }
}
