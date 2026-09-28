use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use orbit_platform::{AuthenticatedUser, Id, PasswordService, TestDatabase, TimestampMillis};
use orbit_server::auth_routes::CookieMode;
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::repositories::task_filter::{
    FilterGroup, ShowCompleted, SubIssuesDisplay, parse_filter, preset_filter,
};
use orbit_server::repositories::tasks::{
    CreateTask, GithubWorkItem, SortOrder, TaskError, TaskFilter, TaskRepository, TaskSort,
};
use orbit_server::repositories::workspaces::WorkspaceRepository;
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
    owner_id: Id,
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
            owner_id: setup.user_id,
        }
    }

    async fn create_task(&self, title: &str) -> Value {
        let response = self
            .app
            .clone()
            .oneshot(json_request(
                "POST",
                &format!("/api/v1/workspaces/{}/tasks", self.workspace_id),
                &self.owner_cookie,
                json!({
                    "project_id": self.project_id,
                    "status_id": self.status_id,
                    "title": title
                }),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        response_json(response).await
    }
}

#[tokio::test]
async fn github_issue_link_reports_paused_sync_state() {
    let fixture = Fixture::new().await;
    let task = fixture.create_task("GitHub task").await;
    let task_id = task["id"].as_str().unwrap();
    sqlx::query("INSERT INTO github_issue_links (workspace_id, repository, issue_number, task_id, sync_paused) VALUES (?, 'acme/repo', 12, ?, 1)")
        .bind(&fixture.workspace_id)
        .bind(task_id)
        .execute(fixture.database.pool())
        .await
        .unwrap();
    let path = format!(
        "/api/v1/workspaces/{}/tasks/{task_id}/github-links",
        fixture.workspace_id
    );
    let response = fixture
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&path)
                .header(header::COOKIE, &fixture.owner_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let link = response_json(response).await;
    assert_eq!(link[0]["state"], "paused");
    assert_eq!(link[0]["source"], true);

    sqlx::query("UPDATE github_issue_links SET sync_paused = 0 WHERE task_id = ?")
        .bind(task_id)
        .execute(fixture.database.pool())
        .await
        .unwrap();
    let response = fixture
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&path)
                .header(header::COOKIE, &fixture.owner_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response_json(response).await[0]["state"], "active");
}

#[tokio::test]
async fn github_pull_task_reports_its_source_and_state() {
    let fixture = Fixture::new().await;
    let task = fixture.create_task("Pull request task").await;
    let task_id = task["id"].as_str().unwrap();
    sqlx::query("INSERT INTO github_issue_links (workspace_id, repository, issue_number, task_id, kind, pull_state) VALUES (?, 'acme/repo', 8, ?, 'pull_request', 'merged')")
        .bind(&fixture.workspace_id).bind(task_id).execute(fixture.database.pool()).await.unwrap();
    let path = format!(
        "/api/v1/workspaces/{}/tasks/{task_id}/github-links",
        fixture.workspace_id
    );
    let response = fixture
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&path)
                .header(header::COOKIE, &fixture.owner_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let links = response_json(response).await;
    assert_eq!(links[0]["kind"], "pull_request");
    assert_eq!(links[0]["url"], "https://github.com/acme/repo/pull/8");
    assert_eq!(links[0]["state"], "merged");
    assert_eq!(links[0]["source"], true);
    let edit = fixture
        .app
        .clone()
        .oneshot(json_request(
            "PATCH",
            &format!(
                "/api/v1/workspaces/{}/tasks/{task_id}",
                fixture.workspace_id
            ),
            &fixture.owner_cookie,
            json!({"expected_version":0,"title":"Orbit edit"}),
        ))
        .await
        .unwrap();
    assert_eq!(edit.status(), StatusCode::CONFLICT);
    assert_eq!(
        response_json(edit).await["code"],
        "github_content_read_only"
    );
}

