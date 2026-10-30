//! Deleting your own account and changing its email.

use axum::Json;
use axum::extract::{Extension, State};
use axum::http::header::SET_COOKIE;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use orbit_platform::{ClientIp, RequestId, TimestampMillis, normalize_email};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::{
    ApiError, ApiJson, AuthState, ProblemBody, SessionCredential, accepted, authenticate,
    clear_session_cookie, request_id_value, request_session, two_factor,
};
use crate::mail;
use crate::repositories::account::{DeleteAccountError, EmailChangeError};

const ME: &str = "/api/v1/auth/me";
const CHANGE_EMAIL: &str = "/api/v1/auth/me/email";
const CONFIRM_EMAIL: &str = "/api/v1/auth/email-change/confirm";
const EMAIL_CHANGE_LIFETIME_MILLIS: i64 = 60 * 60 * 1_000;

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct DeleteAccountBody {
    /// The account's current email, typed again to confirm.
    email: String,
    /// Required when the account has a password. An account without one confirms with a session that signed in
    /// (or confirmed with a provider) in the last 10 minutes, else 401 `reauthentication_required`.
    password: Option<String>,
}

/// Deletes the account for good (GDPR erasure). The account is anonymized: what it authored stays and shows
/// "Deleted user"; its email is freed, every session ends and it can never sign in again. Refused with 409
/// `owns_workspaces` (the problem lists them in `workspaces`) while the account owns a workspace outside the trash,
/// and with 409 `root_account` for the root user.
#[utoipa::path(delete, path = "/api/v1/auth/me", request_body = DeleteAccountBody, responses((status = 204, description = "Deleted; the session cookie is cleared."), (status = 401, description = "authentication_required, password_required, invalid_credentials or reauthentication_required", body = ProblemBody, content_type = "application/problem+json"), (status = 409, description = "owns_workspaces or root_account", body = ProblemBody, content_type = "application/problem+json"), (status = 422, description = "confirmation_mismatch", body = ProblemBody, content_type = "application/problem+json"), (status = 429, description = "authentication_throttled", body = ProblemBody, content_type = "application/problem+json")))]
pub(super) async fn delete_account(
    State(state): State<AuthState>,
    headers: HeaderMap,
    client_ip: Option<Extension<ClientIp>>,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<DeleteAccountBody>,
) -> Result<Response, ApiError> {
    let rid = request_id.as_ref();
    let session = authenticate(&state, &headers, ME, rid).await?;
    if normalize_email(&body.email) != normalize_email(&session.user.email) {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "confirmation_mismatch",
            "Confirmation does not match",
            "Type your account's email to confirm.",
            ME,
            rid,
        ));
    }
    two_factor::confirm_identity(&state, &session, body.password, client_ip, ME, rid).await?;
    state
        .repository
        .delete_account(
            session.user.id,
            request_id_value(rid),
            TimestampMillis::now(),
        )
        .await
        .map_err(|error| match error {
            DeleteAccountError::RootAccount => ApiError::new(
                StatusCode::CONFLICT,
                "root_account",
                "Root account",
                "The root account cannot be deleted. Make another account the root user first.",
                ME,
                rid,
            ),
            DeleteAccountError::OwnsWorkspaces(workspaces) => {
                let mut error = ApiError::new(
                    StatusCode::CONFLICT,
                    "owns_workspaces",
                    "You own workspaces",
                    "Delete the workspaces you own first.",
                    ME,
                    rid,
                );
                error.body.workspaces = Some(workspaces);
                error
            }
            DeleteAccountError::NotFound | DeleteAccountError::Unavailable(_) => {
                ApiError::internal(ME, rid)
            }
        })?;
    let mut response = StatusCode::NO_CONTENT.into_response();
    if let Some(SessionCredential::Cookie(_)) = state.cookie_mode.session_credential(&headers) {
        response.headers_mut().insert(
            SET_COOKIE,
            HeaderValue::from_str(&clear_session_cookie(state.cookie_mode)).unwrap(),
        );
    }
    Ok(response)
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct ChangeEmailBody {
    /// The new address.
    email: String,
    /// Required when the account has a password (see `DeleteAccountBody.password`).
    password: Option<String>,
}

