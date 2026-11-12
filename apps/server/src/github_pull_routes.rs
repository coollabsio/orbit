//! Pull requests in Orbit, read with the member's own GitHub token. Orbit's own rule comes first (the member sees a
//! project connected to the repository); then GitHub's (the member's token can read it).

use axum::Json;
use axum::extract::{Extension, Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use orbit_platform::{Id, RequestId, Row};
use serde::Deserialize;
use utoipa::ToSchema;

use crate::github::pulls::{
    self, FileContents, PullDetail, PullFile, PullFilter, PullList, PullRef, PullTaskRef, PullThreads,
    PullTimelineItem, valid_file_path, valid_name,
};
use crate::github::user_tokens::{UserToken, mark_broken, user_token};
use crate::github::{GithubClient, GithubError};
use crate::integration_routes::{ApiError, ApiJson, IntegrationState, github_project_access, github_workspace_access};
use crate::repositories::project_access::user_sees_project_sql;

/// Who reads which pull request, with which token.
pub(crate) struct PullAccess {
    pub(crate) workspace: Id,
    pub(crate) user: Id,
    pub(crate) token: UserToken,
    pub(crate) pull: PullRef,
}

fn request_id(id: Option<&Extension<RequestId>>) -> Option<&RequestId> {
    id.map(|Extension(id)| id)
}

fn not_found(request_id: Option<&RequestId>) -> ApiError {
    GithubError::NotFound.into_api(request_id)
}

/// The member, the pull request and their token; 404 when Orbit or the names say no.
pub(crate) async fn pull_access(
    state: &IntegrationState,
    headers: &HeaderMap,
    workspace: &str,
    owner: &str,
    repo: &str,
    number: i64,
    request_id: Option<&RequestId>,
) -> Result<PullAccess, ApiError> {
    if !valid_name(owner) || !valid_name(repo) || number <= 0 {
        return Err(not_found(request_id));
    }
    let (workspace_id, _) = github_workspace_access(state, headers, workspace, request_id).await?;
    let user = state.session_user(headers, request_id).await?;
    let pull = PullRef {
        owner: owner.to_owned(),
        repo: repo.to_owned(),
        number,
    };
    let orbit_flag: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT COUNT(*) FROM github_project_connections c JOIN projects p ON p.id = c.project_id \
         WHERE c.workspace_id = $1 AND lower(c.repository) = $2 AND p.deleted_at IS NULL AND {}",
        user_sees_project_sql("p.id", "$3")
    )))
    .bind(workspace)
    .bind(pull.repository())
    .bind(user.to_string())
    .fetch_one(state.database().pool())
    .await
    .map_err(|_| ApiError::internal(request_id))?;
    if orbit_flag == 0 {
        return Err(not_found(request_id));
    }
    let token = user_token(state, &workspace_id, &user)
        .await
        .map_err(|error| error.into_api(request_id))?;
    Ok(PullAccess {
        workspace: workspace_id,
        user,
        token,
        pull,
    })
}

pub(crate) fn github(state: &IntegrationState, request_id: Option<&RequestId>) -> Result<GithubClient, ApiError> {
    state
        .github_client()
        .cloned()
        .ok_or_else(|| ApiError::internal(request_id))
}

/// A GitHub error as the API answers it; a 401 also marks the member's connection broken.
pub(crate) async fn github_failed(
    state: &IntegrationState,
    access: &PullAccess,
    error: GithubError,
    request_id: Option<&RequestId>,
) -> ApiError {
    if matches!(error, GithubError::Unauthorized) {
        let _ = mark_broken(state, &access.workspace, &access.user, &access.token.token).await;
    }
    error.into_api(request_id)
}

type PullPath = Path<(String, String, String, i64)>;

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/github/pulls/{owner}/{repo}/{number}", params(("workspace_id" = String, Path), ("owner" = String, Path), ("repo" = String, Path), ("number" = i64, Path)), responses((status = 200, body = PullDetail)))]
pub(crate) async fn get_github_pull(
    State(state): State<IntegrationState>,
    Path((workspace, owner, repo, number)): PullPath,
    headers: HeaderMap,
    request: Option<Extension<RequestId>>,
) -> Result<Json<PullDetail>, ApiError> {
    let id = request_id(request.as_ref());
    let access = pull_access(&state, &headers, &workspace, &owner, &repo, number, id).await?;
    let mut detail = match pulls::detail(&github(&state, id)?, &access.token.token, &access.pull).await {
        Ok(detail) => detail,
        Err(error) => return Err(github_failed(&state, &access, error, id).await),
    };
    detail.tasks = linked_tasks(&state, &access, id).await?;
    Ok(Json(detail))
}

