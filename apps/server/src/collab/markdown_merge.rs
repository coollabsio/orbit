//! A Markdown write onto an existing document (token API doc writes): the blocks the Markdown
//! leaves unchanged stay exactly as they are, and changed blocks keep what Markdown cannot show.
//!
//! The current blocks are exported (`export::markdown`, with the same resolver as the read the
//! client saw) and the result is compared line by line with the new Markdown. Per sibling group:
//! - a block whose lines are all unchanged is kept as it is (id, props, content, and for blocks
//!   whose children nest inside it, its children);
//! - in a changed region, old and new blocks are paired by Markdown class (a toggle list item
//!   and a bullet are both `- `, a callout and a quote both `>`). A pair keeps the old id, type
//!   and the props Markdown cannot show (colors, alignment, toggles, emoji, image width), takes
//!   the visible props (heading level, checked, code language, list start) and the new text;
//!   inline styles Markdown cannot show (text colors) and mentions stay on the unchanged start
//!   and end of the text;
//! - children that the exporter writes unindented after a paragraph or heading (a toggle
//!   heading's content) go back under their parent;
//! - a new block equal to a removed one elsewhere in the group is that block, moved;
//! - blocks that write no Markdown (empty paragraphs) stay before their next kept neighbor.
//!
//! The result has the shape `doc_to_blocks` returns; callers sanitize it.

use std::collections::HashMap;
use std::ops::Range;

use orbit_platform::Id;
use serde_json::{Map, Value, json};

use super::blocknote::MAX_BLOCK_DEPTH;
use super::markdown_import::markdown_to_blocks_with_spans;
use crate::export::markdown::{LinkResolver, Span, blocks_to_markdown_with_spans};

/// The most blocks (nested ones included, see [`block_count`]) a doc can have before and after
/// a Markdown write. The Yjs reconcile inserts moved blocks by index, walking every deleted item
/// yrs keeps in the sibling list, so repeated large reorders of one doc get slower each time
/// (reversing 4,000 blocks: 0.29 s the first time, 2.0 s after 8 rounds in release builds). The
/// limit bounds that work, which holds only the doc's room lock, not the database write lock.
pub const MAX_BLOCKS: usize = 4_000;

/// The number of blocks in `blocks`, nested ones included, without a last empty paragraph: the
/// editor keeps one at the end of a doc (an empty doc is one), Markdown does not show it, and a
/// write keeps it.
#[must_use]
pub fn block_count(blocks: &[Value]) -> usize {
    let empty = |value: Option<&Value>| match value {
        None | Some(Value::Null) => true,
        Some(Value::Array(items)) => items.is_empty(),
        Some(Value::String(text)) => text.is_empty(),
        Some(_) => false,
    };
    let trailing = blocks.last().is_some_and(|block| {
        block_type(block) == "paragraph"
            && empty(block.get("content"))
            && empty(block.get("children"))
    });
    let mut count = 0;
    let mut stack = vec![blocks];
    while let Some(group) = stack.pop() {
        count += group.len();
        stack.extend(
            group
                .iter()
                .filter_map(|block| block.get("children").and_then(Value::as_array))
                .map(Vec::as_slice),
        );
    }
    count - usize::from(trailing)
}

/// Gaps without unique lines are compared line by line up to this many line pairs.
const MAX_GAP_CELLS: usize = 1 << 18;

/// Pairs in a changed region are found by a full comparison up to this many block pairs.
const MAX_PAIR_CELLS: usize = 1 << 16;

/// Beyond that, each old block pairs with one of this many next new blocks.
const PAIR_WINDOW: usize = 64;

/// The blocks `markdown` describes, reusing `base` (the current blocks) wherever it can.
/// `members` are `(user id, display name)` pairs for new `@Name` mentions; `resolver` must be
/// the one the client's read used, so unchanged blocks export to the same lines.
#[must_use]
pub fn merge(
    base: &[Value],
    markdown: &str,
    members: &[(Id, String)],
    resolver: &dyn LinkResolver,
) -> Vec<Value> {
    let (old_text, old_spans) = blocks_to_markdown_with_spans(base, resolver);
    let old = lines(&old_text);
    let new = lines(markdown);
    if old == new {
        return base.to_vec();
    }
    let (blocks, spans) = markdown_to_blocks_with_spans(markdown, members);
    let context = Context {
        old_to_new: diff(&old, &new),
        old,
        new,
        resolver,
    };
    context.level(base, &old_spans, &blocks, &spans, 1)
}

/// The lines of a Markdown text, without trailing whitespace (a final newline ends the last
/// line; the importer counts lines the same way).
fn lines(text: &str) -> Vec<&str> {
    let text = text.strip_suffix('\n').unwrap_or(text);
    if text.is_empty() {
        return Vec::new();
    }
    text.split('\n').map(str::trim_end).collect()
}

