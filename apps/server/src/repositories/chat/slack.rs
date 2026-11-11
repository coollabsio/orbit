//! What makes an incoming webhook a drop-in for a Slack one: Slack's mrkdwn becomes Orbit
//! Markdown, legacy `attachments` become attachment blocks, and values Slack would accept but
//! Orbit cannot show are cut or left out instead of refusing the message.

use orbit_platform::Id;
use serde::Deserialize;
use serde_json::Value;

use super::blocks::{AttachmentBlock, AttachmentField, BlocksError, MessageBlock, parse_blocks};

/// Attachments in one payload, as Slack allows.
const MAX_ATTACHMENTS: usize = 100;
const ATTACHMENT_TEXT_MAX_CHARS: usize = 3000;
const ATTACHMENT_LINE_MAX_CHARS: usize = 250;
const FIELD_VALUE_MAX_CHARS: usize = 2000;
const URL_MAX_CHARS: usize = 2048;
const EMOJI_NAME_MAX_CHARS: usize = 64;

/// Marks a converted `<…>` token while the emphasis pass runs (private-use characters).
const TOKEN_START: char = '\u{E000}';
const TOKEN_END: char = '\u{E001}';
/// Characters that a backslash keeps as text in Orbit Markdown.
const MARKDOWN_SPECIAL: &[char] = &['\\', '`', '*', '_', '~', '[', ']', '|', '#', '!', '-'];

/// At most `max` characters; a longer text ends with "…".
pub fn cut(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn escape_markdown(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        if MARKDOWN_SPECIAL.contains(&character) || character == '<' || character == '>' {
            out.push('\\');
        }
        out.push(character);
    }
    out
}

/// Slack's three entities, as text.
fn unescape_plain(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

/// Slack's entities in Markdown: an escaped `<` must not start a mention token.
fn unescape_markdown(text: &str) -> String {
    text.replace("&lt;", "\\<")
        .replace("&gt;", "\\>")
        .replace("&amp;", "&")
}

fn web_url(url: &str) -> Option<&str> {
    let url = url.trim();
    (url.chars().count() <= URL_MAX_CHARS
        && url::Url::parse(url).is_ok_and(|parsed| matches!(parsed.scheme(), "https" | "http")))
    .then_some(url)
}

fn https_url(url: &str) -> Option<String> {
    let url = url.trim();
    (url.chars().count() <= URL_MAX_CHARS
        && url::Url::parse(url)
            .is_ok_and(|parsed| parsed.scheme() == "https" && parsed.host_str().is_some()))
    .then(|| url.to_owned())
}

/// One `<…>` token of Slack mrkdwn as Orbit Markdown: mentions keep Orbit's token form, links
/// become Markdown links, and special commands (`!subteam^…`, `!date^…`) show their label.
fn angle_token(inner: &str) -> String {
    let (target, label) = match inner.split_once('|') {
        Some((target, label)) => (target, Some(unescape_plain(label))),
        None => (inner, None),
    };
    // Orbit ids stay mention tokens. A Slack id (`U024BE7LH`) names nobody here: its label, or
    // the id itself, shows as text.
    for (sigil, token) in [('@', "<@"), ('#', "<#")] {
        if let Some(id) = target.strip_prefix(sigil) {
            if id.parse::<Id>().is_ok() {
                return format!("{token}{id}>");
            }
            let name = label
                .as_deref()
                .map_or(id, |label| label.trim_start_matches(sigil));
            return escape_markdown(&format!("{sigil}{name}"));
        }
    }
    if let Some(command) = target.strip_prefix('!') {
        return match command {
            "here" => "<!here>".to_owned(),
            "channel" | "everyone" => "<!channel>".to_owned(),
            _ => label
                .map(|label| escape_markdown(&label))
                .unwrap_or_default(),
        };
    }
    let target = unescape_plain(target);
    if let Some(url) = web_url(&target) {
        // `(`, `)` and spaces would end the Markdown link early.
        let url = url
            .replace('(', "%28")
            .replace(')', "%29")
            .replace(' ', "%20");
        return match label {
            Some(label) if !label.trim().is_empty() => {
                format!("[{}]({url})", escape_markdown(&label))
            }
            _ => format!("<{url}>"),
        };
    }
    if let Some(address) = target.strip_prefix("mailto:") {
        return match label {
            Some(label) if label != address => {
                format!("{} ({})", escape_markdown(&label), escape_markdown(address))
            }
            _ => escape_markdown(address),
        };
    }
    escape_markdown(label.as_deref().unwrap_or(&target))
}

fn is_boundary(character: Option<char>) -> bool {
    character.is_none_or(|character| !character.is_alphanumeric())
}

/// Slack's single-marker `*bold*` and `~strike~` as Markdown's doubled markers. `_italic_` is the
/// same in both; doubled markers that are Markdown already stay.
fn emphasis_line(line: &[char]) -> String {
    let mut out = String::with_capacity(line.len() + 8);
    let mut index = 0;
    while index < line.len() {
        let marker = line[index];
        let previous = index.checked_sub(1).map(|at| line[at]);
        let next = line.get(index + 1).copied();
        let opens = matches!(marker, '*' | '~')
            && previous != Some(marker)
            && previous != Some('\\')
            && is_boundary(previous)
            && next.is_some_and(|next| next != marker && !next.is_whitespace());
        if opens {
            let close = (index + 2..line.len()).find(|&at| {
                line[at] == marker
                    && !line[at - 1].is_whitespace()
                    && line[at - 1] != marker
                    && line.get(at + 1).copied() != Some(marker)
                    && is_boundary(line.get(at + 1).copied())
            });
            if let Some(close) = close {
                out.push(marker);
                out.push(marker);
                out.extend(&line[index + 1..close]);
                out.push(marker);
                out.push(marker);
                index = close + 1;
                continue;
            }
        }
        out.push(marker);
        index += 1;
    }
    out
}

/// Text outside code: tokens, entities, then emphasis (`format`) or escaping (`!format`).
fn convert_text(text: &str, format: bool) -> String {
    let mut tokens: Vec<String> = Vec::new();
    let mut plain = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('<') {
        plain.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find(['>', '<', '\n']) {
            Some(end) if after.as_bytes()[end] == b'>' => {
                plain.push(TOKEN_START);
                plain.push_str(&tokens.len().to_string());
                plain.push(TOKEN_END);
                tokens.push(angle_token(&after[..end]));
                rest = &after[end + 1..];
            }
            // A lone `<`: Slack would have sent `&lt;`.
            _ => {
                plain.push_str("&lt;");
                rest = after;
            }
        }
    }
    plain.push_str(rest);
    let plain = if format {
        let unescaped = unescape_markdown(&plain);
        unescaped
            .split_inclusive('\n')
            .map(|line| emphasis_line(&line.chars().collect::<Vec<_>>()))
            .collect::<String>()
    } else {
        unescape_markdown(&escape_markdown_keeping_tokens(&plain))
    };
    restore_tokens(&plain, &tokens)
}

/// Escapes Markdown but leaves the token marks and entities (`&lt;`) for the next steps.
fn escape_markdown_keeping_tokens(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        if MARKDOWN_SPECIAL.contains(&character) || character == '>' {
            out.push('\\');
        }
        out.push(character);
    }
    out
}

