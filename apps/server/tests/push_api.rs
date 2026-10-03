use std::sync::{Arc, Mutex};

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use futures_util::future::BoxFuture;
use orbit_platform::{AuthenticatedUser, Id, PasswordService, TestDatabase, TimestampMillis};
use orbit_server::auth_routes::{AuthState, CookieMode, auth_router};
use orbit_server::live::LiveHub;
use orbit_server::push::{
    Notice, PushKind, PushSender, PushService, SendError, Subscription, Vapid,
};
use orbit_server::repositories::identity::{
    IdentityRepository, Presence, SetupRequest, UserStatus,
};
use orbit_server::task_routes::{TaskState, task_router};
use serde_json::{Value, json};
use tower::ServiceExt;

/// Records what it is asked to send. An endpoint with "gone" in it answers as an ended one,
/// one with "refused" is refused every time, and one with "busy" is busy the first time.
#[derive(Default)]
struct FakeSender {
    sent: Mutex<Vec<(String, Value)>>,
    /// Every endpoint a send was tried for, with the ones that failed.
    tried: Mutex<Vec<String>>,
}

impl PushSender for FakeSender {
    fn send(
        &self,
        _vapid: Vapid,
        subscription: Subscription,
        payload: Vec<u8>,
    ) -> BoxFuture<'static, Result<(), SendError>> {
        let endpoint = subscription.endpoint;
        let earlier = {
            let mut tried = self.tried.lock().unwrap();
            tried.push(endpoint.clone());
            tried.iter().filter(|tried| **tried == endpoint).count() - 1
        };
        let result = if endpoint.contains("gone") {
            Err(SendError::Gone)
        } else if endpoint.contains("refused") {
            Err(SendError::Failed("refused".to_owned()))
        } else if endpoint.contains("busy") && earlier == 0 {
            Err(SendError::Busy("busy".to_owned()))
        } else {
            let payload = serde_json::from_slice(&payload).unwrap();
            self.sent.lock().unwrap().push((endpoint, payload));
            Ok(())
        };
        Box::pin(async move { result })
    }
}

struct Fixture {
    database: TestDatabase,
    identity: Arc<IdentityRepository>,
    sender: Arc<FakeSender>,
    push: PushService,
    app: axum::Router,
    workspace_id: Id,
    owner_id: Id,
    owner_cookie: String,
}

const KEY: &str =
    "BCVxsr7N_eNgVRqvHtD0zTZsEc6-VV-JvLexhqUzORcxaOzi6-AYWXvTBHm4bjyPjs7Vd8pZGH6SRpkNtoIAiw4";
const AUTH: &str = "BTBZMqHH6r4Tts7J_aSIgg";

impl Fixture {
    async fn new() -> Self {
        let database = TestDatabase::new().await.unwrap();
        let sender = Arc::new(FakeSender::default());
        let push = PushService::with_sender(&database, sender.clone());
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
        let owner_id: String = sqlx::query_scalar("SELECT id FROM users")
            .fetch_one(database.pool())
            .await
            .unwrap();
        let app = auth_router(AuthState::new(Arc::clone(&identity), CookieMode::secure())).merge(
            task_router(TaskState::new(Arc::clone(&identity), CookieMode::secure())),
        );
        Self {
            database,
            identity,
            sender,
            push,
            app,
            workspace_id: setup.workspace_id,
            owner_id: owner_id.parse().unwrap(),
            owner_cookie: format!("__Host-orbit_session={}", setup.session.token),
        }
    }

    async fn member(&self, name: &str) -> (Id, String) {
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
        sqlx::query(
            "INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, updated_at) \
             VALUES (?, ?, ?, 'member', 0, ?, ?)",
        )
        .bind(Id::new_v7().to_string())
        .bind(self.workspace_id.to_string())
        .bind(id.to_string())
        .bind(now.as_millis())
        .bind(now.as_millis())
        .execute(self.database.pool())
        .await
        .unwrap();
        let user = AuthenticatedUser {
            id,
            email,
            display_name: name.to_owned(),
        };
        let session = self.identity.create_session(&user, now).await.unwrap();
        (id, format!("__Host-orbit_session={}", session.token))
    }

