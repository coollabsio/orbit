//! Notion import API under `/api/v1/workspaces/{workspace_id}/imports/notion`. An import is
//! visible only to the member who created it; everyone else (admins included) gets 404.

use std::sync::Arc;

use axum::extract::{Extension, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use orbit_platform::{Id, RequestId};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::auth_routes::CookieMode;
use crate::notion::import::{
    ImportDestination, ImportError, ImportSelection, NotionImport, NotionImportService,
};
use crate::repositories::identity::IdentityRepository;
use crate::task_routes::{ApiError, ApiJson, authenticate_session, validation};

#[derive(Clone)]
pub struct ImportState {
    identity: Arc<IdentityRepository>,
    service: NotionImportService,
    cookie_mode: CookieMode,
}

impl ImportState {
    #[must_use]
    pub fn new(
        identity: Arc<IdentityRepository>,
        service: NotionImportService,
        cookie_mode: CookieMode,
    ) -> Self {
        Self {
            identity,
            service,
            cookie_mode,
        }
    }
}

pub fn import_router(state: ImportState) -> Router {
    Router::new()
        .route(
            "/api/v1/workspaces/{workspace_id}/imports/notion",
            get(list_notion_imports).post(create_notion_import),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/imports/notion/{import_id}",
            get(get_notion_import),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/imports/notion/{import_id}/start",
            post(start_notion_import),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/imports/notion/{import_id}/cancel",
            post(cancel_notion_import),
        )
        .with_state(state)
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct CreateNotionImportBody {
    /// A Notion personal access token (recommended) or internal connection token. Validated
    /// with Notion, stored encrypted, and deleted when the import ends.
    token: String,
}

/// `{ "all": true }` for everything the token can see, or `{ "notion_ids": [...] }` for these
/// pages and all their sub-pages.
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct NotionImportSelectionBody {
    all: Option<bool>,
    notion_ids: Option<Vec<String>>,
}

/// Where the imported top-level pages go. With `parent_page_id` they become its sub-pages
/// (and join its space); otherwise they go to the root of `teamspace_id`, of the caller's
/// private space (`private: true`), or of the default teamspace.
#[derive(Default, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct NotionImportDestinationBody {
    teamspace_id: Option<String>,
    private: Option<bool>,
    parent_page_id: Option<String>,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct StartNotionImportBody {
    selection: NotionImportSelectionBody,
    #[serde(default)]
    destination: NotionImportDestinationBody,
}

#[derive(Serialize, ToSchema)]
struct NotionImportList {
    items: Vec<NotionImport>,
}

fn collection(workspace: &str) -> String {
    format!("/api/v1/workspaces/{workspace}/imports/notion")
}

async fn scope(
    state: &ImportState,
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

/// Validates the token with Notion, stores it encrypted and starts the scan.
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/imports/notion", params(("workspace_id" = String, Path)), request_body = CreateNotionImportBody, responses((status = 201, body = NotionImport)))]
async fn create_notion_import(
    State(state): State<ImportState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<CreateNotionImportBody>,
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

/// The caller's 20 most recent imports in this workspace (without trees).
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/imports/notion", params(("workspace_id" = String, Path)), responses((status = 200, body = NotionImportList)))]
async fn list_notion_imports(
    State(state): State<ImportState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<NotionImportList>, ApiError> {
    let instance = collection(&workspace);
    let request = request_id.as_ref();
    let (workspace_id, user_id) = scope(&state, &headers, &workspace, &instance, request).await?;
    state
        .service
        .list(workspace_id, user_id)
        .await
        .map(|items| Json(NotionImportList { items }))
        .map_err(|error| import_problem(error, &instance, request))
}

/// One of the caller's imports: status, tree (once scanned), progress and report.
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/imports/notion/{import_id}", params(("workspace_id" = String, Path), ("import_id" = String, Path)), responses((status = 200, body = NotionImport)))]
async fn get_notion_import(
    State(state): State<ImportState>,
    Path((workspace, import)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<NotionImport>, ApiError> {
    let instance = format!("{}/{import}", collection(&workspace));
    let request = request_id.as_ref();
    let (workspace_id, user_id) = scope(&state, &headers, &workspace, &instance, request).await?;
    let import_id = import
        .parse()
        .map_err(|_| import_not_found(&instance, request))?;
    state
        .service
        .get(workspace_id, user_id, import_id)
        .await
        .map(Json)
        .map_err(|error| import_problem(error, &instance, request))
}

/// Starts a scanned import with a selection and a destination.
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/imports/notion/{import_id}/start", params(("workspace_id" = String, Path), ("import_id" = String, Path)), request_body = StartNotionImportBody, responses((status = 200, body = NotionImport)))]
async fn start_notion_import(
    State(state): State<ImportState>,
    Path((workspace, import)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<StartNotionImportBody>,
) -> Result<Json<NotionImport>, ApiError> {
    let instance = format!("{}/{import}/start", collection(&workspace));
    let request = request_id.as_ref();
    let (workspace_id, user_id) = scope(&state, &headers, &workspace, &instance, request).await?;
    let import_id = import
        .parse()
        .map_err(|_| import_not_found(&instance, request))?;
    let selection = match (body.selection.all, body.selection.notion_ids) {
        (Some(true), None) => ImportSelection::All,
        (None | Some(false), Some(ids)) if !ids.is_empty() && ids.len() <= 5_000 => {
            ImportSelection::NotionIds(ids)
        }
        _ => return Err(validation("selection", &instance, request)),
    };
    let destination = ImportDestination {
        teamspace_id: body
            .destination
            .teamspace_id
            .map(|id| {
                id.parse()
                    .map_err(|_| import_problem(ImportError::TeamspaceNotFound, &instance, request))
            })
            .transpose()?,
        private: body.destination.private,
        parent_page_id: body
            .destination
            .parent_page_id
            .map(|id| {
                id.parse()
                    .map_err(|_| import_problem(ImportError::PageNotFound, &instance, request))
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

/// Cancels a scan or import and deletes its token; pages already imported stay.
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/imports/notion/{import_id}/cancel", params(("workspace_id" = String, Path), ("import_id" = String, Path)), responses((status = 200, body = NotionImport)))]
async fn cancel_notion_import(
    State(state): State<ImportState>,
    Path((workspace, import)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<NotionImport>, ApiError> {
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
        "notion_import_not_found",
        "Import not found",
        "The requested import was not found.",
        instance,
        request_id,
    )
}

fn import_problem(
    error: ImportError,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> ApiError {
    let (status, code, title, detail) = match error {
        ImportError::NotFound => return import_not_found(instance, request_id),
        ImportError::Invalid { field } => return validation(field, instance, request_id),
        ImportError::AppKeyMissing => (
            StatusCode::CONFLICT,
            "app_key_missing",
            "App key missing",
            "The server app key (secrets.app_key) must be configured before importing from Notion.",
        ),
        ImportError::TokenInvalid => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "notion_token_invalid",
            "Notion token invalid",
            "Notion rejected the token. Paste a valid personal access token or connection token.",
        ),
        ImportError::NotionUnavailable => (
            StatusCode::BAD_GATEWAY,
            "notion_unavailable",
            "Notion unavailable",
            "Notion could not be reached. Try again in a moment.",
        ),
        ImportError::InProgress => (
            StatusCode::CONFLICT,
            "import_in_progress",
            "Import in progress",
            "You already have an import running. Wait for it to finish or cancel it.",
        ),
        ImportError::StateConflict => (
            StatusCode::CONFLICT,
            "notion_import_state_conflict",
            "Import state conflict",
            "The import is not in a state that allows this action.",
        ),
        ImportError::TooLarge => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "notion_import_too_large",
            "Import too large",
            "An import can contain at most 5,000 pages.",
        ),
        ImportError::PageNotFound => (
            StatusCode::NOT_FOUND,
            "page_not_found",
            "Page not found",
            "The destination page was not found.",
        ),
        ImportError::TeamspaceNotFound => (
            StatusCode::NOT_FOUND,
            "teamspace_not_found",
            "Teamspace not found",
            "The destination teamspace was not found.",
        ),
        ImportError::Unavailable(error) => {
            tracing::error!(error = %error, "Notion import storage failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "Internal server error",
                "An unexpected error occurred. Use the request ID when contacting support.",
            )
        }
    };
    ApiError::new(status, code, title, detail, instance, request_id)
}
