//! Bounce, complaint and delivery events of the send routes' providers (spec §5.5, §5.7): each
//! request is authenticated first with the route's secret, then read into [`ParsedEvent`]s. No I/O:
//! the server passes the request's headers and body (and, for SES, the SNS certificate it fetched).
//!
//! | Kind | Authentication | Correlation |
//! |---|---|---|
//! | PostShiba | `X-Capsule-Signature` = HMAC-SHA256 of `{timestamp}.{body}` | `unique_args.orbit` |
//! | Mailgun | `signature` object: HMAC-SHA256 of `timestamp` + `token` | `user-variables.orbit` |
//! | Resend | Svix: `svix-signature` over `{svix-id}.{svix-timestamp}.{body}` | `tags.orbit`, `message_id` |
//! | Postmark | HTTP basic auth with the credentials Orbit made (Postmark does not sign) | `Metadata.orbit` |
//! | SES | SNS signature (slice 1's check), the route's topic only | `mail.messageId` |
//! | Cloudflare | Orbit's Worker: `x-orbit-signature` = HMAC of `{x-orbit-timestamp}.{body}` | `messageId` |
//!
//! A signed timestamp must be within [`REPLAY_WINDOW_SECS`] (5 minutes); SNS messages keep their
//! signing time across retries and get [`ses::SNS_WINDOW_SECS`]. An event type Orbit does not use
//! (opens, `processed`, unsubscribes, …) gives no event, so the server answers 200.
//!
//! Sources (2026-11):
//! - PostShiba: <https://www.postshiba.com/docs/webhooks/event-types>,
//!   <https://www.postshiba.com/docs/webhooks/verify-webhooks-requests>
//! - Mailgun: <https://documentation.mailgun.com/docs/mailgun/user-manual/webhooks/webhook-payloads>,
//!   <https://documentation.mailgun.com/docs/mailgun/user-manual/webhooks/securing-webhooks>
//! - Resend: <https://resend.com/docs/webhooks/emails/bounced>,
//!   <https://resend.com/docs/webhooks/verify-webhooks-requests.md>
//! - Postmark: <https://postmarkapp.com/developer/webhooks/bounce-webhook>,
//!   <https://postmarkapp.com/developer/webhooks/spam-complaint-webhook>,
//!   <https://postmarkapp.com/developer/webhooks/delivery-webhook>,
//!   <https://postmarkapp.com/developer/api/bounce-api#bounce-types>
//! - SES: <https://docs.aws.amazon.com/ses/latest/dg/notification-contents.html>
//! - Cloudflare: <https://developers.cloudflare.com/email-service/reference/event-subscriptions/>

use std::str::FromStr;

use serde::Deserialize;
use serde_json::Value;

use crate::inbound::{
    InboundError, REPLAY_WINDOW_SECS, fresh, postmark, postshiba, resend, ses, verify_hex_hmac,
};

/// What a provider reports about one recipient.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProviderEvent {
    Delivered,
    /// Not delivered, final. `hard`: the provider calls it permanent (a hard bounce); otherwise it
    /// gave up after temporary failures. `diagnostic`: the remote server's reply or the provider's
    /// reason, when it gives one. A policy refusal without a status code gets `5.7.1` in front
    /// (delivery not authorized), so the suppression rule does not take it for a bad address.
    Bounced {
        hard: bool,
        diagnostic: Option<String>,
    },
    /// The recipient marked the message as spam.
    Complained,
    /// A temporary failure; the provider keeps trying.
    Deferred {
        diagnostic: Option<String>,
    },
}

/// One event, for one recipient.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedEvent {
    /// The provider's ID of the sent message, without `<>`.
    pub provider_message_id: Option<String>,
    /// The correlation value Orbit put on the message (`orbit` in the provider's metadata).
    pub correlation: Option<String>,
    /// The `Message-ID` header of the sent message, without `<>`, when the provider reports it.
    pub message_id: Option<String>,
    /// The recipient's address, as the provider reports it.
    pub recipient: String,
    /// When it happened (milliseconds since 1970), when the provider says.
    pub at_ms: Option<i64>,
    pub event: ProviderEvent,
}

