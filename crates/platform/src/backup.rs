use std::collections::HashSet;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use chrono::{Datelike, TimeZone, Utc};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use tokio::sync::{OwnedRwLockReadGuard, RwLock};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::Database;

const DATABASE_FILE: &str = "database.sqlite";
const MANIFEST_FILE: &str = "manifest.json";
const SUPPORTED_SCHEMA_VERSION: i64 = 14;

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
    pub schema_version: i64,
    pub application_version: String,
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
    #[error("backup root {backup_root} and attachment root {attachment_root} must not overlap")]
    PathOverlap {
        backup_root: PathBuf,
        attachment_root: PathBuf,
    },
    #[error(
        "backup schema version {found} is unsupported; maximum supported is {maximum_supported}"
    )]
    UnsupportedSchema { found: i64, maximum_supported: i64 },
    #[error("could not read the database schema version: {0}")]
    SchemaVersion(#[source] sqlx::Error),
    #[error("SQLite online backup failed: {0}")]
    OnlineBackup(String),
    #[error("backup was cancelled")]
    Cancelled,
    #[error("backup blocking task failed: {0}")]
    BlockingTask(String),
    #[error("database is currently owned by a serving Orbit process: {path}")]
    RestoreTargetOwned { path: PathBuf },
    #[error(
        "restore database path {database_path} and attachment path {attachment_path} must not overlap"
    )]
    RestorePathOverlap {
        database_path: PathBuf,
        attachment_path: PathBuf,
    },
    #[error("restore failed ({original}) and rollback was incomplete ({rollback})")]
    RestoreRollback { original: String, rollback: String },
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
        self.create_cancellable(database, CancellationToken::new())
            .await
    }

    pub async fn create_cancellable(
        &self,
        database: &Database,
        cancellation: CancellationToken,
    ) -> Result<BackupSnapshot, BackupError> {
        let snapshot = self
            .create_kind(database, BackupKind::Snapshot, cancellation.clone())
            .await?;
        self.apply_retention(cancellation).await?;
        Ok(snapshot)
    }

    pub async fn create_pre_migration(
        &self,
        database: &Database,
    ) -> Result<BackupSnapshot, BackupError> {
        self.create_kind(database, BackupKind::PreMigration, CancellationToken::new())
            .await
    }

    pub async fn list(&self) -> Result<Vec<BackupSnapshot>, BackupError> {
        let service = self.clone();
        tokio::task::spawn_blocking(move || service.list_kind_blocking(BackupKind::Snapshot))
            .await
            .map_err(|error| BackupError::BlockingTask(error.to_string()))?
    }

    pub async fn list_pre_migration(&self) -> Result<Vec<BackupSnapshot>, BackupError> {
        let service = self.clone();
        tokio::task::spawn_blocking(move || service.list_kind_blocking(BackupKind::PreMigration))
            .await
            .map_err(|error| BackupError::BlockingTask(error.to_string()))?
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
        self.restore_to_with_hook(id, database_path, attachment_path, || Ok(()))
            .await
    }

    async fn restore_to_with_hook<F>(
        &self,
        id: &str,
        database_path: &Path,
        attachment_path: &Path,
        late_hook: F,
    ) -> Result<(), BackupError>
    where
        F: FnOnce() -> Result<(), BackupError>,
    {
        self.validate_layout()?;
        let snapshot = self.verify(id).await?;
        let database_path = canonicalize_future_path(database_path)?;
        let attachment_path = canonicalize_future_path(attachment_path)?;
        if database_path == attachment_path
            || database_path.starts_with(&attachment_path)
            || attachment_path.starts_with(&database_path)
        {
            return Err(BackupError::RestorePathOverlap {
                database_path,
                attachment_path,
            });
        }
        let database_parent = parent_directory(&database_path);
        let attachment_parent = parent_directory(&attachment_path);
        create_dir_all(database_parent)?;
        create_dir_all(attachment_parent)?;

        let token = Uuid::now_v7();
        let staged_database = database_parent.join(format!(".orbit-restore-new-db-{token}"));
        let old_database = database_parent.join(format!(".orbit-restore-old-db-{token}"));
        let staged_attachments =
            attachment_parent.join(format!(".orbit-restore-new-attachments-{token}"));
        let old_attachments =
            attachment_parent.join(format!(".orbit-restore-old-attachments-{token}"));

        let result = (|| {
            copy_file_synced(&snapshot.path.join(DATABASE_FILE), &staged_database)?;
            copy_directory(
                &snapshot.path.join("attachments"),
                &staged_attachments,
                &snapshot.path,
                None,
            )?;

            let staged_database_lock = OpenOptions::new()
                .read(true)
                .write(true)
                .open(&staged_database)
                .map_err(|source| io_error(&staged_database, source))?;
            staged_database_lock
                .try_lock_exclusive()
                .map_err(|source| io_error(&staged_database, source))?;

            let database_existed = database_path.exists();
            let target_lock = OpenOptions::new()
                .create(true)
                .read(true)
                .write(true)
                .truncate(false)
                .open(&database_path)
                .map_err(|source| io_error(&database_path, source))?;
            target_lock
                .try_lock_exclusive()
                .map_err(|_| BackupError::RestoreTargetOwned {
                    path: database_path.clone(),
                })?;

            let mut database_old_moved = false;
            let mut database_new_installed = false;
            let mut attachments_old_moved = false;
            let mut attachments_new_installed = false;
            let mut moved_sidecars = Vec::new();

            let switch_result: Result<(), BackupError> = (|| {
                if database_existed {
                    fs::rename(&database_path, &old_database)
                        .map_err(|source| io_error(&database_path, source))?;
                    database_old_moved = true;
                } else {
                    fs::remove_file(&database_path)
                        .map_err(|source| io_error(&database_path, source))?;
                }
                for suffix in ["-wal", "-shm"] {
                    let original = sidecar_path(&database_path, suffix);
                    if original.exists() {
                        let preserved = sidecar_path(&old_database, suffix);
                        fs::rename(&original, &preserved)
                            .map_err(|source| io_error(&original, source))?;
                        moved_sidecars.push((original, preserved));
                    }
                }
                fs::rename(&staged_database, &database_path)
                    .map_err(|source| io_error(&database_path, source))?;
                database_new_installed = true;

                if attachment_path.exists() {
                    fs::rename(&attachment_path, &old_attachments)
                        .map_err(|source| io_error(&attachment_path, source))?;
                    attachments_old_moved = true;
                }
                late_hook()?;
                fs::rename(&staged_attachments, &attachment_path)
                    .map_err(|source| io_error(&attachment_path, source))?;
                attachments_new_installed = true;
                Ok(())
            })();

            if let Err(original) = switch_result {
                let rollback = rollback_restore(
                    &database_path,
                    &old_database,
                    database_old_moved,
                    database_new_installed,
                    &attachment_path,
                    &old_attachments,
                    attachments_old_moved,
                    attachments_new_installed,
                    &moved_sidecars,
                );
                return match rollback {
                    Ok(()) => Err(original),
                    Err(rollback) => Err(BackupError::RestoreRollback {
                        original: original.to_string(),
                        rollback: rollback.to_string(),
                    }),
                };
            }

            drop(target_lock);
            let _ = remove_path_if_exists(&old_database);
            for (_, preserved) in moved_sidecars {
                let _ = remove_path_if_exists(&preserved);
            }
            let _ = remove_path_if_exists(&old_attachments);
            drop(staged_database_lock);
            Ok(())
        })();

        let _ = remove_path_if_exists(&staged_database);
        let _ = remove_path_if_exists(&staged_attachments);
        result
    }

    async fn create_kind(
        &self,
        database: &Database,
        kind: BackupKind,
        cancellation: CancellationToken,
    ) -> Result<BackupSnapshot, BackupError> {
        self.validate_layout()?;
        let _pause = tokio::select! {
            guard = self.attachment_mutations.lock.write() => guard,
            () = cancellation.cancelled() => return Err(BackupError::Cancelled),
        };
        let schema_version = tokio::select! {
            result = database.scalar::<i64>("SELECT COALESCE(MAX(version), 0) FROM schema_migrations") => {
                result.map_err(BackupError::SchemaVersion)?
            }
            () = cancellation.cancelled() => return Err(BackupError::Cancelled),
        };
        let id = Uuid::now_v7().to_string();
        let kind_root = self.kind_root(kind);
        create_dir_all(&kind_root)?;
        let temporary = kind_root.join(format!(".{id}.tmp"));
        let final_path = kind_root.join(&id);
        create_dir_all(&temporary)?;

        let database_path = temporary.join(DATABASE_FILE);
        if let Err(error) = online_backup(database, &database_path, &cancellation).await {
            let _ = fs::remove_dir_all(&temporary);
            return Err(error);
        }
        let attachment_root = self.attachment_root.clone();
        tokio::task::spawn_blocking(move || {
            let result = (|| {
                check_cancelled(&cancellation)?;
                let mut files = vec![file_manifest_cancellable(
                    &database_path,
                    &temporary,
                    &cancellation,
                )?];
                copy_directory_cancellable(
                    &attachment_root,
                    &temporary.join("attachments"),
                    &temporary,
                    &mut files,
                    &cancellation,
                )?;
                files.sort_by(|left, right| left.path.cmp(&right.path));
                let manifest = BackupManifest {
                    id: id.clone(),
                    created_at: Utc::now().timestamp_millis(),
                    kind,
                    schema_version,
                    application_version: env!("CARGO_PKG_VERSION").to_owned(),
                    files,
                };
                write_manifest(&temporary, &manifest)?;
                let unpublished = BackupSnapshot {
                    id: id.clone(),
                    path: temporary.clone(),
                    manifest: manifest.clone(),
                };
                verify_snapshot_cancellable(&unpublished, &cancellation)?;
                check_cancelled(&cancellation)?;
                fs::rename(&temporary, &final_path)
                    .map_err(|source| io_error(&final_path, source))?;
                Ok(BackupSnapshot {
                    id,
                    path: final_path,
                    manifest,
                })
            })();
            if result.is_err() {
                let _ = fs::remove_dir_all(&temporary);
            }
            result
        })
        .await
        .map_err(|error| BackupError::BlockingTask(error.to_string()))?
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

    fn list_kind_blocking(&self, kind: BackupKind) -> Result<Vec<BackupSnapshot>, BackupError> {
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

    async fn apply_retention(&self, cancellation: CancellationToken) -> Result<(), BackupError> {
        let service = self.clone();
        tokio::task::spawn_blocking(move || {
            let snapshots = service.list_kind_blocking(BackupKind::Snapshot)?;
            let mut daily = HashSet::new();
            let mut weekly = HashSet::new();
            let mut keep = HashSet::new();
            for snapshot in &snapshots {
                check_cancelled(&cancellation)?;
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
                check_cancelled(&cancellation)?;
                if !keep.contains(&snapshot.id) {
                    fs::remove_dir_all(&snapshot.path)
                        .map_err(|source| io_error(&snapshot.path, source))?;
                }
            }
            Ok(())
        })
        .await
        .map_err(|error| BackupError::BlockingTask(error.to_string()))?
    }

    fn kind_root(&self, kind: BackupKind) -> PathBuf {
        self.backup_root.join(match kind {
            BackupKind::Snapshot => "snapshots",
            BackupKind::PreMigration => "pre-migration",
        })
    }

    fn validate_layout(&self) -> Result<(), BackupError> {
        let backup_root = canonicalize_future_path(&self.backup_root)?;
        let attachment_root = canonicalize_future_path(&self.attachment_root)?;
        if backup_root == attachment_root
            || backup_root.starts_with(&attachment_root)
            || attachment_root.starts_with(&backup_root)
        {
            return Err(BackupError::PathOverlap {
                backup_root,
                attachment_root,
            });
        }
        Ok(())
    }
}

async fn online_backup(
    database: &Database,
    destination: &Path,
    cancellation: &CancellationToken,
) -> Result<(), BackupError> {
    check_cancelled(cancellation)?;
    let destination = destination.to_string_lossy().into_owned();
    tokio::select! {
        biased;
        () = cancellation.cancelled() => return Err(BackupError::Cancelled),
        result = sqlx::query("VACUUM INTO ?").bind(destination).execute(database.pool()) => {
            result.map_err(|error| BackupError::OnlineBackup(error.to_string()))?;
        }
    }
    check_cancelled(cancellation)
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

fn check_cancelled(cancellation: &CancellationToken) -> Result<(), BackupError> {
    if cancellation.is_cancelled() {
        Err(BackupError::Cancelled)
    } else {
        Ok(())
    }
}

fn copy_directory_cancellable(
    source: &Path,
    destination: &Path,
    backup_root: &Path,
    files: &mut Vec<BackupFile>,
    cancellation: &CancellationToken,
) -> Result<(), BackupError> {
    check_cancelled(cancellation)?;
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
        check_cancelled(cancellation)?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let file_type = entry
            .file_type()
            .map_err(|error| io_error(&source_path, error))?;
        if file_type.is_dir() {
            copy_directory_cancellable(
                &source_path,
                &destination_path,
                backup_root,
                files,
                cancellation,
            )?;
        } else if file_type.is_file() {
            copy_file_cancellable(&source_path, &destination_path, cancellation)?;
            files.push(file_manifest_cancellable(
                &destination_path,
                backup_root,
                cancellation,
            )?);
        } else {
            return Err(BackupError::UnsupportedAttachment { path: source_path });
        }
    }
    Ok(())
}

fn copy_file_cancellable(
    source: &Path,
    destination: &Path,
    cancellation: &CancellationToken,
) -> Result<(), BackupError> {
    let mut source_file = File::open(source).map_err(|error| io_error(source, error))?;
    let mut destination_file =
        File::create(destination).map_err(|error| io_error(destination, error))?;
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        check_cancelled(cancellation)?;
        let read = source_file
            .read(&mut buffer)
            .map_err(|error| io_error(source, error))?;
        if read == 0 {
            break;
        }
        destination_file
            .write_all(&buffer[..read])
            .map_err(|error| io_error(destination, error))?;
    }
    Ok(())
}

