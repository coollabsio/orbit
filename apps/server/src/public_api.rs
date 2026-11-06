//! The public REST API at `/api/public/v1` for API tokens (`Authorization: Bearer orb_…`).
//! Every rule lives in [`crate::token_api`]; this module parses requests, picks the public
//! fields of each record and writes problem+json errors. It takes no cookies or sessions.
use std::sync::Arc;

use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::{FromRequestParts, OriginalUri, Path, Query, RawPathParams, Request, State};
use axum::http::request::Parts;
use axum::http::{HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use orbit_platform::{Id, Problem, RequestId, TimestampMillis};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::{IntoParams, ToSchema};

use crate::billing::guard::READ_ONLY_DETAIL;
use crate::repositories::api_tokens::{
    ApiTokenError, ApiTokenPrincipal, ApiTokenRepository, ApiTokenScope,
};
use crate::repositories::pages::PageSummary;
use crate::repositories::pages::{PageError, PageRepository};
use crate::repositories::tasks::{
    CommentRecord, LabelRecord, ProjectRecord, SortOrder, StatusRecord, TaskError, TaskRepository,
    TaskSort,
};
use crate::token_api::{
    AssigneeFilter, DOC_TOO_LARGE_DETAIL, DocPatch, DocView, Me, MemberView, Named, NewDoc,
    NewTask, TaskPatch, TaskQuery, TaskView, TokenApi, TokenApiError,
};

pub const BASE_PATH: &str = "/api/public/v1";

#[derive(Clone)]
pub struct PublicApiState {
    tokens: Arc<ApiTokenRepository>,
    api: TokenApi,
}

impl PublicApiState {
    #[must_use]
    /// `web_origin` is the public origin of the web app, for the `url` of each task.
    pub fn new(
        tokens: Arc<ApiTokenRepository>,
        tasks: Arc<TaskRepository>,
        web_origin: &str,
    ) -> Self {
        Self {
            tokens,
            api: TokenApi::new(tasks).with_web_origin(web_origin),
        }
    }

    #[must_use]
    pub fn with_pages(mut self, pages: Arc<PageRepository>) -> Self {
        self.api = self.api.with_pages(pages);
        self
    }
}

pub fn router(state: PublicApiState) -> Router {
    // Workspace paths leave room for tokens with more than one workspace; today the path must
    // name the token's only workspace.
    let workspace = Router::new()
        .route("/projects", get(projects))
        .route("/projects/{project_id}/statuses", get(statuses))
        .route("/projects/{project_id}/labels", get(labels))
        .route("/members", get(members))
        .route("/tasks", get(list_tasks).post(create_task))
        .route(
            "/tasks/{task}",
            get(get_task).patch(update_task).delete(delete_task),
        )
        .route("/tasks/{task}/comments", get(comments).post(create_comment))
        .route("/docs", get(docs).post(create_doc))
        .route("/docs/{page_id}", get(doc).patch(update_doc));
    let routes = Router::new()
        .route("/me", get(me))
        .nest("/workspaces/{workspace_id}", workspace)
        .route_layer(middleware::from_fn_with_state(state.clone(), authenticate))
        .with_state(state);
    Router::new().nest(BASE_PATH, routes)
}

async fn authenticate(
    State(state): State<PublicApiState>,
    mut request: Request,
    next: Next,
) -> Response {
    let ctx = Responder::new(request.extensions(), request.uri());
    let mut headers = request.headers().get_all(header::AUTHORIZATION).iter();
    let token = headers
        .next()
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split_once(' '))
        .filter(|(scheme, token)| scheme.eq_ignore_ascii_case("bearer") && !token.is_empty())
        .map(|(_, token)| token.trim().to_owned());
    if headers.next().is_some() {
        return ctx.unauthorized();
    }
    let Some(token) = token else {
        return ctx.unauthorized();
    };
    match state
        .tokens
        .authenticate(&token, None, TimestampMillis::now())
        .await
    {
        Ok(principal) => {
            request.extensions_mut().insert(principal);
            next.run(request).await
        }
        Err(ApiTokenError::NotFound) => ctx.unauthorized(),
        Err(ApiTokenError::Forbidden) => ctx.error(TokenApiError::MissingScope),
        Err(_) => ctx.error(TokenApiError::Internal),
    }
}

/// The request facts that every response needs.
struct Responder {
    request_id: String,
    instance: String,
}

/// An authenticated request.
struct Ctx {
    principal: ApiTokenPrincipal,
    out: Responder,
}

impl std::ops::Deref for Ctx {
    type Target = Responder;

    fn deref(&self) -> &Responder {
        &self.out
    }
}

