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

/// Deepest block nesting in [`doc_to_blocks`] and the markdown import: deeper blocks become flat
/// children of the block at this depth. Keeps the JSON (two levels per block) under serde_json's
/// recursion limit of 128 and bounds every recursive walk over blocks.
pub const MAX_BLOCK_DEPTH: usize = 50;

/// Deepest element nesting a live document may reach (two elements per block level).
pub const MAX_XML_DEPTH: usize = 256;

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

/// Replaces the whole document inside `txn`: connected editors receive it as an ordinary update.
pub fn replace_blocks(txn: &mut TransactionMut, fragment: &XmlFragmentRef, blocks: &[Value]) {
    let len = fragment.len(txn);
    if len > 0 {
        fragment.remove_range(txn, 0, len);
    }
    write_blocks(txn, fragment, blocks);
}

/// Changes the document to `blocks` with small edits (restore, import, API content writes on a
/// live document), so what stays the same keeps its Yjs items: comment marks, and the place of
/// concurrent edits. Per sibling group, blocks are matched by id in order; a block that stays
/// gets only its changed props, and changed text only its changed middle when that lies in one
/// run of text (else the block's inline content is rewritten). Removed blocks are deleted, new
/// and moved ones written fresh, a changed table rewritten whole. Marks outside the editor's
/// styles (comment marks) are ignored when comparing and not given to new text.
pub fn reconcile_blocks(txn: &mut TransactionMut, fragment: &XmlFragmentRef, blocks: &[Value]) {
    let group = match fragment.get(txn, 0) {
        Some(XmlOut::Element(group))
            if fragment.len(txn) == 1 && group.tag().as_ref() == "blockGroup" =>
        {
            group
        }
        _ => {
            replace_blocks(txn, fragment, blocks);
            return;
        }
    };
    let target = normalize_blocks(blocks);
    if !shares_ids(txn, &group, &target) {
        // Nothing stays: a fresh group (inserts into the old one would walk its deleted items).
        replace_blocks(txn, fragment, &target);
        return;
    }
    reconcile_group(txn, &group, &target, 1);
}

/// Whether a block of `target` has the id of a block of `group`.
fn shares_ids<T: ReadTxn>(txn: &T, group: &XmlElementRef, target: &[Value]) -> bool {
    let ids: std::collections::HashSet<&str> = target
        .iter()
        .filter_map(|block| block.get("id").and_then(Value::as_str))
        .collect();
    containers(txn, group).any(|container| {
        attr(txn, &container, "id")
            .as_ref()
            .and_then(Value::as_str)
            .is_some_and(|id| ids.contains(id))
    })
}

/// Blocks in the form a document built from them reads back as (props filled in, known types
/// only; no blocks become one empty paragraph). Built in small documents: appending to a long
/// sibling list costs yrs a walk over it.
#[must_use]
pub fn normalize_blocks(blocks: &[Value]) -> Vec<Value> {
    let known: Vec<&Value> = blocks.iter().filter(|block| known_type(block)).collect();
    if known.is_empty() {
        let doc = new_doc(None);
        let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
        write_blocks(&mut doc.transact_mut(), &fragment, &[]);
        return doc_to_blocks(&doc);
    }
    let mut out = Vec::with_capacity(known.len());
    for chunk in known.chunks(64) {
        let doc = new_doc(None);
        let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
        {
            let mut txn = doc.transact_mut();
            let group = fragment.push_back(&mut txn, XmlElementPrelim::empty("blockGroup"));
            for block in chunk {
                write_container(&mut txn, &group, block);
            }
        }
        out.extend(doc_to_blocks(&doc));
    }
    out
}

/// Removes every child of `element`.
fn clear(txn: &mut TransactionMut, element: &XmlElementRef) {
    let len = element.len(txn);
    if len > 0 {
        element.remove_range(txn, 0, len);
    }
}

