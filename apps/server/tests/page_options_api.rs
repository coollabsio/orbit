//! Page options over the real server: full width, page lock (REST, version restore, imports and
//! the co-editing socket with a Rust yrs client), "last edited by" and recent pages.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use orbit_platform::{
    AuthenticatedUser, Config, EnvironmentMode, Id, PasswordService, TimestampMillis,
};
use orbit_server::app::App;
use orbit_server::collab::blocknote::{FRAGMENT, doc_to_blocks, new_doc};
use orbit_server::collab::{CollabConfig, CollabHub};
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::repositories::page_versions::SaveOrigin;
use orbit_server::repositories::pages::{PageChanges, PageRepository};
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::{self, Message as Ws};
use tokio_util::sync::CancellationToken;
use yrs::encoding::read::Cursor;
use yrs::sync::awareness::AwarenessUpdateEntry;
use yrs::sync::{AwarenessUpdate, Message, MessageReader, SyncMessage};
use yrs::updates::decoder::{Decode, DecoderV1};
use yrs::updates::encoder::Encode;
use yrs::{Doc, ReadTxn, Text, Transact, TransactionMut, Update, XmlFragment, XmlOut, XmlTextRef};

const COOKIE: &str = "orbit_session_dev";

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

fn fast_config() -> CollabConfig {
    CollabConfig {
        flush_after: Duration::from_millis(20),
        project_idle: Duration::from_millis(200),
        project_max_delay: Duration::from_millis(800),
        room_ttl: Duration::from_millis(300),
        recheck_interval: Duration::from_millis(500),
        tick: Duration::from_millis(20),
        ..CollabConfig::default()
    }
}

fn app_config(root: &TempDir) -> Config {
    let mut config = Config {
        environment: EnvironmentMode::Development,
        ..Config::default()
    };
    config.data.database = root.path().join("data/orbit.sqlite");
    config.data.attachments = root.path().join("data/attachments");
    config.data.backups = root.path().join("backups");
    config.rate_limits.general_per_minute = 100_000;
    config
}

struct Server {
    base: String,
    database: orbit_platform::Database,
    shutdown: CancellationToken,
    task: tokio::task::JoinHandle<()>,
    http: reqwest::Client,
}

impl Server {
    async fn start(root: &TempDir, collab: CollabConfig) -> Self {
        let app = App::build(app_config(root)).await.unwrap();
        CollabHub::install(app.database(), collab);
        let database = app.database().clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let shutdown = CancellationToken::new();
        let token = shutdown.clone();
        let task = tokio::spawn(async move {
            app.serve(listener, token).await.unwrap();
        });
        Self {
            base,
            database,
            shutdown,
            task,
            http: reqwest::Client::new(),
        }
    }

    /// Stops the server (documents are written) and releases the database.
    async fn stop(self) {
        self.shutdown.cancel();
        self.task.await.unwrap();
        drop(self.database);
    }

    async fn api(
        &self,
        token: &str,
        method: &str,
        path: &str,
        body: Option<Value>,
    ) -> (u16, Value) {
        let mut request = self
            .http
            .request(method.parse().unwrap(), format!("{}{path}", self.base))
            .header("origin", &self.base)
            .header("cookie", format!("{COOKIE}={token}"));
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.unwrap();
        let status = response.status().as_u16();
        let text = response.text().await.unwrap();
        (
            status,
            if text.is_empty() {
                Value::Null
            } else {
                serde_json::from_str(&text).unwrap()
            },
        )
    }

    fn ws_url(&self, workspace: &str, page: &str, query: &str) -> String {
        format!(
            "{}/api/v1/workspaces/{workspace}/pages/{page}/collab?{query}",
            self.base.replacen("http", "ws", 1)
        )
    }

    /// Opens a raw socket; `Err(status)` when the handshake is refused.
    async fn socket(
        &self,
        url: &str,
        token: Option<&str>,
        origin: Option<&str>,
    ) -> Result<Socket, u16> {
        let mut request = url.into_client_request().unwrap();
        if let Some(origin) = origin {
            request
                .headers_mut()
                .insert("origin", HeaderValue::from_str(origin).unwrap());
        }
        if let Some(token) = token {
            request.headers_mut().insert(
                "cookie",
                HeaderValue::from_str(&format!("{COOKIE}={token}")).unwrap(),
            );
        }
        match tokio_tungstenite::connect_async(request).await {
            Ok((socket, _)) => Ok(socket),
            Err(tungstenite::Error::Http(response)) => Err(response.status().as_u16()),
            Err(error) => panic!("unexpected handshake error: {error}"),
        }
    }
}

