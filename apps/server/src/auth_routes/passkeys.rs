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

use super::two_factor::{Pending, busy, confirm_identity};
use super::{
    ApiError, ApiJson, AuthState, LoginResponse, ProblemBody, SessionDelivery, authenticate,
    last_sign_in_method, request_id_value, signed_in_response,
};
use crate::audit::AuditOutcome;
use crate::repositories::identity::IdentityError;
use crate::repositories::two_factor::{
    PasskeyRecord, PasskeyRemoval, PasskeySaveError, StoredPasskey,
};
use crate::sign_in_providers::is_pkce_challenge;

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
        .map_err(|error| ApiError::internal(instance, request_id.as_ref()).cause(error))
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct StartPasskeyRegistrationBody {
    /// The current password: a new passkey is a new way into the account. Leave it out when the account has none
    /// (see `reauthentication_required`).
    password: Option<String>,
}

#[utoipa::path(post, path = "/api/v1/auth/passkeys/register/start", request_body = StartPasskeyRegistrationBody, responses((status = 200, body = PasskeyChallenge), (status = 401, description = "authentication_required, invalid_credentials, password_required or reauthentication_required", body = ProblemBody, content_type = "application/problem+json"), (status = 409, description = "passkeys_unavailable", body = ProblemBody, content_type = "application/problem+json"), (status = 429, description = "authentication_throttled", body = ProblemBody, content_type = "application/problem+json")))]
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
    confirm_identity(
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
        .map_err(|error| ApiError::internal(instance, request_id.as_ref()).cause(error))?
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
        .map_err(|error| ApiError::internal(instance, request_id.as_ref()).cause(error))?;
    let mut options = serde_json::to_value(options)
        .map_err(|error| ApiError::internal(instance, request_id.as_ref()).cause(error))?;
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
        .await
        .map_err(|error| ApiError::internal(instance, request_id.as_ref()).cause(error))?
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
    )) = state
        .pending
        .take(&body.challenge_token)
        .await
        .map_err(|error| ApiError::internal(instance, request_id.as_ref()).cause(error))?
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
            PasskeySaveError::Unavailable(IdentityError::Unavailable(error)) => {
                ApiError::internal(instance, request_id.as_ref()).cause(error)
            }
            PasskeySaveError::Unavailable(error) => {
                ApiError::internal(instance, request_id.as_ref()).cause(error)
            }
        })?;
    Ok((StatusCode::CREATED, Json(record)).into_response())
}

#[utoipa::path(delete, path = "/api/v1/auth/passkeys/{id}", params(("id" = String, Path)), responses((status = 204), (status = 401, description = "authentication_required", body = ProblemBody, content_type = "application/problem+json"), (status = 404, description = "passkey_not_found", body = ProblemBody, content_type = "application/problem+json"), (status = 409, description = "last_sign_in_method", body = ProblemBody, content_type = "application/problem+json")))]
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
    let removal = state
        .repository
        .delete_passkey(
            session.user.id,
            id,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map_err(|error| ApiError::internal(instance, request_id.as_ref()).cause(error))?;
    match removal {
        PasskeyRemoval::Removed => Ok(StatusCode::NO_CONTENT),
        PasskeyRemoval::NotFound => Err(not_found()),
        PasskeyRemoval::LastSignInMethod => Err(last_sign_in_method(instance, request_id.as_ref())),
    }
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
        .map_err(|error| ApiError::internal(instance, request_id.as_ref()).cause(error))?;
    let mut options = serde_json::to_value(options)
        .map_err(|error| ApiError::internal(instance, request_id.as_ref()).cause(error))?;
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
        .await
        .map_err(|error| ApiError::internal(instance, request_id.as_ref()).cause(error))?
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
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<FinishPasskeyLoginBody>,
) -> Result<Response, ApiError> {
    let instance = "/api/v1/auth/passkey/login/finish";
    let rid = request_id.as_ref();
    let stored = verified_passkey(
        &state,
        &body.challenge_token,
        body.credential,
        instance,
        rid,
    )
    .await?;
    passkey_session(&state, &headers, &stored, instance, rid).await
}

fn rejected(instance: &'static str, request_id: Option<&Extension<RequestId>>) -> ApiError {
    ApiError::new(
        StatusCode::UNAUTHORIZED,
        "invalid_passkey",
        "Authentication failed",
        "This passkey cannot sign in here. It may have been removed from your account.",
        instance,
        request_id,
    )
}

