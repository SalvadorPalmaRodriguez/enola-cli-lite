// ═══════════════════════════════════════════════════════════════════════════
// Docs Command Module — Documentación de uso embebida en el binario
// ═══════════════════════════════════════════════════════════════════════════
//
// Los archivos de documentación se embeben en el binario con include_str!()
// para que estén disponibles offline, sin acceso al filesystem, y siempre
// sincronizados con la versión del CLI instalada.

use crate::cli::commands::CliResult;

// Documentos embebidos en el binario en tiempo de compilación
const DOCS_QUICKSTART: &str = include_str!("../../docs/user/guia/quickstart.md");
const DOCS_COMMANDS_REF: &str = include_str!("../../docs/user/general/commands.md");
const DOCS_CONCEPTS: &str = include_str!("../../docs/user/general/concepts.md");
const DOCS_FAQ: &str = include_str!("../../docs/user/general/faq.md");
const DOCS_EXAMPLES: &str = include_str!("../../docs/user/guia/examples.md");

// Guías avanzadas embebidas (NAV-DOC-001, 2026-04-05)
const DOCS_QUANTUM_SEC: &str = include_str!("../../docs/user/general/quantum-security.md");
const DOCS_VERIFY_DL: &str = include_str!("../../docs/user/verify/verify-downloads.md");
const DOCS_SECURITY: &str = include_str!("../../docs/user/general/SECURITY.md");
const DOCS_INSTALL_ISO: &str = include_str!("../../docs/user/guia/install-from-iso.md");
const DOCS_UNINSTALL: &str = include_str!("../../docs/user/uninstall/uninstall.md");

// Referencias detalladas por familia de comandos. `commands.md` es solo el
// índice: el detalle vive en un documento por familia (sin duplicar contenido).
const DOCS_COMMANDS_TOR: &str = include_str!("../../docs/user/tor/commands-tor.md");
const DOCS_COMMANDS_GIT: &str = include_str!("../../docs/user/git/commands-git.md");
const DOCS_COMMANDS_WP: &str = include_str!("../../docs/user/wp/commands-wp.md");
const DOCS_COMMANDS_DRUPAL: &str = include_str!("../../docs/user/drupal/commands-drupal.md");
const DOCS_COMMANDS_GHOST: &str = include_str!("../../docs/user/ghost/commands-ghost.md");
const DOCS_COMMANDS_MAGNOLIA: &str = include_str!("../../docs/user/magnolia/commands-magnolia.md");
const DOCS_COMMANDS_STRAPI: &str = include_str!("../../docs/user/strapi/commands-strapi.md");
const DOCS_COMMANDS_WAGTAIL: &str = include_str!("../../docs/user/wagtail/commands-wagtail.md");
const DOCS_COMMANDS_FILES: &str = include_str!("../../docs/user/files/commands-files.md");
const DOCS_COMMANDS_FIREWALL: &str = include_str!("../../docs/user/firewall/commands-firewall.md");
const DOCS_COMMANDS_APPARMOR: &str = include_str!("../../docs/user/apparmor/commands-apparmor.md");
const DOCS_COMMANDS_VPN: &str = include_str!("../../docs/user/vpn/commands-vpn.md");
const DOCS_COMMANDS_PORTS: &str = include_str!("../../docs/user/ports/commands-ports.md");
const DOCS_COMMANDS_MAINTENANCE: &str =
    include_str!("../../docs/user/maintenance/commands-maintenance.md");
const DOCS_COMMANDS_DIAG: &str = include_str!("../../docs/user/diag/commands-diag.md");
const DOCS_COMMANDS_LOGS: &str = include_str!("../../docs/user/logs/commands-logs.md");
const DOCS_COMMANDS_UPDATE: &str = include_str!("../../docs/user/update/commands-update.md");
const DOCS_COMMANDS_SIMPLE: &str = include_str!("../../docs/user/general/commands-simple.md");
const DOCS_COMMANDS_SETUP: &str = include_str!("../../docs/user/setup/commands-setup.md");
const DOCS_COMMANDS_TEST: &str = include_str!("../../docs/user/test/commands-test.md");
const DOCS_WEB: &str = include_str!("../../docs/user/web/README.md");
const DOCS_COMMANDS_DOCS: &str = include_str!("../../docs/user/docs/commands-docs.md");
const DOCS_TOR_CLIENT_AUTH: &str = include_str!("../../docs/user/tor/tor-client-auth.md");

