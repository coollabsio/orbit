//! The editor's content policy on the server (port of `apps/web/src/features/docs/editor/content.ts`):
//! only block types of the editor schema, links to http(s), mailto and `/docs/<page id>`, and
//! image/file URLs that are page files or absolute http(s) URLs. Applied to JSON before it becomes a
//! collaborative document, and to the document itself (a member can send raw Yjs updates).

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::Value;
use yrs::types::text::YChange;
use yrs::{Any, Out, ReadTxn, Text, TransactionMut, Xml, XmlElementRef, XmlFragment, XmlOut};

use super::blocknote::schema;

/// Blocks whose `props.url` is a page file or an external URL.
const FILE_BLOCKS: [&str; 2] = ["image", "file"];

/// Characters browsers strip from URLs before they parse the scheme.
fn is_url_noise(c: char) -> bool {
    matches!(c,
        '\u{0}'..='\u{20}'
        | '\u{7f}'..='\u{9f}'
        | '\u{a0}'
        | '\u{1680}'
        | '\u{180e}'
        | '\u{2000}'..='\u{2029}'
        | '\u{205f}'
        | '\u{3000}'
        | '\u{feff}')
}

fn strip_noise(value: &str) -> String {
    value.chars().filter(|c| !is_url_noise(*c)).collect()
}

fn is_uuid(value: &str, lowercase_only: bool) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 36
        && bytes.iter().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => *byte == b'-',
            _ => {
                byte.is_ascii_digit()
                    || (b'a'..=b'f').contains(byte)
                    || (!lowercase_only && (b'A'..=b'F').contains(byte))
            }
        })
}

fn starts_with_ignore_case(value: &str, prefix: &str) -> bool {
    value.len() >= prefix.len()
        && value.is_char_boundary(prefix.len())
        && value[..prefix.len()].eq_ignore_ascii_case(prefix)
}

fn has_safe_scheme(value: &str) -> bool {
    ["http://", "https://", "mailto:"]
        .iter()
        .any(|prefix| starts_with_ignore_case(value, prefix))
}

/// `/docs/<uuid>` exactly (any hex case), what the Notion import writes for links between pages.
fn is_internal_page_link(value: &str) -> bool {
    value
        .strip_prefix("/docs/")
        .is_some_and(|id| is_uuid(id, false))
}

/// `^[a-z][a-z0-9+.-]*:` (case-insensitive).
fn has_any_scheme(value: &str) -> bool {
    let mut chars = value.chars();
    if !chars.next().is_some_and(|c| c.is_ascii_alphabetic()) {
        return false;
    }
    for c in chars {
        if c == ':' {
            return true;
        }
        if !(c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-')) {
            return false;
        }
    }
    false
}

/// `^[^:/]*\.[^:/]*:\d+(?:[/?#]|$)`: a dotted host with a port ("example.com:8080/x").
fn is_dotted_host_with_port(value: &str) -> bool {
    let Some(colon) = value.find(':') else {
        return false;
    };
    let host = &value[..colon];
    if host.contains('/') || !host.contains('.') {
        return false;
    }
    let rest = &value[colon + 1..];
    let digits = rest.chars().take_while(char::is_ascii_digit).count();
    digits > 0
        && rest[digits..]
            .chars()
            .next()
            .is_none_or(|c| matches!(c, '/' | '?' | '#'))
}

/// Normalizes a link href: keeps http(s)/mailto and `/docs/<uuid>`, upgrades bare hosts
/// ("example.com/x") to https, and returns `None` for everything else.
#[must_use]
pub fn normalize_link_href(href: &str) -> Option<String> {
    let cleaned = strip_noise(href);
    if cleaned.is_empty() {
        return None;
    }
    if has_safe_scheme(&cleaned) {
        return Some(cleaned);
    }
    if cleaned == href && is_internal_page_link(&cleaned) {
        return Some(cleaned);
    }
    if has_any_scheme(&cleaned) && !is_dotted_host_with_port(&cleaned) {
        return None;
    }
    if cleaned.starts_with(['/', '?', '#', '.', '\\']) {
        return None;
    }
    let host = cleaned.split(['/', '?', '#']).next().unwrap_or("");
    if !host.contains('.') {
        return None;
    }
    Some(format!("https://{cleaned}"))
}

/// `/api/v1/workspaces/<uuid>/pages/<uuid>/files/<uuid>` (lowercase ids, what the server hands out).
fn is_page_file_path(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("/api/v1/workspaces/") else {
        return false;
    };
    let parts: Vec<&str> = rest.split('/').collect();
    matches!(parts.as_slice(), [workspace, "pages", page, "files", file]
        if is_uuid(workspace, true) && is_uuid(page, true) && is_uuid(file, true))
}

/// Image/file URL policy: empty (waiting for an upload), a page file, or an absolute http(s) URL.
#[must_use]
pub fn is_safe_file_url(url: &str) -> bool {
    if url.is_empty() || is_page_file_path(url) {
        return true;
    }
    let after_scheme = if starts_with_ignore_case(url, "http://") {
        &url[7..]
    } else if starts_with_ignore_case(url, "https://") {
        &url[8..]
    } else {
        return false;
    };
    strip_noise(url) == url
        && after_scheme
            .chars()
            .next()
            .is_some_and(|c| c != '/' && c != '\\')
}

