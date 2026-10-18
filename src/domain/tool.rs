// MCP-004 — Tipos puros de tools MCP + clasificación lectura/escritura.

use serde::{Deserialize, Serialize};

/// Clasificación de seguridad de una tool MCP.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum ToolKind {
    /// No muta el sistema: listar, consultar, planificar, verificar.
    ReadOnly,
    /// Puede mutar el sistema: crear, borrar, publicar, configurar.
    Mutating,
    /// Mutación destructiva o irreversible: borrado de servicios/datos,
    /// eliminación de hidden services (la dirección .onion se pierde),
    /// revocación de credenciales de cliente. Exige `allow_write` +
    /// allowlist Y una confirmación de dos pasos (MCP-009).
    Destructive,
}

impl ToolKind {
    /// `true` si la tool puede modificar el sistema (Mutating o Destructive).
    pub(crate) fn writes(self) -> bool {
        matches!(self, ToolKind::Mutating | ToolKind::Destructive)
    }
}

/// Especificación de una tool MCP (nombre, descripción, schema y clase).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct ToolSpec {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) input_schema: serde_json::Value,
    pub(crate) kind: ToolKind,
}

impl ToolSpec {
    /// Construye una `ToolSpec` validando que el schema es un JSON Schema de
    /// tipo `object` (lo que los clientes MCP esperan para `inputSchema`).
    pub(crate) fn new(
        name: &str,
        description: &str,
        input_schema: serde_json::Value,
        kind: ToolKind,
    ) -> Result<Self, String> {
        if name.trim().is_empty() {
            return Err("tool name must not be empty".to_string());
        }
        let is_object_schema = input_schema
            .get("type")
            .and_then(|t| t.as_str())
            .map(|t| t == "object")
            .unwrap_or(false);
        if !is_object_schema {
            return Err("inputSchema must be a JSON Schema object with type=object".to_string());
        }
        Ok(Self {
            name: name.to_string(),
            description: description.to_string(),
            input_schema,
            kind,
        })
    }
}

/// Trunca `text` a `max` bytes respetando límites de caracteres UTF-8.
/// Devuelve `(texto, si_truncó)`.
pub(crate) fn truncate_output(text: &str, max: usize) -> (String, bool) {
    if text.len() <= max {
        return (text.to_string(), false);
    }
    let mut end = max;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    (text[..end].to_string(), true)
}

/// Subcomandos de LECTURA por familia (allowlist explícita).
///
/// Default-deny: cualquier (familia, subcomando) no listado aquí se clasifica
/// como `Mutating`. Un comando nuevo NUNCA se cuela como lectura por descuido.
pub(crate) const READ_ONLY_SUBCOMMANDS: &[(&str, &str)] = &[
    ("tor", "list"),
    ("git", "list"),
    ("git", "status"),
    ("wp", "list"),
    ("wp", "status"),
    ("drupal", "list"),
    ("drupal", "status"),
    ("ghost", "list"),
    ("ghost", "status"),
    ("magnolia", "list"),
    ("magnolia", "status"),
    ("strapi", "list"),
    ("strapi", "status"),
    ("wagtail", "list"),
    ("wagtail", "status"),
    ("files", "list"),
    ("maintenance", "status"),
    ("maintenance", "timer-status"),
    ("maintenance", "ssh-config"),
    ("diag", "summary"),
    ("diag", "nginx"),
    ("diag", "tor"),
    ("diag", "ssh"),
    ("diag", "word-press"),
    ("diag", "wp-sync"),
    ("diag", "nginx-test"),
    ("diag", "resources"),
    ("diag", "onion"),
    ("test", "list"),
    ("test", "results"),
    ("logs", "list"),
    ("logs", "view"),
    ("logs", "install"),
    ("logs", "smoke-test"),
    ("ports", "list"),
    ("firewall", "status"),
    ("apparmor", "status"),
    ("vpn", "list"),
    ("vpn", "status"),
    ("docs", "quickstart"),
    ("docs", "commands"),
    ("docs", "concepts"),
    ("docs", "faq"),
    ("docs", "examples"),
    ("docs", "search"),
    ("docs", "quantum-security"),
    ("docs", "verify-downloads"),
    ("docs", "security"),
    ("docs", "install-from-iso"),
    ("update", "check"),
    ("update", "schema"),
    // `update verify-feed` NO es lectura (F-22): `--source` acepta rutas
    // locales (std::fs::read → oráculo de ficheros) y URLs http(s)
    // arbitrarias (SSRF). Mutating por defecto.
    ("wp", "config"),
    ("plan", "wp"),
    ("plan", "tor"),
    ("plan", "git"),
];

