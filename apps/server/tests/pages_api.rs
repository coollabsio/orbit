use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use orbit_platform::{AuthenticatedUser, Id, PasswordService, TestDatabase, TimestampMillis};
use orbit_server::auth_routes::CookieMode;
use orbit_server::page_routes::{PageState, page_router};
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::repositories::workspaces::WorkspaceRepository;
use orbit_server::teamspace_routes::{TeamspaceState, teamspace_router};
use serde_json::{Value, json};
use tower::ServiceExt;

const RETENTION_MILLIS: i64 = 30 * 24 * 60 * 60 * 1_000;

struct Fixture {
    database: TestDatabase,
    identity: Arc<IdentityRepository>,
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
        let app = page_router(PageState::new(Arc::clone(&identity), CookieMode::secure())).merge(
            teamspace_router(TeamspaceState::new(
                Arc::clone(&identity),
                CookieMode::secure(),
            )),
        );
        Self {
            database,
            identity,
            app,
            workspace_id: setup.workspace_id.to_string(),
            owner_cookie: format!("__Host-orbit_session={}", setup.session.token),
            owner_id: setup.user_id,
        }
    }

    fn pages_uri(&self) -> String {
        format!("/api/v1/workspaces/{}/pages", self.workspace_id)
    }

    fn page_uri(&self, page_id: &str) -> String {
        format!("{}/{page_id}", self.pages_uri())
    }

    async fn call(&self, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
        call_as(self, &self.owner_cookie, method, uri, body).await
    }

    async fn create(&self, body: Value) -> Value {
        let (status, page) = self.call("POST", &self.pages_uri(), Some(body)).await;
        assert_eq!(status, StatusCode::CREATED, "{page}");
        page
    }

    async fn tree(&self) -> Vec<Value> {
        let (status, list) = self.call("GET", &self.pages_uri(), None).await;
        assert_eq!(status, StatusCode::OK, "{list}");
        list["items"].as_array().unwrap().clone()
    }

    async fn get(&self, page_id: &str) -> Value {
        let (status, page) = self.call("GET", &self.page_uri(page_id), None).await;
        assert_eq!(status, StatusCode::OK, "{page}");
        page
    }

    async fn move_to(
        &self,
        page: &Value,
        parent_id: Option<&str>,
        position: i64,
    ) -> (StatusCode, Value) {
        self.call(
            "POST",
            &format!("{}/move", self.page_uri(id_of(page))),
            Some(json!({
                "expected_version": page["version"],
                "parent_id": parent_id,
                "position": position
            })),
        )
        .await
    }

    async fn trash(&self, page_id: &str) {
        let version = self.get(page_id).await["version"].as_u64().unwrap();
        let (status, body) = self
            .call(
                "DELETE",
                &format!("{}?expected_version={version}", self.page_uri(page_id)),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    }

    async fn trash_ids(&self) -> Vec<String> {
        let (status, trash) = self
            .call("GET", &format!("{}/trash", self.pages_uri()), None)
            .await;
        assert_eq!(status, StatusCode::OK, "{trash}");
        let mut ids = trash["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| id_of(item).to_owned())
            .collect::<Vec<_>>();
        ids.sort();
        ids
    }

    async fn restore(&self, page_id: &str) -> (StatusCode, Value) {
        let version: i64 = sqlx::query_scalar("SELECT version FROM pages WHERE id = ?")
            .bind(page_id)
            .fetch_one(self.database.pool())
            .await
            .unwrap();
        self.call(
            "POST",
            &format!("{}/restore", self.page_uri(page_id)),
            Some(json!({"expected_version": version})),
        )
        .await
    }

    async fn teamspace_ids(&self) -> Vec<String> {
        let (status, list) = self
            .call(
                "GET",
                &format!("/api/v1/workspaces/{}/teamspaces", self.workspace_id),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{list}");
        list["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| id_of(item).to_owned())
            .collect()
    }

    /// `(title, teamspace_id, owner_id)` of every stored page, by title.
    async fn spaces(&self) -> Vec<(String, Option<String>, Option<String>)> {
        sqlx::query_as("SELECT title, teamspace_id, owner_id FROM pages ORDER BY title")
            .fetch_all(self.database.pool())
            .await
            .unwrap()
    }

    fn favorites_uri(&self) -> String {
        format!("{}/favorites", self.pages_uri())
    }

    fn favorite_uri(&self, page_id: &str) -> String {
        format!("{}/favorite", self.page_uri(page_id))
    }

    async fn favorites_as(&self, cookie: &str) -> Vec<(String, i64)> {
        let (status, list) = call_as(self, cookie, "GET", &self.favorites_uri(), None).await;
        assert_eq!(status, StatusCode::OK, "{list}");
        list["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| {
                (
                    item["page_id"].as_str().unwrap().to_owned(),
                    item["position"].as_i64().unwrap(),
                )
            })
            .collect()
    }

    async fn favorite_ids(&self) -> Vec<String> {
        favorite_ids(self.favorites_as(&self.owner_cookie).await)
    }

    async fn favorite(&self, page_id: &str) -> Value {
        let (status, favorite) = self.call("PUT", &self.favorite_uri(page_id), None).await;
        assert_eq!(status, StatusCode::OK, "{favorite}");
        favorite
    }

    async fn move_favorite(&self, page_id: &str, position: i64) -> (StatusCode, Value) {
        self.call(
            "POST",
            &format!("{}/{page_id}/move", self.favorites_uri()),
            Some(json!({"position": position})),
        )
        .await
    }

    async fn set_text(&self, page: &Value, text: &str) {
        let version = self.get(id_of(page)).await["version"].as_u64().unwrap();
        let (status, body) = self
            .call(
                "PATCH",
                &self.page_uri(id_of(page)),
                Some(json!({
                    "expected_version": version,
                    "content": [{"type": "paragraph", "content": [{"type": "text", "text": text}]}]
                })),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }

    async fn purge(&self, page_id: &str) {
        let version: i64 = sqlx::query_scalar("SELECT version FROM pages WHERE id = ?")
            .bind(page_id)
            .fetch_one(self.database.pool())
            .await
            .unwrap();
        let (status, body) = self
            .call(
                "DELETE",
                &format!(
                    "{}/permanent?expected_version={version}",
                    self.page_uri(page_id)
                ),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    }

    /// `(parent_id, trashed_with)` of a stored page.
    async fn get_raw(&self, page_id: &str) -> (Option<String>, Option<String>) {
        sqlx::query_as("SELECT parent_id, trashed_with FROM pages WHERE id = ?")
            .bind(page_id)
            .fetch_one(self.database.pool())
            .await
            .unwrap()
    }

    async fn search(&self, query: &str) -> (StatusCode, Value) {
        self.call(
            "GET",
            &format!("{}/search?q={query}", self.pages_uri()),
            None,
        )
        .await
    }
}

#[tokio::test]
async fn pages_can_be_created_read_updated_and_listed_in_tree_order() {
    let fixture = Fixture::new().await;
    let untitled = fixture.create(json!({})).await;
    assert_eq!(untitled["title"], "");
    assert_eq!(untitled["parent_id"], Value::Null);
    assert_eq!(untitled["icon"], Value::Null);
    assert_eq!(untitled["content"], json!([]));
    assert_eq!(untitled["position"], 0);
    assert_eq!(untitled["version"], 0);
    assert_eq!(untitled["deleted_at"], Value::Null);
    assert_eq!(untitled["creator_id"], fixture.owner_id.to_string());
    assert_eq!(untitled["updated_by"], fixture.owner_id.to_string());

    let guide = fixture
        .create(json!({"title": "Guide", "icon": "📘"}))
        .await;
    assert_eq!(guide["position"], 1);
    let appended = fixture
        .create(json!({"parent_id": id_of(&guide), "title": "Appended"}))
        .await;
    let first = fixture
        .create(json!({"parent_id": id_of(&guide), "title": "First", "position": 0}))
        .await;
    let clamped = fixture
        .create(json!({"parent_id": id_of(&guide), "title": "Last", "position": 99}))
        .await;
    assert_eq!(clamped["position"], 2);

    let tree = fixture.tree().await;
    let titles = tree
        .iter()
        .map(|page| page["title"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(titles, ["", "Guide", "First", "Appended", "Last"]);
    assert_eq!(
        tree.iter()
            .map(|page| page["position"].as_i64().unwrap())
            .collect::<Vec<_>>(),
        [0, 1, 0, 1, 2]
    );
    assert!(tree[0].get("content").is_none());
    assert_eq!(tree[2]["parent_id"], guide["id"]);
    assert_eq!(fixture.get(id_of(&appended)).await["position"], 1);
    assert_eq!(fixture.get(id_of(&first)).await["title"], "First");

    let content = json!([
        {"id": "a", "type": "paragraph", "content": [{"type": "text", "text": "Hello", "styles": {}}], "children": []},
        {"id": "b", "type": "heading", "content": [{"type": "text", "text": "World", "styles": {}}], "children": []}
    ]);
    let (status, updated) = fixture
        .call(
            "PATCH",
            &fixture.page_uri(id_of(&guide)),
            Some(json!({
                "expected_version": 0,
                "title": "Handbook",
                "cover_url": "https://example.com/cover.png",
                "cover_position": "50,12.5",
                "content": content
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["title"], "Handbook");
    assert_eq!(updated["icon"], "📘");
    assert_eq!(updated["cover_url"], "https://example.com/cover.png");
    assert_eq!(updated["cover_position"], "50,12.5");
    assert_eq!(updated["content"], content);
    assert_eq!(updated["version"], 1);
    assert_eq!(fixture.get(id_of(&guide)).await, updated);
    let text: String = sqlx::query_scalar("SELECT content_text FROM pages WHERE id = ?")
        .bind(id_of(&guide))
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    assert_eq!(text, "Hello World");

    // Absent fields stay; null clears nullable fields.
    let (status, cleared) = fixture
        .call(
            "PATCH",
            &fixture.page_uri(id_of(&guide)),
            Some(json!({"expected_version": 1, "icon": null, "cover_url": null})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{cleared}");
    assert_eq!(cleared["icon"], Value::Null);
    assert_eq!(cleared["cover_url"], Value::Null);
    assert_eq!(cleared["cover_position"], "50,12.5");
    assert_eq!(cleared["title"], "Handbook");
    assert_eq!(cleared["content"], content);
    assert_eq!(cleared["version"], 2);
}

#[tokio::test]
async fn page_input_is_validated() {
    let fixture = Fixture::new().await;
    let page = fixture.create(json!({"title": "Target"})).await;
    let uri = fixture.page_uri(id_of(&page));
    for (body, field) in [
        (
            json!({"expected_version": 0, "title": "x".repeat(501)}),
            "title",
        ),
        (
            json!({"expected_version": 0, "icon": "x".repeat(65)}),
            "icon",
        ),
        (
            json!({"expected_version": 0, "cover_url": "javascript:alert(1)"}),
            "cover_url",
        ),
        (
            json!({"expected_version": 0, "cover_url": format!("https://example.com/{}", "a".repeat(2048))}),
            "cover_url",
        ),
        (
            json!({"expected_version": 0, "cover_position": "101,5"}),
            "cover_position",
        ),
        (
            json!({"expected_version": 0, "cover_position": "50"}),
            "cover_position",
        ),
        (
            json!({"expected_version": 0, "cover_position": "-1,5"}),
            "cover_position",
        ),
        (
            json!({"expected_version": 0, "content": {"type": "paragraph"}}),
            "content",
        ),
    ] {
        let (status, problem) = fixture.call("PATCH", &uri, Some(body)).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{problem}");
        assert_eq!(problem["code"], "validation_failed");
        assert_eq!(problem["detail"], field);
    }
    let (status, problem) = fixture
        .call(
            "PATCH",
            &uri,
            Some(json!({"expected_version": 0, "unknown": true})),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{problem}");
    let (status, _) = fixture
        .call(
            "POST",
            &fixture.pages_uri(),
            Some(json!({"title": "x".repeat(501)})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _) = fixture
        .call("POST", &fixture.pages_uri(), Some(json!({"position": -1})))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, problem) = fixture
        .call(
            "POST",
            &fixture.pages_uri(),
            Some(json!({"parent_id": Id::new_v7().to_string()})),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(problem["code"], "page_not_found");
    let (status, problem) = fixture
        .call(
            "POST",
            &format!("{uri}/move"),
            Some(json!({"expected_version": 0, "position": 0})),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "move requires parent_id: {problem}"
    );

    let huge = json!([{"type": "paragraph", "content": [{"type": "text", "text": "x".repeat(1024 * 1024)}]}]);
    let (status, problem) = fixture
        .call(
            "PATCH",
            &uri,
            Some(json!({"expected_version": 0, "content": huge})),
        )
        .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE, "{problem}");
    assert_eq!(problem["code"], "request_too_large");
    assert_eq!(fixture.get(id_of(&page)).await["version"], 0);
}

#[tokio::test]
async fn moves_renumber_siblings_and_reject_cycles() {
    let fixture = Fixture::new().await;
    let a = fixture.create(json!({"title": "A"})).await;
    let b = fixture.create(json!({"title": "B"})).await;
    let c = fixture.create(json!({"title": "C"})).await;

    let (status, moved) = fixture.move_to(&c, None, 0).await;
    assert_eq!(status, StatusCode::OK, "{moved}");
    assert_eq!(moved["position"], 0);
    assert_eq!(moved["version"], 1);
    assert_eq!(titles(&fixture.tree().await), ["C", "A", "B"]);
    // Sibling renumbering does not bump versions, so concurrent content saves keep working.
    assert_eq!(fixture.get(id_of(&a)).await["version"], 0);

    let (status, nested) = fixture.move_to(&a, Some(id_of(&b)), 0).await;
    assert_eq!(status, StatusCode::OK, "{nested}");
    assert_eq!(nested["parent_id"], b["id"]);
    let tree = fixture.tree().await;
    assert_eq!(titles(&tree), ["C", "B", "A"]);
    assert_eq!(
        tree.iter()
            .map(|page| page["position"].as_i64().unwrap())
            .collect::<Vec<_>>(),
        [0, 1, 0]
    );

    let grandchild = fixture
        .create(json!({"parent_id": id_of(&a), "title": "A1"}))
        .await;
    let b = fixture.get(id_of(&b)).await;
    for parent in [id_of(&b), id_of(&nested), id_of(&grandchild)] {
        let (status, problem) = fixture.move_to(&b, Some(parent), 0).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{problem}");
        assert_eq!(problem["detail"], "parent_id");
    }
    assert_eq!(fixture.get(id_of(&b)).await["parent_id"], Value::Null);

    // Moving within the same parent to the end.
    let c = fixture.get(id_of(&c)).await;
    let (status, last) = fixture.move_to(&c, None, 10).await;
    assert_eq!(status, StatusCode::OK, "{last}");
    assert_eq!(last["position"], 1);
    let tree = fixture.tree().await;
    let roots = tree
        .iter()
        .filter(|page| page["parent_id"].is_null())
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(titles(&roots), ["B", "C"]);
    assert_eq!(tree.len(), 4);
}

#[tokio::test]
async fn stale_versions_return_conflicts_with_a_refresh_link() {
    let fixture = Fixture::new().await;
    let page = fixture.create(json!({"title": "Draft"})).await;
    let uri = fixture.page_uri(id_of(&page));
    let (status, _) = fixture
        .call(
            "PATCH",
            &uri,
            Some(json!({"expected_version": 0, "title": "First save"})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, problem) = fixture
        .call(
            "PATCH",
            &uri,
            Some(json!({"expected_version": 0, "title": "Second tab"})),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{problem}");
    assert_eq!(problem["code"], "conflict");
    assert_eq!(problem["conflict"]["current_version"], 1);
    assert_eq!(problem["conflict"]["current"]["title"], "First save");
    assert_eq!(problem["conflict"]["refresh"], uri);

    let (status, problem) = fixture.move_to(&page, None, 0).await;
    assert_eq!(status, StatusCode::CONFLICT, "{problem}");
    let (status, _) = fixture
        .call("DELETE", &format!("{uri}?expected_version=0"), None)
        .await;
    assert_eq!(status, StatusCode::CONFLICT);

    fixture.trash(id_of(&page)).await;
    let (status, problem) = fixture
        .call(
            "POST",
            &format!("{uri}/restore"),
            Some(json!({"expected_version": 0})),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{problem}");
    assert_eq!(
        problem["conflict"]["refresh"],
        format!("{}/trash", fixture.pages_uri())
    );
}

#[tokio::test]
async fn trash_takes_the_live_subtree_and_restore_brings_back_that_batch() {
    let fixture = Fixture::new().await;
    let root = fixture.create(json!({"title": "Root"})).await;
    let parent = fixture.create(json!({"title": "Parent"})).await;
    let child = fixture
        .create(json!({"parent_id": id_of(&parent), "title": "Child"}))
        .await;
    let grandchild = fixture
        .create(json!({"parent_id": id_of(&child), "title": "Grandchild"}))
        .await;
    let early = fixture
        .create(json!({"parent_id": id_of(&parent), "title": "Early"}))
        .await;

    fixture.trash(id_of(&early)).await;
    fixture.trash(id_of(&parent)).await;
    assert_eq!(titles(&fixture.tree().await), ["Root"]);
    for page in [&parent, &child, &grandchild, &early] {
        let (status, problem) = fixture
            .call("GET", &fixture.page_uri(id_of(page)), None)
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(problem["code"], "page_not_found");
    }
    let mut direct = vec![id_of(&parent).to_owned(), id_of(&early).to_owned()];
    direct.sort();
    assert_eq!(fixture.trash_ids().await, direct);
    let (_, trash) = fixture
        .call("GET", &format!("{}/trash", fixture.pages_uri()), None)
        .await;
    assert!(trash["items"][0]["deleted_at"].is_string());

    // Early was trashed on its own, and its parent is still in the trash: it comes back at root.
    let (status, restored) = fixture.restore(id_of(&early)).await;
    assert_eq!(status, StatusCode::OK, "{restored}");
    assert_eq!(restored["parent_id"], Value::Null);
    assert_eq!(restored["deleted_at"], Value::Null);
    assert_eq!(titles(&fixture.tree().await), ["Root", "Early"]);
    assert_eq!(fixture.trash_ids().await, [id_of(&parent)]);

    let (status, restored) = fixture.restore(id_of(&parent)).await;
    assert_eq!(status, StatusCode::OK, "{restored}");
    assert_eq!(restored["position"], 1);
    assert_eq!(
        titles(&fixture.tree().await),
        ["Root", "Parent", "Early", "Child", "Grandchild"]
    );
    assert_eq!(fixture.get(id_of(&child)).await["parent_id"], parent["id"]);
    assert_eq!(
        fixture.get(id_of(&grandchild)).await["parent_id"],
        child["id"]
    );
    assert!(fixture.trash_ids().await.is_empty());
    assert_eq!(fixture.get(id_of(&root)).await["position"], 0);

    // A restored page cannot be restored again, and expired trash is gone.
    let (status, _) = fixture.restore(id_of(&parent)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    fixture.trash(id_of(&child)).await;
    sqlx::query("UPDATE pages SET deleted_at = ? WHERE deleted_at IS NOT NULL")
        .bind(TimestampMillis::now().as_millis() - RETENTION_MILLIS - 1_000)
        .execute(fixture.database.pool())
        .await
        .unwrap();
    assert!(fixture.trash_ids().await.is_empty());
    let (status, _) = fixture.restore(id_of(&child)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn search_matches_titles_and_body_text() {
    let fixture = Fixture::new().await;
    let titled = fixture.create(json!({"title": "Roadmap 2027"})).await;
    let body = fixture.create(json!({"title": "Notes"})).await;
    let long_intro = "intro ".repeat(40);
    fixture
        .set_text(
            &body,
            &format!("{long_intro}The ROADMAP lives here 100% of the time"),
        )
        .await;
    let trashed = fixture.create(json!({"title": "Old roadmap"})).await;
    fixture.trash(id_of(&trashed)).await;

    let (status, results) = fixture.search("roadmap").await;
    assert_eq!(status, StatusCode::OK, "{results}");
    let items = results["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["id"], titled["id"]);
    assert_eq!(items[0]["snippet"], "");
    assert_eq!(items[0]["snippet_highlights"], json!([]));
    assert_eq!(
        items[0]["title_highlights"],
        json!([{"start": 0, "end": 7}])
    );
    assert_eq!(items[1]["id"], body["id"]);
    assert_eq!(items[1]["title"], "Notes");
    assert_eq!(items[1]["title_highlights"], json!([]));
    let snippet = items[1]["snippet"].as_str().unwrap();
    assert!(snippet.starts_with('…'), "{snippet}");
    assert!(snippet.contains("The ROADMAP lives here"), "{snippet}");
    assert_eq!(highlighted(&items[1]), ["ROADMAP"]);

    let (_, results) = fixture.search("100%25").await;
    assert_eq!(results["items"].as_array().unwrap().len(), 1);
    // Nothing searchable (no letter or digit) and blank queries find nothing.
    for query in ["%25", "%20", "*", "%22%22"] {
        let (status, results) = fixture.search(query).await;
        assert_eq!(status, StatusCode::OK, "{query}");
        assert!(results["items"].as_array().unwrap().is_empty(), "{query}");
    }
    let (status, problem) = fixture.search(&"x".repeat(201)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(problem["detail"], "q");
    let (status, _) = fixture
        .call("GET", &format!("{}/search", fixture.pages_uri()), None)
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    for index in 0..25 {
        fixture
            .create(json!({"title": format!("Bulk roadmap {index}")}))
            .await;
    }
    let (_, results) = fixture.search("roadmap").await;
    assert_eq!(results["items"].as_array().unwrap().len(), 20);
}

#[tokio::test]
async fn search_folds_diacritics_matches_prefixes_and_ranks_titles_first() {
    let fixture = Fixture::new().await;
    let uber = fixture.create(json!({"title": "Über uns"})).await;
    let cafe = fixture.create(json!({"title": "Menu"})).await;
    fixture
        .set_text(&cafe, "Das Café liegt über dem Fluss")
        .await;
    for query in ["über", "uber", "ÜBER", "Uber"] {
        let (status, results) = fixture.search(&encode(query)).await;
        assert_eq!(status, StatusCode::OK, "{query}");
        // The title match ranks above the body match.
        assert_eq!(
            result_ids(&results),
            [id_of(&uber), id_of(&cafe)],
            "{query}"
        );
    }
    let (_, results) = fixture.search(&encode("uber")).await;
    assert_eq!(
        results["items"][0]["title_highlights"],
        json!([{"start": 0, "end": 4}])
    );
    assert_eq!(highlighted(&results["items"][1]), ["über"]);
    let (_, results) = fixture.search("cafe").await;
    assert_eq!(result_ids(&results), [id_of(&cafe)]);
    assert_eq!(highlighted(&results["items"][0]), ["Café"]);

    // The last word matches as a prefix, earlier words must match whole; all words must match.
    let (_, results) = fixture.search("flu").await;
    assert_eq!(result_ids(&results), [id_of(&cafe)]);
    let (_, results) = fixture.search("caf%20fluss").await;
    assert!(result_ids(&results).is_empty());
    let (_, results) = fixture.search("cafe%20flu").await;
    assert_eq!(result_ids(&results), [id_of(&cafe)]);
    let (_, results) = fixture.search("cafe%20missing").await;
    assert!(result_ids(&results).is_empty());

    // FTS syntax in the input is plain text, never an error.
    for query in [
        "\"",
        "\"cafe",
        "cafe\"",
        "*",
        "cafe*",
        "caf*e",
        ":",
        "title:cafe",
        "body:x",
        "-cafe",
        "cafe -fluss",
        "NEAR",
        "NEAR(cafe fluss)",
        "cafe AND fluss",
        "cafe OR x",
        "NOT cafe",
        "(cafe",
        "cafe)",
        "()",
        "^cafe",
        "{title}: cafe",
        "cafe + fluss",
        "'",
        "\u{2}cafe\u{3}",
        "🙂",
        "a\"b\"c",
    ] {
        let (status, results) = fixture.search(&encode(query)).await;
        assert_eq!(status, StatusCode::OK, "{query}: {results}");
    }
    let (_, results) = fixture.search(&encode("NEAR(cafe fluss)")).await;
    assert!(result_ids(&results).is_empty(), "NEAR is a word here");
    let (_, results) = fixture.search(&encode("\"café\" (fluss)")).await;
    assert_eq!(result_ids(&results), [id_of(&cafe)]);

    // Title and body changes are searchable right away.
    let (status, _) = fixture
        .call(
            "PATCH",
            &fixture.page_uri(id_of(&uber)),
            Some(json!({"expected_version": 0, "title": "Impressum"})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (_, results) = fixture.search("uber").await;
    assert_eq!(result_ids(&results), [id_of(&cafe)]);
    let (_, results) = fixture.search("impressum").await;
    assert_eq!(result_ids(&results), [id_of(&uber)]);
    fixture.set_text(&cafe, "Neuer Text").await;
    let (_, results) = fixture.search("fluss").await;
    assert!(result_ids(&results).is_empty());
    let (_, results) = fixture.search("neuer").await;
    assert_eq!(result_ids(&results), [id_of(&cafe)]);
}

#[tokio::test]
async fn search_only_returns_live_pages_the_caller_can_see() {
    let fixture = Fixture::new().await;
    let (_, member_cookie) =
        add_member(&fixture, &fixture.workspace_id, "member@example.com").await;
    let shared = fixture.create(json!({"title": "Quokka shared"})).await;
    let private = fixture
        .create(json!({"private": true, "title": "Quokka private"}))
        .await;
    let private_child = fixture
        .create(json!({"parent_id": id_of(&private), "title": "Quokka private child"}))
        .await;
    let trashed = fixture.create(json!({"title": "Quokka trashed"})).await;
    fixture.trash(id_of(&trashed)).await;
    let foreign_workspace = create_workspace(&fixture, "Foreign").await;
    let (status, foreign) = fixture
        .call(
            "POST",
            &format!("/api/v1/workspaces/{foreign_workspace}/pages"),
            Some(json!({"title": "Quokka foreign"})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{foreign}");

    let (_, found) = fixture.search("quokka").await;
    let mut owner_ids = result_ids(&found);
    owner_ids.sort_unstable();
    let mut expected = vec![id_of(&shared), id_of(&private), id_of(&private_child)];
    expected.sort_unstable();
    assert_eq!(owner_ids, expected);
    let (status, member) = call_as(
        &fixture,
        &member_cookie,
        "GET",
        &format!("{}/search?q=quokka", fixture.pages_uri()),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(result_ids(&member), [id_of(&shared)]);
    let (status, _) = call_as(
        &fixture,
        &member_cookie,
        "GET",
        &format!("/api/v1/workspaces/{foreign_workspace}/pages/search?q=quokka"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Restored pages come back; purged ones leave the index.
    let (status, _) = fixture.restore(id_of(&trashed)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        fixture.search("trashed").await.1["items"][0]["id"],
        trashed["id"]
    );
    fixture.trash(id_of(&trashed)).await;
    fixture.purge(id_of(&trashed)).await;
    assert!(result_ids(&fixture.search("trashed").await.1).is_empty());
    let orphans: i64 = sqlx::query_scalar(
        "SELECT (SELECT COUNT(*) FROM page_search) + (SELECT COUNT(*) FROM page_search_rows) \
         - 2 * (SELECT COUNT(*) FROM pages)",
    )
    .fetch_one(fixture.database.pool())
    .await
    .unwrap();
    assert_eq!(orphans, 0);
}

#[tokio::test]
async fn delete_forever_purges_a_trashed_batch_by_permission() {
    let fixture = Fixture::new().await;
    let (_, member_cookie) =
        add_member(&fixture, &fixture.workspace_id, "member@example.com").await;
    let (admin_id, admin_cookie) =
        add_member(&fixture, &fixture.workspace_id, "admin@example.com").await;
    sqlx::query("UPDATE memberships SET role = 'admin' WHERE user_id = ?")
        .bind(admin_id.to_string())
        .execute(fixture.database.pool())
        .await
        .unwrap();
    let parent = fixture.create(json!({"title": "Parent"})).await;
    let child = fixture
        .create(json!({"parent_id": id_of(&parent), "title": "Child"}))
        .await;
    let early = fixture
        .create(json!({"parent_id": id_of(&parent), "title": "Early"}))
        .await;
    let live = fixture.create(json!({"title": "Live"})).await;
    fixture.favorite(id_of(&child)).await;
    fixture.trash(id_of(&early)).await;
    fixture.trash(id_of(&parent)).await;
    let purge_uri = |page: &Value, version: u64| {
        format!(
            "{}/permanent?expected_version={version}",
            fixture.page_uri(id_of(page))
        )
    };

    // Live pages and pages trashed along with an ancestor are not trash entries.
    let (status, problem) = fixture.call("DELETE", &purge_uri(&live, 0), None).await;
    assert_eq!(status, StatusCode::CONFLICT, "{problem}");
    assert_eq!(problem["code"], "page_not_trashed");
    let (status, problem) = fixture.call("DELETE", &purge_uri(&child, 1), None).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(problem["code"], "page_not_trashed");
    let (status, problem) = fixture
        .call(
            "DELETE",
            &purge_uri(&json!({"id": Id::new_v7().to_string()}), 0),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(problem["code"], "page_not_found");
    // Members may not purge teamspace pages; stale versions conflict.
    let (status, problem) = call_as(
        &fixture,
        &member_cookie,
        "DELETE",
        &purge_uri(&parent, 1),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{problem}");
    assert_eq!(problem["code"], "workspace_action_forbidden");
    let (status, problem) = fixture.call("DELETE", &purge_uri(&parent, 0), None).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(problem["code"], "conflict");

    let (status, body) = call_as(
        &fixture,
        &admin_cookie,
        "DELETE",
        &purge_uri(&parent, 1),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    // The batch (parent + child) is gone; Early, trashed on its own before, stays in the trash.
    assert_eq!(fixture.trash_ids().await, [id_of(&early)]);
    let remaining: Vec<String> = sqlx::query_scalar("SELECT title FROM pages ORDER BY title")
        .fetch_all(fixture.database.pool())
        .await
        .unwrap();
    assert_eq!(remaining, ["Early", "Live"]);
    assert_eq!(
        fixture.get_raw(id_of(&early)).await,
        (None::<String>, Some(id_of(&early).to_owned()))
    );
    let favorites: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM page_favorites")
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    assert_eq!(favorites, 0);
    let (status, restored) = fixture.restore(id_of(&early)).await;
    assert_eq!(status, StatusCode::OK, "{restored}");
    assert_eq!(restored["parent_id"], Value::Null);

    // Private pages: only their owner can see (404 for others) and purge them.
    let (status, diary) = call_as(
        &fixture,
        &member_cookie,
        "POST",
        &fixture.pages_uri(),
        Some(json!({"private": true, "title": "Member diary"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) = call_as(
        &fixture,
        &member_cookie,
        "DELETE",
        &format!("{}?expected_version=0", fixture.page_uri(id_of(&diary))),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, problem) = fixture.call("DELETE", &purge_uri(&diary, 1), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(problem["code"], "page_not_found");
    let (status, _) = call_as(
        &fixture,
        &member_cookie,
        "DELETE",
        &purge_uri(&diary, 1),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let audits: Vec<(String, String)> = sqlx::query_as(
        "SELECT resource_id, metadata_json FROM audit_events WHERE action = 'page.purged' ORDER BY occurred_at, id",
    )
    .fetch_all(fixture.database.pool())
    .await
    .unwrap();
    assert_eq!(
        audits,
        [
            (id_of(&parent).to_owned(), r#"{"pages":2}"#.to_owned()),
            (id_of(&diary).to_owned(), r#"{"pages":1}"#.to_owned()),
        ]
    );
}

#[tokio::test]
async fn empty_trash_purges_only_what_the_caller_may_purge() {
    let fixture = Fixture::new().await;
    let (_, member_cookie) =
        add_member(&fixture, &fixture.workspace_id, "member@example.com").await;
    let shared = fixture.create(json!({"title": "Shared"})).await;
    let shared_child = fixture
        .create(json!({"parent_id": id_of(&shared), "title": "Shared child"}))
        .await;
    let own = fixture
        .create(json!({"private": true, "title": "Owner private"}))
        .await;
    let live = fixture.create(json!({"title": "Live"})).await;
    fixture.trash(id_of(&shared)).await;
    fixture.trash(id_of(&own)).await;
    let (_, mine) = call_as(
        &fixture,
        &member_cookie,
        "POST",
        &fixture.pages_uri(),
        Some(json!({"private": true, "title": "Member private"})),
    )
    .await;
    call_as(
        &fixture,
        &member_cookie,
        "DELETE",
        &format!("{}?expected_version=0", fixture.page_uri(id_of(&mine))),
        None,
    )
    .await;
    let empty_uri = format!("{}/trash/empty", fixture.pages_uri());

    // A member empties only their own private trash; the teamspace trash stays.
    let (status, emptied) = call_as(&fixture, &member_cookie, "POST", &empty_uri, None).await;
    assert_eq!(status, StatusCode::OK, "{emptied}");
    assert_eq!(emptied, json!({"purged": 1}));
    let mut expected = vec![id_of(&shared).to_owned(), id_of(&own).to_owned()];
    expected.sort();
    assert_eq!(fixture.trash_ids().await, expected);

    // The owner (an admin) empties teamspace pages and their own private pages.
    let (status, emptied) = fixture.call("POST", &empty_uri, None).await;
    assert_eq!(status, StatusCode::OK, "{emptied}");
    assert_eq!(emptied, json!({"purged": 2}));
    assert!(fixture.trash_ids().await.is_empty());
    let titles: Vec<String> = sqlx::query_scalar("SELECT title FROM pages ORDER BY title")
        .fetch_all(fixture.database.pool())
        .await
        .unwrap();
    assert_eq!(titles, ["Live"]);
    assert_eq!(fixture.get(id_of(&live)).await["title"], "Live");
    let (status, _) = fixture
        .call("GET", &fixture.page_uri(id_of(&shared_child)), None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (_, emptied) = fixture.call("POST", &empty_uri, None).await;
    assert_eq!(emptied, json!({"purged": 0}));
    let purged: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM audit_events WHERE action = 'page.purged'")
            .fetch_one(fixture.database.pool())
            .await
            .unwrap();
    assert_eq!(purged, 3);
}

#[tokio::test]
async fn duplicate_copies_a_page_or_subtree_after_the_original() {
    let fixture = Fixture::new().await;
    let (_, member_cookie) =
        add_member(&fixture, &fixture.workspace_id, "member@example.com").await;
    let first = fixture.create(json!({"title": "First"})).await;
    let source = fixture
        .create(json!({"title": "Source", "icon": "🚀"}))
        .await;
    let last = fixture.create(json!({"title": "Last"})).await;
    let child_a = fixture
        .create(json!({"parent_id": id_of(&source), "title": "A"}))
        .await;
    let child_b = fixture
        .create(json!({"parent_id": id_of(&source), "title": "B"}))
        .await;
    let grandchild = fixture
        .create(json!({"parent_id": id_of(&child_a), "title": "A1"}))
        .await;
    let gone = fixture
        .create(json!({"parent_id": id_of(&source), "title": "Gone"}))
        .await;
    fixture.trash(id_of(&gone)).await;
    let content = json!([
        {"id": "p", "type": "paragraph", "content": [
            {"type": "text", "text": "See ", "styles": {}},
            {"type": "link", "href": format!("/docs/{}", id_of(&child_b)), "content": [{"type": "text", "text": "B", "styles": {}}]},
            {"type": "link", "href": format!("/docs/{}", id_of(&first)), "content": [{"type": "text", "text": "outside", "styles": {}}]},
            {"type": "text", "text": format!("/docs/{}", id_of(&child_b)), "styles": {}}
        ], "children": []},
        {"id": "c", "type": "page", "props": {"pageId": id_of(&child_a)}, "children": []},
        {"id": "o", "type": "page", "props": {"pageId": id_of(&last)}, "children": []}
    ]);
    let (status, updated) = fixture
        .call(
            "PATCH",
            &fixture.page_uri(id_of(&source)),
            Some(json!({"expected_version": 0, "content": content, "cover_url": "https://example.com/c.png", "cover_position": "50,20"})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    let (status, _) = fixture
        .call(
            "PATCH",
            &fixture.page_uri(id_of(&grandchild)),
            Some(json!({"expected_version": 0, "content": [
                {"id": "g", "type": "paragraph", "content": [{"type": "link", "href": format!("/docs/{}", id_of(&source)), "content": [{"type": "text", "text": "up", "styles": {}}]}], "children": []}
            ]})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let duplicate_uri = |page: &Value| format!("{}/duplicate", fixture.page_uri(id_of(page)));

    // Without sub-pages: just the page, links unchanged (the children were not copied).
    let (status, copy) = fixture
        .call(
            "POST",
            &duplicate_uri(&source),
            Some(json!({"include_children": false})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{copy}");
    assert_eq!(copy["title"], "Source (copy)");
    assert_eq!(copy["icon"], "🚀");
    assert_eq!(copy["cover_url"], "https://example.com/c.png");
    assert_eq!(copy["cover_position"], "50,20");
    assert_eq!(copy["version"], 0);
    assert_eq!(copy["position"], 2);
    assert_eq!(copy["content"], updated["content"]);
    assert_eq!(
        fixture.get(id_of(&copy)).await["content"],
        updated["content"]
    );
    let roots = |tree: &[Value]| {
        tree.iter()
            .filter(|page| page["parent_id"].is_null())
            .map(|page| page["title"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        roots(&fixture.tree().await),
        ["First", "Source", "Source (copy)", "Last"]
    );

    // With sub-pages: the live subtree in order, and links inside it point at the copies.
    let (status, deep) = fixture
        .call(
            "POST",
            &duplicate_uri(&source),
            Some(json!({"include_children": true})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{deep}");
    assert_eq!(deep["position"], 2);
    assert_eq!(
        roots(&fixture.tree().await),
        ["First", "Source", "Source (copy)", "Source (copy)", "Last"]
    );
    let tree = fixture.tree().await;
    let children_of = |parent: &str| {
        tree.iter()
            .filter(|page| page["parent_id"] == parent)
            .map(|page| {
                (
                    page["title"].as_str().unwrap(),
                    id_of(page),
                    page["position"].as_i64().unwrap(),
                )
            })
            .collect::<Vec<_>>()
    };
    let copied = children_of(id_of(&deep));
    assert_eq!(
        copied
            .iter()
            .map(|(title, _, position)| (*title, *position))
            .collect::<Vec<_>>(),
        [("A", 0), ("B", 1)]
    );
    let (a_copy, b_copy) = (copied[0].1, copied[1].1);
    let a1 = children_of(a_copy);
    assert_eq!(a1.len(), 1);
    assert_eq!(a1[0].0, "A1");
    let deep = fixture.get(id_of(&deep)).await;
    let blocks = deep["content"].as_array().unwrap();
    let inline = blocks[0]["content"].as_array().unwrap();
    assert_eq!(inline[1]["href"], format!("/docs/{b_copy}"));
    assert_eq!(inline[2]["href"], format!("/docs/{}", id_of(&first)));
    assert_eq!(inline[3]["text"], format!("/docs/{}", id_of(&child_b)));
    assert_eq!(blocks[1]["props"]["pageId"], a_copy);
    assert_eq!(blocks[2]["props"]["pageId"], id_of(&last));
    let a1_copy = fixture.get(a1[0].1).await;
    assert_eq!(
        a1_copy["content"][0]["content"][0]["href"],
        format!("/docs/{}", id_of(&deep))
    );
    // The originals are untouched.
    assert_eq!(
        fixture.get(id_of(&source)).await["content"],
        updated["content"]
    );
    assert_eq!(fixture.get(id_of(&child_a)).await["position"], 0);

    // Empty titles, other members, invisible and trashed pages.
    let untitled = fixture.create(json!({})).await;
    let (_, copy) = fixture
        .call("POST", &duplicate_uri(&untitled), Some(json!({})))
        .await;
    assert_eq!(copy["title"], "Untitled (copy)");
    let long = fixture.create(json!({"title": "é".repeat(500)})).await;
    let (_, copy) = fixture
        .call("POST", &duplicate_uri(&long), Some(json!({})))
        .await;
    assert_eq!(copy["title"].as_str().unwrap().chars().count(), 500);
    assert!(copy["title"].as_str().unwrap().ends_with(" (copy)"));
    let (status, copy) = call_as(
        &fixture,
        &member_cookie,
        "POST",
        &duplicate_uri(&first),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{copy}");
    assert_eq!(copy["creator_id"], copy["updated_by"]);
    assert_ne!(copy["creator_id"], fixture.owner_id.to_string());
    let diary = fixture
        .create(json!({"private": true, "title": "Diary"}))
        .await;
    let (status, problem) = call_as(
        &fixture,
        &member_cookie,
        "POST",
        &duplicate_uri(&diary),
        Some(json!({"include_children": true})),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(problem["code"], "page_not_found");
    let (status, private_copy) = fixture
        .call("POST", &duplicate_uri(&diary), Some(json!({})))
        .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(private_copy["private"], true);
    let (status, _) = fixture
        .call("POST", &duplicate_uri(&gone), Some(json!({})))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = fixture
        .call("POST", &duplicate_uri(&first), Some(json!({"deep": true})))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let duplicated: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events WHERE action = 'page.duplicated' AND resource_id = ?",
    )
    .bind(a_copy)
    .fetch_one(fixture.database.pool())
    .await
    .unwrap();
    assert_eq!(duplicated, 1);
    let (_, found) = fixture.search("A1").await;
    assert_eq!(found["items"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn pages_are_isolated_between_workspaces() {
    let fixture = Fixture::new().await;
    let page = fixture.create(json!({"title": "Private to Orbit"})).await;
    let foreign_workspace = create_workspace(&fixture, "Foreign").await;
    let (_, outsider_cookie) =
        add_member(&fixture, &foreign_workspace, "outsider@example.com").await;

    let (status, list) = call_as(
        &fixture,
        &outsider_cookie,
        "GET",
        &fixture.pages_uri(),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{list}");
    assert_eq!(list["code"], "page_not_found");
    for (method, uri, body) in [
        ("GET", fixture.page_uri(id_of(&page)), None),
        (
            "PATCH",
            fixture.page_uri(id_of(&page)),
            Some(json!({"expected_version": 0, "title": "Hijacked"})),
        ),
        (
            "DELETE",
            format!("{}?expected_version=0", fixture.page_uri(id_of(&page))),
            None,
        ),
        (
            "POST",
            fixture.pages_uri(),
            Some(json!({"title": "Intruder"})),
        ),
        (
            "GET",
            format!("{}/search?q=private", fixture.pages_uri()),
            None,
        ),
    ] {
        let (status, problem) = call_as(&fixture, &outsider_cookie, method, &uri, body).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{method} {uri}: {problem}");
    }

    // The owner belongs to both workspaces, but a page is only reachable through its own.
    let foreign_pages = format!("/api/v1/workspaces/{foreign_workspace}/pages");
    let (status, _) = fixture
        .call("GET", &format!("{foreign_pages}/{}", id_of(&page)), None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = fixture
        .call(
            "POST",
            &foreign_pages,
            Some(json!({"parent_id": id_of(&page)})),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, list) = fixture.call("GET", &foreign_pages, None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(list["items"].as_array().unwrap().is_empty());
    assert_eq!(fixture.get(id_of(&page)).await["title"], "Private to Orbit");

    // Workspace members other than the creator can edit every page.
    let (_, member_cookie) =
        add_member(&fixture, &fixture.workspace_id, "member@example.com").await;
    let (status, edited) = call_as(
        &fixture,
        &member_cookie,
        "PATCH",
        &fixture.page_uri(id_of(&page)),
        Some(json!({"expected_version": 0, "title": "Shared"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{edited}");
    assert_ne!(edited["updated_by"], edited["creator_id"]);
}

#[tokio::test]
async fn page_mutations_are_audited_and_emit_realtime_events() {
    let fixture = Fixture::new().await;
    let page = fixture.create(json!({"title": "Audited"})).await;
    let (status, _) = fixture
        .call(
            "PATCH",
            &fixture.page_uri(id_of(&page)),
            Some(json!({"expected_version": 0, "title": "Renamed"})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let page = fixture.get(id_of(&page)).await;
    let (status, _) = fixture.move_to(&page, None, 0).await;
    assert_eq!(status, StatusCode::OK);
    fixture.trash(id_of(&page)).await;
    let (status, _) = fixture.restore(id_of(&page)).await;
    assert_eq!(status, StatusCode::OK);

    let rows = sqlx::query_as::<_, (String, String, i64)>(
        "SELECT audit_events.action, audit_events.resource_type, \
         (SELECT COUNT(*) FROM outbox_events WHERE outbox_events.id = audit_events.id AND outbox_events.topic = 'workspace.changed') \
         FROM audit_events WHERE workspace_id = ? AND resource_id = ? ORDER BY occurred_at, id",
    )
    .bind(&fixture.workspace_id)
    .bind(id_of(&page))
    .fetch_all(fixture.database.pool())
    .await
    .unwrap();
    assert_eq!(
        rows,
        [
            ("page.created".to_owned(), "page".to_owned(), 1),
            ("page.updated".to_owned(), "page".to_owned(), 1),
            ("page.moved".to_owned(), "page".to_owned(), 1),
            ("page.deleted".to_owned(), "page".to_owned(), 1),
            ("page.restored".to_owned(), "page".to_owned(), 1),
        ]
    );
}

#[tokio::test]
async fn retention_purges_expired_pages_and_deleted_workspaces() {
    let fixture = Fixture::new().await;
    let parent = fixture.create(json!({"title": "Expired"})).await;
    fixture
        .create(json!({"parent_id": id_of(&parent), "title": "Expired child"}))
        .await;
    let recent = fixture.create(json!({"title": "Recently trashed"})).await;
    fixture.create(json!({"title": "Live"})).await;
    let private = fixture
        .create(json!({"private": true, "title": "Private live"}))
        .await;
    fixture
        .create(json!({"parent_id": id_of(&private), "title": "Private child"}))
        .await;
    fixture.trash(id_of(&parent)).await;
    fixture.trash(id_of(&recent)).await;
    sqlx::query("UPDATE pages SET deleted_at = ? WHERE trashed_with = ?")
        .bind(TimestampMillis::now().as_millis() - RETENTION_MILLIS - 1_000)
        .bind(id_of(&parent))
        .execute(fixture.database.pool())
        .await
        .unwrap();
    let workspaces = WorkspaceRepository::new((*fixture.database).clone());
    workspaces
        .purge_retention(TimestampMillis::now())
        .await
        .unwrap();
    let titles: Vec<String> =
        sqlx::query_scalar("SELECT title FROM pages WHERE workspace_id = ? ORDER BY title")
            .bind(&fixture.workspace_id)
            .fetch_all(fixture.database.pool())
            .await
            .unwrap();
    assert_eq!(
        titles,
        ["Live", "Private child", "Private live", "Recently trashed"]
    );

    sqlx::query("UPDATE workspaces SET deleted_at = ? WHERE id = ?")
        .bind(TimestampMillis::now().as_millis() - RETENTION_MILLIS - 1_000)
        .bind(&fixture.workspace_id)
        .execute(fixture.database.pool())
        .await
        .unwrap();
    workspaces
        .purge_retention(TimestampMillis::now())
        .await
        .unwrap();
    let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pages")
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    assert_eq!(remaining, 0);
    let teamspaces: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM teamspaces WHERE workspace_id = ?")
            .bind(&fixture.workspace_id)
            .fetch_one(fixture.database.pool())
            .await
            .unwrap();
    assert_eq!(teamspaces, 0);
}

#[tokio::test]
async fn private_pages_are_invisible_to_other_members() {
    let fixture = Fixture::new().await;
    let (_, member_cookie) =
        add_member(&fixture, &fixture.workspace_id, "member@example.com").await;
    fixture.create(json!({"title": "Shared diary"})).await;
    let diary = fixture
        .create(json!({"private": true, "title": "Diary"}))
        .await;
    assert_eq!(diary["private"], true);
    assert_eq!(diary["teamspace_id"], Value::Null);
    let entry = fixture
        .create(json!({"parent_id": id_of(&diary), "title": "Diary entry"}))
        .await;
    assert_eq!(entry["private"], true);
    let doomed = fixture
        .create(json!({"private": true, "title": "Doomed diary"}))
        .await;
    fixture.trash(id_of(&doomed)).await;

    // The owner sees everything, private pages after the teamspace pages.
    assert_eq!(
        titles(&fixture.tree().await),
        ["Shared diary", "Diary", "Diary entry"]
    );
    let (_, found) = fixture.search("diary").await;
    assert_eq!(found["items"].as_array().unwrap().len(), 3);
    assert_eq!(fixture.trash_ids().await, [id_of(&doomed)]);

    let as_member = |method: &'static str, uri: String, body: Option<Value>| {
        let cookie = member_cookie.clone();
        let fixture = &fixture;
        async move { call_as(fixture, &cookie, method, &uri, body).await }
    };
    let (status, tree) = as_member("GET", fixture.pages_uri(), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(titles(tree["items"].as_array().unwrap()), ["Shared diary"]);
    let (_, found) = as_member(
        "GET",
        format!("{}/search?q=diary", fixture.pages_uri()),
        None,
    )
    .await;
    assert_eq!(titles(found["items"].as_array().unwrap()), ["Shared diary"]);
    let (_, trash) = as_member("GET", format!("{}/trash", fixture.pages_uri()), None).await;
    assert!(trash["items"].as_array().unwrap().is_empty());
    let member_page = {
        let (status, page) = as_member(
            "POST",
            fixture.pages_uri(),
            Some(json!({"title": "Member page"})),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{page}");
        page
    };
    for (method, uri, body) in [
        ("GET", fixture.page_uri(id_of(&diary)), None),
        ("GET", fixture.page_uri(id_of(&entry)), None),
        (
            "PATCH",
            fixture.page_uri(id_of(&diary)),
            Some(json!({"expected_version": 0, "title": "Read"})),
        ),
        (
            "DELETE",
            format!("{}?expected_version=0", fixture.page_uri(id_of(&diary))),
            None,
        ),
        (
            "POST",
            format!("{}/move", fixture.page_uri(id_of(&entry))),
            Some(json!({"expected_version": 0, "parent_id": null, "position": 0})),
        ),
        (
            "POST",
            format!("{}/restore", fixture.page_uri(id_of(&doomed))),
            Some(json!({"expected_version": 1})),
        ),
        (
            "POST",
            fixture.pages_uri(),
            Some(json!({"parent_id": id_of(&diary), "title": "Sneaky"})),
        ),
        (
            "POST",
            format!("{}/move", fixture.page_uri(id_of(&member_page))),
            Some(json!({"expected_version": 0, "parent_id": id_of(&diary), "position": 0})),
        ),
    ] {
        let (status, problem) = as_member(method, uri.clone(), body).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{method} {uri}: {problem}");
        assert_eq!(problem["code"], "page_not_found");
    }
    assert_eq!(fixture.get(id_of(&diary)).await["title"], "Diary");
}

#[tokio::test]
async fn new_pages_go_to_the_requested_space_and_sub_pages_inherit_it() {
    let fixture = Fixture::new().await;
    let general = fixture.teamspace_ids().await.remove(0);
    let (status, design) = fixture
        .call(
            "POST",
            &format!("/api/v1/workspaces/{}/teamspaces", fixture.workspace_id),
            Some(json!({"name": "Design"})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let design = id_of(&design).to_owned();

    let default = fixture.create(json!({"title": "Default"})).await;
    assert_eq!(default["teamspace_id"], general.as_str());
    assert_eq!(default["private"], false);
    let explicit = fixture
        .create(json!({"teamspace_id": design, "title": "Explicit"}))
        .await;
    assert_eq!(explicit["teamspace_id"], design.as_str());
    assert_eq!(explicit["position"], 0, "positions are per space");
    let not_private = fixture
        .create(json!({"private": false, "title": "Not private"}))
        .await;
    assert_eq!(not_private["teamspace_id"], general.as_str());
    assert_eq!(not_private["position"], 1);
    let private = fixture
        .create(json!({"private": true, "title": "Private"}))
        .await;
    assert_eq!(private["position"], 0);
    assert_eq!(private["private"], true);

    let design_child = fixture
        .create(json!({"parent_id": id_of(&explicit), "title": "Design child"}))
        .await;
    assert_eq!(design_child["teamspace_id"], design.as_str());
    let agreeing = fixture
        .create(json!({"parent_id": id_of(&explicit), "teamspace_id": design, "private": false}))
        .await;
    assert_eq!(agreeing["teamspace_id"], design.as_str());
    let private_child = fixture
        .create(json!({"parent_id": id_of(&private), "title": "Private child", "private": true}))
        .await;
    assert_eq!(private_child["private"], true);
    assert_eq!(private_child["teamspace_id"], Value::Null);
    let stored: (Option<String>, Option<String>) =
        sqlx::query_as("SELECT teamspace_id, owner_id FROM pages WHERE id = ?")
            .bind(id_of(&private_child))
            .fetch_one(fixture.database.pool())
            .await
            .unwrap();
    assert_eq!(stored, (None, Some(fixture.owner_id.to_string())));

    for (body, status, code) in [
        (
            json!({"parent_id": id_of(&explicit), "private": true}),
            StatusCode::UNPROCESSABLE_ENTITY,
            "validation_failed",
        ),
        (
            json!({"parent_id": id_of(&explicit), "teamspace_id": general}),
            StatusCode::UNPROCESSABLE_ENTITY,
            "validation_failed",
        ),
        (
            json!({"parent_id": id_of(&private), "private": false}),
            StatusCode::UNPROCESSABLE_ENTITY,
            "validation_failed",
        ),
        (
            json!({"private": true, "teamspace_id": general}),
            StatusCode::UNPROCESSABLE_ENTITY,
            "validation_failed",
        ),
        (
            json!({"teamspace_id": Id::new_v7().to_string()}),
            StatusCode::NOT_FOUND,
            "teamspace_not_found",
        ),
        (
            json!({"teamspace_id": "not-an-id"}),
            StatusCode::NOT_FOUND,
            "teamspace_not_found",
        ),
    ] {
        let (actual, problem) = fixture
            .call("POST", &fixture.pages_uri(), Some(body.clone()))
            .await;
        assert_eq!(actual, status, "{body}: {problem}");
        assert_eq!(problem["code"], code, "{body}");
    }
}

#[tokio::test]
async fn moving_a_root_between_spaces_takes_its_whole_subtree() {
    let fixture = Fixture::new().await;
    let general = fixture.teamspace_ids().await.remove(0);
    let (_, member_cookie) =
        add_member(&fixture, &fixture.workspace_id, "member@example.com").await;
    let shared = fixture.create(json!({"title": "Shared"})).await;
    let root = fixture
        .create(json!({"private": true, "title": "Root"}))
        .await;
    let child = fixture
        .create(json!({"parent_id": id_of(&root), "title": "Child"}))
        .await;
    let grandchild = fixture
        .create(json!({"parent_id": id_of(&child), "title": "Grandchild"}))
        .await;
    let trashed = fixture
        .create(json!({"parent_id": id_of(&root), "title": "Trashed child"}))
        .await;
    fixture.trash(id_of(&trashed)).await;
    let member_titles = || async {
        let (status, tree) =
            call_as(&fixture, &member_cookie, "GET", &fixture.pages_uri(), None).await;
        assert_eq!(status, StatusCode::OK);
        titles(tree["items"].as_array().unwrap())
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    assert_eq!(member_titles().await, ["Shared"]);

    // Into a teamspace at the root.
    let (status, moved) = fixture
        .call(
            "POST",
            &format!("{}/move", fixture.page_uri(id_of(&root))),
            Some(json!({"expected_version": 0, "parent_id": null, "position": 0, "teamspace_id": general})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{moved}");
    assert_eq!(moved["teamspace_id"], general.as_str());
    assert_eq!(moved["private"], false);
    assert_eq!(moved["position"], 0);
    assert_eq!(moved["version"], 1);
    assert_eq!(
        fixture.spaces().await,
        [
            ("Child".to_owned(), Some(general.clone()), None),
            ("Grandchild".to_owned(), Some(general.clone()), None),
            ("Root".to_owned(), Some(general.clone()), None),
            ("Shared".to_owned(), Some(general.clone()), None),
            ("Trashed child".to_owned(), Some(general.clone()), None),
        ]
    );
    assert_eq!(fixture.get(id_of(&shared)).await["position"], 1);
    assert_eq!(fixture.get(id_of(&child)).await["version"], 0);
    assert_eq!(
        member_titles().await,
        ["Root", "Shared", "Child", "Grandchild"]
    );

    // Back into the private space; `null` parent without a space keeps the current one.
    let (status, reordered) = fixture
        .call(
            "POST",
            &format!("{}/move", fixture.page_uri(id_of(&root))),
            Some(json!({"expected_version": 1, "parent_id": null, "position": 5})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{reordered}");
    assert_eq!(reordered["teamspace_id"], general.as_str());
    assert_eq!(reordered["position"], 1);
    let (status, private) = fixture
        .call(
            "POST",
            &format!("{}/move", fixture.page_uri(id_of(&root))),
            Some(json!({"expected_version": 2, "parent_id": null, "position": 0, "private": true})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{private}");
    assert_eq!(private["private"], true);
    let owner = Some(fixture.owner_id.to_string());
    assert_eq!(
        fixture.spaces().await,
        [
            ("Child".to_owned(), None, owner.clone()),
            ("Grandchild".to_owned(), None, owner.clone()),
            ("Root".to_owned(), None, owner.clone()),
            ("Shared".to_owned(), Some(general.clone()), None),
            ("Trashed child".to_owned(), None, owner.clone()),
        ]
    );
    assert_eq!(fixture.get(id_of(&shared)).await["position"], 0);
    assert_eq!(member_titles().await, ["Shared"]);

    // Under a parent the page joins the parent's space.
    let shared = fixture.get(id_of(&shared)).await;
    let (status, nested) = fixture.move_to(&shared, Some(id_of(&grandchild)), 0).await;
    assert_eq!(status, StatusCode::OK, "{nested}");
    assert_eq!(nested["private"], true);
    assert_eq!(member_titles().await, Vec::<String>::new());
    let (status, problem) = fixture
        .call(
            "POST",
            &format!("{}/move", fixture.page_uri(id_of(&shared))),
            Some(json!({"expected_version": 1, "parent_id": id_of(&root), "position": 0, "teamspace_id": general})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{problem}");
    // `private: false` at the root leaves the private space for the default teamspace.
    let (status, out) = fixture
        .call(
            "POST",
            &format!("{}/move", fixture.page_uri(id_of(&shared))),
            Some(
                json!({"expected_version": 1, "parent_id": null, "position": 0, "private": false}),
            ),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{out}");
    assert_eq!(out["teamspace_id"], general.as_str());
    assert_eq!(member_titles().await, ["Shared"]);
}

#[tokio::test]
async fn restore_goes_to_the_root_of_the_page_own_space() {
    let fixture = Fixture::new().await;
    fixture.create(json!({"title": "Team root"})).await;
    let first = fixture
        .create(json!({"private": true, "title": "First private"}))
        .await;
    let parent = fixture
        .create(json!({"private": true, "title": "Parent"}))
        .await;
    let child = fixture
        .create(json!({"parent_id": id_of(&parent), "title": "Child"}))
        .await;
    fixture.trash(id_of(&child)).await;
    fixture.trash(id_of(&parent)).await;
    let (status, restored) = fixture.restore(id_of(&child)).await;
    assert_eq!(status, StatusCode::OK, "{restored}");
    assert_eq!(restored["parent_id"], Value::Null);
    assert_eq!(restored["private"], true);
    assert_eq!(restored["teamspace_id"], Value::Null);
    // It keeps its old index (0) among the private roots; only that space is renumbered.
    assert_eq!(restored["position"], 0);
    let tree = fixture.tree().await;
    assert_eq!(titles(&tree), ["Team root", "Child", "First private"]);
    assert_eq!(tree[0]["position"], 0);
    assert_eq!(tree[2]["id"], first["id"]);
    assert_eq!(tree[2]["position"], 1);
}

#[tokio::test]
async fn favorites_are_idempotent_ordered_and_reorderable() {
    let fixture = Fixture::new().await;
    assert!(fixture.favorite_ids().await.is_empty());
    let [a, b, c] = [
        fixture.create(json!({"title": "A"})).await,
        fixture.create(json!({"title": "B"})).await,
        fixture.create(json!({"private": true, "title": "C"})).await,
    ];
    let [a, b, c] = [id_of(&a), id_of(&b), id_of(&c)];

    assert_eq!(
        fixture.favorite(b).await,
        json!({"page_id": b, "position": 0})
    );
    assert_eq!(
        fixture.favorite(a).await,
        json!({"page_id": a, "position": 1})
    );
    // Adding again keeps the favorite where it is.
    assert_eq!(
        fixture.favorite(b).await,
        json!({"page_id": b, "position": 0})
    );
    fixture.favorite(c).await;
    assert_eq!(
        fixture.favorites_as(&fixture.owner_cookie).await,
        [(b.to_owned(), 0), (a.to_owned(), 1), (c.to_owned(), 2)]
    );

    let (status, list) = fixture.move_favorite(c, 0).await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert_eq!(
        list,
        json!({"items": [
            {"page_id": c, "position": 0},
            {"page_id": b, "position": 1},
            {"page_id": a, "position": 2}
        ]})
    );
    let (status, _) = fixture.move_favorite(c, 99).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(fixture.favorite_ids().await, [b, a, c]);
    let (status, problem) = fixture.move_favorite(c, -1).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{problem}");
    assert_eq!(problem["code"], "validation_failed");
    let (status, problem) = fixture
        .call(
            "POST",
            &format!("{}/{a}/move", fixture.favorites_uri()),
            Some(json!({"position": 0, "extra": true})),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{problem}");

    // Removing is idempotent and closes the gap.
    for _ in 0..2 {
        let (status, body) = fixture.call("DELETE", &fixture.favorite_uri(b), None).await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    }
    assert_eq!(
        fixture.favorites_as(&fixture.owner_cookie).await,
        [(a.to_owned(), 0), (c.to_owned(), 1)]
    );
    let positions: Vec<i64> = sqlx::query_scalar(
        "SELECT position FROM page_favorites WHERE user_id = ? ORDER BY position",
    )
    .bind(fixture.owner_id.to_string())
    .fetch_all(fixture.database.pool())
    .await
    .unwrap();
    assert_eq!(positions, [0, 1]);
    // Moving a page that is not a favorite is a 404.
    let (status, problem) = fixture.move_favorite(b, 0).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{problem}");
    assert_eq!(problem["code"], "page_not_found");

    // Favorites are personal preferences: no audit rows, no realtime events.
    let audited: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM audit_events WHERE action LIKE '%favorite%'")
            .fetch_one(fixture.database.pool())
            .await
            .unwrap();
    assert_eq!(audited, 0);
}

#[tokio::test]
async fn trashed_favorites_hide_and_return_on_restore() {
    let fixture = Fixture::new().await;
    let [a, b, c] = [
        fixture.create(json!({"title": "A"})).await,
        fixture.create(json!({"title": "B"})).await,
        fixture.create(json!({"title": "C"})).await,
    ];
    let child = fixture
        .create(json!({"parent_id": id_of(&b), "title": "B child"}))
        .await;
    let [a, b, c, child] = [id_of(&a), id_of(&b), id_of(&c), id_of(&child)];
    for id in [a, child, b, c] {
        fixture.favorite(id).await;
    }
    fixture.trash(b).await;
    assert_eq!(
        fixture.favorites_as(&fixture.owner_cookie).await,
        [(a.to_owned(), 0), (c.to_owned(), 1)]
    );
    for (method, uri, body) in [
        ("PUT", fixture.favorite_uri(b), None),
        ("DELETE", fixture.favorite_uri(b), None),
        (
            "POST",
            format!("{}/{b}/move", fixture.favorites_uri()),
            Some(json!({"position": 0})),
        ),
    ] {
        let (status, problem) = fixture.call(method, &uri, body).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{method} {uri}: {problem}");
        assert_eq!(problem["code"], "page_not_found");
    }
    // Reordering visible favorites keeps the hidden ones in their slot.
    let (status, _) = fixture.move_favorite(c, 0).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(fixture.favorite_ids().await, [c, a]);

    let (status, restored) = fixture.restore(b).await;
    assert_eq!(status, StatusCode::OK, "{restored}");
    assert_eq!(fixture.favorite_ids().await, [c, a, child, b]);
}

#[tokio::test]
async fn favorites_are_personal_and_follow_page_visibility() {
    let fixture = Fixture::new().await;
    let (_, member_cookie) =
        add_member(&fixture, &fixture.workspace_id, "member@example.com").await;
    let shared = fixture.create(json!({"title": "Shared"})).await;
    let diary = fixture
        .create(json!({"private": true, "title": "Diary"}))
        .await;
    fixture.favorite(id_of(&shared)).await;

    // Members don't see each other's favorites.
    assert!(fixture.favorites_as(&member_cookie).await.is_empty());
    let (status, favorite) = call_as(
        &fixture,
        &member_cookie,
        "PUT",
        &fixture.favorite_uri(id_of(&shared)),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{favorite}");
    assert_eq!(favorite["position"], 0);
    // Someone else's private page cannot be favorited.
    for method in ["PUT", "DELETE"] {
        let (status, problem) = call_as(
            &fixture,
            &member_cookie,
            method,
            &fixture.favorite_uri(id_of(&diary)),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{method}: {problem}");
        assert_eq!(problem["code"], "page_not_found");
    }
    let (status, _) = fixture
        .call("DELETE", &fixture.favorite_uri(id_of(&shared)), None)
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(fixture.favorite_ids().await.is_empty());
    assert_eq!(
        favorite_ids(fixture.favorites_as(&member_cookie).await),
        [id_of(&shared)]
    );

    // Moving the page into the owner's private space hides it from the member; moving it back
    // brings the favorite back.
    let (status, moved) = fixture
        .call(
            "POST",
            &format!("{}/move", fixture.page_uri(id_of(&shared))),
            Some(json!({"expected_version": 0, "parent_id": null, "private": true, "position": 0})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{moved}");
    assert!(fixture.favorites_as(&member_cookie).await.is_empty());
    let (status, moved) = fixture
        .call(
            "POST",
            &format!("{}/move", fixture.page_uri(id_of(&shared))),
            Some(
                json!({"expected_version": 1, "parent_id": null, "private": false, "position": 0}),
            ),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{moved}");
    assert_eq!(
        favorite_ids(fixture.favorites_as(&member_cookie).await),
        [id_of(&shared)]
    );
}

#[tokio::test]
async fn favorites_are_scoped_to_their_workspace() {
    let fixture = Fixture::new().await;
    let page = fixture.create(json!({"title": "Home"})).await;
    fixture.favorite(id_of(&page)).await;
    let foreign_workspace = create_workspace(&fixture, "Foreign").await;
    let foreign_pages = format!("/api/v1/workspaces/{foreign_workspace}/pages");
    let (status, list) = fixture
        .call("GET", &format!("{foreign_pages}/favorites"), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert_eq!(list, json!({"items": []}));
    for (method, uri, body) in [
        (
            "PUT",
            format!("{foreign_pages}/{}/favorite", id_of(&page)),
            None,
        ),
        (
            "DELETE",
            format!("{foreign_pages}/{}/favorite", id_of(&page)),
            None,
        ),
        (
            "POST",
            format!("{foreign_pages}/favorites/{}/move", id_of(&page)),
            Some(json!({"position": 0})),
        ),
        ("PUT", fixture.favorite_uri("not-an-id"), None),
    ] {
        let (status, problem) = fixture.call(method, &uri, body).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{method} {uri}: {problem}");
        assert_eq!(problem["code"], "page_not_found");
    }

    // A non-member of the workspace gets 404 for everything.
    let (_, outsider_cookie) =
        add_member(&fixture, &foreign_workspace, "outsider@example.com").await;
    for (method, uri) in [
        ("GET", fixture.favorites_uri()),
        ("PUT", fixture.favorite_uri(id_of(&page))),
    ] {
        let (status, _) = call_as(&fixture, &outsider_cookie, method, &uri, None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{method} {uri}");
    }
    assert_eq!(fixture.favorite_ids().await, [id_of(&page)]);
}

#[tokio::test]
async fn purges_take_favorites_along() {
    let fixture = Fixture::new().await;
    let expired = fixture.create(json!({"title": "Expired"})).await;
    let live = fixture.create(json!({"title": "Live"})).await;
    fixture.favorite(id_of(&expired)).await;
    fixture.favorite(id_of(&live)).await;
    fixture.trash(id_of(&expired)).await;
    sqlx::query("UPDATE pages SET deleted_at = ? WHERE id = ?")
        .bind(TimestampMillis::now().as_millis() - RETENTION_MILLIS - 1_000)
        .bind(id_of(&expired))
        .execute(fixture.database.pool())
        .await
        .unwrap();
    let workspaces = WorkspaceRepository::new((*fixture.database).clone());
    workspaces
        .purge_retention(TimestampMillis::now())
        .await
        .unwrap();
    let count = || async {
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM page_favorites")
            .fetch_one(fixture.database.pool())
            .await
            .unwrap()
    };
    assert_eq!(count().await, 1);
    assert_eq!(fixture.favorite_ids().await, [id_of(&live)]);

    sqlx::query("UPDATE workspaces SET deleted_at = ? WHERE id = ?")
        .bind(TimestampMillis::now().as_millis() - RETENTION_MILLIS - 1_000)
        .bind(&fixture.workspace_id)
        .execute(fixture.database.pool())
        .await
        .unwrap();
    workspaces
        .purge_retention(TimestampMillis::now())
        .await
        .unwrap();
    assert_eq!(count().await, 0);
}

fn favorite_ids(items: Vec<(String, i64)>) -> Vec<String> {
    items.into_iter().map(|(id, _)| id).collect()
}

fn titles(pages: &[Value]) -> Vec<&str> {
    pages
        .iter()
        .map(|page| page["title"].as_str().unwrap())
        .collect()
}

fn result_ids(results: &Value) -> Vec<&str> {
    results["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(id_of)
        .collect()
}

/// The snippet text under each highlight range (UTF-16 offsets).
fn highlighted(result: &Value) -> Vec<String> {
    let snippet = result["snippet"]
        .as_str()
        .unwrap()
        .encode_utf16()
        .collect::<Vec<_>>();
    result["snippet_highlights"]
        .as_array()
        .unwrap()
        .iter()
        .map(|range| {
            let start = range["start"].as_u64().unwrap() as usize;
            let end = range["end"].as_u64().unwrap() as usize;
            String::from_utf16(&snippet[start..end]).unwrap()
        })
        .collect()
}

/// Percent-encodes a query-string value.
fn encode(value: &str) -> String {
    value
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() {
                (byte as char).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

fn id_of(value: &Value) -> &str {
    value["id"].as_str().unwrap()
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

async fn create_workspace(fixture: &Fixture, name: &str) -> String {
    WorkspaceRepository::new((*fixture.database).clone())
        .create(
            fixture.owner_id,
            name.to_owned(),
            "pages-test",
            TimestampMillis::now(),
        )
        .await
        .unwrap()
        .id
        .to_string()
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
