//! Markdown -> BlockNote import (old task descriptions, REST markdown writes, GitHub sync).

use orbit_platform::Id;
use orbit_server::collab::markdown_import::markdown_to_blocks;
use orbit_server::export::markdown::{KeepLinks, blocks_to_markdown};
use serde_json::{Value, json};

fn import(markdown: &str) -> Vec<Value> {
    markdown_to_blocks(markdown, &[])
}

fn round_trip(markdown: &str) -> String {
    blocks_to_markdown(&import(markdown), &KeepLinks)
}

fn texts(block: &Value) -> Vec<(String, Value)> {
    block["content"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| match item["type"].as_str().unwrap() {
            "link" => (
                format!("link:{}", item["href"].as_str().unwrap()),
                item["content"][0]["text"].clone(),
            ),
            kind => (
                item["text"]
                    .as_str()
                    .map_or_else(|| kind.to_owned(), str::to_owned),
                item["styles"].clone(),
            ),
        })
        .collect()
}

#[test]
fn paragraph_with_marks() {
    let blocks = import("**b** *i* ~~s~~ `code` [l](https://x.dev)");
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0]["type"], "paragraph");
    let parts = texts(&blocks[0]);
    assert!(
        parts.contains(&("b".into(), json!({"bold": true}))),
        "{parts:?}"
    );
    assert!(
        parts.contains(&("i".into(), json!({"italic": true}))),
        "{parts:?}"
    );
    assert!(
        parts.contains(&("s".into(), json!({"strike": true}))),
        "{parts:?}"
    );
    assert!(
        parts.contains(&("code".into(), json!({"code": true}))),
        "{parts:?}"
    );
    assert!(
        parts.contains(&("link:https://x.dev".into(), json!("l"))),
        "{parts:?}"
    );
    assert_eq!(
        round_trip("**b** *i* ~~s~~ `code` [l](https://x.dev)"),
        "**b** *i* ~~s~~ `code` [l](https://x.dev)\n"
    );
}

#[test]
fn headings_keep_their_level() {
    let blocks = import("# One\n\n#### Four");
    assert_eq!(blocks[0]["type"], "heading");
    assert_eq!(blocks[0]["props"]["level"], 1);
    assert_eq!(blocks[1]["props"]["level"], 4);
    assert_eq!(blocks[1]["content"][0]["text"], "Four");
}

#[test]
fn nested_lists_and_checks() {
    let blocks = import("- a\n  - b\n\n1. one\n\n- [x] done\n- [ ] open");
    assert_eq!(blocks[0]["type"], "bulletListItem");
    assert_eq!(blocks[0]["content"][0]["text"], "a");
    assert_eq!(blocks[0]["children"][0]["type"], "bulletListItem");
    assert_eq!(blocks[0]["children"][0]["content"][0]["text"], "b");
    assert_eq!(blocks[1]["type"], "numberedListItem");
    assert_eq!(blocks[2]["type"], "checkListItem");
    assert_eq!(blocks[2]["props"]["checked"], true);
    assert_eq!(blocks[2]["content"][0]["text"], "done");
    assert_eq!(blocks[3]["props"]["checked"], false);
}

#[test]
fn quote_code_mermaid() {
    let blocks = import("> q\n\n```rust\nfn a() {}\n```\n\n```mermaid\ngraph TD\n```");
    assert_eq!(blocks[0]["type"], "quote");
    assert_eq!(blocks[0]["content"][0]["text"], "q");
    assert_eq!(blocks[1]["type"], "codeBlock");
    assert_eq!(blocks[1]["props"]["language"], "rust");
    assert_eq!(blocks[1]["content"][0]["text"], "fn a() {}");
    assert_eq!(blocks[2]["props"]["language"], "mermaid");
    assert_eq!(blocks[2]["content"][0]["text"], "graph TD");
}

#[test]
fn gfm_table() {
    let blocks = import("| a | b |\n| - | - |\n| 1 | 2 |");
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0]["type"], "table");
    let rows = blocks[0]["content"]["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["cells"][1]["content"][0]["text"], "b");
    assert_eq!(rows[1]["cells"][0]["content"][0]["text"], "1");
}

