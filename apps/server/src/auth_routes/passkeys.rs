//! Passkeys (WebAuthn): adding them in the account settings and signing in with one.

use axum::Json;
use axum::extract::{Extension, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use orbit_platform::{ClientIp, Id, RequestId, TimestampMillis};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use utoipa::ToSchema;
use webauthn_rs::prelude::{
    DiscoverableKey, PublicKeyCredential, RegisterPublicKeyCredential, Url, Uuid,
};
use webauthn_rs::{Webauthn, WebauthnBuilder};

use super::two_factor::{Pending, busy, confirm_password};
use super::{
    ApiError, ApiJson, AuthState, LoginResponse, ProblemBody, authenticate, request_id_value,
    signed_in_response,
};
use crate::audit::AuditOutcome;
use crate::repositories::identity::IdentityError;
use crate::repositories::two_factor::{PasskeyRecord, PasskeySaveError};

/// Passkeys for `public_origin`, or `None` when its host is not a domain name (browsers refuse passkeys for IP
/// addresses).
pub(super) fn relying_party(public_origin: &str) -> Option<Webauthn> {
    let origin = Url::parse(public_origin).ok()?;
    let host = origin.domain()?.to_owned();
    WebauthnBuilder::new(&host, &origin)
        .ok()?
        .rp_name("Orbit")
        .build()
        .ok()
}

/// The browser options of a passkey ceremony and the token to finish it with.
#[derive(Serialize, ToSchema)]
pub(super) struct PasskeyChallenge {
    /// Send it back with the browser's answer within five minutes.
    challenge_token: String,
    /// `{ publicKey: … }` in the WebAuthn JSON form (`PublicKeyCredential.parse…OptionsFromJSON`).
    #[schema(value_type = Object)]
    options: Value,
}

fn unavailable(instance: &'static str, request_id: Option<&Extension<RequestId>>) -> ApiError {
    ApiError::new(
        StatusCode::CONFLICT,
        "passkeys_unavailable",
        "Passkeys are not available",
        "Passkeys need Orbit to be opened at a domain name, not an IP address.",
        instance,
        request_id,
    )
}

fn invalid_passkey(instance: &'static str, request_id: Option<&Extension<RequestId>>) -> ApiError {
    ApiError::new(
        StatusCode::BAD_REQUEST,
        "invalid_passkey",
        "Passkey not accepted",
        "The passkey could not be verified. Try again.",
        instance,
        request_id,
    )
}

/// Drops the extensions webauthn-rs adds for hardware keys: credProtect and the deprecated `uvm`. Password managers
/// such as Enpass do not support them and hand such a request over to the browser's own passkeys (iCloud Keychain).
/// Neither is needed: user verification is required anyway, and nothing reads their results.
fn keep_standard_extensions(options: &mut Value) {
    if let Some(extensions) = options
        .pointer_mut("/publicKey/extensions")
        .and_then(Value::as_object_mut)
    {
        for name in [
            "credentialProtectionPolicy",
            "enforceCredentialProtectionPolicy",
            "uvm",
        ] {
            extensions.remove(name);
        }
    }
}

fn user_handle(user_id: Id) -> Uuid {
    Uuid::parse_str(&user_id.to_string()).expect("IDs are UUIDs")
}

#[utoipa::path(get, path = "/api/v1/auth/passkeys", responses((status = 200, body = Vec<PasskeyRecord>), (status = 401, description = "authentication_required", body = ProblemBody, content_type = "application/problem+json")))]
pub(super) async fn list_passkeys(
    State(state): State<AuthState>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<Vec<PasskeyRecord>>, ApiError> {
    let instance = "/api/v1/auth/passkeys";
    let session = authenticate(&state, &headers, instance, request_id.as_ref()).await?;
    state
        .repository
        .list_passkeys(session.user.id)
        .await
        .map(Json)
        .map_err(|_| ApiError::internal(instance, request_id.as_ref()))
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct StartPasskeyRegistrationBody {
    /// The current password: a new passkey is a new way into the account.
    password: String,
}

#[utoipa::path(post, path = "/api/v1/auth/passkeys/register/start", request_body = StartPasskeyRegistrationBody, responses((status = 200, body = PasskeyChallenge), (status = 401, description = "authentication_required or invalid_credentials", body = ProblemBody, content_type = "application/problem+json"), (status = 409, description = "passkeys_unavailable", body = ProblemBody, content_type = "application/problem+json"), (status = 429, description = "authentication_throttled", body = ProblemBody, content_type = "application/problem+json")))]
pub(super) async fn start_passkey_registration(
    State(state): State<AuthState>,
    headers: HeaderMap,
    client_ip: Option<Extension<ClientIp>>,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<StartPasskeyRegistrationBody>,
) -> Result<Json<PasskeyChallenge>, ApiError> {
    let instance = "/api/v1/auth/passkeys/register/start";
    let session = authenticate(&state, &headers, instance, request_id.as_ref()).await?;
    let webauthn = state
        .webauthn
        .clone()
        .ok_or_else(|| unavailable(instance, request_id.as_ref()))?;
    confirm_password(
        &state,
        &session,
        body.password,
        client_ip,
        instance,
        request_id.as_ref(),
    )
    .await?;
    let existing = state
        .repository
        .passkeys_of(session.user.id)
        .await
        .map_err(|_| ApiError::internal(instance, request_id.as_ref()))?
        .iter()
        .map(|passkey| passkey.cred_id().clone())
        .collect::<Vec<_>>();
    let (options, registration) = webauthn
        .start_passkey_registration(
            user_handle(session.user.id),
            &session.user.email,
            &session.user.display_name,
            Some(existing),
        )
        .map_err(|_| ApiError::internal(instance, request_id.as_ref()))?;
    let mut options = serde_json::to_value(options)
        .map_err(|_| ApiError::internal(instance, request_id.as_ref()))?;
    // Ask for a discoverable credential: signing in starts without an email, so the passkey must name the account.
    // "preferred", not "required": Enpass hands a request with both this and user verification required to iCloud
    // Keychain. Password managers and phones make discoverable passkeys anyway; finishing refuses one that is not.
    if let Some(selection) = options
        .pointer_mut("/publicKey/authenticatorSelection")
        .and_then(Value::as_object_mut)
    {
        selection.insert("residentKey".to_owned(), json!("preferred"));
        selection.insert("requireResidentKey".to_owned(), json!(false));
    }
    keep_standard_extensions(&mut options);
    let challenge_token = state
        .pending
        .insert(Pending::PasskeyRegistration {
            user_id: session.user.id,
            state: Box::new(registration),
        })
        .ok_or_else(|| busy(instance, request_id.as_ref()))?;
    Ok(Json(PasskeyChallenge {
        challenge_token,
        options,
    }))
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct FinishPasskeyRegistrationBody {
    challenge_token: String,
    /// What the user calls it, e.g. "MacBook" or "YubiKey".
    name: String,
    /// The browser's `PublicKeyCredential.toJSON()`.
    #[schema(value_type = Object)]
    credential: Value,
}

#[utoipa::path(post, path = "/api/v1/auth/passkeys/register/finish", request_body = FinishPasskeyRegistrationBody, responses((status = 201, body = PasskeyRecord), (status = 400, description = "invalid_passkey", body = ProblemBody, content_type = "application/problem+json"), (status = 401, description = "authentication_required", body = ProblemBody, content_type = "application/problem+json"), (status = 409, description = "passkey_exists or passkeys_unavailable", body = ProblemBody, content_type = "application/problem+json"), (status = 422, description = "invalid_passkey_name or passkey_not_discoverable", body = ProblemBody, content_type = "application/problem+json")))]
pub(super) async fn finish_passkey_registration(
    State(state): State<AuthState>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<FinishPasskeyRegistrationBody>,
) -> Result<Response, ApiError> {
    let instance = "/api/v1/auth/passkeys/register/finish";
    let session = authenticate(&state, &headers, instance, request_id.as_ref()).await?;
    let webauthn = state
        .webauthn
        .clone()
        .ok_or_else(|| unavailable(instance, request_id.as_ref()))?;
    let name = body.name.trim();
    if name.is_empty() || name.chars().count() > 64 {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_passkey_name",
            "Invalid name",
            "Give the passkey a name of 1 to 64 characters.",
            instance,
            request_id.as_ref(),
        ));
    }
    let Some((
        _,
        Pending::PasskeyRegistration {
            user_id,
            state: registration,
        },
    )) = state.pending.take(&body.challenge_token)
    else {
        return Err(invalid_passkey(instance, request_id.as_ref()));
    };
    if user_id != session.user.id {
        return Err(invalid_passkey(instance, request_id.as_ref()));
    }
    let credential = serde_json::from_value::<RegisterPublicKeyCredential>(body.credential)
        .map_err(|_| invalid_passkey(instance, request_id.as_ref()))?;
    // The browser says whether the passkey can name the account (`credProps`). Without that it could never sign in.
    if credential
        .extensions
        .cred_props
        .as_ref()
        .and_then(|props| props.rk)
        == Some(false)
    {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "passkey_not_discoverable",
            "Passkey cannot sign in",
            "This security key saved a passkey that cannot sign in without an email. Use a password manager, \
             your phone or your computer instead.",
            instance,
            request_id.as_ref(),
        ));
    }
    let passkey = webauthn
        .finish_passkey_registration(&credential, &registration)
        .map_err(|_| invalid_passkey(instance, request_id.as_ref()))?;
    let record = state
        .repository
        .save_passkey(
            session.user.id,
            name,
            &passkey,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map_err(|error| match error {
            PasskeySaveError::Duplicate => ApiError::new(
                StatusCode::CONFLICT,
                "passkey_exists",
                "Passkey already added",
                "This passkey is already added.",
                instance,
                request_id.as_ref(),
            ),
            PasskeySaveError::Unavailable(_) => ApiError::internal(instance, request_id.as_ref()),
        })?;
    Ok((StatusCode::CREATED, Json(record)).into_response())
}

#[utoipa::path(delete, path = "/api/v1/auth/passkeys/{id}", params(("id" = String, Path)), responses((status = 204), (status = 401, description = "authentication_required", body = ProblemBody, content_type = "application/problem+json"), (status = 404, description = "passkey_not_found", body = ProblemBody, content_type = "application/problem+json")))]
pub(super) async fn delete_passkey(
    State(state): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    request_id: Option<Extension<RequestId>>,
) -> Result<StatusCode, ApiError> {
    let instance = "/api/v1/auth/passkeys/{id}";
    let session = authenticate(&state, &headers, instance, request_id.as_ref()).await?;
    let not_found = || {
        ApiError::new(
            StatusCode::NOT_FOUND,
            "passkey_not_found",
            "Passkey not found",
            "The passkey does not exist.",
            instance,
            request_id.as_ref(),
        )
    };
    let id = id.parse::<Id>().map_err(|_| not_found())?;
    let deleted = state
        .repository
        .delete_passkey(
            session.user.id,
            id,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map_err(|_| ApiError::internal(instance, request_id.as_ref()))?;
    if !deleted {
        return Err(not_found());
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Starts a sign-in with a passkey. The browser lets the user pick one of the passkeys it has for this site.
#[utoipa::path(post, path = "/api/v1/auth/passkey/login/start", responses((status = 200, body = PasskeyChallenge), (status = 409, description = "passkeys_unavailable", body = ProblemBody, content_type = "application/problem+json"), (status = 503, description = "authentication_busy", body = ProblemBody, content_type = "application/problem+json")))]
pub(super) async fn start_passkey_login(
    State(state): State<AuthState>,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<PasskeyChallenge>, ApiError> {
    let instance = "/api/v1/auth/passkey/login/start";
    let webauthn = state
        .webauthn
        .clone()
        .ok_or_else(|| unavailable(instance, request_id.as_ref()))?;
    let (options, authentication) = webauthn
        .start_discoverable_authentication()
        .map_err(|_| ApiError::internal(instance, request_id.as_ref()))?;
    let mut options = serde_json::to_value(options)
        .map_err(|_| ApiError::internal(instance, request_id.as_ref()))?;
    // The sign-in page asks with a button, not in the email field's autofill.
    if let Some(options) = options.as_object_mut() {
        options.remove("mediation");
    }
    keep_standard_extensions(&mut options);
    let challenge_token = state
        .pending
        .insert(Pending::PasskeyLogin {
            state: Box::new(authentication),
        })
        .ok_or_else(|| busy(instance, request_id.as_ref()))?;
    Ok(Json(PasskeyChallenge {
        challenge_token,
        options,
    }))
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct FinishPasskeyLoginBody {
    challenge_token: String,
    /// The browser's `PublicKeyCredential.toJSON()`.
    #[schema(value_type = Object)]
    credential: Value,
}

/// Signs in with the passkey the browser returned. A passkey is checked with the device's fingerprint, face or PIN,
/// so it needs no password and no second factor.
#[utoipa::path(post, path = "/api/v1/auth/passkey/login/finish", request_body = FinishPasskeyLoginBody, responses((status = 200, body = LoginResponse), (status = 401, description = "invalid_passkey", body = ProblemBody, content_type = "application/problem+json"), (status = 409, description = "passkeys_unavailable", body = ProblemBody, content_type = "application/problem+json")))]
pub(super) async fn finish_passkey_login(
    State(state): State<AuthState>,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<FinishPasskeyLoginBody>,
) -> Result<Response, ApiError> {
    let instance = "/api/v1/auth/passkey/login/finish";
    let webauthn = state
        .webauthn
        .clone()
        .ok_or_else(|| unavailable(instance, request_id.as_ref()))?;
    let rejected = || {
        ApiError::new(
            StatusCode::UNAUTHORIZED,
            "invalid_passkey",
            "Authentication failed",
            "This passkey cannot sign in here. It may have been removed from your account.",
            instance,
            request_id.as_ref(),
        )
    };
    let Some((
        _,
        Pending::PasskeyLogin {
            state: authentication,
        },
    )) = state.pending.take(&body.challenge_token)
    else {
        return Err(rejected());
    };
    let credential =
        serde_json::from_value::<PublicKeyCredential>(body.credential).map_err(|_| rejected())?;
    let (handle, credential_id) = webauthn
        .identify_discoverable_authentication(&credential)
        .map_err(|_| rejected())?;
    let stored = state
        .repository
        .passkey_by_credential(credential_id)
        .await
        .map_err(|_| ApiError::internal(instance, request_id.as_ref()))?
        .filter(|stored| user_handle(stored.user_id) == handle);
    let Some(mut stored) = stored else {
        record_failure(&state, None, request_id.as_ref()).await;
        return Err(rejected());
    };
    let result = webauthn.finish_discoverable_authentication(
        &credential,
        *authentication,
        &[DiscoverableKey::from(&stored.passkey)],
    );
    let Ok(result) = result else {
        record_failure(&state, Some(stored.user_id), request_id.as_ref()).await;
        return Err(rejected());
    };
    stored.passkey.update_credential(&result);
    let (user, session) = state
        .repository
        .create_passkey_session_audited(
            &stored,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map_err(|error| match error {
            // Suspended, or the passkey was removed meanwhile.
            IdentityError::InvalidCredential => rejected(),
            _ => ApiError::internal(instance, request_id.as_ref()),
        })?;
    Ok(signed_in_response(&state, user, &session).await)
}

async fn record_failure(
    state: &AuthState,
    user_id: Option<Id>,
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
            json!({"method": "passkey"}),
            TimestampMillis::now(),
        )
        .await;
}