/// The stored passkey that signed the challenge, with its use counter moved on. Uses the challenge up.
async fn verified_passkey(
    state: &AuthState,
    challenge_token: &str,
    credential: Value,
    instance: &'static str,
    rid: Option<&Extension<RequestId>>,
) -> Result<StoredPasskey, ApiError> {
    let webauthn = state
        .webauthn
        .clone()
        .ok_or_else(|| unavailable(instance, rid))?;
    let Some((
        _,
        Pending::PasskeyLogin {
            state: authentication,
        },
    )) = state
        .pending
        .take(challenge_token)
        .await
        .map_err(|error| ApiError::internal(instance, rid).cause(error))?
    else {
        return Err(rejected(instance, rid));
    };
    let credential = serde_json::from_value::<PublicKeyCredential>(credential)
        .map_err(|_| rejected(instance, rid))?;
    let (handle, credential_id) = webauthn
        .identify_discoverable_authentication(&credential)
        .map_err(|_| rejected(instance, rid))?;
    let stored = state
        .repository
        .passkey_by_credential(credential_id)
        .await
        .map_err(|error| ApiError::internal(instance, rid).cause(error))?
        .filter(|stored| user_handle(stored.user_id) == handle);
    let Some(mut stored) = stored else {
        record_failure(state, None, rid).await;
        return Err(rejected(instance, rid));
    };
    let result = webauthn.finish_discoverable_authentication(
        &credential,
        *authentication,
        &[DiscoverableKey::from(&stored.passkey)],
    );
    let Ok(result) = result else {
        record_failure(state, Some(stored.user_id), rid).await;
        return Err(rejected(instance, rid));
    };
    stored.passkey.update_credential(&result);
    Ok(stored)
}

/// The session of a verified passkey: a cookie for browsers, `session_token` for requests without Origin.
pub(super) async fn passkey_session(
    state: &AuthState,
    headers: &HeaderMap,
    stored: &StoredPasskey,
    instance: &'static str,
    rid: Option<&Extension<RequestId>>,
) -> Result<Response, ApiError> {
    let (user, session) = state
        .repository
        .create_passkey_session_audited(stored, request_id_value(rid), TimestampMillis::now())
        .await
        .map_err(|error| match error {
            // Suspended, or the passkey was removed meanwhile.
            IdentityError::InvalidCredential => rejected(instance, rid),
            IdentityError::Unavailable(error) => ApiError::internal(instance, rid).cause(error),
            error => ApiError::internal(instance, rid).cause(error),
        })?;
    Ok(signed_in_response(state, user, &session, SessionDelivery::for_request(headers)).await)
}

const APP_START: &str = "/api/v1/auth/passkey/app/start";
const APP_FINISH: &str = "/api/v1/auth/passkey/app/finish";
/// The web page that asks for the passkey and sends the browser back to the app.
const APP_SIGN_IN_PATH: &str = "/login/app";
/// Opening the browser and choosing a passkey can take longer than five minutes.
const APP_SIGN_IN_LIFETIME_MILLIS: i64 = 10 * 60 * 1_000;

