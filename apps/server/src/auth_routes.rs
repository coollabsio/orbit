use std::collections::BTreeMap;
use std::fmt;
use std::net::SocketAddr;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Json;
use axum::Router;
use axum::extract::{DefaultBodyLimit, Extension, FromRequest, Multipart, Path, Request, State};
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, COOKIE, RETRY_AFTER, SET_COOKIE};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use orbit_platform::{
    AuthenticatedUser, ClientIp, Id, LoginThrottler, PasswordError, PasswordExecutor,
    PasswordService, RequestId, ThrottleDecision, TimestampMillis,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::ToSchema;

use crate::audit::AuditOutcome;
use crate::live::LiveHub;
use crate::repositories::identity::{
    AuthenticatedSession, IdentityError, IdentityRepository, ProfileChanges, ProfileFields,
    SetupError, SetupRequest, UserStatus,
};

pub(crate) mod push;

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

    /// The session token a request carries, if any.
    pub(crate) fn session_token(self, headers: &HeaderMap) -> Option<&str> {
        let name = self.session_cookie_name();
        headers
            .get(COOKIE)?
            .to_str()
            .ok()?
            .split(';')
            .find_map(|pair| pair.trim().strip_prefix(name)?.strip_prefix('='))
            .filter(|token| !token.is_empty())
    }
}

/// The session a request carries. `None` when the cookie is missing or the session is expired,
/// revoked or belongs to a suspended user. Every cookie-authenticated route and socket uses this.
pub(crate) async fn request_session(
    identity: &IdentityRepository,
    cookie_mode: CookieMode,
    headers: &HeaderMap,
) -> Option<AuthenticatedSession> {
    let token = cookie_mode.session_token(headers)?;
    identity
        .authenticate_session(token, TimestampMillis::now())
        .await
        .ok()
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
        .route("/api/v1/auth/me", get(me).patch(update_me))
        .route("/api/v1/auth/status", put(put_status))
        .route("/api/v1/push/key", get(push::get_push_key))
        .route(
            "/api/v1/push/subscriptions",
            get(push::list_push_subscriptions).post(push::create_push_subscription),
        )
        .route(
            "/api/v1/push/subscriptions/{id}",
            delete(push::delete_push_subscription),
        )
        .route("/api/v1/push/test", post(push::send_test_push))
        .route(
            "/api/v1/notification-preferences",
            get(push::get_notification_preferences).put(push::put_notification_preferences),
        )
        .route(
            "/api/v1/auth/shortcuts",
            get(get_shortcuts).put(put_shortcuts),
        )
        .route(
            "/api/v1/auth/avatar",
            put(upload_avatar)
                .delete(remove_avatar)
                // the image plus the multipart framing
                .layer(DefaultBodyLimit::max(MAX_AVATAR_BYTES + 16 * 1024)),
        )
        .route("/api/v1/users/{user_id}/avatar", get(user_avatar))
        .route("/api/v1/auth/password", post(change_password))
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
        HeaderValue::from_str(&issued_session_cookie(
            state.cookie_mode,
            &session.token,
            session.absolute_expires_at,
        ))
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
    /// May manage backups, the global audit log and account suspension.
    installation_admin: bool,
    /// The profile picture; absent while the user has none. The URL changes with each upload.
    avatar_url: Option<String>,
    /// The presence and custom status the user set for themselves.
    status: UserStatus,
    #[serde(flatten)]
    profile: ProfileFields,
}

/// Where a user's profile picture is served. The version query lets browsers cache it for good.
pub(crate) fn avatar_url(user_id: Id, updated_at: Option<i64>) -> Option<String> {
    updated_at.map(|version| format!("/api/v1/users/{user_id}/avatar?v={version}"))
}

async fn user_response(
    state: &AuthState,
    user: AuthenticatedUser,
    instance: &'static str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<AuthUserResponse, ApiError> {
    let installation_admin = state
        .repository
        .is_installation_admin(user.id)
        .await
        .map_err(|_| ApiError::internal(instance, request_id))?;
    let avatar_updated_at = state
        .repository
        .avatar_updated_at(user.id)
        .await
        .map_err(|_| ApiError::internal(instance, request_id))?;
    let status = state
        .repository
        .status(user.id, TimestampMillis::now())
        .await
        .map_err(|_| ApiError::internal(instance, request_id))?;
    let profile = state
        .repository
        .profile_fields(user.id)
        .await
        .map_err(|_| ApiError::internal(instance, request_id))?;
    Ok(AuthUserResponse {
        status,
        profile,
        avatar_url: avatar_url(user.id, avatar_updated_at),
        id: user.id.to_string(),
        email: user.email,
        display_name: user.display_name,
        installation_admin,
    })
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
    // Nothing here may fail: the session exists, so the response must carry its cookie.
    // A failed avatar read only leaves the picture out.
    let avatar_updated_at = state
        .repository
        .avatar_updated_at(user.id)
        .await
        .ok()
        .flatten();
    let status = state
        .repository
        .status(user.id, TimestampMillis::now())
        .await
        .unwrap_or_default();
    let profile = state
        .repository
        .profile_fields(user.id)
        .await
        .unwrap_or_default();
    let mut response = Json(LoginResponse {
        user: AuthUserResponse {
            status,
            profile,
            avatar_url: avatar_url(user.id, avatar_updated_at),
            id: user.id.to_string(),
            email: user.email,
            display_name: user.display_name,
            installation_admin: identity.installation_admin,
        },
        session_id: session.id.to_string(),
    })
    .into_response();
    response.headers_mut().insert(
        SET_COOKIE,
        HeaderValue::from_str(&issued_session_cookie(
            state.cookie_mode,
            &session.token,
            session.absolute_expires_at,
        ))
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
        HeaderValue::from_str(&clear_session_cookie(state.cookie_mode)).unwrap(),
    );
    Ok(response)
}

