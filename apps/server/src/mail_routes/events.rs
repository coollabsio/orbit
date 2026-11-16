//! `POST /api/v1/mail/events/{kind}/{route_id}`: bounce, complaint and delivery events of a send
//! route's provider (spec §5.5, §5.7). No session: the route's secret authenticates the request
//! (`orbit_mail::events`). Answers: 200 once handled, for events Orbit does not know too (unknown
//! messages, other types, events older than a retry), so the provider stops sending them; 401 for
//! a bad signature or credentials or an old timestamp; 404 for no such route of the kind; 413 for
//! a body over the kind's limit; 503 to have the provider send it again (the database, the SNS
//! certificate, or a bounce of a recipient still being sent). Neither secrets nor payloads are
//! logged.

use axum::Json;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use orbit_mail::events::{
    self, EventError, EventKind, EventSecret, ParsedEvent, SnsEvents, verify_and_parse,
};
use orbit_platform::{Id, TimestampMillis};
use serde_json::json;

use super::MailState;
use crate::repositories::mail::{EventOutcome, RouteSecrets, SendRouteKind};

/// The largest event request of each kind: one small JSON event (an SNS envelope can carry
/// 256 KiB), a batch of up to 100 queue messages from Cloudflare's Worker.
fn body_limit(kind: EventKind) -> usize {
    const KIB: usize = 1024;
    match kind {
        EventKind::Resend => 256 * KIB,
        EventKind::Ses => 512 * KIB,
        EventKind::Postshiba | EventKind::Mailgun | EventKind::Postmark | EventKind::Cloudflare => {
            1024 * KIB
        }
    }
}

/// Events of a send route's provider. The body is the provider's own format.
#[utoipa::path(
    post,
    path = "/api/v1/mail/events/{kind}/{route_id}",
    params(("kind" = String, Path, description = "postshiba, mailgun, resend, postmark, ses or cloudflare"), ("route_id" = String, Path)),
    request_body(content = String, content_type = "application/json", description = "The provider's own event payload"),
    responses((status = 200), (status = 400), (status = 401), (status = 404), (status = 413), (status = 503))
)]
pub(crate) async fn post_mail_events(
    State(state): State<MailState>,
    Path((kind, route_id)): Path<(String, String)>,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let (Ok(event_kind), Ok(route_kind), Ok(route_id)) = (
        kind.parse::<EventKind>(),
        kind.parse::<SendRouteKind>(),
        route_id.parse::<Id>(),
    ) else {
        return not_found();
    };
    let (route, secrets) = match state.mail.load_route(route_id).await {
        Ok(Some(found)) if found.0.kind == route_kind => found,
        Ok(_) => return not_found(),
        Err(error) => {
            tracing::warn!(%error, "mail send route lookup failed");
            return reply(StatusCode::SERVICE_UNAVAILABLE, "try again later");
        }
    };
    let header_value = |name: &str| headers.get(name).and_then(|value| value.to_str().ok());

    // what can be checked before the body is read: anyone can post to this URL
    let secret = match secrets {
        RouteSecrets::Postshiba { webhook_secret, .. } => Some(EventSecret::Hmac(webhook_secret)),
        RouteSecrets::Mailgun {
            webhook_signing_key,
            ..
        } => Some(EventSecret::Hmac(webhook_signing_key)),
        RouteSecrets::Resend { webhook_secret, .. } => Some(EventSecret::Hmac(webhook_secret)),
        RouteSecrets::Cloudflare { event_secret, .. } => Some(EventSecret::Hmac(event_secret)),
        RouteSecrets::Postmark {
            webhook_user,
            webhook_password,
            ..
        } => {
            if orbit_mail::inbound::postmark::check_auth(
                header_value(header::AUTHORIZATION.as_str()),
                &webhook_user,
                &webhook_password,
            )
            .is_err()
            {
                return unauthorized();
            }
            Some(EventSecret::Basic {
                user: webhook_user,
                password: webhook_password,
            })
        }
        // the certificate is fetched once the body names it
        RouteSecrets::Ses { .. } => None,
        RouteSecrets::None | RouteSecrets::Smtp { .. } => return not_found(),
    };
    let signed = match event_kind {
        EventKind::Postshiba => header_value("x-capsule-signature").is_some(),
        EventKind::Resend => header_value("svix-signature").is_some(),
        EventKind::Cloudflare => header_value("x-orbit-signature").is_some(),
        EventKind::Ses => route.settings.topic_arn.is_some(),
        EventKind::Mailgun | EventKind::Postmark => true,
    };
    if !signed {
        return unauthorized();
    }
    let Ok(body) = axum::body::to_bytes(body, body_limit(event_kind)).await else {
        return reply(StatusCode::PAYLOAD_TOO_LARGE, "request too large");
    };
    let now = TimestampMillis::now();
    let now_secs = now.as_millis() / 1000;

    let parsed = if let Some(secret) = &secret {
        let pairs: Vec<(String, String)> = headers
            .iter()
            .filter_map(|(name, value)| {
                Some((name.as_str().to_owned(), value.to_str().ok()?.to_owned()))
            })
            .collect();
        match verify_and_parse(event_kind, &pairs, &body, secret, now_secs) {
            Ok(events) => events,
            Err(error) => return refused(error),
        }
    } else {
        match ses_events(&state, route.settings.topic_arn.as_deref(), &body, now_secs).await {
            Ok(Ok(events)) => events,
            Ok(Err(response)) => return response,
            Err(error) => return refused(error),
        }
    };

    let (mut applied, mut ignored, mut busy) = (0, 0, false);
    for event in &parsed {
        match state.mail.apply_provider_event(route.id, event, now).await {
            Ok(EventOutcome::Applied) => applied += 1,
            Ok(EventOutcome::Ignored) => ignored += 1,
            Ok(EventOutcome::Busy) => busy = true,
            Err(error) => {
                tracing::warn!(%error, kind = kind.as_str(), "mail provider event not applied");
                return reply(StatusCode::SERVICE_UNAVAILABLE, "try again later");
            }
        }
    }
    if busy {
        // a bounce of a recipient still being sent; the others are applied, and applying them
        // again changes nothing
        return reply(StatusCode::SERVICE_UNAVAILABLE, "try again later");
    }
    (
        StatusCode::OK,
        Json(json!({ "applied": applied, "ignored": ignored })),
    )
        .into_response()
}

