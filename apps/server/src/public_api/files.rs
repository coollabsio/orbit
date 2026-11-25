//! Task and comment attachments, and doc files. Uploads are `multipart/form-data` with one
//! `file` field, as in the web app; the server detects the type from the bytes. Downloads stream
//! the bytes with the stored type, `Content-Disposition` and `nosniff`. Uploads and deletes act
//! as the token's user (a service-account token gets 403).
use axum::Router;
use axum::extract::rejection::QueryRejection;
use axum::extract::{DefaultBodyLimit, Path, Query, Request, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use orbit_platform::Id;
use utoipa::ToSchema;

use super::{Ctx, List, PageQuery, PublicApiState, list, problem_response};
use crate::attachment_routes::{StageFailure, StagedRequest, download_response, stage_single_file};
use crate::repositories::api_tokens::ApiTokenScope;
use crate::token_api::{
    Authorized, FileDownload, TokenApiError, TokenFile, TokenPageFile, too_large, upload_error,
};

pub(super) fn routes(state: &PublicApiState) -> Router<PublicApiState> {
    // Multipart stays streaming; this bounds the whole body, as the web upload routes do.
    let limit = DefaultBodyLimit::max(state.api.upload_request_limit());
    Router::new()
        .route(
            "/tasks/{task}/attachments",
            get(task_attachments).post(upload_task_attachment),
        )
        .route(
            "/tasks/{task}/attachments/{attachment_id}",
            delete(delete_task_attachment),
        )
        .route(
            "/tasks/{task}/attachments/{attachment_id}/download",
            get(download_task_attachment),
        )
        .route(
            "/tasks/{task}/comments/{comment_id}/attachments",
            get(comment_attachments).post(upload_comment_attachment),
        )
        .route(
            "/tasks/{task}/comments/{comment_id}/attachments/{attachment_id}",
            delete(delete_comment_attachment),
        )
        .route(
            "/tasks/{task}/comments/{comment_id}/attachments/{attachment_id}/download",
            get(download_comment_attachment),
        )
        .route("/docs/{page_id}/files", post(upload_doc_file))
        .route("/docs/{page_id}/files/{file_id}", get(download_doc_file))
        .layer(limit)
}

/// One `file` field.
#[derive(ToSchema)]
#[schema(as = PublicFileUploadBody)]
#[allow(dead_code)]
pub(super) struct UploadBody {
    /// The file, with its name in the part's `filename` (1 to 255 bytes).
    #[schema(value_type = String, format = Binary)]
    file: Vec<u8>,
}

#[derive(ToSchema)]
#[schema(as = PublicFileBytes, value_type = String, format = Binary)]
#[allow(dead_code)]
pub(super) struct FileBytes(Vec<u8>);

#[utoipa::path(
    get,
    path = "/api/public/v1/workspaces/{workspace_id}/tasks/{task}/attachments",
    operation_id = "public_list_task_attachments",
    tag = "public-api",
    summary = "The files of a task, oldest first",
    description = "Files of its comments and files placed in the description are not listed.",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("task" = String, Path, description = "A task UUID or identifier such as `ENG-12` (any letter case)."), PageQuery),
    responses((status = 200, description = "A page of files", body = List<TokenFile>)),
    security(("bearerAuth" = []))
)]
pub(super) async fn task_attachments(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, task)): Path<(String, String)>,
    query: Result<Query<PageQuery>, QueryRejection>,
) -> Response {
    let query = input!(ctx, ApiTokenScope::TasksRead, query);
    list_files(s, ctx, task, None, query).await
}

#[utoipa::path(
    get,
    path = "/api/public/v1/workspaces/{workspace_id}/tasks/{task}/comments/{comment_id}/attachments",
    operation_id = "public_list_comment_attachments",
    tag = "public-api",
    summary = "The files of a task comment, oldest first",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("task" = String, Path, description = "A task UUID or identifier such as `ENG-12` (any letter case)."), ("comment_id" = String, Path), PageQuery),
    responses((status = 200, description = "A page of files", body = List<TokenFile>)),
    security(("bearerAuth" = []))
)]
pub(super) async fn comment_attachments(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, task, comment)): Path<(String, String, String)>,
    query: Result<Query<PageQuery>, QueryRejection>,
) -> Response {
    let comment = path_id!(ctx, comment);
    let query = input!(ctx, ApiTokenScope::TasksRead, query);
    list_files(s, ctx, task, Some(comment), query).await
}

