//! Teamspaces and task activity.
use axum::extract::rejection::QueryRejection;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Response;

use super::{Ctx, List, PageQuery, PublicApiState, list};
use crate::repositories::api_tokens::ApiTokenScope;
use crate::token_api::{TaskActivity, TeamspaceView, TokenApiError};

#[utoipa::path(
    get,
    path = "/api/public/v1/workspaces/{workspace_id}/teamspaces",
    operation_id = "public_list_teamspaces",
    tag = "public-api",
    summary = "The teamspaces that the token's creator sees, the default one first",
    description = "Send an `id` as `teamspace_id` to create or move a doc. `project_id` is the linked project; `null` for global docs or a project that is not approved.",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`.")),
    responses((status = 200, description = "Every teamspace, in order; `next_cursor` is `null`", body = List<TeamspaceView>)),
    security(("bearerAuth" = []))
)]
pub(super) async fn teamspaces(State(s): State<PublicApiState>, ctx: Ctx) -> Response {
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::DocsRead, &ctx.request_id)
            .await?;
        Ok(list::<_, TeamspaceView>(api.list_teamspaces().await?, None))
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    get,
    path = "/api/public/v1/workspaces/{workspace_id}/tasks/{task}/activity",
    operation_id = "public_list_task_activity",
    tag = "public-api",
    summary = "The history of a task, newest first",
    description = "Who did what and when. `metadata` depends on `action`; a task or project that is not approved is `null`, with `<field>_hidden: true`.",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("task" = String, Path, description = "A task UUID or identifier such as `ENG-12` (any letter case)."), PageQuery),
    responses((status = 200, description = "A page of events", body = List<TaskActivity>)),
    security(("bearerAuth" = []))
)]
pub(super) async fn task_activity(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, task)): Path<(String, String)>,
    query: Result<Query<PageQuery>, QueryRejection>,
) -> Response {
    let query = input!(ctx, ApiTokenScope::TasksRead, query);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::TasksRead, &ctx.request_id)
            .await?;
        let page = api
            .task_activity(&task, query.cursor.as_deref(), query.limit)
            .await?;
        Ok(list::<_, TaskActivity>(page.items, page.next_cursor))
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}
