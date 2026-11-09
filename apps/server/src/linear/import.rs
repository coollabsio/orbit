//! Linear import: a member pastes a Linear personal API key, a `linear.scan` job lists the teams
//! it can see, the member picks teams, and a `linear.import` job copies each team into a new
//! Orbit project.
//!
//! State machine (`linear_imports.status`), as for Asana imports:
//! `scanning` → `ready` → `queued` → `importing` → `completed`; any active state can become
//! `failed` or `cancelled`, and a `ready` import that is not started within 24 hours becomes
//! `expired`. The key is stored only as ciphertext under the app key and is wiped (NULL) in
//! every final state (a CHECK constraint enforces it).
//!
//! Jobs carry only the import id. Progress lives in `linear_import_items`, so a restarted job
//! continues where it stopped, and every step converges on a second run:
//! - Phase A, per selected team: create the Orbit project (its id recorded first), map the
//!   workflow states to statuses (same name reuses), and list its issues (parents first) and
//!   their relations as items.
//! - Phase B, per issue: create the Orbit task (its id recorded first) silently, without
//!   notifications.
//! - Phase C, per issue: uploaded files (downloaded with the key, attached, and their URLs in the
//!   description rewritten), then comments (skipping as many as the importer already wrote).
//! - Phase D, per relation: blocks, related and duplicate relations between imported issues. A
//!   relation Orbit cannot hold (one relation per pair of tasks, one duplicate per task, no
//!   duplicate chains) is skipped and counted.
//!
//! An issue that fails is recorded and the import continues; a revoked key fails the import.
//! What was imported before a failure or cancellation stays.
//!
//! A later import of the same team reuses what a finished earlier import in the workspace made:
//! its Orbit project and its tasks, while they exist. An issue the earlier import finished is
//! counted as done and not copied again.

use crate::import_notice::{ImportNotices, ImportSource};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::time::Duration;

use orbit_platform::Row;
use orbit_platform::{
    Backend, Database, Flag, Id, Job, JobError, JobKind, JobKindRegistrationError, JobQueue,
    JobStore, TimestampMillis, Worker,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;
use utoipa::ToSchema;

use super::client::{LinearClient, LinearClientConfig, LinearError};
use super::model::{LinearComment, LinearIssue};
pub use super::scan::{LinearImportTeam, LinearImportTree};
use super::scan::{MAX_TEAMS, build_tree};
use crate::import_people::{self, Match, Source};
use crate::notion::files::sanitize_file_name;
use crate::notion::import::clamp;
use crate::repositories::attachments::AttachmentRepository;
use crate::repositories::membership;
use crate::repositories::task_relations::NewTaskRelationType;
use crate::repositories::tasks::{
    CommentContent, CreateTask, TaskChanges, TaskError, TaskRepository, TaskUpdate,
};
use crate::secret_box::{decrypt_secret, encrypt_secret};
pub use crate::task_import::JobRun;
use crate::task_import::{
    Attached, Failure, ImportTables, LABEL_COLORS, NewProject, PROJECT_COLORS, attach_once,
    attachment_url, category_color, display_instant, ensure_project, ensure_writable,
    escape_markdown, find_or_create_label, job_kind, markdown_links, member_emails, mentions_any,
    parse_date_millis, parse_instant_millis, parse_stored_id, project_key_base, project_statuses,
    retryable, reuse_earlier_projects, reuse_earlier_tasks, rewrite_links, stopped,
    with_source_details,
};

/// Job kind names.
pub const SCAN_JOB: &str = "linear.scan";
pub const IMPORT_JOB: &str = "linear.import";
/// Most issues (sub-issues included) one import creates.
pub const MAX_TASKS: usize = 10_000;
/// Sub-issue levels kept below a top-level issue; a deeper issue starts a new tree.
pub const MAX_SUB_ISSUE_DEPTH: usize = 5;
/// A scanned import waits this long for "start" before its key is deleted.
pub const READY_TTL: Duration = Duration::from_secs(24 * 60 * 60);
/// An active import that made no progress for this long is failed by the cleanup.
const STALLED_AFTER: Duration = Duration::from_secs(24 * 60 * 60);
const MAX_TOKEN_CHARS: usize = 1_024;
const MAX_FAILURES_LISTED: i64 = 200;
const MAX_LISTED_IMPORTS: i64 = 20;
const ACTIVE: &str = "('scanning', 'queued', 'importing')";

const TABLES: ImportTables = ImportTables {
    imports: "linear_imports",
    items: "linear_import_items",
    source_id: "linear_id",
    project_kind: "team",
    task_kind: "issue",
};
const OPEN: &str = "('scanning', 'ready', 'queued', 'importing')";
/// How often a running import writes the issues read and a rate-limit wait.
const PROGRESS_INTERVAL: Duration = Duration::from_secs(2);

/// Both job kinds: [`job_kind`], as for the Asana and Notion imports.
#[must_use]
pub fn scan_job_kind() -> JobKind {
    job_kind(SCAN_JOB)
}

#[must_use]
pub fn import_job_kind() -> JobKind {
    job_kind(IMPORT_JOB)
}

/// Registers the `linear.scan` and `linear.import` handlers.
pub fn register_jobs(
    worker: Worker,
    service: LinearImportService,
    notices: ImportNotices,
) -> Result<Worker, JobKindRegistrationError> {
    let scan = service.clone();
    worker
        .with_handler(scan_job_kind(), move |context| {
            let service = scan.clone();
            async move {
                let Some(import_id) = payload_import_id(&context.job.payload) else {
                    return Err(JobError::Permanent(
                        "invalid linear.scan payload".to_owned(),
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
                        "invalid linear.import payload".to_owned(),
                    ));
                };
                let run = JobRun {
                    cancel: context.cancellation_token(),
                    last_attempt: context.claim.attempt >= context.job.max_attempts,
                };
                let result = service.run_import(import_id, &run).await;
                let result = service.after_attempt(import_id, &run, result).await;
                notices.after_run(ImportSource::Linear, import_id).await;
                result
            }
        })
}

fn payload_import_id(payload: &Value) -> Option<Id> {
    payload.get("import_id")?.as_str()?.parse().ok()
}

#[derive(Clone, Debug)]
pub struct LinearImportSettings {
    /// Linear client settings for scan and import jobs (endpoint, request rate).
    pub client: LinearClientConfig,
    /// The server app key; without it no import can store its key.
    pub app_key: Option<[u8; 32]>,
    /// Largest file the import downloads (the upload limit).
    pub max_file_bytes: u64,
}

#[derive(Clone)]
pub struct LinearImportService {
    inner: std::sync::Arc<Inner>,
}

struct Inner {
    database: Database,
    tasks: TaskRepository,
    attachments: AttachmentRepository,
    jobs: JobQueue,
    settings: LinearImportSettings,
}

/// Errors of the import API.
#[derive(Debug, Error)]
pub enum LinearImportError {
    #[error("import not found")]
    NotFound,
    #[error("the app key is not configured")]
    AppKeyMissing,
    #[error("the Linear API key is invalid")]
    TokenInvalid,
    #[error("Linear is unavailable")]
    LinearUnavailable,
    #[error("another import is in progress")]
    InProgress,
    #[error("the import is not in a state that allows this")]
    StateConflict,
    #[error("invalid input: {field}")]
    Invalid { field: &'static str },
    #[error("import storage is unavailable")]
    Unavailable(String),
}

impl From<sqlx::Error> for LinearImportError {
    fn from(error: sqlx::Error) -> Self {
        Self::Unavailable(error.to_string())
    }
}

/// `linear_imports.status`.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum LinearImportStatus {
    /// The scan job is listing teams.
    Scanning,
    /// The tree is ready; waiting for `start` (the key is deleted after 24 hours).
    Ready,
    /// Started; waiting for a job slot.
    Queued,
    Importing,
    Completed,
    Failed,
    Cancelled,
    /// Not started within 24 hours; the key was deleted.
    Expired,
}

impl LinearImportStatus {
    fn parse(value: &str) -> Result<Self, LinearImportError> {
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
                return Err(LinearImportError::Unavailable(
                    "unknown import status".to_owned(),
                ));
            }
        })
    }
}

#[derive(Clone, Copy, Debug, Serialize, ToSchema)]
pub struct LinearImportProgress {
    /// Issues found so far (sub-issues included); grows while teams are listed.
    pub total: u64,
    /// Issues imported with their files and comments.
    pub done: u64,
    /// Issues, teams and relations that could not be imported.
    pub failed: u64,
    /// Issues the selected teams had when they were scanned: the goal of the `reading` stage.
    pub expected: u64,
    /// Issues read from Linear so far, the team being read included.
    pub read: u64,
    /// Tasks created so far.
    pub created: u64,
    /// What the job does now; `null` before the start and once the import ended.
    #[schema(required = true)]
    pub stage: Option<LinearImportStage>,
}

/// The steps of a running import, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum LinearImportStage {
    /// Reading the issue lists of the selected teams from Linear.
    Reading,
    /// Creating the tasks.
    Creating,
    /// Adding files and comments.
    Details,
    /// Linking blocking, related and duplicate issues.
    Linking,
}

impl LinearImportStage {
    fn as_str(self) -> &'static str {
        match self {
            Self::Reading => "reading",
            Self::Creating => "creating",
            Self::Details => "details",
            Self::Linking => "linking",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        [Self::Reading, Self::Creating, Self::Details, Self::Linking]
            .into_iter()
            .find(|stage| stage.as_str() == value)
    }
}

/// A team, issue or relation the import could not create or fill.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct LinearImportFailure {
    pub linear_id: String,
    /// `team`, `issue` or `relation`.
    pub kind: String,
    pub title: String,
    pub error: String,
}

/// What the import carried over besides the issues themselves.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct LinearImportReport {
    pub comments_imported: u32,
    /// Files uploaded to Linear, downloaded and attached to the tasks.
    pub files_imported: u32,
    /// Uploaded files that could not be downloaded (too large, gone, refused).
    pub missing_files: u32,
    /// Linked attachments (pull requests, Slack threads, URLs), added to the descriptions.
    pub links_added: u32,
    /// Blocks, related and duplicate relations between imported issues.
    pub relations_linked: u32,
    /// Relations Orbit cannot hold: a second relation between the same two tasks, a second
    /// duplicate of one task, or a duplicate of a duplicate.
    pub relations_skipped: u32,
    /// Issues whose Linear assignee has no Orbit member with the same email (left unassigned).
    pub unmatched_assignees: u32,
    /// Sub-issues imported at the top level: more than five levels deep, or with a parent in
    /// another team.
    pub detached_sub_issues: u32,
    /// The first 200 failures.
    pub failures: Vec<LinearImportFailure>,
}