    async fn send(
        &self,
        method: &str,
        uri: &str,
        cookie: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let request = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::COOKIE, cookie)
            .header(header::CONTENT_TYPE, "application/json")
            .body(body.map_or_else(Body::empty, |body| Body::from(body.to_string())))
            .unwrap();
        let response = self.app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn subscribe(&self, cookie: &str, endpoint: &str) -> (StatusCode, Value) {
        let body = json!({ "endpoint": endpoint, "p256dh": KEY, "auth": AUTH, "label": "Firefox on Linux" });
        self.send("POST", "/api/v1/push/subscriptions", cookie, Some(body))
            .await
    }

    fn sent(&self) -> Vec<(String, Value)> {
        std::mem::take(&mut self.sender.sent.lock().unwrap())
    }
}

fn notice() -> Notice {
    Notice {
        title: "Ada".to_owned(),
        body: "Hello".to_owned(),
        url: "/chat/1".to_owned(),
        tag: "chat:1".to_owned(),
        sound: "mention",
        icon: Some("/api/v1/users/1/avatar?v=2".to_owned()),
    }
}

#[tokio::test]
async fn subscriptions_are_checked_listed_per_user_and_removed_when_the_browser_ended_them() {
    let fixture = Fixture::new().await;
    let (_, ada_cookie) = fixture.member("ada").await;
    let owner = &fixture.owner_cookie;

    let (status, key) = fixture.send("GET", "/api/v1/push/key", owner, None).await;
    assert_eq!(status, StatusCode::OK);
    let public_key = key["public_key"].as_str().unwrap().to_owned();
    assert_eq!(
        public_key.len(),
        87,
        "an uncompressed P-256 point, base64url"
    );
    // The key is made once.
    let (_, again) = fixture
        .send("GET", "/api/v1/push/key", &ada_cookie, None)
        .await;
    assert_eq!(again["public_key"], public_key.as_str());

    // The server sends to the endpoint, so it must be a browser vendor's push service.
    for endpoint in [
        "https://attacker.example/hook",
        "http://127.0.0.1:8080/api/v1/admin",
    ] {
        let (status, problem) = fixture.subscribe(owner, endpoint).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(problem["code"], "invalid_subscription");
    }
    let good = "https://updates.push.services.mozilla.com/wpush/v2/good";
    let gone = "https://updates.push.services.mozilla.com/wpush/v2/gone";
    let (status, record) = fixture.subscribe(owner, good).await;
    assert_eq!(status, StatusCode::OK);
    // The same browser again: still one row.
    fixture.subscribe(owner, good).await;
    fixture.subscribe(owner, gone).await;
    let (_, list) = fixture
        .send("GET", "/api/v1/push/subscriptions", owner, None)
        .await;
    assert_eq!(list.as_array().unwrap().len(), 2);
    let (_, list) = fixture
        .send("GET", "/api/v1/push/subscriptions", &ada_cookie, None)
        .await;
    assert!(list.as_array().unwrap().is_empty());

    // A test push reaches the good one; the ended one is removed.
    let (_, result) = fixture.send("POST", "/api/v1/push/test", owner, None).await;
    assert_eq!(result["sent"], 1);
    assert_eq!(fixture.sent()[0].0, good);
    let (_, list) = fixture
        .send("GET", "/api/v1/push/subscriptions", owner, None)
        .await;
    assert_eq!(list.as_array().unwrap().len(), 1);

    // Somebody else cannot remove it; its owner can.
    let uri = format!(
        "/api/v1/push/subscriptions/{}",
        record["id"].as_str().unwrap()
    );
    fixture.send("DELETE", &uri, &ada_cookie, None).await;
    let (_, list) = fixture
        .send("GET", "/api/v1/push/subscriptions", owner, None)
        .await;
    assert_eq!(list.as_array().unwrap().len(), 1);
    let (status, _) = fixture.send("DELETE", &uri, owner, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, result) = fixture.send("POST", "/api/v1/push/test", owner, None).await;
    assert_eq!(result["sent"], 0);
}

