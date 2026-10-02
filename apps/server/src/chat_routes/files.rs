//! Chat files: upload (one multipart `file` field), download, and the files of a conversation.

use std::io;

use axum::extract::{DefaultBodyLimit, Extension, Path, Query, Request, State};
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use orbit_platform::{BlobStoreError, RequestId, UploadError};
use serde::Deserialize;
use utoipa::{IntoParams, ToSchema};

use super::{Call, ChatState, RequestIdExtension};
use crate::attachment_routes::{StageFailure, download_response, stage_single_file};
use crate::repositories::chat::{ChatError, ChatFileRecord, MessageRecord};
use crate::task_routes::{ApiError, validation};

pub(super) fn file_router(state: ChatState) -> Router {
    let request_limit =
        usize::try_from(state.chat.uploads().limits().max_request_bytes()).unwrap_or(usize::MAX);
    Router::new()
        .route(
            "/api/v1/workspaces/{workspace_id}/chat/files",
            post(upload_chat_file),
        )
        // Multipart stays streaming; this bounds boundaries, headers, and ignored fields too.
        .layer(DefaultBodyLimit::max(request_limit))
        .route(
            "/api/v1/workspaces/{workspace_id}/chat/files/{file_id}",
            get(download_chat_file),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/chat/conversations/{conversation_id}/files",
            get(list_chat_files),
        )
        .with_state(state)
}

#[derive(ToSchema)]
#[allow(dead_code)]
pub(crate) struct ChatFileUploadBody {
    #[schema(value_type = String, format = Binary)]
    file: Vec<u8>,
}

#[derive(ToSchema)]
#[schema(value_type = String, format = Binary)]
#[allow(dead_code)]
pub(crate) struct ChatFileDownload(Vec<u8>);

/// The size of an image in pixels, as the uploader measured it. Only a hint for the message
/// list to reserve space; values out of range are dropped.
#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct ChatFileSize {
    width: Option<u32>,
    height: Option<u32>,
}

/// Uploads a file for a message that is about to be sent. Pass its `id` in `file_ids` of the
/// send. A file that is not sent within a day is removed.
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/chat/files", params(ChatFileSize, ("workspace_id" = String, Path)), request_body(content = ChatFileUploadBody, content_type = "multipart/form-data"), responses((status = 201, body = ChatFileRecord)))]
pub(crate) async fn upload_chat_file(
    State(state): State<ChatState>,
    Path(workspace): Path<String>,
    request: Request,
) -> Result<(StatusCode, Json<ChatFileRecord>), ApiError> {
    let uri = request.uri().clone();
    let request_id: RequestIdExtension = request
        .extensions()
        .get::<RequestId>()
        .cloned()
        .map(Extension);
    // Authorize before the body is read.
    let call = Call::enter(&state, request.headers(), &uri, &workspace, &request_id).await?;
    let Query(size) = Query::<ChatFileSize>::try_from_uri(&uri)
        .map_err(|_| call.problem(ChatError::Invalid { field: "width" }))?;
    state
        .chat
        .authorize_upload(call.workspace_id, call.actor_id)
        .await
        .map_err(|error| call.problem(error))?;
    let mut staged = stage_single_file(
        state.chat.uploads(),
        request,
        call.workspace_id,
        call.actor_id,
    )
    .await
    .map_err(|failure| match failure {
        StageFailure::TooLarge => too_large(&call.instance, call.request_id),
        StageFailure::InvalidMultipart => invalid_multipart(&call.instance, call.request_id),
        StageFailure::Upload(error) => upload_problem(error, &call.instance, call.request_id),
    })?;
    let upload = staged.upload().clone();
    match state
        .chat
        .finalize_file(
            call.workspace_id,
            call.actor_id,
            &upload,
            size.width,
            size.height,
        )
        .await
    {
        Ok(file) => {
            staged.complete();
            Ok((StatusCode::CREATED, Json(file)))
        }
        Err(error) => {
            staged.discard().await;
            Err(call.problem(error))
        }
    }
}

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/chat/files/{file_id}", params(("workspace_id" = String, Path), ("file_id" = String, Path)), responses((status = 200, body = ChatFileDownload, content_type = "application/octet-stream")))]
pub(crate) async fn download_chat_file(
    State(state): State<ChatState>,
    Path((workspace, file)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<Response, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let file_id = call.id(&file)?;
    let download = state
        .chat
        .download_file(call.workspace_id, call.actor_id, file_id)
        .await
        .map_err(|error| call.problem(error))?;
    download_response(download).ok_or_else(|| {
        call.problem(ChatError::Unavailable(sqlx::Error::Protocol(
            "the download response could not be built".to_owned(),
        )))
    })
}

/// The messages of a conversation that have files, newest first (at most 100).
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/chat/conversations/{conversation_id}/files", params(("workspace_id" = String, Path), ("conversation_id" = String, Path)), responses((status = 200, body = Vec<MessageRecord>)))]
pub(crate) async fn list_chat_files(
    State(state): State<ChatState>,
    Path((workspace, conversation)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<Json<Vec<MessageRecord>>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let conversation_id = call.id(&conversation)?;
    call.read(
        state
            .chat
            .list_files(call.workspace_id, call.actor_id, conversation_id)
            .await,
    )
}

fn too_large(instance: &str, request_id: Option<&Extension<RequestId>>) -> ApiError {
    ApiError::new(
        StatusCode::PAYLOAD_TOO_LARGE,
        "upload_too_large",
        "Upload too large",
        "The upload exceeds the configured size limit.",
        instance,
        request_id,
    )
}

fn invalid_multipart(instance: &str, request_id: Option<&Extension<RequestId>>) -> ApiError {
    ApiError::new(
        StatusCode::BAD_REQUEST,
        "invalid_multipart",
        "Invalid multipart upload",
        "The request must contain exactly one file field named \"file\".",
        instance,
        request_id,
    )
}

pub(super) fn upload_problem(
    error: UploadError,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> ApiError {
    match error {
        UploadError::FileTooLarge { .. } | UploadError::RequestTooLarge { .. } => {
            too_large(instance, request_id)
        }
        UploadError::InvalidDisplayName => validation("file", instance, request_id),
        UploadError::BlobStore(BlobStoreError::Io { source, .. })
            if source.kind() == io::ErrorKind::FileTooLarge =>
        {
            too_large(instance, request_id)
        }
        UploadError::BlobStore(BlobStoreError::Io { source, .. })
            if source.kind() == io::ErrorKind::InvalidData =>
        {
            invalid_multipart(instance, request_id)
        }
        _ => ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            "Internal server error",
            "An unexpected error occurred. Use the request ID when contacting support.",
            instance,
            request_id,
        ),
    }
}
