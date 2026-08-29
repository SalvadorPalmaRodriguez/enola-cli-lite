use crate::domain::error::{EnolaError, Result};
use std::collections::HashSet;
use std::io::Write;
use std::path::Path;

pub struct GitCodeFlattener;

fn write_err(e: std::io::Error) -> EnolaError {
    EnolaError::InfrastructureError(format!("Write failed: {}", e))
}

impl GitCodeFlattener {
    pub fn flatten(
        root_path: &Path,
        extensions: &[String],
        ignore_dirs: &[String],
        output_path: &Path,
    ) -> Result<String> {
        let mut file = std::fs::File::create(output_path).map_err(|e| {
            EnolaError::InfrastructureError(format!("Failed to create output file: {}", e))
        })?;

        // Header
        writeln!(file, "# Project Context Export").map_err(write_err)?;
        writeln!(file, "# Source: {:?}", root_path).map_err(write_err)?;
        writeln!(file, "# Content:\n").map_err(write_err)?;

        let ignore_set: HashSet<&String> = ignore_dirs.iter().collect();
        let ext_set: HashSet<&String> = extensions.iter().collect();

        let mut stack = vec![root_path.to_path_buf()];

        while let Some(current_dir) = stack.pop() {
            let entries = std::fs::read_dir(&current_dir)
                .map_err(|e| EnolaError::InfrastructureError(format!("Read dir failed: {}", e)))?;

            for entry in entries {
                let entry = entry
                    .map_err(|e| EnolaError::InfrastructureError(format!("Entry fail: {}", e)))?;
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().to_string();

                if name.starts_with('.') && name != ".env" {
                    // Skip hidden except .env usually? Script says .git .vscode...
                    // Script has explicit ignore list but also standard hidden ignore.
                    // Standard hidden
                    continue;
                }

                if path.is_dir() {
                    if ignore_set.contains(&name) {
                        continue;
                    }
                    stack.push(path);
                } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    if ext_set.contains(&ext.to_string()) {
                        // Read and Write
                        let rel_path = path.strip_prefix(root_path).unwrap_or(&path);
                        let content_res = std::fs::read_to_string(&path);
                        // Only UTF-8
                        if let Ok(content) = content_res {
                            writeln!(file, "<file path=\"{}\">", rel_path.display())
                                .map_err(write_err)?;
                            writeln!(file, "{}", content).map_err(write_err)?;
                            writeln!(file, "</file>\n").map_err(write_err)?;
                        }
                    }
                } else {
                    // Handle strict name matches like "Dockerfile"
                    if ext_set.contains(&name) {
                        let rel_path = path.strip_prefix(root_path).unwrap_or(&path);
                        let content_res = std::fs::read_to_string(&path);
                        if let Ok(content) = content_res {
                            writeln!(file, "<file path=\"{}\">", rel_path.display())
                                .map_err(write_err)?;
                            writeln!(file, "{}", content).map_err(write_err)?;
                            writeln!(file, "</file>\n").map_err(write_err)?;
                        }
                    }
                }
            }
        }

