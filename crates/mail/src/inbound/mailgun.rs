//! Mailgun route `forward()` to an Orbit URL that ends in `mime`: the form has the raw message in
//! `body-mime`, the envelope in `recipient` and `sender`, and `timestamp`, `token` and `signature`
//! (hex HMAC-SHA256 of timestamp and token, with the webhook signing key). The server remembers
//! tokens to refuse a replay. Mailgun stops retrying on 406.
//! <https://documentation.mailgun.com/docs/mailgun/user-manual/receive-forward-store/receive-http>
//! <https://documentation.mailgun.com/docs/mailgun/user-manual/webhooks/securing-webhooks>

use super::{
    Delivery, InboundError, SpfHeaderOwned, SpfSource, address, fresh, recipients, verify_hex_hmac,
};

fn field<'a>(form: &'a [(String, String)], name: &str) -> Option<&'a str> {
    form.iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
}

/// Checks the signature and the timestamp (seconds), then reads the message. Returns the token too.
pub fn verify_and_parse(
    form: &[(String, String)],
    signing_key: &[u8],
    now: i64,
    max_raw: usize,
) -> Result<(Delivery, String), InboundError> {
    let (Some(timestamp), Some(token), Some(signature)) = (
        field(form, "timestamp"),
        field(form, "token"),
        field(form, "signature"),
    ) else {
        return Err(InboundError::Unauthorized);
    };
    verify_hex_hmac(
        signing_key,
        format!("{timestamp}{token}").as_bytes(),
        signature,
    )?;
    fresh(timestamp, now)?;
    let raw = field(form, "body-mime").ok_or(InboundError::Malformed("body_mime_missing"))?;
    if raw.len() > max_raw {
        return Err(InboundError::TooLarge);
    }
    let recipients = recipients(&[field(form, "recipient").unwrap_or_default()]);
    if recipients.is_empty() {
        return Err(InboundError::Malformed("recipient"));
    }
    Ok((
        Delivery {
            raw: raw.as_bytes().to_vec(),
            mail_from: address(field(form, "sender")),
            recipients,
            peer_ip: None,
            helo: None,
            spf: SpfSource::Header(SpfHeaderOwned::ReceivedSpf),
        },
        token.to_owned(),
    ))
}
