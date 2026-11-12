//! What a scoped credential (an API token today) may read and change. MCP tools and the public
//! REST API are thin layers over this module, so the token's limits live in one place: its
//! workspace, its approved projects, its scopes, the billing write guard, and no docs or chat
//! for service accounts.
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use orbit_platform::{Id, Row, TimestampMillis};
use serde::Serialize;
use serde_json::{Value, json};
use utoipa::ToSchema;

use crate::billing::WorkspaceAccess;
use crate::collab::kind_task::members;
use crate::collab::markdown_import::markdown_to_blocks;
use crate::collab::markdown_merge::{MAX_BLOCKS, block_count};
use crate::export::markdown::{KeepLinks, LinkResolver, PageLink, blocks_to_markdown};
use crate::repositories::api_tokens::{ApiTokenPrincipal, ApiTokenScope};
use crate::repositories::billing::write_access;
use crate::repositories::chat::{
    ChatError, ChatRepository, ConversationRecord, MessageCursor, MessagePage, SearchInput,
    SearchPage, ThreadPage,
};
use crate::repositories::membership;
use crate::repositories::page_versions::{PageVersion, PageVersionList};
use crate::repositories::pages::{
    CreatePage, MarkdownContent, PageChanges, PageError, PageList, PageRecord, PageRepository,
    SpaceRequest, revision,
};
use crate::repositories::task_filter::{
    Condition, FilterField, FilterGroup, FilterNode, FilterOperator, GroupOp, ShowCompleted,
    SubIssuesDisplay,
};
use crate::repositories::task_recurrences::TaskRecurrence;
use crate::repositories::tasks::{
    CommentContent, CommentRecord, CreateTask, CycleRecord, ImportedComment, ImportedTask,
    LabelRecord, MilestoneRecord, Page, ProjectRecord, SortOrder, StatusRecord, TaskChanges,
    TaskError, TaskFilter, TaskRecord, TaskRepository, TaskSort, TaskUpdate, TokenActor,
    normalize_project_key, parse_task_identifier,
};
use crate::repositories::workspaces::{WorkspaceError, WorkspaceRepository};
use crate::task_import::escape_markdown;
use orbit_domain::Permission;

mod databases;
mod relations;
mod setup;
pub use databases::{DatabaseRowPatch, NewDatabaseRow};
pub use setup::StatusPatch;

const PRIORITIES: [&str; 5] = ["none", "low", "medium", "high", "urgent"];
/// The web app's second project color, indigo.
const DEFAULT_PROJECT_COLOR: &str = "#6366f1";
/// Find an imported task or comment by `external_id` (bind order: workspace, external ID).
const EXTERNAL_TASK: &str = "SELECT id, id AS task_id, deleted_at FROM tasks \
     WHERE workspace_id = $1 AND external_id = $2";
const EXTERNAL_COMMENT: &str = "SELECT task_comments.id, task_comments.task_id, tasks.deleted_at \
     FROM task_comments JOIN tasks ON tasks.id = task_comments.task_id \
     WHERE task_comments.workspace_id = $1 AND task_comments.external_id = $2";

#[derive(Debug)]
pub enum TokenApiError {
    /// The credential lacks the scope of the operation.
    MissingScope,
    /// A task write in a workspace without an active subscription.
    ReadOnly,
    BillingUnavailable,
    Invalid(&'static str),
    /// Outside the grant, hidden, or missing: callers must not tell these apart.
    NotAvailable,
    /// Deleting a task forever takes a workspace owner or admin.
    PurgeForbidden,
    /// Creating a project takes a credential for all projects: one limited to chosen projects
    /// could not use the new project.
    AllProjectsRequired,
    /// A `tasks:import` credential whose creator is no longer a workspace owner or admin.
    ImportForbidden,
    Task(TaskError),
    Page(PageError),
    /// A database refusal other than not found (see `databases`).
    Database(crate::repositories::databases::DatabaseError),
    /// Why it failed, for the server log; clients do not see it.
    Internal(String),
}

impl From<PageError> for TokenApiError {
    fn from(error: PageError) -> Self {
        match error {
            PageError::Unavailable(error) => Self::Internal(error.to_string()),
            error @ PageError::Corrupt => Self::Internal(error.to_string()),
            error => Self::Page(error),
        }
    }
}

impl From<TaskError> for TokenApiError {
    fn from(error: TaskError) -> Self {
        Self::Task(error)
    }
}

impl From<sqlx::Error> for TokenApiError {
    fn from(error: sqlx::Error) -> Self {
        Self::Internal(error.to_string())
    }
}

type Result<T> = std::result::Result<T, TokenApiError>;

/// Why a doc write failed with [`PageError::TooLarge`].
pub const DOC_TOO_LARGE_DETAIL: &str = "A doc can have at most 4,000 blocks (paragraphs, \
    headings, list items and the like, nested ones included), before and after a write. Split \
    the content into several docs.";
const _: () = assert!(MAX_BLOCKS == 4_000, "DOC_TOO_LARGE_DETAIL names the limit");

/// A task without `TaskRecord`'s embedded parent, duplicate and ancestor references: those can
/// name tasks from projects outside the grant.
#[derive(Clone, Debug, Serialize, ToSchema)]
#[schema(description = "A task's own fields.")]
pub struct ReadTask {
    #[schema(value_type = String)]
    pub id: Id,
    #[schema(value_type = String)]
    pub project_id: Id,
    /// With the project key, the task identifier (`ENG-12`).
    pub number: i64,
    #[schema(value_type = String)]
    pub status_id: Id,
    pub title: String,
    /// Markdown.
    pub description: String,
    /// `none`, `low`, `medium`, `high` or `urgent`.
    pub priority: String,
    #[schema(value_type = Vec<String>)]
    pub assignee_ids: Vec<Id>,
    #[schema(value_type = Vec<String>)]
    pub label_ids: Vec<Id>,
    #[schema(value_type = Option<String>, format = DateTime, required = true)]
    pub due_start_at: Option<TimestampMillis>,
    #[schema(value_type = Option<String>, format = DateTime, required = true)]
    pub due_at: Option<TimestampMillis>,
    /// How the task repeats; null when it does not. Completing it creates the next occurrence.
    #[schema(required = true)]
    pub recurrence: Option<TaskRecurrence>,
    /// A milestone of the task's project; `null` when none.
    #[schema(value_type = Option<String>, required = true)]
    pub milestone_id: Option<Id>,
    /// A cycle of the task's project; `null` when none.
    #[schema(value_type = Option<String>, required = true)]
    pub cycle_id: Option<Id>,
    /// Points, 0 to 1000; `null` when not estimated.
    #[schema(required = true)]
    pub estimate: Option<i64>,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: TimestampMillis,
    #[schema(value_type = String, format = DateTime)]
    pub updated_at: TimestampMillis,
    /// When the task entered a completed or cancelled status; `null` while it is open.
    #[schema(value_type = Option<String>, format = DateTime, required = true)]
    pub completed_at: Option<TimestampMillis>,
    /// The member who created the task; `null` when a service account did.
    #[schema(value_type = Option<String>, required = true)]
    pub creator_id: Option<Id>,
    /// Set when a service account created the task; show it as the creator then.
    #[schema(required = true)]
    pub creator_service_account: Option<Named>,
    /// The source tool's ID of an imported task (`tasks:import`); `null` otherwise.
    #[schema(required = true)]
    pub external_id: Option<String>,
    /// The task's revision: an opaque number below 2^53 that changes with every change, live
    /// edits of the description included. Compare it for equality only, and send it back as
    /// `expected_version`.
    pub version: u64,
}

/// [`ReadTask`] plus the parent's id while the parent is visible and in an approved project, the
/// identifier (`ENG-12`) and the task's page in the web app.
#[derive(Clone, Debug, Serialize, ToSchema)]
#[schema(
    as = PublicTask,
    description = "A task, without embedded parent, duplicate or ancestor records."
)]
pub struct TaskView {
    #[serde(flatten)]
    #[schema(inline)]
    pub task: ReadTask,
    /// `null` when the task has no parent or the parent is not in an approved project.
    #[schema(value_type = Option<String>, required = true)]
    pub parent_task_id: Option<Id>,
    /// The project key and the task number (`ENG-12`), for `{task}` paths.
    pub identifier: String,
    /// The task's page in the Orbit web app.
    pub url: String,
}

