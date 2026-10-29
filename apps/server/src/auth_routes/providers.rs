//! Sign in with Google and GitHub: start, the provider's callback, and the exchange where Orbit decides.
//!
//! The callback never signs anyone in. It talks to the provider and parks the outcome behind a one-time handoff
//! code bound to the client that started (the S256 challenge of a verifier only that client holds). Web and native
//! clients both finish with `POST /api/v1/auth/providers/exchange`, which answers like `/api/v1/auth/login`.

use axum::Json;
use axum::extract::{Extension, Path, Query, State};
use axum::http::header::{CACHE_CONTROL, LOCATION, REFERRER_POLICY};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use orbit_platform::{ClientIp, Id, RequestId, TimestampMillis};
use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::{IntoParams, ToSchema};

use super::two_factor::{self, FirstFactor, Pending, busy};
use super::{
    ApiError, ApiJson, AuthState, LoginResponse, ProblemBody, SessionDelivery, authenticate,
    last_sign_in_method, request_id_value, signed_in_response,
};
use crate::audit::AuditOutcome;
use crate::repositories::instance_settings::InstanceSettingsRepository;
use crate::repositories::sign_in_providers::{
    ConnectedAccount, IdentityChangeError, ProviderSignInError, SavedSignInProvider, SignInRefusal,
};
use crate::secret_box::decrypt_secret;
use crate::sign_in_providers::{
    Freshness, Provider, ProviderCredentials, ProviderFailure, ProviderProfile, is_pkce_challenge,
    pkce_challenge, pkce_verifier,
};

/// Provider consent (with the provider's own sign-in and second factor) can take longer than five minutes.
const AUTHORIZATION_LIFETIME_MILLIS: i64 = 10 * 60 * 1_000;
/// The web page that reads the handoff code from the fragment and calls the exchange.
const WEB_RETURN_PATH: &str = "/login/callback";
const START: &str = "/api/v1/auth/providers/{provider}/start";
const EXCHANGE: &str = "/api/v1/auth/providers/exchange";

/// What the person asked for, fixed at start. The session ids make a connect or confirm land only on the session
/// that asked for it.
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum ProviderIntent {
    SignIn,
    Connect { session_id: Id, user_id: Id },
    Confirm { session_id: Id, user_id: Id },
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum ProviderRedirect {
    /// `{public_origin}/login/callback#handoff=…`
    Web,
    /// An allowlisted app URI: `{uri}?handoff=…`
    Native { uri: String },
}

/// A provider flow from start to exchange.
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct ProviderFlow {
    provider: Provider,
    intent: ProviderIntent,
    /// S256 of the verifier the starting client keeps; the exchange must bring the verifier.
    client_challenge: String,
    redirect: ProviderRedirect,
    return_to: Option<String>,
}

/// The configured providers whose secret this server can open. None without an app key, and none whose secret was
/// saved under another key, so the sign-in page never offers a button that cannot work.
pub(super) async fn configured(state: &AuthState) -> Result<Vec<Provider>, sqlx::Error> {
    let Some(app_key) = state.app_key.as_ref() else {
        return Ok(Vec::new());
    };
    Ok(InstanceSettingsRepository::new(state.database().clone())
        .sign_in_providers()
        .await?
        .into_iter()
        .filter_map(|saved| {
            let provider = saved.provider;
            open(app_key, saved).map(|_| provider)
        })
        .collect())
}

/// The decrypted OAuth client, or `None` when it is not configured or the app key cannot open the secret.
async fn credentials(
    state: &AuthState,
    provider: Provider,
) -> Result<Option<ProviderCredentials>, sqlx::Error> {
    let Some(app_key) = state.app_key.as_ref() else {
        return Ok(None);
    };
    let saved = InstanceSettingsRepository::new(state.database().clone())
        .sign_in_provider(provider)
        .await?;
    Ok(saved.and_then(|saved| open(app_key, saved)))
}

fn open(app_key: &[u8; 32], saved: SavedSignInProvider) -> Option<ProviderCredentials> {
    let client_secret = decrypt_secret(app_key, &saved.client_secret).ok()?;
    Some(ProviderCredentials {
        client_id: saved.client_id,
        client_secret,
    })
}

fn not_configured(instance: &'static str, request_id: Option<&Extension<RequestId>>) -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        "provider_not_configured",
        "Provider not available",
        "This sign-in provider is not set up on this server.",
        instance,
        request_id,
    )
}

