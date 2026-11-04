//! The public REST API (`/api/public/v1`) for API tokens. SQLite by default, Postgres when
//! `ORBIT_TEST_POSTGRES_URL` is set.
use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use orbit_platform::{
    HttpPlatformLayer, Id, OriginPolicy, PasswordService, TestDatabase, TimestampMillis,
};
use orbit_server::public_api::{PublicApiState, router};
use orbit_server::repositories::api_tokens::ApiTokenRepository;
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::repositories::pages::{CreatePage, PageRepository, SpaceRequest};
use orbit_server::repositories::tasks::{CreateTask, TaskRepository};
use serde_json::{Value, json};
use tower::ServiceExt;

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
            PublicApiState::new(Arc::new(tokens.clone()), Arc::new(tasks.clone()))
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

    /// A token for the approved project with these scopes (`read`, `write`, `docs:read`, …).
    async fn token(&self, scopes: &[&str], service_account: bool) -> (String, Id) {
        let extra = scopes
            .iter()
            .filter(|scope| scope.contains(':'))
            .copied()
            .collect::<Vec<_>>()
            .join(" ");
        let issued = self
            .tokens
            .create_scoped(
                self.workspace,
                self.user,
                format!("cli-{}", Id::new_v7()),
                vec![self.project],
                scopes.contains(&"read"),
                scopes.contains(&"write"),
                service_account,
                None,
                "test",
                TimestampMillis::now(),
                &extra,
            )
            .await
            .unwrap();
        (issued.token, issued.api_token.id)
    }

    async fn call(
        &self,
        token: Option<&str>,
        method: &str,
        path: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
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
    let (token, token_id) = f.token(&["read"], false).await;

    let (status, me) = f.call(Some(&token), "GET", "/me", None).await;
    assert_eq!(status, StatusCode::OK, "{me}");
    assert_eq!(me["token"]["id"], token_id.to_string());
    assert_eq!(me["token"]["scopes"], json!(["read"]));
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

    let (status, labels) = f
        .call(
            Some(&token),
            "GET",
            &format!("/projects/{}/labels", f.project),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{labels}");
    assert_eq!(
        labels,
        json!({"items": [{"id": label.id, "project_id": f.project, "name": "bug", "color": "#00ff00", "version": 0}], "next_cursor": null})
    );

    let (status, members) = f.call(Some(&token), "GET", "/members", None).await;
    assert_eq!(status, StatusCode::OK, "{members}");
    assert_eq!(
        members,
        json!({"items": [{"user_id": f.user, "display_name": "Owner", "email": "owner@example.com", "role": "owner"}], "next_cursor": null})
    );

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
            "label_ids",
            "number",
            "parent_task_id",
            "priority",
            "project_id",
            "status_id",
            "title",
            "updated_at",
            "version"
        ]
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

    f.tasks
        .create_comment(
            f.workspace,
            f.task,
            f.user,
            None,
            orbit_server::repositories::tasks::CommentContent::Markdown {
                body: "First comment".into(),
                mentioned_user_ids: vec![],
            },
            "test",
            TimestampMillis::now(),
        )
        .await
        .unwrap();
    let (status, comments) = f
        .call(
            Some(&token),
            "GET",
            &format!("/tasks/{key}-{number}/comments"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{comments}");
    let comment = &comments["items"][0];
    assert_eq!(comment["body"], "First comment");
    assert_eq!(comment["task_id"], f.task.to_string());
    assert_eq!(comment["author_id"], f.user.to_string());
    assert_eq!(comment["parent_id"], Value::Null);
    assert!(comment.get("body_json").is_none() && comment.get("can_edit").is_none());
}

#[tokio::test]
async fn resources_of_unapproved_projects_are_not_found() {
    let f = Fixture::new().await;
    let (token, _) = f.token(&["read", "write"], false).await;
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
        ("GET", format!("/projects/{other}/labels"), None),
        ("GET", "/projects/not-an-id/labels".to_owned(), None),
        ("GET", format!("/tasks/{task}/comments"), None),
        (
            "POST",
            format!("/tasks/{task}/comments"),
            Some(json!({"body": "Leak?"})),
        ),
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
    let comments: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM task_comments WHERE task_id = $1")
        .bind(task.to_string())
        .fetch_one(f.database.pool())
        .await
        .unwrap();
    assert_eq!(comments, 0);
}

#[tokio::test]
async fn task_writes_follow_scope_version_and_workspace_rules() {
    let f = Fixture::new().await;
    let status = f.first_status(f.project).await;
    let current = f.tasks.get_task(f.workspace, f.task, f.user).await.unwrap();
    let (read_only, _) = f.token(&["read"], false).await;
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
        (
            "POST",
            format!("/tasks/{}/comments", f.task),
            Some(json!({"body": "x"})),
        ),
    ] {
        let (code, problem) = f.call(Some(&read_only), method, &path, body).await;
        assert_eq!(code, StatusCode::FORBIDDEN, "{method} {path}: {problem}");
        assert_eq!(problem["code"], "insufficient_scope");
    }

    let (write_only, _) = f.token(&["write"], false).await;
    let (code, _) = f.call(Some(&write_only), "GET", "/tasks", None).await;
    assert_eq!(code, StatusCode::FORBIDDEN);

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
    let (token, _) = f.token(&["read", "write"], false).await;
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
    assert_eq!(created["version"], 0);
    let id = created["id"].as_str().unwrap().to_owned();
    let actor: String = sqlx::query_scalar(
        "SELECT actor_id FROM audit_events WHERE resource_id = $1 AND action = 'task.created'",
    )
    .bind(&id)
    .fetch_one(f.database.pool())
    .await
    .unwrap();
    assert_eq!(actor, f.user.to_string());

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
            Some(json!({"expected_version": 0, "title": "Renamed", "assignee_ids": [], "label_ids": []})),
        )
        .await;
    assert_eq!(code, StatusCode::OK, "{updated}");
    assert_eq!(updated["title"], "Renamed");
    assert_eq!(updated["assignee_ids"], json!([]));
    assert_eq!(updated["version"], 1);
    let (code, stale) = f
        .call(
            Some(&token),
            "PATCH",
            &format!("/tasks/{id}"),
            Some(json!({"expected_version": 0, "title": "Again"})),
        )
        .await;
    assert_eq!(code, StatusCode::CONFLICT, "{stale}");
    assert_eq!(stale["conflict"]["current_version"], 1);
    assert!(stale["conflict"].get("current").is_none(), "{stale}");
    let (code, _) = f
        .call(
            Some(&token),
            "PATCH",
            &format!("/tasks/{id}"),
            Some(json!({"expected_version": 1, "label_ids": [other_label.id]})),
        )
        .await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);

    let (code, comment) = f
        .call(
            Some(&token),
            "POST",
            &format!("/tasks/{id}/comments"),
            Some(json!({"body": "Done"})),
        )
        .await;
    assert_eq!(code, StatusCode::CREATED, "{comment}");
    assert_eq!(comment["body"], "Done");
    assert_eq!(comment["task_id"], id);
    let (code, _) = f
        .call(
            Some(&token),
            "POST",
            &format!("/tasks/{id}/comments"),
            Some(json!({"body": "  "})),
        )
        .await;
    assert_eq!(code, StatusCode::BAD_REQUEST);

    let (code, _) = f
        .call(
            Some(&token),
            "DELETE",
            &format!("/tasks/{id}?expected_version=0"),
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
            &format!("/tasks/{id}?expected_version=1"),
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
    let (token, _) = f.token(&["read", "write"], false).await;
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
async fn docs_need_their_scope_and_a_personal_token_and_chat_is_not_served() {
    let f = Fixture::new().await;
    let page = f
        .pages
        .create_page(
            f.workspace,
            f.user,
            CreatePage {
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
    let (plain, _) = f.token(&["read"], false).await;
    for path in ["/docs".to_owned(), format!("/docs/{}", page.id)] {
        let (code, problem) = f.call(Some(&plain), "GET", &path, None).await;
        assert_eq!(code, StatusCode::FORBIDDEN, "{path}");
        assert_eq!(problem["code"], "insufficient_scope");
    }

    let (token, _) = f.token(&["read", "docs:read", "chat:read"], false).await;
    let (code, me) = f.call(Some(&token), "GET", "/me", None).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(
        me["token"]["scopes"],
        json!(["read", "docs:read", "chat:read"])
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
                    .uri(format!("/api/public/v1{path}"))
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
    }

    let (service, _) = f.token(&["read", "docs:read", "chat:read"], true).await;
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
async fn bad_expired_and_revoked_tokens_are_unauthorized() {
    let f = Fixture::new().await;
    let (expired, expired_id) = f.token(&["read"], false).await;
    sqlx::query("UPDATE api_tokens SET expires_at = $1 WHERE id = $2")
        .bind(TimestampMillis::now().as_millis() - 1)
        .bind(expired_id.to_string())
        .execute(f.database.pool())
        .await
        .unwrap();
    let (revoked, revoked_id) = f.token(&["read"], false).await;
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
                .uri("/api/public/v1/tasks")
                .header(header::AUTHORIZATION, "Bearer orb_unknown")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(response.headers().contains_key(header::WWW_AUTHENTICATE));
}
