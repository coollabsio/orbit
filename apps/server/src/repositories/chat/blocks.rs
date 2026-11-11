//! Layout blocks of an integration's message: a header, sections with fields, context lines,
//! images, dividers, Markdown and rich text, shown as a card. The shape is a subset of Slack's
//! Block Kit, so a Slack payload with these blocks posts as it is. `mrkdwn` text is Orbit Markdown,
//! not Slack's mrkdwn.
//!
//! The types fix the nesting (rich text is at most block > list > section > inline element), so
//! reading untrusted blocks needs no depth limit. They are checked once, when they are posted, and
//! stored in this normal form.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;

const MAX_BLOCKS: usize = 50;
const MAX_RICH_ELEMENTS: usize = 50;
const MAX_INLINE_ELEMENTS: usize = 100;
const MARKDOWN_MAX_CHARS: usize = 4000;
const ID_MAX_CHARS: usize = 64;
const MAX_FIELDS: usize = 10;
const MAX_CONTEXT_ELEMENTS: usize = 10;
const HEADER_MAX_CHARS: usize = 150;
const SECTION_MAX_CHARS: usize = 3000;
const FIELD_MAX_CHARS: usize = 2000;
const CONTEXT_MAX_CHARS: usize = 2000;
const ALT_MAX_CHARS: usize = 2000;
const URL_MAX_CHARS: usize = 2048;
/// Blocks of one message, with the ones made from Slack attachments.
const MAX_MESSAGE_BLOCKS: usize = 100;
/// The stored JSON of all the blocks of one message.
const MAX_BLOCKS_BYTES: usize = 48 * 1024;
const MAX_ATTACHMENT_FIELDS: usize = 20;
/// Block types a payload may post. Others (`actions`, `input`, `video`, …) are left out.
const POSTABLE_TYPES: [&str; 8] = [
    "header",
    "section",
    "context",
    "image",
    "divider",
    "markdown",
    "rich_text",
    "actions",
];
const MAX_BUTTONS: usize = 25;
const BUTTON_TEXT_MAX_CHARS: usize = 75;
const BUTTON_VALUE_MAX_CHARS: usize = 2000;
const ACTION_ID_MAX_CHARS: usize = 255;
const INLINE_TYPES: [&str; 6] = ["text", "link", "emoji", "user", "channel", "broadcast"];
const CONTEXT_TYPES: [&str; 3] = ["plain_text", "mrkdwn", "image"];

