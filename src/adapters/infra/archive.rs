//! PLAN-BACKUP-RESTORE (Fase 2) — Adapter de `ArchiveReaderPort` con `tar` + `flate2`
//! en proceso (sin lanzar `tar`): lee tipos de entrada reales, y extrae con
//! comprobaciones propias además de las de la crate.

use crate::domain::backup::{normalize_member, ArchiveEntry, EntryKind, RestoreItem};
use crate::domain::error::{EnolaError, Result};
use crate::ports::archive::ArchiveReaderPort;
use flate2::read::GzDecoder;
use std::collections::HashMap;
use std::io::Read;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// Tamaño máximo de un miembro de texto leído en memoria (manifiesto).
const MAX_TEXT_MEMBER: u64 = 1024 * 1024;

fn io_err(what: &str, e: impl std::fmt::Display) -> EnolaError {
    EnolaError::InfrastructureError(format!("{what}: {e}"))
}

/// Implementación `.tar.gz`.
#[derive(Debug, Clone, Copy)]
pub struct TarGzArchive {
    /// `true` solo como root: reponer propietarios (`chown`). Sin root no se puede.
    apply_ownership: bool,
}

impl TarGzArchive {
    /// `apply_ownership` = el proceso es root.
    pub fn new(apply_ownership: bool) -> Self {
        Self { apply_ownership }
    }
}

fn open(archive: &Path) -> Result<tar::Archive<GzDecoder<std::fs::File>>> {
    let f = std::fs::File::open(archive)
        .map_err(|e| io_err(&format!("Cannot open {}", archive.display()), e))?;
    Ok(tar::Archive::new(GzDecoder::new(f)))
}

fn kind_of(t: tar::EntryType) -> EntryKind {
    match t {
        tar::EntryType::Regular | tar::EntryType::Continuous => EntryKind::File,
        tar::EntryType::Directory => EntryKind::Dir,
        tar::EntryType::Symlink => EntryKind::Symlink,
        tar::EntryType::Link => EntryKind::Hardlink,
        _ => EntryKind::Other,
    }
}

fn path_string(p: std::borrow::Cow<'_, Path>) -> Result<String> {
    p.to_str()
        .map(str::to_string)
        .ok_or_else(|| EnolaError::ValidationError("archive path is not valid UTF-8".to_string()))
}

/// uid/gid de un usuario del sistema (`id -u/-g`); `root` es siempre 0.
fn lookup_owner(name: &str) -> Result<(u32, u32)> {
    if name == "root" {
        return Ok((0, 0));
    }
    let q = |flag: &str| -> Result<u32> {
        let out = std::process::Command::new("id")
            .args([flag, name])
            .output()
            .map_err(|e| io_err("id", e))?;
        String::from_utf8_lossy(&out.stdout)
            .trim()
            .parse::<u32>()
            .map_err(|_| {
                EnolaError::ValidationError(format!(
                    "system user '{name}' does not exist: install the service that owns it (e.g. Tor) before restoring"
                ))
            })
    };
    Ok((q("-u")?, q("-g")?))
}

/// Comprueba que ningún componente EXISTENTE de `dest` (bajo `root`) es un enlace
/// simbólico, y crea los directorios que faltan (0755).
fn prepare_parents(root: &Path, rel: &str) -> Result<PathBuf> {
    let mut cur = root.to_path_buf();
    let comps: Vec<&str> = rel.split('/').collect();
    for seg in &comps[..comps.len() - 1] {
        cur.push(seg);
        match std::fs::symlink_metadata(&cur) {
            Ok(md) if md.file_type().is_symlink() => {
                return Err(EnolaError::ValidationError(format!(
                    "refusing to restore through the symlink {}",
                    cur.display()
                )))
            }
            Ok(md) if md.is_dir() => {}
            Ok(_) => {
                return Err(EnolaError::ValidationError(format!(
                    "{} exists and is not a directory",
                    cur.display()
                )))
            }
            Err(_) => {
                std::fs::create_dir(&cur).map_err(|e| io_err(&cur.display().to_string(), e))?;
                std::fs::set_permissions(&cur, std::fs::Permissions::from_mode(0o755))
                    .map_err(|e| io_err(&cur.display().to_string(), e))?;
            }
        }
    }
    Ok(cur.join(comps[comps.len() - 1]))
}