/// Familias de comandos con documento de referencia propio.
/// Cada entrada admite varios alias para que el usuario acierte sin memorizar.
const COMMAND_FAMILY_DOCS: &[(&[&str], &str)] = &[
    (&["tor", "onion", "hidden"], DOCS_COMMANDS_TOR),
    (&["git", "forgejo"], DOCS_COMMANDS_GIT),
    (&["wp", "wordpress"], DOCS_COMMANDS_WP),
    (&["drupal"], DOCS_COMMANDS_DRUPAL),
    (&["ghost"], DOCS_COMMANDS_GHOST),
    (&["magnolia"], DOCS_COMMANDS_MAGNOLIA),
    (&["strapi"], DOCS_COMMANDS_STRAPI),
    (&["wagtail"], DOCS_COMMANDS_WAGTAIL),
    (&["files", "file"], DOCS_COMMANDS_FILES),
    (&["firewall", "ufw"], DOCS_COMMANDS_FIREWALL),
    (&["apparmor"], DOCS_COMMANDS_APPARMOR),
    (&["vpn", "wireguard"], DOCS_COMMANDS_VPN),
    (&["ports", "port"], DOCS_COMMANDS_PORTS),
    (&["maintenance"], DOCS_COMMANDS_MAINTENANCE),
    (&["diag", "diagnostics"], DOCS_COMMANDS_DIAG),
    (&["logs", "log"], DOCS_COMMANDS_LOGS),
    (&["update", "updates"], DOCS_COMMANDS_UPDATE),
    (&["web", "dashboard", "gui"], DOCS_WEB),
    (&["setup", "dependencies"], DOCS_COMMANDS_SETUP),
    (&["test", "tests", "benchmark"], DOCS_COMMANDS_TEST),
    (
        &[
            "simple",
            "doctor",
            "config",
            "quickref",
            "license",
            "verify",
            "uninstall",
        ],
        DOCS_COMMANDS_SIMPLE,
    ),
    (&["docs", "documentation"], DOCS_COMMANDS_DOCS),
    (
        &["tor-auth", "tor-client-auth", "client-auth"],
        DOCS_TOR_CLIENT_AUTH,
    ),
];

/// Nombres canónicos (primer alias de cada familia) para los mensajes de ayuda.
fn family_names() -> String {
    COMMAND_FAMILY_DOCS
        .iter()
        .filter_map(|(aliases, _)| aliases.first().copied())
        .collect::<Vec<_>>()
        .join(" · ")
}

/// Muestra la guía de inicio rápido
pub fn quickstart() -> CliResult<String> {
    Ok(render_markdown(DOCS_QUICKSTART))
}

/// Muestra la referencia de comandos.
///
/// Sin grupo devuelve el índice (`commands.md`). Con grupo devuelve el documento
/// detallado de esa familia, que es donde vive el detalle real (flags,
/// argumentos, ejemplos) sin duplicarlo en el índice.
pub fn commands_ref(group: Option<&str>) -> CliResult<String> {
    match group {
        None => Ok(render_markdown(DOCS_COMMANDS_REF)),
        Some(g) => {
            let needle = g.trim().to_lowercase();
            let family = COMMAND_FAMILY_DOCS
                .iter()
                .find(|(aliases, _)| aliases.contains(&needle.as_str()));

            if let Some((_, doc)) = family {
                return Ok(render_markdown(doc));
            }

            // Fallback: buscar el término como sección dentro del índice.
            let section = extract_section(DOCS_COMMANDS_REF, &needle);
            if section.is_empty() {
                Ok(format!(
                    "❌ No se encontró la sección '{}'\n\nGrupos disponibles:\n  {}\n\n\
                     Ejemplo: sudo enola-cli docs commands tor",
                    g,
                    family_names()
                ))
            } else {
                Ok(render_markdown(&section))
            }
        }
    }
}

