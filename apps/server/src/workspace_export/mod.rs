//! Full workspace export (data portability): an owner or admin starts it, a `workspace.export` job
//! writes a ZIP of the workspace (layout in [`archive::README`]) and stores it, and the owner and
//! admins download it until it expires.
//!
//! State machine (`workspace_exports.status`): `queued` → `running` → `ready`, or `failed` after
//! the job's last attempt. One export per workspace is `queued` or `running` at a time. The ZIP is
//! `exports/{workspace_id}/{id}.zip` where new attachment blobs of the workspace go (its own bucket,
//! the installation bucket, or the server's disk under the backups directory, which backups do not
//! copy). Retention deletes it with its row [`EXPORT_TTL_MILLIS`] after it is ready.

mod archive;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use orbit_domain::Permission;
use orbit_platform::{
    BlobReader, BlobStore, Database, Id, Job, JobError, JobKind, JobKindRegistrationError,
    JobStore, ObjectStorage, Row, S3Bucket, TimestampMillis, Worker,
};
use serde::Serialize;
use serde_json::{Value, json};
use thiserror::Error;
use tokio_util::sync::CancellationToken;
use utoipa::ToSchema;

use crate::audit::{self, AuditOutcome};
use crate::export::names::{STEM_MAX_CHARS, sanitize_segment};
use crate::repositories::membership;

pub const EXPORT_JOB: &str = "workspace.export";
/// A ready export is deleted this long after it was written.
pub const EXPORT_TTL_MILLIS: i64 = 7 * 24 * 60 * 60 * 1000;
/// An export still `queued` or `running` after this long failed without its job noticing.
const STALLED_AFTER_MILLIS: i64 = 24 * 60 * 60 * 1000;
const MAX_LISTED: i64 = 20;
const ACTIVE: &str = "('queued', 'running')";

/// Two exports at a time on all servers, one per workspace while another waits (Admin can change
/// both), and two retries for storage outages.
#[must_use]
pub fn job_kind() -> JobKind {
    JobKind::new(EXPORT_JOB)
        .with_concurrency_limit(2)
        .with_workspace_limit(1)
        .with_retry_policy(
            3,
            vec![Duration::from_secs(60), Duration::from_secs(10 * 60)],
        )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceExportStatus {
    Queued,
    Running,
    Ready,
    Failed,
}

impl WorkspaceExportStatus {
    fn parse(value: &str) -> Self {
        match value {
            "queued" => Self::Queued,
            "running" => Self::Running,
            "ready" => Self::Ready,
            _ => Self::Failed,
        }
    }
}

/// Where a ready archive is stored.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Location {
    /// The server's disk (`{backups}/exports/…`).
    Local,
    /// The installation's bucket (Admin → Storage).
    Bucket,
    /// The workspace's own bucket (Settings → File storage).
    WorkspaceBucket,
}

