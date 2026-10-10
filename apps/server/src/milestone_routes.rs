//! Milestone routes (spec `2026-10-08-project-details-milestones-design.md`): milestone CRUD, the
//! update feed with health, and the owned description pages. Part of the task router.

use axum::extract::{Extension, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use orbit_platform::{RequestId, TimestampMillis};
use serde::Deserialize;
use utoipa::ToSchema;

use crate::repositories::milestones::{
    CreateMilestone, MilestoneChanges, MilestoneRecord, MilestoneUpdateRecord, OwnedPageRecord,
};
use crate::repositories::tasks::Page;
use crate::task_routes::{
    ApiError, ApiJson, ApiQuery, MutationQuery, TaskState, deserialize_due_patch, message,
    parse_id, request_id_value, scope, task_problem, text,
};

pub(crate) fn routes() -> Router<TaskState> {
    Router::new()
        .route(
            "/api/v1/workspaces/{workspace_id}/milestones",
            get(list_workspace_milestones),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/projects/{project_id}/overview-page",
            post(create_project_overview_page),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/projects/{project_id}/milestones",
            get(list_milestones).post(create_milestone),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/projects/{project_id}/milestones/{milestone_id}",
            patch(update_milestone).delete(delete_milestone),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/projects/{project_id}/milestones/{milestone_id}/description-page",
            post(create_milestone_description_page),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/projects/{project_id}/milestones/{milestone_id}/updates",
            get(list_milestone_updates).post(create_milestone_update),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/projects/{project_id}/milestones/{milestone_id}/updates/{update_id}",
            patch(update_milestone_update).delete(delete_milestone_update),
        )
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct MilestoneBody {
    name: String,
    /// `planned` (the default), `in_progress`, `completed` or `cancelled`.
    status: Option<String>,
    #[schema(value_type = Option<String>, format = DateTime)]
    start_at: Option<TimestampMillis>,
    #[schema(value_type = Option<String>, format = DateTime)]
    target_at: Option<TimestampMillis>,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct MilestoneUpdateBody {
    expected_version: u64,
    name: Option<String>,
    status: Option<String>,
    /// Absent: unchanged. `null`: no start date.
    #[serde(default, deserialize_with = "deserialize_due_patch")]
    #[schema(value_type = Option<String>, format = DateTime)]
    start_at: Option<Option<TimestampMillis>>,
    /// Absent: unchanged. `null`: no target date.
    #[serde(default, deserialize_with = "deserialize_due_patch")]
    #[schema(value_type = Option<String>, format = DateTime)]
    target_at: Option<Option<TimestampMillis>>,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct HealthUpdateBody {
    /// `on_track`, `at_risk` or `off_track`.
    health: String,
    /// Markdown.
    body: String,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct HealthUpdatePatchBody {
    expected_version: u64,
    health: String,
    body: String,
}

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/milestones", params(("workspace_id" = String, Path)), responses((status = 200, body = Page<MilestoneRecord>)))]
pub(crate) async fn list_workspace_milestones(
    State(state): State<TaskState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<Page<MilestoneRecord>>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/milestones");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    state
        .tasks
        .milestones(workspace_id, actor_id, None)
        .await
        .map(Json)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/projects/{project_id}/milestones", params(("workspace_id" = String, Path), ("project_id" = String, Path)), responses((status = 200, body = Page<MilestoneRecord>)))]
pub(crate) async fn list_milestones(
    State(state): State<TaskState>,
    Path((workspace, project)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<Page<MilestoneRecord>>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/projects/{project}/milestones");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let project_id = parse_id(&project, &instance, request_id.as_ref())?;
    state
        .tasks
        .milestones(workspace_id, actor_id, Some(project_id))
        .await
        .map(Json)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/projects/{project_id}/milestones", params(("workspace_id" = String, Path), ("project_id" = String, Path)), request_body = MilestoneBody, responses((status = 201, body = MilestoneRecord)))]
pub(crate) async fn create_milestone(
    State(state): State<TaskState>,
    Path((workspace, project)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<MilestoneBody>,
) -> Result<Response, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/projects/{project}/milestones");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let project_id = parse_id(&project, &instance, request_id.as_ref())?;
    let input = CreateMilestone {
        name: text(body.name, 200, 200, "name", &instance, request_id.as_ref())?,
        status: body.status.unwrap_or_else(|| "planned".to_owned()),
        start_at: body.start_at,
        target_at: body.target_at,
    };
    state
        .tasks
        .create_milestone(
            workspace_id,
            project_id,
            actor_id,
            input,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map(|record| (StatusCode::CREATED, Json(record)).into_response())
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(patch, path = "/api/v1/workspaces/{workspace_id}/projects/{project_id}/milestones/{milestone_id}", params(("workspace_id" = String, Path), ("project_id" = String, Path), ("milestone_id" = String, Path)), request_body = MilestoneUpdateBody, responses((status = 200, body = MilestoneRecord)))]
pub(crate) async fn update_milestone(
    State(state): State<TaskState>,
    Path((workspace, project, milestone)): Path<(String, String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<MilestoneUpdateBody>,
) -> Result<Json<MilestoneRecord>, ApiError> {
    let instance =
        format!("/api/v1/workspaces/{workspace}/projects/{project}/milestones/{milestone}");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let project_id = parse_id(&project, &instance, request_id.as_ref())?;
    let milestone_id = parse_id(&milestone, &instance, request_id.as_ref())?;
    let changes = MilestoneChanges {
        name: body
            .name
            .map(|name| text(name, 200, 200, "name", &instance, request_id.as_ref()))
            .transpose()?,
        status: body.status,
        start_at: body.start_at,
        target_at: body.target_at,
    };
    state
        .tasks
        .update_milestone(
            workspace_id,
            project_id,
            milestone_id,
            actor_id,
            changes,
            body.expected_version,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map(Json)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(delete, path = "/api/v1/workspaces/{workspace_id}/projects/{project_id}/milestones/{milestone_id}", params(MutationQuery, ("workspace_id" = String, Path), ("project_id" = String, Path), ("milestone_id" = String, Path)), responses((status = 204)))]
pub(crate) async fn delete_milestone(
    State(state): State<TaskState>,
    Path((workspace, project, milestone)): Path<(String, String, String)>,
    ApiQuery(query): ApiQuery<MutationQuery>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<StatusCode, ApiError> {
    let instance =
        format!("/api/v1/workspaces/{workspace}/projects/{project}/milestones/{milestone}");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let project_id = parse_id(&project, &instance, request_id.as_ref())?;
    let milestone_id = parse_id(&milestone, &instance, request_id.as_ref())?;
    state
        .tasks
        .delete_milestone(
            workspace_id,
            project_id,
            milestone_id,
            actor_id,
            query.expected_version,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map(|()| StatusCode::NO_CONTENT)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/projects/{project_id}/overview-page", params(("workspace_id" = String, Path), ("project_id" = String, Path)), responses((status = 200, body = OwnedPageRecord)))]
pub(crate) async fn create_project_overview_page(
    State(state): State<TaskState>,
    Path((workspace, project)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<OwnedPageRecord>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/projects/{project}/overview-page");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let project_id = parse_id(&project, &instance, request_id.as_ref())?;
    state
        .tasks
        .project_overview_page(
            workspace_id,
            project_id,
            actor_id,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map(Json)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/projects/{project_id}/milestones/{milestone_id}/description-page", params(("workspace_id" = String, Path), ("project_id" = String, Path), ("milestone_id" = String, Path)), responses((status = 200, body = OwnedPageRecord)))]
pub(crate) async fn create_milestone_description_page(
    State(state): State<TaskState>,
    Path((workspace, project, milestone)): Path<(String, String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<OwnedPageRecord>, ApiError> {
    let instance = format!(
        "/api/v1/workspaces/{workspace}/projects/{project}/milestones/{milestone}/description-page"
    );
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let project_id = parse_id(&project, &instance, request_id.as_ref())?;
    let milestone_id = parse_id(&milestone, &instance, request_id.as_ref())?;
    state
        .tasks
        .milestone_description_page(
            workspace_id,
            project_id,
            milestone_id,
            actor_id,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map(Json)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/projects/{project_id}/milestones/{milestone_id}/updates", params(("workspace_id" = String, Path), ("project_id" = String, Path), ("milestone_id" = String, Path)), responses((status = 200, body = Page<MilestoneUpdateRecord>)))]
pub(crate) async fn list_milestone_updates(
    State(state): State<TaskState>,
    Path((workspace, project, milestone)): Path<(String, String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<Page<MilestoneUpdateRecord>>, ApiError> {
    let instance =
        format!("/api/v1/workspaces/{workspace}/projects/{project}/milestones/{milestone}/updates");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let project_id = parse_id(&project, &instance, request_id.as_ref())?;
    let milestone_id = parse_id(&milestone, &instance, request_id.as_ref())?;
    state
        .tasks
        .milestone_updates(workspace_id, project_id, milestone_id, actor_id)
        .await
        .map(Json)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/projects/{project_id}/milestones/{milestone_id}/updates", params(("workspace_id" = String, Path), ("project_id" = String, Path), ("milestone_id" = String, Path)), request_body = HealthUpdateBody, responses((status = 201, body = MilestoneUpdateRecord)))]
pub(crate) async fn create_milestone_update(
    State(state): State<TaskState>,
    Path((workspace, project, milestone)): Path<(String, String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<HealthUpdateBody>,
) -> Result<Response, ApiError> {
    let instance =
        format!("/api/v1/workspaces/{workspace}/projects/{project}/milestones/{milestone}/updates");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let project_id = parse_id(&project, &instance, request_id.as_ref())?;
    let milestone_id = parse_id(&milestone, &instance, request_id.as_ref())?;
    let text = message(
        body.body,
        20_000,
        20_000,
        "body",
        &instance,
        request_id.as_ref(),
    )?;
    state
        .tasks
        .create_milestone_update(
            workspace_id,
            project_id,
            milestone_id,
            actor_id,
            body.health,
            text,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map(|record| (StatusCode::CREATED, Json(record)).into_response())
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(patch, path = "/api/v1/workspaces/{workspace_id}/projects/{project_id}/milestones/{milestone_id}/updates/{update_id}", params(("workspace_id" = String, Path), ("project_id" = String, Path), ("milestone_id" = String, Path), ("update_id" = String, Path)), request_body = HealthUpdatePatchBody, responses((status = 200, body = MilestoneUpdateRecord)))]
pub(crate) async fn update_milestone_update(
    State(state): State<TaskState>,
    Path((workspace, project, milestone, update)): Path<(String, String, String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<HealthUpdatePatchBody>,
) -> Result<Json<MilestoneUpdateRecord>, ApiError> {
    let instance = format!(
        "/api/v1/workspaces/{workspace}/projects/{project}/milestones/{milestone}/updates/{update}"
    );
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let project_id = parse_id(&project, &instance, request_id.as_ref())?;
    let milestone_id = parse_id(&milestone, &instance, request_id.as_ref())?;
    let update_id = parse_id(&update, &instance, request_id.as_ref())?;
    let text = message(
        body.body,
        20_000,
        20_000,
        "body",
        &instance,
        request_id.as_ref(),
    )?;
    state
        .tasks
        .update_milestone_update(
            workspace_id,
            project_id,
            milestone_id,
            update_id,
            actor_id,
            body.health,
            text,
            body.expected_version,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map(Json)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(delete, path = "/api/v1/workspaces/{workspace_id}/projects/{project_id}/milestones/{milestone_id}/updates/{update_id}", params(MutationQuery, ("workspace_id" = String, Path), ("project_id" = String, Path), ("milestone_id" = String, Path), ("update_id" = String, Path)), responses((status = 204)))]
pub(crate) async fn delete_milestone_update(
    State(state): State<TaskState>,
    Path((workspace, project, milestone, update)): Path<(String, String, String, String)>,
    ApiQuery(query): ApiQuery<MutationQuery>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<StatusCode, ApiError> {
    let instance = format!(
        "/api/v1/workspaces/{workspace}/projects/{project}/milestones/{milestone}/updates/{update}"
    );
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let project_id = parse_id(&project, &instance, request_id.as_ref())?;
    let milestone_id = parse_id(&milestone, &instance, request_id.as_ref())?;
    let update_id = parse_id(&update, &instance, request_id.as_ref())?;
    state
        .tasks
        .delete_milestone_update(
            workspace_id,
            project_id,
            milestone_id,
            update_id,
            actor_id,
            query.expected_version,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map(|()| StatusCode::NO_CONTENT)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}
