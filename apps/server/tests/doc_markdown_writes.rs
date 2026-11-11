//! Token API Markdown writes onto live documents (`PATCH /api/public/v1/.../docs/{id}`): the
//! Markdown a client read writes back without loss, and an edit changes only what it edits.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use orbit_platform::{Id, TimestampMillis};
use orbit_server::collab::blocknote::FRAGMENT;
use orbit_server::repositories::api_tokens::{ApiTokenRepository, ApiTokenScope};
use orbit_server::repositories::pages::{CreatePage, PageRepository, SpaceRequest};
use serde_json::{Value, json};
use yrs::types::text::YChange;
use yrs::{Any, Doc, Out, ReadTxn, Text, Transact, Xml, XmlElementRef, XmlFragment, XmlOut};

mod collab_support;
#[allow(dead_code)] // `collab_support` needs it.
mod shared_postgres;

use collab_support::{Client, Server, Users, wait_until};

struct Live {
    server: Server,
    users: Users,
    token: String,
    pages: PageRepository,
}

impl Live {
    async fn start(root: &tempfile::TempDir) -> Self {
        Self::on(Server::start(root, collab_support::fast_config()).await).await
    }

    async fn on(server: Server) -> Self {
        let users = collab_support::setup(&server).await;
        let project: String = sqlx::query_scalar(
            "SELECT id FROM projects WHERE workspace_id = $1 ORDER BY created_at",
        )
        .bind(&users.workspace)
        .fetch_one(server.database.pool())
        .await
        .unwrap();
        let token = ApiTokenRepository::new(server.database.clone())
            .create(
                users.workspace.parse().unwrap(),
                users.owner_id,
                "docs".into(),
                vec![project.parse().unwrap()],
                &[ApiTokenScope::DocsWrite],
                false,
                None,
                "test",
                TimestampMillis::now(),
            )
            .await
            .unwrap()
            .token;
        let pages = PageRepository::new(server.database.clone());
        Self {
            server,
            users,
            token,
            pages,
        }
    }

    fn workspace(&self) -> Id {
        self.users.workspace.parse().unwrap()
    }

    async fn call(&self, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
        let mut request = self
            .server
            .http
            .request(
                method.parse().unwrap(),
                format!(
                    "{}/api/public/v1/workspaces/{}{path}",
                    self.server.base, self.users.workspace
                ),
            )
            .bearer_auth(&self.token);
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.unwrap();
        let status = response.status().as_u16();
        let text = response.text().await.unwrap();
        (status, serde_json::from_str(&text).unwrap_or(Value::Null))
    }

    async fn page(&self, title: &str, parent: Option<Id>, content: &[Value]) -> Id {
        self.pages
            .create_page_with_content(
                self.workspace(),
                self.users.owner_id,
                CreatePage {
                    kind: orbit_server::repositories::pages::PageKind::Page,
                    parent_id: parent,
                    space: SpaceRequest::default(),
                    title: title.into(),
                    icon: None,
                    position: None,
                },
                content,
                "test",
                TimestampMillis::now(),
            )
            .await
            .unwrap()
            .id
    }

    async fn content_json(&self, page: Id) -> String {
        sqlx::query_scalar("SELECT content_json FROM pages WHERE id = $1")
            .bind(page.to_string())
            .fetch_one(self.server.database.pool())
            .await
            .unwrap()
    }

    async fn logged(&self, page: Id) -> i64 {
        collab_support::scalar_i64(
            &self.server,
            "SELECT COUNT(*) FROM page_collab_updates WHERE page_id = $1",
            &page.to_string(),
        )
        .await
    }

    async fn read(&self, page: Id) -> Value {
        let (status, doc) = self.call("GET", &format!("/docs/{page}"), None).await;
        assert_eq!(status, 200, "{doc}");
        doc
    }

    async fn write(&self, page: Id, version: &Value, markdown: &str) -> (u16, Value) {
        self.call(
            "PATCH",
            &format!("/docs/{page}"),
            Some(json!({"expected_version": version, "markdown": markdown})),
        )
        .await
    }
}

