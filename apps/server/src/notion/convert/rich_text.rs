//! Notion rich text → BlockNote inline content (`text` and `link` items).

use reqwest::Url;
use serde_json::{Map, Value, json};

use super::ConvertContext;
use crate::notion::model::normalize_id;

/// BlockNote's default color names; Notion uses the same palette.
const COLORS: &[&str] = &[
    "gray", "brown", "red", "orange", "yellow", "green", "blue", "purple", "pink",
];

const NOTION_HOSTS: &[&str] = &[
    "notion.so",
    "www.notion.so",
    "app.notion.com",
    "notion.com",
    "www.notion.com",
];

/// Splits a Notion color (`red`, `red_background`, `default`) into BlockNote
/// `(textColor, backgroundColor)`. Unknown colors map to neither.
pub(crate) fn split_color(color: &str) -> (Option<&'static str>, Option<&'static str>) {
    let (name, background) = match color.strip_suffix("_background") {
        Some(name) => (name, true),
        None => (color, false),
    };
    let Some(name) = COLORS.iter().copied().find(|known| *known == name) else {
        return (None, None);
    };
    if background {
        (None, Some(name))
    } else {
        (Some(name), None)
    }
}

/// A styled BlockNote text item.
pub(crate) fn text(value: impl Into<String>, styles: Map<String, Value>) -> Value {
    json!({"type": "text", "text": value.into(), "styles": Value::Object(styles)})
}

/// An unstyled BlockNote text item.
pub(crate) fn plain(value: impl Into<String>) -> Value {
    text(value, Map::new())
}

/// A BlockNote link item wrapping styled text items.
pub(crate) fn link(href: impl Into<String>, content: Vec<Value>) -> Value {
    json!({"type": "link", "href": href.into(), "content": content})
}

/// Converts a Notion rich text array to BlockNote inline content. Empty text runs are dropped
/// and adjacent runs of the same link are merged into one link item.
pub(crate) fn rich_text_to_inline(items: &[Value], ctx: &mut ConvertContext<'_>) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    for item in items {
        if let Some(mention) = member_mention(item, ctx) {
            out.push(mention);
            continue;
        }
        let Some((content, href)) = convert_item(item, ctx) else {
            continue;
        };
        match href {
            Some(href) => {
                if let Some(last) = out.last_mut()
                    && last.get("type").and_then(Value::as_str) == Some("link")
                    && last.get("href").and_then(Value::as_str) == Some(href.as_str())
                    && let Some(Value::Array(existing)) = last.get_mut("content")
                {
                    existing.push(content);
                } else {
                    out.push(link(href, vec![content]));
                }
            }
            None => out.push(content),
        }
    }
    out
}

/// A Notion user mention whose email belongs to an Orbit member → an Orbit `mention`.
fn member_mention(item: &Value, ctx: &ConvertContext<'_>) -> Option<Value> {
    if str_at(item, &["type"]) != Some("mention")
        || str_at(item, &["mention", "type"]) != Some("user")
    {
        return None;
    }
    let email = str_at(item, &["mention", "user", "person", "email"])?
        .trim()
        .to_lowercase();
    let (user_id, name) = ctx.resolver.member_by_email(&email)?;
    Some(json!({"type": "mention", "props": {"userId": user_id, "name": name}}))
}

/// One rich text item → (styled text item, optional link target).
fn convert_item(item: &Value, ctx: &mut ConvertContext<'_>) -> Option<(Value, Option<String>)> {
    let mut styles = styles(item.get("annotations"));
    let plain_text = str_at(item, &["plain_text"]).unwrap_or_default().to_owned();
    let kind = str_at(item, &["type"]).unwrap_or_default();
    let (value, href) = match kind {
        "text" => {
            let content = str_at(item, &["text", "content"])
                .map(str::to_owned)
                .unwrap_or(plain_text);
            let href = str_at(item, &["text", "link", "url"])
                .or_else(|| str_at(item, &["href"]))
                .and_then(|url| resolve_href(url, ctx));
            (content, href)
        }
        "equation" => {
            styles.insert("code".into(), Value::Bool(true));
            let expression = str_at(item, &["equation", "expression"])
                .map(str::to_owned)
                .unwrap_or(plain_text);
            (expression, None)
        }
        "mention" => mention(item, plain_text, ctx),
        other => {
            ctx.lossy(&format!("rich_text:{other}"));
            let href = str_at(item, &["href"]).and_then(|url| resolve_href(url, ctx));
            (plain_text, href)
        }
    };
    if value.is_empty() {
        return None;
    }
    Some((text(value, styles), href))
}