/// The report's counts as `linear_imports.report_json` keeps them.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
struct StoredReport {
    comments_imported: u32,
    files_imported: u32,
    missing_files: u32,
    links_added: u32,
    relations_linked: u32,
    relations_skipped: u32,
    unmatched_assignees: u32,
    detached_sub_issues: u32,
}

/// A Linear import. Never contains the key.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct LinearImport {
    #[schema(value_type = String)]
    pub id: Id,
    #[schema(value_type = String)]
    pub workspace_id: Id,
    pub status: LinearImportStatus,
    /// The Linear user the key belongs to.
    #[schema(required = true)]
    pub linear_user_name: Option<String>,
    pub progress: LinearImportProgress,
    /// Set while a Linear rate limit pauses the import: when it goes on.
    #[schema(value_type = Option<String>, format = DateTime, required = true)]
    pub waiting_until: Option<TimestampMillis>,
    /// Why the import failed or expired.
    #[schema(required = true)]
    pub error: Option<String>,
    /// The scan result, once ready. Only in `GET …/imports/linear/{id}?include=tree`; `null`
    /// everywhere else, so polling stays light.
    #[schema(required = true)]
    pub tree: Option<LinearImportTree>,
    /// Set once the import was started.
    #[schema(required = true)]
    pub report: Option<LinearImportReport>,
    /// The Orbit projects the import created that still exist, in selection order.
    #[schema(value_type = Vec<String>)]
    pub project_ids: Vec<Id>,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: TimestampMillis,
    #[schema(value_type = String, format = DateTime)]
    pub updated_at: TimestampMillis,
}

#[derive(Deserialize, Serialize)]
struct StoredSelection {
    team_ids: Vec<String>,
}

/// A team item's data: its key at start, then the status of each workflow state.
#[derive(Default, Deserialize, Serialize)]
struct StoredTeam {
    #[serde(default)]
    key: String,
    /// `(state id, status id)`.
    #[serde(default)]
    states: Vec<(String, String)>,
}

/// A relation item's data: `issue_id` is the source (the blocker, or the duplicate).
#[derive(Deserialize, Serialize)]
struct StoredRelation {
    kind: String,
    issue_id: String,
    related_id: String,
}

#[derive(Clone, Debug)]
struct Item {
    linear_id: String,
    team_id: String,
    parent_id: Option<String>,
    title: String,
    data_json: Option<String>,
    orbit_id: Option<Id>,
    state: String,
}

impl From<LinearError> for Failure {
    fn from(error: LinearError) -> Self {
        match &error {
            LinearError::Unauthorized { .. } => {
                Self::Fatal("The Linear API key was revoked or is no longer valid.".to_owned())
            }
            LinearError::Forbidden { .. } => {
                Self::Fatal("The Linear API key lost access to a selected team.".to_owned())
            }
            LinearError::RateLimited { .. } | LinearError::Http(_) => {
                Self::Transient(error.to_string())
            }
            LinearError::Api { status, .. } if *status >= 500 => Self::Transient(error.to_string()),
            _ => Self::Item(error.to_string()),
        }
    }
}

/// The Orbit side of one imported team, built from its item.
struct TeamContext {
    project_id: Id,
    /// State id → status id.
    states: HashMap<String, Id>,
    /// Status of an issue whose state has no status.
    open_status: Id,
    /// Lowercase name → label id.
    labels: HashMap<String, Id>,
}

/// Shared state of one import run.
struct Run<'a> {
    import_id: Id,
    workspace_id: Id,
    user_id: Id,
    client: &'a LinearClient,
    job: &'a JobRun,
    request_id: String,
    report: StoredReport,
    /// Lowercase email → member id.
    members: HashMap<String, Id>,
    /// Name key → member of the Linear people an admin mapped (Settings → Import).
    mapped: HashMap<String, Id>,
    teams: HashMap<String, TeamContext>,
    reading: Arc<Reading>,
}

/// The team being listed: `total` when its listing began, and its issues read so far.
#[derive(Default)]
struct Reading {
    base: AtomicI64,
    read: AtomicU64,
}

/// Stops the progress writer of an import run when dropped.
struct ProgressWriter(tokio::task::JoinHandle<()>);

impl Drop for ProgressWriter {
    fn drop(&mut self) {
        self.0.abort();
    }
}

impl LinearImportService {
    #[must_use]
    pub fn new(
        database: Database,
        attachments: AttachmentRepository,
        settings: LinearImportSettings,
    ) -> Self {
        Self {
            inner: std::sync::Arc::new(Inner {
                tasks: TaskRepository::new(database.clone()),
                jobs: JobQueue::new(JobStore::new(database.clone())),
                database,
                attachments,
                settings,
            }),
        }
    }

    fn database(&self) -> &Database {
        &self.inner.database
    }

