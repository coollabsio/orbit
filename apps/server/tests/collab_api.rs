//! Real-time co-editing over the real server (App + TCP listener) with a Rust yrs client
//! speaking y-sync like y-websocket: auth / Origin / visibility at the handshake, convergence,
//! awareness, persistence + compaction + reload, restart recovery, access loss while connected,
//! live server-side replacement, limits, epochs, and the JSON projection.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use orbit_platform::{Id, TimestampMillis};
use orbit_server::collab::blocknote::{FRAGMENT, doc_to_blocks, new_doc};
use orbit_server::collab::{CollabConfig, CollabHub};
use orbit_server::repositories::page_versions::SaveOrigin;
use orbit_server::repositories::pages::{PageChanges, PageRepository};
use serde_json::{Value, json};
use tempfile::TempDir;
use yrs::sync::awareness::AwarenessUpdateEntry;
use yrs::sync::{AwarenessUpdate, Message, SyncMessage};
use yrs::types::text::YChange;
use yrs::updates::decoder::Decode;
use yrs::{
    Any, ClientID, Doc, ReadTxn, Text, Transact, TransactionMut, Update, Xml, XmlFragment, XmlOut,
};

mod collab_support;
mod shared_postgres;

use collab_support::*;

#[tokio::test]
async fn handshake_checks_origin_session_membership_and_page_visibility() {
    let root = TempDir::new().unwrap();
    let server = Server::start(&root, fast_config()).await;
    let users = setup(&server).await;
    let shared = create_page(
        &server,
        &users,
        &users.owner,
        json!({"title": "Shared"}),
        "hello",
    )
    .await;
    let private = create_page(
        &server,
        &users,
        &users.owner,
        json!({"title": "Mine", "private": true}),
        "secret",
    )
    .await;
    let shared_id = shared["id"].as_str().unwrap();
    let private_id = private["id"].as_str().unwrap();
    let query = format!("v=1&epoch={}", shared["collab_epoch"].as_str().unwrap());
    let url = server.ws_url(&users.workspace, shared_id, &query);
    let base = server.base.clone();

    // Origin (platform layer): missing, opaque, or two.
    assert_eq!(
        server.socket(&url, Some(&users.owner), None).await.err(),
        Some(403)
    );
    assert_eq!(
        server
            .socket(&url, Some(&users.owner), Some("null"))
            .await
            .err(),
        Some(403)
    );
    // Session.
    assert_eq!(
        server.socket(&url, None, Some(&base)).await.err(),
        Some(401)
    );
    assert_eq!(
        server.socket(&url, Some("forged"), Some(&base)).await.err(),
        Some(401)
    );
    // Visibility: someone else's private page looks exactly like an unknown id.
    let private_url = server.ws_url(&users.workspace, private_id, &query);
    let unknown_url = server.ws_url(&users.workspace, &Id::new_v7().to_string(), &query);
    assert_eq!(
        server
            .socket(&private_url, Some(&users.member), Some(&base))
            .await
            .err(),
        Some(404)
    );
    assert_eq!(
        server
            .socket(&unknown_url, Some(&users.member), Some(&base))
            .await
            .err(),
        Some(404)
    );
    assert_eq!(
        server
            .socket(
                &server.ws_url(&users.workspace, "not-an-id", &query),
                Some(&users.member),
                Some(&base)
            )
            .await
            .err(),
        Some(404)
    );
    // Another workspace's member.
    let (_, stranger) = add_user(&server, None, "stranger@example.com", "Stranger").await;
    assert_eq!(
        server
            .socket(&url, Some(&stranger), Some(&base))
            .await
            .err(),
        Some(404)
    );
    // The owner may open their private page; the member the shared one.
    assert!(
        server
            .socket(&private_url, Some(&users.owner), Some(&base))
            .await
            .is_ok()
    );
    let client = Client::open(&server, &users, &users.member, shared_id).await;
    assert_eq!(client.text(), "hello");

    // Protocol version and epoch are checked after the upgrade.
    assert_eq!(
        Client::open_with(&server, &users, &users.member, shared_id, "v=2&epoch=x")
            .await
            .err(),
        Some(4426)
    );
    assert_eq!(
        Client::open_with(&server, &users, &users.member, shared_id, "v=1&epoch=stale")
            .await
            .err(),
        Some(4409)
    );
    assert_eq!(
        Client::open_with(&server, &users, &users.member, shared_id, "v=1")
            .await
            .err(),
        Some(4409)
    );
    server.stop().await;
}

