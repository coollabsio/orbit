//! Obsidian import: a member uploads a zip of a vault, an `obsidian.scan` job checks it and lists
//! its folders and notes as a page tree, the member picks everything or some of it plus a
//! destination, and an `obsidian.import` job creates the Orbit pages.
//!
//! State machine (`obsidian_imports.status`):
//! `scanning` → `ready` → `queued` → `importing` → `completed`; any active state can become
//! `failed` or `cancelled`, and a `ready` import that is not started within 24 hours becomes
//! `expired`. The archive is deleted in every final state (the hourly cleanup retries a
//! deletion that failed).
//!
//! Jobs carry only the import id. Progress lives in `obsidian_import_items`, so a restarted job
//! continues where it stopped: pass 1 creates every page (parents first, each with an id recorded
//! before the page exists), pass 2 reads each note from the archive, attaches the files it links
//! to or embeds as page files, and saves its content with links pointing to the new pages. A page
//! that fails is recorded and the import continues. When an import fails or is cancelled, the
//! pages pass 1 created but pass 2 never filled are moved to the trash; filled pages stay.

use crate::import_notice::{ImportNotices, ImportSource};
use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use orbit_platform::Row;
use orbit_platform::{
    Backend, Database, Flag, Id, Job, JobError, JobKind, JobKindRegistrationError, JobQueue,
    JobStore, ObjectStorage, TimestampMillis, Worker,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use thiserror::Error;
use tokio_util::sync::CancellationToken;
use utoipa::ToSchema;
use zip::ZipArchive;

use super::archive::{ArchiveStore, LocalArchive, Location};
use super::scan::{ObsidianImportNodeKind, ObsidianImportTree, scan};
use super::vault::{ObsidianSkippedFiles, Resolved, Vault, VaultError, VaultLimits, open_archive};
use crate::billing::WorkspaceAccess;
use crate::collab::markdown_import::{
    LinkKind, LinkResolver, LinkTarget, obsidian_links, obsidian_to_blocks,
};
use crate::notion::convert::{TRUNCATION_NOTE, cut_to_size};
use crate::notion::files::sanitize_file_name;
use crate::notion::import::clamp;
use crate::repositories::billing;
use crate::repositories::membership;
use crate::repositories::page_files::{PageFileRepository, page_file_url};
use crate::repositories::page_versions::SaveOrigin;
use crate::repositories::pages::{
    CreatePage, PageChanges, PageError, PageRepository, PageSpace, SpaceRequest,
};

/// Job kind names.
pub const SCAN_JOB: &str = "obsidian.scan";
pub const IMPORT_JOB: &str = "obsidian.import";
/// Most pages one import creates.
pub const MAX_PAGES: usize = 5_000;
/// A scanned import waits this long for "start" before its archive is deleted.
pub const READY_TTL: Duration = Duration::from_secs(24 * 60 * 60);
/// An active import that made no progress for this long is failed by the cleanup.
const STALLED_AFTER: Duration = Duration::from_secs(24 * 60 * 60);
/// The Notion import's page size cap: below the 1 MiB page body limit.
const MAX_CONTENT_BYTES: usize = 900 * 1024;
const MAX_TITLE_CHARS: usize = 500;
const MAX_TITLE_BYTES: usize = 2_000;
const MAX_FILE_NAME_CHARS: usize = 255;
const MAX_FAILURES_LISTED: i64 = 200;
const MAX_LISTED_IMPORTS: i64 = 20;
const ACTIVE: &str = "('scanning', 'queued', 'importing')";
const OPEN: &str = "('scanning', 'ready', 'queued', 'importing')";
const FINAL: &str = "('completed', 'failed', 'cancelled', 'expired')";

/// Both job kinds: two at a time on all servers and one per workspace while another workspace
/// waits (defaults; Admin can change them), a few retries for storage outages.
#[must_use]
pub fn scan_job_kind() -> JobKind {
    obsidian_job_kind(SCAN_JOB)
}

#[must_use]
pub fn import_job_kind() -> JobKind {
    obsidian_job_kind(IMPORT_JOB)
}

fn obsidian_job_kind(name: &str) -> JobKind {
    JobKind::new(name)
        .with_concurrency_limit(2)
        .with_workspace_limit(1)
        .with_retry_policy(
            5,
            vec![
                Duration::from_secs(30),
                Duration::from_secs(2 * 60),
                Duration::from_secs(10 * 60),
                Duration::from_secs(30 * 60),
            ],
        )
}

/// Registers the `obsidian.scan` and `obsidian.import` handlers.
pub fn register_jobs(
    worker: Worker,
    service: ObsidianImportService,
    notices: ImportNotices,
) -> Result<Worker, JobKindRegistrationError> {
    let scan = service.clone();
    worker
        .with_handler(scan_job_kind(), move |context| {
            let service = scan.clone();
            async move {
                let Some(import_id) = payload_import_id(&context.job.payload) else {
                    return Err(JobError::Permanent(
                        "invalid obsidian.scan payload".to_owned(),
                    ));
                };
                let run = JobRun {
                    cancel: context.cancellation_token(),
                    last_attempt: context.claim.attempt >= context.job.max_attempts,
                };
                let result = service.run_scan(import_id, &run).await;
                service.after_attempt(import_id, &run, result).await
            }
        })?
        .with_handler(import_job_kind(), move |context| {
            let service = service.clone();
            let notices = notices.clone();
            async move {
                let Some(import_id) = payload_import_id(&context.job.payload) else {
                    return Err(JobError::Permanent(
                        "invalid obsidian.import payload".to_owned(),
                    ));
                };
                let run = JobRun {
                    cancel: context.cancellation_token(),
                    last_attempt: context.claim.attempt >= context.job.max_attempts,
                };
                let result = service.run_import(import_id, &run).await;
                let result = service.after_attempt(import_id, &run, result).await;
                notices.after_run(ImportSource::Obsidian, import_id).await;
                result
            }
        })
}

fn payload_import_id(payload: &Value) -> Option<Id> {
    payload.get("import_id")?.as_str()?.parse().ok()
}

/// One job attempt: `cancel` fires on server shutdown (the job resumes later), and
/// `last_attempt` turns a transient failure into a failed import instead of a retry.
#[derive(Clone, Debug, Default)]
pub struct JobRun {
    pub cancel: CancellationToken,
    pub last_attempt: bool,
}

/// Service settings.
#[derive(Clone, Copy, Debug)]
pub struct ObsidianImportSettings {
    /// Largest upload (the request size limit).
    pub max_archive_bytes: u64,
    /// Largest file inside the archive that is imported (the upload limit for one file).
    pub max_file_bytes: u64,
}

impl ObsidianImportSettings {
    fn limits(self) -> VaultLimits {
        VaultLimits::new(self.max_file_bytes)
    }
}

#[derive(Clone)]
pub struct ObsidianImportService {
    inner: Arc<Inner>,
}

struct Inner {
    database: Database,
    pages: PageRepository,
    files: PageFileRepository,
    jobs: JobQueue,
    archives: ArchiveStore,
    settings: ObsidianImportSettings,
}

/// Errors of the import API.
#[derive(Debug, Error)]
pub enum ObsidianImportError {
    #[error("import not found")]
    NotFound,
    #[error("another import is in progress")]
    InProgress,
    #[error("the import is not in a state that allows this")]
    StateConflict,
    #[error("the selection has more than {MAX_PAGES} pages")]
    TooLarge,
    #[error("the archive is larger than {0} bytes")]
    ArchiveTooLarge(u64),
    #[error("the upload is not a zip archive")]
    ArchiveInvalid,
    #[error("invalid input: {field}")]
    Invalid { field: &'static str },
    #[error("destination page not found")]
    PageNotFound,
    #[error("destination teamspace not found")]
    TeamspaceNotFound,
    #[error("import storage is unavailable")]
    Unavailable(String),
}

impl From<sqlx::Error> for ObsidianImportError {
    fn from(error: sqlx::Error) -> Self {
        Self::Unavailable(error.to_string())
    }
}

/// `obsidian_imports.status`.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ObsidianImportStatus {
    /// The scan job is checking the archive and listing its notes.
    Scanning,
    /// The tree is ready; waiting for `start` (the archive is deleted after 24 hours).
    Ready,
    /// Started; waiting for a job slot.
    Queued,
    Importing,
    Completed,
    /// The archive was refused (see `error`) or the import stopped.
    Failed,
    Cancelled,
    /// Not started within 24 hours; the archive was deleted.
    Expired,
}

impl ObsidianImportStatus {
    fn parse(value: &str) -> Result<Self, ObsidianImportError> {
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
                return Err(ObsidianImportError::Unavailable(
                    "unknown import status".to_owned(),
                ));
            }
        })
    }
}

