//! Imports from other tools through the public REST API: project setup writes, relations and
//! `tasks:import` tokens. SQLite by default, Postgres when `ORBIT_TEST_POSTGRES_URL` is set.
use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use orbit_platform::{
    HttpPlatformLayer, Id, OriginPolicy, PasswordService, TestDatabase, TimestampMillis,
};
use orbit_server::public_api::{PublicApiState, router};
use orbit_server::repositories::api_tokens::{ApiTokenError, ApiTokenRepository, ApiTokenScope};
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::repositories::tasks::TaskRepository;
use serde_json::{Value, json};
use tower::ServiceExt;

const IMPORT: &[ApiTokenScope] = &[ApiTokenScope::TasksWrite, ApiTokenScope::TasksImport];

struct Fixture {
    database: TestDatabase,
    app: axum::Router,
    tokens: ApiTokenRepository,
    user: Id,
    workspace: Id,
    project: Id,
    other_project: Id,
}

impl Fixture {
    async fn new() -> Self {
        let database = TestDatabase::new().await.unwrap();
        let identity = IdentityRepository::new((*database).clone());
        let now = TimestampMillis::now();
        identity
            .store_setup_token(
                "setup",
                TimestampMillis::from_millis(now.as_millis() + 60_000),
            )
            .await
            .unwrap();
        let setup = identity
            .complete_setup(
                SetupRequest {
                    token: "setup".into(),
                    email: "owner@example.com".into(),
                    display_name: "Owner".into(),
                    password_hash: PasswordService::default()
                        .hash("correct horse battery")
                        .unwrap(),
                    workspace_name: "Orbit".into(),
                    project_name: "General".into(),
                },
                now,
            )
            .await
            .unwrap();
        let tasks = TaskRepository::new((*database).clone());
        let other = tasks
            .create_project(
                setup.workspace_id,
                setup.user_id,
                "Private".into(),
                "PRV".into(),
                "#ff0000".into(),
                false,
                "test",
                now,
            )
            .await
            .unwrap();
        let tokens = ApiTokenRepository::new((*database).clone());
        let app = router(PublicApiState::new(
            Arc::new(tokens.clone()),
            Arc::new(tasks),
            "https://orbit.test",
        ))
        .layer(HttpPlatformLayer::new(OriginPolicy::new(
            "https://orbit.test",
        )));
        Self {
            database,
            app,
            tokens,
            user: setup.user_id,
            workspace: setup.workspace_id,
            project: setup.project_id,
            other_project: other.id,
        }
    }

    async fn token_by(
        &self,
        creator: Id,
        scopes: &[ApiTokenScope],
    ) -> Result<String, ApiTokenError> {
        self.tokens
            .create(
                self.workspace,
                creator,
                format!("cli-{}", Id::new_v7()),
                vec![self.project],
                scopes,
                false,
                None,
                "test",
                TimestampMillis::now(),
            )
            .await
            .map(|issued| issued.token)
    }

    async fn token(&self, scopes: &[ApiTokenScope]) -> String {
        self.token_by(self.user, scopes).await.unwrap()
    }