/// `plain_text` shows as it is; `mrkdwn` is Markdown with mention tokens, as a message body.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum TextKind {
    PlainText,
    Mrkdwn,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct BlockText {
    #[serde(rename = "type")]
    pub kind: TextKind,
    pub text: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContextElement {
    PlainText { text: String },
    Mrkdwn { text: String },
    Image { image_url: String, alt_text: String },
}

/// The style of a rich text run. Slack's other keys (`highlight`, `underline`, …) are dropped.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq, ToSchema)]
pub struct TextStyle {
    #[serde(default, skip_serializing_if = "is_false")]
    #[schema(required = false)]
    pub bold: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    #[schema(required = false)]
    pub italic: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    #[schema(required = false)]
    pub strike: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    #[schema(required = false)]
    pub code: bool,
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_false(value: &bool) -> bool {
    !value
}

fn is_plain(style: &TextStyle) -> bool {
    *style == TextStyle::default()
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum BroadcastRange {
    Here,
    Channel,
    /// Slack's workspace-wide broadcast; Orbit treats it as `channel`.
    Everyone,
}

/// One run inside a rich text section, quote or code block.
#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RichInline {
    Text {
        text: String,
        #[serde(default, skip_serializing_if = "is_plain")]
        #[schema(required = false)]
        style: TextStyle,
    },
    /// An http(s) or mailto link; without `text` it shows the URL.
    Link {
        url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[schema(required = false)]
        text: Option<String>,
        #[serde(default, skip_serializing_if = "is_plain")]
        #[schema(required = false)]
        style: TextStyle,
    },
    /// A standard or custom emoji by name, without colons.
    Emoji {
        name: String,
    },
    /// An Orbit user id.
    User {
        user_id: String,
    },
    /// An Orbit channel id.
    Channel {
        channel_id: String,
    },
    Broadcast {
        range: BroadcastRange,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ListStyle {
    Bullet,
    Ordered,
}

/// An item of a rich text list: one section.
#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RichListItem {
    RichTextSection { elements: Vec<RichInline> },
}

/// A paragraph-level part of a `rich_text` block. The names are Slack's wire names.
#[allow(clippy::enum_variant_names)]
#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RichElement {
    RichTextSection {
        elements: Vec<RichInline>,
    },
    RichTextList {
        style: ListStyle,
        /// Nesting level, 0 to 8.
        #[serde(default)]
        #[schema(required = false)]
        indent: u8,
        elements: Vec<RichListItem>,
    },
    RichTextQuote {
        elements: Vec<RichInline>,
    },
    RichTextPreformatted {
        elements: Vec<RichInline>,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ButtonStyle {
    Primary,
    Danger,
}

/// A Block Kit button. With `url` it opens the link. Without one it sends `action_id` and `value`
/// to the integration, which needs interactivity (not there yet), so Orbit shows it disabled.
#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct ButtonElement {
    /// `plain_text`, at most 75 characters.
    pub text: BlockText,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(required = false)]
    pub action_id: Option<String>,
    /// An http(s) link the button opens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(required = false)]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(required = false)]
    pub value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(required = false)]
    pub style: Option<ButtonStyle>,
}

/// An interactive element. Only buttons; Slack's menus, date pickers and the like are left out.
#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ActionElement {
    Button(ButtonElement),
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MessageBlock {
    /// Large bold plain text.
    Header {
        text: BlockText,
    },
    /// Text, and up to 10 short fields in two columns. At least one of them.
    Section {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[schema(required = false)]
        text: Option<BlockText>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        #[schema(required = false)]
        fields: Vec<BlockText>,
        /// A button on the right of the section.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[schema(required = false)]
        accessory: Option<ActionElement>,
        /// Names the block in a click payload. Orbit makes one (`b0`, `b1`, …) when it is missing.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[schema(required = false)]
        block_id: Option<String>,
    },
    /// A row of up to 25 buttons.
    Actions {
        elements: Vec<ActionElement>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[schema(required = false)]
        block_id: Option<String>,
    },
    /// A line of small text and icons.
    Context {
        elements: Vec<ContextElement>,
    },
    /// An https image.
    Image {
        image_url: String,
        alt_text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[schema(required = false)]
        title: Option<BlockText>,
    },
    Divider,
    /// Orbit Markdown (Slack's standard Markdown block).
    Markdown {
        text: String,
    },
    /// Styled text: paragraphs, lists, quotes and code blocks.
    RichText {
        elements: Vec<RichElement>,
    },
    /// A Slack legacy attachment: a box with a colored bar. Made from a payload's `attachments`
    /// only; a payload cannot post it in `blocks`.
    Attachment(Box<AttachmentBlock>),
}

/// A legacy attachment, cleaned: texts are Orbit Markdown, links http(s) and images https.
#[derive(Clone, Debug, Default, Deserialize, Serialize, ToSchema)]
pub struct AttachmentBlock {
    /// `#rrggbb`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(required = false)]
    pub color: Option<String>,
    /// Markdown above the box.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(required = false)]
    pub pretext: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(required = false)]
    pub author_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(required = false)]
    pub author_link: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(required = false)]
    pub author_icon: Option<String>,
    /// Plain text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(required = false)]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(required = false)]
    pub title_link: Option<String>,
    /// Markdown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(required = false)]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schema(required = false)]
    pub fields: Vec<AttachmentField>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(required = false)]
    pub image_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(required = false)]
    pub thumb_url: Option<String>,
    /// Markdown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(required = false)]
    pub footer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(required = false)]
    pub footer_icon: Option<String>,
    /// Unix seconds, shown after the footer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(required = false)]
    pub ts: Option<i64>,
    /// Plain text for notifications; not shown in the box.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(required = false)]
    pub fallback: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, ToSchema)]
pub struct AttachmentField {
    /// Plain text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(required = false)]
    pub title: Option<String>,
    /// Markdown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(required = false)]
    pub value: Option<String>,
    /// Half width: two short fields share a row.
    #[serde(default)]
    #[schema(required = false)]
    pub short: bool,
}