impl<S: Send + Sync> FromRequestParts<S> for Ctx {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Response> {
        let out = Responder::new(&parts.extensions, &parts.uri);
        let Some(principal) = parts.extensions.get::<ApiTokenPrincipal>().cloned() else {
            return Err(out.error(TokenApiError::Internal));
        };
        let params = RawPathParams::from_request_parts(parts, state)
            .await
            .map_err(|_| out.error(TokenApiError::Internal))?;
        if params.iter().any(|(name, value)| {
            name == "workspace_id" && value.parse::<Id>().ok() != Some(principal.workspace_id)
        }) {
            return Err(out.error(TokenApiError::NotAvailable));
        }
        Ok(Self { principal, out })
    }
}

impl Responder {
    fn new(extensions: &axum::http::Extensions, uri: &axum::http::Uri) -> Self {
        Self {
            request_id: extensions
                .get::<RequestId>()
                .map_or_else(|| "unknown".to_owned(), |id| id.as_str().to_owned()),
            instance: extensions
                .get::<OriginalUri>()
                .map_or(uri, |original| &original.0)
                .path()
                .to_owned(),
        }
    }

    fn reply<T: Serialize>(
        &self,
        status: StatusCode,
        result: Result<T, TokenApiError>,
    ) -> Response {
        match result {
            Ok(value) => (status, Json(value)).into_response(),
            Err(error) => self.error(error),
        }
    }

    fn problem(&self, status: StatusCode, code: &str, title: &str, detail: &str) -> Problem {
        Problem {
            type_uri: format!("https://docs.orbit.dev/problems/{code}"),
            title: title.to_owned(),
            status: status.as_u16(),
            code: code.to_owned(),
            detail: detail.to_owned(),
            instance: self.instance.clone(),
            request_id: self.request_id.clone(),
            errors: None,
            conflict: None,
        }
    }

    fn unauthorized(&self) -> Response {
        let mut response = problem_response(self.problem(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "Unauthorized",
            "Send a valid, unexpired API token as `Authorization: Bearer <token>`.",
        ));
        response.headers_mut().insert(
            header::WWW_AUTHENTICATE,
            HeaderValue::from_static("Bearer realm=\"orbit\""),
        );
        response
    }

    fn not_found(&self) -> Problem {
        self.problem(
            StatusCode::NOT_FOUND,
            "not_found",
            "Not found",
            "The resource does not exist or this token cannot use it.",
        )
    }

    fn invalid(&self, detail: &str) -> Response {
        problem_response(self.problem(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "Invalid request",
            detail,
        ))
    }

    fn error(&self, error: TokenApiError) -> Response {
        let problem = match error {
            TokenApiError::MissingScope => self.problem(
                StatusCode::FORBIDDEN,
                "insufficient_scope",
                "Insufficient scope",
                "The API token does not have the scope that this request needs.",
            ),
            TokenApiError::ReadOnly => self.problem(
                StatusCode::PAYMENT_REQUIRED,
                "workspace_read_only",
                "Workspace is read-only",
                READ_ONLY_DETAIL,
            ),
            TokenApiError::BillingUnavailable => self.problem(
                StatusCode::SERVICE_UNAVAILABLE,
                "service_unavailable",
                "Service unavailable",
                "Billing is unavailable. Try again later.",
            ),
            TokenApiError::Invalid(detail) => return self.invalid(detail),
            TokenApiError::NotAvailable => self.not_found(),
            TokenApiError::PurgeForbidden => self.problem(
                StatusCode::FORBIDDEN,
                "workspace_action_forbidden",
                "Workspace action forbidden",
                "Only workspace owners and administrators may delete tasks forever.",
            ),
            TokenApiError::Internal => Problem::internal(&self.request_id, &self.instance),
            TokenApiError::Task(error) => self.task_problem(error),
            TokenApiError::Page(error) => self.page_problem(error),
        };
        problem_response(problem)
    }

