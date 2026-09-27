use std::sync::Arc;

use axum::extract::{Extension, FromRequest, FromRequestParts, Path, Query, Request, State};
use axum::http::header::{CONTENT_TYPE, COOKIE};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post};
use axum::{Json, Router};
use orbit_platform::{Id, RequestId, TimestampMillis};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::Row;
use utoipa::{IntoParams, ToSchema};

use crate::auth_routes::CookieMode;
use crate::repositories::identity::{AuthenticatedSession, IdentityRepository};
use crate::repositories::task_filter::{
    self, Condition, FilterField, FilterGroup, FilterNode, FilterOperator, GroupOp, OrderBy,
    OrderDirection, ShowCompleted,
};
use crate::repositories::task_relations::{NewTaskRelationType, TaskRelationRecord};
use crate::repositories::tasks::{
    CreateTask, NotificationRecord, Page, SortOrder, TaskChanges, TaskError, TaskFilter,
    TaskRecord, TaskRepository, TaskSort, TaskUpdate,
};
use crate::repositories::views::ViewRepository;

#[derive(Clone)]
pub struct TaskState {
    pub(crate) identity: Arc<IdentityRepository>,
    tasks: Arc<TaskRepository>,
    pub(crate) views: Arc<ViewRepository>,
    pub(crate) cookie_mode: CookieMode,
}