/// Orbit tasks linked to the pull request (by the webhook or by issue sync) that the member sees.
async fn linked_tasks(
    state: &IntegrationState,
    access: &PullAccess,
    request_id: Option<&RequestId>,
) -> Result<Vec<PullTaskRef>, ApiError> {
    let sees = user_sees_project_sql("t.project_id", "$5");
    let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT t.id, t.title FROM tasks t WHERE t.deleted_at IS NULL AND t.workspace_id = $1 AND {sees} AND t.id IN (\
         SELECT task_id FROM github_issue_links WHERE workspace_id = $1 AND lower(repository) = $2 AND issue_number = $3 AND kind = 'pull_request' \
         UNION SELECT task_id FROM github_pull_links WHERE workspace_id = $1 AND lower(repository) = $2 AND pull_number = $4) \
         ORDER BY t.title"
    )))
    .bind(access.workspace.to_string())
    .bind(access.pull.repository())
    .bind(access.pull.number)
    .bind(access.pull.number)
    .bind(access.user.to_string())
    .fetch_all(state.database().pool())
    .await
    .map_err(|_| ApiError::internal(request_id))?;
    Ok(rows
        .into_iter()
        .map(|row| PullTaskRef {
            task_id: row.get("id"),
            title: row.get("title"),
        })
        .collect())
}

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/github/pulls/{owner}/{repo}/{number}/files", params(("workspace_id" = String, Path), ("owner" = String, Path), ("repo" = String, Path), ("number" = i64, Path)), responses((status = 200, body = Vec<PullFile>)))]
pub(crate) async fn list_github_pull_files(
    State(state): State<IntegrationState>,
    Path((workspace, owner, repo, number)): PullPath,
    headers: HeaderMap,
    request: Option<Extension<RequestId>>,
) -> Result<Json<Vec<PullFile>>, ApiError> {
    let id = request_id(request.as_ref());
    let access = pull_access(&state, &headers, &workspace, &owner, &repo, number, id).await?;
    match pulls::files(&github(&state, id)?, &access.token.token, &access.pull).await {
        Ok(files) => Ok(Json(files)),
        Err(error) => Err(github_failed(&state, &access, error, id).await),
    }
}