/// Every block type of the editor schema, with the props and styles Markdown cannot show.
fn every_block(sub_page: Id, owner: Id) -> Vec<Value> {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/collab/editor_exercise.json");
    let golden: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let mut blocks = golden["expected"].as_array().unwrap().clone();
    for block in &mut blocks {
        if block["type"] == "page" {
            block["props"]["pageId"] = json!(sub_page.to_string());
        }
    }
    blocks.extend([
        json!({"id": "x-numbered", "type": "numberedListItem", "props": {"start": 5}, "content": "five"}),
        json!({"id": "x-numbered-2", "type": "numberedListItem", "content": "six"}),
        json!({"id": "x-empty-2", "type": "paragraph", "content": []}),
        json!({"id": "x-parent", "type": "paragraph", "props": {"textColor": "orange"}, "content": "parent", "children": [
            {"id": "x-child", "type": "paragraph", "content": "child of a paragraph"},
        ]}),
        json!({"id": "x-video", "type": "video", "props": {"url": "https://example.com/v.mp4", "name": "clip", "caption": "A clip"}}),
        json!({"id": "x-audio", "type": "audio", "props": {"url": "https://example.com/a.mp3"}}),
        json!({"id": "x-mention", "type": "paragraph", "content": [
            {"type": "text", "text": "Ask ", "styles": {}},
            {"type": "mention", "props": {"userId": owner.to_string(), "name": "Olivia Owner"}},
            {"type": "text", "text": " and ", "styles": {"textColor": "blue"}},
            {"type": "mention", "props": {"userId": Id::new_v7().to_string(), "name": "Gone Person"}},
        ]}),
        json!({"id": "x-toggle-h", "type": "heading", "props": {"level": 3, "isToggleable": true}, "content": "Details", "children": [
            {"id": "x-toggle-h-c", "type": "bulletListItem", "content": "hidden detail"},
        ]}),
        json!({"id": "x-colored", "type": "paragraph", "props": {"backgroundColor": "red", "textAlignment": "justify"}, "content": "Colored and justified"}),
    ]);
    blocks
}

/// The content element of the block with `id` in a client's document.
fn content_element<T: ReadTxn>(txn: &T, id: &str) -> XmlElementRef {
    let fragment = txn.get_xml_fragment(FRAGMENT).unwrap();
    let mut stack: Vec<XmlOut> = fragment.children(txn).collect();
    while let Some(node) = stack.pop() {
        if let XmlOut::Element(element) = node {
            if element.tag().as_ref() == "blockContainer"
                && element
                    .get_attribute(txn, "id")
                    .map(|id| id.to_string(txn))
                    .as_deref()
                    == Some(id)
                && let Some(XmlOut::Element(content)) = element.get(txn, 0)
            {
                return content;
            }
            stack.extend(element.children(txn));
        }
    }
    panic!("no block {id}")
}

/// Puts a comment mark on `len` UTF-16 units of the block's first text from `at`.
fn comment(doc: &Doc, txn: &mut yrs::TransactionMut, id: &str, thread: &str, at: u32, len: u32) {
    let _ = doc;
    let element = content_element(txn, id);
    let Some(XmlOut::Text(text)) = element.get(txn, 0) else {
        panic!("{id} has no text")
    };
    let mut value = std::collections::HashMap::new();
    value.insert("orphan".to_owned(), Any::Bool(false));
    value.insert("threadId".to_owned(), Any::from(thread.to_owned()));
    let mut attrs = yrs::types::Attrs::new();
    attrs.insert(
        Arc::from(format!("comment--{thread}")),
        Any::Map(Arc::new(value)),
    );
    text.format(txn, at, len, attrs);
}

/// Every commented run: `(block id, text, comment keys)`.
fn comments(doc: &Doc) -> Vec<(String, String, Vec<String>)> {
    let txn = doc.transact();
    let fragment = txn.get_xml_fragment(FRAGMENT).unwrap();
    let mut out = Vec::new();
    let mut stack: Vec<(XmlOut, String)> = fragment
        .children(&txn)
        .map(|n| (n, String::new()))
        .collect();
    while let Some((node, block)) = stack.pop() {
        match node {
            XmlOut::Element(element) => {
                let block = element
                    .get_attribute(&txn, "id")
                    .map_or(block, |id| id.to_string(&txn));
                stack.extend(element.children(&txn).map(|child| (child, block.clone())));
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
                    if !keys.is_empty() {
                        keys.sort();
                        out.push((block.clone(), value.to_string(), keys));
                    }
                }
            }
            XmlOut::Fragment(_) => {}
        }
    }
    out.sort();
    out
}

