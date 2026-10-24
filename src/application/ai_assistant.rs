//! Fase 9 (9.3) — Orquestador `AiAssistant` para `enola-cli ask`.
//!
//! Conecta un `LlmEngine` (Ollama local) con el `ToolRegistry` compartido con MCP:
//! - Oculta las meta-propiedades MCP (`confirm_token`, `try_run`) en los schemas
//!   enviados al LLM local (R11).
//! - Clasifica cada tool antes de ejecutar (`registry.kind_of` para tools directas
//!   y `domain::tool::classify(family, &tokens)` para `enola_run_command`).
//! - `ReadOnly`: ejecuta directamente.
//! - `Mutating` / `Destructive`:
//!   - Con `no_exec: true` (`--no-exec`): simula mediante `registry.try_call` sin
//!     efectos secundarios y devuelve el plan/argv.
//!   - Con `no_exec: false`: exige TTY interactivo; `--yes` solo autoriza
//!     `Mutating`, nunca `Destructive` (D7). Sin TTY falla cerrado igual que
//!     `N-08`. Muestra argumentos saneados (`sanitize_args` /
//!     `redact_argv`) con aviso reforzado si es `Destructive`, y pide `[s/N]`.
//!   - Tras la aprobación humana, ejecuta en un único turno del modelo, emitiendo
//!     y adjuntando internamente el `confirm_token` del `ConfirmationStore` cuando
//!     `enola_run_command` apunta a una ruta `Destructive` (R18).

use crate::application::mcp::audit::sanitize_args;
use crate::application::mcp::confirmation::ConfirmationStore;
use crate::application::mcp::registry::ToolRegistry;
use crate::application::mcp::run_command::{policy_name, preview_redacted_argv};
use crate::domain::error::{EnolaError, Result};
use crate::domain::tool::{classify, truncate_output, ToolKind};
use crate::ports::container::ContainerInfo;
use crate::ports::llm::{ChatMessage, LlmEngine, LlmToolDefinition, MCP_SCHEMA_META_KEYS};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

/// Endpoint local por defecto de Ollama (`[ai].endpoint`).
pub(crate) const DEFAULT_AI_ENDPOINT: &str = "http://127.0.0.1:11434";

/// Modelo por defecto de Ollama (`[ai].model`).
pub(crate) const DEFAULT_AI_MODEL: &str = "llama3.1:8b";

/// Timeout por defecto en segundos para peticiones a Ollama (`[ai].timeout_secs`).
pub(crate) const DEFAULT_AI_TIMEOUT_SECS: u64 = 120;

/// Configuración efectiva de `[ai]` tras resolver `flag > env > config.toml > default`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AiConfig {
    pub(crate) endpoint: String,
    pub(crate) model: String,
    pub(crate) timeout_secs: u64,
    /// `true` si las descargas de modelos (`ai model download`) deben salir
    /// por el proxy Tor SOCKS5h (R22). Default: `false` (clearnet).
    pub(crate) use_tor_for_downloads: bool,
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            endpoint: DEFAULT_AI_ENDPOINT.to_string(),
            model: DEFAULT_AI_MODEL.to_string(),
            timeout_secs: DEFAULT_AI_TIMEOUT_SECS,
            use_tor_for_downloads: false,
        }
    }
}

/// Overrides procedentes de flags CLI (`--endpoint`, `--model`, `--tor`).
#[derive(Debug, Default, Clone)]
pub(crate) struct AiFlagOverrides {
    pub(crate) endpoint: Option<String>,
    pub(crate) model: Option<String>,
    pub(crate) timeout_secs: Option<u64>,
    /// `--tor` de `ai model download` (solo `Some(true)` fuerza Tor).
    pub(crate) use_tor_for_downloads: Option<bool>,
}

fn non_empty_trimmed(val: Option<&str>) -> Option<String> {
    val.map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
}

fn env_non_empty(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .and_then(|v| non_empty_trimmed(Some(&v)))
}

fn parse_bool(val: Option<&str>) -> Option<bool> {
    val.map(str::trim)
        .and_then(|s| match s.to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Some(true),
            "0" | "false" | "no" | "off" => Some(false),
            _ => None,
        })
}

fn parse_positive_u64(val: Option<&str>) -> Option<u64> {
    val.and_then(|s| s.trim().parse::<u64>().ok())
        .filter(|&n| n >= 1)
}

/// Resuelve la configuración efectiva de `[ai]` según la prioridad:
/// `flag > env (ENOLA_AI_ENDPOINT, ENOLA_AI_MODEL, ENOLA_AI_TIMEOUT_SECS) > ~/.enola/config.toml [ai] > default`.
pub(crate) fn resolve_ai_config(
    flags: &AiFlagOverrides,
    config_section: &HashMap<String, String>,
) -> AiConfig {
    let endpoint = non_empty_trimmed(flags.endpoint.as_deref())
        .or_else(|| env_non_empty("ENOLA_AI_ENDPOINT"))
        .or_else(|| non_empty_trimmed(config_section.get("endpoint").map(String::as_str)))
        .unwrap_or_else(|| DEFAULT_AI_ENDPOINT.to_string())
        .trim_end_matches('/')
        .to_string();

    let model = non_empty_trimmed(flags.model.as_deref())
        .or_else(|| env_non_empty("ENOLA_AI_MODEL"))
        .or_else(|| non_empty_trimmed(config_section.get("model").map(String::as_str)))
        .unwrap_or_else(|| DEFAULT_AI_MODEL.to_string());

    let timeout_secs = flags
        .timeout_secs
        .filter(|&n| n >= 1)
        .or_else(|| {
            std::env::var("ENOLA_AI_TIMEOUT_SECS")
                .ok()
                .and_then(|v| parse_positive_u64(Some(&v)))
        })
        .or_else(|| parse_positive_u64(config_section.get("timeout_secs").map(String::as_str)))
        .unwrap_or(DEFAULT_AI_TIMEOUT_SECS);

    let use_tor_for_downloads = flags
        .use_tor_for_downloads
        .or_else(|| parse_bool(env_non_empty("ENOLA_AI_USE_TOR_DOWNLOADS").as_deref()))
        .or_else(|| {
            parse_bool(
                config_section
                    .get("use_tor_for_downloads")
                    .map(String::as_str),
            )
        })
        .unwrap_or(false);

    AiConfig {
        endpoint,
        model,
        timeout_secs,
        use_tor_for_downloads,
    }
}