/// Familias HOJA (sin subcomando) que son de LECTURA.
pub(crate) const READ_ONLY_LEAF_FAMILIES: &[&str] = &[
    "doctor",
    "quickref",
    "license",
    "verify",
    "config-show",
    "config-validate",
];

/// Comandos DESTRUCTIVOS por ruta completa (familia → tokens de subcomando).
///
/// Una operación es destructiva si destruye datos o recursos de forma
/// irreversible: borrado de sitios/servicios, eliminación de hidden services
/// (la dirección .onion se pierde si no hay backup de claves), revocación o
/// rotación de credenciales de cliente Tor, limpieza de datos.
///
/// La comparación es por path completo: `("git", ["user","delete"])` solo
/// casa con `git user delete`, no con `git user create`.
pub(crate) const DESTRUCTIVE_COMMANDS: &[(&str, &[&str])] = &[
    ("wp", &["delete"]),
    ("wp", &["hide"]),
    ("drupal", &["delete"]),
    ("drupal", &["hide"]),
    ("ghost", &["delete"]),
    ("ghost", &["hide"]),
    ("magnolia", &["delete"]),
    ("magnolia", &["hide"]),
    ("strapi", &["delete"]),
    ("strapi", &["hide"]),
    ("wagtail", &["delete"]),
    ("wagtail", &["hide"]),
    ("git", &["delete"]),
    ("git", &["hide"]),
    ("git", &["user", "delete"]),
    ("files", &["delete"]),
    ("vpn", &["delete"]),
    ("vpn", &["peer", "remove"]),
    ("tor", &["remove"]),
    ("tor", &["auth", "revoke"]),
    ("tor", &["auth", "rotate"]),
    // Concesión de acceso, exposición de servicios, degradación de defensa
    // y exportación de secretos tienen impacto equivalente a un borrado
    // (auditoría F-02, F-20, F-23). Criterio uniforme: cualquier operación
    // que abra un servicio a la red, otorgue credenciales/acceso a un
    // tercero o debilite una protección va tras el gate Destructive.
    ("tor", &["create"]),
    ("tor", &["edit"]),
    ("tor", &["rotate"]),
    ("tor", &["auth", "add"]),
    ("tor", &["auth", "generate"]),
    ("tor", &["auth", "disable"]),
    ("vpn", &["create"]),
    ("vpn", &["peer", "add"]),
    ("vpn", &["peer", "add-pubkey"]),
    ("firewall", &["setup"]),
    ("firewall", &["allow"]),
    ("firewall", &["deny"]),
    ("apparmor", &["setup"]),
    ("apparmor", &["mode"]),
    ("git", &["create"]),
    ("git", &["registration"]),
    ("git", &["user", "create"]),
    ("maintenance", &["ssh-harden-pqc"]),
    ("maintenance", &["backup"]),
    ("maintenance", &["disable-checks"]),
    // Publicación/exposición de red en un paso.
    ("wp", &["publish"]),
    ("drupal", &["publish"]),
    ("ghost", &["publish"]),
    ("magnolia", &["publish"]),
    ("strapi", &["publish"]),
    ("wagtail", &["publish"]),
    ("git", &["publish"]),
    ("files", &["create"]),
    ("maintenance", &["cleanup"]),
    // `update apply` y `update download --yes` reemplazan el binario
    // /usr/local/bin/enola-cli (con backup .bak pero irreversible para el
    // agente). `download` puede aplicar con --yes → clasificada destructiva
    // para que ninguna vía la ejecute sin confirmación.
    ("update", &["download"]),
    ("update", &["apply"]),
];

/// Familias HOJA destructivas (sin subcomando).
///
/// `uninstall` permanece aquí aunque también está vetada por
/// `FORBIDDEN_FAMILIES`: la clasificación Destructive documenta su clase de
/// impacto para el CLI directo y para la tabla de auditoría.
pub(crate) const DESTRUCTIVE_LEAF_FAMILIES: &[&str] = &["uninstall"];

