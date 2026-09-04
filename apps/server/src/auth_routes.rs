use std::net::{IpAddr, Ipv4Addr};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Json;
use axum::Router;
use axum::extract::{Extension, Path, State};
use axum::http::header::{CONTENT_TYPE, COOKIE, RETRY_AFTER, SET_COOKIE};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use orbit_platform::{
    AuthenticatedUser, ClientIp, Id, LoginThrottler, OneTimeTokenStore, PasswordError,
    PasswordService, RequestId, ThrottleDecision, TimestampMillis, TokenKind,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::ToSchema;

use crate::repositories::identity::{IdentityRepository, SetupError, SetupRequest};

const SESSION_COOKIE: &str = "__Host-orbit_session";
const DEV_SESSION_COOKIE: &str = "orbit_session_dev";
const GENERIC_LOGIN_DETAIL: &str = "Email or password is incorrect.";
const GENERIC_RECOVERY_DETAIL: &str =
    "If the account exists, password recovery instructions will be provided.";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CookieMode {
    Secure,
    LoopbackDevelopment,
}

#[derive(Clone)]
pub struct AuthState {
    repository: Arc<IdentityRepository>,
    passwords: PasswordService,
    throttler: Arc<Mutex<LoginThrottler>>,
    recovery_tokens: Arc<OneTimeTokenStore>,
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
            passwords,
            throttler: Arc::new(Mutex::new(LoginThrottler::new())),
            recovery_tokens: Arc::new(OneTimeTokenStore::new()),
            cookie_mode,
            dummy_hash,
        }
    }
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

