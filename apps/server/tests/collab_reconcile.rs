//! `blocknote::reconcile_blocks`: a server-side content write changes only what differs, so
//! comment marks on untouched text and concurrent edits in other blocks survive it.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use base64::Engine;
use orbit_server::collab::blocknote::{
    FRAGMENT, blocks_to_update, doc_to_blocks, new_doc, reconcile_blocks,
};
use serde_json::{Value, json};
use yrs::types::text::YChange;
use yrs::updates::decoder::Decode;
use yrs::{Any, Doc, Out, ReadTxn, StateVector, Text, Transact, Update, Xml, XmlFragment, XmlOut};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/collab")
}

fn golden(name: &str) -> Value {
    serde_json::from_slice(&std::fs::read(fixtures().join(format!("{name}.json"))).unwrap())
        .unwrap()
}

fn doc_from(update: &[u8]) -> Doc {
    let doc = new_doc(None);
    doc.transact_mut()
        .apply_update(Update::decode_v1(update).unwrap())
        .unwrap();
    doc
}

fn golden_doc(name: &str) -> Doc {
    let update = base64::engine::general_purpose::STANDARD
        .decode(golden(name)["y_update"].as_str().unwrap())
        .unwrap();
    doc_from(&update)
}

/// Reconciles `doc` to `blocks`; the updates it produced.
fn reconcile(doc: &Doc, blocks: &[Value]) -> Vec<Vec<u8>> {
    let updates = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&updates);
    doc.observe_update_v1("test", move |_, event: &yrs::UpdateEvent| {
        sink.lock().unwrap().push(event.update.clone());
    })
    .unwrap();
    {
        let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
        reconcile_blocks(&mut doc.transact_mut(), &fragment, blocks);
    }
    doc.unobserve_update_v1("test").unwrap();
    std::mem::take(&mut *updates.lock().unwrap())
}

/// `blocks` as a document built from them reads back.
fn canonical(blocks: &[Value]) -> Vec<Value> {
    doc_to_blocks(&doc_from(&blocks_to_update(blocks, None)))
}

/// Every run of text with comment marks: `(text, comment keys)`, in document order.
fn comments(doc: &Doc) -> Vec<(String, Vec<String>)> {
    let txn = doc.transact();
    let fragment = txn.get_xml_fragment(FRAGMENT).unwrap();
    let mut out = Vec::new();
    let mut stack: Vec<XmlOut> = fragment.children(&txn).collect();
    stack.reverse();
    while let Some(node) = stack.pop() {
        match node {
            XmlOut::Element(element) => {
                let start = stack.len();
                stack.extend(element.children(&txn));
                stack[start..].reverse();
            }
            XmlOut::Text(text) => {
                for chunk in text.diff(&txn, YChange::identity) {
                    let Out::Any(Any::String(value)) = chunk.insert else {
                        continue;
                    };
                    let mut keys: Vec<String> = chunk
                        .attributes
                        .iter()
                        .flat_map(|attrs| attrs.keys())
                        .filter(|key| key.starts_with("comment--"))
                        .map(|key| key.to_string())
                        .collect();
                    keys.sort();
                    if !keys.is_empty() {
                        out.push((value.to_string(), keys));
                    }
                }
            }
            XmlOut::Fragment(_) => {}
        }
    }
    out
}

fn fixture_names() -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(fixtures())
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .map(|path| path.file_stem().unwrap().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names.push("marks/comment_marks".to_owned());
    names
}

#[test]
fn reconciling_to_the_same_blocks_changes_nothing() {
    for name in fixture_names() {
        let doc = golden_doc(&name);
        let blocks = doc_to_blocks(&doc);
        assert!(reconcile(&doc, &blocks).is_empty(), "{name}");
        assert_eq!(doc_to_blocks(&doc), blocks, "{name}");
    }
}

/// Changes the first text of every block, recursively.
fn edit_texts(blocks: &mut [Value]) {
    for block in blocks {
        if let Some(first) = block["content"].as_array_mut().and_then(|c| c.first_mut())
            && first["type"] == "text"
        {
            let text = first["text"].as_str().unwrap_or("").to_owned();
            first["text"] = json!(format!(
                "{}~{}",
                &text[..text.len() / 2],
                &text[text.len() / 2..]
            ));
        }
        if let Some(children) = block["children"].as_array_mut() {
            edit_texts(children);
        }
    }
}

