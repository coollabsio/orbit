//! Milestone and cycle writes, and status and label deletes. A date field takes `YYYY-MM-DD` or
//! an RFC 3339 time, as the MCP tools do.
use axum::Json;
use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use orbit_platform::{Id, TimestampMillis};
use serde::Deserialize;
use utoipa::ToSchema;

use super::{
    Ctx, CycleOut, ExpectedVersionQuery, MilestoneOut, PublicApiState, present, problem_response,
};
use crate::repositories::api_tokens::ApiTokenScope;
use crate::repositories::tasks::{CycleChanges, MilestoneChanges};
use crate::token_api::{NOON_MS, TokenApiError, parse_plan_time};

#[derive(Deserialize, ToSchema)]
#[schema(as = PublicCreateMilestoneBody)]
#[serde(deny_unknown_fields)]
pub(super) struct CreateMilestoneBody {
    /// 1 to 200 characters, unique in the project.
    name: String,
    /// Up to 20,000 characters.
    description: Option<String>,
    /// The target date: `YYYY-MM-DD` (stored at noon UTC, as the web app does) or an RFC 3339
    /// time such as `2026-12-01T12:00:00Z`.
    target_at: Option<String>,
}

/// A missing field keeps its value.
#[derive(Deserialize, ToSchema)]
#[schema(as = PublicUpdateMilestoneBody)]
#[serde(deny_unknown_fields)]
pub(super) struct UpdateMilestoneBody {
    /// The `version` that you read.
    expected_version: u64,
    /// 1 to 200 characters, unique in the project.
    name: Option<String>,
    /// Up to 20,000 characters; `null` or blank clears it.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = Option<String>)]
    description: Option<Option<String>>,
    /// `YYYY-MM-DD` (noon UTC) or an RFC 3339 time; `null` clears it.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = Option<String>)]
    target_at: Option<Option<String>>,
    /// The milestone's place in the project's order (0 is first).
    position: Option<i64>,
}

#[derive(Deserialize, ToSchema)]
#[schema(as = PublicCreateCycleBody)]
#[serde(deny_unknown_fields)]
pub(super) struct CreateCycleBody {
    /// Up to 200 characters. Without one, the cycle shows as its number.
    name: Option<String>,
    /// The first instant: an RFC 3339 time, or `YYYY-MM-DD` for midnight UTC. Default: when the
    /// project's last cycle ends, or today's UTC midnight for the first cycle.
    starts_at: Option<String>,
    /// The end, exclusive (`YYYY-MM-DD` is midnight UTC, so the day before is the last one).
    /// Default: as many days after `starts_at` as the last cycle lasted, or 14 for the first.
    /// Cycles of a project cannot overlap; they may touch.
    ends_at: Option<String>,
}

/// A missing field keeps its value.
#[derive(Deserialize, ToSchema)]
#[schema(as = PublicUpdateCycleBody)]
#[serde(deny_unknown_fields)]
pub(super) struct UpdateCycleBody {
    /// The `version` that you read.
    expected_version: u64,
    /// Up to 200 characters; `null` or blank clears it, and the cycle shows as its number.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = Option<String>)]
    name: Option<Option<String>>,
    /// An RFC 3339 time, or `YYYY-MM-DD` for midnight UTC.
    starts_at: Option<String>,
    /// The end, exclusive: an RFC 3339 time, or `YYYY-MM-DD` for midnight UTC.
    ends_at: Option<String>,
}

/// A date field as [`parse_plan_time`] reads it, or 400 with `invalid`.
fn plan_time(
    raw: Option<&str>,
    day_offset_ms: i64,
    invalid: &'static str,
) -> Result<Option<TimestampMillis>, TokenApiError> {
    raw.map(|raw| parse_plan_time(raw, day_offset_ms).ok_or(TokenApiError::Invalid(invalid)))
        .transpose()
}

const TARGET_AT: &str = "target_at must be YYYY-MM-DD or an RFC 3339 time";
const STARTS_AT: &str = "starts_at must be YYYY-MM-DD or an RFC 3339 time";
const ENDS_AT: &str = "ends_at must be YYYY-MM-DD or an RFC 3339 time";

fn no_content(ctx: &Ctx, result: Result<(), TokenApiError>) -> Response {
    match result {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => ctx.error(error),
    }
}

