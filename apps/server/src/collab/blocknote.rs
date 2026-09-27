//! BlockNote JSON <-> Yjs (`Y.XmlFragment` in y-prosemirror's layout) for Orbit's page editor.
//!
//! Ports of BlockNote 0.55's `blockToNode` + y-prosemirror's `prosemirrorToYXmlFragment` (JSON -> Y)
//! and of `nodeToBlock` (Y -> JSON). Layout:
//! `blockGroup > blockContainer{id} > <type>{props} [> blockGroup > ...]`; inline text is one
//! `XmlText` per run of text nodes with marks as formatting attributes (boolean styles `{}`,
//! string styles `{stringValue}`, `link {href}`), hard breaks are `hardBreak` elements, tables are
//! `table > tableRow > tableHeader|tableCell{attrs} > tableParagraph`, code blocks hold plain text.
//! Custom inline content without content (`mention {userId, name}`) is an element between the
//! text runs, its props as attributes.
//!
//! Schema facts (attribute order and defaults, style kinds) come from `blocknote-schema.json`,
//! generated from the real editor schema by the web test `collabParity.test.ts`, never by hand.
//! Parity goldens (`tests/fixtures/collab`) pin both directions against BlockNote itself.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

use serde::Deserialize;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use yrs::types::text::YChange;
use yrs::{
    Any, Doc, OffsetKind, Options, Out, ReadTxn, StateVector, Text, Transact, TransactionMut, Xml,
    XmlElementPrelim, XmlElementRef, XmlFragment, XmlFragmentRef, XmlOut, XmlTextPrelim,
    XmlTextRef,
};

/// The Y.Doc root BlockNote binds the editor to (`withCollaboration({ fragment })`).
pub const FRAGMENT: &str = "prosemirror";

/// Bump when the conversion code changes in a way that should rebuild stored documents; the
/// schema table's content is hashed in as well (see [`converter_version`]).
const CONVERTER_REVISION: u32 = 1;

const SCHEMA_JSON: &str = include_str!("blocknote-schema.json");

static SCHEMA: LazyLock<SchemaTable> = LazyLock::new(|| {
    serde_json::from_str(SCHEMA_JSON).expect("blocknote-schema.json is valid (checked by tests)")
});

static CONVERTER_VERSION: LazyLock<i64> = LazyLock::new(|| {
    let canonical: Value = serde_json::from_str(SCHEMA_JSON).unwrap_or(Value::Null);
    let mut hasher = Sha256::new();
    hasher.update(CONVERTER_REVISION.to_be_bytes());
    hasher.update(canonical.to_string().as_bytes());
    let digest = hasher.finalize();
    let mut bytes = [0_u8; 8];
    bytes.copy_from_slice(&digest[..8]);
    i64::from_be_bytes(bytes) & i64::MAX
});

#[derive(Debug, Deserialize)]
pub struct SchemaTable {
    pub blocks: HashMap<String, BlockTable>,
    /// Custom inline content types (`mention`), by type.
    #[serde(default)]
    pub inline: HashMap<String, InlineTable>,
    pub cell: CellTable,
    pub styles: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
pub struct BlockTable {
    /// `inline`, `none`, `table` or `plain`.
    pub content: String,
    pub attrs: Vec<AttrTable>,
}

#[derive(Debug, Deserialize)]
pub struct InlineTable {
    /// `none` (an atom such as a mention); other kinds are not used by the editor.
    pub content: String,
    pub attrs: Vec<AttrTable>,
}

#[derive(Debug, Deserialize)]
pub struct AttrTable {
    pub name: String,
    pub prop: bool,
    pub missing: Missing,
}

/// The value ProseMirror uses when the Y element has no such attribute.
#[derive(Debug, Deserialize)]
pub struct Missing {
    #[serde(default)]
    pub undefined: bool,
    #[serde(default)]
    pub value: Value,
}

#[derive(Debug, Deserialize)]
pub struct CellTable {
    pub attrs: Vec<String>,
    pub defaults: Map<String, Value>,
}

/// The editor schema the converter follows.
#[must_use]
pub fn schema() -> &'static SchemaTable {
    &SCHEMA
}

