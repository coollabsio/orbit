//! Custom task fields (issue #87): the workspace's field gallery, the fields of a project, and a
//! task's values.

use axum::extract::{Extension, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, put};
use axum::{Json, Router};
use orbit_platform::{RequestId, TimestampMillis};
use serde::Deserialize;
use serde_json::Value;
use utoipa::ToSchema;

use crate::repositories::tasks::{
    Page, TaskFieldKind, TaskFieldOptionInput, TaskFieldRecord, TaskFieldValue,
};
use crate::task_routes::{
    ApiError, ApiJson, ApiQuery, MutationQuery, TaskState, parse_id, request_id_value, scope,
    task_problem, validation,
};

pub(crate) fn routes() -> Router<TaskState> {
    Router::new()
        .route(
            "/api/v1/workspaces/{workspace_id}/task-fields",
            get(list_task_fields).post(create_task_field),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/task-fields/{field_id}",
            patch(update_task_field).delete(delete_task_field),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/projects/{project_id}/fields",
            get(list_project_task_fields).put(set_project_task_fields),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/tasks/{task_id}/fields",
            get(list_task_field_values),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/tasks/{task_id}/fields/{field_id}",
            put(set_task_field_value),
        )
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
#[schema(as = TaskFieldBody)]
pub(crate) struct TaskFieldBody {
    name: String,
    kind: TaskFieldKind,
    /// The choices of a `select` or `multi_select` field. Absent: none.
    #[serde(default)]
    options: Vec<TaskFieldOptionInput>,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
#[schema(as = TaskFieldUpdateBody)]
pub(crate) struct TaskFieldUpdateBody {
    /// Absent: unchanged.
    name: Option<String>,
    /// Absent: unchanged. Replaces every option: send the `id` of an option to keep it.
    options: Option<Vec<TaskFieldOptionInput>>,
    expected_version: u64,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
#[schema(as = ProjectTaskFieldsBody)]
pub(crate) struct ProjectTaskFieldsBody {
    /// Every field the project shows, in order; a field that is not listed is removed.
    field_ids: Vec<String>,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
#[schema(as = TaskFieldValueBody)]
pub(crate) struct TaskFieldValueBody {
    /// The new value (see `TaskFieldValue`); `null` clears it.
    #[schema(value_type = Value)]
    value: Value,
}

/// A trimmed name of 1 to 100 characters.
fn field_name(
    name: String,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<String, ApiError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 100 {
        Err(validation("name", instance, request_id))
    } else {
        Ok(name.to_owned())
    }
}

fn page<T>(items: Vec<T>) -> Json<Page<T>> {
    Json(Page {
        items,
        next_cursor: None,
    })
}

/// The workspace's field gallery.
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/task-fields", params(("workspace_id" = String, Path)), responses((status = 200, body = Page<TaskFieldRecord>)))]
pub(crate) async fn list_task_fields(
    State(state): State<TaskState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<Page<TaskFieldRecord>>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/task-fields");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    state
        .tasks
        .task_fields(workspace_id, actor_id)
        .await
        .map(page)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

/// Adds a field to the gallery; a name the workspace already has is a conflict.
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/task-fields", params(("workspace_id" = String, Path)), request_body = TaskFieldBody, responses((status = 201, body = TaskFieldRecord)))]
pub(crate) async fn create_task_field(
    State(state): State<TaskState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<TaskFieldBody>,
) -> Result<Response, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/task-fields");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let name = field_name(body.name, &instance, request_id.as_ref())?;
    state
        .tasks
        .create_task_field(
            workspace_id,
            actor_id,
            name,
            body.kind,
            body.options,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map(|record| (StatusCode::CREATED, Json(record)).into_response())
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

/// Renames the field or replaces its options. Only its creator, an owner or an admin may.
#[utoipa::path(patch, path = "/api/v1/workspaces/{workspace_id}/task-fields/{field_id}", params(("workspace_id" = String, Path), ("field_id" = String, Path)), request_body = TaskFieldUpdateBody, responses((status = 200, body = TaskFieldRecord)))]
pub(crate) async fn update_task_field(
    State(state): State<TaskState>,
    Path((workspace, field)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<TaskFieldUpdateBody>,
) -> Result<Json<TaskFieldRecord>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/task-fields/{field}");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let field_id = parse_id(&field, &instance, request_id.as_ref())?;
    let name = body
        .name
        .map(|name| field_name(name, &instance, request_id.as_ref()))
        .transpose()?;
    state
        .tasks
        .update_task_field(
            workspace_id,
            field_id,
            actor_id,
            name,
            body.options,
            body.expected_version,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map(Json)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

/// Deletes the field from the gallery, every project and every task.
#[utoipa::path(delete, path = "/api/v1/workspaces/{workspace_id}/task-fields/{field_id}", params(MutationQuery, ("workspace_id" = String, Path), ("field_id" = String, Path)), responses((status = 204)))]
pub(crate) async fn delete_task_field(
    State(state): State<TaskState>,
    Path((workspace, field)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiQuery(query): ApiQuery<MutationQuery>,
) -> Result<StatusCode, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/task-fields/{field}");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let field_id = parse_id(&field, &instance, request_id.as_ref())?;
    state
        .tasks
        .delete_task_field(
            workspace_id,
            field_id,
            actor_id,
            query.expected_version,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map(|()| StatusCode::NO_CONTENT)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

/// The fields the project's tasks show, in order.
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/projects/{project_id}/fields", params(("workspace_id" = String, Path), ("project_id" = String, Path)), responses((status = 200, body = Page<TaskFieldRecord>)))]
pub(crate) async fn list_project_task_fields(
    State(state): State<TaskState>,
    Path((workspace, project)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<Page<TaskFieldRecord>>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/projects/{project}/fields");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let project_id = parse_id(&project, &instance, request_id.as_ref())?;
    state
        .tasks
        .project_task_fields(workspace_id, project_id, actor_id)
        .await
        .map(page)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

/// Sets the fields the project's tasks show, in order. Needs `projects.manage`.
#[utoipa::path(put, path = "/api/v1/workspaces/{workspace_id}/projects/{project_id}/fields", params(("workspace_id" = String, Path), ("project_id" = String, Path)), request_body = ProjectTaskFieldsBody, responses((status = 200, body = Page<TaskFieldRecord>)))]
pub(crate) async fn set_project_task_fields(
    State(state): State<TaskState>,
    Path((workspace, project)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<ProjectTaskFieldsBody>,
) -> Result<Json<Page<TaskFieldRecord>>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/projects/{project}/fields");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let project_id = parse_id(&project, &instance, request_id.as_ref())?;
    let field_ids = body
        .field_ids
        .iter()
        .map(|id| id.parse().map_err(|_| validation("field_ids", &instance, request_id.as_ref())))
        .collect::<Result<Vec<_>, _>>()?;
    state
        .tasks
        .set_project_task_fields(
            workspace_id,
            project_id,
            actor_id,
            &field_ids,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map(page)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

/// The task's field values, also of fields its project does not show.
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/tasks/{task_id}/fields", params(("workspace_id" = String, Path), ("task_id" = String, Path)), responses((status = 200, body = Page<TaskFieldValue>)))]
pub(crate) async fn list_task_field_values(
    State(state): State<TaskState>,
    Path((workspace, task)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<Page<TaskFieldValue>>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/tasks/{task}/fields");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let task_id = parse_id(&task, &instance, request_id.as_ref())?;
    state
        .tasks
        .task_field_values(workspace_id, task_id, actor_id)
        .await
        .map(page)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

/// Sets or clears the task's value of a field its project shows; returns all its values.
#[utoipa::path(put, path = "/api/v1/workspaces/{workspace_id}/tasks/{task_id}/fields/{field_id}", params(("workspace_id" = String, Path), ("task_id" = String, Path), ("field_id" = String, Path)), request_body = TaskFieldValueBody, responses((status = 200, body = Page<TaskFieldValue>)))]
pub(crate) async fn set_task_field_value(
    State(state): State<TaskState>,
    Path((workspace, task, field)): Path<(String, String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<TaskFieldValueBody>,
) -> Result<Json<Page<TaskFieldValue>>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/tasks/{task}/fields/{field}");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let task_id = parse_id(&task, &instance, request_id.as_ref())?;
    let field_id = parse_id(&field, &instance, request_id.as_ref())?;
    state
        .tasks
        .set_task_field_value(
            workspace_id,
            task_id,
            field_id,
            actor_id,
            body.value,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map(page)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}
