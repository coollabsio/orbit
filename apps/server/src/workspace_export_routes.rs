//! Full workspace export (`crate::workspace_export`): start one, list them, download a ZIP.
//! Owners and admins only; a read-only (billing) workspace still exports.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Extension, Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::Response;
use axum::routing::get;
use axum::{Json, Router};
use orbit_platform::RequestId;
use tokio_util::io::ReaderStream;
use utoipa::ToSchema;

use crate::auth_routes::CookieMode;
use crate::export::names::{ascii_file_name, rfc5987};
use crate::repositories::identity::IdentityRepository;
use crate::task_routes::{ApiError, authenticate_session, parse_id, request_id_value};
use crate::workspace_export::{
    ExportError, WorkspaceExport, WorkspaceExportList, WorkspaceExportService,
};

#[derive(Clone)]
pub struct WorkspaceExportState {
    identity: Arc<IdentityRepository>,
    exports: WorkspaceExportService,
    cookie_mode: CookieMode,
}

impl WorkspaceExportState {
    #[must_use]
    pub fn new(
        identity: Arc<IdentityRepository>,
        exports: WorkspaceExportService,
        cookie_mode: CookieMode,
    ) -> Self {
        Self {
            identity,
            exports,
            cookie_mode,
        }
    }
}

pub fn workspace_export_router(state: WorkspaceExportState) -> Router {
    Router::new()
        .route(
            "/api/v1/workspaces/{workspace_id}/exports",
            get(list_workspace_exports).post(create_workspace_export),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/exports/{export_id}/download",
            get(download_workspace_export),
        )
        .with_state(state)
}

#[derive(ToSchema)]
#[schema(value_type = String, format = Binary)]
#[allow(dead_code)]
struct WorkspaceExportArchive(Vec<u8>);

/// Starts an export of the whole workspace as a ZIP archive (tasks and chat as JSON, pages as
/// Markdown, and the files). It uses the caller's view: their private pages, and the private
/// channels and direct messages they are in. One export at a time per workspace (409).
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/exports", params(("workspace_id" = String, Path)), responses((status = 202, body = WorkspaceExport)))]
async fn create_workspace_export(
    State(state): State<WorkspaceExportState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<(StatusCode, Json<WorkspaceExport>), ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/exports");
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
    state
        .exports
        .create(workspace_id, session.user.id, request_id_value(request_id))
        .await
        .map(|export| (StatusCode::ACCEPTED, Json(export)))
        .map_err(|error| problem(error, &instance, request_id))
}

/// The workspace's exports that have not expired, newest first.
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/exports", params(("workspace_id" = String, Path)), responses((status = 200, body = WorkspaceExportList)))]
async fn list_workspace_exports(
    State(state): State<WorkspaceExportState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<WorkspaceExportList>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/exports");
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
    state
        .exports
        .list(workspace_id, session.user.id)
        .await
        .map(|items| Json(WorkspaceExportList { items }))
        .map_err(|error| problem(error, &instance, request_id))
}

/// The ZIP archive of a ready export (the layout is in its `README.txt`).
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/exports/{export_id}/download", params(("workspace_id" = String, Path), ("export_id" = String, Path)), responses((status = 200, body = WorkspaceExportArchive, content_type = "application/zip")))]
async fn download_workspace_export(
    State(state): State<WorkspaceExportState>,
    Path((workspace, export)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Response, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/exports/{export}/download");
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
    let export_id = parse_id(&export, &instance, request_id)?;
    let download = state
        .exports
        .download(
            workspace_id,
            export_id,
            session.user.id,
            request_id_value(request_id),
        )
        .await
        .map_err(|error| problem(error, &instance, request_id))?;
    let disposition = format!(
        "attachment; filename=\"{}\"; filename*={}",
        ascii_file_name(&download.file_name),
        rfc5987(&download.file_name)
    );
    let mut response = Response::new(Body::from_stream(ReaderStream::new(download.reader)));
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/zip"),
    );
    if let Ok(value) = HeaderValue::from_str(&disposition) {
        headers.insert(header::CONTENT_DISPOSITION, value);
    }
    headers.insert(
        header::CONTENT_LENGTH,
        HeaderValue::from(download.size_bytes),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    Ok(response)
}

fn problem(
    error: ExportError,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> ApiError {
    match error {
        ExportError::NotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            "workspace_resource_not_found",
            "Workspace resource not found",
            "The requested workspace resource was not found.",
            instance,
            request_id,
        ),
        ExportError::Forbidden => ApiError::new(
            StatusCode::FORBIDDEN,
            "workspace_action_forbidden",
            "Workspace action forbidden",
            "Only workspace owners and administrators can export the workspace.",
            instance,
            request_id,
        ),
        ExportError::InProgress => ApiError::new(
            StatusCode::CONFLICT,
            "export_in_progress",
            "Export in progress",
            "An export of this workspace is already being prepared. Wait until it is ready.",
            instance,
            request_id,
        ),
        ExportError::Cancelled | ExportError::Storage(_) | ExportError::Unavailable(_) => {
            tracing::error!(error = %error, "workspace export request failed");
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "Internal server error",
                "An unexpected error occurred. Use the request ID when contacting support.",
                instance,
                request_id,
            )
        }
    }
}
