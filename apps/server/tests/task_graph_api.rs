use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use orbit_platform::{Id, PasswordService, TestDatabase, TimestampMillis};
use orbit_server::auth_routes::CookieMode;
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::task_routes::{TaskState, task_router};
use serde_json::{Value, json};
use tower::ServiceExt;

struct Fixture {
    database: TestDatabase,
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
            app,
            workspace_id: setup.workspace_id.to_string(),
            project_id: setup.project_id.to_string(),
            status_id,
            owner_cookie: format!("__Host-orbit_session={}", setup.session.token),
        }
    }

    async fn create_task(&self, title: &str) -> Value {
        self.create_task_with(json!({ "title": title })).await
    }

    async fn create_task_with(&self, extra: Value) -> Value {
        let mut body = json!({
            "project_id": self.project_id,
            "status_id": self.status_id,
        });
        for (key, value) in extra.as_object().unwrap() {
            body[key] = value.clone();
        }
        let response = self
            .app
            .clone()
            .oneshot(json_request(
                "POST",
                &format!("/api/v1/workspaces/{}/tasks", self.workspace_id),
                &self.owner_cookie,
                body,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        response_json(response).await
    }

    fn task_path(&self, task: &Value) -> String {
        format!(
            "/api/v1/workspaces/{}/tasks/{}",
            self.workspace_id,
            task["id"].as_str().unwrap()
        )
    }

    async fn patch(&self, task: &Value, body: Value) -> axum::response::Response {
        self.app
            .clone()
            .oneshot(json_request(
                "PATCH",
                &self.task_path(task),
                &self.owner_cookie,
                body,
            ))
            .await
            .unwrap()
    }

    async fn get(&self, uri: &str) -> axum::response::Response {
        self.app
            .clone()
            .oneshot(cookie_request("GET", uri, &self.owner_cookie))
            .await
            .unwrap()
    }
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

#[tokio::test]
async fn migration_0016_adds_parent_column_indexes_and_graph_tables() {
    let database = TestDatabase::new().await.unwrap();

    let columns: Vec<String> = sqlx::query_scalar("SELECT name FROM pragma_table_info('tasks')")
        .fetch_all(database.pool())
        .await
        .unwrap();
    assert!(columns.contains(&"parent_id".to_owned()));

    let objects: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE name IN (\
         'tasks_parent', 'tasks_parent_rollup', 'task_relations', 'task_references', \
         'task_relations_one_duplicate', 'task_relations_target', 'task_references_target', \
         'tasks_validate_parent_insert', 'tasks_validate_parent_update', \
         'task_relations_validate_scope_insert', 'task_references_validate_scope_insert', \
         'tasks_cleanup_references', 'task_comments_cleanup_references') \
         ORDER BY name",
    )
    .fetch_all(database.pool())
    .await
    .unwrap();
    assert_eq!(objects.len(), 13, "missing schema objects: {objects:?}");

    let rollup: String =
        sqlx::query_scalar("SELECT sql FROM sqlite_master WHERE name = 'tasks_parent_rollup'")
            .fetch_one(database.pool())
            .await
            .unwrap();
    assert!(rollup.contains("WHERE deleted_at IS NULL AND parent_id IS NOT NULL"));
}

/// Seeds a workspace with one owner, one project and one status. Returns
/// `(workspace_id, project_id, status_id, user_id)`.
async fn seed_workspace(database: &TestDatabase, key: &str) -> (String, String, String, String) {
    let workspace_id = Id::new_v7().to_string();
    let project_id = Id::new_v7().to_string();
    let status_id = Id::new_v7().to_string();
    let user_id = Id::new_v7().to_string();
    let membership_id = Id::new_v7().to_string();
    let email = format!("{}@example.com", key.to_lowercase());
    database
        .execute(&format!(
            "BEGIN;\
             INSERT INTO users (id, email, normalized_email, display_name, password_hash, created_at, updated_at)\
             VALUES ('{user_id}', '{email}', '{email}', '{key}', 'unused', 1, 1);\
             INSERT INTO workspaces (id, name, version, owner_membership_id, created_at, updated_at)\
             VALUES ('{workspace_id}', '{key}', 0, '{membership_id}', 1, 1);\
             INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, updated_at)\
             VALUES ('{membership_id}', '{workspace_id}', '{user_id}', 'owner', 0, 1, 1);\
             INSERT INTO projects (id, workspace_id, name, project_key, color, version, created_at, updated_at)\
             VALUES ('{project_id}', '{workspace_id}', '{key}', '{key}', '#5e6ad2', 0, 1, 1);\
             INSERT INTO task_statuses (id, workspace_id, project_id, name, description, color, category, position, version, created_at, updated_at)\
             VALUES ('{status_id}', '{workspace_id}', '{project_id}', 'Todo', '', '#8b8f98', 'unstarted', 0, 0, 1, 1);\
             COMMIT;"
        ))
        .await
        .unwrap();
    (workspace_id, project_id, status_id, user_id)
}

async fn seed_task(
    database: &TestDatabase,
    scope: &(String, String, String, String),
    key: &str,
    number: i64,
) -> String {
    let id = Id::new_v7().to_string();
    sqlx::query(
        "INSERT INTO tasks (id, workspace_id, project_id, status_id, identifier_key, number, title, \
         priority, position, creator_id, version, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, 'none', 0, ?, 0, 1, 1)",
    )
    .bind(&id)
    .bind(&scope.0)
    .bind(&scope.1)
    .bind(&scope.2)
    .bind(key)
    .bind(number)
    .bind(format!("{key}-{number}"))
    .bind(&scope.3)
    .execute(database.pool())
    .await
    .unwrap();
    id
}

#[tokio::test]
async fn parent_trigger_rejects_cross_workspace_and_self_parenting() {
    let database = TestDatabase::new().await.unwrap();
    let first = seed_workspace(&database, "ALP").await;
    let second = seed_workspace(&database, "BET").await;
    let child = seed_task(&database, &first, "ALP", 1).await;
    let foreign = seed_task(&database, &second, "BET", 1).await;

    let cross = sqlx::query("UPDATE tasks SET parent_id = ? WHERE id = ?")
        .bind(&foreign)
        .bind(&child)
        .execute(database.pool())
        .await;
    assert!(cross.is_err(), "cross-workspace parent must abort");

    let itself = sqlx::query("UPDATE tasks SET parent_id = ? WHERE id = ?")
        .bind(&child)
        .bind(&child)
        .execute(database.pool())
        .await;
    assert!(itself.is_err(), "self-parenting must abort");

    let sibling = seed_task(&database, &first, "ALP", 2).await;
    sqlx::query("UPDATE tasks SET parent_id = ? WHERE id = ?")
        .bind(&sibling)
        .bind(&child)
        .execute(database.pool())
        .await
        .expect("same-workspace parent is allowed");

    // a relation or a reference cannot reach across workspaces either
    let relation = sqlx::query(
        "INSERT INTO task_relations (id, workspace_id, kind, source_task_id, target_task_id, created_by, created_at) \
         VALUES (?, ?, 'duplicate_of', ?, ?, ?, 1)",
    )
    .bind(Id::new_v7().to_string())
    .bind(&first.0)
    .bind(&child)
    .bind(&foreign)
    .bind(&first.3)
    .execute(database.pool())
    .await;
    assert!(relation.is_err(), "cross-workspace relation must abort");

    let reference = sqlx::query(
        "INSERT INTO task_references (workspace_id, source_type, source_id, target_task_id) \
         VALUES (?, 'task', ?, ?)",
    )
    .bind(&first.0)
    .bind(&child)
    .bind(&foreign)
    .execute(database.pool())
    .await;
    assert!(reference.is_err(), "cross-workspace reference must abort");
}

#[tokio::test]
async fn sub_issue_parent_is_accepted_and_cycles_are_refused() {
    let fixture = Fixture::new().await;
    let parent = fixture.create_task("Parent").await;
    let child = fixture.create_task("Child").await;
    let grandchild = fixture.create_task("Grandchild").await;

    let response = fixture
        .patch(
            &child,
            json!({"expected_version": 0, "parent_id": parent["id"]}),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let updated = response_json(response).await;
    assert_eq!(updated["parent_id"], parent["id"]);

    let response = fixture
        .patch(
            &grandchild,
            json!({"expected_version": 0, "parent_id": child["id"]}),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);

    // three-task cycle: parent -> child -> grandchild, now parent under grandchild
    let response = fixture
        .patch(
            &parent,
            json!({"expected_version": 0, "parent_id": grandchild["id"]}),
        )
        .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let problem = response_json(response).await;
    assert_eq!(problem["code"], "validation_failed");
    assert_eq!(problem["detail"], "parent_id");

    // self-parenting never reaches SQLite
    let response = fixture
        .patch(
            &parent,
            json!({"expected_version": 0, "parent_id": parent["id"]}),
        )
        .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);

    // an unknown or malformed parent is a validation failure, not a 404
    for bogus in [Id::new_v7().to_string(), "not-an-id".to_owned()] {
        let response = fixture
            .patch(&parent, json!({"expected_version": 0, "parent_id": bogus}))
            .await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(response_json(response).await["detail"], "parent_id");
    }

    // an edit that leaves parent_id out keeps it
    let response = fixture
        .patch(&child, json!({"expected_version": 1, "title": "Child!"}))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response_json(response).await["parent_id"], parent["id"]);

    // the detail read carries the stored edge
    let detail = response_json(fixture.get(&fixture.task_path(&grandchild)).await).await;
    assert_eq!(detail["parent_id"], child["id"]);

    // detaching clears it
    let response = fixture
        .patch(&child, json!({"expected_version": 2, "parent_id": null}))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response_json(response).await["parent_id"].is_null());
    let stored: Option<String> = sqlx::query_scalar("SELECT parent_id FROM tasks WHERE id = ?")
        .bind(child["id"].as_str().unwrap())
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    assert!(stored.is_none());
}