fn unprocessable(
    code: &'static str,
    detail: &'static str,
    instance: &'static str,
    request_id: Option<&Extension<RequestId>>,
) -> ApiError {
    ApiError::new(
        StatusCode::UNPROCESSABLE_ENTITY,
        code,
        "Invalid provider sign-in",
        detail,
        instance,
        request_id,
    )
}

/// A path inside Orbit to open after the sign-in. Never another origin.
fn valid_return_to(value: &str) -> bool {
    value.len() <= 2048
        && value.starts_with('/')
        && !value.starts_with("//")
        && !value.contains('\\')
        && !value.chars().any(char::is_control)
}

/// `sign_in` needs no session. `connect` and `confirm` need one and apply only to it.
#[derive(Clone, Copy, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(super) enum SignInIntent {
    SignIn,
    Connect,
    Confirm,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct StartProviderBody {
    intent: SignInIntent,
    /// `base64url(sha256(code_verifier))` of a random verifier the client keeps (43 characters). The same verifier
    /// goes to the exchange.
    code_challenge: String,
    /// `web`, or a native redirect URI: `orbit://auth/callback` (development servers can allow more with
    /// `ORBIT_DEV_NATIVE_REDIRECTS`).
    redirect: String,
    /// A path in Orbit to open afterwards, given back with the handoff.
    return_to: Option<String>,
    /// `connect` only: the current password, since a connected account is a new way into the account. Leave it out
    /// when the account has none (see `reauthentication_required`).
    password: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub(super) struct StartProviderResponse {
    /// Open it in the browser (web: the same tab; mobile: an auth session).
    authorize_url: String,
}

/// Starts a provider flow. Problems: 404 `provider_not_configured`, 422 `invalid_code_challenge`,
/// `invalid_redirect` or `invalid_return_to`, 401 `authentication_required` for `connect` and `confirm`, and for
/// `connect` 401 `password_required`, `invalid_credentials` or `reauthentication_required` and 429
/// `authentication_throttled`.
#[utoipa::path(post, path = "/api/v1/auth/providers/{provider}/start", params(("provider" = Provider, Path)), request_body = StartProviderBody, responses((status = 200, body = StartProviderResponse), (status = 401, description = "authentication_required, invalid_credentials, password_required or reauthentication_required", body = ProblemBody, content_type = "application/problem+json"), (status = 404, description = "provider_not_configured", body = ProblemBody, content_type = "application/problem+json"), (status = 422, description = "invalid_code_challenge, invalid_redirect or invalid_return_to", body = ProblemBody, content_type = "application/problem+json"), (status = 429, description = "authentication_throttled", body = ProblemBody, content_type = "application/problem+json"), (status = 503, description = "authentication_busy", body = ProblemBody, content_type = "application/problem+json")))]
pub(super) async fn start_provider_sign_in(
    State(state): State<AuthState>,
    headers: HeaderMap,
    Path(provider): Path<String>,
    client_ip: Option<Extension<ClientIp>>,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<StartProviderBody>,
) -> Result<Json<StartProviderResponse>, ApiError> {
    let rid = request_id.as_ref();
    let provider = Provider::parse(&provider).ok_or_else(|| not_configured(START, rid))?;
    let credentials = credentials(&state, provider)
        .await
        .map_err(|_| ApiError::internal(START, rid))?
        .ok_or_else(|| not_configured(START, rid))?;
    if !is_pkce_challenge(&body.code_challenge) {
        return Err(unprocessable(
            "invalid_code_challenge",
            "code_challenge must be the base64url SHA-256 of the code verifier.",
            START,
            rid,
        ));
    }
    let redirect = match body.redirect.as_str() {
        "web" => ProviderRedirect::Web,
        uri if state.native_redirects.allows(uri) => ProviderRedirect::Native {
            uri: uri.to_owned(),
        },
        _ => {
            return Err(unprocessable(
                "invalid_redirect",
                "This redirect URI is not allowed.",
                START,
                rid,
            ));
        }
    };
    if body
        .return_to
        .as_deref()
        .is_some_and(|value| !valid_return_to(value))
    {
        return Err(unprocessable(
            "invalid_return_to",
            "return_to must be a path in Orbit.",
            START,
            rid,
        ));
    }
    let intent = match body.intent {
        SignInIntent::SignIn => ProviderIntent::SignIn,
        SignInIntent::Connect | SignInIntent::Confirm => {
            let session = authenticate(&state, &headers, START, rid).await?;
            let (session_id, user_id) = (session.id, session.user.id);
            if matches!(body.intent, SignInIntent::Connect) {
                two_factor::confirm_identity(
                    &state,
                    &session,
                    body.password,
                    client_ip,
                    START,
                    rid,
                )
                .await?;
                ProviderIntent::Connect {
                    session_id,
                    user_id,
                }
            } else {
                ProviderIntent::Confirm {
                    session_id,
                    user_id,
                }
            }
        }
    };
    let freshness = if matches!(intent, ProviderIntent::Confirm { .. }) {
        Freshness::ForceLogin
    } else {
        Freshness::Any
    };
    let verifier = pkce_verifier();
    let challenge = pkce_challenge(&verifier);
    let flow = ProviderFlow {
        provider,
        intent,
        client_challenge: body.code_challenge,
        redirect,
        return_to: body.return_to,
    };
    let state_token = state
        .pending
        .insert_for(
            Pending::ProviderAuthorization {
                flow,
                pkce_verifier: verifier,
            },
            AUTHORIZATION_LIFETIME_MILLIS,
        )
        .await
        .map_err(|_| ApiError::internal(START, rid))?
        .ok_or_else(|| busy(START, rid))?;
    let url = state.providers.authorization_url(
        provider,
        &credentials.client_id,
        &provider.callback_url(&state.public_origin),
        &state_token,
        &challenge,
        freshness,
    );
    Ok(Json(StartProviderResponse {
        authorize_url: url.into(),
    }))
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(super) struct CallbackQuery {
    state: Option<String>,
    code: Option<String>,
    error: Option<String>,
}

fn see_other(location: &str) -> Response {
    let mut response = StatusCode::SEE_OTHER.into_response();
    let headers = response.headers_mut();
    if let Ok(location) = HeaderValue::from_str(location) {
        headers.insert(LOCATION, location);
    }
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    response
}

/// The provider sends the browser here. Talks to the provider and parks the result behind a one-time handoff code;
/// never creates a session, a link or an account. Answers 303 to the client that started: web
/// `{public_origin}/login/callback#handoff=…`, native `{redirect}?handoff=…`, and `&return_to=…` when one was
/// given. An unknown, used or expired `state` answers 303 `/login?sign_in_error=provider_expired`.
#[utoipa::path(get, path = "/api/v1/auth/providers/{provider}/callback", params(("provider" = Provider, Path), CallbackQuery), responses((status = 303, description = "To the client that started the flow, with a handoff code.")))]
pub(super) async fn provider_callback(
    State(state): State<AuthState>,
    Path(provider): Path<String>,
    Query(query): Query<CallbackQuery>,
) -> Response {
    let expired = || {
        see_other(&format!(
            "{}/login?sign_in_error=provider_expired",
            state.public_origin
        ))
    };
    let Some(state_token) = query.state.filter(|token| token.len() <= 256) else {
        return expired();
    };
    let Ok(Some((
        _,
        Pending::ProviderAuthorization {
            flow,
            pkce_verifier,
        },
    ))) = state.pending.take(&state_token).await
    else {
        return expired();
    };
    if Provider::parse(&provider) != Some(flow.provider) {
        return expired();
    }
    let outcome = match (query.error, query.code) {
        (None, Some(code)) => match credentials(&state, flow.provider).await {
            Ok(Some(credentials)) => {
                state
                    .providers
                    .profile(
                        flow.provider,
                        &credentials,
                        &flow.provider.callback_url(&state.public_origin),
                        &code,
                        &pkce_verifier,
                    )
                    .await
            }
            _ => Err(ProviderFailure::Unavailable),
        },
        _ => Err(ProviderFailure::Denied),
    };
    let handoff = state
        .pending
        .insert(Pending::ProviderHandoff {
            flow: flow.clone(),
            outcome,
        })
        .await;
    let Ok(Some(handoff)) = handoff else {
        return expired();
    };
    let mut parameters = url::form_urlencoded::Serializer::new(String::new());
    parameters.append_pair("handoff", &handoff);
    if let Some(return_to) = &flow.return_to {
        parameters.append_pair("return_to", return_to);
    }
    let parameters = parameters.finish();
    match &flow.redirect {
        ProviderRedirect::Web => see_other(&format!(
            "{}{WEB_RETURN_PATH}#{parameters}",
            state.public_origin
        )),
        ProviderRedirect::Native { uri } => {
            let separator = if uri.contains('?') { '&' } else { '?' };
            see_other(&format!("{uri}{separator}{parameters}"))
        }
    }
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct ExchangeProviderBody {
    /// The one-time code from the callback redirect.
    handoff: String,
    /// The verifier whose S256 challenge went to start.
    code_verifier: String,
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .fold(0_u8, |difference, (a, b)| difference | (a ^ b))
            == 0
}

async fn record_failure(
    state: &AuthState,
    user_id: Option<Id>,
    provider: Provider,
    reason: &str,
    request_id: Option<&Extension<RequestId>>,
) {
    // The sign-in is refused either way; a failed audit write only loses the record.
    let _ = state
        .repository
        .record_security_event(
            user_id,
            "authentication.login",
            AuditOutcome::Failure,
            "authentication",
            user_id,
            request_id_value(request_id),
            json!({"method": provider.as_str(), "reason": reason}),
            TimestampMillis::now(),
        )
        .await;
}

fn refusal_problem(refusal: SignInRefusal, request_id: Option<&Extension<RequestId>>) -> ApiError {
    let (status, code, title, detail) = match refusal {
        SignInRefusal::EmailUnverified => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "provider_email_unverified",
            "Email not verified",
            "The provider has not verified the email address of this account. Verify it there, or sign in another way.",
        ),
        SignInRefusal::RegistrationClosed => (
            StatusCode::FORBIDDEN,
            "registration_closed",
            "Registration closed",
            "There is no Orbit account for this email, and this server does not accept new accounts.",
        ),
        SignInRefusal::IdentityConflict => (
            StatusCode::CONFLICT,
            "identity_conflict",
            "Another account is connected",
            "The Orbit account with this email is connected to another account at this provider.",
        ),
        SignInRefusal::Suspended => (
            StatusCode::FORBIDDEN,
            "account_suspended",
            "Account suspended",
            "This account is suspended.",
        ),
        SignInRefusal::Stale => return invalid_handoff(request_id),
    };
    ApiError::new(status, code, title, detail, EXCHANGE, request_id)
}

fn refusal_reason(refusal: SignInRefusal) -> &'static str {
    match refusal {
        SignInRefusal::EmailUnverified => "email_unverified",
        SignInRefusal::RegistrationClosed => "registration_closed",
        SignInRefusal::IdentityConflict => "identity_conflict",
        SignInRefusal::Suspended => "suspended",
        SignInRefusal::Stale => "stale",
    }
}

fn invalid_handoff(request_id: Option<&Extension<RequestId>>) -> ApiError {
    ApiError::new(
        StatusCode::BAD_REQUEST,
        "invalid_handoff",
        "Sign-in expired",
        "This sign-in has expired or was already used. Start again.",
        EXCHANGE,
        request_id,
    )
}

/// Where every provider flow ends, for every client and intent. `sign_in`: 200 `LoginResponse` (a cookie for
/// browsers, `session_token` for requests without Origin) or 202 two-factor challenge, the same answers as
/// `/api/v1/auth/login`. `connect` and `confirm`: 204.
#[utoipa::path(post, path = "/api/v1/auth/providers/exchange", request_body = ExchangeProviderBody, responses((status = 200, body = LoginResponse), (status = 202, description = "The account has two-factor sign-in on: send a code to /api/v1/auth/login/two-factor.", body = two_factor::TwoFactorChallenge), (status = 204, description = "connect or confirm finished"), (status = 400, description = "invalid_handoff or provider_denied", body = ProblemBody, content_type = "application/problem+json"), (status = 401, description = "authentication_required", body = ProblemBody, content_type = "application/problem+json"), (status = 403, description = "registration_closed, account_suspended, session_mismatch or identity_mismatch", body = ProblemBody, content_type = "application/problem+json"), (status = 409, description = "identity_in_use, identity_conflict or provider_already_connected", body = ProblemBody, content_type = "application/problem+json"), (status = 422, description = "provider_email_unverified", body = ProblemBody, content_type = "application/problem+json"), (status = 502, description = "provider_failed", body = ProblemBody, content_type = "application/problem+json"), (status = 503, description = "authentication_busy", body = ProblemBody, content_type = "application/problem+json")))]
pub(super) async fn exchange_provider_sign_in(
    State(state): State<AuthState>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<ExchangeProviderBody>,
) -> Result<Response, ApiError> {
    let rid = request_id.as_ref();
    let Some((_, Pending::ProviderHandoff { flow, outcome })) = state
        .pending
        .take(&body.handoff)
        .await
        .map_err(|_| ApiError::internal(EXCHANGE, rid))?
    else {
        return Err(invalid_handoff(rid));
    };
    if !constant_time_eq(
        pkce_challenge(&body.code_verifier).as_bytes(),
        flow.client_challenge.as_bytes(),
    ) {
        return Err(invalid_handoff(rid));
    }
    let provider = flow.provider;
    let profile = match outcome {
        Ok(profile) => profile,
        Err(ProviderFailure::Denied) => {
            record_failure(&state, None, provider, "denied", rid).await;
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "provider_denied",
                "Sign-in cancelled",
                "The sign-in was cancelled at the provider.",
                EXCHANGE,
                rid,
            ));
        }
        Err(ProviderFailure::Unavailable) => {
            record_failure(&state, None, provider, "provider_failed", rid).await;
            return Err(ApiError::new(
                StatusCode::BAD_GATEWAY,
                "provider_failed",
                "Provider unavailable",
                "The provider did not confirm the sign-in. Try again in a moment.",
                EXCHANGE,
                rid,
            ));
        }
    };
    match flow.intent {
        ProviderIntent::SignIn => sign_in(&state, &headers, profile, rid).await,
        ProviderIntent::Connect {
            session_id,
            user_id,
        } => {
            same_session(&state, &headers, session_id, rid).await?;
            state
                .repository
                .connect_identity(
                    user_id,
                    &profile,
                    request_id_value(rid),
                    TimestampMillis::now(),
                )
                .await
                .map_err(|error| identity_change_problem(error, rid))?;
            Ok(StatusCode::NO_CONTENT.into_response())
        }
        ProviderIntent::Confirm {
            session_id,
            user_id,
        } => {
            same_session(&state, &headers, session_id, rid).await?;
            state
                .repository
                .confirm_with_provider(
                    session_id,
                    user_id,
                    &profile,
                    request_id_value(rid),
                    TimestampMillis::now(),
                )
                .await
                .map_err(|error| identity_change_problem(error, rid))?;
            Ok(StatusCode::NO_CONTENT.into_response())
        }
    }
}