/// A live page with every block type and three comment threads, as the token reads it.
async fn commented_page(live: &Live) -> (Id, Client, Value) {
    let sub_page = live.page("Launch notes", None, &[]).await;
    let blocks = every_block(sub_page, live.users.owner_id);
    let page = live.page("Everything", None, &blocks).await;
    // The sub-page is a child of the page (its page block links it).
    sqlx::query("UPDATE pages SET parent_id = $1 WHERE id = $2")
        .bind(page.to_string())
        .bind(sub_page.to_string())
        .execute(live.server.database.pool())
        .await
        .unwrap();
    let mut client = Client::open(
        &live.server,
        &live.users,
        &live.users.owner,
        &page.to_string(),
    )
    .await;
    client
        .edit(|doc, txn| {
            comment(doc, txn, "ex-h1", "t1", 0, 8);
            comment(doc, txn, "ex-p1", "t2", 0, 5);
            comment(doc, txn, "ex-last", "t3", 4, 3);
        })
        .await;
    wait_until("the comments are stored", || async {
        live.logged(page).await > 0
    })
    .await;
    assert_eq!(comments(&client.doc).len(), 3);
    let read = live.read(page).await;
    (page, client, read)
}

#[tokio::test]
async fn writing_back_the_markdown_of_a_live_doc_changes_nothing() {
    let root = tempfile::TempDir::new().unwrap();
    let live = Live::start(&root).await;
    let (page, client, read) = commented_page(&live).await;
    let markdown = read["markdown"].as_str().unwrap().to_owned();
    assert!(markdown.contains("[Launch notes](/docs/"), "{markdown}");
    let stored = live.content_json(page).await;
    let logged = live.logged(page).await;
    let audited = || async {
        collab_support::scalar_i64(
            &live.server,
            "SELECT COUNT(*) FROM audit_events WHERE resource_id = $1 AND action = 'page.updated'",
            &page.to_string(),
        )
        .await
    };
    let audits = audited().await;

    let (status, written) = live.write(page, &read["version"], &markdown).await;
    assert_eq!(status, 200, "{written}");
    assert_eq!(written["markdown"], read["markdown"]);
    // Nothing changed, so the version stays and a client holding it can still write.
    assert_eq!(written["version"], read["version"]);
    assert_eq!(audited().await, audits, "the no-op write was audited");
    assert_eq!(live.read(page).await["version"], read["version"]);
    assert_eq!(
        live.content_json(page).await,
        stored,
        "content_json changed"
    );
    assert_eq!(
        live.logged(page).await,
        logged,
        "the live document got an update"
    );
    // A new editor sees every comment mark.
    let fresh = Client::open(
        &live.server,
        &live.users,
        &live.users.member,
        &page.to_string(),
    )
    .await;
    assert_eq!(comments(&fresh.doc), comments(&client.doc));
    assert_eq!(comments(&fresh.doc).len(), 3);
    live.server.stop().await;
}

