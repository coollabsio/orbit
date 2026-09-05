use std::fmt;
use std::net::SocketAddr;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Json;
use axum::Router;
use axum::extract::{Extension, FromRequest, Path, Request, State};
use axum::http::header::{CONTENT_TYPE, COOKIE, RETRY_AFTER, SET_COOKIE};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use orbit_platform::{
    AuthenticatedUser, ClientIp, Id, LoginThrottler, PasswordError, PasswordExecutor,
    PasswordService, RequestId, ThrottleDecision, TimestampMillis,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::ToSchema;

use crate::audit::AuditOutcome;
use crate::repositories::identity::{IdentityError, IdentityRepository, SetupError, SetupRequest};

const SESSION_COOKIE: &str = "__Host-orbit_session";
const DEV_SESSION_COOKIE: &str = "orbit_session_dev";
const GENERIC_LOGIN_DETAIL: &str = "Email or password is incorrect.";
const GENERIC_RECOVERY_DETAIL: &str =
    "Contact your installation administrator to request a password recovery link.";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CookieMode {
    secure: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("insecure authentication mode requires an explicit loopback listener")]
pub struct CookieModeError;

impl CookieMode {
    #[must_use]
    pub const fn secure() -> Self {
        Self { secure: true }
    }

    pub fn loopback_development(listener: SocketAddr) -> Result<Self, CookieModeError> {
        if !listener.ip().is_loopback() {
            return Err(CookieModeError);
        }
        tracing::warn!(%listener, "insecure loopback authentication cookie mode enabled");
        Ok(Self { secure: false })
    }

    pub(crate) const fn session_cookie_name(self) -> &'static str {
        if self.secure {
            SESSION_COOKIE
        } else {
            DEV_SESSION_COOKIE
        }
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct SetupLaunch {
    pub url: String,
}

impl fmt::Debug for SetupLaunch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SetupLaunch")
            .field("url", &"[REDACTED]")
            .finish()
    }
}

#[derive(Clone)]
pub struct AuthState {
    repository: Arc<IdentityRepository>,
    passwords: PasswordExecutor,
    throttler: Arc<Mutex<LoginThrottler>>,
    cookie_mode: CookieMode,
    dummy_hash: String,
}

impl AuthState {
    pub fn new(repository: Arc<IdentityRepository>, cookie_mode: CookieMode) -> Self {
        let passwords = PasswordService::default();
        let dummy_hash = passwords
            .hash("not a real account password")
            .expect("the static dummy password satisfies policy");
        Self {
            repository,
            passwords: PasswordExecutor::new(passwords, 2)
                .expect("password executor concurrency is non-zero"),
            throttler: Arc::new(Mutex::new(LoginThrottler::new())),
            cookie_mode,
            dummy_hash,
        }
    }
}

pub async fn initialize_auth(
    repository: Arc<IdentityRepository>,
    cookie_mode: CookieMode,
    public_origin: String,
) -> Result<(AuthState, Option<SetupLaunch>), crate::repositories::identity::IdentityError> {
    let issued = repository
        .initialize_setup_token(TimestampMillis::now())
        .await?;
    let launch = issued.map(|issued| SetupLaunch {
        url: format!(
            "{}/setup?token={}",
            public_origin.trim_end_matches('/'),
            issued.token
        ),
    });
    Ok((AuthState::new(repository, cookie_mode), launch))
}

pub fn auth_router(state: AuthState) -> Router {
    Router::new()
        .route("/api/v1/setup/status", get(setup_status))
        .route("/api/v1/setup/complete", post(setup_complete))
        .route("/api/v1/auth/login", post(login))
        .route("/api/v1/auth/logout", post(logout))
        .route("/api/v1/auth/me", get(me))
        .route("/api/v1/auth/recovery/request", post(recovery_request))
        .route("/api/v1/auth/recovery/complete", post(recovery_complete))
        .route("/api/v1/auth/sessions", get(list_sessions))
        .route("/api/v1/auth/sessions/{id}", delete(revoke_session))
        .with_state(state)
}

struct ApiJson<T>(T);

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

#[derive(Debug, Serialize, ToSchema)]
struct SetupStatus {
    complete: bool,
}

#[derive(Debug, Serialize, ToSchema)]
struct SetupResponse {
    user_id: String,
    workspace_id: String,
    project_id: String,
    session_id: String,
}