    /// Calls `/api/public/v1/workspaces/{workspace}{path}`.
    async fn call(
        &self,
        token: &str,
        method: &str,
        path: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let builder = Request::builder()
            .method(method)
            .uri(format!(
                "/api/public/v1/workspaces/{}{path}",
                self.workspace
            ))
            .header(header::AUTHORIZATION, format!("Bearer {token}"));
        let request = match body {
            Some(body) => builder
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string())),
            None => builder.body(Body::empty()),
        }
        .unwrap();
        let response = self.app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 4 * 1024 * 1024)
            .await
            .unwrap();
        let value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        };
        (status, value)
    }

    async fn status(&self, category: &str) -> String {
        sqlx::query_scalar(
            "SELECT id FROM task_statuses WHERE project_id = $1 AND category = $2 ORDER BY position LIMIT 1",
        )
        .bind(self.project.to_string())
        .bind(category)
        .fetch_one(self.database.pool())
        .await
        .unwrap()
    }

    async fn member(&self, email: &str, role: &str) -> Id {
        let user = Id::new_v7();
        let mut tx = self.database.immediate_transaction().await.unwrap();
        sqlx::query("INSERT INTO users (id, email, normalized_email, display_name, password_hash, created_at, updated_at) VALUES ($1, $2, $2, $3, 'x', 1, 1)")
            .bind(user.to_string()).bind(email).bind(email).execute(&mut *tx).await.unwrap();
        sqlx::query("INSERT INTO memberships (id, workspace_id, user_id, role, created_at, updated_at) VALUES ($1, $2, $3, $4, 1, 1)")
            .bind(Id::new_v7().to_string()).bind(self.workspace.to_string()).bind(user.to_string()).bind(role)
            .execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
        user
    }

    async fn notifications(&self, user: Id) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM notifications WHERE recipient_user_id = $1")
            .bind(user.to_string())
            .fetch_one(self.database.pool())
            .await
            .unwrap()
    }
}

#[tokio::test]
async fn an_import_token_needs_write_and_a_creator_who_stays_owner_or_admin() {
    let f = Fixture::new().await;
    assert!(matches!(
        f.token_by(
            f.user,
            &[ApiTokenScope::TasksRead, ApiTokenScope::TasksImport]
        )
        .await,
        Err(ApiTokenError::NotFound)
    ));
    let status = f.status("unstarted").await;
    let task = json!({"project_id": f.project, "status_id": status, "title": "Old", "number": 7});

    // Import fields need the scope.
    let write = f.token(&[ApiTokenScope::TasksWrite]).await;
    let (code, problem) = f.call(&write, "POST", "/tasks", Some(task.clone())).await;
    assert_eq!(code, StatusCode::FORBIDDEN, "{problem}");
    assert_eq!(problem["code"], "insufficient_scope");

    // An admin's import token stops writing when the admin becomes a member.
    let admin = f.member("admin@example.com", "admin").await;
    let token = f.token_by(admin, IMPORT).await.unwrap();
    let (code, _) = f.call(&token, "POST", "/tasks", Some(task.clone())).await;
    assert_eq!(code, StatusCode::CREATED);
    sqlx::query("UPDATE memberships SET role = 'member' WHERE user_id = $1")
        .bind(admin.to_string())
        .execute(f.database.pool())
        .await
        .unwrap();
    let (code, problem) = f
        .call(
            &token,
            "POST",
            "/tasks",
            Some(json!({"project_id": f.project, "status_id": status, "title": "New"})),
        )
        .await;
    assert_eq!(code, StatusCode::FORBIDDEN, "{problem}");
    assert_eq!(problem["code"], "workspace_action_forbidden");
}

