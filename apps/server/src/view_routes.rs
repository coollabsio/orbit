//! Saved task views (`/views`). Session-authenticated like the task routes; errors use
//! `task_problem`, so they share the `TaskProblem` schema.

use axum::extract::{Extension, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, put};
use axum::{Json, Router};
use orbit_platform::{Id, RequestId};
use serde::Deserialize;
use utoipa::ToSchema;

use crate::repositories::task_filter::ViewState;
use crate::repositories::tasks::TaskError;
use crate::repositories::views::{
    SavedViewRecord, ViewCreate, ViewPreferenceRecord, ViewUpdate, Visibility,
};
use crate::task_routes::{
    ApiError, ApiJson, TaskState, deserialize_source_patch, parse_id, request_id_value, scope,
    task_problem,
};

/// Upper bound for one favorites reorder; far above any real sidebar.
const MAX_FAVORITE_IDS: usize = 500;

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
        .route(
            "/api/v1/workspaces/{workspace_id}/views/{view_id}/favorite",
            put(favorite_view).delete(unfavorite_view),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/view-favorites/order",
            put(reorder_view_favorites),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/view-preferences/{page_key}",
            get(get_view_preference).put(put_view_preference),
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

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct ViewFavoritesOrderBody {
    /// Every view the caller has favorited in this workspace, in the new order.
    view_ids: Vec<String>,
}

#[utoipa::path(put, path = "/api/v1/workspaces/{workspace_id}/views/{view_id}/favorite", params(("workspace_id" = String, Path), ("view_id" = String, Path)), responses((status = 204)))]
async fn favorite_view(
    State(state): State<TaskState>,
    Path((workspace, view)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<StatusCode, ApiError> {
    change_favorite(state, workspace, view, headers, request_id, true).await
}

#[utoipa::path(delete, path = "/api/v1/workspaces/{workspace_id}/views/{view_id}/favorite", params(("workspace_id" = String, Path), ("view_id" = String, Path)), responses((status = 204)))]
async fn unfavorite_view(
    State(state): State<TaskState>,
    Path((workspace, view)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<StatusCode, ApiError> {
    change_favorite(state, workspace, view, headers, request_id, false).await
}

async fn change_favorite(
    state: TaskState,
    workspace: String,
    view: String,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    favorite: bool,
) -> Result<StatusCode, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/views/{view}/favorite");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let view_id = parse_id(&view, &instance, request_id.as_ref())?;
    state
        .views
        .set_favorite(
            workspace_id,
            actor_id,
            view_id,
            favorite,
            request_id_value(request_id.as_ref()),
        )
        .await
        .map(|()| StatusCode::NO_CONTENT)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(put, path = "/api/v1/workspaces/{workspace_id}/view-favorites/order", params(("workspace_id" = String, Path)), request_body = ViewFavoritesOrderBody, responses((status = 204)))]
async fn reorder_view_favorites(
    State(state): State<TaskState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<ViewFavoritesOrderBody>,
) -> Result<StatusCode, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/view-favorites/order");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let view_ids = parse_view_ids(&body.view_ids)
        .map_err(|error| task_problem(error, instance.clone(), request_id.as_ref()))?;
    state
        .views
        .reorder_favorites(
            workspace_id,
            actor_id,
            view_ids,
            request_id_value(request_id.as_ref()),
        )
        .await
        .map(|()| StatusCode::NO_CONTENT)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

fn parse_view_ids(values: &[String]) -> Result<Vec<Id>, TaskError> {
    if values.len() > MAX_FAVORITE_IDS {
        return Err(TaskError::Invalid { field: "view_ids" });
    }
    values
        .iter()
        .map(|value| {
            value
                .parse::<Id>()
                .map_err(|_| TaskError::Invalid { field: "view_ids" })
        })
        .collect()
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct ViewPreferenceBody {
    state: ViewState,
}

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/view-preferences/{page_key}", params(("workspace_id" = String, Path), ("page_key" = String, Path, description = "`all`, `project:<project_id>` or `preset:<mine|overdue|due_soon|current_week|my_week>`")), responses((status = 200, body = ViewPreferenceRecord)))]
async fn get_view_preference(
    State(state): State<TaskState>,
    Path((workspace, page_key)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<ViewPreferenceRecord>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/view-preferences/{page_key}");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    state
        .views
        .get_preference(workspace_id, actor_id, &page_key)
        .await
        .map(Json)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(put, path = "/api/v1/workspaces/{workspace_id}/view-preferences/{page_key}", params(("workspace_id" = String, Path), ("page_key" = String, Path, description = "`all`, `project:<project_id>` or `preset:<mine|overdue|due_soon|current_week|my_week>`")), request_body = ViewPreferenceBody, responses((status = 200, body = ViewPreferenceRecord)))]
async fn put_view_preference(
    State(state): State<TaskState>,
    Path((workspace, page_key)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<ViewPreferenceBody>,
) -> Result<Json<ViewPreferenceRecord>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/view-preferences/{page_key}");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    state
        .views
        .put_preference(workspace_id, actor_id, &page_key, body.state)
        .await
        .map(Json)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}