fn restore_tokens(text: &str, tokens: &[String]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find(TOKEN_START) {
        out.push_str(&rest[..start]);
        let after = &rest[start + TOKEN_START.len_utf8()..];
        let Some(end) = after.find(TOKEN_END) else {
            break;
        };
        if let Some(token) = after[..end]
            .parse::<usize>()
            .ok()
            .and_then(|at| tokens.get(at))
        {
            out.push_str(token);
        }
        rest = &after[end + TOKEN_END.len_utf8()..];
    }
    out.push_str(rest);
    out
}

/// Slack mrkdwn as Orbit Markdown: `*bold*`, `~strike~`, `<url|label>`, `<@user>`, `<!here>`
/// and `&amp;`/`&lt;`/`&gt;`. Code (`` ` `` and ```` ``` ````) keeps its text. With `format` off
/// (Slack's `"mrkdwn": false`) the text shows as it is, links and mentions still work. Markdown
/// that is not Slack syntax (`**bold**`, `[label](url)`) stays as it is.
pub fn mrkdwn_to_markdown(text: &str, format: bool) -> String {
    let mut out = String::with_capacity(text.len() + 16);
    for (fence_index, fenced) in text.split("```").enumerate() {
        if fence_index > 0 {
            out.push_str("```");
        }
        if fence_index % 2 == 1 {
            out.push_str(&unescape_plain(fenced));
            continue;
        }
        let pieces: Vec<&str> = fenced.split('`').collect();
        // An odd backtick has no partner: the text after it is not code.
        let balanced = pieces.len() % 2 == 1;
        for (index, piece) in pieces.iter().enumerate() {
            if index > 0 {
                out.push('`');
            }
            let code = index % 2 == 1 && (balanced || index + 1 < pieces.len());
            if code {
                out.push_str(&unescape_plain(piece));
            } else {
                out.push_str(&convert_text(piece, format));
            }
        }
    }
    out
}

/// `icon_emoji` (`:ghost:`, `:+1::skin-tone-2:`) as an emoji name, or `None` if it is none.
pub fn emoji_name(value: &str) -> Option<String> {
    let name = value.trim().trim_matches(':').split("::").next()?;
    let valid = !name.is_empty()
        && name.chars().count() <= EMOJI_NAME_MAX_CHARS
        && name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "_+-'".contains(character));
    valid.then(|| name.to_owned())
}

