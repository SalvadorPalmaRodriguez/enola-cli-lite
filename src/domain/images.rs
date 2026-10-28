//! A25-07/08 (P3) — Tabla única de imágenes Docker de los servicios publicables
//! (WordPress, Git, CMS) con versión fija, digest, fecha EOL y capabilities.
//!
//! Reglas (SD-9/SD-10, `security/SKILL.md`):
//! - Prohibido `:latest` o tag sin versión: cada entrada lleva tag con versión
//!   o, si upstream solo publica `latest`, va SIN tag y pineada por digest.
//! - Toda imagen de registro va pineada por digest (`image@sha256:…`); los
//!   digests se regeneran con `scripts/dev/refresh_image_digests.sh` y cada
//!   bump se anota en CHANGELOG.
//! - `eol`: fecha (YYYY-MM-DD) a partir de la cual upstream deja de publicar
//!   parches de seguridad para esa versión. `doctor --security` (`image-eol`)
//!   avisa de contenedores enola con imagen EOL.
//! - Contenedores con `cap_drop: ["ALL"]` + `cap_add` mínimo, validado
//!   arrancando la imagen real (2026-10-25) con esas caps: HTTP, SSH y parada
//!   limpia (`docker stop` sin agotar el timeout).
//!
//! Dominio puro: solo constantes y funciones sin I/O.

/// Especificación de una imagen de producción.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageSpec {
    /// Clave estable (`wordpress`, `forgejo`, …) para mensajes y tests.
    pub key: &'static str,
    /// Referencia `repo:tag` (o `repo` pelado si upstream solo publica
    /// `latest`; entonces `digest` es obligatorio).
    pub image: &'static str,
    /// Digest del manifest list/index (`sha256:…`); `None` solo para imágenes
    /// construidas en local desde un Dockerfile embebido.
    pub digest: Option<&'static str>,
    /// Fin de parches de seguridad upstream (YYYY-MM-DD), si se conoce.
    pub eol: Option<&'static str>,
    /// Fuente de la fecha EOL (para la re-auditoría: nunca inventar).
    pub eol_source: &'static str,
    /// Motivo documentado si la imagen ya está fuera de soporte y no hay
    /// alternativa mantenida (residual aceptado, visible en `doctor`).
    pub unmaintained: Option<&'static str>,
    /// Capabilities añadidas sobre `cap_drop: ["ALL"]`.
    pub cap_add: &'static [&'static str],
}

impl ImageSpec {
    /// Referencia efectiva para Docker: `image@digest` si hay digest.
    pub fn reference(&self) -> String {
        match self.digest {
            Some(d) => format!("{}@{}", self.image, d),
            None => self.image.to_string(),
        }
    }

    /// `image_digest` para `ContainerConfig`.
    pub fn digest_string(&self) -> Option<String> {
        self.digest.map(String::from)
    }

    /// `cap_add` para `ContainerConfig`.
    pub fn cap_add_vec(&self) -> Vec<String> {
        self.cap_add.iter().map(|c| c.to_string()).collect()
    }

    /// `cap_drop` para `ContainerConfig`: siempre `["ALL"]` (SD-10).
    pub fn cap_drop_all() -> Vec<String> {
        vec!["ALL".to_string()]
    }

    /// Repositorio sin tag (`ghost:6.72.0-alpine` → `ghost`).
    pub fn repository(&self) -> &'static str {
        repository_of(self.image)
    }

    /// `true` si `eol` es anterior a `today` (YYYY-MM-DD, comparación léxica
    /// válida por el formato ISO).
    pub fn is_eol(&self, today: &str) -> bool {
        self.eol.is_some_and(|e| e < today)
    }
}

/// Repositorio de una referencia Docker (quita `@digest` y `:tag`, respetando
/// el puerto de un registro tipo `host:5000/repo`).
pub fn repository_of(reference: &str) -> &str {
    let no_digest = reference.split('@').next().unwrap_or(reference);
    match no_digest.rfind(':') {
        Some(i) if !no_digest[i..].contains('/') => &no_digest[..i],
        _ => no_digest,
    }
}

