use std::sync::Arc;

use axum::extract::{Extension, FromRequest, FromRequestParts, Path, Query, Request, State};
use axum::http::header::{CONTENT_TYPE, COOKIE};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use orbit_platform::{Id, RequestId, TimestampMillis};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::{IntoParams, ToSchema};

use crate::auth_routes::CookieMode;
use crate::repositories::identity::{AuthenticatedSession, IdentityRepository};
use crate::repositories::tasks::{
    CreateTask, NotificationRecord, Page, SortOrder, TaskChanges, TaskError, TaskFilter,
    TaskRecord, TaskRepository, TaskSort, TaskUpdate,
};

#[derive(Clone)]
pub struct TaskState {
    pub(crate) identity: Arc<IdentityRepository>,
    tasks: Arc<TaskRepository>,
    pub(crate) cookie_mode: CookieMode,
}

impl TaskState {
    #[must_use]
    pub fn new(identity: Arc<IdentityRepository>, cookie_mode: CookieMode) -> Self {
        Self {
            tasks: Arc::new(TaskRepository::new(identity.database().clone())),
            identity,
            cookie_mode,
        }
    }

    #[must_use]
    pub fn with_repository(
        identity: Arc<IdentityRepository>,
        tasks: Arc<TaskRepository>,
        cookie_mode: CookieMode,
    ) -> Self {
        Self {
            identity,
            tasks,
            cookie_mode,
        }
    }
}

pub fn task_router(state: TaskState) -> Router {
    Router::new()
        .route(
            "/api/v1/workspaces/{workspace_id}/projects",
            get(list_projects).post(create_project),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/projects/trash",
            get(list_project_trash),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/projects/{project_id}",
            patch(update_project).delete(delete_project),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/projects/{project_id}/restore",
            post(restore_project),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/projects/{project_id}/statuses",
            get(list_statuses).post(create_status),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/projects/{project_id}/statuses/reorder",
            post(reorder_statuses),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/projects/{project_id}/statuses/{status_id}",
            patch(update_status).delete(delete_status),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/labels",
            get(list_labels).post(create_label),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/labels/{label_id}",
            patch(update_label).delete(delete_label),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/tasks",
            get(list_tasks).post(create_task),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/tasks/trash",
            get(list_task_trash),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/tasks/bulk",
            post(bulk_tasks),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/tasks/resolve",
            post(resolve_tasks),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/tasks/reorder",
            post(reorder_tasks),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/tasks/{task_id}",
            get(get_task).patch(update_task).delete(delete_task),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/tasks/{task_id}/activity",
            get(list_task_activity),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/tasks/{task_id}/restore",
            post(restore_task),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/tasks/{task_id}/comments",
            get(list_comments).post(create_comment),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/tasks/{task_id}/comments/{comment_id}",
            patch(update_comment).delete(delete_comment),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/notifications",
            get(list_notifications),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/notifications/read-all",
            post(read_all_notifications),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/notifications/{notification_id}/read",
            post(read_notification),
        )
        .with_state(state)
}

struct ApiJson<T>(T);
struct ApiQuery<T>(T);

impl<S, T> FromRequestParts<S> for ApiQuery<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let instance = parts.uri.path().to_owned();
        let request_id = parts.extensions.get::<RequestId>().cloned().map(Extension);
        Query::<T>::from_request_parts(parts, state)
            .await
            .map(|Query(value)| Self(value))
            .map_err(|_| {
                ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    "Invalid request",
                    "The request query is not valid for this endpoint.",
                    instance,
                    request_id.as_ref(),
                )
            })
    }
}

impl<S, T> FromRequest<S> for ApiJson<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = ApiError;
    async fn from_request(request: Request, state: &S) -> Result<Self, Self::Rejection> {
        let instance = request.uri().path().to_owned();
        let request_id = request
            .extensions()
            .get::<RequestId>()
            .cloned()
            .map(Extension);
        Json::<T>::from_request(request, state)
            .await
            .map(|Json(value)| Self(value))
            .map_err(|_| {
                ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    "Invalid request",
                    "The request body is not valid for this endpoint.",
                    instance,
                    request_id.as_ref(),
                )
            })
    }
}

#[derive(Deserialize, IntoParams, ToSchema)]
#[into_params(parameter_in = Query)]
#[serde(deny_unknown_fields)]
struct PageQuery {
    cursor: Option<String>,
    #[serde(default = "default_limit")]
    #[param(required = false)]
    limit: usize,
}