#[tokio::test]
async fn two_clients_converge_and_share_awareness() {
    let root = TempDir::new().unwrap();
    let server = Server::start(&root, fast_config()).await;
    let users = setup(&server).await;
    let page = create_page(
        &server,
        &users,
        &users.owner,
        json!({"title": "Pad"}),
        "hello",
    )
    .await;
    let id = page["id"].as_str().unwrap();
    let mut a = Client::open(&server, &users, &users.owner, id).await;
    let mut b = Client::open(&server, &users, &users.member, id).await;
    a.type_text(5, " world").await;
    b.wait_text("hello world").await;
    // Concurrent edits converge.
    a.type_text(0, "A").await;
    b.type_text(11, "B").await;
    a.wait_text("Ahello worldB").await;
    b.wait_text("Ahello worldB").await;

    // Awareness: the server stamps the session's user (names cannot be spoofed) and echoes to
    // the sender too.
    a.set_awareness(
        1,
        json!({"user": {"name": "Mallory", "color": "#000"}, "cursor": null}),
    )
    .await;
    let a_id = a.doc.client_id().get();
    assert!(
        b.pump(Duration::from_secs(5), |client, _| client
            .awareness
            .contains_key(&a_id))
            .await
    );
    let state: Value = serde_json::from_str(&b.awareness[&a_id]).unwrap();
    assert_eq!(state["user"]["name"], "Olivia Owner");
    assert_eq!(state["user"]["id"], users.owner_id.to_string());
    assert!(state["user"]["color"].as_str().unwrap().starts_with('#'));
    assert!(
        a.pump(Duration::from_secs(5), |client, _| client
            .awareness
            .contains_key(&a_id))
            .await,
        "the sender gets its own state back"
    );
    // Two people on one page never share a cursor color.
    b.set_awareness(1, json!({"cursor": null})).await;
    let b_id = b.doc.client_id().get();
    assert!(
        a.pump(Duration::from_secs(5), |client, _| client
            .awareness
            .contains_key(&b_id))
            .await
    );
    let b_state: Value = serde_json::from_str(&a.awareness[&b_id]).unwrap();
    assert_eq!(b_state["user"]["id"], users.member_id.to_string());
    assert_ne!(b_state["user"]["color"], state["user"]["color"]);
    // B cannot speak for A's client id.
    let mut clients = HashMap::new();
    clients.insert(
        ClientID::new(a_id),
        AwarenessUpdateEntry {
            clock: 99,
            json: Arc::from("null"),
        },
    );
    b.send(Message::Awareness(AwarenessUpdate { clients }))
        .await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    // A leaves: B learns that its state is gone.
    drop(a);
    assert!(
        b.pump(Duration::from_secs(5), |client, _| {
            client
                .awareness
                .get(&a_id)
                .is_some_and(|state| state == "null")
        })
        .await,
        "awareness removal on disconnect"
    );
    server.stop().await;
}

