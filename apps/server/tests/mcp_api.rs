use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use orbit_platform::{
    HttpPlatformLayer, Id, OriginPolicy, PasswordService, TestDatabase, TimestampMillis,
};
use orbit_server::mcp::{McpState, router};
use orbit_server::repositories::api_tokens::ApiTokenRepository;
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::repositories::tasks::{CreateTask, TaskRepository};
use serde_json::{Value, json};
use tower::ServiceExt;

struct Fixture {
    database: TestDatabase,
    app: axum::Router,
    tokens: ApiTokenRepository,
    tasks: TaskRepository,
    user: Id,
    workspace: Id,
    project: Id,
    other_project: Id,
    token: String,
    token_id: Id,
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
        let task = create_task(
            &tasks,
            &database,
            setup.workspace_id,
            setup.user_id,
            setup.project_id,
            "Approved task",
        )
        .await;
        let other_task = create_task(
            &tasks,
            &database,
            setup.workspace_id,
            setup.user_id,
            other.id,
            "Secret task",
        )
        .await;
        let tokens = ApiTokenRepository::new((*database).clone());
        let issued = tokens
            .create(
                setup.workspace_id,
                setup.user_id,
                "MCP".into(),
                vec![setup.project_id],
                true,
                false,
                false,
                None,
                "test",
                now,
            )
            .await
            .unwrap();
        let app = router(
            McpState::new(Arc::new(tokens.clone()), Arc::new(tasks.clone())),
            "https://orbit.test",
        )
        .layer(HttpPlatformLayer::new(OriginPolicy::new(
            "https://orbit.test",
        )));
        Self {
            database,
            app,
            tokens,
            tasks,
            user: setup.user_id,
            workspace: setup.workspace_id,
            project: setup.project_id,
            other_project: other.id,
            token: issued.token,
            token_id: issued.api_token.id,
            task,
            other_task,
        }
    }

    async fn rpc(&self, method: &str, params: Value) -> Value {
        rpc(&self.app, &self.token, method, params).await
    }

    async fn tool(&self, name: &str, arguments: Value) -> Value {
        self.rpc("tools/call", json!({"name": name, "arguments": arguments}))
            .await
    }
}

async fn create_task(
    tasks: &TaskRepository,
    database: &TestDatabase,
    workspace: Id,
    user: Id,
    project: Id,
    title: &str,
) -> Id {
    let status: String = sqlx::query_scalar(
        "SELECT id FROM task_statuses WHERE project_id = ? ORDER BY position LIMIT 1",
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
                source_url: None,
                priority: "none".into(),
                position: None,
                assignee_ids: vec![],
                label_ids: vec![],
                due_start_at: None,
                due_at: None,
                parent_task_id: None,
            },
            "test",
            TimestampMillis::now(),
        )
        .await
        .unwrap()
        .task
        .id
}

fn request(token: Option<&str>, method: &str, params: Value) -> Request<Body> {
    let mut builder = Request::builder()
        .method("POST")
        .uri("/mcp")
        .header(header::HOST, "orbit.test")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT, "application/json, text/event-stream")
        .header("mcp-protocol-version", "2025-11-25");
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    builder
        .body(Body::from(
            json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).to_string(),
        ))
        .unwrap()
}

async fn rpc(app: &axum::Router, token: &str, method: &str, params: Value) -> Value {
    let response = app
        .clone()
        .oneshot(request(Some(token), method, params))
        .await
        .unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    serde_json::from_slice(&body).unwrap()
}

