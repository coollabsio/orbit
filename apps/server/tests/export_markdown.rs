//! BlockNote JSON → Markdown converter: goldens over the collab and Notion fixtures, plus one
//! focused case per block type. `UPDATE_EXPORT_GOLDENS=1` rewrites `tests/fixtures/export/`.

use std::path::{Path, PathBuf};

use orbit_server::export::markdown::{KeepLinks, LinkResolver, PageLink, blocks_to_markdown};
use serde_json::{Value, json};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn json_files(dir: &Path) -> Vec<(String, Value)> {
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .collect();
    files.sort();
    files
        .into_iter()
        .map(|path| {
            let name = path.file_stem().unwrap().to_string_lossy().into_owned();
            (
                name,
                serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap(),
            )
        })
        .collect()
}

fn check_golden(name: &str, actual: &str) {
    let path = fixtures().join("export").join(format!("{name}.md"));
    if std::env::var_os("UPDATE_EXPORT_GOLDENS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_else(|_| {
        panic!(
            "missing golden {}; run with UPDATE_EXPORT_GOLDENS=1",
            path.display()
        )
    });
    assert_eq!(actual, expected, "golden {name} differs");
}

fn md(blocks: Value) -> String {
    blocks_to_markdown(blocks.as_array().unwrap(), &KeepLinks)
}

#[test]
fn collab_fixtures_match_goldens_in_both_block_forms() {
    let goldens = json_files(&fixtures().join("collab"));
    assert!(!goldens.is_empty());
    for (name, fixture) in goldens {
        let short = blocks_to_markdown(fixture["blocks"].as_array().unwrap(), &KeepLinks);
        let full = blocks_to_markdown(fixture["expected"].as_array().unwrap(), &KeepLinks);
        assert_eq!(
            short, full,
            "{name}: shorthand and full blocks convert differently"
        );
        check_golden(&format!("collab_{name}"), &full);
    }
}

#[test]
fn notion_import_fixtures_match_goldens() {
    let goldens = json_files(&fixtures().join("notion/expected"));
    assert!(!goldens.is_empty());
    for (name, fixture) in goldens {
        let blocks = fixture["content"].as_array().unwrap();
        let first = blocks_to_markdown(blocks, &KeepLinks);
        assert_eq!(
            first,
            blocks_to_markdown(blocks, &KeepLinks),
            "{name}: not deterministic"
        );
        check_golden(&format!("notion_{name}"), &first);
    }
}

#[test]
fn headings_keep_levels_and_toggle_headings_become_headings() {
    let text = md(json!([
        {"type": "heading", "props": {"level": 1}, "content": "One"},
        {"type": "heading", "props": {"level": 3, "isToggleable": true}, "content": "Three",
         "children": [{"type": "paragraph", "content": "inside"}]},
        {"type": "heading", "props": {"level": 9}, "content": "Clamped"},
        {"type": "heading", "props": {"level": 2}, "content": []},
    ]));
    assert_eq!(text, "# One\n\n### Three\n\ninside\n\n###### Clamped\n");
}

#[test]
fn lists_nest_number_and_check() {
    let text = md(json!([
        {"type": "bulletListItem", "content": "a", "children": [
            {"type": "bulletListItem", "content": "a.1"},
            {"type": "numberedListItem", "content": "a.n"},
        ]},
        {"type": "bulletListItem", "content": "b"},
        {"type": "numberedListItem", "props": {"start": 3}, "content": "three"},
        {"type": "numberedListItem", "content": "four", "children": [
            {"type": "checkListItem", "props": {"checked": true}, "content": "done"},
        ]},
        {"type": "paragraph", "content": ""},
        {"type": "numberedListItem", "content": "restart"},
        {"type": "checkListItem", "props": {"checked": false}, "content": "todo"},
        {"type": "toggleListItem", "content": "toggle", "children": [
            {"type": "paragraph", "content": "hidden"},
        ]},
    ]));
    assert_eq!(
        text,
        "- a\n  - a.1\n  1. a.n\n- b\n3. three\n4. four\n   - [x] done\n\n1. restart\n- [ ] todo\n- toggle\n  hidden\n"
    );
}

#[test]
fn quotes_callouts_code_and_dividers() {
    let text = md(json!([
        {"type": "quote", "content": "said\nand more", "children": [
            {"type": "paragraph", "content": "child"},
        ]},
        {"type": "callout", "props": {"emoji": "💡"}, "content": [{"type": "text", "text": "Tip", "styles": {"bold": true}}],
         "children": [{"type": "bulletListItem", "content": "nested"}]},
        {"type": "codeBlock", "props": {"language": "rust"}, "content": "let a = \"```\";"},
        {"type": "codeBlock", "props": {"language": "text"}, "content": ""},
        {"type": "divider"},
    ]));
    assert_eq!(
        text,
        "> said\\\n> and more\n>\n> child\n\n> 💡 **Tip**\n>\n> - nested\n\n````rust\nlet a = \"```\";\n````\n\n```\n```\n\n---\n"
    );
}

