//! `POST /api/v1/mail/inbound/{kind}/{provider_id}`: mail from a provider (provider intake design
//! §4). No session: the provider's signature or credentials authenticate the request. The replies
//! tell each provider whether to send again: 200 once stored (or when nothing can be delivered),
//! 503 to retry, and the provider's own "do not retry" code for a refused request (Postmark 403,
//! Mailgun 406, others 401 or 413).

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use orbit_mail::inbound::{self, InboundError};
use orbit_platform::{Id, TimestampMillis};
use serde_json::json;

use super::MailState;
use crate::mail_delivery::{DeliveryResult, ProviderIntake, reject_text};
use crate::repositories::mail::{MailProvider, ProviderKind, ProviderSecrets};

/// A message from a provider's webhook. The body is the provider's own format.
#[utoipa::path(
    post,
    path = "/api/v1/mail/inbound/{kind}/{provider_id}",
    params(("kind" = String, Path, description = "cloudflare, postmark, mailgun, resend or ses"), ("provider_id" = String, Path)),
    request_body(content = String, content_type = "application/json", description = "The provider's own payload (JSON; Mailgun: a form)"),
    responses((status = 200), (status = 401), (status = 404), (status = 413), (status = 503))
)]
pub(crate) async fn post_inbound(
    State(state): State<MailState>,
    Path((kind, provider_id)): Path<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let Some(intake) = state.inbound.clone() else {
        return reply(
            StatusCode::SERVICE_UNAVAILABLE,
            "mail intake is not running",
        );
    };
    let (Ok(kind), Ok(provider_id)) = (kind.parse::<ProviderKind>(), provider_id.parse::<Id>())
    else {
        return not_found();
    };
    let (provider, secrets) = match intake.mail().load_provider(provider_id).await {
        Ok(Some(found)) if found.0.kind == kind => found,
        Ok(_) => return not_found(),
        Err(error) => {
            tracing::warn!(%error, "mail provider lookup failed");
            return reply(StatusCode::SERVICE_UNAVAILABLE, "try again later");
        }
    };
    let now = TimestampMillis::now();
    let now_secs = now.as_millis() / 1000;
    let max = usize::try_from(intake.max_message_bytes()).unwrap_or(usize::MAX);
    let header = |name: &str| headers.get(name).and_then(|value| value.to_str().ok());

    let parsed = match &secrets {
        ProviderSecrets::Cloudflare { signing_secret } => inbound::cloudflare::verify_and_parse(
            header("x-orbit-timestamp"),
            header("x-orbit-signature"),
            &body,
            signing_secret.as_bytes(),
            now_secs,
            max,
        ),
        ProviderSecrets::Postmark {
            basic_user,
            basic_password,
        } => inbound::postmark::verify_and_parse(
            header(header::AUTHORIZATION.as_str()),
            &body,
            basic_user,
            basic_password,
            max,
        ),
        ProviderSecrets::Mailgun { signing_key } => {
            let form: Vec<(String, String)> = url::form_urlencoded::parse(&body)
                .map(|(key, value)| (key.into_owned(), value.into_owned()))
                .collect();
            inbound::mailgun::verify_and_parse(&form, signing_key.as_bytes(), now_secs, max)
                .and_then(|(delivery, token)| {
                    if intake.first_use(&token, now_secs) {
                        Ok(delivery)
                    } else {
                        Err(InboundError::Replay)
                    }
                })
        }
        ProviderSecrets::Resend {
            api_key,
            webhook_secret,
        } => {
            let event = match inbound::resend::verify_webhook(
                header("svix-id"),
                header("svix-timestamp"),
                header("svix-signature"),
                &body,
                webhook_secret,
                now_secs,
            ) {
                Ok(event) => event,
                // a Resend event of another type (sent, bounced, …): nothing to receive
                Err(InboundError::Malformed("event_type")) => {
                    return (StatusCode::OK, Json(json!({ "ignored": true }))).into_response();
                }
                Err(error) => return refused(&intake, &provider, error, now).await,
            };
            if !event
                .email_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            {
                return refused(&intake, &provider, InboundError::Malformed("email_id"), now).await;
            }
            return delivered(
                kind,
                intake.receive_resend(&provider, api_key, event, now).await,
            );
        }
        ProviderSecrets::Ses {
            access_key_id,
            secret_access_key,
        } => {
            return match intake
                .receive_ses(&provider, access_key_id, secret_access_key, &body, now)
                .await
            {
                Ok(result) => delivered(kind, result),
                Err(error) => refused(&intake, &provider, error, now).await,
            };
        }
    };
    let delivery = match parsed {
        Ok(delivery) => delivery,
        Err(error) => return refused(&intake, &provider, error, now).await,
    };
    delivered(kind, intake.deliver(&provider, delivery, now).await)
}

/// A request the provider must not send again; its error shows on the provider in settings.
async fn refused(
    intake: &ProviderIntake,
    provider: &MailProvider,
    error: InboundError,
    now: TimestampMillis,
) -> Response {
    if let Err(problem) = intake
        .mail()
        .record_provider_receipt(provider.id, now, Some(&error.to_string()))
        .await
    {
        tracing::warn!(error = %problem, "mail provider status not saved");
    }
    let status = match (provider.kind, error) {
        (ProviderKind::Postmark, _) => StatusCode::FORBIDDEN,
        (ProviderKind::Mailgun, _) => StatusCode::NOT_ACCEPTABLE,
        (_, InboundError::TooLarge) => StatusCode::PAYLOAD_TOO_LARGE,
        (_, InboundError::Malformed(_)) => StatusCode::BAD_REQUEST,
        (_, InboundError::Unauthorized | InboundError::Replay) => StatusCode::UNAUTHORIZED,
    };
    reply(status, &error.to_string())
}

fn delivered(kind: ProviderKind, result: DeliveryResult) -> Response {
    match result {
        DeliveryResult::Stored { receipts, rejected } => (
            StatusCode::OK,
            Json(json!({ "stored": receipts, "rejected": rejected })),
        )
            .into_response(),
        // Cloudflare's Worker turns `reject` into a bounce while the sender is still connected
        DeliveryResult::NothingDeliverable { reason, recipient } => (
            StatusCode::OK,
            Json(json!({ "stored": 0, "reject": reject_text(reason, &recipient) })),
        )
            .into_response(),
        DeliveryResult::Confirmed => {
            (StatusCode::OK, Json(json!({ "confirmed": true }))).into_response()
        }
        DeliveryResult::Temporary(error) => {
            tracing::warn!(%error, provider = kind.as_str(), "provider mail not stored");
            reply(StatusCode::SERVICE_UNAVAILABLE, "try again later")
        }
        DeliveryResult::TooLarge => match kind {
            ProviderKind::Postmark => reply(StatusCode::FORBIDDEN, "message too large"),
            ProviderKind::Mailgun => reply(StatusCode::NOT_ACCEPTABLE, "message too large"),
            _ => reply(StatusCode::PAYLOAD_TOO_LARGE, "message too large"),
        },
    }
}

fn not_found() -> Response {
    reply(StatusCode::NOT_FOUND, "no such mail provider")
}

fn reply(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({ "error": message }))).into_response()
}
