//! Two-factor sign-in with an authenticator app (TOTP) and one-time recovery codes.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Mutex;

use axum::Json;
use axum::extract::{Extension, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use orbit_platform::{
    AuthenticatedUser, ClientIp, Id, RequestId, ThrottleDecision, TimestampMillis,
    generate_opaque_token,
};
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use serde_json::json;
use totp_rs::{Builder, Secret, Totp};
use utoipa::ToSchema;
use webauthn_rs::prelude::{DiscoverableAuthentication, PasskeyRegistration};

use super::{
    ApiError, ApiJson, AuthState, LoginResponse, ProblemBody, authenticate, request_id_value,
    signed_in_response,
};
use crate::audit::AuditOutcome;
use crate::repositories::identity::{AuthenticatedSession, IdentityError};
use crate::repositories::two_factor::TwoFactorStatus;
use crate::secret_box::{decrypt_secret, encrypt_secret};

/// How long a sign-in waits for its second factor, and a passkey ceremony for the browser.
const PENDING_LIFETIME_MILLIS: i64 = 5 * 60 * 1_000;
/// Pending entries kept at most. Passkey sign-in starts without an account, so this bounds the memory it can take.
const MAX_PENDING: usize = 10_000;
/// Wrong codes one sign-in may try before the password is needed again.
const MAX_CODE_ATTEMPTS: u8 = 5;

/// Something the server waits on between two requests. Kept in memory: a restart only means signing in again.
pub(super) enum Pending {
    /// The password was right; the session waits for a code.
    Login {
        user: AuthenticatedUser,
        password_hash: String,
        replacement_hash: Option<String>,
        attempts: u8,
    },
    PasskeyRegistration {
        user_id: Id,
        state: Box<PasskeyRegistration>,
    },
    PasskeyLogin {
        state: Box<DiscoverableAuthentication>,
    },
}

#[derive(Default)]
pub(super) struct PendingStore {
    entries: Mutex<HashMap<String, (TimestampMillis, Pending)>>,
}

impl PendingStore {
    /// Keeps `pending` under a new random token. `None` when the store is full.
    pub(super) fn insert(&self, pending: Pending) -> Option<String> {
        let now = TimestampMillis::now();
        let mut entries = self.entries.lock().expect("pending store mutex poisoned");
        entries.retain(|_, (expires_at, _)| *expires_at > now);
        if entries.len() >= MAX_PENDING {
            return None;
        }
        let token = generate_opaque_token();
        let expires_at = TimestampMillis::from_millis(now.as_millis() + PENDING_LIFETIME_MILLIS);
        entries.insert(token.clone(), (expires_at, pending));
        Some(token)
    }

    /// Removes and returns the entry, unless it expired. Each token is good for one try.
    pub(super) fn take(&self, token: &str) -> Option<(TimestampMillis, Pending)> {
        let mut entries = self.entries.lock().expect("pending store mutex poisoned");
        entries
            .remove(token)
            .filter(|(expires_at, _)| *expires_at > TimestampMillis::now())
    }

    /// Puts an entry back under its token with its first expiry, for another try.
    fn restore(&self, token: String, expires_at: TimestampMillis, pending: Pending) {
        self.entries
            .lock()
            .expect("pending store mutex poisoned")
            .insert(token, (expires_at, pending));
    }
}

/// The answer to a right password when the account has two-factor sign-in on.
#[derive(Serialize, ToSchema)]
pub(super) struct TwoFactorChallenge {
    /// Send it with the code to `/api/v1/auth/login/two-factor` within five minutes.
    two_factor_token: String,
}

pub(super) fn challenge(
    state: &AuthState,
    user: AuthenticatedUser,
    password_hash: String,
    replacement_hash: Option<String>,
    request_id: Option<&Extension<RequestId>>,
) -> Result<Response, ApiError> {
    let token = state
        .pending
        .insert(Pending::Login {
            user,
            password_hash,
            replacement_hash,
            attempts: 0,
        })
        .ok_or_else(|| busy("/api/v1/auth/login", request_id))?;
    Ok((
        StatusCode::ACCEPTED,
        Json(TwoFactorChallenge {
            two_factor_token: token,
        }),
    )
        .into_response())
}

pub(super) fn busy(instance: &'static str, request_id: Option<&Extension<RequestId>>) -> ApiError {
    ApiError::new(
        StatusCode::SERVICE_UNAVAILABLE,
        "authentication_busy",
        "Too many sign-ins in progress",
        "Too many sign-ins are in progress. Try again in a few minutes.",
        instance,
        request_id,
    )
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct SecondFactorBody {
    two_factor_token: String,
    /// The 6-digit code of the authenticator app, or a recovery code.
    code: String,
}

/// The second step of a sign-in with two-factor on: a code from the authenticator app or a recovery code.
#[utoipa::path(post, path = "/api/v1/auth/login/two-factor", request_body = SecondFactorBody, responses((status = 200, body = LoginResponse), (status = 400, description = "invalid_two_factor_token", body = ProblemBody, content_type = "application/problem+json"), (status = 401, description = "invalid_two_factor_code", body = ProblemBody, content_type = "application/problem+json"), (status = 429, description = "authentication_throttled", body = ProblemBody, content_type = "application/problem+json")))]
pub(super) async fn login_second_factor(
    State(state): State<AuthState>,
    client_ip: Option<Extension<ClientIp>>,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<SecondFactorBody>,
) -> Result<Response, ApiError> {
    let instance = "/api/v1/auth/login/two-factor";
    let expired = || {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_two_factor_token",
            "Sign-in expired",
            "This sign-in has expired. Enter your email and password again.",
            instance,
            request_id.as_ref(),
        )
    };
    let Some((
        expires_at,
        Pending::Login {
            user,
            password_hash,
            replacement_hash,
            attempts,
        },
    )) = state.pending.take(&body.two_factor_token)
    else {
        return Err(expired());
    };
    // Wrong codes count against their own key: a right password must not reset them.
    let ip = client_ip
        .map(|Extension(client_ip)| client_ip.0)
        .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
    let throttle_key = format!("two-factor:{}", user.email);
    let admission = state
        .throttler
        .lock()
        .expect("throttler mutex poisoned")
        .reserve(&throttle_key, ip, TimestampMillis::now());
    let reservation = match admission {
        Ok(reservation) => reservation,
        Err(ThrottleDecision::RetryAfter(delay)) => {
            state.pending.restore(
                body.two_factor_token,
                expires_at,
                Pending::Login {
                    user,
                    password_hash,
                    replacement_hash,
                    attempts,
                },
            );
            return Err(ApiError::throttled(delay, instance, request_id.as_ref()));
        }
        Err(ThrottleDecision::Allowed) => unreachable!("allowed admission returns a reservation"),
    };

    let accepted = check_code(
        &state,
        &user,
        &body.code,
        request_id_value(request_id.as_ref()),
    )
    .await
    .map_err(|_| ApiError::internal(instance, request_id.as_ref()))?;
    if !accepted {
        state
            .throttler
            .lock()
            .expect("throttler mutex poisoned")
            .finish_failure(reservation, TimestampMillis::now());
        state
            .repository
            .record_security_event(
                Some(user.id),
                "authentication.two_factor",
                AuditOutcome::Failure,
                "authentication",
                Some(user.id),
                request_id_value(request_id.as_ref()),
                json!({}),
                TimestampMillis::now(),
            )
            .await
            .map_err(|_| ApiError::internal(instance, request_id.as_ref()))?;
        if attempts + 1 < MAX_CODE_ATTEMPTS {
            state.pending.restore(
                body.two_factor_token,
                expires_at,
                Pending::Login {
                    user,
                    password_hash,
                    replacement_hash,
                    attempts: attempts + 1,
                },
            );
        }
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "invalid_two_factor_code",
            "Authentication failed",
            "The code is not correct.",
            instance,
            request_id.as_ref(),
        ));
    }
    state
        .throttler
        .lock()
        .expect("throttler mutex poisoned")
        .finish_success(reservation);

    let session = state
        .repository
        .create_session_audited(
            &user,
            &password_hash,
            replacement_hash.as_deref(),
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await;
    match session {
        Ok(session) => Ok(signed_in_response(&state, user, &session).await),
        // The password changed or the account was suspended after the first step.
        Err(IdentityError::InvalidCredential) => Err(expired()),
        Err(_) => Err(ApiError::internal(instance, request_id.as_ref())),
    }
}