/// Muestra conceptos clave, opcionalmente sobre un tema concreto.
///
/// Temas válidos: `tor`, `ports`, `vpn`, `apparmor`, `docker`, `cms`, `pqc`, `advisories`
pub fn concepts(topic: Option<&str>) -> CliResult<String> {
    match topic {
        None => Ok(render_markdown(DOCS_CONCEPTS)),
        Some(t) => {
            let section = extract_section(DOCS_CONCEPTS, t);
            if section.is_empty() {
                let topics = "tor · ports · vpn · apparmor · docker · cms · pqc · advisories";
                Ok(format!(
                    "❌ No se encontró el tema '{}'\n\nTemas disponibles:\n  {}\n\n\
                     Ejemplo: sudo enola-cli docs concepts tor",
                    t, topics
                ))
            } else {
                Ok(render_markdown(&section))
            }
        }
    }
}

/// Muestra las preguntas frecuentes, opcionalmente filtradas por término.
pub fn faq(filter: Option<&str>) -> CliResult<String> {
    match filter {
        None => Ok(render_markdown(DOCS_FAQ)),
        Some(term) => {
            let filtered = filter_by_term(DOCS_FAQ, term);
            if filtered.is_empty() {
                Ok(format!(
                    "No se encontraron entradas con '{}' en la FAQ.\n\n\
                     Prueba: sudo enola-cli docs faq (sin filtro)\n\
                     o:      sudo enola-cli docs search {}",
                    term, term
                ))
            } else {
                Ok(render_markdown(&filtered))
            }
        }
    }
}

/// Muestra ejemplos de uso, opcionalmente para un caso concreto.
///
/// Casos válidos: `deploy`, `git-server`, `wordpress`, `backup`, `firewall`, `files`, `cms`, `vpn`, `apparmor`, `update`, `web`
pub fn examples(case: Option<&str>) -> CliResult<String> {
    match case {
        None => Ok(render_markdown(DOCS_EXAMPLES)),
        Some(c) => {
            let section = extract_section(DOCS_EXAMPLES, c);
            if section.is_empty() {
                let cases = "deploy · git-server · wordpress · backup · firewall · files · cms · vpn · apparmor · update · web";
                Ok(format!(
                    "❌ No se encontró el caso '{}'\n\nCasos disponibles:\n  {}\n\n\
                     Ejemplo: sudo enola-cli docs examples wordpress",
                    c, cases
                ))
            } else {
                Ok(render_markdown(&section))
            }
        }
    }
}

pub fn quantum_security() -> CliResult<String> {
    Ok(render_markdown(DOCS_QUANTUM_SEC))
}
pub fn verify_downloads() -> CliResult<String> {
    Ok(render_markdown(DOCS_VERIFY_DL))
}
pub fn security() -> CliResult<String> {
    Ok(render_markdown(DOCS_SECURITY))
}
pub fn install_from_iso() -> CliResult<String> {
    Ok(render_markdown(DOCS_INSTALL_ISO))
}

