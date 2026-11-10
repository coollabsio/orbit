//! Import from another Orbit instance: a member pastes the other instance's URL and an export
//! API token (or uploads an export ZIP), an `orbit.fetch` job starts the remote export, waits
//! for it, downloads the archive and reads its manifest and tree, the member picks what to
//! import, and an `orbit.import` job creates the teamspaces, projects, tasks, pages, channels
//! and messages in this workspace.
//!
//! State machine (`orbit_imports.status`), as for the Linear import:
//! `scanning` → `ready` → `queued` → `importing` → `completed`; any active state can become
//! `failed` or `cancelled`, and a `ready` import that is not started within 24 hours becomes
//! `expired`. While `scanning`, `phase` tells where the fetch is (`exporting`, `downloading`,
//! `reading`). The token is stored only as ciphertext under the app key, deleted as soon as the
//! archive is stored, and NULL in every final state (a CHECK enforces it). The archive is deleted
//! when the import reaches a final state.
//!
//! Every record the import creates is remembered in `orbit_import_links` by the source
//! workspace's id, the record's id there and a `scope`: `''` for full imports, the importer's
//! user id for personal imports. A full import reads and writes only scope `''`. A personal
//! import writes only its own scope and reads it to resume; it reads scope `''` only to leave
//! alone what a full import already created (counted as existing, nothing written into it).
//! The source workspace id comes from the archive, which a personal importer controls, so no
//! import may write into a record of another scope. Jobs carry only the import id; progress
//! lives in `orbit_import_items`, so a restarted job continues where it stopped.

mod archive;
mod chat;
pub mod client;
mod model;
mod pages;
mod tasks;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use orbit_domain::Permission;
use orbit_platform::{
    Backend, Database, Id, Job, JobError, JobKind, JobKindRegistrationError, JobQueue, JobStore,
    ObjectStorage, Row, TimestampMillis, UploadService, Worker,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;
use utoipa::ToSchema;

use self::archive::{Archive, ArchiveError};
use self::client::{ClientError, OrbitClient};
use crate::notion::import::clamp;
use crate::obsidian::archive::{ArchiveStore, Location};
use crate::repositories::attachments::AttachmentRepository;
use crate::repositories::chat::ChatRepository;
use crate::repositories::membership;
use crate::repositories::page_comments::PageCommentRepository;
use crate::repositories::page_files::PageFileRepository;
use crate::repositories::pages::PageRepository;
use crate::repositories::tasks::TaskRepository;
use crate::repositories::teamspaces::TeamspaceRepository;
use crate::secret_box::{decrypt_secret, encrypt_secret};
pub use crate::task_import::JobRun;
use crate::task_import::{
    Failure, backoff, ensure_writable, job_kind, parse_stored_id, retryable, stopped,
};
use crate::workspace_export::{ExportMode, WorkspaceExportStatus};

pub const FETCH_JOB: &str = "orbit.fetch";
pub const IMPORT_JOB: &str = "orbit.import";
/// Largest archive the import downloads or accepts as an upload.
pub const MAX_ARCHIVE_BYTES: u64 = 5 * 1024 * 1024 * 1024;
/// A fetched import waits this long for "start" before its archive is deleted.
pub const READY_TTL: Duration = Duration::from_secs(24 * 60 * 60);
/// The remote export may take this long before the fetch gives up.
const EXPORT_WAIT: Duration = Duration::from_secs(24 * 60 * 60);
/// An active import that made no progress for this long is failed by the cleanup.
const STALLED_AFTER: Duration = Duration::from_secs(24 * 60 * 60);
const MAX_FAILURES_LISTED: i64 = 200;
const MAX_UNMATCHED_LISTED: usize = 200;
const MAX_LISTED_IMPORTS: i64 = 20;
const MAX_SELECTED: usize = 10_000;
const ACTIVE: &str = "('scanning', 'queued', 'importing')";
const OPEN: &str = "('scanning', 'ready', 'queued', 'importing')";
const FINAL: &str = "('completed', 'failed', 'cancelled', 'expired')";

#[must_use]
pub fn fetch_job_kind() -> JobKind {
    job_kind(FETCH_JOB)
}

#[must_use]
pub fn import_job_kind() -> JobKind {
    job_kind(IMPORT_JOB)
}

/// Registers the `orbit.fetch` and `orbit.import` handlers.
pub fn register_jobs(
    worker: Worker,
    service: OrbitImportService,
) -> Result<Worker, JobKindRegistrationError> {
    let fetch = service.clone();
    worker
        .with_handler(fetch_job_kind(), move |context| {
            let service = fetch.clone();
            async move {
                let Some(import_id) = payload_import_id(&context.job.payload) else {
                    return Err(JobError::Permanent(
                        "invalid orbit.fetch payload".to_owned(),
                    ));
                };
                let run = JobRun {
                    cancel: context.cancellation_token(),
                    last_attempt: context.claim.attempt >= context.job.max_attempts,
                };
                let result = service.run_fetch(import_id, &run).await;
                service.after_attempt(import_id, &run, result).await
            }
        })?
        .with_handler(import_job_kind(), move |context| {
            let service = service.clone();
            async move {
                let Some(import_id) = payload_import_id(&context.job.payload) else {
                    return Err(JobError::Permanent(
                        "invalid orbit.import payload".to_owned(),
                    ));
                };
                let run = JobRun {
                    cancel: context.cancellation_token(),
                    last_attempt: context.claim.attempt >= context.job.max_attempts,
                };
                let result = service.run_import(import_id, &run).await;
                service.after_attempt(import_id, &run, result).await
            }
        })
}

fn payload_import_id(payload: &Value) -> Option<Id> {
    payload.get("import_id")?.as_str()?.parse().ok()
}

#[derive(Clone, Debug)]
pub struct OrbitImportSettings {
    /// The server app key; without it no import can store its token.
    pub app_key: Option<[u8; 32]>,
    /// Largest file inside the archive that is imported (the upload limit for one file).
    pub max_file_bytes: u64,
    /// Largest archive downloaded or uploaded.
    pub max_archive_bytes: u64,
    /// Development only: lets the instance URL be `http://127.0.0.1:<port>`.
    pub allow_local: bool,
}

#[derive(Clone)]
pub struct OrbitImportService {
    inner: Arc<Inner>,
}

struct Inner {
    database: Database,
    tasks: TaskRepository,
    attachments: AttachmentRepository,
    pages: PageRepository,
    page_files: PageFileRepository,
    page_comments: PageCommentRepository,
    teamspaces: TeamspaceRepository,
    chat: ChatRepository,
    uploads: UploadService,
    jobs: JobQueue,
    archives: ArchiveStore,
    settings: OrbitImportSettings,
}

/// Errors of the import API.
#[derive(Debug, Error)]
pub enum OrbitImportError {
    #[error("import not found")]
    NotFound,
    #[error("the app key is not configured")]
    AppKeyMissing,
    #[error("the instance URL is invalid")]
    UrlInvalid,
    #[error("the API token is invalid")]
    TokenInvalid,
    #[error("the API token has no export scope")]
    ScopeMissing,
    #[error("the other Orbit instance is unreachable")]
    Unreachable,
    #[error("the archive is too large")]
    ExportTooLarge,
    #[error("the archive is not a supported workspace export")]
    FormatUnsupported,
    #[error("another import is in progress")]
    InProgress,
    #[error("the import is not in a state that allows this")]
    StateConflict,
    /// A full import needs an owner or admin of the target workspace.
    #[error("the member may not run this import")]
    Forbidden,
    #[error("invalid input: {field}")]
    Invalid { field: &'static str },
    #[error("import storage is unavailable")]
    Unavailable(String),
}

impl From<sqlx::Error> for OrbitImportError {
    fn from(error: sqlx::Error) -> Self {
        Self::Unavailable(error.to_string())
    }
}

/// `orbit_imports.status`.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum OrbitImportStatus {
    /// The fetch job is exporting, downloading or reading the archive.
    Scanning,
    /// The tree is ready; waiting for `start` (the archive is deleted after 24 hours).
    Ready,
    Queued,
    Importing,
    Completed,
    Failed,
    Cancelled,
    Expired,
}

impl OrbitImportStatus {
    fn parse(value: &str) -> Result<Self, OrbitImportError> {
        Ok(match value {
            "scanning" => Self::Scanning,
            "ready" => Self::Ready,
            "queued" => Self::Queued,
            "importing" => Self::Importing,
            "completed" => Self::Completed,
            "failed" => Self::Failed,
            "cancelled" => Self::Cancelled,
            "expired" => Self::Expired,
            _ => {
                return Err(OrbitImportError::Unavailable(
                    "unknown import status".to_owned(),
                ));
            }
        })
    }
}

/// Where a `scanning` import is.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum OrbitImportPhase {
    /// Waiting for the other instance to write the export.
    Exporting,
    Downloading,
    /// Checking the archive and building the tree.
    Reading,
}

