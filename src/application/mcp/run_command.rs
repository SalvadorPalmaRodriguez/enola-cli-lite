// MCP-004 — Meta-tool `enola_run_command` (solo lectura en esta fase).
//
// Ejecuta cualquier subcomando del árbol generado (build.rs → mcp_tools.json)
// como subproceso `enola-cli --format json <family> <sub>... --flag value...`.
//
// No es una shell: nunca se construye una cadena de comando; el argv se pasa
// como array a `ProcessRunnerPort`. Familia, subcomandos y flags se validan
// contra el árbol antes de despachar (default-deny: lo desconocido se
// clasifica como mutante y se rechaza hasta `--allow-write` en Fase 7).

use crate::application::mcp::audit::AuditLogger;
use crate::application::mcp::command_catalog::load_tree;
use crate::application::mcp::policy::McpPolicy;
use crate::application::mcp::registry::ToolHandler;
use crate::domain::error::{EnolaError, Result};
use crate::domain::tool::{classify, truncate_output, ToolKind};
use crate::ports::process_runner::ProcessRunnerPort;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

/// Timeout por defecto de un subcomando despachado.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);
/// Profundidad máxima de la ruta de subcomandos (familia → grupo → hoja).
const MAX_PATH_TOKENS: usize = 2;
/// Timeout máximo aceptado vía `timeout_secs` (5 min).
const MAX_TIMEOUT_SECS: u64 = 300;

fn err(msg: impl Into<String>) -> EnolaError {
    EnolaError::ValidationError(msg.into())
}

/// Localiza la familia en el árbol generado.
fn find_family<'a>(tree: &'a Value, family: &str) -> Result<&'a Value> {
    tree["families"]
        .as_array()
        .and_then(|fams| fams.iter().find(|f| f["name"].as_str() == Some(family)))
        .ok_or_else(|| err(format!("unknown family '{family}'")))
}

/// Recorre la ruta de subcomandos y devuelve (tokens, nodo hoja).
///
/// `subcommand` admite rutas de nivel 3 separadas por espacio
/// (`"wp create"` → `plan wp create`, `"auth list"` → `tor auth list`).
fn resolve_leaf<'a>(
    family_node: &'a Value,
    subcommand: Option<&str>,
) -> Result<(Vec<String>, &'a Value)> {
    let has_children = family_node["subcommands"]
        .as_array()
        .map(|s| !s.is_empty())
        .unwrap_or(false);

    let raw = subcommand.unwrap_or("").trim();
    if !has_children {
        if !raw.is_empty() {
            return Err(err(format!(
                "family '{}' takes no subcommand",
                family_node["name"].as_str().unwrap_or("")
            )));
        }
        return Ok((Vec::new(), family_node));
    }
    if raw.is_empty() {
        return Err(err(format!(
            "family '{}' requires a subcommand",
            family_node["name"].as_str().unwrap_or("")
        )));
    }

    let tokens: Vec<String> = raw.split_whitespace().map(str::to_string).collect();
    if tokens.len() > MAX_PATH_TOKENS {
        return Err(err(format!("subcommand path too deep: '{raw}'")));
    }

    let mut node = family_node;
    for tok in &tokens {
        node = node["subcommands"]
            .as_array()
            .and_then(|subs| {
                subs.iter()
                    .find(|s| s["name"].as_str() == Some(tok.as_str()))
            })
            .ok_or_else(|| err(format!("unknown subcommand '{tok}'")))?;
    }
    if node["subcommands"]
        .as_array()
        .map(|s| !s.is_empty())
        .unwrap_or(false)
    {
        return Err(err(format!(
            "incomplete subcommand path '{raw}': it still has subcommands"
        )));
    }
    Ok((tokens, node))
}