/// Drops blocks of unknown types, unwraps unsafe links and blanks unsafe file URLs, like the
/// editor's `toEditorContent`.
#[must_use]
pub fn sanitize_blocks(blocks: &[Value]) -> Vec<Value> {
    let known = keep_known_blocks(blocks);
    let Value::Array(linked) = sanitize_links(&Value::Array(known)) else {
        return Vec::new();
    };
    linked.into_iter().map(sanitize_file_block).collect()
}

fn keep_known_blocks(blocks: &[Value]) -> Vec<Value> {
    blocks
        .iter()
        .filter(|block| {
            block
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|block_type| schema().blocks.contains_key(block_type))
        })
        .map(|block| {
            let mut block = block.clone();
            if let Some(children) = block.get("children").and_then(Value::as_array) {
                block["children"] = Value::Array(keep_known_blocks(children));
            }
            block
        })
        .collect()
}

fn sanitize_links(value: &Value) -> Value {
    match value {
        Value::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                if item.get("type").and_then(Value::as_str) == Some("link") && item.is_object() {
                    let content = match item.get("content") {
                        Some(Value::Array(content)) => Value::Array(content.clone()),
                        _ => Value::Array(Vec::new()),
                    };
                    let content = sanitize_links(&content);
                    match item
                        .get("href")
                        .and_then(Value::as_str)
                        .and_then(normalize_link_href)
                    {
                        Some(href) => {
                            let mut link = item.clone();
                            link["href"] = Value::String(href);
                            link["content"] = content;
                            out.push(link);
                        }
                        None => {
                            if let Value::Array(parts) = content {
                                out.extend(parts);
                            }
                        }
                    }
                    continue;
                }
                out.push(sanitize_links(item));
            }
            Value::Array(out)
        }
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, child)| (key.clone(), sanitize_links(child)))
                .collect(),
        ),
        other => other.clone(),
    }
}

fn sanitize_file_block(mut block: Value) -> Value {
    let is_file_block = block
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|block_type| FILE_BLOCKS.contains(&block_type));
    if is_file_block
        && let Some(props) = block.get_mut("props").and_then(Value::as_object_mut)
        && !props
            .get("url")
            .and_then(Value::as_str)
            .is_some_and(is_safe_file_url)
    {
        props.insert("url".to_owned(), Value::String(String::new()));
    }
    if let Some(children) = block.get_mut("children").and_then(Value::as_array_mut) {
        let taken = std::mem::take(children);
        *children = taken.into_iter().map(sanitize_file_block).collect();
    }
    block
}

/// Rewrites unsafe links and file URLs inside a live document; returns whether anything changed.
/// The caller broadcasts the resulting update like any other edit.
pub fn repair_document(txn: &mut TransactionMut, fragment: &yrs::XmlFragmentRef) -> bool {
    let mut elements = Vec::new();
    let mut texts = Vec::new();
    for child in fragment.children(txn) {
        collect(txn, child, &mut elements, &mut texts);
    }
    let mut changed = false;
    for element in elements {
        let tag = element.tag().to_string();
        if !FILE_BLOCKS.contains(&tag.as_str()) {
            continue;
        }
        let safe = match element.get_attribute(txn, "url") {
            None | Some(Out::Any(Any::Undefined | Any::Null)) => true,
            Some(Out::Any(Any::String(url))) => is_safe_file_url(&url),
            Some(_) => false,
        };
        if !safe {
            element.insert_attribute(txn, "url", Any::from(String::new()));
            changed = true;
        }
    }
    for text in texts {
        let mut fixes: Vec<(u32, u32, Option<String>)> = Vec::new();
        let mut offset = 0_u32;
        for chunk in text.diff(txn, YChange::identity) {
            let length = match &chunk.insert {
                Out::Any(Any::String(value)) => value.encode_utf16().count() as u32,
                _ => 1,
            };
            if let Some(link) = chunk
                .attributes
                .as_ref()
                .and_then(|attrs| attrs.get("link"))
            {
                let href = match link {
                    Any::Map(map) => match map.get("href") {
                        Some(Any::String(href)) => Some(href.to_string()),
                        _ => None,
                    },
                    _ => None,
                };
                let normalized = href.as_deref().and_then(normalize_link_href);
                if normalized != href || href.is_none() {
                    fixes.push((offset, length, normalized));
                }
            }
            offset += length;
        }
        for (index, length, href) in fixes {
            let value = match href {
                Some(href) => {
                    let mut map = HashMap::new();
                    map.insert("href".to_owned(), Any::from(href));
                    Any::Map(Arc::new(map))
                }
                None => Any::Null,
            };
            let mut attrs = yrs::types::Attrs::new();
            attrs.insert(Arc::from("link"), value);
            text.format(txn, index, length, attrs);
            changed = true;
        }
    }
    changed
}