impl OrbitImportPhase {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Exporting => "exporting",
            Self::Downloading => "downloading",
            Self::Reading => "reading",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "exporting" => Some(Self::Exporting),
            "downloading" => Some(Self::Downloading),
            "reading" => Some(Self::Reading),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum OrbitImportSourceKind {
    /// Pulled from the other instance with an export API token.
    Url,
    /// An uploaded export ZIP.
    Upload,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct OrbitImportSource {
    pub kind: OrbitImportSourceKind,
    /// The other instance's origin; `null` for an upload.
    #[schema(required = true)]
    pub url: Option<String>,
    /// The source workspace's name, once known.
    #[schema(required = true)]
    pub workspace_name: Option<String>,
    /// Who made the export, once known.
    #[schema(required = true)]
    pub exported_by_email: Option<String>,
}

#[derive(Clone, Copy, Debug, Serialize, ToSchema)]
pub struct OrbitImportProgress {
    /// Items planned (teamspaces, projects, tasks, pages, channels).
    pub total: u64,
    pub done: u64,
    pub failed: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct OrbitImportTreeProject {
    pub id: String,
    pub key: String,
    pub name: String,
    pub task_count: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct OrbitImportTreeTeamspace {
    pub id: String,
    pub name: String,
    pub page_count: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct OrbitImportTreeChannel {
    pub id: String,
    /// `public`, `private` or `dm`.
    pub kind: String,
    /// A DM is named after its members.
    pub name: String,
    pub message_count: u64,
}

/// What the archive holds, for the selection.
#[derive(Clone, Debug, Default, Deserialize, Serialize, ToSchema)]
pub struct OrbitImportTree {
    pub projects: Vec<OrbitImportTreeProject>,
    pub teamspaces: Vec<OrbitImportTreeTeamspace>,
    /// Private pages of the member who made the export.
    pub private_page_count: u64,
    pub channels: Vec<OrbitImportTreeChannel>,
}

/// What to import. Personal mode accepts only `include_private_pages` and `channel_ids`.
#[derive(Clone, Debug, Default, Deserialize, Serialize, ToSchema)]
#[serde(default, deny_unknown_fields)]
pub struct OrbitImportSelection {
    pub project_ids: Vec<String>,
    pub teamspace_ids: Vec<String>,
    pub include_private_pages: bool,
    pub channel_ids: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, ToSchema)]
#[serde(default)]
pub struct OrbitImportCreated {
    pub projects: u64,
    pub tasks: u64,
    pub pages: u64,
    pub channels: u64,
    pub messages: u64,
    pub files: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
pub struct OrbitImportUnmatchedUser {
    pub name: String,
    #[schema(required = true)]
    pub email: Option<String>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct OrbitImportFailure {
    /// `teamspace`, `project`, `task`, `page` or `channel`.
    pub kind: String,
    pub title: String,
    pub error: String,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct OrbitImportReport {
    pub created: OrbitImportCreated,
    /// Records an earlier import of the same source workspace had already created.
    pub skipped_existing: u64,
    /// Source members without an Orbit member of the same email here; their tasks are
    /// unassigned and their comments and messages are written by the importer with a header.
    pub unmatched_users: Vec<OrbitImportUnmatchedUser>,
    /// The first 200 items that could not be imported.
    pub failures: Vec<OrbitImportFailure>,
    /// A full import never holds other members' private pages, private channels or DMs: each
    /// member imports them with their own personal token.
    pub members_must_import_private: bool,
}

/// The report's counts as `orbit_imports.report_json` keeps them.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
struct StoredReport {
    created: OrbitImportCreated,
    skipped_existing: u64,
    unmatched_users: Vec<OrbitImportUnmatchedUser>,
}

/// An Orbit import. Never contains the token.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct OrbitImport {
    #[schema(value_type = String)]
    pub id: Id,
    /// Known from the token's scope (URL) or the manifest (upload, once read).
    #[schema(required = true)]
    pub mode: Option<ExportMode>,
    pub source: OrbitImportSource,
    pub status: OrbitImportStatus,
    #[schema(required = true)]
    pub phase: Option<OrbitImportPhase>,
    pub progress: OrbitImportProgress,
    #[schema(required = true)]
    pub error: Option<String>,
    /// Only in `GET …/{id}?include=tree`, once ready.
    #[schema(required = true)]
    pub tree: Option<OrbitImportTree>,
    /// Set once the import was started.
    #[schema(required = true)]
    pub report: Option<OrbitImportReport>,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: TimestampMillis,
    #[schema(value_type = String, format = DateTime)]
    pub updated_at: TimestampMillis,
}

/// An upload being received: the route writes the file to `path`, then calls
/// [`OrbitImportService::create_upload`].
pub struct Upload {
    pub id: Id,
    pub path: std::path::PathBuf,
}

/// One planned item.
#[derive(Clone, Debug)]
struct Item {
    kind: &'static str,
    source_id: String,
    parent_id: Option<String>,
    path: String,
    title: String,
    orbit_id: Option<Id>,
    state: String,
}

const ITEM_KINDS: [&str; 5] = ["teamspace", "project", "task", "page", "channel"];

fn item_kind(value: &str) -> Option<&'static str> {
    ITEM_KINDS.into_iter().find(|kind| *kind == value)
}

/// A source member as `workspace.json` lists them.
#[derive(Clone, Debug)]
struct SourcePerson {
    name: String,
    email: Option<String>,
}

/// Shared state of one import run.
struct Run {
    import_id: Id,
    workspace_id: Id,
    user_id: Id,
    source_workspace_id: String,
    /// The `orbit_import_links` scope this import reads and writes, see [`link_scope`].
    link_scope: String,
    mode: ExportMode,
    selection: OrbitImportSelection,
    job: JobRun,
    request_id: String,
    archive: Arc<Archive>,
    report: StoredReport,
    /// Source user id → name and email.
    people: HashMap<String, SourcePerson>,
    /// Lowercase email → member id here.
    members: HashMap<String, Id>,
    unmatched_seen: HashSet<String>,
}

impl Run {
    /// The member here with the same email as source user `id`; records an unmatched person.
    fn member(&mut self, id: &str, fallback_name: Option<&str>) -> Option<Id> {
        let person = self.people.get(id).cloned();
        let matched = person
            .as_ref()
            .and_then(|person| person.email.as_deref())
            .and_then(|email| self.members.get(&email.trim().to_lowercase()))
            .copied();
        if matched.is_none()
            && self.unmatched_seen.insert(id.to_owned())
            && self.report.unmatched_users.len() < MAX_UNMATCHED_LISTED
        {
            self.report.unmatched_users.push(OrbitImportUnmatchedUser {
                name: person
                    .as_ref()
                    .map(|person| person.name.clone())
                    .or_else(|| fallback_name.map(str::to_owned))
                    .unwrap_or_else(|| "Unknown member".to_owned()),
                email: person.and_then(|person| person.email),
            });
        }
        matched
    }

    /// The name of source user `id`.
    fn name_of(&self, id: &str, fallback: Option<&str>) -> String {
        self.people
            .get(id)
            .map(|person| person.name.clone())
            .or_else(|| fallback.map(str::to_owned))
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| "Unknown member".to_owned())
    }
}

impl From<ArchiveError> for Failure {
    fn from(error: ArchiveError) -> Self {
        Self::Fatal(error.to_string())
    }
}

impl From<ClientError> for Failure {
    fn from(error: ClientError) -> Self {
        match error {
            ClientError::Unreachable(_) | ClientError::NotReady => {
                Self::Transient(error.to_string())
            }
            ClientError::TokenInvalid => {
                Self::Fatal("The API token was revoked or is no longer valid.".to_owned())
            }
            ClientError::ScopeMissing => {
                Self::Fatal("The API token lost its export scope.".to_owned())
            }
            ClientError::Forbidden => Self::Fatal(
                "The token's creator may no longer export the source workspace.".to_owned(),
            ),
            other => Self::Fatal(other.to_string()),
        }
    }
}

impl OrbitImportService {
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        database: Database,
        uploads: UploadService,
        storage: ObjectStorage,
        local_root: impl Into<std::path::PathBuf>,
        settings: OrbitImportSettings,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                tasks: TaskRepository::new(database.clone()),
                attachments: AttachmentRepository::new(database.clone(), uploads.clone()),
                pages: PageRepository::new(database.clone()),
                page_files: PageFileRepository::new(database.clone(), uploads.clone()),
                page_comments: PageCommentRepository::new(database.clone()),
                teamspaces: TeamspaceRepository::new(database.clone()),
                chat: ChatRepository::new(database.clone(), uploads.clone()),
                jobs: JobQueue::new(JobStore::new(database.clone())),
                archives: ArchiveStore::new(storage, local_root),
                database,
                uploads,
                settings,
            }),
        }
    }

    fn database(&self) -> &Database {
        &self.inner.database
    }

    /// The largest upload the route accepts.
    #[must_use]
    pub fn max_archive_bytes(&self) -> u64 {
        self.inner.settings.max_archive_bytes
    }