/// A Slack color (`good`, `warning`, `danger`, `#36a64f`, `36a64f`, `#3a6`) as `#rrggbb`.
fn color(value: &str) -> Option<String> {
    let value = value.trim();
    let named = match value {
        "good" => Some("#2eb67d"),
        "warning" => Some("#ecb22e"),
        "danger" => Some("#e01e5a"),
        _ => None,
    };
    if let Some(named) = named {
        return Some(named.to_owned());
    }
    let hex = value.trim_start_matches('#');
    if !hex.chars().all(|digit| digit.is_ascii_hexdigit()) {
        return None;
    }
    match hex.len() {
        6 => Some(format!("#{}", hex.to_ascii_lowercase())),
        3 => Some(format!(
            "#{}",
            hex.chars()
                .flat_map(|digit| [digit, digit])
                .collect::<String>()
                .to_ascii_lowercase()
        )),
        _ => None,
    }
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct SlackField {
    title: Option<String>,
    value: Option<String>,
    short: bool,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct SlackAttachment {
    fallback: Option<String>,
    color: Option<String>,
    pretext: Option<String>,
    author_name: Option<String>,
    author_link: Option<String>,
    author_icon: Option<String>,
    title: Option<String>,
    title_link: Option<String>,
    text: Option<String>,
    fields: Vec<SlackField>,
    image_url: Option<String>,
    thumb_url: Option<String>,
    footer: Option<String>,
    footer_icon: Option<String>,
    ts: Option<Value>,
    blocks: Option<Value>,
}

fn markdown(text: Option<String>, max: usize) -> Option<String> {
    text.filter(|text| !text.trim().is_empty())
        .map(|text| cut(&mrkdwn_to_markdown(&text, true), max))
}

fn plain(text: Option<String>, max: usize) -> Option<String> {
    text.filter(|text| !text.trim().is_empty())
        .map(|text| cut(&unescape_plain(&text), max))
}

fn attachment_block(attachment: SlackAttachment) -> AttachmentBlock {
    let fields = attachment
        .fields
        .into_iter()
        .take(20)
        .map(|field| AttachmentField {
            title: plain(field.title, ATTACHMENT_LINE_MAX_CHARS),
            value: markdown(field.value, FIELD_VALUE_MAX_CHARS),
            short: field.short,
        })
        .filter(|field| field.title.is_some() || field.value.is_some())
        .collect();
    let ts = match attachment.ts {
        Some(Value::Number(number)) => number.as_f64().map(|seconds| seconds as i64),
        Some(Value::String(text)) => text.parse::<f64>().ok().map(|seconds| seconds as i64),
        _ => None,
    };
    AttachmentBlock {
        color: attachment.color.as_deref().and_then(color),
        pretext: markdown(attachment.pretext, ATTACHMENT_TEXT_MAX_CHARS),
        author_name: plain(attachment.author_name, ATTACHMENT_LINE_MAX_CHARS),
        author_link: attachment
            .author_link
            .as_deref()
            .and_then(web_url)
            .map(str::to_owned),
        author_icon: attachment.author_icon.as_deref().and_then(https_url),
        title: plain(attachment.title, ATTACHMENT_LINE_MAX_CHARS),
        title_link: attachment
            .title_link
            .as_deref()
            .and_then(web_url)
            .map(str::to_owned),
        text: markdown(attachment.text, ATTACHMENT_TEXT_MAX_CHARS),
        fields,
        image_url: attachment.image_url.as_deref().and_then(https_url),
        thumb_url: attachment.thumb_url.as_deref().and_then(https_url),
        footer: markdown(attachment.footer, ATTACHMENT_LINE_MAX_CHARS),
        footer_icon: attachment.footer_icon.as_deref().and_then(https_url),
        ts,
        fallback: plain(attachment.fallback, ATTACHMENT_TEXT_MAX_CHARS),
    }
}

/// Slack's legacy `attachments` as blocks: each one an attachment block, followed by the blocks
/// it holds. Images that are not https and links that are not http(s) are left out, long texts
/// are cut: Slack posts such attachments, so Orbit does too.
pub fn attachment_blocks(value: &Value) -> Result<Vec<MessageBlock>, BlocksError> {
    let Value::Array(items) = value else {
        return Err(BlocksError("attachments: must be an array".to_owned()));
    };
    if items.len() > MAX_ATTACHMENTS {
        return Err(BlocksError(
            "attachments: a message can have at most 100 attachments".to_owned(),
        ));
    }
    let mut blocks = Vec::new();
    for (index, item) in items.iter().enumerate() {
        let mut attachment = SlackAttachment::deserialize(item)
            .map_err(|error| BlocksError(format!("attachments[{index}]: {error}")))?;
        let inner = match attachment.blocks.take() {
            Some(inner) if !inner.is_null() => {
                parse_blocks(&inner).map_err(|BlocksError(detail)| {
                    BlocksError(format!("attachments[{index}].{detail}"))
                })?
            }
            _ => Vec::new(),
        };
        let block = attachment_block(attachment);
        let shows_something = block.pretext.is_some()
            || block.author_name.is_some()
            || block.title.is_some()
            || block.text.is_some()
            || !block.fields.is_empty()
            || block.image_url.is_some()
            || block.footer.is_some();
        if shows_something || block.fallback.is_some() && inner.is_empty() {
            blocks.push(MessageBlock::Attachment(Box::new(block)));
        }
        blocks.extend(inner);
    }
    Ok(blocks)
}
