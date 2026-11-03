//! PLAN-BACKUP-RESTORE-ROLLBACK (Fase 1) — Tipos puros del backup «de sistema»:
//! qué rutas forman la **identidad** de una máquina Enola, el manifiesto que
//! viaja dentro del archivo y la regla de espacio en disco.
//!
//! Identidad = lo que hace falta para reconstruir los servicios `.onion` (claves
//! de Tor, fragmentos torrc, Nginx, certificados, configuración de Enola). Los
//! datos de cada servicio (`/srv/enola-*`) quedan fuera por decisión del operador
//! (D2). Dominio puro: sin I/O.

use crate::domain::error::{EnolaError, Result};
use serde::{Deserialize, Serialize};

/// Versión del formato del archivo de backup. `restore` rechaza cualquier otra
/// (alpha sin retrocompatibilidad).
pub const BACKUP_FORMAT_VERSION: u32 = 1;

/// Dónde se escribe el manifiesto antes de archivar (dentro de `/etc/enola`, que
/// forma parte de la identidad: así viaja en el propio archivo).
pub const BACKUP_MANIFEST_PATH: &str = "/etc/enola/backup-manifest.json";

/// Rutas fijas de identidad. Solo root las escribe, salvo `/var/lib/tor`
/// (`debian-tor`). Se excluyen a propósito `~/.enola` (con `sudo -E` sería el home
/// del invocador, SEC-A5) y `/var/lib/enola/updates` (restaurar claves minisign
/// antiguas desharía una rotación de clave).
pub const SYSTEM_IDENTITY_PATHS: &[&str] = &[
    "/var/lib/tor",
    "/etc/tor/enola.d",
    "/etc/nginx/sites-available",
    "/etc/nginx/sites-enabled",
    "/etc/nginx/ssl",
    "/etc/enola",
    "/opt/enola",
    "/usr/local/share/enola/manifest",
];

/// Directorio y prefijo de los perfiles AppArmor de Enola (`enola-*`), que se
/// añaden uno a uno (no se archiva todo `/etc/apparmor.d`).
pub const APPARMOR_DIR: &str = "/etc/apparmor.d";
/// Prefijo de los perfiles AppArmor propios.
pub const APPARMOR_PREFIX: &str = "enola-";

/// Reserva fija de espacio libre sobre el tamaño de las fuentes (copias
/// `pre-restore`, rotación, metadatos del sistema de ficheros).
pub const DISK_RESERVE_BYTES: u64 = 100 * 1024 * 1024;

/// Espacio libre mínimo para crear un backup: tamaño de las fuentes (cota
/// superior del `.tar.gz`) más la reserva fija.
pub fn required_free_bytes(source_bytes: u64) -> u64 {
    source_bytes.saturating_add(DISK_RESERVE_BYTES)
}

/// Manifiesto del archivo de backup (`etc/enola/backup-manifest.json` dentro del tar).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupManifest {
    /// Versión del formato (`BACKUP_FORMAT_VERSION`).
    pub format_version: u32,
    /// Versión de `enola-cli` que lo creó.
    pub enola_version: String,
    /// Fecha de creación (`YYYY-MM-DD_HH-MM-SS`, hora local).
    pub created_at: String,
    /// Rutas absolutas archivadas.
    pub paths: Vec<String>,
}

impl BackupManifest {
    /// Manifiesto nuevo con el formato vigente.
    pub fn new(enola_version: &str, created_at: &str, paths: Vec<String>) -> Self {
        Self {
            format_version: BACKUP_FORMAT_VERSION,
            enola_version: enola_version.to_string(),
            created_at: created_at.to_string(),
            paths,
        }
    }

    /// Serializa a JSON legible.
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string_pretty(self)
            .map_err(|e| EnolaError::ValidationError(format!("backup manifest: {e}")))
    }

    /// Lee un manifiesto y **rechaza** otra versión de formato.
    pub fn parse(json: &str) -> Result<Self> {
        let m: Self = serde_json::from_str(json)
            .map_err(|e| EnolaError::ValidationError(format!("invalid backup manifest: {e}")))?;
        if m.format_version != BACKUP_FORMAT_VERSION {
            return Err(EnolaError::ValidationError(format!(
                "unsupported backup format {} (this enola-cli reads format {})",
                m.format_version, BACKUP_FORMAT_VERSION
            )));
        }
        Ok(m)
    }
}

// ── Restore (PLAN-BACKUP-RESTORE, Fase 2) ────────────────────────────────────
//
// El restore corre como root y extrae sobre `/`: un archivo manipulado es una
// escalada de privilegios. Por eso se valida TODO el listado antes de escribir
// nada y se rechaza el archivo entero ante la primera entrada sospechosa.

