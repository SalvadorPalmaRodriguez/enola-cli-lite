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
pub const STRAPI_IMAGE_TAG: &str = "enola/strapi:5.49.0";

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
        assert!(
            DOCKERFILE_STRAPI.contains("FROM node:"),
            "debe basarse en node"
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