fn reconcile_group(
    txn: &mut TransactionMut,
    group: &XmlElementRef,
    target: &[Value],
    depth: usize,
) {
    let containers: Option<Vec<XmlElementRef>> = group
        .children(txn)
        .map(|child| match child {
            XmlOut::Element(element) if element.tag().as_ref() == "blockContainer" => Some(element),
            _ => None,
        })
        .collect();
    let rewrite = |txn: &mut TransactionMut| {
        clear(txn, group);
        for block in target {
            write_container(txn, group, block);
        }
    };
    let Some(containers) = containers else {
        rewrite(txn);
        return;
    };
    if depth >= MAX_BLOCK_DEPTH {
        // Read flat below the depth limit: compare the group as a whole.
        let current: Vec<Value> = containers
            .iter()
            .filter_map(|container| container_to_block(txn, container, depth))
            .collect();
        if current != target {
            rewrite(txn);
        }
        return;
    }
    let mut positions: HashMap<&str, usize> = HashMap::new();
    for (index, block) in target.iter().enumerate() {
        if let Some(id) = block.get("id").and_then(Value::as_str) {
            positions.entry(id).or_insert(index);
        }
    }
    let mut taken = vec![false; target.len()];
    let candidates: Vec<(usize, usize)> = containers
        .iter()
        .enumerate()
        .filter_map(|(index, container)| {
            let id = attr(txn, container, "id")?;
            let position = *positions.get(id.as_str()?)?;
            (!std::mem::replace(&mut taken[position], true)).then_some((index, position))
        })
        .collect();
    let kept = increasing_run(&candidates);
    let mut keep = vec![false; containers.len()];
    for (index, _) in &kept {
        keep[*index] = true;
    }
    // Removed blocks go in runs, from the end (each removal walks the group to its index).
    let mut end = containers.len();
    while end > 0 {
        if keep[end - 1] {
            end -= 1;
            continue;
        }
        let mut start = end - 1;
        while start > 0 && !keep[start - 1] {
            start -= 1;
        }
        group.remove_range(txn, start as u32, (end - start) as u32);
        end = start;
    }
    let mut kept = kept.into_iter().peekable();
    let mut at = 0_u32;
    for (position, block) in target.iter().enumerate() {
        match kept.next_if(|(_, target)| *target == position) {
            Some((index, _)) => {
                reconcile_container(txn, group, at, &containers[index], block, depth);
                at += 1;
            }
            None => at += u32::from(write_container_at(txn, group, at, block)),
        }
    }
}

/// The longest run of `(current, target)` pairs (in current order) increasing in `target`.
fn increasing_run(pairs: &[(usize, usize)]) -> Vec<(usize, usize)> {
    let mut tails: Vec<usize> = Vec::new();
    let mut previous: Vec<Option<usize>> = vec![None; pairs.len()];
    for (index, &(_, target)) in pairs.iter().enumerate() {
        let at = tails.partition_point(|&tail| pairs[tail].1 < target);
        previous[index] = at.checked_sub(1).map(|before| tails[before]);
        if at == tails.len() {
            tails.push(index);
        } else {
            tails[at] = index;
        }
    }
    let mut out = Vec::with_capacity(tails.len());
    let mut cursor = tails.last().copied();
    while let Some(index) = cursor {
        out.push(pairs[index]);
        cursor = previous[index];
    }
    out.reverse();
    out
}

/// Brings the `at`-th container of `group` (which has the block's id) to `block`.
fn reconcile_container(
    txn: &mut TransactionMut,
    group: &XmlElementRef,
    at: u32,
    container: &XmlElementRef,
    block: &Value,
    depth: usize,
) {
    let block_type = block.get("type").and_then(Value::as_str).unwrap_or("");
    let mut parts = container.children(txn);
    let content = parts.next();
    let child_group = parts.next();
    let extra = parts.next().is_some();
    let (Some(table), Some(XmlOut::Element(content))) = (SCHEMA.blocks.get(block_type), content)
    else {
        group.remove_range(txn, at, 1);
        write_container_at(txn, group, at, block);
        return;
    };
    let child_group = match child_group {
        None => None,
        Some(XmlOut::Element(child)) if !extra && child.tag().as_ref() == "blockGroup" => {
            Some(child)
        }
        Some(_) => {
            group.remove_range(txn, at, 1);
            write_container_at(txn, group, at, block);
            return;
        }
    };
    if content.tag().as_ref() == block_type {
        reconcile_attrs(txn, &content, table, block);
        match table.content.as_str() {
            "inline" => reconcile_inline(txn, &content, block.get("content"), false),
            "plain" => reconcile_inline(txn, &content, block.get("content"), true),
            "table" => {
                let wanted = block.get("content").cloned().unwrap_or(Value::Null);
                if table_content(txn, &content) != wanted {
                    clear(txn, &content);
                    write_table(txn, &content, Some(&wanted));
                }
            }
            _ => {}
        }
    } else {
        container.remove_range(txn, 0, 1);
        write_content_node(txn, container, block_type, table, block);
    }
    let children = known_children(block);
    match (child_group, children.is_empty()) {
        (Some(_), true) => container.remove_range(txn, 1, 1),
        (Some(child_group), false) => {
            let children: Vec<Value> = children.into_iter().cloned().collect();
            if shares_ids(txn, &child_group, &children) {
                reconcile_group(txn, &child_group, &children, depth + 1);
            } else {
                container.remove_range(txn, 1, 1);
                let child_group = container.push_back(txn, XmlElementPrelim::empty("blockGroup"));
                for child in &children {
                    write_container(txn, &child_group, child);
                }
            }
        }
        (None, false) => {
            let child_group = container.push_back(txn, XmlElementPrelim::empty("blockGroup"));
            for child in children {
                write_container(txn, &child_group, child);
            }
        }
        (None, true) => {}
    }
}