    /// Validates `key` with Linear (`viewer`), stores it encrypted and queues the scan. A member
    /// has at most one active Linear import; their unstarted scans in this workspace are
    /// cancelled (their keys deleted).
    pub async fn create(
        &self,
        workspace_id: Id,
        user_id: Id,
        key: &str,
    ) -> Result<LinearImport, LinearImportError> {
        self.require_member(workspace_id, user_id).await?;
        let app_key = self
            .inner
            .settings
            .app_key
            .ok_or(LinearImportError::AppKeyMissing)?;
        let key = key.trim();
        if key.is_empty()
            || key.chars().count() > MAX_TOKEN_CHARS
            || !key.chars().all(|c| c.is_ascii_graphic())
        {
            return Err(LinearImportError::TokenInvalid);
        }
        let now = TimestampMillis::now();
        self.expire_stale(now).await?;
        let validation = LinearClientConfig {
            max_attempts: 2,
            request_timeout: Duration::from_secs(20),
            max_retry_after: Duration::from_secs(5),
            ..self.inner.settings.client.clone()
        };
        let client =
            LinearClient::new(key, validation).map_err(|_| LinearImportError::TokenInvalid)?;
        let viewer = match client.viewer().await {
            Ok(viewer) => viewer,
            Err(LinearError::Unauthorized { .. } | LinearError::Config(_)) => {
                return Err(LinearImportError::TokenInvalid);
            }
            Err(error) => {
                tracing::warn!(error = %error, "Linear key validation failed");
                return Err(LinearImportError::LinearUnavailable);
            }
        };
        let name = viewer
            .name
            .or(viewer.display_name)
            .filter(|name| !name.trim().is_empty())
            .map(|name| clamp(name.trim(), 200, 800));
        let ciphertext = encrypt_secret(&app_key, key)
            .map_err(|()| LinearImportError::Unavailable("key encryption failed".to_owned()))?;

        let id = Id::new_v7();
        let mut tx = self.database().immediate_transaction().await?;
        let active: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) FROM linear_imports WHERE user_id = $1 AND status IN {ACTIVE}"
        )))
        .bind(user_id.to_string())
        .fetch_one(&mut *tx)
        .await?;
        if active > 0 {
            return Err(LinearImportError::InProgress);
        }
        sqlx::query(
            "UPDATE linear_imports SET status = 'cancelled', token_ciphertext = NULL, updated_at = $1 \
             WHERE user_id = $2 AND workspace_id = $3 AND status = 'ready'",
        )
        .bind(now.as_millis())
        .bind(user_id.to_string())
        .bind(workspace_id.to_string())
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO linear_imports (id, workspace_id, user_id, status, token_ciphertext, \
             linear_user_name, created_at, updated_at) VALUES ($1, $2, $3, 'scanning', $4, $5, $6, $7)",
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
    ) -> Result<Vec<LinearImport>, LinearImportError> {
        self.require_member(workspace_id, user_id).await?;
        self.expire_stale(TimestampMillis::now()).await?;
        let ids: Vec<String> = sqlx::query_scalar(
            "SELECT id FROM linear_imports WHERE workspace_id = $1 AND user_id = $2 \
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
                .map_err(|_| LinearImportError::Unavailable("invalid import id".to_owned()))?;
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
    ) -> Result<LinearImport, LinearImportError> {
        self.require_member(workspace_id, user_id).await?;
        self.expire_stale(TimestampMillis::now()).await?;
        self.load_view(workspace_id, user_id, import_id, with_tree)
            .await
    }

    /// Plans the import of the selected teams and queues the import job. Every id must be a
    /// team of the scan tree.
    pub async fn start(
        &self,
        workspace_id: Id,
        user_id: Id,
        import_id: Id,
        team_ids: Vec<String>,
    ) -> Result<LinearImport, LinearImportError> {
        self.require_member(workspace_id, user_id).await?;
        self.expire_stale(TimestampMillis::now()).await?;
        let row = sqlx::query(
            "SELECT status, tree_json FROM linear_imports WHERE id = $1 AND workspace_id = $2 AND user_id = $3",
        )
        .bind(import_id.to_string())
        .bind(workspace_id.to_string())
        .bind(user_id.to_string())
        .fetch_optional(self.database().pool())
        .await?
        .ok_or(LinearImportError::NotFound)?;
        if row.get::<String, _>("status") != "ready" {
            return Err(LinearImportError::StateConflict);
        }
        let tree: LinearImportTree = serde_json::from_str(
            &row.get::<Option<String>, _>("tree_json")
                .unwrap_or_default(),
        )
        .map_err(|_| LinearImportError::Unavailable("invalid stored tree".to_owned()))?;
        let mut seen = HashSet::new();
        let mut teams = Vec::new();
        for id in &team_ids {
            let team = tree
                .team(id)
                .ok_or(LinearImportError::Invalid { field: "team_ids" })?;
            if seen.insert(id.clone()) {
                teams.push(team.clone());
            }
        }
        if teams.is_empty() || teams.len() > MAX_TEAMS {
            return Err(LinearImportError::Invalid { field: "team_ids" });
        }
        let selection = StoredSelection {
            team_ids: teams.iter().map(|team| team.id.clone()).collect(),
        };
        let expected: u64 = teams.iter().map(|team| team.issue_count).sum();

        let now = TimestampMillis::now();
        let mut tx = self.database().immediate_transaction().await?;
        let active: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) FROM linear_imports WHERE user_id = $1 AND id <> $2 AND status IN {ACTIVE}"
        )))
        .bind(user_id.to_string())
        .bind(import_id.to_string())
        .fetch_one(&mut *tx)
        .await?;
        if active > 0 {
            return Err(LinearImportError::InProgress);
        }
        let updated = sqlx::query(
            "UPDATE linear_imports SET status = 'queued', selection_json = $1, total = 0, done = 0, \
             failed = 0, report_json = $2, updated_at = $3, started_at = $3, expected = $5 \
             WHERE id = $4 AND status = 'ready' AND token_ciphertext IS NOT NULL",
        )
        .bind(serde_json::to_string(&selection).unwrap_or_default())
        .bind(serde_json::to_string(&StoredReport::default()).unwrap_or_default())
        .bind(now.as_millis())
        .bind(import_id.to_string())
        .bind(i64::try_from(expected).unwrap_or(i64::MAX))
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if updated != 1 {
            return Err(LinearImportError::StateConflict);
        }
        for (position, team) in teams.iter().enumerate() {
            let data = StoredTeam {
                key: team.key.clone(),
                states: Vec::new(),
            };
            sqlx::query(
                "INSERT INTO linear_import_items (import_id, kind, linear_id, team_id, position, title, data_json) \
                 VALUES ($1, 'team', $2, $3, $4, $5, $6)",
            )
            .bind(import_id.to_string())
            .bind(&team.id)
            .bind(&team.id)
            .bind(position as i64)
            .bind(&team.name)
            .bind(serde_json::to_string(&data).unwrap_or_default())
            .execute(&mut *tx)
            .await?;
        }
        reuse_earlier_projects(&mut tx, &TABLES, import_id, workspace_id).await?;
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

    /// Cancels an open import and deletes its key. A running job stops before its next item;
    /// what it already imported stays.
    pub async fn cancel(
        &self,
        workspace_id: Id,
        user_id: Id,
        import_id: Id,
    ) -> Result<LinearImport, LinearImportError> {
        self.require_member(workspace_id, user_id).await?;
        let updated = sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE linear_imports SET status = 'cancelled', token_ciphertext = NULL, updated_at = $1 \
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
            return Err(LinearImportError::StateConflict);
        }
        self.load_view(workspace_id, user_id, import_id, false)
            .await
    }

    /// Expires scanned imports that were not started within 24 hours and fails active imports
    /// that made no progress for 24 hours; both lose their key. Returns the rows changed.
    pub async fn expire_stale(&self, now: TimestampMillis) -> Result<u64, LinearImportError> {
        let ready_cutoff = now.as_millis() - READY_TTL.as_millis() as i64;
        let stalled_cutoff = now.as_millis() - STALLED_AFTER.as_millis() as i64;
        let expired = sqlx::query(
            "UPDATE linear_imports SET status = 'expired', token_ciphertext = NULL, \
             error = 'The import was not started within 24 hours.', updated_at = $1 \
             WHERE status = 'ready' AND updated_at < $2",
        )
        .bind(now.as_millis())
        .bind(ready_cutoff)
        .execute(self.database().pool())
        .await?
        .rows_affected();
        let stalled = sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE linear_imports SET status = 'failed', token_ciphertext = NULL, \
             error = 'The import stopped making progress.', updated_at = $1 \
             WHERE status IN {ACTIVE} AND updated_at < $2"
        )))
        .bind(now.as_millis())
        .bind(stalled_cutoff)
        .execute(self.database().pool())
        .await?
        .rows_affected();
        let dead = self.fail_imports_with_dead_jobs().await?;
        Ok(expired + stalled + dead)
    }

    /// When a job attempt that was not interrupted by shutdown fails for the last time, the job
    /// is about to go dead: fail the import now instead of leaving it active until the stall
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
                .finish(
                    import_id,
                    "failed",
                    Some("The import stopped after repeated errors."),
                )
                .await
        {
            tracing::warn!(import_id = %import_id, error = %error, cleanup = %cleanup, "Linear import could not be finalized");
        }
        result
    }

    /// Fails active imports whose job is dead and that have no live job left.
    async fn fail_imports_with_dead_jobs(&self) -> Result<u64, LinearImportError> {
        let import_id_of_job = match self.database().backend() {
            Backend::Sqlite => "json_extract(jobs.payload_json, '$.import_id')",
            Backend::Postgres => "(jobs.payload_json::jsonb ->> 'import_id')",
        };
        let failed = sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE linear_imports SET status = 'failed', token_ciphertext = NULL, \
             error = 'The import stopped after repeated errors.', updated_at = $1 \
             WHERE status IN {ACTIVE} \
             AND EXISTS (SELECT 1 FROM jobs WHERE jobs.kind IN ('{SCAN_JOB}', '{IMPORT_JOB}') \
                 AND jobs.state = 'dead' AND {import_id_of_job} = linear_imports.id) \
             AND NOT EXISTS (SELECT 1 FROM jobs WHERE jobs.kind IN ('{SCAN_JOB}', '{IMPORT_JOB}') \
                 AND jobs.state IN ('queued', 'running') AND {import_id_of_job} = linear_imports.id)"
        )))
        .bind(TimestampMillis::now().as_millis())
        .execute(self.database().pool())
        .await?
        .rows_affected();
        Ok(failed)
    }

    /// The `linear.scan` job: lists the teams and stores the tree.
    pub async fn run_scan(&self, import_id: Id, run: &JobRun) -> Result<(), JobError> {
        let Some(row) =
            sqlx::query("SELECT status, token_ciphertext FROM linear_imports WHERE id = $1")
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
        match build_tree(&client, self.database(), import_id, &run.cancel).await {
            Ok(None) if run.cancel.is_cancelled() => {
                Err(JobError::Retryable("interrupted by shutdown".to_owned()))
            }
            Ok(None) => Ok(()),
            Ok(Some(tree)) => {
                sqlx::query(
                    "UPDATE linear_imports SET status = 'ready', tree_json = $1, updated_at = $2 \
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
            Err(error) => self.job_failure(import_id, Failure::from(error), run).await,
        }
    }

    /// The `linear.import` job.
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
                tracing::warn!(import_id = %import_id, error = %message, "Linear import will retry");
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

    async fn import(&self, import_id: Id, job: &JobRun) -> Result<(), Failure> {
        let Some(row) = sqlx::query(
            "SELECT workspace_id, user_id, status, token_ciphertext, report_json \
             FROM linear_imports WHERE id = $1",
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
        ensure_writable(self.database(), workspace_id).await?;
        let report: StoredReport = row
            .get::<Option<String>, _>("report_json")
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default();
        let client = self
            .client_for(row.get("token_ciphertext"))
            .map_err(Failure::Fatal)?;
        sqlx::query(
            "UPDATE linear_imports SET status = 'importing', updated_at = $1 WHERE id = $2 AND status = 'queued'",
        )
        .bind(TimestampMillis::now().as_millis())
        .bind(import_id.to_string())
        .execute(self.database().pool())
        .await?;
        let mut run = Run {
            import_id,
            workspace_id,
            user_id,
            client: &client,
            job,
            request_id: format!("linear-import-{import_id}"),
            report,
            members: member_emails(self.database(), workspace_id).await?,
            mapped: import_people::mapped_members(self.database(), workspace_id, Source::Linear)
                .await?,
            teams: HashMap::new(),
            reading: Arc::default(),
        };
        let _writer = self.write_progress(import_id, client.clone(), run.reading.clone());
        let outcome: Result<(), Failure> = async {
        // Phase A: projects, their statuses, and the issue and relation lists.
        self.set_stage(import_id, LinearImportStage::Reading).await?;
        let teams: Vec<Item> = self.load_items(import_id, "team").await?;
        for (index, team) in teams.iter().enumerate() {
            if team.state == "filled" || team.state == "failed" {
                continue;
            }
            if self.should_stop(&run).await? {
                return stopped(job);
            }
            match self.prepare_team(&mut run, team, index).await {
                Ok(()) => {}
                Err(Failure::Item(message)) => {
                    self.fail_item(import_id, "team", &team.linear_id, &message)
                        .await?;
                }
                Err(other) => return Err(other),
            }
        }
        for team in self.load_items(import_id, "team").await? {
            if team.state == "filled" {
                let context = self.team_context(&run, &team).await?;
                run.teams.insert(team.linear_id.clone(), context);
            }
        }

        // Phase B: create every issue, parents first.
        self.set_stage(import_id, LinearImportStage::Creating).await?;
        let mut issues = self.load_items(import_id, "issue").await?;
        for index in 0..issues.len() {
            if issues[index].state != "pending" {
                continue;
            }
            if self.should_stop(&run).await? {
                self.save_report(&run).await?;
                return stopped(job);
            }
            let parent = issues[index].parent_id.as_ref().and_then(|parent| {
                issues
                    .iter()
                    .find(|item| {
                        &item.linear_id == parent
                            && matches!(item.state.as_str(), "created" | "filled")
                    })
                    .and_then(|item| item.orbit_id)
            });
            match self.create_task(&mut run, &mut issues[index], parent).await {
                Ok(()) => issues[index].state = "created".to_owned(),
                Err(Failure::Item(message)) => {
                    self.fail_item(import_id, "issue", &issues[index].linear_id, &message)
                        .await?;
                    issues[index].state = "failed".to_owned();
                }
                Err(other) => {
                    self.save_report(&run).await?;
                    return Err(other);
                }
            }
        }
        self.save_report(&run).await?;

        // Phase C: files and comments.
        self.set_stage(import_id, LinearImportStage::Details).await?;
        for item in &issues {
            if item.state != "created" {
                continue;
            }
            if self.should_stop(&run).await? {
                self.save_report(&run).await?;
                return stopped(job);
            }
            match self.fill_task(&mut run, item).await {
                Ok(()) => {
                    sqlx::query(
                        "UPDATE linear_import_items SET state = 'filled', data_json = NULL, error = NULL \
                         WHERE import_id = $1 AND kind = 'issue' AND linear_id = $2",
                    )
                    .bind(import_id.to_string())
                    .bind(&item.linear_id)
                    .execute(self.database().pool())
                    .await?;
                    sqlx::query(
                        "UPDATE linear_imports SET done = done + 1, updated_at = $1 WHERE id = $2",
                    )
                    .bind(TimestampMillis::now().as_millis())
                    .bind(import_id.to_string())
                    .execute(self.database().pool())
                    .await?;
                    self.save_report(&run).await?;
                }
                Err(Failure::Item(message)) => {
                    self.fail_item(import_id, "issue", &item.linear_id, &message)
                        .await?;
                    self.save_report(&run).await?;
                }
                Err(other) => {
                    self.save_report(&run).await?;
                    return Err(other);
                }
            }
        }

        // Phase D: relations between the imported issues.
        self.set_stage(import_id, LinearImportStage::Linking).await?;
        let created = self.created_tasks(import_id).await?;
        for item in self.load_items(import_id, "relation").await? {
            if item.state != "pending" {
                continue;
            }
            if self.should_stop(&run).await? {
                self.save_report(&run).await?;
                return stopped(job);
            }
            match self.link(&mut run, &item, &created).await {
                Ok(()) => {
                    sqlx::query(
                        "UPDATE linear_import_items SET state = 'filled' \
                         WHERE import_id = $1 AND kind = 'relation' AND linear_id = $2",
                    )
                    .bind(import_id.to_string())
                    .bind(&item.linear_id)
                    .execute(self.database().pool())
                    .await?;
                    self.save_report(&run).await?;
                }
                Err(Failure::Item(message)) => {
                    self.fail_item(import_id, "relation", &item.linear_id, &message)
                        .await?;
                }
                Err(other) => {
                    self.save_report(&run).await?;
                    return Err(other);
                }
            }
        }

        sqlx::query(
            "UPDATE linear_imports SET status = 'completed', token_ciphertext = NULL, stage = NULL, \
             reading = 0, waiting_until = NULL, updated_at = $1 WHERE id = $2 AND status = 'importing'",
        )
        .bind(TimestampMillis::now().as_millis())
        .bind(import_id.to_string())
        .execute(self.database().pool())
        .await?;
        Ok(())
        }
        .await;
        // Linear wants a longer pause than the client waits in place: run again after it, without
        // spending one of the job's few retries.
        if matches!(outcome, Err(Failure::Transient(_)))
            && !job.cancel.is_cancelled()
            && let Some(wait) = client.rate_limited_for()
        {
            return self.resume_after(import_id, workspace_id, wait).await;
        }
        outcome
    }

    /// Queues the next run of the import job after `wait` and shows the wait.
    async fn resume_after(
        &self,
        import_id: Id,
        workspace_id: Id,
        wait: Duration,
    ) -> Result<(), Failure> {
        let at = TimestampMillis::from_millis(
            TimestampMillis::now().as_millis()
                + i64::try_from(wait.as_millis()).unwrap_or(i64::MAX / 2),
        );
        sqlx::query(
            "UPDATE linear_imports SET waiting_until = $1 WHERE id = $2 AND status = 'importing'",
        )
        .bind(at.as_millis())
        .bind(import_id.to_string())
        .execute(self.database().pool())
        .await?;
        let job = Job::new(
            import_job_kind(),
            json!({"import_id": import_id.to_string()}),
            at,
        )
        .with_workspace(workspace_id.to_string());
        self.inner
            .jobs
            .enqueue(job)
            .await
            .map_err(|error| Failure::Transient(error.to_string()))?;
        tracing::info!(import_id = %import_id, wait_s = wait.as_secs(), "Linear import waits for the rate limit");
        Ok(())
    }

    async fn set_stage(&self, import_id: Id, stage: LinearImportStage) -> Result<(), Failure> {
        sqlx::query("UPDATE linear_imports SET stage = $1 WHERE id = $2")
            .bind(stage.as_str())
            .bind(import_id.to_string())
            .execute(self.database().pool())
            .await?;
        Ok(())
    }

    /// Every [`PROGRESS_INTERVAL`] while the run lasts, stores the issues read of the team being
    /// listed and when a Linear rate limit lets the import go on. Best effort: a failed write
    /// waits for the next tick.
    fn write_progress(
        &self,
        import_id: Id,
        client: LinearClient,
        reading: Arc<Reading>,
    ) -> ProgressWriter {
        let database = self.database().clone();
        ProgressWriter(tokio::spawn(async move {
            let mut written: Option<(u64, Option<i64>)> = None;
            let mut tick = tokio::time::interval(PROGRESS_INTERVAL);
            loop {
                tick.tick().await;
                let waiting_until = client.rate_limit_wait().map(|wait| {
                    // whole seconds, so a steady wait is not rewritten on every tick
                    let at = TimestampMillis::now().as_millis()
                        + i64::try_from(wait.as_millis()).unwrap_or(i64::MAX / 2);
                    at - at % 1_000
                });
                let read = reading.read.load(Ordering::Relaxed);
                if written == Some((read, waiting_until)) {
                    continue;
                }
                // `total = base`: once the team is stored its count is in `total`, and a late tick
                // must not add it again
                let result = sqlx::query(
                    "UPDATE linear_imports SET reading = CASE WHEN total = $1 THEN $2 ELSE reading END, \
                     waiting_until = $3 WHERE id = $4 AND status = 'importing'",
                )
                .bind(reading.base.load(Ordering::Relaxed))
                .bind(i64::try_from(read).unwrap_or(i64::MAX))
                .bind(waiting_until)
                .bind(import_id.to_string())
                .execute(database.pool())
                .await;
                match result {
                    Ok(_) => written = Some((read, waiting_until)),
                    Err(error) => {
                        tracing::debug!(import_id = %import_id, error = %error, "Linear import progress not written")
                    }
                }
            }
        }))
    }

    /// Phase A for one team: the Orbit project, a status per workflow state, and the items.
    async fn prepare_team(
        &self,
        run: &mut Run<'_>,
        item: &Item,
        index: usize,
    ) -> Result<(), Failure> {
        let mut stored: StoredTeam = item
            .data_json
            .as_deref()
            .and_then(|json| serde_json::from_str(json).ok())
            .unwrap_or_default();
        let project_id = match item.orbit_id {
            Some(id) => id,
            None => {
                let id = Id::new_v7();
                self.record_orbit_id(run.import_id, "team", &item.linear_id, id)
                    .await?;
                id
            }
        };
        let key_source = if stored.key.trim().is_empty() {
            &item.title
        } else {
            &stored.key
        };
        ensure_project(
            self.database(),
            &self.inner.tasks,
            NewProject {
                id: project_id,
                workspace_id: run.workspace_id,
                user_id: run.user_id,
                name: &item.title,
                fallback_name: "Linear team",
                key_base: project_key_base(key_source, "LIN"),
                color: PROJECT_COLORS[index % PROJECT_COLORS.len()],
                request_id: &run.request_id,
            },
        )
        .await?;
        sqlx::query(
            "UPDATE linear_import_items SET state = 'created' WHERE import_id = $1 AND kind = 'team' \
             AND linear_id = $2 AND state = 'pending'",
        )
        .bind(run.import_id.to_string())
        .bind(&item.linear_id)
        .execute(self.database().pool())
        .await?;

        let mut states = run.client.states(&item.linear_id).await?;
        states.sort_by(|a, b| a.position.total_cmp(&b.position));
        let mut statuses = project_statuses(self.database(), project_id).await?;
        let mut mapped = Vec::with_capacity(states.len());
        for state in &states {
            let name = state
                .name
                .as_deref()
                .map(|name| clamp(name.trim(), 100, 100))
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| "Untitled state".to_owned());
            let category = state_category(state.kind.as_deref());
            let same_name = statuses
                .iter()
                .find(|(_, existing, _)| same_status_name(existing, &name));
            let status_id = match same_name {
                // Orbit's Duplicate status is entered only through a duplicate relation (phase
                // D), so Linear's "Duplicate" state lands on a cancelled status until then.
                Some((_, _, existing)) if existing == "duplicate" => statuses
                    .iter()
                    .find(|(_, _, category)| category == "cancelled")
                    .map(|(id, _, _)| *id),
                Some((id, _, _)) => Some(*id),
                None => None,
            };
            let status_id = match status_id {
                Some(id) => id,
                None => {
                    let color = state
                        .color
                        .clone()
                        .filter(|color| is_hex_color(color))
                        .unwrap_or_else(|| category_color(category).to_owned());
                    let status = self
                        .inner
                        .tasks
                        .create_status(
                            run.workspace_id,
                            project_id,
                            run.user_id,
                            name.clone(),
                            String::new(),
                            color,
                            category.to_owned(),
                            None,
                            &run.request_id,
                            TimestampMillis::now(),
                        )
                        .await?;
                    statuses.push((status.id, name, category.to_owned()));
                    status.id
                }
            };
            mapped.push((state.id.clone(), status_id.to_string()));
        }
        stored.states = mapped;
        sqlx::query(
            "UPDATE linear_import_items SET data_json = $1 WHERE import_id = $2 AND kind = 'team' AND linear_id = $3",
        )
        .bind(serde_json::to_string(&stored).unwrap_or_default())
        .bind(run.import_id.to_string())
        .bind(&item.linear_id)
        .execute(self.database().pool())
        .await?;
        self.list_issues(run, &item.linear_id).await?;
        sqlx::query(
            "UPDATE linear_import_items SET state = 'filled' WHERE import_id = $1 AND kind = 'team' AND linear_id = $2",
        )
        .bind(run.import_id.to_string())
        .bind(&item.linear_id)
        .execute(self.database().pool())
        .await?;
        self.touch(run.import_id).await?;
        Ok(())
    }

    /// Lists the team's issues (trashed ones left out) as pending items, parents before their
    /// sub-issues, and their relations. An item already listed keeps its place.
    async fn list_issues(&self, run: &mut Run<'_>, team_id: &str) -> Result<(), Failure> {
        let listed: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM linear_import_items WHERE import_id = $1 AND kind = 'issue'",
        )
        .bind(run.import_id.to_string())
        .fetch_one(self.database().pool())
        .await?;
        let room = MAX_TASKS.saturating_sub(usize::try_from(listed).unwrap_or(MAX_TASKS));
        let total: i64 = sqlx::query_scalar("SELECT total FROM linear_imports WHERE id = $1")
            .bind(run.import_id.to_string())
            .fetch_one(self.database().pool())
            .await?;
        run.reading.read.store(0, Ordering::Relaxed);
        run.reading.base.store(total, Ordering::Relaxed);
        let issues: Vec<LinearIssue> = run
            .client
            .team_issues(team_id, room + 1, &run.reading.read)
            .await?
            .into_iter()
            .filter(|issue| issue.trashed != Some(true))
            .collect();
        if issues.len() > room {
            return Err(Failure::Fatal(format!(
                "The selected teams have more than {MAX_TASKS} issues. Import fewer teams at a time."
            )));
        }
        if self.should_stop(run).await? {
            return stopped(run.job);
        }
        let planned = plan_issues(&issues);
        let mut tx = self.database().immediate_transaction().await?;
        let mut inserted = 0;
        for (index, parent) in &planned {
            let issue = &issues[*index];
            inserted += sqlx::query(
                "INSERT INTO linear_import_items (import_id, kind, linear_id, team_id, parent_id, \
                 position, title, data_json) VALUES ($1, 'issue', $2, $3, $4, \
                 (SELECT COALESCE(MAX(position) + 1, 0) FROM linear_import_items WHERE import_id = $5), $6, $7) \
                 ON CONFLICT (import_id, kind, linear_id) DO NOTHING",
            )
            .bind(run.import_id.to_string())
            .bind(&issue.id)
            .bind(team_id)
            .bind(parent.as_deref())
            .bind(run.import_id.to_string())
            .bind(clamp(&issue_label(issue), 500, 2_000))
            .bind(serde_json::to_string(issue).unwrap_or_default())
            .execute(&mut *tx)
            .await?
            .rows_affected();
        }
        for issue in &issues {
            for relation in &issue.relations.nodes {
                let (Some(kind), Some(related)) = (&relation.kind, &relation.related_issue) else {
                    continue;
                };
                let data = StoredRelation {
                    kind: kind.clone(),
                    issue_id: issue.id.clone(),
                    related_id: related.id.clone(),
                };
                sqlx::query(
                    "INSERT INTO linear_import_items (import_id, kind, linear_id, team_id, \
                     position, title, data_json) VALUES ($1, 'relation', $2, $3, \
                     (SELECT COALESCE(MAX(position) + 1, 0) FROM linear_import_items WHERE import_id = $4), $5, $6) \
                     ON CONFLICT (import_id, kind, linear_id) DO NOTHING",
                )
                .bind(run.import_id.to_string())
                .bind(&relation.id)
                .bind(team_id)
                .bind(run.import_id.to_string())
                .bind(clamp(&format!("{} {kind}", issue_label(issue)), 500, 2_000))
                .bind(serde_json::to_string(&data).unwrap_or_default())
                .execute(&mut *tx)
                .await?;
            }
        }
        sqlx::query(
            "UPDATE linear_imports SET total = total + $1, reading = 0, updated_at = $2 WHERE id = $3",
        )
            .bind(inserted as i64)
            .bind(TimestampMillis::now().as_millis())
            .bind(run.import_id.to_string())
            .execute(&mut *tx)
            .await?;
        reuse_earlier_tasks(&mut tx, &TABLES, run.import_id, run.workspace_id).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Statuses and labels of a team whose phase A finished.
    async fn team_context(&self, run: &Run<'_>, item: &Item) -> Result<TeamContext, Failure> {
        let project_id = item
            .orbit_id
            .ok_or_else(|| Failure::Fatal("Stored import data is invalid.".to_owned()))?;
        let stored: StoredTeam = item
            .data_json
            .as_deref()
            .and_then(|json| serde_json::from_str(json).ok())
            .unwrap_or_default();
        let statuses = project_statuses(self.database(), project_id).await?;
        let usable: HashSet<Id> = statuses
            .iter()
            .filter(|(_, _, category)| category != "duplicate")
            .map(|(id, _, _)| *id)
            .collect();
        let states: HashMap<String, Id> = stored
            .states
            .iter()
            .filter_map(|(state, id)| Some((state.clone(), id.parse().ok()?)))
            .filter(|(_, id)| usable.contains(id))
            .collect();
        let open_status = statuses
            .iter()
            .find(|(_, name, category)| category == "unstarted" && name == "Todo")
            .or_else(|| {
                statuses
                    .iter()
                    .find(|(_, _, category)| category == "unstarted")
            })
            .map(|(id, _, _)| *id)
            .ok_or_else(|| Failure::Item("The project has no open status.".to_owned()))?;
        let labels =
            sqlx::query("SELECT id, name FROM labels WHERE workspace_id = $1 AND project_id = $2")
                .bind(run.workspace_id.to_string())
                .bind(project_id.to_string())
                .fetch_all(self.database().pool())
                .await?
                .into_iter()
                .filter_map(|row| {
                    Some((
                        row.get::<String, _>("name").to_lowercase(),
                        row.get::<String, _>("id").parse().ok()?,
                    ))
                })
                .collect();
        Ok(TeamContext {
            project_id,
            states,
            open_status,
            labels,
        })
    }

    /// Phase B for one issue: its Orbit task, created silently under the recorded id.
    async fn create_task(
        &self,
        run: &mut Run<'_>,
        item: &mut Item,
        parent: Option<Id>,
    ) -> Result<(), Failure> {
        if !run.teams.contains_key(&item.team_id) {
            return Err(Failure::Item("Its team could not be imported.".to_owned()));
        }
        let issue: LinearIssue = item
            .data_json
            .as_deref()
            .and_then(|json| serde_json::from_str(json).ok())
            .ok_or_else(|| Failure::Item("The stored Linear issue is invalid.".to_owned()))?;
        let task_id = match item.orbit_id {
            Some(id) => id,
            None => {
                let id = Id::new_v7();
                self.record_orbit_id(run.import_id, "issue", &item.linear_id, id)
                    .await?;
                item.orbit_id = Some(id);
                id
            }
        };
        let exists: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tasks WHERE id = $1")
            .bind(task_id.to_string())
            .fetch_one(self.database().pool())
            .await?;
        if exists == 0 {
            let detached = issue.parent.is_some() && item.parent_id.is_none();
            let mut input = self
                .task_input(run, &item.team_id, &issue, parent, detached)
                .await?;
            if detached {
                run.report.detached_sub_issues += 1;
            }
            let completed_at = issue
                .completed_at
                .as_deref()
                .or(issue.canceled_at.as_deref())
                .and_then(parse_instant_millis)
                .map(TimestampMillis::from_millis);
            let mut created = self
                .inner
                .tasks
                .import_task(
                    task_id,
                    run.workspace_id,
                    run.user_id,
                    input.clone(),
                    completed_at,
                    &run.request_id,
                    TimestampMillis::now(),
                )
                .await;
            // The parent was deleted meanwhile: keep the task at the top level.
            if matches!(created, Err(TaskError::ParentInvalid)) && input.parent_task_id.is_some() {
                input.parent_task_id = None;
                created = self
                    .inner
                    .tasks
                    .import_task(
                        task_id,
                        run.workspace_id,
                        run.user_id,
                        input,
                        completed_at,
                        &run.request_id,
                        TimestampMillis::now(),
                    )
                    .await;
            }
            created?;
        }
        if let Some((found, name)) = source_assignee(run, &issue)
            && !matches!(found, Match::Email(_))
        {
            let email = issue.assignee.as_ref().and_then(|a| a.email.as_deref());
            import_people::record(
                self.database(),
                run.workspace_id,
                Source::Linear,
                &name,
                email,
                task_id,
            )
            .await?;
        }
        sqlx::query(
            "UPDATE linear_import_items SET state = 'created' WHERE import_id = $1 AND kind = 'issue' AND linear_id = $2",
        )
        .bind(run.import_id.to_string())
        .bind(&item.linear_id)
        .execute(self.database().pool())
        .await?;
        self.touch(run.import_id).await?;
        Ok(())
    }

    /// The Orbit task for a Linear issue: status from its workflow state, priority, assignee by
    /// email, labels, due date, and a description with what Orbit has no field for.
    async fn task_input(
        &self,
        run: &mut Run<'_>,
        team_id: &str,
        issue: &LinearIssue,
        parent: Option<Id>,
        detached: bool,
    ) -> Result<CreateTask, Failure> {
        let mut labels = Vec::new();
        for label in &issue.labels.nodes {
            let Some(name) = label_name(label) else {
                continue;
            };
            let label = self.ensure_label(run, team_id, &name).await?;
            if !labels.contains(&label) {
                labels.push(label);
            }
        }
        let team = run
            .teams
            .get(team_id)
            .ok_or_else(|| Failure::Item("Its team could not be imported.".to_owned()))?;
        let status_id = issue
            .state
            .as_ref()
            .and_then(|state| team.states.get(&state.id))
            .copied()
            .unwrap_or(team.open_status);
        let found = source_assignee(run, issue);
        let assignee = found.as_ref().and_then(|(found, _)| found.user_id());
        let unmatched = match found {
            Some((Match::Unmatched, name)) => {
                run.report.unmatched_assignees += 1;
                Some(name)
            }
            _ => None,
        };
        let links = links(run.client, issue);
        run.report.links_added += u32::try_from(links.len()).unwrap_or(u32::MAX);
        let title = clamp(issue.title.as_deref().unwrap_or_default().trim(), 500, 500);
        Ok(CreateTask {
            project_id: team.project_id,
            status_id,
            title: if title.is_empty() {
                "Untitled issue".to_owned()
            } else {
                title
            },
            description: description(
                issue,
                issue.description.as_deref().unwrap_or_default(),
                unmatched.as_deref(),
                detached,
                &links,
            ),
            description_json: None,
            source_url: issue
                .url
                .clone()
                .filter(|url| url.starts_with("https://") && url.len() <= 2_048),
            priority: priority(issue.priority).to_owned(),
            position: None,
            assignee_ids: assignee.into_iter().collect(),
            label_ids: labels,
            due_start_at: None,
            due_at: issue
                .due_date
                .as_deref()
                .and_then(parse_date_millis)
                .map(TimestampMillis::from_millis),
            parent_task_id: parent,
        })
    }

    /// The project's label with this name, created on first use.
    async fn ensure_label(
        &self,
        run: &mut Run<'_>,
        team_id: &str,
        name: &str,
    ) -> Result<Id, Failure> {
        let key = name.to_lowercase();
        let team = run
            .teams
            .get(team_id)
            .ok_or_else(|| Failure::Item("Its team could not be imported.".to_owned()))?;
        if let Some(id) = team.labels.get(&key) {
            return Ok(*id);
        }
        let color = LABEL_COLORS[team.labels.len() % LABEL_COLORS.len()];
        let id = find_or_create_label(
            self.database(),
            &self.inner.tasks,
            run.workspace_id,
            team.project_id,
            run.user_id,
            name,
            color,
            &run.request_id,
        )
        .await?;
        if let Some(team) = run.teams.get_mut(team_id) {
            team.labels.insert(key, id);
        }
        Ok(id)
    }

    /// Phase C for one issue: uploaded files, then comments. Each step skips what an earlier
    /// attempt already did.
    async fn fill_task(&self, run: &mut Run<'_>, item: &Item) -> Result<(), Failure> {
        let task_id = item
            .orbit_id
            .ok_or_else(|| Failure::Item("The task was not created.".to_owned()))?;
        let issue: LinearIssue = item
            .data_json
            .as_deref()
            .and_then(|json| serde_json::from_str(json).ok())
            .unwrap_or_default();
        let mut comments: Vec<&LinearComment> = issue.comments.nodes.iter().collect();
        comments.sort_by(|a, b| a.created_at.cmp(&b.created_at));

        // Uploads in the description, the comments and the file attachments, each attached once.
        let body = issue.description.as_deref().unwrap_or_default();
        let mut uploads: Vec<(String, Option<String>)> = Vec::new();
        for markdown in std::iter::once(body).chain(
            comments
                .iter()
                .map(|comment| comment.body.as_deref().unwrap_or_default()),
        ) {
            for (url, label) in markdown_links(markdown) {
                if run.client.is_upload_url(&url) && !uploads.iter().any(|(seen, _)| *seen == url) {
                    uploads.push((url, label));
                }
            }
        }
        for attachment in &issue.attachments.nodes {
            if let Some(url) = attachment
                .url
                .clone()
                .filter(|url| run.client.is_upload_url(url))
                && !uploads.iter().any(|(seen, _)| *seen == url)
            {
                uploads.push((url, attachment.title.clone()));
            }
        }
        let files = self.attach_uploads(run, task_id, &uploads).await?;

        if mentions_any(body, &files) {
            let detached = issue.parent.is_some() && item.parent_id.is_none();
            let unmatched = source_assignee(run, &issue)
                .and_then(|(found, name)| (found == Match::Unmatched).then_some(name));
            let markdown = description(
                &issue,
                &rewrite_links(body, &files),
                unmatched.as_deref(),
                detached,
                &links(run.client, &issue),
            );
            let version: i64 = sqlx::query_scalar("SELECT version FROM tasks WHERE id = $1")
                .bind(task_id.to_string())
                .fetch_one(self.database().pool())
                .await?;
            self.inner
                .tasks
                .update_task(
                    run.workspace_id,
                    run.user_id,
                    &TaskUpdate {
                        id: task_id,
                        expected_version: u64::try_from(version).unwrap_or_default(),
                        changes: TaskChanges {
                            description: Some(markdown),
                            ..TaskChanges::default()
                        },
                    },
                    &run.request_id,
                    TimestampMillis::now(),
                )
                .await?;
        }

        // Comments, oldest first with replies in time order. The import writes them in that
        // order, so the importer's comments already on the task are the first ones.
        let written: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM task_comments WHERE task_id = $1 AND author_id = $2",
        )
        .bind(task_id.to_string())
        .bind(run.user_id.to_string())
        .fetch_one(self.database().pool())
        .await?;
        for comment in comments
            .iter()
            .skip(usize::try_from(written).unwrap_or_default())
        {
            let mut header = format!("**{}**", escape_markdown(&comment.author()));
            if let Some(date) = comment.created_at.as_deref().map(display_instant) {
                header.push_str(&format!(" · {date}"));
            }
            if comment.parent.is_some() {
                header.push_str(" · reply");
            }
            let text = rewrite_links(comment.body.as_deref().unwrap_or_default().trim(), &files);
            let body = if text.is_empty() {
                header
            } else {
                format!("{header}\n\n{text}")
            };
            self.inner
                .tasks
                .create_comment(
                    run.workspace_id,
                    task_id,
                    run.user_id,
                    None,
                    CommentContent::Markdown {
                        body,
                        mentioned_user_ids: Vec::new(),
                    },
                    &run.request_id,
                    TimestampMillis::now(),
                )
                .await?;
            run.report.comments_imported += 1;
        }
        Ok(())
    }

    /// Downloads and attaches the uploads (a file already attached, by content, is reused) and
    /// returns each Linear URL with the Orbit URL that replaces it.
    async fn attach_uploads(
        &self,
        run: &mut Run<'_>,
        task_id: Id,
        uploads: &[(String, Option<String>)],
    ) -> Result<Vec<(String, String)>, Failure> {
        if uploads.is_empty() {
            return Ok(Vec::new());
        }
        let mut attached = self
            .inner
            .attachments
            .task_files(run.workspace_id, task_id)
            .await
            .map_err(|error| Failure::Transient(error.to_string()))?;
        let max_bytes = self.inner.settings.max_file_bytes;
        let mut files = Vec::new();
        for (url, label) in uploads {
            let file = match run.client.download_file(url, max_bytes).await {
                Ok(file) => file,
                Err(
                    error @ (LinearError::Unauthorized { .. }
                    | LinearError::RateLimited { .. }
                    | LinearError::Http(_)),
                ) => return Err(Failure::from(error)),
                Err(error) => {
                    tracing::debug!(error = %error, "Linear file download skipped");
                    run.report.missing_files += 1;
                    continue;
                }
            };
            let name = label
                .as_deref()
                .map(str::trim)
                .filter(|label| label.contains('.'))
                .map(sanitize_file_name)
                .filter(|name| !name.is_empty())
                .unwrap_or(file.file_name.clone());
            let attachment_id = match attach_once(
                &self.inner.attachments,
                &mut attached,
                run.workspace_id,
                task_id,
                run.user_id,
                &name,
                &file.bytes,
            )
            .await
            {
                Attached::Existing(id) => id,
                Attached::Created(id) => {
                    run.report.files_imported += 1;
                    id
                }
                Attached::Refused => {
                    run.report.missing_files += 1;
                    continue;
                }
            };
            files.push((
                url.clone(),
                attachment_url(run.workspace_id, task_id, attachment_id),
            ));
        }
        Ok(files)
    }

    /// Phase D for one relation. A relation to an issue outside the import, or of a kind Orbit
    /// has no equivalent for (`similar`), is dropped; one Orbit's rules reject is counted as
    /// skipped. The checks run before the write, so no write fails on a constraint.
    async fn link(
        &self,
        run: &mut Run<'_>,
        item: &Item,
        created: &HashMap<String, Id>,
    ) -> Result<(), Failure> {
        let Some(relation) = item
            .data_json
            .as_deref()
            .and_then(|json| serde_json::from_str::<StoredRelation>(json).ok())
        else {
            return Err(Failure::Item("The stored relation is invalid.".to_owned()));
        };
        let (Some(source), Some(target)) = (
            created.get(&relation.issue_id).copied(),
            created.get(&relation.related_id).copied(),
        ) else {
            return Ok(());
        };
        let kind = match relation.kind.as_str() {
            "blocks" => "blocks",
            "related" => "related",
            "duplicate" => "duplicate",
            _ => return Ok(()),
        };
        if source == target {
            return Ok(());
        }
        let existing = sqlx::query(
            "SELECT task_id, related_task_id, type FROM task_relations \
             WHERE (task_id = $1 AND related_task_id = $2) OR (task_id = $3 AND related_task_id = $4)",
        )
        .bind(source.to_string())
        .bind(target.to_string())
        .bind(target.to_string())
        .bind(source.to_string())
        .fetch_optional(self.database().pool())
        .await?;
        if let Some(row) = existing {
            let same = row.get::<String, _>("type") == kind
                && (kind == "related" || row.get::<String, _>("task_id") == source.to_string());
            if same {
                run.report.relations_linked += 1;
            } else {
                run.report.relations_skipped += 1;
            }
            return Ok(());
        }
        let result = if kind == "duplicate" {
            let Flag(blocked) = sqlx::query_scalar(
                "SELECT EXISTS (SELECT 1 FROM task_relations WHERE type = 'duplicate' \
                 AND (task_id = $1 OR task_id = $2))",
            )
            .bind(source.to_string())
            .bind(target.to_string())
            .fetch_one(self.database().pool())
            .await?;
            if blocked {
                run.report.relations_skipped += 1;
                return Ok(());
            }
            let version: i64 = sqlx::query_scalar("SELECT version FROM tasks WHERE id = $1")
                .bind(source.to_string())
                .fetch_one(self.database().pool())
                .await?;
            self.inner
                .tasks
                .update_task(
                    run.workspace_id,
                    run.user_id,
                    &TaskUpdate {
                        id: source,
                        expected_version: u64::try_from(version).unwrap_or_default(),
                        changes: TaskChanges {
                            duplicate_of_id: Some(Some(target)),
                            ..TaskChanges::default()
                        },
                    },
                    &run.request_id,
                    TimestampMillis::now(),
                )
                .await
                .map(|_| ())
        } else {
            let kind = if kind == "blocks" {
                NewTaskRelationType::Blocks
            } else {
                NewTaskRelationType::Related
            };
            self.inner
                .tasks
                .add_task_relation(
                    run.workspace_id,
                    source,
                    run.user_id,
                    kind,
                    target,
                    &run.request_id,
                    TimestampMillis::now(),
                )
                .await
                .map(|_| ())
        };
        match result {
            Ok(()) => run.report.relations_linked += 1,
            Err(TaskError::Unavailable(error)) => return Err(Failure::from(error)),
            // A task deleted or changed meanwhile.
            Err(_) => run.report.relations_skipped += 1,
        }
        Ok(())
    }

    /// Linear issue id → Orbit task id of the issues that exist in Orbit.
    async fn created_tasks(&self, import_id: Id) -> Result<HashMap<String, Id>, Failure> {
        Ok(self
            .load_items(import_id, "issue")
            .await?
            .into_iter()
            .filter(|item| matches!(item.state.as_str(), "created" | "filled"))
            .filter_map(|item| Some((item.linear_id, item.orbit_id?)))
            .collect())
    }

    /// Whether to stop before the next item; a workspace that became read-only fails the
    /// import.
    async fn should_stop(&self, run: &Run<'_>) -> Result<bool, Failure> {
        if run.job.cancel.is_cancelled() {
            return Ok(true);
        }
        ensure_writable(self.database(), run.workspace_id).await?;
        let status: Option<String> =
            sqlx::query_scalar("SELECT status FROM linear_imports WHERE id = $1")
                .bind(run.import_id.to_string())
                .fetch_optional(self.database().pool())
                .await?;
        Ok(status.as_deref() != Some("importing"))
    }

    async fn record_orbit_id(
        &self,
        import_id: Id,
        kind: &str,
        linear_id: &str,
        orbit_id: Id,
    ) -> Result<(), Failure> {
        sqlx::query(
            "UPDATE linear_import_items SET orbit_id = $1 WHERE import_id = $2 AND kind = $3 AND linear_id = $4",
        )
        .bind(orbit_id.to_string())
        .bind(import_id.to_string())
        .bind(kind)
        .bind(linear_id)
        .execute(self.database().pool())
        .await?;
        Ok(())
    }

    async fn fail_item(
        &self,
        import_id: Id,
        kind: &str,
        linear_id: &str,
        message: &str,
    ) -> Result<(), Failure> {
        let mut tx = self.database().immediate_transaction().await?;
        let changed = sqlx::query(
            "UPDATE linear_import_items SET state = 'failed', error = $1, data_json = NULL \
             WHERE import_id = $2 AND kind = $3 AND linear_id = $4 AND state <> 'failed'",
        )
        .bind(clamp(message, 500, 2_000))
        .bind(import_id.to_string())
        .bind(kind)
        .bind(linear_id)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        sqlx::query(
            "UPDATE linear_imports SET failed = failed + $1, updated_at = $2 WHERE id = $3",
        )
        .bind(changed as i64)
        .bind(TimestampMillis::now().as_millis())
        .bind(import_id.to_string())
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn touch(&self, import_id: Id) -> Result<(), Failure> {
        sqlx::query("UPDATE linear_imports SET updated_at = $1 WHERE id = $2")
            .bind(TimestampMillis::now().as_millis())
            .bind(import_id.to_string())
            .execute(self.database().pool())
            .await?;
        Ok(())
    }

    async fn save_report(&self, run: &Run<'_>) -> Result<(), Failure> {
        sqlx::query("UPDATE linear_imports SET report_json = $1, updated_at = $2 WHERE id = $3")
            .bind(serde_json::to_string(&run.report).unwrap_or_default())
            .bind(TimestampMillis::now().as_millis())
            .bind(run.import_id.to_string())
            .execute(self.database().pool())
            .await?;
        Ok(())
    }

    /// Moves an open import to a final state and deletes its key.
    async fn finish(
        &self,
        import_id: Id,
        status: &str,
        error: Option<&str>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE linear_imports SET status = $1, token_ciphertext = NULL, error = $2, updated_at = $3, \
             stage = NULL, reading = 0, waiting_until = NULL WHERE id = $4 AND status IN {OPEN}"
        )))
        .bind(status)
        .bind(error.map(|error| clamp(error, 500, 2_000)))
        .bind(TimestampMillis::now().as_millis())
        .bind(import_id.to_string())
        .execute(self.database().pool())
        .await?;
        Ok(())
    }

    fn client_for(&self, ciphertext: Option<Vec<u8>>) -> Result<LinearClient, String> {
        let app_key = self
            .inner
            .settings
            .app_key
            .ok_or_else(|| "The server app key is not configured.".to_owned())?;
        let ciphertext = ciphertext.ok_or_else(|| "The Linear API key was deleted.".to_owned())?;
        let key = decrypt_secret(&app_key, &ciphertext)
            .map_err(|()| "The Linear API key could not be decrypted.".to_owned())?;
        LinearClient::new(&key, self.inner.settings.client.clone())
            .map_err(|_| "The Linear API key is invalid.".to_owned())
    }

    async fn enqueue(
        &self,
        kind: JobKind,
        import_id: Id,
        workspace_id: Id,
    ) -> Result<(), LinearImportError> {
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
            .map_err(|error| LinearImportError::Unavailable(error.to_string()))
    }

    async fn require_member(&self, workspace_id: Id, user_id: Id) -> Result<(), LinearImportError> {
        membership::actor(self.database().pool(), workspace_id, user_id)
            .await?
            .map(|_| ())
            .ok_or(LinearImportError::NotFound)
    }

    async fn load_items(&self, import_id: Id, kind: &str) -> Result<Vec<Item>, Failure> {
        let rows = sqlx::query(
            "SELECT linear_id, team_id, parent_id, title, data_json, orbit_id, state \
             FROM linear_import_items WHERE import_id = $1 AND kind = $2 ORDER BY position",
        )
        .bind(import_id.to_string())
        .bind(kind)
        .fetch_all(self.database().pool())
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok(Item {
                    linear_id: row.get("linear_id"),
                    team_id: row.get("team_id"),
                    parent_id: row.get("parent_id"),
                    title: row.get("title"),
                    data_json: row.get("data_json"),
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

    async fn load_view(
        &self,
        workspace_id: Id,
        user_id: Id,
        import_id: Id,
        with_tree: bool,
    ) -> Result<LinearImport, LinearImportError> {
        let row = sqlx::query(
            "SELECT id, workspace_id, status, linear_user_name, \
             CASE WHEN $1 THEN tree_json END AS tree_json, selection_json, total, done, failed, \
             expected, reading, stage, waiting_until, \
             (SELECT COUNT(*) FROM linear_import_items AS items WHERE items.import_id = linear_imports.id \
                 AND items.kind = 'issue' AND items.state IN ('created', 'filled')) AS created, \
             report_json, error, created_at, updated_at FROM linear_imports \
             WHERE id = $2 AND workspace_id = $3 AND user_id = $4",
        )
        .bind(with_tree)
        .bind(import_id.to_string())
        .bind(workspace_id.to_string())
        .bind(user_id.to_string())
        .fetch_optional(self.database().pool())
        .await?
        .ok_or(LinearImportError::NotFound)?;
        let invalid = |_| LinearImportError::Unavailable("invalid stored import".to_owned());
        let status = LinearImportStatus::parse(&row.get::<String, _>("status"))?;
        let active = matches!(
            status,
            LinearImportStatus::Queued | LinearImportStatus::Importing
        );
        let started = row.get::<Option<String>, _>("selection_json").is_some();
        let tree = row
            .get::<Option<String>, _>("tree_json")
            .map(|json| serde_json::from_str::<LinearImportTree>(&json))
            .transpose()
            .map_err(|_| LinearImportError::Unavailable("invalid stored tree".to_owned()))?;
        let report = if started {
            let stored: StoredReport = row
                .get::<Option<String>, _>("report_json")
                .and_then(|json| serde_json::from_str(&json).ok())
                .unwrap_or_default();
            let failures = sqlx::query(
                "SELECT linear_id, kind, title, error FROM linear_import_items \
                 WHERE import_id = $1 AND state = 'failed' ORDER BY position LIMIT $2",
            )
            .bind(import_id.to_string())
            .bind(MAX_FAILURES_LISTED)
            .fetch_all(self.database().pool())
            .await?
            .into_iter()
            .map(|row| LinearImportFailure {
                linear_id: row.get("linear_id"),
                kind: row.get("kind"),
                title: row.get("title"),
                error: row.get::<Option<String>, _>("error").unwrap_or_default(),
            })
            .collect();
            Some(LinearImportReport {
                comments_imported: stored.comments_imported,
                files_imported: stored.files_imported,
                missing_files: stored.missing_files,
                links_added: stored.links_added,
                relations_linked: stored.relations_linked,
                relations_skipped: stored.relations_skipped,
                unmatched_assignees: stored.unmatched_assignees,
                detached_sub_issues: stored.detached_sub_issues,
                failures,
            })
        } else {
            None
        };
        let project_ids = sqlx::query_scalar::<_, String>(
            "SELECT items.orbit_id FROM linear_import_items AS items \
             JOIN projects ON projects.id = items.orbit_id \
             WHERE items.import_id = $1 AND items.kind = 'team' AND projects.workspace_id = $2 \
             AND projects.deleted_at IS NULL ORDER BY items.position",
        )
        .bind(import_id.to_string())
        .bind(workspace_id.to_string())
        .fetch_all(self.database().pool())
        .await?
        .into_iter()
        .map(|id| id.parse())
        .collect::<Result<Vec<Id>, _>>()
        .map_err(invalid)?;
        let count = |column: &str| u64::try_from(row.get::<i64, _>(column)).unwrap_or_default();
        Ok(LinearImport {
            id: row.get::<String, _>("id").parse().map_err(invalid)?,
            workspace_id: row
                .get::<String, _>("workspace_id")
                .parse()
                .map_err(invalid)?,
            status,
            linear_user_name: row.get("linear_user_name"),
            progress: LinearImportProgress {
                total: count("total"),
                done: count("done"),
                failed: count("failed"),
                expected: count("expected"),
                read: count("total") + count("reading"),
                created: count("created"),
                // a cancel or an expiry ends the import without the job: only an active one has a stage
                stage: row
                    .get::<Option<String>, _>("stage")
                    .as_deref()
                    .and_then(LinearImportStage::parse)
                    .filter(|_| active),
            },
            waiting_until: row
                .get::<Option<i64>, _>("waiting_until")
                .filter(|_| active)
                .map(TimestampMillis::from_millis),
            error: row.get("error"),
            tree,
            report,
            project_ids,
            created_at: TimestampMillis::from_millis(row.get("created_at")),
            updated_at: TimestampMillis::from_millis(row.get("updated_at")),
        })
    }
}

/// Orders a team's issues parents first and picks each one's parent: a parent in the same
/// team, unless that would put the issue more than [`MAX_SUB_ISSUE_DEPTH`] levels deep (it then
/// starts a new tree). Returns `(index into issues, parent id)`.
fn plan_issues(issues: &[LinearIssue]) -> Vec<(usize, Option<String>)> {
    let index: HashMap<&str, usize> = issues
        .iter()
        .enumerate()
        .map(|(position, issue)| (issue.id.as_str(), position))
        .collect();
    let parent_of = |position: usize| {
        issues[position]
            .parent
            .as_ref()
            .and_then(|parent| index.get(parent.id.as_str()).copied())
    };
    // Depth below the top of its tree; `None` while unknown. A parent cycle (not possible in
    // Linear, but the data is external) starts a new tree.
    let mut depth: Vec<Option<usize>> = vec![None; issues.len()];
    for start in 0..issues.len() {
        let mut chain = Vec::new();
        let mut current = Some(start);
        while let Some(position) = current {
            if depth[position].is_some() || chain.contains(&position) {
                break;
            }
            chain.push(position);
            current = parent_of(position);
        }
        let mut base = current.and_then(|position| depth[position]);
        for position in chain.into_iter().rev() {
            let level = match (base, parent_of(position)) {
                (Some(parent), Some(_)) if parent < MAX_SUB_ISSUE_DEPTH => parent + 1,
                _ => 0,
            };
            depth[position] = Some(level);
            base = Some(level);
        }
    }
    let mut planned: Vec<(usize, usize, Option<String>)> = (0..issues.len())
        .map(|position| {
            let level = depth[position].unwrap_or_default();
            let parent = (level > 0)
                .then(|| parent_of(position))
                .flatten()
                .map(|parent| issues[parent].id.clone());
            (level, position, parent)
        })
        .collect();
    planned.sort_by_key(|(level, position, _)| (*level, *position));
    planned
        .into_iter()
        .map(|(_, position, parent)| (position, parent))
        .collect()
}

/// `ENG-123 Title`, for items and failure lists.
fn issue_label(issue: &LinearIssue) -> String {
    let title = issue.title.as_deref().unwrap_or_default().trim();
    match issue.identifier.as_deref() {
        Some(identifier) if !title.is_empty() => format!("{identifier} {title}"),
        Some(identifier) => identifier.to_owned(),
        None => title.to_owned(),
    }
}

/// A label's Orbit name: `Group/Name` for a label inside a group.
fn label_name(label: &super::model::LinearLabel) -> Option<String> {
    let name = label.name.as_deref()?.trim();
    if name.is_empty() {
        return None;
    }
    let full = match label
        .parent
        .as_ref()
        .and_then(|group| group.name.as_deref())
        .map(str::trim)
    {
        Some(group) if !group.is_empty() => format!("{group}/{name}"),
        _ => name.to_owned(),
    };
    Some(clamp(&full, 100, 100))
}

/// Orbit's status category for a Linear workflow state type.
fn state_category(kind: Option<&str>) -> &'static str {
    match kind {
        Some("started") => "started",
        Some("completed") => "completed",
        Some("canceled") => "cancelled",
        _ => "unstarted",
    }
}

fn is_hex_color(value: &str) -> bool {
    value.len() == 7
        && value.starts_with('#')
        && value[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Status names match ignoring case and the "canceled"/"cancelled" spelling.
fn same_status_name(a: &str, b: &str) -> bool {
    let normal = |name: &str| name.trim().to_lowercase().replace("cancelled", "canceled");
    normal(a) == normal(b)
}

/// Orbit priority from Linear's 0 (none), 1 (urgent), 2 (high), 3 (medium), 4 (low).
fn priority(value: f64) -> &'static str {
    match value.round() as i64 {
        1 => "urgent",
        2 => "high",
        3 => "medium",
        4 => "low",
        _ => "none",
    }
}

/// How the import assigns the issue's assignee, with the name an unassigned one gets in the
/// description and in Settings → Import.
fn source_assignee(run: &Run<'_>, issue: &LinearIssue) -> Option<(Match, String)> {
    let assignee = issue.assignee.as_ref()?;
    let name = assignee
        .name
        .clone()
        .or_else(|| assignee.display_name.clone())
        .or_else(|| assignee.email.clone())
        .unwrap_or_else(|| "unknown".to_owned());
    let found =
        import_people::match_assignee(&run.members, &run.mapped, assignee.email.as_deref(), &name);
    Some((found, name))
}

/// The issue's linked attachments that are not uploads, as `(title, url)`.
fn links(client: &LinearClient, issue: &LinearIssue) -> Vec<(String, String)> {
    issue
        .attachments
        .nodes
        .iter()
        .filter_map(|attachment| {
            let url = attachment
                .url
                .clone()
                .filter(|url| url.starts_with("https://") || url.starts_with("http://"))
                .filter(|url| !client.is_upload_url(url))?;
            let title = attachment
                .title
                .clone()
                .filter(|title| !title.trim().is_empty())
                .unwrap_or_else(|| url.clone());
            Some((title, url))
        })
        .collect()
}

/// The description: the issue's markdown, then what Orbit has no field for, then the links.
fn description(
    issue: &LinearIssue,
    body: &str,
    unmatched_assignee: Option<&str>,
    detached: bool,
    links: &[(String, String)],
) -> String {
    let mut fields: Vec<(&str, String)> = Vec::new();
    if let Some(identifier) = &issue.identifier {
        fields.push(("Issue", identifier.clone()));
    }
    if let Some(name) = unmatched_assignee {
        fields.push(("Assignee", name.to_owned()));
    }
    if detached && let Some(parent) = &issue.parent {
        fields.push((
            "Parent",
            parent
                .identifier
                .clone()
                .unwrap_or_else(|| parent.id.clone()),
        ));
    }
    if let Some(project) = issue
        .project
        .as_ref()
        .and_then(|project| project.name.clone())
    {
        fields.push(("Project", project));
    }
    if let Some(milestone) = issue
        .project_milestone
        .as_ref()
        .and_then(|milestone| milestone.name.clone())
    {
        fields.push(("Milestone", milestone));
    }
    if let Some(cycle) = &issue.cycle {
        let number = cycle
            .number
            .map(|number| format!("{}", number.round() as i64));
        let value = match (
            number,
            cycle.name.as_deref().filter(|name| !name.trim().is_empty()),
        ) {
            (Some(number), Some(name)) => format!("{number} ({name})"),
            (Some(number), None) => number,
            (None, Some(name)) => name.to_owned(),
            (None, None) => String::new(),
        };
        if !value.is_empty() {
            fields.push(("Cycle", value));
        }
    }
    if let Some(estimate) = issue.estimate {
        fields.push(("Estimate", format!("{estimate}")));
    }
    if let Some(sla) = issue.sla_breaches_at.as_deref() {
        fields.push(("SLA breaches", display_instant(sla)));
    }
    if issue.archived_at.is_some() {
        fields.push(("Archived", "yes".to_owned()));
    }
    with_source_details(body, "Linear", &fields, links)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::linear::model::{Cycle, IssueRef, LinearLabel, NameRef};

    fn issue(id: &str, parent: Option<&str>) -> LinearIssue {
        LinearIssue {
            id: id.to_owned(),
            parent: parent.map(|parent| IssueRef {
                id: parent.to_owned(),
                identifier: None,
            }),
            ..LinearIssue::default()
        }
    }

    #[test]
    fn plans_parents_first_and_caps_depth() {
        // a ← b ← c ← d ← e ← f ← g (seven levels), x has a parent in another team.
        let issues = vec![
            issue("g", Some("f")),
            issue("x", Some("other-team")),
            issue("a", None),
            issue("b", Some("a")),
            issue("c", Some("b")),
            issue("d", Some("c")),
            issue("e", Some("d")),
            issue("f", Some("e")),
        ];
        let planned: Vec<(String, Option<String>)> = plan_issues(&issues)
            .into_iter()
            .map(|(index, parent)| (issues[index].id.clone(), parent))
            .collect();
        let expected: Vec<(String, Option<String>)> = [
            ("g", None),
            ("x", None),
            ("a", None),
            ("b", Some("a")),
            ("c", Some("b")),
            ("d", Some("c")),
            ("e", Some("d")),
            ("f", Some("e")),
        ]
        .into_iter()
        .map(|(id, parent)| (id.to_owned(), parent.map(str::to_owned)))
        .collect();
        assert_eq!(
            planned, expected,
            "f is five levels below a; g starts a new tree"
        );
    }

    #[test]
    fn maps_states_priorities_and_labels() {
        assert_eq!(state_category(Some("triage")), "unstarted");
        assert_eq!(state_category(Some("backlog")), "unstarted");
        assert_eq!(state_category(Some("started")), "started");
        assert_eq!(state_category(Some("completed")), "completed");
        assert_eq!(state_category(Some("canceled")), "cancelled");
        assert!(same_status_name("Canceled", "cancelled"));
        assert!(!same_status_name("Done", "Todo"));
        assert_eq!(
            [0.0, 1.0, 2.0, 3.0, 4.0].map(priority),
            ["none", "urgent", "high", "medium", "low"]
        );
        let label = LinearLabel {
            id: "l".into(),
            name: Some("Bug".into()),
            parent: Some(NameRef {
                name: Some("Type".into()),
            }),
        };
        assert_eq!(label_name(&label).as_deref(), Some("Type/Bug"));
        assert!(is_hex_color("#5E6AD2"));
        assert!(!is_hex_color("red"));
    }

    #[test]
    fn writes_fields_orbit_lacks_into_the_description() {
        let issue = LinearIssue {
            identifier: Some("ENG-7".into()),
            parent: Some(IssueRef {
                id: "p".into(),
                identifier: Some("ENG-1".into()),
            }),
            project: Some(NameRef {
                name: Some("Launch".into()),
            }),
            cycle: Some(Cycle {
                number: Some(3.0),
                name: Some("Sprint *3*".into()),
            }),
            estimate: Some(2.0),
            ..LinearIssue::default()
        };
        assert_eq!(
            description(
                &issue,
                "Body",
                Some("Ghost"),
                true,
                &[("PR #1".into(), "https://github.com/a/b/pull/1".into())]
            ),
            "Body\n\n**Linear**\n\n- Issue: ENG-7\n- Assignee: Ghost\n- Parent: ENG-1\n- Project: Launch\n- Cycle: 3 (Sprint \\*3\\*)\n- Estimate: 2\n\n**Links**\n\n- [PR \\#1](<https://github.com/a/b/pull/1>)"
        );
    }
}
