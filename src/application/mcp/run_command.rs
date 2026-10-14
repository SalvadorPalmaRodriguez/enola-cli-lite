// MCP-004/007/009 — Meta-tool `enola_run_command`.
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
use crate::application::mcp::confirmation::{confirmation_required_result, ConfirmationStore};
use crate::application::mcp::policy::McpPolicy;
use crate::application::mcp::registry::ToolHandler;
use crate::domain::error::{EnolaError, Result};
use crate::domain::tool::{
    classify, is_forbidden_family, is_forbidden_path, truncate_output, ToolKind,
};
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

/// Flags vetadas por hoja en `enola_run_command`: (familia, path, nombres).
///
/// Defensa en profundidad (auditoría F-01/F-25): los bool `SetTrue` son hoy
/// indespachables por el serializado `--flag valor`, pero `--binary`
/// (flag de valor) sí es expresable y rompe la cadena de firma de updates.
const DENIED_FLAGS: &[(&str, &[&str], &[&str])] = &[
    ("update", &["download"], &["allow-unsigned"]),
    ("update", &["apply"], &["binary", "allow-unsigned"]),
];

/// Primera flag vetada presente en `args["args"]` para la ruta, si la hay.
fn denied_flag(family: &str, tokens: &[String], args: &Value) -> Option<String> {
    let obj = args.get("args")?.as_object()?;
    let denied = DENIED_FLAGS.iter().find_map(|(fam, path, denied)| {
        (*fam == family && path.iter().copied().eq(tokens.iter().map(String::as_str)))
            .then_some(*denied)
    })?;
    obj.keys().find(|k| denied.contains(&k.as_str())).cloned()
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

    // `None` en el valor = flag booleana de presencia (`--flag` pelado).
    let mut validated: Vec<(String, Option<String>)> = Vec::with_capacity(args.len());
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
        // F-13: un valor que empieza por '-' se interpretaría como otra flag
        // por clap (o, en posicionales, como flag inyectada). Hoy falla
        // cerrado por el parser, pero el rechazo explícito no depende del
        // comportamiento por defecto de clap.
        if value_str.starts_with('-') {
            return Err(err(format!("value for '--{key}' must not start with '-'")));
        }
        // R-11: las flags de presencia (clap SetTrue/SetFalse) se emiten como
        // `--flag` sin valor; `false` omite la flag. Rechazar cualquier otro
        // valor aquí evita que clap falle con `unexpected argument`.
        if spec["boolean"].as_bool() == Some(true) {
            match value_str.as_str() {
                "true" => validated.push((key.clone(), None)),
                "false" => {}
                _ => return Err(err(format!("flag '--{key}' is boolean; use true or false"))),
            }
            continue;
        }
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
        validated.push((key.clone(), Some(value_str)));
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
    let mut positional: Vec<(usize, &String, &Option<String>)> = Vec::new();
    let mut named: Vec<(&String, &Option<String>)> = Vec::new();
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
        if let Some(v) = value {
            argv.push(v.clone());
        }
    }
    for (key, value) in named {
        argv.push(format!("--{key}"));
        if let Some(v) = value {
            argv.push(v.clone());
        }
    }
    // Fuerza salida JSON: además del `--format json` global, si el hoja expone
    // una flag nativa `--json` y el llamador no la pasó, se añade. Si la flag
    // es de presencia (SetTrue) va pelada; si toma valor, `--json true`.
    if !args.contains_key("json") {
        if let Some(spec) = flags.iter().find(|f| f["name"].as_str() == Some("json")) {
            argv.push("--json".to_string());
            if spec["boolean"].as_bool() != Some(true) {
                argv.push("true".to_string());
            }
        }
    }
    Ok(argv)
}

/// Nombre de política/auditoría para una ruta resuelta (R-6c):
/// `family:sub path` con subcomando, `family` pelado para familias hoja.
///
/// Una entrada de allowlist `doctor` cubre a la vez la tool builtin y la
/// hoja despachada vía meta-tool — misma operación, colisión benigna.
fn policy_name(family: &str, tokens: &[String]) -> String {
    if tokens.is_empty() {
        family.to_string()
    } else {
        format!("{family}:{}", tokens.join(" "))
    }
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
    confirmations: Arc<ConfirmationStore>,
}