impl TaskView {
    fn new(task: TaskRecord, grant: &[Id], key: &str, web_origin: &str) -> Self {
        let identifier = format!("{key}-{}", task.number);
        let url = format!(
            "{web_origin}/tasks/{}?workspace={}",
            task.id, task.workspace_id
        );
        let parent_task_id = task
            .parent
            .as_ref()
            .filter(|parent| grant.contains(&parent.project_id))
            .map(|parent| parent.id);
        Self {
            task: ReadTask::new(task),
            parent_task_id,
            identifier,
            url,
        }
    }
}

impl ReadTask {
    pub(crate) fn new(task: TaskRecord) -> Self {
        Self {
            id: task.id,
            project_id: task.project_id,
            number: task.number,
            status_id: task.status_id,
            title: task.title,
            description: task.description,
            priority: task.priority,
            assignee_ids: task.assignee_ids,
            label_ids: task.label_ids,
            due_start_at: task.due_start_at,
            due_at: task.due_at,
            recurrence: task.recurrence,
            milestone_id: task.milestone_id,
            cycle_id: task.cycle_id,
            estimate: task.estimate,
            created_at: task.created_at,
            updated_at: task.updated_at,
            completed_at: task.completed_at,
            creator_id: task.creator_id,
            creator_service_account: task.creator_service_account_id.map(|id| Named {
                id,
                name: task.creator_service_account_name.unwrap_or_default(),
            }),
            external_id: task.external_id,
            version: revision(task.version, task.updated_at),
        }
    }
}

/// A workspace member as the public API shows it: no email or role.
#[derive(Clone, Debug, Serialize, ToSchema)]
#[schema(as = PublicMember)]
pub struct MemberView {
    #[schema(value_type = String)]
    pub user_id: Id,
    pub display_name: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[schema(as = PublicMe)]
pub struct Me {
    pub token: TokenInfo,
    pub workspace: Named,
    /// Every workspace the token may use, with its access there. Always one item today.
    pub workspaces: Vec<WorkspaceGrant>,
    /// The token's creator; `null` for a service-account token.
    #[schema(required = true)]
    pub user: Option<UserInfo>,
    /// Set for a service-account token.
    #[schema(required = true)]
    pub service_account: Option<Named>,
}

#[derive(Debug, Serialize, ToSchema)]
#[schema(as = PublicWorkspaceGrant)]
pub struct WorkspaceGrant {
    #[schema(value_type = String)]
    pub id: Id,
    pub name: String,
    #[schema(value_type = Vec<ApiTokenScope>)]
    pub scopes: Vec<&'static str>,
    #[schema(value_type = Vec<String>)]
    pub project_ids: Vec<Id>,
}

#[derive(Debug, Serialize, ToSchema)]
#[schema(as = PublicTokenInfo)]
pub struct TokenInfo {
    #[schema(value_type = String)]
    pub id: Id,
    pub name: String,
    #[schema(value_type = Vec<ApiTokenScope>)]
    pub scopes: Vec<&'static str>,
    #[schema(value_type = Vec<String>)]
    pub project_ids: Vec<Id>,
    #[schema(value_type = Option<String>, format = DateTime, required = true)]
    pub expires_at: Option<TimestampMillis>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[schema(as = PublicNamed)]
pub struct Named {
    #[schema(value_type = String)]
    pub id: Id,
    pub name: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[schema(as = PublicUser)]
pub struct UserInfo {
    #[schema(value_type = String)]
    pub id: Id,
    pub email: String,
    pub display_name: String,
}

#[derive(Debug, Default)]
pub struct TaskQuery {
    pub project_id: Option<Id>,
    pub query: Option<String>,
    pub status_id: Option<Id>,
    pub assignee: Option<AssigneeFilter>,
    pub label_id: Option<Id>,
    pub priority: Option<String>,
    pub milestone: Option<MilestoneFilter>,
    pub cycle: Option<CycleFilter>,
    /// Only tasks due strictly before this moment.
    pub due_before: Option<TimestampMillis>,
    /// Only tasks due strictly after this moment.
    pub due_after: Option<TimestampMillis>,
    /// `true`: only tasks with a due date; `false`: only tasks without one.
    pub has_due_date: Option<bool>,
    /// Only the direct sub-issues of this task, which must be in an approved project.
    pub parent_task_id: Option<Id>,
    /// Newest first when unset.
    pub sort: Option<(TaskSort, SortOrder)>,
    pub cursor: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Clone, Copy, Debug)]
pub enum AssigneeFilter {
    User(Id),
    Unassigned,
}

/// A `list_tasks` milestone filter, as `parse_milestone_filter` reads it.
#[derive(Clone, Copy, Debug)]
pub enum MilestoneFilter {
    Milestone(Id),
    Unset,
}

/// A `list_tasks` cycle filter, as `parse_cycle_filter` reads it. The relative values name the
/// cycle of each task's own project, as [`CycleView::relative`] marks it.
#[derive(Clone, Copy, Debug)]
pub enum CycleFilter {
    Cycle(Id),
    Relative(CycleRelative),
    Unset,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[schema(as = PublicCycleRelative)]
#[serde(rename_all = "snake_case")]
pub enum CycleRelative {
    /// The cycle that runs now.
    Current,
    /// The first cycle that starts after now.
    Next,
    /// The last cycle that ended before now.
    Previous,
}

impl CycleRelative {
    fn as_str(self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::Next => "next",
            Self::Previous => "previous",
        }
    }
}

/// A due-date bound: `YYYY-MM-DD` (00:00 UTC of that day) or an RFC 3339 time.
pub fn parse_due_bound(raw: &str, invalid: &'static str) -> Result<TimestampMillis> {
    crate::task_import::parse_instant_millis(raw)
        .or_else(|| {
            crate::repositories::task_filter::parse_ymd(raw.trim()).map(|day| day * 86_400_000)
        })
        .map(TimestampMillis::from_millis)
        .ok_or(TokenApiError::Invalid(invalid))
}

/// A milestone ID, or `none` for tasks without a milestone.
pub fn parse_milestone_filter(raw: &str) -> Result<MilestoneFilter> {
    match raw {
        "none" => Ok(MilestoneFilter::Unset),
        id => id
            .parse()
            .map(MilestoneFilter::Milestone)
            .map_err(|_| TokenApiError::Invalid("milestone_id must be a milestone ID or `none`")),
    }
}

/// A cycle ID, `current`, `next`, `previous`, or `none` for tasks without a cycle.
pub fn parse_cycle_filter(raw: &str) -> Result<CycleFilter> {
    Ok(match raw {
        "none" => CycleFilter::Unset,
        "current" => CycleFilter::Relative(CycleRelative::Current),
        "next" => CycleFilter::Relative(CycleRelative::Next),
        "previous" => CycleFilter::Relative(CycleRelative::Previous),
        id => id.parse().map(CycleFilter::Cycle).map_err(|_| {
            TokenApiError::Invalid("cycle_id must be a cycle ID, current, next, previous or none")
        })?,
    })
}

/// A cycle and where it stands now: `current`, `next`, `previous`, or `null` for any other.
#[derive(Clone, Debug, Serialize)]
pub struct CycleView {
    #[serde(flatten)]
    pub cycle: CycleRecord,
    pub relative: Option<CycleRelative>,
}

/// Marks the current, next and previous cycle of `cycles` (one project's, by start date, so not
/// overlapping) at `now`, as the `cycle` task filter reads them.
fn cycle_views(cycles: Vec<CycleRecord>, now: TimestampMillis) -> Vec<CycleView> {
    let now = now.as_millis();
    let current = cycles
        .iter()
        .position(|c| c.starts_at.as_millis() <= now && now < c.ends_at.as_millis());
    let next = cycles.iter().position(|c| c.starts_at.as_millis() > now);
    let previous = cycles.iter().rposition(|c| c.ends_at.as_millis() <= now);
    cycles
        .into_iter()
        .enumerate()
        .map(|(index, cycle)| CycleView {
            relative: [
                (current, CycleRelative::Current),
                (next, CycleRelative::Next),
                (previous, CycleRelative::Previous),
            ]
            .into_iter()
            .find_map(|(at, relative)| (at == Some(index)).then_some(relative)),
            cycle,
        })
        .collect()
}

#[derive(Debug)]
pub struct NewTask {
    pub project_id: Id,
    pub status_id: Id,
    pub title: String,
    pub description: String,
    pub priority: String,
    pub assignee_ids: Vec<Id>,
    pub label_ids: Vec<Id>,
    pub parent_task_id: Option<Id>,
    pub due_start_at: Option<TimestampMillis>,
    pub due_at: Option<TimestampMillis>,
    /// A milestone of `project_id`.
    pub milestone_id: Option<Id>,
    /// A cycle of `project_id`.
    pub cycle_id: Option<Id>,
    /// Points, 0 to 1000.
    pub estimate: Option<i64>,
    /// What the source tool kept; needs `tasks:import`.
    pub import: Option<ImportedTask>,
}

#[derive(Debug)]
pub struct NewComment {
    /// Markdown.
    pub body: String,
    /// A comment of the same task: the new comment is its reply.
    pub parent_id: Option<Id>,
    /// What the source tool kept; needs `tasks:import`.
    pub import: Option<CommentImport>,
}

/// The source tool's comment: [`ImportedComment`], or an author who is not a member, shown as a
/// bold first line (as the built-in importers do).
#[derive(Debug, Default)]
pub struct CommentImport {
    pub source: ImportedComment,
    pub author_name: Option<String>,
}

/// A write that is not repeated: `created` is false when an earlier write with the same
/// `external_id` made `value`.
#[derive(Debug)]
pub struct Written<T> {
    pub value: T,
    pub created: bool,
}

/// A doc with its content also as Markdown, the format that doc writes take. `version` is the
/// page's [`revision`], so it changes with co-edits too.
#[derive(Clone, Debug, Serialize, ToSchema)]
#[schema(
    as = PublicDoc,
    description = "A doc record. `version` is an opaque number below 2^53 that changes with every change, live edits included: compare it for equality only, and send it back as `expected_version`."
)]
pub struct DocView {
    #[serde(flatten)]
    pub page: PageRecord,
    /// The content as GitHub-flavored Markdown, the format that doc writes take.
    pub markdown: String,
    /// A row of a database: its values, as `POST /docs/{page_id}/database/query` shows them.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<Object>)]
    pub values: Option<std::collections::BTreeMap<String, Value>>,
}

