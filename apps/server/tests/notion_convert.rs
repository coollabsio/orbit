//! Notion → BlockNote converter tests against fixtures modelled on the Notion API docs.
//!
//! Golden outputs live in `tests/fixtures/notion/expected/<fixture>.json` (block ids come from
//! a counter, so they are stable). A missing golden is written on the first run; set
//! `UPDATE_NOTION_GOLDENS=1` to rewrite them after an intended change.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use orbit_server::notion::convert::{
    ConvertContext, ConvertReport, ConvertedPage, MapResolver, PageCover, ResolvedFile,
    collect_file_refs, convert_page, database_row_properties_table, page_cover, page_icon,
    page_title,
};
use orbit_server::notion::model::{BlockNode, FileSource, NotionPage};
use serde_json::{Value, json};

const MAPPED_PAGE: &str = "3c612f56-fdd0-4a30-a4d6-bda7d7426309";
const MAPPED_CHILD: &str = "11111111-1111-4111-8111-111111111111";
const MAPPED_DB: &str = "a1d8501e-1ac1-43e9-a6bd-ea9fe6c8822b";
const MAPPED_RELATION: &str = "dd456007-6c66-4bba-957e-ea501dcda3a6";
const IMG_URL: &str =
    "https://prod-files-secure.s3.us-west-2.amazonaws.com/ws/img1/brocolli.jpeg?X-Amz-Signature=a";
const AUDIO_URL: &str = "https://s3.us-west-2.amazonaws.com/secure.notion-static.com/9bc6c6e0-32b8-4d55-8c12-3ae931f43a01/sample.mp3?x=1";
const EMBED_URL: &str = "https://file.notion.so/f/f/ws/html1/page.html?downloadName=page.html";
const COVER_URL: &str =
    "https://prod-files-secure.s3.us-west-2.amazonaws.com/ws/cover/cover.png?X-Amz-Signature=c";

const FIXTURES: &[&str] = &[
    "text_blocks",
    "lists",
    "rich_text",
    "mentions",
    "table",
    "columns",
    "synced_block",
    "callout",
    "media",
    "child_pages",
    "unsupported",
    "nested_page",
    "database_row",
];

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/notion")
}

fn load(name: &str) -> (NotionPage, Vec<BlockNode>) {
    let raw = std::fs::read_to_string(fixture_dir().join(format!("{name}.json"))).unwrap();
    let value: Value = serde_json::from_str(&raw).unwrap();
    (
        serde_json::from_value(value["page"].clone()).unwrap(),
        serde_json::from_value(value["blocks"].clone()).unwrap(),
    )
}

fn resolver() -> MapResolver {
    let mut resolver = MapResolver::default();
    for (notion, orbit) in [
        (MAPPED_PAGE, "01900000-0000-7000-8000-000000000001"),
        (MAPPED_CHILD, "01900000-0000-7000-8000-000000000002"),
        (MAPPED_DB, "01900000-0000-7000-8000-000000000003"),
        (MAPPED_RELATION, "01900000-0000-7000-8000-000000000004"),
    ] {
        resolver.pages.insert(notion.to_owned(), orbit.to_owned());
    }
    resolver
        .titles
        .insert(MAPPED_RELATION.to_owned(), "Launch project".to_owned());
    resolver
        .titles
        .insert(MAPPED_PAGE.to_owned(), "This is a test page".to_owned());
    resolver.users.insert(
        "c2f20311-9e54-4d11-8c79-7398424ae41e".to_owned(),
        "Ada Lovelace".to_owned(),
    );
    for (url, orbit_url, name) in [
        (IMG_URL, "/api/v1/files/img1", "brocolli.jpeg"),
        (AUDIO_URL, "/api/v1/files/audio1", "sample.mp3"),
        (EMBED_URL, "/api/v1/files/embed1", "page.html"),
        (COVER_URL, "/api/v1/files/cover1", "cover.png"),
    ] {
        resolver.files.insert(
            url.to_owned(),
            ResolvedFile {
                url: orbit_url.to_owned(),
                name: name.to_owned(),
            },
        );
    }
    resolver
}

