//! Two-way chat integrations on a channel webhook: message events sent to the request URL, replies
//! in a thread with `thread_ts`, and reads of the channel with the webhook's secret.

use std::sync::Arc;
use std::time::Duration;

use axum::body::{Body, Bytes, to_bytes};
use axum::http::{HeaderMap, Request, StatusCode, header};
use hmac::{Hmac, Mac};
use orbit_platform::{
    AttachmentMutationCoordinator, AuthenticatedUser, Id, LocalBlobStore, PasswordService,
    TestDatabase, TimestampMillis, UploadLimits, UploadService,
};
use orbit_server::auth_routes::CookieMode;
use orbit_server::chat_routes::{ChatState, chat_router};
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use serde_json::{Value, json};
use sha2::Sha256;
use tokio::sync::mpsc;
use tower::ServiceExt;

struct Fixture {
    _root: tempfile::TempDir,
    _database: TestDatabase,
    app: axum::Router,
    workspace_id: String,
    owner: Member,
    member: Member,
}

#[derive(Clone)]
struct Member {
    id: Id,
    cookie: String,
}

/// What the integration's receiver got: the headers and the raw body of each request.
type Received = mpsc::UnboundedReceiver<(HeaderMap, Bytes)>;

impl Fixture {
    async fn new() -> Self {
        let database = TestDatabase::new().await.unwrap();
        let identity = Arc::new(IdentityRepository::new((*database).clone()));
        let now = TimestampMillis::now();
        identity
            .store_setup_token(
                "operator-secret",
                TimestampMillis::from_millis(now.as_millis() + 60_000),
            )
            .await
            .unwrap();
        let setup = identity
            .complete_setup(
                SetupRequest {
                    token: "operator-secret".to_owned(),
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
        let workspace_id = setup.workspace_id.to_string();

        let member_id = Id::new_v7();
        sqlx::query(
            "INSERT INTO users (id, email, normalized_email, display_name, password_hash, created_at, updated_at) \
             VALUES ($1, 'ana@example.com', 'ana@example.com', 'Ana', 'unused', $2, $3)",
        )
        .bind(member_id.to_string())
        .bind(now.as_millis())
        .bind(now.as_millis())
        .execute(database.pool())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, updated_at) \
             VALUES ($1, $2, $3, 'member', 0, $4, $5)",
        )
        .bind(Id::new_v7().to_string())
        .bind(&workspace_id)
        .bind(member_id.to_string())
        .bind(now.as_millis())
        .bind(now.as_millis())
        .execute(database.pool())
        .await
        .unwrap();
        let session = identity
            .create_session(
                &AuthenticatedUser {
                    id: member_id,
                    email: "ana@example.com".to_owned(),
                    display_name: "Ana".to_owned(),
                },
                now,
            )
            .await
            .unwrap();

        let root = tempfile::tempdir().unwrap();
        let uploads = UploadService::new(
            (*database).clone(),
            Arc::new(LocalBlobStore::new(root.path().join("attachments"))),
            AttachmentMutationCoordinator::default(),
            UploadLimits::default(),
        );
        let app = chat_router(
            ChatState::new(Arc::clone(&identity), uploads, CookieMode::secure())
                .with_interactivity(Some([7; 32]), true),
        );
        Self {
            _root: root,
            _database: database,
            app,
            workspace_id,
            owner: Member {
                id: setup.user_id,
                cookie: format!("__Host-orbit_session={}", setup.session.token),
            },
            member: Member {
                id: member_id,
                cookie: format!("__Host-orbit_session={}", session.token),
            },
        }
    }

    async fn request(&self, request: Request<Body>) -> (StatusCode, Value) {
        let response = self.app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    /// A session call under `/api/v1/workspaces/{id}/chat`.
    async fn call(
        &self,
        member: &Member,
        method: &str,
        path: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let builder = Request::builder()
            .method(method)
            .uri(format!(
                "/api/v1/workspaces/{}/chat{path}",
                self.workspace_id
            ))
            .header(header::COOKIE, &member.cookie);
        let request = match body {
            Some(value) => builder
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(value.to_string())),
            None => builder.body(Body::empty()),
        }
        .unwrap();
        self.request(request).await
    }

    async fn ok(&self, member: &Member, method: &str, path: &str, body: Option<Value>) -> Value {
        let (status, value) = self.call(member, method, path, body).await;
        assert_eq!(status, StatusCode::OK, "{method} {path}: {value}");
        value
    }

