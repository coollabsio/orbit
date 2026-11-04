//! Real-time co-editing over the real server (App + TCP listener) with a Rust yrs client
//! speaking y-sync like y-websocket: auth / Origin / visibility at the handshake, convergence,
//! awareness, persistence + compaction + reload, restart recovery, access loss while connected,
//! live server-side replacement, limits, epochs, and the JSON projection.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use orbit_platform::{Id, TimestampMillis};
use orbit_server::collab::blocknote::{FRAGMENT, doc_to_blocks, new_doc};
use orbit_server::collab::{CollabConfig, CollabHub, DocKey};
use orbit_server::repositories::billing::{self as billing_repository, SubscriptionSnapshot};
use orbit_server::repositories::identity::IdentityRepository;
use orbit_server::repositories::page_versions::SaveOrigin;
use orbit_server::repositories::pages::{PageChanges, PageRepository};
use orbit_server::socket_ticket::SocketTickets;
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
async fn a_ticket_opens_the_socket_from_a_webview_without_the_cookie() {
    let root = TempDir::new().unwrap();
    let server = Server::start(&root, fast_config()).await;
    let users = setup(&server).await;
    let page = create_page(
        &server,
        &users,
        &users.owner,
        json!({"title": "Shared"}),
        "hello",
    )
    .await;
    let id = page["id"].as_str().unwrap();
    let epoch = page["collab_epoch"].as_str().unwrap();
    let url = |ticket: &str| {
        server.ws_url(
            &users.workspace,
            id,
            &format!("v=1&epoch={epoch}&ticket={ticket}"),
        )
    };

    // A WebView sends an opaque or foreign Origin, or none, and no cookie.
    let ticket = server.socket_ticket(&users.member, "page", id).await;
    let socket = server
        .socket(&url(&ticket), None, Some("null"))
        .await
        .expect("upgraded");
    let mut webview = Client::sync(socket).await.expect("synced");
    assert_eq!(webview.text(), "hello");
    let socket = server
        .socket(&url(&ticket), None, Some("https://webview.example"))
        .await
        .expect("upgraded");
    assert_eq!(Client::sync(socket).await.expect("synced").text(), "hello");
    let socket = server
        .socket(&url(&ticket), None, None)
        .await
        .expect("upgraded");
    assert_eq!(Client::sync(socket).await.expect("synced").text(), "hello");

    // It edits like a cookie client, as the ticket's user.
    let mut browser = Client::open(&server, &users, &users.owner, id).await;
    webview.type_text(5, " world").await;
    browser.wait_text("hello world").await;
    browser.type_text(0, "Oh, ").await;
    webview.wait_text("Oh, hello world").await;
    webview
        .set_awareness(1, json!({"user": {"name": "Mallory"}, "cursor": null}))
        .await;
    let webview_id = webview.doc.client_id().get();
    assert!(
        browser
            .pump(Duration::from_secs(5), |client, _| client
                .awareness
                .contains_key(&webview_id))
            .await
    );
    let state: Value = serde_json::from_str(&browser.awareness[&webview_id]).unwrap();
    assert_eq!(state["user"]["id"], users.member_id.to_string());
    assert_eq!(state["user"]["name"], "Mia Member");

    // A ticket never comes with the cookie.
    assert_eq!(
        server
            .socket(&url(&ticket), Some(&users.member), Some(&server.base))
            .await
            .err(),
        Some(401)
    );
    // Altered, expired, foreign-key, or garbage tickets.
    let mut altered = ticket.clone().into_bytes();
    altered[10] = if altered[10] == b'A' { b'B' } else { b'A' };
    let altered = String::from_utf8(altered).unwrap();
    let identity = IdentityRepository::new(server.database.clone());
    let member_session = identity
        .authenticate_session(&users.member, TimestampMillis::now())
        .await
        .unwrap()
        .id;
    let document = DocKey::page(id.parse().unwrap());
    let expired = SocketTickets::new(&[1; 32]).issue(
        member_session,
        document,
        TimestampMillis::from_millis(TimestampMillis::now().as_millis() - 1),
    );
    let foreign_key = SocketTickets::new(&[2; 32]).issue(
        member_session,
        document,
        TimestampMillis::from_millis(TimestampMillis::now().as_millis() + 60_000),
    );
    // A ticket asked for another page (that the member can open) or for a task with this id.
    let other = create_page(&server, &users, &users.owner, json!({"title": "Other"}), "").await;
    let other_page = server
        .socket_ticket(&users.member, "page", other["id"].as_str().unwrap())
        .await;
    let as_task = server.socket_ticket(&users.member, "task", id).await;
    for ticket in [
        altered.as_str(),
        &expired,
        &foreign_key,
        &other_page,
        &as_task,
        "x",
        "",
    ] {
        assert_eq!(
            server.socket(&url(ticket), None, Some("null")).await.err(),
            Some(401),
            "{ticket}"
        );
    }
    // The ticket of a session that signed out.
    let (_, session) = add_user(&server, Some(&users.workspace), "pat@example.com", "Pat").await;
    let ticket = server.socket_ticket(&session, "page", id).await;
    assert!(
        server
            .socket(&url(&ticket), None, Some("null"))
            .await
            .is_ok()
    );
    let (status, _) = server
        .api(&session, "POST", "/api/v1/auth/logout", None)
        .await;
    assert_eq!(status, 204);
    assert_eq!(
        server.socket(&url(&ticket), None, Some("null")).await.err(),
        Some(401)
    );
    // Outside a co-editing socket a ticket does not lift the Origin rule.
    let ticket = server.socket_ticket(&users.member, "page", id).await;
    let live = format!(
        "{}/api/v1/workspaces/{}/live?ticket={ticket}",
        server.base.replacen("http", "ws", 1),
        users.workspace
    );
    assert_eq!(
        server.socket(&live, None, Some("null")).await.err(),
        Some(403)
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

#[tokio::test]
async fn a_page_write_past_the_memory_budget_is_busy() {
    let root = TempDir::new().unwrap();
    let config = CollabConfig {
        memory_budget_bytes: 1,
        ..fast_config()
    };
    let server = Server::start(&root, config).await;
    let users = setup(&server).await;
    let stored = create_page(
        &server,
        &users,
        &users.owner,
        json!({"title": "Stored"}),
        "a",
    )
    .await;
    let stored_id = stored["id"].as_str().unwrap();
    let open = create_page(&server, &users, &users.owner, json!({"title": "Open"}), "b").await;
    let open_id = open["id"].as_str().unwrap();

    // `stored` gets a stored document, then its room goes; someone keeps `open` open
    drop(Client::open(&server, &users, &users.owner, stored_id).await);
    let hub = CollabHub::existing(&server.database).unwrap();
    wait_until("the idle room to go", || async { hub.stats().0 == 0 }).await;
    let _editor = Client::open(&server, &users, &users.owner, open_id).await;

    let page = get_page(&server, &users, &users.owner, stored_id).await;
    let (status, body) = server
        .api(
            &users.owner,
            "PATCH",
            &format!("/api/v1/workspaces/{}/pages/{stored_id}", users.workspace),
            Some(json!({"expected_version": page["version"], "content": []})),
        )
        .await;
    assert_eq!(status, 503, "{body}");
    assert_eq!(body["code"], "page_busy");
    server.stop().await;
}

#[tokio::test]
async fn a_read_only_cloud_workspace_syncs_documents_but_drops_edits() {
    let root = TempDir::new().unwrap();
    let mut config = app_config(&root);
    config.billing = Some(orbit_platform::BillingConfig::new(
        orbit_platform::Secret::new("sk_test_unused"),
        orbit_platform::Secret::new("whsec_unused"),
        vec![orbit_platform::Plan {
            key: "team".to_owned(),
            price_id: "price_team".to_owned(),
        }],
    ));
    let server = Server::start_with(config, fast_config()).await;
    let users = setup(&server).await;
    // Not the exempt root workspace and past its trial, so without a subscription it is read-only.
    sqlx::query("UPDATE workspaces SET created_at = 0, billing_exempt = 0 WHERE id = $1")
        .bind(&users.workspace)
        .execute(server.database.pool())
        .await
        .unwrap();
    let set_status = |status: &'static str| {
        let database = server.database.clone();
        let workspace = users.workspace.clone();
        async move {
            sqlx::query(
                "INSERT INTO workspace_subscriptions (workspace_id, stripe_customer_id, status, updated_at) \
                 VALUES ($1, 'cus_test', $2, 0) \
                 ON CONFLICT (workspace_id) DO UPDATE SET status = excluded.status",
            )
            .bind(workspace)
            .bind(status)
            .execute(database.pool())
            .await
            .unwrap();
        }
    };
    set_status("active").await;
    let page = create_page(
        &server,
        &users,
        &users.owner,
        json!({"title": "Paid"}),
        "abc",
    )
    .await;
    let id = page["id"].as_str().unwrap();
    let mut editing = Client::open(&server, &users, &users.owner, id).await;

    set_status("canceled").await;
    assert_eq!(
        editing.wait_close().await,
        4402,
        "an editing socket closes once the workspace is read-only"
    );
    let mut a = Client::open(&server, &users, &users.owner, id).await;
    let mut b = Client::open(&server, &users, &users.member, id).await;
    assert_eq!(
        b.text(),
        "abc",
        "a read-only workspace still syncs the document"
    );
    b.type_text(3, "X").await;
    assert!(
        !a.pump(Duration::from_millis(1200), |client, _| client.text()
            != "abc"
            || client.closed.is_some())
            .await,
        "edits from a read-only workspace are not broadcast, and read-only sockets stay open \
         past the access recheck"
    );
    assert_eq!(b.closed, None);
    let stored = get_page(&server, &users, &users.owner, id).await;
    assert_eq!(stored["content"][0]["content"][0]["text"], "abc");
    let c = Client::open(&server, &users, &users.owner, id).await;
    assert_eq!(
        c.text(),
        "abc",
        "edits from a read-only workspace are not applied"
    );
    server.stop().await;
}

#[tokio::test]
async fn a_lapsed_subscription_closes_editing_sockets_on_every_server() {
    let Some(database) = shared_postgres::SharedDatabase::create().await else {
        return; // several servers need a Postgres database
    };
    // No periodic recheck within the test: only the cluster message can close the socket.
    let config = || CollabConfig {
        recheck_interval: Duration::from_secs(600),
        ..fast_config()
    };
    let billing = || {
        Some(orbit_platform::BillingConfig::new(
            orbit_platform::Secret::new("sk_test_unused"),
            orbit_platform::Secret::new("whsec_unused"),
            vec![orbit_platform::Plan {
                key: "team".to_owned(),
                price_id: "price_team".to_owned(),
            }],
        ))
    };
    let (root_one, root_two) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    let shared = |root: &TempDir| {
        let mut app = app_config(root);
        app.data.database_url = Some(database.url.clone());
        app.app_key = Some(shared_postgres::APP_KEY.parse().unwrap());
        app.billing = billing();
        app
    };
    let one = Server::start_with(shared(&root_one), config()).await;
    let users = setup(&one).await;
    let two = Server::start_with(shared(&root_two), config()).await;
    let workspace: Id = users.workspace.parse().unwrap();
    // Not the exempt root workspace and past its trial, so without a subscription it is read-only.
    sqlx::query("UPDATE workspaces SET created_at = 0, billing_exempt = 0 WHERE id = $1")
        .bind(&users.workspace)
        .execute(one.database.pool())
        .await
        .unwrap();
    let subscription = |status: &str| SubscriptionSnapshot {
        customer_id: "cus_test".to_owned(),
        subscription_id: "sub_test".to_owned(),
        plan: Some("team".to_owned()),
        status: status.to_owned(),
        cancel_at_period_end: false,
        current_period_end: None,
    };
    billing_repository::apply_subscription(
        &one.database,
        workspace,
        &subscription("active"),
        TimestampMillis::now(),
    )
    .await
    .unwrap();
    let page = create_page(&one, &users, &users.owner, json!({"title": "Pad"}), "x").await;
    let id = page["id"].as_str().unwrap();
    let mut editing = Client::open(&two, &users, &users.member, id).await;

    billing_repository::apply_subscription(
        &one.database,
        workspace,
        &subscription("canceled"),
        TimestampMillis::now(),
    )
    .await
    .unwrap();
    assert_eq!(
        editing.wait_close().await,
        4402,
        "the other server closes its editing socket"
    );
    let reconnected = Client::open(&two, &users, &users.member, id).await;
    assert_eq!(reconnected.text(), "x", "the client reconnects read-only");
    one.stop().await;
    two.stop().await;
    database.drop().await;
}

#[tokio::test]
async fn updates_waiting_for_a_missing_dependency_count_against_the_document_limit() {
    let root = TempDir::new().unwrap();
    let server = Server::start(
        &root,
        CollabConfig {
            max_frame_bytes: 64 * 1024,
            max_doc_bytes: 128 * 1024,
            ..fast_config()
        },
    )
    .await;
    let users = setup(&server).await;
    let page = create_page(&server, &users, &users.owner, json!({"title": "Gap"}), "g").await;
    let id = page["id"].as_str().unwrap();
    let mut client = Client::open(&server, &users, &users.member, id).await;

    // A second client id whose first edit never reaches the server: every later edit of it
    // depends on that one, so yrs keeps them pending and no delta is produced.
    let ghost = new_doc(None);
    let state = client
        .doc
        .transact()
        .encode_state_as_update_v1(&Default::default());
    ghost
        .transact_mut()
        .apply_update(Update::decode_v1(&state).unwrap())
        .unwrap();
    let edit = |text: &str| {
        let before = ghost.transact().state_vector();
        {
            let mut txn = ghost.transact_mut();
            paragraph_text(&ghost, &mut txn, 0).insert(&mut txn, 0, text);
        }
        ghost.transact().encode_state_as_update_v1(&before)
    };
    let _withheld = edit("a");
    let gapped: Vec<Vec<u8>> = (0..4).map(|_| edit(&"z".repeat(40 * 1024))).collect();

    // What yrs does with them: pending, not integrated, no update event.
    let probe = new_doc(None);
    probe
        .transact_mut()
        .apply_update(Update::decode_v1(&state).unwrap())
        .unwrap();
    let events = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = Arc::clone(&events);
    probe
        .observe_update_v1("probe", move |_, _| {
            counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        })
        .unwrap();
    for update in &gapped {
        probe
            .transact_mut()
            .apply_update(Update::decode_v1(update).unwrap())
            .unwrap();
    }
    assert!(probe.transact().has_missing_updates());
    assert_eq!(events.load(std::sync::atomic::Ordering::Relaxed), 0);

    for update in gapped {
        client
            .send(Message::Sync(SyncMessage::Update(update)))
            .await;
    }
    assert_eq!(client.wait_close().await, 4413);
    server.stop().await;
}

#[tokio::test]
async fn too_deeply_nested_updates_are_refused_and_the_room_reloads_without_them() {
    let root = TempDir::new().unwrap();
    let server = Server::start(&root, fast_config()).await;
    let users = setup(&server).await;
    let page = create_page(&server, &users, &users.owner, json!({"title": "Deep"}), "d").await;
    let id = page["id"].as_str().unwrap();
    let mut owner = Client::open(&server, &users, &users.owner, id).await;
    let mut member = Client::open(&server, &users, &users.member, id).await;
    owner.type_text(1, "keep").await;
    assert!(
        member
            .pump(Duration::from_secs(5), |client, _| client.text() == "dkeep")
            .await
    );

    // 100 nested blocks (200 elements) each time: every update alone passes the check before
    // applying; the second one takes the document past the 256 elements it may reach.
    let nest = |txn: &mut TransactionMut| {
        let fragment = txn.get_xml_fragment(FRAGMENT).unwrap();
        let Some(XmlOut::Element(group)) = fragment.get(txn, 0) else {
            panic!("no block group")
        };
        let Some(XmlOut::Element(mut container)) = group.get(txn, 0) else {
            panic!("no block")
        };
        while let Some(XmlOut::Element(group)) = container.get(txn, 1) {
            let Some(XmlOut::Element(inner)) = group.get(txn, 0) else {
                break;
            };
            container = inner;
        }
        for _ in 0..100 {
            let group = container.push_back(txn, yrs::XmlElementPrelim::empty("blockGroup"));
            container = group.push_back(txn, yrs::XmlElementPrelim::empty("blockContainer"));
            let paragraph = container.push_back(txn, yrs::XmlElementPrelim::empty("paragraph"));
            paragraph.push_back(txn, yrs::XmlTextPrelim::new("x"));
        }
    };
    let xs = |client: &Client| client.text().matches('x').count();
    member.edit(|_, txn| nest(txn)).await;
    assert!(
        owner
            .pump(Duration::from_secs(5), |client, _| xs(client) == 100)
            .await
    );
    member.edit(|_, txn| nest(txn)).await;
    assert_eq!(member.wait_close().await, 4413);
    assert_eq!(owner.wait_close().await, 1012, "the room reloads");
    assert_eq!(xs(&owner), 100, "nobody else received it");

    let fresh = Client::open(&server, &users, &users.owner, id).await;
    assert!(
        fresh.text().starts_with("dkeep"),
        "the edits before it are kept"
    );
    assert_eq!(xs(&fresh), 100);
    server.stop().await;
}

#[tokio::test]
async fn mentions_are_repaired_live_in_any_update() {
    let root = TempDir::new().unwrap();
    // No projection (which repairs too) within the test.
    let server = Server::start(
        &root,
        CollabConfig {
            project_idle: Duration::from_secs(600),
            project_max_delay: Duration::from_secs(600),
            ..fast_config()
        },
    )
    .await;
    let users = setup(&server).await;
    let page = create_page(&server, &users, &users.owner, json!({"title": "M"}), "m").await;
    let id = page["id"].as_str().unwrap();
    let mut owner = Client::open(&server, &users, &users.owner, id).await;
    let mut member = Client::open(&server, &users, &users.member, id).await;
    let mentions = |doc: &Doc| {
        let mut txn = doc.transact_mut();
        block_element(&mut txn, 0)
            .children(&txn)
            .filter(|child| matches!(child, XmlOut::Element(element) if element.tag().as_ref() == "mention"))
            .count()
    };
    // Neither `link` nor `url` in this update.
    member
        .edit(|_, txn| {
            let paragraph = block_element(txn, 0);
            let mention = paragraph.push_back(txn, yrs::XmlElementPrelim::empty("mention"));
            mention.insert_attribute(txn, "userId", "not-a-user");
            mention.insert_attribute(txn, "name", "Mallory");
        })
        .await;
    assert_eq!(mentions(&member.doc), 1);
    assert!(
        member
            .pump(Duration::from_secs(5), |client, _| mentions(&client.doc)
                == 0)
            .await,
        "the server removes the mention"
    );
    assert!(
        owner
            .pump(Duration::from_secs(2), |client, _| client.text() == "m"
                && mentions(&client.doc) == 0)
            .await
    );
    server.stop().await;
}

/// A second client id on `client`'s document, for hand-made updates the client's own document
/// never applies (yrs would recurse over them there too).
fn ghost_of(client: &Client) -> Doc {
    let ghost = new_doc(None);
    let state = client
        .doc
        .transact()
        .encode_state_as_update_v1(&Default::default());
    ghost
        .transact_mut()
        .apply_update(Update::decode_v1(&state).unwrap())
        .unwrap();
    ghost
}

/// Nests `levels` more elements in the ghost's first paragraph (inside the last one `tip`
/// names), returning the update and the new tip.
fn nest(
    ghost: &Doc,
    tip: Option<yrs::XmlElementRef>,
    levels: usize,
) -> (Vec<u8>, yrs::XmlElementRef) {
    let before = ghost.transact().state_vector();
    let tip = {
        let mut txn = ghost.transact_mut();
        let mut node = match tip {
            Some(tip) => tip,
            None => block_element(&mut txn, 0),
        };
        for _ in 0..levels {
            node = node.push_back(&mut txn, yrs::XmlElementPrelim::empty("x"));
        }
        node
    };
    (ghost.transact().encode_state_as_update_v1(&before), tip)
}

/// A v1 update that only deletes `id`.
fn delete_only(id: yrs::ID) -> Vec<u8> {
    use yrs::updates::encoder::Encode;
    let mut deletes = yrs::IdSet::default();
    deletes.insert(id, 1);
    let mut update = vec![0];
    update.extend(deletes.encode_v1());
    update
}

async fn assert_alive(server: &Server, users: &Users, id: &str, text: &str) {
    let fresh = Client::open(server, users, &users.owner, id).await;
    assert_eq!(fresh.text(), text, "the server still serves the page");
}

#[tokio::test]
async fn updates_that_would_make_yrs_recurse_too_deep_are_refused_before_yrs_reads_them() {
    let root = TempDir::new().unwrap();
    let server = Server::start(&root, fast_config()).await;
    let users = setup(&server).await;
    let page = create_page(&server, &users, &users.owner, json!({"title": "R"}), "r").await;
    let id = page["id"].as_str().unwrap();

    // One update: a chain 20 000 elements deep and the deletion of its root (yrs deletes and
    // collects a subtree recursively).
    let mut client = Client::open(&server, &users, &users.member, id).await;
    let ghost = ghost_of(&client);
    let root_id = yrs::ID::new(
        ghost.client_id(),
        ghost.transact().state_vector().get(&ghost.client_id()),
    );
    let (chain, _) = nest(&ghost, None, 20_000);
    let update = yrs::merge_updates_v1([chain, delete_only(root_id)]).unwrap();
    client
        .send(Message::Sync(SyncMessage::Update(update)))
        .await;
    assert_eq!(client.wait_close().await, 4413);
    assert_alive(&server, &users, id, "r").await;

    // The same chain in pieces yrs holds back (they nest inside an element it has not seen),
    // then that element with the deletion: everything would integrate, and be deleted, at once.
    let mut client = Client::open(&server, &users, &users.member, id).await;
    let ghost = ghost_of(&client);
    let (withheld, withheld_tip) = nest(&ghost, None, 1);
    let root_id = yrs::ID::new(
        ghost.client_id(),
        ghost.transact().state_vector().get(&ghost.client_id()),
    );
    let mut tip = Some(withheld_tip);
    let mut closed = None;
    for _ in 0..100 {
        let (piece, next) = nest(&ghost, tip, 200);
        tip = Some(next);
        client.send(Message::Sync(SyncMessage::Update(piece))).await;
        if client
            .pump(Duration::from_millis(20), |client, _| {
                client.closed.is_some()
            })
            .await
        {
            closed = client.closed;
            break;
        }
    }
    if closed.is_none() {
        let update = yrs::merge_updates_v1([withheld, delete_only(root_id)]).unwrap();
        client
            .send(Message::Sync(SyncMessage::Update(update)))
            .await;
        closed = Some(client.wait_close().await);
    }
    assert_eq!(closed, Some(4413));
    assert_alive(&server, &users, id, "r").await;

    // An attribute value 100 000 arrays deep (yrs decodes `Any` recursively).
    let mut client = Client::open(&server, &users, &users.member, id).await;
    let mut update = vec![1, 1, 7, 0, 8, 1, 1, b'x', 1];
    for _ in 0..100_000 {
        update.extend([117, 1]);
    }
    update.extend([126, 0]);
    client
        .send(Message::Sync(SyncMessage::Update(update)))
        .await;
    assert_eq!(client.wait_close().await, 4413);
    assert_alive(&server, &users, id, "r").await;
    server.stop().await;
}