#[derive(Clone, Copy, Debug, Serialize, ToSchema)]
pub struct ObsidianImportProgress {
    /// Pages to import (folders and notes).
    pub total: u64,
    pub done: u64,
    pub failed: u64,
}

/// Where the import puts its top-level pages.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct ObsidianImportDestination {
    /// `null` for the importer's private space.
    #[schema(value_type = Option<String>, required = true)]
    pub teamspace_id: Option<Id>,
    pub private: bool,
    /// The page the imported top-level pages go under, if any.
    #[schema(value_type = Option<String>, required = true)]
    pub parent_page_id: Option<Id>,
}

/// A page the import could not create or fill.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct ObsidianImportFailure {
    pub path: String,
    pub title: String,
    pub error: String,
}

/// What the import carried over and what it left out.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct ObsidianImportReport {
    /// Files attached to the new pages.
    pub files_imported: u32,
    /// Linked or embedded files that could not be attached (for example a type page files do
    /// not accept).
    pub missing_files: u32,
    /// Links whose target is not an imported page or file; they stayed as text.
    pub unresolved_links: u32,
    /// Top-level blocks cut from notes larger than the page size limit.
    pub truncated_blocks: u32,
    /// Files of the archive that were not imported, by reason.
    pub skipped_files: ObsidianSkippedFiles,
    /// Pages the import created but never filled because it failed or was cancelled; they
    /// were moved to the trash (a restored page no longer counts).
    pub unfilled_pages_trashed: u32,
    /// The first 200 failed pages.
    pub failures: Vec<ObsidianImportFailure>,
}

/// An Obsidian import.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct ObsidianImport {
    #[schema(value_type = String)]
    pub id: Id,
    #[schema(value_type = String)]
    pub workspace_id: Id,
    pub status: ObsidianImportStatus,
    /// The uploaded file's name.
    pub file_name: String,
    /// The uploaded file's size.
    pub archive_bytes: u64,
    pub progress: ObsidianImportProgress,
    /// Why the archive was refused, or why the import failed or expired.
    #[schema(required = true)]
    pub error: Option<String>,
    /// Set once the import was started.
    #[schema(required = true)]
    pub destination: Option<ObsidianImportDestination>,
    /// The scan result, once ready. Only in `GET …/imports/obsidian/{id}?include=tree`; `null`
    /// everywhere else, so polling stays light.
    #[schema(required = true)]
    pub tree: Option<ObsidianImportTree>,
    /// Import summary, once the import started.
    #[schema(required = true)]
    pub report: Option<ObsidianImportReport>,
    /// The imported top-level pages that still exist, in order (for "Open imported pages").
    #[schema(value_type = Vec<String>)]
    pub root_page_ids: Vec<Id>,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: TimestampMillis,
    #[schema(value_type = String, format = DateTime)]
    pub updated_at: TimestampMillis,
}

/// Which folders and notes to import.
#[derive(Clone, Debug)]
pub enum ImportSelection {
    All,
    /// These nodes and all their descendants.
    Paths(Vec<String>),
}

/// The requested destination; with a parent page the pages join the parent's space.
#[derive(Clone, Debug, Default)]
pub struct ImportDestination {
    pub teamspace_id: Option<Id>,
    pub private: Option<bool>,
    pub parent_page_id: Option<Id>,
}

/// An upload being received: the route writes the file to `path`, then calls
/// [`ObsidianImportService::create`].
pub struct Upload {
    pub id: Id,
    pub path: PathBuf,
}

#[derive(Default, Deserialize, Serialize)]
struct StoredReport {
    #[serde(default)]
    files_imported: u32,
    #[serde(default)]
    missing_files: u32,
    #[serde(default)]
    unresolved_links: u32,
    #[serde(default)]
    truncated_blocks: u32,
    #[serde(default)]
    skipped: ObsidianSkippedFiles,
}

#[derive(Deserialize, Serialize)]
struct StoredSelection {
    all: bool,
    #[serde(default)]
    paths: Vec<String>,
    #[serde(default)]
    parent_page_id: Option<String>,
}

#[derive(Clone, Debug)]
struct Item {
    path: String,
    note_path: Option<String>,
    parent_path: Option<String>,
    title: String,
    page_id: Option<Id>,
    status: String,
}

/// Why one page could not be imported, and what that means for the import.
enum Failure {
    /// Server shutdown: the job is requeued and resumes; never fails the import.
    Interrupted,
    /// Record on the page and continue.
    Page(String),
    /// Stop the import (archive gone, missing destination, read-only workspace).
    Fatal(String),
    /// Database or storage trouble: retry the job later.
    Transient(String),
}

impl From<sqlx::Error> for Failure {
    fn from(error: sqlx::Error) -> Self {
        Self::Transient(format!("database error: {error}"))
    }
}

impl From<VaultError> for Failure {
    fn from(error: VaultError) -> Self {
        Self::Fatal(error.to_string())
    }
}

/// The archive opened for one job attempt.
struct OpenVault {
    vault: Arc<Vault>,
    archive: Arc<Mutex<ZipArchive<File>>>,
    /// Keeps a downloaded copy alive (and deletes it at the end).
    _file: LocalArchive,
}

impl OpenVault {
    async fn read(&self, path: &str, notes: bool) -> Result<Vec<u8>, Failure> {
        let vault = Arc::clone(&self.vault);
        let archive = Arc::clone(&self.archive);
        let path = path.to_owned();
        tokio::task::spawn_blocking(move || {
            let list = if notes {
                &vault.notes
            } else {
                &vault.attachments
            };
            let file = list
                .iter()
                .find(|file| file.path == path)
                .ok_or_else(|| Failure::Page("The file is missing from the archive.".to_owned()))?;
            let mut archive = archive
                .lock()
                .map_err(|_| Failure::Transient("archive lock poisoned".to_owned()))?;
            Vault::read(&mut archive, file).map_err(Failure::from)
        })
        .await
        .map_err(|error| Failure::Transient(error.to_string()))?
    }
}

/// Resolves the links of one note to the pages and files of this import, counting the ones that
/// stay text.
struct NoteResolver {
    vault: Arc<Vault>,
    note: String,
    /// Lowercase note path → page href.
    pages: Arc<HashMap<String, String>>,
    /// Lowercase attachment path → its page file.
    files: HashMap<String, LinkTarget>,
    unresolved: Cell<u32>,
}

impl LinkResolver for NoteResolver {
    fn resolve(&self, target: &str, _kind: LinkKind) -> Option<LinkTarget> {
        let found = match self.vault.resolve(&self.note, target) {
            Some(Resolved::Note(note)) => self
                .pages
                .get(&note.path.to_lowercase())
                .map(|href| LinkTarget::Page { href: href.clone() }),
            Some(Resolved::Attachment(file)) => self.files.get(&file.path.to_lowercase()).cloned(),
            None => None,
        };
        if found.is_none() {
            self.unresolved.set(self.unresolved.get().saturating_add(1));
        }
        found
    }
}

fn is_image(path: &str) -> bool {
    let lower = path.to_lowercase();
    [
        ".png", ".jpg", ".jpeg", ".gif", ".webp", ".svg", ".bmp", ".avif",
    ]
    .iter()
    .any(|extension| lower.ends_with(extension))
}