fn convert(name: &str) -> (ConvertedPage, ConvertReport) {
    let (page, blocks) = load(name);
    let resolver = resolver();
    let mut counter = 0u64;
    let mut ctx = ConvertContext::new(&resolver).with_id_generator(move || {
        counter += 1;
        format!("00000000-0000-4000-8000-{counter:012}")
    });
    let converted = convert_page(&page, &blocks, &mut ctx);
    (converted, ctx.into_report())
}

fn golden(name: &str) -> Value {
    let (converted, report) = convert(name);
    json!({
        "title": converted.title,
        "icon": converted.icon,
        "cover": converted.cover,
        "report": report,
        "content": converted.content,
    })
}

#[test]
fn goldens_match() {
    let update = std::env::var_os("UPDATE_NOTION_GOLDENS").is_some();
    for name in FIXTURES {
        let actual = golden(name);
        assert_blocknote_schema(actual["content"].as_array().unwrap());
        let path = fixture_dir().join(format!("expected/{name}.json"));
        if update || !path.exists() {
            let mut text = serde_json::to_string_pretty(&actual).unwrap();
            text.push('\n');
            std::fs::write(&path, text).unwrap();
            continue;
        }
        let expected: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            actual, expected,
            "golden mismatch for {name}; rerun with UPDATE_NOTION_GOLDENS=1 if intended"
        );
    }
}

#[test]
fn conversion_is_deterministic() {
    for name in FIXTURES {
        assert_eq!(golden(name), golden(name), "{name}");
    }
}

// ---------------------------------------------------------------- schema check

fn props_for(kind: &str) -> (&'static [&'static str], &'static [&'static str]) {
    const DEFAULT: &[&str] = &["textColor", "backgroundColor", "textAlignment"];
    match kind {
        "paragraph" | "bulletListItem" | "toggleListItem" => (DEFAULT, &[]),
        "numberedListItem" => (DEFAULT, &["start"]),
        "checkListItem" => (
            &["textColor", "backgroundColor", "textAlignment", "checked"],
            &[],
        ),
        "heading" => (
            &[
                "textColor",
                "backgroundColor",
                "textAlignment",
                "level",
                "isToggleable",
            ],
            &[],
        ),
        "quote" => (&["textColor", "backgroundColor"], &[]),
        "callout" => (&["emoji", "textColor", "backgroundColor"], &[]),
        "codeBlock" => (&["language"], &[]),
        "divider" => (&[], &[]),
        "table" => (&["textColor"], &[]),
        "image" => (
            &[
                "textAlignment",
                "backgroundColor",
                "name",
                "url",
                "caption",
                "showPreview",
            ],
            &["previewWidth"],
        ),
        "file" => (&["backgroundColor", "name", "url", "caption"], &[]),
        "page" => (&["pageId"], &[]),
        other => panic!("block type {other} is not in Orbit's editor schema"),
    }
}

const COLORS: &[&str] = &[
    "default", "gray", "brown", "red", "orange", "yellow", "green", "blue", "purple", "pink",
];

/// Checks every block against Orbit's editor schema (BlockNote 0.55 defaults + `page` +
/// `callout`).
fn assert_blocknote_schema(blocks: &[Value]) {
    let mut ids = HashSet::new();
    check_blocks(blocks, &mut ids);
}

