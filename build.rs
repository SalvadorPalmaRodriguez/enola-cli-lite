// build.rs — Script de compilación para enola-cli
// Embebe metadatos de build: git commit, fecha y versión.
//
// INT-008: Estos valores se usan en check_self_integrity() en main.rs.
//
// LAUNCH-016 (2026-04-29): Builds reproducibles vía SOURCE_DATE_EPOCH
// (https://reproducible-builds.org/specs/source-date-epoch/).
//
// Cadena de prioridad para ENOLA_BUILD_DATE:
//   1. SOURCE_DATE_EPOCH env var (UNIX seconds, UTC) — estándar reproducible.
//   2. `git log -1 --format=%ct HEAD` — determinístico por commit.
//   3. `date -u +%Y-%m-%d` — fallback en entornos sin git ni env var (NO reproducible).
//
// Garantía: dos `cargo build --release` desde el mismo HEAD con el mismo
// SOURCE_DATE_EPOCH (o sin él, pero con el mismo HEAD) producen el mismo
// valor embebido de ENOLA_BUILD_DATE. Combinado con --remap-path-prefix
// (.cargo/config.toml, LAUNCH-007) y panic=abort+strip (Cargo.toml), el
// binario distribuido es reproducible bit-a-bit en una toolchain idéntica.

use sha2::{Digest, Sha256};
use std::process::Command;

// CLI definitions — included here so build.rs can introspect clap's command tree
// and generate assets/console_commands.json automatically at compile time.
#[path = "src/cli/defs.rs"]
mod defs;
use clap::CommandFactory;

fn format_date_utc(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let year = if m <= 2 { y + 1 } else { y };
    format!("{:04}-{:02}-{:02}", year, m, d)
}

fn main() {
    // ── Git commit hash (short, 8 chars) ──────────────────────────────
    let git_hash = Command::new("git")
        .args(["rev-parse", "--short=8", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string());

    // ── Build date (reproducible vía SOURCE_DATE_EPOCH) ──────────────
    let build_epoch: Option<i64> = std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|s| s.trim().parse::<i64>().ok())
        .or_else(|| {
            // Fallback: timestamp del commit HEAD (determinístico por commit).
            Command::new("git")
                .args(["log", "-1", "--format=%ct", "HEAD"])
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .and_then(|s| s.trim().parse::<i64>().ok())
        });

    let build_date = match build_epoch {
        Some(secs) => format_date_utc(secs),
        None => {
            // Último recurso: fecha actual UTC (NO reproducible).
            // Solo se alcanza sin SOURCE_DATE_EPOCH y sin .git.
            Command::new("date")
                .args(["-u", "+%Y-%m-%d"])
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "unknown".to_string())
        }
    };

    // ── SHA-256 del LICENSE (calculado en build, no hardcodeado) ─────
    let license_hash = std::fs::read("LICENSE")
        .map(|bytes| {
            let mut hasher = Sha256::new();
            hasher.update(&bytes);
            hex::encode(hasher.finalize())
        })
        .unwrap_or_else(|_| {
            panic!("build.rs: cannot read LICENSE file for hashing");
        });

    // ── Exponer como env vars para env!() en el código Rust ───────────
    println!("cargo:rustc-env=ENOLA_GIT_HASH={}", git_hash);
    println!("cargo:rustc-env=ENOLA_BUILD_DATE={}", build_date);
    println!("cargo:rustc-env=ENOLA_LICENSE_HASH={}", license_hash);

    // ── Re-ejecutar si cambia HEAD o SOURCE_DATE_EPOCH ────────────────
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs/heads");
    println!("cargo:rerun-if-changed=LICENSE");
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");

    // ── Generar assets/console_commands.json desde clap ───────────────
    generate_console_commands_json();
    // ── Generar assets/mcp_tools.json (árbol tipado para el servidor MCP) ──
    generate_mcp_tools_json();
    println!("cargo:rerun-if-changed=src/cli/defs.rs");
}

