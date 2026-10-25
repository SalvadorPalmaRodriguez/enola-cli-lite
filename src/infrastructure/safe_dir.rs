// SEC-A2/A6/A8: operaciones de root sobre directorios que controla otro usuario.
//
// **Causa raíz que cierra**: root operaba *por ruta* dentro de directorios ajenos
// (`/run/lock` 1777, `~/.enola`, `~/.ssh`): las comprobaciones previas (`exists`,
// `metadata`) siguen symlinks y dejan una ventana entre comprobar y usar, y los
// `chown`/`fs::write` posteriores siguen symlinks plantados por el dueño del
// directorio.
//
// **Diseño**:
// - `SafeDir::open` abre el directorio UNA vez con `O_DIRECTORY | O_NOFOLLOW` y
//   valida dueño y modo con `fstat` sobre ese fd.
// - Todas las operaciones posteriores son relativas al fd (`openat`, `renameat`,
//   `unlinkat`): aunque la ruta original se sustituya después, se sigue operando
//   dentro del directorio validado.
// - `write_atomic` aplica dueño y modo sobre el fd del temporal ANTES del rename
//   (`fchown`/`fchmod`), nunca por ruta después.
// - Los nombres son un único componente (sin `/`, `.` ni `..`).
use std::ffi::CString;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

/// Quién puede ser dueño del directorio para que root confíe en él.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnerPolicy {
    /// Dueño = euid del proceso (directorios propios de root: locks, staging).
    Euid,
    /// Dueño = un uid concreto.
    Uid(u32),
    /// Dueño = root o un uid concreto (directorios del usuario invocador).
    RootOrUid(u32),
}

impl OwnerPolicy {
    fn allows(self, uid: u32) -> bool {
        match self {
            // SAFETY: geteuid no tiene precondiciones.
            OwnerPolicy::Euid => uid == unsafe { libc::geteuid() },
            OwnerPolicy::Uid(u) => uid == u,
            OwnerPolicy::RootOrUid(u) => uid == 0 || uid == u,
        }
    }
}

/// Qué hacer con el modo de un directorio ya validado (tipo y dueño).
#[derive(Debug, Clone, Copy)]
enum ModeFix {
    /// Rechazar si hay escritura de grupo/otros.
    Reject,
    /// Quitar los permisos que no estén en la máscara (nunca añade ninguno).
    Clamp(u32),
    /// Fijar exactamente este modo.
    Exact(u32),
}

/// Directorio abierto y validado; las operaciones son relativas a su fd.
#[derive(Debug)]
pub struct SafeDir {
    fd: OwnedFd,
    path: PathBuf,
    /// `(modo_anterior, modo_nuevo)` si al abrir se corrigieron permisos.
    corrected: Option<(u32, u32)>,
}

