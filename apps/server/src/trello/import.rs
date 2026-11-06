//! Trello import: a member pastes a Trello API key and token, a `trello.scan` job lists the
//! boards they can see, the member picks boards, and a `trello.import` job copies each board
//! into a new Orbit project.
//!
//! State machine (`trello_imports.status`), as for Asana and Linear imports:
//! `scanning` → `ready` → `queued` → `importing` → `completed`; any active state can become
//! `failed` or `cancelled`, and a `ready` import that is not started within 24 hours becomes
//! `expired`. The key and token are stored only as ciphertext under the app key and are wiped
//! (NULL) in every final state (a CHECK constraint enforces it).
//!
//! Jobs carry only the import id. Progress lives in `trello_import_items`, so a restarted job
//! continues where it stopped, and every step converges on a second run:
//! - Phase A, per selected board: create the Orbit project (its id recorded first), map the
//!   open lists to statuses (same name reuses), and list the open cards of open lists, with
//!   their checklists and comments, as items. Archived lists and cards are counted, not copied.
//! - Phase B, per card: create the Orbit task (its id recorded first) silently, without
//!   notifications.
//! - Phase C, per card: uploaded files (downloaded with the credentials, attached, and their URLs
//!   in the description rewritten), then comments (skipping as many as the importer already
//!   wrote).
//!
//! A card that fails is recorded and the import continues; revoked credentials fail the import.
//! What was imported before a failure or cancellation stays.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use orbit_platform::Row;
use orbit_platform::{
    Backend, Database, Id, Job, JobError, JobKind, JobKindRegistrationError, JobQueue, JobStore,
    TimestampMillis, Worker,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;
use utoipa::ToSchema;

use super::client::{TrelloClient, TrelloClientConfig, TrelloError};
use super::model::{CustomField, TrelloCard, TrelloChecklist, TrelloComment};
use super::scan::{MAX_BOARDS, build_tree};
pub use super::scan::{TrelloImportBoard, TrelloImportTree, TrelloImportWorkspace};
use crate::notion::files::sanitize_file_name;
use crate::notion::import::clamp;
use crate::repositories::attachments::AttachmentRepository;
use crate::repositories::membership;
use crate::repositories::tasks::{
    CommentContent, CreateTask, TaskChanges, TaskRepository, TaskUpdate,
};
use crate::secret_box::{decrypt_secret, encrypt_secret};
pub use crate::task_import::JobRun;
use crate::task_import::{
    Attached, Failure, LABEL_COLORS, NewProject, PROJECT_COLORS, attach_once, attachment_url,
    column_statuses, display_instant, ensure_project, ensure_writable, escape_markdown,
    find_or_create_label, job_kind, markdown_links, member_names, mentions_any,
    parse_instant_millis, parse_stored_id, project_key_base, project_statuses, retryable,
    rewrite_links, stopped, with_source_details,
};

/// Job kind names.
pub const SCAN_JOB: &str = "trello.scan";
pub const IMPORT_JOB: &str = "trello.import";
/// Most cards one import creates.
pub const MAX_TASKS: usize = 10_000;
/// A scanned import waits this long for "start" before its credentials are deleted.
pub const READY_TTL: Duration = Duration::from_secs(24 * 60 * 60);
/// An active import that made no progress for this long is failed by the cleanup.
const STALLED_AFTER: Duration = Duration::from_secs(24 * 60 * 60);
const MAX_CREDENTIAL_CHARS: usize = 512;
const MAX_FAILURES_LISTED: i64 = 200;
const MAX_LISTED_IMPORTS: i64 = 20;
const ACTIVE: &str = "('scanning', 'queued', 'importing')";
const OPEN: &str = "('scanning', 'ready', 'queued', 'importing')";
/// Trello's label colors (the `_dark` and `_light` shades map to the base color).
const TRELLO_COLORS: [(&str, &str); 10] = [
    ("green", "#4bce97"),
    ("yellow", "#f5cd47"),
    ("orange", "#fea362"),
    ("red", "#f87168"),
    ("purple", "#9f8fef"),
    ("blue", "#579dff"),
    ("sky", "#6cc3e0"),
    ("lime", "#94c748"),
    ("pink", "#e774bb"),
    ("black", "#8590a2"),
];

/// Both job kinds: [`job_kind`], as for the Asana, Linear and Notion imports.
#[must_use]
pub fn scan_job_kind() -> JobKind {
    job_kind(SCAN_JOB)
}

#[must_use]
pub fn import_job_kind() -> JobKind {
    job_kind(IMPORT_JOB)
}

/// Registers the `trello.scan` and `trello.import` handlers.
pub fn register_jobs(
    worker: Worker,
    service: TrelloImportService,
) -> Result<Worker, JobKindRegistrationError> {
    let scan = service.clone();
    worker
        .with_handler(scan_job_kind(), move |context| {
            let service = scan.clone();
            async move {
                let Some(import_id) = payload_import_id(&context.job.payload) else {
                    return Err(JobError::Permanent(
                        "invalid trello.scan payload".to_owned(),
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
                        "invalid trello.import payload".to_owned(),
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
pub struct TrelloImportSettings {
    /// Trello client settings for scan and import jobs (base URL, request rate).
    pub client: TrelloClientConfig,
    /// The server app key; without it no import can store its credentials.
    pub app_key: Option<[u8; 32]>,
    /// Largest file the import downloads (the upload limit).
    pub max_file_bytes: u64,
}

#[derive(Clone)]
pub struct TrelloImportService {
    inner: std::sync::Arc<Inner>,
}

struct Inner {
    database: Database,
    tasks: TaskRepository,
    attachments: AttachmentRepository,
    jobs: JobQueue,
    settings: TrelloImportSettings,
}

/// Which credential Trello rejected.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrelloCredential {
    Key,
    Token,
}

/// Errors of the import API.
#[derive(Debug, Error)]
pub enum TrelloImportError {
    #[error("import not found")]
    NotFound,
    #[error("the app key is not configured")]
    AppKeyMissing,
    /// Trello (or the format check) rejected the key or token; `reason` is Trello's answer.
    #[error("Trello rejected the {credential:?}: {reason}")]
    CredentialsRejected {
        credential: TrelloCredential,
        reason: String,
    },
    /// The key or token is not letters and digits; Trello was not asked.
    #[error("the {0:?} is malformed")]
    CredentialMalformed(TrelloCredential),
    #[error("Trello is unavailable")]
    TrelloUnavailable,
    #[error("another import is in progress")]
    InProgress,
    #[error("the import is not in a state that allows this")]
    StateConflict,
    #[error("invalid input: {field}")]
    Invalid { field: &'static str },
    #[error("import storage is unavailable")]
    Unavailable(String),
}

impl From<sqlx::Error> for TrelloImportError {
    fn from(error: sqlx::Error) -> Self {
        Self::Unavailable(error.to_string())
    }
}

/// `trello_imports.status`.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum TrelloImportStatus {
    /// The scan job is listing boards.
    Scanning,
    /// The tree is ready; waiting for `start` (the credentials are deleted after 24 hours).
    Ready,
    /// Started; waiting for a job slot.
    Queued,
    Importing,
    Completed,
    Failed,
    Cancelled,
    /// Not started within 24 hours; the credentials were deleted.
    Expired,
}

impl TrelloImportStatus {
    fn parse(value: &str) -> Result<Self, TrelloImportError> {
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
                return Err(TrelloImportError::Unavailable(
                    "unknown import status".to_owned(),
                ));
            }
        })
    }
}

#[derive(Clone, Copy, Debug, Serialize, ToSchema)]
pub struct TrelloImportProgress {
    /// Cards found so far; grows while boards are listed.
    pub total: u64,
    /// Cards imported with their files and comments.
    pub done: u64,
    /// Boards and cards that could not be imported.
    pub failed: u64,
}

/// A board or card the import could not create or fill.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct TrelloImportFailure {
    pub trello_id: String,
    /// `board` or `card`.
    pub kind: String,
    pub title: String,
    pub error: String,
}

/// What the import carried over besides the cards themselves, and what it left out.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct TrelloImportReport {
    pub comments_imported: u32,
    /// Files uploaded to Trello, downloaded and attached to the tasks.
    pub files_imported: u32,
    /// Uploaded files that could not be downloaded or attached (too large, gone).
    pub missing_files: u32,
    /// Uploaded files Trello refused to hand out (401 or 403; some Enterprise workspaces block
    /// downloads through the API).
    pub refused_files: u32,
    /// Link attachments, added to the descriptions.
    pub links_added: u32,
    /// Checklists written into the descriptions.
    pub checklists_added: u32,
    /// Card members with no single Orbit member of the same full name (named in the
    /// description instead).
    pub unmatched_members: u32,
    /// Archived lists, left out with their cards.
    pub archived_lists_skipped: u32,
    /// Archived cards, and open cards on archived lists, left out.
    pub archived_cards_skipped: u32,
    /// The first 200 failures.
    pub failures: Vec<TrelloImportFailure>,
}

/// The report's counts as `trello_imports.report_json` keeps them.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
struct StoredReport {
    comments_imported: u32,
    files_imported: u32,
    missing_files: u32,
    refused_files: u32,
    links_added: u32,
    checklists_added: u32,
    unmatched_members: u32,
    archived_lists_skipped: u32,
    archived_cards_skipped: u32,
}

/// A Trello import. Never contains the key or token.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct TrelloImport {
    #[schema(value_type = String)]
    pub id: Id,
    #[schema(value_type = String)]
    pub workspace_id: Id,
    pub status: TrelloImportStatus,
    /// The Trello member the token belongs to.
    #[schema(required = true)]
    pub trello_user_name: Option<String>,
    pub progress: TrelloImportProgress,
    /// Why the import failed or expired.
    #[schema(required = true)]
    pub error: Option<String>,
    /// The scan result, once ready. Only in `GET …/imports/trello/{id}?include=tree`; `null`
    /// everywhere else, so polling stays light.
    #[schema(required = true)]
    pub tree: Option<TrelloImportTree>,
    /// Set once the import was started.
    #[schema(required = true)]
    pub report: Option<TrelloImportReport>,
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
    board_ids: Vec<String>,
}

/// A board item's data once phase A mapped it.
#[derive(Default, Deserialize, Serialize)]
struct StoredBoard {
    /// `(list id, status id)` of the open lists.
    #[serde(default)]
    lists: Vec<(String, String)>,
    /// `(label id, name, color)`; an unnamed label is named after its color.
    #[serde(default)]
    labels: Vec<(String, String, Option<String>)>,
    /// `(member id, full name)`.
    #[serde(default)]
    members: Vec<(String, String)>,
    #[serde(default)]
    fields: Vec<StoredField>,
}

/// A custom field definition: list options as `(option id, text)`.
#[derive(Clone, Default, Deserialize, Serialize)]
struct StoredField {
    id: String,
    name: String,
    kind: String,
    options: Vec<(String, String)>,
}

#[derive(Clone, Debug)]
struct Item {
    trello_id: String,
    board_id: String,
    title: String,
    data_json: Option<String>,
    orbit_id: Option<Id>,
    state: String,
}

impl From<TrelloError> for Failure {
    fn from(error: TrelloError) -> Self {
        match &error {
            TrelloError::Unauthorized { .. } => Self::Fatal(
                "The Trello token was revoked or expired, or the API key is no longer valid."
                    .to_owned(),
            ),
            TrelloError::Forbidden { .. } => {
                Self::Fatal("The Trello token lost access to a selected board.".to_owned())
            }
            TrelloError::RateLimited { .. } | TrelloError::Http(_) => {
                Self::Transient(error.to_string())
            }
            TrelloError::Api { status, .. } if *status >= 500 => Self::Transient(error.to_string()),
            _ => Self::Item(error.to_string()),
        }
    }
}

/// The Orbit side of one imported board, built from its item.
struct BoardContext {
    project_id: Id,
    /// List id → `(status id, status category)`.
    lists: HashMap<String, (Id, String)>,
    /// Label id → `(name, Trello color)`.
    trello_labels: HashMap<String, (String, Option<String>)>,
    /// Lowercase name → Orbit label id.
    labels: HashMap<String, Id>,
    /// Member id → full name.
    members: HashMap<String, String>,
    fields: HashMap<String, StoredField>,
}

/// Shared state of one import run.
struct Run<'a> {
    import_id: Id,
    workspace_id: Id,
    user_id: Id,
    client: &'a TrelloClient,
    job: &'a JobRun,
    request_id: String,
    report: StoredReport,
    /// Lowercase full name → the members with that name.
    members: HashMap<String, Vec<Id>>,
    boards: HashMap<String, BoardContext>,
}