    /// Validates the token with the other instance (`GET /me`), reads the export mode from its
    /// scope, checks that the member may run an import of that mode here, stores the token
    /// encrypted and queues the fetch. A member has at most one active Orbit import.
    pub async fn create(
        &self,
        workspace_id: Id,
        user_id: Id,
        url: &str,
        token: &str,
    ) -> Result<OrbitImport, OrbitImportError> {
        self.require_importer(workspace_id, user_id, None).await?;
        let app_key = self
            .inner
            .settings
            .app_key
            .ok_or(OrbitImportError::AppKeyMissing)?;
        let allow_local = self.inner.settings.allow_local;
        let origin =
            client::parse_origin(url, allow_local).map_err(|_| OrbitImportError::UrlInvalid)?;
        let client = OrbitClient::new(origin.clone(), token, allow_local)
            .map_err(|_| OrbitImportError::TokenInvalid)?;
        let now = TimestampMillis::now();
        self.expire_stale(now).await?;
        if has_active_import(self.database().pool(), user_id).await? {
            return Err(OrbitImportError::InProgress);
        }
        let me = match client.me().await {
            Ok(me) => me,
            Err(ClientError::TokenInvalid) => return Err(OrbitImportError::TokenInvalid),
            Err(ClientError::Unreachable(error)) => {
                tracing::warn!(error = %error, "Orbit import token validation failed");
                return Err(OrbitImportError::Unreachable);
            }
            Err(error) => {
                tracing::warn!(error = %error, "Orbit import token validation refused");
                return Err(OrbitImportError::TokenInvalid);
            }
        };
        let mode = me.export_mode().ok_or(OrbitImportError::ScopeMissing)?;
        if me.service_account.is_some() {
            return Err(OrbitImportError::ScopeMissing);
        }
        self.require_importer(workspace_id, user_id, Some(mode))
            .await?;
        let ciphertext = encrypt_secret(&app_key, token.trim())
            .map_err(|()| OrbitImportError::Unavailable("token encryption failed".to_owned()))?;

        let id = Id::new_v7();
        let mut tx = self.database().immediate_transaction().await?;
        if has_active_import(&mut *tx, user_id).await? {
            return Err(OrbitImportError::InProgress);
        }
        self.cancel_ready(&mut tx, workspace_id, user_id, now)
            .await?;
        sqlx::query(
            "INSERT INTO orbit_imports (id, workspace_id, user_id, status, phase, mode, source_kind, \
             source_url, source_workspace_name, exported_by_email, token_ciphertext, created_at, updated_at) \
             VALUES ($1, $2, $3, 'scanning', 'exporting', $4, 'url', $5, $6, $7, $8, $9, $10)",
        )
        .bind(id.to_string())
        .bind(workspace_id.to_string())
        .bind(user_id.to_string())
        .bind(mode_str(mode))
        .bind(origin.as_str().trim_end_matches('/'))
        .bind(clamp(&me.workspace.name, 200, 800))
        .bind(me.user.as_ref().map(|user| clamp(&user.email, 320, 1_000)))
        .bind(ciphertext)
        .bind(now.as_millis())
        .bind(now.as_millis())
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        if let Err(error) = self.enqueue(fetch_job_kind(), id, workspace_id).await {
            self.finish(id, "failed", Some("The fetch could not be queued."))
                .await?;
            return Err(error);
        }
        self.get(workspace_id, user_id, id, false).await
    }

    /// Checks that the member may upload now and returns where to write the upload.
    pub async fn prepare_upload(
        &self,
        workspace_id: Id,
        user_id: Id,
    ) -> Result<Upload, OrbitImportError> {
        self.require_importer(workspace_id, user_id, None).await?;
        self.expire_stale(TimestampMillis::now()).await?;
        if has_active_import(self.database().pool(), user_id).await? {
            return Err(OrbitImportError::InProgress);
        }
        let id = Id::new_v7();
        let path = self
            .inner
            .archives
            .partial_path(workspace_id, id)
            .await
            .map_err(|error| OrbitImportError::Unavailable(error.to_string()))?;
        Ok(Upload { id, path })
    }

    /// Stores a received upload and queues the fetch job, which goes straight to reading the
    /// archive. The upload file is gone afterwards in every case.
    pub async fn create_upload(
        &self,
        workspace_id: Id,
        user_id: Id,
        upload: Upload,
        size: u64,
    ) -> Result<OrbitImport, OrbitImportError> {
        let result = self
            .store_upload(workspace_id, user_id, &upload, size)
            .await;
        let _ = tokio::fs::remove_file(&upload.path).await;
        let id = result?;
        if let Err(error) = self.enqueue(fetch_job_kind(), id, workspace_id).await {
            self.finish(id, "failed", Some("The scan could not be queued."))
                .await?;
            return Err(error);
        }
        self.get(workspace_id, user_id, id, false).await
    }

    async fn store_upload(
        &self,
        workspace_id: Id,
        user_id: Id,
        upload: &Upload,
        size: u64,
    ) -> Result<Id, OrbitImportError> {
        if !is_zip(&upload.path).await {
            return Err(OrbitImportError::FormatUnsupported);
        }
        let location = self
            .inner
            .archives
            .store(workspace_id, upload.id, &upload.path)
            .await
            .map_err(OrbitImportError::Unavailable)?;
        let now = TimestampMillis::now();
        let inserted: Result<(), OrbitImportError> = async {
            let mut tx = self.database().immediate_transaction().await?;
            if has_active_import(&mut *tx, user_id).await? {
                return Err(OrbitImportError::InProgress);
            }
            self.cancel_ready(&mut tx, workspace_id, user_id, now)
                .await?;
            sqlx::query(
                "INSERT INTO orbit_imports (id, workspace_id, user_id, status, phase, source_kind, \
                 archive_location, archive_bytes, created_at, updated_at) \
                 VALUES ($1, $2, $3, 'scanning', 'reading', 'upload', $4, $5, $6, $7)",
            )
            .bind(upload.id.to_string())
            .bind(workspace_id.to_string())
            .bind(user_id.to_string())
            .bind(location.as_str())
            .bind(i64::try_from(size).unwrap_or(i64::MAX))
            .bind(now.as_millis())
            .bind(now.as_millis())
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;
            Ok(())
        }
        .await;
        if inserted.is_err() {
            let _ = self
                .inner
                .archives
                .delete(workspace_id, upload.id, location)
                .await;
        }
        inserted.map(|()| upload.id)
    }