fn collect<T: ReadTxn>(
    txn: &T,
    node: XmlOut,
    elements: &mut Vec<XmlElementRef>,
    texts: &mut Vec<yrs::XmlTextRef>,
) {
    match node {
        XmlOut::Element(element) => {
            for child in element.children(txn) {
                collect(txn, child, elements, texts);
            }
            elements.push(element);
        }
        XmlOut::Text(text) => texts.push(text),
        XmlOut::Fragment(fragment) => {
            for child in fragment.children(txn) {
                collect(txn, child, elements, texts);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const PAGE: &str = "0199a0b0-0000-7000-8000-000000000001";

    #[test]
    fn links_follow_the_editor_policy() {
        let cases = [
            ("https://example.com/a", Some("https://example.com/a")),
            ("HTTP://x.y", Some("HTTP://x.y")),
            ("mailto:a@b.c", Some("mailto:a@b.c")),
            (" java\u{0}script:alert(1)", None),
            ("javascript:alert(1)", None),
            ("data:text/html,x", None),
            ("example.com/x", Some("https://example.com/x")),
            ("example.com:8080/x", Some("https://example.com:8080/x")),
            ("localhost:3000", None),
            ("/relative", None),
            ("#frag", None),
            ("", None),
            ("noDot", None),
        ];
        for (input, expected) in cases {
            assert_eq!(normalize_link_href(input).as_deref(), expected, "{input:?}");
        }
        let internal = format!("/docs/{PAGE}");
        assert_eq!(normalize_link_href(&internal), Some(internal.clone()));
        assert_eq!(normalize_link_href(&format!("{internal}/x")), None);
    }

    #[test]
    fn file_urls_follow_the_editor_policy() {
        let file = format!("/api/v1/workspaces/{PAGE}/pages/{PAGE}/files/{PAGE}");
        for (url, safe) in [
            ("", true),
            (file.as_str(), true),
            ("https://cdn.example.com/a.png", true),
            ("http://cdn.example.com/a.png", true),
            ("https:///x", false),
            ("https://a b", false),
            ("javascript:alert(1)", false),
            ("data:image/png;base64,AA", false),
            ("/api/v1/other", false),
        ] {
            assert_eq!(is_safe_file_url(url), safe, "{url:?}");
        }
    }

    #[test]
    fn live_documents_are_repaired_in_place() {
        use yrs::{Transact, WriteTxn};

        use crate::collab::blocknote::{FRAGMENT, doc_to_blocks, new_doc, write_blocks};

        let doc = new_doc(None);
        {
            let mut txn = doc.transact_mut();
            let fragment = txn.get_or_insert_xml_fragment(FRAGMENT);
            write_blocks(
                &mut txn,
                &fragment,
                &[
                    json!({"id": "i", "type": "image", "props": {"url": "javascript:alert(1)"}}),
                    json!({"id": "p", "type": "paragraph", "content": [
                        {"type": "link", "href": "javascript:alert(1)", "content": "bad"},
                        {"type": "text", "text": " ", "styles": {}},
                        {"type": "link", "href": "https://ok.example", "content": "good"}
                    ]}),
                ],
            );
        }
        let repaired = {
            let mut txn = doc.transact_mut();
            let fragment = txn.get_or_insert_xml_fragment(FRAGMENT);
            repair_document(&mut txn, &fragment)
        };
        assert!(repaired);
        let blocks = doc_to_blocks(&doc);
        assert_eq!(blocks[0]["props"]["url"], "");
        assert_eq!(
            blocks[1]["content"],
            json!([
                {"type": "text", "text": "bad ", "styles": {}},
                {"type": "link", "href": "https://ok.example", "content": [{"type": "text", "text": "good", "styles": {}}]}
            ])
        );
        let again = {
            let mut txn = doc.transact_mut();
            let fragment = txn.get_or_insert_xml_fragment(FRAGMENT);
            repair_document(&mut txn, &fragment)
        };
        assert!(!again, "a repaired document needs no further repair");
    }

    #[test]
    fn blocks_are_sanitized_like_to_editor_content() {
        let blocks = [
            json!({"type": "paragraph", "content": [
                {"type": "link", "href": "javascript:x", "content": [{"type": "text", "text": "bad", "styles": {}}]},
                {"type": "link", "href": "example.com", "content": [{"type": "text", "text": "ok", "styles": {}}]}
            ], "children": [{"type": "video"}, {"type": "image", "props": {"url": "data:x"}}]}),
            json!({"type": "unknown"}),
        ];
        let sanitized = sanitize_blocks(&blocks);
        assert_eq!(sanitized.len(), 1);
        assert_eq!(
            sanitized[0]["content"],
            json!([
                {"type": "text", "text": "bad", "styles": {}},
                {"type": "link", "href": "https://example.com", "content": [{"type": "text", "text": "ok", "styles": {}}]}
            ])
        );
        assert_eq!(sanitized[0]["children"].as_array().unwrap().len(), 1);
        assert_eq!(sanitized[0]["children"][0]["props"]["url"], "");
    }
}
