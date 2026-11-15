//! Building outgoing messages: the MIME tree, reply headers, quoted replies and the plain-text twin of an HTML body.

use std::fmt::Write as _;

use mail_builder::{
    MessageBuilder,
    headers::{address::Address, raw::Raw},
    mime::{BodyPart, MimePart},
};
use ring::rand::{SecureRandom, SystemRandom};
use thiserror::Error;

/// The most IDs a `References` header keeps.
const MAX_REFERENCES: usize = 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposedAddress {
    pub name: Option<String>,
    pub address: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComposedFileKind {
    Attachment,
    /// Shown inside the HTML body through `cid:{content_id}`.
    Inline {
        content_id: String,
    },
    /// A whole message sent as `message/rfc822`.
    Message,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposedFile {
    pub filename: String,
    pub content_type: String,
    pub bytes: Vec<u8>,
    pub kind: ComposedFileKind,
}

/// A message to send. There is no Bcc field: blind recipients only go into the SMTP envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Composed {
    /// With angle brackets.
    pub message_id: String,
    /// Unix seconds.
    pub date: i64,
    pub from: ComposedAddress,
    pub to: Vec<ComposedAddress>,
    pub cc: Vec<ComposedAddress>,
    pub reply_to: Option<ComposedAddress>,
    pub subject: String,
    /// With angle brackets.
    pub in_reply_to: Option<String>,
    /// With angle brackets.
    pub references: Vec<String>,
    pub html: String,
    pub text: String,
    pub files: Vec<ComposedFile>,
    pub extra_headers: Vec<(String, String)>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ComposeError {
    #[error("the {0} header holds a line break or a control character")]
    InvalidHeader(&'static str),
    #[error("the message has no recipients")]
    NoRecipients,
    #[error("the message could not be written")]
    Write,
}

fn clean(value: &str, field: &'static str) -> Result<(), ComposeError> {
    if value.chars().any(|c| c.is_control()) {
        return Err(ComposeError::InvalidHeader(field));
    }
    Ok(())
}

fn address(value: &ComposedAddress, field: &'static str) -> Result<Address<'static>, ComposeError> {
    clean(&value.address, field)?;
    if let Some(name) = &value.name {
        clean(name, field)?;
    }
    if value.address.is_empty() || value.address.contains(['<', '>', ',', ' ']) {
        return Err(ComposeError::InvalidHeader(field));
    }
    Ok(Address::new_address(
        value.name.clone().filter(|n| !n.is_empty()),
        value.address.clone(),
    ))
}

fn address_list(
    values: &[ComposedAddress],
    field: &'static str,
) -> Result<Address<'static>, ComposeError> {
    values
        .iter()
        .map(|a| address(a, field))
        .collect::<Result<Vec<_>, _>>()
        .map(Address::new_list)
}

/// Strips one pair of angle brackets: the builder adds its own.
fn bare_id(value: &str, field: &'static str) -> Result<String, ComposeError> {
    clean(value, field)?;
    let id = value.trim().trim_start_matches('<').trim_end_matches('>');
    if id.is_empty() || id.contains(['<', '>', ' ']) {
        return Err(ComposeError::InvalidHeader(field));
    }
    Ok(id.to_owned())
}

fn file_part(file: &ComposedFile) -> Result<MimePart<'static>, ComposeError> {
    clean(&file.filename, "filename")?;
    clean(&file.content_type, "content type")?;
    let part = MimePart::new(
        file.content_type.clone(),
        BodyPart::from(file.bytes.clone()),
    );
    Ok(match &file.kind {
        ComposedFileKind::Attachment | ComposedFileKind::Message => {
            part.attachment(file.filename.clone())
        }
        ComposedFileKind::Inline { content_id } => {
            let id = bare_id(content_id, "content id")?;
            part.inline().cid(id)
        }
    })
}

/// Builds the RFC 5322 message with CRLF line endings: `multipart/alternative` text and HTML, wrapped in
/// `multipart/related` when there are inline files and in `multipart/mixed` when there are attachments.
pub fn build_message(composed: &Composed) -> Result<Vec<u8>, ComposeError> {
    if composed.to.is_empty() && composed.cc.is_empty() {
        return Err(ComposeError::NoRecipients);
    }
    clean(&composed.subject, "subject")?;
    let message_id = bare_id(&composed.message_id, "message id")?;

    let mut builder = MessageBuilder::new()
        .message_id(message_id)
        .date(composed.date)
        .from(address(&composed.from, "from")?)
        .subject(composed.subject.clone());
    if !composed.to.is_empty() {
        builder = builder.to(address_list(&composed.to, "to")?);
    }
    if !composed.cc.is_empty() {
        builder = builder.cc(address_list(&composed.cc, "cc")?);
    }
    if let Some(reply_to) = &composed.reply_to {
        builder = builder.reply_to(address(reply_to, "reply-to")?);
    }
    if let Some(parent) = &composed.in_reply_to {
        builder = builder.in_reply_to(bare_id(parent, "in-reply-to")?);
    }
    if !composed.references.is_empty() {
        let ids = composed
            .references
            .iter()
            .map(|id| bare_id(id, "references"))
            .collect::<Result<Vec<_>, _>>()?;
        builder = builder.references(ids);
    }
    for (name, value) in &composed.extra_headers {
        if name.is_empty() || !name.bytes().all(|b| b.is_ascii_graphic() && b != b':') {
            return Err(ComposeError::InvalidHeader("extra header"));
        }
        clean(value, "extra header")?;
        builder = builder.header(name.clone(), Raw::new(value.clone()));
    }

    let mut related = Vec::new();
    let mut mixed = Vec::new();
    for file in &composed.files {
        let part = file_part(file)?;
        if matches!(file.kind, ComposedFileKind::Inline { .. }) {
            related.push(part);
        } else {
            mixed.push(part);
        }
    }

    let text = MimePart::new("text/plain", composed.text.clone());
    let mut html = MimePart::new("text/html", composed.html.clone());
    if !related.is_empty() {
        let mut parts = vec![html];
        parts.append(&mut related);
        html = MimePart::new("multipart/related", parts);
    }
    let mut body = MimePart::new("multipart/alternative", vec![text, html]);
    if !mixed.is_empty() {
        let mut parts = vec![body];
        parts.append(&mut mixed);
        body = MimePart::new("multipart/mixed", parts);
    }

    builder
        .body(body)
        .write_to_vec()
        .map_err(|_| ComposeError::Write)
}

/// A fresh `Message-ID` with angle brackets: 128 random bits in lowercase hex.
pub fn new_message_id(domain: &str) -> String {
    let mut bytes = [0u8; 16];
    // SystemRandom only fails when the OS has no entropy source, which is fatal for the whole server anyway.
    SystemRandom::new()
        .fill(&mut bytes)
        .expect("system random source");
    let mut id = String::with_capacity(32);
    for byte in bytes {
        let _ = write!(id, "{byte:02x}");
    }
    format!("<{id}@{domain}>")
}

/// The `References` for a reply: the parent's references and then the parent, at most 20 IDs. Over the limit it
/// keeps the first ID and the last 19.
pub fn trim_references(parent_refs: &[String], parent_id: &str) -> Vec<String> {
    let mut all: Vec<String> = parent_refs.to_vec();
    if all.last().map(String::as_str) != Some(parent_id) {
        all.push(parent_id.to_owned());
    }
    if all.len() > MAX_REFERENCES {
        let tail = all.split_off(all.len() - (MAX_REFERENCES - 1));
        all.truncate(1);
        all.extend(tail);
    }
    all
}

fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

/// `2026-11-15 10:00 UTC`.
fn format_date(unix: i64) -> String {
    let days = unix.div_euclid(86_400);
    let secs = unix.rem_euclid(86_400);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02} UTC",
        secs / 3_600,
        secs % 3_600 / 60
    )
}

