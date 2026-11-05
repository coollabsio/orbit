//! Asana import: a member pastes an Asana personal access token, an `asana.scan` job lists the
//! workspaces and projects it can see, the member picks projects, and an `asana.import` job
//! copies them into Orbit Tasks.
//!
//! State machine (`asana_imports.status`), as for Notion imports:
//! `scanning` → `ready` → `queued` → `importing` → `completed`; any active state can become
//! `failed` or `cancelled`, and a `ready` import that is not started within 24 hours becomes
//! `expired`. The token is stored only as ciphertext under the app key and is wiped (NULL) in
//! every final state (a CHECK constraint enforces it).
//!
//! Jobs carry only the import id. Progress lives in `asana_import_items`, so a restarted job
//! continues where it stopped, and every step converges on a second run:
//! - Phase A, per selected project: create the Orbit project (its id recorded first), map the
//!   sections to statuses by name, and list its tasks and subtasks as items (a task already
//!   listed under another project is skipped).
//! - Phase B, per task, parents first: create the Orbit task (its id recorded first) silently,
//!   without notifications.
//! - Phase C, per task: comments (skipping as many as the importer already wrote on the task),
//!   Asana-hosted files (skipping ones already attached), and "blocked by" relations.
//!
//! A task that fails is recorded and the import continues; a revoked token fails the import.
//! What was imported before a failure or cancellation stays.

use std::collections::{HashMap, HashSet, VecDeque};
use std::time::Duration;

use orbit_platform::Row;
use orbit_platform::{
    Backend, Database, Id, Job, JobError, JobKind, JobKindRegistrationError, JobQueue, JobStore,
    TimestampMillis, Worker,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use thiserror::Error;
use utoipa::ToSchema;

use super::client::{AsanaClient, AsanaClientConfig, AsanaError};
use super::convert::{bold, bullet, html_to_blocks, link, paragraph, text};
use super::model::{AsanaAttachment, AsanaTask};
pub use super::scan::{AsanaImportProject, AsanaImportTree, AsanaImportWorkspace};
use super::scan::{MAX_PROJECTS, build_tree};
use crate::notion::files::sanitize_file_name;
use crate::notion::import::clamp;
use crate::repositories::attachments::AttachmentRepository;
use crate::repositories::membership;
use crate::repositories::task_relations::NewTaskRelationType;
use crate::repositories::tasks::{CommentContent, CreateTask, TaskError, TaskRepository};
use crate::secret_box::{decrypt_secret, encrypt_secret};
pub use crate::task_import::JobRun;
use crate::task_import::{
    Failure, NewProject, display_instant, ensure_project, ensure_writable, find_or_create_label,
    job_kind, member_emails, parse_date_millis, parse_instant_millis, parse_stored_id,
    project_key_base, project_statuses, retryable, stopped,
};

/// Job kind names.
pub const SCAN_JOB: &str = "asana.scan";
pub const IMPORT_JOB: &str = "asana.import";
/// Most tasks (subtasks included) one import creates.
pub const MAX_TASKS: usize = 10_000;
/// Subtask levels below a top-level task that are imported.
pub const MAX_SUBTASK_DEPTH: usize = 5;
/// A scanned import waits this long for "start" before its token is deleted.
pub const READY_TTL: Duration = Duration::from_secs(24 * 60 * 60);
/// An active import that made no progress for this long is failed by the cleanup.
const STALLED_AFTER: Duration = Duration::from_secs(24 * 60 * 60);
const MAX_TOKEN_CHARS: usize = 1_024;
const MAX_FAILURES_LISTED: i64 = 200;
const MAX_LISTED_IMPORTS: i64 = 20;
const ACTIVE: &str = "('scanning', 'queued', 'importing')";
const OPEN: &str = "('scanning', 'ready', 'queued', 'importing')";
const PROJECT_COLORS: [&str; 6] = [
    "#5e6ad2", "#26b5ce", "#4cb782", "#f2c94c", "#f2994a", "#eb5757",
];
const LABEL_COLORS: [&str; 6] = [
    "#eb5757", "#f2994a", "#f2c94c", "#4cb782", "#26b5ce", "#bb87fc",
];

/// Both job kinds: [`job_kind`], as for the Notion import.
#[must_use]
pub fn scan_job_kind() -> JobKind {
    job_kind(SCAN_JOB)
}

#[must_use]
pub fn import_job_kind() -> JobKind {
    job_kind(IMPORT_JOB)
}

/// Registers the `asana.scan` and `asana.import` handlers.
pub fn register_jobs(
    worker: Worker,
    service: AsanaImportService,
) -> Result<Worker, JobKindRegistrationError> {
    let scan = service.clone();
    worker
        .with_handler(scan_job_kind(), move |context| {
            let service = scan.clone();
            async move {
                let Some(import_id) = payload_import_id(&context.job.payload) else {
                    return Err(JobError::Permanent("invalid asana.scan payload".to_owned()));
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
                        "invalid asana.import payload".to_owned(),
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
pub struct AsanaImportSettings {
    /// Asana client settings for scan and import jobs (API root, request rate).
    pub client: AsanaClientConfig,
    /// The server app key; without it no import can store its token.
    pub app_key: Option<[u8; 32]>,
    /// Largest file the import downloads (the upload limit).
    pub max_file_bytes: u64,
}

#[derive(Clone)]
pub struct AsanaImportService {
    inner: std::sync::Arc<Inner>,
}

struct Inner {
    database: Database,
    tasks: TaskRepository,
    attachments: AttachmentRepository,
    jobs: JobQueue,
    settings: AsanaImportSettings,
}

/// Errors of the import API.
#[derive(Debug, Error)]
pub enum AsanaImportError {
    #[error("import not found")]
    NotFound,
    #[error("the app key is not configured")]
    AppKeyMissing,
    #[error("the Asana token is invalid")]
    TokenInvalid,
    #[error("Asana is unavailable")]
    AsanaUnavailable,
    #[error("another import is in progress")]
    InProgress,
    #[error("the import is not in a state that allows this")]
    StateConflict,
    #[error("invalid input: {field}")]
    Invalid { field: &'static str },
    #[error("import storage is unavailable")]
    Unavailable(String),
}

impl From<sqlx::Error> for AsanaImportError {
    fn from(error: sqlx::Error) -> Self {
        Self::Unavailable(error.to_string())
    }
}

/// `asana_imports.status`.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AsanaImportStatus {
    /// The scan job is listing workspaces and projects.
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

impl AsanaImportStatus {
    fn parse(value: &str) -> Result<Self, AsanaImportError> {
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
                return Err(AsanaImportError::Unavailable(
                    "unknown import status".to_owned(),
                ));
            }
        })
    }
}

#[derive(Clone, Copy, Debug, Serialize, ToSchema)]
pub struct AsanaImportProgress {
    /// Tasks found so far (subtasks included); grows while projects are listed.
    pub total: u64,
    /// Tasks imported with their comments, files and dependencies.
    pub done: u64,
    /// Tasks and projects that could not be imported.
    pub failed: u64,
}

/// A project or task the import could not create or fill.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct AsanaImportFailure {
    pub asana_gid: String,
    /// `project` or `task`.
    pub kind: String,
    pub title: String,
    pub error: String,
}

/// What the import carried over besides the tasks themselves.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct AsanaImportReport {
    pub comments_imported: u32,
    /// Asana-hosted files downloaded and attached to the tasks.
    pub files_imported: u32,
    /// Asana-hosted files that could not be downloaded (too large, gone, refused).
    pub missing_files: u32,
    /// Google Drive, Dropbox and other linked attachments, added to the descriptions as links.
    pub links_added: u32,
    /// "Blocked by" relations between imported tasks.
    pub dependencies_linked: u32,
    /// Tasks whose Asana assignee has no Orbit member with the same email (left unassigned).
    pub unmatched_assignees: u32,
    /// Subtasks deeper than five levels, not imported.
    pub skipped_subtasks: u32,
    /// The first 200 failures.
    pub failures: Vec<AsanaImportFailure>,
}

/// The report's counts as `asana_imports.report_json` keeps them.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
struct StoredReport {
    comments_imported: u32,
    files_imported: u32,
    missing_files: u32,
    links_added: u32,
    dependencies_linked: u32,
    unmatched_assignees: u32,
    skipped_subtasks: u32,
}

/// An Asana import. Never contains the token.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct AsanaImport {
    #[schema(value_type = String)]
    pub id: Id,
    #[schema(value_type = String)]
    pub workspace_id: Id,
    pub status: AsanaImportStatus,
    /// The Asana user the token belongs to.
    #[schema(required = true)]
    pub asana_user_name: Option<String>,
    pub progress: AsanaImportProgress,
    /// Why the import failed or expired.
    #[schema(required = true)]
    pub error: Option<String>,
    /// The scan result, once ready. Only in `GET …/imports/asana/{id}?include=tree`; `null`
    /// everywhere else, so polling stays light.
    #[schema(required = true)]
    pub tree: Option<AsanaImportTree>,
    /// Set once the import was started.
    #[schema(required = true)]
    pub report: Option<AsanaImportReport>,
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
    project_gids: Vec<String>,
}

/// What phase A recorded for a project: its sections' statuses, in board order.
#[derive(Default, Deserialize, Serialize)]
struct StoredProject {
    #[serde(default)]
    sections: Vec<(String, String)>,
}

#[derive(Clone, Debug)]
struct Item {
    asana_gid: String,
    project_gid: String,
    parent_gid: Option<String>,
    title: String,
    data_json: Option<String>,
    orbit_id: Option<Id>,
    state: String,
}

impl From<AsanaError> for Failure {
    fn from(error: AsanaError) -> Self {
        match &error {
            AsanaError::Unauthorized { .. } => {
                Self::Fatal("The Asana token was revoked or is no longer valid.".to_owned())
            }
            AsanaError::Forbidden { .. } => {
                Self::Fatal("The Asana token lost access to a selected project.".to_owned())
            }
            AsanaError::RateLimited { .. } | AsanaError::Http(_) => {
                Self::Transient(error.to_string())
            }
            AsanaError::Api { status, .. } if *status >= 500 => Self::Transient(error.to_string()),
            _ => Self::Item(error.to_string()),
        }
    }
}

/// Within one task, a resource the token cannot read fails the task, not the import.
fn task_scoped(error: AsanaError) -> Failure {
    match error {
        AsanaError::Forbidden { message } | AsanaError::NotFound { message } => {
            Failure::Item(format!("Asana refused a request for this task: {message}"))
        }
        other => Failure::from(other),
    }
}

/// The Orbit side of one imported project, built from its item.
struct ProjectContext {
    project_id: Id,
    /// Section gid → status id.
    sections: HashMap<String, Id>,
    categories: HashMap<Id, String>,
    /// Status of an open task without a usable section.
    open_status: Id,
    /// Status of a completed task whose section is not a completed status.
    completed_status: Id,
    /// Lowercase name → label id.
    labels: HashMap<String, Id>,
}

/// Shared state of one import run.
struct Run<'a> {
    import_id: Id,
    workspace_id: Id,
    user_id: Id,
    client: &'a AsanaClient,
    job: &'a JobRun,
    request_id: String,
    report: StoredReport,
    /// Lowercase email → member id.
    members: HashMap<String, Id>,
    /// Selected project gid → name.
    project_names: HashMap<String, String>,
    projects: HashMap<String, ProjectContext>,
}