fn path_cstring(path: &Path) -> io::Result<CString> {
    CString::new(path.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains a NUL byte"))
}

fn fstat(fd: libc::c_int) -> io::Result<libc::stat> {
    // SAFETY: `st` es memoria propia del tamaño correcto; fstat la rellena.
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: fd válido mientras el llamador lo mantenga abierto.
    if unsafe { libc::fstat(fd, &mut st) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(st)
}

fn is_type(st: &libc::stat, kind: libc::mode_t) -> bool {
    st.st_mode & libc::S_IFMT == kind
}

impl SafeDir {
    /// Abre `path` sin seguir un symlink final y valida que sea un directorio
    /// del dueño permitido por `policy` y sin escritura de grupo/otros.
    pub fn open(path: &Path, policy: OwnerPolicy) -> io::Result<Self> {
        Self::open_with(path, policy, ModeFix::Reject)
    }

    /// Como `open`, pero el modo se corrige sobre el fd (tras validar tipo y
    /// dueño) en lugar de rechazar: ver [`ModeFix`].
    fn open_with(path: &Path, policy: OwnerPolicy, fix: ModeFix) -> io::Result<Self> {
        let c = path_cstring(path)?;
        // SAFETY: `c` es una cadena C válida; el fd devuelto pasa a OwnedFd.
        let raw = unsafe {
            libc::open(
                c.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if raw < 0 {
            let e = io::Error::last_os_error();
            return Err(io::Error::new(
                e.kind(),
                format!(
                    "{}: no es un directorio real accesible (¿symlink?): {}",
                    path.display(),
                    e
                ),
            ));
        }
        // SAFETY: `raw` es un fd recién abierto y propio.
        let fd = unsafe { OwnedFd::from_raw_fd(raw) };
        let st = fstat(fd.as_raw_fd())?;
        if !is_type(&st, libc::S_IFDIR) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{}: no es un directorio", path.display()),
            ));
        }
        if !policy.allows(st.st_uid) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "{}: dueño uid {} no permitido ({:?})",
                    path.display(),
                    st.st_uid,
                    policy
                ),
            ));
        }
        let current = st.st_mode & 0o7777;
        let wanted = match fix {
            ModeFix::Reject => current,
            ModeFix::Clamp(mask) => current & mask as libc::mode_t,
            ModeFix::Exact(m) => m as libc::mode_t,
        };
        let mut corrected = None;
        if wanted != current {
            // SAFETY: fd válido y propio; tipo y dueño ya validados.
            if unsafe { libc::fchmod(fd.as_raw_fd(), wanted) } != 0 {
                return Err(io::Error::last_os_error());
            }
            corrected = Some((current as u32, wanted as u32));
        } else if st.st_mode & 0o022 != 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "{}: escribible por grupo/otros (modo {:o})",
                    path.display(),
                    st.st_mode & 0o7777
                ),
            ));
        }
        Ok(SafeDir {
            fd,
            path: path.to_path_buf(),
            corrected,
        })
    }

    /// Como `open`, pero si `path` no existe lo crea (un solo nivel; el padre
    /// debe existir) con exactamente `mode`, independientemente del umask.
    pub fn open_or_create(path: &Path, mode: u32, policy: OwnerPolicy) -> io::Result<Self> {
        let c = path_cstring(path)?;
        // SAFETY: `c` es una cadena C válida. mkdir no sigue un symlink final:
        // si existe cualquier entrada (también un symlink) devuelve EEXIST.
        let rc = unsafe { libc::mkdir(c.as_ptr(), 0o700) };
        let created = if rc == 0 {
            true
        } else {
            let e = io::Error::last_os_error();
            if e.raw_os_error() != Some(libc::EEXIST) {
                return Err(e);
            }
            false
        };
        let dir = SafeDir::open(path, policy)?;
        if created {
            // Modo exacto sobre el fd (el umask no interviene).
            // SAFETY: fd válido y propio.
            if unsafe { libc::fchmod(dir.fd.as_raw_fd(), mode as libc::mode_t) } != 0 {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(dir)
    }

    /// Como `open_or_create`, pero si lo crea le asigna `owner` (uid, gid)
    /// con `fchown` sobre el fd; si ya existía, lo valida con `policy`.
    pub fn open_or_create_owned(
        path: &Path,
        mode: u32,
        owner: (u32, u32),
        policy: OwnerPolicy,
    ) -> io::Result<Self> {
        let c = path_cstring(path)?;
        // SAFETY: `c` es una cadena C válida; mkdir no sigue un symlink final.
        if unsafe { libc::mkdir(c.as_ptr(), 0o700) } != 0 {
            let e = io::Error::last_os_error();
            if e.raw_os_error() != Some(libc::EEXIST) {
                return Err(e);
            }
            return SafeDir::open(path, policy);
        }
        // Recién creado por nosotros: dueño y modo sobre el fd, antes de que
        // nadie más pueda usarlo.
        let dir = SafeDir::open(path, OwnerPolicy::Euid)?;
        let fd = dir.fd.as_raw_fd();
        // SAFETY: fd válido y propio.
        if unsafe { libc::fchown(fd, owner.0, owner.1) } != 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: fd válido y propio.
        if unsafe { libc::fchmod(fd, mode as libc::mode_t) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(dir)
    }

    /// `(modo_anterior, modo_nuevo)` si al abrir se corrigieron permisos.
    pub fn mode_correction(&self) -> Option<(u32, u32)> {
        self.corrected
    }

    /// Ruta con la que se abrió (solo para mensajes).
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Escribe `name` de forma atómica dentro del directorio: temporal
    /// `O_EXCL | O_NOFOLLOW`, `fchown` (si `owner`) y `fchmod(mode)` sobre su fd,
    /// `fsync`, `renameat` y `fsync` del directorio.
    pub fn write_atomic(
        &self,
        name: &str,
        bytes: &[u8],
        mode: u32,
        owner: Option<(u32, u32)>,
    ) -> io::Result<()> {
        let final_c = component_cstring(name)?;
        let tmp_name = format!(
            ".{}.tmp.{}.{:016x}",
            name,
            std::process::id(),
            rand::random::<u64>()
        );
        let tmp_c = component_cstring(&tmp_name)?;
        let dirfd = self.fd.as_raw_fd();
        // SAFETY: dirfd válido, tmp_c cadena C válida; el fd pasa a File.
        let raw = unsafe {
            libc::openat(
                dirfd,
                tmp_c.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o600 as libc::c_uint,
            )
        };
        if raw < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `raw` es un fd recién abierto y propio.
        let mut file = unsafe { File::from_raw_fd(raw) };
        let result = (|| -> io::Result<()> {
            if let Some((uid, gid)) = owner {
                // SAFETY: fd válido mientras `file` viva.
                if unsafe { libc::fchown(file.as_raw_fd(), uid, gid) } != 0 {
                    return Err(io::Error::last_os_error());
                }
            }
            // SAFETY: fd válido mientras `file` viva.
            if unsafe { libc::fchmod(file.as_raw_fd(), mode as libc::mode_t) } != 0 {
                return Err(io::Error::last_os_error());
            }
            file.write_all(bytes)?;
            file.sync_all()?;
            // SAFETY: dirfd válido y ambas cadenas C válidas.
            if unsafe { libc::renameat(dirfd, tmp_c.as_ptr(), dirfd, final_c.as_ptr()) } != 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        })();
        if let Err(e) = result {
            // Limpieza best-effort del temporal (relativa al fd, sin seguir symlinks).
            // SAFETY: dirfd válido y cadena C válida.
            unsafe {
                libc::unlinkat(dirfd, tmp_c.as_ptr(), 0);
            }
            return Err(e);
        }
        // Durabilidad del rename: best-effort, el contenido ya está en disco.
        // SAFETY: dirfd válido.
        unsafe {
            libc::fsync(dirfd);
        }
        Ok(())
    }

    /// Abre (y crea con `mode` si no existe) el fichero `name` para lectura y
    /// escritura sin seguir symlinks; falla si no es un fichero regular.
    pub fn open_file_nofollow(&self, name: &str, mode: u32) -> io::Result<File> {
        let c = component_cstring(name)?;
        // SAFETY: fd de directorio válido, cadena C válida; el fd pasa a File.
        let raw = unsafe {
            libc::openat(
                self.fd.as_raw_fd(),
                c.as_ptr(),
                libc::O_RDWR | libc::O_CREAT | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                mode as libc::c_uint,
            )
        };
        if raw < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `raw` es un fd recién abierto y propio.
        let file = unsafe { File::from_raw_fd(raw) };
        let st = fstat(file.as_raw_fd())?;
        if !is_type(&st, libc::S_IFREG) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{}/{}: no es un fichero regular", self.path.display(), name),
            ));
        }
        Ok(file)
    }

    /// Abre (y crea con `mode` si no existe) `name` en modo append sin seguir
    /// symlinks; falla si no es un fichero regular (logs de auditoría, A25-04).
    pub fn open_append_nofollow(&self, name: &str, mode: u32) -> io::Result<File> {
        let c = component_cstring(name)?;
        // SAFETY: fd de directorio válido, cadena C válida; el fd pasa a File.
        let raw = unsafe {
            libc::openat(
                self.fd.as_raw_fd(),
                c.as_ptr(),
                libc::O_WRONLY
                    | libc::O_APPEND
                    | libc::O_CREAT
                    | libc::O_NOFOLLOW
                    | libc::O_CLOEXEC,
                mode as libc::c_uint,
            )
        };
        if raw < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `raw` es un fd recién abierto y propio.
        let file = unsafe { File::from_raw_fd(raw) };
        let st = fstat(file.as_raw_fd())?;
        if !is_type(&st, libc::S_IFREG) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{}/{}: no es un fichero regular", self.path.display(), name),
            ));
        }
        Ok(file)
    }

    /// Lee `name` como UTF-8 sin seguir symlinks. `Ok(None)` si no existe;
    /// error si no es un fichero regular o supera `max_bytes`.
    pub fn read_to_string_nofollow(
        &self,
        name: &str,
        max_bytes: u64,
    ) -> io::Result<Option<String>> {
        let c = component_cstring(name)?;
        // SAFETY: fd de directorio válido, cadena C válida; el fd pasa a File.
        let raw = unsafe {
            libc::openat(
                self.fd.as_raw_fd(),
                c.as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
            )
        };
        if raw < 0 {
            let e = io::Error::last_os_error();
            if e.raw_os_error() == Some(libc::ENOENT) {
                return Ok(None);
            }
            return Err(e);
        }
        // SAFETY: `raw` es un fd recién abierto y propio.
        let file = unsafe { File::from_raw_fd(raw) };
        let st = fstat(file.as_raw_fd())?;
        if !is_type(&st, libc::S_IFREG) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{}/{}: no es un fichero regular", self.path.display(), name),
            ));
        }
        if st.st_size as u64 > max_bytes {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "{}/{}: excede el máximo de {} bytes",
                    self.path.display(),
                    name,
                    max_bytes
                ),
            ));
        }
        let mut s = String::new();
        file.take(max_bytes + 1).read_to_string(&mut s)?;
        if s.len() as u64 > max_bytes {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "{}/{}: creció por encima del máximo",
                    self.path.display(),
                    name
                ),
            ));
        }
        Ok(Some(s))
    }
}