/// Why posted blocks were refused, for the `detail` of the error (`blocks[2].text: …`).
#[derive(Debug)]
pub struct BlocksError(pub String);

fn fail(at: impl std::fmt::Display, problem: &str) -> BlocksError {
    BlocksError(format!("{at}: {problem}"))
}

fn check_text(text: &str, max: usize, at: &str) -> Result<(), BlocksError> {
    if text.trim().is_empty() {
        return Err(fail(at, "must not be empty"));
    }
    if text.chars().count() > max {
        return Err(fail(at, &format!("must be at most {max} characters")));
    }
    Ok(())
}

fn check_url(url: &str, at: &str) -> Result<(), BlocksError> {
    let valid = url.chars().count() <= URL_MAX_CHARS
        && url::Url::parse(url)
            .is_ok_and(|parsed| parsed.scheme() == "https" && parsed.host_str().is_some());
    if valid {
        Ok(())
    } else {
        Err(fail(at, "must be an https URL"))
    }
}

fn check(block: &MessageBlock, at: &str) -> Result<(), BlocksError> {
    match block {
        MessageBlock::Header { text } => {
            if text.kind != TextKind::PlainText {
                return Err(fail(format!("{at}.text"), "must be plain_text"));
            }
            check_text(&text.text, HEADER_MAX_CHARS, &format!("{at}.text"))
        }
        MessageBlock::Section {
            text,
            fields,
            accessory,
            block_id,
        } => {
            check_block_id(block_id.as_deref(), at)?;
            if text.is_none() && fields.is_empty() {
                return Err(fail(at, "needs text or fields"));
            }
            if let Some(accessory) = accessory {
                check_action(accessory, &format!("{at}.accessory"))?;
            }
            if let Some(text) = text {
                check_text(&text.text, SECTION_MAX_CHARS, &format!("{at}.text"))?;
            }
            if fields.len() > MAX_FIELDS {
                return Err(fail(format!("{at}.fields"), "can have at most 10 fields"));
            }
            for (index, field) in fields.iter().enumerate() {
                check_text(
                    &field.text,
                    FIELD_MAX_CHARS,
                    &format!("{at}.fields[{index}]"),
                )?;
            }
            Ok(())
        }
        MessageBlock::Context { elements } => {
            if elements.is_empty() || elements.len() > MAX_CONTEXT_ELEMENTS {
                return Err(fail(format!("{at}.elements"), "must have 1 to 10 elements"));
            }
            for (index, element) in elements.iter().enumerate() {
                let at = format!("{at}.elements[{index}]");
                match element {
                    ContextElement::PlainText { text } | ContextElement::Mrkdwn { text } => {
                        check_text(text, CONTEXT_MAX_CHARS, &at)?;
                    }
                    ContextElement::Image {
                        image_url,
                        alt_text,
                    } => {
                        check_url(image_url, &format!("{at}.image_url"))?;
                        check_text(alt_text, ALT_MAX_CHARS, &format!("{at}.alt_text"))?;
                    }
                }
            }
            Ok(())
        }
        MessageBlock::Image {
            image_url,
            alt_text,
            title,
        } => {
            check_url(image_url, &format!("{at}.image_url"))?;
            check_text(alt_text, ALT_MAX_CHARS, &format!("{at}.alt_text"))?;
            if let Some(title) = title {
                check_text(&title.text, HEADER_MAX_CHARS, &format!("{at}.title"))?;
            }
            Ok(())
        }
        MessageBlock::Divider => Ok(()),
        MessageBlock::Actions { elements, block_id } => {
            check_block_id(block_id.as_deref(), at)?;
            if elements.is_empty() || elements.len() > MAX_BUTTONS {
                return Err(fail(format!("{at}.elements"), "must have 1 to 25 elements"));
            }
            for (index, element) in elements.iter().enumerate() {
                check_action(element, &format!("{at}.elements[{index}]"))?;
            }
            Ok(())
        }
        MessageBlock::Attachment(attachment) => check_attachment(attachment, at),
        MessageBlock::Markdown { text } => {
            check_text(text, MARKDOWN_MAX_CHARS, &format!("{at}.text"))
        }
        MessageBlock::RichText { elements } => {
            let at = format!("{at}.elements");
            if elements.is_empty() || elements.len() > MAX_RICH_ELEMENTS {
                return Err(fail(&at, "must have 1 to 50 elements"));
            }
            for (index, element) in elements.iter().enumerate() {
                let at = format!("{at}[{index}]");
                match element {
                    RichElement::RichTextSection { elements }
                    | RichElement::RichTextQuote { elements }
                    | RichElement::RichTextPreformatted { elements } => {
                        check_inline(elements, &format!("{at}.elements"))?;
                    }
                    RichElement::RichTextList {
                        indent, elements, ..
                    } => {
                        if *indent > 8 {
                            return Err(fail(format!("{at}.indent"), "must be 0 to 8"));
                        }
                        if elements.is_empty() || elements.len() > MAX_RICH_ELEMENTS {
                            return Err(fail(format!("{at}.elements"), "must have 1 to 50 items"));
                        }
                        for (item_index, RichListItem::RichTextSection { elements }) in
                            elements.iter().enumerate()
                        {
                            check_inline(
                                elements,
                                &format!("{at}.elements[{item_index}].elements"),
                            )?;
                        }
                    }
                }
            }
            Ok(())
        }
    }
}