/// Tipo de una entrada del archivo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    /// Fichero regular.
    File,
    /// Directorio.
    Dir,
    /// Enlace simbólico (solo se admiten los que apuntan dentro de la lista blanca).
    Symlink,
    /// Enlace duro (siempre rechazado).
    Hardlink,
    /// Dispositivo, FIFO, etc. (siempre rechazado).
    Other,
}

/// Entrada de un archivo de backup, tal como la lee el adaptador.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveEntry {
    /// Ruta relativa a `/` tal como está en el tar (sin `/` inicial).
    pub path: String,
    /// Tipo.
    pub kind: EntryKind,
    /// Destino del enlace simbólico, si lo es.
    pub link_target: Option<String>,
    /// Propietario por nombre (`root`, `debian-tor`…), si el tar lo trae.
    pub owner: Option<String>,
    /// Modo Unix tal como viene (se recorta con [`safe_mode`]).
    pub mode: u32,
}

/// Propietarios que el restore acepta reponer por nombre. Cualquier otro se
/// restaura como `root` (un archivo ajeno no puede regalar ficheros a un usuario).
pub const RESTORE_ALLOWED_OWNERS: &[&str] = &["root", "debian-tor"];

/// Modo seguro: se descartan setuid/setgid/sticky y bits desconocidos.
pub fn safe_mode(mode: u32) -> u32 {
    mode & 0o777
}

/// Normaliza una ruta relativa del tar. `None` si es absoluta, vacía, tiene `.`,
/// `..`, segmentos vacíos (salvo el `/` final de un directorio) o bytes NUL.
pub fn normalize_member(path: &str) -> Option<String> {
    if path.is_empty() || path.starts_with('/') || path.contains('\0') {
        return None;
    }
    let trimmed = path.strip_suffix('/').unwrap_or(path);
    if trimmed.is_empty() {
        return None;
    }
    let mut out = Vec::new();
    for seg in trimmed.split('/') {
        if seg.is_empty() || seg == "." || seg == ".." {
            return None;
        }
        out.push(seg);
    }
    Some(out.join("/"))
}

/// `true` si la ruta relativa (ya normalizada) está dentro de la lista blanca de
/// identidad: una ruta fija (o algo bajo ella) o un perfil `etc/apparmor.d/enola-*`.
pub fn is_allowed_member(rel: &str) -> bool {
    let fixed = SYSTEM_IDENTITY_PATHS.iter().any(|p| {
        let p = p.trim_start_matches('/');
        rel == p || rel.strip_prefix(p).is_some_and(|r| r.starts_with('/'))
    });
    if fixed {
        return true;
    }
    let ap = format!(
        "{}/{}",
        APPARMOR_DIR.trim_start_matches('/'),
        APPARMOR_PREFIX
    );
    rel.strip_prefix(&ap)
        .is_some_and(|rest| !rest.contains('/'))
}

/// Resuelve léxicamente (sin tocar el disco) el destino de un enlace simbólico a
/// una ruta relativa a `/`. `None` si escapa por encima de `/`.
fn resolve_link(link_path: &str, target: &str) -> Option<String> {
    let mut stack: Vec<&str> = Vec::new();
    if !target.starts_with('/') {
        let parent = link_path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        stack.extend(parent.split('/').filter(|s| !s.is_empty()));
    }
    for seg in target.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                stack.pop()?;
            }
            s => stack.push(s),
        }
    }
    if stack.is_empty() {
        return None;
    }
    Some(stack.join("/"))
}

/// Entrada ya validada, lista para extraer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreItem {
    /// Ruta normalizada, relativa a `/`.
    pub path: String,
    /// Tipo.
    pub kind: EntryKind,
    /// Modo recortado con [`safe_mode`].
    pub mode: u32,
    /// Propietario a reponer (solo de [`RESTORE_ALLOWED_OWNERS`]); `root` si no.
    pub owner: String,
}