#[utoipa::path(get, path = "/api/v1/auth/me", responses((status = 200, body = AuthUserResponse), (status = 401, description = "authentication_required", body = ProblemBody, content_type = "application/problem+json")))]
async fn me(
    State(state): State<AuthState>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Response, ApiError> {
    let session = authenticate(&state, &headers, "/api/v1/auth/me", request_id.as_ref()).await?;
    let token = state
        .cookie_mode
        .session_token(&headers)
        .expect("authenticate requires the session cookie")
        .to_owned();
    let mut response =
        Json(user_response(&state, session.user, "/api/v1/auth/me", request_id.as_ref()).await?)
            .into_response();
    // Rewrite the cookie on each profile read. A browser that still has the old
    // session cookie then stores it with an expiry before the process stops.
    response.headers_mut().insert(
        SET_COOKIE,
        HeaderValue::from_str(&issued_session_cookie(
            state.cookie_mode,
            &token,
            session.absolute_expires_at,
        ))
        .expect("session cookie is a valid header"),
    );
    Ok(response)
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct UpdateMeBody {
    display_name: String,
    /// The optional parts: absent leaves the part as it is, an empty text removes it.
    title: Option<String>,
    pronouns: Option<String>,
    timezone: Option<String>,
    bio: Option<String>,
    phone: Option<String>,
}

const MAX_PHONE_CHARS: usize = 32;
const MAX_TITLE_CHARS: usize = 80;
const MAX_PRONOUNS_CHARS: usize = 40;
const MAX_TIMEZONE_CHARS: usize = 64;
const MAX_BIO_CHARS: usize = 500;

/// The change a request asks for one part of the profile: none when the part is absent, its
/// removal when it is empty. `Err` when the text is too long or `shaped` says no.
fn profile_part(
    sent: Option<String>,
    max_chars: usize,
    shaped: impl Fn(&str) -> bool,
) -> Result<Option<Option<String>>, ()> {
    let Some(sent) = sent else {
        return Ok(None);
    };
    let sent = sent.trim();
    if sent.is_empty() {
        return Ok(Some(None));
    }
    if sent.chars().count() > max_chars || !shaped(sent) {
        return Err(());
    }
    Ok(Some(Some(sent.to_owned())))
}

/// A phone number as people write it: at least one digit, and digits, spaces and `+ - ( ) .`
/// only.
fn phone_shaped(phone: &str) -> bool {
    phone.chars().any(|c| c.is_ascii_digit())
        && phone
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, ' ' | '+' | '-' | '(' | ')' | '.'))
}

/// The shape of an IANA time zone name; the web app owns the list of names.
fn timezone_shaped(name: &str) -> bool {
    name.chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '_' | '+' | '-'))
}

#[utoipa::path(patch, path = "/api/v1/auth/me", request_body = UpdateMeBody, responses((status = 200, body = AuthUserResponse), (status = 401, description = "authentication_required", body = ProblemBody, content_type = "application/problem+json"), (status = 422, description = "invalid_display_name or invalid_profile", body = ProblemBody, content_type = "application/problem+json")))]
async fn update_me(
    State(state): State<AuthState>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<UpdateMeBody>,
) -> Result<Json<AuthUserResponse>, ApiError> {
    let instance = "/api/v1/auth/me";
    let session = authenticate(&state, &headers, instance, request_id.as_ref()).await?;
    let display_name = body.display_name.trim();
    if display_name.is_empty() {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_display_name",
            "Invalid display name",
            "Display name cannot be empty.",
            instance,
            request_id.as_ref(),
        ));
    }
    let display_name = display_name.to_owned();
    let any = |_: &str| true;
    let changes = (|| {
        Ok(ProfileChanges {
            title: profile_part(body.title, MAX_TITLE_CHARS, any)?,
            pronouns: profile_part(body.pronouns, MAX_PRONOUNS_CHARS, any)?,
            timezone: profile_part(body.timezone, MAX_TIMEZONE_CHARS, timezone_shaped)?,
            bio: profile_part(body.bio, MAX_BIO_CHARS, any)?,
            phone: profile_part(body.phone, MAX_PHONE_CHARS, phone_shaped)?,
        })
    })()
    .map_err(|(): ()| {
        ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_profile",
            "Invalid profile",
            "The title may have 80 characters, the pronouns 40, the time zone 64, the bio 500 and the phone number 32 (digits, spaces and + - ( ) . only).",
            instance,
            request_id.as_ref(),
        )
    })?;
    state
        .repository
        .update_profile_audited(
            session.user.id,
            &display_name,
            &changes,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map_err(|_| ApiError::internal(instance, request_id.as_ref()))?;
    let user = AuthenticatedUser {
        display_name,
        ..session.user
    };
    Ok(Json(
        user_response(&state, user, instance, request_id.as_ref()).await?,
    ))
}