/// Whether `code` is a fresh authenticator app code or an unused recovery code of the user. Uses it up.
async fn check_code(
    state: &AuthState,
    user: &AuthenticatedUser,
    code: &str,
    request_id: &str,
) -> Result<bool, IdentityError> {
    let code = code.trim().replace(' ', "");
    if code.len() == 6 && code.bytes().all(|byte| byte.is_ascii_digit()) {
        let Some(step) = totp_step(state, user, &code).await? else {
            return Ok(false);
        };
        return state.repository.use_totp_step(user.id, step).await;
    }
    state
        .repository
        .use_recovery_code(user.id, &code, request_id, TimestampMillis::now())
        .await
}

/// The time step `code` is right for, by the user's saved secret (enabled or still in setup).
async fn totp_step(
    state: &AuthState,
    user: &AuthenticatedUser,
    code: &str,
) -> Result<Option<i64>, IdentityError> {
    let Some(stored) = state.repository.totp(user.id).await? else {
        return Ok(None);
    };
    let Some(app_key) = state.app_key.as_ref() else {
        return Ok(None);
    };
    let secret =
        decrypt_secret(app_key, &stored.secret).map_err(|()| IdentityError::InvalidCredential)?;
    let Some(totp) = Secret::try_from_base32(&secret)
        .ok()
        .and_then(|secret| totp_for(secret, &user.email))
    else {
        return Ok(None);
    };
    let now = u64::try_from(TimestampMillis::now().as_millis() / 1_000).unwrap_or(0);
    Ok(totp
        .check(code, now)
        .and_then(|step| i64::try_from(step).ok())
        .filter(|step| *step > stored.last_used_step))
}

