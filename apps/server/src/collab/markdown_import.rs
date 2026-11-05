//! Markdown -> BlockNote blocks, for text that exists only as markdown: task descriptions written
//! before the rich editor, REST writes that send markdown, and GitHub issue bodies.
//!
//! Paragraphs, headings, emphasis, strike, underline (`<u>`), inline code, links, bullet /
//! numbered / check lists (nested, with their start number), quotes (the first paragraph is the
//! quote, the rest its children), fenced code, GFM tables, images (an italic-only paragraph right
//! after an image is its caption) and rules map to their blocks: what `export::markdown` writes
//! reads back. `@Name` becomes a mention when exactly one member has that display name. Anything
//! else (HTML) stays as its text. The result has the shape `doc_to_blocks` returns; callers
//! sanitize it.

use std::collections::HashMap;
use std::ops::Range;

use orbit_platform::Id;
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use serde_json::{Map, Value, json};

use super::blocknote::{MAX_BLOCK_DEPTH, normalize_blocks};
use crate::export::markdown::Span;

/// Where the reader saw a block, as a byte range of the input; removed by the Yjs round trip.
const SOURCE: &str = "__source";

/// Converts markdown into BlockNote blocks; `members` are `(user id, display name)` pairs.
#[must_use]
pub fn markdown_to_blocks(markdown: &str, members: &[(Id, String)]) -> Vec<Value> {
    markdown_to_blocks_with_spans(markdown, members).0
}

/// [`markdown_to_blocks`] and the lines each block was read from (one [`Span`] per block,
/// trailing blank lines left out).
#[must_use]
pub fn markdown_to_blocks_with_spans(
    markdown: &str,
    members: &[(Id, String)],
) -> (Vec<Value>, Vec<Span>) {
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let (events, ranges) = Parser::new_ext(markdown, options)
        .into_offset_iter()
        .unzip();
    let mut reader = Reader {
        events,
        ranges,
        index: 0,
        depth: 0,
        members,
    };
    let mut blocks = reader.blocks(None);
    if blocks.is_empty() {
        return (blocks, Vec::new());
    }
    attach_captions(&mut blocks, 0);
    let mut sources = HashMap::new();
    collect_sources(&blocks, &mut sources);
    // A round trip through a Yjs document fills every prop with the schema default.
    let blocks = normalize_blocks(&blocks);
    let lines = Lines::new(markdown);
    let spans = spans_of(&blocks, &sources, &lines);
    (blocks, spans)
}

/// Line starts of the input, to turn byte ranges into line ranges.
struct Lines<'a> {
    text: Vec<&'a str>,
    starts: Vec<usize>,
}

impl<'a> Lines<'a> {
    fn new(markdown: &'a str) -> Self {
        let mut starts = vec![0];
        starts.extend(markdown.match_indices('\n').map(|(at, _)| at + 1));
        Self {
            text: markdown.split('\n').collect(),
            starts,
        }
    }

    fn line(&self, byte: usize) -> usize {
        self.starts
            .partition_point(|start| *start <= byte)
            .saturating_sub(1)
    }

    /// The lines of `bytes`, without blank lines at the end.
    fn range(&self, bytes: &Range<usize>) -> Range<usize> {
        if bytes.is_empty() {
            let at = self.line(bytes.start);
            return at..at;
        }
        let start = self.line(bytes.start);
        let mut end = self.line(bytes.end - 1) + 1;
        while end > start && self.blank(end - 1) {
            end -= 1;
        }
        start..end
    }

    /// Whether a line is empty but for quote markers and spaces.
    fn blank(&self, line: usize) -> bool {
        self.text.get(line).is_none_or(|line| {
            line.trim_matches(|c: char| c == '>' || c.is_whitespace())
                .is_empty()
        })
    }
}

/// The byte range each block (by id) was read from.
fn collect_sources(blocks: &[Value], out: &mut HashMap<String, Range<usize>>) {
    let mut stack: Vec<&Value> = blocks.iter().collect();
    while let Some(block) = stack.pop() {
        if let (Some(id), Some([start, end])) = (
            block["id"].as_str(),
            block[SOURCE].as_array().map(Vec::as_slice),
        ) {
            let (start, end) = (start.as_u64().unwrap_or(0), end.as_u64().unwrap_or(0));
            out.insert(id.to_owned(), start as usize..end as usize);
        }
        stack.extend(block["children"].as_array().into_iter().flatten());
    }
}

