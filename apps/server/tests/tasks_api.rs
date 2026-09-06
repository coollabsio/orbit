use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use orbit_platform::{AuthenticatedUser, Id, PasswordService, TestDatabase, TimestampMillis};
use orbit_server::auth_routes::CookieMode;
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
async fn task_due_date_can_be_created_changed_and_cleared_with_version_checks() {
    let fixture = Fixture::new().await;
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
                "due_at": due_at
            }),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let task = response_json(response).await;
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
    assert_eq!(statuses["items"].as_array().unwrap().len(), 5);
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
    let status = statuses["items"].as_array().unwrap().last().unwrap();
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
async fn mine_overdue_and_due_soon_views_use_the_authenticated_user_and_utc_day() {
    let fixture = Fixture::new().await;
    let (member_id, member_cookie) = add_member(&fixture, "assignee@example.com").await;
    let past = TimestampMillis::from_millis(TimestampMillis::now().as_millis() - 3 * 86_400_000);
    let soon = TimestampMillis::from_millis(TimestampMillis::now().as_millis() + 2 * 86_400_000);
    for (title, assignee, due) in [
        ("Mine overdue", Some(member_id), Some(past)),
        ("Mine soon", Some(member_id), Some(soon)),
        ("Theirs overdue", None, Some(past)),
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
                &format!("/api/v1/workspaces/{}/tasks?view=mine", fixture.workspace_id),
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
