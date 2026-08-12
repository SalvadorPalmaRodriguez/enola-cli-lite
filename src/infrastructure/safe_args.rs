//! SEC-EXT-PRIV-021 — Sanitización estricta de argumentos para subprocesos.
//!
//! Helper centralizado para validar inputs del usuario antes de pasarlos como
//! argumentos a `Command::new(...)` o, peor aún, a un shell (`sh -c`).
//!
//! ## Reglas (§13.74)
//!
//! 1. **Preferir argv (vector) sobre `sh -c`**: `Command::args(&[...])` no
//!    interpreta metacaracteres de shell. Inyección imposible.
//! 2. **Si NO se puede evitar `sh -c`** (p.ej. pipes, `>`, `&&`), validar TODOS
//!    los inputs del usuario con los validadores de este módulo ANTES de
//!    interpolar en el `cmd_str`.
//! 3. **Para paths**: usar `validate_path_no_traversal` + `Path::canonicalize` y
//!    comprobar que el resultado está dentro de la raíz esperada.
//!
//! Este módulo NO ejecuta nada — solo valida. La acción la hace el caller.

use crate::domain::error::EnolaError;
use std::path::{Path, PathBuf};

/// Resultado canónico de validación.
pub type SafeArgResult<'a> = Result<&'a str, EnolaError>;

/// Valida un nombre de servicio/instancia/contenedor.
///
/// Regla: solo `[a-zA-Z0-9._-]`, longitud `1..=64`. No empieza por `-` ni `.`.
pub fn validate_service_name(name: &str) -> SafeArgResult<'_> {
    if name.is_empty() || name.len() > 64 {
        return Err(EnolaError::ValidationError(format!(
            "service name length must be 1..=64 chars, got {}",
            name.len()
        )));
    }
    if name.starts_with('-') || name.starts_with('.') {
        return Err(EnolaError::ValidationError(format!(
            "service name must not start with '-' or '.': {:?}",
            name
        )));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
    {
        return Err(EnolaError::ValidationError(format!(
            "service name must match [a-zA-Z0-9._-]+: {:?}",
            name
        )));
    }
    Ok(name)
}

/// Valida un nombre de fichero (sin path).
pub fn validate_filename(name: &str) -> SafeArgResult<'_> {
    if name.is_empty() || name.len() > 255 {
        return Err(EnolaError::ValidationError(format!(
            "filename length must be 1..=255, got {}",
            name.len()
        )));
    }
    if name == ".." || name == "." || name.contains('/') || name.contains('\\') {
        return Err(EnolaError::ValidationError(format!(
            "filename must not contain path separators or be '.'/'..': {:?}",
            name
        )));
    }
    if name.starts_with('-') {
        return Err(EnolaError::ValidationError(format!(
            "filename must not start with '-': {:?}",
            name
        )));
    }
    if name.chars().any(|c| c.is_control() || c == '\0') {
        return Err(EnolaError::ValidationError(format!(
            "filename contains control chars: {:?}",
            name
        )));
    }
    Ok(name)
}

/// Rechaza metacaracteres de shell en un input destinado a `sh -c`.
///
/// Esto es deliberadamente CONSERVADOR. Si tu input legítimo contiene alguno
/// de estos chars, NO uses `sh -c`: usa `Command::args(&[...])` (argv directo).
pub fn validate_no_shell_metacharacters(input: &str) -> SafeArgResult<'_> {
    const SHELL_METACHARS: &[char] = &[
        '\'', '"', '`', '$', '\\', ';', '&', '|', '<', '>', '(', ')', '{', '}', '*', '?', '[', ']',
        '~', '!', '#', ' ', '\t', '\n', '\r', '\0',
    ];
    if let Some(c) = input.chars().find(|c| SHELL_METACHARS.contains(c)) {
        return Err(EnolaError::ValidationError(format!(
            "input contains shell metacharacter {:?} — use Command::args(&[..]) instead of sh -c, or sanitize first",
            c
        )));
    }
    Ok(input)
}