/// Sets the props that differ (attributes the schema does not know are left alone).
fn reconcile_attrs(
    txn: &mut TransactionMut,
    element: &XmlElementRef,
    table: &BlockTable,
    block: &Value,
) {
    let props = block.get("props").and_then(Value::as_object);
    for attr_table in &table.attrs {
        let wanted = props
            .and_then(|props| props.get(&attr_table.name))
            .cloned()
            .unwrap_or_else(|| attr_table.missing.value.clone());
        let current = attr(txn, element, &attr_table.name);
        if wanted.is_null() {
            if current.is_some() {
                element.remove_attribute(txn, &attr_table.name);
            }
        } else if current.as_ref() != Some(&wanted) {
            element.insert_attribute(txn, attr_table.name.as_str(), any(&wanted));
        }
    }
}

/// One unit of inline content for comparing: a character with its editor marks, a hard break,
/// an atom (mention) with its props, or something else (never equal).
#[derive(Debug, PartialEq)]
enum Piece {
    Char(char, Value, Option<String>),
    Break,
    Atom(String, Vec<(String, Value)>),
    Other,
}

/// Where a character of the current content is: its text node and UTF-16 offset and length.
type Place = Option<(usize, u32, u32)>;

fn reconcile_inline(
    txn: &mut TransactionMut,
    element: &XmlElementRef,
    content: Option<&Value>,
    plain: bool,
) {
    let runs = inline_runs(content, plain);
    let mut wanted: Vec<(Piece, Option<&yrs::types::Attrs>)> = Vec::new();
    for run in &runs {
        match run {
            Run::Text(text, attrs) => {
                let (styles, href) = styles_of(Some(attrs));
                wanted.extend(
                    text.chars()
                        .map(|c| (Piece::Char(c, styles.clone(), href.clone()), Some(attrs))),
                );
            }
            Run::Break => wanted.push((Piece::Break, None)),
            Run::Inline(tag, attrs) => wanted.push((
                Piece::Atom(
                    tag.clone(),
                    attrs
                        .iter()
                        .map(|(name, value)| (name.clone(), json(value)))
                        .collect(),
                ),
                None,
            )),
        }
    }
    let mut texts: Vec<XmlTextRef> = Vec::new();
    let mut current: Vec<(Piece, Place)> = Vec::new();
    for child in element.children(txn) {
        match child {
            XmlOut::Text(text) => {
                let node = texts.len();
                let mut offset = 0_u32;
                for chunk in text.diff(txn, YChange::identity) {
                    let Out::Any(Any::String(value)) = chunk.insert else {
                        current.push((Piece::Other, None));
                        continue;
                    };
                    let (styles, href) = styles_of(chunk.attributes.as_deref());
                    for c in value.chars() {
                        let len = c.len_utf16() as u32;
                        current.push((
                            Piece::Char(c, styles.clone(), href.clone()),
                            Some((node, offset, len)),
                        ));
                        offset += len;
                    }
                }
                texts.push(text);
            }
            XmlOut::Element(child) if child.tag().as_ref() == "hardBreak" && !plain => {
                current.push((Piece::Break, None));
            }
            XmlOut::Element(child) => match SCHEMA.inline.get(child.tag().as_ref()) {
                Some(table) if table.content == "none" && !plain => {
                    let attrs = table
                        .attrs
                        .iter()
                        .filter_map(|attr_table| {
                            let value = attr(txn, &child, &attr_table.name)
                                .unwrap_or_else(|| attr_table.missing.value.clone());
                            (!value.is_null()).then(|| (attr_table.name.clone(), value))
                        })
                        .collect();
                    current.push((Piece::Atom(child.tag().to_string(), attrs), None));
                }
                _ => current.push((Piece::Other, None)),
            },
            XmlOut::Fragment(_) => current.push((Piece::Other, None)),
        }
    }
    // In plain blocks (code) marks do not count.
    let same = |a: &Piece, b: &Piece| match (a, b) {
        (Piece::Char(x, _, _), Piece::Char(y, _, _)) if plain => x == y,
        (Piece::Other, _) | (_, Piece::Other) => false,
        (a, b) => a == b,
    };
    let mut prefix = 0;
    while prefix < current.len()
        && prefix < wanted.len()
        && same(&current[prefix].0, &wanted[prefix].0)
    {
        prefix += 1;
    }
    if prefix == current.len() && prefix == wanted.len() {
        return;
    }
    let mut suffix = 0;
    while suffix < current.len() - prefix
        && suffix < wanted.len() - prefix
        && same(
            &current[current.len() - 1 - suffix].0,
            &wanted[wanted.len() - 1 - suffix].0,
        )
    {
        suffix += 1;
    }
    let old_middle = &current[prefix..current.len() - suffix];
    let new_middle = &wanted[prefix..wanted.len() - suffix];
    // The changed middle as one edit inside one text node, when it lies in one.
    let place = if let Some((first, _)) = old_middle.first() {
        let (node, offset, _) = old_middle[0].1.unwrap_or((usize::MAX, 0, 0));
        let in_one = !matches!(first, Piece::Other)
            && old_middle.iter().all(|(piece, place)| {
                matches!(piece, Piece::Char(..)) && place.is_some_and(|p| p.0 == node)
            });
        in_one.then(|| {
            let len = old_middle
                .iter()
                .filter_map(|(_, place)| *place)
                .map(|p| p.2)
                .sum();
            (node, offset, len)
        })
    } else if let Some((_, Some((node, offset, len)))) = prefix.checked_sub(1).map(|i| &current[i])
    {
        Some((*node, offset + len, 0))
    } else if let Some((_, Some((node, offset, _)))) = current.get(prefix) {
        Some((*node, *offset, 0))
    } else {
        None
    };
    let only_text = new_middle
        .iter()
        .all(|(piece, attrs)| matches!(piece, Piece::Char(..)) && attrs.is_some());
    match place.filter(|_| only_text) {
        Some((node, offset, len)) => {
            let text = &texts[node];
            if len > 0 {
                text.remove_range(txn, offset, len);
            }
            let mut at = offset;
            let mut index = 0;
            while index < new_middle.len() {
                let attrs = new_middle[index].1.expect("text has attributes");
                let mut run = String::new();
                while let Some((Piece::Char(c, _, _), Some(next))) = new_middle.get(index)
                    && *next == attrs
                {
                    run.push(*c);
                    index += 1;
                }
                // Explicit attributes: the new text gets no mark of its neighbors (comments).
                text.insert_with_attributes(txn, at, &run, attrs.clone());
                at += run.encode_utf16().count() as u32;
            }
        }
        None => {
            clear(txn, element);
            write_runs(txn, element, &runs);
        }
    }
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
    let at = parent.len(txn);
    write_container_at(txn, parent, at, block);
}