    /// The member's unstarted imports in this workspace are cancelled when a new one starts.
    async fn cancel_ready(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Any>,
        workspace_id: Id,
        user_id: Id,
        now: TimestampMillis,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE orbit_imports SET status = 'cancelled', token_ciphertext = NULL, updated_at = $1 \
             WHERE user_id = $2 AND workspace_id = $3 AND status = 'ready'",
        )
        .bind(now.as_millis())
        .bind(user_id.to_string())
        .bind(workspace_id.to_string())
        .execute(&mut **tx)
        .await?;
        Ok(())
    }

    /// The member's recent imports in this workspace, newest first (no trees).
    pub async fn list(
        &self,
        workspace_id: Id,
        user_id: Id,
    ) -> Result<Vec<OrbitImport>, OrbitImportError> {
        self.require_importer(workspace_id, user_id, None).await?;
        self.expire_rows(TimestampMillis::now(), Some((workspace_id, user_id)))
            .await?;
        let ids: Vec<String> = sqlx::query_scalar(
            "SELECT id FROM orbit_imports WHERE workspace_id = $1 AND user_id = $2 \
             ORDER BY created_at DESC, id DESC LIMIT $3",
        )
        .bind(workspace_id.to_string())
        .bind(user_id.to_string())
        .bind(MAX_LISTED_IMPORTS)
        .fetch_all(self.database().pool())
        .await?;
        let mut imports = Vec::with_capacity(ids.len());
        for id in ids {
            let id = id
                .parse()
                .map_err(|_| OrbitImportError::Unavailable("invalid import id".to_owned()))?;
            imports.push(self.load_view(workspace_id, user_id, id, false).await?);
        }
        Ok(imports)
    }

    /// One of the member's imports; the tree only `with_tree`. Other members (admins included)
    /// get `NotFound`.
    pub async fn get(
        &self,
        workspace_id: Id,
        user_id: Id,
        import_id: Id,
        with_tree: bool,
    ) -> Result<OrbitImport, OrbitImportError> {
        self.require_importer(workspace_id, user_id, None).await?;
        self.expire_rows(TimestampMillis::now(), Some((workspace_id, user_id)))
            .await?;
        self.load_view(workspace_id, user_id, import_id, with_tree)
            .await
    }

    /// Stores the selection and queues the import job. Every id must be in the tree, and a
    /// personal import selects only private pages and channels.
    pub async fn start(
        &self,
        workspace_id: Id,
        user_id: Id,
        import_id: Id,
        selection: OrbitImportSelection,
    ) -> Result<OrbitImport, OrbitImportError> {
        self.require_importer(workspace_id, user_id, None).await?;
        self.expire_stale(TimestampMillis::now()).await?;
        let row = sqlx::query(
            "SELECT status, mode, tree_json FROM orbit_imports WHERE id = $1 AND workspace_id = $2 AND user_id = $3",
        )
        .bind(import_id.to_string())
        .bind(workspace_id.to_string())
        .bind(user_id.to_string())
        .fetch_optional(self.database().pool())
        .await?
        .ok_or(OrbitImportError::NotFound)?;
        if row.get::<String, _>("status") != "ready" {
            return Err(OrbitImportError::StateConflict);
        }
        let mode = parse_mode(row.get::<Option<String>, _>("mode").as_deref())
            .ok_or(OrbitImportError::StateConflict)?;
        self.require_importer(workspace_id, user_id, Some(mode))
            .await?;
        let tree: OrbitImportTree = serde_json::from_str(
            &row.get::<Option<String>, _>("tree_json")
                .unwrap_or_default(),
        )
        .map_err(|_| OrbitImportError::Unavailable("invalid stored tree".to_owned()))?;
        let selection = checked_selection(selection, &tree, mode)?;

        let now = TimestampMillis::now();
        let mut tx = self.database().immediate_transaction().await?;
        let active: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) FROM orbit_imports WHERE user_id = $1 AND id <> $2 AND status IN {ACTIVE}"
        )))
        .bind(user_id.to_string())
        .bind(import_id.to_string())
        .fetch_one(&mut *tx)
        .await?;
        if active > 0 {
            return Err(OrbitImportError::InProgress);
        }
        let updated = sqlx::query(
            "UPDATE orbit_imports SET status = 'queued', selection_json = $1, total = 0, done = 0, \
             failed = 0, report_json = $2, updated_at = $3 WHERE id = $4 AND status = 'ready'",
        )
        .bind(serde_json::to_string(&selection).unwrap_or_default())
        .bind(serde_json::to_string(&StoredReport::default()).unwrap_or_default())
        .bind(now.as_millis())
        .bind(import_id.to_string())
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if updated != 1 {
            return Err(OrbitImportError::StateConflict);
        }
        tx.commit().await?;
        if let Err(error) = self
            .enqueue(import_job_kind(), import_id, workspace_id)
            .await
        {
            self.finish(import_id, "failed", Some("The import could not be queued."))
                .await?;
            return Err(error);
        }
        self.get(workspace_id, user_id, import_id, false).await
    }

    /// Cancels an open import, deletes its token and archive. A running job stops before its
    /// next item; what it already imported stays.
    pub async fn cancel(
        &self,
        workspace_id: Id,
        user_id: Id,
        import_id: Id,
    ) -> Result<OrbitImport, OrbitImportError> {
        self.require_importer(workspace_id, user_id, None).await?;
        let updated = sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE orbit_imports SET status = 'cancelled', token_ciphertext = NULL, updated_at = $1 \
             WHERE id = $2 AND workspace_id = $3 AND user_id = $4 AND status IN {OPEN}"
        )))
        .bind(TimestampMillis::now().as_millis())
        .bind(import_id.to_string())
        .bind(workspace_id.to_string())
        .bind(user_id.to_string())
        .execute(self.database().pool())
        .await?
        .rows_affected();
        if updated == 0 {
            self.load_view(workspace_id, user_id, import_id, false)
                .await?;
            return Err(OrbitImportError::StateConflict);
        }
        self.release_final_archives().await?;
        self.load_view(workspace_id, user_id, import_id, false)
            .await
    }

    /// Expires fetched imports that were not started within 24 hours, fails active imports that
    /// made no progress for 24 hours, and deletes the archives of final imports. Returns the
    /// rows changed.
    pub async fn expire_stale(&self, now: TimestampMillis) -> Result<u64, OrbitImportError> {
        let changed = self.expire_rows(now, None).await?;
        self.release_final_archives().await?;
        Ok(changed)
    }

    /// The state changes of [`Self::expire_stale`], for all imports or, on the polled read
    /// paths, only for one member's imports in one workspace (`member`). Leaves the archives to
    /// the cleanup.
    async fn expire_rows(
        &self,
        now: TimestampMillis,
        member: Option<(Id, Id)>,
    ) -> Result<u64, OrbitImportError> {
        // The member filter, with its two placeholders numbered from `first`.
        let only = |first: usize| match member {
            Some(_) => format!(
                " AND orbit_imports.workspace_id = ${first} AND orbit_imports.user_id = ${}",
                first + 1
            ),
            None => String::new(),
        };
        let bind_member =
            |query: sqlx::query::Query<'static, sqlx::Any, sqlx::any::AnyArguments>| match member {
                Some((workspace_id, user_id)) => query
                    .bind(workspace_id.to_string())
                    .bind(user_id.to_string()),
                None => query,
            };
        let ready_cutoff = now.as_millis() - READY_TTL.as_millis() as i64;
        let stalled_cutoff = now.as_millis() - STALLED_AFTER.as_millis() as i64;
        let expired = bind_member(
            sqlx::query(sqlx::AssertSqlSafe(format!(
                "UPDATE orbit_imports SET status = 'expired', token_ciphertext = NULL, \
                 error = 'The import was not started within 24 hours.', updated_at = $1 \
                 WHERE status = 'ready' AND updated_at < $2{}",
                only(3)
            )))
            .bind(now.as_millis())
            .bind(ready_cutoff),
        )
        .execute(self.database().pool())
        .await?
        .rows_affected();
        let stalled = bind_member(
            sqlx::query(sqlx::AssertSqlSafe(format!(
                "UPDATE orbit_imports SET status = 'failed', token_ciphertext = NULL, \
                 error = 'The import stopped making progress.', updated_at = $1 \
                 WHERE status IN {ACTIVE} AND updated_at < $2{}",
                only(3)
            )))
            .bind(now.as_millis())
            .bind(stalled_cutoff),
        )
        .execute(self.database().pool())
        .await?
        .rows_affected();
        let import_id_of_job = match self.database().backend() {
            Backend::Sqlite => "json_extract(jobs.payload_json, '$.import_id')",
            Backend::Postgres => "(jobs.payload_json::jsonb ->> 'import_id')",
        };
        let dead = bind_member(
            sqlx::query(sqlx::AssertSqlSafe(format!(
                "UPDATE orbit_imports SET status = 'failed', token_ciphertext = NULL, \
                 error = 'The import stopped after repeated errors.', updated_at = $1 \
                 WHERE status IN {ACTIVE}{} \
                 AND EXISTS (SELECT 1 FROM jobs WHERE jobs.kind IN ('{FETCH_JOB}', '{IMPORT_JOB}') \
                     AND jobs.state = 'dead' AND {import_id_of_job} = orbit_imports.id) \
                 AND NOT EXISTS (SELECT 1 FROM jobs WHERE jobs.kind IN ('{FETCH_JOB}', '{IMPORT_JOB}') \
                     AND jobs.state IN ('queued', 'running') AND {import_id_of_job} = orbit_imports.id)",
                only(2)
            )))
            .bind(now.as_millis()),
        )
        .execute(self.database().pool())
        .await?
        .rows_affected();
        Ok(expired + stalled + dead)
    }

    /// Deletes the archives of imports in a final state (retried on the next cleanup when
    /// storage fails).
    async fn release_final_archives(&self) -> Result<(), OrbitImportError> {
        let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT id, workspace_id, archive_location FROM orbit_imports \
             WHERE status IN {FINAL} AND archive_location IS NOT NULL"
        )))
        .fetch_all(self.database().pool())
        .await?;
        for row in rows {
            let id: String = row.get("id");
            let (Ok(import_id), Ok(workspace_id)) = (
                id.parse::<Id>(),
                row.get::<String, _>("workspace_id").parse::<Id>(),
            ) else {
                continue;
            };
            let Some(location) = Location::parse(row.get::<&str, _>("archive_location")) else {
                continue;
            };
            if let Err(error) = self
                .inner
                .archives
                .delete(workspace_id, import_id, location)
                .await
            {
                tracing::warn!(%error, import_id = %id, "Orbit import archive could not be deleted");
                continue;
            }
            sqlx::query("UPDATE orbit_imports SET archive_location = NULL WHERE id = $1")
                .bind(&id)
                .execute(self.database().pool())
                .await?;
        }
        Ok(())
    }

    async fn after_attempt(
        &self,
        import_id: Id,
        run: &JobRun,
        result: Result<(), JobError>,
    ) -> Result<(), JobError> {
        if let Err(error) = &result
            && run.last_attempt
            && !run.cancel.is_cancelled()
            && let Err(cleanup) = self
                .finish(
                    import_id,
                    "failed",
                    Some("The import stopped after repeated errors."),
                )
                .await
        {
            tracing::warn!(import_id = %import_id, error = %error, cleanup = %cleanup, "Orbit import could not be finalized");
        }
        result
    }

    /// The `orbit.fetch` job: starts and waits for the remote export, downloads it, reads the
    /// manifest and builds the tree.
    pub async fn run_fetch(&self, import_id: Id, run: &JobRun) -> Result<(), JobError> {
        match self.fetch(import_id, run).await {
            Ok(()) => Ok(()),
            Err(failure) => self.job_failure(import_id, failure, run).await,
        }
    }

    pub async fn run_import(&self, import_id: Id, run: &JobRun) -> Result<(), JobError> {
        match self.import(import_id, run).await {
            Ok(()) => Ok(()),
            Err(failure) => self.job_failure(import_id, failure, run).await,
        }
    }

    async fn job_failure(
        &self,
        import_id: Id,
        failure: Failure,
        run: &JobRun,
    ) -> Result<(), JobError> {
        match failure {
            Failure::Interrupted => Err(JobError::Retryable("interrupted by shutdown".to_owned())),
            Failure::Transient(message) if !run.last_attempt && !run.cancel.is_cancelled() => {
                tracing::warn!(import_id = %import_id, error = %message, "Orbit import will retry");
                Err(JobError::Retryable(message))
            }
            Failure::Transient(message) if run.cancel.is_cancelled() => {
                Err(JobError::Retryable(message))
            }
            Failure::Transient(message) | Failure::Fatal(message) | Failure::Item(message) => {
                self.finish(import_id, "failed", Some(&message))
                    .await
                    .map_err(retryable)?;
                Ok(())
            }
        }
    }

    async fn fetch(&self, import_id: Id, run: &JobRun) -> Result<(), Failure> {
        let Some(row) = sqlx::query(
            "SELECT workspace_id, user_id, status, mode, source_kind, source_url, token_ciphertext, \
             remote_export_id, archive_location, created_at FROM orbit_imports WHERE id = $1",
        )
        .bind(import_id.to_string())
        .fetch_optional(self.database().pool())
        .await?
        else {
            return Ok(());
        };
        if row.get::<String, _>("status") != "scanning" {
            return Ok(());
        }
        let workspace_id = parse_stored_id(row.get("workspace_id"))?;
        let user_id = parse_stored_id(row.get("user_id"))?;
        let token_mode = parse_mode(row.get::<Option<String>, _>("mode").as_deref());
        let mut location = row
            .get::<Option<String>, _>("archive_location")
            .as_deref()
            .and_then(Location::parse);

        if location.is_none() {
            let mode = token_mode
                .ok_or_else(|| Failure::Fatal("Stored import data is invalid.".to_owned()))?;
            let client = self
                .client_for(
                    row.get::<Option<String>, _>("source_url").as_deref(),
                    row.get("token_ciphertext"),
                )
                .map_err(Failure::Fatal)?;
            let export_id = match row.get::<Option<String>, _>("remote_export_id") {
                Some(id) => id,
                None => {
                    let export = client.start_export(mode).await?;
                    if export.mode != mode {
                        return Err(Failure::Fatal(
                            "The other instance started an export of another mode.".to_owned(),
                        ));
                    }
                    sqlx::query(
                        "UPDATE orbit_imports SET remote_export_id = $1, updated_at = $2 WHERE id = $3",
                    )
                    .bind(&export.id)
                    .bind(TimestampMillis::now().as_millis())
                    .bind(import_id.to_string())
                    .execute(self.database().pool())
                    .await?;
                    export.id
                }
            };
            let deadline = TimestampMillis::from_millis(
                row.get::<i64, _>("created_at") + EXPORT_WAIT.as_millis() as i64,
            );
            let mut attempt = 0u32;
            loop {
                if !self.is_scanning(import_id).await? {
                    return stopped(run);
                }
                let export = client.export(&export_id).await?;
                match export.status {
                    WorkspaceExportStatus::Ready => break,
                    WorkspaceExportStatus::Failed => {
                        return Err(Failure::Fatal(
                            "The export failed on the other Orbit instance.".to_owned(),
                        ));
                    }
                    WorkspaceExportStatus::Queued | WorkspaceExportStatus::Running => {}
                }
                if TimestampMillis::now().as_millis() > deadline.as_millis() {
                    return Err(Failure::Fatal(
                        "The other Orbit instance did not finish the export within 24 hours."
                            .to_owned(),
                    ));
                }
                attempt = attempt.saturating_add(1);
                let wait = backoff(Duration::from_secs(3), Duration::from_secs(60), attempt);
                tokio::select! {
                    () = run.cancel.cancelled() => return Err(Failure::Interrupted),
                    () = tokio::time::sleep(wait) => {}
                }
                self.touch(import_id).await?;
            }

            self.set_phase(import_id, OrbitImportPhase::Downloading)
                .await?;
            let partial = self
                .inner
                .archives
                .partial_path(workspace_id, import_id)
                .await
                .map_err(|error| Failure::Transient(error.to_string()))?;
            // A member's cancel shows only in the row, so it is polled while the download runs.
            let cancelled_by_member = async {
                loop {
                    tokio::time::sleep(Duration::from_secs(10)).await;
                    if !self.is_scanning(import_id).await? {
                        return Ok(());
                    }
                }
            };
            let downloaded = tokio::select! {
                () = run.cancel.cancelled() => Err(Failure::Interrupted),
                stop = cancelled_by_member => stop.map(|()| None),
                result = client.download_export(
                    &export_id,
                    &partial,
                    self.inner.settings.max_archive_bytes,
                ) => result.map(Some).map_err(Failure::from),
            };
            let size = match downloaded {
                Ok(Some(size)) => size,
                stop => {
                    let _ = tokio::fs::remove_file(&partial).await;
                    return stop.map(|_| ());
                }
            };
            if !is_zip(&partial).await {
                let _ = tokio::fs::remove_file(&partial).await;
                return Err(Failure::Fatal(ArchiveError::NotZip.to_string()));
            }
            let stored = self
                .inner
                .archives
                .store(workspace_id, import_id, &partial)
                .await
                .map_err(Failure::Transient)?;
            // The token did its work: the archive is here.
            sqlx::query(
                "UPDATE orbit_imports SET archive_location = $1, archive_bytes = $2, phase = 'reading', \
                 token_ciphertext = NULL, updated_at = $3 WHERE id = $4",
            )
            .bind(stored.as_str())
            .bind(i64::try_from(size).unwrap_or(i64::MAX))
            .bind(TimestampMillis::now().as_millis())
            .bind(import_id.to_string())
            .execute(self.database().pool())
            .await?;
            location = Some(stored);
        }

        self.set_phase(import_id, OrbitImportPhase::Reading).await?;
        let archive = self.open_archive(workspace_id, import_id, location).await?;
        let manifest = archive::blocking(&archive, Archive::manifest).await?;
        if token_mode.is_some_and(|mode| mode != manifest.mode) {
            return Err(Failure::Fatal(
                "The archive's mode does not match the token's scope.".to_owned(),
            ));
        }
        self.require_importer(workspace_id, user_id, Some(manifest.mode))
            .await
            .map_err(|error| match error {
                OrbitImportError::Forbidden => Failure::Fatal(
                    "A full import needs an owner or administrator of this workspace.".to_owned(),
                ),
                OrbitImportError::NotFound => {
                    Failure::Fatal("The importing member left the workspace.".to_owned())
                }
                other => Failure::Transient(other.to_string()),
            })?;
        let tree = archive::blocking(&archive, build_tree).await?;
        sqlx::query(
            "UPDATE orbit_imports SET status = 'ready', phase = NULL, mode = $1, source_workspace_id = $2, \
             source_workspace_name = $3, exported_by_email = $4, tree_json = $5, updated_at = $6 \
             WHERE id = $7 AND status = 'scanning'",
        )
        .bind(mode_str(manifest.mode))
        .bind(&manifest.workspace_id)
        .bind(clamp(&manifest.workspace_name, 200, 800))
        .bind(
            manifest
                .exported_by
                .email
                .as_deref()
                .map(|email| clamp(email, 320, 1_000)),
        )
        .bind(serde_json::to_string(&tree).map_err(|error| Failure::Fatal(error.to_string()))?)
        .bind(TimestampMillis::now().as_millis())
        .bind(import_id.to_string())
        .execute(self.database().pool())
        .await?;
        Ok(())
    }

    async fn import(&self, import_id: Id, job: &JobRun) -> Result<(), Failure> {
        let Some(row) = sqlx::query(
            "SELECT workspace_id, user_id, status, mode, source_workspace_id, selection_json, \
             report_json, archive_location FROM orbit_imports WHERE id = $1",
        )
        .bind(import_id.to_string())
        .fetch_optional(self.database().pool())
        .await?
        else {
            return Ok(());
        };
        let status: String = row.get("status");
        if status != "queued" && status != "importing" {
            return Ok(());
        }
        let workspace_id = parse_stored_id(row.get("workspace_id"))?;
        let user_id = parse_stored_id(row.get("user_id"))?;
        let invalid = || Failure::Fatal("Stored import data is invalid.".to_owned());
        let mode =
            parse_mode(row.get::<Option<String>, _>("mode").as_deref()).ok_or_else(invalid)?;
        let source_workspace_id: String = row
            .get::<Option<String>, _>("source_workspace_id")
            .ok_or_else(invalid)?;
        let selection: OrbitImportSelection = row
            .get::<Option<String>, _>("selection_json")
            .and_then(|json| serde_json::from_str(&json).ok())
            .ok_or_else(invalid)?;
        let report: StoredReport = row
            .get::<Option<String>, _>("report_json")
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default();
        ensure_writable(self.database(), workspace_id).await?;
        self.require_importer(workspace_id, user_id, Some(mode))
            .await
            .map_err(|error| match error {
                OrbitImportError::Forbidden => Failure::Fatal(
                    "A full import needs an owner or administrator of this workspace.".to_owned(),
                ),
                OrbitImportError::NotFound => {
                    Failure::Fatal("The importing member left the workspace.".to_owned())
                }
                other => Failure::Transient(other.to_string()),
            })?;
        sqlx::query(
            "UPDATE orbit_imports SET status = 'importing', updated_at = $1 WHERE id = $2 AND status = 'queued'",
        )
        .bind(TimestampMillis::now().as_millis())
        .bind(import_id.to_string())
        .execute(self.database().pool())
        .await?;

        let location = row
            .get::<Option<String>, _>("archive_location")
            .as_deref()
            .and_then(Location::parse);
        let archive = self.open_archive(workspace_id, import_id, location).await?;
        let workspace = archive::blocking(&archive, Archive::workspace).await?;
        let people = workspace
            .members
            .iter()
            .map(|member| {
                (
                    member.id.clone(),
                    SourcePerson {
                        name: member.name.clone(),
                        email: Some(member.email.clone()).filter(|email| !email.trim().is_empty()),
                    },
                )
            })
            .collect();
        let mut run = Run {
            import_id,
            workspace_id,
            user_id,
            source_workspace_id,
            link_scope: link_scope(mode, user_id),
            mode,
            selection,
            job: job.clone(),
            request_id: format!("orbit-import-{import_id}"),
            archive,
            report,
            people,
            members: crate::task_import::member_emails(self.database(), workspace_id).await?,
            unmatched_seen: HashSet::new(),
        };

        self.plan(&mut run, &workspace).await?;
        tasks::import_structure(self, &mut run, &workspace).await?;
        tasks::import_tasks(self, &mut run, &workspace).await?;
        pages::import_pages(self, &mut run).await?;
        chat::import_channels(self, &mut run).await?;
        self.save_report(&run).await?;

        sqlx::query(
            "UPDATE orbit_imports SET status = 'completed', token_ciphertext = NULL, updated_at = $1 \
             WHERE id = $2 AND status = 'importing'",
        )
        .bind(TimestampMillis::now().as_millis())
        .bind(import_id.to_string())
        .execute(self.database().pool())
        .await?;
        self.release_final_archives()
            .await
            .map_err(|error| Failure::Transient(error.to_string()))?;
        Ok(())
    }

    /// Writes the plan once: the selected teamspaces, projects, their live tasks (parents
    /// first), the selected pages (parents first) and channels.
    async fn plan(&self, run: &mut Run, workspace: &model::Workspace) -> Result<(), Failure> {
        let planned: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM orbit_import_items WHERE import_id = $1")
                .bind(run.import_id.to_string())
                .fetch_one(self.database().pool())
                .await?;
        if planned > 0 {
            return Ok(());
        }
        let mut items: Vec<Item> = Vec::new();
        let plan_item =
            |kind, source_id: &str, parent_id: Option<&str>, path: &str, title: &str| Item {
                kind,
                source_id: source_id.to_owned(),
                parent_id: parent_id.map(str::to_owned),
                path: path.to_owned(),
                title: clamp(title, 500, 2_000),
                orbit_id: None,
                state: "pending".to_owned(),
            };
        if run.mode == ExportMode::Full {
            for teamspace in &workspace.teamspaces {
                if run.selection.teamspace_ids.contains(&teamspace.id) {
                    items.push(plan_item(
                        "teamspace",
                        &teamspace.id,
                        None,
                        "",
                        &teamspace.name,
                    ));
                }
            }
            for project in &workspace.projects {
                if run.selection.project_ids.contains(&project.id) && !project.deleted {
                    items.push(plan_item("project", &project.id, None, "", &project.name));
                }
            }
            items.extend(tasks::plan_tasks(run).await?);
        }
        items.extend(pages::plan_pages(run).await?);
        items.extend(chat::plan_channels(run).await?);
        if items.len() > MAX_SELECTED * 10 {
            return Err(Failure::Fatal(format!(
                "The selection has more than {} items. Import fewer projects, teamspaces or channels at a time.",
                MAX_SELECTED * 10
            )));
        }
        let mut tx = self.database().immediate_transaction().await?;
        for (position, item) in items.iter().enumerate() {
            sqlx::query(
                "INSERT INTO orbit_import_items (import_id, kind, source_id, parent_id, path, position, title) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7) ON CONFLICT (import_id, kind, source_id) DO NOTHING",
            )
            .bind(run.import_id.to_string())
            .bind(item.kind)
            .bind(&item.source_id)
            .bind(item.parent_id.as_deref())
            .bind(&item.path)
            .bind(position as i64)
            .bind(&item.title)
            .execute(&mut *tx)
            .await?;
        }
        sqlx::query("UPDATE orbit_imports SET total = $1, updated_at = $2 WHERE id = $3")
            .bind(items.len() as i64)
            .bind(TimestampMillis::now().as_millis())
            .bind(run.import_id.to_string())
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Whether to stop before the next item; a workspace that became read-only fails the
    /// import.
    async fn should_stop(&self, run: &Run) -> Result<bool, Failure> {
        if run.job.cancel.is_cancelled() {
            return Ok(true);
        }
        ensure_writable(self.database(), run.workspace_id).await?;
        let status: Option<String> =
            sqlx::query_scalar("SELECT status FROM orbit_imports WHERE id = $1")
                .bind(run.import_id.to_string())
                .fetch_optional(self.database().pool())
                .await?;
        Ok(status.as_deref() != Some("importing"))
    }

    async fn is_scanning(&self, import_id: Id) -> Result<bool, Failure> {
        let status: Option<String> =
            sqlx::query_scalar("SELECT status FROM orbit_imports WHERE id = $1")
                .bind(import_id.to_string())
                .fetch_optional(self.database().pool())
                .await?;
        Ok(status.as_deref() == Some("scanning"))
    }

    /// The Orbit id an earlier import in this import's scope gave the source record
    /// `(kind, source_id)`, if that row still exists.
    async fn linked(&self, run: &Run, kind: &str, source_id: &str) -> Result<Option<Id>, Failure> {
        self.linked_in_scope(run, &run.link_scope, kind, source_id)
            .await
    }

    /// Whether this is a personal import and a full import already created the source record.
    /// Such a record is never written into by a personal import: the archive names the source
    /// ids, so a crafted one could otherwise target any imported channel or page.
    async fn taken_by_full_import(
        &self,
        run: &Run,
        kind: &str,
        source_id: &str,
    ) -> Result<bool, Failure> {
        if run.mode == ExportMode::Full {
            return Ok(false);
        }
        Ok(self
            .linked_in_scope(run, FULL_SCOPE, kind, source_id)
            .await?
            .is_some())
    }

    /// Leaves a personal import's item alone when a full import created the record: counted
    /// as existing and marked filled, so no member, message, comment or file lands in it.
    async fn skip_if_taken_by_full_import(
        &self,
        run: &mut Run,
        item: &mut Item,
    ) -> Result<bool, Failure> {
        if !self
            .taken_by_full_import(run, item.kind, &item.source_id)
            .await?
        {
            return Ok(false);
        }
        run.report.skipped_existing += 1;
        self.set_item_state(run, item, "filled").await?;
        Ok(true)
    }

    async fn linked_in_scope(
        &self,
        run: &Run,
        scope: &str,
        kind: &str,
        source_id: &str,
    ) -> Result<Option<Id>, Failure> {
        let found: Option<String> = sqlx::query_scalar(
            "SELECT orbit_id FROM orbit_import_links WHERE workspace_id = $1 AND source_workspace_id = $2 \
             AND scope = $3 AND kind = $4 AND source_id = $5",
        )
        .bind(run.workspace_id.to_string())
        .bind(&run.source_workspace_id)
        .bind(scope)
        .bind(kind)
        .bind(source_id)
        .fetch_optional(self.database().pool())
        .await?;
        let Some(id) = found.and_then(|id| id.parse::<Id>().ok()) else {
            return Ok(None);
        };
        Ok(self.row_exists(kind, id).await?.then_some(id))
    }

    /// Whether the Orbit row of kind `kind` with id `id` exists.
    async fn row_exists(&self, kind: &str, id: Id) -> Result<bool, Failure> {
        let table = match kind {
            "teamspace" => "teamspaces",
            "project" => "projects",
            "status" => "task_statuses",
            "label" => "labels",
            "task" => "tasks",
            "comment" => "task_comments",
            "attachment" => "attachment_references",
            "page" => "pages",
            "page_file" => "page_files",
            "page_thread" => "page_threads",
            "page_comment" => "page_comments",
            "channel" => "chat_conversations",
            "message" => "chat_messages",
            "message_file" => "chat_message_files",
            _ => return Ok(false),
        };
        let exists: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) FROM {table} WHERE id = $1"
        )))
        .bind(id.to_string())
        .fetch_one(self.database().pool())
        .await?;
        Ok(exists > 0)
    }

    async fn record_link(
        &self,
        run: &Run,
        kind: &str,
        source_id: &str,
        orbit_id: Id,
    ) -> Result<(), Failure> {
        record_link_in(
            self.database().pool(),
            run.workspace_id,
            &run.source_workspace_id,
            &run.link_scope,
            kind,
            source_id,
            orbit_id,
        )
        .await?;
        Ok(())
    }

    /// The Orbit id for an item: the one recorded on the item, else the one an earlier import
    /// made (counted as skipped), else a new one, recorded on the item and in the links before
    /// the row is created. Returns `(id, row already exists)`.
    async fn item_id(&self, run: &mut Run, item: &mut Item) -> Result<(Id, bool), Failure> {
        if let Some(id) = item.orbit_id {
            // The id is recorded before the row is created: after a restart, check the row.
            return Ok((id, self.row_exists(item.kind, id).await?));
        }
        let (id, existing) = match self.linked(run, item.kind, &item.source_id).await? {
            Some(id) => {
                run.report.skipped_existing += 1;
                (id, true)
            }
            None => (Id::new_v7(), false),
        };
        sqlx::query(
            "UPDATE orbit_import_items SET orbit_id = $1 WHERE import_id = $2 AND kind = $3 AND source_id = $4",
        )
        .bind(id.to_string())
        .bind(run.import_id.to_string())
        .bind(item.kind)
        .bind(&item.source_id)
        .execute(self.database().pool())
        .await?;
        self.record_link(run, item.kind, &item.source_id, id)
            .await?;
        item.orbit_id = Some(id);
        Ok((id, existing))
    }

    async fn set_item_state(&self, run: &Run, item: &mut Item, state: &str) -> Result<(), Failure> {
        item.state = state.to_owned();
        sqlx::query(
            "UPDATE orbit_import_items SET state = $1, error = NULL WHERE import_id = $2 AND kind = $3 AND source_id = $4",
        )
        .bind(state)
        .bind(run.import_id.to_string())
        .bind(item.kind)
        .bind(&item.source_id)
        .execute(self.database().pool())
        .await?;
        if state == "filled" {
            sqlx::query("UPDATE orbit_imports SET done = done + 1, updated_at = $1 WHERE id = $2")
                .bind(TimestampMillis::now().as_millis())
                .bind(run.import_id.to_string())
                .execute(self.database().pool())
                .await?;
        } else {
            self.touch(run.import_id).await?;
        }
        Ok(())
    }

    async fn fail_item(&self, run: &Run, item: &Item, message: &str) -> Result<(), Failure> {
        let mut tx = self.database().immediate_transaction().await?;
        let changed = sqlx::query(
            "UPDATE orbit_import_items SET state = 'failed', error = $1 \
             WHERE import_id = $2 AND kind = $3 AND source_id = $4 AND state <> 'failed'",
        )
        .bind(clamp(message, 500, 2_000))
        .bind(run.import_id.to_string())
        .bind(item.kind)
        .bind(&item.source_id)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        sqlx::query("UPDATE orbit_imports SET failed = failed + $1, updated_at = $2 WHERE id = $3")
            .bind(changed as i64)
            .bind(TimestampMillis::now().as_millis())
            .bind(run.import_id.to_string())
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Runs one item step; an item failure is recorded and the import continues, anything else
    /// stops it. `true` when the step succeeded.
    async fn step<F>(&self, run: &mut Run, item: &mut Item, step: F) -> Result<bool, Failure>
    where
        F: AsyncFnOnce(&Self, &mut Run, &mut Item) -> Result<(), Failure>,
    {
        if self.should_stop(run).await? {
            self.save_report(run).await?;
            return stopped(&run.job).map(|()| false);
        }
        match step(self, run, item).await {
            Ok(()) => Ok(true),
            Err(Failure::Item(message)) => {
                self.fail_item(run, item, &message).await?;
                item.state = "failed".to_owned();
                Ok(false)
            }
            Err(other) => {
                self.save_report(run).await?;
                Err(other)
            }
        }
    }

    async fn load_items(&self, import_id: Id, kind: &str) -> Result<Vec<Item>, Failure> {
        let rows = sqlx::query(
            "SELECT kind, source_id, parent_id, path, title, orbit_id, state \
             FROM orbit_import_items WHERE import_id = $1 AND kind = $2 ORDER BY position",
        )
        .bind(import_id.to_string())
        .bind(kind)
        .fetch_all(self.database().pool())
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok(Item {
                    kind: item_kind(row.get::<&str, _>("kind")).ok_or_else(|| {
                        Failure::Fatal("Stored import data is invalid.".to_owned())
                    })?,
                    source_id: row.get("source_id"),
                    parent_id: row.get("parent_id"),
                    path: row.get("path"),
                    title: row.get("title"),
                    orbit_id: row
                        .get::<Option<String>, _>("orbit_id")
                        .map(|id| id.parse())
                        .transpose()
                        .map_err(|_| Failure::Fatal("Stored import data is invalid.".to_owned()))?,
                    state: row.get("state"),
                })
            })
            .collect()
    }

    async fn touch(&self, import_id: Id) -> Result<(), Failure> {
        sqlx::query("UPDATE orbit_imports SET updated_at = $1 WHERE id = $2")
            .bind(TimestampMillis::now().as_millis())
            .bind(import_id.to_string())
            .execute(self.database().pool())
            .await?;
        Ok(())
    }

    async fn set_phase(&self, import_id: Id, phase: OrbitImportPhase) -> Result<(), Failure> {
        sqlx::query("UPDATE orbit_imports SET phase = $1, updated_at = $2 WHERE id = $3")
            .bind(phase.as_str())
            .bind(TimestampMillis::now().as_millis())
            .bind(import_id.to_string())
            .execute(self.database().pool())
            .await?;
        Ok(())
    }

    async fn save_report(&self, run: &Run) -> Result<(), Failure> {
        sqlx::query("UPDATE orbit_imports SET report_json = $1, updated_at = $2 WHERE id = $3")
            .bind(serde_json::to_string(&run.report).unwrap_or_default())
            .bind(TimestampMillis::now().as_millis())
            .bind(run.import_id.to_string())
            .execute(self.database().pool())
            .await?;
        Ok(())
    }

    /// Moves an open import to a final state and deletes its token.
    async fn finish(
        &self,
        import_id: Id,
        status: &str,
        error: Option<&str>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE orbit_imports SET status = $1, token_ciphertext = NULL, phase = NULL, error = $2, \
             updated_at = $3 WHERE id = $4 AND status IN {OPEN}"
        )))
        .bind(status)
        .bind(error.map(|error| clamp(error, 500, 2_000)))
        .bind(TimestampMillis::now().as_millis())
        .bind(import_id.to_string())
        .execute(self.database().pool())
        .await?;
        Ok(())
    }

    fn client_for(
        &self,
        url: Option<&str>,
        ciphertext: Option<Vec<u8>>,
    ) -> Result<OrbitClient, String> {
        let app_key = self
            .inner
            .settings
            .app_key
            .ok_or_else(|| "The server app key is not configured.".to_owned())?;
        let ciphertext = ciphertext.ok_or_else(|| "The API token was deleted.".to_owned())?;
        let token = decrypt_secret(&app_key, &ciphertext)
            .map_err(|()| "The API token could not be decrypted.".to_owned())?;
        let allow_local = self.inner.settings.allow_local;
        let origin = client::parse_origin(url.unwrap_or_default(), allow_local)
            .map_err(|_| "The instance URL is invalid.".to_owned())?;
        OrbitClient::new(origin, &token, allow_local)
            .map_err(|_| "The API token is invalid.".to_owned())
    }

    async fn open_archive(
        &self,
        workspace_id: Id,
        import_id: Id,
        location: Option<Location>,
    ) -> Result<Arc<Archive>, Failure> {
        let location =
            location.ok_or_else(|| Failure::Fatal("The archive was deleted.".to_owned()))?;
        let file = self
            .inner
            .archives
            .open(workspace_id, import_id, location)
            .await
            .map_err(Failure::Transient)?
            .ok_or_else(|| Failure::Fatal("The archive is missing.".to_owned()))?;
        Ok(tokio::task::spawn_blocking(move || Archive::open(file))
            .await
            .map_err(|error| Failure::Transient(error.to_string()))??)
    }

    async fn enqueue(
        &self,
        kind: JobKind,
        import_id: Id,
        workspace_id: Id,
    ) -> Result<(), OrbitImportError> {
        let job = Job::new(
            kind,
            json!({"import_id": import_id.to_string()}),
            TimestampMillis::now(),
        )
        .with_workspace(workspace_id.to_string());
        self.inner
            .jobs
            .enqueue(job)
            .await
            .map(|_| ())
            .map_err(|error| OrbitImportError::Unavailable(error.to_string()))
    }

    /// The member (never a guest), who must be an owner or admin for a full import.
    async fn require_importer(
        &self,
        workspace_id: Id,
        user_id: Id,
        mode: Option<ExportMode>,
    ) -> Result<(), OrbitImportError> {
        let actor = membership::actor(self.database().pool(), workspace_id, user_id)
            .await?
            .ok_or(OrbitImportError::NotFound)?;
        if !actor.can(Permission::WorkspaceExportPersonal) {
            return Err(OrbitImportError::Forbidden);
        }
        if mode == Some(ExportMode::Full) && !actor.can(Permission::WorkspaceExport) {
            return Err(OrbitImportError::Forbidden);
        }
        Ok(())
    }

    async fn load_view(
        &self,
        workspace_id: Id,
        user_id: Id,
        import_id: Id,
        with_tree: bool,
    ) -> Result<OrbitImport, OrbitImportError> {
        let row = sqlx::query(
            "SELECT id, status, phase, mode, source_kind, source_url, source_workspace_name, \
             exported_by_email, CASE WHEN $1 = 1 THEN tree_json END AS tree_json, selection_json, \
             total, done, failed, report_json, error, created_at, updated_at FROM orbit_imports \
             WHERE id = $2 AND workspace_id = $3 AND user_id = $4",
        )
        .bind(i64::from(with_tree))
        .bind(import_id.to_string())
        .bind(workspace_id.to_string())
        .bind(user_id.to_string())
        .fetch_optional(self.database().pool())
        .await?
        .ok_or(OrbitImportError::NotFound)?;
        let invalid = |_| OrbitImportError::Unavailable("invalid stored import".to_owned());
        let status = OrbitImportStatus::parse(&row.get::<String, _>("status"))?;
        let mode = parse_mode(row.get::<Option<String>, _>("mode").as_deref());
        let tree = row
            .get::<Option<String>, _>("tree_json")
            .map(|json| serde_json::from_str::<OrbitImportTree>(&json))
            .transpose()
            .map_err(|_| OrbitImportError::Unavailable("invalid stored tree".to_owned()))?;
        let report = if row.get::<Option<String>, _>("selection_json").is_some() {
            let stored: StoredReport = row
                .get::<Option<String>, _>("report_json")
                .and_then(|json| serde_json::from_str(&json).ok())
                .unwrap_or_default();
            let failures = sqlx::query(
                "SELECT kind, title, error FROM orbit_import_items \
                 WHERE import_id = $1 AND state = 'failed' ORDER BY position LIMIT $2",
            )
            .bind(import_id.to_string())
            .bind(MAX_FAILURES_LISTED)
            .fetch_all(self.database().pool())
            .await?
            .into_iter()
            .map(|row| OrbitImportFailure {
                kind: row.get("kind"),
                title: row.get("title"),
                error: row.get::<Option<String>, _>("error").unwrap_or_default(),
            })
            .collect();
            Some(OrbitImportReport {
                created: stored.created,
                skipped_existing: stored.skipped_existing,
                unmatched_users: stored.unmatched_users,
                failures,
                members_must_import_private: mode == Some(ExportMode::Full),
            })
        } else {
            None
        };
        let count = |column: &str| u64::try_from(row.get::<i64, _>(column)).unwrap_or_default();
        Ok(OrbitImport {
            id: row.get::<String, _>("id").parse().map_err(invalid)?,
            mode,
            source: OrbitImportSource {
                kind: if row.get::<String, _>("source_kind") == "upload" {
                    OrbitImportSourceKind::Upload
                } else {
                    OrbitImportSourceKind::Url
                },
                url: row.get("source_url"),
                workspace_name: row.get("source_workspace_name"),
                exported_by_email: row.get("exported_by_email"),
            },
            status,
            phase: row
                .get::<Option<String>, _>("phase")
                .as_deref()
                .and_then(OrbitImportPhase::parse),
            progress: OrbitImportProgress {
                total: count("total"),
                done: count("done"),
                failed: count("failed"),
            },
            error: row.get("error"),
            tree,
            report,
            created_at: TimestampMillis::from_millis(row.get("created_at")),
            updated_at: TimestampMillis::from_millis(row.get("updated_at")),
        })
    }
}

