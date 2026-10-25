// A25-01/02 (P1.1) — Gate único de comandos para las vías que ejecutan el
// propio binario con argv de un tercero: la meta-tool MCP `enola_run_command`
// y la consola web `/api/console/run`.
//
// Causa raíz de A25-01/02: los vetos (`FORBIDDEN_*`, `DENIED_FLAGS`) y la
// clasificación Destructive vivían solo en el brazo MCP ("fix en un brazo,
// no en el otro"). Este módulo es el ÚNICO punto de decisión: ambos brazos
// llaman a `check_family` + `check_route`, y la consola web además a
// `evaluate_argv` (parser de argv libre contra el árbol generado). Puro y sin
// I/O: testeable con un corpus compartido (garantía de paridad, SD-1/SD-2).

use crate::application::mcp::command_catalog::load_tree;
use crate::domain::error::{EnolaError, Result};
use crate::domain::tool::{classify, is_forbidden_family, is_forbidden_path, ToolKind};
use serde_json::Value;

/// Flags vetadas por hoja: (familia, path, nombres largos).
///
/// Defensa en profundidad (auditoría F-01/F-25): `--binary` y
/// `--allow-unsigned` rompen la cadena de firma de updates. Se aplican igual
/// en MCP y en la consola web (las rutas largas se resuelven desde alias
/// cortos antes de comprobarlas).
const DENIED_FLAGS: &[(&str, &[&str], &[&str])] = &[
    ("update", &["download"], &["allow-unsigned"]),
    ("update", &["apply"], &["binary", "allow-unsigned"]),
];

/// Profundidad máxima de la ruta de subcomandos (familia → grupo → hoja).
const MAX_PATH_TOKENS: usize = 2;

/// Flags globales de `Cli` aceptadas en la consola web (solo antes de la
/// familia, como las acepta clap): (largo, corto, toma valor).
///
/// Los overrides globales de URLs (`--binary-base-url`, `--web-url`,
/// `--docs-url`, `--tor-socks`) NO se aceptan: MCP no puede pasarlos (fija
/// él mismo el argv) y redirigen descargas/proxy Tor como root.
const CONSOLE_GLOBAL_FLAGS: &[(&str, &str, bool)] =
    &[("format", "f", true), ("verbose", "v", false)];

/// Overrides globales (`global = true` en clap) vetados en la consola web.
const CONSOLE_DENIED_GLOBALS: &[&str] = &["binary-base-url", "web-url", "docs-url", "tor-socks"];

fn security(msg: String) -> EnolaError {
    EnolaError::SecurityError(msg)
}

fn invalid(msg: impl Into<String>) -> EnolaError {
    EnolaError::ValidationError(msg.into())
}

/// Veto estructural por familia (`mcp`, `ask`, `ssh`, `uninstall`, `web`,
/// `setup`) — con cualquier política ni allowlist.
pub(crate) fn check_family(family: &str, via: &str) -> Result<()> {
    if is_forbidden_family(family) {
        return Err(security(format!(
            "family '{family}' cannot be dispatched through {via}"
        )));
    }
    Ok(())
}

/// Vetos por ruta y por flag + clasificación, sobre una ruta ya resuelta
/// contra el árbol. `flag_names` son nombres largos (sin `--`).
pub(crate) fn check_route<'a>(
    family: &str,
    tokens: &[String],
    flag_names: impl IntoIterator<Item = &'a str>,
    via: &str,
) -> Result<ToolKind> {
    check_family(family, via)?;
    let refs: Vec<&str> = tokens.iter().map(String::as_str).collect();
    if is_forbidden_path(family, &refs) {
        return Err(security(format!(
            "command '{family} {}' cannot be dispatched through {via}",
            tokens.join(" ")
        )));
    }
    if let Some(denied) = DENIED_FLAGS.iter().find_map(|(fam, path, denied)| {
        (*fam == family && path.iter().copied().eq(refs.iter().copied())).then_some(*denied)
    }) {
        if let Some(flag) = flag_names.into_iter().find(|f| denied.contains(f)) {
            return Err(security(format!(
                "flag '--{flag}' cannot be passed to '{family} {}' through {via}",
                tokens.join(" ")
            )));
        }
    }
    Ok(classify(family, &refs))
}

