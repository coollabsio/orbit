//! Obsidian notes → BlockNote blocks (`obsidian_to_blocks`), and the guarantee that plain
//! markdown (`markdown_to_blocks`) reads Obsidian syntax exactly as before.

use orbit_server::collab::markdown_import::{
    LinkKind, LinkResolver, LinkTarget, markdown_to_blocks, obsidian_links, obsidian_to_blocks,
};
use serde_json::{Value, json};

struct Vault;

impl LinkResolver for Vault {
    fn resolve(&self, target: &str, _kind: LinkKind) -> Option<LinkTarget> {
        let path = target.split('#').next().unwrap_or_default();
        match path {
            "Note" | "Folder/Note" | "Folder/Note.md" => Some(LinkTarget::Page {
                href: "/docs/page-1".to_owned(),
            }),
            "img.png" | "assets/img.png" => Some(LinkTarget::Image {
                url: "/files/img".to_owned(),
            }),
            "doc.pdf" => Some(LinkTarget::File {
                url: "/files/pdf".to_owned(),
                name: "doc.pdf".to_owned(),
            }),
            _ => None,
        }
    }
}

fn import(markdown: &str) -> Vec<Value> {
    obsidian_to_blocks(markdown, &Vault)
}

/// `(kind, text or href, styles)` of each inline item of a block.
fn inline(block: &Value) -> Vec<(String, String, Value)> {
    block["content"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| match item["type"].as_str().unwrap() {
            "link" => (
                format!("link:{}", item["href"].as_str().unwrap()),
                item["content"][0]["text"].as_str().unwrap().to_owned(),
                item["content"][0]["styles"].clone(),
            ),
            kind => (
                kind.to_owned(),
                item["text"].as_str().unwrap_or_default().to_owned(),
                item["styles"].clone(),
            ),
        })
        .collect()
}

#[test]
fn plain_markdown_keeps_obsidian_syntax_as_text() {
    let blocks = markdown_to_blocks(
        "---\ntitle: x\n---\n\n[[Note]] ==hi== %%c%%\n\n> [!note] Title\n> body",
        &[],
    );
    let types: Vec<&str> = blocks.iter().map(|b| b["type"].as_str().unwrap()).collect();
    assert_eq!(types, ["divider", "heading", "paragraph", "quote"]);
    assert_eq!(
        inline(&blocks[2]),
        [(
            "text".to_owned(),
            "[[Note]] ==hi== %%c%%".to_owned(),
            json!({})
        )]
    );
    assert_eq!(inline(&blocks[3])[0].1, "[!note] Title\nbody");
}

#[test]
fn wikilinks_resolve_with_alias_and_heading() {
    let blocks = import("See [[Note]], [[Note#Part|the part]], [[Note#Part]] and [[Missing]].");
    assert_eq!(
        inline(&blocks[0]),
        [
            ("text".to_owned(), "See ".to_owned(), json!({})),
            ("link:/docs/page-1".to_owned(), "Note".to_owned(), json!({})),
            ("text".to_owned(), ", ".to_owned(), json!({})),
            (
                "link:/docs/page-1".to_owned(),
                "the part".to_owned(),
                json!({})
            ),
            ("text".to_owned(), ", ".to_owned(), json!({})),
            (
                "link:/docs/page-1".to_owned(),
                "Note > Part".to_owned(),
                json!({})
            ),
            ("text".to_owned(), " and Missing.".to_owned(), json!({})),
        ]
    );
}

#[test]
fn markdown_links_resolve_relative_targets_only() {
    let blocks = import("[a](Folder/Note.md) [b](https://x.dev) [c](#top) [d](gone.md)");
    assert_eq!(
        inline(&blocks[0]),
        [
            ("link:/docs/page-1".to_owned(), "a".to_owned(), json!({})),
            ("text".to_owned(), " ".to_owned(), json!({})),
            ("link:https://x.dev".to_owned(), "b".to_owned(), json!({})),
            ("text".to_owned(), " ".to_owned(), json!({})),
            ("link:#top".to_owned(), "c".to_owned(), json!({})),
            ("text".to_owned(), " d".to_owned(), json!({})),
        ]
    );
}