    fn task_problem(&self, error: TaskError) -> Problem {
        let (status, code, title, detail) = match error {
            TaskError::NotFound | TaskError::Forbidden => return self.not_found(),
            TaskError::Invalid { field } => {
                return Problem::validation(
                    &self.request_id,
                    &self.instance,
                    [(field, "The value is invalid for this workspace or project.")],
                );
            }
            // `current` is a full task record with relations; it can name unapproved projects.
            TaskError::VersionConflict { current } => {
                return Problem::conflict(
                    &self.request_id,
                    &self.instance,
                    "The resource changed after it was read. Read it again and retry.",
                    current.get("version").and_then(Value::as_i64).unwrap_or(0),
                    None,
                );
            }
            TaskError::TrialLimit(reached) => {
                let detail = reached.detail();
                return self.problem(
                    StatusCode::PAYMENT_REQUIRED,
                    "trial_limit_reached",
                    "Trial limit reached",
                    &detail,
                );
            }
            TaskError::InvalidCursor => (
                StatusCode::BAD_REQUEST,
                "invalid_cursor",
                "Invalid cursor",
                "The cursor does not match this list query.",
            ),
            TaskError::InvalidFilter { .. } => (
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "Invalid request",
                "The task filter is invalid.",
            ),
            TaskError::Conflict
            | TaskError::IntegrationConflict
            | TaskError::RestoreConflict { .. } => (
                StatusCode::CONFLICT,
                "task_conflict",
                "Task conflict",
                "The task operation conflicts with the current state.",
            ),
            TaskError::GithubContentReadOnly => (
                StatusCode::CONFLICT,
                "github_content_read_only",
                "GitHub content is read-only",
                "GitHub controls this task's title and description.",
            ),
            TaskError::GithubLinkedMove => (
                StatusCode::CONFLICT,
                "github_linked_move",
                "Task is synced with GitHub",
                "A task synced with a GitHub issue cannot move to another project.",
            ),
            TaskError::ParentCycle => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "parent_cycle",
                "Invalid parent",
                "A task cannot be its own parent or a sub-issue of its own sub-issues.",
            ),
            TaskError::ParentInvalid => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "parent_invalid",
                "Invalid parent",
                "The parent task does not exist or is in the trash.",
            ),
            TaskError::Busy => (
                StatusCode::SERVICE_UNAVAILABLE,
                "task_busy",
                "Task busy",
                "The task's description is busy on the server. Try again in a moment.",
            ),
            TaskError::Unavailable(_) => {
                return Problem::internal(&self.request_id, &self.instance);
            }
        };
        self.problem(status, code, title, detail)
    }

    fn page_problem(&self, error: PageError) -> Problem {
        let (status, code, title, detail) = match error {
            PageError::Invalid { field } => {
                return Problem::validation(
                    &self.request_id,
                    &self.instance,
                    [(field, "The value is invalid for this workspace.")],
                );
            }
            PageError::TeamspaceNotFound => {
                return Problem::validation(
                    &self.request_id,
                    &self.instance,
                    [(
                        "teamspace_id",
                        "The teamspace does not exist or is not visible.",
                    )],
                );
            }
            PageError::VersionConflict { current } => {
                return Problem::conflict(
                    &self.request_id,
                    &self.instance,
                    "The resource changed after it was read. Read it again and retry.",
                    current.get("version").and_then(Value::as_i64).unwrap_or(0),
                    None,
                );
            }
            PageError::TrialLimit(reached) => {
                let detail = reached.detail();
                return self.problem(
                    StatusCode::PAYMENT_REQUIRED,
                    "trial_limit_reached",
                    "Trial limit reached",
                    &detail,
                );
            }
            PageError::Locked => (
                StatusCode::LOCKED,
                "page_locked",
                "Doc locked",
                "The doc is locked. Unlock it in Orbit to change its title, icon or content.",
            ),
            PageError::Busy => (
                StatusCode::SERVICE_UNAVAILABLE,
                "page_busy",
                "Doc busy",
                "The doc's content is busy on the server. Try again in a moment.",
            ),
            PageError::TooLarge => (
                StatusCode::PAYLOAD_TOO_LARGE,
                "doc_too_large",
                "Doc too large",
                DOC_TOO_LARGE_DETAIL,
            ),
            PageError::Unavailable(_) | PageError::Corrupt => {
                return Problem::internal(&self.request_id, &self.instance);
            }
            _ => return self.not_found(),
        };
        self.problem(status, code, title, detail)
    }
}

