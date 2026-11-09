use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{HeaderValue, Request, StatusCode, header};
use orbit_platform::{AuthenticatedUser, Id, PasswordService, TestDatabase, TimestampMillis};
use orbit_server::auth_routes::CookieMode;
use orbit_server::database_routes::{DatabaseState, database_router};
use orbit_server::page_routes::{PageState, page_router};
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::{self, client::IntoClientRequest};
use tower::ServiceExt;

struct Fixture {
    database: TestDatabase,
    identity: Arc<IdentityRepository>,
    app: axum::Router,
    workspace_id: String,
    cookie: String,
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
        let app = page_router(PageState::new(Arc::clone(&identity), CookieMode::secure())).merge(
            database_router(DatabaseState::new(
                Arc::clone(&identity),
                CookieMode::secure(),
            )),
        );
        Self {
            database,
            identity,
            app,
            workspace_id: setup.workspace_id.to_string(),
            cookie: format!("__Host-orbit_session={}", setup.session.token),
            owner_id: setup.user_id,
        }
    }

    fn uri(&self, path: &str) -> String {
        format!("/api/v1/workspaces/{}/{path}", self.workspace_id)
    }

    async fn call(&self, method: &str, path: &str, body: Option<Value>) -> (StatusCode, Value) {
        self.call_as(&self.cookie, method, path, body).await
    }

    async fn call_as(
        &self,
        cookie: &str,
        method: &str,
        path: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let builder = Request::builder()
            .method(method)
            .uri(self.uri(path))
            .header(header::COOKIE, cookie);
        let request = match body {
            Some(value) => builder
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(value.to_string())),
            None => builder.body(Body::empty()),
        }
        .unwrap();
        let response = self.app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        };
        (status, value)
    }

    async fn ok(&self, method: &str, path: &str, body: Option<Value>) -> Value {
        let (status, value) = self.call(method, path, body).await;
        assert!(status.is_success(), "{method} {path}: {status} {value}");
        value
    }

    async fn create_page(&self, body: Value) -> Value {
        self.ok("POST", "pages", Some(body)).await
    }

    /// A new database; returns its id.
    async fn create_database(&self, title: &str) -> String {
        let page = self
            .create_page(json!({"kind": "database", "title": title}))
            .await;
        id_of(&page).to_owned()
    }

    async fn database(&self, id: &str) -> Value {
        self.ok("GET", &format!("databases/{id}"), None).await
    }

    async fn add_property(&self, database: &str, body: Value) -> Value {
        self.ok(
            "POST",
            &format!("databases/{database}/properties"),
            Some(body),
        )
        .await
    }

    async fn add_row(&self, database: &str, body: Value) -> Value {
        self.ok("POST", &format!("databases/{database}/rows"), Some(body))
            .await
    }

    async fn set_values(&self, database: &str, row: &str, values: Value) -> (StatusCode, Value) {
        self.call(
            "PATCH",
            &format!("databases/{database}/rows/{row}/values"),
            Some(values),
        )
        .await
    }

    async fn page(&self, id: &str) -> Value {
        self.ok("GET", &format!("pages/{id}"), None).await
    }

    async fn trash_page(&self, id: &str) {
        let version = self.page(id).await["version"].as_u64().unwrap();
        self.ok(
            "DELETE",
            &format!("pages/{id}?expected_version={version}"),
            None,
        )
        .await;
    }

    async fn restore_page(&self, id: &str) {
        let (_, trash) = self.call("GET", "pages/trash", None).await;
        let version = trash["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["id"] == id)
            .expect("in the trash")["version"]
            .clone();
        self.ok(
            "POST",
            &format!("pages/{id}/restore"),
            Some(json!({"expected_version": version})),
        )
        .await;
    }

    async fn add_member(&self, email: &str) -> (Id, String) {
        let id = Id::new_v7();
        let now = TimestampMillis::now();
        sqlx::query(
            "INSERT INTO users (id, email, normalized_email, display_name, password_hash, created_at, updated_at) \
             VALUES ($1, $2, $3, 'Member', 'unused', $4, $5)",
        )
        .bind(id.to_string())
        .bind(email)
        .bind(email)
        .bind(now.as_millis())
        .bind(now.as_millis())
        .execute(self.database.pool())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, updated_at) \
             VALUES ($1, $2, $3, 'member', 0, $4, $5)",
        )
        .bind(Id::new_v7().to_string())
        .bind(&self.workspace_id)
        .bind(id.to_string())
        .bind(now.as_millis())
        .bind(now.as_millis())
        .execute(self.database.pool())
        .await
        .unwrap();
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
}

fn id_of(value: &Value) -> &str {
    value["id"].as_str().unwrap()
}

fn property<'a>(database: &'a Value, name: &str) -> &'a Value {
    database["properties"]
        .as_array()
        .unwrap()
        .iter()
        .find(|property| property["name"] == name)
        .unwrap_or_else(|| panic!("no property {name}"))
}

fn names(items: &Value) -> Vec<&str> {
    items
        .as_array()
        .unwrap()
        .iter()
        .map(|item| {
            item["name"]
                .as_str()
                .unwrap_or_else(|| item["title"].as_str().unwrap())
        })
        .collect()
}

