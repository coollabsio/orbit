//! Postmark inbound webhook with "Include raw email content in JSON payload" on (`RawEmail`).
//! Postmark does not sign its requests: the webhook URL carries HTTP basic auth credentials that
//! Orbit made. The envelope recipient is `OriginalRecipient`. Postmark retries until a 200 and stops
//! on a 403. <https://postmarkapp.com/developer/webhooks/inbound-webhook>

use base64::Engine as _;
use serde::Deserialize;

use super::{Delivery, InboundError, SpfHeaderOwned, SpfSource, address, same};

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Body {
    #[serde(default)]
    raw_email: Option<String>,
    #[serde(default)]
    original_recipient: Option<String>,
    #[serde(default)]
    from: Option<String>,
}

/// Checks the `Authorization` header (`Basic …`), then reads the message.
pub fn verify_and_parse(
    authorization: Option<&str>,
    body: &[u8],
    user: &str,
    password: &str,
    max_raw: usize,
) -> Result<Delivery, InboundError> {
    let given = authorization
        .and_then(|value| value.strip_prefix("Basic "))
        .and_then(|value| {
            base64::engine::general_purpose::STANDARD
                .decode(value.trim())
                .ok()
        })
        .ok_or(InboundError::Unauthorized)?;
    if !same(&given, format!("{user}:{password}").as_bytes()) {
        return Err(InboundError::Unauthorized);
    }
    // JSON escapes can double a message's size
    if body.len() > max_raw * 2 + 1024 * 1024 {
        return Err(InboundError::TooLarge);
    }
    let body: Body = serde_json::from_slice(body).map_err(|_| InboundError::Malformed("json"))?;
    let raw = body
        .raw_email
        .filter(|raw| !raw.is_empty())
        .ok_or(InboundError::Malformed("raw_email_missing"))?
        .into_bytes();
    if raw.len() > max_raw {
        return Err(InboundError::TooLarge);
    }
    let recipient = address(body.original_recipient.as_deref())
        .ok_or(InboundError::Malformed("original_recipient"))?;
    Ok(Delivery {
        raw,
        // Postmark gives no envelope sender; `From` is the closest it has (only for display and SPF fallback)
        mail_from: address(body.from.as_deref()),
        recipients: vec![recipient],
        peer_ip: None,
        helo: None,
        spf: SpfSource::Header(SpfHeaderOwned::ReceivedSpf),
    })
}