#[tokio::test]
async fn edits_persist_compact_and_reload_after_eviction() {
    let root = TempDir::new().unwrap();
    let server = Server::start(
        &root,
        CollabConfig {
            compact_updates: 10,
            ..fast_config()
        },
    )
    .await;
    let users = setup(&server).await;
    let page = create_page(&server, &users, &users.owner, json!({"title": "Log"}), "x").await;
    let id = page["id"].as_str().unwrap();
    let mut a = Client::open(&server, &users, &users.owner, id).await;
    let mut expected = "x".to_owned();
    for i in 0..25 {
        let at = expected.encode_utf16().count() as u32;
        a.type_text(at, &i.to_string()).await;
        expected.push_str(&i.to_string());
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    let hub = CollabHub::existing(&server.database).unwrap();
    wait_until("compaction", || async {
        scalar_i64(
            &server,
            "SELECT snapshot_seq FROM page_collab_docs WHERE page_id = $1",
            id,
        )
        .await
            > 0
    })
    .await;
    assert!(
        scalar_i64(
            &server,
            "SELECT COUNT(*) FROM page_collab_updates WHERE page_id = $1",
            id
        )
        .await
            < 25
    );
    drop(a);
    // The room is evicted (flushed, projected, compacted) once idle.
    wait_until("eviction", || async { hub.stats().0 == 0 }).await;
    assert_eq!(
        scalar_i64(
            &server,
            "SELECT COUNT(*) FROM page_collab_updates WHERE page_id = $1",
            id
        )
        .await,
        0
    );
    let b = Client::open(&server, &users, &users.member, id).await;
    assert_eq!(b.text(), expected);
    let stored = get_page(&server, &users, &users.owner, id).await;
    assert_eq!(stored["content"][0]["content"][0]["text"], expected);
    server.stop().await;
}

#[tokio::test]
async fn documents_survive_a_restart_and_clients_push_what_the_server_missed() {
    let root = TempDir::new().unwrap();
    let server = Server::start(&root, fast_config()).await;
    let users = setup(&server).await;
    let page = create_page(
        &server,
        &users,
        &users.owner,
        json!({"title": "Restart"}),
        "base",
    )
    .await;
    let id = page["id"].as_str().unwrap().to_owned();
    let mut a = Client::open(&server, &users, &users.owner, &id).await;
    a.type_text(4, " one").await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    server.stop().await;
    assert_eq!(a.wait_close().await, 1012);
    // Typed while the server is down.
    a.edit(|doc, txn| {
        let node = paragraph_text(doc, txn, 0);
        node.insert(txn, 8, " two");
    })
    .await;

    let server = Server::start(&root, fast_config()).await;
    // The same client reconnects (same epoch) and syncs both ways.
    let epoch = get_page(&server, &users, &users.owner, &id).await["collab_epoch"]
        .as_str()
        .unwrap()
        .to_owned();
    let url = server.ws_url(&users.workspace, &id, &format!("v=1&epoch={epoch}"));
    a.socket = server
        .socket(&url, Some(&users.owner), Some(&server.base))
        .await
        .unwrap();
    a.closed = None;
    let sv = a.doc.transact().state_vector();
    a.send(Message::Sync(SyncMessage::SyncStep1(sv))).await;
    a.pump(Duration::from_secs(5), |_, synced| synced).await;
    let b = Client::open(&server, &users, &users.member, &id).await;
    let mut b = b;
    b.wait_text("base one two").await;
    wait_until("projection", || async {
        get_page(&server, &users, &users.owner, &id).await["content"][0]["content"][0]["text"]
            == "base one two"
    })
    .await;
    server.stop().await;
}

#[tokio::test]
async fn projection_writes_content_search_and_history_without_bumping_the_version() {
    let root = TempDir::new().unwrap();
    let server = Server::start(&root, fast_config()).await;
    let users = setup(&server).await;
    let page = create_page(
        &server,
        &users,
        &users.owner,
        json!({"title": "Notes"}),
        "first",
    )
    .await;
    let id = page["id"].as_str().unwrap();
    let version = page["version"].as_u64().unwrap();
    // The page is older than 10 minutes and has no versions: the next edit snapshots it.
    sqlx::query("UPDATE pages SET created_at = created_at - 3600000 WHERE id = $1")
        .bind(id)
        .execute(server.database.pool())
        .await
        .unwrap();
    let audits_before = scalar_i64(
        &server,
        "SELECT COUNT(*) FROM audit_events WHERE resource_id = $1 AND action = 'page.updated'",
        id,
    )
    .await;
    let mut b = Client::open(&server, &users, &users.member, id).await;
    b.type_text(5, " zebracorn").await;
    wait_until("projection", || async {
        get_page(&server, &users, &users.owner, id).await["content"][0]["content"][0]["text"]
            == "first zebracorn"
    })
    .await;
    let stored = get_page(&server, &users, &users.owner, id).await;
    assert_eq!(
        stored["version"].as_u64().unwrap(),
        version,
        "projection keeps the version"
    );
    assert_eq!(stored["updated_by"], users.member_id.to_string());
    let (_, search) = server
        .api(
            &users.owner,
            "GET",
            &format!(
                "/api/v1/workspaces/{}/pages/search?q=zebracorn",
                users.workspace
            ),
            None,
        )
        .await;
    assert_eq!(search["items"][0]["id"], id);
    // History: the state before the edit became an `auto` version.
    let (_, versions) = server
        .api(
            &users.owner,
            "GET",
            &format!("/api/v1/workspaces/{}/pages/{id}/versions", users.workspace),
            None,
        )
        .await;
    assert_eq!(versions["items"].as_array().unwrap().len(), 1, "{versions}");
    assert_eq!(versions["items"][0]["kind"], "auto");
    let vid = versions["items"][0]["id"].as_str().unwrap();
    let (_, old) = server
        .api(
            &users.owner,
            "GET",
            &format!(
                "/api/v1/workspaces/{}/pages/{id}/versions/{vid}",
                users.workspace
            ),
            None,
        )
        .await;
    assert_eq!(old["content"][0]["content"][0]["text"], "first");
    // Audit: one `page.updated` for this burst of edits, none for the next within 10 minutes.
    let audits = || async {
        scalar_i64(
            &server,
            "SELECT COUNT(*) FROM audit_events WHERE resource_id = $1 AND action = 'page.updated'",
            id,
        )
        .await
    };
    assert_eq!(audits().await, audits_before + 1);
    b.type_text(0, "x").await;
    wait_until("second projection", || async {
        get_page(&server, &users, &users.owner, id).await["content"][0]["content"][0]["text"]
            == "xfirst zebracorn"
    })
    .await;
    assert_eq!(audits().await, audits_before + 1, "throttled");
    server.stop().await;
}

#[tokio::test]
async fn moving_into_a_private_space_closes_4403_and_trash_closes_4404() {
    let root = TempDir::new().unwrap();
    let server = Server::start(&root, fast_config()).await;
    let users = setup(&server).await;
    let page = create_page(
        &server,
        &users,
        &users.owner,
        json!({"title": "Moving"}),
        "m",
    )
    .await;
    let id = page["id"].as_str().unwrap();
    let mut owner = Client::open(&server, &users, &users.owner, id).await;
    let mut member = Client::open(&server, &users, &users.member, id).await;
    let (status, moved) = server
        .api(
            &users.owner,
            "POST",
            &format!("/api/v1/workspaces/{}/pages/{id}/move", users.workspace),
            Some(json!({"expected_version": page["version"], "parent_id": null, "private": true, "position": 0})),
        )
        .await;
    assert_eq!(status, 200, "{moved}");
    assert_eq!(member.wait_close().await, 4403);
    // The owner keeps editing.
    owner.type_text(1, "!").await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(owner.closed.is_none());
    let (status, _) = server
        .api(
            &users.owner,
            "DELETE",
            &format!(
                "/api/v1/workspaces/{}/pages/{id}?expected_version={}",
                users.workspace, moved["version"]
            ),
            None,
        )
        .await;
    assert_eq!(status, 204);
    assert_eq!(owner.wait_close().await, 4404);
    server.stop().await;
}

#[tokio::test]
async fn restores_api_writes_and_imports_replace_the_document_live() {
    let root = TempDir::new().unwrap();
    let server = Server::start(&root, fast_config()).await;
    let users = setup(&server).await;
    let page = create_page(
        &server,
        &users,
        &users.owner,
        json!({"title": "Live"}),
        "original",
    )
    .await;
    let id = page["id"].as_str().unwrap();
    // A version to restore later.
    sqlx::query(
        "INSERT INTO page_versions (id, workspace_id, page_id, title, icon, content_json, kind, created_by, created_at) \
         VALUES ($1, $2, $3, 'Live', NULL, $4, 'auto', $5, 1)",
    )
    .bind(Id::new_v7().to_string())
    .bind(&users.workspace)
    .bind(id)
    .bind(json!([{"id": "v1", "type": "heading", "props": {"level": 2}, "content": "restored"}]).to_string())
    .bind(users.owner_id.to_string())
    .execute(server.database.pool())
    .await
    .unwrap();
    let mut a = Client::open(&server, &users, &users.owner, id).await;
    let mut b = Client::open(&server, &users, &users.member, id).await;
    a.type_text(8, " typed").await;
    b.wait_text("original typed").await;

    // Version restore: projects the typed text into the `restore` version first, then both
    // editors switch to the restored document without reconnecting.
    let (_, versions) = server
        .api(
            &users.owner,
            "GET",
            &format!("/api/v1/workspaces/{}/pages/{id}/versions", users.workspace),
            None,
        )
        .await;
    // The inserted (oldest) version; typing may have added an `auto` one above it.
    let items = versions["items"].as_array().unwrap();
    let vid = items[items.len() - 1]["id"].as_str().unwrap().to_owned();
    let current = get_page(&server, &users, &users.owner, id).await;
    let (status, restored) = server
        .api(
            &users.owner,
            "POST",
            &format!(
                "/api/v1/workspaces/{}/pages/{id}/versions/{vid}/restore",
                users.workspace
            ),
            Some(json!({"expected_version": current["version"]})),
        )
        .await;
    assert_eq!(status, 200, "{restored}");
    a.wait_text("restored").await;
    b.wait_text("restored").await;
    assert!(a.closed.is_none() && b.closed.is_none());
    assert_eq!(doc_to_blocks(&b.doc)[0]["type"], "heading");
    let (_, versions) = server
        .api(
            &users.owner,
            "GET",
            &format!("/api/v1/workspaces/{}/pages/{id}/versions", users.workspace),
            None,
        )
        .await;
    let (_, saved) = server
        .api(
            &users.owner,
            "GET",
            &format!(
                "/api/v1/workspaces/{}/pages/{id}/versions/{}",
                users.workspace,
                versions["items"][0]["id"].as_str().unwrap()
            ),
            None,
        )
        .await;
    assert_eq!(saved["kind"], "restore");
    assert_eq!(saved["content"][0]["content"][0]["text"], "original typed");
    // Typing continues on the restored document.
    b.type_text(8, "!").await;
    a.wait_text("restored!").await;

    // REST content write (API clients).
    let current = get_page(&server, &users, &users.owner, id).await;
    let (status, _) = server
        .api(
            &users.owner,
            "PATCH",
            &format!("/api/v1/workspaces/{}/pages/{id}", users.workspace),
            Some(json!({"expected_version": current["version"], "content": [{"id": "p", "type": "paragraph", "content": "from the api"}]})),
        )
        .await;
    assert_eq!(status, 200);
    a.wait_text("from the api").await;
    b.wait_text("from the api").await;

    // The Notion import path (update_page with origin Import), including an unsafe link.
    let pages = PageRepository::new(server.database.clone());
    let current = get_page(&server, &users, &users.owner, id).await;
    pages
        .update_page(
            users.workspace.parse().unwrap(),
            id.parse().unwrap(),
            users.owner_id,
            current["version"].as_u64().unwrap(),
            PageChanges {
                content: Some(vec![json!({"id": "n", "type": "paragraph", "content": [
                    {"type": "link", "href": "javascript:alert(1)", "content": [{"type": "text", "text": "imported", "styles": {}}]}
                ]})]),
                origin: SaveOrigin::Import,
                ..PageChanges::default()
            },
            "notion-import-test",
            TimestampMillis::now(),
        )
        .await
        .unwrap();
    b.wait_text("imported").await;
    let blocks = doc_to_blocks(&b.doc);
    assert_eq!(
        blocks[0]["content"][0]["type"], "text",
        "the unsafe link never reaches editors"
    );
    server.stop().await;
}

#[tokio::test]
async fn unsafe_links_from_clients_are_repaired_for_everyone() {
    let root = TempDir::new().unwrap();
    let server = Server::start(&root, fast_config()).await;
    let users = setup(&server).await;
    let page = create_page(
        &server,
        &users,
        &users.owner,
        json!({"title": "Links"}),
        "click",
    )
    .await;
    let id = page["id"].as_str().unwrap();
    let mut a = Client::open(&server, &users, &users.member, id).await;
    let mut b = Client::open(&server, &users, &users.owner, id).await;
    a.edit(|doc, txn| {
        let node = paragraph_text(doc, txn, 0);
        let mut href = HashMap::new();
        href.insert("href".to_owned(), Any::from("javascript:alert(1)"));
        let mut attrs = yrs::types::Attrs::new();
        attrs.insert(Arc::from("link"), Any::Map(Arc::new(href)));
        node.format(txn, 0, 5, attrs);
    })
    .await;
    let has_link = |doc: &Doc| {
        let txn = doc.transact();
        let fragment = txn.get_xml_fragment(FRAGMENT).unwrap();
        let Some(XmlOut::Element(group)) = fragment.get(&txn, 0) else {
            return true;
        };
        let Some(XmlOut::Element(container)) = group.get(&txn, 0) else {
            return true;
        };
        let Some(XmlOut::Element(paragraph)) = container.get(&txn, 0) else {
            return true;
        };
        let Some(XmlOut::Text(text)) = paragraph.get(&txn, 0) else {
            return true;
        };
        text.diff(&txn, YChange::identity).iter().any(|chunk| {
            chunk
                .attributes
                .as_ref()
                .is_some_and(|attrs| attrs.contains_key("link"))
        })
    };
    assert!(
        a.pump(Duration::from_secs(5), |client, _| !has_link(&client.doc))
            .await
    );
    assert!(
        b.pump(Duration::from_secs(5), |client, _| !has_link(&client.doc))
            .await
    );
    wait_until("projection", || async {
        get_page(&server, &users, &users.owner, id).await["content"][0]["content"][0]["text"]
            == "click"
    })
    .await;
    let stored = get_page(&server, &users, &users.owner, id).await;
    assert_eq!(stored["content"][0]["content"][0]["type"], "text");
    server.stop().await;
}

#[tokio::test]
async fn duplicates_copy_what_was_just_typed() {
    let root = TempDir::new().unwrap();
    let server = Server::start(
        &root,
        CollabConfig {
            project_idle: Duration::from_secs(30),
            project_max_delay: Duration::from_secs(30),
            ..fast_config()
        },
    )
    .await;
    let users = setup(&server).await;
    let page = create_page(
        &server,
        &users,
        &users.owner,
        json!({"title": "Source"}),
        "draft",
    )
    .await;
    let id = page["id"].as_str().unwrap();
    let mut a = Client::open(&server, &users, &users.owner, id).await;
    a.type_text(5, " final").await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    let (status, copy) = server
        .api(
            &users.owner,
            "POST",
            &format!(
                "/api/v1/workspaces/{}/pages/{id}/duplicate",
                users.workspace
            ),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, 201, "{copy}");
    assert_eq!(copy["content"][0]["content"][0]["text"], "draft final");
    server.stop().await;
}

#[tokio::test]
async fn exports_include_what_was_just_typed() {
    let root = TempDir::new().unwrap();
    let server = Server::start(
        &root,
        CollabConfig {
            project_idle: Duration::from_secs(30),
            project_max_delay: Duration::from_secs(30),
            ..fast_config()
        },
    )
    .await;
    let users = setup(&server).await;
    let page = create_page(
        &server,
        &users,
        &users.owner,
        json!({"title": "Live"}),
        "draft",
    )
    .await;
    let id = page["id"].as_str().unwrap();
    let mut a = Client::open(&server, &users, &users.owner, id).await;
    a.type_text(5, " final").await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    let response = server
        .http
        .get(format!(
            "{}/api/v1/workspaces/{}/pages/{id}/export?format=markdown",
            server.base, users.workspace
        ))
        .header("cookie", format!("{COOKIE}={}", users.owner))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 200);
    let bytes = response.bytes().await.unwrap();
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes.to_vec())).unwrap();
    let mut text = String::new();
    std::io::Read::read_to_string(&mut archive.by_name("Live.md").unwrap(), &mut text).unwrap();
    assert!(text.ends_with("# Live\n\ndraft final\n"), "{text}");
    server.stop().await;
}