        Ok(output_path.to_string_lossy().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// Helper: crea estructura de directorios de prueba
    fn create_test_project() -> TempDir {
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        // Crear archivos de código
        fs::write(root.join("main.rs"), "fn main() { println!(\"hello\"); }").unwrap();
        fs::write(root.join("lib.rs"), "pub mod utils;").unwrap();
        fs::write(root.join("Dockerfile"), "FROM rust:latest").unwrap();
        fs::write(root.join("README.md"), "# Project").unwrap();

        // Crear subdirectorio src/
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/utils.rs"), "pub fn helper() {}").unwrap();

        // Crear directorio a ignorar (.git/)
        fs::create_dir_all(root.join(".git/objects")).unwrap();
        fs::write(root.join(".git/config"), "[core]\n").unwrap();

        // Crear directorio target/ (típicamente ignorado)
        fs::create_dir_all(root.join("target/debug")).unwrap();
        fs::write(root.join("target/debug/binary"), "ELF...").unwrap();

        // Crear archivo .env (debe incluirse a pesar de ser oculto)
        fs::write(root.join(".env"), "SECRET=123").unwrap();

        // Crear archivo binario (no UTF-8, se ignora)
        fs::write(root.join("image.png"), [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A]).unwrap();

        dir
    }

    #[test]
    fn test_flatten_basic() {
        let project = create_test_project();
        let output = TempDir::new().unwrap();
        let output_file = output.path().join("context.md");

        let extensions = vec!["rs".to_string(), "md".to_string()];
        let ignore_dirs = vec!["target".to_string()];

        let result =
            GitCodeFlattener::flatten(project.path(), &extensions, &ignore_dirs, &output_file);

        assert!(result.is_ok());
        assert!(output_file.exists());

        let content = fs::read_to_string(&output_file).unwrap();
        assert!(content.contains("# Project Context Export"));
        assert!(content.contains("main.rs"));
        assert!(content.contains("fn main()"));
    }

    #[test]
    fn test_flatten_includes_dockerfile() {
        let project = create_test_project();
        let output = TempDir::new().unwrap();
        let output_file = output.path().join("context.md");

        // Dockerfile se trata como nombre completo, no extensión
        let extensions = vec!["rs".to_string(), "Dockerfile".to_string()];
        let ignore_dirs = vec!["target".to_string()];

        let result =
            GitCodeFlattener::flatten(project.path(), &extensions, &ignore_dirs, &output_file);

        assert!(result.is_ok());
        let content = fs::read_to_string(&output_file).unwrap();
        assert!(content.contains("Dockerfile"));
        assert!(content.contains("FROM rust:latest"));
    }

    #[test]
    fn test_flatten_ignores_target() {
        let project = create_test_project();
        let output = TempDir::new().unwrap();
        let output_file = output.path().join("context.md");

        let extensions = vec!["rs".to_string()];
        let ignore_dirs = vec!["target".to_string()];

        let result =
            GitCodeFlattener::flatten(project.path(), &extensions, &ignore_dirs, &output_file);

        assert!(result.is_ok());
        let content = fs::read_to_string(&output_file).unwrap();
        // No debe contener archivos de target/
        assert!(!content.contains("target/debug"));
        assert!(!content.contains("binary"));
    }

    #[test]
    fn test_flatten_ignores_hidden_dirs() {
        let project = create_test_project();
        let output = TempDir::new().unwrap();
        let output_file = output.path().join("context.md");

        let extensions = vec!["rs".to_string()];
        let ignore_dirs = vec![];

        let result =
            GitCodeFlattener::flatten(project.path(), &extensions, &ignore_dirs, &output_file);

        assert!(result.is_ok());
        let content = fs::read_to_string(&output_file).unwrap();
        // .git/ se ignora automáticamente (hidden dir)
        assert!(!content.contains(".git/config"));
    }

    #[test]
    fn test_flatten_includes_subdirectories() {
        let project = create_test_project();
        let output = TempDir::new().unwrap();
        let output_file = output.path().join("context.md");

        let extensions = vec!["rs".to_string()];
        let ignore_dirs = vec!["target".to_string()];

        let result =
            GitCodeFlattener::flatten(project.path(), &extensions, &ignore_dirs, &output_file);

        assert!(result.is_ok());
        let content = fs::read_to_string(&output_file).unwrap();
        // Debe incluir archivos en src/
        assert!(content.contains("utils.rs"));
        assert!(content.contains("pub fn helper()"));
    }

    #[test]
    fn test_flatten_filters_by_extension() {
        let project = create_test_project();
        let output = TempDir::new().unwrap();
        let output_file = output.path().join("context.md");

        // Solo archivos .md
        let extensions = vec!["md".to_string()];
        let ignore_dirs = vec!["target".to_string()];

        let result =
            GitCodeFlattener::flatten(project.path(), &extensions, &ignore_dirs, &output_file);

        assert!(result.is_ok());
        let content = fs::read_to_string(&output_file).unwrap();
        // Solo README.md, no archivos .rs
        assert!(content.contains("README.md"));
        assert!(content.contains("# Project"));
        assert!(!content.contains("fn main()"));
    }

    #[test]
    fn test_flatten_empty_extensions() {
        let project = create_test_project();
        let output = TempDir::new().unwrap();
        let output_file = output.path().join("context.md");

        // Sin extensiones = archivo vacío (solo header)
        let extensions: Vec<String> = vec![];
        let ignore_dirs = vec!["target".to_string()];

        let result =
            GitCodeFlattener::flatten(project.path(), &extensions, &ignore_dirs, &output_file);

        assert!(result.is_ok());
        let content = fs::read_to_string(&output_file).unwrap();
        // Solo el header, sin archivos
        assert!(content.contains("# Project Context Export"));
        assert!(!content.contains("<file path="));
    }

    #[test]
    fn test_flatten_nonexistent_root_fails() {
        let output = TempDir::new().unwrap();
        let output_file = output.path().join("context.md");

        let result = GitCodeFlattener::flatten(
            Path::new("/nonexistent/path/that/does/not/exist"),
            &["rs".to_string()],
            &[],
            &output_file,
        );

        assert!(result.is_err());
    }

    #[test]
    fn test_flatten_invalid_output_path_fails() {
        let project = create_test_project();

        let result = GitCodeFlattener::flatten(
            project.path(),
            &["rs".to_string()],
            &[],
            Path::new("/nonexistent/dir/output.md"),
        );

        assert!(result.is_err());
    }

    // ── Error-path / edge-case tests ──

    #[test]
    fn test_flatten_binary_file_skipped() {
        let project = create_test_project();
        let output = TempDir::new().unwrap();
        let output_file = output.path().join("context.md");

        // png is not in extensions, but even if it were, binary content is skipped
        let extensions = vec!["rs".to_string(), "png".to_string()];
        let ignore_dirs = vec!["target".to_string()];

        let result =
            GitCodeFlattener::flatten(project.path(), &extensions, &ignore_dirs, &output_file);

        assert!(result.is_ok());
        let content = fs::read_to_string(&output_file).unwrap();
        // Binary file should not appear (non-UTF-8 is silently skipped)
        assert!(!content.contains("image.png"));
    }

    #[test]
    fn test_flatten_env_file_included_when_extension_matches() {
        let project = create_test_project();
        let output = TempDir::new().unwrap();
        let output_file = output.path().join("context.md");

        // .env is a hidden file but explicitly allowed (name != ".env" check).
        // It has no extension, so it's matched by strict name match in the else branch.
        let extensions = vec![".env".to_string()];
        let ignore_dirs = vec!["target".to_string()];

        let result =
            GitCodeFlattener::flatten(project.path(), &extensions, &ignore_dirs, &output_file);

        assert!(result.is_ok());
        let content = fs::read_to_string(&output_file).unwrap();
        assert!(content.contains("SECRET=123"));
    }

    #[test]
    fn test_flatten_empty_project() {
        let dir = TempDir::new().unwrap();
        let output = TempDir::new().unwrap();
        let output_file = output.path().join("context.md");

        let extensions = vec!["rs".to_string()];
        let ignore_dirs = vec![];

        let result = GitCodeFlattener::flatten(dir.path(), &extensions, &ignore_dirs, &output_file);

        assert!(result.is_ok());
        let content = fs::read_to_string(&output_file).unwrap();
        assert!(content.contains("# Project Context Export"));
        assert!(!content.contains("<file path="));
    }

    #[test]
    fn test_flatten_nested_ignored_dir() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        fs::create_dir_all(root.join("src/ignored")).unwrap();
        fs::write(root.join("src/ignored/secret.rs"), "pub fn secret() {}").unwrap();
        fs::write(root.join("src/main.rs"), "fn main() {}").unwrap();

        let output = TempDir::new().unwrap();
        let output_file = output.path().join("context.md");

        let extensions = vec!["rs".to_string()];
        let ignore_dirs = vec!["ignored".to_string()];

        let result = GitCodeFlattener::flatten(root, &extensions, &ignore_dirs, &output_file);

        assert!(result.is_ok());
        let content = fs::read_to_string(&output_file).unwrap();
        assert!(content.contains("main.rs"));
        assert!(!content.contains("secret.rs"));
    }

    #[test]
    fn test_flatten_returns_output_path() {
        let project = create_test_project();
        let output = TempDir::new().unwrap();
        let output_file = output.path().join("context.md");

        let extensions = vec!["rs".to_string()];
        let ignore_dirs = vec!["target".to_string()];

        let result =
            GitCodeFlattener::flatten(project.path(), &extensions, &ignore_dirs, &output_file);

        assert!(result.is_ok());
        let returned_path = result.unwrap();
        assert!(returned_path.contains("context.md"));
    }
}
