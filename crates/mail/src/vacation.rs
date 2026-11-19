//! When may a vacation auto-reply be sent (RFC 3834 and Orbit's own rules)?

use mail_parser::{Address as ParsedAddress, HeaderValue, MessageParser};

use crate::address::normalize_address;
use crate::auth::{AuthOutcome, DmarcVerdict, Verdict};

pub struct VacationInput<'a> {
    /// The envelope sender; `None` or empty (`<>`) for a bounce.
    pub mail_from: Option<&'a str>,
    /// The message headers as `(name, value)` pairs.
    pub headers: &'a [(String, String)],
    /// The normalized addresses of the mailbox that would reply.
    pub mailbox_addresses: &'a [String],
    pub sender_is_workspace_mailbox: bool,
    pub in_inbox: bool,
    pub dmarc_failed: bool,
    /// The envelope sender is proven ([`envelope_authenticated`], or a message of this
    /// instance): a reply to a forged sender would be backscatter.
    pub sender_authenticated: bool,
    pub replied_within_7_days: bool,
}

/// Whether the envelope sender's domain (`MAIL FROM`) is authenticated: SPF passed for that very
/// domain, or DMARC passed for that very domain as the `From` domain. Relaxed alignment is not
/// enough: a sender who controls `child.example.org` passes DMARC for it and could forge
/// `MAIL FROM:<victim@example.org>`. Only then may an automatic reply go to the envelope sender
/// (no backscatter to a forged address).
#[must_use]
pub fn envelope_authenticated(auth: &AuthOutcome, envelope_domain: &str) -> bool {
    let envelope = envelope_domain.trim_end_matches('.');
    let spf = auth.spf == Verdict::Pass
        && auth
            .spf_domain
            .as_deref()
            .is_some_and(|domain| domain.trim_end_matches('.').eq_ignore_ascii_case(envelope));
    let dmarc = auth.dmarc == DmarcVerdict::Pass
        && auth
            .aligned_domain
            .as_deref()
            .is_some_and(|domain| domain.trim_end_matches('.').eq_ignore_ascii_case(envelope));
    spf || dmarc
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> impl Iterator<Item = &'a str> {
    let name = name.to_owned();
    headers
        .iter()
        .filter(move |(field, _)| field.eq_ignore_ascii_case(&name))
        .map(|(_, value)| value.as_str())
}

/// Normalized addresses in one address header value.
fn addresses(name: &str, value: &str) -> Vec<String> {
    let raw = format!("{name}: {value}\r\n\r\n");
    let Some(message) = MessageParser::default().parse(raw.as_bytes()) else {
        return Vec::new();
    };
    let parsed: Option<&ParsedAddress> = match message.header(name) {
        Some(HeaderValue::Address(address)) => Some(address),
        _ => None,
    };
    parsed
        .into_iter()
        .flat_map(|address| address.iter())
        .filter_map(|addr| addr.address.as_deref())
        .filter_map(|addr| normalize_address(addr.trim()).ok())
        .map(|addr| addr.to_string())
        .collect()
}

fn addressed_to_mailbox(input: &VacationInput<'_>) -> bool {
    ["To", "Cc"].iter().any(|name| {
        header(input.headers, name)
            .flat_map(|value| addresses(name, value))
            .any(|addr| input.mailbox_addresses.contains(&addr))
    })
}

/// True when a vacation reply may be sent for this message.
pub fn vacation_allowed(input: &VacationInput<'_>) -> bool {
    match input.mail_from {
        None => return false,
        Some(sender) if sender.trim().is_empty() || sender.trim() == "<>" => return false,
        Some(_) => {}
    }
    if header(input.headers, "Auto-Submitted").any(|value| !value.trim().eq_ignore_ascii_case("no"))
    {
        return false;
    }
    if header(input.headers, "Precedence").any(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "bulk" | "list" | "junk"
        )
    }) {
        return false;
    }
    if header(input.headers, "List-Id").next().is_some()
        || header(input.headers, "List-Unsubscribe").next().is_some()
    {
        return false;
    }
    if input.sender_is_workspace_mailbox
        || !input.in_inbox
        || input.dmarc_failed
        || !input.sender_authenticated
        || input.replied_within_7_days
    {
        return false;
    }
    addressed_to_mailbox(input)
}
