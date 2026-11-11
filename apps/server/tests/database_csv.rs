use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use orbit_platform::{PasswordService, TestDatabase, TimestampMillis};
use orbit_server::auth_routes::CookieMode;
use orbit_server::database_routes::{DatabaseState, database_router};
use orbit_server::page_routes::{PageState, page_router};
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use serde_json::{Value, json};
use tower::ServiceExt;

struct Fixture {
    _database: TestDatabase,
    app: axum::Router,
    workspace_id: String,
    cookie: String,
    owner_id: String,
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
                    display_name: "Owner Person".to_owned(),
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
            _database: database,
            app,
            workspace_id: setup.workspace_id.to_string(),
            cookie: format!("__Host-orbit_session={}", setup.session.token),
            owner_id: setup.user_id.to_string(),
        }
    }

    async fn call(&self, method: &str, path: &str, body: Option<Value>) -> (StatusCode, Value) {
        let builder = Request::builder()
            .method(method)
            .uri(format!("/api/v1/workspaces/{}/{path}", self.workspace_id))
            .header(header::COOKIE, &self.cookie);
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

    /// A database with the starter "Name" and "Tags", a "Stage" select (option "Open") and a "Score" number.
    async fn database(&self) -> (String, Value) {
        let page = self
            .ok(
                "POST",
                "pages",
                Some(json!({"kind": "database", "title": "Imports"})),
            )
            .await;
        let id = page["id"].as_str().unwrap().to_owned();
        self.ok(
            "POST",
            &format!("databases/{id}/properties"),
            Some(json!({"name": "Stage", "type": "select", "config": {"options": [{"name": "Open"}]}})),
        )
        .await;
        self.ok(
            "POST",
            &format!("databases/{id}/properties"),
            Some(json!({"name": "Score", "type": "number"})),
        )
        .await;
        let detail = self.ok("GET", &format!("databases/{id}"), None).await;
        (id, detail)
    }
}

fn property<'a>(detail: &'a Value, name: &str) -> &'a Value {
    detail["properties"]
        .as_array()
        .unwrap()
        .iter()
        .find(|property| property["name"] == name)
        .unwrap_or_else(|| panic!("property {name}"))
}

fn option_names(property: &Value) -> Vec<String> {
    property["config"]["options"]
        .as_array()
        .unwrap()
        .iter()
        .map(|option| option["name"].as_str().unwrap().to_owned())
        .collect()
}

fn property_names(detail: &Value) -> Vec<&str> {
    detail["properties"]
        .as_array()
        .unwrap()
        .iter()
        .map(|property| property["name"].as_str().unwrap())
        .collect()
}