/// The `orbit_import_links` scope of full imports.
const FULL_SCOPE: &str = "";

/// The `orbit_import_links` scope of an import: full imports share one, each member's personal
/// imports have their own, so one member's archive can never claim or alter another's records.
fn link_scope(mode: ExportMode, user_id: Id) -> String {
    match mode {
        ExportMode::Full => FULL_SCOPE.to_owned(),
        ExportMode::Personal => user_id.to_string(),
    }
}

/// Records a link; a second import of the same record in the same scope keeps the newer id
/// (the earlier row is gone when this runs, see [`OrbitImportService::linked`]).
pub(crate) async fn record_link_in<'e, E>(
    executor: E,
    workspace_id: Id,
    source_workspace_id: &str,
    scope: &str,
    kind: &str,
    source_id: &str,
    orbit_id: Id,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Any>,
{
    sqlx::query(
        "INSERT INTO orbit_import_links (workspace_id, source_workspace_id, scope, kind, source_id, orbit_id, created_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7) \
         ON CONFLICT (workspace_id, source_workspace_id, scope, kind, source_id) DO UPDATE SET orbit_id = EXCLUDED.orbit_id",
    )
    .bind(workspace_id.to_string())
    .bind(source_workspace_id)
    .bind(scope)
    .bind(kind)
    .bind(source_id)
    .bind(orbit_id.to_string())
    .bind(TimestampMillis::now().as_millis())
    .execute(executor)
    .await?;
    Ok(())
}