fn problem_response(problem: Problem) -> Response {
    let status = StatusCode::from_u16(problem.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (
        status,
        [(header::CONTENT_TYPE, "application/problem+json")],
        serde_json::to_vec(&problem).expect("Problem serialization cannot fail"),
    )
        .into_response()
}

#[derive(Serialize, ToSchema)]
struct List<T> {
    items: Vec<T>,
    /// Send it as `cursor` for the next page; `null` on the last page.
    #[schema(required = true)]
    next_cursor: Option<String>,
}

fn list<T, U: From<T>>(items: Vec<T>, next_cursor: Option<String>) -> List<U> {
    List {
        items: items.into_iter().map(U::from).collect(),
        next_cursor,
    }
}

#[derive(Serialize, ToSchema)]
#[schema(as = PublicProject)]
struct ProjectOut {
    #[schema(value_type = String)]
    id: Id,
    name: String,
    key: String,
    color: String,
    version: u64,
    #[schema(value_type = String, format = DateTime)]
    created_at: TimestampMillis,
    #[schema(value_type = String, format = DateTime)]
    updated_at: TimestampMillis,
}

impl From<ProjectRecord> for ProjectOut {
    fn from(p: ProjectRecord) -> Self {
        Self {
            id: p.id,
            name: p.name,
            key: p.key,
            color: p.color,
            version: p.version,
            created_at: p.created_at,
            updated_at: p.updated_at,
        }
    }
}

#[derive(Serialize, ToSchema)]
#[schema(as = PublicStatus)]
struct StatusOut {
    #[schema(value_type = String)]
    id: Id,
    #[schema(value_type = String)]
    project_id: Id,
    name: String,
    description: String,
    color: String,
    /// `unstarted`, `started`, `completed`, `cancelled` or `duplicate`.
    category: String,
    position: i64,
    version: u64,
}

impl From<StatusRecord> for StatusOut {
    fn from(s: StatusRecord) -> Self {
        Self {
            id: s.id,
            project_id: s.project_id,
            name: s.name,
            description: s.description,
            color: s.color,
            category: s.category,
            position: s.position,
            version: s.version,
        }
    }
}

#[derive(Serialize, ToSchema)]
#[schema(as = PublicLabel)]
struct LabelOut {
    #[schema(value_type = String)]
    id: Id,
    #[schema(value_type = String)]
    project_id: Id,
    name: String,
    color: String,
    version: u64,
}

impl From<LabelRecord> for LabelOut {
    fn from(l: LabelRecord) -> Self {
        Self {
            id: l.id,
            project_id: l.project_id,
            name: l.name,
            color: l.color,
            version: l.version,
        }
    }
}

#[derive(Serialize, ToSchema)]
#[schema(as = PublicComment)]
struct CommentOut {
    #[schema(value_type = String)]
    id: Id,
    #[schema(value_type = String)]
    task_id: Id,
    #[schema(value_type = String)]
    author_id: Id,
    /// Set when a service account wrote the comment; show it as the author then.
    #[schema(required = true)]
    author_service_account: Option<Named>,
    /// Markdown.
    body: String,
    #[schema(value_type = Option<String>, required = true)]
    parent_id: Option<Id>,
    version: u64,
    #[schema(value_type = String, format = DateTime)]
    created_at: TimestampMillis,
    #[schema(value_type = String, format = DateTime)]
    updated_at: TimestampMillis,
}

impl From<CommentRecord> for CommentOut {
    fn from(c: CommentRecord) -> Self {
        Self {
            id: c.id,
            task_id: c.task_id,
            author_id: c.author_id,
            author_service_account: c.author_service_account_id.map(|id| Named {
                id,
                name: c.author_service_account_name.unwrap_or_default(),
            }),
            body: c.body,
            parent_id: c.parent_id,
            version: c.version,
            created_at: c.created_at,
            updated_at: c.updated_at,
        }
    }
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
#[serde(deny_unknown_fields)]
struct PageQuery {
    /// The `next_cursor` of the previous page.
    cursor: Option<String>,
    /// 1 to 100; 50 when unset.
    #[param(minimum = 1, maximum = 100)]
    limit: Option<usize>,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
#[serde(deny_unknown_fields)]
struct TasksQuery {
    #[param(value_type = Option<String>)]
    project_id: Option<Id>,
    #[param(value_type = Option<String>)]
    status_id: Option<Id>,
    /// A member's user ID, or `none` for tasks without an assignee.
    assignee_id: Option<String>,
    /// `created_at` (default), `updated_at`, `priority`, `due_date` or `title`.
    sort: Option<String>,
    /// `desc` (default) or `asc`. With `sort=priority`, `desc` lists `urgent` first.
    order: Option<String>,
    #[param(value_type = Option<String>)]
    label_id: Option<Id>,
    /// `none`, `low`, `medium`, `high` or `urgent`.
    priority: Option<String>,
    /// Only the direct sub-issues of this task (a UUID or an identifier such as `ENG-12`), which
    /// must be in an approved project.
    parent_task_id: Option<String>,
    /// Text in the title or description, or a task identifier; up to 500 characters.
    q: Option<String>,
    /// The `next_cursor` of the previous page. Keep the other parameters the same.
    cursor: Option<String>,
    /// 1 to 100; 50 when unset.
    #[param(minimum = 1, maximum = 100)]
    limit: Option<usize>,
}

#[derive(Deserialize, ToSchema)]
#[schema(as = PublicCreateTaskBody)]
#[serde(deny_unknown_fields)]
struct CreateTaskBody {
    #[schema(value_type = String)]
    project_id: Id,
    /// A status of the task's project.
    #[schema(value_type = String)]
    status_id: Id,
    /// 1 to 500 characters.
    title: String,
    /// Markdown, up to 50,000 bytes.
    #[serde(default)]
    description: String,
    /// `none`, `low`, `medium`, `high` or `urgent`.
    #[serde(default = "default_priority")]
    #[schema(default = "none")]
    priority: String,
    /// Members of the workspace.
    #[serde(default)]
    #[schema(value_type = Vec<String>)]
    assignee_ids: Vec<Id>,
    /// Labels of the task's project.
    #[serde(default)]
    #[schema(value_type = Vec<String>)]
    label_ids: Vec<Id>,
    /// A task in an approved project: a UUID or an identifier such as `ENG-12`.
    parent_task_id: Option<String>,
    /// Needs `due_at` and must not be after it.
    #[schema(value_type = Option<String>, format = DateTime)]
    due_start_at: Option<TimestampMillis>,
    #[schema(value_type = Option<String>, format = DateTime)]
    due_at: Option<TimestampMillis>,
}

fn default_priority() -> String {
    "none".to_owned()
}

/// A missing field keeps its value.
#[derive(Deserialize, ToSchema)]
#[schema(as = PublicUpdateTaskBody)]
#[serde(deny_unknown_fields)]
struct UpdateTaskBody {
    /// The `version` that you read: an opaque number below 2^53. Compare it for equality only.
    expected_version: u64,
    /// 1 to 500 characters.
    title: Option<String>,
    /// Markdown, up to 50,000 bytes.
    description: Option<String>,
    #[schema(value_type = Option<String>)]
    status_id: Option<Id>,
    /// `none`, `low`, `medium`, `high` or `urgent`.
    priority: Option<String>,
    /// Replaces the assignees.
    #[schema(value_type = Option<Vec<String>>)]
    assignee_ids: Option<Vec<Id>>,
    /// Replaces the labels.
    #[schema(value_type = Option<Vec<String>>)]
    label_ids: Option<Vec<Id>>,
    /// `null` clears the date.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = Option<String>, format = DateTime)]
    due_start_at: Option<Option<TimestampMillis>>,
    /// `null` clears the date.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = Option<String>, format = DateTime)]
    due_at: Option<Option<TimestampMillis>>,
    /// A task in an approved project (a UUID or an identifier such as `ENG-12`) makes this task
    /// its sub-issue; `null` detaches it.
    #[serde(default, deserialize_with = "present")]
    parent_task_id: Option<Option<String>>,
}

/// A field that is present, `null` included: `null` clears the value, a missing field keeps it.
fn present<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
) -> Result<Option<Option<T>>, D::Error> {
    Option::<T>::deserialize(deserializer).map(Some)
}

#[derive(Deserialize, ToSchema)]
#[schema(as = PublicCommentBody)]
#[serde(deny_unknown_fields)]
struct CommentBody {
    /// Markdown, 1 to 100,000 bytes in UTF-8, not only spaces.
    body: String,
}

/// Without `parent_id`, `teamspace_id` or `private`, the doc goes to the root of the default
/// teamspace.
#[derive(Deserialize, ToSchema)]
#[schema(as = PublicCreateDocBody)]
#[serde(deny_unknown_fields)]
struct CreateDocBody {
    /// The doc goes under this doc, in its space.
    #[schema(value_type = Option<String>)]
    parent_id: Option<Id>,
    /// Root docs only: the teamspace of the doc.
    #[schema(value_type = Option<String>)]
    teamspace_id: Option<Id>,
    /// Root docs only: `true` puts the doc in the token creator's private space.
    private: Option<bool>,
    /// Up to 500 characters.
    #[serde(default)]
    title: String,
    /// Up to 64 characters.
    icon: Option<String>,
    /// GitHub-flavored Markdown, up to 200,000 bytes and 4,000 blocks (`413 doc_too_large`).
    #[serde(default)]
    markdown: String,
}

/// A missing field keeps its value.
#[derive(Deserialize, ToSchema)]
#[schema(as = PublicUpdateDocBody)]
#[serde(deny_unknown_fields)]
struct UpdateDocBody {
    /// The `version` that you read: an opaque number below 2^53. Compare it for equality only.
    expected_version: u64,
    /// Up to 500 characters.
    title: Option<String>,
    /// Up to 64 characters; `null` clears the icon.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = Option<String>)]
    icon: Option<Option<String>>,
    /// The new content as GitHub-flavored Markdown, up to 200,000 bytes. It is merged onto the
    /// current content: blocks left as read stay exactly as they are, and edited blocks keep the
    /// colors, alignment and other settings that Markdown cannot show. The doc can have at most
    /// 4,000 blocks before and after the write (`413 doc_too_large`).
    markdown: Option<String>,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
#[serde(deny_unknown_fields)]
struct DeleteQuery {
    /// The `version` that you read: an opaque number below 2^53. Compare it for equality only.
    /// Not checked when `permanent=true` deletes a task that is already in the trash.
    expected_version: u64,
    /// `true` deletes the task and its sub-issues forever instead of moving them to the trash,
    /// also when the task is already in the trash. Needs a token of a workspace owner or admin,
    /// or a service account.
    #[serde(default)]
    permanent: bool,
}

/// Reads a query string or JSON body, or answers 400. The scope check comes first, so a token
/// without the endpoint's scope gets 403 whatever it sends.
macro_rules! input {
    ($ctx:expr, $scope:expr, $value:expr) => {{
        if !$ctx.principal.allows($scope) {
            return $ctx.error(TokenApiError::MissingScope);
        }
        match $value {
            Ok(value) => value.0,
            // The parser's text can quote the request; answer with a fixed message.
            Err(_) => {
                return $ctx.invalid(
                    "The query string or JSON body is invalid. Check the field names, types and required fields.",
                )
            }
        }
    }};
}

/// A path id that does not parse names no resource.
macro_rules! path_id {
    ($ctx:expr, $raw:expr) => {
        match $raw.parse::<Id>() {
            Ok(id) => id,
            Err(_) => return problem_response($ctx.not_found()),
        }
    };
}

#[utoipa::path(
    get,
    path = "/api/public/v1/me",
    operation_id = "public_get_me",
    tag = "public-api",
    summary = "The token, its workspace and its identity",
    responses((status = 200, description = "The token", body = Me)),
    security(("bearerAuth" = []))
)]
async fn me(State(s): State<PublicApiState>, ctx: Ctx) -> Response {
    ctx.reply(StatusCode::OK, s.api.me(&ctx.principal).await)
}

#[utoipa::path(
    get,
    path = "/api/public/v1/workspaces/{workspace_id}/projects",
    operation_id = "public_list_projects",
    tag = "public-api",
    summary = "The approved live projects",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`.")),
    responses((status = 200, description = "Every approved project; `next_cursor` is `null`", body = List<ProjectOut>)),
    security(("bearerAuth" = []))
)]
async fn projects(State(s): State<PublicApiState>, ctx: Ctx) -> Response {
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::TasksRead, &ctx.request_id)
            .await?;
        Ok(list::<_, ProjectOut>(api.projects().await?, None))
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    get,
    path = "/api/public/v1/workspaces/{workspace_id}/projects/{project_id}/statuses",
    operation_id = "public_list_statuses",
    tag = "public-api",
    summary = "The statuses of a project",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("project_id" = String, Path), PageQuery),
    responses((status = 200, description = "A page of statuses", body = List<StatusOut>)),
    security(("bearerAuth" = []))
)]
async fn statuses(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, project)): Path<(String, String)>,
    query: Result<Query<PageQuery>, QueryRejection>,
) -> Response {
    let project = path_id!(ctx, project);
    let query = input!(ctx, ApiTokenScope::TasksRead, query);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::TasksRead, &ctx.request_id)
            .await?;
        let page = api
            .statuses(project, query.cursor.as_deref(), query.limit)
            .await?;
        Ok(list::<_, StatusOut>(page.items, page.next_cursor))
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    get,
    path = "/api/public/v1/workspaces/{workspace_id}/projects/{project_id}/labels",
    operation_id = "public_list_labels",
    tag = "public-api",
    summary = "The labels of a project",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("project_id" = String, Path), PageQuery),
    responses((status = 200, description = "A page of labels", body = List<LabelOut>)),
    security(("bearerAuth" = []))
)]
async fn labels(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, project)): Path<(String, String)>,
    query: Result<Query<PageQuery>, QueryRejection>,
) -> Response {
    let project = path_id!(ctx, project);
    let query = input!(ctx, ApiTokenScope::TasksRead, query);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::TasksRead, &ctx.request_id)
            .await?;
        let page = api
            .labels(project, query.cursor.as_deref(), query.limit)
            .await?;
        Ok(list::<_, LabelOut>(page.items, page.next_cursor))
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    get,
    path = "/api/public/v1/workspaces/{workspace_id}/members",
    operation_id = "public_list_members",
    tag = "public-api",
    summary = "The workspace members",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), PageQuery),
    responses((status = 200, description = "A page of members", body = List<MemberView>)),
    security(("bearerAuth" = []))
)]
async fn members(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    query: Result<Query<PageQuery>, QueryRejection>,
) -> Response {
    let query = input!(ctx, ApiTokenScope::TasksRead, query);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::TasksRead, &ctx.request_id)
            .await?;
        api.members(query.cursor.as_deref(), query.limit).await
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    get,
    path = "/api/public/v1/workspaces/{workspace_id}/tasks",
    operation_id = "public_list_tasks",
    tag = "public-api",
    summary = "Tasks in the approved projects",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), TasksQuery),
    responses((status = 200, description = "A page of tasks", body = List<TaskView>)),
    security(("bearerAuth" = []))
)]
async fn list_tasks(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    query: Result<Query<TasksQuery>, QueryRejection>,
) -> Response {
    let query = input!(ctx, ApiTokenScope::TasksRead, query);
    let assignee = match query.assignee_id.as_deref() {
        None => None,
        Some("none") => Some(AssigneeFilter::Unassigned),
        Some(id) => match id.parse() {
            Ok(id) => Some(AssigneeFilter::User(id)),
            Err(_) => return ctx.invalid("assignee_id must be a user ID or `none`."),
        },
    };
    let sort = match query.sort.as_deref() {
        None | Some("created_at") => TaskSort::CreatedAt,
        Some("updated_at") => TaskSort::UpdatedAt,
        Some("priority") => TaskSort::Priority,
        Some("due_date") => TaskSort::DueDate,
        Some("title") => TaskSort::Title,
        Some(_) => {
            return ctx
                .invalid("sort must be created_at, updated_at, priority, due_date or title.");
        }
    };
    let order = match query.order.as_deref() {
        None | Some("desc") => SortOrder::Desc,
        Some("asc") => SortOrder::Asc,
        Some(_) => return ctx.invalid("order must be asc or desc."),
    };
    // The repository ranks urgent lowest (ascending is urgent first, as in the app's views);
    // here `desc` means the highest priority first, like the other sorts.
    let order = match (&sort, order) {
        (TaskSort::Priority, SortOrder::Desc) => SortOrder::Asc,
        (TaskSort::Priority, SortOrder::Asc) => SortOrder::Desc,
        (_, order) => order,
    };
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::TasksRead, &ctx.request_id)
            .await?;
        api.list_tasks(TaskQuery {
            project_id: query.project_id,
            query: query.q,
            status_id: query.status_id,
            assignee,
            label_id: query.label_id,
            priority: query.priority,
            parent_task_id: api
                .optional_task_id(query.parent_task_id.as_deref())
                .await?,
            sort: Some((sort, order)),
            cursor: query.cursor,
            limit: query.limit,
        })
        .await
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    get,
    path = "/api/public/v1/workspaces/{workspace_id}/tasks/{task}",
    operation_id = "public_get_task",
    tag = "public-api",
    summary = "A task",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("task" = String, Path, description = "A task UUID or identifier such as `ENG-12` (any letter case).")),
    responses((status = 200, description = "The task", body = TaskView)),
    security(("bearerAuth" = []))
)]
async fn get_task(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, task)): Path<(String, String)>,
) -> Response {
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::TasksRead, &ctx.request_id)
            .await?;
        api.get_task(&task).await
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    post,
    path = "/api/public/v1/workspaces/{workspace_id}/tasks",
    operation_id = "public_create_task",
    tag = "public-api",
    summary = "Create a task",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`.")),
    request_body = CreateTaskBody,
    responses((status = 201, description = "The new task", body = TaskView)),
    security(("bearerAuth" = []))
)]
async fn create_task(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    body: Result<Json<CreateTaskBody>, JsonRejection>,
) -> Response {
    let body = input!(ctx, ApiTokenScope::TasksWrite, body);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::TasksWrite, &ctx.request_id)
            .await?;
        api.create_task(NewTask {
            project_id: body.project_id,
            status_id: body.status_id,
            title: body.title,
            description: body.description,
            priority: body.priority,
            assignee_ids: body.assignee_ids,
            label_ids: body.label_ids,
            parent_task_id: api.optional_task_id(body.parent_task_id.as_deref()).await?,
            due_start_at: body.due_start_at,
            due_at: body.due_at,
        })
        .await
    }
    .await;
    ctx.reply(StatusCode::CREATED, result)
}