/// SHA-1, six digits, 30 seconds and one step of clock skew: what every authenticator app supports.
fn totp_for(secret: Secret, email: &str) -> Option<Totp> {
    Builder::new()
        .with_secret(secret)
        .with_issuer(Some("Orbit"))
        .with_account_name(email.replace(':', ""))
        .build()
        .ok()
}

#[utoipa::path(get, path = "/api/v1/auth/two-factor", responses((status = 200, body = TwoFactorStatus), (status = 401, description = "authentication_required", body = ProblemBody, content_type = "application/problem+json")))]
pub(super) async fn get_two_factor(
    State(state): State<AuthState>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<TwoFactorStatus>, ApiError> {
    let instance = "/api/v1/auth/two-factor";
    let session = authenticate(&state, &headers, instance, request_id.as_ref()).await?;
    state
        .repository
        .two_factor_status(session.user.id)
        .await
        .map(Json)
        .map_err(|_| ApiError::internal(instance, request_id.as_ref()))
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct PasswordConfirmationBody {
    /// The current password: changing how the account signs in asks for it again.
    password: String,
}

#[derive(Serialize, ToSchema)]
pub(super) struct TotpSetup {
    /// The secret in base32, for apps that cannot scan the QR code.
    secret: String,
    /// The `otpauth://` URL the QR code holds.
    otpauth_url: String,
    /// The QR code as a PNG `data:` URL.
    qr_code: String,
}

/// Starts the authenticator app setup: a new secret to scan. Two-factor sign-in stays off until a code from the app
/// confirms it.
#[utoipa::path(post, path = "/api/v1/auth/two-factor/totp/setup", request_body = PasswordConfirmationBody, responses((status = 200, body = TotpSetup), (status = 401, description = "authentication_required or invalid_credentials", body = ProblemBody, content_type = "application/problem+json"), (status = 409, description = "two_factor_enabled", body = ProblemBody, content_type = "application/problem+json"), (status = 429, description = "authentication_throttled", body = ProblemBody, content_type = "application/problem+json")))]
pub(super) async fn setup_totp(
    State(state): State<AuthState>,
    headers: HeaderMap,
    client_ip: Option<Extension<ClientIp>>,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<PasswordConfirmationBody>,
) -> Result<Json<TotpSetup>, ApiError> {
    let instance = "/api/v1/auth/two-factor/totp/setup";
    let session = authenticate(&state, &headers, instance, request_id.as_ref()).await?;
    confirm_password(
        &state,
        &session,
        body.password,
        client_ip,
        instance,
        request_id.as_ref(),
    )
    .await?;
    let app_key = state
        .app_key
        .ok_or_else(|| ApiError::internal(instance, request_id.as_ref()))?;
    let mut bytes = [0_u8; 20];
    OsRng.fill_bytes(&mut bytes);
    let secret = Secret::from(bytes.to_vec());
    let encoded = secret.to_base32();
    let totp = totp_for(secret, &session.user.email)
        .ok_or_else(|| ApiError::internal(instance, request_id.as_ref()))?;
    let encrypted = encrypt_secret(&app_key, &encoded)
        .map_err(|()| ApiError::internal(instance, request_id.as_ref()))?;
    let started = state
        .repository
        .begin_totp_setup(session.user.id, &encrypted, TimestampMillis::now())
        .await
        .map_err(|_| ApiError::internal(instance, request_id.as_ref()))?;
    if !started {
        return Err(already_enabled(instance, request_id.as_ref()));
    }
    let otpauth_url = totp
        .to_url()
        .map_err(|_| ApiError::internal(instance, request_id.as_ref()))?;
    let qr_code = totp
        .to_qr_base64()
        .map_err(|_| ApiError::internal(instance, request_id.as_ref()))?;
    Ok(Json(TotpSetup {
        secret: encoded,
        otpauth_url,
        qr_code: format!("data:image/png;base64,{qr_code}"),
    }))
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct EnableTotpBody {
    /// A code the authenticator app shows now.
    code: String,
}

#[derive(Serialize, ToSchema)]
pub(super) struct RecoveryCodes {
    /// Each signs in once in place of an app code. They are shown only now.
    recovery_codes: Vec<String>,
}

/// Turns two-factor sign-in on with the first code from the app, and returns the recovery codes.
#[utoipa::path(post, path = "/api/v1/auth/two-factor/totp/enable", request_body = EnableTotpBody, responses((status = 200, body = RecoveryCodes), (status = 401, description = "authentication_required", body = ProblemBody, content_type = "application/problem+json"), (status = 409, description = "two_factor_enabled or two_factor_setup_missing", body = ProblemBody, content_type = "application/problem+json"), (status = 422, description = "invalid_two_factor_code", body = ProblemBody, content_type = "application/problem+json")))]
pub(super) async fn enable_totp(
    State(state): State<AuthState>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<EnableTotpBody>,
) -> Result<Json<RecoveryCodes>, ApiError> {
    let instance = "/api/v1/auth/two-factor/totp/enable";
    let session = authenticate(&state, &headers, instance, request_id.as_ref()).await?;
    let stored = state
        .repository
        .totp(session.user.id)
        .await
        .map_err(|_| ApiError::internal(instance, request_id.as_ref()))?;
    match stored {
        Some(stored) if stored.enabled => {
            return Err(already_enabled(instance, request_id.as_ref()));
        }
        Some(_) => {}
        None => {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                "two_factor_setup_missing",
                "Setup not started",
                "Start the authenticator app setup again.",
                instance,
                request_id.as_ref(),
            ));
        }
    }
    let code = body.code.trim().replace(' ', "");
    let step = totp_step(&state, &session.user, &code)
        .await
        .map_err(|_| ApiError::internal(instance, request_id.as_ref()))?
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "invalid_two_factor_code",
                "Invalid code",
                "The code is not correct. Check the time on your device and try the newest code.",
                instance,
                request_id.as_ref(),
            )
        })?;
    let codes = state
        .repository
        .enable_totp(
            session.user.id,
            step,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map_err(|_| ApiError::internal(instance, request_id.as_ref()))?
        .ok_or_else(|| already_enabled(instance, request_id.as_ref()))?;
    Ok(Json(RecoveryCodes {
        recovery_codes: codes,
    }))
}

