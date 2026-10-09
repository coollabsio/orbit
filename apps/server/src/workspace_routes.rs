use std::net::{IpAddr, Ipv4Addr};
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::{Extension, FromRequest, FromRequestParts, Path, Query, Request, State};
use axum::http::header::{
    CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_TYPE, RETRY_AFTER,
};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post, put};
use axum::{Json, Router};
use orbit_domain::WorkspaceRole;
use orbit_platform::{
    BackupError, BackupKind, BackupService, BackupSnapshot, ClientIp, Id, LoginThrottler,
    ObjectStorage, PasswordError, PasswordExecutor, PasswordService, RequestId, ThrottleDecision,
    TieredBlobStore, TimestampMillis,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio_stream::wrappers::ReceiverStream;
use utoipa::{IntoParams, ToSchema};

use crate::auth_routes::{CookieMode, issued_session_cookie, request_session};
use crate::image_compression::{CompressionStatus, ImageCompressor};
use crate::mail::{self, Mailer};
use crate::repositories::api_tokens::{
    ApiTokenError, ApiTokenRecord, ApiTokenRepository, IssuedApiToken,
};
use crate::repositories::identity::{
    AdminAccountError, AdminUser, AuthenticatedSession, IdentityRepository,
};
use crate::repositories::instance_settings::{
    BackupSchedule, InstanceSettingsError, InstanceSettingsRepository, PasswordChange, S3Settings,
    SmtpSecurity, SmtpSettings, StorageSettingsError,
};
use crate::repositories::workspaces::{InvitationDelivery, WorkspaceError, WorkspaceRepository};

#[derive(Clone)]
pub struct WorkspaceState {
    identity: Arc<IdentityRepository>,
    workspaces: Arc<WorkspaceRepository>,
    api_tokens: Arc<ApiTokenRepository>,
    public_origin: String,
    cookie_mode: CookieMode,
    passwords: PasswordExecutor,
    registration_throttler: Arc<Mutex<LoginThrottler>>,
    backups: Option<BackupService>,
    /// Sends invitation emails and keeps the app key for the SMTP password; `None` sends nothing.
    mailer: Option<Mailer>,
    /// The running attachment and backup storage, changed by Admin → Storage.
    object_storage: Option<(ObjectStorage, TieredBlobStore)>,
    /// Makes large stored images smaller (Admin → Storage).
    image_compressor: Option<ImageCompressor>,
}

impl WorkspaceState {
    #[must_use]
    pub fn new(
        identity: Arc<IdentityRepository>,
        public_origin: String,
        cookie_mode: CookieMode,
    ) -> Self {
        Self {
            api_tokens: Arc::new(ApiTokenRepository::new(identity.database().clone())),
            workspaces: Arc::new(WorkspaceRepository::new(identity.database().clone())),
            identity,
            public_origin: public_origin.trim_end_matches('/').to_owned(),
            cookie_mode,
            passwords: PasswordExecutor::new(PasswordService::default(), 2)
                .expect("password executor concurrency is non-zero"),
            registration_throttler: Arc::new(Mutex::new(LoginThrottler::new())),
            backups: None,
            mailer: None,
            object_storage: None,
            image_compressor: None,
        }
    }

    #[must_use]
    pub fn with_repository(
        identity: Arc<IdentityRepository>,
        workspaces: Arc<WorkspaceRepository>,
        public_origin: String,
        cookie_mode: CookieMode,
        backups: BackupService,
    ) -> Self {
        Self {
            api_tokens: Arc::new(ApiTokenRepository::new(identity.database().clone())),
            identity,
            workspaces,
            public_origin: public_origin.trim_end_matches('/').to_owned(),
            cookie_mode,
            passwords: PasswordExecutor::new(PasswordService::default(), 2)
                .expect("password executor concurrency is non-zero"),
            registration_throttler: Arc::new(Mutex::new(LoginThrottler::new())),
            backups: Some(backups),
            mailer: None,
            object_storage: None,
            image_compressor: None,
        }
    }

    #[must_use]
    pub fn with_mailer(mut self, mailer: Mailer) -> Self {
        self.mailer = Some(mailer);
        self
    }

    #[must_use]
    pub fn with_object_storage(mut self, storage: ObjectStorage, store: TieredBlobStore) -> Self {
        self.object_storage = Some((storage, store));
        self
    }

    #[must_use]
    pub fn with_image_compressor(mut self, compressor: ImageCompressor) -> Self {
        self.image_compressor = Some(compressor);
        self
    }

    /// The mailer when a mail server is saved.
    async fn enabled_mailer(&self) -> Option<&Mailer> {
        match &self.mailer {
            Some(mailer) if mailer.enabled().await.unwrap_or(false) => Some(mailer),
            _ => None,
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
            "/api/v1/workspaces/invitations/preview",
            post(preview_invitation),
        )
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
            "/api/v1/workspaces/{workspace_id}/profiles/{user_id}",
            get(get_member_profile),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/profiles/{user_id}/note",
            put(put_member_note),
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
            "/api/v1/workspaces/{workspace_id}/api-tokens",
            get(list_api_tokens).post(create_api_token),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/api-tokens/{token_id}",
            delete(revoke_api_token),
        )
        .route("/api/v1/admin/settings", get(get_instance_settings))
        .route("/api/v1/admin/settings/registration", put(set_registration))
        .route(
            "/api/v1/admin/settings/smtp",
            put(save_smtp).delete(remove_smtp),
        )
        .route("/api/v1/admin/settings/smtp/test", post(send_test_email))
        .route("/api/v1/admin/settings/s3", put(save_s3).delete(remove_s3))
        .route("/api/v1/admin/settings/storage", put(save_storage_options))
        .route(
            "/api/v1/admin/attachments/compress",
            post(start_image_compression),
        )
        .route("/api/v1/admin/users", get(list_admin_users))
        .route(
            "/api/v1/admin/users/{user_id}/suspension",
            post(set_account_suspension),
        )
        .route(
            "/api/v1/admin/users/{user_id}/recovery-link",
            post(create_recovery_link),
        )
        .route(
            "/api/v1/admin/users/{user_id}/two-factor/reset",
            post(reset_two_factor),
        )
        .route(
            "/api/v1/admin/users/{user_id}/admin",
            put(set_instance_admin),
        )
        .route("/api/v1/admin/audit", get(list_global_audit))
        .route("/api/v1/admin/audit/export", get(export_global_audit))
        .route(
            "/api/v1/admin/backups",
            get(list_backups).post(create_backup),
        )
        .route(
            "/api/v1/admin/backups/{backup_id}/download",
            get(download_backup),
        )
        .route("/api/v1/admin/backups/{backup_id}", delete(delete_backup))
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

#[utoipa::path(post, path = "/api/v1/workspaces", request_body = CreateWorkspaceBody, responses((status = 201, body = crate::repositories::workspaces::WorkspaceRecord)))]
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

#[utoipa::path(get, path = "/api/v1/workspaces", responses((status = 200, body = Vec<crate::repositories::workspaces::WorkspaceRecord>)))]
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

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}", params(("workspace_id" = String, Path)), responses((status = 200, body = crate::repositories::workspaces::WorkspaceRecord)))]
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

#[utoipa::path(patch, path = "/api/v1/workspaces/{workspace_id}", params(("workspace_id" = String, Path)), request_body = RenameWorkspaceBody, responses((status = 200, body = crate::repositories::workspaces::WorkspaceRecord)))]
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

#[derive(Deserialize, IntoParams, ToSchema)]
#[into_params(parameter_in = Query)]
#[serde(deny_unknown_fields)]
struct PageQuery {
    cursor: Option<String>,
    #[serde(default = "default_limit")]
    #[param(required = false)]
    limit: usize,
}

#[derive(Deserialize, IntoParams, ToSchema)]
#[into_params(parameter_in = Query)]
#[serde(deny_unknown_fields)]
struct MutationQuery {
    expected_version: u64,
}

#[derive(Serialize, ToSchema)]
struct Page<T> {
    items: Vec<T>,
    #[schema(value_type = Option<String>)]
    next_cursor: Option<Id>,
}

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/members", params(PageQuery, ("workspace_id" = String, Path)), responses((status = 200, body = Page<crate::repositories::workspaces::MemberRecord>)))]
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

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/profiles/{user_id}", params(("workspace_id" = String, Path), ("user_id" = String, Path)), responses((status = 200, body = crate::repositories::workspaces::MemberProfile)))]
async fn get_member_profile(
    State(state): State<WorkspaceState>,
    Path((workspace_id, user_id)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<crate::repositories::workspaces::MemberProfile>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace_id}/profiles/{user_id}");
    let session = authenticate(&state, &headers, &instance, request_id.as_ref()).await?;
    let workspace_id = parse_id(&workspace_id, &instance, request_id.as_ref())?;
    let user_id = parse_id(&user_id, &instance, request_id.as_ref())?;
    state
        .workspaces
        .profile(workspace_id, session.user.id, user_id)
        .await
        .map(Json)
        .map_err(|error| workspace_problem(error, &instance, request_id.as_ref()))
}

const MAX_NOTE_CHARS: usize = 1000;

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct MemberNoteBody {
    /// The note; an empty text removes it.
    body: String,
}

#[utoipa::path(put, path = "/api/v1/workspaces/{workspace_id}/profiles/{user_id}/note", params(("workspace_id" = String, Path), ("user_id" = String, Path)), request_body = MemberNoteBody, responses((status = 204)))]
async fn put_member_note(
    State(state): State<WorkspaceState>,
    Path((workspace_id, user_id)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<MemberNoteBody>,
) -> Result<StatusCode, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace_id}/profiles/{user_id}/note");
    let session = authenticate(&state, &headers, &instance, request_id.as_ref()).await?;
    let workspace_id = parse_id(&workspace_id, &instance, request_id.as_ref())?;
    let user_id = parse_id(&user_id, &instance, request_id.as_ref())?;
    let note = body.body.trim();
    if note.chars().count() > MAX_NOTE_CHARS {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_note",
            "Invalid note",
            "A note may have 1000 characters.",
            instance,
            request_id.as_ref(),
        ));
    }
    state
        .workspaces
        .set_note(
            workspace_id,
            session.user.id,
            user_id,
            note,
            TimestampMillis::now(),
        )
        .await
        .map_err(|error| workspace_problem(error, &instance, request_id.as_ref()))?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Clone, Copy, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