#[utoipa::path(
    patch,
    path = "/api/public/v1/workspaces/{workspace_id}/tasks/{task}",
    operation_id = "public_update_task",
    tag = "public-api",
    summary = "Update a task",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("task" = String, Path, description = "A task UUID or identifier such as `ENG-12` (any letter case).")),
    request_body = UpdateTaskBody,
    responses((status = 200, description = "The task", body = TaskView)),
    security(("bearerAuth" = []))
)]
async fn update_task(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, task)): Path<(String, String)>,
    body: Result<Json<UpdateTaskBody>, JsonRejection>,
) -> Response {
    let body = input!(ctx, ApiTokenScope::TasksWrite, body);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::TasksWrite, &ctx.request_id)
            .await?;
        let patch = TaskPatch {
            title: body.title,
            description: body.description,
            status_id: body.status_id,
            priority: body.priority,
            assignee_ids: body.assignee_ids,
            label_ids: body.label_ids,
            due_start_at: body.due_start_at,
            due_at: body.due_at,
            parent_task_id: match body.parent_task_id {
                Some(parent) => Some(api.optional_task_id(parent.as_deref()).await?),
                None => None,
            },
        };
        api.update_task(&task, body.expected_version, patch).await
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    delete,
    path = "/api/public/v1/workspaces/{workspace_id}/tasks/{task}",
    operation_id = "public_delete_task",
    tag = "public-api",
    summary = "Move a task and its sub-issues to the trash, or delete them forever",
    description = "Moves the task and its live sub-issues to the trash, where they stay for 30 days. With `permanent=true`, deletes them forever, with their comments and attachments; a task that is already in the trash goes with the sub-issues trashed with it.",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("task" = String, Path, description = "A task UUID or identifier such as `ENG-12` (any letter case)."), DeleteQuery),
    responses((status = 204, description = "The task is in the trash, or deleted forever with `permanent=true`")),
    security(("bearerAuth" = []))
)]
async fn delete_task(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, task)): Path<(String, String)>,
    query: Result<Query<DeleteQuery>, QueryRejection>,
) -> Response {
    let query = input!(ctx, ApiTokenScope::TasksWrite, query);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::TasksWrite, &ctx.request_id)
            .await?;
        api.delete_task(&task, query.expected_version, query.permanent)
            .await
    }
    .await;
    match result {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => ctx.error(error),
    }
}