#[derive(Deserialize)]
pub(crate) struct ContentsQuery {
    path: String,
    #[serde(rename = "ref")]
    sha: String,
}

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/github/pulls/{owner}/{repo}/{number}/contents", params(("workspace_id" = String, Path), ("owner" = String, Path), ("repo" = String, Path), ("number" = i64, Path), ("path" = String, Query), ("ref" = String, Query)), responses((status = 200, body = FileContents)))]
pub(crate) async fn get_github_pull_contents(
    State(state): State<IntegrationState>,
    Path((workspace, owner, repo, number)): PullPath,
    Query(query): Query<ContentsQuery>,
    headers: HeaderMap,
    request: Option<Extension<RequestId>>,
) -> Result<Json<FileContents>, ApiError> {
    let id = request_id(request.as_ref());
    if !valid_file_path(&query.path)
        || query.sha.is_empty()
        || query.sha.len() > 64
        || !query.sha.bytes().all(|c| c.is_ascii_alphanumeric())
    {
        return Err(not_found(id));
    }
    let access = pull_access(&state, &headers, &workspace, &owner, &repo, number, id).await?;
    match pulls::contents(&github(&state, id)?, &access.token.token, &access.pull, &query.path, &query.sha).await {
        Ok(contents) => Ok(Json(contents)),
        Err(error) => Err(github_failed(&state, &access, error, id).await),
    }
}

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/github/pulls/{owner}/{repo}/{number}/threads", params(("workspace_id" = String, Path), ("owner" = String, Path), ("repo" = String, Path), ("number" = i64, Path)), responses((status = 200, body = PullThreads)))]
pub(crate) async fn list_github_pull_threads(
    State(state): State<IntegrationState>,
    Path((workspace, owner, repo, number)): PullPath,
    headers: HeaderMap,
    request: Option<Extension<RequestId>>,
) -> Result<Json<PullThreads>, ApiError> {
    let id = request_id(request.as_ref());
    let access = pull_access(&state, &headers, &workspace, &owner, &repo, number, id).await?;
    let client = github(&state, id)?;
    // two independent GitHub queries: asked at the same time
    let (threads, pending) = tokio::join!(
        pulls::threads(&client, &access.token.token, &access.pull),
        reviews::pending_review(&client, &access.token.token, &access.pull),
    );
    match threads.and_then(|threads| pending.map(|pending_review| PullThreads { threads, pending_review })) {
        Ok(result) => Ok(Json(result)),
        Err(error) => Err(github_failed(&state, &access, error, id).await),
    }
}

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/github/pulls/{owner}/{repo}/{number}/conversation", params(("workspace_id" = String, Path), ("owner" = String, Path), ("repo" = String, Path), ("number" = i64, Path)), responses((status = 200, body = Vec<PullTimelineItem>)))]
pub(crate) async fn list_github_pull_conversation(
    State(state): State<IntegrationState>,
    Path((workspace, owner, repo, number)): PullPath,
    headers: HeaderMap,
    request: Option<Extension<RequestId>>,
) -> Result<Json<Vec<PullTimelineItem>>, ApiError> {
    let id = request_id(request.as_ref());
    let access = pull_access(&state, &headers, &workspace, &owner, &repo, number, id).await?;
    match pulls::conversation(&github(&state, id)?, &access.token.token, &access.pull).await {
        Ok(items) => Ok(Json(items)),
        Err(error) => Err(github_failed(&state, &access, error, id).await),
    }
}

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/github/pulls/{owner}/{repo}/{number}/merge-base", params(("workspace_id" = String, Path), ("owner" = String, Path), ("repo" = String, Path), ("number" = i64, Path)), responses((status = 200, body = pulls::MergeBase)))]
pub(crate) async fn get_github_pull_merge_base(
    State(state): State<IntegrationState>,
    Path((workspace, owner, repo, number)): PullPath,
    headers: HeaderMap,
    request: Option<Extension<RequestId>>,
) -> Result<Json<pulls::MergeBase>, ApiError> {
    let id = request_id(request.as_ref());
    let access = pull_access(&state, &headers, &workspace, &owner, &repo, number, id).await?;
    match pulls::merge_base(&github(&state, id)?, &access.token.token, &access.pull).await {
        Ok(base) => Ok(Json(base)),
        Err(error) => Err(github_failed(&state, &access, error, id).await),
    }
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
#[schema(as = GithubViewedFile)]
pub struct ViewedFile {
    path: String,
}

async fn set_viewed(
    state: IntegrationState,
    (workspace, owner, repo, number): (String, String, String, i64),
    headers: HeaderMap,
    request: Option<Extension<RequestId>>,
    path: String,
    viewed: bool,
) -> Result<StatusCode, ApiError> {
    let id = request_id(request.as_ref());
    if path.is_empty() || path.len() > 4096 {
        return Err(not_found(id));
    }
    let access = pull_access(&state, &headers, &workspace, &owner, &repo, number, id).await?;
    match pulls::set_viewed(&github(&state, id)?, &access.token.token, &access.pull, &path, viewed).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(error) => Err(github_failed(&state, &access, error, id).await),
    }
}

#[utoipa::path(put, path = "/api/v1/workspaces/{workspace_id}/github/pulls/{owner}/{repo}/{number}/viewed", params(("workspace_id" = String, Path), ("owner" = String, Path), ("repo" = String, Path), ("number" = i64, Path)), request_body = ViewedFile, responses((status = 204, description = "Marked as viewed on GitHub")))]
pub(crate) async fn mark_github_file_viewed(
    State(state): State<IntegrationState>,
    Path(path): PullPath,
    headers: HeaderMap,
    request: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<ViewedFile>,
) -> Result<StatusCode, ApiError> {
    set_viewed(state, path, headers, request, body.path, true).await
}