/// Valida que un path no contenga traversal (`..`) ni sea absoluto inesperado.
pub fn validate_path_no_traversal(path: &str) -> SafeArgResult<'_> {
    if path.is_empty() || path.len() > 4096 {
        return Err(EnolaError::ValidationError(format!(
            "path length must be 1..=4096, got {}",
            path.len()
        )));
    }
    if path.contains('\0') {
        return Err(EnolaError::ValidationError(
            "path contains NUL byte".to_string(),
        ));
    }
    for seg in path.split(['/', '\\']) {
        if seg == ".." {
            return Err(EnolaError::ValidationError(format!(
                "path contains traversal segment '..': {:?}",
                path
            )));
        }
    }
    Ok(path)
}

/// Canonicaliza `user_path` y verifica que está dentro de `allowed_root`.
///
/// **SEC-EXT-PRIV-022 §13.74**: cierra ataques de path traversal vía symlinks.
/// `Path::canonicalize` resuelve `..`, `.`, symlinks y normaliza a absoluto.
/// Si tras la resolución el path real cae fuera de `allowed_root`, devuelve
/// `EnolaError::ValidationError`.
///
/// # Contrato
/// 1. `allowed_root` debe existir y ser canonicalizable. Si no, error de
///    infraestructura (no del usuario).
/// 2. `user_path` puede no existir aún (caso "output file"); en ese caso se
///    canonicaliza el padre y se concatena el nombre, comprobando que el
///    resultado siga dentro de `allowed_root`.
/// 3. NUNCA usar este helper sustituyendo a `validate_path_no_traversal`:
///    se complementan. Primero rechaza inputs sintácticamente peligrosos
///    (NUL, `..` literales, > 4096 chars), después comprueba realidad FS.
///
/// # Returns
/// `PathBuf` canonicalizado dentro de la raíz, listo para usar en I/O.
pub fn validate_canonical_within_root(
    allowed_root: &Path,
    user_path: &Path,
) -> Result<PathBuf, EnolaError> {
    // Pre-check sintáctico barato: rechaza NUL y traversal explícito.
    let s = user_path.to_string_lossy();
    validate_path_no_traversal(&s)?;
    let canon_root = allowed_root.canonicalize().map_err(|e| {
        EnolaError::InfrastructureError(format!(
            "allowed_root {:?} cannot be canonicalized: {}",
            allowed_root, e
        ))
    })?;
    // Si el path no existe, canonicalizar el padre y reanclar.
    // SEC-EXT-PRIV-022: las tres ramas de error que siguen son codigo
    // defensivo con escenarios practicamente inalcanzables en tests unitarios:
    //  L159-163: exists()=true pero canonicalize() falla (race condition o fs exotic).
    //  L166-170: parent() = None para path no existente (solo ocurre en root "/").
    //  L178-182: file_name() = None (solo rutas que terminan en ".." prevalidadas).
    // Requieren entorno de test muy especifico; el resto del modulo tiene 100% cobertura.
    let canon_user = resolve_user_path_with(user_path, |p| p.exists(), |p| p.canonicalize())?;
    if !canon_user.starts_with(&canon_root) {
        return Err(EnolaError::ValidationError(format!(
            "path {:?} escapes allowed root {:?} (resolved to {:?})",
            user_path, canon_root, canon_user
        )));
    }
    Ok(canon_user)
}