#[test]
fn reconciling_reaches_the_target_from_any_document() {
    let names = fixture_names();
    for name in &names {
        let base = doc_to_blocks(&golden_doc(name));
        let mut edited = base.clone();
        edit_texts(&mut edited);
        if let Some(props) = edited[0]["props"].as_object_mut() {
            props.insert("textColor".into(), json!("red"));
        }
        let mut fewer: Vec<Value> = base.iter().step_by(2).cloned().collect();
        fewer.push(json!({"id": "new-1", "type": "paragraph", "content": "new"}));
        let mut reversed = base.clone();
        reversed.reverse();
        let mut retyped = base.clone();
        retyped[0]["type"] = json!("heading");
        let targets = [
            edited,
            fewer,
            reversed,
            retyped,
            vec![],
            golden(&names[0])["expected"].as_array().unwrap().clone(),
        ];
        for (index, target) in targets.iter().enumerate() {
            let doc = golden_doc(name);
            reconcile(&doc, target);
            let expected = canonical(target);
            if target.is_empty() {
                assert_eq!(doc_to_blocks(&doc).len(), 1, "{name} #{index}");
            } else {
                assert_eq!(doc_to_blocks(&doc), expected, "{name} #{index}");
            }
        }
    }
}

#[test]
fn edits_keep_comment_marks_on_untouched_text_and_new_text_gets_none() {
    let doc = golden_doc("marks/comment_marks");
    let before = comments(&doc);
    assert!(before.len() >= 3, "{before:?}");
    let mut blocks = doc_to_blocks(&doc);
    assert_eq!(
        blocks[0]["content"][0]["text"],
        "Commented text with a second overlapping thread"
    );
    blocks[0]["content"][0]["text"] =
        json!("Commented text with a second overlapping thread, edited");
    blocks[1]["content"][1]["text"] = json!(" and a new tail");
    let updates = reconcile(&doc, &blocks);
    assert_eq!(updates.len(), 1);
    assert_eq!(doc_to_blocks(&doc), canonical(&blocks));
    // Every commented run is still there and commented the same; ", edited" has no comment.
    assert_eq!(comments(&doc), before);
}

#[test]
fn concurrent_edits_in_other_blocks_survive_a_reconcile() {
    let server = doc_from(&blocks_to_update(
        golden("editor_exercise")["expected"].as_array().unwrap(),
        Some(1),
    ));
    let client = new_doc(Some(2));
    let full = server
        .transact()
        .encode_state_as_update_v1(&StateVector::default());
    client
        .transact_mut()
        .apply_update(Update::decode_v1(&full).unwrap())
        .unwrap();
    // The client colors the last block; the server has not seen it yet.
    let client_update = {
        let before = client.transact().state_vector();
        {
            let fragment = client.get_or_insert_xml_fragment(FRAGMENT);
            let mut txn = client.transact_mut();
            let Some(XmlOut::Element(group)) = fragment.get(&txn, 0) else {
                panic!()
            };
            let last = group.len(&txn) - 1;
            let Some(XmlOut::Element(container)) = group.get(&txn, last) else {
                panic!()
            };
            let Some(XmlOut::Element(paragraph)) = container.get(&txn, 0) else {
                panic!()
            };
            paragraph.insert_attribute(&mut txn, "textColor", "green");
        }
        client.transact().encode_state_as_update_v1(&before)
    };
    // The server's write edits another block.
    let mut blocks = doc_to_blocks(&server);
    let index = blocks.iter().position(|b| b["id"] == "ex-h1").unwrap();
    blocks[index]["content"] = json!([{"type": "text", "text": "Exercise, edited", "styles": {}}]);
    let server_updates = reconcile(&server, &blocks);
    server
        .transact_mut()
        .apply_update(Update::decode_v1(&client_update).unwrap())
        .unwrap();
    for update in &server_updates {
        client
            .transact_mut()
            .apply_update(Update::decode_v1(update).unwrap())
            .unwrap();
    }
    let merged = doc_to_blocks(&server);
    assert_eq!(doc_to_blocks(&client), merged);
    assert_eq!(merged[index]["content"][0]["text"], "Exercise, edited");
    assert_eq!(merged.last().unwrap()["props"]["textColor"], "green");
    assert_eq!(merged.last().unwrap()["id"], "ex-last");
}
