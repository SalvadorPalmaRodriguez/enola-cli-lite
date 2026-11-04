// SEC-B11: una ruta que sale de un fichero de configuración (no confiable) solo
// se puede borrar si, resueltos `..` y enlaces simbólicos, queda ESTRICTAMENTE
// dentro de una raíz permitida. Lo usan el borrado de file shares
// (`/srv/enola-files`) y el de sites estáticos (`/var/www`).
use std::path::{Path, PathBuf};

/// Canonicaliza `path` y exige que quede estrictamente dentro de `root`
/// (también canonicalizada); nunca acepta la propia raíz.
pub fn resolve_strictly_within(path: &str, root: &Path) -> Result<PathBuf, String> {
    let root = std::fs::canonicalize(root)
        .map_err(|e| format!("raíz '{}' no resoluble: {e}", root.display()))?;
    let canon =
        std::fs::canonicalize(path).map_err(|e| format!("ruta '{path}' no resoluble: {e}"))?;
    if canon == root || !canon.starts_with(&root) {
        return Err(format!(
            "'{}' no está estrictamente dentro de '{}'",
            canon.display(),
            root.display()
        ));
    }
    Ok(canon)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_child_and_rejects_root_outside_and_dotdot() {
        let t = tempfile::tempdir().expect("tempdir");
        let root = t.path().join("www");
        std::fs::create_dir_all(root.join("site")).expect("mkdir");
        std::fs::create_dir_all(t.path().join("otro")).expect("mkdir");
        let child = root.join("site");
        assert_eq!(
            resolve_strictly_within(child.to_str().unwrap_or_default(), &root).ok(),
            std::fs::canonicalize(&child).ok()
        );
        assert!(resolve_strictly_within(root.to_str().unwrap_or_default(), &root).is_err());
        let escaping = format!("{}/site/../../otro", root.display());
        assert!(resolve_strictly_within(&escaping, &root).is_err());
    }
}
