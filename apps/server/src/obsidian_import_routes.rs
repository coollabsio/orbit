//! Obsidian import API under `/api/v1/workspaces/{workspace_id}/imports/obsidian`. An import is
//! visible only to the member who created it; everyone else (admins included) gets 404.

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
use crate::obsidian::import::{
    ImportDestination, ImportSelection, ObsidianImport, ObsidianImportError, ObsidianImportService,
    Upload,
};
use crate::repositories::identity::IdentityRepository;
use crate::task_routes::{ApiError, ApiJson, ApiQuery, authenticate_session, validation};

#[derive(Clone)]
pub struct ObsidianImportState {
    identity: Arc<IdentityRepository>,
    service: ObsidianImportService,
    cookie_mode: CookieMode,
}

impl ObsidianImportState {
    #[must_use]
    pub fn new(
        identity: Arc<IdentityRepository>,
        service: ObsidianImportService,
        cookie_mode: CookieMode,
    ) -> Self {
        Self {
            identity,
            service,
            cookie_mode,
        }
    }
}

pub fn obsidian_import_router(state: ObsidianImportState) -> Router {
    // Room for the multipart boundaries and headers around the archive.
    let body_limit = usize::try_from(state.service.max_archive_bytes())
        .unwrap_or(usize::MAX)
        .saturating_add(64 * 1024);
    Router::new()
        .route(
            "/api/v1/workspaces/{workspace_id}/imports/obsidian",
            get(list_obsidian_imports)
                .post(create_obsidian_import)
                .layer(DefaultBodyLimit::max(body_limit)),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/imports/obsidian/{import_id}",
            get(get_obsidian_import),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/imports/obsidian/{import_id}/start",
            post(start_obsidian_import),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/imports/obsidian/{import_id}/cancel",
            post(cancel_obsidian_import),
        )
        .with_state(state)
}

/// The vault as one `.zip` file in the `file` field.
#[derive(ToSchema)]
#[allow(dead_code)]
struct ObsidianImportUploadBody {
    #[schema(value_type = String, format = Binary)]
    file: Vec<u8>,
}

/// `{ "all": true }` for the whole vault, or `{ "paths": [...] }` for these folders and notes
/// (a folder brings everything in it).
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct ObsidianImportSelectionBody {
    all: Option<bool>,
    paths: Option<Vec<String>>,
}

/// Where the imported top-level pages go. With `parent_page_id` they become its sub-pages
/// (and join its space); otherwise they go to the root of `teamspace_id`, of the caller's
/// private space (`private: true`), or of the default teamspace.
#[derive(Default, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct ObsidianImportDestinationBody {
    teamspace_id: Option<String>,
    private: Option<bool>,
    parent_page_id: Option<String>,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct StartObsidianImportBody {
    selection: ObsidianImportSelectionBody,
    #[serde(default)]
    destination: ObsidianImportDestinationBody,
}

/// Optional parts of an import detail response.
#[derive(Clone, Copy, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
enum ObsidianImportInclude {
    /// The scan tree. Fetch it once when the import is `ready`; polling leaves it out.
    Tree,
}

#[derive(Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
#[serde(deny_unknown_fields)]
struct ObsidianImportDetailQuery {
    /// `tree` adds the scan tree; without it `tree` is `null`.
    #[param(inline)]
    include: Option<ObsidianImportInclude>,
}

#[derive(Serialize, ToSchema)]
struct ObsidianImportList {
    items: Vec<ObsidianImport>,
}

fn collection(workspace: &str) -> String {
    format!("/api/v1/workspaces/{workspace}/imports/obsidian")
}

async fn scope(
    state: &ObsidianImportState,
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

/// Why an upload could not be received.
enum Received {
    TooLarge,
    Invalid,
    Storage(String),
}

/// Streams the `file` field to the upload's path; returns the file name and size.
async fn receive(
    request: Request,
    upload: &Upload,
    max_bytes: u64,
) -> Result<(String, u64), Received> {
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
        let name = field.file_name().unwrap_or("vault.zip").to_owned();
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
        return Ok((name, size));
    }
    Err(Received::Invalid)
}