/// Busca un término en toda la documentación embebida.
///
/// Devuelve los fragmentos (párrafos) que contienen el término,
/// indicando en qué sección se encontró.
pub fn search(term: &str) -> CliResult<String> {
    let term_lower = term.to_lowercase();
    let mut results: Vec<String> = Vec::new();

    let docs = [
        ("Inicio Rápido", DOCS_QUICKSTART),
        ("Índice de Comandos", DOCS_COMMANDS_REF),
        ("Conceptos", DOCS_CONCEPTS),
        ("FAQ", DOCS_FAQ),
        ("Ejemplos", DOCS_EXAMPLES),
        ("Seguridad", DOCS_SECURITY),
        ("Instalación desde ISO", DOCS_INSTALL_ISO),
        ("Desinstalación", DOCS_UNINSTALL),
        ("Comandos Tor", DOCS_COMMANDS_TOR),
        ("Comandos Git", DOCS_COMMANDS_GIT),
        ("Comandos WordPress", DOCS_COMMANDS_WP),
        ("Comandos Drupal", DOCS_COMMANDS_DRUPAL),
        ("Comandos Ghost", DOCS_COMMANDS_GHOST),
        ("Comandos Magnolia", DOCS_COMMANDS_MAGNOLIA),
        ("Comandos Strapi", DOCS_COMMANDS_STRAPI),
        ("Comandos Wagtail", DOCS_COMMANDS_WAGTAIL),
        ("Comandos Files", DOCS_COMMANDS_FILES),
        ("Comandos Firewall", DOCS_COMMANDS_FIREWALL),
        ("Comandos AppArmor", DOCS_COMMANDS_APPARMOR),
        ("Comandos VPN", DOCS_COMMANDS_VPN),
        ("Comandos Ports", DOCS_COMMANDS_PORTS),
        ("Comandos Maintenance", DOCS_COMMANDS_MAINTENANCE),
        ("Comandos Diagnostics", DOCS_COMMANDS_DIAG),
        ("Comandos Logs", DOCS_COMMANDS_LOGS),
        ("Comandos Update", DOCS_COMMANDS_UPDATE),
        ("Comandos del sistema", DOCS_COMMANDS_SIMPLE),
        ("Comandos Setup", DOCS_COMMANDS_SETUP),
        ("Comandos Test", DOCS_COMMANDS_TEST),
        ("Web Dashboard", DOCS_WEB),
        ("Comandos Docs", DOCS_COMMANDS_DOCS),
        ("Tor Client Auth", DOCS_TOR_CLIENT_AUTH),
    ];

    for (doc_name, content) in &docs {
        let mut doc_results: Vec<String> = Vec::new();
        for paragraph in content.split("\n\n") {
            if paragraph.to_lowercase().contains(&term_lower) {
                let trimmed = paragraph.trim();
                if !trimmed.is_empty() {
                    doc_results.push(format!("  {}", trimmed.replace('\n', "\n  ")));
                }
            }
        }
        if !doc_results.is_empty() {
            results.push(format!(
                "\n📄 **{}**\n{}",
                doc_name,
                doc_results.join("\n\n")
            ));
        }
    }

    if results.is_empty() {
        Ok(format!(
            "🔍 No se encontraron resultados para '{}'\n\n\
             Sugerencias:\n\
             • Prueba con un término más corto\n\
             • Usa: sudo enola-cli docs commands    (referencia completa)\n\
             • Usa: sudo enola-cli docs faq         (preguntas frecuentes)",
            term
        ))
    } else {
        Ok(format!(
            "🔍 Resultados para '{}' ({} sección(es)):\n{}",
            term,
            results.len(),
            results.join("\n\n─────────────────────────────────────\n")
        ))
    }
}

// ─── Helpers internos ───────────────────────────────────────────────────────