enum ApiTokenScope {
    Read,
    Write,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct CreateApiTokenBody {
    name: String,
    #[serde(default)]
    service_account: bool,
    expires_in_days: Option<u16>,
    #[serde(default)]
    project_ids: Vec<String>,
    project_id: Option<String>,
    scopes: Vec<ApiTokenScope>,
}

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/api-tokens", params(("workspace_id" = String, Path)), responses((status = 200, body = Vec<ApiTokenRecord>)))]
async fn list_api_tokens(
    State(state): State<WorkspaceState>,
    Path(workspace_id): Path<String>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<Vec<ApiTokenRecord>>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace_id}/api-tokens");
    let session = authenticate(&state, &headers, &instance, request_id.as_ref()).await?;
    let workspace_id = parse_id(&workspace_id, &instance, request_id.as_ref())?;
    state
        .api_tokens
        .list(workspace_id, session.user.id)
        .await
        .map(Json)
        .map_err(|error| api_token_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/api-tokens", params(("workspace_id" = String, Path)), request_body = CreateApiTokenBody, responses((status = 201, body = IssuedApiToken)))]
async fn create_api_token(
    State(state): State<WorkspaceState>,
    Path(workspace_id): Path<String>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<CreateApiTokenBody>,
) -> Result<Response, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace_id}/api-tokens");
    let session = authenticate(&state, &headers, &instance, request_id.as_ref()).await?;
    let workspace_id = parse_id(&workspace_id, &instance, request_id.as_ref())?;
    let name = body.name.trim();
    let mut requested_project_ids = body.project_ids;
    if let Some(project_id) = body.project_id {
        requested_project_ids.push(project_id);
    }
    let mut project_ids = Vec::new();
    for id in requested_project_ids {
        let id = parse_id(&id, &instance, request_id.as_ref())?;
        if !project_ids.contains(&id) {
            project_ids.push(id);
        }
    }
    if name.is_empty()
        || name.chars().count() > 100
        || project_ids.is_empty()
        || body.scopes.is_empty()
    {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_api_token",
            "Invalid API token",
            "Token names must contain 1 to 100 characters, and at least one project and scope are required.",
            instance,
            request_id.as_ref(),
        ));
    }
    let can_read = body
        .scopes
        .iter()
        .any(|scope| matches!(scope, ApiTokenScope::Read));
    let can_write = body
        .scopes
        .iter()
        .any(|scope| matches!(scope, ApiTokenScope::Write));
    let now = TimestampMillis::now();
    let expires_at = match body.expires_in_days {
        None => None,
        Some(days @ (7 | 30 | 90 | 365)) => Some(TimestampMillis::from_millis(
            now.as_millis() + i64::from(days) * 24 * 60 * 60 * 1_000,
        )),
        Some(_) => {
            return Err(ApiError::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "invalid_api_token",
                "Invalid API token",
                "Token expiration must be 7, 30, 90, or 365 days, or omitted for no expiration.",
                instance,
                request_id.as_ref(),
            ));
        }
    };
    let issued = state
        .api_tokens
        .create(
            workspace_id,
            session.user.id,
            name.to_owned(),
            project_ids,
            can_read,
            can_write,
            body.service_account,
            expires_at,
            request_id_value(request_id.as_ref()),
            now,
        )
        .await
        .map_err(|error| api_token_problem(error, &instance, request_id.as_ref()))?;
    let mut response = (StatusCode::CREATED, Json(issued)).into_response();
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}

