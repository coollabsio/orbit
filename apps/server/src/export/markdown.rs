//! BlockNote JSON (a page's stored `content_json`) → GitHub-flavored Markdown.
//!
//! Deterministic: the same blocks and resolver answers always give the same text. Colors,
//! alignment and widths are dropped; toggle headings become headings and toggle lists list
//! items; a callout becomes a blockquote that starts with its emoji. Children of list items,
//! quotes and callouts nest inside them; children of other blocks follow them unindented
//! (Markdown has no nesting for paragraphs). Both BlockNote's full form and its shorthand
//! (string content, string table cells) are accepted.

use serde_json::{Map, Value};

/// Where a link to a page goes and what it is called.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PageLink {
    pub title: String,
    pub href: String,
}

/// Rewrites references while converting (links between exported pages, asset paths).
pub trait LinkResolver {
    /// The target of a `page` block, or `None` when the page is gone or not visible
    /// (rendered as "Missing page", like the editor does).
    fn page(&self, page_id: &str) -> Option<PageLink>;
    /// The destination for a link, image or file URL.
    fn url(&self, url: &str) -> String;
    /// The current name of a mentioned member; `None` falls back to the name stored in the
    /// mention (and to "Unknown user" without one).
    fn member_name(&self, _user_id: &str) -> Option<String> {
        None
    }
}

/// Keeps every URL as it is and links pages to `/docs/<id>` (titled "Untitled").
pub struct KeepLinks;

impl LinkResolver for KeepLinks {
    fn page(&self, page_id: &str) -> Option<PageLink> {
        (!page_id.is_empty()).then(|| PageLink {
            title: "Untitled".to_owned(),
            href: format!("/docs/{page_id}"),
        })
    }

    fn url(&self, url: &str) -> String {
        url.to_owned()
    }
}

/// Converts a block array; the result ends with one newline unless it is empty.
#[must_use]
pub fn blocks_to_markdown(blocks: &[Value], resolver: &dyn LinkResolver) -> String {
    let text = Converter { resolver }.blocks(blocks);
    if text.is_empty() { text } else { text + "\n" }
}

/// Escapes text for use inside a Markdown link label or image alt text.
#[must_use]
pub fn escape_label(text: &str) -> String {
    escape_text(text, false)
}

/// Makes a link destination safe to put between `(` and `)`: spaces, angle brackets and
/// parentheses are percent-encoded, as are control characters.
#[must_use]
pub fn link_destination(url: &str) -> String {
    let mut out = String::with_capacity(url.len());
    for character in url.chars() {
        match character {
            ' ' => out.push_str("%20"),
            '(' => out.push_str("%28"),
            ')' => out.push_str("%29"),
            '<' => out.push_str("%3C"),
            '>' => out.push_str("%3E"),
            character if character.is_control() => {
                let mut buffer = [0; 4];
                for byte in character.encode_utf8(&mut buffer).bytes() {
                    out.push_str(&format!("%{byte:02X}"));
                }
            }
            character => out.push(character),
        }
    }
    out
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Mode {
    /// Paragraph-like text: newlines are hard breaks.
    Block,
    /// Headings: newlines become spaces.
    Line,
    /// Table cells: newlines become `<br>`, pipes are escaped.
    Cell,
}

#[derive(Clone, Copy, Default, Eq, PartialEq)]
struct Styles {
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
    code: bool,
}

impl Styles {
    fn of(value: Option<&Value>) -> Self {
        let flag = |name: &str| {
            value
                .and_then(|styles| styles.get(name))
                .is_some_and(|flag| {
                    flag.as_bool().unwrap_or(false) || flag.as_str() == Some("true")
                })
        };
        Self {
            bold: flag("bold"),
            italic: flag("italic"),
            underline: flag("underline"),
            strike: flag("strike"),
            code: flag("code"),
        }
    }
}

/// One piece of inline content after merging: styled text or a link.
enum Inline {
    Text(String, Styles),
    Link(String, Vec<(String, Styles)>),
}

struct Converter<'a> {
    resolver: &'a dyn LinkResolver,
}

fn block_type(block: &Value) -> &str {
    block.get("type").and_then(Value::as_str).unwrap_or("")
}

fn props(block: &Value) -> Option<&Map<String, Value>> {
    block.get("props").and_then(Value::as_object)
}

fn prop_str<'v>(block: &'v Value, name: &str) -> &'v str {
    props(block)
        .and_then(|props| props.get(name))
        .and_then(Value::as_str)
        .unwrap_or("")
}