/// Familias que `enola_run_command` NUNCA despacha, con cualquier política.
///
/// `mcp` arrancaría un servidor MCP anidado como subproceso: recursión sin
/// sentido (stdin cerrado → sale por EOF) que además aceptaría flags de
/// política (`--allow-write`) en su propio argv. Se rechaza siempre.
///
/// `ssh` concede shell remota (`add-key` escribe authorized_keys,
/// `deploy-hidden` expone SSH por .onion) — vetada por completo.
/// `uninstall`, `web` y `setup` son hojas cuyo blast radius excede el
/// sobre del MCP (auditoría F-06/F-08): `web` abre un dashboard root con
/// `/api/console/run` argv-libre, `setup` ejecuta apt-get como root,
/// `uninstall` hace `bash` sobre scripts resueltos por cwd.
pub(crate) const FORBIDDEN_FAMILIES: &[&str] = &["mcp", "ask", "ssh", "uninstall", "web", "setup"];

/// Rutas completas (familia + tokens de subcomando) vetadas para
/// `enola_run_command` con cualquier política — complemento de
/// `FORBIDDEN_FAMILIES` para vetos a nivel de hoja dentro de familias que
/// sí tienen subcomandos legítimos.
///
/// `update apply` instala un binario arbitrario sobre `current_exe()`
/// (F-01); `test run` ejecuta `cargo test` en el cwd heredado del servidor
/// — build.rs/proc-macros como root — (F-07); `test benchmark` es un dead
/// command ("not available in lite").
pub(crate) const FORBIDDEN_PATHS: &[(&str, &[&str])] = &[
    ("update", &["apply"]),
    ("test", &["run"]),
    ("test", &["benchmark"]),
];

/// `true` si la familia está prohibida para `enola_run_command`.
pub(crate) fn is_forbidden_family(family: &str) -> bool {
    FORBIDDEN_FAMILIES.contains(&family)
}

/// `true` si la ruta (familia + tokens) está vetada para
/// `enola_run_command`, con cualquier política ni allowlist.
pub(crate) fn is_forbidden_path(family: &str, tokens: &[&str]) -> bool {
    FORBIDDEN_PATHS
        .iter()
        .any(|(f, p)| *f == family && *p == tokens)
}

/// Clasifica un comando como lectura, mutación o destrucción (default-deny).
///
/// `tokens` es la ruta de subcomandos ya resuelta (`["user","delete"]` para
/// `git user delete`; vacío para familias hoja). Lo no clasificado
/// explícitamente es `Mutating` — nunca `ReadOnly`.
pub(crate) fn classify(family: &str, tokens: &[&str]) -> ToolKind {
    if tokens.is_empty() {
        if DESTRUCTIVE_LEAF_FAMILIES.contains(&family) {
            return ToolKind::Destructive;
        }
        if READ_ONLY_LEAF_FAMILIES.contains(&family) {
            return ToolKind::ReadOnly;
        }
        return ToolKind::Mutating;
    }
    for (fam, path) in DESTRUCTIVE_COMMANDS {
        if *fam == family && *path == tokens {
            return ToolKind::Destructive;
        }
    }
    // La lectura se otorga por allowlist sobre el primer token del path
    // (`("plan","wp")` cubre `plan wp create`). Ningún path L3 bajo un
    // primer token de lectura es mutante por construcción del árbol.
    if READ_ONLY_SUBCOMMANDS.contains(&(family, tokens[0])) {
        return ToolKind::ReadOnly;
    }
    ToolKind::Mutating
}

/// Valida `args` contra un subconjunto de JSON Schema: `type: object`,
/// `required`, `properties` (con `type`, `enum`, `maxLength`, `minLength`,
/// `minimum`, `maximum`, `maxItems`) y `additionalProperties: false`
/// (rechaza parámetros desconocidos). Devuelve error descriptivo.
pub(crate) fn validate_input(
    schema: &serde_json::Value,
    args: &serde_json::Value,
) -> Result<(), String> {
    if schema.get("type").and_then(|t| t.as_str()) != Some("object") {
        return Err("schema must declare type: object".to_string());
    }
    let obj = args
        .as_object()
        .ok_or_else(|| "arguments must be a JSON object".to_string())?;

    if let Some(required) = schema.get("required").and_then(|r| r.as_array()) {
        for name in required {
            let name = name
                .as_str()
                .ok_or_else(|| "required must be strings".to_string())?;
            if !obj.contains_key(name) {
                return Err(format!("missing required argument: {name}"));
            }
        }
    }

    let empty = serde_json::Map::new();
    let props = schema
        .get("properties")
        .and_then(|p| p.as_object())
        .unwrap_or(&empty);

    if schema.get("additionalProperties").and_then(|v| v.as_bool()) == Some(false) {
        for key in obj.keys() {
            if !props.contains_key(key) {
                return Err(format!("unknown argument: {key}"));
            }
        }
    }

    for (name, prop_schema) in props {
        let Some(value) = obj.get(name) else { continue };
        validate_property(name, prop_schema, value)?;
    }
    Ok(())
}