struct Users {
    workspace: String,
    owner: String,
    owner_id: Id,
    member: String,
    member_id: Id,
}

async fn setup(server: &Server) -> Users {
    let identity = IdentityRepository::new(server.database.clone());
    let now = TimestampMillis::now();
    identity
        .store_setup_token(
            "collab-setup",
            TimestampMillis::from_millis(now.as_millis() + 60_000),
        )
        .await
        .unwrap();
    let setup = identity
        .complete_setup(
            SetupRequest {
                token: "collab-setup".to_owned(),
                email: "owner@example.com".to_owned(),
                display_name: "Olivia Owner".to_owned(),
                password_hash: PasswordService::default()
                    .hash("correct horse battery")
                    .unwrap(),
                workspace_name: "Orbit".to_owned(),
                project_name: "General".to_owned(),
            },
            now,
        )
        .await
        .unwrap();
    let workspace = setup.workspace_id.to_string();
    let (member_id, member) =
        add_user(server, Some(&workspace), "member@example.com", "Mia Member").await;
    Users {
        workspace,
        owner: setup.session.token.clone(),
        owner_id: setup.user_id,
        member,
        member_id,
    }
}

async fn add_user(
    server: &Server,
    workspace: Option<&str>,
    email: &str,
    name: &str,
) -> (Id, String) {
    let id = Id::new_v7();
    let now = TimestampMillis::now();
    let pool = server.database.pool();
    sqlx::query(
        "INSERT INTO users (id, email, normalized_email, display_name, password_hash, created_at, updated_at) \
         VALUES (?, ?, ?, ?, 'unused', ?, ?)",
    )
    .bind(id.to_string())
    .bind(email)
    .bind(email)
    .bind(name)
    .bind(now.as_millis())
    .bind(now.as_millis())
    .execute(pool)
    .await
    .unwrap();
    if let Some(workspace) = workspace {
        sqlx::query(
            "INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, updated_at) \
             VALUES (?, ?, ?, 'member', 0, ?, ?)",
        )
        .bind(Id::new_v7().to_string())
        .bind(workspace)
        .bind(id.to_string())
        .bind(now.as_millis())
        .bind(now.as_millis())
        .execute(pool)
        .await
        .unwrap();
    }
    let session = IdentityRepository::new(server.database.clone())
        .create_session(
            &AuthenticatedUser {
                id,
                email: email.to_owned(),
                display_name: name.to_owned(),
            },
            now,
        )
        .await
        .unwrap();
    (id, session.token)
}

async fn create_page(
    server: &Server,
    users: &Users,
    token: &str,
    body: Value,
    text: &str,
) -> Value {
    let (status, page) = server
        .api(
            token,
            "POST",
            &format!("/api/v1/workspaces/{}/pages", users.workspace),
            Some(body),
        )
        .await;
    assert_eq!(status, 201, "{page}");
    let id = page["id"].as_str().unwrap().to_owned();
    let (status, page) = server
        .api(
            token,
            "PATCH",
            &format!("/api/v1/workspaces/{}/pages/{id}", users.workspace),
            Some(json!({
                "expected_version": page["version"],
                "content": [{"id": "b1", "type": "paragraph", "content": [{"type": "text", "text": text, "styles": {}}]}],
            })),
        )
        .await;
    assert_eq!(status, 200, "{page}");
    page
}

async fn get_page(server: &Server, users: &Users, token: &str, id: &str) -> Value {
    let (status, page) = server
        .api(
            token,
            "GET",
            &format!("/api/v1/workspaces/{}/pages/{id}", users.workspace),
            None,
        )
        .await;
    assert_eq!(status, 200, "{page}");
    page
}

/// A y-websocket-like client.
struct Client {
    doc: Doc,
    socket: Socket,
    closed: Option<u16>,
    /// Awareness states seen (client id -> JSON; "null" when removed).
    awareness: HashMap<u64, String>,
}

impl Client {
    async fn open(server: &Server, users: &Users, token: &str, page_id: &str) -> Self {
        let epoch = get_page(server, users, token, page_id).await["collab_epoch"]
            .as_str()
            .unwrap()
            .to_owned();
        Self::open_with(server, users, token, page_id, &format!("v=1&epoch={epoch}"))
            .await
            .expect("synced")
    }