async fn sign_in(
    state: &AuthState,
    headers: &HeaderMap,
    profile: ProviderProfile,
    rid: Option<&Extension<RequestId>>,
) -> Result<Response, ApiError> {
    let provider = profile.provider;
    let refused = |refusal, user_id| async move {
        record_failure(state, user_id, provider, refusal_reason(refusal), rid).await;
        refusal_problem(refusal, rid)
    };
    let account = match state
        .repository
        .provider_account(&profile, request_id_value(rid), TimestampMillis::now())
        .await
    {
        Ok(account) => account,
        Err(ProviderSignInError::Refused(refusal)) => return Err(refused(refusal, None).await),
        Err(ProviderSignInError::Database(_)) => return Err(ApiError::internal(EXCHANGE, rid)),
    };
    let totp_enabled = state
        .repository
        .two_factor_status(account.user.id)
        .await
        .map_err(|_| ApiError::internal(EXCHANGE, rid))?
        .totp_enabled;
    if totp_enabled {
        return two_factor::challenge(
            state,
            account.user,
            FirstFactor::Provider {
                profile,
                link: account.link,
            },
            EXCHANGE,
            rid,
        )
        .await;
    }
    let session = match state
        .repository
        .create_provider_session_audited(
            &account.user,
            &profile,
            account.link,
            request_id_value(rid),
            TimestampMillis::now(),
        )
        .await
    {
        Ok(session) => session,
        Err(ProviderSignInError::Refused(refusal)) => {
            return Err(refused(refusal, Some(account.user.id)).await);
        }
        Err(ProviderSignInError::Database(_)) => return Err(ApiError::internal(EXCHANGE, rid)),
    };
    Ok(signed_in_response(
        state,
        account.user,
        &session,
        SessionDelivery::for_request(headers),
    )
    .await)
}