/// Emails a one-use link, valid for 60 minutes, to the new address; the email changes when it is opened. Like
/// registration, the answer does not tell whether another account has the address or the address reached its email
/// limit: then no email is sent and an earlier link stays valid. Needs a mail server (409 `mail_unavailable`).
#[utoipa::path(post, path = "/api/v1/auth/me/email", request_body = ChangeEmailBody, responses((status = 202, body = super::RecoveryRequestResponse), (status = 401, description = "authentication_required, password_required, invalid_credentials or reauthentication_required", body = ProblemBody, content_type = "application/problem+json"), (status = 409, description = "mail_unavailable", body = ProblemBody, content_type = "application/problem+json"), (status = 422, description = "invalid_email or same_email", body = ProblemBody, content_type = "application/problem+json"), (status = 429, description = "authentication_throttled", body = ProblemBody, content_type = "application/problem+json")))]
pub(super) async fn request_email_change(
    State(state): State<AuthState>,
    headers: HeaderMap,
    client_ip: Option<Extension<ClientIp>>,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<ChangeEmailBody>,
) -> Result<Response, ApiError> {
    let rid = request_id.as_ref();
    let session = authenticate(&state, &headers, CHANGE_EMAIL, rid).await?;
    let email = body.email.trim();
    if !mail::valid_address(email) {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_email",
            "Invalid email",
            "Enter a valid email address.",
            CHANGE_EMAIL,
            rid,
        ));
    }
    if normalize_email(email) == normalize_email(&session.user.email) {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "same_email",
            "Same email",
            "This is already your email.",
            CHANGE_EMAIL,
            rid,
        ));
    }
    let Some(mailer) = state.mailer().await else {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "mail_unavailable",
            "Email is not set up",
            "This Orbit cannot send email, so the email cannot be changed. Ask an administrator.",
            CHANGE_EMAIL,
            rid,
        ));
    };
    two_factor::confirm_identity(
        &state,
        &session,
        body.password,
        client_ip,
        CHANGE_EMAIL,
        rid,
    )
    .await?;
    let taken = state
        .repository
        .email_taken_by_other(session.user.id, email)
        .await
        .map_err(|_| ApiError::internal(CHANGE_EMAIL, rid))?;
    if !taken
        && state
            .account_email_allowed(email)
            .await
            .map_err(|_| ApiError::internal(CHANGE_EMAIL, rid))?
    {
        let token = orbit_platform::generate_opaque_token();
        let now = TimestampMillis::now();
        state
            .repository
            .store_email_change_token(
                session.user.id,
                email,
                &token,
                TimestampMillis::from_millis(now.as_millis() + EMAIL_CHANGE_LIFETIME_MILLIS),
                request_id_value(rid),
                now,
            )
            .await
            .map_err(|_| ApiError::internal(CHANGE_EMAIL, rid))?;
        mailer.send_later(mail::templates::email_change(
            email,
            &mailer.link(&format!("/confirm-email?token={token}")),
        ));
    }
    Ok(accepted(
        "We sent a link to the new address. Open it within 60 minutes to finish the change.",
    ))
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct ConfirmEmailChangeBody {
    token: String,
}

#[derive(Serialize, ToSchema)]
pub(super) struct EmailChangeConfirmed {
    /// The account's email now.
    email: String,
}

/// Opens the emailed link: the new address becomes the account's email, verified. Needs no session. Every other
/// session of the account is signed out (the request's own session stays), and the old address gets a notice.
#[utoipa::path(post, path = "/api/v1/auth/email-change/confirm", request_body = ConfirmEmailChangeBody, responses((status = 200, body = EmailChangeConfirmed), (status = 400, description = "invalid_email_change_token", body = ProblemBody, content_type = "application/problem+json"), (status = 409, description = "email_taken", body = ProblemBody, content_type = "application/problem+json")))]
pub(super) async fn confirm_email_change(
    State(state): State<AuthState>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<ConfirmEmailChangeBody>,
) -> Result<Json<EmailChangeConfirmed>, ApiError> {
    let rid = request_id.as_ref();
    let keep_session = request_session(&state.repository, state.cookie_mode, &headers)
        .await
        .map(|session| session.id);
    let changed = state
        .repository
        .confirm_email_change(
            &body.token,
            keep_session,
            request_id_value(rid),
            TimestampMillis::now(),
        )
        .await
        .map_err(|error| match error {
            EmailChangeError::InvalidToken => ApiError::new(
                StatusCode::BAD_REQUEST,
                "invalid_email_change_token",
                "Invalid link",
                "This link is invalid, used or expired. Ask for a new one in your account settings.",
                CONFIRM_EMAIL,
                rid,
            ),
            EmailChangeError::EmailTaken => ApiError::new(
                StatusCode::CONFLICT,
                "email_taken",
                "Email taken",
                "Another account has this email now. Choose another address.",
                CONFIRM_EMAIL,
                rid,
            ),
            EmailChangeError::Unavailable(_) => ApiError::internal(CONFIRM_EMAIL, rid),
        })?;
    if let Some(mailer) = state.mailer().await {
        mailer.send_later(mail::templates::email_changed(
            &changed.old_email,
            &changed.new_email,
        ));
    }
    Ok(Json(EmailChangeConfirmed {
        email: changed.new_email,
    }))
}
