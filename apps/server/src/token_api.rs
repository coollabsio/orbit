//! What a scoped credential (an API token today) may read and change. MCP tools and the public
//! REST API are thin layers over this module, so the token's limits live in one place: its
//! workspace, its approved projects, its scopes, the billing write guard, and no docs or chat
//! for service accounts.
use std::sync::Arc;

use orbit_platform::{Id, Row, TimestampMillis};
use serde::Serialize;
use serde_json::json;

use crate::billing::WorkspaceAccess;
use crate::repositories::api_tokens::{ApiTokenPrincipal, ApiTokenScope};
use crate::repositories::billing::write_access;
use crate::repositories::chat::{
    ChatRepository, ConversationRecord, MessageCursor, MessagePage, ThreadPage,
};
use crate::repositories::pages::{PageList, PageRecord, PageRepository};
use crate::repositories::task_filter::{
    Condition, FilterField, FilterGroup, FilterNode, FilterOperator, GroupOp, ShowCompleted,
    SubIssuesDisplay,
};
use crate::repositories::tasks::{
    CreateTask, Page, ProjectRecord, SortOrder, StatusRecord, TaskChanges, TaskError, TaskFilter,
    TaskRecord, TaskRepository, TaskSort, TaskUpdate, TokenActor, parse_task_identifier,
};

const PRIORITIES: [&str; 5] = ["none", "low", "medium", "high", "urgent"];

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
    Task(TaskError),
    Internal,
}

impl From<TaskError> for TokenApiError {
    fn from(error: TaskError) -> Self {
        Self::Task(error)
    }
}

impl From<sqlx::Error> for TokenApiError {
    fn from(_: sqlx::Error) -> Self {
        Self::Internal
    }
}

type Result<T> = std::result::Result<T, TokenApiError>;

/// A task without `TaskRecord`'s embedded parent, duplicate and ancestor references: those can
/// name tasks from projects outside the grant.
#[derive(Clone, Debug, Serialize)]
pub struct ReadTask {
    pub id: Id,
    pub project_id: Id,
    /// With the project key, the task identifier (`ENG-12`).
    pub number: i64,
    pub status_id: Id,
    pub title: String,
    pub description: String,
    pub priority: String,
    pub assignee_ids: Vec<Id>,
    pub label_ids: Vec<Id>,
    pub due_start_at: Option<TimestampMillis>,
    pub due_at: Option<TimestampMillis>,
    pub created_at: TimestampMillis,
    pub updated_at: TimestampMillis,
    pub version: u64,
}

/// [`ReadTask`] plus the parent's id while the parent is visible and in an approved project.
#[derive(Clone, Debug, Serialize)]
pub struct TaskView {
    #[serde(flatten)]
    pub task: ReadTask,
    pub parent_task_id: Option<Id>,
}