fn validate_property(
    name: &str,
    schema: &serde_json::Value,
    value: &serde_json::Value,
) -> Result<(), String> {
    if let Some(allowed) = schema.get("enum").and_then(|e| e.as_array()) {
        if !allowed.iter().any(|v| v == value) {
            let choices: Vec<String> = allowed
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect();
            return Err(format!(
                "argument '{name}' must be one of: {}",
                choices.join(", ")
            ));
        }
    }

    if let Some(ty) = schema.get("type").and_then(|t| t.as_str()) {
        let ok = match ty {
            "string" => value.is_string(),
            "integer" => value.is_i64() || value.is_u64(),
            "number" => value.is_number(),
            "boolean" => value.is_boolean(),
            "array" => value.is_array(),
            "object" => value.is_object(),
            _ => true,
        };
        if !ok {
            return Err(format!("argument '{name}' must be of type {ty}"));
        }
    }

    if let Some(s) = value.as_str() {
        if let Some(max) = schema.get("maxLength").and_then(|v| v.as_u64()) {
            if s.chars().count() > max as usize {
                return Err(format!("argument '{name}' exceeds maxLength {max}"));
            }
        }
        if let Some(min) = schema.get("minLength").and_then(|v| v.as_u64()) {
            if s.chars().count() < min as usize {
                return Err(format!("argument '{name}' is shorter than minLength {min}"));
            }
        }
    }

    if let Some(n) = value.as_f64() {
        if let Some(min) = schema.get("minimum").and_then(|v| v.as_f64()) {
            if n < min {
                return Err(format!("argument '{name}' is below minimum {min}"));
            }
        }
        if let Some(max) = schema.get("maximum").and_then(|v| v.as_f64()) {
            if n > max {
                return Err(format!("argument '{name}' exceeds maximum {max}"));
            }
        }
    }

    if let Some(items) = value.as_array() {
        if let Some(max) = schema.get("maxItems").and_then(|v| v.as_u64()) {
            if items.len() > max as usize {
                return Err(format!("argument '{name}' exceeds maxItems {max}"));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn load_tree() -> serde_json::Value {
        serde_json::from_str(include_str!("../../assets/mcp_tools.json")).unwrap()
    }

    // ── 4.2 ToolSpec / ToolKind / truncado ──

    #[test]
    fn tool_spec_rejects_invalid_schema() {
        assert!(ToolSpec::new("x", "d", json!({"type": "array"}), ToolKind::ReadOnly).is_err());
        assert!(ToolSpec::new("x", "d", json!(42), ToolKind::ReadOnly).is_err());
    }

    #[test]
    fn tool_spec_accepts_object_schema() {
        let spec = ToolSpec::new("x", "d", json!({"type": "object"}), ToolKind::ReadOnly).unwrap();
        assert_eq!(spec.kind, ToolKind::ReadOnly);
        assert_eq!(spec.name, "x");
    }

    #[test]
    fn truncate_output_respects_limit_and_marks_truncation() {
        assert_eq!(truncate_output("abc", 5), ("abc".to_string(), false));
        assert_eq!(truncate_output("abc", 3), ("abc".to_string(), false));
        assert_eq!(truncate_output("abcdef", 3), ("abc".to_string(), true));
    }

    #[test]
    fn truncate_output_never_panics_on_utf8_boundary() {
        // 'é' ocupa 2 bytes: cortar en medio no debe panickear.
        let (out, truncated) = truncate_output("aé", 1);
        assert_eq!(out, "a");
        assert!(truncated);
    }

    // ── 4.3 Clasificación read/write (default-deny) ──

    #[test]
    fn classify_semantics_sample() {
        assert_eq!(classify("wp", &["list"]), ToolKind::ReadOnly);
        assert_eq!(classify("wp", &["create"]), ToolKind::Mutating);
        // R-2: `firewall allow` es Destructive (debilita la defensa).
        assert_eq!(classify("firewall", &["allow"]), ToolKind::Destructive);
        assert_eq!(classify("config-validate", &[]), ToolKind::ReadOnly);
    }

    #[test]
    fn default_deny_unlisted_is_mutating() {
        assert_eq!(classify("bogus", &["bogus"]), ToolKind::Mutating);
        assert_eq!(classify("setup", &[]), ToolKind::Mutating);
        assert_eq!(classify("web", &[]), ToolKind::Mutating);
    }

    // ── Clasificación destructiva (MCP-009) ──

    #[test]
    fn destructive_commands_are_classified() {
        for (fam, path) in [
            ("wp", &["delete"][..]),
            ("wp", &["hide"][..]),
            ("tor", &["remove"][..]),
            ("files", &["delete"][..]),
            ("vpn", &["delete"][..]),
            ("maintenance", &["cleanup"][..]),
        ] {
            assert_eq!(
                classify(fam, path),
                ToolKind::Destructive,
                "{fam} {path:?} should be Destructive"
            );
        }
    }

    #[test]
    fn destructive_level3_paths_are_classified() {
        assert_eq!(classify("git", &["user", "delete"]), ToolKind::Destructive);
        assert_eq!(classify("vpn", &["peer", "remove"]), ToolKind::Destructive);
        assert_eq!(classify("tor", &["auth", "revoke"]), ToolKind::Destructive);
        assert_eq!(classify("tor", &["auth", "rotate"]), ToolKind::Destructive);
    }

    #[test]
    fn destructive_path_does_not_shadow_siblings() {
        // `git user list` comparte prefijo con `git user delete`/`create`
        // pero es mutante; `create` ya es Destructive (R-2: concede acceso).
        assert_eq!(classify("git", &["user", "list"]), ToolKind::Mutating);
        assert_eq!(classify("tor", &["auth", "list"]), ToolKind::Mutating);
        assert_eq!(classify("wp", &["stop"]), ToolKind::Mutating);
    }

    #[test]
    fn uninstall_leaf_is_destructive() {
        assert_eq!(classify("uninstall", &[]), ToolKind::Destructive);
    }

    #[test]
    fn mcp_family_is_forbidden() {
        assert!(is_forbidden_family("mcp"));
        assert!(!is_forbidden_family("wp"));
    }

    #[test]
    fn ask_family_is_forbidden() {
        // R19: `enola_run_command` nunca puede despachar `ask` como subproceso.
        assert!(is_forbidden_family("ask"));
    }

    #[test]
    fn every_destructive_path_exists_in_tree() {
        let tree = load_tree();
        for (fam, path) in DESTRUCTIVE_COMMANDS {
            let mut node = tree["families"]
                .as_array()
                .and_then(|f| f.iter().find(|f| f["name"].as_str() == Some(*fam)))
                .unwrap_or_else(|| panic!("destructive family '{fam}' not in tree"));
            for tok in *path {
                node = node["subcommands"]
                    .as_array()
                    .and_then(|s| s.iter().find(|s| s["name"].as_str() == Some(*tok)))
                    .unwrap_or_else(|| panic!("'{tok}' missing under '{fam}'"));
            }
        }
        for fam in DESTRUCTIVE_LEAF_FAMILIES {
            assert!(
                tree["families"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|f| f["name"].as_str() == Some(*fam)),
                "destructive leaf '{fam}' not in tree"
            );
        }
        for fam in FORBIDDEN_FAMILIES {
            assert!(
                tree["families"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|f| f["name"].as_str() == Some(*fam)),
                "forbidden family '{fam}' not in tree"
            );
        }
    }

    #[test]
    fn destructive_commands_are_never_readonly() {
        // Coherencia: ninguna entrada destructiva puede ser también lectura.
        for (fam, path) in DESTRUCTIVE_COMMANDS {
            if path.len() == 1 {
                assert!(
                    !READ_ONLY_SUBCOMMANDS.contains(&(*fam, path[0])),
                    "{fam} {} is both destructive and read-only",
                    path[0]
                );
            }
        }
        for fam in DESTRUCTIVE_LEAF_FAMILIES {
            assert!(!READ_ONLY_LEAF_FAMILIES.contains(fam));
        }
    }

    #[test]
    fn read_allowlist_matches_real_tree() {
        let tree = load_tree();
        let families = tree["families"].as_array().unwrap();

        for (fam, sub) in READ_ONLY_SUBCOMMANDS {
            let node = families
                .iter()
                .find(|f| f["name"].as_str() == Some(*fam))
                .unwrap_or_else(|| panic!("allowlist family '{fam}' not in tree"));
            let subs: Vec<&str> = node["subcommands"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| s["name"].as_str().unwrap())
                .collect();
            assert!(
                subs.contains(sub),
                "allowlist subcommand '{fam} {sub}' not in tree"
            );
        }

        for fam in READ_ONLY_LEAF_FAMILIES {
            assert!(
                families.iter().any(|f| f["name"].as_str() == Some(*fam)),
                "allowlist leaf family '{fam}' not in tree"
            );
        }
    }

    #[test]
    fn every_tree_subcommand_is_classified() {
        let tree = load_tree();
        for fam in tree["families"].as_array().unwrap() {
            let subs = fam["subcommands"].as_array().unwrap();
            for sub in subs {
                let kind = classify(
                    fam["name"].as_str().unwrap(),
                    &[sub["name"].as_str().unwrap()],
                );
                assert!(matches!(
                    kind,
                    ToolKind::ReadOnly | ToolKind::Mutating | ToolKind::Destructive
                ));
            }
        }
    }

    // ── validate_input (subconjunto JSON Schema) ──

    #[test]
    fn validate_input_accepts_valid_args() {
        let schema = json!({
            "type": "object",
            "properties": {"name": {"type": "string"}},
            "required": ["name"]
        });
        assert!(validate_input(&schema, &json!({"name": "foo"})).is_ok());
    }

    #[test]
    fn validate_input_rejects_missing_required() {
        let schema = json!({"type": "object", "required": ["name"]});
        let err = validate_input(&schema, &json!({})).unwrap_err();
        assert!(err.contains("missing required argument: name"));
    }

    #[test]
    fn validate_input_rejects_wrong_type() {
        let schema = json!({"type": "object", "properties": {"name": {"type": "string"}}});
        let err = validate_input(&schema, &json!({"name": 42})).unwrap_err();
        assert!(err.contains("must be of type string"));
    }

    #[test]
    fn validate_input_rejects_enum_violation() {
        let schema = json!({"type": "object", "properties": {"mode": {"enum": ["a", "b"]}}});
        let err = validate_input(&schema, &json!({"mode": "z"})).unwrap_err();
        assert!(err.contains("must be one of"));
    }

    #[test]
    fn validate_input_rejects_non_object_args() {
        let schema = json!({"type": "object"});
        assert!(validate_input(&schema, &json!(42)).is_err());
    }

    // ── Validación estricta (MCP-009) ──

    #[test]
    fn validate_input_rejects_unknown_args_when_closed() {
        let schema = json!({
            "type": "object",
            "properties": {"name": {"type": "string"}},
            "additionalProperties": false
        });
        assert!(validate_input(&schema, &json!({"name": "x"})).is_ok());
        let err = validate_input(&schema, &json!({"name": "x", "evil": 1})).unwrap_err();
        assert!(err.contains("unknown argument: evil"), "got: {err}");
    }

    #[test]
    fn validate_input_allows_unknown_args_when_open() {
        // Sin additionalProperties:false el comportamiento previo se mantiene.
        let schema = json!({"type": "object", "properties": {"name": {"type": "string"}}});
        assert!(validate_input(&schema, &json!({"name": "x", "extra": 1})).is_ok());
    }

    #[test]
    fn validate_input_enforces_string_bounds() {
        let schema = json!({
            "type": "object",
            "properties": {"name": {"type": "string", "maxLength": 5, "minLength": 2}}
        });
        assert!(validate_input(&schema, &json!({"name": "abc"})).is_ok());
        assert!(validate_input(&schema, &json!({"name": "abcdef"})).is_err());
        assert!(validate_input(&schema, &json!({"name": "a"})).is_err());
    }

    #[test]
    fn validate_input_enforces_numeric_bounds() {
        let schema = json!({
            "type": "object",
            "properties": {"port": {"type": "integer", "minimum": 1, "maximum": 65535}}
        });
        assert!(validate_input(&schema, &json!({"port": 8080})).is_ok());
        assert!(validate_input(&schema, &json!({"port": 0})).is_err());
        assert!(validate_input(&schema, &json!({"port": 70000})).is_err());
    }

    #[test]
    fn validate_input_enforces_max_items() {
        let schema = json!({
            "type": "object",
            "properties": {"tags": {"type": "array", "maxItems": 2}}
        });
        assert!(validate_input(&schema, &json!({"tags": ["a", "b"]})).is_ok());
        assert!(validate_input(&schema, &json!({"tags": ["a", "b", "c"]})).is_err());
    }
}