/// Nombre de política/auditoría (R-6c): `family:sub path`, o `family`
/// pelado en familias hoja.
pub(crate) fn policy_name(family: &str, tokens: &[String]) -> String {
    if tokens.is_empty() {
        family.to_string()
    } else {
        format!("{family}:{}", tokens.join(" "))
    }
}

/// F-12: redacta los valores de flags sensibles de un argv — el flag
/// `--admin-pass` se conserva pero su valor sale como "***" (misma tabla
/// `SECRET_KEYS` que el audit log MCP). Usado en respuestas y logs de ambos
/// brazos.
pub(crate) fn redact_argv(argv: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(argv.len());
    let mut redact_next = false;
    for arg in argv {
        if redact_next {
            out.push("***".to_string());
            redact_next = false;
            continue;
        }
        if let Some(flag) = arg.strip_prefix("--") {
            let name = flag.split('=').next().unwrap_or("");
            let lower = name.to_lowercase();
            if crate::application::mcp::audit::SECRET_KEYS
                .iter()
                .any(|k| lower.contains(k))
            {
                if flag.contains('=') {
                    out.push(format!("--{}=***", name));
                } else {
                    out.push(arg.clone());
                    redact_next = true;
                }
                continue;
            }
        }
        out.push(arg.clone());
    }
    out
}

/// Decisión del gate para un argv libre (consola web).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GateDecision {
    /// Se puede ejecutar sin más trámite.
    Allow { kind: ToolKind, policy_name: String },
    /// Destructiva: exige `{"confirm": expected}` (equivalente web del
    /// `confirm_token` MCP / `--force` del CLI).
    NeedsConfirm {
        kind: ToolKind,
        policy_name: String,
        expected: String,
    },
}

/// Resultado del parseo de un argv libre contra el árbol generado.
#[derive(Debug, Default)]
struct ParsedArgv {
    family: String,
    tokens: Vec<String>,
    /// Nombres largos de las flags presentes (alias cortos ya resueltos).
    flags: Vec<String>,
    /// Valor de la flag/posicional `name`, si lo hay (recurso a confirmar).
    name_value: Option<String>,
    /// `--help`/`-h`/`--version`: clap imprime y sale sin ejecutar.
    help: bool,
}

/// Busca la spec de flag por nombre largo o alias corto en un nodo hoja.
fn find_flag<'a>(leaf: &'a Value, long: Option<&str>, short: Option<&str>) -> Option<&'a Value> {
    leaf["flags"].as_array()?.iter().find(|f| {
        f["positional"].as_bool() != Some(true)
            && (long.is_some() && f["name"].as_str() == long
                || short.is_some() && f["short"].as_str() == short)
    })
}

fn has_children(node: &Value) -> bool {
    node["subcommands"]
        .as_array()
        .map(|s| !s.is_empty())
        .unwrap_or(false)
}

