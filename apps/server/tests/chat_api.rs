use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use orbit_domain::WorkspaceRole;
use orbit_platform::{
    AttachmentMutationCoordinator, AuthenticatedUser, Id, LocalBlobStore, PasswordService,
    TestDatabase, TimestampMillis, UploadLimits, UploadService,
};
use orbit_server::auth_routes::CookieMode;
use orbit_server::chat_routes::{ChatState, chat_router};
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::repositories::workspaces::{InvitationDelivery, WorkspaceRepository};
use serde_json::{Value, json};
use tower::ServiceExt;

struct Fixture {
    _root: tempfile::TempDir,
    database: TestDatabase,
    identity: Arc<IdentityRepository>,
    app: axum::Router,
    workspace_id: String,
    owner: Member,
    nonce: AtomicU64,
}

/// (conversation, user, unread stored and counted, mentions stored and counted, broadcasts
/// stored and counted)
type CounterRow = (String, String, i64, i64, i64, i64, i64, i64);

#[derive(Clone)]
struct Member {
    id: Id,
    cookie: String,
}

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
        let root = tempfile::tempdir().unwrap();
        let uploads = UploadService::new(
            (*database).clone(),
            Arc::new(LocalBlobStore::new(root.path().join("attachments"))),
            AttachmentMutationCoordinator::default(),
            UploadLimits::default(),
        );
        let app = chat_router(ChatState::new(
            Arc::clone(&identity),
            uploads,
            CookieMode::secure(),
        ));
        Self {
            _root: root,
            database,
            identity,
            app,
            workspace_id: setup.workspace_id.to_string(),
            owner: Member {
                id: setup.user_id,
                cookie: format!("__Host-orbit_session={}", setup.session.token),
            },
            nonce: AtomicU64::new(0),
        }
    }

    /// A user without a membership yet.
    async fn add_user(&self, name: &str) -> (Id, String) {
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
        (id, email)
    }

    async fn session(&self, id: Id, email: String, name: &str) -> Member {
        let session = self
            .identity
            .create_session(
                &AuthenticatedUser {
                    id,
                    email,
                    display_name: name.to_owned(),
                },
                TimestampMillis::now(),
            )
            .await
            .unwrap();
        Member {
            id,
            cookie: format!("__Host-orbit_session={}", session.token),
        }
    }

    /// A member whose membership row is inserted directly, the way members from before public
    /// channels had everyone look: in the default channel only.
    async fn add_member(&self, name: &str) -> Member {
        let (id, email) = self.add_user(name).await;
        let now = TimestampMillis::now();
        sqlx::query(
            "INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, updated_at) \
             VALUES (?, ?, ?, 'member', 0, ?, ?)",
        )
        .bind(Id::new_v7().to_string())
        .bind(&self.workspace_id)
        .bind(id.to_string())
        .bind(now.as_millis())
        .bind(now.as_millis())
        .execute(self.database.pool())
        .await
        .unwrap();
        self.session(id, email, name).await
    }

    /// A member who joins by accepting an invitation, as in the app.
    async fn invite_member(&self, name: &str) -> Member {
        let (id, email) = self.add_user(name).await;
        let now = TimestampMillis::now();
        let workspaces = WorkspaceRepository::new((*self.database).clone());
        let invitation = workspaces
            .invite(
                self.workspace_id.parse().unwrap(),
                self.owner.id,
                email.clone(),
                WorkspaceRole::Member,
                InvitationDelivery::Manual,
                "invite",
                now,
            )
            .await
            .unwrap();
        workspaces
            .accept_invitation(&invitation.token, id, &email, "accept", now)
            .await
            .unwrap();
        self.session(id, email, name).await
    }

    /// The member ids of a conversation, as `member` lists it.
    async fn member_ids(&self, member: &Member, conversation: &str) -> Vec<String> {
        let listed = self.ok(member, "GET", "/conversations", None).await;
        let mut ids: Vec<String> = listed
            .as_array()
            .unwrap()
            .iter()
            .find(|listed| listed["id"] == conversation)
            .unwrap_or_else(|| panic!("{conversation} is not listed"))["member_ids"]
            .as_array()
            .unwrap()
            .iter()
            .map(|id| id.as_str().unwrap().to_owned())
            .collect();
        ids.sort_unstable();
        ids
    }

    async fn call(
        &self,
        member: &Member,
        method: &str,
        path: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let uri = format!("/api/v1/workspaces/{}/chat{path}", self.workspace_id);
        let builder = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::COOKIE, &member.cookie);
        let request = match body {
            Some(value) => builder
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(value.to_string())),
            None => builder.body(Body::empty()),
        }
        .unwrap();
        let response = self.app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    /// A call that must succeed.
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
            .expect("every workspace has a default channel")["id"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    async fn channel(
        &self,
        member: &Member,
        name: &str,
        kind: &str,
        members: &[&Member],
    ) -> String {
        let member_ids: Vec<String> = members.iter().map(|member| member.id.to_string()).collect();
        let written = self
            .ok(
                member,
                "POST",
                "/conversations",
                Some(json!({ "name": name, "kind": kind, "member_ids": member_ids })),
            )
            .await;
        written["result"]["id"].as_str().unwrap().to_owned()
    }

    fn next_nonce(&self) -> String {
        format!("n{}", self.nonce.fetch_add(1, Ordering::Relaxed))
    }

    async fn send(&self, member: &Member, conversation: &str, body: &str) -> String {
        self.send_with(member, conversation, json!({ "body": body }))
            .await["result"]["id"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    async fn reply(&self, member: &Member, conversation: &str, root: &str, body: &str) -> String {
        self.send_with(
            member,
            conversation,
            json!({ "body": body, "thread_root_id": root }),
        )
        .await["result"]["id"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    async fn send_with(&self, member: &Member, conversation: &str, mut body: Value) -> Value {
        if body.get("nonce").is_none() {
            body["nonce"] = json!(self.next_nonce());
        }
        self.ok(
            member,
            "POST",
            &format!("/conversations/{conversation}/messages"),
            Some(body),
        )
        .await
    }

    /// `(unread_count, mention_count)` of the member in the conversation.
    async fn counts(&self, member: &Member, conversation: &str) -> (i64, i64) {
        let states = self.ok(member, "GET", "/states", None).await;
        let state = states
            .as_array()
            .unwrap()
            .iter()
            .find(|state| state["conversation_id"] == conversation)
            .unwrap_or_else(|| panic!("no state for {conversation}"))
            .clone();
        (
            state["unread_count"].as_i64().unwrap(),
            state["mention_count"].as_i64().unwrap(),
        )
    }

    /// `(following, unread_replies, mention_count)` of the member in the thread; `None` without a state.
    async fn thread(&self, member: &Member, root: &str) -> Option<(bool, i64, i64)> {
        let page = self
            .ok(member, "GET", &format!("/threads/{root}"), None)
            .await;
        let state = &page["state"];
        (!state.is_null()).then(|| {
            (
                state["following"].as_bool().unwrap(),
                state["unread_replies"].as_i64().unwrap(),
                state["mention_count"].as_i64().unwrap(),
            )
        })
    }

    async fn main_ids(
        &self,
        member: &Member,
        conversation: &str,
        query: &str,
    ) -> (Vec<String>, Value) {
        let page = self
            .ok(
                member,
                "GET",
                &format!("/conversations/{conversation}/messages{query}"),
                None,
            )
            .await;
        let ids = page["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|message| message["id"].as_str().unwrap().to_owned())
            .collect();
        (ids, page)
    }

    /// Fails when a stored counter differs from a count of the messages it stands for.
    async fn assert_counters_exact(&self, step: &str) {
        let wrong: Vec<CounterRow> = sqlx::query_as(
            "SELECT m.conversation_id, m.user_id, \
                    c.message_count - m.read_count, unread.total, \
                    m.mention_count, mentions.total, m.broadcast_count, unread.broadcasts \
             FROM chat_members m JOIN chat_conversations c ON c.id = m.conversation_id \
             JOIN (SELECT m.conversation_id, m.user_id, COUNT(x.id) AS total, \
                          COALESCE(SUM(x.mention_channel OR x.mention_here), 0) AS broadcasts \
                   FROM chat_members m LEFT JOIN chat_messages x ON x.conversation_id = m.conversation_id \
                        AND (x.thread_root_id IS NULL OR x.also_in_channel = 1) AND x.kind = 'message' \
                        AND x.deleted_at IS NULL AND x.author_id <> m.user_id \
                        AND (m.last_read_message_id IS NULL OR x.id > m.last_read_message_id) \
                   GROUP BY m.conversation_id, m.user_id) unread \
                  ON unread.conversation_id = m.conversation_id AND unread.user_id = m.user_id \
             JOIN (SELECT m.conversation_id, m.user_id, COUNT(x.message_id) AS total \
                   FROM chat_members m LEFT JOIN chat_message_mentions x \
                        ON x.conversation_id = m.conversation_id AND x.user_id = m.user_id AND x.in_main = 1 \
                        AND (m.last_read_message_id IS NULL OR x.message_id > m.last_read_message_id) \
                   GROUP BY m.conversation_id, m.user_id) mentions \
                  ON mentions.conversation_id = m.conversation_id AND mentions.user_id = m.user_id \
             WHERE c.message_count - m.read_count <> unread.total \
                OR m.mention_count <> mentions.total OR m.broadcast_count <> unread.broadcasts",
        )
        .fetch_all(self.database.pool())
        .await
        .unwrap();
        assert!(
            wrong.is_empty(),
            "after {step}: (conversation, user, unread stored/counted, mentions stored/counted, \
             broadcasts stored/counted) {wrong:?}"
        );
        let wrong: Vec<(String, String, i64, i64, i64, i64)> = sqlx::query_as(
            "SELECT t.root_id, t.user_id, r.reply_count - t.read_reply_count, \
                    (SELECT COUNT(*) FROM chat_messages x WHERE x.thread_root_id = t.root_id \
                     AND x.author_id <> t.user_id \
                     AND (t.last_read_reply_id IS NULL OR x.id > t.last_read_reply_id)) AS unread, \
                    t.mention_count, \
                    (SELECT COUNT(*) FROM chat_message_mentions x WHERE x.thread_root_id = t.root_id \
                     AND x.user_id = t.user_id \
                     AND (t.last_read_reply_id IS NULL OR x.message_id > t.last_read_reply_id)) AS mentions \
             FROM chat_thread_members t JOIN chat_messages r ON r.id = t.root_id \
             WHERE t.following = 1 \
             AND (r.reply_count - t.read_reply_count <> unread OR t.mention_count <> mentions)",
        )
        .fetch_all(self.database.pool())
        .await
        .unwrap();
        assert!(
            wrong.is_empty(),
            "after {step}: (root, user, unread stored/counted, mentions stored/counted) {wrong:?}"
        );
        let wrong: Vec<(String, i64, i64)> = sqlx::query_as(
            "SELECT r.id, r.reply_count, (SELECT COUNT(*) FROM chat_messages x WHERE x.thread_root_id = r.id) AS replies \
             FROM chat_messages r WHERE r.thread_root_id IS NULL AND r.reply_count <> replies",
        )
        .fetch_all(self.database.pool())
        .await
        .unwrap();
        assert!(wrong.is_empty(), "after {step}: reply counts {wrong:?}");
    }
}

#[tokio::test]
async fn a_send_is_unread_for_the_other_members_and_a_repeated_send_makes_one_message() {
    let fixture = Fixture::new().await;
    let ada = fixture.add_member("ada").await;
    let general = fixture.general().await;

    let first = fixture
        .send_with(
            &fixture.owner,
            &general,
            json!({ "body": "Hello", "nonce": "same" }),
        )
        .await;
    let again = fixture
        .send_with(
            &fixture.owner,
            &general,
            json!({ "body": "Hello", "nonce": "same" }),
        )
        .await;
    assert_eq!(first["result"]["id"], again["result"]["id"]);
    assert_eq!(first["result"]["nonce"], "same");
    // The response carries the events for the caller: the message and the caller's state.
    let types: Vec<&str> = first["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|event| event["type"].as_str().unwrap())
        .collect();
    assert_eq!(types, ["message.created", "state.changed"]);

    assert_eq!(fixture.counts(&ada, &general).await, (1, 0));
    assert_eq!(fixture.counts(&fixture.owner, &general).await, (0, 0));

    let read = fixture
        .ok(
            &ada,
            "POST",
            &format!("/conversations/{general}/read"),
            None,
        )
        .await;
    assert_eq!(read["result"]["unread_count"], 0);
    assert_eq!(
        read["result"]["last_read_message_id"],
        first["result"]["id"]
    );
    // A second read changes nothing and sends no event.
    let read = fixture
        .ok(
            &ada,
            "POST",
            &format!("/conversations/{general}/read"),
            None,
        )
        .await;
    assert_eq!(read["events"], json!([]));
    fixture.assert_counters_exact("read").await;
}

#[tokio::test]
async fn mentions_broadcasts_threads_and_system_rows_count_by_the_rules() {
    let fixture = Fixture::new().await;
    let ada = fixture.add_member("ada").await;
    let bob = fixture.add_member("bob").await;
    let general = fixture.general().await;

    // A user mention counts for that member only; `@channel` for everyone but the author.
    fixture
        .send(&fixture.owner, &general, &format!("<@{}> look", ada.id))
        .await;
    fixture
        .send(&fixture.owner, &general, "<!channel> all")
        .await;
    assert_eq!(fixture.counts(&ada, &general).await, (2, 2));
    assert_eq!(fixture.counts(&bob, &general).await, (2, 1));

    // A muted conversation still counts `@user`, not `@channel`. Unmuting shows it again.
    let state = |notify: &str| Some(json!({ "notify": notify }));
    let path = format!("/conversations/{general}/state");
    let muted = fixture.ok(&ada, "PATCH", &path, state("muted")).await;
    assert_eq!(muted["result"]["mention_count"], 1);
    let unmuted = fixture.ok(&ada, "PATCH", &path, state("mentions")).await;
    assert_eq!(unmuted["result"]["mention_count"], 2);

    // A thread reply does not change the conversation's counts; "also in channel" does.
    let root = fixture.send(&ada, &general, "A question").await;
    assert_eq!(fixture.counts(&ada, &general).await, (0, 0));
    assert_eq!(fixture.counts(&bob, &general).await, (3, 1));
    fixture
        .reply(&fixture.owner, &general, &root, "An answer")
        .await;
    assert_eq!(fixture.counts(&bob, &general).await, (3, 1));
    assert_eq!(fixture.counts(&ada, &general).await, (0, 0));
    fixture
        .send_with(
            &fixture.owner,
            &general,
            json!({ "body": "For everyone", "thread_root_id": root, "also_in_channel": true }),
        )
        .await;
    assert_eq!(fixture.counts(&bob, &general).await, (4, 1));
    assert_eq!(fixture.counts(&ada, &general).await, (1, 0));

    // `@channel` does nothing in a thread.
    let reply = fixture
        .send_with(
            &fixture.owner,
            &general,
            json!({ "body": "<!channel> in a thread", "thread_root_id": root }),
        )
        .await;
    assert_eq!(reply["result"]["mentions"]["channel"], false);
    assert_eq!(fixture.counts(&bob, &general).await, (4, 1));

    // A pin makes a system row: it is in the list, it is never unread.
    fixture
        .ok(
            &fixture.owner,
            "PUT",
            &format!("/messages/{root}/pin"),
            Some(json!({ "pinned": true })),
        )
        .await;
    assert_eq!(fixture.counts(&bob, &general).await, (4, 1));
    let (_, page) = fixture.main_ids(&bob, &general, "").await;
    assert_eq!(
        page["items"].as_array().unwrap().last().unwrap()["kind"],
        "pin"
    );
    let pins = fixture
        .ok(&bob, "GET", &format!("/conversations/{general}/pins"), None)
        .await;
    assert_eq!(pins[0]["id"], root.as_str());
    fixture.assert_counters_exact("the counting rules").await;
}

#[tokio::test]
async fn a_thread_is_followed_by_its_participants_and_by_mentioned_members() {
    let fixture = Fixture::new().await;
    let ada = fixture.add_member("ada").await;
    let bob = fixture.add_member("bob").await;
    let cy = fixture.add_member("cy").await;
    let general = fixture.general().await;

    let root = fixture.send(&ada, &general, "Root").await;
    assert_eq!(fixture.thread(&ada, &root).await, None);

    // The root's author follows from the first reply; the replier has read it; others do not follow.
    fixture.reply(&bob, &general, &root, "First").await;
    assert_eq!(fixture.thread(&ada, &root).await, Some((true, 1, 0)));
    assert_eq!(fixture.thread(&bob, &root).await, Some((true, 0, 0)));
    assert_eq!(fixture.thread(&cy, &root).await, None);

    // A mention in a reply makes the member follow from that reply on.
    fixture
        .reply(&bob, &general, &root, &format!("<@{}> see this", cy.id))
        .await;
    assert_eq!(fixture.thread(&cy, &root).await, Some((true, 1, 1)));
    assert_eq!(fixture.thread(&ada, &root).await, Some((true, 2, 0)));

    // Reading the thread clears it; the root's summary is in the followed list.
    let read = fixture
        .ok(&ada, "POST", &format!("/threads/{root}/read"), None)
        .await;
    assert_eq!(read["result"]["unread_replies"], 0);
    let followed = fixture.ok(&ada, "GET", "/threads", None).await;
    assert_eq!(followed[0]["root"]["id"], root.as_str());
    assert_eq!(followed[0]["root"]["reply_count"], 2);
    assert_eq!(followed[0]["last_reply"]["author_id"], bob.id.to_string());

    // A member who stops following has nothing unread, and does not follow again by itself.
    let follow = |following: bool| Some(json!({ "following": following }));
    fixture
        .ok(
            &ada,
            "PUT",
            &format!("/threads/{root}/follow"),
            follow(false),
        )
        .await;
    fixture.reply(&bob, &general, &root, "Third").await;
    assert_eq!(fixture.thread(&ada, &root).await, Some((false, 0, 0)));
    // Following again starts from now.
    fixture
        .ok(
            &ada,
            "PUT",
            &format!("/threads/{root}/follow"),
            follow(true),
        )
        .await;
    assert_eq!(fixture.thread(&ada, &root).await, Some((true, 0, 0)));
    fixture.reply(&bob, &general, &root, "Fourth").await;
    assert_eq!(fixture.thread(&ada, &root).await, Some((true, 1, 0)));

    // Thread replies never touched the conversation's counts.
    assert_eq!(fixture.counts(&cy, &general).await, (1, 0));
    fixture.assert_counters_exact("the follow rules").await;
}

#[tokio::test]
async fn private_channels_and_dms_do_not_exist_for_other_members() {
    let fixture = Fixture::new().await;
    let ada = fixture.add_member("ada").await;
    let bob = fixture.add_member("bob").await;

    let secret = fixture
        .channel(&fixture.owner, "Secret Plans", "private", &[&ada])
        .await;
    let message = fixture.send(&fixture.owner, &secret, "Only for us").await;
    let listed = fixture.ok(&bob, "GET", "/conversations", None).await;
    assert!(
        listed
            .as_array()
            .unwrap()
            .iter()
            .all(|conversation| conversation["id"] != secret.as_str())
    );
    for (method, path, body) in [
        ("GET", format!("/conversations/{secret}/messages"), None),
        (
            "POST",
            format!("/conversations/{secret}/messages"),
            Some(json!({ "body": "Let me in", "nonce": "x" })),
        ),
        ("POST", format!("/conversations/{secret}/read"), None),
        ("POST", format!("/conversations/{secret}/join"), None),
        ("GET", format!("/conversations/{secret}/pins"), None),
        ("GET", format!("/threads/{message}"), None),
        (
            "PATCH",
            format!("/messages/{message}"),
            Some(json!({ "body": "Changed" })),
        ),
        ("DELETE", format!("/messages/{message}"), None),
        (
            "PUT",
            format!("/messages/{message}/reactions/%F0%9F%91%8D"),
            None,
        ),
    ] {
        let (status, problem) = fixture.call(&bob, method, &path, body).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{method} {path}: {problem}");
        assert_eq!(problem["code"], "chat_not_found");
    }
    // The name is stored normalized, and a member sees the channel.
    let listed = fixture.ok(&ada, "GET", "/conversations", None).await;
    let channel = listed
        .as_array()
        .unwrap()
        .iter()
        .find(|conversation| conversation["id"] == secret.as_str())
        .unwrap();
    assert_eq!(channel["name"], "secret-plans");
    assert_eq!(channel["member_ids"].as_array().unwrap().len(), 2);

    // One set of people has one DM, whoever opens it.
    let open = |members: &[&Member]| {
        Some(
            json!({ "user_ids": members.iter().map(|member| member.id.to_string()).collect::<Vec<_>>() }),
        )
    };
    let dm = fixture.ok(&ada, "POST", "/dms", open(&[&bob])).await;
    let same = fixture.ok(&bob, "POST", "/dms", open(&[&ada])).await;
    assert_eq!(dm["result"]["id"], same["result"]["id"]);
    assert_eq!(dm["result"]["kind"], "dm");
    assert_eq!(dm["result"]["self_dm"], false);
    let group = fixture
        .ok(&ada, "POST", "/dms", open(&[&bob, &fixture.owner]))
        .await;
    assert_ne!(group["result"]["id"], dm["result"]["id"]);
    let dm_id = dm["result"]["id"].as_str().unwrap();
    let (status, _) = fixture
        .call(
            &fixture.owner,
            "GET",
            &format!("/conversations/{dm_id}/messages"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    // A DM counts every message, and nobody leaves it.
    fixture.send(&ada, dm_id, "Hi").await;
    assert_eq!(fixture.counts(&bob, dm_id).await, (1, 0));
    let (status, problem) = fixture
        .call(&bob, "POST", &format!("/conversations/{dm_id}/leave"), None)
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{problem}");

    // Someone who is not in the workspace gets nothing, not even for the default channel.
    let outsider = Member {
        id: Id::new_v7(),
        cookie: "__Host-orbit_session=nothing".to_owned(),
    };
    let (status, _) = fixture.call(&outsider, "GET", "/conversations", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn public_channels_are_readable_by_all_and_writable_by_members() {
    let fixture = Fixture::new().await;
    let ada = fixture.add_member("ada").await;
    let bob = fixture.add_member("bob").await;
    let general = fixture.general().await;
    let design = fixture.channel(&ada, "design", "public", &[]).await;
    fixture.send(&ada, &design, "First").await;

    // A member from before the channel had everyone is not in it: can read, cannot write.
    let cy = fixture.add_member("cy").await;
    let (ids, _) = fixture.main_ids(&cy, &design, "").await;
    assert_eq!(ids.len(), 1);
    let (status, problem) = fixture
        .call(
            &cy,
            "POST",
            &format!("/conversations/{design}/messages"),
            Some(json!({ "body": "Hi", "nonce": "x" })),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{problem}");
    assert_eq!(problem["code"], "chat_forbidden");

    // Joining starts with everything read, and adds a system row.
    let joined = fixture
        .ok(&cy, "POST", &format!("/conversations/{design}/join"), None)
        .await;
    assert_eq!(joined["result"]["member_ids"].as_array().unwrap().len(), 4);
    assert_eq!(fixture.counts(&cy, &design).await, (0, 0));
    fixture.send(&cy, &design, "Hi").await;
    assert_eq!(fixture.counts(&ada, &design).await, (1, 0));
    assert_eq!(fixture.counts(&bob, &design).await, (2, 0));

    // A duplicate name is a conflict; an empty one is invalid.
    let create = |name: &str| Some(json!({ "name": name, "kind": "public" }));
    let (status, problem) = fixture
        .call(&bob, "POST", "/conversations", create("Design"))
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{problem}");
    let (status, _) = fixture
        .call(&bob, "POST", "/conversations", create("   "))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    // Only the creator and managers edit and archive; nobody leaves or archives the default channel.
    let (status, _) = fixture
        .call(
            &bob,
            "PATCH",
            &format!("/conversations/{design}"),
            Some(json!({ "topic": "Mine now" })),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let updated = fixture
        .ok(
            &fixture.owner,
            "PATCH",
            &format!("/conversations/{design}"),
            Some(json!({ "topic": "Screens" })),
        )
        .await;
    assert_eq!(updated["result"]["topic"], "Screens");
    for path in [
        format!("/conversations/{general}/leave"),
        format!("/conversations/{general}/archive"),
    ] {
        let (status, problem) = fixture.call(&fixture.owner, "POST", &path, None).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{path}: {problem}");
    }

    // An archived channel is read-only and unlisted.
    fixture
        .ok(
            &ada,
            "POST",
            &format!("/conversations/{design}/archive"),
            None,
        )
        .await;
    let (status, _) = fixture
        .call(
            &ada,
            "POST",
            &format!("/conversations/{design}/messages"),
            Some(json!({ "body": "Too late", "nonce": "y" })),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let listed = fixture.ok(&ada, "GET", "/conversations", None).await;
    assert!(
        listed
            .as_array()
            .unwrap()
            .iter()
            .all(|conversation| conversation["id"] != design.as_str())
    );
    // The name is free again.
    fixture.channel(&bob, "design", "public", &[]).await;
    fixture.assert_counters_exact("channel changes").await;
}

#[tokio::test]
async fn a_public_channel_has_every_workspace_member_and_nobody_leaves_it() {
    let fixture = Fixture::new().await;
    let ada = fixture.add_member("ada").await;
    let bob = fixture.add_member("bob").await;
    let general = fixture.general().await;
    let design = fixture
        .channel(&fixture.owner, "design", "public", &[])
        .await;
    let secret = fixture
        .channel(&fixture.owner, "secret", "private", &[&ada, &bob])
        .await;
    let old = fixture.channel(&fixture.owner, "old", "public", &[]).await;
    fixture.send(&ada, &design, "Before cy").await;
    fixture.send(&ada, &secret, "Before cy").await;
    fixture
        .ok(
            &fixture.owner,
            "POST",
            &format!("/conversations/{old}/archive"),
            None,
        )
        .await;

    // A new public channel has every workspace member; a private one only the chosen ones.
    let mut everyone = vec![
        fixture.owner.id.to_string(),
        ada.id.to_string(),
        bob.id.to_string(),
    ];
    everyone.sort_unstable();
    assert_eq!(fixture.member_ids(&bob, &design).await, everyone);
    assert_eq!(fixture.counts(&bob, &design).await, (1, 0));
    assert_eq!(fixture.member_ids(&ada, &secret).await.len(), 3);

    // A private channel can be left.
    fixture
        .ok(
            &bob,
            "POST",
            &format!("/conversations/{secret}/leave"),
            None,
        )
        .await;

    // Someone who joins the workspace joins every live public channel, with everything read.
    let cy = fixture.invite_member("cy").await;
    everyone.push(cy.id.to_string());
    everyone.sort_unstable();
    assert_eq!(fixture.member_ids(&cy, &design).await, everyone);
    assert_eq!(fixture.member_ids(&cy, &general).await, everyone);
    assert_eq!(fixture.counts(&cy, &design).await, (0, 0));
    let listed = fixture.ok(&cy, "GET", "/conversations", None).await;
    assert!(
        listed
            .as_array()
            .unwrap()
            .iter()
            .all(|conversation| conversation["id"] != secret.as_str())
    );
    let in_old: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM chat_members WHERE conversation_id = ? AND user_id = ?",
    )
    .bind(&old)
    .bind(cy.id.to_string())
    .fetch_one(fixture.database.pool())
    .await
    .unwrap();
    assert_eq!(in_old, 0, "an archived channel gets nobody new");

    // A private channel made public gets everyone, with everything read.
    let updated = fixture
        .ok(
            &fixture.owner,
            "PATCH",
            &format!("/conversations/{secret}"),
            Some(json!({ "kind": "public" })),
        )
        .await;
    assert_eq!(updated["result"]["member_ids"].as_array().unwrap().len(), 4);
    assert_eq!(fixture.counts(&bob, &secret).await, (0, 0));
    assert_eq!(fixture.counts(&cy, &secret).await, (0, 0));

    // Nobody leaves a public channel or is removed from it.
    for (member, method, path) in [
        (&bob, "POST", format!("/conversations/{design}/leave")),
        (&ada, "POST", format!("/conversations/{secret}/leave")),
        (
            &fixture.owner,
            "DELETE",
            format!("/conversations/{design}/members/{}", ada.id),
        ),
    ] {
        let (status, problem) = fixture.call(member, method, &path, None).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{method} {path}: {problem}");
        assert_eq!(problem["code"], "chat_forbidden");
    }
    assert_eq!(fixture.member_ids(&ada, &design).await, everyone);
    fixture.assert_counters_exact("public channels").await;
}

#[tokio::test]
async fn a_dm_with_only_yourself_is_a_place_for_notes() {
    let fixture = Fixture::new().await;
    let me = fixture.add_member("me").await;
    let notes = fixture
        .ok(&me, "POST", "/dms", Some(json!({ "user_ids": [] })))
        .await;
    let same = fixture
        .ok(
            &me,
            "POST",
            "/dms",
            Some(json!({ "user_ids": [me.id.to_string()] })),
        )
        .await;
    assert_eq!(notes["result"]["id"], same["result"]["id"]);
    assert_eq!(notes["result"]["member_ids"], json!([me.id.to_string()]));
    assert_eq!(notes["result"]["self_dm"], true);
    let notes = notes["result"]["id"].as_str().unwrap();
    let root = fixture
        .send(&me, notes, &format!("A note for <@{}>", me.id))
        .await;
    fixture.reply(&me, notes, &root, "More").await;
    assert_eq!(fixture.counts(&me, notes).await, (0, 0));
    assert_eq!(fixture.thread(&me, &root).await, Some((true, 0, 0)));
    let inbox: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM notifications WHERE recipient_user_id = ?")
            .bind(me.id.to_string())
            .fetch_one(fixture.database.pool())
            .await
            .unwrap();
    assert_eq!(inbox, 0);
    fixture.assert_counters_exact("notes").await;
}

#[tokio::test]
async fn a_deleted_root_with_replies_stays_empty_and_other_messages_are_removed() {
    let fixture = Fixture::new().await;
    let ada = fixture.add_member("ada").await;
    let general = fixture.general().await;

    let root = fixture
        .send(&fixture.owner, &general, &format!("<@{}> root", ada.id))
        .await;
    let first = fixture.reply(&ada, &general, &root, "First").await;
    let second = fixture.reply(&ada, &general, &root, "Second").await;
    let plain = fixture.send(&fixture.owner, &general, "Plain").await;
    assert_eq!(fixture.counts(&ada, &general).await, (2, 1));

    // Only the author and managers delete.
    let (status, _) = fixture
        .call(&ada, "DELETE", &format!("/messages/{plain}"), None)
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // A message without replies is gone, and is no longer unread for anyone.
    let deleted = fixture
        .ok(
            &fixture.owner,
            "DELETE",
            &format!("/messages/{plain}"),
            None,
        )
        .await;
    assert_eq!(deleted["events"][0]["type"], "message.deleted");
    assert_eq!(fixture.counts(&ada, &general).await, (1, 1));

    // A root with replies stays, empty; its mention no longer counts.
    fixture
        .ok(&fixture.owner, "DELETE", &format!("/messages/{root}"), None)
        .await;
    let (ids, page) = fixture.main_ids(&ada, &general, "").await;
    assert_eq!(ids, std::slice::from_ref(&root));
    assert_eq!(page["items"][0]["deleted"], true);
    assert_eq!(page["items"][0]["body"], "");
    assert_eq!(page["items"][0]["reply_count"], 2);
    assert_eq!(fixture.counts(&ada, &general).await, (0, 0));
    fixture.assert_counters_exact("a deleted root").await;

    // The summary follows its replies; the deleted root goes with the last one.
    fixture
        .ok(&ada, "DELETE", &format!("/messages/{second}"), None)
        .await;
    let (_, page) = fixture.main_ids(&ada, &general, "").await;
    assert_eq!(page["items"][0]["reply_count"], 1);
    assert_eq!(page["items"][0]["last_reply"]["body"], "First");
    let last = fixture
        .ok(&ada, "DELETE", &format!("/messages/{first}"), None)
        .await;
    let deleted: Vec<&str> = last["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|event| event["type"] == "message.deleted")
        .map(|event| event["message_id"].as_str().unwrap())
        .collect();
    assert_eq!(deleted, [first.as_str(), root.as_str()]);
    let (ids, _) = fixture.main_ids(&ada, &general, "").await;
    assert!(ids.is_empty());
    fixture.assert_counters_exact("the last reply").await;
}

#[tokio::test]
async fn messages_page_by_id_in_both_directions() {
    let fixture = Fixture::new().await;
    let general = fixture.general().await;
    let mut sent = Vec::new();
    for index in 0..120 {
        sent.push(
            fixture
                .send(&fixture.owner, &general, &format!("Message {index}"))
                .await,
        );
    }
    // Replies are not in the main list.
    fixture
        .reply(&fixture.owner, &general, &sent[5], "Reply")
        .await;

    let (newest, page) = fixture.main_ids(&fixture.owner, &general, "").await;
    assert_eq!(newest, sent[70..]);
    assert_eq!(page["before"], sent[70].as_str());
    assert!(page["after"].is_null());

    let (older, page) = fixture
        .main_ids(
            &fixture.owner,
            &general,
            &format!("?before={}&limit=100", sent[70]),
        )
        .await;
    assert_eq!(older, sent[..70]);
    assert!(page["before"].is_null());

    let (around, page) = fixture
        .main_ids(
            &fixture.owner,
            &general,
            &format!("?around={}&limit=20", sent[60]),
        )
        .await;
    assert_eq!(around, sent[50..70]);
    assert_eq!(page["before"], sent[50].as_str());
    assert_eq!(page["after"], sent[69].as_str());

    let (newer, page) = fixture
        .main_ids(&fixture.owner, &general, &format!("?after={}", sent[100]))
        .await;
    assert_eq!(newer, sent[101..]);
    assert!(page["after"].is_null());

    let (status, _) = fixture
        .call(
            &fixture.owner,
            "GET",
            &format!(
                "/conversations/{general}/messages?before={}&after={}",
                sent[1], sent[2]
            ),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    let threads = fixture
        .ok(
            &fixture.owner,
            "GET",
            &format!("/conversations/{general}/threads"),
            None,
        )
        .await;
    assert_eq!(threads.as_array().unwrap().len(), 1);
    assert_eq!(threads[0]["id"], sent[5].as_str());
}

#[tokio::test]
async fn edits_reactions_and_limits() {
    let fixture = Fixture::new().await;
    let ada = fixture.add_member("ada").await;
    let general = fixture.general().await;
    let message = fixture.send(&fixture.owner, &general, "Draft").await;

    // Only the author edits; an edit that adds a mention counts for the mentioned member.
    let edit = |body: String| Some(json!({ "body": body }));
    let (status, _) = fixture
        .call(
            &ada,
            "PATCH",
            &format!("/messages/{message}"),
            edit("Mine".to_owned()),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let edited = fixture
        .ok(
            &fixture.owner,
            "PATCH",
            &format!("/messages/{message}"),
            edit(format!("<@{}> final", ada.id)),
        )
        .await;
    assert!(!edited["result"]["edited_at"].is_null());
    assert_eq!(
        edited["result"]["mentions"]["user_ids"][0],
        ada.id.to_string()
    );
    assert_eq!(fixture.counts(&ada, &general).await, (1, 1));
    fixture
        .ok(
            &fixture.owner,
            "PATCH",
            &format!("/messages/{message}"),
            edit("No mention".to_owned()),
        )
        .await;
    assert_eq!(fixture.counts(&ada, &general).await, (1, 0));

    let (status, problem) = fixture
        .call(
            &fixture.owner,
            "PATCH",
            &format!("/messages/{message}"),
            edit("x".repeat(4001)),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(problem["code"], "chat_message_too_long");

    // Adding a reaction twice is one reaction; removing one that is not there is fine.
    let path = format!("/messages/{message}/reactions/%F0%9F%91%8D");
    fixture.ok(&ada, "PUT", &path, None).await;
    let reacted = fixture.ok(&ada, "PUT", &path, None).await;
    assert_eq!(
        reacted["result"]["reactions"],
        json!([{ "emoji": "👍", "user_ids": [ada.id.to_string()] }])
    );
    fixture.ok(&fixture.owner, "DELETE", &path, None).await;
    let removed = fixture.ok(&ada, "DELETE", &path, None).await;
    assert_eq!(removed["result"]["reactions"], json!([]));
    fixture.assert_counters_exact("edits").await;
}

#[tokio::test]
async fn mark_unread_read_all_and_restore_move_the_cursors() {
    let fixture = Fixture::new().await;
    let ada = fixture.add_member("ada").await;
    let general = fixture.general().await;
    let design = fixture
        .channel(&fixture.owner, "design", "public", &[&ada])
        .await;

    let first = fixture.send(&fixture.owner, &general, "One").await;
    let second = fixture
        .send(&fixture.owner, &general, &format!("<@{}> two", ada.id))
        .await;
    fixture.send(&fixture.owner, &general, "Three").await;
    fixture.send(&fixture.owner, &design, "Sketch").await;
    let root = fixture.send(&ada, &design, "Root").await;
    fixture.reply(&fixture.owner, &design, &root, "Reply").await;
    fixture
        .ok(
            &ada,
            "POST",
            &format!("/conversations/{general}/read"),
            None,
        )
        .await;
    assert_eq!(fixture.counts(&ada, &general).await, (0, 0));

    // "Mark as unread" puts the cursor before the message and counts from there.
    let unread = fixture
        .ok(&ada, "POST", &format!("/messages/{second}/unread"), None)
        .await;
    assert_eq!(unread["result"]["last_read_message_id"], first.as_str());
    assert_eq!(unread["result"]["unread_count"], 2);
    assert_eq!(unread["result"]["mention_count"], 1);
    fixture.assert_counters_exact("mark unread").await;

    // "Mark all as read" returns what it changed; restoring puts it back.
    let all = fixture.ok(&ada, "POST", "/read-all", None).await;
    assert_eq!(all["result"]["states"].as_array().unwrap().len(), 1);
    assert_eq!(all["result"]["threads"].as_array().unwrap().len(), 1);
    assert_eq!(fixture.counts(&ada, &general).await, (0, 0));
    assert!(
        fixture
            .ok(&ada, "GET", "/threads", None)
            .await
            .as_array()
            .unwrap()
            .iter()
            .all(|thread| thread["state"]["unread_replies"] == 0)
    );
    fixture.assert_counters_exact("read all").await;

    fixture
        .ok(
            &ada,
            "POST",
            "/read-restore",
            Some(restore_body(&all["result"])),
        )
        .await;
    assert_eq!(fixture.counts(&ada, &general).await, (2, 1));
    let followed = fixture.ok(&ada, "GET", "/threads", None).await;
    assert_eq!(followed[0]["state"]["unread_replies"], 1);
    fixture.assert_counters_exact("restore").await;
}

/// The cursors of a `read-all` result, in the shape that `read-restore` takes.
fn restore_body(result: &Value) -> Value {
    let pick = |list: &Value, keys: [&str; 2]| -> Vec<Value> {
        list.as_array()
            .unwrap()
            .iter()
            .map(|state| json!({ keys[0]: state[keys[0]], keys[1]: state[keys[1]] }))
            .collect()
    };
    json!({
        "states": pick(&result["states"], ["conversation_id", "last_read_message_id"]),
        "threads": pick(&result["threads"], ["root_id", "last_read_reply_id"]),
    })
}

#[tokio::test]
async fn categories_and_channel_order_are_for_managers() {
    let fixture = Fixture::new().await;
    let ada = fixture.add_member("ada").await;
    let name = |name: &str| Some(json!({ "name": name }));

    let (status, _) = fixture
        .call(&ada, "POST", "/categories", name("Product"))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let product = fixture
        .ok(&fixture.owner, "POST", "/categories", name("Product"))
        .await["result"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let company = fixture
        .ok(&fixture.owner, "POST", "/categories", name("Company"))
        .await["result"]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let moved = fixture
        .ok(
            &fixture.owner,
            "POST",
            "/move",
            Some(json!({ "category_id": company, "before_id": product })),
        )
        .await;
    let order: Vec<&str> = moved["events"][0]["categories"]
        .as_array()
        .unwrap()
        .iter()
        .map(|category| category["name"].as_str().unwrap())
        .collect();
    assert_eq!(order, ["Company", "Product"]);

    // A channel goes into a category before another channel, or to its end. Only a manager moves it.
    let general = fixture.general().await;
    let design = fixture
        .channel(&fixture.owner, "design", "public", &[&ada])
        .await;
    let docs = fixture
        .channel(&fixture.owner, "docs", "public", &[&ada])
        .await;
    let place = |category: Value, before: Value| {
        Some(json!({ "category_id": category, "before_id": before }))
    };
    let (status, _) = fixture
        .call(
            &ada,
            "POST",
            &format!("/conversations/{design}/place"),
            place(json!(company), Value::Null),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    for (channel, before) in [(&design, Value::Null), (&docs, json!(design))] {
        fixture
            .ok(
                &fixture.owner,
                "POST",
                &format!("/conversations/{channel}/place"),
                place(json!(company), before),
            )
            .await;
    }
    // Back to the channels without a category, before the default channel.
    let placed = fixture
        .ok(
            &fixture.owner,
            "POST",
            &format!("/conversations/{design}/place"),
            place(Value::Null, json!(general)),
        )
        .await;
    assert!(!placed["events"].as_array().unwrap().is_empty());
    let listed = fixture.ok(&ada, "GET", "/conversations", None).await;
    let at = |id: &str| {
        let channel = listed
            .as_array()
            .unwrap()
            .iter()
            .find(|conversation| conversation["id"] == id)
            .unwrap();
        (channel["category_id"].clone(), channel["position"].clone())
    };
    assert_eq!(at(&design), (Value::Null, json!(0)));
    assert_eq!(at(&general), (Value::Null, json!(1)));
    assert_eq!(at(&docs), (json!(company), json!(0)));

    // A channel of a deleted category goes to the channels without one.
    let created = fixture
        .ok(
            &fixture.owner,
            "POST",
            "/conversations",
            Some(json!({ "name": "roadmap", "kind": "public", "category_id": product })),
        )
        .await;
    assert_eq!(created["result"]["category_id"], product.as_str());
    fixture
        .ok(
            &fixture.owner,
            "DELETE",
            &format!("/categories/{product}"),
            None,
        )
        .await;
    let listed = fixture.ok(&ada, "GET", "/conversations", None).await;
    let roadmap = listed
        .as_array()
        .unwrap()
        .iter()
        .find(|conversation| conversation["name"] == "roadmap")
        .unwrap();
    assert!(roadmap["category_id"].is_null());
    assert_eq!(
        roadmap["position"], 2,
        "after the channels without a category"
    );
    let categories = fixture.ok(&ada, "GET", "/categories", None).await;
    assert_eq!(categories.as_array().unwrap().len(), 1);
}

/// Random sends, replies, edits, deletes and reads by three members. After every step each
/// stored counter must equal a count of the messages it stands for.
#[tokio::test]
async fn counters_stay_exact_under_random_activity() {
    let fixture = Fixture::new().await;
    let members = [
        fixture.owner.clone(),
        fixture.add_member("ada").await,
        fixture.add_member("bob").await,
    ];
    let general = fixture.general().await;
    let mut seed = 0x9E37_79B9_7F4A_7C15_u64;
    let mut random = |bound: usize| {
        seed = seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((seed >> 33) as usize) % bound
    };
    // (id, author index, root id) of every live message.
    let mut messages: Vec<(String, usize, Option<String>)> = Vec::new();

    for step in 0..400 {
        let who = random(3);
        let member = &members[who];
        let body = match random(6) {
            0 => format!("<@{}> hello", members[random(3)].id),
            1 => "<!channel> all".to_owned(),
            2 => format!("<@{}> and <@{}>", members[0].id, members[2].id),
            _ => format!("Step {step}"),
        };
        let pick = |messages: &[(String, usize, Option<String>)], index: usize| {
            messages.get(index % messages.len().max(1)).cloned()
        };
        let action = random(12);
        let description = match (action, pick(&messages, random(1000))) {
            (0..=2, _) | (_, None) => {
                let id = fixture.send(member, &general, &body).await;
                messages.push((id, who, None));
                "send"
            }
            (3..=5, Some((id, _, root))) => {
                let root = root.unwrap_or(id);
                let also = random(4) == 0;
                let (status, written) = fixture
                    .call(
                        member,
                        "POST",
                        &format!("/conversations/{general}/messages"),
                        Some(json!({
                            "body": body, "thread_root_id": root, "also_in_channel": also,
                            "nonce": fixture.next_nonce(),
                        })),
                    )
                    .await;
                assert_eq!(status, StatusCode::OK, "{written}");
                messages.push((
                    written["result"]["id"].as_str().unwrap().to_owned(),
                    who,
                    Some(root),
                ));
                "reply"
            }
            (6, Some((id, author, _))) => {
                let (status, edited) = fixture
                    .call(
                        &members[author],
                        "PATCH",
                        &format!("/messages/{id}"),
                        Some(json!({ "body": body })),
                    )
                    .await;
                // A deleted root is still in `messages` until its last reply goes.
                assert!(
                    status == StatusCode::OK || status == StatusCode::NOT_FOUND,
                    "{edited}"
                );
                "edit"
            }
            (7, Some((id, author, _))) => {
                let (status, deleted) = fixture
                    .call(&members[author], "DELETE", &format!("/messages/{id}"), None)
                    .await;
                assert!(
                    status == StatusCode::OK || status == StatusCode::NOT_FOUND,
                    "{deleted}"
                );
                if status == StatusCode::OK {
                    let gone: Vec<&str> = deleted["events"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .filter(|event| event["type"] == "message.deleted")
                        .map(|event| event["message_id"].as_str().unwrap())
                        .collect();
                    messages.retain(|(id, _, _)| !gone.contains(&id.as_str()));
                }
                "delete"
            }
            (8, _) => {
                fixture
                    .ok(
                        member,
                        "POST",
                        &format!("/conversations/{general}/read"),
                        None,
                    )
                    .await;
                "read"
            }
            (9, Some((id, _, _))) => {
                fixture
                    .ok(member, "POST", &format!("/messages/{id}/unread"), None)
                    .await;
                "mark unread"
            }
            (10, Some((id, _, root))) => {
                let root = root.unwrap_or(id);
                let (status, read) = fixture
                    .call(member, "POST", &format!("/threads/{root}/read"), None)
                    .await;
                assert_eq!(status, StatusCode::OK, "{read}");
                "thread read"
            }
            (_, Some((id, _, root))) => {
                let root = root.unwrap_or(id);
                fixture
                    .ok(
                        member,
                        "PUT",
                        &format!("/threads/{root}/follow"),
                        Some(json!({ "following": random(2) == 0 })),
                    )
                    .await;
                "follow"
            }
        };
        fixture
            .assert_counters_exact(&format!("step {step} ({description})"))
            .await;
    }
}

const PNG: &[u8] = b"\x89\x50\x4e\x47\x0d\x0a\x1a\x0a\x00\x00\x00\x0d\x49\x48\x44\x52\x00\x00\x00\x01\x00\x00\x00\x01\x08\x04\x00\x00\x00\xb5\x1c\x0c\x02\x00\x00\x00\x0b\x49\x44\x41\x54\x78\xda\x63\x64\xf8\x0f\x00\x01\x05\x01\x01\x27\x18\xe3\x66\x00\x00\x00\x00\x49\x45\x4e\x44\xae\x42\x60\x82";

impl Fixture {
    /// Uploads a file for a message; returns its record.
    async fn upload(&self, member: &Member, name: &str, bytes: &[u8]) -> Value {
        let boundary = "orbit-test-boundary";
        let mut body = Vec::new();
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{name}\"\r\n\r\n"
            )
            .as_bytes(),
        );
        body.extend_from_slice(bytes);
        body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
        let request = Request::builder()
            .method("POST")
            .uri(format!(
                "/api/v1/workspaces/{}/chat/files?width=640&height=400",
                self.workspace_id
            ))
            .header(header::COOKIE, &member.cookie)
            .header(
                header::CONTENT_TYPE,
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(Body::from(body))
            .unwrap();
        let response = self.app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        assert_eq!(status, StatusCode::CREATED, "{value}");
        value
    }

    /// The status of a download of a chat file path.
    async fn download(&self, member: &Member, url: &str) -> StatusCode {
        let request = Request::builder()
            .uri(url)
            .header(header::COOKIE, &member.cookie)
            .body(Body::empty())
            .unwrap();
        self.app.clone().oneshot(request).await.unwrap().status()
    }
}

#[tokio::test]
async fn a_file_is_its_uploaders_until_a_message_takes_it() {
    let fixture = Fixture::new().await;
    let ada = fixture.add_member("ada").await;
    let bob = fixture.add_member("bob").await;
    let secret = fixture
        .channel(&fixture.owner, "secret", "private", &[&ada])
        .await;

    let file = fixture.upload(&ada, "plan.png", PNG).await;
    let url = file["url"].as_str().unwrap().to_owned();
    assert_eq!(file["mime_type"], "image/png");
    assert_eq!(file["width"], 640);
    // Before the message is sent only the uploader can open it.
    assert_eq!(fixture.download(&ada, &url).await, StatusCode::OK);
    assert_eq!(
        fixture.download(&fixture.owner, &url).await,
        StatusCode::NOT_FOUND
    );

    // Nobody else can attach it, and an unknown id is refused.
    let with_file =
        |nonce: &str| Some(json!({ "body": "", "nonce": nonce, "file_ids": [file["id"]] }));
    let messages = format!("/conversations/{secret}/messages");
    let (status, problem) = fixture
        .call(&fixture.owner, "POST", &messages, with_file("a"))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{problem}");

    // A message may be only a file. The file is then for the readers of the conversation.
    let sent = fixture.ok(&ada, "POST", &messages, with_file("b")).await;
    assert_eq!(sent["result"]["attachments"][0]["id"], file["id"]);
    assert_eq!(sent["result"]["attachments"][0]["file_name"], "plan.png");
    assert_eq!(fixture.download(&fixture.owner, &url).await, StatusCode::OK);
    assert_eq!(fixture.download(&bob, &url).await, StatusCode::NOT_FOUND);
    // It cannot go to a second message.
    let (status, _) = fixture.call(&ada, "POST", &messages, with_file("c")).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    // The page and the files list carry the file.
    let (_, page) = fixture.main_ids(&fixture.owner, &secret, "").await;
    assert_eq!(page["items"][0]["attachments"].as_array().unwrap().len(), 1);
    let files = fixture
        .ok(&ada, "GET", &format!("/conversations/{secret}/files"), None)
        .await;
    assert_eq!(files.as_array().unwrap().len(), 1);
    assert_eq!(files[0]["id"], sent["result"]["id"]);
    // Without text and without a file there is no message.
    let (status, _) = fixture
        .call(
            &ada,
            "POST",
            &messages,
            Some(json!({ "body": " ", "nonce": "d" })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    // A deleted message lets its file go: the blob is quarantined and reclaimed later.
    let message = sent["result"]["id"].as_str().unwrap();
    fixture
        .ok(&ada, "DELETE", &format!("/messages/{message}"), None)
        .await;
    assert_eq!(fixture.download(&ada, &url).await, StatusCode::NOT_FOUND);
    let files: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM chat_message_files")
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    assert_eq!(files, 0);
}

#[tokio::test]
async fn search_finds_messages_in_readable_conversations_only() {
    let fixture = Fixture::new().await;
    let ada = fixture.add_member("ada").await;
    let bob = fixture.add_member("bob").await;
    let general = fixture.general().await;
    let secret = fixture
        .channel(&fixture.owner, "secret", "private", &[&ada])
        .await;
    let release = fixture
        .send(&fixture.owner, &general, "The Über release plan is ready")
        .await;
    fixture.send(&ada, &general, "Lunch plans?").await;
    let hidden = fixture
        .send(&fixture.owner, &secret, "The release budget")
        .await;
    let ids = |page: &Value| -> Vec<String> {
        page["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|hit| hit["message"]["id"].as_str().unwrap().to_owned())
            .collect()
    };

    // Every word must match, without regard to case and accents; the last word is a prefix.
    let page = fixture
        .ok(&ada, "GET", "/search?query=uber%20rel", None)
        .await;
    assert_eq!(ids(&page), std::slice::from_ref(&release));
    assert_eq!(page["items"][0]["ranges"], json!([[4, 8], [9, 16]]));
    assert!(page["cursor"].is_null());

    // A member of the private channel finds its messages, newest first; others do not.
    let page = fixture.ok(&ada, "GET", "/search?query=release", None).await;
    assert_eq!(ids(&page), [hidden.clone(), release.clone()]);
    let page = fixture.ok(&bob, "GET", "/search?query=release", None).await;
    assert_eq!(ids(&page), std::slice::from_ref(&release));
    let (status, _) = fixture
        .call(
            &bob,
            "GET",
            &format!("/search?query=release&conversation_id={secret}"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Filters: one conversation, one author; no words and no filter finds nothing.
    let page = fixture
        .ok(
            &ada,
            "GET",
            &format!("/search?query=plan&author_id={}", ada.id),
            None,
        )
        .await;
    assert_eq!(ids(&page).len(), 1);
    let page = fixture
        .ok(
            &ada,
            "GET",
            &format!("/search?conversation_id={general}"),
            None,
        )
        .await;
    assert_eq!(ids(&page).len(), 2);
    let page = fixture.ok(&ada, "GET", "/search?query=%20", None).await;
    assert!(ids(&page).is_empty());

    // An edited message is found by its new text only; a deleted one is not found.
    fixture
        .ok(
            &fixture.owner,
            "PATCH",
            &format!("/messages/{release}"),
            Some(json!({ "body": "The launch is ready" })),
        )
        .await;
    let page = fixture.ok(&bob, "GET", "/search?query=release", None).await;
    assert!(ids(&page).is_empty());
    let page = fixture.ok(&bob, "GET", "/search?query=launch", None).await;
    assert_eq!(ids(&page), std::slice::from_ref(&release));
    fixture
        .ok(
            &fixture.owner,
            "DELETE",
            &format!("/messages/{release}"),
            None,
        )
        .await;
    let page = fixture.ok(&bob, "GET", "/search?query=launch", None).await;
    assert!(ids(&page).is_empty());

    // The index is whole after edits and deletes (the server's own integrity job runs this).
    sqlx::query("INSERT INTO chat_search (chat_search) VALUES ('integrity-check')")
        .execute(fixture.database.pool())
        .await
        .unwrap();
}

#[tokio::test]
async fn a_mention_in_a_channel_makes_an_inbox_item_that_reading_the_channel_clears() {
    let fixture = Fixture::new().await;
    let ada = fixture.add_member("ada").await;
    let bob = fixture.add_member("bob").await;
    let general = fixture.general().await;
    // (recipient, read) of every chat notification, oldest first.
    let inbox = || async {
        sqlx::query_as::<_, (String, bool)>(
            "SELECT recipient_user_id, read_at IS NOT NULL FROM notifications \
             WHERE kind = 'chat_mentioned' ORDER BY created_at, recipient_user_id",
        )
        .fetch_all(fixture.database.pool())
        .await
        .unwrap()
    };

    // `@user` notifies that member, not the author and not the others.
    let message = fixture
        .send(
            &fixture.owner,
            &general,
            &format!("<@{}> and <@{}>", ada.id, fixture.owner.id),
        )
        .await;
    assert_eq!(inbox().await, [(ada.id.to_string(), false)]);

    // A thread reply and a direct message never go to the inbox.
    fixture
        .reply(
            &fixture.owner,
            &general,
            &message,
            &format!("<@{}> in a thread", bob.id),
        )
        .await;
    let dm = fixture
        .ok(
            &fixture.owner,
            "POST",
            "/dms",
            Some(json!({ "user_ids": [bob.id] })),
        )
        .await["result"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    fixture
        .send(&fixture.owner, &dm, &format!("<@{}> hi", bob.id))
        .await;
    assert_eq!(inbox().await.len(), 1);

    // Reading the channel in chat reads the inbox item.
    fixture
        .ok(
            &ada,
            "POST",
            &format!("/conversations/{general}/read"),
            None,
        )
        .await;
    assert_eq!(inbox().await, [(ada.id.to_string(), true)]);

    // `@channel` notifies every member who has not muted the channel; `@here` only the
    // members who are online (nobody is, without a live socket).
    fixture
        .ok(
            &bob,
            "PATCH",
            &format!("/conversations/{general}/state"),
            Some(json!({ "notify": "muted" })),
        )
        .await;
    let broadcast = fixture
        .send(&fixture.owner, &general, "<!channel> all")
        .await;
    fixture.send(&fixture.owner, &general, "<!here> now").await;
    let unread: Vec<String> = inbox()
        .await
        .into_iter()
        .filter(|(_, read)| !read)
        .map(|(recipient, _)| recipient)
        .collect();
    assert_eq!(unread, [ada.id.to_string()]);

    // A deleted message takes its inbox items along.
    fixture
        .ok(
            &fixture.owner,
            "DELETE",
            &format!("/messages/{broadcast}"),
            None,
        )
        .await;
    assert!(inbox().await.iter().all(|(_, read)| *read));
}

/// Half a million messages in one channel: every call that a page or a click makes must stay
/// fast. Run by hand: `cargo test -p orbit-server --test chat_api scale -- --ignored --nocapture`.
#[tokio::test]
#[ignore = "seeds 500,000 messages; run by hand"]
async fn every_call_stays_fast_with_half_a_million_messages() {
    const MESSAGES: i64 = 500_000;
    let fixture = Fixture::new().await;
    let ada = fixture.add_member("ada").await;
    let general = fixture.general().await;
    let id_at = |index: i64| {
        let time = 1_700_000_000_000_i64 + index;
        format!(
            "{:08x}-{:04x}-7000-8000-000000000000",
            time >> 16,
            time & 0xffff
        )
    };
    sqlx::query(
        "WITH RECURSIVE n(i) AS (SELECT 0 UNION ALL SELECT i + 1 FROM n WHERE i < ? - 1) \
         INSERT INTO chat_messages (id, conversation_id, kind, author_id, body, created_at) \
         SELECT printf('%08x-%04x-7000-8000-000000000000', (1700000000000 + i) >> 16, (1700000000000 + i) & 65535), \
                ?, 'message', ?, 'Message number ' || i, 1700000000000 + i FROM n",
    )
    .bind(MESSAGES)
    .bind(&general)
    .bind(fixture.owner.id.to_string())
    .execute(fixture.database.pool())
    .await
    .unwrap();
    sqlx::query("UPDATE chat_conversations SET message_count = ? WHERE id = ?")
        .bind(MESSAGES)
        .bind(&general)
        .execute(fixture.database.pool())
        .await
        .unwrap();
    sqlx::query(
        "UPDATE chat_members SET last_read_message_id = ?, read_count = ? WHERE user_id = ?",
    )
    .bind(id_at(MESSAGES - 1))
    .bind(MESSAGES)
    .bind(fixture.owner.id.to_string())
    .execute(fixture.database.pool())
    .await
    .unwrap();
    assert_eq!(fixture.counts(&ada, &general).await, (MESSAGES, 0));
    fixture.assert_counters_exact("the seed").await;

    let messages = format!("/conversations/{general}/messages");
    let calls: Vec<(&str, &Member, &str, String, Option<Value>)> = vec![
        ("newest page", &ada, "GET", messages.clone(), None),
        (
            "page around the middle",
            &ada,
            "GET",
            format!("{messages}?around={}", id_at(MESSAGES / 2)),
            None,
        ),
        (
            "older page",
            &ada,
            "GET",
            format!("{messages}?before={}", id_at(1000)),
            None,
        ),
        ("states", &ada, "GET", "/states".to_owned(), None),
        (
            "conversations",
            &ada,
            "GET",
            "/conversations".to_owned(),
            None,
        ),
        (
            "search, a word in one message",
            &ada,
            "GET",
            "/search?query=number%20123456".to_owned(),
            None,
        ),
        (
            "search, a word in every message",
            &ada,
            "GET",
            "/search?query=message%20num".to_owned(),
            None,
        ),
        (
            "send",
            &fixture.owner,
            "POST",
            messages.clone(),
            Some(json!({ "body": "One more <!channel>", "nonce": "scale" })),
        ),
        (
            "react",
            &ada,
            "PUT",
            format!("/messages/{}/reactions/%F0%9F%91%8D", id_at(MESSAGES / 2)),
            None,
        ),
        (
            "pin",
            &ada,
            "PUT",
            format!("/messages/{}/pin", id_at(MESSAGES / 2)),
            Some(json!({ "pinned": true })),
        ),
        (
            "pins",
            &ada,
            "GET",
            format!("/conversations/{general}/pins"),
            None,
        ),
        (
            "reply",
            &ada,
            "POST",
            messages.clone(),
            Some(json!({ "body": "A reply", "nonce": "scale-reply", "thread_root_id": id_at(10) })),
        ),
        (
            "threads",
            &ada,
            "GET",
            format!("/conversations/{general}/threads"),
            None,
        ),
        (
            "thread",
            &ada,
            "GET",
            format!("/threads/{}", id_at(10)),
            None,
        ),
        (
            "delete",
            &fixture.owner,
            "DELETE",
            format!("/messages/{}", id_at(MESSAGES / 3)),
            None,
        ),
        (
            "mark unread, 1,000 messages back",
            &ada,
            "POST",
            format!("/messages/{}/unread", id_at(MESSAGES - 1000)),
            None,
        ),
        (
            "read",
            &ada,
            "POST",
            format!("/conversations/{general}/read"),
            None,
        ),
    ];
    for (name, member, method, path, body) in calls {
        let started = std::time::Instant::now();
        fixture.ok(member, method, &path, body).await;
        let elapsed = started.elapsed();
        eprintln!("{name:<34} {elapsed:>10.2?}");
        assert!(
            elapsed < std::time::Duration::from_millis(250),
            "{name} took {elapsed:?}"
        );
    }
    // The one call whose cost grows with the distance: "mark as unread" on the oldest message
    // counts every message after it. Reported, not bounded.
    let started = std::time::Instant::now();
    fixture
        .ok(
            &ada,
            "POST",
            &format!("/messages/{}/unread", id_at(0)),
            None,
        )
        .await;
    eprintln!(
        "{:<34} {:>10.2?}",
        "mark unread, 500,000 back",
        started.elapsed()
    );
    fixture.assert_counters_exact("the scale run").await;
}

#[tokio::test]
async fn a_member_who_left_does_not_edit_and_an_archived_channel_takes_no_reactions() {
    let fixture = Fixture::new().await;
    let ada = fixture.add_member("ada").await;
    let design = fixture
        .channel(&fixture.owner, "design", "private", &[&ada])
        .await;
    let message = fixture.send(&ada, &design, "Mine").await;
    let edit = Some(json!({ "body": "<!channel> changed" }));

    fixture
        .ok(
            &ada,
            "POST",
            &format!("/conversations/{design}/leave"),
            None,
        )
        .await;
    // The private channel is gone for the member who left.
    let (status, _) = fixture
        .call(&ada, "PATCH", &format!("/messages/{message}"), edit)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    fixture
        .ok(
            &fixture.owner,
            "POST",
            &format!("/conversations/{design}/archive"),
            None,
        )
        .await;
    let (status, _) = fixture
        .call(
            &fixture.owner,
            "PUT",
            &format!("/messages/{message}/reactions/%F0%9F%91%8D"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    fixture.assert_counters_exact("left and archived").await;
}

#[tokio::test]
async fn a_dm_gets_its_member_back_after_the_member_returns_to_the_workspace() {
    let fixture = Fixture::new().await;
    let ada = fixture.add_member("ada").await;
    let open = Some(json!({ "user_ids": [ada.id.to_string()] }));
    let dm = fixture
        .ok(&fixture.owner, "POST", "/dms", open.clone())
        .await;
    let dm_id = dm["result"]["id"].as_str().unwrap();
    fixture.send(&fixture.owner, dm_id, "Hi").await;

    // Removed from the workspace and added again: the membership row is a new one.
    let (membership, created_at): (String, i64) = sqlx::query_as(
        "DELETE FROM memberships WHERE workspace_id = ? AND user_id = ? RETURNING id, created_at",
    )
    .bind(&fixture.workspace_id)
    .bind(ada.id.to_string())
    .fetch_one(fixture.database.pool())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, updated_at) \
         VALUES (?, ?, ?, 'member', 0, ?, ?)",
    )
    .bind(membership)
    .bind(&fixture.workspace_id)
    .bind(ada.id.to_string())
    .bind(created_at)
    .bind(created_at)
    .execute(fixture.database.pool())
    .await
    .unwrap();
    let messages = format!("/conversations/{dm_id}/messages");
    let (status, _) = fixture.call(&ada, "GET", &messages, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let again = fixture.ok(&fixture.owner, "POST", "/dms", open).await;
    assert_eq!(again["result"]["id"], dm["result"]["id"]);
    assert_eq!(again["result"]["member_ids"].as_array().unwrap().len(), 2);
    fixture.ok(&ada, "GET", &messages, None).await;
    assert_eq!(
        fixture.counts(&ada, dm_id).await,
        (0, 0),
        "starts with all read"
    );
    fixture.assert_counters_exact("dm repaired").await;
}

/// Records the pushes it is asked to send.
#[derive(Default)]
struct RecordedPushes(std::sync::Mutex<Vec<(String, Value)>>);

impl orbit_server::push::PushSender for RecordedPushes {
    fn send(
        &self,
        _vapid: orbit_server::push::Vapid,
        subscription: orbit_server::push::Subscription,
        payload: Vec<u8>,
    ) -> futures_util::future::BoxFuture<'static, Result<(), orbit_server::push::SendError>> {
        self.0.lock().unwrap().push((
            subscription.endpoint,
            serde_json::from_slice(&payload).unwrap(),
        ));
        Box::pin(async { Ok(()) })
    }
}

#[tokio::test]
async fn a_message_is_pushed_to_the_members_it_is_for() {
    let fixture = Fixture::new().await;
    let pushes = Arc::new(RecordedPushes::default());
    // Before the first send: the database keeps the first push service it gets.
    let push = orbit_server::push::PushService::with_sender(&fixture.database, pushes.clone());
    let ada = fixture.add_member("ada").await;
    let bob = fixture.add_member("bob").await;
    for member in [&ada, &bob, &fixture.owner] {
        let session: String = sqlx::query_scalar("SELECT id FROM sessions WHERE user_id = ?")
            .bind(member.id.to_string())
            .fetch_one(fixture.database.pool())
            .await
            .unwrap();
        push.subscribe(
            member.id,
            session.parse().unwrap(),
            &format!("https://fcm.googleapis.com/fcm/send/{}", member.id),
            "key",
            "auth",
            "Browser",
            TimestampMillis::now(),
        )
        .await
        .unwrap();
    }
    sqlx::query("INSERT INTO user_avatars (user_id, mime_type, bytes, updated_at) VALUES (?, 'image/png', x'89', 7)")
        .bind(fixture.owner.id.to_string())
        .execute(fixture.database.pool())
        .await
        .unwrap();
    let general = fixture.general().await;
    // The pushes of a send go out after the response: wait for as many as the send must make.
    let pushed = |count: usize| {
        let pushes = pushes.clone();
        async move {
            for _ in 0..100 {
                if pushes.0.lock().unwrap().len() >= count {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
            // Nothing more may follow.
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            std::mem::take(&mut *pushes.0.lock().unwrap())
        }
    };
    let endpoint = |member: &Member| format!("https://fcm.googleapis.com/fcm/send/{}", member.id);

    // A plain channel message at the default level ("mentions"): nobody.
    fixture.send(&fixture.owner, &general, "hello").await;
    assert!(pushed(0).await.is_empty());

    // A mention: that member only, with the name in place of the token; never the author.
    let body = format!("<@{}> can you check\nthe <#{general}> import?", ada.id);
    fixture.send(&fixture.owner, &general, &body).await;
    let sent = pushed(1).await;
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].0, endpoint(&ada));
    assert_eq!(sent[0].1["title"], "Owner in #general");
    assert_eq!(sent[0].1["body"], "@ada can you check the #general import?");
    assert_eq!(sent[0].1["url"], format!("/chat/{general}"));
    assert_eq!(sent[0].1["sound"], "mention");
    assert_eq!(
        sent[0].1["icon"],
        format!("/api/v1/users/{}/avatar?v=7", fixture.owner.id)
    );

    // A token gets a name only for a member the message mentions and for a public channel of
    // the workspace: a private channel and somebody unknown stay without a name.
    let secret = fixture
        .channel(&fixture.owner, "secret", "private", &[])
        .await;
    let body = format!(
        "<@{}> see <#{secret}> and <#{general}>, ask <@{}>",
        ada.id,
        Id::new_v7()
    );
    fixture.send(&fixture.owner, &general, &body).await;
    let sent = pushed(1).await;
    assert_eq!(sent.len(), 1);
    assert_eq!(
        sent[0].1["body"],
        "@ada see #channel and #general, ask @someone"
    );

    // Bob set the channel to "all": he gets every message. Ada muted it: not even a mention.
    let state = |level: &str| json!({ "notify": level });
    let path = format!("/conversations/{general}/state");
    fixture.ok(&bob, "PATCH", &path, Some(state("all"))).await;
    fixture.ok(&ada, "PATCH", &path, Some(state("muted"))).await;
    fixture
        .send(&fixture.owner, &general, &format!("<@{}> again", ada.id))
        .await;
    let sent = pushed(1).await;
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].0, endpoint(&bob));
    assert_eq!(sent[0].1["sound"], "message");

    // A reply in a thread: its followers (the root's author), not a channel set to "all".
    let root = fixture.send(&ada, &general, "root").await;
    pushed(1).await;
    fixture
        .reply(&fixture.owner, &general, &root, "reply")
        .await;
    let sent = pushed(0).await;
    assert!(
        sent.is_empty(),
        "Ada follows the thread but muted the channel; Bob does not follow: {sent:?}"
    );
    fixture
        .ok(&ada, "PATCH", &path, Some(state("mentions")))
        .await;
    fixture
        .reply(&fixture.owner, &general, &root, "second reply")
        .await;
    let sent = pushed(1).await;
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].0, endpoint(&ada));
    assert_eq!(sent[0].1["url"], format!("/chat/{general}?thread={root}"));
}