/// Introspecta Cli::command() (clap) y genera assets/console_commands.json
/// con la estructura de módulos, subcomandos y flags del CLI.
///
/// Este JSON lo carga la consola web (assets/app.js) para autocompletado,
/// eliminando la necesidad de sincronizar manualmente app.js con clap.
fn generate_console_commands_json() {
    let cmd = defs::Cli::command();

    let mut modules: Vec<String> = Vec::new();
    let mut subcommands: serde_json::Map<String, serde_json::Value> = serde_json::Map::new();
    let mut flags: serde_json::Map<String, serde_json::Value> = serde_json::Map::new();

    for sub in cmd.get_subcommands() {
        let name = sub.get_name().to_string();
        modules.push(name.clone());

        // Subcomandos anidados (ej: tor → list, create, start, ...)
        let subs: Vec<String> = sub
            .get_subcommands()
            .map(|s| s.get_name().to_string())
            .collect();
        if !subs.is_empty() {
            subcommands.insert(
                name.clone(),
                serde_json::Value::Array(subs.into_iter().map(serde_json::Value::String).collect()),
            );
        }

        // Flags del comando hoja (ej: setup --all --vpn --security --pqc-tls)
        let cmd_flags: Vec<String> = sub
            .get_arguments()
            .filter_map(|a| a.get_long().map(|l| format!("--{}", l)))
            .collect();
        if !cmd_flags.is_empty() {
            flags.insert(
                name.clone(),
                serde_json::Value::Array(
                    cmd_flags
                        .into_iter()
                        .map(serde_json::Value::String)
                        .collect(),
                ),
            );
        }

        // Flags por subcomando anidado (ej: tor.create --name --service-type --ssl)
        for nested in sub.get_subcommands() {
            let nflags: Vec<String> = nested
                .get_arguments()
                .filter_map(|a| a.get_long().map(|l| format!("--{}", l)))
                .collect();
            if !nflags.is_empty() {
                let key = format!("{}.{}", name, nested.get_name());
                flags.insert(
                    key,
                    serde_json::Value::Array(
                        nflags.into_iter().map(serde_json::Value::String).collect(),
                    ),
                );
            }

            // Nivel 3: si `nested` es un grupo con UN SOLO subcomando hoja
            // (ej: `plan wp` → `create`), promover las flags del hoja a la
            // clave de 2 niveles `plan.wp`. La consola web solo resuelve
            // `parts[0].parts[1]`, así que es la única vía de autocompletarlas.
            // Grupos con varios leaves (tor auth, git user, vpn peer) son
            // ambiguos → se omiten, igual que hasta ahora.
            let leaves: Vec<_> = nested.get_subcommands().collect();
            if leaves.len() == 1 {
                let leaf_flags: Vec<String> = leaves[0]
                    .get_arguments()
                    .filter_map(|a| a.get_long().map(|l| format!("--{}", l)))
                    .collect();
                if !leaf_flags.is_empty() {
                    let key = format!("{}.{}", name, nested.get_name());
                    // Merge defensivo: si el grupo tuviera args propios ya
                    // registrados, se unen con los del hoja sin duplicar.
                    let mut merged: Vec<String> = flags
                        .get(&key)
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str().map(String::from))
                                .collect()
                        })
                        .unwrap_or_default();
                    for f in leaf_flags {
                        if !merged.contains(&f) {
                            merged.push(f);
                        }
                    }
                    flags.insert(
                        key,
                        serde_json::Value::Array(
                            merged.into_iter().map(serde_json::Value::String).collect(),
                        ),
                    );
                }
            }
        }
    }

    let json = serde_json::json!({
        "modules": modules,
        "subcommands": subcommands,
        "flags": flags,
    });

    let json_str =
        serde_json::to_string_pretty(&json).expect("no se pudo serializar console_commands.json");
    write_if_changed("assets/console_commands.json", &json_str);
}

/// Escribe `content` en `path` solo si difiere del contenido actual.
///
/// Los artefactos generados viven commiteados en el árbol de fuentes: una
/// escritura incondicional actualizaría el mtime aunque el contenido no
/// cambiara, ensuciando el working tree y disparando rebuilds innecesarios.
fn write_if_changed(path: &str, content: &str) {
    match std::fs::read_to_string(path) {
        Ok(existing) if existing == content => {}
        _ => std::fs::write(path, content)
            .unwrap_or_else(|e| panic!("no se pudo escribir {path}: {e}")),
    }
}

/// Primera línea útil de un doc comment clap (ignora marcadores DOC-SYNC).
fn clean_description(about: &str) -> String {
    about
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with("DOC-SYNC:"))
        .unwrap_or("")
        .to_string()
}

/// Descripción limpia de un comando clap (convierte StyledStr a texto plano).
fn cmd_description(cmd: &clap::Command) -> String {
    let raw = cmd
        .get_long_about()
        .or_else(|| cmd.get_about())
        .map(|s| s.to_string())
        .unwrap_or_default();
    clean_description(&raw)
}

