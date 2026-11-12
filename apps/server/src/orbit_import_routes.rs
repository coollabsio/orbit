//! Orbit import API under `/api/v1/workspaces/{workspace_id}/imports/orbit`. Mirrors the Linear
//! import API: an import is visible only to the member who created it; everyone else (admins
//! included) gets 404. A full import needs an owner or admin of this workspace, a personal one
//! any member but a guest.

use std::sync::Arc;

use axum::extract::{DefaultBodyLimit, Extension, FromRequest, Multipart, Path, Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use orbit_platform::{Id, RequestId};
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use utoipa::{IntoParams, ToSchema};

use crate::attachment_routes::{has_length_limit, multipart_too_large};
use crate::auth_routes::CookieMode;
use crate::orbit_import::{
    OrbitImport, OrbitImportError, OrbitImportSelection, OrbitImportService, Upload,
};
use crate::repositories::identity::IdentityRepository;
use crate::task_routes::{ApiError, ApiJson, ApiQuery, authenticate_session, validation};

#[derive(Clone)]
pub struct OrbitImportState {
    identity: Arc<IdentityRepository>,
    service: OrbitImportService,
    cookie_mode: CookieMode,
}

impl OrbitImportState {
    #[must_use]
    pub fn new(
        identity: Arc<IdentityRepository>,
        service: OrbitImportService,
        cookie_mode: CookieMode,
    ) -> Self {
        Self {
            identity,
            service,
            cookie_mode,
        }
    }
}

/// The archive upload route ends with this path; the platform body limit lets it through.
pub const UPLOAD_PATH_SUFFIX: &str = "/imports/orbit/upload";

/// The largest upload request: the archive and room for the multipart boundaries and headers.
#[must_use]
pub fn upload_body_limit(max_archive_bytes: u64) -> usize {
    usize::try_from(max_archive_bytes)
        .unwrap_or(usize::MAX)
        .saturating_add(64 * 1024)
}

pub fn orbit_import_router(state: OrbitImportState) -> Router {
    let body_limit = upload_body_limit(state.service.max_archive_bytes());
    Router::new()
        .route(
            "/api/v1/workspaces/{workspace_id}/imports/orbit",
            get(list_orbit_imports).post(create_orbit_import),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/imports/orbit/upload",
            post(upload_orbit_import).layer(DefaultBodyLimit::max(body_limit)),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/imports/orbit/{import_id}",
            get(get_orbit_import),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/imports/orbit/{import_id}/start",
            post(start_orbit_import),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/imports/orbit/{import_id}/cancel",
            post(cancel_orbit_import),
        )
        .with_state(state)
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct CreateOrbitImportBody {
    /// The other Orbit instance, `https://orbit.example.com`.
    url: String,
    /// An export API token of the source workspace (`workspace:export` for a full import,
    /// `workspace:export:personal` for a personal one). Stored encrypted and deleted once the
    /// archive is downloaded.
    token: String,
}

/// The export archive as one `.zip` file in the `file` field.
#[derive(ToSchema)]
#[allow(dead_code)]
struct OrbitImportUploadBody {
    #[schema(value_type = String, format = Binary)]
    file: Vec<u8>,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct StartOrbitImportBody {
    selection: OrbitImportSelection,
}

/// Optional parts of an import detail response.
#[derive(Clone, Copy, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
enum OrbitImportInclude {
    /// The tree of the archive. Fetch it once when the import is `ready`; polling leaves it
    /// out.
    Tree,
}

#[derive(Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
#[serde(deny_unknown_fields)]
struct OrbitImportDetailQuery {
    /// `tree` adds the tree; without it `tree` is `null`.
    #[param(inline)]
    include: Option<OrbitImportInclude>,
}

#[derive(Serialize, ToSchema)]
struct OrbitImportList {
    imports: Vec<OrbitImport>,
}

fn collection(workspace: &str) -> String {
    format!("/api/v1/workspaces/{workspace}/imports/orbit")
}

async fn scope(
    state: &OrbitImportState,
    headers: &HeaderMap,
    workspace: &str,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<(Id, Id), ApiError> {
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
        .map_err(|_| import_not_found(instance, request_id))?;
    Ok((workspace_id, session.user.id))
}

fn import_id(
    value: &str,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<Id, ApiError> {
    value
        .parse()
        .map_err(|_| import_not_found(instance, request_id))
}

/// Validates the token with the other instance, stores it encrypted and starts the fetch.
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/imports/orbit", params(("workspace_id" = String, Path)), request_body = CreateOrbitImportBody, responses((status = 201, body = OrbitImport)))]
async fn create_orbit_import(
    State(state): State<OrbitImportState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<CreateOrbitImportBody>,
) -> Result<Response, ApiError> {
    let instance = collection(&workspace);
    let request = request_id.as_ref();
    let (workspace_id, user_id) = scope(&state, &headers, &workspace, &instance, request).await?;
    state
        .service
        .create(workspace_id, user_id, &body.url, &body.token)
        .await
        .map(|import| (StatusCode::CREATED, Json(import)).into_response())
        .map_err(|error| import_problem(error, &instance, request))
}

/// Why an upload could not be received.
enum Received {
    TooLarge,
    Invalid,
    Storage(String),
}

/// Streams the `file` field to the upload's path; returns its size.
async fn receive(request: Request, upload: &Upload, max_bytes: u64) -> Result<u64, Received> {
    let mut multipart = Multipart::from_request(request, &())
        .await
        .map_err(|error| {
            if has_length_limit(&error)
                || error.into_response().status() == StatusCode::PAYLOAD_TOO_LARGE
            {
                Received::TooLarge
            } else {
                Received::Invalid
            }
        })?;
    let multipart_error = |error: axum::extract::multipart::MultipartError| {
        if multipart_too_large(&error) {
            Received::TooLarge
        } else {
            Received::Invalid
        }
    };
    let storage = |error: std::io::Error| Received::Storage(error.to_string());
    while let Some(mut field) = multipart.next_field().await.map_err(multipart_error)? {
        if field.name() != Some("file") {
            continue;
        }
        let mut file = tokio::fs::File::create(&upload.path)
            .await
            .map_err(storage)?;
        let mut size = 0u64;
        while let Some(chunk) = field.chunk().await.map_err(multipart_error)? {
            size = size.saturating_add(chunk.len() as u64);
            if size > max_bytes {
                return Err(Received::TooLarge);
            }
            file.write_all(&chunk).await.map_err(storage)?;
        }
        file.flush().await.map_err(storage)?;
        return Ok(size);
    }
    Err(Received::Invalid)
}

/// Uploads an export archive (multipart, one `.zip` in `file`), stores it and reads it.
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/imports/orbit/upload", params(("workspace_id" = String, Path)), request_body(content = OrbitImportUploadBody, content_type = "multipart/form-data"), responses((status = 201, body = OrbitImport)))]
async fn upload_orbit_import(
    State(state): State<OrbitImportState>,
    Path(workspace): Path<String>,
    request: Request,
) -> Result<Response, ApiError> {
    let instance = format!("{}/upload", collection(&workspace));
    let request_id = request
        .extensions()
        .get::<RequestId>()
        .cloned()
        .map(Extension);
    let headers = request.headers().clone();
    let id = request_id.as_ref();
    // Authorize before the body is read.
    let (workspace_id, user_id) = scope(&state, &headers, &workspace, &instance, id).await?;
    let upload = state
        .service
        .prepare_upload(workspace_id, user_id)
        .await
        .map_err(|error| import_problem(error, &instance, id))?;
    let max_bytes = state.service.max_archive_bytes();
    let size = match receive(request, &upload, max_bytes).await {
        Ok(size) => size,
        Err(failure) => {
            let _ = tokio::fs::remove_file(&upload.path).await;
            let error = match failure {
                Received::TooLarge => OrbitImportError::ExportTooLarge,
                Received::Invalid => OrbitImportError::FormatUnsupported,
                Received::Storage(error) => OrbitImportError::Unavailable(error),
            };
            return Err(import_problem(error, &instance, id));
        }
    };
    state
        .service
        .create_upload(workspace_id, user_id, upload, size)
        .await
        .map(|import| (StatusCode::CREATED, Json(import)).into_response())
        .map_err(|error| import_problem(error, &instance, id))
}

/// The caller's 20 most recent Orbit imports in this workspace (without trees).
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/imports/orbit", params(("workspace_id" = String, Path)), responses((status = 200, body = OrbitImportList)))]
async fn list_orbit_imports(
    State(state): State<OrbitImportState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<OrbitImportList>, ApiError> {
    let instance = collection(&workspace);
    let request = request_id.as_ref();
    let (workspace_id, user_id) = scope(&state, &headers, &workspace, &instance, request).await?;
    state
        .service
        .list(workspace_id, user_id)
        .await
        .map(|imports| Json(OrbitImportList { imports }))
        .map_err(|error| import_problem(error, &instance, request))
}

/// One of the caller's Orbit imports: status, phase, progress and report. The tree (once
/// ready) only with `?include=tree`.
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/imports/orbit/{import_id}", params(("workspace_id" = String, Path), ("import_id" = String, Path), OrbitImportDetailQuery), responses((status = 200, body = OrbitImport)))]
async fn get_orbit_import(
    State(state): State<OrbitImportState>,
    Path((workspace, import)): Path<(String, String)>,
    ApiQuery(query): ApiQuery<OrbitImportDetailQuery>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<OrbitImport>, ApiError> {
    let instance = format!("{}/{import}", collection(&workspace));
    let request = request_id.as_ref();
    let (workspace_id, user_id) = scope(&state, &headers, &workspace, &instance, request).await?;
    let import_id = import_id(&import, &instance, request)?;
    state
        .service
        .get(
            workspace_id,
            user_id,
            import_id,
            query.include == Some(OrbitImportInclude::Tree),
        )
        .await
        .map(Json)
        .map_err(|error| import_problem(error, &instance, request))
}

/// Starts a ready import with what to import.
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/imports/orbit/{import_id}/start", params(("workspace_id" = String, Path), ("import_id" = String, Path)), request_body = StartOrbitImportBody, responses((status = 200, body = OrbitImport)))]
async fn start_orbit_import(
    State(state): State<OrbitImportState>,
    Path((workspace, import)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<StartOrbitImportBody>,
) -> Result<Json<OrbitImport>, ApiError> {
    let instance = format!("{}/{import}/start", collection(&workspace));
    let request = request_id.as_ref();
    let (workspace_id, user_id) = scope(&state, &headers, &workspace, &instance, request).await?;
    let import_id = import_id(&import, &instance, request)?;
    state
        .service
        .start(workspace_id, user_id, import_id, body.selection)
        .await
        .map(Json)
        .map_err(|error| import_problem(error, &instance, request))
}

/// Cancels a fetch or import and deletes its token and archive; what was already imported
/// stays.
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/imports/orbit/{import_id}/cancel", params(("workspace_id" = String, Path), ("import_id" = String, Path)), responses((status = 200, body = OrbitImport)))]
async fn cancel_orbit_import(
    State(state): State<OrbitImportState>,
    Path((workspace, import)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<OrbitImport>, ApiError> {
    let instance = format!("{}/{import}/cancel", collection(&workspace));
    let request = request_id.as_ref();
    let (workspace_id, user_id) = scope(&state, &headers, &workspace, &instance, request).await?;
    let import_id = import_id(&import, &instance, request)?;
    state
        .service
        .cancel(workspace_id, user_id, import_id)
        .await
        .map(Json)
        .map_err(|error| import_problem(error, &instance, request))
}

fn import_not_found(instance: &str, request_id: Option<&Extension<RequestId>>) -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        "orbit_import_not_found",
        "Import not found",
        "The requested import was not found.",
        instance,
        request_id,
    )
}

fn import_problem(
    error: OrbitImportError,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> ApiError {
    let (status, code, title, detail) = match error {
        OrbitImportError::NotFound => return import_not_found(instance, request_id),
        OrbitImportError::Invalid { field } => return validation(field, instance, request_id),
        OrbitImportError::AppKeyMissing => (
            StatusCode::CONFLICT,
            "app_key_missing",
            "App key missing",
            "The server app key (ORBIT_APP_KEY) must be configured before importing from another Orbit.",
        ),
        OrbitImportError::UrlInvalid => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "orbit_url_invalid",
            "Instance URL invalid",
            "Enter the other Orbit's address as https://orbit.example.com.",
        ),
        OrbitImportError::TokenInvalid => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "orbit_token_invalid",
            "API token invalid",
            "The other Orbit rejected the API token. Paste a valid export token.",
        ),
        OrbitImportError::ScopeMissing => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "orbit_scope_missing",
            "Export scope missing",
            "The API token is not an export token. Create one with the workspace:export or workspace:export:personal scope.",
        ),
        OrbitImportError::Unreachable => (
            StatusCode::BAD_GATEWAY,
            "orbit_unreachable",
            "Orbit instance unreachable",
            "The other Orbit could not be reached. Check the address and try again in a moment.",
        ),
        OrbitImportError::ExportTooLarge => (
            StatusCode::PAYLOAD_TOO_LARGE,
            "orbit_export_too_large",
            "Export too large",
            "The archive is larger than the server's import limit.",
        ),
        OrbitImportError::FormatUnsupported => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "orbit_format_unsupported",
            "Not an Orbit export",
            "Upload the ZIP archive of an Orbit workspace export.",
        ),
        OrbitImportError::InProgress => (
            StatusCode::CONFLICT,
            "import_in_progress",
            "Import in progress",
            "You already have an import running. Wait for it to finish or cancel it.",
        ),
        OrbitImportError::StateConflict => (
            StatusCode::CONFLICT,
            "orbit_import_state_conflict",
            "Import state conflict",
            "The import is not in a state that allows this action.",
        ),
        OrbitImportError::Forbidden => (
            StatusCode::FORBIDDEN,
            "forbidden",
            "Forbidden",
            "A full import needs a workspace owner or administrator.",
        ),
        OrbitImportError::Unavailable(error) => {
            tracing::error!(error = %error, "Orbit import storage failed");
            return ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "Internal server error",
                "An unexpected error occurred. Use the request ID when contacting support.",
                instance,
                request_id,
            )
            .cause(error);
        }
    };
    ApiError::new(status, code, title, detail, instance, request_id)
}