/// Parsea un argv libre al modo de clap, de forma ESTRICTA: lo que el gate
/// no entiende se rechaza (fail-closed) en lugar de adivinar cómo lo
/// interpretaría clap.
fn parse_argv(tree: &Value, argv: &[String]) -> Result<ParsedArgv> {
    let mut parsed = ParsedArgv::default();
    let mut i = 0;

    // 1) Flags globales previas a la familia.
    while i < argv.len() && argv[i].starts_with('-') {
        let tok = argv[i].as_str();
        if matches!(tok, "--help" | "-h" | "--version" | "-V") {
            parsed.help = true;
            return Ok(parsed);
        }
        let (long, short, inline) = split_flag(tok)?;
        let spec = CONSOLE_GLOBAL_FLAGS
            .iter()
            .find(|(l, s, _)| Some(*l) == long || Some(*s) == short);
        let Some((_, _, takes_value)) = spec else {
            return Err(denied_global_or_unknown(long.or(short).unwrap_or(tok)));
        };
        i += 1;
        if *takes_value && inline.is_none() {
            let v = argv
                .get(i)
                .ok_or_else(|| invalid(format!("flag '{tok}' needs a value")))?;
            reject_dash_value(tok, v)?;
            i += 1;
        }
    }

    // 2) Familia.
    let family = argv.get(i).ok_or_else(|| invalid("missing command"))?;
    // Veto estructural antes de mirar el árbol (mismo orden que MCP).
    check_family(family, "the web console")?;
    let mut node = tree["families"]
        .as_array()
        .and_then(|fams| fams.iter().find(|f| f["name"].as_str() == Some(family)))
        .ok_or_else(|| invalid(format!("unknown command '{family}'")))?;
    parsed.family = family.clone();
    i += 1;

    // 3) Ruta de subcomandos.
    while has_children(node) {
        let tok = argv.get(i).ok_or_else(|| {
            invalid(format!(
                "incomplete command '{}': a subcommand is required",
                std::iter::once(parsed.family.as_str())
                    .chain(parsed.tokens.iter().map(String::as_str))
                    .collect::<Vec<_>>()
                    .join(" ")
            ))
        })?;
        if matches!(tok.as_str(), "--help" | "-h") {
            parsed.help = true;
            return Ok(parsed);
        }
        if parsed.tokens.len() >= MAX_PATH_TOKENS {
            return Err(invalid("subcommand path too deep"));
        }
        node = node["subcommands"]
            .as_array()
            .and_then(|subs| {
                subs.iter()
                    .find(|s| s["name"].as_str() == Some(tok.as_str()))
            })
            .ok_or_else(|| invalid(format!("unknown subcommand '{tok}'")))?;
        parsed.tokens.push(tok.clone());
        i += 1;
    }
    let leaf = node;

    // 4) Flags y posicionales de la hoja.
    let positionals: Vec<&Value> = leaf["flags"]
        .as_array()
        .map(|f| {
            f.iter()
                .filter(|f| f["positional"].as_bool() == Some(true))
                .collect()
        })
        .unwrap_or_default();
    let mut pos_idx = 0;
    while i < argv.len() {
        let tok = argv[i].as_str();
        i += 1;
        if tok == "--" {
            return Err(invalid("'--' is not accepted in the web console"));
        }
        if matches!(tok, "--help" | "-h") {
            parsed.help = true;
            continue;
        }
        if !tok.starts_with('-') || tok == "-" {
            // Posicional.
            if let Some(spec) = positionals.get(pos_idx) {
                if spec["name"].as_str() == Some("name") {
                    parsed.name_value = Some(tok.to_string());
                }
            }
            pos_idx += 1;
            continue;
        }
        let (long, short, inline) = split_flag(tok)?;
        if long.is_some_and(|l| CONSOLE_DENIED_GLOBALS.contains(&l)) {
            return Err(denied_global_or_unknown(long.unwrap_or(tok)));
        }
        let spec =
            find_flag(leaf, long, short).ok_or_else(|| invalid(format!("unknown flag '{tok}'")))?;
        let name = spec["name"].as_str().unwrap_or("").to_string();
        let value = if spec["boolean"].as_bool() == Some(true) {
            if inline.is_some() {
                return Err(invalid(format!("flag '--{name}' takes no value")));
            }
            None
        } else {
            let v = match inline {
                Some(v) => v.to_string(),
                None => {
                    let v = argv
                        .get(i)
                        .ok_or_else(|| invalid(format!("flag '--{name}' needs a value")))?;
                    i += 1;
                    v.clone()
                }
            };
            reject_dash_value(tok, &v)?;
            Some(v)
        };
        if name == "name" {
            parsed.name_value = value;
        }
        parsed.flags.push(name);
    }
    Ok(parsed)
}