/// The spans of `blocks` (as read back from the Yjs document, at most [`MAX_BLOCK_DEPTH`] deep).
fn spans_of(blocks: &[Value], sources: &HashMap<String, Range<usize>>, lines: &Lines) -> Vec<Span> {
    blocks
        .iter()
        .map(|block| {
            let full = block["id"]
                .as_str()
                .and_then(|id| sources.get(id))
                .map_or(0..0, |bytes| lines.range(bytes));
            let children = spans_of(
                block["children"].as_array().map_or(&[], Vec::as_slice),
                sources,
                lines,
            );
            let head_end = children
                .iter()
                .map(|child| child.full.start)
                .filter(|start| *start > full.start)
                .min()
                .unwrap_or(full.end)
                .min(full.end);
            let mut head = full.start..head_end;
            while head.end > head.start && lines.blank(head.end - 1) {
                head.end -= 1;
            }
            Span {
                full,
                head,
                children,
            }
        })
        .collect()
}

/// An italic-only paragraph right after an image is the image's caption (as exported).
fn attach_captions(blocks: &mut Vec<Value>, depth: usize) {
    if depth > MAX_BLOCK_DEPTH + 1 {
        return;
    }
    let mut index = 0;
    while index < blocks.len() {
        if let Some(children) = blocks[index]["children"].as_array_mut() {
            attach_captions(children, depth + 1);
        }
        let caption = blocks
            .get(index + 1)
            .filter(|_| blocks[index]["type"] == "image")
            .and_then(italic_text);
        if let Some(caption) = caption {
            let next = blocks.remove(index + 1);
            let image = &mut blocks[index];
            image["props"]["caption"] = Value::String(caption);
            if let (Some(end), Some(source)) =
                (next[SOURCE].get(1).cloned(), image[SOURCE].as_array_mut())
                && source.len() == 2
            {
                source[1] = end;
            }
        }
        index += 1;
    }
}

/// The text of a paragraph whose whole content is italic text (and nothing else).
fn italic_text(block: &Value) -> Option<String> {
    if block["type"] != "paragraph" || block["children"].as_array().is_some_and(|c| !c.is_empty()) {
        return None;
    }
    let content = block["content"].as_array()?;
    let mut text = String::new();
    for item in content {
        let italic = item["type"] == "text"
            && item["styles"].as_object().is_some_and(|styles| {
                styles.len() == 1 && styles.get("italic") == Some(&json!(true))
            });
        if !italic {
            return None;
        }
        text.push_str(item["text"].as_str()?);
    }
    (!text.trim().is_empty()).then_some(text)
}

/// Inline content while reading: text and links, or an image that becomes its own block.
enum Inline {
    Content(Value),
    Image { url: String, alt: String },
}

struct Reader<'a, 'm> {
    events: Vec<Event<'a>>,
    /// The byte range of each event.
    ranges: Vec<Range<usize>>,
    index: usize,
    /// Blocks being read around the current one; see [`Reader::flat`].
    depth: usize,
    members: &'m [(Id, String)],
}

fn block(kind: &str, props: Value, content: Vec<Value>, children: Vec<Value>) -> Value {
    json!({
        "id": Id::new_v7().to_string(),
        "type": kind,
        "props": props,
        "content": content,
        "children": children,
    })
}

fn text(value: &str, styles: &Map<String, Value>) -> Value {
    json!({"type": "text", "text": value, "styles": styles})
}

fn same_end(tag: &TagEnd, end: Option<&TagEnd>) -> bool {
    end.is_some_and(|end| std::mem::discriminant(end) == std::mem::discriminant(tag))
}