/// A doc after a move.
#[derive(Clone, Debug, Serialize, ToSchema)]
#[schema(as = PublicMovedDoc)]
pub struct MovedDocView {
    #[serde(flatten)]
    pub doc: DocView,
    /// Property values that the move deleted: the doc left its database, or the new database has
    /// no property of the same name and type (or no such option) for them.
    pub dropped_values: u64,
}

/// A stored version of a doc: its title, icon and content as they were.
#[derive(Clone, Debug, Serialize, ToSchema)]
#[schema(as = PublicDocVersion)]
pub struct DocVersionView {
    #[serde(flatten)]
    pub version: PageVersion,
    /// The content as GitHub-flavored Markdown, as in a doc read.
    pub markdown: String,
}

/// Links pages to `/docs/<id>` under the titles of the pages that the actor can see; any other
/// page stays "Untitled", so a link does not tell what a hidden page is called.
struct DocLinks(HashMap<String, String>);

impl LinkResolver for DocLinks {
    fn page(&self, page_id: &str) -> Option<PageLink> {
        KeepLinks.page(page_id).map(|link| PageLink {
            title: self.0.get(page_id).cloned().unwrap_or(link.title),
            ..link
        })
    }

    fn url(&self, url: &str) -> String {
        KeepLinks.url(url)
    }
}

/// The ids in the `page` and `database` blocks of `blocks`, nested blocks included.
fn linked_pages(blocks: &[Value], ids: &mut HashSet<String>) {
    for block in blocks {
        let target = match block["type"].as_str() {
            Some("page") => block["props"]["pageId"].as_str(),
            Some("database") => block["props"]["databaseId"].as_str(),
            _ => None,
        };
        if let Some(id) = target {
            ids.insert(id.to_owned());
        }
        if let Some(children) = block["children"].as_array() {
            linked_pages(children, ids);
        }
    }
}

#[derive(Debug, Default)]
pub struct NewDoc {
    /// With a parent the doc joins the parent's space.
    pub parent_id: Option<Id>,
    /// Root docs only: the teamspace; with neither this nor `private`, the default teamspace.
    pub teamspace_id: Option<Id>,
    /// Root docs only: `true` puts the doc in the token creator's private space.
    pub private: Option<bool>,
    pub title: String,
    pub icon: Option<String>,
    pub markdown: String,
}

#[derive(Debug, Default)]
pub struct DocMove {
    /// The new parent; `None` moves the doc to a space root.
    pub parent_id: Option<Id>,
    /// Root moves only: the target teamspace. Unset keeps the doc's space.
    pub teamspace_id: Option<Id>,
    /// Root moves only: `true` moves the doc to the token creator's private space.
    pub private: Option<bool>,
    /// Index among the new siblings; `None` puts the doc last.
    pub position: Option<u64>,
}

#[derive(Debug, Default)]
pub struct DocPatch {
    pub title: Option<String>,
    /// `Some(None)` clears the icon.
    pub icon: Option<Option<String>>,
    /// The whole new content, merged onto the current one (see
    /// `PageRepository::update_page_markdown_at_revision`).
    pub markdown: Option<String>,
}

#[derive(Debug, Default)]
pub struct TaskPatch {
    pub title: Option<String>,
    pub description: Option<String>,
    pub status_id: Option<Id>,
    pub priority: Option<String>,
    pub assignee_ids: Option<Vec<Id>>,
    pub label_ids: Option<Vec<Id>>,
    /// `Some(None)` clears the date.
    pub due_start_at: Option<Option<TimestampMillis>>,
    pub due_at: Option<Option<TimestampMillis>>,
    /// `Some(None)` detaches the task from its parent.
    pub parent_task_id: Option<Option<Id>>,
    /// `Some(None)` stops repeating.
    pub recurrence: Option<Option<TaskRecurrence>>,
    /// `Some(None)` clears the milestone, cycle or estimate.
    pub milestone_id: Option<Option<Id>>,
    pub cycle_id: Option<Option<Id>>,
    pub estimate: Option<Option<i64>>,
    /// `Some(Some(id))` marks the task as a duplicate of `id`; `Some(None)` unmarks it.
    pub duplicate_of_id: Option<Option<Id>>,
}

#[derive(Clone)]
pub struct TokenApi {
    tasks: Arc<TaskRepository>,
    pages: Option<Arc<PageRepository>>,
    chat: Option<Arc<ChatRepository>>,
    web_origin: String,
}

impl TokenApi {
    #[must_use]
    pub fn new(tasks: Arc<TaskRepository>) -> Self {
        Self {
            tasks,
            pages: None,
            chat: None,
            web_origin: String::new(),
        }
    }

    /// The origin of the web app, for the `url` of each task.
    #[must_use]
    pub fn with_web_origin(mut self, origin: &str) -> Self {
        self.web_origin = origin.trim_end_matches('/').to_owned();
        self
    }

    #[must_use]
    pub fn with_pages(mut self, pages: Arc<PageRepository>) -> Self {
        self.pages = Some(pages);
        self
    }