fn option_id(property: &Value, name: &str) -> String {
    property["config"]["options"]
        .as_array()
        .unwrap()
        .iter()
        .find(|option| option["name"] == name)
        .unwrap_or_else(|| panic!("no option {name}"))["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[tokio::test]
async fn a_new_database_has_a_title_tags_and_one_table_view_and_no_body() {
    let fixture = Fixture::new().await;
    let page = fixture
        .create_page(json!({"kind": "database", "title": "Research"}))
        .await;
    assert_eq!(page["kind"], "database");
    assert_eq!(page["database_id"], Value::Null);
    let id = id_of(&page);

    let database = fixture.database(id).await;
    assert_eq!(database["database"]["title"], "Research");
    assert_eq!(database["database"]["row_count"], 0);
    assert_eq!(database["database"]["locked"], false);
    let properties: Vec<(&str, &str, i64)> = database["properties"]
        .as_array()
        .unwrap()
        .iter()
        .map(|property| {
            (
                property["name"].as_str().unwrap(),
                property["type"].as_str().unwrap(),
                property["position"].as_i64().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        properties,
        [("Name", "title", 0), ("Tags", "multi_select", 1)]
    );
    assert_eq!(
        property(&database, "Tags")["config"],
        json!({"options": []})
    );
    let views = database["views"].as_array().unwrap();
    assert_eq!(views.len(), 1);
    assert_eq!(views[0]["name"], "Table");
    assert_eq!(views[0]["type"], "table");
    assert_eq!(
        views[0]["config"],
        json!({
            "columns": [
                {"property_id": property(&database, "Name")["id"], "width": 280, "hidden": false},
                {"property_id": property(&database, "Tags")["id"], "width": 200, "hidden": false},
            ],
            "sorts": [],
            "filter": {"op": "and", "children": []},
            "wrap_cells": false,
        })
    );
    assert_eq!(database["rows"], json!([]));

    let (_, tree) = fixture.call("GET", "pages", None).await;
    assert_eq!(tree["items"][0]["kind"], "database");
    let (status, body) = fixture
        .call(
            "PATCH",
            &format!("pages/{id}"),
            Some(json!({"expected_version": 0, "content": []})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");

    // A plain page is not a database.
    let plain = fixture.create_page(json!({"title": "Plain"})).await;
    let (status, body) = fixture
        .call("GET", &format!("databases/{}", id_of(&plain)), None)
        .await;
    assert_eq!(
        (status, body["code"].as_str()),
        (StatusCode::NOT_FOUND, Some("page_not_found"))
    );

    // The co-editing socket refuses the database page and opens for a plain page.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = fixture.app.clone();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let open = |page: String| {
        let mut request = format!(
            "ws://{address}/api/v1/workspaces/{}/pages/{page}/collab?v=1",
            fixture.workspace_id
        )
        .into_client_request()
        .unwrap();
        request
            .headers_mut()
            .insert("cookie", HeaderValue::from_str(&fixture.cookie).unwrap());
        async move {
            match tokio_tungstenite::connect_async(request).await {
                Ok(_) => 101,
                Err(tungstenite::Error::Http(response)) => response.status().as_u16(),
                Err(error) => panic!("handshake: {error}"),
            }
        }
    };
    assert_eq!(open(id.to_owned()).await, 409);
    assert_eq!(open(id_of(&plain).to_owned()).await, 101);
    server.abort();
}

#[tokio::test]
async fn values_are_checked_against_each_property_type() {
    let fixture = Fixture::new().await;
    let (member_id, _) = fixture.add_member("member@example.com").await;
    let db = fixture.create_database("Tracker").await;
    let mut ids = std::collections::HashMap::new();
    for (name, kind) in [
        ("Notes", "text"),
        ("Score", "number"),
        ("Stage", "select"),
        ("Status", "status"),
        ("Due", "date"),
        ("Owner", "person"),
        ("Done", "checkbox"),
        ("Link", "url"),
        ("Created", "created_time"),
    ] {
        let config = (kind == "select")
            .then(|| json!({"options": [{"id": "s1", "name": "One", "color": "blue"}]}));
        let property = fixture
            .add_property(&db, json!({"name": name, "type": kind, "config": config}))
            .await;
        ids.insert(name, id_of(&property).to_owned());
    }
    let database = fixture.database(&db).await;
    let status = property(&database, "Status");
    assert_eq!(
        status["config"]["options"]
            .as_array()
            .unwrap()
            .iter()
            .map(|option| (
                option["name"].as_str().unwrap(),
                option["group"].as_str().unwrap()
            ))
            .collect::<Vec<_>>(),
        [
            ("Not started", "todo"),
            ("In progress", "in_progress"),
            ("Done", "done")
        ]
    );
    let done = option_id(status, "Done");
    let tags = id_of(property(&database, "Tags")).to_owned();
    let name = id_of(property(&database, "Name")).to_owned();
    // A view lists every property, the new ones appended.
    assert_eq!(
        database["views"][0]["config"]["columns"]
            .as_array()
            .unwrap()
            .len(),
        11
    );

    let row = fixture.add_row(&db, json!({"title": "Task"})).await;
    assert_eq!(row["values"], json!({}));
    let row_id = id_of(&row).to_owned();
    let good = json!({
        &ids["Notes"]: "plain text",
        &ids["Score"]: 12.5,
        &ids["Stage"]: "s1",
        &ids["Status"]: done,
        &ids["Due"]: {"start": "2026-11-09T14:00Z", "end": "2026-11-10T09:30:00Z"},
        &ids["Owner"]: [member_id.to_string(), fixture.owner_id.to_string()],
        &ids["Done"]: true,
        &ids["Link"]: "https://example.com/a?b=c",
    });
    let (code, updated) = fixture.set_values(&db, &row_id, good).await;
    assert_eq!(code, StatusCode::OK, "{updated}");
    assert_eq!(
        updated["values"],
        json!({
            &ids["Notes"]: "plain text",
            &ids["Score"]: 12.5,
            &ids["Stage"]: "s1",
            &ids["Status"]: done,
            &ids["Due"]: {"start": "2026-11-09T14:00:00Z", "end": "2026-11-10T09:30:00Z"},
            &ids["Owner"]: [member_id.to_string(), fixture.owner_id.to_string()],
            &ids["Done"]: true,
            &ids["Link"]: "https://example.com/a?b=c",
        })
    );
    assert_eq!(
        updated["version"], row["version"],
        "a value write keeps the version"
    );
    assert!(updated["updated_at"].as_str() >= row["updated_at"].as_str());

    let stranger = Id::new_v7().to_string();
    let long_text = "x".repeat(10_001);
    let long_url = format!("https://example.com/{}", "a".repeat(2_000));
    let too_many: Vec<String> = (0..101).map(|_| Id::new_v7().to_string()).collect();
    for (key, value) in [
        (&ids["Notes"], json!(5)),
        (&ids["Notes"], json!(long_text)),
        (&ids["Score"], json!("12")),
        (&ids["Stage"], json!("missing-option")),
        (&ids["Status"], json!(["s1"])),
        (&ids["Due"], json!({"start": "2026-02-30"})),
        (
            &ids["Due"],
            json!({"start": "2026-11-10", "end": "2026-11-09"}),
        ),
        (
            &ids["Due"],
            json!({"start": "2026-11-10", "end": "2026-11-11T10:00Z"}),
        ),
        (&ids["Owner"], json!([stranger])),
        (
            &ids["Owner"],
            json!([member_id.to_string(), member_id.to_string()]),
        ),
        (&ids["Owner"], json!(too_many)),
        (&ids["Done"], json!("yes")),
        (&ids["Link"], json!("javascript:alert(1)")),
        (&ids["Link"], json!(long_url)),
        (&ids["Created"], json!("2026-11-09")),
        (&name, json!("A title is not a value")),
        (&tags, json!(["not-an-option"])),
        (&stranger, json!("unknown property")),
    ] {
        let (code, body) = fixture
            .set_values(&db, &row_id, json!({ key.as_str(): value }))
            .await;
        assert_eq!(
            code,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{key} {value}: {body}"
        );
        assert_eq!(body["detail"], format!("values.{key}"), "{body}");
    }

    // null, false and "" clear; the rest stays.
    let (_, cleared) = fixture
        .set_values(
            &db,
            &row_id,
            json!({&ids["Notes"]: "", &ids["Done"]: false, &ids["Score"]: null}),
        )
        .await;
    let mut keys: Vec<&String> = cleared["values"].as_object().unwrap().keys().collect();
    keys.sort();
    let mut expected = vec![
        &ids["Stage"],
        &ids["Status"],
        &ids["Due"],
        &ids["Owner"],
        &ids["Link"],
    ];
    expected.sort();
    assert_eq!(keys, expected);
    // Values can be given when the row is made.
    let second = fixture
        .add_row(
            &db,
            json!({"title": "Second", "values": {&ids["Score"]: 3}}),
        )
        .await;
    assert_eq!(second["values"], json!({&ids["Score"]: 3.0}));
}

#[tokio::test]
async fn type_changes_convert_values_as_the_table_says() {
    let fixture = Fixture::new().await;
    let db = fixture.create_database("Conversions").await;
    let text = id_of(
        &fixture
            .add_property(&db, json!({"name": "Raw", "type": "text"}))
            .await,
    )
    .to_owned();
    let mut rows = Vec::new();
    for value in [
        "12.5",
        "AI, Rust",
        "yes",
        "https://orbit.dev",
        "2026-11-09",
        "AI, Rust",
    ] {
        let row = fixture
            .add_row(&db, json!({"title": value, "values": {&text: value}}))
            .await;
        rows.push(id_of(&row).to_owned());
    }
    let retype = |to: &'static str| {
        let fixture = &fixture;
        let path = format!("databases/{db}/properties/{text}");
        async move { fixture.ok("PATCH", &path, Some(json!({"type": to}))).await }
    };
    let value_of = |database: &Value, row: usize| {
        database["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["id"] == rows[row].as_str())
            .unwrap()["values"][&text]
            .clone()
    };

    // text → multi_select makes options from the comma-separated values.
    let update = retype("multi_select").await;
    assert_eq!(
        (update["converted"].as_u64(), update["cleared"].as_u64()),
        (Some(6), Some(0))
    );
    let options: Vec<&str> = update["property"]["config"]["options"]
        .as_array()
        .unwrap()
        .iter()
        .map(|option| option["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        options,
        [
            "12.5",
            "AI",
            "Rust",
            "yes",
            "https://orbit.dev",
            "2026-11-09"
        ]
    );
    let database = fixture.database(&db).await;
    let ai = option_id(&update["property"], "AI");
    let rust = option_id(&update["property"], "Rust");
    assert_eq!(value_of(&database, 1), json!([ai, rust]));
    assert_eq!(value_of(&database, 5), json!([ai, rust]));

    // multi → single keeps the first option and the options.
    let update = retype("select").await;
    assert_eq!(
        update["property"]["config"]["options"]
            .as_array()
            .unwrap()
            .len(),
        6
    );
    assert_eq!(value_of(&fixture.database(&db).await, 1), json!(ai));

    // select → text writes the option names.
    retype("text").await;
    let database = fixture.database(&db).await;
    assert_eq!(value_of(&database, 1), json!("AI"));
    assert_eq!(value_of(&database, 2), json!("yes"));

    // text → number keeps what parses.
    let update = retype("number").await;
    assert_eq!(
        (update["converted"].as_u64(), update["cleared"].as_u64()),
        (Some(1), Some(5))
    );
    assert_eq!(value_of(&fixture.database(&db).await, 0), json!(12.5));

    // Back to text, then to checkbox, url and date.
    retype("text").await;
    for row in 1..6 {
        let original = [
            "12.5",
            "AI, Rust",
            "yes",
            "https://orbit.dev",
            "2026-11-09",
            "AI, Rust",
        ][row];
        fixture
            .set_values(&db, &rows[row], json!({&text: original}))
            .await;
    }
    let update = retype("checkbox").await;
    assert_eq!(update["converted"], 1);
    assert_eq!(value_of(&fixture.database(&db).await, 2), json!(true));
    retype("text").await;
    assert_eq!(value_of(&fixture.database(&db).await, 2), json!("true"));

    for row in 0..6 {
        let original = [
            "12.5",
            "AI, Rust",
            "yes",
            "https://orbit.dev",
            "2026-11-09",
            "AI, Rust",
        ][row];
        fixture
            .set_values(&db, &rows[row], json!({&text: original}))
            .await;
    }
    let update = retype("url").await;
    assert_eq!(update["converted"], 1);
    assert_eq!(
        value_of(&fixture.database(&db).await, 3),
        json!("https://orbit.dev")
    );
    retype("text").await;
    fixture
        .set_values(&db, &rows[4], json!({&text: "2026-11-09"}))
        .await;
    let update = retype("date").await;
    assert_eq!(update["converted"], 1);
    assert_eq!(
        value_of(&fixture.database(&db).await, 4),
        json!({"start": "2026-11-09", "end": null})
    );

    // To person (and to a computed type) clears every value.
    let update = retype("person").await;
    assert_eq!(
        (update["converted"].as_u64(), update["cleared"].as_u64()),
        (Some(0), Some(1))
    );
    assert_eq!(fixture.database(&db).await["rows"][4]["values"], json!({}));

    // A type change derives the config; sending one with it is refused, as is a second title.
    let (status, body) = fixture
        .call(
            "PATCH",
            &format!("databases/{db}/properties/{text}"),
            Some(json!({"type": "select", "config": {"options": []}})),
        )
        .await;
    assert_eq!(
        (status, body["detail"].as_str()),
        (StatusCode::UNPROCESSABLE_ENTITY, Some("config"))
    );
    let name = id_of(property(&fixture.database(&db).await, "Name")).to_owned();
    let (status, body) = fixture
        .call(
            "PATCH",
            &format!("databases/{db}/properties/{name}"),
            Some(json!({"type": "text"})),
        )
        .await;
    assert_eq!(
        (status, body["code"].as_str()),
        (StatusCode::CONFLICT, Some("database_title_property"))
    );
    let (status, _) = fixture
        .call(
            "POST",
            &format!("databases/{db}/properties"),
            Some(json!({"name": "Second title", "type": "title"})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn a_dry_run_type_change_reports_the_counts_and_changes_nothing() {
    let fixture = Fixture::new().await;
    let db = fixture.create_database("Dry run").await;
    let raw = id_of(
        &fixture
            .add_property(&db, json!({"name": "Raw", "type": "text"}))
            .await,
    )
    .to_owned();
    for value in ["12", "abc", "7.5"] {
        fixture
            .add_row(&db, json!({"title": value, "values": {&raw: value}}))
            .await;
    }
    let before = fixture.database(&db).await;
    let audit_count = || async {
        sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM audit_events WHERE resource_id = $1")
            .bind(&db)
            .fetch_one(fixture.database.pool())
            .await
            .unwrap()
            .0
    };
    let audited = audit_count().await;

    let preview = fixture
        .ok(
            "PATCH",
            &format!("databases/{db}/properties/{raw}"),
            Some(json!({"type": "number", "dry_run": true})),
        )
        .await;
    assert_eq!(
        (
            preview["property"]["type"].as_str(),
            preview["converted"].as_u64(),
            preview["cleared"].as_u64()
        ),
        (Some("number"), Some(2), Some(1))
    );
    assert_eq!(fixture.database(&db).await, before);
    assert_eq!(audit_count().await, audited);

    // A lossless change previews no cleared values.
    let preview = fixture
        .ok(
            "PATCH",
            &format!("databases/{db}/properties/{raw}"),
            Some(json!({"type": "select", "dry_run": true})),
        )
        .await;
    assert_eq!(
        (preview["converted"].as_u64(), preview["cleared"].as_u64()),
        (Some(3), Some(0))
    );
    assert_eq!(fixture.database(&db).await, before);
}

#[tokio::test]
async fn duplicating_a_property_copies_its_options_and_values_next_to_it() {
    let fixture = Fixture::new().await;
    let db = fixture.create_database("Copies").await;
    let stage = fixture
        .add_property(
            &db,
            json!({"name": "Stage", "type": "select", "config": {"options": [{"name": "Idea"}, {"name": "Live", "color": "green"}]}}),
        )
        .await;
    let stage_id = id_of(&stage).to_owned();
    let notes = id_of(
        &fixture
            .add_property(&db, json!({"name": "Notes", "type": "text"}))
            .await,
    )
    .to_owned();
    let live = option_id(&stage, "Live");
    let row = fixture
        .add_row(
            &db,
            json!({"title": "A", "values": {&stage_id: live, &notes: "hello"}}),
        )
        .await;
    fixture
        .ok(
            "POST",
            &format!("databases/{db}/views"),
            Some(json!({"name": "Second", "type": "table"})),
        )
        .await;

    let (status, _) = fixture
        .call(
            "POST",
            &format!("databases/{db}/properties"),
            Some(json!({"duplicate_of": stage_id, "type": "text"})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let title = id_of(property(&fixture.database(&db).await, "Name")).to_owned();
    let (status, _) = fixture
        .call(
            "POST",
            &format!("databases/{db}/properties"),
            Some(json!({"duplicate_of": title})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    let copy = fixture
        .add_property(&db, json!({"duplicate_of": stage_id}))
        .await;
    let copy_id = id_of(&copy).to_owned();
    assert_eq!(
        (copy["name"].as_str(), copy["type"].as_str()),
        (Some("Stage (copy)"), Some("select"))
    );
    assert_eq!(
        copy["position"].as_i64(),
        Some(stage["position"].as_i64().unwrap() + 1)
    );
    let copied_live = option_id(&copy, "Live");
    assert_ne!(copied_live, live);
    assert_eq!(
        names(&copy["config"]["options"]),
        names(&stage["config"]["options"])
    );

    let database = fixture.database(&db).await;
    assert_eq!(
        names(&database["properties"]),
        ["Name", "Tags", "Stage", "Stage (copy)", "Notes"]
    );
    let stored = database["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == row["id"])
        .unwrap()["values"]
        .clone();
    assert_eq!(stored[&copy_id], json!(copied_live));
    assert_eq!(stored[&stage_id], json!(live));
    for view in database["views"].as_array().unwrap() {
        let columns: Vec<&str> = view["config"]["columns"]
            .as_array()
            .unwrap()
            .iter()
            .map(|column| column["property_id"].as_str().unwrap())
            .collect();
        let at = columns.iter().position(|id| *id == stage_id).unwrap();
        assert_eq!(columns[at + 1], copy_id, "view {}", view["name"]);
    }

    let named = fixture
        .add_property(&db, json!({"duplicate_of": notes, "name": "More notes"}))
        .await;
    assert_eq!(named["name"], "More notes");
    let database = fixture.database(&db).await;
    let stored = &database["rows"][0]["values"];
    assert_eq!(stored[id_of(&named)], json!("hello"));
}

#[tokio::test]
async fn removing_options_and_properties_cleans_values_and_views() {
    let fixture = Fixture::new().await;
    let db = fixture.create_database("Cleanup").await;
    let database = fixture.database(&db).await;
    let tags = id_of(property(&database, "Tags")).to_owned();
    let view = id_of(&database["views"][0]).to_owned();
    let update = fixture
        .ok(
            "PATCH",
            &format!("databases/{db}/properties/{tags}"),
            Some(
                json!({"config": {"options": [{"name": "AI"}, {"name": "Rust", "color": "red"}]}}),
            ),
        )
        .await;
    let ai = option_id(&update["property"], "AI");
    let rust = option_id(&update["property"], "Rust");
    let only_ai = fixture
        .add_row(&db, json!({"title": "A", "values": {&tags: [ai]}}))
        .await;
    let both = fixture
        .add_row(&db, json!({"title": "B", "values": {&tags: [ai, rust]}}))
        .await;

    // Removing "AI" clears the row that had only it and keeps "Rust" on the other.
    let update = fixture
        .ok(
            "PATCH",
            &format!("databases/{db}/properties/{tags}"),
            Some(json!({"config": {"options": [{"id": rust, "name": "Rust", "color": "red"}]}})),
        )
        .await;
    assert_eq!(update["cleared"], 1);
    let database = fixture.database(&db).await;
    let values = |row: &Value| {
        database["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["id"] == row["id"])
            .unwrap()["values"]
            .clone()
    };
    assert_eq!(values(&only_ai), json!({}));
    assert_eq!(values(&both), json!({&tags: [rust]}));

    // A view that sorts, filters and shows the property drops it when the property goes.
    let name = id_of(property(&database, "Name")).to_owned();
    let saved = fixture
        .ok(
            "PATCH",
            &format!("databases/{db}/views/{view}"),
            Some(json!({"config": {
                "columns": [{"property_id": tags, "width": 120}, {"property_id": name}],
                "sorts": [{"property_id": tags, "direction": "asc"}, {"property_id": name, "direction": "desc"}],
                "filter": {"op": "and", "children": [
                    {"property_id": tags, "operator": "contains", "value": [rust]},
                    {"property_id": name, "operator": "contains", "value": "B"},
                    {"property_id": Id::new_v7().to_string(), "operator": "is_empty"}
                ]},
            }})),
        )
        .await;
    assert_eq!(
        saved["config"]["filter"]["children"]
            .as_array()
            .unwrap()
            .len(),
        2,
        "unknown property dropped"
    );
    fixture
        .ok("DELETE", &format!("databases/{db}/properties/{tags}"), None)
        .await;
    let database = fixture.database(&db).await;
    assert_eq!(names(&database["properties"]), ["Name"]);
    assert_eq!(
        database["views"][0]["config"],
        json!({
            "columns": [{"property_id": name, "width": 200, "hidden": false}],
            "sorts": [{"property_id": name, "direction": "desc"}],
            "filter": {"op": "and", "children": [{"property_id": name, "operator": "contains", "value": "B"}]},
            "wrap_cells": false,
        })
    );
    assert_eq!(database["rows"][1]["values"], json!({}));
    let stored: i64 = fixture
        .database
        .scalar(&format!(
            "SELECT COUNT(*) FROM page_property_values WHERE property_id = '{tags}'"
        ))
        .await
        .unwrap();
    assert_eq!(stored, 0);
    let (status, body) = fixture
        .call("DELETE", &format!("databases/{db}/properties/{name}"), None)
        .await;
    assert_eq!(
        (status, body["code"].as_str()),
        (StatusCode::CONFLICT, Some("database_title_property"))
    );

    // Bad view configs name their field; the last view stays.
    for (config, field) in [
        (
            json!({"columns": [{"property_id": name, "width": 20}]}),
            "config.columns[0].width",
        ),
        (
            json!({"filter": {"op": "and", "children": [{"property_id": name, "operator": ">", "value": 1}]}}),
            "config.filter.children[0].operator",
        ),
        (
            json!({"filter": {"op": "and", "children": [{"property_id": name, "operator": "contains", "value": 1}]}}),
            "config.filter.children[0].value",
        ),
        (
            json!({"sorts": (0..11).map(|_| json!({"property_id": name, "direction": "asc"})).collect::<Vec<_>>()}),
            "config.sorts",
        ),
        (
            json!({"filter": {"op": "and", "children": [{"property_id": name, "operator": "nope"}]}}),
            "config",
        ),
    ] {
        let (status, body) = fixture
            .call(
                "PATCH",
                &format!("databases/{db}/views/{view}"),
                Some(json!({"config": config})),
            )
            .await;
        assert_eq!(
            (status, body["detail"].as_str()),
            (StatusCode::UNPROCESSABLE_ENTITY, Some(field)),
            "{body}"
        );
    }
    let (status, body) = fixture
        .call("DELETE", &format!("databases/{db}/views/{view}"), None)
        .await;
    assert_eq!(
        (status, body["code"].as_str()),
        (StatusCode::CONFLICT, Some("database_last_view"))
    );
    let copy = fixture
        .ok(
            "POST",
            &format!("databases/{db}/views"),
            Some(json!({"name": "Copy", "type": "table", "duplicate_of": view})),
        )
        .await;
    assert_eq!(copy["config"], database["views"][0]["config"]);
    assert_eq!(copy["position"], 1);
    fixture
        .ok("DELETE", &format!("databases/{db}/views/{view}"), None)
        .await;
    assert_eq!(names(&fixture.database(&db).await["views"]), ["Copy"]);
}

/// Inserts `count` live rows into the database with SQL (the API would take minutes); returns
/// their ids.
async fn seed_rows(fixture: &Fixture, db: &str, count: i64) -> Vec<String> {
    let teamspace: String = fixture
        .database
        .scalar(&format!("SELECT teamspace_id FROM pages WHERE id = '{db}'"))
        .await
        .unwrap();
    let now = TimestampMillis::now().as_millis();
    let mut tx = fixture.database.pool().begin().await.unwrap();
    let mut ids = Vec::new();
    for position in 0..count {
        let id = Id::new_v7().to_string();
        sqlx::query(
            "INSERT INTO pages (id, workspace_id, parent_id, teamspace_id, title, position, creator_id, updated_by, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, 'Row', $5, $6, $7, $8, $9)",
        )
        .bind(&id)
        .bind(&fixture.workspace_id)
        .bind(db)
        .bind(&teamspace)
        .bind(position)
        .bind(fixture.owner_id.to_string())
        .bind(fixture.owner_id.to_string())
        .bind(now)
        .bind(now)
        .execute(&mut *tx)
        .await
        .unwrap();
        ids.push(id);
    }
    tx.commit().await.unwrap();
    ids
}

fn assert_row_limit((status, body): (StatusCode, Value)) {
    assert_eq!(
        (status, body["code"].as_str()),
        (StatusCode::CONFLICT, Some("database_row_limit")),
        "{body}"
    );
}

#[tokio::test]
async fn a_database_holds_at_most_5000_live_rows() {
    let fixture = Fixture::new().await;
    let db = fixture.create_database("Big").await;
    let rows = seed_rows(&fixture, &db, 5_000).await;
    assert_row_limit(
        fixture
            .call("POST", &format!("databases/{db}/rows"), Some(json!({})))
            .await,
    );

    fixture
        .ok(
            "POST",
            &format!("databases/{db}/rows/trash"),
            Some(json!({"row_ids": [rows[0]]})),
        )
        .await;
    let (status, body) = fixture
        .call(
            "POST",
            &format!("databases/{db}/rows"),
            Some(json!({"after_row_id": null})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["position"], 0);
}

#[tokio::test]
async fn restoring_or_duplicating_a_row_of_a_full_database_is_refused() {
    let fixture = Fixture::new().await;
    let db = fixture.create_database("Big").await;
    let trashed = fixture.add_row(&db, json!({})).await;
    let trashed = id_of(&trashed).to_owned();
    fixture.trash_page(&trashed).await;
    let rows = seed_rows(&fixture, &db, 5_000).await;

    let (_, trash) = fixture.call("GET", "pages/trash", None).await;
    let version = trash["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == trashed.as_str())
        .expect("in the trash")["version"]
        .clone();
    assert_row_limit(
        fixture
            .call(
                "POST",
                &format!("pages/{trashed}/restore"),
                Some(json!({"expected_version": version})),
            )
            .await,
    );
    for include_children in [false, true] {
        assert_row_limit(
            fixture
                .call(
                    "POST",
                    &format!("pages/{}/duplicate", rows[0]),
                    Some(json!({"include_children": include_children})),
                )
                .await,
        );
    }
    let live: i64 = fixture
        .database
        .scalar(&format!(
            "SELECT CAST(COUNT(*) AS BIGINT) FROM pages WHERE parent_id = '{db}' AND deleted_at IS NULL"
        ))
        .await
        .unwrap();
    assert_eq!(live, 5_000);

    // With room again, the row comes back.
    fixture
        .ok(
            "POST",
            &format!("databases/{db}/rows/trash"),
            Some(json!({"row_ids": [rows[0]]})),
        )
        .await;
    fixture.restore_page(&trashed).await;
}

#[tokio::test]
async fn the_lock_freezes_properties_and_views_but_not_rows_or_values() {
    let fixture = Fixture::new().await;
    let db = fixture.create_database("Locked").await;
    let database = fixture.database(&db).await;
    let tags = id_of(property(&database, "Tags")).to_owned();
    let view = id_of(&database["views"][0]).to_owned();
    fixture
        .ok(
            "POST",
            &format!("pages/{db}/lock"),
            Some(json!({"locked": true})),
        )
        .await;
    assert_eq!(fixture.database(&db).await["database"]["locked"], true);

    for (method, path, body) in [
        (
            "POST",
            format!("databases/{db}/properties"),
            Some(json!({"name": "Notes", "type": "text"})),
        ),
        (
            "PATCH",
            format!("databases/{db}/properties/{tags}"),
            Some(json!({"name": "Labels"})),
        ),
        ("DELETE", format!("databases/{db}/properties/{tags}"), None),
        (
            "POST",
            format!("databases/{db}/views"),
            Some(json!({"name": "Second", "type": "table"})),
        ),
        (
            "PATCH",
            format!("databases/{db}/views/{view}"),
            Some(json!({"name": "Renamed"})),
        ),
        ("DELETE", format!("databases/{db}/views/{view}"), None),
    ] {
        let (status, body) = fixture.call(method, &path, body).await;
        assert_eq!(
            (status, body["code"].as_str()),
            (StatusCode::LOCKED, Some("page_locked")),
            "{method} {path}"
        );
    }

    let row = fixture
        .add_row(&db, json!({"title": "Still allowed"}))
        .await;
    let (status, body) = fixture
        .set_values(&db, id_of(&row), json!({&tags: []}))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let other = fixture.add_row(&db, json!({"title": "Second"})).await;
    let moved = fixture
        .ok(
            "POST",
            &format!("databases/{db}/rows/{}/move", id_of(&row)),
            Some(json!({"after_row_id": id_of(&other)})),
        )
        .await;
    assert_eq!(moved["position"], 1);
    assert_eq!(
        names(&fixture.database(&db).await["rows"]),
        ["Second", "Still allowed"]
    );

    fixture
        .ok(
            "POST",
            &format!("pages/{db}/lock"),
            Some(json!({"locked": false})),
        )
        .await;
    fixture
        .add_property(&db, json!({"name": "Notes", "type": "text"}))
        .await;
}

#[tokio::test]
async fn favorites_carry_what_the_sidebar_shows_for_rows_and_their_sub_pages() {
    let fixture = Fixture::new().await;
    let db = fixture.create_database("Research").await;
    let row = fixture
        .add_row(&db, json!({"title": "Paper A", "icon": "📄"}))
        .await;
    let row = id_of(&row).to_owned();
    let sub = fixture
        .create_page(json!({"parent_id": row, "title": "Notes"}))
        .await;
    let sub = id_of(&sub).to_owned();
    let page = fixture.create_page(json!({"title": "Plan"})).await;
    let page = id_of(&page).to_owned();
    for id in [&row, &sub, &page, &db] {
        fixture
            .ok("PUT", &format!("pages/{id}/favorite"), None)
            .await;
    }
    let favorite = |id: &str,
                    position: i64,
                    title: &str,
                    icon: Value,
                    kind: &str,
                    database: Value,
                    database_title: Value| {
        json!({"page_id": id, "position": position, "title": title, "icon": icon, "kind": kind,
               "database_id": database, "database_title": database_title})
    };
    assert_eq!(
        fixture.ok("GET", "pages/favorites", None).await,
        json!({"items": [
            favorite(&row, 0, "Paper A", json!("📄"), "page", json!(db), json!("Research")),
            favorite(&sub, 1, "Notes", Value::Null, "page", Value::Null, Value::Null),
            favorite(&page, 2, "Plan", Value::Null, "page", Value::Null, Value::Null),
            favorite(&db, 3, "Research", Value::Null, "database", Value::Null, Value::Null),
        ]})
    );
    // Adding returns the same entry.
    assert_eq!(
        fixture
            .ok("PUT", &format!("pages/{row}/favorite"), None)
            .await,
        favorite(
            &row,
            0,
            "Paper A",
            json!("📄"),
            "page",
            json!(db),
            json!("Research")
        )
    );

    // The favorites of a trashed row (and of its sub-pages) stay hidden; reordering skips them.
    fixture.trash_page(&row).await;
    let ids = |list: Value| {
        list["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["page_id"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        ids(fixture.ok("GET", "pages/favorites", None).await),
        [page.clone(), db.clone()]
    );
    let moved = fixture
        .ok(
            "POST",
            &format!("pages/favorites/{db}/move"),
            Some(json!({"position": 0})),
        )
        .await;
    assert_eq!(ids(moved), [db.clone(), page.clone()]);
    fixture.restore_page(&row).await;
    assert_eq!(
        ids(fixture.ok("GET", "pages/favorites", None).await),
        [row, sub, db, page]
    );
}

#[tokio::test]
async fn a_row_lists_its_live_sub_pages_and_copies_them() {
    let fixture = Fixture::new().await;
    let db = fixture.create_database("Research").await;
    let row = fixture.add_row(&db, json!({"title": "Paper A"})).await;
    let row = id_of(&row).to_owned();
    let notes = fixture
        .create_page(json!({"parent_id": row, "title": "Notes"}))
        .await;
    let notes = id_of(&notes).to_owned();
    let deep = fixture
        .create_page(json!({"parent_id": notes, "title": "Deep"}))
        .await;
    let deep = id_of(&deep).to_owned();
    let nested = fixture
        .create_page(json!({"parent_id": row, "kind": "database", "title": "Reading list"}))
        .await;
    let nested = id_of(&nested).to_owned();
    fixture
        .add_row(&nested, json!({"title": "Nested row"}))
        .await;
    let gone = fixture
        .create_page(json!({"parent_id": row, "title": "Gone"}))
        .await;
    fixture.trash_page(id_of(&gone)).await;

    let titles = |list: Value| {
        list["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|page| page["title"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>()
    };
    // Parents first; a nested database counts, its rows (its content) do not.
    assert_eq!(
        titles(
            fixture
                .ok("GET", &format!("pages/{row}/descendants"), None)
                .await
        ),
        ["Notes", "Reading list", "Deep"]
    );
    assert_eq!(
        titles(
            fixture
                .ok("GET", &format!("pages/{notes}/descendants"), None)
                .await
        ),
        ["Deep"]
    );
    for leaf in [&deep, &db] {
        assert_eq!(
            fixture
                .ok("GET", &format!("pages/{leaf}/descendants"), None)
                .await,
            json!({"items": []})
        );
    }
    let (status, body) = fixture
        .call("GET", &format!("pages/{}/descendants", id_of(&gone)), None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    // "Duplicate with sub-pages" copies the row's sub-pages under the new row.
    let copy = fixture
        .ok(
            "POST",
            &format!("pages/{row}/duplicate"),
            Some(json!({"include_children": true})),
        )
        .await;
    assert_eq!(copy["database_id"].as_str(), Some(db.as_str()));
    assert_eq!(
        titles(
            fixture
                .ok("GET", &format!("pages/{}/descendants", id_of(&copy)), None)
                .await
        ),
        ["Notes", "Reading list", "Deep"]
    );
}

#[tokio::test]
async fn rows_stay_out_of_the_tree_and_out_of_page_moves() {
    let fixture = Fixture::new().await;
    let db = fixture.create_database("Inbox").await;
    let row = fixture
        .add_row(&db, json!({"title": "Row", "icon": "📄"}))
        .await;
    let row_id = id_of(&row);
    let child = fixture
        .create_page(json!({"parent_id": row_id, "title": "Row child"}))
        .await;
    let other = fixture.create_page(json!({"title": "Other"})).await;

    let (_, tree) = fixture.call("GET", "pages", None).await;
    assert_eq!(names(&tree["items"]), ["Inbox", "Other"]);
    let row_page = fixture.page(row_id).await;
    assert_eq!(row_page["database_id"], db.as_str());
    assert_eq!(row_page["kind"], "page");
    assert_eq!(
        fixture.page(id_of(&child)).await["database_id"],
        Value::Null
    );

    let refused = |status: StatusCode, body: &Value| {
        assert_eq!(
            (status, body["code"].as_str()),
            (StatusCode::CONFLICT, Some("database_move_unsupported")),
            "{body}"
        );
    };
    let (status, body) = fixture
        .call(
            "POST",
            &format!("pages/{row_id}/move"),
            Some(
                json!({"expected_version": row_page["version"], "parent_id": null, "position": 0}),
            ),
        )
        .await;
    refused(status, &body);
    let (status, body) = fixture
        .call(
            "POST",
            &format!("pages/{}/move", id_of(&other)),
            Some(json!({"expected_version": 0, "parent_id": db, "position": 0})),
        )
        .await;
    refused(status, &body);
    let (status, body) = fixture
        .call(
            "POST",
            "pages",
            Some(json!({"parent_id": db, "title": "Sneaky row"})),
        )
        .await;
    refused(status, &body);

    // The database page itself still moves.
    let version = fixture.page(&db).await["version"].clone();
    fixture
        .ok(
            "POST",
            &format!("pages/{db}/move"),
            Some(json!({"expected_version": version, "parent_id": id_of(&other), "position": 0})),
        )
        .await;
    assert_eq!(fixture.database(&db).await["rows"][0]["icon"], "📄");
}

#[tokio::test]
async fn duplicating_copies_properties_views_rows_and_values_with_new_ids() {
    let fixture = Fixture::new().await;
    let db = fixture.create_database("Original").await;
    let database = fixture.database(&db).await;
    let tags = id_of(property(&database, "Tags")).to_owned();
    let update = fixture
        .ok(
            "PATCH",
            &format!("databases/{db}/properties/{tags}"),
            Some(json!({"config": {"options": [{"id": "ai", "name": "AI"}]}})),
        )
        .await;
    assert_eq!(update["property"]["config"]["options"][0]["id"], "ai");
    let notes = id_of(
        &fixture
            .add_property(&db, json!({"name": "Notes", "type": "text"}))
            .await,
    )
    .to_owned();
    let view = id_of(&database["views"][0]).to_owned();
    fixture
        .ok(
            "PATCH",
            &format!("databases/{db}/views/{view}"),
            Some(json!({"config": {
                "columns": [
                    {"property_id": property(&database, "Name")["id"]},
                    {"property_id": tags},
                    {"property_id": notes},
                ],
                "sorts": [{"property_id": notes, "direction": "asc"}],
            }})),
        )
        .await;
    let row = fixture
        .add_row(
            &db,
            json!({"title": "Paper", "values": {&tags: ["ai"], &notes: "read"}}),
        )
        .await;
    fixture.add_row(&db, json!({"title": "Empty"})).await;

    let copy = fixture
        .ok("POST", &format!("pages/{db}/duplicate"), Some(json!({})))
        .await;
    assert_eq!(
        (copy["kind"].as_str(), copy["title"].as_str()),
        (Some("database"), Some("Original (copy)"))
    );
    let copied = fixture.database(id_of(&copy)).await;
    let original = fixture.database(&db).await;
    assert_eq!(names(&copied["properties"]), ["Name", "Tags", "Notes"]);
    let copied_tags = id_of(property(&copied, "Tags")).to_owned();
    let copied_notes = id_of(property(&copied, "Notes")).to_owned();
    assert_ne!(copied_tags, tags);
    assert_ne!(copied_notes, notes);
    assert_eq!(
        property(&copied, "Tags")["config"],
        property(&original, "Tags")["config"]
    );
    assert_ne!(id_of(&copied["views"][0]), view);
    assert_eq!(
        copied["views"][0]["config"]["sorts"],
        json!([{"property_id": copied_notes, "direction": "asc"}])
    );
    assert_eq!(
        copied["views"][0]["config"]["columns"][2]["property_id"],
        copied_notes.as_str()
    );
    assert_eq!(names(&copied["rows"]), ["Paper", "Empty"]);
    assert_ne!(id_of(&copied["rows"][0]), id_of(&row));
    assert_eq!(
        copied["rows"][0]["values"],
        json!({&copied_tags: ["ai"], &copied_notes: "read"})
    );
    assert_eq!(
        original["rows"][0]["values"],
        json!({&tags: ["ai"], &notes: "read"})
    );
    let (_, tree) = fixture.call("GET", "pages", None).await;
    assert_eq!(names(&tree["items"]), ["Original", "Original (copy)"]);

    // A duplicated row stays in its database with its values.
    let row_copy = fixture
        .ok(
            "POST",
            &format!("pages/{}/duplicate", id_of(&row)),
            Some(json!({})),
        )
        .await;
    assert_eq!(row_copy["database_id"], db.as_str());
    let original = fixture.database(&db).await;
    assert_eq!(names(&original["rows"]), ["Paper", "Paper (copy)", "Empty"]);
    assert_eq!(
        original["rows"][1]["values"],
        json!({&tags: ["ai"], &notes: "read"})
    );
}

#[tokio::test]
async fn trash_and_restore_bring_back_rows_and_values() {
    let fixture = Fixture::new().await;
    let db = fixture.create_database("Restorable").await;
    let tags = id_of(property(&fixture.database(&db).await, "Tags")).to_owned();
    fixture
        .ok(
            "PATCH",
            &format!("databases/{db}/properties/{tags}"),
            Some(json!({"config": {"options": [{"id": "a", "name": "A"}]}})),
        )
        .await;
    let first = fixture
        .add_row(&db, json!({"title": "First", "values": {&tags: ["a"]}}))
        .await;
    let second = fixture.add_row(&db, json!({"title": "Second"})).await;
    let third = fixture.add_row(&db, json!({"title": "Third"})).await;

    fixture.trash_page(&db).await;
    let (status, _) = fixture.call("GET", &format!("databases/{db}"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    fixture.restore_page(&db).await;
    let database = fixture.database(&db).await;
    assert_eq!(names(&database["rows"]), ["First", "Second", "Third"]);
    assert_eq!(database["rows"][0]["values"], json!({&tags: ["a"]}));

    // Bulk trash: one trash entry per row; unknown ids are skipped.
    fixture
        .ok(
            "POST",
            &format!("databases/{db}/rows/trash"),
            Some(json!({"row_ids": [id_of(&first), id_of(&third), Id::new_v7().to_string()]})),
        )
        .await;
    let database = fixture.database(&db).await;
    assert_eq!(names(&database["rows"]), ["Second"]);
    assert_eq!(database["database"]["row_count"], 1);
    let (_, trash) = fixture.call("GET", "pages/trash", None).await;
    let mut trashed = names(&trash["items"]);
    trashed.sort_unstable();
    assert_eq!(trashed, ["First", "Third"]);
    fixture.restore_page(id_of(&first)).await;
    let database = fixture.database(&db).await;
    assert_eq!(names(&database["rows"]), ["First", "Second"]);
    assert_eq!(database["rows"][0]["values"], json!({&tags: ["a"]}));
    assert_eq!(id_of(&database["rows"][1]), id_of(&second));
}

#[tokio::test]
async fn a_row_trashed_alone_restores_into_its_database_wherever_it_moved() {
    let fixture = Fixture::new().await;
    let db = fixture.create_database("Movable").await;
    let tags = id_of(property(&fixture.database(&db).await, "Tags")).to_owned();
    fixture
        .ok(
            "PATCH",
            &format!("databases/{db}/properties/{tags}"),
            Some(json!({"config": {"options": [{"id": "a", "name": "A"}]}})),
        )
        .await;
    let row = fixture
        .add_row(&db, json!({"title": "Row", "values": {&tags: ["a"]}}))
        .await;
    let row_id = id_of(&row);
    fixture.trash_page(row_id).await;

    let version = fixture.page(&db).await["version"].clone();
    fixture
        .ok(
            "POST",
            &format!("pages/{db}/move"),
            Some(json!({"expected_version": version, "parent_id": null, "private": true, "position": 0})),
        )
        .await;
    fixture.restore_page(row_id).await;
    let restored = fixture.page(row_id).await;
    assert_eq!(
        (
            restored["parent_id"].as_str(),
            restored["database_id"].as_str(),
            restored["private"].as_bool()
        ),
        (Some(db.as_str()), Some(db.as_str()), Some(true)),
        "{restored}"
    );
    let database = fixture.database(&db).await;
    assert_eq!(names(&database["rows"]), ["Row"]);
    assert_eq!(database["rows"][0]["values"], json!({&tags: ["a"]}));
    let (_, tree) = fixture.call("GET", "pages", None).await;
    assert_eq!(names(&tree["items"]), ["Movable"]);

    // The row cannot come back while its database is in the trash.
    fixture.trash_page(row_id).await;
    fixture.trash_page(&db).await;
    let (_, trash) = fixture.call("GET", "pages/trash", None).await;
    let row_version = trash["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == row_id)
        .unwrap()["version"]
        .clone();
    let (status, body) = fixture
        .call(
            "POST",
            &format!("pages/{row_id}/restore"),
            Some(json!({"expected_version": row_version})),
        )
        .await;
    assert_eq!(
        (status, body["code"].as_str()),
        (StatusCode::CONFLICT, Some("database_trashed")),
        "{body}"
    );
    fixture.restore_page(&db).await;
    assert_eq!(
        names(&fixture.database(&db).await["rows"]),
        Vec::<&str>::new()
    );
    fixture.restore_page(row_id).await;
    assert_eq!(names(&fixture.database(&db).await["rows"]), ["Row"]);

    // Once the database is deleted forever, the row has nothing to rejoin: it comes back as a
    // plain page at the top level of its space.
    fixture.trash_page(row_id).await;
    fixture.trash_page(&db).await;
    let (_, trash) = fixture.call("GET", "pages/trash", None).await;
    let db_version = trash["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == db.as_str())
        .unwrap()["version"]
        .clone();
    fixture
        .ok(
            "DELETE",
            &format!("pages/{db}/permanent?expected_version={db_version}"),
            None,
        )
        .await;
    fixture.restore_page(row_id).await;
    let orphan = fixture.page(row_id).await;
    assert_eq!(
        (
            orphan["parent_id"].as_str(),
            orphan["database_id"].as_str(),
            orphan["private"].as_bool()
        ),
        (None, None, Some(true)),
        "{orphan}"
    );
    let (_, tree) = fixture.call("GET", "pages", None).await;
    assert_eq!(names(&tree["items"]), ["Row"]);
}

#[tokio::test]
async fn a_private_database_is_not_found_for_other_members() {
    let fixture = Fixture::new().await;
    let (_, member) = fixture.add_member("member@example.com").await;
    let page = fixture
        .create_page(json!({"kind": "database", "title": "Diary", "private": true}))
        .await;
    let db = id_of(&page).to_owned();
    let database = fixture.database(&db).await;
    let tags = id_of(property(&database, "Tags")).to_owned();
    let view = id_of(&database["views"][0]).to_owned();
    let row = id_of(&fixture.add_row(&db, json!({"title": "Entry"})).await).to_owned();

    for (method, path, body) in [
        ("GET", format!("databases/{db}"), None),
        (
            "POST",
            format!("databases/{db}/properties"),
            Some(json!({"name": "N", "type": "text"})),
        ),
        (
            "PATCH",
            format!("databases/{db}/properties/{tags}"),
            Some(json!({"name": "N"})),
        ),
        ("DELETE", format!("databases/{db}/properties/{tags}"), None),
        (
            "POST",
            format!("databases/{db}/views"),
            Some(json!({"name": "V", "type": "table"})),
        ),
        (
            "PATCH",
            format!("databases/{db}/views/{view}"),
            Some(json!({"name": "V"})),
        ),
        ("DELETE", format!("databases/{db}/views/{view}"), None),
        ("POST", format!("databases/{db}/rows"), Some(json!({}))),
        (
            "PATCH",
            format!("databases/{db}/rows/{row}/values"),
            Some(json!({&tags: []})),
        ),
        (
            "POST",
            format!("databases/{db}/rows/trash"),
            Some(json!({"row_ids": [row]})),
        ),
        (
            "POST",
            format!("databases/{db}/rows/{row}/move"),
            Some(json!({"after_row_id": null})),
        ),
    ] {
        let (status, body) = fixture.call_as(&member, method, &path, body).await;
        assert_eq!(
            (status, body["code"].as_str()),
            (StatusCode::NOT_FOUND, Some("page_not_found")),
            "{method} {path}"
        );
    }
    assert_eq!(
        fixture.database(&db).await["rows"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn database_writes_are_audited_and_emit_realtime_events() {
    let fixture = Fixture::new().await;
    let db = fixture.create_database("Audited").await;
    let tags = id_of(property(&fixture.database(&db).await, "Tags")).to_owned();
    let row = fixture.add_row(&db, json!({"title": "Row"})).await;
    fixture
        .set_values(&db, id_of(&row), json!({&tags: []}))
        .await;
    fixture
        .ok(
            "POST",
            &format!("databases/{db}/properties"),
            Some(json!({"name": "Notes", "type": "text"})),
        )
        .await;

    let rows = sqlx::query_as::<_, (String, i64)>(
        "SELECT audit_events.action, \
         (SELECT COUNT(*) FROM outbox_events WHERE outbox_events.id = audit_events.id AND outbox_events.topic = 'workspace.changed') \
         FROM audit_events WHERE workspace_id = $1 AND resource_type = 'database' AND resource_id = $2 \
         ORDER BY occurred_at, id",
    )
    .bind(&fixture.workspace_id)
    .bind(&db)
    .fetch_all(fixture.database.pool())
    .await
    .unwrap();
    assert_eq!(
        rows,
        [
            ("database.row_created".to_owned(), 1),
            ("database.values_updated".to_owned(), 1),
            ("database.property_created".to_owned(), 1),
        ]
    );
}

#[tokio::test]
async fn board_views_check_and_repair_their_grouping() {
    let fixture = Fixture::new().await;
    let db = fixture.create_database("Board").await;
    let database = fixture.database(&db).await;
    let tags = id_of(property(&database, "Tags")).to_owned();
    let name = id_of(property(&database, "Name")).to_owned();

    let board = fixture
        .ok(
            "POST",
            &format!("databases/{db}/views"),
            Some(json!({"name": "Board", "type": "board"})),
        )
        .await;
    assert_eq!(board["type"], "board");
    assert_eq!(
        board["config"].get("group_by"),
        None,
        "no select or status property to group by"
    );
    let view = id_of(&board).to_owned();
    let (status, body) = fixture
        .call(
            "POST",
            &format!("databases/{db}/views"),
            Some(json!({"name": "Gallery", "type": "gallery"})),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    let priority = fixture
        .add_property(
            &db,
            json!({"name": "Priority", "type": "select", "config": {"options": [{"name": "High"}, {"name": "Low"}]}}),
        )
        .await;
    let priority_id = id_of(&priority).to_owned();
    let high = option_id(&priority, "High");
    let low = option_id(&priority, "Low");
    let state = fixture
        .add_property(&db, json!({"name": "State", "type": "status"}))
        .await;
    let state_id = id_of(&state).to_owned();
    let views = fixture.database(&db).await["views"].clone();
    assert_eq!(
        views[1]["config"]["group_by"], state_id,
        "a board without a group property groups by the first status"
    );

    let (status, body) = fixture
        .call(
            "PATCH",
            &format!("databases/{db}/views/{view}"),
            Some(json!({"config": {"group_by": tags}})),
        )
        .await;
    assert_eq!(
        (status, body["detail"].as_str()),
        (StatusCode::UNPROCESSABLE_ENTITY, Some("config.group_by")),
        "a multi-select cannot group a board"
    );

    let saved = fixture
        .ok(
            "PATCH",
            &format!("databases/{db}/views/{view}"),
            Some(json!({"config": {
                "group_by": priority_id,
                "group_order": [low, null, "bogus", high, low],
                "hidden_groups": [null, "bogus"],
                "card_properties": [tags, name, tags, Id::new_v7().to_string()],
            }})),
        )
        .await;
    assert_eq!(
        saved["config"],
        json!({
            "columns": [], "sorts": [], "filter": {"op": "and", "children": []}, "wrap_cells": false,
            "group_by": priority_id, "group_order": [low, null, high], "hidden_groups": [null],
            "card_properties": [tags],
        }),
        "unknown groups, repeats, the title and unknown properties are dropped"
    );

    let copy = fixture
        .ok(
            "POST",
            &format!("databases/{db}/views"),
            Some(json!({"name": "Board 2", "type": "table", "duplicate_of": view})),
        )
        .await;
    assert_eq!(copy["type"], "board", "a copy keeps its source's type");
    assert_eq!(copy["config"], saved["config"]);

    fixture
        .ok(
            "DELETE",
            &format!("databases/{db}/properties/{priority_id}"),
            None,
        )
        .await;
    let views = fixture.database(&db).await["views"].clone();
    assert_eq!(
        views[1]["config"],
        json!({
            "columns": [], "sorts": [], "filter": {"op": "and", "children": []}, "wrap_cells": false,
            "group_by": state_id, "card_properties": [tags],
        }),
        "a deleted group property falls back to the first status, with the default order"
    );
}

#[tokio::test]
async fn moving_a_card_sets_its_group_and_its_place_in_one_request() {
    let fixture = Fixture::new().await;
    let db = fixture.create_database("Cards").await;
    let priority = fixture
        .add_property(
            &db,
            json!({"name": "Priority", "type": "select", "config": {"options": [{"name": "High"}, {"name": "Low"}]}}),
        )
        .await;
    let priority_id = id_of(&priority).to_owned();
    let high = option_id(&priority, "High");
    let low = option_id(&priority, "Low");
    let a = fixture
        .add_row(&db, json!({"title": "A", "values": {&priority_id: high}}))
        .await;
    let b = fixture
        .add_row(&db, json!({"title": "B", "values": {&priority_id: high}}))
        .await;
    let c = fixture
        .add_row(&db, json!({"title": "C", "values": {&priority_id: low}}))
        .await;

    let moved = fixture
        .ok(
            "POST",
            &format!("databases/{db}/rows/{}/move", id_of(&b)),
            Some(json!({"after_row_id": id_of(&c), "values": {&priority_id: low}})),
        )
        .await;
    assert_eq!(moved["values"], json!({&priority_id: low}));
    assert_eq!(moved["position"], 2);
    assert_eq!(names(&fixture.database(&db).await["rows"]), ["A", "C", "B"]);

    let (status, body) = fixture
        .call(
            "POST",
            &format!("databases/{db}/rows/{}/move", id_of(&a)),
            Some(json!({"after_row_id": id_of(&b), "values": {&priority_id: "bogus"}})),
        )
        .await;
    assert_eq!(
        (status, body["detail"].as_str()),
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            Some(format!("values.{priority_id}").as_str())
        )
    );
    let database = fixture.database(&db).await;
    assert_eq!(
        names(&database["rows"]),
        ["A", "C", "B"],
        "a bad value moves nothing"
    );
    assert_eq!(database["rows"][0]["values"], json!({&priority_id: high}));
}

impl Fixture {
    async fn link(&self, db: &str, host: &str) -> Value {
        self.ok(
            "POST",
            &format!("databases/{db}/links"),
            Some(json!({"host_page_id": host})),
        )
        .await
    }

    async fn set_content(&self, page: &str, content: Value) {
        let version = self.page(page).await["version"].as_u64().unwrap();
        self.ok(
            "PATCH",
            &format!("pages/{page}"),
            Some(json!({"expected_version": version, "content": content})),
        )
        .await;
    }

    async fn purge_page(&self, id: &str) {
        self.trash_page(id).await;
        let (_, trash) = self.call("GET", "pages/trash", None).await;
        let version = trash["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["id"] == id)
            .expect("in the trash")["version"]
            .clone();
        self.ok(
            "DELETE",
            &format!("pages/{id}/permanent?expected_version={version}"),
            None,
        )
        .await;
    }

    async fn count(&self, sql: &str, id: &str) -> i64 {
        sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_owned()))
            .bind(id)
            .fetch_one(self.database.pool())
            .await
            .unwrap()
    }
}

#[tokio::test]
async fn a_linked_view_has_its_own_views_and_leaves_the_database_views_alone() {
    let fixture = Fixture::new().await;
    let db = fixture.create_database("Tasks").await;
    let host = id_of(&fixture.create_page(json!({"title": "Host"})).await).to_owned();
    let own = fixture.database(&db).await["views"].clone();
    let own_view = id_of(&own[0]).to_owned();

    let linked = fixture.link(&db, &host).await;
    let link = id_of(&linked["link"]).to_owned();
    assert_eq!(
        (
            linked["link"]["database_id"].as_str(),
            linked["link"]["host_page_id"].as_str()
        ),
        (Some(db.as_str()), Some(host.as_str()))
    );
    assert_eq!(names(&linked["views"]), ["Table"]);
    let link_view = id_of(&linked["views"][0]).to_owned();
    assert_ne!(link_view, own_view);
    assert_eq!(linked["views"][0]["config"], own[0]["config"]);

    // Changes to the link's views stay in the link.
    fixture
        .ok(
            "PATCH",
            &format!("databases/{db}/views/{link_view}?link_id={link}"),
            Some(json!({"name": "Mine", "config": {"wrap_cells": true}})),
        )
        .await;
    fixture
        .ok(
            "POST",
            &format!("databases/{db}/views"),
            Some(json!({"name": "Board", "type": "board", "link_id": link})),
        )
        .await;
    let through_link = fixture
        .ok("GET", &format!("databases/{db}?link={link}"), None)
        .await;
    assert_eq!(names(&through_link["views"]), ["Mine", "Board"]);
    assert_eq!(through_link["views"][0]["config"]["wrap_cells"], true);
    assert_eq!(through_link["rows"], fixture.database(&db).await["rows"]);
    let database = fixture.database(&db).await;
    assert_eq!(database["views"], own);

    // An own view id is not a view of the link, and the other way around.
    let (status, body) = fixture
        .call(
            "PATCH",
            &format!("databases/{db}/views/{own_view}?link_id={link}"),
            Some(json!({"name": "X"})),
        )
        .await;
    assert_eq!(
        (status, body["code"].as_str()),
        (StatusCode::NOT_FOUND, Some("database_view_not_found"))
    );
    let (status, _) = fixture
        .call("DELETE", &format!("databases/{db}/views/{link_view}"), None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // The last view of a link stays.
    let board = id_of(&through_link["views"][1]).to_owned();
    fixture
        .ok(
            "DELETE",
            &format!("databases/{db}/views/{board}?link_id={link}"),
            None,
        )
        .await;
    let (status, body) = fixture
        .call(
            "DELETE",
            &format!("databases/{db}/views/{link_view}?link_id={link}"),
            None,
        )
        .await;
    assert_eq!(
        (status, body["code"].as_str()),
        (StatusCode::CONFLICT, Some("database_last_view"))
    );

    // A new property shows in the link's views too.
    let notes = fixture
        .add_property(&db, json!({"name": "Notes", "type": "text"}))
        .await;
    let through_link = fixture
        .ok("GET", &format!("databases/{db}?link={link}"), None)
        .await;
    let columns: Vec<&str> = through_link["views"][0]["config"]["columns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|column| column["property_id"].as_str().unwrap())
        .collect();
    assert!(columns.contains(&id_of(&notes)), "{columns:?}");
}

#[tokio::test]
async fn a_link_of_another_database_is_not_found() {
    let fixture = Fixture::new().await;
    let db = fixture.create_database("Tasks").await;
    let other = fixture.create_database("Other").await;
    let host = id_of(&fixture.create_page(json!({"title": "Host"})).await).to_owned();
    let link = id_of(&fixture.link(&db, &host).await["link"]).to_owned();
    for (method, path, body) in [
        ("GET", format!("databases/{other}?link={link}"), None),
        (
            "POST",
            format!("databases/{other}/views"),
            Some(json!({"name": "V", "type": "table", "link_id": link})),
        ),
        ("DELETE", format!("databases/{other}/links/{link}"), None),
        ("GET", format!("databases/{db}?link=not-an-id"), None),
    ] {
        let (status, body) = fixture.call(method, &path, body).await;
        assert_eq!(
            (status, body["code"].as_str()),
            (StatusCode::NOT_FOUND, Some("database_link_not_found")),
            "{method} {path}"
        );
    }
}

#[tokio::test]
async fn a_link_needs_read_access_to_the_database_and_edit_access_to_the_host() {
    let fixture = Fixture::new().await;
    let (_, member) = fixture.add_member("member@example.com").await;
    let shared = fixture.create_database("Shared").await;
    let private_db = id_of(
        &fixture
            .create_page(json!({"kind": "database", "title": "Diary", "private": true}))
            .await,
    )
    .to_owned();
    let host = id_of(&fixture.create_page(json!({"title": "Host"})).await).to_owned();
    let private_host = id_of(
        &fixture
            .create_page(json!({"title": "Mine", "private": true}))
            .await,
    )
    .to_owned();
    let link_as = |cookie: String, db: String, host: String| {
        let fixture = &fixture;
        async move {
            fixture
                .call_as(
                    &cookie,
                    "POST",
                    &format!("databases/{db}/links"),
                    Some(json!({"host_page_id": host})),
                )
                .await
        }
    };

    // A database the member cannot read.
    let (status, body) = link_as(member.clone(), private_db.clone(), host.clone()).await;
    assert_eq!(
        (status, body["code"].as_str()),
        (StatusCode::NOT_FOUND, Some("page_not_found"))
    );
    // A host page the member cannot see.
    let (status, body) = link_as(member.clone(), shared.clone(), private_host.clone()).await;
    assert_eq!(
        (status, body["code"].as_str()),
        (StatusCode::UNPROCESSABLE_ENTITY, Some("validation_failed")),
        "{body}"
    );
    let (status, _) = link_as(member.clone(), shared.clone(), host.clone()).await;
    assert_eq!(status, StatusCode::CREATED);

    // A locked host freezes its linked views, not the database's own views.
    let link = id_of(&fixture.link(&shared, &host).await["link"]).to_owned();
    fixture
        .ok(
            "POST",
            &format!("pages/{host}/lock"),
            Some(json!({"locked": true})),
        )
        .await;
    let (status, body) = fixture
        .call(
            "POST",
            &format!("databases/{shared}/views"),
            Some(json!({"name": "V", "type": "table", "link_id": link})),
        )
        .await;
    assert_eq!(
        (status, body["code"].as_str()),
        (StatusCode::LOCKED, Some("page_locked"))
    );
    let (status, _) = link_as(fixture.cookie.clone(), shared.clone(), host.clone()).await;
    assert_eq!(status, StatusCode::LOCKED);
    fixture
        .ok(
            "POST",
            &format!("databases/{shared}/views"),
            Some(json!({"name": "Own", "type": "table"})),
        )
        .await;

    // A locked database freezes its own views, not a link's.
    fixture
        .ok(
            "POST",
            &format!("pages/{host}/lock"),
            Some(json!({"locked": false})),
        )
        .await;
    fixture
        .ok(
            "POST",
            &format!("pages/{shared}/lock"),
            Some(json!({"locked": true})),
        )
        .await;
    fixture
        .ok(
            "POST",
            &format!("databases/{shared}/views"),
            Some(json!({"name": "Linked", "type": "table", "link_id": link})),
        )
        .await;
}

#[tokio::test]
async fn deleting_the_host_forever_removes_its_links_and_their_views() {
    let fixture = Fixture::new().await;
    let db = fixture.create_database("Tasks").await;
    let host = id_of(&fixture.create_page(json!({"title": "Host"})).await).to_owned();
    let link = id_of(&fixture.link(&db, &host).await["link"]).to_owned();
    let views = "SELECT COUNT(*) FROM database_views WHERE link_id = $1";
    let links = "SELECT COUNT(*) FROM database_links WHERE id = $1";

    // A trashed host keeps them.
    fixture.trash_page(&host).await;
    assert_eq!(
        (
            fixture.count(links, &link).await,
            fixture.count(views, &link).await
        ),
        (1, 1)
    );
    fixture.restore_page(&host).await;
    fixture.purge_page(&host).await;
    assert_eq!(
        (
            fixture.count(links, &link).await,
            fixture.count(views, &link).await
        ),
        (0, 0)
    );
    assert_eq!(
        fixture.database(&db).await["views"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn duplicating_a_page_copies_its_inline_database_and_its_linked_views() {
    let fixture = Fixture::new().await;
    let elsewhere = fixture.create_database("Elsewhere").await;
    let host = id_of(&fixture.create_page(json!({"title": "Host"})).await).to_owned();
    let inline = id_of(
        &fixture
            .create_page(json!({"kind": "database", "title": "Inline", "parent_id": host}))
            .await,
    )
    .to_owned();
    let inline_link = fixture.link(&inline, &host).await;
    let link = id_of(&inline_link["link"]).to_owned();
    let inline_view = id_of(&inline_link["views"][0]).to_owned();
    let notes = id_of(
        &fixture
            .add_property(&inline, json!({"name": "Notes", "type": "text"}))
            .await,
    )
    .to_owned();
    fixture
        .ok(
            "PATCH",
            &format!("databases/{inline}/views/{inline_view}?link_id={link}"),
            Some(json!({"config": {"sorts": [{"property_id": notes, "direction": "desc"}]}})),
        )
        .await;
    let outside = id_of(&fixture.link(&elsewhere, &host).await["link"]).to_owned();
    fixture
        .set_content(
            &host,
            json!([
                {"id": "a", "type": "database", "props": {"databaseId": inline, "linkId": ""}, "children": []},
                {"id": "b", "type": "database", "props": {"databaseId": inline, "linkId": link}, "children": []},
                {"id": "c", "type": "database", "props": {"databaseId": elsewhere, "linkId": outside}, "children": []},
            ]),
        )
        .await;

    let copy = fixture
        .ok(
            "POST",
            &format!("pages/{host}/duplicate"),
            Some(json!({"include_children": true})),
        )
        .await;
    let blocks = copy["content"].as_array().unwrap();
    let props: Vec<(&str, &str)> = blocks
        .iter()
        .map(|block| {
            (
                block["props"]["databaseId"].as_str().unwrap(),
                block["props"]["linkId"].as_str().unwrap(),
            )
        })
        .collect();
    let inline_copy = props[0].0;
    assert_ne!(inline_copy, inline);
    assert_eq!(props[0].1, "");
    assert_eq!(props[1].0, inline_copy);
    assert_ne!(props[1].1, link);
    assert_eq!(props[2].0, elsewhere);
    assert_ne!(props[2].1, outside);

    // The copied link shows the copied database, its view sorted by the copied property.
    let copied = fixture
        .ok(
            "GET",
            &format!("databases/{inline_copy}?link={}", props[1].1),
            None,
        )
        .await;
    let copied_notes = id_of(property(&copied, "Notes")).to_owned();
    assert_ne!(copied_notes, notes);
    assert_eq!(
        copied["views"][0]["config"]["sorts"],
        json!([{"property_id": copied_notes, "direction": "desc"}])
    );
    assert_eq!(names(&copied["views"]), ["Table"]);
    // The copy of the outside link still shows the original database, from the copied host.
    let (_, outside_copy) = fixture
        .call(
            "GET",
            &format!("databases/{elsewhere}?link={}", props[2].1),
            None,
        )
        .await;
    assert_eq!(names(&outside_copy["views"]), ["Table"]);
    let host_of_copy: String =
        sqlx::query_scalar("SELECT host_page_id FROM database_links WHERE id = $1")
            .bind(props[2].1)
            .fetch_one(fixture.database.pool())
            .await
            .unwrap();
    assert_eq!(host_of_copy, id_of(&copy));
    // The originals are untouched.
    let original = fixture
        .ok("GET", &format!("databases/{inline}?link={link}"), None)
        .await;
    assert_eq!(
        original["views"][0]["config"]["sorts"],
        json!([{"property_id": notes, "direction": "desc"}])
    );
}

#[tokio::test]
async fn footer_calculations_fit_their_property_type() {
    let fixture = Fixture::new().await;
    let db = fixture.create_database("Footer").await;
    let database = fixture.database(&db).await;
    let view = id_of(&database["views"][0]).to_owned();
    let name = id_of(property(&database, "Name")).to_owned();
    let price = id_of(
        &fixture
            .add_property(&db, json!({"name": "Price", "type": "number"}))
            .await,
    )
    .to_owned();
    let path = format!("databases/{db}/views/{view}");

    let saved = fixture
        .ok(
            "PATCH",
            &path,
            Some(json!({"config": {"calculations": {
                &price: "sum",
                &name: "count_values",
                Id::new_v7().to_string(): "count_all",
            }}})),
        )
        .await;
    assert_eq!(
        saved["config"]["calculations"],
        json!({&name: "count_values", &price: "sum"}),
        "a calculation of an unknown property is dropped"
    );

    let (status, body) = fixture
        .call(
            "PATCH",
            &path,
            Some(json!({"config": {"calculations": {&name: "sum"}}})),
        )
        .await;
    assert_eq!(
        (status, body["detail"].as_str()),
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            Some(format!("config.calculations.{name}").as_str())
        ),
        "{body}"
    );

    // A type change to checkbox drops the sum; the title's count stays.
    fixture
        .ok(
            "PATCH",
            &format!("databases/{db}/properties/{price}"),
            Some(json!({"type": "checkbox"})),
        )
        .await;
    let database = fixture.database(&db).await;
    assert_eq!(
        database["views"][0]["config"]["calculations"],
        json!({&name: "count_values"})
    );
}

#[tokio::test]
async fn filters_nest_and_or_groups_three_levels_deep() {
    let fixture = Fixture::new().await;
    let db = fixture.create_database("Groups").await;
    let database = fixture.database(&db).await;
    let view = id_of(&database["views"][0]).to_owned();
    let name = id_of(property(&database, "Name")).to_owned();
    let tags = id_of(property(&database, "Tags")).to_owned();
    let path = format!("databases/{db}/views/{view}");
    let rule = |property: &str, value: &str| json!({"property_id": property, "operator": "contains", "value": value});
    let tags_rule = json!({"property_id": tags, "operator": "is_empty", "value": null});

    let filter = json!({"op": "and", "children": [
        rule(&name, "a"),
        {"op": "or", "children": [
            rule(&name, "b"),
            {"op": "and", "children": [rule(&name, "c"), tags_rule]},
        ]},
    ]});
    let saved = fixture
        .ok("PATCH", &path, Some(json!({"config": {"filter": filter}})))
        .await;
    assert_eq!(
        saved["config"]["filter"], filter,
        "3 levels are kept as sent"
    );

    let deep = json!({"op": "and", "children": [
        rule(&name, "a"),
        {"op": "or", "children": [
            {"op": "and", "children": [{"op": "or", "children": [rule(&name, "d")]}]},
        ]},
    ]});
    let many = |count: usize| (0..count).map(|_| rule(&name, "x")).collect::<Vec<_>>();
    for (filter, field) in [
        (deep, "config.filter.children[1].children[0].children[0]"),
        (
            json!({"op": "or", "children": [{"op": "and", "children": [{"property_id": name, "operator": ">", "value": 1}]}]}),
            "config.filter.children[0].children[0].operator",
        ),
        (
            json!({"op": "and", "children": many(11)}),
            "config.filter.children",
        ),
        (
            json!({"op": "and", "children": [
                {"op": "or", "children": many(10)},
                {"op": "or", "children": many(10)},
                rule(&name, "one too many"),
            ]}),
            "config.filter",
        ),
        (json!({"rules": []}), "config"),
    ] {
        let (status, body) = fixture
            .call("PATCH", &path, Some(json!({"config": {"filter": filter}})))
            .await;
        assert_eq!(
            (status, body["detail"].as_str()),
            (StatusCode::UNPROCESSABLE_ENTITY, Some(field)),
            "{body}"
        );
    }

    // Deleting Tags drops its rule; the group it leaves with one rule stays.
    fixture
        .ok("DELETE", &format!("databases/{db}/properties/{tags}"), None)
        .await;
    let repaired = &fixture.database(&db).await["views"][0]["config"]["filter"];
    assert_eq!(
        repaired,
        &json!({"op": "and", "children": [
            rule(&name, "a"),
            {"op": "or", "children": [
                rule(&name, "b"),
                {"op": "and", "children": [rule(&name, "c")]},
            ]},
        ]})
    );
    // A group whose only rule goes is dropped.
    let lonely = fixture
        .add_property(&db, json!({"name": "Lonely", "type": "text"}))
        .await;
    let lonely = id_of(&lonely).to_owned();
    fixture
        .ok(
            "PATCH",
            &path,
            Some(json!({"config": {"filter": {"op": "and", "children": [
                rule(&name, "a"),
                {"op": "or", "children": [rule(&lonely, "z")]},
            ]}}})),
        )
        .await;
    fixture
        .ok(
            "DELETE",
            &format!("databases/{db}/properties/{lonely}"),
            None,
        )
        .await;
    assert_eq!(
        fixture.database(&db).await["views"][0]["config"]["filter"],
        json!({"op": "and", "children": [rule(&name, "a")]})
    );
}

#[tokio::test]
async fn the_filter_migration_turns_p1_rules_into_the_root_and_group() {
    let fixture = Fixture::new().await;
    let db = fixture.create_database("Before groups").await;
    let database = fixture.database(&db).await;
    let view = id_of(&database["views"][0]).to_owned();
    let name = id_of(property(&database, "Name")).to_owned();
    let rules = json!([{"property_id": name, "operator": "contains", "value": "rust"}]);
    // The view as P1 stored it, and the migration not yet run.
    sqlx::query("UPDATE database_views SET config_json = $1 WHERE id = $2")
        .bind(
            json!({"columns": [], "sorts": [], "filter": {"rules": rules}, "wrap_cells": true})
                .to_string(),
        )
        .bind(&view)
        .execute(fixture.database.pool())
        .await
        .unwrap();
    sqlx::query("DELETE FROM schema_migrations WHERE version = 82")
        .execute(fixture.database.pool())
        .await
        .unwrap();
    orbit_platform::MigrationRunner::embedded("test")
        .run(&fixture.database)
        .await
        .unwrap();
    let config = &fixture.database(&db).await["views"][0]["config"];
    assert_eq!(config["filter"], json!({"op": "and", "children": rules}));
    assert_eq!(config["wrap_cells"], true);
}