#[tokio::test]
async fn an_edit_changes_only_the_blocks_it_edits() {
    let root = tempfile::TempDir::new().unwrap();
    let live = Live::start(&root).await;
    let (page, mut client, read) = commented_page(&live).await;
    let before: Vec<Value> = serde_json::from_str(&live.content_json(page).await).unwrap();
    let markdown = read["markdown"].as_str().unwrap();
    let edited = markdown
        .replacen("second line", "2nd line", 1)
        .replacen("\ninside\n", "\ninside, edited\n", 1)
        .replacen("# Exercise\n", "# Exercise\n\nA new paragraph\n", 1)
        .replacen("> *Quoted*\n\n", "", 1)
        .replacen("- hidden detail", "- hidden detail, edited", 1);
    let logged = live.logged(page).await;
    let (status, written) = live.write(page, &read["version"], &edited).await;
    assert_eq!(status, 200, "{written}");
    assert_eq!(live.logged(page).await, logged + 1);
    let after: Vec<Value> = serde_json::from_str(&live.content_json(page).await).unwrap();
    assert_eq!(after[1]["content"][0]["text"], "A new paragraph");
    let changed = ["ex-p1", "ex-h2-c", "ex-quote", "x-toggle-h-c"];
    let unchanged = |blocks: &[Value]| -> Vec<Value> {
        let mut out = Vec::new();
        let mut stack: Vec<(&Value, usize)> = blocks.iter().map(|b| (b, 0)).collect();
        while let Some((block, depth)) = stack.pop() {
            stack.extend(
                block["children"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|c| (c, depth + 1)),
            );
            if !changed.contains(&block["id"].as_str().unwrap_or(""))
                && block["content"][0]["text"] != "A new paragraph"
            {
                let mut block = block.clone();
                block["children"] = json!(block["children"].as_array().map_or(0, Vec::len));
                out.push(json!([depth, block]));
            }
        }
        out.sort_by_key(|entry| entry[1]["id"].to_string());
        out
    };
    assert_eq!(unchanged(&after), unchanged(&before));
    let find = |blocks: &[Value], id: &str| -> Value {
        let mut stack: Vec<&Value> = blocks.iter().collect();
        while let Some(block) = stack.pop() {
            if block["id"] == id {
                return block.clone();
            }
            stack.extend(block["children"].as_array().into_iter().flatten());
        }
        Value::Null
    };
    let paragraph = find(&after, "ex-p1");
    assert_eq!(paragraph["props"], find(&before, "ex-p1")["props"]);
    assert_eq!(
        paragraph["content"][4]["styles"],
        json!({"textColor": "red"})
    );
    assert_eq!(
        find(&after, "ex-h2")["children"][0]["content"][0]["text"],
        "inside, edited"
    );
    assert_eq!(
        find(&after, "x-toggle-h")["children"][0]["content"][0]["text"],
        "hidden detail, edited"
    );
    assert_eq!(find(&after, "ex-quote"), Value::Null);
    // The open editor got the edit and kept every comment, the edited paragraph's too.
    let expected = comments(&client.doc);
    client
        .pump(Duration::from_secs(5), |client, _| {
            client.text().contains("2nd line")
        })
        .await;
    assert!(client.closed.is_none());
    assert_eq!(comments(&client.doc), expected);
    let fresh = Client::open(
        &live.server,
        &live.users,
        &live.users.member,
        &page.to_string(),
    )
    .await;
    assert_eq!(comments(&fresh.doc), expected);
    live.server.stop().await;
}

#[tokio::test]
async fn a_live_edit_in_another_block_survives_a_markdown_write() {
    let root = tempfile::TempDir::new().unwrap();
    let live = Live::start(&root).await;
    let (page, mut client, _) = commented_page(&live).await;
    // An editor colors a block before the token's read: Markdown cannot show it.
    client
        .edit(|_, txn| {
            content_element(txn, "x-colored").insert_attribute(txn, "textColor", "green");
        })
        .await;
    wait_until("the color is stored", || async {
        live.read(page).await["content"]
            .to_string()
            .contains("\"green\"")
    })
    .await;
    let read = live.read(page).await;
    // Another color change is still on its way while the token writes.
    let late = {
        let before = client.doc.transact().state_vector();
        {
            let mut txn = client.doc.transact_mut();
            content_element(&txn, "ex-last").insert_attribute(&mut txn, "textColor", "purple");
        }
        client.doc.transact().encode_state_as_update_v1(&before)
    };
    let markdown =
        read["markdown"]
            .as_str()
            .unwrap()
            .replacen("# Exercise", "# Exercise, edited", 1);
    let (status, written) = live.write(page, &read["version"], &markdown).await;
    assert_eq!(status, 200, "{written}");
    client
        .send(yrs::sync::Message::Sync(yrs::sync::SyncMessage::Update(
            late,
        )))
        .await;
    wait_until("the late color is stored", || async {
        live.read(page).await["content"]
            .to_string()
            .contains("\"purple\"")
    })
    .await;
    let content = live.read(page).await["content"].clone();
    let block = |id: &str| {
        content
            .as_array()
            .unwrap()
            .iter()
            .find(|b| b["id"] == id)
            .unwrap()
            .clone()
    };
    assert_eq!(block("ex-h1")["content"][0]["text"], "Exercise, edited");
    assert_eq!(block("x-colored")["props"]["textColor"], "green");
    assert_eq!(block("x-colored")["props"]["backgroundColor"], "red");
    assert_eq!(block("ex-last")["props"]["textColor"], "purple");
    assert!(client.closed.is_none());
    live.server.stop().await;
}