/// For each line of `a`, the line of `b` it stays as (a patience diff: lines unique on both
/// sides anchor, the gaps between are compared the same way, then line by line when small).
/// Iterative, so no input can recurse deeply.
fn diff(a: &[&str], b: &[&str]) -> Vec<Option<usize>> {
    let mut map = vec![None; a.len()];
    let mut work = vec![(0, a.len(), 0, b.len())];
    while let Some((mut a0, mut a1, mut b0, mut b1)) = work.pop() {
        while a0 < a1 && b0 < b1 && a[a0] == b[b0] {
            map[a0] = Some(b0);
            a0 += 1;
            b0 += 1;
        }
        while a0 < a1 && b0 < b1 && a[a1 - 1] == b[b1 - 1] {
            a1 -= 1;
            b1 -= 1;
            map[a1] = Some(b1);
        }
        if a0 == a1 || b0 == b1 {
            continue;
        }
        let anchors = unique_anchors(&a[a0..a1], &b[b0..b1]);
        if anchors.is_empty() {
            if (a1 - a0) * (b1 - b0) <= MAX_GAP_CELLS {
                for (i, j) in lcs(a1 - a0, b1 - b0, |i, j| a[a0 + i] == b[b0 + j]) {
                    map[a0 + i] = Some(b0 + j);
                }
            }
            continue;
        }
        let (mut pa, mut pb) = (a0, b0);
        for (i, j) in anchors {
            let (i, j) = (a0 + i, b0 + j);
            map[i] = Some(j);
            work.push((pa, i, pb, j));
            pa = i + 1;
            pb = j + 1;
        }
        work.push((pa, a1, pb, b1));
    }
    map
}

/// The longest increasing run of lines that occur exactly once on each side.
fn unique_anchors(a: &[&str], b: &[&str]) -> Vec<(usize, usize)> {
    let mut counts: HashMap<&str, (usize, usize, usize, usize)> = HashMap::new();
    for (index, line) in a.iter().enumerate() {
        let entry = counts.entry(line).or_default();
        entry.0 += 1;
        entry.1 = index;
    }
    for (index, line) in b.iter().enumerate() {
        let entry = counts.entry(line).or_default();
        entry.2 += 1;
        entry.3 = index;
    }
    let mut pairs: Vec<(usize, usize)> = counts
        .into_iter()
        .filter(|(line, (in_a, _, in_b, _))| !line.is_empty() && *in_a == 1 && *in_b == 1)
        .map(|(_, (_, i, _, j))| (i, j))
        .collect();
    pairs.sort_unstable();
    // Patience sorting: the longest run increasing in `j`.
    let mut tails: Vec<usize> = Vec::new();
    let mut previous: Vec<Option<usize>> = vec![None; pairs.len()];
    for (index, &(_, j)) in pairs.iter().enumerate() {
        let at = tails.partition_point(|&tail| pairs[tail].1 < j);
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

/// A longest common subsequence of two sequences of lengths `n` and `m`, as index pairs.
fn lcs(n: usize, m: usize, same: impl Fn(usize, usize) -> bool) -> Vec<(usize, usize)> {
    weighted_lcs(n, m, |i, j| u32::from(same(i, j)))
}

/// The pairing with the highest total weight (0 = cannot pair) that keeps both orders.
fn weighted_lcs(n: usize, m: usize, weight: impl Fn(usize, usize) -> u32) -> Vec<(usize, usize)> {
    let width = m + 1;
    let mut table = vec![0_u32; (n + 1) * width];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            let w = weight(i, j);
            let pair = if w > 0 {
                table[(i + 1) * width + j + 1] + w
            } else {
                0
            };
            table[i * width + j] = pair
                .max(table[(i + 1) * width + j])
                .max(table[i * width + j + 1]);
        }
    }
    let mut out = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        let w = weight(i, j);
        if w > 0 && table[i * width + j] == table[(i + 1) * width + j + 1] + w {
            out.push((i, j));
            i += 1;
            j += 1;
        } else if table[(i + 1) * width + j] >= table[i * width + j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    out
}

/// Text of a line range for comparing blocks across places: each line without its
/// indentation and quote markers, blank lines left out.
fn normalized(lines: &[&str], range: &Range<usize>) -> String {
    let end = range.end.min(lines.len());
    let start = range.start.min(end);
    lines[start..end]
        .iter()
        .map(|line| line.trim_start_matches(|c: char| c == '>' || c.is_whitespace()))
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn block_type(block: &Value) -> &str {
    block["type"].as_str().unwrap_or("")
}

/// Whether the exporter writes the block's children inside it (indented or quoted) rather than
/// unindented after it.
fn nests_children(kind: &str) -> bool {
    matches!(
        kind,
        "bulletListItem"
            | "numberedListItem"
            | "checkListItem"
            | "toggleListItem"
            | "quote"
            | "callout"
    )
}

/// The only link of a paragraph whose whole content is that link.
fn only_link(block: &Value) -> Option<&Value> {
    match block["content"].as_array().map(Vec::as_slice) {
        Some([link]) if link["type"] == "link" && block_type(block) == "paragraph" => Some(link),
        _ => None,
    }
}

/// What a block looks like in Markdown: blocks of one class can turn into each other.
fn class(block: &Value) -> &'static str {
    match block_type(block) {
        "heading" => "heading",
        "bulletListItem" | "toggleListItem" => "bullet",
        "numberedListItem" => "numbered",
        "checkListItem" => "check",
        "quote" | "callout" => "quote",
        "codeBlock" => "code",
        "divider" => "divider",
        "table" => "table",
        "image" => "image",
        "file" | "video" | "audio" | "page" => "link",
        "paragraph" if only_link(block).is_some() => "link",
        _ => "paragraph",
    }
}

/// A caption the exporter writes under media: a paragraph of italic text only.
fn caption_text(block: &Value) -> Option<String> {
    if block_type(block) != "paragraph" {
        return None;
    }
    let mut text = String::new();
    for item in block["content"].as_array()? {
        let italic = item["type"] == "text"
            && item["styles"].as_object().is_some_and(|styles| {
                styles.len() == 1 && styles.get("italic") == Some(&Value::Bool(true))
            });
        if !italic {
            return None;
        }
        text.push_str(item["text"].as_str()?);
    }
    (!text.trim().is_empty()).then_some(text)
}

fn plain_text(content: &Value) -> String {
    let mut out = String::new();
    for item in content.as_array().into_iter().flatten() {
        if let Some(text) = item["text"].as_str() {
            out.push_str(text);
        }
        for part in item["content"].as_array().into_iter().flatten() {
            out.push_str(part["text"].as_str().unwrap_or(""));
        }
    }
    out
}

/// An old block at one level, with the blocks the exporter writes unindented after it
/// flattened into the same sequence.
struct Unit<'b> {
    block: &'b Value,
    span: &'b Span,
    /// The units this one is an unindented child of, outermost first.
    path: Vec<usize>,
    /// Its children are units of their own.
    flat: bool,
    /// The lines that are this unit's: the block's own for a flat unit, else the whole block.
    lines: Range<usize>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Fate {
    Removed,
    /// Writes no Markdown; stays before its next kept neighbor.
    Invisible,
    /// Unchanged or moved: kept as it is.
    Kept,
    /// Changed: merged with the new block of that index.
    Paired(usize),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Source {
    New,
    /// The first new block of a kept unit.
    Kept(usize),
    Paired(usize),
    /// Taken up by a unit (further lines of a kept unit, a caption).
    Used,
}

struct Context<'a> {
    old: Vec<&'a str>,
    new: Vec<&'a str>,
    old_to_new: Vec<Option<usize>>,
    resolver: &'a dyn LinkResolver,
}

static NO_SPAN: Span = Span {
    full: 0..0,
    head: 0..0,
    children: Vec::new(),
};

/// The span of block `index` (blocks without one never count as unchanged).
fn span_at(spans: &[Span], index: usize) -> &Span {
    spans.get(index).unwrap_or(&NO_SPAN)
}

fn flatten<'b>(blocks: &'b [Value], spans: &'b [Span], path: &[usize], units: &mut Vec<Unit<'b>>) {
    for (index, block) in blocks.iter().enumerate() {
        let span = span_at(spans, index);
        let flat = !nests_children(block_type(block)) && path.len() < MAX_BLOCK_DEPTH;
        let unit = units.len();
        units.push(Unit {
            block,
            span,
            path: path.to_vec(),
            flat,
            lines: if flat {
                span.head.clone()
            } else {
                span.full.clone()
            },
        });
        if flat {
            let mut inner = path.to_vec();
            inner.push(unit);
            flatten(
                block["children"].as_array().map_or(&[], Vec::as_slice),
                &span.children,
                &inner,
                units,
            );
        }
    }
}

