//! PostShiba inbound: an inbox's JSON webhook (signed with the inbox's `webhook_secret`) names the
//! message; the server then fetches its raw MIME with the Inbound messages API
//! (`GET /api/v1/inboxes/{inbox_id}/inbound_messages/{id}`, `raw`). The JSON webhook has no raw
//! message, and PostShiba keeps `raw` only until the webhook succeeds, so the fetch happens while
//! the webhook waits for its answer.
//! <https://www.postshiba.com/docs/webhooks/verify-webhooks-requests.md>
//! <https://www.postshiba.com/docs/dashboard/receiving/get-email-content.md>
//! <https://www.postshiba.com/docs/api-reference/messages.md>

use serde::Deserialize;

use super::{InboundError, address, fresh, recipients, verify_hex_hmac};
use crate::auth::{ProviderSpf, Verdict};

/// A received message, before its raw bytes are fetched.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PostshibaEvent {
    pub message_id: String,
    pub inbox_id: String,
    /// `envelope.rcpt_to`: the envelope recipients.
    pub recipients: Vec<String>,
    /// `envelope.mail_from`.
    pub mail_from: Option<String>,
    /// `authentication.spf`, which PostShiba measured on receipt.
    pub spf: Option<ProviderSpf>,
}

#[derive(Deserialize)]
struct Payload {
    id: String,
    inbox_id: String,
    envelope: Option<Envelope>,
    authentication: Option<Authentication>,
}

#[derive(Deserialize)]
struct Envelope {
    mail_from: Option<String>,
    #[serde(default)]
    rcpt_to: Recipients,
}

/// `rcpt_to` is one address in PostShiba's docs; a list is read too.
#[derive(Deserialize, Default)]
#[serde(untagged)]
enum Recipients {
    One(String),
    Many(Vec<String>),
    #[default]
    None,
}

#[derive(Deserialize)]
struct Authentication {
    spf: Option<Spf>,
}

#[derive(Deserialize)]
struct Spf {
    result: String,
    #[serde(default)]
    domain: Option<String>,
}

fn verdict(result: &str) -> Option<Verdict> {
    Some(match result.trim().to_ascii_lowercase().as_str() {
        "pass" => Verdict::Pass,
        "fail" | "hardfail" => Verdict::Fail,
        "softfail" => Verdict::SoftFail,
        "neutral" => Verdict::Neutral,
        "none" => Verdict::None,
        "temperror" => Verdict::TempError,
        "permerror" => Verdict::PermError,
        _ => return None,
    })
}

/// A PostShiba id goes into the API path: letters, digits, `-` and `_` only.
fn path_safe(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

/// Checks `X-Capsule-Signature` (`sha256=` and the hex HMAC-SHA256 of `{timestamp}.{body}`, keyed
/// with the inbox's webhook secret) and `X-Capsule-Timestamp`, then reads the event. Each attempt,
/// a retry too, is signed with a new timestamp.
pub fn verify_webhook(
    timestamp: Option<&str>,
    signature: Option<&str>,
    body: &[u8],
    secret: &str,
    now: i64,
) -> Result<PostshibaEvent, InboundError> {
    let (Some(timestamp), Some(signature)) = (timestamp, signature) else {
        return Err(InboundError::Unauthorized);
    };
    let signature = signature.trim();
    let signature = signature.strip_prefix("sha256=").unwrap_or(signature);
    let mut signed = format!("{}.", timestamp.trim()).into_bytes();
    signed.extend_from_slice(body);
    verify_hex_hmac(secret.trim().as_bytes(), &signed, signature)?;
    fresh(timestamp, now)?;

    let payload: Payload =
        serde_json::from_slice(body).map_err(|_| InboundError::Malformed("json"))?;
    if !path_safe(&payload.id) || !path_safe(&payload.inbox_id) {
        return Err(InboundError::Malformed("id"));
    }
    let envelope = payload
        .envelope
        .ok_or(InboundError::Malformed("no_envelope"))?;
    // the headers' To and Cc are not the envelope (a Bcc copy, a mailing list)
    let recipients = match &envelope.rcpt_to {
        Recipients::One(one) => recipients(&[one.as_str()]),
        Recipients::Many(many) => recipients(&many.iter().map(String::as_str).collect::<Vec<_>>()),
        Recipients::None => Vec::new(),
    };
    if recipients.is_empty() {
        return Err(InboundError::Malformed("no_envelope"));
    }
    let spf = payload
        .authentication
        .and_then(|authentication| authentication.spf)
        .and_then(|spf| {
            Some(ProviderSpf {
                verdict: verdict(&spf.result)?,
                domain: spf.domain.unwrap_or_default().trim().to_owned(),
            })
        });
    Ok(PostshibaEvent {
        message_id: payload.id,
        inbox_id: payload.inbox_id,
        recipients,
        mail_from: address(envelope.mail_from.as_deref()),
        spf,
    })
}
