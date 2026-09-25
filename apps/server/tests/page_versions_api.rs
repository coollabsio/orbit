use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use orbit_platform::{AuthenticatedUser, Id, PasswordService, TestDatabase, TimestampMillis};
use orbit_server::auth_routes::CookieMode;
use orbit_server::page_routes::{PageState, page_router};
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::repositories::page_versions::SaveOrigin;
use orbit_server::repositories::pages::{PageChanges, PageRepository};
use orbit_server::repositories::workspaces::WorkspaceRepository;
use serde_json::{Value, json};
use tower::ServiceExt;

const MINUTE: i64 = 60 * 1_000;
const DAY: i64 = 24 * 60 * MINUTE;

struct Fixture {
    database: TestDatabase,
    identity: Arc<IdentityRepository>,
    pages: PageRepository,
    app: axum::Router,
    workspace_id: String,
    owner_cookie: String,
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
        let app = page_router(PageState::new(Arc::clone(&identity), CookieMode::secure()));
        Self {
            pages: PageRepository::new((*database).clone()),
            database,
            identity,
            app,
            workspace_id: setup.workspace_id.to_string(),
            owner_cookie: format!("__Host-orbit_session={}", setup.session.token),
            owner_id: setup.user_id,
        }
    }

    fn workspace(&self) -> Id {
        self.workspace_id.parse().unwrap()
    }

    fn pages_uri(&self) -> String {
        format!("/api/v1/workspaces/{}/pages", self.workspace_id)
    }

    fn page_uri(&self, page_id: &str) -> String {
        format!("{}/{page_id}", self.pages_uri())
    }

    fn versions_uri(&self, page_id: &str) -> String {
        format!("{}/versions", self.page_uri(page_id))
    }

    async fn call(&self, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
        call_as(self, &self.owner_cookie, method, uri, body).await
    }

    async fn create(&self, body: Value) -> Value {
        let (status, page) = self.call("POST", &self.pages_uri(), Some(body)).await;
        assert_eq!(status, StatusCode::CREATED, "{page}");
        page
    }

    async fn get(&self, page_id: &str) -> Value {
        let (status, page) = self.call("GET", &self.page_uri(page_id), None).await;
        assert_eq!(status, StatusCode::OK, "{page}");
        page
    }

    async fn patch(&self, page_id: &str, body: Value) -> Value {
        let mut body = body;
        body["expected_version"] = self.get(page_id).await["version"].clone();
        let (status, page) = self
            .call("PATCH", &self.page_uri(page_id), Some(body))
            .await;
        assert_eq!(status, StatusCode::OK, "{page}");
        page
    }

    /// Saves `changes` through the repository at `at` (the repository takes the clock as input).
    async fn edit_at(&self, page_id: &str, changes: PageChanges, at: i64) {
        let page_id: Id = page_id.parse().unwrap();
        let current = self
            .pages
            .get_page(self.workspace(), page_id, self.owner_id)
            .await
            .unwrap();
        self.pages
            .update_page(
                self.workspace(),
                page_id,
                self.owner_id,
                current.version,
                changes,
                "test",
                TimestampMillis::from_millis(at),
            )
            .await
            .unwrap();
    }

    /// `(kind, title, body text)` of the stored versions, newest first.
    async fn versions(&self, page_id: &str) -> Vec<(String, String, String)> {
        let rows: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT kind, title, content_json FROM page_versions WHERE page_id = ? \
             ORDER BY created_at DESC, id DESC",
        )
        .bind(page_id)
        .fetch_all(self.database.pool())
        .await
        .unwrap();
        rows.into_iter()
            .map(|(kind, title, content)| {
                let blocks: Vec<Value> = serde_json::from_str(&content).unwrap();
                (kind, title, text_of(&blocks))
            })
            .collect()
    }

    async fn insert_version(&self, page_id: &str, text: &str, created_at: i64) -> String {
        let id = Id::new_v7().to_string();
        sqlx::query(
            "INSERT INTO page_versions (id, workspace_id, page_id, title, icon, content_json, kind, \
             created_by, created_at) VALUES (?, ?, ?, ?, NULL, ?, 'auto', ?, ?)",
        )
        .bind(&id)
        .bind(&self.workspace_id)
        .bind(page_id)
        .bind(text)
        .bind(json!(paragraph(text)).to_string())
        .bind(self.owner_id.to_string())
        .bind(created_at)
        .execute(self.database.pool())
        .await
        .unwrap();
        id
    }

    /// Moves the page's creation an hour back, so its first real edit may snapshot.
    async fn backdate(&self, page_id: &str) {
        sqlx::query("UPDATE pages SET created_at = created_at - ? WHERE id = ?")
            .bind(60 * MINUTE)
            .bind(page_id)
            .execute(self.database.pool())
            .await
            .unwrap();
    }
}