impl Run<'_> {
    /// The Orbit member with exactly this full name (case and surrounding spaces ignored).
    fn member_named(&self, name: &str) -> Option<Id> {
        match self.members.get(&name.trim().to_lowercase())?.as_slice() {
            [only] => Some(*only),
            _ => None,
        }
    }
}

impl TrelloImportService {
    #[must_use]
    pub fn new(
        database: Database,
        attachments: AttachmentRepository,
        settings: TrelloImportSettings,
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

    /// Validates `key` and `token` with Trello (`members/me`), stores both encrypted and queues
    /// the scan. A member has at most one active Trello import; their unstarted scans in this
    /// workspace are cancelled (their credentials deleted).
    pub async fn create(
        &self,
        workspace_id: Id,
        user_id: Id,
        key: &str,
        token: &str,
    ) -> Result<TrelloImport, TrelloImportError> {
        self.require_member(workspace_id, user_id).await?;
        let app_key = self
            .inner
            .settings
            .app_key
            .ok_or(TrelloImportError::AppKeyMissing)?;
        let (key, token) = (key.trim(), token.trim());
        for (value, credential) in [
            (key, TrelloCredential::Key),
            (token, TrelloCredential::Token),
        ] {
            if value.is_empty()
                || value.chars().count() > MAX_CREDENTIAL_CHARS
                || !value.chars().all(|c| c.is_ascii_alphanumeric())
            {
                return Err(TrelloImportError::CredentialMalformed(credential));
            }
        }
        let now = TimestampMillis::now();
        self.expire_stale(now).await?;
        let validation = TrelloClientConfig {
            max_attempts: 2,
            request_timeout: Duration::from_secs(20),
            max_retry_after: Duration::from_secs(5),
            ..self.inner.settings.client.clone()
        };
        let client = TrelloClient::new(key, token, validation)
            .map_err(|_| TrelloImportError::CredentialMalformed(TrelloCredential::Key))?;
        let member = match client.me().await {
            Ok(member) => member,
            Err(TrelloError::Unauthorized { message }) => {
                return Err(TrelloImportError::CredentialsRejected {
                    credential: rejected_credential(&message),
                    reason: if message.is_empty() {
                        "401 Unauthorized".to_owned()
                    } else {
                        message
                    },
                });
            }
            Err(error) => {
                tracing::warn!(error = %error, "Trello credential validation failed");
                return Err(TrelloImportError::TrelloUnavailable);
            }
        };
        let name = member.display().map(|name| clamp(&name, 200, 800));
        let encrypt = |secret: &str| {
            encrypt_secret(&app_key, secret).map_err(|()| {
                TrelloImportError::Unavailable("credential encryption failed".to_owned())
            })
        };
        let (key_ciphertext, token_ciphertext) = (encrypt(key)?, encrypt(token)?);

        let id = Id::new_v7();
        let mut tx = self.database().immediate_transaction().await?;
        let active: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) FROM trello_imports WHERE user_id = $1 AND status IN {ACTIVE}"
        )))
        .bind(user_id.to_string())
        .fetch_one(&mut *tx)
        .await?;
        if active > 0 {
            return Err(TrelloImportError::InProgress);
        }
        sqlx::query(
            "UPDATE trello_imports SET status = 'cancelled', key_ciphertext = NULL, \
             token_ciphertext = NULL, updated_at = $1 \
             WHERE user_id = $2 AND workspace_id = $3 AND status = 'ready'",
        )
        .bind(now.as_millis())
        .bind(user_id.to_string())
        .bind(workspace_id.to_string())
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO trello_imports (id, workspace_id, user_id, status, key_ciphertext, \
             token_ciphertext, trello_user_name, created_at, updated_at) \
             VALUES ($1, $2, $3, 'scanning', $4, $5, $6, $7, $8)",
        )
        .bind(id.to_string())
        .bind(workspace_id.to_string())
        .bind(user_id.to_string())
        .bind(key_ciphertext)
        .bind(token_ciphertext)
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
    ) -> Result<Vec<TrelloImport>, TrelloImportError> {
        self.require_member(workspace_id, user_id).await?;
        self.expire_stale(TimestampMillis::now()).await?;
        let ids: Vec<String> = sqlx::query_scalar(
            "SELECT id FROM trello_imports WHERE workspace_id = $1 AND user_id = $2 \
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
                .map_err(|_| TrelloImportError::Unavailable("invalid import id".to_owned()))?;
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
    ) -> Result<TrelloImport, TrelloImportError> {
        self.require_member(workspace_id, user_id).await?;
        self.expire_stale(TimestampMillis::now()).await?;
        self.load_view(workspace_id, user_id, import_id, with_tree)
            .await
    }

    /// Plans the import of the selected boards and queues the import job. Every id must be a
    /// board of the scan tree.
    pub async fn start(
        &self,
        workspace_id: Id,
        user_id: Id,
        import_id: Id,
        board_ids: Vec<String>,
    ) -> Result<TrelloImport, TrelloImportError> {
        self.require_member(workspace_id, user_id).await?;
        self.expire_stale(TimestampMillis::now()).await?;
        let row = sqlx::query(
            "SELECT status, tree_json FROM trello_imports WHERE id = $1 AND workspace_id = $2 AND user_id = $3",
        )
        .bind(import_id.to_string())
        .bind(workspace_id.to_string())
        .bind(user_id.to_string())
        .fetch_optional(self.database().pool())
        .await?
        .ok_or(TrelloImportError::NotFound)?;
        if row.get::<String, _>("status") != "ready" {
            return Err(TrelloImportError::StateConflict);
        }
        let tree: TrelloImportTree = serde_json::from_str(
            &row.get::<Option<String>, _>("tree_json")
                .unwrap_or_default(),
        )
        .map_err(|_| TrelloImportError::Unavailable("invalid stored tree".to_owned()))?;
        let mut seen = HashSet::new();
        let mut boards = Vec::new();
        for id in &board_ids {
            let board = tree
                .board(id)
                .ok_or(TrelloImportError::Invalid { field: "board_ids" })?;
            if seen.insert(id.clone()) {
                boards.push(board.clone());
            }
        }
        if boards.is_empty() || boards.len() > MAX_BOARDS {
            return Err(TrelloImportError::Invalid { field: "board_ids" });
        }
        let selection = StoredSelection {
            board_ids: boards.iter().map(|board| board.id.clone()).collect(),
        };

        let now = TimestampMillis::now();
        let mut tx = self.database().immediate_transaction().await?;
        let active: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) FROM trello_imports WHERE user_id = $1 AND id <> $2 AND status IN {ACTIVE}"
        )))
        .bind(user_id.to_string())
        .bind(import_id.to_string())
        .fetch_one(&mut *tx)
        .await?;
        if active > 0 {
            return Err(TrelloImportError::InProgress);
        }
        let updated = sqlx::query(
            "UPDATE trello_imports SET status = 'queued', selection_json = $1, total = 0, done = 0, \
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
            return Err(TrelloImportError::StateConflict);
        }
        for (position, board) in boards.iter().enumerate() {
            sqlx::query(
                "INSERT INTO trello_import_items (import_id, kind, trello_id, board_id, position, title) \
                 VALUES ($1, 'board', $2, $3, $4, $5)",
            )
            .bind(import_id.to_string())
            .bind(&board.id)
            .bind(&board.id)
            .bind(position as i64)
            .bind(&board.name)
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

    /// Cancels an open import and deletes its credentials. A running job stops before its next
    /// item; what it already imported stays.
    pub async fn cancel(
        &self,
        workspace_id: Id,
        user_id: Id,
        import_id: Id,
    ) -> Result<TrelloImport, TrelloImportError> {
        self.require_member(workspace_id, user_id).await?;
        let updated = sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE trello_imports SET status = 'cancelled', key_ciphertext = NULL, \
             token_ciphertext = NULL, updated_at = $1 \
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
            return Err(TrelloImportError::StateConflict);
        }
        self.load_view(workspace_id, user_id, import_id, false)
            .await
    }

    /// Expires scanned imports that were not started within 24 hours and fails active imports
    /// that made no progress for 24 hours; both lose their credentials. Returns the rows
    /// changed.
    pub async fn expire_stale(&self, now: TimestampMillis) -> Result<u64, TrelloImportError> {
        let ready_cutoff = now.as_millis() - READY_TTL.as_millis() as i64;
        let stalled_cutoff = now.as_millis() - STALLED_AFTER.as_millis() as i64;
        let expired = sqlx::query(
            "UPDATE trello_imports SET status = 'expired', key_ciphertext = NULL, \
             token_ciphertext = NULL, error = 'The import was not started within 24 hours.', \
             updated_at = $1 WHERE status = 'ready' AND updated_at < $2",
        )
        .bind(now.as_millis())
        .bind(ready_cutoff)
        .execute(self.database().pool())
        .await?
        .rows_affected();
        let stalled = sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE trello_imports SET status = 'failed', key_ciphertext = NULL, \
             token_ciphertext = NULL, error = 'The import stopped making progress.', \
             updated_at = $1 WHERE status IN {ACTIVE} AND updated_at < $2"
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
            tracing::warn!(import_id = %import_id, error = %error, cleanup = %cleanup, "Trello import could not be finalized");
        }
        result
    }

    /// Fails active imports whose job is dead and that have no live job left.
    async fn fail_imports_with_dead_jobs(&self) -> Result<u64, TrelloImportError> {
        let import_id_of_job = match self.database().backend() {
            Backend::Sqlite => "json_extract(jobs.payload_json, '$.import_id')",
            Backend::Postgres => "(jobs.payload_json::jsonb ->> 'import_id')",
        };
        let failed = sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE trello_imports SET status = 'failed', key_ciphertext = NULL, \
             token_ciphertext = NULL, error = 'The import stopped after repeated errors.', \
             updated_at = $1 \
             WHERE status IN {ACTIVE} \
             AND EXISTS (SELECT 1 FROM jobs WHERE jobs.kind IN ('{SCAN_JOB}', '{IMPORT_JOB}') \
                 AND jobs.state = 'dead' AND {import_id_of_job} = trello_imports.id) \
             AND NOT EXISTS (SELECT 1 FROM jobs WHERE jobs.kind IN ('{SCAN_JOB}', '{IMPORT_JOB}') \
                 AND jobs.state IN ('queued', 'running') AND {import_id_of_job} = trello_imports.id)"
        )))
        .bind(TimestampMillis::now().as_millis())
        .execute(self.database().pool())
        .await?
        .rows_affected();
        Ok(failed)
    }

    /// The `trello.scan` job: lists the boards and stores the tree.
    pub async fn run_scan(&self, import_id: Id, run: &JobRun) -> Result<(), JobError> {
        let Some(row) = sqlx::query(
            "SELECT status, key_ciphertext, token_ciphertext FROM trello_imports WHERE id = $1",
        )
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
        let client = match self.client_for(row.get("key_ciphertext"), row.get("token_ciphertext")) {
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
                    "UPDATE trello_imports SET status = 'ready', tree_json = $1, updated_at = $2 \
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

    /// The `trello.import` job.
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
                tracing::warn!(import_id = %import_id, error = %message, "Trello import will retry");
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
            "SELECT workspace_id, user_id, status, key_ciphertext, token_ciphertext, report_json \
             FROM trello_imports WHERE id = $1",
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
            .client_for(row.get("key_ciphertext"), row.get("token_ciphertext"))
            .map_err(Failure::Fatal)?;
        sqlx::query(
            "UPDATE trello_imports SET status = 'importing', updated_at = $1 WHERE id = $2 AND status = 'queued'",
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
            request_id: format!("trello-import-{import_id}"),
            report,
            members: member_names(self.database(), workspace_id).await?,
            boards: HashMap::new(),
        };

        // Phase A: projects, their statuses, and the card lists.
        let boards: Vec<Item> = self.load_items(import_id, "board").await?;
        for (index, board) in boards.iter().enumerate() {
            if board.state == "filled" || board.state == "failed" {
                continue;
            }
            if self.should_stop(&run).await? {
                return stopped(job);
            }
            match self.prepare_board(&mut run, board, index).await {
                Ok(()) => {}
                Err(Failure::Item(message)) => {
                    self.fail_item(import_id, "board", &board.trello_id, &message)
                        .await?;
                }
                Err(other) => return Err(other),
            }
        }
        for board in self.load_items(import_id, "board").await? {
            if board.state == "filled" {
                let context = self.board_context(&run, &board).await?;
                run.boards.insert(board.trello_id.clone(), context);
            }
        }

        // Phase B: create every card.
        let mut cards = self.load_items(import_id, "card").await?;
        for card in &mut cards {
            if card.state != "pending" {
                continue;
            }
            if self.should_stop(&run).await? {
                self.save_report(&run).await?;
                return stopped(job);
            }
            match self.create_task(&mut run, card).await {
                Ok(()) => card.state = "created".to_owned(),
                Err(Failure::Item(message)) => {
                    self.fail_item(import_id, "card", &card.trello_id, &message)
                        .await?;
                    card.state = "failed".to_owned();
                }
                Err(other) => {
                    self.save_report(&run).await?;
                    return Err(other);
                }
            }
        }
        self.save_report(&run).await?;

        // Phase C: files and comments.
        for card in &cards {
            if card.state != "created" {
                continue;
            }
            if self.should_stop(&run).await? {
                self.save_report(&run).await?;
                return stopped(job);
            }
            match self.fill_task(&mut run, card).await {
                Ok(()) => {
                    sqlx::query(
                        "UPDATE trello_import_items SET state = 'filled', data_json = NULL, error = NULL \
                         WHERE import_id = $1 AND kind = 'card' AND trello_id = $2",
                    )
                    .bind(import_id.to_string())
                    .bind(&card.trello_id)
                    .execute(self.database().pool())
                    .await?;
                    sqlx::query(
                        "UPDATE trello_imports SET done = done + 1, updated_at = $1 WHERE id = $2",
                    )
                    .bind(TimestampMillis::now().as_millis())
                    .bind(import_id.to_string())
                    .execute(self.database().pool())
                    .await?;
                    self.save_report(&run).await?;
                }
                Err(Failure::Item(message)) => {
                    self.fail_item(import_id, "card", &card.trello_id, &message)
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
            "UPDATE trello_imports SET status = 'completed', key_ciphertext = NULL, \
             token_ciphertext = NULL, updated_at = $1 WHERE id = $2 AND status = 'importing'",
        )
        .bind(TimestampMillis::now().as_millis())
        .bind(import_id.to_string())
        .execute(self.database().pool())
        .await?;
        Ok(())
    }

    /// Phase A for one board: the Orbit project, a status per open list, the board's labels,
    /// members and custom fields, and the card items.
    async fn prepare_board(
        &self,
        run: &mut Run<'_>,
        item: &Item,
        index: usize,
    ) -> Result<(), Failure> {
        let project_id = match item.orbit_id {
            Some(id) => id,
            None => {
                let id = Id::new_v7();
                self.record_orbit_id(run.import_id, "board", &item.trello_id, id)
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
                fallback_name: "Trello board",
                key_base: project_key_base(&item.title, "TRL"),
                color: PROJECT_COLORS[index % PROJECT_COLORS.len()],
                request_id: &run.request_id,
            },
        )
        .await?;
        sqlx::query(
            "UPDATE trello_import_items SET state = 'created' WHERE import_id = $1 AND kind = 'board' \
             AND trello_id = $2 AND state = 'pending'",
        )
        .bind(run.import_id.to_string())
        .bind(&item.trello_id)
        .execute(self.database().pool())
        .await?;

        let board_id = item.trello_id.as_str();
        let mut lists = run.client.lists(board_id).await?;
        lists.sort_by(|a, b| a.pos.total_cmp(&b.pos));
        let (open, archived): (Vec<_>, Vec<_>) = lists.into_iter().partition(|list| !list.closed);
        let columns: Vec<(String, String)> = open
            .iter()
            .map(|list| {
                let name = list
                    .name
                    .as_deref()
                    .map(|name| clamp(name.trim(), 100, 100))
                    .filter(|name| !name.is_empty())
                    .unwrap_or_else(|| "Untitled list".to_owned());
                (list.id.clone(), name)
            })
            .collect();
        let mapped = column_statuses(
            self.database(),
            &self.inner.tasks,
            run.workspace_id,
            project_id,
            run.user_id,
            &run.request_id,
            &columns,
        )
        .await?;
        let labels = run
            .client
            .labels(board_id)
            .await?
            .into_iter()
            .filter_map(|label| {
                let name = label
                    .name
                    .as_deref()
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                    .map(str::to_owned)
                    .or_else(|| label.color.clone())?;
                Some((label.id, clamp(&name, 100, 100), label.color))
            })
            .collect();
        let members = run
            .client
            .members(board_id)
            .await?
            .into_iter()
            .filter_map(|member| Some((member.id.clone(), member.display()?)))
            .collect();
        let fields = match run.client.custom_fields(board_id).await {
            Ok(fields) => fields.into_iter().map(stored_field).collect(),
            // Boards without the Power-Up may refuse the list.
            Err(TrelloError::Forbidden { .. } | TrelloError::NotFound { .. }) => Vec::new(),
            Err(error) => return Err(Failure::from(error)),
        };
        let stored = StoredBoard {
            lists: mapped,
            labels,
            members,
            fields,
        };
        sqlx::query(
            "UPDATE trello_import_items SET data_json = $1 WHERE import_id = $2 AND kind = 'board' AND trello_id = $3",
        )
        .bind(serde_json::to_string(&stored).unwrap_or_default())
        .bind(run.import_id.to_string())
        .bind(&item.trello_id)
        .execute(self.database().pool())
        .await?;

        let skipped_cards = self.list_cards(run, board_id, &open).await?;
        run.report.archived_lists_skipped += u32::try_from(archived.len()).unwrap_or(u32::MAX);
        run.report.archived_cards_skipped += skipped_cards;
        // The board is filled and its counts reported together, so a rerun counts them once.
        let mut tx = self.database().immediate_transaction().await?;
        sqlx::query(
            "UPDATE trello_import_items SET state = 'filled' WHERE import_id = $1 AND kind = 'board' AND trello_id = $2",
        )
        .bind(run.import_id.to_string())
        .bind(&item.trello_id)
        .execute(&mut *tx)
        .await?;
        sqlx::query("UPDATE trello_imports SET report_json = $1, updated_at = $2 WHERE id = $3")
            .bind(serde_json::to_string(&run.report).unwrap_or_default())
            .bind(TimestampMillis::now().as_millis())
            .bind(run.import_id.to_string())
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Lists the open cards on the board's open lists as pending items, in list order and then
    /// card order, each with its checklists and comments. An item already listed keeps its
    /// place. Returns how many cards were left out as archived.
    async fn list_cards(
        &self,
        run: &mut Run<'_>,
        board_id: &str,
        open_lists: &[super::model::TrelloList],
    ) -> Result<u32, Failure> {
        let listed: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM trello_import_items WHERE import_id = $1 AND kind = 'card'",
        )
        .bind(run.import_id.to_string())
        .fetch_one(self.database().pool())
        .await?;
        let room = MAX_TASKS.saturating_sub(usize::try_from(listed).unwrap_or(MAX_TASKS));
        let list_order: HashMap<&str, usize> = open_lists
            .iter()
            .enumerate()
            .map(|(position, list)| (list.id.as_str(), position))
            .collect();
        let all_open = run.client.open_cards(board_id).await?;
        let open_count = all_open.len();
        let mut cards: Vec<TrelloCard> = all_open
            .into_iter()
            .filter(|card| !card.closed && list_order.contains_key(card.id_list.as_str()))
            .collect();
        let on_archived_lists = open_count - cards.len();
        if cards.len() > room {
            return Err(Failure::Fatal(format!(
                "The selected boards have more than {MAX_TASKS} open cards. Import fewer boards at a time."
            )));
        }
        let archived = run.client.archived_card_count(board_id).await? + on_archived_lists;
        if self.should_stop(run).await? {
            return stopped(run.job).map(|()| 0);
        }
        cards.sort_by(|a, b| {
            list_order[a.id_list.as_str()]
                .cmp(&list_order[b.id_list.as_str()])
                .then(a.pos.total_cmp(&b.pos))
        });
        let mut checklists: HashMap<String, Vec<TrelloChecklist>> = HashMap::new();
        for checklist in run.client.checklists(board_id).await? {
            checklists
                .entry(checklist.id_card.clone())
                .or_default()
                .push(checklist);
        }
        let mut comments: HashMap<String, Vec<TrelloComment>> = HashMap::new();
        // Newest first from Trello; reversed below.
        for action in run.client.comments(board_id).await? {
            let Some(card) = action.data.card.map(|card| card.id) else {
                continue;
            };
            comments.entry(card).or_default().push(TrelloComment {
                id: action.id,
                text: action.data.text.unwrap_or_default(),
                date: action.date,
                author: action.member_creator.and_then(|member| member.display()),
            });
        }
        for card in &mut cards {
            let mut lists = checklists.remove(&card.id).unwrap_or_default();
            lists.sort_by(|a, b| a.pos.total_cmp(&b.pos));
            for checklist in &mut lists {
                checklist
                    .check_items
                    .sort_by(|a, b| a.pos.total_cmp(&b.pos));
            }
            card.checklists = lists;
            let mut thread = comments.remove(&card.id).unwrap_or_default();
            thread.reverse();
            card.comments = thread;
        }

        let mut tx = self.database().immediate_transaction().await?;
        let mut inserted = 0;
        for card in &cards {
            inserted += sqlx::query(
                "INSERT INTO trello_import_items (import_id, kind, trello_id, board_id, position, \
                 title, data_json) VALUES ($1, 'card', $2, $3, \
                 (SELECT COALESCE(MAX(position) + 1, 0) FROM trello_import_items WHERE import_id = $4), $5, $6) \
                 ON CONFLICT (import_id, kind, trello_id) DO NOTHING",
            )
            .bind(run.import_id.to_string())
            .bind(&card.id)
            .bind(board_id)
            .bind(run.import_id.to_string())
            .bind(clamp(
                card.name.as_deref().unwrap_or_default().trim(),
                500,
                2_000,
            ))
            .bind(serde_json::to_string(card).unwrap_or_default())
            .execute(&mut *tx)
            .await?
            .rows_affected();
        }
        sqlx::query("UPDATE trello_imports SET total = total + $1, updated_at = $2 WHERE id = $3")
            .bind(inserted as i64)
            .bind(TimestampMillis::now().as_millis())
            .bind(run.import_id.to_string())
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(u32::try_from(archived).unwrap_or(u32::MAX))
    }

    /// Statuses, labels, members and fields of a board whose phase A finished.
    async fn board_context(&self, run: &Run<'_>, item: &Item) -> Result<BoardContext, Failure> {
        let project_id = item
            .orbit_id
            .ok_or_else(|| Failure::Fatal("Stored import data is invalid.".to_owned()))?;
        let stored: StoredBoard = item
            .data_json
            .as_deref()
            .and_then(|json| serde_json::from_str(json).ok())
            .unwrap_or_default();
        let categories: HashMap<Id, String> = project_statuses(self.database(), project_id)
            .await?
            .into_iter()
            .filter(|(_, _, category)| category != "duplicate")
            .map(|(id, _, category)| (id, category))
            .collect();
        let lists = stored
            .lists
            .iter()
            .filter_map(|(list, id)| {
                let id: Id = id.parse().ok()?;
                Some((list.clone(), (id, categories.get(&id)?.clone())))
            })
            .collect();
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
        Ok(BoardContext {
            project_id,
            lists,
            trello_labels: stored
                .labels
                .into_iter()
                .map(|(id, name, color)| (id, (name, color)))
                .collect(),
            labels,
            members: stored.members.into_iter().collect(),
            fields: stored
                .fields
                .into_iter()
                .map(|field| (field.id.clone(), field))
                .collect(),
        })
    }

    /// Phase B for one card: its Orbit task, created silently under the recorded id.
    async fn create_task(&self, run: &mut Run<'_>, item: &mut Item) -> Result<(), Failure> {
        if !run.boards.contains_key(&item.board_id) {
            return Err(Failure::Item("Its board could not be imported.".to_owned()));
        }
        let card: TrelloCard = item
            .data_json
            .as_deref()
            .and_then(|json| serde_json::from_str(json).ok())
            .ok_or_else(|| Failure::Item("The stored Trello card is invalid.".to_owned()))?;
        let task_id = match item.orbit_id {
            Some(id) => id,
            None => {
                let id = Id::new_v7();
                self.record_orbit_id(run.import_id, "card", &item.trello_id, id)
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
            let input = self.task_input(run, &item.board_id, &card).await?;
            let completed_at = card
                .date_last_activity
                .as_deref()
                .and_then(parse_instant_millis)
                .map(TimestampMillis::from_millis);
            self.inner
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
                .await?;
        }
        sqlx::query(
            "UPDATE trello_import_items SET state = 'created' WHERE import_id = $1 AND kind = 'card' AND trello_id = $2",
        )
        .bind(run.import_id.to_string())
        .bind(&item.trello_id)
        .execute(self.database().pool())
        .await?;
        self.touch(run.import_id).await?;
        Ok(())
    }

    /// The Orbit task for a Trello card: status from its list, labels, assignees by full name,
    /// start and due dates, and a description with checklists and what Orbit has no field for.
    async fn task_input(
        &self,
        run: &mut Run<'_>,
        board_id: &str,
        card: &TrelloCard,
    ) -> Result<CreateTask, Failure> {
        let board = run
            .boards
            .get(board_id)
            .ok_or_else(|| Failure::Item("Its board could not be imported.".to_owned()))?;
        let names: Vec<(String, Option<String>)> = card
            .id_labels
            .iter()
            .filter_map(|id| board.trello_labels.get(id).cloned())
            .collect();
        let (status_id, _) = board
            .lists
            .get(&card.id_list)
            .cloned()
            .ok_or_else(|| Failure::Item("Its list could not be imported.".to_owned()))?;
        let project_id = board.project_id;
        let mut labels = Vec::new();
        for (name, color) in names {
            let label = self
                .ensure_label(run, board_id, &name, color.as_deref())
                .await?;
            if !labels.contains(&label) {
                labels.push(label);
            }
        }
        let text = card_text(
            run,
            board_id,
            card,
            card.desc.as_deref().unwrap_or_default(),
        );
        run.report.unmatched_members += text.unmatched_members;
        run.report.links_added += text.links;
        run.report.checklists_added += u32::try_from(card.checklists.len()).unwrap_or(u32::MAX);
        let title = clamp(card.name.as_deref().unwrap_or_default().trim(), 500, 500);
        Ok(CreateTask {
            project_id,
            status_id,
            title: if title.is_empty() {
                "Untitled card".to_owned()
            } else {
                title
            },
            description: text.markdown,
            description_json: None,
            source_url: card
                .short_url
                .clone()
                .or_else(|| card.url.clone())
                .filter(|url| url.starts_with("https://") && url.len() <= 2_048),
            priority: "none".to_owned(),
            position: None,
            assignee_ids: text.assignees,
            label_ids: labels,
            due_start_at: card
                .start
                .as_deref()
                .and_then(parse_instant_millis)
                .map(TimestampMillis::from_millis),
            due_at: card
                .due
                .as_deref()
                .and_then(parse_instant_millis)
                .map(TimestampMillis::from_millis),
            parent_task_id: None,
        })
    }

    /// The project's label with this name, created on first use in the Trello color.
    async fn ensure_label(
        &self,
        run: &mut Run<'_>,
        board_id: &str,
        name: &str,
        color: Option<&str>,
    ) -> Result<Id, Failure> {
        let key = name.to_lowercase();
        let board = run
            .boards
            .get(board_id)
            .ok_or_else(|| Failure::Item("Its board could not be imported.".to_owned()))?;
        if let Some(id) = board.labels.get(&key) {
            return Ok(*id);
        }
        let color = color
            .and_then(label_color)
            .unwrap_or(LABEL_COLORS[board.labels.len() % LABEL_COLORS.len()]);
        let id = find_or_create_label(
            self.database(),
            &self.inner.tasks,
            run.workspace_id,
            board.project_id,
            run.user_id,
            name,
            color,
            &run.request_id,
        )
        .await?;
        if let Some(board) = run.boards.get_mut(board_id) {
            board.labels.insert(key, id);
        }
        Ok(id)
    }

    /// Phase C for one card: uploaded files, then comments. Each step skips what an earlier
    /// attempt already did.
    async fn fill_task(&self, run: &mut Run<'_>, item: &Item) -> Result<(), Failure> {
        let task_id = item
            .orbit_id
            .ok_or_else(|| Failure::Item("The task was not created.".to_owned()))?;
        let card: TrelloCard = item
            .data_json
            .as_deref()
            .and_then(|json| serde_json::from_str(json).ok())
            .unwrap_or_default();

        // Uploaded attachments, then uploads linked from the description and the comments,
        // each attached once.
        let body = card.desc.as_deref().unwrap_or_default();
        let mut uploads: Vec<(String, Option<String>, Option<u64>)> = Vec::new();
        for attachment in &card.attachments {
            if let Some(url) = attachment
                .url
                .clone()
                .filter(|url| attachment.is_upload && run.client.is_download_url(url))
                && !uploads.iter().any(|(seen, _, _)| *seen == url)
            {
                uploads.push((url, attachment.name.clone(), attachment.bytes));
            }
        }
        for markdown in std::iter::once(body).chain(card.comments.iter().map(|c| c.text.as_str())) {
            for (url, label) in markdown_links(markdown) {
                if run.client.is_download_url(&url)
                    && !uploads.iter().any(|(seen, _, _)| *seen == url)
                {
                    uploads.push((url, label, None));
                }
            }
        }
        let files = self.attach_uploads(run, task_id, &uploads).await?;

        if mentions_any(body, &files) {
            let markdown =
                card_text(run, &item.board_id, &card, &rewrite_links(body, &files)).markdown;
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

        // Comments, oldest first. The import writes them in that order, so the importer's
        // comments already on the task are the first ones.
        let written: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM task_comments WHERE task_id = $1 AND author_id = $2",
        )
        .bind(task_id.to_string())
        .bind(run.user_id.to_string())
        .fetch_one(self.database().pool())
        .await?;
        for comment in card
            .comments
            .iter()
            .skip(usize::try_from(written).unwrap_or_default())
        {
            let author = comment.author.as_deref().unwrap_or("Trello member");
            let mut header = format!("**{}**", escape_markdown(author));
            if let Some(date) = comment.date.as_deref().map(display_instant) {
                header.push_str(&format!(" · {date}"));
            }
            let text = rewrite_links(comment.text.trim(), &files);
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

    /// Downloads and attaches the uploads `(url, name, size)` (a file already attached, by
    /// content, is reused) and returns each Trello URL with the Orbit URL that replaces it.
    async fn attach_uploads(
        &self,
        run: &mut Run<'_>,
        task_id: Id,
        uploads: &[(String, Option<String>, Option<u64>)],
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
        for (url, label, size) in uploads {
            if size.is_some_and(|size| size > max_bytes) {
                run.report.missing_files += 1;
                continue;
            }
            let file = match run.client.download_file(url, max_bytes).await {
                Ok(file) => file,
                Err(TrelloError::FileRefused { .. }) => {
                    run.report.refused_files += 1;
                    continue;
                }
                Err(error @ (TrelloError::RateLimited { .. } | TrelloError::Http(_))) => {
                    return Err(Failure::from(error));
                }
                Err(error) => {
                    tracing::debug!(error = %error, "Trello file download skipped");
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

    /// Whether to stop before the next item; a workspace that became read-only fails the
    /// import.
    async fn should_stop(&self, run: &Run<'_>) -> Result<bool, Failure> {
        if run.job.cancel.is_cancelled() {
            return Ok(true);
        }
        ensure_writable(self.database(), run.workspace_id).await?;
        let status: Option<String> =
            sqlx::query_scalar("SELECT status FROM trello_imports WHERE id = $1")
                .bind(run.import_id.to_string())
                .fetch_optional(self.database().pool())
                .await?;
        Ok(status.as_deref() != Some("importing"))
    }

    async fn record_orbit_id(
        &self,
        import_id: Id,
        kind: &str,
        trello_id: &str,
        orbit_id: Id,
    ) -> Result<(), Failure> {
        sqlx::query(
            "UPDATE trello_import_items SET orbit_id = $1 WHERE import_id = $2 AND kind = $3 AND trello_id = $4",
        )
        .bind(orbit_id.to_string())
        .bind(import_id.to_string())
        .bind(kind)
        .bind(trello_id)
        .execute(self.database().pool())
        .await?;
        Ok(())
    }

    async fn fail_item(
        &self,
        import_id: Id,
        kind: &str,
        trello_id: &str,
        message: &str,
    ) -> Result<(), Failure> {
        let mut tx = self.database().immediate_transaction().await?;
        let changed = sqlx::query(
            "UPDATE trello_import_items SET state = 'failed', error = $1, data_json = NULL \
             WHERE import_id = $2 AND kind = $3 AND trello_id = $4 AND state <> 'failed'",
        )
        .bind(clamp(message, 500, 2_000))
        .bind(import_id.to_string())
        .bind(kind)
        .bind(trello_id)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        sqlx::query(
            "UPDATE trello_imports SET failed = failed + $1, updated_at = $2 WHERE id = $3",
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
        sqlx::query("UPDATE trello_imports SET updated_at = $1 WHERE id = $2")
            .bind(TimestampMillis::now().as_millis())
            .bind(import_id.to_string())
            .execute(self.database().pool())
            .await?;
        Ok(())
    }

    async fn save_report(&self, run: &Run<'_>) -> Result<(), Failure> {
        sqlx::query("UPDATE trello_imports SET report_json = $1, updated_at = $2 WHERE id = $3")
            .bind(serde_json::to_string(&run.report).unwrap_or_default())
            .bind(TimestampMillis::now().as_millis())
            .bind(run.import_id.to_string())
            .execute(self.database().pool())
            .await?;
        Ok(())
    }

    /// Moves an open import to a final state and deletes its credentials.
    async fn finish(
        &self,
        import_id: Id,
        status: &str,
        error: Option<&str>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE trello_imports SET status = $1, key_ciphertext = NULL, token_ciphertext = NULL, \
             error = $2, updated_at = $3 WHERE id = $4 AND status IN {OPEN}"
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
        key: Option<Vec<u8>>,
        token: Option<Vec<u8>>,
    ) -> Result<TrelloClient, String> {
        let app_key = self
            .inner
            .settings
            .app_key
            .ok_or_else(|| "The server app key is not configured.".to_owned())?;
        let (Some(key), Some(token)) = (key, token) else {
            return Err("The Trello API key and token were deleted.".to_owned());
        };
        let decrypt = |ciphertext: &[u8]| {
            decrypt_secret(&app_key, ciphertext)
                .map_err(|()| "The Trello API key or token could not be decrypted.".to_owned())
        };
        TrelloClient::new(
            &decrypt(&key)?,
            &decrypt(&token)?,
            self.inner.settings.client.clone(),
        )
        .map_err(|_| "The Trello API key or token is invalid.".to_owned())
    }

    async fn enqueue(
        &self,
        kind: JobKind,
        import_id: Id,
        workspace_id: Id,
    ) -> Result<(), TrelloImportError> {
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
            .map_err(|error| TrelloImportError::Unavailable(error.to_string()))
    }

    async fn require_member(&self, workspace_id: Id, user_id: Id) -> Result<(), TrelloImportError> {
        membership::actor(self.database().pool(), workspace_id, user_id)
            .await?
            .map(|_| ())
            .ok_or(TrelloImportError::NotFound)
    }

    async fn load_items(&self, import_id: Id, kind: &str) -> Result<Vec<Item>, Failure> {
        let rows = sqlx::query(
            "SELECT trello_id, board_id, title, data_json, orbit_id, state \
             FROM trello_import_items WHERE import_id = $1 AND kind = $2 ORDER BY position",
        )
        .bind(import_id.to_string())
        .bind(kind)
        .fetch_all(self.database().pool())
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok(Item {
                    trello_id: row.get("trello_id"),
                    board_id: row.get("board_id"),
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
    ) -> Result<TrelloImport, TrelloImportError> {
        let row = sqlx::query(
            "SELECT id, workspace_id, status, trello_user_name, \
             CASE WHEN $1 THEN tree_json END AS tree_json, selection_json, total, done, failed, \
             report_json, error, created_at, updated_at FROM trello_imports \
             WHERE id = $2 AND workspace_id = $3 AND user_id = $4",
        )
        .bind(with_tree)
        .bind(import_id.to_string())
        .bind(workspace_id.to_string())
        .bind(user_id.to_string())
        .fetch_optional(self.database().pool())
        .await?
        .ok_or(TrelloImportError::NotFound)?;
        let invalid = |_| TrelloImportError::Unavailable("invalid stored import".to_owned());
        let status = TrelloImportStatus::parse(&row.get::<String, _>("status"))?;
        let started = row.get::<Option<String>, _>("selection_json").is_some();
        let tree = row
            .get::<Option<String>, _>("tree_json")
            .map(|json| serde_json::from_str::<TrelloImportTree>(&json))
            .transpose()
            .map_err(|_| TrelloImportError::Unavailable("invalid stored tree".to_owned()))?;
        let report = if started {
            let stored: StoredReport = row
                .get::<Option<String>, _>("report_json")
                .and_then(|json| serde_json::from_str(&json).ok())
                .unwrap_or_default();
            let failures = sqlx::query(
                "SELECT trello_id, kind, title, error FROM trello_import_items \
                 WHERE import_id = $1 AND state = 'failed' ORDER BY position LIMIT $2",
            )
            .bind(import_id.to_string())
            .bind(MAX_FAILURES_LISTED)
            .fetch_all(self.database().pool())
            .await?
            .into_iter()
            .map(|row| TrelloImportFailure {
                trello_id: row.get("trello_id"),
                kind: row.get("kind"),
                title: row.get("title"),
                error: row.get::<Option<String>, _>("error").unwrap_or_default(),
            })
            .collect();
            Some(TrelloImportReport {
                comments_imported: stored.comments_imported,
                files_imported: stored.files_imported,
                missing_files: stored.missing_files,
                refused_files: stored.refused_files,
                links_added: stored.links_added,
                checklists_added: stored.checklists_added,
                unmatched_members: stored.unmatched_members,
                archived_lists_skipped: stored.archived_lists_skipped,
                archived_cards_skipped: stored.archived_cards_skipped,
                failures,
            })
        } else {
            None
        };
        let project_ids = sqlx::query_scalar::<_, String>(
            "SELECT items.orbit_id FROM trello_import_items AS items \
             JOIN projects ON projects.id = items.orbit_id \
             WHERE items.import_id = $1 AND items.kind = 'board' AND projects.workspace_id = $2 \
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
        Ok(TrelloImport {
            id: row.get::<String, _>("id").parse().map_err(invalid)?,
            workspace_id: row
                .get::<String, _>("workspace_id")
                .parse()
                .map_err(invalid)?,
            status,
            trello_user_name: row.get("trello_user_name"),
            progress: TrelloImportProgress {
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

/// Which credential a `401` body names: Trello answers `invalid key` for a bad key and
/// `invalid token`, `expired token` or similar for a bad token.
fn rejected_credential(message: &str) -> TrelloCredential {
    let lower = message.to_ascii_lowercase();
    if lower.contains("key") && !lower.contains("token") {
        TrelloCredential::Key
    } else {
        TrelloCredential::Token
    }
}

/// The hex color of a Trello label color (`green`, `red_dark`, …).
fn label_color(color: &str) -> Option<&'static str> {
    let base = color.split('_').next()?;
    TRELLO_COLORS
        .iter()
        .find(|(name, _)| *name == base)
        .map(|(_, hex)| *hex)
}

fn stored_field(field: CustomField) -> StoredField {
    StoredField {
        name: clamp(field.name.as_deref().unwrap_or("Field").trim(), 100, 400),
        kind: field.kind.unwrap_or_default(),
        options: field
            .options
            .into_iter()
            .filter_map(|option| {
                let text = option.value?.get("text")?.as_str()?.to_owned();
                Some((option.id, text))
            })
            .collect(),
        id: field.id,
    }
}

/// A card's description and the members it could not assign.
struct CardText {
    markdown: String,
    assignees: Vec<Id>,
    /// Members named in the description instead.
    unmatched_members: u32,
    links: u32,
}

/// The card's description from `body` (its markdown, or the markdown with file URLs
/// rewritten), with its assignees: the same on every call, so phase C can rewrite it.
fn card_text(run: &Run<'_>, board_id: &str, card: &TrelloCard, body: &str) -> CardText {
    let board = run.boards.get(board_id);
    let mut assignees = Vec::new();
    let mut unmatched = Vec::new();
    for member in &card.id_members {
        let name = board
            .and_then(|board| board.members.get(member))
            .cloned()
            .unwrap_or_else(|| "Trello member".to_owned());
        match run.member_named(&name) {
            Some(id) if !assignees.contains(&id) => assignees.push(id),
            Some(_) => {}
            None => unmatched.push(name),
        }
    }
    let completed = board
        .and_then(|board| board.lists.get(&card.id_list))
        .is_some_and(|(_, category)| category == "completed");
    let mut fields: Vec<(&str, String)> = Vec::new();
    if !unmatched.is_empty() {
        fields.push(("Members", unmatched.join(", ")));
    }
    if card.due_complete && card.due.is_some() && !completed {
        fields.push(("Due date", "marked complete".to_owned()));
    }
    let custom: Vec<(String, String)> = board
        .map(|board| custom_field_lines(board, card))
        .unwrap_or_default();
    for (name, value) in &custom {
        fields.push((name.as_str(), value.clone()));
    }
    let links: Vec<(String, String)> = card
        .attachments
        .iter()
        .filter(|attachment| !attachment.is_upload)
        .filter_map(|attachment| {
            let url = attachment
                .url
                .clone()
                .filter(|url| url.starts_with("https://") || url.starts_with("http://"))?;
            let title = attachment
                .name
                .clone()
                .filter(|name| !name.trim().is_empty())
                .unwrap_or_else(|| url.clone());
            Some((title, url))
        })
        .collect();
    let body = with_checklists(body, &card.checklists);
    CardText {
        markdown: with_source_details(&body, "Trello", &fields, &links),
        assignees,
        unmatched_members: u32::try_from(unmatched.len()).unwrap_or(u32::MAX),
        links: u32::try_from(links.len()).unwrap_or(u32::MAX),
    }
}

/// `(field name, value)` of the card's custom field values.
fn custom_field_lines(board: &BoardContext, card: &TrelloCard) -> Vec<(String, String)> {
    card.custom_field_items
        .iter()
        .filter_map(|item| {
            let field = board.fields.get(&item.id_custom_field)?;
            let value = match (field.kind.as_str(), &item.id_value, &item.value) {
                ("list", Some(option), _) => field
                    .options
                    .iter()
                    .find(|(id, _)| id == option)
                    .map(|(_, text)| text.clone())?,
                ("checkbox", _, Some(value)) => {
                    if value.get("checked").and_then(Value::as_str) == Some("true") {
                        "yes".to_owned()
                    } else {
                        "no".to_owned()
                    }
                }
                ("date", _, Some(value)) => display_instant(value.get("date")?.as_str()?),
                (_, _, Some(value)) => value
                    .get("text")
                    .or_else(|| value.get("number"))
                    .and_then(Value::as_str)?
                    .to_owned(),
                _ => return None,
            };
            let value = value.trim().to_owned();
            (!value.is_empty()).then(|| (field.name.clone(), clamp(&value, 500, 2_000)))
        })
        .collect()
}

/// Appends each checklist as a bold name over a task list (`- [x] item`), which Orbit turns
/// into checklist blocks.
fn with_checklists(body: &str, checklists: &[TrelloChecklist]) -> String {
    let mut markdown = body.trim().to_owned();
    for checklist in checklists {
        if !markdown.is_empty() {
            markdown.push_str("\n\n");
        }
        let name = checklist
            .name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .unwrap_or("Checklist");
        markdown.push_str(&format!(
            "**{}**\n",
            escape_markdown(&name.replace(['\r', '\n'], " "))
        ));
        for item in &checklist.check_items {
            let done = item.state.as_deref() == Some("complete");
            let text = item
                .name
                .as_deref()
                .unwrap_or_default()
                .replace(['\r', '\n'], " ");
            markdown.push_str(&format!(
                "\n- [{}] {}",
                if done { "x" } else { " " },
                text.trim()
            ));
        }
    }
    markdown
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trello::model::{CheckItem, CustomFieldItem};

    #[test]
    fn maps_label_colors_and_rejected_credentials() {
        assert_eq!(label_color("green"), Some("#4bce97"));
        assert_eq!(label_color("red_dark"), Some("#f87168"));
        assert_eq!(label_color("plaid"), None);
        assert_eq!(rejected_credential("invalid key"), TrelloCredential::Key);
        assert_eq!(
            rejected_credential("invalid token"),
            TrelloCredential::Token
        );
        assert_eq!(
            rejected_credential("expired token"),
            TrelloCredential::Token
        );
        assert_eq!(rejected_credential(""), TrelloCredential::Token);
    }

    #[test]
    fn writes_checklists_as_task_lists() {
        let checklist = TrelloChecklist {
            name: Some("Launch *list*".into()),
            check_items: vec![
                CheckItem {
                    name: Some("Write post".into()),
                    state: Some("complete".into()),
                    pos: 1.0,
                },
                CheckItem {
                    name: Some("Ship\nit".into()),
                    state: Some("incomplete".into()),
                    pos: 2.0,
                },
            ],
            ..TrelloChecklist::default()
        };
        assert_eq!(
            with_checklists("Body", &[checklist]),
            "Body\n\n**Launch \\*list\\***\n\n- [x] Write post\n- [ ] Ship it"
        );
    }

    #[test]
    fn reads_custom_field_values() {
        let field = |id: &str, kind: &str| StoredField {
            id: id.into(),
            name: id.to_uppercase(),
            kind: kind.into(),
            options: vec![("o1".into(), "High".into())],
        };
        let board = BoardContext {
            project_id: Id::new_v7(),
            lists: HashMap::new(),
            trello_labels: HashMap::new(),
            labels: HashMap::new(),
            members: HashMap::new(),
            fields: [
                field("list", "list"),
                field("check", "checkbox"),
                field("num", "number"),
            ]
            .into_iter()
            .map(|field| (field.id.clone(), field))
            .collect(),
        };
        let item = |id: &str, option: Option<&str>, value: Option<Value>| CustomFieldItem {
            id_custom_field: id.into(),
            id_value: option.map(str::to_owned),
            value,
        };
        let card = TrelloCard {
            custom_field_items: vec![
                item("list", Some("o1"), None),
                item("check", None, Some(json!({"checked": "true"}))),
                item("num", None, Some(json!({"number": "3.5"}))),
                item("gone", None, Some(json!({"text": "x"}))),
            ],
            ..TrelloCard::default()
        };
        assert_eq!(
            custom_field_lines(&board, &card),
            [
                ("LIST".to_owned(), "High".to_owned()),
                ("CHECK".to_owned(), "yes".to_owned()),
                ("NUM".to_owned(), "3.5".to_owned()),
            ]
        );
    }
}