impl TaskState {
    #[must_use]
    pub fn new(identity: Arc<IdentityRepository>, cookie_mode: CookieMode) -> Self {
        let database = identity.database().clone();
        Self {
            tasks: Arc::new(TaskRepository::new(database.clone())),
            views: Arc::new(ViewRepository::new(database)),
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
            views: Arc::new(ViewRepository::new(tasks.database().clone())),
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
            "/api/v1/workspaces/{workspace_id}/tasks/query",
            post(query_tasks),
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
            "/api/v1/workspaces/{workspace_id}/tasks/{task_id}/relations",
            get(list_task_relations).post(create_task_relation),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/tasks/{task_id}/relations/{relation_id}",
            delete(delete_task_relation),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/tasks/{task_id}/github-links",
            get(list_github_links),
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

pub(crate) struct ApiJson<T>(pub(crate) T);
pub(crate) struct ApiQuery<T>(pub(crate) T);

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
            .map_err(|rejection| {
                if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE {
                    return ApiError::new(
                        StatusCode::PAYLOAD_TOO_LARGE,
                        "request_too_large",
                        "Request too large",
                        "The request exceeds the configured limit.",
                        instance,
                        request_id.as_ref(),
                    );
                }
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
pub(crate) struct MutationQuery {
    pub(crate) expected_version: u64,
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
pub(crate) struct RestoreBody {
    pub(crate) expected_version: u64,
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

#[derive(Deserialize, IntoParams, ToSchema)]
#[into_params(parameter_in = Query)]
#[serde(deny_unknown_fields)]
struct TaskQuery {
    project_id: Option<String>,
    status_id: Option<String>,
    assignee_id: Option<String>,
    #[serde(default)]
    unassigned: bool,
    label_id: Option<String>,
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
struct TaskQueryBody {
    /// The complete filter tree, including any preset and project scope.
    #[schema(value_type = crate::repositories::task_filter::FilterGroup)]
    filter: Value,
    order_by: OrderBy,
    /// Ignored when `order_by` is `manual`.
    order_direction: OrderDirection,
    show_completed: ShowCompleted,
    cursor: Option<String>,
    /// Page size, 1–100 (default 50).
    limit: Option<usize>,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct CreateTaskBody {
    project_id: String,
    status_id: String,
    title: String,
    #[serde(default)]
    description: String,
    source_url: Option<String>,
    #[serde(default = "default_priority")]
    priority: String,
    position: Option<i64>,
    #[serde(default)]
    assignee_ids: Vec<String>,
    #[serde(default)]
    label_ids: Vec<String>,
    #[schema(value_type = Option<String>, format = DateTime)]
    due_start_at: Option<TimestampMillis>,
    #[schema(value_type = Option<String>, format = DateTime)]
    due_at: Option<TimestampMillis>,
}

#[derive(Clone, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct TaskUpdateBody {
    expected_version: u64,
    project_id: Option<String>,
    status_id: Option<String>,
    title: Option<String>,
    description: Option<String>,
    #[serde(default, deserialize_with = "deserialize_source_patch")]
    source_url: Option<Option<String>>,
    priority: Option<String>,
    position: Option<i64>,
    assignee_ids: Option<Vec<String>>,
    label_ids: Option<Vec<String>>,
    #[serde(default, deserialize_with = "deserialize_due_patch")]
    #[schema(value_type = Option<String>, format = DateTime)]
    due_start_at: Option<Option<TimestampMillis>>,
    #[serde(default, deserialize_with = "deserialize_due_patch")]
    #[schema(value_type = Option<String>, format = DateTime)]
    due_at: Option<Option<TimestampMillis>>,
    /// Absent: unchanged. A task id: mark this task as a duplicate of it. `null`: unmark.
    #[serde(default, deserialize_with = "deserialize_source_patch")]
    duplicate_of_id: Option<Option<String>>,
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
    description: Option<String>,
    #[serde(default, deserialize_with = "deserialize_source_patch")]
    source_url: Option<Option<String>>,
    priority: Option<String>,
    position: Option<i64>,
    assignee_ids: Option<Vec<String>>,
    label_ids: Option<Vec<String>>,
    #[serde(default, deserialize_with = "deserialize_due_patch")]
    #[schema(value_type = Option<String>, format = DateTime)]
    due_start_at: Option<Option<TimestampMillis>>,
    #[serde(default, deserialize_with = "deserialize_due_patch")]
    #[schema(value_type = Option<String>, format = DateTime)]
    due_at: Option<Option<TimestampMillis>>,
    /// Absent: unchanged. A task id: mark this task as a duplicate of it. `null`: unmark.
    #[serde(default, deserialize_with = "deserialize_source_patch")]
    duplicate_of_id: Option<Option<String>>,
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
    // Validate in the historical order so the same bad request keeps the same error.
    let project_id = optional_id(query.project_id, &instance, request_id.as_ref())?;
    let status_id = optional_id(query.status_id, &instance, request_id.as_ref())?;
    let assignee_id = optional_id(query.assignee_id, &instance, request_id.as_ref())?;
    let label_id = optional_id(query.label_id, &instance, request_id.as_ref())?;
    let priority_value = query
        .priority
        .map(|value| priority(value, &instance, request_id.as_ref()))
        .transpose()?;
    let search = query
        .search
        .map(|value| bounded(value, 200, 200, "search", &instance, request_id.as_ref()))
        .transpose()?;
    let preset = match query.view.as_deref() {
        None | Some("") => None,
        Some(view) => Some(
            task_filter::preset_filter(view)
                .ok_or_else(|| validation("view", &instance, request_id.as_ref()))?,
        ),
    };
    let sort = match query.sort.as_str() {
        "position" => TaskSort::Position,
        "priority" => TaskSort::Priority,
        "title" => TaskSort::Title,
        "created_at" => TaskSort::CreatedAt,
        "updated_at" => TaskSort::UpdatedAt,
        "due_date" => TaskSort::DueDate,
        _ => return Err(validation("sort", &instance, request_id.as_ref())),
    };
    let order = match query.order.as_str() {
        "asc" => SortOrder::Asc,
        "desc" => SortOrder::Desc,
        _ => return Err(validation("order", &instance, request_id.as_ref())),
    };
    // view=mine|my_week always meant the caller; an explicit assignee_id was ignored with them.
    let personal_view = matches!(query.view.as_deref(), Some("mine" | "my_week"));
    let mut children = Vec::new();
    if let Some(id) = project_id {
        children.push(legacy_condition(
            FilterField::Project,
            FilterOperator::Is,
            serde_json::json!([id.to_string()]),
        ));
    }
    if let Some(id) = status_id {
        // A bare id keeps the exact-status meaning (see task_filter::push_status).
        children.push(legacy_condition(
            FilterField::Status,
            FilterOperator::Is,
            serde_json::json!([id.to_string()]),
        ));
    }
    if let Some(value) = priority_value {
        children.push(legacy_condition(
            FilterField::Priority,
            FilterOperator::Is,
            serde_json::json!([value]),
        ));
    }
    if let Some(id) = assignee_id.filter(|_| !personal_view) {
        children.push(legacy_condition(
            FilterField::Assignee,
            FilterOperator::Is,
            serde_json::json!([id.to_string()]),
        ));
    }
    if query.unassigned {
        children.push(legacy_condition(
            FilterField::Assignee,
            FilterOperator::IsEmpty,
            Value::Null,
        ));
    }
    if let Some(id) = label_id {
        children.push(legacy_condition(
            FilterField::Label,
            FilterOperator::IncludesAny,
            serde_json::json!([id.to_string()]),
        ));
    }
    // An empty search matched everything, so it adds no condition.
    if let Some(text) = search.filter(|text| !text.is_empty()) {
        children.push(legacy_condition(
            FilterField::Text,
            FilterOperator::Contains,
            Value::String(text),
        ));
    }
    let mut groups: Vec<FilterGroup> = preset.into_iter().collect();
    groups.push(FilterGroup {
        op: GroupOp::And,
        children,
    });
    let filter = TaskFilter {
        tree: task_filter::and_groups(groups),
        show_completed: ShowCompleted::All,
        sort,
        order,
    };
    state
        .tasks
        .tasks(
            workspace_id,
            actor_id,
            &filter,
            query.cursor.as_deref(),
            query.limit,
            TimestampMillis::now(),
        )
        .await
        .map(Json)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

fn legacy_condition(field: FilterField, operator: FilterOperator, value: Value) -> FilterNode {
    FilterNode::Condition(Condition {
        field,
        operator,
        value,
    })
}

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/tasks/query", params(("workspace_id" = String, Path)), request_body = TaskQueryBody, responses((status = 200, body = Page<crate::repositories::tasks::TaskRecord>)))]
async fn query_tasks(
    State(state): State<TaskState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<TaskQueryBody>,
) -> Result<Json<Page<TaskRecord>>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/tasks/query");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let tree = task_filter::parse_filter(&body.filter)
        .map_err(|error| task_problem(error.into(), instance.clone(), request_id.as_ref()))?;
    let (sort, order) = task_order(body.order_by, body.order_direction);
    let filter = TaskFilter {
        tree,
        show_completed: body.show_completed,
        sort,
        order,
    };
    state
        .tasks
        .tasks(
            workspace_id,
            actor_id,
            &filter,
            body.cursor.as_deref(),
            body.limit.unwrap_or_else(default_limit),
            TimestampMillis::now(),
        )
        .await
        .map(Json)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

/// Maps display ordering onto the list sort. Manual order has no direction; priority
/// ascending means urgent first, like `GET /tasks?sort=priority&order=asc`.
fn task_order(order_by: OrderBy, direction: OrderDirection) -> (TaskSort, SortOrder) {
    let sort = match order_by {
        OrderBy::Manual => TaskSort::Position,
        OrderBy::Priority => TaskSort::Priority,
        OrderBy::Created => TaskSort::CreatedAt,
        OrderBy::Updated => TaskSort::UpdatedAt,
        OrderBy::Title => TaskSort::Title,
        OrderBy::DueDate => TaskSort::DueDate,
    };
    let order = match (order_by, direction) {
        (OrderBy::Manual, _) | (_, OrderDirection::Asc) => SortOrder::Asc,
        (_, OrderDirection::Desc) => SortOrder::Desc,
    };
    (sort, order)
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

#[derive(Serialize, ToSchema)]
pub struct GithubLink {
    kind: String,
    title: String,
    url: String,
    state: String,
    source: bool,
}

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/tasks/{task_id}/github-links", params(("workspace_id" = String, Path), ("task_id" = String, Path)), responses((status = 200, body = Vec<GithubLink>)))]
pub(crate) async fn list_github_links(
    State(state): State<TaskState>,
    Path((workspace, task)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<Vec<GithubLink>>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/tasks/{task}/github-links");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let task_id = parse_id(&task, &instance, request_id.as_ref())?;
    state
        .tasks
        .get_task(workspace_id, task_id, actor_id)
        .await
        .map_err(|error| task_problem(error, instance.clone(), request_id.as_ref()))?;
    let rows = sqlx::query("SELECT kind, repository || '#' || issue_number AS title, 'https://github.com/' || repository || CASE kind WHEN 'pull_request' THEN '/pull/' ELSE '/issues/' END || issue_number AS url, CASE WHEN sync_paused = 1 THEN 'paused' WHEN kind = 'pull_request' THEN pull_state ELSE 'active' END AS state, 1 AS source FROM github_issue_links WHERE workspace_id = ? AND task_id = ? UNION ALL SELECT 'pull_request' AS kind, title, url, state, 0 AS source FROM github_pull_links WHERE workspace_id = ? AND task_id = ? ORDER BY kind, title")
        .bind(workspace_id.to_string()).bind(task_id.to_string())
        .bind(workspace_id.to_string()).bind(task_id.to_string())
        .fetch_all(state.tasks.database().pool()).await
        .map_err(|error| task_problem(TaskError::Unavailable(error), instance, request_id.as_ref()))?;
    Ok(Json(
        rows.into_iter()
            .map(|row| GithubLink {
                kind: row.get("kind"),
                title: row.get("title"),
                url: row.get("url"),
                state: row.get("state"),
                source: row.get::<i64, _>("source") != 0,
            })
            .collect(),
    ))
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

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct TaskRelationBody {
    #[serde(rename = "type")]
    relation_type: NewTaskRelationType,
    task_id: String,
}

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/tasks/{task_id}/relations", params(("workspace_id" = String, Path), ("task_id" = String, Path)), responses((status = 200, body = Vec<crate::repositories::task_relations::TaskRelationRecord>)))]
async fn list_task_relations(
    State(state): State<TaskState>,
    Path((workspace, task)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<Vec<TaskRelationRecord>>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/tasks/{task}/relations");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let task_id = parse_id(&task, &instance, request_id.as_ref())?;
    state
        .tasks
        .task_relations(workspace_id, task_id, actor_id)
        .await
        .map(Json)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/tasks/{task_id}/relations", params(("workspace_id" = String, Path), ("task_id" = String, Path)), request_body = TaskRelationBody, responses((status = 201, body = crate::repositories::task_relations::TaskRelationRecord)))]
async fn create_task_relation(
    State(state): State<TaskState>,
    Path((workspace, task)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<TaskRelationBody>,
) -> Result<Response, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/tasks/{task}/relations");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let task_id = parse_id(&task, &instance, request_id.as_ref())?;
    let other_id = parse_id(&body.task_id, &instance, request_id.as_ref())?;
    state
        .tasks
        .add_task_relation(
            workspace_id,
            task_id,
            actor_id,
            body.relation_type,
            other_id,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map(|record| (StatusCode::CREATED, Json(record)).into_response())
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(delete, path = "/api/v1/workspaces/{workspace_id}/tasks/{task_id}/relations/{relation_id}", params(("workspace_id" = String, Path), ("task_id" = String, Path), ("relation_id" = String, Path)), responses((status = 204)))]
async fn delete_task_relation(
    State(state): State<TaskState>,
    Path((workspace, task, relation)): Path<(String, String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<StatusCode, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/tasks/{task}/relations/{relation}");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let task_id = parse_id(&task, &instance, request_id.as_ref())?;
    let relation_id = parse_id(&relation, &instance, request_id.as_ref())?;
    state
        .tasks
        .remove_task_relation(
            workspace_id,
            task_id,
            relation_id,
            actor_id,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map(|()| StatusCode::NO_CONTENT)
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
        description: bounded(
            body.description,
            100_000,
            100_000,
            "description",
            &instance,
            request_id.as_ref(),
        )?,
        source_url: source_url(body.source_url, &instance, request_id.as_ref())?,
        priority: priority(body.priority, &instance, request_id.as_ref())?,
        position: body.position,
        assignee_ids: parse_ids(body.assignee_ids, &instance, request_id.as_ref())?,
        label_ids: parse_ids(body.label_ids, &instance, request_id.as_ref())?,
        due_start_at: body.due_start_at,
        due_at: body.due_at,
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
                    description: item.description,
                    source_url: item.source_url,
                    priority: item.priority,
                    position: item.position,
                    assignee_ids: item.assignee_ids,
                    label_ids: item.label_ids,
                    due_start_at: item.due_start_at,
                    due_at: item.due_at,
                    duplicate_of_id: item.duplicate_of_id,
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
    body: String,
    parent_id: Option<String>,
    #[serde(default)]
    mentioned_user_ids: Vec<String>,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct CommentUpdateBody {
    body: String,
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
    let mentioned_user_ids = parse_ids(body.mentioned_user_ids, &instance, request_id.as_ref())?;
    let body = message(
        body.body,
        100_000,
        100_000,
        "body",
        &instance,
        request_id.as_ref(),
    )?;
    state
        .tasks
        .create_comment(
            workspace_id,
            task_id,
            actor_id,
            parent_id,
            body,
            mentioned_user_ids,
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
    let body_text = message(
        body.body,
        100_000,
        100_000,
        "body",
        &instance,
        request_id.as_ref(),
    )?;
    state
        .tasks
        .update_comment(
            workspace_id,
            task_id,
            comment_id,
            actor_id,
            body_text,
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
            description: body
                .description
                .map(|value| bounded(value, 100_000, 100_000, "description", instance, request_id))
                .transpose()?,
            source_url: body
                .source_url
                .map(|value| source_url(value, instance, request_id))
                .transpose()?,
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
            due_start_at: body.due_start_at,
            due_at: body.due_at,
            duplicate_of_id: body
                .duplicate_of_id
                .map(|value| optional_id(value, instance, request_id))
                .transpose()?,
        },
    })
}

fn source_url(
    value: Option<String>,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<Option<String>, ApiError> {
    value
        .map(|url| http_url(url, "source_url", instance, request_id))
        .transpose()
}

/// An absolute http(s) URL of at most 2048 bytes.
pub(crate) fn http_url(
    url: String,
    field: &'static str,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<String, ApiError> {
    let url = bounded(url, 2048, 2048, field, instance, request_id)?;
    let valid = reqwest::Url::parse(&url).is_ok_and(|parsed| {
        matches!(parsed.scheme(), "https" | "http") && parsed.host_str().is_some()
    });
    if !valid || url.chars().any(char::is_whitespace) {
        return Err(validation(field, instance, request_id));
    }
    Ok(url)
}

pub(crate) fn deserialize_source_patch<'de, D>(
    deserializer: D,
) -> Result<Option<Option<String>>, D::Error>
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

pub(crate) async fn scope(
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
    authenticate_session(
        &state.identity,
        state.cookie_mode,
        headers,
        instance,
        request_id,
    )
    .await
}

pub(crate) async fn authenticate_session(
    identity: &IdentityRepository,
    cookie_mode: CookieMode,
    headers: &HeaderMap,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<AuthenticatedSession, ApiError> {
    let token = cookie_value(headers, cookie_mode.session_cookie_name()).ok_or_else(|| {
        ApiError::new(
            StatusCode::UNAUTHORIZED,
            "authentication_required",
            "Authentication required",
            "A valid session is required.",
            instance,
            request_id,
        )
    })?;
    identity
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

pub(crate) fn parse_id(
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

pub(crate) fn bounded(
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

fn message(
    value: String,
    max_chars: usize,
    max_bytes: usize,
    field: &'static str,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<String, ApiError> {
    if value.trim().is_empty() || value.chars().count() > max_chars || value.len() > max_bytes {
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
        "unstarted" | "started" | "completed" | "cancelled" | "duplicate"
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

pub(crate) fn validation(
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

pub(crate) fn task_problem(
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
        TaskError::Conflict | TaskError::IntegrationConflict => ApiError::new(
            StatusCode::CONFLICT,
            "task_conflict",
            "Task conflict",
            "The task operation conflicts with current state.",
            instance,
            request_id,
        ),
        TaskError::GithubContentReadOnly => ApiError::new(
            StatusCode::CONFLICT,
            "github_content_read_only",
            "GitHub content is read-only",
            "GitHub controls this task's title and description.",
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
        TaskError::InvalidFilter { path, message } => {
            ApiError::invalid_filter(path, message, instance, request_id)
        }
        TaskError::Forbidden => ApiError::new(
            StatusCode::FORBIDDEN,
            "task_action_forbidden",
            "Action forbidden",
            "You do not have permission to change this resource.",
            instance,
            request_id,
        ),
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

pub(crate) fn request_id_value(request_id: Option<&Extension<RequestId>>) -> &str {
    request_id.map_or("unknown", |Extension(value)| value.as_str())
}
const fn default_limit() -> usize {
    50
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
    /// JSON path of the first invalid filter node, e.g. `filter.children[2].value`. Only set
    /// for `invalid_filter`.
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
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

pub(crate) struct ApiError {
    status: StatusCode,
    body: Box<ProblemBody>,
}

impl ApiError {
    pub(crate) fn new(
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
                path: None,
            }),
        }
    }

    pub(crate) fn version_conflict(
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

    fn invalid_filter(
        path: String,
        detail: &'static str,
        instance: String,
        request_id: Option<&Extension<RequestId>>,
    ) -> Self {
        let mut error = Self::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_filter",
            "Invalid filter",
            detail,
            instance,
            request_id,
        );
        error.body.path = Some(path);
        error
    }
}

fn refresh_for_current(current: &Value) -> Option<String> {
    let workspace = current.get("workspace_id")?.as_str()?;
    let id = current.get("id")?.as_str()?;
    if current.get("visibility").is_some() {
        return Some(format!("/api/v1/workspaces/{workspace}/views/{id}"));
    }
    if current.get("cover_url").is_some() {
        if current
            .get("deleted_at")
            .is_some_and(|value| !value.is_null())
        {
            return Some(format!("/api/v1/workspaces/{workspace}/pages/trash"));
        }
        return Some(format!("/api/v1/workspaces/{workspace}/pages/{id}"));
    }
    if current.get("is_default").is_some() {
        return Some(format!("/api/v1/workspaces/{workspace}/teamspaces"));
    }
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