#[utoipa::path(
    post,
    path = "/api/public/v1/workspaces/{workspace_id}/projects/{project_id}/milestones",
    operation_id = "public_create_milestone",
    tag = "public-api",
    summary = "Add a milestone after the others of a project",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("project_id" = String, Path)),
    request_body = CreateMilestoneBody,
    responses((status = 201, description = "The new milestone", body = MilestoneOut)),
    security(("bearerAuth" = []))
)]
pub(super) async fn create_milestone(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, project)): Path<(String, String)>,
    body: Result<Json<CreateMilestoneBody>, JsonRejection>,
) -> Response {
    let project = path_id!(ctx, project);
    let body = input!(ctx, ApiTokenScope::TasksWrite, body);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::TasksWrite, &ctx.request_id)
            .await?;
        let target_at = plan_time(body.target_at.as_deref(), NOON_MS, TARGET_AT)?;
        api.create_milestone(project, &body.name, body.description, target_at)
            .await
            .map(MilestoneOut::from)
    }
    .await;
    ctx.reply(StatusCode::CREATED, result)
}

#[utoipa::path(
    patch,
    path = "/api/public/v1/workspaces/{workspace_id}/projects/{project_id}/milestones/{milestone_id}",
    operation_id = "public_update_milestone",
    tag = "public-api",
    summary = "Change a milestone: name, description, target date or position",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("project_id" = String, Path), ("milestone_id" = String, Path)),
    request_body = UpdateMilestoneBody,
    responses((status = 200, description = "The milestone", body = MilestoneOut)),
    security(("bearerAuth" = []))
)]
pub(super) async fn update_milestone(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, project, milestone)): Path<(String, String, String)>,
    body: Result<Json<UpdateMilestoneBody>, JsonRejection>,
) -> Response {
    let project = path_id!(ctx, project);
    let milestone = path_id!(ctx, milestone);
    let body = input!(ctx, ApiTokenScope::TasksWrite, body);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::TasksWrite, &ctx.request_id)
            .await?;
        let target_at = match body.target_at {
            Some(value) => Some(plan_time(value.as_deref(), NOON_MS, TARGET_AT)?),
            None => None,
        };
        let changes = MilestoneChanges {
            name: body.name,
            description: body.description,
            target_at,
            position: body.position,
        };
        api.update_milestone(project, milestone, body.expected_version, changes)
            .await
            .map(MilestoneOut::from)
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    delete,
    path = "/api/public/v1/workspaces/{workspace_id}/projects/{project_id}/milestones/{milestone_id}",
    operation_id = "public_delete_milestone",
    tag = "public-api",
    summary = "Delete a milestone",
    description = "Its tasks stay, without a milestone.",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("project_id" = String, Path), ("milestone_id" = String, Path), ExpectedVersionQuery),
    responses((status = 204, description = "The milestone is deleted")),
    security(("bearerAuth" = []))
)]
pub(super) async fn delete_milestone(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, project, milestone)): Path<(String, String, String)>,
    query: Result<Query<ExpectedVersionQuery>, QueryRejection>,
) -> Response {
    let project = path_id!(ctx, project);
    let milestone = path_id!(ctx, milestone);
    let query = input!(ctx, ApiTokenScope::TasksWrite, query);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::TasksWrite, &ctx.request_id)
            .await?;
        api.delete_milestone(project, milestone, query.expected_version)
            .await
    }
    .await;
    no_content(&ctx, result)
}

#[utoipa::path(
    post,
    path = "/api/public/v1/workspaces/{workspace_id}/projects/{project_id}/cycles",
    operation_id = "public_create_cycle",
    tag = "public-api",
    summary = "Add the next-numbered cycle to a project",
    description = "Missing dates continue after the last cycle with its length, as the web app does. A cycle that overlaps another answers `409`.",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("project_id" = String, Path)),
    request_body = CreateCycleBody,
    responses((status = 201, description = "The new cycle", body = CycleOut)),
    security(("bearerAuth" = []))
)]
pub(super) async fn create_cycle(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, project)): Path<(String, String)>,
    body: Result<Json<CreateCycleBody>, JsonRejection>,
) -> Response {
    let project = path_id!(ctx, project);
    let body = input!(ctx, ApiTokenScope::TasksWrite, body);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::TasksWrite, &ctx.request_id)
            .await?;
        let starts_at = plan_time(body.starts_at.as_deref(), 0, STARTS_AT)?;
        let ends_at = plan_time(body.ends_at.as_deref(), 0, ENDS_AT)?;
        api.create_cycle(project, body.name, starts_at, ends_at)
            .await
            .map(CycleOut::from)
    }
    .await;
    ctx.reply(StatusCode::CREATED, result)
}