/// Turns two-factor sign-in off and deletes the recovery codes. Passkeys stay.
#[utoipa::path(post, path = "/api/v1/auth/two-factor/disable", request_body = PasswordConfirmationBody, responses((status = 204), (status = 401, description = "authentication_required or invalid_credentials", body = ProblemBody, content_type = "application/problem+json"), (status = 429, description = "authentication_throttled", body = ProblemBody, content_type = "application/problem+json")))]
pub(super) async fn disable_two_factor(
    State(state): State<AuthState>,
    headers: HeaderMap,
    client_ip: Option<Extension<ClientIp>>,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<PasswordConfirmationBody>,
) -> Result<StatusCode, ApiError> {
    let instance = "/api/v1/auth/two-factor/disable";
    let session = authenticate(&state, &headers, instance, request_id.as_ref()).await?;
    confirm_password(
        &state,
        &session,
        body.password,
        client_ip,
        instance,
        request_id.as_ref(),
    )
    .await?;
    state
        .repository
        .disable_totp(
            session.user.id,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map_err(|_| ApiError::internal(instance, request_id.as_ref()))?;
    Ok(StatusCode::NO_CONTENT)
}

/// New recovery codes in place of all the old ones.
#[utoipa::path(post, path = "/api/v1/auth/two-factor/recovery-codes", request_body = PasswordConfirmationBody, responses((status = 200, body = RecoveryCodes), (status = 401, description = "authentication_required or invalid_credentials", body = ProblemBody, content_type = "application/problem+json"), (status = 409, description = "two_factor_disabled", body = ProblemBody, content_type = "application/problem+json"), (status = 429, description = "authentication_throttled", body = ProblemBody, content_type = "application/problem+json")))]
pub(super) async fn regenerate_recovery_codes(
    State(state): State<AuthState>,
    headers: HeaderMap,
    client_ip: Option<Extension<ClientIp>>,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<PasswordConfirmationBody>,
) -> Result<Json<RecoveryCodes>, ApiError> {
    let instance = "/api/v1/auth/two-factor/recovery-codes";
    let session = authenticate(&state, &headers, instance, request_id.as_ref()).await?;
    confirm_password(
        &state,
        &session,
        body.password,
        client_ip,
        instance,
        request_id.as_ref(),
    )
    .await?;
    let codes = state
        .repository
        .regenerate_recovery_codes(
            session.user.id,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map_err(|_| ApiError::internal(instance, request_id.as_ref()))?
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::CONFLICT,
                "two_factor_disabled",
                "Two-factor sign-in is off",
                "Turn on two-factor sign-in first.",
                instance,
                request_id.as_ref(),
            )
        })?;
    Ok(Json(RecoveryCodes {
        recovery_codes: codes,
    }))
}