#[utoipa::path(delete, path = "/api/v1/workspaces/{workspace_id}/github/pulls/{owner}/{repo}/{number}/viewed", params(("workspace_id" = String, Path), ("owner" = String, Path), ("repo" = String, Path), ("number" = i64, Path)), request_body = ViewedFile, responses((status = 204, description = "Marked as not viewed on GitHub")))]
pub(crate) async fn unmark_github_file_viewed(
    State(state): State<IntegrationState>,
    Path(path): PullPath,
    headers: HeaderMap,
    request: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<ViewedFile>,
) -> Result<StatusCode, ApiError> {
    set_viewed(state, path, headers, request, body.path, false).await
}

#[derive(Deserialize)]
pub(crate) struct ListQuery {
    filter: Option<PullFilter>,
    q: Option<String>,
    cursor: Option<String>,
}

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/projects/{project_id}/github/pulls", params(("workspace_id" = String, Path), ("project_id" = String, Path), ("filter" = Option<PullFilter>, Query), ("q" = Option<String>, Query), ("cursor" = Option<String>, Query)), responses((status = 200, body = PullList)))]
pub(crate) async fn list_github_project_pulls(
    State(state): State<IntegrationState>,
    Path((workspace, project)): Path<(String, String)>,
    Query(query): Query<ListQuery>,
    headers: HeaderMap,
    request: Option<Extension<RequestId>>,
) -> Result<Json<PullList>, ApiError> {
    let id = request_id(request.as_ref());
    let (workspace_id, project_id, _) = github_project_access(&state, &headers, &workspace, &project, id).await?;
    let user = state.session_user(&headers, id).await?;
    let connection = sqlx::query("SELECT repository, label FROM github_project_connections WHERE workspace_id = $1 AND project_id = $2")
        .bind(&workspace)
        .bind(project_id.to_string())
        .fetch_optional(state.database().pool())
        .await
        .map_err(|_| ApiError::internal(id))?
        .ok_or_else(|| not_found(id))?;
    let repository: String = connection.get("repository");
    let label: String = connection.get("label");
    // The label tells the projects of one repository apart; a repository with one project lists all its PRs.
    let shared: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM github_project_connections c JOIN projects p ON p.id = c.project_id \
         WHERE c.workspace_id = $1 AND lower(c.repository) = lower($2) AND p.deleted_at IS NULL",
    )
    .bind(&workspace)
    .bind(&repository)
    .fetch_one(state.database().pool())
    .await
    .map_err(|_| ApiError::internal(id))?;
    let page = query
        .cursor
        .as_deref()
        .map_or(Ok(1), str::parse::<u32>)
        .ok()
        .filter(|page| (1..=40).contains(page))
        .ok_or_else(|| not_found(id))?;
    let token = user_token(&state, &workspace_id, &user)
        .await
        .map_err(|error| error.into_api(id))?;
    let result = pulls::list(
        &github(&state, id)?,
        &token.token,
        &repository.to_lowercase(),
        (shared > 1).then_some(label.as_str()),
        query.filter.unwrap_or(PullFilter::Open),
        query.q.as_deref().unwrap_or(""),
        page,
    )
    .await;
    match result {
        Ok(list) => Ok(Json(list)),
        Err(error) => {
            if matches!(error, GithubError::Unauthorized) {
                let _ = mark_broken(&state, &workspace_id, &user, &token.token).await;
            }
            Err(error.into_api(id))
        }
    }
}

// ---- writes (plan 3): comments, threads, review, merge, branch ----

use crate::github::pulls::{ReviewComment, ReviewThread};
use crate::github::reviews::{self, MergePull, MergeResult, NewReviewComment, ReviewRefusal, SubmitReview, valid_node_id};

fn unprocessable(code: &str, title: &str, detail: &str, request_id: Option<&RequestId>) -> ApiError {
    ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, code, title, detail, request_id)
}

