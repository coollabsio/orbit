//! MIME parsing with caps, thread headers, address lists, text body and the attachment list.
//!
//! Parsing uses `mail-parser` 0.11 (<https://docs.rs/mail-parser/0.11.9/mail_parser/>), which flattens every MIME part
//! of a message into one list (`Message::parts`); a part is named by its index in that list (`part_id`). Nested
//! `message/rfc822` parts keep their own list, so only top-level indexes are part ids.

use std::borrow::Cow;

use mail_parser::{
    Address as HeaderAddress, Encoding, HeaderValue, MessageParser, MimeHeaders, PartType,
};
use thiserror::Error;

/// Caps checked while parsing. The header caps are checked on the raw bytes before `mail-parser` runs; part count and
/// depth are counted on the parsed tree before anything is mapped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseLimits {
    /// Size of the top-level header block.
    pub max_header_bytes: usize,
    /// Header fields in the top-level header block, and in each part.
    pub max_headers: usize,
    /// MIME parts, nested messages included.
    pub max_parts: usize,
    /// MIME nesting (multiparts and nested messages).
    pub max_depth: usize,
    /// IDs kept from `References`; longer lists keep the first ID and the newest ones.
    pub max_references: usize,
}

impl Default for ParseLimits {
    fn default() -> Self {
        Self {
            max_header_bytes: 256 * 1024,
            max_headers: 1000,
            max_parts: 500,
            max_depth: 20,
            max_references: 100,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ParseError {
    /// A cap was hit: `header_bytes`, `headers`, `parts` or `depth`.
    #[error("the message is over the {0} limit")]
    Limit(&'static str),
    /// The bytes are not a message at all; the caller keeps them as plain text.
    #[error("the message could not be parsed")]
    Malformed,
}

/// A display name and an address as written in a header (the address is lowercased).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mailbox {
    pub name: Option<String>,
    pub address: String,
}

/// One attachment or inline part. Its bytes come from the raw message through `part_id` ([`part_bytes`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentInfo {
    pub part_id: u32,
    pub filename: Option<String>,
    /// Lowercase `type/subtype` from `Content-Type`.
    pub declared_type: String,
    /// Set only for the allowed inline image types ([`sniff_image`]).
    pub sniffed_type: Option<String>,
    /// Decoded size in bytes.
    pub size: u64,
    /// `Content-ID` without angle brackets.
    pub content_id: Option<String>,
    /// `Content-Disposition: inline`, or no disposition and a `Content-ID`.
    pub inline: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedMessage {
    /// `Message-ID` without angle brackets.
    pub message_id: Option<String>,
    pub in_reply_to: Vec<String>,
    /// At most `ParseLimits::max_references` IDs.
    pub references: Vec<String>,
    pub from: Option<Mailbox>,
    pub sender: Option<Mailbox>,
    pub reply_to: Vec<Mailbox>,
    pub to: Vec<Mailbox>,
    pub cc: Vec<Mailbox>,
    /// Decoded subject (RFC 2047), trimmed.
    pub subject: String,
    /// [`normalize_subject`] of `subject`.
    pub normalized_subject: String,
    /// `Date` as Unix seconds, when it is valid.
    pub sent_at: Option<i64>,
    /// Text bodies, or the HTML body as text when there is no text part.
    pub body_text: String,
    /// The part id of the HTML body.
    pub html_part: Option<u32>,
    pub attachments: Vec<AttachmentInfo>,
    /// First 200 characters of `body_text`, whitespace collapsed.
    pub snippet: String,
}

const SNIPPET_CHARS: usize = 200;

/// Parses a raw message. On [`ParseError`] the caller stores the message as plain text with a warning.
pub fn parse(raw: &[u8], limits: &ParseLimits) -> Result<ParsedMessage, ParseError> {
    check_header_block(raw, limits)?;
    let message = MessageParser::default()
        .parse(raw)
        .ok_or(ParseError::Malformed)?;
    if message.parts.is_empty() {
        return Err(ParseError::Malformed);
    }
    check_tree(&message, limits)?;

    let subject = message.subject().unwrap_or_default().trim().to_owned();
    let mut body_text = body_text(&message);
    // A multipart whose boundary never appears comes back as one unnamed text part listed as an attachment; show it
    // as the body instead.
    let root_is_body = body_text.is_empty()
        && message.html_body.is_empty()
        && message.root_part().attachment_name().is_none()
        && matches!(message.root_part().body, PartType::Text(_));
    if root_is_body && let Some(text) = message.root_part().text_contents() {
        body_text = text.to_owned();
    }
    let html_part = message.html_body.iter().copied().find(|id| {
        matches!(
            message.part(*id).map(|part| &part.body),
            Some(PartType::Html(_))
        )
    });
    let attachments = message
        .attachments
        .iter()
        .filter(|id| !(root_is_body && **id == 0))
        .filter_map(|id| attachment_info(&message, raw, *id))
        .collect();

    Ok(ParsedMessage {
        message_id: message.message_id().map(str::to_owned),
        in_reply_to: ids(message.in_reply_to()),
        references: cap_references(ids(message.references()), limits.max_references),
        from: message
            .from()
            .and_then(|value| mailboxes(value).into_iter().next()),
        sender: message
            .sender()
            .and_then(|value| mailboxes(value).into_iter().next()),
        reply_to: message.reply_to().map(mailboxes).unwrap_or_default(),
        to: message.to().map(mailboxes).unwrap_or_default(),
        cc: message.cc().map(mailboxes).unwrap_or_default(),
        normalized_subject: normalize_subject(&subject),
        subject,
        sent_at: message
            .date()
            .filter(|date| date.is_valid())
            .map(|date| date.to_timestamp()),
        snippet: snippet(&body_text),
        body_text,
        html_part,
        attachments,
    })
}

/// The decoded body of one top-level part, or `None` for an unknown id or a multipart container. Text parts come back
/// as UTF-8.
pub fn part_bytes(raw: &[u8], part_id: u32) -> Option<Vec<u8>> {
    let limits = ParseLimits::default();
    check_header_block(raw, &limits).ok()?;
    let message = MessageParser::default().parse(raw)?;
    check_tree(&message, &limits).ok()?;
    let part = message.part(part_id)?;
    if matches!(part.body, PartType::Multipart(_)) {
        return None;
    }
    Some(part_contents(part, raw).into_owned())
}

/// Strips `Re:`, `Fwd:`, `Fw:`, `AW:` and `SV:` (also `Re[2]:`) repeatedly, keeps `[tag]` prefixes, collapses
/// whitespace and lowercases. Used to match replies to threads.
pub fn normalize_subject(subject: &str) -> String {
    let mut tags: Vec<&str> = Vec::new();
    let mut rest = subject.trim_start();
    loop {
        if rest.starts_with('[')
            && let Some(end) = rest.find(']')
        {
            tags.push(&rest[..=end]);
            rest = rest[end + 1..].trim_start();
            continue;
        }
        match strip_reply_prefix(rest) {
            Some(after) => rest = after.trim_start(),
            None => break,
        }
    }
    let mut joined = tags.join(" ");
    if !rest.is_empty() {
        if !joined.is_empty() {
            joined.push(' ');
        }
        joined.push_str(rest);
    }
    collapse_whitespace(&joined).to_lowercase()
}

/// The MIME type for the magic bytes of PNG, JPEG, GIF, WebP and AVIF. Every other type, SVG included, is `None`.
pub fn sniff_image(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else if is_avif(bytes) {
        Some("image/avif")
    } else {
        None
    }
}

/// An ISO BMFF `ftyp` box whose major or compatible brands include `avif` or `avis`.
fn is_avif(bytes: &[u8]) -> bool {
    if bytes.len() < 16 || &bytes[4..8] != b"ftyp" {
        return false;
    }
    let box_len = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
    let end = box_len.clamp(16, 256).min(bytes.len());
    let is_avif_brand = |brand: &[u8]| brand == b"avif" || brand == b"avis";
    // Major brand at 8..12, minor version at 12..16, then compatible brands.
    is_avif_brand(&bytes[8..12]) || bytes[16..end].chunks_exact(4).any(is_avif_brand)
}

fn strip_reply_prefix(subject: &str) -> Option<&str> {
    let word_len = subject
        .bytes()
        .take_while(|byte| byte.is_ascii_alphabetic())
        .count();
    let word = &subject[..word_len];
    if !["re", "fwd", "fw", "aw", "sv"]
        .iter()
        .any(|prefix| word.eq_ignore_ascii_case(prefix))
    {
        return None;
    }
    let mut rest = &subject[word_len..];
    // `Re[2]:` and `Re(2):` counters.
    for (open, close) in [('[', ']'), ('(', ')')] {
        if let Some(inner) = rest.strip_prefix(open)
            && let Some(end) = inner.find(close)
            && inner[..end].bytes().all(|byte| byte.is_ascii_digit())
        {
            rest = &inner[end + 1..];
            break;
        }
    }
    rest.trim_start_matches([' ', '\t']).strip_prefix(':')
}

fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn snippet(body_text: &str) -> String {
    let mut snippet = String::new();
    let mut chars = 0;
    for word in body_text.split_whitespace() {
        if chars >= SNIPPET_CHARS {
            break;
        }
        if !snippet.is_empty() {
            snippet.push(' ');
            chars += 1;
        }
        for character in word.chars() {
            if chars >= SNIPPET_CHARS {
                break;
            }
            snippet.push(character);
            chars += 1;
        }
    }
    snippet.trim_end().to_owned()
}

/// Header size and count of the top-level header block, checked on the raw bytes.
fn check_header_block(raw: &[u8], limits: &ParseLimits) -> Result<(), ParseError> {
    let window = &raw[..raw.len().min(limits.max_header_bytes.saturating_add(1))];
    let mut fields = 0usize;
    let mut position = 0usize;
    while position < window.len() {
        let line_end = window[position..]
            .iter()
            .position(|byte| *byte == b'\n')
            .map(|offset| position + offset);
        let Some(line_end) = line_end else {
            // The header block runs past the window.
            break;
        };
        let line = &window[position..line_end];
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.is_empty() {
            return Ok(());
        }
        if !matches!(line[0], b' ' | b'\t') {
            fields += 1;
            if fields > limits.max_headers {
                return Err(ParseError::Limit("headers"));
            }
        }
        position = line_end + 1;
    }
    if raw.len() > limits.max_header_bytes {
        return Err(ParseError::Limit("header_bytes"));
    }
    Ok(())
}

/// Counts parts and nesting of the parsed tree (iteratively, so a deep tree cannot overflow the stack).
fn check_tree(message: &mail_parser::Message<'_>, limits: &ParseLimits) -> Result<(), ParseError> {
    let mut parts = 0usize;
    // (message, part id, depth)
    let mut stack: Vec<(&mail_parser::Message<'_>, u32, usize)> = vec![(message, 0, 0)];
    while let Some((owner, part_id, depth)) = stack.pop() {
        if depth > limits.max_depth {
            return Err(ParseError::Limit("depth"));
        }
        let Some(part) = owner.part(part_id) else {
            continue;
        };
        parts += 1;
        if parts > limits.max_parts {
            return Err(ParseError::Limit("parts"));
        }
        if part.headers.len() > limits.max_headers {
            return Err(ParseError::Limit("headers"));
        }
        match &part.body {
            PartType::Multipart(children) => {
                stack.extend(children.iter().map(|child| (owner, *child, depth + 1)));
            }
            PartType::Message(nested) => stack.push((nested, 0, depth + 1)),
            _ => {}
        }
    }
    Ok(())
}

fn body_text(message: &mail_parser::Message<'_>) -> String {
    let mut text = String::new();
    for index in 0..message.text_body.len() {
        let Some(body) = message.body_text(index) else {
            continue;
        };
        if body.trim().is_empty() {
            continue;
        }
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&body);
    }
    text
}

fn attachment_info(
    message: &mail_parser::Message<'_>,
    raw: &[u8],
    part_id: u32,
) -> Option<AttachmentInfo> {
    let part = message.part(part_id)?;
    let declared_type = match part.content_type() {
        Some(content_type) => match content_type.subtype() {
            Some(subtype) => format!("{}/{}", content_type.ctype(), subtype),
            None => content_type.ctype().to_owned(),
        }
        .to_ascii_lowercase(),
        None => match part.body {
            PartType::Message(_) => "message/rfc822".to_owned(),
            PartType::Text(_) => "text/plain".to_owned(),
            PartType::Html(_) => "text/html".to_owned(),
            _ => "application/octet-stream".to_owned(),
        },
    };
    let contents = part_contents(part, raw);
    let sniffed_type = match part.body {
        PartType::Binary(_) | PartType::InlineBinary(_) => {
            sniff_image(&contents).map(str::to_owned)
        }
        _ => None,
    };
    let content_id = part
        .content_id()
        .map(|id| {
            id.trim()
                .trim_start_matches('<')
                .trim_end_matches('>')
                .to_owned()
        })
        .filter(|id| !id.is_empty());
    let inline = match part.content_disposition() {
        Some(disposition) => disposition.is_inline(),
        None => content_id.is_some(),
    };
    Some(AttachmentInfo {
        part_id,
        filename: part
            .attachment_name()
            .map(|name| name.trim().to_owned())
            .filter(|name| !name.is_empty()),
        declared_type,
        sniffed_type,
        size: contents.len() as u64,
        content_id,
        inline,
    })
}

/// Decoded part bytes. An unencoded nested message carries the whole outer message as its `raw_message`, so its bytes
/// are sliced from the raw input by offset instead.
fn part_contents<'a>(part: &'a mail_parser::MessagePart<'_>, raw: &'a [u8]) -> Cow<'a, [u8]> {
    match &part.body {
        PartType::Message(_) if part.encoding == Encoding::None => {
            let start = part.offset_body as usize;
            let end = (part.offset_end as usize).min(raw.len());
            Cow::Borrowed(raw.get(start..end).unwrap_or_default())
        }
        _ => Cow::Borrowed(part.contents()),
    }
}

fn ids(value: &HeaderValue<'_>) -> Vec<String> {
    match value {
        HeaderValue::Text(id) => vec![id.to_string()],
        HeaderValue::TextList(list) => list.iter().map(|id| id.to_string()).collect(),
        _ => Vec::new(),
    }
}

/// Keeps the thread root (first ID) and the newest `max - 1` IDs.
fn cap_references(mut references: Vec<String>, max: usize) -> Vec<String> {
    if references.len() <= max {
        return references;
    }
    if max == 0 {
        return Vec::new();
    }
    let drop_until = references.len() - (max - 1);
    references.drain(1..drop_until);
    references
}

fn mailboxes(value: &HeaderAddress<'_>) -> Vec<Mailbox> {
    value
        .iter()
        .filter_map(|addr| {
            let address = addr.address()?.trim();
            if address.is_empty() {
                return None;
            }
            Some(Mailbox {
                name: addr
                    .name()
                    .map(collapse_whitespace)
                    .filter(|name| !name.is_empty()),
                address: address.to_lowercase(),
            })
        })
        .collect()
}