/// Inserts the block as the `at`-th child of `parent`; `false` for an unknown block type.
fn write_container_at(
    txn: &mut TransactionMut,
    parent: &XmlElementRef,
    at: u32,
    block: &Value,
) -> bool {
    let block_type = block
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("paragraph");
    let Some(table) = SCHEMA.blocks.get(block_type) else {
        return false;
    };
    let container = parent.insert(txn, at, XmlElementPrelim::empty("blockContainer"));
    let id = block
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map_or_else(new_block_id, str::to_owned);
    container.insert_attribute(txn, "id", Any::from(id));
    write_content_node(txn, &container, block_type, table, block);
    let children = known_children(block);
    if !children.is_empty() {
        let group = container.push_back(txn, XmlElementPrelim::empty("blockGroup"));
        for child in children {
            write_container(txn, &group, child);
        }
    }
    true
}

fn known_children(block: &Value) -> Vec<&Value> {
    block
        .get("children")
        .and_then(Value::as_array)
        .map(|children| children.iter().filter(|child| known_type(child)).collect())
        .unwrap_or_default()
}

/// The block's content element (its type, props and content) as the container's first child.
fn write_content_node(
    txn: &mut TransactionMut,
    container: &XmlElementRef,
    block_type: &str,
    table: &BlockTable,
    block: &Value,
) {
    let content_node = container.insert(txn, 0, XmlElementPrelim::empty(block_type));
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
    containers(txn, &group)
        .filter_map(|container| container_to_block(txn, &container, 1))
        .collect()
}