fn check_block_id(block_id: Option<&str>, at: &str) -> Result<(), BlocksError> {
    if block_id.is_some_and(|id| id.chars().count() > ACTION_ID_MAX_CHARS) {
        return Err(fail(
            format!("{at}.block_id"),
            "must be at most 255 characters",
        ));
    }
    Ok(())
}

fn check_action(element: &ActionElement, at: &str) -> Result<(), BlocksError> {
    let ActionElement::Button(button) = element;
    if button.text.kind != TextKind::PlainText {
        return Err(fail(format!("{at}.text"), "must be plain_text"));
    }
    check_text(
        &button.text.text,
        BUTTON_TEXT_MAX_CHARS,
        &format!("{at}.text"),
    )?;
    if let Some(url) = &button.url {
        check_web_link(url, &format!("{at}.url"))?;
    }
    if button
        .value
        .as_ref()
        .is_some_and(|value| value.chars().count() > BUTTON_VALUE_MAX_CHARS)
    {
        return Err(fail(
            format!("{at}.value"),
            "must be at most 2000 characters",
        ));
    }
    if button
        .action_id
        .as_ref()
        .is_some_and(|id| id.chars().count() > ACTION_ID_MAX_CHARS)
    {
        return Err(fail(
            format!("{at}.action_id"),
            "must be at most 255 characters",
        ));
    }
    Ok(())
}

fn check_web_link(url: &str, at: &str) -> Result<(), BlocksError> {
    let valid = url.chars().count() <= URL_MAX_CHARS
        && url::Url::parse(url).is_ok_and(|parsed| matches!(parsed.scheme(), "https" | "http"));
    if valid {
        Ok(())
    } else {
        Err(fail(at, "must be an http(s) URL"))
    }
}

fn check_attachment(attachment: &AttachmentBlock, at: &str) -> Result<(), BlocksError> {
    let color_valid = attachment.color.as_deref().is_none_or(|color| {
        color.len() == 7
            && color.starts_with('#')
            && color[1..].chars().all(|digit| digit.is_ascii_hexdigit())
    });
    if !color_valid {
        return Err(fail(format!("{at}.color"), "must be #rrggbb"));
    }
    for (url, name) in [
        (&attachment.author_link, "author_link"),
        (&attachment.title_link, "title_link"),
    ] {
        if let Some(url) = url {
            check_web_link(url, &format!("{at}.{name}"))?;
        }
    }
    for (url, name) in [
        (&attachment.author_icon, "author_icon"),
        (&attachment.image_url, "image_url"),
        (&attachment.thumb_url, "thumb_url"),
        (&attachment.footer_icon, "footer_icon"),
    ] {
        if let Some(url) = url {
            check_url(url, &format!("{at}.{name}"))?;
        }
    }
    if attachment.fields.len() > MAX_ATTACHMENT_FIELDS {
        return Err(fail(format!("{at}.fields"), "can have at most 20 fields"));
    }
    Ok(())
}

