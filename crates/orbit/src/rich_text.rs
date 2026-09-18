//! Rich text documents. Pure domain: an explicit node and mark allowlist, text
//! extraction for search, and mention id extraction for notifications and
//! references. The server derives every stored text field from this module and
//! never trusts client-supplied text or mention lists.
use orbit_platform::Id;
use serde_json::Value;

use crate::workspace::DomainError;

pub const MAX_DEPTH: usize = 50;
pub const MAX_NODES: usize = 10_000;
pub const MAX_BYTES: usize = 262_144;

const LEAF_NODES: &[&str] = &[
    "text",
    "hardBreak",
    "horizontalRule",
    "mention",
    "taskMention",
];
const CONTAINER_NODES: &[&str] = &[
    "doc",
    "paragraph",
    "heading",
    "bulletList",
    "orderedList",
    "listItem",
    "taskList",
    "taskItem",
    "blockquote",
    "codeBlock",
];
const MARKS: &[&str] = &["bold", "italic", "strike", "code", "underline", "link"];

fn allowed_attributes(node_type: &str) -> &'static [&'static str] {
    match node_type {
        "heading" => &["level"],
        "orderedList" => &["start"],
        "codeBlock" => &["language"],
        "taskItem" => &["checked"],
        "mention" => &["id", "label"],
        "taskMention" => &["id", "identifier"],
        _ => &[],
    }
}

fn required_attributes(node_type: &str) -> &'static [&'static str] {
    match node_type {
        "heading" => &["level"],
        "mention" => &["id", "label"],
        "taskMention" => &["id", "identifier"],
        _ => &[],
    }
}

fn invalid(reason: &'static str) -> DomainError {
    DomainError::InvalidDocument { reason }
}

#[must_use]
pub fn empty_document() -> Value {
    serde_json::json!({ "type": "doc", "content": [] })
}

#[must_use]
pub fn is_empty(document: &Value) -> bool {
    extract_text(document).trim().is_empty()
}

/// Validates the node and mark allowlist, the attribute allowlist, and the
/// depth, node count and size caps. Every failure is a caller error (422).
pub fn validate(document: &Value) -> Result<(), DomainError> {
    if document.get("type").and_then(Value::as_str) != Some("doc") {
        return Err(invalid("document root must be a doc node"));
    }
    let serialized =
        serde_json::to_vec(document).map_err(|_| invalid("document is not encodable"))?;
    if serialized.len() > MAX_BYTES {
        return Err(invalid("document is too large"));
    }
    let mut nodes = 0usize;
    validate_node(document, 0, &mut nodes)
}

fn validate_node(node: &Value, depth: usize, nodes: &mut usize) -> Result<(), DomainError> {
    if depth >= MAX_DEPTH {
        return Err(invalid("document nests too deeply"));
    }
    *nodes += 1;
    if *nodes > MAX_NODES {
        return Err(invalid("document has too many nodes"));
    }
    let object = node
        .as_object()
        .ok_or_else(|| invalid("node must be an object"))?;
    let node_type = object
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("node is missing a type"))?;
    let is_leaf = LEAF_NODES.contains(&node_type);
    if !is_leaf && !CONTAINER_NODES.contains(&node_type) {
        return Err(invalid("unsupported node type"));
    }
    for key in object.keys() {
        if !matches!(
            key.as_str(),
            "type" | "attrs" | "content" | "marks" | "text"
        ) {
            return Err(invalid("unsupported node field"));
        }
    }
    validate_attributes(node_type, object.get("attrs"))?;
    if node_type == "text" {
        let text = object
            .get("text")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid("text node is missing text"))?;
        if text.is_empty() {
            return Err(invalid("text node is empty"));
        }
    } else if object.contains_key("text") {
        return Err(invalid("only text nodes carry text"));
    }
    validate_marks(object.get("marks"))?;
    match object.get("content") {
        None => Ok(()),
        Some(Value::Array(children)) => {
            if is_leaf {
                return Err(invalid("leaf node cannot have content"));
            }
            for child in children {
                validate_node(child, depth + 1, nodes)?;
            }
            Ok(())
        }
        Some(_) => Err(invalid("node content must be an array")),
    }
}

