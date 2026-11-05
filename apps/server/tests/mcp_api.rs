use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use orbit_platform::{
    HttpPlatformLayer, Id, OriginPolicy, PasswordService, TestDatabase, TimestampMillis,
};
use orbit_server::mcp::{McpState, router};
use orbit_server::repositories::api_tokens::{ApiTokenRepository, ApiTokenScope};
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
                &[ApiTokenScope::TasksRead],
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
    sqlx::query("UPDATE tasks SET parent_task_id = $1 WHERE id = $2")
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
            "SELECT projects.project_key, tasks.number FROM tasks JOIN projects ON projects.id = tasks.project_id WHERE tasks.id = $1",
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
            &[ApiTokenScope::TasksWrite],
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
    assert_eq!(response.status(), StatusCode::OK);
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
    sqlx::query("UPDATE api_tokens SET expires_at = 1 WHERE id = $1")
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
    sqlx::query("UPDATE api_tokens SET expires_at = NULL WHERE id = $1")
        .bind(fixture.token_id.to_string())
        .execute(fixture.database.pool())
        .await
        .unwrap();
    // Transfer ownership before removing the original owner's membership.
    let new_owner = Id::new_v7();
    let membership = Id::new_v7();
    sqlx::query("INSERT INTO users (id, email, normalized_email, display_name, password_hash, created_at, updated_at) SELECT $1, 'new-owner@example.com', 'new-owner@example.com', 'New owner', password_hash, created_at, updated_at FROM users WHERE id = $2")
        .bind(new_owner.to_string()).bind(fixture.user.to_string())
        .execute(fixture.database.pool()).await.unwrap();
    sqlx::query("INSERT INTO memberships (id, workspace_id, user_id, role, created_at, updated_at) VALUES ($1, $2, $3, 'admin', 1, 1)")
        .bind(membership.to_string()).bind(fixture.workspace.to_string()).bind(new_owner.to_string())
        .execute(fixture.database.pool()).await.unwrap();
    sqlx::query("UPDATE workspaces SET owner_membership_id = $1 WHERE id = $2")
        .bind(membership.to_string())
        .bind(fixture.workspace.to_string())
        .execute(fixture.database.pool())
        .await
        .unwrap();
    sqlx::query("DELETE FROM memberships WHERE workspace_id = $1 AND user_id = $2")
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
            &[ApiTokenScope::TasksRead],
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
        sqlx::query_scalar("SELECT id FROM projects WHERE workspace_id = $1")
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
    sqlx::query("UPDATE tasks SET deleted_at = 1 WHERE id = $1")
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
    sqlx::query("UPDATE projects SET deleted_at = 1 WHERE id = $1")
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
        app_key: Some(orbit_platform::AppKey::new([1; 32])),
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
        assert_eq!(result["result"]["tools"].as_array().unwrap().len(), 4);
    }
}