fn check_blocks(blocks: &[Value], ids: &mut HashSet<String>) {
    for block in blocks {
        let object = block.as_object().expect("block object");
        let id = object["id"].as_str().expect("block id");
        assert!(ids.insert(id.to_owned()), "duplicate id {id}");
        let kind = object["type"].as_str().unwrap();
        let props = object["props"].as_object().expect("props");
        let (required, optional) = props_for(kind);
        for key in required {
            assert!(props.contains_key(*key), "{kind} misses prop {key}");
        }
        for key in props.keys() {
            assert!(
                required.contains(&key.as_str()) || optional.contains(&key.as_str()),
                "{kind} has unknown prop {key}"
            );
        }
        for color_key in ["textColor", "backgroundColor"] {
            if let Some(color) = props.get(color_key) {
                assert!(
                    COLORS.contains(&color.as_str().unwrap()),
                    "{kind} {color_key} {color}"
                );
            }
        }
        if kind == "callout" {
            assert!(
                !props["emoji"].as_str().unwrap().is_empty(),
                "callout emoji"
            );
        }
        match kind {
            "paragraph" | "heading" | "bulletListItem" | "numberedListItem" | "checkListItem"
            | "toggleListItem" | "quote" | "callout" => {
                check_inline(object["content"].as_array().expect("inline content"))
            }
            "codeBlock" => {
                for item in object["content"].as_array().unwrap() {
                    assert_eq!(item["type"], "text");
                    assert_eq!(item["styles"], json!({}));
                }
            }
            "table" => {
                let content = &object["content"];
                assert_eq!(content["type"], "tableContent");
                let width = content["columnWidths"].as_array().unwrap().len();
                for row in content["rows"].as_array().unwrap() {
                    let cells = row["cells"].as_array().unwrap();
                    assert_eq!(cells.len(), width, "row width");
                    for cell in cells {
                        assert_eq!(cell["type"], "tableCell");
                        check_inline(cell["content"].as_array().unwrap());
                    }
                }
            }
            _ => assert!(
                !object.contains_key("content"),
                "{kind} must not have content"
            ),
        }
        check_blocks(object["children"].as_array().expect("children"), ids);
    }
}

fn check_inline(items: &[Value]) {
    for item in items {
        match item["type"].as_str().unwrap() {
            "text" => check_text(item),
            "link" => {
                let href = item["href"].as_str().unwrap();
                assert!(
                    href.starts_with("https://")
                        || href.starts_with("http://")
                        || href.starts_with("mailto:")
                        || href.starts_with("/docs/"),
                    "unexpected href {href}"
                );
                let content = item["content"].as_array().unwrap();
                assert!(!content.is_empty());
                content.iter().for_each(check_text);
            }
            other => panic!("unknown inline type {other}"),
        }
    }
}

fn check_text(item: &Value) {
    assert_eq!(item["type"], "text");
    assert!(
        !item["text"].as_str().unwrap().is_empty(),
        "empty text item"
    );
    for (key, value) in item["styles"].as_object().unwrap() {
        match key.as_str() {
            "bold" | "italic" | "underline" | "strike" | "code" => assert_eq!(value, &json!(true)),
            "textColor" | "backgroundColor" => {
                assert!(COLORS[1..].contains(&value.as_str().unwrap()), "{value}")
            }
            other => panic!("unknown style {other}"),
        }
    }
}

// ---------------------------------------------------------------- structure

fn types(blocks: &[Value]) -> Vec<&str> {
    blocks
        .iter()
        .map(|block| block["type"].as_str().unwrap())
        .collect()
}

fn texts(content: &Value) -> String {
    content
        .as_array()
        .unwrap()
        .iter()
        .map(|item| match item["type"].as_str().unwrap() {
            "link" => texts(&item["content"]),
            _ => item["text"].as_str().unwrap().to_owned(),
        })
        .collect()
}

