//! The public REST API (`/api/public/v1`) for API tokens. SQLite by default, Postgres when
//! `ORBIT_TEST_POSTGRES_URL` is set.
use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use orbit_platform::{
    HttpPlatformLayer, Id, OriginPolicy, PasswordService, TestDatabase, TimestampMillis,
};
use orbit_server::public_api::{PublicApiState, router};
use orbit_server::repositories::api_tokens::{ApiTokenError, ApiTokenRepository, ApiTokenScope};
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::repositories::pages::{CreatePage, PageChanges, PageRepository, SpaceRequest};
use orbit_server::repositories::tasks::{CreateTask, TaskRepository};
use serde_json::{Value, json};
use tower::ServiceExt;

mod collab_support;
#[allow(dead_code)] // `collab_support` needs it; the shared-database tests live elsewhere.
mod shared_postgres;

struct Fixture {
    database: TestDatabase,
    app: axum::Router,
    tokens: ApiTokenRepository,
    tasks: TaskRepository,
    pages: Arc<PageRepository>,
    user: Id,
    workspace: Id,
    project: Id,
    other_project: Id,
    task: Id,
    other_task: Id,
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
        let other_task = create_task(
            &tasks,
            &database,
            setup.workspace_id,
            setup.user_id,
            other.id,
            "Secret task",
            None,
        )
        .await;
        let task = create_task(
            &tasks,
            &database,
            setup.workspace_id,
            setup.user_id,
            setup.project_id,
            "Approved task",
            None,
        )
        .await;
        let pages = Arc::new(PageRepository::new((*database).clone()));
        let tokens = ApiTokenRepository::new((*database).clone());
        let app = router(
            PublicApiState::new(
                Arc::new(tokens.clone()),
                Arc::new(tasks.clone()),
                "https://orbit.test",
            )
            .with_pages(Arc::clone(&pages)),
        )
        .layer(HttpPlatformLayer::new(OriginPolicy::new(
            "https://orbit.test",
        )));
        Self {
            database,
            app,
            tokens,
            tasks,
            pages,
            user: setup.user_id,
            workspace: setup.workspace_id,
            project: setup.project_id,
            other_project: other.id,
            task,
            other_task,
        }
    }

    async fn token(&self, scopes: &[ApiTokenScope], service_account: bool) -> (String, Id) {
        let issued = self
            .tokens
            .create(
                self.workspace,
                self.user,
                format!("cli-{}", Id::new_v7()),
                vec![self.project],
                scopes,
                service_account,
                None,
                "test",
                TimestampMillis::now(),
            )
            .await
            .unwrap();
        (issued.token, issued.api_token.id)
    }

    /// Calls `/api/public/v1{path}`; a path that is not `/me` or `/workspaces/…` is a path of
    /// the fixture's workspace (`/tasks` is `/workspaces/{workspace}/tasks`).
    async fn call(
        &self,
        token: Option<&str>,
        method: &str,
        path: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let path = if path == "/me" || path.starts_with("/workspaces/") {
            path.to_owned()
        } else {
            format!("/workspaces/{}{path}", self.workspace)
        };
        let mut builder = Request::builder()
            .method(method)
            .uri(format!("/api/public/v1{path}"));
        if let Some(token) = token {
            builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
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
            serde_json::from_slice(&bytes)
                .unwrap_or_else(|_| panic!("{}", String::from_utf8_lossy(&bytes)))
        };
        (status, value)
    }

    async fn first_status(&self, project: Id) -> Id {
        sqlx::query_scalar::<_, String>(
            "SELECT id FROM task_statuses WHERE project_id = $1 ORDER BY position LIMIT 1",
        )
        .bind(project.to_string())
        .fetch_one(self.database.pool())
        .await
        .unwrap()
        .parse()
        .unwrap()
    }
}

async fn create_task(
    tasks: &TaskRepository,
    database: &TestDatabase,
    workspace: Id,
    user: Id,
    project: Id,
    title: &str,
    parent_task_id: Option<Id>,
) -> Id {
    let status: String = sqlx::query_scalar(
        "SELECT id FROM task_statuses WHERE project_id = $1 ORDER BY position LIMIT 1",
    )
    .bind(project.to_string())
    .fetch_one(database.pool())
    .await
    .unwrap();
    tasks
        .create_task(
            workspace,
            user,
            CreateTask {
                project_id: project,
                status_id: status.parse().unwrap(),
                title: title.into(),
                description: "Description".into(),
                description_json: None,
                source_url: None,
                priority: "none".into(),
                position: None,
                assignee_ids: vec![],
                label_ids: vec![],
                due_start_at: None,
                due_at: None,
                parent_task_id,
            },
            "test",
            TimestampMillis::now(),
        )
        .await
        .unwrap()
        .task
        .id
}