/// Whether the document nests elements deeper than [`MAX_XML_DEPTH`].
pub fn too_deep<T: ReadTxn>(txn: &T, fragment: &XmlFragmentRef) -> bool {
    let mut stack: Vec<(XmlElementRef, usize)> = fragment
        .children(txn)
        .filter_map(|node| match node {
            XmlOut::Element(element) => Some((element, 1)),
            _ => None,
        })
        .collect();
    while let Some((element, depth)) = stack.pop() {
        if depth > MAX_XML_DEPTH {
            return true;
        }
        for child in element.children(txn) {
            if let XmlOut::Element(child) = child {
                stack.push((child, depth + 1));
            }
        }
    }
    false
}

/// Whether `update` (v1) nests shared types more than [`MAX_XML_DEPTH`] levels below where it
/// attaches to the document, or holds an `Any` value nested that deep. yrs recurses once per
/// level to decode such a value and to delete or collect such a subtree, so the hub runs this
/// before yrs sees an update. It reads the blocks like yrs's `Update::decode`, without recursion.
pub fn update_too_deep(update: &[u8]) -> Result<bool, yrs::encoding::read::Error> {
    use yrs::block::{
        BLOCK_GC_REF_NUMBER, BLOCK_SKIP_REF_NUMBER, HAS_ORIGIN, HAS_PARENT_SUB, HAS_RIGHT_ORIGIN,
    };
    use yrs::encoding::read::{Cursor, Error, Read};
    use yrs::updates::decoder::{Decoder, DecoderV1};

    /// How an item attaches: under a type (one level deeper) or next to an item (same level).
    enum Link {
        Root,
        Parent(yrs::ID),
        Sibling(yrs::ID),
    }
    struct Entry {
        clock: u32,
        len: u32,
        link: Link,
        /// Levels below the attachment point; `None` until computed.
        depth: Option<usize>,
    }

    let mut decoder = DecoderV1::new(Cursor::new(update));
    let mut clients: HashMap<yrs::ClientID, Vec<Entry>> = HashMap::new();
    let sections: u32 = decoder.read_var()?;
    for _ in 0..sections {
        let blocks: u32 = decoder.read_var()?;
        let client = decoder.read_client()?;
        let mut clock: u32 = decoder.read_var()?;
        let entries = clients.entry(client).or_default();
        for _ in 0..blocks {
            let info = decoder.read_info()?;
            let len = match info {
                BLOCK_SKIP_REF_NUMBER => decoder.read_var()?,
                BLOCK_GC_REF_NUMBER => decoder.read_len()?,
                info => {
                    let origin = if info & HAS_ORIGIN != 0 {
                        Some(decoder.read_left_id()?)
                    } else {
                        None
                    };
                    let right = if info & HAS_RIGHT_ORIGIN != 0 {
                        Some(decoder.read_right_id()?)
                    } else {
                        None
                    };
                    let link = match origin.or(right) {
                        Some(id) => Link::Sibling(id),
                        None => {
                            let link = if decoder.read_parent_info()? {
                                decoder.read_string()?;
                                Link::Root
                            } else {
                                Link::Parent(decoder.read_left_id()?)
                            };
                            if info & HAS_PARENT_SUB != 0 {
                                decoder.read_string()?;
                            }
                            link
                        }
                    };
                    let Some(len) = content_len(&mut decoder, info)? else {
                        return Ok(true);
                    };
                    if len == 0 {
                        // yrs drops empty items without advancing the clock.
                        continue;
                    }
                    entries.push(Entry {
                        clock,
                        len,
                        link,
                        depth: None,
                    });
                    len
                }
            };
            clock = clock.checked_add(len).ok_or(Error::UnexpectedValue)?;
        }
    }
    for entries in clients.values_mut() {
        entries.sort_by_key(|entry| entry.clock);
    }
    let find = |clients: &HashMap<yrs::ClientID, Vec<Entry>>, id: &yrs::ID| {
        let entries = clients.get(&id.client)?;
        let index = entries
            .partition_point(|entry| entry.clock <= id.clock)
            .checked_sub(1)?;
        let entry = &entries[index];
        (id.clock - entry.clock < entry.len).then_some((id.client, index))
    };
    let total: usize = clients.values().map(Vec::len).sum();
    let keys: Vec<(yrs::ClientID, usize)> = clients
        .iter()
        .flat_map(|(client, entries)| (0..entries.len()).map(|index| (*client, index)))
        .collect();
    for key in keys {
        // Follows the links with an explicit stack; a chain longer than the update is a cycle.
        let mut stack = vec![key];
        while let Some(&(client, index)) = stack.last() {
            let entry = &clients[&client][index];
            if entry.depth.is_some() {
                stack.pop();
                continue;
            }
            let (target, step) = match &entry.link {
                Link::Root => (None, 0),
                Link::Parent(id) => (find(&clients, id), 1),
                Link::Sibling(id) => (find(&clients, id), 0),
            };
            let depth = match target {
                None => step,
                Some(target) => match clients[&target.0][target.1].depth {
                    Some(depth) => depth + step,
                    None => {
                        if stack.len() > total {
                            return Err(Error::UnexpectedValue);
                        }
                        stack.push(target);
                        continue;
                    }
                },
            };
            if depth > MAX_XML_DEPTH {
                return Ok(true);
            }
            clients.get_mut(&client).expect("entry")[index].depth = Some(depth);
            stack.pop();
        }
    }
    Ok(false)
}