#[derive(Deserialize, IntoParams, ToSchema)]
#[into_params(parameter_in = Query)]
#[serde(deny_unknown_fields)]
struct MutationQuery {
    expected_version: u64,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct ProjectBody {
    name: String,
    key: String,
    color: String,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct ProjectUpdateBody {
    name: String,
    key: String,
    color: String,
    expected_version: u64,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct RestoreBody {
    expected_version: u64,
}

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/projects", params(PageQuery, ("workspace_id" = String, Path)), responses((status = 200, body = Page<crate::repositories::tasks::ProjectRecord>)))]
async fn list_projects(
    State(state): State<TaskState>,
    Path(workspace): Path<String>,
    ApiQuery(page): ApiQuery<PageQuery>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<Page<crate::repositories::tasks::ProjectRecord>>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/projects");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    state
        .tasks
        .projects(workspace_id, actor_id, page.cursor.as_deref(), page.limit)
        .await
        .map(Json)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/projects", params(("workspace_id" = String, Path)), request_body = ProjectBody, responses((status = 201, body = crate::repositories::tasks::ProjectRecord)))]
async fn create_project(
    State(state): State<TaskState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<ProjectBody>,
) -> Result<Response, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/projects");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let name = text(body.name, 200, 200, "name", &instance, request_id.as_ref())?;
    let key = project_key(body.key, &instance, request_id.as_ref())?;
    let color = color(body.color, &instance, request_id.as_ref())?;
    state
        .tasks
        .create_project(
            workspace_id,
            actor_id,
            name,
            key,
            color,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map(|record| (StatusCode::CREATED, Json(record)).into_response())
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(patch, path = "/api/v1/workspaces/{workspace_id}/projects/{project_id}", params(("workspace_id" = String, Path), ("project_id" = String, Path)), request_body = ProjectUpdateBody, responses((status = 200, body = crate::repositories::tasks::ProjectRecord)))]
async fn update_project(
    State(state): State<TaskState>,
    Path((workspace, project)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<ProjectUpdateBody>,
) -> Result<Json<crate::repositories::tasks::ProjectRecord>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/projects/{project}");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let project_id = parse_id(&project, &instance, request_id.as_ref())?;
    let name = text(body.name, 200, 200, "name", &instance, request_id.as_ref())?;
    let key = project_key(body.key, &instance, request_id.as_ref())?;
    let color = color(body.color, &instance, request_id.as_ref())?;
    state
        .tasks
        .update_project(
            workspace_id,
            project_id,
            actor_id,
            name,
            key,
            color,
            body.expected_version,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map(Json)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(delete, path = "/api/v1/workspaces/{workspace_id}/projects/{project_id}", params(MutationQuery, ("workspace_id" = String, Path), ("project_id" = String, Path)), responses((status = 204)))]
async fn delete_project(
    State(state): State<TaskState>,
    Path((workspace, project)): Path<(String, String)>,
    ApiQuery(query): ApiQuery<MutationQuery>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<StatusCode, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/projects/{project}");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let project_id = parse_id(&project, &instance, request_id.as_ref())?;
    state
        .tasks
        .delete_project(
            workspace_id,
            project_id,
            actor_id,
            query.expected_version,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map(|()| StatusCode::NO_CONTENT)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/projects/{project_id}/restore", params(("workspace_id" = String, Path), ("project_id" = String, Path)), request_body = RestoreBody, responses((status = 200, body = crate::repositories::tasks::ProjectRecord)))]
async fn restore_project(
    State(state): State<TaskState>,
    Path((workspace, project)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<RestoreBody>,
) -> Result<Json<crate::repositories::tasks::ProjectRecord>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/projects/{project}/restore");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let project_id = parse_id(&project, &instance, request_id.as_ref())?;
    state
        .tasks
        .restore_project(
            workspace_id,
            project_id,
            actor_id,
            body.expected_version,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map(Json)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/projects/trash", params(PageQuery, ("workspace_id" = String, Path)), responses((status = 200, body = Page<crate::repositories::tasks::ProjectRecord>)))]
async fn list_project_trash(
    State(state): State<TaskState>,
    Path(workspace): Path<String>,
    ApiQuery(page): ApiQuery<PageQuery>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<Page<crate::repositories::tasks::ProjectRecord>>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/projects/trash");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    state
        .tasks
        .project_trash(
            workspace_id,
            actor_id,
            page.cursor.as_deref(),
            page.limit,
            TimestampMillis::now(),
        )
        .await
        .map(Json)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct StatusBody {
    name: String,
    #[serde(default)]
    description: String,
    color: String,
    category: String,
    position: Option<i64>,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct StatusUpdateBody {
    name: String,
    #[serde(default)]
    #[schema(value_type = String, required = false)]
    description: StatusDescriptionPatch,
    color: String,
    category: String,
    position: i64,
    expected_version: u64,
}

/// Status descriptions are non-nullable: omission preserves, a string replaces, and JSON null is
/// rejected. An empty string is a valid replacement that clears the description.
#[derive(Default)]
enum StatusDescriptionPatch {
    #[default]
    Omitted,
    Null,
    Value(String),
}

impl<'de> Deserialize<'de> for StatusDescriptionPatch {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(match Option::<String>::deserialize(deserializer)? {
            Some(value) => Self::Value(value),
            None => Self::Null,
        })
    }
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct ReorderBody {
    items: Vec<ReorderItem>,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct ReorderItem {
    id: String,
    expected_version: u64,
    position: i64,
}

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/projects/{project_id}/statuses", params(PageQuery, ("workspace_id" = String, Path), ("project_id" = String, Path)), responses((status = 200, body = Page<crate::repositories::tasks::StatusRecord>)))]
async fn list_statuses(
    State(state): State<TaskState>,
    Path((workspace, project)): Path<(String, String)>,
    ApiQuery(page): ApiQuery<PageQuery>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<Page<crate::repositories::tasks::StatusRecord>>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/projects/{project}/statuses");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let project_id = parse_id(&project, &instance, request_id.as_ref())?;
    state
        .tasks
        .statuses(
            workspace_id,
            project_id,
            actor_id,
            page.cursor.as_deref(),
            page.limit,
        )
        .await
        .map(Json)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/projects/{project_id}/statuses", params(("workspace_id" = String, Path), ("project_id" = String, Path)), request_body = StatusBody, responses((status = 201, body = crate::repositories::tasks::StatusRecord)))]
async fn create_status(
    State(state): State<TaskState>,
    Path((workspace, project)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<StatusBody>,
) -> Result<Response, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/projects/{project}/statuses");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let project_id = parse_id(&project, &instance, request_id.as_ref())?;
    let name = text(body.name, 200, 200, "name", &instance, request_id.as_ref())?;
    let description = bounded(
        body.description,
        20_000,
        20_000,
        "description",
        &instance,
        request_id.as_ref(),
    )?;
    let color = color(body.color, &instance, request_id.as_ref())?;
    let category = category(body.category, &instance, request_id.as_ref())?;
    state
        .tasks
        .create_status(
            workspace_id,
            project_id,
            actor_id,
            name,
            description,
            color,
            category,
            body.position,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map(|record| (StatusCode::CREATED, Json(record)).into_response())
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(patch, path = "/api/v1/workspaces/{workspace_id}/projects/{project_id}/statuses/{status_id}", params(("workspace_id" = String, Path), ("project_id" = String, Path), ("status_id" = String, Path)), request_body = StatusUpdateBody, responses((status = 200, body = crate::repositories::tasks::StatusRecord)))]
async fn update_status(
    State(state): State<TaskState>,
    Path((workspace, project, status)): Path<(String, String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<StatusUpdateBody>,
) -> Result<Json<crate::repositories::tasks::StatusRecord>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/projects/{project}/statuses/{status}");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let project_id = parse_id(&project, &instance, request_id.as_ref())?;
    let status_id = parse_id(&status, &instance, request_id.as_ref())?;
    let name = text(body.name, 200, 200, "name", &instance, request_id.as_ref())?;
    let description = match body.description {
        StatusDescriptionPatch::Omitted => None,
        StatusDescriptionPatch::Null => {
            return Err(validation("description", &instance, request_id.as_ref()));
        }
        StatusDescriptionPatch::Value(value) => Some(bounded(
            value,
            20_000,
            20_000,
            "description",
            &instance,
            request_id.as_ref(),
        )?),
    };
    let color = color(body.color, &instance, request_id.as_ref())?;
    let category = category(body.category, &instance, request_id.as_ref())?;
    state
        .tasks
        .update_status(
            workspace_id,
            project_id,
            status_id,
            actor_id,
            name,
            description,
            color,
            category,
            body.position,
            body.expected_version,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map(Json)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(delete, path = "/api/v1/workspaces/{workspace_id}/projects/{project_id}/statuses/{status_id}", params(MutationQuery, ("workspace_id" = String, Path), ("project_id" = String, Path), ("status_id" = String, Path)), responses((status = 204)))]
async fn delete_status(
    State(state): State<TaskState>,
    Path((workspace, project, status)): Path<(String, String, String)>,
    ApiQuery(query): ApiQuery<MutationQuery>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<StatusCode, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/projects/{project}/statuses/{status}");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let project_id = parse_id(&project, &instance, request_id.as_ref())?;
    let status_id = parse_id(&status, &instance, request_id.as_ref())?;
    state
        .tasks
        .delete_status(
            workspace_id,
            project_id,
            status_id,
            actor_id,
            query.expected_version,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map(|()| StatusCode::NO_CONTENT)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/projects/{project_id}/statuses/reorder", params(("workspace_id" = String, Path), ("project_id" = String, Path)), request_body = ReorderBody, responses((status = 200, body = Page<crate::repositories::tasks::StatusRecord>)))]
async fn reorder_statuses(
    State(state): State<TaskState>,
    Path((workspace, project)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<ReorderBody>,
) -> Result<Json<Page<crate::repositories::tasks::StatusRecord>>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/projects/{project}/statuses/reorder");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let project_id = parse_id(&project, &instance, request_id.as_ref())?;
    let items = parse_reorder(body.items, &instance, request_id.as_ref())?;
    state
        .tasks
        .reorder_statuses(
            workspace_id,
            project_id,
            actor_id,
            &items,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map(|items| {
            Json(Page {
                items,
                next_cursor: None,
            })
        })
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct LabelBody {
    name: String,
    color: String,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct LabelUpdateBody {
    name: String,
    color: String,
    expected_version: u64,
}

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/labels", params(PageQuery, ("workspace_id" = String, Path)), responses((status = 200, body = Page<crate::repositories::tasks::LabelRecord>)))]
async fn list_labels(
    State(state): State<TaskState>,
    Path(workspace): Path<String>,
    ApiQuery(page): ApiQuery<PageQuery>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<Page<crate::repositories::tasks::LabelRecord>>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/labels");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    state
        .tasks
        .labels(workspace_id, actor_id, page.cursor.as_deref(), page.limit)
        .await
        .map(Json)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/labels", params(("workspace_id" = String, Path)), request_body = LabelBody, responses((status = 201, body = crate::repositories::tasks::LabelRecord)))]
async fn create_label(
    State(state): State<TaskState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<LabelBody>,
) -> Result<Response, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/labels");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let name = text(body.name, 100, 100, "name", &instance, request_id.as_ref())?;
    let color = color(body.color, &instance, request_id.as_ref())?;
    state
        .tasks
        .create_label(
            workspace_id,
            actor_id,
            name,
            color,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map(|record| (StatusCode::CREATED, Json(record)).into_response())
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(patch, path = "/api/v1/workspaces/{workspace_id}/labels/{label_id}", params(("workspace_id" = String, Path), ("label_id" = String, Path)), request_body = LabelUpdateBody, responses((status = 200, body = crate::repositories::tasks::LabelRecord)))]
async fn update_label(
    State(state): State<TaskState>,
    Path((workspace, label)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<LabelUpdateBody>,
) -> Result<Json<crate::repositories::tasks::LabelRecord>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/labels/{label}");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let label_id = parse_id(&label, &instance, request_id.as_ref())?;
    let name = text(body.name, 100, 100, "name", &instance, request_id.as_ref())?;
    let color = color(body.color, &instance, request_id.as_ref())?;
    state
        .tasks
        .update_label(
            workspace_id,
            label_id,
            actor_id,
            name,
            color,
            body.expected_version,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map(Json)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(delete, path = "/api/v1/workspaces/{workspace_id}/labels/{label_id}", params(MutationQuery, ("workspace_id" = String, Path), ("label_id" = String, Path)), responses((status = 204)))]
async fn delete_label(
    State(state): State<TaskState>,
    Path((workspace, label)): Path<(String, String)>,
    ApiQuery(query): ApiQuery<MutationQuery>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<StatusCode, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/labels/{label}");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let label_id = parse_id(&label, &instance, request_id.as_ref())?;
    state
        .tasks
        .delete_label(
            workspace_id,
            label_id,
            actor_id,
            query.expected_version,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map(|()| StatusCode::NO_CONTENT)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct ResolveBody {
    identifiers: Vec<String>,
}

/// Batch identifier lookup so a rich-text document resolves all of its task chips in one request.
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/tasks/resolve", params(("workspace_id" = String, Path)), request_body = ResolveBody, responses((status = 200, body = Page<crate::repositories::tasks::TaskRecord>)))]
async fn resolve_tasks(
    State(state): State<TaskState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<ResolveBody>,
) -> Result<Json<Page<TaskRecord>>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/tasks/resolve");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    if body.identifiers.is_empty() || body.identifiers.len() > 100 {
        return Err(validation("identifiers", &instance, request_id.as_ref()));
    }
    let mut identifiers = Vec::with_capacity(body.identifiers.len());
    for value in &body.identifiers {
        let parsed = parse_identifier(value, &instance, request_id.as_ref())?;
        if !identifiers.contains(&parsed) {
            identifiers.push(parsed);
        }
    }
    state
        .tasks
        .resolve_tasks(workspace_id, actor_id, &identifiers)
        .await
        .map(|items| {
            Json(Page {
                items,
                next_cursor: None,
            })
        })
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[derive(Deserialize, IntoParams, ToSchema)]
#[into_params(parameter_in = Query)]
#[serde(deny_unknown_fields)]
struct TaskQuery {
    project_id: Option<String>,
    status_id: Option<String>,
    assignee_id: Option<String>,
    label_id: Option<String>,
    identifier: Option<String>,
    priority: Option<String>,
    search: Option<String>,
    view: Option<String>,
    #[serde(default = "default_task_sort")]
    #[param(required = false)]
    sort: String,
    #[serde(default = "default_order")]
    #[param(required = false)]
    order: String,
    cursor: Option<String>,
    #[serde(default = "default_limit")]
    #[param(required = false)]
    limit: usize,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct CreateTaskBody {
    project_id: String,
    status_id: String,
    title: String,
    #[serde(default = "rich_text_default")]
    #[schema(value_type = Object)]
    description_json: Value,
    #[serde(default = "default_priority")]
    priority: String,
    position: Option<i64>,
    #[serde(default)]
    assignee_ids: Vec<String>,
    #[serde(default)]
    label_ids: Vec<String>,
    #[schema(value_type = Option<String>, format = DateTime)]
    due_at: Option<TimestampMillis>,
    /// Make the new task a sub-issue of this task.
    parent_id: Option<String>,
}

#[derive(Clone, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct TaskUpdateBody {
    expected_version: u64,
    project_id: Option<String>,
    status_id: Option<String>,
    title: Option<String>,
    #[schema(value_type = Option<Object>)]
    description_json: Option<Value>,
    priority: Option<String>,
    position: Option<i64>,
    assignee_ids: Option<Vec<String>>,
    label_ids: Option<Vec<String>>,
    #[serde(default, deserialize_with = "deserialize_due_patch")]
    #[schema(value_type = Option<String>, format = DateTime)]
    due_at: Option<Option<TimestampMillis>>,
    /// Omit to leave unchanged, `null` to detach, an id to (re-)parent.
    #[serde(default, deserialize_with = "deserialize_parent_patch")]
    #[schema(value_type = Option<String>)]
    parent_id: Option<Option<String>>,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct BulkBody {
    updates: Vec<BulkItem>,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct BulkItem {
    id: String,
    expected_version: u64,
    project_id: Option<String>,
    status_id: Option<String>,
    title: Option<String>,
    #[schema(value_type = Option<Object>)]
    description_json: Option<Value>,
    priority: Option<String>,
    position: Option<i64>,
    assignee_ids: Option<Vec<String>>,
    label_ids: Option<Vec<String>>,
    #[serde(default, deserialize_with = "deserialize_due_patch")]
    #[schema(value_type = Option<String>, format = DateTime)]
    due_at: Option<Option<TimestampMillis>>,
    /// Omit to leave unchanged, `null` to detach, an id to (re-)parent.
    #[serde(default, deserialize_with = "deserialize_parent_patch")]
    #[schema(value_type = Option<String>)]
    parent_id: Option<Option<String>>,
}

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/tasks", params(TaskQuery, ("workspace_id" = String, Path)), responses((status = 200, body = Page<crate::repositories::tasks::TaskRecord>)))]
async fn list_tasks(
    State(state): State<TaskState>,
    Path(workspace): Path<String>,
    ApiQuery(query): ApiQuery<TaskQuery>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<Page<TaskRecord>>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/tasks");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let filter = TaskFilter {
        project_id: optional_id(query.project_id, &instance, request_id.as_ref())?,
        status_id: optional_id(query.status_id, &instance, request_id.as_ref())?,
        assignee_id: optional_id(query.assignee_id, &instance, request_id.as_ref())?,
        label_id: optional_id(query.label_id, &instance, request_id.as_ref())?,
        identifier: query
            .identifier
            .as_deref()
            .map(|value| parse_identifier(value, &instance, request_id.as_ref()))
            .transpose()?,
        priority: query
            .priority
            .map(|value| priority(value, &instance, request_id.as_ref()))
            .transpose()?,
        search: query
            .search
            .map(|value| bounded(value, 200, 200, "search", &instance, request_id.as_ref()))
            .transpose()?,
        view: match query.view.as_deref() {
            None | Some("") => None,
            Some("mine") | Some("overdue") | Some("due_soon") => query.view,
            _ => return Err(validation("view", &instance, request_id.as_ref())),
        },
        sort: match query.sort.as_str() {
            "position" => TaskSort::Position,
            "priority" => TaskSort::Priority,
            "title" => TaskSort::Title,
            "created_at" => TaskSort::CreatedAt,
            "updated_at" => TaskSort::UpdatedAt,
            "relevance" => TaskSort::Relevance,
            _ => return Err(validation("sort", &instance, request_id.as_ref())),
        },
        order: match query.order.as_str() {
            "asc" => SortOrder::Asc,
            "desc" => SortOrder::Desc,
            _ => return Err(validation("order", &instance, request_id.as_ref())),
        },
    };
    state
        .tasks
        .tasks(
            workspace_id,
            actor_id,
            &filter,
            query.cursor.as_deref(),
            query.limit,
        )
        .await
        .map(Json)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/tasks/{task_id}", params(("workspace_id" = String, Path), ("task_id" = String, Path)), responses((status = 200, body = crate::repositories::tasks::TaskRecord)))]
async fn get_task(
    State(state): State<TaskState>,
    Path((workspace, task)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<TaskRecord>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/tasks/{task}");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let task_id = parse_id(&task, &instance, request_id.as_ref())?;
    state
        .tasks
        .get_task(workspace_id, task_id, actor_id)
        .await
        .map(Json)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/tasks/{task_id}/activity", params(PageQuery, ("workspace_id" = String, Path), ("task_id" = String, Path)), responses((status = 200, body = Page<crate::audit::AuditEvent>)))]
async fn list_task_activity(
    State(state): State<TaskState>,
    Path((workspace, task)): Path<(String, String)>,
    ApiQuery(page): ApiQuery<PageQuery>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<Page<crate::audit::AuditEvent>>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/tasks/{task}/activity");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let task_id = parse_id(&task, &instance, request_id.as_ref())?;
    state
        .tasks
        .task_activity(
            workspace_id,
            task_id,
            actor_id,
            page.cursor.as_deref(),
            page.limit,
        )
        .await
        .map(Json)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/tasks", params(("workspace_id" = String, Path)), request_body = CreateTaskBody, responses((status = 201, body = crate::repositories::tasks::TaskRecord)))]
async fn create_task(
    State(state): State<TaskState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<CreateTaskBody>,
) -> Result<Response, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/tasks");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let input = CreateTask {
        project_id: parse_id(&body.project_id, &instance, request_id.as_ref())?,
        status_id: parse_id(&body.status_id, &instance, request_id.as_ref())?,
        title: text(
            body.title,
            500,
            500,
            "title",
            &instance,
            request_id.as_ref(),
        )?,
        description_json: body.description_json,
        priority: priority(body.priority, &instance, request_id.as_ref())?,
        position: body.position,
        assignee_ids: parse_ids(body.assignee_ids, &instance, request_id.as_ref())?,
        label_ids: parse_ids(body.label_ids, &instance, request_id.as_ref())?,
        due_at: body.due_at,
        parent_id: body
            .parent_id
            .map(|raw| parent_id(&raw, &instance, request_id.as_ref()))
            .transpose()?,
    };
    state
        .tasks
        .create_task(
            workspace_id,
            actor_id,
            input,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map(|record| (StatusCode::CREATED, Json(record)).into_response())
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(patch, path = "/api/v1/workspaces/{workspace_id}/tasks/{task_id}", params(("workspace_id" = String, Path), ("task_id" = String, Path)), request_body = TaskUpdateBody, responses((status = 200, body = crate::repositories::tasks::TaskRecord)))]
async fn update_task(
    State(state): State<TaskState>,
    Path((workspace, task)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<TaskUpdateBody>,
) -> Result<Json<TaskRecord>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/tasks/{task}");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let update = task_update(task, body, &instance, request_id.as_ref())?;
    state
        .tasks
        .update_task(
            workspace_id,
            actor_id,
            &update,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map(Json)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/tasks/bulk", params(("workspace_id" = String, Path)), request_body = BulkBody, responses((status = 200, body = Page<crate::repositories::tasks::TaskRecord>)))]
async fn bulk_tasks(
    State(state): State<TaskState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<BulkBody>,
) -> Result<Json<Page<TaskRecord>>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/tasks/bulk");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    if body.updates.is_empty() || body.updates.len() > 100 {
        return Err(validation("updates", &instance, request_id.as_ref()));
    }
    let updates = body
        .updates
        .into_iter()
        .map(|item| {
            task_update(
                item.id,
                TaskUpdateBody {
                    expected_version: item.expected_version,
                    project_id: item.project_id,
                    status_id: item.status_id,
                    title: item.title,
                    description_json: item.description_json,
                    priority: item.priority,
                    position: item.position,
                    assignee_ids: item.assignee_ids,
                    label_ids: item.label_ids,
                    due_at: item.due_at,
                    parent_id: item.parent_id,
                },
                &instance,
                request_id.as_ref(),
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    reject_duplicate_ids(
        updates.iter().map(|update| update.id),
        "updates",
        &instance,
        request_id.as_ref(),
    )?;
    state
        .tasks
        .bulk_update_tasks(
            workspace_id,
            actor_id,
            &updates,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map(|items| {
            Json(Page {
                items,
                next_cursor: None,
            })
        })
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/tasks/reorder", params(("workspace_id" = String, Path)), request_body = ReorderBody, responses((status = 200, body = Page<crate::repositories::tasks::TaskRecord>)))]
async fn reorder_tasks(
    State(state): State<TaskState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<ReorderBody>,
) -> Result<Json<Page<TaskRecord>>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/tasks/reorder");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let items = parse_reorder(body.items, &instance, request_id.as_ref())?;
    state
        .tasks
        .reorder_tasks(
            workspace_id,
            actor_id,
            &items,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map(|items| {
            Json(Page {
                items,
                next_cursor: None,
            })
        })
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(delete, path = "/api/v1/workspaces/{workspace_id}/tasks/{task_id}", params(MutationQuery, ("workspace_id" = String, Path), ("task_id" = String, Path)), responses((status = 204)))]
async fn delete_task(
    State(state): State<TaskState>,
    Path((workspace, task)): Path<(String, String)>,
    ApiQuery(query): ApiQuery<MutationQuery>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<StatusCode, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/tasks/{task}");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let task_id = parse_id(&task, &instance, request_id.as_ref())?;
    state
        .tasks
        .delete_task(
            workspace_id,
            task_id,
            actor_id,
            query.expected_version,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map(|()| StatusCode::NO_CONTENT)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/tasks/{task_id}/restore", params(("workspace_id" = String, Path), ("task_id" = String, Path)), request_body = RestoreBody, responses((status = 200, body = crate::repositories::tasks::TaskRecord)))]
async fn restore_task(
    State(state): State<TaskState>,
    Path((workspace, task)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<RestoreBody>,
) -> Result<Json<TaskRecord>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/tasks/{task}/restore");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let task_id = parse_id(&task, &instance, request_id.as_ref())?;
    state
        .tasks
        .restore_task(
            workspace_id,
            task_id,
            actor_id,
            body.expected_version,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map(Json)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/tasks/trash", params(PageQuery, ("workspace_id" = String, Path)), responses((status = 200, body = Page<crate::repositories::tasks::TaskRecord>)))]
async fn list_task_trash(
    State(state): State<TaskState>,
    Path(workspace): Path<String>,
    ApiQuery(page): ApiQuery<PageQuery>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<Page<TaskRecord>>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/tasks/trash");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    state
        .tasks
        .task_trash(
            workspace_id,
            actor_id,
            page.cursor.as_deref(),
            page.limit,
            TimestampMillis::now(),
        )
        .await
        .map(Json)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct CommentBody {
    #[schema(value_type = Object)]
    body_json: Value,
    parent_id: Option<String>,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct CommentUpdateBody {
    #[schema(value_type = Object)]
    body_json: Value,
    expected_version: u64,
}

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/tasks/{task_id}/comments", params(PageQuery, ("workspace_id" = String, Path), ("task_id" = String, Path)), responses((status = 200, body = Page<crate::repositories::tasks::CommentRecord>)))]
async fn list_comments(
    State(state): State<TaskState>,
    Path((workspace, task)): Path<(String, String)>,
    ApiQuery(page): ApiQuery<PageQuery>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<Page<crate::repositories::tasks::CommentRecord>>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/tasks/{task}/comments");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let task_id = parse_id(&task, &instance, request_id.as_ref())?;
    state
        .tasks
        .comments(
            workspace_id,
            task_id,
            actor_id,
            page.cursor.as_deref(),
            page.limit,
        )
        .await
        .map(Json)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/tasks/{task_id}/comments", params(("workspace_id" = String, Path), ("task_id" = String, Path)), request_body = CommentBody, responses((status = 201, body = crate::repositories::tasks::CommentRecord)))]
async fn create_comment(
    State(state): State<TaskState>,
    Path((workspace, task)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<CommentBody>,
) -> Result<Response, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/tasks/{task}/comments");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let task_id = parse_id(&task, &instance, request_id.as_ref())?;
    let parent_id = optional_id(body.parent_id, &instance, request_id.as_ref())?;
    state
        .tasks
        .create_comment(
            workspace_id,
            task_id,
            actor_id,
            parent_id,
            body.body_json,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map(|record| (StatusCode::CREATED, Json(record)).into_response())
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(patch, path = "/api/v1/workspaces/{workspace_id}/tasks/{task_id}/comments/{comment_id}", params(("workspace_id" = String, Path), ("task_id" = String, Path), ("comment_id" = String, Path)), request_body = CommentUpdateBody, responses((status = 200, body = crate::repositories::tasks::CommentRecord)))]
async fn update_comment(
    State(state): State<TaskState>,
    Path((workspace, task, comment)): Path<(String, String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<CommentUpdateBody>,
) -> Result<Json<crate::repositories::tasks::CommentRecord>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/tasks/{task}/comments/{comment}");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let task_id = parse_id(&task, &instance, request_id.as_ref())?;
    let comment_id = parse_id(&comment, &instance, request_id.as_ref())?;
    state
        .tasks
        .update_comment(
            workspace_id,
            task_id,
            comment_id,
            actor_id,
            body.body_json,
            body.expected_version,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map(Json)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(delete, path = "/api/v1/workspaces/{workspace_id}/tasks/{task_id}/comments/{comment_id}", params(MutationQuery, ("workspace_id" = String, Path), ("task_id" = String, Path), ("comment_id" = String, Path)), responses((status = 204)))]
async fn delete_comment(
    State(state): State<TaskState>,
    Path((workspace, task, comment)): Path<(String, String, String)>,
    ApiQuery(query): ApiQuery<MutationQuery>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<StatusCode, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/tasks/{task}/comments/{comment}");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let task_id = parse_id(&task, &instance, request_id.as_ref())?;
    let comment_id = parse_id(&comment, &instance, request_id.as_ref())?;
    state
        .tasks
        .delete_comment(
            workspace_id,
            task_id,
            comment_id,
            actor_id,
            query.expected_version,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map(|()| StatusCode::NO_CONTENT)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[derive(Deserialize, IntoParams, ToSchema)]
#[into_params(parameter_in = Query)]
#[serde(deny_unknown_fields)]
struct NotificationQuery {
    #[serde(default)]
    unread: bool,
    cursor: Option<String>,
    #[serde(default = "default_limit")]
    #[param(required = false)]
    limit: usize,
}

#[derive(Serialize, ToSchema)]
struct ReadAllResponse {
    updated: u64,
}

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/notifications", params(NotificationQuery, ("workspace_id" = String, Path)), responses((status = 200, body = Page<NotificationRecord>)))]
async fn list_notifications(
    State(state): State<TaskState>,
    Path(workspace): Path<String>,
    ApiQuery(query): ApiQuery<NotificationQuery>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<Page<NotificationRecord>>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/notifications");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    state
        .tasks
        .notifications(
            workspace_id,
            actor_id,
            query.unread,
            query.cursor.as_deref(),
            query.limit,
        )
        .await
        .map(Json)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/notifications/{notification_id}/read", params(("workspace_id" = String, Path), ("notification_id" = String, Path)), responses((status = 200, body = NotificationRecord)))]
async fn read_notification(
    State(state): State<TaskState>,
    Path((workspace, notification)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<NotificationRecord>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/notifications/{notification}/read");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let notification_id = parse_id(&notification, &instance, request_id.as_ref())?;
    state
        .tasks
        .mark_notification_read(
            workspace_id,
            actor_id,
            notification_id,
            TimestampMillis::now(),
        )
        .await
        .map(Json)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/notifications/read-all", params(("workspace_id" = String, Path)), responses((status = 200, body = ReadAllResponse)))]
async fn read_all_notifications(
    State(state): State<TaskState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<ReadAllResponse>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/notifications/read-all");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    state
        .tasks
        .mark_notifications_read(workspace_id, actor_id, TimestampMillis::now())
        .await
        .map(|updated| Json(ReadAllResponse { updated }))
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

fn task_update(
    id: String,
    body: TaskUpdateBody,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<TaskUpdate, ApiError> {
    Ok(TaskUpdate {
        id: parse_id(&id, instance, request_id)?,
        expected_version: body.expected_version,
        changes: TaskChanges {
            project_id: optional_id(body.project_id, instance, request_id)?,
            status_id: optional_id(body.status_id, instance, request_id)?,
            title: body
                .title
                .map(|value| text(value, 500, 500, "title", instance, request_id))
                .transpose()?,
            description_json: body.description_json,
            priority: body
                .priority
                .map(|value| priority(value, instance, request_id))
                .transpose()?,
            position: body.position,
            assignee_ids: body
                .assignee_ids
                .map(|values| parse_ids(values, instance, request_id))
                .transpose()?,
            label_ids: body
                .label_ids
                .map(|values| parse_ids(values, instance, request_id))
                .transpose()?,
            due_at: body.due_at,
            parent_id: body
                .parent_id
                .map(|value| {
                    value
                        .map(|raw| parent_id(&raw, instance, request_id))
                        .transpose()
                })
                .transpose()?,
        },
    })
}

/// A malformed parent id is a validation failure on the field, not a 404: the
/// resource the URL names exists, the proposed edge does not.
fn parent_id(
    raw: &str,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<Id, ApiError> {
    raw.parse()
        .map_err(|_| validation("parent_id", instance, request_id))
}

fn deserialize_parent_patch<'de, D>(deserializer: D) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer).map(Some)
}

fn deserialize_due_patch<'de, D>(
    deserializer: D,
) -> Result<Option<Option<TimestampMillis>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<TimestampMillis>::deserialize(deserializer).map(Some)
}

fn parse_reorder(
    items: Vec<ReorderItem>,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<Vec<(Id, u64, i64)>, ApiError> {
    if items.is_empty() || items.len() > 100 {
        return Err(validation("items", instance, request_id));
    }
    let items = items
        .into_iter()
        .map(|item| {
            Ok((
                parse_id(&item.id, instance, request_id)?,
                item.expected_version,
                item.position,
            ))
        })
        .collect::<Result<Vec<_>, _>>()?;
    reject_duplicate_ids(
        items.iter().map(|(id, _, _)| *id),
        "items",
        instance,
        request_id,
    )?;
    Ok(items)
}

fn reject_duplicate_ids(
    ids: impl Iterator<Item = Id>,
    field: &'static str,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<(), ApiError> {
    let mut ids = ids.collect::<Vec<_>>();
    ids.sort_unstable();
    if ids.windows(2).any(|pair| pair[0] == pair[1]) {
        Err(validation(field, instance, request_id))
    } else {
        Ok(())
    }
}

async fn scope(
    state: &TaskState,
    headers: &HeaderMap,
    workspace: &str,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<(Id, Id), ApiError> {
    let session = authenticate(state, headers, instance, request_id).await?;
    Ok((parse_id(workspace, instance, request_id)?, session.user.id))
}

async fn authenticate(
    state: &TaskState,
    headers: &HeaderMap,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<AuthenticatedSession, ApiError> {
    let token =
        cookie_value(headers, state.cookie_mode.session_cookie_name()).ok_or_else(|| {
            ApiError::new(
                StatusCode::UNAUTHORIZED,
                "authentication_required",
                "Authentication required",
                "A valid session is required.",
                instance,
                request_id,
            )
        })?;
    state
        .identity
        .authenticate_session(&token, TimestampMillis::now())
        .await
        .map_err(|_| {
            ApiError::new(
                StatusCode::UNAUTHORIZED,
                "authentication_required",
                "Authentication required",
                "A valid session is required.",
                instance,
                request_id,
            )
        })
}

fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .map(str::trim)
        .find_map(|pair| pair.strip_prefix(&format!("{name}=")).map(str::to_owned))
}

fn parse_id(
    value: &str,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<Id, ApiError> {
    value.parse().map_err(|_| {
        ApiError::new(
            StatusCode::NOT_FOUND,
            "task_resource_not_found",
            "Task resource not found",
            "The requested task resource was not found.",
            instance,
            request_id,
        )
    })
}

fn optional_id(
    value: Option<String>,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<Option<Id>, ApiError> {
    value
        .map(|value| parse_id(&value, instance, request_id))
        .transpose()
}

fn parse_ids(
    values: Vec<String>,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<Vec<Id>, ApiError> {
    values
        .into_iter()
        .map(|value| parse_id(&value, instance, request_id))
        .collect()
}

fn text(
    value: String,
    max_chars: usize,
    max_bytes: usize,
    field: &'static str,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<String, ApiError> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > max_chars || value.len() > max_bytes {
        Err(validation(field, instance, request_id))
    } else {
        Ok(value.to_owned())
    }
}

fn bounded(
    value: String,
    max_chars: usize,
    max_bytes: usize,
    field: &'static str,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<String, ApiError> {
    if value.chars().count() > max_chars || value.len() > max_bytes {
        Err(validation(field, instance, request_id))
    } else {
        Ok(value)
    }
}

fn project_key(
    value: String,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<String, ApiError> {
    let value = value.trim().to_ascii_uppercase();
    if value.is_empty()
        || value.len() > 20
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
    {
        Err(validation("key", instance, request_id))
    } else {
        Ok(value)
    }
}

/// `ORB-12` -> `("ORB", 12)`. The key may contain hyphens; only the final segment is the number.
fn parse_identifier(
    value: &str,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<(String, i64), ApiError> {
    let invalid = || validation("identifier", instance, request_id);
    let (key, number) = value.rsplit_once('-').ok_or_else(invalid)?;
    if key.is_empty() {
        return Err(invalid());
    }
    let number: i64 = number.parse().map_err(|_| invalid())?;
    if number < 0 {
        return Err(invalid());
    }
    Ok((key.to_owned(), number))
}

fn color(
    value: String,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<String, ApiError> {
    if value.len() == 7
        && value.starts_with('#')
        && value[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        Ok(value.to_ascii_lowercase())
    } else {
        Err(validation("color", instance, request_id))
    }
}

fn category(
    value: String,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<String, ApiError> {
    if matches!(
        value.as_str(),
        "unstarted" | "started" | "completed" | "cancelled"
    ) {
        Ok(value)
    } else {
        Err(validation("category", instance, request_id))
    }
}

fn priority(
    value: String,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<String, ApiError> {
    if matches!(
        value.as_str(),
        "none" | "low" | "medium" | "high" | "urgent"
    ) {
        Ok(value)
    } else {
        Err(validation("priority", instance, request_id))
    }
}

fn validation(
    field: &'static str,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> ApiError {
    ApiError::new(
        StatusCode::UNPROCESSABLE_ENTITY,
        "validation_failed",
        "Validation failed",
        field,
        instance,
        request_id,
    )
}

fn task_problem(
    error: TaskError,
    instance: impl Into<String>,
    request_id: Option<&Extension<RequestId>>,
) -> ApiError {
    let instance = instance.into();
    match error {
        TaskError::NotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            "task_resource_not_found",
            "Task resource not found",
            "The requested task resource was not found.",
            instance,
            request_id,
        ),
        TaskError::Invalid { field } => validation(field, &instance, request_id),
        TaskError::InvalidDocument { field: _, reason } => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "validation_failed",
            "Validation failed",
            reason,
            &instance,
            request_id,
        ),
        TaskError::Conflict => ApiError::new(
            StatusCode::CONFLICT,
            "task_conflict",
            "Task conflict",
            "The task operation conflicts with current state.",
            instance,
            request_id,
        ),
        TaskError::RestoreConflict { field } => {
            ApiError::restore_conflict(field, instance, request_id)
        }
        TaskError::InvalidCursor => ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_cursor",
            "Invalid cursor",
            "The cursor does not match this collection query.",
            instance,
            request_id,
        ),
        TaskError::VersionConflict { current } => {
            ApiError::version_conflict(*current, instance, request_id)
        }
        TaskError::Unavailable(_) => ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            "Internal server error",
            "An unexpected error occurred. Use the request ID when contacting support.",
            instance,
            request_id,
        ),
    }
}

fn request_id_value(request_id: Option<&Extension<RequestId>>) -> &str {
    request_id.map_or("unknown", |Extension(value)| value.as_str())
}
const fn default_limit() -> usize {
    50
}
/// TipTap's empty document, so a create body may omit the description entirely.
fn rich_text_default() -> Value {
    orbit_domain::rich_text::empty_document()
}

fn default_priority() -> String {
    "none".to_owned()
}
fn default_task_sort() -> String {
    "position".to_owned()
}
fn default_order() -> String {
    "asc".to_owned()
}

#[derive(Debug, Serialize, ToSchema)]
#[schema(as = TaskProblem)]
pub(crate) struct ProblemBody {
    #[serde(rename = "type")]
    type_uri: String,
    title: &'static str,
    status: u16,
    code: &'static str,
    detail: &'static str,
    instance: String,
    request_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    conflict: Option<ConflictBody>,
}

#[derive(Debug, Serialize, ToSchema)]
#[schema(as = TaskConflict)]
pub(crate) struct ConflictBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    current_version: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    current: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    refresh: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    field: Option<&'static str>,
}

struct ApiError {
    status: StatusCode,
    body: Box<ProblemBody>,
}

impl ApiError {
    fn new(
        status: StatusCode,
        code: &'static str,
        title: &'static str,
        detail: &'static str,
        instance: impl Into<String>,
        request_id: Option<&Extension<RequestId>>,
    ) -> Self {
        Self {
            status,
            body: Box::new(ProblemBody {
                type_uri: format!("https://docs.orbit.dev/problems/{code}"),
                title,
                status: status.as_u16(),
                code,
                detail,
                instance: instance.into(),
                request_id: request_id
                    .map(|Extension(value)| value.as_str().to_owned())
                    .unwrap_or_else(|| "unknown".to_owned()),
                conflict: None,
            }),
        }
    }

    fn version_conflict(
        current: Value,
        instance: String,
        request_id: Option<&Extension<RequestId>>,
    ) -> Self {
        let current_version = current.get("version").and_then(Value::as_u64);
        let refresh = refresh_for_current(&current).unwrap_or_else(|| instance.clone());
        let mut error = Self::new(
            StatusCode::CONFLICT,
            "conflict",
            "Conflict",
            "The resource changed after it was read. Refresh and retry.",
            instance.clone(),
            request_id,
        );
        error.body.conflict = Some(ConflictBody {
            current_version,
            current: Some(current),
            refresh: Some(refresh),
            field: None,
        });
        error
    }

    fn restore_conflict(
        field: &'static str,
        instance: String,
        request_id: Option<&Extension<RequestId>>,
    ) -> Self {
        let mut error = Self::new(
            StatusCode::CONFLICT,
            "restore_conflict",
            "Restore conflict",
            "The resource cannot be restored because a unique value is already in use.",
            instance,
            request_id,
        );
        error.body.conflict = Some(ConflictBody {
            current_version: None,
            current: None,
            refresh: None,
            field: Some(field),
        });
        error
    }
}

fn refresh_for_current(current: &Value) -> Option<String> {
    let workspace = current.get("workspace_id")?.as_str()?;
    let id = current.get("id")?.as_str()?;
    if current.get("author_id").is_some() {
        let task = current.get("task_id")?.as_str()?;
        return Some(format!(
            "/api/v1/workspaces/{workspace}/tasks/{task}/comments"
        ));
    }
    if current.get("creator_id").is_some() {
        if current
            .get("deleted_at")
            .is_some_and(|value| !value.is_null())
        {
            return Some(format!("/api/v1/workspaces/{workspace}/tasks/trash"));
        }
        return Some(format!("/api/v1/workspaces/{workspace}/tasks/{id}"));
    }
    if let Some(project) = current.get("project_id").and_then(Value::as_str) {
        return Some(format!(
            "/api/v1/workspaces/{workspace}/projects/{project}/statuses"
        ));
    }
    if current.get("key").is_some() {
        let suffix = if current
            .get("deleted_at")
            .is_some_and(|value| !value.is_null())
        {
            "/trash"
        } else {
            ""
        };
        return Some(format!("/api/v1/workspaces/{workspace}/projects{suffix}"));
    }
    Some(format!("/api/v1/workspaces/{workspace}/labels"))
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut response = (self.status, Json(*self.body)).into_response();
        response.headers_mut().insert(
            CONTENT_TYPE,
            HeaderValue::from_static("application/problem+json"),
        );
        response
    }
}