/// Identifies the converter + schema that built a stored document; a stored document with another
/// version is rebuilt from `pages.content_json` when its room loads.
#[must_use]
pub fn converter_version() -> i64 {
    *CONVERTER_VERSION
}

/// A document configured like the browser's (offsets in UTF-16 code units).
#[must_use]
pub fn new_doc(client_id: Option<u64>) -> Doc {
    let mut options = Options {
        offset_kind: OffsetKind::Utf16,
        ..Options::default()
    };
    if let Some(client_id) = client_id {
        options.client_id = yrs::ClientID::new(client_id);
    }
    Doc::with_options(options)
}

fn any(value: &Value) -> Any {
    serde_json::from_value(value.clone()).unwrap_or(Any::Null)
}

/// JSON of a Y value; integral numbers become JSON integers (JavaScript has one number type, and
/// `JSON.stringify(120)` is `120`, not `120.0`).
fn json(any: &Any) -> Value {
    integral(serde_json::to_value(any).unwrap_or(Value::Null))
}

fn integral(value: Value) -> Value {
    match value {
        Value::Number(number) => match number.as_f64() {
            Some(float)
                if number.is_f64()
                    && float.fract() == 0.0
                    && float.abs() < 9_007_199_254_740_992.0 =>
            {
                Value::from(float as i64)
            }
            _ => Value::Number(number),
        },
        Value::Array(items) => Value::Array(items.into_iter().map(integral).collect()),
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(key, value)| (key, integral(value)))
                .collect(),
        ),
        other => other,
    }
}

// ---------------------------------------------------------------------------------------------
// JSON -> Y
// ---------------------------------------------------------------------------------------------

/// Builds a fresh document from blocks and returns it as one v1 update (tests, parity goldens).
#[must_use]
pub fn blocks_to_update(blocks: &[Value], client_id: Option<u64>) -> Vec<u8> {
    let doc = new_doc(client_id);
    let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
    {
        let mut txn = doc.transact_mut();
        write_blocks(&mut txn, &fragment, blocks);
    }
    doc.transact()
        .encode_state_as_update_v1(&StateVector::default())
}

/// Replaces the whole document inside `txn` (restore, import, API content writes on a live
/// document): connected editors receive it as an ordinary update.
pub fn replace_blocks(txn: &mut TransactionMut, fragment: &XmlFragmentRef, blocks: &[Value]) {
    let len = fragment.len(txn);
    if len > 0 {
        fragment.remove_range(txn, 0, len);
    }
    write_blocks(txn, fragment, blocks);
}

/// Appends the document to an empty fragment. Unknown block types are skipped; an empty document
/// becomes one empty paragraph (an editor must never bind to an empty fragment).
pub fn write_blocks(txn: &mut TransactionMut, fragment: &XmlFragmentRef, blocks: &[Value]) {
    let group = fragment.push_back(txn, XmlElementPrelim::empty("blockGroup"));
    let known: Vec<&Value> = blocks.iter().filter(|block| known_type(block)).collect();
    if known.is_empty() {
        write_container(txn, &group, &json!({ "type": "paragraph" }));
    }
    for block in known {
        write_container(txn, &group, block);
    }
}

fn known_type(block: &Value) -> bool {
    block
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|block_type| SCHEMA.blocks.contains_key(block_type))
}

fn new_block_id() -> String {
    orbit_platform::Id::new_v7().to_string()
}

