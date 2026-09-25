//! Notion import: a member pastes a Notion token, a `notion.scan` job lists what the token can
//! see as a page tree, the member picks everything or some pages plus a destination, and a
//! `notion.import` job creates the Orbit pages.
//!
//! State machine (`notion_imports.status`):
//! `scanning` → `ready` → `queued` → `importing` → `completed`; any active state can become
//! `failed` or `cancelled`, and a `ready` import that is not started within 24 hours becomes
//! `expired`. The token is stored only as ciphertext under the app key and is wiped (NULL) in
//! every final state (a CHECK constraint enforces it).
//!
//! Jobs carry only the import id. Progress lives in `notion_import_items`, so a restarted job
//! continues where it stopped: pass 1 creates every page (parents first, each with an id recorded
//! before the page exists), pass 2 fetches each page's blocks, downloads its Notion-hosted files
//! as page files, converts the blocks with links rewritten to the new pages, and saves content,
//! cover and child order. A page that fails is recorded and the import continues; a revoked token
//! fails the import. When an import fails or is cancelled, the pages pass 1 created but pass 2
//! never filled are moved to the trash (audited as `page.deleted`); filled pages stay.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use orbit_platform::{
    Database, Id, Job, JobError, JobKind, JobKindRegistrationError, JobQueue, JobStore,
    TimestampMillis, Worker,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::Row;
use thiserror::Error;
use tokio_util::sync::CancellationToken;
use utoipa::ToSchema;

use super::client::{NotionClient, NotionClientConfig, NotionError};
use super::convert::{
    ConvertContext, ConvertReport, ConvertedPage, MapResolver, PageCover, ResolvedFile,
    collect_file_refs, convert_page,
};
use super::files::sanitize_file_name;
use super::model::{BlockNode, NotionPage, normalize_id};
pub use super::scan::{NotionImportNode, NotionImportNodeKind, NotionImportTree};
use super::scan::{ScanControl, build_tree};
use crate::repositories::page_files::{PageFileRepository, page_file_url};
use crate::repositories::page_versions::SaveOrigin;
use crate::repositories::pages::{
    CreatePage, PageChanges, PageError, PageRepository, PageSpace, SpaceRequest,
};
use crate::secret_box::{decrypt_secret, encrypt_secret};

/// Job kind names.
pub const SCAN_JOB: &str = "notion.scan";
pub const IMPORT_JOB: &str = "notion.import";
/// Most pages one import lists and imports.
pub const MAX_PAGES: usize = 5_000;
/// A scanned import waits this long for "start" before its token is deleted.
pub const READY_TTL: Duration = Duration::from_secs(24 * 60 * 60);
/// An active import that made no progress for this long is failed by the cleanup.
const STALLED_AFTER: Duration = Duration::from_secs(24 * 60 * 60);
const MAX_TOKEN_CHARS: usize = 1_024;
const MAX_TITLE_CHARS: usize = 500;
const MAX_TITLE_BYTES: usize = 2_000;
const MAX_ICON_CHARS: usize = 64;
const MAX_COVER_URL_CHARS: usize = 2_048;
const MAX_FAILURES_LISTED: i64 = 200;
const MAX_LISTED_IMPORTS: i64 = 20;
const ACTIVE: &str = "('scanning', 'queued', 'importing')";
const OPEN: &str = "('scanning', 'ready', 'queued', 'importing')";

/// Both import job kinds: two at a time globally, a few retries for Notion outages.
#[must_use]
pub fn scan_job_kind() -> JobKind {
    notion_job_kind(SCAN_JOB)
}

#[must_use]
pub fn import_job_kind() -> JobKind {
    notion_job_kind(IMPORT_JOB)
}

fn notion_job_kind(name: &str) -> JobKind {
    JobKind::new(name)
        .with_concurrency_limit(2)
        .with_retry_policy(
            5,
            vec![
                Duration::from_secs(30),
                Duration::from_secs(2 * 60),
                Duration::from_secs(10 * 60),
                Duration::from_secs(30 * 60),
            ],
        )
        .with_sensitive_fields(["token"])
}

