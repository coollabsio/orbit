//! Pages moving into and out of a database (docs databases P2 item 8).

use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use orbit_platform::{Id, PasswordService, TestDatabase, TimestampMillis};
use orbit_server::auth_routes::CookieMode;
use orbit_server::database_routes::{DatabaseState, database_router};
use orbit_server::page_routes::{PageState, page_router};
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use serde_json::{Value, json};
use tower::ServiceExt;

struct Fixture {
    database: TestDatabase,
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
            database_router(DatabaseState::new(identity, CookieMode::secure())),
        );
        Self {
            database,
            app,
            workspace_id: setup.workspace_id.to_string(),
            cookie: format!("__Host-orbit_session={}", setup.session.token),
            owner_id: setup.user_id,
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

    async fn create_page(&self, body: Value) -> String {
        id_of(&self.ok("POST", "pages", Some(body)).await)
    }

    async fn create_database(&self, title: &str) -> String {
        self.create_page(json!({"kind": "database", "title": title}))
            .await
    }

    async fn add_property(&self, database: &str, body: Value) -> Value {
        self.ok(
            "POST",
            &format!("databases/{database}/properties"),
            Some(body),
        )
        .await
    }

    async fn add_row(&self, database: &str, body: Value) -> String {
        id_of(
            &self
                .ok("POST", &format!("databases/{database}/rows"), Some(body))
                .await,
        )
    }

    async fn page(&self, id: &str) -> Value {
        self.ok("GET", &format!("pages/{id}"), None).await
    }

    /// `POST /pages/{id}/move` on the page's current version.
    async fn move_page(&self, id: &str, body: Value) -> (StatusCode, Value) {
        let mut body = body;
        body["expected_version"] = self.page(id).await["version"].clone();
        self.call("POST", &format!("pages/{id}/move"), Some(body))
            .await
    }

    async fn moved(&self, id: &str, body: Value) -> Value {
        let (status, value) = self.move_page(id, body).await;
        assert_eq!(status, StatusCode::OK, "{value}");
        value
    }

    /// The row's values in `GET /databases/{id}`.
    async fn row_values(&self, database: &str, row: &str) -> Value {
        let detail = self.ok("GET", &format!("databases/{database}"), None).await;
        detail["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["id"] == row)
            .unwrap_or_else(|| panic!("{row} is not a row of {database}"))["values"]
            .clone()
    }

    async fn row_ids(&self, database: &str) -> Vec<String> {
        let detail = self.ok("GET", &format!("databases/{database}"), None).await;
        detail["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(id_of)
            .collect()
    }

    async fn property_text(&self, page: &str) -> Option<String> {
        sqlx::query_scalar("SELECT property_text FROM pages WHERE id = $1")
            .bind(page)
            .fetch_one(self.database.pool())
            .await
            .unwrap()
    }

    async fn stored_values(&self, page: &str) -> i64 {
        sqlx::query_scalar(
            "SELECT CAST(COUNT(*) AS BIGINT) FROM page_property_values WHERE page_id = $1",
        )
        .bind(page)
        .fetch_one(self.database.pool())
        .await
        .unwrap()
    }

    async fn search(&self, query: &str) -> Vec<String> {
        self.ok("GET", &format!("pages/search?q={query}"), None)
            .await["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(id_of)
            .collect()
    }
}

fn id_of(value: &Value) -> String {
    value["id"].as_str().unwrap().to_owned()
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

fn assert_conflict((status, body): (StatusCode, Value), code: &str) {
    assert_eq!(
        (status, body["code"].as_str()),
        (StatusCode::CONFLICT, Some(code)),
        "{body}"
    );
}

#[tokio::test]
async fn a_page_moved_into_a_database_is_its_last_row_with_no_values() {
    let fixture = Fixture::new().await;
    let db = fixture.create_database("Reading list").await;
    let first = fixture.add_row(&db, json!({"title": "First"})).await;
    let second = fixture.add_row(&db, json!({"title": "Second"})).await;
    let page = fixture.create_page(json!({"title": "Loose notes"})).await;
    let child = fixture
        .create_page(json!({"parent_id": page, "title": "Child"}))
        .await;
    fixture
        .ok("PUT", &format!("pages/{page}/favorite"), None)
        .await;

    let moved = fixture
        .moved(&page, json!({"parent_id": db, "position": 0}))
        .await;
    assert_eq!(
        (
            moved["database_id"].as_str(),
            moved["position"].as_i64(),
            moved["dropped_values"].as_u64()
        ),
        (Some(db.as_str()), Some(2), Some(0))
    );
    assert_eq!(fixture.row_ids(&db).await, [first, second, page.clone()]);
    assert_eq!(fixture.row_values(&db, &page).await, json!({}));
    assert_eq!(fixture.property_text(&page).await.as_deref(), Some(""));

    // Its sub-page stays its sub-page, and is not a row.
    let child = fixture.page(&child).await;
    assert_eq!(
        (child["parent_id"].as_str(), &child["database_id"]),
        (Some(page.as_str()), &Value::Null)
    );
    // Rows are not in the tree; the favorite now names the database.
    let tree = fixture.ok("GET", "pages", None).await;
    assert!(
        !tree["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["id"] == page.as_str()),
        "{tree}"
    );
    let favorites = fixture.ok("GET", "pages/favorites", None).await;
    assert_eq!(favorites["items"][0]["page_id"], page.as_str());
    assert_eq!(favorites["items"][0]["database_id"], db.as_str());

    // A page created under a database is a row too.
    let created = fixture
        .ok(
            "POST",
            "pages",
            Some(json!({"parent_id": db, "title": "Made here"})),
        )
        .await;
    assert_eq!(created["database_id"], db.as_str());
    assert_eq!(fixture.row_ids(&db).await.len(), 4);
    assert_eq!(
        fixture.property_text(&id_of(&created)).await.as_deref(),
        Some("")
    );
}

#[tokio::test]
async fn a_database_cannot_be_a_row_but_can_sit_below_one() {
    let fixture = Fixture::new().await;
    let db = fixture.create_database("Projects").await;
    let other = fixture.create_database("Other").await;
    let row = fixture.add_row(&db, json!({"title": "Row"})).await;

    assert_conflict(
        fixture
            .move_page(&other, json!({"parent_id": db, "position": 0}))
            .await,
        "database_in_database",
    );
    assert_conflict(
        fixture
            .call(
                "POST",
                "pages",
                Some(json!({"parent_id": db, "kind": "database", "title": "Nested"})),
            )
            .await,
        "database_in_database",
    );
    assert_conflict(
        fixture
            .move_page(
                &other,
                json!({"parent_id": db, "position": 0, "dry_run": true}),
            )
            .await,
        "database_in_database",
    );

    let moved = fixture
        .moved(&other, json!({"parent_id": row, "position": 0}))
        .await;
    assert_eq!(moved["database_id"], Value::Null);
    assert_eq!(fixture.row_ids(&db).await, [row]);
}

#[tokio::test]
async fn a_full_database_refuses_a_page_moving_in() {
    let fixture = Fixture::new().await;
    let db = fixture.create_database("Full").await;
    let teamspace: String = sqlx::query_scalar("SELECT teamspace_id FROM pages WHERE id = $1")
        .bind(&db)
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    let now = TimestampMillis::now().as_millis();
    let mut tx = fixture.database.pool().begin().await.unwrap();
    for position in 0..5_000_i64 {
        sqlx::query(
            "INSERT INTO pages (id, workspace_id, parent_id, teamspace_id, title, position, creator_id, updated_by, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, 'Row', $5, $6, $7, $8, $9)",
        )
        .bind(Id::new_v7().to_string())
        .bind(&fixture.workspace_id)
        .bind(&db)
        .bind(&teamspace)
        .bind(position)
        .bind(fixture.owner_id.to_string())
        .bind(fixture.owner_id.to_string())
        .bind(now)
        .bind(now)
        .execute(&mut *tx)
        .await
        .unwrap();
    }
    tx.commit().await.unwrap();
    let page = fixture.create_page(json!({"title": "One more"})).await;

    for dry_run in [true, false] {
        assert_conflict(
            fixture
                .move_page(
                    &page,
                    json!({"parent_id": db, "position": 0, "dry_run": dry_run}),
                )
                .await,
            "database_row_limit",
        );
    }
    assert_conflict(
        fixture
            .call(
                "POST",
                "pages",
                Some(json!({"parent_id": db, "title": "Too many"})),
            )
            .await,
        "database_row_limit",
    );
    assert_eq!(fixture.page(&page).await["parent_id"], Value::Null);
}

#[tokio::test]
async fn a_row_moved_out_loses_its_values_and_search_text_and_can_move_back() {
    let fixture = Fixture::new().await;
    let db = fixture.create_database("Research").await;
    let stage = fixture
        .add_property(
            &db,
            json!({"name": "Stage", "type": "select", "config": {"options": [{"name": "Reading"}]}}),
        )
        .await;
    let reading = option_id(&stage, "Reading");
    let stage = id_of(&stage);
    let ticket = id_of(
        &fixture
            .add_property(&db, json!({"name": "Ticket", "type": "text"}))
            .await,
    );
    let row = fixture
        .add_row(
            &db,
            json!({"title": "Paper", "values": {&stage: reading, &ticket: "ACME-12"}}),
        )
        .await;
    let child = fixture
        .create_page(json!({"parent_id": row, "title": "Notes"}))
        .await;
    assert_eq!(fixture.search("acme").await, [row.as_str()]);
    let audited = || async {
        sqlx::query_scalar::<_, i64>(
            "SELECT CAST(COUNT(*) AS BIGINT) FROM audit_events WHERE resource_id = $1",
        )
        .bind(&db)
        .fetch_one(fixture.database.pool())
        .await
        .unwrap()
    };
    let audit_before = audited().await;
    let version = fixture.page(&row).await["version"].clone();

    // A dry run counts the values and changes nothing.
    let preview = fixture
        .moved(
            &row,
            json!({"parent_id": null, "position": 0, "dry_run": true}),
        )
        .await;
    assert_eq!(
        (
            preview["dropped_values"].as_u64(),
            &preview["database_id"],
            preview["version"].as_u64()
        ),
        (Some(2), &Value::Null, Some(version.as_u64().unwrap() + 1))
    );
    assert_eq!(fixture.page(&row).await["version"], version);
    assert_eq!(fixture.stored_values(&row).await, 2);
    assert_eq!(fixture.row_ids(&db).await, [row.as_str()]);
    assert_eq!(audited().await, audit_before);

    let moved = fixture
        .moved(&row, json!({"parent_id": null, "position": 0}))
        .await;
    assert_eq!(
        (moved["dropped_values"].as_u64(), &moved["database_id"]),
        (Some(2), &Value::Null)
    );
    assert_eq!(fixture.stored_values(&row).await, 0);
    assert_eq!(fixture.property_text(&row).await, None);
    assert!(fixture.row_ids(&db).await.is_empty());
    assert!(fixture.search("acme").await.is_empty());
    assert_eq!(fixture.search("paper").await, [row.as_str()]);
    assert_eq!(fixture.page(&child).await["parent_id"], row.as_str());
    assert_eq!(
        audited().await,
        audit_before + 1,
        "the database hears of it"
    );

    // Undo: moving it back makes it a row again, with no values.
    let back = fixture
        .moved(&row, json!({"parent_id": db, "position": 0}))
        .await;
    assert_eq!(
        (
            back["database_id"].as_str(),
            back["dropped_values"].as_u64()
        ),
        (Some(db.as_str()), Some(0))
    );
    assert_eq!(fixture.row_values(&db, &row).await, json!({}));
    assert_eq!(fixture.property_text(&row).await.as_deref(), Some(""));
}

#[tokio::test]
async fn a_row_moved_to_another_database_keeps_values_of_same_name_and_type() {
    let fixture = Fixture::new().await;
    let from = fixture.create_database("From").await;
    let to = fixture.create_database("To").await;
    let stage = fixture
        .add_property(
            &from,
            json!({"name": "Stage", "type": "select", "config": {"options": [{"name": "Reading"}, {"name": "Done"}]}}),
        )
        .await;
    let labels = fixture
        .add_property(
            &from,
            json!({"name": "Labels", "type": "multi_select", "config": {"options": [{"name": "Rust"}, {"name": "Go"}]}}),
        )
        .await;
    let notes = id_of(
        &fixture
            .add_property(&from, json!({"name": "Notes", "type": "text"}))
            .await,
    );
    let score = id_of(
        &fixture
            .add_property(&from, json!({"name": "Score", "type": "number"}))
            .await,
    );
    let owner = id_of(
        &fixture
            .add_property(&from, json!({"name": "Owner", "type": "person"}))
            .await,
    );
    let row = fixture
        .add_row(
            &from,
            json!({"title": "Paper", "values": {
                id_of(&stage): option_id(&stage, "Done"),
                id_of(&labels): [option_id(&labels, "Rust"), option_id(&labels, "Go")],
                &notes: "hello",
                &score: 3,
                &owner: [fixture.owner_id.to_string()],
            }}),
        )
        .await;

    // Same name in another case and type: kept, options matched by name. Notes is a number
    // there, Score is missing, and only "Rust" exists among the labels.
    let to_stage = fixture
        .add_property(
            &to,
            json!({"name": "stage", "type": "select", "config": {"options": [{"name": "Done"}]}}),
        )
        .await;
    let to_labels = fixture
        .add_property(
            &to,
            json!({"name": "Labels", "type": "multi_select", "config": {"options": [{"name": "Rust"}]}}),
        )
        .await;
    fixture
        .add_property(&to, json!({"name": "Notes", "type": "number"}))
        .await;
    let to_owner = id_of(
        &fixture
            .add_property(&to, json!({"name": "OWNER", "type": "person"}))
            .await,
    );

    let preview = fixture
        .moved(
            &row,
            json!({"parent_id": to, "position": 0, "dry_run": true}),
        )
        .await;
    assert_eq!(preview["dropped_values"], 2);
    assert_eq!(fixture.stored_values(&row).await, 5);

    let moved = fixture
        .moved(&row, json!({"parent_id": to, "position": 0}))
        .await;
    assert_eq!(
        (
            moved["database_id"].as_str(),
            moved["dropped_values"].as_u64()
        ),
        (Some(to.as_str()), Some(2))
    );
    assert_eq!(
        fixture.row_values(&to, &row).await,
        json!({
            id_of(&to_stage): option_id(&to_stage, "Done"),
            id_of(&to_labels): [option_id(&to_labels, "Rust")],
            &to_owner: [fixture.owner_id.to_string()],
        })
    );
    assert_eq!(fixture.stored_values(&row).await, 3);
    assert_eq!(
        fixture.property_text(&row).await.as_deref(),
        Some("Done\nRust")
    );
    assert!(fixture.row_ids(&from).await.is_empty());
}
