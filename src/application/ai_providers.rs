// Fase 13.2 — Casos de uso `ai provider` (BYOK cloud) + `ask --provider`.
//
// ⚠️ PRIVACIDAD (requisito no negociable del plan): los providers externos
// son el ÚNICO punto donde datos del usuario salen de su máquina. Todo
// envío exige consentimiento previo (prompt TTY o `--accept-risks`) — ver
// `require_external_consent` (fail-closed sin TTY).
//
// Hexagonal: usa SOLO `AiProvidersStorePort` + `ExternalAiPort`.

use crate::domain::ai_providers::{key_status, validate_api_key, ExternalProvider, ProviderEntry};
use crate::domain::error::{EnolaError, Result};
use crate::ports::external_ai::{AiProvidersStorePort, ExternalAiPort, ProviderTestResult};
use crate::ports::llm::ChatMessage;
use serde::Serialize;
use std::io::IsTerminal;
use std::sync::Arc;

/// Vista pública de un provider para `ai provider list`/`status`.
/// La API key JAMÁS se incluye — solo `(set)`/`(not set)`.
#[derive(Debug, Clone, Serialize)]
pub struct ProviderStatusView {
    /// Provider.
    pub provider: String,
    /// Habilitado.
    pub enabled: bool,
    /// "(set)" | "(not set)" — nunca el valor.
    pub api_key: String,
    /// Modelo efectivo.
    pub model: String,
    /// Endpoint efectivo.
    pub endpoint: String,
    /// `true` si puede usarse (habilitado + con clave).
    pub usable: bool,
}

/// Orquestador de `ai provider <sub>` + envío de `ask --provider`.
pub struct ManageAiProviders {
    store: Arc<dyn AiProvidersStorePort + Send + Sync>,
    client: Arc<dyn ExternalAiPort + Send + Sync>,
    /// `true` si el cliente externo sale por SOCKS5h (para el aviso de ruta).
    via_tor: bool,
}

impl ManageAiProviders {
    /// Construye el caso de uso con store + cliente externo inyectados.
    pub fn new(
        store: Arc<dyn AiProvidersStorePort + Send + Sync>,
        client: Arc<dyn ExternalAiPort + Send + Sync>,
        via_tor: bool,
    ) -> Self {
        Self {
            store,
            client,
            via_tor,
        }
    }

    /// `true` si las peticiones externas salen por SOCKS5h.
    pub fn is_via_tor(&self) -> bool {
        self.via_tor
    }

    /// `ai provider list` — estado de todos los providers (sin keys).
    pub async fn list(&self) -> Result<Vec<ProviderStatusView>> {
        let file = self.store.load().await?;
        Ok(ExternalProvider::all()
            .iter()
            .map(|p| {
                let e = file.entries.get(p);
                ProviderStatusView {
                    provider: p.as_str().to_string(),
                    enabled: e.map(|x| x.enabled).unwrap_or(false),
                    api_key: key_status(&e.and_then(|x| x.api_key.clone())).to_string(),
                    model: e
                        .map(|x| x.effective_model(*p))
                        .unwrap_or_else(|| p.default_model().to_string()),
                    endpoint: e
                        .map(|x| x.effective_endpoint(*p))
                        .unwrap_or_else(|| p.default_endpoint().to_string()),
                    usable: e.map(|x| x.is_usable()).unwrap_or(false),
                }
            })
            .collect())
    }

    /// `ai provider status <provider>` — vista de un provider.
    pub async fn status(&self, provider: ExternalProvider) -> Result<ProviderStatusView> {
        let all = self.list().await?;
        all.into_iter()
            .find(|v| v.provider == provider.as_str())
            .ok_or_else(|| EnolaError::NotFound("provider".into()))
    }

    /// `ai provider set <provider>` — configura clave/modelo/endpoint.
    /// `api_key` llega del prompt TTY o de stdin — NUNCA de argv/logs.
    pub async fn set(
        &self,
        provider: ExternalProvider,
        api_key: Option<String>,
        model: Option<String>,
        endpoint: Option<String>,
        enable: bool,
    ) -> Result<()> {
        if let Some(k) = &api_key {
            validate_api_key(k)?; // T9 — inyección de control
        }
        if let Some(ep) = &endpoint {
            if !(ep.starts_with("https://") || ep.starts_with("http://")) {
                return Err(EnolaError::ValidationError(format!(
                    "El endpoint debe ser http(s): '{ep}'"
                )));
            }
        }
        let mut entry = self.store.get(provider).await?.unwrap_or(ProviderEntry {
            enabled: false,
            api_key: None,
            model: None,
            endpoint: None,
        });
        if api_key.is_some() {
            entry.api_key = api_key;
        }
        if model.is_some() {
            entry.model = model;
        }
        if endpoint.is_some() {
            entry.endpoint = endpoint;
        }
        entry.enabled = enable || entry.api_key.is_some();
        self.store.set(provider, entry).await
    }