#[utoipa::path(
    get,
    path = "/api/public/v1/workspaces/{workspace_id}/docs",
    operation_id = "public_list_docs",
    tag = "public-api",
    summary = "The docs that the token's creator can see",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`.")),
    responses((status = 200, description = "Every visible doc; `next_cursor` is `null`", body = List<PageSummary>)),
    security(("bearerAuth" = []))
)]
async fn docs(State(s): State<PublicApiState>, ctx: Ctx) -> Response {
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::DocsRead, &ctx.request_id)
            .await?;
        Ok(List {
            items: api.list_docs().await?.items,
            next_cursor: None,
        })
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    get,
    path = "/api/public/v1/workspaces/{workspace_id}/docs/{page_id}",
    operation_id = "public_get_doc",
    tag = "public-api",
    summary = "A doc with its content as Markdown",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("page_id" = String, Path)),
    responses((status = 200, description = "The doc", body = DocView)),
    security(("bearerAuth" = []))
)]
async fn doc(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, page)): Path<(String, String)>,
) -> Response {
    let page = path_id!(ctx, page);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::DocsRead, &ctx.request_id)
            .await?;
        api.get_doc(page).await
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    post,
    path = "/api/public/v1/workspaces/{workspace_id}/docs",
    operation_id = "public_create_doc",
    tag = "public-api",
    summary = "Create a doc from Markdown",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`.")),
    request_body = CreateDocBody,
    responses((status = 201, description = "The new doc", body = DocView)),
    security(("bearerAuth" = []))
)]
async fn create_doc(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    body: Result<Json<CreateDocBody>, JsonRejection>,
) -> Response {
    let body = input!(ctx, ApiTokenScope::DocsWrite, body);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::DocsWrite, &ctx.request_id)
            .await?;
        api.create_doc(NewDoc {
            parent_id: body.parent_id,
            teamspace_id: body.teamspace_id,
            private: body.private,
            title: body.title,
            icon: body.icon,
            markdown: body.markdown,
        })
        .await
    }
    .await;
    ctx.reply(StatusCode::CREATED, result)
}

