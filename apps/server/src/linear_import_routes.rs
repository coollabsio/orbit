//! Linear import API under `/api/v1/workspaces/{workspace_id}/imports/linear`. Mirrors the Asana
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

use crate::auth_routes::CookieMode;
use crate::linear::import::{LinearImport, LinearImportError, LinearImportService};
use crate::repositories::identity::IdentityRepository;
use crate::task_routes::{ApiError, ApiJson, ApiQuery, authenticate_session, validation};

#[derive(Clone)]
pub struct LinearImportState {
    identity: Arc<IdentityRepository>,
    service: LinearImportService,
    cookie_mode: CookieMode,
}

impl LinearImportState {
    #[must_use]
    pub fn new(
        identity: Arc<IdentityRepository>,
        service: LinearImportService,
        cookie_mode: CookieMode,
    ) -> Self {
        Self {
            identity,
            service,
            cookie_mode,
        }
    }
}

pub fn linear_import_router(state: LinearImportState) -> Router {
    Router::new()
        .route(
            "/api/v1/workspaces/{workspace_id}/imports/linear",
            get(list_linear_imports).post(create_linear_import),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/imports/linear/{import_id}",
            get(get_linear_import),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/imports/linear/{import_id}/start",
            post(start_linear_import),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/imports/linear/{import_id}/cancel",
            post(cancel_linear_import),
        )
        .with_state(state)
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct CreateLinearImportBody {
    /// A Linear personal API key (`lin_api_…`). Validated with Linear, stored encrypted, and
    /// deleted when the import ends.
    token: String,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct StartLinearImportBody {
    /// Teams of the scan tree to import (each becomes a new Orbit project).
    team_ids: Vec<String>,
}

/// Optional parts of an import detail response.
#[derive(Clone, Copy, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
enum LinearImportInclude {
    /// The scan tree (up to 1,000 teams). Fetch it once when the import is `ready`;
    /// polling leaves it out.
    Tree,
}

#[derive(Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
#[serde(deny_unknown_fields)]
struct LinearImportDetailQuery {
    /// `tree` adds the scan tree; without it `tree` is `null`.
    #[param(inline)]
    include: Option<LinearImportInclude>,
}

#[derive(Serialize, ToSchema)]
struct LinearImportList {
    items: Vec<LinearImport>,
}

fn collection(workspace: &str) -> String {
    format!("/api/v1/workspaces/{workspace}/imports/linear")
}

async fn scope(
    state: &LinearImportState,
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

/// Validates the API key with Linear, stores it encrypted and starts the scan.
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/imports/linear", params(("workspace_id" = String, Path)), request_body = CreateLinearImportBody, responses((status = 201, body = LinearImport)))]
async fn create_linear_import(
    State(state): State<LinearImportState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<CreateLinearImportBody>,
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

/// The caller's 20 most recent Linear imports in this workspace (without trees).
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/imports/linear", params(("workspace_id" = String, Path)), responses((status = 200, body = LinearImportList)))]
async fn list_linear_imports(
    State(state): State<LinearImportState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<LinearImportList>, ApiError> {
    let instance = collection(&workspace);
    let request = request_id.as_ref();
    let (workspace_id, user_id) = scope(&state, &headers, &workspace, &instance, request).await?;
    state
        .service
        .list(workspace_id, user_id)
        .await
        .map(|items| Json(LinearImportList { items }))
        .map_err(|error| import_problem(error, &instance, request))
}

/// One of the caller's Linear imports: status, progress and report. The scan tree (once
/// scanned) only with `?include=tree`.
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/imports/linear/{import_id}", params(("workspace_id" = String, Path), ("import_id" = String, Path), LinearImportDetailQuery), responses((status = 200, body = LinearImport)))]
async fn get_linear_import(
    State(state): State<LinearImportState>,
    Path((workspace, import)): Path<(String, String)>,
    ApiQuery(query): ApiQuery<LinearImportDetailQuery>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<LinearImport>, ApiError> {
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
            query.include == Some(LinearImportInclude::Tree),
        )
        .await
        .map(Json)
        .map_err(|error| import_problem(error, &instance, request))
}

/// Starts a scanned import with the teams to import.
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/imports/linear/{import_id}/start", params(("workspace_id" = String, Path), ("import_id" = String, Path)), request_body = StartLinearImportBody, responses((status = 200, body = LinearImport)))]
async fn start_linear_import(
    State(state): State<LinearImportState>,
    Path((workspace, import)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<StartLinearImportBody>,
) -> Result<Json<LinearImport>, ApiError> {
    let instance = format!("{}/{import}/start", collection(&workspace));
    let request = request_id.as_ref();
    let (workspace_id, user_id) = scope(&state, &headers, &workspace, &instance, request).await?;
    let import_id = import_id(&import, &instance, request)?;
    if body.team_ids.is_empty() || body.team_ids.len() > 1_000 {
        return Err(validation("team_ids", &instance, request));
    }
    state
        .service
        .start(workspace_id, user_id, import_id, body.team_ids)
        .await
        .map(Json)
        .map_err(|error| import_problem(error, &instance, request))
}

/// Cancels a scan or import and deletes its API key; what was already imported stays.
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/imports/linear/{import_id}/cancel", params(("workspace_id" = String, Path), ("import_id" = String, Path)), responses((status = 200, body = LinearImport)))]
async fn cancel_linear_import(
    State(state): State<LinearImportState>,
    Path((workspace, import)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<LinearImport>, ApiError> {
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
        "linear_import_not_found",
        "Import not found",
        "The requested import was not found.",
        instance,
        request_id,
    )
}

fn import_problem(
    error: LinearImportError,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> ApiError {
    let (status, code, title, detail) = match error {
        LinearImportError::NotFound => return import_not_found(instance, request_id),
        LinearImportError::Invalid { field } => return validation(field, instance, request_id),
        LinearImportError::AppKeyMissing => (
            StatusCode::CONFLICT,
            "app_key_missing",
            "App key missing",
            "The server app key (ORBIT_APP_KEY) must be configured before importing from Linear.",
        ),
        LinearImportError::TokenInvalid => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "linear_token_invalid",
            "Linear API key invalid",
            "Linear rejected the API key. Paste a valid personal API key.",
        ),
        LinearImportError::LinearUnavailable => (
            StatusCode::BAD_GATEWAY,
            "linear_unavailable",
            "Linear unavailable",
            "Linear could not be reached. Try again in a moment.",
        ),
        LinearImportError::InProgress => (
            StatusCode::CONFLICT,
            "import_in_progress",
            "Import in progress",
            "You already have an import running. Wait for it to finish or cancel it.",
        ),
        LinearImportError::StateConflict => (
            StatusCode::CONFLICT,
            "linear_import_state_conflict",
            "Import state conflict",
            "The import is not in a state that allows this action.",
        ),
        LinearImportError::Unavailable(error) => {
            tracing::error!(error = %error, "Linear import storage failed");
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
