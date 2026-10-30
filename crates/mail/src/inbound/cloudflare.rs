//! Cloudflare Email Routing: an Email Worker ([`WORKER_SCRIPT`]) posts each message as JSON
//! `{from, to, raw}` (`raw` in base64), signed with `x-orbit-signature` = hex HMAC-SHA256 of
//! `"<x-orbit-timestamp>.<body>"`. The Worker runs while the sender is still connected: Orbit's
//! `{"reject": "…"}` becomes `message.setReject` (a bounce), and any other failure throws, which
//! Cloudflare reports to the sender as a temporary failure, so the sender retries.
//! <https://developers.cloudflare.com/email-routing/email-workers/runtime-api/>

use base64::Engine as _;
use serde::Deserialize;

use super::{Delivery, InboundError, SpfHeaderOwned, SpfSource, address, fresh, verify_hex_hmac};

/// The Email Worker the admin pastes into Cloudflare, with the Worker variables
/// `ORBIT_WEBHOOK_URL` and `ORBIT_SIGNING_SECRET` (a secret).
pub const WORKER_SCRIPT: &str = r#"// Orbit mail: forwards each message to Orbit.
// Variables: ORBIT_WEBHOOK_URL, and the secret ORBIT_SIGNING_SECRET.
export default {
  async email(message, env) {
    const raw = new Uint8Array(await new Response(message.raw).arrayBuffer());
    let binary = "";
    for (let i = 0; i < raw.length; i += 0x8000) {
      binary += String.fromCharCode(...raw.subarray(i, i + 0x8000));
    }
    const body = JSON.stringify({ from: message.from, to: message.to, raw: btoa(binary) });
    const timestamp = Math.floor(Date.now() / 1000).toString();
    const key = await crypto.subtle.importKey(
      "raw", new TextEncoder().encode(env.ORBIT_SIGNING_SECRET),
      { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
    const mac = await crypto.subtle.sign("HMAC", key, new TextEncoder().encode(timestamp + "." + body));
    const signature = [...new Uint8Array(mac)].map((b) => b.toString(16).padStart(2, "0")).join("");
    const response = await fetch(env.ORBIT_WEBHOOK_URL, {
      method: "POST",
      headers: { "content-type": "application/json", "x-orbit-timestamp": timestamp, "x-orbit-signature": signature },
      body,
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

#[derive(Deserialize)]
struct Body {
    from: Option<String>,
    to: Option<String>,
    raw: String,
}

/// Checks the signature and the timestamp (seconds), then reads the message.
pub fn verify_and_parse(
    timestamp: Option<&str>,
    signature: Option<&str>,
    body: &[u8],
    secret: &[u8],
    now: i64,
    max_raw: usize,
) -> Result<Delivery, InboundError> {
    // base64 grows the message by 4/3; the envelope adds a little
    if body.len() > max_raw / 3 * 4 + 64 * 1024 {
        return Err(InboundError::TooLarge);
    }
    let (Some(timestamp), Some(signature)) = (timestamp, signature) else {
        return Err(InboundError::Unauthorized);
    };
    let mut signed = Vec::with_capacity(timestamp.len() + 1 + body.len());
    signed.extend_from_slice(timestamp.as_bytes());
    signed.push(b'.');
    signed.extend_from_slice(body);
    verify_hex_hmac(secret, &signed, signature)?;
    fresh(timestamp, now)?;

    let body: Body = serde_json::from_slice(body).map_err(|_| InboundError::Malformed("json"))?;
    let raw = base64::engine::general_purpose::STANDARD
        .decode(body.raw.as_bytes())
        .map_err(|_| InboundError::Malformed("raw_base64"))?;
    if raw.len() > max_raw {
        return Err(InboundError::TooLarge);
    }
    let recipient = address(body.to.as_deref()).ok_or(InboundError::Malformed("to"))?;
    Ok(Delivery {
        raw,
        mail_from: address(body.from.as_deref()),
        recipients: vec![recipient],
        peer_ip: None,
        helo: None,
        spf: SpfSource::Header(SpfHeaderOwned::ReceivedSpf),
    })
}