/// Valida el listado completo de un archivo de backup. Rechaza el archivo ENTERO
/// (sin escribir nada) si alguna entrada: tiene ruta insegura o fuera de la lista
/// blanca, es un enlace duro o un fichero especial, es un enlace simbólico que
/// apunta fuera de la lista blanca, o cuelga de un enlace simbólico del propio
/// archivo (escritura "a través" del enlace).
pub fn validate_restore_entries(entries: &[ArchiveEntry]) -> Result<Vec<RestoreItem>> {
    let bad = |what: &str, path: &str| {
        EnolaError::ValidationError(format!(
            "unsafe backup archive: {what} '{path}' — refusing to restore anything"
        ))
    };
    if entries.is_empty() {
        return Err(EnolaError::ValidationError(
            "backup archive is empty".to_string(),
        ));
    }
    let mut items = Vec::with_capacity(entries.len());
    let mut symlinks: Vec<String> = Vec::new();
    for e in entries {
        let rel = normalize_member(&e.path).ok_or_else(|| bad("invalid path", &e.path))?;
        if !is_allowed_member(&rel) {
            return Err(bad("path outside the allowed list", &e.path));
        }
        match e.kind {
            EntryKind::File | EntryKind::Dir => {}
            EntryKind::Hardlink => return Err(bad("hard link", &e.path)),
            EntryKind::Other => return Err(bad("special file", &e.path)),
            EntryKind::Symlink => {
                let target = e
                    .link_target
                    .as_deref()
                    .ok_or_else(|| bad("symlink without target", &e.path))?;
                let ok = target.split('\0').count() == 1
                    && resolve_link(&rel, target).is_some_and(|t| is_allowed_member(&t));
                if !ok {
                    return Err(bad("symlink pointing outside the allowed list", &e.path));
                }
                symlinks.push(rel.clone());
            }
        }
        let owner = match e.owner.as_deref() {
            Some(o) if RESTORE_ALLOWED_OWNERS.contains(&o) => o.to_string(),
            _ => "root".to_string(),
        };
        items.push(RestoreItem {
            path: rel,
            kind: e.kind,
            mode: safe_mode(e.mode),
            owner,
        });
    }
    for it in &items {
        for s in &symlinks {
            if it.path.len() > s.len()
                && it.path.starts_with(s.as_str())
                && it.path.as_bytes()[s.len()] == b'/'
            {
                return Err(bad(
                    "entry written through a symlink of the archive",
                    &it.path,
                ));
            }
        }
    }
    Ok(items)
}

