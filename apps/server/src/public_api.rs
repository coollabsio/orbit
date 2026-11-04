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

use crate::billing::guard::READ_ONLY_DETAIL;
use crate::repositories::api_tokens::{
    ApiTokenError, ApiTokenPrincipal, ApiTokenRepository, ApiTokenScope,
};
use crate::repositories::pages::PageRepository;
use crate::repositories::tasks::{ProjectRecord, StatusRecord, TaskError, TaskRepository};
use crate::token_api::{NewTask, TaskPatch, TaskQuery, TokenApi, TokenApiError};

pub const BASE_PATH: &str = "/api/public/v1";

#[derive(Clone)]
pub struct PublicApiState {
    tokens: Arc<ApiTokenRepository>,
    api: TokenApi,
}

impl PublicApiState {
    #[must_use]
    pub fn new(tokens: Arc<ApiTokenRepository>, tasks: Arc<TaskRepository>) -> Self {
        Self {
            tokens,
            api: TokenApi::new(tasks),
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
        .route("/tasks", get(list_tasks).post(create_task))
        .route(
            "/tasks/{task}",
            get(get_task).patch(update_task).delete(delete_task),
        )
        .route("/docs", get(docs))
        .route("/docs/{page_id}", get(doc));
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
            TokenApiError::Internal => Problem::internal(&self.request_id, &self.instance),
            TokenApiError::Task(error) => self.task_problem(error),
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

#[derive(Serialize)]
struct List<T> {
    items: Vec<T>,
    next_cursor: Option<String>,
}

fn list<T, U: From<T>>(items: Vec<T>, next_cursor: Option<String>) -> List<U> {
    List {
        items: items.into_iter().map(U::from).collect(),
        next_cursor,
    }
}

#[derive(Serialize)]
struct ProjectOut {
    id: Id,
    name: String,
    key: String,
    color: String,
    version: u64,
    created_at: TimestampMillis,
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

#[derive(Serialize)]
struct StatusOut {
    id: Id,
    project_id: Id,
    name: String,
    description: String,
    color: String,
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PageQuery {
    cursor: Option<String>,
    limit: Option<usize>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TasksQuery {
    project_id: Option<Id>,
    status_id: Option<Id>,
    assignee_id: Option<Id>,
    label_id: Option<Id>,
    priority: Option<String>,
    q: Option<String>,
    cursor: Option<String>,
    limit: Option<usize>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateTaskBody {
    project_id: Id,
    status_id: Id,
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default = "default_priority")]
    priority: String,
    #[serde(default)]
    assignee_ids: Vec<Id>,
    #[serde(default)]
    label_ids: Vec<Id>,
    parent_task_id: Option<Id>,
}

fn default_priority() -> String {
    "none".to_owned()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateTaskBody {
    expected_version: u64,
    title: Option<String>,
    description: Option<String>,
    status_id: Option<Id>,
    priority: Option<String>,
    assignee_ids: Option<Vec<Id>>,
    label_ids: Option<Vec<Id>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeleteQuery {
    expected_version: u64,
}

/// Reads a query string or JSON body, or answers 400.
macro_rules! input {
    ($ctx:expr, $value:expr) => {
        match $value {
            Ok(value) => value.0,
            // The parser's text can quote the request; answer with a fixed message.
            Err(_) => {
                return $ctx.invalid(
                    "The query string or JSON body is invalid. Check the field names, types and required fields.",
                )
            }
        }
    };
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

async fn me(State(s): State<PublicApiState>, ctx: Ctx) -> Response {
    ctx.reply(StatusCode::OK, s.api.me(&ctx.principal).await)
}

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

async fn statuses(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_, project)): Path<(String, String)>,
    query: Result<Query<PageQuery>, QueryRejection>,
) -> Response {
    let project = path_id!(ctx, project);
    let query = input!(ctx, query);
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

async fn list_tasks(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    query: Result<Query<TasksQuery>, QueryRejection>,
) -> Response {
    let query = input!(ctx, query);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::TasksRead, &ctx.request_id)
            .await?;
        api.list_tasks(TaskQuery {
            project_id: query.project_id,
            query: query.q,
            status_id: query.status_id,
            assignee_id: query.assignee_id,
            label_id: query.label_id,
            priority: query.priority,
            cursor: query.cursor,
            limit: query.limit,
        })
        .await
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

async fn get_task(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_, task)): Path<(String, String)>,
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

async fn create_task(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    body: Result<Json<CreateTaskBody>, JsonRejection>,
) -> Response {
    let body = input!(ctx, body);
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
            parent_task_id: body.parent_task_id,
        })
        .await
    }
    .await;
    ctx.reply(StatusCode::CREATED, result)
}

async fn update_task(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_, task)): Path<(String, String)>,
    body: Result<Json<UpdateTaskBody>, JsonRejection>,
) -> Response {
    let body = input!(ctx, body);
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
        };
        api.update_task(&task, body.expected_version, patch).await
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

async fn delete_task(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_, task)): Path<(String, String)>,
    query: Result<Query<DeleteQuery>, QueryRejection>,
) -> Response {
    let query = input!(ctx, query);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::TasksWrite, &ctx.request_id)
            .await?;
        api.delete_task(&task, query.expected_version).await
    }
    .await;
    match result {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => ctx.error(error),
    }
}

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

async fn doc(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_, page)): Path<(String, String)>,
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
