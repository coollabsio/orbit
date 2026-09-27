use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use orbit_platform::{AuthenticatedUser, Id, PasswordService, TestDatabase, TimestampMillis};
use orbit_server::auth_routes::CookieMode;
use orbit_server::page_comment_routes::{PageCommentState, page_comment_router};
use orbit_server::page_routes::{PageState, page_router};
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::repositories::workspaces::WorkspaceRepository;
use orbit_server::task_routes::{TaskState, task_router};
use serde_json::{Value, json};
use tower::ServiceExt;

struct Fixture {
    database: TestDatabase,
    identity: Arc<IdentityRepository>,
    app: axum::Router,
    workspace_id: String,
    owner_id: Id,
    owner: String,
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
        let app = page_router(PageState::new(Arc::clone(&identity), CookieMode::secure()))
            .merge(page_comment_router(PageCommentState::new(
                Arc::clone(&identity),
                CookieMode::secure(),
            )))
            .merge(task_router(TaskState::new(
                Arc::clone(&identity),
                CookieMode::secure(),
            )));
        Self {
            database,
            identity,
            app,
            workspace_id: setup.workspace_id.to_string(),
            owner_id: setup.user_id,
            owner: format!("__Host-orbit_session={}", setup.session.token),
        }
    }

    fn pages_uri(&self) -> String {
        format!("/api/v1/workspaces/{}/pages", self.workspace_id)
    }

    fn threads_uri(&self, page_id: &str) -> String {
        format!("{}/{page_id}/threads", self.pages_uri())
    }

    async fn call(
        &self,
        cookie: &str,
        method: &str,
        uri: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut builder = Request::builder().method(method).uri(uri);
        if !cookie.is_empty() {
            builder = builder.header(header::COOKIE, cookie);
        }
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
        let value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or(Value::Null)
        };
        (status, value)
    }

    async fn create_page(&self, cookie: &str, body: Value) -> String {
        let (status, page) = self
            .call(cookie, "POST", &self.pages_uri(), Some(body))
            .await;
        assert_eq!(status, StatusCode::CREATED, "{page}");
        page["id"].as_str().unwrap().to_owned()
    }

    async fn start_thread(&self, cookie: &str, page_id: &str, body: Value) -> (StatusCode, Value) {
        self.call(
            cookie,
            "POST",
            &self.threads_uri(page_id),
            Some(json!({ "body": body, "quote": "  selected words " })),
        )
        .await
    }

    async fn inbox(&self, cookie: &str) -> Vec<Value> {
        let (status, body) = self
            .call(
                cookie,
                "GET",
                &format!("/api/v1/workspaces/{}/notifications", self.workspace_id),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body["items"].as_array().unwrap().clone()
    }

    async fn count(&self, sql: &str) -> i64 {
        sqlx::query_scalar(sql)
            .fetch_one(self.database.pool())
            .await
            .unwrap()
    }

    async fn page_version(&self, page_id: &str) -> i64 {
        sqlx::query_scalar("SELECT version FROM pages WHERE id = ?")
            .bind(page_id)
            .fetch_one(self.database.pool())
            .await
            .unwrap()
    }

    async fn add_member(&self, workspace_id: &str, email: &str, name: &str) -> Member {
        let id = Id::new_v7();
        let now = TimestampMillis::now();
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
        .execute(self.database.pool())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, updated_at) \
             VALUES (?, ?, ?, 'member', 0, ?, ?)",
        )
        .bind(Id::new_v7().to_string())
        .bind(workspace_id)
        .bind(id.to_string())
        .bind(now.as_millis())
        .bind(now.as_millis())
        .execute(self.database.pool())
        .await
        .unwrap();
        let session = self
            .identity
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
        Member {
            id,
            cookie: format!("__Host-orbit_session={}", session.token),
        }
    }
}

fn text(value: &str) -> Value {
    json!([{ "type": "paragraph", "content": [{ "type": "text", "text": value, "styles": {} }] }])
}

