//! Trello import API under `/api/v1/workspaces/{workspace_id}/imports/trello`. Mirrors the Asana
//! and Linear import APIs: an import is visible only to the member who created it; everyone else (admins
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
use crate::repositories::identity::IdentityRepository;
use crate::task_routes::{ApiError, ApiJson, ApiQuery, authenticate_session, validation};
use crate::trello::import::{
    TrelloCredential, TrelloImport, TrelloImportError, TrelloImportService,
};

#[derive(Clone)]
pub struct TrelloImportState {
    identity: Arc<IdentityRepository>,
    service: TrelloImportService,
    cookie_mode: CookieMode,
}

impl TrelloImportState {
    #[must_use]
    pub fn new(
        identity: Arc<IdentityRepository>,
        service: TrelloImportService,
        cookie_mode: CookieMode,
    ) -> Self {
        Self {
            identity,
            service,
            cookie_mode,
        }
    }
}

pub fn trello_import_router(state: TrelloImportState) -> Router {
    Router::new()
        .route(
            "/api/v1/workspaces/{workspace_id}/imports/trello",
            get(list_trello_imports).post(create_trello_import),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/imports/trello/{import_id}",
            get(get_trello_import),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/imports/trello/{import_id}/start",
            post(start_trello_import),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/imports/trello/{import_id}/cancel",
            post(cancel_trello_import),
        )
        .with_state(state)
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct CreateTrelloImportBody {
    /// The API key of a Trello Power-Up (its Trello Auth tab in the Power-Up admin portal).
    key: String,
    /// A user token authorized for that key (`scope=read`). Both are validated with Trello,
    /// stored encrypted, and deleted when the import ends.
    token: String,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct StartTrelloImportBody {
    /// Boards of the scan tree to import (each becomes a new Orbit project).
    board_ids: Vec<String>,
}

/// Optional parts of an import detail response.
#[derive(Clone, Copy, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
enum TrelloImportInclude {
    /// The scan tree (up to 1,000 boards). Fetch it once when the import is `ready`;
    /// polling leaves it out.
    Tree,
}

#[derive(Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
#[serde(deny_unknown_fields)]
struct TrelloImportDetailQuery {
    /// `tree` adds the scan tree; without it `tree` is `null`.
    #[param(inline)]
    include: Option<TrelloImportInclude>,
}

#[derive(Serialize, ToSchema)]
struct TrelloImportList {
    items: Vec<TrelloImport>,
}

fn collection(workspace: &str) -> String {
    format!("/api/v1/workspaces/{workspace}/imports/trello")
}

async fn scope(
    state: &TrelloImportState,
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

/// Validates the API key and token with Trello, stores both encrypted and starts the scan.
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/imports/trello", params(("workspace_id" = String, Path)), request_body = CreateTrelloImportBody, responses((status = 201, body = TrelloImport)))]
async fn create_trello_import(
    State(state): State<TrelloImportState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<CreateTrelloImportBody>,
) -> Result<Response, ApiError> {
    let instance = collection(&workspace);
    let request = request_id.as_ref();
    let (workspace_id, user_id) = scope(&state, &headers, &workspace, &instance, request).await?;
    state
        .service
        .create(workspace_id, user_id, &body.key, &body.token)
        .await
        .map(|import| (StatusCode::CREATED, Json(import)).into_response())
        .map_err(|error| import_problem(error, &instance, request))
}

/// The caller's 20 most recent Trello imports in this workspace (without trees).
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/imports/trello", params(("workspace_id" = String, Path)), responses((status = 200, body = TrelloImportList)))]
async fn list_trello_imports(
    State(state): State<TrelloImportState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<TrelloImportList>, ApiError> {
    let instance = collection(&workspace);
    let request = request_id.as_ref();
    let (workspace_id, user_id) = scope(&state, &headers, &workspace, &instance, request).await?;
    state
        .service
        .list(workspace_id, user_id)
        .await
        .map(|items| Json(TrelloImportList { items }))
        .map_err(|error| import_problem(error, &instance, request))
}

/// One of the caller's Trello imports: status, progress and report. The scan tree (once
/// scanned) only with `?include=tree`.
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/imports/trello/{import_id}", params(("workspace_id" = String, Path), ("import_id" = String, Path), TrelloImportDetailQuery), responses((status = 200, body = TrelloImport)))]
async fn get_trello_import(
    State(state): State<TrelloImportState>,
    Path((workspace, import)): Path<(String, String)>,
    ApiQuery(query): ApiQuery<TrelloImportDetailQuery>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<TrelloImport>, ApiError> {
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
            query.include == Some(TrelloImportInclude::Tree),
        )
        .await
        .map(Json)
        .map_err(|error| import_problem(error, &instance, request))
}

/// Starts a scanned import with the boards to import.
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/imports/trello/{import_id}/start", params(("workspace_id" = String, Path), ("import_id" = String, Path)), request_body = StartTrelloImportBody, responses((status = 200, body = TrelloImport)))]
async fn start_trello_import(
    State(state): State<TrelloImportState>,
    Path((workspace, import)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<StartTrelloImportBody>,
) -> Result<Json<TrelloImport>, ApiError> {
    let instance = format!("{}/{import}/start", collection(&workspace));
    let request = request_id.as_ref();
    let (workspace_id, user_id) = scope(&state, &headers, &workspace, &instance, request).await?;
    let import_id = import_id(&import, &instance, request)?;
    if body.board_ids.is_empty() || body.board_ids.len() > 1_000 {
        return Err(validation("board_ids", &instance, request));
    }
    state
        .service
        .start(workspace_id, user_id, import_id, body.board_ids)
        .await
        .map(Json)
        .map_err(|error| import_problem(error, &instance, request))
}

/// Cancels a scan or import and deletes its API key and token; what was already imported stays.
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/imports/trello/{import_id}/cancel", params(("workspace_id" = String, Path), ("import_id" = String, Path)), responses((status = 200, body = TrelloImport)))]
async fn cancel_trello_import(
    State(state): State<TrelloImportState>,
    Path((workspace, import)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<TrelloImport>, ApiError> {
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
        "trello_import_not_found",
        "Import not found",
        "The requested import was not found.",
        instance,
        request_id,
    )
}

fn import_problem(
    error: TrelloImportError,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> ApiError {
    let (status, code, title, detail) = match error {
        TrelloImportError::NotFound => return import_not_found(instance, request_id),
        TrelloImportError::Invalid { field } => return validation(field, instance, request_id),
        TrelloImportError::AppKeyMissing => (
            StatusCode::CONFLICT,
            "app_key_missing",
            "App key missing",
            "The server app key (ORBIT_APP_KEY) must be configured before importing from Trello.",
        ),
        TrelloImportError::CredentialsRejected {
            credential: TrelloCredential::Key,
            reason,
        } => {
            return ApiError::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "trello_key_invalid",
                "Trello API key invalid",
                "",
                instance,
                request_id,
            )
            .with_detail(format!(
                "Trello rejected this API key ({reason}). Copy the key again from the Trello Auth tab of your Power-Up."
            ));
        }
        TrelloImportError::CredentialsRejected {
            credential: TrelloCredential::Token,
            reason,
        } => {
            return ApiError::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "trello_token_invalid",
                "Trello token invalid",
                "",
                instance,
                request_id,
            )
            .with_detail(format!(
                "Trello rejected this token ({reason}). Generate a new token for this API key and paste it."
            ));
        }
        TrelloImportError::CredentialMalformed(TrelloCredential::Key) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "trello_key_invalid",
            "Trello API key invalid",
            "This is not a Trello API key: a key has only letters and digits. Copy it again from the Trello Auth tab of your Power-Up.",
        ),
        TrelloImportError::CredentialMalformed(TrelloCredential::Token) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "trello_token_invalid",
            "Trello token invalid",
            "This is not a Trello token: a token has only letters and digits. Copy it again from the authorization page.",
        ),
        TrelloImportError::TrelloUnavailable => (
            StatusCode::BAD_GATEWAY,
            "trello_unavailable",
            "Trello unavailable",
            "Trello could not be reached. Try again in a moment.",
        ),
        TrelloImportError::InProgress => (
            StatusCode::CONFLICT,
            "import_in_progress",
            "Import in progress",
            "You already have an import running. Wait for it to finish or cancel it.",
        ),
        TrelloImportError::StateConflict => (
            StatusCode::CONFLICT,
            "trello_import_state_conflict",
            "Import state conflict",
            "The import is not in a state that allows this action.",
        ),
        TrelloImportError::Unavailable(error) => {
            tracing::error!(error = %error, "Trello import storage failed");
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
