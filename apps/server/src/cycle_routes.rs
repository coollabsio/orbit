//! Cycle routes (spec `2026-10-08-cycles-estimates-design.md`): the cycle settings of a project,
//! its cycles with their totals, and the manual actions. Part of the task router.

use axum::extract::{Extension, Path, State};
use axum::http::HeaderMap;
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use orbit_platform::{RequestId, TimestampMillis};
use serde::Deserialize;
use utoipa::ToSchema;

use crate::repositories::cycles::{
    CycleChanges, CycleDayRecord, CycleRecord, CycleSettingsInput, CycleSettingsRecord,
};
use crate::repositories::tasks::Page;
use crate::task_routes::{
    ApiError, ApiJson, TaskState, bounded, deserialize_source_patch, parse_id, request_id_value,
    scope, task_problem, text,
};

pub(crate) fn routes() -> Router<TaskState> {
    Router::new()
        .route(
            "/api/v1/workspaces/{workspace_id}/cycles/current",
            get(list_current_cycles),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/projects/{project_id}/cycle-settings",
            get(get_cycle_settings).put(update_cycle_settings),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/projects/{project_id}/cycles",
            get(list_cycles),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/projects/{project_id}/cycles/{cycle_id}",
            patch(update_cycle),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/projects/{project_id}/cycles/{cycle_id}/burndown",
            get(list_cycle_days),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/projects/{project_id}/cycles/{cycle_id}/start-today",
            post(start_cycle_today),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/projects/{project_id}/cycles/{cycle_id}/end-today",
            post(end_cycle_today),
        )
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct CycleSettingsBody {
    expected_version: u64,
    enabled: bool,
    weeks: i64,
    /// 0 = Sunday ... 6 = Saturday.
    start_weekday: i64,
    cooldown_weeks: i64,
    cycles_ahead: i64,
    /// IANA name, for example `Europe/Berlin`.
    timezone: String,
    auto_add_started: bool,
    auto_add_completed: bool,
    /// `off`, `backlog` or `cycle`.
    active_without_cycle: String,
}

/// Absent fields are unchanged.
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct CycleUpdateBody {
    expected_version: u64,
    /// `null`: the default name "Cycle {number}".
    #[serde(default, deserialize_with = "deserialize_source_patch")]
    name: Option<Option<String>>,
    description: Option<String>,
    /// A future cycle only.
    #[schema(value_type = Option<String>, format = DateTime)]
    starts_at: Option<TimestampMillis>,
    /// A future cycle only. The later future cycles move by the same amount.
    #[schema(value_type = Option<String>, format = DateTime)]
    ends_at: Option<TimestampMillis>,
}

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/cycles/current", params(("workspace_id" = String, Path)), responses((status = 200, body = Page<CycleRecord>)))]
pub(crate) async fn list_current_cycles(
    State(state): State<TaskState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<Page<CycleRecord>>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/cycles/current");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    state
        .tasks
        .current_cycles(workspace_id, actor_id, TimestampMillis::now())
        .await
        .map(Json)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/projects/{project_id}/cycle-settings", params(("workspace_id" = String, Path), ("project_id" = String, Path)), responses((status = 200, body = CycleSettingsRecord)))]
pub(crate) async fn get_cycle_settings(
    State(state): State<TaskState>,
    Path((workspace, project)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<CycleSettingsRecord>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/projects/{project}/cycle-settings");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let project_id = parse_id(&project, &instance, request_id.as_ref())?;
    state
        .tasks
        .cycle_settings(workspace_id, project_id, actor_id)
        .await
        .map(Json)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(put, path = "/api/v1/workspaces/{workspace_id}/projects/{project_id}/cycle-settings", params(("workspace_id" = String, Path), ("project_id" = String, Path)), request_body = CycleSettingsBody, responses((status = 200, body = CycleSettingsRecord)))]
pub(crate) async fn update_cycle_settings(
    State(state): State<TaskState>,
    Path((workspace, project)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<CycleSettingsBody>,
) -> Result<Json<CycleSettingsRecord>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/projects/{project}/cycle-settings");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let project_id = parse_id(&project, &instance, request_id.as_ref())?;
    state
        .tasks
        .update_cycle_settings(
            workspace_id,
            project_id,
            actor_id,
            CycleSettingsInput {
                enabled: body.enabled,
                weeks: body.weeks,
                start_weekday: body.start_weekday,
                cooldown_weeks: body.cooldown_weeks,
                cycles_ahead: body.cycles_ahead,
                timezone: body.timezone,
                auto_add_started: body.auto_add_started,
                auto_add_completed: body.auto_add_completed,
                active_without_cycle: body.active_without_cycle,
            },
            body.expected_version,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map(Json)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/projects/{project_id}/cycles", params(("workspace_id" = String, Path), ("project_id" = String, Path)), responses((status = 200, body = Page<CycleRecord>)))]
pub(crate) async fn list_cycles(
    State(state): State<TaskState>,
    Path((workspace, project)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<Page<CycleRecord>>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/projects/{project}/cycles");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let project_id = parse_id(&project, &instance, request_id.as_ref())?;
    state
        .tasks
        .cycles(workspace_id, project_id, actor_id, TimestampMillis::now())
        .await
        .map(Json)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/projects/{project_id}/cycles/{cycle_id}/burndown", params(("workspace_id" = String, Path), ("project_id" = String, Path), ("cycle_id" = String, Path)), responses((status = 200, body = Page<CycleDayRecord>)))]
pub(crate) async fn list_cycle_days(
    State(state): State<TaskState>,
    Path((workspace, project, cycle)): Path<(String, String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<Page<CycleDayRecord>>, ApiError> {
    let instance =
        format!("/api/v1/workspaces/{workspace}/projects/{project}/cycles/{cycle}/burndown");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let project_id = parse_id(&project, &instance, request_id.as_ref())?;
    let cycle_id = parse_id(&cycle, &instance, request_id.as_ref())?;
    state
        .tasks
        .cycle_days(workspace_id, project_id, cycle_id, actor_id)
        .await
        .map(Json)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(patch, path = "/api/v1/workspaces/{workspace_id}/projects/{project_id}/cycles/{cycle_id}", params(("workspace_id" = String, Path), ("project_id" = String, Path), ("cycle_id" = String, Path)), request_body = CycleUpdateBody, responses((status = 200, body = CycleRecord)))]
pub(crate) async fn update_cycle(
    State(state): State<TaskState>,
    Path((workspace, project, cycle)): Path<(String, String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<CycleUpdateBody>,
) -> Result<Json<CycleRecord>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/projects/{project}/cycles/{cycle}");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let project_id = parse_id(&project, &instance, request_id.as_ref())?;
    let cycle_id = parse_id(&cycle, &instance, request_id.as_ref())?;
    let name = match body.name {
        Some(Some(name)) if !name.trim().is_empty() => Some(Some(text(
            name,
            200,
            200,
            "name",
            &instance,
            request_id.as_ref(),
        )?)),
        Some(_) => Some(None),
        None => None,
    };
    let changes = CycleChanges {
        name,
        description: body
            .description
            .map(|value| {
                bounded(
                    value,
                    20_000,
                    20_000,
                    "description",
                    &instance,
                    request_id.as_ref(),
                )
            })
            .transpose()?,
        starts_at: body.starts_at,
        ends_at: body.ends_at,
    };
    state
        .tasks
        .update_cycle(
            workspace_id,
            project_id,
            cycle_id,
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

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/projects/{project_id}/cycles/{cycle_id}/start-today", params(("workspace_id" = String, Path), ("project_id" = String, Path), ("cycle_id" = String, Path)), responses((status = 200, body = CycleRecord)))]
pub(crate) async fn start_cycle_today(
    State(state): State<TaskState>,
    Path((workspace, project, cycle)): Path<(String, String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<CycleRecord>, ApiError> {
    let instance =
        format!("/api/v1/workspaces/{workspace}/projects/{project}/cycles/{cycle}/start-today");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let project_id = parse_id(&project, &instance, request_id.as_ref())?;
    let cycle_id = parse_id(&cycle, &instance, request_id.as_ref())?;
    state
        .tasks
        .start_cycle_today(
            workspace_id,
            project_id,
            cycle_id,
            actor_id,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map(Json)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/projects/{project_id}/cycles/{cycle_id}/end-today", params(("workspace_id" = String, Path), ("project_id" = String, Path), ("cycle_id" = String, Path)), responses((status = 200, body = CycleRecord)))]
pub(crate) async fn end_cycle_today(
    State(state): State<TaskState>,
    Path((workspace, project, cycle)): Path<(String, String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<CycleRecord>, ApiError> {
    let instance =
        format!("/api/v1/workspaces/{workspace}/projects/{project}/cycles/{cycle}/end-today");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let project_id = parse_id(&project, &instance, request_id.as_ref())?;
    let cycle_id = parse_id(&cycle, &instance, request_id.as_ref())?;
    state
        .tasks
        .end_cycle_today(
            workspace_id,
            project_id,
            cycle_id,
            actor_id,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map(Json)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}