type NodePath = Path<(String, String, String, i64, String)>;

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/github/pulls/{owner}/{repo}/{number}/comments", params(("workspace_id" = String, Path), ("owner" = String, Path), ("repo" = String, Path), ("number" = i64, Path)), request_body = NewReviewComment, responses((status = 200, body = ReviewThread)))]
pub(crate) async fn create_github_review_comment(
    State(state): State<IntegrationState>,
    Path((workspace, owner, repo, number)): PullPath,
    headers: HeaderMap,
    request: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<NewReviewComment>,
) -> Result<Json<ReviewThread>, ApiError> {
    let id = request_id(request.as_ref());
    let access = pull_access(&state, &headers, &workspace, &owner, &repo, number, id).await?;
    if !body.valid() {
        return Err(unprocessable("invalid_review_comment", "Invalid comment", "Give a side (LEFT or RIGHT), a line, a body and a mode (pending or single).", id));
    }
    match reviews::add_comment(&github(&state, id)?, &access.token.token, &access.pull, &body).await {
        Ok(thread) => Ok(Json(thread)),
        Err(GithubError::Conflict(_)) => Err(ApiError::new(StatusCode::CONFLICT, "pull_head_changed", "New commits", "New commits were pushed. Refresh to comment on the latest changes.", id)),
        Err(error) => Err(github_failed(&state, &access, error, id).await),
    }
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
#[schema(as = GithubCommentBody)]
pub struct CommentBody {
    /// Markdown.
    body: String,
}

fn check_body(body: &CommentBody, request_id: Option<&RequestId>) -> Result<(), ApiError> {
    if body.body.trim().is_empty() || body.body.len() > 65_536 {
        return Err(unprocessable("invalid_review_comment", "Invalid comment", "Write a comment first.", request_id));
    }
    Ok(())
}

/// The comment or thread `node` must belong to the pull request in the URL, whose project the member can see.
async fn check_node(state: &IntegrationState, access: &PullAccess, node: &str, id: Option<&RequestId>) -> Result<(), ApiError> {
    if !valid_node_id(node) {
        return Err(not_found(id));
    }
    match reviews::node_in_pull(&github(state, id)?, &access.token.token, &access.pull, node).await {
        Ok(true) => Ok(()),
        Ok(false) => Err(not_found(id)),
        Err(error) => Err(github_failed(state, access, error, id).await),
    }
}

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/github/pulls/{owner}/{repo}/{number}/threads/{thread_id}/replies", params(("workspace_id" = String, Path), ("owner" = String, Path), ("repo" = String, Path), ("number" = i64, Path), ("thread_id" = String, Path)), request_body = CommentBody, responses((status = 200, body = ReviewComment)))]
pub(crate) async fn reply_github_review_thread(
    State(state): State<IntegrationState>,
    Path((workspace, owner, repo, number, thread)): NodePath,
    headers: HeaderMap,
    request: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<CommentBody>,
) -> Result<Json<ReviewComment>, ApiError> {
    let id = request_id(request.as_ref());
    let access = pull_access(&state, &headers, &workspace, &owner, &repo, number, id).await?;
    check_node(&state, &access, &thread, id).await?;
    check_body(&body, id)?;
    match reviews::reply(&github(&state, id)?, &access.token.token, &access.pull, &thread, &body.body).await {
        Ok(comment) => Ok(Json(comment)),
        Err(error) => Err(github_failed(&state, &access, error, id).await),
    }
}

#[utoipa::path(patch, path = "/api/v1/workspaces/{workspace_id}/github/pulls/{owner}/{repo}/{number}/comments/{comment_id}", params(("workspace_id" = String, Path), ("owner" = String, Path), ("repo" = String, Path), ("number" = i64, Path), ("comment_id" = String, Path)), request_body = CommentBody, responses((status = 200, body = ReviewComment)))]
pub(crate) async fn update_github_review_comment(
    State(state): State<IntegrationState>,
    Path((workspace, owner, repo, number, comment)): NodePath,
    headers: HeaderMap,
    request: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<CommentBody>,
) -> Result<Json<ReviewComment>, ApiError> {
    let id = request_id(request.as_ref());
    let access = pull_access(&state, &headers, &workspace, &owner, &repo, number, id).await?;
    check_node(&state, &access, &comment, id).await?;
    check_body(&body, id)?;
    match reviews::update_comment(&github(&state, id)?, &access.token.token, &comment, &body.body).await {
        Ok(comment) => Ok(Json(comment)),
        Err(error) => Err(github_failed(&state, &access, error, id).await),
    }
}

#[utoipa::path(delete, path = "/api/v1/workspaces/{workspace_id}/github/pulls/{owner}/{repo}/{number}/comments/{comment_id}", params(("workspace_id" = String, Path), ("owner" = String, Path), ("repo" = String, Path), ("number" = i64, Path), ("comment_id" = String, Path)), responses((status = 204, description = "Deleted on GitHub")))]
pub(crate) async fn delete_github_review_comment(
    State(state): State<IntegrationState>,
    Path((workspace, owner, repo, number, comment)): NodePath,
    headers: HeaderMap,
    request: Option<Extension<RequestId>>,
) -> Result<StatusCode, ApiError> {
    let id = request_id(request.as_ref());
    let access = pull_access(&state, &headers, &workspace, &owner, &repo, number, id).await?;
    check_node(&state, &access, &comment, id).await?;
    match reviews::delete_comment(&github(&state, id)?, &access.token.token, &comment).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(error) => Err(github_failed(&state, &access, error, id).await),
    }
}

