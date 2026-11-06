//! Asana rich text (`html_notes`, a story's `html_text`) to BlockNote blocks.
//!
//! Asana writes an XML fragment in a `<body>` root with a small tag set
//! (<https://developers.asana.com/docs/rich-text>): `strong`, `em`, `u`, `s`, `code`, `a`,
//! `ul`/`ol`/`li`, `h1`/`h2`, `pre`, `blockquote`, `hr` and `img`. Line breaks are newline
//! characters in text. Images are dropped (Asana lists them as task attachments too, and the
//! import attaches those). Anything else is kept as its text. Nesting deeper than [`MAX_DEPTH`] is
//! flattened to text, so a hostile document cannot overflow the stack. The output is sanitized
//! again by the task repository.

use serde_json::{Map, Value, json};

use orbit_platform::Id;

/// Element nesting the parser keeps; deeper elements become their text.
const MAX_DEPTH: usize = 24;
/// Rich text longer than this is cut before parsing (task descriptions hold 100,000 characters).
const MAX_INPUT_CHARS: usize = 200_000;

#[derive(Debug, PartialEq)]
enum Node {
    Text(String),
    Element {
        name: String,
        href: Option<String>,
        children: Vec<Node>,
    },
}

/// Converts Asana rich text to BlockNote blocks (an empty body gives no blocks).
#[must_use]
pub fn html_to_blocks(html: &str) -> Vec<Value> {
    let html: String = html.chars().take(MAX_INPUT_CHARS).collect();
    let nodes = parse(&html);
    let nodes = match nodes.as_slice() {
        [Node::Element { name, children, .. }] if name == "body" => children.as_slice(),
        _ => nodes.as_slice(),
    };
    let mut out = Vec::new();
    blocks(nodes, &mut out);
    out
}

/// A paragraph of plain text; `bold` prefix runs make labels such as "**Fields**".
#[must_use]
pub fn paragraph(content: Vec<Value>) -> Value {
    block("paragraph", Map::new(), Some(content), Vec::new())
}

#[must_use]
pub fn bullet(content: Vec<Value>) -> Value {
    block("bulletListItem", Map::new(), Some(content), Vec::new())
}

#[must_use]
pub fn text(value: &str) -> Value {
    styled(value, &Styles::default())
}

#[must_use]
pub fn bold(value: &str) -> Value {
    styled(
        value,
        &Styles {
            bold: true,
            ..Styles::default()
        },
    )
}

#[must_use]
pub fn link(href: &str, label: &str) -> Value {
    json!({"type": "link", "href": href, "content": [text(label)]})
}

fn block(
    kind: &str,
    props: Map<String, Value>,
    content: Option<Vec<Value>>,
    children: Vec<Value>,
) -> Value {
    let mut block = Map::new();
    block.insert("id".into(), json!(Id::new_v7().to_string()));
    block.insert("type".into(), json!(kind));
    block.insert("props".into(), Value::Object(props));
    if let Some(content) = content {
        block.insert("content".into(), Value::Array(content));
    }
    block.insert("children".into(), Value::Array(children));
    Value::Object(block)
}

#[derive(Clone, Copy, Default)]
struct Styles {
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
    code: bool,
}

fn styled(value: &str, styles: &Styles) -> Value {
    let mut map = Map::new();
    for (on, name) in [
        (styles.bold, "bold"),
        (styles.italic, "italic"),
        (styles.underline, "underline"),
        (styles.strike, "strike"),
        (styles.code, "code"),
    ] {
        if on {
            map.insert(name.into(), json!(true));
        }
    }
    json!({"type": "text", "text": value, "styles": Value::Object(map)})
}

fn is_block(name: &str) -> bool {
    matches!(
        name,
        "ul" | "ol" | "li" | "h1" | "h2" | "pre" | "blockquote" | "hr" | "body" | "p"
    )
}

/// Block-level conversion: runs of inline nodes become paragraphs (one per line).
fn blocks(nodes: &[Node], out: &mut Vec<Value>) {
    let mut lines: Vec<Vec<Value>> = vec![Vec::new()];
    let flush = |lines: &mut Vec<Vec<Value>>, out: &mut Vec<Value>| {
        let count = lines.len();
        for (index, line) in lines.drain(..).enumerate() {
            // A trailing newline before a block element is not an empty paragraph.
            if !(line.is_empty() && index + 1 == count) {
                out.push(paragraph(line));
            }
        }
        lines.push(Vec::new());
    };
    for node in nodes {
        match node {
            Node::Element { name, children, .. } if is_block(name) => {
                flush(&mut lines, out);
                block_element(name, children, out);
            }
            _ => inline_lines(node, &Styles::default(), None, &mut lines),
        }
    }
    let last_is_empty = lines.len() == 1 && lines[0].is_empty();
    if !last_is_empty {
        flush(&mut lines, out);
    }
}