#[derive(Debug, Serialize, ToSchema)]
struct SetupStatus {
    complete: bool,
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

#[utoipa::path(post, path = "/api/v1/setup/complete", request_body = SetupBody, responses((status = 201)))]
async fn setup_complete(
    State(state): State<AuthState>,
    request_id: Option<Extension<RequestId>>,
    Json(body): Json<SetupBody>,
) -> Result<Response, ApiError> {
    let authenticated_email = body.email.trim().to_owned();
    let authenticated_display_name = body.display_name.clone();
    let password_hash = state
        .passwords
        .hash(&body.password)
        .map_err(|error| password_problem(error, "/api/v1/setup/complete", request_id.as_ref()))?;
    let result = state
        .repository
        .complete_setup(
            SetupRequest {
                token: body.token,
                email: body.email,
                display_name: body.display_name,
                password_hash,
                workspace_name: body.workspace_name,
                project_name: body.project_name,
            },
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
    let user = AuthenticatedUser {
        id: result.user_id,
        email: authenticated_email,
        display_name: authenticated_display_name,
    };
    let session = state
        .repository
        .create_session(&user, TimestampMillis::now())
        .await
        .map_err(|_| ApiError::internal("/api/v1/setup/complete", request_id.as_ref()))?;
    let mut response = (
        StatusCode::CREATED,
        Json(json!({
            "user_id": result.user_id,
            "workspace_id": result.workspace_id,
            "project_id": result.project_id,
            "session_id": session.id,
        })),
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

#[utoipa::path(post, path = "/api/v1/auth/login", request_body = LoginBody, responses((status = 200, body = LoginResponse), (status = 401), (status = 429)))]
async fn login(
    State(state): State<AuthState>,
    client_ip: Option<Extension<ClientIp>>,
    request_id: Option<Extension<RequestId>>,
    Json(body): Json<LoginBody>,
) -> Result<Response, ApiError> {
    let ip = client_ip
        .map(|Extension(client_ip)| client_ip.0)
        .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
    let decision = state
        .throttler
        .lock()
        .expect("throttler mutex poisoned")
        .check(&body.email, ip, TimestampMillis::now());
    if let ThrottleDecision::RetryAfter(delay) = decision {
        return Err(ApiError::throttled(
            delay,
            "/api/v1/auth/login",
            request_id.as_ref(),
        ));
    }

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
    let verification = state.passwords.verify(&body.password, hash).ok();
    let valid = identity
        .as_ref()
        .is_some_and(|identity| !identity.suspended)
        && verification.as_ref().is_some_and(|result| result.valid);
    if !valid {
        state
            .throttler
            .lock()
            .expect("throttler mutex poisoned")
            .record_failure(&body.email, ip, TimestampMillis::now());
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
    state
        .throttler
        .lock()
        .expect("throttler mutex poisoned")
        .record_success(&body.email);
    if let Some(replacement) = verification.and_then(|result| result.replacement_hash) {
        let _ = state
            .repository
            .update_password_hash(identity.id, &replacement, TimestampMillis::now())
            .await;
    }
    let user = AuthenticatedUser {
        id: identity.id,
        email: identity.email,
        display_name: identity.display_name,
    };
    let session = state
        .repository
        .create_session(&user, TimestampMillis::now())
        .await
        .map_err(|_| ApiError::internal("/api/v1/auth/login", request_id.as_ref()))?;
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

#[utoipa::path(post, path = "/api/v1/auth/logout", responses((status = 204), (status = 401)))]
async fn logout(
    State(state): State<AuthState>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Response, ApiError> {
    let session =
        authenticate(&state, &headers, "/api/v1/auth/logout", request_id.as_ref()).await?;
    let _ = state
        .repository
        .revoke_session(session.id, session.user.id, TimestampMillis::now())
        .await;
    let mut response = StatusCode::NO_CONTENT.into_response();
    response.headers_mut().insert(
        SET_COOKIE,
        HeaderValue::from_str(&session_cookie(state.cookie_mode, "", true)).unwrap(),
    );
    Ok(response)
}

#[utoipa::path(get, path = "/api/v1/auth/me", responses((status = 200), (status = 401)))]
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

#[utoipa::path(post, path = "/api/v1/auth/recovery/request", request_body = RecoveryRequestBody, responses((status = 202)))]
async fn recovery_request(
    State(state): State<AuthState>,
    Json(body): Json<RecoveryRequestBody>,
) -> Response {
    if let Ok(Some(identity)) = state.repository.find_by_email(&body.email).await
        && !identity.suspended
        && let Ok(issued) = state.recovery_tokens.issue(
            TokenKind::Recovery,
            identity.id.to_string(),
            TimestampMillis::now(),
            Duration::from_secs(30 * 60),
        )
    {
        let _ = state
            .repository
            .store_recovery_token(&identity.id.to_string(), &issued.token, issued.expires_at)
            .await;
    }
    (
        StatusCode::ACCEPTED,
        Json(json!({ "detail": GENERIC_RECOVERY_DETAIL })),
    )
        .into_response()
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct RecoveryCompleteBody {
    token: String,
    password: String,
}

#[utoipa::path(post, path = "/api/v1/auth/recovery/complete", request_body = RecoveryCompleteBody, responses((status = 204), (status = 400), (status = 422)))]
async fn recovery_complete(
    State(state): State<AuthState>,
    request_id: Option<Extension<RequestId>>,
    Json(body): Json<RecoveryCompleteBody>,
) -> Result<StatusCode, ApiError> {
    let password_hash = state.passwords.hash(&body.password).map_err(|error| {
        password_problem(error, "/api/v1/auth/recovery/complete", request_id.as_ref())
    })?;
    let user_id = state
        .repository
        .consume_recovery_token(&body.token, TimestampMillis::now())
        .await
        .map_err(|_| {
            ApiError::new(
                StatusCode::BAD_REQUEST,
                "invalid_recovery_token",
                "Recovery failed",
                "The recovery token is invalid or expired.",
                "/api/v1/auth/recovery/complete",
                request_id.as_ref(),
            )
        })?;
    state
        .repository
        .update_password_hash(user_id, &password_hash, TimestampMillis::now())
        .await
        .map_err(|_| ApiError::internal("/api/v1/auth/recovery/complete", request_id.as_ref()))?;
    state
        .repository
        .revoke_user_sessions(user_id, TimestampMillis::now())
        .await
        .map_err(|_| ApiError::internal("/api/v1/auth/recovery/complete", request_id.as_ref()))?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(get, path = "/api/v1/auth/sessions", responses((status = 200), (status = 401)))]
async fn list_sessions(
    State(state): State<AuthState>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<Vec<orbit_platform::SessionRecord>>, ApiError> {
    let user = authenticate(
        &state,
        &headers,
        "/api/v1/auth/sessions",
        request_id.as_ref(),
    )
    .await?
    .user;
    let sessions = state
        .repository
        .list_sessions(user.id, TimestampMillis::now())
        .await
        .map_err(|_| ApiError::internal("/api/v1/auth/sessions", request_id.as_ref()))?;
    Ok(Json(sessions))
}

#[utoipa::path(delete, path = "/api/v1/auth/sessions/{id}", params(("id" = String, Path)), responses((status = 204), (status = 401), (status = 404)))]
async fn revoke_session(
    State(state): State<AuthState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<StatusCode, ApiError> {
    let user = authenticate(
        &state,
        &headers,
        "/api/v1/auth/sessions/{id}",
        request_id.as_ref(),
    )
    .await?
    .user;
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
        .revoke_session(id, user.id, TimestampMillis::now())
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
    Ok(StatusCode::NO_CONTENT)
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
    match mode {
        CookieMode::Secure => SESSION_COOKIE,
        CookieMode::LoopbackDevelopment => DEV_SESSION_COOKIE,
    }
}

fn session_cookie(mode: CookieMode, token: &str, expired: bool) -> String {
    let mut cookie = format!("{}={token}; Path=/", cookie_name(mode));
    if mode == CookieMode::Secure {
        cookie.push_str("; Secure");
    }
    cookie.push_str("; HttpOnly; SameSite=Lax");
    if expired {
        cookie.push_str("; Max-Age=0");
    }
    cookie
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
struct ProblemBody {
    #[serde(rename = "type")]
    type_uri: String,
    title: &'static str,
    status: u16,
    code: &'static str,
    detail: &'static str,
    instance: &'static str,
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
        instance: &'static str,
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
                instance,
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

    use super::{AuthState, CookieMode, auth_router};
    use crate::repositories::identity::{IdentityRepository, SetupRequest};

    #[tokio::test]
    async fn auth_login_uses_a_host_only_secure_cookie() {
        let (app, _) = application(CookieMode::Secure).await;
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
        let app = auth_router(AuthState::new(repository, CookieMode::Secure)).layer(
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
        let (app, _) = application(CookieMode::LoopbackDevelopment).await;
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
        let (app, _) = application(CookieMode::Secure).await;
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
        let (app, _) = application(CookieMode::Secure).await;
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
        let (app, _) = application(CookieMode::Secure).await;
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
        let (app, repository) = application(CookieMode::Secure).await;
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

        let rebuilt = auth_router(AuthState::new(repository, CookieMode::Secure)).layer(
            HttpPlatformLayer::new(OriginPolicy::new("https://orbit.test")),
        );
        let me = rebuilt
            .oneshot(cookie_request("GET", "/api/v1/auth/me", &cookie))
            .await
            .unwrap();
        assert_eq!(me.status(), StatusCode::OK);
    }

    async fn application(mode: CookieMode) -> (axum::Router, Arc<IdentityRepository>) {
        let database = TestDatabase::new().await.unwrap();
        let repository = Arc::new(IdentityRepository::new((*database).clone()));
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
        let state = AuthState::new(Arc::clone(&repository), mode);
        let app = auth_router(state).layer(HttpPlatformLayer::new(OriginPolicy::new(
            "https://orbit.test",
        )));
        (app, repository)
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