#[test]
fn image_block() {
    let blocks = import("![alt](https://x.dev/a.png)");
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0]["type"], "image");
    assert_eq!(blocks[0]["props"]["url"], "https://x.dev/a.png");
    assert_eq!(blocks[0]["props"]["name"], "alt");
    assert_eq!(blocks[0]["props"]["caption"], "");
}

#[test]
fn an_italic_paragraph_after_an_image_is_its_caption() {
    let blocks = import("![Chart](https://x.dev/a.png)\n\n*Sales by month*\n\n*not a caption*");
    assert_eq!(blocks.len(), 2, "{blocks:#?}");
    assert_eq!(blocks[0]["props"]["name"], "Chart");
    assert_eq!(blocks[0]["props"]["caption"], "Sales by month");
    assert_eq!(blocks[1]["type"], "paragraph");
    // Italic text with other styles is no caption.
    let blocks = import("![a](https://x.dev/a.png)\n\n***bold too***");
    assert_eq!(blocks.len(), 2);
    assert_eq!(blocks[0]["props"]["caption"], "");
}

#[test]
fn underline_tags_become_underline_and_br_a_line_break() {
    let blocks = import("a <u>under *both*</u> b");
    assert_eq!(
        texts(&blocks[0]),
        vec![
            ("a ".to_owned(), json!({})),
            ("under ".to_owned(), json!({"underline": true})),
            (
                "both".to_owned(),
                json!({"underline": true, "italic": true})
            ),
            (" b".to_owned(), json!({})),
        ]
    );
    let blocks = import("| a<br>b |\n| - |\n| c |");
    assert_eq!(
        blocks[0]["content"]["rows"][0]["cells"][0]["content"][0]["text"],
        "a\nb"
    );
    // Escaped tags stay text.
    assert_eq!(import("\\<u>x")[0]["content"][0]["text"], "<u>x");
}

#[test]
fn numbered_lists_keep_their_start_on_the_first_item() {
    let blocks = import("5. five\n6. six");
    assert_eq!(blocks[0]["props"]["start"], 5);
    assert!(blocks[1]["props"].get("start").is_none(), "{blocks:#?}");
    let blocks = import("1. one\n2. two");
    assert!(blocks[0]["props"].get("start").is_none());
}

#[test]
fn a_quote_s_first_paragraph_is_its_text_and_the_rest_its_children() {
    let blocks = import("> head\n>\n> child\n>\n> - item");
    assert_eq!(blocks.len(), 1, "{blocks:#?}");
    assert_eq!(blocks[0]["type"], "quote");
    assert_eq!(blocks[0]["content"][0]["text"], "head");
    let children = blocks[0]["children"].as_array().unwrap();
    assert_eq!(children.len(), 2);
    assert_eq!(children[0]["type"], "paragraph");
    assert_eq!(children[1]["type"], "bulletListItem");
}

#[test]
fn spans_name_the_lines_of_each_block() {
    let (blocks, spans) = orbit_server::collab::markdown_import::markdown_to_blocks_with_spans(
        "# Title\n\ntext\nmore\n\n- a\n  - b\n\n- c\n\n> q\n>\n> r\n",
        &[],
    );
    assert_eq!(blocks.len(), 5, "{blocks:#?}");
    let lines: Vec<_> = spans.iter().map(|span| span.full.clone()).collect();
    assert_eq!(lines, vec![0..1, 2..4, 5..7, 8..9, 10..13]);
    assert_eq!(spans[2].head, 5..6);
    assert_eq!(spans[2].children[0].full, 6..7);
    assert_eq!(spans[4].head, 10..11);
    assert_eq!(spans[4].children[0].full, 12..13);
}

