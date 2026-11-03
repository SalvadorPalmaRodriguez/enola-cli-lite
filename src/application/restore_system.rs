//! PLAN-BACKUP-RESTORE (Fase 2) — Restauración de la identidad del sistema desde un
//! backup (`maintenance restore`).
//!
//! Reglas (D3, D5): solo restaura **ficheros** y dice qué reiniciar (no toca
//! `systemctl`/`docker`); sin `--force` solo muestra lo que haría; antes de
//! sobrescribir guarda una copia `pre-restore` de lo existente; valida TODO el
//! listado del archivo antes de escribir nada (ver `domain::backup`).

use crate::application::backup_system::BackupSystem;
use crate::domain::backup::{
    is_allowed_member, restart_hints, validate_restore_entries, BackupManifest, EntryKind,
    RestoreItem, APPARMOR_DIR, BACKUP_MANIFEST_PATH, SYSTEM_IDENTITY_PATHS,
};
use crate::domain::error::{EnolaError, Result};
use crate::ports::archive::ArchiveReaderPort;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Resultado de un restore (o de su simulación).
#[derive(Debug, Clone)]
pub struct RestoreReport {
    /// Manifiesto leído del archivo.
    pub manifest: BackupManifest,
    /// Entradas validadas.
    pub items: Vec<RestoreItem>,
    /// Rutas (absolutas) que ya existen y se sobrescribirían.
    pub overwrites: Vec<PathBuf>,
    /// Copia `pre-restore` creada (solo con `force`).
    pub pre_restore: Option<PathBuf>,
    /// `true` si se escribió algo (con `force`).
    pub applied: bool,
    /// Qué reiniciar a mano al terminar.
    pub hints: Vec<String>,
}

/// Caso de uso: restaurar la identidad del sistema.
pub struct RestoreSystem {
    archive: Arc<dyn ArchiveReaderPort>,
    backup: BackupSystem,
    /// Raíz del sistema de ficheros (`/` en producción; un tempdir en tests).
    root: PathBuf,
}

impl RestoreSystem {
    /// `root` = `/` en producción.
    pub fn new(archive: Arc<dyn ArchiveReaderPort>, backup: BackupSystem, root: PathBuf) -> Self {
        Self {
            archive,
            backup,
            root,
        }
    }

    /// Ruta de identidad (absoluta bajo `root`) que contiene `rel`.
    fn identity_root_of(&self, rel: &str) -> Option<PathBuf> {
        for p in SYSTEM_IDENTITY_PATHS {
            let p = p.trim_start_matches('/');
            if rel == p || rel.strip_prefix(p).is_some_and(|r| r.starts_with('/')) {
                return Some(self.root.join(p));
            }
        }
        // Perfil AppArmor suelto: el propio fichero.
        let ap = APPARMOR_DIR.trim_start_matches('/');
        if rel.starts_with(ap) && is_allowed_member(rel) {
            return Some(self.root.join(rel));
        }
        None
    }