#[tokio::test]
async fn limits_close_with_4413_4429_and_1013() {
    let root = TempDir::new().unwrap();
    let server = Server::start(
        &root,
        CollabConfig {
            max_frame_bytes: 64 * 1024,
            max_doc_bytes: 128 * 1024,
            max_connections_per_page: 2,
            sync_messages_per_second: 20,
            ..fast_config()
        },
    )
    .await;
    let users = setup(&server).await;
    let page = create_page(
        &server,
        &users,
        &users.owner,
        json!({"title": "Limits"}),
        "l",
    )
    .await;
    let id = page["id"].as_str().unwrap();

    // Per-page connection cap.
    let _a = Client::open(&server, &users, &users.owner, id).await;
    let _b = Client::open(&server, &users, &users.member, id).await;
    let epoch = page["collab_epoch"].as_str().unwrap();
    assert_eq!(
        Client::open_with(
            &server,
            &users,
            &users.owner,
            id,
            &format!("v=1&epoch={epoch}")
        )
        .await
        .err(),
        Some(1013)
    );
    drop(_b);
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Frame too large.
    let mut c = Client::open(&server, &users, &users.member, id).await;
    c.type_text(1, &"x".repeat(80 * 1024)).await;
    assert_eq!(c.wait_close().await, 4413);

    // Document too large: several frames under the frame limit.
    let mut d = Client::open(&server, &users, &users.member, id).await;
    for _ in 0..4 {
        d.type_text(1, &"y".repeat(40 * 1024)).await;
    }
    assert_eq!(d.wait_close().await, 4413);

    // Message rate.
    let mut e = Client::open(&server, &users, &users.member, id).await;
    for i in 0..60 {
        e.type_text(0, &i.to_string()).await;
    }
    assert_eq!(e.wait_close().await, 4429);
    server.stop().await;
}