async fn set_resolved(
    state: IntegrationState,
    (workspace, owner, repo, number, thread): (String, String, String, i64, String),
    headers: HeaderMap,
    request: Option<Extension<RequestId>>,
    resolved: bool,
) -> Result<Json<ReviewThread>, ApiError> {
    let id = request_id(request.as_ref());
    let access = pull_access(&state, &headers, &workspace, &owner, &repo, number, id).await?;
    check_node(&state, &access, &thread, id).await?;
    match reviews::set_resolved(&github(&state, id)?, &access.token.token, &thread, resolved).await {
        Ok(thread) => Ok(Json(thread)),
        Err(error) => Err(github_failed(&state, &access, error, id).await),
    }
}

#[utoipa::path(put, path = "/api/v1/workspaces/{workspace_id}/github/pulls/{owner}/{repo}/{number}/threads/{thread_id}/resolved", params(("workspace_id" = String, Path), ("owner" = String, Path), ("repo" = String, Path), ("number" = i64, Path), ("thread_id" = String, Path)), responses((status = 200, body = ReviewThread)))]
pub(crate) async fn resolve_github_review_thread(
    State(state): State<IntegrationState>,
    Path(path): NodePath,
    headers: HeaderMap,
    request: Option<Extension<RequestId>>,
) -> Result<Json<ReviewThread>, ApiError> {
    set_resolved(state, path, headers, request, true).await
}

#[utoipa::path(delete, path = "/api/v1/workspaces/{workspace_id}/github/pulls/{owner}/{repo}/{number}/threads/{thread_id}/resolved", params(("workspace_id" = String, Path), ("owner" = String, Path), ("repo" = String, Path), ("number" = i64, Path), ("thread_id" = String, Path)), responses((status = 200, body = ReviewThread)))]
pub(crate) async fn unresolve_github_review_thread(
    State(state): State<IntegrationState>,
    Path(path): NodePath,
    headers: HeaderMap,
    request: Option<Extension<RequestId>>,
) -> Result<Json<ReviewThread>, ApiError> {
    set_resolved(state, path, headers, request, false).await
}

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/github/pulls/{owner}/{repo}/{number}/review", params(("workspace_id" = String, Path), ("owner" = String, Path), ("repo" = String, Path), ("number" = i64, Path)), request_body = SubmitReview, responses((status = 204, description = "Review submitted on GitHub")))]
pub(crate) async fn submit_github_review(
    State(state): State<IntegrationState>,
    Path((workspace, owner, repo, number)): PullPath,
    headers: HeaderMap,
    request: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<SubmitReview>,
) -> Result<StatusCode, ApiError> {
    let id = request_id(request.as_ref());
    let access = pull_access(&state, &headers, &workspace, &owner, &repo, number, id).await?;
    if !matches!(body.event.as_str(), "COMMENT" | "APPROVE" | "REQUEST_CHANGES") || body.body.len() > 65_536 {
        return Err(unprocessable("invalid_review", "Invalid review", "Choose Comment, Approve or Request changes.", id));
    }
    match reviews::submit_review(&github(&state, id)?, &access.token.token, &access.pull, &body).await {
        Ok(Ok(())) => Ok(StatusCode::NO_CONTENT),
        Ok(Err(ReviewRefusal::OwnPull)) => Err(unprocessable("cannot_review_own_pull", "Your own pull request", "You cannot approve or request changes on your own pull request.", id)),
        Ok(Err(ReviewRefusal::Empty)) => Err(unprocessable("review_empty", "Empty review", "Write a summary or add comments first.", id)),
        Err(error) => Err(github_failed(&state, &access, error, id).await),
    }
}

