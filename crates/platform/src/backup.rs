use std::collections::HashSet;
use std::ffi::{CStr, CString};
use std::fs::{self, File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use std::ptr;
use std::sync::Arc;

use chrono::{Datelike, TimeZone, Utc};
use fs2::FileExt;
use libsqlite3_sys::{
    SQLITE_BUSY, SQLITE_DONE, SQLITE_LOCKED, SQLITE_OK, SQLITE_OPEN_CREATE, SQLITE_OPEN_READWRITE,
    sqlite3_backup_finish, sqlite3_backup_init, sqlite3_backup_step, sqlite3_close, sqlite3_errmsg,
    sqlite3_open_v2, sqlite3_sleep,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use tokio::sync::{OwnedRwLockReadGuard, RwLock};
use uuid::Uuid;

use crate::Database;

const DATABASE_FILE: &str = "database.sqlite";
const MANIFEST_FILE: &str = "manifest.json";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BackupKind {
    Snapshot,
    PreMigration,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BackupFile {
    pub path: String,
    pub sha256: String,
    pub byte_size: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BackupManifest {
    pub id: String,
    pub created_at: i64,
    pub kind: BackupKind,
    pub files: Vec<BackupFile>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackupSnapshot {
    pub id: String,
    pub path: PathBuf,
    pub manifest: BackupManifest,
}

#[derive(Clone, Debug, Default)]
pub struct AttachmentMutationCoordinator {
    lock: Arc<RwLock<()>>,
}

#[derive(Debug)]
pub struct AttachmentMutationGuard {
    _guard: OwnedRwLockReadGuard<()>,
}

impl AttachmentMutationCoordinator {
    pub async fn begin(&self) -> AttachmentMutationGuard {
        AttachmentMutationGuard {
            _guard: self.lock.clone().read_owned().await,
        }
    }
}

#[derive(Clone, Debug)]
pub struct BackupService {
    backup_root: PathBuf,
    attachment_root: PathBuf,
    attachment_mutations: AttachmentMutationCoordinator,
}

#[derive(Debug, Error)]
pub enum BackupError {
    #[error("backup I/O failed at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("backup manifest is invalid at {path}: {source}")]
    Manifest {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("backup {id} does not exist")]
    NotFound { id: String },
    #[error("backup file {path} has checksum {actual}, expected {expected}")]
    ChecksumMismatch {
        path: String,
        expected: String,
        actual: String,
    },
    #[error("backup contains unsupported attachment entry {path}")]
    UnsupportedAttachment { path: PathBuf },
    #[error("backup manifest contains unsafe file path {path}")]
    UnsafeManifestPath { path: String },
    #[error("backup manifest does not match the files stored in the snapshot")]
    InventoryMismatch,
    #[error("SQLite online backup failed: {0}")]
    OnlineBackup(String),
    #[error("database is currently owned by a serving Orbit process: {path}")]
    RestoreTargetOwned { path: PathBuf },
}

impl BackupService {
    #[must_use]
    pub fn new(backup_root: impl Into<PathBuf>, attachment_root: impl Into<PathBuf>) -> Self {
        Self {
            backup_root: backup_root.into(),
            attachment_root: attachment_root.into(),
            attachment_mutations: AttachmentMutationCoordinator::default(),
        }
    }

    #[must_use]
    pub fn attachment_mutations(&self) -> AttachmentMutationCoordinator {
        self.attachment_mutations.clone()
    }

    pub async fn create(&self, database: &Database) -> Result<BackupSnapshot, BackupError> {
        let snapshot = self.create_kind(database, BackupKind::Snapshot).await?;
        self.apply_retention().await?;
        Ok(snapshot)
    }

    pub async fn create_pre_migration(
        &self,
        database: &Database,
    ) -> Result<BackupSnapshot, BackupError> {
        self.create_kind(database, BackupKind::PreMigration).await
    }

    pub async fn list(&self) -> Result<Vec<BackupSnapshot>, BackupError> {
        self.list_kind(BackupKind::Snapshot).await
    }

    pub async fn list_pre_migration(&self) -> Result<Vec<BackupSnapshot>, BackupError> {
        self.list_kind(BackupKind::PreMigration).await
    }

    pub async fn verify(&self, id: &str) -> Result<BackupSnapshot, BackupError> {
        let snapshot = self.find(id).await?;
        verify_snapshot(&snapshot)?;
        Ok(snapshot)
    }

    pub async fn restore(&self, id: &str, database_path: &Path) -> Result<(), BackupError> {
        self.restore_to(id, database_path, &self.attachment_root)
            .await
    }

    pub async fn restore_to(
        &self,
        id: &str,
        database_path: &Path,
        attachment_path: &Path,
    ) -> Result<(), BackupError> {
        let snapshot = self.verify(id).await?;
        let parent = parent_directory(database_path);
        create_dir_all(parent)?;
        let mut target = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(database_path)
            .map_err(|source| io_error(database_path, source))?;
        target
            .try_lock_exclusive()
            .map_err(|_| BackupError::RestoreTargetOwned {
                path: database_path.to_owned(),
            })?;

        let attachment_parent = parent_directory(attachment_path);
        create_dir_all(attachment_parent)?;
        let staged_attachments =
            attachment_parent.join(format!(".orbit-restore-{}", Uuid::now_v7()));
        copy_directory(
            &snapshot.path.join("attachments"),
            &staged_attachments,
            &snapshot.path,
            None,
        )?;

        let mut source = File::open(snapshot.path.join(DATABASE_FILE))
            .map_err(|source| io_error(snapshot.path.join(DATABASE_FILE), source))?;
        target
            .set_len(0)
            .and_then(|()| target.seek(SeekFrom::Start(0)).map(|_| ()))
            .and_then(|()| std::io::copy(&mut source, &mut target).map(|_| ()))
            .and_then(|()| target.sync_all())
            .map_err(|source| io_error(database_path, source))?;
        remove_if_exists(&sidecar_path(database_path, "-wal"))?;
        remove_if_exists(&sidecar_path(database_path, "-shm"))?;

        if attachment_path.exists() {
            fs::remove_dir_all(attachment_path)
                .map_err(|source| io_error(attachment_path, source))?;
        }
        fs::rename(&staged_attachments, attachment_path)
            .map_err(|source| io_error(attachment_path, source))?;
        Ok(())
    }

    async fn create_kind(
        &self,
        database: &Database,
        kind: BackupKind,
    ) -> Result<BackupSnapshot, BackupError> {
        let _pause = self.attachment_mutations.lock.write().await;
        let id = Uuid::now_v7().to_string();
        let kind_root = self.kind_root(kind);
        create_dir_all(&kind_root)?;
        let temporary = kind_root.join(format!(".{id}.tmp"));
        let final_path = kind_root.join(&id);
        create_dir_all(&temporary)?;

        let database_path = temporary.join(DATABASE_FILE);
        if let Err(error) = online_backup(database, &database_path).await {
            let _ = fs::remove_dir_all(&temporary);
            return Err(error);
        }
        let mut files = vec![file_manifest(&database_path, &temporary)?];
        copy_directory(
            &self.attachment_root,
            &temporary.join("attachments"),
            &temporary,
            Some(&mut files),
        )?;
        files.sort_by(|left, right| left.path.cmp(&right.path));
        let manifest = BackupManifest {
            id: id.clone(),
            created_at: Utc::now().timestamp_millis(),
            kind,
            files,
        };
        write_manifest(&temporary, &manifest)?;
        let snapshot = BackupSnapshot {
            id,
            path: temporary.clone(),
            manifest,
        };
        verify_snapshot(&snapshot)?;
        fs::rename(&temporary, &final_path).map_err(|source| io_error(&final_path, source))?;
        Ok(BackupSnapshot {
            path: final_path,
            ..snapshot
        })
    }

    async fn find(&self, id: &str) -> Result<BackupSnapshot, BackupError> {
        if Uuid::parse_str(id).is_err() {
            return Err(BackupError::NotFound { id: id.to_owned() });
        }
        for kind in [BackupKind::Snapshot, BackupKind::PreMigration] {
            let path = self.kind_root(kind).join(id);
            if path.is_dir() {
                return read_snapshot(path);
            }
        }
        Err(BackupError::NotFound { id: id.to_owned() })
    }

    async fn list_kind(&self, kind: BackupKind) -> Result<Vec<BackupSnapshot>, BackupError> {
        let root = self.kind_root(kind);
        if !root.exists() {
            return Ok(Vec::new());
        }
        let mut snapshots = Vec::new();
        for entry in fs::read_dir(&root).map_err(|source| io_error(&root, source))? {
            let entry = entry.map_err(|source| io_error(&root, source))?;
            if entry
                .file_type()
                .map_err(|source| io_error(entry.path(), source))?
                .is_dir()
                && !entry.file_name().to_string_lossy().starts_with('.')
            {
                snapshots.push(read_snapshot(entry.path())?);
            }
        }
        snapshots.sort_by(|left, right| {
            right
                .manifest
                .created_at
                .cmp(&left.manifest.created_at)
                .then_with(|| right.id.cmp(&left.id))
        });
        Ok(snapshots)
    }

    async fn apply_retention(&self) -> Result<(), BackupError> {
        let snapshots = self.list().await?;
        let mut daily = HashSet::new();
        let mut weekly = HashSet::new();
        let mut keep = HashSet::new();
        for snapshot in &snapshots {
            let Some(created) = Utc
                .timestamp_millis_opt(snapshot.manifest.created_at)
                .single()
            else {
                continue;
            };
            let daily_bucket = (created.year(), created.ordinal());
            if daily.len() < 7 && daily.insert(daily_bucket) {
                keep.insert(snapshot.id.clone());
            }
            let week = created.iso_week();
            let weekly_bucket = (week.year(), week.week());
            if weekly.len() < 4 && weekly.insert(weekly_bucket) {
                keep.insert(snapshot.id.clone());
            }
        }
        for snapshot in snapshots {
            if !keep.contains(&snapshot.id) {
                fs::remove_dir_all(&snapshot.path)
                    .map_err(|source| io_error(&snapshot.path, source))?;
            }
        }
        Ok(())
    }

    fn kind_root(&self, kind: BackupKind) -> PathBuf {
        self.backup_root.join(match kind {
            BackupKind::Snapshot => "snapshots",
            BackupKind::PreMigration => "pre-migration",
        })
    }
}

async fn online_backup(database: &Database, destination: &Path) -> Result<(), BackupError> {
    let filename = CString::new(destination.to_string_lossy().as_bytes())
        .map_err(|_| BackupError::OnlineBackup("destination contains a NUL byte".to_owned()))?;
    let main = c"main";
    let mut connection = database
        .pool()
        .acquire()
        .await
        .map_err(|error| BackupError::OnlineBackup(error.to_string()))?;
    let mut source = connection
        .lock_handle()
        .await
        .map_err(|error| BackupError::OnlineBackup(error.to_string()))?;

    let mut destination_handle = ptr::null_mut();
    // SAFETY: SQLite owns the destination handle until it is closed below. The source handle is
    // protected from SQLx's worker by `lock_handle` for the entire backup operation.
    unsafe {
        let opened = sqlite3_open_v2(
            filename.as_ptr(),
            &mut destination_handle,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
            ptr::null(),
        );
        if opened != SQLITE_OK {
            let message = sqlite_error(destination_handle);
            if !destination_handle.is_null() {
                sqlite3_close(destination_handle);
            }
            return Err(BackupError::OnlineBackup(message));
        }
        let backup = sqlite3_backup_init(
            destination_handle,
            main.as_ptr(),
            source.as_raw_handle().as_ptr(),
            main.as_ptr(),
        );
        if backup.is_null() {
            let message = sqlite_error(destination_handle);
            sqlite3_close(destination_handle);
            return Err(BackupError::OnlineBackup(message));
        }
        let result = loop {
            match sqlite3_backup_step(backup, 128) {
                SQLITE_DONE => break Ok(()),
                SQLITE_OK => {}
                SQLITE_BUSY | SQLITE_LOCKED => {
                    sqlite3_sleep(25);
                }
                _ => break Err(BackupError::OnlineBackup(sqlite_error(destination_handle))),
            }
        };
        let finish = sqlite3_backup_finish(backup);
        let close = sqlite3_close(destination_handle);
        result?;
        if finish != SQLITE_OK || close != SQLITE_OK {
            return Err(BackupError::OnlineBackup(
                "could not finalize the SQLite snapshot".to_owned(),
            ));
        }
    }
    Ok(())
}

unsafe fn sqlite_error(handle: *mut libsqlite3_sys::sqlite3) -> String {
    if handle.is_null() {
        return "could not allocate SQLite connection".to_owned();
    }
    // SAFETY: SQLite guarantees that errmsg returns a valid, connection-owned C string.
    unsafe { CStr::from_ptr(sqlite3_errmsg(handle)) }
        .to_string_lossy()
        .into_owned()
}

fn copy_directory(
    source: &Path,
    destination: &Path,
    backup_root: &Path,
    mut files: Option<&mut Vec<BackupFile>>,
) -> Result<(), BackupError> {
    create_dir_all(destination)?;
    if !source.exists() {
        return Ok(());
    }
    let mut entries = fs::read_dir(source)
        .map_err(|error| io_error(source, error))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| io_error(source, error))?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let file_type = entry
            .file_type()
            .map_err(|error| io_error(&source_path, error))?;
        if file_type.is_dir() {
            copy_directory(
                &source_path,
                &destination_path,
                backup_root,
                files.as_deref_mut(),
            )?;
        } else if file_type.is_file() {
            fs::copy(&source_path, &destination_path)
                .map_err(|error| io_error(&destination_path, error))?;
            if let Some(files) = files.as_deref_mut() {
                files.push(file_manifest(&destination_path, backup_root)?);
            }
        } else {
            return Err(BackupError::UnsupportedAttachment { path: source_path });
        }
    }
    Ok(())
}

fn file_manifest(path: &Path, root: &Path) -> Result<BackupFile, BackupError> {
    let bytes = fs::read(path).map_err(|source| io_error(path, source))?;
    let relative = path
        .strip_prefix(root)
        .expect("backup files are created below the backup root")
        .to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/");
    Ok(BackupFile {
        path: relative,
        sha256: format!("{:x}", Sha256::digest(&bytes)),
        byte_size: bytes.len() as u64,
    })
}

fn write_manifest(path: &Path, manifest: &BackupManifest) -> Result<(), BackupError> {
    let manifest_path = path.join(MANIFEST_FILE);
    let contents = serde_json::to_vec_pretty(manifest).map_err(|source| BackupError::Manifest {
        path: manifest_path.clone(),
        source,
    })?;
    let mut file =
        File::create(&manifest_path).map_err(|source| io_error(&manifest_path, source))?;
    file.write_all(&contents)
        .and_then(|()| file.sync_all())
        .map_err(|source| io_error(&manifest_path, source))
}

fn read_snapshot(path: PathBuf) -> Result<BackupSnapshot, BackupError> {
    let manifest_path = path.join(MANIFEST_FILE);
    let contents = fs::read(&manifest_path).map_err(|source| io_error(&manifest_path, source))?;
    let manifest: BackupManifest =
        serde_json::from_slice(&contents).map_err(|source| BackupError::Manifest {
            path: manifest_path,
            source,
        })?;
    Ok(BackupSnapshot {
        id: manifest.id.clone(),
        path,
        manifest,
    })
}

fn verify_snapshot(snapshot: &BackupSnapshot) -> Result<(), BackupError> {
    let mut actual_files = Vec::new();
    collect_snapshot_files(&snapshot.path, &snapshot.path, &mut actual_files)?;
    actual_files.sort();
    let mut expected_files = snapshot
        .manifest
        .files
        .iter()
        .map(|file| file.path.clone())
        .collect::<Vec<_>>();
    expected_files.sort();
    if actual_files != expected_files {
        return Err(BackupError::InventoryMismatch);
    }
    for expected in &snapshot.manifest.files {
        let relative = Path::new(&expected.path);
        if relative.as_os_str().is_empty()
            || relative
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err(BackupError::UnsafeManifestPath {
                path: expected.path.clone(),
            });
        }
        let path = snapshot.path.join(relative);
        let actual = file_manifest(&path, &snapshot.path)?;
        if actual.sha256 != expected.sha256 || actual.byte_size != expected.byte_size {
            return Err(BackupError::ChecksumMismatch {
                path: expected.path.clone(),
                expected: expected.sha256.clone(),
                actual: actual.sha256,
            });
        }
    }
    Ok(())
}

fn collect_snapshot_files(
    root: &Path,
    directory: &Path,
    files: &mut Vec<String>,
) -> Result<(), BackupError> {
    for entry in fs::read_dir(directory).map_err(|source| io_error(directory, source))? {
        let entry = entry.map_err(|source| io_error(directory, source))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|source| io_error(&path, source))?;
        if file_type.is_dir() {
            collect_snapshot_files(root, &path, files)?;
        } else if file_type.is_file() {
            let relative = path
                .strip_prefix(root)
                .expect("snapshot entries are below the snapshot root")
                .to_string_lossy()
                .replace(std::path::MAIN_SEPARATOR, "/");
            if relative != MANIFEST_FILE {
                files.push(relative);
            }
        } else {
            return Err(BackupError::UnsupportedAttachment { path });
        }
    }
    Ok(())
}

fn create_dir_all(path: &Path) -> Result<(), BackupError> {
    fs::create_dir_all(path).map_err(|source| io_error(path, source))
}

fn remove_if_exists(path: &Path) -> Result<(), BackupError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(io_error(path, source)),
    }
}

fn sidecar_path(path: &Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_owned();
    value.push(suffix);
    value.into()
}

fn parent_directory(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn io_error(path: impl Into<PathBuf>, source: std::io::Error) -> BackupError {
    BackupError::Io {
        path: path.into(),
        source,
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::parent_directory;

    #[test]
    fn a_bare_filename_uses_the_current_directory_as_its_parent() {
        assert_eq!(parent_directory(Path::new("orbit.sqlite")), Path::new("."));
    }
}