#[tokio::test]
async fn converter_changes_reset_the_document_under_a_new_epoch() {
    let root = TempDir::new().unwrap();
    let server = Server::start(&root, fast_config()).await;
    let users = setup(&server).await;
    let page = create_page(
        &server,
        &users,
        &users.owner,
        json!({"title": "Reset"}),
        "kept",
    )
    .await;
    let id = page["id"].as_str().unwrap().to_owned();
    let mut a = Client::open(&server, &users, &users.owner, &id).await;
    a.type_text(4, " text").await;
    // The edit reached the server (a frame still in flight at shutdown is re-pushed by a
    // reconnecting client, which this test does not model).
    wait_until("projection", || async {
        get_page(&server, &users, &users.owner, &id).await["content"][0]["content"][0]["text"]
            == "kept text"
    })
    .await;
    let old_epoch = get_page(&server, &users, &users.owner, &id).await["collab_epoch"]
        .as_str()
        .unwrap()
        .to_owned();
    server.stop().await;
    // Simulate a document built by another converter version.
    let database = orbit_platform::Database::open(&orbit_platform::DatabaseConfig::new(
        root.path().join("data/orbit.sqlite"),
    ))
    .await
    .unwrap();
    sqlx::query("UPDATE page_collab_docs SET converter_version = 0 WHERE page_id = $1")
        .bind(&id)
        .execute(database.pool())
        .await
        .unwrap();
    drop(database);

    let server = Server::start(&root, fast_config()).await;
    let url = server.ws_url(&users.workspace, &id, &format!("v=1&epoch={old_epoch}"));
    let mut stale = Client {
        doc: new_doc(None),
        socket: server
            .socket(&url, Some(&users.owner), Some(&server.base))
            .await
            .unwrap(),
        closed: None,
        awareness: HashMap::new(),
    };
    assert_eq!(stale.wait_close().await, 4409);
    let fresh = get_page(&server, &users, &users.owner, &id).await;
    assert_ne!(fresh["collab_epoch"], old_epoch);
    let b = Client::open(&server, &users, &users.owner, &id).await;
    assert_eq!(b.text(), "kept text");
    server.stop().await;
}