/// Reads an item's content like `ItemContent::decode`; returns its length in clock units, or
/// `None` for an `Any` nested deeper than [`MAX_XML_DEPTH`].
fn content_len(
    decoder: &mut yrs::updates::decoder::DecoderV1<'_>,
    info: u8,
) -> Result<Option<u32>, yrs::encoding::read::Error> {
    use yrs::encoding::read::{Error, Read};
    use yrs::updates::decoder::Decoder;
    Ok(Some(match info & 0b1111 {
        1 => decoder.read_len()?,
        2 => {
            let count = decoder.read_len()?;
            for _ in 0..=count {
                decoder.read_string()?;
            }
            count.checked_add(1).ok_or(Error::UnexpectedValue)?
        }
        3 => {
            decoder.read_buf()?;
            1
        }
        4 => u32::try_from(decoder.read_string()?.encode_utf16().count())
            .map_err(|_| Error::UnexpectedValue)?,
        5 => {
            decoder.read_string()?;
            1
        }
        6 => {
            decoder.read_string()?;
            decoder.read_string()?;
            1
        }
        7 => {
            match decoder.read_type_ref()? {
                yrs::types::TYPE_REFS_XML_ELEMENT => {
                    decoder.read_string()?;
                }
                0..=6 | 9 | 15 => {}
                _ => return Err(Error::UnexpectedValue),
            }
            1
        }
        8 => {
            let count = decoder.read_len()?;
            for _ in 0..count {
                if !skip_any(decoder)? {
                    return Ok(None);
                }
            }
            count
        }
        9 => {
            decoder.read_string()?;
            if !skip_any(decoder)? {
                return Ok(None);
            }
            1
        }
        _ => return Err(Error::UnexpectedValue),
    }))
}

/// Reads one lib0 `Any` like `Any::decode`; `false` when it nests deeper than
/// [`MAX_XML_DEPTH`].
fn skip_any(
    decoder: &mut yrs::updates::decoder::DecoderV1<'_>,
) -> Result<bool, yrs::encoding::read::Error> {
    use yrs::encoding::read::{Error, Read};
    // Values still to read at each open level, and whether that level is a map.
    let mut open: Vec<(usize, bool)> = vec![(1, false)];
    while let Some((left, map)) = open.last_mut() {
        if *left == 0 {
            open.pop();
            continue;
        }
        *left -= 1;
        if *map {
            decoder.read_string()?;
        }
        match decoder.read_u8()? {
            127 | 126 | 121 | 120 => {}
            125 => {
                decoder.read_var::<i64>()?;
            }
            124 => {
                decoder.read_exact(4)?;
            }
            123 | 122 => {
                decoder.read_exact(8)?;
            }
            119 => {
                decoder.read_string()?;
            }
            116 => {
                decoder.read_buf()?;
            }
            kind @ (117 | 118) => {
                let len: usize = decoder.read_var()?;
                open.push((len, kind == 118));
                if open.len() > MAX_XML_DEPTH {
                    return Ok(false);
                }
            }
            _ => return Err(Error::UnexpectedValue),
        }
    }
    Ok(true)
}

