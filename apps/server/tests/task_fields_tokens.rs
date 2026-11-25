//! Custom task fields (#87) through API tokens: the public REST API and the MCP tools. SQLite by
//! default, Postgres when `ORBIT_TEST_POSTGRES_URL` is set.
use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use orbit_platform::{
    HttpPlatformLayer, Id, OriginPolicy, PasswordService, TestDatabase, TimestampMillis,
};
use orbit_server::mcp::McpState;
use orbit_server::public_api::PublicApiState;
use orbit_server::repositories::api_tokens::{ApiTokenRepository, ApiTokenScope, TokenProjects};
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::repositories::tasks::{
    CreateTask, TaskFieldKind, TaskFieldOptionInput, TaskRepository,
};
use serde_json::{Value, json};
use tower::ServiceExt;

const READ: &[ApiTokenScope] = &[ApiTokenScope::TasksRead];
const WRITE: &[ApiTokenScope] = &[ApiTokenScope::TasksRead, ApiTokenScope::TasksWrite];

struct Fixture {
    database: TestDatabase,
    rest: axum::Router,
    mcp: axum::Router,
    tokens: ApiTokenRepository,
    tasks: TaskRepository,
    user: Id,
    workspace: Id,
    project: Id,
    other_project: Id,
    task: Id,
    other_task: Id,
    /// A number field that both projects show.
    points: Id,
    /// A select field that both projects show, with its `High` option.
    stage: Id,
    high: String,
    /// A text field that only the other project shows.
    secret: Id,
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
        let (workspace, user, project) = (setup.workspace_id, setup.user_id, setup.project_id);
        let tasks = TaskRepository::new((*database).clone());
        let other = tasks
            .create_project(
                workspace,
                user,
                "Private".into(),
                "PRV".into(),
                "#ff0000".into(),
                false,
                "test",
                now,
            )
            .await
            .unwrap();
        let task = create_task(&tasks, &database, workspace, user, project).await;
        let other_task = create_task(&tasks, &database, workspace, user, other.id).await;
        let field = |name: &str, kind, options: Vec<TaskFieldOptionInput>| {
            let tasks = tasks.clone();
            let name = name.to_owned();
            async move {
                tasks
                    .create_task_field(workspace, user, name, kind, options, "test", now)
                    .await
                    .unwrap()
            }
        };
        let points = field("Points", TaskFieldKind::Number, vec![]).await;
        let option = |name: &str| TaskFieldOptionInput {
            id: None,
            name: name.into(),
            color: "#00ff00".into(),
        };
        let stage = field(
            "Stage",
            TaskFieldKind::Select,
            vec![option("Low"), option("High")],
        )
        .await;
        let secret = field("Customer", TaskFieldKind::Text, vec![]).await;
        tasks
            .set_project_task_fields(
                workspace,
                project,
                user,
                &[points.id, stage.id],
                "test",
                now,
            )
            .await
            .unwrap();
        tasks
            .set_project_task_fields(
                workspace,
                other.id,
                user,
                &[points.id, stage.id, secret.id],
                "test",
                now,
            )
            .await
            .unwrap();
        let tokens = ApiTokenRepository::new((*database).clone());
        let platform = || HttpPlatformLayer::new(OriginPolicy::new("https://orbit.test"));
        let rest = orbit_server::public_api::router(PublicApiState::new(
            Arc::new(tokens.clone()),
            Arc::new(tasks.clone()),
            "https://orbit.test",
        ))
        .layer(platform());
        let mcp = orbit_server::mcp::router(
            McpState::new(Arc::new(tokens.clone()), Arc::new(tasks.clone())),
            "https://orbit.test",
        )
        .layer(platform());
        Self {
            database,
            rest,
            mcp,
            tokens,
            tasks,
            user,
            workspace,
            project,
            other_project: other.id,
            task,
            other_task,
            points: points.id,
            stage: stage.id,
            high: stage.options[1].id.clone(),
            secret: secret.id,
        }
    }

    /// A token for the fixture's first project only, or for all projects.
    async fn token(&self, scopes: &[ApiTokenScope], all: bool, service_account: bool) -> String {
        let projects = if all {
            TokenProjects::All
        } else {
            TokenProjects::Only(vec![self.project])
        };
        self.tokens
            .create(
                self.workspace,
                self.user,
                format!("cli-{}", Id::new_v7()),
                projects,
                scopes,
                service_account,
                None,
                "test",
                TimestampMillis::now(),
            )
            .await
            .unwrap()
            .token
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
        let response = self.rest.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        let value = if body.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&body).unwrap()
        };
        (status, value)
    }

    async fn rpc(&self, token: &str, method: &str, params: Value) -> Value {
        let request = Request::builder()
            .method("POST")
            .uri("/mcp")
            .header(header::HOST, "orbit.test")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ACCEPT, "application/json, text/event-stream")
            .header("mcp-protocol-version", "2025-11-25")
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .body(Body::from(
                json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).to_string(),
            ))
            .unwrap();
        let response = self.mcp.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
        serde_json::from_slice(&body).unwrap()
    }

    async fn tool(&self, token: &str, name: &str, arguments: Value) -> Value {
        self.rpc(
            token,
            "tools/call",
            json!({"name": name, "arguments": arguments}),
        )
        .await
    }

    /// The task's identifier, such as `GEN-1`.
    async fn identifier(&self) -> String {
        let task = self
            .tasks
            .get_task(self.workspace, self.task, self.user)
            .await
            .unwrap();
        let key: String = sqlx::query_scalar("SELECT project_key FROM projects WHERE id = $1")
            .bind(self.project.to_string())
            .fetch_one(self.database.pool())
            .await
            .unwrap();
        format!("{key}-{}", task.number)
    }

    async fn stored_value(&self, task: Id, field: Id) -> Option<String> {
        sqlx::query_scalar(
            "SELECT value_json FROM task_field_values WHERE task_id = $1 AND field_id = $2",
        )
        .bind(task.to_string())
        .bind(field.to_string())
        .fetch_optional(self.database.pool())
        .await
        .unwrap()
    }
}

