// Docs domain — Documentación de usuario embebida en el binario.
//
// Los archivos `docs/user/**` se embeben con include_str!() para que estén
// disponibles offline y siempre sincronizados con la versión instalada.
// `DOC_CATALOG` deriva de estas mismas constantes (sin duplicar include_str!)
// y es el catálogo cerrado que exponen los MCP `resources/*`.

/// Documento embebido del catálogo MCP.
///
/// `path` es la ruta relativa a `docs/user/` (`wp/commands-wp.md`) y se usa
/// para construir la URI estable `enola://docs/<path>`.
pub struct DocEntry {
    /// Ruta relativa a `docs/user/` (también el identificador del resource).
    pub path: &'static str,
    /// Contenido Markdown embebido.
    pub content: &'static str,
}

impl DocEntry {
    /// URI MCP estable del documento (`enola://docs/<path>`).
    pub fn uri(&self) -> String {
        format!("enola://docs/{}", self.path)
    }
}

// Documentos embebidos en el binario en tiempo de compilación.
pub const DOCS_QUICKSTART: &str = include_str!("../../docs/user/guia/quickstart.md");
pub const DOCS_COMMANDS_REF: &str = include_str!("../../docs/user/general/commands.md");
pub const DOCS_CONCEPTS: &str = include_str!("../../docs/user/general/concepts.md");
pub const DOCS_FAQ: &str = include_str!("../../docs/user/general/faq.md");
pub const DOCS_EXAMPLES: &str = include_str!("../../docs/user/guia/examples.md");

// Guías avanzadas embebidas (NAV-DOC-001, 2026-04-05)
pub const DOCS_QUANTUM_SEC: &str = include_str!("../../docs/user/general/quantum-security.md");
pub const DOCS_VERIFY_DL: &str = include_str!("../../docs/user/verify/verify-downloads.md");
pub const DOCS_SECURITY: &str = include_str!("../../docs/user/general/SECURITY.md");
pub const DOCS_INSTALL_ISO: &str = include_str!("../../docs/user/guia/install-from-iso.md");
pub const DOCS_UNINSTALL: &str = include_str!("../../docs/user/uninstall/uninstall.md");

// Referencias de comandos por familia
pub const DOCS_COMMANDS_TOR: &str = include_str!("../../docs/user/tor/commands-tor.md");
pub const DOCS_COMMANDS_GIT: &str = include_str!("../../docs/user/git/commands-git.md");
pub const DOCS_COMMANDS_WP: &str = include_str!("../../docs/user/wp/commands-wp.md");
pub const DOCS_COMMANDS_DRUPAL: &str = include_str!("../../docs/user/drupal/commands-drupal.md");
pub const DOCS_COMMANDS_GHOST: &str = include_str!("../../docs/user/ghost/commands-ghost.md");
pub const DOCS_COMMANDS_MAGNOLIA: &str =
    include_str!("../../docs/user/magnolia/commands-magnolia.md");
pub const DOCS_COMMANDS_STRAPI: &str = include_str!("../../docs/user/strapi/commands-strapi.md");
pub const DOCS_COMMANDS_WAGTAIL: &str = include_str!("../../docs/user/wagtail/commands-wagtail.md");
pub const DOCS_COMMANDS_FILES: &str = include_str!("../../docs/user/files/commands-files.md");
pub const DOCS_COMMANDS_FIREWALL: &str =
    include_str!("../../docs/user/firewall/commands-firewall.md");
pub const DOCS_COMMANDS_APPARMOR: &str =
    include_str!("../../docs/user/apparmor/commands-apparmor.md");
pub const DOCS_COMMANDS_VPN: &str = include_str!("../../docs/user/vpn/commands-vpn.md");
pub const DOCS_COMMANDS_PORTS: &str = include_str!("../../docs/user/ports/commands-ports.md");
pub const DOCS_COMMANDS_MAINTENANCE: &str =
    include_str!("../../docs/user/maintenance/commands-maintenance.md");
