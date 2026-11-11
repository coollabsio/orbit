//! `POST /api/v1/mail/inbound/{kind}/{provider_id}`: mail from a provider (provider intake design
//! §4). No session: the provider's signature or credentials authenticate the request. The replies
//! tell each provider whether to send again: 200 once stored (or when nothing can be delivered),
//! 503 to retry, and the provider's own "do not retry" code for a refused request (Postmark 403,
//! Mailgun 406, others 401 or 413).

use axum::Json;
use axum::body::{Body, Bytes};
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
    params(("kind" = String, Path, description = "postshiba, cloudflare, postmark, mailgun, resend or ses"), ("provider_id" = String, Path)),
    request_body(content = String, content_type = "application/json", description = "The provider's own payload (JSON; Mailgun: a form)"),
    responses((status = 200), (status = 401), (status = 404), (status = 413), (status = 503))
)]
pub(crate) async fn post_inbound(
    State(state): State<MailState>,
    Path((kind, provider_id)): Path<(String, String)>,
    headers: HeaderMap,
    body: Body,
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

    // what can be checked before the body is read: anyone can post to this URL
    let signed = match &secrets {
        ProviderSecrets::Postmark {
            basic_user,
            basic_password,
        } => inbound::postmark::check_auth(
            header(header::AUTHORIZATION.as_str()),
            basic_user,
            basic_password,
        )
        .is_ok(),
        ProviderSecrets::Cloudflare { .. } => [
            "x-orbit-signature",
            "x-orbit-timestamp",
            "x-orbit-from",
            "x-orbit-to",
        ]
        .iter()
        .all(|name| header(name).is_some()),
        ProviderSecrets::Postshiba { .. } => header("x-capsule-signature").is_some(),
        ProviderSecrets::Resend { .. } => header("svix-signature").is_some(),
        ProviderSecrets::Mailgun { .. } | ProviderSecrets::Ses { .. } => true,
    };
    if !signed {
        return refused(&intake, &provider, InboundError::Unauthorized, now, false).await;
    }
    let Some(_permit) = intake.body_permit().await else {
        return reply(StatusCode::SERVICE_UNAVAILABLE, "try again later");
    };
    let Ok(body) = axum::body::to_bytes(body, body_limit(kind, max)).await else {
        // only Postmark's credentials are checked before the body
        let checked = kind == ProviderKind::Postmark;
        return refused(&intake, &provider, InboundError::TooLarge, now, checked).await;
    };

    let parsed = match &secrets {
        ProviderSecrets::Cloudflare { signing_secret } => inbound::cloudflare::verify_and_parse(
            inbound::cloudflare::Envelope {
                timestamp: header("x-orbit-timestamp"),
                signature: header("x-orbit-signature"),
                from: header("x-orbit-from"),
                to: header("x-orbit-to"),
            },
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
            let content_type = header(header::CONTENT_TYPE.as_str()).unwrap_or_default();
            let form = if content_type.starts_with("multipart/form-data") {
                match multipart_form(content_type, body).await {
                    Some(form) => form,
                    None => {
                        let error = InboundError::Malformed("form");
                        return refused(&intake, &provider, error, now, false).await;
                    }
                }
            } else {
                inbound::mailgun::parse_urlencoded(&body)
            };
            let (delivery, token) = match inbound::mailgun::verify_and_parse(
                &form,
                signing_key.as_bytes(),
                now_secs,
                max,
            ) {
                Ok(parsed) if !intake.token_seen(&parsed.1, now_secs) => parsed,
                Ok(_) => return refused(&intake, &provider, InboundError::Replay, now, true).await,
                Err(error) => return refused(&intake, &provider, error, now, true).await,
            };
            let result = intake.deliver(&provider, delivery, now).await;
            if matches!(
                result,
                DeliveryResult::Stored { .. } | DeliveryResult::NothingDeliverable { .. }
            ) {
                intake.remember_token(token, now_secs);
            }
            return delivered(kind, result);
        }
        ProviderSecrets::Postshiba {
            api_key,
            webhook_secret,
        } => {
            let event = match inbound::postshiba::verify_webhook(
                header("x-capsule-timestamp"),
                header("x-capsule-signature"),
                &body,
                webhook_secret,
                now_secs,
            ) {
                Ok(event) => event,
                Err(error) => return refused(&intake, &provider, error, now, true).await,
            };
            return delivered(
                kind,
                intake
                    .receive_postshiba(&provider, api_key, event, now)
                    .await,
            );
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
                Err(error) => return refused(&intake, &provider, error, now, true).await,
            };
            if !event
                .email_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            {
                let error = InboundError::Malformed("email_id");
                return refused(&intake, &provider, error, now, true).await;
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
                // receive_ses notes the errors that come after the SNS signature
                Err(error) => refused(&intake, &provider, error, now, false).await,
            };
        }
    };
    let delivery = match parsed {
        Ok(delivery) => delivery,
        Err(error) => return refused(&intake, &provider, error, now, true).await,
    };
    delivered(kind, intake.deliver(&provider, delivery, now).await)
}

/// A request the provider must not send again. When `checked` (the request passed the signature
/// or credential check), its error shows on the provider in settings: anyone can post to the URL,
/// so an unchecked request, a bad signature or a replay must not hide the provider's real status.
async fn refused(
    intake: &ProviderIntake,
    provider: &MailProvider,
    error: InboundError,
    now: TimestampMillis,
    checked: bool,
) -> Response {
    if checked && !matches!(error, InboundError::Unauthorized | InboundError::Replay) {
        intake.note(provider, now, &error.to_string()).await;
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
        DeliveryResult::AlreadyHandled => {
            (StatusCode::OK, Json(json!({ "stored": 0 }))).into_response()
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

/// The largest body each provider's request may have: JSON escapes (Postmark) and percent-encoding
/// (Mailgun) grow a message; Cloudflare sends the raw message; Resend and SES send small events.
fn body_limit(kind: ProviderKind, max: usize) -> usize {
    const MIB: usize = 1024 * 1024;
    match kind {
        ProviderKind::Cloudflare => max.saturating_add(64 * 1024),
        ProviderKind::Postmark => max.saturating_mul(2).saturating_add(MIB),
        ProviderKind::Mailgun => max.saturating_mul(3).saturating_add(MIB),
        // the JSON event carries the attachments in base64 up to 4 MB
        ProviderKind::Postshiba => 8 * MIB,
        ProviderKind::Resend | ProviderKind::Ses => 256 * 1024,
    }
}

/// A `multipart/form-data` body as name and bytes; `None` when it does not parse.
async fn multipart_form(content_type: &str, body: Bytes) -> Option<Vec<(String, Vec<u8>)>> {
    let boundary = multer::parse_boundary(content_type).ok()?;
    let stream = futures_util::stream::once(async move { Ok::<_, std::io::Error>(body) });
    let mut multipart = multer::Multipart::new(stream, boundary);
    let mut form = Vec::new();
    loop {
        match multipart.next_field().await {
            Ok(Some(field)) => {
                let name = field.name()?.to_owned();
                form.push((name, field.bytes().await.ok()?.to_vec()));
            }
            Ok(None) => return Some(form),
            Err(_) => return None,
        }
    }
}

fn not_found() -> Response {
    reply(StatusCode::NOT_FOUND, "no such mail provider")
}

fn reply(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({ "error": message }))).into_response()
}
