use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use orbit_platform::{AuthenticatedUser, Id, PasswordService, TestDatabase, TimestampMillis};
use orbit_server::auth_routes::CookieMode;
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::task_routes::{TaskState, task_router};
use serde_json::{Value, json};
use tower::ServiceExt;

struct Fixture {
    database: TestDatabase,
    identity: Arc<IdentityRepository>,
    app: axum::Router,
    workspace_id: String,
    project_id: String,
    status_id: String,
    owner_cookie: String,
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
        let status_id: String = sqlx::query_scalar(
            "SELECT id FROM task_statuses WHERE project_id = ? ORDER BY position LIMIT 1",
        )
        .bind(setup.project_id.to_string())
        .fetch_one(database.pool())
        .await
        .unwrap();
        let app = task_router(TaskState::new(Arc::clone(&identity), CookieMode::secure()));
        Self {
            database,
            identity,
            app,
            workspace_id: setup.workspace_id.to_string(),
            project_id: setup.project_id.to_string(),
            status_id,
            owner_cookie: format!("__Host-orbit_session={}", setup.session.token),
        }
    }
}

#[tokio::test]
async fn assigning_a_member_creates_one_notification_and_retries_do_not_duplicate() {
    let fixture = Fixture::new().await;
    let (member_id, member_cookie) = add_member(&fixture, "member@example.com").await;
    let created = fixture
        .app
        .clone()
        .oneshot(json_request(
            "POST",
            &format!("/api/v1/workspaces/{}/tasks", fixture.workspace_id),
            &fixture.owner_cookie,
            json!({
                "project_id": fixture.project_id,
                "status_id": fixture.status_id,
                "title": "Assigned work",
                "assignee_ids": [member_id.to_string()]
            }),
        ))
        .await
        .unwrap();
    assert_eq!(created.status(), StatusCode::CREATED);
    let task = response_json(created).await;

    let inbox = list_notifications(&fixture, &member_cookie, false).await;
    assert_eq!(inbox["items"].as_array().unwrap().len(), 1);
    assert_eq!(inbox["items"][0]["kind"], "task_assigned");
    assert_eq!(inbox["items"][0]["task_id"], task["id"]);
    assert!(inbox["items"][0]["read_at"].is_null());

    let owner_inbox = list_notifications(&fixture, &fixture.owner_cookie, false).await;
    assert_eq!(owner_inbox["items"].as_array().unwrap().len(), 0);

    let retry = fixture
        .app
        .clone()
        .oneshot(json_request(
            "PATCH",
            &format!(
                "/api/v1/workspaces/{}/tasks/{}",
                fixture.workspace_id,
                task["id"].as_str().unwrap()
            ),
            &fixture.owner_cookie,
            json!({
                "expected_version": task["version"],
                "assignee_ids": [member_id.to_string()]
            }),
        ))
        .await
        .unwrap();
    assert_eq!(retry.status(), StatusCode::OK);
    let inbox = list_notifications(&fixture, &member_cookie, false).await;
    assert_eq!(inbox["items"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn mentions_notify_named_members_and_rolled_back_writes_do_not() {
    let fixture = Fixture::new().await;
    let (member_id, member_cookie) = add_member(&fixture, "member@example.com").await;
    let task = create_task(&fixture).await;

    sqlx::query("CREATE TRIGGER reject_comment_audit BEFORE INSERT ON audit_events WHEN NEW.action = 'comment.created' BEGIN SELECT RAISE(ABORT, 'audit unavailable'); END")
        .execute(fixture.database.pool())
        .await
        .unwrap();
    let failed = fixture
        .app
        .clone()
        .oneshot(json_request(
            "POST",
            &format!(
                "/api/v1/workspaces/{}/tasks/{}/comments",
                fixture.workspace_id,
                task["id"].as_str().unwrap()
            ),
            &fixture.owner_cookie,
            json!({
                "body_json": doc("please look @member"),
                "mentioned_user_ids": [member_id.to_string()]
            }),
        ))
        .await
        .unwrap();
    assert_eq!(failed.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let inbox = list_notifications(&fixture, &member_cookie, false).await;
    assert_eq!(inbox["items"].as_array().unwrap().len(), 0);
    sqlx::query("DROP TRIGGER reject_comment_audit")
        .execute(fixture.database.pool())
        .await
        .unwrap();

    let comment = fixture
        .app
        .clone()
        .oneshot(json_request(
            "POST",
            &format!(
                "/api/v1/workspaces/{}/tasks/{}/comments",
                fixture.workspace_id,
                task["id"].as_str().unwrap()
            ),
            &fixture.owner_cookie,
            json!({
                "body_json": doc("please look @member"),
                "mentioned_user_ids": [member_id.to_string()]
            }),
        ))
        .await
        .unwrap();
    assert_eq!(comment.status(), StatusCode::CREATED);
    let inbox = list_notifications(&fixture, &member_cookie, false).await;
    assert_eq!(inbox["items"].as_array().unwrap().len(), 1);
    assert_eq!(inbox["items"][0]["kind"], "comment_mentioned");
}

#[tokio::test]
async fn read_state_is_isolated_and_removed_members_cannot_read_notifications() {
    let fixture = Fixture::new().await;
    let (member_id, member_cookie) = add_member(&fixture, "member@example.com").await;
    let (_other_id, other_cookie) = add_member(&fixture, "other@example.com").await;
    let created = fixture
        .app
        .clone()
        .oneshot(json_request(
            "POST",
            &format!("/api/v1/workspaces/{}/tasks", fixture.workspace_id),
            &fixture.owner_cookie,
            json!({
                "project_id": fixture.project_id,
                "status_id": fixture.status_id,
                "title": "Shared",
                "assignee_ids": [member_id.to_string()]
            }),
        ))
        .await
        .unwrap();
    let task = response_json(created).await;
    let inbox = list_notifications(&fixture, &member_cookie, true).await;
    let notification_id = inbox["items"][0]["id"].as_str().unwrap().to_owned();

    let other_read = fixture
        .app
        .clone()
        .oneshot(json_request(
            "POST",
            &format!(
                "/api/v1/workspaces/{}/notifications/{notification_id}/read",
                fixture.workspace_id
            ),
            &other_cookie,
            json!({}),
        ))
        .await
        .unwrap();
    assert_eq!(other_read.status(), StatusCode::NOT_FOUND);

    let marked = fixture
        .app
        .clone()
        .oneshot(json_request(
            "POST",
            &format!(
                "/api/v1/workspaces/{}/notifications/{notification_id}/read",
                fixture.workspace_id
            ),
            &member_cookie,
            json!({}),
        ))
        .await
        .unwrap();
    assert_eq!(marked.status(), StatusCode::OK);
    assert!(!response_json(marked).await["read_at"].is_null());
    let unread = list_notifications(&fixture, &member_cookie, true).await;
    assert_eq!(unread["items"].as_array().unwrap().len(), 0);

    sqlx::query("DELETE FROM memberships WHERE user_id = ? AND workspace_id = ?")
        .bind(member_id.to_string())
        .bind(&fixture.workspace_id)
        .execute(fixture.database.pool())
        .await
        .unwrap();
    let forbidden = fixture
        .app
        .clone()
        .oneshot(cookie_request(
            "GET",
            &format!("/api/v1/workspaces/{}/notifications", fixture.workspace_id),
            &member_cookie,
        ))
        .await
        .unwrap();
    assert_eq!(forbidden.status(), StatusCode::NOT_FOUND);
    let _ = task;
}

async fn create_task(fixture: &Fixture) -> Value {
    let response = fixture
        .app
        .clone()
        .oneshot(json_request(
            "POST",
            &format!("/api/v1/workspaces/{}/tasks", fixture.workspace_id),
            &fixture.owner_cookie,
            json!({
                "project_id": fixture.project_id,
                "status_id": fixture.status_id,
                "title": "Work"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    response_json(response).await
}

async fn list_notifications(fixture: &Fixture, cookie: &str, unread: bool) -> Value {
    let unread_query = if unread { "?unread=true" } else { "" };
    let response = fixture
        .app
        .clone()
        .oneshot(cookie_request(
            "GET",
            &format!(
                "/api/v1/workspaces/{}/notifications{unread_query}",
                fixture.workspace_id
            ),
            cookie,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    response_json(response).await
}

async fn add_member(fixture: &Fixture, email: &str) -> (Id, String) {
    let id = Id::new_v7();
    let membership_id = Id::new_v7();
    let now = TimestampMillis::now();
    sqlx::query(
        "INSERT INTO users (id, email, normalized_email, display_name, password_hash, created_at, updated_at) \
         VALUES (?, ?, ?, 'Member', 'unused', ?, ?)",
    )
    .bind(id.to_string())
    .bind(email)
    .bind(email)
    .bind(now.as_millis())
    .bind(now.as_millis())
    .execute(fixture.database.pool())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, updated_at) \
         VALUES (?, ?, ?, 'member', 0, ?, ?)",
    )
    .bind(membership_id.to_string())
    .bind(&fixture.workspace_id)
    .bind(id.to_string())
    .bind(now.as_millis())
    .bind(now.as_millis())
    .execute(fixture.database.pool())
    .await
    .unwrap();
    let session = fixture
        .identity
        .create_session(
            &AuthenticatedUser {
                id,
                email: email.to_owned(),
                display_name: "Member".to_owned(),
            },
            now,
        )
        .await
        .unwrap();
    (id, format!("__Host-orbit_session={}", session.token))
}

fn json_request(method: &str, uri: &str, cookie: &str, value: Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::COOKIE, cookie)
        .body(Body::from(value.to_string()))
        .unwrap()
}

fn cookie_request(method: &str, uri: &str, cookie: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::COOKIE, cookie)
        .body(Body::empty())
        .unwrap()
}

async fn response_json(response: axum::response::Response) -> Value {
    serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap()
}

/// A minimal TipTap document, so tests can post prose without hand-writing JSON.
fn doc(text: &str) -> serde_json::Value {
    json!({ "type": "doc", "content": [
        { "type": "paragraph", "content": [{ "type": "text", "text": text }] }
    ] })
}