impl ArchiveReaderPort for TarGzArchive {
    fn list(&self, archive: &Path) -> Result<Vec<ArchiveEntry>> {
        let mut ar = open(archive)?;
        let mut out = Vec::new();
        for entry in ar.entries().map_err(|e| io_err("read archive", e))? {
            let entry = entry.map_err(|e| io_err("read archive entry", e))?;
            let path = path_string(entry.path().map_err(|e| io_err("entry path", e))?)?;
            let link_target = match entry.link_name() {
                Ok(Some(l)) => Some(path_string(l)?),
                _ => None,
            };
            let h = entry.header();
            out.push(ArchiveEntry {
                path,
                kind: kind_of(h.entry_type()),
                link_target,
                owner: h.username().ok().flatten().map(str::to_string),
                mode: h.mode().unwrap_or(0o600),
            });
        }
        Ok(out)
    }

    fn read_text(&self, archive: &Path, member: &str) -> Result<Option<String>> {
        let mut ar = open(archive)?;
        for entry in ar.entries().map_err(|e| io_err("read archive", e))? {
            let entry = entry.map_err(|e| io_err("read archive entry", e))?;
            let path = path_string(entry.path().map_err(|e| io_err("entry path", e))?)?;
            if path == member {
                if entry.header().entry_type() != tar::EntryType::Regular {
                    return Ok(None);
                }
                let mut s = String::new();
                entry
                    .take(MAX_TEXT_MEMBER)
                    .read_to_string(&mut s)
                    .map_err(|e| io_err("read manifest", e))?;
                return Ok(Some(s));
            }
        }
        Ok(None)
    }