/// Extrae el puerto host mapeado al puerto interno de Ollama (`11434/tcp`) o el
/// primer puerto TCP publicado del contenedor.
fn extract_ollama_host_port(ports: &[String]) -> u16 {
    for entry in ports {
        // Formatos típicos de BollardDockerAdapter:
        // "127.0.0.1:11435->11434/tcp", "0.0.0.0:11434->11434/tcp", "11435->11434/tcp"
        if let Some((host_part, priv_part)) = entry.split_once("->") {
            let is_ollama_port = priv_part.starts_with("11434/") || priv_part == "11434";
            let port_str = host_part.rsplit(':').next().unwrap_or(host_part);
            if let Ok(port) = port_str.trim().parse::<u16>() {
                if is_ollama_port || ports.len() == 1 {
                    return port;
                }
            }
        }
    }
    11434
}

/// Resuelve el endpoint HTTP local de una instancia Ollama en contenedores Docker:
/// - Con `instance = Some(name)`: busca `enola-ai-<name>-ollama`, `enola-ollama-<name>`,
///   `enola-ai-<name>`, `<name>-ollama` o `<name>`; falla con error claro si no existe.
/// - Con `instance = None`: autodetección de cualquier contenedor `enola-ai-*-ollama`,
///   `enola-ollama-*` o `piloto-ollama` en ejecución.
pub(crate) fn resolve_instance_endpoint(
    containers: &[ContainerInfo],
    instance: Option<&str>,
) -> Result<Option<String>> {
    if let Some(raw_name) = instance {
        let name = raw_name.trim();
        // Por prioridad: el contenedor canónico de `ai create` primero; el resto
        // son formas legacy/externas. Se recorren los candidatos (no los
        // contenedores) para que el orden de `docker ps` no decida.
        let candidates = [
            format!("enola-ai-{name}-ollama"),
            format!("enola-ollama-{name}"),
            format!("enola-ai-{name}"),
            format!("{name}-ollama"),
            name.to_string(),
        ];
        if let Some(found) = candidates
            .iter()
            .find_map(|cand| containers.iter().find(|c| c.name == *cand))
        {
            let port = extract_ollama_host_port(&found.ports);
            return Ok(Some(format!("http://127.0.0.1:{port}")));
        }
        return Err(EnolaError::NotFound(format!(
            "No se encontró ningún contenedor Ollama para la instancia '{name}'. \
             Verifica con `docker ps` o crea una instancia con `sudo enola-cli ai create --name {name}` \
             (o pasa `--endpoint http://127.0.0.1:<puerto>`)."
        )));
    }

    // Autodescubrimiento determinista: clase (gestionada `enola-ai-*-ollama` →
    // legacy `enola-ollama-*` → externa `piloto-ollama`) y, dentro de la clase,
    // nombre alfabético. Nunca el orden arbitrario de `docker ps`.
    let class = |name: &str| -> Option<u8> {
        if name.starts_with("enola-ai-") && name.ends_with("-ollama") {
            Some(0)
        } else if name.starts_with("enola-ollama-") {
            Some(1)
        } else if name == "piloto-ollama" {
            Some(2)
        } else {
            None
        }
    };
    if let Some(found) = containers
        .iter()
        .filter_map(|c| class(&c.name).map(|k| (k, c)))
        .min_by(|(ka, a), (kb, b)| ka.cmp(kb).then_with(|| a.name.cmp(&b.name)))
        .map(|(_, c)| c)
    {
        let port = extract_ollama_host_port(&found.ports);
        return Ok(Some(format!("http://127.0.0.1:{port}")));
    }

    Ok(None)
}

/// Formatea un mensaje accionable cuando el endpoint de Ollama no responde,
/// guiando a instalar Ollama localmente o usar `enola-cli ai create` (y mencionando
/// contenedores detectados si existen).
pub(crate) fn format_unreachable_ollama_guidance(
    endpoint: &str,
    discovered_endpoint: Option<&str>,
    cause: &str,
) -> String {
    let mut msg = format!(
        "No se pudo conectar con el motor de IA local (Ollama) en '{endpoint}' ({cause}).\n\n\
         Cómo solucionarlo:\n\
         1. Si ya tienes Ollama instalado en el host, arráncalo:\n\
            ollama serve\n\
            ollama pull {DEFAULT_AI_MODEL}\n\
         2. Para instalar Ollama en el host:\n\
            curl -fsSL https://ollama.com/install.sh | sh\n\
         3. O despliega una instancia aislada con Enola CLI:\n\
            sudo enola-cli ai create\n\
         4. Si Ollama escucha en otra URL o puerto, indícalo con:\n\
            enola-cli ask \"...\" --endpoint http://127.0.0.1:<puerto>\n\
            (o configura ENOLA_AI_ENDPOINT / [ai].endpoint en ~/.enola/config.toml)."
    );
    if let Some(disc) = discovered_endpoint {
        if disc != endpoint {
            msg.push_str(&format!(
                "\n\n💡 Se detectó un contenedor Ollama activo en '{disc}'. \
                 Prueba con `--endpoint {disc}` o `--instance <nombre>`."
            ));
        }
    }
    msg
}

/// Límite por defecto de bytes por salida de tool inyectada al contexto del LLM.
pub(crate) const DEFAULT_MAX_TOOL_OUTPUT_BYTES: usize = 16 * 1024;

/// Máximo de turnos de bucle `chat → tools` por petición `ask`.
pub(crate) const DEFAULT_MAX_TURNS: usize = 8;

/// Decisión del gate de confirmación para operaciones `Mutating` / `Destructive`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GateDecision {
    /// Operación aprobada (`s`/`y` en TTY o `--yes` explícito).
    Approved,
    /// Operación rechazada por el usuario en el prompt `[s/N]`.
    RejectedByUser,
    /// Sin TTY interactivo (y sin `--yes` si es `Mutating`): falla cerrado (N-08, D7).
    NoTty,
}

/// Gate de confirmación inyectable para poder probar `AiAssistant` sin TTY real.
pub(crate) trait ConfirmationGate: Send + Sync {
    /// Solicita confirmación al operador para ejecutar `display_tool` de clase `kind`
    /// con el resumen saneado `sanitized_summary`.
    fn confirm(&self, display_tool: &str, kind: ToolKind, sanitized_summary: &str) -> GateDecision;
}