/// A connect or confirm result applies only to the session that started it.
async fn same_session(
    state: &AuthState,
    headers: &HeaderMap,
    session_id: Id,
    rid: Option<&Extension<RequestId>>,
) -> Result<(), ApiError> {
    let session = authenticate(state, headers, EXCHANGE, rid).await?;
    if session.id != session_id {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "session_mismatch",
            "Different session",
            "Finish the connection in the window where you started it.",
            EXCHANGE,
            rid,
        ));
    }
    Ok(())
}

fn identity_change_problem(
    error: IdentityChangeError,
    rid: Option<&Extension<RequestId>>,
) -> ApiError {
    let (status, code, title, detail) = match error {
        IdentityChangeError::InUse => (
            StatusCode::CONFLICT,
            "identity_in_use",
            "Account in use",
            "This provider account is connected to another Orbit account.",
        ),
        IdentityChangeError::AlreadyConnected => (
            StatusCode::CONFLICT,
            "provider_already_connected",
            "Already connected",
            "Another account at this provider is already connected. Disconnect it first.",
        ),
        IdentityChangeError::Mismatch | IdentityChangeError::NotConnected => (
            StatusCode::FORBIDDEN,
            "identity_mismatch",
            "Different account",
            "Sign in with the provider account that is connected to your Orbit account.",
        ),
        IdentityChangeError::LastSignInMethod => {
            return last_sign_in_method(EXCHANGE, rid);
        }
        IdentityChangeError::Database(_) => return ApiError::internal(EXCHANGE, rid),
    };
    ApiError::new(status, code, title, detail, EXCHANGE, rid)
}