#[tokio::test]
async fn merge_adds_rows_options_and_new_properties() {
    let fixture = Fixture::new().await;
    let (id, detail) = fixture.database().await;
    let title = property(&detail, "Name")["id"].clone();
    let stage = property(&detail, "Stage")["id"].clone();
    let score = property(&detail, "Score")["id"].clone();
    let body = json!({
        "columns": [
            {"name": "Title", "target": {"property_id": title}},
            {"name": "Stage", "target": {"property_id": stage}},
            {"name": "Score", "target": {"property_id": score}},
            {"name": "Owner", "target": {"new": "person"}},
            {"name": "Ignored", "target": "skip"},
            {"name": "Due", "target": {"new": "date"}},
        ],
        "rows": [
            ["First", "open", "1.5", "owner@example.com", "x", "2026-11-09"],
            ["Second", "Closed", "many", "OWNER PERSON", "y", "soon"],
            ["Third"],
        ],
    });
    let dry = fixture
        .ok(
            "POST",
            &format!("databases/{id}/import"),
            Some(json!({"dry_run": true, "columns": body["columns"], "rows": body["rows"]})),
        )
        .await;
    let after_dry = fixture.ok("GET", &format!("databases/{id}"), None).await;
    assert_eq!(after_dry["rows"].as_array().unwrap().len(), 0);
    assert_eq!(
        property_names(&after_dry),
        ["Name", "Tags", "Stage", "Score"]
    );

    let result = fixture
        .ok("POST", &format!("databases/{id}/import"), Some(body))
        .await;
    assert_eq!(result, dry);
    assert_eq!(
        result,
        json!({
            "created_rows": 3,
            "converted": 9,
            "cleared": 2,
            "created_options": 1,
            "values_truncated": 0,
            "columns": [
                {"converted": 3, "cleared": 0, "created_options": 0, "values_truncated": 0},
                {"converted": 2, "cleared": 0, "created_options": 1, "values_truncated": 0},
                {"converted": 1, "cleared": 1, "created_options": 0, "values_truncated": 0},
                {"converted": 2, "cleared": 0, "created_options": 0, "values_truncated": 0},
                {"converted": 0, "cleared": 0, "created_options": 0, "values_truncated": 0},
                {"converted": 1, "cleared": 1, "created_options": 0, "values_truncated": 0},
            ],
        })
    );

    let detail = fixture.ok("GET", &format!("databases/{id}"), None).await;
    assert_eq!(
        property_names(&detail),
        ["Name", "Tags", "Stage", "Score", "Owner", "Due"]
    );
    let stage = property(&detail, "Stage");
    assert_eq!(option_names(stage), ["Open", "Closed"]);
    let open = stage["config"]["options"][0]["id"].clone();
    let owner = property(&detail, "Owner")["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let due = property(&detail, "Due")["id"].as_str().unwrap().to_owned();
    let rows = detail["rows"].as_array().unwrap();
    let titles: Vec<&str> = rows
        .iter()
        .map(|row| row["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, ["First", "Second", "Third"]);
    let first = &rows[0]["values"];
    assert_eq!(first[stage["id"].as_str().unwrap()], open);
    assert_eq!(first[score.as_str().unwrap()], json!(1.5));
    assert_eq!(first[&owner], json!([fixture.owner_id]));
    assert_eq!(first[&due], json!({"start": "2026-11-09", "end": null}));
    assert_eq!(rows[1]["values"][&owner], json!([fixture.owner_id]));
    assert!(rows[1]["values"].get(score.as_str().unwrap()).is_none());
    assert_eq!(rows[2]["values"], json!({}));
    // Every view shows the new properties.
    let columns: Vec<&Value> = detail["views"][0]["config"]["columns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|column| &column["property_id"])
        .collect();
    assert!(columns.contains(&&json!(owner)) && columns.contains(&&json!(due)));
}

#[tokio::test]
async fn a_bad_row_or_a_missing_title_writes_nothing() {
    let fixture = Fixture::new().await;
    let (id, detail) = fixture.database().await;
    let title = property(&detail, "Name")["id"].clone();
    let (status, problem) = fixture
        .call(
            "POST",
            &format!("databases/{id}/import"),
            Some(json!({
                "columns": [
                    {"name": "Name", "target": {"property_id": title}},
                    {"name": "Kind", "target": {"new": "select"}},
                ],
                "rows": [["ok", "a"], ["too", "many", "cells"]],
            })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{problem}");
    assert_eq!(problem["detail"], "rows[1]");

    let (status, problem) = fixture
        .call(
            "POST",
            &format!("databases/{id}/import"),
            Some(json!({
                "columns": [{"name": "Kind", "target": {"new": "select"}}],
                "rows": [["a"]],
            })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{problem}");
    assert_eq!(problem["detail"], "columns");

    let (status, _) = fixture
        .call(
            "POST",
            &format!("databases/{id}/import"),
            Some(json!({
                "columns": [
                    {"name": "Name", "target": {"property_id": title}},
                    {"name": "Again", "target": {"property_id": title}},
                ],
                "rows": [],
            })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    let after = fixture.ok("GET", &format!("databases/{id}"), None).await;
    assert_eq!(after["rows"].as_array().unwrap().len(), 0);
    assert_eq!(property_names(&after), ["Name", "Tags", "Stage", "Score"]);
}

#[tokio::test]
async fn rows_past_the_cap_roll_back_the_whole_import() {
    let fixture = Fixture::new().await;
    let (id, detail) = fixture.database().await;
    let title = property(&detail, "Name")["id"].clone();
    let rows = |count: usize| -> Vec<Vec<String>> {
        (0..count)
            .map(|index| vec![format!("Row {index}"), "x".to_owned()])
            .collect()
    };
    let columns = json!([
        {"name": "Name", "target": {"property_id": title}},
        {"name": "Extra", "target": {"new": "text"}},
    ]);
    let result = fixture
        .ok(
            "POST",
            &format!("databases/{id}/import"),
            Some(json!({"columns": columns, "rows": rows(4_999)})),
        )
        .await;
    assert_eq!(result["created_rows"], 4_999);

    for dry_run in [true, false] {
        let (status, problem) = fixture
            .call(
                "POST",
                &format!("databases/{id}/import"),
                Some(json!({"dry_run": dry_run, "columns": columns, "rows": rows(2)})),
            )
            .await;
        assert_eq!(status, StatusCode::CONFLICT, "{problem}");
        assert_eq!(problem["code"], "database_row_limit");
    }
    let detail = fixture.ok("GET", &format!("databases/{id}"), None).await;
    assert_eq!(detail["rows"].as_array().unwrap().len(), 4_999);
    assert_eq!(
        property_names(&detail)
            .iter()
            .filter(|name| **name == "Extra")
            .count(),
        1
    );
    let last = detail["rows"].as_array().unwrap().last().unwrap();
    assert_eq!(last["title"], "Row 4998");
}

#[tokio::test]
async fn csv_creates_a_database_in_one_step() {
    let fixture = Fixture::new().await;
    let body = json!({
        "title": "Contacts",
        "title_column": 1,
        "columns": [
            {"name": "Email", "target": {"new": "email_is_not_a_type"}},
        ],
        "rows": [],
    });
    let (status, _) = fixture.call("POST", "databases/import", Some(body)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let body = json!({
        "title": "Contacts",
        "title_column": 1,
        "columns": [
            {"name": "Site", "target": {"new": "url"}},
            {"name": "Name", "target": "skip"},
            {"name": "Status", "target": {"new": "status"}},
            {"name": "Tags", "target": {"new": "multi_select"}},
        ],
        "rows": [
            ["https://example.com", "Ada", "Done", "math, poetry"],
            ["not a url", "Grace", "Blocked", "navy"],
        ],
    });
    let dry = fixture
        .ok(
            "POST",
            "databases/import",
            Some({
                let mut body = body.clone();
                body["dry_run"] = json!(true);
                body
            }),
        )
        .await;
    assert_eq!(dry["database"], Value::Null);
    let (_, pages) = fixture.call("GET", "pages", None).await;
    assert!(!pages.to_string().contains("Contacts"));

    let (status, created) = fixture.call("POST", "databases/import", Some(body)).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["result"], dry["result"]);
    assert_eq!(created["result"]["created_options"], 4);
    assert_eq!(created["result"]["cleared"], 1);
    assert_eq!(created["database"]["kind"], "database");
    let id = created["database"]["id"].as_str().unwrap();
    let detail = fixture.ok("GET", &format!("databases/{id}"), None).await;
    assert_eq!(property_names(&detail), ["Name", "Site", "Status", "Tags"]);
    assert_eq!(property(&detail, "Name")["type"], "title");
    // "Done" matches the default status option; "Blocked" is new.
    assert_eq!(
        option_names(property(&detail, "Status")),
        ["Not started", "In progress", "Done", "Blocked"]
    );
    assert_eq!(
        option_names(property(&detail, "Tags")),
        ["math", "poetry", "navy"]
    );
    let rows = detail["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["title"], "Ada");
    let site = property(&detail, "Site")["id"].as_str().unwrap();
    assert_eq!(rows[0]["values"][site], "https://example.com");
    assert!(rows[1]["values"].get(site).is_none());
    assert_eq!(detail["views"].as_array().unwrap().len(), 1);
    assert_eq!(
        detail["views"][0]["config"]["columns"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
}