/// The content element (paragraph) of the `index`th top-level block.
fn block_element(txn: &mut TransactionMut, index: u32) -> yrs::XmlElementRef {
    let fragment = txn.get_xml_fragment(FRAGMENT).expect("fragment");
    let Some(XmlOut::Element(group)) = fragment.get(txn, 0) else {
        panic!("no block group")
    };
    let Some(XmlOut::Element(container)) = group.get(txn, index) else {
        panic!("no block")
    };
    let Some(XmlOut::Element(paragraph)) = container.get(txn, 0) else {
        panic!("no paragraph")
    };
    paragraph
}

impl Client {
    /// Appends a mention of `user` to the `block`th block (what picking a member in "@" does).
    async fn mention(&mut self, block: u32, user: Id, name: &str) {
        let (user, name) = (user.to_string(), name.to_owned());
        self.edit(|_, txn| {
            let paragraph = block_element(txn, block);
            let mention = paragraph.push_back(txn, yrs::XmlElementPrelim::empty("mention"));
            mention.insert_attribute(txn, "userId", user);
            mention.insert_attribute(txn, "name", name);
        })
        .await;
    }

    /// Removes every mention of `user` from the `block`th block.
    async fn unmention(&mut self, block: u32, user: Id) {
        let user = user.to_string();
        self.edit(|_, txn| {
            let paragraph = block_element(txn, block);
            let indexes: Vec<u32> = paragraph
                .children(txn)
                .enumerate()
                .filter_map(|(index, child)| match child {
                    XmlOut::Element(element)
                        if element.tag().as_ref() == "mention"
                            && element
                                .get_attribute(txn, "userId")
                                .map(|id| id.to_string(txn))
                                == Some(user.clone()) =>
                    {
                        Some(index as u32)
                    }
                    _ => None,
                })
                .collect();
            for index in indexes.into_iter().rev() {
                paragraph.remove_range(txn, index, 1);
            }
        })
        .await;
    }
}

async fn page_mentions(server: &Server, page: &str, recipient: Id) -> i64 {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM notifications WHERE kind = 'page_mentioned' AND page_id = $1 \
         AND recipient_user_id = $2",
    )
    .bind(page)
    .bind(recipient.to_string())
    .fetch_one(server.database.pool())
    .await
    .unwrap()
}

async fn mention_count(server: &Server, users: &Users, page: &str) -> usize {
    let stored = get_page(server, users, &users.owner, page).await;
    let mut count = 0;
    for block in stored["content"].as_array().unwrap() {
        count += block["content"].as_array().map_or(0, |items| {
            items
                .iter()
                .filter(|item| item["type"] == "mention")
                .count()
        });
    }
    count
}