fn validate_attributes(node_type: &str, attrs: Option<&Value>) -> Result<(), DomainError> {
    let allowed = allowed_attributes(node_type);
    let attrs = match attrs {
        None => {
            return if required_attributes(node_type).is_empty() {
                Ok(())
            } else {
                Err(invalid("missing node attribute"))
            };
        }
        Some(Value::Object(map)) => map,
        Some(_) => return Err(invalid("node attrs must be an object")),
    };
    for (key, value) in attrs {
        if !allowed.contains(&key.as_str()) {
            return Err(invalid("unsupported node attribute"));
        }
        validate_attribute_value(node_type, key, value)?;
    }
    for key in required_attributes(node_type) {
        if !attrs.contains_key(*key) || attrs[*key].is_null() {
            return Err(invalid("missing node attribute"));
        }
    }
    Ok(())
}

fn validate_attribute_value(node_type: &str, key: &str, value: &Value) -> Result<(), DomainError> {
    match (node_type, key) {
        ("heading", "level") => match value.as_u64() {
            Some(1..=3) => Ok(()),
            _ => Err(invalid("heading level must be 1, 2 or 3")),
        },
        ("orderedList", "start") => match value.as_u64() {
            Some(start) if start >= 1 => Ok(()),
            _ => Err(invalid("ordered list start must be a positive integer")),
        },
        ("codeBlock", "language") => {
            if value.is_null() || value.as_str().is_some_and(|value| value.len() <= 40) {
                Ok(())
            } else {
                Err(invalid("code block language is invalid"))
            }
        }
        ("taskItem", "checked") => {
            if value.is_boolean() {
                Ok(())
            } else {
                Err(invalid("task item checked must be a boolean"))
            }
        }
        (_, "id") => {
            if value
                .as_str()
                .is_some_and(|value| value.parse::<Id>().is_ok())
            {
                Ok(())
            } else {
                Err(invalid("mention id must be an identifier"))
            }
        }
        (_, "label" | "identifier") => {
            if value
                .as_str()
                .is_some_and(|value| !value.is_empty() && value.len() <= 200)
            {
                Ok(())
            } else {
                Err(invalid("mention label is invalid"))
            }
        }
        _ => Err(invalid("unsupported node attribute")),
    }
}

fn validate_marks(marks: Option<&Value>) -> Result<(), DomainError> {
    let Some(marks) = marks else { return Ok(()) };
    let marks = marks
        .as_array()
        .ok_or_else(|| invalid("marks must be an array"))?;
    for mark in marks {
        let object = mark
            .as_object()
            .ok_or_else(|| invalid("mark must be an object"))?;
        let mark_type = object
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid("mark is missing a type"))?;
        if !MARKS.contains(&mark_type) {
            return Err(invalid("unsupported mark type"));
        }
        for key in object.keys() {
            if !matches!(key.as_str(), "type" | "attrs") {
                return Err(invalid("unsupported mark field"));
            }
        }
        match (mark_type, object.get("attrs")) {
            ("link", Some(Value::Object(attrs))) => {
                for key in attrs.keys() {
                    if !matches!(key.as_str(), "href" | "target" | "rel") {
                        return Err(invalid("unsupported mark attribute"));
                    }
                }
                let href = attrs
                    .get("href")
                    .and_then(Value::as_str)
                    .ok_or_else(|| invalid("link href must be http or https"))?;
                if !(href.starts_with("http://") || href.starts_with("https://"))
                    || href.len() > 2_000
                {
                    return Err(invalid("link href must be http or https"));
                }
            }
            ("link", _) => return Err(invalid("link href must be http or https")),
            (_, Some(_)) => return Err(invalid("unsupported mark attribute")),
            (_, None) => {}
        }
    }
    Ok(())
}

/// Plain text for `description_text` / `body_text` and the fts5 row. Blocks are
/// joined with newlines; mentions render as their cached label so search finds
/// the words a reader actually sees.
#[must_use]
pub fn extract_text(document: &Value) -> String {
    let mut blocks = Vec::new();
    collect_blocks(document, &mut blocks);
    blocks.join("\n")
}

