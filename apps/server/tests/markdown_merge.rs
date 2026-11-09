//! Markdown writes onto an existing document (`collab::markdown_merge`): what the Markdown does
//! not change stays exactly as it was, and changed blocks keep what Markdown cannot show.

use std::path::Path;
use std::time::{Duration, Instant};

use orbit_server::collab::markdown_merge::merge;
use orbit_server::export::markdown::{KeepLinks, blocks_to_markdown};
use serde_json::{Value, json};

fn fixture(name: &str) -> Vec<Value> {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fixtures/collab/{name}.json"));
    let golden: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    golden["expected"].as_array().unwrap().clone()
}

fn exercise() -> Vec<Value> {
    fixture("editor_exercise")
}

fn md(blocks: &[Value]) -> String {
    blocks_to_markdown(blocks, &KeepLinks)
}

/// `markdown` with `from` replaced by `to` (exactly once).
fn edit(markdown: &str, from: &str, to: &str) -> String {
    assert_eq!(markdown.matches(from).count(), 1, "{from:?} in {markdown}");
    markdown.replacen(from, to, 1)
}

fn find<'a>(blocks: &'a [Value], id: &str) -> Option<&'a Value> {
    let mut stack: Vec<&Value> = blocks.iter().collect();
    while let Some(block) = stack.pop() {
        if block["id"] == id {
            return Some(block);
        }
        stack.extend(block["children"].as_array().into_iter().flatten());
    }
    None
}

/// The ids of every block, depth first, with their depth.
fn outline(blocks: &[Value]) -> Vec<(String, usize)> {
    let mut out = Vec::new();
    let mut stack: Vec<(&Value, usize)> = blocks.iter().rev().map(|block| (block, 0)).collect();
    while let Some((block, depth)) = stack.pop() {
        out.push((block["id"].as_str().unwrap_or("").to_owned(), depth));
        let children = block["children"].as_array().into_iter().flatten().rev();
        stack.extend(children.map(|child| (child, depth + 1)));
    }
    out
}

/// Asserts that every block of `base` except `changed` is in `merged`, equal to before.
fn others_unchanged(base: &[Value], merged: &[Value], changed: &[&str]) {
    let mut stack: Vec<&Value> = base.iter().collect();
    while let Some(block) = stack.pop() {
        let id = block["id"].as_str().unwrap();
        stack.extend(block["children"].as_array().into_iter().flatten());
        if changed.contains(&id) {
            continue;
        }
        let found = find(merged, id).unwrap_or_else(|| panic!("{id} is gone: {merged:#?}"));
        let strip = |block: &Value| {
            let mut block = block.clone();
            block["children"] = json!(outline(block["children"].as_array().unwrap()));
            block
        };
        assert_eq!(strip(found), strip(block), "{id} changed");
    }
}

#[test]
fn the_markdown_of_a_document_writes_back_the_same_document() {
    for name in [
        "editor_exercise",
        "editor_mentions",
        "notion_lists",
        "notion_nested_page",
        "notion_table",
        "notion_media",
        "notion_callout",
    ] {
        let base = fixture(name);
        let markdown = md(&base);
        assert_eq!(merge(&base, &markdown, &[], &KeepLinks), base, "{name}");
        // Through the full comparison too: one paragraph more at the end, the rest unchanged.
        let merged = merge(
            &base,
            &format!("{markdown}\nAdded at the end\n"),
            &[],
            &KeepLinks,
        );
        let added: Vec<&Value> = merged
            .iter()
            .filter(|block| block["content"][0]["text"] == "Added at the end")
            .collect();
        assert_eq!(added.len(), 1, "{name}: {merged:#?}");
        let without: Vec<Value> = merged
            .iter()
            .filter(|block| block["content"][0]["text"] != "Added at the end")
            .cloned()
            .collect();
        assert_eq!(without, base, "{name}");
    }
}