    /// Valida el archivo y, con `force`, lo restaura. Sin `force` no escribe nada.
    pub async fn run(&self, archive_path: &Path, force: bool) -> Result<RestoreReport> {
        if !archive_path.is_file() {
            return Err(EnolaError::NotFound(format!(
                "Backup file not found: {}",
                archive_path.display()
            )));
        }

        // 1. Todo el listado, antes de escribir nada.
        let entries = self.archive.list(archive_path)?;
        let items = validate_restore_entries(&entries)?;

        // 2. Manifiesto obligatorio y con formato vigente.
        let member = BACKUP_MANIFEST_PATH.trim_start_matches('/');
        let manifest_json = self
            .archive
            .read_text(archive_path, member)?
            .ok_or_else(|| {
                EnolaError::ValidationError(
                    "this archive has no backup manifest (made by an older enola-cli): it is not supported".to_string(),
                )
            })?;
        let manifest = BackupManifest::parse(&manifest_json)?;

        // 3. Qué se sobrescribiría.
        let overwrites: Vec<PathBuf> = items
            .iter()
            .filter(|i| matches!(i.kind, EntryKind::File | EntryKind::Symlink))
            .map(|i| self.root.join(&i.path))
            .filter(|p| std::fs::symlink_metadata(p).is_ok())
            .collect();
        let hints = restart_hints(&items);

        if !force {
            return Ok(RestoreReport {
                manifest,
                items,
                overwrites,
                pre_restore: None,
                applied: false,
                hints,
            });
        }

        // 4. Copia pre-restore de lo que ya existe y se va a tocar.
        let mut to_save: Vec<PathBuf> = items
            .iter()
            .filter_map(|i| self.identity_root_of(&i.path))
            .filter(|p| std::fs::symlink_metadata(p).is_ok())
            .collect();
        to_save.sort();
        to_save.dedup();
        let pre_restore = if to_save.is_empty() {
            None
        } else {
            Some(
                self.backup
                    .create_system_backup(
                        "pre-restore",
                        &to_save,
                        &self.root.join(member),
                        env!("CARGO_PKG_VERSION"),
                    )
                    .await?,
            )
        };

        // 5. Extracción segura (el manifiesto del archivo sustituye al de la copia previa).
        self.archive.extract(archive_path, &self.root, &items)?;

        Ok(RestoreReport {
            manifest,
            items,
            overwrites,
            pre_restore,
            applied: true,
            hints,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::backup::{ArchiveEntry, BACKUP_FORMAT_VERSION};
    use crate::ports::archive::MockArchiveReaderPort;
    use std::sync::atomic::{AtomicBool, Ordering};

    fn entry(path: &str, kind: EntryKind) -> ArchiveEntry {
        ArchiveEntry {
            path: path.into(),
            kind,
            link_target: None,
            owner: None,
            mode: 0o644,
        }
    }

    fn manifest_json() -> String {
        BackupManifest::new("0.12.0", "2026-11-03_20-00-00", vec!["/etc/enola".into()])
            .to_json()
            .unwrap()
    }

    fn system(mock: MockArchiveReaderPort, root: &Path) -> RestoreSystem {
        let backup = BackupSystem::with_backup_root(
            Arc::new(crate::adapters::infra::filesystem::EnolaFileAdapter::new()),
            root.join("backups"),
        );
        RestoreSystem::new(Arc::new(mock), backup, root.to_path_buf())
    }

    fn good_entries() -> Vec<ArchiveEntry> {
        vec![
            entry("etc/enola/", EntryKind::Dir),
            entry("etc/enola/backup-manifest.json", EntryKind::File),
            entry("var/lib/tor/svc/hs_ed25519_secret_key", EntryKind::File),
        ]
    }

    fn archive_file(root: &Path) -> PathBuf {
        let p = root.join("a.tar.gz");
        std::fs::write(&p, b"x").unwrap();
        p
    }

    /// Sin `--force`: informa y NO extrae ni crea copia previa.
    #[tokio::test]
    async fn dry_run_reports_overwrites_and_writes_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("var/lib/tor/svc")).unwrap();
        std::fs::write(
            tmp.path().join("var/lib/tor/svc/hs_ed25519_secret_key"),
            "old",
        )
        .unwrap();
        let mut mock = MockArchiveReaderPort::new();
        mock.expect_list().returning(|_| Ok(good_entries()));
        mock.expect_read_text()
            .returning(|_, _| Ok(Some(manifest_json())));
        mock.expect_extract().times(0);
        let sys = system(mock, tmp.path());

        let r = sys.run(&archive_file(tmp.path()), false).await.unwrap();
        assert!(!r.applied);
        assert!(r.pre_restore.is_none());
        assert_eq!(
            r.overwrites,
            vec![tmp.path().join("var/lib/tor/svc/hs_ed25519_secret_key")]
        );
        assert!(r.hints.iter().any(|h| h.contains("restart tor")));
        assert!(
            !tmp.path().join("backups").exists(),
            "ni siquiera crea el directorio de backups"
        );
    }

    /// Con `--force`: copia `pre-restore` ANTES de extraer, y extrae una vez.
    #[tokio::test]
    async fn force_saves_a_pre_restore_copy_before_extracting() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("var/lib/tor/svc")).unwrap();
        std::fs::write(
            tmp.path().join("var/lib/tor/svc/hs_ed25519_secret_key"),
            "old",
        )
        .unwrap();
        let backups = tmp.path().join("backups");
        let saw_copy = Arc::new(AtomicBool::new(false));
        let flag = saw_copy.clone();
        let mut mock = MockArchiveReaderPort::new();
        mock.expect_list().returning(|_| Ok(good_entries()));
        mock.expect_read_text()
            .returning(|_, _| Ok(Some(manifest_json())));
        mock.expect_extract().times(1).returning(move |_, _, _| {
            let has = std::fs::read_dir(&backups)
                .map(|d| {
                    d.flatten()
                        .any(|e| e.file_name().to_string_lossy().contains("pre-restore"))
                })
                .unwrap_or(false);
            flag.store(has, Ordering::SeqCst);
            Ok(())
        });
        let sys = system(mock, tmp.path());

        let r = sys.run(&archive_file(tmp.path()), true).await.unwrap();
        assert!(r.applied);
        assert!(r.pre_restore.is_some());
        assert!(
            saw_copy.load(Ordering::SeqCst),
            "la copia previa debe existir antes de extraer"
        );
    }

    #[tokio::test]
    async fn dangerous_archive_is_rejected_before_any_write() {
        let tmp = tempfile::tempdir().unwrap();
        let mut mock = MockArchiveReaderPort::new();
        mock.expect_list().returning(|_| {
            Ok(vec![
                entry("etc/enola/ok", EntryKind::File),
                entry("etc/cron.d/x", EntryKind::File),
            ])
        });
        mock.expect_read_text().times(0);
        mock.expect_extract().times(0);
        let sys = system(mock, tmp.path());
        assert!(sys.run(&archive_file(tmp.path()), true).await.is_err());
        assert!(!tmp.path().join("backups").exists());
    }

    #[tokio::test]
    async fn archive_without_manifest_or_with_other_format_is_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let mut mock = MockArchiveReaderPort::new();
        mock.expect_list().returning(|_| Ok(good_entries()));
        mock.expect_read_text().returning(|_, _| Ok(None));
        mock.expect_extract().times(0);
        let r = system(mock, tmp.path())
            .run(&archive_file(tmp.path()), true)
            .await;
        assert!(r.unwrap_err().to_string().contains("no backup manifest"));

        let mut mock = MockArchiveReaderPort::new();
        mock.expect_list().returning(|_| Ok(good_entries()));
        let other = manifest_json().replace(
            &format!("\"format_version\": {BACKUP_FORMAT_VERSION}"),
            "\"format_version\": 7",
        );
        mock.expect_read_text()
            .returning(move |_, _| Ok(Some(other.clone())));
        mock.expect_extract().times(0);
        let r = system(mock, tmp.path())
            .run(&archive_file(tmp.path()), true)
            .await;
        assert!(r
            .unwrap_err()
            .to_string()
            .contains("unsupported backup format"));
    }

    #[tokio::test]
    async fn missing_archive_is_not_found() {
        let tmp = tempfile::tempdir().unwrap();
        let sys = system(MockArchiveReaderPort::new(), tmp.path());
        let r = sys.run(&tmp.path().join("nope.tar.gz"), true).await;
        assert!(matches!(r, Err(EnolaError::NotFound(_))));
    }
}
