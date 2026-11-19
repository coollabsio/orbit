//! Bounce return paths and delivery status notifications (RFC 3464).
//!
//! The envelope sender of each outgoing recipient is `bounce+{token}@domain`. The token is the recipient id and a
//! truncated HMAC, so a bounce can be matched to its recipient without a lookup table and cannot be forged.

use hmac::{Hmac, Mac};
use mail_parser::{MessageParser, MimeHeaders, PartType};
use sha2::Sha256;

const PREFIX: &str = "bounce+";
const MAC_LEN: usize = 10;
const ID_LEN: usize = 16;
const ALPHABET: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";

fn mac(recipient_id: &[u8; 16], key: &[u8; 32]) -> Hmac<Sha256> {
    // HMAC accepts any key length, so this cannot fail.
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC takes any key length");
    mac.update(b"orbit-bounce-v1");
    mac.update(recipient_id);
    mac
}

fn base32_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 8 / 5 + 1);
    let mut buffer = 0u32;
    let mut bits = 0;
    for byte in bytes {
        buffer = (buffer << 8) | u32::from(*byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(ALPHABET[((buffer >> bits) & 31) as usize] as char);
        }
        buffer &= (1 << bits) - 1;
    }
    if bits > 0 {
        out.push(ALPHABET[((buffer << (5 - bits)) & 31) as usize] as char);
    }
    out
}

/// Decodes lowercase RFC 4648 base32 without padding. Returns `None` for other characters or non-zero spare bits.
fn base32_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 5 / 8);
    let mut buffer = 0u32;
    let mut bits = 0;
    for character in text.bytes() {
        let value = ALPHABET.iter().position(|letter| *letter == character)? as u32;
        buffer = (buffer << 5) | value;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
            buffer &= (1 << bits) - 1;
        }
    }
    (buffer == 0).then_some(out)
}

/// The local part of the return path for one recipient: `bounce+{base32(id ‖ hmac10)}`, lowercase, no padding.
pub fn bounce_local_part(recipient_id: &[u8; 16], key: &[u8; 32]) -> String {
    let tag = mac(recipient_id, key).finalize().into_bytes();
    let mut raw = Vec::with_capacity(ID_LEN + MAC_LEN);
    raw.extend_from_slice(recipient_id);
    raw.extend_from_slice(&tag[..MAC_LEN]);
    format!("{PREFIX}{}", base32_encode(&raw))
}

/// Reads a bounce local part (case-insensitive) and returns the recipient id when the MAC matches.
pub fn parse_bounce_local_part(local: &str, key: &[u8; 32]) -> Option<[u8; 16]> {
    let local = local.to_ascii_lowercase();
    let token = local.strip_prefix(PREFIX)?;
    let raw = base32_decode(token)?;
    if raw.len() != ID_LEN + MAC_LEN {
        return None;
    }
    let (id, tag) = raw.split_at(ID_LEN);
    let id: [u8; 16] = id.try_into().ok()?;
    // `verify_truncated_left` compares in constant time.
    mac(&id, key).verify_truncated_left(tag).ok()?;
    Some(id)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DsnAction {
    Failed,
    Delayed,
    Delivered,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DsnRecipient {
    /// `Final-Recipient`, lower case, without its address type and `<>`.
    pub address: String,
    /// `Original-Recipient` (the address as sent, before an alias expanded it), in the same form.
    pub original: Option<String>,
    pub action: DsnAction,
    pub status: String,
    pub diagnostic: Option<String>,
}

/// True for a permanent failure status (`5.x.x`).
pub fn is_hard(status: &str) -> bool {
    status.trim().starts_with("5.")
}

/// Parses a `multipart/report; report-type=delivery-status` message. Returns the per-recipient results, or `None`
/// when the message is not a delivery status notification.
pub fn parse_dsn(raw: &[u8]) -> Option<Vec<DsnRecipient>> {
    let message = MessageParser::default().parse(raw)?;
    let root = message.parts.first()?;
    let content_type = root.content_type()?;
    if !content_type.ctype().eq_ignore_ascii_case("multipart")
        || !content_type
            .subtype()
            .is_some_and(|subtype| subtype.eq_ignore_ascii_case("report"))
    {
        return None;
    }
    if !content_type
        .attribute("report-type")
        .is_some_and(|value| value.eq_ignore_ascii_case("delivery-status"))
    {
        return None;
    }
    let status_part = message.parts.iter().find(|part| {
        part.content_type().is_some_and(|ct| {
            ct.ctype().eq_ignore_ascii_case("message")
                && ct
                    .subtype()
                    .is_some_and(|subtype| subtype.eq_ignore_ascii_case("delivery-status"))
        })
    })?;
    let body = match &status_part.body {
        PartType::Text(text) | PartType::Html(text) => text.as_bytes(),
        PartType::Binary(bytes) | PartType::InlineBinary(bytes) => bytes.as_ref(),
        _ => return None,
    };
    let text = String::from_utf8_lossy(body);
    let mut recipients = Vec::new();
    // Group 0 is the per-message block; every later group describes one recipient.
    for group in groups(&text).into_iter().skip(1) {
        let Some(address) = field(&group, "final-recipient").and_then(|value| recipient(&value))
        else {
            continue;
        };
        let original = field(&group, "original-recipient").and_then(|value| recipient(&value));
        let action = match field(&group, "action")
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str()
        {
            "failed" => DsnAction::Failed,
            "delayed" => DsnAction::Delayed,
            "delivered" => DsnAction::Delivered,
            _ => DsnAction::Other,
        };
        recipients.push(DsnRecipient {
            address,
            original,
            action,
            status: field(&group, "status").unwrap_or_default(),
            diagnostic: field(&group, "diagnostic-code").filter(|value| !value.is_empty()),
        });
    }
    (!recipients.is_empty()).then_some(recipients)
}

/// An address field of a DSN (`rfc822; <Ann@Example.org>`): without the address type and the
/// angle brackets, lower case; `None` when empty.
fn recipient(value: &str) -> Option<String> {
    let address = value
        .split_once(';')
        .map_or(value, |(_, rest)| rest)
        .trim()
        .trim_start_matches('<')
        .trim_end_matches('>')
        .trim()
        .to_ascii_lowercase();
    (!address.is_empty()).then_some(address)
}

/// Splits header-like text into blank-line separated groups of unfolded `(lowercase name, value)` fields.
fn groups(text: &str) -> Vec<Vec<(String, String)>> {
    let mut groups = Vec::new();
    let mut current: Vec<(String, String)> = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            if !current.is_empty() {
                groups.push(std::mem::take(&mut current));
            }
        } else if line.starts_with([' ', '\t']) {
            if let Some((_, value)) = current.last_mut() {
                value.push(' ');
                value.push_str(line.trim());
            }
        } else if let Some((name, value)) = line.split_once(':') {
            current.push((name.trim().to_ascii_lowercase(), value.trim().to_owned()));
        }
    }
    if !current.is_empty() {
        groups.push(current);
    }
    groups
}

fn field(group: &[(String, String)], name: &str) -> Option<String> {
    group
        .iter()
        .find(|(field_name, _)| field_name == name)
        .map(|(_, value)| value.clone())
}