const MAX_STATUS_EMOJI_CHARS: usize = 32;
const MAX_STATUS_TEXT_CHARS: usize = 100;

/// Trims the custom status and drops its empty parts; `None` when a part is too long or the
/// status ends in the past.
fn checked_status(status: UserStatus, now: TimestampMillis) -> Option<UserStatus> {
    let part = |value: Option<String>, max: usize| {
        let value = value.map(|value| value.trim().to_owned());
        match value {
            Some(value) if value.chars().count() > max => Err(()),
            Some(value) if value.is_empty() => Ok(None),
            value => Ok(value),
        }
    };
    let emoji = part(status.emoji, MAX_STATUS_EMOJI_CHARS).ok()?;
    let text = part(status.text, MAX_STATUS_TEXT_CHARS).ok()?;
    let custom = emoji.is_some() || text.is_some();
    if custom && status.expires_at.is_some_and(|end| end <= now) {
        return None;
    }
    Some(UserStatus {
        presence: status.presence,
        emoji,
        text,
        expires_at: status.expires_at.filter(|_| custom),
    })
}

#[utoipa::path(put, path = "/api/v1/auth/status", request_body = UserStatus, responses((status = 200, body = AuthUserResponse), (status = 401, description = "authentication_required", body = ProblemBody, content_type = "application/problem+json"), (status = 422, description = "invalid_status", body = ProblemBody, content_type = "application/problem+json")))]
async fn put_status(
    State(state): State<AuthState>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<UserStatus>,
) -> Result<Json<AuthUserResponse>, ApiError> {
    let instance = "/api/v1/auth/status";
    let session = authenticate(&state, &headers, instance, request_id.as_ref()).await?;
    let now = TimestampMillis::now();
    let status = checked_status(body, now).ok_or_else(|| {
        ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_status",
            "Invalid status",
            "The emoji may have 32 characters and the text 100; the end time must be in the future.",
            instance,
            request_id.as_ref(),
        )
    })?;
    state
        .repository
        .set_status(session.user.id, &status, now)
        .await
        .map_err(|_| ApiError::internal(instance, request_id.as_ref()))?;
    LiveHub::of(state.repository.database()).set_status(session.user.id, &status);
    Ok(Json(
        user_response(&state, session.user, instance, request_id.as_ref()).await?,
    ))
}

const MAX_SHORTCUT_BINDINGS: usize = 200;
const MAX_SHORTCUT_ID_CHARS: usize = 64;
const MAX_SHORTCUT_KEYS_CHARS: usize = 32;

/// Keyboard shortcut overrides: command id -> keys, or null for "no shortcut".
/// The web app owns the command ids and the key notation; the server only bounds the size.
#[derive(Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct ShortcutsBody {
    bindings: BTreeMap<String, Option<String>>,
}

fn shortcuts_within_limits(bindings: &BTreeMap<String, Option<String>>) -> bool {
    let within = |text: &str, max: usize| (1..=max).contains(&text.chars().count());
    bindings.len() <= MAX_SHORTCUT_BINDINGS
        && bindings.iter().all(|(id, keys)| {
            within(id, MAX_SHORTCUT_ID_CHARS)
                && keys
                    .as_deref()
                    .is_none_or(|keys| within(keys, MAX_SHORTCUT_KEYS_CHARS))
        })
}