    /// Connects and syncs; `Err(close code)` when the server closes first.
    async fn open_with(
        server: &Server,
        users: &Users,
        token: &str,
        page_id: &str,
        query: &str,
    ) -> Result<Self, u16> {
        let url = server.ws_url(&users.workspace, page_id, query);
        let socket = server
            .socket(&url, Some(token), Some(&server.base))
            .await
            .expect("handshake");
        let mut client = Self {
            doc: new_doc(None),
            socket,
            closed: None,
            awareness: HashMap::new(),
        };
        let sv = client.doc.transact().state_vector();
        client.send(Message::Sync(SyncMessage::SyncStep1(sv))).await;
        client
            .pump(Duration::from_secs(5), |_, synced| synced)
            .await;
        match client.closed {
            Some(code) => Err(code),
            None => Ok(client),
        }
    }

    async fn send(&mut self, message: Message) {
        let _ = self
            .socket
            .send(Ws::Binary(message.encode_v1().into()))
            .await;
    }

    async fn edit(&mut self, f: impl FnOnce(&Doc, &mut TransactionMut)) {
        let before = self.doc.transact().state_vector();
        {
            let doc = self.doc.clone();
            let mut txn = self.doc.transact_mut();
            f(&doc, &mut txn);
        }
        let update = self.doc.transact().encode_state_as_update_v1(&before);
        self.send(Message::Sync(SyncMessage::Update(update))).await;
    }

    async fn type_text(&mut self, at: u32, text: &str) {
        let text = text.to_owned();
        self.edit(|doc, txn| {
            let node = paragraph_text(doc, txn, 0);
            node.insert(txn, at, &text);
        })
        .await;
    }

    async fn set_awareness(&mut self, clock: u32, state: Value) {
        let mut clients = HashMap::new();
        clients.insert(
            self.doc.client_id(),
            AwarenessUpdateEntry {
                clock,
                json: Arc::from(state.to_string()),
            },
        );
        self.send(Message::Awareness(AwarenessUpdate { clients }))
            .await;
    }

    /// Reads frames until `done(self, synced)` or the socket closes or `timeout` passes.
    async fn pump(&mut self, timeout: Duration, done: impl Fn(&Self, bool) -> bool) -> bool {
        let deadline = tokio::time::Instant::now() + timeout;
        let mut synced = false;
        loop {
            if done(self, synced) {
                return true;
            }
            if self.closed.is_some() {
                return false;
            }
            let frame = match tokio::time::timeout_at(deadline, self.socket.next()).await {
                Err(_) => return false,
                Ok(None | Some(Err(_))) => {
                    self.closed.get_or_insert(1006);
                    continue;
                }
                Ok(Some(Ok(frame))) => frame,
            };
            match frame {
                Ws::Binary(data) => {
                    let mut decoder = DecoderV1::new(Cursor::new(&data));
                    let messages: Vec<_> = MessageReader::new(&mut decoder).collect();
                    for message in messages {
                        match message.unwrap() {
                            Message::Sync(SyncMessage::SyncStep1(sv)) => {
                                let update = self.doc.transact().encode_state_as_update_v1(&sv);
                                self.send(Message::Sync(SyncMessage::SyncStep2(update)))
                                    .await;
                            }
                            Message::Sync(SyncMessage::SyncStep2(update)) => {
                                self.apply(&update);
                                synced = true;
                            }
                            Message::Sync(SyncMessage::Update(update)) => self.apply(&update),
                            Message::Awareness(update) => {
                                for (id, entry) in update.clients {
                                    self.awareness.insert(id.get(), entry.json.to_string());
                                }
                            }
                            _ => {}
                        }
                    }
                }
                Ws::Close(frame) => {
                    self.closed = Some(frame.map_or(1005, |frame| u16::from(frame.code)));
                }
                _ => {}
            }
        }
    }

    fn apply(&self, update: &[u8]) {
        self.doc
            .transact_mut()
            .apply_update(Update::decode_v1(update).unwrap())
            .unwrap();
    }

    fn text(&self) -> String {
        let blocks = doc_to_blocks(&self.doc);
        orbit_server::repositories::pages::content_text(&blocks)
    }

    async fn wait_text(&mut self, expected: &str) {
        let expected = expected.to_owned();
        let ok = self
            .pump(Duration::from_secs(5), |client, _| {
                client.text() == expected
            })
            .await;
        assert!(ok, "expected {expected:?}, have {:?}", self.text());
    }

    async fn wait_close(&mut self) -> u16 {
        self.pump(Duration::from_secs(5), |client, _| client.closed.is_some())
            .await;
        self.closed.expect("socket closed")
    }
}

/// The XmlText of the `index`th top-level block's paragraph.
fn paragraph_text(doc: &Doc, txn: &mut TransactionMut, index: u32) -> XmlTextRef {
    let _ = doc;
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
    match paragraph.get(txn, 0) {
        Some(XmlOut::Text(text)) => text,
        _ => paragraph.push_back(txn, yrs::XmlTextPrelim::new("")),
    }
}