#[tokio::test]
async fn a_notice_goes_to_the_active_tab_or_else_to_every_browser_and_respects_the_user() {
    let fixture = Fixture::new().await;
    let owner = fixture.owner_id;
    let endpoint = "https://fcm.googleapis.com/fcm/send/one";
    fixture.subscribe(&fixture.owner_cookie, endpoint).await;
    let push = &fixture.push;

    // Nobody at a device: a push.
    push.notify(owner, PushKind::DirectMessages, &notice())
        .await
        .unwrap();
    let sent = fixture.sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(
        sent[0].1,
        json!({ "title": "Ada", "body": "Hello", "url": "/chat/1", "tag": "chat:1", "sound": "mention",
            "icon": "/api/v1/users/1/avatar?v=2" })
    );

    // An active tab: it gets the signal and no browser gets a push.
    let hub = LiveHub::of(&fixture.database);
    let mut tab = hub.connect(
        fixture.workspace_id,
        owner,
        UserStatus::default(),
        None,
        None,
    );
    push.notify(owner, PushKind::DirectMessages, &notice())
        .await
        .unwrap();
    assert!(fixture.sent().is_empty());
    let frame: Value = serde_json::from_str(&tab.frames.try_recv().unwrap()).unwrap();
    assert_eq!(frame["topic"], "notify");
    assert_eq!(frame["event"]["url"], "/chat/1");
    // The window went out of focus for a while: the user is not looking, so the push goes out
    // again. The open tab still gets the signal, for its sound.
    let note = notice();
    let direct = || push.notify(owner, PushKind::DirectMessages, &note);
    hub.set_away(fixture.workspace_id, tab.id, true);
    direct().await.unwrap();
    assert_eq!(fixture.sent().len(), 1);
    assert!(tab.frames.try_recv().is_ok());
    hub.set_away(fixture.workspace_id, tab.id, false);
    direct().await.unwrap();
    assert!(fixture.sent().is_empty());
    assert!(tab.frames.try_recv().is_ok());
    // The same for a tab without input for a long time.
    hub.set_idle(fixture.workspace_id, tab.id, owner, true);
    direct().await.unwrap();
    assert_eq!(fixture.sent().len(), 1);
    hub.disconnect(fixture.workspace_id, tab.id, owner);

    // A kind the user turned off, and "do not disturb".
    let prefs = json!({ "direct_messages": false, "chat_mentions": true, "thread_replies": true,
        "channel_messages": true, "task_assigned": true, "mentions": true });
    let (status, _) = fixture
        .send(
            "PUT",
            "/api/v1/notification-preferences",
            &fixture.owner_cookie,
            Some(prefs.clone()),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (_, stored) = fixture
        .send(
            "GET",
            "/api/v1/notification-preferences",
            &fixture.owner_cookie,
            None,
        )
        .await;
    assert_eq!(stored, prefs);
    push.notify(owner, PushKind::DirectMessages, &notice())
        .await
        .unwrap();
    assert!(fixture.sent().is_empty());
    push.notify(owner, PushKind::ChatMentions, &notice())
        .await
        .unwrap();
    assert_eq!(fixture.sent().len(), 1);
    let dnd = UserStatus {
        presence: Presence::Dnd,
        ..UserStatus::default()
    };
    fixture
        .identity
        .set_status(owner, &dnd, TimestampMillis::now())
        .await
        .unwrap();
    push.notify(owner, PushKind::ChatMentions, &notice())
        .await
        .unwrap();
    assert!(fixture.sent().is_empty());
    fixture
        .identity
        .set_status(owner, &UserStatus::default(), TimestampMillis::now())
        .await
        .unwrap();

    // The session that subscribed ended: no push to that browser.
    sqlx::query("UPDATE sessions SET revoked_at = 1")
        .execute(fixture.database.pool())
        .await
        .unwrap();
    push.notify(owner, PushKind::ChatMentions, &notice())
        .await
        .unwrap();
    assert!(fixture.sent().is_empty());
}

#[tokio::test]
async fn a_user_keeps_the_newest_twenty_subscriptions() {
    let fixture = Fixture::new().await;
    let (ada, ada_cookie) = fixture.member("ada").await;
    fixture
        .subscribe(&ada_cookie, "https://web.push.apple.com/ada")
        .await;
    let session: String = sqlx::query_scalar("SELECT id FROM sessions WHERE user_id = ?")
        .bind(fixture.owner_id.to_string())
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    let session: Id = session.parse().unwrap();
    let endpoint = |n: i64| format!("https://fcm.googleapis.com/fcm/send/{n}");
    let now = TimestampMillis::now().as_millis();
    let subscribe = |n: i64, at: i64| {
        let (push, endpoint) = (fixture.push.clone(), endpoint(n));
        async move {
            let at = TimestampMillis::from_millis(now + at);
            push.subscribe(
                fixture.owner_id,
                session,
                &endpoint,
                KEY,
                AUTH,
                "Browser",
                at,
            )
            .await
            .unwrap();
        }
    };
    let endpoints = || async {
        let list = fixture.push.subscriptions(fixture.owner_id).await.unwrap();
        let mut numbers: Vec<i64> = list
            .iter()
            .map(|record| record.endpoint.rsplit('/').next().unwrap().parse().unwrap())
            .collect();
        numbers.sort_unstable();
        numbers
    };

    for n in 0..20 {
        subscribe(n, n).await;
    }
    assert_eq!(endpoints().await, (0..20).collect::<Vec<_>>());
    // One more: the oldest goes.
    subscribe(20, 20).await;
    assert_eq!(endpoints().await, (1..=20).collect::<Vec<_>>());
    // The oldest browser subscribes again: it keeps its row, and nothing goes.
    subscribe(1, 30).await;
    assert_eq!(endpoints().await, (1..=20).collect::<Vec<_>>());
    // Somebody else's subscriptions do not count and do not go.
    assert_eq!(fixture.push.subscriptions(ada).await.unwrap().len(), 1);
}

#[tokio::test]
async fn a_long_title_and_body_are_cut_before_they_are_sent() {
    let fixture = Fixture::new().await;
    let owner = fixture.owner_id;
    fixture
        .subscribe(
            &fixture.owner_cookie,
            "https://fcm.googleapis.com/fcm/send/one",
        )
        .await;
    let long = Notice {
        title: "T".repeat(2000),
        body: "é".repeat(2000),
        ..notice()
    };

    fixture
        .push
        .notify(owner, PushKind::Mentions, &long)
        .await
        .unwrap();
    fixture.push.push(owner, &long).await.unwrap();
    let sent = fixture.sent();
    assert_eq!(sent.len(), 2);
    for (_, payload) in &sent {
        let title = payload["title"].as_str().unwrap();
        let body = payload["body"].as_str().unwrap();
        assert_eq!(title.chars().count(), 120);
        assert_eq!(body.chars().count(), 180);
        assert!(title.ends_with('…') && body.ends_with('…'));
    }

    // An open tab gets the same short notice.
    let hub = LiveHub::of(&fixture.database);
    let mut tab = hub.connect(
        fixture.workspace_id,
        owner,
        UserStatus::default(),
        None,
        None,
    );
    fixture
        .push
        .notify(owner, PushKind::Mentions, &long)
        .await
        .unwrap();
    assert!(fixture.sent().is_empty());
    let frame: Value = serde_json::from_str(&tab.frames.try_recv().unwrap()).unwrap();
    assert_eq!(
        frame["event"]["body"].as_str().unwrap().chars().count(),
        180
    );
}

#[tokio::test]
async fn inbox_notifications_are_pushed_once() {
    let fixture = Fixture::new().await;
    let (ada, ada_cookie) = fixture.member("ada").await;
    fixture
        .subscribe(&ada_cookie, "https://web.push.apple.com/ada")
        .await;
    let project: String = sqlx::query_scalar("SELECT id FROM projects LIMIT 1")
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    let status: String = sqlx::query_scalar(
        "SELECT id FROM task_statuses WHERE project_id = ? ORDER BY position LIMIT 1",
    )
    .bind(&project)
    .fetch_one(fixture.database.pool())
    .await
    .unwrap();
    // A real assignment, so the notification is the one the task writer makes.
    let (created, _) = fixture
        .send(
            "POST",
            &format!("/api/v1/workspaces/{}/tasks", fixture.workspace_id),
            &fixture.owner_cookie,
            Some(json!({
                "project_id": project,
                "status_id": status,
                "title": "Ship the parser",
                "assignee_ids": [ada.to_string()]
            })),
        )
        .await;
    assert_eq!(created, StatusCode::CREATED);

    assert_eq!(fixture.push.push_inbox().await.unwrap(), 1);
    let sent = fixture.sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].1["title"], "Owner assigned you a task");
    assert_eq!(sent[0].1["body"], "Ship the parser");
    assert_eq!(sent[0].1["url"], "/inbox");
    // The owner has no picture: the app's icon shows.
    assert!(sent[0].1["icon"].is_null());
    assert_eq!(fixture.push.push_inbox().await.unwrap(), 0);
    assert!(fixture.sent().is_empty());
}