#[utoipa::path(get, path = "/api/v1/setup/status", responses((status = 200, body = SetupStatus)))]
async fn setup_status(State(state): State<AuthState>) -> Result<Json<SetupStatus>, ApiError> {
    let complete = state
        .repository
        .setup_complete()
        .await
        .map_err(|_| ApiError::internal("/api/v1/setup/status", None))?;
    Ok(Json(SetupStatus { complete }))
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct SetupBody {
    token: String,
    email: String,
    display_name: String,
    password: String,
    workspace_name: String,
    project_name: String,
}

#[utoipa::path(post, path = "/api/v1/setup/complete", request_body = SetupBody, responses((status = 201, body = SetupResponse), (status = 401, description = "invalid_setup_token", body = ProblemBody, content_type = "application/problem+json"), (status = 409, description = "setup_unavailable", body = ProblemBody, content_type = "application/problem+json"), (status = 422, description = "invalid_password", body = ProblemBody, content_type = "application/problem+json")))]
async fn setup_complete(
    State(state): State<AuthState>,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<SetupBody>,
) -> Result<Response, ApiError> {
    let token_valid = state
        .repository
        .setup_token_valid(&body.token, TimestampMillis::now())
        .await
        .map_err(|_| ApiError::internal("/api/v1/setup/complete", request_id.as_ref()))?;
    if !token_valid {
        state
            .repository
            .record_security_event(
                None,
                "setup.complete",
                AuditOutcome::Failure,
                "installation",
                None,
                request_id_value(request_id.as_ref()),
                json!({"reason":"invalid_token"}),
                TimestampMillis::now(),
            )
            .await
            .map_err(|_| ApiError::internal("/api/v1/setup/complete", request_id.as_ref()))?;
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "invalid_setup_token",
            "Invalid setup token",
            "The setup token is invalid or expired.",
            "/api/v1/setup/complete",
            request_id.as_ref(),
        ));
    }
    let password_hash =
        state.passwords.hash(body.password).await.map_err(|error| {
            password_problem(error, "/api/v1/setup/complete", request_id.as_ref())
        })?;
    let result = state
        .repository
        .complete_setup_audited(
            SetupRequest {
                token: body.token,
                email: body.email,
                display_name: body.display_name,
                password_hash,
                workspace_name: body.workspace_name,
                project_name: body.project_name,
            },
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map_err(|error| match error {
            SetupError::AlreadyComplete => ApiError::new(
                StatusCode::CONFLICT,
                "setup_unavailable",
                "Setup unavailable",
                "This installation has already been initialized.",
                "/api/v1/setup/complete",
                request_id.as_ref(),
            ),
            SetupError::InvalidToken => ApiError::new(
                StatusCode::UNAUTHORIZED,
                "invalid_setup_token",
                "Invalid setup token",
                "The setup token is invalid or expired.",
                "/api/v1/setup/complete",
                request_id.as_ref(),
            ),
            SetupError::Unavailable(_) => {
                ApiError::internal("/api/v1/setup/complete", request_id.as_ref())
            }
        })?;
    let session = result.session;
    let mut response = (
        StatusCode::CREATED,
        Json(SetupResponse {
            user_id: result.user_id.to_string(),
            workspace_id: result.workspace_id.to_string(),
            project_id: result.project_id.to_string(),
            session_id: session.id.to_string(),
        }),
    )
        .into_response();
    response.headers_mut().insert(
        SET_COOKIE,
        HeaderValue::from_str(&session_cookie(state.cookie_mode, &session.token, false))
            .expect("generated tokens are valid cookie values"),
    );
    Ok(response)
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct LoginBody {
    email: String,
    password: String,
}

#[derive(Debug, Serialize, ToSchema)]
struct LoginResponse {
    user: AuthUserResponse,
    session_id: String,
}

#[derive(Debug, Serialize, ToSchema)]
struct AuthUserResponse {
    id: String,
    email: String,
    display_name: String,
}

#[utoipa::path(post, path = "/api/v1/auth/login", request_body = LoginBody, responses((status = 200, body = LoginResponse), (status = 401, description = "invalid_credentials", body = ProblemBody, content_type = "application/problem+json"), (status = 429, description = "authentication_throttled", body = ProblemBody, content_type = "application/problem+json")))]
async fn login(
    State(state): State<AuthState>,
    client_ip: Option<Extension<ClientIp>>,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<LoginBody>,
) -> Result<Response, ApiError> {
    let ip = client_ip
        .map(|Extension(client_ip)| client_ip.0)
        .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
    let admission = state
        .throttler
        .lock()
        .expect("throttler mutex poisoned")
        .reserve(&body.email, ip, TimestampMillis::now());
    let reservation = match admission {
        Ok(reservation) => reservation,
        Err(ThrottleDecision::RetryAfter(delay)) => {
            state
                .repository
                .record_security_event(
                    None,
                    "authentication.throttled",
                    AuditOutcome::Failure,
                    "authentication",
                    None,
                    request_id_value(request_id.as_ref()),
                    json!({}),
                    TimestampMillis::now(),
                )
                .await
                .map_err(|_| ApiError::internal("/api/v1/auth/login", request_id.as_ref()))?;
            return Err(ApiError::throttled(
                delay,
                "/api/v1/auth/login",
                request_id.as_ref(),
            ));
        }
        Err(ThrottleDecision::Allowed) => unreachable!("allowed admission returns a reservation"),
    };

    let identity = state
        .repository
        .find_by_email(&body.email)
        .await
        .ok()
        .flatten();
    let hash = identity
        .as_ref()
        .map_or(state.dummy_hash.as_str(), |identity| {
            identity.password_hash.as_str()
        });
    let verification = state
        .passwords
        .verify(body.password, hash.to_owned())
        .await
        .ok();
    let valid = identity
        .as_ref()
        .is_some_and(|identity| !identity.suspended)
        && verification.as_ref().is_some_and(|result| result.valid);
    if !valid {
        state
            .throttler
            .lock()
            .expect("throttler mutex poisoned")
            .finish_failure(reservation, TimestampMillis::now());
        let actor_id = identity.as_ref().map(|identity| identity.id);
        state
            .repository
            .record_security_event(
                actor_id,
                "authentication.login",
                AuditOutcome::Failure,
                "authentication",
                actor_id,
                request_id_value(request_id.as_ref()),
                json!({}),
                TimestampMillis::now(),
            )
            .await
            .map_err(|_| ApiError::internal("/api/v1/auth/login", request_id.as_ref()))?;
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "invalid_credentials",
            "Authentication failed",
            GENERIC_LOGIN_DETAIL,
            "/api/v1/auth/login",
            request_id.as_ref(),
        ));
    }

    let identity = identity.expect("valid login has an identity");
    let replacement_hash = verification.and_then(|result| result.replacement_hash);
    let user = AuthenticatedUser {
        id: identity.id,
        email: identity.email,
        display_name: identity.display_name,
    };
    let session = state
        .repository
        .create_session_audited(
            &user,
            &identity.password_hash,
            replacement_hash.as_deref(),
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await;
    let session = match session {
        Ok(session) => {
            state
                .throttler
                .lock()
                .expect("throttler mutex poisoned")
                .finish_success(reservation);
            session
        }
        Err(IdentityError::InvalidCredential) => {
            state
                .throttler
                .lock()
                .expect("throttler mutex poisoned")
                .finish_failure(reservation, TimestampMillis::now());
            return Err(ApiError::new(
                StatusCode::UNAUTHORIZED,
                "invalid_credentials",
                "Authentication failed",
                GENERIC_LOGIN_DETAIL,
                "/api/v1/auth/login",
                request_id.as_ref(),
            ));
        }
        Err(_) => {
            return Err(ApiError::internal(
                "/api/v1/auth/login",
                request_id.as_ref(),
            ));
        }
    };
    let mut response = Json(LoginResponse {
        user: AuthUserResponse {
            id: user.id.to_string(),
            email: user.email,
            display_name: user.display_name,
        },
        session_id: session.id.to_string(),
    })
    .into_response();
    response.headers_mut().insert(
        SET_COOKIE,
        HeaderValue::from_str(&session_cookie(state.cookie_mode, &session.token, false))
            .expect("generated tokens are valid cookie values"),
    );
    Ok(response)
}