/// Whether the member has an import that is not in a final state. Run it on the transaction
/// that creates the next one, so two concurrent creates cannot both pass.
async fn has_active_import<'e, E>(executor: E, user_id: Id) -> Result<bool, OrbitImportError>
where
    E: sqlx::Executor<'e, Database = sqlx::Any>,
{
    let active: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT COUNT(*) FROM orbit_imports WHERE user_id = $1 AND status IN {ACTIVE}"
    )))
    .bind(user_id.to_string())
    .fetch_one(executor)
    .await?;
    Ok(active > 0)
}

const fn mode_str(mode: ExportMode) -> &'static str {
    match mode {
        ExportMode::Full => "full",
        ExportMode::Personal => "personal",
    }
}

fn parse_mode(value: Option<&str>) -> Option<ExportMode> {
    match value {
        Some("full") => Some(ExportMode::Full),
        Some("personal") => Some(ExportMode::Personal),
        _ => None,
    }
}

/// Keeps the selected ids that are in the tree (deduplicated), refuses an empty or oversized
/// selection and, in personal mode, any project or teamspace.
fn checked_selection(
    selection: OrbitImportSelection,
    tree: &OrbitImportTree,
    mode: ExportMode,
) -> Result<OrbitImportSelection, OrbitImportError> {
    fn keep(
        ids: Vec<String>,
        known: impl Fn(&str) -> bool,
        field: &'static str,
    ) -> Result<Vec<String>, OrbitImportError> {
        let mut seen = HashSet::new();
        let mut kept = Vec::new();
        for id in ids {
            if !known(&id) {
                return Err(OrbitImportError::Invalid { field });
            }
            if seen.insert(id.clone()) {
                kept.push(id);
            }
        }
        Ok(kept)
    }
    if mode == ExportMode::Personal
        && (!selection.project_ids.is_empty() || !selection.teamspace_ids.is_empty())
    {
        return Err(OrbitImportError::Invalid {
            field: "project_ids",
        });
    }
    let selection = OrbitImportSelection {
        project_ids: keep(
            selection.project_ids,
            |id| tree.projects.iter().any(|project| project.id == id),
            "project_ids",
        )?,
        teamspace_ids: keep(
            selection.teamspace_ids,
            |id| tree.teamspaces.iter().any(|teamspace| teamspace.id == id),
            "teamspace_ids",
        )?,
        include_private_pages: selection.include_private_pages,
        channel_ids: keep(
            selection.channel_ids,
            |id| tree.channels.iter().any(|channel| channel.id == id),
            "channel_ids",
        )?,
    };
    let count =
        selection.project_ids.len() + selection.teamspace_ids.len() + selection.channel_ids.len();
    if count == 0 && !selection.include_private_pages {
        return Err(OrbitImportError::Invalid { field: "selection" });
    }
    if count > MAX_SELECTED {
        return Err(OrbitImportError::Invalid { field: "selection" });
    }
    Ok(selection)
}