#[tokio::test]
async fn task_writes_require_scope_and_versions_and_stay_in_granted_projects() {
    let f = Fixture::new().await;
    let current = f.tasks.get_task(f.workspace, f.task, f.user).await.unwrap();
    let denied = f
        .tool(
            "update_task",
            json!({"task_id": f.task, "expected_version": current.version, "title": "Changed"}),
        )
        .await;
    assert!(denied.get("error").is_some(), "{denied}");
    sqlx::query("UPDATE api_tokens SET scopes = 'tasks:read tasks:write' WHERE id = $1")
        .bind(f.token_id.to_string())
        .execute(f.database.pool())
        .await
        .unwrap();
    let updated = f
        .tool(
            "update_task",
            json!({"task_id": f.task, "expected_version": current.version, "title": "Changed"}),
        )
        .await;
    assert_eq!(
        updated["result"]["structuredContent"]["title"], "Changed",
        "{updated}"
    );
    let stale = f
        .tool(
            "delete_task",
            json!({"task_id": f.task, "expected_version": current.version}),
        )
        .await;
    assert_eq!(stale["result"]["isError"], true, "{stale}");
    let secret = f
        .tasks
        .get_task(f.workspace, f.other_task, f.user)
        .await
        .unwrap();
    let denied = f
        .tool(
            "delete_task",
            json!({"task_id": f.other_task, "expected_version": secret.version}),
        )
        .await;
    assert_eq!(denied["result"]["isError"], true, "{denied}");
    let statuses = f
        .tool("list_task_statuses", json!({"project_id": f.project}))
        .await;
    assert!(
        !statuses["result"]["structuredContent"]["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let status = current.status_id;
    let created = f
        .tool(
            "create_task",
            json!({"project_id": f.project, "status_id": status, "title": "Created by MCP"}),
        )
        .await;
    assert_eq!(
        created["result"]["structuredContent"]["title"], "Created by MCP",
        "{created}"
    );
    let id = &created["result"]["structuredContent"]["id"];
    let version = &created["result"]["structuredContent"]["version"];
    let deleted = f
        .tool(
            "delete_task",
            json!({"task_id": id, "expected_version": version}),
        )
        .await;
    assert_eq!(
        deleted["result"]["structuredContent"]["deleted"], true,
        "{deleted}"
    );
    // An approved parent must not trash an unapproved child.
    sqlx::query("UPDATE tasks SET parent_task_id = $1 WHERE id = $2")
        .bind(f.task.to_string())
        .bind(f.other_task.to_string())
        .execute(f.database.pool())
        .await
        .unwrap();
    let current = f.tasks.get_task(f.workspace, f.task, f.user).await.unwrap();
    let denied = f
        .tool(
            "delete_task",
            json!({"task_id": f.task, "expected_version": current.version}),
        )
        .await;
    assert_eq!(denied["result"]["isError"], true, "{denied}");
    assert!(
        f.tasks
            .get_task(f.workspace, f.other_task, f.user)
            .await
            .is_ok()
    );
    let denied = f.tool("update_task", json!({"task_id": f.task, "expected_version": current.version, "title": "Forbidden tree write"})).await;
    assert_eq!(denied["result"]["isError"], true, "{denied}");
}

#[tokio::test]
async fn docs_and_chat_require_explicit_scopes_and_doc_writes_need_docs_write() {
    use orbit_platform::{
        AttachmentMutationCoordinator, LocalBlobStore, UploadLimits, UploadService,
    };
    use orbit_server::repositories::{
        chat::{ChannelCreate, ChatRepository, ConversationKind, SendInput},
        pages::{CreatePage, PageRepository, SpaceRequest},
    };
    let mut f = Fixture::new().await;
    let root = tempfile::tempdir().unwrap();
    let uploads = UploadService::new(
        (*f.database).clone(),
        Arc::new(LocalBlobStore::new(root.path().join("files"))),
        AttachmentMutationCoordinator::default(),
        UploadLimits::default(),
    );
    let pages = Arc::new(PageRepository::new((*f.database).clone()));
    let chat = Arc::new(ChatRepository::new((*f.database).clone(), uploads));
    let page = pages
        .create_page(
            f.workspace,
            f.user,
            CreatePage {
                parent_id: None,
                space: SpaceRequest::default(),
                title: "MCP document".into(),
                icon: None,
                position: None,
            },
            "test",
            TimestampMillis::now(),
        )
        .await
        .unwrap();
    let channel = chat
        .create_channel(
            f.workspace,
            f.user,
            ChannelCreate {
                name: "mcp-channel".into(),
                topic: "".into(),
                category_id: None,
                kind: ConversationKind::Public,
                member_ids: vec![],
            },
        )
        .await
        .unwrap();
    let outsider = Id::new_v7();
    let mut tx = f.database.immediate_transaction().await.unwrap();
    sqlx::query("INSERT INTO users (id, email, normalized_email, display_name, password_hash, created_at, updated_at) VALUES ($1, 'outsider@example.com', 'outsider@example.com', 'Outsider', 'x', 1, 1)").bind(outsider.to_string()).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO memberships (id, workspace_id, user_id, role, created_at, updated_at) VALUES ($1, $2, $3, 'member', 1, 1)").bind(Id::new_v7().to_string()).bind(f.workspace.to_string()).bind(outsider.to_string()).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let secret_page = pages
        .create_page(
            f.workspace,
            outsider,
            CreatePage {
                parent_id: None,
                space: SpaceRequest {
                    private: Some(true),
                    teamspace_id: None,
                },
                title: "Hidden private document".into(),
                icon: None,
                position: None,
            },
            "test",
            TimestampMillis::now(),
        )
        .await
        .unwrap();
    let secret_channel = chat
        .create_channel(
            f.workspace,
            outsider,
            ChannelCreate {
                name: "hidden-private-channel".into(),
                topic: "".into(),
                category_id: None,
                kind: ConversationKind::Private,
                member_ids: vec![],
            },
        )
        .await
        .unwrap();
    let send = |actor, conversation_id, body: &str| {
        let chat = chat.clone();
        let input = SendInput {
            conversation_id,
            thread_root_id: None,
            reply_to_id: None,
            body: body.into(),
            sticker_id: None,
            file_ids: vec![],
            online: vec![],
            also_in_channel: false,
            nonce: Id::new_v7().to_string(),
        };
        async move { chat.send_message(f.workspace, actor, input).await.unwrap() }
    };
    send(f.user, channel.value.id, "deploy pineapple tonight").await;
    send(outsider, secret_channel.value.id, "secret pineapple plan").await;
    f.app = router(
        McpState::new(Arc::new(f.tokens.clone()), Arc::new(f.tasks.clone()))
            .with_content(pages, chat),
        "https://orbit.test",
    );
    for name in ["list_docs", "list_conversations", "search_messages"] {
        let r = f.tool(name, json!({})).await;
        assert!(r.get("error").is_some(), "{r}");
    }
    sqlx::query("UPDATE api_tokens SET scopes = 'tasks:read docs:read chat:read' WHERE id = $1")
        .bind(f.token_id.to_string())
        .execute(f.database.pool())
        .await
        .unwrap();
    let docs = f.tool("list_docs", json!({})).await;
    assert!(docs.to_string().contains("MCP document"), "{docs}");
    let doc = f.tool("get_doc", json!({"page_id": page.id})).await;
    assert_eq!(
        doc["result"]["structuredContent"]["title"], "MCP document",
        "{doc}"
    );
    let messages = f
        .tool(
            "list_messages",
            json!({"conversation_id": channel.value.id, "limit": 1}),
        )
        .await;
    assert!(
        messages["result"]["structuredContent"]["items"].is_array(),
        "{messages}"
    );
    let invalid = f
        .tool(
            "list_messages",
            json!({"conversation_id": channel.value.id, "limit": 101}),
        )
        .await;
    assert!(invalid.get("error").is_some(), "{invalid}");
    let chats = f.tool("list_conversations", json!({})).await;
    assert!(chats.to_string().contains("mcp-channel"), "{chats}");
    assert!(!docs.to_string().contains("Hidden private document"));
    assert!(!chats.to_string().contains("hidden-private-channel"));
    let denied = f.tool("get_doc", json!({"page_id": secret_page.id})).await;
    assert_eq!(denied["result"]["isError"], true, "{denied}");
    let denied = f
        .tool(
            "list_messages",
            json!({"conversation_id": secret_channel.value.id}),
        )
        .await;
    assert_eq!(denied["result"]["isError"], true, "{denied}");
    let found = f.tool("search_messages", json!({"query": "pineapp"})).await;
    let items = &found["result"]["structuredContent"]["items"];
    assert_eq!(items.as_array().map(Vec::len), Some(1), "{found}");
    assert_eq!(
        items[0]["message"]["body"], "deploy pineapple tonight",
        "{found}"
    );
    assert_eq!(
        items[0]["conversation_id"],
        json!(channel.value.id),
        "{found}"
    );
    let denied = f
        .tool(
            "search_messages",
            json!({"query": "pineapple", "conversation_id": secret_channel.value.id}),
        )
        .await;
    assert_eq!(denied["result"]["isError"], true, "{denied}");
    let invalid = f
        .tool("search_messages", json!({"query": "x".repeat(501)}))
        .await;
    assert!(invalid.get("error").is_some(), "{invalid}");
    let listing = f.rpc("tools/list", json!({})).await;
    assert!(listing.to_string().contains("search_messages"), "{listing}");
    assert!(!listing.to_string().contains("send_message"));
    assert!(!listing.to_string().contains("create_doc"));
    let denied = f.tool("get_doc", json!({"page_id": Id::new_v7()})).await;
    assert_eq!(denied["result"]["isError"], true, "{denied}");
    let refused = f.tool("create_doc", json!({"title": "x"})).await;
    assert!(refused.get("error").is_some(), "{refused}");

    sqlx::query("UPDATE api_tokens SET scopes = 'tasks:read docs:write' WHERE id = $1")
        .bind(f.token_id.to_string())
        .execute(f.database.pool())
        .await
        .unwrap();
    let listing = f.rpc("tools/list", json!({})).await.to_string();
    for name in ["create_doc", "update_doc", "get_doc"] {
        assert!(listing.contains(name), "{name}: {listing}");
    }
    let created = f
        .tool(
            "create_doc",
            json!({"title": "Agent notes", "markdown": "## Findings\n\nAll good."}),
        )
        .await;
    let doc = &created["result"]["structuredContent"];
    assert_eq!(doc["title"], "Agent notes", "{created}");
    assert_eq!(doc["content"][0]["type"], "heading", "{created}");
    let updated = f
        .tool(
            "update_doc",
            json!({"page_id": doc["id"], "expected_version": doc["version"], "markdown": "Rewritten."}),
        )
        .await;
    let text = &updated["result"]["structuredContent"]["content"][0]["content"][0]["text"];
    assert_eq!(text, "Rewritten.", "{updated}");
    let read = f.tool("get_doc", json!({"page_id": doc["id"]})).await;
    let markdown = &read["result"]["structuredContent"]["markdown"];
    assert_eq!(markdown, "Rewritten.\n", "{read}");
    let stale = f
        .tool(
            "update_doc",
            json!({"page_id": doc["id"], "expected_version": doc["version"], "title": "x"}),
        )
        .await;
    assert_eq!(stale["result"]["isError"], true, "{stale}");
    assert!(stale.to_string().contains("version changed"), "{stale}");
}

#[tokio::test]
async fn writes_are_refused_in_a_read_only_cloud_workspace() {
    let f = Fixture::new().await;
    sqlx::query("UPDATE api_tokens SET scopes = 'tasks:read tasks:write docs:write' WHERE id = $1")
        .bind(f.token_id.to_string())
        .execute(f.database.pool())
        .await
        .unwrap();
    // Not the exempt root workspace and past its trial, so without a subscription it is read-only.
    sqlx::query("UPDATE workspaces SET created_at = 0, billing_exempt = 0 WHERE id = $1")
        .bind(f.workspace.to_string())
        .execute(f.database.pool())
        .await
        .unwrap();
    let _billing = orbit_server::billing::Billing::new(
        orbit_platform::BillingConfig::new(
            orbit_platform::Secret::new("sk_test_unused"),
            orbit_platform::Secret::new("whsec_unused"),
            vec![],
        ),
        (*f.database).clone(),
        "https://orbit.test",
    );
    let current = f.tasks.get_task(f.workspace, f.task, f.user).await.unwrap();
    let calls = [
        (
            "create_task",
            json!({"project_id": f.project, "status_id": current.status_id, "title": "Created"}),
        ),
        (
            "update_task",
            json!({"task_id": f.task, "expected_version": current.version, "title": "Changed"}),
        ),
        (
            "delete_task",
            json!({"task_id": f.task, "expected_version": current.version}),
        ),
        ("create_doc", json!({"title": "Created"})),
    ];
    for (name, arguments) in calls {
        let refused = f.tool(name, arguments).await;
        assert_eq!(refused["result"]["isError"], true, "{name}: {refused}");
        assert!(
            refused.to_string().contains("read-only"),
            "{name}: {refused}"
        );
    }
    let after = f.tasks.get_task(f.workspace, f.task, f.user).await.unwrap();
    assert_eq!(after.version, current.version);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM tasks WHERE workspace_id = $1")
        .bind(f.workspace.to_string())
        .fetch_one(f.database.pool())
        .await
        .unwrap();
    assert_eq!(count, 2);
    // Reads stay open.
    let read = f.tool("get_task", json!({"task_id": f.task})).await;
    assert_eq!(
        read["result"]["structuredContent"]["title"],
        "Approved task"
    );
}