/// Divide un token de flag en (largo, corto, valor inline).
fn split_flag(tok: &str) -> Result<(Option<&str>, Option<&str>, Option<&str>)> {
    if let Some(rest) = tok.strip_prefix("--") {
        let (name, inline) = match rest.split_once('=') {
            Some((n, v)) => (n, Some(v)),
            None => (rest, None),
        };
        if name.is_empty() || name.contains(char::is_whitespace) {
            return Err(invalid(format!("invalid flag '{tok}'")));
        }
        return Ok((Some(name), None, inline));
    }
    let rest = &tok[1..];
    // Solo alias cortos simples (`-n`): agrupados (`-yf`) o con valor pegado
    // (`-nfoo`) cambiarían el significado según la acción clap → rechazo.
    if rest.chars().count() != 1 {
        return Err(invalid(format!(
            "short flag '{tok}' not supported in the web console; use the long form"
        )));
    }
    Ok((None, Some(rest), None))
}

/// F-13: un valor que empieza por '-' se interpretaría como otra flag.
fn reject_dash_value(flag: &str, value: &str) -> Result<()> {
    if value.starts_with('-') {
        return Err(invalid(format!(
            "value for '{flag}' must not start with '-'"
        )));
    }
    Ok(())
}

fn denied_global_or_unknown(flag: &str) -> EnolaError {
    if CONSOLE_DENIED_GLOBALS.contains(&flag) {
        security(format!(
            "global override '--{flag}' cannot be passed through the web console"
        ))
    } else {
        invalid(format!("unknown global flag '{flag}'"))
    }
}

/// Evalúa un argv libre de la consola web contra el MISMO gate que MCP.
///
/// - `Err(SecurityError)` → vetado (403): familia/ruta/flag prohibida.
/// - `Err(ValidationError)` → argv no reconocible (400).
/// - `Ok(NeedsConfirm)` → Destructive: exige `confirm` (409 si falta).
/// - `Ok(Allow)` → ejecutar.
pub(crate) fn evaluate_argv(argv: &[String]) -> Result<GateDecision> {
    let tree = load_tree();
    evaluate_argv_in(&tree, argv)
}

fn evaluate_argv_in(tree: &Value, argv: &[String]) -> Result<GateDecision> {
    if argv.is_empty() {
        return Err(invalid("missing command"));
    }
    let parsed = parse_argv(tree, argv)?;
    if parsed.family.is_empty() {
        // `--help`/`--version` global: clap imprime y sale.
        return Ok(GateDecision::Allow {
            kind: ToolKind::ReadOnly,
            policy_name: "help".to_string(),
        });
    }
    let kind = check_route(
        &parsed.family,
        &parsed.tokens,
        parsed.flags.iter().map(String::as_str),
        "the web console",
    )?;
    let policy_name = policy_name(&parsed.family, &parsed.tokens);
    if parsed.help {
        // clap imprime la ayuda y sale antes de ejecutar nada.
        return Ok(GateDecision::Allow {
            kind: ToolKind::ReadOnly,
            policy_name,
        });
    }
    if kind != ToolKind::Destructive {
        return Ok(GateDecision::Allow { kind, policy_name });
    }
    // R-13 (paridad con MCP): si la hoja declara `--force`, debe venir.
    let leaf_requires_force = leaf_of(tree, &parsed.family, &parsed.tokens)
        .and_then(|l| l["flags"].as_array())
        .map(|f| {
            f.iter().any(|fl| {
                fl["name"].as_str() == Some("force") && fl["boolean"].as_bool() == Some(true)
            })
        })
        .unwrap_or(false);
    if leaf_requires_force && !parsed.flags.iter().any(|f| f == "force") {
        return Err(invalid(format!(
            "destructive command '{policy_name}' requires --force"
        )));
    }
    let expected = parsed.name_value.unwrap_or_else(|| policy_name.clone());
    Ok(GateDecision::NeedsConfirm {
        kind,
        policy_name,
        expected,
    })
}

fn leaf_of<'a>(tree: &'a Value, family: &str, tokens: &[String]) -> Option<&'a Value> {
    let mut node = tree["families"]
        .as_array()?
        .iter()
        .find(|f| f["name"].as_str() == Some(family))?;
    for t in tokens {
        node = node["subcommands"]
            .as_array()?
            .iter()
            .find(|s| s["name"].as_str() == Some(t.as_str()))?;
    }
    Some(node)
}