    async fn general(&self) -> String {
        let conversations = self.ok(&self.owner, "GET", "/conversations", None).await;
        conversations
            .as_array()
            .unwrap()
            .iter()
            .find(|conversation| conversation["is_default"] == true)
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    async fn send(&self, member: &Member, conversation: &str, body: Value) -> String {
        let mut body = body;
        body["nonce"] = json!(Id::new_v7().to_string());
        self.ok(
            member,
            "POST",
            &format!("/conversations/{conversation}/messages"),
            Some(body),
        )
        .await["result"]["id"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    /// A webhook named `name` in the channel: `(webhook id, URL path with the secret, secret)`.
    async fn webhook(&self, conversation: &str, name: &str) -> (String, String, String) {
        let (status, created) = self
            .call(
                &self.owner,
                "POST",
                &format!("/conversations/{conversation}/webhooks"),
                Some(json!({ "name": name })),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{created}");
        let url = created["url"].as_str().unwrap().to_owned();
        let secret = url.rsplit('/').next().unwrap().to_owned();
        (
            created["webhook"]["id"].as_str().unwrap().to_owned(),
            url,
            secret,
        )
    }

    async fn set_events(
        &self,
        conversation: &str,
        webhook: &str,
        request_url: Option<&str>,
        events: bool,
    ) -> Value {
        let (status, body) = self
            .call(
                &self.owner,
                "PUT",
                &format!("/conversations/{conversation}/webhooks/{webhook}/interactivity"),
                Some(json!({ "request_url": request_url, "events": events })),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body
    }

    async fn post_hook(&self, url: &str, payload: Value) -> StatusCode {
        let request = Request::builder()
            .method("POST")
            .uri(url)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(payload.to_string()))
            .unwrap();
        self.app.clone().oneshot(request).await.unwrap().status()
    }

    async fn read_hook(&self, webhook: &str, secret: &str, query: &str) -> (StatusCode, Value) {
        let request = Request::builder()
            .uri(format!("/api/v1/hooks/chat/{webhook}/messages{query}"))
            .header(header::AUTHORIZATION, format!("Bearer {secret}"))
            .body(Body::empty())
            .unwrap();
        self.request(request).await
    }
}

/// A local receiver that answers 200 and passes on each request.
async fn receiver() -> (String, Received) {
    let (sender, received) = mpsc::unbounded_channel();
    let app = axum::Router::new().route(
        "/events",
        axum::routing::post(move |headers: HeaderMap, body: Bytes| {
            let sender = sender.clone();
            async move {
                sender.send((headers, body)).unwrap();
                StatusCode::OK
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{address}/events"), received)
}

async fn next_event(received: &mut Received, signing_secret: &str) -> Value {
    let (headers, body) = tokio::time::timeout(Duration::from_secs(5), received.recv())
        .await
        .expect("an event within 5 seconds")
        .unwrap();
    assert_eq!(headers[header::CONTENT_TYPE], "application/json");
    let timestamp = headers["x-slack-request-timestamp"].to_str().unwrap();
    let mut mac = Hmac::<Sha256>::new_from_slice(signing_secret.as_bytes()).unwrap();
    mac.update(format!("v0:{timestamp}:").as_bytes());
    mac.update(&body);
    let expected: String = mac
        .finalize()
        .into_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    assert_eq!(
        headers["x-slack-signature"].to_str().unwrap(),
        format!("v0={expected}")
    );
    serde_json::from_slice(&body).unwrap()
}

#[tokio::test]
async fn an_app_gets_mentions_and_answers_in_the_thread() {
    let fixture = Fixture::new().await;
    let general = fixture.general().await;
    let (webhook, url, secret) = fixture.webhook(&general, "Assistant").await;
    let (request_url, mut received) = receiver().await;
    let record = fixture
        .set_events(&general, &webhook, Some(&request_url), true)
        .await;
    assert_eq!(record["events"], true);
    let signing_secret = fixture
        .ok(
            &fixture.owner,
            "GET",
            &format!("/conversations/{general}/webhooks/{webhook}/signing-secret"),
            None,
        )
        .await["signing_secret"]
        .as_str()
        .unwrap()
        .to_owned();

    let root = fixture
        .send(
            &fixture.member,
            &general,
            json!({ "body": "@assistant sum up the launch" }),
        )
        .await;
    let event = next_event(&mut received, &signing_secret).await;
    assert_eq!(event["type"], "event_callback");
    assert_eq!(event["api_app_id"], webhook);
    assert_eq!(event["team_id"], fixture.workspace_id);
    assert_eq!(event["event"]["type"], "app_mention");
    assert_eq!(event["event"]["channel"], general);
    assert_eq!(event["event"]["user"], fixture.member.id.to_string());
    assert_eq!(event["event"]["user_profile"]["name"], "Ana");
    assert_eq!(event["event"]["ts"], root);
    assert!(event["event"].get("thread_ts").is_none());

    let reply = fixture
        .send(
            &fixture.member,
            &general,
            json!({ "body": "also the budget", "thread_root_id": root }),
        )
        .await;
    let event = next_event(&mut received, &signing_secret).await;
    assert_eq!(event["event"]["type"], "message");
    assert_eq!(event["event"]["ts"], reply);
    assert_eq!(event["event"]["thread_ts"], root);

    // The integration reads the thread, then answers in it.
    let (status, thread) = fixture
        .read_hook(&webhook, &secret, &format!("?thread_ts={root}"))
        .await;
    assert_eq!(status, StatusCode::OK, "{thread}");
    assert_eq!(thread["messages"].as_array().unwrap().len(), 1);
    assert_eq!(thread["messages"][0]["text"], "also the budget");
    assert_eq!(thread["messages"][0]["username"], "Ana");

    let status = fixture
        .post_hook(
            &url,
            json!({ "text": "Here is the summary", "thread_ts": root }),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (_, thread) = fixture
        .read_hook(&webhook, &secret, &format!("?thread_ts={root}"))
        .await;
    let replies = thread["messages"].as_array().unwrap();
    assert_eq!(replies.len(), 2);
    assert_eq!(replies[1]["text"], "Here is the summary");
    assert_eq!(replies[1]["bot_id"], webhook);
    assert_eq!(replies[1]["username"], "Assistant");
    assert_eq!(replies[1]["thread_ts"], root);

    // The channel shows the root with two replies, and the bot's reply is not a channel message.
    let (_, channel) = fixture.read_hook(&webhook, &secret, "").await;
    let root_row = channel["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|message| message["ts"] == root)
        .unwrap();
    assert_eq!(root_row["reply_count"], 2);
    assert_eq!(root_row["thread_ts"], root);
    assert!(
        channel["messages"]
            .as_array()
            .unwrap()
            .iter()
            .all(|message| message["text"] != "Here is the summary")
    );

    // The thread summary under the root names the bot, not the member who set it up.
    let page = fixture
        .ok(
            &fixture.member,
            "GET",
            &format!("/conversations/{general}/messages"),
            None,
        )
        .await;
    let summary = page["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|message| message["id"] == root)
        .unwrap();
    assert_eq!(summary["last_reply"]["bot_name"], "Assistant");
    assert_eq!(
        summary["reply_user_ids"],
        json!([fixture.member.id.to_string()])
    );

    // The member who set the webhook up does not follow the thread the bot answered in; the
    // root's author does.
    let followed = fixture.ok(&fixture.owner, "GET", "/threads", None).await;
    assert!(followed.as_array().unwrap().is_empty(), "{followed}");
    let followed = fixture.ok(&fixture.member, "GET", "/threads", None).await;
    assert_eq!(followed.as_array().unwrap().len(), 1, "{followed}");

    // The integration's own message sends no event.
    assert!(
        tokio::time::timeout(Duration::from_millis(300), received.recv())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn only_a_webhook_with_events_reads_its_channel() {
    let fixture = Fixture::new().await;
    let general = fixture.general().await;
    let (webhook, url, secret) = fixture.webhook(&general, "CI").await;
    let root = fixture
        .send(&fixture.member, &general, json!({ "body": "hello" }))
        .await;

    // A post-only webhook: its URL may sit in a CI config, so it reads nothing.
    let (status, body) = fixture.read_hook(&webhook, &secret, "").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body["code"], "webhook_not_found");

    // Buttons alone do not let it read either.
    let (request_url, _received) = receiver().await;
    let record = fixture
        .set_events(&general, &webhook, Some(&request_url), false)
        .await;
    assert_eq!(record["events"], false);
    let (status, _) = fixture.read_hook(&webhook, &secret, "").await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    fixture
        .set_events(&general, &webhook, Some(&request_url), true)
        .await;
    let (status, channel) = fixture.read_hook(&webhook, &secret, "").await;
    assert_eq!(status, StatusCode::OK, "{channel}");
    assert!(
        channel["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["ts"] == root)
    );
    let (status, _) = fixture.read_hook(&webhook, "wrong-secret", "").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = fixture
        .read_hook(&webhook, &secret, &format!("?thread_ts={}", Id::new_v7()))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    // A reply to a message that is no root of this channel is refused.
    let status = fixture
        .post_hook(
            &url,
            json!({ "text": "hi", "thread_ts": Id::new_v7().to_string() }),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let status = fixture
        .post_hook(
            &url,
            json!({ "text": "hi", "thread_ts": "1700000000.000100" }),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    // Turning the request URL off turns events off, and the secret reads nothing again.
    let record = fixture.set_events(&general, &webhook, None, true).await;
    assert_eq!(record["events"], false);
    let (status, _) = fixture.read_hook(&webhook, &secret, "").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
