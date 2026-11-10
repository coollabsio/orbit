//! Database tools of MCP and the public API (spec P3 14.3, 14.6): scopes, access, value keys,
//! queries, cursors, row writes and trash, driven over HTTP as agents call them. SQLite by
//! default, Postgres when `ORBIT_TEST_POSTGRES_URL` is set.
use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use orbit_platform::{
    AttachmentMutationCoordinator, AuthenticatedUser, Id, LocalBlobStore, PasswordService,
    TestDatabase, TimestampMillis, UploadLimits, UploadService,
};
use orbit_server::auth_routes::CookieMode;
use orbit_server::database_routes::{DatabaseState, database_router};
use orbit_server::mcp::{self, McpState};
use orbit_server::page_routes::{PageState, page_router};
use orbit_server::public_api::{self, PublicApiState};
use orbit_server::repositories::api_tokens::{ApiTokenRepository, ApiTokenScope};
use orbit_server::repositories::chat::ChatRepository;
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::repositories::pages::PageRepository;
use orbit_server::repositories::tasks::TaskRepository;
use serde_json::{Value, json};
use tower::ServiceExt;

const ORIGIN: &str = "https://orbit.test";

struct Fixture {
    database: TestDatabase,
    _files: tempfile::TempDir,
    identity: Arc<IdentityRepository>,
    tokens: ApiTokenRepository,
    web: axum::Router,
    api: axum::Router,
    mcp: axum::Router,
    workspace: Id,
    project: Id,
    user: Id,
    /// "Member", member@example.com.
    member: Id,
    cookie: String,
}

/// The seeded "Tasks" database: its id, its property ids and its rows' ids by title.
struct Seeded {
    db: String,
    /// The title property, "Name".
    name: String,
    stage: String,
    points: String,
    owner: String,
    view: String,
    rows: Vec<(String, String)>,
}

impl Seeded {
    fn row(&self, title: &str) -> &str {
        &self.rows.iter().find(|(name, _)| name == title).unwrap().1
    }
}

impl Fixture {
    async fn new() -> Self {
        let database = TestDatabase::new().await.unwrap();
        let identity = Arc::new(IdentityRepository::new((*database).clone()));
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
        let web = page_router(PageState::new(Arc::clone(&identity), CookieMode::secure())).merge(
            database_router(DatabaseState::new(
                Arc::clone(&identity),
                CookieMode::secure(),
            )),
        );
        let tokens = ApiTokenRepository::new((*database).clone());
        let tasks = Arc::new(TaskRepository::new((*database).clone()));
        let pages = Arc::new(PageRepository::new((*database).clone()));
        let api = public_api::router(
            PublicApiState::new(Arc::new(tokens.clone()), Arc::clone(&tasks), ORIGIN)
                .with_pages(Arc::clone(&pages)),
        );
        let files = tempfile::tempdir().unwrap();
        let uploads = UploadService::new(
            (*database).clone(),
            Arc::new(LocalBlobStore::new(files.path().join("files"))),
            AttachmentMutationCoordinator::default(),
            UploadLimits::default(),
        );
        let chat = Arc::new(ChatRepository::new((*database).clone(), uploads));
        let mcp = mcp::router(
            McpState::new(Arc::new(tokens.clone()), tasks).with_content(pages, chat),
            ORIGIN,
        );
        let mut fixture = Self {
            database,
            _files: files,
            identity,
            tokens,
            web,
            api,
            mcp,
            workspace: setup.workspace_id,
            project: setup.project_id,
            user: setup.user_id,
            member: setup.user_id,
            cookie: format!("__Host-orbit_session={}", setup.session.token),
        };
        fixture.member = fixture.add_member("member@example.com").await.0;
        fixture
    }

    async fn token(&self, scopes: &[ApiTokenScope]) -> String {
        self.tokens
            .create(
                self.workspace,
                self.user,
                format!("cli-{}", Id::new_v7()),
                vec![self.project],
                scopes,
                false,
                None,
                "test",
                TimestampMillis::now(),
            )
            .await
            .unwrap()
            .token
    }

    /// A service-account token cannot get docs scopes; one from before that rule (its stored
    /// scopes changed here) still holds them.
    async fn service_token(&self) -> String {
        let issued = self
            .tokens
            .create(
                self.workspace,
                self.user,
                format!("bot-{}", Id::new_v7()),
                vec![self.project],
                &[ApiTokenScope::TasksRead],
                true,
                None,
                "test",
                TimestampMillis::now(),
            )
            .await
            .unwrap();
        sqlx::query("UPDATE api_tokens SET scopes = $1 WHERE id = $2")
            .bind(ApiTokenScope::format_list(&[
                ApiTokenScope::DocsRead,
                ApiTokenScope::DocsWrite,
            ]))
            .bind(issued.api_token.id.to_string())
            .execute(self.database.pool())
            .await
            .unwrap();
        issued.token
    }

