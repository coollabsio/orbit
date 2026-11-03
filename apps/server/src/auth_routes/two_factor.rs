//! Two-factor sign-in with an authenticator app (TOTP) and one-time recovery codes.

use std::net::{IpAddr, Ipv4Addr};

use axum::Json;
use axum::extract::{Extension, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use orbit_platform::{
    AuthenticatedUser, ClientIp, Database, Id, RequestId, Row, ThrottleDecision, TimestampMillis,
    generate_opaque_token,
};
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use totp_rs::{Builder, Secret, Totp};
use utoipa::ToSchema;
use webauthn_rs::prelude::{DiscoverableAuthentication, PasskeyRegistration};

use super::providers::ProviderFlow;
use super::{
    ApiError, ApiJson, AuthState, LoginResponse, ProblemBody, SessionDelivery, authenticate,
    request_id_value, signed_in_response,
};
use crate::audit::AuditOutcome;
use crate::repositories::identity::{AuthenticatedSession, IdentityError};
use crate::repositories::sign_in_providers::ProviderSignInError;
use crate::repositories::two_factor::TwoFactorStatus;
use crate::secret_box::{decrypt_secret, encrypt_secret};
use crate::sign_in_providers::{ProviderFailure, ProviderProfile};

/// How long a sign-in waits for its second factor, and a passkey ceremony for the browser.
const PENDING_LIFETIME_MILLIS: i64 = 5 * 60 * 1_000;
/// Pending entries kept at most. Passkey sign-in starts without an account, so this bounds the rows it can add.
const MAX_PENDING: usize = 10_000;
/// Wrong codes one sign-in may try before the password is needed again.
const MAX_CODE_ATTEMPTS: u8 = 5;
/// How long after a sign-in (or a provider confirmation) an account without a password may change how it signs in.
pub(super) const FRESH_SIGN_IN_MILLIS: i64 = 10 * 60 * 1_000;

/// Something the server waits on between two requests. Kept in the database, so the next request
/// can go to any server.
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum Pending {
    /// The first factor was right; the session waits for a code.
    Login {
        user: AuthenticatedUser,
        first_factor: FirstFactor,
        attempts: u8,
    },
    PasskeyRegistration {
        user_id: Id,
        state: Box<PasskeyRegistration>,
    },
    PasskeyLogin {
        state: Box<DiscoverableAuthentication>,
    },
    /// Keyed by the OAuth `state`: waiting for the provider to send the browser back.
    ProviderAuthorization {
        flow: ProviderFlow,
        /// Orbit's PKCE verifier for the provider leg. It never leaves the server.
        pkce_verifier: String,
    },
    /// Keyed by the one-time handoff code: the provider step is over, the client that started picks up the result.
    ProviderHandoff {
        flow: ProviderFlow,
        outcome: Result<ProviderProfile, ProviderFailure>,
    },
}

/// What the first step of a sign-in proved, so the second factor can finish it.
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum FirstFactor {
    Password {
        password_hash: String,
        replacement_hash: Option<String>,
    },
    /// `link`: the identity is new and is written only once the code passes, so a provider account that fails the
    /// second factor does not stay connected.
    Provider {
        profile: ProviderProfile,
        link: bool,
    },
}

pub(super) struct PendingStore {
    database: Database,
}

fn token_hash(token: &str) -> String {
    format!("{:x}", Sha256::digest(token.as_bytes()))
}

fn encode(pending: &Pending) -> Result<String, sqlx::Error> {
    serde_json::to_string(pending).map_err(|error| sqlx::Error::Encode(Box::new(error)))
}

impl PendingStore {
    pub(super) fn new(database: Database) -> Self {
        Self { database }
    }

    /// Keeps `pending` under a new random token for five minutes. `None` when the store is full.
    pub(super) async fn insert(&self, pending: Pending) -> Result<Option<String>, sqlx::Error> {
        self.insert_for(pending, PENDING_LIFETIME_MILLIS).await
    }

    /// [`Self::insert`] with its own lifetime.
    pub(super) async fn insert_for(
        &self,
        pending: Pending,
        lifetime_millis: i64,
    ) -> Result<Option<String>, sqlx::Error> {
        let payload = encode(&pending)?;
        let now = TimestampMillis::now();
        let mut transaction = self.database.immediate_transaction().await?;
        sqlx::query("DELETE FROM auth_pending WHERE expires_at <= $1")
            .bind(now.as_millis())
            .execute(&mut *transaction)
            .await?;
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM auth_pending")
            .fetch_one(&mut *transaction)
            .await?;
        if count >= i64::try_from(MAX_PENDING).unwrap_or(i64::MAX) {
            return Ok(None);
        }
        let token = generate_opaque_token();
        sqlx::query(
            "INSERT INTO auth_pending (token_hash, expires_at, payload_json) VALUES ($1, $2, $3)",
        )
        .bind(token_hash(&token))
        .bind(now.as_millis() + lifetime_millis)
        .bind(payload)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(Some(token))
    }

    /// Removes and returns the entry, unless it expired. Each token is good for one try, also
    /// when two servers get it at the same time.
    pub(super) async fn take(
        &self,
        token: &str,
    ) -> Result<Option<(TimestampMillis, Pending)>, sqlx::Error> {
        let mut transaction = self.database.immediate_transaction().await?;
        let row = sqlx::query(
            "DELETE FROM auth_pending WHERE token_hash = $1 RETURNING expires_at, payload_json",
        )
        .bind(token_hash(token))
        .fetch_optional(&mut *transaction)
        .await?;
        transaction.commit().await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let expires_at = TimestampMillis::from_millis(row.get("expires_at"));
        if expires_at <= TimestampMillis::now() {
            return Ok(None);
        }
        // An entry an older version wrote in another shape is as good as expired.
        Ok(serde_json::from_str(&row.get::<String, _>("payload_json"))
            .ok()
            .map(|pending| (expires_at, pending)))
    }

    /// Puts an entry back under its token with its first expiry, for another try.
    async fn restore(
        &self,
        token: &str,
        expires_at: TimestampMillis,
        pending: Pending,
    ) -> Result<(), sqlx::Error> {
        let payload = encode(&pending)?;
        let mut transaction = self.database.immediate_transaction().await?;
        sqlx::query(
            "INSERT INTO auth_pending (token_hash, expires_at, payload_json) VALUES ($1, $2, $3) \
             ON CONFLICT (token_hash) DO NOTHING",
        )
        .bind(token_hash(token))
        .bind(expires_at.as_millis())
        .bind(payload)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await
    }
}

/// The answer to a right password when the account has two-factor sign-in on.
#[derive(Serialize, ToSchema)]
pub(super) struct TwoFactorChallenge {
    /// Send it with the code to `/api/v1/auth/login/two-factor` within five minutes.
    two_factor_token: String,
}

pub(super) async fn challenge(
    state: &AuthState,
    user: AuthenticatedUser,
    first_factor: FirstFactor,
    instance: &'static str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<Response, ApiError> {
    let token = state
        .pending
        .insert(Pending::Login {
            user,
            first_factor,
            attempts: 0,
        })
        .await
        .map_err(|_| ApiError::internal(instance, request_id))?
        .ok_or_else(|| busy(instance, request_id))?;
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
    headers: HeaderMap,
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
            first_factor,
            attempts,
        },
    )) = state
        .pending
        .take(&body.two_factor_token)
        .await
        .map_err(|_| ApiError::internal(instance, request_id.as_ref()))?
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
        .reserve(&throttle_key, ip, TimestampMillis::now())
        .await;
    let reservation = match admission {
        Ok(reservation) => reservation,
        Err(ThrottleDecision::RetryAfter(delay)) => {
            state
                .pending
                .restore(
                    &body.two_factor_token,
                    expires_at,
                    Pending::Login {
                        user,
                        first_factor,
                        attempts,
                    },
                )
                .await
                .map_err(|_| ApiError::internal(instance, request_id.as_ref()))?;
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
            .finish_failure(reservation, TimestampMillis::now())
            .await;
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
            state
                .pending
                .restore(
                    &body.two_factor_token,
                    expires_at,
                    Pending::Login {
                        user,
                        first_factor,
                        attempts: attempts + 1,
                    },
                )
                .await
                .map_err(|_| ApiError::internal(instance, request_id.as_ref()))?;
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
    state.throttler.finish_success(reservation).await;

    let rid = request_id_value(request_id.as_ref());
    let session = match &first_factor {
        FirstFactor::Password {
            password_hash,
            replacement_hash,
        } => state
            .repository
            .create_session_audited(
                &user,
                password_hash,
                replacement_hash.as_deref(),
                rid,
                TimestampMillis::now(),
            )
            .await
            .map_err(|error| match error {
                // The password changed or the account was suspended after the first step.
                IdentityError::InvalidCredential => expired(),
                _ => ApiError::internal(instance, request_id.as_ref()),
            })?,
        FirstFactor::Provider { profile, link } => state
            .repository
            .create_provider_session_audited(&user, profile, *link, rid, TimestampMillis::now())
            .await
            .map_err(|error| match error {
                ProviderSignInError::Refused(_) => expired(),
                ProviderSignInError::Database(_) => {
                    ApiError::internal(instance, request_id.as_ref())
                }
            })?,
    };
    Ok(signed_in_response(
        &state,
        user,
        &session,
        SessionDelivery::for_request(&headers),
    )
    .await)
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
    /// The current password: changing how the account signs in asks for it again. Leave it out when the account has
    /// none: then the session must have signed in or confirmed with a provider in the last 10 minutes.
    password: Option<String>,
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
#[utoipa::path(post, path = "/api/v1/auth/two-factor/totp/setup", request_body = PasswordConfirmationBody, responses((status = 200, body = TotpSetup), (status = 401, description = "authentication_required, invalid_credentials, password_required or reauthentication_required", body = ProblemBody, content_type = "application/problem+json"), (status = 409, description = "two_factor_enabled", body = ProblemBody, content_type = "application/problem+json"), (status = 429, description = "authentication_throttled", body = ProblemBody, content_type = "application/problem+json")))]
pub(super) async fn setup_totp(
    State(state): State<AuthState>,
    headers: HeaderMap,
    client_ip: Option<Extension<ClientIp>>,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<PasswordConfirmationBody>,
) -> Result<Json<TotpSetup>, ApiError> {
    let instance = "/api/v1/auth/two-factor/totp/setup";
    let session = authenticate(&state, &headers, instance, request_id.as_ref()).await?;
    confirm_identity(
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
#[utoipa::path(post, path = "/api/v1/auth/two-factor/disable", request_body = PasswordConfirmationBody, responses((status = 204), (status = 401, description = "authentication_required, invalid_credentials, password_required or reauthentication_required", body = ProblemBody, content_type = "application/problem+json"), (status = 429, description = "authentication_throttled", body = ProblemBody, content_type = "application/problem+json")))]
pub(super) async fn disable_two_factor(
    State(state): State<AuthState>,
    headers: HeaderMap,
    client_ip: Option<Extension<ClientIp>>,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<PasswordConfirmationBody>,
) -> Result<StatusCode, ApiError> {
    let instance = "/api/v1/auth/two-factor/disable";
    let session = authenticate(&state, &headers, instance, request_id.as_ref()).await?;
    confirm_identity(
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
#[utoipa::path(post, path = "/api/v1/auth/two-factor/recovery-codes", request_body = PasswordConfirmationBody, responses((status = 200, body = RecoveryCodes), (status = 401, description = "authentication_required, invalid_credentials, password_required or reauthentication_required", body = ProblemBody, content_type = "application/problem+json"), (status = 409, description = "two_factor_disabled", body = ProblemBody, content_type = "application/problem+json"), (status = 429, description = "authentication_throttled", body = ProblemBody, content_type = "application/problem+json")))]
pub(super) async fn regenerate_recovery_codes(
    State(state): State<AuthState>,
    headers: HeaderMap,
    client_ip: Option<Extension<ClientIp>>,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<PasswordConfirmationBody>,
) -> Result<Json<RecoveryCodes>, ApiError> {
    let instance = "/api/v1/auth/two-factor/recovery-codes";
    let session = authenticate(&state, &headers, instance, request_id.as_ref()).await?;
    confirm_identity(
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

/// Checks that the person at this session is the account owner, before a change to how the account signs in.
///
/// An account with a password sends it again; wrong passwords count toward the sign-in throttle, so a stolen
/// session cannot guess it quickly. An account without one passes when the session signed in, or confirmed with a
/// provider, in the last 10 minutes; otherwise 401 `reauthentication_required` and the client runs a `confirm`
/// provider flow, then retries.
pub(super) async fn confirm_identity(
    state: &AuthState,
    session: &AuthenticatedSession,
    password: Option<String>,
    client_ip: Option<Extension<ClientIp>>,
    instance: &'static str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<(), ApiError> {
    let identity = state
        .repository
        .find_by_email(&session.user.email)
        .await
        .map_err(|_| ApiError::internal(instance, request_id))?
        .ok_or_else(|| ApiError::internal(instance, request_id))?;
    let Some(password_hash) = identity.password_hash else {
        let since =
            TimestampMillis::from_millis(TimestampMillis::now().as_millis() - FRESH_SIGN_IN_MILLIS);
        let fresh = state
            .repository
            .session_fresh_since(session.id, since)
            .await
            .map_err(|_| ApiError::internal(instance, request_id))?;
        if fresh {
            return Ok(());
        }
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "reauthentication_required",
            "Confirm it is you",
            "Sign in with your connected account again to confirm this change.",
            instance,
            request_id,
        ));
    };
    let Some(password) = password else {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "password_required",
            "Password required",
            "Enter your current password.",
            instance,
            request_id,
        ));
    };
    let ip = client_ip
        .map(|Extension(client_ip)| client_ip.0)
        .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
    let reservation = state
        .throttler
        .reserve(&session.user.email, ip, TimestampMillis::now())
        .await
        .map_err(|decision| match decision {
            ThrottleDecision::RetryAfter(delay) => ApiError::throttled(delay, instance, request_id),
            ThrottleDecision::Allowed => ApiError::internal(instance, request_id),
        })?;
    let valid = state
        .passwords
        .verify(password, password_hash)
        .await
        .map_err(|_| ApiError::internal(instance, request_id))?
        .valid;
    if valid {
        state.throttler.finish_success(reservation).await;
        return Ok(());
    }
    state
        .throttler
        .finish_failure(reservation, TimestampMillis::now())
        .await;
    Err(ApiError::new(
        StatusCode::UNAUTHORIZED,
        "invalid_credentials",
        "Authentication failed",
        "The password is incorrect.",
        instance,
        request_id,
    ))
}