#[utoipa::path(post, path = "/api/v1/auth/logout", responses((status = 204), (status = 401, description = "authentication_required", body = ProblemBody, content_type = "application/problem+json")))]
async fn logout(
    State(state): State<AuthState>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Response, ApiError> {
    let session =
        authenticate(&state, &headers, "/api/v1/auth/logout", request_id.as_ref()).await?;
    let revoked = state
        .repository
        .revoke_session_audited(
            session.id,
            session.user.id,
            "session.logout",
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map_err(|_| ApiError::internal("/api/v1/auth/logout", request_id.as_ref()))?;
    if !revoked {
        return Err(ApiError::internal(
            "/api/v1/auth/logout",
            request_id.as_ref(),
        ));
    }
    let mut response = StatusCode::NO_CONTENT.into_response();
    response.headers_mut().insert(
        SET_COOKIE,
        HeaderValue::from_str(&session_cookie(state.cookie_mode, "", true)).unwrap(),
    );
    Ok(response)
}

#[utoipa::path(get, path = "/api/v1/auth/me", responses((status = 200, body = AuthUserResponse), (status = 401, description = "authentication_required", body = ProblemBody, content_type = "application/problem+json")))]
async fn me(
    State(state): State<AuthState>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<AuthUserResponse>, ApiError> {
    let user = authenticate(&state, &headers, "/api/v1/auth/me", request_id.as_ref())
        .await?
        .user;
    Ok(Json(AuthUserResponse {
        id: user.id.to_string(),
        email: user.email,
        display_name: user.display_name,
    }))
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct RecoveryRequestBody {
    email: String,
}

#[derive(Serialize, ToSchema)]
struct RecoveryRequestResponse {
    detail: &'static str,
}

#[utoipa::path(post, path = "/api/v1/auth/recovery/request", request_body = RecoveryRequestBody, responses((status = 202, body = RecoveryRequestResponse)))]
async fn recovery_request(
    State(_state): State<AuthState>,
    _request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<RecoveryRequestBody>,
) -> Response {
    let _ = body.email;
    (
        StatusCode::ACCEPTED,
        Json(RecoveryRequestResponse {
            detail: GENERIC_RECOVERY_DETAIL,
        }),
    )
        .into_response()
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct RecoveryCompleteBody {
    token: String,
    password: String,
}

#[utoipa::path(post, path = "/api/v1/auth/recovery/complete", request_body = RecoveryCompleteBody, responses((status = 204), (status = 400, description = "invalid_recovery_token", body = ProblemBody, content_type = "application/problem+json"), (status = 422, description = "invalid_password", body = ProblemBody, content_type = "application/problem+json")))]
async fn recovery_complete(
    State(state): State<AuthState>,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<RecoveryCompleteBody>,
) -> Result<StatusCode, ApiError> {
    let valid = state
        .repository
        .recovery_token_valid(&body.token, TimestampMillis::now())
        .await
        .map_err(|_| ApiError::internal("/api/v1/auth/recovery/complete", request_id.as_ref()))?;
    if !valid {
        state
            .repository
            .record_security_event(
                None,
                "recovery.completed",
                AuditOutcome::Failure,
                "user",
                None,
                request_id_value(request_id.as_ref()),
                json!({"reason":"invalid_token"}),
                TimestampMillis::now(),
            )
            .await
            .map_err(|_| {
                ApiError::internal("/api/v1/auth/recovery/complete", request_id.as_ref())
            })?;
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_recovery_token",
            "Recovery failed",
            "The recovery token is invalid or expired.",
            "/api/v1/auth/recovery/complete",
            request_id.as_ref(),
        ));
    }
    let password_hash = state.passwords.hash(body.password).await.map_err(|error| {
        password_problem(error, "/api/v1/auth/recovery/complete", request_id.as_ref())
    })?;
    state
        .repository
        .complete_recovery_audited(
            &body.token,
            &password_hash,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map_err(|error| match error {
            IdentityError::InvalidCredential => ApiError::new(
                StatusCode::BAD_REQUEST,
                "invalid_recovery_token",
                "Recovery failed",
                "The recovery token is invalid or expired.",
                "/api/v1/auth/recovery/complete",
                request_id.as_ref(),
            ),
            _ => ApiError::internal("/api/v1/auth/recovery/complete", request_id.as_ref()),
        })?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(get, path = "/api/v1/auth/sessions", responses((status = 200, body = Vec<orbit_platform::SessionRecord>), (status = 401, description = "authentication_required", body = ProblemBody, content_type = "application/problem+json")))]
async fn list_sessions(
    State(state): State<AuthState>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<Vec<orbit_platform::SessionRecord>>, ApiError> {
    let session = authenticate(
        &state,
        &headers,
        "/api/v1/auth/sessions",
        request_id.as_ref(),
    )
    .await?;
    let mut sessions = state
        .repository
        .list_sessions(session.user.id, TimestampMillis::now())
        .await
        .map_err(|_| ApiError::internal("/api/v1/auth/sessions", request_id.as_ref()))?;
    for item in &mut sessions {
        item.current = item.id == session.id;
    }
    Ok(Json(sessions))
}

#[utoipa::path(delete, path = "/api/v1/auth/sessions/{id}", params(("id" = String, Path)), responses((status = 204), (status = 401, description = "authentication_required", body = ProblemBody, content_type = "application/problem+json"), (status = 404, description = "session_not_found", body = ProblemBody, content_type = "application/problem+json")))]
async fn revoke_session(
    State(state): State<AuthState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Response, ApiError> {
    let session = authenticate(
        &state,
        &headers,
        "/api/v1/auth/sessions/{id}",
        request_id.as_ref(),
    )
    .await?;
    let id = id.parse::<Id>().map_err(|_| {
        ApiError::new(
            StatusCode::NOT_FOUND,
            "session_not_found",
            "Session not found",
            "The requested session was not found.",
            "/api/v1/auth/sessions/{id}",
            request_id.as_ref(),
        )
    })?;
    let revoked = state
        .repository
        .revoke_session_audited(
            id,
            session.user.id,
            "session.revoked",
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map_err(|_| ApiError::internal("/api/v1/auth/sessions/{id}", request_id.as_ref()))?;
    if !revoked {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "session_not_found",
            "Session not found",
            "The requested session was not found.",
            "/api/v1/auth/sessions/{id}",
            request_id.as_ref(),
        ));
    }
    let mut response = StatusCode::NO_CONTENT.into_response();
    if id == session.id {
        response.headers_mut().insert(
            SET_COOKIE,
            HeaderValue::from_str(&session_cookie(state.cookie_mode, "", true))
                .expect("session cookie is a valid header"),
        );
    }
    Ok(response)
}

fn request_id_value(request_id: Option<&Extension<RequestId>>) -> &str {
    request_id.map_or("unknown", |Extension(value)| value.as_str())
}

async fn authenticate(
    state: &AuthState,
    headers: &HeaderMap,
    instance: &'static str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<crate::repositories::identity::AuthenticatedSession, ApiError> {
    let token = cookie_value(headers, cookie_name(state.cookie_mode)).ok_or_else(|| {
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
        .repository
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

fn cookie_name(mode: CookieMode) -> &'static str {
    mode.session_cookie_name()
}

fn session_cookie(mode: CookieMode, token: &str, expired: bool) -> String {
    let mut cookie = format!("{}={token}; Path=/", cookie_name(mode));
    if mode.secure {
        cookie.push_str("; Secure");
    }
    cookie.push_str("; HttpOnly; SameSite=Lax");
    if expired {
        cookie.push_str("; Max-Age=0");
    }
    cookie
}

pub(crate) fn issued_session_cookie(mode: CookieMode, token: &str) -> String {
    session_cookie(mode, token, false)
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

fn password_problem(
    error: PasswordError,
    instance: &'static str,
    request_id: Option<&Extension<RequestId>>,
) -> ApiError {
    let detail = match error {
        PasswordError::InvalidLength => "Password must contain between 12 and 128 characters.",
        PasswordError::CommonPassword => "Choose a password that is not commonly used.",
        PasswordError::InvalidHash | PasswordError::HashingFailed => {
            return ApiError::internal(instance, request_id);
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

#[derive(Debug, Serialize, ToSchema)]
#[schema(as = AuthProblem)]
struct ProblemBody {
    #[serde(rename = "type")]
    type_uri: String,
    title: &'static str,
    status: u16,
    code: &'static str,
    detail: &'static str,
    instance: String,
    request_id: String,
}

struct ApiError {
    status: StatusCode,
    body: Box<ProblemBody>,
    retry_after: Option<Duration>,
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
            }),
            retry_after: None,
        }
    }

    fn internal(instance: &'static str, request_id: Option<&Extension<RequestId>>) -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            "Internal server error",
            "An unexpected error occurred. Use the request ID when contacting support.",
            instance,
            request_id,
        )
    }

    fn throttled(
        retry_after: Duration,
        instance: &'static str,
        request_id: Option<&Extension<RequestId>>,
    ) -> Self {
        let mut error = Self::new(
            StatusCode::TOO_MANY_REQUESTS,
            "authentication_throttled",
            "Too many attempts",
            "Too many authentication attempts were made. Try again later.",
            instance,
            request_id,
        );
        error.retry_after = Some(retry_after);
        error
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::body::{Body, to_bytes};
    use axum::http::{Request, StatusCode, header};
    use orbit_platform::{
        HttpPlatformLayer, OriginPolicy, PasswordService, TestDatabase, TimestampMillis,
    };
    use serde_json::{Value, json};
    use tower::ServiceExt;

    use super::{AuthState, CookieMode, SetupLaunch, auth_router, initialize_auth};
    use crate::repositories::identity::{IdentityRepository, SetupRequest};

    #[tokio::test]
    async fn auth_login_uses_a_host_only_secure_cookie() {
        let (app, _, _database) = application(CookieMode::secure()).await;
        let response = app
            .oneshot(json_request(
                "/api/v1/auth/login",
                json!({"email":"owner@example.com","password":"correct horse battery"}),
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let cookie = response
            .headers()
            .get(header::SET_COOKIE)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(cookie.starts_with("__Host-orbit_session="));
        assert!(cookie.contains("; Path=/"));
        assert!(cookie.contains("; Secure"));
        assert!(cookie.contains("; HttpOnly"));
        assert!(cookie.contains("; SameSite=Lax"));
        assert!(!cookie.contains("Domain="));
    }

    #[tokio::test]
    async fn revoking_the_current_session_expires_its_browser_cookie() {
        let (app, _, _database) = application(CookieMode::secure()).await;
        let login = app
            .clone()
            .oneshot(json_request(
                "/api/v1/auth/login",
                json!({"email":"owner@example.com","password":"correct horse battery"}),
            ))
            .await
            .unwrap();
        let cookie = login.headers()[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();
        let login_body: Value = serde_json::from_slice(&body(login).await).unwrap();
        let session_id = login_body["session_id"].as_str().unwrap();

        let response = app
            .oneshot(cookie_request(
                "DELETE",
                &format!("/api/v1/auth/sessions/{session_id}"),
                &cookie,
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        let expired = response.headers()[header::SET_COOKIE].to_str().unwrap();
        assert!(expired.starts_with("__Host-orbit_session="));
        assert!(expired.contains("Max-Age=0"));
        assert!(expired.contains("; Secure; HttpOnly; SameSite=Lax"));
    }

    #[tokio::test]
    async fn public_recovery_is_a_generic_no_op_that_preserves_an_admin_link() {
        let database = TestDatabase::new().await.unwrap();
        let repository = Arc::new(IdentityRepository::new((*database).clone()));
        setup_repository(&repository).await;
        let identity = repository
            .find_by_email("owner@example.com")
            .await
            .unwrap()
            .unwrap();
        repository
            .store_recovery_token_audited(
                identity.id,
                "admin-issued-token",
                TimestampMillis::from_millis(TimestampMillis::now().as_millis() + 60_000),
                "operator-cli",
            )
            .await
            .unwrap();
        let state = AuthState::new(Arc::clone(&repository), CookieMode::secure());
        let app = auth_router(state).layer(HttpPlatformLayer::new(OriginPolicy::new(
            "https://orbit.test",
        )));

        let response = app
            .oneshot(json_request(
                "/api/v1/auth/recovery/request",
                json!({"email":"owner@example.com"}),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        assert!(
            repository
                .recovery_token_valid("admin-issued-token", TimestampMillis::now())
                .await
                .unwrap()
        );
        let response: Value = serde_json::from_slice(&body(response).await).unwrap();
        assert_eq!(
            response["detail"],
            "Contact your installation administrator to request a password recovery link."
        );
    }

    #[tokio::test]
    async fn concurrent_recovery_completion_maps_the_loser_to_documented_bad_request() {
        let database = TestDatabase::new().await.unwrap();
        let repository = Arc::new(IdentityRepository::new((*database).clone()));
        setup_repository(&repository).await;
        let identity = repository
            .find_by_email("owner@example.com")
            .await
            .unwrap()
            .unwrap();
        repository
            .store_recovery_token_audited(
                identity.id,
                "one-time-recovery-token",
                TimestampMillis::from_millis(TimestampMillis::now().as_millis() + 60_000),
                "operator-cli",
            )
            .await
            .unwrap();
        let app = auth_router(AuthState::new(repository, CookieMode::secure())).layer(
            HttpPlatformLayer::new(OriginPolicy::new("https://orbit.test")),
        );
        let first = app.clone().oneshot(json_request(
            "/api/v1/auth/recovery/complete",
            json!({"token":"one-time-recovery-token","password":"new secure password one"}),
        ));
        let second = app.oneshot(json_request(
            "/api/v1/auth/recovery/complete",
            json!({"token":"one-time-recovery-token","password":"new secure password two"}),
        ));

        let (first, second) = tokio::join!(first, second);
        let mut responses = vec![first.unwrap(), second.unwrap()];
        responses.sort_by_key(|response| response.status());
        assert_eq!(responses[0].status(), StatusCode::NO_CONTENT);
        assert_eq!(responses[1].status(), StatusCode::BAD_REQUEST);
        let problem: Value = serde_json::from_slice(&body(responses.pop().unwrap()).await).unwrap();
        assert_eq!(problem["code"], "invalid_recovery_token");
    }

    #[test]
    fn auth_secret_bearing_urls_are_redacted_from_debug_output() {
        let secret = "secret-bearer-token";
        let setup = SetupLaunch {
            url: format!("https://orbit.test/setup?token={secret}"),
        };

        let output = format!("{setup:?}");
        assert!(output.contains("[REDACTED]"));
        assert!(!output.contains(secret));
    }

    #[tokio::test]
    async fn auth_json_rejections_use_problem_details() {
        let (app, _, _database) = application(CookieMode::secure()).await;
        for request in [
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/login")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::ORIGIN, "https://orbit.test")
                .body(Body::from("{"))
                .unwrap(),
            json_request(
                "/api/v1/auth/login",
                json!({"email":"owner@example.com","password":"wrong password value","extra":true}),
            ),
        ] {
            let response = app.clone().oneshot(request).await.unwrap();
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            assert_eq!(
                response.headers().get(header::CONTENT_TYPE).unwrap(),
                "application/problem+json"
            );
            let problem: Value = serde_json::from_slice(&body(response).await).unwrap();
            assert_eq!(problem["code"], "invalid_request");
            assert!(problem["request_id"].as_str().is_some());
        }
    }

    #[tokio::test]
    async fn auth_missing_origin_is_rejected_before_setup_or_recovery_mutation() {
        let (app, _, _database) = application(CookieMode::secure()).await;
        let request = Request::builder()
            .method("POST")
            .uri("/api/v1/auth/recovery/request")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(r#"{"email":"owner@example.com"}"#))
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn auth_logout_does_not_clear_cookie_when_revocation_fails() {
        let (app, repository, _database) = application(CookieMode::secure()).await;
        let login = app
            .clone()
            .oneshot(json_request(
                "/api/v1/auth/login",
                json!({"email":"owner@example.com","password":"correct horse battery"}),
            ))
            .await
            .unwrap();
        let cookie = login
            .headers()
            .get(header::SET_COOKIE)
            .unwrap()
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();
        repository
            .database()
            .execute(
                "CREATE TRIGGER reject_logout BEFORE UPDATE OF revoked_at ON sessions \
             BEGIN SELECT RAISE(ABORT, 'revocation unavailable'); END",
            )
            .await
            .unwrap();

        let logout = app
            .clone()
            .oneshot(cookie_request("POST", "/api/v1/auth/logout", &cookie))
            .await
            .unwrap();
        assert_eq!(logout.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert!(logout.headers().get(header::SET_COOKIE).is_none());
        let me = app
            .oneshot(cookie_request("GET", "/api/v1/auth/me", &cookie))
            .await
            .unwrap();
        assert_eq!(me.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn auth_insecure_cookie_mode_requires_an_explicit_loopback_listener() {
        assert!(CookieMode::loopback_development("127.0.0.1:8080".parse().unwrap()).is_ok());
        assert!(CookieMode::loopback_development("0.0.0.0:8080".parse().unwrap()).is_err());
        assert!(CookieMode::loopback_development("192.0.2.1:8080".parse().unwrap()).is_err());
    }

    #[tokio::test]
    async fn auth_initialization_returns_setup_url_only_once() {
        let database = TestDatabase::new().await.unwrap();
        let repository = Arc::new(IdentityRepository::new((*database).clone()));
        let (_, launch) = initialize_auth(
            Arc::clone(&repository),
            CookieMode::secure(),
            "https://orbit.test".to_owned(),
        )
        .await
        .unwrap();
        let launch = launch.unwrap();
        assert!(launch.url.starts_with("https://orbit.test/setup?token="));
        let (_, repeated) = initialize_auth(
            repository,
            CookieMode::secure(),
            "https://orbit.test".to_owned(),
        )
        .await
        .unwrap();
        assert!(repeated.is_none());
    }

    #[tokio::test]
    async fn auth_setup_creates_the_initial_session_with_the_secure_cookie_policy() {
        let database = TestDatabase::new().await.unwrap();
        let repository = Arc::new(IdentityRepository::new((*database).clone()));
        repository
            .store_setup_token(
                "operator-secret",
                TimestampMillis::from_millis(TimestampMillis::now().as_millis() + 60_000),
            )
            .await
            .unwrap();
        let app = auth_router(AuthState::new(repository, CookieMode::secure())).layer(
            HttpPlatformLayer::new(OriginPolicy::new("https://orbit.test")),
        );

        let response = app
            .oneshot(json_request(
                "/api/v1/setup/complete",
                json!({
                    "token": "operator-secret",
                    "email": "owner@example.com",
                    "display_name": "Owner",
                    "password": "correct horse battery",
                    "workspace_name": "Orbit",
                    "project_name": "General"
                }),
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);
        assert!(
            response
                .headers()
                .get(header::SET_COOKIE)
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("__Host-orbit_session=")
        );
    }

    #[tokio::test]
    async fn auth_loopback_development_cookie_is_explicitly_insecure_and_unprefixed() {
        let (app, _, _database) = application(
            CookieMode::loopback_development("127.0.0.1:8080".parse().unwrap()).unwrap(),
        )
        .await;
        let response = app
            .oneshot(json_request(
                "/api/v1/auth/login",
                json!({"email":"owner@example.com","password":"correct horse battery"}),
            ))
            .await
            .unwrap();

        let cookie = response
            .headers()
            .get(header::SET_COOKIE)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(cookie.starts_with("orbit_session_dev="));
        assert!(!cookie.contains("__Host-"));
        assert!(!cookie.contains("; Secure"));
        assert!(cookie.contains("; HttpOnly; SameSite=Lax"));
    }

    #[tokio::test]
    async fn auth_login_and_recovery_errors_do_not_enumerate_accounts() {
        let (app, _, _database) = application(CookieMode::secure()).await;
        let wrong_password = app
            .clone()
            .oneshot(json_request(
                "/api/v1/auth/login",
                json!({"email":"owner@example.com","password":"wrong password value"}),
            ))
            .await
            .unwrap();
        let missing_account = app
            .clone()
            .oneshot(json_request(
                "/api/v1/auth/login",
                json!({"email":"missing@example.com","password":"wrong password value"}),
            ))
            .await
            .unwrap();
        assert_eq!(wrong_password.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(missing_account.status(), StatusCode::UNAUTHORIZED);
        let mut wrong: Value = serde_json::from_slice(&body(wrong_password).await).unwrap();
        let mut missing: Value = serde_json::from_slice(&body(missing_account).await).unwrap();
        wrong.as_object_mut().unwrap().remove("request_id");
        missing.as_object_mut().unwrap().remove("request_id");
        assert_eq!(wrong, missing);

        let known_recovery = app
            .clone()
            .oneshot(json_request(
                "/api/v1/auth/recovery/request",
                json!({"email":"owner@example.com"}),
            ))
            .await
            .unwrap();
        let unknown_recovery = app
            .oneshot(json_request(
                "/api/v1/auth/recovery/request",
                json!({"email":"missing@example.com"}),
            ))
            .await
            .unwrap();
        assert_eq!(known_recovery.status(), StatusCode::ACCEPTED);
        assert_eq!(unknown_recovery.status(), StatusCode::ACCEPTED);
        assert_eq!(body(known_recovery).await, body(unknown_recovery).await);
    }

    #[tokio::test]
    async fn auth_origin_failure_happens_before_the_login_handler() {
        let (app, _, _database) = application(CookieMode::secure()).await;
        let mut request = json_request(
            "/api/v1/auth/login",
            json!({"email":"owner@example.com","password":"correct horse battery"}),
        );
        request
            .headers_mut()
            .insert(header::ORIGIN, "https://evil.example".parse().unwrap());
        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let problem: Value = serde_json::from_slice(&body(response).await).unwrap();
        assert_eq!(problem["code"], "origin_forbidden");
    }

    #[tokio::test]
    async fn auth_session_revocation_takes_effect_immediately() {
        let (app, _, _database) = application(CookieMode::secure()).await;
        let login = app
            .clone()
            .oneshot(json_request(
                "/api/v1/auth/login",
                json!({"email":"owner@example.com","password":"correct horse battery"}),
            ))
            .await
            .unwrap();
        let set_cookie = login
            .headers()
            .get(header::SET_COOKIE)
            .unwrap()
            .to_str()
            .unwrap();
        let cookie = set_cookie.split(';').next().unwrap().to_owned();
        let login_body: Value = serde_json::from_slice(&body(login).await).unwrap();
        let session_id = login_body["session_id"].as_str().unwrap();

        let me = app
            .clone()
            .oneshot(cookie_request("GET", "/api/v1/auth/me", &cookie))
            .await
            .unwrap();
        assert_eq!(me.status(), StatusCode::OK);

        let sessions = app
            .clone()
            .oneshot(cookie_request("GET", "/api/v1/auth/sessions", &cookie))
            .await
            .unwrap();
        let sessions: Value = serde_json::from_slice(&body(sessions).await).unwrap();
        assert_eq!(sessions[0]["id"], session_id);
        assert_eq!(sessions[0]["current"], true);

        let revoke = app
            .clone()
            .oneshot(cookie_request(
                "DELETE",
                &format!("/api/v1/auth/sessions/{session_id}"),
                &cookie,
            ))
            .await
            .unwrap();
        assert_eq!(revoke.status(), StatusCode::NO_CONTENT);

        let me = app
            .oneshot(cookie_request("GET", "/api/v1/auth/me", &cookie))
            .await
            .unwrap();
        assert_eq!(me.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn auth_sessions_survive_rebuilding_the_http_state() {
        let (app, repository, _database) = application(CookieMode::secure()).await;
        let login = app
            .oneshot(json_request(
                "/api/v1/auth/login",
                json!({"email":"owner@example.com","password":"correct horse battery"}),
            ))
            .await
            .unwrap();
        let cookie = login
            .headers()
            .get(header::SET_COOKIE)
            .unwrap()
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();

        let rebuilt = auth_router(AuthState::new(repository, CookieMode::secure())).layer(
            HttpPlatformLayer::new(OriginPolicy::new("https://orbit.test")),
        );
        let me = rebuilt
            .oneshot(cookie_request("GET", "/api/v1/auth/me", &cookie))
            .await
            .unwrap();
        assert_eq!(me.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn auth_security_outcomes_are_recorded_in_the_durable_audit() {
        let (app, _, database) = application(CookieMode::secure()).await;
        let failed = app
            .clone()
            .oneshot(json_request(
                "/api/v1/auth/login",
                json!({"email":"owner@example.com","password":"wrong password value"}),
            ))
            .await
            .unwrap();
        assert_eq!(failed.status(), StatusCode::UNAUTHORIZED);
        let login = app
            .clone()
            .oneshot(json_request(
                "/api/v1/auth/login",
                json!({"email":"owner@example.com","password":"correct horse battery"}),
            ))
            .await
            .unwrap();
        let cookie = login
            .headers()
            .get(header::SET_COOKIE)
            .unwrap()
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();
        let logout = app
            .clone()
            .oneshot(cookie_request("POST", "/api/v1/auth/logout", &cookie))
            .await
            .unwrap();
        assert_eq!(logout.status(), StatusCode::NO_CONTENT);
        let recovery = app
            .oneshot(json_request(
                "/api/v1/auth/recovery/request",
                json!({"email":"owner@example.com"}),
            ))
            .await
            .unwrap();
        assert_eq!(recovery.status(), StatusCode::ACCEPTED);

        let rows = sqlx::query_as::<_, (String, String)>(
            "SELECT action, outcome FROM audit_events WHERE action LIKE 'authentication.%' \
             OR action = 'session.logout' ORDER BY occurred_at, id",
        )
        .fetch_all(database.pool())
        .await
        .unwrap();
        assert!(rows.contains(&("authentication.login".to_owned(), "failure".to_owned())));
        assert!(rows.contains(&("authentication.login".to_owned(), "success".to_owned())));
        assert!(rows.contains(&("session.logout".to_owned(), "success".to_owned())));
        assert!(
            !rows
                .iter()
                .any(|(action, _)| action == "recovery.requested")
        );
    }

    #[tokio::test]
    async fn login_session_and_success_audit_commit_atomically() {
        let (app, repository, database) = application(CookieMode::secure()).await;
        repository
            .database()
            .execute(
                "CREATE TRIGGER reject_login_audit BEFORE INSERT ON audit_events \
                 WHEN NEW.action = 'authentication.login' AND NEW.outcome = 'success' \
                 BEGIN SELECT RAISE(ABORT, 'audit unavailable'); END",
            )
            .await
            .unwrap();
        let before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions")
            .fetch_one(database.pool())
            .await
            .unwrap();

        let response = app
            .oneshot(json_request(
                "/api/v1/auth/login",
                json!({"email":"owner@example.com","password":"correct horse battery"}),
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM sessions")
                .fetch_one(database.pool())
                .await
                .unwrap(),
            before
        );
    }

    #[tokio::test]
    async fn logout_revocation_and_success_audit_commit_atomically() {
        let (app, repository, _database) = application(CookieMode::secure()).await;
        let login = app
            .clone()
            .oneshot(json_request(
                "/api/v1/auth/login",
                json!({"email":"owner@example.com","password":"correct horse battery"}),
            ))
            .await
            .unwrap();
        let cookie = login
            .headers()
            .get(header::SET_COOKIE)
            .unwrap()
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();
        repository.database().execute("CREATE TRIGGER reject_logout_audit BEFORE INSERT ON audit_events WHEN NEW.action = 'session.logout' BEGIN SELECT RAISE(ABORT, 'audit unavailable'); END").await.unwrap();

        let logout = app
            .clone()
            .oneshot(cookie_request("POST", "/api/v1/auth/logout", &cookie))
            .await
            .unwrap();

        assert_eq!(logout.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(
            app.oneshot(cookie_request("GET", "/api/v1/auth/me", &cookie))
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
    }

    async fn application(
        mode: CookieMode,
    ) -> (axum::Router, Arc<IdentityRepository>, TestDatabase) {
        let database = TestDatabase::new().await.unwrap();
        let repository = Arc::new(IdentityRepository::new((*database).clone()));
        setup_repository(&repository).await;
        let state = AuthState::new(Arc::clone(&repository), mode);
        let app = auth_router(state).layer(HttpPlatformLayer::new(OriginPolicy::new(
            "https://orbit.test",
        )));
        (app, repository, database)
    }

    async fn setup_repository(repository: &IdentityRepository) {
        let now = TimestampMillis::now();
        repository
            .store_setup_token(
                "operator-secret",
                TimestampMillis::from_millis(now.as_millis() + 60_000),
            )
            .await
            .unwrap();
        repository
            .complete_setup(
                SetupRequest {
                    token: "operator-secret".to_owned(),
                    email: "Owner@Example.com".to_owned(),
                    display_name: "Owner".to_owned(),
                    password_hash: PasswordService::default()
                        .hash("correct horse battery")
                        .unwrap(),
                    workspace_name: "Orbit".to_owned(),
                    project_name: "General".to_owned(),
                },
                now,
            )
            .await
            .unwrap();
    }

    fn json_request(uri: &str, value: Value) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ORIGIN, "https://orbit.test")
            .body(Body::from(value.to_string()))
            .unwrap()
    }

    fn cookie_request(method: &str, uri: &str, cookie: &str) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .header(header::COOKIE, cookie)
            .header(header::ORIGIN, "https://orbit.test")
            .body(Body::empty())
            .unwrap()
    }

    async fn body(response: axum::response::Response) -> Vec<u8> {
        to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec()
    }
}