#[utoipa::path(delete, path = "/api/v1/workspaces/{workspace_id}/github/pulls/{owner}/{repo}/{number}/review", params(("workspace_id" = String, Path), ("owner" = String, Path), ("repo" = String, Path), ("number" = i64, Path)), responses((status = 204, description = "Pending review discarded")))]
pub(crate) async fn discard_github_review(
    State(state): State<IntegrationState>,
    Path((workspace, owner, repo, number)): PullPath,
    headers: HeaderMap,
    request: Option<Extension<RequestId>>,
) -> Result<StatusCode, ApiError> {
    let id = request_id(request.as_ref());
    let access = pull_access(&state, &headers, &workspace, &owner, &repo, number, id).await?;
    match reviews::discard_review(&github(&state, id)?, &access.token.token, &access.pull).await {
        Ok(true) => Ok(StatusCode::NO_CONTENT),
        Ok(false) => Err(ApiError::new(StatusCode::NOT_FOUND, "no_pending_review", "No pending review", "You have no pending review on this pull request.", id)),
        Err(error) => Err(github_failed(&state, &access, error, id).await),
    }
}

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/github/pulls/{owner}/{repo}/{number}/merge", params(("workspace_id" = String, Path), ("owner" = String, Path), ("repo" = String, Path), ("number" = i64, Path)), request_body = MergePull, responses((status = 200, body = MergeResult)))]
pub(crate) async fn merge_github_pull(
    State(state): State<IntegrationState>,
    Path((workspace, owner, repo, number)): PullPath,
    headers: HeaderMap,
    request: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<MergePull>,
) -> Result<Json<MergeResult>, ApiError> {
    let id = request_id(request.as_ref());
    let access = pull_access(&state, &headers, &workspace, &owner, &repo, number, id).await?;
    let sha_ok = (1..=64).contains(&body.expected_head_sha.len()) && body.expected_head_sha.bytes().all(|c| c.is_ascii_alphanumeric());
    if !matches!(body.method.as_str(), "merge" | "squash" | "rebase") || !sha_ok {
        return Err(unprocessable("invalid_merge", "Invalid merge", "Choose merge, squash or rebase.", id));
    }
    match reviews::merge(&github(&state, id)?, &access.token.token, &access.pull, &body).await {
        Ok(result) => Ok(Json(result)),
        Err(GithubError::Conflict(_)) => Err(ApiError::new(StatusCode::CONFLICT, "pull_head_changed", "New commits", "New commits were pushed. Refresh before you merge.", id)),
        Err(GithubError::MethodNotAllowed(message)) => Err(unprocessable("merge_blocked", "Cannot merge", &format!("GitHub says: {message}"), id)),
        Err(error) => Err(github_failed(&state, &access, error, id).await),
    }
}

#[utoipa::path(delete, path = "/api/v1/workspaces/{workspace_id}/github/pulls/{owner}/{repo}/{number}/branch", params(("workspace_id" = String, Path), ("owner" = String, Path), ("repo" = String, Path), ("number" = i64, Path)), responses((status = 204, description = "Head branch deleted")))]
pub(crate) async fn delete_github_pull_branch(
    State(state): State<IntegrationState>,
    Path((workspace, owner, repo, number)): PullPath,
    headers: HeaderMap,
    request: Option<Extension<RequestId>>,
) -> Result<StatusCode, ApiError> {
    let id = request_id(request.as_ref());
    let access = pull_access(&state, &headers, &workspace, &owner, &repo, number, id).await?;
    match reviews::delete_branch(&github(&state, id)?, &access.token.token, &access.pull).await {
        Ok(true) => Ok(StatusCode::NO_CONTENT),
        Ok(false) => Err(unprocessable("branch_not_deletable", "Branch stays", "Only the branch of a merged pull request in this repository can be deleted here.", id)),
        Err(error) => Err(github_failed(&state, &access, error, id).await),
    }
}
