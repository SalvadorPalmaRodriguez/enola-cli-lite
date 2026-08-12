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
        }
    }

    let json = serde_json::json!({
        "modules": modules,
        "subcommands": subcommands,
        "flags": flags,
    });

    let json_str =
        serde_json::to_string_pretty(&json).expect("no se pudo serializar console_commands.json");
    std::fs::write("assets/console_commands.json", json_str)
        .expect("no se pudo escribir assets/console_commands.json");
}

#[cfg(test)]
mod tests {
    use super::format_date_utc;

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
}
