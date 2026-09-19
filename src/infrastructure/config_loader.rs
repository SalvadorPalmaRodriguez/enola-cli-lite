//! Loader genérico del archivo `~/.enola/config.toml` (CONFIG-001 + CONFIG-002 + QUAL-001).
//!
//! Lee una sección del `config.toml` y devuelve un mapa `clave → valor` con
//! los valores stringificados (strings sin comillas, enteros/bool a su
//! representación textual, arrays en su forma TOML).
//!
//! # QUAL-001 (2026-04-20)
//!
//! Anteriormente este módulo parseaba TOML "a mano" con `split_once('=')`,
//! lo que impedía soportar strings multilínea, arrays, tablas anidadas o
//! escapes. Ahora delega en la crate `toml` (ya en `Cargo.toml`). La firma
//! pública `load_section(&str) -> HashMap<String,String>` se mantiene
//! intacta para que los callers existentes no cambien.
//!
//! Formato esperado:
//! ```toml
//! [web]
//! web_public_url = "https://enola.example.com"
//!
//! [distribution]
//! binary_base_url = "https://releases.example.com"
//! ```
//!
//! Comportamiento tolerante (contrato):
//! - Archivo inexistente / ilegible → `HashMap` vacío.
//! - TOML inválido → `HashMap` vacío (no propaga error).
//! - Sección inexistente → `HashMap` vacío.
use std::collections::HashMap;
use std::path::PathBuf;
/// Devuelve la ruta por defecto al `config.toml` del usuario.
///
/// Bajo `sudo` (euid 0 + `SUDO_USER`), resuelve el home del usuario
/// invocador: el CLI exige root para operar servicios, pero la config es
/// del usuario. Así `sudo enola-cli …` y `sudo -E enola-cli …` leen y
/// escriben el mismo `~/.enola/config.toml` en lugar de `/root/.enola/`.
pub fn config_path() -> Option<PathBuf> {
    effective_home().map(|h| h.join(".enola").join("config.toml"))
}

/// Home efectivo para la configuración: el del invocador bajo sudo;
/// si no, el del proceso.
fn effective_home() -> Option<PathBuf> {
    let euid = unsafe { libc::geteuid() };
    if euid == 0 {
        if let Ok(user) = std::env::var("SUDO_USER") {
            if !user.is_empty() && user != "root" {
                if let Some(home) = home_of(&user) {
                    return Some(home);
                }
            }
        }
    }
    dirs::home_dir()
}

/// Resuelve el home de `user` leyendo `/etc/passwd` (sin deps extra).
fn home_of(user: &str) -> Option<PathBuf> {
    let passwd = std::fs::read_to_string("/etc/passwd").ok()?;
    passwd
        .lines()
        .find(|l| l.split(':').next() == Some(user))
        .and_then(|l| l.split(':').nth(5))
        .map(PathBuf::from)
}
/// Lee una sección concreta del `config.toml` y devuelve sus pares clave-valor.
///
/// Todos los valores escalares se convierten a `String` (ver [`value_to_string`]).
/// Las sub-tablas anidadas (`[section.sub]`) NO se expanden aquí: pide la
/// sub-sección con otra llamada usando el dot-path completo (`"section.sub"`).
pub fn load_section(section: &str) -> HashMap<String, String> {
    load_section_from_path(config_path(), section)
}

fn load_section_from_path(path: Option<PathBuf>, section: &str) -> HashMap<String, String> {
    let Some(path) = path else {
        return HashMap::new();
    };
    let Ok(content) = std::fs::read_to_string(&path) else {
        return HashMap::new();
    };
    parse_section(&content, section)
}
/// Parsea la sección `section` de un texto TOML real.
///
/// `section` puede ser simple (`"misc"`) o dot-path (`"misc.extra"`).
/// Separado para ser testeable sin filesystem.
pub fn parse_section(content: &str, section: &str) -> HashMap<String, String> {
    let Ok(root) = toml::from_str::<toml::Value>(content) else {
        return HashMap::new();
    };
    // Navegar `a.b.c` descendiendo por tablas.
    let mut current: &toml::Value = &root;
    for part in section.split('.') {
        match current.get(part) {
            Some(v) => current = v,
            None => return HashMap::new(),
        }
    }
    let Some(table) = current.as_table() else {
        return HashMap::new();
    };
    let mut out = HashMap::new();
    for (k, v) in table {
        if let Some(s) = value_to_string(v) {
            out.insert(k.clone(), s);
        }
    }
    out
}
/// Inserta o actualiza `key` en la sección `section` del `~/.enola/config.toml`.
///
/// Contrato:
/// - Crea el archivo, el directorio `~/.enola` y las sub-secciones si faltan.
/// - `section` admite dot-path (`"a.b"`), igual que [`load_section`].
/// - Si `value` parsea como entero se guarda como entero TOML; si no, como
///   string TOML.
/// - Si el archivo existente tiene TOML inválido, devuelve error y NO lo toca.
/// - Escritura atómica (tmp + rename) y permisos 0600 (ver config.example.toml).
pub fn set_key(section: &str, key: &str, value: &str) -> Result<(), String> {
    let path = config_path().ok_or_else(|| "no se pudo resolver HOME".to_string())?;
    set_key_in_path(&path, section, key, value)
}