/// What the exporter writes reads back as the same Markdown (the fixtures hold every block type).
#[test]
fn exported_markdown_reads_back_to_the_same_markdown() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/collab");
    let mut checked = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|extension| extension != "json") {
            continue;
        }
        let golden: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let Some(blocks) = golden["expected"].as_array() else {
            continue;
        };
        let markdown = blocks_to_markdown(blocks, &KeepLinks);
        let expected = if path.ends_with("editor_mentions.json") {
            // Known limit: bold text that starts with punctuation right after a word cannot open
            // emphasis in CommonMark, so it reads back as literal asterisks.
            markdown.replace("**, see**", "\\*\\*, see\\*\\*")
        } else {
            markdown.clone()
        };
        assert_eq!(round_trip(&markdown), expected, "{}", path.display());
        checked += 1;
    }
    assert!(checked > 5);
}

/// Every text in the blocks, nested ones included.
fn all_texts(blocks: &[Value]) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack: Vec<&Value> = blocks.iter().collect();
    while let Some(block) = stack.pop() {
        for item in block["content"].as_array().into_iter().flatten() {
            if let Some(text) = item["text"].as_str() {
                out.push(text.to_owned());
            }
        }
        stack.extend(block["children"].as_array().into_iter().flatten());
    }
    out
}

#[test]
fn exact_mention() {
    let ada = Id::new_v7();
    let blocks = markdown_to_blocks(
        "hi @Ada Lovelace, see this",
        &[
            (ada, "Ada Lovelace".to_owned()),
            (Id::new_v7(), "Ada".to_owned()),
        ],
    );
    let content = blocks[0]["content"].as_array().unwrap();
    assert_eq!(content[0]["text"], "hi ");
    assert_eq!(content[1]["type"], "mention");
    assert_eq!(content[1]["props"]["userId"], ada.to_string());
    assert_eq!(content[1]["props"]["name"], "Ada Lovelace");
    assert_eq!(content[2]["text"], ", see this");
}

#[test]
fn ambiguous_mention_stays_text() {
    let blocks = markdown_to_blocks(
        "hi @Ada",
        &[
            (Id::new_v7(), "Ada".to_owned()),
            (Id::new_v7(), "Ada".to_owned()),
        ],
    );
    assert_eq!(
        blocks[0]["content"],
        json!([{"type": "text", "text": "hi @Ada", "styles": {}}])
    );
}

#[test]
fn mention_needs_a_word_boundary() {
    let blocks = markdown_to_blocks("@Adam", &[(Id::new_v7(), "Ada".to_owned())]);
    assert_eq!(blocks[0]["content"][0]["text"], "@Adam");
}

#[test]
fn html_stays_text() {
    let blocks = import("<b>x</b>");
    assert_eq!(blocks[0]["type"], "paragraph");
    assert_eq!(blocks[0]["content"][0]["text"], "<b>x</b>");
}

#[test]
fn empty_markdown_has_no_blocks() {
    assert!(import("").is_empty());
    assert!(import("  \n\n").is_empty());
}

/// How deep `blocks` nest (1 = top level only).
fn nesting(blocks: &[Value]) -> usize {
    let mut deepest = 0;
    let mut stack: Vec<(&Value, usize)> = blocks.iter().map(|block| (block, 1)).collect();
    while let Some((block, depth)) = stack.pop() {
        deepest = deepest.max(depth);
        for child in block["children"].as_array().into_iter().flatten() {
            stack.push((child, depth + 1));
        }
    }
    deepest
}

#[test]
fn deeply_nested_markdown_is_flattened_without_recursing() {
    // Runs on a test thread (2 MB stack): one frame per level would overflow it.
    let quotes = format!("{} deep", ">".repeat(100_000));
    let blocks = import(&quotes);
    assert!(nesting(&blocks) <= orbit_server::collab::blocknote::MAX_BLOCK_DEPTH + 1);
    assert!(all_texts(&blocks).iter().any(|text| text == "deep"));

    let list = format!("{}deep", "- ".repeat(100_000));
    let blocks = import(&list);
    let depth = nesting(&blocks);
    assert!(depth > 10 && depth <= orbit_server::collab::blocknote::MAX_BLOCK_DEPTH + 1);
    assert!(round_trip(&list).contains("deep"));

    // Shallow nesting is unchanged.
    let blocks = import("- a\n  - b\n    - c");
    assert_eq!(nesting(&blocks), 3);
}
