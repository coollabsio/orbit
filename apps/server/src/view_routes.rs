//! Saved task views (`/views`). Session-authenticated like the task routes; errors use
//! `task_problem`, so they share the `TaskProblem` schema.

use axum::extract::{Extension, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use orbit_platform::RequestId;
use serde::Deserialize;
use utoipa::ToSchema;

use crate::repositories::task_filter::ViewState;
use crate::repositories::views::{SavedViewRecord, ViewCreate, ViewUpdate, Visibility};
use crate::task_routes::{
    ApiError, ApiJson, TaskState, deserialize_source_patch, parse_id, request_id_value, scope,
    task_problem,
};

pub fn view_router(state: TaskState) -> Router {
    Router::new()
        .route(
            "/api/v1/workspaces/{workspace_id}/views",
            get(list_views).post(create_view),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/views/{view_id}",
            get(get_view).patch(update_view).delete(delete_view),
        )
        .with_state(state)
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct ViewCreateBody {
    /// 1–80 characters after trimming.
    name: String,
    /// Up to 500 characters.
    #[serde(default)]
    #[schema(required = false)]
    description: String,
    /// Icon name: ASCII letters, digits, `-` or `_`, up to 64 bytes.
    icon: Option<String>,
    /// `#rrggbb`.
    color: Option<String>,
    visibility: Visibility,
    state: ViewState,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct ViewUpdateBody {
    expected_version: i64,
    name: Option<String>,
    description: Option<String>,
    /// Absent: unchanged. `null`: clear.
    #[serde(default, deserialize_with = "deserialize_source_patch")]
    icon: Option<Option<String>>,
    /// Absent: unchanged. `null`: clear.
    #[serde(default, deserialize_with = "deserialize_source_patch")]
    color: Option<Option<String>>,
    /// Only the view's owner may change this.
    visibility: Option<Visibility>,
    state: Option<ViewState>,
}

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/views", params(("workspace_id" = String, Path)), responses((status = 200, body = Vec<SavedViewRecord>)))]
async fn list_views(
    State(state): State<TaskState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<Vec<SavedViewRecord>>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/views");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    state
        .views
        .list_views(workspace_id, actor_id)
        .await
        .map(Json)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/views", params(("workspace_id" = String, Path)), request_body = ViewCreateBody, responses((status = 201, body = SavedViewRecord)))]
async fn create_view(
    State(state): State<TaskState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<ViewCreateBody>,
) -> Result<Response, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/views");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let input = ViewCreate {
        name: body.name,
        description: body.description,
        icon: body.icon,
        color: body.color,
        visibility: body.visibility,
        state: body.state,
    };
    state
        .views
        .create_view(
            workspace_id,
            actor_id,
            input,
            request_id_value(request_id.as_ref()),
        )
        .await
        .map(|record| (StatusCode::CREATED, Json(record)).into_response())
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/views/{view_id}", params(("workspace_id" = String, Path), ("view_id" = String, Path)), responses((status = 200, body = SavedViewRecord)))]
async fn get_view(
    State(state): State<TaskState>,
    Path((workspace, view)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<SavedViewRecord>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/views/{view}");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let view_id = parse_id(&view, &instance, request_id.as_ref())?;
    state
        .views
        .get_view(workspace_id, actor_id, view_id)
        .await
        .map(Json)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(patch, path = "/api/v1/workspaces/{workspace_id}/views/{view_id}", params(("workspace_id" = String, Path), ("view_id" = String, Path)), request_body = ViewUpdateBody, responses((status = 200, body = SavedViewRecord)))]
async fn update_view(
    State(state): State<TaskState>,
    Path((workspace, view)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<ViewUpdateBody>,
) -> Result<Json<SavedViewRecord>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/views/{view}");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let view_id = parse_id(&view, &instance, request_id.as_ref())?;
    let input = ViewUpdate {
        expected_version: body.expected_version,
        name: body.name,
        description: body.description,
        icon: body.icon,
        color: body.color,
        visibility: body.visibility,
        state: body.state,
    };
    state
        .views
        .update_view(
            workspace_id,
            actor_id,
            view_id,
            input,
            request_id_value(request_id.as_ref()),
        )
        .await
        .map(Json)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(delete, path = "/api/v1/workspaces/{workspace_id}/views/{view_id}", params(("workspace_id" = String, Path), ("view_id" = String, Path)), responses((status = 204)))]
async fn delete_view(
    State(state): State<TaskState>,
    Path((workspace, view)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<StatusCode, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/views/{view}");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let view_id = parse_id(&view, &instance, request_id.as_ref())?;
    state
        .views
        .delete_view(
            workspace_id,
            actor_id,
            view_id,
            request_id_value(request_id.as_ref()),
        )
        .await
        .map(|()| StatusCode::NO_CONTENT)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}