async fn list_files(
    s: PublicApiState,
    ctx: Ctx,
    task: String,
    comment: Option<Id>,
    query: PageQuery,
) -> Response {
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::TasksRead, &ctx.request_id)
            .await?;
        let page = api
            .attachments(&task, comment, query.cursor.as_deref(), query.limit)
            .await?;
        Ok(list::<_, TokenFile>(page.items, page.next_cursor))
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    post,
    path = "/api/public/v1/workspaces/{workspace_id}/tasks/{task}/attachments",
    operation_id = "public_upload_task_attachment",
    tag = "public-api",
    summary = "Attach a file to a task",
    description = "`multipart/form-data` with one `file` field. The server's upload limit and the workspace's storage quota apply (`413`). Not for service-account tokens (`403`).",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("task" = String, Path, description = "A task UUID or identifier such as `ENG-12` (any letter case).")),
    request_body(content = UploadBody, content_type = "multipart/form-data"),
    responses((status = 201, description = "The file", body = TokenFile)),
    security(("bearerAuth" = []))
)]
pub(super) async fn upload_task_attachment(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, task)): Path<(String, String)>,
    request: Request,
) -> Response {
    upload_file(s, ctx, task, None, request).await
}

#[utoipa::path(
    post,
    path = "/api/public/v1/workspaces/{workspace_id}/tasks/{task}/comments/{comment_id}/attachments",
    operation_id = "public_upload_comment_attachment",
    tag = "public-api",
    summary = "Attach a file to a task comment",
    description = "As for a task. Follows the comment's edit rule: its author, or a workspace owner or admin (`403` otherwise).",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("task" = String, Path, description = "A task UUID or identifier such as `ENG-12` (any letter case)."), ("comment_id" = String, Path)),
    request_body(content = UploadBody, content_type = "multipart/form-data"),
    responses((status = 201, description = "The file", body = TokenFile)),
    security(("bearerAuth" = []))
)]
pub(super) async fn upload_comment_attachment(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, task, comment)): Path<(String, String, String)>,
    request: Request,
) -> Response {
    let comment = path_id!(ctx, comment);
    upload_file(s, ctx, task, Some(comment), request).await
}

async fn upload_file(
    s: PublicApiState,
    ctx: Ctx,
    task: String,
    comment: Option<Id>,
    request: Request,
) -> Response {
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::TasksWrite, &ctx.request_id)
            .await?;
        // Checked before the body is read.
        let target = api.start_upload(&task, comment).await?;
        let mut staged = stage(&api, request).await?;
        let upload = staged.upload().clone();
        finish(&mut staged, api.finish_upload(&target, &upload).await).await
    }
    .await;
    ctx.reply(StatusCode::CREATED, result)
}

#[utoipa::path(
    delete,
    path = "/api/public/v1/workspaces/{workspace_id}/tasks/{task}/attachments/{attachment_id}",
    operation_id = "public_delete_task_attachment",
    tag = "public-api",
    summary = "Delete a file of a task",
    description = "Not for service-account tokens (`403`).",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("task" = String, Path, description = "A task UUID or identifier such as `ENG-12` (any letter case)."), ("attachment_id" = String, Path)),
    responses((status = 204, description = "The file is deleted")),
    security(("bearerAuth" = []))
)]
pub(super) async fn delete_task_attachment(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, task, attachment)): Path<(String, String, String)>,
) -> Response {
    let attachment = path_id!(ctx, attachment);
    delete_file(s, ctx, task, None, attachment).await
}

#[utoipa::path(
    delete,
    path = "/api/public/v1/workspaces/{workspace_id}/tasks/{task}/comments/{comment_id}/attachments/{attachment_id}",
    operation_id = "public_delete_comment_attachment",
    tag = "public-api",
    summary = "Delete a file of a task comment",
    description = "Follows the comment's edit rule (`403` otherwise). Not for service-account tokens.",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("task" = String, Path, description = "A task UUID or identifier such as `ENG-12` (any letter case)."), ("comment_id" = String, Path), ("attachment_id" = String, Path)),
    responses((status = 204, description = "The file is deleted")),
    security(("bearerAuth" = []))
)]
pub(super) async fn delete_comment_attachment(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, task, comment, attachment)): Path<(String, String, String, String)>,
) -> Response {
    let comment = path_id!(ctx, comment);
    let attachment = path_id!(ctx, attachment);
    delete_file(s, ctx, task, Some(comment), attachment).await
}

async fn delete_file(
    s: PublicApiState,
    ctx: Ctx,
    task: String,
    comment: Option<Id>,
    attachment: Id,
) -> Response {
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::TasksWrite, &ctx.request_id)
            .await?;
        api.delete_attachment(&task, comment, attachment).await
    }
    .await;
    match result {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => ctx.error(error),
    }
}

#[utoipa::path(
    get,
    path = "/api/public/v1/workspaces/{workspace_id}/tasks/{task}/attachments/{attachment_id}/download",
    operation_id = "public_download_task_attachment",
    tag = "public-api",
    summary = "The bytes of a task's file",
    description = "With the file's type and `Content-Disposition` (with its name). A file that an admin deleted answers `410` with a \"Deleted file\" image.",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("task" = String, Path, description = "A task UUID or identifier such as `ENG-12` (any letter case)."), ("attachment_id" = String, Path)),
    responses((status = 200, description = "The file", body = FileBytes, content_type = "application/octet-stream")),
    security(("bearerAuth" = []))
)]
pub(super) async fn download_task_attachment(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, task, attachment)): Path<(String, String, String)>,
) -> Response {
    let attachment = path_id!(ctx, attachment);
    download_file(s, ctx, task, None, attachment).await
}