#[tokio::test]
async fn deep_and_large_markdown_writes_finish_quickly() {
    let root = tempfile::TempDir::new().unwrap();
    let live = Live::start(&root).await;
    let page = live
        .page(
            "Big",
            None,
            &[json!({"type": "paragraph", "content": "start"})],
        )
        .await;
    let _client = Client::open(
        &live.server,
        &live.users,
        &live.users.owner,
        &page.to_string(),
    )
    .await;
    let budget = Duration::from_secs(if cfg!(debug_assertions) { 30 } else { 5 });
    let started = Instant::now();
    let deep: String = (0..60)
        .map(|level| format!("{}- level {level}\n", "  ".repeat(level)))
        .collect();
    let read = live.read(page).await;
    let (status, written) = live.write(page, &read["version"], &deep).await;
    assert_eq!(status, 200, "{written}");
    let (status, again) = live
        .write(
            page,
            &written["version"],
            written["markdown"].as_str().unwrap(),
        )
        .await;
    assert_eq!(status, 200, "{again}");
    // 50,000 nested quotes (100 KB) onto the list, then edited: no crash, nesting capped.
    let quotes = format!("{} deep", "> ".repeat(50_000));
    let (status, quoted) = live.write(page, &again["version"], &quotes).await;
    assert_eq!(status, 200, "{quoted}");
    let (status, again) = live
        .write(page, &quoted["version"], &quotes.replace("deep", "deeper"))
        .await;
    assert_eq!(status, 200, "{again}");
    assert!(again["markdown"].as_str().unwrap().contains("deeper"));
    let deep_done = started.elapsed();

    // Just under the 200,000-byte limit of a doc write.
    let text = |word: &str| -> String {
        (0..1600)
            .map(|index| format!("Paragraph {index} {}\n\n", word.repeat(18)))
            .collect()
    };
    let first = text("lorem ");
    assert!(
        first.len() > 190_000 && first.len() < 200_000,
        "{}",
        first.len()
    );
    let started = Instant::now();
    let (status, written) = live.write(page, &again["version"], &first).await;
    assert_eq!(status, 200, "{written}");
    let (status, rewritten) = live.write(page, &written["version"], &text("ipsum ")).await;
    assert_eq!(status, 200, "{rewritten}");
    let edited = text("ipsum ").replacen("Paragraph 1000 ", "Paragraph one thousand ", 1);
    let (status, once) = live.write(page, &rewritten["version"], &edited).await;
    assert_eq!(status, 200, "{once}");
    let large_done = started.elapsed();
    // 4,000 one-line list items (the most a doc can have), then all of them turned into
    // paragraphs (every block new).
    let items: String = (0..4_000)
        .map(|index| format!("- item {index}\n"))
        .collect();
    let started = Instant::now();
    let (status, listed) = live.write(page, &once["version"], &items).await;
    assert_eq!(status, 200, "{listed}");
    let (status, retyped) = live
        .write(page, &listed["version"], &items.replace("- ", "\n"))
        .await;
    assert_eq!(status, 200, "{retyped}");
    let items_done = started.elapsed();
    eprintln!(
        "60 levels: {deep_done:?}; 200 KB write, full rewrite, one-line edit: {large_done:?}; \
         4,000 items, then all retyped: {items_done:?}"
    );
    assert!(
        deep_done + large_done + items_done < budget,
        "{deep_done:?} + {large_done:?} + {items_done:?}"
    );
    assert_eq!(once["content"].as_array().unwrap().len(), 1600);
    assert_eq!(once["content"][999]["id"], rewritten["content"][999]["id"]);
    assert_eq!(retyped["content"][0]["type"], "paragraph");
    live.server.stop().await;
}

