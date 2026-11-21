//! Task comment edits and doc comments. Edits and deletes act as the token's user, so a
//! service-account token gets 404.
use axum::Json;
use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use utoipa::ToSchema;

use super::{CommentOut, Ctx, ExpectedVersionQuery, List, PublicApiState, list, problem_response};
use crate::repositories::api_tokens::ApiTokenScope;
use crate::token_api::{DocComment, DocThread, TokenApiError};
use orbit_platform::Id;

#[derive(Deserialize, ToSchema)]
#[schema(as = PublicUpdateCommentBody)]
#[serde(deny_unknown_fields)]
pub(super) struct UpdateCommentBody {
    /// The comment's `version` as read.
    expected_version: u64,
    /// The new comment as Markdown, 1 to 100,000 bytes; `@Name` mentions a member.
    body: String,
}

/// Send exactly one of `thread_id` and `quote`.
#[derive(Deserialize, ToSchema)]
#[schema(as = PublicCreateDocCommentBody)]
#[serde(deny_unknown_fields)]
pub(super) struct CreateDocCommentBody {
    /// The comment as Markdown, 1 to 100,000 bytes: paragraphs, bold, italic, code, links and
    /// `@Name` mentions. Headings and list items become plain paragraphs.
    body: String,
    /// Reply in this thread, from `GET /docs/{page_id}/comments`.
    #[schema(value_type = Option<String>)]
    thread_id: Option<Id>,
    /// Start a new thread on this text of the doc, 1 to 1,000 characters.
    quote: Option<String>,
}

#[utoipa::path(
    patch,
    path = "/api/public/v1/workspaces/{workspace_id}/tasks/{task}/comments/{comment_id}",
    operation_id = "public_update_comment",
    tag = "public-api",
    summary = "Replace the body of a task comment",
    description = "Only the comment's author can edit it (`400` otherwise). Not for service-account tokens (`404`).",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("task" = String, Path, description = "A task UUID or identifier such as `ENG-12` (any letter case)."), ("comment_id" = String, Path)),
    request_body = UpdateCommentBody,
    responses((status = 200, description = "The comment with its new version", body = CommentOut)),
    security(("bearerAuth" = []))
)]
pub(super) async fn update_comment(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, task, comment)): Path<(String, String, String)>,
    body: Result<Json<UpdateCommentBody>, JsonRejection>,
) -> Response {
    let comment = path_id!(ctx, comment);
    let body = input!(ctx, ApiTokenScope::TasksWrite, body);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::TasksWrite, &ctx.request_id)
            .await?;
        api.update_comment(&task, comment, body.expected_version, body.body)
            .await
            .map(CommentOut::from)
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    delete,
    path = "/api/public/v1/workspaces/{workspace_id}/tasks/{task}/comments/{comment_id}",
    operation_id = "public_delete_comment",
    tag = "public-api",
    summary = "Delete a task comment",
    description = "Your own comment, or any comment if you may moderate comments in the project (`400` otherwise). This cannot be undone. Not for service-account tokens (`404`).",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("task" = String, Path, description = "A task UUID or identifier such as `ENG-12` (any letter case)."), ("comment_id" = String, Path), ExpectedVersionQuery),
    responses((status = 204, description = "The comment is deleted")),
    security(("bearerAuth" = []))
)]
pub(super) async fn delete_comment(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, task, comment)): Path<(String, String, String)>,
    query: Result<Query<ExpectedVersionQuery>, QueryRejection>,
) -> Response {
    let comment = path_id!(ctx, comment);
    let query = input!(ctx, ApiTokenScope::TasksWrite, query);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::TasksWrite, &ctx.request_id)
            .await?;
        api.delete_comment(&task, comment, query.expected_version)
            .await
    }
    .await;
    match result {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => ctx.error(error),
    }
}

#[utoipa::path(
    get,
    path = "/api/public/v1/workspaces/{workspace_id}/docs/{page_id}/comments",
    operation_id = "public_list_doc_comments",
    tag = "public-api",
    summary = "The comment threads of a doc, oldest first",
    description = "Each thread has its quoted text, resolved state and comments as Markdown, with author names.",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("page_id" = String, Path)),
    responses((status = 200, description = "Every thread; `next_cursor` is `null`", body = List<DocThread>)),
    security(("bearerAuth" = []))
)]
pub(super) async fn doc_comments(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, page)): Path<(String, String)>,
) -> Response {
    let page = path_id!(ctx, page);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::DocsRead, &ctx.request_id)
            .await?;
        Ok(list::<_, DocThread>(api.doc_comments(page).await?, None))
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    post,
    path = "/api/public/v1/workspaces/{workspace_id}/docs/{page_id}/comments",
    operation_id = "public_create_doc_comment",
    tag = "public-api",
    summary = "Comment on a doc",
    description = "Replies in a thread (`thread_id`), or starts a thread on a quoted text of the doc (`quote`). A new thread is not anchored in the doc's text: the doc lists it with its quote.",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("page_id" = String, Path)),
    request_body = CreateDocCommentBody,
    responses((status = 201, description = "The new comment with its `thread_id`", body = DocComment)),
    security(("bearerAuth" = []))
)]
pub(super) async fn create_doc_comment(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, page)): Path<(String, String)>,
    body: Result<Json<CreateDocCommentBody>, JsonRejection>,
) -> Response {
    let page = path_id!(ctx, page);
    let body = input!(ctx, ApiTokenScope::DocsWrite, body);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::DocsWrite, &ctx.request_id)
            .await?;
        api.create_doc_comment(page, body.thread_id, body.quote, &body.body)
            .await
    }
    .await;
    ctx.reply(StatusCode::CREATED, result)
}
