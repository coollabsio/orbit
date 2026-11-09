//! People from Asana, Linear and Trello imports, under
//! `/api/v1/workspaces/{workspace_id}/import-people`: admins map each to a member, which assigns
//! all of their imported tasks.

use std::sync::Arc;

use axum::extract::{Extension, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, put};
use axum::{Json, Router};
use orbit_platform::{Database, Id, RequestId};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::auth_routes::CookieMode;
use crate::import_people::{self, ImportPeopleError, ImportPerson};
use crate::repositories::identity::IdentityRepository;
use crate::task_routes::{ApiError, ApiJson, authenticate_session, request_id_value, validation};

#[derive(Clone)]
pub struct ImportPeopleState {
    identity: Arc<IdentityRepository>,
    database: Database,
    cookie_mode: CookieMode,
}

impl ImportPeopleState {
    #[must_use]
    pub fn new(identity: Arc<IdentityRepository>, cookie_mode: CookieMode) -> Self {
        let database = identity.database().clone();
        Self {
            identity,
            database,
            cookie_mode,
        }
    }
}

pub fn import_people_router(state: ImportPeopleState) -> Router {
    Router::new()
        .route(
            "/api/v1/workspaces/{workspace_id}/import-people",
            get(list_import_people),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/import-people/{person_id}",
            put(update_import_person),
        )
        .with_state(state)
}

#[derive(Serialize, ToSchema)]
struct ImportPeopleList {
    items: Vec<ImportPerson>,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct UpdateImportPersonBody {
    /// The member who gets the person's tasks; `null` takes them back off the tasks.
    user_id: Option<String>,
}

fn collection(workspace: &str) -> String {
    format!("/api/v1/workspaces/{workspace}/import-people")
}

async fn scope(
    state: &ImportPeopleState,
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
        .map_err(|_| not_found(instance, request_id))?;
    Ok((workspace_id, session.user.id))
}

/// The people that Asana, Linear and Trello imports could not assign (no member had their
/// email), with their tasks that are not deleted. Workspace admins only.
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/import-people", params(("workspace_id" = String, Path)), responses((status = 200, body = ImportPeopleList)))]
async fn list_import_people(
    State(state): State<ImportPeopleState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<ImportPeopleList>, ApiError> {
    let instance = collection(&workspace);
    let request = request_id.as_ref();
    let (workspace_id, user_id) = scope(&state, &headers, &workspace, &instance, request).await?;
    import_people::list(&state.database, workspace_id, user_id)
        .await
        .map(|items| Json(ImportPeopleList { items }))
        .map_err(|error| problem(error, &instance, request))
}

/// Maps a person to a member (any member; the email need not match): their tasks lose the
/// previously mapped member and get this one, without notifications. Workspace admins only.
#[utoipa::path(put, path = "/api/v1/workspaces/{workspace_id}/import-people/{person_id}", params(("workspace_id" = String, Path), ("person_id" = String, Path)), request_body = UpdateImportPersonBody, responses((status = 200, body = ImportPerson)))]
async fn update_import_person(
    State(state): State<ImportPeopleState>,
    Path((workspace, person)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<UpdateImportPersonBody>,
) -> Result<Json<ImportPerson>, ApiError> {
    let instance = format!("{}/{person}", collection(&workspace));
    let request = request_id.as_ref();
    let (workspace_id, actor_id) = scope(&state, &headers, &workspace, &instance, request).await?;
    let person_id: Id = person.parse().map_err(|_| not_found(&instance, request))?;
    let user_id = body
        .user_id
        .map(|id| id.parse::<Id>())
        .transpose()
        .map_err(|_| validation("user_id", &instance, request))?;
    import_people::set_member(
        &state.database,
        workspace_id,
        actor_id,
        person_id,
        user_id,
        request_id_value(request),
    )
    .await
    .map(Json)
    .map_err(|error| problem(error, &instance, request))
}

fn not_found(instance: &str, request_id: Option<&Extension<RequestId>>) -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        "import_person_not_found",
        "Person not found",
        "The requested person from an import was not found.",
        instance,
        request_id,
    )
}

fn problem(
    error: ImportPeopleError,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> ApiError {
    match error {
        ImportPeopleError::NotFound => not_found(instance, request_id),
        ImportPeopleError::Forbidden => ApiError::new(
            StatusCode::FORBIDDEN,
            "workspace_action_forbidden",
            "Action forbidden",
            "Only workspace admins can assign people from imports.",
            instance,
            request_id,
        ),
        ImportPeopleError::InvalidMember => validation("user_id", instance, request_id),
        error => {
            tracing::error!(error = %error, "import people storage failed");
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