/// Valida `args` (objeto flag→valor) contra las flags del nodo hoja y
/// devuelve los pares `--flag valor` ordenados para el argv.
fn build_flag_argv(leaf: &Value, args: &Value) -> Result<Vec<String>> {
    let empty = json!({});
    let args = if args.is_null() { &empty } else { args };
    let args = args
        .as_object()
        .ok_or_else(|| err("'args' must be an object of flag: value"))?;
    let flags = leaf["flags"].as_array().cloned().unwrap_or_default();

    let mut validated: Vec<(String, String)> = Vec::with_capacity(args.len());
    for (key, value) in args {
        if key.starts_with('-') || key.contains(char::is_whitespace) {
            return Err(err(format!("invalid flag name '{key}'")));
        }
        let spec = flags
            .iter()
            .find(|f| f["name"].as_str() == Some(key.as_str()))
            .ok_or_else(|| err(format!("unknown flag '--{key}'")))?;
        let value_str = match value {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            Value::Bool(b) => b.to_string(),
            _ => return Err(err(format!("flag '--{key}' needs a scalar value"))),
        };
        let possible: Vec<String> = spec["possible_values"]
            .as_array()
            .map(|p| {
                p.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        if !possible.is_empty() && !possible.iter().any(|p| p == &value_str) {
            return Err(err(format!(
                "invalid value '{value_str}' for '--{key}' (expected one of: {})",
                possible.join(", ")
            )));
        }
        validated.push((key.clone(), value_str));
    }

    for spec in &flags {
        if spec["required"].as_bool() == Some(true) {
            let name = spec["name"].as_str().unwrap_or("");
            if !args.contains_key(name) {
                return Err(err(format!("missing required flag '--{name}'")));
            }
        }
    }

    validated.sort_by(|a, b| a.0.cmp(&b.0));
    let mut argv = Vec::with_capacity(validated.len() * 2 + 2);
    // Posicionales primero, en su orden de declaración clap (sin `--`).
    // Las flags nombradas van después como pares `--flag valor`.
    let mut positional: Vec<(usize, &String, &String)> = Vec::new();
    let mut named: Vec<(&String, &String)> = Vec::new();
    for (key, value) in &validated {
        if let Some(idx) = flags.iter().position(|f| {
            f["name"].as_str() == Some(key.as_str()) && f["positional"].as_bool() == Some(true)
        }) {
            positional.push((idx, key, value));
        } else {
            named.push((key, value));
        }
    }
    positional.sort_by_key(|(idx, _, _)| *idx);
    for (_, _, value) in positional {
        argv.push(value.clone());
    }
    for (key, value) in named {
        argv.push(format!("--{key}"));
        argv.push(value.clone());
    }
    // Fuerza salida JSON: además del `--format json` global, si el hoja expone
    // una flag nativa `--json` y el llamador no la pasó, se añade `--json true`.
    let has_json_flag = flags.iter().any(|f| f["name"].as_str() == Some("json"));
    if has_json_flag && !args.contains_key("json") {
        argv.push("--json".to_string());
        argv.push("true".to_string());
    }
    Ok(argv)
}

/// Construye el argv completo `["--format","json",family,sub...,--flag,val...]`.
///
/// Es `pub(crate)` para tests; solo se llama tras `resolve_leaf` +
/// `build_flag_argv`, así que la ruta y las flags ya están validadas.
pub(crate) fn build_argv(family: &str, tokens: &[String], flag_argv: Vec<String>) -> Vec<String> {
    let mut argv = vec![
        "--format".to_string(),
        "json".to_string(),
        family.to_string(),
    ];
    argv.extend(tokens.iter().cloned());
    argv.extend(flag_argv);
    argv
}

struct RunCommandHandler {
    runner: Arc<dyn ProcessRunnerPort + Send + Sync>,
    policy: Arc<McpPolicy>,
    audit: Arc<AuditLogger>,
}

#[async_trait::async_trait]
impl ToolHandler for RunCommandHandler {
    #[tracing::instrument(
        skip_all,
        fields(family = tracing::field::Empty, command = tracing::field::Empty)
    )]
    async fn call(&self, args: Value) -> Result<Value> {
        let family = args
            .get("family")
            .and_then(|v| v.as_str())
            .ok_or_else(|| err("missing required 'family'"))?;
        let subcommand = args.get("subcommand").and_then(|v| v.as_str());
        let tree = load_tree();
        let family_node = find_family(&tree, family)?;
        let (tokens, leaf) = resolve_leaf(family_node, subcommand)?;
        let span = tracing::Span::current();
        span.record("family", family);
        span.record("command", tokens.join(" ").as_str());

        // Política de escritura (Fase 7): el nombre evaluado es `family:sub`
        // para que la allowlist pueda limitar comandos concretos. La llamada
        // mutante queda auditada aunque la meta-tool sea ReadOnly.
        let kind = classify(family, tokens.first().map(String::as_str));
        let policy_name = format!("{family}:{}", tokens.join(" "));
        let mutating = kind == ToolKind::Mutating;
        if let Err(e) = self.policy.allows(kind, &policy_name) {
            if mutating {
                self.audit.log_call(&policy_name, &args, "rejected");
            }
            return Err(e);
        }

        let flag_argv = build_flag_argv(leaf, args.get("args").unwrap_or(&Value::Null))?;
        let argv = build_argv(family, &tokens, flag_argv);

        let timeout = args
            .get("timeout_secs")
            .and_then(|v| v.as_u64())
            .map(|s| Duration::from_secs(s.clamp(1, MAX_TIMEOUT_SECS)))
            .unwrap_or(DEFAULT_TIMEOUT);

        let out = self.runner.run(&argv, timeout).await;
        if mutating {
            let label = match &out {
                Ok(_) => "ok".to_string(),
                Err(e) => format!("error: {e}"),
            };
            self.audit.log_call(&policy_name, &args, &label);
        }
        let out = out?;
        let (stdout, t1) = truncate_output(&out.stdout, self.policy.max_output_bytes);
        let (stderr, t2) = truncate_output(&out.stderr, self.policy.max_output_bytes);
        Ok(json!({
            "argv": argv,
            "exit_code": out.exit_code,
            "stdout": stdout,
            "stderr": stderr,
            "truncated": t1 || t2,
        }))
    }
}