#[utoipa::path(get, path = "/api/v1/auth/shortcuts", responses((status = 200, body = ShortcutsBody), (status = 401, description = "authentication_required", body = ProblemBody, content_type = "application/problem+json")))]
async fn get_shortcuts(
    State(state): State<AuthState>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<ShortcutsBody>, ApiError> {
    let instance = "/api/v1/auth/shortcuts";
    let session = authenticate(&state, &headers, instance, request_id.as_ref()).await?;
    let stored = state
        .repository
        .shortcut_bindings(session.user.id)
        .await
        .map_err(|_| ApiError::internal(instance, request_id.as_ref()))?;
    // a stored map that no longer parses reads as "no overrides": shortcuts must never lock a user out
    let bindings = stored
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default();
    Ok(Json(ShortcutsBody { bindings }))
}

#[utoipa::path(put, path = "/api/v1/auth/shortcuts", request_body = ShortcutsBody, responses((status = 200, body = ShortcutsBody), (status = 401, description = "authentication_required", body = ProblemBody, content_type = "application/problem+json"), (status = 422, description = "invalid_shortcuts", body = ProblemBody, content_type = "application/problem+json")))]
async fn put_shortcuts(
    State(state): State<AuthState>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<ShortcutsBody>,
) -> Result<Json<ShortcutsBody>, ApiError> {
    let instance = "/api/v1/auth/shortcuts";
    let session = authenticate(&state, &headers, instance, request_id.as_ref()).await?;
    if !shortcuts_within_limits(&body.bindings) {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_shortcuts",
            "Invalid shortcuts",
            "Send at most 200 shortcuts, with command ids of 1 to 64 characters and keys of 1 to 32 characters.",
            instance,
            request_id.as_ref(),
        ));
    }
    let json = serde_json::to_string(&body.bindings)
        .map_err(|_| ApiError::internal(instance, request_id.as_ref()))?;
    state
        .repository
        .set_shortcut_bindings(session.user.id, &json, TimestampMillis::now())
        .await
        .map_err(|_| ApiError::internal(instance, request_id.as_ref()))?;
    Ok(Json(body))
}

/// Largest accepted profile picture. The web app uploads a 256 px square, far below this.
const MAX_AVATAR_BYTES: usize = 512 * 1024;

/// The image type by its magic bytes; anything else (SVG included) is refused.
fn avatar_mime_type(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

#[derive(ToSchema)]
#[allow(dead_code)]
struct AvatarUploadBody {
    /// A PNG, JPEG or WebP image of at most 512 KiB.
    #[schema(value_type = String, format = Binary)]
    file: Vec<u8>,
}

#[derive(ToSchema)]
#[schema(value_type = String, format = Binary)]
#[allow(dead_code)]
struct AvatarImage(Vec<u8>);

#[utoipa::path(put, path = "/api/v1/auth/avatar", request_body(content = AvatarUploadBody, content_type = "multipart/form-data"), responses((status = 200, body = AuthUserResponse), (status = 400, description = "invalid_multipart", body = ProblemBody, content_type = "application/problem+json"), (status = 401, description = "authentication_required", body = ProblemBody, content_type = "application/problem+json"), (status = 413, description = "avatar_too_large", body = ProblemBody, content_type = "application/problem+json"), (status = 422, description = "invalid_avatar", body = ProblemBody, content_type = "application/problem+json")))]
async fn upload_avatar(
    State(state): State<AuthState>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    mut multipart: Multipart,
) -> Result<Json<AuthUserResponse>, ApiError> {
    let instance = "/api/v1/auth/avatar";
    let session = authenticate(&state, &headers, instance, request_id.as_ref()).await?;
    let invalid_multipart = || {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_multipart",
            "Invalid multipart upload",
            "Send the image as the multipart field \"file\".",
            instance,
            request_id.as_ref(),
        )
    };
    let too_large = || {
        ApiError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "avatar_too_large",
            "Image too large",
            "The image must be at most 512 KiB.",
            instance,
            request_id.as_ref(),
        )
    };
    let mut image = None;
    while let Some(field) = multipart.next_field().await.map_err(|error| {
        if error.status() == StatusCode::PAYLOAD_TOO_LARGE {
            too_large()
        } else {
            invalid_multipart()
        }
    })? {
        if field.name() == Some("file") {
            let bytes = field.bytes().await.map_err(|error| {
                if error.status() == StatusCode::PAYLOAD_TOO_LARGE {
                    too_large()
                } else {
                    invalid_multipart()
                }
            })?;
            image = Some(bytes);
        }
    }
    let image = image.ok_or_else(invalid_multipart)?;
    if image.len() > MAX_AVATAR_BYTES {
        return Err(too_large());
    }
    let mime_type = avatar_mime_type(&image).ok_or_else(|| {
        ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_avatar",
            "Invalid image",
            "Upload a PNG, JPEG or WebP image.",
            instance,
            request_id.as_ref(),
        )
    })?;
    state
        .repository
        .set_avatar_audited(
            session.user.id,
            mime_type,
            &image,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map_err(|_| ApiError::internal(instance, request_id.as_ref()))?;
    Ok(Json(
        user_response(&state, session.user, instance, request_id.as_ref()).await?,
    ))
}