/// SEC-A6/A8: abre un directorio del usuario (`~/.ssh`, `~/.enola`): sin
/// seguir symlinks y con dueño permitido por `policy`; si tiene más permisos
/// que `mode` se le QUITAN (nunca se añaden) y se avisa por stderr. Si no
/// existe, se crea con `mode` y dueño `owner`.
pub fn open_user_dir(
    path: &Path,
    mode: u32,
    owner: (u32, u32),
    policy: OwnerPolicy,
) -> io::Result<SafeDir> {
    let dir = match SafeDir::open_with(path, policy, ModeFix::Clamp(mode)) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return SafeDir::open_or_create_owned(path, mode, owner, policy);
        }
        other => other?,
    };
    if let Some((from, to)) = dir.mode_correction() {
        eprintln!(
            "⚠️  Permisos de {} corregidos de {:o} a {:o} (solo su dueño debe acceder).",
            path.display(),
            from,
            to
        );
    }
    Ok(dir)
}

/// SEC-A6: dueño a aplicar a lo que root escribe en el home del usuario
/// invocador de `sudo` (`None` si no somos root bajo sudo).
pub fn invoker_owner() -> Option<(u32, u32)> {
    // SAFETY: geteuid no tiene precondiciones.
    if unsafe { libc::geteuid() } != 0 {
        return None;
    }
    crate::infrastructure::drop_privs::invoking_user().map(|u| (u.uid, u.gid))
}