/// Uploads a vault (multipart, one `.zip` in `file`), stores it and starts the scan.
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/imports/obsidian", params(("workspace_id" = String, Path)), request_body(content = ObsidianImportUploadBody, content_type = "multipart/form-data"), responses((status = 201, body = ObsidianImport)))]
async fn create_obsidian_import(
    State(state): State<ObsidianImportState>,
    Path(workspace): Path<String>,
    request: Request,
) -> Result<Response, ApiError> {
    let instance = collection(&workspace);
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
        .prepare(workspace_id, user_id)
        .await
        .map_err(|error| import_problem(error, &instance, id))?;
    let max_bytes = state.service.max_archive_bytes();
    let (name, size) = match receive(request, &upload, max_bytes).await {
        Ok(received) => received,
        Err(failure) => {
            let _ = tokio::fs::remove_file(&upload.path).await;
            let error = match failure {
                Received::TooLarge => ObsidianImportError::ArchiveTooLarge(max_bytes),
                Received::Invalid => ObsidianImportError::ArchiveInvalid,
                Received::Storage(error) => ObsidianImportError::Unavailable(error),
            };
            return Err(import_problem(error, &instance, id));
        }
    };
    state
        .service
        .create(workspace_id, user_id, upload, &name, size)
        .await
        .map(|import| (StatusCode::CREATED, Json(import)).into_response())
        .map_err(|error| import_problem(error, &instance, id))
}