#[tokio::test]
async fn read_endpoints_return_public_fields_of_approved_projects_only() {
    let f = Fixture::new().await;
    let label = f
        .tasks
        .create_label(
            f.workspace,
            f.project,
            f.user,
            "bug".into(),
            "#00ff00".into(),
            "test",
            TimestampMillis::now(),
        )
        .await
        .unwrap();
    let (token, token_id) = f.token(&[ApiTokenScope::TasksRead], false).await;

    let (status, me) = f.call(Some(&token), "GET", "/me", None).await;
    assert_eq!(status, StatusCode::OK, "{me}");
    assert_eq!(me["token"]["id"], token_id.to_string());
    assert_eq!(me["token"]["scopes"], json!(["tasks:read"]));
    assert_eq!(me["token"]["project_ids"], json!([f.project]));
    assert_eq!(me["token"]["expires_at"], Value::Null);
    assert_eq!(me["workspace"], json!({"id": f.workspace, "name": "Orbit"}));
    assert_eq!(
        me["user"],
        json!({"id": f.user, "email": "owner@example.com", "display_name": "Owner"})
    );
    assert_eq!(me["service_account"], Value::Null);

    let (status, projects) = f.call(Some(&token), "GET", "/projects", None).await;
    assert_eq!(status, StatusCode::OK, "{projects}");
    assert_eq!(projects["next_cursor"], Value::Null);
    let items = projects["items"].as_array().unwrap();
    assert_eq!(items.len(), 1, "{projects}");
    let project = items[0].as_object().unwrap();
    let mut keys = project.keys().cloned().collect::<Vec<_>>();
    keys.sort();
    assert_eq!(
        keys,
        [
            "color",
            "created_at",
            "id",
            "key",
            "name",
            "updated_at",
            "version"
        ]
    );
    assert_eq!(project["name"], "General");
    let key = project["key"].as_str().unwrap().to_owned();

    let (status, statuses) = f
        .call(
            Some(&token),
            "GET",
            &format!("/projects/{}/statuses", f.project),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{statuses}");
    let first = &statuses["items"][0];
    assert_eq!(first["id"], f.first_status(f.project).await.to_string());
    assert_eq!(first["project_id"], f.project.to_string());
    assert!(first.get("workspace_id").is_none(), "{first}");
    assert!(first["category"].is_string() && first["position"].is_i64());

    let (status, tasks) = f.call(Some(&token), "GET", "/tasks", None).await;
    assert_eq!(status, StatusCode::OK, "{tasks}");
    assert_eq!(tasks["items"].as_array().unwrap().len(), 1, "{tasks}");
    assert_eq!(tasks["items"][0]["id"], f.task.to_string());
    assert!(!tasks.to_string().contains("Secret task"));
    let (_, searched) = f
        .call(Some(&token), "GET", "/tasks?q=nothing-matches", None)
        .await;
    assert_eq!(searched["items"], json!([]));
    let (_, filtered) = f
        .call(
            Some(&token),
            "GET",
            &format!(
                "/tasks?priority=none&status_id={}&limit=1",
                f.first_status(f.project).await
            ),
            None,
        )
        .await;
    assert_eq!(filtered["items"][0]["id"], f.task.to_string(), "{filtered}");
    let (_, by_label) = f
        .call(
            Some(&token),
            "GET",
            &format!("/tasks?label_id={}", label.id),
            None,
        )
        .await;
    assert_eq!(by_label["items"], json!([]), "{by_label}");
    let (status, _) = f.call(Some(&token), "GET", "/tasks?limit=101", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = f
        .call(Some(&token), "GET", "/tasks?priority=critical", None)
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, task) = f
        .call(Some(&token), "GET", &format!("/tasks/{}", f.task), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{task}");
    let number = task["number"].as_i64().unwrap();
    let mut keys = task
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    keys.sort();
    assert_eq!(
        keys,
        [
            "assignee_ids",
            "created_at",
            "description",
            "due_at",
            "due_start_at",
            "id",
            "identifier",
            "label_ids",
            "number",
            "parent_task_id",
            "priority",
            "project_id",
            "status_id",
            "title",
            "updated_at",
            "url",
            "version"
        ]
    );
    assert_eq!(task["identifier"], format!("{key}-{number}"));
    assert_eq!(
        task["url"],
        format!(
            "https://orbit.test/tasks/{}?workspace={}",
            f.task, f.workspace
        )
    );
    let (status, by_identifier) = f
        .call(
            Some(&token),
            "GET",
            &format!("/tasks/{}-{number}", key.to_lowercase()),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{by_identifier}");
    assert_eq!(by_identifier, task);
}

#[tokio::test]
async fn resources_of_unapproved_projects_are_not_found() {
    let f = Fixture::new().await;
    let (token, _) = f
        .token(
            &[ApiTokenScope::TasksRead, ApiTokenScope::TasksWrite],
            false,
        )
        .await;
    let other = f.other_project;
    let task = f.other_task;
    let current = f.tasks.get_task(f.workspace, task, f.user).await.unwrap();
    let secret_key: String = sqlx::query_scalar("SELECT project_key FROM projects WHERE id = $1")
        .bind(other.to_string())
        .fetch_one(f.database.pool())
        .await
        .unwrap();
    let missing = Id::new_v7();
    let status = f.first_status(other).await;
    for (method, path, body) in [
        ("GET", format!("/tasks/{task}"), None),
        (
            "GET",
            format!("/tasks/{secret_key}-{}", current.number),
            None,
        ),
        ("GET", format!("/tasks/{missing}"), None),
        ("GET", format!("/tasks?project_id={other}"), None),
        ("GET", format!("/projects/{other}/statuses"), None),
        ("GET", "/projects/not-an-id/statuses".to_owned(), None),
        (
            "PATCH",
            format!("/tasks/{task}"),
            Some(json!({"expected_version": current.version, "title": "Changed"})),
        ),
        (
            "DELETE",
            format!("/tasks/{task}?expected_version={}", current.version),
            None,
        ),
        (
            "POST",
            "/tasks".to_owned(),
            Some(json!({"project_id": other, "status_id": status, "title": "Nope"})),
        ),
    ] {
        let (code, problem) = f.call(Some(&token), method, &path, body).await;
        assert_eq!(code, StatusCode::NOT_FOUND, "{method} {path}: {problem}");
        assert!(!problem.to_string().contains("Secret task"), "{problem}");
        assert_eq!(problem["status"], 404, "{problem}");
        assert!(problem["request_id"].is_string());
    }
    let after = f.tasks.get_task(f.workspace, task, f.user).await.unwrap();
    assert_eq!(after.version, current.version);
}

#[tokio::test]
async fn task_writes_follow_scope_version_and_workspace_rules() {
    let f = Fixture::new().await;
    let status = f.first_status(f.project).await;
    let current = f.tasks.get_task(f.workspace, f.task, f.user).await.unwrap();
    let (read_only, _) = f.token(&[ApiTokenScope::TasksRead], false).await;
    for (method, path, body) in [
        (
            "POST",
            "/tasks".to_owned(),
            Some(json!({"project_id": f.project, "status_id": status, "title": "x"})),
        ),
        (
            "PATCH",
            format!("/tasks/{}", f.task),
            Some(json!({"expected_version": current.version, "title": "x"})),
        ),
        (
            "DELETE",
            format!("/tasks/{}?expected_version={}", f.task, current.version),
            None,
        ),
    ] {
        let (code, problem) = f.call(Some(&read_only), method, &path, body).await;
        assert_eq!(code, StatusCode::FORBIDDEN, "{method} {path}: {problem}");
        assert_eq!(problem["code"], "insufficient_scope");
    }

    let (write_only, _) = f.token(&[ApiTokenScope::TasksWrite], false).await;
    // `tasks:write` includes `tasks:read`.
    let (code, _) = f.call(Some(&write_only), "GET", "/tasks", None).await;
    assert_eq!(code, StatusCode::OK);

    let label = f
        .tasks
        .create_label(
            f.workspace,
            f.project,
            f.user,
            "ops".into(),
            "#0000ff".into(),
            "test",
            TimestampMillis::now(),
        )
        .await
        .unwrap();
    let other_label = f
        .tasks
        .create_label(
            f.workspace,
            f.other_project,
            f.user,
            "secret".into(),
            "#0000ff".into(),
            "test",
            TimestampMillis::now(),
        )
        .await
        .unwrap();
    let (token, token_id) = f
        .token(
            &[ApiTokenScope::TasksRead, ApiTokenScope::TasksWrite],
            false,
        )
        .await;
    let (code, created) = f
        .call(
            Some(&token),
            "POST",
            "/tasks",
            Some(json!({
                "project_id": f.project, "status_id": status, "title": "From the CLI",
                "description": "Body", "priority": "high",
                "assignee_ids": [f.user], "label_ids": [label.id], "parent_task_id": f.task,
            })),
        )
        .await;
    assert_eq!(code, StatusCode::CREATED, "{created}");
    assert_eq!(created["title"], "From the CLI");
    assert_eq!(created["priority"], "high");
    assert_eq!(created["assignee_ids"], json!([f.user]));
    assert_eq!(created["label_ids"], json!([label.id]));
    assert_eq!(created["parent_task_id"], f.task.to_string());
    assert!(created["version"].is_u64(), "{created}");
    let id = created["id"].as_str().unwrap().to_owned();
    let (actor, metadata) = audit_of(&f, &id, "task.created").await;
    assert_eq!(actor.as_deref(), Some(f.user.to_string().as_str()));
    assert_eq!(metadata, json!({"api_token_id": token_id}));

    for body in [
        json!({"project_id": f.project, "status_id": status, "title": "x", "label_ids": [other_label.id]}),
        json!({"project_id": f.project, "status_id": status, "title": "x", "assignee_ids": [Id::new_v7()]}),
    ] {
        let (code, problem) = f
            .call(Some(&token), "POST", "/tasks", Some(body.clone()))
            .await;
        assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY, "{body}: {problem}");
    }
    let foreign_status = f.first_status(f.other_project).await;
    let (code, _) = f
        .call(
            Some(&token),
            "POST",
            "/tasks",
            Some(json!({"project_id": f.project, "status_id": foreign_status, "title": "x"})),
        )
        .await;
    assert_eq!(code, StatusCode::NOT_FOUND);
    for body in [
        json!({"project_id": f.project, "status_id": status, "title": " "}),
        json!({"project_id": f.project, "status_id": status, "title": "x", "priority": "p0"}),
        json!({"project_id": f.project, "status_id": status, "title": "x", "unknown": 1}),
        json!({"project_id": "nope", "status_id": status, "title": "x"}),
    ] {
        let (code, problem) = f
            .call(Some(&token), "POST", "/tasks", Some(body.clone()))
            .await;
        assert_eq!(code.as_u16() / 100, 4, "{body}: {problem}");
        assert_ne!(code, StatusCode::CREATED);
    }
    let (code, _) = f
        .call(
            Some(&token),
            "POST",
            "/tasks",
            Some(json!({"project_id": f.project, "status_id": status, "title": "x", "parent_task_id": f.other_task})),
        )
        .await;
    assert_eq!(code, StatusCode::NOT_FOUND);

    let (code, updated) = f
        .call(
            Some(&token),
            "PATCH",
            &format!("/tasks/{id}"),
            Some(json!({"expected_version": created["version"], "title": "Renamed", "assignee_ids": [], "label_ids": []})),
        )
        .await;
    assert_eq!(code, StatusCode::OK, "{updated}");
    assert_eq!(updated["title"], "Renamed");
    assert_eq!(updated["assignee_ids"], json!([]));
    assert_ne!(updated["version"], created["version"]);
    let (code, stale) = f
        .call(
            Some(&token),
            "PATCH",
            &format!("/tasks/{id}"),
            Some(json!({"expected_version": created["version"], "title": "Again"})),
        )
        .await;
    assert_eq!(code, StatusCode::CONFLICT, "{stale}");
    assert_eq!(stale["conflict"]["current_version"], updated["version"]);
    assert!(stale["conflict"].get("current").is_none(), "{stale}");
    let (code, _) = f
        .call(
            Some(&token),
            "PATCH",
            &format!("/tasks/{id}"),
            Some(json!({"expected_version": updated["version"], "label_ids": [other_label.id]})),
        )
        .await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);

    let (code, _) = f
        .call(
            Some(&token),
            "DELETE",
            &format!("/tasks/{id}?expected_version={}", created["version"]),
            None,
        )
        .await;
    assert_eq!(code, StatusCode::CONFLICT);
    let (code, _) = f
        .call(Some(&token), "DELETE", &format!("/tasks/{id}"), None)
        .await;
    assert_eq!(code, StatusCode::BAD_REQUEST);
    let (code, body) = f
        .call(
            Some(&token),
            "DELETE",
            &format!("/tasks/{id}?expected_version={}", updated["version"]),
            None,
        )
        .await;
    assert_eq!(code, StatusCode::NO_CONTENT, "{body}");
    let (code, _) = f
        .call(Some(&token), "GET", &format!("/tasks/{id}"), None)
        .await;
    assert_eq!(code, StatusCode::NOT_FOUND);
    let trashed: Option<i64> = sqlx::query_scalar("SELECT deleted_at FROM tasks WHERE id = $1")
        .bind(&id)
        .fetch_one(f.database.pool())
        .await
        .unwrap();
    assert!(trashed.is_some());
}

#[tokio::test]
async fn a_parent_in_an_unapproved_project_does_not_leak() {
    let f = Fixture::new().await;
    let child = create_task(
        &f.tasks,
        &f.database,
        f.workspace,
        f.user,
        f.project,
        "Visible child",
        Some(f.other_task),
    )
    .await;
    let (token, _) = f
        .token(
            &[ApiTokenScope::TasksRead, ApiTokenScope::TasksWrite],
            false,
        )
        .await;
    let (code, task) = f
        .call(Some(&token), "GET", &format!("/tasks/{child}"), None)
        .await;
    assert_eq!(code, StatusCode::OK, "{task}");
    assert_eq!(task["parent_task_id"], Value::Null);
    let (_, list) = f.call(Some(&token), "GET", "/tasks", None).await;
    for body in [task.to_string(), list.to_string()] {
        assert!(!body.contains("Secret task"), "{body}");
        assert!(!body.contains(&f.other_task.to_string()), "{body}");
    }
    // The tree reaches an unapproved project, so task automation could change it.
    let (code, _) = f
        .call(
            Some(&token),
            "PATCH",
            &format!("/tasks/{child}"),
            Some(json!({"expected_version": 0, "title": "x"})),
        )
        .await;
    assert_eq!(code, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn docs_need_their_scope_and_a_personal_token_and_unserved_paths_are_not_found() {
    let f = Fixture::new().await;
    let page = f
        .pages
        .create_page(
            f.workspace,
            f.user,
            CreatePage {
                kind: orbit_server::repositories::pages::PageKind::Page,
                parent_id: None,
                space: SpaceRequest::default(),
                title: "Runbook".into(),
                icon: None,
                position: None,
            },
            "test",
            TimestampMillis::now(),
        )
        .await
        .unwrap();
    let (plain, _) = f.token(&[ApiTokenScope::TasksRead], false).await;
    for path in ["/docs".to_owned(), format!("/docs/{}", page.id)] {
        let (code, problem) = f.call(Some(&plain), "GET", &path, None).await;
        assert_eq!(code, StatusCode::FORBIDDEN, "{path}");
        assert_eq!(problem["code"], "insufficient_scope");
    }

    let (token, _) = f
        .token(
            &[
                ApiTokenScope::TasksRead,
                ApiTokenScope::DocsRead,
                ApiTokenScope::ChatRead,
            ],
            false,
        )
        .await;
    let (code, me) = f.call(Some(&token), "GET", "/me", None).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(
        me["token"]["scopes"],
        json!(["tasks:read", "docs:read", "chat:read"])
    );
    let (code, docs) = f.call(Some(&token), "GET", "/docs", None).await;
    assert_eq!(code, StatusCode::OK, "{docs}");
    assert_eq!(docs["next_cursor"], Value::Null);
    assert!(
        docs["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["title"] == "Runbook"),
        "{docs}"
    );
    let (code, doc) = f
        .call(Some(&token), "GET", &format!("/docs/{}", page.id), None)
        .await;
    assert_eq!(code, StatusCode::OK, "{doc}");
    assert_eq!(doc["title"], "Runbook");
    let (code, _) = f
        .call(
            Some(&token),
            "GET",
            &format!("/docs/{}", Id::new_v7()),
            None,
        )
        .await;
    assert_eq!(code, StatusCode::NOT_FOUND);
    for path in [
        "/conversations".to_owned(),
        format!("/conversations/{}/messages", Id::new_v7()),
    ] {
        let response = f
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/public/v1/workspaces/{}{path}", f.workspace))
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
    }

    let service = old_service_account_token(
        &f,
        &[
            ApiTokenScope::TasksRead,
            ApiTokenScope::DocsRead,
            ApiTokenScope::ChatRead,
        ],
    )
    .await;
    let (code, me) = f.call(Some(&service), "GET", "/me", None).await;
    assert_eq!(code, StatusCode::OK, "{me}");
    assert_eq!(me["user"], Value::Null);
    assert!(
        me["service_account"]["name"]
            .as_str()
            .unwrap()
            .starts_with("cli-"),
        "{me}"
    );
    for path in ["/docs".to_owned(), format!("/docs/{}", page.id)] {
        let (code, body) = f.call(Some(&service), "GET", &path, None).await;
        assert_eq!(code, StatusCode::NOT_FOUND, "{path}: {body}");
    }
    let (code, _) = f.call(Some(&service), "GET", "/tasks", None).await;
    assert_eq!(code, StatusCode::OK);
}

#[tokio::test]
async fn docs_write_creates_and_updates_docs_from_markdown() {
    let f = Fixture::new().await;
    let (reader, _) = f.token(&[ApiTokenScope::DocsRead], false).await;
    let (code, problem) = f
        .call(Some(&reader), "POST", "/docs", Some(json!({"title": "x"})))
        .await;
    assert_eq!(code, StatusCode::FORBIDDEN, "{problem}");
    assert_eq!(problem["code"], "insufficient_scope");

    let (writer, _) = f.token(&[ApiTokenScope::DocsWrite], false).await;
    let (code, doc) = f
        .call(
            Some(&writer),
            "POST",
            "/docs",
            Some(json!({"title": "Release plan", "private": true, "markdown": "# Steps\n\nShip **it**."})),
        )
        .await;
    assert_eq!(code, StatusCode::CREATED, "{doc}");
    assert_eq!(doc["title"], "Release plan");
    assert_eq!(doc["private"], true);
    assert_eq!(doc["content"][0]["type"], "heading", "{doc}");
    assert_eq!(doc["content"][0]["content"][0]["text"], "Steps", "{doc}");
    let id = doc["id"].as_str().unwrap().to_owned();
    let version = doc["version"].clone();

    // `docs:write` includes `docs:read`, and the stored doc has the new content.
    let (code, read) = f
        .call(Some(&writer), "GET", &format!("/docs/{id}"), None)
        .await;
    assert_eq!(code, StatusCode::OK, "{read}");
    assert_eq!(read["content"], doc["content"]);
    // Reads also give the content as Markdown, the format that writes take.
    assert_eq!(read["markdown"], "# Steps\n\nShip **it**.\n", "{read}");
    assert_eq!(doc["markdown"], read["markdown"]);
    // The list shows the same version, so a list read is enough for a write.
    let (_, list) = f.call(Some(&writer), "GET", "/docs", None).await;
    let listed = list["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == id.as_str())
        .unwrap();
    assert_eq!(listed["version"], read["version"], "{list}");

    let (code, child) = f
        .call(
            Some(&writer),
            "POST",
            "/docs",
            Some(json!({"parent_id": id, "title": "Notes"})),
        )
        .await;
    assert_eq!(code, StatusCode::CREATED, "{child}");
    assert_eq!(child["parent_id"], id.as_str());
    assert_eq!(child["content"], json!([]));
    assert_eq!(child["markdown"], "");

    let path = format!("/docs/{id}");
    let (code, updated) = f
        .call(
            Some(&writer),
            "PATCH",
            &path,
            Some(json!({"expected_version": version, "title": "Release plan v2", "icon": "🚀", "markdown": "- one\n- two"})),
        )
        .await;
    assert_eq!(code, StatusCode::OK, "{updated}");
    assert_eq!(updated["title"], "Release plan v2");
    assert_eq!(updated["icon"], "🚀");
    assert_eq!(updated["content"][0]["type"], "bulletListItem", "{updated}");
    assert_ne!(updated["version"], version);
    assert_eq!(updated["markdown"], "- one\n- two\n", "{updated}");

    let (code, problem) = f
        .call(
            Some(&writer),
            "PATCH",
            &path,
            Some(json!({"expected_version": version, "title": "stale"})),
        )
        .await;
    assert_eq!(code, StatusCode::CONFLICT, "{problem}");

    // `null` clears the icon; an absent field keeps the title and content.
    let (code, cleared) = f
        .call(
            Some(&writer),
            "PATCH",
            &path,
            Some(json!({"expected_version": updated["version"], "icon": null})),
        )
        .await;
    assert_eq!(code, StatusCode::OK, "{cleared}");
    assert_eq!(cleared["icon"], Value::Null);
    assert_eq!(cleared["title"], "Release plan v2");
    assert_eq!(cleared["content"], updated["content"]);

    sqlx::query("UPDATE pages SET locked_at = 1 WHERE id = $1")
        .bind(&id)
        .execute(f.database.pool())
        .await
        .unwrap();
    let (code, problem) = f
        .call(
            Some(&writer),
            "PATCH",
            &path,
            Some(json!({"expected_version": cleared["version"], "markdown": "locked"})),
        )
        .await;
    assert_eq!(code, StatusCode::LOCKED, "{problem}");
    assert_eq!(problem["code"], "page_locked");

    let (code, _) = f
        .call(
            Some(&writer),
            "PATCH",
            &format!("/docs/{}", Id::new_v7()),
            Some(json!({"expected_version": 0, "title": "x"})),
        )
        .await;
    assert_eq!(code, StatusCode::NOT_FOUND);
    let (code, _) = f
        .call(
            Some(&writer),
            "POST",
            "/docs",
            Some(json!({"title": "x".repeat(501)})),
        )
        .await;
    assert_eq!(code, StatusCode::BAD_REQUEST);

    // Docs act as the token's creator, so a service account cannot write them.
    let service = old_service_account_token(&f, &[ApiTokenScope::DocsWrite]).await;
    let (code, _) = f
        .call(Some(&service), "POST", "/docs", Some(json!({"title": "x"})))
        .await;
    assert_eq!(code, StatusCode::NOT_FOUND);
}

async fn count(f: &Fixture, sql: &str, id: &str) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_owned()))
        .bind(id)
        .fetch_one(f.database.pool())
        .await
        .unwrap()
}

#[tokio::test]
async fn a_doc_patch_that_changes_nothing_keeps_the_version() {
    let f = Fixture::new().await;
    let (writer, _) = f.token(&[ApiTokenScope::DocsWrite], false).await;
    let (code, doc) = f
        .call(
            Some(&writer),
            "POST",
            "/docs",
            Some(json!({"title": "Plan", "icon": "🚀", "markdown": "# Steps\n\nShip **it**."})),
        )
        .await;
    assert_eq!(code, StatusCode::CREATED, "{doc}");
    let id = doc["id"].as_str().unwrap().to_owned();
    let path = format!("/docs/{id}");
    let audits =
        "SELECT COUNT(*) FROM audit_events WHERE resource_id = $1 AND action = 'page.updated'";
    let snapshots = "SELECT COUNT(*) FROM page_versions WHERE page_id = $1";
    let (audited, snapshotted) = (
        count(&f, audits, &id).await,
        count(&f, snapshots, &id).await,
    );

    for body in [
        json!({"expected_version": doc["version"], "title": "Plan", "icon": "🚀", "markdown": doc["markdown"]}),
        json!({"expected_version": doc["version"], "markdown": "# Steps\n\nShip **it**.\n"}),
        json!({"expected_version": doc["version"]}),
    ] {
        let (code, same) = f
            .call(Some(&writer), "PATCH", &path, Some(body.clone()))
            .await;
        assert_eq!(code, StatusCode::OK, "{body}: {same}");
        assert_eq!(same["version"], doc["version"], "{body}");
        assert_eq!(same["updated_at"], doc["updated_at"], "{body}");
        assert_eq!(same["markdown"], doc["markdown"], "{body}");
    }
    assert_eq!(count(&f, audits, &id).await, audited);
    assert_eq!(count(&f, snapshots, &id).await, snapshotted);
    let (_, read) = f.call(Some(&writer), "GET", &path, None).await;
    assert_eq!(read["version"], doc["version"]);

    // A stale version still conflicts, even when the write would change nothing.
    let (code, problem) = f
        .call(
            Some(&writer),
            "PATCH",
            &path,
            Some(json!({"expected_version": 1, "title": "Plan"})),
        )
        .await;
    assert_eq!(code, StatusCode::CONFLICT, "{problem}");

    // A real change still makes a new version.
    let (code, changed) = f
        .call(
            Some(&writer),
            "PATCH",
            &path,
            Some(
                json!({"expected_version": doc["version"], "markdown": "# Steps\n\nShip it now."}),
            ),
        )
        .await;
    assert_eq!(code, StatusCode::OK, "{changed}");
    assert_ne!(changed["version"], doc["version"]);
    assert_eq!(count(&f, audits, &id).await, audited + 1);
}

#[tokio::test]
async fn bad_expired_and_revoked_tokens_are_unauthorized() {
    let f = Fixture::new().await;
    let (expired, expired_id) = f.token(&[ApiTokenScope::TasksRead], false).await;
    sqlx::query("UPDATE api_tokens SET expires_at = $1 WHERE id = $2")
        .bind(TimestampMillis::now().as_millis() - 1)
        .bind(expired_id.to_string())
        .execute(f.database.pool())
        .await
        .unwrap();
    let (revoked, revoked_id) = f.token(&[ApiTokenScope::TasksRead], false).await;
    f.tokens
        .revoke(
            f.workspace,
            revoked_id,
            f.user,
            "test",
            TimestampMillis::now(),
        )
        .await
        .unwrap();
    for token in [
        None,
        Some("orb_not-a-token"),
        Some(expired.as_str()),
        Some(revoked.as_str()),
    ] {
        let mut request = Request::builder().uri("/api/public/v1/me");
        if let Some(token) = token {
            request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        let response = f
            .app
            .clone()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{token:?}");
        assert_eq!(
            response.headers()[header::WWW_AUTHENTICATE],
            "Bearer realm=\"orbit\""
        );
        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            "application/problem+json"
        );
    }
    let (code, _) = f.call(Some(&revoked), "GET", "/tasks", None).await;
    assert_eq!(code, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn the_application_mounts_the_router_without_origin_or_contract_checks() {
    let root = tempfile::TempDir::new().unwrap();
    let mut config = orbit_platform::Config {
        environment: orbit_platform::EnvironmentMode::Development,
        app_key: Some(orbit_platform::AppKey::new([1; 32])),
        ..Default::default()
    };
    config.data.database = root.path().join("orbit.sqlite");
    config.data.attachments = root.path().join("attachments");
    config.data.backups = root.path().join("backups");
    let app = orbit_server::app::App::build(config).await.unwrap();
    // A write with a bearer token and no Origin or contract header reaches authentication.
    let response = app
        .router()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/public/v1/workspaces/{}/tasks", Id::new_v7()))
                .header(header::AUTHORIZATION, "Bearer orb_unknown")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(response.headers().contains_key(header::WWW_AUTHENTICATE));

    // A method that the path does not serve is a 405 problem that names the methods it serves.
    let response = app
        .router()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("/api/public/v1/workspaces/{}/tasks", Id::new_v7()))
                .header(header::AUTHORIZATION, "Bearer orb_unknown")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(response.headers()[header::ALLOW], "GET,HEAD,POST");
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "application/problem+json"
    );
    let problem: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
    assert_eq!(problem["code"], "method_not_allowed");
    assert_eq!(problem["status"], 405);
}

async fn audit_of(f: &Fixture, resource_id: &str, action: &str) -> (Option<String>, Value) {
    let (actor, metadata): (Option<String>, String) = sqlx::query_as(
        "SELECT actor_id, metadata_json FROM audit_events WHERE resource_id = $1 AND action = $2",
    )
    .bind(resource_id)
    .bind(action)
    .fetch_one(f.database.pool())
    .await
    .unwrap();
    (actor, serde_json::from_str(&metadata).unwrap())
}

#[tokio::test]
async fn service_account_writes_name_the_service_account_and_the_token() {
    let f = Fixture::new().await;
    let (token, token_id) = f.token(&[ApiTokenScope::TasksWrite], true).await;
    let (_, me) = f.call(Some(&token), "GET", "/me", None).await;
    let account = me["service_account"].clone();
    let status = f.first_status(f.project).await;
    let (code, created) = f
        .call(
            Some(&token),
            "POST",
            "/tasks",
            Some(json!({"project_id": f.project, "status_id": status, "title": "From CI"})),
        )
        .await;
    assert_eq!(code, StatusCode::CREATED, "{created}");
    let id = created["id"].as_str().unwrap().to_owned();
    let creator: Option<String> =
        sqlx::query_scalar("SELECT creator_service_account_id FROM tasks WHERE id = $1")
            .bind(&id)
            .fetch_one(f.database.pool())
            .await
            .unwrap();
    assert_eq!(creator.as_deref(), account["id"].as_str());
    let (code, renamed) = f
        .call(
            Some(&token),
            "PATCH",
            &format!("/tasks/{id}"),
            Some(json!({"expected_version": created["version"], "title": "Renamed"})),
        )
        .await;
    assert_eq!(code, StatusCode::OK);
    let (code, _) = f
        .call(
            Some(&token),
            "DELETE",
            &format!("/tasks/{id}?expected_version={}", renamed["version"]),
            None,
        )
        .await;
    assert_eq!(code, StatusCode::NO_CONTENT);
    for action in ["task.created", "task.updated", "task.deleted"] {
        let (actor, metadata) = audit_of(&f, &id, action).await;
        assert_eq!(actor, None, "{action}");
        assert_eq!(metadata["api_token_id"], json!(token_id), "{action}");
        assert_eq!(
            metadata["actor_service_account_id"], account["id"],
            "{action}"
        );
        assert_eq!(
            metadata["actor_service_account_name"], account["name"],
            "{action}"
        );
    }
}

#[tokio::test]
async fn invalid_input_gets_a_fixed_message() {
    let f = Fixture::new().await;
    let (token, _) = f.token(&[ApiTokenScope::TasksWrite], false).await;
    let (code, problem) = f
        .call(
            Some(&token),
            "POST",
            "/tasks",
            Some(json!({"project_id": f.project, "title": "x", "secret_field_xyz": "<script>"})),
        )
        .await;
    assert_eq!(code, StatusCode::BAD_REQUEST);
    assert_eq!(problem["code"], "invalid_request");
    assert_eq!(
        problem["detail"],
        "The query string or JSON body is invalid. Check the field names, types and required fields."
    );
    let (code, problem) = f
        .call(
            Some(&token),
            "GET",
            "/tasks?limit=abc&secret_param_xyz=1",
            None,
        )
        .await;
    assert_eq!(code, StatusCode::BAD_REQUEST);
    assert!(
        !problem.to_string().contains("secret_param_xyz"),
        "{problem}"
    );
}

#[tokio::test]
async fn paths_name_the_token_s_workspace() {
    let f = Fixture::new().await;
    let (token, token_id) = f.token(&[ApiTokenScope::TasksRead], false).await;
    let (code, me) = f.call(Some(&token), "GET", "/me", None).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(
        me["workspaces"],
        json!([{"id": f.workspace, "name": "Orbit", "scopes": ["tasks:read"], "project_ids": [f.project]}])
    );
    assert_eq!(me["token"]["id"], json!(token_id));

    let (code, _) = f
        .call(
            Some(&token),
            "GET",
            &format!("/workspaces/{}/tasks", f.workspace),
            None,
        )
        .await;
    assert_eq!(code, StatusCode::OK);
    for workspace in [Id::new_v7().to_string(), "not-an-id".to_owned()] {
        for path in [
            "tasks".to_owned(),
            format!("tasks/{}", f.task),
            "projects".to_owned(),
        ] {
            let (code, problem) = f
                .call(
                    Some(&token),
                    "GET",
                    &format!("/workspaces/{workspace}/{path}"),
                    None,
                )
                .await;
            assert_eq!(code, StatusCode::NOT_FOUND, "{workspace}/{path}: {problem}");
            assert!(!problem.to_string().contains("Approved task"));
        }
    }
    for path in [
        "/api/public/v1/tasks",
        "/api/public/v1/projects",
        "/api/public/v1/docs",
    ] {
        let response = f
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(path)
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
    }
}

#[tokio::test]
async fn labels_and_members_show_only_what_assigning_needs() {
    let f = Fixture::new().await;
    let label = f
        .tasks
        .create_label(
            f.workspace,
            f.project,
            f.user,
            "bug".into(),
            "#00ff00".into(),
            "test",
            TimestampMillis::now(),
        )
        .await
        .unwrap();
    f.tasks
        .create_label(
            f.workspace,
            f.other_project,
            f.user,
            "secret-label".into(),
            "#00ff00".into(),
            "test",
            TimestampMillis::now(),
        )
        .await
        .unwrap();
    let (token, _) = f.token(&[ApiTokenScope::TasksRead], false).await;
    let (code, labels) = f
        .call(
            Some(&token),
            "GET",
            &format!("/projects/{}/labels", f.project),
            None,
        )
        .await;
    assert_eq!(code, StatusCode::OK, "{labels}");
    assert_eq!(
        labels,
        json!({"items": [{"id": label.id, "project_id": f.project, "name": "bug", "color": "#00ff00", "version": 0}], "next_cursor": null})
    );
    let (code, problem) = f
        .call(
            Some(&token),
            "GET",
            &format!("/projects/{}/labels", f.other_project),
            None,
        )
        .await;
    assert_eq!(code, StatusCode::NOT_FOUND);
    assert!(!problem.to_string().contains("secret-label"));

    let (code, members) = f.call(Some(&token), "GET", "/members", None).await;
    assert_eq!(code, StatusCode::OK, "{members}");
    assert_eq!(
        members,
        json!({"items": [{"user_id": f.user, "display_name": "Owner"}], "next_cursor": null})
    );

    let (docs_only, _) = f.token(&[ApiTokenScope::DocsRead], false).await;
    let (code, _) = f.call(Some(&docs_only), "GET", "/members", None).await;
    assert_eq!(code, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn comments_follow_the_grant_scopes_and_attribution() {
    let f = Fixture::new().await;
    let (writer, writer_id) = f.token(&[ApiTokenScope::TasksWrite], false).await;
    let (reader, _) = f.token(&[ApiTokenScope::TasksRead], false).await;
    let (bot, bot_id) = f.token(&[ApiTokenScope::TasksWrite], true).await;
    let path = format!("/tasks/{}/comments", f.task);

    let (code, empty) = f.call(Some(&reader), "GET", &path, None).await;
    assert_eq!(code, StatusCode::OK, "{empty}");
    assert_eq!(empty, json!({"items": [], "next_cursor": null}));
    let (code, comment) = f
        .call(
            Some(&writer),
            "POST",
            &path,
            Some(json!({"body": "Shipped in **512**"})),
        )
        .await;
    assert_eq!(code, StatusCode::CREATED, "{comment}");
    assert_eq!(comment["body"], "Shipped in **512**");
    assert_eq!(comment["task_id"], f.task.to_string());
    assert_eq!(comment["author_id"], f.user.to_string());
    let id = comment["id"].as_str().unwrap().to_owned();
    let (_, listed) = f.call(Some(&reader), "GET", &path, None).await;
    assert_eq!(listed["items"][0]["id"], id);
    let (actor, metadata) = audit_of(&f, &id, "comment.created").await;
    assert_eq!(actor.as_deref(), Some(f.user.to_string().as_str()));
    assert_eq!(metadata, json!({"api_token_id": writer_id}));

    let (code, by_bot) = f
        .call(Some(&bot), "POST", &path, Some(json!({"body": "From CI"})))
        .await;
    assert_eq!(code, StatusCode::CREATED, "{by_bot}");
    let (actor, metadata) = audit_of(&f, by_bot["id"].as_str().unwrap(), "comment.created").await;
    assert_eq!(actor, None);
    assert_eq!(metadata["api_token_id"], json!(bot_id));
    assert!(
        metadata["actor_service_account_name"].is_string(),
        "{metadata}"
    );

    let (code, problem) = f
        .call(Some(&reader), "POST", &path, Some(json!({"body": "no"})))
        .await;
    assert_eq!(code, StatusCode::FORBIDDEN);
    assert_eq!(problem["code"], "insufficient_scope");
    let (code, _) = f
        .call(Some(&writer), "POST", &path, Some(json!({"body": "  "})))
        .await;
    assert_eq!(code, StatusCode::BAD_REQUEST);

    let secret = format!("/tasks/{}/comments", f.other_task);
    let (code, _) = f.call(Some(&writer), "GET", &secret, None).await;
    assert_eq!(code, StatusCode::NOT_FOUND);
    let (code, _) = f
        .call(
            Some(&writer),
            "POST",
            &secret,
            Some(json!({"body": "leak"})),
        )
        .await;
    assert_eq!(code, StatusCode::NOT_FOUND);
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM task_comments WHERE task_id = $1")
        .bind(f.other_task.to_string())
        .fetch_one(f.database.pool())
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn due_dates_can_be_set_kept_and_cleared() {
    let f = Fixture::new().await;
    let (token, _) = f.token(&[ApiTokenScope::TasksWrite], false).await;
    let status = f.first_status(f.project).await;
    let (code, task) = f
        .call(
            Some(&token),
            "POST",
            "/tasks",
            Some(
                json!({"project_id": f.project, "status_id": status, "title": "Due",
                "due_start_at": "2026-12-01T00:00:00.000Z", "due_at": "2026-12-05T00:00:00.000Z"}),
            ),
        )
        .await;
    assert_eq!(code, StatusCode::CREATED, "{task}");
    assert_eq!(task["due_start_at"], "2026-12-01T00:00:00.000Z");
    assert_eq!(task["due_at"], "2026-12-05T00:00:00.000Z");
    let id = task["id"].as_str().unwrap().to_owned();
    let (code, kept) = f
        .call(
            Some(&token),
            "PATCH",
            &format!("/tasks/{id}"),
            Some(json!({"expected_version": task["version"], "title": "Due 2"})),
        )
        .await;
    assert_eq!(code, StatusCode::OK, "{kept}");
    assert_eq!(kept["due_at"], "2026-12-05T00:00:00.000Z");
    let (code, cleared) = f
        .call(
            Some(&token),
            "PATCH",
            &format!("/tasks/{id}"),
            Some(
                json!({"expected_version": kept["version"], "due_start_at": null, "due_at": null}),
            ),
        )
        .await;
    assert_eq!(code, StatusCode::OK, "{cleared}");
    assert_eq!(cleared["due_at"], Value::Null);
    assert_eq!(cleared["due_start_at"], Value::Null);
    let (code, _) = f
        .call(
            Some(&token),
            "POST",
            "/tasks",
            Some(
                json!({"project_id": f.project, "status_id": status, "title": "Bad",
                "due_start_at": "2026-12-09T00:00:00.000Z", "due_at": "2026-12-05T00:00:00.000Z"}),
            ),
        )
        .await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);
    let (code, _) = f
        .call(Some(&token), "POST", "/tasks", Some(json!({"project_id": f.project, "status_id": status, "title": "Bad", "due_at": "tomorrow"})))
        .await;
    assert_eq!(code, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn sub_issues_take_a_parent_by_identifier_and_can_move_and_detach() {
    let f = Fixture::new().await;
    let (token, _) = f.token(&[ApiTokenScope::TasksWrite], false).await;
    let status = f.first_status(f.project).await;
    let (_, parent) = f
        .call(Some(&token), "GET", &format!("/tasks/{}", f.task), None)
        .await;
    let identifier = parent["identifier"].as_str().unwrap().to_lowercase();
    let (code, child) = f
        .call(
            Some(&token),
            "POST",
            "/tasks",
            Some(json!({"project_id": f.project, "status_id": status, "title": "Child", "parent_task_id": identifier})),
        )
        .await;
    assert_eq!(code, StatusCode::CREATED, "{child}");
    assert_eq!(child["parent_task_id"], f.task.to_string());
    let (code, list) = f
        .call(
            Some(&token),
            "GET",
            &format!("/tasks?parent_task_id={identifier}"),
            None,
        )
        .await;
    assert_eq!(code, StatusCode::OK, "{list}");
    assert_eq!(list["items"].as_array().unwrap().len(), 1);
    assert_eq!(list["items"][0]["id"], child["id"]);
    let id = child["id"].as_str().unwrap().to_owned();
    let (code, cycle) = f
        .call(
            Some(&token),
            "PATCH",
            &format!("/tasks/{}", f.task),
            Some(json!({"expected_version": parent["version"], "parent_task_id": id})),
        )
        .await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY, "{cycle}");
    assert_eq!(cycle["code"], "parent_cycle");
    let (code, detached) = f
        .call(
            Some(&token),
            "PATCH",
            &format!("/tasks/{id}"),
            Some(json!({"expected_version": child["version"], "parent_task_id": null})),
        )
        .await;
    assert_eq!(code, StatusCode::OK, "{detached}");
    assert_eq!(detached["parent_task_id"], Value::Null);
    let (code, hidden) = f
        .call(
            Some(&token),
            "PATCH",
            &format!("/tasks/{id}"),
            Some(json!({"expected_version": detached["version"], "parent_task_id": f.other_task})),
        )
        .await;
    assert_eq!(code, StatusCode::NOT_FOUND, "{hidden}");
    let (code, attached) = f
        .call(
            Some(&token),
            "PATCH",
            &format!("/tasks/{id}"),
            Some(json!({"expected_version": detached["version"], "parent_task_id": identifier})),
        )
        .await;
    assert_eq!(code, StatusCode::OK, "{attached}");
    assert_eq!(attached["parent_task_id"], f.task.to_string());
}

#[tokio::test]
async fn task_lists_sort_and_find_unassigned_tasks() {
    let f = Fixture::new().await;
    let (token, _) = f.token(&[ApiTokenScope::TasksWrite], false).await;
    let status = f.first_status(f.project).await;
    for (title, assignees) in [("B assigned", json!([f.user])), ("A open", json!([]))] {
        let (code, _) = f
            .call(
                Some(&token),
                "POST",
                "/tasks",
                Some(json!({"project_id": f.project, "status_id": status, "title": title, "assignee_ids": assignees})),
            )
            .await;
        assert_eq!(code, StatusCode::CREATED);
    }
    let titles = |page: &Value| {
        page["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|task| task["title"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>()
    };
    let (code, page) = f
        .call(Some(&token), "GET", "/tasks?sort=title&order=asc", None)
        .await;
    assert_eq!(code, StatusCode::OK, "{page}");
    assert_eq!(titles(&page), ["A open", "Approved task", "B assigned"]);
    let (_, page) = f
        .call(Some(&token), "GET", "/tasks?sort=title&order=desc", None)
        .await;
    assert_eq!(titles(&page), ["B assigned", "Approved task", "A open"]);
    let (_, page) = f
        .call(
            Some(&token),
            "GET",
            "/tasks?sort=title&order=asc&limit=1",
            None,
        )
        .await;
    let cursor = page["next_cursor"].as_str().unwrap().to_owned();
    let (_, next) = f
        .call(
            Some(&token),
            "GET",
            &format!("/tasks?sort=title&order=asc&limit=1&cursor={cursor}"),
            None,
        )
        .await;
    assert_eq!(titles(&next), ["Approved task"]);

    let (_, page) = f
        .call(
            Some(&token),
            "GET",
            "/tasks?assignee_id=none&sort=title&order=asc",
            None,
        )
        .await;
    assert_eq!(titles(&page), ["A open", "Approved task"]);
    let (_, page) = f
        .call(
            Some(&token),
            "GET",
            &format!("/tasks?assignee_id={}", f.user),
            None,
        )
        .await;
    assert_eq!(titles(&page), ["B assigned"]);
    for bad in ["sort=position", "order=up", "assignee_id=nobody"] {
        let (code, _) = f
            .call(Some(&token), "GET", &format!("/tasks?{bad}"), None)
            .await;
        assert_eq!(code, StatusCode::BAD_REQUEST, "{bad}");
    }
}

#[tokio::test]
async fn priority_sort_desc_lists_the_highest_priority_first() {
    let f = Fixture::new().await;
    let (token, _) = f.token(&[ApiTokenScope::TasksWrite], false).await;
    let status = f.first_status(f.project).await;
    for (title, priority) in [("Low", "low"), ("Urgent", "urgent"), ("High", "high")] {
        let (code, _) = f
            .call(
                Some(&token),
                "POST",
                "/tasks",
                Some(json!({"project_id": f.project, "status_id": status, "title": title, "priority": priority})),
            )
            .await;
        assert_eq!(code, StatusCode::CREATED);
    }
    let priorities = |page: &Value| {
        page["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|task| task["priority"].as_str().unwrap().to_owned())
            .filter(|priority| priority != "none")
            .collect::<Vec<_>>()
    };
    for (query, expected) in [
        ("sort=priority", ["urgent", "high", "low"]),
        ("sort=priority&order=desc", ["urgent", "high", "low"]),
        ("sort=priority&order=asc", ["low", "high", "urgent"]),
    ] {
        let (code, page) = f
            .call(Some(&token), "GET", &format!("/tasks?{query}"), None)
            .await;
        assert_eq!(code, StatusCode::OK, "{page}");
        assert_eq!(priorities(&page), expected, "{query}");
    }
    // The cursor keeps the public direction.
    let (_, page) = f
        .call(Some(&token), "GET", "/tasks?sort=priority&limit=1", None)
        .await;
    assert_eq!(priorities(&page), ["urgent"]);
    let cursor = page["next_cursor"].as_str().unwrap().to_owned();
    let (_, next) = f
        .call(
            Some(&token),
            "GET",
            &format!("/tasks?sort=priority&limit=1&cursor={cursor}"),
            None,
        )
        .await;
    assert_eq!(priorities(&next), ["high"]);
}

#[tokio::test]
async fn a_token_without_the_scope_gets_403_also_for_an_invalid_body() {
    let f = Fixture::new().await;
    let (read_only, _) = f.token(&[ApiTokenScope::TasksRead], false).await;
    for (method, path) in [
        ("POST", "/tasks".to_owned()),
        ("PATCH", format!("/tasks/{}", f.task)),
        ("POST", format!("/tasks/{}/comments", f.task)),
    ] {
        let (code, problem) = f
            .call(Some(&read_only), method, &path, Some(json!({})))
            .await;
        assert_eq!(code, StatusCode::FORBIDDEN, "{method} {path}: {problem}");
        assert_eq!(problem["code"], "insufficient_scope");
    }
    let (code, problem) = f
        .call(
            Some(&read_only),
            "DELETE",
            &format!("/tasks/{}?bogus=1", f.task),
            None,
        )
        .await;
    assert_eq!(code, StatusCode::FORBIDDEN, "{problem}");
}

async fn add_member(f: &Fixture, email: &str, role: &str) -> Id {
    let user = Id::new_v7();
    let mut tx = f.database.immediate_transaction().await.unwrap();
    sqlx::query("INSERT INTO users (id, email, normalized_email, display_name, password_hash, created_at, updated_at) VALUES ($1, $2, $2, $3, 'x', 1, 1)")
        .bind(user.to_string()).bind(email).bind(email).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO memberships (id, workspace_id, user_id, role, created_at, updated_at) VALUES ($1, $2, $3, $4, 1, 1)")
        .bind(Id::new_v7().to_string()).bind(f.workspace.to_string()).bind(user.to_string()).bind(role)
        .execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    user
}

/// A service-account token cannot get docs or chat scopes. One from before that rule (made here
/// by changing a task token's stored scopes) is still refused docs when it is used.
async fn old_service_account_token(f: &Fixture, scopes: &[ApiTokenScope]) -> String {
    let refused = f
        .tokens
        .create(
            f.workspace,
            f.user,
            format!("cli-{}", Id::new_v7()),
            vec![f.project],
            scopes,
            true,
            None,
            "test",
            TimestampMillis::now(),
        )
        .await;
    assert!(
        matches!(refused, Err(ApiTokenError::NotFound)),
        "{refused:?}"
    );
    let (token, id) = f.token(&[ApiTokenScope::TasksRead], true).await;
    sqlx::query("UPDATE api_tokens SET scopes = $1 WHERE id = $2")
        .bind(ApiTokenScope::format_list(scopes))
        .bind(id.to_string())
        .execute(f.database.pool())
        .await
        .unwrap();
    token
}

async fn token_by(
    f: &Fixture,
    creator: Id,
    service_account: bool,
) -> Result<(String, Id), ApiTokenError> {
    f.tokens
        .create(
            f.workspace,
            creator,
            format!("role-{}", Id::new_v7()),
            vec![f.project],
            &[ApiTokenScope::TasksWrite],
            service_account,
            None,
            "test",
            TimestampMillis::now(),
        )
        .await
        .map(|issued| (issued.token, issued.api_token.id))
}

async fn remove(f: &Fixture, user: Id) {
    sqlx::query("DELETE FROM memberships WHERE workspace_id = $1 AND user_id = $2")
        .bind(f.workspace.to_string())
        .bind(user.to_string())
        .execute(f.database.pool())
        .await
        .unwrap();
}

#[tokio::test]
async fn only_owners_and_admins_manage_tokens_and_tokens_follow_their_creator() {
    let f = Fixture::new().await;
    let admin = add_member(&f, "admin@example.com", "admin").await;
    let other_admin = add_member(&f, "admin2@example.com", "admin").await;
    let member = add_member(&f, "member@example.com", "member").await;
    let status = f.first_status(f.project).await;
    let body = json!({"project_id": f.project, "status_id": status, "title": "role check"});

    for user in [f.user, admin] {
        assert!(token_by(&f, user, false).await.is_ok());
        assert!(f.tokens.list(f.workspace, user).await.is_ok());
    }
    assert!(matches!(
        token_by(&f, member, false).await,
        Err(ApiTokenError::Forbidden)
    ));
    assert!(matches!(
        token_by(&f, member, true).await,
        Err(ApiTokenError::Forbidden)
    ));
    assert!(matches!(
        f.tokens.list(f.workspace, member).await,
        Err(ApiTokenError::Forbidden)
    ));

    let (admin_token, admin_token_id) = token_by(&f, admin, false).await.unwrap();
    assert!(matches!(
        f.tokens
            .revoke(
                f.workspace,
                admin_token_id,
                member,
                "test",
                TimestampMillis::now()
            )
            .await,
        Err(ApiTokenError::Forbidden)
    ));
    let (code, _) = f
        .call(Some(&admin_token), "POST", "/tasks", Some(body.clone()))
        .await;
    assert_eq!(code, StatusCode::CREATED);
    f.tokens
        .revoke(
            f.workspace,
            admin_token_id,
            other_admin,
            "test",
            TimestampMillis::now(),
        )
        .await
        .unwrap();
    let (code, _) = f.call(Some(&admin_token), "GET", "/me", None).await;
    assert_eq!(code, StatusCode::UNAUTHORIZED);

    let (personal, _) = token_by(&f, other_admin, false).await.unwrap();
    remove(&f, other_admin).await;
    for (method, path) in [("GET", "/me"), ("GET", "/tasks"), ("POST", "/tasks")] {
        let (code, _) = f
            .call(
                Some(&personal),
                method,
                path,
                (method == "POST").then(|| body.clone()),
            )
            .await;
        assert_eq!(code, StatusCode::UNAUTHORIZED, "{method} {path}");
    }
}

async fn notifications_for(f: &Fixture, user: Id) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM notifications WHERE recipient_user_id = $1")
        .bind(user.to_string())
        .fetch_one(f.database.pool())
        .await
        .unwrap()
}

#[tokio::test]
async fn service_account_tokens_do_not_depend_on_their_creator() {
    let f = Fixture::new().await;
    let admin = add_member(&f, "admin@example.com", "admin").await;
    let member = add_member(&f, "member@example.com", "member").await;
    let (bot, _) = token_by(&f, admin, true).await.unwrap();
    let (personal, _) = token_by(&f, admin, false).await.unwrap();
    remove(&f, admin).await;

    let (code, _) = f.call(Some(&personal), "GET", "/me", None).await;
    assert_eq!(
        code,
        StatusCode::UNAUTHORIZED,
        "a personal token stops with its creator"
    );
    let (code, me) = f.call(Some(&bot), "GET", "/me", None).await;
    assert_eq!(code, StatusCode::OK, "{me}");
    assert_eq!(me["user"], Value::Null);
    let account = me["service_account"].clone();
    let (code, _) = f.call(Some(&bot), "GET", "/tasks", None).await;
    assert_eq!(code, StatusCode::OK);
    let status = f.first_status(f.project).await;
    let (code, task) = f
        .call(
            Some(&bot),
            "POST",
            "/tasks",
            Some(json!({"project_id": f.project, "status_id": status, "title": "From CI", "assignee_ids": [member]})),
        )
        .await;
    assert_eq!(code, StatusCode::CREATED, "{task}");
    let id = task["id"].as_str().unwrap().to_owned();
    let creator: Option<String> =
        sqlx::query_scalar("SELECT creator_service_account_id FROM tasks WHERE id = $1")
            .bind(&id)
            .fetch_one(f.database.pool())
            .await
            .unwrap();
    assert_eq!(creator.as_deref(), account["id"].as_str());
    let (code, assigned) = f
        .call(
            Some(&bot),
            "PATCH",
            &format!("/tasks/{id}"),
            Some(json!({"expected_version": task["version"], "assignee_ids": [member, f.user]})),
        )
        .await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(
        notifications_for(&f, member).await,
        0,
        "no member assigned this task"
    );
    assert_eq!(
        notifications_for(&f, f.user).await,
        0,
        "no member assigned this task"
    );

    let (code, comment) = f
        .call(
            Some(&bot),
            "POST",
            &format!("/tasks/{id}/comments"),
            Some(json!({"body": "Built"})),
        )
        .await;
    assert_eq!(code, StatusCode::CREATED, "{comment}");
    assert_eq!(comment["author_service_account"], account);
    let (_, listed) = f
        .call(Some(&bot), "GET", &format!("/tasks/{id}/comments"), None)
        .await;
    assert_eq!(listed["items"][0]["author_service_account"], account);

    // In the web app nobody edits the bot's comment as its author; moderators may delete it.
    let comment_id: Id = comment["id"].as_str().unwrap().parse().unwrap();
    let task_id: Id = id.parse().unwrap();
    for (user, can_delete) in [(f.user, true), (member, false)] {
        let page = f
            .tasks
            .comments(f.workspace, task_id, user, None, 10)
            .await
            .unwrap();
        let record = page.items.iter().find(|c| c.id == comment_id).unwrap();
        assert!(!record.can_edit);
        assert_eq!(record.can_delete, can_delete);
        assert_eq!(
            record.author_service_account_name.as_deref(),
            account["name"].as_str()
        );
    }
    let (code, _) = f
        .call(
            Some(&bot),
            "DELETE",
            &format!("/tasks/{id}?expected_version={}", assigned["version"]),
            None,
        )
        .await;
    assert_eq!(code, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn personal_token_writes_still_notify() {
    let f = Fixture::new().await;
    let member = add_member(&f, "member@example.com", "member").await;
    let (token, _) = f.token(&[ApiTokenScope::TasksWrite], false).await;
    let status = f.first_status(f.project).await;
    let (code, _) = f
        .call(
            Some(&token),
            "POST",
            "/tasks",
            Some(json!({"project_id": f.project, "status_id": status, "title": "Assigned", "assignee_ids": [member]})),
        )
        .await;
    assert_eq!(code, StatusCode::CREATED);
    assert_eq!(notifications_for(&f, member).await, 1);
}

/// A real server (with co-editing sockets) and its users; `token` is the owner's token with
/// `scopes` for the workspace's first project.
async fn live_server(
    root: &tempfile::TempDir,
    scopes: &[ApiTokenScope],
) -> (
    collab_support::Server,
    collab_support::Users,
    String,
    String,
) {
    let server =
        collab_support::Server::start(root, orbit_server::collab::CollabConfig::default()).await;
    let users = collab_support::setup(&server).await;
    let project: String =
        sqlx::query_scalar("SELECT id FROM projects WHERE workspace_id = $1 ORDER BY created_at")
            .bind(&users.workspace)
            .fetch_one(server.database.pool())
            .await
            .unwrap();
    let token = ApiTokenRepository::new(server.database.clone())
        .create(
            users.workspace.parse().unwrap(),
            users.owner_id,
            "live".into(),
            vec![project.parse().unwrap()],
            scopes,
            false,
            None,
            "test",
            TimestampMillis::now(),
        )
        .await
        .unwrap()
        .token;
    (server, users, token, project)
}

/// Calls `/api/public/v1/workspaces/{workspace}{path}` of a real server with `token`.
async fn public_call(
    server: &collab_support::Server,
    users: &collab_support::Users,
    token: &str,
    method: &str,
    path: &str,
    body: Option<Value>,
) -> (u16, Value) {
    let mut request = server
        .http
        .request(
            method.parse().unwrap(),
            format!(
                "{}/api/public/v1/workspaces/{}{path}",
                server.base, users.workspace
            ),
        )
        .bearer_auth(token);
    if let Some(body) = body {
        request = request.json(&body);
    }
    let response = request.send().await.unwrap();
    let status = response.status().as_u16();
    let text = response.text().await.unwrap();
    let value = if text.is_empty() {
        Value::Null
    } else {
        serde_json::from_str(&text).unwrap()
    };
    (status, value)
}

#[tokio::test]
async fn doc_writes_conflict_with_live_edits_made_after_the_read() {
    let root = tempfile::TempDir::new().unwrap();
    let (server, users, token, _) = live_server(&root, &[ApiTokenScope::DocsWrite]).await;
    let (status, doc) = public_call(
        &server,
        &users,
        &token,
        "POST",
        "/docs",
        Some(json!({"title": "Plan", "markdown": "first"})),
    )
    .await;
    assert_eq!(status, 201, "{doc}");
    let id = doc["id"].as_str().unwrap().to_owned();
    let path = format!("/docs/{id}");
    let (_, read) = public_call(&server, &users, &token, "GET", &path, None).await;

    // A member types after the token's read; the edit is still in the server's memory.
    let mut member = collab_support::Client::open(&server, &users, &users.member, &id).await;
    let mut watcher = collab_support::Client::open(&server, &users, &users.owner, &id).await;
    member.type_text(5, " live").await;
    watcher.wait_text("first live").await;

    let (status, problem) = public_call(
        &server,
        &users,
        &token,
        "PATCH",
        &path,
        Some(json!({"expected_version": read["version"], "markdown": "from the token"})),
    )
    .await;
    assert_eq!(
        status, 409,
        "a stale content write must conflict: {problem}"
    );
    assert_eq!(problem["code"], "conflict");
    let (_, fresh) = public_call(&server, &users, &token, "GET", &path, None).await;
    assert_eq!(fresh["markdown"], "first live\n", "{fresh}");
    assert_eq!(problem["conflict"]["current_version"], fresh["version"]);
    // A title write with the stale version conflicts too: the doc changed after the read.
    let (status, _) = public_call(
        &server,
        &users,
        &token,
        "PATCH",
        &path,
        Some(json!({"expected_version": read["version"], "title": "Stale"})),
    )
    .await;
    assert_eq!(status, 409);

    // With the version of a read after the edit, the write goes through, also to open editors.
    let (status, updated) = public_call(
        &server,
        &users,
        &token,
        "PATCH",
        &path,
        Some(json!({"expected_version": fresh["version"], "markdown": "from the token"})),
    )
    .await;
    assert_eq!(status, 200, "{updated}");
    assert_eq!(updated["markdown"], "from the token\n");
    watcher.wait_text("from the token").await;
    let (_, after) = public_call(&server, &users, &token, "GET", &path, None).await;
    assert_eq!(after["version"], updated["version"], "{after}");
    server.stop().await;
}

#[tokio::test]
async fn task_description_writes_conflict_with_live_edits_made_after_the_read() {
    let root = tempfile::TempDir::new().unwrap();
    let (server, users, token, project) = live_server(&root, &[ApiTokenScope::TasksWrite]).await;
    let status_id: String = sqlx::query_scalar(
        "SELECT id FROM task_statuses WHERE project_id = $1 ORDER BY position LIMIT 1",
    )
    .bind(&project)
    .fetch_one(server.database.pool())
    .await
    .unwrap();
    let (status, task) = public_call(
        &server,
        &users,
        &token,
        "POST",
        "/tasks",
        Some(json!({"project_id": project, "status_id": status_id, "title": "Task", "description": "first"})),
    )
    .await;
    assert_eq!(status, 201, "{task}");
    let id = task["id"].as_str().unwrap().to_owned();
    let path = format!("/tasks/{id}");
    let (_, read) = public_call(&server, &users, &token, "GET", &path, None).await;

    let mut member =
        collab_support::Client::open_doc(&server, &users, &users.member, "tasks", &id).await;
    let mut watcher =
        collab_support::Client::open_doc(&server, &users, &users.owner, "tasks", &id).await;
    member.type_text(5, " live").await;
    watcher.wait_text("first live").await;

    let (status, problem) = public_call(
        &server,
        &users,
        &token,
        "PATCH",
        &path,
        Some(json!({"expected_version": read["version"], "description": "from the token"})),
    )
    .await;
    assert_eq!(
        status, 409,
        "a stale description write must conflict: {problem}"
    );
    let (_, fresh) = public_call(&server, &users, &token, "GET", &path, None).await;
    assert_eq!(fresh["description"], "first live\n", "{fresh}");
    assert_eq!(problem["conflict"]["current_version"], fresh["version"]);

    let (status, updated) = public_call(
        &server,
        &users,
        &token,
        "PATCH",
        &path,
        Some(json!({"expected_version": fresh["version"], "description": "from the token"})),
    )
    .await;
    assert_eq!(status, 200, "{updated}");
    assert_eq!(updated["description"], "from the token");
    let (_, after) = public_call(&server, &users, &token, "GET", &path, None).await;
    assert_eq!(after["version"], updated["version"], "{after}");
    watcher.wait_text("from the token").await;
    let (status, _) = public_call(
        &server,
        &users,
        &token,
        "DELETE",
        &format!("{path}?expected_version={}", read["version"]),
        None,
    )
    .await;
    assert_eq!(status, 409, "a stale delete conflicts");
    let (status, _) = public_call(
        &server,
        &users,
        &token,
        "DELETE",
        &format!("{path}?expected_version={}", updated["version"]),
        None,
    )
    .await;
    assert_eq!(status, 204);
    server.stop().await;
}

#[tokio::test]
async fn creating_a_doc_with_markdown_is_one_write() {
    let f = Fixture::new().await;
    let (writer, _) = f.token(&[ApiTokenScope::DocsWrite], false).await;
    let (code, doc) = f
        .call(
            Some(&writer),
            "POST",
            "/docs",
            Some(json!({"title": "Notes", "markdown": "Hello **there**"})),
        )
        .await;
    assert_eq!(code, StatusCode::CREATED, "{doc}");
    assert_eq!(doc["markdown"], "Hello **there**\n", "{doc}");
    let id = doc["id"].as_str().unwrap().to_owned();
    let actions: Vec<String> =
        sqlx::query_scalar("SELECT action FROM audit_events WHERE resource_id = $1")
            .bind(&id)
            .fetch_all(f.database.pool())
            .await
            .unwrap();
    assert_eq!(actions, ["page.created"]);
    let (_, read) = f
        .call(Some(&writer), "GET", &format!("/docs/{id}"), None)
        .await;
    assert_eq!(read["content"], doc["content"]);
    assert_eq!(read["version"], doc["version"], "{read}");
}

#[tokio::test]
async fn a_malformed_task_ref_is_a_bad_request_for_every_method() {
    let f = Fixture::new().await;
    let (token, _) = f
        .token(
            &[ApiTokenScope::TasksRead, ApiTokenScope::TasksWrite],
            false,
        )
        .await;
    for task in ["not_a_ref", "GEN-999"] {
        let expected = if task == "GEN-999" {
            StatusCode::NOT_FOUND
        } else {
            StatusCode::BAD_REQUEST
        };
        for (method, path, body) in [
            ("GET", format!("/tasks/{task}"), None),
            (
                "PATCH",
                format!("/tasks/{task}"),
                Some(json!({"expected_version": 0, "title": "x"})),
            ),
            ("DELETE", format!("/tasks/{task}?expected_version=0"), None),
            ("GET", format!("/tasks/{task}/comments"), None),
            (
                "POST",
                format!("/tasks/{task}/comments"),
                Some(json!({"body": "x"})),
            ),
        ] {
            let (code, problem) = f.call(Some(&token), method, &path, body).await;
            assert_eq!(code, expected, "{method} {path}: {problem}");
            if expected == StatusCode::BAD_REQUEST {
                assert_eq!(problem["code"], "invalid_request", "{method} {path}");
            }
        }
    }
}

#[tokio::test]
async fn doc_reads_report_database_errors_as_server_errors() {
    let f = Fixture::new().await;
    let (token, _) = f.token(&[ApiTokenScope::DocsRead], false).await;
    let id = Id::new_v7();
    let (code, _) = f
        .call(Some(&token), "GET", &format!("/docs/{id}"), None)
        .await;
    assert_eq!(code, StatusCode::NOT_FOUND);
    // A broken database is not a missing doc: clients must not drop their copy on a 404.
    sqlx::query("ALTER TABLE pages RENAME TO pages_unavailable")
        .execute(f.database.pool())
        .await
        .unwrap();
    for path in ["/docs".to_owned(), format!("/docs/{id}")] {
        let (code, problem) = f.call(Some(&token), "GET", &path, None).await;
        assert_eq!(code, StatusCode::INTERNAL_SERVER_ERROR, "{path}: {problem}");
    }
}

#[tokio::test]
async fn doc_markdown_names_visible_sub_pages_and_hides_the_others() {
    let f = Fixture::new().await;
    let (writer, _) = f.token(&[ApiTokenScope::DocsWrite], false).await;
    let (_, doc) = f
        .call(
            Some(&writer),
            "POST",
            "/docs",
            Some(json!({"title": "Plan"})),
        )
        .await;
    let id: Id = doc["id"].as_str().unwrap().parse().unwrap();
    let (code, child) = f
        .call(
            Some(&writer),
            "POST",
            "/docs",
            Some(json!({"parent_id": id, "title": "Launch [notes]"})),
        )
        .await;
    assert_eq!(code, StatusCode::CREATED, "{child}");
    let member = add_member(&f, "member@example.com", "member").await;
    let secret = f
        .pages
        .create_page(
            f.workspace,
            member,
            CreatePage {
                kind: orbit_server::repositories::pages::PageKind::Page,
                parent_id: None,
                space: SpaceRequest {
                    private: Some(true),
                    teamspace_id: None,
                },
                title: "Secret plans".into(),
                icon: None,
                position: None,
            },
            "test",
            TimestampMillis::now(),
        )
        .await
        .unwrap();
    let page = f.pages.get_page(f.workspace, id, f.user).await.unwrap();
    let block = |page_id: String| json!({"id": page_id.clone(), "type": "page", "props": {"pageId": page_id}});
    f.pages
        .update_page(
            f.workspace,
            id,
            f.user,
            page.version,
            PageChanges {
                content: Some(vec![
                    block(child["id"].as_str().unwrap().to_owned()),
                    block(secret.id.to_string()),
                ]),
                ..PageChanges::default()
            },
            "test",
            TimestampMillis::now(),
        )
        .await
        .unwrap();
    let (code, read) = f
        .call(Some(&writer), "GET", &format!("/docs/{id}"), None)
        .await;
    assert_eq!(code, StatusCode::OK, "{read}");
    assert_eq!(
        read["markdown"],
        format!(
            "[Launch \\[notes\\]](/docs/{})\n\n[Untitled](/docs/{})\n",
            child["id"].as_str().unwrap(),
            secret.id
        ),
        "{read}"
    );
    assert!(!read.to_string().contains("Secret plans"), "{read}");
}

/// A stored attachment of `task`, as an upload leaves it; returns the blob's storage key.
async fn attach(f: &Fixture, task: Id) -> String {
    let blob = Id::new_v7();
    let key = format!("blobs/{blob}");
    sqlx::query("INSERT INTO attachment_blobs (id, workspace_id, sha256, byte_size, storage_key, created_at, quarantine_until) VALUES ($1, $2, $3, 3, $4, 1, 1)")
        .bind(blob.to_string()).bind(f.workspace.to_string()).bind(blob.to_string()).bind(&key)
        .execute(f.database.pool()).await.unwrap();
    sqlx::query("INSERT INTO attachment_references (id, workspace_id, task_id, comment_id, owner_id, blob_id, display_name, media_type, byte_size, created_at) VALUES ($1, $2, $3, NULL, $4, $5, 'a.txt', 'text/plain', 3, 1)")
        .bind(Id::new_v7().to_string()).bind(f.workspace.to_string()).bind(task.to_string())
        .bind(f.user.to_string()).bind(blob.to_string())
        .execute(f.database.pool()).await.unwrap();
    key
}

async fn trashed(f: &Fixture) -> Vec<String> {
    f.tasks
        .task_trash(f.workspace, f.user, None, 100, TimestampMillis::now())
        .await
        .unwrap()
        .items
        .into_iter()
        .map(|task| task.id.to_string())
        .collect()
}

async fn rows(f: &Fixture, table: &str, column: &str, ids: &[Id]) -> i64 {
    let mut total = 0;
    for id in ids {
        total += count(
            f,
            &format!("SELECT COUNT(*) FROM {table} WHERE {column} = $1"),
            &id.to_string(),
        )
        .await;
    }
    total
}

#[tokio::test]
async fn permanent_delete_removes_a_trashed_or_live_task_like_the_trash_purge() {
    let f = Fixture::new().await;
    let (token, _) = f.token(&[ApiTokenScope::TasksWrite], false).await;
    let new = |title: &'static str, parent: Option<Id>| {
        create_task(
            &f.tasks,
            &f.database,
            f.workspace,
            f.user,
            f.project,
            title,
            parent,
        )
    };
    let version = |task: Id| {
        let f = &f;
        let token = token.clone();
        async move {
            let (code, read) = f
                .call(Some(&token), "GET", &format!("/tasks/{task}"), None)
                .await;
            assert_eq!(code, StatusCode::OK, "{read}");
            read["version"].clone()
        }
    };
    let parent = new("Parent", None).await;
    let child = new("Child", Some(parent)).await;
    let key = attach(&f, parent).await;
    let (code, _) = f
        .call(
            Some(&token),
            "POST",
            &format!("/tasks/{child}/comments"),
            Some(json!({"body": "A comment"})),
        )
        .await;
    assert_eq!(code, StatusCode::CREATED);

    // The default DELETE only moves the tree to the trash.
    let (code, _) = f
        .call(
            Some(&token),
            "DELETE",
            &format!("/tasks/{parent}?expected_version={}", version(parent).await),
            None,
        )
        .await;
    assert_eq!(code, StatusCode::NO_CONTENT);
    let trash = trashed(&f).await;
    assert!(trash.contains(&parent.to_string()) && trash.contains(&child.to_string()));
    assert_eq!(
        rows(&f, "attachment_references", "task_id", &[parent]).await,
        1
    );
    assert_eq!(rows(&f, "task_comments", "task_id", &[child]).await, 1);

    // A task in the trash cannot change and its version cannot be read, so it is not checked.
    let (code, problem) = f
        .call(
            Some(&token),
            "DELETE",
            &format!("/tasks/{parent}?expected_version=1&permanent=true"),
            None,
        )
        .await;
    assert_eq!(code, StatusCode::NO_CONTENT, "{problem}");
    let trash = trashed(&f).await;
    assert!(!trash.contains(&parent.to_string()) && !trash.contains(&child.to_string()));
    assert_eq!(rows(&f, "tasks", "id", &[parent, child]).await, 0);
    assert_eq!(
        rows(&f, "attachment_references", "task_id", &[parent]).await,
        0
    );
    assert_eq!(rows(&f, "task_comments", "task_id", &[child]).await, 0);
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) FROM attachment_blobs WHERE storage_key = $1",
            &key
        )
        .await,
        0
    );
    // The stored file goes with the next maintenance run, as for the 30-day purge.
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) FROM attachment_file_deletions WHERE path = $1",
            &key
        )
        .await,
        1
    );
    let (_, metadata) = audit_of(&f, &parent.to_string(), "task.purged").await;
    assert_eq!(metadata["tasks"], 2, "{metadata}");
    for task in [parent, child] {
        let (code, _) = f
            .call(Some(&token), "GET", &format!("/tasks/{task}"), None)
            .await;
        assert_eq!(code, StatusCode::NOT_FOUND);
    }
    let (code, _) = f
        .call(
            Some(&token),
            "DELETE",
            &format!("/tasks/{parent}?expected_version=1&permanent=true"),
            None,
        )
        .await;
    assert_eq!(code, StatusCode::NOT_FOUND);

    // A live task needs its current version, and goes with its live sub-issues.
    let live = new("Live", None).await;
    let sub = new("Live child", Some(live)).await;
    let (code, problem) = f
        .call(
            Some(&token),
            "DELETE",
            &format!("/tasks/{live}?expected_version=1&permanent=true"),
            None,
        )
        .await;
    assert_eq!(code, StatusCode::CONFLICT, "{problem}");
    assert_eq!(rows(&f, "tasks", "id", &[live, sub]).await, 2);
    let (code, problem) = f
        .call(
            Some(&token),
            "DELETE",
            &format!(
                "/tasks/{live}?expected_version={}&permanent=true",
                version(live).await
            ),
            None,
        )
        .await;
    assert_eq!(code, StatusCode::NO_CONTENT, "{problem}");
    assert_eq!(rows(&f, "tasks", "id", &[live, sub]).await, 0);
    assert!(!trashed(&f).await.contains(&live.to_string()));
}

#[tokio::test]
async fn permanent_delete_needs_the_scope_the_grant_and_an_owner_or_admin() {
    let f = Fixture::new().await;
    let version = f
        .tasks
        .get_task(f.workspace, f.task, f.user)
        .await
        .unwrap()
        .version;
    let (reader, _) = f.token(&[ApiTokenScope::TasksRead], false).await;
    let (code, problem) = f
        .call(
            Some(&reader),
            "DELETE",
            &format!(
                "/tasks/{}?expected_version={version}&permanent=true",
                f.task
            ),
            None,
        )
        .await;
    assert_eq!(code, StatusCode::FORBIDDEN, "{problem}");
    assert_eq!(problem["code"], "insufficient_scope");

    // Outside the grant (the token only has `project`).
    let (writer, _) = f.token(&[ApiTokenScope::TasksWrite], false).await;
    let other = f
        .tasks
        .get_task(f.workspace, f.other_task, f.user)
        .await
        .unwrap()
        .version;
    let (code, _) = f
        .call(
            Some(&writer),
            "DELETE",
            &format!(
                "/tasks/{}?expected_version={other}&permanent=true",
                f.other_task
            ),
            None,
        )
        .await;
    assert_eq!(code, StatusCode::NOT_FOUND);

    // A member may move tasks to the trash but not delete them forever, like teamspace pages.
    // Members cannot make tokens, so this one is from an admin who is now a member.
    let member = add_member(&f, "member@example.com", "admin").await;
    let (member_token, _) = token_by(&f, member, false).await.unwrap();
    sqlx::query("UPDATE memberships SET role = 'member' WHERE user_id = $1")
        .bind(member.to_string())
        .execute(f.database.pool())
        .await
        .unwrap();
    let path = |version: Value| {
        format!(
            "/tasks/{}?expected_version={version}&permanent=true",
            f.task
        )
    };
    let (_, read) = f
        .call(
            Some(&member_token),
            "GET",
            &format!("/tasks/{}", f.task),
            None,
        )
        .await;
    let (code, problem) = f
        .call(
            Some(&member_token),
            "DELETE",
            &path(read["version"].clone()),
            None,
        )
        .await;
    assert_eq!(code, StatusCode::FORBIDDEN, "{problem}");
    assert_eq!(problem["code"], "workspace_action_forbidden");
    assert_eq!(rows(&f, "tasks", "id", &[f.task]).await, 1);
    assert!(
        !trashed(&f).await.contains(&f.task.to_string()),
        "refused, yet trashed"
    );

    let admin = add_member(&f, "admin@example.com", "admin").await;
    let (admin_token, _) = token_by(&f, admin, false).await.unwrap();
    let (code, problem) = f
        .call(
            Some(&admin_token),
            "DELETE",
            &path(read["version"].clone()),
            None,
        )
        .await;
    assert_eq!(code, StatusCode::NO_CONTENT, "{problem}");
    assert_eq!(rows(&f, "tasks", "id", &[f.task]).await, 0);

    // A service account acts as the workspace owner.
    let (service, _) = f.token(&[ApiTokenScope::TasksWrite], true).await;
    let task = create_task(
        &f.tasks,
        &f.database,
        f.workspace,
        f.user,
        f.project,
        "SA",
        None,
    )
    .await;
    let (_, read) = f
        .call(Some(&service), "GET", &format!("/tasks/{task}"), None)
        .await;
    let (code, problem) = f
        .call(
            Some(&service),
            "DELETE",
            &format!(
                "/tasks/{task}?expected_version={}&permanent=true",
                read["version"]
            ),
            None,
        )
        .await;
    assert_eq!(code, StatusCode::NO_CONTENT, "{problem}");
    let (actor, metadata) = audit_of(&f, &task.to_string(), "task.purged").await;
    assert_eq!(actor, None);
    assert!(
        metadata["actor_service_account_id"].is_string(),
        "{metadata}"
    );
}