/// Caps de las imágenes oficiales que arrancan como root y bajan a un usuario
/// propio (entrypoint con `chown` + `su`/`gosu`).
const DB_CAPS: &[&str] = &["CHOWN", "SETGID", "SETUID", "DAC_OVERRIDE", "FOWNER"];
/// Igual + bind del puerto 80 dentro del contenedor (Apache).
const APACHE_CAPS: &[&str] = &[
    "CHOWN",
    "SETGID",
    "SETUID",
    "DAC_OVERRIDE",
    "FOWNER",
    "NET_BIND_SERVICE",
];

/// MariaDB de WordPress — 11.4 LTS (decisión del operador 2026-10-25).
pub const MARIADB_WORDPRESS: ImageSpec = ImageSpec {
    key: "mariadb-wordpress",
    image: "mariadb:11.4.13",
    digest: Some("sha256:250e1609e93c0e6aecf64bfbab2d84d5e89d2481540fe77447ff6c855c3b1873"),
    eol: Some("2029-05-29"),
    eol_source: "endoflife.date/mariadb (11.4 LTS)",
    unmaintained: None,
    cap_add: DB_CAPS,
};

/// WordPress 6.x + PHP 8.3 (decisión del operador: rama 6.x pineada).
pub const WORDPRESS: ImageSpec = ImageSpec {
    key: "wordpress",
    image: "wordpress:6.9.4-php8.3-apache",
    digest: Some("sha256:5d2c212561c4b5442ebc4d98933a9cbadcf3dee8888ed3fd9ed44667c27cc905"),
    eol: Some("2027-12-31"),
    eol_source:
        "php.net supported versions (PHP 8.3 security support); WordPress core has no fixed EOL",
    unmaintained: None,
    cap_add: APACHE_CAPS,
};

/// Forgejo 15 LTS (decisión del operador 2026-10-25; 11 LTS EOL 2026-07-16).
/// `SYS_CHROOT`: privsep de sshd (git por SSH); `KILL`: s6 (root) señaliza a
/// gitea (usuario git) al parar; `NET_BIND_SERVICE`: sshd en el puerto 22.
pub const FORGEJO: ImageSpec = ImageSpec {
    key: "forgejo",
    image: "codeberg.org/forgejo/forgejo:15.0.10",
    digest: Some("sha256:7b2f61163ff1d19f2aaf62d37ae4e805e822c2f86d6628b2bd468470929beacb"),
    eol: Some("2027-07-15"),
    eol_source: "forgejo.org/docs/latest/admin/release-schedule (15.0 LTS)",
    unmaintained: None,
    cap_add: &[
        "CHOWN",
        "SETGID",
        "SETUID",
        "DAC_OVERRIDE",
        "FOWNER",
        "NET_BIND_SERVICE",
        "SYS_CHROOT",
        "KILL",
    ],
};

/// Drupal 11.4 (Drupal 10 EOL 2026-12-09).
pub const DRUPAL: ImageSpec = ImageSpec {
    key: "drupal",
    image: "drupal:11.4.11-apache",
    digest: Some("sha256:20b150d4c58412be014ef2b973938efddc43b79fd32e1c83670dc3598d44f101"),
    eol: Some("2027-06-30"),
    eol_source: "drupal.org core release schedule: 11.4 security support until June 2027 \
                 [verificar día exacto]",
    unmaintained: None,
    cap_add: APACHE_CAPS,
};

/// MariaDB de Drupal — 10.11 LTS.
pub const MARIADB_DRUPAL: ImageSpec = ImageSpec {
    key: "mariadb-drupal",
    image: "mariadb:10.11.19",
    digest: Some("sha256:0d0945c57f75b3a3b9ebe71aedaaad005fa5305054e8fca3d2e42a8c92b43b5e"),
    eol: Some("2028-02-16"),
    eol_source: "endoflife.date/mariadb (10.11 LTS)",
    unmaintained: None,
    cap_add: DB_CAPS,
};