fn block_element(name: &str, children: &[Node], out: &mut Vec<Value>) {
    match name {
        "h1" | "h2" => {
            let mut props = Map::new();
            props.insert("level".into(), json!(if name == "h1" { 1 } else { 2 }));
            out.push(block("heading", props, Some(inline(children)), Vec::new()));
        }
        "ul" | "ol" => list(name == "ol", children, out),
        "li" => list_item("bulletListItem", children, out),
        "pre" => {
            let mut props = Map::new();
            props.insert("language".into(), json!("text"));
            let code = plain_text(children);
            out.push(block(
                "codeBlock",
                props,
                Some(vec![text(code.trim_end_matches('\n'))]),
                Vec::new(),
            ));
        }
        "blockquote" => out.push(block(
            "quote",
            Map::new(),
            Some(inline(children)),
            Vec::new(),
        )),
        "hr" => out.push(block("divider", Map::new(), None, Vec::new())),
        _ => blocks(children, out),
    }
}

fn list(numbered: bool, items: &[Node], out: &mut Vec<Value>) {
    let kind = if numbered {
        "numberedListItem"
    } else {
        "bulletListItem"
    };
    for item in items {
        match item {
            Node::Element { name, children, .. } if name == "li" => list_item(kind, children, out),
            // Whitespace between items.
            Node::Text(value) if value.trim().is_empty() => {}
            other => blocks(std::slice::from_ref(other), out),
        }
    }
}

fn list_item(kind: &str, children: &[Node], out: &mut Vec<Value>) {
    let (nested, own): (Vec<&Node>, Vec<&Node>) = children.iter().partition(
        |child| matches!(child, Node::Element { name, .. } if name == "ul" || name == "ol"),
    );
    let mut content = Vec::new();
    for node in own {
        inline_into(node, &Styles::default(), None, &mut content);
    }
    trim_trailing_newlines(&mut content);
    let mut sublist = Vec::new();
    for node in nested {
        if let Node::Element { name, children, .. } = node {
            list(name == "ol", children, &mut sublist);
        }
    }
    out.push(block(kind, Map::new(), Some(content), sublist));
}

fn inline(nodes: &[Node]) -> Vec<Value> {
    let mut content = Vec::new();
    for node in nodes {
        inline_into(node, &Styles::default(), None, &mut content);
    }
    trim_trailing_newlines(&mut content);
    content
}

fn trim_trailing_newlines(content: &mut Vec<Value>) {
    while let Some(last) = content.last_mut() {
        let Some(value) = last.get("text").and_then(Value::as_str) else {
            return;
        };
        let trimmed = value.trim_end_matches('\n').to_owned();
        if trimmed.is_empty() {
            content.pop();
        } else {
            last["text"] = json!(trimmed);
            return;
        }
    }
}

/// Inline content of one node, newlines kept in the text.
fn inline_into(node: &Node, styles: &Styles, href: Option<&str>, out: &mut Vec<Value>) {
    match node {
        Node::Text(value) if value.is_empty() => {}
        Node::Text(value) => push_text(value, styles, href, out),
        Node::Element {
            name,
            href: own,
            children,
        } => {
            let styles = with_style(name, styles);
            let href = safe_href(own.as_deref()).or(href);
            for child in children {
                inline_into(child, &styles, href, out);
            }
        }
    }
}

/// Inline content of one node at block level, where a newline starts a new paragraph.
fn inline_lines(node: &Node, styles: &Styles, href: Option<&str>, lines: &mut Vec<Vec<Value>>) {
    match node {
        Node::Text(value) => {
            for (index, part) in value.split('\n').enumerate() {
                if index > 0 {
                    lines.push(Vec::new());
                }
                if !part.is_empty() {
                    let line = lines.last_mut().expect("at least one line");
                    push_text(part, styles, href, line);
                }
            }
        }
        Node::Element {
            name,
            href: own,
            children,
        } => {
            let styles = with_style(name, styles);
            let href = safe_href(own.as_deref()).or(href);
            for child in children {
                inline_lines(child, &styles, href, lines);
            }
        }
    }
}

