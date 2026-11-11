//! Database tools of the public API (spec P3 14.3): error messages per field, dates with a UTC
//! offset, and readable shapes for formula people, relations and unique IDs. SQLite by default,
//! Postgres when `ORBIT_TEST_POSTGRES_URL` is set.
use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use orbit_platform::{Id, PasswordService, TestDatabase, TimestampMillis};
use orbit_server::auth_routes::CookieMode;
use orbit_server::database_routes::{DatabaseState, database_router};
use orbit_server::page_routes::{PageState, page_router};
use orbit_server::public_api::{PublicApiState, router};
use orbit_server::repositories::api_tokens::{ApiTokenRepository, ApiTokenScope};
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::repositories::pages::PageRepository;
use orbit_server::repositories::tasks::TaskRepository;
use serde_json::{Value, json};
use tower::ServiceExt;

struct Fixture {
    _database: TestDatabase,
    web: axum::Router,
    api: axum::Router,
    workspace: Id,
    user: Id,
    cookie: String,
    token: String,
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
        let api = router(
            PublicApiState::new(
                Arc::new(tokens.clone()),
                Arc::new(TaskRepository::new((*database).clone())),
                "https://orbit.test",
            )
            .with_pages(Arc::new(PageRepository::new((*database).clone()))),
        );
        let token = tokens
            .create(
                setup.workspace_id,
                setup.user_id,
                "cli".into(),
                vec![setup.project_id],
                &[ApiTokenScope::DocsRead, ApiTokenScope::DocsWrite],
                false,
                None,
                "test",
                now,
            )
            .await
            .unwrap()
            .token;
        Self {
            _database: database,
            web,
            api,
            workspace: setup.workspace_id,
            user: setup.user_id,
            cookie: format!("__Host-orbit_session={}", setup.session.token),
            token,
        }
    }

    async fn send(app: &axum::Router, request: Request<Body>) -> (StatusCode, Value) {
        let response = app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        };
        (status, value)
    }

    fn request(
        uri: String,
        method: &str,
        auth: (header::HeaderName, String),
        body: Value,
    ) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .header(auth.0, auth.1)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    /// A web route of the fixture's workspace, as the owner; it must succeed.
    async fn web(&self, method: &str, path: &str, body: Value) -> Value {
        let uri = format!("/api/v1/workspaces/{}/{path}", self.workspace);
        let request = Self::request(uri, method, (header::COOKIE, self.cookie.clone()), body);
        let (status, value) = Self::send(&self.web, request).await;
        assert!(status.is_success(), "{method} {path}: {status} {value}");
        value
    }

    /// A public API route of the fixture's workspace, with the docs token.
    async fn api(&self, method: &str, path: &str, body: Value) -> (StatusCode, Value) {
        let uri = format!("/api/public/v1/workspaces/{}/{path}", self.workspace);
        let auth = (header::AUTHORIZATION, format!("Bearer {}", self.token));
        Self::send(&self.api, Self::request(uri, method, auth, body)).await
    }

    async fn database(&self, title: &str) -> String {
        let page = self
            .web("POST", "pages", json!({"kind": "database", "title": title}))
            .await;
        page["id"].as_str().unwrap().to_owned()
    }

    async fn property(&self, database: &str, body: Value) -> String {
        let property = self
            .web("POST", &format!("databases/{database}/properties"), body)
            .await;
        property["id"].as_str().unwrap().to_owned()
    }

    async fn row(&self, database: &str, body: Value) -> Value {
        let (status, row) = self
            .api("POST", &format!("docs/{database}/rows"), body)
            .await;
        assert_eq!(status, StatusCode::CREATED, "{row}");
        row
    }

    async fn query(&self, database: &str, body: Value) -> (StatusCode, Value) {
        self.api("POST", &format!("docs/{database}/database/query"), body)
            .await
    }
}