fn set_key_in_path(
    path: &std::path::Path,
    section: &str,
    key: &str,
    value: &str,
) -> Result<(), String> {
    let content = std::fs::read_to_string(path).unwrap_or_default();
    let mut root: toml::Value = if content.trim().is_empty() {
        toml::Value::Table(toml::map::Map::new())
    } else {
        toml::from_str(&content).map_err(|e| format!("config.toml inválido: {}", e))?
    };

    let mut table = root
        .as_table_mut()
        .ok_or_else(|| "config.toml: la raíz no es una tabla".to_string())?;
    for part in section.split('.') {
        table = table
            .entry(part)
            .or_insert_with(|| toml::Value::Table(toml::map::Map::new()))
            .as_table_mut()
            .ok_or_else(|| format!("config.toml: '{}' no es una tabla", part))?;
    }
    let parsed = value
        .trim()
        .parse::<i64>()
        .map(toml::Value::Integer)
        .unwrap_or_else(|_| toml::Value::String(value.to_string()));
    table.insert(key.to_string(), parsed);

    let rendered = toml::to_string_pretty(&root)
        .map_err(|e| format!("no se pudo serializar config.toml: {}", e))?;

    let mut created_dir = None;
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            created_dir = Some(parent.to_path_buf());
        }
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("no se pudo crear {:?}: {}", parent, e))?;
    }
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, rendered).map_err(|e| format!("escribir {:?}: {}", tmp, e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
    }
    std::fs::rename(&tmp, path).map_err(|e| format!("renombrar {:?}: {}", tmp, e))?;

    // Si escribimos como root vía sudo, devolver la propiedad al invocador:
    // si no, ~/.enola/config.toml quedaría root-owned y el usuario no podría
    // ni leer su propia config.
    if unsafe { libc::geteuid() } == 0 {
        chown_sudo_user(path);
        if let Some(dir) = created_dir {
            chown_sudo_user(&dir);
        }
    }
    Ok(())
}

/// `chown uid:gid` numérico según `SUDO_UID`/`SUDO_GID`. No-op fuera de sudo.
fn chown_sudo_user(path: &std::path::Path) {
    let (Ok(uid), Ok(gid)) = (std::env::var("SUDO_UID"), std::env::var("SUDO_GID")) else {
        return;
    };
    if uid.parse::<u32>().is_err() || gid.parse::<u32>().is_err() {
        return;
    }
    let _ = std::process::Command::new("chown")
        .arg(format!("{}:{}", uid, gid))
        .arg(path)
        .status();
}