    /// A member "Member" with `email`; returns the id and a session cookie.
    async fn add_member(&self, email: &str) -> (Id, String) {
        let id = Id::new_v7();
        let now = TimestampMillis::now();
        let mut tx = self.database.immediate_transaction().await.unwrap();
        sqlx::query(
            "INSERT INTO users (id, email, normalized_email, display_name, password_hash, created_at, updated_at) \
             VALUES ($1, $2, $3, 'Member', 'unused', $4, $5)",
        )
        .bind(id.to_string())
        .bind(email)
        .bind(email)
        .bind(now.as_millis())
        .bind(now.as_millis())
        .execute(&mut *tx)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, updated_at) \
             VALUES ($1, $2, $3, 'member', 0, $4, $5)",
        )
        .bind(Id::new_v7().to_string())
        .bind(self.workspace.to_string())
        .bind(id.to_string())
        .bind(now.as_millis())
        .bind(now.as_millis())
        .execute(&mut *tx)
        .await
        .unwrap();
        tx.commit().await.unwrap();
        let session = self
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

    async fn send(app: &axum::Router, request: Request<Body>) -> (StatusCode, Value) {
        let response = app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes)
                .unwrap_or_else(|_| panic!("{}", String::from_utf8_lossy(&bytes)))
        };
        (status, value)
    }

    /// A web route of the workspace with a session cookie; it must succeed.
    async fn web_as(&self, cookie: &str, method: &str, path: &str, body: Value) -> Value {
        let request = Request::builder()
            .method(method)
            .uri(format!("/api/v1/workspaces/{}/{path}", self.workspace))
            .header(header::COOKIE, cookie)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        let (status, value) = Self::send(&self.web, request).await;
        assert!(status.is_success(), "{method} {path}: {status} {value}");
        value
    }

    async fn web(&self, method: &str, path: &str, body: Value) -> Value {
        self.web_as(&self.cookie, method, path, body).await
    }

    /// A public API route of the workspace, such as `docs/{id}/database`.
    async fn api(
        &self,
        token: &str,
        method: &str,
        path: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let builder = Request::builder()
            .method(method)
            .uri(format!(
                "/api/public/v1/workspaces/{}/{path}",
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
        Self::send(&self.api, request).await
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
        let (status, value) = Self::send(&self.mcp, request).await;
        assert_eq!(status, StatusCode::OK, "{value}");
        value
    }

    /// The JSON-RPC response of a tool call.
    async fn tool(&self, token: &str, name: &str, arguments: Value) -> Value {
        self.rpc(
            token,
            "tools/call",
            json!({"name": name, "arguments": arguments}),
        )
        .await
    }

    /// A tool's structured result; the call must succeed.
    async fn tool_ok(&self, token: &str, name: &str, arguments: Value) -> Value {
        let response = self.tool(token, name, arguments).await;
        assert_ne!(response["result"]["isError"], true, "{response}");
        response["result"]["structuredContent"].clone()
    }

    async fn property(&self, db: &str, body: Value) -> String {
        let property = self
            .web("POST", &format!("databases/{db}/properties"), body)
            .await;
        property["id"].as_str().unwrap().to_owned()
    }

    /// "Tasks" with Stage (select Idea, Doing, Done), Points (number) and Owner (person), and
    /// five rows in this manual order:
    ///
    /// | Title   | Stage | Points | Owner  |
    /// |---------|-------|--------|--------|
    /// | Alpha   | Idea  | 3      | owner  |
    /// | Beta    | Doing | 1      | member |
    /// | Gamma   | Done  | 2      |        |
    /// | Delta   | Doing | 5      | owner  |
    /// | Epsilon |       |        |        |
    async fn seed(&self, token: &str) -> Seeded {
        let page = self
            .web(
                "POST",
                "pages",
                json!({"kind": "database", "title": "Tasks"}),
            )
            .await;
        let db = page["id"].as_str().unwrap().to_owned();
        let stage = self
            .property(
                &db,
                json!({"name": "Stage", "type": "select", "config": {"options": [{"name": "Idea"}, {"name": "Doing"}, {"name": "Done"}]}}),
            )
            .await;
        let points = self
            .property(&db, json!({"name": "Points", "type": "number"}))
            .await;
        let owner = self
            .property(&db, json!({"name": "Owner", "type": "person"}))
            .await;
        let detail = self.web("GET", &format!("databases/{db}"), json!({})).await;
        let view = detail["views"][0]["id"].as_str().unwrap().to_owned();
        let name = detail["properties"][0]["id"].as_str().unwrap().to_owned();
        assert_eq!(detail["properties"][0]["type"], "title");
        let mut rows = Vec::new();
        for (title, values) in [
            (
                "Alpha",
                json!({"Stage": "Idea", "Points": 3.0, "Owner": ["owner@example.com"]}),
            ),
            (
                "Beta",
                json!({"Stage": "Doing", "Points": 1, "Owner": ["member@example.com"]}),
            ),
            ("Gamma", json!({"Stage": "Done", "Points": 2})),
            (
                "Delta",
                json!({"Stage": "Doing", "Points": 5, "Owner": ["me"]}),
            ),
            ("Epsilon", json!({})),
        ] {
            let (status, row) = self
                .api(
                    token,
                    "POST",
                    &format!("docs/{db}/rows"),
                    Some(json!({"title": title, "values": values})),
                )
                .await;
            assert_eq!(status, StatusCode::CREATED, "{title}: {row}");
            rows.push((title.to_owned(), row["id"].as_str().unwrap().to_owned()));
        }
        Seeded {
            db,
            name,
            stage,
            points,
            owner,
            view,
            rows,
        }
    }
}

fn titles(page: &Value) -> Vec<&str> {
    page["items"]
        .as_array()
        .unwrap_or_else(|| panic!("{page}"))
        .iter()
        .map(|row| row["title"].as_str().unwrap())
        .collect()
}

/// The text of a tool's error result.
fn tool_error(response: &Value) -> &str {
    assert_eq!(response["result"]["isError"], true, "{response}");
    response["result"]["content"][0]["text"].as_str().unwrap()
}

const TEMPLATE_DETAIL: &str = "No template of this database has this id. Send the id of one of the database's templates (its templates list), or leave template_id out.";
const VALUE_DETAIL: &str = "The value is invalid for this database: an unknown or shared property name, an unknown option or member, or a value of the wrong type.";
const PROPERTY_DETAIL: &str =
    "No property has this name or id, or two properties share the name: use the property id.";

#[tokio::test]
async fn each_tool_and_endpoint_needs_its_docs_scope() {
    let f = Fixture::new().await;
    let write = f.token(&[ApiTokenScope::DocsWrite]).await;
    let s = f.seed(&write).await;
    let (db, alpha) = (s.db.as_str(), s.row("Alpha"));
    let tasks_only = f.token(&[ApiTokenScope::TasksRead]).await;
    let read = f.token(&[ApiTokenScope::DocsRead]).await;

    let reads = [
        ("get_database", json!({"database_id": db})),
        ("query_database", json!({"database_id": db})),
    ];
    let writes = [
        (
            "create_database_row",
            json!({"database_id": db, "title": "New"}),
        ),
        (
            "update_database_row",
            json!({"database_id": db, "row_id": alpha, "title": "Renamed"}),
        ),
        (
            "trash_database_rows",
            json!({"database_id": db, "row_ids": [alpha]}),
        ),
    ];
    let listed = |listing: &Value| -> Vec<String> {
        listing["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap().to_owned())
            .filter(|name| name.contains("database"))
            .collect()
    };
    assert_eq!(
        listed(&f.rpc(&tasks_only, "tools/list", json!({})).await),
        Vec::<String>::new()
    );
    assert_eq!(
        listed(&f.rpc(&read, "tools/list", json!({})).await),
        ["get_database", "query_database"]
    );
    assert_eq!(
        listed(&f.rpc(&write, "tools/list", json!({})).await),
        [
            "get_database",
            "query_database",
            "create_database_row",
            "update_database_row",
            "trash_database_rows"
        ]
    );
    for (name, arguments) in reads.iter().chain(&writes) {
        let refused = f.tool(&tasks_only, name, arguments.clone()).await;
        assert_eq!(
            refused["error"]["message"], "Connection lacks the required permission",
            "{name}: {refused}"
        );
    }
    for (name, arguments) in &writes {
        let refused = f.tool(&read, name, arguments.clone()).await;
        assert_eq!(
            refused["error"]["message"], "Connection lacks the required permission",
            "{name}: {refused}"
        );
    }
    let schema = f.tool_ok(&read, "get_database", reads[0].1.clone()).await;
    assert_eq!(schema["title"], "Tasks");
    let rows = f.tool_ok(&read, "query_database", reads[1].1.clone()).await;
    assert_eq!(
        titles(&rows),
        ["Alpha", "Beta", "Gamma", "Delta", "Epsilon"]
    );

    let rest = [
        ("GET", format!("docs/{db}/database"), None, true),
        (
            "POST",
            format!("docs/{db}/database/query"),
            Some(json!({})),
            true,
        ),
        (
            "POST",
            format!("docs/{db}/rows"),
            Some(json!({"title": "New"})),
            false,
        ),
        (
            "PATCH",
            format!("docs/{db}/rows/{alpha}"),
            Some(json!({"title": "Renamed"})),
            false,
        ),
        (
            "POST",
            format!("docs/{db}/rows/trash"),
            Some(json!({"row_ids": [alpha]})),
            false,
        ),
    ];
    for (method, path, body, is_read) in &rest {
        let (status, problem) = f.api(&tasks_only, method, path, body.clone()).await;
        assert_eq!(
            (status, problem["code"].as_str()),
            (StatusCode::FORBIDDEN, Some("insufficient_scope")),
            "{method} {path}"
        );
        let (status, problem) = f.api(&read, method, path, body.clone()).await;
        if *is_read {
            assert_eq!(status, StatusCode::OK, "{method} {path}: {problem}");
        } else {
            assert_eq!(
                (status, problem["code"].as_str()),
                (StatusCode::FORBIDDEN, Some("insufficient_scope")),
                "{method} {path}"
            );
        }
    }

    // docs:write includes docs:read, and each write lands.
    let (status, created) = f.api(&write, "POST", &rest[2].1, rest[2].2.clone()).await;
    assert_eq!(
        (status, created["title"].as_str()),
        (StatusCode::CREATED, Some("New"))
    );
    let (status, renamed) = f.api(&write, "PATCH", &rest[3].1, rest[3].2.clone()).await;
    assert_eq!(
        (status, renamed["title"].as_str()),
        (StatusCode::OK, Some("Renamed"))
    );
    let (status, trashed) = f.api(&write, "POST", &rest[4].1, rest[4].2.clone()).await;
    assert_eq!((status, trashed), (StatusCode::OK, json!({"trashed": 1})));
    let made = f
        .tool_ok(&write, "create_database_row", writes[0].1.clone())
        .await;
    assert_eq!(made["title"], "New");
    let (status, rows) = f.api(&write, "POST", &rest[1].1, rest[1].2.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        titles(&rows),
        ["Beta", "Gamma", "Delta", "Epsilon", "New", "New"]
    );
}

#[tokio::test]
async fn service_accounts_and_other_members_private_databases_get_nothing() {
    let f = Fixture::new().await;
    let write = f.token(&[ApiTokenScope::DocsWrite]).await;
    let s = f.seed(&write).await;
    let (_, member) = f.add_member("diary@example.com").await;
    let private = f
        .web_as(
            &member,
            "POST",
            "pages",
            json!({"kind": "database", "title": "Diary", "private": true}),
        )
        .await;
    let private = private["id"].as_str().unwrap();

    let service = f.service_token().await;
    for (name, arguments) in [
        ("get_database", json!({"database_id": s.db})),
        ("query_database", json!({"database_id": s.db})),
        (
            "create_database_row",
            json!({"database_id": s.db, "title": "Bot"}),
        ),
        (
            "trash_database_rows",
            json!({"database_id": s.db, "row_ids": [s.row("Alpha")]}),
        ),
    ] {
        let refused = f.tool(&service, name, arguments).await;
        assert_eq!(tool_error(&refused), "Resource is not available", "{name}");
    }
    for (method, path, body) in [
        ("GET", format!("docs/{}/database", s.db), None),
        (
            "POST",
            format!("docs/{}/database/query", s.db),
            Some(json!({})),
        ),
        (
            "POST",
            format!("docs/{}/rows", s.db),
            Some(json!({"title": "Bot"})),
        ),
    ] {
        let (status, _) = f.api(&service, method, &path, body).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{method} {path}");
    }

    for (name, arguments) in [
        ("get_database", json!({"database_id": private})),
        ("query_database", json!({"database_id": private})),
        (
            "create_database_row",
            json!({"database_id": private, "title": "Peek"}),
        ),
    ] {
        let refused = f.tool(&write, name, arguments).await;
        assert_eq!(tool_error(&refused), "Resource is not available", "{name}");
    }
    for (method, path, body) in [
        ("GET", format!("docs/{private}/database"), None),
        (
            "POST",
            format!("docs/{private}/database/query"),
            Some(json!({})),
        ),
        (
            "POST",
            format!("docs/{private}/rows"),
            Some(json!({"title": "Peek"})),
        ),
    ] {
        let (status, _) = f.api(&write, method, &path, body).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{method} {path}");
    }
    let rows = f
        .web_as(&member, "GET", &format!("databases/{private}"), json!({}))
        .await;
    assert_eq!(rows["rows"], json!([]), "no row was added");
    let rows = f
        .tool_ok(&write, "query_database", json!({"database_id": s.db}))
        .await;
    assert_eq!(
        rows["total"], 5,
        "the service account added and trashed nothing"
    );
}

#[tokio::test]
async fn get_database_shows_properties_options_views_and_templates() {
    let f = Fixture::new().await;
    let write = f.token(&[ApiTokenScope::DocsWrite]).await;
    let s = f.seed(&write).await;
    let template = f
        .web(
            "POST",
            &format!("databases/{}/templates", s.db),
            json!({"name": "Weekly"}),
        )
        .await;
    let schema = f
        .tool_ok(&write, "get_database", json!({"database_id": s.db}))
        .await;
    assert_eq!(schema["id"], s.db);
    assert_eq!(schema["title"], "Tasks");
    assert_eq!(
        schema["url"],
        format!("{ORIGIN}/docs/{}?workspace={}", s.db, f.workspace)
    );
    assert_eq!(schema["locked"], false);
    assert_eq!(schema["row_count"], 5);
    let properties: Vec<(&str, &str)> = schema["properties"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| (p["name"].as_str().unwrap(), p["type"].as_str().unwrap()))
        .collect();
    assert_eq!(
        properties,
        [
            ("Name", "title"),
            ("Tags", "multi_select"),
            ("Stage", "select"),
            ("Points", "number"),
            ("Owner", "person")
        ]
    );
    let stage = &schema["properties"][2];
    assert_eq!(stage["id"], s.stage);
    let options: Vec<&str> = stage["options"]
        .as_array()
        .unwrap()
        .iter()
        .map(|option| option["name"].as_str().unwrap())
        .collect();
    assert_eq!(options, ["Idea", "Doing", "Done"]);
    assert_eq!(
        schema["views"],
        json!([{"id": s.view, "name": "Table", "type": "table"}])
    );
    assert_eq!(
        schema["templates"],
        json!([{"id": template["id"], "name": "Weekly"}])
    );
    let (status, rest) = f
        .api(&write, "GET", &format!("docs/{}/database", s.db), None)
        .await;
    assert_eq!((status, &rest), (StatusCode::OK, &schema));
}

#[tokio::test]
async fn values_take_names_ids_options_and_people_as_agents_write_them() {
    let f = Fixture::new().await;
    let write = f.token(&[ApiTokenScope::DocsWrite]).await;
    let s = f.seed(&write).await;
    let member = f.member;

    // The seed wrote Owner by email and "me"; results show people as {id, name}.
    let rows = f
        .tool_ok(&write, "query_database", json!({"database_id": s.db}))
        .await;
    let alpha = &rows["items"][0];
    assert_eq!(alpha["id"], s.row("Alpha"));
    assert_eq!(
        alpha["url"],
        format!("{ORIGIN}/docs/{}?workspace={}", s.row("Alpha"), f.workspace)
    );
    assert_eq!(
        alpha["values"],
        json!({
            "Stage": "Idea",
            "Points": 3.0,
            "Owner": [{"id": f.user, "name": "Owner"}],
            "Tags": null,
        })
    );
    assert_eq!(alpha["created_by"], json!({"id": f.user, "name": "Owner"}));
    assert_eq!(
        rows["items"][1]["values"]["Owner"],
        json!([{"id": member, "name": "Member"}])
    );

    // Keys by id, by exact name and by a name in another letter case; option by a name in
    // another letter case; a person by email.
    let row = f
        .tool_ok(
            &write,
            "update_database_row",
            json!({
                "database_id": s.db,
                "row_id": s.row("Epsilon"),
                "values": {s.points.clone(): 8, "stage": "doNE", "OWNER": ["member@example.com"]},
            }),
        )
        .await;
    assert_eq!(row["values"]["Points"], 8.0);
    assert_eq!(row["values"]["Stage"], "Done");
    assert_eq!(
        row["values"]["Owner"],
        json!([{"id": member, "name": "Member"}])
    );

    // An unknown option is refused with the key the agent sent, and adds no option.
    let refused = f
        .tool(
            &write,
            "update_database_row",
            json!({"database_id": s.db, "row_id": s.row("Epsilon"), "values": {"stage": "Shipped"}}),
        )
        .await;
    assert_eq!(
        tool_error(&refused),
        format!("Invalid database input values.stage. {VALUE_DETAIL}")
    );
    let (status, problem) = f
        .api(
            &write,
            "POST",
            &format!("docs/{}/rows", s.db),
            Some(json!({"title": "Nope", "values": {"Stage": "Shipped"}})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{problem}");
    assert_eq!(problem["errors"], json!({"values.Stage": [VALUE_DETAIL]}));
    let schema = f
        .tool_ok(&write, "get_database", json!({"database_id": s.db}))
        .await;
    assert_eq!(
        schema["properties"][2]["options"].as_array().unwrap().len(),
        3
    );
    assert_eq!(schema["row_count"], 5, "the refused create added no row");

    // An email of no member is refused too.
    let (status, problem) = f
        .api(
            &write,
            "PATCH",
            &format!("docs/{}/rows/{}", s.db, s.row("Gamma")),
            Some(json!({"values": {"Owner": ["nobody@example.com"]}})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{problem}");
    assert_eq!(problem["errors"], json!({"values.Owner": [VALUE_DETAIL]}));

    // Two properties named "Note": the name is refused, each id works, and rows show them
    // under their ids.
    let first = f
        .property(&s.db, json!({"name": "Note", "type": "text"}))
        .await;
    let second = f
        .property(&s.db, json!({"name": "Note", "type": "text"}))
        .await;
    let (status, problem) = f
        .api(
            &write,
            "PATCH",
            &format!("docs/{}/rows/{}", s.db, s.row("Gamma")),
            Some(json!({"values": {"note": "x"}})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{problem}");
    assert_eq!(problem["errors"], json!({"values.note": [VALUE_DETAIL]}));
    let (status, problem) = f
        .api(
            &write,
            "POST",
            &format!("docs/{}/database/query", s.db),
            Some(json!({"sorts": [{"property_id": "Note"}]})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{problem}");
    assert_eq!(
        problem["errors"],
        json!({"sorts[0].property_id": [PROPERTY_DETAIL]})
    );
    let (status, row) = f
        .api(
            &write,
            "PATCH",
            &format!("docs/{}/rows/{}", s.db, s.row("Gamma")),
            Some(json!({"values": {first.clone(): "one", second.clone(): "two"}})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{row}");
    assert_eq!(row["values"][&first], "one");
    assert_eq!(row["values"][&second], "two");
    assert!(
        !row["values"].as_object().unwrap().contains_key("Note"),
        "{row}"
    );
}

#[tokio::test]
async fn queries_filter_and_sort_by_names_and_views() {
    let f = Fixture::new().await;
    let write = f.token(&[ApiTokenScope::DocsWrite]).await;
    let s = f.seed(&write).await;
    let query = |body: Value| {
        let f = &f;
        let db = s.db.clone();
        let write = write.clone();
        async move {
            let mut arguments = body;
            arguments["database_id"] = json!(db);
            f.tool_ok(&write, "query_database", arguments).await
        }
    };

    let rows = query(json!({
        "filter": {"op": "or", "children": [
            {"property_id": "stage", "operator": "is", "value": ["doing"]},
            {"property_id": s.points, "operator": ">=", "value": 3},
        ]},
        "sorts": [{"property_id": "Points", "direction": "desc"}],
    }))
    .await;
    assert_eq!(titles(&rows), ["Delta", "Alpha", "Beta"]);
    assert_eq!(rows["total"], 3);
    assert_eq!(rows["next_cursor"], Value::Null);

    let rows = query(json!({
        "filter": {"property_id": s.owner, "operator": "contains", "value": ["me"]},
        "sorts": [{"property_id": "Points"}],
    }))
    .await;
    assert_eq!(titles(&rows), ["Alpha", "Delta"]);
    let rows = query(json!({
        "filter": {"property_id": "Owner", "operator": "contains", "value": ["member@example.com"]},
    }))
    .await;
    assert_eq!(titles(&rows), ["Beta"]);
    let rows = query(json!({"sorts": [{"property_id": "Points", "direction": "asc"}]})).await;
    assert_eq!(
        titles(&rows),
        ["Beta", "Gamma", "Alpha", "Delta", "Epsilon"],
        "empty values sort last"
    );

    // A view with a filter (Points is not empty) and a sort (Name descending).
    f.web(
        "PATCH",
        &format!("databases/{}/views/{}", s.db, s.view),
        json!({"config": {
            "filter": {"op": "and", "children": [{"property_id": s.points, "operator": "is_not_empty", "value": null}]},
            "sorts": [{"property_id": s.name, "direction": "desc"}],
        }}),
    )
    .await;
    let rows = query(json!({"view_id": s.view})).await;
    assert_eq!(titles(&rows), ["Gamma", "Delta", "Beta", "Alpha"]);
    let rows = query(json!({
        "view_id": s.view,
        "filter": {"property_id": "Stage", "operator": "is_not", "value": ["Done"]},
    }))
    .await;
    assert_eq!(
        titles(&rows),
        ["Delta", "Beta", "Alpha"],
        "the sent filter is ANDed with the view's"
    );
    let rows = query(json!({
        "view_id": s.view,
        "sorts": [{"property_id": "Points", "direction": "desc"}],
    }))
    .await;
    assert_eq!(
        titles(&rows),
        ["Delta", "Alpha", "Gamma", "Beta"],
        "the sent sorts replace the view's"
    );
    let (status, rest) = f
        .api(
            &write,
            "POST",
            &format!("docs/{}/database/query", s.db),
            Some(json!({"view_id": s.view, "sorts": []})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{rest}");
    assert_eq!(
        titles(&rest),
        ["Alpha", "Beta", "Gamma", "Delta"],
        "empty sorts replace the view's too"
    );
    let unknown = f
        .tool(
            &write,
            "query_database",
            json!({"database_id": s.db, "view_id": Id::new_v7()}),
        )
        .await;
    assert_eq!(tool_error(&unknown), "Database view is not available");
}

#[tokio::test]
async fn cursors_page_one_query_and_bad_inputs_name_their_field() {
    let f = Fixture::new().await;
    let write = f.token(&[ApiTokenScope::DocsWrite]).await;
    let s = f.seed(&write).await;
    let path = format!("docs/{}/database/query", s.db);
    let sorts = json!([{"property_id": "Name"}]);

    let mut seen = Vec::new();
    let mut cursor = Value::Null;
    for expected in [
        vec!["Alpha", "Beta"],
        vec!["Delta", "Epsilon"],
        vec!["Gamma"],
    ] {
        let (status, page) = f
            .api(
                &write,
                "POST",
                &path,
                Some(json!({"sorts": sorts, "limit": 2, "cursor": cursor})),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{page}");
        assert_eq!(titles(&page), expected);
        assert_eq!(page["total"], 5);
        seen.extend(expected);
        cursor = page["next_cursor"].clone();
    }
    assert_eq!(cursor, Value::Null, "the last page has no next_cursor");
    assert_eq!(seen.len(), 5);

    let (_, first) = f
        .api(
            &write,
            "POST",
            &path,
            Some(json!({"sorts": sorts, "limit": 2})),
        )
        .await;
    let old = first["next_cursor"].as_str().unwrap();
    for changed in [
        json!({"sorts": [{"property_id": "Name", "direction": "desc"}], "limit": 2, "cursor": old}),
        json!({"sorts": sorts, "limit": 2, "cursor": old, "tz": "Europe/Budapest"}),
        json!({"sorts": sorts, "limit": 2, "cursor": old, "view_id": s.view}),
        json!({"sorts": sorts, "limit": 2, "cursor": old, "filter": {"property_id": "Points", "operator": "is_not_empty"}}),
    ] {
        let (status, problem) = f.api(&write, "POST", &path, Some(changed.clone())).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{changed}");
        assert_eq!(
            problem["errors"]
                .as_object()
                .unwrap()
                .keys()
                .collect::<Vec<_>>(),
            ["cursor"],
            "{changed}"
        );
    }
    // A different limit is the same query: the cursor continues.
    let (status, next) = f
        .api(
            &write,
            "POST",
            &path,
            Some(json!({"sorts": sorts, "limit": 100, "cursor": old})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{next}");
    assert_eq!(titles(&next), ["Delta", "Epsilon", "Gamma"]);

    for (body, field) in [
        (json!({"limit": 0}), "limit"),
        (json!({"limit": 101}), "limit"),
        (json!({"tz": "Mars/Olympus"}), "tz"),
        (
            json!({"filter": {"property_id": "Size", "operator": "is_empty"}}),
            "filter.property_id",
        ),
        (
            json!({"filter": {"op": "and", "children": [{"property_id": "Points", "operator": "contains", "value": "1"}]}}),
            "filter.children[0].operator",
        ),
        (
            json!({"sorts": [{"property_id": "Points", "direction": "up"}]}),
            "sorts[0].direction",
        ),
    ] {
        let (status, problem) = f.api(&write, "POST", &path, Some(body.clone())).await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{body}: {problem}"
        );
        assert_eq!(
            problem["errors"]
                .as_object()
                .unwrap()
                .keys()
                .collect::<Vec<_>>(),
            [field],
            "{body}: {problem}"
        );
        let mut arguments = body.clone();
        arguments["database_id"] = json!(s.db);
        let refused = f.tool(&write, "query_database", arguments).await;
        assert!(
            tool_error(&refused).starts_with(&format!("Invalid database input {field}. ")),
            "{body}: {refused}"
        );
    }
    let (status, page) = f
        .api(
            &write,
            "POST",
            &path,
            Some(json!({"limit": 100, "tz": "Europe/Budapest"})),
        )
        .await;
    assert_eq!((status, page["total"].clone()), (StatusCode::OK, json!(5)));
    assert_eq!(page["items"].as_array().unwrap().len(), 5);
}

#[tokio::test]
async fn a_locked_database_takes_value_writes_and_trash_counts_live_rows() {
    let f = Fixture::new().await;
    let write = f.token(&[ApiTokenScope::DocsWrite]).await;
    let s = f.seed(&write).await;
    f.web(
        "POST",
        &format!("pages/{}/lock", s.db),
        json!({"locked": true}),
    )
    .await;
    let schema = f
        .tool_ok(&write, "get_database", json!({"database_id": s.db}))
        .await;
    assert_eq!(schema["locked"], true);

    let row = f
        .tool_ok(
            &write,
            "update_database_row",
            json!({"database_id": s.db, "row_id": s.row("Gamma"), "values": {"Points": 9, "Stage": "Idea"}, "title": "Gamma 2"}),
        )
        .await;
    assert_eq!(
        (
            &row["title"],
            &row["values"]["Points"],
            &row["values"]["Stage"]
        ),
        (&json!("Gamma 2"), &json!(9.0), &json!("Idea"))
    );
    let (status, made) = f
        .api(
            &write,
            "POST",
            &format!("docs/{}/rows", s.db),
            Some(json!({"title": "Zeta", "values": {"Points": 4}})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{made}");
    assert_eq!(made["values"]["Points"], 4.0);

    let trash = |row_ids: Value| {
        let f = &f;
        let write = write.clone();
        let db = s.db.clone();
        async move {
            f.tool_ok(
                &write,
                "trash_database_rows",
                json!({"database_id": db, "row_ids": row_ids}),
            )
            .await
        }
    };
    assert_eq!(trash(json!([s.row("Alpha")])).await, json!({"trashed": 1}));
    // Alpha is in the trash already, an unknown id and the database itself are no rows.
    assert_eq!(
        trash(json!([
            s.row("Alpha"),
            s.row("Beta"),
            s.row("Beta"),
            Id::new_v7(),
            s.db
        ]))
        .await,
        json!({"trashed": 1})
    );
    let (status, problem) = f
        .api(
            &write,
            "POST",
            &format!("docs/{}/rows/trash", s.db),
            Some(json!({"row_ids": (0..101).map(|_| Id::new_v7()).collect::<Vec<_>>()})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{problem}");
    assert_eq!(
        problem["errors"],
        json!({"row_ids": ["Send 1 to 100 row ids."]})
    );
    let rows = f
        .tool_ok(&write, "query_database", json!({"database_id": s.db}))
        .await;
    assert_eq!(titles(&rows), ["Gamma 2", "Delta", "Epsilon", "Zeta"]);
}

#[tokio::test]
async fn rows_get_a_markdown_body_or_a_template_and_read_as_docs() {
    let f = Fixture::new().await;
    let write = f.token(&[ApiTokenScope::DocsWrite]).await;
    let s = f.seed(&write).await;

    let row = f
        .tool_ok(
            &write,
            "create_database_row",
            json!({"database_id": s.db, "title": "Plan", "values": {"Stage": "Idea"}, "markdown": "## Steps\n\nShip it."}),
        )
        .await;
    assert_eq!(row["title"], "Plan");
    let doc = f
        .tool_ok(&write, "get_doc", json!({"page_id": row["id"]}))
        .await;
    assert_eq!(doc["markdown"], "## Steps\n\nShip it.\n");
    assert_eq!(doc["database_id"], s.db);
    assert_eq!(doc["values"], row["values"]);
    assert_eq!(doc["values"]["Stage"], "Idea");

    let template = f
        .web(
            "POST",
            &format!("databases/{}/templates", s.db),
            json!({"name": "Weekly"}),
        )
        .await;
    let template = template["id"].as_str().unwrap();
    let schema = f
        .tool_ok(&write, "get_database", json!({"database_id": s.db}))
        .await;
    let done = schema["properties"][2]["options"][2]["id"].clone();
    let page = f.web("GET", &format!("pages/{template}"), json!({})).await;
    f.web(
        "PATCH",
        &format!("pages/{template}"),
        json!({"expected_version": page["version"], "title": "Weekly review"}),
    )
    .await;
    f.web(
        "PATCH",
        &format!("databases/{}/rows/{template}/values", s.db),
        json!({s.stage.clone(): done, s.points.clone(): 7}),
    )
    .await;

    let (status, from_template) = f
        .api(
            &write,
            "POST",
            &format!("docs/{}/rows", s.db),
            Some(json!({"template_id": template, "values": {"Points": 1}, "markdown": "Fresh body."})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{from_template}");
    assert_eq!(from_template["title"], "Weekly review");
    assert_eq!(from_template["values"]["Stage"], "Done");
    assert_eq!(from_template["values"]["Points"], 1.0, "sent values win");
    let (status, doc) = f
        .api(
            &write,
            "GET",
            &format!("docs/{}", from_template["id"].as_str().unwrap()),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{doc}");
    assert_eq!(doc["markdown"], "Fresh body.\n");
    assert_eq!(doc["database_id"], s.db);
    assert_eq!(doc["values"], from_template["values"]);
    let missing = f
        .tool(
            &write,
            "create_database_row",
            json!({"database_id": s.db, "template_id": Id::new_v7()}),
        )
        .await;
    assert_eq!(
        tool_error(&missing),
        format!("Invalid database input template_id. {TEMPLATE_DETAIL}")
    );
    let (status, problem) = f
        .api(
            &write,
            "POST",
            &format!("docs/{}/rows", s.db),
            Some(json!({"template_id": Id::new_v7()})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{problem}");
    assert_eq!(problem["errors"], json!({"template_id": [TEMPLATE_DETAIL]}));

    // `list_docs` shows the database with its kind; a plain doc has no database_id or values.
    let docs = f.tool_ok(&write, "list_docs", json!({})).await;
    let kinds: Vec<(&str, &str)> = docs["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| (d["title"].as_str().unwrap(), d["kind"].as_str().unwrap()))
        .collect();
    assert!(kinds.contains(&("Tasks", "database")), "{kinds:?}");
    let (_, listed) = f.api(&write, "GET", "docs", None).await;
    let tasks = listed["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["id"] == s.db)
        .unwrap();
    assert_eq!(tasks["kind"], "database");
    let plain = f
        .tool_ok(
            &write,
            "create_doc",
            json!({"title": "Plain", "markdown": "Hi."}),
        )
        .await;
    let plain = f
        .tool_ok(&write, "get_doc", json!({"page_id": plain["id"]}))
        .await;
    assert_eq!(plain["database_id"], Value::Null);
    assert_eq!(plain.get("values"), None);
}
