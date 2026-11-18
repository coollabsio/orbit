//! Reports of chat messages and blocks between users (App Store guideline 1.2).

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use orbit_platform::{
    AttachmentMutationCoordinator, AuthenticatedUser, Id, LocalBlobStore, PasswordService,
    TestDatabase, TimestampMillis, UploadLimits, UploadService,
};
use orbit_server::auth_routes::CookieMode;
use orbit_server::chat_routes::{ChatState, chat_router};
use orbit_server::mail::{Mailer, Outbox};
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use serde_json::{Value, json};
use tower::ServiceExt;

struct Fixture {
    _root: tempfile::TempDir,
    database: TestDatabase,
    identity: Arc<IdentityRepository>,
    app: axum::Router,
    outbox: Outbox,
    workspace_id: String,
    owner: Member,
    nonce: AtomicU64,
}

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
        let (mailer, outbox) = Mailer::recording((*database).clone(), "https://orbit.example");
        let app = chat_router(
            ChatState::new(Arc::clone(&identity), uploads, CookieMode::secure())
                .with_mailer(mailer),
        );
        Self {
            _root: root,
            database,
            identity,
            app,
            outbox,
            workspace_id: setup.workspace_id.to_string(),
            owner: Member {
                id: setup.user_id,
                cookie: format!("__Host-orbit_session={}", setup.session.token),
            },
            nonce: AtomicU64::new(0),
        }
    }

    /// A user with a session; a member of the workspace when `member` is set.
    async fn user(&self, name: &str, member: bool) -> Member {
        let id = Id::new_v7();
        let now = TimestampMillis::now().as_millis();
        let email = format!("{name}@example.com");
        sqlx::query(
            "INSERT INTO users (id, email, normalized_email, display_name, password_hash, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, 'unused', $5, $6)",
        )
        .bind(id.to_string())
        .bind(&email)
        .bind(&email)
        .bind(name)
        .bind(now)
        .bind(now)
        .execute(self.database.pool())
        .await
        .unwrap();
        if member {
            sqlx::query(
                "INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, updated_at) \
                 VALUES ($1, $2, $3, 'member', 0, $4, $5)",
            )
            .bind(Id::new_v7().to_string())
            .bind(&self.workspace_id)
            .bind(id.to_string())
            .bind(now)
            .bind(now)
            .execute(self.database.pool())
            .await
            .unwrap();
        }
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

    /// `path` is under `/api/v1`.
    async fn call(
        &self,
        member: &Member,
        method: &str,
        path: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let builder = Request::builder()
            .method(method)
            .uri(format!("/api/v1{path}"))
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

    fn chat(&self, path: &str) -> String {
        format!("/workspaces/{}/chat{path}", self.workspace_id)
    }

    async fn general(&self) -> String {
        let (_, conversations) = self
            .call(&self.owner, "GET", &self.chat("/conversations"), None)
            .await;
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

    async fn dm(&self, member: &Member, with: &Member) -> (StatusCode, Value) {
        self.call(
            member,
            "POST",
            &self.chat("/dms"),
            Some(json!({ "user_ids": [with.id.to_string()] })),
        )
        .await
    }

    async fn send(&self, member: &Member, conversation: &str, body: &str) -> (StatusCode, Value) {
        let nonce = format!("n{}", self.nonce.fetch_add(1, Ordering::Relaxed));
        self.call(
            member,
            "POST",
            &self.chat(&format!("/conversations/{conversation}/messages")),
            Some(json!({ "body": body, "nonce": nonce })),
        )
        .await
    }

    async fn report(&self, member: &Member, message: &str, body: Value) -> (StatusCode, Value) {
        self.call(
            member,
            "POST",
            &self.chat(&format!("/messages/{message}/report")),
            Some(body),
        )
        .await
    }

    async fn report_notifications(&self, recipient: Id) -> Vec<String> {
        sqlx::query_scalar(
            "SELECT chat_report_id FROM notifications WHERE recipient_user_id = $1 \
             AND kind = 'chat_reported' ORDER BY id",
        )
        .bind(recipient.to_string())
        .fetch_all(self.database.pool())
        .await
        .unwrap()
    }
}

#[tokio::test]
async fn a_member_reports_a_message_they_can_read_and_owners_and_admins_act_on_it() {
    let fixture = Fixture::new().await;
    let ada = fixture.user("ada", true).await;
    let bob = fixture.user("bob", true).await;
    let outsider = fixture.user("outsider", false).await;
    let general = fixture.general().await;
    let (_, sent) = fixture
        .send(&fixture.owner, &general, "Buy cheap watches")
        .await;
    let message = sent["result"]["id"].as_str().unwrap().to_owned();

    let (status, report) = fixture
        .report(
            &ada,
            &message,
            json!({ "reason": "spam", "note": "  Again  " }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!(report["reason"], "spam");
    assert_eq!(report["note"], "Again");
    assert_eq!(report["body"], "Buy cheap watches");
    assert_eq!(report["reporter_id"], ada.id.to_string());
    assert_eq!(report["author_id"], fixture.owner.id.to_string());
    let report_id = report["id"].as_str().unwrap().to_owned();
    assert_eq!(
        fixture.report_notifications(fixture.owner.id).await,
        vec![report_id.clone()]
    );
    let emails = fixture.outbox.lock().unwrap().clone();
    assert_eq!(emails.len(), 1);
    assert_eq!(emails[0].to, "owner@example.com");
    assert!(
        emails[0].body.contains("Reason: spam"),
        "{}",
        emails[0].body
    );
    assert!(emails[0].body.contains("\"Buy cheap watches\""));

    // A second report of the same message is the first one, with no new notice.
    let (status, again) = fixture
        .report(&ada, &message, json!({ "reason": "other" }))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(again["id"], report_id.as_str());
    assert_eq!(
        fixture.report_notifications(fixture.owner.id).await.len(),
        1
    );
    assert_eq!(fixture.outbox.lock().unwrap().len(), 1);

    // Nobody reports their own message, and the reason and note are checked.
    let (status, problem) = fixture
        .report(&fixture.owner, &message, json!({ "reason": "spam" }))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{problem}");
    let (status, _) = fixture
        .report(&bob, &message, json!({ "reason": "rude" }))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = fixture
        .report(
            &bob,
            &message,
            json!({ "reason": "other", "note": "x".repeat(501) }),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    // A message the caller cannot read does not exist for them: a DM of others, or any message
    // for someone outside the workspace.
    let (_, dm) = fixture.dm(&ada, &bob).await;
    let dm = dm["result"]["id"].as_str().unwrap().to_owned();
    let (_, sent) = fixture.send(&bob, &dm, "You are an idiot").await;
    let private = sent["result"]["id"].as_str().unwrap().to_owned();
    for (member, target) in [(&fixture.owner, &private), (&outsider, &message)] {
        let (status, problem) = fixture
            .report(member, target, json!({ "reason": "spam" }))
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{problem}");
    }

    // Ada reports Bob in their DM. The owner cannot read the DM, but reads the report and
    // deletes the message; a member can do neither.
    let (status, report) = fixture
        .report(&ada, &private, json!({ "reason": "harassment" }))
        .await;
    assert_eq!(status, StatusCode::OK, "{report}");
    let dm_report = report["id"].as_str().unwrap().to_owned();
    let (status, _) = fixture
        .call(&ada, "GET", &fixture.chat("/reports"), None)
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = fixture
        .call(
            &bob,
            "DELETE",
            &fixture.chat(&format!("/reports/{dm_report}/message")),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, listed) = fixture
        .call(&fixture.owner, "GET", &fixture.chat("/reports"), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    let listed: Vec<(&str, &str, bool)> = listed
        .as_array()
        .unwrap()
        .iter()
        .map(|report| {
            (
                report["body"].as_str().unwrap(),
                report["reporter_name"].as_str().unwrap(),
                report["message_deleted"].as_bool().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        listed,
        vec![
            ("You are an idiot", "ada", false),
            ("Buy cheap watches", "ada", false)
        ]
    );
    let (status, _) = fixture
        .call(
            &fixture.owner,
            "DELETE",
            &fixture.chat(&format!("/reports/{dm_report}/message")),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (_, page) = fixture
        .call(
            &ada,
            "GET",
            &fixture.chat(&format!("/conversations/{dm}/messages")),
            None,
        )
        .await;
    assert_eq!(page["items"].as_array().unwrap().len(), 0, "{page}");
    let (_, listed) = fixture
        .call(&fixture.owner, "GET", &fixture.chat("/reports"), None)
        .await;
    assert_eq!(listed[0]["message_deleted"], true);
    assert_eq!(listed[0]["body"], "You are an idiot");
}

#[tokio::test]
async fn a_blocked_person_cannot_open_or_write_a_dm_with_the_blocker() {
    let fixture = Fixture::new().await;
    let ada = fixture.user("ada", true).await;
    let bob = fixture.user("bob", true).await;
    let outsider = fixture.user("outsider", false).await;
    let (_, dm) = fixture.dm(&bob, &ada).await;
    let dm = dm["result"]["id"].as_str().unwrap().to_owned();

    let block = |target: &Member| format!("/chat/blocks/{}", target.id);
    let (status, _) = fixture.call(&ada, "PUT", &block(&bob), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = fixture.call(&ada, "PUT", &block(&bob), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, listed) = fixture.call(&ada, "GET", "/chat/blocks", None).await;
    assert_eq!(listed.as_array().unwrap().len(), 1, "{listed}");
    assert_eq!(listed[0]["user_id"], bob.id.to_string());
    assert_eq!(listed[0]["display_name"], "bob");
    let (_, theirs) = fixture.call(&bob, "GET", "/chat/blocks", None).await;
    assert_eq!(theirs, json!([]));

    // Only people who share a workspace, and not oneself.
    let (status, _) = fixture.call(&ada, "PUT", &block(&outsider), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = fixture.call(&ada, "PUT", &block(&ada), None).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    let (status, problem) = fixture.dm(&bob, &ada).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{problem}");
    assert_eq!(problem["code"], "chat_forbidden");
    let (status, problem) = fixture.send(&bob, &dm, "Hello?").await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{problem}");
    let (status, _) = fixture
        .call(
            &bob,
            "POST",
            &fixture.chat("/dms"),
            Some(json!({ "user_ids": [ada.id.to_string(), fixture.owner.id.to_string()] })),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    // The blocker still writes, and Bob still writes in channels.
    let (status, _) = fixture.send(&ada, &dm, "Stop").await;
    assert_eq!(status, StatusCode::OK);
    let general = fixture.general().await;
    let (status, _) = fixture.send(&bob, &general, "Hi all").await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = fixture.call(&ada, "DELETE", &block(&bob), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = fixture.send(&bob, &dm, "Sorry").await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = fixture.dm(&bob, &ada).await;
    assert_eq!(status, StatusCode::OK);
}