/// The quoted original of a reply as HTML: "On {date}, {from} wrote:" and the original in a `blockquote`. The
/// original HTML is sanitized again here.
pub fn quote_reply(
    original_html: Option<&str>,
    original_text: &str,
    from: &str,
    date: i64,
) -> String {
    let body = match original_html {
        Some(html) if !html.trim().is_empty() => ammonia::clean(html),
        _ => escape(original_text).replace('\n', "<br>"),
    };
    format!(
        "<div class=\"orbit-quote-intro\">On {}, {} wrote:</div><blockquote class=\"orbit-quote\">{body}</blockquote>",
        escape(&format_date(date)),
        escape(from),
    )
}

/// The header block that opens a forwarded message.
pub fn forward_block(from: &str, date: i64, subject: &str, to: &str) -> String {
    format!(
        "<div class=\"orbit-forward\">---------- Forwarded message ----------<br>From: {}<br>Date: {}<br>Subject: {}<br>To: {}</div>",
        escape(from),
        escape(&format_date(date)),
        escape(subject),
        escape(to),
    )
}

fn decode_entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        rest = &rest[start..];
        let Some(end) = rest.find(';').filter(|e| *e <= 10) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let entity = &rest[1..end];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            _ => entity.strip_prefix('#').and_then(|n| {
                let code = match n.strip_prefix(['x', 'X']) {
                    Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                    None => n.parse().ok()?,
                };
                char::from_u32(code)
            }),
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// A small HTML to text converter for the plain-text twin of a message: tags are dropped, `br` and block ends
/// become line breaks, `li` becomes "- ", and the basic entities are decoded.
pub fn html_to_text(html: &str) -> String {
    let mut out = String::new();
    let mut rest = html;
    let mut skip: Option<&'static str> = None;
    while !rest.is_empty() {
        let Some(open) = rest.find('<') else {
            if skip.is_none() {
                push_text(&mut out, rest);
            }
            break;
        };
        if skip.is_none() {
            push_text(&mut out, &rest[..open]);
        }
        rest = &rest[open..];
        let Some(close) = rest.find('>') else {
            break;
        };
        let tag = rest[1..close].trim();
        rest = &rest[close + 1..];
        let closing = tag.starts_with('/');
        let name: String = tag
            .trim_start_matches('/')
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect::<String>()
            .to_ascii_lowercase();
        if let Some(until) = skip {
            if closing && name == until {
                skip = None;
            }
            continue;
        }
        match (name.as_str(), closing) {
            ("script", false) => skip = Some("script"),
            ("style", false) => skip = Some("style"),
            ("head", false) => skip = Some("head"),
            ("br", _) => out.push('\n'),
            ("li", false) => {
                if !out.is_empty() && !out.ends_with('\n') {
                    out.push('\n');
                }
                out.push_str("- ");
            }
            ("li", true) => out.push('\n'),
            (
                "p" | "div" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "ul" | "ol" | "blockquote"
                | "table" | "pre",
                true,
            ) => out.push_str("\n\n"),
            ("tr", true) => out.push('\n'),
            _ => {}
        }
    }
    let mut collapsed = String::with_capacity(out.len());
    let mut newlines = 0;
    for line in out.split('\n') {
        let line = line.trim();
        if line.is_empty() {
            newlines += 1;
            continue;
        }
        if !collapsed.is_empty() {
            collapsed.push('\n');
            if newlines > 0 {
                collapsed.push('\n');
            }
        }
        newlines = 0;
        collapsed.push_str(line);
    }
    collapsed
}

/// Appends text with HTML whitespace rules: any run of whitespace is one space.
fn push_text(out: &mut String, text: &str) {
    let decoded = decode_entities(text);
    for c in decoded.chars() {
        if c.is_whitespace() && c != '\u{a0}' {
            if !out.is_empty() && !out.ends_with([' ', '\n']) {
                out.push(' ');
            }
        } else {
            out.push(if c == '\u{a0}' { ' ' } else { c });
        }
    }
}