    fn extract(&self, archive: &Path, root: &Path, items: &[RestoreItem]) -> Result<()> {
        let wanted: HashMap<&str, &RestoreItem> =
            items.iter().map(|i| (i.path.as_str(), i)).collect();

        // Propietarios resueltos ANTES de escribir nada.
        let mut owners: HashMap<&str, (u32, u32)> = HashMap::new();
        if self.apply_ownership {
            for it in items {
                if !owners.contains_key(it.owner.as_str()) {
                    owners.insert(it.owner.as_str(), lookup_owner(&it.owner)?);
                }
            }
        }

        let mut ar = open(archive)?;
        for entry in ar.entries().map_err(|e| io_err("read archive", e))? {
            let mut entry = entry.map_err(|e| io_err("read archive entry", e))?;
            let raw = path_string(entry.path().map_err(|e| io_err("entry path", e))?)?;
            let rel = normalize_member(&raw).ok_or_else(|| {
                EnolaError::ValidationError(format!("unsafe path in archive: {raw}"))
            })?;
            let item = wanted.get(rel.as_str()).ok_or_else(|| {
                EnolaError::ValidationError(format!("entry not validated: {rel}"))
            })?;
            let dest = prepare_parents(root, &rel)?;
            let chown = |p: &Path, symlink: bool| -> Result<()> {
                if let Some((u, g)) = owners.get(item.owner.as_str()) {
                    let r = if symlink {
                        std::os::unix::fs::lchown(p, Some(*u), Some(*g))
                    } else {
                        std::os::unix::fs::chown(p, Some(*u), Some(*g))
                    };
                    r.map_err(|e| io_err(&format!("chown {}", p.display()), e))?;
                }
                Ok(())
            };
            match item.kind {
                EntryKind::Dir => {
                    match std::fs::symlink_metadata(&dest) {
                        Ok(md) if md.is_dir() => {}
                        Ok(_) => {
                            return Err(EnolaError::ValidationError(format!(
                                "{} exists and is not a directory",
                                dest.display()
                            )))
                        }
                        Err(_) => std::fs::create_dir(&dest)
                            .map_err(|e| io_err(&dest.display().to_string(), e))?,
                    }
                    std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(item.mode))
                        .map_err(|e| io_err(&dest.display().to_string(), e))?;
                    chown(&dest, false)?;
                }
                EntryKind::File => {
                    if let Ok(md) = std::fs::symlink_metadata(&dest) {
                        if md.is_dir() {
                            return Err(EnolaError::ValidationError(format!(
                                "{} is a directory",
                                dest.display()
                            )));
                        }
                    }
                    // Temporal exclusivo en el mismo directorio + rename atómico.
                    let tmp = dest.with_file_name(format!(
                        ".enola-restore-{}.tmp",
                        dest.file_name().and_then(|n| n.to_str()).unwrap_or("f")
                    ));
                    let _ = std::fs::remove_file(&tmp);
                    let mut f = std::fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .mode(0o600)
                        .open(&tmp)
                        .map_err(|e| io_err(&tmp.display().to_string(), e))?;
                    std::io::copy(&mut entry, &mut f)
                        .map_err(|e| io_err(&tmp.display().to_string(), e))?;
                    f.sync_all().map_err(|e| io_err("sync", e))?;
                    drop(f);
                    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(item.mode))
                        .map_err(|e| io_err(&tmp.display().to_string(), e))?;
                    chown(&tmp, false)?;
                    std::fs::rename(&tmp, &dest)
                        .map_err(|e| io_err(&dest.display().to_string(), e))?;
                }
                EntryKind::Symlink => {
                    let target = entry
                        .link_name()
                        .map_err(|e| io_err("link", e))?
                        .ok_or_else(|| {
                            EnolaError::ValidationError(format!("symlink without target: {rel}"))
                        })?
                        .into_owned();
                    if let Ok(md) = std::fs::symlink_metadata(&dest) {
                        if md.is_dir() {
                            return Err(EnolaError::ValidationError(format!(
                                "{} is a directory",
                                dest.display()
                            )));
                        }
                        std::fs::remove_file(&dest)
                            .map_err(|e| io_err(&dest.display().to_string(), e))?;
                    }
                    std::os::unix::fs::symlink(&target, &dest)
                        .map_err(|e| io_err(&dest.display().to_string(), e))?;
                    chown(&dest, true)?;
                }
                // `validate_restore_entries` ya los rechazó; defensa en profundidad.
                EntryKind::Hardlink | EntryKind::Other => {
                    return Err(EnolaError::ValidationError(format!(
                        "refusing special entry: {rel}"
                    )))
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::backup::validate_restore_entries;

    fn build(
        path: &Path,
        f: impl FnOnce(&mut tar::Builder<flate2::write::GzEncoder<std::fs::File>>),
    ) {
        let gz = flate2::write::GzEncoder::new(
            std::fs::File::create(path).unwrap(),
            flate2::Compression::fast(),
        );
        let mut b = tar::Builder::new(gz);
        f(&mut b);
        b.into_inner().unwrap().finish().unwrap();
    }

    fn file(
        b: &mut tar::Builder<flate2::write::GzEncoder<std::fs::File>>,
        name: &str,
        data: &[u8],
        mode: u32,
    ) {
        let mut h = tar::Header::new_gnu();
        h.set_size(data.len() as u64);
        h.set_mode(mode);
        h.set_entry_type(tar::EntryType::Regular);
        b.append_data(&mut h, name, data).unwrap();
    }

    fn sym(
        b: &mut tar::Builder<flate2::write::GzEncoder<std::fs::File>>,
        name: &str,
        target: &str,
    ) {
        let mut h = tar::Header::new_gnu();
        h.set_size(0);
        h.set_mode(0o777);
        h.set_entry_type(tar::EntryType::Symlink);
        b.append_link(&mut h, name, target).unwrap();
    }

    #[test]
    fn list_reports_real_entry_types_and_links() {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("b.tar.gz");
        build(&a, |b| {
            file(b, "etc/enola/x", b"hi", 0o644);
            sym(b, "etc/nginx/sites-enabled/s", "../sites-available/s");
        });
        let list = TarGzArchive::new(false).list(&a).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].kind, EntryKind::File);
        assert_eq!(list[1].kind, EntryKind::Symlink);
        assert_eq!(list[1].link_target.as_deref(), Some("../sites-available/s"));
    }

    #[test]
    fn read_text_returns_member_or_none() {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("b.tar.gz");
        build(&a, |b| {
            file(b, "etc/enola/backup-manifest.json", b"{}", 0o600)
        });
        let ar = TarGzArchive::new(false);
        assert_eq!(
            ar.read_text(&a, "etc/enola/backup-manifest.json")
                .unwrap()
                .as_deref(),
            Some("{}")
        );
        assert_eq!(ar.read_text(&a, "etc/enola/otro").unwrap(), None);
    }