/// Texto de "qué reiniciar" que el restore imprime al terminar (no lo ejecuta, D3).
pub fn restart_hints(items: &[RestoreItem]) -> Vec<String> {
    let touches = |prefix: &str| items.iter().any(|i| i.path.starts_with(prefix));
    let mut hints = Vec::new();
    if touches("var/lib/tor") || touches("etc/tor") {
        hints.push("sudo systemctl restart tor".to_string());
    }
    if touches("etc/nginx") {
        hints.push("sudo nginx -t && sudo systemctl reload nginx".to_string());
    }
    if touches("etc/apparmor.d") {
        hints.push("sudo systemctl reload apparmor".to_string());
    }
    hints.push(
        "enola-cli <servicio> restart <nombre>   # cada contenedor que uses (wp, git, ghost…)"
            .to_string(),
    );
    hints
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_paths_are_absolute_normalized_and_exclude_service_data() {
        for p in SYSTEM_IDENTITY_PATHS {
            assert!(p.starts_with('/'), "{p}");
            assert!(!p.split('/').any(|s| s == ".." || s == "."), "{p}");
            assert!(
                !p.starts_with("/srv/enola-"),
                "datos de servicio fuera (D2): {p}"
            );
            assert!(!p.contains(".enola"), "~/.enola excluido: {p}");
            assert!(
                !p.starts_with("/var/lib/enola/updates"),
                "claves de confianza fuera: {p}"
            );
        }
        for must in [
            "/var/lib/tor",
            "/etc/tor/enola.d",
            "/etc/nginx/ssl",
            "/etc/nginx/sites-enabled",
            "/etc/enola",
        ] {
            assert!(SYSTEM_IDENTITY_PATHS.contains(&must), "falta {must}");
        }
    }

    #[test]
    fn manifest_lives_inside_an_archived_path() {
        assert!(SYSTEM_IDENTITY_PATHS
            .iter()
            .any(|p| BACKUP_MANIFEST_PATH.starts_with(&format!("{p}/"))));
    }

    #[test]
    fn manifest_roundtrips_and_rejects_other_format_versions() {
        let m = BackupManifest::new("0.12.0", "2026-11-03_20-00-00", vec!["/etc/enola".into()]);
        let json = m.to_json().unwrap();
        assert_eq!(BackupManifest::parse(&json).unwrap(), m);

        let other = json.replace("\"format_version\": 1", "\"format_version\": 99");
        assert!(BackupManifest::parse(&other).is_err());
        assert!(BackupManifest::parse("no es json").is_err());
        assert!(BackupManifest::parse("{}").is_err());
    }

    #[test]
    fn required_free_bytes_adds_reserve_and_never_overflows() {
        assert_eq!(required_free_bytes(0), DISK_RESERVE_BYTES);
        assert_eq!(required_free_bytes(1000), 1000 + DISK_RESERVE_BYTES);
        assert_eq!(required_free_bytes(u64::MAX), u64::MAX);
    }

    fn entry(path: &str, kind: EntryKind) -> ArchiveEntry {
        ArchiveEntry {
            path: path.to_string(),
            kind,
            link_target: None,
            owner: None,
            mode: 0o644,
        }
    }
    fn link(path: &str, target: &str) -> ArchiveEntry {
        ArchiveEntry {
            link_target: Some(target.to_string()),
            ..entry(path, EntryKind::Symlink)
        }
    }

    #[test]
    fn normalize_member_rejects_unsafe_paths() {
        for bad in [
            "",
            "/etc/enola",
            "etc/../shadow",
            "etc//enola",
            "./etc",
            "etc/./x",
            "..",
            "a\0b",
            "/",
        ] {
            assert!(normalize_member(bad).is_none(), "{bad:?}");
        }
        assert_eq!(normalize_member("etc/enola/").as_deref(), Some("etc/enola"));
        assert_eq!(
            normalize_member("var/lib/tor/a/hostname").as_deref(),
            Some("var/lib/tor/a/hostname")
        );
    }

    #[test]
    fn allowed_members_follow_the_identity_list() {
        for ok in [
            "var/lib/tor",
            "var/lib/tor/svc/hs_ed25519_secret_key",
            "etc/enola/backup-manifest.json",
            "etc/nginx/ssl/a.key",
            "etc/apparmor.d/enola-nginx",
        ] {
            assert!(is_allowed_member(ok), "{ok}");
        }
        for no in [
            "etc/shadow",
            "etc",
            "var/lib/torx",
            "etc/enolax",
            "srv/enola-wordpress/x",
            "root/.ssh/authorized_keys",
            "etc/apparmor.d/usr.bin.x",
            "etc/apparmor.d/enola-a/b",
            "var/lib/enola/updates/trusted_minisign_keys.json",
            "usr/local/bin/enola-cli",
        ] {
            assert!(!is_allowed_member(no), "{no}");
        }
    }

    #[test]
    fn a_normal_identity_archive_is_accepted_and_modes_are_stripped() {
        let mut key = entry("var/lib/tor/svc/hs_ed25519_secret_key", EntryKind::File);
        key.owner = Some("debian-tor".into());
        key.mode = 0o4600; // setuid: se descarta
        let mut evil_owner = entry("etc/enola/x", EntryKind::File);
        evil_owner.owner = Some("mallory".into());
        let items = validate_restore_entries(&[
            entry("etc/enola/", EntryKind::Dir),
            key,
            evil_owner,
            // sites-enabled -> ../sites-available/x : legítimo
            link("etc/nginx/sites-enabled/x", "../sites-available/x"),
        ])
        .unwrap();
        assert_eq!(items[1].owner, "debian-tor");
        assert_eq!(items[1].mode, 0o600);
        assert_eq!(items[2].owner, "root", "propietario ajeno → root");
    }

    #[test]
    fn dangerous_archives_are_rejected_entirely() {
        let cases: Vec<(&str, Vec<ArchiveEntry>)> = vec![
            (
                "traversal",
                vec![entry("etc/enola/../../shadow", EntryKind::File)],
            ),
            ("absolute", vec![entry("/etc/enola/x", EntryKind::File)]),
            ("outside list", vec![entry("etc/shadow", EntryKind::File)]),
            ("hardlink", vec![entry("etc/enola/h", EntryKind::Hardlink)]),
            ("device", vec![entry("etc/enola/dev", EntryKind::Other)]),
            ("symlink out", vec![link("etc/enola/l", "/etc/shadow")]),
            (
                "symlink up",
                vec![link("etc/enola/l", "../../../etc/shadow")],
            ),
            (
                "symlink above root",
                vec![link("etc/enola/l", "../../../../../../../etc/enola")],
            ),
            (
                "symlink no target",
                vec![entry("etc/enola/l", EntryKind::Symlink)],
            ),
            (
                "write through symlink",
                vec![
                    link("etc/enola/l", "../nginx/ssl"),
                    entry("etc/enola/l/evil", EntryKind::File),
                ],
            ),
            ("empty", vec![]),
        ];
        for (name, entries) in cases {
            assert!(
                validate_restore_entries(&entries).is_err(),
                "debe rechazar: {name}"
            );
        }
        // Una sola entrada mala entre muchas buenas rechaza TODO el archivo.
        let r = validate_restore_entries(&[
            entry("etc/enola/ok", EntryKind::File),
            entry("var/lib/tor/ok", EntryKind::File),
            entry("etc/cron.d/evil", EntryKind::File),
        ]);
        assert!(r.is_err());
    }

    #[test]
    fn restart_hints_depend_on_what_was_restored() {
        let items = validate_restore_entries(&[
            entry("var/lib/tor/a", EntryKind::File),
            entry("etc/nginx/ssl/a", EntryKind::File),
        ])
        .unwrap();
        let h = restart_hints(&items).join("\n");
        assert!(h.contains("restart tor") && h.contains("nginx -t"));
        assert!(!h.contains("apparmor"));
    }
}