impl<'a> Reader<'a, '_> {
    fn next(&mut self) -> Option<Event<'a>> {
        let event = self.events.get(self.index).cloned();
        self.index += 1;
        event
    }

    /// Blocks until `end` (or the end of the input).
    fn blocks(&mut self, end: Option<TagEnd>) -> Vec<Value> {
        let mut out = Vec::new();
        while let Some(event) = self.next() {
            match event {
                Event::End(tag) if same_end(&tag, end.as_ref()) => break,
                Event::Start(tag) => self.block(tag, &mut out),
                Event::Rule => {
                    let mut divider = block("divider", json!({}), Vec::new(), Vec::new());
                    divider[SOURCE] = self.source();
                    out.push(divider);
                }
                Event::Text(value) | Event::Html(value) | Event::InlineHtml(value) => {
                    let first = out.len();
                    push_paragraph(&mut out, value.trim_end_matches('\n'));
                    for paragraph in &mut out[first..] {
                        paragraph[SOURCE] = self.source();
                    }
                }
                _ => {}
            }
        }
        out
    }

    /// The byte range of the event just read.
    fn source(&self) -> Value {
        let range = self
            .index
            .checked_sub(1)
            .and_then(|index| self.ranges.get(index))
            .cloned()
            .unwrap_or(0..0);
        json!([range.start, range.end])
    }

    fn block(&mut self, tag: Tag<'a>, out: &mut Vec<Value>) {
        let source = self.source();
        let first = out.len();
        if self.depth >= MAX_BLOCK_DEPTH {
            self.flat(out);
        } else {
            self.depth += 1;
            self.read_block(tag, out);
            self.depth -= 1;
        }
        for block in &mut out[first..] {
            if block.get(SOURCE).is_none() {
                block[SOURCE] = source.clone();
            }
        }
    }

    /// A block nested deeper than [`MAX_BLOCK_DEPTH`] (markdown has no nesting limit), read
    /// without recursion: its text becomes plain paragraphs at this depth.
    fn flat(&mut self, out: &mut Vec<Value>) {
        let inline = |tag: &TagEnd| {
            matches!(
                tag,
                TagEnd::Emphasis
                    | TagEnd::Strong
                    | TagEnd::Strikethrough
                    | TagEnd::Link
                    | TagEnd::Image
            )
        };
        let mut open = 1_usize;
        let mut paragraph = String::new();
        while let Some(event) = self.next() {
            let tag = match event {
                Event::Start(tag) => {
                    open += 1;
                    tag.to_end()
                }
                Event::End(tag) => {
                    open -= 1;
                    tag
                }
                Event::Text(value)
                | Event::Code(value)
                | Event::Html(value)
                | Event::InlineHtml(value) => {
                    paragraph.push_str(&value);
                    continue;
                }
                Event::SoftBreak | Event::HardBreak => {
                    paragraph.push('\n');
                    continue;
                }
                _ => continue,
            };
            if !inline(&tag) {
                push_paragraph(out, std::mem::take(&mut paragraph).trim_end_matches('\n'));
            }
            if open == 0 {
                break;
            }
        }
        push_paragraph(out, paragraph.trim_end_matches('\n'));
    }

    fn read_block(&mut self, tag: Tag<'a>, out: &mut Vec<Value>) {
        match tag {
            Tag::Paragraph => {
                let inline = self.inline(TagEnd::Paragraph);
                self.push_inline("paragraph", json!({}), inline, out);
            }
            Tag::Heading { level, .. } => {
                let level = match level {
                    HeadingLevel::H1 => 1,
                    HeadingLevel::H2 => 2,
                    HeadingLevel::H3 => 3,
                    HeadingLevel::H4 => 4,
                    HeadingLevel::H5 => 5,
                    HeadingLevel::H6 => 6,
                };
                let inline = self.inline(TagEnd::Heading(HeadingLevel::H1));
                self.push_inline("heading", json!({"level": level}), inline, out);
            }
            Tag::BlockQuote(_) => {
                // As exported: the first paragraph is the quote's text, the rest its children.
                let mut inner = self.blocks(Some(TagEnd::BlockQuote(None)));
                let content = match inner.first() {
                    Some(first) if first["type"] == "paragraph" => content_of(&inner.remove(0)),
                    _ => Vec::new(),
                };
                out.push(block("quote", json!({}), content, inner));
            }
            Tag::CodeBlock(kind) => {
                let language = match kind {
                    CodeBlockKind::Fenced(info) => {
                        info.split_whitespace().next().unwrap_or("").to_owned()
                    }
                    CodeBlockKind::Indented => String::new(),
                };
                let mut code = String::new();
                while let Some(event) = self.next() {
                    match event {
                        Event::End(TagEnd::CodeBlock) => break,
                        Event::Text(value) => code.push_str(&value),
                        _ => {}
                    }
                }
                let code = code.strip_suffix('\n').unwrap_or(&code);
                let content = if code.is_empty() {
                    Vec::new()
                } else {
                    vec![text(code, &Map::new())]
                };
                out.push(block(
                    "codeBlock",
                    json!({"language": language}),
                    content,
                    Vec::new(),
                ));
            }
            Tag::List(start) => {
                let kind = if start.is_some() {
                    "numberedListItem"
                } else {
                    "bulletListItem"
                };
                let mut first = true;
                while let Some(event) = self.next() {
                    match event {
                        Event::End(TagEnd::List(_)) => break,
                        Event::Start(Tag::Item) => {
                            let mut item = self.item(kind);
                            // A list that does not start at 1 keeps its number on its first item.
                            if let Some(start) = start.filter(|start| *start != 1 && first)
                                && item["type"] == "numberedListItem"
                            {
                                item["props"]["start"] = json!(start);
                            }
                            first = false;
                            out.push(item);
                        }
                        _ => {}
                    }
                }
            }
            Tag::Table(_) => out.push(self.table()),
            Tag::HtmlBlock => {
                let mut html = String::new();
                while let Some(event) = self.next() {
                    match event {
                        Event::End(TagEnd::HtmlBlock) => break,
                        Event::Html(value) | Event::Text(value) => html.push_str(&value),
                        _ => {}
                    }
                }
                push_paragraph(out, html.trim_end_matches('\n'));
            }
            other => {
                // Unsupported containers (footnotes, definitions): keep their blocks.
                let end = other.to_end();
                out.extend(self.blocks(Some(end)));
            }
        }
    }

    /// One list item: its first text is the item's content, nested blocks are its children.
    fn item(&mut self, kind: &str) -> Value {
        let source = self.source();
        let mut checked = None;
        let mut inline = Vec::new();
        let mut children = Vec::new();
        let mut has_content = false;
        while let Some(event) = self.events.get(self.index).cloned() {
            match event {
                Event::End(TagEnd::Item) => {
                    self.index += 1;
                    break;
                }
                Event::TaskListMarker(done) => {
                    self.index += 1;
                    checked = Some(done);
                }
                Event::Start(Tag::Paragraph) if !has_content && children.is_empty() => {
                    self.index += 1;
                    // In a loose list the task box is inside the item's paragraph.
                    if let Some(Event::TaskListMarker(done)) = self.events.get(self.index) {
                        checked = Some(*done);
                        self.index += 1;
                    }
                    inline = self.inline(TagEnd::Paragraph);
                    has_content = true;
                }
                Event::Start(tag) if is_block(&tag) => {
                    self.index += 1;
                    self.block(tag, &mut children);
                }
                _ if !has_content && children.is_empty() => {
                    inline = self.inline(TagEnd::Item);
                    has_content = true;
                    // `inline` stops before the item's nested blocks or its end.
                }
                _ => self.index += 1,
            }
        }
        let (content, images) = self.split_images(inline);
        children.splice(0..0, images);
        let mut item = match checked {
            Some(done) => block("checkListItem", json!({"checked": done}), content, children),
            None => block(kind, json!({}), content, children),
        };
        item[SOURCE] = source;
        item
    }

    fn table(&mut self) -> Value {
        let mut rows: Vec<Value> = Vec::new();
        let mut cells: Vec<Value> = Vec::new();
        while let Some(event) = self.next() {
            match event {
                Event::End(TagEnd::Table) => break,
                Event::Start(Tag::TableCell) => {
                    let inline = self.inline(TagEnd::TableCell);
                    let (content, _) = self.split_images(inline);
                    cells.push(json!({"type": "tableCell", "props": {}, "content": content}));
                }
                Event::End(TagEnd::TableHead | TagEnd::TableRow) => {
                    rows.push(json!({"cells": std::mem::take(&mut cells)}));
                }
                _ => {}
            }
        }
        let columns = rows
            .iter()
            .map(|row| row["cells"].as_array().map_or(0, Vec::len))
            .max()
            .unwrap_or(0);
        let content = json!({
            "type": "tableContent",
            "columnWidths": vec![Value::Null; columns],
            "headerRows": 1,
            "rows": rows,
        });
        json!({
            "id": Id::new_v7().to_string(),
            "type": "table",
            "props": {},
            "content": content,
            "children": [],
        })
    }

    /// Inline content until `end`; for a list item it also stops before a nested block.
    fn inline(&mut self, end: TagEnd) -> Vec<Inline> {
        let mut out: Vec<Inline> = Vec::new();
        let mut styles = Map::new();
        let mut link: Option<(String, Vec<Value>)> = None;
        while let Some(event) = self.events.get(self.index).cloned() {
            if end == TagEnd::Item
                && let Event::Start(tag) = &event
                && is_block(tag)
            {
                break;
            }
            self.index += 1;
            let piece = match event {
                Event::End(tag) if same_end(&tag, Some(&end)) => {
                    if end == TagEnd::Item {
                        self.index -= 1;
                    }
                    break;
                }
                Event::InlineHtml(value) => match value.trim().to_ascii_lowercase().as_str() {
                    // What the exporter writes for underline and for line breaks in table cells.
                    "<u>" => toggle(&mut styles, "underline", true),
                    "</u>" => toggle(&mut styles, "underline", false),
                    "<br>" | "<br/>" | "<br />" => Some(text("\n", &styles)),
                    _ => Some(text(&value, &styles)),
                },
                Event::Text(value) | Event::Html(value) => Some(text(&value, &styles)),
                Event::Code(value) => {
                    let mut code = styles.clone();
                    code.insert("code".to_owned(), Value::Bool(true));
                    Some(text(&value, &code))
                }
                Event::SoftBreak | Event::HardBreak => Some(text("\n", &styles)),
                Event::Start(Tag::Emphasis) => toggle(&mut styles, "italic", true),
                Event::End(TagEnd::Emphasis) => toggle(&mut styles, "italic", false),
                Event::Start(Tag::Strong) => toggle(&mut styles, "bold", true),
                Event::End(TagEnd::Strong) => toggle(&mut styles, "bold", false),
                Event::Start(Tag::Strikethrough) => toggle(&mut styles, "strike", true),
                Event::End(TagEnd::Strikethrough) => toggle(&mut styles, "strike", false),
                Event::Start(Tag::Link { dest_url, .. }) => {
                    link = Some((dest_url.into_string(), Vec::new()));
                    None
                }
                Event::End(TagEnd::Link) => {
                    if let Some((href, content)) = link.take() {
                        out.push(Inline::Content(
                            json!({"type": "link", "href": href, "content": merge(content)}),
                        ));
                    }
                    None
                }
                Event::Start(Tag::Image { dest_url, .. }) => {
                    let mut alt = String::new();
                    while let Some(event) = self.next() {
                        match event {
                            Event::End(TagEnd::Image) => break,
                            Event::Text(value) | Event::Code(value) => alt.push_str(&value),
                            _ => {}
                        }
                    }
                    out.push(Inline::Image {
                        url: dest_url.into_string(),
                        alt,
                    });
                    None
                }
                _ => None,
            };
            if let Some(piece) = piece {
                match &mut link {
                    Some((_, content)) => content.push(piece),
                    None => out.push(Inline::Content(piece)),
                }
            }
        }
        out
    }

    /// Splits inline content into the block's content and image blocks found inside it.
    fn split_images(&self, inline: Vec<Inline>) -> (Vec<Value>, Vec<Value>) {
        let mut content = Vec::new();
        let mut images = Vec::new();
        for piece in inline {
            match piece {
                Inline::Content(value) => content.push(value),
                Inline::Image { url, alt } => images.push(image(url, alt)),
            }
        }
        (self.mentions(merge(content)), images)
    }

    /// A paragraph or heading; images inside it become image blocks around it, in order.
    fn push_inline(&self, kind: &str, props: Value, inline: Vec<Inline>, out: &mut Vec<Value>) {
        let mut run = Vec::new();
        let flush = |run: &mut Vec<Value>, out: &mut Vec<Value>| {
            let content = self.mentions(merge(std::mem::take(run)));
            if !content.is_empty() {
                out.push(block(kind, props.clone(), content, Vec::new()));
            }
        };
        for piece in inline {
            match piece {
                Inline::Content(value) => run.push(value),
                Inline::Image { url, alt } => {
                    flush(&mut run, out);
                    out.push(image(url, alt));
                }
            }
        }
        flush(&mut run, out);
    }

    /// Turns `@Name` in plain text into a mention when exactly one member has that name.
    fn mentions(&self, content: Vec<Value>) -> Vec<Value> {
        if self.members.is_empty() {
            return content;
        }
        let mut out = Vec::with_capacity(content.len());
        for item in content {
            let plain = item["type"] == "text" && item["styles"].get("code").is_none();
            let Some(value) = item["text"].as_str().filter(|_| plain) else {
                out.push(item);
                continue;
            };
            let styles = item["styles"].as_object().cloned().unwrap_or_default();
            let mut rest = value;
            let mut before = String::new();
            while let Some(at) = rest.find('@') {
                let boundary = before
                    .chars()
                    .chain(rest[..at].chars())
                    .last()
                    .is_none_or(|c| !c.is_alphanumeric());
                let after = &rest[at + 1..];
                match self.member(after).filter(|_| boundary) {
                    Some((id, name)) => {
                        before.push_str(&rest[..at]);
                        if !before.is_empty() {
                            out.push(text(&before, &styles));
                        }
                        before.clear();
                        out.push(json!({
                            "type": "mention",
                            "props": {"userId": id.to_string(), "name": name},
                        }));
                        rest = &after[name.len()..];
                    }
                    None => {
                        before.push_str(&rest[..=at]);
                        rest = after;
                    }
                }
            }
            before.push_str(rest);
            if !before.is_empty() {
                out.push(text(&before, &styles));
            }
        }
        out
    }

    /// The member whose name starts `text` (longest name first), unless two members share it.
    fn member(&self, text: &str) -> Option<(Id, &str)> {
        let fits = |name: &str| {
            !name.is_empty()
                && text.starts_with(name)
                && text[name.len()..]
                    .chars()
                    .next()
                    .is_none_or(|c| !c.is_alphanumeric())
        };
        let longest = self
            .members
            .iter()
            .filter(|(_, name)| fits(name))
            .map(|(_, name)| name.len())
            .max()?;
        let mut matches = self
            .members
            .iter()
            .filter(|(_, name)| name.len() == longest && fits(name));
        let (id, name) = matches.next()?;
        matches.next().is_none().then_some((*id, name.as_str()))
    }
}