fn push_text(value: &str, styles: &Styles, href: Option<&str>, out: &mut Vec<Value>) {
    let item = styled(value, styles);
    match href {
        Some(href) => {
            if let Some(last) = out.last_mut()
                && last.get("type").and_then(Value::as_str) == Some("link")
                && last.get("href").and_then(Value::as_str) == Some(href)
                && let Some(Value::Array(content)) = last.get_mut("content")
            {
                content.push(item);
                return;
            }
            out.push(json!({"type": "link", "href": href, "content": [item]}));
        }
        None => out.push(item),
    }
}

fn with_style(name: &str, styles: &Styles) -> Styles {
    let mut styles = *styles;
    match name {
        "strong" | "b" => styles.bold = true,
        "em" | "i" => styles.italic = true,
        "u" => styles.underline = true,
        "s" | "del" | "strike" => styles.strike = true,
        "code" => styles.code = true,
        _ => {}
    }
    styles
}

fn safe_href(href: Option<&str>) -> Option<&str> {
    let href = href?.trim();
    let lower = href.to_ascii_lowercase();
    (lower.starts_with("https://") || lower.starts_with("http://") || lower.starts_with("mailto:"))
        .then_some(href)
}

fn plain_text(nodes: &[Node]) -> String {
    let mut out = String::new();
    for node in nodes {
        match node {
            Node::Text(value) => out.push_str(value),
            Node::Element { children, .. } => out.push_str(&plain_text(children)),
        }
    }
    out
}

/// A tolerant parser for Asana's XML subset: unknown or unbalanced tags never fail, entities
/// are decoded, and only `href` is kept from attributes.
fn parse(input: &str) -> Vec<Node> {
    // Open elements: (name, href, children). The root has no name.
    let mut stack: Vec<(String, Option<String>, Vec<Node>)> =
        vec![(String::new(), None, Vec::new())];
    let mut rest = input;
    while !rest.is_empty() {
        let Some(start) = rest.find('<') else {
            push_node(&mut stack, Node::Text(decode_entities(rest)));
            break;
        };
        if start > 0 {
            push_node(&mut stack, Node::Text(decode_entities(&rest[..start])));
        }
        rest = &rest[start..];
        let Some(end) = rest.find('>') else {
            push_node(&mut stack, Node::Text(decode_entities(rest)));
            break;
        };
        let tag = &rest[1..end];
        rest = &rest[end + 1..];
        if tag.starts_with('!') || tag.starts_with('?') {
            continue;
        }
        if let Some(name) = tag.strip_prefix('/') {
            let name = name.trim().to_ascii_lowercase();
            if let Some(index) = stack.iter().rposition(|(open, _, _)| *open == name)
                && index > 0
            {
                while stack.len() > index {
                    close(&mut stack);
                }
            }
            continue;
        }
        let self_closing = tag.ends_with('/');
        let tag = tag.trim_end_matches('/');
        let name_end = tag.find(char::is_whitespace).unwrap_or(tag.len());
        let name = tag[..name_end].to_ascii_lowercase();
        if name.is_empty() {
            continue;
        }
        let href = attribute(&tag[name_end..], "href");
        if self_closing || name == "hr" || name == "img" || name == "br" {
            if name == "br" {
                push_node(&mut stack, Node::Text("\n".to_owned()));
            } else {
                push_node(
                    &mut stack,
                    Node::Element {
                        name,
                        href,
                        children: Vec::new(),
                    },
                );
            }
        } else if stack.len() > MAX_DEPTH {
            // Too deep: keep the text, drop the structure.
            continue;
        } else {
            stack.push((name, href, Vec::new()));
        }
    }
    while stack.len() > 1 {
        close(&mut stack);
    }
    stack.pop().map(|(_, _, nodes)| nodes).unwrap_or_default()
}

fn push_node(stack: &mut [(String, Option<String>, Vec<Node>)], node: Node) {
    if let Some((_, _, children)) = stack.last_mut() {
        children.push(node);
    }
}

fn close(stack: &mut Vec<(String, Option<String>, Vec<Node>)>) {
    if let Some((name, href, children)) = stack.pop() {
        push_node(
            stack,
            Node::Element {
                name,
                href,
                children,
            },
        );
    }
}

