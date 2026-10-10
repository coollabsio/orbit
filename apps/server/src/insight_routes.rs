//! Insight routes (spec `2026-10-08-insights-design.md`): the numbers behind the project charts.
//! All workspace members can read them. Part of the task router.

use axum::extract::{Extension, Path, State};
use axum::http::HeaderMap;
use axum::routing::get;
use axum::{Json, Router};
use orbit_platform::{RequestId, TimestampMillis};
use serde::Deserialize;
use utoipa::{IntoParams, ToSchema};

use crate::repositories::insights::{BurnupWeek, Insight, OpenBy, OpenGroup, ThroughputWeek};
use crate::task_routes::{ApiError, ApiQuery, TaskState, parse_id, scope, task_problem};

pub(crate) fn routes() -> Router<TaskState> {
    Router::new()
        .route(
            "/api/v1/workspaces/{workspace_id}/projects/{project_id}/insights/throughput",
            get(get_insight_throughput),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/projects/{project_id}/insights/open",
            get(get_insight_open),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/projects/{project_id}/milestones/{milestone_id}/burnup",
            get(get_milestone_burnup),
        )
}

fn default_timezone() -> String {
    "UTC".to_owned()
}

fn default_weeks() -> i64 {
    12
}

#[derive(Deserialize, IntoParams, ToSchema)]
#[into_params(parameter_in = Query)]
#[serde(deny_unknown_fields)]
pub(crate) struct ThroughputQuery {
    /// The IANA timezone of the viewer; a week starts on Monday in it. Default `UTC`.
    #[serde(default = "default_timezone")]
    #[param(required = false)]
    tz: String,
    /// 1 to 104 (default 12).
    #[serde(default = "default_weeks")]
    #[param(required = false)]
    weeks: i64,
}

#[derive(Deserialize, IntoParams, ToSchema)]
#[into_params(parameter_in = Query)]
#[serde(deny_unknown_fields)]
pub(crate) struct TimezoneQuery {
    /// The IANA timezone of the viewer; a week starts on Monday in it. Default `UTC`.
    #[serde(default = "default_timezone")]
    #[param(required = false)]
    tz: String,
}

#[derive(Deserialize, IntoParams, ToSchema)]
#[into_params(parameter_in = Query)]
#[serde(deny_unknown_fields)]
pub(crate) struct OpenQuery {
    /// `status`, `assignee`, `priority` or `label`.
    by: String,
}

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/projects/{project_id}/insights/throughput", params(ThroughputQuery, ("workspace_id" = String, Path), ("project_id" = String, Path)), responses((status = 200, body = Insight<ThroughputWeek>)))]
pub(crate) async fn get_insight_throughput(
    State(state): State<TaskState>,
    Path((workspace, project)): Path<(String, String)>,
    ApiQuery(query): ApiQuery<ThroughputQuery>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<Insight<ThroughputWeek>>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/projects/{project}/insights/throughput");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let project_id = parse_id(&project, &instance, request_id.as_ref())?;
    state
        .tasks
        .insight_throughput(
            workspace_id,
            project_id,
            actor_id,
            &query.tz,
            query.weeks,
            TimestampMillis::now(),
        )
        .await
        .map(Json)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/projects/{project_id}/insights/open", params(OpenQuery, ("workspace_id" = String, Path), ("project_id" = String, Path)), responses((status = 200, body = Insight<OpenGroup>)))]
pub(crate) async fn get_insight_open(
    State(state): State<TaskState>,
    Path((workspace, project)): Path<(String, String)>,
    ApiQuery(query): ApiQuery<OpenQuery>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<Insight<OpenGroup>>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/projects/{project}/insights/open");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let project_id = parse_id(&project, &instance, request_id.as_ref())?;
    let by = OpenBy::parse(&query.by)
        .map_err(|error| task_problem(error, instance.clone(), request_id.as_ref()))?;
    state
        .tasks
        .insight_open(workspace_id, project_id, actor_id, by)
        .await
        .map(Json)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/projects/{project_id}/milestones/{milestone_id}/burnup", params(TimezoneQuery, ("workspace_id" = String, Path), ("project_id" = String, Path), ("milestone_id" = String, Path)), responses((status = 200, body = Insight<BurnupWeek>)))]
pub(crate) async fn get_milestone_burnup(
    State(state): State<TaskState>,
    Path((workspace, project, milestone)): Path<(String, String, String)>,
    ApiQuery(query): ApiQuery<TimezoneQuery>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<Insight<BurnupWeek>>, ApiError> {
    let instance =
        format!("/api/v1/workspaces/{workspace}/projects/{project}/milestones/{milestone}/burnup");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let project_id = parse_id(&project, &instance, request_id.as_ref())?;
    let milestone_id = parse_id(&milestone, &instance, request_id.as_ref())?;
    state
        .tasks
        .insight_burnup(
            workspace_id,
            project_id,
            milestone_id,
            actor_id,
            &query.tz,
            TimestampMillis::now(),
        )
        .await
        .map(Json)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}