/// Convierte un `toml::Value` escalar a `String`.
///
/// - `String` → su contenido sin comillas.
/// - `Integer` / `Float` / `Boolean` / `Datetime` → `to_string()`.
/// - `Array` → representación TOML (`[a, b]`), útil para inspección/logging.
/// - `Table` → `None` (las sub-tablas se leen con otra llamada a `load_section`).
fn value_to_string(v: &toml::Value) -> Option<String> {
    match v {
        toml::Value::String(s) => Some(s.clone()),
        toml::Value::Integer(i) => Some(i.to_string()),
        toml::Value::Float(f) => Some(f.to_string()),
        toml::Value::Boolean(b) => Some(b.to_string()),
        toml::Value::Datetime(d) => Some(d.to_string()),
        toml::Value::Array(_) => Some(v.to_string()),
        toml::Value::Table(_) => None,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn empty_content_empty_map() {
        let m = parse_section("", "distribution");
        assert!(m.is_empty());
    }
    #[test]
    fn reads_target_section_only() {
        let toml = r#"
[misc]
key = "https://example.com"
[distribution]
binary_base_url = "https://dl.example.com"
minisign_pubkey_url = "https://example.com/pub"
[web]
web_public_url = "https://example.com"
"#;
        let d = parse_section(toml, "distribution");
        assert_eq!(d.get("binary_base_url").unwrap(), "https://dl.example.com");
        assert_eq!(
            d.get("minisign_pubkey_url").unwrap(),
            "https://example.com/pub"
        );
        assert_eq!(d.len(), 2);
        let w = parse_section(toml, "web");
        assert_eq!(w.get("web_public_url").unwrap(), "https://example.com");
    }
    #[test]
    fn ignores_comments_and_blank_lines() {
        let toml = r#"
# un comentario
[distribution]
# otro comentario
binary_base_url = "https://dl.example.com"  # inline
"#;
        let d = parse_section(toml, "distribution");
        assert_eq!(d.get("binary_base_url").unwrap(), "https://dl.example.com");
    }
    #[test]
    fn unknown_section_returns_empty() {
        let toml = "[misc]\nkey = \"x\"\n";
        assert!(parse_section(toml, "distribution").is_empty());
    }
    #[test]
    fn invalid_toml_returns_empty() {
        // QUAL-001: contrato tolerante — TOML malformado devuelve mapa vacío,
        // nunca propaga error ni hace panic.
        let bad = "[misc\nkey = unquoted";
        assert!(parse_section(bad, "misc").is_empty());
    }

    #[test]
    fn hostile_toml_corpus_never_panics_and_keeps_tolerant_contract() {
        // SEC-EXT-DEV-070: corpus hostil para parser TOML. Debe devolver mapa
        // (vacío o con datos) pero NUNCA panic ni error propagado.
        let corpus = vec![
            "[misc\nkey='broken'",                          // tabla sin cierre
            "[misc]\nkey = \"\\u0000\"",                    // NUL escaped
            "[misc]\nkey = \"http://example.com\n",         // string sin cerrar
            "[misc]\narr = [1,2,3,",                        // array truncado
            "[misc.extra]\nvalue = { nested = { x = 1 } }", // inline nested
            "[a.b.c.d.e]\nvalue='ok'",                      // dot-path profundo
            "not even toml",                                // basura total
        ];

        for sample in corpus {
            let _ = parse_section(sample, "misc");
            let _ = parse_section(sample, "misc.extra");
            let _ = parse_section(sample, "a.b.c.d.e");
        }
    }

    // TEST-COV-UNIT-003: cubrir value_to_string Datetime (L87).
    #[test]
    fn value_to_string_datetime_branch() {
        let toml = "[section]\nsome_date = 2024-01-15\n";
        let m = parse_section(toml, "section");
        assert!(
            m.contains_key("some_date"),
            "fecha debe estar presente: {:?}",
            m
        );
        assert!(m.get("some_date").unwrap().contains("2024"));
    }

    // TEST-COV-UNIT-003: cubrir value_to_string Array (L88).
    #[test]
    fn value_to_string_array_branch() {
        let toml = "[section]\nvals = [1, 2, 3]\n";
        let m = parse_section(toml, "section");
        assert!(m.contains_key("vals"), "array debe estar presente: {:?}", m);
    }

    // TEST-COV-UNIT-003: cubrir L65 (dot-path a escalar, no tabla).
    #[test]
    fn section_dot_path_to_scalar_returns_empty() {
        let toml = "[misc]\nurl = \"https://example.com\"\n";
        let m = parse_section(toml, "misc.url");
        assert!(m.is_empty(), "escalar no es tabla -> mapa vacio");
    }

    #[test]
    fn load_section_from_none_path_returns_empty() {
        let m = load_section_from_path(None, "misc");
        assert!(m.is_empty());
    }

    #[test]
    fn set_key_creates_file_and_section() {
        let tmp = tempfile::tempdir().unwrap(); // unwrap: test-only
        let path = tmp.path().join("sub").join("config.toml");
        set_key_in_path(&path, "backup", "max_backups", "3").unwrap();
        let m = load_section_from_path(Some(path), "backup");
        assert_eq!(m.get("max_backups").unwrap(), "3");
    }

    #[test]
    fn set_key_preserves_other_sections_and_keys() {
        let tmp = tempfile::tempdir().unwrap(); // unwrap: test-only
        let path = tmp.path().join("config.toml");
        std::fs::write(
            &path,
            "[web]\nweb_public_url = \"https://x.dev\"\n[backup]\nother = \"keepme\"\n",
        )
        .unwrap();
        set_key_in_path(&path, "backup", "max_backups", "7").unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("https://x.dev"));
        let m = load_section_from_path(Some(path), "backup");
        assert_eq!(m.get("max_backups").unwrap(), "7");
        assert_eq!(m.get("other").unwrap(), "keepme");
    }

    #[test]
    fn set_key_refuses_invalid_toml_without_touching() {
        let tmp = tempfile::tempdir().unwrap(); // unwrap: test-only
        let path = tmp.path().join("config.toml");
        let bad = "[misc\nkey = unquoted";
        std::fs::write(&path, bad).unwrap();
        let r = set_key_in_path(&path, "backup", "max_backups", "3");
        assert!(r.is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), bad);
    }

    #[test]
    fn home_of_root_resolves_from_etc_passwd() {
        let h = home_of("root").expect("root existe siempre en /etc/passwd");
        assert_eq!(h, PathBuf::from("/root"));
    }

    #[test]
    fn home_of_unknown_user_returns_none() {
        assert!(home_of("usuario_que_no_existe_xyz").is_none());
    }

    #[test]
    fn set_key_stores_integers_unquoted() {
        let tmp = tempfile::tempdir().unwrap(); // unwrap: test-only
        let path = tmp.path().join("config.toml");
        set_key_in_path(&path, "backup", "max_backups", "3").unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("max_backups = 3"), "{}", content);
    }
}