/// Gate de confirmación real sobre TTY (`stdin` / `stderr`).
#[derive(Debug, Clone, Copy)]
pub(crate) struct TtyConfirmationGate {
    /// Si es `true` (`--yes`), aprueba `Mutating` sin preguntar (nunca `Destructive`, D7).
    pub(crate) assume_yes: bool,
}

impl TtyConfirmationGate {
    pub(crate) fn new(assume_yes: bool) -> Self {
        Self { assume_yes }
    }
}

/// Parsea la respuesta del usuario al prompt `[s/N]`.
pub(crate) fn parse_confirmation_answer(input: &str) -> bool {
    matches!(
        input.trim().to_lowercase().as_str(),
        "s" | "si" | "sí" | "y" | "yes"
    )
}

/// Decisión del gate que no necesita preguntar (o `None` si hay que mostrar
/// el prompt `[s/N]`).
///
/// D7: `--yes` solo aprueba `Mutating`. Una `Destructive` la elige el modelo y
/// puede venir inducida por contenido de herramientas (RAG, logs): exige
/// siempre la confirmación humana en TTY, con o sin `--yes`.
pub(crate) fn decide_without_prompt(
    assume_yes: bool,
    kind: ToolKind,
    is_tty: bool,
) -> Option<GateDecision> {
    match kind {
        ToolKind::ReadOnly => Some(GateDecision::Approved),
        ToolKind::Mutating if assume_yes => Some(GateDecision::Approved),
        _ if !is_tty => Some(GateDecision::NoTty),
        _ => None,
    }
}

impl ConfirmationGate for TtyConfirmationGate {
    fn confirm(&self, display_tool: &str, kind: ToolKind, sanitized_summary: &str) -> GateDecision {
        #[allow(unsafe_code)]
        let is_tty = unsafe { libc::isatty(libc::STDIN_FILENO) != 0 };
        if let Some(decision) = decide_without_prompt(self.assume_yes, kind, is_tty) {
            return decision;
        }
        match kind {
            ToolKind::Destructive => {
                eprintln!(
                    "\n⚠️  [DESTRUCTIVO / IRREVERSIBLE] El asistente solicita ejecutar `{display_tool}`:"
                );
                eprintln!("   {sanitized_summary}");
                eprintln!(
                    "   ATENCIÓN: Esta operación puede eliminar datos/servicios, exponer red o modificar defensas."
                );
            }
            ToolKind::Mutating => {
                eprintln!("\n🔒 [MUTACIÓN] El asistente solicita ejecutar `{display_tool}`:");
                eprintln!("   {sanitized_summary}");
            }
            ToolKind::ReadOnly => return GateDecision::Approved,
        }
        eprint!("¿Confirmas la ejecución? [s/N]: ");
        let mut line = String::new();
        if std::io::stdin().read_line(&mut line).is_ok() && parse_confirmation_answer(&line) {
            GateDecision::Approved
        } else {
            GateDecision::RejectedByUser
        }
    }
}

/// Opciones de ejecución de una consulta `ask`.
#[derive(Debug, Clone)]
pub(crate) struct AskOptions {
    /// Nombre del modelo en Ollama (p.ej. `llama3.1:8b`).
    pub(crate) model: String,
    /// Si es `true` (`--no-exec`), simula mutaciones con `try_call` sin ejecutarlas.
    pub(crate) no_exec: bool,
    /// Límite de bytes de salida por tool antes de truncar.
    pub(crate) max_tool_output_bytes: usize,
    /// Máximo de iteraciones del bucle `chat → tools`.
    pub(crate) max_turns: usize,
}

impl Default for AskOptions {
    fn default() -> Self {
        Self {
            model: "llama3.1:8b".to_string(),
            no_exec: false,
            max_tool_output_bytes: DEFAULT_MAX_TOOL_OUTPUT_BYTES,
            max_turns: DEFAULT_MAX_TURNS,
        }
    }
}

/// Prompt de sistema para el asistente local de `enola-cli ask`.
const SYSTEM_PROMPT: &str = "\
You are the local AI assistant built into enola-cli. \
Operate the user's infrastructure exclusively through the provided tools. \
Never invent command names, flags, ports, or .onion addresses. \
Respond concisely in the same language as the user's prompt.";

/// Elimina las meta-keys de MCP (`confirm_token`, `try_run`) si un modelo local
/// intentara enviarlas por su cuenta.
fn strip_meta_args(args: &Value) -> Value {
    match args {
        Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (k, v) in map {
                if !MCP_SCHEMA_META_KEYS.contains(&k.as_str()) {
                    out.insert(k.clone(), v.clone());
                }
            }
            Value::Object(out)
        }
        other => other.clone(),
    }
}

/// Orquestador del comando `enola-cli ask`.
pub(crate) struct AiAssistant {
    engine: Arc<dyn LlmEngine>,
    registry: Arc<ToolRegistry>,
    confirmations: Arc<ConfirmationStore>,
    gate: Arc<dyn ConfirmationGate>,
}

impl AiAssistant {
    pub(crate) fn new(
        engine: Arc<dyn LlmEngine>,
        registry: Arc<ToolRegistry>,
        confirmations: Arc<ConfirmationStore>,
        gate: Arc<dyn ConfirmationGate>,
    ) -> Self {
        Self {
            engine,
            registry,
            confirmations,
            gate,
        }
    }