    #[must_use]
    pub fn with_chat(mut self, chat: Arc<ChatRepository>) -> Self {
        self.chat = Some(chat);
        self
    }

    /// Checks `scope` (and, for writes, that the workspace is writable) once per request. Every
    /// operation of the result checks that it was authorized for its own scope.
    pub async fn authorize<'a>(
        &'a self,
        principal: &'a ApiTokenPrincipal,
        scope: ApiTokenScope,
        request_id: &'a str,
    ) -> Result<Authorized<'a>> {
        if !principal.allows(scope) {
            return Err(TokenApiError::MissingScope);
        }
        if matches!(scope, ApiTokenScope::TasksWrite | ApiTokenScope::DocsWrite) {
            match write_access(self.tasks.database(), principal.workspace_id).await {
                Ok(WorkspaceAccess::ReadOnly) => return Err(TokenApiError::ReadOnly),
                Ok(_) => {}
                Err(_) => return Err(TokenApiError::BillingUnavailable),
            }
        }
        // Import writes can backdate work and name other members as its authors, so they stay
        // with owners and admins: checked on each write, as the creator's role can change.
        if scope == ApiTokenScope::TasksWrite
            && principal.scopes.contains(&ApiTokenScope::TasksImport)
        {
            let actor = membership::actor(
                self.tasks.database().pool(),
                principal.workspace_id,
                principal.actor_id,
            )
            .await?;
            if !actor.is_some_and(|actor| actor.can(Permission::ApiTokensManage)) {
                return Err(TokenApiError::ImportForbidden);
            }
        }
        Ok(Authorized {
            api: self,
            principal,
            scope,
            request_id,
        })
    }

    /// The token, its workspace and its identity. Needs no scope.
    pub async fn me(&self, principal: &ApiTokenPrincipal) -> Result<Me> {
        let row = sqlx::query(
            "SELECT api_tokens.name, api_tokens.expires_at, workspaces.name AS workspace_name, \
             users.email, users.display_name \
             FROM api_tokens JOIN workspaces ON workspaces.id = api_tokens.workspace_id \
             LEFT JOIN users ON users.id = api_tokens.created_by \
             WHERE api_tokens.id = $1 AND api_tokens.workspace_id = $2",
        )
        .bind(principal.token_id.to_string())
        .bind(principal.workspace_id.to_string())
        .fetch_optional(self.tasks.database().pool())
        .await?
        .ok_or(TokenApiError::NotAvailable)?;
        let service_account = principal.service_account.as_ref().map(|account| Named {
            id: account.id,
            name: account.name.clone(),
        });
        let user = match service_account {
            Some(_) => None,
            None => Some(UserInfo {
                id: principal.creator_id,
                email: row.get::<Option<String>, _>("email").unwrap_or_default(),
                display_name: row
                    .get::<Option<String>, _>("display_name")
                    .unwrap_or_default(),
            }),
        };
        let scopes: Vec<&'static str> = principal
            .scopes
            .iter()
            .map(|scope| scope.as_str())
            .collect();
        let workspace_name: String = row.get("workspace_name");
        Ok(Me {
            workspaces: vec![WorkspaceGrant {
                id: principal.workspace_id,
                name: workspace_name.clone(),
                scopes: scopes.clone(),
                project_ids: principal.project_ids.clone(),
            }],
            token: TokenInfo {
                id: principal.token_id,
                name: row.get("name"),
                scopes,
                project_ids: principal.project_ids.clone(),
                expires_at: row
                    .get::<Option<i64>, _>("expires_at")
                    .map(TimestampMillis::from_millis),
            },
            workspace: Named {
                id: principal.workspace_id,
                name: workspace_name,
            },
            user,
            service_account,
        })
    }
}

pub struct Authorized<'a> {
    api: &'a TokenApi,
    principal: &'a ApiTokenPrincipal,
    scope: ApiTokenScope,
    request_id: &'a str,
}