fn paragraph(text: &str) -> Vec<Value> {
    vec![json!({"type": "paragraph", "content": [{"type": "text", "text": text}], "children": []})]
}

fn body(text: &str) -> PageChanges {
    PageChanges {
        content: Some(paragraph(text)),
        ..PageChanges::default()
    }
}

fn text_of(blocks: &[Value]) -> String {
    blocks
        .iter()
        .flat_map(|block| block["content"].as_array().cloned().unwrap_or_default())
        .filter_map(|inline| inline["text"].as_str().map(str::to_owned))
        .collect::<Vec<_>>()
        .join(" ")
}

fn id_of(value: &Value) -> &str {
    value["id"].as_str().unwrap()
}

fn version(kind: &str, title: &str, text: &str) -> (String, String, String) {
    (kind.to_owned(), title.to_owned(), text.to_owned())
}

#[tokio::test]
async fn edits_store_the_previous_state_at_most_every_ten_minutes() {
    let fixture = Fixture::new().await;
    let page = fixture.create(json!({"title": ""})).await;
    let id = id_of(&page);
    let created = fixture.get(id).await;
    let t0 = TimestampMillis::now().as_millis();
    assert!(created["version"] == 0);

    // A new page's first ten minutes leave no versions (and a blank page is never stored).
    fixture
        .edit_at(
            id,
            PageChanges {
                title: Some("Plan".to_owned()),
                ..PageChanges::default()
            },
            t0 + MINUTE,
        )
        .await;
    fixture.edit_at(id, body("one"), t0 + 2 * MINUTE).await;
    assert!(fixture.versions(id).await.is_empty());

    // Ten minutes after creation, the state before the edit is stored.
    fixture.edit_at(id, body("two"), t0 + 12 * MINUTE).await;
    assert_eq!(fixture.versions(id).await, [version("auto", "Plan", "one")]);
    // Then at most one per ten minutes.
    fixture.edit_at(id, body("three"), t0 + 15 * MINUTE).await;
    fixture.edit_at(id, body("four"), t0 + 21 * MINUTE).await;
    assert_eq!(fixture.versions(id).await.len(), 1);
    fixture.edit_at(id, body("five"), t0 + 23 * MINUTE).await;
    assert_eq!(
        fixture.versions(id).await,
        [
            version("auto", "Plan", "four"),
            version("auto", "Plan", "one")
        ]
    );

    // Saves that change neither title nor content never snapshot, even after a long pause.
    fixture
        .edit_at(
            id,
            PageChanges {
                icon: Some(Some("🚀".to_owned())),
                content: Some(paragraph("five")),
                ..PageChanges::default()
            },
            t0 + 60 * MINUTE,
        )
        .await;
    assert_eq!(fixture.versions(id).await.len(), 2);
    // A title change counts, and the version carries the icon of that state.
    fixture
        .edit_at(
            id,
            PageChanges {
                title: Some("Launch".to_owned()),
                ..PageChanges::default()
            },
            t0 + 61 * MINUTE,
        )
        .await;
    assert_eq!(
        fixture.versions(id).await[0],
        version("auto", "Plan", "five")
    );
    let (icon, created_by): (Option<String>, String) = sqlx::query_as(
        "SELECT icon, created_by FROM page_versions WHERE page_id = ? ORDER BY created_at DESC LIMIT 1",
    )
    .bind(id)
    .fetch_one(fixture.database.pool())
    .await
    .unwrap();
    assert_eq!(icon.as_deref(), Some("🚀"));
    assert_eq!(created_by, fixture.owner_id.to_string());
}