#[utoipa::path(delete, path = "/api/v1/workspaces/{workspace_id}/api-tokens/{token_id}", params(("workspace_id" = String, Path), ("token_id" = String, Path)), responses((status = 204)))]
async fn revoke_api_token(
    State(state): State<WorkspaceState>,
    Path((workspace_id, token_id)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<StatusCode, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace_id}/api-tokens/{token_id}");
    let session = authenticate(&state, &headers, &instance, request_id.as_ref()).await?;
    let workspace_id = parse_id(&workspace_id, &instance, request_id.as_ref())?;
    let token_id = parse_id(&token_id, &instance, request_id.as_ref())?;
    state
        .api_tokens
        .revoke(
            workspace_id,
            token_id,
            session.user.id,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map_err(|error| api_token_problem(error, instance, request_id.as_ref()))?;
    Ok(StatusCode::NO_CONTENT)
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

#[utoipa::path(patch, path = "/api/v1/workspaces/{workspace_id}/members/{membership_id}", params(("workspace_id" = String, Path), ("membership_id" = String, Path)), request_body = RoleChangeBody, responses((status = 204)))]
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

#[utoipa::path(delete, path = "/api/v1/workspaces/{workspace_id}/members/{membership_id}", params(MutationQuery, ("workspace_id" = String, Path), ("membership_id" = String, Path)), responses((status = 204)))]
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

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/transfer-ownership", params(("workspace_id" = String, Path)), request_body = TransferBody, responses((status = 204)))]
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

#[derive(Serialize, ToSchema)]
struct InvitationResponse {
    invitation: crate::repositories::workspaces::InvitationRecord,
    #[serde(skip_serializing_if = "Option::is_none")]
    url: Option<String>,
}

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/invitations", params(("workspace_id" = String, Path)), request_body = InvitationBody, responses((status = 201, body = InvitationResponse)))]
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
    let mailer = match delivery {
        InvitationDelivery::Manual => None,
        InvitationDelivery::Smtp => Some(
            state
                .enabled_mailer()
                .await
                .ok_or_else(|| email_not_configured(&instance, request_id.as_ref()))?,
        ),
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
    let link = format!(
        "{}/accept-invitation?token={}",
        state.public_origin, issued.token
    );
    if let Some(mailer) = mailer {
        let now = TimestampMillis::now();
        let workspace_name = state
            .workspaces
            .preview_invitation(&issued.token, now)
            .await
            .map(|preview| preview.workspace_name)
            .unwrap_or_else(|_| "Orbit".to_owned());
        let sent = mailer
            .send(mail::templates::invitation(
                &issued.invitation.email,
                &session.user.display_name,
                &workspace_name,
                &link,
            ))
            .await;
        if let Err(error) = sent {
            // Nobody can use an invitation whose link never arrived: take it back, so the admin can retry.
            let _ = state
                .workspaces
                .revoke_invitation(
                    workspace_id,
                    issued.invitation.id,
                    session.user.id,
                    request_id_value(request_id.as_ref()),
                    now,
                )
                .await;
            return Err(email_failed(&error, &instance, request_id.as_ref()));
        }
    }
    let url = (delivery == InvitationDelivery::Manual).then_some(link);
    Ok((
        StatusCode::CREATED,
        Json(InvitationResponse {
            invitation: issued.invitation,
            url,
        }),
    )
        .into_response())
}

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/invitations", params(PageQuery, ("workspace_id" = String, Path)), responses((status = 200, body = Page<crate::repositories::workspaces::InvitationRecord>)))]
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

#[utoipa::path(delete, path = "/api/v1/workspaces/{workspace_id}/invitations/{invitation_id}", params(("workspace_id" = String, Path), ("invitation_id" = String, Path)), responses((status = 204)))]
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
struct PreviewInvitationBody {
    token: String,
}

#[utoipa::path(post, path = "/api/v1/workspaces/invitations/preview", request_body = PreviewInvitationBody, responses((status = 200, body = crate::repositories::workspaces::InvitationPreview)))]
async fn preview_invitation(
    State(state): State<WorkspaceState>,
    request_id: Option<Extension<RequestId>>,
    body: Result<ApiJson<PreviewInvitationBody>, ApiError>,
) -> Response {
    let result = match body {
        Ok(ApiJson(body)) => state
            .workspaces
            .preview_invitation(&body.token, TimestampMillis::now())
            .await
            .map(Json)
            .map_err(|error| {
                workspace_problem(
                    error,
                    "/api/v1/workspaces/invitations/preview",
                    request_id.as_ref(),
                )
            }),
        Err(error) => Err(error),
    };
    let mut response = result.into_response();
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
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

#[utoipa::path(post, path = "/api/v1/workspaces/invitations/accept", request_body = AcceptBody, responses((status = 200, body = crate::repositories::workspaces::AcceptanceRecord), (status = 201, body = crate::repositories::workspaces::AcceptanceRecord)))]
async fn accept_invitation(
    State(state): State<WorkspaceState>,
    headers: HeaderMap,
    client_ip: Option<Extension<ClientIp>>,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<AcceptBody>,
) -> Result<Response, ApiError> {
    let instance = "/api/v1/workspaces/invitations/accept";
    if state.cookie_mode.session_token(&headers).is_some() {
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
    let cookie = issued_session_cookie(
        state.cookie_mode,
        &registered.session.token,
        registered.session.absolute_expires_at,
    );
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

#[utoipa::path(delete, path = "/api/v1/workspaces/{workspace_id}", params(MutationQuery, ("workspace_id" = String, Path)), responses((status = 204)))]
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

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/restore", params(("workspace_id" = String, Path)), request_body = RestoreBody, responses((status = 204)))]
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

#[utoipa::path(get, path = "/api/v1/workspaces/trash", responses((status = 200, body = Vec<crate::repositories::workspaces::WorkspaceRecord>)))]
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

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/audit", params(PageQuery, ("workspace_id" = String, Path)), responses((status = 200, body = Page<crate::audit::AuditEvent>)))]
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

#[utoipa::path(post, path = "/api/v1/admin/users/{user_id}/suspension", params(("user_id" = String, Path)), request_body = SuspensionBody, responses((status = 204)))]
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
        .map_err(|error| admin_account_problem(error, &instance, request_id.as_ref()))?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, IntoParams, ToSchema)]
#[into_params(parameter_in = Query)]
#[serde(deny_unknown_fields)]
struct AdminUserQuery {
    /// Part of the display name or email, ignoring case.
    q: Option<String>,
    cursor: Option<String>,
    #[serde(default = "default_limit")]
    #[param(required = false)]
    limit: usize,
}

/// Accounts on this instance in cursor pages, for the Admin area.
#[utoipa::path(get, path = "/api/v1/admin/users", params(AdminUserQuery), responses((status = 200, body = Page<AdminUser>)))]
async fn list_admin_users(
    State(state): State<WorkspaceState>,
    ApiQuery(query): ApiQuery<AdminUserQuery>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<Page<AdminUser>>, ApiError> {
    let instance = "/api/v1/admin/users";
    let session = authenticate(&state, &headers, instance, request_id.as_ref()).await?;
    require_installation_admin(&state, session.user.id, instance, request_id.as_ref()).await?;
    let cursor = optional_id(query.cursor, instance, request_id.as_ref())?;
    let (items, next_cursor) = state
        .identity
        .list_users_for_admin(query.q.as_deref(), cursor, query.limit)
        .await
        .map_err(|_| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "Internal server error",
                "An unexpected error occurred. Use the request ID when contacting support.",
                instance,
                request_id.as_ref(),
            )
        })?;
    Ok(Json(Page { items, next_cursor }))
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct InstanceAdminBody {
    admin: bool,
}

/// Makes an account an instance admin, or takes that back. Root only.
#[utoipa::path(put, path = "/api/v1/admin/users/{user_id}/admin", params(("user_id" = String, Path)), request_body = InstanceAdminBody, responses((status = 204)))]
async fn set_instance_admin(
    State(state): State<WorkspaceState>,
    Path(user_id): Path<String>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<InstanceAdminBody>,
) -> Result<StatusCode, ApiError> {
    let instance = format!("/api/v1/admin/users/{user_id}/admin");
    let session = authenticate(&state, &headers, &instance, request_id.as_ref()).await?;
    let user_id = parse_id(&user_id, &instance, request_id.as_ref())?;
    state
        .identity
        .set_instance_admin(
            session.user.id,
            user_id,
            body.admin,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map_err(|error| admin_account_problem(error, &instance, request_id.as_ref()))?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize, ToSchema)]
struct RecoveryLink {
    url: String,
    #[schema(value_type = String, format = DateTime)]
    expires_at: TimestampMillis,
}

/// A one-time password recovery link for another account. It lasts 30 minutes, the same as `orbit recovery-link`.
#[utoipa::path(post, path = "/api/v1/admin/users/{user_id}/recovery-link", params(("user_id" = String, Path)), responses((status = 201, body = RecoveryLink)))]
async fn create_recovery_link(
    State(state): State<WorkspaceState>,
    Path(user_id): Path<String>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<(StatusCode, Json<RecoveryLink>), ApiError> {
    const LIFETIME_MILLIS: i64 = 30 * 60 * 1_000;
    let instance = format!("/api/v1/admin/users/{user_id}/recovery-link");
    let session = authenticate(&state, &headers, &instance, request_id.as_ref()).await?;
    let user_id = parse_id(&user_id, &instance, request_id.as_ref())?;
    let token = orbit_platform::generate_opaque_token();
    let expires_at =
        TimestampMillis::from_millis(TimestampMillis::now().as_millis() + LIFETIME_MILLIS);
    state
        .identity
        .issue_recovery_token_as_admin(
            session.user.id,
            user_id,
            &token,
            expires_at,
            request_id_value(request_id.as_ref()),
        )
        .await
        .map_err(|error| admin_account_problem(error, &instance, request_id.as_ref()))?;
    Ok((
        StatusCode::CREATED,
        Json(RecoveryLink {
            url: format!("{}/recovery?token={token}", state.public_origin),
            expires_at,
        }),
    ))
}

/// Turns another account's two-factor sign-in off, for a user who lost the authenticator app and the recovery codes.
#[utoipa::path(post, path = "/api/v1/admin/users/{user_id}/two-factor/reset", params(("user_id" = String, Path)), responses((status = 204)))]
async fn reset_two_factor(
    State(state): State<WorkspaceState>,
    Path(user_id): Path<String>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<StatusCode, ApiError> {
    let instance = format!("/api/v1/admin/users/{user_id}/two-factor/reset");
    let session = authenticate(&state, &headers, &instance, request_id.as_ref()).await?;
    let user_id = parse_id(&user_id, &instance, request_id.as_ref())?;
    state
        .identity
        .reset_two_factor_as_admin(
            session.user.id,
            user_id,
            request_id_value(request_id.as_ref()),
        )
        .await
        .map_err(|error| admin_account_problem(error, &instance, request_id.as_ref()))?;
    Ok(StatusCode::NO_CONTENT)
}

fn email_not_configured(instance: &str, request_id: Option<&Extension<RequestId>>) -> ApiError {
    ApiError::new(
        StatusCode::CONFLICT,
        "email_not_configured",
        "Email not configured",
        "Save a mail server in Admin → Settings first.",
        instance,
        request_id,
    )
}

fn email_failed(
    error: &mail::MailError,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> ApiError {
    ApiError::new(
        StatusCode::BAD_GATEWAY,
        "email_failed",
        "Email failed",
        format!("The email could not be sent: {error}"),
        instance,
        request_id,
    )
}

fn internal(instance: &str, request_id: Option<&Extension<RequestId>>) -> ApiError {
    ApiError::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        "internal_error",
        "Internal server error",
        "An unexpected error occurred. Use the request ID when contacting support.",
        instance,
        request_id,
    )
}

#[derive(Serialize, ToSchema)]
struct SmtpView {
    host: String,
    port: u16,
    security: SmtpSecurity,
    username: Option<String>,
    /// A password is saved. The password itself is never returned.
    password_set: bool,
    from_address: String,
    from_name: Option<String>,
}

/// The saved bucket. The secret key is never returned.
#[derive(Serialize, ToSchema)]
struct S3View {
    endpoint: String,
    region: String,
    bucket: String,
    /// Key prefix in the bucket; empty for the bucket root.
    prefix: String,
    access_key_id: String,
    path_style: bool,
}

#[derive(Serialize, ToSchema)]
struct StorageView {
    /// `null` until a bucket is saved.
    s3: Option<S3View>,
    /// New attachment files go to the bucket; existing ones move there in the background.
    attachments_in_s3: bool,
    /// New backups are uploaded to the bucket instead of kept on this server.
    backups_in_s3: bool,
    backup_schedule: BackupSchedule,
    /// Attachment files still waiting to move between this server's disk and the bucket.
    files_to_move: u64,
    /// Why moving files stopped last time. Orbit retries every ten minutes.
    move_error: Option<String>,
    /// The last run of "Compress images"; `null` when this server cannot compress.
    image_compression: Option<CompressionStatus>,
}

#[derive(Serialize, ToSchema)]
struct InstanceSettingsView {
    registration_open: bool,
    /// `null` until a mail server is saved.
    smtp: Option<SmtpView>,
    storage: StorageView,
}

async fn settings_view(
    state: &WorkspaceState,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<Json<InstanceSettingsView>, ApiError> {
    let repository = InstanceSettingsRepository::new(state.identity.database().clone());
    let settings = repository
        .get()
        .await
        .map_err(|_| internal(instance, request_id))?;
    let storage = repository
        .storage()
        .await
        .map_err(|_| internal(instance, request_id))?;
    let moving = state
        .object_storage
        .as_ref()
        .map(|(_, store)| store.status())
        .unwrap_or_default();
    Ok(Json(InstanceSettingsView {
        storage: StorageView {
            s3: storage.s3.map(|s3| S3View {
                endpoint: s3.endpoint,
                region: s3.region,
                bucket: s3.bucket,
                prefix: s3.prefix,
                access_key_id: s3.access_key_id,
                path_style: s3.path_style,
            }),
            attachments_in_s3: storage.attachments_in_s3,
            backups_in_s3: storage.backups_in_s3,
            backup_schedule: storage.backup_schedule,
            files_to_move: u64::try_from(moving.remaining).unwrap_or(u64::MAX),
            move_error: moving.error,
            image_compression: state.image_compressor.as_ref().map(ImageCompressor::status),
        },
        registration_open: settings.registration_open,
        smtp: settings.smtp.map(|smtp| SmtpView {
            host: smtp.host,
            port: smtp.port,
            security: smtp.security,
            username: smtp.username,
            password_set: smtp.password.is_some(),
            from_address: smtp.from_address,
            from_name: smtp.from_name,
        }),
    }))
}

/// Root only.
async fn require_root(
    state: &WorkspaceState,
    headers: &HeaderMap,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<AuthenticatedSession, ApiError> {
    let session = authenticate(state, headers, instance, request_id).await?;
    require_installation_admin(state, session.user.id, instance, request_id).await?;
    Ok(session)
}

#[utoipa::path(get, path = "/api/v1/admin/settings", responses((status = 200, body = InstanceSettingsView)))]
async fn get_instance_settings(
    State(state): State<WorkspaceState>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<InstanceSettingsView>, ApiError> {
    let instance = "/api/v1/admin/settings";
    require_root(&state, &headers, instance, request_id.as_ref()).await?;
    settings_view(&state, instance, request_id.as_ref()).await
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct RegistrationBody {
    open: bool,
}

/// Opens or closes registration. Opening needs a saved mail server (409 `email_not_configured`).
#[utoipa::path(put, path = "/api/v1/admin/settings/registration", request_body = RegistrationBody, responses((status = 200, body = InstanceSettingsView)))]
async fn set_registration(
    State(state): State<WorkspaceState>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<RegistrationBody>,
) -> Result<Json<InstanceSettingsView>, ApiError> {
    let instance = "/api/v1/admin/settings/registration";
    let session = require_root(&state, &headers, instance, request_id.as_ref()).await?;
    InstanceSettingsRepository::new(state.identity.database().clone())
        .set_registration_open(
            session.user.id,
            body.open,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map_err(|error| match error {
            InstanceSettingsError::EmailNotConfigured => {
                email_not_configured(instance, request_id.as_ref())
            }
            InstanceSettingsError::Unavailable(_) => internal(instance, request_id.as_ref()),
        })?;
    settings_view(&state, instance, request_id.as_ref()).await
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct SmtpBody {
    host: String,
    port: u16,
    security: SmtpSecurity,
    username: Option<String>,
    /// Absent: keep the saved password. Empty: remove it. Otherwise: the new password.
    password: Option<String>,
    from_address: String,
    from_name: Option<String>,
}

#[utoipa::path(put, path = "/api/v1/admin/settings/smtp", request_body = SmtpBody, responses((status = 200, body = InstanceSettingsView)))]
async fn save_smtp(
    State(state): State<WorkspaceState>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<SmtpBody>,
) -> Result<Json<InstanceSettingsView>, ApiError> {
    let instance = "/api/v1/admin/settings/smtp";
    let session = require_root(&state, &headers, instance, request_id.as_ref()).await?;
    let invalid = |field: &str, detail: &str| {
        ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_smtp_settings",
            "Invalid mail settings",
            format!("{field}: {detail}"),
            instance,
            request_id.as_ref(),
        )
    };
    let host = body.host.trim().to_owned();
    if host.is_empty() || host.len() > 253 || host.contains(char::is_whitespace) {
        return Err(invalid("host", "enter the mail server's host name."));
    }
    if body.port == 0 {
        return Err(invalid("port", "enter a port from 1 to 65535."));
    }
    let from_address = body.from_address.trim().to_owned();
    if !mail::valid_address(&from_address) {
        return Err(invalid("from_address", "enter one email address."));
    }
    let optional = |value: Option<String>| {
        value
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
    };
    let username = optional(body.username);
    let from_name = optional(body.from_name);
    if from_name
        .as_ref()
        .is_some_and(|name| name.chars().count() > 120)
    {
        return Err(invalid("from_name", "use at most 120 characters."));
    }
    let password = match body.password {
        None => PasswordChange::Keep,
        Some(password) if password.is_empty() => PasswordChange::Clear,
        Some(password) => {
            let key = state
                .mailer
                .as_ref()
                .and_then(Mailer::app_key)
                .ok_or_else(|| {
                    ApiError::new(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "app_key_missing",
                        "App key missing",
                        "The server has no app key to encrypt the password with.",
                        instance,
                        request_id.as_ref(),
                    )
                })?;
            PasswordChange::Set(
                crate::secret_box::encrypt_secret(key, &password)
                    .map_err(|()| internal(instance, request_id.as_ref()))?,
            )
        }
    };
    InstanceSettingsRepository::new(state.identity.database().clone())
        .save_smtp(
            session.user.id,
            SmtpSettings {
                host,
                port: body.port,
                security: body.security,
                username,
                password: None,
                from_address,
                from_name,
            },
            password,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map_err(|_| internal(instance, request_id.as_ref()))?;
    settings_view(&state, instance, request_id.as_ref()).await
}

/// Removes the mail server. Registration closes with it.
#[utoipa::path(delete, path = "/api/v1/admin/settings/smtp", responses((status = 200, body = InstanceSettingsView)))]
async fn remove_smtp(
    State(state): State<WorkspaceState>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<InstanceSettingsView>, ApiError> {
    let instance = "/api/v1/admin/settings/smtp";
    let session = require_root(&state, &headers, instance, request_id.as_ref()).await?;
    InstanceSettingsRepository::new(state.identity.database().clone())
        .clear_smtp(
            session.user.id,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map_err(|_| internal(instance, request_id.as_ref()))?;
    settings_view(&state, instance, request_id.as_ref()).await
}

/// Sends a test email to the root user's own address with the saved settings and waits for the mail server.
#[utoipa::path(post, path = "/api/v1/admin/settings/smtp/test", responses((status = 204)))]
async fn send_test_email(
    State(state): State<WorkspaceState>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<StatusCode, ApiError> {
    let instance = "/api/v1/admin/settings/smtp/test";
    let session = require_root(&state, &headers, instance, request_id.as_ref()).await?;
    let mailer = state
        .enabled_mailer()
        .await
        .ok_or_else(|| email_not_configured(instance, request_id.as_ref()))?;
    mailer
        .send(mail::templates::test(&session.user.email))
        .await
        .map_err(|error| email_failed(&error, instance, request_id.as_ref()))?;
    Ok(StatusCode::NO_CONTENT)
}

fn storage_unreachable(
    error: impl std::fmt::Display,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> ApiError {
    ApiError::new(
        StatusCode::BAD_GATEWAY,
        "storage_unreachable",
        "Storage unreachable",
        format!("Orbit could not use the bucket: {error}"),
        instance,
        request_id,
    )
}

fn storage_in_use(
    detail: &'static str,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> ApiError {
    ApiError::new(
        StatusCode::CONFLICT,
        "storage_in_use",
        "Storage in use",
        detail,
        instance,
        request_id,
    )
}

fn app_key_missing(instance: &str, request_id: Option<&Extension<RequestId>>) -> ApiError {
    ApiError::new(
        StatusCode::SERVICE_UNAVAILABLE,
        "app_key_missing",
        "App key missing",
        "The server has no app key to encrypt the secret with.",
        instance,
        request_id,
    )
}

/// Makes the running server use the saved storage settings.
async fn reload_storage(
    state: &WorkspaceState,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<(), ApiError> {
    let Some((storage, _)) = &state.object_storage else {
        return Ok(());
    };
    let settings = InstanceSettingsRepository::new(state.identity.database().clone())
        .storage()
        .await
        .map_err(|_| internal(instance, request_id))?;
    crate::object_storage::apply(
        storage,
        &settings,
        state.mailer.as_ref().and_then(Mailer::app_key),
    )
    .map_err(|_| internal(instance, request_id))
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct S3Body {
    /// `https://s3.eu-central-1.amazonaws.com`, `https://<account>.r2.cloudflarestorage.com`, `http://minio:9000`.
    endpoint: String,
    /// Empty: `us-east-1`.
    region: Option<String>,
    bucket: String,
    /// Key prefix in the bucket, for example `orbit`. Empty: the bucket root.
    prefix: Option<String>,
    access_key_id: String,
    /// Absent or empty: keep the saved secret key.
    secret_access_key: Option<String>,
    /// `endpoint/bucket/key` URLs (MinIO and most self-hosted servers) instead of `bucket.endpoint/key`.
    #[serde(default)]
    path_style: bool,
}

/// Saves the bucket after writing, reading and deleting a test object in it (502 `storage_unreachable` when that
/// fails). Moving to another bucket or prefix needs the attachment files back on this server first.
#[utoipa::path(put, path = "/api/v1/admin/settings/s3", request_body = S3Body, responses((status = 200, body = InstanceSettingsView)))]
async fn save_s3(
    State(state): State<WorkspaceState>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<S3Body>,
) -> Result<Json<InstanceSettingsView>, ApiError> {
    let instance = "/api/v1/admin/settings/s3";
    let session = require_root(&state, &headers, instance, request_id.as_ref()).await?;
    let invalid = |field: &str, detail: &str| {
        ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_storage_settings",
            "Invalid storage settings",
            format!("{field}: {detail}"),
            instance,
            request_id.as_ref(),
        )
    };
    let endpoint = body.endpoint.trim().trim_end_matches('/').to_owned();
    if !(endpoint.starts_with("https://") || endpoint.starts_with("http://"))
        || endpoint.len() > 2048
    {
        return Err(invalid(
            "endpoint",
            "enter the http(s) URL of the S3 server.",
        ));
    }
    let bucket = body.bucket.trim().to_owned();
    if bucket.len() < 3 || bucket.len() > 63 {
        return Err(invalid(
            "bucket",
            "enter a bucket name of 3 to 63 characters.",
        ));
    }
    let region = body
        .region
        .map(|region| region.trim().to_owned())
        .filter(|region| !region.is_empty())
        .unwrap_or_else(|| "us-east-1".to_owned());
    if region.len() > 64 {
        return Err(invalid("region", "use at most 64 characters."));
    }
    let prefix = body
        .prefix
        .unwrap_or_default()
        .trim()
        .trim_matches('/')
        .to_owned();
    if prefix.len() > 512
        || !prefix
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/'))
        || prefix.split('/').any(|part| part == "." || part == "..")
    {
        return Err(invalid(
            "prefix",
            "use letters, digits, '-', '_', '.' and '/' (at most 512).",
        ));
    }
    let access_key_id = body.access_key_id.trim().to_owned();
    if access_key_id.is_empty() || access_key_id.len() > 256 {
        return Err(invalid("access_key_id", "enter the access key."));
    }
    let key = state
        .mailer
        .as_ref()
        .and_then(Mailer::app_key)
        .ok_or_else(|| app_key_missing(instance, request_id.as_ref()))?;
    let repository = InstanceSettingsRepository::new(state.identity.database().clone());
    let saved = repository
        .storage()
        .await
        .map_err(|_| internal(instance, request_id.as_ref()))?;
    let secret = match body.secret_access_key.filter(|secret| !secret.is_empty()) {
        Some(secret) => secret,
        None => saved
            .s3
            .as_ref()
            .and_then(|s3| crate::secret_box::decrypt_secret(key, &s3.secret_access_key).ok())
            .ok_or_else(|| invalid("secret_access_key", "enter the secret key."))?,
    };
    let s3 = S3Settings {
        endpoint,
        region,
        bucket,
        prefix,
        access_key_id,
        secret_access_key: crate::secret_box::encrypt_secret(key, &secret)
            .map_err(|()| internal(instance, request_id.as_ref()))?,
        path_style: body.path_style,
    };
    if let Some(old) = &saved.s3 {
        let moved =
            (&old.endpoint, &old.bucket, &old.prefix) != (&s3.endpoint, &s3.bucket, &s3.prefix);
        if moved {
            let in_use = saved.attachments_in_s3
                || crate::object_storage::saved_bucket(old, Some(key))
                    .map_err(|_| internal(instance, request_id.as_ref()))?
                    .any("blobs/")
                    .await
                    .map_err(|error| storage_unreachable(error, instance, request_id.as_ref()))?;
            if in_use {
                return Err(storage_in_use(
                    "Attachment files are in the saved bucket. Turn off S3 for attachments and wait until \
                     no files are left to move before you change the endpoint, bucket or prefix.",
                    instance,
                    request_id.as_ref(),
                ));
            }
        }
    }
    crate::object_storage::bucket(&s3, secret)
        .map_err(|error| invalid("endpoint", &error))?
        .probe()
        .await
        .map_err(|error| storage_unreachable(error, instance, request_id.as_ref()))?;
    repository
        .save_s3(
            session.user.id,
            &s3,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map_err(|_| internal(instance, request_id.as_ref()))?;
    reload_storage(&state, instance, request_id.as_ref()).await?;
    settings_view(&state, instance, request_id.as_ref()).await
}

/// Removes the bucket. Refused (409 `storage_in_use`) while attachments use it or files in it still wait to move
/// back to this server. Backups already in the bucket stay there.
#[utoipa::path(delete, path = "/api/v1/admin/settings/s3", responses((status = 200, body = InstanceSettingsView)))]
async fn remove_s3(
    State(state): State<WorkspaceState>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<InstanceSettingsView>, ApiError> {
    let instance = "/api/v1/admin/settings/s3";
    let session = require_root(&state, &headers, instance, request_id.as_ref()).await?;
    let repository = InstanceSettingsRepository::new(state.identity.database().clone());
    let saved = repository
        .storage()
        .await
        .map_err(|_| internal(instance, request_id.as_ref()))?;
    if let Some(s3) = &saved.s3 {
        if saved.attachments_in_s3 {
            return Err(storage_in_use(
                "Attachments use the bucket. Turn off S3 for attachments first.",
                instance,
                request_id.as_ref(),
            ));
        }
        let app_key = state.mailer.as_ref().and_then(Mailer::app_key);
        let files_left = crate::object_storage::saved_bucket(s3, app_key)
            .map_err(|_| internal(instance, request_id.as_ref()))?
            .any("blobs/")
            .await
            .map_err(|error| storage_unreachable(error, instance, request_id.as_ref()))?;
        if files_left {
            return Err(storage_in_use(
                "Attachment files are still moving back to this server. Try again when none are left.",
                instance,
                request_id.as_ref(),
            ));
        }
    }
    repository
        .clear_s3(
            session.user.id,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map_err(|_| internal(instance, request_id.as_ref()))?;
    reload_storage(&state, instance, request_id.as_ref()).await?;
    settings_view(&state, instance, request_id.as_ref()).await
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct StorageOptionsBody {
    /// New attachment files go to the bucket and existing ones move there. Off: they move back to this server.
    attachments_in_s3: bool,
    /// New backups are uploaded to the bucket.
    backups_in_s3: bool,
    backup_schedule: BackupSchedule,
}

/// Where attachments and backups are stored, and how often Orbit backs up by itself. Using S3 needs a saved
/// bucket (409 `storage_not_configured`).
#[utoipa::path(put, path = "/api/v1/admin/settings/storage", request_body = StorageOptionsBody, responses((status = 200, body = InstanceSettingsView)))]
async fn save_storage_options(
    State(state): State<WorkspaceState>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<StorageOptionsBody>,
) -> Result<Json<InstanceSettingsView>, ApiError> {
    let instance = "/api/v1/admin/settings/storage";
    let session = require_root(&state, &headers, instance, request_id.as_ref()).await?;
    InstanceSettingsRepository::new(state.identity.database().clone())
        .save_storage_options(
            session.user.id,
            body.attachments_in_s3,
            body.backups_in_s3,
            body.backup_schedule,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map_err(|error| match error {
            StorageSettingsError::NotConfigured => ApiError::new(
                StatusCode::CONFLICT,
                "storage_not_configured",
                "Storage not configured",
                "Save a bucket first.",
                instance,
                request_id.as_ref(),
            ),
            StorageSettingsError::Unavailable(_) => internal(instance, request_id.as_ref()),
        })?;
    reload_storage(&state, instance, request_id.as_ref()).await?;
    settings_view(&state, instance, request_id.as_ref()).await
}

/// Starts making large stored images smaller (at most 2560 px, WebP), on this server's disk and in S3. Runs in
/// the background; the settings show its progress in `storage.image_compression`. Starting it again while it runs
/// does nothing.
#[utoipa::path(post, path = "/api/v1/admin/attachments/compress", responses((status = 200, body = InstanceSettingsView)))]
async fn start_image_compression(
    State(state): State<WorkspaceState>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<InstanceSettingsView>, ApiError> {
    let instance = "/api/v1/admin/attachments/compress";
    require_root(&state, &headers, instance, request_id.as_ref()).await?;
    let Some(compressor) = &state.image_compressor else {
        return Err(internal(instance, request_id.as_ref()));
    };
    compressor.start();
    settings_view(&state, instance, request_id.as_ref()).await
}

#[derive(Serialize, ToSchema)]
struct BackupCreated {
    id: String,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
enum BackupSummaryKind {
    Snapshot,
    PreMigration,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
enum BackupLocation {
    /// This server's backup directory.
    Local,
    /// The S3 bucket from Admin → Storage.
    S3,
}

#[derive(Serialize, ToSchema)]
struct BackupSummary {
    id: String,
    kind: BackupSummaryKind,
    location: BackupLocation,
    /// Unix milliseconds.
    created_at: i64,
    /// Total size of the database and attachment files in the snapshot.
    byte_size: u64,
    file_count: usize,
    schema_version: i64,
    application_version: String,
}

impl From<BackupSnapshot> for BackupSummary {
    fn from(snapshot: BackupSnapshot) -> Self {
        let manifest = snapshot.manifest;
        Self {
            id: snapshot.id,
            kind: match manifest.kind {
                BackupKind::Snapshot => BackupSummaryKind::Snapshot,
                BackupKind::PreMigration => BackupSummaryKind::PreMigration,
            },
            location: if snapshot.in_object_storage {
                BackupLocation::S3
            } else {
                BackupLocation::Local
            },
            created_at: manifest.created_at,
            byte_size: manifest.files.iter().map(|file| file.byte_size).sum(),
            file_count: manifest.files.len(),
            schema_version: manifest.schema_version,
            application_version: manifest.application_version,
        }
    }
}

#[derive(Serialize, ToSchema)]
struct BackupList {
    items: Vec<BackupSummary>,
}

/// A backup snapshot as a ZIP archive: `{id}/manifest.json`, `{id}/database.sqlite` and `{id}/attachments/…`.
#[derive(ToSchema)]
#[schema(value_type = String, format = Binary)]
#[allow(dead_code)]
struct BackupArchive(Vec<u8>);

fn backup_service<'a>(
    state: &'a WorkspaceState,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<&'a BackupService, ApiError> {
    state.backups.as_ref().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "backup_unavailable",
            "Backup unavailable",
            "The backup service is not available.",
            instance,
            request_id,
        )
    })
}

#[utoipa::path(post, path = "/api/v1/admin/backups", responses((status = 201, body = BackupCreated)))]
async fn create_backup(
    State(state): State<WorkspaceState>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<(StatusCode, Json<BackupCreated>), ApiError> {
    let instance = "/api/v1/admin/backups";
    let session = authenticate(&state, &headers, instance, request_id.as_ref()).await?;
    require_installation_admin(&state, session.user.id, instance, request_id.as_ref()).await?;
    let backups = backup_service(&state, instance, request_id.as_ref())?;
    let snapshot = backups
        .create(state.identity.database())
        .await
        .map_err(|_| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "backup_failed",
                "Backup failed",
                "Orbit could not create a verified backup.",
                instance,
                request_id.as_ref(),
            )
        })?;
    Ok((StatusCode::CREATED, Json(BackupCreated { id: snapshot.id })))
}

/// Snapshots and pre-migration backups, newest first.
#[utoipa::path(get, path = "/api/v1/admin/backups", responses((status = 200, body = BackupList)))]
async fn list_backups(
    State(state): State<WorkspaceState>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<BackupList>, ApiError> {
    let instance = "/api/v1/admin/backups";
    let session = authenticate(&state, &headers, instance, request_id.as_ref()).await?;
    require_installation_admin(&state, session.user.id, instance, request_id.as_ref()).await?;
    let backups = backup_service(&state, instance, request_id.as_ref())?;
    let fail = |_| {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "backup_list_failed",
            "Backup list failed",
            "Orbit could not read the stored backups.",
            instance,
            request_id.as_ref(),
        )
    };
    let mut items = backups.list().await.map_err(fail)?;
    items.extend(backups.list_pre_migration().await.map_err(fail)?);
    items.sort_by(|left, right| {
        right
            .manifest
            .created_at
            .cmp(&left.manifest.created_at)
            .then_with(|| right.id.cmp(&left.id))
    });
    Ok(Json(BackupList {
        items: items.into_iter().map(BackupSummary::from).collect(),
    }))
}

/// Verifies the snapshot's checksums, then streams it as a ZIP archive.
#[utoipa::path(get, path = "/api/v1/admin/backups/{backup_id}/download", params(("backup_id" = String, Path)), responses((status = 200, body = BackupArchive, content_type = "application/zip")))]
async fn download_backup(
    State(state): State<WorkspaceState>,
    Path(backup_id): Path<String>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Response, ApiError> {
    let instance = format!("/api/v1/admin/backups/{backup_id}/download");
    let session = authenticate(&state, &headers, &instance, request_id.as_ref()).await?;
    require_installation_admin(&state, session.user.id, &instance, request_id.as_ref()).await?;
    let backups = backup_service(&state, &instance, request_id.as_ref())?;
    let failed = || {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "backup_download_failed",
            "Backup download failed",
            "Orbit could not verify or package this backup.",
            &instance,
            request_id.as_ref(),
        )
    };
    let snapshot = backups
        .verify(&backup_id)
        .await
        .map_err(|error| match error {
            BackupError::NotFound { .. } => ApiError::new(
                StatusCode::NOT_FOUND,
                "backup_not_found",
                "Backup not found",
                "The backup does not exist.",
                &instance,
                request_id.as_ref(),
            ),
            _ => failed(),
        })?;
    let name = format!("orbit-backup-{}.zip", snapshot.id);
    let file = tokio::task::spawn_blocking(move || write_backup_archive(&snapshot))
        .await
        .map_err(|_| failed())?
        .map_err(|_| failed())?;
    let length = file.metadata().map(|metadata| metadata.len()).ok();
    let mut response = Response::new(Body::from_stream(tokio_util::io::ReaderStream::new(
        tokio::fs::File::from_std(file),
    )));
    let headers = response.headers_mut();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/zip"));
    if let Ok(value) = HeaderValue::from_str(&format!("attachment; filename=\"{name}\"")) {
        headers.insert(CONTENT_DISPOSITION, value);
    }
    if let Some(length) = length {
        headers.insert(CONTENT_LENGTH, HeaderValue::from(length));
    }
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}

/// Deletes a snapshot or pre-migration backup, from this server or the S3 bucket.
#[utoipa::path(delete, path = "/api/v1/admin/backups/{backup_id}", params(("backup_id" = String, Path)), responses((status = 204)))]
async fn delete_backup(
    State(state): State<WorkspaceState>,
    Path(backup_id): Path<String>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<StatusCode, ApiError> {
    let instance = format!("/api/v1/admin/backups/{backup_id}");
    let session = authenticate(&state, &headers, &instance, request_id.as_ref()).await?;
    require_installation_admin(&state, session.user.id, &instance, request_id.as_ref()).await?;
    let backups = backup_service(&state, &instance, request_id.as_ref())?;
    backups
        .delete(&backup_id)
        .await
        .map_err(|error| match error {
            BackupError::NotFound { .. } => ApiError::new(
                StatusCode::NOT_FOUND,
                "backup_not_found",
                "Backup not found",
                "The backup does not exist.",
                &instance,
                request_id.as_ref(),
            ),
            _ => ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "backup_delete_failed",
                "Backup delete failed",
                "Orbit could not delete this backup.",
                &instance,
                request_id.as_ref(),
            ),
        })?;
    Ok(StatusCode::NO_CONTENT)
}

/// Writes the snapshot to an anonymous temporary file, rewound for reading. Entries sit under a `{id}/` folder, so
/// extracting the archive into the backup `snapshots` directory makes it restorable with `orbit backup restore`.
fn write_backup_archive(snapshot: &BackupSnapshot) -> std::io::Result<std::fs::File> {
    use std::io::{Seek, SeekFrom};
    use zip::write::SimpleFileOptions;
    use zip::{CompressionMethod, ZipWriter};

    let mut zip = ZipWriter::new(tempfile::tempfile()?);
    let manifest = std::iter::once(("manifest.json", u64::MAX));
    let files = snapshot
        .manifest
        .files
        .iter()
        .map(|file| (file.path.as_str(), file.byte_size));
    for (path, byte_size) in manifest.chain(files) {
        // Attachments are mostly compressed media already; the database and manifest compress well.
        let method = if path.starts_with("attachments/") {
            CompressionMethod::Stored
        } else {
            CompressionMethod::Deflated
        };
        let options = SimpleFileOptions::default()
            .compression_method(method)
            .large_file(byte_size >= u64::from(u32::MAX))
            .unix_permissions(0o600);
        zip.start_file(format!("{}/{path}", snapshot.id), options)
            .map_err(std::io::Error::other)?;
        let mut source = std::fs::File::open(snapshot.path.join(path))?;
        std::io::copy(&mut source, &mut zip)?;
    }
    let mut file = zip.finish().map_err(std::io::Error::other)?;
    file.seek(SeekFrom::Start(0))?;
    Ok(file)
}

#[derive(Deserialize, IntoParams, ToSchema)]
#[into_params(parameter_in = Query)]
#[serde(deny_unknown_fields)]
struct AdminAuditQuery {
    workspace_id: Option<String>,
    action: Option<String>,
    cursor: Option<String>,
    #[serde(default = "default_limit")]
    #[param(required = false)]
    limit: usize,
}

#[utoipa::path(get, path = "/api/v1/admin/audit", params(AdminAuditQuery), responses((status = 200, body = Page<crate::audit::AuditEvent>)))]
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

#[utoipa::path(get, path = "/api/v1/admin/audit/export", params(AdminAuditQuery), responses((status = 200, body = String, content_type = "text/csv")))]
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
            "Only an installation administrator may do this.",
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
    request_session(&state.identity, state.cookie_mode, headers)
        .await
        .ok_or_else(|| {
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

fn api_token_problem(
    error: ApiTokenError,
    instance: impl Into<String>,
    request_id: Option<&Extension<RequestId>>,
) -> ApiError {
    let instance = instance.into();
    match error {
        ApiTokenError::NotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            "workspace_resource_not_found",
            "Workspace resource not found",
            "The requested workspace resource was not found.",
            instance,
            request_id,
        ),
        ApiTokenError::Forbidden => ApiError::new(
            StatusCode::FORBIDDEN,
            "workspace_action_forbidden",
            "Workspace action forbidden",
            "Only workspace owners and administrators may manage API tokens.",
            instance,
            request_id,
        ),
        ApiTokenError::Unavailable(_) | ApiTokenError::InvalidIdentifier => ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            "Internal server error",
            "An unexpected error occurred. Use the request ID when contacting support.",
            instance,
            request_id,
        ),
    }
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

fn admin_account_problem(
    error: AdminAccountError,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> ApiError {
    match error {
        AdminAccountError::Forbidden => ApiError::new(
            StatusCode::FORBIDDEN,
            "installation_admin_required",
            "Installation administrator required",
            "Only an instance admin may manage accounts.",
            instance,
            request_id,
        ),
        AdminAccountError::NotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            "user_not_found",
            "User not found",
            "The requested global user was not found.",
            instance,
            request_id,
        ),
        AdminAccountError::RootAccount => ApiError::new(
            StatusCode::CONFLICT,
            "root_account",
            "Root account",
            "Nobody can suspend or reset the root account, and only the root user can change other admins.",
            instance,
            request_id,
        ),
        AdminAccountError::RootRequired => ApiError::new(
            StatusCode::FORBIDDEN,
            "root_required",
            "Root user required",
            "Only the root user can choose instance admins.",
            instance,
            request_id,
        ),
        AdminAccountError::OwnAccount => ApiError::new(
            StatusCode::CONFLICT,
            "own_account",
            "Own account",
            "Change your own password in your profile instead.",
            instance,
            request_id,
        ),
        AdminAccountError::Suspended => ApiError::new(
            StatusCode::CONFLICT,
            "account_suspended",
            "Account suspended",
            "Reinstate the account before you create a recovery link.",
            instance,
            request_id,
        ),
        AdminAccountError::Unavailable(_) => ApiError::new(
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

#[derive(Debug, Serialize, ToSchema)]
#[schema(as = WorkspaceProblem)]
pub(crate) struct ProblemBody {
    #[serde(rename = "type")]
    type_uri: String,
    title: &'static str,
    status: u16,
    code: &'static str,
    #[schema(value_type = String)]
    detail: std::borrow::Cow<'static, str>,
    instance: String,
    request_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    conflict: Option<ConflictBody>,
}

#[derive(Debug, Serialize, ToSchema)]
#[schema(as = WorkspaceConflict)]
pub(crate) struct ConflictBody {
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
        detail: impl Into<std::borrow::Cow<'static, str>>,
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
                detail: detail.into(),
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
                detail: "The resource changed after it was read. Refresh and retry.".into(),
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