fn write_container(txn: &mut TransactionMut, parent: &XmlElementRef, block: &Value) {
    let block_type = block
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("paragraph");
    let Some(table) = SCHEMA.blocks.get(block_type) else {
        return;
    };
    let container = parent.push_back(txn, XmlElementPrelim::empty("blockContainer"));
    let id = block
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map_or_else(new_block_id, str::to_owned);
    container.insert_attribute(txn, "id", Any::from(id));
    let content_node = container.push_back(txn, XmlElementPrelim::empty(block_type));
    let props = block.get("props").and_then(Value::as_object);
    for attr in &table.attrs {
        // ProseMirror fills unspecified attrs with their defaults; y-prosemirror writes every
        // attribute that is not null.
        let value = props
            .and_then(|props| props.get(&attr.name))
            .cloned()
            .unwrap_or_else(|| attr.missing.value.clone());
        if !value.is_null() {
            content_node.insert_attribute(txn, attr.name.as_str(), any(&value));
        }
    }
    match table.content.as_str() {
        "inline" => {
            let runs = inline_runs(block.get("content"), false);
            write_runs(txn, &content_node, &runs);
        }
        "plain" => {
            let runs = inline_runs(block.get("content"), true);
            write_runs(txn, &content_node, &runs);
        }
        "table" => write_table(txn, &content_node, block.get("content")),
        _ => {}
    }
    let children: Vec<&Value> = block
        .get("children")
        .and_then(Value::as_array)
        .map(|children| children.iter().filter(|child| known_type(child)).collect())
        .unwrap_or_default();
    if !children.is_empty() {
        let group = container.push_back(txn, XmlElementPrelim::empty("blockGroup"));
        for child in children {
            write_container(txn, &group, child);
        }
    }
}

/// A ProseMirror inline node: text with marks, a hard break, or custom inline content (an atom
/// element with its attributes, e.g. a mention).
enum Run {
    Text(String, yrs::types::Attrs),
    Break,
    Inline(String, Vec<(String, Any)>),
}

/// A custom inline content item (`{ type: "mention", props }`) as an element run; `None` for other
/// items. PM fills unspecified attrs with their defaults, y-prosemirror skips null ones.
fn inline_node_run(object: &Map<String, Value>) -> Option<Run> {
    let inline_type = object.get("type").and_then(Value::as_str)?;
    let table = SCHEMA.inline.get(inline_type)?;
    if table.content != "none" {
        return None;
    }
    let props = object.get("props").and_then(Value::as_object);
    let attrs = table
        .attrs
        .iter()
        .filter_map(|attr| {
            let value = props
                .and_then(|props| props.get(&attr.name))
                .cloned()
                .unwrap_or_else(|| attr.missing.value.clone());
            (!value.is_null()).then(|| (attr.name.clone(), any(&value)))
        })
        .collect();
    Some(Run::Inline(inline_type.to_owned(), attrs))
}

fn marks_of(styles: Option<&Value>, link: Option<&str>, plain: bool) -> yrs::types::Attrs {
    let mut attrs = yrs::types::Attrs::new();
    if plain {
        // Code blocks allow no marks.
        return attrs;
    }
    if let Some(styles) = styles.and_then(Value::as_object) {
        for (name, value) in styles {
            match SCHEMA.styles.get(name).map(String::as_str) {
                Some("boolean") if value.as_bool() == Some(true) => {
                    attrs.insert(Arc::from(name.as_str()), Any::Map(Arc::new(HashMap::new())));
                }
                Some("string") if value.as_str().is_some_and(|value| !value.is_empty()) => {
                    let mut map = HashMap::new();
                    map.insert("stringValue".to_owned(), any(value));
                    attrs.insert(Arc::from(name.as_str()), Any::Map(Arc::new(map)));
                }
                _ => {}
            }
        }
    }
    if let Some(href) = link {
        let mut map = HashMap::new();
        map.insert("href".to_owned(), Any::from(href.to_owned()));
        attrs.insert(Arc::from("link"), Any::Map(Arc::new(map)));
    }
    attrs
}

/// Text becomes text and hard breaks (`\n`), except in plain blocks, which keep newlines as text.
fn push_text(runs: &mut Vec<Run>, text: &str, attrs: yrs::types::Attrs, plain: bool) {
    if plain {
        if !text.is_empty() {
            runs.push(Run::Text(text.to_owned(), attrs));
        }
        return;
    }
    for (index, part) in text.split('\n').enumerate() {
        if index > 0 {
            runs.push(Run::Break);
        }
        if !part.is_empty() {
            runs.push(Run::Text(part.to_owned(), attrs.clone()));
        }
    }
}