/// SEC-A6: abre el directorio de configuración de enola del usuario
/// (`~/.enola`) sin seguir symlinks y exigiendo dueño = `owner` (o el proceso)
/// o root. Si tiene escritura de grupo/otros se la QUITA (es el directorio de
/// enola; solo se reducen permisos, tras validar dueño sobre el fd). Si no
/// existe, lo crea `0700` con ese dueño.
pub fn open_user_config_dir(dir: &Path, owner: Option<(u32, u32)>) -> io::Result<SafeDir> {
    // SAFETY: geteuid/getegid no tienen precondiciones.
    let (uid, gid) = owner.unwrap_or_else(|| unsafe { (libc::geteuid(), libc::getegid()) });
    open_user_dir(dir, 0o700, (uid, gid), OwnerPolicy::RootOrUid(uid))
}

/// SEC-A7 (D13): el directorio base de datos de un servicio
/// (`/srv/enola-<svc>`) debe ser un directorio real del euid (root) en modo
/// exactamente `0700`: los secretos que escriben los contenedores (p. ej.
/// `wp-config.php` 0644) quedan inalcanzables para otros usuarios locales. Los
/// contenedores no atraviesan los padres del host en un bind mount. Si no
/// existe se crea `0700`; si existe se valida (sin seguir symlinks) y se fija
/// el modo sobre el fd.
pub fn secure_service_base_dir(base: &Path) -> io::Result<()> {
    match SafeDir::open_with(base, OwnerPolicy::Euid, ModeFix::Exact(0o700)) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            if let Some(parent) = base.parent() {
                std::fs::create_dir_all(parent)?;
            }
            SafeDir::open_or_create(base, 0o700, OwnerPolicy::Euid).map(|_| ())
        }
        other => other.map(|_| ()),
    }
}

