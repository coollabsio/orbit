//! Docs comments: threads and comments of a page under
//! `/api/v1/workspaces/{workspace_id}/pages/{page_id}/threads`. Access follows the page's
//! visibility (see `repositories::page_comments`); a hidden page answers exactly like an unknown
//! id (`404 page_not_found`).

use std::sync::Arc;

use axum::extract::{DefaultBodyLimit, Extension, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use orbit_platform::{Id, RequestId, TimestampMillis};
use serde::Deserialize;
use serde_json::Value;
use utoipa::ToSchema;

use crate::auth_routes::CookieMode;
use crate::repositories::identity::IdentityRepository;
use crate::repositories::page_comments::{
    CommentBody, PageComment, PageCommentError, PageCommentRepository, PageThread, PageThreadList,
};
use crate::task_routes::{ApiError, ApiJson, authenticate_session, request_id_value, validation};

/// A comment body is small; this also bounds the quote.
const COMMENT_BODY_LIMIT: usize = 128 * 1024;

#[derive(Clone)]
pub struct PageCommentState {
    identity: Arc<IdentityRepository>,
    comments: Arc<PageCommentRepository>,
    cookie_mode: CookieMode,
}

impl PageCommentState {
    #[must_use]
    pub fn new(identity: Arc<IdentityRepository>, cookie_mode: CookieMode) -> Self {
        Self {
            comments: Arc::new(PageCommentRepository::new(identity.database().clone())),
            identity,
            cookie_mode,
        }
    }
}

pub fn page_comment_router(state: PageCommentState) -> Router {
    Router::new()
        .route(
            "/api/v1/workspaces/{workspace_id}/pages/{page_id}/threads",
            get(list_page_threads).post(create_page_thread),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/pages/{page_id}/threads/{thread_id}",
            axum::routing::delete(delete_page_thread),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/pages/{page_id}/threads/{thread_id}/comments",
            post(create_page_comment),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/pages/{page_id}/threads/{thread_id}/comments/{comment_id}",
            patch(update_page_comment).delete(delete_page_comment),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/pages/{page_id}/threads/{thread_id}/resolve",
            post(resolve_page_thread),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/pages/{page_id}/threads/{thread_id}/reopen",
            post(reopen_page_thread),
        )
        .layer(DefaultBodyLimit::max(COMMENT_BODY_LIMIT))
        .with_state(state)
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct CreatePageThreadBody {
    /// The first comment: BlockNote blocks of the comment editor (paragraphs with `text`, `link`
    /// and `mention` inline content, mention props `{ userId, name }`).
    #[schema(value_type = Vec<Object>)]
    body: Value,
    /// The selected text the thread is about (trimmed, at most 1000 characters are kept).
    #[serde(default)]
    quote: String,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct PageCommentBody {
    /// BlockNote blocks of the comment editor (see `CreatePageThreadBody.body`).
    #[schema(value_type = Vec<Object>)]
    body: Value,
}

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/pages/{page_id}/threads", params(("workspace_id" = String, Path), ("page_id" = String, Path)), responses((status = 200, body = PageThreadList)))]
async fn list_page_threads(
    State(state): State<PageCommentState>,
    Path((workspace, page)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<PageThreadList>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/pages/{page}/threads");
    let request = request_id.as_ref();
    let (workspace_id, page_id, actor_id) =
        scope(&state, &headers, &workspace, &page, &instance, request).await?;
    state
        .comments
        .threads(workspace_id, page_id, actor_id)
        .await
        .map(Json)
        .map_err(|error| problem(error, &instance, request))
}

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/pages/{page_id}/threads", params(("workspace_id" = String, Path), ("page_id" = String, Path)), request_body = CreatePageThreadBody, responses((status = 201, body = PageThread)))]
async fn create_page_thread(
    State(state): State<PageCommentState>,
    Path((workspace, page)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<CreatePageThreadBody>,
) -> Result<(StatusCode, Json<PageThread>), ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/pages/{page}/threads");
    let request = request_id.as_ref();
    let (workspace_id, page_id, actor_id) =
        scope(&state, &headers, &workspace, &page, &instance, request).await?;
    let comment =
        CommentBody::parse(body.body).map_err(|error| problem(error, &instance, request))?;
    state
        .comments
        .create_thread(
            workspace_id,
            page_id,
            actor_id,
            comment,
            &body.quote,
            request_id_value(request),
            TimestampMillis::now(),
        )
        .await
        .map(|thread| (StatusCode::CREATED, Json(thread)))
        .map_err(|error| problem(error, &instance, request))
}

#[utoipa::path(delete, path = "/api/v1/workspaces/{workspace_id}/pages/{page_id}/threads/{thread_id}", params(("workspace_id" = String, Path), ("page_id" = String, Path), ("thread_id" = String, Path)), responses((status = 204)))]
async fn delete_page_thread(
    State(state): State<PageCommentState>,
    Path((workspace, page, thread)): Path<(String, String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<StatusCode, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/pages/{page}/threads/{thread}");
    let request = request_id.as_ref();
    let (workspace_id, page_id, actor_id) =
        scope(&state, &headers, &workspace, &page, &instance, request).await?;
    let thread_id = parse_child(&thread, &instance, request)?;
    state
        .comments
        .delete_thread(
            workspace_id,
            page_id,
            thread_id,
            actor_id,
            request_id_value(request),
            TimestampMillis::now(),
        )
        .await
        .map(|()| StatusCode::NO_CONTENT)
        .map_err(|error| problem(error, &instance, request))
}

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/pages/{page_id}/threads/{thread_id}/comments", params(("workspace_id" = String, Path), ("page_id" = String, Path), ("thread_id" = String, Path)), request_body = PageCommentBody, responses((status = 201, body = PageComment)))]
async fn create_page_comment(
    State(state): State<PageCommentState>,
    Path((workspace, page, thread)): Path<(String, String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<PageCommentBody>,
) -> Result<(StatusCode, Json<PageComment>), ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/pages/{page}/threads/{thread}/comments");
    let request = request_id.as_ref();
    let (workspace_id, page_id, actor_id) =
        scope(&state, &headers, &workspace, &page, &instance, request).await?;
    let thread_id = parse_child(&thread, &instance, request)?;
    let comment =
        CommentBody::parse(body.body).map_err(|error| problem(error, &instance, request))?;
    state
        .comments
        .add_comment(
            workspace_id,
            page_id,
            thread_id,
            actor_id,
            comment,
            request_id_value(request),
            TimestampMillis::now(),
        )
        .await
        .map(|comment| (StatusCode::CREATED, Json(comment)))
        .map_err(|error| problem(error, &instance, request))
}

#[utoipa::path(patch, path = "/api/v1/workspaces/{workspace_id}/pages/{page_id}/threads/{thread_id}/comments/{comment_id}", params(("workspace_id" = String, Path), ("page_id" = String, Path), ("thread_id" = String, Path), ("comment_id" = String, Path)), request_body = PageCommentBody, responses((status = 200, body = PageComment)))]
async fn update_page_comment(
    State(state): State<PageCommentState>,
    Path((workspace, page, thread, comment)): Path<(String, String, String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<PageCommentBody>,
) -> Result<Json<PageComment>, ApiError> {
    let instance =
        format!("/api/v1/workspaces/{workspace}/pages/{page}/threads/{thread}/comments/{comment}");
    let request = request_id.as_ref();
    let (workspace_id, page_id, actor_id) =
        scope(&state, &headers, &workspace, &page, &instance, request).await?;
    let thread_id = parse_child(&thread, &instance, request)?;
    let comment_id = parse_child(&comment, &instance, request)?;
    let body = CommentBody::parse(body.body).map_err(|error| problem(error, &instance, request))?;
    state
        .comments
        .update_comment(
            workspace_id,
            page_id,
            thread_id,
            comment_id,
            actor_id,
            body,
            request_id_value(request),
            TimestampMillis::now(),
        )
        .await
        .map(Json)
        .map_err(|error| problem(error, &instance, request))
}

#[utoipa::path(delete, path = "/api/v1/workspaces/{workspace_id}/pages/{page_id}/threads/{thread_id}/comments/{comment_id}", params(("workspace_id" = String, Path), ("page_id" = String, Path), ("thread_id" = String, Path), ("comment_id" = String, Path)), responses((status = 204)))]
async fn delete_page_comment(
    State(state): State<PageCommentState>,
    Path((workspace, page, thread, comment)): Path<(String, String, String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<StatusCode, ApiError> {
    let instance =
        format!("/api/v1/workspaces/{workspace}/pages/{page}/threads/{thread}/comments/{comment}");
    let request = request_id.as_ref();
    let (workspace_id, page_id, actor_id) =
        scope(&state, &headers, &workspace, &page, &instance, request).await?;
    let thread_id = parse_child(&thread, &instance, request)?;
    let comment_id = parse_child(&comment, &instance, request)?;
    state
        .comments
        .delete_comment(
            workspace_id,
            page_id,
            thread_id,
            comment_id,
            actor_id,
            request_id_value(request),
            TimestampMillis::now(),
        )
        .await
        .map(|()| StatusCode::NO_CONTENT)
        .map_err(|error| problem(error, &instance, request))
}

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/pages/{page_id}/threads/{thread_id}/resolve", params(("workspace_id" = String, Path), ("page_id" = String, Path), ("thread_id" = String, Path)), responses((status = 200, body = PageThread)))]
async fn resolve_page_thread(
    State(state): State<PageCommentState>,
    Path((workspace, page, thread)): Path<(String, String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<PageThread>, ApiError> {
    set_resolved(
        state, workspace, page, thread, headers, request_id, true, "resolve",
    )
    .await
}

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/pages/{page_id}/threads/{thread_id}/reopen", params(("workspace_id" = String, Path), ("page_id" = String, Path), ("thread_id" = String, Path)), responses((status = 200, body = PageThread)))]
async fn reopen_page_thread(
    State(state): State<PageCommentState>,
    Path((workspace, page, thread)): Path<(String, String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<PageThread>, ApiError> {
    set_resolved(
        state, workspace, page, thread, headers, request_id, false, "reopen",
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn set_resolved(
    state: PageCommentState,
    workspace: String,
    page: String,
    thread: String,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    resolved: bool,
    action: &str,
) -> Result<Json<PageThread>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/pages/{page}/threads/{thread}/{action}");
    let request = request_id.as_ref();
    let (workspace_id, page_id, actor_id) =
        scope(&state, &headers, &workspace, &page, &instance, request).await?;
    let thread_id = parse_child(&thread, &instance, request)?;
    state
        .comments
        .set_resolved(
            workspace_id,
            page_id,
            thread_id,
            actor_id,
            resolved,
            request_id_value(request),
            TimestampMillis::now(),
        )
        .await
        .map(Json)
        .map_err(|error| problem(error, &instance, request))
}

/// Authenticates and parses the workspace and page ids (unparseable ids name no page).
async fn scope(
    state: &PageCommentState,
    headers: &HeaderMap,
    workspace: &str,
    page: &str,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<(Id, Id, Id), ApiError> {
    let session = authenticate_session(
        &state.identity,
        state.cookie_mode,
        headers,
        instance,
        request_id,
    )
    .await?;
    let workspace_id = workspace
        .parse()
        .map_err(|_| page_not_found(instance, request_id))?;
    let page_id = page
        .parse()
        .map_err(|_| page_not_found(instance, request_id))?;
    Ok((workspace_id, page_id, session.user.id))
}

/// An unparseable thread or comment id. The page is checked by the repository first for valid
/// ids, so answering `page_not_found` here keeps hidden pages indistinguishable.
fn parse_child(
    value: &str,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<Id, ApiError> {
    value
        .parse()
        .map_err(|_| page_not_found(instance, request_id))
}

fn page_not_found(instance: &str, request_id: Option<&Extension<RequestId>>) -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        "page_not_found",
        "Page not found",
        "The requested page was not found.",
        instance,
        request_id,
    )
}

fn problem(
    error: PageCommentError,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> ApiError {
    match error {
        PageCommentError::PageNotFound => page_not_found(instance, request_id),
        PageCommentError::NotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            "page_comment_not_found",
            "Comment not found",
            "The requested thread or comment was not found.",
            instance,
            request_id,
        ),
        PageCommentError::Forbidden => ApiError::new(
            StatusCode::FORBIDDEN,
            "page_comment_forbidden",
            "Comment action forbidden",
            "Only the author may edit or delete this comment or thread.",
            instance,
            request_id,
        ),
        PageCommentError::Invalid { field } => validation(field, instance, request_id),
        PageCommentError::Corrupt | PageCommentError::Unavailable(_) => ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            "Internal server error",
            "An unexpected error occurred. Use the request ID when contacting support.",
            instance,
            request_id,
        ),
    }
}
