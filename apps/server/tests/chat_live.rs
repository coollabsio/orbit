//! The live socket over the real server (App + TCP listener): the handshake, who gets which
//! chat event, typing, and what a client that reconnects gets.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use orbit_platform::{
    AuthenticatedUser, Config, EnvironmentMode, Id, PasswordService, TimestampMillis,
};
use orbit_server::app::App;
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::{self, Message as Ws};
use tokio_util::sync::CancellationToken;

const COOKIE: &str = "orbit_session_dev";

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

struct Server {
    base: String,
    database: orbit_platform::Database,
    shutdown: CancellationToken,
    task: tokio::task::JoinHandle<()>,
    http: reqwest::Client,
    workspace: String,
    _root: TempDir,
}

struct User {
    id: Id,
    token: String,
}

impl Server {
    /// A server with one workspace; returns it with the workspace's owner.
    async fn start() -> (Self, User) {
        let root = tempfile::tempdir().unwrap();
        let mut config = Config {
            environment: EnvironmentMode::Development,
            ..Config::default()
        };
        config.data.database = root.path().join("data/orbit.sqlite");
        config.data.attachments = root.path().join("data/attachments");
        config.data.backups = root.path().join("backups");
        config.rate_limits.general_per_minute = 100_000;
        let app = App::build(config).await.unwrap();
        let database = app.database().clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let shutdown = CancellationToken::new();
        let token = shutdown.clone();
        let task = tokio::spawn(async move {
            app.serve(listener, token).await.unwrap();
        });

        let identity = IdentityRepository::new(database.clone());
        let now = TimestampMillis::now();
        identity
            .store_setup_token(
                "live-setup",
                TimestampMillis::from_millis(now.as_millis() + 60_000),
            )
            .await
            .unwrap();
        let setup = identity
            .complete_setup(
                SetupRequest {
                    token: "live-setup".to_owned(),
                    email: "owner@example.com".to_owned(),
                    display_name: "Owner".to_owned(),
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
        let server = Self {
            base,
            database,
            shutdown,
            task,
            http: reqwest::Client::new(),
            workspace: setup.workspace_id.to_string(),
            _root: root,
        };
        let owner = User {
            id: setup.user_id,
            token: setup.session.token.clone(),
        };
        (server, owner)
    }

    async fn stop(self) {
        self.shutdown.cancel();
        self.task.await.unwrap();
    }

    /// A user with a session; a member of the workspace if `member`.
    async fn user(&self, name: &str, member: bool) -> User {
        let id = Id::new_v7();
        let now = TimestampMillis::now();
        let email = format!("{name}@example.com");
        sqlx::query(
            "INSERT INTO users (id, email, normalized_email, display_name, password_hash, created_at, updated_at) \
             VALUES (?, ?, ?, ?, 'unused', ?, ?)",
        )
        .bind(id.to_string())
        .bind(&email)
        .bind(&email)
        .bind(name)
        .bind(now.as_millis())
        .bind(now.as_millis())
        .execute(self.database.pool())
        .await
        .unwrap();
        if member {
            sqlx::query(
                "INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, updated_at) \
                 VALUES (?, ?, ?, 'member', 0, ?, ?)",
            )
            .bind(Id::new_v7().to_string())
            .bind(&self.workspace)
            .bind(id.to_string())
            .bind(now.as_millis())
            .bind(now.as_millis())
            .execute(self.database.pool())
            .await
            .unwrap();
        }
        let session = IdentityRepository::new(self.database.clone())
            .create_session(
                &AuthenticatedUser {
                    id,
                    email,
                    display_name: name.to_owned(),
                },
                now,
            )
            .await
            .unwrap();
        User {
            id,
            token: session.token,
        }
    }

    /// A chat API call that must succeed.
    async fn chat(&self, user: &User, method: &str, path: &str, body: Option<Value>) -> Value {
        let mut request = self
            .http
            .request(
                method.parse().unwrap(),
                format!(
                    "{}/api/v1/workspaces/{}/chat{path}",
                    self.base, self.workspace
                ),
            )
            .header("origin", &self.base)
            .header("cookie", format!("{COOKIE}={}", user.token));
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.unwrap();
        let status = response.status();
        let value: Value = response.json().await.unwrap();
        assert!(status.is_success(), "{method} {path}: {value}");
        value
    }

    async fn general(&self, user: &User) -> String {
        let conversations = self.chat(user, "GET", "/conversations", None).await;
        conversations[0]["id"].as_str().unwrap().to_owned()
    }

    async fn send(&self, user: &User, conversation: &str, body: &str, nonce: &str) -> String {
        let written = self
            .chat(
                user,
                "POST",
                &format!("/conversations/{conversation}/messages"),
                Some(json!({ "body": body, "nonce": nonce })),
            )
            .await;
        written["result"]["id"].as_str().unwrap().to_owned()
    }

    /// Opens the live socket; `Err(status)` when the handshake is refused.
    async fn socket(&self, token: Option<&str>, query: &str) -> Result<Socket, u16> {
        let url = format!(
            "{}/api/v1/workspaces/{}/live{query}",
            self.base.replacen("http", "ws", 1),
            self.workspace
        );
        let mut request = url.into_client_request().unwrap();
        request
            .headers_mut()
            .insert("origin", HeaderValue::from_str(&self.base).unwrap());
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

    /// A connected client that has read its `hello` and the `resync` of a first connection.
    async fn client(&self, user: &User) -> (Socket, Value) {
        let mut socket = self.socket(Some(&user.token), "").await.unwrap();
        let hello = next(&mut socket).await.expect("hello");
        assert_eq!(hello["type"], "hello");
        assert_eq!(next(&mut socket).await.unwrap()["type"], "resync");
        (socket, hello)
    }
}

/// The next JSON frame, or `None` when nothing comes for a moment.
async fn next(socket: &mut Socket) -> Option<Value> {
    loop {
        let message = tokio::time::timeout(Duration::from_millis(400), socket.next())
            .await
            .ok()??
            .ok()?;
        if let Ws::Text(text) = message {
            let value: Value = serde_json::from_str(&text).unwrap();
            if value["type"] != "ping" {
                return Some(value);
            }
        }
    }
}

/// Every frame that is waiting, as `(topic, event type)`.
/// The (topic, type) of the frames that wait, without the `notify` signals: those are sent
/// after the write's response, so they have no fixed place among the write's events.
async fn drain(socket: &mut Socket) -> Vec<(String, String)> {
    let mut seen = Vec::new();
    while let Some(frame) = next(socket).await {
        if frame["topic"] == "notify" {
            continue;
        }
        seen.push((
            frame["topic"].as_str().unwrap_or_default().to_owned(),
            frame["event"]["type"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
        ));
    }
    seen
}

#[tokio::test]
async fn the_handshake_needs_a_session_and_membership() {
    let (server, owner) = Server::start().await;
    let outsider = server.user("outsider", false).await;
    // A refused handshake is an upgrade that closes at once: a browser can read the close
    // code, and it cannot read an HTTP status.
    for (token, code) in [
        (None, 4401),
        (Some("nothing"), 4401),
        (Some(outsider.token.as_str()), 4403),
    ] {
        let mut socket = server.socket(token, "").await.unwrap();
        let first = tokio::time::timeout(Duration::from_secs(2), socket.next())
            .await
            .unwrap();
        assert!(
            matches!(&first, Some(Ok(Ws::Close(Some(frame)))) if u16::from(frame.code) == code),
            "{first:?}"
        );
    }
    // A request without an Origin is still refused before the upgrade.
    let request = format!(
        "{}/api/v1/workspaces/{}/live",
        server.base.replacen("http", "ws", 1),
        server.workspace
    )
    .into_client_request()
    .unwrap();
    assert!(matches!(
        tokio_tungstenite::connect_async(request).await,
        Err(tungstenite::Error::Http(response)) if response.status() == 403
    ));

    let (mut socket, hello) = server.client(&owner).await;
    assert_eq!(hello["presence"][0]["user_id"], json!(owner.id));
    assert_eq!(hello["presence"][0]["status"], "online");
    assert_eq!(hello["seq"], 0);

    // A frame the server does not know closes the socket.
    socket
        .send(Ws::Text(json!({ "type": "shout" }).to_string().into()))
        .await
        .unwrap();
    let closed = tokio::time::timeout(Duration::from_secs(2), socket.next())
        .await
        .unwrap();
    assert!(
        matches!(&closed, Some(Ok(Ws::Close(Some(frame)))) if u16::from(frame.code) == 4400),
        "{closed:?}"
    );
    server.stop().await;
}

#[tokio::test]
async fn chat_events_go_to_the_members_who_may_see_them() {
    let (server, owner) = Server::start().await;
    let ada = server.user("ada", true).await;
    let bob = server.user("bob", true).await;
    let general = server.general(&owner).await;
    let (mut owner_socket, _) = server.client(&owner).await;
    let (mut ada_socket, hello) = server.client(&ada).await;
    let (mut bob_socket, _) = server.client(&bob).await;
    assert_eq!(hello["presence"].as_array().unwrap().len(), 2);
    // Each earlier client saw the later ones come online.
    assert_eq!(drain(&mut owner_socket).await.len(), 2);
    assert_eq!(
        drain(&mut ada_socket).await,
        [("presence".to_owned(), "presence".to_owned())]
    );

    // A message in a public channel: everyone gets it; the others also get their new unread
    // count, the sender gets its own state.
    let message = server
        .send(&owner, &general, &format!("<@{}> hello", ada.id), "n1")
        .await;
    let frame = next(&mut ada_socket).await.unwrap();
    assert_eq!(frame["seq"], 1);
    assert_eq!(frame["topic"], "chat");
    assert_eq!(frame["event"]["type"], "message.created");
    assert_eq!(frame["event"]["message"]["id"], message.as_str());
    // The mention made an inbox item: Ada's inbox is told to refresh. Bob's is not.
    let inbox = next(&mut ada_socket).await.unwrap();
    assert_eq!(inbox["topic"], "inbox");
    assert_eq!(inbox["event"]["type"], "changed");
    let state = next(&mut ada_socket).await.unwrap();
    assert_eq!(state["event"]["type"], "state.changed");
    assert_eq!(state["event"]["state"]["unread_count"], 1);
    assert_eq!(state["event"]["state"]["mention_count"], 1);
    // Ada is at this tab, so the mention notifies her here and not by push.
    let notice = next(&mut ada_socket).await.unwrap();
    assert_eq!(notice["topic"], "notify");
    assert_eq!(notice["event"]["url"], format!("/chat/{general}"));
    assert_eq!(notice["event"]["sound"], "mention");
    assert!(next(&mut ada_socket).await.is_none());
    let chat = |types: &[&str]| -> Vec<(String, String)> {
        types
            .iter()
            .map(|kind| ("chat".to_owned(), (*kind).to_owned()))
            .collect()
    };
    assert_eq!(
        drain(&mut bob_socket).await,
        chat(&["message.created", "state.changed"])
    );
    assert_eq!(
        drain(&mut owner_socket).await,
        chat(&["message.created", "state.changed"])
    );

    // A private channel of the owner and Ada: Bob gets nothing about it.
    let secret = server
        .chat(
            &owner,
            "POST",
            "/conversations",
            Some(json!({ "name": "secret", "kind": "private", "member_ids": [ada.id] })),
        )
        .await["result"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    server.send(&owner, &secret, "Only for us", "n2").await;
    assert_eq!(
        drain(&mut ada_socket).await,
        chat(&[
            "conversation.changed",
            "state.changed",
            "message.created",
            "state.changed"
        ])
    );
    assert!(drain(&mut bob_socket).await.is_empty());

    // A thread reply: the root's author follows the thread and gets its state.
    drain(&mut owner_socket).await;
    server
        .chat(
            &ada,
            "POST",
            &format!("/conversations/{general}/messages"),
            Some(json!({ "body": "A reply", "nonce": "n3", "thread_root_id": message })),
        )
        .await;
    assert_eq!(
        drain(&mut owner_socket).await,
        chat(&["message.created", "message.updated", "thread.changed"])
    );
    // Bob does not follow the thread: he sees the reply and the root's new summary only.
    assert_eq!(
        drain(&mut bob_socket).await,
        chat(&["message.created", "message.updated"])
    );

    // Typing goes to the others who can read the conversation, and only from a member of it.
    let typing = |conversation: &str| {
        Ws::Text(
            json!({ "type": "typing", "conversation_id": conversation, "thread_root_id": null })
                .to_string()
                .into(),
        )
    };
    drain(&mut ada_socket).await;
    ada_socket.send(typing(&secret)).await.unwrap();
    let frame = next(&mut owner_socket).await.unwrap();
    assert!(frame.get("seq").is_none(), "typing is not numbered");
    assert_eq!(
        frame["event"],
        json!({ "type": "typing", "conversation_id": secret, "thread_root_id": null, "user_id": ada.id })
    );
    assert!(drain(&mut bob_socket).await.is_empty());
    bob_socket.send(typing(&secret)).await.unwrap();
    assert!(drain(&mut owner_socket).await.is_empty());
    server.stop().await;
}

#[tokio::test]
async fn a_client_that_reconnects_gets_what_it_missed_or_a_resync() {
    let (server, owner) = Server::start().await;
    let ada = server.user("ada", true).await;
    let general = server.general(&owner).await;
    let (mut socket, hello) = server.client(&ada).await;
    let epoch = hello["epoch"].as_str().unwrap().to_owned();

    server.send(&owner, &general, "One", "n1").await;
    // Ada sees the message and her state; the owner's own state has a number she never sees.
    assert_eq!(next(&mut socket).await.unwrap()["seq"], 1);
    let last = next(&mut socket).await.unwrap()["seq"].as_u64().unwrap();
    assert_eq!(last, 3);
    drop(socket);
    server.send(&owner, &general, "Two", "n2").await;

    // Same epoch and a number the buffer still has: the missed events, no resync.
    let mut socket = server
        .socket(Some(&ada.token), &format!("?epoch={epoch}&after={last}"))
        .await
        .unwrap();
    let hello = next(&mut socket).await.unwrap();
    assert_eq!(hello["type"], "hello");
    let missed = next(&mut socket).await.unwrap();
    assert_eq!(missed["event"]["type"], "message.created");
    assert_eq!(missed["event"]["message"]["body"], "Two");
    assert!(
        drain(&mut socket)
            .await
            .iter()
            .all(|(_, kind)| kind != "resync")
    );

    // Another epoch (the server restarted): fetch everything again.
    let mut socket = server
        .socket(Some(&ada.token), "?epoch=older&after=3")
        .await
        .unwrap();
    assert_eq!(next(&mut socket).await.unwrap()["type"], "hello");
    assert_eq!(next(&mut socket).await.unwrap()["type"], "resync");
    server.stop().await;
}

#[tokio::test]
async fn a_new_or_deleted_emoji_tells_every_member_to_read_the_list_again() {
    let (server, owner) = Server::start().await;
    let ada = server.user("ada", true).await;
    let (mut owner_socket, _) = server.client(&owner).await;
    let (mut ada_socket, _) = server.client(&ada).await;
    drain(&mut owner_socket).await;

    let boundary = "orbit-test-boundary";
    let mut body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"emoji\"\r\n\r\n"
    )
    .into_bytes();
    body.extend_from_slice(b"GIF89a\x01\x00\x01\x00\x80\x00\x00\x00\x00\x00\xff\xff\xff\x3b");
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    let upload = body.clone();
    let response = server
        .http
        .post(format!(
            "{}/api/v1/workspaces/{}/chat/emoji?name=blink",
            server.base, server.workspace
        ))
        .header("origin", &server.base)
        .header("cookie", format!("{COOKIE}={}", owner.token))
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(body)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 201);
    let emoji: Value = response.json().await.unwrap();
    for socket in [&mut owner_socket, &mut ada_socket] {
        let frame = next(socket).await.unwrap();
        assert_eq!(frame["topic"], "chat");
        assert_eq!(frame["event"], json!({ "type": "emoji.changed" }));
    }

    let response = server
        .http
        .delete(format!(
            "{}/api/v1/workspaces/{}/chat/emoji/{}",
            server.base,
            server.workspace,
            emoji["id"].as_str().unwrap()
        ))
        .header("origin", &server.base)
        .header("cookie", format!("{COOKIE}={}", owner.token))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 204);
    for socket in [&mut owner_socket, &mut ada_socket] {
        assert_eq!(
            drain(socket).await,
            [("chat".to_owned(), "emoji.changed".to_owned())]
        );
    }

    // A sticker is told the same way.
    let response = server
        .http
        .post(format!(
            "{}/api/v1/workspaces/{}/chat/stickers?name=blink",
            server.base, server.workspace
        ))
        .header("origin", &server.base)
        .header("cookie", format!("{COOKIE}={}", owner.token))
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(upload)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 201);
    for socket in [&mut owner_socket, &mut ada_socket] {
        let frame = next(socket).await.unwrap();
        assert_eq!(frame["topic"], "chat");
        assert_eq!(frame["event"], json!({ "type": "stickers.changed" }));
    }
    server.stop().await;
}