/// SEC-A7: directorios base de datos de los servicios (todos `0700`).
/// `/srv/enola-files` NO está: Nginx (www-data) sirve desde sus shares.
/// KEEP-IN-SYNC: `DEFAULT_*_BASE_DIR` de `adapters/cms/*`, `deploy_wordpress`,
/// `deploy_git_server` y `domain::ai::DEFAULT_AI_BASE_DIR`.
pub const SERVICE_DATA_BASES: [&str; 8] = [
    "/srv/enola-git",
    "/srv/enola-wordpress",
    "/srv/enola-drupal",
    "/srv/enola-ghost",
    "/srv/enola-magnolia",
    "/srv/enola-strapi",
    "/srv/enola-wagtail",
    "/srv/enola-ai",
];

/// SEC-A7 (D3): corrige el modo de los directorios base que YA existen (no
/// crea ninguno). Solo como root; los errores se avisan por stderr y no abortan.
pub fn secure_existing_service_bases(bases: &[&str]) {
    // SAFETY: geteuid no tiene precondiciones.
    if unsafe { libc::geteuid() } != 0 {
        return;
    }
    for base in bases {
        let p = Path::new(base);
        if std::fs::symlink_metadata(p).is_err() {
            continue;
        }
        if let Err(e) = secure_service_base_dir(p) {
            eprintln!("⚠️  {}: could not restrict to 0700: {}", base, e);
        }
    }
}