fn check_inline(elements: &[RichInline], at: &str) -> Result<(), BlocksError> {
    if elements.len() > MAX_INLINE_ELEMENTS {
        return Err(fail(at, "can have at most 100 elements"));
    }
    for (index, element) in elements.iter().enumerate() {
        let at = format!("{at}[{index}]");
        match element {
            // A run may be only spaces or a line break: Slack sends `"\n"` between lines.
            RichInline::Text { text, .. } => {
                if text.is_empty() || text.chars().count() > MARKDOWN_MAX_CHARS {
                    return Err(fail(format!("{at}.text"), "must be 1 to 4000 characters"));
                }
            }
            RichInline::Link { url, text, .. } => {
                let valid = url.chars().count() <= URL_MAX_CHARS
                    && url::Url::parse(url)
                        .is_ok_and(|parsed| matches!(parsed.scheme(), "https" | "http" | "mailto"));
                if !valid {
                    return Err(fail(
                        format!("{at}.url"),
                        "must be an http(s) or mailto URL",
                    ));
                }
                if let Some(text) = text {
                    check_text(text, CONTEXT_MAX_CHARS, &format!("{at}.text"))?;
                }
            }
            RichInline::Emoji { name } => check_text(name, ID_MAX_CHARS, &format!("{at}.name"))?,
            RichInline::User { user_id } => {
                check_text(user_id, ID_MAX_CHARS, &format!("{at}.user_id"))?;
            }
            RichInline::Channel { channel_id } => {
                check_text(channel_id, ID_MAX_CHARS, &format!("{at}.channel_id"))?;
            }
            RichInline::Broadcast { .. } => {}
        }
    }
    Ok(())
}

/// The plain text of rich text runs, with Orbit's mention tokens, so a fallback body mentions
/// the same people.
fn inline_text(elements: &[RichInline], out: &mut String) {
    for element in elements {
        match element {
            RichInline::Text { text, .. } => out.push_str(text),
            RichInline::Link { url, text, .. } => out.push_str(text.as_deref().unwrap_or(url)),
            RichInline::Emoji { name } => {
                out.push(':');
                out.push_str(name);
                out.push(':');
            }
            RichInline::User { user_id } => out.push_str(&format!("<@{user_id}>")),
            RichInline::Channel { channel_id } => out.push_str(&format!("<#{channel_id}>")),
            RichInline::Broadcast {
                range: BroadcastRange::Here,
            } => out.push_str("<!here>"),
            RichInline::Broadcast { .. } => out.push_str("<!channel>"),
        }
    }
}

fn rich_text(elements: &[RichElement]) -> String {
    let mut out = String::new();
    for element in elements {
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        match element {
            RichElement::RichTextSection { elements }
            | RichElement::RichTextQuote { elements }
            | RichElement::RichTextPreformatted { elements } => inline_text(elements, &mut out),
            RichElement::RichTextList { elements, .. } => {
                for RichListItem::RichTextSection { elements } in elements {
                    out.push_str("- ");
                    inline_text(elements, &mut out);
                    out.push('\n');
                }
            }
        }
    }
    out
}