/// The provider accounts connected to the signed-in account.
#[utoipa::path(get, path = "/api/v1/auth/connected-accounts", responses((status = 200, body = Vec<ConnectedAccount>), (status = 401, description = "authentication_required", body = ProblemBody, content_type = "application/problem+json")))]
pub(super) async fn list_connected_accounts(
    State(state): State<AuthState>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<Vec<ConnectedAccount>>, ApiError> {
    let instance = "/api/v1/auth/connected-accounts";
    let session = authenticate(&state, &headers, instance, request_id.as_ref()).await?;
    state
        .repository
        .connected_accounts(session.user.id)
        .await
        .map(Json)
        .map_err(|_| ApiError::internal(instance, request_id.as_ref()))
}

/// Disconnects a provider. Refused with 409 `last_sign_in_method` when it is the account's only way in.
#[utoipa::path(delete, path = "/api/v1/auth/connected-accounts/{provider}", params(("provider" = Provider, Path)), responses((status = 204), (status = 401, description = "authentication_required", body = ProblemBody, content_type = "application/problem+json"), (status = 404, description = "identity_not_found", body = ProblemBody, content_type = "application/problem+json"), (status = 409, description = "last_sign_in_method", body = ProblemBody, content_type = "application/problem+json")))]
pub(super) async fn disconnect_connected_account(
    State(state): State<AuthState>,
    headers: HeaderMap,
    Path(provider): Path<String>,
    request_id: Option<Extension<RequestId>>,
) -> Result<StatusCode, ApiError> {
    let instance = "/api/v1/auth/connected-accounts/{provider}";
    let rid = request_id.as_ref();
    let session = authenticate(&state, &headers, instance, rid).await?;
    let not_found = || {
        ApiError::new(
            StatusCode::NOT_FOUND,
            "identity_not_found",
            "Not connected",
            "This provider is not connected to your account.",
            instance,
            rid,
        )
    };
    let provider = Provider::parse(&provider).ok_or_else(not_found)?;
    match state
        .repository
        .disconnect_identity(
            session.user.id,
            provider,
            request_id_value(rid),
            TimestampMillis::now(),
        )
        .await
    {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(IdentityChangeError::NotConnected) => Err(not_found()),
        Err(IdentityChangeError::LastSignInMethod) => Err(last_sign_in_method(instance, rid)),
        Err(_) => Err(ApiError::internal(instance, rid)),
    }
}

#[cfg(test)]
mod tests {
    use super::valid_return_to;

    #[test]
    fn return_to_stays_inside_orbit() {
        assert!(valid_return_to("/oauth/authorize?client_id=x"));
        for refused in [
            "https://evil.example",
            "//evil.example",
            "/\\evil",
            "evil",
            "/a\nb",
        ] {
            assert!(!valid_return_to(refused), "{refused}");
        }
    }
}
