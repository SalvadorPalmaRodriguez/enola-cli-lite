//! Embedded Dockerfile for the Strapi production image.
use std::path::{Path, PathBuf};

// ── Strapi production image (CMS-STRAPI-PROD-001) ─────────────────────────
/// Directorio canónico donde se extrae el Dockerfile de Strapi.
pub const STRAPI_DOCKER_DIR: &str = "/opt/enola/docker/strapi";
/// Dockerfile de la imagen Strapi 5 de producción embebido en tiempo de compilación.
pub(crate) const DOCKERFILE_STRAPI: &str = include_str!("scripts/Dockerfile.strapi");
/// Nombre canónico del Dockerfile de Strapi extraído al disco.
pub const STRAPI_DOCKERFILE_NAME: &str = "Dockerfile.strapi";
/// Tag de la imagen Strapi construida localmente.
/// A25-07: tag ligado a la base Node del Dockerfile (cambiarla obliga a
/// reconstruir; ver `domain::images::STRAPI`).
pub const STRAPI_IMAGE_TAG: &str = crate::domain::images::STRAPI.image;

// ── Wagtail production image (A25-N3) ─────────────────────────────────────
/// Directorio canónico donde se extrae el contexto de build de Wagtail.
pub const WAGTAIL_DOCKER_DIR: &str = "/opt/enola/docker/wagtail";
/// Dockerfile de la imagen Wagtail (proyecto vacío, rama LTS) embebido en el binario.
pub(crate) const DOCKERFILE_WAGTAIL: &str = include_str!("scripts/Dockerfile.wagtail");
/// Dependencias exactas con hash (`pip-compile --generate-hashes`).
pub(crate) const REQUIREMENTS_WAGTAIL: &str = include_str!("scripts/requirements.wagtail.txt");
/// Entrypoint de la imagen: migraciones, superusuario inicial y gunicorn.
pub(crate) const ENTRYPOINT_WAGTAIL: &str = include_str!("scripts/docker-entrypoint.wagtail.sh");
/// Ajustes de producción (`website/settings/production.py`).
pub(crate) const SETTINGS_WAGTAIL: &str = include_str!("scripts/wagtail_production_settings.py");
/// URLconf con la ruta de `media/` antes del comodín de Wagtail.
pub(crate) const URLS_WAGTAIL: &str = include_str!("scripts/wagtail_enola_urls.py");
/// Nombre canónico del Dockerfile de Wagtail extraído al disco.
pub const WAGTAIL_DOCKERFILE_NAME: &str = "Dockerfile.wagtail";
/// Tag de la imagen Wagtail construida localmente (ligado a la versión: cambiarla
/// obliga a reconstruir; ver `domain::images::WAGTAIL`).
pub const WAGTAIL_IMAGE_TAG: &str = crate::domain::images::WAGTAIL.image;

/// Extrae a `dir` el contexto de build completo de Wagtail y lo devuelve.
/// Siempre sobrescribe: un contexto obsoleto de una versión anterior del
/// binario produciría una imagen distinta de la que el tag promete.
pub fn ensure_wagtail_context_in(dir: &Path) -> Result<PathBuf, String> {
    extract_script(DOCKERFILE_WAGTAIL, dir, WAGTAIL_DOCKERFILE_NAME)?;
    extract_script(REQUIREMENTS_WAGTAIL, dir, "requirements.wagtail.txt")?;
    extract_script(SETTINGS_WAGTAIL, dir, "wagtail_production_settings.py")?;
    extract_script(URLS_WAGTAIL, dir, "wagtail_enola_urls.py")?;
    let ep = extract_script(ENTRYPOINT_WAGTAIL, dir, "docker-entrypoint.wagtail.sh")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&ep, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("No se pudo hacer ejecutable {:?}: {}", ep, e))?;
    }
    Ok(dir.to_path_buf())
}

/// Contexto de build de Wagtail en el directorio canónico (`/opt/enola/docker/wagtail`).
pub fn ensure_wagtail_context() -> Result<PathBuf, String> {
    ensure_wagtail_context_in(Path::new(WAGTAIL_DOCKER_DIR))
}