async fn wait_until<F, Fut>(what: &str, mut check: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
    while !check().await {
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for {what}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

async fn scalar_i64(server: &Server, sql: &str, page: &str) -> i64 {
    sqlx::query_scalar(sql)
        .bind(page)
        .fetch_one(server.database.pool())
        .await
        .unwrap()
}

fn pages_path(users: &Users) -> String {
    format!("/api/v1/workspaces/{}/pages", users.workspace)
}

async fn audits(server: &Server, page: &str, action: &str) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM audit_events WHERE resource_id = ? AND action = ?")
        .bind(page)
        .bind(action)
        .fetch_one(server.database.pool())
        .await
        .unwrap()
}

async fn lock(server: &Server, users: &Users, token: &str, id: &str, locked: bool) -> (u16, Value) {
    server
        .api(
            token,
            "POST",
            &format!("{}/{id}/lock", pages_path(users)),
            Some(json!({ "locked": locked })),
        )
        .await
}

async fn patch(server: &Server, users: &Users, token: &str, id: &str, body: Value) -> (u16, Value) {
    server
        .api(
            token,
            "PATCH",
            &format!("{}/{id}", pages_path(users)),
            Some(body),
        )
        .await
}

#[tokio::test]
async fn full_width_and_lock_persist_with_version_checks_and_audit() {
    let root = TempDir::new().unwrap();
    let server = Server::start(&root, fast_config()).await;
    let users = setup(&server).await;
    let page = create_page(
        &server,
        &users,
        &users.owner,
        json!({"title": "Plan"}),
        "hello",
    )
    .await;
    let id = page["id"].as_str().unwrap();
    let version = page["version"].as_u64().unwrap();
    assert_eq!(page["full_width"], false);
    assert_eq!(page["locked_at"], Value::Null);
    assert_eq!(page["locked_by"], Value::Null);
    assert_eq!(
        page["updated_by_user"],
        json!({"id": users.owner_id.to_string(), "display_name": "Olivia Owner"})
    );

    // Full width: an ordinary versioned PATCH by any member, not an edit.
    let (status, body) = patch(
        &server,
        &users,
        &users.member,
        id,
        json!({"expected_version": version + 5, "full_width": true}),
    )
    .await;
    assert_eq!(status, 409, "{body}");
    let (status, body) = patch(
        &server,
        &users,
        &users.member,
        id,
        json!({"expected_version": version, "full_width": true}),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["full_width"], true);
    assert_eq!(body["version"], version + 1);
    assert_eq!(
        body["updated_by"],
        users.owner_id.to_string(),
        "layout is not an edit"
    );
    assert_eq!(body["updated_at"], page["updated_at"]);
    assert_eq!(
        get_page(&server, &users, &users.owner, id).await["full_width"],
        true
    );
    let (status, body) = patch(
        &server,
        &users,
        &users.member,
        id,
        json!({"expected_version": version + 1, "full_width": "yes"}),
    )
    .await;
    assert!(status == 422 || status == 400, "{status} {body}");

    // Lock by a member: audited, bumps the version, names the locker.
    let (status, locked) = lock(&server, &users, &users.member, id, true).await;
    assert_eq!(status, 200, "{locked}");
    assert_eq!(locked["version"], version + 2);
    assert!(locked["locked_at"].is_string());
    assert_eq!(
        locked["locked_by"],
        json!({"id": users.member_id.to_string(), "display_name": "Mia Member"})
    );
    assert_eq!(audits(&server, id, "page.locked").await, 1);
    // Locking again changes nothing.
    let (status, again) = lock(&server, &users, &users.owner, id, true).await;
    assert_eq!(status, 200);
    assert_eq!(again["version"], version + 2);
    assert_eq!(again["locked_by"]["id"], users.member_id.to_string());
    assert_eq!(audits(&server, id, "page.locked").await, 1);
    let stored = get_page(&server, &users, &users.owner, id).await;
    assert_eq!(stored["locked_at"], locked["locked_at"]);

    // Edits are refused with 423 (whatever the version), layout and moves are not.
    let v = version + 2;
    for body in [
        json!({"expected_version": v, "title": "New"}),
        json!({"expected_version": v, "icon": "🔥"}),
        json!({"expected_version": v, "cover_url": "https://example.com/a.png"}),
        json!({"expected_version": v, "cover_position": "10,10"}),
        json!({"expected_version": v, "content": []}),
        json!({"expected_version": 0, "title": "Stale"}),
        json!({"expected_version": v, "title": "New", "full_width": false}),
    ] {
        let (status, problem) = patch(&server, &users, &users.owner, id, body.clone()).await;
        assert_eq!(status, 423, "{body} -> {problem}");
        assert_eq!(problem["code"], "page_locked");
    }
    let (status, body) = patch(
        &server,
        &users,
        &users.owner,
        id,
        json!({"expected_version": v, "full_width": false}),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let (status, moved) = server
        .api(
            &users.owner,
            "POST",
            &format!("{}/{id}/move", pages_path(&users)),
            Some(json!({"expected_version": v + 1, "parent_id": null, "position": 0})),
        )
        .await;
    assert_eq!(status, 200, "{moved}");
    assert!(moved["locked_at"].is_string(), "a move keeps the lock");
    let (status, copy) = server
        .api(
            &users.owner,
            "POST",
            &format!("{}/{id}/duplicate", pages_path(&users)),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, 201, "{copy}");
    assert_eq!(copy["locked_at"], Value::Null, "copies are not locked");
    let stored = get_page(&server, &users, &users.owner, id).await;
    assert_eq!(stored["title"], "Plan");
    assert_eq!(stored["content"][0]["content"][0]["text"], "hello");

    // Unlock by someone else: writes work again.
    let (status, unlocked) = lock(&server, &users, &users.owner, id, false).await;
    assert_eq!(status, 200, "{unlocked}");
    assert_eq!(unlocked["locked_at"], Value::Null);
    assert_eq!(unlocked["locked_by"], Value::Null);
    assert_eq!(audits(&server, id, "page.unlocked").await, 1);
    let (status, body) = patch(
        &server,
        &users,
        &users.member,
        id,
        json!({"expected_version": unlocked["version"], "title": "Plan B"}),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    // "Last edited by" follows the editor.
    assert_eq!(
        body["updated_by_user"],
        json!({"id": users.member_id.to_string(), "display_name": "Mia Member"})
    );
    assert_eq!(
        get_page(&server, &users, &users.owner, id).await["updated_by_user"]["display_name"],
        "Mia Member"
    );
    // Bad input and unknown pages.
    let (status, _) = server
        .api(
            &users.owner,
            "POST",
            &format!("{}/{id}/lock", pages_path(&users)),
            Some(json!({})),
        )
        .await;
    assert!(status == 422 || status == 400);
    let (status, body) = lock(
        &server,
        &users,
        &users.owner,
        &Id::new_v7().to_string(),
        true,
    )
    .await;
    assert_eq!(status, 404, "{body}");
    server.stop().await;
}

#[tokio::test]
async fn private_pages_cannot_be_locked_widened_or_visited_by_others() {
    let root = TempDir::new().unwrap();
    let server = Server::start(&root, fast_config()).await;
    let users = setup(&server).await;
    let private = create_page(
        &server,
        &users,
        &users.owner,
        json!({"title": "Mine", "private": true}),
        "secret",
    )
    .await;
    let id = private["id"].as_str().unwrap();
    let unknown = Id::new_v7().to_string();
    for page in [id, unknown.as_str()] {
        let (status, body) = lock(&server, &users, &users.member, page, true).await;
        assert_eq!(
            (status, body["code"].clone()),
            (404, json!("page_not_found"))
        );
        let (status, body) = patch(
            &server,
            &users,
            &users.member,
            page,
            json!({"expected_version": 1, "full_width": true}),
        )
        .await;
        assert_eq!(
            (status, body["code"].clone()),
            (404, json!("page_not_found"))
        );
        let (status, body) = server
            .api(
                &users.member,
                "POST",
                &format!("{}/{page}/visit", pages_path(&users)),
                None,
            )
            .await;
        assert_eq!(
            (status, body["code"].clone()),
            (404, json!("page_not_found"))
        );
    }
    let stored = get_page(&server, &users, &users.owner, id).await;
    assert_eq!(stored["locked_at"], Value::Null);
    assert_eq!(stored["full_width"], false);
    assert_eq!(
        scalar_i64(
            &server,
            "SELECT COUNT(*) FROM page_visits WHERE page_id = ?",
            id
        )
        .await,
        0
    );
    // The owner can lock their own private page.
    let (status, body) = lock(&server, &users, &users.owner, id, true).await;
    assert_eq!(status, 200, "{body}");
    server.stop().await;
}

#[tokio::test]
async fn version_restore_and_imports_are_refused_on_locked_pages() {
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
    // Backdate so the next edit keeps the current state as a version.
    sqlx::query("UPDATE pages SET created_at = created_at - 3600000, updated_at = updated_at - 3600000 WHERE id = ?")
        .bind(id)
        .execute(server.database.pool())
        .await
        .unwrap();
    let (status, edited) = patch(
        &server,
        &users,
        &users.owner,
        id,
        json!({"expected_version": page["version"], "content": [{"id": "b1", "type": "paragraph", "content": [{"type": "text", "text": "second", "styles": {}}]}]}),
    )
    .await;
    assert_eq!(status, 200, "{edited}");
    let (_, versions) = server
        .api(
            &users.owner,
            "GET",
            &format!("{}/{id}/versions", pages_path(&users)),
            None,
        )
        .await;
    let version_id = versions["items"][0]["id"]
        .as_str()
        .expect("a version")
        .to_owned();
    let (status, locked) = lock(&server, &users, &users.owner, id, true).await;
    assert_eq!(status, 200);
    let (status, body) = server
        .api(
            &users.owner,
            "POST",
            &format!("{}/{id}/versions/{version_id}/restore", pages_path(&users)),
            Some(json!({"expected_version": locked["version"]})),
        )
        .await;
    assert_eq!(
        (status, body["code"].clone()),
        (423, json!("page_locked")),
        "{body}"
    );
    // The Notion import writes through `update_page` (origin Import): refused as well.
    let repository = PageRepository::new(server.database.clone());
    let error = repository
        .update_page(
            users.workspace.parse().unwrap(),
            id.parse().unwrap(),
            users.owner_id,
            locked["version"].as_u64().unwrap(),
            PageChanges {
                title: Some("Imported".to_owned()),
                content: Some(vec![]),
                origin: SaveOrigin::Import,
                ..PageChanges::default()
            },
            "import-test",
            TimestampMillis::now(),
        )
        .await
        .unwrap_err();
    assert!(
        matches!(error, orbit_server::repositories::pages::PageError::Locked),
        "{error:?}"
    );
    let stored = get_page(&server, &users, &users.owner, id).await;
    assert_eq!(stored["content"][0]["content"][0]["text"], "second");
    assert_eq!(stored["title"], "Notes");
    // Unlocked, the restore goes through.
    let (_, unlocked) = lock(&server, &users, &users.owner, id, false).await;
    let (status, body) = server
        .api(
            &users.owner,
            "POST",
            &format!("{}/{id}/versions/{version_id}/restore", pages_path(&users)),
            Some(json!({"expected_version": unlocked["version"]})),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["content"][0]["content"][0]["text"], "first");
    server.stop().await;
}

#[tokio::test]
async fn collab_refuses_content_while_locked_and_closes_sockets_on_lock_changes() {
    let root = TempDir::new().unwrap();
    let server = Server::start(&root, fast_config()).await;
    let users = setup(&server).await;
    let page = create_page(
        &server,
        &users,
        &users.owner,
        json!({"title": "Live"}),
        "abc",
    )
    .await;
    let id = page["id"].as_str().unwrap();
    let mut a = Client::open(&server, &users, &users.owner, id).await;
    let mut b = Client::open(&server, &users, &users.member, id).await;
    b.type_text(3, "d").await;
    a.wait_text("abcd").await;

    // Locking closes every socket with 4423.
    let (status, _) = lock(&server, &users, &users.owner, id, true).await;
    assert_eq!(status, 200);
    assert_eq!(a.wait_close().await, 4423);
    assert_eq!(b.wait_close().await, 4423);

    // Clients reconnect (read-only) and still receive the document and presence...
    let mut a = Client::open(&server, &users, &users.owner, id).await;
    let mut b = Client::open(&server, &users, &users.member, id).await;
    assert_eq!(a.text(), "abcd");
    b.set_awareness(1, json!({"cursor": null})).await;
    let member = users.member_id.to_string();
    assert!(
        a.pump(Duration::from_secs(5), |client, _| client
            .awareness
            .values()
            .any(|state| state.contains(&member)))
            .await,
        "presence works while locked"
    );
    // ...but content updates are ignored: not applied, not broadcast, not stored.
    b.type_text(4, "X").await;
    assert_eq!(b.text(), "abcdX");
    assert!(
        !a.pump(Duration::from_millis(600), |client, _| client.text()
            != "abcd")
            .await,
        "a locked page must not broadcast edits"
    );
    let stored = get_page(&server, &users, &users.owner, id).await;
    assert_eq!(stored["content"][0]["content"][0]["text"], "abcd");
    let mut c = Client::open(&server, &users, &users.owner, id).await;
    assert_eq!(c.text(), "abcd", "a late joiner sees the locked document");
    drop(c.socket.close(None).await);

    // Unlocking closes again, so the refused edit never resurfaces; afterwards edits flow.
    let (status, _) = lock(&server, &users, &users.member, id, false).await;
    assert_eq!(status, 200);
    assert_eq!(a.wait_close().await, 4423);
    assert_eq!(b.wait_close().await, 4423);
    let mut a = Client::open(&server, &users, &users.owner, id).await;
    let mut b = Client::open(&server, &users, &users.member, id).await;
    assert_eq!(b.text(), "abcd");
    b.type_text(4, "e").await;
    a.wait_text("abcde").await;
    wait_until("projection", || async {
        get_page(&server, &users, &users.owner, id).await["content"][0]["content"][0]["text"]
            == "abcde"
    })
    .await;
    let stored = get_page(&server, &users, &users.owner, id).await;
    assert_eq!(
        stored["updated_by_user"]["display_name"], "Mia Member",
        "co-editing sets the last editor"
    );
    server.stop().await;
}

#[tokio::test]
async fn a_lock_taken_while_the_page_is_not_open_applies_on_the_next_open() {
    let root = TempDir::new().unwrap();
    let server = Server::start(&root, fast_config()).await;
    let users = setup(&server).await;
    let page = create_page(
        &server,
        &users,
        &users.owner,
        json!({"title": "Cold"}),
        "cold",
    )
    .await;
    let id = page["id"].as_str().unwrap();
    let (status, _) = lock(&server, &users, &users.owner, id, true).await;
    assert_eq!(status, 200);
    // Wait for the room loaded by the lock to be evicted, then open it fresh.
    wait_until("eviction", || async {
        CollabHub::of(&server.database).stats().0 == 0
    })
    .await;
    let mut a = Client::open(&server, &users, &users.owner, id).await;
    let mut b = Client::open(&server, &users, &users.member, id).await;
    b.type_text(4, "!").await;
    assert!(
        !a.pump(Duration::from_millis(600), |client, _| client.text()
            != "cold")
            .await
    );
    assert_eq!(
        get_page(&server, &users, &users.owner, id).await["content"][0]["content"][0]["text"],
        "cold"
    );
    server.stop().await;
}

async fn visit(server: &Server, users: &Users, token: &str, id: &str) -> u16 {
    server
        .api(
            token,
            "POST",
            &format!("{}/{id}/visit", pages_path(users)),
            None,
        )
        .await
        .0
}

async fn recent(server: &Server, users: &Users, token: &str, query: &str) -> (u16, Vec<String>) {
    let (status, body) = server
        .api(
            token,
            "GET",
            &format!("{}/recent{query}", pages_path(users)),
            None,
        )
        .await;
    let ids = body["items"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .map(|item| item["id"].as_str().unwrap().to_owned())
                .collect()
        })
        .unwrap_or_default();
    (status, ids)
}

#[tokio::test]
async fn recent_pages_are_ordered_limited_private_and_scoped() {
    let root = TempDir::new().unwrap();
    let server = Server::start(&root, fast_config()).await;
    let users = setup(&server).await;
    let mut ids = Vec::new();
    for title in ["One", "Two", "Three"] {
        let page = create_page(
            &server,
            &users,
            &users.owner,
            json!({"title": title}),
            title,
        )
        .await;
        ids.push(page["id"].as_str().unwrap().to_owned());
    }
    let (status, empty) = recent(&server, &users, &users.member, "").await;
    assert_eq!((status, empty.len()), (200, 0));
    for id in &ids {
        assert_eq!(visit(&server, &users, &users.member, id).await, 204);
        tokio::time::sleep(Duration::from_millis(3)).await;
    }
    assert_eq!(visit(&server, &users, &users.member, &ids[0]).await, 204);
    let (_, list) = recent(&server, &users, &users.member, "").await;
    assert_eq!(list, [ids[0].clone(), ids[2].clone(), ids[1].clone()]);
    let (_, list) = recent(&server, &users, &users.member, "?limit=2").await;
    assert_eq!(list, [ids[0].clone(), ids[2].clone()]);
    let (_, body) = server
        .api(
            &users.member,
            "GET",
            &format!("{}/recent", pages_path(&users)),
            None,
        )
        .await;
    assert_eq!(body["items"][0]["title"], "One");
    assert!(body["items"][0]["visited_at"].is_string());
    for query in ["?limit=0", "?limit=51", "?limit=x", "?other=1"] {
        let (status, _) = recent(&server, &users, &users.member, query).await;
        assert!(status == 422 || status == 400, "{query}: {status}");
    }
    // Visits are per user: the owner saw nothing.
    assert_eq!(recent(&server, &users, &users.owner, "").await.1.len(), 0);

    // Privacy: a visited page moved into the owner's private space disappears for the member,
    // and comes back when it is shared again.
    let page = get_page(&server, &users, &users.owner, &ids[2]).await;
    let (status, moved) = server
        .api(
            &users.owner,
            "POST",
            &format!("{}/{}/move", pages_path(&users), ids[2]),
            Some(json!({"expected_version": page["version"], "parent_id": null, "private": true, "position": 0})),
        )
        .await;
    assert_eq!(status, 200, "{moved}");
    assert_eq!(
        recent(&server, &users, &users.member, "").await.1,
        [ids[0].clone(), ids[1].clone()]
    );
    assert_eq!(visit(&server, &users, &users.member, &ids[2]).await, 404);
    // Trashed pages drop out; purged ones take their rows along.
    let page = get_page(&server, &users, &users.owner, &ids[0]).await;
    let (status, _) = server
        .api(
            &users.owner,
            "DELETE",
            &format!(
                "{}/{}?expected_version={}",
                pages_path(&users),
                ids[0],
                page["version"]
            ),
            None,
        )
        .await;
    assert_eq!(status, 204);
    assert_eq!(
        recent(&server, &users, &users.member, "").await.1,
        [ids[1].clone()]
    );
    let trashed_version = page["version"].as_u64().unwrap() + 1;
    let (status, body) = server
        .api(
            &users.owner,
            "DELETE",
            &format!(
                "{}/{}/permanent?expected_version={trashed_version}",
                pages_path(&users),
                ids[0]
            ),
            None,
        )
        .await;
    assert_eq!(status, 204, "{body}");
    assert_eq!(
        scalar_i64(
            &server,
            "SELECT COUNT(*) FROM page_visits WHERE page_id = ?",
            &ids[0]
        )
        .await,
        0
    );

    // Another workspace: its pages cannot be visited through this one, and lists stay apart.
    let other =
        orbit_server::repositories::workspaces::WorkspaceRepository::new(server.database.clone())
            .create(
                users.owner_id,
                "Other".to_owned(),
                "options-test",
                TimestampMillis::now(),
            )
            .await
            .unwrap()
            .id
            .to_string();
    let (status, foreign) = server
        .api(
            &users.owner,
            "POST",
            &format!("/api/v1/workspaces/{other}/pages"),
            Some(json!({"title": "Elsewhere"})),
        )
        .await;
    assert_eq!(status, 201, "{foreign}");
    let foreign_id = foreign["id"].as_str().unwrap();
    assert_eq!(visit(&server, &users, &users.owner, foreign_id).await, 404);
    let (status, _) = server
        .api(
            &users.owner,
            "POST",
            &format!("/api/v1/workspaces/{other}/pages/{foreign_id}/visit"),
            None,
        )
        .await;
    assert_eq!(status, 204);
    assert_eq!(recent(&server, &users, &users.owner, "").await.1.len(), 0);
    let (_, body) = server
        .api(
            &users.owner,
            "GET",
            &format!("/api/v1/workspaces/{other}/pages/recent"),
            None,
        )
        .await;
    assert_eq!(body["items"][0]["id"], foreign_id);
    // Non-members cannot use another workspace's list.
    let (status, _) = server
        .api(
            &users.member,
            "GET",
            &format!("/api/v1/workspaces/{other}/pages/recent"),
            None,
        )
        .await;
    assert!(status == 404 || status == 403, "{status}");
    server.stop().await;
}

#[tokio::test]
async fn visits_are_pruned_to_the_newest_fifty() {
    let root = TempDir::new().unwrap();
    let server = Server::start(&root, fast_config()).await;
    let users = setup(&server).await;
    let mut ids = Vec::new();
    for index in 0..55 {
        let (status, page) = server
            .api(
                &users.owner,
                "POST",
                &pages_path(&users),
                Some(json!({"title": format!("P{index}")})),
            )
            .await;
        assert_eq!(status, 201);
        let id = page["id"].as_str().unwrap().to_owned();
        assert_eq!(visit(&server, &users, &users.owner, &id).await, 204);
        ids.push(id);
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM page_visits WHERE user_id = ?")
        .bind(users.owner_id.to_string())
        .fetch_one(server.database.pool())
        .await
        .unwrap();
    assert_eq!(rows, 50);
    let (_, list) = recent(&server, &users, &users.owner, "?limit=50").await;
    assert_eq!(list.len(), 50);
    assert_eq!(list[0], ids[54]);
    assert!(!list.contains(&ids[0]));
    server.stop().await;
}