/// The `blockContainer` children of a `blockGroup`.
fn containers<'a, T: ReadTxn>(
    txn: &'a T,
    group: &XmlElementRef,
) -> impl Iterator<Item = XmlElementRef> + 'a {
    group.children(txn).filter_map(|node| match node {
        XmlOut::Element(element) if element.tag().as_ref() == "blockContainer" => Some(element),
        _ => None,
    })
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

/// The block of a `blockContainer` at nesting `depth` (1 = top level). Below
/// [`MAX_BLOCK_DEPTH`] the nested blocks are read without recursion and kept as flat children.
fn container_to_block<T: ReadTxn>(
    txn: &T,
    container: &XmlElementRef,
    depth: usize,
) -> Option<Value> {
    let (mut block, group) = container_parts(txn, container)?;
    let children: Vec<Value> = match group {
        Some(group) if depth < MAX_BLOCK_DEPTH => containers(txn, &group)
            .filter_map(|child| container_to_block(txn, &child, depth + 1))
            .collect(),
        Some(group) => {
            let mut flat = Vec::new();
            let mut stack: Vec<XmlElementRef> = containers(txn, &group).collect();
            stack.reverse();
            while let Some(child) = stack.pop() {
                let Some((mut child_block, child_group)) = container_parts(txn, &child) else {
                    continue;
                };
                if let Some(child_group) = child_group {
                    let start = stack.len();
                    stack.extend(containers(txn, &child_group));
                    stack[start..].reverse();
                }
                child_block.insert("children".into(), json!([]));
                flat.push(Value::Object(child_block));
            }
            flat
        }
        None => Vec::new(),
    };
    block.insert("children".into(), Value::Array(children));
    Some(Value::Object(block))
}