fn already_enabled(instance: &'static str, request_id: Option<&Extension<RequestId>>) -> ApiError {
    ApiError::new(
        StatusCode::CONFLICT,
        "two_factor_enabled",
        "Two-factor sign-in is on",
        "Two-factor sign-in is already on.",
        instance,
        request_id,
    )
}

/// Checks the signed-in user's password again before a change to how the account signs in. Wrong passwords count
/// toward the sign-in throttle, so a stolen session cannot guess it quickly.
pub(super) async fn confirm_password(
    state: &AuthState,
    session: &AuthenticatedSession,
    password: String,
    client_ip: Option<Extension<ClientIp>>,
    instance: &'static str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<(), ApiError> {
    let ip = client_ip
        .map(|Extension(client_ip)| client_ip.0)
        .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
    let reservation = state
        .throttler
        .lock()
        .expect("throttler mutex poisoned")
        .reserve(&session.user.email, ip, TimestampMillis::now())
        .map_err(|decision| match decision {
            ThrottleDecision::RetryAfter(delay) => ApiError::throttled(delay, instance, request_id),
            ThrottleDecision::Allowed => ApiError::internal(instance, request_id),
        })?;
    let identity = state
        .repository
        .find_by_email(&session.user.email)
        .await
        .map_err(|_| ApiError::internal(instance, request_id))?
        .ok_or_else(|| ApiError::internal(instance, request_id))?;
    let valid = state
        .passwords
        .verify(password, identity.password_hash)
        .await
        .map_err(|_| ApiError::internal(instance, request_id))?
        .valid;
    let mut throttler = state.throttler.lock().expect("throttler mutex poisoned");
    if valid {
        throttler.finish_success(reservation);
        return Ok(());
    }
    throttler.finish_failure(reservation, TimestampMillis::now());
    Err(ApiError::new(
        StatusCode::UNAUTHORIZED,
        "invalid_credentials",
        "Authentication failed",
        "The password is incorrect.",
        instance,
        request_id,
    ))
}