#[test]
fn editing_one_paragraph_keeps_its_id_props_and_untouched_styles() {
    let base = exercise();
    let markdown = edit(&md(&base), "second line", "2nd line");
    let merged = merge(&base, &markdown, &[], &KeepLinks);
    others_unchanged(&base, &merged, &["ex-p1"]);
    let paragraph = find(&merged, "ex-p1").unwrap();
    let old = find(&base, "ex-p1").unwrap();
    assert_eq!(paragraph["props"], old["props"]);
    let content = paragraph["content"].as_array().unwrap();
    // The colored runs before the edit and the links after it are the old ones.
    assert_eq!(content[..6], old["content"].as_array().unwrap()[..6]);
    assert_eq!(content[4]["styles"], json!({"textColor": "red"}));
    let text: String = content
        .iter()
        .filter_map(|item| item["text"].as_str())
        .collect();
    assert!(text.contains("2nd line 🙂 ünïcode"), "{content:#?}");
    assert_eq!(
        content[content.len() - 4..],
        old["content"].as_array().unwrap()[old["content"].as_array().unwrap().len() - 4..]
    );
}

#[test]
fn inserted_and_deleted_blocks_leave_their_neighbors_alone() {
    let base = exercise();
    let markdown = edit(
        &md(&base),
        "## Toggle heading\n",
        "A new paragraph\n\n## Toggle heading\n",
    );
    let markdown = edit(&markdown, "> *Quoted*\n\n", "");
    let merged = merge(&base, &markdown, &[], &KeepLinks);
    others_unchanged(&base, &merged, &["ex-quote"]);
    assert!(find(&merged, "ex-quote").is_none());
    let ids: Vec<&str> = merged
        .iter()
        .map(|block| block["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids[0], "ex-h1");
    assert_eq!(ids[2], "ex-h2", "{ids:?}");
    assert_eq!(merged[1]["content"][0]["text"], "A new paragraph");
    // The new paragraph is no child of the heading before it.
    assert_eq!(merged[0]["children"], json!([]));
}

#[test]
fn edited_children_of_toggles_and_callouts_stay_where_they_are() {
    let base = exercise();
    let markdown = edit(&md(&base), "inside", "inside, edited");
    let markdown = edit(&markdown, "  hidden", "  shown");
    let markdown = edit(&markdown, "> - nested", "> - nested and changed");
    let markdown = edit(&markdown, "Callout **text**", "Callout **changed**");
    let merged = merge(&base, &markdown, &[], &KeepLinks);
    others_unchanged(
        &base,
        &merged,
        &["ex-h2-c", "ex-toggle-c", "ex-callout", "ex-callout-c"],
    );
    assert_eq!(outline(&merged), outline(&base));
    let heading = find(&merged, "ex-h2").unwrap();
    assert_eq!(heading["props"]["isToggleable"], true);
    assert_eq!(
        heading["children"][0]["content"][0]["text"],
        "inside, edited"
    );
    let toggle = find(&merged, "ex-toggle").unwrap();
    assert_eq!(toggle["type"], "toggleListItem");
    assert_eq!(toggle["children"][0]["content"][0]["text"], "shown");
    let callout = find(&merged, "ex-callout").unwrap();
    assert_eq!(callout["type"], "callout");
    assert_eq!(callout["props"]["emoji"], "🚀");
    assert_eq!(callout["props"]["backgroundColor"], "purple");
    assert_eq!(
        callout["content"],
        json!([{"type": "text", "text": "Callout ", "styles": {}}, {"type": "text", "text": "changed", "styles": {"bold": true}}])
    );
    assert_eq!(
        callout["children"][0]["content"][0]["text"],
        "nested and changed"
    );
}

#[test]
fn visible_props_change_and_invisible_ones_stay() {
    let base = exercise();
    let markdown = edit(
        &md(&base),
        "  3. Numbered from three\n  4. Next",
        "  5. Numbered from three\n  6. Next",
    );
    let markdown = edit(&markdown, "## Toggle heading", "### Toggle heading");
    let markdown = edit(&markdown, "- [x] Done", "- [ ] Done");
    let markdown = edit(&markdown, "*Caption*", "*New caption*");
    let markdown = edit(&markdown, "```rust", "```python");
    let markdown = edit(&markdown, "| Side |", "| Edge |");
    let merged = merge(&base, &markdown, &[], &KeepLinks);
    assert_eq!(outline(&merged), outline(&base));
    others_unchanged(
        &base,
        &merged,
        &[
            "ex-b1-n", "ex-h2", "ex-b1-c", "ex-image", "ex-code", "ex-table",
        ],
    );
    assert_eq!(find(&merged, "ex-b1-n").unwrap()["props"]["start"], 5);
    let heading = find(&merged, "ex-h2").unwrap();
    assert_eq!(heading["props"]["level"], 3);
    assert_eq!(heading["props"]["textColor"], "blue");
    assert_eq!(heading["props"]["isToggleable"], true);
    assert_eq!(find(&merged, "ex-b1-c").unwrap()["props"]["checked"], false);
    let image = find(&merged, "ex-image").unwrap();
    assert_eq!(image["props"]["caption"], "New caption");
    assert_eq!(image["props"]["previewWidth"], 320);
    assert_eq!(image["props"]["textAlignment"], "center");
    assert_eq!(image["props"]["name"], "a.png");
    assert_eq!(
        find(&merged, "ex-code").unwrap()["props"]["language"],
        "python"
    );
    let table = find(&merged, "ex-table").unwrap();
    let old_table = find(&base, "ex-table").unwrap();
    assert_eq!(
        table["content"]["columnWidths"],
        old_table["content"]["columnWidths"]
    );
    assert_eq!(
        table["content"]["rows"][1]["cells"][0]["content"][0]["text"],
        "Edge"
    );
    assert_eq!(table["content"]["rows"][0], old_table["content"]["rows"][0]);
    assert_eq!(
        table["content"]["rows"][1]["cells"][2],
        old_table["content"]["rows"][1]["cells"][2]
    );
}

#[test]
fn moved_blocks_keep_their_id_and_sub_pages_stay_sub_pages() {
    let base = exercise();
    let markdown = edit(&md(&base), "> *Quoted*\n\n", "");
    let markdown = format!("{markdown}\n> *Quoted*\n");
    // The page link's label is the sub-page's title; another label is still that sub-page.
    let page = "/docs/0199a0b0-0000-7000-8000-000000000001)";
    let markdown = edit(
        &markdown,
        &format!("[Untitled]({page}"),
        &format!("[Renamed]({page}"),
    );
    let merged = merge(&base, &markdown, &[], &KeepLinks);
    others_unchanged(&base, &merged, &[]);
    assert_eq!(merged.last().unwrap()["id"], "ex-quote");
    assert_eq!(find(&merged, "ex-page").unwrap()["type"], "page");
    let database = find(&merged, "ex-database-link").unwrap();
    assert_eq!(
        (database["type"].as_str(), &database["props"]),
        (Some("database"), &find(&base, "ex-database-link").unwrap()["props"])
    );
}

#[test]
fn mentions_stay_mentions_next_to_edits() {
    let base = fixture("editor_mentions");
    let markdown = edit(
        &md(&base),
        "@Bob Stone@Ann Lee",
        "@Bob Stone and @Ann Lee, edited",
    );
    let merged = merge(&base, &markdown, &[], &KeepLinks);
    let edited = merged
        .iter()
        .find(|block| {
            block["content"]
                .as_array()
                .is_some_and(|c| c.len() >= 3 && c[1]["text"] == " and ")
        })
        .unwrap_or_else(|| panic!("{merged:#?}"));
    let old = base
        .iter()
        .find(|block| block["id"] == edited["id"])
        .unwrap();
    assert_eq!(edited["content"][0], old["content"][0]);
    assert_eq!(edited["content"][2], old["content"][1]);
    assert_eq!(edited["content"][0]["type"], "mention");
    assert_eq!(edited["content"][2]["type"], "mention");
}

/// How deep `blocks` nest (1 = top level only).
fn nesting(blocks: &[Value]) -> usize {
    outline(blocks)
        .iter()
        .map(|(_, depth)| depth + 1)
        .max()
        .unwrap_or(0)
}

#[test]
fn deep_and_large_documents_merge_quickly_without_recursing_deeply() {
    let budget = Duration::from_secs(if cfg!(debug_assertions) { 10 } else { 2 });
    // A list 60 levels deep: written, then written again with an edit at the bottom.
    let deep: String = (0..60)
        .map(|level| format!("{}- level {level}\n", "  ".repeat(level)))
        .collect();
    let started = Instant::now();
    let first = merge(&[], &deep, &[], &KeepLinks);
    assert!(nesting(&first) <= orbit_server::collab::blocknote::MAX_BLOCK_DEPTH + 1);
    let again = merge(&first, &md(&first), &[], &KeepLinks);
    assert_eq!(again, first);
    let edited = merge(
        &first,
        &md(&first).replace("level 59", "level fifty-nine"),
        &[],
        &KeepLinks,
    );
    assert_eq!(outline(&edited)[..40], outline(&first)[..40]);

    // 200 KB, then all of it rewritten, then one line of it edited.
    let paragraph = |index: usize, word: &str| format!("Paragraph {index} {}", word.repeat(18));
    let base_markdown: String = (0..2000)
        .map(|index| paragraph(index, "lorem ") + "\n\n")
        .collect();
    assert!(base_markdown.len() > 200_000);
    let base = merge(&[], &base_markdown, &[], &KeepLinks);
    let rewritten: String = (0..2000)
        .map(|index| paragraph(index + 7, "ipsum ") + "\n\n")
        .collect();
    let merged = merge(&base, &rewritten, &[], &KeepLinks);
    assert_eq!(merged.len(), 2000);
    // Every block is paired with one in the same place.
    assert_eq!(merged[10]["id"], base[10]["id"]);
    let one = merge(
        &base,
        &base_markdown.replace("Paragraph 1000 ", "Paragraph one thousand "),
        &[],
        &KeepLinks,
    );
    assert_eq!(outline(&one), outline(&base));
    assert_eq!(one[1001], base[1001]);

    // 200 KB of one-line list items: rewritten as other items, as paragraphs, then one edit.
    let items = |word: &str| -> String {
        (0..14_000)
            .map(|index| format!("- {word} {index}\n"))
            .collect()
    };
    let base_markdown = items("item");
    assert!(base_markdown.len() > 150_000, "{}", base_markdown.len());
    let base = merge(&[], &base_markdown, &[], &KeepLinks);
    let rewritten = merge(&base, &items("entry"), &[], &KeepLinks);
    assert_eq!(rewritten[5]["id"], base[5]["id"]);
    let paragraphs = merge(&base, &base_markdown.replace("- ", "\n"), &[], &KeepLinks);
    assert_eq!(paragraphs[0]["type"], "paragraph");
    let one = merge(
        &base,
        &base_markdown.replacen("item 7000\n", "item seven thousand\n", 1),
        &[],
        &KeepLinks,
    );
    assert_eq!(outline(&one), outline(&base));
    // 100,000 nested quotes: read flat below the depth limit, on both sides of a merge.
    let quotes = format!("{} deep", "> ".repeat(100_000));
    let first = merge(&[], &quotes, &[], &KeepLinks);
    let again = merge(&first, &quotes.replace("deep", "deeper"), &[], &KeepLinks);
    assert!(nesting(&again) <= orbit_server::collab::blocknote::MAX_BLOCK_DEPTH + 1);
    let elapsed = started.elapsed();
    eprintln!("deep + 200 KB merges: {elapsed:?}");
    assert!(elapsed < budget, "{elapsed:?}");
}