#[tokio::test]
async fn a_large_markdown_write_merges_before_it_takes_the_write_lock() {
    let root = tempfile::TempDir::new().unwrap();
    let live = Live::start(&root).await;
    let page = live
        .page(
            "Big",
            None,
            &[json!({"type": "paragraph", "content": "start"})],
        )
        .await;
    let _client = Client::open(
        &live.server,
        &live.users,
        &live.users.owner,
        &page.to_string(),
    )
    .await;
    let items: String = (0..4_000)
        .map(|index| format!("- item {index}\n"))
        .collect();
    let read = live.read(page).await;
    let (status, listed) = live.write(page, &read["version"], &items).await;
    assert_eq!(status, 200, "{listed}");

    // Every item moved (the slowest merge of this size); meanwhile, other writers go on.
    let reversed: String = (0..4_000)
        .rev()
        .map(|index| format!("- item {index}\n"))
        .collect();
    let started = Instant::now();
    let mut longest_wait = Duration::ZERO;
    let (status, written) = {
        let write = live.write(page, &listed["version"], &reversed);
        tokio::pin!(write);
        loop {
            tokio::select! {
                result = &mut write => break result,
                () = tokio::time::sleep(Duration::from_millis(5)) => {
                    let waiting = Instant::now();
                    let tx = live.server.database.immediate_transaction().await.unwrap();
                    longest_wait = longest_wait.max(waiting.elapsed());
                    tx.rollback().await.unwrap();
                }
            }
        }
    };
    let took = started.elapsed();
    assert_eq!(status, 200, "{written}");
    assert!(
        written["markdown"]
            .as_str()
            .unwrap()
            .starts_with("- item 3999\n")
    );
    eprintln!("write: {took:?}; longest wait of another writer: {longest_wait:?}");
    assert!(longest_wait * 4 < took, "{longest_wait:?} of {took:?}");
    live.server.stop().await;
}