#[tokio::test]
async fn an_imported_task_keeps_its_number_dates_and_creator_and_notifies_nobody() {
    let f = Fixture::new().await;
    let member = f.member("member@example.com", "member").await;
    let token = f.token(IMPORT).await;
    let done = f.status("completed").await;
    let (code, task) = f
        .call(
            &token,
            "POST",
            "/tasks",
            Some(json!({
                "project_id": f.project, "status_id": done, "title": "From Plane",
                "assignee_ids": [member], "number": 166, "creator_id": member,
                "created_at": "2024-01-02T03:04:05.000Z", "updated_at": "2024-02-03T04:05:06.000Z",
                "completed_at": "2024-02-01T00:00:00.000Z",
            })),
        )
        .await;
    assert_eq!(code, StatusCode::CREATED, "{task}");
    assert_eq!(task["number"], 166);
    assert!(
        task["identifier"].as_str().unwrap().ends_with("-166"),
        "{task}"
    );
    assert_eq!(task["created_at"], "2024-01-02T03:04:05.000Z");
    assert_eq!(task["updated_at"], "2024-02-03T04:05:06.000Z");
    assert_eq!(task["completed_at"], "2024-02-01T00:00:00.000Z");
    assert_eq!(task["creator_id"], member.to_string());
    assert_eq!(task["creator_service_account"], Value::Null);
    assert_eq!(task["external_id"], Value::Null);
    assert_eq!(f.notifications(member).await, 0);

    // The number is taken; later tasks are numbered after it.
    let open = f.status("unstarted").await;
    let (code, _) = f
        .call(&token, "POST", "/tasks", Some(json!({"project_id": f.project, "status_id": open, "title": "Again", "number": 166})))
        .await;
    assert_eq!(code, StatusCode::CONFLICT);
    let (code, next) = f
        .call(
            &token,
            "POST",
            "/tasks",
            Some(json!({"project_id": f.project, "status_id": open, "title": "Next"})),
        )
        .await;
    assert_eq!(code, StatusCode::CREATED);
    assert_eq!(next["number"], 167);

    // updated_at before created_at, and a guest as creator, are refused.
    let (code, _) = f
        .call(&token, "POST", "/tasks", Some(json!({"project_id": f.project, "status_id": open, "title": "Bad", "created_at": "2024-02-01T00:00:00.000Z", "updated_at": "2024-01-01T00:00:00.000Z"})))
        .await;
    assert_eq!(code, StatusCode::BAD_REQUEST);
    let guest = f.member("guest@example.com", "guest").await;
    let (code, _) = f
        .call(&token, "POST", "/tasks", Some(json!({"project_id": f.project, "status_id": open, "title": "Bad", "creator_id": guest})))
        .await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn external_ids_make_a_restarted_import_return_what_it_wrote() {
    let f = Fixture::new().await;
    let token = f.token(IMPORT).await;
    let open = f.status("unstarted").await;
    let body = json!({"project_id": f.project, "status_id": open, "title": "Once", "external_id": "plane:abc"});
    let (code, first) = f.call(&token, "POST", "/tasks", Some(body.clone())).await;
    assert_eq!(code, StatusCode::CREATED);
    let (code, again) = f.call(&token, "POST", "/tasks", Some(body)).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(again["id"], first["id"]);
    assert_eq!(first["external_id"], "plane:abc");
    assert_eq!(first["completed_at"], Value::Null);
    let path = format!("/tasks/{}/comments", first["id"].as_str().unwrap());
    let comment = json!({"body": "Hi", "external_id": "plane:c1"});
    let (code, one) = f.call(&token, "POST", &path, Some(comment.clone())).await;
    assert_eq!(code, StatusCode::CREATED);
    let (code, two) = f.call(&token, "POST", &path, Some(comment)).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(two["id"], one["id"]);
    assert_eq!(two["external_id"], "plane:c1");
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM task_comments WHERE task_id = $1")
        .bind(first["id"].as_str().unwrap())
        .fetch_one(f.database.pool())
        .await
        .unwrap();
    assert_eq!(count, 1);

    // A task in the trash keeps its ID.
    let (code, _) = f
        .call(
            &token,
            "DELETE",
            &format!(
                "/tasks/{}?expected_version={}",
                first["id"].as_str().unwrap(),
                first["version"]
            ),
            None,
        )
        .await;
    assert_eq!(code, StatusCode::NO_CONTENT);
    let (code, _) = f
        .call(&token, "POST", "/tasks", Some(json!({"project_id": f.project, "status_id": open, "title": "Once", "external_id": "plane:abc"})))
        .await;
    assert_eq!(code, StatusCode::CONFLICT);
}

#[tokio::test]
async fn imported_comments_keep_their_author_dates_and_threads() {
    let f = Fixture::new().await;
    let member = f.member("member@example.com", "member").await;
    let token = f.token(IMPORT).await;
    let open = f.status("unstarted").await;
    let (_, task) = f
        .call(
            &token,
            "POST",
            "/tasks",
            Some(json!({"project_id": f.project, "status_id": open, "title": "Talk"})),
        )
        .await;
    let path = format!("/tasks/{}/comments", task["id"].as_str().unwrap());
    let (code, first) = f
        .call(&token, "POST", &path, Some(json!({"body": "First", "author_id": member, "created_at": "2023-05-06T07:08:09.000Z"})))
        .await;
    assert_eq!(code, StatusCode::CREATED, "{first}");
    assert_eq!(first["author_id"], member.to_string());
    assert_eq!(first["created_at"], "2023-05-06T07:08:09.000Z");
    assert_eq!(first["updated_at"], "2023-05-06T07:08:09.000Z");
    let (code, reply) = f
        .call(
            &token,
            "POST",
            &path,
            Some(
                json!({"body": "Reply", "author_name": "Ada *Lovelace*", "parent_id": first["id"]}),
            ),
        )
        .await;
    assert_eq!(code, StatusCode::CREATED, "{reply}");
    assert_eq!(reply["parent_id"], first["id"]);
    assert_eq!(reply["body"], "**Ada \\*Lovelace\\***\n\nReply");
    let (code, _) = f
        .call(
            &token,
            "POST",
            &path,
            Some(json!({"body": "Both", "author_name": "Ada", "author_id": member})),
        )
        .await;
    assert_eq!(code, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn setup_writes_make_statuses_and_labels_and_rename_projects() {
    let f = Fixture::new().await;
    let token = f.token(&[ApiTokenScope::TasksWrite]).await;
    let project = format!("/projects/{}", f.project);
    let (code, status) = f
        .call(
            &token,
            "POST",
            &format!("{project}/statuses"),
            Some(json!({"name": "In review", "category": "started"})),
        )
        .await;
    assert_eq!(code, StatusCode::CREATED, "{status}");
    assert_eq!(status["color"], "#f2c94c");
    let (code, status) = f
        .call(
            &token,
            "PATCH",
            &format!("{project}/statuses/{}", status["id"].as_str().unwrap()),
            Some(json!({"expected_version": status["version"], "name": "Review", "category": "completed"})),
        )
        .await;
    assert_eq!(code, StatusCode::OK, "{status}");
    assert_eq!(
        (status["name"].as_str(), status["category"].as_str()),
        (Some("Review"), Some("completed"))
    );
    let (code, _) = f
        .call(
            &token,
            "POST",
            &format!("{project}/statuses"),
            Some(json!({"name": "Dup", "category": "duplicate"})),
        )
        .await;
    assert_eq!(code, StatusCode::BAD_REQUEST);

    let (code, label) = f
        .call(
            &token,
            "POST",
            &format!("{project}/labels"),
            Some(json!({"name": "Bug", "color": "#EB5757"})),
        )
        .await;
    assert_eq!(code, StatusCode::CREATED, "{label}");
    assert_eq!(label["color"], "#eb5757");
    let (code, _) = f
        .call(
            &token,
            "POST",
            &format!("{project}/labels"),
            Some(json!({"name": "Bug"})),
        )
        .await;
    assert_eq!(code, StatusCode::BAD_REQUEST);
    let (code, label) = f
        .call(
            &token,
            "PATCH",
            &format!("{project}/labels/{}", label["id"].as_str().unwrap()),
            Some(json!({"expected_version": label["version"], "name": "Defect"})),
        )
        .await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(
        (label["name"].as_str(), label["color"].as_str()),
        (Some("Defect"), Some("#eb5757"))
    );

    let (_, projects) = f.call(&token, "GET", "/projects", None).await;
    let version = projects["items"][0]["version"].clone();
    let (code, renamed) = f
        .call(
            &token,
            "PATCH",
            &project,
            Some(json!({"expected_version": version, "name": "Platform"})),
        )
        .await;
    assert_eq!(code, StatusCode::OK, "{renamed}");
    assert_eq!(renamed["name"], "Platform");
    let (code, _) = f
        .call(
            &token,
            "PATCH",
            &project,
            Some(json!({"expected_version": version, "name": "Stale"})),
        )
        .await;
    assert_eq!(code, StatusCode::CONFLICT);
    // A project outside the grant is not found.
    let (code, _) = f
        .call(
            &token,
            "POST",
            &format!("/projects/{}/labels", f.other_project),
            Some(json!({"name": "X"})),
        )
        .await;
    assert_eq!(code, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn relations_link_tasks_of_approved_projects() {
    let f = Fixture::new().await;
    let token = f.token(&[ApiTokenScope::TasksWrite]).await;
    let open = f.status("unstarted").await;
    let mut ids = Vec::new();
    for title in ["Blocker", "Blocked", "Copy"] {
        let (_, task) = f
            .call(
                &token,
                "POST",
                "/tasks",
                Some(json!({"project_id": f.project, "status_id": open, "title": title})),
            )
            .await;
        ids.push(task["id"].as_str().unwrap().to_owned());
    }
    let (code, relation) = f
        .call(
            &token,
            "POST",
            &format!("/tasks/{}/relations", ids[0]),
            Some(json!({"type": "blocks", "task_id": ids[1]})),
        )
        .await;
    assert_eq!(code, StatusCode::CREATED, "{relation}");
    assert_eq!(
        (relation["type"].as_str(), relation["direction"].as_str()),
        (Some("blocks"), Some("outgoing"))
    );
    let (_, list) = f
        .call(&token, "GET", &format!("/tasks/{}/relations", ids[1]), None)
        .await;
    assert_eq!(list["items"][0]["direction"], "incoming");

    // A task outside the grant cannot be linked.
    let secret: String =
        sqlx::query_scalar("SELECT id FROM task_statuses WHERE project_id = $1 LIMIT 1")
            .bind(f.other_project.to_string())
            .fetch_one(f.database.pool())
            .await
            .unwrap();
    let hidden = TaskRepository::new((*f.database).clone())
        .create_task(
            f.workspace,
            f.user,
            orbit_server::repositories::tasks::CreateTask {
                project_id: f.other_project,
                status_id: secret.parse().unwrap(),
                title: "Secret".into(),
                description: String::new(),
                description_json: None,
                source_url: None,
                priority: "none".into(),
                position: None,
                assignee_ids: vec![],
                label_ids: vec![],
                due_start_at: None,
                due_at: None,
                parent_task_id: None,
                milestone_id: None,
                cycle_id: None,
                estimate: None,
            },
            "test",
            TimestampMillis::now(),
        )
        .await
        .unwrap()
        .task
        .id;
    let (code, _) = f
        .call(
            &token,
            "POST",
            &format!("/tasks/{}/relations", ids[0]),
            Some(json!({"type": "related", "task_id": hidden})),
        )
        .await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);

    let relation_path = format!(
        "/tasks/{}/relations/{}",
        ids[0],
        relation["id"].as_str().unwrap()
    );
    assert_eq!(
        f.call(&token, "DELETE", &relation_path, None).await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        f.call(&token, "DELETE", &relation_path, None).await.0,
        StatusCode::NOT_FOUND
    );

    // A duplicate goes through the task update and moves to the Duplicate status.
    let (_, copy) = f
        .call(&token, "GET", &format!("/tasks/{}", ids[2]), None)
        .await;
    let (code, copy) = f
        .call(
            &token,
            "PATCH",
            &format!("/tasks/{}", ids[2]),
            Some(json!({"expected_version": copy["version"], "duplicate_of_id": ids[0]})),
        )
        .await;
    assert_eq!(code, StatusCode::OK, "{copy}");
    assert_eq!(copy["status_id"], f.status("duplicate").await);
    let (_, list) = f
        .call(&token, "GET", &format!("/tasks/{}/relations", ids[2]), None)
        .await;
    assert_eq!(list["items"][0]["type"], "duplicate");
}