#[utoipa::path(
    patch,
    path = "/api/public/v1/workspaces/{workspace_id}/projects/{project_id}/cycles/{cycle_id}",
    operation_id = "public_update_cycle",
    tag = "public-api",
    summary = "Rename a cycle or move its dates",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("project_id" = String, Path), ("cycle_id" = String, Path)),
    request_body = UpdateCycleBody,
    responses((status = 200, description = "The cycle", body = CycleOut)),
    security(("bearerAuth" = []))
)]
pub(super) async fn update_cycle(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, project, cycle)): Path<(String, String, String)>,
    body: Result<Json<UpdateCycleBody>, JsonRejection>,
) -> Response {
    let project = path_id!(ctx, project);
    let cycle = path_id!(ctx, cycle);
    let body = input!(ctx, ApiTokenScope::TasksWrite, body);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::TasksWrite, &ctx.request_id)
            .await?;
        let changes = CycleChanges {
            name: body.name,
            starts_at: plan_time(body.starts_at.as_deref(), 0, STARTS_AT)?,
            ends_at: plan_time(body.ends_at.as_deref(), 0, ENDS_AT)?,
        };
        api.update_cycle(project, cycle, body.expected_version, changes)
            .await
            .map(CycleOut::from)
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    delete,
    path = "/api/public/v1/workspaces/{workspace_id}/projects/{project_id}/cycles/{cycle_id}",
    operation_id = "public_delete_cycle",
    tag = "public-api",
    summary = "Delete a cycle",
    description = "Its tasks stay, without a cycle.",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("project_id" = String, Path), ("cycle_id" = String, Path), ExpectedVersionQuery),
    responses((status = 204, description = "The cycle is deleted")),
    security(("bearerAuth" = []))
)]
pub(super) async fn delete_cycle(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, project, cycle)): Path<(String, String, String)>,
    query: Result<Query<ExpectedVersionQuery>, QueryRejection>,
) -> Response {
    let project = path_id!(ctx, project);
    let cycle = path_id!(ctx, cycle);
    let query = input!(ctx, ApiTokenScope::TasksWrite, query);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::TasksWrite, &ctx.request_id)
            .await?;
        api.delete_cycle(project, cycle, query.expected_version)
            .await
    }
    .await;
    no_content(&ctx, result)
}

#[utoipa::path(
    delete,
    path = "/api/public/v1/workspaces/{workspace_id}/projects/{project_id}/statuses/{status_id}",
    operation_id = "public_delete_status",
    tag = "public-api",
    summary = "Delete a status of a project",
    description = "No task may use it, trashed tasks included; the Duplicate status cannot be deleted. The token's creator must be allowed to manage the project.",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("project_id" = String, Path), ("status_id" = String, Path), ExpectedVersionQuery),
    responses((status = 204, description = "The status is deleted")),
    security(("bearerAuth" = []))
)]
pub(super) async fn delete_status(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, project, status)): Path<(String, String, String)>,
    query: Result<Query<ExpectedVersionQuery>, QueryRejection>,
) -> Response {
    let project = path_id!(ctx, project);
    let status = path_id!(ctx, status);
    let query = input!(ctx, ApiTokenScope::TasksWrite, query);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::TasksWrite, &ctx.request_id)
            .await?;
        api.delete_status(project, status, query.expected_version)
            .await
    }
    .await;
    no_content(&ctx, result)
}

#[utoipa::path(
    delete,
    path = "/api/public/v1/workspaces/{workspace_id}/projects/{project_id}/labels/{label_id}",
    operation_id = "public_delete_label",
    tag = "public-api",
    summary = "Delete a label of a project",
    description = "Takes the label off its tasks. The token's creator must be allowed to manage the project.",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("project_id" = String, Path), ("label_id" = String, Path), ExpectedVersionQuery),
    responses((status = 204, description = "The label is deleted")),
    security(("bearerAuth" = []))
)]
pub(super) async fn delete_label(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, project, label)): Path<(String, String, String)>,
    query: Result<Query<ExpectedVersionQuery>, QueryRejection>,
) -> Response {
    let project = path_id!(ctx, project);
    let label = path_id!(ctx, label);
    let query = input!(ctx, ApiTokenScope::TasksWrite, query);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::TasksWrite, &ctx.request_id)
            .await?;
        api.delete_label(project, label, query.expected_version)
            .await
    }
    .await;
    no_content(&ctx, result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_dates_take_a_day_or_a_time() {
        let noon = plan_time(Some("2026-12-01"), NOON_MS, TARGET_AT).unwrap();
        let time = plan_time(Some("2026-12-01T12:00:00Z"), 0, TARGET_AT).unwrap();
        assert_eq!(noon, time);
        assert!(matches!(
            plan_time(Some("next week"), 0, ENDS_AT),
            Err(TokenApiError::Invalid(ENDS_AT))
        ));
        assert!(plan_time(None, 0, ENDS_AT).unwrap().is_none());
    }
}