/// Ghost 6 con SQLite (Ghost 5 + SQLite expone hashes de contraseñas por la
/// API pública: CVE-2026-53949, corregido solo en ≥ 6.21.2).
pub const GHOST: ImageSpec = ImageSpec {
    key: "ghost",
    image: "ghost:6.72.0-alpine",
    digest: Some("sha256:cd617a0901e659cda978b9063f449535d105f5f1a0340ffac415db2c42d22606"),
    eol: None,
    eol_source: "Ghost supports only the current major (6.x); no fixed EOL date published",
    unmaintained: None,
    cap_add: DB_CAPS,
};

/// PostgreSQL 16 (Wagtail, Strapi).
pub const POSTGRES: ImageSpec = ImageSpec {
    key: "postgres",
    image: "postgres:16.15-alpine",
    digest: Some("sha256:f4610795587492cd4fb887c2dc3de301b813278bb6f616992061141b5f15ac36"),
    eol: Some("2028-11-09"),
    eol_source: "endoflife.date/postgresql (16)",
    unmaintained: None,
    cap_add: DB_CAPS,
};

/// Magnolia: upstream solo publica `latest` (imagen de 2020-11-19, OpenJDK
/// 11.0.8, Tomcat 9.0.38). Pineada por digest para que al menos sea
/// reproducible; residual documentado (A25-N3).
pub const MAGNOLIA: ImageSpec = ImageSpec {
    key: "magnolia",
    image: "ghcr.io/magnolia-sre/magnolia-docker/magnolia-docker",
    digest: Some("sha256:cadb30ab24b9e30703f171d66b56d2ef06a8e0fe56fe590e5c4b35c7800649e6"),
    eol: Some("2020-11-19"),
    eol_source: "image created 2020-11-19; ghcr publishes only :latest (no updates since)",
    unmaintained: Some(
        "upstream image unmaintained since 2020 (OpenJDK 11.0.8, Tomcat 9.0.38); \
         no maintained public image — pending operator decision (A25-N3)",
    ),
    cap_add: DB_CAPS,
};

/// Wagtail: imagen construida en local desde `Dockerfile.wagtail` (proyecto
/// vacío `wagtail start`, rama 7.4 LTS, Django 5.2 LTS, Python 3.13 sobre
/// Debian 13 con la base pineada por digest en el propio Dockerfile). El tag
/// cambia con Wagtail/Python para forzar la reconstrucción. A25-N3.
pub const WAGTAIL: ImageSpec = ImageSpec {
    key: "wagtail",
    image: "enola/wagtail:7.4.4-py313",
    digest: None,
    eol: Some("2027-11-02"),
    eol_source: "Wagtail 7.4 LTS (endoflife.date/wagtail, el componente que antes caduca); \
                 Django 5.2 LTS 2028-04-30, Python 3.13 2029-10-31, Debian 13 2030-06-30",
    unmaintained: None,
    cap_add: &[],
};

/// Strapi: imagen construida en local desde `Dockerfile.strapi` (base
/// `node:22.x-bookworm-slim` pineada por digest en el propio Dockerfile).
/// El tag cambia con la base para forzar la reconstrucción.
pub const STRAPI: ImageSpec = ImageSpec {
    key: "strapi",
    image: "enola/strapi:5.49.0-node22",
    digest: None,
    eol: Some("2027-04-30"),
    eol_source: "nodejs.org release schedule (Node 22 LTS)",
    unmaintained: None,
    cap_add: &[],
};

/// A25-N3 (decisión del operador 2026-10-26: mantener con aviso): texto que se
/// muestra al crear/publicar un servicio cuya imagen upstream no se mantiene.
pub fn unmaintained_warning(spec: &ImageSpec) -> Option<String> {
    spec.unmaintained.map(|why| {
        format!(
            "⚠️  Unmaintained image ({}): {why}. The software inside receives no security \
             fixes; avoid publishing it on Tor or accept the risk. `doctor --security` \
             reports it (image-eol).",
            spec.image
        )
    })
}

/// Todas las imágenes de producción de servicios publicables.
pub const ALL: &[ImageSpec] = &[
    MARIADB_WORDPRESS,
    WORDPRESS,
    FORGEJO,
    DRUPAL,
    MARIADB_DRUPAL,
    GHOST,
    POSTGRES,
    MAGNOLIA,
    WAGTAIL,
    STRAPI,
];

