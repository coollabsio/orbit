//! Resend inbound: the `email.received` webhook (signed by Svix) names the message; the server then
//! fetches it with the Receiving API (`GET /emails/receiving/{id}`, `raw.download_url`).
//! <https://resend.com/docs/webhooks/emails/received> <https://docs.svix.com/receiving/verifying-payloads/how-manual>

use base64::Engine as _;
use hmac::{Hmac, Mac};
use serde::Deserialize;
use sha2::Sha256;

use super::{InboundError, address, fresh, recipients};

/// A received message, before its raw bytes are fetched.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResendEvent {
    pub email_id: String,
    /// `received_for`: the envelope recipients.
    pub recipients: Vec<String>,
    pub mail_from: Option<String>,
}

#[derive(Deserialize)]
struct Event {
    #[serde(rename = "type")]
    kind: String,
    data: Data,
}

#[derive(Deserialize)]
struct Data {
    email_id: String,
    from: Option<String>,
    #[serde(default)]
    received_for: Vec<String>,
}

/// Checks the Svix headers (`svix-id`, `svix-timestamp`, `svix-signature`) with the endpoint secret
/// (`whsec_…`), then reads the event. Other event types are `Malformed("event_type")`.
pub fn verify_webhook(
    id: Option<&str>,
    timestamp: Option<&str>,
    signature: Option<&str>,
    body: &[u8],
    secret: &str,
    now: i64,
) -> Result<ResendEvent, InboundError> {
    let (Some(id), Some(timestamp), Some(signature)) = (id, timestamp, signature) else {
        return Err(InboundError::Unauthorized);
    };
    let key = base64::engine::general_purpose::STANDARD
        .decode(
            secret
                .trim()
                .strip_prefix("whsec_")
                .unwrap_or(secret.trim()),
        )
        .map_err(|_| InboundError::Unauthorized)?;
    let mut signed = format!("{id}.{timestamp}.").into_bytes();
    signed.extend_from_slice(body);
    let valid = signature.split_ascii_whitespace().any(|entry| {
        let Some(encoded) = entry.strip_prefix("v1,") else {
            return false;
        };
        let Ok(given) = base64::engine::general_purpose::STANDARD.decode(encoded) else {
            return false;
        };
        let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(&key) else {
            return false;
        };
        mac.update(&signed);
        mac.verify_slice(&given).is_ok()
    });
    if !valid {
        return Err(InboundError::Unauthorized);
    }
    fresh(timestamp, now)?;
    let event: Event = serde_json::from_slice(body).map_err(|_| InboundError::Malformed("json"))?;
    if event.kind != "email.received" {
        return Err(InboundError::Malformed("event_type"));
    }
    let data = event.data;
    // the headers' To and Cc are not the envelope (a Bcc copy, a mailing list): without
    // `received_for` there is nothing to deliver to
    if data.received_for.is_empty() {
        return Err(InboundError::Malformed("no_envelope"));
    }
    let envelope: Vec<&str> = data.received_for.iter().map(String::as_str).collect();
    Ok(ResendEvent {
        email_id: data.email_id,
        recipients: recipients(&envelope),
        mail_from: address(data.from.as_deref()),
    })
}