/// Counts what the archive holds. Blocking.
fn build_tree(archive: &Archive) -> Result<OrbitImportTree, ArchiveError> {
    let manifest = archive.manifest()?;
    let workspace = archive.workspace()?;
    let mut tree = OrbitImportTree::default();
    if manifest.mode == ExportMode::Full {
        let mut task_counts: HashMap<String, u64> = HashMap::new();
        for path in archive.paths("tasks/", ".json") {
            let task: model::Task = archive.read_json(&path)?;
            if !task.deleted {
                *task_counts.entry(task.project_id).or_default() += 1;
            }
        }
        tree.projects = workspace
            .projects
            .iter()
            .filter(|project| !project.deleted)
            .map(|project| OrbitImportTreeProject {
                id: project.id.clone(),
                key: project.key.clone(),
                name: project.name.clone(),
                task_count: task_counts.get(&project.id).copied().unwrap_or_default(),
            })
            .collect();
    }
    let mut page_counts: HashMap<String, u64> = HashMap::new();
    for path in archive.paths("pages/", ".blocknote.json") {
        let page: model::Page = archive.read_json(&path)?;
        match page.teamspace_id {
            Some(teamspace) if manifest.mode == ExportMode::Full => {
                *page_counts.entry(teamspace).or_default() += 1;
            }
            Some(_) => {}
            None => tree.private_page_count += 1,
        }
    }
    if manifest.mode == ExportMode::Full {
        tree.teamspaces = workspace
            .teamspaces
            .iter()
            .map(|teamspace| OrbitImportTreeTeamspace {
                id: teamspace.id.clone(),
                name: teamspace.name.clone(),
                page_count: page_counts.get(&teamspace.id).copied().unwrap_or_default(),
            })
            .collect();
    }
    for path in archive.paths("chat/", ".json") {
        let conversation: model::Conversation = archive.read_json(&path)?;
        if manifest.mode == ExportMode::Personal && conversation.kind == "public" {
            continue;
        }
        let message_count = conversation
            .messages
            .iter()
            .map(|message| 1 + message.replies.len() as u64)
            .sum();
        tree.channels.push(OrbitImportTreeChannel {
            id: conversation.id.clone(),
            kind: conversation.kind.clone(),
            name: chat::conversation_title(&conversation),
            message_count,
        });
    }
    Ok(tree)
}

/// Whether a file starts like a zip archive with entries (a local file header).
async fn is_zip(path: &std::path::Path) -> bool {
    use tokio::io::AsyncReadExt;
    let Ok(mut file) = tokio::fs::File::open(path).await else {
        return false;
    };
    let mut magic = [0u8; 4];
    file.read_exact(&mut magic).await.is_ok() && magic == *b"PK\x03\x04"
}