fn mentioning(value: &str, users: &[(Id, &str)]) -> Value {
    let mut content = vec![json!({ "type": "text", "text": value, "styles": {} })];
    for (id, name) in users {
        content.push(
            json!({ "type": "mention", "props": { "userId": id.to_string(), "name": name } }),
        );
    }
    json!([{ "type": "paragraph", "content": content }])
}

fn id(value: &Value) -> String {
    value["id"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn threads_support_replies_edits_resolve_and_author_only_changes() {
    let fixture = Fixture::new().await;
    let owner = fixture.owner.clone();
    let member = fixture
        .add_member(&fixture.workspace_id, "member@example.com", "Member")
        .await;
    let page = fixture.create_page(&owner, json!({"title": "Plan"})).await;

    let (status, thread) = fixture
        .start_thread(
            &owner,
            &page,
            json!([{
                "id": "b1",
                "type": "paragraph",
                "props": { "textColor": "default" },
                "content": [
                    { "type": "text", "text": "Check ", "styles": { "bold": true } },
                    { "type": "link", "href": "javascript:alert(1)", "content": [{ "type": "text", "text": "this", "styles": {} }] }
                ],
                "children": []
            }]),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{thread}");
    assert_eq!(thread["page_id"], page.as_str());
    assert_eq!(thread["quote"], "selected words");
    assert_eq!(thread["resolved"], false);
    assert_eq!(thread["created_by"], fixture.owner_id.to_string());
    let first = &thread["comments"][0];
    assert_eq!(first["body_text"], "Check this");
    // The unsafe link became plain text.
    assert_eq!(
        first["body"][0]["content"][1],
        json!({ "type": "text", "text": "this", "styles": {} })
    );
    let thread_id = id(&thread);
    let first_id = id(first);
    let thread_uri = format!("{}/{thread_id}", fixture.threads_uri(&page));

    // Any member who sees the page reads, replies and resolves.
    let (status, list) = fixture
        .call(&member.cookie, "GET", &fixture.threads_uri(&page), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list["items"].as_array().unwrap().len(), 1);
    let (status, reply) = fixture
        .call(
            &member.cookie,
            "POST",
            &format!("{thread_uri}/comments"),
            Some(json!({ "body": text("Done") })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{reply}");
    assert_eq!(reply["author_id"], member.id.to_string());
    let reply_id = id(&reply);

    // Only authors edit or delete their comments.
    let (status, problem) = fixture
        .call(
            &member.cookie,
            "PATCH",
            &format!("{thread_uri}/comments/{first_id}"),
            Some(json!({ "body": text("hijack") })),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(problem["code"], "page_comment_forbidden");
    let (status, _) = fixture
        .call(
            &owner,
            "DELETE",
            &format!("{thread_uri}/comments/{reply_id}"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = fixture
        .call(&member.cookie, "DELETE", &thread_uri, None)
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, edited) = fixture
        .call(
            &owner,
            "PATCH",
            &format!("{thread_uri}/comments/{first_id}"),
            Some(json!({ "body": text("Check this, please") })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{edited}");
    assert_eq!(edited["body_text"], "Check this, please");
    assert!(edited["edited_at"].is_string());

    let (status, resolved) = fixture
        .call(
            &member.cookie,
            "POST",
            &format!("{thread_uri}/resolve"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(resolved["resolved"], true);
    assert_eq!(resolved["resolved_by"], member.id.to_string());
    // Resolving twice is a no-op; reopening clears it.
    let (_, again) = fixture
        .call(&owner, "POST", &format!("{thread_uri}/resolve"), None)
        .await;
    assert_eq!(again["resolved_by"], member.id.to_string());
    let (status, reopened) = fixture
        .call(&owner, "POST", &format!("{thread_uri}/reopen"), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reopened["resolved"], false);
    assert!(reopened["resolved_by"].is_null());

    // A deleted comment stays as a placeholder without its body.
    let (status, _) = fixture
        .call(
            &member.cookie,
            "DELETE",
            &format!("{thread_uri}/comments/{reply_id}"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, list) = fixture
        .call(&owner, "GET", &fixture.threads_uri(&page), None)
        .await;
    let deleted = &list["items"][0]["comments"][1];
    assert!(deleted["deleted_at"].is_string());
    assert_eq!(deleted["body"], json!([]));
    assert_eq!(deleted["body_text"], "");
    let (status, _) = fixture
        .call(
            &member.cookie,
            "PATCH",
            &format!("{thread_uri}/comments/{reply_id}"),
            Some(json!({ "body": text("back") })),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Deleting the last live comment removes the thread.
    let (status, _) = fixture
        .call(
            &owner,
            "DELETE",
            &format!("{thread_uri}/comments/{first_id}"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, list) = fixture
        .call(&owner, "GET", &fixture.threads_uri(&page), None)
        .await;
    assert_eq!(list["items"], json!([]));

    // Every change is audited (and so reaches other viewers as a realtime event).
    for action in [
        "page.thread_created",
        "page.comment_created",
        "page.comment_updated",
        "page.thread_resolved",
        "page.thread_reopened",
        "page.comment_deleted",
    ] {
        let recorded = fixture
            .count(&format!(
                "SELECT COUNT(*) FROM audit_events WHERE action = '{action}' AND outcome = 'success'"
            ))
            .await;
        assert!(recorded >= 1, "{action}");
    }
    assert_eq!(
        fixture
            .count("SELECT COUNT(*) FROM audit_events WHERE action = 'page.thread_resolved'")
            .await,
        1
    );

    // A thread's creator deletes the whole thread.
    let (_, thread) = fixture.start_thread(&owner, &page, text("Again")).await;
    let (status, _) = fixture
        .call(
            &owner,
            "DELETE",
            &format!("{}/{}", fixture.threads_uri(&page), id(&thread)),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(fixture.count("SELECT COUNT(*) FROM page_threads").await, 0);
}

#[tokio::test]
async fn invalid_bodies_are_rejected() {
    let fixture = Fixture::new().await;
    let page = fixture
        .create_page(&fixture.owner, json!({"title": "Plan"}))
        .await;
    for body in [
        json!([]),
        json!("text"),
        text("   "),
        json!([{ "type": "heading", "content": [{ "type": "text", "text": "x" }] }]),
        json!([{ "type": "paragraph", "content": [{ "type": "mention", "props": { "userId": "x" } }] }]),
    ] {
        let (status, problem) = fixture.start_thread(&fixture.owner, &page, body).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{problem}");
    }
    let (status, _) = fixture
        .call(
            &fixture.owner,
            "POST",
            &fixture.threads_uri(&page),
            Some(json!({ "body": text("ok"), "extra": 1 })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = fixture.start_thread("", &page, text("anonymous")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn mentions_notify_members_who_can_see_the_page_once() {
    let fixture = Fixture::new().await;
    let owner = fixture.owner.clone();
    let member = fixture
        .add_member(&fixture.workspace_id, "member@example.com", "Member")
        .await;
    let second = fixture
        .add_member(&fixture.workspace_id, "second@example.com", "Second")
        .await;
    let page = fixture.create_page(&owner, json!({"title": "Plan"})).await;

    // Mentioning yourself never notifies; the member gets one inbox entry.
    let (status, thread) = fixture
        .start_thread(
            &owner,
            &page,
            mentioning(
                "Hey ",
                &[(member.id, "Member"), (fixture.owner_id, "Owner")],
            ),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{thread}");
    let comment = &thread["comments"][0];
    let mut mentioned: Vec<String> = comment["mentioned_user_ids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|id| id.as_str().unwrap().to_owned())
        .collect();
    mentioned.sort();
    let mut expected = vec![member.id.to_string(), fixture.owner_id.to_string()];
    expected.sort();
    assert_eq!(mentioned, expected);
    assert_eq!(comment["body_text"], "Hey @Member@Owner");
    assert!(fixture.inbox(&owner).await.is_empty());
    let inbox = fixture.inbox(&member.cookie).await;
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0]["kind"], "page_comment_mentioned");
    assert_eq!(inbox[0]["page_id"], page.as_str());
    assert_eq!(inbox[0]["page_thread_id"], thread["id"]);
    assert_eq!(inbox[0]["page_comment_id"], comment["id"]);
    assert!(inbox[0]["task_id"].is_null());
    assert_eq!(inbox[0]["actor_user_id"], fixture.owner_id.to_string());

    // Editing notifies only newly mentioned members (removing and re-adding does not repeat).
    let comment_uri = format!(
        "{}/{}/comments/{}",
        fixture.threads_uri(&page),
        id(&thread),
        id(comment)
    );
    for body in [
        mentioning("Only ", &[(second.id, "Second")]),
        mentioning("Both ", &[(member.id, "Member"), (second.id, "Second")]),
    ] {
        let (status, _) = fixture
            .call(&owner, "PATCH", &comment_uri, Some(json!({ "body": body })))
            .await;
        assert_eq!(status, StatusCode::OK);
    }
    assert_eq!(fixture.inbox(&member.cookie).await.len(), 1);
    assert_eq!(fixture.inbox(&second.cookie).await.len(), 1);

    // Nobody who cannot see the page is notified: a private page, another workspace's user.
    let private = fixture
        .create_page(&owner, json!({"title": "Secret", "private": true}))
        .await;
    let other = WorkspaceRepository::new((*fixture.database).clone())
        .create(
            fixture.owner_id,
            "Other".to_owned(),
            "comments-test",
            TimestampMillis::now(),
        )
        .await
        .unwrap()
        .id;
    let outsider = fixture
        .add_member(&other.to_string(), "outsider@example.com", "Outsider")
        .await;
    let (status, thread) = fixture
        .start_thread(
            &owner,
            &private,
            mentioning("Psst ", &[(member.id, "Member"), (outsider.id, "Outsider")]),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{thread}");
    assert_eq!(thread["comments"][0]["mentioned_user_ids"], json!([]));
    let (status, thread) = fixture
        .start_thread(
            &owner,
            &page,
            mentioning("Hi ", &[(outsider.id, "Outsider")]),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(thread["comments"][0]["mentioned_user_ids"], json!([]));
    assert_eq!(fixture.inbox(&member.cookie).await.len(), 1);
    assert_eq!(
        fixture
            .count("SELECT COUNT(*) FROM notifications WHERE kind = 'page_comment_mentioned'")
            .await,
        2
    );

    // The entry hides while the page is out of the member's reach and comes back.
    let version = fixture.page_version(&page).await;
    let (status, _) = fixture
        .call(
            &owner,
            "POST",
            &format!("{}/{page}/move", fixture.pages_uri()),
            Some(json!({"expected_version": version, "parent_id": null, "private": true, "position": 0})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(fixture.inbox(&member.cookie).await.is_empty());
    let version = fixture.page_version(&page).await;
    let (status, _) = fixture
        .call(
            &owner,
            "POST",
            &format!("{}/{page}/move", fixture.pages_uri()),
            Some(json!({"expected_version": version, "parent_id": null, "private": false, "position": 0})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(fixture.inbox(&member.cookie).await.len(), 1);

    // Deleting the comment withdraws its notifications.
    let (status, _) = fixture.call(&owner, "DELETE", &comment_uri, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(fixture.inbox(&member.cookie).await.is_empty());
}

#[tokio::test]
async fn private_and_foreign_pages_answer_like_unknown_ids() {
    let fixture = Fixture::new().await;
    let owner = fixture.owner.clone();
    let member = fixture
        .add_member(&fixture.workspace_id, "member@example.com", "Member")
        .await;
    let private = fixture
        .create_page(&owner, json!({"title": "Secret", "private": true}))
        .await;
    let (_, thread) = fixture.start_thread(&owner, &private, text("mine")).await;
    let thread_uri = format!("{}/{}", fixture.threads_uri(&private), id(&thread));
    let comment_id = id(&thread["comments"][0]);
    let unknown = Id::new_v7().to_string();
    let unknown_uri = format!("{}/{}", fixture.threads_uri(&unknown), id(&thread));

    let attempts: Vec<(&str, String, Option<Value>)> = vec![
        ("GET", fixture.threads_uri(&private), None),
        (
            "POST",
            fixture.threads_uri(&private),
            Some(json!({ "body": text("x") })),
        ),
        (
            "POST",
            format!("{thread_uri}/comments"),
            Some(json!({ "body": text("x") })),
        ),
        ("POST", format!("{thread_uri}/resolve"), None),
        ("POST", format!("{thread_uri}/reopen"), None),
        ("DELETE", thread_uri.clone(), None),
        (
            "PATCH",
            format!("{thread_uri}/comments/{comment_id}"),
            Some(json!({ "body": text("x") })),
        ),
        (
            "DELETE",
            format!("{thread_uri}/comments/{comment_id}"),
            None,
        ),
    ];
    for (method, uri, body) in attempts {
        let (status, problem) = fixture
            .call(&member.cookie, method, &uri, body.clone())
            .await;
        let unknown_target = uri.replace(&thread_uri, &unknown_uri).replace(
            &fixture.threads_uri(&private),
            &fixture.threads_uri(&unknown),
        );
        let (unknown_status, unknown_problem) = fixture
            .call(&member.cookie, method, &unknown_target, body)
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{method} {uri}");
        assert_eq!(problem["code"], "page_not_found");
        assert_eq!(status, unknown_status);
        assert_eq!(problem["code"], unknown_problem["code"]);
        assert_eq!(problem["title"], unknown_problem["title"]);
        assert_eq!(problem["detail"], unknown_problem["detail"]);
    }
    assert_eq!(fixture.count("SELECT COUNT(*) FROM page_comments").await, 1);

    // Another workspace: neither its members nor the page's id through its URL reach the page.
    let other = WorkspaceRepository::new((*fixture.database).clone())
        .create(
            fixture.owner_id,
            "Other".to_owned(),
            "comments-cross",
            TimestampMillis::now(),
        )
        .await
        .unwrap()
        .id;
    let shared = fixture
        .create_page(&owner, json!({"title": "Shared"}))
        .await;
    let cross = fixture
        .threads_uri(&shared)
        .replace(&fixture.workspace_id, &other.to_string());
    let (status, _) = fixture.call(&owner, "GET", &cross, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let outsider = fixture
        .add_member(&other.to_string(), "outsider@example.com", "Outsider")
        .await;
    let (status, _) = fixture
        .call(&outsider.cookie, "GET", &fixture.threads_uri(&shared), None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = fixture
        .start_thread(&outsider.cookie, &shared, text("x"))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // A thread of another page is not reachable through this page.
    let (status, _) = fixture
        .call(
            &owner,
            "POST",
            &format!("{}/{}/resolve", fixture.threads_uri(&shared), id(&thread)),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn trashed_pages_hide_threads_and_purge_removes_them() {
    let fixture = Fixture::new().await;
    let owner = fixture.owner.clone();
    let member = fixture
        .add_member(&fixture.workspace_id, "member@example.com", "Member")
        .await;
    let page = fixture
        .create_page(&owner, json!({"title": "Doomed"}))
        .await;
    let child = fixture
        .create_page(&owner, json!({"title": "Child", "parent_id": page}))
        .await;
    for target in [&page, &child] {
        let (status, _) = fixture
            .start_thread(&owner, target, mentioning("See ", &[(member.id, "Member")]))
            .await;
        assert_eq!(status, StatusCode::CREATED);
    }
    assert_eq!(fixture.inbox(&member.cookie).await.len(), 2);

    let version = fixture.page_version(&page).await;
    let (status, _) = fixture
        .call(
            &owner,
            "DELETE",
            &format!("{}/{page}?expected_version={version}", fixture.pages_uri()),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = fixture
        .call(&owner, "GET", &fixture.threads_uri(&page), None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(fixture.inbox(&member.cookie).await.is_empty());

    let (status, body) = fixture
        .call(
            &owner,
            "POST",
            &format!("{}/trash/empty", fixture.pages_uri()),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    for table in [
        "page_threads",
        "page_comments",
        "page_comment_mentions",
        "notifications",
    ] {
        assert_eq!(
            fixture
                .count(&format!("SELECT COUNT(*) FROM {table}"))
                .await,
            0,
            "{table}"
        );
    }
    assert_eq!(
        fixture
            .count("SELECT COUNT(*) FROM pragma_foreign_key_check")
            .await,
        0
    );
}
