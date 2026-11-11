//! Task relations: blocks, related and duplicate. A duplicate is made with `duplicate_of_id`
//! on a task update, as in the web app.
use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use utoipa::ToSchema;

use super::{Ctx, List, PublicApiState, list, problem_response};
use crate::repositories::api_tokens::ApiTokenScope;
use crate::repositories::task_relations::{NewTaskRelationType, TaskRelationRecord};
use crate::token_api::TokenApiError;
use orbit_platform::Id;

#[derive(Deserialize, ToSchema)]
#[schema(as = PublicCreateRelationBody)]
#[serde(deny_unknown_fields)]
pub(super) struct CreateRelationBody {
    /// `blocks` (this task blocks `task_id`), `blocked_by` or `related`.
    #[serde(rename = "type")]
    kind: NewTaskRelationType,
    /// The other task, in an approved project: a UUID or an identifier such as `ENG-12`.
    task_id: String,
}

#[utoipa::path(
    get,
    path = "/api/public/v1/workspaces/{workspace_id}/tasks/{task}/relations",
    operation_id = "public_list_relations",
    tag = "public-api",
    summary = "The relations of a task",
    description = "Relations to tasks outside the approved projects are left out.",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("task" = String, Path, description = "A task UUID or identifier such as `ENG-12` (any letter case).")),
    responses((status = 200, description = "Every relation; `next_cursor` is `null`", body = List<TaskRelationRecord>)),
    security(("bearerAuth" = []))
)]
pub(super) async fn relations(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, task)): Path<(String, String)>,
) -> Response {
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::TasksRead, &ctx.request_id)
            .await?;
        Ok(list::<_, TaskRelationRecord>(
            api.relations(&task).await?,
            None,
        ))
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    post,
    path = "/api/public/v1/workspaces/{workspace_id}/tasks/{task}/relations",
    operation_id = "public_create_relation",
    tag = "public-api",
    summary = "Link two tasks",
    description = "A relation that already links the two tasks is replaced; the same relation again is returned unchanged. A duplicate relation cannot be replaced here (409): remove it or update `duplicate_of_id` first.",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("task" = String, Path, description = "A task UUID or identifier such as `ENG-12` (any letter case).")),
    request_body = CreateRelationBody,
    responses((status = 201, description = "The relation, seen from this task", body = TaskRelationRecord)),
    security(("bearerAuth" = []))
)]
pub(super) async fn create_relation(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, task)): Path<(String, String)>,
    body: Result<Json<CreateRelationBody>, JsonRejection>,
) -> Response {
    let body = input!(ctx, ApiTokenScope::TasksWrite, body);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::TasksWrite, &ctx.request_id)
            .await?;
        api.add_relation(&task, body.kind, &body.task_id).await
    }
    .await;
    ctx.reply(StatusCode::CREATED, result)
}

#[utoipa::path(
    delete,
    path = "/api/public/v1/workspaces/{workspace_id}/tasks/{task}/relations/{relation_id}",
    operation_id = "public_delete_relation",
    tag = "public-api",
    summary = "Remove a relation",
    description = "Removing a duplicate relation also moves the duplicate out of the Duplicate status.",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("task" = String, Path, description = "A task UUID or identifier such as `ENG-12` (any letter case)."), ("relation_id" = String, Path)),
    responses((status = 204, description = "The relation is removed")),
    security(("bearerAuth" = []))
)]
pub(super) async fn delete_relation(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, task, relation)): Path<(String, String, String)>,
) -> Response {
    let relation = path_id!(ctx, relation);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::TasksWrite, &ctx.request_id)
            .await?;
        api.remove_relation(&task, relation).await
    }
    .await;
    match result {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => ctx.error(error),
    }
}