/// Vetos expuestos a la UI (`/console_commands.json`, P1.7): familias y
/// rutas que ninguna vía de terceros (MCP/consola web) despacha.
pub(crate) fn forbidden_for_ui() -> Value {
    use crate::domain::tool::{FORBIDDEN_FAMILIES, FORBIDDEN_PATHS};
    serde_json::json!({
        "families": FORBIDDEN_FAMILIES,
        "paths": FORBIDDEN_PATHS
            .iter()
            .map(|(f, p)| std::iter::once(*f).chain(p.iter().copied()).collect::<Vec<_>>().join(" "))
            .collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    fn is_security(r: &Result<GateDecision>) -> bool {
        matches!(r, Err(EnolaError::SecurityError(_)))
    }

    fn is_invalid(r: &Result<GateDecision>) -> bool {
        matches!(r, Err(EnolaError::ValidationError(_)))
    }

    #[test]
    fn forbidden_families_are_vetoed_in_console() {
        for cmd in [
            "web",
            "web --port 9000",
            "mcp",
            "mcp --allow-write",
            "setup",
            "setup --pqc-tls",
            "uninstall",
            "ask hola",
            "ssh add-key",
        ] {
            assert!(
                is_security(&evaluate_argv(&argv(cmd))),
                "{cmd} debe vetarse"
            );
        }
    }

    #[test]
    fn forbidden_paths_are_vetoed_in_console() {
        for cmd in [
            "update apply",
            "update apply --binary /tmp/x --allow-unsigned",
            "test run",
            "test benchmark",
            "ai finetune wizard",
        ] {
            assert!(
                is_security(&evaluate_argv(&argv(cmd))),
                "{cmd} debe vetarse"
            );
        }
    }

    #[test]
    fn denied_flags_are_vetoed_in_console() {
        let r = evaluate_argv(&argv("update download --allow-unsigned --force"));
        assert!(is_security(&r), "{r:?}");
    }

    #[test]
    fn global_url_overrides_are_vetoed() {
        for cmd in [
            "--tor-socks socks5h://10.0.0.5:9050 tor list",
            "tor list --tor-socks socks5h://10.0.0.5:9050",
            "--binary-base-url http://evil update check",
            "update check --binary-base-url=http://evil",
        ] {
            assert!(
                is_security(&evaluate_argv(&argv(cmd))),
                "{cmd} debe vetarse"
            );
        }
    }

    #[test]
    fn readonly_commands_are_allowed() {
        let r = evaluate_argv(&argv("tor list")).unwrap();
        assert!(matches!(
            r,
            GateDecision::Allow {
                kind: ToolKind::ReadOnly,
                ..
            }
        ));
        let r = evaluate_argv(&argv("--format json doctor")).unwrap();
        assert!(matches!(
            r,
            GateDecision::Allow {
                kind: ToolKind::ReadOnly,
                ..
            }
        ));
    }

    #[test]
    fn destructive_requires_confirm_bound_to_resource() {
        let r = evaluate_argv(&argv("tor remove x --force")).unwrap();
        match r {
            GateDecision::NeedsConfirm { expected, kind, .. } => {
                assert_eq!(kind, ToolKind::Destructive);
                assert_eq!(expected, "x");
            }
            other => panic!("expected NeedsConfirm, got {other:?}"),
        }
    }

    #[test]
    fn destructive_leaf_with_force_flag_requires_it() {
        assert!(is_invalid(&evaluate_argv(&argv("tor remove x"))));
    }

    #[test]
    fn help_is_readonly_but_not_for_forbidden_families() {
        let r = evaluate_argv(&argv("tor remove --help")).unwrap();
        assert!(matches!(
            r,
            GateDecision::Allow {
                kind: ToolKind::ReadOnly,
                ..
            }
        ));
        let r = evaluate_argv(&argv("tor --help")).unwrap();
        assert!(matches!(
            r,
            GateDecision::Allow {
                kind: ToolKind::ReadOnly,
                ..
            }
        ));
        assert!(is_security(&evaluate_argv(&argv("web --help"))));
    }

    #[test]
    fn strict_parse_rejects_ambiguous_input() {
        for cmd in [
            "tor remove -- --help",
            "tor nope",
            "nope",
            "tor list --evil",
            "tor create --name -x",
            "tor",
        ] {
            let r = evaluate_argv(&argv(cmd));
            assert!(r.is_err(), "{cmd} debe rechazarse: {r:?}");
        }
    }

    #[test]
    fn redact_argv_masks_secret_values() {
        let out = redact_argv(&argv("wp create --name a --admin-pass hunter2"));
        assert!(!out.contains(&"hunter2".to_string()));
        assert!(out.contains(&"***".to_string()));
    }

    /// P1 — paridad permanente web ↔ MCP: para un corpus compartido, la
    /// decisión de la consola web (argv libre) y la de `enola_run_command`
    /// (JSON estructurado) coinciden en veto y clasificación.
    #[test]
    fn web_console_and_mcp_agree_on_shared_corpus() {
        use serde_json::json;
        // (family, subcommand, args MCP, argv web equivalente)
        let corpus: Vec<(&str, &str, Value, &str)> = vec![
            ("tor", "list", json!({}), "tor list"),
            ("doctor", "", json!({}), "doctor"),
            (
                "tor",
                "remove",
                json!({"name": "x", "force": true}),
                "tor remove x --force",
            ),
            ("update", "check", json!({}), "update check"),
            ("update", "apply", json!({}), "update apply"),
            (
                "update",
                "download",
                json!({"allow-unsigned": true}),
                "update download --allow-unsigned",
            ),
            (
                "update",
                "download",
                json!({"force": true}),
                "update download --force",
            ),
            ("test", "run", json!({}), "test run"),
            ("test", "benchmark", json!({}), "test benchmark"),
            ("test", "list", json!({}), "test list"),
            ("web", "", json!({}), "web"),
            ("mcp", "", json!({}), "mcp"),
            ("setup", "", json!({}), "setup"),
            ("uninstall", "", json!({}), "uninstall"),
            ("ssh", "add-key", json!({}), "ssh add-key"),
            ("ai", "finetune wizard", json!({}), "ai finetune wizard"),
            ("ai", "list", json!({}), "ai list"),
            ("firewall", "status", json!({}), "firewall status"),
            ("ports", "", json!({}), "ports"),
        ];
        for (family, sub, args, web) in corpus {
            let mut mcp_args = json!({"family": family, "args": args});
            if !sub.is_empty() {
                mcp_args["subcommand"] = json!(sub);
            }
            let mcp = crate::application::mcp::run_command::resolve_kind_for_parity(&mcp_args);
            let web_r = evaluate_argv(&argv(web));
            match (&mcp, &web_r) {
                (Err(EnolaError::SecurityError(_)), Err(EnolaError::SecurityError(_))) => {}
                (Err(EnolaError::ValidationError(_)), Err(EnolaError::ValidationError(_))) => {}
                (Ok(k), Ok(GateDecision::Allow { kind, .. })) => {
                    assert_eq!(k, kind, "kind diverge para '{web}'")
                }
                (Ok(k), Ok(GateDecision::NeedsConfirm { kind, .. })) => {
                    assert_eq!(*k, ToolKind::Destructive, "'{web}'");
                    assert_eq!(k, kind, "kind diverge para '{web}'");
                }
                _ => panic!("paridad rota para '{web}': mcp={mcp:?} web={web_r:?}"),
            }
        }
    }

    #[test]
    fn every_forbidden_family_is_vetoed_by_both_arms() {
        use crate::domain::tool::FORBIDDEN_FAMILIES;
        for fam in FORBIDDEN_FAMILIES {
            assert!(is_security(&evaluate_argv(&[fam.to_string()])), "{fam}");
            let mcp = crate::application::mcp::run_command::resolve_kind_for_parity(
                &serde_json::json!({"family": fam}),
            );
            assert!(matches!(mcp, Err(EnolaError::SecurityError(_))), "{fam}");
        }
    }

    #[test]
    fn forbidden_for_ui_lists_families_and_paths() {
        let v = forbidden_for_ui();
        assert!(v["families"].as_array().unwrap().iter().any(|f| f == "web"));
        assert!(v["paths"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p == "update apply"));
    }
}