#[tokio::test]
async fn each_invalid_query_field_says_what_it_needs() {
    let f = Fixture::new().await;
    let db = f.database("Tasks").await;
    let cases = [
        (json!({"limit": 0}), "limit", "Send a limit from 1 to 100."),
        (
            json!({"cursor": "50.nope"}),
            "cursor",
            "The cursor is not a next_cursor of this query. Send the previous page's next_cursor with the same filter, sorts, view_id and tz.",
        ),
        (
            json!({"tz": "Nowhere/Land"}),
            "tz",
            "Send an IANA time zone name, such as Europe/Budapest.",
        ),
        (
            json!({"filter": {"property_id": "Nope", "operator": "is_empty"}}),
            "filter.property_id",
            "No property has this name or id, or two properties share the name: use the property id.",
        ),
    ];
    for (body, field, message) in cases {
        let (status, problem) = f.query(&db, body).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{problem}");
        assert_eq!(problem["errors"], json!({ field: [message] }));
    }
    let (status, problem) = f
        .api(
            "POST",
            &format!("docs/{db}/rows/trash"),
            json!({"row_ids": []}),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{problem}");
    assert_eq!(
        problem["errors"],
        json!({"row_ids": ["Send 1 to 100 row ids."]})
    );
}

#[tokio::test]
async fn dates_with_an_offset_are_stored_in_utc_and_filter() {
    let f = Fixture::new().await;
    let db = f.database("Meetings").await;
    f.property(&db, json!({"name": "When", "type": "date"}))
        .await;
    let row = f
        .row(
            &db,
            json!({"title": "Sync", "values": {"When": "2026-11-10T14:00:00+01:00"}}),
        )
        .await;
    assert_eq!(
        row["values"]["When"],
        json!({"start": "2026-11-10T13:00:00Z", "end": null})
    );
    let range = f
        .row(&db, json!({"title": "Trip", "values": {"When": {"start": "2026-11-10T00:30:00+02:00", "end": "2026-11-10T12:00:00-03:30"}}}))
        .await;
    assert_eq!(
        range["values"]["When"],
        json!({"start": "2026-11-09T22:30:00Z", "end": "2026-11-10T15:30:00Z"})
    );
    let day = f
        .row(
            &db,
            json!({"title": "Holiday", "values": {"When": "2026-11-11"}}),
        )
        .await;
    assert_eq!(
        day["values"]["When"],
        json!({"start": "2026-11-11", "end": null})
    );
    let (status, page) = f
        .query(
            &db,
            json!({"filter": {"property_id": "When", "operator": "on_or_after", "value": "2026-11-11T01:30:00+01:00"}}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    let titles: Vec<&str> = page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, ["Holiday"]);
}

#[tokio::test]
async fn formula_people_relations_and_unique_ids_read_as_other_values() {
    let f = Fixture::new().await;
    let projects = f.database("Projects").await;
    let tasks = f.database("Tasks").await;
    let apollo = f.row(&projects, json!({"title": "Apollo"})).await;
    let apollo_id = apollo["id"].as_str().unwrap();
    let owner = f
        .property(&tasks, json!({"name": "Owner", "type": "person"}))
        .await;
    f.property(
        &tasks,
        json!({"name": "Project", "type": "relation", "config": {"database_id": projects}}),
    )
    .await;
    f.property(
        &tasks,
        json!({"name": "Who", "type": "formula", "config": {"expression": format!("prop(\"{owner}\")")}}),
    )
    .await;
    f.property(
        &tasks,
        json!({"name": "Key", "type": "unique_id", "config": {"prefix": "TASK"}}),
    )
    .await;
    let row = f
        .row(
            &tasks,
            json!({"title": "Launch", "values": {"Owner": "me", "Project": [apollo_id]}}),
        )
        .await;
    let user = f.user.to_string();
    let expected = json!({
        "Owner": [{"id": user, "name": "Owner"}],
        "Who": [{"id": user, "name": "Owner"}],
        "Project": [{"id": apollo_id, "title": "Apollo"}],
        "Key": "TASK-1",
        "Tags": null,
    });
    assert_eq!(row["values"], expected);
    assert_eq!(row.get("hidden_links"), None);
    f.row(&tasks, json!({"title": "Land"})).await;

    let (status, page) = f
        .query(&tasks, json!({"sorts": [{"property_id": "Key"}]}))
        .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(page["items"][0]["values"], expected);
    assert_eq!(page["items"][1]["values"]["Key"], "TASK-2");
    assert_eq!(page["items"][1]["values"]["Project"], Value::Null);

    for value in [json!("TASK-2"), json!("task-2"), json!(2)] {
        let (status, page) = f
            .query(
                &tasks,
                json!({"filter": {"property_id": "Key", "operator": "=", "value": value}}),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{page}");
        assert_eq!(page["total"], 1, "{value}");
        assert_eq!(page["items"][0]["title"], "Land", "{value}");
    }
    let (status, problem) = f
        .query(
            &tasks,
            json!({"filter": {"property_id": "Key", "operator": "=", "value": "BUG-2"}}),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{problem}");
    assert!(problem["errors"]["filter.value"].is_array(), "{problem}");
}

#[tokio::test]
async fn relations_take_page_ids_only() {
    let f = Fixture::new().await;
    let projects = f.database("Projects").await;
    let tasks = f.database("Tasks").await;
    f.row(&projects, json!({"title": "Apollo"})).await;
    f.property(
        &tasks,
        json!({"name": "Project", "type": "relation", "config": {"database_id": projects}}),
    )
    .await;
    let (status, problem) = f
        .api(
            "POST",
            &format!("docs/{tasks}/rows"),
            json!({"title": "Bad", "values": {"Project": ["Apollo"]}}),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{problem}");
    assert!(problem["errors"]["values.Project"].is_array(), "{problem}");
}
