//! Shared helpers of the co-editing tests: a real server (App + TCP listener), users, and a
//! y-websocket-like client for any collaborative document (pages, tasks).
#![allow(dead_code, unused_imports)]

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
use yrs::types::text::YChange;
use yrs::updates::decoder::{Decode, DecoderV1};
use yrs::updates::encoder::Encode;
use yrs::{
    Any, ClientID, Doc, ReadTxn, Text, Transact, TransactionMut, Update, Xml, XmlFragment, XmlOut,
    XmlTextRef,
};

pub const COOKIE: &str = "orbit_session_dev";

pub type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

pub fn fast_config() -> CollabConfig {
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

pub fn app_config(root: &TempDir) -> Config {
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

pub struct Server {
    pub base: String,
    pub database: orbit_platform::Database,
    pub shutdown: CancellationToken,
    pub task: tokio::task::JoinHandle<()>,
    pub http: reqwest::Client,
}

impl Server {
    pub async fn start(root: &TempDir, collab: CollabConfig) -> Self {
        Self::start_with(app_config(root), collab).await
    }

    /// A server of the Postgres database `url`, which other servers may share.
    pub async fn start_shared(root: &TempDir, url: &str, collab: CollabConfig) -> Self {
        let mut config = app_config(root);
        config.data.database_url = Some(url.to_owned());
        config.secrets.insert(
            "app_key".to_owned(),
            orbit_platform::Secret::new(crate::shared_postgres::APP_KEY),
        );
        Self::start_with(config, collab).await
    }

    pub async fn start_with(config: Config, collab: CollabConfig) -> Self {
        let app = App::build(config).await.unwrap();
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
    pub async fn stop(self) {
        self.shutdown.cancel();
        self.task.await.unwrap();
        drop(self.database);
    }

    pub async fn api(
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

    pub fn ws_url(&self, workspace: &str, page: &str, query: &str) -> String {
        self.doc_ws_url(workspace, "pages", page, query)
    }

    /// The co-editing socket of a document; `kind` is the path segment ("pages", "tasks").
    pub fn doc_ws_url(&self, workspace: &str, kind: &str, id: &str, query: &str) -> String {
        format!(
            "{}/api/v1/workspaces/{workspace}/{kind}/{id}/collab?{query}",
            self.base.replacen("http", "ws", 1)
        )
    }

    /// Opens a raw socket; `Err(status)` when the handshake is refused.
    pub async fn socket(
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

pub struct Users {
    pub workspace: String,
    pub owner: String,
    pub owner_id: Id,
    pub member: String,
    pub member_id: Id,
}

pub async fn setup(server: &Server) -> Users {
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

pub async fn add_user(
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
         VALUES ($1, $2, $3, $4, 'unused', $5, $6)",
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
             VALUES ($1, $2, $3, 'member', 0, $4, $5)",
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

pub async fn create_page(
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

pub async fn get_page(server: &Server, users: &Users, token: &str, id: &str) -> Value {
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
pub struct Client {
    pub doc: Doc,
    pub socket: Socket,
    pub closed: Option<u16>,
    /// Awareness states seen (client id -> JSON; "null" when removed).
    pub awareness: HashMap<u64, String>,
}

impl Client {
    pub async fn open(server: &Server, users: &Users, token: &str, page_id: &str) -> Self {
        Self::open_doc(server, users, token, "pages", page_id).await
    }

    /// Opens the document `kind`/`id` at the epoch its REST resource reports.
    pub async fn open_doc(
        server: &Server,
        users: &Users,
        token: &str,
        kind: &str,
        id: &str,
    ) -> Self {
        let (status, resource) = server
            .api(
                token,
                "GET",
                &format!("/api/v1/workspaces/{}/{kind}/{id}", users.workspace),
                None,
            )
            .await;
        assert_eq!(status, 200, "{resource}");
        let epoch = resource["collab_epoch"].as_str().unwrap().to_owned();
        Self::open_doc_with(server, users, token, kind, id, &format!("v=1&epoch={epoch}"))
            .await
            .expect("synced")
    }

    /// Connects and syncs; `Err(close code)` when the server closes first.
    pub async fn open_with(
        server: &Server,
        users: &Users,
        token: &str,
        page_id: &str,
        query: &str,
    ) -> Result<Self, u16> {
        Self::open_doc_with(server, users, token, "pages", page_id, query).await
    }

    /// [`Self::open_with`] for the document `kind`/`id`.
    pub async fn open_doc_with(
        server: &Server,
        users: &Users,
        token: &str,
        kind: &str,
        id: &str,
        query: &str,
    ) -> Result<Self, u16> {
        let url = server.doc_ws_url(&users.workspace, kind, id, query);
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

    pub async fn send(&mut self, message: Message) {
        let _ = self
            .socket
            .send(Ws::Binary(message.encode_v1().into()))
            .await;
    }

    pub async fn edit(&mut self, f: impl FnOnce(&Doc, &mut TransactionMut)) {
        let before = self.doc.transact().state_vector();
        {
            let doc = self.doc.clone();
            let mut txn = self.doc.transact_mut();
            f(&doc, &mut txn);
        }
        let update = self.doc.transact().encode_state_as_update_v1(&before);
        self.send(Message::Sync(SyncMessage::Update(update))).await;
    }

    pub async fn type_text(&mut self, at: u32, text: &str) {
        let text = text.to_owned();
        self.edit(|doc, txn| {
            let node = paragraph_text(doc, txn, 0);
            node.insert(txn, at, &text);
        })
        .await;
    }

    pub async fn set_awareness(&mut self, clock: u32, state: Value) {
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
    pub async fn pump(&mut self, timeout: Duration, done: impl Fn(&Self, bool) -> bool) -> bool {
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

    pub fn apply(&self, update: &[u8]) {
        self.doc
            .transact_mut()
            .apply_update(Update::decode_v1(update).unwrap())
            .unwrap();
    }

    pub fn text(&self) -> String {
        let blocks = doc_to_blocks(&self.doc);
        orbit_server::repositories::pages::content_text(&blocks)
    }

    pub async fn wait_text(&mut self, expected: &str) {
        let expected = expected.to_owned();
        let ok = self
            .pump(Duration::from_secs(5), |client, _| {
                client.text() == expected
            })
            .await;
        assert!(ok, "expected {expected:?}, have {:?}", self.text());
    }

    pub async fn wait_close(&mut self) -> u16 {
        self.pump(Duration::from_secs(5), |client, _| client.closed.is_some())
            .await;
        self.closed.expect("socket closed")
    }
}

/// The XmlText of the `index`th top-level block's paragraph.
pub fn paragraph_text(doc: &Doc, txn: &mut TransactionMut, index: u32) -> XmlTextRef {
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

pub async fn wait_until<F, Fut>(what: &str, mut check: F)
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

pub async fn scalar_i64(server: &Server, sql: &'static str, page: &str) -> i64 {
    sqlx::query_scalar(sql)
        .bind(page)
        .fetch_one(server.database.pool())
        .await
        .unwrap()
}