/// SES through SNS: the route's topic and the signature with the certificate at the message's
/// `SigningCertURL` (an SNS host); then confirms a subscription, or reads the events.
async fn ses_events(
    state: &MailState,
    topic_arn: Option<&str>,
    body: &[u8],
    now_secs: i64,
) -> Result<Result<Vec<ParsedEvent>, Response>, EventError> {
    let Some(topic_arn) = topic_arn else {
        return Err(EventError::Unauthorized);
    };
    let cert_url = events::sns_cert_url(body, topic_arn)?;
    let Some(intake) = state.inbound.as_ref() else {
        return Ok(Err(reply(
            StatusCode::SERVICE_UNAVAILABLE,
            "mail intake is not running",
        )));
    };
    let cert = match intake.cert(&cert_url).await {
        Ok(cert) => cert,
        Err(error) => {
            tracing::warn!(%error, "SNS certificate for mail events not fetched");
            return Ok(Err(reply(
                StatusCode::SERVICE_UNAVAILABLE,
                "try again later",
            )));
        }
    };
    match events::verify_sns_events(body, topic_arn, &cert, now_secs)? {
        SnsEvents::Events(events) => Ok(Ok(events)),
        SnsEvents::Confirm { subscribe_url } => {
            Ok(Err(match intake.confirm_sns(&subscribe_url).await {
                Ok(()) => (StatusCode::OK, Json(json!({ "confirmed": true }))).into_response(),
                Err(error) => {
                    tracing::warn!(%error, "SNS subscription for mail events not confirmed");
                    reply(StatusCode::SERVICE_UNAVAILABLE, "try again later")
                }
            }))
        }
    }
}

fn refused(error: EventError) -> Response {
    match error {
        EventError::Unauthorized | EventError::Replay => unauthorized(),
        EventError::Malformed(_) => reply(StatusCode::BAD_REQUEST, &error.to_string()),
    }
}

fn unauthorized() -> Response {
    reply(
        StatusCode::UNAUTHORIZED,
        "the request is not signed by the provider",
    )
}

fn not_found() -> Response {
    reply(StatusCode::NOT_FOUND, "no such mail send route")
}

fn reply(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({ "error": message }))).into_response()
}