fn collect_blocks(node: &Value, blocks: &mut Vec<String>) {
    let Some(node_type) = node.get("type").and_then(Value::as_str) else {
        return;
    };
    // listItem and taskItem are containers: their text always lives in a child paragraph, so
    // treating them as blocks too would emit every list entry twice.
    if matches!(node_type, "paragraph" | "heading" | "codeBlock") {
        let mut inline = String::new();
        collect_inline(node, &mut inline);
        if !inline.is_empty() {
            blocks.push(inline);
        }
    }
    if let Some(children) = node.get("content").and_then(Value::as_array) {
        for child in children {
            let child_type = child
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if matches!(node_type, "paragraph" | "heading" | "codeBlock")
                && !matches!(
                    child_type,
                    "bulletList" | "orderedList" | "taskList" | "blockquote"
                )
            {
                continue;
            }
            collect_blocks(child, blocks);
        }
    }
}

fn collect_inline(node: &Value, out: &mut String) {
    let Some(node_type) = node.get("type").and_then(Value::as_str) else {
        return;
    };
    match node_type {
        "text" => out.push_str(node.get("text").and_then(Value::as_str).unwrap_or_default()),
        "hardBreak" => out.push('\n'),
        "mention" => {
            out.push('@');
            out.push_str(attr_str(node, "label"));
        }
        "taskMention" => out.push_str(attr_str(node, "identifier")),
        "bulletList" | "orderedList" | "taskList" | "blockquote" => return,
        _ => {}
    }
    if let Some(children) = node.get("content").and_then(Value::as_array) {
        for child in children {
            collect_inline(child, out);
        }
    }
}

fn attr_str<'a>(node: &'a Value, key: &str) -> &'a str {
    node.get("attrs")
        .and_then(|attrs| attrs.get(key))
        .and_then(Value::as_str)
        .unwrap_or_default()
}

/// Referenced task ids, in document order, deduplicated. Feeds notifications now
/// and `task_references` in plan 04.
#[must_use]
pub fn extract_task_ids(document: &Value) -> Vec<Id> {
    collect_ids(document, "taskMention")
}

/// Mentioned user ids, in document order, deduplicated. The server uses this
/// instead of a client-supplied `mentioned_user_ids` array.
#[must_use]
pub fn extract_user_ids(document: &Value) -> Vec<Id> {
    collect_ids(document, "mention")
}

fn collect_ids(node: &Value, node_type: &str) -> Vec<Id> {
    let mut found = Vec::new();
    walk_ids(node, node_type, &mut found);
    found
}

fn walk_ids(node: &Value, node_type: &str, found: &mut Vec<Id>) {
    if node.get("type").and_then(Value::as_str) == Some(node_type)
        && let Ok(id) = attr_str(node, "id").parse::<Id>()
        && !found.contains(&id)
    {
        found.push(id);
    }
    if let Some(children) = node.get("content").and_then(Value::as_array) {
        for child in children {
            walk_ids(child, node_type, found);
        }
    }
}