fn file_manifest_cancellable(
    path: &Path,
    root: &Path,
    cancellation: &CancellationToken,
) -> Result<BackupFile, BackupError> {
    let mut file = File::open(path).map_err(|source| io_error(path, source))?;
    let mut digest = Sha256::new();
    let mut byte_size = 0_u64;
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        check_cancelled(cancellation)?;
        let read = file
            .read(&mut buffer)
            .map_err(|source| io_error(path, source))?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
        byte_size = byte_size.saturating_add(read as u64);
    }
    let relative = path
        .strip_prefix(root)
        .expect("backup files are created below the backup root")
        .to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/");
    Ok(BackupFile {
        path: relative,
        sha256: format!("{:x}", digest.finalize()),
        byte_size,
    })
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
    verify_snapshot_cancellable(snapshot, &CancellationToken::new())
}

fn verify_snapshot_cancellable(
    snapshot: &BackupSnapshot,
    cancellation: &CancellationToken,
) -> Result<(), BackupError> {
    check_cancelled(cancellation)?;
    if snapshot.manifest.schema_version < 0
        || snapshot.manifest.schema_version > SUPPORTED_SCHEMA_VERSION
    {
        return Err(BackupError::UnsupportedSchema {
            found: snapshot.manifest.schema_version,
            maximum_supported: SUPPORTED_SCHEMA_VERSION,
        });
    }
    let mut actual_files = Vec::new();
    collect_snapshot_files_cancellable(
        &snapshot.path,
        &snapshot.path,
        &mut actual_files,
        cancellation,
    )?;
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
        check_cancelled(cancellation)?;
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
        let actual = file_manifest_cancellable(&path, &snapshot.path, cancellation)?;
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

fn collect_snapshot_files_cancellable(
    root: &Path,
    directory: &Path,
    files: &mut Vec<String>,
    cancellation: &CancellationToken,
) -> Result<(), BackupError> {
    for entry in fs::read_dir(directory).map_err(|source| io_error(directory, source))? {
        check_cancelled(cancellation)?;
        let entry = entry.map_err(|source| io_error(directory, source))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|source| io_error(&path, source))?;
        if file_type.is_dir() {
            collect_snapshot_files_cancellable(root, &path, files, cancellation)?;
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

fn copy_file_synced(source: &Path, destination: &Path) -> Result<(), BackupError> {
    let mut source_file = File::open(source).map_err(|error| io_error(source, error))?;
    let mut destination_file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(destination)
        .map_err(|error| io_error(destination, error))?;
    std::io::copy(&mut source_file, &mut destination_file)
        .and_then(|_| destination_file.sync_all())
        .map_err(|error| io_error(destination, error))
}

#[allow(clippy::too_many_arguments)]
fn rollback_restore(
    database_path: &Path,
    old_database: &Path,
    database_old_moved: bool,
    database_new_installed: bool,
    attachment_path: &Path,
    old_attachments: &Path,
    attachments_old_moved: bool,
    attachments_new_installed: bool,
    moved_sidecars: &[(PathBuf, PathBuf)],
) -> Result<(), BackupError> {
    let mut first_error = None;
    if attachments_new_installed {
        capture_error(&mut first_error, remove_path_if_exists(attachment_path));
    }
    if attachments_old_moved {
        capture_error(
            &mut first_error,
            fs::rename(old_attachments, attachment_path)
                .map_err(|source| io_error(attachment_path, source)),
        );
    }
    if database_new_installed {
        capture_error(&mut first_error, remove_path_if_exists(database_path));
    }
    if database_old_moved {
        capture_error(
            &mut first_error,
            fs::rename(old_database, database_path)
                .map_err(|source| io_error(database_path, source)),
        );
    }
    for (original, preserved) in moved_sidecars.iter().rev() {
        capture_error(
            &mut first_error,
            fs::rename(preserved, original).map_err(|source| io_error(original, source)),
        );
    }
    match first_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

fn capture_error(first_error: &mut Option<BackupError>, result: Result<(), BackupError>) {
    if let Err(error) = result
        && first_error.is_none()
    {
        *first_error = Some(error);
    }
}

fn remove_path_if_exists(path: &Path) -> Result<(), BackupError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(io_error(path, error)),
    };
    if metadata.file_type().is_dir() {
        fs::remove_dir_all(path).map_err(|source| io_error(path, source))
    } else {
        fs::remove_file(path).map_err(|source| io_error(path, source))
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

fn canonicalize_future_path(path: &Path) -> Result<PathBuf, BackupError> {
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()
            .map_err(|source| io_error(path, source))?
            .join(path)
    };
    let normalized = normalize_path(&absolute)?;
    let mut existing = normalized.clone();
    let mut missing = Vec::<OsString>::new();
    while !existing.exists() {
        let name = existing.file_name().ok_or_else(|| {
            io_error(
                path,
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "path has no existing ancestor",
                ),
            )
        })?;
        missing.push(name.to_owned());
        existing.pop();
    }
    let mut resolved = fs::canonicalize(&existing).map_err(|source| io_error(&existing, source))?;
    for component in missing.into_iter().rev() {
        resolved.push(component);
    }
    Ok(resolved)
}

fn normalize_path(path: &Path) -> Result<PathBuf, BackupError> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    return Err(io_error(
                        path,
                        std::io::Error::new(
                            std::io::ErrorKind::InvalidInput,
                            "path escapes its filesystem root",
                        ),
                    ));
                }
            }
        }
    }
    Ok(normalized)
}