#[test]
fn embeds_become_image_file_blocks_or_links() {
    let blocks = import(
        "![[img.png]]\n\n![[Note]] and ![[nothing.png]]\n\n![[doc.pdf]]\n\n![alt](assets/img.png)\n\nRead [the spec](doc.pdf).",
    );
    let types: Vec<&str> = blocks.iter().map(|b| b["type"].as_str().unwrap()).collect();
    assert_eq!(
        types,
        ["image", "paragraph", "file", "image", "paragraph", "file"]
    );
    assert_eq!(blocks[0]["props"]["url"], "/files/img");
    assert_eq!(blocks[0]["props"]["name"], "img.png");
    assert_eq!(
        inline(&blocks[1]),
        [
            ("link:/docs/page-1".to_owned(), "Note".to_owned(), json!({})),
            ("text".to_owned(), " and nothing.png".to_owned(), json!({})),
        ]
    );
    assert_eq!(blocks[2]["props"]["url"], "/files/pdf");
    assert_eq!(blocks[2]["props"]["name"], "doc.pdf");
    assert_eq!(blocks[3]["props"]["name"], "alt");
    assert_eq!(
        inline(&blocks[4]),
        [("text".to_owned(), "Read the spec.".to_owned(), json!({}))]
    );
    assert_eq!(blocks[5]["props"]["url"], "/files/pdf");
}

#[test]
fn callouts_become_callout_blocks() {
    let blocks = import(
        "> [!warning]- Careful now\n> First line\n>\n> Second paragraph\n\n> [!tip]\n> Body\n\n> plain quote",
    );
    assert_eq!(blocks[0]["type"], "callout");
    assert_eq!(blocks[0]["props"]["emoji"], "⚠️");
    assert_eq!(blocks[0]["props"]["backgroundColor"], "orange");
    assert_eq!(
        inline(&blocks[0]),
        [(
            "text".to_owned(),
            "Careful now".to_owned(),
            json!({"bold": true})
        )]
    );
    let children = blocks[0]["children"].as_array().unwrap();
    assert_eq!(children.len(), 2);
    assert_eq!(inline(&children[0])[0].1, "First line");
    assert_eq!(inline(&children[1])[0].1, "Second paragraph");

    assert_eq!(blocks[1]["type"], "callout");
    assert_eq!(blocks[1]["props"]["emoji"], "🔥");
    assert_eq!(inline(&blocks[1])[0].1, "Tip");
    assert_eq!(inline(&blocks[1]["children"][0])[0].1, "Body");

    assert_eq!(blocks[2]["type"], "quote");
}

#[test]
fn highlights_comments_and_frontmatter() {
    let blocks = import(
        "---\ntags: [a]\n---\n# Title\n\nA ==marked **bold**== end %%hidden%% and == alone\n\n%%\nblock comment\n%%\n\n`==code== %%kept%%`",
    );
    assert_eq!(blocks[0]["type"], "heading");
    assert_eq!(
        inline(&blocks[1]),
        [
            ("text".to_owned(), "A ".to_owned(), json!({})),
            (
                "text".to_owned(),
                "marked ".to_owned(),
                json!({"backgroundColor": "yellow"})
            ),
            (
                "text".to_owned(),
                "bold".to_owned(),
                json!({"bold": true, "backgroundColor": "yellow"})
            ),
            (
                "text".to_owned(),
                " end  and == alone".to_owned(),
                json!({})
            ),
        ]
    );
    assert_eq!(
        inline(&blocks[2]),
        [(
            "text".to_owned(),
            "==code== %%kept%%".to_owned(),
            json!({"code": true})
        )]
    );
    assert_eq!(blocks.len(), 3);
}

#[test]
fn lists_link_and_embed_targets() {
    let links = obsidian_links(
        "[[Note#H|x]] ![[img.png]] [a](My%20Note.md) [b](https://x.dev) %%[[Hidden]]%%\n\n```\n[[InCode]]\n```",
    );
    assert_eq!(
        links,
        [
            ("Note#H".to_owned(), LinkKind::Link),
            ("img.png".to_owned(), LinkKind::Embed),
            ("My Note.md".to_owned(), LinkKind::Link),
        ]
    );
}
