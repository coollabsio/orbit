//! Parity of the server's BlockNote JSON <-> Yjs converter with BlockNote + y-prosemirror.
//!
//! The goldens in `tests/fixtures/collab` are written by the web test `collabParity.test.ts`
//! from the real editor schema: `blocks`, the Yjs update BlockNote builds for them (`y_update`)
//! and what BlockNote reads back (`expected`). This test asserts that the Rust converter
//! 1. reads BlockNote's documents exactly like BlockNote (`fragment_to_blocks(y_update)`),
//! 2. builds the same document structure from `blocks` (XML elements, attributes, text runs and
//!    their formatting) and reads its own document back as `expected`.
//!
//! With `UPDATE_COLLAB_GOLDENS=1` it writes the Rust-built updates to `fixtures/collab/rust/`,
//! which the web test loads into BlockNote (`yDocToBlocks(rust update) == expected`).

use std::path::{Path, PathBuf};

use base64::Engine;
use orbit_server::collab::blocknote::{self, FRAGMENT, blocks_to_update, doc_to_blocks};
use serde_json::{Map, Value, json};
use yrs::types::text::YChange;
use yrs::updates::decoder::Decode;
use yrs::{Any, Out, ReadTxn, Text, Transact, Update, WriteTxn, Xml, XmlFragment, XmlOut};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/collab")
}

fn goldens() -> Vec<(String, Value)> {
    let mut files: Vec<_> = std::fs::read_dir(fixtures())
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
            let value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            (name, value)
        })
        .collect()
}

fn doc_from(update: &[u8]) -> yrs::Doc {
    let doc = blocknote::new_doc(Some(99));
    doc.transact_mut()
        .apply_update(Update::decode_v1(update).unwrap())
        .unwrap();
    doc
}

fn base64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn unbase64(text: &str) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD
        .decode(text)
        .unwrap()
}

/// The document's structure: element tags with their (defined) attributes, text as runs of
/// `{text, attrs}`. Y item ids and client ids are left out.
fn dump(doc: &yrs::Doc) -> Value {
    let txn = doc.transact();
    let fragment = txn.get_xml_fragment(FRAGMENT).expect("fragment");
    Value::Array(
        fragment
            .children(&txn)
            .map(|child| dump_node(&txn, child))
            .collect(),
    )
}

fn any_json(any: &Any) -> Value {
    let value = serde_json::to_value(any).unwrap();
    normalize_numbers(value)
}

fn normalize_numbers(value: Value) -> Value {
    match value {
        Value::Number(number) => json!(number.as_f64().unwrap()),
        Value::Array(items) => Value::Array(items.into_iter().map(normalize_numbers).collect()),
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(key, value)| (key, normalize_numbers(value)))
                .collect(),
        ),
        other => other,
    }
}

fn dump_node<T: ReadTxn>(txn: &T, node: XmlOut) -> Value {
    match node {
        XmlOut::Element(element) => {
            let mut attrs = Map::new();
            for (name, value) in element.attributes(txn) {
                match value {
                    // y-prosemirror writes `undefined` attributes; missing means the same.
                    Out::Any(Any::Undefined) => {}
                    Out::Any(any) => {
                        attrs.insert(name.to_owned(), any_json(&any));
                    }
                    other => {
                        attrs.insert(name.to_owned(), json!(other.to_string(txn)));
                    }
                }
            }
            // Block ids are whatever the input had (or random): not structure.
            if element.tag().as_ref() != "blockContainer" {
                json!({
                    "tag": element.tag().to_string(),
                    "attrs": attrs,
                    "children": element.children(txn).map(|child| dump_node(txn, child)).collect::<Vec<_>>(),
                })
            } else {
                json!({
                    "tag": "blockContainer",
                    "id": attrs.get("id").cloned().unwrap_or(Value::Null),
                    "children": element.children(txn).map(|child| dump_node(txn, child)).collect::<Vec<_>>(),
                })
            }
        }
        XmlOut::Text(text) => {
            let runs: Vec<Value> = text
                .diff(txn, YChange::identity)
                .into_iter()
                .map(|chunk| {
                    let insert = match chunk.insert {
                        Out::Any(Any::String(value)) => json!(value.to_string()),
                        other => json!(format!("{other:?}")),
                    };
                    let mut attrs = Map::new();
                    for (name, value) in chunk.attributes.iter().flat_map(|attrs| attrs.iter()) {
                        attrs.insert(name.to_string(), any_json(value));
                    }
                    json!({ "insert": insert, "attrs": attrs })
                })
                .collect();
            json!({ "text": runs })
        }
        XmlOut::Fragment(_) => Value::Null,
    }
}

#[test]
fn goldens_cover_the_notion_fixtures_and_the_editor_exercise() {
    let names: Vec<String> = goldens().into_iter().map(|(name, _)| name).collect();
    assert!(names.contains(&"editor_exercise".to_owned()), "{names:?}");
    let notion = std::fs::read_dir(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/notion/expected"),
    )
    .unwrap()
    .count();
    assert_eq!(
        names
            .iter()
            .filter(|name| name.starts_with("notion_"))
            .count(),
        notion,
        "regenerate with UPDATE_COLLAB_GOLDENS=1 bun test (apps/web)"
    );
}