impl ObsidianImportService {
    #[must_use]
    pub fn new(
        database: Database,
        files: PageFileRepository,
        storage: ObjectStorage,
        local_root: impl Into<PathBuf>,
        settings: ObsidianImportSettings,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                pages: PageRepository::new(database.clone()),
                jobs: JobQueue::new(JobStore::new(database.clone())),
                archives: ArchiveStore::new(storage, local_root),
                database,
                files,
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

    /// Checks that the member may upload now and returns where to write the upload. A member
    /// has at most one active import.
    pub async fn prepare(
        &self,
        workspace_id: Id,
        user_id: Id,
    ) -> Result<Upload, ObsidianImportError> {
        self.require_member(workspace_id, user_id).await?;
        self.expire_stale(TimestampMillis::now()).await?;
        if self.has_active_import(user_id).await? {
            return Err(ObsidianImportError::InProgress);
        }
        let id = Id::new_v7();
        let path = self
            .inner
            .archives
            .partial_path(workspace_id, id)
            .await
            .map_err(|error| ObsidianImportError::Unavailable(error.to_string()))?;
        Ok(Upload { id, path })
    }

    /// Stores a received upload and queues the scan. The member's unstarted imports in this
    /// workspace are cancelled. The upload file is gone afterwards in every case.
    pub async fn create(
        &self,
        workspace_id: Id,
        user_id: Id,
        upload: Upload,
        file_name: &str,
        size: u64,
    ) -> Result<ObsidianImport, ObsidianImportError> {
        let result = self
            .store_upload(workspace_id, user_id, &upload, file_name, size)
            .await;
        let _ = tokio::fs::remove_file(&upload.path).await;
        let id = result?;
        self.release_final_archives().await?;
        if let Err(error) = self.enqueue(scan_job_kind(), id, workspace_id).await {
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
        file_name: &str,
        size: u64,
    ) -> Result<Id, ObsidianImportError> {
        if !is_zip(&upload.path).await {
            return Err(ObsidianImportError::ArchiveInvalid);
        }
        let location = self
            .inner
            .archives
            .store(workspace_id, upload.id, &upload.path)
            .await
            .map_err(ObsidianImportError::Unavailable)?;
        let inserted = self
            .insert(workspace_id, user_id, upload.id, file_name, size, location)
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

    async fn insert(
        &self,
        workspace_id: Id,
        user_id: Id,
        id: Id,
        file_name: &str,
        size: u64,
        location: Location,
    ) -> Result<(), ObsidianImportError> {
        let now = TimestampMillis::now();
        let mut tx = self.database().immediate_transaction().await?;
        let active: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) FROM obsidian_imports WHERE user_id = $1 AND status IN {ACTIVE}"
        )))
        .bind(user_id.to_string())
        .fetch_one(&mut *tx)
        .await?;
        if active > 0 {
            return Err(ObsidianImportError::InProgress);
        }
        sqlx::query(
            "UPDATE obsidian_imports SET status = 'cancelled', updated_at = $1 \
             WHERE user_id = $2 AND workspace_id = $3 AND status = 'ready'",
        )
        .bind(now.as_millis())
        .bind(user_id.to_string())
        .bind(workspace_id.to_string())
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO obsidian_imports (id, workspace_id, user_id, status, file_name, \
             archive_bytes, archive_location, created_at, updated_at) \
             VALUES ($1, $2, $3, 'scanning', $4, $5, $6, $7, $8)",
        )
        .bind(id.to_string())
        .bind(workspace_id.to_string())
        .bind(user_id.to_string())
        .bind(clamp(file_name.trim(), MAX_FILE_NAME_CHARS, 1_000))
        .bind(i64::try_from(size).unwrap_or(i64::MAX))
        .bind(location.as_str())
        .bind(now.as_millis())
        .bind(now.as_millis())
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// The member's recent imports in this workspace, newest first (no trees).
    pub async fn list(
        &self,
        workspace_id: Id,
        user_id: Id,
    ) -> Result<Vec<ObsidianImport>, ObsidianImportError> {
        self.require_member(workspace_id, user_id).await?;
        self.expire_stale(TimestampMillis::now()).await?;
        let ids: Vec<String> = sqlx::query_scalar(
            "SELECT id FROM obsidian_imports WHERE workspace_id = $1 AND user_id = $2 \
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
                .map_err(|_| ObsidianImportError::Unavailable("invalid import id".to_owned()))?;
            imports.push(self.load_view(workspace_id, user_id, id, false).await?);
        }
        Ok(imports)
    }

    /// One of the member's imports; the scan tree only `with_tree`. Other members (admins
    /// included) get `NotFound`.
    pub async fn get(
        &self,
        workspace_id: Id,
        user_id: Id,
        import_id: Id,
        with_tree: bool,
    ) -> Result<ObsidianImport, ObsidianImportError> {
        self.require_member(workspace_id, user_id).await?;
        self.expire_stale(TimestampMillis::now()).await?;
        self.load_view(workspace_id, user_id, import_id, with_tree)
            .await
    }

    /// Plans the import of `selection` into `destination` and queues the import job.
    pub async fn start(
        &self,
        workspace_id: Id,
        user_id: Id,
        import_id: Id,
        selection: ImportSelection,
        destination: ImportDestination,
    ) -> Result<ObsidianImport, ObsidianImportError> {
        self.require_member(workspace_id, user_id).await?;
        self.expire_stale(TimestampMillis::now()).await?;
        let row = sqlx::query(
            "SELECT status, tree_json FROM obsidian_imports WHERE id = $1 AND workspace_id = $2 AND user_id = $3",
        )
        .bind(import_id.to_string())
        .bind(workspace_id.to_string())
        .bind(user_id.to_string())
        .fetch_optional(self.database().pool())
        .await?
        .ok_or(ObsidianImportError::NotFound)?;
        if row.get::<String, _>("status") != "ready" {
            return Err(ObsidianImportError::StateConflict);
        }
        let tree: ObsidianImportTree = serde_json::from_str(
            &row.get::<Option<String>, _>("tree_json")
                .unwrap_or_default(),
        )
        .map_err(|_| ObsidianImportError::Unavailable("invalid stored tree".to_owned()))?;

        let space = self
            .inner
            .pages
            .resolve_destination(
                workspace_id,
                user_id,
                destination.parent_page_id,
                SpaceRequest {
                    teamspace_id: destination.teamspace_id,
                    private: destination.private,
                },
            )
            .await
            .map_err(|error| match error {
                PageError::NotFound if destination.parent_page_id.is_some() => {
                    ObsidianImportError::PageNotFound
                }
                PageError::NotFound => ObsidianImportError::NotFound,
                PageError::TeamspaceNotFound => ObsidianImportError::TeamspaceNotFound,
                PageError::Invalid { field } => ObsidianImportError::Invalid { field },
                other => ObsidianImportError::Unavailable(other.to_string()),
            })?;

        let plan = plan_items(&tree, &selection)?;
        if plan.len() > MAX_PAGES {
            return Err(ObsidianImportError::TooLarge);
        }
        let report = StoredReport {
            skipped: tree.skipped,
            ..StoredReport::default()
        };
        let stored_selection = StoredSelection {
            all: matches!(selection, ImportSelection::All),
            paths: match &selection {
                ImportSelection::All => Vec::new(),
                ImportSelection::Paths(paths) => paths.clone(),
            },
            parent_page_id: destination.parent_page_id.map(|id| id.to_string()),
        };

        let now = TimestampMillis::now();
        let mut tx = self.database().immediate_transaction().await?;
        let active: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) FROM obsidian_imports WHERE user_id = $1 AND id <> $2 AND status IN {ACTIVE}"
        )))
        .bind(user_id.to_string())
        .bind(import_id.to_string())
        .fetch_one(&mut *tx)
        .await?;
        if active > 0 {
            return Err(ObsidianImportError::InProgress);
        }
        let updated = sqlx::query(
            "UPDATE obsidian_imports SET status = 'queued', selection_json = $1, target_teamspace_id = $2, \
             target_private = $3, target_parent_page_id = $4, total = $5, done = 0, failed = 0, \
             report_json = $6, updated_at = $7, started_at = $7 \
             WHERE id = $8 AND status = 'ready' AND archive_location IS NOT NULL",
        )
        .bind(serde_json::to_string(&stored_selection).unwrap_or_default())
        .bind(space.teamspace_id().map(|id| id.to_string()))
        .bind(i64::from(matches!(space, PageSpace::Private(_))))
        .bind(destination.parent_page_id.map(|id| id.to_string()))
        .bind(plan.len() as i64)
        .bind(serde_json::to_string(&report).unwrap_or_default())
        .bind(now.as_millis())
        .bind(import_id.to_string())
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if updated != 1 {
            return Err(ObsidianImportError::StateConflict);
        }
        for (position, item) in plan.iter().enumerate() {
            sqlx::query(
                "INSERT INTO obsidian_import_items (import_id, path, kind, note_path, parent_path, \
                 position, title) VALUES ($1, $2, $3, $4, $5, $6, $7)",
            )
            .bind(import_id.to_string())
            .bind(&item.path)
            .bind(match item.kind {
                ObsidianImportNodeKind::Folder => "folder",
                ObsidianImportNodeKind::Note => "note",
            })
            .bind(&item.note_path)
            .bind(&item.parent_path)
            .bind(position as i64)
            .bind(clamp(&item.title, MAX_TITLE_CHARS, MAX_TITLE_BYTES))
            .execute(&mut *tx)
            .await?;
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

    /// Cancels an open import and deletes its archive. A running job stops before its next page.
    pub async fn cancel(
        &self,
        workspace_id: Id,
        user_id: Id,
        import_id: Id,
    ) -> Result<ObsidianImport, ObsidianImportError> {
        self.require_member(workspace_id, user_id).await?;
        let updated = sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE obsidian_imports SET status = 'cancelled', updated_at = $1 \
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
            return Err(ObsidianImportError::StateConflict);
        }
        // A running job trashes its unfilled pages itself once it sees the cancellation (the
        // page it is filling right now must not be trashed under it). Otherwise clean up now.
        if !self.is_running(import_id).await? {
            self.trash_unfilled_pages(import_id).await?;
        }
        self.release_final_archives().await?;
        self.load_view(workspace_id, user_id, import_id, false)
            .await
    }

    /// Expires scanned imports that were not started within 24 hours, fails active imports that
    /// made no progress for 24 hours, and deletes the archives of finished imports. Returns the
    /// imports changed.
    pub async fn expire_stale(&self, now: TimestampMillis) -> Result<u64, ObsidianImportError> {
        let ready_cutoff = now.as_millis() - READY_TTL.as_millis() as i64;
        let stalled_cutoff = now.as_millis() - STALLED_AFTER.as_millis() as i64;
        let expired = sqlx::query(
            "UPDATE obsidian_imports SET status = 'expired', \
             error = 'The import was not started within 24 hours.', updated_at = $1 \
             WHERE status = 'ready' AND updated_at < $2",
        )
        .bind(now.as_millis())
        .bind(ready_cutoff)
        .execute(self.database().pool())
        .await?
        .rows_affected();
        let stalled = sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE obsidian_imports SET status = 'failed', \
             error = 'The import stopped making progress.', updated_at = $1 \
             WHERE status IN {ACTIVE} AND updated_at < $2"
        )))
        .bind(now.as_millis())
        .bind(stalled_cutoff)
        .execute(self.database().pool())
        .await?
        .rows_affected();
        let dead = self.fail_imports_with_dead_jobs().await?;
        self.release_final_archives().await?;
        Ok(expired + stalled + dead)
    }

    /// Deletes the archives of imports in a final state. A deletion that fails is retried by
    /// the next call (the hourly cleanup at the latest).
    async fn release_final_archives(&self) -> Result<(), ObsidianImportError> {
        let rows: Vec<(String, String, String)> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT id, workspace_id, archive_location FROM obsidian_imports \
             WHERE status IN {FINAL} AND archive_location IS NOT NULL LIMIT 100"
        )))
        .fetch_all(self.database().pool())
        .await?;
        for (id, workspace, location) in rows {
            let (Ok(import_id), Ok(workspace_id), Some(location)) = (
                id.parse::<Id>(),
                workspace.parse::<Id>(),
                Location::parse(&location),
            ) else {
                continue;
            };
            match self
                .inner
                .archives
                .delete(workspace_id, import_id, location)
                .await
            {
                Ok(()) => {
                    sqlx::query(
                        "UPDATE obsidian_imports SET archive_location = NULL WHERE id = $1",
                    )
                    .bind(&id)
                    .execute(self.database().pool())
                    .await?;
                }
                Err(error) => {
                    tracing::warn!(import_id = %import_id, error = %error, "Obsidian import archive could not be deleted");
                }
            }
        }
        Ok(())
    }

    /// When a job attempt that was not interrupted by shutdown fails for the last time, the job
    /// is about to go dead: finish the import now instead of leaving it active until the stall
    /// rule.
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
                .fail_open_import(import_id, "The import stopped after repeated errors.")
                .await
        {
            tracing::warn!(import_id = %import_id, error = %error, cleanup = %cleanup, "Obsidian import could not be finalized");
        }
        result
    }

    fn import_id_of_job(&self) -> &'static str {
        match self.database().backend() {
            Backend::Sqlite => "json_extract(jobs.payload_json, '$.import_id')",
            Backend::Postgres => "(jobs.payload_json::jsonb ->> 'import_id')",
        }
    }

    /// Fails active imports whose job is dead and that have no live job left.
    async fn fail_imports_with_dead_jobs(&self) -> Result<u64, ObsidianImportError> {
        let import_id_of_job = self.import_id_of_job();
        let ids: Vec<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT id FROM obsidian_imports WHERE status IN {ACTIVE} \
             AND EXISTS (SELECT 1 FROM jobs WHERE jobs.kind IN ('{SCAN_JOB}', '{IMPORT_JOB}') \
                 AND jobs.state = 'dead' \
                 AND {import_id_of_job} = obsidian_imports.id) \
             AND NOT EXISTS (SELECT 1 FROM jobs WHERE jobs.kind IN ('{SCAN_JOB}', '{IMPORT_JOB}') \
                 AND jobs.state IN ('queued', 'running') \
                 AND {import_id_of_job} = obsidian_imports.id)"
        )))
        .fetch_all(self.database().pool())
        .await?;
        let mut finished = 0;
        for id in ids {
            let Ok(import_id) = id.parse::<Id>() else {
                continue;
            };
            self.fail_open_import(import_id, "The import stopped after repeated errors.")
                .await?;
            finished += 1;
        }
        Ok(finished)
    }

    /// Trashes the unfilled pages of an open import, then fails it.
    async fn fail_open_import(&self, import_id: Id, message: &str) -> Result<(), sqlx::Error> {
        let status: Option<String> =
            sqlx::query_scalar("SELECT status FROM obsidian_imports WHERE id = $1")
                .bind(import_id.to_string())
                .fetch_optional(self.database().pool())
                .await?;
        if !matches!(
            status.as_deref(),
            Some("scanning" | "ready" | "queued" | "importing")
        ) {
            return Ok(());
        }
        self.trash_unfilled_pages(import_id).await?;
        self.finish(import_id, "failed", Some(message)).await
    }

    /// Opens the stored archive on this server, or explains why it cannot.
    async fn open_archive(&self, import_id: Id) -> Result<LocalArchive, Failure> {
        let row = sqlx::query(
            "SELECT workspace_id, archive_location FROM obsidian_imports WHERE id = $1",
        )
        .bind(import_id.to_string())
        .fetch_optional(self.database().pool())
        .await?
        .ok_or_else(|| Failure::Fatal("The import was deleted.".to_owned()))?;
        let workspace_id = parse_stored_id(row.get("workspace_id"))?;
        let location = row
            .get::<Option<String>, _>("archive_location")
            .as_deref()
            .and_then(Location::parse)
            .ok_or_else(|| Failure::Fatal("The uploaded archive was deleted.".to_owned()))?;
        self.inner
            .archives
            .open(workspace_id, import_id, location)
            .await
            .map_err(Failure::Transient)?
            .ok_or_else(|| Failure::Fatal("The uploaded archive is missing.".to_owned()))
    }

    /// The `obsidian.scan` job: checks the archive and stores the tree.
    pub async fn run_scan(&self, import_id: Id, run: &JobRun) -> Result<(), JobError> {
        let status: Option<String> =
            sqlx::query_scalar("SELECT status FROM obsidian_imports WHERE id = $1")
                .bind(import_id.to_string())
                .fetch_optional(self.database().pool())
                .await
                .map_err(retryable)?;
        if status.as_deref() != Some("scanning") {
            return Ok(());
        }
        let result = match self.open_archive(import_id).await {
            Ok(file) => {
                let limits = self.inner.settings.limits();
                tokio::task::spawn_blocking(move || {
                    let reader = File::open(&file.path)
                        .map_err(|error| VaultError::Read(error.to_string()))?;
                    let mut archive = open_archive(reader)?;
                    scan(&mut archive, limits)
                })
                .await
                .map_err(|error| Failure::Transient(error.to_string()))
                .and_then(|scanned| scanned.map_err(Failure::from))
            }
            Err(failure) => Err(failure),
        };
        match result {
            Ok(tree) => {
                sqlx::query(
                    "UPDATE obsidian_imports SET status = 'ready', tree_json = $1, updated_at = $2 \
                     WHERE id = $3 AND status = 'scanning'",
                )
                .bind(serde_json::to_string(&tree).map_err(retryable)?)
                .bind(TimestampMillis::now().as_millis())
                .bind(import_id.to_string())
                .execute(self.database().pool())
                .await
                .map_err(retryable)?;
                Ok(())
            }
            Err(failure) => self.job_failure(import_id, failure, run).await,
        }
    }

    /// The `obsidian.import` job: pass 1 creates the pages, pass 2 fills them. A failed or
    /// cancelled import trashes the pages it created but did not fill.
    pub async fn run_import(&self, import_id: Id, run: &JobRun) -> Result<(), JobError> {
        match self.import(import_id, run).await {
            Ok(()) => {}
            Err(failure) => self.job_failure(import_id, failure, run).await?,
        }
        let status: Option<String> =
            sqlx::query_scalar("SELECT status FROM obsidian_imports WHERE id = $1")
                .bind(import_id.to_string())
                .fetch_optional(self.database().pool())
                .await
                .map_err(retryable)?;
        if matches!(status.as_deref(), Some("failed" | "cancelled")) {
            self.trash_unfilled_pages(import_id)
                .await
                .map_err(retryable)?;
        }
        self.release_final_archives().await.map_err(retryable)?;
        Ok(())
    }

    /// Whether the `obsidian.import` job of the import runs now, on any server.
    async fn is_running(&self, import_id: Id) -> Result<bool, ObsidianImportError> {
        let import_id_of_job = self.import_id_of_job();
        let Flag(running) = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT EXISTS (SELECT 1 FROM jobs WHERE jobs.kind = '{IMPORT_JOB}' \
             AND jobs.state = 'running' AND {import_id_of_job} = $1)"
        )))
        .bind(import_id.to_string())
        .fetch_one(self.database().pool())
        .await?;
        Ok(running)
    }

    /// Moves the pages that pass 1 created and pass 2 never filled (items still `created`) to
    /// the trash, one batch per topmost such page, as the importing member (audited as
    /// `page.deleted`). A subtree is kept when any live page in it has content or is not one of
    /// these pages. Idempotent; returns the pages trashed.
    async fn trash_unfilled_pages(&self, import_id: Id) -> Result<u64, sqlx::Error> {
        let Some(row) =
            sqlx::query("SELECT workspace_id, user_id FROM obsidian_imports WHERE id = $1")
                .bind(import_id.to_string())
                .fetch_optional(self.database().pool())
                .await?
        else {
            return Ok(0);
        };
        let (Ok(workspace_id), Ok(user_id)) = (
            row.get::<String, _>("workspace_id").parse::<Id>(),
            row.get::<String, _>("user_id").parse::<Id>(),
        ) else {
            return Ok(0);
        };
        let unfilled: Vec<(String, Option<String>, String)> = sqlx::query_as(
            "SELECT path, parent_path, page_id FROM obsidian_import_items \
             WHERE import_id = $1 AND status = 'created' AND page_id IS NOT NULL ORDER BY position",
        )
        .bind(import_id.to_string())
        .fetch_all(self.database().pool())
        .await?;
        let unfilled_paths: HashSet<&str> =
            unfilled.iter().map(|(path, _, _)| path.as_str()).collect();
        let request_id = format!("obsidian-import-{import_id}");
        let mut trashed = 0;
        for (_, parent, page_id) in &unfilled {
            if parent
                .as_deref()
                .is_some_and(|parent| unfilled_paths.contains(parent))
            {
                continue; // Goes with its unfilled parent.
            }
            let Ok(page) = page_id.parse::<Id>() else {
                continue;
            };
            // (version, live pages in the subtree, of which kept: filled or not ours)
            let (version, size, kept): (Option<i64>, i64, i64) = sqlx::query_as(
                "WITH RECURSIVE subtree(id) AS ( \
                     SELECT id FROM pages WHERE id = $1 AND deleted_at IS NULL \
                     UNION ALL \
                     SELECT pages.id FROM pages JOIN subtree ON pages.parent_id = subtree.id \
                     WHERE pages.deleted_at IS NULL \
                 ) \
                 SELECT (SELECT version FROM pages WHERE id = $2), COUNT(*), \
                        CAST(COALESCE(SUM(CASE WHEN pages.content_json <> '[]' OR pages.id NOT IN ( \
                            SELECT page_id FROM obsidian_import_items \
                            WHERE import_id = $3 AND status = 'created' AND page_id IS NOT NULL) \
                            THEN 1 ELSE 0 END), 0) AS BIGINT) \
                 FROM pages WHERE pages.id IN (SELECT id FROM subtree)",
            )
            .bind(page_id)
            .bind(page_id)
            .bind(import_id.to_string())
            .fetch_one(self.database().pool())
            .await?;
            let Some(version) = version.and_then(|version| u64::try_from(version).ok()) else {
                continue;
            };
            if size == 0 || kept > 0 {
                continue;
            }
            match self
                .inner
                .pages
                .delete_page(
                    workspace_id,
                    page,
                    user_id,
                    version,
                    &request_id,
                    TimestampMillis::now(),
                )
                .await
            {
                Ok(()) => trashed += u64::try_from(size).unwrap_or_default(),
                // Filled, trashed or moved away meanwhile, or the member lost access: keep it.
                Err(PageError::Unavailable(error)) => return Err(error),
                Err(_) => {}
            }
        }
        if trashed > 0 {
            tracing::info!(import_id = %import_id, pages = trashed, "Obsidian import trashed unfilled pages");
        }
        Ok(trashed)
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
                tracing::warn!(import_id = %import_id, error = %message, "Obsidian import will retry");
                Err(JobError::Retryable(message))
            }
            Failure::Transient(message) if run.cancel.is_cancelled() => {
                Err(JobError::Retryable(message))
            }
            Failure::Transient(message) | Failure::Fatal(message) | Failure::Page(message) => {
                // Trash first, so a client that stops polling at `failed` sees the final report.
                self.trash_unfilled_pages(import_id)
                    .await
                    .map_err(retryable)?;
                self.finish(import_id, "failed", Some(&message))
                    .await
                    .map_err(retryable)?;
                self.release_final_archives().await.map_err(retryable)?;
                Ok(())
            }
        }
    }

    async fn import(&self, import_id: Id, run: &JobRun) -> Result<(), Failure> {
        let Some(row) = sqlx::query(
            "SELECT workspace_id, user_id, status, selection_json, \
             target_teamspace_id, target_private, report_json FROM obsidian_imports WHERE id = $1",
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
        self.ensure_writable(workspace_id).await?;
        let selection: Option<StoredSelection> = row
            .get::<Option<String>, _>("selection_json")
            .and_then(|json| serde_json::from_str(&json).ok());
        let parent_page_id = selection
            .as_ref()
            .and_then(|selection| selection.parent_page_id.as_deref())
            .map(|id| id.parse::<Id>())
            .transpose()
            .map_err(|_| Failure::Fatal("The import destination is invalid.".to_owned()))?;
        let teamspace_id = row
            .get::<Option<String>, _>("target_teamspace_id")
            .map(|id| id.parse::<Id>())
            .transpose()
            .map_err(|_| Failure::Fatal("The import destination is invalid.".to_owned()))?;
        let private = row.get::<bool, _>("target_private");
        let mut report: StoredReport = row
            .get::<Option<String>, _>("report_json")
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default();
        sqlx::query(
            "UPDATE obsidian_imports SET status = 'importing', updated_at = $1 WHERE id = $2 AND status = 'queued'",
        )
        .bind(TimestampMillis::now().as_millis())
        .bind(import_id.to_string())
        .execute(self.database().pool())
        .await?;

        // The destination must still exist (a deleted teamspace clears target_teamspace_id).
        if !private && teamspace_id.is_none() && parent_page_id.is_none() {
            return Err(Failure::Fatal(
                "The destination teamspace was deleted.".to_owned(),
            ));
        }
        let destination_space = SpaceRequest {
            teamspace_id: if parent_page_id.is_some() {
                None
            } else {
                teamspace_id
            },
            private: if parent_page_id.is_some() {
                None
            } else {
                Some(private)
            },
        };
        self.inner
            .pages
            .resolve_destination(workspace_id, user_id, parent_page_id, destination_space)
            .await
            .map_err(|error| match error {
                PageError::Unavailable(error) => Failure::from(error),
                _ => Failure::Fatal("The import destination is no longer available.".to_owned()),
            })?;

        let file = self.open_archive(import_id).await?;
        let limits = self.inner.settings.limits();
        let path = file.path.clone();
        let (vault, archive) = tokio::task::spawn_blocking(move || {
            let reader = File::open(&path).map_err(|error| VaultError::Read(error.to_string()))?;
            let mut archive = open_archive(reader)?;
            let vault = Vault::open(&mut archive, limits)?;
            Ok::<_, VaultError>((vault, archive))
        })
        .await
        .map_err(|error| Failure::Transient(error.to_string()))??;
        let vault = OpenVault {
            vault: Arc::new(vault),
            archive: Arc::new(Mutex::new(archive)),
            _file: file,
        };

        let mut items = self.load_items(import_id).await?;

        // Pass 1: create every page, parents first.
        for index in 0..items.len() {
            if items[index].status != "pending" {
                continue;
            }
            if self.should_stop(import_id, workspace_id, run).await? {
                return stopped(run);
            }
            let parent = items[index]
                .parent_path
                .as_ref()
                .and_then(|parent| {
                    items.iter().find(|item| {
                        &item.path == parent && matches!(item.status.as_str(), "created" | "done")
                    })
                })
                .and_then(|item| item.page_id);
            let result = self
                .create_item_page(
                    import_id,
                    workspace_id,
                    user_id,
                    &mut items[index],
                    parent,
                    parent_page_id,
                    destination_space,
                )
                .await;
            match result {
                Ok(()) => items[index].status = "created".to_owned(),
                Err(Failure::Page(message)) => {
                    self.fail_item(import_id, &items[index].path, &message)
                        .await?;
                    items[index].status = "failed".to_owned();
                }
                Err(other) => return Err(other),
            }
        }

        // Pass 2: fill each page.
        let pages: Arc<HashMap<String, String>> = Arc::new(
            items
                .iter()
                .filter(|item| matches!(item.status.as_str(), "created" | "done"))
                .filter_map(|item| {
                    Some((
                        item.note_path.as_ref()?.to_lowercase(),
                        format!("/docs/{}", item.page_id?),
                    ))
                })
                .collect(),
        );
        for index in 0..items.len() {
            if items[index].status != "created" {
                continue;
            }
            if self.should_stop(import_id, workspace_id, run).await? {
                return stopped(run);
            }
            let item = items[index].clone();
            let result = self
                .fill_item_page(
                    workspace_id,
                    user_id,
                    &item,
                    &items,
                    &vault,
                    &pages,
                    &mut report,
                )
                .await;
            match result {
                Ok(()) => {
                    sqlx::query(
                        "UPDATE obsidian_import_items SET status = 'done', error = NULL \
                         WHERE import_id = $1 AND path = $2",
                    )
                    .bind(import_id.to_string())
                    .bind(&item.path)
                    .execute(self.database().pool())
                    .await?;
                    self.save_progress(import_id, &report, 1).await?;
                    items[index].status = "done".to_owned();
                }
                Err(Failure::Page(message)) => {
                    self.fail_item(import_id, &item.path, &message).await?;
                    self.save_progress(import_id, &report, 0).await?;
                    items[index].status = "failed".to_owned();
                }
                Err(other) => {
                    self.save_progress(import_id, &report, 0).await?;
                    return Err(other);
                }
            }
        }

        sqlx::query(
            "UPDATE obsidian_imports SET status = 'completed', updated_at = $1 \
             WHERE id = $2 AND status = 'importing'",
        )
        .bind(TimestampMillis::now().as_millis())
        .bind(import_id.to_string())
        .execute(self.database().pool())
        .await?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn create_item_page(
        &self,
        import_id: Id,
        workspace_id: Id,
        user_id: Id,
        item: &mut Item,
        parent: Option<Id>,
        destination_parent: Option<Id>,
        destination_space: SpaceRequest,
    ) -> Result<(), Failure> {
        let page_id = match item.page_id {
            Some(page_id) => page_id,
            None => {
                let page_id = Id::new_v7();
                sqlx::query(
                    "UPDATE obsidian_import_items SET page_id = $1 WHERE import_id = $2 AND path = $3",
                )
                .bind(page_id.to_string())
                .bind(import_id.to_string())
                .bind(&item.path)
                .execute(self.database().pool())
                .await?;
                item.page_id = Some(page_id);
                page_id
            }
        };
        let exists: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pages WHERE id = $1")
            .bind(page_id.to_string())
            .fetch_one(self.database().pool())
            .await?;
        if exists == 0 {
            let is_root = parent.is_none();
            let (parent_id, space) = match parent {
                Some(parent) => (Some(parent), SpaceRequest::default()),
                None => (destination_parent, destination_space),
            };
            let input = CreatePage {
                kind: crate::repositories::pages::PageKind::Page,
                parent_id,
                space,
                title: clamp(&item.title, MAX_TITLE_CHARS, MAX_TITLE_BYTES),
                icon: None,
                position: None,
            };
            let request_id = format!("obsidian-import-{import_id}");
            match self
                .inner
                .pages
                .create_page_with_id(
                    page_id,
                    workspace_id,
                    user_id,
                    input,
                    &request_id,
                    TimestampMillis::now(),
                )
                .await
            {
                Ok(_) => {}
                Err(PageError::Unavailable(error)) => return Err(Failure::from(error)),
                Err(PageError::TrialLimit(reached)) => {
                    return Err(Failure::Fatal(format!(
                        "The import stopped at the trial's limit. {}",
                        reached.detail()
                    )));
                }
                Err(_) if is_root => {
                    return Err(Failure::Fatal(
                        "The import destination is no longer available.".to_owned(),
                    ));
                }
                Err(_) => {
                    return Err(Failure::Page(
                        "The parent page was deleted during the import.".to_owned(),
                    ));
                }
            }
        }
        sqlx::query(
            "UPDATE obsidian_import_items SET status = 'created' WHERE import_id = $1 AND path = $2",
        )
        .bind(import_id.to_string())
        .bind(&item.path)
        .execute(self.database().pool())
        .await?;
        sqlx::query("UPDATE obsidian_imports SET updated_at = $1 WHERE id = $2")
            .bind(TimestampMillis::now().as_millis())
            .bind(import_id.to_string())
            .execute(self.database().pool())
            .await?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn fill_item_page(
        &self,
        workspace_id: Id,
        user_id: Id,
        item: &Item,
        items: &[Item],
        vault: &OpenVault,
        pages: &Arc<HashMap<String, String>>,
        report: &mut StoredReport,
    ) -> Result<(), Failure> {
        let page_id = item
            .page_id
            .ok_or_else(|| Failure::Page("The page was not created.".to_owned()))?;
        let request_id = format!("obsidian-import-{page_id}");
        let content = match &item.note_path {
            Some(note_path) => {
                let bytes = vault.read(note_path, true).await?;
                let markdown = String::from_utf8_lossy(&bytes).into_owned();
                let files = self
                    .attach_files(
                        workspace_id,
                        user_id,
                        page_id,
                        note_path,
                        &markdown,
                        vault,
                        report,
                    )
                    .await?;
                let resolver = NoteResolver {
                    vault: Arc::clone(&vault.vault),
                    note: note_path.clone(),
                    pages: Arc::clone(pages),
                    files,
                    unresolved: Cell::new(0),
                };
                let (mut content, unresolved) = tokio::task::spawn_blocking(move || {
                    let content = obsidian_to_blocks(&markdown, &resolver);
                    (content, resolver.unresolved.get())
                })
                .await
                .map_err(|error| Failure::Transient(error.to_string()))?;
                report.unresolved_links = report.unresolved_links.saturating_add(unresolved);
                let cut = cut_to_size(&mut content, MAX_CONTENT_BYTES);
                if cut > 0 {
                    report.truncated_blocks = report
                        .truncated_blocks
                        .saturating_add(u32::try_from(cut).unwrap_or(u32::MAX));
                    content.push(json!({
                        "id": Id::new_v7().to_string(),
                        "type": "paragraph",
                        "props": {},
                        "content": [{"type": "text", "text": TRUNCATION_NOTE, "styles": {"italic": true}}],
                        "children": [],
                    }));
                }
                content
            }
            // A folder without a folder note lists its pages.
            None => items
                .iter()
                .filter(|child| child.parent_path.as_deref() == Some(item.path.as_str()))
                .filter(|child| matches!(child.status.as_str(), "created" | "done"))
                .filter_map(|child| child.page_id)
                .map(|child| {
                    json!({
                        "id": Id::new_v7().to_string(),
                        "type": "page",
                        "props": {"pageId": child.to_string()},
                        "children": []
                    })
                })
                .collect(),
        };
        let current = self
            .inner
            .pages
            .get_page(workspace_id, page_id, user_id)
            .await
            .map_err(page_failure)?;
        self.inner
            .pages
            .update_page(
                workspace_id,
                page_id,
                user_id,
                current.version,
                PageChanges {
                    content: Some(content),
                    // The imported state becomes an `import` version in the page history.
                    origin: SaveOrigin::Import,
                    ..PageChanges::default()
                },
                &request_id,
                TimestampMillis::now(),
            )
            .await
            .map_err(page_failure)?;
        Ok(())
    }

    /// Attaches the files a note links to or embeds to its page (once each; a retried page
    /// reuses the files an earlier attempt attached). Returns them by lowercase vault path.
    #[allow(clippy::too_many_arguments)]
    async fn attach_files(
        &self,
        workspace_id: Id,
        user_id: Id,
        page_id: Id,
        note_path: &str,
        markdown: &str,
        vault: &OpenVault,
        report: &mut StoredReport,
    ) -> Result<HashMap<String, LinkTarget>, Failure> {
        let mut targets: Vec<String> = Vec::new();
        for (target, _) in obsidian_links(markdown) {
            if let Some(Resolved::Attachment(file)) = vault.vault.resolve(note_path, &target)
                && !targets.contains(&file.path)
            {
                targets.push(file.path.clone());
            }
        }
        let mut files = HashMap::new();
        if targets.is_empty() {
            return Ok(files);
        }
        let attached = self.attached_files(workspace_id, page_id).await?;
        let request_id = format!("obsidian-import-{page_id}");
        for path in targets {
            let bytes = match vault.read(&path, false).await {
                Ok(bytes) => bytes,
                Err(Failure::Page(_)) => {
                    report.missing_files += 1;
                    continue;
                }
                Err(other) => return Err(other),
            };
            let sha256 = format!("{:x}", Sha256::digest(&bytes));
            let name = sanitize_file_name(path.rsplit('/').next().unwrap_or(&path));
            let url = match attached.iter().find(|(hash, size, _)| {
                *hash == sha256 && usize::try_from(*size).ok() == Some(bytes.len())
            }) {
                Some((_, _, url)) => url.clone(),
                None => match self
                    .inner
                    .files
                    .create_from_bytes(workspace_id, page_id, user_id, &name, &bytes, &request_id)
                    .await
                {
                    Ok(stored) => {
                        report.files_imported += 1;
                        stored.url
                    }
                    Err(error) => {
                        tracing::debug!(error = %error, "Obsidian file could not be attached");
                        report.missing_files += 1;
                        continue;
                    }
                },
            };
            let target = if is_image(&path) {
                LinkTarget::Image { url }
            } else {
                LinkTarget::File { url, name }
            };
            files.insert(path.to_lowercase(), target);
        }
        Ok(files)
    }

    /// The page's files with their content hash and size: `(sha256, byte_size, url)`.
    async fn attached_files(
        &self,
        workspace_id: Id,
        page_id: Id,
    ) -> Result<Vec<(String, i64, String)>, Failure> {
        let rows: Vec<(String, String, i64)> = sqlx::query_as(
            "SELECT page_files.id, attachment_blobs.sha256, attachment_blobs.byte_size \
             FROM page_files JOIN attachment_blobs ON attachment_blobs.id = page_files.blob_id \
             WHERE page_files.workspace_id = $1 AND page_files.page_id = $2",
        )
        .bind(workspace_id.to_string())
        .bind(page_id.to_string())
        .fetch_all(self.database().pool())
        .await?;
        Ok(rows
            .into_iter()
            .filter_map(|(file_id, sha256, size)| {
                let file_id = file_id.parse().ok()?;
                Some((sha256, size, page_file_url(workspace_id, page_id, file_id)))
            })
            .collect())
    }

    /// Whether to stop before the next page; a workspace that became read-only fails the import.
    async fn should_stop(
        &self,
        import_id: Id,
        workspace_id: Id,
        run: &JobRun,
    ) -> Result<bool, Failure> {
        if run.cancel.is_cancelled() {
            return Ok(true);
        }
        self.ensure_writable(workspace_id).await?;
        let status: Option<String> =
            sqlx::query_scalar("SELECT status FROM obsidian_imports WHERE id = $1")
                .bind(import_id.to_string())
                .fetch_optional(self.database().pool())
                .await?;
        Ok(status.as_deref() != Some("importing"))
    }

    /// Orbit Cloud: a read-only workspace (past its trial, no subscription) takes no imported pages.
    async fn ensure_writable(&self, workspace_id: Id) -> Result<(), Failure> {
        match billing::write_access(self.database(), workspace_id).await? {
            WorkspaceAccess::Full | WorkspaceAccess::Trial => Ok(()),
            WorkspaceAccess::ReadOnly => Err(Failure::Fatal(
                "The import stopped because the workspace is read-only: it has no active \
                 subscription."
                    .to_owned(),
            )),
        }
    }

    async fn fail_item(&self, import_id: Id, path: &str, message: &str) -> Result<(), Failure> {
        let mut tx = self.database().immediate_transaction().await?;
        let changed = sqlx::query(
            "UPDATE obsidian_import_items SET status = 'failed', error = $1 \
             WHERE import_id = $2 AND path = $3 AND status <> 'failed'",
        )
        .bind(clamp(message, 500, 2_000))
        .bind(import_id.to_string())
        .bind(path)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        sqlx::query(
            "UPDATE obsidian_imports SET failed = failed + $1, updated_at = $2 WHERE id = $3",
        )
        .bind(changed as i64)
        .bind(TimestampMillis::now().as_millis())
        .bind(import_id.to_string())
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn save_progress(
        &self,
        import_id: Id,
        report: &StoredReport,
        done: i64,
    ) -> Result<(), Failure> {
        sqlx::query(
            "UPDATE obsidian_imports SET done = done + $1, report_json = $2, updated_at = $3 \
             WHERE id = $4",
        )
        .bind(done)
        .bind(serde_json::to_string(report).unwrap_or_default())
        .bind(TimestampMillis::now().as_millis())
        .bind(import_id.to_string())
        .execute(self.database().pool())
        .await?;
        Ok(())
    }

    /// Moves an open import to a final state; its archive is deleted by
    /// [`Self::release_final_archives`].
    async fn finish(
        &self,
        import_id: Id,
        status: &str,
        error: Option<&str>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE obsidian_imports SET status = $1, error = $2, updated_at = $3 \
             WHERE id = $4 AND status IN {OPEN}"
        )))
        .bind(status)
        .bind(error.map(|error| clamp(error, 500, 2_000)))
        .bind(TimestampMillis::now().as_millis())
        .bind(import_id.to_string())
        .execute(self.database().pool())
        .await?;
        Ok(())
    }

    async fn enqueue(
        &self,
        kind: JobKind,
        import_id: Id,
        workspace_id: Id,
    ) -> Result<(), ObsidianImportError> {
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
            .map_err(|error| ObsidianImportError::Unavailable(error.to_string()))
    }

    async fn require_member(
        &self,
        workspace_id: Id,
        user_id: Id,
    ) -> Result<(), ObsidianImportError> {
        membership::actor(self.database().pool(), workspace_id, user_id)
            .await?
            .map(|_| ())
            .ok_or(ObsidianImportError::NotFound)
    }

    async fn has_active_import(&self, user_id: Id) -> Result<bool, ObsidianImportError> {
        let count: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) FROM obsidian_imports WHERE user_id = $1 AND status IN {ACTIVE}"
        )))
        .bind(user_id.to_string())
        .fetch_one(self.database().pool())
        .await?;
        Ok(count > 0)
    }

    async fn load_items(&self, import_id: Id) -> Result<Vec<Item>, Failure> {
        let rows = sqlx::query(
            "SELECT path, note_path, parent_path, title, page_id, status \
             FROM obsidian_import_items WHERE import_id = $1 ORDER BY position",
        )
        .bind(import_id.to_string())
        .fetch_all(self.database().pool())
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok(Item {
                    path: row.get("path"),
                    note_path: row.get("note_path"),
                    parent_path: row.get("parent_path"),
                    title: row.get("title"),
                    page_id: row
                        .get::<Option<String>, _>("page_id")
                        .map(|id| id.parse())
                        .transpose()
                        .map_err(|_| Failure::Fatal("Stored import data is invalid.".to_owned()))?,
                    status: row.get("status"),
                })
            })
            .collect()
    }

    async fn load_view(
        &self,
        workspace_id: Id,
        user_id: Id,
        import_id: Id,
        with_tree: bool,
    ) -> Result<ObsidianImport, ObsidianImportError> {
        let row = sqlx::query(
            "SELECT id, workspace_id, status, file_name, archive_bytes, \
             CASE WHEN $1 THEN tree_json END AS tree_json, selection_json, target_teamspace_id, \
             target_private, target_parent_page_id, total, done, failed, \
             report_json, error, created_at, updated_at FROM obsidian_imports \
             WHERE id = $2 AND workspace_id = $3 AND user_id = $4",
        )
        .bind(with_tree)
        .bind(import_id.to_string())
        .bind(workspace_id.to_string())
        .bind(user_id.to_string())
        .fetch_optional(self.database().pool())
        .await?
        .ok_or(ObsidianImportError::NotFound)?;
        let invalid = |_| ObsidianImportError::Unavailable("invalid stored import".to_owned());
        let status = ObsidianImportStatus::parse(&row.get::<String, _>("status"))?;
        let started = row.get::<Option<String>, _>("selection_json").is_some();
        let tree = row
            .get::<Option<String>, _>("tree_json")
            .map(|json| serde_json::from_str::<ObsidianImportTree>(&json))
            .transpose()
            .map_err(|_| ObsidianImportError::Unavailable("invalid stored tree".to_owned()))?;
        let destination = if started {
            Some(ObsidianImportDestination {
                teamspace_id: row
                    .get::<Option<String>, _>("target_teamspace_id")
                    .map(|id| id.parse())
                    .transpose()
                    .map_err(invalid)?,
                private: row.get("target_private"),
                parent_page_id: row
                    .get::<Option<String>, _>("target_parent_page_id")
                    .map(|id| id.parse())
                    .transpose()
                    .map_err(invalid)?,
            })
        } else {
            None
        };
        let report = if started {
            let stored: StoredReport = row
                .get::<Option<String>, _>("report_json")
                .and_then(|json| serde_json::from_str(&json).ok())
                .unwrap_or_default();
            let failures = sqlx::query(
                "SELECT path, title, error FROM obsidian_import_items \
                 WHERE import_id = $1 AND status = 'failed' ORDER BY position LIMIT $2",
            )
            .bind(import_id.to_string())
            .bind(MAX_FAILURES_LISTED)
            .fetch_all(self.database().pool())
            .await?
            .into_iter()
            .map(|row| ObsidianImportFailure {
                path: row.get("path"),
                title: row.get("title"),
                error: row.get::<Option<String>, _>("error").unwrap_or_default(),
            })
            .collect();
            let unfilled_pages_trashed: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM obsidian_import_items AS items \
                 JOIN pages ON pages.id = items.page_id \
                 WHERE items.import_id = $1 AND items.status = 'created' \
                 AND pages.deleted_at IS NOT NULL",
            )
            .bind(import_id.to_string())
            .fetch_one(self.database().pool())
            .await?;
            Some(ObsidianImportReport {
                files_imported: stored.files_imported,
                missing_files: stored.missing_files,
                unresolved_links: stored.unresolved_links,
                truncated_blocks: stored.truncated_blocks,
                skipped_files: stored.skipped,
                unfilled_pages_trashed: u32::try_from(unfilled_pages_trashed).unwrap_or_default(),
                failures,
            })
        } else {
            None
        };
        let root_page_ids = sqlx::query_scalar::<_, String>(
            "SELECT items.page_id FROM obsidian_import_items AS items \
             JOIN pages ON pages.id = items.page_id \
             WHERE items.import_id = $1 AND items.parent_path IS NULL \
             AND items.status IN ('created', 'done') AND pages.workspace_id = $2 \
             AND pages.deleted_at IS NULL AND (pages.teamspace_id IS NOT NULL OR pages.owner_id = $3) \
             ORDER BY items.position",
        )
        .bind(import_id.to_string())
        .bind(workspace_id.to_string())
        .bind(user_id.to_string())
        .fetch_all(self.database().pool())
        .await?
        .into_iter()
        .map(|id| id.parse())
        .collect::<Result<Vec<Id>, _>>()
        .map_err(invalid)?;
        let count = |column: &str| u64::try_from(row.get::<i64, _>(column)).unwrap_or_default();
        Ok(ObsidianImport {
            id: row.get::<String, _>("id").parse().map_err(invalid)?,
            workspace_id: row
                .get::<String, _>("workspace_id")
                .parse()
                .map_err(invalid)?,
            status,
            file_name: row.get("file_name"),
            archive_bytes: count("archive_bytes"),
            progress: ObsidianImportProgress {
                total: count("total"),
                done: count("done"),
                failed: count("failed"),
            },
            error: row.get("error"),
            destination,
            tree,
            report,
            root_page_ids,
            created_at: TimestampMillis::from_millis(row.get("created_at")),
            updated_at: TimestampMillis::from_millis(row.get("updated_at")),
        })
    }
}