fn mention(
    item: &Value,
    plain_text: String,
    ctx: &mut ConvertContext<'_>,
) -> (String, Option<String>) {
    let mention = item.get("mention").unwrap_or(&Value::Null);
    let kind = str_at(mention, &["type"]).unwrap_or_default();
    let data = mention.get(kind).unwrap_or(&Value::Null);
    match kind {
        "page" | "database" | "data_source" => {
            let id = str_at(data, &["id"]).and_then(normalize_id);
            let mut label = plain_text;
            if (label.is_empty() || label == "Untitled")
                && let Some(title) = id.as_deref().and_then(|id| ctx.resolver.page_title(id))
            {
                label = title;
            }
            if label.is_empty() {
                label = "Untitled".to_owned();
            }
            let href = match id.as_deref() {
                Some(id) => Some(ctx.page_href(id, str_at(item, &["href"]))),
                None => str_at(item, &["href"]).and_then(|url| resolve_href(url, ctx)),
            };
            (label, href)
        }
        "user" => {
            let name = str_at(data, &["name"])
                .map(str::to_owned)
                .or_else(|| str_at(data, &["id"]).and_then(|id| ctx.resolver.user_name(id)));
            let label = match name {
                Some(name) => format!("@{name}"),
                None if plain_text.starts_with('@') => plain_text,
                None if plain_text.is_empty() => "@Unknown user".to_owned(),
                None => format!("@{plain_text}"),
            };
            (label, None)
        }
        "date" => (format_date_value(data).unwrap_or(plain_text), None),
        "link_preview" => {
            let url = str_at(data, &["url"]).or_else(|| str_at(item, &["href"]));
            let label = if plain_text.is_empty() {
                url.unwrap_or_default().to_owned()
            } else {
                plain_text
            };
            (label, url.and_then(|url| resolve_href(url, ctx)))
        }
        "link_mention" => {
            let url = str_at(data, &["href"]).or_else(|| str_at(item, &["href"]));
            let label = str_at(data, &["title"])
                .map(str::to_owned)
                .filter(|title| !title.is_empty())
                .unwrap_or(plain_text);
            (label, url.and_then(|url| resolve_href(url, ctx)))
        }
        "custom_emoji" => {
            let label = str_at(data, &["name"])
                .map(|name| format!(":{name}:"))
                .unwrap_or(plain_text);
            (label, None)
        }
        "template_mention" => (plain_text, None),
        other => {
            ctx.lossy(&format!("mention:{other}"));
            let href = str_at(item, &["href"]).and_then(|url| resolve_href(url, ctx));
            (plain_text, href)
        }
    }
}

fn styles(annotations: Option<&Value>) -> Map<String, Value> {
    let mut styles = Map::new();
    let Some(annotations) = annotations else {
        return styles;
    };
    for (notion, blocknote) in [
        ("bold", "bold"),
        ("italic", "italic"),
        ("underline", "underline"),
        ("strikethrough", "strike"),
        ("code", "code"),
    ] {
        if annotations.get(notion).and_then(Value::as_bool) == Some(true) {
            styles.insert(blocknote.into(), Value::Bool(true));
        }
    }
    if let Some(color) = annotations.get("color").and_then(Value::as_str) {
        match split_color(color) {
            (Some(text_color), _) => {
                styles.insert("textColor".into(), json!(text_color));
            }
            (_, Some(background)) => {
                styles.insert("backgroundColor".into(), json!(background));
            }
            _ => {}
        }
    }
    styles
}

/// Rewrites a link target: Notion page links → `/docs/<orbit id>` when the page is imported,
/// else an absolute Notion URL; other links pass through when their scheme is `http`, `https`,
/// `mailto` or `tel`. Anything else (e.g. `javascript:`) is dropped and reported.
pub(crate) fn resolve_href(raw: &str, ctx: &mut ConvertContext<'_>) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() || raw.starts_with('#') {
        return None;
    }
    if let Some(id) = notion_page_id(raw) {
        let fallback = if raw.starts_with('/') {
            format!("https://www.notion.so{raw}")
        } else {
            raw.to_owned()
        };
        return Some(ctx.page_href(&id, Some(&fallback)));
    }
    if raw.starts_with("//") {
        ctx.lossy("unsafe_link");
        return None;
    }
    if raw.starts_with('/') {
        return Some(format!("https://www.notion.so{raw}"));
    }
    match Url::parse(raw) {
        Ok(url) if matches!(url.scheme(), "http" | "https" | "mailto" | "tel") => {
            Some(raw.to_owned())
        }
        _ => {
            ctx.lossy("unsafe_link");
            None
        }
    }
}