/// Leaves out what Orbit cannot show, as Slack clients do with what they do not know: block
/// types, rich text runs and context elements of other types. A `date` run becomes its
/// `fallback` text. Returns `None` for a block of another type.
fn postable(item: &Value) -> Option<Value> {
    let kind = item.get("type").and_then(Value::as_str)?;
    if !POSTABLE_TYPES.contains(&kind) {
        return None;
    }
    let mut item = item.clone();
    let keep_inline = |runs: &mut Value| {
        if let Some(runs) = runs.get_mut("elements").and_then(Value::as_array_mut) {
            for run in runs.iter_mut() {
                if run.get("type").and_then(Value::as_str) == Some("date")
                    && let Some(fallback) = run.get("fallback").and_then(Value::as_str)
                {
                    *run = serde_json::json!({ "type": "text", "text": fallback });
                }
            }
            runs.retain(|run| {
                run.get("type")
                    .and_then(Value::as_str)
                    .is_some_and(|kind| INLINE_TYPES.contains(&kind))
            });
        }
    };
    // Only buttons, and only Slack's two button styles ("default" is no style).
    let is_button = |element: &Value| element.get("type").and_then(Value::as_str) == Some("button");
    let clean_style = |element: &mut Value| {
        let known = element
            .get("style")
            .and_then(Value::as_str)
            .is_some_and(|style| matches!(style, "primary" | "danger"));
        if !known && let Some(object) = element.as_object_mut() {
            object.remove("style");
        }
    };
    match kind {
        "actions" => {
            let elements = item.get_mut("elements").and_then(Value::as_array_mut)?;
            elements.retain(is_button);
            elements.iter_mut().for_each(clean_style);
            if elements.is_empty() {
                return None;
            }
        }
        "section" => {
            if let Some(object) = item.as_object_mut() {
                match object.get_mut("accessory") {
                    Some(accessory) if is_button(accessory) => clean_style(accessory),
                    Some(_) => {
                        object.remove("accessory");
                    }
                    None => {}
                }
            }
        }
        "context" => {
            if let Some(elements) = item.get_mut("elements").and_then(Value::as_array_mut) {
                elements.retain(|element| {
                    element
                        .get("type")
                        .and_then(Value::as_str)
                        .is_some_and(|kind| CONTEXT_TYPES.contains(&kind))
                });
            }
        }
        "rich_text" => {
            if let Some(elements) = item.get_mut("elements").and_then(Value::as_array_mut) {
                for element in elements.iter_mut() {
                    if element.get("type").and_then(Value::as_str) == Some("rich_text_list") {
                        if let Some(items) =
                            element.get_mut("elements").and_then(Value::as_array_mut)
                        {
                            items.iter_mut().for_each(keep_inline);
                        }
                    } else {
                        keep_inline(element);
                    }
                }
            }
        }
        _ => {}
    }
    Some(item)
}

/// Slack mrkdwn in text objects becomes Orbit Markdown.
fn to_markdown(block: &mut MessageBlock) {
    let convert = |text: &mut BlockText| {
        if text.kind == TextKind::Mrkdwn {
            text.text = super::slack::mrkdwn_to_markdown(&text.text, true);
        }
    };
    match block {
        MessageBlock::Section { text, fields, .. } => {
            text.iter_mut().for_each(convert);
            fields.iter_mut().for_each(convert);
        }
        MessageBlock::Context { elements } => {
            for element in elements {
                if let ContextElement::Mrkdwn { text } = element {
                    *text = super::slack::mrkdwn_to_markdown(text, true);
                }
            }
        }
        _ => {}
    }
}

/// The blocks of a posted payload, checked, without the parts Orbit cannot show ([`postable`]).
/// Fields Orbit does not use (`block_id`, `emoji`, `accessory`) are dropped. A block of a known
/// type that breaks its rules is refused, as Slack refuses it.
pub fn parse_blocks(value: &Value) -> Result<Vec<MessageBlock>, BlocksError> {
    let Value::Array(items) = value else {
        return Err(BlocksError("blocks: must be an array".to_owned()));
    };
    if items.len() > MAX_BLOCKS {
        return Err(BlocksError(
            "blocks: a message can have at most 50 blocks".to_owned(),
        ));
    }
    let mut blocks = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let at = format!("blocks[{index}]");
        let Some(item) = postable(item) else {
            continue;
        };
        let mut block =
            MessageBlock::deserialize(&item).map_err(|error| fail(&at, &error.to_string()))?;
        check(&block, &at)?;
        to_markdown(&mut block);
        name_actions(&mut block, index);
        blocks.push(block);
    }
    Ok(blocks)
}

