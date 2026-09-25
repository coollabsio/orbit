use std::sync::Arc;

use axum::extract::{Extension, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch};
use axum::{Json, Router};
use orbit_platform::{Id, RequestId, TimestampMillis};
use serde::Deserialize;
use utoipa::ToSchema;

use crate::auth_routes::CookieMode;
use crate::repositories::identity::IdentityRepository;
use crate::repositories::teamspaces::{
    TeamspaceChanges, TeamspaceError, TeamspaceList, TeamspaceRecord, TeamspaceRepository,
};
use crate::task_routes::{
    ApiError, ApiJson, ApiQuery, MutationQuery, authenticate_session, bounded,
    deserialize_source_patch, request_id_value, validation,
};

#[derive(Clone)]
pub struct TeamspaceState {
    identity: Arc<IdentityRepository>,
    teamspaces: Arc<TeamspaceRepository>,
    cookie_mode: CookieMode,
}

impl TeamspaceState {
    #[must_use]
    pub fn new(identity: Arc<IdentityRepository>, cookie_mode: CookieMode) -> Self {
        Self {
            teamspaces: Arc::new(TeamspaceRepository::new(identity.database().clone())),
            identity,
            cookie_mode,
        }
    }
}

pub fn teamspace_router(state: TeamspaceState) -> Router {
    Router::new()
        .route(
            "/api/v1/workspaces/{workspace_id}/teamspaces",
            get(list_teamspaces).post(create_teamspace),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/teamspaces/{teamspace_id}",
            patch(update_teamspace).delete(delete_teamspace),
        )
        .with_state(state)
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct CreateTeamspaceBody {
    /// 1 to 100 characters after trimming.
    name: String,
    icon: Option<String>,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct TeamspaceUpdateBody {
    expected_version: u64,
    /// 1 to 100 characters after trimming.
    name: Option<String>,
    /// Absent: unchanged. `null`: cleared.
    #[serde(default, deserialize_with = "deserialize_source_patch")]
    icon: Option<Option<String>>,
}

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/teamspaces", params(("workspace_id" = String, Path)), responses((status = 200, body = TeamspaceList)))]
async fn list_teamspaces(
    State(state): State<TeamspaceState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<TeamspaceList>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/teamspaces");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    state
        .teamspaces
        .teamspaces(workspace_id, actor_id)
        .await
        .map(Json)
        .map_err(|error| teamspace_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/teamspaces", params(("workspace_id" = String, Path)), request_body = CreateTeamspaceBody, responses((status = 201, body = TeamspaceRecord)))]
async fn create_teamspace(
    State(state): State<TeamspaceState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<CreateTeamspaceBody>,
) -> Result<Response, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/teamspaces");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let request = request_id.as_ref();
    let name = name(body.name, &instance, request)?;
    let icon = body
        .icon
        .map(|value| icon(value, &instance, request))
        .transpose()?;
    state
        .teamspaces
        .create_teamspace(
            workspace_id,
            actor_id,
            name,
            icon,
            request_id_value(request),
            TimestampMillis::now(),
        )
        .await
        .map(|record| (StatusCode::CREATED, Json(record)).into_response())
        .map_err(|error| teamspace_problem(error, instance, request))
}

#[utoipa::path(patch, path = "/api/v1/workspaces/{workspace_id}/teamspaces/{teamspace_id}", params(("workspace_id" = String, Path), ("teamspace_id" = String, Path)), request_body = TeamspaceUpdateBody, responses((status = 200, body = TeamspaceRecord)))]
async fn update_teamspace(
    State(state): State<TeamspaceState>,
    Path((workspace, teamspace)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<TeamspaceUpdateBody>,
) -> Result<Json<TeamspaceRecord>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/teamspaces/{teamspace}");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let request = request_id.as_ref();
    let teamspace_id = parse_id(&teamspace, &instance, request)?;
    let changes = TeamspaceChanges {
        name: body
            .name
            .map(|value| name(value, &instance, request))
            .transpose()?,
        icon: body
            .icon
            .map(|value| {
                value
                    .map(|value| icon(value, &instance, request))
                    .transpose()
            })
            .transpose()?,
    };
    state
        .teamspaces
        .update_teamspace(
            workspace_id,
            teamspace_id,
            actor_id,
            body.expected_version,
            changes,
            request_id_value(request),
            TimestampMillis::now(),
        )
        .await
        .map(Json)
        .map_err(|error| teamspace_problem(error, instance, request))
}

#[utoipa::path(delete, path = "/api/v1/workspaces/{workspace_id}/teamspaces/{teamspace_id}", params(MutationQuery, ("workspace_id" = String, Path), ("teamspace_id" = String, Path)), responses((status = 204)))]
async fn delete_teamspace(
    State(state): State<TeamspaceState>,
    Path((workspace, teamspace)): Path<(String, String)>,
    ApiQuery(query): ApiQuery<MutationQuery>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<StatusCode, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/teamspaces/{teamspace}");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let teamspace_id = parse_id(&teamspace, &instance, request_id.as_ref())?;
    state
        .teamspaces
        .delete_teamspace(
            workspace_id,
            teamspace_id,
            actor_id,
            query.expected_version,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map(|()| StatusCode::NO_CONTENT)
        .map_err(|error| teamspace_problem(error, instance, request_id.as_ref()))
}

async fn scope(
    state: &TeamspaceState,
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
    Ok((parse_id(workspace, instance, request_id)?, session.user.id))
}

fn parse_id(
    value: &str,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<Id, ApiError> {
    value
        .parse()
        .map_err(|_| not_found(instance.to_owned(), request_id))
}

fn name(
    value: String,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<String, ApiError> {
    let value = value.trim().to_owned();
    if value.is_empty() {
        return Err(validation("name", instance, request_id));
    }
    bounded(value, 100, 400, "name", instance, request_id)
}

fn icon(
    value: String,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<String, ApiError> {
    bounded(value, 64, 256, "icon", instance, request_id)
}

fn not_found(instance: String, request_id: Option<&Extension<RequestId>>) -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        "teamspace_not_found",
        "Teamspace not found",
        "The requested teamspace was not found.",
        instance,
        request_id,
    )
}

fn teamspace_problem(
    error: TeamspaceError,
    instance: String,
    request_id: Option<&Extension<RequestId>>,
) -> ApiError {
    match error {
        TeamspaceError::NotFound => not_found(instance, request_id),
        TeamspaceError::Forbidden => ApiError::new(
            StatusCode::FORBIDDEN,
            "workspace_action_forbidden",
            "Workspace action forbidden",
            "Only workspace owners and administrators may delete teamspaces.",
            instance,
            request_id,
        ),
        TeamspaceError::NotEmpty => ApiError::new(
            StatusCode::CONFLICT,
            "teamspace_not_empty",
            "Teamspace not empty",
            "Move or delete the teamspace's pages before deleting it.",
            instance,
            request_id,
        ),
        TeamspaceError::LastTeamspace => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "last_teamspace",
            "Last teamspace",
            "A workspace keeps at least one teamspace.",
            instance,
            request_id,
        ),
        TeamspaceError::VersionConflict { current } => {
            ApiError::version_conflict(*current, instance, request_id)
        }
        TeamspaceError::Corrupt | TeamspaceError::Unavailable(_) => ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            "Internal server error",
            "An unexpected error occurred. Use the request ID when contacting support.",
            instance,
            request_id,
        ),
    }
}