impl AsanaImportService {
    #[must_use]
    pub fn new(
        database: Database,
        attachments: AttachmentRepository,
        settings: AsanaImportSettings,
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

    /// Validates `token` with Asana (`GET /users/me`), stores it encrypted and queues the scan.
    /// A member has at most one active Asana import; their unstarted scans in this workspace
    /// are cancelled (their tokens deleted).
    pub async fn create(
        &self,
        workspace_id: Id,
        user_id: Id,
        token: &str,
    ) -> Result<AsanaImport, AsanaImportError> {
        self.require_member(workspace_id, user_id).await?;
        let key = self
            .inner
            .settings
            .app_key
            .ok_or(AsanaImportError::AppKeyMissing)?;
        let token = token.trim();
        if token.is_empty()
            || token.chars().count() > MAX_TOKEN_CHARS
            || !token.chars().all(|c| c.is_ascii_graphic())
        {
            return Err(AsanaImportError::TokenInvalid);
        }
        let now = TimestampMillis::now();
        self.expire_stale(now).await?;
        let validation = AsanaClientConfig {
            max_attempts: 2,
            request_timeout: Duration::from_secs(20),
            max_retry_after: Duration::from_secs(5),
            ..self.inner.settings.client.clone()
        };
        let client =
            AsanaClient::new(token, validation).map_err(|_| AsanaImportError::TokenInvalid)?;
        let me = match client.me().await {
            Ok(me) => me,
            Err(AsanaError::Unauthorized { .. } | AsanaError::Config(_)) => {
                return Err(AsanaImportError::TokenInvalid);
            }
            Err(error) => {
                tracing::warn!(error = %error, "Asana token validation failed");
                return Err(AsanaImportError::AsanaUnavailable);
            }
        };
        let name = me
            .name
            .filter(|name| !name.trim().is_empty())
            .map(|name| clamp(name.trim(), 200, 800));
        let ciphertext = encrypt_secret(&key, token)
            .map_err(|()| AsanaImportError::Unavailable("token encryption failed".to_owned()))?;

        let id = Id::new_v7();
        let mut tx = self.database().immediate_transaction().await?;
        let active: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) FROM asana_imports WHERE user_id = $1 AND status IN {ACTIVE}"
        )))
        .bind(user_id.to_string())
        .fetch_one(&mut *tx)
        .await?;
        if active > 0 {
            return Err(AsanaImportError::InProgress);
        }
        sqlx::query(
            "UPDATE asana_imports SET status = 'cancelled', token_ciphertext = NULL, updated_at = $1 \
             WHERE user_id = $2 AND workspace_id = $3 AND status = 'ready'",
        )
        .bind(now.as_millis())
        .bind(user_id.to_string())
        .bind(workspace_id.to_string())
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO asana_imports (id, workspace_id, user_id, status, token_ciphertext, \
             asana_user_name, created_at, updated_at) VALUES ($1, $2, $3, 'scanning', $4, $5, $6, $7)",
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
    ) -> Result<Vec<AsanaImport>, AsanaImportError> {
        self.require_member(workspace_id, user_id).await?;
        self.expire_stale(TimestampMillis::now()).await?;
        let ids: Vec<String> = sqlx::query_scalar(
            "SELECT id FROM asana_imports WHERE workspace_id = $1 AND user_id = $2 \
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
                .map_err(|_| AsanaImportError::Unavailable("invalid import id".to_owned()))?;
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
    ) -> Result<AsanaImport, AsanaImportError> {
        self.require_member(workspace_id, user_id).await?;
        self.expire_stale(TimestampMillis::now()).await?;
        self.load_view(workspace_id, user_id, import_id, with_tree)
            .await
    }

    /// Plans the import of the selected projects and queues the import job. Every gid must
    /// be a project of the scan tree.
    pub async fn start(
        &self,
        workspace_id: Id,
        user_id: Id,
        import_id: Id,
        project_gids: Vec<String>,
    ) -> Result<AsanaImport, AsanaImportError> {
        self.require_member(workspace_id, user_id).await?;
        self.expire_stale(TimestampMillis::now()).await?;
        let row = sqlx::query(
            "SELECT status, tree_json FROM asana_imports WHERE id = $1 AND workspace_id = $2 AND user_id = $3",
        )
        .bind(import_id.to_string())
        .bind(workspace_id.to_string())
        .bind(user_id.to_string())
        .fetch_optional(self.database().pool())
        .await?
        .ok_or(AsanaImportError::NotFound)?;
        if row.get::<String, _>("status") != "ready" {
            return Err(AsanaImportError::StateConflict);
        }
        let tree: AsanaImportTree = serde_json::from_str(
            &row.get::<Option<String>, _>("tree_json")
                .unwrap_or_default(),
        )
        .map_err(|_| AsanaImportError::Unavailable("invalid stored tree".to_owned()))?;
        let mut seen = HashSet::new();
        let mut projects = Vec::new();
        for gid in &project_gids {
            let project = tree.project(gid).ok_or(AsanaImportError::Invalid {
                field: "project_gids",
            })?;
            if seen.insert(gid.clone()) {
                projects.push(project.clone());
            }
        }
        if projects.is_empty() || projects.len() > MAX_PROJECTS {
            return Err(AsanaImportError::Invalid {
                field: "project_gids",
            });
        }
        let selection = StoredSelection {
            project_gids: projects.iter().map(|project| project.gid.clone()).collect(),
        };

        let now = TimestampMillis::now();
        let mut tx = self.database().immediate_transaction().await?;
        let active: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) FROM asana_imports WHERE user_id = $1 AND id <> $2 AND status IN {ACTIVE}"
        )))
        .bind(user_id.to_string())
        .bind(import_id.to_string())
        .fetch_one(&mut *tx)
        .await?;
        if active > 0 {
            return Err(AsanaImportError::InProgress);
        }
        let updated = sqlx::query(
            "UPDATE asana_imports SET status = 'queued', selection_json = $1, total = 0, done = 0, \
             failed = 0, report_json = $2, updated_at = $3 \
             WHERE id = $4 AND status = 'ready' AND token_ciphertext IS NOT NULL",
        )
        .bind(serde_json::to_string(&selection).unwrap_or_default())
        .bind(serde_json::to_string(&StoredReport::default()).unwrap_or_default())
        .bind(now.as_millis())
        .bind(import_id.to_string())
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if updated != 1 {
            return Err(AsanaImportError::StateConflict);
        }
        for (position, project) in projects.iter().enumerate() {
            sqlx::query(
                "INSERT INTO asana_import_items (import_id, kind, asana_gid, project_gid, position, title) \
                 VALUES ($1, 'project', $2, $3, $4, $5)",
            )
            .bind(import_id.to_string())
            .bind(&project.gid)
            .bind(&project.gid)
            .bind(position as i64)
            .bind(&project.name)
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

    /// Cancels an open import and deletes its token. A running job stops before its next
    /// item; what it already imported stays.
    pub async fn cancel(
        &self,
        workspace_id: Id,
        user_id: Id,
        import_id: Id,
    ) -> Result<AsanaImport, AsanaImportError> {
        self.require_member(workspace_id, user_id).await?;
        let updated = sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE asana_imports SET status = 'cancelled', token_ciphertext = NULL, updated_at = $1 \
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
            return Err(AsanaImportError::StateConflict);
        }
        self.load_view(workspace_id, user_id, import_id, false)
            .await
    }

    /// Expires scanned imports that were not started within 24 hours and fails active imports
    /// that made no progress for 24 hours; both lose their token. Returns the rows changed.
    pub async fn expire_stale(&self, now: TimestampMillis) -> Result<u64, AsanaImportError> {
        let ready_cutoff = now.as_millis() - READY_TTL.as_millis() as i64;
        let stalled_cutoff = now.as_millis() - STALLED_AFTER.as_millis() as i64;
        let expired = sqlx::query(
            "UPDATE asana_imports SET status = 'expired', token_ciphertext = NULL, \
             error = 'The import was not started within 24 hours.', updated_at = $1 \
             WHERE status = 'ready' AND updated_at < $2",
        )
        .bind(now.as_millis())
        .bind(ready_cutoff)
        .execute(self.database().pool())
        .await?
        .rows_affected();
        let stalled = sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE asana_imports SET status = 'failed', token_ciphertext = NULL, \
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
            tracing::warn!(import_id = %import_id, error = %error, cleanup = %cleanup, "Asana import could not be finalized");
        }
        result
    }

    /// Fails active imports whose job is dead and that have no live job left.
    async fn fail_imports_with_dead_jobs(&self) -> Result<u64, AsanaImportError> {
        let import_id_of_job = match self.database().backend() {
            Backend::Sqlite => "json_extract(jobs.payload_json, '$.import_id')",
            Backend::Postgres => "(jobs.payload_json::jsonb ->> 'import_id')",
        };
        let failed = sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE asana_imports SET status = 'failed', token_ciphertext = NULL, \
             error = 'The import stopped after repeated errors.', updated_at = $1 \
             WHERE status IN {ACTIVE} \
             AND EXISTS (SELECT 1 FROM jobs WHERE jobs.kind IN ('{SCAN_JOB}', '{IMPORT_JOB}') \
                 AND jobs.state = 'dead' AND {import_id_of_job} = asana_imports.id) \
             AND NOT EXISTS (SELECT 1 FROM jobs WHERE jobs.kind IN ('{SCAN_JOB}', '{IMPORT_JOB}') \
                 AND jobs.state IN ('queued', 'running') AND {import_id_of_job} = asana_imports.id)"
        )))
        .bind(TimestampMillis::now().as_millis())
        .execute(self.database().pool())
        .await?
        .rows_affected();
        Ok(failed)
    }

    /// The `asana.scan` job: lists workspaces and projects and stores the tree.
    pub async fn run_scan(&self, import_id: Id, run: &JobRun) -> Result<(), JobError> {
        let Some(row) =
            sqlx::query("SELECT status, token_ciphertext FROM asana_imports WHERE id = $1")
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
                    "UPDATE asana_imports SET status = 'ready', tree_json = $1, updated_at = $2 \
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

    /// The `asana.import` job.
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
                tracing::warn!(import_id = %import_id, error = %message, "Asana import will retry");
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
            "SELECT workspace_id, user_id, status, token_ciphertext, selection_json, report_json \
             FROM asana_imports WHERE id = $1",
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
            "UPDATE asana_imports SET status = 'importing', updated_at = $1 WHERE id = $2 AND status = 'queued'",
        )
        .bind(TimestampMillis::now().as_millis())
        .bind(import_id.to_string())
        .execute(self.database().pool())
        .await?;

        let members = member_emails(self.database(), workspace_id).await?;
        let mut run = Run {
            import_id,
            workspace_id,
            user_id,
            client: &client,
            job,
            request_id: format!("asana-import-{import_id}"),
            report,
            members,
            project_names: HashMap::new(),
            projects: HashMap::new(),
        };

        // Phase A: projects, their statuses, and the task list.
        let projects: Vec<Item> = self.load_items(import_id, "project").await?;
        for project in &projects {
            run.project_names
                .insert(project.asana_gid.clone(), project.title.clone());
        }
        for (index, project) in projects.iter().enumerate() {
            if project.state == "filled" || project.state == "failed" {
                continue;
            }
            if self.should_stop(&run).await? {
                return stopped(job);
            }
            match self.prepare_project(&mut run, project, index).await {
                Ok(()) => {}
                Err(Failure::Item(message)) => {
                    self.fail_item(import_id, "project", &project.asana_gid, &message)
                        .await?;
                }
                Err(other) => return Err(other),
            }
        }
        for project in self.load_items(import_id, "project").await? {
            if project.state == "filled" {
                let context = self.project_context(&run, &project).await?;
                run.projects.insert(project.asana_gid.clone(), context);
            }
        }

        // Phase B: create every task, parents first.
        let mut tasks = self.load_items(import_id, "task").await?;
        for index in 0..tasks.len() {
            if tasks[index].state != "pending" {
                continue;
            }
            if self.should_stop(&run).await? {
                self.save_report(&run).await?;
                return stopped(job);
            }
            let parent = tasks[index].parent_gid.as_ref().and_then(|parent| {
                tasks
                    .iter()
                    .find(|item| {
                        &item.asana_gid == parent
                            && matches!(item.state.as_str(), "created" | "filled")
                    })
                    .and_then(|item| item.orbit_id)
            });
            match self.create_task(&mut run, &mut tasks[index], parent).await {
                Ok(()) => tasks[index].state = "created".to_owned(),
                Err(Failure::Item(message)) => {
                    self.fail_item(import_id, "task", &tasks[index].asana_gid, &message)
                        .await?;
                    tasks[index].state = "failed".to_owned();
                }
                Err(other) => {
                    self.save_report(&run).await?;
                    return Err(other);
                }
            }
        }
        self.save_report(&run).await?;

        // Phase C: comments, files and dependencies.
        let created: HashMap<String, Id> = tasks
            .iter()
            .filter(|item| matches!(item.state.as_str(), "created" | "filled"))
            .filter_map(|item| Some((item.asana_gid.clone(), item.orbit_id?)))
            .collect();
        for item in &tasks {
            if item.state != "created" {
                continue;
            }
            if self.should_stop(&run).await? {
                self.save_report(&run).await?;
                return stopped(job);
            }
            match self.fill_task(&mut run, item, &created).await {
                Ok(()) => {
                    sqlx::query(
                        "UPDATE asana_import_items SET state = 'filled', data_json = NULL, error = NULL \
                         WHERE import_id = $1 AND kind = 'task' AND asana_gid = $2",
                    )
                    .bind(import_id.to_string())
                    .bind(&item.asana_gid)
                    .execute(self.database().pool())
                    .await?;
                    sqlx::query(
                        "UPDATE asana_imports SET done = done + 1, updated_at = $1 WHERE id = $2",
                    )
                    .bind(TimestampMillis::now().as_millis())
                    .bind(import_id.to_string())
                    .execute(self.database().pool())
                    .await?;
                    self.save_report(&run).await?;
                }
                Err(Failure::Item(message)) => {
                    self.fail_item(import_id, "task", &item.asana_gid, &message)
                        .await?;
                    self.save_report(&run).await?;
                }
                Err(other) => {
                    self.save_report(&run).await?;
                    return Err(other);
                }
            }
        }

        sqlx::query(
            "UPDATE asana_imports SET status = 'completed', token_ciphertext = NULL, updated_at = $1 \
             WHERE id = $2 AND status = 'importing'",
        )
        .bind(TimestampMillis::now().as_millis())
        .bind(import_id.to_string())
        .execute(self.database().pool())
        .await?;
        Ok(())
    }

    /// Phase A for one project: the Orbit project, a status per section, and the task items.
    async fn prepare_project(
        &self,
        run: &mut Run<'_>,
        item: &Item,
        index: usize,
    ) -> Result<(), Failure> {
        let project_id = self.ensure_project(run, item, index).await?;
        let sections = run.client.sections(&item.asana_gid).await?;
        let mut statuses = project_statuses(self.database(), project_id).await?;
        let mut mapped = Vec::with_capacity(sections.len());
        for (position, section) in sections.iter().enumerate() {
            let name = section
                .name
                .as_deref()
                .map(|name| clamp(name.trim(), 100, 100))
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| "Untitled section".to_owned());
            let existing = statuses.iter().find(|(_, existing, category)| {
                category != "duplicate" && existing.to_lowercase() == name.to_lowercase()
            });
            let status_id = match existing {
                Some((id, _, _)) => *id,
                None => {
                    let category = section_category(&name, position);
                    let color = match category {
                        "completed" => "#4cb782",
                        "started" => "#f2c94c",
                        _ => "#8b8f98",
                    };
                    let status = self
                        .inner
                        .tasks
                        .create_status(
                            run.workspace_id,
                            project_id,
                            run.user_id,
                            name.clone(),
                            String::new(),
                            color.to_owned(),
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
            mapped.push((section.gid.clone(), status_id.to_string()));
        }
        sqlx::query(
            "UPDATE asana_import_items SET data_json = $1 WHERE import_id = $2 AND kind = 'project' AND asana_gid = $3",
        )
        .bind(serde_json::to_string(&StoredProject { sections: mapped }).unwrap_or_default())
        .bind(run.import_id.to_string())
        .bind(&item.asana_gid)
        .execute(self.database().pool())
        .await?;
        self.list_tasks(run, &item.asana_gid).await?;
        sqlx::query(
            "UPDATE asana_import_items SET state = 'filled' WHERE import_id = $1 AND kind = 'project' AND asana_gid = $2",
        )
        .bind(run.import_id.to_string())
        .bind(&item.asana_gid)
        .execute(self.database().pool())
        .await?;
        self.touch(run.import_id).await?;
        Ok(())
    }

    /// Creates the Orbit project of `item` unless an earlier attempt did; returns its id.
    async fn ensure_project(
        &self,
        run: &Run<'_>,
        item: &Item,
        index: usize,
    ) -> Result<Id, Failure> {
        let project_id = match item.orbit_id {
            Some(id) => id,
            None => {
                let id = Id::new_v7();
                self.record_orbit_id(run.import_id, "project", &item.asana_gid, id)
                    .await?;
                id
            }
        };
        ensure_project(
            self.database(),
            &self.inner.tasks,
            NewProject {
                id: project_id,
                workspace_id: run.workspace_id,
                user_id: run.user_id,
                name: &item.title,
                fallback_name: "Asana project",
                key_base: project_key_base(&clamp(item.title.trim(), 200, 200), "ASANA"),
                color: PROJECT_COLORS[index % PROJECT_COLORS.len()],
                request_id: &run.request_id,
            },
        )
        .await?;
        sqlx::query(
            "UPDATE asana_import_items SET state = 'created' WHERE import_id = $1 AND kind = 'project' \
             AND asana_gid = $2 AND state = 'pending'",
        )
        .bind(run.import_id.to_string())
        .bind(&item.asana_gid)
        .execute(self.database().pool())
        .await?;
        Ok(project_id)
    }

    /// Lists the project's tasks and their subtasks (breadth first, so parents come first) as
    /// pending items. A task already listed (under another project) keeps its first place.
    async fn list_tasks(&self, run: &mut Run<'_>, project_gid: &str) -> Result<(), Failure> {
        let listed: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM asana_import_items WHERE import_id = $1 AND kind = 'task'",
        )
        .bind(run.import_id.to_string())
        .fetch_one(self.database().pool())
        .await?;
        let room = MAX_TASKS.saturating_sub(usize::try_from(listed).unwrap_or(MAX_TASKS));
        let too_many = || {
            Failure::Fatal(format!(
                "The selected projects have more than {MAX_TASKS} tasks. Import fewer projects at a time."
            ))
        };
        let mut found: Vec<(AsanaTask, Option<String>)> = Vec::new();
        let mut queue: VecDeque<(String, usize)> = VecDeque::new();
        for task in run.client.project_tasks(project_gid, room + 1).await? {
            if task.num_subtasks > 0 {
                queue.push_back((task.gid.clone(), 1));
            }
            found.push((task, None));
        }
        while let Some((parent, depth)) = queue.pop_front() {
            if found.len() > room {
                break;
            }
            if self.should_stop(run).await? {
                return stopped(run.job);
            }
            let children = match run.client.subtasks(&parent, room + 1).await {
                Ok(children) => children,
                Err(AsanaError::NotFound { .. } | AsanaError::Forbidden { .. }) => Vec::new(),
                Err(error) => return Err(Failure::from(error)),
            };
            for child in children {
                if depth >= MAX_SUBTASK_DEPTH {
                    run.report.skipped_subtasks += 1;
                    continue;
                }
                if child.num_subtasks > 0 {
                    queue.push_back((child.gid.clone(), depth + 1));
                }
                found.push((child, Some(parent.clone())));
            }
        }
        if found.len() > room {
            return Err(too_many());
        }
        let mut tx = self.database().immediate_transaction().await?;
        let mut inserted = 0;
        for (task, parent) in &found {
            let title = task.name.clone().unwrap_or_default();
            inserted += sqlx::query(
                "INSERT INTO asana_import_items (import_id, kind, asana_gid, project_gid, parent_gid, \
                 position, title, data_json) VALUES ($1, 'task', $2, $3, $4, \
                 (SELECT COALESCE(MAX(position) + 1, 0) FROM asana_import_items WHERE import_id = $5), $6, $7) \
                 ON CONFLICT (import_id, kind, asana_gid) DO NOTHING",
            )
            .bind(run.import_id.to_string())
            .bind(&task.gid)
            .bind(project_gid)
            .bind(parent.as_deref())
            .bind(run.import_id.to_string())
            .bind(clamp(&title, 500, 2_000))
            .bind(serde_json::to_string(task).unwrap_or_default())
            .execute(&mut *tx)
            .await?
            .rows_affected();
        }
        sqlx::query("UPDATE asana_imports SET total = total + $1, updated_at = $2 WHERE id = $3")
            .bind(inserted as i64)
            .bind(TimestampMillis::now().as_millis())
            .bind(run.import_id.to_string())
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Statuses and labels of a project whose phase A finished.
    async fn project_context(&self, run: &Run<'_>, item: &Item) -> Result<ProjectContext, Failure> {
        let project_id = item
            .orbit_id
            .ok_or_else(|| Failure::Fatal("Stored import data is invalid.".to_owned()))?;
        let stored: StoredProject = item
            .data_json
            .as_deref()
            .and_then(|json| serde_json::from_str(json).ok())
            .unwrap_or_default();
        let statuses = project_statuses(self.database(), project_id).await?;
        let categories: HashMap<Id, String> = statuses
            .iter()
            .map(|(id, _, category)| (*id, category.clone()))
            .collect();
        let sections: HashMap<String, Id> = stored
            .sections
            .iter()
            .filter_map(|(gid, id)| Some((gid.clone(), id.parse().ok()?)))
            .filter(|(_, id)| categories.contains_key(id))
            .collect();
        let is = |id: &Id, wanted: &str| categories.get(id).is_some_and(|c| c == wanted);
        let open_status = stored
            .sections
            .iter()
            .filter_map(|(_, id)| id.parse::<Id>().ok())
            .find(|id| is(id, "unstarted") || is(id, "started"))
            .or_else(|| {
                statuses
                    .iter()
                    .find(|(_, name, category)| category == "unstarted" && name == "Todo")
                    .or_else(|| {
                        statuses
                            .iter()
                            .find(|(_, _, category)| category == "unstarted")
                    })
                    .map(|(id, _, _)| *id)
            })
            .ok_or_else(|| Failure::Item("The project has no open status.".to_owned()))?;
        let completed_status = statuses
            .iter()
            .find(|(_, name, category)| {
                category == "completed" && name.eq_ignore_ascii_case("done")
            })
            .or_else(|| {
                statuses
                    .iter()
                    .find(|(_, _, category)| category == "completed")
            })
            .map(|(id, _, _)| *id)
            .ok_or_else(|| Failure::Item("The project has no completed status.".to_owned()))?;
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
        Ok(ProjectContext {
            project_id,
            sections,
            categories,
            open_status,
            completed_status,
            labels,
        })
    }

    /// Phase B for one task: its Orbit task, created silently under the recorded id.
    async fn create_task(
        &self,
        run: &mut Run<'_>,
        item: &mut Item,
        parent: Option<Id>,
    ) -> Result<(), Failure> {
        if !run.projects.contains_key(&item.project_gid) {
            return Err(Failure::Item(
                "Its project could not be imported.".to_owned(),
            ));
        }
        let mut task: AsanaTask = item
            .data_json
            .as_deref()
            .and_then(|json| serde_json::from_str(json).ok())
            .ok_or_else(|| Failure::Item("The stored Asana task is invalid.".to_owned()))?;
        let task_id = match item.orbit_id {
            Some(id) => id,
            None => {
                let id = Id::new_v7();
                self.record_orbit_id(run.import_id, "task", &item.asana_gid, id)
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
            if task.attachments.is_none() {
                // Listed once and kept: links go into the description, files are fetched in
                // phase C.
                task.attachments = Some(
                    run.client
                        .attachments(&task.gid)
                        .await
                        .map_err(task_scoped)?,
                );
                sqlx::query(
                    "UPDATE asana_import_items SET data_json = $1 WHERE import_id = $2 AND kind = 'task' AND asana_gid = $3",
                )
                .bind(serde_json::to_string(&task).unwrap_or_default())
                .bind(run.import_id.to_string())
                .bind(&item.asana_gid)
                .execute(self.database().pool())
                .await?;
                item.data_json = serde_json::to_string(&task).ok();
            }
            let mut input = self
                .task_input(run, &item.project_gid, &task, parent)
                .await?;
            let completed_at = task
                .completed_at
                .as_deref()
                .filter(|_| task.completed)
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
        sqlx::query(
            "UPDATE asana_import_items SET state = 'created' WHERE import_id = $1 AND kind = 'task' AND asana_gid = $2",
        )
        .bind(run.import_id.to_string())
        .bind(&item.asana_gid)
        .execute(self.database().pool())
        .await?;
        self.touch(run.import_id).await?;
        Ok(())
    }

    /// The Orbit task for an Asana task: status from its section, priority, assignee by email,
    /// labels from tags, dates, and a description with what Orbit has no field for.
    async fn task_input(
        &self,
        run: &mut Run<'_>,
        project_gid: &str,
        task: &AsanaTask,
        parent: Option<Id>,
    ) -> Result<CreateTask, Failure> {
        let mut labels = Vec::new();
        for tag in &task.tags {
            let Some(name) = tag
                .name
                .as_deref()
                .map(|name| clamp(name.trim(), 100, 100))
                .filter(|name| !name.is_empty())
            else {
                continue;
            };
            let label = self.ensure_label(run, project_gid, &name).await?;
            if !labels.contains(&label) {
                labels.push(label);
            }
        }
        let project = run
            .projects
            .get(project_gid)
            .ok_or_else(|| Failure::Item("Its project could not be imported.".to_owned()))?;
        let section_status = task
            .memberships
            .iter()
            .filter(|membership| {
                membership
                    .project
                    .as_ref()
                    .map(|project| project.gid.as_str())
                    == Some(project_gid)
            })
            .find_map(|membership| membership.section.as_ref())
            .and_then(|section| project.sections.get(&section.gid))
            .copied();
        let closed = |id: &Id| {
            project
                .categories
                .get(id)
                .is_some_and(|category| category == "completed" || category == "cancelled")
        };
        let status_id = match section_status {
            Some(status)
                if task.completed
                    && project
                        .categories
                        .get(&status)
                        .is_some_and(|c| c == "completed") =>
            {
                status
            }
            _ if task.completed => project.completed_status,
            Some(status) if !closed(&status) => status,
            _ => project.open_status,
        };
        let assignee = task.assignee.as_ref().and_then(|assignee| {
            assignee
                .email
                .as_deref()
                .and_then(|email| run.members.get(&email.trim().to_lowercase()))
                .copied()
        });
        let unmatched = match (&task.assignee, assignee) {
            (Some(asana), None) => {
                run.report.unmatched_assignees += 1;
                Some(
                    asana
                        .name
                        .clone()
                        .or_else(|| asana.email.clone())
                        .unwrap_or_else(|| "unknown".to_owned()),
                )
            }
            _ => None,
        };
        let other_projects: Vec<String> = task
            .memberships
            .iter()
            .filter_map(|membership| membership.project.as_ref())
            .filter(|other| other.gid != project_gid)
            .filter_map(|other| run.project_names.get(&other.gid).cloned())
            .collect();
        let links: Vec<(String, String)> = task
            .attachments
            .iter()
            .flatten()
            .filter(|attachment| !attachment.is_hosted())
            .filter_map(|attachment| {
                let url = attachment
                    .view_url
                    .clone()
                    .or_else(|| attachment.permanent_url.clone())
                    .filter(|url| url.starts_with("https://") || url.starts_with("http://"))?;
                Some((attachment.name.clone().unwrap_or_else(|| url.clone()), url))
            })
            .collect();
        run.report.links_added += u32::try_from(links.len()).unwrap_or(u32::MAX);
        let extras = Extras {
            fields: task
                .custom_fields
                .iter()
                .filter(|field| !is_priority_field(field.name.as_deref()))
                .filter_map(|field| {
                    let value = field.display_value.as_deref()?.trim();
                    (!value.is_empty()).then(|| {
                        (
                            field.name.clone().unwrap_or_else(|| "Field".to_owned()),
                            value.to_owned(),
                        )
                    })
                })
                .collect(),
            assignee: unmatched,
            other_projects,
            links,
        };
        let (description, description_json) = description(task, &extras);
        let due_at = task
            .due_at
            .as_deref()
            .and_then(parse_instant_millis)
            .or_else(|| task.due_on.as_deref().and_then(parse_date_millis));
        let start = task
            .start_at
            .as_deref()
            .and_then(parse_instant_millis)
            .or_else(|| task.start_on.as_deref().and_then(parse_date_millis))
            .filter(|start| due_at.is_some_and(|due| *start <= due));
        let title = clamp(task.name.as_deref().unwrap_or_default().trim(), 500, 500);
        Ok(CreateTask {
            project_id: project.project_id,
            status_id,
            title: if title.is_empty() {
                "Untitled task".to_owned()
            } else {
                title
            },
            description,
            description_json,
            source_url: task
                .permalink_url
                .clone()
                .filter(|url| url.starts_with("https://") && url.len() <= 2_048),
            priority: priority(task).to_owned(),
            position: None,
            assignee_ids: assignee.into_iter().collect(),
            label_ids: labels,
            due_start_at: start.map(TimestampMillis::from_millis),
            due_at: due_at.map(TimestampMillis::from_millis),
            parent_task_id: parent,
        })
    }

    /// The project's label with this name, created on first use.
    async fn ensure_label(
        &self,
        run: &mut Run<'_>,
        project_gid: &str,
        name: &str,
    ) -> Result<Id, Failure> {
        let key = name.to_lowercase();
        let project = run
            .projects
            .get(project_gid)
            .ok_or_else(|| Failure::Item("Its project could not be imported.".to_owned()))?;
        if let Some(id) = project.labels.get(&key) {
            return Ok(*id);
        }
        let color = LABEL_COLORS[project.labels.len() % LABEL_COLORS.len()];
        let project_id = project.project_id;
        let id = find_or_create_label(
            self.database(),
            &self.inner.tasks,
            run.workspace_id,
            project_id,
            run.user_id,
            name,
            color,
            &run.request_id,
        )
        .await?;
        if let Some(project) = run.projects.get_mut(project_gid) {
            project.labels.insert(key, id);
        }
        Ok(id)
    }

    /// Phase C for one task: comments, files and "blocked by" relations. Each step skips what
    /// an earlier attempt already did.
    async fn fill_task(
        &self,
        run: &mut Run<'_>,
        item: &Item,
        created: &HashMap<String, Id>,
    ) -> Result<(), Failure> {
        let task_id = item
            .orbit_id
            .ok_or_else(|| Failure::Item("The task was not created.".to_owned()))?;
        let task: AsanaTask = item
            .data_json
            .as_deref()
            .and_then(|json| serde_json::from_str(json).ok())
            .unwrap_or_default();

        // Comments, in Asana order. The import writes them in that order, so the importer's
        // comments already on the task are the first ones.
        let stories = run
            .client
            .stories(&item.asana_gid)
            .await
            .map_err(task_scoped)?;
        let written: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM task_comments WHERE task_id = $1 AND author_id = $2",
        )
        .bind(task_id.to_string())
        .bind(run.user_id.to_string())
        .fetch_one(self.database().pool())
        .await?;
        for story in stories
            .iter()
            .filter(|story| story.is_comment())
            .skip(usize::try_from(written).unwrap_or_default())
        {
            let author = story
                .created_by
                .as_ref()
                .and_then(|author| author.name.clone())
                .unwrap_or_else(|| "Asana user".to_owned());
            let mut header = vec![bold(&author)];
            if let Some(date) = story.created_at.as_deref().map(display_instant) {
                header.push(text(&format!(" · {date}")));
            }
            let mut blocks = vec![paragraph(header)];
            match story.html_text.as_deref().map(html_to_blocks) {
                Some(body) if !body.is_empty() => blocks.extend(body),
                _ => {
                    for line in story.text.as_deref().unwrap_or_default().lines() {
                        blocks.push(paragraph(vec![text(line)]));
                    }
                }
            }
            self.inner
                .tasks
                .create_comment(
                    run.workspace_id,
                    task_id,
                    run.user_id,
                    None,
                    CommentContent::Blocks(blocks),
                    &run.request_id,
                    TimestampMillis::now(),
                )
                .await?;
            run.report.comments_imported += 1;
        }

        // Asana-hosted files. The download URL lives about two minutes, so each attachment
        // is read right before its download.
        let attached = self
            .inner
            .attachments
            .task_file_hashes(run.workspace_id, task_id)
            .await
            .map_err(|error| Failure::Transient(error.to_string()))?;
        let max_bytes = self.inner.settings.max_file_bytes;
        let hosted: Vec<&AsanaAttachment> = task
            .attachments
            .iter()
            .flatten()
            .filter(|attachment| attachment.is_hosted())
            .collect();
        for listed in hosted {
            if listed.size.is_some_and(|size| size > max_bytes) {
                run.report.missing_files += 1;
                continue;
            }
            let file = match run.client.attachment(&listed.gid).await {
                Ok(file) => file,
                Err(AsanaError::Unauthorized { message }) => {
                    return Err(Failure::from(AsanaError::Unauthorized { message }));
                }
                Err(error @ (AsanaError::RateLimited { .. } | AsanaError::Http(_))) => {
                    return Err(Failure::from(error));
                }
                Err(_) => {
                    run.report.missing_files += 1;
                    continue;
                }
            };
            let Some(url) = file.download_url.as_deref() else {
                run.report.missing_files += 1;
                continue;
            };
            let bytes = match run.client.download_file(url, max_bytes).await {
                Ok(bytes) => bytes,
                Err(error) => {
                    tracing::debug!(error = %error, "Asana file download skipped");
                    run.report.missing_files += 1;
                    continue;
                }
            };
            let sha256 = format!("{:x}", Sha256::digest(&bytes.bytes));
            if attached.iter().any(|(hash, size)| {
                *hash == sha256 && usize::try_from(*size).ok() == Some(bytes.bytes.len())
            }) {
                continue;
            }
            let name = sanitize_file_name(
                file.name
                    .as_deref()
                    .or(listed.name.as_deref())
                    .filter(|name| !name.trim().is_empty())
                    .unwrap_or(&bytes.file_name),
            );
            match self
                .inner
                .attachments
                .create_from_bytes(run.workspace_id, task_id, run.user_id, &name, &bytes.bytes)
                .await
            {
                Ok(_) => run.report.files_imported += 1,
                Err(error) => {
                    tracing::debug!(error = %error, "Asana file could not be attached");
                    run.report.missing_files += 1;
                }
            }
        }

        // Dependencies between tasks of this import. Free Asana plans have none (402).
        let dependencies = match run.client.dependencies(&item.asana_gid).await {
            Ok(dependencies) => dependencies,
            Err(
                AsanaError::PaymentRequired { .. }
                | AsanaError::Forbidden { .. }
                | AsanaError::NotFound { .. },
            ) => Vec::new(),
            Err(error) => return Err(Failure::from(error)),
        };
        for dependency in dependencies {
            let Some(blocker) = created.get(&dependency.gid) else {
                continue;
            };
            match self
                .inner
                .tasks
                .add_task_relation(
                    run.workspace_id,
                    task_id,
                    run.user_id,
                    NewTaskRelationType::BlockedBy,
                    *blocker,
                    &run.request_id,
                    TimestampMillis::now(),
                )
                .await
            {
                Ok(_) => run.report.dependencies_linked += 1,
                Err(TaskError::Unavailable(error)) => return Err(Failure::from(error)),
                // A blocker deleted meanwhile, or a reverse relation someone added.
                Err(_) => {}
            }
        }
        Ok(())
    }

    /// Whether to stop before the next item; a workspace that became read-only fails the
    /// import.
    async fn should_stop(&self, run: &Run<'_>) -> Result<bool, Failure> {
        if run.job.cancel.is_cancelled() {
            return Ok(true);
        }
        ensure_writable(self.database(), run.workspace_id).await?;
        let status: Option<String> =
            sqlx::query_scalar("SELECT status FROM asana_imports WHERE id = $1")
                .bind(run.import_id.to_string())
                .fetch_optional(self.database().pool())
                .await?;
        Ok(status.as_deref() != Some("importing"))
    }

    async fn record_orbit_id(
        &self,
        import_id: Id,
        kind: &str,
        asana_gid: &str,
        orbit_id: Id,
    ) -> Result<(), Failure> {
        sqlx::query(
            "UPDATE asana_import_items SET orbit_id = $1 WHERE import_id = $2 AND kind = $3 AND asana_gid = $4",
        )
        .bind(orbit_id.to_string())
        .bind(import_id.to_string())
        .bind(kind)
        .bind(asana_gid)
        .execute(self.database().pool())
        .await?;
        Ok(())
    }

    async fn fail_item(
        &self,
        import_id: Id,
        kind: &str,
        asana_gid: &str,
        message: &str,
    ) -> Result<(), Failure> {
        let mut tx = self.database().immediate_transaction().await?;
        let changed = sqlx::query(
            "UPDATE asana_import_items SET state = 'failed', error = $1, data_json = NULL \
             WHERE import_id = $2 AND kind = $3 AND asana_gid = $4 AND state <> 'failed'",
        )
        .bind(clamp(message, 500, 2_000))
        .bind(import_id.to_string())
        .bind(kind)
        .bind(asana_gid)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        sqlx::query("UPDATE asana_imports SET failed = failed + $1, updated_at = $2 WHERE id = $3")
            .bind(changed as i64)
            .bind(TimestampMillis::now().as_millis())
            .bind(import_id.to_string())
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn touch(&self, import_id: Id) -> Result<(), Failure> {
        sqlx::query("UPDATE asana_imports SET updated_at = $1 WHERE id = $2")
            .bind(TimestampMillis::now().as_millis())
            .bind(import_id.to_string())
            .execute(self.database().pool())
            .await?;
        Ok(())
    }

    async fn save_report(&self, run: &Run<'_>) -> Result<(), Failure> {
        sqlx::query("UPDATE asana_imports SET report_json = $1, updated_at = $2 WHERE id = $3")
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
            "UPDATE asana_imports SET status = $1, token_ciphertext = NULL, error = $2, updated_at = $3 \
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

    fn client_for(&self, ciphertext: Option<Vec<u8>>) -> Result<AsanaClient, String> {
        let key = self
            .inner
            .settings
            .app_key
            .ok_or_else(|| "The server app key is not configured.".to_owned())?;
        let ciphertext = ciphertext.ok_or_else(|| "The Asana token was deleted.".to_owned())?;
        let token = decrypt_secret(&key, &ciphertext)
            .map_err(|()| "The Asana token could not be decrypted.".to_owned())?;
        AsanaClient::new(&token, self.inner.settings.client.clone())
            .map_err(|_| "The Asana token is invalid.".to_owned())
    }

    async fn enqueue(
        &self,
        kind: JobKind,
        import_id: Id,
        workspace_id: Id,
    ) -> Result<(), AsanaImportError> {
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
            .map_err(|error| AsanaImportError::Unavailable(error.to_string()))
    }

    async fn require_member(&self, workspace_id: Id, user_id: Id) -> Result<(), AsanaImportError> {
        membership::actor(self.database().pool(), workspace_id, user_id)
            .await?
            .map(|_| ())
            .ok_or(AsanaImportError::NotFound)
    }

    async fn load_items(&self, import_id: Id, kind: &str) -> Result<Vec<Item>, Failure> {
        let rows = sqlx::query(
            "SELECT asana_gid, project_gid, parent_gid, title, data_json, orbit_id, state \
             FROM asana_import_items WHERE import_id = $1 AND kind = $2 ORDER BY position",
        )
        .bind(import_id.to_string())
        .bind(kind)
        .fetch_all(self.database().pool())
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok(Item {
                    asana_gid: row.get("asana_gid"),
                    project_gid: row.get("project_gid"),
                    parent_gid: row.get("parent_gid"),
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
    ) -> Result<AsanaImport, AsanaImportError> {
        let row = sqlx::query(
            "SELECT id, workspace_id, status, asana_user_name, \
             CASE WHEN $1 THEN tree_json END AS tree_json, selection_json, total, done, failed, \
             report_json, error, created_at, updated_at FROM asana_imports \
             WHERE id = $2 AND workspace_id = $3 AND user_id = $4",
        )
        .bind(with_tree)
        .bind(import_id.to_string())
        .bind(workspace_id.to_string())
        .bind(user_id.to_string())
        .fetch_optional(self.database().pool())
        .await?
        .ok_or(AsanaImportError::NotFound)?;
        let invalid = |_| AsanaImportError::Unavailable("invalid stored import".to_owned());
        let status = AsanaImportStatus::parse(&row.get::<String, _>("status"))?;
        let started = row.get::<Option<String>, _>("selection_json").is_some();
        let tree = row
            .get::<Option<String>, _>("tree_json")
            .map(|json| serde_json::from_str::<AsanaImportTree>(&json))
            .transpose()
            .map_err(|_| AsanaImportError::Unavailable("invalid stored tree".to_owned()))?;
        let report = if started {
            let stored: StoredReport = row
                .get::<Option<String>, _>("report_json")
                .and_then(|json| serde_json::from_str(&json).ok())
                .unwrap_or_default();
            let failures = sqlx::query(
                "SELECT asana_gid, kind, title, error FROM asana_import_items \
                 WHERE import_id = $1 AND state = 'failed' ORDER BY position LIMIT $2",
            )
            .bind(import_id.to_string())
            .bind(MAX_FAILURES_LISTED)
            .fetch_all(self.database().pool())
            .await?
            .into_iter()
            .map(|row| AsanaImportFailure {
                asana_gid: row.get("asana_gid"),
                kind: row.get("kind"),
                title: row.get("title"),
                error: row.get::<Option<String>, _>("error").unwrap_or_default(),
            })
            .collect();
            Some(AsanaImportReport {
                comments_imported: stored.comments_imported,
                files_imported: stored.files_imported,
                missing_files: stored.missing_files,
                links_added: stored.links_added,
                dependencies_linked: stored.dependencies_linked,
                unmatched_assignees: stored.unmatched_assignees,
                skipped_subtasks: stored.skipped_subtasks,
                failures,
            })
        } else {
            None
        };
        let project_ids = sqlx::query_scalar::<_, String>(
            "SELECT items.orbit_id FROM asana_import_items AS items \
             JOIN projects ON projects.id = items.orbit_id \
             WHERE items.import_id = $1 AND items.kind = 'project' AND projects.workspace_id = $2 \
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
        Ok(AsanaImport {
            id: row.get::<String, _>("id").parse().map_err(invalid)?,
            workspace_id: row
                .get::<String, _>("workspace_id")
                .parse()
                .map_err(invalid)?,
            status,
            asana_user_name: row.get("asana_user_name"),
            progress: AsanaImportProgress {
                total: count("total"),
                done: count("done"),
                failed: count("failed"),
            },
            error: row.get("error"),
            tree,
            report,
            project_ids,
            created_at: TimestampMillis::from_millis(row.get("created_at")),
            updated_at: TimestampMillis::from_millis(row.get("updated_at")),
        })
    }
}

/// The status category of a new status for an Asana section: a done-looking name is
/// `completed`, the first section `unstarted`, the others `started`.
fn section_category(name: &str, position: usize) -> &'static str {
    let lower = name.trim().to_lowercase();
    if ["done", "complete", "completed", "closed", "shipped"].contains(&lower.as_str()) {
        "completed"
    } else if position == 0 {
        "unstarted"
    } else {
        "started"
    }
}

fn is_priority_field(name: Option<&str>) -> bool {
    name.is_some_and(|name| name.trim().eq_ignore_ascii_case("priority"))
}

/// Orbit priority from a custom field named "Priority".
fn priority(task: &AsanaTask) -> &'static str {
    let value = task
        .custom_fields
        .iter()
        .find(|field| is_priority_field(field.name.as_deref()))
        .and_then(|field| {
            field
                .enum_value
                .as_ref()
                .and_then(|value| value.name.clone())
                .or_else(|| field.display_value.clone())
        })
        .map(|value| value.trim().to_lowercase());
    match value.as_deref() {
        Some("low") => "low",
        Some("medium" | "med") => "medium",
        Some("high") => "high",
        Some("urgent" | "critical" | "highest") => "urgent",
        _ => "none",
    }
}

/// What the description carries besides the task's notes.
struct Extras {
    fields: Vec<(String, String)>,
    assignee: Option<String>,
    other_projects: Vec<String>,
    links: Vec<(String, String)>,
}

/// The description: rich text as BlockNote blocks, or plain `notes` as markdown when the task
/// has no rich text. Either way followed by the extras.
fn description(task: &AsanaTask, extras: &Extras) -> (String, Option<Vec<Value>>) {
    let rich = task
        .html_notes
        .as_deref()
        .map(html_to_blocks)
        .unwrap_or_default();
    let notes = task.notes.as_deref().unwrap_or_default().trim();
    if rich.is_empty() && !notes.is_empty() {
        let mut markdown = notes.to_owned();
        if let Some(name) = &extras.assignee {
            markdown.push_str(&format!("\n\nAsana assignee: {name}"));
        }
        if !extras.other_projects.is_empty() {
            markdown.push_str(&format!(
                "\n\nAlso in Asana projects: {}",
                extras.other_projects.join(", ")
            ));
        }
        if !extras.fields.is_empty() {
            markdown.push_str("\n\n**Fields**\n");
            for (name, value) in &extras.fields {
                markdown.push_str(&format!("\n- {name}: {value}"));
            }
        }
        if !extras.links.is_empty() {
            markdown.push_str("\n\n**Attachments**\n");
            for (name, url) in &extras.links {
                markdown.push_str(&format!("\n- [{}](<{url}>)", name.replace(['[', ']'], "")));
            }
        }
        return (markdown, None);
    }
    let mut blocks = rich;
    if let Some(name) = &extras.assignee {
        blocks.push(paragraph(vec![text(&format!("Asana assignee: {name}"))]));
    }
    if !extras.other_projects.is_empty() {
        blocks.push(paragraph(vec![text(&format!(
            "Also in Asana projects: {}",
            extras.other_projects.join(", ")
        ))]));
    }
    if !extras.fields.is_empty() {
        blocks.push(paragraph(vec![bold("Fields")]));
        for (name, value) in &extras.fields {
            blocks.push(bullet(vec![text(&format!("{name}: {value}"))]));
        }
    }
    if !extras.links.is_empty() {
        blocks.push(paragraph(vec![bold("Attachments")]));
        for (name, url) in &extras.links {
            blocks.push(bullet(vec![link(url, name)]));
        }
    }
    if blocks.is_empty() {
        (String::new(), None)
    } else {
        (String::new(), Some(blocks))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asana::model::{CustomField, Named};

    #[test]
    fn derives_categories_and_priorities() {
        assert_eq!(section_category("Shipped", 3), "completed");
        assert_eq!(section_category("To do", 0), "unstarted");
        assert_eq!(section_category("Doing", 1), "started");
        let task = |value: &str| AsanaTask {
            custom_fields: vec![CustomField {
                name: Some("priority".into()),
                display_value: Some(value.into()),
                enum_value: Some(Named {
                    gid: "1".into(),
                    name: Some(value.into()),
                }),
            }],
            ..AsanaTask::default()
        };
        assert_eq!(priority(&task("Critical")), "urgent");
        assert_eq!(priority(&task("Med")), "medium");
        assert_eq!(priority(&task("Someday")), "none");
        assert_eq!(priority(&AsanaTask::default()), "none");
    }

    #[test]
    fn plain_notes_become_markdown_with_extras() {
        let task = AsanaTask {
            notes: Some("Line one".into()),
            ..AsanaTask::default()
        };
        let extras = Extras {
            fields: vec![("Team".into(), "Web".into())],
            assignee: Some("Grace".into()),
            other_projects: vec!["Ops".into()],
            links: vec![("Spec [v2]".into(), "https://drive.google.com/x".into())],
        };
        let (markdown, blocks) = description(&task, &extras);
        assert!(blocks.is_none());
        assert_eq!(
            markdown,
            "Line one\n\nAsana assignee: Grace\n\nAlso in Asana projects: Ops\n\n**Fields**\n\n- Team: Web\n\n**Attachments**\n\n- [Spec v2](<https://drive.google.com/x>)"
        );
    }
}