#[utoipa::path(
    patch,
    path = "/api/public/v1/workspaces/{workspace_id}/docs/{page_id}",
    operation_id = "public_update_doc",
    tag = "public-api",
    summary = "Update a doc",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("page_id" = String, Path)),
    request_body = UpdateDocBody,
    responses((status = 200, description = "The doc", body = DocView)),
    security(("bearerAuth" = []))
)]
async fn update_doc(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, page)): Path<(String, String)>,
    body: Result<Json<UpdateDocBody>, JsonRejection>,
) -> Response {
    let page = path_id!(ctx, page);
    let body = input!(ctx, ApiTokenScope::DocsWrite, body);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::DocsWrite, &ctx.request_id)
            .await?;
        let patch = DocPatch {
            title: body.title,
            icon: body.icon,
            markdown: body.markdown,
        };
        api.update_doc(page, body.expected_version, patch).await
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    get,
    path = "/api/public/v1/workspaces/{workspace_id}/tasks/{task}/comments",
    operation_id = "public_list_comments",
    tag = "public-api",
    summary = "The comments of a task, oldest first",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("task" = String, Path, description = "A task UUID or identifier such as `ENG-12` (any letter case)."), PageQuery),
    responses((status = 200, description = "A page of comments", body = List<CommentOut>)),
    security(("bearerAuth" = []))
)]
async fn comments(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, task)): Path<(String, String)>,
    query: Result<Query<PageQuery>, QueryRejection>,
) -> Response {
    let query = input!(ctx, ApiTokenScope::TasksRead, query);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::TasksRead, &ctx.request_id)
            .await?;
        let page = api
            .comments(&task, query.cursor.as_deref(), query.limit)
            .await?;
        Ok(list::<_, CommentOut>(page.items, page.next_cursor))
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    post,
    path = "/api/public/v1/workspaces/{workspace_id}/tasks/{task}/comments",
    operation_id = "public_create_comment",
    tag = "public-api",
    summary = "Comment on a task",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("task" = String, Path, description = "A task UUID or identifier such as `ENG-12` (any letter case).")),
    request_body = CommentBody,
    responses((status = 201, description = "The new comment", body = CommentOut)),
    security(("bearerAuth" = []))
)]
async fn create_comment(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, task)): Path<(String, String)>,
    body: Result<Json<CommentBody>, JsonRejection>,
) -> Response {
    let body = input!(ctx, ApiTokenScope::TasksWrite, body);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::TasksWrite, &ctx.request_id)
            .await?;
        api.create_comment(&task, body.body)
            .await
            .map(CommentOut::from)
    }
    .await;
    ctx.reply(StatusCode::CREATED, result)
}