/// Gives a block with buttons, and each of its buttons, the id that a click sends back, when the
/// payload has none: `b{block}` and `a{button}`, as positions are stable in a stored message.
fn name_actions(block: &mut MessageBlock, index: usize) {
    let (block_id, buttons): (&mut Option<String>, Vec<&mut ButtonElement>) = match block {
        MessageBlock::Section {
            accessory,
            block_id,
            ..
        } => (
            block_id,
            accessory
                .iter_mut()
                .map(|ActionElement::Button(button)| button)
                .collect(),
        ),
        MessageBlock::Actions { elements, block_id } => (
            block_id,
            elements
                .iter_mut()
                .map(|ActionElement::Button(button)| button)
                .collect(),
        ),
        _ => return,
    };
    if block_id.as_deref().is_none_or(|id| id.trim().is_empty()) {
        *block_id = Some(format!("b{index}"));
    }
    for (position, button) in buttons.into_iter().enumerate() {
        if button
            .action_id
            .as_deref()
            .is_none_or(|id| id.trim().is_empty())
        {
            button.action_id = Some(format!("a{position}"));
        }
    }
}

/// The button of a stored message that a click names. A click carries only these two ids; the
/// rest (`value`, `text`) comes from here, so a member cannot change what the integration gets.
pub fn find_button<'a>(
    blocks: &'a [MessageBlock],
    block_id: &str,
    action_id: &str,
) -> Option<&'a ButtonElement> {
    blocks.iter().find_map(|block| {
        let (id, mut buttons): (&Option<String>, Box<dyn Iterator<Item = &ActionElement>>) =
            match block {
                MessageBlock::Section {
                    accessory,
                    block_id,
                    ..
                } => (block_id, Box::new(accessory.iter())),
                MessageBlock::Actions { elements, block_id } => {
                    (block_id, Box::new(elements.iter()))
                }
                _ => return None,
            };
        if id.as_deref() != Some(block_id) {
            return None;
        }
        buttons.find_map(|ActionElement::Button(button)| {
            (button.action_id.as_deref() == Some(action_id)).then_some(button)
        })
    })
}

/// All the blocks of one message, from `blocks` and `attachments` together.
pub fn check_message_blocks(blocks: &[MessageBlock]) -> Result<(), BlocksError> {
    if blocks.len() > MAX_MESSAGE_BLOCKS {
        return Err(BlocksError(
            "blocks: a message can have at most 100 blocks and attachments".to_owned(),
        ));
    }
    for (index, block) in blocks.iter().enumerate() {
        if let MessageBlock::Attachment(attachment) = block {
            check_attachment(attachment, &format!("attachments[{index}]"))?;
        }
    }
    if to_json(blocks).len() > MAX_BLOCKS_BYTES {
        return Err(BlocksError("blocks: must be at most 48 KiB".to_owned()));
    }
    Ok(())
}

pub(super) fn to_json(blocks: &[MessageBlock]) -> String {
    serde_json::to_string(blocks).unwrap_or_default()
}

/// Plain text of the blocks, for a message posted without `text`: what search, notifications,
/// quotes and previews show. At most `max` characters.
pub fn fallback_text(blocks: &[MessageBlock], max: usize) -> String {
    let mut lines: Vec<String> = Vec::new();
    for block in blocks {
        match block {
            MessageBlock::Header { text } => lines.push(text.text.clone()),
            MessageBlock::Section { text, fields, .. } => {
                lines.extend(text.iter().map(|text| text.text.clone()));
                lines.extend(fields.iter().map(|field| field.text.clone()));
            }
            MessageBlock::Context { elements } => {
                lines.extend(elements.iter().filter_map(|element| match element {
                    ContextElement::PlainText { text } | ContextElement::Mrkdwn { text } => {
                        Some(text.clone())
                    }
                    ContextElement::Image { .. } => None,
                }));
            }
            MessageBlock::Image {
                alt_text, title, ..
            } => lines.push(title.as_ref().map_or(alt_text, |title| &title.text).clone()),
            // Buttons name actions, not content.
            MessageBlock::Divider | MessageBlock::Actions { .. } => {}
            MessageBlock::Markdown { text } => lines.push(text.clone()),
            MessageBlock::Attachment(attachment) => lines.extend(
                [
                    &attachment.fallback,
                    &attachment.pretext,
                    &attachment.title,
                    &attachment.text,
                ]
                .into_iter()
                .flatten()
                .next()
                .cloned(),
            ),
            MessageBlock::RichText { elements } => {
                lines.push(rich_text(elements).trim_end().to_owned());
            }
        }
    }
    lines.join("\n").chars().take(max).collect()
}