#[tokio::test]
async fn an_edit_another_server_logs_during_a_markdown_merge_makes_the_write_conflict() {
    if !orbit_platform::testing_postgres() {
        return; // other servers write the same log on Postgres only
    }
    let root = tempfile::TempDir::new().unwrap();
    let live = Live::start(&root).await;
    let mut content = vec![json!({"id": "typed", "type": "paragraph", "content": "typed"})];
    content.extend((0..3_999).map(|index| {
        json!({"id": format!("item-{index}"), "type": "bulletListItem", "content": format!("item {index}")})
    }));
    let page = live.page("Big", None, &content).await;
    let editor = Client::open(
        &live.server,
        &live.users,
        &live.users.owner,
        &page.to_string(),
    )
    .await;
    let read = live.read(page).await;
    let reversed = |markdown: &str| {
        let (typed, items) = markdown.split_once('\n').unwrap();
        let mut lines: Vec<&str> = items.lines().filter(|line| !line.is_empty()).collect();
        lines.reverse();
        format!("{typed}\n\n{}\n", lines.join("\n"))
    };
    // What another server's editor typed, as that server logs it (without projecting it).
    let log_typing = || async {
        let before = editor.doc.transact().state_vector();
        {
            let mut txn = editor.doc.transact_mut();
            collab_support::paragraph_text(&editor.doc, &mut txn, 0).insert(&mut txn, 0, "x");
        }
        let update = editor.doc.transact().encode_state_as_update_v1(&before);
        let mut tx = live.server.database.immediate_transaction().await.unwrap();
        sqlx::query(
            "INSERT INTO page_collab_updates (page_id, workspace_id, data, user_id, created_at) \
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(page.to_string())
        .bind(&live.users.workspace)
        .bind(update)
        .bind(live.users.owner_id.to_string())
        .bind(TimestampMillis::now().as_millis())
        .execute(&mut *tx)
        .await
        .unwrap();
        tx.commit().await.unwrap();
    };

    let mut typed = 0;
    let markdown = reversed(read["markdown"].as_str().unwrap());
    let (status, conflict) = {
        let write = live.write(page, &read["version"], &markdown);
        tokio::pin!(write);
        loop {
            tokio::select! {
                result = &mut write => break result,
                () = tokio::time::sleep(Duration::from_millis(20)) => {
                    log_typing().await;
                    typed += 1;
                }
            }
        }
    };
    assert_eq!(status, 409, "{conflict}");
    let stored = live.read(page).await;
    let expected = format!("{}typed", "x".repeat(typed));
    assert_eq!(
        stored["content"][0]["content"][0]["text"],
        expected.as_str()
    );
    assert_eq!(stored["content"][1]["id"], "item-0", "nothing of the write");

    // Written again on what is there now, it goes through and keeps the typing.
    let again = reversed(stored["markdown"].as_str().unwrap());
    let (status, written) = live.write(page, &stored["version"], &again).await;
    assert_eq!(status, 200, "{written}");
    assert_eq!(
        written["content"][0]["content"][0]["text"],
        expected.as_str()
    );
    assert_eq!(written["content"][1]["id"], "item-3998");
    drop(editor);
    live.server.stop().await;
}

#[tokio::test]
async fn a_doc_write_over_the_block_limit_is_refused_and_changes_nothing() {
    let root = tempfile::TempDir::new().unwrap();
    let live = Live::start(&root).await;
    let page = live
        .page(
            "Doc",
            None,
            &[json!({"type": "paragraph", "content": "start"})],
        )
        .await;
    let _client = Client::open(
        &live.server,
        &live.users,
        &live.users.owner,
        &page.to_string(),
    )
    .await;
    let items = |count: usize| -> String {
        (0..count)
            .map(|index| format!("- item {index}\n"))
            .collect()
    };
    let read = live.read(page).await;
    let logged = live.logged(page).await;
    let (status, problem) = live.write(page, &read["version"], &items(4_001)).await;
    assert_eq!(status, 413, "{problem}");
    assert_eq!(problem["code"], "doc_too_large");
    let after = live.read(page).await;
    assert_eq!(after["version"], read["version"]);
    assert_eq!(after["markdown"], read["markdown"]);
    assert_eq!(live.logged(page).await, logged);
    let (status, written) = live.write(page, &read["version"], &items(4_000)).await;
    assert_eq!(status, 200, "{written}");

    // A doc that is over the limit already (from the editor): its content cannot be written
    // as Markdown, its title can.
    let blocks: Vec<Value> = (0..4_001)
        .map(|index| json!({"type": "paragraph", "content": format!("line {index}")}))
        .collect();
    let big = live.page("Big", None, &blocks).await;
    let read = live.read(big).await;
    let (status, problem) = live.write(big, &read["version"], "short\n").await;
    assert_eq!(status, 413, "{problem}");
    assert_eq!(live.read(big).await["markdown"], read["markdown"]);
    let (status, renamed) = live
        .call(
            "PATCH",
            &format!("/docs/{big}"),
            Some(json!({"expected_version": read["version"], "title": "Renamed"})),
        )
        .await;
    assert_eq!(status, 200, "{renamed}");

    // A new doc too.
    let (status, problem) = live
        .call(
            "POST",
            "/docs",
            Some(json!({"title": "Too big", "markdown": items(4_001)})),
        )
        .await;
    assert_eq!(status, 413, "{problem}");
    let created: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pages WHERE title = 'Too big'")
        .fetch_one(live.server.database.pool())
        .await
        .unwrap();
    assert_eq!(created, 0);
    live.server.stop().await;
}

#[tokio::test]
async fn the_empty_paragraph_an_open_empty_doc_keeps_does_not_count_against_the_limit() {
    let root = tempfile::TempDir::new().unwrap();
    let live = Live::start(&root).await;
    let page = live.page("Empty", None, &[]).await;
    let _client = Client::open(
        &live.server,
        &live.users,
        &live.users.owner,
        &page.to_string(),
    )
    .await;
    let items = |count: usize, reversed: bool| -> String {
        let mut lines: Vec<String> = (0..count)
            .map(|index| format!("- item {index}\n"))
            .collect();
        if reversed {
            lines.reverse();
        }
        lines.concat()
    };
    let read = live.read(page).await;
    let (status, written) = live
        .write(page, &read["version"], &items(4_000, false))
        .await;
    assert_eq!(status, 200, "{written}");
    // The editor's empty last paragraph stays, and the doc can still be written.
    let content = written["content"].as_array().unwrap();
    assert_eq!(content.len(), 4_001);
    assert_eq!(content[4_000]["type"], "paragraph");
    let (status, again) = live
        .write(page, &written["version"], &items(4_000, true))
        .await;
    assert_eq!(status, 200, "{again}");
    let (status, problem) = live
        .write(page, &again["version"], &items(4_001, false))
        .await;
    assert_eq!(status, 413, "{problem}");
    assert_eq!(problem["code"], "doc_too_large");
    live.server.stop().await;
}