#[test]
fn text_blocks() {
    let (page, report) = convert("text_blocks");
    assert_eq!(page.title, "Text blocks");
    assert_eq!(page.icon.as_deref(), Some("🥬"));
    let c = &page.content;
    assert_eq!(
        types(c),
        [
            "paragraph",
            "paragraph",
            "heading",
            "heading",
            "heading",
            "heading",
            "quote",
            "divider",
            "codeBlock",
            "paragraph",
            "codeBlock",
            "paragraph",
            "paragraph"
        ]
    );
    assert_eq!(c[1]["content"], json!([]));
    assert_eq!(c[2]["props"]["level"], 1);
    assert_eq!(c[3]["content"][0]["styles"], json!({"textColor": "green"}));
    assert_eq!(c[4]["props"]["level"], 3);
    assert_eq!(c[4]["props"]["isToggleable"], true);
    assert_eq!(c[4]["props"]["backgroundColor"], "blue");
    assert_eq!(
        texts(&c[4]["children"][0]["content"]),
        "Hidden under the heading"
    );
    assert_eq!(c[5]["props"]["level"], 4);
    assert_eq!(
        c[6]["props"],
        json!({"textColor": "gray", "backgroundColor": "default"})
    );
    assert_eq!(c[6]["children"].as_array().unwrap().len(), 1);
    assert_eq!(c[8]["props"]["language"], "javascript");
    assert_eq!(texts(&c[8]["content"]), "const a = 3\nconsole.log(a)");
    assert_eq!(texts(&c[9]["content"]), "Example");
    assert_eq!(c[10]["props"]["language"], "cpp");
    assert_eq!(
        c[11]["content"][0],
        json!({"type": "text", "text": "e=mc^2", "styles": {"code": true}})
    );
    assert_eq!(c[12]["props"]["textColor"], "red");
    assert_eq!(report.lossy.get("code_caption"), Some(&1));
    assert_eq!(report.lossy.get("equation"), Some(&1));
}

#[test]
fn lists() {
    let (page, report) = convert("lists");
    let c = &page.content;
    assert_eq!(
        types(c),
        [
            "bulletListItem",
            "numberedListItem",
            "numberedListItem",
            "numberedListItem",
            "checkListItem",
            "checkListItem",
            "toggleListItem"
        ]
    );
    let nested = c[0]["children"].as_array().unwrap();
    assert_eq!(types(nested), ["bulletListItem", "bulletListItem"]);
    assert_eq!(
        types(nested[1]["children"].as_array().unwrap()),
        ["paragraph"]
    );
    assert_eq!(c[1]["props"]["start"], 3);
    assert!(c[2]["props"].get("start").is_none());
    assert_eq!(c[4]["props"]["checked"], false);
    assert_eq!(c[5]["props"]["checked"], true);
    assert_eq!(c[5]["props"]["textColor"], "purple");
    assert_eq!(texts(&c[6]["children"][0]["content"]), "Inside the toggle");
    assert_eq!(report.lossy.get("numbered_list_format"), Some(&1));
}

#[test]
fn rich_text_annotations_colors_and_links() {
    let (page, report) = convert("rich_text");
    let c = &page.content;
    let first = c[0]["content"].as_array().unwrap();
    assert_eq!(first[0]["styles"], json!({"bold": true}));
    assert_eq!(first[1]["styles"], json!({"italic": true}));
    assert_eq!(first[2]["styles"], json!({"underline": true}));
    assert_eq!(first[3]["styles"], json!({"strike": true}));
    assert_eq!(first[4]["styles"], json!({"code": true}));
    assert_eq!(
        first[6]["styles"],
        json!({"bold": true, "italic": true, "underline": true, "strike": true, "code": true, "textColor": "red"})
    );
    let second = c[1]["content"].as_array().unwrap();
    assert_eq!(second[0]["styles"], json!({"textColor": "red"}));
    assert_eq!(second[2]["styles"], json!({"backgroundColor": "yellow"}));
    assert_eq!(second[4]["styles"], json!({}));
    assert_eq!(second[6]["styles"], json!({}));

    let links: Vec<&Value> = c[2]["content"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["type"] == "link")
        .collect();
    assert_eq!(links[0]["href"], "https://developers.notion.com/");
    // Two runs of the same link merge into one link with two styled texts.
    assert_eq!(links[1]["href"], "https://example.com/a");
    assert_eq!(links[1]["content"].as_array().unwrap().len(), 2);
    assert_eq!(
        links[2]["href"],
        "/docs/01900000-0000-7000-8000-000000000001"
    );
    assert!(
        links[3]["href"]
            .as_str()
            .unwrap()
            .starts_with("https://www.notion.so/acme/Other-")
    );
    assert_eq!(links[4]["href"], "mailto:ada@example.com");
    assert_eq!(links.len(), 5, "javascript: link is dropped");
    assert!(texts(&c[2]["content"]).contains("bad link"));
    assert_eq!(report.lossy.get("unsafe_link"), Some(&1));
    assert_eq!(report.unresolved_page_links, 1);

    let equation = &c[3]["content"][1];
    assert_eq!(
        equation,
        &json!({"type": "text", "text": "E = mc^2", "styles": {"code": true}})
    );
    assert_eq!(c[3]["content"][2]["text"], " and line\nbreak");
}