/// Converts legacy Markdown to a TipTap document for the 0014 backfill.
///
/// Headings deeper than 3 clamp to 3 because the allowlist stops there. Images,
/// tables, footnotes and raw HTML are dropped - the old renderer never produced
/// them from user input. `@Name` mentions survive as plain text: the old mention
/// ids were never persisted, so there is nothing to convert them to.
#[must_use]
pub fn markdown_to_document(markdown: &str) -> Value {
    use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

    type Frame = (String, serde_json::Map<String, Value>, Vec<Value>);

    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TABLES);

    // Stack of open block nodes; index 0 is the doc.
    let mut stack: Vec<Frame> = vec![("doc".to_owned(), serde_json::Map::new(), Vec::new())];
    let mut marks: Vec<Value> = Vec::new();
    let mut code_text = String::new();
    let mut in_code_block = false;
    let mut skipped_depth = 0usize;

    fn open(stack: &mut Vec<Frame>, name: &str, attrs: serde_json::Map<String, Value>) {
        stack.push((name.to_owned(), attrs, Vec::new()));
    }
    /// A tight Markdown list puts text straight inside the item, but TipTap's
    /// `listItem` schema is `paragraph block*`. Wrap each run of inline nodes so
    /// the document stays valid and `extract_text` still sees the words.
    fn wrap_inline_runs(children: Vec<Value>) -> Vec<Value> {
        let is_inline = |node: &Value| {
            matches!(
                node.get("type").and_then(Value::as_str),
                Some("text" | "hardBreak" | "mention" | "taskMention")
            )
        };
        if !children.iter().any(is_inline) {
            return children;
        }
        let mut wrapped: Vec<Value> = Vec::with_capacity(children.len());
        let mut run: Vec<Value> = Vec::new();
        for child in children {
            if is_inline(&child) {
                run.push(child);
            } else {
                if !run.is_empty() {
                    wrapped.push(serde_json::json!({
                        "type": "paragraph",
                        "content": std::mem::take(&mut run)
                    }));
                }
                wrapped.push(child);
            }
        }
        if !run.is_empty() {
            wrapped.push(serde_json::json!({ "type": "paragraph", "content": run }));
        }
        wrapped
    }

    fn close(stack: &mut Vec<Frame>) {
        let Some((name, attrs, children)) = stack.pop() else {
            return;
        };
        let children = if name == "listItem" || name == "taskItem" {
            wrap_inline_runs(children)
        } else {
            children
        };
        let mut node = serde_json::Map::new();
        node.insert("type".to_owned(), Value::String(name.clone()));
        if !attrs.is_empty() {
            node.insert("attrs".to_owned(), Value::Object(attrs));
        }
        if !children.is_empty() {
            node.insert("content".to_owned(), Value::Array(children));
        }
        if name == "paragraph" && !node.contains_key("content") {
            return;
        }
        if let Some(parent) = stack.last_mut() {
            parent.2.push(Value::Object(node));
        }
    }
    fn push_text(stack: &mut [Frame], marks: &[Value], text: &str) {
        if text.is_empty() {
            return;
        }
        let mut node = serde_json::Map::new();
        node.insert("type".to_owned(), Value::String("text".to_owned()));
        node.insert("text".to_owned(), Value::String(text.to_owned()));
        if !marks.is_empty() {
            node.insert("marks".to_owned(), Value::Array(marks.to_vec()));
        }
        if let Some(parent) = stack.last_mut() {
            parent.2.push(Value::Object(node));
        }
    }
    fn mark(name: &str) -> Value {
        serde_json::json!({ "type": name })
    }
    fn active_marks(marks: &[Value]) -> Vec<Value> {
        marks
            .iter()
            .filter(|mark| !mark.is_null())
            .cloned()
            .collect()
    }

    for event in Parser::new_ext(markdown, options) {
        if skipped_depth > 0 {
            match event {
                Event::Start(_) => skipped_depth += 1,
                Event::End(_) => skipped_depth -= 1,
                _ => {}
            }
            continue;
        }
        match event {
            Event::Start(Tag::Paragraph) => open(&mut stack, "paragraph", serde_json::Map::new()),
            Event::End(TagEnd::Paragraph) => close(&mut stack),
            Event::Start(Tag::Heading { level, .. }) => {
                let level = match level {
                    HeadingLevel::H1 => 1u64,
                    HeadingLevel::H2 => 2,
                    _ => 3,
                };
                let mut attrs = serde_json::Map::new();
                attrs.insert("level".to_owned(), Value::from(level));
                open(&mut stack, "heading", attrs);
            }
            Event::End(TagEnd::Heading(_)) => close(&mut stack),
            Event::Start(Tag::BlockQuote(_)) => {
                open(&mut stack, "blockquote", serde_json::Map::new());
            }
            Event::End(TagEnd::BlockQuote(_)) => close(&mut stack),
            Event::Start(Tag::List(start)) => {
                let mut attrs = serde_json::Map::new();
                let name = if let Some(start) = start {
                    if start != 1 {
                        attrs.insert("start".to_owned(), Value::from(start.max(1)));
                    }
                    "orderedList"
                } else {
                    "bulletList"
                };
                open(&mut stack, name, attrs);
            }
            Event::End(TagEnd::List(_)) => close(&mut stack),
            Event::Start(Tag::Item) => open(&mut stack, "listItem", serde_json::Map::new()),
            Event::End(TagEnd::Item) => close(&mut stack),
            Event::Start(Tag::CodeBlock(kind)) => {
                let mut attrs = serde_json::Map::new();
                if let CodeBlockKind::Fenced(language) = kind
                    && !language.is_empty()
                {
                    attrs.insert(
                        "language".to_owned(),
                        Value::String(language.chars().take(40).collect()),
                    );
                }
                open(&mut stack, "codeBlock", attrs);
                in_code_block = true;
                code_text.clear();
            }
            Event::End(TagEnd::CodeBlock) => {
                let text = code_text.trim_end_matches('\n').to_owned();
                push_text(&mut stack, &[], &text);
                code_text.clear();
                in_code_block = false;
                close(&mut stack);
            }
            Event::Start(Tag::Emphasis) => marks.push(mark("italic")),
            Event::End(TagEnd::Emphasis) => {
                marks.pop();
            }
            Event::Start(Tag::Strong) => marks.push(mark("bold")),
            Event::End(TagEnd::Strong) => {
                marks.pop();
            }
            Event::Start(Tag::Strikethrough) => marks.push(mark("strike")),
            Event::End(TagEnd::Strikethrough) => {
                marks.pop();
            }
            Event::Start(Tag::Link { dest_url, .. }) => {
                if dest_url.starts_with("http://") || dest_url.starts_with("https://") {
                    marks.push(serde_json::json!({
                        "type": "link",
                        "attrs": { "href": dest_url.chars().take(2_000).collect::<String>() }
                    }));
                } else {
                    marks.push(Value::Null);
                }
            }
            Event::End(TagEnd::Link) => {
                marks.pop();
            }
            Event::Start(Tag::Image { .. }) => skipped_depth = 1,
            Event::Start(Tag::Table(_) | Tag::FootnoteDefinition(_) | Tag::MetadataBlock(_)) => {
                skipped_depth = 1;
            }
            Event::Text(text) | Event::InlineHtml(text) => {
                if in_code_block {
                    code_text.push_str(&text);
                } else {
                    push_text(&mut stack, &active_marks(&marks), &text);
                }
            }
            Event::Code(text) => {
                let mut active = active_marks(&marks);
                active.push(mark("code"));
                push_text(&mut stack, &active, &text);
            }
            // CommonMark would render a lone newline as a space. Both sources we convert
            // treat it as a line break (Orbit's old renderer and Discord), so keep it.
            Event::SoftBreak | Event::HardBreak => {
                if let Some(parent) = stack.last_mut() {
                    parent.2.push(serde_json::json!({ "type": "hardBreak" }));
                }
            }
            Event::Rule => {
                if let Some(parent) = stack.last_mut() {
                    parent
                        .2
                        .push(serde_json::json!({ "type": "horizontalRule" }));
                }
            }
            _ => {}
        }
    }

    while stack.len() > 1 {
        close(&mut stack);
    }
    let (_, _, content) = stack.pop().expect("the doc frame is never popped");
    serde_json::json!({ "type": "doc", "content": content })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn heading(level: u64, text: &str) -> serde_json::Value {
        json!({ "type": "heading", "attrs": { "level": level },
                "content": [{ "type": "text", "text": text }] })
    }

    #[test]
    fn accepts_a_document_using_every_allowlisted_node_and_mark() {
        let document = json!({
            "type": "doc",
            "content": [
                heading(3, "Release plan"),
                { "type": "paragraph", "content": [
                    { "type": "text", "text": "bold", "marks": [{ "type": "bold" }] },
                    { "type": "text", "text": "italic", "marks": [{ "type": "italic" }] },
                    { "type": "text", "text": "strike", "marks": [{ "type": "strike" }] },
                    { "type": "text", "text": "code", "marks": [{ "type": "code" }] },
                    { "type": "text", "text": "under", "marks": [{ "type": "underline" }] },
                    { "type": "text", "text": "link", "marks": [
                        { "type": "link", "attrs": { "href": "https://example.com", "target": "_blank", "rel": "noreferrer" } }
                    ] },
                    { "type": "hardBreak" },
                    { "type": "mention", "attrs": { "id": "0193c0de-0000-7000-8000-000000000001", "label": "Ada Lovelace" } },
                    { "type": "taskMention", "attrs": { "id": "0193c0de-0000-7000-8000-000000000002", "identifier": "ORB-12" } }
                ] },
                { "type": "bulletList", "content": [
                    { "type": "listItem", "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": "one" }] }] }
                ] },
                { "type": "orderedList", "attrs": { "start": 3 }, "content": [
                    { "type": "listItem", "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": "two" }] }] }
                ] },
                { "type": "taskList", "content": [
                    { "type": "taskItem", "attrs": { "checked": true },
                      "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": "done" }] }] }
                ] },
                { "type": "blockquote", "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": "quoted" }] }] },
                { "type": "codeBlock", "attrs": { "language": "rust" }, "content": [{ "type": "text", "text": "fn main() {}" }] },
                { "type": "horizontalRule" }
            ]
        });

        assert_eq!(validate(&document), Ok(()));
    }

    #[test]
    fn rejects_a_disallowed_node() {
        let document =
            json!({ "type": "doc", "content": [{ "type": "image", "attrs": { "src": "x" } }] });

        assert_eq!(
            validate(&document),
            Err(DomainError::InvalidDocument {
                reason: "unsupported node type"
            })
        );
    }

    #[test]
    fn rejects_a_disallowed_mark() {
        let document = json!({ "type": "doc", "content": [
            { "type": "paragraph", "content": [
                { "type": "text", "text": "x", "marks": [{ "type": "highlight" }] }
            ] }
        ] });

        assert_eq!(
            validate(&document),
            Err(DomainError::InvalidDocument {
                reason: "unsupported mark type"
            })
        );
    }

    #[test]
    fn rejects_a_disallowed_attribute_and_an_out_of_range_heading() {
        let extra = json!({ "type": "doc", "content": [
            { "type": "paragraph", "attrs": { "align": "center" } }
        ] });
        assert_eq!(
            validate(&extra),
            Err(DomainError::InvalidDocument {
                reason: "unsupported node attribute"
            })
        );

        assert_eq!(
            validate(&json!({ "type": "doc", "content": [heading(4, "too deep")] })),
            Err(DomainError::InvalidDocument {
                reason: "heading level must be 1, 2 or 3"
            })
        );
    }

    #[test]
    fn rejects_a_non_http_link_and_a_mention_without_an_id() {
        let link = json!({ "type": "doc", "content": [
            { "type": "paragraph", "content": [
                { "type": "text", "text": "x", "marks": [{ "type": "link", "attrs": { "href": "javascript:alert(1)" } }] }
            ] }
        ] });
        assert_eq!(
            validate(&link),
            Err(DomainError::InvalidDocument {
                reason: "link href must be http or https"
            })
        );

        let mention = json!({ "type": "doc", "content": [
            { "type": "paragraph", "content": [{ "type": "mention", "attrs": { "label": "Ada" } }] }
        ] });
        assert_eq!(
            validate(&mention),
            Err(DomainError::InvalidDocument {
                reason: "missing node attribute"
            })
        );
    }

    #[test]
    fn rejects_a_document_that_is_too_deep() {
        let mut node =
            json!({ "type": "paragraph", "content": [{ "type": "text", "text": "deep" }] });
        for _ in 0..MAX_DEPTH {
            node = json!({ "type": "blockquote", "content": [node] });
        }
        let document = json!({ "type": "doc", "content": [node] });

        assert_eq!(
            validate(&document),
            Err(DomainError::InvalidDocument {
                reason: "document nests too deeply"
            })
        );
    }

    #[test]
    fn rejects_a_document_with_too_many_nodes() {
        let content: Vec<serde_json::Value> = (0..MAX_NODES)
            .map(|_| json!({ "type": "paragraph" }))
            .collect();
        let document = json!({ "type": "doc", "content": content });

        assert_eq!(
            validate(&document),
            Err(DomainError::InvalidDocument {
                reason: "document has too many nodes"
            })
        );
    }

    #[test]
    fn rejects_a_document_that_is_too_large() {
        let document = json!({ "type": "doc", "content": [
            { "type": "paragraph", "content": [{ "type": "text", "text": "x".repeat(MAX_BYTES) }] }
        ] });

        assert_eq!(
            validate(&document),
            Err(DomainError::InvalidDocument {
                reason: "document is too large"
            })
        );
    }

    #[test]
    fn rejects_a_root_that_is_not_a_doc() {
        assert_eq!(
            validate(&json!({ "type": "paragraph" })),
            Err(DomainError::InvalidDocument {
                reason: "document root must be a doc node"
            })
        );
        assert_eq!(
            validate(&json!("hello")),
            Err(DomainError::InvalidDocument {
                reason: "document root must be a doc node"
            })
        );
    }

    #[test]
    fn extracts_text_across_nested_lists_blocks_and_mentions() {
        let document = json!({ "type": "doc", "content": [
            heading(1, "Plan"),
            { "type": "bulletList", "content": [
                { "type": "listItem", "content": [
                    { "type": "paragraph", "content": [{ "type": "text", "text": "outer" }] },
                    { "type": "orderedList", "content": [
                        { "type": "listItem", "content": [
                            { "type": "paragraph", "content": [{ "type": "text", "text": "inner" }] }
                        ] }
                    ] }
                ] }
            ] },
            { "type": "paragraph", "content": [
                { "type": "text", "text": "ping " },
                { "type": "mention", "attrs": { "id": "0193c0de-0000-7000-8000-000000000001", "label": "Ada Lovelace" } },
                { "type": "text", "text": " about " },
                { "type": "taskMention", "attrs": { "id": "0193c0de-0000-7000-8000-000000000002", "identifier": "ORB-12" } }
            ] },
            { "type": "codeBlock", "content": [{ "type": "text", "text": "let x = 1;" }] },
            { "type": "horizontalRule" }
        ] });

        assert_eq!(
            extract_text(&document),
            "Plan\nouter\ninner\nping @Ada Lovelace about ORB-12\nlet x = 1;"
        );
    }

    #[test]
    fn extracts_task_ids_in_order_without_duplicates() {
        let first = "0193c0de-0000-7000-8000-000000000002";
        let second = "0193c0de-0000-7000-8000-000000000003";
        let document = json!({ "type": "doc", "content": [
            { "type": "paragraph", "content": [
                { "type": "taskMention", "attrs": { "id": first, "identifier": "ORB-12" } },
                { "type": "taskMention", "attrs": { "id": second, "identifier": "ORB-13" } }
            ] },
            { "type": "blockquote", "content": [
                { "type": "paragraph", "content": [
                    { "type": "taskMention", "attrs": { "id": first, "identifier": "ORB-12" } }
                ] }
            ] }
        ] });

        assert_eq!(
            extract_task_ids(&document),
            vec![first.parse::<Id>().unwrap(), second.parse::<Id>().unwrap()]
        );
        assert!(extract_user_ids(&document).is_empty());
    }

    #[test]
    fn extracts_user_ids_in_order_without_duplicates() {
        let ada = "0193c0de-0000-7000-8000-000000000001";
        let grace = "0193c0de-0000-7000-8000-000000000004";
        let document = json!({ "type": "doc", "content": [
            { "type": "paragraph", "content": [
                { "type": "mention", "attrs": { "id": grace, "label": "Grace" } },
                { "type": "mention", "attrs": { "id": ada, "label": "Ada" } },
                { "type": "mention", "attrs": { "id": grace, "label": "Grace Hopper" } }
            ] }
        ] });

        assert_eq!(
            extract_user_ids(&document),
            vec![grace.parse::<Id>().unwrap(), ada.parse::<Id>().unwrap()]
        );
        assert!(extract_task_ids(&document).is_empty());
    }

    #[test]
    fn empty_documents_are_recognised() {
        assert!(is_empty(&empty_document()));
        assert!(is_empty(
            &json!({ "type": "doc", "content": [{ "type": "paragraph" }] })
        ));
        assert!(!is_empty(&json!({ "type": "doc", "content": [
            { "type": "paragraph", "content": [{ "type": "text", "text": "x" }] }
        ] })));
        assert_eq!(validate(&empty_document()), Ok(()));
    }

    #[test]
    fn converts_headings_lists_quotes_and_code_from_markdown() {
        let markdown = "# Title\n\n## Sub\n\nParagraph text.\n\n- one\n- two\n\n1. first\n2. second\n\n> quoted\n\n```rust\nfn main() {}\n```\n\n---\n";

        let document = markdown_to_document(markdown);

        assert_eq!(validate(&document), Ok(()));
        let content = document["content"].as_array().unwrap();
        assert_eq!(content[0], heading(1, "Title"));
        assert_eq!(content[1], heading(2, "Sub"));
        assert_eq!(content[2]["type"], "paragraph");
        assert_eq!(content[3]["type"], "bulletList");
        assert_eq!(
            content[3]["content"][1]["content"][0]["content"][0]["text"],
            "two"
        );
        assert_eq!(content[4]["type"], "orderedList");
        assert_eq!(content[5]["type"], "blockquote");
        assert_eq!(
            content[6],
            json!({
                "type": "codeBlock",
                "attrs": { "language": "rust" },
                "content": [{ "type": "text", "text": "fn main() {}" }]
            })
        );
        assert_eq!(content[7], json!({ "type": "horizontalRule" }));
    }

    #[test]
    fn converts_inline_marks_and_links_from_markdown() {
        let document =
            markdown_to_document("**bold** _italic_ ~~gone~~ `code` [site](https://example.com)");

        assert_eq!(validate(&document), Ok(()));
        let inline = document["content"][0]["content"].as_array().unwrap();
        assert_eq!(
            inline[0],
            json!({ "type": "text", "text": "bold", "marks": [{ "type": "bold" }] })
        );
        assert_eq!(
            inline[2],
            json!({ "type": "text", "text": "italic", "marks": [{ "type": "italic" }] })
        );
        assert_eq!(
            inline[4],
            json!({ "type": "text", "text": "gone", "marks": [{ "type": "strike" }] })
        );
        assert_eq!(
            inline[6],
            json!({ "type": "text", "text": "code", "marks": [{ "type": "code" }] })
        );
        assert_eq!(
            inline[8],
            json!({
                "type": "text",
                "text": "site",
                "marks": [{ "type": "link", "attrs": { "href": "https://example.com" } }]
            })
        );
    }

    #[test]
    fn clamps_deep_headings_and_drops_unsupported_constructs() {
        let document = markdown_to_document(
            "###### deep\n\n![alt](https://example.com/a.png)\n\n| a | b |\n| - | - |\n| 1 | 2 |\n",
        );

        assert_eq!(validate(&document), Ok(()));
        assert_eq!(document["content"][0]["attrs"]["level"], 3);
        let text = extract_text(&document);
        assert!(text.starts_with("deep"));
        assert!(!text.contains("https://example.com/a.png"));
    }

    #[test]
    fn preserves_at_mentions_as_plain_text_because_ids_were_never_stored() {
        let document = markdown_to_document("ping @Ada Lovelace about this");

        assert_eq!(validate(&document), Ok(()));
        assert!(extract_user_ids(&document).is_empty());
        assert_eq!(extract_text(&document), "ping @Ada Lovelace about this");
    }

    #[test]
    fn converts_empty_markdown_to_an_empty_document() {
        assert_eq!(markdown_to_document(""), empty_document());
        assert_eq!(markdown_to_document("   \n\n  "), empty_document());
        assert!(is_empty(&markdown_to_document("")));
    }

    #[test]
    fn a_single_newline_stays_a_line_break_rather_than_becoming_a_space() {
        // CommonMark renders a lone newline as a space, but both sources we convert
        // treat it as a line break: Orbit's old renderer put every line in its own
        // paragraph, and Discord shows every newline.
        let document = markdown_to_document("first line\nsecond line\n\nnew paragraph");

        assert_eq!(validate(&document), Ok(()));
        assert_eq!(
            document["content"][0]["content"][1],
            json!({ "type": "hardBreak" })
        );
        assert_eq!(
            extract_text(&document),
            "first line\nsecond line\nnew paragraph"
        );
    }
}