/// The caller's 20 most recent imports in this workspace (without trees).
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/imports/obsidian", params(("workspace_id" = String, Path)), responses((status = 200, body = ObsidianImportList)))]
async fn list_obsidian_imports(
    State(state): State<ObsidianImportState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<ObsidianImportList>, ApiError> {
    let instance = collection(&workspace);
    let request = request_id.as_ref();
    let (workspace_id, user_id) = scope(&state, &headers, &workspace, &instance, request).await?;
    state
        .service
        .list(workspace_id, user_id)
        .await
        .map(|items| Json(ObsidianImportList { items }))
        .map_err(|error| import_problem(error, &instance, request))
}

/// One of the caller's imports: status, progress and report. The scan tree (once scanned) only
/// with `?include=tree`.
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/imports/obsidian/{import_id}", params(("workspace_id" = String, Path), ("import_id" = String, Path), ObsidianImportDetailQuery), responses((status = 200, body = ObsidianImport)))]
async fn get_obsidian_import(
    State(state): State<ObsidianImportState>,
    Path((workspace, import)): Path<(String, String)>,
    ApiQuery(query): ApiQuery<ObsidianImportDetailQuery>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<ObsidianImport>, ApiError> {
    let instance = format!("{}/{import}", collection(&workspace));
    let request = request_id.as_ref();
    let (workspace_id, user_id) = scope(&state, &headers, &workspace, &instance, request).await?;
    let import_id = import
        .parse()
        .map_err(|_| import_not_found(&instance, request))?;
    state
        .service
        .get(
            workspace_id,
            user_id,
            import_id,
            query.include == Some(ObsidianImportInclude::Tree),
        )
        .await
        .map(Json)
        .map_err(|error| import_problem(error, &instance, request))
}

/// Starts a scanned import with a selection and a destination.
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/imports/obsidian/{import_id}/start", params(("workspace_id" = String, Path), ("import_id" = String, Path)), request_body = StartObsidianImportBody, responses((status = 200, body = ObsidianImport)))]
async fn start_obsidian_import(
    State(state): State<ObsidianImportState>,
    Path((workspace, import)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<StartObsidianImportBody>,
) -> Result<Json<ObsidianImport>, ApiError> {
    let instance = format!("{}/{import}/start", collection(&workspace));
    let request = request_id.as_ref();
    let (workspace_id, user_id) = scope(&state, &headers, &workspace, &instance, request).await?;
    let import_id = import
        .parse()
        .map_err(|_| import_not_found(&instance, request))?;
    let selection = match (body.selection.all, body.selection.paths) {
        (Some(true), None) => ImportSelection::All,
        (None | Some(false), Some(paths)) if !paths.is_empty() && paths.len() <= 5_000 => {
            ImportSelection::Paths(paths)
        }
        _ => return Err(validation("selection", &instance, request)),
    };
    let destination = ImportDestination {
        teamspace_id: body
            .destination
            .teamspace_id
            .map(|id| {
                id.parse().map_err(|_| {
                    import_problem(ObsidianImportError::TeamspaceNotFound, &instance, request)
                })
            })
            .transpose()?,
        private: body.destination.private,
        parent_page_id: body
            .destination
            .parent_page_id
            .map(|id| {
                id.parse().map_err(|_| {
                    import_problem(ObsidianImportError::PageNotFound, &instance, request)
                })
            })
            .transpose()?,
    };
    state
        .service
        .start(workspace_id, user_id, import_id, selection, destination)
        .await
        .map(Json)
        .map_err(|error| import_problem(error, &instance, request))
}

/// Cancels a scan or import and deletes the uploaded archive; pages already imported stay.
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/imports/obsidian/{import_id}/cancel", params(("workspace_id" = String, Path), ("import_id" = String, Path)), responses((status = 200, body = ObsidianImport)))]
async fn cancel_obsidian_import(
    State(state): State<ObsidianImportState>,
    Path((workspace, import)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<ObsidianImport>, ApiError> {
    let instance = format!("{}/{import}/cancel", collection(&workspace));
    let request = request_id.as_ref();
    let (workspace_id, user_id) = scope(&state, &headers, &workspace, &instance, request).await?;
    let import_id = import
        .parse()
        .map_err(|_| import_not_found(&instance, request))?;
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
        "obsidian_import_not_found",
        "Import not found",
        "The requested import was not found.",
        instance,
        request_id,
    )
}

fn import_problem(
    error: ObsidianImportError,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> ApiError {
    let (status, code, title, detail) = match error {
        ObsidianImportError::NotFound => return import_not_found(instance, request_id),
        ObsidianImportError::Invalid { field } => return validation(field, instance, request_id),
        ObsidianImportError::InProgress => (
            StatusCode::CONFLICT,
            "import_in_progress",
            "Import in progress",
            "You already have an import running. Wait for it to finish or cancel it.",
        ),
        ObsidianImportError::StateConflict => (
            StatusCode::CONFLICT,
            "obsidian_import_state_conflict",
            "Import state conflict",
            "The import is not in a state that allows this action.",
        ),
        ObsidianImportError::TooLarge => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "obsidian_import_too_large",
            "Import too large",
            "An import can contain at most 5,000 pages.",
        ),
        ObsidianImportError::ArchiveTooLarge(_) => (
            StatusCode::PAYLOAD_TOO_LARGE,
            "obsidian_archive_too_large",
            "Archive too large",
            "The archive is larger than the server's upload limit.",
        ),
        ObsidianImportError::ArchiveInvalid => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "obsidian_archive_invalid",
            "Not a zip archive",
            "Upload the vault as one .zip file.",
        ),
        ObsidianImportError::PageNotFound => (
            StatusCode::NOT_FOUND,
            "page_not_found",
            "Page not found",
            "The destination page was not found.",
        ),
        ObsidianImportError::TeamspaceNotFound => (
            StatusCode::NOT_FOUND,
            "teamspace_not_found",
            "Teamspace not found",
            "The destination teamspace was not found.",
        ),
        ObsidianImportError::Unavailable(error) => {
            tracing::error!(error = %error, "Obsidian import storage failed");
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