#[test]
fn mentions() {
    let (page, report) = convert("mentions");
    let c = &page.content;
    let pages = c[0]["content"].as_array().unwrap();
    assert_eq!(pages[0]["type"], "link");
    assert_eq!(
        pages[0]["href"],
        "/docs/01900000-0000-7000-8000-000000000001"
    );
    assert_eq!(texts(&pages[0]["content"]), "This is a test page");
    assert_eq!(
        pages[2]["href"],
        "https://app.notion.com/p/9f9e9d9c111142228333444455556666"
    );
    assert_eq!(
        pages[4]["href"],
        "/docs/01900000-0000-7000-8000-000000000003"
    );
    assert_eq!(
        texts(&c[1]["content"]),
        "@Anonymous @Ada Lovelace @Grace Hopper"
    );
    assert_eq!(c[1]["content"][4]["styles"], json!({"bold": true}));
    let third = texts(&c[2]["content"]);
    assert!(
        third.starts_with("2022-12-16 2023-03-01 09:00 UTC → 2023-03-02 17:30 UTC "),
        "{third}"
    );
    assert!(third.contains("@Today"));
    assert!(third.contains(":bufo:"));
    assert!(third.ends_with("future thing"));
    assert_eq!(c[2]["content"][4]["type"], "link");
    assert_eq!(report.unresolved_page_links, 1);
    assert_eq!(report.lossy.get("mention:future_mention"), Some(&1));
}