impl Location {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Bucket => "bucket",
            Self::WorkspaceBucket => "workspace_bucket",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "local" => Some(Self::Local),
            "bucket" => Some(Self::Bucket),
            "workspace_bucket" => Some(Self::WorkspaceBucket),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct WorkspaceExport {
    #[schema(value_type = String)]
    pub id: Id,
    pub status: WorkspaceExportStatus,
    /// The member who started it; `null` when the account is gone.
    pub requested_by_name: Option<String>,
    /// Size of the ZIP archive once it is ready.
    pub size_bytes: Option<u64>,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: TimestampMillis,
    #[schema(value_type = Option<String>, format = DateTime)]
    pub completed_at: Option<TimestampMillis>,
    /// The archive is deleted at this time.
    #[schema(value_type = String, format = DateTime)]
    pub expires_at: TimestampMillis,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct WorkspaceExportList {
    /// The newest exports first, without expired ones.
    pub items: Vec<WorkspaceExport>,
}

/// A ready archive, opened for download.
pub struct ExportDownload {
    pub reader: BlobReader,
    pub size_bytes: u64,
    pub file_name: String,
}

#[derive(Debug, Error)]
pub enum ExportError {
    /// Not a member, an unknown workspace, or no such (ready, unexpired) export.
    #[error("workspace export not found")]
    NotFound,
    #[error("only workspace owners and administrators can export the workspace")]
    Forbidden,
    #[error("another export of the workspace is in progress")]
    InProgress,
    #[error("the export was stopped")]
    Cancelled,
    #[error("workspace export storage failed: {0}")]
    Storage(String),
    #[error("workspace export repository is unavailable")]
    Unavailable(#[from] sqlx::Error),
}

#[derive(Clone)]
pub struct WorkspaceExportService {
    inner: Arc<Inner>,
}

struct Inner {
    database: Database,
    blobs: Arc<dyn BlobStore>,
    storage: ObjectStorage,
    /// Holds `exports/…` for archives on the server's disk.
    local_root: PathBuf,
    /// Links to pages outside the export use this origin.
    public_origin: String,
}

fn storage_key(workspace_id: &str, export_id: &str) -> String {
    format!("exports/{workspace_id}/{export_id}.zip")
}

fn storage_error(error: impl std::fmt::Display) -> ExportError {
    ExportError::Storage(error.to_string())
}

async fn require_exporter<'e, E>(
    executor: E,
    workspace_id: Id,
    user_id: Id,
) -> Result<(), ExportError>
where
    E: sqlx::Executor<'e, Database = sqlx::Any>,
{
    let actor = membership::actor(executor, workspace_id, user_id)
        .await?
        .ok_or(ExportError::NotFound)?;
    if actor.can(Permission::WorkspaceExport) {
        Ok(())
    } else {
        Err(ExportError::Forbidden)
    }
}

const SELECT_EXPORT: &str = "SELECT workspace_exports.id, workspace_exports.status, \
     workspace_exports.size_bytes, workspace_exports.created_at, workspace_exports.completed_at, \
     workspace_exports.expires_at, users.display_name FROM workspace_exports \
     LEFT JOIN users ON users.id = workspace_exports.requested_by";

fn export_from_row(row: &sqlx::any::AnyRow) -> Result<WorkspaceExport, ExportError> {
    Ok(WorkspaceExport {
        id: row
            .get::<String, _>("id")
            .parse()
            .map_err(|_| storage_error("invalid export id"))?,
        status: WorkspaceExportStatus::parse(row.get("status")),
        requested_by_name: row.get("display_name"),
        size_bytes: row
            .get::<Option<i64>, _>("size_bytes")
            .and_then(|size| u64::try_from(size).ok()),
        created_at: TimestampMillis::from_millis(row.get("created_at")),
        completed_at: row
            .get::<Option<i64>, _>("completed_at")
            .map(TimestampMillis::from_millis),
        expires_at: TimestampMillis::from_millis(row.get("expires_at")),
    })
}

impl WorkspaceExportService {
    #[must_use]
    pub fn new(
        database: Database,
        blobs: Arc<dyn BlobStore>,
        storage: ObjectStorage,
        local_root: impl Into<PathBuf>,
        public_origin: impl Into<String>,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                database,
                blobs,
                storage,
                local_root: local_root.into(),
                public_origin: public_origin.into(),
            }),
        }
    }

    fn database(&self) -> &Database {
        &self.inner.database
    }

    /// Queues an export of the workspace as `actor_id` (whose visibility it uses).
    pub async fn create(
        &self,
        workspace_id: Id,
        actor_id: Id,
        request_id: &str,
    ) -> Result<WorkspaceExport, ExportError> {
        let now = self.database().database_now().await?;
        let id = Id::new_v7();
        let mut tx = self.database().immediate_transaction().await?;
        require_exporter(&mut *tx, workspace_id, actor_id).await?;
        let active: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) FROM workspace_exports WHERE workspace_id = $1 AND status IN {ACTIVE}"
        )))
        .bind(workspace_id.to_string())
        .fetch_one(&mut *tx)
        .await?;
        if active > 0 {
            return Err(ExportError::InProgress);
        }
        sqlx::query(
            "INSERT INTO workspace_exports (id, workspace_id, requested_by, status, created_at, expires_at) \
             VALUES ($1, $2, $3, 'queued', $4, $5)",
        )
        .bind(id.to_string())
        .bind(workspace_id.to_string())
        .bind(actor_id.to_string())
        .bind(now.as_millis())
        .bind(now.as_millis() + EXPORT_TTL_MILLIS)
        .execute(&mut *tx)
        .await?;
        let job = Job::new(job_kind(), json!({"export_id": id.to_string()}), now)
            .with_workspace(workspace_id.to_string());
        JobStore::enqueue_in(&mut tx, &job)
            .await
            .map_err(storage_error)?;
        audit::record(
            &mut tx,
            workspace_id,
            Some(actor_id),
            "workspace.export_created",
            AuditOutcome::Success,
            "workspace_export",
            Some(id),
            request_id,
            json!({}),
            now,
        )
        .await?;
        let row = sqlx::query(sqlx::AssertSqlSafe(format!(
            "{SELECT_EXPORT} WHERE workspace_exports.id = $1"
        )))
        .bind(id.to_string())
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        export_from_row(&row)
    }

    pub async fn list(
        &self,
        workspace_id: Id,
        actor_id: Id,
    ) -> Result<Vec<WorkspaceExport>, ExportError> {
        let pool = self.database().pool();
        require_exporter(pool, workspace_id, actor_id).await?;
        let now = self.database().database_now().await?;
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "{SELECT_EXPORT} WHERE workspace_exports.workspace_id = $1 AND workspace_exports.expires_at > $2 \
             ORDER BY workspace_exports.created_at DESC, workspace_exports.id DESC LIMIT $3"
        )))
        .bind(workspace_id.to_string())
        .bind(now.as_millis())
        .bind(MAX_LISTED)
        .fetch_all(pool)
        .await?
        .iter()
        .map(export_from_row)
        .collect()
    }

    /// Opens a ready, unexpired export for an owner or admin and audits the download.
    pub async fn download(
        &self,
        workspace_id: Id,
        export_id: Id,
        actor_id: Id,
        request_id: &str,
    ) -> Result<ExportDownload, ExportError> {
        let pool = self.database().pool();
        require_exporter(pool, workspace_id, actor_id).await?;
        let now = self.database().database_now().await?;
        let row = sqlx::query(
            "SELECT workspace_exports.location, workspace_exports.size_bytes, workspace_exports.created_at, \
             workspaces.name FROM workspace_exports JOIN workspaces ON workspaces.id = workspace_exports.workspace_id \
             WHERE workspace_exports.id = $1 AND workspace_exports.workspace_id = $2 \
             AND workspace_exports.status = 'ready' AND workspace_exports.expires_at > $3",
        )
        .bind(export_id.to_string())
        .bind(workspace_id.to_string())
        .bind(now.as_millis())
        .fetch_optional(pool)
        .await?
        .ok_or(ExportError::NotFound)?;
        let location = row
            .get::<Option<String>, _>("location")
            .as_deref()
            .and_then(Location::parse)
            .ok_or(ExportError::NotFound)?;
        let reader = self
            .open(workspace_id, export_id, location)
            .await?
            .ok_or(ExportError::NotFound)?;
        let mut tx = self.database().immediate_transaction().await?;
        audit::record(
            &mut tx,
            workspace_id,
            Some(actor_id),
            "workspace.export_downloaded",
            AuditOutcome::Success,
            "workspace_export",
            Some(export_id),
            request_id,
            json!({}),
            now,
        )
        .await?;
        tx.commit().await?;
        let day: String = TimestampMillis::from_millis(row.get("created_at"))
            .to_string()
            .chars()
            .take(10)
            .collect();
        let name = sanitize_segment(row.get::<&str, _>("name"), STEM_MAX_CHARS, "Workspace");
        Ok(ExportDownload {
            reader,
            size_bytes: u64::try_from(row.get::<Option<i64>, _>("size_bytes").unwrap_or(0))
                .unwrap_or(0),
            file_name: format!("{name} export {day}.zip"),
        })
    }

    /// Runs the export job: writes the archive, stores it and marks the export ready. An export
    /// that is no longer active, or whose requester lost the permission, ends without an archive.
    pub async fn run(&self, export_id: Id, cancel: &CancellationToken) -> Result<(), ExportError> {
        let mut tx = self.database().immediate_transaction().await?;
        let row = sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE workspace_exports SET status = 'running' WHERE id = $1 AND status IN {ACTIVE} \
             RETURNING workspace_id, requested_by"
        )))
        .bind(export_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        tx.commit().await?;
        let Some(row) = row else {
            return Ok(());
        };
        let workspace: String = row.get("workspace_id");
        let workspace_id: Id = workspace
            .parse()
            .map_err(|_| storage_error("invalid workspace id"))?;
        let requester = row
            .get::<Option<String>, _>("requested_by")
            .and_then(|id| id.parse::<Id>().ok());
        let allowed = match requester {
            Some(actor_id) => {
                match require_exporter(self.database().pool(), workspace_id, actor_id).await {
                    Ok(()) => Some(actor_id),
                    Err(ExportError::NotFound | ExportError::Forbidden) => None,
                    Err(error) => return Err(error),
                }
            }
            None => None,
        };
        let Some(actor_id) = allowed else {
            return self.fail(export_id).await;
        };

        let key = storage_key(&workspace, &export_id.to_string());
        let partial = self.inner.local_root.join(format!("{key}.partial"));
        if let Some(parent) = partial.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(storage_error)?;
        }
        let now = self.database().database_now().await?;
        let source = archive::Source {
            database: self.database(),
            blobs: self.inner.blobs.as_ref(),
            workspace_id,
            actor_id,
            origin: &self.inner.public_origin,
            now,
            cancel,
        };
        let stored = match archive::write(&source, partial.clone()).await {
            Ok(size) => self
                .store(&workspace, &key, &partial)
                .await
                .map(|location| (location, size)),
            Err(error) => Err(error),
        };
        let _ = tokio::fs::remove_file(&partial).await;
        let (location, size) = stored?;

        let completed = self.database().database_now().await?;
        let mut tx = self.database().immediate_transaction().await?;
        let updated = sqlx::query(
            "UPDATE workspace_exports SET status = 'ready', location = $1, size_bytes = $2, \
             completed_at = $3, expires_at = $4 WHERE id = $5 AND status = 'running'",
        )
        .bind(location.as_str())
        .bind(i64::try_from(size).unwrap_or(i64::MAX))
        .bind(completed.as_millis())
        .bind(completed.as_millis() + EXPORT_TTL_MILLIS)
        .bind(export_id.to_string())
        .execute(&mut *tx)
        .await?
        .rows_affected();
        tx.commit().await?;
        if updated == 0 {
            self.delete_stored(&workspace, &key, location).await?;
        }
        Ok(())
    }

    /// Marks an active export failed (the job gave up).
    pub async fn fail(&self, export_id: Id) -> Result<(), ExportError> {
        let mut tx = self.database().immediate_transaction().await?;
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE workspace_exports SET status = 'failed' WHERE id = $1 AND status IN {ACTIVE}"
        )))
        .bind(export_id.to_string())
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Retention: fails stalled exports, then deletes expired exports with their archives. An
    /// archive that cannot be deleted now keeps its row for the next run. Returns the rows deleted.
    pub async fn expire(&self, now: TimestampMillis) -> Result<u64, ExportError> {
        let mut tx = self.database().immediate_transaction().await?;
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE workspace_exports SET status = 'failed' WHERE status IN {ACTIVE} AND created_at < $1"
        )))
        .bind(now.as_millis() - STALLED_AFTER_MILLIS)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        let expired = sqlx::query(
            "SELECT id, workspace_id, location FROM workspace_exports WHERE expires_at <= $1",
        )
        .bind(now.as_millis())
        .fetch_all(self.database().pool())
        .await?;
        let mut deleted = 0;
        for row in expired {
            let id: String = row.get("id");
            let workspace: String = row.get("workspace_id");
            let key = storage_key(&workspace, &id);
            let _ =
                tokio::fs::remove_file(self.inner.local_root.join(format!("{key}.partial"))).await;
            if let Some(location) = row
                .get::<Option<String>, _>("location")
                .as_deref()
                .and_then(Location::parse)
                && let Err(error) = self.delete_stored(&workspace, &key, location).await
            {
                tracing::warn!(%error, export_id = %id, "expired workspace export could not be deleted");
                continue;
            }
            let mut tx = self.database().immediate_transaction().await?;
            deleted += sqlx::query("DELETE FROM workspace_exports WHERE id = $1")
                .bind(&id)
                .execute(&mut *tx)
                .await?
                .rows_affected();
            tx.commit().await?;
        }
        Ok(deleted)
    }

    /// The tier that new attachment blobs of the workspace go to.
    async fn store(
        &self,
        workspace: &str,
        key: &str,
        file: &Path,
    ) -> Result<Location, ExportError> {
        let state = self.inner.storage.current();
        let (location, bucket) = match state
            .workspace_buckets
            .get(workspace)
            .filter(|own| own.active)
        {
            Some(own) => (Location::WorkspaceBucket, Some(own.bucket.clone())),
            None => match state.bucket.filter(|_| state.attachments) {
                Some(bucket) => (Location::Bucket, Some(bucket)),
                None => (Location::Local, None),
            },
        };
        match bucket {
            Some(bucket) => bucket.put_file(key, file).await.map_err(storage_error)?,
            None => tokio::fs::rename(file, self.inner.local_root.join(key))
                .await
                .map_err(storage_error)?,
        }
        Ok(location)
    }

    fn bucket(&self, workspace: &str, location: Location) -> Option<S3Bucket> {
        let state = self.inner.storage.current();
        match location {
            Location::Local => None,
            Location::Bucket => state.bucket,
            Location::WorkspaceBucket => state
                .workspace_buckets
                .get(workspace)
                .map(|own| own.bucket.clone()),
        }
    }

    async fn open(
        &self,
        workspace_id: Id,
        export_id: Id,
        location: Location,
    ) -> Result<Option<BlobReader>, ExportError> {
        let workspace = workspace_id.to_string();
        let key = storage_key(&workspace, &export_id.to_string());
        if location == Location::Local {
            return match tokio::fs::File::open(self.inner.local_root.join(&key)).await {
                Ok(file) => Ok(Some(Box::pin(file))),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(error) => Err(storage_error(error)),
            };
        }
        match self.bucket(&workspace, location) {
            Some(bucket) => bucket.get(&key).await.map_err(storage_error),
            None => Ok(None),
        }
    }

    async fn delete_stored(
        &self,
        workspace: &str,
        key: &str,
        location: Location,
    ) -> Result<(), ExportError> {
        if location == Location::Local {
            return match tokio::fs::remove_file(self.inner.local_root.join(key)).await {
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                    Err(storage_error(error))
                }
                _ => Ok(()),
            };
        }
        match self.bucket(workspace, location) {
            Some(bucket) => bucket.delete(key).await.map_err(storage_error),
            // The bucket was removed from the settings; its objects are out of Orbit's reach.
            None => Ok(()),
        }
    }
}

