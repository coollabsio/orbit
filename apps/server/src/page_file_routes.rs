//! Docs page files: `POST .../pages/{page_id}/files` (multipart, one `file` field) and
//! `GET .../pages/{page_id}/files/{file_id}` (download). Access follows the page's visibility.

use std::io;
use std::sync::Arc;

use axum::extract::{DefaultBodyLimit, Extension, Path, Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use orbit_platform::{BlobStoreError, Id, RequestId, UploadError, UploadService};
use utoipa::ToSchema;

use crate::attachment_routes::{StageFailure, download_response, stage_single_file};
use crate::auth_routes::CookieMode;
use crate::repositories::identity::IdentityRepository;
use crate::repositories::page_files::{PageFile, PageFileError, PageFileRepository};
use crate::task_routes::{ApiError, authenticate_session, request_id_value};

#[derive(Clone)]
pub struct PageFileState {
    identity: Arc<IdentityRepository>,
    files: Arc<PageFileRepository>,
    cookie_mode: CookieMode,
}

impl PageFileState {
    #[must_use]
    pub fn new(
        identity: Arc<IdentityRepository>,
        uploads: UploadService,
        cookie_mode: CookieMode,
    ) -> Self {
        Self {
            files: Arc::new(PageFileRepository::new(
                identity.database().clone(),
                uploads,
            )),
            identity,
            cookie_mode,
        }
    }

    /// The repository behind the routes (also used for server-side imports).
    #[must_use]
    pub fn repository(&self) -> Arc<PageFileRepository> {
        Arc::clone(&self.files)
    }
}

pub fn page_file_router(state: PageFileState) -> Router {
    let request_limit =
        usize::try_from(state.files.uploads().limits().max_request_bytes()).unwrap_or(usize::MAX);
    Router::new()
        .route(
            "/api/v1/workspaces/{workspace_id}/pages/{page_id}/files",
            post(upload_page_file),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/pages/{page_id}/files/{file_id}",
            get(download_page_file),
        )
        // Multipart stays streaming; this bounds boundaries, headers, and ignored fields too.
        .layer(DefaultBodyLimit::max(request_limit))
        .with_state(state)
}

#[derive(ToSchema)]
#[allow(dead_code)]
struct PageFileUploadBody {
    #[schema(value_type = String, format = Binary)]
    file: Vec<u8>,
}

#[derive(ToSchema)]
#[schema(value_type = String, format = Binary)]
#[allow(dead_code)]
struct PageFileDownload(Vec<u8>);

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/pages/{page_id}/files", params(("workspace_id" = String, Path), ("page_id" = String, Path)), request_body(content = PageFileUploadBody, content_type = "multipart/form-data"), responses((status = 201, body = PageFile)))]
async fn upload_page_file(
    State(state): State<PageFileState>,
    Path((workspace, page)): Path<(String, String)>,
    request: Request,
) -> Result<Response, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/pages/{page}/files");
    let request_id = request
        .extensions()
        .get::<RequestId>()
        .cloned()
        .map(Extension);
    let request_id = request_id.as_ref();
    // Authorize before the body is read.
    let session = authenticate_session(
        &state.identity,
        state.cookie_mode,
        request.headers(),
        &instance,
        request_id,
    )
    .await?;
    let actor_id = session.user.id;
    let workspace_id = parse_id(&workspace, &instance, request_id)?;
    let page_id = parse_id(&page, &instance, request_id)?;
    state
        .files
        .authorize_upload(workspace_id, page_id, actor_id)
        .await
        .map_err(|error| problem(error, &instance, request_id))?;
    let mut staged = stage_single_file(state.files.uploads(), request, workspace_id, actor_id)
        .await
        .map_err(|failure| match failure {
            StageFailure::TooLarge => too_large(&instance, request_id),
            StageFailure::InvalidMultipart => invalid_multipart(&instance, request_id),
            StageFailure::Upload(error) => upload_problem(error, &instance, request_id),
        })?;
    let upload = staged.upload().clone();
    match state
        .files
        .finalize(
            workspace_id,
            page_id,
            actor_id,
            &upload,
            request_id_value(request_id),
        )
        .await
    {
        Ok(file) => {
            staged.complete();
            Ok((StatusCode::CREATED, Json(file)).into_response())
        }
        Err(error) => {
            staged.discard().await;
            Err(problem(error, &instance, request_id))
        }
    }
}

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/pages/{page_id}/files/{file_id}", params(("workspace_id" = String, Path), ("page_id" = String, Path), ("file_id" = String, Path)), responses((status = 200, body = PageFileDownload, content_type = "application/octet-stream")))]
async fn download_page_file(
    State(state): State<PageFileState>,
    Path((workspace, page, file)): Path<(String, String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Response, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/pages/{page}/files/{file}");
    let request_id = request_id.as_ref();
    let session = authenticate_session(
        &state.identity,
        state.cookie_mode,
        &headers,
        &instance,
        request_id,
    )
    .await?;
    let workspace_id = parse_id(&workspace, &instance, request_id)?;
    let page_id = parse_id(&page, &instance, request_id)?;
    let file_id = parse_id(&file, &instance, request_id)?;
    let download = state
        .files
        .download(workspace_id, page_id, file_id, session.user.id)
        .await
        .map_err(|error| problem(error, &instance, request_id))?;
    download_response(download).ok_or_else(|| internal(&instance, request_id))
}

fn parse_id(
    value: &str,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<Id, ApiError> {
    value.parse().map_err(|_| not_found(instance, request_id))
}

fn not_found(instance: &str, request_id: Option<&Extension<RequestId>>) -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        "page_file_not_found",
        "File not found",
        "The requested page or file was not found.",
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

fn internal(instance: &str, request_id: Option<&Extension<RequestId>>) -> ApiError {
    ApiError::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        "internal_error",
        "Internal server error",
        "An unexpected error occurred. Use the request ID when contacting support.",
        instance,
        request_id,
    )
}

fn upload_problem(
    error: UploadError,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> ApiError {
    match error {
        UploadError::FileTooLarge { .. } | UploadError::RequestTooLarge { .. } => {
            too_large(instance, request_id)
        }
        UploadError::InvalidDisplayName => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "validation_failed",
            "Validation failed",
            "The file name is invalid.",
            instance,
            request_id,
        ),
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
        UploadError::Unauthorized => not_found(instance, request_id),
        _ => internal(instance, request_id),
    }
}

fn problem(
    error: PageFileError,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> ApiError {
    match error {
        PageFileError::NotFound => not_found(instance, request_id),
        PageFileError::Upload(error) => upload_problem(error, instance, request_id),
        PageFileError::Database(_) => internal(instance, request_id),
    }
}