/// Whether a file starts like a zip archive with entries (a local file header).
async fn is_zip(path: &Path) -> bool {
    use tokio::io::AsyncReadExt;
    let Ok(mut file) = tokio::fs::File::open(path).await else {
        return false;
    };
    let mut magic = [0u8; 4];
    file.read_exact(&mut magic).await.is_ok() && magic == *b"PK\x03\x04"
}

/// Shutdown: leave the job to resume later. Cancelled or failed elsewhere: done.
fn stopped(run: &JobRun) -> Result<(), Failure> {
    if run.cancel.is_cancelled() {
        Err(Failure::Interrupted)
    } else {
        Ok(())
    }
}

fn retryable(error: impl std::fmt::Display) -> JobError {
    JobError::Retryable(error.to_string())
}

fn parse_stored_id(value: String) -> Result<Id, Failure> {
    value
        .parse()
        .map_err(|_| Failure::Fatal("Stored import data is invalid.".to_owned()))
}

fn page_failure(error: PageError) -> Failure {
    match error {
        PageError::Unavailable(error) => Failure::from(error),
        PageError::NotFound => Failure::Page("The page was deleted during the import.".to_owned()),
        PageError::VersionConflict { .. } => {
            Failure::Page("The page was edited during the import.".to_owned())
        }
        PageError::Locked => Failure::Page("The page was locked during the import.".to_owned()),
        other => Failure::Page(format!("The page could not be saved: {other}.")),
    }
}