#[test]
fn blocknote_documents_project_exactly_like_blocknote() {
    for (name, golden) in goldens() {
        let doc = doc_from(&unbase64(golden["y_update"].as_str().unwrap()));
        let blocks = doc_to_blocks(&doc);
        assert_eq!(
            Value::Array(blocks),
            golden["expected"],
            "{name}: fragment_to_blocks(BlockNote's document) differs from yDocToBlocks"
        );
    }
}

#[test]
fn rust_built_documents_match_blocknote_structure_and_projection() {
    let update_goldens = std::env::var("UPDATE_COLLAB_GOLDENS").as_deref() == Ok("1");
    let rust_dir = fixtures().join("rust");
    for (name, golden) in goldens() {
        let blocks = golden["blocks"].as_array().unwrap().clone();
        let update = blocks_to_update(&blocks, Some(2));
        let rust_doc = doc_from(&update);
        let js_doc = doc_from(&unbase64(golden["y_update"].as_str().unwrap()));
        assert_eq!(
            dump(&rust_doc),
            dump(&js_doc),
            "{name}: the Rust-built document differs from BlockNote's"
        );
        assert_eq!(
            Value::Array(doc_to_blocks(&rust_doc)),
            golden["expected"],
            "{name}: Rust round trip"
        );
        let path = rust_dir.join(format!("{name}.json"));
        if update_goldens {
            std::fs::create_dir_all(&rust_dir).unwrap();
            std::fs::write(
                &path,
                format!(
                    "{}\n",
                    serde_json::to_string_pretty(&json!({ "update": base64(&update) })).unwrap()
                ),
            )
            .unwrap();
        }
        // The checked-in Rust update (read by the web test) still means the same document.
        let stored: Value = serde_json::from_slice(
            &std::fs::read(&path)
                .unwrap_or_else(|_| panic!("missing {path:?}: run with UPDATE_COLLAB_GOLDENS=1")),
        )
        .unwrap();
        let stored_doc = doc_from(&unbase64(stored["update"].as_str().unwrap()));
        assert_eq!(
            dump(&stored_doc),
            dump(&rust_doc),
            "{name}: stale rust golden"
        );
    }
}

#[test]
fn replacing_a_document_keeps_one_consistent_tree() {
    let (_, golden) = goldens()
        .into_iter()
        .find(|(name, _)| name == "editor_exercise")
        .unwrap();
    let doc = doc_from(&unbase64(golden["y_update"].as_str().unwrap()));
    let replacement = vec![json!({"id": "r1", "type": "paragraph", "content": "Replaced"})];
    {
        let mut txn = doc.transact_mut();
        let fragment = txn.get_or_insert_xml_fragment(FRAGMENT);
        blocknote::replace_blocks(&mut txn, &fragment, &replacement);
    }
    let blocks = doc_to_blocks(&doc);
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0]["id"], "r1");
    assert_eq!(
        blocks[0]["content"],
        json!([{"type": "text", "text": "Replaced", "styles": {}}])
    );
}

/// Comment threads anchor to text with BlockNote's `comment` mark (`comment--<hash>` attributes,
/// written only by editors with comments). The server reads such documents like BlockNote (the
/// marks never reach the JSON) and keeps them through a repair (`marks/comment_marks.json`, written
/// by `collabParity.test.ts`).
#[test]
fn comment_marks_are_invisible_to_the_projection_and_survive_repair() {
    let golden: Value = serde_json::from_slice(
        &std::fs::read(fixtures().join("marks/comment_marks.json")).unwrap(),
    )
    .unwrap();
    let doc = doc_from(&unbase64(golden["y_update"].as_str().unwrap()));
    let comment_keys = |doc: &yrs::Doc| {
        let dumped = dump(doc).to_string();
        dumped.matches("\"comment--").count()
    };
    let before = comment_keys(&doc);
    assert!(before >= 4, "{}", dump(&doc));
    assert_eq!(Value::Array(doc_to_blocks(&doc)), golden["expected"]);
    {
        let mut txn = doc.transact_mut();
        let fragment = txn.get_or_insert_xml_fragment(FRAGMENT);
        assert!(!orbit_server::collab::sanitize::repair_document(
            &mut txn, &fragment
        ));
    }
    assert_eq!(comment_keys(&doc), before);
    // The same blocks built by the server (no marks) project identically.
    let blocks = golden["blocks"].as_array().unwrap().clone();
    let rust_doc = doc_from(&blocks_to_update(&blocks, Some(2)));
    assert_eq!(Value::Array(doc_to_blocks(&rust_doc)), golden["expected"]);
    assert_eq!(comment_keys(&rust_doc), 0);
}