async fn create_task(
    tasks: &TaskRepository,
    database: &TestDatabase,
    workspace: Id,
    user: Id,
    project: Id,
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
                title: "Task".into(),
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
        .id
}

fn ids(list: &Value) -> Vec<String> {
    list["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["id"].as_str().unwrap().to_owned())
        .collect()
}

#[tokio::test]
async fn a_project_limited_token_reads_the_gallery_but_only_its_projects_and_values() {
    let f = Fixture::new().await;
    let token = f.token(READ, false, false).await;

    let (status, gallery) = f.call(&token, "GET", "/task-fields", None).await;
    assert_eq!(status, StatusCode::OK, "{gallery}");
    // The whole gallery, by name; counts cover the approved project only.
    assert_eq!(
        ids(&gallery),
        [
            f.secret.to_string(),
            f.points.to_string(),
            f.stage.to_string()
        ]
    );
    let counts: Vec<i64> = gallery["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|field| field["project_count"].as_i64().unwrap())
        .collect();
    assert_eq!(counts, [0, 1, 1]);
    assert_eq!(gallery["next_cursor"], Value::Null);

    let (status, fields) = f
        .call(
            &token,
            "GET",
            &format!("/projects/{}/fields", f.project),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{fields}");
    assert_eq!(ids(&fields), [f.points.to_string(), f.stage.to_string()]);
    let other_fields = format!("/projects/{}/fields", f.other_project);
    let (status, _) = f.call(&token, "GET", &other_fields, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    f.tasks
        .set_task_field_value(
            f.workspace,
            f.other_task,
            f.points,
            f.user,
            json!(5),
            "test",
            TimestampMillis::now(),
        )
        .await
        .unwrap();
    let (status, values) = f
        .call(&token, "GET", &format!("/tasks/{}/fields", f.task), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{values}");
    assert_eq!(values["items"], json!([]));
    let (status, _) = f
        .call(
            &token,
            "GET",
            &format!("/tasks/{}/fields", f.other_task),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn token_writes_need_tasks_write_and_stay_in_the_grant() {
    let f = Fixture::new().await;
    let reader = f.token(READ, false, false).await;
    let writer = f.token(WRITE, false, false).await;
    let value_path = |task: Id, field: Id| format!("/tasks/{task}/fields/{field}");

    // A read-only token writes nothing.
    let (status, problem) = f
        .call(
            &reader,
            "PUT",
            &value_path(f.task, f.points),
            Some(json!({"value": 3})),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{problem}");
    assert_eq!(problem["code"], "insufficient_scope");
    let (status, _) = f
        .call(
            &reader,
            "POST",
            "/task-fields",
            Some(json!({"name": "Size", "kind": "number"})),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(f.stored_value(f.task, f.points).await, None);

    // Set, by identifier, then clear with null.
    let (status, values) = f
        .call(
            &writer,
            "PUT",
            &format!("/tasks/{}/fields/{}", f.identifier().await, f.points),
            Some(json!({"value": 3.5})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{values}");
    assert_eq!(
        values["items"],
        json!([{"field_id": f.points, "value": 3.5}])
    );
    let (status, values) = f
        .call(
            &writer,
            "PUT",
            &value_path(f.task, f.points),
            Some(json!({"value": null})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{values}");
    assert_eq!(values["items"], json!([]));

    // Another project's task, a field the project does not show, a value of the wrong kind.
    let (status, _) = f
        .call(
            &writer,
            "PUT",
            &value_path(f.other_task, f.points),
            Some(json!({"value": 1})),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(f.stored_value(f.other_task, f.points).await, None);
    let (status, problem) = f
        .call(
            &writer,
            "PUT",
            &value_path(f.task, f.secret),
            Some(json!({"value": "Acme"})),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{problem}");
    let (status, problem) = f
        .call(
            &writer,
            "PUT",
            &value_path(f.task, f.points),
            Some(json!({"value": "three"})),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{problem}");
    assert!(problem["detail"].as_str().unwrap().contains("YYYY-MM-DD"));

    // The project's fields: not another project's, and only gallery fields.
    let other_fields = format!("/projects/{}/fields", f.other_project);
    let (status, _) = f
        .call(
            &writer,
            "PUT",
            &other_fields,
            Some(json!({"field_ids": []})),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let fields = format!("/projects/{}/fields", f.project);
    let (status, list) = f
        .call(
            &writer,
            "PUT",
            &fields,
            Some(json!({"field_ids": [f.stage, f.points]})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert_eq!(ids(&list), [f.stage.to_string(), f.points.to_string()]);
    let (status, _) = f
        .call(
            &writer,
            "PUT",
            &fields,
            Some(json!({"field_ids": [Id::new_v7()]})),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // A limited token adds fields, but changes or deletes none: that reaches other projects.
    let (status, created) = f
        .call(
            &writer,
            "POST",
            "/task-fields",
            Some(json!({"name": "Size", "kind": "multi_select", "options": [{"name": "S", "color": "#112233"}]})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["options"][0]["name"], "S");
    assert_eq!(created["project_count"], 0);
    let (status, _) = f
        .call(
            &writer,
            "POST",
            "/task-fields",
            Some(json!({"name": "size", "kind": "text"})),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let field_path = format!("/task-fields/{}", f.points);
    let (status, problem) = f
        .call(
            &writer,
            "PATCH",
            &field_path,
            Some(json!({"expected_version": 0, "name": "Story points"})),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{problem}");
    let (status, _) = f
        .call(
            &writer,
            "DELETE",
            &format!("{field_path}?expected_version=0"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // A token for all projects may.
    let all = f.token(WRITE, true, false).await;
    let (status, renamed) = f
        .call(
            &all,
            "PATCH",
            &field_path,
            Some(json!({"expected_version": 0, "name": "Story points"})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{renamed}");
    assert_eq!(renamed["name"], "Story points");
    assert_eq!(renamed["project_count"], 2);
    let (status, _) = f
        .call(
            &all,
            "DELETE",
            &format!("{field_path}?expected_version=0"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, _) = f
        .call(
            &all,
            "DELETE",
            &format!("{field_path}?expected_version=1"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, gallery) = f.call(&all, "GET", "/task-fields", None).await;
    assert!(!ids(&gallery).contains(&f.points.to_string()));
}

#[tokio::test]
async fn a_service_account_s_value_change_is_audited_as_the_service_account() {
    let f = Fixture::new().await;
    let token = f.token(WRITE, false, true).await;
    let (status, values) = f
        .call(
            &token,
            "PUT",
            &format!("/tasks/{}/fields/{}", f.task, f.stage),
            Some(json!({"value": f.high})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{values}");
    let (actor, metadata): (Option<String>, String) = sqlx::query_as(
        "SELECT actor_id, metadata_json FROM audit_events WHERE resource_id = $1 AND action = 'task.field_changed'",
    )
    .bind(f.task.to_string())
    .fetch_one(f.database.pool())
    .await
    .unwrap();
    assert_eq!(actor, None);
    let metadata: Value = serde_json::from_str(&metadata).unwrap();
    assert_eq!(metadata["field_name"], "Stage");
    assert!(
        metadata["actor_service_account_name"].is_string(),
        "{metadata}"
    );
}

#[tokio::test]
async fn mcp_tools_list_read_and_set_values() {
    let f = Fixture::new().await;
    let reader = f.token(READ, false, false).await;
    let writer = f.token(WRITE, false, false).await;

    let names = |listing: &Value| -> Vec<String> {
        listing["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap().to_owned())
            .collect()
    };
    let read_tools = names(&f.rpc(&reader, "tools/list", json!({})).await);
    assert!(read_tools.contains(&"list_task_fields".to_owned()));
    assert!(read_tools.contains(&"get_task_field_values".to_owned()));
    assert!(!read_tools.contains(&"set_task_field_value".to_owned()));
    let listing = f.rpc(&writer, "tools/list", json!({})).await;
    let write_tools = names(&listing);
    for tool in [
        "set_task_field_value",
        "create_task_field",
        "set_project_task_fields",
    ] {
        assert!(write_tools.contains(&tool.to_owned()), "{tool}");
    }
    // Changing or deleting a gallery field is not a tool.
    assert!(!write_tools.iter().any(|name| name == "delete_task_field"));
    let set_tool = listing["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "set_task_field_value")
        .unwrap();
    let description = set_tool["description"].as_str().unwrap();
    for shape in ["YYYY-MM-DD", "option id", "true", "null"] {
        assert!(description.contains(shape), "{shape}: {description}");
    }

    let fields = f
        .tool(
            &reader,
            "list_task_fields",
            json!({"project_id": f.project}),
        )
        .await;
    let items = &fields["result"]["structuredContent"]["items"];
    assert_eq!(items[1]["name"], "Stage", "{fields}");
    assert_eq!(items[1]["options"][1]["id"], f.high);
    let gallery = f.tool(&reader, "list_task_fields", json!({})).await;
    assert_eq!(
        gallery["result"]["structuredContent"]["items"]
            .as_array()
            .unwrap()
            .len(),
        3
    );

    let denied = f
        .tool(
            &reader,
            "set_task_field_value",
            json!({"task_id": f.task, "field_id": f.stage, "value": f.high}),
        )
        .await;
    assert!(denied.get("error").is_some(), "{denied}");

    let set = f
        .tool(
            &writer,
            "set_task_field_value",
            json!({"task_id": f.identifier().await, "field_id": f.stage, "value": f.high}),
        )
        .await;
    assert_eq!(
        set["result"]["structuredContent"]["items"],
        json!([{"field_id": f.stage, "name": "Stage", "kind": "select", "value": f.high, "option_names": ["High"]}]),
        "{set}"
    );
    let read = f
        .tool(&reader, "get_task_field_values", json!({"task_id": f.task}))
        .await;
    assert_eq!(
        read["result"]["structuredContent"],
        set["result"]["structuredContent"]
    );

    // An option name instead of its id, and another project's task, change nothing.
    let invalid = f
        .tool(
            &writer,
            "set_task_field_value",
            json!({"task_id": f.task, "field_id": f.stage, "value": "Low"}),
        )
        .await;
    assert!(invalid.get("error").is_some(), "{invalid}");
    let hidden = f
        .tool(
            &writer,
            "set_task_field_value",
            json!({"task_id": f.other_task, "field_id": f.points, "value": 1}),
        )
        .await;
    assert_eq!(hidden["result"]["isError"], true, "{hidden}");
    assert_eq!(f.stored_value(f.other_task, f.points).await, None);
    assert_eq!(
        f.stored_value(f.task, f.stage).await,
        Some(json!(f.high).to_string())
    );

    let created = f
        .tool(
            &writer,
            "create_task_field",
            json!({"name": "Due review", "kind": "date"}),
        )
        .await;
    let created_id = created["result"]["structuredContent"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("{created}"))
        .to_owned();
    let shown = f
        .tool(
            &writer,
            "set_project_task_fields",
            json!({"project_id": f.project, "field_ids": [f.points, f.stage, created_id]}),
        )
        .await;
    assert_eq!(
        shown["result"]["structuredContent"]["items"][2]["name"], "Due review",
        "{shown}"
    );
    let dated = f
        .tool(
            &writer,
            "set_task_field_value",
            json!({"task_id": f.task, "field_id": created_id, "value": "2026-12-01"}),
        )
        .await;
    assert_eq!(
        dated["result"]["structuredContent"]["items"]
            .as_array()
            .unwrap()
            .len(),
        2,
        "{dated}"
    );
}