#[tokio::test]
async fn mentions_in_the_body_notify_members_who_can_see_the_page_once() {
    let root = TempDir::new().unwrap();
    let server = Server::start(&root, fast_config()).await;
    let users = setup(&server).await;
    let (outsider, _) = add_user(&server, None, "out@example.com", "Otto Outsider").await;
    let (suspended, _) = add_user(
        &server,
        Some(&users.workspace),
        "sus@example.com",
        "Sam Suspended",
    )
    .await;
    sqlx::query("UPDATE users SET suspended_at = 1 WHERE id = $1")
        .bind(suspended.to_string())
        .execute(server.database.pool())
        .await
        .unwrap();
    let page = create_page(
        &server,
        &users,
        &users.owner,
        json!({"title": "[mention-test]"}),
        "Hello",
    )
    .await;
    let id = page["id"].as_str().unwrap();
    let mut owner = Client::open(&server, &users, &users.owner, id).await;
    owner.mention(0, users.member_id, "Mia Member").await;
    owner.mention(0, users.owner_id, "Olivia Owner").await;
    owner.mention(0, outsider, "Otto Outsider").await;
    owner.mention(0, suspended, "Sam Suspended").await;
    wait_until("mentions projected", || async {
        mention_count(&server, &users, id).await == 4
    })
    .await;
    wait_until("notification", || async {
        page_mentions(&server, id, users.member_id).await == 1
    })
    .await;
    for nobody in [users.owner_id, outsider, suspended] {
        assert_eq!(page_mentions(&server, id, nobody).await, 0, "{nobody}");
    }
    // content_text has "@Mia Member": search finds the page by the mentioned name.
    let (_, search) = server
        .api(
            &users.owner,
            "GET",
            &format!("/api/v1/workspaces/{}/pages/search?q=Mia", users.workspace),
            None,
        )
        .await;
    assert_eq!(search["items"][0]["id"], id, "{search}");

    // The member's inbox: "Olivia mentioned you" linking to the page and the block.
    let (status, inbox) = server
        .api(
            &users.member,
            "GET",
            &format!("/api/v1/workspaces/{}/notifications", users.workspace),
            None,
        )
        .await;
    assert_eq!(status, 200, "{inbox}");
    let item = &inbox["items"][0];
    assert_eq!(item["kind"], "page_mentioned");
    assert_eq!(item["page_id"], id);
    assert_eq!(item["page_block_id"], "b1");
    assert_eq!(item["actor_user_id"], users.owner_id.to_string());
    assert!(item["page_thread_id"].is_null());

    // Editing around the mention adds nothing new.
    owner.type_text(0, "Well, ").await;
    wait_until("text projected", || async {
        get_page(&server, &users, &users.owner, id).await["content"][0]["content"][0]["text"]
            == "Well, Hello"
    })
    .await;
    // Removing and re-adding within 10 minutes: deduplicated.
    owner.unmention(0, users.member_id).await;
    wait_until("mention removed", || async {
        mention_count(&server, &users, id).await == 3
    })
    .await;
    owner.mention(0, users.member_id, "Mia Member").await;
    wait_until("mention re-added", || async {
        mention_count(&server, &users, id).await == 4
    })
    .await;
    assert_eq!(page_mentions(&server, id, users.member_id).await, 1);

    // After the window a new mention notifies again.
    sqlx::query("UPDATE notifications SET created_at = created_at - 660000 WHERE page_id = $1")
        .bind(id)
        .execute(server.database.pool())
        .await
        .unwrap();
    owner.unmention(0, users.member_id).await;
    wait_until("mention removed again", || async {
        mention_count(&server, &users, id).await == 3
    })
    .await;
    owner.mention(0, users.member_id, "Mia Member").await;
    wait_until("second notification", || async {
        page_mentions(&server, id, users.member_id).await == 2
    })
    .await;
    // The member mentioning the owner notifies the owner (the editor is the actor).
    let mut member = Client::open(&server, &users, &users.member, id).await;
    member.unmention(0, users.owner_id).await;
    wait_until("owner mention removed", || async {
        mention_count(&server, &users, id).await == 3
    })
    .await;
    member.mention(0, users.owner_id, "Olivia Owner").await;
    wait_until("owner notified", || async {
        page_mentions(&server, id, users.owner_id).await == 1
    })
    .await;
    let actor: String = sqlx::query_scalar(
        "SELECT actor_user_id FROM notifications WHERE page_id = $1 AND recipient_user_id = $2",
    )
    .bind(id)
    .bind(users.owner_id.to_string())
    .fetch_one(server.database.pool())
    .await
    .unwrap();
    assert_eq!(actor, users.member_id.to_string());
    server.stop().await;
}

#[tokio::test]
async fn rest_content_writes_notify_new_mentions_but_never_on_private_pages() {
    let root = TempDir::new().unwrap();
    let server = Server::start(&root, fast_config()).await;
    let users = setup(&server).await;
    let mention = |user: Id, name: &str| json!({"type": "mention", "props": {"userId": user.to_string(), "name": name}});
    let patch = |id: String, version: Value, content: Value| {
        (
            format!("/api/v1/workspaces/{}/pages/{id}", users.workspace),
            json!({"expected_version": version, "content": content}),
        )
    };

    // A private page: mentioning the member notifies nobody (they cannot see it).
    let private = create_page(
        &server,
        &users,
        &users.owner,
        json!({"title": "Secret", "private": true}),
        "private",
    )
    .await;
    let private_id = private["id"].as_str().unwrap().to_owned();
    let (path, body) = patch(
        private_id.clone(),
        private["version"].clone(),
        json!([{"id": "p1", "type": "paragraph", "content": [mention(users.member_id, "Mia Member")]}]),
    );
    let (status, written) = server.api(&users.owner, "PATCH", &path, Some(body)).await;
    assert_eq!(status, 200, "{written}");
    assert_eq!(
        page_mentions(&server, &private_id, users.member_id).await,
        0
    );

    // A teamspace page written through the API by the member: the owner is notified once,
    // the member mentioning themselves is not.
    let page = create_page(
        &server,
        &users,
        &users.owner,
        json!({"title": "Shared"}),
        "shared",
    )
    .await;
    let id = page["id"].as_str().unwrap().to_owned();
    let content = json!([
        {"id": "a", "type": "paragraph", "content": [{"type": "text", "text": "Hi ", "styles": {}}, mention(users.owner_id, "Olivia Owner")]},
        {"id": "b", "type": "paragraph", "content": [mention(users.member_id, "Mia Member")]}
    ]);
    let (path, body) = patch(id.clone(), page["version"].clone(), content);
    let (status, written) = server.api(&users.member, "PATCH", &path, Some(body)).await;
    assert_eq!(status, 200, "{written}");
    assert_eq!(page_mentions(&server, &id, users.owner_id).await, 1);
    assert_eq!(page_mentions(&server, &id, users.member_id).await, 0);
    let block: Option<String> = sqlx::query_scalar(
        "SELECT page_block_id FROM notifications WHERE page_id = $1 AND kind = 'page_mentioned'",
    )
    .bind(&id)
    .fetch_one(server.database.pool())
    .await
    .unwrap();
    assert_eq!(block.as_deref(), Some("a"));
    // Moving the mention to another block is no new mention.
    let moved = json!([
        {"id": "b", "type": "paragraph", "content": [mention(users.member_id, "Mia Member"), mention(users.owner_id, "Olivia Owner")]}
    ]);
    let (path, body) = patch(id.clone(), written["version"].clone(), moved);
    let (status, written) = server.api(&users.member, "PATCH", &path, Some(body)).await;
    assert_eq!(status, 200, "{written}");
    assert_eq!(page_mentions(&server, &id, users.owner_id).await, 1);
    // A mention of a user id that is no member of the workspace is ignored.
    let (stranger, _) = add_user(&server, None, "x@example.com", "X").await;
    let (path, body) = patch(
        id.clone(),
        written["version"].clone(),
        json!([{"id": "c", "type": "paragraph", "content": [mention(stranger, "X")]}]),
    );
    let (status, _) = server.api(&users.member, "PATCH", &path, Some(body)).await;
    assert_eq!(status, 200);
    assert_eq!(page_mentions(&server, &id, stranger).await, 0);
    server.stop().await;
}