    /// `ai provider remove <provider> --force` — borra la entrada.
    pub async fn remove(&self, provider: ExternalProvider, force: bool) -> Result<()> {
        if !force {
            return Err(EnolaError::ValidationError(
                "`ai provider remove` borra la clave del disco: requiere --force".into(),
            ));
        }
        if !self.store.remove(provider).await? {
            return Err(EnolaError::NotFound(format!(
                "El provider '{}' no tiene configuración guardada",
                provider.as_str()
            )));
        }
        Ok(())
    }

    /// Resuelve provider usable + endpoint/modelo/key para envío.
    /// Error accionable si falta clave o está deshabilitado.
    async fn usable_entry(&self, provider: ExternalProvider) -> Result<(String, String, String)> {
        let entry = self.store.get(provider).await?.ok_or_else(|| {
            EnolaError::ValidationError(format!(
                "Provider '{}' sin configurar — usa `ai provider set {}`",
                provider.as_str(),
                provider.as_str()
            ))
        })?;
        let key = entry.api_key.clone().ok_or_else(|| {
            EnolaError::ValidationError(format!(
                "Provider '{}' sin API key — usa `ai provider set {}`",
                provider.as_str(),
                provider.as_str()
            ))
        })?;
        if !entry.enabled {
            return Err(EnolaError::ValidationError(format!(
                "Provider '{}' deshabilitado — `ai provider set {}` lo habilita",
                provider.as_str(),
                provider.as_str()
            )));
        }
        Ok((
            key,
            entry.effective_endpoint(provider),
            entry.effective_model(provider),
        ))
    }

    /// `ai provider test <provider>` — ping real al provider.
    /// REQUIERE consentimiento previo (`require_external_consent`).
    pub async fn test(&self, provider: ExternalProvider) -> Result<ProviderTestResult> {
        let (key, endpoint, model) = self.usable_entry(provider).await?;
        self.client
            .test_connection(provider, &endpoint, &model, &key)
            .await
    }

    /// `ask --provider <p>` — chat single-shot SIN tool registry (los
    /// providers cloud no ejecutan comandos enola — menor superficie de
    /// fuga). REQUIERE consentimiento previo.
    pub async fn send_chat(
        &self,
        provider: ExternalProvider,
        prompt: &str,
        system_prompt: Option<&str>,
        model: Option<&str>,
    ) -> Result<String> {
        let (key, endpoint, default_model) = self.usable_entry(provider).await?;
        let model = model
            .filter(|m| !m.trim().is_empty())
            .map(str::to_string)
            .unwrap_or(default_model);
        self.client
            .chat(
                provider,
                &endpoint,
                &model,
                &key,
                &[ChatMessage::user(prompt)],
                system_prompt.map(str::to_string),
            )
            .await
    }
}

/// Gate de privacidad para providers externos (fail-closed).
///
/// - `--accept-risks` (`accept_risks=true`) → consentimiento explícito.
/// - Sin flag + TTY interactivo → `confirm_fn` muestra el aviso y pide `[s/N]`.
/// - Sin flag + sin TTY → **error cerrado** (un script nunca exfiltra sin
///   consentimiento explícito).
///
/// Devuelve `Ok(())` solo con consentimiento.
pub fn require_external_consent(
    provider: ExternalProvider,
    accept_risks: bool,
    confirm_fn: impl FnOnce() -> bool,
) -> Result<()> {
    if accept_risks {
        return Ok(());
    }
    if !std::io::stdin().is_terminal() {
        return Err(EnolaError::ValidationError(format!(
            "El provider externo '{}' enviará datos fuera de esta máquina. \
             Sin TTY interactivo se requiere `--accept-risks` explícito \
             (rechazo por defecto — privacidad).",
            provider.as_str()
        )));
    }
    if confirm_fn() {
        Ok(())
    } else {
        Err(EnolaError::ValidationError(
            "Operación cancelada por el usuario (privacidad de datos)".into(),
        ))
    }
}

/// Resolución de enrutado Tor para llamadas a providers externos.
/// Prioridad: flag `--tor` > env `ENOLA_AI_TOR_PROXY` > `[ai].tor_proxy`.
pub fn resolve_provider_tor(flag_tor: bool) -> bool {
    if flag_tor {
        return true;
    }
    let env = std::env::var("ENOLA_AI_TOR_PROXY").ok();
    let cfg = crate::infrastructure::config_loader::load_security_section(
        "ai",
        crate::infrastructure::config_loader::AI_SECURITY_KEYS,
    );
    for v in [env, cfg.get("tor_proxy").cloned()].into_iter().flatten() {
        let v = v.trim().to_ascii_lowercase();
        if matches!(v.as_str(), "1" | "true" | "yes" | "on") {
            return true;
        }
        if matches!(v.as_str(), "0" | "false" | "no" | "off") {
            return false;
        }
    }
    false
}