impl Authorized<'_> {
    fn need(&self, scope: ApiTokenScope) -> Result<()> {
        if self.scope == scope {
            Ok(())
        } else {
            Err(TokenApiError::MissingScope)
        }
    }

    fn approved(&self, project_id: Id) -> bool {
        self.principal.project_ids.contains(&project_id)
    }

    /// A `tasks:import` credential: its writes notify nobody and run no automation, and may
    /// keep the source tool's dates, numbers, authors and IDs.
    fn importing(&self) -> bool {
        self.principal.scopes.contains(&ApiTokenScope::TasksImport)
    }

    /// The task repository limited to the token's projects, attributing writes to the token.
    fn granted(&self) -> TaskRepository {
        let tasks = if self.importing() {
            self.api.tasks.as_ref().clone().with_import()
        } else {
            self.api.tasks.as_ref().clone()
        };
        tasks
            .with_project_grant(self.principal.project_ids.clone())
            .with_token(TokenActor {
                token_id: self.principal.token_id,
                service_account: self
                    .principal
                    .service_account
                    .as_ref()
                    .map(|account| (account.id, account.name.clone())),
            })
            .with_revisions()
    }

    /// The public form of `tasks`, with the identifiers built from the approved projects' keys.
    async fn views(&self, tasks: Vec<TaskRecord>) -> Result<Vec<TaskView>> {
        let p = self.principal;
        let keys: std::collections::HashMap<Id, String> = self
            .api
            .tasks
            .approved_projects(p.workspace_id, p.actor_id, &p.project_ids)
            .await?
            .into_iter()
            .map(|project| (project.id, project.key))
            .collect();
        tasks
            .into_iter()
            .map(|task| {
                let key = keys
                    .get(&task.project_id)
                    .ok_or(TokenApiError::NotAvailable)?;
                Ok(TaskView::new(
                    task,
                    &p.project_ids,
                    key,
                    &self.api.web_origin,
                ))
            })
            .collect()
    }

    async fn view(&self, task: TaskRecord) -> Result<TaskView> {
        Ok(self.views(vec![task]).await?.remove(0))
    }

    pub async fn labels(
        &self,
        project_id: Id,
        cursor: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Page<LabelRecord>> {
        self.need(ApiTokenScope::TasksRead)?;
        let limit = page_limit(cursor, limit)?;
        if !self.approved(project_id) {
            return Err(TokenApiError::NotAvailable);
        }
        let p = self.principal;
        Ok(self
            .api
            .tasks
            .labels(p.workspace_id, Some(project_id), p.actor_id, cursor, limit)
            .await?)
    }

    pub async fn members(
        &self,
        cursor: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Page<MemberView>> {
        self.need(ApiTokenScope::TasksRead)?;
        let limit = page_limit(cursor, limit)?;
        let cursor = cursor
            .map(str::parse)
            .transpose()
            .map_err(|_| TokenApiError::Invalid("Invalid cursor or limit"))?;
        let p = self.principal;
        let (members, next) = WorkspaceRepository::new(self.api.tasks.database().clone())
            .members(p.workspace_id, p.actor_id, cursor, limit)
            .await
            .map_err(|error| match error {
                WorkspaceError::NotFound => TokenApiError::NotAvailable,
                WorkspaceError::Unavailable(error) => TokenApiError::Internal(error.to_string()),
                error => TokenApiError::Internal(error.to_string()),
            })?;
        Ok(Page {
            items: members
                .into_iter()
                .map(|member| MemberView {
                    user_id: member.user_id,
                    display_name: member.display_name,
                })
                .collect(),
            next_cursor: next.map(|id| id.to_string()),
        })
    }

    pub async fn projects(&self) -> Result<Vec<ProjectRecord>> {
        self.need(ApiTokenScope::TasksRead)?;
        let p = self.principal;
        // Apply the grant in SQL; never paginate across unapproved projects.
        Ok(self
            .api
            .tasks
            .approved_projects(p.workspace_id, p.actor_id, &p.project_ids)
            .await?)
    }

    /// Creates a project with the default workflow. The credential must cover all projects, so
    /// the new project is approved for it from the next request on.
    pub async fn create_project(
        &self,
        name: String,
        key: &str,
        color: Option<String>,
    ) -> Result<ProjectRecord> {
        self.need(ApiTokenScope::TasksWrite)?;
        if !self.principal.all_projects {
            return Err(TokenApiError::AllProjectsRequired);
        }
        let name = name.trim().to_owned();
        if name.is_empty() || name.chars().count() > 200 {
            return Err(TokenApiError::Invalid("Invalid project name"));
        }
        let key = normalize_project_key(key).ok_or(TokenApiError::Invalid(
            "A project key has 1 to 20 letters, digits or underscores",
        ))?;
        let color = valid_color(color, DEFAULT_PROJECT_COLOR)?;
        let p = self.principal;
        self.granted()
            .create_project(
                p.workspace_id,
                p.actor_id,
                name,
                key,
                color,
                false,
                self.request_id,
                TimestampMillis::now(),
            )
            .await
            .map_err(|error| match error {
                TaskError::Conflict => {
                    TokenApiError::Invalid("A project with this key already exists")
                }
                error => error.into(),
            })
    }

    pub async fn milestones(&self, project_id: Id) -> Result<Vec<MilestoneRecord>> {
        self.need(ApiTokenScope::TasksRead)?;
        if !self.approved(project_id) {
            return Err(TokenApiError::NotAvailable);
        }
        let p = self.principal;
        Ok(self
            .api
            .tasks
            .milestones(p.workspace_id, project_id, p.actor_id)
            .await?)
    }

    pub async fn cycles(&self, project_id: Id) -> Result<Vec<CycleView>> {
        self.need(ApiTokenScope::TasksRead)?;
        if !self.approved(project_id) {
            return Err(TokenApiError::NotAvailable);
        }
        let p = self.principal;
        let cycles = self
            .api
            .tasks
            .cycles(p.workspace_id, project_id, p.actor_id)
            .await?;
        Ok(cycle_views(cycles, TimestampMillis::now()))
    }

    pub async fn statuses(
        &self,
        project_id: Id,
        cursor: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Page<StatusRecord>> {
        self.need(ApiTokenScope::TasksRead)?;
        let limit = page_limit(cursor, limit)?;
        if !self.approved(project_id) {
            return Err(TokenApiError::NotAvailable);
        }
        let p = self.principal;
        Ok(self
            .granted()
            .statuses(p.workspace_id, project_id, p.actor_id, cursor, limit)
            .await?)
    }

    pub async fn list_tasks(&self, query: TaskQuery) -> Result<Page<TaskView>> {
        self.need(ApiTokenScope::TasksRead)?;
        let (sort, order) = query
            .sort
            .clone()
            .unwrap_or((TaskSort::CreatedAt, SortOrder::Desc));
        let limit = query.limit.unwrap_or(50);
        if !(1..=100).contains(&limit)
            || query
                .query
                .as_ref()
                .is_some_and(|q| q.chars().count() > 500)
            || query.cursor.as_ref().is_some_and(|c| c.len() > 4096)
        {
            return Err(TokenApiError::Invalid(
                "Invalid query length, cursor, or limit",
            ));
        }
        if query
            .priority
            .as_deref()
            .is_some_and(|p| !PRIORITIES.contains(&p))
        {
            return Err(TokenApiError::Invalid("Invalid priority"));
        }
        let project_ids = match query.project_id {
            Some(id) if !self.approved(id) => return Err(TaskError::NotFound.into()),
            Some(id) => vec![id],
            None => self.principal.project_ids.clone(),
        };
        if let Some(parent) = query.parent_task_id {
            self.task(&parent.to_string()).await?;
        }
        let condition = |field, operator, value| {
            FilterNode::Condition(Condition {
                field,
                operator,
                value,
            })
        };
        let mut children = vec![condition(
            FilterField::Project,
            FilterOperator::Is,
            json!(
                project_ids
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
            ),
        )];
        if let Some(text) = query.query.filter(|q| !q.is_empty()) {
            children.push(condition(
                FilterField::Text,
                FilterOperator::Contains,
                json!(text),
            ));
        }
        if let Some(id) = query.status_id {
            children.push(condition(
                FilterField::Status,
                FilterOperator::Is,
                json!([id.to_string()]),
            ));
        }
        match query.assignee {
            Some(AssigneeFilter::User(id)) => children.push(condition(
                FilterField::Assignee,
                FilterOperator::Is,
                json!([id.to_string()]),
            )),
            Some(AssigneeFilter::Unassigned) => children.push(condition(
                FilterField::Assignee,
                FilterOperator::IsEmpty,
                Value::Null,
            )),
            None => {}
        }
        if let Some(id) = query.label_id {
            children.push(condition(
                FilterField::Label,
                FilterOperator::IncludesAny,
                json!([id.to_string()]),
            ));
        }
        if let Some(priority) = query.priority {
            children.push(condition(
                FilterField::Priority,
                FilterOperator::Is,
                json!([priority]),
            ));
        }
        match query.milestone {
            Some(MilestoneFilter::Milestone(id)) => children.push(condition(
                FilterField::Milestone,
                FilterOperator::Is,
                json!([id.to_string()]),
            )),
            Some(MilestoneFilter::Unset) => children.push(condition(
                FilterField::Milestone,
                FilterOperator::IsEmpty,
                Value::Null,
            )),
            None => {}
        }
        match query.cycle {
            Some(CycleFilter::Cycle(id)) => children.push(condition(
                FilterField::Cycle,
                FilterOperator::Is,
                json!([id.to_string()]),
            )),
            Some(CycleFilter::Relative(relative)) => children.push(condition(
                FilterField::Cycle,
                FilterOperator::Is,
                json!([relative.as_str()]),
            )),
            Some(CycleFilter::Unset) => children.push(condition(
                FilterField::Cycle,
                FilterOperator::IsEmpty,
                Value::Null,
            )),
            None => {}
        }
        match query.has_due_date {
            Some(true) => children.push(condition(
                FilterField::DueDate,
                FilterOperator::IsNotEmpty,
                Value::Null,
            )),
            Some(false) => children.push(condition(
                FilterField::DueDate,
                FilterOperator::IsEmpty,
                Value::Null,
            )),
            None => {}
        }
        if let Some(before) = query.due_before {
            children.push(condition(
                FilterField::DueDate,
                FilterOperator::Before,
                json!({ "instant": before.as_millis() }),
            ));
        }
        if let Some(after) = query.due_after {
            children.push(condition(
                FilterField::DueDate,
                FilterOperator::After,
                json!({ "instant": after.as_millis() }),
            ));
        }
        let filter = TaskFilter {
            tree: FilterGroup {
                op: GroupOp::And,
                children,
            },
            show_completed: ShowCompleted::All,
            sort: sort.clone(),
            order: order.clone(),
            parent_task_id: query.parent_task_id,
            sub_issues: SubIssuesDisplay::Flat,
        };
        let p = self.principal;
        let page = self
            .api
            .tasks
            .tasks(
                p.workspace_id,
                p.actor_id,
                &filter,
                query.cursor.as_deref(),
                limit,
                TimestampMillis::now(),
            )
            .await?;
        Ok(Page {
            items: self.views(page.items).await?,
            next_cursor: page.next_cursor,
        })
    }

    pub async fn get_task(&self, task_ref: &str) -> Result<TaskView> {
        self.need(ApiTokenScope::TasksRead)?;
        let task = self.task(task_ref).await?;
        self.view(task).await
    }

    /// Creates a task. With `import`, a task that an earlier write with the same `external_id`
    /// made is returned instead (`created` is false), so a restarted import does not repeat it.
    pub async fn create_task(&self, input: NewTask) -> Result<Written<TaskView>> {
        self.need(ApiTokenScope::TasksWrite)?;
        validate_task_input(
            Some(&input.title),
            Some(&input.description),
            Some(&input.priority),
        )?;
        if !self.approved(input.project_id) {
            return Err(TokenApiError::NotAvailable);
        }
        let source = self.import_input(input.import)?;
        if let Some(source) = &source {
            validate_import(
                source.created_at,
                source.updated_at,
                source.external_id.as_deref(),
            )?;
            if source
                .number
                .is_some_and(|number| !(1..=i64::from(i32::MAX)).contains(&number))
            {
                return Err(TokenApiError::Invalid("A task number is 1 to 2147483647"));
            }
            if let Some((id, _)) = self
                .external(EXTERNAL_TASK, source.external_id.as_deref())
                .await?
            {
                let task = self.task(&id.to_string()).await?;
                return Ok(Written {
                    value: self.view(task).await?,
                    created: false,
                });
            }
        }
        let p = self.principal;
        let create = CreateTask {
            project_id: input.project_id,
            status_id: input.status_id,
            title: input.title,
            description: input.description,
            description_json: None,
            source_url: None,
            priority: input.priority,
            position: None,
            assignee_ids: input.assignee_ids,
            label_ids: input.label_ids,
            due_start_at: input.due_start_at,
            due_at: input.due_at,
            parent_task_id: input.parent_task_id,
            milestone_id: input.milestone_id,
            cycle_id: input.cycle_id,
            estimate: input.estimate,
        };
        let (tasks, now) = (self.granted(), TimestampMillis::now());
        // An import token creates every task as an import: no notifications, no automation.
        let task = if self.importing() {
            let source = source.unwrap_or_default();
            tasks
                .import_task(
                    Id::new_v7(),
                    p.workspace_id,
                    p.actor_id,
                    create,
                    source,
                    self.request_id,
                    now,
                )
                .await?
        } else {
            tasks
                .create_task(p.workspace_id, p.actor_id, create, self.request_id, now)
                .await?
                .task
        };
        Ok(Written {
            value: self.view(task).await?,
            created: true,
        })
    }

    /// The import fields of a write: refused without `tasks:import`.
    fn import_input<T>(&self, import: Option<T>) -> Result<Option<T>> {
        match import {
            Some(_) if !self.importing() => Err(TokenApiError::MissingScope),
            import => Ok(import),
        }
    }

    /// The task or comment ([`EXTERNAL_TASK`], [`EXTERNAL_COMMENT`]) that holds `external_id`,
    /// with its task. One whose task is in the trash keeps the ID, so the write conflicts.
    async fn external(
        &self,
        sql: &'static str,
        external_id: Option<&str>,
    ) -> Result<Option<(Id, Id)>> {
        let Some(external_id) = external_id else {
            return Ok(None);
        };
        let row = sqlx::query(sql)
            .bind(self.principal.workspace_id.to_string())
            .bind(external_id)
            .fetch_optional(self.api.tasks.database().pool())
            .await?;
        match row {
            None => Ok(None),
            Some(row) if row.get::<Option<i64>, _>("deleted_at").is_some() => {
                Err(TaskError::Conflict.into())
            }
            Some(row) => {
                let id = |column: &str| {
                    row.get::<String, _>(column)
                        .parse::<Id>()
                        .map_err(|error| TokenApiError::Internal(error.to_string()))
                };
                Ok(Some((id("id")?, id("task_id")?)))
            }
        }
    }

    pub async fn update_task(
        &self,
        task_ref: &str,
        expected_version: u64,
        patch: TaskPatch,
    ) -> Result<TaskView> {
        self.need(ApiTokenScope::TasksWrite)?;
        validate_task_input(
            patch.title.as_deref(),
            patch.description.as_deref(),
            patch.priority.as_deref(),
        )?;
        let recurrence = patch
            .recurrence
            .map(|value| value.map(|recurrence| recurrence.rule()).transpose())
            .transpose()?;
        let p = self.principal;
        let id = self.task_id(task_ref).await?;
        if let Some(Some(original)) = patch.duplicate_of_id
            && self.task(&original.to_string()).await.is_err()
        {
            return Err(TaskError::Invalid {
                field: "duplicate_of_id",
            }
            .into());
        }
        let outcome = self
            .granted()
            .update_task(
                p.workspace_id,
                p.actor_id,
                &TaskUpdate {
                    id,
                    expected_version,
                    changes: TaskChanges {
                        title: patch.title,
                        description: patch.description,
                        status_id: patch.status_id,
                        priority: patch.priority,
                        assignee_ids: patch.assignee_ids,
                        label_ids: patch.label_ids,
                        due_start_at: patch.due_start_at,
                        due_at: patch.due_at,
                        parent_task_id: patch.parent_task_id,
                        recurrence,
                        milestone_id: patch.milestone_id,
                        cycle_id: patch.cycle_id,
                        estimate: patch.estimate,
                        duplicate_of_id: patch.duplicate_of_id,
                        ..Default::default()
                    },
                },
                self.request_id,
                TimestampMillis::now(),
            )
            .await?;
        self.view(outcome.task).await
    }

    /// Moves the task and its sub-issues to the trash; with `permanent`, deletes it forever,
    /// also from the trash (see [`TaskRepository::purge_task`]).
    pub async fn delete_task(
        &self,
        task_ref: &str,
        expected_version: u64,
        permanent: bool,
    ) -> Result<()> {
        self.need(ApiTokenScope::TasksWrite)?;
        let p = self.principal;
        let id = self.task_id(task_ref).await?;
        let (tasks, now) = (self.granted(), TimestampMillis::now());
        if !permanent {
            let deleted = tasks.delete_task(
                p.workspace_id,
                id,
                p.actor_id,
                expected_version,
                self.request_id,
                now,
            );
            return Ok(deleted.await?);
        }
        tasks
            .purge_task(
                p.workspace_id,
                id,
                p.actor_id,
                Some(expected_version),
                self.request_id,
                now,
            )
            .await
            .map_err(|error| match error {
                TaskError::Forbidden => TokenApiError::PurgeForbidden,
                error => error.into(),
            })
    }

    /// The id of a task by UUID or identifier (`ENG-12`), for every task operation: a value that
    /// is neither is invalid, an identifier of no task is not found.
    async fn task_id(&self, task_ref: &str) -> Result<Id> {
        let p = self.principal;
        match self
            .api
            .tasks
            .resolve_task_id(p.workspace_id, task_ref)
            .await
        {
            Err(TaskError::NotFound) if parse_task_identifier(task_ref).is_none() => {
                Err(TokenApiError::Invalid("Invalid identifier"))
            }
            result => Ok(result?),
        }
    }

    /// [`Self::task_id`] of an optional reference, such as a `parent_task_id` field.
    pub async fn optional_task_id(&self, task_ref: Option<&str>) -> Result<Option<Id>> {
        match task_ref {
            Some(task_ref) => self.task_id(task_ref).await.map(Some),
            None => Ok(None),
        }
    }

    /// A live task in an approved project, by UUID or identifier (`ENG-12`).
    async fn task(&self, task_ref: &str) -> Result<TaskRecord> {
        let p = self.principal;
        let id = self.task_id(task_ref).await?;
        let task = self
            .api
            .tasks
            .get_task(p.workspace_id, id, p.actor_id)
            .await?;
        if self.approved(task.project_id) {
            Ok(task)
        } else {
            Err(TaskError::NotFound.into())
        }
    }

    pub async fn comments(
        &self,
        task_ref: &str,
        cursor: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Page<CommentRecord>> {
        self.need(ApiTokenScope::TasksRead)?;
        let limit = page_limit(cursor, limit)?;
        let task = self.task(task_ref).await?;
        let p = self.principal;
        Ok(self
            .api
            .tasks
            .comments(p.workspace_id, task.id, p.actor_id, cursor, limit)
            .await?)
    }

    /// Comments on a task. With `import`, a comment that an earlier write with the same
    /// `external_id` made is returned instead (`created` is false).
    pub async fn create_comment(
        &self,
        task_ref: &str,
        comment: NewComment,
    ) -> Result<Written<CommentRecord>> {
        self.need(ApiTokenScope::TasksWrite)?;
        let mut body = comment.body;
        if body.trim().is_empty() || body.chars().count() > 100_000 || body.len() > 100_000 {
            return Err(TokenApiError::Invalid("Invalid comment body"));
        }
        let task = self.task(task_ref).await?;
        let p = self.principal;
        let mut source = ImportedComment::default();
        if let Some(import) = self.import_input(comment.import)? {
            source = import.source;
            validate_import(
                source.created_at,
                source.updated_at,
                source.external_id.as_deref(),
            )?;
            if let Some(name) = import.author_name {
                if source.author_id.is_some() {
                    return Err(TokenApiError::Invalid(
                        "Send author_id or author_name, not both",
                    ));
                }
                let name =
                    valid_name(&name.replace(['\r', '\n'], " "), 200, "Invalid author_name")?;
                body = format!("**{}**\n\n{}", escape_markdown(&name), body.trim());
            }
            if let Some((id, task_id)) = self
                .external(EXTERNAL_COMMENT, source.external_id.as_deref())
                .await?
            {
                // The ID names a comment of another task.
                if task_id != task.id {
                    return Err(TaskError::Conflict.into());
                }
                let value = self
                    .api
                    .tasks
                    .comment(p.workspace_id, task.id, id, p.actor_id)
                    .await?;
                return Ok(Written {
                    value,
                    created: false,
                });
            }
        }
        let value = self
            .granted()
            .import_comment(
                p.workspace_id,
                task.id,
                p.actor_id,
                comment.parent_id,
                CommentContent::Markdown {
                    body,
                    mentioned_user_ids: Vec::new(),
                },
                source,
                self.request_id,
                TimestampMillis::now(),
            )
            .await?;
        Ok(Written {
            value,
            created: true,
        })
    }

    /// Docs and chat act as the token's creator, so they need a personal token.
    fn content<'r, T>(
        &self,
        scope: ApiTokenScope,
        repository: &'r Option<Arc<T>>,
    ) -> Result<&'r Arc<T>> {
        self.need(scope)?;
        if self.principal.service_account.is_some() {
            return Err(TokenApiError::NotAvailable);
        }
        repository.as_ref().ok_or(TokenApiError::NotAvailable)
    }

    /// Each doc's `version` is its [`revision`], as in [`DocView`].
    pub async fn list_docs(&self) -> Result<PageList> {
        let pages = self.content(ApiTokenScope::DocsRead, &self.api.pages)?;
        let p = self.principal;
        let mut list = pages
            .pages(p.workspace_id, p.actor_id)
            .await
            .map_err(hidden_page)?;
        for page in &mut list.items {
            page.version = revision(page.version, page.updated_at);
        }
        Ok(list)
    }

    pub async fn get_doc(&self, page_id: Id) -> Result<DocView> {
        let pages = self.content(ApiTokenScope::DocsRead, &self.api.pages)?;
        let p = self.principal;
        let page = pages
            .get_page(p.workspace_id, page_id, p.actor_id)
            .await
            .map_err(hidden_page)?;
        self.doc_view(pages, page).await
    }

    /// The titles that the actor can see for the links of the doc `page_id`: the page tree and
    /// the doc's own sub-pages. A database row and its sub-pages are not in the tree, so a link
    /// from a row to its sub-pages would stay "Untitled" without the second list.
    async fn link_titles(
        &self,
        pages: &PageRepository,
        page_id: Id,
    ) -> Result<HashMap<String, String>> {
        let p = self.principal;
        let tree = pages
            .pages(p.workspace_id, p.actor_id)
            .await
            .map_err(hidden_page)?;
        let sub_pages = pages
            .descendants(p.workspace_id, page_id, p.actor_id)
            .await
            .map_err(hidden_page)?;
        Ok(tree
            .items
            .into_iter()
            .chain(sub_pages.items)
            .map(|summary| (summary.id.to_string(), summary.title))
            .collect())
    }

    async fn doc_view(&self, pages: &PageRepository, mut page: PageRecord) -> Result<DocView> {
        let markdown = self.markdown(pages, &page.content).await?;
        let values = self.row_values(&page).await?;
        page.version = revision(page.version, page.updated_at);
        Ok(DocView {
            page,
            markdown,
            values,
        })
    }

    /// `blocks` as Markdown, with links to the pages that the actor can see.
    async fn markdown(&self, pages: &PageRepository, blocks: &[Value]) -> Result<String> {
        let mut linked = HashSet::new();
        linked_pages(blocks, &mut linked);
        let mut titles = HashMap::new();
        if !linked.is_empty() {
            titles = self.link_titles(pages, page.id).await?;
            titles.retain(|id, _| linked.contains(id));
        }
        Ok(blocks_to_markdown(blocks, &DocLinks(titles)))
    }

    pub async fn create_doc(&self, input: NewDoc) -> Result<DocView> {
        let pages = self.content(ApiTokenScope::DocsWrite, &self.api.pages)?;
        validate_doc_input(
            Some(&input.title),
            input.icon.as_deref(),
            Some(&input.markdown),
        )?;
        let content = match input.markdown.trim() {
            "" => Vec::new(),
            _ => self.blocks(&input.markdown).await?,
        };
        if block_count(&content) > MAX_BLOCKS {
            return Err(PageError::TooLarge.into());
        }
        let p = self.principal;
        let page = pages
            .create_page_with_content(
                p.workspace_id,
                p.actor_id,
                CreatePage {
                    kind: crate::repositories::pages::PageKind::Page,
                    parent_id: input.parent_id,
                    space: SpaceRequest {
                        teamspace_id: input.teamspace_id,
                        private: input.private,
                    },
                    title: input.title,
                    icon: input.icon,
                    position: None,
                },
                &content,
                self.request_id,
                TimestampMillis::now(),
            )
            .await?;
        self.doc_view(pages, page).await
    }

    /// `expected_version` is the doc's `version` as read (its [`revision`]).
    pub async fn update_doc(
        &self,
        page_id: Id,
        expected_version: u64,
        patch: DocPatch,
    ) -> Result<DocView> {
        let pages = self.content(ApiTokenScope::DocsWrite, &self.api.pages)?;
        validate_doc_input(
            patch.title.as_deref(),
            patch.icon.as_ref().and_then(Option::as_deref),
            patch.markdown.as_deref(),
        )?;
        let p = self.principal;
        let changes = PageChanges {
            title: patch.title,
            icon: patch.icon,
            ..PageChanges::default()
        };
        let page = match patch.markdown {
            // Merged onto the current document with the links of `doc_view`, so the blocks the
            // Markdown leaves as read stay exactly as they are.
            Some(markdown) => {
                let titles = self.link_titles(pages, page_id).await?;
                let members = members(self.api.tasks.database().pool(), p.workspace_id).await?;
                pages
                    .update_page_markdown_at_revision(
                        p.workspace_id,
                        page_id,
                        p.actor_id,
                        expected_version,
                        changes,
                        MarkdownContent {
                            markdown,
                            members,
                            links: Arc::new(DocLinks(titles)),
                        },
                        self.request_id,
                        TimestampMillis::now(),
                    )
                    .await?
            }
            None => {
                pages
                    .update_page_at_revision(
                        p.workspace_id,
                        page_id,
                        p.actor_id,
                        expected_version,
                        changes,
                        self.request_id,
                        TimestampMillis::now(),
                    )
                    .await?
            }
        };
        self.doc_view(pages, page).await
    }

    /// Moves the doc and its sub-docs to the trash. `expected_version` is the doc's `version` as
    /// read (its [`revision`]).
    pub async fn delete_doc(&self, page_id: Id, expected_version: u64) -> Result<()> {
        let pages = self.content(ApiTokenScope::DocsWrite, &self.api.pages)?;
        let p = self.principal;
        pages
            .delete_page_at_revision(
                p.workspace_id,
                page_id,
                p.actor_id,
                expected_version,
                self.request_id,
                TimestampMillis::now(),
            )
            .await?;
        Ok(())
    }

    /// `expected_version` is the doc's `version` as read (its [`revision`]).
    pub async fn move_doc(
        &self,
        page_id: Id,
        expected_version: u64,
        target: DocMove,
    ) -> Result<MovedDocView> {
        let pages = self.content(ApiTokenScope::DocsWrite, &self.api.pages)?;
        let p = self.principal;
        let moved = pages
            .move_page_at_revision(
                p.workspace_id,
                page_id,
                p.actor_id,
                expected_version,
                target.parent_id,
                SpaceRequest {
                    teamspace_id: target.teamspace_id,
                    private: target.private,
                },
                target.position.map_or(i64::MAX, |position| {
                    i64::try_from(position).unwrap_or(i64::MAX)
                }),
                self.request_id,
                TimestampMillis::now(),
            )
            .await?;
        Ok(MovedDocView {
            doc: self.doc_view(pages, moved.page).await?,
            dropped_values: moved.dropped_values,
        })
    }

    /// The history of a doc, newest first, without content.
    pub async fn doc_versions(
        &self,
        page_id: Id,
        cursor: Option<&str>,
        limit: Option<usize>,
    ) -> Result<PageVersionList> {
        let pages = self.content(ApiTokenScope::DocsRead, &self.api.pages)?;
        let limit = page_limit(cursor, limit)?;
        let p = self.principal;
        pages
            .page_versions(p.workspace_id, page_id, p.actor_id, cursor, limit)
            .await
            .map_err(|error| match error {
                PageError::InvalidCursor => TokenApiError::Page(error),
                error => hidden_page(error),
            })
    }

    pub async fn doc_version(&self, page_id: Id, version_id: Id) -> Result<DocVersionView> {
        let pages = self.content(ApiTokenScope::DocsRead, &self.api.pages)?;
        let p = self.principal;
        let version = pages
            .page_version(p.workspace_id, page_id, version_id, p.actor_id)
            .await
            .map_err(hidden_page)?;
        let markdown = self.markdown(pages, &version.content).await?;
        Ok(DocVersionView { version, markdown })
    }

    /// Doc blocks from Markdown; `@Name` mentions of workspace members become mentions.
    async fn blocks(&self, markdown: &str) -> Result<Vec<Value>> {
        let database = self.api.tasks.database();
        let members = members(database.pool(), self.principal.workspace_id).await?;
        Ok(markdown_to_blocks(markdown, &members))
    }

    pub async fn conversations(&self) -> Result<Vec<ConversationRecord>> {
        let chat = self.content(ApiTokenScope::ChatRead, &self.api.chat)?;
        let p = self.principal;
        chat.list_conversations(p.workspace_id, p.actor_id)
            .await
            .map_err(hidden_chat)
    }

    pub async fn messages(
        &self,
        conversation_id: Id,
        before: Option<Id>,
        limit: Option<usize>,
    ) -> Result<MessagePage> {
        let chat = self.content(ApiTokenScope::ChatRead, &self.api.chat)?;
        let p = self.principal;
        chat.list_messages(
            p.workspace_id,
            p.actor_id,
            conversation_id,
            message_cursor(before, limit)?,
        )
        .await
        .map_err(hidden_chat)
    }

    pub async fn thread(
        &self,
        message_id: Id,
        before: Option<Id>,
        limit: Option<usize>,
    ) -> Result<ThreadPage> {
        let chat = self.content(ApiTokenScope::ChatRead, &self.api.chat)?;
        let p = self.principal;
        chat.get_thread(
            p.workspace_id,
            p.actor_id,
            message_id,
            message_cursor(before, limit)?,
        )
        .await
        .map_err(hidden_chat)
    }

    pub async fn search_messages(&self, input: SearchInput) -> Result<SearchPage> {
        let chat = self.content(ApiTokenScope::ChatRead, &self.api.chat)?;
        if input.query.chars().count() > 500 {
            return Err(TokenApiError::Invalid("Invalid query length"));
        }
        let p = self.principal;
        chat.search_messages(p.workspace_id, p.actor_id, input)
            .await
            .map_err(hidden_chat)
    }
}

/// Doc reads tell no refusal apart from a missing doc; a failing database stays an error.
fn hidden_page(error: PageError) -> TokenApiError {
    match error {
        PageError::Unavailable(error) => TokenApiError::Internal(error.to_string()),
        error @ PageError::Corrupt => TokenApiError::Internal(error.to_string()),
        _ => TokenApiError::NotAvailable,
    }
}

/// [`hidden_page`] for chat reads.
fn hidden_chat(error: ChatError) -> TokenApiError {
    match error {
        ChatError::Unavailable(error) => TokenApiError::Internal(error.to_string()),
        ChatError::Upload(error) => TokenApiError::Internal(error.to_string()),
        _ => TokenApiError::NotAvailable,
    }
}

fn page_limit(cursor: Option<&str>, limit: Option<usize>) -> Result<usize> {
    let limit = limit.unwrap_or(50);
    if !(1..=100).contains(&limit) || cursor.is_some_and(|c| c.len() > 4096) {
        return Err(TokenApiError::Invalid("Invalid cursor or limit"));
    }
    Ok(limit)
}

fn message_cursor(before: Option<Id>, limit: Option<usize>) -> Result<MessageCursor> {
    if limit.is_some_and(|n| !(1..=100).contains(&n)) {
        return Err(TokenApiError::Invalid("Invalid limit"));
    }
    Ok(MessageCursor {
        before,
        limit,
        ..Default::default()
    })
}

/// A trimmed name of 1 to `max` characters.
fn valid_name(name: &str, max: usize, invalid: &'static str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > max {
        return Err(TokenApiError::Invalid(invalid));
    }
    Ok(name.to_owned())
}