impl RunCommandHandler {
    /// Resolución común a `call` y `try_run`: valida family/subcommand contra
    /// el árbol, rechaza familias prohibidas y clasifica el comando.
    fn resolve<'a>(
        tree: &'a Value,
        args: &'a Value,
    ) -> Result<(&'a str, Vec<String>, &'a Value, ToolKind)> {
        let family = args
            .get("family")
            .and_then(|v| v.as_str())
            .ok_or_else(|| err("missing required 'family'"))?;
        if is_forbidden_family(family) {
            return Err(EnolaError::SecurityError(format!(
                "family '{family}' cannot be dispatched through enola_run_command"
            )));
        }
        let subcommand = args.get("subcommand").and_then(|v| v.as_str());
        let family_node = find_family(tree, family)?;
        let (tokens, leaf) = resolve_leaf(family_node, subcommand)?;
        let token_refs: Vec<&str> = tokens.iter().map(String::as_str).collect();
        if is_forbidden_path(family, &token_refs) {
            return Err(EnolaError::SecurityError(format!(
                "command '{family} {}' cannot be dispatched through enola_run_command",
                tokens.join(" ")
            )));
        }
        if let Some(flag) = denied_flag(family, &tokens, args) {
            return Err(EnolaError::SecurityError(format!(
                "flag '--{flag}' cannot be passed to '{family} {}' through enola_run_command",
                tokens.join(" ")
            )));
        }
        let kind = {
            let refs: Vec<&str> = tokens.iter().map(String::as_str).collect();
            classify(family, &refs)
        };
        Ok((family, tokens, leaf, kind))
    }
}