/// A passkey sign-in the Orbit app started, from start to exchange.
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct AppSignIn {
    /// S256 of the verifier the app keeps; the exchange must bring the verifier.
    pub(super) client_challenge: String,
    /// An allowlisted app URI: `{redirect}?handoff=…`
    redirect: String,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct StartAppPasskeyBody {
    /// `base64url(sha256(code_verifier))` of a random verifier the app keeps (43 characters). The same verifier goes
    /// to `/api/v1/auth/providers/exchange`.
    code_challenge: String,
    /// The app's redirect URI: `orbit://auth/callback` (development servers can allow more with
    /// `ORBIT_DEV_NATIVE_REDIRECTS`).
    redirect: String,
}

#[derive(Serialize, ToSchema)]
pub(super) struct StartAppPasskeyResponse {
    /// Orbit's own sign-in page for this flow. Open it in the system browser's auth session.
    authorize_url: String,
}

fn unprocessable(
    code: &'static str,
    detail: &'static str,
    request_id: Option<&Extension<RequestId>>,
) -> ApiError {
    ApiError::new(
        StatusCode::UNPROCESSABLE_ENTITY,
        code,
        "Invalid passkey sign-in",
        detail,
        APP_START,
        request_id,
    )
}

/// Starts a passkey sign-in for the Orbit app. A passkey belongs to the server's web origin, so the app opens the
/// returned page in the system browser; the page ends at the app's redirect with a one-time handoff code, which the
/// app trades with its verifier at `/api/v1/auth/providers/exchange`.
#[utoipa::path(post, path = "/api/v1/auth/passkey/app/start", request_body = StartAppPasskeyBody, responses((status = 200, body = StartAppPasskeyResponse), (status = 409, description = "passkeys_unavailable", body = ProblemBody, content_type = "application/problem+json"), (status = 422, description = "invalid_code_challenge or invalid_redirect", body = ProblemBody, content_type = "application/problem+json"), (status = 503, description = "authentication_busy", body = ProblemBody, content_type = "application/problem+json")))]
pub(super) async fn start_app_passkey_sign_in(
    State(state): State<AuthState>,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<StartAppPasskeyBody>,
) -> Result<Json<StartAppPasskeyResponse>, ApiError> {
    let rid = request_id.as_ref();
    if state.webauthn.is_none() {
        return Err(unavailable(APP_START, rid));
    }
    if !is_pkce_challenge(&body.code_challenge) {
        return Err(unprocessable(
            "invalid_code_challenge",
            "code_challenge must be the base64url SHA-256 of the code verifier.",
            rid,
        ));
    }
    if !state.native_redirects.allows(&body.redirect) {
        return Err(unprocessable(
            "invalid_redirect",
            "This redirect URI is not allowed.",
            rid,
        ));
    }
    let flow = state
        .pending
        .insert_for(
            Pending::AppSignIn {
                flow: AppSignIn {
                    client_challenge: body.code_challenge,
                    redirect: body.redirect,
                },
            },
            APP_SIGN_IN_LIFETIME_MILLIS,
        )
        .await
        .map_err(|error| ApiError::internal(APP_START, rid).cause(error))?
        .ok_or_else(|| busy(APP_START, rid))?;
    // In the fragment, so the token stays out of server logs and Referer headers.
    Ok(Json(StartAppPasskeyResponse {
        authorize_url: format!("{}{APP_SIGN_IN_PATH}#flow={flow}", state.public_origin),
    }))
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct FinishAppPasskeyBody {
    /// The `flow` of the page's URL fragment.
    flow: String,
    /// From `/api/v1/auth/passkey/login/start`.
    challenge_token: String,
    /// The browser's `PublicKeyCredential.toJSON()`.
    #[schema(value_type = Object)]
    credential: Value,
}

#[derive(Serialize, ToSchema)]
pub(super) struct FinishAppPasskeyResponse {
    /// The app's redirect with the handoff code: `{redirect}?handoff=…`. Send the browser there.
    redirect_url: String,
}

/// The sign-in page's answer to [`start_app_passkey_sign_in`]. Checks the passkey and parks the result behind a
/// one-time handoff code for the app. It makes no session and sets no cookie: the browser stays as it was, and only
/// the exchange with the app's verifier signs in.
#[utoipa::path(post, path = "/api/v1/auth/passkey/app/finish", request_body = FinishAppPasskeyBody, responses((status = 200, body = FinishAppPasskeyResponse), (status = 400, description = "invalid_app_sign_in: the flow expired or was used", body = ProblemBody, content_type = "application/problem+json"), (status = 401, description = "invalid_passkey", body = ProblemBody, content_type = "application/problem+json"), (status = 409, description = "passkeys_unavailable", body = ProblemBody, content_type = "application/problem+json"), (status = 503, description = "authentication_busy", body = ProblemBody, content_type = "application/problem+json")))]
pub(super) async fn finish_app_passkey_sign_in(
    State(state): State<AuthState>,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<FinishAppPasskeyBody>,
) -> Result<Json<FinishAppPasskeyResponse>, ApiError> {
    let rid = request_id.as_ref();
    // The passkey first: a wrong one leaves the flow open for another try.
    let stored = verified_passkey(
        &state,
        &body.challenge_token,
        body.credential,
        APP_FINISH,
        rid,
    )
    .await?;
    let Some((_, Pending::AppSignIn { flow })) = state
        .pending
        .take(&body.flow)
        .await
        .map_err(|error| ApiError::internal(APP_FINISH, rid).cause(error))?
    else {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_app_sign_in",
            "Sign-in expired",
            "This sign-in has expired or was already used. Go back to the Orbit app and start again.",
            APP_FINISH,
            rid,
        ));
    };
    let redirect = flow.redirect.clone();
    let handoff = state
        .pending
        .insert(Pending::PasskeyHandoff {
            flow,
            passkey_id: stored.id,
            user_id: stored.user_id,
            passkey: Box::new(stored.passkey),
        })
        .await
        .map_err(|error| ApiError::internal(APP_FINISH, rid).cause(error))?
        .ok_or_else(|| busy(APP_FINISH, rid))?;
    let separator = if redirect.contains('?') { '&' } else { '?' };
    let parameters = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("handoff", &handoff)
        .finish();
    Ok(Json(FinishAppPasskeyResponse {
        redirect_url: format!("{redirect}{separator}{parameters}"),
    }))
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