#[tokio::test]
async fn a_task_can_be_created_as_a_sub_issue_up_to_the_depth_bound() {
    let fixture = Fixture::new().await;
    let root = fixture.create_task("Root").await;
    let mut chain = vec![root];
    // nine more levels: the deepest task has nine ancestors
    for level in 1..10 {
        let parent = chain.last().unwrap()["id"].clone();
        let child = fixture
            .create_task_with(json!({"title": format!("Level {level}"), "parent_id": parent}))
            .await;
        assert_eq!(child["parent_id"], parent);
        chain.push(child);
    }
    // a tenth ancestor cannot be proved acyclic inside the bounded walk
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
                "title": "Too deep",
                "parent_id": chain.last().unwrap()["id"],
            }),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(response_json(response).await["detail"], "parent_id");
}

#[tokio::test]
async fn parent_filter_changes_the_cursor_fingerprint_and_hides_sub_issues() {
    let fixture = Fixture::new().await;
    let parent = fixture.create_task("Parent").await;
    let child = fixture.create_task("Child").await;
    let response = fixture
        .patch(
            &child,
            json!({"expected_version": 0, "parent_id": parent["id"]}),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);

    let fixture = &fixture;
    let list = |query: String| async move {
        let uri = format!("/api/v1/workspaces/{}/tasks{query}", fixture.workspace_id);
        fixture.get(&uri).await
    };

    // default nesting=all keeps today's behaviour
    let page = response_json(list(String::new()).await).await;
    assert_eq!(page["items"].as_array().unwrap().len(), 2);

    // nesting=roots hides the sub-issue
    let page = response_json(list("?nesting=roots".to_owned()).await).await;
    let roots = page["items"].as_array().unwrap();
    assert_eq!(roots.len(), 1);
    assert_eq!(roots[0]["id"], parent["id"]);

    // parent_id returns only the children
    let page =
        response_json(list(format!("?parent_id={}", parent["id"].as_str().unwrap())).await).await;
    let children = page["items"].as_array().unwrap();
    assert_eq!(children.len(), 1);
    assert_eq!(children[0]["id"], child["id"]);

    // an unknown nesting mode is a validation failure
    let response = list("?nesting=deep".to_owned()).await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(response_json(response).await["detail"], "nesting");

    // a cursor minted for one filter cannot be replayed against another
    let page = response_json(list("?limit=1".to_owned()).await).await;
    let cursor = page["next_cursor"]
        .as_str()
        .expect("cursor for page 1")
        .to_owned();
    let response = list(format!("?limit=1&cursor={cursor}&nesting=roots")).await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(response_json(response).await["code"], "invalid_cursor");

    let response = list(format!(
        "?limit=1&cursor={cursor}&parent_id={}",
        parent["id"].as_str().unwrap()
    ))
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    // ...while the same filter accepts it
    let response = list(format!("?limit=1&cursor={cursor}")).await;
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn sub_issue_progress_rolls_up_for_a_whole_page() {
    let fixture = Fixture::new().await;
    let done_status: String = sqlx::query_scalar(
        "SELECT id FROM task_statuses WHERE project_id = ? AND category = 'completed' LIMIT 1",
    )
    .bind(&fixture.project_id)
    .fetch_one(fixture.database.pool())
    .await
    .unwrap();

    let mut parents = Vec::new();
    let mut trashed = None;
    for index in 0..3 {
        let parent = fixture.create_task(&format!("Parent {index}")).await;
        for child_index in 0..5 {
            let child = fixture
                .create_task(&format!("Child {index}-{child_index}"))
                .await;
            let body = if child_index < 2 {
                json!({"expected_version": 0, "parent_id": parent["id"], "status_id": done_status})
            } else {
                json!({"expected_version": 0, "parent_id": parent["id"]})
            };
            let response = fixture.patch(&child, body).await;
            assert_eq!(response.status(), StatusCode::OK);
            if child_index == 4 {
                trashed = Some(child);
            }
        }
        // the fifth child goes to the trash and must drop out of the rollup
        let child = trashed.take().unwrap();
        let response = fixture
            .app
            .clone()
            .oneshot(cookie_request(
                "DELETE",
                &format!("{}?expected_version=1", fixture.task_path(&child)),
                &fixture.owner_cookie,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        parents.push(parent["id"].as_str().unwrap().to_owned());
    }

    let page = response_json(
        fixture
            .get(&format!(
                "/api/v1/workspaces/{}/tasks?nesting=roots&limit=50",
                fixture.workspace_id
            ))
            .await,
    )
    .await;
    let items = page["items"].as_array().unwrap();
    assert_eq!(items.len(), 3);
    for item in items {
        assert!(parents.contains(&item["id"].as_str().unwrap().to_owned()));
        assert_eq!(item["sub_issue_total"], 4);
        assert_eq!(item["sub_issue_done"], 2);
    }

    let detail = response_json(
        fixture
            .get(&format!(
                "/api/v1/workspaces/{}/tasks/{}",
                fixture.workspace_id, parents[0]
            ))
            .await,
    )
    .await;
    assert_eq!(detail["sub_issue_total"], 4);
    assert_eq!(detail["sub_issue_done"], 2);

    // a leaf reports zero, and an update response carries the rollup too
    let response = fixture
        .patch(&detail, json!({"expected_version": 0, "title": "Renamed"}))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let updated = response_json(response).await;
    assert_eq!(updated["sub_issue_total"], 4);
    let leaf = fixture.create_task("Leaf").await;
    assert_eq!(leaf["sub_issue_total"], 0);
    assert_eq!(leaf["sub_issue_done"], 0);
}