#[test]
fn tables_escape_pipes_span_columns_and_break_lines() {
    let text = md(json!([{
        "type": "table",
        "content": {"type": "tableContent", "rows": [
            {"cells": ["A|B", {"type": "tableCell", "props": {"colspan": 2}, "content": [{"type": "text", "text": "wide", "styles": {"italic": true}}]}]},
            {"cells": [[{"type": "text", "text": "x\ny", "styles": {}}], "", [{"type": "link", "href": "https://e.com/a|b", "content": "l"}]]},
        ]},
    }]));
    assert_eq!(
        text,
        "| A\\|B | *wide* |  |\n| --- | --- | --- |\n| x<br>y |  | [l](https://e.com/a%7Cb) |\n"
    );
}

#[test]
fn media_and_page_blocks_use_the_resolver() {
    struct Links;
    impl LinkResolver for Links {
        fn page(&self, page_id: &str) -> Option<PageLink> {
            (page_id == "known").then(|| PageLink {
                title: "Child [1]".to_owned(),
                href: "Parent/Child page.md".to_owned(),
            })
        }
        fn url(&self, url: &str) -> String {
            url.replace("/files/", "/assets/")
        }
    }
    let blocks = json!([
        {"type": "image", "props": {"url": "/p/files/a.png", "name": "a.png", "caption": "The *cap*"}},
        {"type": "image", "props": {"url": ""}},
        {"type": "file", "props": {"url": "https://e.com/r (1).pdf"}},
        {"type": "page", "props": {"pageId": "known"}},
        {"type": "page", "props": {"pageId": "gone"}},
        {"type": "paragraph", "content": [{"type": "link", "href": "/files/x", "content": [{"type": "text", "text": "see", "styles": {"code": true}}]}]},
    ]);
    let text = blocks_to_markdown(blocks.as_array().unwrap(), &Links);
    assert_eq!(
        text,
        "![a.png](/p/assets/a.png)\n\n*The \\*cap\\**\n\n[r (1).pdf](https://e.com/r%20%281%29.pdf)\n\n[Child \\[1\\]](Parent/Child%20page.md)\n\n*Missing page*\n\n[`see`](/assets/x)\n"
    );
}

#[test]
fn inline_styles_escape_and_keep_edge_spaces_outside_markers() {
    let text = md(json!([
        {"type": "paragraph", "content": [
            {"type": "text", "text": "a ", "styles": {}},
            {"type": "text", "text": "bold ", "styles": {"bold": true, "textColor": "red"}},
            {"type": "text", "text": "it", "styles": {"italic": true}},
            {"type": "text", "text": " ", "styles": {}},
            {"type": "text", "text": "gone", "styles": {"strike": true}},
            {"type": "text", "text": " ", "styles": {}},
            {"type": "text", "text": "u", "styles": {"underline": true}},
            {"type": "text", "text": " `tick` ", "styles": {"code": true}},
            {"type": "text", "text": "*not* [x] <b> snake_case _y_ ~z~", "styles": {}},
        ]},
        {"type": "paragraph", "content": "# not a heading\n1. not a list\n- nor this"},
    ]));
    assert_eq!(
        text,
        "a **bold** *it* ~~gone~~ <u>u</u> `` `tick` `` \\*not\\* \\[x\\] \\<b\\> snake_case \\_y\\_ \\~z\\~\n\n\\# not a heading\\\n1\\. not a list\\\n\\- nor this\n"
    );
}

#[test]
fn empty_documents_and_unknown_blocks() {
    assert_eq!(md(json!([])), "");
    assert_eq!(md(json!([{"type": "paragraph", "content": []}])), "");
    assert_eq!(
        md(
            json!([{"type": "mystery", "content": "kept", "children": [{"type": "paragraph", "content": "too"}]}])
        ),
        "kept\n\ntoo\n"
    );
}

#[test]
fn mentions_render_as_at_names_with_live_names_first() {
    struct Members;
    impl LinkResolver for Members {
        fn page(&self, _page_id: &str) -> Option<PageLink> {
            None
        }
        fn url(&self, url: &str) -> String {
            url.to_owned()
        }
        fn member_name(&self, user_id: &str) -> Option<String> {
            (user_id == "0199a0b0-0000-7000-8000-0000000000a1").then(|| "Ann Renamed".to_owned())
        }
    }
    let blocks = json!([
        {"type": "paragraph", "content": [
            {"type": "text", "text": "Hi ", "styles": {}},
            {"type": "mention", "props": {"userId": "0199a0b0-0000-7000-8000-0000000000a1", "name": "Ann Lee"}},
            {"type": "text", "text": " and ", "styles": {}},
            {"type": "mention", "props": {"userId": "0199a0b0-0000-7000-8000-0000000000b2", "name": "Bob_Stone"}},
            {"type": "mention", "props": {"userId": "0199a0b0-0000-7000-8000-0000000000c3", "name": ""}},
        ]},
        {"type": "table", "content": {"type": "tableContent", "rows": [
            {"cells": [[{"type": "mention", "props": {"userId": "x", "name": "Cell|Name"}}]]},
        ]}},
    ]);
    assert_eq!(
        md(blocks.clone()),
        "Hi @Ann Lee and @Bob_Stone@Unknown user\n\n| @Cell\\|Name |\n| --- |\n"
    );
    assert_eq!(
        blocks_to_markdown(blocks.as_array().unwrap(), &Members),
        "Hi @Ann Renamed and @Bob_Stone@Unknown user\n\n| @Cell\\|Name |\n| --- |\n"
    );
}