/// Busca la especificación por repositorio de una referencia (p. ej. la
/// `Config.Image` de un contenedor existente).
/// (Las rutas de edición usan esto para conservar `cap_add` al recrear.)
pub fn find_by_reference(reference: &str) -> Option<&'static ImageSpec> {
    let repo = repository_of(reference);
    ALL.iter().find(|s| s.repository() == repo)
}

/// Imágenes conocidas como EOL publicadas por versiones anteriores de enola
/// (instancias existentes): repositorio + prefijo de tag → fecha EOL.
/// `doctor --security` las reporta (no hay migración automática de datos).
pub const LEGACY_EOL: &[(&str, &str, &str)] = &[
    ("codeberg.org/forgejo/forgejo", "9", "2025-01-16"),
    ("codeberg.org/forgejo/forgejo", "11", "2026-07-16"),
    ("mariadb", "10.6", "2026-07-06"),
    ("drupal", "10", "2026-12-09"),
    ("ghost", "5", "CVE-2026-53949"),
    ("enola/strapi", "5.49.0", "2026-04-30"),
    ("wagtail/bakerydemo", "latest", "2021-05-10"),
    ("wagtail/bakerydemo", "release-4469a5a", "2021-05-10"),
    (
        "ghcr.io/magnolia-sre/magnolia-docker/magnolia-docker",
        "latest",
        "2020-11-19",
    ),
];

