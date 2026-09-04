use std::net::{IpAddr, Ipv4Addr};
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::{Extension, FromRequest, FromRequestParts, Path, Query, Request, State};
use axum::http::header::{CONTENT_TYPE, COOKIE, RETRY_AFTER};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post};
use axum::{Json, Router};
use orbit_domain::WorkspaceRole;
use orbit_platform::{
    ClientIp, Id, LoginThrottler, PasswordError, PasswordExecutor, PasswordService, RequestId,
    ThrottleDecision, TimestampMillis,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio_stream::wrappers::ReceiverStream;
use utoipa::ToSchema;

use crate::auth_routes::{CookieMode, issued_session_cookie};
use crate::repositories::identity::{AuthenticatedSession, IdentityRepository, SuspensionError};
use crate::repositories::workspaces::{InvitationDelivery, WorkspaceError, WorkspaceRepository};

#[derive(Clone)]
pub struct WorkspaceState {
    identity: Arc<IdentityRepository>,
    workspaces: Arc<WorkspaceRepository>,
    public_origin: String,
    cookie_mode: CookieMode,
    passwords: PasswordExecutor,
    registration_throttler: Arc<Mutex<LoginThrottler>>,
}

impl WorkspaceState {
    #[must_use]
    pub fn new(
        identity: Arc<IdentityRepository>,
        public_origin: String,
        cookie_mode: CookieMode,
    ) -> Self {
        Self {
            workspaces: Arc::new(WorkspaceRepository::new(identity.database().clone())),
            identity,
            public_origin: public_origin.trim_end_matches('/').to_owned(),
            cookie_mode,
            passwords: PasswordExecutor::new(PasswordService::default(), 2)
                .expect("password executor concurrency is non-zero"),
            registration_throttler: Arc::new(Mutex::new(LoginThrottler::new())),
        }
    }

    #[must_use]
    pub fn with_repository(
        identity: Arc<IdentityRepository>,
        workspaces: Arc<WorkspaceRepository>,
        public_origin: String,
        cookie_mode: CookieMode,
    ) -> Self {
        Self {
            identity,
            workspaces,
            public_origin: public_origin.trim_end_matches('/').to_owned(),
            cookie_mode,
            passwords: PasswordExecutor::new(PasswordService::default(), 2)
                .expect("password executor concurrency is non-zero"),
            registration_throttler: Arc::new(Mutex::new(LoginThrottler::new())),
        }
    }
}

pub fn workspace_router(state: WorkspaceState) -> Router {
    Router::new()
        .route(
            "/api/v1/workspaces",
            get(list_workspaces).post(create_workspace),
        )
        .route("/api/v1/workspaces/trash", get(list_trash))
        .route(
            "/api/v1/workspaces/invitations/accept",
            post(accept_invitation),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}",
            get(get_workspace)
                .patch(rename_workspace)
                .delete(delete_workspace),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/restore",
            post(restore_workspace),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/members",
            get(list_members),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/members/{membership_id}",
            patch(change_member_role).delete(remove_member),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/transfer-ownership",
            post(transfer_ownership),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/invitations",
            get(list_invitations).post(create_invitation),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/invitations/{invitation_id}",
            delete(revoke_invitation),
        )
        .route("/api/v1/workspaces/{workspace_id}/audit", get(list_audit))
        .route(
            "/api/v1/admin/users/{user_id}/suspension",
            post(set_account_suspension),
        )
        .route("/api/v1/admin/audit", get(list_global_audit))
        .route("/api/v1/admin/audit/export", get(export_global_audit))
        .with_state(state)
}

struct ApiJson<T>(T);

struct ApiQuery<T>(T);

impl<S, T> FromRequestParts<S> for ApiQuery<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let instance = parts.uri.path().to_owned();
        let request_id = parts.extensions.get::<RequestId>().cloned().map(Extension);
        Query::<T>::from_request_parts(parts, state)
            .await
            .map(|Query(value)| Self(value))
            .map_err(|_| {
                ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    "Invalid request",
                    "The request query is not valid for this endpoint.",
                    instance,
                    request_id.as_ref(),
                )
            })
    }
}