#[test]
fn tables() {
    let (page, report) = convert("table");
    let c = &page.content;
    assert_eq!(types(c), ["table", "table"]);
    let content = &c[0]["content"];
    assert_eq!(content["columnWidths"], json!([null, null, null]));
    assert_eq!(content["headerRows"], 1);
    assert!(content.get("headerCols").is_none());
    let rows = content["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(texts(&rows[1]["cells"][1]["content"]), "Engineer");
    assert_eq!(rows[1]["cells"][2]["content"][0]["type"], "link");
    // A short row is padded to the table width.
    assert_eq!(rows[2]["cells"][2]["content"], json!([]));
    assert_eq!(c[1]["content"]["headerCols"], 1);
    assert_eq!(report.skipped.get("table_empty"), Some(&1));
}

#[test]
fn columns_are_flattened_in_order() {
    let (page, report) = convert("columns");
    let c = &page.content;
    assert_eq!(
        types(c),
        [
            "paragraph",
            "heading",
            "paragraph",
            "bulletListItem",
            "paragraph"
        ]
    );
    assert_eq!(texts(&c[2]["content"]), "Left content");
    assert_eq!(report.lossy.get("column_list"), Some(&1));
}

#[test]
fn synced_blocks_are_flattened() {
    let (page, report) = convert("synced_block");
    assert_eq!(types(&page.content), ["callout", "callout"]);
    assert_eq!(
        texts(&page.content[0]["content"]),
        "Callout in synced block"
    );
    assert_eq!(page.content[0]["props"]["emoji"], "⭐");
    assert_eq!(report.skipped.get("synced_block_unavailable"), Some(&1));
}

#[test]
fn callouts_become_callout_blocks() {
    let (page, report) = convert("callout");
    let c = &page.content;
    assert_eq!(types(c), ["callout", "callout", "callout", "callout"]);
    // Emoji icon, background color, rich text and nested children are kept.
    assert_eq!(
        c[0]["props"],
        json!({"emoji": "⭐", "backgroundColor": "gray", "textColor": "default"})
    );
    assert_eq!(texts(&c[0]["content"]), "Lacinato kale is tasty");
    assert_eq!(c[0]["content"][0]["styles"], json!({"bold": true}));
    assert_eq!(types(c[0]["children"].as_array().unwrap()), ["paragraph"]);
    // A Notion text color is a text color; a native icon becomes the default emoji.
    assert_eq!(
        c[1]["props"],
        json!({"emoji": "💡", "backgroundColor": "default", "textColor": "red"})
    );
    assert_eq!(texts(&c[1]["content"]), "Native icon callout");
    // Custom emoji: default emoji, text unchanged.
    assert_eq!(c[2]["props"]["emoji"], "💡");
    assert_eq!(c[2]["props"]["backgroundColor"], "default");
    assert_eq!(texts(&c[2]["content"]), "Custom emoji callout");
    // No icon at all: default emoji, not counted.
    assert_eq!(
        c[3]["props"],
        json!({"emoji": "💡", "backgroundColor": "blue", "textColor": "default"})
    );
    assert_eq!(
        types(c[3]["children"].as_array().unwrap()),
        ["bulletListItem"]
    );
    assert_eq!(report.lossy.get("callout"), None);
    assert_eq!(report.lossy.get("callout_icon"), Some(&2));
}

#[test]
fn media_uses_uploaded_files_and_keeps_external_links() {
    let (page, report) = convert("media");
    let c = &page.content;
    assert_eq!(
        types(c),
        [
            "image",
            "paragraph",
            "paragraph",
            "paragraph",
            "paragraph",
            "paragraph",
            "file",
            "paragraph",
            "paragraph",
            "paragraph",
            "file",
            "paragraph"
        ]
    );
    assert_eq!(
        c[0]["props"],
        json!({
            "textAlignment": "left", "backgroundColor": "default", "name": "brocolli.jpeg",
            "url": "/api/v1/files/img1", "caption": "A brocolli", "showPreview": true
        })
    );
    assert_eq!(
        c[1]["content"][0]["href"],
        "https://website.domain/images/image.png"
    );
    assert_eq!(
        texts(&c[2]["content"]),
        "File not imported from Notion: report.pdf"
    );
    assert_eq!(texts(&c[3]["content"]), "doc.txt");
    assert_eq!(texts(&c[4]["content"]), "The spec");
    assert_eq!(c[6]["props"]["url"], "/api/v1/files/audio1");
    assert_eq!(texts(&c[7]["content"]), "https://companywebsite.com");
    assert_eq!(c[8]["content"][0]["href"], "https://developers.notion.com");
    assert_eq!(texts(&c[8]["content"]), "Notion docs");
    assert_eq!(c[10]["props"]["url"], "/api/v1/files/embed1");
    assert_eq!(report.missing_files, 1);
    assert_eq!(report.lossy.get("audio_as_file"), Some(&1));
    assert_eq!(report.lossy.get("external_image_as_link"), Some(&1));

    let (notion_page, blocks) = load("media");
    let refs = collect_file_refs(Some(&notion_page), &blocks);
    let urls: Vec<&str> = refs.iter().map(|file| file.url.as_str()).collect();
    assert_eq!(urls.len(), 4, "{urls:?}");
    assert!(urls.contains(&IMG_URL) && urls.contains(&AUDIO_URL) && urls.contains(&EMBED_URL));
    assert!(refs.iter().all(|file| !file.url.contains("website.domain")));
    let report_file = refs
        .iter()
        .find(|file| file.url.contains("report.pdf"))
        .unwrap();
    assert_eq!(report_file.source, FileSource::File);
    assert_eq!(report_file.name.as_deref(), Some("report.pdf"));
}

#[test]
fn child_pages_and_page_links() {
    let (page, report) = convert("child_pages");
    let c = &page.content;
    assert_eq!(
        types(c),
        [
            "page",
            "paragraph",
            "paragraph",
            "page",
            "page",
            "paragraph",
            "page"
        ]
    );
    assert_eq!(
        c[0]["props"],
        json!({"pageId": "01900000-0000-7000-8000-000000000002"})
    );
    assert!(c[0].get("content").is_none());
    assert_eq!(
        c[1]["content"][0]["href"],
        "https://www.notion.so/22222222222242228222222222222222"
    );
    assert_eq!(texts(&c[1]["content"]), "Not imported");
    assert_eq!(texts(&c[2]["content"]), "Untitled");
    assert_eq!(
        c[3]["props"]["pageId"],
        "01900000-0000-7000-8000-000000000003"
    );
    assert_eq!(
        c[4]["props"]["pageId"],
        "01900000-0000-7000-8000-000000000001"
    );
    assert_eq!(report.unresolved_page_links, 3);
}

#[test]
fn unsupported_blocks_are_counted() {
    let (page, report) = convert("unsupported");
    let c = &page.content;
    assert_eq!(
        types(c),
        [
            "paragraph",
            "paragraph",
            "paragraph",
            "heading",
            "paragraph",
            "heading",
            "paragraph"
        ]
    );
    assert_eq!(texts(&c[0]["content"]), "Kept child of a future block");
    assert_eq!(texts(&c[1]["content"]), "Team Sync");
    assert_eq!(texts(&c[3]["content"]), "Overview");
    assert!(
        !serde_json::to_string(c)
            .unwrap()
            .contains("Trashed paragraph")
    );
    let skipped: Vec<(&str, u32)> = report
        .skipped
        .iter()
        .map(|(k, v)| (k.as_str(), *v))
        .collect();
    assert_eq!(
        skipped,
        [
            ("breadcrumb", 1),
            ("future_block", 1),
            ("table_of_contents", 1),
            ("template", 1),
            ("unsupported:button", 1),
            ("unsupported:form", 1)
        ]
    );
    assert_eq!(report.lossy.get("tab"), Some(&1));
    assert_eq!(report.lossy.get("meeting_notes"), Some(&1));
}

#[test]
fn nested_page_title_icon_cover() {
    let (page, report) = convert("nested_page");
    assert_eq!(page.title, "Nested page");
    assert_eq!(page.icon.as_deref(), Some("🐞"));
    match &page.cover {
        Some(PageCover::File(file)) => {
            assert_eq!(file.url, COVER_URL);
            assert_eq!(file.source, FileSource::Cover);
        }
        other => panic!("{other:?}"),
    }
    let toggle = &page.content[1];
    assert_eq!(toggle["type"], "toggleListItem");
    let level4 = &toggle["children"][0]["children"][0]["children"][0];
    assert_eq!(level4["type"], "quote");
    assert_eq!(level4["children"][0]["type"], "codeBlock");
    assert_eq!(level4["children"][0]["props"]["language"], "sql");
    assert_eq!(page.content[2]["type"], "page");
    assert!(report.is_clean(), "{report:?}");
}

#[test]
fn database_row_properties() {
    let (page, report) = convert("database_row");
    assert_eq!(page.title, "Launch checklist");
    assert_eq!(page.icon, None, "external icons are not emoji");
    assert_eq!(
        page.cover,
        Some(PageCover::External {
            url: "https://example.com/cover.png".to_owned()
        })
    );
    assert_eq!(types(&page.content), ["table", "paragraph"]);
    let table = &page.content[0]["content"];
    assert_eq!(table["headerCols"], 1);
    let rows: HashMap<String, (String, Value)> = table["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            let cells = row["cells"].as_array().unwrap();
            (
                texts(&cells[0]["content"]),
                (texts(&cells[1]["content"]), cells[1]["content"].clone()),
            )
        })
        .collect();
    let value = |name: &str| rows.get(name).map(|(text, _)| text.as_str());
    assert_eq!(
        texts(&table["rows"][0]["cells"][0]["content"]),
        "Name",
        "title first"
    );
    assert_eq!(value("Name"), Some("Launch checklist"));
    assert_eq!(value("Summary"), Some("Ship v1 docs"));
    assert_eq!(value("Estimate"), Some("8"));
    assert_eq!(value("Ratio"), Some("0.25"));
    assert_eq!(value("Priority"), Some("High"));
    assert_eq!(value("Tags"), Some("Backend, Import"));
    assert_eq!(value("Status"), Some("In progress"));
    assert_eq!(value("Due date"), Some("2026-09-15 → 2026-09-18"));
    assert_eq!(
        value("Meeting"),
        Some("2026-09-15 09:00 (America/Los_Angeles)")
    );
    assert_eq!(value("Owner"), Some("Ada Lovelace, Grace Hopper"));
    assert_eq!(value("Done"), Some("Yes"));
    assert_eq!(value("Blocked"), Some("No"));
    assert_eq!(
        value("Project link"),
        Some("https://example.com/projects/launch")
    );
    assert_eq!(
        rows["Contact email"].1[0]["href"],
        "mailto:alex@example.com"
    );
    assert_eq!(value("Contact phone"), Some("+1 415 555 0123"));
    assert_eq!(value("Projects"), Some("Launch project, +1 linked page"));
    assert_eq!(
        rows["Projects"].1[0]["href"],
        "/docs/01900000-0000-7000-8000-000000000004"
    );
    assert_eq!(value("Double estimate"), Some("16"));
    assert_eq!(value("Label"), Some("Ready"));
    assert_eq!(value("Total estimate"), Some("8"));
    assert_eq!(value("Estimates"), Some("8, 5"));
    assert_eq!(value("Attachments"), Some("Project brief.pdf, Resume.pdf"));
    assert_eq!(value("Created time"), Some("2026-09-01 09:00 UTC"));
    assert_eq!(value("Created by"), Some("Ada Lovelace"));
    assert_eq!(value("Last edited by"), Some("Unknown user"));
    assert_eq!(value("Task ID"), Some("TASK-42"));
    assert_eq!(value("Office"), Some("San Francisco"));
    assert_eq!(
        value("Verification"),
        Some("verified until 2026-10-01 09:00 UTC")
    );
    for empty in [
        "Empty text",
        "No number",
        "No select",
        "Blockers",
        "Broken formula",
        "Send update",
        "Future",
    ] {
        assert!(!rows.contains_key(empty), "{empty} should be skipped");
    }
    assert_eq!(report.lossy.get("property:future_property"), Some(&1));
    assert_eq!(report.unresolved_page_links, 1);
}

#[test]
fn properties_table_is_standalone_and_optional() {
    let (page, _) = load("database_row");
    let resolver = resolver();
    let mut ctx = ConvertContext::new(&resolver);
    let table = database_row_properties_table(&page, &mut ctx).unwrap();
    assert_eq!(table["type"], "table");
    assert_eq!(table["id"].as_str().unwrap().len(), 36, "UUID block id");

    let (plain_page, _) = load("lists");
    let mut empty = plain_page.clone();
    empty.properties.clear();
    assert!(database_row_properties_table(&empty, &mut ctx).is_none());
    assert_eq!(page_title(&empty), "Untitled");
    assert_eq!(page_title(&plain_page), "Lists");
    assert_eq!(page_icon(&plain_page), None);
    assert_eq!(page_cover(&plain_page), None);
}