#[tokio::test]
async fn task_due_date_can_be_created_changed_and_cleared_with_version_checks() {
    let fixture = Fixture::new().await;
    let due_start_at = "2029-12-30T00:00:00Z";
    let due_at = "2030-01-02T12:30:00Z";
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
                "title": "Ship release",
                "due_start_at": due_start_at,
                "due_at": due_at
            }),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let task = response_json(response).await;
    assert_eq!(task["due_start_at"], "2029-12-30T00:00:00.000Z");
    assert_eq!(task["due_at"], "2030-01-02T12:30:00.000Z");

    let uri = format!(
        "/api/v1/workspaces/{}/tasks/{}",
        fixture.workspace_id,
        task["id"].as_str().unwrap()
    );
    let response = fixture
        .app
        .clone()
        .oneshot(json_request(
            "PATCH",
            &uri,
            &fixture.owner_cookie,
            json!({"expected_version": 0, "due_at": null}),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let cleared = response_json(response).await;
    assert!(cleared["due_start_at"].is_null());
    assert!(cleared["due_at"].is_null());
    assert_eq!(cleared["version"], 1);

    let stale = fixture
        .app
        .clone()
        .oneshot(json_request(
            "PATCH",
            &uri,
            &fixture.owner_cookie,
            json!({"expected_version": 0, "due_at": due_at}),
        ))
        .await
        .unwrap();
    assert_eq!(stale.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn task_source_url_can_be_created_changed_and_cleared() {
    let fixture = Fixture::new().await;
    let tasks_uri = format!("/api/v1/workspaces/{}/tasks", fixture.workspace_id);
    let created = fixture
        .app
        .clone()
        .oneshot(json_request(
            "POST",
            &tasks_uri,
            &fixture.owner_cookie,
            json!({"project_id": fixture.project_id, "status_id": fixture.status_id,
            "title": "Linked task", "description": "Only the details",
            "source_url": "https://example.com/issues/1"}),
        ))
        .await
        .unwrap();
    assert_eq!(created.status(), StatusCode::CREATED);
    let task = response_json(created).await;
    assert_eq!(task["description"], "Only the details");
    assert_eq!(task["source_url"], "https://example.com/issues/1");

    let uri = format!("{tasks_uri}/{}", task["id"].as_str().unwrap());
    let changed = fixture
        .app
        .clone()
        .oneshot(json_request(
            "PATCH",
            &uri,
            &fixture.owner_cookie,
            json!({"expected_version": 0, "source_url": "https://example.com/issues/2"}),
        ))
        .await
        .unwrap();
    assert_eq!(changed.status(), StatusCode::OK);
    assert_eq!(
        response_json(changed).await["source_url"],
        "https://example.com/issues/2"
    );

    let invalid = fixture
        .app
        .clone()
        .oneshot(json_request(
            "PATCH",
            &uri,
            &fixture.owner_cookie,
            json!({"expected_version": 1, "source_url": "javascript:alert(1)"}),
        ))
        .await
        .unwrap();
    assert_eq!(invalid.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let cleared = fixture
        .app
        .clone()
        .oneshot(json_request(
            "PATCH",
            &uri,
            &fixture.owner_cookie,
            json!({"expected_version": 1, "source_url": null}),
        ))
        .await
        .unwrap();
    assert_eq!(cleared.status(), StatusCode::OK);
    assert!(response_json(cleared).await["source_url"].is_null());
}

#[tokio::test]
async fn task_activity_is_task_scoped_and_paginated() {
    let fixture = Fixture::new().await;
    let first = fixture.create_task("First task").await;
    let second = fixture.create_task("Second task").await;
    let response = fixture
        .app
        .clone()
        .oneshot(cookie_request(
            "GET",
            &format!(
                "/api/v1/workspaces/{}/tasks/{}/activity?limit=1",
                fixture.workspace_id,
                first["id"].as_str().unwrap()
            ),
            &fixture.owner_cookie,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let page = response_json(response).await;
    assert_eq!(page["items"].as_array().unwrap().len(), 1);
    assert_eq!(page["items"][0]["resource_id"], first["id"]);
    assert_ne!(page["items"][0]["resource_id"], second["id"]);
}

#[tokio::test]
async fn project_creation_is_atomic_and_statuses_are_project_scoped() {
    let fixture = Fixture::new().await;
    let response = fixture
        .app
        .clone()
        .oneshot(json_request(
            "POST",
            &format!("/api/v1/workspaces/{}/projects", fixture.workspace_id),
            &fixture.owner_cookie,
            json!({"name":"Product", "key":"PROD", "color":"#123456"}),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let project = response_json(response).await;
    let project_id = project["id"].as_str().unwrap();

    let statuses = fixture
        .app
        .clone()
        .oneshot(cookie_request(
            "GET",
            &format!(
                "/api/v1/workspaces/{}/projects/{project_id}/statuses",
                fixture.workspace_id
            ),
            &fixture.owner_cookie,
        ))
        .await
        .unwrap();
    assert_eq!(statuses.status(), StatusCode::OK);
    let statuses = response_json(statuses).await;
    assert_eq!(statuses["items"].as_array().unwrap().len(), 6);
    assert_eq!(statuses["items"][5]["name"], "Duplicate");
    assert_eq!(statuses["items"][5]["category"], "duplicate");
    assert_eq!(statuses["items"][0]["name"], "Backlog");
    assert!(
        statuses["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|status| status["project_id"] == project_id
                && status["workspace_id"] == fixture.workspace_id)
    );

    let duplicate = fixture
        .app
        .clone()
        .oneshot(json_request(
            "POST",
            &format!("/api/v1/workspaces/{}/projects", fixture.workspace_id),
            &fixture.owner_cookie,
            json!({"name":"Duplicate", "key":"PROD", "color":"#ffffff"}),
        ))
        .await
        .unwrap();
    assert_eq!(duplicate.status(), StatusCode::CONFLICT);
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM projects WHERE workspace_id = ? AND project_key = 'PROD'",
    )
    .bind(&fixture.workspace_id)
    .fetch_one(fixture.database.pool())
    .await
    .unwrap();
    assert_eq!(count, 1, "a failed project transaction leaves no defaults");
}

#[tokio::test]
async fn members_can_crud_all_task_area_resources_and_unknown_fields_are_rejected() {
    let fixture = Fixture::new().await;
    let (member_id, member_cookie) = add_member(&fixture, "member@example.com").await;

    let project = fixture
        .app
        .clone()
        .oneshot(json_request(
            "POST",
            &format!("/api/v1/workspaces/{}/projects", fixture.workspace_id),
            &member_cookie,
            json!({"name":"Member project", "key":"MEM", "color":"#abcdef"}),
        ))
        .await
        .unwrap();
    assert_eq!(project.status(), StatusCode::CREATED);
    let project = response_json(project).await;

    let label = fixture
        .app
        .clone()
        .oneshot(json_request(
            "POST",
            &format!("/api/v1/workspaces/{}/labels", fixture.workspace_id),
            &member_cookie,
            json!({"name":"bug", "color":"#ff0000"}),
        ))
        .await
        .unwrap();
    assert_eq!(label.status(), StatusCode::CREATED);
    let label = response_json(label).await;

    let task = fixture
        .app
        .clone()
        .oneshot(json_request(
            "POST",
            &format!("/api/v1/workspaces/{}/tasks", fixture.workspace_id),
            &member_cookie,
            json!({
                "project_id": fixture.project_id,
                "status_id": fixture.status_id,
                "title": "Member task",
                "assignee_ids": [member_id],
                "label_ids": [label["id"]]
            }),
        ))
        .await
        .unwrap();
    assert_eq!(task.status(), StatusCode::CREATED);
    let task = response_json(task).await;

    let changed = fixture
        .app
        .clone()
        .oneshot(json_request(
            "PATCH",
            &format!(
                "/api/v1/workspaces/{}/tasks/{}",
                fixture.workspace_id,
                task["id"].as_str().unwrap()
            ),
            &member_cookie,
            json!({"title":"Changed", "expected_version":0}),
        ))
        .await
        .unwrap();
    assert_eq!(changed.status(), StatusCode::OK);

    let unknown = fixture
        .app
        .clone()
        .oneshot(json_request(
            "PATCH",
            &format!(
                "/api/v1/workspaces/{}/tasks/{}",
                fixture.workspace_id,
                task["id"].as_str().unwrap()
            ),
            &member_cookie,
            json!({"expected_version":1, "bogus":true}),
        ))
        .await
        .unwrap();
    assert_eq!(unknown.status(), StatusCode::BAD_REQUEST);

    let comment_uri = format!(
        "/api/v1/workspaces/{}/tasks/{}/comments",
        fixture.workspace_id,
        task["id"].as_str().unwrap()
    );
    let comment = fixture
        .app
        .clone()
        .oneshot(json_request(
            "POST",
            &comment_uri,
            &member_cookie,
            json!({"body":"Member comment"}),
        ))
        .await
        .unwrap();
    assert_eq!(comment.status(), StatusCode::CREATED);
    let comment = response_json(comment).await;
    let deleted_comment = fixture
        .app
        .clone()
        .oneshot(cookie_request(
            "DELETE",
            &format!(
                "{comment_uri}/{}?expected_version=0",
                comment["id"].as_str().unwrap()
            ),
            &member_cookie,
        ))
        .await
        .unwrap();
    assert_eq!(deleted_comment.status(), StatusCode::NO_CONTENT);

    let deleted = fixture
        .app
        .clone()
        .oneshot(cookie_request(
            "DELETE",
            &format!(
                "/api/v1/workspaces/{}/tasks/{}?expected_version=1",
                fixture.workspace_id,
                task["id"].as_str().unwrap()
            ),
            &member_cookie,
        ))
        .await
        .unwrap();
    assert_eq!(deleted.status(), StatusCode::NO_CONTENT);

    let deleted_label = fixture
        .app
        .clone()
        .oneshot(cookie_request(
            "DELETE",
            &format!(
                "/api/v1/workspaces/{}/labels/{}?expected_version=0",
                fixture.workspace_id,
                label["id"].as_str().unwrap()
            ),
            &member_cookie,
        ))
        .await
        .unwrap();
    assert_eq!(deleted_label.status(), StatusCode::NO_CONTENT);

    let project_id = project["id"].as_str().unwrap();
    let statuses = response_json(
        fixture
            .app
            .clone()
            .oneshot(cookie_request(
                "GET",
                &format!(
                    "/api/v1/workspaces/{}/projects/{project_id}/statuses",
                    fixture.workspace_id
                ),
                &member_cookie,
            ))
            .await
            .unwrap(),
    )
    .await;
    let status = statuses["items"]
        .as_array()
        .unwrap()
        .iter()
        .rfind(|status| status["category"] != "duplicate")
        .unwrap();
    let deleted_status = fixture
        .app
        .clone()
        .oneshot(cookie_request(
            "DELETE",
            &format!(
                "/api/v1/workspaces/{}/projects/{project_id}/statuses/{}?expected_version=0",
                fixture.workspace_id,
                status["id"].as_str().unwrap()
            ),
            &member_cookie,
        ))
        .await
        .unwrap();
    assert_eq!(deleted_status.status(), StatusCode::NO_CONTENT);
    let deleted_project = fixture
        .app
        .oneshot(cookie_request(
            "DELETE",
            &format!(
                "/api/v1/workspaces/{}/projects/{project_id}?expected_version=0",
                fixture.workspace_id
            ),
            &member_cookie,
        ))
        .await
        .unwrap();
    assert_eq!(deleted_project.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn task_lists_filter_sort_and_reject_cursor_query_mismatches() {
    let fixture = Fixture::new().await;
    for (title, priority) in [("Zulu", "low"), ("Alpha", "urgent"), ("Beta", "urgent")] {
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
                    "title": title,
                    "priority": priority
                }),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
    }

    let priorities = fixture
        .app
        .clone()
        .oneshot(cookie_request(
            "GET",
            &format!(
                "/api/v1/workspaces/{}/tasks?sort=priority&order=asc&limit=2",
                fixture.workspace_id
            ),
            &fixture.owner_cookie,
        ))
        .await
        .unwrap();
    assert_eq!(priorities.status(), StatusCode::OK);
    let priorities = response_json(priorities).await;
    assert_eq!(priorities["items"][0]["priority"], "urgent");
    assert_eq!(priorities["items"][1]["priority"], "urgent");
    let priorities_next = fixture
        .app
        .clone()
        .oneshot(cookie_request(
            "GET",
            &format!(
                "/api/v1/workspaces/{}/tasks?sort=priority&order=asc&limit=2&cursor={}",
                fixture.workspace_id,
                priorities["next_cursor"].as_str().unwrap()
            ),
            &fixture.owner_cookie,
        ))
        .await
        .unwrap();
    assert_eq!(
        response_json(priorities_next).await["items"][0]["priority"],
        "low"
    );

    let first = fixture
        .app
        .clone()
        .oneshot(cookie_request(
            "GET",
            &format!(
                "/api/v1/workspaces/{}/tasks?priority=urgent&sort=title&order=asc&limit=1",
                fixture.workspace_id
            ),
            &fixture.owner_cookie,
        ))
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::OK);
    let first = response_json(first).await;
    assert_eq!(first["items"][0]["title"], "Alpha");
    let cursor = first["next_cursor"].as_str().unwrap();
    assert_ne!(cursor, first["items"][0]["id"]);

    let second = fixture
        .app
        .clone()
        .oneshot(cookie_request(
            "GET",
            &format!(
                "/api/v1/workspaces/{}/tasks?priority=urgent&sort=title&order=asc&limit=1&cursor={cursor}",
                fixture.workspace_id
            ),
            &fixture.owner_cookie,
        ))
        .await
        .unwrap();
    assert_eq!(response_json(second).await["items"][0]["title"], "Beta");

    let mismatch = fixture
        .app
        .clone()
        .oneshot(cookie_request(
            "GET",
            &format!(
                "/api/v1/workspaces/{}/tasks?priority=low&sort=title&order=asc&cursor={cursor}",
                fixture.workspace_id
            ),
            &fixture.owner_cookie,
        ))
        .await
        .unwrap();
    assert_eq!(mismatch.status(), StatusCode::BAD_REQUEST);
    assert_eq!(response_json(mismatch).await["code"], "invalid_cursor");

    let foreign_workspace = create_workspace(&fixture, "Cursor scope").await;
    let wrong_workspace = fixture
        .app
        .clone()
        .oneshot(cookie_request(
            "GET",
            &format!(
                "/api/v1/workspaces/{foreign_workspace}/tasks?priority=urgent&sort=title&order=asc&cursor={cursor}"
            ),
            &fixture.owner_cookie,
        ))
        .await
        .unwrap();
    assert_eq!(wrong_workspace.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        response_json(wrong_workspace).await["code"],
        "invalid_cursor"
    );

    let searched = fixture
        .app
        .oneshot(cookie_request(
            "GET",
            &format!(
                "/api/v1/workspaces/{}/tasks?search=alp",
                fixture.workspace_id
            ),
            &fixture.owner_cookie,
        ))
        .await
        .unwrap();
    assert_eq!(searched.status(), StatusCode::OK);
    let searched = response_json(searched).await;
    assert_eq!(searched["items"].as_array().unwrap().len(), 1);
    assert_eq!(searched["items"][0]["title"], "Alpha");
}

#[tokio::test]
async fn title_sort_ignores_case_across_pages() {
    let fixture = Fixture::new().await;
    for title in ["Zebra", "apple", "Banana"] {
        fixture.create_task(title).await;
    }
    let mut titles = Vec::new();
    let mut cursor = String::new();
    loop {
        let page = response_json(
            fixture
                .app
                .clone()
                .oneshot(cookie_request(
                    "GET",
                    &format!(
                        "/api/v1/workspaces/{}/tasks?sort=title&order=asc&limit=1{cursor}",
                        fixture.workspace_id
                    ),
                    &fixture.owner_cookie,
                ))
                .await
                .unwrap(),
        )
        .await;
        titles.extend(
            page["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|task| task["title"].clone()),
        );
        match page["next_cursor"].as_str() {
            Some(next) => cursor = format!("&cursor={next}"),
            None => break,
        }
    }
    assert_eq!(titles, [json!("apple"), json!("Banana"), json!("Zebra")]);
}

#[tokio::test]
async fn descending_title_cursors_handle_prefixes() {
    let fixture = Fixture::new().await;
    fixture.create_task("A").await;
    fixture.create_task("AA").await;
    let first = response_json(
        fixture
            .app
            .clone()
            .oneshot(cookie_request(
                "GET",
                &format!(
                    "/api/v1/workspaces/{}/tasks?sort=title&order=desc&limit=1",
                    fixture.workspace_id
                ),
                &fixture.owner_cookie,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(first["items"][0]["title"], "AA");
    let second = response_json(
        fixture
            .app
            .oneshot(cookie_request(
                "GET",
                &format!(
                    "/api/v1/workspaces/{}/tasks?sort=title&order=desc&limit=1&cursor={}",
                    fixture.workspace_id,
                    first["next_cursor"].as_str().unwrap()
                ),
                &fixture.owner_cookie,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(second["items"][0]["title"], "A");
}

#[tokio::test]
async fn stale_versions_return_current_safe_records() {
    let fixture = Fixture::new().await;
    let task = fixture.create_task("Original").await;
    let uri = format!(
        "/api/v1/workspaces/{}/tasks/{}",
        fixture.workspace_id,
        task["id"].as_str().unwrap()
    );
    let first = fixture
        .app
        .clone()
        .oneshot(json_request(
            "PATCH",
            &uri,
            &fixture.owner_cookie,
            json!({"title":"First", "expected_version":0}),
        ))
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::OK);

    let stale = fixture
        .app
        .oneshot(json_request(
            "PATCH",
            &uri,
            &fixture.owner_cookie,
            json!({"title":"Lost", "expected_version":0}),
        ))
        .await
        .unwrap();
    assert_eq!(stale.status(), StatusCode::CONFLICT);
    let stale = response_json(stale).await;
    assert_eq!(stale["code"], "conflict");
    assert_eq!(stale["conflict"]["current_version"], 1);
    assert_eq!(stale["conflict"]["current"]["title"], "First");
    assert!(stale["conflict"]["current"].get("description").is_some());
}

#[tokio::test]
async fn assignment_requires_active_membership_and_foreign_ids_are_hidden() {
    let fixture = Fixture::new().await;
    let (member_id, _) = add_member(&fixture, "assignee@example.com").await;
    sqlx::query("DELETE FROM memberships WHERE workspace_id = ? AND user_id = ?")
        .bind(&fixture.workspace_id)
        .bind(member_id.to_string())
        .execute(fixture.database.pool())
        .await
        .unwrap();
    let invalid = fixture
        .app
        .clone()
        .oneshot(json_request(
            "POST",
            &format!("/api/v1/workspaces/{}/tasks", fixture.workspace_id),
            &fixture.owner_cookie,
            json!({
                "project_id": fixture.project_id,
                "status_id": fixture.status_id,
                "title": "Invalid assignment",
                "assignee_ids": [member_id]
            }),
        ))
        .await
        .unwrap();
    assert_eq!(invalid.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let foreign = create_workspace(&fixture, "Foreign").await;
    let foreign_project: String = sqlx::query_scalar(
        "SELECT id FROM projects WHERE workspace_id = ? AND deleted_at IS NULL LIMIT 1",
    )
    .bind(&foreign)
    .fetch_one(fixture.database.pool())
    .await
    .unwrap();
    let foreign_status: String = sqlx::query_scalar(
        "SELECT id FROM task_statuses WHERE project_id = ? ORDER BY position LIMIT 1",
    )
    .bind(&foreign_project)
    .fetch_one(fixture.database.pool())
    .await
    .unwrap();
    let crossed = fixture
        .app
        .oneshot(json_request(
            "POST",
            &format!("/api/v1/workspaces/{}/tasks", fixture.workspace_id),
            &fixture.owner_cookie,
            json!({
                "project_id": foreign_project,
                "status_id": foreign_status,
                "title": "Crossed"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(crossed.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn removing_a_membership_removes_its_task_assignments() {
    let fixture = Fixture::new().await;
    let (member_id, _) = add_member(&fixture, "departing@example.com").await;
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
                "title": "Assigned",
                "assignee_ids": [member_id]
            }),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let task = response_json(response).await;
    sqlx::query("DELETE FROM memberships WHERE workspace_id = ? AND user_id = ?")
        .bind(&fixture.workspace_id)
        .bind(member_id.to_string())
        .execute(fixture.database.pool())
        .await
        .unwrap();
    let assignments: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM task_assignees WHERE task_id = ?")
            .bind(task["id"].as_str().unwrap())
            .fetch_one(fixture.database.pool())
            .await
            .unwrap();
    assert_eq!(assignments, 0);
}

#[tokio::test]
async fn github_issue_task_text_is_read_only_but_other_fields_can_change() {
    let fixture = Fixture::new().await;
    let github_task = fixture.create_task("GitHub title").await;
    let ordinary_task = fixture.create_task("Orbit title").await;
    let github_id = github_task["id"].as_str().unwrap();
    sqlx::query("INSERT INTO github_issue_links (workspace_id, repository, issue_number, task_id) VALUES (?, ?, ?, ?)")
        .bind(&fixture.workspace_id)
        .bind("owner/repo")
        .bind(7)
        .bind(github_id)
        .execute(fixture.database.pool())
        .await
        .unwrap();

    for body in [
        json!({"expected_version":0,"title":"Orbit edit"}),
        json!({"expected_version":0,"description":"Orbit edit"}),
    ] {
        let response = fixture
            .app
            .clone()
            .oneshot(json_request(
                "PATCH",
                &format!(
                    "/api/v1/workspaces/{}/tasks/{github_id}",
                    fixture.workspace_id
                ),
                &fixture.owner_cookie,
                body,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert_eq!(
            response_json(response).await["code"],
            "github_content_read_only"
        );
    }

    let bulk = fixture
        .app
        .clone()
        .oneshot(json_request(
            "POST",
            &format!("/api/v1/workspaces/{}/tasks/bulk", fixture.workspace_id),
            &fixture.owner_cookie,
            json!({"updates":[
                {"id":ordinary_task["id"],"expected_version":0,"priority":"high"},
                {"id":github_id,"expected_version":0,"title":"Orbit edit"}
            ]}),
        ))
        .await
        .unwrap();
    assert_eq!(bulk.status(), StatusCode::CONFLICT);
    assert_eq!(
        response_json(bulk).await["code"],
        "github_content_read_only"
    );

    let ordinary_priority: String = sqlx::query_scalar("SELECT priority FROM tasks WHERE id = ?")
        .bind(ordinary_task["id"].as_str().unwrap())
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    assert_eq!(ordinary_priority, "none", "bulk update must roll back");

    let changed = fixture
        .app
        .clone()
        .oneshot(json_request(
            "PATCH",
            &format!(
                "/api/v1/workspaces/{}/tasks/{github_id}",
                fixture.workspace_id
            ),
            &fixture.owner_cookie,
            json!({"expected_version":0,"priority":"high"}),
        ))
        .await
        .unwrap();
    assert_eq!(changed.status(), StatusCode::OK);
    let changed = response_json(changed).await;
    assert_eq!(changed["priority"], "high");
    assert_eq!(changed["title"], "GitHub title");

    sqlx::query("INSERT INTO github_pull_links (workspace_id, repository, pull_number, task_id, title, url, state, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)")
        .bind(&fixture.workspace_id)
        .bind("owner/repo")
        .bind(8)
        .bind(ordinary_task["id"].as_str().unwrap())
        .bind("Pull request")
        .bind("https://github.com/owner/repo/pull/8")
        .bind("open")
        .bind(TimestampMillis::now().as_millis())
        .execute(fixture.database.pool())
        .await
        .unwrap();

    let ordinary = fixture
        .app
        .clone()
        .oneshot(json_request(
            "PATCH",
            &format!(
                "/api/v1/workspaces/{}/tasks/{}",
                fixture.workspace_id,
                ordinary_task["id"].as_str().unwrap()
            ),
            &fixture.owner_cookie,
            json!({"expected_version":0,"title":"Orbit edit"}),
        ))
        .await
        .unwrap();
    assert_eq!(ordinary.status(), StatusCode::OK);
}

#[tokio::test]
async fn bulk_updates_and_reorders_are_atomic() {
    let fixture = Fixture::new().await;
    let first = fixture.create_task("First").await;
    let second = fixture.create_task("Second").await;
    let failed = fixture
        .app
        .clone()
        .oneshot(json_request(
            "POST",
            &format!("/api/v1/workspaces/{}/tasks/bulk", fixture.workspace_id),
            &fixture.owner_cookie,
            json!({"updates":[
                {"id":first["id"], "expected_version":0, "priority":"high"},
                {"id":second["id"], "expected_version":99, "priority":"urgent"}
            ]}),
        ))
        .await
        .unwrap();
    assert_eq!(failed.status(), StatusCode::CONFLICT);
    let priority: String = sqlx::query_scalar("SELECT priority FROM tasks WHERE id = ?")
        .bind(first["id"].as_str().unwrap())
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    assert_eq!(priority, "none", "the first update must roll back");

    let reordered = fixture
        .app
        .oneshot(json_request(
            "POST",
            &format!("/api/v1/workspaces/{}/tasks/reorder", fixture.workspace_id),
            &fixture.owner_cookie,
            json!({"items":[
                {"id":second["id"], "expected_version":0, "position":10},
                {"id":first["id"], "expected_version":0, "position":20}
            ]}),
        ))
        .await
        .unwrap();
    assert_eq!(reordered.status(), StatusCode::OK);
    let reordered = response_json(reordered).await;
    assert_eq!(reordered["items"][0]["position"], 10);
    assert_eq!(reordered["items"][1]["position"], 20);
}

#[tokio::test]
async fn project_deletion_hides_children_and_trash_expires_after_thirty_days() {
    let fixture = Fixture::new().await;
    let task = fixture.create_task("Hidden child").await;
    let deleted = fixture
        .app
        .clone()
        .oneshot(cookie_request(
            "DELETE",
            &format!(
                "/api/v1/workspaces/{}/projects/{}?expected_version=0",
                fixture.workspace_id, fixture.project_id
            ),
            &fixture.owner_cookie,
        ))
        .await
        .unwrap();
    assert_eq!(deleted.status(), StatusCode::NO_CONTENT);

    let hidden = fixture
        .app
        .clone()
        .oneshot(cookie_request(
            "GET",
            &format!(
                "/api/v1/workspaces/{}/tasks/{}",
                fixture.workspace_id,
                task["id"].as_str().unwrap()
            ),
            &fixture.owner_cookie,
        ))
        .await
        .unwrap();
    assert_eq!(hidden.status(), StatusCode::NOT_FOUND);

    let trash = fixture
        .app
        .clone()
        .oneshot(cookie_request(
            "GET",
            &format!("/api/v1/workspaces/{}/projects/trash", fixture.workspace_id),
            &fixture.owner_cookie,
        ))
        .await
        .unwrap();
    assert_eq!(
        response_json(trash).await["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    sqlx::query("UPDATE projects SET deleted_at = ? WHERE id = ?")
        .bind(TimestampMillis::now().as_millis() - 30 * 24 * 60 * 60 * 1_000)
        .bind(&fixture.project_id)
        .execute(fixture.database.pool())
        .await
        .unwrap();
    let expired = fixture
        .app
        .clone()
        .oneshot(cookie_request(
            "GET",
            &format!("/api/v1/workspaces/{}/projects/trash", fixture.workspace_id),
            &fixture.owner_cookie,
        ))
        .await
        .unwrap();
    assert!(
        response_json(expired).await["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let restore = fixture
        .app
        .oneshot(json_request(
            "POST",
            &format!(
                "/api/v1/workspaces/{}/projects/{}/restore",
                fixture.workspace_id, fixture.project_id
            ),
            &fixture.owner_cookie,
            json!({"expected_version":1}),
        ))
        .await
        .unwrap();
    assert_eq!(restore.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn durable_retention_purges_expired_projects_and_tasks() {
    let fixture = Fixture::new().await;
    let task = fixture.create_task("Purge child").await;
    sqlx::query("UPDATE projects SET deleted_at = ? WHERE id = ?")
        .bind(TimestampMillis::now().as_millis() - 31 * 24 * 60 * 60 * 1_000)
        .bind(&fixture.project_id)
        .execute(fixture.database.pool())
        .await
        .unwrap();
    WorkspaceRepository::new((*fixture.database).clone())
        .purge_retention(TimestampMillis::now())
        .await
        .unwrap();
    let projects: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM projects WHERE id = ?")
        .bind(&fixture.project_id)
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    let tasks: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tasks WHERE id = ?")
        .bind(task["id"].as_str().unwrap())
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    assert_eq!((projects, tasks), (0, 0));
}

#[tokio::test]
async fn workspace_retention_cascades_through_task_records() {
    let fixture = Fixture::new().await;
    fixture.create_task("Workspace child").await;
    sqlx::query("UPDATE workspaces SET deleted_at = ? WHERE id = ?")
        .bind(TimestampMillis::now().as_millis() - 31 * 24 * 60 * 60 * 1_000)
        .bind(&fixture.workspace_id)
        .execute(fixture.database.pool())
        .await
        .unwrap();
    WorkspaceRepository::new((*fixture.database).clone())
        .purge_retention(TimestampMillis::now())
        .await
        .unwrap();
    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tasks WHERE workspace_id = ?")
        .bind(&fixture.workspace_id)
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    assert_eq!(rows, 0);
}

#[tokio::test]
async fn project_restore_reports_key_conflicts_without_renaming() {
    let fixture = Fixture::new().await;
    let deleted = fixture
        .app
        .clone()
        .oneshot(cookie_request(
            "DELETE",
            &format!(
                "/api/v1/workspaces/{}/projects/{}?expected_version=0",
                fixture.workspace_id, fixture.project_id
            ),
            &fixture.owner_cookie,
        ))
        .await
        .unwrap();
    assert_eq!(deleted.status(), StatusCode::NO_CONTENT);
    let replacement = fixture
        .app
        .clone()
        .oneshot(json_request(
            "POST",
            &format!("/api/v1/workspaces/{}/projects", fixture.workspace_id),
            &fixture.owner_cookie,
            json!({"name":"Replacement", "key":"GEN", "color":"#000000"}),
        ))
        .await
        .unwrap();
    assert_eq!(replacement.status(), StatusCode::CREATED);

    let restore = fixture
        .app
        .oneshot(json_request(
            "POST",
            &format!(
                "/api/v1/workspaces/{}/projects/{}/restore",
                fixture.workspace_id, fixture.project_id
            ),
            &fixture.owner_cookie,
            json!({"expected_version":1}),
        ))
        .await
        .unwrap();
    assert_eq!(restore.status(), StatusCode::CONFLICT);
    let restore = response_json(restore).await;
    assert_eq!(restore["code"], "restore_conflict");
    assert_eq!(restore["conflict"]["field"], "key");
}

#[tokio::test]
async fn deleted_tasks_can_be_listed_and_restored_within_retention() {
    let fixture = Fixture::new().await;
    let task = fixture.create_task("Recover me").await;
    let task_id = task["id"].as_str().unwrap();
    let deleted = fixture
        .app
        .clone()
        .oneshot(cookie_request(
            "DELETE",
            &format!(
                "/api/v1/workspaces/{}/tasks/{task_id}?expected_version=0",
                fixture.workspace_id
            ),
            &fixture.owner_cookie,
        ))
        .await
        .unwrap();
    assert_eq!(deleted.status(), StatusCode::NO_CONTENT);
    let trash = response_json(
        fixture
            .app
            .clone()
            .oneshot(cookie_request(
                "GET",
                &format!("/api/v1/workspaces/{}/tasks/trash", fixture.workspace_id),
                &fixture.owner_cookie,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(trash["items"][0]["id"], task_id);
    let restored = fixture
        .app
        .clone()
        .oneshot(json_request(
            "POST",
            &format!(
                "/api/v1/workspaces/{}/tasks/{task_id}/restore",
                fixture.workspace_id
            ),
            &fixture.owner_cookie,
            json!({"expected_version":1}),
        ))
        .await
        .unwrap();
    assert_eq!(restored.status(), StatusCode::OK);
    let restored = response_json(restored).await;
    assert_eq!(restored["deleted_at"], Value::Null);
    assert_eq!(restored["version"], 2);
}

#[tokio::test]
async fn comments_are_scoped_versioned_and_hard_deleted() {
    let fixture = Fixture::new().await;
    let task = fixture.create_task("Discuss").await;
    let uri = format!(
        "/api/v1/workspaces/{}/tasks/{}/comments",
        fixture.workspace_id,
        task["id"].as_str().unwrap()
    );
    let comment = fixture
        .app
        .clone()
        .oneshot(json_request(
            "POST",
            &uri,
            &fixture.owner_cookie,
            json!({"body":"First"}),
        ))
        .await
        .unwrap();
    assert_eq!(comment.status(), StatusCode::CREATED);
    let comment = response_json(comment).await;
    let comment_uri = format!("{uri}/{}", comment["id"].as_str().unwrap());
    let updated = fixture
        .app
        .clone()
        .oneshot(json_request(
            "PATCH",
            &comment_uri,
            &fixture.owner_cookie,
            json!({"body":"Edited", "expected_version":0}),
        ))
        .await
        .unwrap();
    assert_eq!(response_json(updated).await["version"], 1);

    let deleted = fixture
        .app
        .oneshot(cookie_request(
            "DELETE",
            &format!("{comment_uri}?expected_version=1"),
            &fixture.owner_cookie,
        ))
        .await
        .unwrap();
    assert_eq!(deleted.status(), StatusCode::NO_CONTENT);
    let exists: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM task_comments WHERE id = ?")
        .bind(comment["id"].as_str().unwrap())
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    assert_eq!(exists, 0);
}

#[tokio::test]
async fn parent_side_scope_changes_are_rejected_by_the_database() {
    let fixture = Fixture::new().await;
    let foreign_workspace = create_workspace(&fixture, "Foreign scope").await;
    let foreign_project: String = sqlx::query_scalar(
        "SELECT id FROM projects WHERE workspace_id = ? AND deleted_at IS NULL LIMIT 1",
    )
    .bind(&foreign_workspace)
    .fetch_one(fixture.database.pool())
    .await
    .unwrap();
    let foreign_status: String = sqlx::query_scalar(
        "SELECT id FROM task_statuses WHERE project_id = ? ORDER BY position LIMIT 1",
    )
    .bind(&foreign_project)
    .fetch_one(fixture.database.pool())
    .await
    .unwrap();
    let (member_id, _) = add_member(&fixture, "scope-member@example.com").await;
    let membership_id: String =
        sqlx::query_scalar("SELECT id FROM memberships WHERE workspace_id = ? AND user_id = ?")
            .bind(&fixture.workspace_id)
            .bind(member_id.to_string())
            .fetch_one(fixture.database.pool())
            .await
            .unwrap();
    let label = response_json(
        fixture
            .app
            .clone()
            .oneshot(json_request(
                "POST",
                &format!("/api/v1/workspaces/{}/labels", fixture.workspace_id),
                &fixture.owner_cookie,
                json!({"name":"scope", "color":"#112233"}),
            ))
            .await
            .unwrap(),
    )
    .await;
    let task = response_json(
        fixture
            .app
            .clone()
            .oneshot(json_request(
                "POST",
                &format!("/api/v1/workspaces/{}/tasks", fixture.workspace_id),
                &fixture.owner_cookie,
                json!({
                    "project_id": fixture.project_id,
                    "status_id": fixture.status_id,
                    "title": "Scoped",
                    "assignee_ids": [member_id],
                    "label_ids": [label["id"]]
                }),
            ))
            .await
            .unwrap(),
    )
    .await;
    let second_task = fixture.create_task("Second parent").await;
    let parent = response_json(
        fixture
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
                json!({"body":"parent"}),
            ))
            .await
            .unwrap(),
    )
    .await;
    fixture
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
            json!({"body":"reply", "parent_id":parent["id"]}),
        ))
        .await
        .unwrap();

    for (sql, first, second) in [
        (
            "UPDATE projects SET workspace_id = ? WHERE id = ?",
            foreign_workspace.as_str(),
            fixture.project_id.as_str(),
        ),
        (
            "UPDATE task_statuses SET workspace_id = ?, project_id = ? WHERE id = ?",
            foreign_workspace.as_str(),
            foreign_project.as_str(),
        ),
    ] {
        let result = if sql.contains("task_statuses") {
            sqlx::query(sql)
                .bind(first)
                .bind(second)
                .bind(&fixture.status_id)
                .execute(fixture.database.pool())
                .await
        } else {
            sqlx::query(sql)
                .bind(first)
                .bind(second)
                .execute(fixture.database.pool())
                .await
        };
        assert!(result.is_err(), "parent-side scope update must be rejected");
    }
    assert!(
        sqlx::query(
            "UPDATE tasks SET workspace_id = ?, project_id = ?, status_id = ? WHERE id = ?"
        )
        .bind(&foreign_workspace)
        .bind(&foreign_project)
        .bind(&foreign_status)
        .bind(task["id"].as_str().unwrap())
        .execute(fixture.database.pool())
        .await
        .is_err()
    );
    assert!(
        sqlx::query("UPDATE labels SET workspace_id = ? WHERE id = ?")
            .bind(&foreign_workspace)
            .bind(label["id"].as_str().unwrap())
            .execute(fixture.database.pool())
            .await
            .is_err()
    );
    assert!(
        sqlx::query("UPDATE memberships SET workspace_id = ? WHERE id = ?")
            .bind(&foreign_workspace)
            .bind(&membership_id)
            .execute(fixture.database.pool())
            .await
            .is_err()
    );
    assert!(
        sqlx::query("UPDATE task_comments SET task_id = ? WHERE id = ?")
            .bind(second_task["id"].as_str().unwrap())
            .bind(parent["id"].as_str().unwrap())
            .execute(fixture.database.pool())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn suspended_assignees_are_removed_from_reads_and_filters() {
    let fixture = Fixture::new().await;
    let (member_id, _) = add_member(&fixture, "suspended-assignee@example.com").await;
    let task = response_json(
        fixture
            .app
            .clone()
            .oneshot(json_request(
                "POST",
                &format!("/api/v1/workspaces/{}/tasks", fixture.workspace_id),
                &fixture.owner_cookie,
                json!({
                    "project_id": fixture.project_id,
                    "status_id": fixture.status_id,
                    "title": "Assigned before suspension",
                    "assignee_ids": [member_id]
                }),
            ))
            .await
            .unwrap(),
    )
    .await;
    sqlx::query("UPDATE users SET suspended_at = ? WHERE id = ?")
        .bind(TimestampMillis::now().as_millis())
        .bind(member_id.to_string())
        .execute(fixture.database.pool())
        .await
        .unwrap();

    let stored: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM task_assignees WHERE task_id = ?")
        .bind(task["id"].as_str().unwrap())
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    assert_eq!(stored, 0);

    sqlx::query("UPDATE users SET suspended_at = NULL WHERE id = ?")
        .bind(member_id.to_string())
        .execute(fixture.database.pool())
        .await
        .unwrap();
    let membership_id: String =
        sqlx::query_scalar("SELECT id FROM memberships WHERE workspace_id = ? AND user_id = ?")
            .bind(&fixture.workspace_id)
            .bind(member_id.to_string())
            .fetch_one(fixture.database.pool())
            .await
            .unwrap();
    sqlx::query("INSERT INTO task_assignees (task_id, membership_id, user_id) VALUES (?, ?, ?)")
        .bind(task["id"].as_str().unwrap())
        .bind(membership_id)
        .bind(member_id.to_string())
        .execute(fixture.database.pool())
        .await
        .unwrap();
    sqlx::query("DROP TRIGGER users_remove_task_assignments_on_suspend")
        .execute(fixture.database.pool())
        .await
        .unwrap();
    sqlx::query("UPDATE users SET suspended_at = ? WHERE id = ?")
        .bind(TimestampMillis::now().as_millis())
        .bind(member_id.to_string())
        .execute(fixture.database.pool())
        .await
        .unwrap();
    let legacy_stored: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM task_assignees WHERE task_id = ?")
            .bind(task["id"].as_str().unwrap())
            .fetch_one(fixture.database.pool())
            .await
            .unwrap();
    assert_eq!(
        legacy_stored, 1,
        "simulate a pre-migration stale assignment"
    );

    let read = response_json(
        fixture
            .app
            .clone()
            .oneshot(cookie_request(
                "GET",
                &format!(
                    "/api/v1/workspaces/{}/tasks/{}",
                    fixture.workspace_id,
                    task["id"].as_str().unwrap()
                ),
                &fixture.owner_cookie,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(read["assignee_ids"], json!([]));
    let filtered = response_json(
        fixture
            .app
            .clone()
            .oneshot(cookie_request(
                "GET",
                &format!(
                    "/api/v1/workspaces/{}/tasks?assignee_id={member_id}",
                    fixture.workspace_id
                ),
                &fixture.owner_cookie,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(filtered["items"], json!([]));
    let unassigned = response_json(
        fixture
            .app
            .clone()
            .oneshot(cookie_request(
                "GET",
                &format!(
                    "/api/v1/workspaces/{}/tasks?unassigned=true",
                    fixture.workspace_id
                ),
                &fixture.owner_cookie,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(unassigned["items"][0]["id"], task["id"]);
}

#[tokio::test]
async fn patch_preserves_status_description_and_text_limits_preserve_source() {
    let fixture = Fixture::new().await;
    let status_uri = format!(
        "/api/v1/workspaces/{}/projects/{}/statuses",
        fixture.workspace_id, fixture.project_id
    );
    let status = response_json(
        fixture
            .app
            .clone()
            .oneshot(json_request(
                "POST",
                &status_uri,
                &fixture.owner_cookie,
                json!({
                    "name":"Review",
                    "description":"  keep status source  ",
                    "color":"#445566",
                    "category":"started"
                }),
            ))
            .await
            .unwrap(),
    )
    .await;
    let updated = fixture
        .app
        .clone()
        .oneshot(json_request(
            "PATCH",
            &format!("{status_uri}/{}", status["id"].as_str().unwrap()),
            &fixture.owner_cookie,
            json!({
                "name":"Reviewing",
                "color":"#445566",
                "category":"started",
                "position":status["position"],
                "expected_version":0
            }),
        ))
        .await
        .unwrap();
    assert_eq!(updated.status(), StatusCode::OK);
    let updated = response_json(updated).await;
    assert_eq!(updated["description"], "  keep status source  ");
    let cleared = fixture
        .app
        .clone()
        .oneshot(json_request(
            "PATCH",
            &format!("{status_uri}/{}", status["id"].as_str().unwrap()),
            &fixture.owner_cookie,
            json!({
                "name":"Reviewing",
                "description":"",
                "color":"#445566",
                "category":"started",
                "position":status["position"],
                "expected_version":1
            }),
        ))
        .await
        .unwrap();
    assert_eq!(cleared.status(), StatusCode::OK);
    assert_eq!(response_json(cleared).await["description"], "");

    let task = fixture.create_task("Text source").await;
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
            json!({"body":"  exact comment source  \n"}),
        ))
        .await
        .unwrap();
    assert_eq!(comment.status(), StatusCode::CREATED);
    assert_eq!(
        response_json(comment).await["body"],
        "  exact comment source  \n"
    );

    let oversized_bytes = "😀".repeat(300);
    let rejected = fixture
        .app
        .clone()
        .oneshot(json_request(
            "POST",
            &format!("/api/v1/workspaces/{}/tasks", fixture.workspace_id),
            &fixture.owner_cookie,
            json!({
                "project_id": fixture.project_id,
                "status_id": fixture.status_id,
                "title": oversized_bytes
            }),
        ))
        .await
        .unwrap();
    assert_eq!(rejected.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn status_patch_rejects_null_description_without_mutation_or_audit() {
    let fixture = Fixture::new().await;
    let statuses_uri = format!(
        "/api/v1/workspaces/{}/projects/{}/statuses",
        fixture.workspace_id, fixture.project_id
    );
    let status = response_json(
        fixture
            .app
            .clone()
            .oneshot(json_request(
                "POST",
                &statuses_uri,
                &fixture.owner_cookie,
                json!({
                    "name":"Null contract",
                    "description":"retained",
                    "color":"#445566",
                    "category":"started"
                }),
            ))
            .await
            .unwrap(),
    )
    .await;
    let status_id = status["id"].as_str().unwrap();

    let rejected = fixture
        .app
        .clone()
        .oneshot(json_request(
            "PATCH",
            &format!("{statuses_uri}/{status_id}"),
            &fixture.owner_cookie,
            json!({
                "name":"Changed despite null",
                "description":null,
                "color":"#445566",
                "category":"started",
                "position":status["position"],
                "expected_version":0
            }),
        ))
        .await
        .unwrap();
    assert_eq!(rejected.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(response_json(rejected).await["code"], "validation_failed");

    let statuses = response_json(
        fixture
            .app
            .clone()
            .oneshot(cookie_request("GET", &statuses_uri, &fixture.owner_cookie))
            .await
            .unwrap(),
    )
    .await;
    let unchanged = statuses["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == status_id)
        .unwrap();
    assert_eq!(unchanged["name"], "Null contract");
    assert_eq!(unchanged["description"], "retained");
    assert_eq!(unchanged["version"], 0);
    let updates: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events WHERE action = 'status.updated' AND resource_id = ?",
    )
    .bind(status_id)
    .fetch_one(fixture.database.pool())
    .await
    .unwrap();
    assert_eq!(updates, 0);
}

#[tokio::test]
async fn generic_cursors_reject_wrong_key_arity_and_types() {
    let fixture = Fixture::new().await;
    let projects_uri = format!("/api/v1/workspaces/{}/projects", fixture.workspace_id);
    for key in [json!([]), json!(["name", fixture.project_id, "extra"])] {
        let cursor = cursor_hex(json!({
            "version":1,
            "fingerprint":format!("projects:{}", fixture.workspace_id),
            "key":key
        }));
        let response = fixture
            .app
            .clone()
            .oneshot(cookie_request(
                "GET",
                &format!("{projects_uri}?cursor={cursor}"),
                &fixture.owner_cookie,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(response_json(response).await["code"], "invalid_cursor");
    }
    let cursor = cursor_hex(json!({
        "version":1,
        "fingerprint":format!("statuses:{}", fixture.project_id),
        "key":["not-a-position", fixture.status_id]
    }));
    let response = fixture
        .app
        .clone()
        .oneshot(cookie_request(
            "GET",
            &format!(
                "/api/v1/workspaces/{}/projects/{}/statuses?cursor={cursor}",
                fixture.workspace_id, fixture.project_id
            ),
            &fixture.owner_cookie,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(response_json(response).await["code"], "invalid_cursor");
}

#[tokio::test]
async fn duplicate_batches_are_rejected_and_conflicts_link_to_readable_resources() {
    let fixture = Fixture::new().await;
    let first = fixture.create_task("Batch first").await;
    let task_id = first["id"].as_str().unwrap();
    let bulk_uri = format!("/api/v1/workspaces/{}/tasks/bulk", fixture.workspace_id);
    let duplicate_bulk = fixture
        .app
        .clone()
        .oneshot(json_request(
            "POST",
            &bulk_uri,
            &fixture.owner_cookie,
            json!({"updates":[
                {"id":task_id, "expected_version":0, "priority":"high"},
                {"id":task_id, "expected_version":1, "priority":"low"}
            ]}),
        ))
        .await
        .unwrap();
    assert_eq!(duplicate_bulk.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let task_reorder_uri = format!("/api/v1/workspaces/{}/tasks/reorder", fixture.workspace_id);
    let duplicate_task_reorder = fixture
        .app
        .clone()
        .oneshot(json_request(
            "POST",
            &task_reorder_uri,
            &fixture.owner_cookie,
            json!({"items":[
                {"id":task_id, "expected_version":0, "position":1},
                {"id":task_id, "expected_version":1, "position":2}
            ]}),
        ))
        .await
        .unwrap();
    assert_eq!(
        duplicate_task_reorder.status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );

    let status_reorder_uri = format!(
        "/api/v1/workspaces/{}/projects/{}/statuses/reorder",
        fixture.workspace_id, fixture.project_id
    );
    let duplicate_status_reorder = fixture
        .app
        .clone()
        .oneshot(json_request(
            "POST",
            &status_reorder_uri,
            &fixture.owner_cookie,
            json!({"items":[
                {"id":fixture.status_id, "expected_version":0, "position":1},
                {"id":fixture.status_id, "expected_version":1, "position":2}
            ]}),
        ))
        .await
        .unwrap();
    assert_eq!(
        duplicate_status_reorder.status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );

    let changed = fixture
        .app
        .clone()
        .oneshot(json_request(
            "PATCH",
            &format!(
                "/api/v1/workspaces/{}/tasks/{task_id}",
                fixture.workspace_id
            ),
            &fixture.owner_cookie,
            json!({"priority":"urgent", "expected_version":0}),
        ))
        .await
        .unwrap();
    assert_eq!(changed.status(), StatusCode::OK);
    let stale_bulk = fixture
        .app
        .clone()
        .oneshot(json_request(
            "POST",
            &bulk_uri,
            &fixture.owner_cookie,
            json!({"updates":[{"id":task_id, "expected_version":0, "priority":"low"}]}),
        ))
        .await
        .unwrap();
    assert_eq!(stale_bulk.status(), StatusCode::CONFLICT);
    let conflict = response_json(stale_bulk).await;
    let refresh = conflict["conflict"]["refresh"].as_str().unwrap();
    assert!(refresh.ends_with(&format!("/tasks/{task_id}")));
    let refreshed = fixture
        .app
        .clone()
        .oneshot(cookie_request("GET", refresh, &fixture.owner_cookie))
        .await
        .unwrap();
    assert_eq!(refreshed.status(), StatusCode::OK);

    let stale_reorder = fixture
        .app
        .clone()
        .oneshot(json_request(
            "POST",
            &task_reorder_uri,
            &fixture.owner_cookie,
            json!({"items":[{"id":task_id, "expected_version":0, "position":10}]}),
        ))
        .await
        .unwrap();
    let conflict = response_json(stale_reorder).await;
    let refresh = conflict["conflict"]["refresh"].as_str().unwrap();
    assert!(refresh.ends_with(&format!("/tasks/{task_id}")));
}

#[tokio::test]
async fn saved_task_views_use_the_authenticated_user_and_utc_calendar() {
    let fixture = Fixture::new().await;
    let (member_id, member_cookie) = add_member(&fixture, "assignee@example.com").await;
    let past = TimestampMillis::from_millis(TimestampMillis::now().as_millis() - 3 * 86_400_000);
    let soon = TimestampMillis::from_millis(TimestampMillis::now().as_millis() + 2 * 86_400_000);
    let day_ms = 86_400_000;
    let start_of_utc_day = (TimestampMillis::now().as_millis() / day_ms) * day_ms;
    let days_since_epoch = start_of_utc_day / day_ms;
    let monday = start_of_utc_day - (days_since_epoch + 3).rem_euclid(7) * day_ms;
    let this_week = TimestampMillis::from_millis(monday + day_ms / 2);
    let next_week = TimestampMillis::from_millis(monday + 7 * day_ms + day_ms / 2);
    for (title, assignee, due) in [
        ("Mine overdue", Some(member_id), Some(past)),
        ("Mine soon", Some(member_id), Some(soon)),
        ("Theirs overdue", None, Some(past)),
        ("This calendar week", None, Some(this_week)),
        ("Mine this week", Some(member_id), Some(this_week)),
        ("Next calendar week", None, Some(next_week)),
        ("Mine next week", Some(member_id), Some(next_week)),
        ("No date", Some(member_id), None),
    ] {
        let mut body = json!({
            "project_id": fixture.project_id,
            "status_id": fixture.status_id,
            "title": title,
        });
        if let Some(id) = assignee {
            body["assignee_ids"] = json!([id.to_string()]);
        }
        if let Some(due_at) = due {
            body["due_at"] = json!(due_at);
        }
        let response = fixture
            .app
            .clone()
            .oneshot(json_request(
                "POST",
                &format!("/api/v1/workspaces/{}/tasks", fixture.workspace_id),
                &fixture.owner_cookie,
                body,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
    }

    let mine = response_json(
        fixture
            .app
            .clone()
            .oneshot(cookie_request(
                "GET",
                &format!(
                    "/api/v1/workspaces/{}/tasks?view=mine",
                    fixture.workspace_id
                ),
                &member_cookie,
            ))
            .await
            .unwrap(),
    )
    .await;
    let mine_titles: Vec<_> = mine["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|task| task["title"].as_str().unwrap().to_owned())
        .collect();
    assert!(mine_titles.contains(&"Mine overdue".to_owned()));
    assert!(mine_titles.contains(&"Mine soon".to_owned()));
    assert!(!mine_titles.contains(&"Theirs overdue".to_owned()));

    let overdue = response_json(
        fixture
            .app
            .clone()
            .oneshot(cookie_request(
                "GET",
                &format!(
                    "/api/v1/workspaces/{}/tasks?view=overdue",
                    fixture.workspace_id
                ),
                &member_cookie,
            ))
            .await
            .unwrap(),
    )
    .await;
    let overdue_titles: Vec<_> = overdue["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|task| task["title"].as_str().unwrap().to_owned())
        .collect();
    assert!(overdue_titles.contains(&"Mine overdue".to_owned()));
    assert!(overdue_titles.contains(&"Theirs overdue".to_owned()));
    assert!(!overdue_titles.contains(&"Mine soon".to_owned()));
    assert!(!overdue_titles.contains(&"No date".to_owned()));

    let soon_view = response_json(
        fixture
            .app
            .clone()
            .oneshot(cookie_request(
                "GET",
                &format!(
                    "/api/v1/workspaces/{}/tasks?view=due_soon",
                    fixture.workspace_id
                ),
                &member_cookie,
            ))
            .await
            .unwrap(),
    )
    .await;
    let soon_titles: Vec<_> = soon_view["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|task| task["title"].as_str().unwrap().to_owned())
        .collect();
    assert!(soon_titles.contains(&"Mine soon".to_owned()));
    assert!(!soon_titles.contains(&"Mine overdue".to_owned()));

    let current_week = response_json(
        fixture
            .app
            .clone()
            .oneshot(cookie_request(
                "GET",
                &format!(
                    "/api/v1/workspaces/{}/tasks?view=current_week",
                    fixture.workspace_id
                ),
                &member_cookie,
            ))
            .await
            .unwrap(),
    )
    .await;
    let current_week_titles: Vec<_> = current_week["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|task| task["title"].as_str().unwrap().to_owned())
        .collect();
    assert!(current_week_titles.contains(&"This calendar week".to_owned()));
    assert!(current_week_titles.contains(&"Mine this week".to_owned()));
    assert!(!current_week_titles.contains(&"Next calendar week".to_owned()));

    let my_week = response_json(
        fixture
            .app
            .clone()
            .oneshot(cookie_request(
                "GET",
                &format!(
                    "/api/v1/workspaces/{}/tasks?view=my_week",
                    fixture.workspace_id
                ),
                &member_cookie,
            ))
            .await
            .unwrap(),
    )
    .await;
    let my_week_titles: Vec<_> = my_week["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|task| task["title"].as_str().unwrap().to_owned())
        .collect();
    assert!(my_week_titles.contains(&"Mine this week".to_owned()));
    assert!(!my_week_titles.contains(&"This calendar week".to_owned()));
    assert!(!my_week_titles.contains(&"Mine next week".to_owned()));
    assert!(!my_week_titles.contains(&"No date".to_owned()));
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

async fn create_workspace(fixture: &Fixture, name: &str) -> String {
    orbit_server::repositories::workspaces::WorkspaceRepository::new((*fixture.database).clone())
        .create(
            fixture.owner_id,
            name.to_owned(),
            "tasks-test",
            TimestampMillis::now(),
        )
        .await
        .unwrap()
        .id
        .to_string()
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

fn cursor_hex(value: Value) -> String {
    serde_json::to_vec(&value)
        .unwrap()
        .into_iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[tokio::test]
async fn duplicate_status_is_system_managed() {
    let fixture = Fixture::new().await;
    let statuses_uri = format!(
        "/api/v1/workspaces/{}/projects/{}/statuses",
        fixture.workspace_id, fixture.project_id
    );
    let (status, statuses) = call(&fixture, "GET", &statuses_uri, None).await;
    assert_eq!(status, StatusCode::OK);
    let items = statuses["items"].as_array().unwrap();
    let duplicate = items.last().unwrap().clone();
    assert_eq!(duplicate["name"], "Duplicate");
    assert_eq!(duplicate["category"], "duplicate");
    let duplicate_uri = format!("{statuses_uri}/{}", duplicate["id"].as_str().unwrap());

    let (status, problem) = call(
        &fixture,
        "POST",
        &statuses_uri,
        Some(json!({"name":"Dupes", "color":"#112233", "category":"duplicate"})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(problem["detail"], "category");

    let (status, problem) = call(
        &fixture,
        "PATCH",
        &duplicate_uri,
        Some(json!({
            "name":"Duplicate", "color":"#8b8f98", "category":"started",
            "position": duplicate["position"], "expected_version": 0
        })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(problem["detail"], "category");

    let todo = items[1].clone();
    let (status, problem) = call(
        &fixture,
        "PATCH",
        &format!("{statuses_uri}/{}", todo["id"].as_str().unwrap()),
        Some(json!({
            "name":"Todo", "color":"#8b8f98", "category":"duplicate",
            "position": todo["position"], "expected_version": 0
        })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(problem["detail"], "category");

    let (status, renamed) = call(
        &fixture,
        "PATCH",
        &duplicate_uri,
        Some(json!({
            "name":"Dupe", "color":"#aabbcc", "category":"duplicate",
            "position": duplicate["position"], "expected_version": 0
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(renamed["name"], "Dupe");
    assert_eq!(renamed["color"], "#aabbcc");

    let (status, problem) = call(
        &fixture,
        "DELETE",
        &format!("{duplicate_uri}?expected_version=1"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(problem["detail"], "status_id");
}

#[tokio::test]
async fn due_views_skip_tasks_in_the_duplicate_status() {
    let fixture = Fixture::new().await;
    let tasks_uri = format!("/api/v1/workspaces/{}/tasks", fixture.workspace_id);
    let (status, task) = call(
        &fixture,
        "POST",
        &tasks_uri,
        Some(json!({
            "project_id": fixture.project_id,
            "status_id": fixture.status_id,
            "title": "Overdue duplicate",
            "due_at": "2020-01-01T00:00:00Z"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let overdue_uri = format!("{tasks_uri}?view=overdue");
    let (_, before) = call(&fixture, "GET", &overdue_uri, None).await;
    assert_eq!(before["items"].as_array().unwrap().len(), 1);

    let duplicate_status = status_id_by_category(&fixture, &fixture.project_id, "duplicate").await;
    sqlx::query("UPDATE tasks SET status_id = ? WHERE id = ?")
        .bind(&duplicate_status)
        .bind(task["id"].as_str().unwrap())
        .execute(fixture.database.pool())
        .await
        .unwrap();
    let (_, after) = call(&fixture, "GET", &overdue_uri, None).await;
    assert!(after["items"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn marking_a_duplicate_moves_it_to_the_duplicate_status_and_unmarking_restores_it() {
    let fixture = Fixture::new().await;
    let task = fixture.create_task("Login fails on Safari").await;
    let canonical = fixture.create_task("Login broken").await;
    let started = status_id_by_category(&fixture, &fixture.project_id, "started").await;
    let duplicate_status = status_id_by_category(&fixture, &fixture.project_id, "duplicate").await;
    let uri = task_uri(&fixture, id_of(&task));

    let (status, _) = call(
        &fixture,
        "PATCH",
        &uri,
        Some(json!({"expected_version": 0, "status_id": started})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, marked) = call(
        &fixture,
        "PATCH",
        &uri,
        Some(json!({"expected_version": 1, "duplicate_of_id": id_of(&canonical)})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(marked["status_id"], duplicate_status);
    assert_eq!(marked["version"], 2);
    assert_eq!(
        duplicate_target(&fixture, id_of(&task)).await.as_deref(),
        Some(id_of(&canonical))
    );

    let (status, unmarked) = call(
        &fixture,
        "PATCH",
        &uri,
        Some(json!({"expected_version": 2, "duplicate_of_id": null})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(unmarked["status_id"], started);
    assert_eq!(duplicate_target(&fixture, id_of(&task)).await, None);
}

#[tokio::test]
async fn unmarking_falls_back_to_the_first_unstarted_status_when_the_previous_one_is_gone() {
    let fixture = Fixture::new().await;
    let task = fixture.create_task("Flaky upload").await;
    let canonical = fixture.create_task("Uploads fail").await;
    let statuses_uri = format!(
        "/api/v1/workspaces/{}/projects/{}/statuses",
        fixture.workspace_id, fixture.project_id
    );
    let (status, review) = call(
        &fixture,
        "POST",
        &statuses_uri,
        Some(json!({"name":"Review", "color":"#445566", "category":"started"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let uri = task_uri(&fixture, id_of(&task));
    let (status, _) = call(
        &fixture,
        "PATCH",
        &uri,
        Some(json!({"expected_version": 0, "status_id": id_of(&review)})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = call(
        &fixture,
        "PATCH",
        &uri,
        Some(json!({"expected_version": 1, "duplicate_of_id": id_of(&canonical)})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = call(
        &fixture,
        "DELETE",
        &format!("{statuses_uri}/{}?expected_version=0", id_of(&review)),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, unmarked) = call(
        &fixture,
        "PATCH",
        &uri,
        Some(json!({"expected_version": 2, "duplicate_of_id": null})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(unmarked["status_id"], fixture.status_id);
}

#[tokio::test]
async fn moving_a_duplicate_to_another_status_removes_the_relation() {
    let fixture = Fixture::new().await;
    let task = fixture.create_task("Crash on save").await;
    let canonical = fixture.create_task("Saving crashes").await;
    let done = status_id_by_category(&fixture, &fixture.project_id, "completed").await;
    let uri = task_uri(&fixture, id_of(&task));
    let (status, _) = call(
        &fixture,
        "PATCH",
        &uri,
        Some(json!({"expected_version": 0, "duplicate_of_id": id_of(&canonical)})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, moved) = call(
        &fixture,
        "PATCH",
        &uri,
        Some(json!({"expected_version": 1, "status_id": done})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(moved["status_id"], done);
    assert_eq!(duplicate_target(&fixture, id_of(&task)).await, None);
    let unmarked: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events WHERE action = 'task.unmarked_duplicate' AND resource_id = ?",
    )
    .bind(id_of(&task))
    .fetch_one(fixture.database.pool())
    .await
    .unwrap();
    assert_eq!(unmarked, 1);
}

#[tokio::test]
async fn direct_duplicate_status_writes_and_invalid_targets_are_rejected() {
    let fixture = Fixture::new().await;
    let task = fixture.create_task("Task").await;
    let canonical = fixture.create_task("Canonical").await;
    let trashed = fixture.create_task("Trashed").await;
    let duplicate_status = status_id_by_category(&fixture, &fixture.project_id, "duplicate").await;
    let (status, _) = call(
        &fixture,
        "DELETE",
        &format!("{}?expected_version=0", task_uri(&fixture, id_of(&trashed))),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let uri = task_uri(&fixture, id_of(&task));
    for (body, field) in [
        (
            json!({"expected_version": 0, "status_id": duplicate_status}),
            "status_id",
        ),
        (
            json!({"expected_version": 0, "status_id": fixture.status_id, "duplicate_of_id": id_of(&canonical)}),
            "duplicate_of_id",
        ),
        (
            json!({"expected_version": 0, "duplicate_of_id": id_of(&task)}),
            "duplicate_of_id",
        ),
        (
            json!({"expected_version": 0, "duplicate_of_id": id_of(&trashed)}),
            "duplicate_of_id",
        ),
    ] {
        let (status, problem) = call(&fixture, "PATCH", &uri, Some(body.clone())).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        assert_eq!(problem["detail"], field, "{body}");
    }
    let (status, problem) = call(
        &fixture,
        "POST",
        &format!("/api/v1/workspaces/{}/tasks", fixture.workspace_id),
        Some(json!({"project_id": fixture.project_id, "status_id": duplicate_status, "title": "Born duplicate"})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(problem["detail"], "status_id");
    let (_, current) = call(&fixture, "GET", &uri, None).await;
    assert_eq!(current["version"], 0);
    assert_eq!(current["status_id"], fixture.status_id);
}

#[tokio::test]
async fn duplicates_never_chain_and_marking_a_canonical_repoints_its_duplicates() {
    let fixture = Fixture::new().await;
    let first = fixture.create_task("First").await;
    let second = fixture.create_task("Second").await;
    let third = fixture.create_task("Third").await;
    let todo: String =
        sqlx::query_scalar("SELECT id FROM task_statuses WHERE project_id = ? AND name = 'Todo'")
            .bind(&fixture.project_id)
            .fetch_one(fixture.database.pool())
            .await
            .unwrap();
    let first_uri = task_uri(&fixture, id_of(&first));
    let (status, _) = call(
        &fixture,
        "PATCH",
        &first_uri,
        Some(json!({"expected_version": 0, "status_id": todo})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = call(
        &fixture,
        "PATCH",
        &first_uri,
        Some(json!({"expected_version": 1, "duplicate_of_id": id_of(&second)})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, problem) = call(
        &fixture,
        "PATCH",
        &task_uri(&fixture, id_of(&third)),
        Some(json!({"expected_version": 0, "duplicate_of_id": id_of(&first)})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(problem["detail"], "duplicate_of_id");

    let (status, _) = call(
        &fixture,
        "PATCH",
        &task_uri(&fixture, id_of(&second)),
        Some(json!({"expected_version": 0, "duplicate_of_id": id_of(&third)})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        duplicate_target(&fixture, id_of(&first)).await.as_deref(),
        Some(id_of(&third))
    );
    assert_eq!(
        duplicate_target(&fixture, id_of(&second)).await.as_deref(),
        Some(id_of(&third))
    );
    let (_, repointed) = call(&fixture, "GET", &first_uri, None).await;
    assert_eq!(
        repointed["version"], 2,
        "re-pointing must not bump the follower's version"
    );

    let (status, unmarked) = call(
        &fixture,
        "PATCH",
        &first_uri,
        Some(json!({"expected_version": 2, "duplicate_of_id": null})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        unmarked["status_id"], todo,
        "the original previous status survives re-pointing"
    );
}

#[tokio::test]
async fn cross_project_duplicates_use_their_own_duplicate_status_and_follow_project_moves() {
    let fixture = Fixture::new().await;
    let other_project = create_project(&fixture, "OTH").await;
    let other_backlog = status_id_by_category(&fixture, &other_project, "unstarted").await;
    let (status, canonical) = call(
        &fixture,
        "POST",
        &format!("/api/v1/workspaces/{}/tasks", fixture.workspace_id),
        Some(json!({"project_id": other_project, "status_id": other_backlog, "title": "Other canonical"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let task = fixture.create_task("Local duplicate").await;
    let uri = task_uri(&fixture, id_of(&task));

    let (status, marked) = call(
        &fixture,
        "PATCH",
        &uri,
        Some(json!({"expected_version": 0, "duplicate_of_id": id_of(&canonical)})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        marked["status_id"],
        status_id_by_category(&fixture, &fixture.project_id, "duplicate").await
    );
    let metadata: String = sqlx::query_scalar(
        "SELECT metadata_json FROM audit_events WHERE action = 'task.marked_duplicate' AND resource_id = ?",
    )
    .bind(id_of(&task))
    .fetch_one(fixture.database.pool())
    .await
    .unwrap();
    let metadata: Value = serde_json::from_str(&metadata).unwrap();
    assert_eq!(
        metadata["related_task_project_id"], other_project,
        "cross-project identifiers need the other task's project"
    );

    let (status, moved) = call(
        &fixture,
        "PATCH",
        &uri,
        Some(json!({"expected_version": 1, "project_id": other_project})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(moved["project_id"], other_project);
    assert_eq!(
        moved["status_id"],
        status_id_by_category(&fixture, &other_project, "duplicate").await
    );
    assert_eq!(
        duplicate_target(&fixture, id_of(&task)).await.as_deref(),
        Some(id_of(&canonical))
    );
}

#[tokio::test]
async fn bulk_marking_and_undo_are_atomic() {
    let fixture = Fixture::new().await;
    let first = fixture.create_task("First").await;
    let second = fixture.create_task("Second").await;
    let canonical = fixture.create_task("Canonical").await;
    let duplicate_status = status_id_by_category(&fixture, &fixture.project_id, "duplicate").await;
    let bulk_uri = format!("/api/v1/workspaces/{}/tasks/bulk", fixture.workspace_id);

    let (status, problem) = call(
        &fixture,
        "POST",
        &bulk_uri,
        Some(json!({"updates": [
            {"id": id_of(&first), "expected_version": 0, "duplicate_of_id": id_of(&canonical)},
            {"id": id_of(&second), "expected_version": 0, "duplicate_of_id": id_of(&second)}
        ]})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(problem["detail"], "duplicate_of_id");
    assert_eq!(
        duplicate_target(&fixture, id_of(&first)).await,
        None,
        "the first item must roll back"
    );

    let (status, marked) = call(
        &fixture,
        "POST",
        &bulk_uri,
        Some(json!({"updates": [
            {"id": id_of(&first), "expected_version": 0, "duplicate_of_id": id_of(&canonical)},
            {"id": id_of(&second), "expected_version": 0, "duplicate_of_id": id_of(&canonical)}
        ]})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        marked["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["status_id"] == duplicate_status)
    );

    let (status, undone) = call(
        &fixture,
        "POST",
        &bulk_uri,
        Some(json!({"updates": [
            {"id": id_of(&first), "expected_version": 1, "duplicate_of_id": null},
            {"id": id_of(&second), "expected_version": 1, "duplicate_of_id": null}
        ]})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        undone["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["status_id"] == fixture.status_id)
    );
    assert_eq!(duplicate_target(&fixture, id_of(&first)).await, None);
    assert_eq!(duplicate_target(&fixture, id_of(&second)).await, None);
}

#[tokio::test]
async fn bulk_marking_a_canonical_and_its_duplicate_together_succeeds() {
    let fixture = Fixture::new().await;
    let first = fixture.create_task("First").await;
    let second = fixture.create_task("Second").await;
    let canonical = fixture.create_task("Canonical").await;
    let (status, _) = call(
        &fixture,
        "PATCH",
        &task_uri(&fixture, id_of(&first)),
        Some(json!({"expected_version": 0, "duplicate_of_id": id_of(&second)})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = call(
        &fixture,
        "POST",
        &format!("/api/v1/workspaces/{}/tasks/bulk", fixture.workspace_id),
        Some(json!({"updates": [
            {"id": id_of(&second), "expected_version": 0, "duplicate_of_id": id_of(&canonical)},
            {"id": id_of(&first), "expected_version": 1, "duplicate_of_id": id_of(&canonical)}
        ]})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        duplicate_target(&fixture, id_of(&first)).await.as_deref(),
        Some(id_of(&canonical))
    );
    assert_eq!(
        duplicate_target(&fixture, id_of(&second)).await.as_deref(),
        Some(id_of(&canonical))
    );
}

#[tokio::test]
async fn duplicate_edge_cases_stay_recoverable() {
    let fixture = Fixture::new().await;
    let task = fixture.create_task("Duplicate").await;
    let canonical = fixture.create_task("Canonical").await;
    let plain = fixture.create_task("Plain").await;
    let duplicate_status = status_id_by_category(&fixture, &fixture.project_id, "duplicate").await;
    let uri = task_uri(&fixture, id_of(&task));
    let (status, _) = call(
        &fixture,
        "PATCH",
        &uri,
        Some(json!({"expected_version": 0, "duplicate_of_id": id_of(&canonical)})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Re-sending the current Duplicate status alongside other fields is not a status change.
    let (status, saved) = call(
        &fixture,
        "PATCH",
        &uri,
        Some(json!({"expected_version": 1, "status_id": duplicate_status, "priority": "high"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(saved["priority"], "high");
    assert_eq!(
        duplicate_target(&fixture, id_of(&task)).await.as_deref(),
        Some(id_of(&canonical))
    );

    // Unmarking a task that is not a duplicate is a no-op, so a retried bulk undo succeeds.
    let (status, untouched) = call(
        &fixture,
        "PATCH",
        &task_uri(&fixture, id_of(&plain)),
        Some(json!({"expected_version": 0, "duplicate_of_id": null})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(untouched["status_id"], fixture.status_id);

    // A retention purge of the canonical leaves the duplicate recoverable.
    sqlx::query("DELETE FROM tasks WHERE id = ?")
        .bind(id_of(&canonical))
        .execute(fixture.database.pool())
        .await
        .unwrap();
    let (status, restored) = call(
        &fixture,
        "PATCH",
        &uri,
        Some(json!({"expected_version": 2, "duplicate_of_id": null})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(restored["status_id"], fixture.status_id);
}

#[tokio::test]
async fn duplicate_marking_is_audited_on_both_tasks() {
    let fixture = Fixture::new().await;
    let task = fixture.create_task("Duplicate task").await;
    let canonical = fixture.create_task("Canonical task").await;
    let uri = task_uri(&fixture, id_of(&task));
    let (status, _) = call(
        &fixture,
        "PATCH",
        &uri,
        Some(json!({"expected_version": 0, "duplicate_of_id": id_of(&canonical)})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT resource_id, metadata_json FROM audit_events WHERE action = 'task.marked_duplicate'",
    )
    .fetch_all(fixture.database.pool())
    .await
    .unwrap();
    assert_eq!(rows.len(), 2);
    let metadata = |resource: &str| -> Value {
        serde_json::from_str(&rows.iter().find(|(id, _)| id == resource).unwrap().1).unwrap()
    };
    assert_eq!(
        metadata(id_of(&task)),
        json!({"type": "duplicate", "direction": "outgoing", "related_task_id": id_of(&canonical), "related_task_project_id": fixture.project_id, "related_task_title": "Canonical task"}),
        "the duplicate's event is outgoing"
    );
    assert_eq!(
        metadata(id_of(&canonical)),
        json!({"type": "duplicate", "direction": "incoming", "related_task_id": id_of(&task), "related_task_project_id": fixture.project_id, "related_task_title": "Duplicate task"}),
        "the canonical's event is incoming"
    );
    let (_, activity) = call(
        &fixture,
        "GET",
        &format!("{}/activity", task_uri(&fixture, id_of(&canonical))),
        None,
    )
    .await;
    assert!(
        activity["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|event| event["action"] == "task.marked_duplicate")
    );

    let (status, _) = call(
        &fixture,
        "PATCH",
        &uri,
        Some(json!({"expected_version": 1, "duplicate_of_id": null})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let unmarked: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events WHERE action = 'task.unmarked_duplicate'",
    )
    .fetch_one(fixture.database.pool())
    .await
    .unwrap();
    assert_eq!(unmarked, 2);
}

#[tokio::test]
async fn github_state_changes_move_duplicates_out_and_drop_the_relation() {
    let fixture = Fixture::new().await;
    let task = fixture.create_task("Synced issue").await;
    let canonical = fixture.create_task("Canonical issue").await;
    sqlx::query("INSERT INTO github_issue_links (workspace_id, repository, issue_number, task_id, kind) VALUES (?, 'acme/repo', 12, ?, 'issue')")
        .bind(&fixture.workspace_id)
        .bind(id_of(&task))
        .execute(fixture.database.pool())
        .await
        .unwrap();
    let (status, _) = call(
        &fixture,
        "PATCH",
        &task_uri(&fixture, id_of(&task)),
        Some(json!({"expected_version": 0, "duplicate_of_id": id_of(&canonical)})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    TaskRepository::new((*fixture.database).clone())
        .sync_github_work_item(
            fixture.workspace_id.parse().unwrap(),
            fixture.owner_id,
            GithubWorkItem {
                repository: "acme/repo".to_owned(),
                number: 12,
                project_id: fixture.project_id.parse().unwrap(),
                title: "Synced issue".to_owned(),
                description: String::new(),
                kind: "issue",
                state: "closed",
                state_changed: true,
            },
            "github-test",
            TimestampMillis::now(),
        )
        .await
        .unwrap();

    let category: String = sqlx::query_scalar("SELECT category FROM task_statuses JOIN tasks ON tasks.status_id = task_statuses.id WHERE tasks.id = ?")
        .bind(id_of(&task))
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    assert_eq!(category, "completed");
    assert_eq!(duplicate_target(&fixture, id_of(&task)).await, None);
    let actor: Option<String> = sqlx::query_scalar(
        "SELECT actor_id FROM audit_events WHERE action = 'task.unmarked_duplicate' AND resource_id = ?",
    )
    .bind(id_of(&task))
    .fetch_one(fixture.database.pool())
    .await
    .unwrap();
    assert_eq!(actor, None, "GitHub acts through its service account");
    let metadata: String = sqlx::query_scalar(
        "SELECT metadata_json FROM audit_events WHERE action = 'task.unmarked_duplicate' AND resource_id = ?",
    )
    .bind(id_of(&task))
    .fetch_one(fixture.database.pool())
    .await
    .unwrap();
    let metadata: Value = serde_json::from_str(&metadata).unwrap();
    assert_eq!(metadata["direction"], "outgoing");
    assert_eq!(metadata["related_task_id"], id_of(&canonical));
    assert_eq!(metadata["related_task_project_id"], fixture.project_id);
    assert_eq!(metadata["related_task_title"], "Canonical issue");
    assert!(metadata["actor_service_account_id"].is_string());
}

#[tokio::test]
async fn task_records_expose_the_visible_duplicate_target() {
    let fixture = Fixture::new().await;
    let task = fixture.create_task("Duplicate").await;
    let canonical = fixture.create_task("Canonical").await;
    assert!(task["duplicate_of"].is_null());
    assert_eq!(task["blocked"], false);
    let uri = task_uri(&fixture, id_of(&task));
    let key = project_key_of(&fixture, &fixture.project_id).await;
    let expected = json!({"id": id_of(&canonical), "project_id": fixture.project_id, "project_key": key, "title": "Canonical"});

    let (status, marked) = call(
        &fixture,
        "PATCH",
        &uri,
        Some(json!({"expected_version": 0, "duplicate_of_id": id_of(&canonical)})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(marked["duplicate_of"], expected);
    let (_, fetched) = call(&fixture, "GET", &uri, None).await;
    assert_eq!(fetched["duplicate_of"], expected);
    let (_, listed) = call(
        &fixture,
        "GET",
        &format!(
            "/api/v1/workspaces/{}/tasks?project_id={}",
            fixture.workspace_id, fixture.project_id
        ),
        None,
    )
    .await;
    let items = listed["items"].as_array().unwrap();
    let listed_task = items.iter().find(|item| item["id"] == task["id"]).unwrap();
    assert_eq!(listed_task["duplicate_of"], expected);
    let listed_canonical = items
        .iter()
        .find(|item| item["id"] == canonical["id"])
        .unwrap();
    assert!(listed_canonical["duplicate_of"].is_null());

    let canonical_uri = task_uri(&fixture, id_of(&canonical));
    let (status, _) = call(
        &fixture,
        "DELETE",
        &format!("{canonical_uri}?expected_version=0"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, hidden) = call(&fixture, "GET", &uri, None).await;
    assert!(
        hidden["duplicate_of"].is_null(),
        "a trashed target is hidden"
    );
    let (status, _) = call(
        &fixture,
        "POST",
        &format!("{canonical_uri}/restore"),
        Some(json!({"expected_version": 1})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, back) = call(&fixture, "GET", &uri, None).await;
    assert_eq!(back["duplicate_of"], expected);

    let (status, unmarked) = call(
        &fixture,
        "PATCH",
        &uri,
        Some(json!({"expected_version": 1, "duplicate_of_id": null})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(unmarked["duplicate_of"].is_null());
}

#[tokio::test]
async fn blocked_flag_follows_open_blockers() {
    let fixture = Fixture::new().await;
    let blocker = fixture.create_task("Blocker").await;
    let blocked = fixture.create_task("Blocked").await;
    sqlx::query("INSERT INTO task_relations (id, workspace_id, task_id, related_task_id, type, created_at) VALUES (?, ?, ?, ?, 'blocks', 0)")
        .bind(Id::new_v7().to_string())
        .bind(&fixture.workspace_id)
        .bind(id_of(&blocker))
        .bind(id_of(&blocked))
        .execute(fixture.database.pool())
        .await
        .unwrap();
    let blocked_uri = task_uri(&fixture, id_of(&blocked));
    let blocker_uri = task_uri(&fixture, id_of(&blocker));
    let done = status_id_by_category(&fixture, &fixture.project_id, "completed").await;

    assert_eq!(
        call(&fixture, "GET", &blocked_uri, None).await.1["blocked"],
        true
    );
    assert_eq!(
        call(&fixture, "GET", &blocker_uri, None).await.1["blocked"],
        false
    );
    let (_, listed) = call(
        &fixture,
        "GET",
        &format!("/api/v1/workspaces/{}/tasks", fixture.workspace_id),
        None,
    )
    .await;
    let listed_blocked = listed["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == blocked["id"])
        .unwrap()
        .clone();
    assert_eq!(listed_blocked["blocked"], true);

    let (status, _) = call(
        &fixture,
        "PATCH",
        &blocker_uri,
        Some(json!({"expected_version": 0, "status_id": done})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        call(&fixture, "GET", &blocked_uri, None).await.1["blocked"],
        false
    );
    let (status, _) = call(
        &fixture,
        "PATCH",
        &blocker_uri,
        Some(json!({"expected_version": 1, "status_id": fixture.status_id})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        call(&fixture, "GET", &blocked_uri, None).await.1["blocked"],
        true
    );
    let (status, _) = call(
        &fixture,
        "DELETE",
        &format!("{blocker_uri}?expected_version=2"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        call(&fixture, "GET", &blocked_uri, None).await.1["blocked"],
        false
    );
}

#[tokio::test]
async fn relations_can_be_added_listed_and_removed() {
    let fixture = Fixture::new().await;
    let blocker = fixture.create_task("Auth token refresh").await;
    let blocked = fixture.create_task("Login fails").await;
    let (status, created) = call(
        &fixture,
        "POST",
        &relations_uri(&fixture, id_of(&blocker)),
        Some(json!({"type": "blocks", "task_id": id_of(&blocked)})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(created["type"], "blocks");
    assert_eq!(created["direction"], "outgoing");
    assert_eq!(
        created["task"],
        json!({"id": id_of(&blocked), "project_id": fixture.project_id, "title": "Login fails", "status_id": fixture.status_id})
    );
    assert!(created["created_at"].is_string());

    let (status, from_blocked) = call(
        &fixture,
        "GET",
        &relations_uri(&fixture, id_of(&blocked)),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(from_blocked.as_array().unwrap().len(), 1);
    assert_eq!(from_blocked[0]["id"], created["id"]);
    assert_eq!(from_blocked[0]["direction"], "incoming");
    assert_eq!(from_blocked[0]["task"]["id"], blocker["id"]);
    assert_eq!(
        call(&fixture, "GET", &task_uri(&fixture, id_of(&blocked)), None)
            .await
            .1["blocked"],
        true
    );

    let relation_uri = format!(
        "{}/{}",
        relations_uri(&fixture, id_of(&blocked)),
        id_of(&created)
    );
    let (status, _) = call(&fixture, "DELETE", &relation_uri, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, remaining) = call(
        &fixture,
        "GET",
        &relations_uri(&fixture, id_of(&blocker)),
        None,
    )
    .await;
    assert!(remaining.as_array().unwrap().is_empty());
    let (status, _) = call(&fixture, "DELETE", &relation_uri, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn blocked_by_is_normalised_and_related_pairs_are_ordered() {
    let fixture = Fixture::new().await;
    let first = fixture.create_task("First").await;
    let second = fixture.create_task("Second").await;
    let third = fixture.create_task("Third").await;
    let (status, created) = call(
        &fixture,
        "POST",
        &relations_uri(&fixture, id_of(&first)),
        Some(json!({"type": "blocked_by", "task_id": id_of(&second)})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(created["type"], "blocks");
    assert_eq!(created["direction"], "incoming");
    assert_eq!(created["task"]["id"], second["id"]);
    let stored: (String, String, String) =
        sqlx::query_as("SELECT task_id, related_task_id, type FROM task_relations WHERE id = ?")
            .bind(id_of(&created))
            .fetch_one(fixture.database.pool())
            .await
            .unwrap();
    assert_eq!(
        stored,
        (
            id_of(&second).to_owned(),
            id_of(&first).to_owned(),
            "blocks".to_owned()
        )
    );

    let (status, related) = call(
        &fixture,
        "POST",
        &relations_uri(&fixture, id_of(&third)),
        Some(json!({"type": "related", "task_id": id_of(&first)})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(related["type"], "related");
    let (low, high): (String, String) =
        sqlx::query_as("SELECT task_id, related_task_id FROM task_relations WHERE id = ?")
            .bind(id_of(&related))
            .fetch_one(fixture.database.pool())
            .await
            .unwrap();
    assert!(low < high);
    let mut pair = [low, high];
    pair.sort();
    let mut expected = [id_of(&first).to_owned(), id_of(&third).to_owned()];
    expected.sort();
    assert_eq!(pair, expected);
}

#[tokio::test]
async fn relations_convert_existing_pairs_and_reject_cycles_and_duplicates() {
    let fixture = Fixture::new().await;
    let first = fixture.create_task("First").await;
    let second = fixture.create_task("Second").await;
    let canonical = fixture.create_task("Canonical").await;
    let (status, created) = call(
        &fixture,
        "POST",
        &relations_uri(&fixture, id_of(&first)),
        Some(json!({"type": "blocks", "task_id": id_of(&second)})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, problem) = call(
        &fixture,
        "POST",
        &relations_uri(&fixture, id_of(&second)),
        Some(json!({"type": "blocks", "task_id": id_of(&first)})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(problem["code"], "task_conflict");
    let (status, again) = call(
        &fixture,
        "POST",
        &relations_uri(&fixture, id_of(&first)),
        Some(json!({"type": "blocks", "task_id": id_of(&second)})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(again["id"], created["id"]);

    let (status, converted) = call(
        &fixture,
        "POST",
        &relations_uri(&fixture, id_of(&second)),
        Some(json!({"type": "related", "task_id": id_of(&first)})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(converted["type"], "related");
    let types: Vec<String> = sqlx::query_scalar("SELECT type FROM task_relations")
        .fetch_all(fixture.database.pool())
        .await
        .unwrap();
    assert_eq!(types, ["related"]);

    let (status, problem) = call(
        &fixture,
        "POST",
        &relations_uri(&fixture, id_of(&first)),
        Some(json!({"type": "related", "task_id": id_of(&first)})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(problem["detail"], "task_id");
    let (status, _) = call(
        &fixture,
        "POST",
        &relations_uri(&fixture, id_of(&first)),
        Some(json!({"type": "duplicate", "task_id": id_of(&second)})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, _) = call(
        &fixture,
        "PATCH",
        &task_uri(&fixture, id_of(&first)),
        Some(json!({"expected_version": 0, "duplicate_of_id": id_of(&canonical)})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = call(
        &fixture,
        "POST",
        &relations_uri(&fixture, id_of(&canonical)),
        Some(json!({"type": "related", "task_id": id_of(&first)})),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "a duplicate pair must be unmarked first"
    );
}

#[tokio::test]
async fn deleting_a_duplicate_relation_unmarks_the_duplicate() {
    let fixture = Fixture::new().await;
    let task = fixture.create_task("Duplicate").await;
    let canonical = fixture.create_task("Canonical").await;
    let (status, _) = call(
        &fixture,
        "PATCH",
        &task_uri(&fixture, id_of(&task)),
        Some(json!({"expected_version": 0, "duplicate_of_id": id_of(&canonical)})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, relations) = call(
        &fixture,
        "GET",
        &relations_uri(&fixture, id_of(&canonical)),
        None,
    )
    .await;
    assert_eq!(relations[0]["type"], "duplicate");
    assert_eq!(relations[0]["direction"], "incoming");
    assert_eq!(relations[0]["task"]["id"], task["id"]);

    let (status, _) = call(
        &fixture,
        "DELETE",
        &format!(
            "{}/{}",
            relations_uri(&fixture, id_of(&canonical)),
            relations[0]["id"].as_str().unwrap()
        ),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, unmarked) = call(&fixture, "GET", &task_uri(&fixture, id_of(&task)), None).await;
    assert_eq!(unmarked["status_id"], fixture.status_id);
    assert_eq!(unmarked["version"], 2);
    assert!(unmarked["duplicate_of"].is_null());
}

#[tokio::test]
async fn relations_to_trashed_tasks_are_hidden_until_restore() {
    let fixture = Fixture::new().await;
    let first = fixture.create_task("First").await;
    let second = fixture.create_task("Second").await;
    let (status, _) = call(
        &fixture,
        "POST",
        &relations_uri(&fixture, id_of(&first)),
        Some(json!({"type": "related", "task_id": id_of(&second)})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let second_uri = task_uri(&fixture, id_of(&second));
    let (status, _) = call(
        &fixture,
        "DELETE",
        &format!("{second_uri}?expected_version=0"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, hidden) = call(
        &fixture,
        "GET",
        &relations_uri(&fixture, id_of(&first)),
        None,
    )
    .await;
    assert!(hidden.as_array().unwrap().is_empty());
    let (status, _) = call(
        &fixture,
        "POST",
        &format!("{second_uri}/restore"),
        Some(json!({"expected_version": 1})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, restored) = call(
        &fixture,
        "GET",
        &relations_uri(&fixture, id_of(&first)),
        None,
    )
    .await;
    assert_eq!(restored.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn relation_changes_are_audited_on_both_tasks() {
    let fixture = Fixture::new().await;
    let blocker = fixture.create_task("Blocker").await;
    let blocked = fixture.create_task("Blocked").await;
    let (_, created) = call(
        &fixture,
        "POST",
        &relations_uri(&fixture, id_of(&blocker)),
        Some(json!({"type": "blocks", "task_id": id_of(&blocked)})),
    )
    .await;
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT resource_id, metadata_json FROM audit_events WHERE action = 'task.relation_added'",
    )
    .fetch_all(fixture.database.pool())
    .await
    .unwrap();
    assert_eq!(rows.len(), 2);
    let metadata = |resource: &str| -> Value {
        serde_json::from_str(&rows.iter().find(|(id, _)| id == resource).unwrap().1).unwrap()
    };
    assert_eq!(
        metadata(id_of(&blocker)),
        json!({"type": "blocks", "direction": "outgoing", "related_task_id": id_of(&blocked), "related_task_project_id": fixture.project_id, "related_task_title": "Blocked"}),
        "the blocker's event is outgoing"
    );
    assert_eq!(
        metadata(id_of(&blocked)),
        json!({"type": "blocks", "direction": "incoming", "related_task_id": id_of(&blocker), "related_task_project_id": fixture.project_id, "related_task_title": "Blocker"}),
        "the blocked task's event is incoming"
    );
    let (status, _) = call(
        &fixture,
        "DELETE",
        &format!(
            "{}/{}",
            relations_uri(&fixture, id_of(&blocker)),
            id_of(&created)
        ),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let removed: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events WHERE action = 'task.relation_removed'",
    )
    .fetch_one(fixture.database.pool())
    .await
    .unwrap();
    assert_eq!(removed, 2);
}

#[tokio::test]
async fn marking_a_duplicate_audits_the_relation_it_replaces() {
    let fixture = Fixture::new().await;
    let task = fixture.create_task("Blocker").await;
    let canonical = fixture.create_task("Blocked").await;
    let (status, _) = call(
        &fixture,
        "POST",
        &relations_uri(&fixture, id_of(&task)),
        Some(json!({"type": "blocks", "task_id": id_of(&canonical)})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) = call(
        &fixture,
        "PATCH",
        &task_uri(&fixture, id_of(&task)),
        Some(json!({"expected_version": 0, "duplicate_of_id": id_of(&canonical)})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let removed: Vec<String> = sqlx::query_scalar(
        "SELECT metadata_json FROM audit_events WHERE action = 'task.relation_removed'",
    )
    .fetch_all(fixture.database.pool())
    .await
    .unwrap();
    assert_eq!(removed.len(), 2, "both tasks record the removed blocker");
    assert!(
        removed
            .iter()
            .all(|metadata| metadata.contains(r#""type":"blocks""#))
    );
}

#[tokio::test]
async fn purging_a_canonical_task_moves_its_duplicates_out_of_the_duplicate_status() {
    let fixture = Fixture::new().await;
    let task = fixture.create_task("Login fails on Safari").await;
    let canonical = fixture.create_task("Login broken").await;
    let started = status_id_by_category(&fixture, &fixture.project_id, "started").await;
    let uri = task_uri(&fixture, id_of(&task));
    let (status, _) = call(
        &fixture,
        "PATCH",
        &uri,
        Some(json!({"expected_version": 0, "status_id": started})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = call(
        &fixture,
        "PATCH",
        &uri,
        Some(json!({"expected_version": 1, "duplicate_of_id": id_of(&canonical)})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    sqlx::query("UPDATE tasks SET deleted_at = ? WHERE id = ?")
        .bind(TimestampMillis::now().as_millis() - 31 * 24 * 60 * 60 * 1_000)
        .bind(id_of(&canonical))
        .execute(fixture.database.pool())
        .await
        .unwrap();

    WorkspaceRepository::new((*fixture.database).clone())
        .purge_retention(TimestampMillis::now())
        .await
        .unwrap();

    let (status, survivor) = call(&fixture, "GET", &uri, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(survivor["status_id"], started);
    assert_eq!(survivor["version"], 3);
    assert_eq!(duplicate_target(&fixture, id_of(&task)).await, None);
}

async fn call(
    fixture: &Fixture,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let request = match body {
        Some(value) => json_request(method, uri, &fixture.owner_cookie, value),
        None => cookie_request(method, uri, &fixture.owner_cookie),
    };
    let response = fixture.app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    (status, value)
}

async fn status_id_by_category(fixture: &Fixture, project_id: &str, category: &str) -> String {
    sqlx::query_scalar(
        "SELECT id FROM task_statuses WHERE project_id = ? AND category = ? ORDER BY position, id LIMIT 1",
    )
    .bind(project_id)
    .bind(category)
    .fetch_one(fixture.database.pool())
    .await
    .unwrap()
}

fn task_uri(fixture: &Fixture, task_id: &str) -> String {
    format!(
        "/api/v1/workspaces/{}/tasks/{task_id}",
        fixture.workspace_id
    )
}

fn id_of(value: &Value) -> &str {
    value["id"].as_str().unwrap()
}

async fn duplicate_target(fixture: &Fixture, task_id: &str) -> Option<String> {
    sqlx::query_scalar(
        "SELECT related_task_id FROM task_relations WHERE task_id = ? AND type = 'duplicate'",
    )
    .bind(task_id)
    .fetch_optional(fixture.database.pool())
    .await
    .unwrap()
}

async fn create_project(fixture: &Fixture, key: &str) -> String {
    let (status, project) = call(
        fixture,
        "POST",
        &format!("/api/v1/workspaces/{}/projects", fixture.workspace_id),
        Some(json!({"name": key, "key": key, "color": "#123456"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    id_of(&project).to_owned()
}

fn relations_uri(fixture: &Fixture, task_id: &str) -> String {
    format!("{}/relations", task_uri(fixture, task_id))
}

#[tokio::test]
async fn completed_at_is_set_and_cleared_by_status_changes() {
    let fixture = Fixture::new().await;
    let done = status_id_by_category(&fixture, &fixture.project_id, "completed").await;
    let cancelled = status_id_by_category(&fixture, &fixture.project_id, "cancelled").await;
    let task = fixture.create_task("Finish me").await;
    let id = id_of(&task).to_owned();
    assert_eq!(task_timestamps(&fixture, &id).await.0, None);

    let (status, task) = call(
        &fixture,
        "PATCH",
        &task_uri(&fixture, &id),
        Some(json!({"expected_version": task["version"], "status_id": done})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (completed_at, updated_at) = task_timestamps(&fixture, &id).await;
    assert_eq!(completed_at, Some(updated_at));

    // Every PATCH rewrites status_id; an unrelated edit must keep completed_at.
    let (status, task) = call(
        &fixture,
        "PATCH",
        &task_uri(&fixture, &id),
        Some(json!({"expected_version": task["version"], "title": "Finished"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(task_timestamps(&fixture, &id).await.0, completed_at);

    // Done -> Cancelled stays in a done category, so completed_at is kept.
    let (status, task) = call(
        &fixture,
        "PATCH",
        &task_uri(&fixture, &id),
        Some(json!({"expected_version": task["version"], "status_id": cancelled})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(task_timestamps(&fixture, &id).await.0, completed_at);

    // Back to an open status clears it.
    let (status, _) = call(
        &fixture,
        "PATCH",
        &task_uri(&fixture, &id),
        Some(json!({"expected_version": task["version"], "status_id": fixture.status_id})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(task_timestamps(&fixture, &id).await.0, None);

    // A task created directly in a done status gets completed_at on insert.
    let (status, born_done) = call(
        &fixture,
        "POST",
        &format!("/api/v1/workspaces/{}/tasks", fixture.workspace_id),
        Some(json!({"project_id": fixture.project_id, "status_id": done, "title": "Born done"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (completed_at, updated_at) = task_timestamps(&fixture, id_of(&born_done)).await;
    assert_eq!(completed_at, Some(updated_at));
}

#[tokio::test]
async fn completed_at_follows_status_category_edits() {
    let fixture = Fixture::new().await;
    let task = fixture.create_task("Shipped").await;
    let id = id_of(&task).to_owned();
    async fn edit_category(fixture: &Fixture, category: &str, version: i64) -> (StatusCode, Value) {
        let uri = format!(
            "/api/v1/workspaces/{}/projects/{}/statuses/{}",
            fixture.workspace_id, fixture.project_id, fixture.status_id
        );
        let body = json!({"name": "Shipped", "color": "#123456", "category": category,
                          "position": 0, "expected_version": version});
        call(fixture, "PATCH", &uri, Some(body)).await
    }

    let (status, body) = edit_category(&fixture, "completed", 0).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(task_timestamps(&fixture, &id).await.0.is_some());

    // done -> done keeps the time; done -> open clears it
    let completed_at = task_timestamps(&fixture, &id).await.0;
    let (status, _) = edit_category(&fixture, "cancelled", 1).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(task_timestamps(&fixture, &id).await.0, completed_at);
    let (status, _) = edit_category(&fixture, "started", 2).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(task_timestamps(&fixture, &id).await.0, None);
}

async fn task_timestamps(fixture: &Fixture, task_id: &str) -> (Option<i64>, i64) {
    sqlx::query_as("SELECT completed_at, updated_at FROM tasks WHERE id = ?")
        .bind(task_id)
        .fetch_one(fixture.database.pool())
        .await
        .unwrap()
}

#[tokio::test]
async fn legacy_list_parameters_keep_their_results() {
    let fixture = Fixture::new().await;
    let (member_id, member_cookie) = add_member(&fixture, "legacy@example.com").await;
    let started = status_id_by_category(&fixture, &fixture.project_id, "started").await;
    let done = status_id_by_category(&fixture, &fixture.project_id, "completed").await;
    let other_project = create_project(&fixture, "LEGACY").await;
    let other_status = status_id_by_category(&fixture, &other_project, "unstarted").await;
    let (status, label) = call(
        &fixture,
        "POST",
        &format!("/api/v1/workspaces/{}/labels", fixture.workspace_id),
        Some(json!({"name": "bug", "color": "#ff0000"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let bug = id_of(&label).to_owned();
    let yesterday = TimestampMillis::from_millis(TimestampMillis::now().as_millis() - 86_400_000);
    let tomorrow = TimestampMillis::from_millis(TimestampMillis::now().as_millis() + 86_400_000);
    for body in [
        json!({"project_id": fixture.project_id, "status_id": started, "title": "Alpha 100%", "priority": "urgent",
               "assignee_ids": [fixture.owner_id.to_string()], "label_ids": [bug], "due_at": yesterday}),
        json!({"project_id": fixture.project_id, "status_id": done, "title": "Bravo", "priority": "high",
               "assignee_ids": [member_id.to_string()], "due_at": yesterday}),
        json!({"project_id": other_project, "status_id": other_status, "title": "Charlie",
               "description": "find the needle"}),
        json!({"project_id": fixture.project_id, "status_id": fixture.status_id, "title": "Delta", "priority": "urgent",
               "assignee_ids": [member_id.to_string()], "label_ids": [bug], "due_at": tomorrow}),
    ] {
        let (status, _) = call(
            &fixture,
            "POST",
            &format!("/api/v1/workspaces/{}/tasks", fixture.workspace_id),
            Some(body),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
    }

    let main = fixture.project_id.clone();
    let member = member_id.to_string();
    let unknown = Id::new_v7().to_string();
    for (query, expected) in [
        (
            "search=".to_owned(),
            vec!["Alpha 100%", "Bravo", "Charlie", "Delta"],
        ),
        (
            format!("project_id={main}"),
            vec!["Alpha 100%", "Bravo", "Delta"],
        ),
        (format!("project_id={other_project}"), vec!["Charlie"]),
        (format!("status_id={started}"), vec!["Alpha 100%"]),
        (format!("status_id={unknown}"), vec![]),
        (format!("assignee_id={member}"), vec!["Bravo", "Delta"]),
        ("unassigned=true".to_owned(), vec!["Charlie"]),
        (format!("unassigned=true&project_id={main}"), vec![]),
        (format!("label_id={bug}"), vec!["Alpha 100%", "Delta"]),
        ("priority=urgent".to_owned(), vec!["Alpha 100%", "Delta"]),
        ("search=needle".to_owned(), vec!["Charlie"]),
        ("search=100%25".to_owned(), vec!["Alpha 100%"]),
        ("search=ALPHA".to_owned(), vec!["Alpha 100%"]),
        ("view=overdue".to_owned(), vec!["Alpha 100%"]),
        // view=mine always meant the caller; assignee_id was ignored with it.
        (
            format!("view=mine&assignee_id={member}"),
            vec!["Alpha 100%"],
        ),
        (
            format!("priority=urgent&label_id={bug}&assignee_id={member}"),
            vec!["Delta"],
        ),
    ] {
        assert_eq!(
            legacy_titles(&fixture, &fixture.owner_cookie, &query).await,
            expected,
            "{query}"
        );
    }
    assert_eq!(
        legacy_titles(&fixture, &member_cookie, "view=mine").await,
        ["Bravo", "Delta"]
    );

    for (query, status, code) in [
        (
            "view=bogus",
            StatusCode::UNPROCESSABLE_ENTITY,
            "validation_failed",
        ),
        (
            "priority=critical",
            StatusCode::UNPROCESSABLE_ENTITY,
            "validation_failed",
        ),
        (
            "sort=estimate",
            StatusCode::UNPROCESSABLE_ENTITY,
            "validation_failed",
        ),
        (
            "status_id=not-a-uuid",
            StatusCode::NOT_FOUND,
            "task_resource_not_found",
        ),
    ] {
        let (actual, problem) = call(
            &fixture,
            "GET",
            &format!("/api/v1/workspaces/{}/tasks?{query}", fixture.workspace_id),
            None,
        )
        .await;
        assert_eq!(actual, status, "{query}");
        assert_eq!(problem["code"], code, "{query}");
    }
}

async fn legacy_titles(fixture: &Fixture, cookie: &str, query: &str) -> Vec<String> {
    let response = fixture
        .app
        .clone()
        .oneshot(cookie_request(
            "GET",
            &format!(
                "/api/v1/workspaces/{}/tasks?sort=title&order=asc&{query}",
                fixture.workspace_id
            ),
            cookie,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK, "{query}");
    response_json(response).await["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|task| task["title"].as_str().unwrap().to_owned())
        .collect()
}

const FILTER_DAY: i64 = 86_400_000;
const FILTER_HOUR: i64 = 3_600_000;
/// 2026-09-24T12:00:00Z, a Thursday (day 20_720 since the Unix epoch).
const FILTER_NOW: i64 = 20_720 * FILTER_DAY + 12 * FILTER_HOUR;

struct FilterData {
    fixture: Fixture,
    repo: TaskRepository,
    workspace: Id,
    project: Id,
    member: Id,
    bug: Id,
    ui: Id,
    done: Id,
}

fn filter_task(project: Id, status: Id, title: &str) -> CreateTask {
    CreateTask {
        project_id: project,
        status_id: status,
        title: title.to_owned(),
        description: String::new(),
        source_url: None,
        priority: "none".to_owned(),
        position: None,
        assignee_ids: Vec::new(),
        label_ids: Vec::new(),
        due_start_at: None,
        due_at: None,
        parent_task_id: None,
    }
}

async fn status_named(fixture: &Fixture, name: &str) -> Id {
    let id: String =
        sqlx::query_scalar("SELECT id FROM task_statuses WHERE project_id = ? AND name = ?")
            .bind(&fixture.project_id)
            .bind(name)
            .fetch_one(fixture.database.pool())
            .await
            .unwrap();
    id.parse().unwrap()
}

/// Five tasks in the default project, created by the owner:
///
/// | title   | status      | priority | assignees      | labels  | due (UTC)                     | created   |
/// |---------|-------------|----------|----------------|---------|-------------------------------|-----------|
/// | Alpha   | Backlog     | urgent   | owner          | bug     | Wed 2026-09-23 10:00          | now − 10d |
/// | Bravo   | In Progress | high     | member         | bug, ui | Thu 2026-09-24 02:00 (today)  | now − 3d  |
/// | Charlie | Done        | none     | —              | —       | —                             | now − 1d  |
/// | Delta   | Todo        | low      | member         | ui      | Mon 2026-09-28 01:00          | now       |
/// | Echo    | Cancelled   | medium   | owner, member  | —       | Sun 2026-09-27 23:59:59.999   | now − 2d  |
///
/// Delta's description is "Ship 100% of the done_ish work"; every other description is empty.
async fn filter_data() -> FilterData {
    let fixture = Fixture::new().await;
    let repo = TaskRepository::new((*fixture.database).clone());
    let workspace: Id = fixture.workspace_id.parse().unwrap();
    let project: Id = fixture.project_id.parse().unwrap();
    let owner = fixture.owner_id;
    let (member, _) = add_member(&fixture, "filters@example.com").await;
    let at = TimestampMillis::from_millis;
    let now = at(FILTER_NOW);
    let bug = repo
        .create_label(
            workspace,
            owner,
            "bug".to_owned(),
            "#ff0000".to_owned(),
            "test",
            now,
        )
        .await
        .unwrap()
        .id;
    let ui = repo
        .create_label(
            workspace,
            owner,
            "ui".to_owned(),
            "#00ff00".to_owned(),
            "test",
            now,
        )
        .await
        .unwrap()
        .id;
    let backlog = status_named(&fixture, "Backlog").await;
    let todo = status_named(&fixture, "Todo").await;
    let in_progress = status_named(&fixture, "In Progress").await;
    let done = status_named(&fixture, "Done").await;
    let cancelled = status_named(&fixture, "Cancelled").await;
    let tasks = [
        (
            CreateTask {
                priority: "urgent".to_owned(),
                assignee_ids: vec![owner],
                label_ids: vec![bug],
                due_at: Some(at(20_719 * FILTER_DAY + 10 * FILTER_HOUR)),
                ..filter_task(project, backlog, "Alpha")
            },
            FILTER_NOW - 10 * FILTER_DAY,
        ),
        (
            CreateTask {
                priority: "high".to_owned(),
                assignee_ids: vec![member],
                label_ids: vec![bug, ui],
                due_at: Some(at(20_720 * FILTER_DAY + 2 * FILTER_HOUR)),
                ..filter_task(project, in_progress, "Bravo")
            },
            FILTER_NOW - 3 * FILTER_DAY,
        ),
        (
            filter_task(project, done, "Charlie"),
            FILTER_NOW - FILTER_DAY,
        ),
        (
            CreateTask {
                priority: "low".to_owned(),
                description: "Ship 100% of the done_ish work".to_owned(),
                assignee_ids: vec![member],
                label_ids: vec![ui],
                due_at: Some(at(20_724 * FILTER_DAY + FILTER_HOUR)),
                ..filter_task(project, todo, "Delta")
            },
            FILTER_NOW,
        ),
        (
            CreateTask {
                priority: "medium".to_owned(),
                assignee_ids: vec![owner, member],
                due_at: Some(at(20_724 * FILTER_DAY - 1)),
                ..filter_task(project, cancelled, "Echo")
            },
            FILTER_NOW - 2 * FILTER_DAY,
        ),
    ];
    for (input, created_at) in tasks {
        repo.create_task(workspace, owner, input, "test", at(created_at))
            .await
            .unwrap();
    }
    FilterData {
        fixture,
        repo,
        workspace,
        project,
        member,
        bug,
        ui,
        done,
    }
}

async fn page_titles(
    repo: &TaskRepository,
    workspace: Id,
    actor: Id,
    filter: &TaskFilter,
    limit: usize,
    now: i64,
) -> Vec<String> {
    let mut titles = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let page = repo
            .tasks(
                workspace,
                actor,
                filter,
                cursor.as_deref(),
                limit,
                TimestampMillis::from_millis(now),
            )
            .await
            .unwrap();
        titles.extend(page.items.into_iter().map(|task| task.title));
        match page.next_cursor {
            Some(next) => cursor = Some(next),
            None => return titles,
        }
    }
}

async fn filtered(data: &FilterData, actor: Id, tree: &Value, now: i64) -> Vec<String> {
    let filter = TaskFilter {
        tree: parse_filter(tree).unwrap(),
        show_completed: ShowCompleted::All,
        sort: TaskSort::Title,
        order: SortOrder::Asc,
        parent_task_id: None,
        sub_issues: SubIssuesDisplay::Nested,
    };
    page_titles(&data.repo, data.workspace, actor, &filter, 100, now).await
}

fn only(field: &str, operator: &str, value: Value) -> Value {
    json!({ "op": "and", "children": [{ "field": field, "operator": operator, "value": value }] })
}

#[tokio::test]
async fn filter_trees_match_every_field_and_operator() {
    let data = filter_data().await;
    let owner = data.fixture.owner_id;
    let member = data.member.to_string();
    let bug = data.bug.to_string();
    let ui = data.ui.to_string();
    let done = data.done.to_string();
    let project = data.project.to_string();
    let unknown = Id::new_v7().to_string();
    let all = vec!["Alpha", "Bravo", "Charlie", "Delta", "Echo"];
    let urgent = json!({ "field": "priority", "operator": "is", "value": ["urgent"] });
    let not_done = json!({ "field": "status_category", "operator": "is_not", "value": ["completed", "cancelled", "duplicate"] });
    let cases: Vec<(Value, Vec<&str>)> = vec![
        // status
        (
            only("status", "is", json!(["started:in progress"])),
            vec!["Bravo"],
        ),
        (
            only("status", "is", json!(["started:In Progress"])),
            vec!["Bravo"],
        ),
        (
            only(
                "status",
                "is_not",
                json!(["unstarted:backlog", "unstarted:todo"]),
            ),
            vec!["Bravo", "Charlie", "Echo"],
        ),
        (only("status", "is", json!([done])), vec!["Charlie"]),
        (only("status", "is", json!([unknown])), vec![]),
        // status_category
        (
            only("status_category", "is", json!(["completed", "cancelled"])),
            vec!["Charlie", "Echo"],
        ),
        (
            only(
                "status_category",
                "is_not",
                json!(["completed", "cancelled", "duplicate"]),
            ),
            vec!["Alpha", "Bravo", "Delta"],
        ),
        // assignee (active members only; "me" is the caller)
        (only("assignee", "is", json!(["me"])), vec!["Alpha", "Echo"]),
        (
            only("assignee", "is", json!([member])),
            vec!["Bravo", "Delta", "Echo"],
        ),
        (
            only("assignee", "is_not", json!(["me"])),
            vec!["Bravo", "Charlie", "Delta"],
        ),
        (only("assignee", "is_empty", Value::Null), vec!["Charlie"]),
        (
            only("assignee", "is_not_empty", Value::Null),
            vec!["Alpha", "Bravo", "Delta", "Echo"],
        ),
        (only("assignee", "is", json!([unknown])), vec![]),
        // creator
        (only("creator", "is", json!(["me"])), all.clone()),
        (only("creator", "is", json!([member])), vec![]),
        (only("creator", "is_not", json!(["me"])), vec![]),
        // label
        (
            only("label", "includes_any", json!([bug])),
            vec!["Alpha", "Bravo"],
        ),
        (
            only("label", "includes_all", json!([bug, ui])),
            vec!["Bravo"],
        ),
        (only("label", "includes_all", json!([bug, unknown])), vec![]),
        (
            only("label", "excludes", json!([bug])),
            vec!["Charlie", "Delta", "Echo"],
        ),
        (
            only("label", "is_empty", Value::Null),
            vec!["Charlie", "Echo"],
        ),
        (
            only("label", "is_not_empty", Value::Null),
            vec!["Alpha", "Bravo", "Delta"],
        ),
        (only("label", "includes_any", json!([unknown])), vec![]),
        // priority
        (
            only("priority", "is", json!(["urgent", "high"])),
            vec!["Alpha", "Bravo"],
        ),
        (
            only("priority", "is_not", json!(["none"])),
            vec!["Alpha", "Bravo", "Delta", "Echo"],
        ),
        // project
        (only("project", "is", json!([project])), all.clone()),
        (only("project", "is_not", json!([project])), vec![]),
        (only("project", "is", json!([unknown])), vec![]),
        // due_date: before is exclusive, after D means >= D + 1 day, between is inclusive of both days
        (
            only("due_date", "before", json!({ "relative": "today" })),
            vec!["Alpha"],
        ),
        (
            only("due_date", "after", json!({ "relative": "today" })),
            vec!["Delta", "Echo"],
        ),
        (
            only(
                "due_date",
                "between",
                json!([{ "relative": "today" }, { "relative": "today" }]),
            ),
            vec!["Bravo"],
        ),
        (
            only(
                "due_date",
                "between",
                json!([{ "relative": "start_of_week" }, { "relative": "end_of_week" }]),
            ),
            vec!["Alpha", "Bravo", "Echo"],
        ),
        (
            only("due_date", "before", json!({ "absolute": "2026-09-24" })),
            vec!["Alpha"],
        ),
        (
            only(
                "due_date",
                "after",
                json!({ "relative": "today", "offset_days": 3 }),
            ),
            vec!["Delta"],
        ),
        (only("due_date", "is_empty", Value::Null), vec!["Charlie"]),
        (
            only("due_date", "is_not_empty", Value::Null),
            vec!["Alpha", "Bravo", "Delta", "Echo"],
        ),
        // created_at / updated_at
        (
            only(
                "created_at",
                "before",
                json!({ "relative": "today", "offset_days": -2 }),
            ),
            vec!["Alpha", "Bravo"],
        ),
        (
            only(
                "created_at",
                "after",
                json!({ "relative": "today", "offset_days": -1 }),
            ),
            vec!["Delta"],
        ),
        (
            only("updated_at", "after", json!({ "absolute": "2026-09-23" })),
            vec!["Delta"],
        ),
        // text: case-insensitive, LIKE wildcards are literal
        (only("text", "contains", json!("ALPHA")), vec!["Alpha"]),
        (only("text", "contains", json!("100%")), vec!["Delta"]),
        (only("text", "contains", json!("%")), vec!["Delta"]),
        (only("text", "contains", json!("_ish")), vec!["Delta"]),
        // groups
        (
            json!({ "op": "or", "children": [urgent.clone(), { "field": "label", "operator": "includes_any", "value": [ui] }] }),
            vec!["Alpha", "Bravo", "Delta"],
        ),
        (
            json!({ "op": "and", "children": [
                not_done.clone(),
                { "op": "or", "children": [
                    { "field": "assignee", "operator": "is_empty" },
                    { "op": "and", "children": [{ "field": "priority", "operator": "is", "value": ["low"] }] }
                ] }
            ] }),
            vec!["Delta"],
        ),
        (json!({ "op": "and", "children": [] }), all.clone()),
        (
            json!({ "op": "and", "children": [{ "op": "or", "children": [] }, urgent.clone()] }),
            vec!["Alpha"],
        ),
        (
            json!({ "op": "or", "children": [{ "op": "and", "children": [] }, urgent.clone()] }),
            all.clone(),
        ),
    ];
    for (tree, expected) in cases {
        assert_eq!(
            filtered(&data, owner, &tree, FILTER_NOW).await,
            expected,
            "{tree}"
        );
    }
    assert_eq!(
        filtered(
            &data,
            data.member,
            &only("assignee", "is", json!(["me"])),
            FILTER_NOW
        )
        .await,
        ["Bravo", "Delta", "Echo"]
    );
}

#[tokio::test]
async fn presets_resolve_days_and_weeks_in_utc() {
    let data = filter_data().await;
    let owner = data.fixture.owner_id;
    let preset = |name: &str| serde_json::to_value(preset_filter(name).unwrap()).unwrap();
    let sunday_last_ms = 20_724 * FILTER_DAY - 1;
    let monday_first_ms = 20_724 * FILTER_DAY;
    for (name, actor, now, expected) in [
        ("overdue", owner, FILTER_NOW, vec!["Alpha"]),
        ("due_soon", owner, FILTER_NOW, vec!["Bravo", "Delta"]),
        ("current_week", owner, FILTER_NOW, vec!["Alpha", "Bravo"]),
        ("mine", owner, FILTER_NOW, vec!["Alpha", "Echo"]),
        (
            "mine",
            data.member,
            FILTER_NOW,
            vec!["Bravo", "Delta", "Echo"],
        ),
        ("my_week", owner, FILTER_NOW, vec!["Alpha"]),
        ("my_week", data.member, FILTER_NOW, vec!["Bravo"]),
        ("overdue", owner, sunday_last_ms, vec!["Alpha", "Bravo"]),
        (
            "current_week",
            owner,
            sunday_last_ms,
            vec!["Alpha", "Bravo"],
        ),
        ("current_week", owner, monday_first_ms, vec!["Delta"]),
        ("overdue", owner, monday_first_ms, vec!["Alpha", "Bravo"]),
    ] {
        assert_eq!(
            filtered(&data, actor, &preset(name), now).await,
            expected,
            "{name} at {now}"
        );
    }
}

#[tokio::test]
async fn show_completed_windows_use_completed_at() {
    let fixture = Fixture::new().await;
    let repo = TaskRepository::new((*fixture.database).clone());
    let workspace: Id = fixture.workspace_id.parse().unwrap();
    let project: Id = fixture.project_id.parse().unwrap();
    let backlog: Id = fixture.status_id.parse().unwrap();
    let done = status_named(&fixture, "Done").await;
    let cancelled = status_named(&fixture, "Cancelled").await;
    for (title, status, age_days) in [
        ("Open", backlog, 0),
        ("Done recently", done, 2),
        ("Cancelled recently", cancelled, 1),
        ("Done last month", done, 20),
        ("Done long ago", done, 40),
    ] {
        repo.create_task(
            workspace,
            fixture.owner_id,
            filter_task(project, status, title),
            "test",
            TimestampMillis::from_millis(FILTER_NOW - age_days * FILTER_DAY),
        )
        .await
        .unwrap();
    }
    for (show, expected) in [
        (
            ShowCompleted::All,
            vec![
                "Cancelled recently",
                "Done last month",
                "Done long ago",
                "Done recently",
                "Open",
            ],
        ),
        (
            ShowCompleted::PastMonth,
            vec![
                "Cancelled recently",
                "Done last month",
                "Done recently",
                "Open",
            ],
        ),
        (
            ShowCompleted::PastWeek,
            vec!["Cancelled recently", "Done recently", "Open"],
        ),
        (ShowCompleted::None, vec!["Open"]),
    ] {
        let filter = TaskFilter {
            tree: FilterGroup::default(),
            show_completed: show,
            sort: TaskSort::Title,
            order: SortOrder::Asc,
            parent_task_id: None,
            sub_issues: SubIssuesDisplay::Nested,
        };
        assert_eq!(
            page_titles(&repo, workspace, fixture.owner_id, &filter, 100, FILTER_NOW).await,
            expected,
            "{show:?}"
        );
    }
}

#[tokio::test]
async fn due_date_order_keeps_empty_dates_last_across_pages() {
    let fixture = Fixture::new().await;
    let repo = TaskRepository::new((*fixture.database).clone());
    let workspace: Id = fixture.workspace_id.parse().unwrap();
    let project: Id = fixture.project_id.parse().unwrap();
    let backlog: Id = fixture.status_id.parse().unwrap();
    let mut undated = Vec::new();
    for (title, due_day) in [
        ("d2", Some(20_722)),
        ("n1", None),
        ("d1", Some(20_721)),
        ("n2", None),
        ("d3", Some(20_723)),
    ] {
        let mut input = filter_task(project, backlog, title);
        input.due_at = due_day.map(|day: i64| TimestampMillis::from_millis(day * FILTER_DAY));
        let task = repo
            .create_task(
                workspace,
                fixture.owner_id,
                input,
                "test",
                TimestampMillis::from_millis(FILTER_NOW),
            )
            .await
            .unwrap();
        if due_day.is_none() {
            undated.push((task.task.id, title));
        }
    }
    // Ties (both undated) fall back to the id, in the same direction as the sort.
    undated.sort();
    let ascending: Vec<&str> = ["d1", "d2", "d3"]
        .into_iter()
        .chain(undated.iter().map(|(_, title)| *title))
        .collect();
    let descending: Vec<&str> = ["d3", "d2", "d1"]
        .into_iter()
        .chain(undated.iter().rev().map(|(_, title)| *title))
        .collect();
    for (order, expected) in [(SortOrder::Asc, ascending), (SortOrder::Desc, descending)] {
        let filter = TaskFilter {
            tree: FilterGroup::default(),
            show_completed: ShowCompleted::All,
            sort: TaskSort::DueDate,
            order,
            parent_task_id: None,
            sub_issues: SubIssuesDisplay::Nested,
        };
        assert_eq!(
            page_titles(&repo, workspace, fixture.owner_id, &filter, 2, FILTER_NOW).await,
            expected
        );
    }

    // A cursor only continues the query that produced it.
    let by_due = TaskFilter {
        tree: FilterGroup::default(),
        show_completed: ShowCompleted::All,
        sort: TaskSort::DueDate,
        order: SortOrder::Asc,
        parent_task_id: None,
        sub_issues: SubIssuesDisplay::Nested,
    };
    let now = TimestampMillis::from_millis(FILTER_NOW);
    let first = repo
        .tasks(workspace, fixture.owner_id, &by_due, None, 2, now)
        .await
        .unwrap();
    let mine = TaskFilter {
        tree: preset_filter("mine").unwrap(),
        ..by_due.clone()
    };
    assert!(matches!(
        repo.tasks(
            workspace,
            fixture.owner_id,
            &mine,
            first.next_cursor.as_deref(),
            2,
            now
        )
        .await,
        Err(TaskError::InvalidCursor)
    ));
}

fn query_uri(fixture: &Fixture) -> String {
    format!("/api/v1/workspaces/{}/tasks/query", fixture.workspace_id)
}

fn query_body(filter: Value, order_by: &str, direction: &str, show_completed: &str) -> Value {
    json!({
        "filter": filter,
        "order_by": order_by,
        "order_direction": direction,
        "show_completed": show_completed
    })
}

fn query_titles(page: &Value) -> Vec<String> {
    page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|task| task["title"].as_str().unwrap().to_owned())
        .collect()
}

#[tokio::test]
async fn task_query_filters_orders_and_pages() {
    let fixture = Fixture::new().await;
    let done = status_id_by_category(&fixture, &fixture.project_id, "completed").await;
    let now = TimestampMillis::now().as_millis();
    let tomorrow = TimestampMillis::from_millis(now + 86_400_000);
    let tomorrow_later = TimestampMillis::from_millis(now + 86_400_000 + 3_600_000);
    let next_week = TimestampMillis::from_millis(now + 7 * 86_400_000);
    for body in [
        json!({"project_id": fixture.project_id, "status_id": fixture.status_id, "title": "Alpha", "priority": "urgent", "due_at": next_week}),
        json!({"project_id": fixture.project_id, "status_id": fixture.status_id, "title": "Beta", "priority": "urgent"}),
        json!({"project_id": fixture.project_id, "status_id": done, "title": "Gamma", "priority": "urgent", "due_at": tomorrow}),
        json!({"project_id": fixture.project_id, "status_id": fixture.status_id, "title": "Delta", "priority": "low", "due_at": tomorrow_later}),
    ] {
        let (status, _) = call(
            &fixture,
            "POST",
            &format!("/api/v1/workspaces/{}/tasks", fixture.workspace_id),
            Some(body),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
    }

    let urgent = json!({"op": "and", "children": [{"field": "priority", "operator": "is", "value": ["urgent"]}]});
    let mut body = query_body(urgent.clone(), "title", "asc", "all");
    body["limit"] = json!(2);
    let (status, first) = call(&fixture, "POST", &query_uri(&fixture), Some(body.clone())).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(query_titles(&first), ["Alpha", "Beta"]);
    body["cursor"] = first["next_cursor"].clone();
    let (status, second) = call(&fixture, "POST", &query_uri(&fixture), Some(body)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(query_titles(&second), ["Gamma"]);
    assert!(second["next_cursor"].is_null());

    let (_, open) = call(
        &fixture,
        "POST",
        &query_uri(&fixture),
        Some(query_body(urgent, "title", "asc", "none")),
    )
    .await;
    assert_eq!(query_titles(&open), ["Alpha", "Beta"]);

    let everything = json!({"op": "and", "children": []});
    let (_, ascending) = call(
        &fixture,
        "POST",
        &query_uri(&fixture),
        Some(query_body(everything.clone(), "due_date", "asc", "all")),
    )
    .await;
    assert_eq!(
        query_titles(&ascending),
        ["Gamma", "Delta", "Alpha", "Beta"]
    );
    let (_, descending) = call(
        &fixture,
        "POST",
        &query_uri(&fixture),
        Some(query_body(everything.clone(), "due_date", "desc", "all")),
    )
    .await;
    assert_eq!(
        query_titles(&descending),
        ["Alpha", "Delta", "Gamma", "Beta"]
    );

    // Manual order has no direction.
    let (_, manual_asc) = call(
        &fixture,
        "POST",
        &query_uri(&fixture),
        Some(query_body(everything.clone(), "manual", "asc", "all")),
    )
    .await;
    let (_, manual_desc) = call(
        &fixture,
        "POST",
        &query_uri(&fixture),
        Some(query_body(everything.clone(), "manual", "desc", "all")),
    )
    .await;
    assert_eq!(query_titles(&manual_asc), query_titles(&manual_desc));

    // A cursor only continues the query that produced it.
    let mut other = query_body(everything, "title", "asc", "all");
    other["cursor"] = first["next_cursor"].clone();
    let (status, problem) = call(&fixture, "POST", &query_uri(&fixture), Some(other)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(problem["code"], "invalid_cursor");
}

#[tokio::test]
async fn task_query_rejects_invalid_filters_with_a_json_path() {
    let fixture = Fixture::new().await;
    let ok = json!({"field": "priority", "operator": "is", "value": ["high"]});
    let too_deep = json!({"op": "and", "children": [
        {"op": "and", "children": [
            {"op": "and", "children": [
                {"op": "and", "children": [
                    {"op": "and", "children": []}
                ]}
            ]}
        ]}
    ]});
    for (filter, path) in [
        (
            json!({"op": "and", "children": [ok.clone(), ok.clone(), {"field": "priority", "operator": "is", "value": ["critical"]}]}),
            "filter.children[2].value[0]",
        ),
        (
            json!({"op": "and", "children": [{"field": "priority", "operator": "contains", "value": "x"}]}),
            "filter.children[0].operator",
        ),
        (
            json!({"op": "and", "children": [{"field": "estimate", "operator": "is", "value": ["1"]}]}),
            "filter.children[0].field",
        ),
        (
            json!({"op": "and", "children": [], "mode": "all"}),
            "filter.mode",
        ),
        (
            too_deep,
            "filter.children[0].children[0].children[0].children[0]",
        ),
        (
            json!({"op": "and", "children": [{"field": "text", "operator": "contains", "value": "x".repeat(201)}]}),
            "filter.children[0].value",
        ),
    ] {
        let response = fixture
            .app
            .clone()
            .oneshot(json_request(
                "POST",
                &query_uri(&fixture),
                &fixture.owner_cookie,
                query_body(filter, "manual", "asc", "all"),
            ))
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "{path}"
        );
        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            "application/problem+json"
        );
        let problem = response_json(response).await;
        assert_eq!(problem["code"], "invalid_filter", "{path}");
        assert_eq!(problem["path"], path);
        assert!(
            problem["detail"]
                .as_str()
                .is_some_and(|detail| !detail.is_empty())
        );
    }

    let (status, problem) = call(
        &fixture,
        "POST",
        &query_uri(&fixture),
        Some(json!({"filter": {"op": "and", "children": []}, "order_by": "title", "order_direction": "asc"})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(problem["code"], "invalid_request");
}

#[tokio::test]
async fn task_query_resolves_me_for_the_caller_and_hides_foreign_workspaces() {
    let fixture = Fixture::new().await;
    let (member_id, member_cookie) = add_member(&fixture, "query-member@example.com").await;
    for (title, assignee) in [("Mine", member_id), ("Theirs", fixture.owner_id)] {
        let (status, _) = call(
            &fixture,
            "POST",
            &format!("/api/v1/workspaces/{}/tasks", fixture.workspace_id),
            Some(
                json!({"project_id": fixture.project_id, "status_id": fixture.status_id,
                        "title": title, "assignee_ids": [assignee.to_string()]}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
    }
    let mine = query_body(
        json!({"op": "and", "children": [{"field": "assignee", "operator": "is", "value": ["me"]}]}),
        "title",
        "asc",
        "all",
    );
    let response = fixture
        .app
        .clone()
        .oneshot(json_request(
            "POST",
            &query_uri(&fixture),
            &member_cookie,
            mine.clone(),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(query_titles(&response_json(response).await), ["Mine"]);
    let (_, owner_page) = call(&fixture, "POST", &query_uri(&fixture), Some(mine.clone())).await;
    assert_eq!(query_titles(&owner_page), ["Theirs"]);

    let foreign = create_workspace(&fixture, "Elsewhere").await;
    let response = fixture
        .app
        .clone()
        .oneshot(json_request(
            "POST",
            &format!("/api/v1/workspaces/{foreign}/tasks/query"),
            &member_cookie,
            mine,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        response_json(response).await["code"],
        "task_resource_not_found"
    );
}

// ---- Sub-issues ---------------------------------------------------------------------------

async fn create_task_in(
    fixture: &Fixture,
    project_id: &str,
    title: &str,
    parent: Option<&str>,
) -> Value {
    let status_id = status_id_by_category(fixture, project_id, "unstarted").await;
    let mut body = json!({"project_id": project_id, "status_id": status_id, "title": title});
    if let Some(parent) = parent {
        body["parent_task_id"] = json!(parent);
    }
    let (status, task) = call(
        fixture,
        "POST",
        &format!("/api/v1/workspaces/{}/tasks", fixture.workspace_id),
        Some(body),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{task}");
    task
}

async fn create_sub_issue(fixture: &Fixture, title: &str, parent: Option<&str>) -> Value {
    create_task_in(fixture, &fixture.project_id, title, parent).await
}

async fn fetch_task(fixture: &Fixture, task_id: &str) -> Value {
    let (status, task) = call(fixture, "GET", &task_uri(fixture, task_id), None).await;
    assert_eq!(status, StatusCode::OK, "{task}");
    task
}

/// PATCHes `task_id` at its current version.
async fn patch_current(fixture: &Fixture, task_id: &str, mut body: Value) -> (StatusCode, Value) {
    body["expected_version"] = fetch_task(fixture, task_id).await["version"].clone();
    call(fixture, "PATCH", &task_uri(fixture, task_id), Some(body)).await
}

fn bulk_uri(fixture: &Fixture) -> String {
    format!("/api/v1/workspaces/{}/tasks/bulk", fixture.workspace_id)
}

async fn project_key_of(fixture: &Fixture, project_id: &str) -> String {
    sqlx::query_scalar("SELECT project_key FROM projects WHERE id = ?")
        .bind(project_id)
        .fetch_one(fixture.database.pool())
        .await
        .unwrap()
}

async fn parent_of(fixture: &Fixture, task_id: &str) -> Option<String> {
    sqlx::query_scalar("SELECT parent_task_id FROM tasks WHERE id = ?")
        .bind(task_id)
        .fetch_one(fixture.database.pool())
        .await
        .unwrap()
}

#[tokio::test]
async fn sub_issue_parent_is_set_on_create_update_and_bulk() {
    let fixture = Fixture::new().await;
    let parent = create_sub_issue(&fixture, "Parent", None).await;
    let parent_id = id_of(&parent).to_owned();
    assert_eq!(parent["parent_task_id"], Value::Null);
    assert_eq!(parent["parent"], Value::Null);
    assert_eq!(parent["sub_issue_count"], 0);
    assert_eq!(parent["sub_issue_closed_count"], 0);

    let child = create_sub_issue(&fixture, "Child", Some(&parent_id)).await;
    let key = project_key_of(&fixture, &fixture.project_id).await;
    assert_eq!(child["parent_task_id"], parent_id.as_str());
    assert_eq!(
        child["parent"],
        json!({"id": parent_id, "project_id": fixture.project_id, "project_key": key, "title": "Parent"})
    );

    // Update attaches and detaches; each real change is audited once.
    let other = create_sub_issue(&fixture, "Other", None).await;
    let other_id = id_of(&other).to_owned();
    let (status, attached) =
        patch_current(&fixture, &other_id, json!({"parent_task_id": parent_id})).await;
    assert_eq!(status, StatusCode::OK, "{attached}");
    assert_eq!(attached["parent_task_id"], parent_id.as_str());
    assert_eq!(attached["parent"]["title"], "Parent");
    let (status, detached) =
        patch_current(&fixture, &other_id, json!({"parent_task_id": null})).await;
    assert_eq!(status, StatusCode::OK, "{detached}");
    assert_eq!(detached["parent_task_id"], Value::Null);
    assert_eq!(detached["parent"], Value::Null);
    let (status, _) = patch_current(&fixture, &other_id, json!({"title": "Other renamed"})).await;
    assert_eq!(status, StatusCode::OK);
    let changes: Vec<String> = sqlx::query_scalar(
        "SELECT metadata_json FROM audit_events WHERE action = 'task.parent_changed' AND resource_id = ?",
    )
    .bind(&other_id)
    .fetch_all(fixture.database.pool())
    .await
    .unwrap();
    let mut changes: Vec<Value> = changes
        .iter()
        .map(|metadata| serde_json::from_str(metadata).unwrap())
        .collect();
    changes.sort_by_key(|change| change["from"].is_string());
    let from_to: Vec<Value> = changes
        .iter()
        .map(|change| json!({"from": change["from"], "to": change["to"]}))
        .collect();
    assert_eq!(
        from_to,
        [
            json!({"from": null, "to": parent_id}),
            json!({"from": parent_id, "to": null})
        ]
    );
    // Project ids travel with the ids so the activity feed can build identifiers.
    assert!(changes[0]["to_project_id"].is_string());
    assert_eq!(changes[0]["from_project_id"], Value::Null);
    assert!(changes[1]["from_project_id"].is_string());
    assert_eq!(changes[1]["to_project_id"], Value::Null);

    // Bulk.
    let first = create_sub_issue(&fixture, "Bulk one", None).await;
    let second = create_sub_issue(&fixture, "Bulk two", None).await;
    let (status, bulk) = call(
        &fixture,
        "POST",
        &bulk_uri(&fixture),
        Some(json!({"updates": [
            {"id": id_of(&first), "expected_version": 0, "parent_task_id": parent_id},
            {"id": id_of(&second), "expected_version": 0, "parent_task_id": parent_id}
        ]})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{bulk}");
    assert!(
        bulk["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|task| task["parent_task_id"] == parent_id.as_str())
    );

    // Counts cover direct live children; a cancelled child counts as closed.
    let cancelled = status_id_by_category(&fixture, &fixture.project_id, "cancelled").await;
    let (status, _) = patch_current(&fixture, id_of(&child), json!({"status_id": cancelled})).await;
    assert_eq!(status, StatusCode::OK);
    let grandchild = create_sub_issue(&fixture, "Grandchild", Some(id_of(&child))).await;
    let parent = fetch_task(&fixture, &parent_id).await;
    assert_eq!(parent["sub_issue_count"], 3);
    assert_eq!(parent["sub_issue_closed_count"], 1);
    assert_eq!(parent["ancestors"], json!([]));

    // Direct children only, on both list endpoints; list items carry no ancestors.
    let (status, listed) = call(
        &fixture,
        "GET",
        &format!(
            "/api/v1/workspaces/{}/tasks?parent_task_id={parent_id}&sort=title",
            fixture.workspace_id
        ),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    assert_eq!(query_titles(&listed), ["Bulk one", "Bulk two", "Child"]);
    assert!(listed["items"][0].get("ancestors").is_none());
    let mut body = query_body(json!({"op": "and", "children": []}), "title", "asc", "all");
    body["parent_task_id"] = json!(id_of(&child));
    let (status, queried) = call(&fixture, "POST", &query_uri(&fixture), Some(body)).await;
    assert_eq!(status, StatusCode::OK, "{queried}");
    assert_eq!(query_titles(&queried), ["Grandchild"]);
    assert_eq!(queried["items"][0]["id"], id_of(&grandchild));
}

#[tokio::test]
async fn sub_issue_parent_rejects_cycles_and_invalid_parents() {
    let fixture = Fixture::new().await;
    let a = create_sub_issue(&fixture, "A", None).await;
    let b = create_sub_issue(&fixture, "B", Some(id_of(&a))).await;
    let c = create_sub_issue(&fixture, "C", Some(id_of(&b))).await;

    let (status, problem) =
        patch_current(&fixture, id_of(&a), json!({"parent_task_id": id_of(&a)})).await;
    assert_eq!(
        (status, problem["code"].as_str()),
        (StatusCode::UNPROCESSABLE_ENTITY, Some("parent_cycle"))
    );
    // Deep chain A → B → C: making C the parent of A is a cycle.
    let (status, problem) =
        patch_current(&fixture, id_of(&a), json!({"parent_task_id": id_of(&c)})).await;
    assert_eq!(
        (status, problem["code"].as_str()),
        (StatusCode::UNPROCESSABLE_ENTITY, Some("parent_cycle"))
    );
    assert_eq!(fetch_task(&fixture, id_of(&a)).await["version"], 0);

    // A cycle built inside one bulk request rolls the whole request back.
    let x = create_sub_issue(&fixture, "X", None).await;
    let y = create_sub_issue(&fixture, "Y", None).await;
    let (status, problem) = call(
        &fixture,
        "POST",
        &bulk_uri(&fixture),
        Some(json!({"updates": [
            {"id": id_of(&x), "expected_version": 0, "parent_task_id": id_of(&y)},
            {"id": id_of(&y), "expected_version": 0, "parent_task_id": id_of(&x)}
        ]})),
    )
    .await;
    assert_eq!(
        (status, problem["code"].as_str()),
        (StatusCode::UNPROCESSABLE_ENTITY, Some("parent_cycle"))
    );
    assert_eq!(parent_of(&fixture, id_of(&x)).await, None);

    // Missing, trashed, other-workspace and trashed-project parents are parent_invalid.
    let trashed = create_sub_issue(&fixture, "Trashed", None).await;
    let (status, _) = call(
        &fixture,
        "DELETE",
        &format!("{}?expected_version=0", task_uri(&fixture, id_of(&trashed))),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let other_workspace = create_workspace(&fixture, "Elsewhere").await;
    let (status, foreign_project) = call(
        &fixture,
        "POST",
        &format!("/api/v1/workspaces/{other_workspace}/projects"),
        Some(json!({"name": "Else", "key": "ELS", "color": "#123456"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{foreign_project}");
    let foreign_status =
        status_id_by_category(&fixture, id_of(&foreign_project), "unstarted").await;
    let (status, foreign) = call(
        &fixture,
        "POST",
        &format!("/api/v1/workspaces/{other_workspace}/tasks"),
        Some(json!({"project_id": id_of(&foreign_project), "status_id": foreign_status, "title": "Foreign"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{foreign}");
    let archived_project = create_project(&fixture, "ARC").await;
    let archived = create_task_in(&fixture, &archived_project, "Archived", None).await;
    let (status, _) = call(
        &fixture,
        "DELETE",
        &format!(
            "/api/v1/workspaces/{}/projects/{archived_project}?expected_version=0",
            fixture.workspace_id
        ),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    for parent in [
        Id::new_v7().to_string(),
        id_of(&trashed).to_owned(),
        id_of(&foreign).to_owned(),
        id_of(&archived).to_owned(),
    ] {
        let (status, problem) = call(
            &fixture,
            "POST",
            &format!("/api/v1/workspaces/{}/tasks", fixture.workspace_id),
            Some(json!({"project_id": fixture.project_id, "status_id": fixture.status_id, "title": "Orphan", "parent_task_id": parent.as_str()})),
        )
        .await;
        assert_eq!(
            (status, problem["code"].as_str()),
            (StatusCode::UNPROCESSABLE_ENTITY, Some("parent_invalid")),
            "create under {parent}"
        );
        let (status, problem) = patch_current(
            &fixture,
            id_of(&x),
            json!({"parent_task_id": parent.as_str()}),
        )
        .await;
        assert_eq!(
            (status, problem["code"].as_str()),
            (StatusCode::UNPROCESSABLE_ENTITY, Some("parent_invalid")),
            "move under {parent}"
        );
    }

    // Moving a task to another project keeps its parent and its children.
    let moved_project = create_project(&fixture, "MOV").await;
    let moved_status = status_id_by_category(&fixture, &moved_project, "unstarted").await;
    let (status, moved) = patch_current(
        &fixture,
        id_of(&b),
        json!({"project_id": moved_project, "status_id": moved_status}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{moved}");
    assert_eq!(moved["parent_task_id"], id_of(&a));
    assert_eq!(moved["sub_issue_count"], 1);
    assert_eq!(
        parent_of(&fixture, id_of(&c)).await.as_deref(),
        Some(id_of(&b))
    );

    // The database refuses a self-parent even without the repository.
    let error = sqlx::query("UPDATE tasks SET parent_task_id = id WHERE id = ?")
        .bind(id_of(&a))
        .execute(fixture.database.pool())
        .await
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("task parent must be another task in the same workspace"),
        "{error}"
    );
}

#[tokio::test]
async fn sub_issue_ancestors_are_listed_root_first_on_the_task_endpoint() {
    let fixture = Fixture::new().await;
    let other_project = create_project(&fixture, "ANC").await;
    let root = create_sub_issue(&fixture, "Root", None).await;
    let middle = create_task_in(&fixture, &other_project, "Middle", Some(id_of(&root))).await;
    let leaf = create_sub_issue(&fixture, "Leaf", Some(id_of(&middle))).await;
    let root_key = project_key_of(&fixture, &fixture.project_id).await;

    let leaf = fetch_task(&fixture, id_of(&leaf)).await;
    assert_eq!(
        leaf["ancestors"],
        json!([
            {"id": id_of(&root), "project_id": fixture.project_id, "project_key": root_key, "title": "Root"},
            {"id": id_of(&middle), "project_id": other_project, "project_key": "ANC", "title": "Middle"}
        ])
    );
    assert_eq!(leaf["parent"]["project_key"], "ANC");
    assert_eq!(
        fetch_task(&fixture, id_of(&root)).await["ancestors"],
        json!([])
    );
}

#[tokio::test]
async fn sub_issue_under_a_hidden_parent_lists_as_top_level_everywhere_and_ancestors_stop_there() {
    let fixture = Fixture::new().await;
    let other = create_project(&fixture, "HID").await;
    let root = create_sub_issue(&fixture, "Root", None).await;
    let hidden = create_task_in(&fixture, &other, "Hidden", Some(id_of(&root))).await;
    let visible = create_sub_issue(&fixture, "Visible", Some(id_of(&hidden))).await;
    let leaf = create_sub_issue(&fixture, "Leaf", Some(id_of(&visible))).await;
    let everything = json!({"op": "and", "children": []});
    assert_eq!(
        titles_for(&fixture, everything.clone(), "hidden").await,
        ["Root"]
    );
    let hidden_id = id_of(&hidden).to_owned();
    assert_eq!(
        titles_for(&fixture, only("parent", "is", json!([hidden_id])), "nested").await,
        ["Visible"]
    );
    let (status, _) = call(
        &fixture,
        "DELETE",
        &format!(
            "/api/v1/workspaces/{}/projects/{other}?expected_version=0",
            fixture.workspace_id
        ),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Visible shows no parent, so the walk from Leaf ends there instead of skipping to Root.
    let visible_key = project_key_of(&fixture, &fixture.project_id).await;
    let leaf = fetch_task(&fixture, id_of(&leaf)).await;
    assert_eq!(
        leaf["ancestors"],
        json!([{"id": id_of(&visible), "project_id": fixture.project_id, "project_key": visible_key, "title": "Visible"}])
    );
    let visible = fetch_task(&fixture, id_of(&visible)).await;
    assert_eq!(visible["parent"], Value::Null);
    assert_eq!(visible["ancestors"], json!([]));
    // `hidden` keeps Visible reachable: it lists as top-level, Leaf stays under it.
    assert_eq!(
        titles_for(&fixture, everything, "hidden").await,
        ["Root", "Visible"]
    );
    // The parent filter reads the same visible parent.
    let cases = [
        (
            only("parent", "is", json!(["none"])),
            vec!["Root", "Visible"],
        ),
        (only("parent", "is", json!([hidden_id])), vec![]),
        (
            only("parent", "is_not", json!([hidden_id])),
            vec!["Leaf", "Root", "Visible"],
        ),
        (only("parent", "is_not", json!(["none"])), vec!["Leaf"]),
        (only("parent", "is", json!([visible["id"]])), vec!["Leaf"]),
    ];
    for (filter, expected) in cases {
        assert_eq!(
            titles_for(&fixture, filter.clone(), "nested").await,
            expected,
            "{filter}"
        );
    }
}

#[tokio::test]
async fn sub_issue_restored_under_a_parent_in_a_trashed_project_keeps_the_link() {
    let fixture = Fixture::new().await;
    let other = create_project(&fixture, "KEP").await;
    let parent = create_task_in(&fixture, &other, "Parent elsewhere", None).await;
    let child = create_sub_issue(&fixture, "Child", Some(id_of(&parent))).await;
    let (status, _) = call(
        &fixture,
        "DELETE",
        &format!("{}?expected_version=0", task_uri(&fixture, id_of(&child))),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let project_uri = format!(
        "/api/v1/workspaces/{}/projects/{other}",
        fixture.workspace_id
    );
    let (status, _) = call(
        &fixture,
        "DELETE",
        &format!("{project_uri}?expected_version=0"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // The parent itself is not in the trash, so the link stays, hidden like the parent.
    let (status, restored) = call(
        &fixture,
        "POST",
        &format!("{}/restore", task_uri(&fixture, id_of(&child))),
        Some(json!({"expected_version": 1})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{restored}");
    assert_eq!(restored["parent_task_id"], id_of(&parent));
    assert_eq!(restored["parent"], Value::Null);
    assert_eq!(
        fetch_task(&fixture, id_of(&child)).await["ancestors"],
        json!([])
    );

    let (status, project) = call(
        &fixture,
        "POST",
        &format!("{project_uri}/restore"),
        Some(json!({"expected_version": 1})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{project}");
    assert_eq!(
        fetch_task(&fixture, id_of(&child)).await["parent"]["id"],
        id_of(&parent)
    );
}

#[tokio::test]
async fn sub_issue_delete_trashes_the_subtree_and_restore_returns_what_was_trashed_with_it() {
    let fixture = Fixture::new().await;
    let pool = fixture.database.pool();
    let parent = create_sub_issue(&fixture, "Parent", None).await;
    let child = create_sub_issue(&fixture, "Child", Some(id_of(&parent))).await;
    let grandchild = create_sub_issue(&fixture, "Grandchild", Some(id_of(&child))).await;
    let earlier = create_sub_issue(&fixture, "Earlier", Some(id_of(&parent))).await;
    // Earlier goes to the trash on its own, strictly before its parent.
    let (status, _) = call(
        &fixture,
        "DELETE",
        &format!("{}?expected_version=0", task_uri(&fixture, id_of(&earlier))),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    sqlx::query("UPDATE tasks SET deleted_at = deleted_at - 1000 WHERE id = ?")
        .bind(id_of(&earlier))
        .execute(pool)
        .await
        .unwrap();

    let (status, _) = call(
        &fixture,
        "DELETE",
        &format!("{}?expected_version=0", task_uri(&fixture, id_of(&parent))),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let stamps: Vec<Option<i64>> =
        sqlx::query_scalar("SELECT deleted_at FROM tasks WHERE id IN (?, ?, ?)")
            .bind(id_of(&parent))
            .bind(id_of(&child))
            .bind(id_of(&grandchild))
            .fetch_all(pool)
            .await
            .unwrap();
    assert_eq!(stamps.len(), 3);
    assert!(
        stamps[0].is_some() && stamps.iter().all(|stamp| *stamp == stamps[0]),
        "{stamps:?}"
    );
    let deleted_audits: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events WHERE action = 'task.deleted' AND resource_id IN (?, ?, ?)",
    )
    .bind(id_of(&parent))
    .bind(id_of(&child))
    .bind(id_of(&grandchild))
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(deleted_audits, 3);

    let (status, trash) = call(
        &fixture,
        "GET",
        &format!("/api/v1/workspaces/{}/tasks/trash", fixture.workspace_id),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{trash}");
    let descendants = |id: &str| {
        trash["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|task| task["id"] == id)
            .unwrap()["trashed_descendant_count"]
            .clone()
    };
    assert_eq!(descendants(id_of(&parent)), 2);
    assert_eq!(descendants(id_of(&child)), 1);
    assert_eq!(descendants(id_of(&earlier)), 0);
    // One-item pages carry the same counts as the full page.
    let mut paged = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let mut uri = format!(
            "/api/v1/workspaces/{}/tasks/trash?limit=1",
            fixture.workspace_id
        );
        if let Some(cursor) = &cursor {
            uri.push_str(&format!("&cursor={cursor}"));
        }
        let (status, page) = call(&fixture, "GET", &uri, None).await;
        assert_eq!(status, StatusCode::OK, "{page}");
        paged.extend(page["items"].as_array().unwrap().iter().cloned());
        match page["next_cursor"].as_str() {
            Some(next) => cursor = Some(next.to_owned()),
            None => break,
        }
    }
    assert_eq!(paged, trash["items"].as_array().unwrap().clone());

    let (status, restored) = call(
        &fixture,
        "POST",
        &format!("{}/restore", task_uri(&fixture, id_of(&parent))),
        Some(json!({"expected_version": 1})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{restored}");
    assert_eq!(restored["deleted_at"], Value::Null);
    assert_eq!(restored["sub_issue_count"], 1);
    assert_eq!(
        fetch_task(&fixture, id_of(&child)).await["deleted_at"],
        Value::Null
    );
    assert_eq!(
        fetch_task(&fixture, id_of(&grandchild)).await["parent_task_id"],
        id_of(&child)
    );
    let earlier_stamp: Option<i64> =
        sqlx::query_scalar("SELECT deleted_at FROM tasks WHERE id = ?")
            .bind(id_of(&earlier))
            .fetch_one(pool)
            .await
            .unwrap();
    assert!(
        earlier_stamp.is_some(),
        "a child trashed earlier stays in the trash"
    );
    let restored_audits: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events WHERE action = 'task.restored' AND resource_id IN (?, ?, ?)",
    )
    .bind(id_of(&parent))
    .bind(id_of(&child))
    .bind(id_of(&grandchild))
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(restored_audits, 3);
}

#[tokio::test]
async fn sub_issue_restored_without_its_trashed_parent_becomes_top_level() {
    let fixture = Fixture::new().await;
    let parent = create_sub_issue(&fixture, "Parent", None).await;
    let child = create_sub_issue(&fixture, "Child", Some(id_of(&parent))).await;
    let (status, _) = call(
        &fixture,
        "DELETE",
        &format!("{}?expected_version=0", task_uri(&fixture, id_of(&parent))),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, restored) = call(
        &fixture,
        "POST",
        &format!("{}/restore", task_uri(&fixture, id_of(&child))),
        Some(json!({"expected_version": 1})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{restored}");
    assert_eq!(restored["parent_task_id"], Value::Null);
    assert_eq!(restored["parent"], Value::Null);
    let metadata: String = sqlx::query_scalar(
        "SELECT metadata_json FROM audit_events WHERE action = 'task.parent_changed' AND resource_id = ?",
    )
    .bind(id_of(&child))
    .fetch_one(fixture.database.pool())
    .await
    .unwrap();
    let metadata = serde_json::from_str::<Value>(&metadata).unwrap();
    assert_eq!(metadata["from"], json!(id_of(&parent)));
    assert_eq!(metadata["to"], Value::Null);
    assert!(metadata["from_project_id"].is_string());
    assert_eq!(metadata["to_project_id"], Value::Null);

    // Restoring the parent afterwards does not pull the child back under it.
    let (status, parent) = call(
        &fixture,
        "POST",
        &format!("{}/restore", task_uri(&fixture, id_of(&parent))),
        Some(json!({"expected_version": 1})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{parent}");
    assert_eq!(parent["sub_issue_count"], 0);
}

#[tokio::test]
async fn sub_issue_retention_purges_trashed_subtrees_and_detaches_survivors() {
    let fixture = Fixture::new().await;
    let pool = fixture.database.pool();
    let expired = TimestampMillis::now().as_millis() - 31 * 24 * 60 * 60 * 1_000;
    let parent = create_sub_issue(&fixture, "Parent", None).await;
    let child = create_sub_issue(&fixture, "Child", Some(id_of(&parent))).await;
    let (status, _) = call(
        &fixture,
        "DELETE",
        &format!("{}?expected_version=0", task_uri(&fixture, id_of(&parent))),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    sqlx::query("UPDATE tasks SET deleted_at = ? WHERE id IN (?, ?)")
        .bind(expired)
        .bind(id_of(&parent))
        .bind(id_of(&child))
        .execute(pool)
        .await
        .unwrap();
    // A live task whose parent sits in a project that expires from the trash.
    let old_project = create_project(&fixture, "OLD").await;
    let old_parent = create_task_in(&fixture, &old_project, "Old parent", None).await;
    let survivor = create_sub_issue(&fixture, "Survivor", Some(id_of(&old_parent))).await;
    sqlx::query("UPDATE projects SET deleted_at = ? WHERE id = ?")
        .bind(expired)
        .bind(&old_project)
        .execute(pool)
        .await
        .unwrap();

    WorkspaceRepository::new((*fixture.database).clone())
        .purge_retention(TimestampMillis::now())
        .await
        .unwrap();

    let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tasks WHERE id IN (?, ?, ?)")
        .bind(id_of(&parent))
        .bind(id_of(&child))
        .bind(id_of(&old_parent))
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(remaining, 0);
    assert_eq!(parent_of(&fixture, id_of(&survivor)).await, None);
    let violations: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pragma_foreign_key_check")
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(violations, 0);
}

async fn status_of(fixture: &Fixture, task_id: &str) -> String {
    sqlx::query_scalar("SELECT status_id FROM tasks WHERE id = ?")
        .bind(task_id)
        .fetch_one(fixture.database.pool())
        .await
        .unwrap()
}

async fn auto_closed_metadata(fixture: &Fixture, task_id: &str) -> Value {
    let metadata: String = sqlx::query_scalar(
        "SELECT metadata_json FROM audit_events WHERE action = 'task.auto_closed' AND resource_id = ?",
    )
    .bind(task_id)
    .fetch_one(fixture.database.pool())
    .await
    .unwrap();
    serde_json::from_str(&metadata).unwrap()
}

fn sorted_by_id(value: &Value) -> Vec<Value> {
    let mut items = value.as_array().unwrap().clone();
    items.sort_by_key(|item| item["id"].as_str().unwrap().to_owned());
    items
}

#[tokio::test]
async fn sub_issue_project_flags_default_on_and_patch_independently() {
    let fixture = Fixture::new().await;
    let (status, project) = call(
        &fixture,
        "POST",
        &format!("/api/v1/workspaces/{}/projects", fixture.workspace_id),
        Some(json!({"name": "Flags", "key": "FLG", "color": "#123456"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{project}");
    assert_eq!(
        (
            project["auto_close_parent"].clone(),
            project["auto_close_sub_issues"].clone()
        ),
        (json!(true), json!(true))
    );
    let uri = format!(
        "/api/v1/workspaces/{}/projects/{}",
        fixture.workspace_id,
        id_of(&project)
    );
    let (status, patched) = call(
        &fixture,
        "PATCH",
        &uri,
        Some(json!({"name": "Flags", "key": "FLG", "color": "#123456", "expected_version": 0, "auto_close_parent": false})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{patched}");
    assert_eq!(
        (
            patched["auto_close_parent"].clone(),
            patched["auto_close_sub_issues"].clone()
        ),
        (json!(false), json!(true))
    );
    // Omitting the flags keeps them.
    let (status, renamed) = call(
        &fixture,
        "PATCH",
        &uri,
        Some(json!({"name": "Flags renamed", "key": "FLG", "color": "#123456", "expected_version": 1})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{renamed}");
    assert_eq!(renamed["auto_close_parent"], false);
    let (_, listed) = call(
        &fixture,
        "GET",
        &format!("/api/v1/workspaces/{}/projects", fixture.workspace_id),
        None,
    )
    .await;
    let listed = listed["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == id_of(&project))
        .unwrap()
        .clone();
    assert_eq!(
        (
            listed["auto_close_parent"].clone(),
            listed["auto_close_sub_issues"].clone()
        ),
        (json!(false), json!(true))
    );
}

#[tokio::test]
async fn sub_issue_closing_every_child_closes_the_parent_chain() {
    let fixture = Fixture::new().await;
    let done = status_id_by_category(&fixture, &fixture.project_id, "completed").await;
    let backlog = fixture.status_id.clone();
    let grandparent = create_sub_issue(&fixture, "Grandparent", None).await;
    let parent = create_sub_issue(&fixture, "Parent", Some(id_of(&grandparent))).await;
    let first = create_sub_issue(&fixture, "First", Some(id_of(&parent))).await;
    let second = create_sub_issue(&fixture, "Second", Some(id_of(&parent))).await;

    let (status, closed_first) =
        patch_current(&fixture, id_of(&first), json!({"status_id": done})).await;
    assert_eq!(status, StatusCode::OK, "{closed_first}");
    assert_eq!(closed_first["auto_closed"], json!([]));
    assert_eq!(status_of(&fixture, id_of(&parent)).await, backlog);

    let (status, closed_second) =
        patch_current(&fixture, id_of(&second), json!({"status_id": done})).await;
    assert_eq!(status, StatusCode::OK, "{closed_second}");
    assert_eq!(closed_second["id"], id_of(&second));
    assert_eq!(
        closed_second["auto_closed"],
        json!([
            {"id": id_of(&parent), "status_id": done},
            {"id": id_of(&grandparent), "status_id": done}
        ])
    );
    assert_eq!(status_of(&fixture, id_of(&grandparent)).await, done);
    let (completed_at, updated_at) = task_timestamps(&fixture, id_of(&parent)).await;
    assert_eq!(completed_at, Some(updated_at));

    // Auto-closed tasks bump their version, so a write based on the old version conflicts.
    assert_eq!(fetch_task(&fixture, id_of(&parent)).await["version"], 1);
    let (status, _) = call(
        &fixture,
        "PATCH",
        &task_uri(&fixture, id_of(&parent)),
        Some(json!({"expected_version": 0, "title": "Stale"})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);

    assert_eq!(
        auto_closed_metadata(&fixture, id_of(&parent)).await,
        json!({"source_task_id": id_of(&second), "source_project_id": second["project_id"], "from_status_id": backlog, "to_status_id": done, "reason": "sub_issues_done"})
    );
    assert_eq!(
        auto_closed_metadata(&fixture, id_of(&grandparent)).await["source_task_id"],
        id_of(&parent)
    );
    let actor: Option<String> = sqlx::query_scalar(
        "SELECT actor_id FROM audit_events WHERE action = 'task.auto_closed' AND resource_id = ?",
    )
    .bind(id_of(&parent))
    .fetch_one(fixture.database.pool())
    .await
    .unwrap();
    assert_eq!(actor, Some(fixture.owner_id.to_string()));

    // Re-opening a child never re-opens its parent.
    let (status, reopened) =
        patch_current(&fixture, id_of(&first), json!({"status_id": backlog})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reopened["auto_closed"], json!([]));
    assert_eq!(status_of(&fixture, id_of(&parent)).await, done);
}

#[tokio::test]
async fn sub_issue_parent_stays_open_when_every_child_is_cancelled() {
    let fixture = Fixture::new().await;
    let done = status_id_by_category(&fixture, &fixture.project_id, "completed").await;
    let cancelled = status_id_by_category(&fixture, &fixture.project_id, "cancelled").await;
    let parent = create_sub_issue(&fixture, "Parent", None).await;
    let first = create_sub_issue(&fixture, "First", Some(id_of(&parent))).await;
    let second = create_sub_issue(&fixture, "Second", Some(id_of(&parent))).await;
    for task in [&first, &second] {
        let (status, closed) =
            patch_current(&fixture, id_of(task), json!({"status_id": cancelled})).await;
        assert_eq!(status, StatusCode::OK, "{closed}");
        assert_eq!(closed["auto_closed"], json!([]));
    }
    assert_eq!(status_of(&fixture, id_of(&parent)).await, fixture.status_id);

    // One completed child among cancelled ones is enough.
    let third = create_sub_issue(&fixture, "Third", Some(id_of(&parent))).await;
    let (status, closed) = patch_current(&fixture, id_of(&third), json!({"status_id": done})).await;
    assert_eq!(status, StatusCode::OK, "{closed}");
    assert_eq!(
        closed["auto_closed"],
        json!([{"id": id_of(&parent), "status_id": done}])
    );
}

#[tokio::test]
async fn sub_issue_closing_a_parent_closes_open_descendants_with_their_own_project_statuses() {
    let fixture = Fixture::new().await;
    let other_project = create_project(&fixture, "OTH").await;
    // OTH gets a second completed status that sorts first; the automation must pick it.
    let shipped = Id::new_v7().to_string();
    sqlx::query(
        "INSERT INTO task_statuses (id, workspace_id, project_id, name, description, color, category, position, version, created_at, updated_at) \
         VALUES (?, ?, ?, 'Shipped', '', '#4cb782', 'completed', -1, 0, 0, 0)",
    )
    .bind(&shipped)
    .bind(&fixture.workspace_id)
    .bind(&other_project)
    .execute(fixture.database.pool())
    .await
    .unwrap();
    let done = status_id_by_category(&fixture, &fixture.project_id, "completed").await;
    let cancelled = status_id_by_category(&fixture, &fixture.project_id, "cancelled").await;
    let parent = create_sub_issue(&fixture, "Parent", None).await;
    let remote = create_task_in(
        &fixture,
        &other_project,
        "Remote child",
        Some(id_of(&parent)),
    )
    .await;
    let grandchild = create_sub_issue(&fixture, "Grandchild", Some(id_of(&remote))).await;
    let already = create_sub_issue(&fixture, "Already cancelled", Some(id_of(&parent))).await;
    let (status, _) =
        patch_current(&fixture, id_of(&already), json!({"status_id": cancelled})).await;
    assert_eq!(status, StatusCode::OK);

    let (status, closed) =
        patch_current(&fixture, id_of(&parent), json!({"status_id": done})).await;
    assert_eq!(status, StatusCode::OK, "{closed}");
    let mut expected = vec![
        json!({"id": id_of(&remote), "status_id": shipped}),
        json!({"id": id_of(&grandchild), "status_id": done}),
    ];
    expected.sort_by_key(|item| item["id"].as_str().unwrap().to_owned());
    assert_eq!(sorted_by_id(&closed["auto_closed"]), expected);
    assert_eq!(status_of(&fixture, id_of(&already)).await, cancelled);
    assert_eq!(closed["sub_issue_closed_count"], 2);
    let metadata = auto_closed_metadata(&fixture, id_of(&remote)).await;
    assert_eq!(metadata["reason"], "parent_closed");
    assert_eq!(metadata["source_task_id"], id_of(&parent));

    // Cancelling a parent cancels its open sub-issues.
    let cancelled_parent = create_sub_issue(&fixture, "Cancelled parent", None).await;
    let open_child = create_sub_issue(&fixture, "Open child", Some(id_of(&cancelled_parent))).await;
    let (status, closed) = patch_current(
        &fixture,
        id_of(&cancelled_parent),
        json!({"status_id": cancelled}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{closed}");
    assert_eq!(
        closed["auto_closed"],
        json!([{"id": id_of(&open_child), "status_id": cancelled}])
    );
}

#[tokio::test]
async fn sub_issue_automation_skips_tasks_whose_project_has_no_target_status() {
    let fixture = Fixture::new().await;
    let bare = create_project(&fixture, "NOS").await;
    sqlx::query("DELETE FROM task_statuses WHERE project_id = ? AND category = 'completed'")
        .bind(&bare)
        .execute(fixture.database.pool())
        .await
        .unwrap();
    let done = status_id_by_category(&fixture, &fixture.project_id, "completed").await;

    // Rule B: the child in NOS has no completed status and stays open.
    let parent = create_sub_issue(&fixture, "Parent", None).await;
    let bare_child = create_task_in(&fixture, &bare, "Bare child", Some(id_of(&parent))).await;
    let local_child = create_sub_issue(&fixture, "Local child", Some(id_of(&parent))).await;
    let (status, closed) =
        patch_current(&fixture, id_of(&parent), json!({"status_id": done})).await;
    assert_eq!(status, StatusCode::OK, "{closed}");
    assert_eq!(
        closed["auto_closed"],
        json!([{"id": id_of(&local_child), "status_id": done}])
    );
    assert_eq!(
        status_of(&fixture, id_of(&bare_child)).await,
        status_id_by_category(&fixture, &bare, "unstarted").await
    );

    // Rule A: a parent in NOS cannot be completed, so it stays open without an error.
    let bare_parent = create_task_in(&fixture, &bare, "Bare parent", None).await;
    let child = create_sub_issue(&fixture, "Child of bare", Some(id_of(&bare_parent))).await;
    let (status, closed) = patch_current(&fixture, id_of(&child), json!({"status_id": done})).await;
    assert_eq!(status, StatusCode::OK, "{closed}");
    assert_eq!(closed["auto_closed"], json!([]));
}

#[tokio::test]
async fn sub_issue_automation_follows_each_projects_flags() {
    let fixture = Fixture::new().await;
    sqlx::query(
        "UPDATE projects SET auto_close_parent = 0, auto_close_sub_issues = 0 WHERE id = ?",
    )
    .bind(&fixture.project_id)
    .execute(fixture.database.pool())
    .await
    .unwrap();
    let done = status_id_by_category(&fixture, &fixture.project_id, "completed").await;
    let parent = create_sub_issue(&fixture, "Parent", None).await;
    let only = create_sub_issue(&fixture, "Only child", Some(id_of(&parent))).await;
    let (status, closed) = patch_current(&fixture, id_of(&only), json!({"status_id": done})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(closed["auto_closed"], json!([]));
    assert_eq!(status_of(&fixture, id_of(&parent)).await, fixture.status_id);
    let open_child = create_sub_issue(&fixture, "Open child", Some(id_of(&parent))).await;
    let (status, closed) =
        patch_current(&fixture, id_of(&parent), json!({"status_id": done})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(closed["auto_closed"], json!([]));
    assert_eq!(
        status_of(&fixture, id_of(&open_child)).await,
        fixture.status_id
    );

    // Rule A reads the parent's project, not the child's.
    let on = create_project(&fixture, "ONN").await;
    let on_done = status_id_by_category(&fixture, &on, "completed").await;
    let on_parent = create_task_in(&fixture, &on, "On parent", None).await;
    let off_child = create_sub_issue(&fixture, "Off child", Some(id_of(&on_parent))).await;
    let (status, closed) =
        patch_current(&fixture, id_of(&off_child), json!({"status_id": done})).await;
    assert_eq!(status, StatusCode::OK, "{closed}");
    assert_eq!(
        closed["auto_closed"],
        json!([{"id": id_of(&on_parent), "status_id": on_done}])
    );
}

#[tokio::test]
async fn sub_issue_bulk_delete_and_detach_run_the_parent_automation() {
    let fixture = Fixture::new().await;
    let done = status_id_by_category(&fixture, &fixture.project_id, "completed").await;

    // Bulk closing every child reports the parent once.
    let parent = create_sub_issue(&fixture, "Parent", None).await;
    let a = create_sub_issue(&fixture, "A", Some(id_of(&parent))).await;
    let b = create_sub_issue(&fixture, "B", Some(id_of(&parent))).await;
    let (status, bulk) = call(
        &fixture,
        "POST",
        &bulk_uri(&fixture),
        Some(json!({"updates": [
            {"id": id_of(&a), "expected_version": 0, "status_id": done},
            {"id": id_of(&b), "expected_version": 0, "status_id": done}
        ]})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{bulk}");
    assert_eq!(
        bulk["auto_closed"],
        json!([{"id": id_of(&parent), "status_id": done}])
    );
    assert_eq!(bulk["items"].as_array().unwrap().len(), 2);
    assert_eq!(bulk["next_cursor"], Value::Null);

    // Closing a parent and one child in the same request does not go stale on rule B.
    let q = create_sub_issue(&fixture, "Q", None).await;
    let d1 = create_sub_issue(&fixture, "D1", Some(id_of(&q))).await;
    let d2 = create_sub_issue(&fixture, "D2", Some(id_of(&q))).await;
    let (status, bulk) = call(
        &fixture,
        "POST",
        &bulk_uri(&fixture),
        Some(json!({"updates": [
            {"id": id_of(&q), "expected_version": 0, "status_id": done},
            {"id": id_of(&d1), "expected_version": 0, "status_id": done}
        ]})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{bulk}");
    assert_eq!(
        bulk["auto_closed"],
        json!([{"id": id_of(&d2), "status_id": done}])
    );

    // Deleting the last open child closes the parent.
    let r = create_sub_issue(&fixture, "R", None).await;
    let e1 = create_sub_issue(&fixture, "E1", Some(id_of(&r))).await;
    let e2 = create_sub_issue(&fixture, "E2", Some(id_of(&r))).await;
    let (status, _) = patch_current(&fixture, id_of(&e1), json!({"status_id": done})).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = call(
        &fixture,
        "DELETE",
        &format!("{}?expected_version=0", task_uri(&fixture, id_of(&e2))),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(status_of(&fixture, id_of(&r)).await, done);
    assert_eq!(
        auto_closed_metadata(&fixture, id_of(&r)).await["source_task_id"],
        id_of(&e2)
    );

    // Detaching the last open child closes the parent too.
    let s = create_sub_issue(&fixture, "S", None).await;
    let f1 = create_sub_issue(&fixture, "F1", Some(id_of(&s))).await;
    let f2 = create_sub_issue(&fixture, "F2", Some(id_of(&s))).await;
    let (status, _) = patch_current(&fixture, id_of(&f1), json!({"status_id": done})).await;
    assert_eq!(status, StatusCode::OK);
    let (status, detached) =
        patch_current(&fixture, id_of(&f2), json!({"parent_task_id": null})).await;
    assert_eq!(status, StatusCode::OK, "{detached}");
    assert_eq!(
        detached["auto_closed"],
        json!([{"id": id_of(&s), "status_id": done}])
    );
}

#[tokio::test]
async fn sub_issue_bulk_automation_uses_each_items_own_write_not_the_item_order() {
    let fixture = Fixture::new().await;
    let done = status_id_by_category(&fixture, &fixture.project_id, "completed").await;
    // P has a closed child C0 (with an open grandchild G) and one open child C1.
    let p = create_sub_issue(&fixture, "P", None).await;
    let c0 = create_sub_issue(&fixture, "C0", Some(id_of(&p))).await;
    let c1 = create_sub_issue(&fixture, "C1", Some(id_of(&p))).await;
    let (status, _) = patch_current(&fixture, id_of(&c0), json!({"status_id": done})).await;
    assert_eq!(status, StatusCode::OK);
    let g = create_sub_issue(&fixture, "G", Some(id_of(&c0))).await;
    assert_eq!(status_of(&fixture, id_of(&p)).await, fixture.status_id);
    let p_version = fetch_task(&fixture, id_of(&p)).await["version"].clone();

    // C1's rule A closes P; P's own write (a priority change) did not close it, so P must not
    // run rule B and G stays open, whatever the item order.
    let (status, bulk) = call(
        &fixture,
        "POST",
        &bulk_uri(&fixture),
        Some(json!({"updates": [
            {"id": id_of(&c1), "expected_version": 0, "status_id": done},
            {"id": id_of(&p), "expected_version": p_version, "priority": "high"}
        ]})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{bulk}");
    assert_eq!(
        bulk["auto_closed"],
        json!([{"id": id_of(&p), "status_id": done}])
    );
    assert_eq!(status_of(&fixture, id_of(&g)).await, fixture.status_id);
}

#[tokio::test]
async fn sub_issue_parent_closes_when_a_cancelled_child_becomes_completed() {
    let fixture = Fixture::new().await;
    let done = status_id_by_category(&fixture, &fixture.project_id, "completed").await;
    let cancelled = status_id_by_category(&fixture, &fixture.project_id, "cancelled").await;
    let parent = create_sub_issue(&fixture, "Parent", None).await;
    let first = create_sub_issue(&fixture, "First", Some(id_of(&parent))).await;
    let second = create_sub_issue(&fixture, "Second", Some(id_of(&parent))).await;
    for task in [&first, &second] {
        let (status, _) =
            patch_current(&fixture, id_of(task), json!({"status_id": cancelled})).await;
        assert_eq!(status, StatusCode::OK);
    }
    assert_eq!(status_of(&fixture, id_of(&parent)).await, fixture.status_id);

    let (status, moved) = patch_current(&fixture, id_of(&first), json!({"status_id": done})).await;
    assert_eq!(status, StatusCode::OK, "{moved}");
    assert_eq!(
        moved["auto_closed"],
        json!([{"id": id_of(&parent), "status_id": done}])
    );
    // Rule B did not run: the child was already closed, so the cancelled sibling is untouched.
    assert_eq!(status_of(&fixture, id_of(&second)).await, cancelled);
}

#[tokio::test]
async fn sub_issue_moving_a_closed_task_under_a_parent_can_complete_it() {
    let fixture = Fixture::new().await;
    let done = status_id_by_category(&fixture, &fixture.project_id, "completed").await;
    let cancelled = status_id_by_category(&fixture, &fixture.project_id, "cancelled").await;
    let parent = create_sub_issue(&fixture, "Parent", None).await;
    let dropped = create_sub_issue(&fixture, "Dropped", Some(id_of(&parent))).await;
    let (status, _) =
        patch_current(&fixture, id_of(&dropped), json!({"status_id": cancelled})).await;
    assert_eq!(status, StatusCode::OK);
    let finished = create_sub_issue(&fixture, "Finished", None).await;
    let (status, _) = patch_current(&fixture, id_of(&finished), json!({"status_id": done})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(status_of(&fixture, id_of(&parent)).await, fixture.status_id);

    let (status, moved) = patch_current(
        &fixture,
        id_of(&finished),
        json!({"parent_task_id": id_of(&parent)}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{moved}");
    assert_eq!(
        moved["auto_closed"],
        json!([{"id": id_of(&parent), "status_id": done}])
    );
    assert_eq!(
        auto_closed_metadata(&fixture, id_of(&parent)).await["source_task_id"],
        id_of(&finished)
    );

    // Bulk moves run the same check.
    let other_parent = create_sub_issue(&fixture, "Other parent", None).await;
    let finished = fetch_task(&fixture, id_of(&finished)).await;
    let (status, bulk) = call(
        &fixture,
        "POST",
        &bulk_uri(&fixture),
        Some(json!({"updates": [{
            "id": id_of(&finished),
            "expected_version": finished["version"],
            "parent_task_id": id_of(&other_parent),
        }]})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{bulk}");
    assert_eq!(
        bulk["auto_closed"],
        json!([{"id": id_of(&other_parent), "status_id": done}])
    );
}

#[tokio::test]
async fn sub_issue_created_closed_can_complete_its_parent() {
    let fixture = Fixture::new().await;
    let done = status_id_by_category(&fixture, &fixture.project_id, "completed").await;
    let cancelled = status_id_by_category(&fixture, &fixture.project_id, "cancelled").await;
    let parent = create_sub_issue(&fixture, "Parent", None).await;
    let dropped = create_sub_issue(&fixture, "Dropped", Some(id_of(&parent))).await;
    let (status, _) =
        patch_current(&fixture, id_of(&dropped), json!({"status_id": cancelled})).await;
    assert_eq!(status, StatusCode::OK);

    let tasks_uri = format!("/api/v1/workspaces/{}/tasks", fixture.workspace_id);
    let (status, created) = call(
        &fixture,
        "POST",
        &tasks_uri,
        Some(json!({
            "project_id": fixture.project_id,
            "status_id": done,
            "title": "Done on arrival",
            "parent_task_id": id_of(&parent),
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["parent_task_id"], id_of(&parent));
    assert_eq!(
        created["auto_closed"],
        json!([{"id": id_of(&parent), "status_id": done}])
    );
    assert_eq!(
        auto_closed_metadata(&fixture, id_of(&parent)).await["source_task_id"],
        id_of(&created)
    );

    // The parent's project flag still decides.
    let off = create_project(&fixture, "OFF").await;
    sqlx::query("UPDATE projects SET auto_close_parent = 0 WHERE id = ?")
        .bind(&off)
        .execute(fixture.database.pool())
        .await
        .unwrap();
    let off_parent = create_task_in(&fixture, &off, "Off parent", None).await;
    let (status, created) = call(
        &fixture,
        "POST",
        &tasks_uri,
        Some(json!({
            "project_id": fixture.project_id,
            "status_id": done,
            "title": "Done under off",
            "parent_task_id": id_of(&off_parent),
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["auto_closed"], json!([]));
    let off_backlog = status_id_by_category(&fixture, &off, "unstarted").await;
    assert_eq!(status_of(&fixture, id_of(&off_parent)).await, off_backlog);
}

#[tokio::test]
async fn sub_issue_parent_change_audit_names_a_hidden_old_parents_project() {
    let fixture = Fixture::new().await;
    let other = create_project(&fixture, "HID").await;
    let parent = create_task_in(&fixture, &other, "Parent elsewhere", None).await;
    let child = create_sub_issue(&fixture, "Child", Some(id_of(&parent))).await;
    let (status, _) = call(
        &fixture,
        "DELETE",
        &format!(
            "/api/v1/workspaces/{}/projects/{other}?expected_version=0",
            fixture.workspace_id
        ),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        fetch_task(&fixture, id_of(&child)).await["parent"],
        Value::Null
    );

    let (status, detached) =
        patch_current(&fixture, id_of(&child), json!({"parent_task_id": null})).await;
    assert_eq!(status, StatusCode::OK, "{detached}");
    let metadata: String = sqlx::query_scalar(
        "SELECT metadata_json FROM audit_events WHERE action = 'task.parent_changed' AND resource_id = ?",
    )
    .bind(id_of(&child))
    .fetch_one(fixture.database.pool())
    .await
    .unwrap();
    let metadata = serde_json::from_str::<Value>(&metadata).unwrap();
    assert_eq!(metadata["from"], json!(id_of(&parent)));
    assert_eq!(metadata["from_project_id"], json!(other));
}

async fn link_github_issue(fixture: &Fixture, task_id: &str, number: i64) {
    sqlx::query("INSERT INTO github_issue_links (workspace_id, repository, issue_number, task_id, kind) VALUES (?, 'acme/repo', ?, ?, 'issue')")
        .bind(&fixture.workspace_id)
        .bind(number)
        .bind(task_id)
        .execute(fixture.database.pool())
        .await
        .unwrap();
}

async fn sync_github_issue(fixture: &Fixture, number: i64, title: &str, state: &'static str) {
    TaskRepository::new((*fixture.database).clone())
        .sync_github_work_item(
            fixture.workspace_id.parse().unwrap(),
            fixture.owner_id,
            GithubWorkItem {
                repository: "acme/repo".to_owned(),
                number,
                project_id: fixture.project_id.parse().unwrap(),
                title: title.to_owned(),
                description: String::new(),
                kind: "issue",
                state,
                state_changed: true,
            },
            "github-test",
            TimestampMillis::now(),
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn sub_issue_github_sync_runs_the_automation_and_restores_top_level_under_a_trashed_parent() {
    let fixture = Fixture::new().await;
    let done = status_id_by_category(&fixture, &fixture.project_id, "completed").await;

    // Rule A: GitHub closing the last open child closes the parent, audited as GitHub.
    let parent = create_sub_issue(&fixture, "Parent", None).await;
    let child = create_sub_issue(&fixture, "Synced child", Some(id_of(&parent))).await;
    link_github_issue(&fixture, id_of(&child), 21).await;
    sync_github_issue(&fixture, 21, "Synced child", "closed").await;
    assert_eq!(status_of(&fixture, id_of(&child)).await, done);
    assert_eq!(status_of(&fixture, id_of(&parent)).await, done);
    let (actor, metadata): (Option<String>, String) = sqlx::query_as(
        "SELECT actor_id, metadata_json FROM audit_events WHERE action = 'task.auto_closed' AND resource_id = ?",
    )
    .bind(id_of(&parent))
    .fetch_one(fixture.database.pool())
    .await
    .unwrap();
    assert_eq!(actor, None, "GitHub acts through its service account");
    let metadata = serde_json::from_str::<Value>(&metadata).unwrap();
    assert_eq!(metadata["source_task_id"], json!(id_of(&child)));
    assert_eq!(metadata["reason"], "sub_issues_done");
    assert!(metadata["actor_service_account_id"].is_string());
    assert_eq!(metadata["actor_service_account_name"], "GitHub");

    // Rule B: GitHub closing a parent closes its open sub-issues.
    let synced_parent = create_sub_issue(&fixture, "Synced parent", None).await;
    let open_child = create_sub_issue(&fixture, "Open child", Some(id_of(&synced_parent))).await;
    link_github_issue(&fixture, id_of(&synced_parent), 22).await;
    sync_github_issue(&fixture, 22, "Synced parent", "closed").await;
    assert_eq!(status_of(&fixture, id_of(&open_child)).await, done);
    assert_eq!(
        auto_closed_metadata(&fixture, id_of(&open_child)).await["reason"],
        "parent_closed"
    );

    // Un-trashing through sync while the parent stays in the trash restores top-level.
    let trashed_parent = create_sub_issue(&fixture, "Trashed parent", None).await;
    let trashed_child =
        create_sub_issue(&fixture, "Trashed child", Some(id_of(&trashed_parent))).await;
    link_github_issue(&fixture, id_of(&trashed_child), 23).await;
    let (status, _) = call(
        &fixture,
        "DELETE",
        &format!(
            "{}?expected_version=0",
            task_uri(&fixture, id_of(&trashed_parent))
        ),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    sync_github_issue(&fixture, 23, "Trashed child", "open").await;
    let restored = fetch_task(&fixture, id_of(&trashed_child)).await;
    assert_eq!(restored["parent_task_id"], Value::Null);
    let (actor, metadata): (Option<String>, String) = sqlx::query_as(
        "SELECT actor_id, metadata_json FROM audit_events WHERE action = 'task.parent_changed' AND resource_id = ?",
    )
    .bind(id_of(&trashed_child))
    .fetch_one(fixture.database.pool())
    .await
    .unwrap();
    assert_eq!(actor, None);
    let metadata = serde_json::from_str::<Value>(&metadata).unwrap();
    assert_eq!(metadata["from"], json!(id_of(&trashed_parent)));
    assert_eq!(metadata["to"], Value::Null);
    assert_eq!(metadata["from_project_id"], json!(fixture.project_id));
    assert!(metadata["actor_service_account_id"].is_string());
}

async fn titles_for(fixture: &Fixture, filter: Value, sub_issues: &str) -> Vec<String> {
    let mut body = query_body(filter, "title", "asc", "all");
    body["sub_issues"] = json!(sub_issues);
    let (status, page) = call(fixture, "POST", &query_uri(fixture), Some(body)).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    query_titles(&page)
}

#[tokio::test]
async fn sub_issue_filters_and_hidden_display_select_by_hierarchy() {
    let fixture = Fixture::new().await;
    let parent = create_sub_issue(&fixture, "Parent", None).await;
    let parent_id = id_of(&parent).to_owned();
    let child = create_sub_issue(&fixture, "Child", Some(&parent_id)).await;
    create_sub_issue(&fixture, "Loner", None).await;

    let cases = [
        (only("parent", "is", json!([parent_id])), vec!["Child"]),
        (
            only("parent", "is", json!(["none"])),
            vec!["Loner", "Parent"],
        ),
        (
            only("parent", "is", json!(["none", parent_id])),
            vec!["Child", "Loner", "Parent"],
        ),
        (
            only("parent", "is_not", json!([parent_id])),
            vec!["Loner", "Parent"],
        ),
        (only("parent", "is_not", json!(["none"])), vec!["Child"]),
        (only("sub_issues", "is", json!(["has"])), vec!["Parent"]),
        (
            only("sub_issues", "is", json!(["none"])),
            vec!["Child", "Loner"],
        ),
        (
            only("sub_issues", "is", json!(["has", "none"])),
            vec!["Child", "Loner", "Parent"],
        ),
    ];
    for (filter, expected) in cases {
        assert_eq!(
            titles_for(&fixture, filter.clone(), "nested").await,
            expected,
            "{filter}"
        );
    }
    let everything = json!({"op": "and", "children": []});
    assert_eq!(
        titles_for(&fixture, everything.clone(), "flat").await,
        ["Child", "Loner", "Parent"]
    );
    assert_eq!(
        titles_for(&fixture, everything.clone(), "hidden").await,
        ["Loner", "Parent"]
    );
    // Omitting sub_issues keeps today's result.
    let (_, page) = call(
        &fixture,
        "POST",
        &query_uri(&fixture),
        Some(query_body(everything.clone(), "title", "asc", "all")),
    )
    .await;
    assert_eq!(query_titles(&page), ["Child", "Loner", "Parent"]);

    // Trashed children are not sub-issues.
    let (status, _) = call(
        &fixture,
        "DELETE",
        &format!("{}?expected_version=0", task_uri(&fixture, id_of(&child))),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(
        titles_for(&fixture, only("sub_issues", "is", json!(["has"])), "nested")
            .await
            .is_empty()
    );

    for (filter, path) in [
        (
            only("parent", "is", json!(["me"])),
            "filter.children[0].value[0]",
        ),
        (
            only("sub_issues", "is", json!(["some"])),
            "filter.children[0].value[0]",
        ),
        (
            only("sub_issues", "is_not", json!(["has"])),
            "filter.children[0].operator",
        ),
    ] {
        let (status, problem) = call(
            &fixture,
            "POST",
            &query_uri(&fixture),
            Some(query_body(filter, "title", "asc", "all")),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{problem}");
        assert_eq!(problem["path"], path);
    }
    let mut body = query_body(everything, "title", "asc", "all");
    body["sub_issues"] = json!("tree");
    let (status, _) = call(&fixture, "POST", &query_uri(&fixture), Some(body)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