fn io_error(path: impl Into<PathBuf>, source: std::io::Error) -> BackupError {
    BackupError::Io {
        path: path.into(),
        source,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use crate::{Database, DatabaseConfig, MigrationRunner};

    use super::{BackupService, io_error, parent_directory};

    #[test]
    fn a_bare_filename_uses_the_current_directory_as_its_parent() {
        assert_eq!(parent_directory(Path::new("orbit.sqlite")), Path::new("."));
    }

    #[tokio::test]
    async fn late_restore_failure_rolls_back_the_database_and_attachments() {
        let root = tempfile::tempdir().unwrap();
        let source_database =
            Database::open(&DatabaseConfig::new(root.path().join("source.sqlite")))
                .await
                .unwrap();
        MigrationRunner::embedded(env!("CARGO_PKG_VERSION"))
            .run(&source_database)
            .await
            .unwrap();
        source_database
            .execute("INSERT INTO installation_state (id, initialized) VALUES (1, 1)")
            .await
            .unwrap();
        let source_attachments = root.path().join("source-attachments");
        fs::create_dir_all(&source_attachments).unwrap();
        fs::write(source_attachments.join("new.txt"), b"new attachment").unwrap();
        let service = BackupService::new(root.path().join("backups"), &source_attachments);
        let snapshot = service.create(&source_database).await.unwrap();

        let target_path = root.path().join("target.sqlite");
        let target_database = Database::open(&DatabaseConfig::new(&target_path))
            .await
            .unwrap();
        MigrationRunner::embedded(env!("CARGO_PKG_VERSION"))
            .run(&target_database)
            .await
            .unwrap();
        target_database
            .execute("INSERT INTO installation_state (id, initialized) VALUES (1, 0)")
            .await
            .unwrap();
        drop(target_database);
        let target_attachments = root.path().join("target-attachments");
        fs::create_dir_all(&target_attachments).unwrap();
        fs::write(target_attachments.join("old.txt"), b"old attachment").unwrap();

        let error = service
            .restore_to_with_hook(&snapshot.id, &target_path, &target_attachments, || {
                Err(io_error(
                    &target_attachments,
                    std::io::Error::other("injected late restore failure"),
                ))
            })
            .await
            .unwrap_err();

        assert!(error.to_string().contains("injected late restore failure"));
        let target_database = Database::open(&DatabaseConfig::new(&target_path))
            .await
            .unwrap();
        assert_eq!(
            target_database
                .scalar::<i64>("SELECT initialized FROM installation_state WHERE id = 1")
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            fs::read(target_attachments.join("old.txt")).unwrap(),
            b"old attachment"
        );
        assert!(!target_attachments.join("new.txt").exists());
        assert!(fs::read_dir(root.path()).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".orbit-restore")
        }));
    }
}