/// Registers the `notion.scan` and `notion.import` handlers.
pub fn register_jobs(
    worker: Worker,
    service: NotionImportService,
) -> Result<Worker, JobKindRegistrationError> {
    let scan = service.clone();
    worker
        .with_handler(scan_job_kind(), move |context| {
            let service = scan.clone();
            async move {
                let Some(import_id) = payload_import_id(&context.job.payload) else {
                    return Err(JobError::Permanent(
                        "invalid notion.scan payload".to_owned(),
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
            async move {
                let Some(import_id) = payload_import_id(&context.job.payload) else {
                    return Err(JobError::Permanent(
                        "invalid notion.import payload".to_owned(),
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

/// One job attempt: `cancel` fires on server shutdown (the job resumes later), and
/// `last_attempt` turns a transient Notion failure into a failed import instead of a retry.
#[derive(Clone, Debug, Default)]
pub struct JobRun {
    pub cancel: CancellationToken,
    pub last_attempt: bool,
}

/// Service settings.
#[derive(Clone, Debug)]
pub struct NotionImportSettings {
    /// Notion client settings for scan and import jobs (API origin, request rate).
    pub client: NotionClientConfig,
    /// The server app key; without it no import can store its token.
    pub app_key: Option<[u8; 32]>,
    /// Largest file the import downloads (the upload limit).
    pub max_file_bytes: u64,
}

#[derive(Clone)]
pub struct NotionImportService {
    inner: Arc<Inner>,
}

struct Inner {
    database: Database,
    pages: PageRepository,
    files: PageFileRepository,
    jobs: JobQueue,
    settings: NotionImportSettings,
    /// Imports whose `notion.import` job runs in this process right now.
    running: Mutex<HashSet<Id>>,
}

/// Marks an import as running in this process until dropped.
struct RunningGuard<'a> {
    running: &'a Mutex<HashSet<Id>>,
    import_id: Id,
}

impl<'a> RunningGuard<'a> {
    fn enter(running: &'a Mutex<HashSet<Id>>, import_id: Id) -> Self {
        running
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(import_id);
        Self { running, import_id }
    }
}

impl Drop for RunningGuard<'_> {
    fn drop(&mut self) {
        self.running
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&self.import_id);
    }
}

/// Errors of the import API.
#[derive(Debug, Error)]
pub enum ImportError {
    #[error("import not found")]
    NotFound,
    #[error("the app key is not configured")]
    AppKeyMissing,
    #[error("the Notion token is invalid")]
    TokenInvalid,
    #[error("Notion is unavailable")]
    NotionUnavailable,
    #[error("another import is in progress")]
    InProgress,
    #[error("the import is not in a state that allows this")]
    StateConflict,
    #[error("the selection has more than {MAX_PAGES} pages")]
    TooLarge,
    #[error("invalid input: {field}")]
    Invalid { field: &'static str },
    #[error("destination page not found")]
    PageNotFound,
    #[error("destination teamspace not found")]
    TeamspaceNotFound,
    #[error("import storage is unavailable")]
    Unavailable(String),
}

impl From<sqlx::Error> for ImportError {
    fn from(error: sqlx::Error) -> Self {
        Self::Unavailable(error.to_string())
    }
}

/// `notion_imports.status`.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum NotionImportStatus {
    /// The scan job is listing what the token can see.
    Scanning,
    /// The tree is ready; waiting for `start` (the token is deleted after 24 hours).
    Ready,
    /// Started; waiting for a job slot.
    Queued,
    Importing,
    Completed,
    Failed,
    Cancelled,
    /// Not started within 24 hours; the token was deleted.
    Expired,
}

impl NotionImportStatus {
    fn parse(value: &str) -> Result<Self, ImportError> {
        Ok(match value {
            "scanning" => Self::Scanning,
            "ready" => Self::Ready,
            "queued" => Self::Queued,
            "importing" => Self::Importing,
            "completed" => Self::Completed,
            "failed" => Self::Failed,
            "cancelled" => Self::Cancelled,
            "expired" => Self::Expired,
            _ => return Err(ImportError::Unavailable("unknown import status".to_owned())),
        })
    }
}

#[derive(Clone, Copy, Debug, Serialize, ToSchema)]
pub struct NotionImportProgress {
    /// Pages to import (pages, databases and database rows).
    pub total: u64,
    pub done: u64,
    pub failed: u64,
}

/// Where the import puts its root pages.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct NotionImportDestination {
    /// `null` for the importer's private space.
    #[schema(value_type = Option<String>, required = true)]
    pub teamspace_id: Option<Id>,
    pub private: bool,
    /// The page the imported root pages go under, if any.
    #[schema(value_type = Option<String>, required = true)]
    pub parent_page_id: Option<Id>,
}

/// A page the import could not create or fill.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct NotionImportFailure {
    pub notion_id: String,
    pub title: String,
    pub error: String,
}

/// What the conversion could not carry over, summed over all imported pages.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct NotionImportReport {
    /// Blocks dropped, by Notion type.
    pub skipped: BTreeMap<String, u32>,
    /// Blocks converted with information loss, by kind.
    pub lossy: BTreeMap<String, u32>,
    /// Links to Notion pages that were not part of the import (they still point to Notion).
    pub unresolved_page_links: u32,
    /// Notion-hosted files that could not be downloaded.
    pub missing_files: u32,
    /// Top-level blocks cut from pages larger than the page size limit.
    pub truncated_blocks: u32,
    /// Files downloaded and attached to the new pages.
    pub files_imported: u32,
    /// Selected pages this member imported before (the import creates new copies).
    pub previously_imported: u32,
    /// Pages the import created but never filled because it failed or was cancelled; they
    /// were moved to the trash (a restored page no longer counts).
    pub unfilled_pages_trashed: u32,
    /// The first 200 failed pages.
    pub failures: Vec<NotionImportFailure>,
}

/// A Notion import. Never contains the token.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct NotionImport {
    #[schema(value_type = String)]
    pub id: Id,
    #[schema(value_type = String)]
    pub workspace_id: Id,
    pub status: NotionImportStatus,
    /// The Notion workspace name (connection tokens) or the token owner's name (personal
    /// access tokens).
    #[schema(required = true)]
    pub notion_workspace_name: Option<String>,
    pub progress: NotionImportProgress,
    /// Why the import failed or expired.
    #[schema(required = true)]
    pub error: Option<String>,
    /// Set once the import was started.
    #[schema(required = true)]
    pub destination: Option<NotionImportDestination>,
    /// The scan result, once ready. Only in `GET …/imports/notion/{id}?include=tree`; `null`
    /// everywhere else (the default detail, lists, create, start and cancel), so polling
    /// stays light.
    #[schema(required = true)]
    pub tree: Option<NotionImportTree>,
    /// Conversion summary, once the import started.
    #[schema(required = true)]
    pub report: Option<NotionImportReport>,
    /// The imported top-level pages that still exist, in order (for "Open imported pages").
    #[schema(value_type = Vec<String>)]
    pub root_page_ids: Vec<Id>,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: TimestampMillis,
    #[schema(value_type = String, format = DateTime)]
    pub updated_at: TimestampMillis,
}

/// Which pages to import.
#[derive(Clone, Debug)]
pub enum ImportSelection {
    All,
    /// These nodes and all their descendants.
    NotionIds(Vec<String>),
}

/// The requested destination; with a parent page the pages join the parent's space.
#[derive(Clone, Debug, Default)]
pub struct ImportDestination {
    pub teamspace_id: Option<Id>,
    pub private: Option<bool>,
    pub parent_page_id: Option<Id>,
}

#[derive(Default, Deserialize, Serialize)]
struct StoredReport {
    #[serde(flatten)]
    convert: ConvertReport,
    #[serde(default)]
    files_imported: u32,
    #[serde(default)]
    previously_imported: u32,
}

#[derive(Deserialize, Serialize)]
struct StoredSelection {
    all: bool,
    #[serde(default)]
    notion_ids: Vec<String>,
    #[serde(default)]
    parent_page_id: Option<String>,
}

#[derive(Clone, Debug)]
struct Item {
    notion_id: String,
    kind: String,
    parent_notion_id: Option<String>,
    title: String,
    icon: Option<String>,
    page_id: Option<Id>,
    status: String,
}

/// Why one page could not be imported, and what that means for the import.
enum Failure {
    /// Server shutdown: the job is requeued and resumes; never fails the import.
    Interrupted,
    /// Record on the page and continue.
    Page(String),
    /// Stop the import (revoked token, lost access, missing destination).
    Fatal(String),
    /// Notion is overloaded or unreachable: retry the job later.
    Transient(String),
}

impl From<NotionError> for Failure {
    fn from(error: NotionError) -> Self {
        match &error {
            NotionError::Unauthorized { .. } => {
                Self::Fatal("The Notion token was revoked or is no longer valid.".to_owned())
            }
            NotionError::Blocked { .. } => {
                Self::Fatal("Notion blocked API access for this token.".to_owned())
            }
            NotionError::RateLimited { .. } | NotionError::Http(_) => {
                Self::Transient(error.to_string())
            }
            NotionError::Api { status, .. } if *status >= 500 => Self::Transient(error.to_string()),
            _ => Self::Page(error.to_string()),
        }
    }
}

impl From<sqlx::Error> for Failure {
    fn from(error: sqlx::Error) -> Self {
        Self::Transient(format!("database error: {error}"))
    }
}

impl NotionImportService {
    #[must_use]
    pub fn new(
        database: Database,
        files: PageFileRepository,
        settings: NotionImportSettings,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                pages: PageRepository::new(database.clone()),
                jobs: JobQueue::new(JobStore::new(database.clone())),
                database,
                files,
                settings,
                running: Mutex::new(HashSet::new()),
            }),
        }
    }

    fn database(&self) -> &Database {
        &self.inner.database
    }

    /// Validates `token` with Notion (`GET /v1/users/me`), stores it encrypted and queues the
    /// scan. A member has at most one active import; their unstarted scans in this workspace
    /// are cancelled (their tokens deleted).
    pub async fn create(
        &self,
        workspace_id: Id,
        user_id: Id,
        token: &str,
    ) -> Result<NotionImport, ImportError> {
        self.require_member(workspace_id, user_id).await?;
        let key = self
            .inner
            .settings
            .app_key
            .ok_or(ImportError::AppKeyMissing)?;
        let token = token.trim();
        if token.is_empty()
            || token.chars().count() > MAX_TOKEN_CHARS
            || !token.chars().all(|c| c.is_ascii_graphic())
        {
            return Err(ImportError::TokenInvalid);
        }
        let now = TimestampMillis::now();
        self.expire_stale(now).await?;
        if self.has_active_import(user_id, None).await? {
            return Err(ImportError::InProgress);
        }
        let validation = NotionClientConfig {
            max_attempts: 2,
            request_timeout: Duration::from_secs(20),
            max_retry_after: Duration::from_secs(5),
            ..self.inner.settings.client.clone()
        };
        let client = NotionClient::new(token, validation).map_err(|_| ImportError::TokenInvalid)?;
        let me = match client.me().await {
            Ok(me) => Some(me),
            // A connection without the "read user information" capability.
            Err(NotionError::RestrictedResource { .. }) => None,
            Err(NotionError::Unauthorized { .. } | NotionError::Config(_)) => {
                return Err(ImportError::TokenInvalid);
            }
            Err(error) => {
                tracing::warn!(error = %error, "Notion token validation failed");
                return Err(ImportError::NotionUnavailable);
            }
        };
        let name = me
            .as_ref()
            .and_then(|me| {
                me.workspace_name()
                    .map(str::to_owned)
                    .or_else(|| me.name.clone().filter(|name| !name.trim().is_empty()))
            })
            .map(|name| clamp(&name, 200, 800));
        let ciphertext = encrypt_secret(&key, token)
            .map_err(|()| ImportError::Unavailable("token encryption failed".to_owned()))?;

        let id = Id::new_v7();
        let mut tx = self.database().immediate_transaction().await?;
        let active: i64 = sqlx::query_scalar(&format!(
            "SELECT COUNT(*) FROM notion_imports WHERE user_id = ? AND status IN {ACTIVE}"
        ))
        .bind(user_id.to_string())
        .fetch_one(&mut *tx)
        .await?;
        if active > 0 {
            return Err(ImportError::InProgress);
        }
        sqlx::query(
            "UPDATE notion_imports SET status = 'cancelled', token_ciphertext = NULL, updated_at = ? \
             WHERE user_id = ? AND workspace_id = ? AND status = 'ready'",
        )
        .bind(now.as_millis())
        .bind(user_id.to_string())
        .bind(workspace_id.to_string())
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO notion_imports (id, workspace_id, user_id, status, token_ciphertext, \
             notion_workspace_name, created_at, updated_at) VALUES (?, ?, ?, 'scanning', ?, ?, ?, ?)",
        )
        .bind(id.to_string())
        .bind(workspace_id.to_string())
        .bind(user_id.to_string())
        .bind(ciphertext)
        .bind(name)
        .bind(now.as_millis())
        .bind(now.as_millis())
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        if let Err(error) = self.enqueue(scan_job_kind(), id, workspace_id).await {
            self.finish(id, "failed", Some("The scan could not be queued."))
                .await?;
            return Err(error);
        }
        self.get(workspace_id, user_id, id, false).await
    }

    /// The member's recent imports in this workspace, newest first (no trees).
    pub async fn list(
        &self,
        workspace_id: Id,
        user_id: Id,
    ) -> Result<Vec<NotionImport>, ImportError> {
        self.require_member(workspace_id, user_id).await?;
        self.expire_stale(TimestampMillis::now()).await?;
        let ids: Vec<String> = sqlx::query_scalar(
            "SELECT id FROM notion_imports WHERE workspace_id = ? AND user_id = ? \
             ORDER BY created_at DESC, id DESC LIMIT ?",
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
                .map_err(|_| ImportError::Unavailable("invalid import id".to_owned()))?;
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
    ) -> Result<NotionImport, ImportError> {
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
    ) -> Result<NotionImport, ImportError> {
        self.require_member(workspace_id, user_id).await?;
        self.expire_stale(TimestampMillis::now()).await?;
        let row = sqlx::query(
            "SELECT status, tree_json FROM notion_imports WHERE id = ? AND workspace_id = ? AND user_id = ?",
        )
        .bind(import_id.to_string())
        .bind(workspace_id.to_string())
        .bind(user_id.to_string())
        .fetch_optional(self.database().pool())
        .await?
        .ok_or(ImportError::NotFound)?;
        if row.get::<String, _>("status") != "ready" {
            return Err(ImportError::StateConflict);
        }
        let tree: NotionImportTree = serde_json::from_str(
            &row.get::<Option<String>, _>("tree_json")
                .unwrap_or_default(),
        )
        .map_err(|_| ImportError::Unavailable("invalid stored tree".to_owned()))?;

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
                    ImportError::PageNotFound
                }
                PageError::NotFound => ImportError::NotFound,
                PageError::TeamspaceNotFound => ImportError::TeamspaceNotFound,
                PageError::Invalid { field } => ImportError::Invalid { field },
                other => ImportError::Unavailable(other.to_string()),
            })?;

        let plan = plan_items(&tree, &selection)?;
        if plan.len() > MAX_PAGES {
            return Err(ImportError::TooLarge);
        }
        let previously: HashSet<String> = sqlx::query_scalar(
            "SELECT DISTINCT items.notion_id FROM notion_import_items AS items \
             JOIN notion_imports ON notion_imports.id = items.import_id \
             WHERE notion_imports.workspace_id = ? AND notion_imports.user_id = ? \
             AND notion_imports.id <> ? AND items.status = 'done'",
        )
        .bind(workspace_id.to_string())
        .bind(user_id.to_string())
        .bind(import_id.to_string())
        .fetch_all(self.database().pool())
        .await?
        .into_iter()
        .collect();
        let report = StoredReport {
            previously_imported: u32::try_from(
                plan.iter()
                    .filter(|item| previously.contains(&item.notion_id))
                    .count(),
            )
            .unwrap_or(u32::MAX),
            ..StoredReport::default()
        };
        let stored_selection = match &selection {
            ImportSelection::All => StoredSelection {
                all: true,
                notion_ids: Vec::new(),
                parent_page_id: destination.parent_page_id.map(|id| id.to_string()),
            },
            ImportSelection::NotionIds(ids) => StoredSelection {
                all: false,
                notion_ids: ids.clone(),
                parent_page_id: destination.parent_page_id.map(|id| id.to_string()),
            },
        };

        let now = TimestampMillis::now();
        let mut tx = self.database().immediate_transaction().await?;
        let active: i64 = sqlx::query_scalar(&format!(
            "SELECT COUNT(*) FROM notion_imports WHERE user_id = ? AND id <> ? AND status IN {ACTIVE}"
        ))
        .bind(user_id.to_string())
        .bind(import_id.to_string())
        .fetch_one(&mut *tx)
        .await?;
        if active > 0 {
            return Err(ImportError::InProgress);
        }
        let updated = sqlx::query(
            "UPDATE notion_imports SET status = 'queued', selection_json = ?, target_teamspace_id = ?, \
             target_private = ?, target_parent_page_id = ?, total = ?, done = 0, failed = 0, \
             report_json = ?, updated_at = ? \
             WHERE id = ? AND status = 'ready' AND token_ciphertext IS NOT NULL",
        )
        .bind(serde_json::to_string(&stored_selection).unwrap_or_default())
        .bind(space.teamspace_id().map(|id| id.to_string()))
        .bind(matches!(space, PageSpace::Private(_)))
        .bind(destination.parent_page_id.map(|id| id.to_string()))
        .bind(plan.len() as i64)
        .bind(serde_json::to_string(&report).unwrap_or_default())
        .bind(now.as_millis())
        .bind(import_id.to_string())
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if updated != 1 {
            return Err(ImportError::StateConflict);
        }
        for (position, item) in plan.iter().enumerate() {
            sqlx::query(
                "INSERT INTO notion_import_items (import_id, notion_id, kind, parent_notion_id, \
                 position, title, icon) VALUES (?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(import_id.to_string())
            .bind(&item.notion_id)
            .bind(&item.kind)
            .bind(&item.parent_notion_id)
            .bind(position as i64)
            .bind(&item.title)
            .bind(&item.icon)
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

    /// Cancels an open import and deletes its token. A running job stops before its next page.
    pub async fn cancel(
        &self,
        workspace_id: Id,
        user_id: Id,
        import_id: Id,
    ) -> Result<NotionImport, ImportError> {
        self.require_member(workspace_id, user_id).await?;
        let updated = sqlx::query(&format!(
            "UPDATE notion_imports SET status = 'cancelled', token_ciphertext = NULL, updated_at = ? \
             WHERE id = ? AND workspace_id = ? AND user_id = ? AND status IN {OPEN}"
        ))
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
            return Err(ImportError::StateConflict);
        }
        // A running job trashes its unfilled pages itself once it sees the cancellation (the
        // page it is filling right now must not be trashed under it). Otherwise (queued, or
        // waiting for a retry) clean up now.
        if !self.is_running_here(import_id) {
            self.trash_unfilled_pages(import_id).await?;
        }
        self.load_view(workspace_id, user_id, import_id, false)
            .await
    }

    /// Expires scanned imports that were not started within 24 hours and fails active imports
    /// that made no progress for 24 hours; both lose their token. Returns the rows changed.
    pub async fn expire_stale(&self, now: TimestampMillis) -> Result<u64, ImportError> {
        let ready_cutoff = now.as_millis() - READY_TTL.as_millis() as i64;
        let stalled_cutoff = now.as_millis() - STALLED_AFTER.as_millis() as i64;
        let expired = sqlx::query(
            "UPDATE notion_imports SET status = 'expired', token_ciphertext = NULL, \
             error = 'The import was not started within 24 hours.', updated_at = ? \
             WHERE status = 'ready' AND updated_at < ?",
        )
        .bind(now.as_millis())
        .bind(ready_cutoff)
        .execute(self.database().pool())
        .await?
        .rows_affected();
        let stalled = sqlx::query(&format!(
            "UPDATE notion_imports SET status = 'failed', token_ciphertext = NULL, \
             error = 'The import stopped making progress.', updated_at = ? \
             WHERE status IN {ACTIVE} AND updated_at < ?"
        ))
        .bind(now.as_millis())
        .bind(stalled_cutoff)
        .execute(self.database().pool())
        .await?
        .rows_affected();
        let dead = self.fail_imports_with_dead_jobs().await?;
        Ok(expired + stalled + dead)
    }

    /// When a job attempt that was not interrupted by shutdown fails for the last time, the job
    /// is about to go dead: finish the import now (failed, token deleted, unfilled pages
    /// trashed) instead of leaving it active until the stall rule.
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
            tracing::warn!(import_id = %import_id, error = %error, cleanup = %cleanup, "Notion import could not be finalized");
        }
        result
    }

    /// Fails active imports whose job is dead (for example its lease expired on the final
    /// attempt after a crash) and that have no live job left. Returns the imports finished.
    async fn fail_imports_with_dead_jobs(&self) -> Result<u64, ImportError> {
        let ids: Vec<String> = sqlx::query_scalar(&format!(
            "SELECT id FROM notion_imports WHERE status IN {ACTIVE} \
             AND EXISTS (SELECT 1 FROM jobs WHERE jobs.kind IN ('{SCAN_JOB}', '{IMPORT_JOB}') \
                 AND jobs.state = 'dead' \
                 AND json_extract(jobs.payload_json, '$.import_id') = notion_imports.id) \
             AND NOT EXISTS (SELECT 1 FROM jobs WHERE jobs.kind IN ('{SCAN_JOB}', '{IMPORT_JOB}') \
                 AND jobs.state IN ('queued', 'running') \
                 AND json_extract(jobs.payload_json, '$.import_id') = notion_imports.id)"
        ))
        .fetch_all(self.database().pool())
        .await?;
        let mut finished = 0;
        for id in ids {
            let Ok(import_id) = id.parse::<Id>() else {
                continue;
            };
            if self.is_running_here(import_id) {
                continue;
            }
            self.fail_open_import(import_id, "The import stopped after repeated errors.")
                .await?;
            finished += 1;
        }
        Ok(finished)
    }

    /// Trashes the unfilled pages of an open import, then fails it and deletes its token.
    async fn fail_open_import(&self, import_id: Id, message: &str) -> Result<(), sqlx::Error> {
        let status: Option<String> =
            sqlx::query_scalar("SELECT status FROM notion_imports WHERE id = ?")
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

    /// The `notion.scan` job: lists pages and data sources and stores the tree.
    pub async fn run_scan(&self, import_id: Id, run: &JobRun) -> Result<(), JobError> {
        let Some(row) =
            sqlx::query("SELECT status, token_ciphertext FROM notion_imports WHERE id = ?")
                .bind(import_id.to_string())
                .fetch_optional(self.database().pool())
                .await
                .map_err(retryable)?
        else {
            return Ok(());
        };
        if row.get::<String, _>("status") != "scanning" {
            return Ok(());
        }
        let client = match self.client_for(row.get("token_ciphertext")) {
            Ok(client) => client,
            Err(message) => {
                self.finish(import_id, "failed", Some(&message))
                    .await
                    .map_err(retryable)?;
                return Ok(());
            }
        };
        let control = ScanRun {
            database: self.database(),
            import_id,
            cancel: &run.cancel,
        };
        match build_tree(&client, &control).await {
            Ok(None) if run.cancel.is_cancelled() => {
                Err(JobError::Retryable("interrupted by shutdown".to_owned()))
            }
            // Cancelled (or expired) while scanning.
            Ok(None) => Ok(()),
            Ok(Some(tree)) => {
                sqlx::query(
                    "UPDATE notion_imports SET status = 'ready', tree_json = ?, updated_at = ? \
                     WHERE id = ? AND status = 'scanning'",
                )
                .bind(serde_json::to_string(&tree).map_err(retryable)?)
                .bind(TimestampMillis::now().as_millis())
                .bind(import_id.to_string())
                .execute(self.database().pool())
                .await
                .map_err(retryable)?;
                Ok(())
            }
            Err(error) => self.job_failure(import_id, Failure::from(error), run).await,
        }
    }

    /// The `notion.import` job: pass 1 creates the pages, pass 2 fills them. A failed or
    /// cancelled import trashes the pages it created but did not fill.
    pub async fn run_import(&self, import_id: Id, run: &JobRun) -> Result<(), JobError> {
        let _running = RunningGuard::enter(&self.inner.running, import_id);
        match self.import(import_id, run).await {
            Ok(()) => {}
            Err(failure) => self.job_failure(import_id, failure, run).await?,
        }
        let status: Option<String> =
            sqlx::query_scalar("SELECT status FROM notion_imports WHERE id = ?")
                .bind(import_id.to_string())
                .fetch_optional(self.database().pool())
                .await
                .map_err(retryable)?;
        if matches!(status.as_deref(), Some("failed" | "cancelled")) {
            self.trash_unfilled_pages(import_id)
                .await
                .map_err(retryable)?;
        }
        Ok(())
    }

    fn is_running_here(&self, import_id: Id) -> bool {
        self.inner
            .running
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains(&import_id)
    }

    /// Moves the pages that pass 1 created and pass 2 never filled (items still `created`) to
    /// the trash, one batch per topmost such page, as the importing member (audited as
    /// `page.deleted`). A subtree is kept when any live page in it has content or is not one of
    /// these pages (e.g. a page someone moved under it). Idempotent; returns the pages trashed.
    async fn trash_unfilled_pages(&self, import_id: Id) -> Result<u64, sqlx::Error> {
        let Some(row) =
            sqlx::query("SELECT workspace_id, user_id FROM notion_imports WHERE id = ?")
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
            "SELECT notion_id, parent_notion_id, page_id FROM notion_import_items \
             WHERE import_id = ? AND status = 'created' AND page_id IS NOT NULL ORDER BY position",
        )
        .bind(import_id.to_string())
        .fetch_all(self.database().pool())
        .await?;
        let unfilled_ids: HashSet<&str> = unfilled.iter().map(|(id, _, _)| id.as_str()).collect();
        let request_id = format!("notion-import-{import_id}");
        let mut trashed = 0;
        for (_, parent, page_id) in &unfilled {
            if parent
                .as_deref()
                .is_some_and(|parent| unfilled_ids.contains(parent))
            {
                continue; // Goes with its unfilled parent.
            }
            let Ok(page) = page_id.parse::<Id>() else {
                continue;
            };
            // (version, live pages in the subtree, of which kept: filled or not ours)
            let (version, size, kept): (Option<i64>, i64, i64) = sqlx::query_as(
                "WITH RECURSIVE subtree(id) AS ( \
                     SELECT id FROM pages WHERE id = ? AND deleted_at IS NULL \
                     UNION ALL \
                     SELECT pages.id FROM pages JOIN subtree ON pages.parent_id = subtree.id \
                     WHERE pages.deleted_at IS NULL \
                 ) \
                 SELECT (SELECT version FROM pages WHERE id = ?), COUNT(*), \
                        COALESCE(SUM(pages.content_json <> '[]' OR pages.id NOT IN ( \
                            SELECT page_id FROM notion_import_items \
                            WHERE import_id = ? AND status = 'created' AND page_id IS NOT NULL)), 0) \
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
            tracing::info!(import_id = %import_id, pages = trashed, "Notion import trashed unfilled pages");
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
                tracing::warn!(import_id = %import_id, error = %message, "Notion import will retry");
                Err(JobError::Retryable(message))
            }
            Failure::Transient(message) if run.cancel.is_cancelled() => {
                // Shutdown cut a request short: resume later instead of failing.
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
                Ok(())
            }
        }
    }

    async fn import(&self, import_id: Id, run: &JobRun) -> Result<(), Failure> {
        let Some(row) = sqlx::query(
            "SELECT workspace_id, user_id, status, token_ciphertext, selection_json, \
             target_teamspace_id, target_private, report_json FROM notion_imports WHERE id = ?",
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
        let client = self
            .client_for(row.get("token_ciphertext"))
            .map_err(Failure::Fatal)?;
        sqlx::query(
            "UPDATE notion_imports SET status = 'importing', updated_at = ? WHERE id = ? AND status = 'queued'",
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

        let mut items = self.load_items(import_id).await?;

        // Pass 1: create every page, parents first.
        for index in 0..items.len() {
            if items[index].status != "pending" {
                continue;
            }
            if self.should_stop(import_id, run).await? {
                return stopped(run);
            }
            let parent = items[index]
                .parent_notion_id
                .as_ref()
                .and_then(|parent| {
                    items.iter().find(|item| {
                        &item.notion_id == parent
                            && matches!(item.status.as_str(), "created" | "done")
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
                    self.fail_item(import_id, &items[index].notion_id, &message)
                        .await?;
                    items[index].status = "failed".to_owned();
                }
                Err(other) => return Err(other),
            }
        }

        // Pass 2: fill each page.
        let mut resolver = MapResolver::default();
        for item in &items {
            resolver
                .titles
                .insert(item.notion_id.clone(), item.title.clone());
            if matches!(item.status.as_str(), "created" | "done")
                && let Some(page_id) = item.page_id
            {
                resolver
                    .pages
                    .insert(item.notion_id.clone(), page_id.to_string());
            }
        }
        for index in 0..items.len() {
            if items[index].status != "created" {
                continue;
            }
            if self.should_stop(import_id, run).await? {
                return stopped(run);
            }
            let item = items[index].clone();
            let result = self
                .fill_item_page(
                    workspace_id,
                    user_id,
                    &item,
                    &items,
                    &client,
                    &mut resolver,
                    &mut report,
                )
                .await;
            match result {
                Ok(()) => {
                    sqlx::query(
                        "UPDATE notion_import_items SET status = 'done', error = NULL \
                         WHERE import_id = ? AND notion_id = ?",
                    )
                    .bind(import_id.to_string())
                    .bind(&item.notion_id)
                    .execute(self.database().pool())
                    .await?;
                    self.save_progress(import_id, &report, 1, 0).await?;
                    items[index].status = "done".to_owned();
                }
                Err(Failure::Page(message)) => {
                    self.fail_item(import_id, &item.notion_id, &message).await?;
                    self.save_progress(import_id, &report, 0, 0).await?;
                    items[index].status = "failed".to_owned();
                }
                Err(other) => {
                    self.save_progress(import_id, &report, 0, 0).await?;
                    return Err(other);
                }
            }
        }

        sqlx::query(
            "UPDATE notion_imports SET status = 'completed', token_ciphertext = NULL, updated_at = ? \
             WHERE id = ? AND status = 'importing'",
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
                    "UPDATE notion_import_items SET page_id = ? WHERE import_id = ? AND notion_id = ?",
                )
                .bind(page_id.to_string())
                .bind(import_id.to_string())
                .bind(&item.notion_id)
                .execute(self.database().pool())
                .await?;
                item.page_id = Some(page_id);
                page_id
            }
        };
        let exists: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pages WHERE id = ?")
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
                parent_id,
                space,
                title: clamp(&item.title, MAX_TITLE_CHARS, MAX_TITLE_BYTES),
                icon: item
                    .icon
                    .as_deref()
                    .map(|icon| clamp(icon, MAX_ICON_CHARS, 256)),
                position: None,
            };
            let request_id = format!("notion-import-{import_id}");
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
            "UPDATE notion_import_items SET status = 'created' WHERE import_id = ? AND notion_id = ?",
        )
        .bind(import_id.to_string())
        .bind(&item.notion_id)
        .execute(self.database().pool())
        .await?;
        sqlx::query("UPDATE notion_imports SET updated_at = ? WHERE id = ?")
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
        client: &NotionClient,
        resolver: &mut MapResolver,
        report: &mut StoredReport,
    ) -> Result<(), Failure> {
        let page_id = item
            .page_id
            .ok_or_else(|| Failure::Page("The page was not created.".to_owned()))?;
        let request_id = format!("notion-import-{page_id}");
        let children: Vec<Id> = items
            .iter()
            .filter(|child| child.parent_notion_id.as_deref() == Some(item.notion_id.as_str()))
            .filter(|child| matches!(child.status.as_str(), "created" | "done"))
            .filter_map(|child| child.page_id)
            .collect();
        let changes = if item.kind == "database" {
            PageChanges {
                content: Some(
                    children
                        .iter()
                        .map(|child| {
                            json!({
                                "id": Id::new_v7().to_string(),
                                "type": "page",
                                "props": {"pageId": child.to_string()},
                                "children": []
                            })
                        })
                        .collect(),
                ),
                origin: SaveOrigin::Import,
                ..PageChanges::default()
            }
        } else {
            let page = client.retrieve_page(&item.notion_id).await?;
            let tree = client.fetch_block_tree(&item.notion_id).await?;
            // Files an earlier attempt of this page already attached (a retry reuses them).
            let attached = self.attached_files(workspace_id, page_id).await?;
            let mut downloaded = HashSet::new();
            for file in collect_file_refs(Some(&page), &tree.blocks) {
                if !downloaded.insert(file.url.clone()) || resolver.files.contains_key(&file.url) {
                    continue;
                }
                let bytes = match client
                    .download_file(&file.url, self.inner.settings.max_file_bytes)
                    .await
                {
                    Ok(bytes) => bytes,
                    Err(NotionError::Unauthorized { .. }) => {
                        return Err(Failure::from(NotionError::Unauthorized {
                            message: String::new(),
                        }));
                    }
                    Err(error) => {
                        tracing::debug!(error = %error, "Notion file download skipped");
                        continue;
                    }
                };
                // A retried page reuses the files its earlier attempt already attached.
                let sha256 = format!("{:x}", Sha256::digest(&bytes.bytes));
                if let Some((_, _, existing)) = attached.iter().find(|(hash, size, _)| {
                    *hash == sha256 && usize::try_from(*size).ok() == Some(bytes.bytes.len())
                }) {
                    resolver.files.insert(file.url.clone(), existing.clone());
                    continue;
                }
                let name = sanitize_file_name(
                    file.name
                        .as_deref()
                        .filter(|name| !name.trim().is_empty())
                        .unwrap_or(&bytes.file_name),
                );
                match self
                    .inner
                    .files
                    .create_from_bytes(
                        workspace_id,
                        page_id,
                        user_id,
                        &name,
                        &bytes.bytes,
                        &request_id,
                    )
                    .await
                {
                    Ok(stored) => {
                        report.files_imported += 1;
                        resolver.files.insert(
                            file.url.clone(),
                            ResolvedFile {
                                url: stored.url,
                                name: stored.file_name,
                            },
                        );
                    }
                    Err(error) => {
                        tracing::debug!(error = %error, "Notion file could not be attached");
                    }
                }
            }
            let (converted, convert_report) = convert(&page, &tree.blocks, resolver);
            report.convert.merge(&convert_report);
            let cover_url = match &converted.cover {
                Some(PageCover::External { url })
                    if url.starts_with("https://")
                        && url.chars().count() <= MAX_COVER_URL_CHARS =>
                {
                    Some(url.clone())
                }
                Some(PageCover::File(file)) => {
                    resolver.files.get(&file.url).map(|file| file.url.clone())
                }
                _ => None,
            };
            // Notion files are per-page: forget them so the map stays small.
            resolver.files.clear();
            let mut ordered = Vec::new();
            child_page_ids(&tree.blocks, &mut ordered);
            let ordered: Vec<Id> = ordered
                .iter()
                .filter_map(|notion_id| resolver.pages.get(notion_id))
                .filter_map(|id| id.parse().ok())
                .collect();
            if !ordered.is_empty() {
                self.inner
                    .pages
                    .order_children(workspace_id, user_id, page_id, &ordered)
                    .await
                    .map_err(page_failure)?;
            }
            PageChanges {
                title: Some(clamp(&converted.title, MAX_TITLE_CHARS, MAX_TITLE_BYTES)),
                icon: Some(
                    converted
                        .icon
                        .as_deref()
                        .map(|icon| clamp(icon, MAX_ICON_CHARS, 256)),
                ),
                cover_url: cover_url.map(Some),
                cover_position: None,
                content: Some(converted.content),
                // The imported state becomes an `import` version in the page history.
                origin: SaveOrigin::Import,
            }
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
                changes,
                &request_id,
                TimestampMillis::now(),
            )
            .await
            .map_err(page_failure)?;
        Ok(())
    }

    /// The page's files with their content hash and size: `(sha256, byte_size, file)`.
    async fn attached_files(
        &self,
        workspace_id: Id,
        page_id: Id,
    ) -> Result<Vec<(String, i64, ResolvedFile)>, Failure> {
        let rows: Vec<(String, String, String, i64)> = sqlx::query_as(
            "SELECT page_files.id, page_files.file_name, attachment_blobs.sha256, \
             attachment_blobs.byte_size FROM page_files \
             JOIN attachment_blobs ON attachment_blobs.id = page_files.blob_id \
             WHERE page_files.workspace_id = ? AND page_files.page_id = ? \
             ORDER BY page_files.created_at, page_files.id",
        )
        .bind(workspace_id.to_string())
        .bind(page_id.to_string())
        .fetch_all(self.database().pool())
        .await?;
        Ok(rows
            .into_iter()
            .filter_map(|(file_id, name, sha256, size)| {
                let file_id = file_id.parse().ok()?;
                Some((
                    sha256,
                    size,
                    ResolvedFile {
                        url: page_file_url(workspace_id, page_id, file_id),
                        name,
                    },
                ))
            })
            .collect())
    }

    async fn should_stop(&self, import_id: Id, run: &JobRun) -> Result<bool, Failure> {
        if run.cancel.is_cancelled() {
            return Ok(true);
        }
        let status: Option<String> =
            sqlx::query_scalar("SELECT status FROM notion_imports WHERE id = ?")
                .bind(import_id.to_string())
                .fetch_optional(self.database().pool())
                .await?;
        Ok(status.as_deref() != Some("importing"))
    }

    async fn fail_item(
        &self,
        import_id: Id,
        notion_id: &str,
        message: &str,
    ) -> Result<(), Failure> {
        let mut tx = self.database().immediate_transaction().await?;
        let changed = sqlx::query(
            "UPDATE notion_import_items SET status = 'failed', error = ? \
             WHERE import_id = ? AND notion_id = ? AND status <> 'failed'",
        )
        .bind(clamp(message, 500, 2_000))
        .bind(import_id.to_string())
        .bind(notion_id)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        sqlx::query("UPDATE notion_imports SET failed = failed + ?, updated_at = ? WHERE id = ?")
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
        failed: i64,
    ) -> Result<(), Failure> {
        sqlx::query(
            "UPDATE notion_imports SET done = done + ?, failed = failed + ?, report_json = ?, \
             updated_at = ? WHERE id = ?",
        )
        .bind(done)
        .bind(failed)
        .bind(serde_json::to_string(report).unwrap_or_default())
        .bind(TimestampMillis::now().as_millis())
        .bind(import_id.to_string())
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
        sqlx::query(&format!(
            "UPDATE notion_imports SET status = ?, token_ciphertext = NULL, error = ?, updated_at = ? \
             WHERE id = ? AND status IN {OPEN}"
        ))
        .bind(status)
        .bind(error.map(|error| clamp(error, 500, 2_000)))
        .bind(TimestampMillis::now().as_millis())
        .bind(import_id.to_string())
        .execute(self.database().pool())
        .await?;
        Ok(())
    }

    fn client_for(&self, ciphertext: Option<Vec<u8>>) -> Result<NotionClient, String> {
        let key = self
            .inner
            .settings
            .app_key
            .ok_or_else(|| "The server app key is not configured.".to_owned())?;
        let ciphertext = ciphertext.ok_or_else(|| "The Notion token was deleted.".to_owned())?;
        let token = decrypt_secret(&key, &ciphertext)
            .map_err(|()| "The Notion token could not be decrypted.".to_owned())?;
        NotionClient::new(&token, self.inner.settings.client.clone())
            .map_err(|_| "The Notion token is invalid.".to_owned())
    }

    async fn enqueue(
        &self,
        kind: JobKind,
        import_id: Id,
        workspace_id: Id,
    ) -> Result<(), ImportError> {
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
            .map_err(|error| ImportError::Unavailable(error.to_string()))
    }

    async fn require_member(&self, workspace_id: Id, user_id: Id) -> Result<(), ImportError> {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM memberships JOIN workspaces ON workspaces.id = memberships.workspace_id \
             WHERE memberships.workspace_id = ? AND memberships.user_id = ? AND workspaces.deleted_at IS NULL",
        )
        .bind(workspace_id.to_string())
        .bind(user_id.to_string())
        .fetch_one(self.database().pool())
        .await?;
        if count == 1 {
            Ok(())
        } else {
            Err(ImportError::NotFound)
        }
    }

    async fn has_active_import(
        &self,
        user_id: Id,
        except: Option<Id>,
    ) -> Result<bool, ImportError> {
        let count: i64 = sqlx::query_scalar(&format!(
            "SELECT COUNT(*) FROM notion_imports WHERE user_id = ? AND id IS NOT ? AND status IN {ACTIVE}"
        ))
        .bind(user_id.to_string())
        .bind(except.map(|id| id.to_string()))
        .fetch_one(self.database().pool())
        .await?;
        Ok(count > 0)
    }

    async fn load_items(&self, import_id: Id) -> Result<Vec<Item>, Failure> {
        let rows = sqlx::query(
            "SELECT notion_id, kind, parent_notion_id, title, icon, page_id, status \
             FROM notion_import_items WHERE import_id = ? ORDER BY position",
        )
        .bind(import_id.to_string())
        .fetch_all(self.database().pool())
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok(Item {
                    notion_id: row.get("notion_id"),
                    kind: row.get("kind"),
                    parent_notion_id: row.get("parent_notion_id"),
                    title: row.get("title"),
                    icon: row.get("icon"),
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
    ) -> Result<NotionImport, ImportError> {
        let row = sqlx::query(
            "SELECT id, workspace_id, status, notion_workspace_name, \
             CASE WHEN ? THEN tree_json END AS tree_json, selection_json, target_teamspace_id, \
             target_private, target_parent_page_id, total, done, failed, \
             report_json, error, created_at, updated_at FROM notion_imports \
             WHERE id = ? AND workspace_id = ? AND user_id = ?",
        )
        .bind(with_tree)
        .bind(import_id.to_string())
        .bind(workspace_id.to_string())
        .bind(user_id.to_string())
        .fetch_optional(self.database().pool())
        .await?
        .ok_or(ImportError::NotFound)?;
        let invalid = |_| ImportError::Unavailable("invalid stored import".to_owned());
        let status = NotionImportStatus::parse(&row.get::<String, _>("status"))?;
        let started = row.get::<Option<String>, _>("selection_json").is_some();
        // `tree_json` is only read `with_tree` (see the query).
        let tree = row
            .get::<Option<String>, _>("tree_json")
            .map(|json| serde_json::from_str::<NotionImportTree>(&json))
            .transpose()
            .map_err(|_| ImportError::Unavailable("invalid stored tree".to_owned()))?;
        let destination = if started {
            Some(NotionImportDestination {
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
                "SELECT notion_id, title, error FROM notion_import_items \
                 WHERE import_id = ? AND status = 'failed' ORDER BY position LIMIT ?",
            )
            .bind(import_id.to_string())
            .bind(MAX_FAILURES_LISTED)
            .fetch_all(self.database().pool())
            .await?
            .into_iter()
            .map(|row| NotionImportFailure {
                notion_id: row.get("notion_id"),
                title: row.get("title"),
                error: row.get::<Option<String>, _>("error").unwrap_or_default(),
            })
            .collect();
            let unfilled_pages_trashed: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM notion_import_items AS items \
                 JOIN pages ON pages.id = items.page_id \
                 WHERE items.import_id = ? AND items.status = 'created' \
                 AND pages.deleted_at IS NOT NULL",
            )
            .bind(import_id.to_string())
            .fetch_one(self.database().pool())
            .await?;
            Some(NotionImportReport {
                skipped: stored.convert.skipped,
                lossy: stored.convert.lossy,
                unresolved_page_links: stored.convert.unresolved_page_links,
                missing_files: stored.convert.missing_files,
                truncated_blocks: stored.convert.truncated_blocks,
                files_imported: stored.files_imported,
                previously_imported: stored.previously_imported,
                unfilled_pages_trashed: u32::try_from(unfilled_pages_trashed).unwrap_or_default(),
                failures,
            })
        } else {
            None
        };
        let root_page_ids = sqlx::query_scalar::<_, String>(
            "SELECT items.page_id FROM notion_import_items AS items \
             JOIN pages ON pages.id = items.page_id \
             WHERE items.import_id = ? AND items.parent_notion_id IS NULL \
             AND items.status IN ('created', 'done') AND pages.workspace_id = ? \
             AND pages.deleted_at IS NULL AND (pages.teamspace_id IS NOT NULL OR pages.owner_id = ?) \
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
        Ok(NotionImport {
            id: row.get::<String, _>("id").parse().map_err(invalid)?,
            workspace_id: row
                .get::<String, _>("workspace_id")
                .parse()
                .map_err(invalid)?,
            status,
            notion_workspace_name: row.get("notion_workspace_name"),
            progress: NotionImportProgress {
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
        other => Failure::Page(format!("The page could not be saved: {other}.")),
    }
}

/// Cuts `value` to `max_chars` characters and `max_bytes` bytes (on a character boundary).
pub(crate) fn clamp(value: &str, max_chars: usize, max_bytes: usize) -> String {
    let mut out = String::new();
    for (count, c) in value.chars().enumerate() {
        if count >= max_chars || out.len() + c.len_utf8() > max_bytes {
            break;
        }
        out.push(c);
    }
    out
}

/// Runs the (synchronous, non-`Send`) converter outside any await point.
fn convert(
    page: &NotionPage,
    blocks: &[BlockNode],
    resolver: &MapResolver,
) -> (ConvertedPage, ConvertReport) {
    let mut context = ConvertContext::new(resolver);
    let converted = convert_page(page, blocks, &mut context);
    (converted, context.into_report())
}

/// `child_page` / `child_database` ids in document order (inside toggles and columns too).
fn child_page_ids(nodes: &[BlockNode], out: &mut Vec<String>) {
    for node in nodes {
        if node.block.is_page_boundary()
            && !node.block.in_trash
            && let Some(id) = normalize_id(&node.block.id)
        {
            out.push(id);
        }
        child_page_ids(&node.children, out);
    }
}

struct PlannedItem {
    notion_id: String,
    kind: String,
    parent_notion_id: Option<String>,
    title: String,
    icon: Option<String>,
}

/// Selected nodes (a selected node brings all its descendants) in tree order. A node whose
/// parent is not selected becomes a root of the import.
fn plan_items(
    tree: &NotionImportTree,
    selection: &ImportSelection,
) -> Result<Vec<PlannedItem>, ImportError> {
    let kinds: HashMap<&str, NotionImportNodeKind> = tree
        .nodes
        .iter()
        .map(|node| (node.notion_id.as_str(), node.kind))
        .collect();
    let mut selected: HashSet<String> = HashSet::new();
    match selection {
        ImportSelection::All => {
            selected.extend(tree.nodes.iter().map(|node| node.notion_id.clone()));
        }
        ImportSelection::NotionIds(ids) => {
            if ids.is_empty() {
                return Err(ImportError::Invalid { field: "selection" });
            }
            for id in ids {
                let id = normalize_id(id).ok_or(ImportError::Invalid { field: "selection" })?;
                if !kinds.contains_key(id.as_str()) {
                    return Err(ImportError::Invalid { field: "selection" });
                }
                selected.insert(id);
            }
            // Tree order puts parents first, so one pass adds every descendant.
            for node in &tree.nodes {
                if node
                    .parent_id
                    .as_ref()
                    .is_some_and(|parent| selected.contains(parent))
                {
                    selected.insert(node.notion_id.clone());
                }
            }
        }
    }
    Ok(tree
        .nodes
        .iter()
        .filter(|node| selected.contains(&node.notion_id))
        .map(|node| {
            let parent = node
                .parent_id
                .clone()
                .filter(|parent| selected.contains(parent));
            let under_database = node
                .parent_id
                .as_deref()
                .and_then(|parent| kinds.get(parent))
                == Some(&NotionImportNodeKind::Database);
            PlannedItem {
                notion_id: node.notion_id.clone(),
                kind: match node.kind {
                    NotionImportNodeKind::Database => "database",
                    NotionImportNodeKind::Page if under_database => "row",
                    NotionImportNodeKind::Page => "page",
                }
                .to_owned(),
                parent_notion_id: parent,
                title: node.title.clone(),
                icon: node.icon.clone(),
            }
        })
        .collect())
}

/// Stops a scan on shutdown or when the import left `scanning` (cancelled, expired).
struct ScanRun<'a> {
    database: &'a Database,
    import_id: Id,
    cancel: &'a CancellationToken,
}

impl ScanControl for ScanRun<'_> {
    fn should_stop(&self) -> Pin<Box<dyn Future<Output = bool> + Send + '_>> {
        Box::pin(async move {
            if self.cancel.is_cancelled() {
                return true;
            }
            match sqlx::query_scalar::<_, String>("SELECT status FROM notion_imports WHERE id = ?")
                .bind(self.import_id.to_string())
                .fetch_optional(self.database.pool())
                .await
            {
                Ok(status) => status.as_deref() != Some("scanning"),
                // A database hiccup is not a reason to stop; the next check sees it again.
                Err(_) => false,
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str, kind: NotionImportNodeKind, parent: Option<&str>) -> NotionImportNode {
        NotionImportNode {
            notion_id: id.to_owned(),
            kind,
            title: id.to_owned(),
            icon: None,
            parent_id: parent.map(str::to_owned),
            child_count: 0,
        }
    }

    fn uuid(n: u32) -> String {
        format!("c0000000-0000-4000-8000-{n:012}")
    }

    #[test]
    fn plans_selection_with_descendants_and_new_roots() {
        let tree = NotionImportTree {
            nodes: vec![
                node(&uuid(1), NotionImportNodeKind::Page, None),
                node(&uuid(2), NotionImportNodeKind::Page, Some(&uuid(1))),
                node(&uuid(3), NotionImportNodeKind::Database, Some(&uuid(2))),
                node(&uuid(4), NotionImportNodeKind::Page, Some(&uuid(3))),
                node(&uuid(5), NotionImportNodeKind::Page, None),
            ],
            truncated: false,
            incomplete: false,
        };
        let all = plan_items(&tree, &ImportSelection::All).unwrap();
        assert_eq!(all.len(), 5);
        assert_eq!(all[3].kind, "row");
        assert_eq!(all[2].kind, "database");

        let subset = plan_items(
            &tree,
            &ImportSelection::NotionIds(vec![uuid(2).replace('-', "").to_uppercase()]),
        )
        .unwrap();
        let ids: Vec<_> = subset.iter().map(|item| item.notion_id.clone()).collect();
        assert_eq!(ids, [uuid(2), uuid(3), uuid(4)]);
        assert_eq!(
            subset[0].parent_notion_id, None,
            "a child without its parent is a root"
        );
        assert_eq!(
            subset[1].parent_notion_id.as_deref(),
            Some(uuid(2).as_str())
        );

        assert!(matches!(
            plan_items(&tree, &ImportSelection::NotionIds(vec![uuid(9)])),
            Err(ImportError::Invalid { .. })
        ));
        assert!(matches!(
            plan_items(&tree, &ImportSelection::NotionIds(Vec::new())),
            Err(ImportError::Invalid { .. })
        ));
    }

    #[test]
    fn clamps_on_character_boundaries() {
        assert_eq!(clamp("héllo", 3, 100), "hél");
        assert_eq!(clamp("ééé", 10, 3), "é");
        assert_eq!(clamp("", 3, 3), "");
    }
}