#[utoipa::path(
    get,
    path = "/api/public/v1/workspaces/{workspace_id}/tasks/{task}/comments/{comment_id}/attachments/{attachment_id}/download",
    operation_id = "public_download_comment_attachment",
    tag = "public-api",
    summary = "The bytes of a task comment's file",
    description = "As for a task's file.",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("task" = String, Path, description = "A task UUID or identifier such as `ENG-12` (any letter case)."), ("comment_id" = String, Path), ("attachment_id" = String, Path)),
    responses((status = 200, description = "The file", body = FileBytes, content_type = "application/octet-stream")),
    security(("bearerAuth" = []))
)]
pub(super) async fn download_comment_attachment(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, task, comment, attachment)): Path<(String, String, String, String)>,
) -> Response {
    let comment = path_id!(ctx, comment);
    let attachment = path_id!(ctx, attachment);
    download_file(s, ctx, task, Some(comment), attachment).await
}

async fn download_file(
    s: PublicApiState,
    ctx: Ctx,
    task: String,
    comment: Option<Id>,
    attachment: Id,
) -> Response {
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::TasksRead, &ctx.request_id)
            .await?;
        api.download_attachment(&task, comment, attachment).await
    }
    .await;
    stream(&ctx, result)
}

#[utoipa::path(
    post,
    path = "/api/public/v1/workspaces/{workspace_id}/docs/{page_id}/files",
    operation_id = "public_upload_doc_file",
    tag = "public-api",
    summary = "Upload a file to a doc",
    description = "`multipart/form-data` with one `file` field. Put the answer's `url` in the doc's Markdown (`![name](url)` for an image) with `PATCH /docs/{page_id}`, or use it as the cover. The upload limit and the storage quota apply (`413`). Not for service-account tokens (`404`). A locked doc answers `423`.",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("page_id" = String, Path)),
    request_body(content = UploadBody, content_type = "multipart/form-data"),
    responses((status = 201, description = "The file", body = TokenPageFile)),
    security(("bearerAuth" = []))
)]
pub(super) async fn upload_doc_file(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, page)): Path<(String, String)>,
    request: Request,
) -> Response {
    let page = path_id!(ctx, page);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::DocsWrite, &ctx.request_id)
            .await?;
        let target = api.start_doc_upload(page).await?;
        let mut staged = stage(&api, request).await?;
        let upload = staged.upload().clone();
        finish(&mut staged, api.finish_doc_upload(&target, &upload).await).await
    }
    .await;
    ctx.reply(StatusCode::CREATED, result)
}

#[utoipa::path(
    get,
    path = "/api/public/v1/workspaces/{workspace_id}/docs/{page_id}/files/{file_id}",
    operation_id = "public_download_doc_file",
    tag = "public-api",
    summary = "The bytes of a doc's file",
    description = "The `{file_id}` of a doc file `url` (`/api/v1/workspaces/…/pages/{page_id}/files/{file_id}`). With the file's type and `Content-Disposition`.",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("page_id" = String, Path), ("file_id" = String, Path)),
    responses((status = 200, description = "The file", body = FileBytes, content_type = "application/octet-stream")),
    security(("bearerAuth" = []))
)]
pub(super) async fn download_doc_file(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, page, file)): Path<(String, String, String)>,
) -> Response {
    let page = path_id!(ctx, page);
    let file = path_id!(ctx, file);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::DocsRead, &ctx.request_id)
            .await?;
        api.download_doc_file(page, file).await
    }
    .await;
    stream(&ctx, result)
}

/// Streams the request's one `file` field into a staged upload.
async fn stage(api: &Authorized<'_>, request: Request) -> Result<StagedRequest, TokenApiError> {
    let (workspace_id, actor_id) = api.uploader();
    stage_single_file(api.uploads()?, request, workspace_id, actor_id)
        .await
        .map_err(|failure| match failure {
            StageFailure::TooLarge => too_large(),
            StageFailure::InvalidMultipart => TokenApiError::Invalid(
                "Send multipart/form-data with exactly one file field named `file`",
            ),
            StageFailure::Upload(error) => upload_error(error),
        })
}

/// Keeps the staged bytes when the write succeeded, else drops them.
async fn finish<T>(
    staged: &mut StagedRequest,
    result: Result<T, TokenApiError>,
) -> Result<T, TokenApiError> {
    match result {
        Ok(value) => {
            staged.complete();
            Ok(value)
        }
        Err(error) => {
            staged.discard().await;
            Err(error)
        }
    }
}

fn stream(ctx: &Ctx, result: Result<FileDownload, TokenApiError>) -> Response {
    match result {
        Ok(download) => download_response(download.blob).unwrap_or_else(|| {
            ctx.error(TokenApiError::Internal(
                "invalid stored file headers".to_owned(),
            ))
        }),
        Err(error) => ctx.error(error),
    }
}