/// Extrae los flags de un comando clap como array JSON.
///
/// `positional: true` marca argumentos posicionales (sin `--long`): el
/// dispatcher los emite como valores sueltos en su orden de declaración.
fn extract_flags(cmd: &clap::Command) -> serde_json::Value {
    let mut flags: Vec<serde_json::Value> = Vec::new();
    for arg in cmd.get_arguments() {
        // Nombre de la flag: el long si existe, si no el id (posicional).
        let name = match arg.get_long() {
            Some(l) => l.to_string(),
            None => arg.get_id().as_str().to_string(),
        };
        flags.push(serde_json::json!({
            "name": name,
            "required": arg.is_required_set(),
            "positional": arg.get_long().is_none(),
            // A25-02: alias corto (`-n`) — el gate de la consola web lo
            // resuelve al nombre largo antes de aplicar vetos y DENIED_FLAGS.
            "short": arg.get_short().map(|c| c.to_string()),
            // R-11: distingue flags de presencia (clap SetTrue/SetFalse,
            // `--flag` sin valor) de las que toman valor (`--flag x`). Los
            // bool clap reportan possible_values ["true","false"] aunque no
            // acepten valor — la acción es el discriminador real.
            "boolean": matches!(
                arg.get_action(),
                clap::ArgAction::SetTrue | clap::ArgAction::SetFalse
            ),
            "value_name": arg.get_value_names().and_then(|v| v.first()).map(|s| s.to_string()),
            "possible_values": arg.get_possible_values().iter().map(|p| p.get_name().to_string()).collect::<Vec<_>>(),
        }));
    }
    serde_json::Value::Array(flags)
}

/// Nodo del árbol de comandos: nombre, descripción, flags y subcomandos
/// (recursivo, cubre grupos de nivel 3 como `plan wp create` o `tor auth`).
fn command_node(cmd: &clap::Command) -> serde_json::Value {
    let nested: Vec<serde_json::Value> = cmd.get_subcommands().map(command_node).collect();
    serde_json::json!({
        "name": cmd.get_name(),
        "description": cmd_description(cmd),
        "flags": extract_flags(cmd),
        "subcommands": nested,
    })
}

/// Construye el árbol tipado de comandos (familia → subcomandos → flags,
/// con grupos anidados recursivos).
fn build_command_tree() -> serde_json::Value {
    let cmd = defs::Cli::command();
    let families: Vec<serde_json::Value> = cmd.get_subcommands().map(command_node).collect();
    serde_json::json!({ "families": families })
}

/// Genera assets/mcp_tools.json con el árbol tipado de comandos.
fn generate_mcp_tools_json() {
    let tree = build_command_tree();
    let json_str =
        serde_json::to_string_pretty(&tree).expect("no se pudo serializar mcp_tools.json");
    write_if_changed("assets/mcp_tools.json", &json_str);
}

#[cfg(test)]
mod tests {
    use super::{build_command_tree, format_date_utc};

    #[test]
    fn epoch_zero_is_1970_01_01() {
        assert_eq!(format_date_utc(0), "1970-01-01");
    }

    #[test]
    fn known_timestamps_match_iso_calendar() {
        // 2024-01-01 00:00:00 UTC
        assert_eq!(format_date_utc(1_704_067_200), "2024-01-01");
        // 2026-04-29 00:00:00 UTC
        assert_eq!(format_date_utc(1_777_420_800), "2026-04-29");
        // 2000-02-29 (leap year)
        assert_eq!(format_date_utc(951_782_400), "2000-02-29");
    }

    #[test]
    fn handles_pre_epoch_dates() {
        // 1969-12-31 00:00:00 UTC
        assert_eq!(format_date_utc(-86_400), "1969-12-31");
    }

    #[test]
    fn command_tree_contains_wp_create_with_flags() {
        let tree = build_command_tree();
        let families = tree["families"].as_array().unwrap();
        assert!(
            families.len() >= 30,
            "expected >= 30 families, got {}",
            families.len()
        );

        let wp = families
            .iter()
            .find(|f| f["name"] == "wp")
            .expect("wp family missing");
        let create = wp["subcommands"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["name"] == "create")
            .expect("wp create subcommand missing");

        let flags = create["flags"].as_array().unwrap();
        let name = flags
            .iter()
            .find(|f| f["name"] == "name")
            .expect("--name flag missing");
        assert_eq!(name["required"], true);

        let http_port = flags
            .iter()
            .find(|f| f["name"] == "http-port")
            .expect("--http-port flag missing");
        assert_eq!(http_port["required"], false);
        assert_eq!(http_port["value_name"], "PORT");
    }

    #[test]
    fn command_tree_nests_level3_groups() {
        // `plan wp` es un grupo con un único hoja `create`; el árbol debe
        // exponer la ruta completa `plan wp create` con sus flags.
        let tree = build_command_tree();
        let families = tree["families"].as_array().unwrap();
        let plan = families
            .iter()
            .find(|f| f["name"] == "plan")
            .expect("plan family missing");
        let wp = plan["subcommands"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["name"] == "wp")
            .expect("plan wp group missing");
        let create = wp["subcommands"]
            .as_array()
            .expect("plan wp must expose nested subcommands")
            .iter()
            .find(|s| s["name"] == "create")
            .expect("plan wp create leaf missing");
        let flags = create["flags"].as_array().unwrap();
        assert!(flags.iter().any(|f| f["name"] == "name"));
    }
}
