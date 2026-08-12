// CLI Module for Enola Rust Core
// This module provides command-line interface that mirrors TUI functionality
// Each CLI command maps directly to a Use Case in the application layer

pub mod commands;
pub mod docs;
pub mod executor;

mod defs;
pub use defs::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hostile_port_edit_cli_corpus_rejects_invalid_numbers_without_panic() {
        // SEC-EXT-DEV-070: corpus hostil en edición de puertos (wp edit).
        let valid = Cli::try_parse_from(["enola", "wp", "edit", "demo", "--http-port", "8080"]);
        assert!(valid.is_ok());

        let invalid_high =
            Cli::try_parse_from(["enola", "wp", "edit", "demo", "--http-port", "70000"]);
        assert!(invalid_high.is_err());

        let invalid_negative =
            Cli::try_parse_from(["enola", "wp", "edit", "demo", "--http-port", "-1"]);
        assert!(invalid_negative.is_err());

        let invalid_alpha =
            Cli::try_parse_from(["enola", "wp", "edit", "demo", "--https-port", "abc"]);
        assert!(invalid_alpha.is_err());
    }

    #[test]
    fn neg001_port_above_u16_max_rejects() {
        // TEST-COV-NEG-001: puerto > 65535 → clap rechaza (overflow u16)
        let err = Cli::try_parse_from(["enola", "wp", "edit", "demo", "--http-port", "65536"]);
        assert!(err.is_err(), "Puerto 65536 debe rechazarse");
    }
    #[test]
    fn neg001_port_non_numeric_corpus_all_rejected() {
        // TEST-COV-NEG-001: corpus de puertos no-numéricos → todos dan error
        let bad_ports = ["abc", "8o80", "http", "9090x", "NaN", "Inf", "-1"];
        for bad_port in bad_ports {
            let err = Cli::try_parse_from(["enola", "wp", "edit", "demo", "--http-port", bad_port]);
            assert!(
                err.is_err(),
                "Puerto no-numérico {:?} debe rechazarse sin panic",
                bad_port
            );
            let msg = err.unwrap_err().to_string();
            assert!(!msg.is_empty(), "Mensaje de error no debe estar vacío");
        }
    }
    #[test]
    fn neg001_git_port_non_numeric_rejected() {
        // TEST-COV-NEG-001: --ssh-port no numérico (Git)
        let err = Cli::try_parse_from(["enola", "git", "edit", "myrepo", "--ssh-port", "abc"]);
        assert!(err.is_err(), "--ssh-port abc debe rechazarse");
    }
    #[test]
    fn neg001_path_traversal_no_panic() {
        // TEST-COV-NEG-001: path con .. no causa panic en parser
        // El rechazo ocurre en execute() via validate_path_no_traversal (§13.74)
        let result = Cli::try_parse_from(["enola", "files", "create", "../../etc/passwd"]);
        // Clap acepta el string; el ejecutor lo rechaza. No panic = garantía.
        let _ = result;
    }

    // TEST-COV-XCUT-017: VpnCommands — subcomandos minoritarios
    #[test]
    fn vpn_create_parses() {
        let r = Cli::try_parse_from(["enola", "vpn", "create", "myvpn", "--port", "51820"]);
        assert!(r.is_ok(), "vpn create: {:?}", r.err());
    }

    #[test]
    fn vpn_list_parses() {
        let r = Cli::try_parse_from(["enola", "vpn", "list"]);
        assert!(r.is_ok());
    }

    // TEST-COV-XCUT-017: global flags parsing
    #[test]
    fn global_format_json_parses() {
        let r = Cli::try_parse_from(["enola", "--format", "json", "tor", "list"]);
        assert!(r.is_ok());
        assert_eq!(r.unwrap().format, "json");
    }

    #[test]
    fn global_verbose_flag_parses() {
        let r = Cli::try_parse_from(["enola", "--verbose", "tor", "list"]);
        assert!(r.is_ok());
        assert!(r.unwrap().verbose);
    }

    #[test]
    fn global_tor_socks_override_parses() {
        let r = Cli::try_parse_from([
            "enola",
            "--tor-socks",
            "socks5h://127.0.0.1:9150",
            "tor",
            "list",
        ]);
        assert!(r.is_ok());
        assert_eq!(
            r.unwrap().tor_socks.as_deref(),
            Some("socks5h://127.0.0.1:9150")
        );
    }
}

// TEST-COV-UNIT-004: proptest/fuzzing del parser CLI (clap).
// Garantiza que Cli::try_parse_from nunca entra en pánico ante args
// arbitrarios — solo Ok(cli) o Err(clap::Error). 10 000 iteraciones × 4 seeds.
#[cfg(test)]
mod proptest_fuzz {
    use super::*;
    use proptest::prelude::*;