/// Aviso de privacidad estándar mostrado en el prompt TTY.
pub fn privacy_notice(provider: ExternalProvider, endpoint: &str) -> String {
    format!(
        "⚠️  PRIVACIDAD: el provider externo '{}' recibirá tus mensajes fuera \
         de esta máquina ({endpoint}). Los datos dejan tu equipo y quedan \
         bajo la política del provider. ¿Continuar? [s/N]",
        provider.as_str()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ai_providers::ExternalProvider as P;
    use crate::domain::ai_providers::ProvidersFile;
    use crate::ports::external_ai::{MockAiProvidersStorePort, MockExternalAiPort};

    fn entry_usable() -> ProviderEntry {
        ProviderEntry {
            enabled: true,
            api_key: Some("test_k".into()),
            model: None,
            endpoint: None,
        }
    }

    #[tokio::test]
    async fn list_shows_status_without_leaking_key() {
        let mut store = MockAiProvidersStorePort::new();
        store.expect_load().returning(|| {
            let mut f = ProvidersFile::default();
            f.entries.insert(P::OpenAi, entry_usable());
            Ok(f)
        });
        let svc =
            ManageAiProviders::new(Arc::new(store), Arc::new(MockExternalAiPort::new()), false);
        let list = svc.list().await.unwrap();
        assert_eq!(list.len(), 5);
        let oai = &list[0];
        assert_eq!(oai.provider, "openai");
        assert!(oai.usable);
        assert_eq!(oai.api_key, "(set)");
        // La serialización no contiene la key
        let json = serde_json::to_string(&list).unwrap();
        assert!(!json.contains("test_k"));
    }

    #[tokio::test]
    async fn set_validates_key_and_persists() {
        let mut store = MockAiProvidersStorePort::new();
        store.expect_get().returning(|_| Ok(None));
        store
            .expect_set()
            .withf(|p, e| *p == P::Groq && e.enabled && e.api_key.is_some())
            .times(1)
            .returning(|_, _| Ok(()));
        let svc =
            ManageAiProviders::new(Arc::new(store), Arc::new(MockExternalAiPort::new()), false);
        svc.set(P::Groq, Some("test_k9".into()), None, None, false)
            .await
            .unwrap();
        // key con control chars → error antes de persistir
        let mut store2 = MockAiProvidersStorePort::new();
        store2.expect_get().returning(|_| Ok(None));
        let svc2 =
            ManageAiProviders::new(Arc::new(store2), Arc::new(MockExternalAiPort::new()), false);
        assert!(svc2
            .set(P::Groq, Some("bad\nkey".into()), None, None, false)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn remove_requires_force_and_delete() {
        let store = MockAiProvidersStorePort::new();
        let svc =
            ManageAiProviders::new(Arc::new(store), Arc::new(MockExternalAiPort::new()), false);
        assert!(svc.remove(P::OpenAi, false).await.is_err());

        let mut store2 = MockAiProvidersStorePort::new();
        store2.expect_remove().returning(|_| Ok(true));
        let svc2 =
            ManageAiProviders::new(Arc::new(store2), Arc::new(MockExternalAiPort::new()), false);
        svc2.remove(P::OpenAi, true).await.unwrap();
    }

    #[tokio::test]
    async fn send_chat_resolves_entry_and_calls_client() {
        let mut store = MockAiProvidersStorePort::new();
        store.expect_get().returning(|_| Ok(Some(entry_usable())));
        let mut client = MockExternalAiPort::new();
        client
            .expect_chat()
            .withf(|p, ep, m, k, _, _| {
                *p == P::OpenAi
                    && ep.contains("api.openai.com")
                    && m == "gpt-4o-mini"
                    && k == "test_k"
            })
            .returning(|_, _, _, _, _, _| Ok("hola".into()));
        let svc = ManageAiProviders::new(Arc::new(store), Arc::new(client), true);
        assert!(svc.is_via_tor());
        let out = svc.send_chat(P::OpenAi, "hi", None, None).await.unwrap();
        assert_eq!(out, "hola");
    }

    #[tokio::test]
    async fn send_chat_without_key_fails_actionable() {
        let mut store = MockAiProvidersStorePort::new();
        store.expect_get().returning(|_| Ok(None));
        let svc =
            ManageAiProviders::new(Arc::new(store), Arc::new(MockExternalAiPort::new()), false);
        let err = svc.send_chat(P::Grok, "hi", None, None).await.unwrap_err();
        assert!(err.to_string().contains("ai provider set"));
    }

    #[test]
    fn consent_gate_accepts_flag_and_rejects_closed() {
        // Con --accept-risks → Ok directo (sin llamar al callback).
        assert!(require_external_consent(P::OpenAi, true, || panic!()).is_ok());
        // Sin flag + sin TTY (stdin de test no es terminal) → error cerrado.
        let err = require_external_consent(P::OpenAi, false, || true).unwrap_err();
        assert!(err.to_string().contains("--accept-risks"));
    }

    #[test]
    fn resolve_provider_tor_flag_overrides_all() {
        assert!(resolve_provider_tor(true));
    }

    #[test]
    fn privacy_notice_mentions_provider_and_endpoint() {
        let n = privacy_notice(P::Anthropic, "https://api.anthropic.com/v1");
        assert!(n.contains("anthropic") && n.contains("anthropic.com"));
    }
}
