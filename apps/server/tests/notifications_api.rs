use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use orbit_platform::{AuthenticatedUser, Id, PasswordService, TestDatabase, TimestampMillis};
use orbit_server::auth_routes::CookieMode;
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::repositories::tasks::{GithubWorkItem, TaskRepository};
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
    let (_member_id, member_cookie) = add_member(&fixture, "member@example.com").await;
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
            json!({ "body": "please look @member" }),
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
            json!({ "body": "please look @member" }),
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
    // The name is the handle (the email before the `@`), so `@name` names one member.
    let name = email.split('@').next().unwrap();
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
                display_name: name.to_owned(),
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

async fn send(
    fixture: &Fixture,
    method: &str,
    path: &str,
    cookie: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let uri = format!("/api/v1/workspaces/{}{path}", fixture.workspace_id);
    let request = match body {
        Some(body) => json_request(method, &uri, cookie, body),
        None => cookie_request(method, &uri, cookie),
    };
    let response = fixture.app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn id(value: &Value) -> &str {
    value["id"].as_str().unwrap()
}

async fn new_task(fixture: &Fixture, cookie: &str, extra: Value) -> Value {
    let mut body = json!({
        "project_id": fixture.project_id,
        "status_id": fixture.status_id,
        "title": "Work"
    });
    body.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    let (status, task) = send(fixture, "POST", "/tasks", cookie, Some(body)).await;
    assert_eq!(status, StatusCode::CREATED);
    task
}

/// Applies `changes` to the task at its current version.
async fn change_task(fixture: &Fixture, cookie: &str, task_id: &str, mut changes: Value) {
    let path = format!("/tasks/{task_id}");
    let (_, task) = send(fixture, "GET", &path, cookie, None).await;
    changes["expected_version"] = task["version"].clone();
    let (status, body) = send(fixture, "PATCH", &path, cookie, Some(changes)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

/// A comment that mentions the members with these handles.
async fn comment(fixture: &Fixture, cookie: &str, task_id: &str, mentions: &[&str]) -> Value {
    let body: String = mentions
        .iter()
        .map(|handle| format!(" @{handle}"))
        .collect();
    let (status, comment) = send(
        fixture,
        "POST",
        &format!("/tasks/{task_id}/comments"),
        cookie,
        Some(json!({ "body": format!("a comment{body}") })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    comment
}

async fn inbox(fixture: &Fixture, cookie: &str, query: &str) -> Vec<Value> {
    let (status, page) = send(
        fixture,
        "GET",
        &format!("/notifications{query}"),
        cookie,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    page["items"].as_array().unwrap().clone()
}

/// The recipient's row of the task as `kind:read|unread`, or `-` without one in the inbox.
async fn row(fixture: &Fixture, cookie: &str, task_id: &str) -> String {
    let rows: Vec<Value> = inbox(fixture, cookie, "")
        .await
        .into_iter()
        .filter(|item| item["task_id"] == task_id)
        .collect();
    assert!(rows.len() <= 1, "one row for each task: {rows:?}");
    rows.first().map_or("-".to_owned(), |item| {
        let read = if item["read_at"].is_null() {
            "unread"
        } else {
            "read"
        };
        format!("{}:{read}", item["kind"].as_str().unwrap())
    })
}

async fn read_all(fixture: &Fixture, cookie: &str) {
    let (status, _) = send(fixture, "POST", "/notifications/read-all", cookie, None).await;
    assert_eq!(status, StatusCode::OK);
}

async fn subscribe(fixture: &Fixture, cookie: &str, task_id: &str, subscribed: bool) {
    let (status, _) = send(
        fixture,
        "PUT",
        &format!("/tasks/{task_id}/subscription"),
        cookie,
        Some(json!({ "subscribed": subscribed })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

async fn subscribers(fixture: &Fixture, task_id: &str) -> Vec<String> {
    let (status, body) = send(
        fixture,
        "GET",
        &format!("/tasks/{task_id}/subscribers"),
        &fixture.owner_cookie,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let mut ids: Vec<String> = body["user_ids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|id| id.as_str().unwrap().to_owned())
        .collect();
    ids.sort();
    ids
}

fn sorted(ids: &[Id]) -> Vec<String> {
    let mut ids: Vec<String> = ids.iter().map(ToString::to_string).collect();
    ids.sort();
    ids
}

async fn owner_id(fixture: &Fixture) -> Id {
    sqlx::query_scalar::<_, String>(
        "SELECT user_id FROM memberships WHERE role = 'owner' AND workspace_id = ?",
    )
    .bind(&fixture.workspace_id)
    .fetch_one(fixture.database.pool())
    .await
    .unwrap()
    .parse()
    .unwrap()
}

async fn status_of(fixture: &Fixture, category: &str) -> String {
    sqlx::query_scalar(
        "SELECT id FROM task_statuses WHERE project_id = ? AND category = ? ORDER BY position LIMIT 1",
    )
    .bind(&fixture.project_id)
    .bind(category)
    .fetch_one(fixture.database.pool())
    .await
    .unwrap()
}

#[tokio::test]
async fn creator_assignee_comment_author_and_mentioned_person_are_subscribed() {
    let fixture = Fixture::new().await;
    let owner = owner_id(&fixture).await;
    let (member, member_cookie) = add_member(&fixture, "member@example.com").await;
    let (third, third_cookie) = add_member(&fixture, "third@example.com").await;
    let task = new_task(
        &fixture,
        &fixture.owner_cookie,
        json!({ "assignee_ids": [member.to_string()] }),
    )
    .await;
    let task_id = id(&task);
    assert_eq!(
        subscribers(&fixture, task_id).await,
        sorted(&[owner, member])
    );

    comment(&fixture, &third_cookie, task_id, &[]).await;
    assert_eq!(
        subscribers(&fixture, task_id).await,
        sorted(&[owner, member, third])
    );

    // A manual unsubscribe stays after a later comment by that person.
    subscribe(&fixture, &third_cookie, task_id, false).await;
    comment(&fixture, &third_cookie, task_id, &[]).await;
    assert_eq!(
        subscribers(&fixture, task_id).await,
        sorted(&[owner, member])
    );
    // They hear nothing of the task, and a mention subscribes them again.
    comment(&fixture, &fixture.owner_cookie, task_id, &[]).await;
    assert_eq!(row(&fixture, &third_cookie, task_id).await, "-");
    comment(&fixture, &fixture.owner_cookie, task_id, &["third"]).await;
    assert_eq!(
        row(&fixture, &third_cookie, task_id).await,
        "comment_mentioned:unread"
    );
    assert_eq!(
        subscribers(&fixture, task_id).await,
        sorted(&[owner, member, third])
    );

    // So does an assignment.
    subscribe(&fixture, &member_cookie, task_id, false).await;
    change_task(
        &fixture,
        &fixture.owner_cookie,
        task_id,
        json!({ "assignee_ids": [] }),
    )
    .await;
    assert_eq!(
        subscribers(&fixture, task_id).await,
        sorted(&[owner, third])
    );
    change_task(
        &fixture,
        &fixture.owner_cookie,
        task_id,
        json!({ "assignee_ids": [member.to_string()] }),
    )
    .await;
    assert_eq!(
        subscribers(&fixture, task_id).await,
        sorted(&[owner, member, third])
    );

    // Only a person who can read the task can subscribe.
    let (outsider, outsider_cookie) = add_member(&fixture, "outsider@example.com").await;
    sqlx::query("DELETE FROM memberships WHERE user_id = ?")
        .bind(outsider.to_string())
        .execute(fixture.database.pool())
        .await
        .unwrap();
    let (status, _) = send(
        &fixture,
        "PUT",
        &format!("/tasks/{task_id}/subscription"),
        &outsider_cookie,
        Some(json!({ "subscribed": true })),
    )
    .await;
    assert_ne!(status, StatusCode::OK);
}

#[tokio::test]
async fn subscribers_get_comments_and_status_changes_in_one_row_for_each_task() {
    let fixture = Fixture::new().await;
    let (member, member_cookie) = add_member(&fixture, "member@example.com").await;
    let (third, third_cookie) = add_member(&fixture, "third@example.com").await;
    let task = new_task(&fixture, &fixture.owner_cookie, json!({})).await;
    let task_id = id(&task);
    let done = status_of(&fixture, "completed").await;

    // A comment: the other subscribers hear of it, the author does not.
    comment(&fixture, &member_cookie, task_id, &[]).await;
    let rows = inbox(&fixture, &fixture.owner_cookie, "").await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["kind"], "task_commented");
    assert_eq!(rows[0]["actor_user_id"], member.to_string());
    assert_eq!(rows[0]["task_title"], "Work");
    let key: String = sqlx::query_scalar("SELECT project_key FROM projects WHERE id = ?")
        .bind(&fixture.project_id)
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    assert_eq!(
        rows[0]["task_identifier"],
        format!("{key}-{}", task["number"])
    );
    assert!(!rows[0]["comment_id"].is_null());
    assert_eq!(row(&fixture, &member_cookie, task_id).await, "-");

    // A comment with a mention: one row with the mention kind, also for a subscriber.
    comment(
        &fixture,
        &fixture.owner_cookie,
        task_id,
        &["member", "third"],
    )
    .await;
    assert_eq!(
        row(&fixture, &member_cookie, task_id).await,
        "comment_mentioned:unread"
    );
    assert_eq!(
        row(&fixture, &third_cookie, task_id).await,
        "comment_mentioned:unread"
    );

    // An unread mention keeps its kind after a status change.
    change_task(
        &fixture,
        &fixture.owner_cookie,
        task_id,
        json!({ "status_id": done }),
    )
    .await;
    assert_eq!(
        row(&fixture, &member_cookie, task_id).await,
        "comment_mentioned:unread"
    );
    // The actor's own row did not change.
    assert_eq!(
        row(&fixture, &fixture.owner_cookie, task_id).await,
        "task_commented:unread"
    );

    // A read row takes the new event and is unread again (bulk path).
    read_all(&fixture, &member_cookie).await;
    assert_eq!(
        row(&fixture, &member_cookie, task_id).await,
        "comment_mentioned:read"
    );
    let (_, current) = send(
        &fixture,
        "GET",
        &format!("/tasks/{task_id}"),
        &fixture.owner_cookie,
        None,
    )
    .await;
    let (status, body) = send(
        &fixture,
        "POST",
        "/tasks/bulk",
        &fixture.owner_cookie,
        Some(json!({ "updates": [{
            "id": task_id,
            "expected_version": current["version"],
            "status_id": fixture.status_id
        }] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        row(&fixture, &member_cookie, task_id).await,
        "task_status_changed:unread"
    );

    // A subscriber who left the workspace or is suspended is skipped without an error.
    sqlx::query("UPDATE users SET suspended_at = 1 WHERE id = ?")
        .bind(third.to_string())
        .execute(fixture.database.pool())
        .await
        .unwrap();
    sqlx::query("DELETE FROM notifications WHERE recipient_user_id = ?")
        .bind(third.to_string())
        .execute(fixture.database.pool())
        .await
        .unwrap();
    comment(&fixture, &fixture.owner_cookie, task_id, &[]).await;
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM notifications WHERE recipient_user_id = ?"
        )
        .bind(third.to_string())
        .fetch_one(fixture.database.pool())
        .await
        .unwrap(),
        0
    );
}

#[tokio::test]
async fn automation_and_github_status_changes_notify_subscribers() {
    let fixture = Fixture::new().await;
    let owner = owner_id(&fixture).await;
    let (member, member_cookie) = add_member(&fixture, "member@example.com").await;
    let done = status_of(&fixture, "completed").await;

    // The last open sub-issue closes: the parent closes with it.
    let parent = new_task(&fixture, &fixture.owner_cookie, json!({})).await;
    let child = new_task(
        &fixture,
        &member_cookie,
        json!({ "parent_task_id": id(&parent) }),
    )
    .await;
    change_task(
        &fixture,
        &member_cookie,
        id(&child),
        json!({ "status_id": done }),
    )
    .await;
    let rows = inbox(&fixture, &fixture.owner_cookie, "").await;
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["task_id"], parent["id"]);
    assert_eq!(rows[0]["kind"], "task_status_changed");
    assert_eq!(rows[0]["actor_user_id"], member.to_string());

    // GitHub closes an issue: no user is the actor.
    let repository = TaskRepository::new((*fixture.database).clone());
    let sync = |state: &'static str| {
        repository.sync_github_work_item(
            fixture.workspace_id.parse().unwrap(),
            owner,
            GithubWorkItem {
                repository: "acme/repo".to_owned(),
                number: 7,
                project_id: fixture.project_id.parse().unwrap(),
                title: "Synced issue".to_owned(),
                description: String::new(),
                kind: "issue",
                state,
                state_changed: true,
            },
            "github-test",
            TimestampMillis::now(),
        )
    };
    let synced = sync("open").await.unwrap().to_string();
    // A task that GitHub made has no creator to subscribe.
    assert!(subscribers(&fixture, &synced).await.is_empty());
    subscribe(&fixture, &member_cookie, &synced, true).await;
    sync("closed").await.unwrap();
    let rows: Vec<Value> = inbox(&fixture, &member_cookie, "")
        .await
        .into_iter()
        .filter(|item| item["task_id"] == synced.as_str())
        .collect();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["kind"], "task_status_changed");
    assert!(rows[0]["actor_user_id"].is_null());
    assert_eq!(rows[0]["task_title"], "Synced issue");
}

#[tokio::test]
async fn blocked_and_unblocked_are_sent_only_when_the_blocked_state_changes() {
    let fixture = Fixture::new().await;
    let (_, member_cookie) = add_member(&fixture, "member@example.com").await;
    let done = status_of(&fixture, "completed").await;
    let first = new_task(&fixture, &fixture.owner_cookie, json!({})).await;
    let second = new_task(&fixture, &fixture.owner_cookie, json!({})).await;
    let blocked = new_task(&fixture, &fixture.owner_cookie, json!({})).await;
    let blocked_id = id(&blocked);
    subscribe(&fixture, &member_cookie, blocked_id, true).await;
    let block = |blocker: &Value| {
        let path = format!("/tasks/{}/relations", id(blocker));
        let fixture = &fixture;
        async move {
            let body = json!({ "type": "blocks", "task_id": blocked_id });
            send(fixture, "POST", &path, &fixture.owner_cookie, Some(body)).await
        }
    };

    let (status, _) = block(&first).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(
        row(&fixture, &member_cookie, blocked_id).await,
        "task_blocked:unread"
    );
    // The owner made the change: no row for them.
    assert_eq!(row(&fixture, &fixture.owner_cookie, blocked_id).await, "-");

    // A second blocker, and the first one closing, do not change the blocked state.
    read_all(&fixture, &member_cookie).await;
    let (_, relation) = block(&second).await;
    change_task(
        &fixture,
        &fixture.owner_cookie,
        id(&first),
        json!({ "status_id": done }),
    )
    .await;
    assert_eq!(
        row(&fixture, &member_cookie, blocked_id).await,
        "task_blocked:read"
    );

    // The last open blocker closes.
    change_task(
        &fixture,
        &fixture.owner_cookie,
        id(&second),
        json!({ "status_id": done }),
    )
    .await;
    assert_eq!(
        row(&fixture, &member_cookie, blocked_id).await,
        "task_unblocked:unread"
    );
    // A blocker is reopened.
    change_task(
        &fixture,
        &fixture.owner_cookie,
        id(&second),
        json!({ "status_id": fixture.status_id }),
    )
    .await;
    assert_eq!(
        row(&fixture, &member_cookie, blocked_id).await,
        "task_blocked:unread"
    );
    // Its relation is removed.
    let (status, _) = send(
        &fixture,
        "DELETE",
        &format!("/tasks/{}/relations/{}", id(&second), id(&relation)),
        &fixture.owner_cookie,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        row(&fixture, &member_cookie, blocked_id).await,
        "task_unblocked:unread"
    );
}

#[tokio::test]
async fn a_row_can_be_snoozed_archived_and_marked_unread_and_a_new_event_restores_it() {
    let fixture = Fixture::new().await;
    let (member, member_cookie) = add_member(&fixture, "member@example.com").await;
    let task = new_task(
        &fixture,
        &fixture.owner_cookie,
        json!({ "assignee_ids": [member.to_string()] }),
    )
    .await;
    let task_id = id(&task);
    let rows = inbox(&fixture, &member_cookie, "").await;
    let path = format!("/notifications/{}", id(&rows[0]));
    let patch = |cookie: &str, body: Value| {
        let cookie = cookie.to_owned();
        let path = path.clone();
        let fixture = &fixture;
        async move { send(fixture, "PATCH", &path, &cookie, Some(body)).await }
    };
    let counts = || async {
        format!(
            "{}/{}/{}",
            inbox(&fixture, &member_cookie, "?state=inbox").await.len(),
            inbox(&fixture, &member_cookie, "?state=snoozed")
                .await
                .len(),
            inbox(&fixture, &member_cookie, "?state=archived")
                .await
                .len()
        )
    };

    // Only the recipient, and only a time in the future.
    let (status, _) = patch(&fixture.owner_cookie, json!({ "archived": true })).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = patch(
        &member_cookie,
        json!({ "snoozed_until": "2001-01-01T00:00:00Z" }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _) = patch(&member_cookie, json!({ "unknown": true })).await;
    assert!(status.is_client_error());
    assert_eq!(counts().await, "1/0/0");

    let (status, snoozed) = patch(
        &member_cookie,
        json!({ "snoozed_until": "2999-01-01T00:00:00Z" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(!snoozed["snoozed_until"].is_null());
    assert_eq!(counts().await, "0/1/0");
    // The undo.
    patch(&member_cookie, json!({ "snoozed_until": null })).await;
    assert_eq!(counts().await, "1/0/0");

    // A new event brings a snoozed row back at once.
    patch(
        &member_cookie,
        json!({ "snoozed_until": "2999-01-01T00:00:00Z", "read": true }),
    )
    .await;
    comment(&fixture, &fixture.owner_cookie, task_id, &[]).await;
    assert_eq!(counts().await, "1/0/0");
    assert_eq!(
        row(&fixture, &member_cookie, task_id).await,
        "task_commented:unread"
    );

    // Archive; "mark all read" does not reach an archived row.
    let (_, archived) = patch(&member_cookie, json!({ "archived": true })).await;
    assert!(!archived["archived_at"].is_null());
    assert_eq!(counts().await, "0/0/1");
    let (_, read) = send(
        &fixture,
        "POST",
        "/notifications/read-all",
        &member_cookie,
        None,
    )
    .await;
    assert_eq!(read["updated"], 0);
    patch(&member_cookie, json!({ "archived": false })).await;
    assert_eq!(counts().await, "1/0/0");
    // And a new event restores an archived row and makes it unread.
    patch(&member_cookie, json!({ "archived": true, "read": true })).await;
    comment(&fixture, &fixture.owner_cookie, task_id, &[]).await;
    assert_eq!(counts().await, "1/0/0");
    assert_eq!(
        row(&fixture, &member_cookie, task_id).await,
        "task_commented:unread"
    );

    // Read, then unread.
    let (_, read) = patch(&member_cookie, json!({ "read": true })).await;
    assert!(!read["read_at"].is_null());
    let (_, unread) = patch(&member_cookie, json!({ "read": false })).await;
    assert!(unread["read_at"].is_null());
    assert_eq!(
        inbox(&fixture, &member_cookie, "?unread=true").await.len(),
        1
    );
}

#[tokio::test]
async fn the_server_reads_mentions_from_comments_and_descriptions() {
    let fixture = Fixture::new().await;
    let (member, member_cookie) = add_member(&fixture, "member@example.com").await;
    let (_, third_cookie) = add_member(&fixture, "third@example.com").await;

    // A description: the mentioned person gets `task_mentioned` and follows the task.
    let task = new_task(
        &fixture,
        &fixture.owner_cookie,
        json!({ "description": "Plan with @member.\n\n```\n@third in code\n```\nand `@third`" }),
    )
    .await;
    let task_id = id(&task);
    assert_eq!(
        row(&fixture, &member_cookie, task_id).await,
        "task_mentioned:unread"
    );
    // A name in a code block or in inline code is not a mention.
    assert_eq!(row(&fixture, &third_cookie, task_id).await, "-");
    assert!(
        subscribers(&fixture, task_id)
            .await
            .contains(&member.to_string())
    );

    // An edit of the description notifies only the person who is new in it.
    read_all(&fixture, &member_cookie).await;
    change_task(
        &fixture,
        &fixture.owner_cookie,
        task_id,
        json!({ "description": "Plan with @member and @third." }),
    )
    .await;
    assert_eq!(
        row(&fixture, &third_cookie, task_id).await,
        "task_mentioned:unread"
    );
    assert_eq!(
        row(&fixture, &member_cookie, task_id).await,
        "task_mentioned:read"
    );

    // The same for a comment edit. The body has no id list: the server reads the text.
    let other = new_task(&fixture, &fixture.owner_cookie, json!({})).await;
    let other_id = id(&other);
    let created = comment(&fixture, &fixture.owner_cookie, other_id, &["member"]).await;
    assert_eq!(
        row(&fixture, &member_cookie, other_id).await,
        "comment_mentioned:unread"
    );
    read_all(&fixture, &member_cookie).await;
    let path = format!("/tasks/{other_id}/comments/{}", id(&created));
    let (status, _) = send(
        &fixture,
        "PATCH",
        &path,
        &fixture.owner_cookie,
        Some(json!({ "body": "now also @third, and still @member", "expected_version": 0 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        row(&fixture, &third_cookie, other_id).await,
        "comment_mentioned:unread"
    );
    assert_eq!(
        row(&fixture, &member_cookie, other_id).await,
        "comment_mentioned:read"
    );
    // A person does not mention themself, and an unknown name is plain text.
    comment(&fixture, &third_cookie, other_id, &["third", "nobody"]).await;
    assert_eq!(
        row(&fixture, &third_cookie, other_id).await,
        "comment_mentioned:unread"
    );
    // The old field is not accepted: a client cannot name recipients.
    let (status, _) = send(
        &fixture,
        "POST",
        &format!("/tasks/{other_id}/comments"),
        &fixture.owner_cookie,
        Some(json!({ "body": "x", "mentioned_user_ids": [member.to_string()] })),
    )
    .await;
    assert!(status.is_client_error());
}

#[tokio::test]
async fn comment_reactions_are_added_and_removed_for_the_caller() {
    let fixture = Fixture::new().await;
    let owner = owner_id(&fixture).await;
    let (member, member_cookie) = add_member(&fixture, "member@example.com").await;
    let task = new_task(&fixture, &fixture.owner_cookie, json!({})).await;
    let task_id = id(&task);
    let created = comment(&fixture, &fixture.owner_cookie, task_id, &[]).await;
    assert_eq!(created["reactions"], json!([]));
    let react = |method: &'static str, cookie: &str, emoji: &'static str| {
        let path = format!(
            "/tasks/{task_id}/comments/{}/reactions/{emoji}",
            id(&created)
        );
        let cookie = cookie.to_owned();
        let fixture = &fixture;
        async move { send(fixture, method, &path, &cookie, None).await }
    };
    let sequence = || async {
        sqlx::query_scalar::<_, i64>(
            "SELECT sequence FROM realtime_sequences WHERE workspace_id = ?",
        )
        .bind(&fixture.workspace_id)
        .fetch_one(fixture.database.pool())
        .await
        .unwrap()
    };
    let audits = || async {
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM audit_events")
            .fetch_one(fixture.database.pool())
            .await
            .unwrap()
    };
    let (before_sequence, before_audits) = (sequence().await, audits().await);

    // %F0%9F%91%8D is 👍. A repeat changes nothing.
    let (status, record) = react("PUT", &fixture.owner_cookie, "%F0%9F%91%8D").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        record["reactions"],
        json!([{ "emoji": "👍", "count": 1, "reacted": true, "user_ids": [owner.to_string()] }])
    );
    // The other clients hear of it, and there is no audit row and no notification.
    assert_eq!(sequence().await, before_sequence + 1);
    react("PUT", &fixture.owner_cookie, "%F0%9F%91%8D").await;
    assert_eq!(sequence().await, before_sequence + 1);
    assert_eq!(audits().await, before_audits);
    assert_eq!(row(&fixture, &member_cookie, task_id).await, "-");

    react("PUT", &member_cookie, "%F0%9F%91%8D").await;
    let (_, record) = react("PUT", &member_cookie, ":party_parrot:").await;
    assert_eq!(
        record["reactions"],
        json!([
            { "emoji": "👍", "count": 2, "reacted": true, "user_ids": [owner.to_string(), member.to_string()] },
            { "emoji": ":party_parrot:", "count": 1, "reacted": true, "user_ids": [member.to_string()] }
        ])
    );
    // The list of comments has them, with `reacted` for the reader.
    let (_, page) = send(
        &fixture,
        "GET",
        &format!("/tasks/{task_id}/comments"),
        &fixture.owner_cookie,
        None,
    )
    .await;
    assert_eq!(page["items"][0]["reactions"][0]["count"], 2);
    assert_eq!(page["items"][0]["reactions"][1]["reacted"], false);

    // Remove; a repeat changes nothing; only the caller's own reaction goes.
    let (status, record) = react("DELETE", &member_cookie, "%F0%9F%91%8D").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(record["reactions"][0]["count"], 1);
    assert_eq!(record["reactions"][0]["reacted"], false);
    let (status, record) = react("DELETE", &member_cookie, "%F0%9F%91%8D").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        record["reactions"][0]["user_ids"],
        json!([owner.to_string()])
    );

    // Not an emoji, and a person who cannot read the task.
    let (status, _) = react("PUT", &fixture.owner_cookie, "two%20words").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (outsider, outsider_cookie) = add_member(&fixture, "outsider@example.com").await;
    sqlx::query("DELETE FROM memberships WHERE user_id = ?")
        .bind(outsider.to_string())
        .execute(fixture.database.pool())
        .await
        .unwrap();
    let (status, _) = react("PUT", &outsider_cookie, "%F0%9F%91%8D").await;
    assert!(status.is_client_error());
    // A deleted comment takes its reactions along.
    let (status, _) = send(
        &fixture,
        "DELETE",
        &format!(
            "/tasks/{task_id}/comments/{}?expected_version=0",
            id(&created)
        ),
        &fixture.owner_cookie,
        None,
    )
    .await;
    assert!(status.is_success(), "{status}");
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM task_comment_reactions")
            .fetch_one(fixture.database.pool())
            .await
            .unwrap(),
        0
    );
}