// ── Magnolia production image (A25-N3) ─────────────────────────────────────
/// Directorio canónico donde se extrae el contexto de build de Magnolia.
pub const MAGNOLIA_DOCKER_DIR: &str = "/opt/enola/docker/magnolia";
/// Dockerfile de la imagen Magnolia CE (Tomcat 10.1 + Java 21, PostgreSQL) embebido.
pub(crate) const DOCKERFILE_MAGNOLIA: &str = include_str!("scripts/Dockerfile.magnolia");
/// Propiedades de Enola que se añaden a `magnolia.properties` del WAR.
pub(crate) const PROPERTIES_MAGNOLIA: &str = include_str!("scripts/magnolia_enola.properties");
/// Entrypoint: genera la config de Jackrabbit desde los secretos y lanza Tomcat.
pub(crate) const ENTRYPOINT_MAGNOLIA: &str = include_str!("scripts/docker-entrypoint.magnolia.sh");
/// Nombre canónico del Dockerfile de Magnolia extraído al disco.
pub const MAGNOLIA_DOCKERFILE_NAME: &str = "Dockerfile.magnolia";
/// Tag de la imagen Magnolia construida localmente (ligado a la versión: cambiarla
/// obliga a reconstruir; ver `domain::images::MAGNOLIA`).
pub const MAGNOLIA_IMAGE_TAG: &str = crate::domain::images::MAGNOLIA.image;

/// Extrae a `dir` el contexto de build completo de Magnolia y lo devuelve.
/// Siempre sobrescribe (ver `ensure_wagtail_context_in`).
pub fn ensure_magnolia_context_in(dir: &Path) -> Result<PathBuf, String> {
    extract_script(DOCKERFILE_MAGNOLIA, dir, MAGNOLIA_DOCKERFILE_NAME)?;
    extract_script(PROPERTIES_MAGNOLIA, dir, "magnolia_enola.properties")?;
    let ep = extract_script(ENTRYPOINT_MAGNOLIA, dir, "docker-entrypoint.magnolia.sh")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&ep, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("No se pudo hacer ejecutable {:?}: {}", ep, e))?;
    }
    Ok(dir.to_path_buf())
}

/// Contexto de build de Magnolia en el directorio canónico (`/opt/enola/docker/magnolia`).
pub fn ensure_magnolia_context() -> Result<PathBuf, String> {
    ensure_magnolia_context_in(Path::new(MAGNOLIA_DOCKER_DIR))
}

// ── Strapi Dockerfile ─────────────────────────────────────────────────────
/// Devuelve la ruta del `Dockerfile.strapi`, extrayéndolo del binario si no existe.
pub fn ensure_strapi_dockerfile() -> Result<PathBuf, String> {
    let cwd = std::env::current_dir().ok();
    ensure_strapi_dockerfile_in(Path::new(STRAPI_DOCKER_DIR), cwd.as_deref())
}

/// Prepara el build context de Strapi (solo el Dockerfile) y devuelve el directorio.
pub fn ensure_strapi_context() -> Result<PathBuf, String> {
    let dir = Path::new(STRAPI_DOCKER_DIR);
    std::fs::create_dir_all(dir).map_err(|e| format!("No se pudo crear {:?}: {}", dir, e))?;
    ensure_strapi_dockerfile_in(dir, None)?;
    Ok(dir.to_path_buf())
}

fn ensure_strapi_dockerfile_in(install_dir: &Path, cwd: Option<&Path>) -> Result<PathBuf, String> {
    let dest = install_dir.join(STRAPI_DOCKERFILE_NAME);
    if dest.exists() {
        return Ok(dest);
    }
    if let Some(cwd) = cwd {
        let p = cwd.join("src/infrastructure/scripts/Dockerfile.strapi");
        if p.exists() {
            return p
                .canonicalize()
                .map_err(|e| format!("No se pudo resolver {:?}: {}", p, e));
        }
    }
    extract_script(DOCKERFILE_STRAPI, install_dir, STRAPI_DOCKERFILE_NAME)
}