#[tokio::test]
async fn task_reads_are_filtered_before_pagination_and_hide_related_projects() {
    let fixture = Fixture::new().await;
    sqlx::query("UPDATE tasks SET parent_task_id = ? WHERE id = ?")
        .bind(fixture.other_task.to_string())
        .bind(fixture.task.to_string())
        .execute(fixture.database.pool())
        .await
        .unwrap();
    let projects = fixture.tool("list_projects", json!({})).await;
    assert_eq!(
        projects["result"]["structuredContent"]["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        projects["result"]["structuredContent"]["items"][0]["id"],
        fixture.project.to_string()
    );
    let list = fixture.tool("list_tasks", json!({"limit": 1})).await;
    let items = list["result"]["structuredContent"]["items"]
        .as_array()
        .unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["id"], fixture.task.to_string());
    assert!(list["result"]["structuredContent"]["next_cursor"].is_null());
    let task = fixture
        .tool("get_task", json!({"task_id": fixture.task}))
        .await;
    assert_eq!(
        task["result"]["structuredContent"]["title"],
        "Approved task"
    );
    assert!(!task.to_string().contains("Secret task"));
    assert!(!task.to_string().contains(&fixture.other_task.to_string()));
    let denied = fixture
        .tool("get_task", json!({"task_id": fixture.other_task}))
        .await;
    assert_eq!(denied["result"]["isError"], true);
    // An identifier (`KEY-N`) names a task too; one outside the grant stays hidden.
    for (project, task) in [
        (fixture.project, fixture.task),
        (fixture.other_project, fixture.other_task),
    ] {
        let (key, number): (String, i64) = sqlx::query_as(
            "SELECT projects.project_key, tasks.number FROM tasks JOIN projects ON projects.id = tasks.project_id WHERE tasks.id = ?",
        )
        .bind(task.to_string())
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
        let read = fixture
            .tool(
                "get_task",
                json!({"task_id": format!("{}-{number}", key.to_lowercase())}),
            )
            .await;
        if project == fixture.project {
            assert_eq!(read["result"]["structuredContent"]["id"], task.to_string());
            assert_eq!(read["result"]["structuredContent"]["number"], number);
        } else {
            assert_eq!(read["result"]["isError"], true);
        }
    }
    let denied = fixture
        .tool("list_tasks", json!({"project_id": fixture.other_project}))
        .await;
    assert_eq!(denied["result"]["isError"], true);
    let secret_search = fixture.tool("list_tasks", json!({"query": "Secret"})).await;
    assert!(
        secret_search["result"]["structuredContent"]["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn cursors_continue_only_within_the_approved_query() {
    let fixture = Fixture::new().await;
    let second = create_task(
        &fixture.tasks,
        &fixture.database,
        fixture.workspace,
        fixture.user,
        fixture.project,
        "Second task",
    )
    .await;
    let first = fixture.tool("list_tasks", json!({"limit": 1})).await;
    let cursor = first["result"]["structuredContent"]["next_cursor"]
        .as_str()
        .unwrap();
    let next = fixture
        .tool("list_tasks", json!({"limit": 1, "cursor": cursor}))
        .await;
    assert_eq!(
        next["result"]["structuredContent"]["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_ne!(
        first["result"]["structuredContent"]["items"][0]["id"],
        next["result"]["structuredContent"]["items"][0]["id"]
    );
    let all = fixture.tool("list_tasks", json!({})).await;
    let ids: Vec<_> = all["result"]["structuredContent"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&fixture.task.to_string().as_str()));
    assert!(ids.contains(&second.to_string().as_str()));
    let changed = fixture
        .tool("list_tasks", json!({"query": "Second", "cursor": cursor}))
        .await;
    assert_eq!(changed["result"]["isError"], true);
    let found = fixture.tool("list_tasks", json!({"query": "Second"})).await;
    assert_eq!(
        found["result"]["structuredContent"]["items"][0]["id"],
        second.to_string()
    );
}

#[tokio::test]
async fn bearer_credentials_are_required_and_checked_on_every_request() {
    let fixture = Fixture::new().await;
    for token in [None, Some("invalid")] {
        let response = fixture
            .app
            .clone()
            .oneshot(request(token, "tools/list", json!({})))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert!(response.headers().contains_key(header::WWW_AUTHENTICATE));
    }
    let issued = fixture
        .tokens
        .create(
            fixture.workspace,
            fixture.user,
            "Write only".into(),
            vec![fixture.project],
            false,
            true,
            false,
            None,
            "test",
            TimestampMillis::now(),
        )
        .await
        .unwrap();
    let response = fixture
        .app
        .clone()
        .oneshot(request(Some(&issued.token), "tools/list", json!({})))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let listing = fixture.rpc("tools/list", json!({})).await;
    assert!(listing.get("error").is_none(), "{listing}");
    fixture
        .tokens
        .revoke(
            fixture.workspace,
            fixture.token_id,
            fixture.user,
            "test",
            TimestampMillis::now(),
        )
        .await
        .unwrap();
    let response = fixture
        .app
        .clone()
        .oneshot(request(Some(&fixture.token), "tools/list", json!({})))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn removed_membership_and_expired_tokens_stop_access() {
    let fixture = Fixture::new().await;
    sqlx::query("UPDATE api_tokens SET expires_at = 1 WHERE id = ?")
        .bind(fixture.token_id.to_string())
        .execute(fixture.database.pool())
        .await
        .unwrap();
    let response = fixture
        .app
        .clone()
        .oneshot(request(Some(&fixture.token), "tools/list", json!({})))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    sqlx::query("UPDATE api_tokens SET expires_at = NULL WHERE id = ?")
        .bind(fixture.token_id.to_string())
        .execute(fixture.database.pool())
        .await
        .unwrap();
    // Transfer ownership before removing the original owner's membership.
    let new_owner = Id::new_v7();
    let membership = Id::new_v7();
    sqlx::query("INSERT INTO users (id, email, normalized_email, display_name, password_hash, created_at, updated_at) SELECT ?, 'new-owner@example.com', 'new-owner@example.com', 'New owner', password_hash, created_at, updated_at FROM users WHERE id = ?")
        .bind(new_owner.to_string()).bind(fixture.user.to_string())
        .execute(fixture.database.pool()).await.unwrap();
    sqlx::query("INSERT INTO memberships (id, workspace_id, user_id, role, created_at, updated_at) VALUES (?, ?, ?, 'admin', 1, 1)")
        .bind(membership.to_string()).bind(fixture.workspace.to_string()).bind(new_owner.to_string())
        .execute(fixture.database.pool()).await.unwrap();
    sqlx::query("UPDATE workspaces SET owner_membership_id = ? WHERE id = ?")
        .bind(membership.to_string())
        .bind(fixture.workspace.to_string())
        .execute(fixture.database.pool())
        .await
        .unwrap();
    sqlx::query("DELETE FROM memberships WHERE workspace_id = ? AND user_id = ?")
        .bind(fixture.workspace.to_string())
        .bind(fixture.user.to_string())
        .execute(fixture.database.pool())
        .await
        .unwrap();
    let response = fixture
        .app
        .clone()
        .oneshot(request(Some(&fixture.token), "tools/list", json!({})))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn identity_is_request_local_and_not_reused_between_connections() {
    let fixture = Fixture::new().await;
    let other = fixture
        .tokens
        .create(
            fixture.workspace,
            fixture.user,
            "Other".into(),
            vec![fixture.other_project],
            true,
            false,
            false,
            None,
            "test",
            TimestampMillis::now(),
        )
        .await
        .unwrap();
    let (approved, private) = tokio::join!(
        rpc(
            &fixture.app,
            &fixture.token,
            "tools/call",
            json!({"name": "list_tasks", "arguments": {}})
        ),
        rpc(
            &fixture.app,
            &other.token,
            "tools/call",
            json!({"name": "list_tasks", "arguments": {}})
        ),
    );
    assert_eq!(
        approved["result"]["structuredContent"]["items"][0]["id"],
        fixture.task.to_string()
    );
    assert_eq!(
        private["result"]["structuredContent"]["items"][0]["id"],
        fixture.other_task.to_string()
    );
}

#[tokio::test]
async fn unknown_write_tools_and_invalid_inputs_do_not_change_tasks() {
    let fixture = Fixture::new().await;
    let before: i64 = sqlx::query_scalar("SELECT count(*) FROM tasks")
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    let result = fixture
        .tool("create_task", json!({"title": "Not permitted"}))
        .await;
    assert!(result.get("error").is_some());
    for args in [
        json!({"limit": 0}),
        json!({"limit": 101}),
        json!({"workspace_id": fixture.workspace}),
        json!({"project_id": "bad"}),
    ] {
        let result = fixture.tool("list_tasks", args).await;
        assert!(result.get("error").is_some(), "{result}");
    }
    let after: i64 = sqlx::query_scalar("SELECT count(*) FROM tasks")
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    assert_eq!(before, after);
}

#[tokio::test]
async fn origin_and_host_checks_do_not_block_non_browser_clients() {
    let fixture = Fixture::new().await;
    let init = fixture.rpc("initialize", json!({"protocolVersion": "2025-11-25", "capabilities": {}, "clientInfo": {"name": "test", "version": "1"}})).await;
    assert_eq!(init["result"]["serverInfo"]["name"], "orbit");
    for (name, value) in [
        (header::ORIGIN, "https://evil.test"),
        (header::HOST, "evil.test"),
    ] {
        let mut req = request(Some(&fixture.token), "tools/list", json!({}));
        req.headers_mut().insert(name, value.parse().unwrap());
        let response = fixture.app.clone().oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
    let mut req = request(Some(&fixture.token), "tools/list", json!({}));
    req.headers_mut()
        .append(header::AUTHORIZATION, "Bearer different".parse().unwrap());
    let response = fixture.app.clone().oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn cross_workspace_and_deleted_resources_are_not_returned() {
    let fixture = Fixture::new().await;
    let workspaces = orbit_server::repositories::workspaces::WorkspaceRepository::new(
        (*fixture.database).clone(),
    );
    let other_workspace = workspaces
        .create(
            fixture.user,
            "Another workspace".into(),
            "test",
            TimestampMillis::now(),
        )
        .await
        .unwrap();
    let other_project: String =
        sqlx::query_scalar("SELECT id FROM projects WHERE workspace_id = ?")
            .bind(other_workspace.id.to_string())
            .fetch_one(fixture.database.pool())
            .await
            .unwrap();
    let foreign = create_task(
        &fixture.tasks,
        &fixture.database,
        other_workspace.id,
        fixture.user,
        other_project.parse().unwrap(),
        "Foreign task",
    )
    .await;
    let result = fixture.tool("get_task", json!({"task_id": foreign})).await;
    assert_eq!(result["result"]["isError"], true);
    let result = fixture
        .tool("list_tasks", json!({"query": "Foreign"}))
        .await;
    assert!(
        result["result"]["structuredContent"]["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    sqlx::query("UPDATE tasks SET deleted_at = 1 WHERE id = ?")
        .bind(fixture.task.to_string())
        .execute(fixture.database.pool())
        .await
        .unwrap();
    let result = fixture
        .tool("get_task", json!({"task_id": fixture.task}))
        .await;
    assert_eq!(result["result"]["isError"], true);
    let result = fixture.tool("list_tasks", json!({})).await;
    assert!(
        result["result"]["structuredContent"]["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    sqlx::query("UPDATE projects SET deleted_at = 1 WHERE id = ?")
        .bind(fixture.project.to_string())
        .execute(fixture.database.pool())
        .await
        .unwrap();
    let response = fixture
        .app
        .clone()
        .oneshot(request(Some(&fixture.token), "tools/list", json!({})))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn mcp_route_is_mounted_in_the_application_and_rejects_oversized_bodies() {
    let root = tempfile::TempDir::new().unwrap();
    let mut config = orbit_platform::Config {
        environment: orbit_platform::EnvironmentMode::Development,
        ..Default::default()
    };
    config.data.database = root.path().join("orbit.sqlite");
    config.data.attachments = root.path().join("attachments");
    config.data.backups = root.path().join("backups");
    let app = orbit_server::app::App::build(config).await.unwrap();
    let response = app
        .router()
        .oneshot(request(None, "tools/list", json!({})))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(response.headers().contains_key(header::WWW_AUTHENTICATE));

    let fixture = Fixture::new().await;
    let response = fixture
        .app
        .clone()
        .oneshot(request(
            Some(&fixture.token),
            "tools/call",
            json!({"name": "list_tasks", "arguments": {"query": "x".repeat(70 * 1024)}}),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn tools_list_includes_required_cache_fields_for_current_protocol() {
    let fixture = Fixture::new().await;
    for version in ["2026-07-28", "2025-11-25"] {
        let mut req = request(
            Some(&fixture.token),
            "tools/list",
            json!({"_meta": {"io.modelcontextprotocol/protocolVersion": version, "io.modelcontextprotocol/clientCapabilities": {}}}),
        );
        req.headers_mut()
            .insert("mcp-protocol-version", version.parse().unwrap());
        req.headers_mut()
            .insert("mcp-method", "tools/list".parse().unwrap());
        let response = fixture.app.clone().oneshot(req).await.unwrap();
        let status = response.status();
        let body = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
        assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
        let result: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(result["result"]["ttlMs"], 0, "{result}");
        assert_eq!(result["result"]["cacheScope"], "private", "{result}");
        assert_eq!(result["result"]["tools"].as_array().unwrap().len(), 3);
    }
}