struct PlannedItem {
    path: String,
    kind: ObsidianImportNodeKind,
    note_path: Option<String>,
    parent_path: Option<String>,
    title: String,
}

/// Selected nodes (a selected folder brings everything in it) in tree order. A node whose
/// parent is not selected becomes a top-level page of the import.
fn plan_items(
    tree: &ObsidianImportTree,
    selection: &ImportSelection,
) -> Result<Vec<PlannedItem>, ObsidianImportError> {
    let known: HashSet<&str> = tree.nodes.iter().map(|node| node.path.as_str()).collect();
    let mut selected: HashSet<String> = HashSet::new();
    match selection {
        ImportSelection::All => {
            selected.extend(tree.nodes.iter().map(|node| node.path.clone()));
        }
        ImportSelection::Paths(paths) => {
            if paths.is_empty() {
                return Err(ObsidianImportError::Invalid { field: "selection" });
            }
            for path in paths {
                if !known.contains(path.as_str()) {
                    return Err(ObsidianImportError::Invalid { field: "selection" });
                }
                selected.insert(path.clone());
            }
            // Tree order puts parents first, so one pass adds every descendant.
            for node in &tree.nodes {
                if node
                    .parent_path
                    .as_ref()
                    .is_some_and(|parent| selected.contains(parent))
                {
                    selected.insert(node.path.clone());
                }
            }
        }
    }
    Ok(tree
        .nodes
        .iter()
        .filter(|node| selected.contains(&node.path))
        .map(|node| PlannedItem {
            path: node.path.clone(),
            kind: node.kind,
            note_path: node.note_path.clone(),
            parent_path: node
                .parent_path
                .clone()
                .filter(|parent| selected.contains(parent)),
            title: node.title.clone(),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::obsidian::scan::ObsidianImportNode;

    fn node(path: &str, kind: ObsidianImportNodeKind, parent: Option<&str>) -> ObsidianImportNode {
        ObsidianImportNode {
            path: path.to_owned(),
            kind,
            title: path.to_owned(),
            note_path: None,
            parent_path: parent.map(str::to_owned),
            child_count: 0,
        }
    }

    #[test]
    fn plans_selection_with_descendants_and_new_roots() {
        let tree = ObsidianImportTree {
            nodes: vec![
                node("A", ObsidianImportNodeKind::Folder, None),
                node("A/B", ObsidianImportNodeKind::Folder, Some("A")),
                node("A/B/n.md", ObsidianImportNodeKind::Note, Some("A/B")),
                node("x.md", ObsidianImportNodeKind::Note, None),
            ],
            ..ObsidianImportTree::default()
        };
        assert_eq!(plan_items(&tree, &ImportSelection::All).unwrap().len(), 4);
        let subset = plan_items(&tree, &ImportSelection::Paths(vec!["A/B".to_owned()])).unwrap();
        let paths: Vec<_> = subset.iter().map(|item| item.path.as_str()).collect();
        assert_eq!(paths, ["A/B", "A/B/n.md"]);
        assert_eq!(
            subset[0].parent_path, None,
            "a child without its parent is a root"
        );
        assert_eq!(subset[1].parent_path.as_deref(), Some("A/B"));
        assert!(matches!(
            plan_items(&tree, &ImportSelection::Paths(vec!["nope".to_owned()])),
            Err(ObsidianImportError::Invalid { .. })
        ));
    }
}
