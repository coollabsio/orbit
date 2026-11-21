//! The task and doc trash: list, read and restore. Items stay in the trash for 30 days.
use axum::Json;
use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Response;
use serde::Deserialize;
use utoipa::ToSchema;

use super::{Ctx, List, PageQuery, PublicApiState, list, problem_response};
use crate::repositories::api_tokens::ApiTokenScope;
use crate::repositories::pages::TrashedPage;
use crate::token_api::{DocView, TaskView, TokenApiError, TrashedTaskView};
use orbit_platform::Id;

#[derive(Deserialize, ToSchema)]
#[schema(as = PublicRestoreBody)]
#[serde(deny_unknown_fields)]
pub(super) struct RestoreBody {
    /// The `version` that you read in the trash.
    expected_version: u64,
}

#[utoipa::path(
    get,
    path = "/api/public/v1/workspaces/{workspace_id}/tasks/trash",
    operation_id = "public_list_task_trash",
    tag = "public-api",
    summary = "The tasks in the trash of the approved projects, last trashed first",
    description = "Each task has `deleted_at` and `trashed_descendant_count`, the sub-issues that a restore brings back too. A page can hold fewer than `limit` tasks before the last one: follow `next_cursor`.",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), PageQuery),
    responses((status = 200, description = "A page of trashed tasks", body = List<TrashedTaskView>)),
    security(("bearerAuth" = []))
)]
pub(super) async fn task_trash(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    query: Result<Query<PageQuery>, QueryRejection>,
) -> Response {
    let query = input!(ctx, ApiTokenScope::TasksRead, query);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::TasksRead, &ctx.request_id)
            .await?;
        let page = api.task_trash(query.cursor.as_deref(), query.limit).await?;
        Ok(list::<_, TrashedTaskView>(page.items, page.next_cursor))
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    get,
    path = "/api/public/v1/workspaces/{workspace_id}/tasks/trash/{task}",
    operation_id = "public_get_trashed_task",
    tag = "public-api",
    summary = "A task in the trash",
    description = "With its `status`, `labels` and the user who trashed it (`deleted_by`).",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("task" = String, Path, description = "A trashed task's UUID or identifier such as `ENG-12` (any letter case).")),
    responses((status = 200, description = "The trashed task", body = TrashedTaskView)),
    security(("bearerAuth" = []))
)]
pub(super) async fn trashed_task(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, task)): Path<(String, String)>,
) -> Response {
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::TasksRead, &ctx.request_id)
            .await?;
        api.get_trashed_task(&task).await
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    post,
    path = "/api/public/v1/workspaces/{workspace_id}/tasks/{task}/restore",
    operation_id = "public_restore_task",
    tag = "public-api",
    summary = "Restore a task from the trash",
    description = "Brings back the sub-issues trashed with it. A task whose parent is still in the trash comes back top-level.",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("task" = String, Path, description = "A trashed task's UUID or identifier such as `ENG-12` (any letter case).")),
    request_body = RestoreBody,
    responses((status = 200, description = "The task, as `GET /tasks/{task}` shows it", body = TaskView)),
    security(("bearerAuth" = []))
)]
pub(super) async fn restore_task(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, task)): Path<(String, String)>,
    body: Result<Json<RestoreBody>, JsonRejection>,
) -> Response {
    let body = input!(ctx, ApiTokenScope::TasksWrite, body);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::TasksWrite, &ctx.request_id)
            .await?;
        api.restore_task(&task, body.expected_version).await
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    get,
    path = "/api/public/v1/workspaces/{workspace_id}/docs/trash",
    operation_id = "public_list_doc_trash",
    tag = "public-api",
    summary = "The docs in the trash that the token's creator can see, last trashed first",
    description = "Docs trashed with a parent doc are not listed: restoring the parent brings them back.",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`.")),
    responses((status = 200, description = "Every trashed doc; `next_cursor` is `null`", body = List<TrashedPage>)),
    security(("bearerAuth" = []))
)]
pub(super) async fn doc_trash(State(s): State<PublicApiState>, ctx: Ctx) -> Response {
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::DocsRead, &ctx.request_id)
            .await?;
        Ok(list::<_, TrashedPage>(api.doc_trash().await?.items, None))
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    post,
    path = "/api/public/v1/workspaces/{workspace_id}/docs/{page_id}/restore",
    operation_id = "public_restore_doc",
    tag = "public-api",
    summary = "Restore a doc from the trash",
    description = "Brings back the sub-docs trashed with it. A row of a database that is in the trash cannot be restored before the database.",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("page_id" = String, Path)),
    request_body = RestoreBody,
    responses((status = 200, description = "The doc, as `GET /docs/{page_id}` shows it", body = DocView)),
    security(("bearerAuth" = []))
)]
pub(super) async fn restore_doc(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, page)): Path<(String, String)>,
    body: Result<Json<RestoreBody>, JsonRejection>,
) -> Response {
    let page = path_id!(ctx, page);
    let body = input!(ctx, ApiTokenScope::DocsWrite, body);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::DocsWrite, &ctx.request_id)
            .await?;
        api.restore_doc(page, body.expected_version).await
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}