    #[test]
    fn extract_restores_files_modes_dirs_and_symlinks_atomically() {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("b.tar.gz");
        build(&a, |b| {
            file(b, "etc/enola/conf", b"new", 0o4640);
            file(b, "var/lib/tor/svc/key", b"KEY", 0o600);
            sym(b, "etc/nginx/sites-enabled/s", "../sites-available/s");
        });
        let ar = TarGzArchive::new(false);
        let items = validate_restore_entries(&ar.list(&a).unwrap()).unwrap();
        let root = tmp.path().join("root");
        std::fs::create_dir_all(root.join("etc/enola")).unwrap();
        std::fs::write(root.join("etc/enola/conf"), "old").unwrap();

        ar.extract(&a, &root, &items).unwrap();

        assert_eq!(
            std::fs::read_to_string(root.join("etc/enola/conf")).unwrap(),
            "new"
        );
        let mode = std::fs::metadata(root.join("etc/enola/conf"))
            .unwrap()
            .permissions()
            .mode()
            & 0o7777;
        assert_eq!(mode, 0o640, "setuid descartado");
        assert_eq!(
            std::fs::read(root.join("var/lib/tor/svc/key")).unwrap(),
            b"KEY"
        );
        assert_eq!(
            std::fs::read_link(root.join("etc/nginx/sites-enabled/s")).unwrap(),
            Path::new("../sites-available/s")
        );
        // Sin temporales residuales.
        let leftovers: Vec<_> = walk(&root)
            .into_iter()
            .filter(|p| p.to_string_lossy().contains(".enola-restore-"))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
    }

    fn walk(p: &Path) -> Vec<PathBuf> {
        let mut v = vec![];
        if let Ok(rd) = std::fs::read_dir(p) {
            for e in rd.flatten() {
                v.push(e.path());
                if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    v.extend(walk(&e.path()));
                }
            }
        }
        v
    }

    /// Defensa en profundidad: aunque el listado fuese válido, un enlace simbólico
    /// que YA existe en el disco no se atraviesa.
    #[test]
    fn extract_refuses_to_write_through_an_existing_symlink() {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("b.tar.gz");
        build(&a, |b| file(b, "etc/enola/conf", b"pwn", 0o644));
        let ar = TarGzArchive::new(false);
        let items = validate_restore_entries(&ar.list(&a).unwrap()).unwrap();

        let root = tmp.path().join("root");
        let outside = tmp.path().join("outside");
        std::fs::create_dir_all(root.join("etc")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("etc/enola")).unwrap();

        let r = ar.extract(&a, &root, &items);
        assert!(r.is_err());
        assert!(!outside.join("conf").exists(), "no debe escribir fuera");
    }

    #[test]
    fn extract_rejects_entries_that_were_not_validated() {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("b.tar.gz");
        build(&a, |b| {
            file(b, "etc/enola/ok", b"1", 0o644);
            file(b, "etc/enola/sneaky", b"2", 0o644);
        });
        let ar = TarGzArchive::new(false);
        let mut items = validate_restore_entries(&ar.list(&a).unwrap()).unwrap();
        items.retain(|i| i.path.ends_with("/ok"));
        let root = tmp.path().join("root");
        assert!(ar.extract(&a, &root, &items).is_err());
    }

    #[test]
    fn dangerous_archives_fail_validation_end_to_end() {
        let tmp = tempfile::tempdir().unwrap();
        let ar = TarGzArchive::new(false);

        // Enlace simbólico hacia fuera + escritura a través de él.
        let a = tmp.path().join("evil1.tar.gz");
        build(&a, |b| {
            sym(b, "etc/enola/l", "/etc");
            file(b, "etc/enola/l/shadow", b"x", 0o644);
        });
        assert!(validate_restore_entries(&ar.list(&a).unwrap()).is_err());

        // Enlace duro.
        let a = tmp.path().join("evil2.tar.gz");
        build(&a, |b| {
            let mut h = tar::Header::new_gnu();
            h.set_size(0);
            h.set_entry_type(tar::EntryType::Link);
            b.append_link(&mut h, "etc/enola/h", "etc/shadow").unwrap();
        });
        assert!(validate_restore_entries(&ar.list(&a).unwrap()).is_err());

        // Fuera de la lista blanca.
        let a = tmp.path().join("evil3.tar.gz");
        build(&a, |b| file(b, "etc/cron.d/x", b"* * * * * root id", 0o644));
        assert!(validate_restore_entries(&ar.list(&a).unwrap()).is_err());
    }
}