pub const DOCS_COMMANDS_DIAG: &str = include_str!("../../docs/user/diag/commands-diag.md");
pub const DOCS_COMMANDS_LOGS: &str = include_str!("../../docs/user/logs/commands-logs.md");
pub const DOCS_COMMANDS_UPDATE: &str = include_str!("../../docs/user/update/commands-update.md");
pub const DOCS_COMMANDS_SIMPLE: &str = include_str!("../../docs/user/general/commands-simple.md");
pub const DOCS_COMMANDS_PLAN: &str = include_str!("../../docs/user/general/commands-plan.md");
pub const DOCS_COMMANDS_SETUP: &str = include_str!("../../docs/user/setup/commands-setup.md");
pub const DOCS_COMMANDS_TEST: &str = include_str!("../../docs/user/test/commands-test.md");
pub const DOCS_WEB: &str = include_str!("../../docs/user/web/README.md");
pub const DOCS_COMMANDS_DOCS: &str = include_str!("../../docs/user/docs/commands-docs.md");
pub const DOCS_TOR_CLIENT_AUTH: &str = include_str!("../../docs/user/tor/tor-client-auth.md");
pub const DOCS_COMMANDS_MCP: &str = include_str!("../../docs/user/mcp/commands-mcp.md");
pub const DOCS_COMMANDS_ASK: &str = include_str!("../../docs/user/ai/commands-ask.md");

/// Prefijo del esquema de URIs de documentación MCP.
pub const DOCS_URI_PREFIX: &str = "enola://docs/";

/// Catálogo cerrado de recursos documentales (derivado de las constantes
/// embebidas — un `include_str!` nuevo sin entrada aquí rompe el test
/// anti-deriva de este módulo).
pub const DOC_CATALOG: &[DocEntry] = &[
    DocEntry {
        path: "guia/quickstart.md",
        content: DOCS_QUICKSTART,
    },
    DocEntry {
        path: "general/commands.md",
        content: DOCS_COMMANDS_REF,
    },
    DocEntry {
        path: "general/concepts.md",
        content: DOCS_CONCEPTS,
    },
    DocEntry {
        path: "general/faq.md",
        content: DOCS_FAQ,
    },
    DocEntry {
        path: "guia/examples.md",
        content: DOCS_EXAMPLES,
    },
    DocEntry {
        path: "general/quantum-security.md",
        content: DOCS_QUANTUM_SEC,
    },
    DocEntry {
        path: "verify/verify-downloads.md",
        content: DOCS_VERIFY_DL,
    },
    DocEntry {
        path: "general/SECURITY.md",
        content: DOCS_SECURITY,
    },
    DocEntry {
        path: "guia/install-from-iso.md",
        content: DOCS_INSTALL_ISO,
    },
    DocEntry {
        path: "uninstall/uninstall.md",
        content: DOCS_UNINSTALL,
    },
    DocEntry {
        path: "tor/commands-tor.md",
        content: DOCS_COMMANDS_TOR,
    },
    DocEntry {
        path: "git/commands-git.md",
        content: DOCS_COMMANDS_GIT,
    },
    DocEntry {
        path: "wp/commands-wp.md",
        content: DOCS_COMMANDS_WP,
    },
    DocEntry {
        path: "drupal/commands-drupal.md",
        content: DOCS_COMMANDS_DRUPAL,
    },
    DocEntry {
        path: "ghost/commands-ghost.md",
        content: DOCS_COMMANDS_GHOST,
    },
    DocEntry {
        path: "magnolia/commands-magnolia.md",
        content: DOCS_COMMANDS_MAGNOLIA,
    },
    DocEntry {
        path: "strapi/commands-strapi.md",
        content: DOCS_COMMANDS_STRAPI,
    },
    DocEntry {
        path: "wagtail/commands-wagtail.md",
        content: DOCS_COMMANDS_WAGTAIL,
    },
    DocEntry {
        path: "files/commands-files.md",
        content: DOCS_COMMANDS_FILES,
    },
    DocEntry {
        path: "firewall/commands-firewall.md",
        content: DOCS_COMMANDS_FIREWALL,
    },
    DocEntry {
        path: "apparmor/commands-apparmor.md",
        content: DOCS_COMMANDS_APPARMOR,
    },
    DocEntry {
        path: "vpn/commands-vpn.md",
        content: DOCS_COMMANDS_VPN,
    },
    DocEntry {
        path: "ports/commands-ports.md",
        content: DOCS_COMMANDS_PORTS,
    },
    DocEntry {
        path: "maintenance/commands-maintenance.md",
        content: DOCS_COMMANDS_MAINTENANCE,
    },
    DocEntry {
        path: "diag/commands-diag.md",
        content: DOCS_COMMANDS_DIAG,
    },
    DocEntry {
        path: "logs/commands-logs.md",
        content: DOCS_COMMANDS_LOGS,
    },
    DocEntry {
        path: "update/commands-update.md",
        content: DOCS_COMMANDS_UPDATE,
    },
    DocEntry {
        path: "general/commands-simple.md",
        content: DOCS_COMMANDS_SIMPLE,
    },
    DocEntry {
        path: "general/commands-plan.md",
        content: DOCS_COMMANDS_PLAN,
    },
    DocEntry {
        path: "setup/commands-setup.md",
        content: DOCS_COMMANDS_SETUP,
    },
    DocEntry {
        path: "test/commands-test.md",
        content: DOCS_COMMANDS_TEST,
    },
    DocEntry {
        path: "web/README.md",
        content: DOCS_WEB,
    },
    DocEntry {
        path: "docs/commands-docs.md",
        content: DOCS_COMMANDS_DOCS,
    },
    DocEntry {
        path: "tor/tor-client-auth.md",
        content: DOCS_TOR_CLIENT_AUTH,
    },
    DocEntry {
        path: "mcp/commands-mcp.md",
        content: DOCS_COMMANDS_MCP,
    },
    DocEntry {
        path: "ai/commands-ask.md",
        content: DOCS_COMMANDS_ASK,
    },
];