/// Registra la meta-tool `enola_run_command` en el registry.
pub(crate) fn register(
    registry: &mut crate::application::mcp::registry::ToolRegistry,
    runner: Arc<dyn ProcessRunnerPort + Send + Sync>,
    policy: Arc<McpPolicy>,
    audit: Arc<AuditLogger>,
) -> Result<()> {
    use crate::domain::tool::ToolSpec;
    let spec = ToolSpec::new(
        "enola_run_command",
        "Run any enola-cli command from the generated catalog. Mutating commands \
         require --allow-write (+ allowlist). 'subcommand' accepts level-3 paths \
         like 'wp create'. 'args' is an object of flag values. Always forces \
         --format json.",
        json!({
            "type": "object",
            "properties": {
                "family": {"type": "string"},
                "subcommand": {"type": "string"},
                "args": {"type": "object"},
                "timeout_secs": {"type": "integer"}
            },
            "required": ["family"]
        }),
        ToolKind::ReadOnly,
    )
    .map_err(EnolaError::ValidationError)?;
    registry.register(
        spec,
        Arc::new(RunCommandHandler {
            runner,
            policy,
            audit,
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::process_runner::{MockProcessRunnerPort, ProcessOutput};
    use std::time::Duration;

    fn handler_with(runner: MockProcessRunnerPort) -> RunCommandHandler {
        handler_with_policy(runner, McpPolicy::default())
    }

    fn handler_with_policy(runner: MockProcessRunnerPort, policy: McpPolicy) -> RunCommandHandler {
        RunCommandHandler {
            runner: Arc::new(runner),
            policy: Arc::new(policy),
            audit: Arc::new(AuditLogger::new(None)),
        }
    }

    fn ok_runner(expected_argv: Vec<String>) -> MockProcessRunnerPort {
        let mut mock = MockProcessRunnerPort::new();
        mock.expect_run()
            .withf(move |argv, _| *argv == expected_argv)
            .returning(|_, _| {
                Ok(ProcessOutput {
                    exit_code: 0,
                    stdout: "{\"ok\":true}".to_string(),
                    stderr: String::new(),
                })
            });
        mock
    }

    #[tokio::test]
    async fn rejects_unknown_family_without_dispatching() {
        let mock = MockProcessRunnerPort::new(); // sin expectativas: falla si se llama
        let h = handler_with(mock);
        let r = h.call(json!({"family": "nope"})).await;
        assert!(r.is_err());
    }

    #[tokio::test]
    async fn rejects_unknown_subcommand() {
        let mock = MockProcessRunnerPort::new();
        let h = handler_with(mock);
        let r = h.call(json!({"family": "tor", "subcommand": "nope"})).await;
        assert!(r.is_err());
    }

    #[tokio::test]
    async fn rejects_missing_subcommand_for_nonleaf_family() {
        let mock = MockProcessRunnerPort::new();
        let h = handler_with(mock);
        let r = h.call(json!({"family": "tor"})).await;
        assert!(r.is_err());
    }

    #[tokio::test]
    async fn rejects_subcommand_on_leaf_family() {
        let mock = MockProcessRunnerPort::new();
        let h = handler_with(mock);
        let r = h
            .call(json!({"family": "doctor", "subcommand": "list"}))
            .await;
        assert!(r.is_err());
    }

    #[tokio::test]
    async fn rejects_incomplete_path() {
        let mock = MockProcessRunnerPort::new();
        let h = handler_with(mock);
        let r = h.call(json!({"family": "plan", "subcommand": "wp"})).await;
        assert!(r.is_err());
    }

    #[tokio::test]
    async fn rejects_unknown_flag() {
        let mock = MockProcessRunnerPort::new();
        let h = handler_with(mock);
        let r = h
            .call(json!({
                "family": "wp", "subcommand": "status",
                "args": {"evil": "x", "name": "s"}
            }))
            .await;
        assert!(r.is_err());
    }

    #[tokio::test]
    async fn rejects_missing_required_flag() {
        let mock = MockProcessRunnerPort::new();
        let h = handler_with(mock);
        let r = h
            .call(json!({"family": "wp", "subcommand": "status", "args": {}}))
            .await;
        assert!(r.is_err());
    }

    #[tokio::test]
    async fn rejects_mutating_command() {
        let mock = MockProcessRunnerPort::new();
        let h = handler_with(mock);
        let r = h
            .call(json!({
                "family": "wp", "subcommand": "create",
                "args": {"name": "x"}
            }))
            .await;
        let e = r.unwrap_err().to_string();
        assert!(e.contains("allow-write"), "got: {e}");
    }

    #[tokio::test]
    async fn dispatches_leaf_family_forcing_json() {
        let mock = ok_runner(vec!["--format".into(), "json".into(), "doctor".into()]);
        let h = handler_with(mock);
        let r = h.call(json!({"family": "doctor"})).await.unwrap();
        assert_eq!(r["exit_code"], 0);
        assert_eq!(r["argv"][2], "doctor");
    }

    #[tokio::test]
    async fn dispatches_readonly_subcommand_with_flags() {
        // `wp status` toma `name` como posicional → se emite como valor suelto.
        let mock = ok_runner(vec![
            "--format".into(),
            "json".into(),
            "wp".into(),
            "status".into(),
            "mysite".into(),
        ]);
        let h = handler_with(mock);
        let r = h
            .call(json!({
                "family": "wp", "subcommand": "status",
                "args": {"name": "mysite"}
            }))
            .await
            .unwrap();
        assert_eq!(r["exit_code"], 0);
    }

    #[tokio::test]
    async fn dispatches_level3_path() {
        let mock = ok_runner(vec![
            "--format".into(),
            "json".into(),
            "plan".into(),
            "wp".into(),
            "create".into(),
            "--name".into(),
            "blog".into(),
        ]);
        let h = handler_with(mock);
        let r = h
            .call(json!({
                "family": "plan", "subcommand": "wp create",
                "args": {"name": "blog"}
            }))
            .await
            .unwrap();
        let argv: Vec<String> = serde_json::from_value(r["argv"].clone()).unwrap();
        assert_eq!(&argv[2..5], ["plan", "wp", "create"]);
    }

    #[tokio::test]
    async fn shell_payload_is_passed_as_literal_argv_value() {
        // R17: cargas como `; rm -rf /` no pueden escapar a una shell porque
        // se pasan como elemento literal del argv, nunca concatenadas.
        let payload = "; rm -rf / $(id) `id`\nwhoami";
        let mock = ok_runner(vec![
            "--format".into(),
            "json".into(),
            "wp".into(),
            "status".into(),
            payload.to_string(),
        ]);
        let h = handler_with(mock);
        let r = h
            .call(json!({
                "family": "wp", "subcommand": "status",
                "args": {"name": payload}
            }))
            .await
            .unwrap();
        // El valor llega íntegro como UN elemento del argv (sin expansión).
        assert_eq!(r["argv"].as_array().unwrap().last().unwrap(), payload);
    }

    #[tokio::test]
    async fn timeout_error_propagates() {
        let mut mock = MockProcessRunnerPort::new();
        mock.expect_run()
            .returning(|_, _: Duration| Err(EnolaError::Timeout("timed out".to_string())));
        let h = handler_with(mock);
        let r = h.call(json!({"family": "doctor"})).await;
        assert!(r.is_err());
    }

    #[tokio::test]
    async fn appends_native_json_flag_when_leaf_supports_it() {
        // `ports list` solo emite JSON vía su propia `--json`; la meta-tool la
        // añade sola junto al `--format json` global.
        let mock = ok_runner(vec![
            "--format".into(),
            "json".into(),
            "ports".into(),
            "list".into(),
            "--json".into(),
            "true".into(),
        ]);
        let h = handler_with(mock);
        let r = h
            .call(json!({"family": "ports", "subcommand": "list"}))
            .await
            .unwrap();
        assert_eq!(r["exit_code"], 0);
    }

    #[tokio::test]
    async fn explicit_json_arg_is_not_overridden() {
        let mock = ok_runner(vec![
            "--format".into(),
            "json".into(),
            "ports".into(),
            "list".into(),
            "--json".into(),
            "false".into(),
        ]);
        let h = handler_with(mock);
        assert!(h
            .call(json!({
                "family": "ports", "subcommand": "list",
                "args": {"json": false}
            }))
            .await
            .is_ok());
    }

    #[tokio::test]
    async fn possible_values_are_enforced() {
        let mock = MockProcessRunnerPort::new();
        let h = handler_with(mock);
        // `tor list` no tiene flags con valores posibles; usar `plan wp create`...
        // mejor: `wp status --name` no tiene possible_values. Probar flag `json`
        // de `ports list` (possible_values true/false).
        let r = h
            .call(json!({
                "family": "ports", "subcommand": "list",
                "args": {"json": "yes"}
            }))
            .await;
        assert!(r.is_err());
        let mock = ok_runner(vec![
            "--format".into(),
            "json".into(),
            "ports".into(),
            "list".into(),
            "--json".into(),
            "true".into(),
        ]);
        let h = handler_with(mock);
        assert!(h
            .call(json!({
                "family": "ports", "subcommand": "list",
                "args": {"json": true}
            }))
            .await
            .is_ok());
    }

    // ── Fase 7.4: política de escritura en la meta-tool ─────────────────────

    #[tokio::test]
    async fn mutating_commands_rejected_without_allow_write() {
        // Muestra de familias mutantes: la política no depende de que la tool
        // sea de primera clase (todas pasan por enola_run_command).
        for (family, sub) in [
            ("drupal", "create"),
            ("ghost", "create"),
            ("strapi", "create"),
            ("wagtail", "create"),
            ("magnolia", "create"),
            ("firewall", "allow"),
            ("vpn", "create"),
            ("vpn", "delete"),
            ("maintenance", "cleanup"),
        ] {
            let mock = MockProcessRunnerPort::new(); // no dispatch esperado
            let h = handler_with(mock);
            let r = h
                .call(json!({"family": family, "subcommand": sub, "args": {}}))
                .await;
            assert!(r.is_err(), "{family} {sub} should be rejected");
        }
    }

    #[tokio::test]
    async fn mutating_command_dispatched_with_allow_write() {
        let mock = ok_runner(vec![
            "--format".into(),
            "json".into(),
            "tor".into(),
            "create".into(),
            "--name".into(),
            "svc".into(),
        ]);
        let policy = McpPolicy {
            allow_write: true,
            ..Default::default()
        };
        let h = handler_with_policy(mock, policy);
        // tor create requiere --name (required). virtual/target port opcionales.
        let r = h
            .call(json!({
                "family": "tor", "subcommand": "create",
                "args": {"name": "svc"}
            }))
            .await
            .unwrap();
        assert_eq!(r["exit_code"], 0);
    }

    #[tokio::test]
    async fn mutating_command_rejected_when_not_in_allowlist() {
        let mock = MockProcessRunnerPort::new();
        let policy = McpPolicy {
            allow_write: true,
            allowed_tools: vec!["wp:create".to_string()],
            ..Default::default()
        };
        let h = handler_with_policy(mock, policy);
        let r = h
            .call(json!({
                "family": "tor", "subcommand": "create",
                "args": {"name": "svc"}
            }))
            .await;
        assert!(r.is_err());
    }

    #[tokio::test]
    async fn allowlist_matches_command_path() {
        let mock = ok_runner(vec![
            "--format".into(),
            "json".into(),
            "wp".into(),
            "create".into(),
            "--name".into(),
            "blog".into(),
        ]);
        let policy = McpPolicy {
            allow_write: true,
            allowed_tools: vec!["wp:create".to_string()],
            ..Default::default()
        };
        let h = handler_with_policy(mock, policy);
        assert!(h
            .call(json!({
                "family": "wp", "subcommand": "create",
                "args": {"name": "blog"}
            }))
            .await
            .is_ok());
    }

    #[tokio::test]
    async fn destructive_commands_never_get_force_injected() {
        // `tor remove` exige --force en clap; la tool no lo añade por su cuenta:
        // el argv solo lleva lo que el llamador pasó explícitamente.
        let mock = ok_runner(vec![
            "--format".into(),
            "json".into(),
            "tor".into(),
            "remove".into(),
            "svc".into(),
        ]);
        let policy = McpPolicy {
            allow_write: true,
            ..Default::default()
        };
        let h = handler_with_policy(mock, policy);
        let r = h
            .call(json!({
                "family": "tor", "subcommand": "remove",
                "args": {"name": "svc"}
            }))
            .await
            .unwrap();
        let argv: Vec<String> = serde_json::from_value(r["argv"].clone()).unwrap();
        assert!(!argv.iter().any(|a| a == "--force" || a == "--yes"));
    }
}