/// Motivo EOL de una imagen en uso, si la tabla la conoce como fuera de
/// soporte a fecha `today`. `None` = soportada o desconocida.
pub fn eol_reason(reference: &str, today: &str) -> Option<String> {
    let repo = repository_of(reference);
    let tag = reference
        .split('@')
        .next()
        .and_then(|r| r.strip_prefix(repo))
        .and_then(|t| t.strip_prefix(':'))
        .unwrap_or("");
    // Imagen actual de la tabla: se evalúa con su propia ficha.
    if let Some(spec) = ALL
        .iter()
        .find(|s| s.repository() == repo && reference.split('@').next() == Some(s.image))
    {
        if let Some(why) = spec.unmaintained {
            return Some(format!("{}: {why}", spec.image));
        }
        return spec.is_eol(today).then(|| {
            format!(
                "{} reached end of life on {}",
                spec.image,
                spec.eol.unwrap_or("?")
            )
        });
    }
    // Versiones antiguas publicadas por enola (prefijo de tag exacto por
    // componentes: "9" casa con "9.0" pero no con "90").
    for (r, prefix, when) in LEGACY_EOL {
        if *r == repo
            && (tag == *prefix
                || tag.starts_with(&format!("{prefix}."))
                || tag.starts_with(&format!("{prefix}-")))
        {
            if when.starts_with("CVE-") {
                return Some(format!(
                    "{repo}:{tag} is vulnerable ({when}) and out of support"
                ));
            }
            if *when < today {
                return Some(format!("{repo}:{tag} reached end of life on {when}"));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn today() -> String {
        chrono::Utc::now().format("%Y-%m-%d").to_string()
    }

    /// SD-9: ni `:latest` ni tag sin versión; sin tag solo si hay digest.
    #[test]
    fn no_image_uses_latest_or_bare_tag() {
        for s in ALL {
            let tag = s
                .image
                .strip_prefix(s.repository())
                .and_then(|t| t.strip_prefix(':'));
            match tag {
                Some(t) => {
                    assert_ne!(t, "latest", "{}: :latest prohibido", s.key);
                    assert!(
                        t.chars().any(|c| c.is_ascii_digit()),
                        "{}: tag sin versión",
                        s.key
                    );
                }
                None => assert!(s.digest.is_some(), "{}: sin tag exige digest", s.key),
            }
        }
    }

    #[test]
    fn registry_images_are_digest_pinned() {
        for s in ALL {
            if s.image.starts_with("enola/") {
                continue; // construida en local desde Dockerfile embebido
            }
            let d = s
                .digest
                .unwrap_or_else(|| panic!("{}: falta digest", s.key));
            assert!(
                d.starts_with("sha256:") && d.len() == 71,
                "{}: digest inválido",
                s.key
            );
        }
    }

    /// Una imagen EOL solo puede estar en la tabla como residual documentado.
    #[test]
    fn eol_images_are_documented_residuals() {
        let today = today();
        for s in ALL {
            if s.is_eol(&today) {
                assert!(
                    s.unmaintained.is_some(),
                    "{} está EOL sin motivo documentado",
                    s.key
                );
            }
            assert!(!s.eol_source.is_empty(), "{}: falta fuente EOL", s.key);
        }
    }

    #[test]
    fn unmaintained_warning_only_for_residual_images() {
        assert!(unmaintained_warning(&WAGTAIL).is_none());
        assert!(unmaintained_warning(&MAGNOLIA).is_some());
        assert!(unmaintained_warning(&GHOST).is_none());
        assert!(unmaintained_warning(&FORGEJO).is_none());
    }

    #[test]
    fn caps_are_known_and_minimal() {
        const ALLOWED: &[&str] = &[
            "CHOWN",
            "SETGID",
            "SETUID",
            "DAC_OVERRIDE",
            "FOWNER",
            "NET_BIND_SERVICE",
            "SYS_CHROOT",
            "KILL",
        ];
        for s in ALL {
            for c in s.cap_add {
                assert!(ALLOWED.contains(c), "{}: cap {c} no aprobada", s.key);
            }
        }
    }

    /// A25-N3: Wagtail se construye en local (rama LTS), sin digest ni aviso.
    #[test]
    fn wagtail_is_built_locally_on_the_lts_branch() {
        assert_eq!(WAGTAIL.image, "enola/wagtail:7.4.4-py313");
        assert!(WAGTAIL.digest.is_none());
        assert!(WAGTAIL.unmaintained.is_none());
        assert!(WAGTAIL.cap_add.is_empty());
        // EOL = el del componente que antes caduca (Wagtail 7.4 LTS).
        assert_eq!(WAGTAIL.eol, Some("2027-11-02"));
        assert!(WAGTAIL.eol_source.contains("Wagtail"));
        assert!(!WAGTAIL.is_eol("2026-10-28"));
        // La demo abandonada sigue marcada como EOL para `doctor --security`.
        assert!(eol_reason("wagtail/bakerydemo:release-4469a5a", "2026-10-28").is_some());
    }

    #[test]
    fn repository_and_reference_parsing() {
        assert_eq!(repository_of("ghost:6.72.0-alpine"), "ghost");
        assert_eq!(repository_of("ghost:6.72.0-alpine@sha256:abc"), "ghost");
        assert_eq!(
            repository_of("codeberg.org/forgejo/forgejo:15.0.10"),
            "codeberg.org/forgejo/forgejo"
        );
        assert_eq!(repository_of("localhost:5000/x"), "localhost:5000/x");
        assert!(GHOST.reference().ends_with(GHOST.digest.unwrap()));
        assert_eq!(STRAPI.reference(), "enola/strapi:5.49.0-node22");
        assert_eq!(find_by_reference("drupal:10-apache"), Some(&DRUPAL));
    }

    #[test]
    fn eol_reason_flags_legacy_and_unmaintained() {
        let t = "2026-10-25";
        assert!(eol_reason("codeberg.org/forgejo/forgejo:9.0", t).is_some());
        assert!(eol_reason("codeberg.org/forgejo/forgejo:11", t).is_some());
        assert!(eol_reason("mariadb:10.6", t).is_some());
        assert!(eol_reason("ghost:5-alpine", t).is_some());
        assert!(eol_reason("enola/strapi:5.49.0", t).is_some());
        assert!(eol_reason("wagtail/bakerydemo:latest", t).is_some());
        assert!(eol_reason(&MAGNOLIA.reference(), t).is_some());
        // Soportadas hoy.
        assert!(eol_reason(&FORGEJO.reference(), t).is_none());
        assert!(eol_reason(&MARIADB_WORDPRESS.reference(), t).is_none());
        assert!(eol_reason(&GHOST.reference(), t).is_none());
        assert!(eol_reason("enola/strapi:5.49.0-node22", t).is_none());
        assert!(eol_reason(&WAGTAIL.reference(), t).is_none());
        // "9" no debe casar con "90".
        assert!(eol_reason("codeberg.org/forgejo/forgejo:90.0", t).is_none());
    }
}