/// The providers that report events.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind {
    Postshiba,
    Mailgun,
    Resend,
    Postmark,
    Ses,
    Cloudflare,
}

impl FromStr for EventKind {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Ok(match value {
            "postshiba" => Self::Postshiba,
            "mailgun" => Self::Mailgun,
            "resend" => Self::Resend,
            "postmark" => Self::Postmark,
            "ses" => Self::Ses,
            "cloudflare" => Self::Cloudflare,
            _ => return Err(()),
        })
    }
}

/// The route's secret for its events.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EventSecret {
    /// PostShiba's webhook secret, Mailgun's webhook signing key, Resend's `whsec_…` secret, the
    /// Cloudflare Worker's event secret.
    Hmac(String),
    /// Postmark: the basic auth credentials in the webhook URL.
    Basic { user: String, password: String },
    /// SES: the route's SNS topic, and the PEM certificate at the message's `SigningCertURL` (the
    /// server fetched it after [`sns_cert_url`]).
    Sns {
        topic_arn: String,
        cert_pem: Vec<u8>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum EventError {
    /// Missing or wrong credentials or signature, or the wrong SNS topic.
    #[error("the request is not signed by the provider")]
    Unauthorized,
    /// A signed timestamp outside the window.
    #[error("the request is too old")]
    Replay,
    #[error("the request is not an event: {0}")]
    Malformed(&'static str),
}

impl From<InboundError> for EventError {
    fn from(error: InboundError) -> Self {
        match error {
            InboundError::Unauthorized => Self::Unauthorized,
            InboundError::Replay => Self::Replay,
            InboundError::TooLarge => Self::Malformed("too_large"),
            InboundError::Malformed(what) => Self::Malformed(what),
        }
    }
}

/// A verified SNS request: a subscription to confirm (the server fetches `subscribe_url`, an SNS
/// URL), or events.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SnsEvents {
    Confirm { subscribe_url: String },
    Events(Vec<ParsedEvent>),
}

/// A header value by name (any case).
fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

/// Authenticates the request with `secret`, then reads its events. `now_s`: seconds since 1970.
/// SES goes through [`verify_sns_events`] (a subscription confirmation gives no events here).
pub fn verify_and_parse(
    kind: EventKind,
    headers: &[(String, String)],
    body: &[u8],
    secret: &EventSecret,
    now_s: i64,
) -> Result<Vec<ParsedEvent>, EventError> {
    match (kind, secret) {
        (EventKind::Postshiba, EventSecret::Hmac(secret)) => {
            postshiba::verify_capsule(
                header(headers, "x-capsule-timestamp"),
                header(headers, "x-capsule-signature"),
                body,
                secret,
                now_s,
            )?;
            postshiba_events(body)
        }
        (EventKind::Mailgun, EventSecret::Hmac(key)) => mailgun_events(body, key, now_s),
        (EventKind::Resend, EventSecret::Hmac(secret)) => {
            resend::verify_svix(
                header(headers, "svix-id"),
                header(headers, "svix-timestamp"),
                header(headers, "svix-signature"),
                body,
                secret,
                now_s,
            )?;
            resend_events(body)
        }
        (EventKind::Postmark, EventSecret::Basic { user, password }) => {
            postmark::check_auth(header(headers, "authorization"), user, password)?;
            postmark_events(body)
        }
        (
            EventKind::Ses,
            EventSecret::Sns {
                topic_arn,
                cert_pem,
            },
        ) => match verify_sns_events(body, topic_arn, cert_pem, now_s)? {
            SnsEvents::Confirm { .. } => Ok(Vec::new()),
            SnsEvents::Events(events) => Ok(events),
        },
        (EventKind::Cloudflare, EventSecret::Hmac(secret)) => {
            let (Some(timestamp), Some(signature)) = (
                header(headers, "x-orbit-timestamp"),
                header(headers, "x-orbit-signature"),
            ) else {
                return Err(EventError::Unauthorized);
            };
            let mut signed = format!("{}.", timestamp.trim()).into_bytes();
            signed.extend_from_slice(body);
            verify_hex_hmac(secret.as_bytes(), &signed, signature)?;
            fresh(timestamp, now_s)?;
            cloudflare_events(body)
        }
        _ => Err(EventError::Unauthorized),
    }
}

/// The `SigningCertURL` of an SNS request (not verified yet) of the route's topic, if it is an SNS
/// certificate URL ([`ses::cert_url_ok`]); the server fetches it, then calls [`verify_sns_events`].
/// A request of another topic is refused before anything is fetched.
pub fn sns_cert_url(body: &[u8], topic_arn: &str) -> Result<String, EventError> {
    let envelope = ses::parse_envelope(body)?;
    if topic_arn.trim().is_empty() || envelope.topic_arn != topic_arn.trim() {
        return Err(EventError::Unauthorized);
    }
    if ses::cert_url_ok(&envelope.signed.cert_url) {
        Ok(envelope.signed.cert_url)
    } else {
        Err(EventError::Unauthorized)
    }
}

/// Checks an SNS request as slice 1 does: the route's topic only (SNS signs every topic's messages,
/// and Orbit confirms subscriptions by itself), the signature with `cert_pem`, the signing time;
/// then reads the SES notification (`notificationType`, or `eventType` of a configuration set's
/// event destination).
pub fn verify_sns_events(
    body: &[u8],
    topic_arn: &str,
    cert_pem: &[u8],
    now_s: i64,
) -> Result<SnsEvents, EventError> {
    let envelope = ses::parse_envelope(body)?;
    if topic_arn.trim().is_empty() || envelope.topic_arn != topic_arn.trim() {
        return Err(EventError::Unauthorized);
    }
    if !ses::cert_url_ok(&envelope.signed.cert_url) {
        return Err(EventError::Unauthorized);
    }
    ses::verify_signed(&envelope.signed, cert_pem)?;
    ses::fresh_signed(&envelope.signed, now_s)?;
    if envelope.kind == "SubscriptionConfirmation" {
        let subscribe_url = envelope
            .subscribe_url
            .filter(|url| ses::sns_url_ok(url))
            .ok_or(EventError::Malformed("subscribe_url"))?;
        return Ok(SnsEvents::Confirm { subscribe_url });
    }
    ses_events(&envelope.message).map(SnsEvents::Events)
}

// ------------------------------------------------------------------------------------ helpers

/// `value` without `<>` and spaces; `None` when empty.
fn bare_id(value: Option<&str>) -> Option<String> {
    let value = value?.trim().trim_start_matches('<').trim_end_matches('>');
    Some(value.trim().to_owned()).filter(|value| !value.is_empty())
}

/// An address as reported (`a@b`, `<a@b>`, `Name <a@b>`); `None` when empty.
fn recipient(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    let value = match (value.rfind('<'), value.rfind('>')) {
        (Some(start), Some(end)) if start < end => &value[start + 1..end],
        _ => value,
    };
    Some(value.trim().to_owned()).filter(|value| value.contains('@'))
}

/// A diagnostic text: the address type (`smtp;`) dropped, trimmed; `None` when empty.
fn diagnostic(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    let value = match value.split_once(';') {
        Some((kind, rest)) if kind.trim().eq_ignore_ascii_case("smtp") => rest.trim(),
        _ => value,
    };
    Some(value.to_owned()).filter(|value| !value.is_empty())
}

/// Whether `text` has an enhanced status code (RFC 3463, `5.1.1`).
fn has_enhanced(text: &str) -> bool {
    text.split_whitespace().any(|token| {
        let token = token.trim_matches(|c: char| !c.is_ascii_alphanumeric());
        let parts: Vec<&str> = token.split('.').collect();
        parts.len() == 3
            && matches!(parts[0], "2" | "4" | "5")
            && parts[1..].iter().all(|part| {
                (1..=3).contains(&part.len()) && part.bytes().all(|b| b.is_ascii_digit())
            })
    })
}

/// A policy refusal (a block, DMARC, spam): `5.7.1` in front unless it has a status code.
fn policy(text: Option<String>) -> Option<String> {
    match text {
        Some(text) if has_enhanced(&text) => Some(text),
        Some(text) => Some(format!("5.7.1 {text}")),
        None => Some("5.7.1 The message was refused by policy".to_owned()),
    }
}

/// `code` and `text` together, `code` only when `text` does not start with it.
fn with_code(code: Option<&str>, text: Option<String>) -> Option<String> {
    match (code.map(str::trim).filter(|code| !code.is_empty()), text) {
        (Some(code), Some(text)) if !text.starts_with(code) => Some(format!("{code} {text}")),
        (Some(code), None) => Some(code.to_owned()),
        (_, text) => text,
    }
}

/// The `orbit` value of a metadata object (a string or a number).
fn orbit_value(value: Option<&Value>) -> Option<String> {
    let value = value?.get("orbit")?;
    match value {
        Value::String(text) => Some(text.trim().to_owned()).filter(|text| !text.is_empty()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

fn text<'a>(value: &'a Value, path: &[&str]) -> Option<&'a str> {
    path.iter()
        .try_fold(value, |value, key| value.get(*key))?
        .as_str()
}

/// Milliseconds of `YYYY-MM-DDTHH:MM:SS[.fff…](Z|±HH:MM)`.
fn iso_millis(value: &str) -> Option<i64> {
    let value = value.trim();
    let bytes = value.as_bytes();
    if bytes.len() < 20 || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    if !matches!(bytes[10], b'T' | b' ') || bytes[13] != b':' || bytes[16] != b':' {
        return None;
    }
    let number = |from: usize, to: usize| -> Option<i64> {
        let part = value.get(from..to)?;
        part.bytes()
            .all(|byte| byte.is_ascii_digit())
            .then(|| part.parse().ok())
            .flatten()
    };
    let (year, month, day) = (number(0, 4)?, number(5, 7)?, number(8, 10)?);
    let (hour, minute, second) = (number(11, 13)?, number(14, 16)?, number(17, 19)?);
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }
    let mut rest = &value[19..];
    let mut millis = 0;
    if let Some(fraction) = rest.strip_prefix('.') {
        let digits = fraction.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 {
            return None;
        }
        let first: String = fraction[..digits]
            .chars()
            .chain("000".chars())
            .take(3)
            .collect();
        millis = first.parse::<i64>().ok()?;
        rest = &fraction[digits..];
    }
    let offset = match rest {
        "Z" | "z" => 0,
        _ => {
            let sign = match rest.as_bytes().first()? {
                b'+' => 1,
                b'-' => -1,
                _ => return None,
            };
            let zone = &rest[1..];
            let (hours, minutes) = zone
                .split_once(':')
                .unwrap_or((zone.get(..2)?, zone.get(2..)?));
            let (hours, minutes): (i64, i64) = (hours.parse().ok()?, minutes.parse().ok()?);
            if hours > 23 || minutes > 59 {
                return None;
            }
            sign * (hours * 3_600 + minutes * 60)
        }
    };
    // days from 1970-01-01 (Howard Hinnant's days_from_civil)
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    let seconds = days * 86_400 + hour * 3_600 + minute * 60 + second - offset;
    Some(seconds * 1000 + millis)
}

/// Seconds since 1970 as a JSON number (an integer or a fraction) to milliseconds.
fn epoch_millis(value: Option<&Value>) -> Option<i64> {
    let seconds = value?.as_f64()?;
    #[allow(clippy::cast_possible_truncation)]
    (seconds.is_finite() && seconds > 0.0).then_some((seconds * 1000.0) as i64)
}

fn json(body: &[u8]) -> Result<Value, EventError> {
    serde_json::from_slice(body).map_err(|_| EventError::Malformed("json"))
}

// ---------------------------------------------------------------------------------- PostShiba

/// A JSON array of SendGrid-style events (one per request).
fn postshiba_events(body: &[u8]) -> Result<Vec<ParsedEvent>, EventError> {
    let value = json(body)?;
    let items = match &value {
        Value::Array(items) => items.as_slice(),
        Value::Object(_) => std::slice::from_ref(&value),
        _ => return Err(EventError::Malformed("json")),
    };
    let mut events = Vec::new();
    for item in items {
        let Some(address) = recipient(text(item, &["email"])) else {
            continue;
        };
        let response = diagnostic(text(item, &["response"]).or_else(|| text(item, &["reason"])));
        let response = with_code(
            text(item, &["status"])
                .filter(|status| response.as_deref().is_none_or(|r| !r.contains(status))),
            response,
        );
        let event = match text(item, &["event"]).unwrap_or_default() {
            "delivered" => ProviderEvent::Delivered,
            "deferred" => ProviderEvent::Deferred {
                diagnostic: response,
            },
            // `type`: `bounce` (a bad address) or `blocked` (the message was refused)
            "bounce" if text(item, &["type"]) == Some("blocked") => ProviderEvent::Bounced {
                hard: true,
                diagnostic: policy(response),
            },
            // `dropped`: not sent, as for an address on PostShiba's suppression list
            "bounce" | "dropped" => ProviderEvent::Bounced {
                hard: true,
                diagnostic: response,
            },
            "spamreport" => ProviderEvent::Complained,
            _ => continue,
        };
        events.push(ParsedEvent {
            provider_message_id: bare_id(text(item, &["sg_message_id"])),
            correlation: orbit_value(item.get("unique_args")).or_else(|| orbit_value(Some(item))),
            message_id: bare_id(text(item, &["smtp-id"])),
            recipient: address,
            at_ms: epoch_millis(item.get("timestamp")),
            event,
        });
    }
    Ok(events)
}

// ------------------------------------------------------------------------------------ Mailgun

#[derive(Deserialize)]
struct MailgunBody {
    signature: MailgunSignature,
    #[serde(rename = "event-data")]
    event_data: Value,
}

#[derive(Deserialize)]
struct MailgunSignature {
    timestamp: Value,
    token: String,
    signature: String,
}

/// `{"signature": {timestamp, token, signature}, "event-data": {…}}`.
fn mailgun_events(body: &[u8], key: &str, now_s: i64) -> Result<Vec<ParsedEvent>, EventError> {
    let parsed: MailgunBody = serde_json::from_slice(body).map_err(|_| EventError::Unauthorized)?;
    let timestamp = match &parsed.signature.timestamp {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        _ => return Err(EventError::Unauthorized),
    };
    verify_hex_hmac(
        key.trim().as_bytes(),
        format!("{timestamp}{}", parsed.signature.token).as_bytes(),
        &parsed.signature.signature,
    )?;
    if (now_s
        - timestamp
            .trim()
            .parse::<i64>()
            .map_err(|_| EventError::Unauthorized)?)
    .abs()
        > REPLAY_WINDOW_SECS
    {
        return Err(EventError::Replay);
    }
    let data = &parsed.event_data;
    let Some(address) = recipient(text(data, &["recipient"])) else {
        return Ok(Vec::new());
    };
    let status = data.get("delivery-status");
    let code = status
        .and_then(|status| status.get("code"))
        .and_then(|code| match code {
            Value::Number(number) => Some(number.to_string()),
            Value::String(text) => Some(text.clone()),
            _ => None,
        })
        .filter(|code| code != "0");
    let message = status.and_then(|status| {
        diagnostic(text(status, &["message"]).filter(|m| !m.trim().is_empty()))
            .or_else(|| diagnostic(text(status, &["description"])))
    });
    let enhanced = status.and_then(|status| text(status, &["enhanced-code"]));
    let message = with_code(
        enhanced.filter(|code| message.as_deref().is_none_or(|m| !m.contains(code))),
        message,
    );
    let response = with_code(code.as_deref(), message);
    let event = match text(data, &["event"]).unwrap_or_default() {
        "delivered" => ProviderEvent::Delivered,
        "complained" => ProviderEvent::Complained,
        "failed" | "permanent_fail" | "temporary_fail" => {
            let permanent = text(data, &["severity"]) == Some("permanent")
                || text(data, &["event"]) == Some("permanent_fail");
            if !permanent {
                ProviderEvent::Deferred {
                    diagnostic: response,
                }
            } else {
                match text(data, &["reason"]).unwrap_or_default() {
                    // gave up after its retries: not a bad address
                    "old" => ProviderEvent::Bounced {
                        hard: false,
                        diagnostic: response,
                    },
                    "espblock" => ProviderEvent::Bounced {
                        hard: true,
                        diagnostic: policy(response),
                    },
                    // `bounce`, `generic`, `suppress-*` (on Mailgun's own lists)
                    _ => ProviderEvent::Bounced {
                        hard: true,
                        diagnostic: response,
                    },
                }
            }
        }
        _ => return Ok(Vec::new()),
    };
    let message_id = bare_id(text(data, &["message", "headers", "message-id"]));
    Ok(vec![ParsedEvent {
        provider_message_id: message_id.clone(),
        // `user-variables` is `[]` when the message had none
        correlation: orbit_value(data.get("user-variables")),
        message_id,
        recipient: address,
        at_ms: epoch_millis(data.get("timestamp")),
        event,
    }])
}

// ------------------------------------------------------------------------------------- Resend

/// `{"type": "email.…", "created_at": …, "data": {email_id, message_id, to: […], bounce, tags}}`.
fn resend_events(body: &[u8]) -> Result<Vec<ParsedEvent>, EventError> {
    let value = json(body)?;
    let data = value.get("data").unwrap_or(&Value::Null);
    let bounce = data.get("bounce");
    let reason = bounce.and_then(|bounce| {
        bounce
            .get("diagnosticCode")
            .and_then(|codes| match codes {
                Value::Array(codes) => codes.iter().find_map(|code| diagnostic(code.as_str())),
                Value::String(code) => diagnostic(Some(code)),
                _ => None,
            })
            .or_else(|| diagnostic(text(bounce, &["message"])))
    });
    let event = match text(&value, &["type"]).unwrap_or_default() {
        "email.delivered" => ProviderEvent::Delivered,
        "email.complained" => ProviderEvent::Complained,
        "email.delivery_delayed" => ProviderEvent::Deferred { diagnostic: None },
        "email.bounced" => {
            let permanent = bounce.and_then(|bounce| text(bounce, &["type"])) == Some("Permanent");
            ProviderEvent::Bounced {
                hard: permanent,
                diagnostic: reason,
            }
        }
        // not sent: the address is on Resend's suppression list
        "email.suppressed" => ProviderEvent::Bounced {
            hard: true,
            diagnostic: reason,
        },
        "email.failed" => ProviderEvent::Bounced {
            hard: false,
            diagnostic: diagnostic(text(data, &["failed", "reason"])),
        },
        _ => return Ok(Vec::new()),
    };
    // `tags` is an object; a list of `{name, value}` is read too
    let correlation = match data.get("tags") {
        Some(Value::Array(tags)) => tags.iter().find_map(|tag| {
            (text(tag, &["name"]) == Some("orbit"))
                .then(|| text(tag, &["value"]).map(str::to_owned))
                .flatten()
        }),
        other => orbit_value(other),
    };
    let at_ms = text(&value, &["created_at"]).and_then(iso_millis);
    let addresses: Vec<String> = match data.get("to") {
        Some(Value::Array(to)) => to.iter().filter_map(|to| recipient(to.as_str())).collect(),
        Some(Value::String(to)) => recipient(Some(to)).into_iter().collect(),
        _ => Vec::new(),
    };
    Ok(addresses
        .into_iter()
        .map(|address| ParsedEvent {
            provider_message_id: bare_id(text(data, &["email_id"])),
            correlation: correlation.clone(),
            message_id: bare_id(text(data, &["message_id"])),
            recipient: address,
            at_ms,
            event: event.clone(),
        })
        .collect())
}

// ----------------------------------------------------------------------------------- Postmark

/// One webhook: `RecordType` `Bounce`, `SpamComplaint` or `Delivery`.
fn postmark_events(body: &[u8]) -> Result<Vec<ParsedEvent>, EventError> {
    let value = json(body)?;
    let details = diagnostic(text(&value, &["Details"]))
        .or_else(|| diagnostic(text(&value, &["Description"])));
    let type_code = value.get("TypeCode").and_then(Value::as_i64);
    let (event, address, at) = match text(&value, &["RecordType"]).unwrap_or_default() {
        "Delivery" => (
            ProviderEvent::Delivered,
            text(&value, &["Recipient"]),
            text(&value, &["DeliveredAt"]),
        ),
        "SpamComplaint" => (
            ProviderEvent::Complained,
            text(&value, &["Email"]),
            text(&value, &["BouncedAt"]),
        ),
        "Bounce" => {
            let event = match type_code {
                // HardBounce, BadEmailAddress, ManuallyDeactivated (Postmark will not send to it)
                Some(1 | 100_000 | 100_002) => ProviderEvent::Bounced {
                    hard: true,
                    diagnostic: details,
                },
                // Blocked (ISP block), DMARCPolicy
                Some(100_006 | 100_009) => ProviderEvent::Bounced {
                    hard: true,
                    diagnostic: policy(details),
                },
                Some(100_001) => ProviderEvent::Complained,
                // Transient: delayed, Postmark keeps trying
                Some(2) => ProviderEvent::Deferred {
                    diagnostic: details,
                },
                // DnsError, SoftBounce: not delivered, not a bad address
                Some(256 | 4096) => ProviderEvent::Bounced {
                    hard: false,
                    diagnostic: details,
                },
                // auto replies, unsubscribe requests, challenges, unknown: no change
                _ => return Ok(Vec::new()),
            };
            (
                event,
                text(&value, &["Email"]),
                text(&value, &["BouncedAt"]),
            )
        }
        _ => return Ok(Vec::new()),
    };
    let Some(address) = recipient(address) else {
        return Ok(Vec::new());
    };
    Ok(vec![ParsedEvent {
        provider_message_id: bare_id(text(&value, &["MessageID"])),
        correlation: orbit_value(value.get("Metadata")),
        message_id: None,
        recipient: address,
        at_ms: at.and_then(iso_millis),
        event,
    }])
}

// ---------------------------------------------------------------------------------------- SES

/// An SES notification (`notificationType`) or event (`eventType`): Bounce, Complaint, Delivery,
/// DeliveryDelay.
fn ses_events(message: &str) -> Result<Vec<ParsedEvent>, EventError> {
    let value: Value =
        serde_json::from_str(message).map_err(|_| EventError::Malformed("ses_notification"))?;
    let kind = text(&value, &["notificationType"])
        .or_else(|| text(&value, &["eventType"]))
        .unwrap_or_default();
    let mail = value.get("mail").unwrap_or(&Value::Null);
    let provider_message_id = bare_id(text(mail, &["messageId"]));
    let message_id = bare_id(text(mail, &["commonHeaders", "messageId"]));
    let list = |section: &str, field: &str| -> Vec<Value> {
        match value.get(section).and_then(|section| section.get(field)) {
            Some(Value::Array(items)) => items.clone(),
            _ => Vec::new(),
        }
    };
    let at = |section: &str| {
        value
            .get(section)
            .and_then(|section| text(section, &["timestamp"]))
            .and_then(iso_millis)
    };
    let reported = |item: &Value| {
        with_code(
            text(item, &["status"]).filter(|status| {
                text(item, &["diagnosticCode"]).is_none_or(|code| !code.contains(status))
            }),
            diagnostic(text(item, &["diagnosticCode"])),
        )
    };
    let mut events = Vec::new();
    let mut push = |address: Option<String>, at_ms: Option<i64>, event: ProviderEvent| {
        if let Some(address) = address {
            events.push(ParsedEvent {
                provider_message_id: provider_message_id.clone(),
                correlation: None,
                message_id: message_id.clone(),
                recipient: address,
                at_ms,
                event,
            });
        }
    };
    match kind {
        "Bounce" => {
            let bounce = value.get("bounce").unwrap_or(&Value::Null);
            // Transient and Undetermined: SES stopped trying; not a bad address
            let hard = text(bounce, &["bounceType"]) == Some("Permanent");
            for item in list("bounce", "bouncedRecipients") {
                push(
                    recipient(text(&item, &["emailAddress"])),
                    at("bounce"),
                    ProviderEvent::Bounced {
                        hard,
                        diagnostic: reported(&item),
                    },
                );
            }
        }
        "Complaint" => {
            for item in list("complaint", "complainedRecipients") {
                push(
                    recipient(text(&item, &["emailAddress"])),
                    at("complaint"),
                    ProviderEvent::Complained,
                );
            }
        }
        "Delivery" => {
            for item in list("delivery", "recipients") {
                push(
                    recipient(item.as_str()),
                    at("delivery"),
                    ProviderEvent::Delivered,
                );
            }
        }
        "DeliveryDelay" => {
            for item in list("deliveryDelay", "delayedRecipients") {
                push(
                    recipient(text(&item, &["emailAddress"])),
                    at("deliveryDelay"),
                    ProviderEvent::Deferred {
                        diagnostic: reported(&item),
                    },
                );
            }
        }
        _ => {}
    }
    Ok(events)
}

// --------------------------------------------------------------------------------- Cloudflare

/// A JSON array of queue message bodies (Orbit's Worker posts each batch), or one body.
fn cloudflare_events(body: &[u8]) -> Result<Vec<ParsedEvent>, EventError> {
    let value = json(body)?;
    let items = match &value {
        Value::Array(items) => items.as_slice(),
        Value::Object(_) => std::slice::from_ref(&value),
        _ => return Err(EventError::Malformed("json")),
    };
    let mut events = Vec::new();
    for item in items {
        let kind = text(item, &["type"]).unwrap_or_default();
        let Some(kind) = kind.strip_prefix("cf.email.sending.") else {
            continue;
        };
        let payload = item.get("payload").unwrap_or(&Value::Null);
        let Some(address) = recipient(text(payload, &["recipient"])) else {
            continue;
        };
        let response = diagnostic(text(payload, &["delivery", "smtpResponse"]))
            .or_else(|| diagnostic(text(payload, &["bounce", "reason"])));
        let event = match kind {
            "message.delivered" => ProviderEvent::Delivered,
            "message.deferred" => ProviderEvent::Deferred {
                diagnostic: response,
            },
            // `bounce.type` `soft`: its retries ran out
            "message.bounced" => ProviderEvent::Bounced {
                hard: text(payload, &["bounce", "type"]) != Some("soft"),
                diagnostic: response,
            },
            "message.failed" => ProviderEvent::Bounced {
                hard: false,
                diagnostic: diagnostic(text(payload, &["failure", "reason"])),
            },
            "message.rejected" => {
                let detail = diagnostic(text(payload, &["rejection", "detail"]))
                    .or_else(|| diagnostic(text(payload, &["rejection", "reason"])));
                if text(payload, &["rejection", "reason"]) == Some("suppressed") {
                    ProviderEvent::Bounced {
                        hard: true,
                        diagnostic: detail,
                    }
                } else {
                    ProviderEvent::Bounced {
                        hard: true,
                        diagnostic: policy(detail),
                    }
                }
            }
            "message.complained" => ProviderEvent::Complained,
            _ => continue,
        };
        events.push(ParsedEvent {
            provider_message_id: bare_id(text(payload, &["messageId"])),
            correlation: None,
            message_id: None,
            recipient: address,
            at_ms: text(item, &["metadata", "eventTimestamp"]).and_then(iso_millis),
            event,
        });
    }
    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::{has_enhanced, iso_millis, policy};

    #[test]
    fn reads_iso_times_with_zones() {
        assert_eq!(iso_millis("1970-01-01T00:00:01Z"), Some(1000));
        assert_eq!(iso_millis("1970-01-01T00:00:01.5Z"), Some(1500));
        assert_eq!(iso_millis("1970-01-01T01:00:01.2345+01:00"), Some(1234));
        assert_eq!(
            iso_millis("2019-11-05T16:33:54.9070259Z"),
            Some(1_572_971_634_907)
        );
        assert_eq!(
            iso_millis("2024-11-22T23:41:11.894719+00:00"),
            Some(1_732_318_871_894)
        );
        assert_eq!(iso_millis("yesterday"), None);
    }

    #[test]
    fn policy_refusals_get_a_policy_status() {
        assert!(has_enhanced("550 5.7.1 blocked"));
        assert!(!has_enhanced("550 blocked 2024"));
        assert_eq!(
            policy(Some("550 5.7.1 x".to_owned())).unwrap(),
            "550 5.7.1 x"
        );
        assert_eq!(policy(Some("blocked".to_owned())).unwrap(), "5.7.1 blocked");
    }
}