fn payload_export_id(payload: &Value) -> Option<Id> {
    payload.get("export_id")?.as_str()?.parse().ok()
}

/// Registers the `workspace.export` handler.
pub fn register_job(
    worker: Worker,
    service: WorkspaceExportService,
) -> Result<Worker, JobKindRegistrationError> {
    worker.with_handler(job_kind(), move |context| {
        let service = service.clone();
        async move {
            let Some(export_id) = payload_export_id(&context.job.payload) else {
                return Err(JobError::Permanent(
                    "invalid workspace.export payload".to_owned(),
                ));
            };
            let cancel = context.cancellation_token();
            match service.run(export_id, &cancel).await {
                Ok(()) => Ok(()),
                // Server shutdown: the job runs again later.
                Err(_) if cancel.is_cancelled() => {
                    Err(JobError::Retryable("workspace export stopped".to_owned()))
                }
                Err(error) => {
                    tracing::warn!(%error, %export_id, "workspace export failed");
                    if context.claim.attempt >= context.job.max_attempts {
                        service
                            .fail(export_id)
                            .await
                            .map_err(|error| JobError::Retryable(error.to_string()))?;
                        Err(JobError::Permanent(error.to_string()))
                    } else {
                        Err(JobError::Retryable(error.to_string()))
                    }
                }
            }
        }
    })
}