#[async_trait::async_trait]
impl ToolHandler for RunCommandHandler {
    #[tracing::instrument(
        skip_all,
        fields(family = tracing::field::Empty, command = tracing::field::Empty)
    )]
    async fn call(&self, args: Value) -> Result<Value> {
        let tree = load_tree();
        let (family, tokens, leaf, kind) = Self::resolve(&tree, &args)?;
        let span = tracing::Span::current();
        span.record("family", family);
        span.record("command", tokens.join(" ").as_str());

        // Política de escritura (Fase 7): el nombre evaluado es `family:sub`
        // (`family` pelado en hojas, R-6c) para que la allowlist pueda limitar
        // comandos concretos. La llamada mutante/destructiva queda auditada
        // aunque la meta-tool sea ReadOnly.
        let policy_name = policy_name(family, &tokens);
        if let Err(e) = self.policy.allows(kind, &policy_name) {
            if kind.writes() {
                self.audit.log_call(&policy_name, &args, "rejected");
            }
            return Err(e);
        }
        if kind.writes() {
            self.audit.log_call(&policy_name, &args, "requested");
        }

        let flag_argv = build_flag_argv(leaf, args.get("args").unwrap_or(&Value::Null))?;
        let argv = build_argv(family, &tokens, flag_argv);

        // MCP-009: las destructivas exigen confirmación de dos pasos vinculada
        // a (comando + args). `force` solo no basta: sin token válido se
        // devuelve un challenge, nunca se ejecuta.
        if kind == ToolKind::Destructive {
            // R-13: si la hoja declara una flag booleana `force`, el llamante
            // debe pasarla a true — se valida ANTES de emitir el challenge,
            // para no autorizar argv que el propio comando rechazaría ni
            // dejar el token como única barrera donde el CLI exige doble gate.
            let leaf_requires_force = leaf["flags"]
                .as_array()
                .map(|f| {
                    f.iter().any(|fl| {
                        fl["name"].as_str() == Some("force")
                            && fl["boolean"].as_bool() == Some(true)
                    })
                })
                .unwrap_or(false);
            let force_given = args
                .get("args")
                .and_then(|a| a.get("force"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if leaf_requires_force && !force_given {
                return Err(err(format!(
                    "destructive command '{policy_name}' requires \"force\": true in 'args'"
                )));
            }
            match args.get("confirm_token").and_then(|v| v.as_str()) {
                Some(token) => {
                    if let Err(e) = self.confirmations.consume(token, &policy_name, &args) {
                        self.audit
                            .log_call_op(&policy_name, &args, "rejected", Some(token));
                        return Err(e);
                    }
                    self.audit
                        .log_call_op(&policy_name, &args, "authorized", Some(token));
                }
                None => {
                    let token = self.confirmations.create(&policy_name, &args)?;
                    self.audit.log_call_op(
                        &policy_name,
                        &args,
                        "confirmation_requested",
                        Some(&token),
                    );
                    return Ok(confirmation_required_result(&policy_name, &args, &token));
                }
            }
        }

        let timeout = args
            .get("timeout_secs")
            .and_then(|v| v.as_u64())
            .map(|s| Duration::from_secs(s.clamp(1, MAX_TIMEOUT_SECS)))
            .unwrap_or(DEFAULT_TIMEOUT);

        let out = self.runner.run(&argv, timeout).await;
        if kind.writes() {
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

    /// MCP-009 — `try_run`: devuelve el argv que se ejecutaría y la
    /// clasificación del comando SIN despachar el subproceso. Aplica las
    /// mismas validaciones (familia, subcomando, flags) y la misma política
    /// de escritura; no crea confirmaciones ni toca el sistema.
    async fn try_run(&self, args: Value) -> Result<Value> {
        let tree = load_tree();
        let (family, tokens, leaf, kind) = Self::resolve(&tree, &args)?;
        let policy_name = policy_name(family, &tokens);
        self.policy.allows(kind, &policy_name)?;

        let flag_argv = build_flag_argv(leaf, args.get("args").unwrap_or(&Value::Null))?;
        let argv = build_argv(family, &tokens, flag_argv);

        if kind.writes() {
            self.audit.log_call(&policy_name, &args, "simulated");
        }
        Ok(json!({
            "simulated": true,
            "reliable": true,
            "operation": policy_name,
            "argv": argv,
            "kind": match kind {
                ToolKind::ReadOnly => "read_only",
                ToolKind::Mutating => "mutating",
                ToolKind::Destructive => "destructive",
            },
            "limitations": "the subprocess is not executed; exit code and side effects \
                            are not predicted",
        }))
    }
}

/// Registra la meta-tool `enola_run_command` en el registry.
pub(crate) fn register(
    registry: &mut crate::application::mcp::registry::ToolRegistry,
    runner: Arc<dyn ProcessRunnerPort + Send + Sync>,
    policy: Arc<McpPolicy>,
    audit: Arc<AuditLogger>,
    confirmations: Arc<ConfirmationStore>,
) -> Result<()> {
    use crate::domain::tool::ToolSpec;
    let spec = ToolSpec::new(
        "enola_run_command",
        "Run any enola-cli command from the generated catalog. Mutating commands \
         require --allow-write (+ allowlist); destructive ones also need a \
         single-use confirm_token (first call returns the challenge). \
         'subcommand' accepts level-3 paths like 'wp create'. 'args' is an \
         object of flag values. 'try_run': true returns the argv without \
         executing. Always forces --format json.",
        json!({
            "type": "object",
            "properties": {
                "family": {"type": "string", "minLength": 1, "maxLength": 64},
                "subcommand": {"type": "string", "maxLength": 128},
                "args": {"type": "object"},
                "timeout_secs": {"type": "integer", "minimum": 1, "maximum": MAX_TIMEOUT_SECS},
                "try_run": {"type": "boolean"},
                "confirm_token": {"type": "string", "maxLength": 128}
            },
            "required": ["family"],
            "additionalProperties": false
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
            confirmations,
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
            confirmations: Arc::new(ConfirmationStore::new()),
        }
    }

    #[test]
    fn policy_name_uses_bare_family_for_leaves() {
        // R-6c (F-9): `doctor`/`web`/`uninstall` se nombran `family`, no
        // `family:`. La colisión con el builtin `doctor` es benigna (misma
        // operación, misma entrada de allowlist).
        assert_eq!(policy_name("doctor", &[]), "doctor");
        assert_eq!(policy_name("wp", &["create".to_string()]), "wp:create");
        assert_eq!(
            policy_name("tor", &["auth".to_string(), "revoke".to_string()]),
            "tor:auth revoke"
        );
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
    async fn rejects_flag_values_starting_with_dash() {
        // R-6a (F-13): un valor `-x` podría reinterpretarse como flag por
        // clap; se rechaza en validación, no por el parser del hijo.
        // `wp status --name` es posicional: el valor viaja suelto en argv.
        let mock = MockProcessRunnerPort::new(); // sin dispatch esperado
        let h = handler_with(mock);
        for args in [
            json!({"family": "wp", "subcommand": "status",
                   "args": {"name": "--force"}}),
            json!({"family": "wp", "subcommand": "status",
                   "args": {"name": "-rf"}}),
        ] {
            let r = h.call(args.clone()).await;
            let e = r.expect_err("dash value must be rejected");
            assert!(
                e.to_string().contains("must not start with '-'"),
                "got: {e}"
            );
        }
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
        // añade sola junto al `--format json` global. `--json` es SetTrue en
        // clap → se emite pelada (R-11).
        let mock = ok_runner(vec![
            "--format".into(),
            "json".into(),
            "ports".into(),
            "list".into(),
            "--json".into(),
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
        // `{"json": false}` con flag SetTrue → la flag se omite del argv
        // (R-11); la meta-tool no la re-añade porque el caller la pasó.
        let mock = ok_runner(vec![
            "--format".into(),
            "json".into(),
            "ports".into(),
            "list".into(),
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
        // `--json` es SetTrue → argv lleva la flag pelada (R-11).
        let mock = ok_runner(vec![
            "--format".into(),
            "json".into(),
            "ports".into(),
            "list".into(),
            "--json".into(),
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
        // `wp start` sigue siendo Mutating (tor create pasó a Destructive, R-2).
        // `name` es flag posicional en wp start → argv posicional.
        let mock = ok_runner(vec![
            "--format".into(),
            "json".into(),
            "wp".into(),
            "start".into(),
            "svc".into(),
        ]);
        let policy = McpPolicy {
            allow_write: true,
            ..Default::default()
        };
        let h = handler_with_policy(mock, policy);
        let r = h
            .call(json!({
                "family": "wp", "subcommand": "start",
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
                "family": "wp", "subcommand": "start",
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
        // La tool no añade --force por su cuenta: el argv solo lleva lo que el
        // llamador pasó explícitamente. `tor auth revoke` es Destructive sin
        // flag `force` declarada → tras el challenge el argv va sin ella.
        let mock = ok_runner(vec![
            "--format".into(),
            "json".into(),
            "tor".into(),
            "auth".into(),
            "revoke".into(),
            "svc".into(),
            "--client".into(),
            "x".into(),
        ]);
        let policy = McpPolicy {
            allow_write: true,
            ..Default::default()
        };
        let h = handler_with_policy(mock, policy);
        // Paso 1: challenge de confirmación (sin ejecutar).
        let challenge = h
            .call(json!({
                "family": "tor", "subcommand": "auth revoke",
                "args": {"service": "svc", "client": "x"}
            }))
            .await
            .unwrap();
        let token = challenge["confirm_token"].as_str().unwrap().to_string();
        // Paso 2: confirmación con los mismos args efectivos.
        let r = h
            .call(json!({
                "family": "tor", "subcommand": "auth revoke",
                "args": {"service": "svc", "client": "x"},
                "confirm_token": token
            }))
            .await
            .unwrap();
        let argv: Vec<String> = serde_json::from_value(r["argv"].clone()).unwrap();
        assert!(!argv.iter().any(|a| a == "--force" || a == "--yes"));
    }

    // ── MCP-009: confirmación de dos pasos, try_run y familias prohibidas ──

    #[tokio::test]
    async fn destructive_without_token_returns_challenge_not_execution() {
        let mock = MockProcessRunnerPort::new(); // sin dispatch esperado
        let policy = McpPolicy {
            allow_write: true,
            ..Default::default()
        };
        let h = handler_with_policy(mock, policy);
        let r = h
            .call(json!({
                "family": "wp", "subcommand": "delete",
                "args": {"name": "site", "force": true}
            }))
            .await
            .unwrap();
        assert_eq!(r["confirmation_required"], true);
        assert!(r["confirm_token"].as_str().unwrap().len() >= 32);
        assert_eq!(r["tool"], "wp:delete");
        assert_eq!(r["params"]["args"]["name"], "site");
    }

    #[tokio::test]
    async fn destructive_rejects_unknown_or_foreign_token() {
        let mock = MockProcessRunnerPort::new();
        let policy = McpPolicy {
            allow_write: true,
            ..Default::default()
        };
        let h = handler_with_policy(mock, policy);
        // Token desconocido → error, sin dispatch.
        assert!(h
            .call(json!({
                "family": "wp", "subcommand": "delete",
                "args": {"name": "site", "force": true},
                "confirm_token": "deadbeef"
            }))
            .await
            .is_err());
        // Token válido pero args distintos → mismatch.
        let challenge = h
            .call(json!({
                "family": "wp", "subcommand": "delete",
                "args": {"name": "site", "force": true}
            }))
            .await
            .unwrap();
        let token = challenge["confirm_token"].as_str().unwrap().to_string();
        assert!(h
            .call(json!({
                "family": "wp", "subcommand": "delete",
                "args": {"name": "OTHER", "force": true},
                "confirm_token": token
            }))
            .await
            .is_err());
    }

    #[tokio::test]
    async fn destructive_token_is_single_use() {
        // `name` es posicional y `force` SetTrue → argv: `wp delete site --force`.
        let mock = ok_runner(vec![
            "--format".into(),
            "json".into(),
            "wp".into(),
            "delete".into(),
            "site".into(),
            "--force".into(),
        ]);
        let policy = McpPolicy {
            allow_write: true,
            ..Default::default()
        };
        let h = handler_with_policy(mock, policy);
        let challenge = h
            .call(json!({
                "family": "wp", "subcommand": "delete",
                "args": {"name": "site", "force": true}
            }))
            .await
            .unwrap();
        let token = challenge["confirm_token"].as_str().unwrap().to_string();
        h.call(json!({
            "family": "wp", "subcommand": "delete",
            "args": {"name": "site", "force": true},
            "confirm_token": token
        }))
        .await
        .unwrap();
        // Replay: el mock ya consumió su expectativa (.times(1) implícito) y el
        // token está gastado — el error llega antes del dispatch.
        assert!(h
            .call(json!({
                "family": "wp", "subcommand": "delete",
                "args": {"name": "site", "force": true},
                "confirm_token": token
            }))
            .await
            .is_err());
    }

    #[tokio::test]
    async fn destructive_still_requires_allow_write() {
        // Sin allow_write ni siquiera se emite el challenge.
        let mock = MockProcessRunnerPort::new();
        let h = handler_with(mock);
        assert!(h
            .call(json!({
                "family": "wp", "subcommand": "delete",
                "args": {"name": "site", "force": true}
            }))
            .await
            .is_err());
    }

    #[tokio::test]
    async fn mcp_family_is_never_dispatched() {
        let mock = MockProcessRunnerPort::new();
        let policy = McpPolicy {
            allow_write: true,
            ..Default::default()
        };
        let h = handler_with_policy(mock, policy);
        for args in [
            json!({"family": "mcp"}),
            json!({"family": "mcp", "args": {"allow-write": true}}),
            json!({"family": "mcp", "try_run": true}),
        ] {
            let r = h.call(args.clone()).await;
            assert!(r.is_err(), "mcp family must be rejected: {args}");
        }
        // try_run tampoco despacha la familia prohibida.
        assert!(h.try_run(json!({"family": "mcp"})).await.is_err());
    }

    #[tokio::test]
    async fn try_run_returns_argv_without_dispatching() {
        let mock = MockProcessRunnerPort::new(); // cualquier run() panicaría
        let policy = McpPolicy {
            allow_write: true,
            ..Default::default()
        };
        let h = handler_with_policy(mock, policy);
        let r = h
            .try_run(json!({
                "family": "wp", "subcommand": "create",
                "args": {"name": "blog"}
            }))
            .await
            .unwrap();
        assert_eq!(r["simulated"], true);
        assert_eq!(r["kind"], "mutating");
        let argv: Vec<String> = serde_json::from_value(r["argv"].clone()).unwrap();
        assert_eq!(&argv[..4], ["--format", "json", "wp", "create"]);
    }

    #[tokio::test]
    async fn try_run_applies_write_policy() {
        // Una simulación de comando mutante exige allow_write igual que la
        // ejecución real (los permisos se validan también en simulación).
        let mock = MockProcessRunnerPort::new();
        let h = handler_with(mock);
        assert!(h
            .try_run(json!({"family": "wp", "subcommand": "create", "args": {"name": "x"}}))
            .await
            .is_err());
        let mock2 = MockProcessRunnerPort::new();
        let h2 = handler_with(mock2);
        // Lectura: permitida sin allow_write.
        assert!(h2.try_run(json!({"family": "doctor"})).await.is_ok());
    }

    #[tokio::test]
    async fn try_run_validates_flags_and_never_confirms() {
        let mock = MockProcessRunnerPort::new();
        let policy = McpPolicy {
            allow_write: true,
            ..Default::default()
        };
        let h = handler_with_policy(mock, policy);
        // Flag desconocido → error de validación también en simulación.
        assert!(h
            .try_run(json!({
                "family": "wp", "subcommand": "create",
                "args": {"name": "x", "evil": "y"}
            }))
            .await
            .is_err());
        // Destructiva en try_run: informa sin emitir confirm_token.
        let r = h
            .try_run(json!({
                "family": "wp", "subcommand": "delete",
                "args": {"name": "x", "force": true}
            }))
            .await
            .unwrap();
        assert_eq!(r["kind"], "destructive");
        assert!(r.get("confirm_token").is_none());
    }

    #[tokio::test]
    async fn forbidden_paths_never_dispatch() {
        // R-1: rutas vetadas por path o familia — rechazo aunque la política
        // permita escritura y la allowlist esté vacía. El mock sin
        // expectativas panicaría si se llamara a run().
        let cases: Vec<Value> = vec![
            // Familias vetadas enteras (hoja o con subcomandos).
            json!({"family": "web"}),
            json!({"family": "web", "args": {"port": 8080}}),
            json!({"family": "setup"}),
            json!({"family": "uninstall"}),
            json!({"family": "ssh", "subcommand": "add-key", "args": {"pubkey": "ssh-ed25519 AA"}}),
            json!({"family": "ssh", "subcommand": "deploy-hidden", "args": {"name": "x"}}),
            // Vetos por path dentro de familias válidas.
            json!({"family": "update", "subcommand": "apply"}),
            json!({"family": "update", "subcommand": "apply", "args": {"binary": "/tmp/x"}}),
            json!({"family": "test", "subcommand": "run"}),
            json!({"family": "test", "subcommand": "benchmark"}),
        ];
        for args in cases {
            let mock = MockProcessRunnerPort::new();
            let policy = McpPolicy {
                allow_write: true,
                ..Default::default()
            };
            let h = handler_with_policy(mock, policy);
            let r = h.call(args.clone()).await;
            assert!(r.is_err(), "forbidden path must be rejected: {args}");
            // try_run tampoco resuelve la ruta vetada.
            let mock2 = MockProcessRunnerPort::new();
            let h2 = handler_with_policy(
                mock2,
                McpPolicy {
                    allow_write: true,
                    ..Default::default()
                },
            );
            assert!(
                h2.try_run(args.clone()).await.is_err(),
                "try_run must reject forbidden path: {args}"
            );
        }
    }

    #[tokio::test]
    async fn denied_flags_are_rejected() {
        // R-1 defensa en profundidad: `--binary` y `--allow-unsigned` en
        // update download/apply no son expresables vía la meta-tool.
        // (`apply` ya está vetado por path; el test documenta ambas capas.)
        for args in [
            json!({"family": "update", "subcommand": "download",
                   "args": {"allow-unsigned": "1"}}),
            json!({"family": "update", "subcommand": "download",
                   "args": {"allow-unsigned": true}}),
        ] {
            let mock = MockProcessRunnerPort::new();
            let policy = McpPolicy {
                allow_write: true,
                ..Default::default()
            };
            let h = handler_with_policy(mock, policy);
            assert!(
                h.call(args.clone()).await.is_err(),
                "denied flag must be rejected: {args}"
            );
        }
        // Una flag permitida en la misma hoja sigue siendo rechazable solo
        // por política, no por la denylist: `update download` es Destructive
        // (con `--force` declarada → R-13 la exige) → sin token devuelve
        // challenge, no error de flag.
        let mock = MockProcessRunnerPort::new();
        let policy = McpPolicy {
            allow_write: true,
            ..Default::default()
        };
        let h = handler_with_policy(mock, policy);
        let r = h
            .call(json!({
                "family": "update", "subcommand": "download",
                "args": {"force": true}
            }))
            .await
            .unwrap();
        assert!(r.get("confirm_token").is_some());
    }

    #[tokio::test]
    async fn destructive_requires_force_flag_before_challenge() {
        // R-13: `wp delete` declara `--force` → el challenge no se emite sin
        // `force: true`. `tor auth revoke` no declara force → challenge normal.
        let mock = MockProcessRunnerPort::new();
        let policy = McpPolicy {
            allow_write: true,
            ..Default::default()
        };
        let h = handler_with_policy(mock, policy);
        // Sin force → ValidationError, sin token emitido.
        let r = h
            .call(json!({
                "family": "wp", "subcommand": "delete",
                "args": {"name": "site"}
            }))
            .await;
        assert!(r.is_err());
        assert!(r.unwrap_err().to_string().contains("requires \"force\""));
        // force:false → igual.
        let mock = MockProcessRunnerPort::new();
        let h = handler_with_policy(
            mock,
            McpPolicy {
                allow_write: true,
                ..Default::default()
            },
        );
        assert!(h
            .call(json!({
                "family": "wp", "subcommand": "delete",
                "args": {"name": "site", "force": false}
            }))
            .await
            .is_err());
        // Hoja Destructive sin flag force (`tor auth revoke`) → challenge sin
        // requerir force.
        let mock = MockProcessRunnerPort::new();
        let h = handler_with_policy(
            mock,
            McpPolicy {
                allow_write: true,
                ..Default::default()
            },
        );
        let r = h
            .call(json!({
                "family": "tor", "subcommand": "auth revoke",
                "args": {"service": "svc", "client": "x"}
            }))
            .await
            .unwrap();
        assert_eq!(r["confirmation_required"], true);
    }

    #[tokio::test]
    async fn boolean_flags_emit_bare_flag_and_omit_false() {
        // R-11: las flags SetTrue se despachan como `--flag` pelado; `false`
        // omite la flag; valores no-booleanos se rechazan con ValidationError
        // (no con un error de clap del hijo).
        // `wp delete` es Destructive → usar try_run para inspeccionar el argv.
        let mock = MockProcessRunnerPort::new();
        let policy = McpPolicy {
            allow_write: true,
            ..Default::default()
        };
        let h = handler_with_policy(mock, policy);
        let r = h
            .try_run(json!({
                "family": "wp", "subcommand": "delete",
                "args": {"name": "site", "force": true}
            }))
            .await
            .unwrap();
        assert_eq!(
            r["argv"],
            json!(["--format", "json", "wp", "delete", "site", "--force"])
        );

        // force:false → la flag no aparece.
        let mock = MockProcessRunnerPort::new();
        let h = handler_with_policy(
            mock,
            McpPolicy {
                allow_write: true,
                ..Default::default()
            },
        );
        let r = h
            .try_run(json!({
                "family": "wp", "subcommand": "delete",
                "args": {"name": "site", "force": false}
            }))
            .await
            .unwrap();
        assert_eq!(
            r["argv"],
            json!(["--format", "json", "wp", "delete", "site"])
        );

        // Valor no-booleano → error de validación.
        let mock = MockProcessRunnerPort::new();
        let h = handler_with_policy(
            mock,
            McpPolicy {
                allow_write: true,
                ..Default::default()
            },
        );
        assert!(h
            .try_run(json!({
                "family": "wp", "subcommand": "delete",
                "args": {"name": "site", "force": "yes"}
            }))
            .await
            .is_err());
    }

    #[tokio::test]
    async fn non_forbidden_paths_still_dispatch() {
        // Regresión: el veto no debe cerrar rutas legítimas.
        // (`update check` tiene flag nativa --json SetTrue → `--json` pelado.)
        let mock = ok_runner(vec![
            "--format".into(),
            "json".into(),
            "update".into(),
            "check".into(),
            "--json".into(),
        ]);
        let h = handler_with(mock);
        let r = h
            .call(json!({"family": "update", "subcommand": "check"}))
            .await
            .unwrap();
        assert_eq!(r["exit_code"], 0);
    }
}