    /// Ejecuta el bucle `chat → tools → respuesta` para `prompt`.
    #[tracing::instrument(skip(self, prompt, options), fields(model = %options.model, no_exec = options.no_exec))]
    pub(crate) async fn ask(&self, prompt: &str, options: &AskOptions) -> Result<String> {
        let specs = self.registry.list_tools();
        let available_names: Vec<String> = specs.iter().map(|s| s.name.clone()).collect();
        let llm_tools: Vec<LlmToolDefinition> = specs
            .iter()
            .map(LlmToolDefinition::from_tool_spec)
            .collect();

        let mut messages = vec![
            ChatMessage::system(SYSTEM_PROMPT),
            ChatMessage::user(prompt),
        ];
        let max_turns = options.max_turns.max(1);

        for _turn in 0..max_turns {
            let resp = self
                .engine
                .chat_tools(&options.model, &messages, &llm_tools)
                .await?;

            if resp.tool_calls.is_empty() {
                return Ok(resp.content);
            }

            messages.push(ChatMessage::assistant_tools(
                resp.content.clone(),
                resp.tool_calls.clone(),
            ));

            for tc in resp.tool_calls {
                let clean_args = strip_meta_args(&tc.arguments);

                // 1. Verificar que la tool existe en el registry (si el modelo alucina
                //    un nombre, devolver error recuperable para que re-pregunte).
                let Some(base_kind) = self.registry.kind_of(&tc.name) else {
                    let err_payload = json!({
                        "error": format!(
                            "unknown tool '{}'. Available tools: {}",
                            tc.name,
                            available_names.join(", ")
                        )
                    });
                    messages.push(ChatMessage::tool_result(&tc.name, err_payload.to_string()));
                    continue;
                };

                // 2. Validar argumentos contra el JSON Schema antes de pedir confirmación
                //    o despachar.
                if let Err(e) = self.registry.validate(&tc.name, &clean_args) {
                    let err_payload = json!({ "error": e.to_string() });
                    messages.push(ChatMessage::tool_result(&tc.name, err_payload.to_string()));
                    continue;
                }

                // 3. Clasificar ToolKind efectivo (para `enola_run_command`, según la
                //    ruta `family` + `subcommand`).
                let (effective_kind, display_tool, run_cmd_scope, sanitized_summary) = if tc.name
                    == "enola_run_command"
                {
                    let family = clean_args
                        .get("family")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let sub = clean_args
                        .get("subcommand")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let tokens: Vec<String> = sub.split_whitespace().map(str::to_string).collect();
                    let token_refs: Vec<&str> = tokens.iter().map(String::as_str).collect();
                    let kind = classify(family, &token_refs);
                    let scope = policy_name(family, &tokens);

                    // Validar ruta/flags de enola_run_command antes del prompt humano.
                    let argv_preview = match preview_redacted_argv(&clean_args) {
                        Ok(argv) => argv,
                        Err(e) => {
                            let err_payload = json!({ "error": e.to_string() });
                            messages
                                .push(ChatMessage::tool_result(&tc.name, err_payload.to_string()));
                            continue;
                        }
                    };
                    let summary = format!(
                        "argv={:?} args={}",
                        argv_preview,
                        sanitize_args(&clean_args)
                    );
                    (
                        kind,
                        format!("enola_run_command ({scope})"),
                        Some(scope),
                        summary,
                    )
                } else {
                    let summary = sanitize_args(&clean_args).to_string();
                    (base_kind, tc.name.clone(), None, summary)
                };

                // 4. Despachar según clase y flag `--no-exec`.
                let outcome = match effective_kind {
                    ToolKind::ReadOnly => self.registry.call(&tc.name, clean_args).await,
                    ToolKind::Mutating | ToolKind::Destructive if options.no_exec => {
                        self.registry.try_call(&tc.name, clean_args).await
                    }
                    ToolKind::Mutating | ToolKind::Destructive => {
                        match self
                            .gate
                            .confirm(&display_tool, effective_kind, &sanitized_summary)
                        {
                            GateDecision::NoTty => {
                                let hint = if effective_kind == ToolKind::Destructive {
                                    "Es destructiva: --yes no la cubre. Ejecuta en una terminal \
                                     interactiva o usa --no-exec para simular."
                                } else {
                                    "Ejecuta en una terminal interactiva, pasa --yes para \
                                     autorizar explícitamente o usa --no-exec para simular."
                                };
                                return Err(EnolaError::SecurityError(format!(
                                    "La operación '{display_tool}' modifica el sistema y requiere \
                                     confirmación interactiva (TTY). {hint}"
                                )));
                            }
                            GateDecision::RejectedByUser => {
                                let rejected = json!({
                                    "rejected": true,
                                    "error": format!(
                                        "operation '{}' was rejected by the user at the confirmation prompt",
                                        display_tool
                                    )
                                });
                                messages
                                    .push(ChatMessage::tool_result(&tc.name, rejected.to_string()));
                                continue;
                            }
                            GateDecision::Approved => {
                                let mut exec_args = clean_args;
                                // R18: si la llamada va por `enola_run_command` sobre una ruta
                                // `Destructive`, tras el `[s/N]` del humano emitimos y adjuntamos
                                // internamente el `confirm_token` del `ConfirmationStore` para
                                // ejecutar en un solo turno del LLM.
                                if effective_kind == ToolKind::Destructive {
                                    if let Some(ref scope) = run_cmd_scope {
                                        let token = self.confirmations.create(scope, &exec_args)?;
                                        if let Some(obj) = exec_args.as_object_mut() {
                                            obj.insert("confirm_token".to_string(), json!(token));
                                        }
                                    }
                                }
                                self.registry.call(&tc.name, exec_args).await
                            }
                        }
                    }
                };

                // 5. Formatear y truncar el resultado de la tool antes de añadirlo al historial.
                let raw_result = match outcome {
                    Ok(val) => val.to_string(),
                    Err(e) => json!({ "error": e.to_string() }).to_string(),
                };
                let (mut text, truncated) =
                    truncate_output(&raw_result, options.max_tool_output_bytes);
                if truncated {
                    text.push_str("...[truncated]");
                }
                messages.push(ChatMessage::tool_result(&tc.name, text));
            }
        }

        Err(EnolaError::ValidationError(format!(
            "El asistente alcanzó el límite de {} turnos de herramientas sin respuesta final",
            max_turns
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::mcp::audit::AuditLogger;
    use crate::application::mcp::policy::McpPolicy;
    use crate::application::mcp::registry::ToolHandler;
    use crate::application::mcp::run_command;
    use crate::domain::tool::ToolSpec;
    use crate::ports::llm::{LlmChatResponse, LlmToolCall, MockLlmEngine};
    use crate::ports::process_runner::{MockProcessRunnerPort, ProcessOutput};
    use serde_json::{json, Value};
    use std::sync::Mutex;

    #[test]
    fn yes_approves_mutating_but_never_destructive() {
        // D7: `--yes` solo cubre `Mutating`; `Destructive` exige siempre TTY.
        assert_eq!(
            decide_without_prompt(true, ToolKind::Mutating, false),
            Some(GateDecision::Approved)
        );
        assert_eq!(
            decide_without_prompt(true, ToolKind::Destructive, false),
            Some(GateDecision::NoTty)
        );
        // Con TTY, `--yes` no salta el prompt de una destructiva.
        assert_eq!(
            decide_without_prompt(true, ToolKind::Destructive, true),
            None
        );
        // ReadOnly nunca pregunta; sin `--yes` y sin TTY, falla cerrado.
        assert_eq!(
            decide_without_prompt(false, ToolKind::ReadOnly, false),
            Some(GateDecision::Approved)
        );
        assert_eq!(
            decide_without_prompt(false, ToolKind::Mutating, false),
            Some(GateDecision::NoTty)
        );
        assert_eq!(decide_without_prompt(false, ToolKind::Mutating, true), None);
    }

    struct FixedGate {
        decision: GateDecision,
        recorded: Mutex<Vec<(String, ToolKind, String)>>,
    }

    impl FixedGate {
        fn new(decision: GateDecision) -> Arc<Self> {
            Arc::new(Self {
                decision,
                recorded: Mutex::new(Vec::new()),
            })
        }
    }

    impl ConfirmationGate for FixedGate {
        fn confirm(
            &self,
            display_tool: &str,
            kind: ToolKind,
            sanitized_summary: &str,
        ) -> GateDecision {
            self.recorded.lock().unwrap().push((
                display_tool.to_string(),
                kind,
                sanitized_summary.to_string(),
            ));
            self.decision
        }
    }

    struct SpyTool {
        calls: Mutex<Vec<Value>>,
        try_calls: Mutex<Vec<Value>>,
        response: Value,
    }

    impl SpyTool {
        fn new(response: Value) -> Arc<Self> {
            Arc::new(Self {
                calls: Mutex::new(Vec::new()),
                try_calls: Mutex::new(Vec::new()),
                response,
            })
        }
    }

    #[async_trait::async_trait]
    impl ToolHandler for SpyTool {
        async fn call(&self, args: Value) -> Result<Value> {
            self.calls.lock().unwrap().push(args);
            Ok(self.response.clone())
        }

        async fn try_run(&self, args: Value) -> Result<Value> {
            self.try_calls.lock().unwrap().push(args.clone());
            Ok(json!({
                "simulated": true,
                "reliable": true,
                "plan": { "would_run_with": args }
            }))
        }
    }

    fn register_tool(
        reg: &mut ToolRegistry,
        name: &str,
        kind: ToolKind,
        schema: Value,
        handler: Arc<dyn ToolHandler>,
    ) {
        let spec = ToolSpec::new(name, &format!("Desc for {name}"), schema, kind).unwrap();
        reg.register(spec, handler).unwrap();
    }

    #[tokio::test]
    async fn single_readonly_tool_call_executes_without_prompt_and_returns_answer() {
        let mut reg = ToolRegistry::new();
        let spy = SpyTool::new(json!({"services": [{"name": "blog", "onion": "abc.onion"}]}));
        register_tool(
            &mut reg,
            "tor_list",
            ToolKind::ReadOnly,
            json!({"type": "object", "properties": {}, "additionalProperties": false}),
            spy.clone(),
        );

        let mut engine = MockLlmEngine::new();
        let mut turn = 0usize;
        engine
            .expect_chat_tools()
            .times(2)
            .returning(move |_, msgs, _| {
                turn += 1;
                if turn == 1 {
                    Ok(LlmChatResponse {
                        content: String::new(),
                        tool_calls: vec![LlmToolCall {
                            name: "tor_list".to_string(),
                            arguments: json!({}),
                        }],
                        ..Default::default()
                    })
                } else {
                    assert!(
                        msgs.iter().any(|m| m.content.contains("abc.onion")),
                        "el segundo turno debe incluir el resultado de tor_list"
                    );
                    Ok(LlmChatResponse {
                        content: "Tienes 1 servicio Tor activo: blog (abc.onion).".to_string(),
                        tool_calls: vec![],
                        ..Default::default()
                    })
                }
            });

        let gate = FixedGate::new(GateDecision::RejectedByUser);
        let assistant = AiAssistant::new(
            Arc::new(engine),
            Arc::new(reg),
            Arc::new(ConfirmationStore::new()),
            gate.clone(),
        );

        let out = assistant
            .ask("lista mis servicios tor", &AskOptions::default())
            .await
            .unwrap();
        assert!(out.contains("abc.onion"));
        assert_eq!(spy.calls.lock().unwrap().len(), 1);
        assert!(
            gate.recorded.lock().unwrap().is_empty(),
            "ReadOnly no debe pasar por el gate de confirmación"
        );
    }

    #[tokio::test]
    async fn two_chained_readonly_tool_calls_execute_in_sequence() {
        let mut reg = ToolRegistry::new();
        let wp_list_spy = SpyTool::new(json!({"sites": ["miblog"]}));
        let wp_status_spy = SpyTool::new(json!({"name": "miblog", "running": true}));
        register_tool(
            &mut reg,
            "wp_list",
            ToolKind::ReadOnly,
            json!({"type": "object", "properties": {}, "additionalProperties": false}),
            wp_list_spy.clone(),
        );
        register_tool(
            &mut reg,
            "wp_status",
            ToolKind::ReadOnly,
            json!({
                "type": "object",
                "properties": {"name": {"type": "string"}},
                "required": ["name"],
                "additionalProperties": false
            }),
            wp_status_spy.clone(),
        );

        let mut engine = MockLlmEngine::new();
        let mut turn = 0usize;
        engine
            .expect_chat_tools()
            .times(3)
            .returning(move |_, _, _| {
                turn += 1;
                match turn {
                    1 => Ok(LlmChatResponse {
                        content: String::new(),
                        tool_calls: vec![LlmToolCall {
                            name: "wp_list".to_string(),
                            arguments: json!({}),
                        }],
                        ..Default::default()
                    }),
                    2 => Ok(LlmChatResponse {
                        content: String::new(),
                        tool_calls: vec![LlmToolCall {
                            name: "wp_status".to_string(),
                            arguments: json!({"name": "miblog"}),
                        }],
                        ..Default::default()
                    }),
                    _ => Ok(LlmChatResponse {
                        content: "El sitio miblog está en ejecución.".to_string(),
                        tool_calls: vec![],
                        ..Default::default()
                    }),
                }
            });

        let assistant = AiAssistant::new(
            Arc::new(engine),
            Arc::new(reg),
            Arc::new(ConfirmationStore::new()),
            FixedGate::new(GateDecision::Approved),
        );
        let answer = assistant
            .ask("comprueba mis wordpress", &AskOptions::default())
            .await
            .unwrap();
        assert_eq!(answer, "El sitio miblog está en ejecución.");
        assert_eq!(wp_list_spy.calls.lock().unwrap().len(), 1);
        assert_eq!(wp_status_spy.calls.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn mutating_tool_call_rejected_by_user_does_not_invoke_handler_and_informs_model() {
        let mut reg = ToolRegistry::new();
        let wp_create_spy = SpyTool::new(json!({"created": true}));
        register_tool(
            &mut reg,
            "wp_create",
            ToolKind::Mutating,
            json!({
                "type": "object",
                "properties": {
                    "name": {"type": "string"},
                    "db_password": {"type": "string"}
                },
                "required": ["name"],
                "additionalProperties": false
            }),
            wp_create_spy.clone(),
        );

        let mut engine = MockLlmEngine::new();
        let mut turn = 0usize;
        engine
            .expect_chat_tools()
            .times(2)
            .returning(move |_, msgs, _| {
                turn += 1;
                if turn == 1 {
                    Ok(LlmChatResponse {
                        content: String::new(),
                        tool_calls: vec![LlmToolCall {
                            name: "wp_create".to_string(),
                            arguments: json!({"name": "blog1", "db_password": "my-secret-value"}),
                        }],
                        ..Default::default()
                    })
                } else {
                    let last = msgs.last().unwrap();
                    assert!(
                        last.content.contains("rejected") || last.content.contains("cancel"),
                        "debe informar al modelo del rechazo del usuario: {}",
                        last.content
                    );
                    Ok(LlmChatResponse {
                        content: "Entendido, he cancelado la creación de blog1.".to_string(),
                        tool_calls: vec![],
                        ..Default::default()
                    })
                }
            });

        let gate = FixedGate::new(GateDecision::RejectedByUser);
        let assistant = AiAssistant::new(
            Arc::new(engine),
            Arc::new(reg),
            Arc::new(ConfirmationStore::new()),
            gate.clone(),
        );
        let out = assistant
            .ask("crea un wordpress blog1", &AskOptions::default())
            .await
            .unwrap();
        assert!(out.contains("cancelado"));
        assert!(
            wp_create_spy.calls.lock().unwrap().is_empty(),
            "el handler NO debe invocarse si el usuario responde N"
        );
        let recorded = gate.recorded.lock().unwrap();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].1, ToolKind::Mutating);
        assert!(
            !recorded[0].2.contains("my-secret-value"),
            "los secretos deben redactarse al mostrarlos en el gate: {}",
            recorded[0].2
        );
        assert!(recorded[0].2.contains("[REDACTED]"));
    }

    #[tokio::test]
    async fn destructive_run_command_approved_by_user_executes_in_single_llm_turn() {
        let mut reg = ToolRegistry::new();
        let confirmations = Arc::new(ConfirmationStore::new());
        let policy = Arc::new(McpPolicy {
            allow_write: true,
            ..Default::default()
        });
        let audit = Arc::new(AuditLogger::new(None));

        let mut runner = MockProcessRunnerPort::new();
        runner.expect_run().times(1).returning(|argv, _| {
            assert!(argv.contains(&"tor".to_string()));
            assert!(argv.contains(&"remove".to_string()));
            Ok(ProcessOutput {
                exit_code: 0,
                stdout: r#"{"removed":"old-svc"}"#.to_string(),
                stderr: String::new(),
            })
        });

        run_command::register(
            &mut reg,
            Arc::new(runner),
            policy,
            audit,
            confirmations.clone(),
        )
        .unwrap();

        let mut engine = MockLlmEngine::new();
        let mut turn = 0usize;
        engine
            .expect_chat_tools()
            .times(2)
            .returning(move |_, msgs, tools| {
                turn += 1;
                // R11: verificar que confirm_token y try_run NO aparecen en los schemas enviados al LLM.
                for t in tools {
                    if let Some(props) = t.parameters.get("properties").and_then(|p| p.as_object())
                    {
                        assert!(!props.contains_key("confirm_token"));
                        assert!(!props.contains_key("try_run"));
                    }
                }
                if turn == 1 {
                    Ok(LlmChatResponse {
                        content: String::new(),
                        tool_calls: vec![LlmToolCall {
                            name: "enola_run_command".to_string(),
                            arguments: json!({
                                "family": "tor",
                                "subcommand": "remove",
                                "args": { "name": "old-svc", "force": true }
                            }),
                        }],
                        ..Default::default()
                    })
                } else {
                    let last = msgs.last().unwrap();
                    assert!(
                        last.content.contains("old-svc")
                            && !last.content.contains("confirmation_required"),
                        "R18: debe haberse ejecutado en un solo turno tras el 's' del humano: {}",
                        last.content
                    );
                    Ok(LlmChatResponse {
                        content: "El servicio Tor old-svc ha sido eliminado.".to_string(),
                        tool_calls: vec![],
                        ..Default::default()
                    })
                }
            });

        let gate = FixedGate::new(GateDecision::Approved);
        let assistant =
            AiAssistant::new(Arc::new(engine), Arc::new(reg), confirmations, gate.clone());
        let out = assistant
            .ask("elimina el servicio tor old-svc", &AskOptions::default())
            .await
            .unwrap();
        assert!(out.contains("old-svc"));
        let recorded = gate.recorded.lock().unwrap();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].1, ToolKind::Destructive);
    }

    #[tokio::test]
    async fn mutating_tool_without_tty_and_without_yes_fails_closed() {
        let mut reg = ToolRegistry::new();
        let wp_create_spy = SpyTool::new(json!({"created": true}));
        register_tool(
            &mut reg,
            "wp_create",
            ToolKind::Mutating,
            json!({
                "type": "object",
                "properties": {"name": {"type": "string"}},
                "required": ["name"],
                "additionalProperties": false
            }),
            wp_create_spy.clone(),
        );

        let mut engine = MockLlmEngine::new();
        engine.expect_chat_tools().times(1).returning(|_, _, _| {
            Ok(LlmChatResponse {
                content: String::new(),
                tool_calls: vec![LlmToolCall {
                    name: "wp_create".to_string(),
                    arguments: json!({"name": "blog1"}),
                }],
                ..Default::default()
            })
        });

        let assistant = AiAssistant::new(
            Arc::new(engine),
            Arc::new(reg),
            Arc::new(ConfirmationStore::new()),
            FixedGate::new(GateDecision::NoTty),
        );
        let err = assistant
            .ask("crea wp blog1", &AskOptions::default())
            .await
            .unwrap_err();
        assert!(matches!(err, EnolaError::SecurityError(_)));
        assert!(wp_create_spy.calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn mutating_tool_with_no_exec_simulates_without_calling_handler() {
        let mut reg = ToolRegistry::new();
        let wp_create_spy = SpyTool::new(json!({"created": true}));
        register_tool(
            &mut reg,
            "wp_create",
            ToolKind::Mutating,
            json!({
                "type": "object",
                "properties": {"name": {"type": "string"}},
                "required": ["name"],
                "additionalProperties": false
            }),
            wp_create_spy.clone(),
        );

        let mut engine = MockLlmEngine::new();
        let mut turn = 0usize;
        engine
            .expect_chat_tools()
            .times(2)
            .returning(move |_, msgs, _| {
                turn += 1;
                if turn == 1 {
                    Ok(LlmChatResponse {
                        content: String::new(),
                        tool_calls: vec![LlmToolCall {
                            name: "wp_create".to_string(),
                            arguments: json!({"name": "simblog"}),
                        }],
                        ..Default::default()
                    })
                } else {
                    let last = msgs.last().unwrap();
                    assert!(
                        last.content.contains("\"simulated\":true")
                            || last.content.contains("simulated"),
                        "debe recibir la simulación try_run: {}",
                        last.content
                    );
                    Ok(LlmChatResponse {
                        content: "Simulación (--no-exec): se crearía el WordPress simblog."
                            .to_string(),
                        tool_calls: vec![],
                        ..Default::default()
                    })
                }
            });

        // Incluso si el gate fuese NoTty, --no-exec nunca ejecuta ni pide confirmación.
        let gate = FixedGate::new(GateDecision::NoTty);
        let assistant = AiAssistant::new(
            Arc::new(engine),
            Arc::new(reg),
            Arc::new(ConfirmationStore::new()),
            gate.clone(),
        );
        let opts = AskOptions {
            no_exec: true,
            ..Default::default()
        };
        let out = assistant.ask("crea wp simblog", &opts).await.unwrap();
        assert!(out.contains("simblog"));
        assert!(wp_create_spy.calls.lock().unwrap().is_empty());
        assert_eq!(wp_create_spy.try_calls.lock().unwrap().len(), 1);
        assert!(gate.recorded.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn hallucinated_tool_returns_recoverable_error_and_re_asks_model() {
        let mut reg = ToolRegistry::new();
        let tor_spy = SpyTool::new(json!({"services": []}));
        register_tool(
            &mut reg,
            "tor_list",
            ToolKind::ReadOnly,
            json!({"type": "object", "properties": {}, "additionalProperties": false}),
            tor_spy.clone(),
        );

        let mut engine = MockLlmEngine::new();
        let mut turn = 0usize;
        engine
            .expect_chat_tools()
            .times(3)
            .returning(move |_, msgs, _| {
                turn += 1;
                match turn {
                    1 => Ok(LlmChatResponse {
                        content: String::new(),
                        tool_calls: vec![LlmToolCall {
                            name: "magic_nonexistent_tool".to_string(),
                            arguments: json!({}),
                        }],
                        ..Default::default()
                    }),
                    2 => {
                        let last = msgs.last().unwrap();
                        assert!(
                            last.content.contains("unknown tool")
                                && last.content.contains("magic_nonexistent_tool"),
                            "el error de tool alucinada debe enviarse al LLM: {}",
                            last.content
                        );
                        Ok(LlmChatResponse {
                            content: String::new(),
                            tool_calls: vec![LlmToolCall {
                                name: "tor_list".to_string(),
                                arguments: json!({}),
                            }],
                            ..Default::default()
                        })
                    }
                    _ => Ok(LlmChatResponse {
                        content: "No tienes servicios Tor.".to_string(),
                        tool_calls: vec![],
                        ..Default::default()
                    }),
                }
            });

        let assistant = AiAssistant::new(
            Arc::new(engine),
            Arc::new(reg),
            Arc::new(ConfirmationStore::new()),
            FixedGate::new(GateDecision::Approved),
        );
        let out = assistant
            .ask("lista tor", &AskOptions::default())
            .await
            .unwrap();
        assert_eq!(out, "No tienes servicios Tor.");
        assert_eq!(tor_spy.calls.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn huge_tool_output_is_truncated_before_returning_to_llm() {
        let mut reg = ToolRegistry::new();
        let huge = "X".repeat(4096);
        let spy = SpyTool::new(json!({"log": huge}));
        register_tool(
            &mut reg,
            "ports_list",
            ToolKind::ReadOnly,
            json!({"type": "object", "properties": {}, "additionalProperties": false}),
            spy,
        );

        let mut engine = MockLlmEngine::new();
        let mut turn = 0usize;
        engine
            .expect_chat_tools()
            .times(2)
            .returning(move |_, msgs, _| {
                turn += 1;
                if turn == 1 {
                    Ok(LlmChatResponse {
                        content: String::new(),
                        tool_calls: vec![LlmToolCall {
                            name: "ports_list".to_string(),
                            arguments: json!({}),
                        }],
                        ..Default::default()
                    })
                } else {
                    let last = msgs.last().unwrap();
                    assert!(
                        last.content.len() < 600 && last.content.contains("[truncated]"),
                        "salida grande debe ir truncada y marcada, len={}: {}",
                        last.content.len(),
                        last.content
                    );
                    Ok(LlmChatResponse {
                        content: "Resumen de puertos.".to_string(),
                        tool_calls: vec![],
                        ..Default::default()
                    })
                }
            });

        let assistant = AiAssistant::new(
            Arc::new(engine),
            Arc::new(reg),
            Arc::new(ConfirmationStore::new()),
            FixedGate::new(GateDecision::Approved),
        );
        let opts = AskOptions {
            max_tool_output_bytes: 256,
            ..Default::default()
        };
        let out = assistant.ask("puertos", &opts).await.unwrap();
        assert_eq!(out, "Resumen de puertos.");
    }

    #[test]
    fn parse_confirmation_answer_accepts_spanish_and_english_affirmative() {
        assert!(parse_confirmation_answer("s"));
        assert!(parse_confirmation_answer("S"));
        assert!(parse_confirmation_answer("si"));
        assert!(parse_confirmation_answer("sí"));
        assert!(parse_confirmation_answer("y"));
        assert!(parse_confirmation_answer("YES\n"));
        assert!(!parse_confirmation_answer(""));
        assert!(!parse_confirmation_answer("n"));
        assert!(!parse_confirmation_answer("no"));
    }

    // ── Paso 9.4: Configuración [ai], --instance y guía de primera ejecución ──

    #[test]
    fn resolve_ai_config_uses_defaults_when_no_flags_env_or_file() {
        let _g = crate::infrastructure::TEST_ENV_LOCK.lock().unwrap();
        for k in [
            "ENOLA_AI_ENDPOINT",
            "ENOLA_AI_MODEL",
            "ENOLA_AI_TIMEOUT_SECS",
        ] {
            std::env::remove_var(k);
        }
        let cfg = resolve_ai_config(&AiFlagOverrides::default(), &HashMap::new());
        assert_eq!(cfg.endpoint, "http://127.0.0.1:11434");
        assert_eq!(cfg.model, "llama3.1:8b");
        assert_eq!(cfg.timeout_secs, 120);
    }

    #[test]
    fn resolve_ai_config_precedence_flag_beats_env_beats_file_beats_default() {
        let _g = crate::infrastructure::TEST_ENV_LOCK.lock().unwrap();
        std::env::set_var("ENOLA_AI_ENDPOINT", "http://127.0.0.1:11499/");
        std::env::set_var("ENOLA_AI_MODEL", "qwen2.5:7b");
        std::env::remove_var("ENOLA_AI_TIMEOUT_SECS");

        let mut file = HashMap::new();
        file.insert("endpoint".to_string(), "http://127.0.0.1:11488".to_string());
        file.insert("model".to_string(), "mistral:7b".to_string());
        file.insert("timeout_secs".to_string(), "180".to_string());

        let flags = AiFlagOverrides {
            endpoint: Some("http://127.0.0.1:11477/".to_string()),
            model: None,
            timeout_secs: None,
            use_tor_for_downloads: None,
        };
        let cfg = resolve_ai_config(&flags, &file);
        assert_eq!(cfg.endpoint, "http://127.0.0.1:11477"); // flag gana a env y file
        assert_eq!(cfg.model, "qwen2.5:7b"); // env gana a file
        assert_eq!(cfg.timeout_secs, 180); // file gana a default

        for k in [
            "ENOLA_AI_ENDPOINT",
            "ENOLA_AI_MODEL",
            "ENOLA_AI_TIMEOUT_SECS",
        ] {
            std::env::remove_var(k);
        }
    }

    #[test]
    fn resolve_instance_endpoint_finds_explicit_instance_and_auto_discovers_piloto_or_enola_ai() {
        let containers = vec![
            ContainerInfo {
                id: "1".to_string(),
                name: "enola-ai-dev-ollama".to_string(),
                image: "ollama/ollama:latest".to_string(),
                status: "Up 2 hours".to_string(),
                ports: vec!["127.0.0.1:11435->11434/tcp".to_string()],
            },
            ContainerInfo {
                id: "2".to_string(),
                name: "piloto-ollama".to_string(),
                image: "ollama/ollama:latest".to_string(),
                status: "Up 5 hours".to_string(),
                ports: vec!["0.0.0.0:11434->11434/tcp".to_string()],
            },
        ];

        let explicit = resolve_instance_endpoint(&containers, Some("dev")).unwrap();
        assert_eq!(explicit.as_deref(), Some("http://127.0.0.1:11435"));

        let piloto = resolve_instance_endpoint(&containers, Some("piloto")).unwrap();
        assert_eq!(piloto.as_deref(), Some("http://127.0.0.1:11434"));

        let auto = resolve_instance_endpoint(&containers, None).unwrap();
        assert_eq!(auto.as_deref(), Some("http://127.0.0.1:11435"));

        let missing = resolve_instance_endpoint(&containers, Some("nonexistent")).unwrap_err();
        assert!(missing.to_string().contains("nonexistent"));
        assert!(missing.to_string().contains("ai create"));
    }

    fn ollama_container(name: &str, host_port: u16) -> ContainerInfo {
        ContainerInfo {
            id: name.to_string(),
            name: name.to_string(),
            image: "ollama/ollama:latest".to_string(),
            status: "Up".to_string(),
            ports: vec![format!("127.0.0.1:{host_port}->11434/tcp")],
        }
    }

    #[test]
    fn resolve_instance_endpoint_prefers_canonical_container_over_legacy_names() {
        // Docker lista antes un contenedor con nombre legacy (`dev`): aun así
        // `--instance dev` debe resolver al canónico `enola-ai-dev-ollama`.
        let containers = vec![
            ollama_container("dev", 11500),
            ollama_container("enola-ollama-dev", 11501),
            ollama_container("enola-ai-dev-ollama", 11435),
        ];
        let endpoint = resolve_instance_endpoint(&containers, Some("dev")).unwrap();
        assert_eq!(endpoint.as_deref(), Some("http://127.0.0.1:11435"));
    }

    #[test]
    fn resolve_instance_endpoint_autodiscovery_is_ordered_by_class_then_name() {
        // Sin `--instance`: primero instancias gestionadas (`enola-ai-*-ollama`),
        // luego legacy (`enola-ollama-*`), luego el externo `piloto-ollama`; dentro
        // de cada clase, orden alfabético. Nunca el orden arbitrario de Docker.
        let containers = vec![
            ollama_container("piloto-ollama", 11434),
            ollama_container("enola-ollama-old", 11600),
            ollama_container("enola-ai-zeta-ollama", 11437),
            ollama_container("enola-ai-alpha-ollama", 11436),
        ];
        let auto = resolve_instance_endpoint(&containers, None).unwrap();
        assert_eq!(auto.as_deref(), Some("http://127.0.0.1:11436"));

        let legacy_and_external = vec![
            ollama_container("piloto-ollama", 11434),
            ollama_container("enola-ollama-old", 11600),
        ];
        let auto = resolve_instance_endpoint(&legacy_and_external, None).unwrap();
        assert_eq!(auto.as_deref(), Some("http://127.0.0.1:11600"));
    }

    #[test]
    fn unreachable_guidance_mentions_install_and_ai_create() {
        let msg = format_unreachable_ollama_guidance(
            "http://127.0.0.1:11434",
            Some("http://127.0.0.1:11435"),
            "connection refused",
        );
        assert!(msg.contains("http://127.0.0.1:11434"));
        assert!(msg.contains("enola-cli ai create"));
        assert!(msg.contains("http://127.0.0.1:11435"));
    }
}