/// `#rrggbb` in lower case, or `default` when unset.
fn valid_color(color: Option<String>, default: &str) -> Result<String> {
    let color = color.unwrap_or_else(|| default.to_owned());
    if color.len() == 7
        && color.starts_with('#')
        && color[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        Ok(color.to_ascii_lowercase())
    } else {
        Err(TokenApiError::Invalid("A color is #rrggbb"))
    }
}

/// The import fields both tasks and comments take.
fn validate_import(
    created_at: Option<TimestampMillis>,
    updated_at: Option<TimestampMillis>,
    external_id: Option<&str>,
) -> Result<()> {
    if updated_at.is_some_and(|updated| created_at.is_none_or(|created| updated < created)) {
        return Err(TokenApiError::Invalid(
            "updated_at needs created_at and must not be before it",
        ));
    }
    if external_id.is_some_and(|id| id.trim().is_empty() || id.chars().count() > 200) {
        return Err(TokenApiError::Invalid(
            "An external_id has 1 to 200 characters",
        ));
    }
    Ok(())
}

fn validate_task_input(
    title: Option<&str>,
    description: Option<&str>,
    priority: Option<&str>,
) -> Result<()> {
    if title.is_some_and(|s| s.trim().is_empty() || s.chars().count() > 500)
        || description.is_some_and(|s| s.len() > 50_000)
        || priority.is_some_and(|s| !PRIORITIES.contains(&s))
    {
        return Err(TokenApiError::Invalid(
            "Invalid task title, description or priority",
        ));
    }
    Ok(())
}

/// The limits of the web editor for the title and icon; content up to 200,000 bytes of Markdown.
fn validate_doc_input(
    title: Option<&str>,
    icon: Option<&str>,
    markdown: Option<&str>,
) -> Result<()> {
    if title.is_some_and(|s| s.chars().count() > 500 || s.len() > 2000)
        || icon.is_some_and(|s| s.chars().count() > 64 || s.len() > 256)
        || markdown.is_some_and(|s| s.len() > 200_000)
    {
        return Err(TokenApiError::Invalid(
            "Invalid doc title, icon or markdown",
        ));
    }
    Ok(())
}