#[tokio::test]
async fn a_busy_push_service_gets_the_push_again_and_a_subscription_that_keeps_failing_goes() {
    let fixture = Fixture::new().await;
    let owner = fixture.owner_id;
    let busy = "https://fcm.googleapis.com/fcm/send/busy";
    let refused = "https://fcm.googleapis.com/fcm/send/refused";
    fixture.subscribe(&fixture.owner_cookie, busy).await;
    fixture.subscribe(&fixture.owner_cookie, refused).await;
    let failures = |endpoint: &'static str| {
        let pool = fixture.database.pool().clone();
        async move {
            sqlx::query_scalar::<_, i64>(
                "SELECT failures FROM push_subscriptions WHERE endpoint = ?",
            )
            .bind(endpoint)
            .fetch_optional(&pool)
            .await
            .unwrap()
        }
    };

    // The busy one takes the push at the second try and does not count as failed.
    assert_eq!(fixture.push.push(owner, &notice()).await.unwrap(), 1);
    assert_eq!(fixture.sent()[0].0, busy);
    let tries = |endpoint: &str| {
        let tried = fixture.sender.tried.lock().unwrap();
        tried.iter().filter(|tried| *tried == endpoint).count()
    };
    assert_eq!(tries(busy), 2);
    assert_eq!(tries(refused), 1, "a refused push is not sent again");
    assert_eq!(failures(busy).await, Some(0));
    assert_eq!(failures(refused).await, Some(1));

    // The refused one is removed at its fifth failure in a row.
    for _ in 0..3 {
        fixture.push.push(owner, &notice()).await.unwrap();
    }
    assert_eq!(failures(refused).await, Some(4));
    fixture.push.push(owner, &notice()).await.unwrap();
    assert_eq!(failures(refused).await, None);
    assert_eq!(failures(busy).await, Some(0));

    // A push that goes through sets the count back.
    sqlx::query("UPDATE push_subscriptions SET failures = 3")
        .execute(fixture.database.pool())
        .await
        .unwrap();
    fixture.push.push(owner, &notice()).await.unwrap();
    assert_eq!(failures(busy).await, Some(0));
}