impl<S, T> FromRequest<S> for ApiJson<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = ApiError;

    async fn from_request(request: Request, state: &S) -> Result<Self, Self::Rejection> {
        let instance = request.uri().path().to_owned();
        let request_id = request
            .extensions()
            .get::<RequestId>()
            .cloned()
            .map(Extension);
        Json::<T>::from_request(request, state)
            .await
            .map(|Json(value)| Self(value))
            .map_err(|_| {
                ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    "Invalid request",
                    "The request body is not valid for this endpoint.",
                    instance,
                    request_id.as_ref(),
                )
            })
    }
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct CreateWorkspaceBody {
    name: String,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct RenameWorkspaceBody {
    name: String,
    expected_version: u64,
}

#[utoipa::path(post, path = "/api/v1/workspaces", request_body = CreateWorkspaceBody, responses((status = 201)))]
async fn create_workspace(
    State(state): State<WorkspaceState>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<CreateWorkspaceBody>,
) -> Result<Response, ApiError> {
    let session = authenticate(&state, &headers, "/api/v1/workspaces", request_id.as_ref()).await?;
    let name = nonempty_name(body.name, "/api/v1/workspaces", request_id.as_ref())?;
    let workspace = state
        .workspaces
        .create(
            session.user.id,
            name,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map_err(|error| workspace_problem(error, "/api/v1/workspaces", request_id.as_ref()))?;
    Ok((StatusCode::CREATED, Json(workspace)).into_response())
}

#[utoipa::path(get, path = "/api/v1/workspaces", responses((status = 200)))]
async fn list_workspaces(
    State(state): State<WorkspaceState>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<Vec<crate::repositories::workspaces::WorkspaceRecord>>, ApiError> {
    let session = authenticate(&state, &headers, "/api/v1/workspaces", request_id.as_ref()).await?;
    let workspaces = state
        .workspaces
        .list_for_user(session.user.id)
        .await
        .map_err(|error| workspace_problem(error, "/api/v1/workspaces", request_id.as_ref()))?;
    Ok(Json(workspaces))
}

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}", responses((status = 200)))]
async fn get_workspace(
    State(state): State<WorkspaceState>,
    Path(workspace_id): Path<String>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<crate::repositories::workspaces::WorkspaceRecord>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace_id}");
    let session = authenticate(&state, &headers, &instance, request_id.as_ref()).await?;
    let workspace_id = parse_id(&workspace_id, &instance, request_id.as_ref())?;
    state
        .workspaces
        .get(workspace_id, session.user.id)
        .await
        .map(Json)
        .map_err(|error| workspace_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(patch, path = "/api/v1/workspaces/{workspace_id}", request_body = RenameWorkspaceBody, responses((status = 200)))]
async fn rename_workspace(
    State(state): State<WorkspaceState>,
    Path(workspace_id): Path<String>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<RenameWorkspaceBody>,
) -> Result<Json<crate::repositories::workspaces::WorkspaceRecord>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace_id}");
    let session = authenticate(&state, &headers, &instance, request_id.as_ref()).await?;
    let workspace_id = parse_id(&workspace_id, &instance, request_id.as_ref())?;
    let name = nonempty_name(body.name, &instance, request_id.as_ref())?;
    state
        .workspaces
        .rename(
            workspace_id,
            session.user.id,
            name,
            body.expected_version,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map(Json)
        .map_err(|error| workspace_problem(error, instance, request_id.as_ref()))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PageQuery {
    cursor: Option<String>,
    #[serde(default = "default_limit")]
    limit: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MutationQuery {
    expected_version: u64,
}

#[derive(Serialize)]
struct Page<T> {
    items: Vec<T>,
    next_cursor: Option<Id>,
}

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/members", responses((status = 200)))]
async fn list_members(
    State(state): State<WorkspaceState>,
    Path(workspace_id): Path<String>,
    ApiQuery(page): ApiQuery<PageQuery>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<Page<crate::repositories::workspaces::MemberRecord>>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace_id}/members");
    let session = authenticate(&state, &headers, &instance, request_id.as_ref()).await?;
    let workspace_id = parse_id(&workspace_id, &instance, request_id.as_ref())?;
    let cursor = optional_id(page.cursor, &instance, request_id.as_ref())?;
    let (items, next_cursor) = state
        .workspaces
        .members(workspace_id, session.user.id, cursor, page.limit)
        .await
        .map_err(|error| workspace_problem(error, &instance, request_id.as_ref()))?;
    Ok(Json(Page { items, next_cursor }))
}

#[derive(Clone, Copy, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
enum RoleBody {
    Owner,
    Admin,
    Member,
}

impl From<RoleBody> for WorkspaceRole {
    fn from(value: RoleBody) -> Self {
        match value {
            RoleBody::Owner => Self::Owner,
            RoleBody::Admin => Self::Admin,
            RoleBody::Member => Self::Member,
        }
    }
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct RoleChangeBody {
    role: RoleBody,
    expected_version: u64,
}

#[utoipa::path(patch, path = "/api/v1/workspaces/{workspace_id}/members/{membership_id}", request_body = RoleChangeBody, responses((status = 204)))]
async fn change_member_role(
    State(state): State<WorkspaceState>,
    Path((workspace_id, membership_id)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<RoleChangeBody>,
) -> Result<StatusCode, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace_id}/members/{membership_id}");
    let session = authenticate(&state, &headers, &instance, request_id.as_ref()).await?;
    let workspace_id = parse_id(&workspace_id, &instance, request_id.as_ref())?;
    let membership_id = parse_id(&membership_id, &instance, request_id.as_ref())?;
    state
        .workspaces
        .change_role(
            workspace_id,
            membership_id,
            session.user.id,
            body.role.into(),
            body.expected_version,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map_err(|error| workspace_problem(error, instance, request_id.as_ref()))?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(delete, path = "/api/v1/workspaces/{workspace_id}/members/{membership_id}", responses((status = 204)))]
async fn remove_member(
    State(state): State<WorkspaceState>,
    Path((workspace_id, membership_id)): Path<(String, String)>,
    ApiQuery(mutation): ApiQuery<MutationQuery>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<StatusCode, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace_id}/members/{membership_id}");
    let session = authenticate(&state, &headers, &instance, request_id.as_ref()).await?;
    let workspace_id = parse_id(&workspace_id, &instance, request_id.as_ref())?;
    let membership_id = parse_id(&membership_id, &instance, request_id.as_ref())?;
    state
        .workspaces
        .remove_member(
            workspace_id,
            membership_id,
            session.user.id,
            mutation.expected_version,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map_err(|error| workspace_problem(error, instance, request_id.as_ref()))?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct TransferBody {
    membership_id: String,
    expected_version: u64,
    membership_version: u64,
}

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/transfer-ownership", request_body = TransferBody, responses((status = 204)))]
async fn transfer_ownership(
    State(state): State<WorkspaceState>,
    Path(workspace_id): Path<String>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<TransferBody>,
) -> Result<StatusCode, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace_id}/transfer-ownership");
    let session = authenticate(&state, &headers, &instance, request_id.as_ref()).await?;
    let workspace_id = parse_id(&workspace_id, &instance, request_id.as_ref())?;
    let membership_id = parse_id(&body.membership_id, &instance, request_id.as_ref())?;
    state
        .workspaces
        .transfer_ownership(
            workspace_id,
            membership_id,
            session.user.id,
            body.expected_version,
            body.membership_version,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map_err(|error| workspace_problem(error, instance, request_id.as_ref()))?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Clone, Copy, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
enum DeliveryBody {
    Manual,
    #[serde(alias = "email")]
    Smtp,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct InvitationBody {
    email: String,
    role: RoleBody,
    delivery: DeliveryBody,
}

#[derive(Serialize)]
struct InvitationResponse {
    invitation: crate::repositories::workspaces::InvitationRecord,
    #[serde(skip_serializing_if = "Option::is_none")]
    url: Option<String>,
}

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/invitations", request_body = InvitationBody, responses((status = 201)))]
async fn create_invitation(
    State(state): State<WorkspaceState>,
    Path(workspace_id): Path<String>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<InvitationBody>,
) -> Result<Response, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace_id}/invitations");
    let session = authenticate(&state, &headers, &instance, request_id.as_ref()).await?;
    let workspace_id = parse_id(&workspace_id, &instance, request_id.as_ref())?;
    if body.email.trim().is_empty() || !body.email.contains('@') {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_email",
            "Invalid email",
            "A valid invitation email address is required.",
            instance,
            request_id.as_ref(),
        ));
    }
    let delivery = match body.delivery {
        DeliveryBody::Manual => InvitationDelivery::Manual,
        DeliveryBody::Smtp => InvitationDelivery::Smtp,
    };
    let issued = state
        .workspaces
        .invite(
            workspace_id,
            session.user.id,
            body.email.trim().to_owned(),
            body.role.into(),
            delivery,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map_err(|error| workspace_problem(error, &instance, request_id.as_ref()))?;
    let url = (delivery == InvitationDelivery::Manual).then(|| {
        format!(
            "{}/accept-invitation?token={}",
            state.public_origin, issued.token
        )
    });
    Ok((
        StatusCode::CREATED,
        Json(InvitationResponse {
            invitation: issued.invitation,
            url,
        }),
    )
        .into_response())
}

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/invitations", responses((status = 200)))]
async fn list_invitations(
    State(state): State<WorkspaceState>,
    Path(workspace_id): Path<String>,
    ApiQuery(page): ApiQuery<PageQuery>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<Page<crate::repositories::workspaces::InvitationRecord>>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace_id}/invitations");
    let session = authenticate(&state, &headers, &instance, request_id.as_ref()).await?;
    let workspace_id = parse_id(&workspace_id, &instance, request_id.as_ref())?;
    let cursor = optional_id(page.cursor, &instance, request_id.as_ref())?;
    let (items, next_cursor) = state
        .workspaces
        .invitations(
            workspace_id,
            session.user.id,
            cursor,
            page.limit,
            TimestampMillis::now(),
        )
        .await
        .map_err(|error| workspace_problem(error, &instance, request_id.as_ref()))?;
    Ok(Json(Page { items, next_cursor }))
}

#[utoipa::path(delete, path = "/api/v1/workspaces/{workspace_id}/invitations/{invitation_id}", responses((status = 204)))]
async fn revoke_invitation(
    State(state): State<WorkspaceState>,
    Path((workspace_id, invitation_id)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<StatusCode, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace_id}/invitations/{invitation_id}");
    let session = authenticate(&state, &headers, &instance, request_id.as_ref()).await?;
    let workspace_id = parse_id(&workspace_id, &instance, request_id.as_ref())?;
    let invitation_id = parse_id(&invitation_id, &instance, request_id.as_ref())?;
    state
        .workspaces
        .revoke_invitation(
            workspace_id,
            invitation_id,
            session.user.id,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map_err(|error| workspace_problem(error, instance, request_id.as_ref()))?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct AcceptBody {
    token: String,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    password: Option<String>,
}

#[utoipa::path(post, path = "/api/v1/workspaces/invitations/accept", request_body = AcceptBody, responses((status = 200)))]
async fn accept_invitation(
    State(state): State<WorkspaceState>,
    headers: HeaderMap,
    client_ip: Option<Extension<ClientIp>>,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<AcceptBody>,
) -> Result<Response, ApiError> {
    let instance = "/api/v1/workspaces/invitations/accept";
    if cookie_value(&headers, state.cookie_mode.session_cookie_name()).is_some() {
        let session = authenticate(&state, &headers, instance, request_id.as_ref()).await?;
        let accepted = state
            .workspaces
            .accept_invitation(
                &body.token,
                session.user.id,
                &session.user.email,
                request_id_value(request_id.as_ref()),
                TimestampMillis::now(),
            )
            .await
            .map_err(|error| workspace_problem(error, instance, request_id.as_ref()))?;
        return Ok(Json(accepted).into_response());
    }
    let email = body
        .email
        .ok_or_else(|| registration_field_problem("email", instance, request_id.as_ref()))?;
    let display_name = body
        .display_name
        .ok_or_else(|| registration_field_problem("display_name", instance, request_id.as_ref()))?;
    let password = body
        .password
        .ok_or_else(|| registration_field_problem("password", instance, request_id.as_ref()))?;
    if email.trim().is_empty() || !email.contains('@') || display_name.trim().is_empty() {
        return Err(registration_field_problem(
            "registration",
            instance,
            request_id.as_ref(),
        ));
    }
    let ip = client_ip
        .map(|Extension(value)| value.0)
        .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
    let reservation = state
        .registration_throttler
        .lock()
        .expect("registration throttler mutex poisoned")
        .reserve(&email, ip, TimestampMillis::now())
        .map_err(|decision| match decision {
            ThrottleDecision::RetryAfter(delay) => {
                invitation_throttled_problem(delay, instance, request_id.as_ref())
            }
            ThrottleDecision::Allowed => unreachable!("allowed admission returns a reservation"),
        })?;
    if let Err(error) = state
        .workspaces
        .validate_invited_registration(
            &body.token,
            &email,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
    {
        state
            .registration_throttler
            .lock()
            .expect("registration throttler mutex poisoned")
            .finish_failure(reservation, TimestampMillis::now());
        return Err(workspace_problem(error, instance, request_id.as_ref()));
    }
    let password_hash = match state.passwords.hash(password).await {
        Ok(hash) => hash,
        Err(error) => {
            state
                .registration_throttler
                .lock()
                .expect("registration throttler mutex poisoned")
                .finish_failure(reservation, TimestampMillis::now());
            return Err(password_problem(error, instance, request_id.as_ref()));
        }
    };
    let registered = match state
        .workspaces
        .register_invited_account(
            &body.token,
            email.trim().to_owned(),
            display_name.trim().to_owned(),
            password_hash,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
    {
        Ok(registered) => registered,
        Err(error) => {
            state
                .registration_throttler
                .lock()
                .expect("registration throttler mutex poisoned")
                .finish_failure(reservation, TimestampMillis::now());
            return Err(workspace_problem(error, instance, request_id.as_ref()));
        }
    };
    state
        .registration_throttler
        .lock()
        .expect("registration throttler mutex poisoned")
        .finish_success(reservation);
    let cookie = issued_session_cookie(state.cookie_mode, &registered.session.token);
    let mut response = (StatusCode::CREATED, Json(registered.acceptance)).into_response();
    response.headers_mut().insert(
        axum::http::header::SET_COOKIE,
        HeaderValue::from_str(&cookie).map_err(|_| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "Internal server error",
                "An unexpected error occurred. Use the request ID when contacting support.",
                instance,
                request_id.as_ref(),
            )
        })?,
    );
    Ok(response)
}

#[utoipa::path(delete, path = "/api/v1/workspaces/{workspace_id}", responses((status = 204)))]
async fn delete_workspace(
    State(state): State<WorkspaceState>,
    Path(workspace_id): Path<String>,
    ApiQuery(mutation): ApiQuery<MutationQuery>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<StatusCode, ApiError> {
    set_deleted(
        state,
        workspace_id,
        headers,
        request_id,
        true,
        mutation.expected_version,
    )
    .await
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct RestoreBody {
    expected_version: u64,
}

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/restore", responses((status = 204)))]
async fn restore_workspace(
    State(state): State<WorkspaceState>,
    Path(workspace_id): Path<String>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<RestoreBody>,
) -> Result<StatusCode, ApiError> {
    set_deleted(
        state,
        workspace_id,
        headers,
        request_id,
        false,
        body.expected_version,
    )
    .await
}

async fn set_deleted(
    state: WorkspaceState,
    workspace_id: String,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    deleted: bool,
    expected_version: u64,
) -> Result<StatusCode, ApiError> {
    let instance = if deleted {
        format!("/api/v1/workspaces/{workspace_id}")
    } else {
        format!("/api/v1/workspaces/{workspace_id}/restore")
    };
    let session = authenticate(&state, &headers, &instance, request_id.as_ref()).await?;
    let workspace_id = parse_id(&workspace_id, &instance, request_id.as_ref())?;
    state
        .workspaces
        .set_deleted(
            workspace_id,
            session.user.id,
            deleted,
            expected_version,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map_err(|error| workspace_problem(error, instance, request_id.as_ref()))?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(get, path = "/api/v1/workspaces/trash", responses((status = 200)))]
async fn list_trash(
    State(state): State<WorkspaceState>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<Vec<crate::repositories::workspaces::WorkspaceRecord>>, ApiError> {
    let instance = "/api/v1/workspaces/trash";
    let session = authenticate(&state, &headers, instance, request_id.as_ref()).await?;
    state
        .workspaces
        .list_trash(session.user.id)
        .await
        .map(Json)
        .map_err(|error| workspace_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/audit", responses((status = 200)))]
async fn list_audit(
    State(state): State<WorkspaceState>,
    Path(workspace_id): Path<String>,
    ApiQuery(page): ApiQuery<PageQuery>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<Page<crate::audit::AuditEvent>>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace_id}/audit");
    let session = authenticate(&state, &headers, &instance, request_id.as_ref()).await?;
    let workspace_id = parse_id(&workspace_id, &instance, request_id.as_ref())?;
    let cursor = optional_id(page.cursor, &instance, request_id.as_ref())?;
    let (items, next_cursor) = state
        .workspaces
        .audit(workspace_id, session.user.id, cursor, page.limit)
        .await
        .map_err(|error| workspace_problem(error, &instance, request_id.as_ref()))?;
    Ok(Json(Page { items, next_cursor }))
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct SuspensionBody {
    suspended: bool,
}

#[utoipa::path(post, path = "/api/v1/admin/users/{user_id}/suspension", request_body = SuspensionBody, responses((status = 204)))]
async fn set_account_suspension(
    State(state): State<WorkspaceState>,
    Path(user_id): Path<String>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<SuspensionBody>,
) -> Result<StatusCode, ApiError> {
    let instance = format!("/api/v1/admin/users/{user_id}/suspension");
    let session = authenticate(&state, &headers, &instance, request_id.as_ref()).await?;
    let user_id = parse_id(&user_id, &instance, request_id.as_ref())?;
    state
        .identity
        .set_suspended(
            session.user.id,
            user_id,
            body.suspended,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map_err(|error| suspension_problem(error, &instance, request_id.as_ref()))?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AdminAuditQuery {
    workspace_id: Option<String>,
    action: Option<String>,
    cursor: Option<String>,
    #[serde(default = "default_limit")]
    limit: usize,
}

async fn list_global_audit(
    State(state): State<WorkspaceState>,
    ApiQuery(query): ApiQuery<AdminAuditQuery>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<Page<crate::audit::AuditEvent>>, ApiError> {
    let instance = "/api/v1/admin/audit";
    let session = authenticate(&state, &headers, instance, request_id.as_ref()).await?;
    require_installation_admin(&state, session.user.id, instance, request_id.as_ref()).await?;
    let workspace_id = optional_id(query.workspace_id, instance, request_id.as_ref())?;
    let cursor = optional_id(query.cursor, instance, request_id.as_ref())?;
    let (items, next_cursor) = state
        .workspaces
        .global_audit(workspace_id, query.action.as_deref(), cursor, query.limit)
        .await
        .map_err(|error| workspace_problem(error, instance, request_id.as_ref()))?;
    Ok(Json(Page { items, next_cursor }))
}

async fn export_global_audit(
    State(state): State<WorkspaceState>,
    ApiQuery(query): ApiQuery<AdminAuditQuery>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Response, ApiError> {
    let instance = "/api/v1/admin/audit/export";
    let session = authenticate(&state, &headers, instance, request_id.as_ref()).await?;
    require_installation_admin(&state, session.user.id, instance, request_id.as_ref()).await?;
    let workspace_id = optional_id(query.workspace_id, instance, request_id.as_ref())?;
    let cursor = optional_id(query.cursor, instance, request_id.as_ref())?;
    let (first_page, next_cursor) = state
        .workspaces
        .global_audit(workspace_id, query.action.as_deref(), cursor, query.limit)
        .await
        .map_err(|error| workspace_problem(error, instance, request_id.as_ref()))?;
    let repository = Arc::clone(&state.workspaces);
    let action = query.action;
    let page_size = query.limit;
    let (sender, receiver) = tokio::sync::mpsc::channel::<Result<String, std::io::Error>>(1);
    tokio::spawn(async move {
        if sender
            .send(Ok(audit_csv_page(first_page, true)))
            .await
            .is_err()
        {
            return;
        }
        let mut cursor = next_cursor;
        while let Some(current) = cursor {
            let result = repository
                .global_audit(workspace_id, action.as_deref(), Some(current), page_size)
                .await;
            let (page, next) = match result {
                Ok(page) => page,
                Err(_) => {
                    let _ = sender
                        .send(Err(std::io::Error::other("audit export failed")))
                        .await;
                    return;
                }
            };
            if sender.send(Ok(audit_csv_page(page, false))).await.is_err() {
                return;
            }
            cursor = next;
        }
    });
    let mut response = Body::from_stream(ReceiverStream::new(receiver)).into_response();
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/csv; charset=utf-8"),
    );
    Ok(response)
}

fn audit_csv_page(events: Vec<crate::audit::AuditEvent>, include_header: bool) -> String {
    let mut csv = if include_header {
        "id,workspace_id,actor_id,action,outcome,resource_type,resource_id,request_id,occurred_at\n"
            .to_owned()
    } else {
        String::new()
    };
    for event in events {
        let fields = [
            event.id.to_string(),
            event
                .workspace_id
                .map(|id| id.to_string())
                .unwrap_or_default(),
            event.actor_id.map(|id| id.to_string()).unwrap_or_default(),
            event.action,
            event.outcome,
            event.resource_type,
            event
                .resource_id
                .map(|id| id.to_string())
                .unwrap_or_default(),
            event.request_id,
            event.occurred_at.to_string(),
        ];
        csv.push_str(
            &fields
                .into_iter()
                .map(|field| csv_field(&field))
                .collect::<Vec<_>>()
                .join(","),
        );
        csv.push('\n');
    }
    csv
}

async fn require_installation_admin(
    state: &WorkspaceState,
    user_id: Id,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<(), ApiError> {
    if state
        .identity
        .is_installation_admin(user_id)
        .await
        .map_err(|_| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "Internal server error",
                "An unexpected error occurred. Use the request ID when contacting support.",
                instance,
                request_id,
            )
        })?
    {
        Ok(())
    } else {
        Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "installation_admin_required",
            "Installation administrator required",
            "Only an installation administrator may access global audit records.",
            instance,
            request_id,
        ))
    }
}

fn csv_field(value: &str) -> String {
    let value = if matches!(value.chars().next(), Some('=' | '+' | '-' | '@')) {
        format!("'{value}")
    } else {
        value.to_owned()
    };
    format!("\"{}\"", value.replace('"', "\"\""))
}

async fn authenticate(
    state: &WorkspaceState,
    headers: &HeaderMap,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<AuthenticatedSession, ApiError> {
    let token =
        cookie_value(headers, state.cookie_mode.session_cookie_name()).ok_or_else(|| {
            ApiError::new(
                StatusCode::UNAUTHORIZED,
                "authentication_required",
                "Authentication required",
                "A valid session is required.",
                instance,
                request_id,
            )
        })?;
    state
        .identity
        .authenticate_session(&token, TimestampMillis::now())
        .await
        .map_err(|_| {
            ApiError::new(
                StatusCode::UNAUTHORIZED,
                "authentication_required",
                "Authentication required",
                "A valid session is required.",
                instance,
                request_id,
            )
        })
}

fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .map(str::trim)
        .find_map(|pair| pair.strip_prefix(&format!("{name}=")).map(str::to_owned))
}

fn parse_id(
    value: &str,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<Id, ApiError> {
    value.parse().map_err(|_| {
        ApiError::new(
            StatusCode::NOT_FOUND,
            "workspace_resource_not_found",
            "Workspace resource not found",
            "The requested workspace resource was not found.",
            instance,
            request_id,
        )
    })
}

fn optional_id(
    value: Option<String>,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<Option<Id>, ApiError> {
    value
        .map(|value| {
            value.parse().map_err(|_| {
                ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    "Invalid request",
                    "The request query is not valid for this endpoint.",
                    instance,
                    request_id,
                )
            })
        })
        .transpose()
}

fn nonempty_name(
    name: String,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<String, ApiError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 200 {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_workspace_name",
            "Invalid workspace name",
            "Workspace names must contain between 1 and 200 characters.",
            instance,
            request_id,
        ));
    }
    Ok(name.to_owned())
}

fn workspace_problem(
    error: WorkspaceError,
    instance: impl Into<String>,
    request_id: Option<&Extension<RequestId>>,
) -> ApiError {
    let instance = instance.into();
    match error {
        WorkspaceError::NotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            "workspace_resource_not_found",
            "Workspace resource not found",
            "The requested workspace resource was not found.",
            instance,
            request_id,
        ),
        WorkspaceError::Forbidden => ApiError::new(
            StatusCode::FORBIDDEN,
            "workspace_action_forbidden",
            "Workspace action forbidden",
            "Your workspace role does not permit this action.",
            instance,
            request_id,
        ),
        WorkspaceError::TransferRequired => ApiError::new(
            StatusCode::CONFLICT,
            "ownership_transfer_required",
            "Ownership transfer required",
            "Transfer workspace ownership before altering or removing the owner.",
            instance,
            request_id,
        ),
        WorkspaceError::InvalidInvitation => ApiError::new(
            StatusCode::NOT_FOUND,
            "invitation_not_found",
            "Invitation not found",
            "The invitation is invalid, expired, or no longer pending.",
            instance,
            request_id,
        ),
        WorkspaceError::EmailMismatch => ApiError::new(
            StatusCode::FORBIDDEN,
            "invitation_email_mismatch",
            "Invitation email mismatch",
            "Sign in with the account matching the invitation email.",
            instance,
            request_id,
        ),
        WorkspaceError::RegistrationRequiresSignIn => ApiError::new(
            StatusCode::UNAUTHORIZED,
            "authentication_required",
            "Authentication required",
            "The invited account already exists. Sign in to accept this invitation.",
            instance,
            request_id,
        ),
        WorkspaceError::Conflict => ApiError::new(
            StatusCode::CONFLICT,
            "workspace_conflict",
            "Workspace conflict",
            "The workspace operation conflicts with its current state.",
            instance,
            request_id,
        ),
        WorkspaceError::VersionConflict { current_version } => {
            ApiError::conflict(current_version, instance.clone(), instance, request_id)
        }
        WorkspaceError::Unavailable(_) | WorkspaceError::Storage(_) => ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            "Internal server error",
            "An unexpected error occurred. Use the request ID when contacting support.",
            instance,
            request_id,
        ),
    }
}

fn registration_field_problem(
    _field: &str,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> ApiError {
    ApiError::new(
        StatusCode::UNPROCESSABLE_ENTITY,
        "invalid_registration",
        "Invalid registration",
        "Email, display name, and a valid password are required.",
        instance,
        request_id,
    )
}

fn invitation_throttled_problem(
    retry_after: std::time::Duration,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> ApiError {
    let mut error = ApiError::new(
        StatusCode::TOO_MANY_REQUESTS,
        "invitation_registration_throttled",
        "Too many attempts",
        "Too many invitation registration attempts were made. Try again later.",
        instance,
        request_id,
    );
    error.retry_after = Some(retry_after);
    error
}

fn password_problem(
    error: PasswordError,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> ApiError {
    let detail = match error {
        PasswordError::InvalidLength => "Password must contain between 12 and 128 characters.",
        PasswordError::CommonPassword => "Choose a password that is not commonly used.",
        PasswordError::InvalidHash | PasswordError::HashingFailed => {
            return ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "Internal server error",
                "An unexpected error occurred. Use the request ID when contacting support.",
                instance,
                request_id,
            );
        }
    };
    ApiError::new(
        StatusCode::UNPROCESSABLE_ENTITY,
        "invalid_password",
        "Invalid password",
        detail,
        instance,
        request_id,
    )
}

fn suspension_problem(
    error: SuspensionError,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> ApiError {
    match error {
        SuspensionError::Forbidden => ApiError::new(
            StatusCode::FORBIDDEN,
            "installation_admin_required",
            "Installation administrator required",
            "Only an installation administrator may suspend global accounts.",
            instance,
            request_id,
        ),
        SuspensionError::NotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            "user_not_found",
            "User not found",
            "The requested global user was not found.",
            instance,
            request_id,
        ),
        SuspensionError::Unavailable(_) => ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            "Internal server error",
            "An unexpected error occurred. Use the request ID when contacting support.",
            instance,
            request_id,
        ),
    }
}

fn request_id_value(request_id: Option<&Extension<RequestId>>) -> &str {
    request_id.map_or("unknown", |Extension(value)| value.as_str())
}

const fn default_limit() -> usize {
    50
}

#[derive(Debug, Serialize)]
struct ProblemBody {
    #[serde(rename = "type")]
    type_uri: String,
    title: &'static str,
    status: u16,
    code: &'static str,
    detail: &'static str,
    instance: String,
    request_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    conflict: Option<ConflictBody>,
}

#[derive(Debug, Serialize)]
struct ConflictBody {
    current_version: u64,
    refresh: String,
}

struct ApiError {
    status: StatusCode,
    body: Box<ProblemBody>,
    retry_after: Option<std::time::Duration>,
}

impl ApiError {
    fn new(
        status: StatusCode,
        code: &'static str,
        title: &'static str,
        detail: &'static str,
        instance: impl Into<String>,
        request_id: Option<&Extension<RequestId>>,
    ) -> Self {
        Self {
            status,
            body: Box::new(ProblemBody {
                type_uri: format!("https://docs.orbit.dev/problems/{code}"),
                title,
                status: status.as_u16(),
                code,
                detail,
                instance: instance.into(),
                request_id: request_id
                    .map(|Extension(value)| value.as_str().to_owned())
                    .unwrap_or_else(|| "unknown".to_owned()),
                conflict: None,
            }),
            retry_after: None,
        }
    }

    fn conflict(
        current_version: u64,
        refresh: String,
        instance: String,
        request_id: Option<&Extension<RequestId>>,
    ) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            body: Box::new(ProblemBody {
                type_uri: "https://docs.orbit.dev/problems/conflict".to_owned(),
                title: "Conflict",
                status: StatusCode::CONFLICT.as_u16(),
                code: "conflict",
                detail: "The resource changed after it was read. Refresh and retry.",
                instance,
                request_id: request_id
                    .map(|Extension(value)| value.as_str().to_owned())
                    .unwrap_or_else(|| "unknown".to_owned()),
                conflict: Some(ConflictBody {
                    current_version,
                    refresh,
                }),
            }),
            retry_after: None,
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut response = (self.status, Json(*self.body)).into_response();
        response.headers_mut().insert(
            CONTENT_TYPE,
            HeaderValue::from_static("application/problem+json"),
        );
        if let Some(retry_after) = self.retry_after {
            response.headers_mut().insert(
                RETRY_AFTER,
                HeaderValue::from_str(&retry_after.as_secs().max(1).to_string()).unwrap(),
            );
        }
        response
    }
}