/// A container's block without `children`, and its `blockGroup` of nested blocks.
fn container_parts<T: ReadTxn>(
    txn: &T,
    container: &XmlElementRef,
) -> Option<(Map<String, Value>, Option<XmlElementRef>)> {
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
    let group = match group {
        Some(XmlOut::Element(group)) if group.tag().as_ref() == "blockGroup" => Some(group),
        _ => None,
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
    Some((block, group))
}

/// The text under `element`, in document order (an explicit stack: nesting is unbounded).
fn text_content<T: ReadTxn>(txn: &T, element: &XmlElementRef) -> String {
    let mut out = String::new();
    let mut stack: Vec<XmlOut> = element.children(txn).collect();
    stack.reverse();
    while let Some(child) = stack.pop() {
        match child {
            XmlOut::Text(text) => {
                for chunk in text.diff(txn, YChange::identity) {
                    if let Out::Any(Any::String(value)) = chunk.insert {
                        out.push_str(&value);
                    }
                }
            }
            XmlOut::Element(element) => {
                let start = stack.len();
                stack.extend(element.children(txn));
                stack[start..].reverse();
            }
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

    /// A document whose first block nests `levels` blocks deep, its innermost one a code block
    /// whose text sits `levels` elements deep.
    fn deep_doc(levels: usize) -> Doc {
        let doc = new_doc(None);
        {
            let mut txn = doc.transact_mut();
            let fragment = txn.get_or_insert_xml_fragment(FRAGMENT);
            let mut group = fragment.push_back(&mut txn, XmlElementPrelim::empty("blockGroup"));
            for level in 0..levels {
                let container =
                    group.push_back(&mut txn, XmlElementPrelim::empty("blockContainer"));
                container.insert_attribute(&mut txn, "id", level.to_string());
                if level + 1 == levels {
                    let mut node =
                        container.push_back(&mut txn, XmlElementPrelim::empty("codeBlock"));
                    for _ in 0..levels {
                        node = node.push_back(&mut txn, XmlElementPrelim::empty("span"));
                    }
                    node.push_back(&mut txn, XmlTextPrelim::new("deep"));
                } else {
                    let paragraph =
                        container.push_back(&mut txn, XmlElementPrelim::empty("paragraph"));
                    paragraph.push_back(&mut txn, XmlTextPrelim::new(level.to_string()));
                    group = container.push_back(&mut txn, XmlElementPrelim::empty("blockGroup"));
                }
            }
        }
        doc
    }

    #[test]
    fn deeply_nested_documents_are_read_without_recursing() {
        // Runs on a test thread (2 MB stack): one frame per level would overflow it.
        let doc = deep_doc(100_000);
        {
            let txn = doc.transact();
            let fragment = txn.get_xml_fragment(FRAGMENT).unwrap();
            assert!(too_deep(&txn, &fragment));
        }
        let blocks = doc_to_blocks(&doc);
        let mut block = &blocks[0];
        for _ in 1..MAX_BLOCK_DEPTH {
            block = &block["children"][0];
        }
        // The blocks below the limit are kept, in order, as flat children of the last one.
        let flat = block["children"].as_array().unwrap();
        assert_eq!(flat.len(), 100_000 - MAX_BLOCK_DEPTH);
        assert_eq!(flat[0]["id"], MAX_BLOCK_DEPTH.to_string());
        assert_eq!(flat[0]["content"][0]["text"], MAX_BLOCK_DEPTH.to_string());
        assert!(flat.iter().all(|child| child["children"] == json!([])));
        let last = flat.last().unwrap();
        assert_eq!(last["type"], "codeBlock");
        assert_eq!(last["content"][0]["text"], "deep");
        // The capped blocks fit serde_json's recursion limit.
        let encoded = serde_json::to_string(&blocks).unwrap();
        assert!(serde_json::from_str::<Value>(&encoded).is_ok());

        let mut txn = doc.transact_mut();
        let fragment = txn.get_or_insert_xml_fragment(FRAGMENT);
        assert!(!crate::collab::sanitize::repair_document(
            &mut txn, &fragment
        ));
    }

    #[test]
    fn documents_past_the_element_depth_limit_are_too_deep() {
        let depth = |doc: &Doc| {
            let txn = doc.transact();
            too_deep(&txn, &txn.get_xml_fragment(FRAGMENT).unwrap())
        };
        // 1 + 2 * 39 + 2 + 40 = 121 elements deep, then 1 + 2 * 99 + 2 + 100 = 301.
        assert!(!depth(&deep_doc(40)));
        assert!(depth(&deep_doc(100)));
    }

    /// A v1 update with one `Any` value nested `levels` arrays deep in a root named `x`.
    fn nested_any_update(levels: usize) -> Vec<u8> {
        let mut update = vec![1, 1, 7, 0, 8, 1, 1, b'x', 1];
        for _ in 0..levels {
            update.extend([117, 1]);
        }
        update.extend([126, 0]);
        update
    }

    /// A v1 update of `levels` elements, each inside the previous one.
    fn chain_update(levels: usize) -> Vec<u8> {
        let doc = new_doc(Some(7));
        {
            let mut txn = doc.transact_mut();
            let mut node = txn
                .get_or_insert_xml_fragment(FRAGMENT)
                .push_back(&mut txn, XmlElementPrelim::empty("a"));
            for _ in 1..levels {
                node = node.push_back(&mut txn, XmlElementPrelim::empty("a"));
            }
        }
        doc.transact()
            .encode_state_as_update_v1(&StateVector::default())
    }

    #[test]
    fn updates_nesting_past_the_limit_are_found_before_yrs_reads_them() {
        let blocks = vec![
            json!({"type": "paragraph", "content": [
                {"type": "text", "text": "héllo 🙂", "styles": {"bold": true}},
                {"type": "link", "href": "https://example.com", "content": "x"},
                {"type": "mention", "props": {"userId": "u", "name": "n"}},
            ], "children": [{"type": "bulletListItem", "content": "child"}]}),
            json!({"type": "table", "content": {"type": "tableContent", "columnWidths": [100, null],
                "rows": [{"cells": [[{"type": "text", "text": "a", "styles": {}}], []]}]}}),
            json!({"type": "codeBlock", "props": {"language": "rust"}, "content": "fn main() {}"}),
        ];
        assert!(!update_too_deep(&blocks_to_update(&blocks, Some(7))).unwrap());
        assert!(!update_too_deep(&chain_update(MAX_XML_DEPTH)).unwrap());
        assert!(update_too_deep(&chain_update(MAX_XML_DEPTH + 2)).unwrap());
        // The crafted bytes are what yrs reads.
        assert!(yrs::Update::decode_v1(&nested_any_update(3)).is_ok());
        assert!(!update_too_deep(&nested_any_update(3)).unwrap());
        assert!(update_too_deep(&nested_any_update(100_000)).unwrap());
        assert!(update_too_deep(&[1, 1, 7, 0, 8, 1, 1]).is_err());
    }

    use yrs::WriteTxn;
    use yrs::updates::decoder::Decode;
}