fn resolve_user_path_with<E, C>(
    user_path: &Path,
    exists_fn: E,
    canonicalize_fn: C,
) -> Result<PathBuf, EnolaError>
where
    E: Fn(&Path) -> bool,
    C: Fn(&Path) -> std::io::Result<PathBuf>,
{
    if exists_fn(user_path) {
        return canonicalize_fn(user_path).map_err(|e| {
            EnolaError::ValidationError(format!(
                "user_path {:?} cannot be canonicalized: {}",
                user_path, e
            ))
        });
    }

    let parent = user_path.parent().ok_or_else(|| {
        EnolaError::ValidationError(format!(
            "user_path {:?} has no parent for resolution",
            user_path
        ))
    })?;
    let canon_parent = canonicalize_fn(parent).map_err(|e| {
        EnolaError::ValidationError(format!(
            "parent of user_path {:?} cannot be canonicalized: {}",
            user_path, e
        ))
    })?;
    let leaf = user_path.file_name().ok_or_else(|| {
        EnolaError::ValidationError(format!(
            "user_path {:?} has no file_name component",
            user_path
        ))
    })?;
    Ok(canon_parent.join(leaf))
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_service_name_accepts_typical_names() {
        for n in ["mi-ia", "blog_1", "wp.local", "a", "MyService"] {
            assert!(validate_service_name(n).is_ok(), "should accept {:?}", n);
        }
    }

    #[test]
    fn validate_service_name_rejects_metacharacters_and_edges() {
        let long = "x".repeat(65);
        let cases: &[&str] = &[
            "",
            "x'; rm -rf /",
            "a b",
            "a;b",
            "a|b",
            "a$b",
            "-flag",
            ".hidden",
            "a/b",
            "a\\b",
            "a\nb",
            &long,
        ];
        for n in cases {
            assert!(validate_service_name(n).is_err(), "should reject {:?}", n);
        }
    }

    #[test]
    fn validate_filename_rejects_traversal_and_separators() {
        for n in ["..", ".", "a/b", "a\\b", "", "-flag", "x\0y"] {
            assert!(validate_filename(n).is_err(), "should reject {:?}", n);
        }
        assert!(validate_filename("backup.tar.gz").is_ok());
        assert!(validate_filename("wp-blog-2026.q4").is_ok());
    }

    #[test]
    fn validate_no_shell_metacharacters_blocks_known_payloads() {
        for s in [
            "x'; rm -rf /; echo 'y",
            "$(whoami)",
            "`id`",
            "a && b",
            "a | b",
            "a > /etc/passwd",
            "a; b",
            "with space",
            "with\nnewline",
        ] {
            assert!(
                validate_no_shell_metacharacters(s).is_err(),
                "should reject {:?}",
                s
            );
        }
    }

    #[test]
    fn validate_no_shell_metacharacters_accepts_safe_inputs() {
        for s in ["wp-blog-1", "backup.tar.gz", "wp_blog1", "a-b_c.d-1"] {
            assert!(
                validate_no_shell_metacharacters(s).is_ok(),
                "should accept {:?}",
                s
            );
        }
    }

    #[test]
    fn validate_path_no_traversal_rejects_dotdot_and_nul() {
        for p in ["../etc/passwd", "/var/lib/../../etc", "a/../b", "x\0y", ""] {
            assert!(
                validate_path_no_traversal(p).is_err(),
                "should reject {:?}",
                p
            );
        }
    }

    #[test]
    fn validate_path_no_traversal_accepts_normal_paths() {
        for p in [
            "/var/lib/enola/data",
            "backups/wp-blog.tar.gz",
            "/srv/enola-wordpress/blog1_wp/wp-content",
        ] {
            assert!(
                validate_path_no_traversal(p).is_ok(),
                "should accept {:?}",
                p
            );
        }
    }

    use std::os::unix::fs::symlink;
    fn mk_tmp_dir(label: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "enola_safe_args_test_{}_{}",
            label,
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }
    #[test]
    fn canonical_within_root_accepts_normal_subpath() {
        let root = mk_tmp_dir("ok");
        let f = root.join("data.txt");
        std::fs::write(&f, b"hi").unwrap();
        let resolved = validate_canonical_within_root(&root, &f).expect("ok");
        assert!(resolved.starts_with(root.canonicalize().unwrap()));
        std::fs::remove_dir_all(&root).ok();
    }
    #[test]
    fn canonical_within_root_rejects_dotdot_traversal() {
        let root = mk_tmp_dir("trav");
        let bad = root.join("../etc/passwd");
        let err = validate_canonical_within_root(&root, &bad).unwrap_err();
        assert!(
            matches!(err, EnolaError::ValidationError(_)),
            "expected ValidationError, got {:?}",
            err
        );
        std::fs::remove_dir_all(&root).ok();
    }
    #[test]
    fn canonical_within_root_rejects_symlink_escaping_root() {
        let root = mk_tmp_dir("sym");
        let outside = mk_tmp_dir("sym_out");
        let target = outside.join("secret.txt");
        std::fs::write(&target, b"S").unwrap();
        // Symlink dentro de root apuntando fuera.
        let link = root.join("escape");
        symlink(&target, &link).unwrap();
        let err = validate_canonical_within_root(&root, &link).unwrap_err();
        let EnolaError::ValidationError(ref msg) = err else {
            unreachable!("expected ValidationError")
        };
        assert!(
            msg.contains("escapes") || msg.contains("escape"),
            "msg: {}",
            msg
        );
        std::fs::remove_dir_all(&root).ok();
        std::fs::remove_dir_all(&outside).ok();
    }
    #[test]
    fn canonical_within_root_rejects_absolute_outside_root() {
        let root = mk_tmp_dir("abs");
        let bad = std::path::PathBuf::from("/etc/passwd");
        let err = validate_canonical_within_root(&root, &bad).unwrap_err();
        assert!(
            matches!(err, EnolaError::ValidationError(_)),
            "expected ValidationError, got {:?}",
            err
        );
        std::fs::remove_dir_all(&root).ok();
    }
    #[test]
    fn canonical_within_root_rejects_nul_byte() {
        let root = mk_tmp_dir("nul");
        let bad_str = "weird\0name";
        let bad = root.join(bad_str);
        let err = validate_canonical_within_root(&root, &bad).unwrap_err();
        let EnolaError::ValidationError(ref msg) = err else {
            unreachable!("expected ValidationError")
        };
        assert!(msg.contains("NUL") || msg.contains("traversal") || msg.contains("path"));
        std::fs::remove_dir_all(&root).ok();
    }
    #[test]
    fn canonical_within_root_accepts_nonexistent_leaf_within_root() {
        // Output file que aún no existe — el padre sí.
        let root = mk_tmp_dir("nonexistent");
        let f = root.join("output_will_be_created.jsonl");
        let resolved = validate_canonical_within_root(&root, &f).expect("ok");
        assert!(resolved.starts_with(root.canonicalize().unwrap()));
        std::fs::remove_dir_all(&root).ok();
    }
    #[test]
    fn canonical_within_root_rejects_nonexistent_leaf_outside_root() {
        let root = mk_tmp_dir("nx_out");
        let bad = std::path::PathBuf::from("/tmp/some_file_that_does_not_exist_xyz123");
        let err = validate_canonical_within_root(&root, &bad).unwrap_err();
        assert!(
            matches!(err, EnolaError::ValidationError(_)),
            "expected ValidationError, got {:?}",
            err
        );
        std::fs::remove_dir_all(&root).ok();
    }
    #[test]
    fn canonical_within_root_errors_when_root_does_not_exist() {
        let root = std::path::PathBuf::from("/tmp/enola_does_not_exist_abc999");
        let user = std::path::PathBuf::from("/tmp/whatever");
        let err = validate_canonical_within_root(&root, &user).unwrap_err();
        assert!(
            matches!(err, EnolaError::InfrastructureError(_)),
            "expected InfrastructureError, got {:?}",
            err
        );
    }

    // TEST-COV-UNIT-003: cubrir canon_parent.canonicalize() error
    // (parent dir no existe → ValidationError en rama no-existente)
    #[test]
    fn canonical_within_root_rejects_path_whose_parent_does_not_exist() {
        let root = mk_tmp_dir("par_nx");
        // El directorio padre "level1/" no existe dentro de root
        let bad = root.join("level1/file.txt");
        // bad.exists() = false → va a rama else → parent = root/level1/
        // root/level1/ no existe → canon_parent.canonicalize() falla
        let err = validate_canonical_within_root(&root, &bad).unwrap_err();
        assert!(
            matches!(err, EnolaError::ValidationError(_)),
            "expected ValidationError, got {:?}",
            err
        );
        std::fs::remove_dir_all(&root).ok();
    }

    // TEST-COV-UNIT-003: validate_service_name con nombre demasiado largo (>64)
    #[test]
    fn validate_service_name_rejects_too_long_name() {
        let long_name = "a".repeat(65);
        let err = validate_service_name(&long_name).unwrap_err();
        let EnolaError::ValidationError(ref msg) = err else {
            unreachable!("expected ValidationError")
        };
        assert!(
            msg.contains("length"),
            "mensaje debe mencionar length: {}",
            msg
        );
    }

    // TEST-COV-UNIT-003: validate_filename con control char
    #[test]
    fn validate_filename_rejects_control_characters() {
        let name_with_ctrl = "file\x01name.py";
        let err = validate_filename(name_with_ctrl).unwrap_err();
        let EnolaError::ValidationError(ref msg) = err else {
            unreachable!("expected ValidationError")
        };
        assert!(
            msg.contains("control"),
            "mensaje debe mencionar control: {}",
            msg
        );
    }

    // TEST-COV-UNIT-003: validate_filename con nombre muy largo (>255)
    #[test]
    fn validate_filename_rejects_too_long_name() {
        let long = "a".repeat(256);
        let err = validate_filename(&long).unwrap_err();
        let EnolaError::ValidationError(ref msg) = err else {
            unreachable!("expected ValidationError")
        };
        assert!(
            msg.contains("length") || msg.contains("255"),
            "msg: {}",
            msg
        );
    }

    // TEST-COV-UNIT-003: validate_filename con ".."
    #[test]
    fn validate_filename_rejects_dotdot() {
        let err = validate_filename("..").unwrap_err();
        assert!(
            matches!(err, EnolaError::ValidationError(_)),
            "expected ValidationError, got {:?}",
            err
        );
    }

    // TEST-COV-UNIT-003: validate_filename con slash
    #[test]
    fn validate_filename_rejects_slash() {
        let err = validate_filename("path/to/file").unwrap_err();
        assert!(
            matches!(err, EnolaError::ValidationError(_)),
            "expected ValidationError, got {:?}",
            err
        );
    }

    // TEST-COV-UNIT-003: validate_path_no_traversal con path vacío
    #[test]
    fn validate_path_no_traversal_rejects_empty_path() {
        let err = validate_path_no_traversal("").unwrap_err();
        let EnolaError::ValidationError(ref msg) = err else {
            unreachable!("expected ValidationError")
        };
        assert!(
            msg.contains("length") || msg.contains("4096"),
            "msg: {}",
            msg
        );
    }

    // TEST-COV-UNIT-003: validate_path_no_traversal con NUL byte
    #[test]
    fn validate_path_no_traversal_rejects_nul_byte() {
        let err = validate_path_no_traversal("file\0name").unwrap_err();
        let EnolaError::ValidationError(ref msg) = err else {
            unreachable!("expected ValidationError")
        };
        assert!(msg.contains("NUL"), "msg: {}", msg);
    }

    #[test]
    fn resolve_user_path_with_existing_path_canonicalize_error() {
        let p = PathBuf::from("/tmp/existing.txt");
        let err = resolve_user_path_with(
            &p,
            |_| true,
            |_pp| Err(std::io::Error::other("forced-canon-error")),
        )
        .unwrap_err();
        assert!(matches!(err, EnolaError::ValidationError(_)));
    }

    #[test]
    fn resolve_user_path_with_missing_path_and_no_parent() {
        let p = PathBuf::from("");
        let err =
            resolve_user_path_with(&p, |_| false, |_pp| Ok(PathBuf::from("/tmp"))).unwrap_err();
        assert!(matches!(err, EnolaError::ValidationError(_)));
    }

    #[test]
    fn resolve_user_path_with_missing_path_parent_canonicalize_error() {
        let p = PathBuf::from("a/b.txt");
        let err = resolve_user_path_with(
            &p,
            |_| false,
            |_pp| Err(std::io::Error::other("forced-parent-canon-error")),
        )
        .unwrap_err();
        assert!(matches!(err, EnolaError::ValidationError(_)));
    }

    #[test]
    fn resolve_user_path_with_missing_path_no_file_name() {
        let p = PathBuf::from("/tmp/..");
        let err =
            resolve_user_path_with(&p, |_| false, |_pp| Ok(PathBuf::from("/tmp"))).unwrap_err();
        assert!(matches!(err, EnolaError::ValidationError(_)));
    }
}