/// Renderiza markdown básico a texto formateado con ANSI colors para terminal.
/// No es un renderer completo — solo formatea los elementos más comunes.
fn render_markdown(md: &str) -> String {
    let mut out = String::with_capacity(md.len() + 512);

    for line in md.lines() {
        if let Some(title) = line.strip_prefix("# ") {
            out.push_str(&format!("\n\x1b[1;36m{}\x1b[0m\n", title));
        } else if let Some(title) = line.strip_prefix("## ") {
            out.push_str(&format!("\n\x1b[1;33m{}\x1b[0m\n", title));
        } else if let Some(title) = line.strip_prefix("### ") {
            out.push_str(&format!("\n\x1b[1m{}\x1b[0m\n", title));
        } else if line.starts_with("```") {
            // Marcadores de bloque de código — ignorar la línea del marcador
            out.push('\n');
        } else if line.starts_with("> ") {
            let note = line.trim_start_matches("> ");
            out.push_str(&format!("\x1b[2m  ℹ️  {}\x1b[0m\n", note));
        } else if line.starts_with("| ") {
            // Tablas — formato básico
            out.push_str(&format!("\x1b[2m{}\x1b[0m\n", line));
        } else if line.starts_with("- ") || line.starts_with("* ") {
            out.push_str(&format!("  • {}\n", &line[2..]));
        } else if line.starts_with("---") || line.starts_with("===") {
            out.push_str("\x1b[2m─────────────────────────────────────────\x1b[0m\n");
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }

    out
}

/// Extrae la sección de un documento markdown que coincide con el término dado.
/// Busca en los encabezados (## y ###) de forma case-insensitive.
fn extract_section(content: &str, term: &str) -> String {
    let term_lower = term.to_lowercase();
    let lines: Vec<&str> = content.lines().collect();
    let mut in_section = false;
    let mut section_lines: Vec<&str> = Vec::new();
    let mut depth = 0u8;

    for line in &lines {
        if line.starts_with("## ") || line.starts_with("### ") {
            let heading = line.trim_start_matches('#').trim().to_lowercase();
            let current_depth: u8 = if line.starts_with("### ") { 3 } else { 2 };

            if heading.contains(&term_lower) {
                in_section = true;
                depth = current_depth;
                section_lines.push(line);
            } else if in_section {
                // Parar si llegamos a una sección del mismo nivel o superior
                if current_depth <= depth {
                    break;
                }
                section_lines.push(line);
            }
        } else if in_section {
            section_lines.push(line);
        }
    }

    section_lines.join("\n")
}

/// Filtra párrafos de un documento que contienen el término dado.
fn filter_by_term(content: &str, term: &str) -> String {
    let term_lower = term.to_lowercase();
    content
        .split("\n\n")
        .filter(|p| p.to_lowercase().contains(&term_lower))
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quickstart_not_empty() {
        let result = quickstart().unwrap();
        assert!(!result.is_empty());
        assert!(result.contains("sudo enola-cli"));
    }

    #[test]
    fn test_commands_ref_filtered() {
        let result = commands_ref(Some("tor")).unwrap();
        // Debe servir el documento detallado, no el mensaje de error.
        assert!(!result.contains("No se encontró"));
        assert!(result.contains("tor create"));
    }

    #[test]
    fn test_commands_ref_index_lists_families() {
        let result = commands_ref(None).unwrap();
        assert!(result.contains("commands-tor.md"));
        assert!(result.contains("commands-vpn.md"));
    }

    #[test]
    fn test_commands_ref_alias_resolves() {
        let by_alias = commands_ref(Some("wordpress")).unwrap();
        let by_name = commands_ref(Some("wp")).unwrap();
        assert_eq!(by_alias, by_name);
        assert!(!by_alias.contains("No se encontró"));
    }

    #[test]
    fn test_every_family_doc_resolves() {
        for (aliases, _) in COMMAND_FAMILY_DOCS {
            for alias in *aliases {
                let out = commands_ref(Some(alias)).unwrap();
                assert!(
                    !out.contains("No se encontró"),
                    "alias '{}' no resuelve a ningún documento",
                    alias
                );
            }
        }
    }

    #[test]
    fn test_commands_ref_unknown_group() {
        let result = commands_ref(Some("unknown_xyz")).unwrap();
        assert!(result.contains("No se encontró"));
    }

    #[test]
    fn test_search_found() {
        let result = search("wordpress").unwrap();
        assert!(result.contains("wordpress") || result.contains("WordPress"));
    }

    #[test]
    fn test_search_not_found() {
        let result = search("xyzabcnotexistsever12345").unwrap();
        assert!(result.contains("No se encontraron resultados"));
    }

    #[test]
    fn test_faq_filtered() {
        let result = faq(Some("sudo")).unwrap();
        assert!(!result.is_empty());
    }

    #[test]
    fn test_render_markdown_headings() {
        let md = "# Título\n## Sección\nTexto normal\n";
        let rendered = render_markdown(md);
        assert!(rendered.contains("Título"));
        assert!(rendered.contains("Sección"));
    }
}