fn extract_script(content: &str, dir: &Path, filename: &str) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("No se pudo crear {:?}: {}", dir, e))?;
    let dest = dir.join(filename);
    std::fs::write(&dest, content).map_err(|e| format!("No se pudo escribir {:?}: {}", dest, e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o644)).ok();
    }
    tracing::info!("📦 Extraído del binario → {:?}", dest);
    Ok(dest)
}
// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // ── Wagtail (A25-N3): contexto de build embebido ────────────────────────
    #[test]
    fn wagtail_context_writes_all_build_files_and_overwrites_stale_ones() {
        let tmp = TempDir::new().expect("tempdir");
        std::fs::write(tmp.path().join(WAGTAIL_DOCKERFILE_NAME), "viejo").unwrap();
        let ctx = ensure_wagtail_context_in(tmp.path()).expect("contexto");
        assert_eq!(ctx, tmp.path());
        for f in [
            WAGTAIL_DOCKERFILE_NAME,
            "requirements.wagtail.txt",
            "docker-entrypoint.wagtail.sh",
            "wagtail_production_settings.py",
            "wagtail_enola_urls.py",
        ] {
            assert!(tmp.path().join(f).is_file(), "falta {f}");
        }
        // A diferencia de Strapi, un Dockerfile obsoleto NO se reutiliza:
        // tras actualizar el binario hay que construir con el contenido nuevo.
        let df = std::fs::read_to_string(tmp.path().join(WAGTAIL_DOCKERFILE_NAME)).unwrap();
        assert_ne!(df, "viejo");
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(tmp.path().join("docker-entrypoint.wagtail.sh"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o111, 0o111, "el entrypoint debe ser ejecutable");
    }

    #[test]
    fn wagtail_image_tag_matches_domain_spec() {
        assert_eq!(WAGTAIL_IMAGE_TAG, crate::domain::images::WAGTAIL.image);
    }

    /// Invariantes de seguridad del Dockerfile (SD-9/SD-10, A25-N3).
    #[test]
    fn wagtail_dockerfile_is_pinned_non_root_and_has_no_dev_server() {
        // Solo las instrucciones: los comentarios pueden citar lo que se evita.
        let code: String = DOCKERFILE_WAGTAIL
            .lines()
            .filter(|l| !l.trim_start().starts_with('#'))
            .collect::<Vec<_>>()
            .join("\n");
        let df = code.as_str();
        assert!(
            df.lines()
                .any(|l| l.starts_with("ARG PYTHON_IMAGE=") && l.contains("@sha256:")),
            "la base Python debe ir pineada por digest"
        );
        assert!(
            df.contains("--require-hashes"),
            "pip con hashes obligatorios"
        );
        assert!(df.contains("USER "), "el proceso no debe ser root");
        assert!(
            ENTRYPOINT_WAGTAIL.contains("exec gunicorn"),
            "servidor de producción"
        );
        assert!(!ENTRYPOINT_WAGTAIL.contains("runserver"));
        for banned in ["runserver", ":latest", "pip install wagtail"] {
            assert!(!df.contains(banned), "prohibido en el Dockerfile: {banned}");
        }
        assert!(
            REQUIREMENTS_WAGTAIL.contains("wagtail==7.4.4")
                && REQUIREMENTS_WAGTAIL.contains("--hash=sha256:"),
            "dependencias fijadas con hash"
        );
    }

    /// El entrypoint no debe volcar secretos a variables de entorno ni a argv.
    #[test]
    fn wagtail_entrypoint_and_settings_read_secrets_from_files_only() {
        let ep = ENTRYPOINT_WAGTAIL;
        assert!(ep.contains("/run/secrets/admin_password"));
        assert!(!ep.contains("export SECRET_KEY") && !ep.contains("DJANGO_SUPERUSER_PASSWORD"));
        assert!(
            ep.contains("exec "),
            "debe terminar en exec (PID 1 = gunicorn)"
        );
        let st = SETTINGS_WAGTAIL;
        assert!(st.contains("\"/run/secrets\""));
        assert!(st.contains("_secret(\"django_secret_key\")"));
        assert!(st.contains("_secret(\"db_password\")"));
        assert!(!st.contains("DEBUG = True"));
    }

    // ── Magnolia (A25-N3): contexto de build embebido ───────────────────────
    #[test]
    fn magnolia_context_writes_all_build_files_and_overwrites_stale_ones() {
        let tmp = TempDir::new().expect("tempdir");
        std::fs::write(tmp.path().join(MAGNOLIA_DOCKERFILE_NAME), "viejo").unwrap();
        let ctx = ensure_magnolia_context_in(tmp.path()).expect("contexto");
        assert_eq!(ctx, tmp.path());
        for f in [
            MAGNOLIA_DOCKERFILE_NAME,
            "magnolia_enola.properties",
            "docker-entrypoint.magnolia.sh",
        ] {
            assert!(tmp.path().join(f).is_file(), "falta {f}");
        }
        let df = std::fs::read_to_string(tmp.path().join(MAGNOLIA_DOCKERFILE_NAME)).unwrap();
        assert_ne!(df, "viejo", "un contexto obsoleto no se reutiliza");
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(tmp.path().join("docker-entrypoint.magnolia.sh"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o111, 0o111, "el entrypoint debe ser ejecutable");
    }

    #[test]
    fn magnolia_image_tag_matches_domain_spec_and_dockerfile_version() {
        assert_eq!(MAGNOLIA_IMAGE_TAG, crate::domain::images::MAGNOLIA.image);
        // El tag promete una versión: el Dockerfile debe construir exactamente esa.
        let ver = MAGNOLIA_IMAGE_TAG
            .split(':')
            .nth(1)
            .and_then(|t| t.split('-').next())
            .expect("versión en el tag");
        assert!(
            DOCKERFILE_MAGNOLIA.contains(&format!("ARG MAGNOLIA_VERSION={ver}\n")),
            "el Dockerfile no construye Magnolia {ver}"
        );
    }

    /// Cadena de suministro y mínimo privilegio del Dockerfile (SD-9/SD-10).
    #[test]
    fn magnolia_dockerfile_is_pinned_verified_and_non_root() {
        let code: String = DOCKERFILE_MAGNOLIA
            .lines()
            .filter(|l| !l.trim_start().starts_with('#'))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            code.lines()
                .any(|l| l.starts_with("ARG TOMCAT_IMAGE=") && l.contains("@sha256:")),
            "la base Tomcat debe ir pineada por digest"
        );
        assert!(
            code.contains("tomcat:10.1"),
            "Magnolia 6.4 exige Tomcat 10.1 (Jakarta EE 10)"
        );
        assert!(code.contains("jre21"), "Java 21 LTS");
        assert_eq!(
            code.matches("sha256sum -c").count(),
            2,
            "WAR y driver JDBC verificados por SHA-256"
        );
        assert!(code.contains("USER "), "el proceso no debe ser root");
        assert!(
            code.contains("postgresql.jar"),
            "el WAR no trae el driver PostgreSQL"
        );
        for banned in [":latest", "-h2-", "magnolia-docker"] {
            assert!(
                !code.contains(banned),
                "prohibido en el Dockerfile: {banned}"
            );
        }
    }

    /// Sin contraseña por defecto y sin secretos en claro (Magnolia >= 6.4.4).
    #[test]
    fn magnolia_config_uses_secret_files_and_never_cleartext() {
        let p = PROPERTIES_MAGNOLIA;
        assert!(
            p.contains("magnolia.superuser.bootstrap.password_file=/run/secrets/admin_password")
        );
        assert!(p.contains("magnolia.superuser.bootstrap.interactive_setup=false"));
        assert!(
            p.contains("magnolia.update.auto=true"),
            "sin esto se queda en el instalador web"
        );
        assert!(p.contains("magnolia.home=/data/magnolia"));
        assert!(
            !p.lines().any(|l| l
                .trim_start()
                .starts_with("magnolia.superuser.bootstrap.password=")),
            "la contraseña en claro no puede ir en las propiedades"
        );
        let ep = ENTRYPOINT_MAGNOLIA;
        assert!(ep.contains("/run/secrets/db_password"));
        assert!(ep.contains("exec catalina.sh run"), "PID 1 = Tomcat");
        assert!(!ep.contains("export DB_PASSWORD") && !ep.contains("-Dmagnolia.db"));
        assert!(ep.contains("umask 077"), "el XML con la clave sale 0600");
    }

    // TEST-COV-UNIT-003: cubrir extract_script directamente con tmpdir
    #[test]
    fn extract_script_writes_content_to_tmpdir() {
        let tmp = TempDir::new().expect("tempdir");
        let result = extract_script(
            "#!/usr/bin/env python3\nprint('hello')\n",
            tmp.path(),
            "test_script.py",
        );
        assert!(
            result.is_ok(),
            "extract_script debe tener éxito en tmpdir: {:?}",
            result
        );
        let dest = result.unwrap();
        assert!(dest.exists(), "archivo debe existir: {:?}", dest);
        let content = std::fs::read_to_string(&dest).unwrap();
        assert!(content.contains("hello"), "contenido debe preservarse");
    }

    // TEST-COV-UNIT-003: idempotencia de extract_script
    #[test]
    fn extract_script_succeeds_twice_in_same_dir() {
        let tmp = TempDir::new().expect("tempdir");
        let r1 = extract_script("v1", tmp.path(), "idempotent.py");
        let r2 = extract_script("v2", tmp.path(), "idempotent.py");
        assert!(
            r1.is_ok() && r2.is_ok(),
            "ambas escrituras deben tener éxito"
        );
        let content = std::fs::read_to_string(r2.unwrap()).unwrap();
        assert_eq!(content, "v2", "segunda escritura sobreescribe la primera");
    }

    // ── Strapi Dockerfile tests ────────────────────────────────────────────
    #[test]
    fn strapi_dockerfile_is_embedded_and_valid() {
        assert!(
            !DOCKERFILE_STRAPI.is_empty(),
            "Dockerfile.strapi debe estar embebido"
        );
        // A25-07: base Node LTS soportada y pineada por digest.
        assert!(
            DOCKERFILE_STRAPI.contains("ARG NODE_IMAGE=node:22.")
                && DOCKERFILE_STRAPI.contains("-bookworm-slim@sha256:"),
            "debe basarse en node 22 LTS pineado por digest"
        );
        assert!(DOCKERFILE_STRAPI.contains("FROM ${NODE_IMAGE} AS runtime"));
        assert!(
            STRAPI_IMAGE_TAG.ends_with("-node22"),
            "el tag debe cambiar con la base para forzar la reconstrucción"
        );
        assert!(
            DOCKERFILE_STRAPI.contains("create-strapi-app"),
            "debe hacer scaffold"
        );
        assert!(
            DOCKERFILE_STRAPI.contains("npm run build"),
            "debe compilar el admin"
        );
        assert!(
            DOCKERFILE_STRAPI.contains("npm run start"),
            "debe arrancar en produccion"
        );
        assert!(
            DOCKERFILE_STRAPI.contains("NODE_ENV=production"),
            "debe fijar NODE_ENV=production"
        );
    }

    #[test]
    fn ensure_strapi_dockerfile_does_not_panic() {
        let _ = ensure_strapi_dockerfile();
    }

    #[test]
    fn ensure_strapi_dockerfile_in_falls_back_to_extract() {
        let install = TempDir::new().expect("tempdir");
        let cwd = TempDir::new().expect("tempdir");
        let out = ensure_strapi_dockerfile_in(install.path(), Some(cwd.path()))
            .expect("fallback extraction should succeed");
        assert!(out.exists());
        assert!(out.ends_with(STRAPI_DOCKERFILE_NAME));
    }

    #[test]
    fn ensure_strapi_dockerfile_in_prefers_dev_file_when_present() {
        let install = TempDir::new().expect("tempdir");
        let cwd = TempDir::new().expect("tempdir");
        let dev_dir = cwd.path().join("src/infrastructure/scripts");
        std::fs::create_dir_all(&dev_dir).expect("create dev dir");
        let dev_file = dev_dir.join(STRAPI_DOCKERFILE_NAME);
        std::fs::write(&dev_file, "FROM scratch\n").expect("write dev dockerfile");

        let out = ensure_strapi_dockerfile_in(install.path(), Some(cwd.path()))
            .expect("dev dockerfile should be used");
        assert!(out.exists());
        let content = std::fs::read_to_string(out).expect("read dev dockerfile");
        assert!(content.contains("FROM scratch"));
    }

    #[test]
    fn ensure_strapi_dockerfile_in_returns_existing_install_file() {
        let install = TempDir::new().expect("tempdir");
        let existing = install.path().join(STRAPI_DOCKERFILE_NAME);
        std::fs::write(&existing, "EXISTING\n").expect("write existing dockerfile");

        let out = ensure_strapi_dockerfile_in(install.path(), None)
            .expect("existing file should be returned");
        assert_eq!(out, existing);
    }
}