/// The Notion page id a link points to (`/<id>`, `/Title-<id>`, `notion.so/...`, `?p=<id>`).
pub(crate) fn notion_page_id(raw: &str) -> Option<String> {
    let url = if raw.starts_with('/') && !raw.starts_with("//") {
        Url::parse(&format!("https://www.notion.so{raw}")).ok()?
    } else {
        let url = Url::parse(raw).ok()?;
        let host = url.host_str()?.to_ascii_lowercase();
        if !(NOTION_HOSTS.contains(&host.as_str()) || host.ends_with(".notion.site")) {
            return None;
        }
        url
    };
    if let Some(id) = url
        .query_pairs()
        .find(|(key, _)| key == "p")
        .and_then(|(_, value)| normalize_id(&value))
    {
        return Some(id);
    }
    let segment = url
        .path_segments()?
        .rev()
        .find(|segment| !segment.is_empty())?;
    if let Some(id) = normalize_id(segment) {
        return Some(id);
    }
    let tail = segment.get(segment.len().checked_sub(32)?..)?;
    normalize_id(tail)
}

/// Formats a Notion date object: `start`, `start → end`, plus ` (time zone)` when set.
pub(crate) fn format_date_value(date: &Value) -> Option<String> {
    let start = str_at(date, &["start"])?;
    let mut out = format_timestamp(start);
    if let Some(end) = str_at(date, &["end"]) {
        out.push_str(" → ");
        out.push_str(&format_timestamp(end));
    }
    if let Some(zone) = str_at(date, &["time_zone"]) {
        out.push_str(&format!(" ({zone})"));
    }
    Some(out)
}

/// `2026-09-01` stays; `2026-09-01T09:00:00.000Z` → `2026-09-01 09:00 UTC`;
/// `2026-09-15T09:00:00-07:00` → `2026-09-15 09:00 UTC-07:00`.
pub(crate) fn format_timestamp(raw: &str) -> String {
    let Some((date, time)) = raw.split_once('T') else {
        return raw.to_owned();
    };
    let clock: String = time.chars().take(5).collect();
    let zone = if time.ends_with('Z') {
        " UTC".to_owned()
    } else if let Some(position) = time.rfind(['+', '-']) {
        format!(" UTC{}", &time[position..])
    } else {
        String::new()
    };
    format!("{date} {clock}{zone}")
}

pub(crate) fn str_at<'a>(value: &'a Value, path: &[&str]) -> Option<&'a str> {
    let mut current = value;
    for key in path {
        current = current.get(key)?;
    }
    current.as_str()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_colors() {
        assert_eq!(split_color("red"), (Some("red"), None));
        assert_eq!(split_color("blue_background"), (None, Some("blue")));
        assert_eq!(split_color("default"), (None, None));
        assert_eq!(split_color("default_background"), (None, None));
        assert_eq!(split_color("teal"), (None, None));
    }

    #[test]
    fn finds_notion_page_ids_in_links() {
        let id = Some("3c612f56-fdd0-4a30-a4d6-bda7d7426309".to_owned());
        assert_eq!(notion_page_id("/3c612f56fdd04a30a4d6bda7d7426309"), id);
        assert_eq!(
            notion_page_id("https://www.notion.so/acme/Plan-3c612f56fdd04a30a4d6bda7d7426309#abc"),
            id
        );
        assert_eq!(
            notion_page_id("https://app.notion.com/p/3c612f56fdd04a30a4d6bda7d7426309"),
            id
        );
        assert_eq!(
            notion_page_id("https://www.notion.so/db?v=1&p=3c612f56fdd04a30a4d6bda7d7426309"),
            id
        );
        assert_eq!(
            notion_page_id("https://acme.notion.site/Plan-3c612f56fdd04a30a4d6bda7d7426309"),
            id
        );
        assert_eq!(
            notion_page_id("https://example.com/3c612f56fdd04a30a4d6bda7d7426309"),
            None
        );
        assert_eq!(notion_page_id("https://www.notion.so/pricing"), None);
    }

    #[test]
    fn formats_timestamps() {
        assert_eq!(format_timestamp("2026-09-01"), "2026-09-01");
        assert_eq!(
            format_timestamp("2026-09-01T09:00:00.000Z"),
            "2026-09-01 09:00 UTC"
        );
        assert_eq!(
            format_timestamp("2026-09-15T09:00:00-07:00"),
            "2026-09-15 09:00 UTC-07:00"
        );
        assert_eq!(format_timestamp("2026-09-15T09:00:00"), "2026-09-15 09:00");
    }
}
