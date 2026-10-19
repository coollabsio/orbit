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
                item["text"].as_str().map_or_else(|| kind.to_owned(), str::to_owned),
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
    assert!(parts.contains(&("b".into(), json!({"bold": true}))), "{parts:?}");
    assert!(parts.contains(&("i".into(), json!({"italic": true}))), "{parts:?}");
    assert!(parts.contains(&("s".into(), json!({"strike": true}))), "{parts:?}");
    assert!(parts.contains(&("code".into(), json!({"code": true}))), "{parts:?}");
    assert!(parts.contains(&("link:https://x.dev".into(), json!("l"))), "{parts:?}");
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
    assert_eq!(blocks[0]["props"]["caption"], "alt");
}

#[test]
fn exact_mention() {
    let ada = Id::new_v7();
    let blocks = markdown_to_blocks(
        "hi @Ada Lovelace, see this",
        &[(ada, "Ada Lovelace".to_owned()), (Id::new_v7(), "Ada".to_owned())],
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
        &[(Id::new_v7(), "Ada".to_owned()), (Id::new_v7(), "Ada".to_owned())],
    );
    assert_eq!(blocks[0]["content"], json!([{"type": "text", "text": "hi @Ada", "styles": {}}]));
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
