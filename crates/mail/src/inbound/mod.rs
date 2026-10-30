//! Mail that a provider received and delivers to Orbit over HTTPS (provider intake design §2): one
//! module for each provider. Each function first authenticates the request with the provider's
//! secret, then reads the raw message, the envelope and where the SPF result is. No I/O: the server
//! passes the request's bytes and header values.

pub mod cloudflare;
pub mod mailgun;
pub mod postmark;
pub mod resend;
pub mod ses;

use std::net::IpAddr;

use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::auth::{ProviderSpf, SpfHeader};

/// How far a signed timestamp may be from the server's clock.
pub const REPLAY_WINDOW_SECS: i64 = 5 * 60;

/// One message from a provider.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Delivery {
    pub raw: Vec<u8>,
    /// The envelope sender; `None` for the null sender `<>`.
    pub mail_from: Option<String>,
    /// The envelope recipients (never the `To`/`Cc` headers, so Bcc works).
    pub recipients: Vec<String>,
    /// The sending server, when the provider reports it.
    pub peer_ip: Option<IpAddr>,
    pub helo: Option<String>,
    pub spf: SpfSource,
}

/// Where the provider's SPF result is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpfSource {
    /// In a header the provider adds to the message.
    Header(SpfHeaderOwned),
    /// In the provider's request.
    Given(ProviderSpf),
    None,
}

/// [`SpfHeader`] with its own `authserv-id`, to keep with a delivery.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpfHeaderOwned {
    ReceivedSpf,
    AuthResults { authserv_id: String },
}

impl SpfHeaderOwned {
    #[must_use]
    pub fn as_header(&self) -> SpfHeader<'_> {
        match self {
            Self::ReceivedSpf => SpfHeader::ReceivedSpf,
            Self::AuthResults { authserv_id } => SpfHeader::AuthResults { authserv_id },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum InboundError {
    /// Missing or wrong credentials or signature.
    #[error("the request is not signed by the provider")]
    Unauthorized,
    /// A signed timestamp outside [`REPLAY_WINDOW_SECS`].
    #[error("the request is too old")]
    Replay,
    #[error("the message is over the size limit")]
    TooLarge,
    #[error("the request is not a delivery: {0}")]
    Malformed(&'static str),
}

/// `timestamp` (seconds) is within [`REPLAY_WINDOW_SECS`] of `now`.
pub(crate) fn fresh(timestamp: &str, now: i64) -> Result<(), InboundError> {
    let timestamp: i64 = timestamp
        .trim()
        .parse()
        .map_err(|_| InboundError::Unauthorized)?;
    if (now - timestamp).abs() > REPLAY_WINDOW_SECS {
        Err(InboundError::Replay)
    } else {
        Ok(())
    }
}

/// Checks a hex HMAC-SHA256 of `message` in constant time.
pub(crate) fn verify_hex_hmac(
    key: &[u8],
    message: &[u8],
    signature: &str,
) -> Result<(), InboundError> {
    let expected = decode_hex(signature.trim()).ok_or(InboundError::Unauthorized)?;
    let mut mac = Hmac::<Sha256>::new_from_slice(key).map_err(|_| InboundError::Unauthorized)?;
    mac.update(message);
    mac.verify_slice(&expected)
        .map_err(|_| InboundError::Unauthorized)
}

fn decode_hex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(text.get(index..index + 2)?, 16).ok())
        .collect()
}

/// Equal in constant time for equal lengths (the length itself is not secret).
pub(crate) fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0_u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// An address as sent (`a@b`, `<a@b>` or `Name <a@b>`), or `None` for the null sender and empty
/// values.
pub(crate) fn address(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    let value = match (value.rfind('<'), value.rfind('>')) {
        (Some(start), Some(end)) if start < end => &value[start + 1..end],
        _ => value,
    };
    Some(value.trim().to_owned()).filter(|value| !value.is_empty())
}

/// Comma-separated envelope recipients, without empty items.
pub(crate) fn recipients(values: &[&str]) -> Vec<String> {
    values
        .iter()
        .flat_map(|value| value.split(','))
        .filter_map(|value| address(Some(value)))
        .collect()
}