fn is_block(tag: &Tag<'_>) -> bool {
    matches!(
        tag,
        Tag::Paragraph
            | Tag::Heading { .. }
            | Tag::BlockQuote(_)
            | Tag::CodeBlock(_)
            | Tag::List(_)
            | Tag::Table(_)
            | Tag::HtmlBlock
    )
}

fn toggle(styles: &mut Map<String, Value>, style: &str, on: bool) -> Option<Value> {
    if on {
        styles.insert(style.to_owned(), Value::Bool(true));
    } else {
        styles.remove(style);
    }
    None
}

/// An image; its alt text is its name (a caption is the paragraph after it, see
/// [`attach_captions`]).
fn image(url: String, alt: String) -> Value {
    block(
        "image",
        json!({"url": url, "name": alt}),
        Vec::new(),
        Vec::new(),
    )
}

fn content_of(block: &Value) -> Vec<Value> {
    block["content"].as_array().cloned().unwrap_or_default()
}

fn push_paragraph(out: &mut Vec<Value>, value: &str) {
    if !value.trim().is_empty() {
        out.push(block(
            "paragraph",
            json!({}),
            vec![text(value, &Map::new())],
            Vec::new(),
        ));
    }
}

/// Joins neighbouring text pieces with the same styles.
fn merge(content: Vec<Value>) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::with_capacity(content.len());
    for item in content {
        if let Some(last) = out.last_mut()
            && last["type"] == "text"
            && item["type"] == "text"
            && last["styles"] == item["styles"]
        {
            let joined = format!(
                "{}{}",
                last["text"].as_str().unwrap_or(""),
                item["text"].as_str().unwrap_or("")
            );
            last["text"] = Value::String(joined);
            continue;
        }
        out.push(item);
    }
    out
}
