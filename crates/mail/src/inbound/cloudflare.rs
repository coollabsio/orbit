//! Cloudflare Email Routing: an Email Worker ([`WORKER_SCRIPT`]) posts each raw message as the body
//! (`application/octet-stream`), with the envelope in `x-orbit-from` and `x-orbit-to`
//! (percent-encoded) and `x-orbit-signature` = hex HMAC-SHA256 of
//! `"<x-orbit-timestamp>.<x-orbit-from>.<x-orbit-to>.<hex SHA-256 of the body>"`. The Worker hashes
//! the message instead of copying it into base64 and JSON, so a 25 MiB message fits its memory. It
//! runs while the sender is still connected: Orbit's `{"reject": "…"}` becomes `message.setReject`
//! (a bounce), and any other failure throws, which Cloudflare reports to the sender as a temporary
//! failure, so the sender retries.
//! <https://developers.cloudflare.com/email-routing/email-workers/runtime-api/>

use sha2::{Digest, Sha256};

use super::{Delivery, InboundError, SpfHeaderOwned, SpfSource, address, fresh, verify_hex_hmac};

/// The Email Worker the admin pastes into Cloudflare, with the Worker variables
/// `ORBIT_WEBHOOK_URL` and `ORBIT_SIGNING_SECRET` (a secret).
pub const WORKER_SCRIPT: &str = r#"// Orbit mail: forwards each message to Orbit.
// Variables: ORBIT_WEBHOOK_URL, and the secret ORBIT_SIGNING_SECRET.
const hex = (bytes) => [...new Uint8Array(bytes)].map((b) => b.toString(16).padStart(2, "0")).join("");

export default {
  async email(message, env) {
    const raw = await new Response(message.raw).arrayBuffer();
    const from = encodeURIComponent(message.from || "");
    const to = encodeURIComponent(message.to || "");
    const timestamp = Math.floor(Date.now() / 1000).toString();
    const digest = hex(await crypto.subtle.digest("SHA-256", raw));
    const key = await crypto.subtle.importKey(
      "raw", new TextEncoder().encode(env.ORBIT_SIGNING_SECRET),
      { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
    const signed = [timestamp, from, to, digest].join(".");
    const signature = hex(await crypto.subtle.sign("HMAC", key, new TextEncoder().encode(signed)));
    const response = await fetch(env.ORBIT_WEBHOOK_URL, {
      method: "POST",
      headers: {
        "content-type": "application/octet-stream",
        "x-orbit-timestamp": timestamp,
        "x-orbit-from": from,
        "x-orbit-to": to,
        "x-orbit-signature": signature,
      },
      body: raw,
    });
    if (response.status === 200) {
      const reply = await response.json().catch(() => ({}));
      if (reply.reject) message.setReject(reply.reject);
      return;
    }
    if (response.status === 413) {
      message.setReject("Message too large");
      return;
    }
    // Anything else (Orbit down, a storage error): a temporary failure, so the sender retries.
    throw new Error("Orbit answered " + response.status);
  },
};
"#;

/// The header values of a Worker request, as sent.
#[derive(Clone, Copy, Debug)]
pub struct Envelope<'a> {
    pub timestamp: Option<&'a str>,
    pub signature: Option<&'a str>,
    /// Percent-encoded envelope sender (empty for the null sender).
    pub from: Option<&'a str>,
    /// Percent-encoded envelope recipient.
    pub to: Option<&'a str>,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Percent-decodes `value` (as `encodeURIComponent` wrote it).
fn decoded(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = std::str::from_utf8(bytes.get(index + 1..index + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Checks the signature and the timestamp (seconds), then the size, then reads the message
/// (`raw` is the body).
pub fn verify_and_parse(
    envelope: Envelope<'_>,
    raw: &[u8],
    secret: &[u8],
    now: i64,
    max_raw: usize,
) -> Result<Delivery, InboundError> {
    let (Some(timestamp), Some(signature), Some(from), Some(to)) = (
        envelope.timestamp,
        envelope.signature,
        envelope.from,
        envelope.to,
    ) else {
        return Err(InboundError::Unauthorized);
    };
    let signed = format!("{timestamp}.{from}.{to}.{}", hex(&Sha256::digest(raw)));
    verify_hex_hmac(secret, signed.as_bytes(), signature)?;
    fresh(timestamp, now)?;
    // after the signature: only a signed request can be the provider's error
    if raw.len() > max_raw {
        return Err(InboundError::TooLarge);
    }
    let recipient = decoded(to)
        .and_then(|to| address(Some(&to)))
        .ok_or(InboundError::Malformed("to"))?;
    Ok(Delivery {
        raw: raw.to_vec(),
        mail_from: decoded(from).and_then(|from| address(Some(&from))),
        recipients: vec![recipient],
        peer_ip: None,
        helo: None,
        spf: SpfSource::Header(SpfHeaderOwned::ReceivedSpf),
    })
}