    // ── Estrategia: subcomando raíz válido aleatorio ──────────────────────
    fn arb_top_subcmd() -> impl Strategy<Value = &'static str> {
        prop_oneof![
            Just("tor"),
            Just("wp"),
            Just("drupal"),
            Just("ghost"),
            Just("magnolia"),
            Just("strapi"),
            Just("wagtail"),
            Just("git"),
            Just("files"),
            Just("ports"),
            Just("firewall"),
            Just("apparmor"),
            Just("vpn"),
            Just("docs"),
            Just("setup"),
            Just("diag"),
            Just("update"),
            Just("uninstall"),
        ]
    }

    // ── Estrategia: subcomando de segundo nivel (mayoritariamente válido) ──
    fn arb_sub2() -> impl Strategy<Value = &'static str> {
        prop_oneof![
            Just("list"),
            Just("start"),
            Just("stop"),
            Just("status"),
            Just("deploy"),
            Just("show"),
            Just("info"),
            Just("help"),
        ]
    }

    // ── Estrategia: string de arg libre (Unicode arbitrario) ─────────────
    fn arb_arg() -> impl Strategy<Value = String> {
        prop_oneof![
            // nombre de instancia kebab-case
            "[a-z][a-z0-9\\-]{0,15}".prop_map(|s| s),
            // string Unicode amplio (incluye NUL, surrogates, etc.)
            any::<String>(),
            // path traversal hostil
            Just("../../../etc/passwd".to_string()),
            Just("\x00\x01\x02".to_string()),
            Just("' OR 1=1 --".to_string()),
            Just("http://[::1:invalid".to_string()),
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            cases: 2500,        // 2500 casos × 4 seeds = 10 000 iteraciones
            max_shrink_iters: 64,
            ..ProptestConfig::default()
        })]

        // Fuzz: [enola, <subcmd>, <sub2>, <free_arg>] — nunca pánico
        #[test]
        fn fuzz_two_level_args_never_panic(
            top in arb_top_subcmd(),
            sub in arb_sub2(),
            arg in arb_arg(),
        ) {
            let _ = Cli::try_parse_from(["enola", top, sub, arg.as_str()]);
        }

        // Fuzz: global flags con valores arbitrarios — nunca pánico
        #[test]
        fn fuzz_global_flags_never_panic(
            flag in prop_oneof![
                Just("--binary-base-url"), Just("--web-url"), Just("--tor-socks"),
            ],
            val in arb_arg(),
            top in arb_top_subcmd(),
        ) {
            let _ = Cli::try_parse_from(["enola", flag, val.as_str(), top, "list"]);
        }

        // Fuzz: args completamente aleatorios (longitud 1-6) — nunca pánico
        #[test]
        fn fuzz_random_argv_never_panic(
            args in prop::collection::vec(arb_arg(), 1..=6),
        ) {
            let mut argv = vec!["enola".to_string()];
            argv.extend(args);
            let argv_refs: Vec<&str> = argv.iter().map(|s| s.as_str()).collect();
            let _ = Cli::try_parse_from(argv_refs);
        }

        // Fuzz: wp/drupal/ghost con --http-port arbitrario — nunca pánico
        #[test]
        fn fuzz_cms_http_port_never_panic(
            cms in prop_oneof![Just("wp"), Just("drupal"), Just("ghost"), Just("magnolia")],
            name in "[a-z]{1,12}",
            port in any::<String>(),
        ) {
            let _ = Cli::try_parse_from([
                "enola", cms, "deploy", name.as_str(), "--http-port", port.as_str(),
            ]);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// CONSOLE-JSON-SYNC: Regresión que compara assets/console_commands.json contra clap
//
// Objetivo: verificar que el JSON generado por build.rs (que la consola web
// carga para autocompletado) coincide con la estructura real del CLI.
// Si este test falla, ejecuta `cargo build` para regenerar el JSON.
// ═══════════════════════════════════════════════════════════════════════════
#[cfg(test)]
mod console_json_sync {
    use super::Cli;
    use clap::CommandFactory;
    use std::collections::BTreeSet;

    #[test]
    fn console_json_matches_real_cli() {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let json_path = format!("{manifest_dir}/assets/console_commands.json");
        let json_str = std::fs::read_to_string(&json_path)
            .unwrap_or_else(|e| panic!("no se pudo leer {json_path}: {e}"));
        let json: serde_json::Value =
            serde_json::from_str(&json_str).expect("console_commands.json no es JSON válido");

        let cmd = Cli::command();

        // ── Verificar modules ──────────────────────────────────────────
        let json_modules: BTreeSet<String> = json["modules"]
            .as_array()
            .expect("modules no es un array")
            .iter()
            .map(|v| v.as_str().expect("module no es string").to_string())
            .collect();

        let real_modules: BTreeSet<String> = cmd
            .get_subcommands()
            .map(|s| s.get_name().to_string())
            .collect();

        assert_eq!(
            json_modules, real_modules,
            "\n\nmodules en JSON no coincide con clap.\n\
             JSON: {json_modules:?}\n\
             clap: {real_modules:?}\n\n\
             Ejecuta `cargo build` para regenerar console_commands.json.\n"
        );

        // ── Verificar subcommands por módulo ───────────────────────────
        let json_subcommands = json["subcommands"]
            .as_object()
            .expect("subcommands no es un objeto");

        for sub in cmd.get_subcommands() {
            let name = sub.get_name();
            let real_subs: BTreeSet<String> = sub
                .get_subcommands()
                .map(|s| s.get_name().to_string())
                .collect();

            if real_subs.is_empty() {
                // Módulos sin subcomandos anidados no deben aparecer en subcommands
                assert!(
                    !json_subcommands.contains_key(name),
                    "módulo '{name}' no tiene subcomandos en clap pero aparece en JSON.subcommands"
                );
                continue;
            }

            let json_subs: BTreeSet<String> = json_subcommands
                .get(name)
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .map(|v| v.as_str().expect("subcommand no es string").to_string())
                        .collect()
                })
                .unwrap_or_default();

            assert_eq!(
                json_subs, real_subs,
                "\n\nsubcommands de '{name}' no coinciden.\n\
                 JSON: {json_subs:?}\n\
                 clap: {real_subs:?}\n\n\
                 Ejecuta `cargo build` para regenerar console_commands.json.\n"
            );
        }

        // ── Verificar que no hay subcommands extra en JSON ─────────────
        for key in json_subcommands.keys() {
            assert!(
                real_modules.contains(key),
                "JSON.subcommands contiene '{key}' que no es un módulo real en clap"
            );
        }
    }
}