fn inline_runs(content: Option<&Value>, plain: bool) -> Vec<Run> {
    let mut runs = Vec::new();
    match content {
        Some(Value::String(text)) => push_text(&mut runs, text, yrs::types::Attrs::new(), plain),
        Some(Value::Array(items)) => {
            for item in items {
                match item {
                    Value::String(text) => {
                        push_text(&mut runs, text, yrs::types::Attrs::new(), plain);
                    }
                    Value::Object(object)
                        if object.get("type").and_then(Value::as_str) == Some("link") =>
                    {
                        let href = object.get("href").and_then(Value::as_str).unwrap_or("");
                        let link = (!plain).then_some(href);
                        match object.get("content") {
                            Some(Value::String(text)) => {
                                push_text(&mut runs, text, marks_of(None, link, plain), plain);
                            }
                            Some(Value::Array(parts)) => {
                                for part in parts {
                                    let text = part
                                        .get("text")
                                        .and_then(Value::as_str)
                                        .or(part.as_str())
                                        .unwrap_or("");
                                    let marks = marks_of(part.get("styles"), link, plain);
                                    push_text(&mut runs, text, marks, plain);
                                }
                            }
                            _ => {}
                        }
                    }
                    Value::Object(object) if object.contains_key("text") || plain => {
                        let text = object.get("text").and_then(Value::as_str).unwrap_or("");
                        let marks = marks_of(object.get("styles"), None, plain);
                        push_text(&mut runs, text, marks, plain);
                    }
                    Value::Object(object) => {
                        if let Some(run) = inline_node_run(object) {
                            runs.push(run);
                        }
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
    runs
}

/// y-prosemirror: consecutive text nodes share one `Y.XmlText` (formatting attributes = marks);
/// other inline nodes (hard breaks) are elements between them.
fn write_runs(txn: &mut TransactionMut, parent: &XmlElementRef, runs: &[Run]) {
    let mut current: Option<XmlTextRef> = None;
    for run in runs {
        match run {
            Run::Break => {
                current = None;
                parent.push_back(txn, XmlElementPrelim::empty("hardBreak"));
            }
            Run::Inline(inline_type, attrs) => {
                current = None;
                let element = parent.push_back(txn, XmlElementPrelim::empty(inline_type.as_str()));
                for (name, value) in attrs {
                    element.insert_attribute(txn, name.as_str(), value.clone());
                }
            }
            Run::Text(text, attrs) => {
                let node =
                    current.get_or_insert_with(|| parent.push_back(txn, XmlTextPrelim::new("")));
                let end = node.len(txn);
                node.insert_with_attributes(txn, end, text, attrs.clone());
            }
        }
    }
}

fn span(cell: &Value, name: &str) -> usize {
    cell.get("props")
        .and_then(|props| props.get(name))
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|value| *value >= 1)
        .unwrap_or(1)
}

fn is_table_cell(cell: &Value) -> bool {
    cell.get("type").and_then(Value::as_str) == Some("tableCell")
}

/// Port of BlockNote's `getTableCellOccupancyGrid` + `getAbsoluteTableCells`: the absolute column
/// of every cell, accounting for row and column spans. `None` for a malformed table.
fn absolute_columns(rows: &[Value]) -> Option<Vec<Vec<usize>>> {
    let cells = |row: &Value| -> Vec<Value> {
        row.get("cells")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    };
    let height = rows.len();
    let width = rows
        .iter()
        .map(|row| {
            cells(row)
                .iter()
                .map(|cell| {
                    if is_table_cell(cell) {
                        span(cell, "colspan")
                    } else {
                        1
                    }
                })
                .sum::<usize>()
        })
        .max()
        .unwrap_or(0);
    let mut grid = vec![vec![false; width]; height];
    let mut columns = Vec::with_capacity(height);
    for (row_index, row) in rows.iter().enumerate() {
        let mut row_columns = Vec::new();
        for (col_index, cell) in cells(row).iter().enumerate() {
            let (rowspan, colspan) = if is_table_cell(cell) {
                (span(cell, "rowspan"), span(cell, "colspan"))
            } else {
                (1, 1)
            };
            let (start_row, start_col) = (row_index..height)
                .flat_map(|i| (col_index..width).map(move |j| (i, j)))
                .find(|(i, j)| !grid[*i][*j])?;
            for occupied_row in grid.iter_mut().skip(start_row).take(rowspan) {
                for slot in occupied_row.iter_mut().skip(start_col).take(colspan) {
                    if *slot {
                        return None;
                    }
                    *slot = true;
                }
            }
            if start_row + rowspan > height || start_col + colspan > width {
                return None;
            }
            // BlockNote resolves a cell's position by its relative indices; only the column matters.
            row_columns.push(start_col);
        }
        columns.push(row_columns);
    }
    Some(columns)
}

fn write_table(txn: &mut TransactionMut, table: &XmlElementRef, content: Option<&Value>) {
    let Some(content) = content.and_then(Value::as_object) else {
        return;
    };
    let header_rows = content
        .get("headerRows")
        .and_then(Value::as_u64)
        .unwrap_or(0) as usize;
    let header_cols = content
        .get("headerCols")
        .and_then(Value::as_u64)
        .unwrap_or(0) as usize;
    let widths: Vec<Value> = content
        .get("columnWidths")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let rows: Vec<Value> = content
        .get("rows")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let absolute = absolute_columns(&rows);
    // A width is used when it is a truthy number (JS `columnWidths[i] ? [w] : null`).
    let width_at = |index: usize| {
        widths
            .get(index)
            .filter(|width| width.as_f64().is_some_and(|width| width != 0.0))
            .cloned()
    };
    for (r, row) in rows.iter().enumerate() {
        let row_el = table.push_back(txn, XmlElementPrelim::empty("tableRow"));
        let cells: Vec<Value> = row
            .get("cells")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for (c, cell) in cells.iter().enumerate() {
            let tag = if r < header_rows || c < header_cols {
                "tableHeader"
            } else {
                "tableCell"
            };
            let cell_el = row_el.push_back(txn, XmlElementPrelim::empty(tag));
            let column = absolute
                .as_ref()
                .and_then(|columns| columns.get(r))
                .and_then(|row| row.get(c))
                .copied()
                .unwrap_or(c);
            let props = if is_table_cell(cell) {
                cell.get("props").and_then(Value::as_object).cloned()
            } else {
                None
            };
            let colspan = if is_table_cell(cell) {
                span(cell, "colspan")
            } else {
                1
            };
            let colwidth = if colspan > 1 {
                Value::Array(
                    (0..colspan)
                        .map(|offset| widths.get(column + offset).cloned().unwrap_or(Value::Null))
                        .collect(),
                )
            } else {
                width_at(column).map_or(Value::Null, |width| json!([width]))
            };
            for name in &SCHEMA.cell.attrs {
                let value = if name == "colwidth" {
                    colwidth.clone()
                } else {
                    props
                        .as_ref()
                        .and_then(|props| props.get(name))
                        .cloned()
                        .unwrap_or_else(|| {
                            SCHEMA
                                .cell
                                .defaults
                                .get(name)
                                .cloned()
                                .unwrap_or(Value::Null)
                        })
                };
                if !value.is_null() {
                    cell_el.insert_attribute(txn, name.as_str(), any(&value));
                }
            }
            let paragraph = cell_el.push_back(txn, XmlElementPrelim::empty("tableParagraph"));
            let runs = match cell {
                // A string cell is one text node; its newlines are not hard breaks.
                Value::String(text) if !text.is_empty() => {
                    vec![Run::Text(text.clone(), yrs::types::Attrs::new())]
                }
                Value::String(_) | Value::Null => Vec::new(),
                Value::Object(_) if is_table_cell(cell) => inline_runs(cell.get("content"), false),
                other => inline_runs(Some(other), false),
            };
            write_runs(txn, &paragraph, &runs);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Y -> JSON
// ---------------------------------------------------------------------------------------------

/// The document as BlockNote blocks (what `yDocToBlocks` returns). Never panics on foreign
/// structure: unknown elements are skipped.
pub fn fragment_to_blocks<T: ReadTxn>(txn: &T, fragment: &XmlFragmentRef) -> Vec<Value> {
    let Some(XmlOut::Element(group)) = fragment.get(txn, 0) else {
        return Vec::new();
    };
    group
        .children(txn)
        .filter_map(|node| match node {
            XmlOut::Element(element) if element.tag().as_ref() == "blockContainer" => {
                container_to_block(txn, &element)
            }
            _ => None,
        })
        .collect()
}

/// [`fragment_to_blocks`] of a whole document.
#[must_use]
pub fn doc_to_blocks(doc: &Doc) -> Vec<Value> {
    let txn = doc.transact();
    match txn.get_xml_fragment(FRAGMENT) {
        Some(fragment) => fragment_to_blocks(&txn, &fragment),
        None => Vec::new(),
    }
}

fn attr<T: ReadTxn>(txn: &T, element: &XmlElementRef, name: &str) -> Option<Value> {
    match element.get_attribute(txn, name)? {
        // y-prosemirror stores `undefined` attrs (e.g. numberedListItem.start) as Any::Undefined.
        Out::Any(Any::Undefined) => None,
        Out::Any(value) => Some(json(&value)),
        other => Some(Value::String(other.to_string(txn))),
    }
}

fn container_to_block<T: ReadTxn>(txn: &T, container: &XmlElementRef) -> Option<Value> {
    let id = attr(txn, container, "id").unwrap_or_else(|| Value::String(new_block_id()));
    let mut children_el = container.children(txn);
    let Some(XmlOut::Element(content_node)) = children_el.next() else {
        return None;
    };
    let group = children_el.next();
    let block_type = content_node.tag().to_string();
    let table = SCHEMA.blocks.get(&block_type)?;
    let mut props = Map::new();
    for attr_table in table.attrs.iter().filter(|attr| attr.prop) {
        match attr(txn, &content_node, &attr_table.name) {
            Some(value) => {
                props.insert(attr_table.name.clone(), value);
            }
            None if !attr_table.missing.undefined => {
                props.insert(attr_table.name.clone(), attr_table.missing.value.clone());
            }
            None => {}
        }
    }
    let children: Vec<Value> = match group {
        Some(XmlOut::Element(group)) if group.tag().as_ref() == "blockGroup" => group
            .children(txn)
            .filter_map(|node| match node {
                XmlOut::Element(element) if element.tag().as_ref() == "blockContainer" => {
                    container_to_block(txn, &element)
                }
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    };
    let mut block = Map::new();
    block.insert("id".into(), id);
    block.insert("type".into(), Value::String(block_type));
    block.insert("props".into(), Value::Object(props));
    match table.content.as_str() {
        "inline" => {
            block.insert(
                "content".into(),
                Value::Array(inline_content(txn, &content_node)),
            );
        }
        "plain" => {
            let text = text_content(txn, &content_node);
            block.insert(
                "content".into(),
                if text.is_empty() {
                    json!([])
                } else {
                    json!([{ "type": "text", "text": text, "styles": {} }])
                },
            );
        }
        "table" => {
            block.insert("content".into(), table_content(txn, &content_node));
        }
        _ => {}
    }
    block.insert("children".into(), Value::Array(children));
    Some(Value::Object(block))
}

fn text_content<T: ReadTxn>(txn: &T, element: &XmlElementRef) -> String {
    let mut out = String::new();
    for child in element.children(txn) {
        match child {
            XmlOut::Text(text) => {
                for chunk in text.diff(txn, YChange::identity) {
                    if let Out::Any(Any::String(value)) = chunk.insert {
                        out.push_str(&value);
                    }
                }
            }
            XmlOut::Element(element) => out.push_str(&text_content(txn, &element)),
            XmlOut::Fragment(_) => {}
        }
    }
    out
}

fn append_text(node: &mut Value, suffix: &str) {
    let text = node["text"].as_str().unwrap_or("").to_owned() + suffix;
    node["text"] = Value::String(text);
}

/// The last text run of a link, or the text node itself.
fn last_run(node: &mut Value) -> Option<&mut Value> {
    if node["type"] == "link" {
        node["content"].as_array_mut()?.last_mut()
    } else {
        Some(node)
    }
}

/// Port of `nodeToCustomInlineContent` for atoms: `{ type, props }` (props from the attributes,
/// PM defaults for missing ones; `content` is undefined and therefore absent).
fn inline_node<T: ReadTxn>(txn: &T, element: &XmlElementRef, table: &InlineTable) -> Value {
    let mut props = Map::new();
    for attr_table in table.attrs.iter().filter(|attr| attr.prop) {
        match attr(txn, element, &attr_table.name) {
            Some(value) => {
                props.insert(attr_table.name.clone(), value);
            }
            None if !attr_table.missing.undefined => {
                props.insert(attr_table.name.clone(), attr_table.missing.value.clone());
            }
            None => {}
        }
    }
    json!({ "type": element.tag().as_ref(), "props": Value::Object(props) })
}

/// Port of `contentNodeToInlineContent`: merges runs with equal styles, groups link runs, and
/// turns hard breaks into `\n` on the previous run. Custom inline content (mentions) ends the
/// current run and is its own item.
fn inline_content<T: ReadTxn>(txn: &T, element: &XmlElementRef) -> Vec<Value> {
    let mut content: Vec<Value> = Vec::new();
    let mut current: Option<Value> = None;
    for child in element.children(txn) {
        match child {
            XmlOut::Element(element)
                if SCHEMA
                    .inline
                    .get(element.tag().as_ref())
                    .is_some_and(|table| table.content == "none") =>
            {
                if let Some(node) = current.take() {
                    content.push(node);
                }
                let table = &SCHEMA.inline[element.tag().as_ref()];
                content.push(inline_node(txn, &element, table));
            }
            XmlOut::Element(element) if element.tag().as_ref() == "hardBreak" => {
                match current.as_mut().and_then(last_run) {
                    Some(run) => append_text(run, "\n"),
                    None => {
                        current = Some(json!({ "type": "text", "text": "\n", "styles": {} }));
                    }
                }
            }
            XmlOut::Text(text) => {
                for chunk in text.diff(txn, YChange::identity) {
                    let Out::Any(Any::String(value)) = chunk.insert else {
                        continue;
                    };
                    let (styles, href) = styles_of(chunk.attributes.as_deref());
                    current = Some(merge_run(
                        &mut content,
                        current.take(),
                        &value,
                        styles,
                        href,
                    ));
                }
            }
            _ => {}
        }
    }
    if let Some(node) = current {
        content.push(node);
    }
    content
}

fn styles_of(attributes: Option<&yrs::types::Attrs>) -> (Value, Option<String>) {
    let mut styles = Map::new();
    let mut href = None;
    for (name, value) in attributes.into_iter().flatten() {
        if name.as_ref() == "link" {
            href = match value {
                Any::Map(map) => map.get("href").map(|href| match href {
                    Any::String(href) => href.to_string(),
                    other => json(other).as_str().unwrap_or("").to_owned(),
                }),
                _ => None,
            };
            continue;
        }
        match SCHEMA.styles.get(name.as_ref()).map(String::as_str) {
            Some("boolean") => {
                styles.insert(name.to_string(), Value::Bool(true));
            }
            Some("string") => {
                let value = match value {
                    Any::Map(map) => map.get("stringValue").map_or(Value::Null, json),
                    _ => Value::Null,
                };
                styles.insert(name.to_string(), value);
            }
            _ => {}
        }
    }
    (Value::Object(styles), href)
}

fn merge_run(
    content: &mut Vec<Value>,
    current: Option<Value>,
    text: &str,
    styles: Value,
    href: Option<String>,
) -> Value {
    let run = json!({ "type": "text", "text": text, "styles": styles });
    let link = |href: String, run: Value| json!({ "type": "link", "href": href, "content": [run] });
    match (current, href) {
        (None, None) => run,
        (None, Some(href)) => link(href, run),
        (Some(mut node), None) if node["type"] == "text" => {
            if node["styles"] == styles {
                append_text(&mut node, text);
                node
            } else {
                content.push(node);
                run
            }
        }
        (Some(node), Some(href)) if node["type"] == "text" => {
            content.push(node);
            link(href, run)
        }
        (Some(mut node), Some(href)) if node["href"].as_str() == Some(href.as_str()) => {
            if let Some(items) = node["content"].as_array_mut() {
                match items.last_mut() {
                    Some(last) if last["styles"] == styles => append_text(last, text),
                    _ => items.push(run),
                }
            }
            node
        }
        (Some(node), Some(href)) => {
            content.push(node);
            link(href, run)
        }
        (Some(node), None) => {
            content.push(node);
            run
        }
    }
}

/// Port of `contentNodeToTableContent`.
fn table_content<T: ReadTxn>(txn: &T, table: &XmlElementRef) -> Value {
    let mut widths: Vec<Value> = Vec::new();
    let mut rows = Vec::new();
    let mut header: Vec<Vec<bool>> = Vec::new();
    let mut row_index = 0;
    for row in table.children(txn) {
        let XmlOut::Element(row) = row else { continue };
        let mut cells = Vec::new();
        let mut header_row = Vec::new();
        for cell in row.children(txn) {
            let XmlOut::Element(cell) = cell else {
                continue;
            };
            let get = |name: &str| {
                attr(txn, &cell, name).unwrap_or_else(|| {
                    SCHEMA
                        .cell
                        .defaults
                        .get(name)
                        .cloned()
                        .unwrap_or(Value::Null)
                })
            };
            if row_index == 0 {
                match get("colwidth") {
                    Value::Array(values) => widths.extend(values),
                    _ => widths
                        .extend((0..get("colspan").as_u64().unwrap_or(1)).map(|_| Value::Null)),
                }
            }
            header_row.push(cell.tag().as_ref() == "tableHeader");
            let mut content: Vec<Value> = Vec::new();
            for paragraph in cell.children(txn) {
                let XmlOut::Element(paragraph) = paragraph else {
                    continue;
                };
                let mut part = inline_content(txn, &paragraph).into_iter();
                let Some(first) = part.next() else {
                    continue;
                };
                match content.last_mut() {
                    Some(last)
                        if last["type"] == "text"
                            && first["type"] == "text"
                            && last["styles"] == first["styles"] =>
                    {
                        let joined = format!(
                            "{}\n{}",
                            last["text"].as_str().unwrap_or(""),
                            first["text"].as_str().unwrap_or("")
                        );
                        last["text"] = Value::String(joined);
                    }
                    _ => content.push(first),
                }
                content.extend(part);
            }
            cells.push(json!({
                "type": "tableCell",
                "content": content,
                "props": {
                    "colspan": get("colspan"),
                    "rowspan": get("rowspan"),
                    "backgroundColor": get("backgroundColor"),
                    "textColor": get("textColor"),
                    "textAlignment": get("textAlignment"),
                },
            }));
        }
        header.push(header_row);
        rows.push(json!({ "cells": cells }));
        row_index += 1;
    }
    let mut out = json!({ "type": "tableContent", "columnWidths": widths, "rows": rows });
    let header_rows = header
        .iter()
        .filter(|row| row.iter().all(|is_header| *is_header))
        .count();
    if header_rows > 0 {
        out["headerRows"] = json!(header_rows);
    }
    let columns = header.first().map_or(0, Vec::len);
    let header_cols = (0..columns)
        .filter(|index| {
            header
                .iter()
                .all(|row| row.get(*index).copied().unwrap_or(false))
        })
        .count();
    if header_cols > 0 {
        out["headerCols"] = json!(header_cols);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_table_loads_and_versions_are_stable() {
        assert!(schema().blocks.contains_key("paragraph"));
        assert!(schema().blocks.contains_key("callout"));
        assert_eq!(converter_version(), converter_version());
        assert!(converter_version() > 0);
    }

    #[test]
    fn empty_documents_become_one_paragraph_and_unknown_types_are_dropped() {
        let update = blocks_to_update(&[json!({"type": "video", "id": "v"})], Some(7));
        let doc = new_doc(None);
        doc.transact_mut()
            .apply_update(yrs::Update::decode_v1(&update).unwrap())
            .unwrap();
        let blocks = doc_to_blocks(&doc);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0]["type"], "paragraph");
        assert_eq!(blocks[0]["content"], json!([]));
    }

    #[test]
    fn rowspans_shift_absolute_columns() {
        let rows = vec![
            json!({"cells": [{"type": "tableCell", "props": {"rowspan": 2}, "content": []}, "b", "c"]}),
            json!({"cells": ["e", "f"]}),
        ];
        assert_eq!(
            absolute_columns(&rows),
            Some(vec![vec![0, 1, 2], vec![1, 2]])
        );
    }

    use yrs::updates::decoder::Decode;
}
