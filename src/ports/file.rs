use crate::domain::error::Result;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[async_trait::async_trait]
pub trait FileManagerPort {
    /// Read entire file as string
    async fn read_file(&self, path: &Path) -> Result<String>;

    /// Write string to file (atomic move preferred)
    async fn write_file(&self, path: &Path, content: &str) -> Result<()>;

    /// Ensure directory exists
    async fn ensure_dir(&self, path: &Path) -> Result<()>;

    /// Read .env style file into Map
    async fn read_env(&self, path: &Path) -> Result<HashMap<String, String>>;

    /// Update or Insert a key in a .env file, preserving other lines/comments if possible
    async fn update_env_key(&self, path: &Path, key: &str, value: &str) -> Result<()>;

    /// Securely delete a file
    async fn delete_file(&self, path: &Path) -> Result<()>;

    /// Copy a file
    async fn copy_file(&self, from: &Path, to: &Path) -> Result<()>;

    /// Set file ownership (chown) - user:group
    async fn set_ownership(&self, path: &Path, user: &str, group: &str) -> Result<()>;

    /// Set file permissions (chmod) - octal mode e.g. 0o755
    async fn set_permissions(&self, path: &Path, mode: u32) -> Result<()>;

    /// Create a tar.gz archive from source_dir into dest_file
    async fn create_archive(&self, source_dir: &Path, dest_file: &Path) -> Result<()>;

    /// Create a tar.gz archive containing multiple absolute paths,
    /// preserving their directory structure relative to `/` so that
    /// `extract_archive(.., "/")` restores them to their original locations.
    /// Each path must be absolute and fully normalized (no `.`/`..` components).
    async fn create_archive_multi(&self, paths: &[PathBuf], dest_file: &Path) -> Result<()>;

    /// Extract a tar.gz archive into dest_dir
    async fn extract_archive(&self, archive: &Path, dest_dir: &Path) -> Result<()>;
}

/// Port para operaciones de archivo atómicas (sin TOCTOU).
/// Segregado de FileManagerPort siguiendo ISP (Interface Segregation Principle).
/// Los clientes que necesitan atomicidad dependen solo de este port.
#[async_trait::async_trait]
pub trait AtomicFilePort {
    /// Escribe archivo atómicamente con permisos `mode` (ej: 0o600, 0o644).
    /// Usa tempfile + rename. El archivo nunca existe en estado intermedio.
    async fn write_atomic(&self, path: &Path, content: &[u8], mode: u32) -> Result<()>;

    /// Borra archivo sin TOCTOU. No usa `exists()` previo.
    /// Retorna `Ok(())` si el archivo no existía.
    async fn delete_safe(&self, path: &Path) -> Result<()>;
}

/// Dueño (uid, gid) de un directorio o fichero de usuario.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UserOwner {
    pub uid: u32,
    pub gid: u32,
}

/// Edición de un fichero: recibe el contenido actual (vacío si no existe) y
/// devuelve `Some(nuevo)` para escribirlo o `None` para no tocarlo.
pub type FileEdit = Box<dyn FnOnce(&str) -> Result<Option<String>> + Send>;

/// SEC-A8: operaciones de root dentro de un directorio de OTRO usuario
/// (p. ej. `~/.ssh`). Segregado (ISP): el adapter valida el directorio una vez
/// (sin seguir symlinks) y opera relativo a él.
#[async_trait::async_trait]
pub trait UserDirFilePort {
    /// Abre `dir` (lo crea con `dir_mode` y dueño `owner` si no existe),
    /// exigiendo un directorio real de `owner.uid` o de root sin escritura de
    /// grupo/otros. Bajo lock exclusivo `lock_name` lee `name` (vacío si no
    /// existe), aplica `edit` y, si devuelve `Some`, escribe atómicamente con
    /// `file_mode` y dueño `owner`. Nunca sigue symlinks.
    #[allow(clippy::too_many_arguments)]
    async fn edit_locked(
        &self,
        dir: &Path,
        owner: UserOwner,
        dir_mode: u32,
        lock_name: &str,
        name: &str,
        file_mode: u32,
        edit: FileEdit,
    ) -> Result<()>;
}

#[cfg(test)]
mockall::mock! {
    pub UserDirFilePort {}
    #[async_trait::async_trait]
    impl UserDirFilePort for UserDirFilePort {
        #[allow(clippy::too_many_arguments)]
        async fn edit_locked(
            &self,
            dir: &Path,
            owner: UserOwner,
            dir_mode: u32,
            lock_name: &str,
            name: &str,
            file_mode: u32,
            edit: FileEdit,
        ) -> Result<()>;
    }
}

#[cfg(test)]
mockall::mock! {
    pub AtomicFilePort {}
    #[async_trait::async_trait]
    impl AtomicFilePort for AtomicFilePort {
        async fn write_atomic(&self, path: &Path, content: &[u8], mode: u32) -> Result<()>;
        async fn delete_safe(&self, path: &Path) -> Result<()>;
    }
}

#[cfg(test)]
mockall::mock! {
    pub FileManagerPort {}
    #[async_trait::async_trait]
    impl FileManagerPort for FileManagerPort {
        async fn read_file(&self, path: &Path) -> Result<String>;
        async fn write_file(&self, path: &Path, content: &str) -> Result<()>;
        async fn ensure_dir(&self, path: &Path) -> Result<()>;
        async fn read_env(&self, path: &Path) -> Result<HashMap<String, String>>;
        async fn update_env_key(&self, path: &Path, key: &str, value: &str) -> Result<()>;
        async fn delete_file(&self, path: &Path) -> Result<()>;
        async fn copy_file(&self, from: &Path, to: &Path) -> Result<()>;
        async fn set_ownership(&self, path: &Path, user: &str, group: &str) -> Result<()>;
        async fn set_permissions(&self, path: &Path, mode: u32) -> Result<()>;
        async fn create_archive(&self, source_dir: &Path, dest_file: &Path) -> Result<()>;
        async fn create_archive_multi(&self, paths: &[PathBuf], dest_file: &Path) -> Result<()>;
        async fn extract_archive(&self, archive: &Path, dest_dir: &Path) -> Result<()>;
    }
}
