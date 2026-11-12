//! Asana import API under `/api/v1/workspaces/{workspace_id}/imports/asana`. Mirrors the Notion
//! import API: an import is visible only to the member who created it; everyone else (admins
//! included) gets 404.

use std::sync::Arc;

use axum::extract::{Extension, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use orbit_platform::{Id, RequestId};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::asana::import::{AsanaImport, AsanaImportError, AsanaImportService};
use crate::auth_routes::CookieMode;
use crate::repositories::identity::IdentityRepository;
use crate::task_routes::{ApiError, ApiJson, ApiQuery, authenticate_session, validation};

#[derive(Clone)]
pub struct AsanaImportState {
    identity: Arc<IdentityRepository>,
    service: AsanaImportService,
    cookie_mode: CookieMode,
}

impl AsanaImportState {
    #[must_use]
    pub fn new(
        identity: Arc<IdentityRepository>,
        service: AsanaImportService,
        cookie_mode: CookieMode,
    ) -> Self {
        Self {
            identity,
            service,
            cookie_mode,
        }
    }
}

pub fn asana_import_router(state: AsanaImportState) -> Router {
    Router::new()
        .route(
            "/api/v1/workspaces/{workspace_id}/imports/asana",
            get(list_asana_imports).post(create_asana_import),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/imports/asana/{import_id}",
            get(get_asana_import),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/imports/asana/{import_id}/start",
            post(start_asana_import),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/imports/asana/{import_id}/cancel",
            post(cancel_asana_import),
        )
        .with_state(state)
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct CreateAsanaImportBody {
    /// An Asana personal access token. Validated with Asana, stored encrypted, and deleted
    /// when the import ends.
    token: String,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct StartAsanaImportBody {
    /// Projects of the scan tree to import (each becomes a new Orbit project).
    project_gids: Vec<String>,
}

/// Optional parts of an import detail response.
#[derive(Clone, Copy, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
enum AsanaImportInclude {
    /// The scan tree (up to 5,000 projects). Fetch it once when the import is `ready`;
    /// polling leaves it out.
    Tree,
}

#[derive(Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
#[serde(deny_unknown_fields)]
struct AsanaImportDetailQuery {
    /// `tree` adds the scan tree; without it `tree` is `null`.
    #[param(inline)]
    include: Option<AsanaImportInclude>,
}

#[derive(Serialize, ToSchema)]
struct AsanaImportList {
    items: Vec<AsanaImport>,
}

fn collection(workspace: &str) -> String {
    format!("/api/v1/workspaces/{workspace}/imports/asana")
}

async fn scope(
    state: &AsanaImportState,
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

/// Validates the token with Asana, stores it encrypted and starts the scan.
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/imports/asana", params(("workspace_id" = String, Path)), request_body = CreateAsanaImportBody, responses((status = 201, body = AsanaImport)))]
async fn create_asana_import(
    State(state): State<AsanaImportState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<CreateAsanaImportBody>,
) -> Result<Response, ApiError> {
    let instance = collection(&workspace);
    let request = request_id.as_ref();
    let (workspace_id, user_id) = scope(&state, &headers, &workspace, &instance, request).await?;
    state
        .service
        .create(workspace_id, user_id, &body.token)
        .await
        .map(|import| (StatusCode::CREATED, Json(import)).into_response())
        .map_err(|error| import_problem(error, &instance, request))
}

/// The caller's 20 most recent Asana imports in this workspace (without trees).
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/imports/asana", params(("workspace_id" = String, Path)), responses((status = 200, body = AsanaImportList)))]
async fn list_asana_imports(
    State(state): State<AsanaImportState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<AsanaImportList>, ApiError> {
    let instance = collection(&workspace);
    let request = request_id.as_ref();
    let (workspace_id, user_id) = scope(&state, &headers, &workspace, &instance, request).await?;
    state
        .service
        .list(workspace_id, user_id)
        .await
        .map(|items| Json(AsanaImportList { items }))
        .map_err(|error| import_problem(error, &instance, request))
}

/// One of the caller's Asana imports: status, progress and report. The scan tree (once
/// scanned) only with `?include=tree`.
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/imports/asana/{import_id}", params(("workspace_id" = String, Path), ("import_id" = String, Path), AsanaImportDetailQuery), responses((status = 200, body = AsanaImport)))]
async fn get_asana_import(
    State(state): State<AsanaImportState>,
    Path((workspace, import)): Path<(String, String)>,
    ApiQuery(query): ApiQuery<AsanaImportDetailQuery>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<AsanaImport>, ApiError> {
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
            query.include == Some(AsanaImportInclude::Tree),
        )
        .await
        .map(Json)
        .map_err(|error| import_problem(error, &instance, request))
}

/// Starts a scanned import with the projects to import.
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/imports/asana/{import_id}/start", params(("workspace_id" = String, Path), ("import_id" = String, Path)), request_body = StartAsanaImportBody, responses((status = 200, body = AsanaImport)))]
async fn start_asana_import(
    State(state): State<AsanaImportState>,
    Path((workspace, import)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<StartAsanaImportBody>,
) -> Result<Json<AsanaImport>, ApiError> {
    let instance = format!("{}/{import}/start", collection(&workspace));
    let request = request_id.as_ref();
    let (workspace_id, user_id) = scope(&state, &headers, &workspace, &instance, request).await?;
    let import_id = import_id(&import, &instance, request)?;
    if body.project_gids.is_empty() || body.project_gids.len() > 5_000 {
        return Err(validation("project_gids", &instance, request));
    }
    state
        .service
        .start(workspace_id, user_id, import_id, body.project_gids)
        .await
        .map(Json)
        .map_err(|error| import_problem(error, &instance, request))
}

/// Cancels a scan or import and deletes its token; what was already imported stays.
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/imports/asana/{import_id}/cancel", params(("workspace_id" = String, Path), ("import_id" = String, Path)), responses((status = 200, body = AsanaImport)))]
async fn cancel_asana_import(
    State(state): State<AsanaImportState>,
    Path((workspace, import)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<AsanaImport>, ApiError> {
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
        "asana_import_not_found",
        "Import not found",
        "The requested import was not found.",
        instance,
        request_id,
    )
}

fn import_problem(
    error: AsanaImportError,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> ApiError {
    let (status, code, title, detail) = match error {
        AsanaImportError::NotFound => return import_not_found(instance, request_id),
        AsanaImportError::Invalid { field } => return validation(field, instance, request_id),
        AsanaImportError::AppKeyMissing => (
            StatusCode::CONFLICT,
            "app_key_missing",
            "App key missing",
            "The server app key (ORBIT_APP_KEY) must be configured before importing from Asana.",
        ),
        AsanaImportError::TokenInvalid => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "asana_token_invalid",
            "Asana token invalid",
            "Asana rejected the token. Paste a valid personal access token.",
        ),
        AsanaImportError::AsanaUnavailable => (
            StatusCode::BAD_GATEWAY,
            "asana_unavailable",
            "Asana unavailable",
            "Asana could not be reached. Try again in a moment.",
        ),
        AsanaImportError::InProgress => (
            StatusCode::CONFLICT,
            "import_in_progress",
            "Import in progress",
            "You already have an import running. Wait for it to finish or cancel it.",
        ),
        AsanaImportError::StateConflict => (
            StatusCode::CONFLICT,
            "asana_import_state_conflict",
            "Import state conflict",
            "The import is not in a state that allows this action.",
        ),
        AsanaImportError::Unavailable(error) => {
            tracing::error!(error = %error, "Asana import storage failed");
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