/// Resuelve una URI `enola://docs/<path>` contra el catálogo cerrado.
///
/// Devuelve `None` para cualquier URI fuera del catálogo, incluyendo
/// traversal (`..`), esquemas ajenos y rutas absolutas.
pub fn doc_by_uri(uri: &str) -> Option<&'static DocEntry> {
    let path = uri.strip_prefix(DOCS_URI_PREFIX)?;
    if path.is_empty() || path.contains("..") || path.starts_with('/') {
        return None;
    }
    DOC_CATALOG.iter().find(|d| d.path == path)
}

/// Primera línea de título `# ...` de un documento (descripción corta).
pub fn doc_title(entry: &DocEntry) -> String {
    entry
        .content
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with("# "))
        .map(|l| l.trim_start_matches('#').trim().to_string())
        .unwrap_or_else(|| entry.path.to_string())
}

/// Busca `term` (case-insensitive) en todo el catálogo y devuelve
/// `(doc, excerpt)` por cada párrafo que lo contiene.
pub fn search_docs(term: &str) -> Vec<(&'static DocEntry, String)> {
    let needle = term.to_lowercase();
    let mut out = Vec::new();
    if needle.is_empty() {
        return out;
    }
    for doc in DOC_CATALOG {
        for paragraph in doc.content.split("\n\n") {
            let trimmed = paragraph.trim();
            if !trimmed.is_empty() && trimmed.to_lowercase().contains(&needle) {
                let excerpt: String = trimmed.chars().take(240).collect();
                out.push((doc, excerpt));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// MCP-005 anti-deriva: todo `include_str!` embebido de este mismo fichero
    /// debe tener una entrada en `DOC_CATALOG`. Falla si se embebe un documento
    /// nuevo y no aparece como resource MCP.
    #[test]
    fn every_embedded_doc_is_a_catalog_resource() {
        let src = include_str!("docs.rs");
        let mut embedded: Vec<String> = Vec::new();
        for line in src.lines() {
            for part in line.split("include_str!(").skip(1) {
                if let Some(rest) = part.strip_prefix('"') {
                    if let Some(path) = rest.strip_prefix("../../docs/user/") {
                        let path = path.split('"').next().unwrap_or("");
                        if path.ends_with(".md") {
                            embedded.push(path.to_string());
                        }
                    }
                }
            }
        }
        assert_eq!(embedded.len(), DOC_CATALOG.len(), "catalog out of sync");
        for path in &embedded {
            assert!(
                DOC_CATALOG.iter().any(|d| d.path == path),
                "embedded '{path}' missing from DOC_CATALOG"
            );
        }
    }

    #[test]
    fn uri_roundtrips_through_catalog() {
        for doc in DOC_CATALOG {
            assert!(doc_by_uri(&doc.uri()).is_some(), "{}", doc.path);
        }
    }

    #[test]
    fn traversal_and_foreign_uris_are_rejected() {
        assert!(doc_by_uri("enola://docs/../Cargo.toml").is_none());
        assert!(doc_by_uri("enola://docs//etc/passwd").is_none());
        assert!(doc_by_uri("enola://secrets/x").is_none());
        assert!(doc_by_uri("file:///etc/passwd").is_none());
        assert!(doc_by_uri("enola://docs/").is_none());
        assert!(doc_by_uri("enola://docs/does/not-exist.md").is_none());
    }

    #[test]
    fn search_finds_paragraphs_across_docs() {
        let hits = search_docs("wordpress");
        assert!(!hits.is_empty());
        assert!(hits.iter().all(|(d, _)| d.path.ends_with(".md")));
        assert!(search_docs("").is_empty());
        assert!(search_docs("zzzz-no-match-zzzz").is_empty());
    }
}