#[tokio::test]
async fn servers_of_one_database_co_edit_and_compact_without_losing_edits() {
    let Some(database) = shared_postgres::SharedDatabase::create().await else {
        return; // several servers need a Postgres database
    };
    let config = || CollabConfig {
        compact_updates: 8,
        ..fast_config()
    };
    let (root_one, root_two) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    let one = Server::start_shared(&root_one, &database.url, config()).await;
    let users = setup(&one).await;
    let two = Server::start_shared(&root_two, &database.url, config()).await;
    let page = create_page(&one, &users, &users.owner, json!({"title": "Pad"}), "x").await;
    let id = page["id"].as_str().unwrap();
    let mut a = Client::open(&one, &users, &users.owner, id).await;
    let mut b = Client::open(&two, &users, &users.member, id).await;

    // An edit on one server reaches the client of the other.
    a.type_text(1, "y").await;
    b.wait_text("xy").await;
    b.type_text(2, "z").await;
    a.wait_text("xyz").await;
    // And so do cursors.
    a.set_awareness(1, json!({"cursor": null})).await;
    let a_id = a.doc.client_id().get();
    assert!(
        b.pump(Duration::from_secs(5), |client, _| client
            .awareness
            .contains_key(&a_id))
            .await,
        "awareness crossed servers"
    );

    // Both type at once while both servers write and compact the same log.
    for i in 0..12 {
        a.type_text(0, &format!("a{i}")).await;
        let end = b.text().encode_utf16().count() as u32;
        b.type_text(end, &format!("b{i}")).await;
        tokio::time::sleep(Duration::from_millis(15)).await;
    }
    let prefix: String = (0..12).rev().map(|i| format!("a{i}")).collect();
    let suffix: String = (0..12).map(|i| format!("b{i}")).collect();
    let expected = format!("{prefix}xyz{suffix}");
    a.wait_text(&expected).await;
    b.wait_text(&expected).await;
    wait_until("a compaction", || async {
        scalar_i64(
            &one,
            "SELECT snapshot_seq FROM page_collab_docs WHERE page_id = $1",
            id,
        )
        .await
            > 0
    })
    .await;

    // Both rooms are evicted (written, compacted); a new client loads every edit.
    drop((a, b));
    for server in [&one, &two] {
        let hub = CollabHub::existing(&server.database).unwrap();
        wait_until("eviction", || async { hub.stats().0 == 0 }).await;
    }
    let c = Client::open(&two, &users, &users.owner, id).await;
    assert_eq!(c.text(), expected);
    let stored = get_page(&one, &users, &users.owner, id).await;
    assert_eq!(stored["content"][0]["content"][0]["text"], expected);
    drop(c);
    one.stop().await;
    two.stop().await;
    database.drop().await;
}

#[tokio::test]
async fn a_compaction_keeps_what_another_server_logged() {
    if !orbit_platform::testing_postgres() {
        return; // other servers write the same log on Postgres only
    }
    let root = TempDir::new().unwrap();
    let server = Server::start(
        &root,
        CollabConfig {
            compact_updates: 8,
            ..fast_config()
        },
    )
    .await;
    let users = setup(&server).await;
    let page = create_page(&server, &users, &users.owner, json!({"title": "Log"}), "x").await;
    let id = page["id"].as_str().unwrap();
    let mut a = Client::open(&server, &users, &users.owner, id).await;

    // Another server logged an edit, and its message never came.
    let other = new_doc(None);
    let known = a
        .doc
        .transact()
        .encode_state_as_update_v1(&Default::default());
    other
        .transact_mut()
        .apply_update(Update::decode_v1(&known).unwrap())
        .unwrap();
    let before = other.transact().state_vector();
    {
        let mut txn = other.transact_mut();
        paragraph_text(&other, &mut txn, 0).insert(&mut txn, 0, "Q");
    }
    let logged = other.transact().encode_state_as_update_v1(&before);
    sqlx::query(
        "INSERT INTO page_collab_updates (page_id, workspace_id, data, user_id, created_at) \
         VALUES ($1, $2, $3, NULL, 0)",
    )
    .bind(id)
    .bind(&users.workspace)
    .bind(logged)
    .execute(server.database.pool())
    .await
    .unwrap();

    // This server writes and compacts: its snapshot takes the other server's row in.
    for _ in 0..10 {
        let end = a.text().encode_utf16().count() as u32;
        a.type_text(end, "z").await;
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    let expected = format!("Qx{}", "z".repeat(10));
    a.wait_text(&expected).await;
    drop(a);
    let hub = CollabHub::existing(&server.database).unwrap();
    wait_until("eviction", || async { hub.stats().0 == 0 }).await;
    let b = Client::open(&server, &users, &users.member, id).await;
    assert_eq!(b.text(), expected);
    server.stop().await;
}