#[tokio::test]
async fn imports_store_the_imported_state_and_identical_states_are_not_repeated() {
    let fixture = Fixture::new().await;
    let page = fixture.create(json!({"title": "Imported"})).await;
    let id = id_of(&page);
    let t0 = TimestampMillis::now().as_millis();
    fixture
        .edit_at(
            id,
            PageChanges {
                origin: SaveOrigin::Import,
                ..body("from notion")
            },
            t0 + MINUTE,
        )
        .await;
    assert_eq!(
        fixture.versions(id).await,
        [version("import", "Imported", "from notion")]
    );
    // Much later, the state before the edit equals the import version: nothing new is stored.
    fixture.edit_at(id, body("edited"), t0 + 90 * MINUTE).await;
    assert_eq!(fixture.versions(id).await.len(), 1);
    fixture
        .edit_at(id, body("edited again"), t0 + 120 * MINUTE)
        .await;
    assert_eq!(
        fixture.versions(id).await[0],
        version("auto", "Imported", "edited")
    );
}

#[tokio::test]
async fn versions_are_listed_read_and_restored_through_the_api() {
    let fixture = Fixture::new().await;
    let page = fixture.create(json!({"title": "Roadmap"})).await;
    let id = id_of(&page).to_owned();
    fixture
        .patch(&id, json!({"content": paragraph("First")}))
        .await;
    let (status, empty) = fixture.call("GET", &fixture.versions_uri(&id), None).await;
    assert_eq!(status, StatusCode::OK, "{empty}");
    assert_eq!(empty, json!({"items": [], "next_cursor": null}));

    // An hour later the next edit stores the state before it; the one after that is throttled.
    fixture.backdate(&id).await;
    fixture
        .patch(&id, json!({"content": paragraph("Second")}))
        .await;
    fixture
        .patch(&id, json!({"content": paragraph("Third")}))
        .await;
    assert_eq!(
        fixture.versions(&id).await,
        [version("auto", "Roadmap", "First")]
    );

    let (status, list) = fixture.call("GET", &fixture.versions_uri(&id), None).await;
    assert_eq!(status, StatusCode::OK, "{list}");
    let items = list["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    let first = &items[0];
    assert_eq!(first["kind"], "auto");
    assert_eq!(first["title"], "Roadmap");
    assert_eq!(first["page_id"], id);
    assert_eq!(
        first["created_by"],
        json!({"id": fixture.owner_id.to_string(), "display_name": "Owner"})
    );
    assert!(first.get("content").is_none());
    let version_id = id_of(first).to_owned();

    let version_uri = format!("{}/{version_id}", fixture.versions_uri(&id));
    let (status, detail) = fixture.call("GET", &version_uri, None).await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    assert_eq!(detail["content"], json!(paragraph("First")));

    // A stale version conflicts like every page write.
    let current = fixture.get(&id).await;
    let (status, conflict) = fixture
        .call(
            "POST",
            &format!("{version_uri}/restore"),
            Some(json!({"expected_version": current["version"].as_u64().unwrap() - 1})),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{conflict}");
    assert_eq!(conflict["code"], "conflict");
    assert_eq!(fixture.versions(&id).await.len(), 1);

    let (status, restored) = fixture
        .call(
            "POST",
            &format!("{version_uri}/restore"),
            Some(json!({"expected_version": current["version"]})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{restored}");
    assert_eq!(restored["content"], json!(paragraph("First")));
    assert_eq!(
        restored["version"].as_u64(),
        Some(current["version"].as_u64().unwrap() + 1)
    );
    assert_eq!(fixture.get(&id).await["content"], json!(paragraph("First")));
    // The state before the restore is kept, newest first.
    assert_eq!(
        fixture.versions(&id).await,
        [
            version("restore", "Roadmap", "Third"),
            version("auto", "Roadmap", "First")
        ]
    );
    // Search follows the restored text.
    let text: String = sqlx::query_scalar("SELECT content_text FROM pages WHERE id = ?")
        .bind(&id)
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    assert_eq!(text, "First");
    let audited: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events WHERE action = 'page.restored_version' \
         AND resource_id = ? AND json_extract(metadata_json, '$.version_id') = ?",
    )
    .bind(&id)
    .bind(&version_id)
    .fetch_one(fixture.database.pool())
    .await
    .unwrap();
    assert_eq!(audited, 1);

    // Unknown versions, and versions of another page, are not found on a visible page.
    let other = fixture.create(json!({"title": "Other"})).await;
    for uri in [
        format!("{}/{}", fixture.versions_uri(&id), Id::new_v7()),
        format!("{}/not-an-id", fixture.versions_uri(&id)),
        format!("{}/{version_id}", fixture.versions_uri(id_of(&other))),
    ] {
        let (status, problem) = fixture.call("GET", &uri, None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri}: {problem}");
        assert_eq!(problem["code"], "page_version_not_found");
    }

    // A trashed page has no reachable history.
    let version = fixture.get(&id).await["version"].clone();
    let (status, _) = fixture
        .call(
            "DELETE",
            &format!("{}?expected_version={version}", fixture.page_uri(&id)),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    for (method, uri, body) in [
        ("GET", fixture.versions_uri(&id), None),
        ("GET", version_uri.clone(), None),
        (
            "POST",
            format!("{version_uri}/restore"),
            Some(json!({"expected_version": 99})),
        ),
    ] {
        let (status, problem) = fixture.call(method, &uri, body).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{method} {uri}: {problem}");
        assert_eq!(problem["code"], "page_not_found");
    }
}

#[tokio::test]
async fn version_lists_page_with_cursors() {
    let fixture = Fixture::new().await;
    let page = fixture.create(json!({"title": "Log"})).await;
    let id = id_of(&page);
    let now = TimestampMillis::now().as_millis();
    for index in 0..5 {
        fixture
            .insert_version(id, &format!("v{index}"), now - (5 - index) * MINUTE)
            .await;
    }
    let mut titles = Vec::new();
    let mut cursor: Option<String> = None;
    let mut pages = 0;
    loop {
        let uri = match &cursor {
            Some(cursor) => format!("{}?limit=2&cursor={cursor}", fixture.versions_uri(id)),
            None => format!("{}?limit=2", fixture.versions_uri(id)),
        };
        let (status, list) = fixture.call("GET", &uri, None).await;
        assert_eq!(status, StatusCode::OK, "{list}");
        pages += 1;
        titles.extend(
            list["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|item| item["title"].as_str().unwrap().to_owned()),
        );
        match list["next_cursor"].as_str() {
            Some(next) => cursor = Some(next.to_owned()),
            None => break,
        }
    }
    assert_eq!(pages, 3);
    assert_eq!(titles, ["v4", "v3", "v2", "v1", "v0"]);

    for (query, status, code) in [
        (
            format!("cursor={}", Id::new_v7()),
            StatusCode::BAD_REQUEST,
            "invalid_cursor",
        ),
        (
            "cursor=garbage".to_owned(),
            StatusCode::BAD_REQUEST,
            "invalid_cursor",
        ),
        (
            "limit=0".to_owned(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "validation_failed",
        ),
        (
            "limit=101".to_owned(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "validation_failed",
        ),
    ] {
        let (actual, problem) = fixture
            .call(
                "GET",
                &format!("{}?{query}", fixture.versions_uri(id)),
                None,
            )
            .await;
        assert_eq!(actual, status, "{query}: {problem}");
        assert_eq!(problem["code"], code, "{query}");
    }
}

#[tokio::test]
async fn versions_follow_page_visibility() {
    let fixture = Fixture::new().await;
    let (_, member_cookie) =
        add_member(&fixture, &fixture.workspace_id, "member@example.com").await;
    let shared = fixture.create(json!({"title": "Shared"})).await;
    let private = fixture
        .create(json!({"private": true, "title": "Diary"}))
        .await;
    let now = TimestampMillis::now().as_millis();
    fixture
        .insert_version(id_of(&shared), "Shared before", now - MINUTE)
        .await;
    let secret = fixture
        .insert_version(id_of(&private), "Secret", now - MINUTE)
        .await;

    // Teamspace history is every member's.
    let (status, list) = call_as(
        &fixture,
        &member_cookie,
        "GET",
        &fixture.versions_uri(id_of(&shared)),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert_eq!(list["items"].as_array().unwrap().len(), 1);

    // Someone else's private page answers exactly like a page that does not exist.
    let random = Id::new_v7().to_string();
    let requests = |page: &str, version: &str| {
        vec![
            ("GET", fixture.versions_uri(page), None),
            (
                "GET",
                format!("{}/{version}", fixture.versions_uri(page)),
                None,
            ),
            (
                "POST",
                format!("{}/{version}/restore", fixture.versions_uri(page)),
                Some(json!({"expected_version": 0})),
            ),
        ]
    };
    for ((method, uri, body), (_, random_uri, random_body)) in requests(id_of(&private), &secret)
        .into_iter()
        .zip(requests(&random, &secret))
    {
        let hidden = call_as(&fixture, &member_cookie, method, &uri, body).await;
        let missing = call_as(&fixture, &member_cookie, method, &random_uri, random_body).await;
        assert_eq!(
            hidden.0,
            StatusCode::NOT_FOUND,
            "{method} {uri}: {}",
            hidden.1
        );
        assert_eq!(hidden.1["code"], "page_not_found");
        assert_eq!(
            (hidden.0, &hidden.1["code"]),
            (missing.0, &missing.1["code"])
        );
    }
    assert_eq!(fixture.get(id_of(&private)).await["title"], "Diary");
    assert_eq!(fixture.versions(id_of(&private)).await.len(), 1);
}

#[tokio::test]
async fn versions_are_isolated_between_workspaces() {
    let fixture = Fixture::new().await;
    let page = fixture.create(json!({"title": "Orbit only"})).await;
    let version_id = fixture
        .insert_version(id_of(&page), "Before", TimestampMillis::now().as_millis())
        .await;
    let foreign = WorkspaceRepository::new((*fixture.database).clone())
        .create(
            fixture.owner_id,
            "Foreign".to_owned(),
            "versions-test",
            TimestampMillis::now(),
        )
        .await
        .unwrap()
        .id
        .to_string();
    let (_, outsider_cookie) = add_member(&fixture, &foreign, "outsider@example.com").await;
    let foreign_versions = format!(
        "/api/v1/workspaces/{foreign}/pages/{}/versions",
        id_of(&page)
    );
    // An outsider on the real path, and the owner through the other workspace's path.
    for (cookie, uri) in [
        (&outsider_cookie, fixture.versions_uri(id_of(&page))),
        (&outsider_cookie, foreign_versions.clone()),
        (&fixture.owner_cookie, foreign_versions.clone()),
        (
            &fixture.owner_cookie,
            format!("{foreign_versions}/{version_id}"),
        ),
    ] {
        let (status, problem) = call_as(&fixture, cookie, "GET", &uri, None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri}: {problem}");
        assert_eq!(problem["code"], "page_not_found");
    }
    let (status, problem) = call_as(
        &fixture,
        &fixture.owner_cookie,
        "POST",
        &format!("{foreign_versions}/{version_id}/restore"),
        Some(json!({"expected_version": 0})),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{problem}");
}

#[tokio::test]
async fn duplicates_start_without_history_and_purges_take_it_along() {
    let fixture = Fixture::new().await;
    let page = fixture.create(json!({"title": "Spec"})).await;
    let id = id_of(&page);
    fixture
        .insert_version(id, "Old spec", TimestampMillis::now().as_millis())
        .await;
    let (status, copy) = fixture
        .call(
            "POST",
            &format!("{}/duplicate", fixture.page_uri(id)),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{copy}");
    assert!(fixture.versions(id_of(&copy)).await.is_empty());

    let version = fixture.get(id).await["version"].clone();
    fixture
        .call(
            "DELETE",
            &format!("{}?expected_version={version}", fixture.page_uri(id)),
            None,
        )
        .await;
    let version: i64 = sqlx::query_scalar("SELECT version FROM pages WHERE id = ?")
        .bind(id)
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    let (status, _) = fixture
        .call(
            "DELETE",
            &format!(
                "{}/permanent?expected_version={version}",
                fixture.page_uri(id)
            ),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM page_versions WHERE page_id = ?")
        .bind(id)
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    assert_eq!(left, 0);
}

#[tokio::test]
async fn retention_keeps_a_month_then_one_per_day_for_a_year_and_the_newest_twenty() {
    let fixture = Fixture::new().await;
    let busy = fixture.create(json!({"title": "Busy"})).await;
    let quiet = fixture.create(json!({"title": "Quiet"})).await;
    // Anchor "now" at noon UTC so day buckets are easy to reason about.
    let now = (TimestampMillis::now().as_millis() / DAY) * DAY + 12 * 60 * MINUTE;
    // Busy page: three versions a day (01:00, 05:00, 09:00 UTC) for 400 days; "h3" is 09:00.
    for day in 0..400 {
        for hour in [3, 7, 11] {
            fixture
                .insert_version(
                    id_of(&busy),
                    &format!("d{day}h{hour}"),
                    now - day * DAY - hour * 60 * MINUTE,
                )
                .await;
        }
    }
    // Quiet page: five old versions, two years back, all kept as its newest twenty.
    for index in 0..5 {
        fixture
            .insert_version(
                id_of(&quiet),
                &format!("old{index}"),
                now - 730 * DAY - index * DAY,
            )
            .await;
    }
    WorkspaceRepository::new((*fixture.database).clone())
        .purge_retention(TimestampMillis::from_millis(now))
        .await
        .unwrap();

    let busy_rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT title, created_at FROM page_versions WHERE page_id = ? ORDER BY created_at DESC",
    )
    .bind(id_of(&busy))
    .fetch_all(fixture.database.pool())
    .await
    .unwrap();
    let recent = busy_rows
        .iter()
        .filter(|(_, at)| *at > now - 30 * DAY)
        .count();
    let older: Vec<&(String, i64)> = busy_rows
        .iter()
        .filter(|(_, at)| *at <= now - 30 * DAY)
        .collect();
    // Days 0..=29 keep all three.
    assert_eq!(recent, 30 * 3);
    // Then one per day (the day's newest) until a year back.
    let mut days: Vec<i64> = older.iter().map(|(_, at)| at / DAY).collect();
    days.dedup();
    assert_eq!(days.len(), older.len(), "one version per day");
    assert!(older.iter().all(|(title, _)| title.ends_with("h3")));
    assert!(busy_rows.iter().all(|(_, at)| *at > now - 365 * DAY));
    assert_eq!(older.len(), 335);

    let quiet_left: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM page_versions WHERE page_id = ?")
            .bind(id_of(&quiet))
            .fetch_one(fixture.database.pool())
            .await
            .unwrap();
    assert_eq!(quiet_left, 5);
}

async fn call_as(
    fixture: &Fixture,
    cookie: &str,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::COOKIE, cookie);
    let request = match body {
        Some(value) => builder
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(value.to_string())),
        None => builder.body(Body::empty()),
    }
    .unwrap();
    let response = fixture.app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    (status, value)
}

async fn add_member(fixture: &Fixture, workspace_id: &str, email: &str) -> (Id, String) {
    let id = Id::new_v7();
    let now = TimestampMillis::now();
    sqlx::query(
        "INSERT INTO users (id, email, normalized_email, display_name, password_hash, created_at, updated_at) \
         VALUES (?, ?, ?, 'Member', 'unused', ?, ?)",
    )
    .bind(id.to_string())
    .bind(email)
    .bind(email)
    .bind(now.as_millis())
    .bind(now.as_millis())
    .execute(fixture.database.pool())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, updated_at) \
         VALUES (?, ?, ?, 'member', 0, ?, ?)",
    )
    .bind(Id::new_v7().to_string())
    .bind(workspace_id)
    .bind(id.to_string())
    .bind(now.as_millis())
    .bind(now.as_millis())
    .execute(fixture.database.pool())
    .await
    .unwrap();
    let session = fixture
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