fn attribute(attributes: &str, wanted: &str) -> Option<String> {
    let mut rest = attributes;
    loop {
        rest = rest.trim_start();
        let eq = rest.find('=')?;
        let name = rest[..eq].trim().to_ascii_lowercase();
        let after = rest[eq + 1..].trim_start();
        let quote = after.chars().next()?;
        if quote != '"' && quote != '\'' {
            return None;
        }
        let close = after[1..].find(quote)? + 1;
        let value = &after[1..close];
        if name == wanted {
            return Some(decode_entities(value));
        }
        rest = &after[close + 1..];
    }
}

fn decode_entities(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        rest = &rest[start..];
        let decoded = rest.find(';').filter(|end| *end <= 10).and_then(|end| {
            let entity = &rest[1..end];
            let c = match entity {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some('\u{a0}'),
                _ => entity
                    .strip_prefix("#x")
                    .or_else(|| entity.strip_prefix("#X"))
                    .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                    .or_else(|| entity.strip_prefix('#').and_then(|dec| dec.parse().ok()))
                    .and_then(char::from_u32),
            }?;
            Some((c, end))
        });
        match decoded {
            Some((c, end)) => {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Blocks without their random ids, for literal comparison.
    fn shape(blocks: &[Value]) -> Value {
        Value::Array(
            blocks
                .iter()
                .map(|block| {
                    let mut block = block.clone();
                    block.as_object_mut().unwrap().remove("id");
                    let children = shape(block["children"].as_array().unwrap());
                    block["children"] = children;
                    block
                })
                .collect(),
        )
    }

    #[test]
    fn converts_the_asana_tag_set() {
        let blocks = html_to_blocks(
            "<body>Intro <strong>bold</strong> &amp; <a href=\"https://example.com/x\">link</a>\nSecond\
             <h1>Title</h1><ul><li>One<ol><li>Nested</li></ol></li><li><em>Two</em></li></ul>\
             <pre>let x = 1;\n</pre><blockquote>Quoted</blockquote><hr/>\
             <a href=\"javascript:alert(1)\">bad</a><img src=\"x\"/><unknown>kept</unknown></body>",
        );
        assert_eq!(
            shape(&blocks),
            json!([
                {"type": "paragraph", "props": {}, "content": [
                    {"type": "text", "text": "Intro ", "styles": {}},
                    {"type": "text", "text": "bold", "styles": {"bold": true}},
                    {"type": "text", "text": " & ", "styles": {}},
                    {"type": "link", "href": "https://example.com/x", "content": [
                        {"type": "text", "text": "link", "styles": {}}]}
                ], "children": []},
                {"type": "paragraph", "props": {}, "content": [
                    {"type": "text", "text": "Second", "styles": {}}], "children": []},
                {"type": "heading", "props": {"level": 1}, "content": [
                    {"type": "text", "text": "Title", "styles": {}}], "children": []},
                {"type": "bulletListItem", "props": {}, "content": [
                    {"type": "text", "text": "One", "styles": {}}], "children": [
                    {"type": "numberedListItem", "props": {}, "content": [
                        {"type": "text", "text": "Nested", "styles": {}}], "children": []}]},
                {"type": "bulletListItem", "props": {}, "content": [
                    {"type": "text", "text": "Two", "styles": {"italic": true}}], "children": []},
                {"type": "codeBlock", "props": {"language": "text"}, "content": [
                    {"type": "text", "text": "let x = 1;", "styles": {}}], "children": []},
                {"type": "quote", "props": {}, "content": [
                    {"type": "text", "text": "Quoted", "styles": {}}], "children": []},
                {"type": "divider", "props": {}, "children": []},
                {"type": "paragraph", "props": {}, "content": [
                    {"type": "text", "text": "bad", "styles": {}},
                    {"type": "text", "text": "kept", "styles": {}}], "children": []}
            ])
        );
    }

    #[test]
    fn deep_nesting_is_flattened_not_recursed() {
        let depth = 40_000;
        let html = format!(
            "<body>{}deep{}</body>",
            "<em>".repeat(depth),
            "</em>".repeat(depth)
        );
        let blocks = html_to_blocks(&html);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0]["content"][0]["text"], "deep");
    }

    #[test]
    fn empty_and_broken_input() {
        assert!(html_to_blocks("<body></body>").is_empty());
        assert!(html_to_blocks("").is_empty());
        let blocks = html_to_blocks("<body>a < b &bogus; <strong>open");
        assert_eq!(blocks[0]["content"][0]["text"], "a ");
    }
}