impl TaskView {
    fn new(task: TaskRecord, grant: &[Id]) -> Self {
        let parent_task_id = task
            .parent
            .as_ref()
            .filter(|parent| grant.contains(&parent.project_id))
            .map(|parent| parent.id);
        Self {
            task: ReadTask {
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
                created_at: task.created_at,
                updated_at: task.updated_at,
                version: task.version,
            },
            parent_task_id,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Me {
    pub token: TokenInfo,
    pub workspace: Named,
    /// Every workspace the token may use, with its access there. Always one item today.
    pub workspaces: Vec<WorkspaceGrant>,
    pub user: Option<UserInfo>,
    pub service_account: Option<Named>,
}

#[derive(Debug, Serialize)]
pub struct WorkspaceGrant {
    pub id: Id,
    pub name: String,
    pub scopes: Vec<&'static str>,
    pub project_ids: Vec<Id>,
}

#[derive(Debug, Serialize)]
pub struct TokenInfo {
    pub id: Id,
    pub name: String,
    pub scopes: Vec<&'static str>,
    pub project_ids: Vec<Id>,
    pub expires_at: Option<TimestampMillis>,
}

#[derive(Debug, Serialize)]
pub struct Named {
    pub id: Id,
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct UserInfo {
    pub id: Id,
    pub email: String,
    pub display_name: String,
}

#[derive(Debug, Default)]
pub struct TaskQuery {
    pub project_id: Option<Id>,
    pub query: Option<String>,
    pub status_id: Option<Id>,
    pub assignee_id: Option<Id>,
    pub label_id: Option<Id>,
    pub priority: Option<String>,
    pub cursor: Option<String>,
    pub limit: Option<usize>,
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
}

#[derive(Debug, Default)]
pub struct TaskPatch {
    pub title: Option<String>,
    pub description: Option<String>,
    pub status_id: Option<Id>,
    pub priority: Option<String>,
    pub assignee_ids: Option<Vec<Id>>,
    pub label_ids: Option<Vec<Id>>,
}

#[derive(Clone)]
pub struct TokenApi {
    tasks: Arc<TaskRepository>,
    pages: Option<Arc<PageRepository>>,
    chat: Option<Arc<ChatRepository>>,
}

impl TokenApi {
    #[must_use]
    pub fn new(tasks: Arc<TaskRepository>) -> Self {
        Self {
            tasks,
            pages: None,
            chat: None,
        }
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
        if scope == ApiTokenScope::TasksWrite {
            match write_access(self.tasks.database(), principal.workspace_id).await {
                Ok(WorkspaceAccess::ReadOnly) => return Err(TokenApiError::ReadOnly),
                Ok(_) => {}
                Err(_) => return Err(TokenApiError::BillingUnavailable),
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

    /// The task repository limited to the token's projects, attributing writes to the token.
    fn granted(&self) -> TaskRepository {
        self.api
            .tasks
            .with_project_grant(self.principal.project_ids.clone())
            .with_token(TokenActor {
                token_id: self.principal.token_id,
                service_account: self
                    .principal
                    .service_account
                    .as_ref()
                    .map(|account| (account.id, account.name.clone())),
            })
    }

    fn view(&self, task: TaskRecord) -> TaskView {
        TaskView::new(task, &self.principal.project_ids)
    }

    pub async fn projects(&self) -> Result<Vec<ProjectRecord>> {
        self.need(ApiTokenScope::TasksRead)?;
        let p = self.principal;
        // Apply the grant in SQL; never paginate across unapproved projects.
        Ok(self
            .api
            .tasks
            .approved_projects(p.workspace_id, p.creator_id, &p.project_ids)
            .await?)
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
            .statuses(p.workspace_id, project_id, p.creator_id, cursor, limit)
            .await?)
    }

    pub async fn list_tasks(&self, query: TaskQuery) -> Result<Page<TaskView>> {
        self.need(ApiTokenScope::TasksRead)?;
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
        if let Some(id) = query.assignee_id {
            children.push(condition(
                FilterField::Assignee,
                FilterOperator::Is,
                json!([id.to_string()]),
            ));
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
        let filter = TaskFilter {
            tree: FilterGroup {
                op: GroupOp::And,
                children,
            },
            show_completed: ShowCompleted::All,
            sort: TaskSort::CreatedAt,
            order: SortOrder::Desc,
            parent_task_id: None,
            sub_issues: SubIssuesDisplay::Flat,
        };
        let p = self.principal;
        let page = self
            .api
            .tasks
            .tasks(
                p.workspace_id,
                p.creator_id,
                &filter,
                query.cursor.as_deref(),
                limit,
                TimestampMillis::now(),
            )
            .await?;
        Ok(Page {
            items: page.items.into_iter().map(|task| self.view(task)).collect(),
            next_cursor: page.next_cursor,
        })
    }

    pub async fn get_task(&self, task_ref: &str) -> Result<TaskView> {
        self.need(ApiTokenScope::TasksRead)?;
        let p = self.principal;
        let id = match self
            .api
            .tasks
            .resolve_task_id(p.workspace_id, task_ref)
            .await
        {
            Ok(id) => id,
            // Neither a UUID nor an identifier.
            Err(TaskError::NotFound) if parse_task_identifier(task_ref).is_none() => {
                return Err(TokenApiError::Invalid("Invalid identifier"));
            }
            Err(error) => return Err(error.into()),
        };
        let task = self
            .api
            .tasks
            .get_task(p.workspace_id, id, p.creator_id)
            .await?;
        if self.approved(task.project_id) {
            Ok(self.view(task))
        } else {
            Err(TaskError::NotFound.into())
        }
    }

    pub async fn create_task(&self, input: NewTask) -> Result<TaskView> {
        self.need(ApiTokenScope::TasksWrite)?;
        validate_task_input(
            Some(&input.title),
            Some(&input.description),
            Some(&input.priority),
        )?;
        if !self.approved(input.project_id) {
            return Err(TokenApiError::NotAvailable);
        }
        let p = self.principal;
        let outcome = self
            .granted()
            .create_task(
                p.workspace_id,
                p.creator_id,
                CreateTask {
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
                    due_start_at: None,
                    due_at: None,
                    parent_task_id: input.parent_task_id,
                },
                self.request_id,
                TimestampMillis::now(),
            )
            .await?;
        Ok(self.view(outcome.task))
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
        let p = self.principal;
        let id = self
            .api
            .tasks
            .resolve_task_id(p.workspace_id, task_ref)
            .await?;
        let outcome = self
            .granted()
            .update_task(
                p.workspace_id,
                p.creator_id,
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
                        ..Default::default()
                    },
                },
                self.request_id,
                TimestampMillis::now(),
            )
            .await?;
        Ok(self.view(outcome.task))
    }

    /// Moves the task and its sub-issues to the trash.
    pub async fn delete_task(&self, task_ref: &str, expected_version: u64) -> Result<()> {
        self.need(ApiTokenScope::TasksWrite)?;
        let p = self.principal;
        let id = self
            .api
            .tasks
            .resolve_task_id(p.workspace_id, task_ref)
            .await?;
        self.granted()
            .delete_task(
                p.workspace_id,
                id,
                p.creator_id,
                expected_version,
                self.request_id,
                TimestampMillis::now(),
            )
            .await?;
        Ok(())
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

    pub async fn list_docs(&self) -> Result<PageList> {
        let pages = self.content(ApiTokenScope::DocsRead, &self.api.pages)?;
        let p = self.principal;
        pages
            .pages(p.workspace_id, p.creator_id)
            .await
            .map_err(|_| TokenApiError::NotAvailable)
    }

    pub async fn get_doc(&self, page_id: Id) -> Result<PageRecord> {
        let pages = self.content(ApiTokenScope::DocsRead, &self.api.pages)?;
        let p = self.principal;
        pages
            .get_page(p.workspace_id, page_id, p.creator_id)
            .await
            .map_err(|_| TokenApiError::NotAvailable)
    }

    pub async fn conversations(&self) -> Result<Vec<ConversationRecord>> {
        let chat = self.content(ApiTokenScope::ChatRead, &self.api.chat)?;
        let p = self.principal;
        chat.list_conversations(p.workspace_id, p.creator_id)
            .await
            .map_err(|_| TokenApiError::NotAvailable)
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
            p.creator_id,
            conversation_id,
            message_cursor(before, limit)?,
        )
        .await
        .map_err(|_| TokenApiError::NotAvailable)
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
            p.creator_id,
            message_id,
            message_cursor(before, limit)?,
        )
        .await
        .map_err(|_| TokenApiError::NotAvailable)
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