#[utoipa::path(delete, path = "/api/v1/auth/avatar", responses((status = 200, body = AuthUserResponse), (status = 401, description = "authentication_required", body = ProblemBody, content_type = "application/problem+json")))]
async fn remove_avatar(
    State(state): State<AuthState>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<AuthUserResponse>, ApiError> {
    let instance = "/api/v1/auth/avatar";
    let session = authenticate(&state, &headers, instance, request_id.as_ref()).await?;
    state
        .repository
        .remove_avatar_audited(
            session.user.id,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map_err(|_| ApiError::internal(instance, request_id.as_ref()))?;
    Ok(Json(
        user_response(&state, session.user, instance, request_id.as_ref()).await?,
    ))
}

#[utoipa::path(get, path = "/api/v1/users/{user_id}/avatar", params(("user_id" = String, Path)), responses((status = 200, body = AvatarImage, content_type = "image/*"), (status = 401, description = "authentication_required", body = ProblemBody, content_type = "application/problem+json"), (status = 404, description = "avatar_not_found", body = ProblemBody, content_type = "application/problem+json")))]
async fn user_avatar(
    State(state): State<AuthState>,
    Path(user_id): Path<String>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Response, ApiError> {
    let instance = "/api/v1/users/{user_id}/avatar";
    let session = authenticate(&state, &headers, instance, request_id.as_ref()).await?;
    // a user without a picture and a user the caller shares no workspace with look the same
    let not_found = || {
        ApiError::new(
            StatusCode::NOT_FOUND,
            "avatar_not_found",
            "Avatar not found",
            "This user has no profile picture.",
            instance,
            request_id.as_ref(),
        )
    };
    let user_id = user_id.parse::<Id>().map_err(|_| not_found())?;
    let avatar = state
        .repository
        .avatar(session.user.id, user_id)
        .await
        .map_err(|_| ApiError::internal(instance, request_id.as_ref()))?
        .ok_or_else(not_found)?;
    let content_type = HeaderValue::from_str(&avatar.mime_type)
        .map_err(|_| ApiError::internal(instance, request_id.as_ref()))?;
    let mut response = avatar.bytes.into_response();
    let response_headers = response.headers_mut();
    response_headers.insert(CONTENT_TYPE, content_type);
    response_headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    // the URL carries the upload time, so a new picture is a new URL
    response_headers.insert(
        CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=31536000, immutable"),
    );
    Ok(response)
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct ChangePasswordBody {
    current_password: String,
    new_password: String,
}

#[utoipa::path(post, path = "/api/v1/auth/password", request_body = ChangePasswordBody, responses((status = 204), (status = 401, description = "invalid_credentials", body = ProblemBody, content_type = "application/problem+json"), (status = 422, description = "invalid_password", body = ProblemBody, content_type = "application/problem+json")))]
async fn change_password(
    State(state): State<AuthState>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<ChangePasswordBody>,
) -> Result<StatusCode, ApiError> {
    let instance = "/api/v1/auth/password";
    let session = authenticate(&state, &headers, instance, request_id.as_ref()).await?;
    let identity = state
        .repository
        .find_by_email(&session.user.email)
        .await
        .map_err(|_| ApiError::internal(instance, request_id.as_ref()))?
        .ok_or_else(|| ApiError::internal(instance, request_id.as_ref()))?;
    let verification = state
        .passwords
        .verify(body.current_password, identity.password_hash)
        .await
        .map_err(|_| ApiError::internal(instance, request_id.as_ref()))?;
    if !verification.valid {
        state
            .repository
            .record_security_event(
                Some(session.user.id),
                "account.password_changed",
                AuditOutcome::Failure,
                "user",
                Some(session.user.id),
                request_id_value(request_id.as_ref()),
                json!({}),
                TimestampMillis::now(),
            )
            .await
            .map_err(|_| ApiError::internal(instance, request_id.as_ref()))?;
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "invalid_credentials",
            "Authentication failed",
            "Current password is incorrect.",
            instance,
            request_id.as_ref(),
        ));
    }
    let password_hash = state
        .passwords
        .hash(body.new_password)
        .await
        .map_err(|error| password_problem(error, instance, request_id.as_ref()))?;
    state
        .repository
        .change_password_keeping_session(
            session.user.id,
            session.id,
            &password_hash,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map_err(|_| ApiError::internal(instance, request_id.as_ref()))?;
    Ok(StatusCode::NO_CONTENT)
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
            HeaderValue::from_str(&clear_session_cookie(state.cookie_mode))
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
) -> Result<AuthenticatedSession, ApiError> {
    request_session(&state.repository, state.cookie_mode, headers)
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

fn cookie_name(mode: CookieMode) -> &'static str {
    mode.session_cookie_name()
}

fn session_cookie(mode: CookieMode, token: &str, max_age_seconds: u64) -> String {
    let mut cookie = format!("{}={token}; Path=/", cookie_name(mode));
    if mode.secure {
        cookie.push_str("; Secure");
    }
    // No Max-Age makes a session cookie. Mobile Chrome deletes it when the
    // browser process stops. Desktop Chrome usually stays open, so the same
    // cookie appears to last. Keep the browser lifetime equal to the server
    // absolute session lifetime.
    cookie.push_str("; HttpOnly; SameSite=Lax");
    cookie.push_str(&format!("; Max-Age={max_age_seconds}"));
    cookie
}

fn max_age_until(expires_at: TimestampMillis) -> u64 {
    let remaining_ms = expires_at
        .as_millis()
        .saturating_sub(TimestampMillis::now().as_millis());
    let seconds = remaining_ms.saturating_add(999) / 1_000;
    u64::try_from(seconds).unwrap_or(u64::MAX)
}

pub(crate) fn issued_session_cookie(
    mode: CookieMode,
    token: &str,
    absolute_expires_at: TimestampMillis,
) -> String {
    session_cookie(mode, token, max_age_until(absolute_expires_at))
}

fn clear_session_cookie(mode: CookieMode) -> String {
    session_cookie(mode, "", 0)
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
    use crate::repositories::identity::{IdentityRepository, Presence, SetupRequest, UserStatus};

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
        assert_persistent_max_age(cookie);
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
        assert_persistent_max_age(response.headers()[header::SET_COOKIE].to_str().unwrap());
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
        assert_persistent_max_age(cookie);
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
        assert_persistent_max_age(me.headers()[header::SET_COOKIE].to_str().unwrap());
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
    async fn patch_me_updates_display_name_and_get_me_reflects_it() {
        let (app, _, database) = application(CookieMode::secure()).await;
        let cookie = login_cookie(&app, "correct horse battery").await;

        let patched = app
            .clone()
            .oneshot(json_cookie_request(
                "PATCH",
                "/api/v1/auth/me",
                &cookie,
                json!({"display_name": "  Ada Lovelace  "}),
            ))
            .await
            .unwrap();
        assert_eq!(patched.status(), StatusCode::OK);
        let patched: Value = serde_json::from_slice(&body(patched).await).unwrap();
        assert_eq!(patched["display_name"], "Ada Lovelace");
        assert_eq!(patched["installation_admin"], true);
        assert_eq!(patched["email"], "Owner@Example.com");
        let workspace_event: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM audit_events WHERE action = 'member.profile_updated' AND workspace_id IS NOT NULL",
        )
        .fetch_one(database.pool()).await.unwrap();
        assert_eq!(workspace_event, 1);

        let me = app
            .oneshot(cookie_request("GET", "/api/v1/auth/me", &cookie))
            .await
            .unwrap();
        assert_eq!(me.status(), StatusCode::OK);
        let me: Value = serde_json::from_slice(&body(me).await).unwrap();
        assert_eq!(me["display_name"], "Ada Lovelace");
        assert_eq!(me["email"], "Owner@Example.com");
    }

    #[tokio::test]
    async fn status_is_checked_stored_and_ends() {
        let (app, repository, _database) = application(CookieMode::secure()).await;
        let cookie = login_cookie(&app, "correct horse battery").await;
        let put = |value: Value| {
            app.clone().oneshot(json_cookie_request(
                "PUT",
                "/api/v1/auth/status",
                &cookie,
                value,
            ))
        };

        let me = app
            .clone()
            .oneshot(cookie_request("GET", "/api/v1/auth/me", &cookie))
            .await
            .unwrap();
        let me: Value = serde_json::from_slice(&body(me).await).unwrap();
        assert_eq!(
            me["status"],
            json!({ "presence": "online", "emoji": null, "text": null, "expires_at": null })
        );

        let too_long = put(json!({ "presence": "dnd", "emoji": null, "text": "x".repeat(101), "expires_at": null })).await.unwrap();
        assert_eq!(too_long.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let past = put(json!({ "presence": "dnd", "emoji": null, "text": "Lunch", "expires_at": "2020-01-01T00:00:00.000Z" })).await.unwrap();
        assert_eq!(past.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let unknown =
            put(json!({ "presence": "away", "emoji": null, "text": null, "expires_at": null }))
                .await
                .unwrap();
        assert_eq!(unknown.status(), StatusCode::BAD_REQUEST);

        // The text is trimmed; an end time without a custom status is dropped.
        let set = put(
            json!({ "presence": "dnd", "emoji": "🍜", "text": "  Lunch ", "expires_at": null }),
        )
        .await
        .unwrap();
        assert_eq!(set.status(), StatusCode::OK);
        let set: Value = serde_json::from_slice(&body(set).await).unwrap();
        assert_eq!(
            set["status"],
            json!({ "presence": "dnd", "emoji": "🍜", "text": "Lunch", "expires_at": null })
        );
        let bare = put(json!({ "presence": "invisible", "emoji": " ", "text": null, "expires_at": "2999-01-01T00:00:00.000Z" })).await.unwrap();
        let bare: Value = serde_json::from_slice(&body(bare).await).unwrap();
        assert_eq!(
            bare["status"],
            json!({ "presence": "invisible", "emoji": null, "text": null, "expires_at": null })
        );

        // A custom status that ended is gone; the presence stays.
        let owner: String = sqlx::query_scalar("SELECT id FROM users")
            .fetch_one(repository.database().pool())
            .await
            .unwrap();
        let owner: orbit_platform::Id = owner.parse().unwrap();
        let now = TimestampMillis::now();
        let later = TimestampMillis::from_millis(now.as_millis() + 60_000);
        let status = UserStatus {
            presence: Presence::Idle,
            emoji: None,
            text: Some("Lunch".to_owned()),
            expires_at: Some(later),
        };
        repository.set_status(owner, &status, now).await.unwrap();
        assert_eq!(repository.status(owner, now).await.unwrap(), status);
        assert_eq!(
            repository.status(owner, later).await.unwrap(),
            UserStatus {
                presence: Presence::Idle,
                ..UserStatus::default()
            }
        );
    }

    #[tokio::test]
    async fn avatar_upload_serve_and_remove() {
        let (app, repository, database) = application(CookieMode::secure()).await;
        let cookie = login_cookie(&app, "correct horse battery").await;
        let png = b"\x89PNG\r\n\x1a\nnot really pixels".to_vec();

        let svg = app
            .clone()
            .oneshot(avatar_request(&cookie, b"<svg onload=alert(1)/>"))
            .await
            .unwrap();
        assert_eq!(svg.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let too_large = app
            .clone()
            .oneshot(avatar_request(
                &cookie,
                &vec![0x89; super::MAX_AVATAR_BYTES + 1],
            ))
            .await
            .unwrap();
        assert_eq!(too_large.status(), StatusCode::PAYLOAD_TOO_LARGE);

        let uploaded = app
            .clone()
            .oneshot(avatar_request(&cookie, &png))
            .await
            .unwrap();
        assert_eq!(uploaded.status(), StatusCode::OK);
        let uploaded: Value = serde_json::from_slice(&body(uploaded).await).unwrap();
        let url = uploaded["avatar_url"].as_str().unwrap().to_owned();
        let profile_events: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM audit_events WHERE action = 'member.profile_updated' AND workspace_id IS NOT NULL",
        )
        .fetch_one(database.pool()).await.unwrap();
        assert_eq!(profile_events, 1);

        let me = app
            .clone()
            .oneshot(cookie_request("GET", "/api/v1/auth/me", &cookie))
            .await
            .unwrap();
        let me: Value = serde_json::from_slice(&body(me).await).unwrap();
        assert_eq!(me["avatar_url"], url.as_str());

        let image = app
            .clone()
            .oneshot(cookie_request("GET", &url, &cookie))
            .await
            .unwrap();
        assert_eq!(image.status(), StatusCode::OK);
        assert_eq!(image.headers()[header::CONTENT_TYPE], "image/png");
        assert_eq!(image.headers()["x-content-type-options"], "nosniff");
        assert_eq!(body(image).await, png);

        // only people who share a workspace with the owner see the picture
        let owner: orbit_platform::Id = me["id"].as_str().unwrap().parse().unwrap();
        let stranger = orbit_platform::Id::new_v7();
        sqlx::query(
            "INSERT INTO users (id, email, normalized_email, display_name, password_hash, created_at, updated_at) \
             VALUES (?, 'stranger@example.com', 'stranger@example.com', 'Stranger', 'x', 0, 0)",
        )
        .bind(stranger.to_string())
        .execute(database.pool())
        .await
        .unwrap();
        assert!(repository.avatar(stranger, owner).await.unwrap().is_none());
        assert!(repository.avatar(owner, owner).await.unwrap().is_some());

        let removed = app
            .clone()
            .oneshot(cookie_request("DELETE", "/api/v1/auth/avatar", &cookie))
            .await
            .unwrap();
        assert_eq!(removed.status(), StatusCode::OK);
        let removed: Value = serde_json::from_slice(&body(removed).await).unwrap();
        assert!(removed["avatar_url"].is_null());
        let gone = app
            .oneshot(cookie_request("GET", &url, &cookie))
            .await
            .unwrap();
        assert_eq!(gone.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn patch_me_rejects_empty_or_whitespace_display_name() {
        let (app, _, _database) = application(CookieMode::secure()).await;
        let cookie = login_cookie(&app, "correct horse battery").await;

        for display_name in ["", "   "] {
            let response = app
                .clone()
                .oneshot(json_cookie_request(
                    "PATCH",
                    "/api/v1/auth/me",
                    &cookie,
                    json!({ "display_name": display_name }),
                ))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
            assert_eq!(
                response.headers().get(header::CONTENT_TYPE).unwrap(),
                "application/problem+json"
            );
            let problem: Value = serde_json::from_slice(&body(response).await).unwrap();
            assert_eq!(problem["code"], "invalid_display_name");
        }

        let me = app
            .oneshot(cookie_request("GET", "/api/v1/auth/me", &cookie))
            .await
            .unwrap();
        let me: Value = serde_json::from_slice(&body(me).await).unwrap();
        assert_eq!(me["display_name"], "Owner");
    }

    #[tokio::test]
    async fn patch_me_requires_authentication() {
        let (app, _, _database) = application(CookieMode::secure()).await;
        let response = app
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri("/api/v1/auth/me")
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::ORIGIN, "https://orbit.test")
                    .body(Body::from(json!({"display_name":"Ada"}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let problem: Value = serde_json::from_slice(&body(response).await).unwrap();
        assert_eq!(problem["code"], "authentication_required");
    }

    #[tokio::test]
    async fn password_change_rejects_wrong_current_password_without_updating_hash() {
        let (app, repository, _database) = application(CookieMode::secure()).await;
        let cookie = login_cookie(&app, "correct horse battery").await;
        let before = repository
            .find_by_email("owner@example.com")
            .await
            .unwrap()
            .unwrap()
            .password_hash;

        let response = app
            .clone()
            .oneshot(json_cookie_request(
                "POST",
                "/api/v1/auth/password",
                &cookie,
                json!({
                    "current_password": "wrong password value",
                    "new_password": "brand new horse battery"
                }),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/problem+json"
        );
        let problem: Value = serde_json::from_slice(&body(response).await).unwrap();
        assert_eq!(problem["code"], "invalid_credentials");

        let after = repository
            .find_by_email("owner@example.com")
            .await
            .unwrap()
            .unwrap()
            .password_hash;
        assert_eq!(after, before);
    }

    #[tokio::test]
    async fn password_change_replaces_credentials_and_keeps_only_the_current_session() {
        let (app, _, database) = application(CookieMode::secure()).await;
        let current = login_cookie(&app, "correct horse battery").await;
        let other = login_cookie(&app, "correct horse battery").await;
        let current_password = "correct horse battery";
        let new_password = "brand new horse battery";

        let response = app
            .clone()
            .oneshot(json_cookie_request(
                "POST",
                "/api/v1/auth/password",
                &current,
                json!({
                    "current_password": current_password,
                    "new_password": new_password
                }),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);

        let old_login = app
            .clone()
            .oneshot(json_request(
                "/api/v1/auth/login",
                json!({"email":"owner@example.com","password": current_password}),
            ))
            .await
            .unwrap();
        assert_eq!(old_login.status(), StatusCode::UNAUTHORIZED);

        let new_login = app
            .clone()
            .oneshot(json_request(
                "/api/v1/auth/login",
                json!({"email":"owner@example.com","password": new_password}),
            ))
            .await
            .unwrap();
        assert_eq!(new_login.status(), StatusCode::OK);

        let other_me = app
            .clone()
            .oneshot(cookie_request("GET", "/api/v1/auth/me", &other))
            .await
            .unwrap();
        assert_eq!(other_me.status(), StatusCode::UNAUTHORIZED);

        let current_me = app
            .clone()
            .oneshot(cookie_request("GET", "/api/v1/auth/me", &current))
            .await
            .unwrap();
        assert_eq!(current_me.status(), StatusCode::OK);

        let rows = sqlx::query_as::<_, (String, String, String)>(
            "SELECT action, outcome, metadata_json FROM audit_events \
             WHERE action = 'account.password_changed'",
        )
        .fetch_all(database.pool())
        .await
        .unwrap();
        assert!(!rows.is_empty());
        for (action, outcome, metadata) in rows {
            assert_eq!(action, "account.password_changed");
            assert_eq!(outcome, "success");
            assert!(!metadata.contains(current_password));
            assert!(!metadata.contains(new_password));
            assert!(!metadata.contains("password_hash"));
            assert!(!metadata.contains("$argon2"));
        }
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

    fn json_cookie_request(method: &str, uri: &str, cookie: &str, value: Value) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::COOKIE, cookie)
            .header(header::ORIGIN, "https://orbit.test")
            .body(Body::from(value.to_string()))
            .unwrap()
    }

    fn assert_persistent_max_age(cookie: &str) {
        let max_age = cookie
            .split(';')
            .map(str::trim)
            .find_map(|part| part.strip_prefix("Max-Age="))
            .unwrap_or_else(|| panic!("session cookie has no Max-Age: {cookie}"))
            .parse::<u64>()
            .unwrap();
        let ninety_days = 90 * 24 * 60 * 60;
        assert!(
            (ninety_days - 60..=ninety_days).contains(&max_age),
            "Max-Age {max_age} is not the 90-day absolute session lifetime"
        );
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

    fn avatar_request(cookie: &str, image: &[u8]) -> Request<Body> {
        let mut multipart = b"--avatar\r\nContent-Disposition: form-data; name=\"file\"; filename=\"a.png\"\r\n\r\n".to_vec();
        multipart.extend_from_slice(image);
        multipart.extend_from_slice(b"\r\n--avatar--\r\n");
        Request::builder()
            .method("PUT")
            .uri("/api/v1/auth/avatar")
            .header(header::CONTENT_TYPE, "multipart/form-data; boundary=avatar")
            .header(header::COOKIE, cookie)
            .header(header::ORIGIN, "https://orbit.test")
            .body(Body::from(multipart))
            .unwrap()
    }

    async fn login_cookie(app: &axum::Router, password: &str) -> String {
        let login = app
            .clone()
            .oneshot(json_request(
                "/api/v1/auth/login",
                json!({"email":"owner@example.com","password": password}),
            ))
            .await
            .unwrap();
        assert_eq!(login.status(), StatusCode::OK);
        login
            .headers()
            .get(header::SET_COOKIE)
            .unwrap()
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned()
    }

    async fn body(response: axum::response::Response) -> Vec<u8> {
        to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec()
    }
}
