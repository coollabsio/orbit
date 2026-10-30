//! Mailgun route `forward()` to an Orbit URL that ends in `mime`: the form has the raw message in
//! `body-mime`, the envelope in `recipient` and `sender`, and `timestamp`, `token` and `signature`
//! (hex HMAC-SHA256 of timestamp and token, with the webhook signing key). Mailgun retries a failed
//! POST for 8 hours with the same signature, so the timestamp may be up to [`RETRY_WINDOW_SECS`]
//! old; the server remembers the tokens of delivered requests to refuse a replay. Mailgun stops
//! retrying on 406. The form is urlencoded or multipart; values stay bytes (a raw message need not
//! be UTF-8).
//! <https://documentation.mailgun.com/docs/mailgun/user-manual/receive-forward-store/receive-http>
//! <https://documentation.mailgun.com/docs/mailgun/user-manual/webhooks/securing-webhooks>

use super::{
    Delivery, InboundError, SpfHeaderOwned, SpfSource, address, fresh_within, recipients,
    verify_hex_hmac,
};

/// How old a Mailgun timestamp may be: its retries span 8 hours.
pub const RETRY_WINDOW_SECS: i64 = 9 * 60 * 60;

fn bytes<'a>(form: &'a [(String, Vec<u8>)], name: &str) -> Option<&'a [u8]> {
    form.iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_slice())
}

fn field<'a>(form: &'a [(String, Vec<u8>)], name: &str) -> Option<&'a str> {
    bytes(form, name).and_then(|value| std::str::from_utf8(value).ok())
}

/// Percent-decodes `part` to bytes; `+` is a space, and a `%` without two hex digits stays.
fn decode(part: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(part.len());
    let mut index = 0;
    while index < part.len() {
        let byte = part[index];
        let hex = (byte == b'%')
            .then(|| part.get(index + 1..index + 3))
            .flatten()
            .and_then(|hex| std::str::from_utf8(hex).ok())
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match (byte, hex) {
            (_, Some(decoded)) => {
                out.push(decoded);
                index += 3;
            }
            (b'+', None) => {
                out.push(b' ');
                index += 1;
            }
            (other, None) => {
                out.push(other);
                index += 1;
            }
        }
    }
    out
}

/// An `application/x-www-form-urlencoded` body, with the values as bytes.
#[must_use]
pub fn parse_urlencoded(body: &[u8]) -> Vec<(String, Vec<u8>)> {
    body.split(|&byte| byte == b'&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (key, value) = match pair.iter().position(|&byte| byte == b'=') {
                Some(at) => (&pair[..at], &pair[at + 1..]),
                None => (pair, &pair[pair.len()..]),
            };
            (
                String::from_utf8_lossy(&decode(key)).into_owned(),
                decode(value),
            )
        })
        .collect()
}

/// Checks the signature and the timestamp (seconds), then reads the message. Returns the token too.
pub fn verify_and_parse(
    form: &[(String, Vec<u8>)],
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
    fresh_within(timestamp, now, RETRY_WINDOW_SECS)?;
    let raw = bytes(form, "body-mime").ok_or(InboundError::Malformed("body_mime_missing"))?;
    if raw.len() > max_raw {
        return Err(InboundError::TooLarge);
    }
    let recipients = recipients(&[field(form, "recipient").unwrap_or_default()]);
    if recipients.is_empty() {
        return Err(InboundError::Malformed("recipient"));
    }
    Ok((
        Delivery {
            raw: raw.to_vec(),
            mail_from: address(field(form, "sender")),
            recipients,
            peer_ip: None,
            helo: None,
            spf: SpfSource::Header(SpfHeaderOwned::ReceivedSpf),
        },
        token.to_owned(),
    ))
}