/// Valida que `name` sea un único componente de ruta seguro.
fn component_cstring(name: &str) -> io::Result<CString> {
    if name.is_empty() || name == "." || name == ".." || name.contains('/') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("nombre de fichero no válido: {:?}", name),
        ));
    }
    CString::new(name)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "nombre con byte NUL"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    use tempfile::TempDir;

    fn euid() -> u32 {
        // SAFETY: geteuid no tiene precondiciones.
        unsafe { libc::geteuid() }
    }

    fn egid() -> u32 {
        // SAFETY: getegid no tiene precondiciones.
        unsafe { libc::getegid() }
    }

    fn private_dir(tmp: &TempDir, name: &str) -> PathBuf {
        let d = tmp.path().join(name);
        std::fs::create_dir(&d).unwrap();
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o700)).unwrap();
        d
    }

    #[test]
    fn open_accepts_own_private_dir() {
        let tmp = TempDir::new().unwrap();
        let d = private_dir(&tmp, "ok");
        let sd = SafeDir::open(&d, OwnerPolicy::Euid).expect("own 0700 dir must open");
        assert_eq!(sd.path(), d.as_path());
    }

    #[test]
    fn open_rejects_symlinked_dir() {
        let tmp = TempDir::new().unwrap();
        let real = private_dir(&tmp, "real");
        let link = tmp.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert!(SafeDir::open(&link, OwnerPolicy::Euid).is_err());
    }

    #[test]
    fn open_rejects_world_writable_dir() {
        let tmp = TempDir::new().unwrap();
        let d = private_dir(&tmp, "ww");
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o777)).unwrap();
        assert!(SafeDir::open(&d, OwnerPolicy::Euid).is_err());
    }

    #[test]
    fn open_rejects_group_writable_dir() {
        let tmp = TempDir::new().unwrap();
        let d = private_dir(&tmp, "gw");
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o770)).unwrap();
        assert!(SafeDir::open(&d, OwnerPolicy::Euid).is_err());
    }

    #[test]
    fn open_rejects_foreign_owner() {
        // El directorio es del euid del test; una política que exige otro uid
        // (y no root) debe rechazarlo. Cubre la rama sin necesitar root.
        let tmp = TempDir::new().unwrap();
        let d = private_dir(&tmp, "foreign");
        let other = euid().wrapping_add(1);
        assert!(SafeDir::open(&d, OwnerPolicy::Uid(other)).is_err());
        if euid() != 0 {
            assert!(SafeDir::open(&d, OwnerPolicy::RootOrUid(other)).is_err());
        }
    }

    #[test]
    fn open_accepts_root_or_uid_for_invoker_dir() {
        let tmp = TempDir::new().unwrap();
        let d = private_dir(&tmp, "invoker");
        assert!(SafeDir::open(&d, OwnerPolicy::RootOrUid(euid())).is_ok());
    }

    #[test]
    fn open_rejects_regular_file() {
        let tmp = TempDir::new().unwrap();
        let f = tmp.path().join("file");
        std::fs::write(&f, b"x").unwrap();
        assert!(SafeDir::open(&f, OwnerPolicy::Euid).is_err());
    }

    #[test]
    fn open_or_create_creates_with_exact_mode() {
        let tmp = TempDir::new().unwrap();
        let d = tmp.path().join("new");
        let _sd = SafeDir::open_or_create(&d, 0o700, OwnerPolicy::Euid).expect("create");
        let md = std::fs::symlink_metadata(&d).unwrap();
        assert!(md.is_dir());
        assert_eq!(md.permissions().mode() & 0o777, 0o700);
    }

    #[test]
    fn open_or_create_validates_existing_dir() {
        let tmp = TempDir::new().unwrap();
        let d = private_dir(&tmp, "exists");
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o777)).unwrap();
        assert!(SafeDir::open_or_create(&d, 0o700, OwnerPolicy::Euid).is_err());
    }

    #[test]
    fn open_or_create_rejects_preplanted_symlink() {
        let tmp = TempDir::new().unwrap();
        let real = private_dir(&tmp, "target");
        let d = tmp.path().join("planted");
        std::os::unix::fs::symlink(&real, &d).unwrap();
        assert!(SafeDir::open_or_create(&d, 0o700, OwnerPolicy::Euid).is_err());
    }

    #[test]
    fn write_atomic_writes_content_and_mode() {
        let tmp = TempDir::new().unwrap();
        let d = private_dir(&tmp, "w");
        let sd = SafeDir::open(&d, OwnerPolicy::Euid).unwrap();
        sd.write_atomic("secret", b"hola", 0o600, None)
            .expect("write");
        let p = d.join("secret");
        assert_eq!(std::fs::read(&p).unwrap(), b"hola");
        assert_eq!(
            std::fs::metadata(&p).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn write_atomic_does_not_follow_symlink_at_final_name() {
        let tmp = TempDir::new().unwrap();
        let d = private_dir(&tmp, "w");
        let victim = tmp.path().join("victim");
        std::fs::write(&victim, b"original").unwrap();
        std::fs::set_permissions(&victim, std::fs::Permissions::from_mode(0o640)).unwrap();
        std::os::unix::fs::symlink(&victim, d.join("token")).unwrap();

        let sd = SafeDir::open(&d, OwnerPolicy::Euid).unwrap();
        sd.write_atomic("token", b"nuevo", 0o600, Some((euid(), egid())))
            .expect("write replaces the symlink itself");

        assert_eq!(std::fs::read(&victim).unwrap(), b"original");
        assert_eq!(
            std::fs::metadata(&victim).unwrap().permissions().mode() & 0o777,
            0o640
        );
        let md = std::fs::symlink_metadata(d.join("token")).unwrap();
        assert!(
            md.file_type().is_file(),
            "final name must be a regular file"
        );
        assert_eq!(std::fs::read(d.join("token")).unwrap(), b"nuevo");
    }

    #[test]
    fn write_atomic_rejects_bad_names() {
        let tmp = TempDir::new().unwrap();
        let d = private_dir(&tmp, "w");
        let sd = SafeDir::open(&d, OwnerPolicy::Euid).unwrap();
        for bad in ["", ".", "..", "a/b", "../x", "/abs", "nul\0byte"] {
            assert!(
                sd.write_atomic(bad, b"x", 0o600, None).is_err(),
                "name {:?} must be rejected",
                bad
            );
        }
    }

    #[test]
    fn write_atomic_applies_owner() {
        let tmp = TempDir::new().unwrap();
        let d = private_dir(&tmp, "w");
        let sd = SafeDir::open(&d, OwnerPolicy::Euid).unwrap();
        sd.write_atomic("owned", b"x", 0o600, Some((euid(), egid())))
            .expect("fchown to own uid/gid always allowed");
        let md = std::fs::metadata(d.join("owned")).unwrap();
        assert_eq!(md.uid(), euid());
        assert_eq!(md.gid(), egid());
    }

    #[test]
    fn write_atomic_leaves_no_temp_files() {
        let tmp = TempDir::new().unwrap();
        let d = private_dir(&tmp, "w");
        let sd = SafeDir::open(&d, OwnerPolicy::Euid).unwrap();
        sd.write_atomic("a", b"1", 0o600, None).unwrap();
        sd.write_atomic("a", b"2", 0o600, None).unwrap();
        let names: Vec<_> = std::fs::read_dir(&d)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(names, vec!["a".to_string()]);
        assert_eq!(std::fs::read(d.join("a")).unwrap(), b"2");
    }

    #[test]
    fn operations_stay_in_validated_dir_after_path_swap() {
        // Abrir A; después sustituir la ruta A por un symlink a B.
        // La escritura debe caer en el A original (renombrado), nunca en B.
        let tmp = TempDir::new().unwrap();
        let a = private_dir(&tmp, "a");
        let b = private_dir(&tmp, "b");
        let sd = SafeDir::open(&a, OwnerPolicy::Euid).unwrap();
        let moved = tmp.path().join("a_moved");
        std::fs::rename(&a, &moved).unwrap();
        std::os::unix::fs::symlink(&b, &a).unwrap();

        sd.write_atomic("f", b"x", 0o600, None).unwrap();

        assert!(
            moved.join("f").exists(),
            "write must land in the validated dir"
        );
        assert!(
            !b.join("f").exists(),
            "write must not follow the swapped path"
        );
    }

    #[test]
    fn open_file_nofollow_creates_regular_file_with_mode() {
        let tmp = TempDir::new().unwrap();
        let d = private_dir(&tmp, "l");
        let sd = SafeDir::open(&d, OwnerPolicy::Euid).unwrap();
        let _f = sd
            .open_file_nofollow("x.lock", 0o644)
            .expect("create lockfile");
        let md = std::fs::symlink_metadata(d.join("x.lock")).unwrap();
        assert!(md.file_type().is_file());
        assert_eq!(md.permissions().mode() & 0o777, 0o644);
    }

    #[test]
    fn open_file_nofollow_rejects_symlink() {
        let tmp = TempDir::new().unwrap();
        let d = private_dir(&tmp, "l");
        let victim = tmp.path().join("victim");
        std::fs::write(&victim, b"keep").unwrap();
        std::os::unix::fs::symlink(&victim, d.join("x.lock")).unwrap();
        let sd = SafeDir::open(&d, OwnerPolicy::Euid).unwrap();
        assert!(sd.open_file_nofollow("x.lock", 0o644).is_err());
        assert_eq!(std::fs::read(&victim).unwrap(), b"keep");
    }

    #[test]
    fn open_file_nofollow_rejects_directory() {
        let tmp = TempDir::new().unwrap();
        let d = private_dir(&tmp, "l");
        std::fs::create_dir(d.join("sub")).unwrap();
        let sd = SafeDir::open(&d, OwnerPolicy::Euid).unwrap();
        assert!(sd.open_file_nofollow("sub", 0o644).is_err());
    }

    #[test]
    fn read_to_string_nofollow_reads_missing_and_rejects_symlink() {
        let tmp = TempDir::new().unwrap();
        let d = private_dir(&tmp, "r");
        std::fs::write(d.join("cfg"), "k=v").unwrap();
        let victim = tmp.path().join("victim");
        std::fs::write(&victim, "secret").unwrap();
        std::os::unix::fs::symlink(&victim, d.join("link")).unwrap();
        let sd = SafeDir::open(&d, OwnerPolicy::Euid).unwrap();

        assert_eq!(
            sd.read_to_string_nofollow("cfg", 1024).unwrap().as_deref(),
            Some("k=v")
        );
        assert_eq!(sd.read_to_string_nofollow("missing", 1024).unwrap(), None);
        assert!(sd.read_to_string_nofollow("link", 1024).is_err());
    }

    #[test]
    fn read_to_string_nofollow_enforces_max_bytes() {
        let tmp = TempDir::new().unwrap();
        let d = private_dir(&tmp, "r");
        std::fs::write(d.join("big"), vec![b'a'; 64]).unwrap();
        let sd = SafeDir::open(&d, OwnerPolicy::Euid).unwrap();
        assert!(sd.read_to_string_nofollow("big", 16).is_err());
        assert!(sd.read_to_string_nofollow("big", 64).is_ok());
    }

    #[test]
    fn component_cstring_accepts_plain_names() {
        assert!(component_cstring("config.toml").is_ok());
        assert!(component_cstring(".hidden").is_ok());
    }

    #[test]
    fn open_or_create_owned_creates_with_owner_and_mode() {
        let tmp = TempDir::new().unwrap();
        let d = tmp.path().join(".ssh");
        let _sd = SafeDir::open_or_create_owned(
            &d,
            0o700,
            (euid(), egid()),
            OwnerPolicy::RootOrUid(euid()),
        )
        .expect("create owned");
        let md = std::fs::symlink_metadata(&d).unwrap();
        assert!(md.is_dir());
        assert_eq!(md.permissions().mode() & 0o777, 0o700);
        assert_eq!(md.uid(), euid());
    }

    #[test]
    fn open_or_create_owned_rejects_preplanted_symlink() {
        let tmp = TempDir::new().unwrap();
        let elsewhere = private_dir(&tmp, "elsewhere");
        let d = tmp.path().join(".ssh");
        std::os::unix::fs::symlink(&elsewhere, &d).unwrap();
        assert!(SafeDir::open_or_create_owned(
            &d,
            0o700,
            (euid(), egid()),
            OwnerPolicy::RootOrUid(euid())
        )
        .is_err());
    }

    #[test]
    fn open_user_config_dir_creates_private_dir() {
        let tmp = TempDir::new().unwrap();
        let d = tmp.path().join(".enola");
        let _sd = open_user_config_dir(&d, None).expect("create");
        let md = std::fs::symlink_metadata(&d).unwrap();
        assert!(md.is_dir());
        assert_eq!(md.permissions().mode() & 0o777, 0o700);
    }

    #[test]
    fn open_user_config_dir_clamps_open_dir_to_0700_and_reports_it() {
        let tmp = TempDir::new().unwrap();
        let d = private_dir(&tmp, ".enola");
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o775)).unwrap();
        let sd = open_user_config_dir(&d, None).expect("own dir must open");
        let mode = std::fs::metadata(&d).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700, "group/other permissions removed");
        assert_eq!(sd.mode_correction(), Some((0o775, 0o700)));
    }

    #[test]
    fn open_user_dir_never_adds_permissions() {
        let tmp = TempDir::new().unwrap();
        let d = private_dir(&tmp, ".ssh");
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o500)).unwrap();
        let sd = open_user_dir(&d, 0o700, (euid(), egid()), OwnerPolicy::RootOrUid(euid()))
            .expect("open");
        let mode = std::fs::metadata(&d).unwrap().permissions().mode() & 0o777;
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(mode, 0o500, "clamp must not add the missing write bit");
        assert_eq!(sd.mode_correction(), None);
    }

    #[test]
    fn open_user_dir_private_dir_reports_no_correction() {
        let tmp = TempDir::new().unwrap();
        let d = private_dir(&tmp, ".ssh");
        let sd = open_user_dir(&d, 0o700, (euid(), egid()), OwnerPolicy::RootOrUid(euid()))
            .expect("open");
        assert_eq!(sd.mode_correction(), None);
    }

    #[test]
    fn open_user_config_dir_rejects_symlink_and_foreign_owner() {
        let tmp = TempDir::new().unwrap();
        let real = private_dir(&tmp, "real");
        let link = tmp.path().join(".enola");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert!(open_user_config_dir(&link, None).is_err());
        if euid() != 0 {
            let other = (euid().wrapping_add(1), egid());
            assert!(open_user_config_dir(&real, Some(other)).is_err());
        }
    }

    #[test]
    fn secure_service_base_dir_creates_0700() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path().join("enola-svc");
        secure_service_base_dir(&base).expect("create");
        let md = std::fs::symlink_metadata(&base).unwrap();
        assert!(md.is_dir());
        assert_eq!(md.permissions().mode() & 0o777, 0o700);
    }

    #[test]
    fn secure_service_base_dir_fixes_existing_0755() {
        let tmp = TempDir::new().unwrap();
        let base = private_dir(&tmp, "enola-svc");
        std::fs::set_permissions(&base, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::create_dir(base.join("inst")).unwrap();
        secure_service_base_dir(&base).expect("fix");
        assert_eq!(
            std::fs::metadata(&base).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert!(base.join("inst").is_dir(), "contents untouched");
    }

    #[test]
    fn secure_service_base_dir_rejects_symlink() {
        let tmp = TempDir::new().unwrap();
        let real = private_dir(&tmp, "real");
        std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o755)).unwrap();
        let base = tmp.path().join("enola-svc");
        std::os::unix::fs::symlink(&real, &base).unwrap();
        assert!(secure_service_base_dir(&base).is_err());
        assert_eq!(
            std::fs::metadata(&real).unwrap().permissions().mode() & 0o777,
            0o755,
            "symlink target must not be chmod-ed"
        );
    }
}