fn prop_bool(block: &Value, name: &str) -> bool {
    props(block)
        .and_then(|props| props.get(name))
        .is_some_and(|value| value.as_bool().unwrap_or(false) || value.as_str() == Some("true"))
}

fn prop_number(block: &Value, name: &str) -> Option<i64> {
    let value = props(block)?.get(name)?;
    value
        .as_i64()
        .or_else(|| value.as_f64().map(|number| number as i64))
        .or_else(|| value.as_str()?.trim().parse().ok())
}

fn children(block: &Value) -> &[Value] {
    block
        .get("children")
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

fn is_list(kind: &str) -> bool {
    matches!(
        kind,
        "bulletListItem" | "numberedListItem" | "checkListItem" | "toggleListItem"
    )
}

/// Prefixes the first line with `first` and every later non-empty line with `rest`.
fn indent(text: &str, first: &str, rest: &str) -> String {
    let mut out = String::with_capacity(text.len() + first.len());
    for (index, line) in text.split('\n').enumerate() {
        if index > 0 {
            out.push('\n');
            if !line.is_empty() {
                out.push_str(rest);
            }
        } else {
            out.push_str(first);
        }
        out.push_str(line);
    }
    out
}

/// Prefixes every line with `> ` (empty lines with `>`), keeping the quote together.
fn quote(text: &str) -> String {
    text.split('\n')
        .map(|line| {
            if line.is_empty() {
                ">".to_owned()
            } else {
                format!("> {line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Backslash-escapes Markdown punctuation. `_` is only escaped where it could start or end
/// emphasis (not inside words), `|` only in table cells.
fn escape_text(text: &str, cell: bool) -> String {
    let characters: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    for (index, &character) in characters.iter().enumerate() {
        let escape = match character {
            '\\' | '`' | '*' | '[' | ']' | '<' | '>' | '~' => true,
            '|' => cell,
            '_' => {
                let word =
                    |other: Option<&char>| other.is_some_and(|other| other.is_alphanumeric());
                !(word(
                    index
                        .checked_sub(1)
                        .and_then(|previous| characters.get(previous)),
                ) && word(characters.get(index + 1)))
            }
            _ => false,
        };
        if escape {
            out.push('\\');
        }
        out.push(character);
    }
    out
}

/// Escapes what would make a line start a heading, list, quote, setext underline or rule.
fn escape_line_start(line: &str) -> String {
    let trimmed = line.trim_start_matches(' ');
    let lead = &line[..line.len() - trimmed.len()];
    let first = trimmed.chars().next();
    match first {
        Some('#' | '>' | '=' | '+' | '-') => format!("{lead}\\{trimmed}"),
        Some(character) if character.is_ascii_digit() => {
            let digits = trimmed.bytes().take_while(u8::is_ascii_digit).count();
            match trimmed.as_bytes().get(digits) {
                Some(b'.' | b')') if digits <= 9 => {
                    format!("{lead}{}\\{}", &trimmed[..digits], &trimmed[digits..])
                }
                _ => line.to_owned(),
            }
        }
        _ => line.to_owned(),
    }
}

/// Inline code with a backtick fence longer than any run inside.
fn code_span(text: &str) -> String {
    let longest = longest_run(text, '`');
    let fence = "`".repeat(longest + 1);
    if text.starts_with('`')
        || text.ends_with('`')
        || (text.starts_with(' ') && text.ends_with(' ') && !text.trim().is_empty())
    {
        format!("{fence} {text} {fence}")
    } else {
        format!("{fence}{text}{fence}")
    }
}

fn longest_run(text: &str, target: char) -> usize {
    let mut longest = 0;
    let mut current = 0;
    for character in text.chars() {
        if character == target {
            current += 1;
            longest = longest.max(current);
        } else {
            current = 0;
        }
    }
    longest
}

/// Formats one styled run. Whitespace at the edges stays outside the markers (CommonMark does
/// not open or close emphasis next to it).
fn styled(text: &str, styles: Styles, mode: Mode) -> String {
    let core = text.trim_matches(|character: char| character == ' ' || character == '\t');
    if core.is_empty() {
        return text.to_owned();
    }
    let start = text.len() - text.trim_start_matches([' ', '\t']).len();
    let lead = &text[..start];
    let trail = &text[start + core.len()..];
    let mut body = if styles.code {
        code_span(core)
    } else {
        escape_text(core, mode == Mode::Cell)
    };
    if styles.underline {
        body = format!("<u>{body}</u>");
    }
    if styles.strike {
        body = format!("~~{body}~~");
    }
    if styles.italic {
        body = format!("*{body}*");
    }
    if styles.bold {
        body = format!("**{body}**");
    }
    format!("{lead}{body}{trail}")
}

/// Styled runs split at newlines, joined with the mode's line break.
fn runs_markdown(runs: &[(String, Styles)], mode: Mode) -> String {
    let mut out = String::new();
    for (text, styles) in runs {
        for (index, piece) in text.split('\n').enumerate() {
            if index > 0 {
                out.push_str(match mode {
                    Mode::Block => "\\\n",
                    Mode::Line => " ",
                    Mode::Cell => "<br>",
                });
            }
            out.push_str(&styled(piece, *styles, mode));
        }
    }
    out
}

/// Appends a run, merging it into the previous one when the styles match.
fn push_run(runs: &mut Vec<(String, Styles)>, text: &str, styles: Styles) {
    if text.is_empty() {
        return;
    }
    match runs.last_mut() {
        Some((previous, previous_styles)) if *previous_styles == styles => previous.push_str(text),
        _ => runs.push((text.to_owned(), styles)),
    }
}

/// The text runs of inline content (`content` of a link: a string or text items).
fn text_runs(value: Option<&Value>) -> Vec<(String, Styles)> {
    let mut runs = Vec::new();
    match value {
        Some(Value::String(text)) => push_run(&mut runs, text, Styles::default()),
        Some(Value::Array(items)) => {
            for item in items {
                match item {
                    Value::String(text) => push_run(&mut runs, text, Styles::default()),
                    item => {
                        if let Some(text) = item.get("text").and_then(Value::as_str) {
                            push_run(&mut runs, text, Styles::of(item.get("styles")));
                        } else {
                            runs.extend(text_runs(item.get("content")));
                        }
                    }
                }
            }
        }
        _ => {}
    }
    runs
}

/// Plain text of inline content (code blocks, labels).
fn plain_text(value: Option<&Value>) -> String {
    text_runs(value).into_iter().map(|(text, _)| text).collect()
}

impl Converter<'_> {
    fn blocks(&self, blocks: &[Value]) -> String {
        let mut out = String::new();
        let mut previous_list = false;
        let mut number = 0_i64;
        for block in blocks {
            let kind = block_type(block);
            if kind == "numberedListItem" {
                number = match prop_number(block, "start") {
                    Some(start) if number == 0 => start,
                    _ => number + 1,
                };
                if number == 0 {
                    number = 1;
                }
            } else {
                number = 0;
            }
            let text = self.block(block, number);
            if text.is_empty() {
                // An empty paragraph still ends the list before it.
                previous_list = false;
                continue;
            }
            let list = is_list(kind);
            if !out.is_empty() {
                out.push_str(if list && previous_list { "\n" } else { "\n\n" });
            }
            out.push_str(&text);
            previous_list = list;
        }
        out
    }

    fn block(&self, block: &Value, number: i64) -> String {
        let kind = block_type(block);
        let nested = || self.blocks(children(block));
        match kind {
            "heading" => {
                let level = prop_number(block, "level").unwrap_or(1).clamp(1, 6);
                let text = self.inline(block.get("content"), Mode::Line);
                let head = if text.trim().is_empty() {
                    String::new()
                } else {
                    format!("{} {}", "#".repeat(level as usize), text.trim())
                };
                join_blocks(&head, &nested())
            }
            "bulletListItem" | "toggleListItem" => self.list_item("- ", "", block),
            "checkListItem" => {
                let mark = if prop_bool(block, "checked") {
                    "[x] "
                } else {
                    "[ ] "
                };
                self.list_item("- ", mark, block)
            }
            "numberedListItem" => self.list_item(&format!("{number}. "), "", block),
            "quote" => {
                let body = join_blocks(&self.paragraph(block.get("content")), &nested());
                if body.is_empty() { body } else { quote(&body) }
            }
            "callout" => {
                let emoji = prop_str(block, "emoji").trim();
                let text = self.paragraph(block.get("content"));
                let head = match (emoji.is_empty(), text.is_empty()) {
                    (true, _) => text,
                    (false, true) => emoji.to_owned(),
                    (false, false) => format!("{emoji} {text}"),
                };
                let body = join_blocks(&head, &nested());
                if body.is_empty() { body } else { quote(&body) }
            }
            "codeBlock" => {
                let code = plain_text(block.get("content"));
                let language: String = prop_str(block, "language")
                    .chars()
                    .filter(|character| !character.is_whitespace() && *character != '`')
                    .collect();
                let language = if language == "text" {
                    String::new()
                } else {
                    language
                };
                let fence = "`".repeat((longest_run(&code, '`') + 1).max(3));
                let fenced = if code.is_empty() {
                    format!("{fence}{language}\n{fence}")
                } else {
                    format!("{fence}{language}\n{code}\n{fence}")
                };
                join_blocks(&fenced, &nested())
            }
            "divider" => join_blocks("---", &nested()),
            "table" => join_blocks(&self.table(block.get("content")), &nested()),
            "image" => {
                let url = prop_str(block, "url");
                let caption = prop_str(block, "caption").trim();
                let media = if url.is_empty() {
                    String::new()
                } else {
                    let name = prop_str(block, "name").trim();
                    let alt = if name.is_empty() { caption } else { name };
                    format!(
                        "![{}]({})",
                        escape_label(alt),
                        link_destination(&self.resolver.url(url))
                    )
                };
                join_blocks(
                    &join_blocks(&media, &self.caption(caption, &media)),
                    &nested(),
                )
            }
            "file" | "video" | "audio" => {
                let url = prop_str(block, "url");
                let caption = prop_str(block, "caption").trim();
                let link = if url.is_empty() {
                    String::new()
                } else {
                    let name = prop_str(block, "name").trim();
                    let label = if name.is_empty() {
                        url.rsplit('/')
                            .next()
                            .filter(|last| !last.is_empty())
                            .unwrap_or("File")
                    } else {
                        name
                    };
                    format!(
                        "[{}]({})",
                        escape_label(label),
                        link_destination(&self.resolver.url(url))
                    )
                };
                join_blocks(
                    &join_blocks(&link, &self.caption(caption, &link)),
                    &nested(),
                )
            }
            "page" => {
                let page_id = prop_str(block, "pageId");
                let link = match self.resolver.page(page_id) {
                    Some(link) => {
                        let title = if link.title.trim().is_empty() {
                            "Untitled"
                        } else {
                            link.title.trim()
                        };
                        format!(
                            "[{}]({})",
                            escape_label(title),
                            link_destination(&link.href)
                        )
                    }
                    None if page_id.is_empty() => String::new(),
                    None => "*Missing page*".to_owned(),
                };
                join_blocks(&link, &nested())
            }
            // Paragraphs and anything unknown: the text, then the children.
            _ => join_blocks(&self.paragraph(block.get("content")), &nested()),
        }
    }

    /// A caption under media; only when the media itself was written.
    fn caption(&self, caption: &str, media: &str) -> String {
        if caption.is_empty() || media.is_empty() {
            String::new()
        } else {
            format!("*{}*", escape_text(caption, false))
        }
    }

    fn list_item(&self, marker: &str, mark: &str, block: &Value) -> String {
        let text = self.inline(block.get("content"), Mode::Block);
        let head = format!("{mark}{text}");
        let head = if head.is_empty() {
            String::new()
        } else {
            escape_list_head(&head, !mark.is_empty())
        };
        let nested = self.blocks(children(block));
        let body = if nested.is_empty() {
            head
        } else if head.is_empty() {
            format!("\n{nested}")
        } else {
            format!("{head}\n{nested}")
        };
        indent(&body, marker, &" ".repeat(marker.len()))
            .trim_end()
            .to_owned()
    }

    fn paragraph(&self, content: Option<&Value>) -> String {
        let text = self.inline(content, Mode::Block);
        if text.trim().is_empty() {
            return String::new();
        }
        text.split('\n')
            .map(escape_line_start)
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn inline(&self, content: Option<&Value>, mode: Mode) -> String {
        let mut items = Vec::new();
        match content {
            Some(Value::String(text)) => items.push(Inline::Text(text.clone(), Styles::default())),
            Some(Value::Array(values)) => {
                for value in values {
                    match value {
                        Value::String(text) => {
                            items.push(Inline::Text(text.clone(), Styles::default()));
                        }
                        value if value.get("type").and_then(Value::as_str) == Some("mention") => {
                            items.push(Inline::Text(
                                format!("@{}", self.mention_name(value)),
                                Styles::default(),
                            ));
                        }
                        value if value.get("type").and_then(Value::as_str) == Some("link") => {
                            let href = value.get("href").and_then(Value::as_str).unwrap_or("");
                            items.push(Inline::Link(
                                href.to_owned(),
                                text_runs(value.get("content")),
                            ));
                        }
                        value => {
                            if let Some(text) = value.get("text").and_then(Value::as_str) {
                                items.push(Inline::Text(
                                    text.to_owned(),
                                    Styles::of(value.get("styles")),
                                ));
                            } else {
                                for (text, styles) in text_runs(value.get("content")) {
                                    items.push(Inline::Text(text, styles));
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        let mut out = String::new();
        let mut runs: Vec<(String, Styles)> = Vec::new();
        for item in items {
            match item {
                Inline::Text(text, styles) => push_run(&mut runs, &text, styles),
                Inline::Link(href, label) => {
                    out.push_str(&runs_markdown(&runs, mode));
                    runs.clear();
                    out.push_str(&self.link(&href, &label, mode));
                }
            }
        }
        out.push_str(&runs_markdown(&runs, mode));
        out
    }

    /// A mention's name: the member's current name, else the stored one, else "Unknown user".
    fn mention_name(&self, mention: &Value) -> String {
        let props = mention.get("props");
        let user_id = props
            .and_then(|props| props.get("userId"))
            .and_then(Value::as_str)
            .unwrap_or("");
        self.resolver
            .member_name(user_id)
            .or_else(|| {
                props
                    .and_then(|props| props.get("name"))
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| "Unknown user".to_owned())
    }

    fn link(&self, href: &str, label: &[(String, Styles)], mode: Mode) -> String {
        let label_text = runs_markdown(
            label,
            if mode == Mode::Cell {
                Mode::Cell
            } else {
                Mode::Line
            },
        );
        if href.trim().is_empty() {
            return label_text;
        }
        let destination = link_destination(&self.resolver.url(href));
        let label_text = if label_text.trim().is_empty() {
            escape_text(href, mode == Mode::Cell)
        } else {
            label_text
        };
        let destination = if mode == Mode::Cell {
            destination.replace('|', "%7C")
        } else {
            destination
        };
        format!("[{label_text}]({destination})")
    }

    fn table(&self, content: Option<&Value>) -> String {
        let Some(rows) = content
            .and_then(|content| content.get("rows"))
            .and_then(Value::as_array)
        else {
            return String::new();
        };
        let mut table: Vec<Vec<String>> = Vec::new();
        for row in rows {
            let mut cells = Vec::new();
            for cell in row
                .get("cells")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let (inline, span) = match cell {
                    Value::Object(object)
                        if object.get("type").and_then(Value::as_str) == Some("tableCell") =>
                    {
                        (
                            object.get("content"),
                            object
                                .get("props")
                                .and_then(|props| props.get("colspan"))
                                .and_then(Value::as_u64)
                                .unwrap_or(1)
                                .clamp(1, 64),
                        )
                    }
                    cell => (Some(cell), 1),
                };
                let text = self.inline(inline, Mode::Cell);
                cells.push(text.trim().to_owned());
                for _ in 1..span {
                    cells.push(String::new());
                }
            }
            table.push(cells);
        }
        let columns = table.iter().map(Vec::len).max().unwrap_or(0);
        if columns == 0 {
            return String::new();
        }
        let line = |cells: &[String]| {
            let mut parts: Vec<&str> = cells.iter().map(String::as_str).collect();
            parts.resize(columns, "");
            format!("| {} |", parts.join(" | "))
        };
        let mut lines = vec![line(&table[0])];
        lines.push(format!("|{}", " --- |".repeat(columns)));
        for row in &table[1..] {
            lines.push(line(row));
        }
        lines.join("\n")
    }
}

/// A list item's text must not read as a nested marker (the task box itself stays).
fn escape_list_head(head: &str, has_box: bool) -> String {
    head.split('\n')
        .enumerate()
        .map(|(index, line)| {
            if index == 0 && has_box {
                line.to_owned()
            } else {
                escape_line_start(line)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Joins a block's own text and its (unindented) children with a blank line.
fn join_blocks(head: &str, rest: &str) -> String {
    match (head.is_empty(), rest.is_empty()) {
        (true, _) => rest.to_owned(),
        (false, true) => head.to_owned(),
        (false, false) => format!("{head}\n\n{rest}"),
    }
}