impl Context<'_> {
    /// Merges one sibling group: `old` blocks with their spans in the old Markdown, `new`
    /// blocks with theirs in the new one.
    fn level(
        &self,
        old: &[Value],
        old_spans: &[Span],
        new: &[Value],
        new_spans: &[Span],
        depth: usize,
    ) -> Vec<Value> {
        if depth > MAX_BLOCK_DEPTH {
            return new.to_vec();
        }
        let mut units = Vec::new();
        flatten(old, old_spans, &[], &mut units);
        let new_span = |index: usize| span_at(new_spans, index);
        let mut fates: Vec<Fate> = units
            .iter()
            .map(|unit| {
                if unit.lines.is_empty() {
                    Fate::Invisible
                } else {
                    Fate::Removed
                }
            })
            .collect();
        let mut sources = vec![Source::New; new.len()];
        let mut captions: HashMap<usize, usize> = HashMap::new();

        // The new blocks that have lines, in order (siblings never share lines, except the
        // image blocks the importer splits out of one paragraph).
        let placed: Vec<usize> = (0..new.len())
            .filter(|&j| !new_span(j).full.is_empty())
            .collect();
        // Unchanged units: every line maps, in order, onto exactly the lines of new blocks.
        let mut anchors: Vec<(usize, Range<usize>)> = Vec::new();
        for (index, unit) in units.iter().enumerate() {
            if fates[index] == Fate::Invisible {
                continue;
            }
            let Some(target) = self.mapped(&unit.lines) else {
                continue;
            };
            let first = placed.partition_point(|&j| new_span(j).full.end <= target.start);
            let covering: Vec<usize> = placed[first..]
                .iter()
                .copied()
                .take_while(|&j| new_span(j).full.start < target.end)
                .collect();
            let tiles = !covering.is_empty()
                && covering.iter().all(|&j| {
                    let full = &new_span(j).full;
                    full.start >= target.start
                        && full.end <= target.end
                        && sources[j] == Source::New
                })
                && covering.iter().map(|&j| new_span(j).full.start).min() == Some(target.start)
                && covering.iter().map(|&j| new_span(j).full.end).max() == Some(target.end)
                && anchors
                    .last()
                    .is_none_or(|(_, previous)| previous.end <= covering[0]);
            if !tiles {
                continue;
            }
            fates[index] = Fate::Kept;
            sources[covering[0]] = Source::Kept(index);
            for &j in &covering[1..] {
                sources[j] = Source::Used;
            }
            anchors.push((index, covering[0]..covering[covering.len() - 1] + 1));
        }

        // Changed regions between unchanged units: pair blocks of the same class.
        let mut old_start = 0;
        let mut new_start = 0;
        for bound in anchors
            .iter()
            .map(|(unit, blocks)| Some((*unit, blocks.clone())))
            .chain([None])
        {
            let (old_end, new_end) = match &bound {
                Some((unit, blocks)) => (*unit, blocks.start),
                None => (units.len(), new.len()),
            };
            let gap_old: Vec<usize> = (old_start..old_end)
                .filter(|&u| fates[u] == Fate::Removed)
                .collect();
            let gap_new: Vec<usize> = (new_start..new_end)
                .filter(|&j| sources[j] == Source::New)
                .collect();
            self.pair(
                &units,
                &gap_old,
                new,
                &gap_new,
                new_spans,
                &mut fates,
                &mut sources,
                &mut captions,
            );
            if let Some((unit, blocks)) = bound {
                old_start = unit + 1;
                new_start = blocks.end;
            }
        }

        // Moves: a new block with the same Markdown as a removed one is that block.
        let mut removed: HashMap<(String, &str), Vec<usize>> = HashMap::new();
        for (index, unit) in units.iter().enumerate().rev() {
            if fates[index] == Fate::Removed {
                removed
                    .entry((normalized(&self.old, &unit.lines), class(unit.block)))
                    .or_default()
                    .push(index);
            }
        }
        for j in 0..new.len() {
            if sources[j] != Source::New {
                continue;
            }
            let key = (normalized(&self.new, &new_span(j).full), class(&new[j]));
            if let Some(index) = removed.get_mut(&key).and_then(Vec::pop) {
                fates[index] = Fate::Kept;
                sources[j] = Source::Kept(index);
            }
        }

        // Blocks without Markdown go before the next unit that stays.
        let mut before: HashMap<usize, Vec<usize>> = HashMap::new();
        let mut waiting = Vec::new();
        for (index, fate) in fates.iter().enumerate() {
            match fate {
                Fate::Invisible => waiting.push(index),
                Fate::Kept | Fate::Paired(_) => {
                    if !waiting.is_empty() {
                        before.insert(index, std::mem::take(&mut waiting));
                    }
                }
                Fate::Removed => {}
            }
        }

        // The new order, each item with the units it belongs under.
        let mut items: Vec<(Option<usize>, Value)> = Vec::new();
        let kept = |index: usize| {
            let unit = &units[index];
            let mut block = unit.block.clone();
            if unit.flat {
                block["children"] = json!([]);
            }
            block
        };
        for (j, source) in sources.iter().enumerate() {
            let unit = match source {
                Source::Kept(unit) | Source::Paired(unit) => *unit,
                Source::New => {
                    items.push((None, new[j].clone()));
                    continue;
                }
                Source::Used => continue,
            };
            for &invisible in before.get(&unit).into_iter().flatten() {
                items.push((Some(invisible), kept(invisible)));
            }
            let block = match source {
                Source::Paired(_) => self.paired(
                    &units[unit],
                    &new[j],
                    new_span(j),
                    captions.get(&j).map(|&caption| &new[caption]),
                    depth,
                ),
                _ => kept(unit),
            };
            items.push((Some(unit), block));
        }
        items.extend(waiting.into_iter().map(|index| (Some(index), kept(index))));
        nest(&units, items)
    }

    /// The new lines that `range` (old lines) became, when all of them stayed, in order.
    fn mapped(&self, range: &Range<usize>) -> Option<Range<usize>> {
        let first = (*self.old_to_new.get(range.start)?)?;
        for (offset, line) in range.clone().enumerate() {
            if *self.old_to_new.get(line)? != Some(first + offset) {
                return None;
            }
        }
        Some(first..first + range.len())
    }

    /// Pairs the removed units and the new blocks of one changed region by class, preferring
    /// pairs with the same text, then the same first lines.
    #[allow(clippy::too_many_arguments)]
    fn pair(
        &self,
        units: &[Unit<'_>],
        gap_old: &[usize],
        new: &[Value],
        gap_new: &[usize],
        new_spans: &[Span],
        fates: &mut [Fate],
        sources: &mut [Source],
        captions: &mut HashMap<usize, usize>,
    ) {
        if gap_old.is_empty() || gap_new.is_empty() {
            return;
        }
        // A caption paragraph under a file link belongs to the link (when a file has one).
        let has_captions = gap_old.iter().any(|&u| {
            matches!(block_type(units[u].block), "file" | "video" | "audio")
                && !units[u].block["props"]["caption"]
                    .as_str()
                    .unwrap_or("")
                    .is_empty()
        });
        let mut candidates = Vec::with_capacity(gap_new.len());
        let mut skip = None;
        for (position, &j) in gap_new.iter().enumerate() {
            if skip == Some(j) {
                continue;
            }
            candidates.push(j);
            if has_captions
                && only_link(&new[j]).is_some()
                && let Some(&next) = gap_new.get(position + 1)
                && next == j + 1
                && caption_text(&new[next]).is_some()
            {
                captions.insert(j, next);
                skip = Some(next);
            }
        }
        let new_span = |index: usize| span_at(new_spans, index);
        let old_keys: Vec<(&str, String, String)> = gap_old
            .iter()
            .map(|&u| {
                let unit = &units[u];
                (
                    class(unit.block),
                    normalized(&self.old, &unit.lines),
                    normalized(&self.old, &unit.span.head),
                )
            })
            .collect();
        let new_keys: Vec<(&str, String, String)> = candidates
            .iter()
            .map(|&j| {
                (
                    class(&new[j]),
                    normalized(&self.new, &new_span(j).full),
                    normalized(&self.new, &new_span(j).head),
                )
            })
            .collect();
        let weight = |a: usize, b: usize| {
            let (old, new) = (&old_keys[a], &new_keys[b]);
            if old.0 != new.0 {
                0
            } else if old.1 == new.1 {
                3
            } else if old.2 == new.2 {
                2
            } else {
                1
            }
        };
        let pairs = if gap_old.len() * candidates.len() <= MAX_PAIR_CELLS {
            weighted_lcs(gap_old.len(), candidates.len(), weight)
        } else {
            // Too large to compare every pair: pair in order, each with the next block of its
            // class among the following few.
            let mut pairs = Vec::new();
            let mut b = 0;
            for a in 0..gap_old.len() {
                let window = b..candidates.len().min(b + PAIR_WINDOW);
                if let Some(found) = window.into_iter().find(|&b| weight(a, b) > 0) {
                    pairs.push((a, found));
                    b = found + 1;
                }
            }
            pairs
        };
        for (a, b) in pairs {
            let (unit, j) = (gap_old[a], candidates[b]);
            fates[unit] = Fate::Paired(j);
            sources[j] = Source::Paired(unit);
        }
        // A caption whose link did not pair with a file is a paragraph of its own.
        captions.retain(|link, caption| {
            let file = matches!(sources[*link], Source::Paired(unit)
                if matches!(block_type(units[unit].block), "file" | "video" | "audio"));
            if file {
                sources[*caption] = Source::Used;
            }
            file
        });
    }

    /// An old block merged with the new block it became.
    fn paired(
        &self,
        unit: &Unit<'_>,
        new: &Value,
        new_span: &Span,
        caption: Option<&Value>,
        depth: usize,
    ) -> Value {
        let old = unit.block;
        let mut out = old.clone();
        let kind = block_type(old);
        let props = |name: &str| new["props"].get(name).cloned();
        match kind {
            "page" => {
                let href = self
                    .resolver
                    .page(old["props"]["pageId"].as_str().unwrap_or(""))
                    .map(|link| link.href);
                let same =
                    only_link(new).is_some_and(|link| link["href"].as_str() == href.as_deref());
                if !same {
                    // Another link: a paragraph now (the sub-page itself is not touched).
                    let mut block = new.clone();
                    block["id"] = old["id"].clone();
                    return block;
                }
            }
            "file" | "video" | "audio" => {
                if let Some(link) = only_link(new) {
                    let url = link["href"].as_str().unwrap_or("");
                    let label = plain_text(&link["content"]);
                    let old_url = old["props"]["url"].as_str().unwrap_or("");
                    let unnamed = old["props"]["name"].as_str().unwrap_or("").is_empty()
                        && (label == old_url.rsplit('/').next().unwrap_or("") || label == "File");
                    if url != self.resolver.url(old_url) {
                        out["props"]["url"] = json!(url);
                    }
                    if !unnamed {
                        out["props"]["name"] = json!(label);
                    }
                    out["props"]["caption"] =
                        json!(caption.and_then(caption_text).unwrap_or_default());
                } else {
                    let mut block = new.clone();
                    block["id"] = old["id"].clone();
                    return block;
                }
            }
            "image" => {
                let url = new["props"]["url"].as_str().unwrap_or("");
                let old_url = old["props"]["url"].as_str().unwrap_or("");
                if url != self.resolver.url(old_url) {
                    out["props"]["url"] = json!(url);
                }
                let name = new["props"]["name"].as_str().unwrap_or("");
                let caption = new["props"]["caption"].as_str().unwrap_or("");
                // The exporter writes the caption as alt text when there is no name.
                let unnamed = old["props"]["name"].as_str().unwrap_or("").is_empty()
                    && name == old["props"]["caption"].as_str().unwrap_or("");
                if !unnamed {
                    out["props"]["name"] = json!(name);
                }
                out["props"]["caption"] = json!(caption);
            }
            "codeBlock" => {
                let language = new["props"]["language"].as_str().unwrap_or("");
                let old_language = old["props"]["language"].as_str().unwrap_or("");
                // Plain text is written without a language.
                if !(language.is_empty() && (old_language.is_empty() || old_language == "text")) {
                    out["props"]["language"] = json!(language);
                }
                if plain_text(&old["content"]) != plain_text(&new["content"]) {
                    out["content"] = new["content"].clone();
                }
            }
            "table" => out["content"] = merge_table(&old["content"], &new["content"]),
            "divider" => {}
            _ => {
                if kind == "heading"
                    && let Some(level) = props("level")
                {
                    out["props"]["level"] = level;
                }
                if kind == "checkListItem"
                    && let Some(checked) = props("checked")
                {
                    out["props"]["checked"] = checked;
                }
                if kind == "numberedListItem"
                    && let Some(props) = out["props"].as_object_mut()
                {
                    match new["props"].get("start") {
                        Some(start) => props.insert("start".to_owned(), start.clone()),
                        None => props.remove("start"),
                    };
                }
                let mut content = new["content"].clone();
                if kind == "callout" {
                    strip_emoji(&mut content, old["props"]["emoji"].as_str().unwrap_or(""));
                }
                out["content"] = self.merge_inline(&old["content"], &content);
            }
        }
        if unit.flat {
            out["children"] = json!([]);
        } else {
            out["children"] = Value::Array(self.level(
                old["children"].as_array().map_or(&[], Vec::as_slice),
                &unit.span.children,
                new["children"].as_array().map_or(&[], Vec::as_slice),
                &new_span.children,
                depth + 1,
            ));
        }
        out
    }

    /// New inline content that keeps the old content's start and end where they read the
    /// same: their invisible styles (text colors) and their mentions stay.
    fn merge_inline(&self, old: &Value, new: &Value) -> Value {
        let old_tokens = tokens(old);
        let new_tokens = tokens(new);
        let names: Vec<(Vec<char>, Value)> = old_tokens
            .iter()
            .filter_map(|token| match token {
                Token::Mention(mention) => Some((
                    format!("@{}", self.mention_name(mention)).chars().collect(),
                    mention.clone(),
                )),
                _ => None,
            })
            .collect();
        let name_of = |mention: &Value| {
            names
                .iter()
                .find(|(_, other)| other == mention)
                .map(|(name, _)| name.as_slice())
        };
        // Whether the old token at `i` reads as `new` from `k` (forward) and how many it takes.
        let matches = |token: &Token, at: &[Token]| -> Option<usize> {
            match (token, at.first()?) {
                (Token::Char(a), Token::Char(b)) => a.reads_as(b).then_some(1),
                (Token::Mention(a), Token::Mention(b)) => {
                    (a["props"]["userId"] == b["props"]["userId"]).then_some(1)
                }
                (Token::Mention(mention), Token::Char(_)) => {
                    let name = name_of(mention)?;
                    (at.len() >= name.len()
                        && at[..name.len()].iter().zip(name).all(
                            |(token, c)| matches!(token, Token::Char(t) if t.c == *c && t.plain()),
                        ))
                    .then_some(name.len())
                }
                _ => None,
            }
        };
        let (mut i, mut k) = (0, 0);
        while i < old_tokens.len() {
            match matches(&old_tokens[i], &new_tokens[k..]) {
                Some(taken) => {
                    i += 1;
                    k += taken;
                }
                None => break,
            }
        }
        if i == old_tokens.len() && k == new_tokens.len() {
            return old.clone();
        }
        let (mut end_old, mut end_new) = (old_tokens.len(), new_tokens.len());
        while end_old > i && end_new > k {
            let token = &old_tokens[end_old - 1];
            let taken = match token {
                Token::Mention(mention) => name_of(mention).map_or(1, <[char]>::len),
                _ => 1,
            };
            if end_new < k + taken {
                break;
            }
            match matches(token, &new_tokens[end_new - taken..end_new]) {
                Some(found) if found == taken => {
                    end_old -= 1;
                    end_new -= taken;
                }
                _ => break,
            }
        }
        let mut merged: Vec<Token> = old_tokens[..i].to_vec();
        merged.extend(reuse_mentions(&new_tokens[k..end_new], &names));
        merged.extend_from_slice(&old_tokens[end_old..]);
        content_of(&merged)
    }

    fn mention_name(&self, mention: &Value) -> String {
        let props = &mention["props"];
        self.resolver
            .member_name(props["userId"].as_str().unwrap_or(""))
            .or_else(|| props["name"].as_str().map(str::to_owned))
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| "Unknown user".to_owned())
    }
}

