//! Time logged on tasks. Everyone logs, edits and deletes their own time; `time.manage` (owners
//! and admins) covers other members' time.
use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use utoipa::ToSchema;

use super::{Ctx, PublicApiState, problem_response};
use crate::repositories::api_tokens::ApiTokenScope;
use crate::repositories::time_entries::{
    NewTimeEntry, TaskTimeEntries, TimeEntryChanges, TimeEntryRecord,
};
use crate::token_api::TokenApiError;
use orbit_platform::Id;

#[derive(Deserialize, ToSchema)]
#[schema(as = PublicLogTimeBody)]
#[serde(deny_unknown_fields)]
pub(super) struct LogTimeBody {
    /// Whole minutes, 1 to 1440.
    minutes: u32,
    /// The day of the work, `YYYY-MM-DD`. Default: today in the member's profile timezone (UTC
    /// when it has none).
    worked_on: Option<String>,
    /// Markdown, up to 10,000 characters.
    #[serde(default)]
    note: String,
    #[serde(default)]
    billable: bool,
    /// The member who did the work. Default: the token's user. Logging for someone else needs
    /// `time.manage`; a service-account token must name the member.
    #[schema(value_type = Option<String>)]
    user_id: Option<Id>,
}

#[derive(Deserialize, ToSchema)]
#[schema(as = PublicUpdateTimeEntryBody)]
#[serde(deny_unknown_fields)]
pub(super) struct UpdateTimeEntryBody {
    minutes: Option<u32>,
    worked_on: Option<String>,
    note: Option<String>,
    billable: Option<bool>,
    #[schema(value_type = Option<String>)]
    user_id: Option<Id>,
    /// The entry's `version` as read; the edit fails with `409` when it changed since.
    expected_version: Option<u64>,
}

#[utoipa::path(
    get,
    path = "/api/public/v1/workspaces/{workspace_id}/tasks/{task}/time-entries",
    operation_id = "public_list_time_entries",
    tag = "public-api",
    summary = "The time logged on a task",
    description = "Every entry, by day worked, with the task's total and the total of its live sub-issues in the approved projects.",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("task" = String, Path, description = "A task UUID or identifier such as `ENG-12` (any letter case).")),
    responses((status = 200, description = "The entries and totals", body = TaskTimeEntries)),
    security(("bearerAuth" = []))
)]
pub(super) async fn time_entries(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, task)): Path<(String, String)>,
) -> Response {
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::TasksRead, &ctx.request_id)
            .await?;
        api.time_entries(&task).await
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    post,
    path = "/api/public/v1/workspaces/{workspace_id}/tasks/{task}/time-entries",
    operation_id = "public_log_time",
    tag = "public-api",
    summary = "Log time on a task",
    description = "Adds a time entry. Logging time for another member needs `time.manage` (`404` otherwise). The task's project must show the workspace's `time_tracking` field (`400` otherwise); existing entries stay readable and editable without it.",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("task" = String, Path, description = "A task UUID or identifier such as `ENG-12` (any letter case).")),
    request_body = LogTimeBody,
    responses((status = 201, description = "The new entry", body = TimeEntryRecord)),
    security(("bearerAuth" = []))
)]
pub(super) async fn log_time(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, task)): Path<(String, String)>,
    body: Result<Json<LogTimeBody>, JsonRejection>,
) -> Response {
    let body = input!(ctx, ApiTokenScope::TasksWrite, body);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::TasksWrite, &ctx.request_id)
            .await?;
        api.log_time(
            &task,
            NewTimeEntry {
                user_id: body.user_id,
                minutes: body.minutes,
                worked_on: body.worked_on,
                note: body.note,
                billable: body.billable,
            },
        )
        .await
    }
    .await;
    ctx.reply(StatusCode::CREATED, result)
}

#[utoipa::path(
    patch,
    path = "/api/public/v1/workspaces/{workspace_id}/time-entries/{entry_id}",
    operation_id = "public_update_time_entry",
    tag = "public-api",
    summary = "Change a time entry",
    description = "Your own entry, or any entry with `time.manage` (`404` otherwise). Fields left out keep their value.",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("entry_id" = String, Path)),
    request_body = UpdateTimeEntryBody,
    responses((status = 200, description = "The entry with its new version", body = TimeEntryRecord)),
    security(("bearerAuth" = []))
)]
pub(super) async fn update_time_entry(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, entry)): Path<(String, String)>,
    body: Result<Json<UpdateTimeEntryBody>, JsonRejection>,
) -> Response {
    let entry = path_id!(ctx, entry);
    let body = input!(ctx, ApiTokenScope::TasksWrite, body);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::TasksWrite, &ctx.request_id)
            .await?;
        api.update_time_entry(
            entry,
            TimeEntryChanges {
                user_id: body.user_id,
                minutes: body.minutes,
                worked_on: body.worked_on,
                note: body.note,
                billable: body.billable,
            },
            body.expected_version,
        )
        .await
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    delete,
    path = "/api/public/v1/workspaces/{workspace_id}/time-entries/{entry_id}",
    operation_id = "public_delete_time_entry",
    tag = "public-api",
    summary = "Delete a time entry",
    description = "Your own entry, or any entry with `time.manage` (`404` otherwise). This cannot be undone.",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("entry_id" = String, Path)),
    responses((status = 204, description = "The entry is deleted")),
    security(("bearerAuth" = []))
)]
pub(super) async fn delete_time_entry(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, entry)): Path<(String, String)>,
) -> Response {
    let entry = path_id!(ctx, entry);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::TasksWrite, &ctx.request_id)
            .await?;
        api.delete_time_entry(entry).await
    }
    .await;
    match result {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => ctx.error(error),
    }
}