/// Puts each item under the units it belongs under, as far as they come before it in the new
/// order; items of no unit go where the next unit item goes (or as deep as both neighbors).
fn nest(units: &[Unit<'_>], items: Vec<(Option<usize>, Value)>) -> Vec<Value> {
    let mut next_path: Vec<&[usize]> = vec![&[]; items.len()];
    let mut following: &[usize] = &[];
    for (index, (unit, _)) in items.iter().enumerate().rev() {
        next_path[index] = following;
        if let Some(unit) = unit {
            following = &units[*unit].path;
        }
    }
    let mut root: Vec<Value> = Vec::new();
    let mut open: Vec<(usize, Value)> = Vec::new();
    let close = |open: &mut Vec<(usize, Value)>, root: &mut Vec<Value>| {
        if let Some((_, block)) = open.pop() {
            match open.last_mut() {
                Some((_, parent)) => push_child(parent, block),
                None => root.push(block),
            }
        }
    };
    for (index, (unit, block)) in items.into_iter().enumerate() {
        let desired: Vec<usize> = match unit {
            Some(unit) => units[unit].path.clone(),
            None => open
                .iter()
                .map(|(unit, _)| *unit)
                .zip(next_path[index])
                .take_while(|(a, b)| a == *b)
                .map(|(a, _)| a)
                .collect(),
        };
        while !desired.starts_with(&open.iter().map(|(unit, _)| *unit).collect::<Vec<_>>()) {
            close(&mut open, &mut root);
        }
        match unit.filter(|unit| units[*unit].flat) {
            Some(unit) => open.push((unit, block)),
            None => match open.last_mut() {
                Some((_, parent)) => push_child(parent, block),
                None => root.push(block),
            },
        }
    }
    while !open.is_empty() {
        close(&mut open, &mut root);
    }
    root
}

fn push_child(parent: &mut Value, child: Value) {
    match parent["children"].as_array_mut() {
        Some(children) => children.push(child),
        None => parent["children"] = json!([child]),
    }
}

/// Drops a callout's emoji from the start of its new text (the exporter writes it there).
fn strip_emoji(content: &mut Value, emoji: &str) {
    if emoji.is_empty() {
        return;
    }
    let Some(first) = content
        .as_array_mut()
        .and_then(|items| items.first_mut())
        .filter(|first| first["type"] == "text")
    else {
        return;
    };
    let text = first["text"].as_str().unwrap_or("");
    let rest = text
        .strip_prefix(emoji)
        .map(|rest| rest.strip_prefix(' ').unwrap_or(rest).to_owned());
    if let Some(rest) = rest {
        if rest.is_empty() {
            content.as_array_mut().map(|items| items.remove(0));
        } else {
            first["text"] = Value::String(rest);
        }
    }
}

/// A table's new content; while the table keeps its shape (a cell spanning columns is written
/// as the cell and empty cells), cells keep their props (colors, alignment, spans) and the
/// table its column widths and header settings.
fn merge_table(old: &Value, new: &Value) -> Value {
    let rows = |table: &Value| table["rows"].as_array().cloned().unwrap_or_default();
    let (old_rows, new_rows) = (rows(old), rows(new));
    if old_rows.len() != new_rows.len() {
        return new.clone();
    }
    let visible = |content: &Value| {
        tokens(content)
            .into_iter()
            .map(|token| match token {
                Token::Char(c) => format!("{}{:?}{:?}", c.c, c.visible(), c.href),
                Token::Mention(m) => m["props"]["userId"].to_string(),
                Token::Other(other) => other.to_string(),
            })
            .collect::<Vec<_>>()
    };
    let mut out = old.clone();
    for (row, new_row) in out["rows"]
        .as_array_mut()
        .into_iter()
        .flatten()
        .zip(&new_rows)
    {
        let new_cells = new_row["cells"].as_array().map_or(&[][..], Vec::as_slice);
        let mut at = 0;
        for cell in row["cells"].as_array_mut().into_iter().flatten() {
            if !cell.is_object() {
                return new.clone();
            }
            let span = cell["props"]["colspan"].as_u64().unwrap_or(1).clamp(1, 64) as usize;
            let Some(slots) = new_cells.get(at..at + span) else {
                return new.clone();
            };
            if slots[1..]
                .iter()
                .any(|slot| !visible(&slot["content"]).is_empty())
            {
                return new.clone();
            }
            if visible(&cell["content"]) != visible(&slots[0]["content"]) {
                cell["content"] = slots[0]["content"].clone();
            }
            at += span;
        }
        if at != new_cells.len() {
            return new.clone();
        }
    }
    out
}

/// One character of inline content with its styles and link.
#[derive(Clone, Debug, PartialEq)]
struct Char {
    c: char,
    styles: Map<String, Value>,
    href: Option<String>,
}

const VISIBLE_STYLES: [&str; 5] = ["bold", "italic", "underline", "strike", "code"];

impl Char {
    /// The styles Markdown shows.
    fn visible(&self) -> [bool; 5] {
        VISIBLE_STYLES.map(|name| self.styles.get(name) == Some(&Value::Bool(true)))
    }

    fn plain(&self) -> bool {
        self.href.is_none() && self.visible() == [false; 5]
    }

    /// Whether the Markdown of `self` reads as `other` (styles of spaces do not show).
    fn reads_as(&self, other: &Self) -> bool {
        self.c == other.c
            && self.href == other.href
            && (self.visible() == other.visible()
                || (self.c.is_whitespace() && self.visible()[4] == other.visible()[4]))
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Char(Char),
    Mention(Value),
    /// Inline content the converter does not know; kept as it is.
    Other(Value),
}

fn styles_of(item: &Value) -> Map<String, Value> {
    item["styles"].as_object().cloned().unwrap_or_default()
}

fn push_chars(out: &mut Vec<Token>, text: &str, styles: &Map<String, Value>, href: Option<&str>) {
    out.extend(text.chars().map(|c| {
        Token::Char(Char {
            c,
            styles: styles.clone(),
            href: href.map(str::to_owned),
        })
    }));
}

fn tokens(content: &Value) -> Vec<Token> {
    let mut out = Vec::new();
    match content {
        Value::String(text) => push_chars(&mut out, text, &Map::new(), None),
        Value::Array(items) => {
            for item in items {
                match item {
                    Value::String(text) => push_chars(&mut out, text, &Map::new(), None),
                    item if item["type"] == "text" => {
                        let text = item["text"].as_str().unwrap_or("");
                        push_chars(&mut out, text, &styles_of(item), None);
                    }
                    item if item["type"] == "link" => {
                        let href = item["href"].as_str();
                        match &item["content"] {
                            Value::String(text) => push_chars(&mut out, text, &Map::new(), href),
                            parts => {
                                for part in parts.as_array().into_iter().flatten() {
                                    let text =
                                        part["text"].as_str().or(part.as_str()).unwrap_or("");
                                    push_chars(&mut out, text, &styles_of(part), href);
                                }
                            }
                        }
                    }
                    item if item["type"] == "mention" => out.push(Token::Mention(item.clone())),
                    item => out.push(Token::Other(item.clone())),
                }
            }
        }
        _ => {}
    }
    out
}

/// Turns `@Name` text that names one of the old mentions back into that mention.
fn reuse_mentions(tokens: &[Token], names: &[(Vec<char>, Value)]) -> Vec<Token> {
    let mut sorted: Vec<&(Vec<char>, Value)> = names.iter().collect();
    sorted.sort_by_key(|(name, _)| std::cmp::Reverse(name.len()));
    let mut out = Vec::with_capacity(tokens.len());
    let mut index = 0;
    'scan: while index < tokens.len() {
        let boundary = out
            .last()
            .is_none_or(|token| !matches!(token, Token::Char(c) if c.c.is_alphanumeric()));
        if boundary {
            for (name, mention) in &sorted {
                let end = index + name.len();
                let fits = end <= tokens.len()
                    && tokens[index..end].iter().zip(name.iter()).all(
                        |(token, c)| matches!(token, Token::Char(t) if t.c == *c && t.plain()),
                    )
                    && !matches!(tokens.get(end), Some(Token::Char(c)) if c.c.is_alphanumeric());
                if fits {
                    out.push(Token::Mention(mention.clone()));
                    index = end;
                    continue 'scan;
                }
            }
        }
        out.push(tokens[index].clone());
        index += 1;
    }
    out
}

/// Inline content of tokens: runs of equal styles, links grouped, atoms between.
fn content_of(tokens: &[Token]) -> Value {
    let mut out: Vec<Value> = Vec::new();
    for token in tokens {
        match token {
            Token::Char(c) => {
                let run = json!({"type": "text", "text": c.c.to_string(), "styles": c.styles});
                let last = out.last_mut();
                match (&c.href, last) {
                    (None, Some(last))
                        if last["type"] == "text" && last["styles"] == run["styles"] =>
                    {
                        append(last, c.c);
                    }
                    (None, _) => out.push(run),
                    (Some(href), Some(last))
                        if last["type"] == "link" && last["href"].as_str() == Some(href) =>
                    {
                        let parts = last["content"].as_array_mut().expect("link parts");
                        match parts.last_mut() {
                            Some(part) if part["styles"] == run["styles"] => append(part, c.c),
                            _ => parts.push(run),
                        }
                    }
                    (Some(href), _) => {
                        out.push(json!({"type": "link", "href": href, "content": [run]}));
                    }
                }
            }
            Token::Mention(value) | Token::Other(value) => out.push(value.clone()),
        }
    }
    Value::Array(out)
}

fn append(run: &mut Value, c: char) {
    let mut text = run["text"].as_str().unwrap_or("").to_owned();
    text.push(c);
    run["text"] = Value::String(text);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_diff_keeps_unique_and_common_lines() {
        let a = ["a", "b", "", "c", "d", ""];
        let b = ["a", "x", "", "c", "d", "", "e"];
        let map = diff(&a, &b);
        assert_eq!(map, vec![Some(0), None, Some(2), Some(3), Some(4), Some(5)]);
        assert_eq!(diff(&[], &["a"]), Vec::<Option<usize>>::new());
        assert_eq!(diff(&["a", "a"], &["a"]), vec![Some(0), None]);
    }
}
