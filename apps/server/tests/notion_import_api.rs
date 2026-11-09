//! Notion import API and jobs against a stateful fake Notion server (axum).

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Json;
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, Request, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use orbit_platform::{
    AttachmentMutationCoordinator, AuthenticatedUser, Id, JobStore, LocalBlobStore,
    PasswordService, TestDatabase, TimestampMillis, UploadLimits, UploadService, Worker,
    WorkerConfig,
};
use orbit_server::auth_routes::CookieMode;
use orbit_server::database_routes::{DatabaseState, database_router};
use orbit_server::import_routes::{ImportState, import_router};
use orbit_server::notion::client::NotionClientConfig;
use orbit_server::notion::import::{
    JobRun, NotionImportService, NotionImportSettings, register_jobs,
};
use orbit_server::page_file_routes::{PageFileState, page_file_router};
use orbit_server::page_routes::{PageState, page_router};
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::repositories::page_files::PageFileRepository;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

const TOKEN: &str = "ntn_good_token";
const PNG: &[u8] = b"\x89\x50\x4e\x47\x0d\x0a\x1a\x0a\x00\x00\x00\x0d\x49\x48\x44\x52\x00\x00\x00\x01\x00\x00\x00\x01\x08\x04\x00\x00\x00\xb5\x1c\x0c\x02\x00\x00\x00\x0b\x49\x44\x41\x54\x78\xda\x63\x64\xf8\x0f\x00\x01\x05\x01\x01\x27\x18\xe3\x66\x00\x00\x00\x00\x49\x45\x4e\x44\xae\x42\x60\x82";

const HANDBOOK: &str = "a0000000-0000-4000-8000-000000000001";
const ALPHA: &str = "a0000000-0000-4000-8000-000000000002";
const BETA: &str = "a0000000-0000-4000-8000-000000000003";
const IN_TOGGLE: &str = "a0000000-0000-4000-8000-000000000004";
const ORPHAN: &str = "a0000000-0000-4000-8000-000000000005";
const ROW_ONE: &str = "a0000000-0000-4000-8000-000000000006";
const ROW_TWO: &str = "a0000000-0000-4000-8000-000000000007";
const HIDDEN: &str = "a0000000-0000-4000-8000-0000000000ff";
const DATABASE: &str = "d0000000-0000-4000-8000-000000000001";
const DATA_SOURCE: &str = "d5000000-0000-4000-8000-000000000001";
const TOGGLE_OUTER: &str = "b0000000-0000-4000-8000-000000000001";
const TOGGLE_INNER: &str = "b0000000-0000-4000-8000-000000000002";

type Hook = Box<dyn Fn() -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>;

#[derive(Default)]
struct Fake {
    base: String,
    revoked: AtomicBool,
    pages: HashMap<String, Value>,
    children: HashMap<String, Vec<Value>>,
    blocks: HashMap<String, Value>,
    search: Vec<Value>,
    hits: Mutex<HashMap<String, usize>>,
    hooks: Mutex<HashMap<String, Hook>>,
    /// Search never ends: 100 generated pages per request, always another cursor.
    endless: AtomicBool,
    /// Every API call answers 503.
    unavailable: AtomicBool,
    databases: HashMap<String, Value>,
    data_sources: HashMap<String, Value>,
    /// Data source id → rows.
    queries: HashMap<String, Vec<Value>>,
    /// Database id → views; databases in `missing_views` answer 404 to the views list.
    views: HashMap<String, Vec<Value>>,
    missing_views: Vec<String>,
    /// `(page id, property id)` → property items.
    property_items: HashMap<(String, String), Vec<Value>>,
}

impl Fake {
    fn authorized(&self, headers: &HeaderMap) -> bool {
        !self.unavailable.load(Ordering::SeqCst)
            && !self.revoked.load(Ordering::SeqCst)
            && headers
                .get(header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok())
                == Some(&format!("Bearer {TOKEN}"))
    }

    fn hits(&self, key: &str) -> usize {
        self.hits.lock().unwrap().get(key).copied().unwrap_or(0)
    }

    fn on_children(&self, page: &str, hook: Hook) {
        self.hooks.lock().unwrap().insert(page.to_owned(), hook);
    }
}

fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({"object": "error", "status": 401, "code": "unauthorized", "message": "API token is invalid."})),
    )
        .into_response()
}

fn not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(json!({"object": "error", "status": 404, "code": "object_not_found", "message": "Could not find object."})),
    )
        .into_response()
}

fn list(results: Vec<Value>) -> Json<Value> {
    Json(json!({"object": "list", "results": results, "next_cursor": null, "has_more": false}))
}

fn rich(text: &str) -> Value {
    json!([{"type": "text", "text": {"content": text, "link": null}, "plain_text": text, "href": null,
            "annotations": {"bold": false, "italic": false, "strikethrough": false, "underline": false, "code": false, "color": "default"}}])
}

fn page(id: &str, parent: Value, title: &str, created: &str) -> Value {
    json!({
        "object": "page", "id": id, "parent": parent, "created_time": created, "in_trash": false,
        "icon": null, "cover": null, "url": format!("https://www.notion.so/{}", id.replace('-', "")),
        "properties": {"title": {"id": "title", "type": "title", "title": rich(title)}}
    })
}

fn block(id: &str, kind: &str, data: Value) -> Value {
    json!({"object": "block", "id": id, "type": kind, "has_children": false, "in_trash": false, kind: data})
}

fn paragraph(id: &str, rich_text: Value) -> Value {
    block(
        id,
        "paragraph",
        json!({"rich_text": rich_text, "color": "default"}),
    )
}

fn block_id(n: u32) -> String {
    format!("e0000000-0000-4000-8000-{n:012}")
}

async fn serve_fake(with_databases: bool) -> Arc<Fake> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let mut fake = build_fake(base);
    if with_databases {
        add_database_fixture(&mut fake);
    }
    let fake = Arc::new(fake);
    let router = Router::new()
        .route("/v1/users/me", get(me))
        .route("/v1/search", post(search))
        .route("/v1/pages/{id}", get(retrieve_page))
        .route("/v1/blocks/{id}", get(retrieve_block))
        .route("/v1/blocks/{id}/children", get(block_children))
        .route("/v1/databases/{id}", get(retrieve_database))
        .route("/v1/data_sources/{id}", get(retrieve_data_source))
        .route("/v1/data_sources/{id}/query", post(query_data_source))
        .route("/v1/views", get(list_views))
        .route("/v1/views/{id}", get(retrieve_view))
        .route("/v1/pages/{id}/properties/{property}", get(property_items))
        .route("/files/{name}", get(file))
        .with_state(Arc::clone(&fake));
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    fake
}

fn build_fake(base: String) -> Fake {
    let row_parent =
        json!({"type": "data_source_id", "data_source_id": DATA_SOURCE, "database_id": DATABASE});
    let mut handbook = page(
        HANDBOOK,
        json!({"type": "workspace", "workspace": true}),
        "Handbook",
        "2024-01-01T00:00:00.000Z",
    );
    handbook["icon"] = json!({"type": "emoji", "emoji": "📘"});
    handbook["cover"] = json!({"type": "file", "file": {"url": format!("{base}/files/cover.png?X-Amz-Signature=1"), "expiry_time": "2030-01-01T00:00:00.000Z"}});
    let mut row_one = page(
        ROW_ONE,
        row_parent.clone(),
        "Row one",
        "2024-01-05T00:00:00.000Z",
    );
    row_one["properties"] = json!({
        "Name": {"id": "title", "type": "title", "title": rich("Row one")},
        "Status": {"id": "st", "type": "select", "select": {"id": "opt-done", "name": "Done", "color": "green"}}
    });
    let mut in_trash = page(
        block_id(99).as_str(),
        json!({"type": "workspace", "workspace": true}),
        "Trashed",
        "2024-01-01T00:00:00.000Z",
    );
    in_trash["in_trash"] = json!(true);
    let pages = vec![
        handbook,
        page(
            ALPHA,
            json!({"type": "page_id", "page_id": HANDBOOK}),
            "Alpha",
            "2024-01-02T00:00:00.000Z",
        ),
        page(
            BETA,
            json!({"type": "page_id", "page_id": HANDBOOK}),
            "Beta",
            "2024-01-03T00:00:00.000Z",
        ),
        page(
            IN_TOGGLE,
            json!({"type": "block_id", "block_id": TOGGLE_INNER}),
            "In toggle",
            "2024-01-04T00:00:00.000Z",
        ),
        page(
            ORPHAN,
            json!({"type": "page_id", "page_id": HIDDEN}),
            "Orphan",
            "2024-02-01T00:00:00.000Z",
        ),
        row_one,
        page(ROW_TWO, row_parent, "Row two", "2024-01-06T00:00:00.000Z"),
    ];
    let mut search: Vec<Value> = pages.clone();
    search.push(in_trash);
    search.push(json!({
        "object": "data_source", "id": DATA_SOURCE, "title": rich("Tasks"), "properties": {},
        "icon": {"type": "emoji", "emoji": "✅"},
        "parent": {"type": "database_id", "database_id": DATABASE},
        "database_parent": {"type": "page_id", "page_id": BETA}
    }));
    let mention = json!([
        {"type": "text", "text": {"content": "Back to ", "link": null}, "plain_text": "Back to ", "href": null},
        {"type": "mention", "mention": {"type": "page", "page": {"id": HANDBOOK}}, "plain_text": "Handbook",
         "href": format!("https://www.notion.so/{}", HANDBOOK.replace('-', ""))}
    ]);
    let mut children = HashMap::new();
    children.insert(
        HANDBOOK.to_owned(),
        vec![
            paragraph(&block_id(1), rich("Welcome")),
            block(BETA, "child_page", json!({"title": "Beta"})),
            block(ALPHA, "child_page", json!({"title": "Alpha"})),
            block(&block_id(2), "image", json!({"type": "file", "caption": [],
                "file": {"url": format!("{base}/files/pic.png?X-Amz-Signature=2"), "expiry_time": "2030-01-01T00:00:00.000Z"}})),
            block(&block_id(3), "link_to_page", json!({"type": "page_id", "page_id": HIDDEN})),
        ],
    );
    children.insert(ALPHA.to_owned(), vec![paragraph(&block_id(4), mention)]);
    children.insert(
        BETA.to_owned(),
        vec![block(DATABASE, "child_database", json!({"title": "Tasks"}))],
    );
    children.insert(
        IN_TOGGLE.to_owned(),
        vec![paragraph(&block_id(5), rich("Nested"))],
    );
    children.insert(
        ORPHAN.to_owned(),
        vec![paragraph(&block_id(6), rich("Lonely"))],
    );
    children.insert(
        ROW_ONE.to_owned(),
        vec![paragraph(&block_id(7), rich("Row body"))],
    );
    children.insert(ROW_TWO.to_owned(), Vec::new());
    let mut blocks = HashMap::new();
    blocks.insert(
        TOGGLE_INNER.to_owned(),
        json!({"object": "block", "id": TOGGLE_INNER, "type": "toggle", "has_children": true,
               "parent": {"type": "block_id", "block_id": TOGGLE_OUTER}, "toggle": {"rich_text": []}}),
    );
    blocks.insert(
        TOGGLE_OUTER.to_owned(),
        json!({"object": "block", "id": TOGGLE_OUTER, "type": "toggle", "has_children": true,
               "parent": {"type": "page_id", "page_id": ALPHA}, "toggle": {"rich_text": []}}),
    );
    let rows = vec![pages[5].clone(), pages[6].clone()];
    Fake {
        base,
        pages: pages
            .into_iter()
            .map(|page| (page["id"].as_str().unwrap().to_owned(), page))
            .collect(),
        children,
        blocks,
        search,
        databases: HashMap::from([(
            DATABASE.to_owned(),
            json!({"object": "database", "id": DATABASE, "title": rich("Tasks"), "is_inline": true,
                   "data_sources": [{"id": DATA_SOURCE, "name": "Tasks"}]}),
        )]),
        data_sources: HashMap::from([(
            DATA_SOURCE.to_owned(),
            json!({"object": "data_source", "id": DATA_SOURCE, "title": rich("Tasks"),
            "parent": {"type": "database_id", "database_id": DATABASE},
            "properties": {
                "Name": {"id": "title", "type": "title", "title": {}},
                "Status": {"id": "st", "type": "select", "select": {"options": [
                    {"id": "opt-done", "name": "Done", "color": "green"}]}}
            }}),
        )]),
        queries: HashMap::from([(DATA_SOURCE.to_owned(), rows)]),
        ..Fake::default()
    }
}

/// Adds `fixtures/notion_import/databases.json`: a "Coverage" database with every property
/// type and three views, a page "Home" with an inline database and a full-page database with
/// two data sources.
fn add_database_fixture(fake: &mut Fake) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/notion_import/databases.json");
    let fixture: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let map = |key: &str| -> HashMap<String, Value> {
        fixture[key]
            .as_object()
            .unwrap()
            .iter()
            .map(|(id, value)| (id.clone(), value.clone()))
            .collect()
    };
    let lists = |key: &str| -> HashMap<String, Vec<Value>> {
        map(key)
            .into_iter()
            .map(|(id, value)| (id, value.as_array().unwrap().clone()))
            .collect()
    };
    fake.search
        .extend(fixture["search"].as_array().unwrap().iter().cloned());
    fake.pages.extend(map("pages"));
    fake.children.extend(lists("children"));
    fake.databases.extend(map("databases"));
    fake.data_sources.extend(map("data_sources"));
    fake.queries.extend(lists("queries"));
    fake.views.extend(lists("views"));
    fake.missing_views = fixture["missing_views"]
        .as_array()
        .unwrap()
        .iter()
        .map(|id| id.as_str().unwrap().to_owned())
        .collect();
    for entry in fixture["property_items"].as_array().unwrap() {
        fake.property_items.insert(
            (
                entry["page"].as_str().unwrap().to_owned(),
                entry["property"].as_str().unwrap().to_owned(),
            ),
            entry["items"].as_array().unwrap().clone(),
        );
    }
}

/// Answers with the object, after counting the hit and running a hook registered for `key`.
async fn serve_object(
    fake: &Fake,
    headers: &HeaderMap,
    key: String,
    object: Option<Value>,
) -> Response {
    if fake.unavailable.load(Ordering::SeqCst) {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    if !fake.authorized(headers) {
        return unauthorized();
    }
    *fake.hits.lock().unwrap().entry(key.clone()).or_default() += 1;
    let hook = fake.hooks.lock().unwrap().remove(&key);
    if let Some(hook) = hook {
        hook().await;
    }
    object.map_or_else(not_found, |object| Json(object).into_response())
}

async fn retrieve_database(
    State(fake): State<Arc<Fake>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let object = fake.databases.get(&id).cloned();
    serve_object(&fake, &headers, format!("database:{id}"), object).await
}

async fn retrieve_data_source(
    State(fake): State<Arc<Fake>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let object = fake.data_sources.get(&id).cloned();
    serve_object(&fake, &headers, format!("data_source:{id}"), object).await
}

async fn query_data_source(
    State(fake): State<Arc<Fake>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let object = fake.queries.get(&id).map(
        |rows| json!({"object": "list", "results": rows, "next_cursor": null, "has_more": false}),
    );
    serve_object(&fake, &headers, format!("query:{id}"), object).await
}

async fn list_views(
    State(fake): State<Arc<Fake>>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Response {
    let database = query.get("database_id").cloned().unwrap_or_default();
    let object = (!fake.missing_views.contains(&database)).then(|| {
        let ids: Vec<Value> = fake
            .views
            .get(&database)
            .into_iter()
            .flatten()
            .map(|view| json!({"object": "view", "id": view["id"]}))
            .collect();
        json!({"object": "list", "results": ids, "next_cursor": null, "has_more": false})
    });
    serve_object(&fake, &headers, format!("views:{database}"), object).await
}

async fn retrieve_view(
    State(fake): State<Arc<Fake>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let object = fake
        .views
        .values()
        .flatten()
        .find(|view| view["id"] == id.as_str())
        .cloned();
    serve_object(&fake, &headers, format!("view:{id}"), object).await
}

/// Property items in pages of 10 (`start_cursor` is the offset).
async fn property_items(
    State(fake): State<Arc<Fake>>,
    Path((id, property)): Path<(String, String)>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Response {
    let start: usize = query
        .get("start_cursor")
        .and_then(|cursor| cursor.parse().ok())
        .unwrap_or(0);
    let object = fake
        .property_items
        .get(&(id.clone(), property.clone()))
        .map(|items| {
            let end = (start + 10).min(items.len());
            let more = end < items.len();
            json!({"object": "list", "type": "property_item", "results": items[start..end],
                   "next_cursor": more.then(|| end.to_string()), "has_more": more,
                   "property_item": {"id": property, "type": items[0]["type"]}})
        });
    serve_object(&fake, &headers, format!("property:{id}:{property}"), object).await
}

async fn me(State(fake): State<Arc<Fake>>, headers: HeaderMap) -> Response {
    if !fake.authorized(&headers) {
        return unauthorized();
    }
    Json(json!({"object": "user", "id": "9188c6a5-7381-452f-b3dc-d4865aa89bdf", "type": "bot", "name": "Importer",
                "bot": {"owner": {"type": "workspace", "workspace": true}, "workspace_name": "Acme Notion"}}))
    .into_response()
}

async fn search(
    State(fake): State<Arc<Fake>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    if fake.unavailable.load(Ordering::SeqCst) {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    if !fake.authorized(&headers) {
        return unauthorized();
    }
    *fake
        .hits
        .lock()
        .unwrap()
        .entry("search".to_owned())
        .or_default() += 1;
    let cursor = body["start_cursor"].as_str().unwrap_or("0").to_owned();
    let hook = fake
        .hooks
        .lock()
        .unwrap()
        .remove(&format!("search:{cursor}"));
    if let Some(hook) = hook {
        hook().await;
    }
    if !fake.endless.load(Ordering::SeqCst) {
        return list(fake.search.clone()).into_response();
    }
    let batch: u32 = cursor.parse().unwrap();
    let results: Vec<Value> = (0..100)
        .map(|n| {
            let id = format!("c0000000-0000-4000-8000-{:012}", batch * 100 + n);
            page(
                &id,
                json!({"type": "workspace", "workspace": true}),
                "Generated",
                "2024-01-01T00:00:00.000Z",
            )
        })
        .collect();
    Json(json!({"object": "list", "results": results, "next_cursor": (batch + 1).to_string(), "has_more": true}))
        .into_response()
}

async fn retrieve_page(
    State(fake): State<Arc<Fake>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response {
    if fake.unavailable.load(Ordering::SeqCst) {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    if !fake.authorized(&headers) {
        return unauthorized();
    }
    fake.pages
        .get(&id)
        .map_or_else(not_found, |page| Json(page.clone()).into_response())
}

async fn retrieve_block(
    State(fake): State<Arc<Fake>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response {
    if !fake.authorized(&headers) {
        return unauthorized();
    }
    fake.blocks
        .get(&id)
        .map_or_else(not_found, |block| Json(block.clone()).into_response())
}

async fn block_children(
    State(fake): State<Arc<Fake>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response {
    if !fake.authorized(&headers) {
        return unauthorized();
    }
    *fake.hits.lock().unwrap().entry(id.clone()).or_default() += 1;
    let hook = fake.hooks.lock().unwrap().remove(&id);
    if let Some(hook) = hook {
        hook().await;
    }
    match fake.children.get(&id) {
        Some(children) => list(children.clone()).into_response(),
        None => not_found(),
    }
}

async fn file(Path(name): Path<String>) -> Response {
    let _ = name;
    ([(header::CONTENT_TYPE, "image/png")], PNG).into_response()
}

struct Harness {
    _root: tempfile::TempDir,
    database: TestDatabase,
    identity: Arc<IdentityRepository>,
    service: NotionImportService,
    fake: Arc<Fake>,
    app: Router,
    workspace_id: String,
    owner_id: Id,
    owner_cookie: String,
}

impl Harness {
    async fn new() -> Self {
        Self::build(Some([7u8; 32]), false).await
    }

    async fn with_key(app_key: Option<[u8; 32]>) -> Self {
        Self::build(app_key, false).await
    }

    /// With the database fixture (`add_database_fixture`) next to the basic pages.
    async fn with_databases() -> Self {
        Self::build(Some([7u8; 32]), true).await
    }

    async fn build(app_key: Option<[u8; 32]>, with_databases: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
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
        let uploads = UploadService::new(
            (*database).clone(),
            Arc::new(LocalBlobStore::new(root.path().join("attachments"))),
            AttachmentMutationCoordinator::default(),
            UploadLimits::new(1024 * 1024, 4 * 1024 * 1024).unwrap(),
        );
        let fake = serve_fake(with_databases).await;
        let port = fake.base.rsplit(':').next().unwrap().parse().unwrap();
        let service = NotionImportService::new(
            (*database).clone(),
            PageFileRepository::new((*database).clone(), uploads.clone()),
            NotionImportSettings {
                client: NotionClientConfig {
                    base_url: fake.base.clone(),
                    requests_per_second: 1000.0,
                    max_attempts: 2,
                    initial_backoff: Duration::from_millis(5),
                    max_backoff: Duration::from_millis(20),
                    loopback_file_port: Some(port),
                    ..NotionClientConfig::default()
                },
                app_key,
                max_file_bytes: 1024 * 1024,
            },
        );
        let app = import_router(ImportState::new(
            Arc::clone(&identity),
            service.clone(),
            CookieMode::secure(),
        ))
        .merge(page_router(PageState::new(
            Arc::clone(&identity),
            CookieMode::secure(),
        )))
        .merge(database_router(DatabaseState::new(
            Arc::clone(&identity),
            CookieMode::secure(),
        )))
        .merge(page_file_router(PageFileState::new(
            Arc::clone(&identity),
            uploads,
            CookieMode::secure(),
        )));
        Self {
            _root: root,
            database,
            identity,
            service,
            fake,
            app,
            workspace_id: setup.workspace_id.to_string(),
            owner_id: setup.user_id,
            owner_cookie: format!("__Host-orbit_session={}", setup.session.token),
        }
    }

    fn imports_uri(&self) -> String {
        format!("/api/v1/workspaces/{}/imports/notion", self.workspace_id)
    }

    async fn call(
        &self,
        cookie: &str,
        method: &str,
        uri: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let (status, bytes) = self.call_raw(cookie, method, uri, body).await;
        let value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or(Value::Null)
        };
        (status, value)
    }

    async fn call_raw(
        &self,
        cookie: &str,
        method: &str,
        uri: &str,
        body: Option<Value>,
    ) -> (StatusCode, Vec<u8>) {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::COOKIE, cookie);
        let request = match body {
            Some(value) => builder
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(value.to_string())),
            None => {
                builder = builder.header(header::CONTENT_LENGTH, 0);
                builder.body(Body::empty())
            }
        }
        .unwrap();
        let response = self.app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (status, bytes.to_vec())
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

    /// Creates an import and runs its scan job directly.
    async fn scanned(&self, cookie: &str) -> String {
        let (status, import) = self
            .call(
                cookie,
                "POST",
                &self.imports_uri(),
                Some(json!({"token": TOKEN})),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{import}");
        assert_eq!(import["status"], "scanning");
        let id = import["id"].as_str().unwrap().to_owned();
        self.service
            .run_scan(id.parse().unwrap(), &JobRun::default())
            .await
            .unwrap();
        id
    }

    async fn start(&self, cookie: &str, id: &str, body: Value) -> (StatusCode, Value) {
        self.call(
            cookie,
            "POST",
            &format!("{}/{id}/start", self.imports_uri()),
            Some(body),
        )
        .await
    }

    /// Runs the import's job as the worker does: the job row shows it running meanwhile.
    async fn run_import(&self, id: &str) {
        let set_state = |from: &'static str, to: &'static str| {
            sqlx::query(
                "UPDATE jobs SET state = $1 WHERE kind = 'notion.import' AND state = $2 \
                 AND payload_json LIKE $3",
            )
            .bind(to)
            .bind(from)
            .bind(format!("%{id}%"))
            .execute(self.database.pool())
        };
        set_state("queued", "running").await.unwrap();
        self.service
            .run_import(id.parse().unwrap(), &JobRun::default())
            .await
            .unwrap();
        set_state("running", "succeeded").await.unwrap();
    }

    async fn detail(&self, cookie: &str, id: &str) -> Value {
        let (status, import) = self
            .call(cookie, "GET", &format!("{}/{id}", self.imports_uri()), None)
            .await;
        assert_eq!(status, StatusCode::OK, "{import}");
        import
    }

    /// The detail with `?include=tree` (the chooser's one-off request).
    async fn detail_with_tree(&self, cookie: &str, id: &str) -> Value {
        let (status, import) = self
            .call(
                cookie,
                "GET",
                &format!("{}/{id}?include=tree", self.imports_uri()),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{import}");
        import
    }

    async fn token_stored(&self, id: &str) -> bool {
        sqlx::query_scalar::<_, Option<Vec<u8>>>(
            "SELECT token_ciphertext FROM notion_imports WHERE id = $1",
        )
        .bind(id)
        .fetch_one(self.database.pool())
        .await
        .unwrap()
        .is_some()
    }

    async fn page(&self, cookie: &str, page_id: &str) -> (StatusCode, Value) {
        self.call(
            cookie,
            "GET",
            &format!("/api/v1/workspaces/{}/pages/{page_id}", self.workspace_id),
            None,
        )
        .await
    }

    /// Orbit page id of an imported Notion page.
    async fn orbit_id(&self, import_id: &str, notion_id: &str) -> String {
        sqlx::query_scalar(
            "SELECT page_id FROM notion_import_items WHERE import_id = $1 AND notion_id = $2",
        )
        .bind(import_id)
        .bind(notion_id)
        .fetch_one(self.database.pool())
        .await
        .unwrap()
    }

    async fn page_count(&self) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM pages WHERE deleted_at IS NULL")
            .fetch_one(self.database.pool())
            .await
            .unwrap()
    }

    async fn page_deleted_audits(&self) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM audit_events WHERE action = 'page.deleted'")
            .fetch_one(self.database.pool())
            .await
            .unwrap()
    }

    /// `GET …/databases/{id}` as the owner.
    async fn database_detail(&self, database: &str) -> Value {
        let (status, detail) = self
            .call(
                &self.owner_cookie,
                "GET",
                &format!(
                    "/api/v1/workspaces/{}/databases/{database}",
                    self.workspace_id
                ),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{detail}");
        detail
    }

    async fn children_titles(&self, parent: &str) -> Vec<String> {
        sqlx::query_scalar("SELECT title FROM pages WHERE parent_id = $1 ORDER BY position, id")
            .bind(parent)
            .fetch_all(self.database.pool())
            .await
            .unwrap()
    }
}

fn text_of(value: &Value) -> String {
    value.to_string()
}

#[tokio::test]
async fn create_validates_token_and_requires_app_key() {
    let harness = Harness::new().await;
    let (status, problem) = harness
        .call(
            &harness.owner_cookie,
            "POST",
            &harness.imports_uri(),
            Some(json!({"token": "ntn_wrong"})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(problem["code"], "notion_token_invalid");
    let (status, problem) = harness
        .call(
            &harness.owner_cookie,
            "POST",
            &harness.imports_uri(),
            Some(json!({"token": "has space"})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(problem["code"], "notion_token_invalid");
    let (status, _) = harness
        .call(
            "",
            "POST",
            &harness.imports_uri(),
            Some(json!({"token": TOKEN})),
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let keyless = Harness::with_key(None).await;
    let (status, problem) = keyless
        .call(
            &keyless.owner_cookie,
            "POST",
            &keyless.imports_uri(),
            Some(json!({"token": TOKEN})),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(problem["code"], "app_key_missing");
}

#[tokio::test]
async fn scan_builds_the_tree_and_never_exposes_the_token() {
    let harness = Harness::new().await;
    let id = harness.scanned(&harness.owner_cookie).await;
    let (status, raw) = harness
        .call_raw(
            &harness.owner_cookie,
            "GET",
            &format!("{}/{id}?include=tree", harness.imports_uri()),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let raw = String::from_utf8(raw).unwrap();
    assert!(!raw.contains(TOKEN));
    assert!(!raw.contains("token"));
    let import: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(import["status"], "ready");
    assert_eq!(import["notion_workspace_name"], "Acme Notion");
    assert!(harness.token_stored(&id).await);
    let nodes = import["tree"]["nodes"].as_array().unwrap();
    let summary: Vec<(String, String, Value, u64)> = nodes
        .iter()
        .map(|node| {
            (
                node["title"].as_str().unwrap().to_owned(),
                node["kind"].as_str().unwrap().to_owned(),
                node["parent_id"].clone(),
                node["child_count"].as_u64().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        summary,
        vec![
            ("Handbook".into(), "page".into(), Value::Null, 2),
            ("Alpha".into(), "page".into(), json!(HANDBOOK), 1),
            ("In toggle".into(), "page".into(), json!(ALPHA), 0),
            ("Beta".into(), "page".into(), json!(HANDBOOK), 1),
            ("Tasks".into(), "database".into(), json!(BETA), 2),
            ("Row one".into(), "page".into(), json!(DATABASE), 0),
            ("Row two".into(), "page".into(), json!(DATABASE), 0),
            ("Orphan".into(), "page".into(), Value::Null, 0),
        ],
        "nested pages, block parents, a database with rows, and an orphan at the root"
    );
    assert_eq!(nodes[0]["icon"], "📘");
    assert_eq!(nodes[4]["icon"], "✅");
    assert_eq!(import["tree"]["truncated"], false);

    let (status, list) = harness
        .call(&harness.owner_cookie, "GET", &harness.imports_uri(), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list["items"].as_array().unwrap().len(), 1);
    assert_eq!(list["items"][0]["tree"], Value::Null);
}

#[tokio::test]
async fn import_detail_leaves_the_tree_out_unless_asked() {
    let harness = Harness::new().await;
    let id = harness.scanned(&harness.owner_cookie).await;

    // Polling gets the light detail: status and progress, no tree.
    let light = harness.detail(&harness.owner_cookie, &id).await;
    assert_eq!(light["status"], "ready");
    assert_eq!(light["tree"], Value::Null);

    let full = harness.detail_with_tree(&harness.owner_cookie, &id).await;
    assert_eq!(full["status"], "ready");
    assert_eq!(full["tree"]["nodes"].as_array().unwrap().len(), 8);
    assert_eq!(full["tree"]["truncated"], false);
    let mut without_tree = full.clone();
    without_tree["tree"] = Value::Null;
    assert_eq!(without_tree, light, "only the tree differs");

    // Unknown include values are rejected instead of silently ignored.
    for query in ["include=trees", "include_tree=true"] {
        let (status, problem) = harness
            .call(
                &harness.owner_cookie,
                "GET",
                &format!("{}/{id}?{query}", harness.imports_uri()),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}");
        assert_eq!(problem["code"], "invalid_request");
    }

    // Another member cannot see it with the flag either.
    let (_, member_cookie) = harness.add_member("member@example.com").await;
    let (status, _) = harness
        .call(
            &member_cookie,
            "GET",
            &format!("{}/{id}?include=tree", harness.imports_uri()),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn import_all_creates_pages_with_content_files_links_and_order() {
    let harness = Harness::new().await;
    let id = harness.scanned(&harness.owner_cookie).await;
    let (status, import) = harness
        .start(
            &harness.owner_cookie,
            &id,
            json!({"selection": {"all": true}, "destination": {}}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{import}");
    assert_eq!(import["status"], "queued");
    assert_eq!(import["progress"]["total"], 8);
    harness.run_import(&id).await;

    let import = harness.detail(&harness.owner_cookie, &id).await;
    assert_eq!(import["status"], "completed", "{import}");
    assert_eq!(
        import["progress"],
        json!({"total": 8, "done": 8, "failed": 0})
    );
    assert!(
        !harness.token_stored(&id).await,
        "the token is deleted at the end"
    );
    assert_eq!(import["destination"]["private"], false);
    assert!(import["destination"]["teamspace_id"].is_string());

    let handbook = harness.orbit_id(&id, HANDBOOK).await;
    let orphan = harness.orbit_id(&id, ORPHAN).await;
    assert_eq!(import["root_page_ids"], json!([handbook, orphan]));
    assert_eq!(harness.page_count().await, 8);

    // Child order follows the child_page blocks (Beta first), not creation time.
    assert_eq!(harness.children_titles(&handbook).await, ["Beta", "Alpha"]);
    let alpha = harness.orbit_id(&id, ALPHA).await;
    assert_eq!(harness.children_titles(&alpha).await, ["In toggle"]);
    let tasks = harness.orbit_id(&id, DATABASE).await;
    assert_eq!(
        harness.children_titles(&tasks).await,
        ["Row one", "Row two"]
    );

    let (status, page) = harness.page(&harness.owner_cookie, &handbook).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["title"], "Handbook");
    assert_eq!(page["icon"], "📘");
    assert!(page["teamspace_id"].is_string());
    let cover = page["cover_url"].as_str().unwrap();
    assert!(cover.starts_with(&format!(
        "/api/v1/workspaces/{}/pages/{handbook}/files/",
        harness.workspace_id
    )));
    let content = page["content"].as_array().unwrap();
    let types: Vec<&str> = content
        .iter()
        .map(|block| block["type"].as_str().unwrap())
        .collect();
    assert_eq!(types, ["paragraph", "page", "page", "image", "paragraph"]);
    let beta = harness.orbit_id(&id, BETA).await;
    assert_eq!(content[1]["props"]["pageId"], beta.as_str());
    assert_eq!(content[2]["props"]["pageId"], alpha.as_str());
    let image_url = content[3]["props"]["url"].as_str().unwrap();
    assert!(image_url.starts_with(&format!(
        "/api/v1/workspaces/{}/pages/{handbook}/files/",
        harness.workspace_id
    )));
    assert!(
        text_of(&content[4]).contains("https://www.notion.so/"),
        "links outside the import stay Notion links"
    );

    let (status, bytes) = harness
        .call_raw(&harness.owner_cookie, "GET", image_url, None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(bytes, PNG);

    let (_, alpha_page) = harness.page(&harness.owner_cookie, &alpha).await;
    assert!(
        text_of(&alpha_page["content"]).contains(&format!("/docs/{handbook}")),
        "page mentions are rewritten"
    );

    let (_, tasks_page) = harness.page(&harness.owner_cookie, &tasks).await;
    assert_eq!(tasks_page["title"], "Tasks");
    assert_eq!(tasks_page["kind"], "database");
    let database = harness.database_detail(&tasks).await;
    let row_one = harness.orbit_id(&id, ROW_ONE).await;
    let rows: Vec<&str> = database["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        rows,
        [row_one.as_str(), &harness.orbit_id(&id, ROW_TWO).await]
    );
    let status = &database["properties"][1];
    assert_eq!(status["name"], "Status");
    let done = status["config"]["options"][0]["id"].clone();
    assert_eq!(
        database["rows"][0]["values"][status["id"].as_str().unwrap()],
        done
    );
    let (_, beta_page) = harness.page(&harness.owner_cookie, &beta).await;
    assert_eq!(beta_page["content"][0]["type"], "database");
    assert_eq!(
        beta_page["content"][0]["props"],
        json!({"databaseId": tasks.as_str(), "linkId": ""}),
        "the inline database block shows the database's own views"
    );
    let (_, row_page) = harness.page(&harness.owner_cookie, &row_one).await;
    assert_eq!(
        row_page["content"][0]["type"], "paragraph",
        "the values live in the database, not in a table on the row page"
    );

    let report = &import["report"];
    assert_eq!(report["databases_imported"], 1);
    assert_eq!(report["files_imported"], 2);
    assert_eq!(report["missing_files"], 0);
    assert!(report["unresolved_page_links"].as_u64().unwrap() >= 1);
    assert_eq!(report["failures"], json!([]));
    assert_eq!(report["previously_imported"], 0);

    // A second import of the same pages creates copies and reports them.
    let second = harness.scanned(&harness.owner_cookie).await;
    let (status, started) = harness
        .start(
            &harness.owner_cookie,
            &second,
            json!({"selection": {"notion_ids": [ORPHAN]}}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{started}");
    assert_eq!(started["report"]["previously_imported"], 1);
}

#[tokio::test]
async fn subset_imports_selected_pages_with_descendants_into_private_space() {
    let harness = Harness::new().await;
    let (_, member_cookie) = harness.add_member("member@example.com").await;
    let id = harness.scanned(&member_cookie).await;
    let (status, import) = harness
        .start(
            &member_cookie,
            &id,
            json!({"selection": {"notion_ids": [BETA.replace('-', "")]}, "destination": {"private": true}}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{import}");
    assert_eq!(import["progress"]["total"], 4);
    harness.run_import(&id).await;
    let import = harness.detail(&member_cookie, &id).await;
    assert_eq!(import["status"], "completed");
    assert_eq!(import["destination"]["private"], true);
    let beta = harness.orbit_id(&id, BETA).await;
    assert_eq!(import["root_page_ids"], json!([beta]));
    let titles: Vec<String> = sqlx::query_scalar("SELECT title FROM pages ORDER BY created_at, id")
        .fetch_all(harness.database.pool())
        .await
        .unwrap();
    assert_eq!(titles, ["Beta", "Tasks", "Row one", "Row two"]);

    let (status, page) = harness.page(&member_cookie, &beta).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["private"], true);
    let (status, _) = harness.page(&harness.owner_cookie, &beta).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "another member cannot see a private import"
    );
}

#[tokio::test]
async fn imports_are_visible_only_to_their_creator() {
    let harness = Harness::new().await;
    let (_, member_cookie) = harness.add_member("member@example.com").await;
    let id = harness.scanned(&member_cookie).await;
    let detail = format!("{}/{id}", harness.imports_uri());
    let (status, problem) = harness
        .call(&harness.owner_cookie, "GET", &detail, None)
        .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "not even the workspace owner"
    );
    assert_eq!(problem["code"], "notion_import_not_found");
    let (status, _) = harness
        .start(
            &harness.owner_cookie,
            &id,
            json!({"selection": {"all": true}}),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = harness
        .call(
            &harness.owner_cookie,
            "POST",
            &format!("{detail}/cancel"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (_, list) = harness
        .call(&harness.owner_cookie, "GET", &harness.imports_uri(), None)
        .await;
    assert_eq!(list["items"], json!([]));
    let (status, _) = harness
        .call(
            &member_cookie,
            "GET",
            "/api/v1/workspaces/not-an-id/imports/notion",
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(harness.detail(&member_cookie, &id).await["status"], "ready");
}

#[tokio::test]
async fn start_validates_selection_destination_and_state() {
    let harness = Harness::new().await;
    let (member_id, member_cookie) = harness.add_member("member@example.com").await;
    let _ = member_id;
    let id = harness.scanned(&harness.owner_cookie).await;

    let (status, problem) = harness
        .call(
            &harness.owner_cookie,
            "POST",
            &harness.imports_uri(),
            Some(json!({"token": TOKEN})),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "a new scan replaces an unstarted one: {problem}"
    );
    let replaced = harness.detail(&harness.owner_cookie, &id).await;
    assert_eq!(replaced["status"], "cancelled");
    assert!(!harness.token_stored(&id).await);
    let (status, problem) = harness
        .call(
            &harness.owner_cookie,
            "POST",
            &harness.imports_uri(),
            Some(json!({"token": TOKEN})),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "one active import per member");
    assert_eq!(problem["code"], "import_in_progress");
    let active = problem_free_id(&harness).await;
    harness
        .service
        .run_scan(active.parse().unwrap(), &JobRun::default())
        .await
        .unwrap();

    for (body, status, code) in [
        (
            json!({"selection": {"notion_ids": ["a0000000-0000-4000-8000-000000000999"]}}),
            422,
            "validation_failed",
        ),
        (
            json!({"selection": {"notion_ids": []}}),
            422,
            "validation_failed",
        ),
        (
            json!({"selection": {"all": true, "notion_ids": [HANDBOOK]}}),
            422,
            "validation_failed",
        ),
        (
            json!({"selection": {"all": true}, "destination": {"private": true, "teamspace_id": Id::new_v7().to_string()}}),
            422,
            "validation_failed",
        ),
        (
            json!({"selection": {"all": true}, "destination": {"teamspace_id": Id::new_v7().to_string()}}),
            404,
            "teamspace_not_found",
        ),
        (
            json!({"selection": {"all": true}, "destination": {"parent_page_id": Id::new_v7().to_string()}}),
            404,
            "page_not_found",
        ),
    ] {
        let (actual, problem) = harness
            .start(&harness.owner_cookie, &active, body.clone())
            .await;
        assert_eq!(actual.as_u16(), status, "{body} {problem}");
        assert_eq!(problem["code"], code, "{body}");
    }

    // Another member's private page is not a valid destination.
    let (status, private_page) = harness
        .call(
            &member_cookie,
            "POST",
            &format!("/api/v1/workspaces/{}/pages", harness.workspace_id),
            Some(json!({"title": "Mine", "private": true})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, problem) = harness
        .start(&harness.owner_cookie, &active, json!({"selection": {"all": true}, "destination": {"parent_page_id": private_page["id"]}}))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(problem["code"], "page_not_found");

    // Under an own page: roots become its sub-pages.
    let (_, parent) = harness
        .call(
            &harness.owner_cookie,
            "POST",
            &format!("/api/v1/workspaces/{}/pages", harness.workspace_id),
            Some(json!({"title": "Imported"})),
        )
        .await;
    let parent_id = parent["id"].as_str().unwrap();
    let (status, started) = harness
        .start(&harness.owner_cookie, &active, json!({"selection": {"notion_ids": [ORPHAN]}, "destination": {"parent_page_id": parent_id}}))
        .await;
    assert_eq!(status, StatusCode::OK, "{started}");
    assert_eq!(started["destination"]["parent_page_id"], parent_id);
    let (status, problem) = harness
        .start(
            &harness.owner_cookie,
            &active,
            json!({"selection": {"all": true}}),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(problem["code"], "notion_import_state_conflict");
    harness.run_import(&active).await;
    assert_eq!(harness.children_titles(parent_id).await, ["Orphan"]);
}

async fn problem_free_id(harness: &Harness) -> String {
    sqlx::query_scalar("SELECT id FROM notion_imports WHERE status = 'scanning'")
        .fetch_one(harness.database.pool())
        .await
        .unwrap()
}

#[tokio::test]
async fn cancel_stops_a_running_import_and_deletes_the_token() {
    let harness = Harness::new().await;
    let id = harness.scanned(&harness.owner_cookie).await;
    let (status, _) = harness
        .start(
            &harness.owner_cookie,
            &id,
            json!({"selection": {"all": true}}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let service = harness.service.clone();
    let (workspace_id, owner_id) = (harness.workspace_id.parse().unwrap(), harness.owner_id);
    let import_id: Id = id.parse().unwrap();
    harness.fake.on_children(
        HANDBOOK,
        Box::new(move || {
            let service = service.clone();
            Box::pin(async move {
                service
                    .cancel(workspace_id, owner_id, import_id)
                    .await
                    .unwrap();
            })
        }),
    );
    harness.run_import(&id).await;
    let import = harness.detail(&harness.owner_cookie, &id).await;
    assert_eq!(import["status"], "cancelled");
    assert_eq!(
        import["progress"]["done"], 1,
        "the page in flight finishes, then the import stops"
    );
    assert!(!harness.token_stored(&id).await);
    assert_eq!(harness.fake.hits(ALPHA), 0);
    // Only the filled Handbook stays; the 7 empty pages pass 1 created go to the trash.
    assert_eq!(harness.page_count().await, 1);
    assert_eq!(import["report"]["unfilled_pages_trashed"], 7);
    assert_eq!(import["root_page_ids"].as_array().unwrap().len(), 1);
    let (status, _) = harness
        .page(
            &harness.owner_cookie,
            &harness.orbit_id(&id, HANDBOOK).await,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        harness.page_deleted_audits().await,
        3,
        "Alpha, Beta, Orphan"
    );
    let (status, problem) = harness
        .call(
            &harness.owner_cookie,
            "POST",
            &format!("{}/{id}/cancel", harness.imports_uri()),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(problem["code"], "notion_import_state_conflict");

    // Cancel before the job runs: nothing is created.
    let other = harness.scanned(&harness.owner_cookie).await;
    let pages = harness.page_count().await;
    harness
        .start(
            &harness.owner_cookie,
            &other,
            json!({"selection": {"all": true}}),
        )
        .await;
    let (status, cancelled) = harness
        .call(
            &harness.owner_cookie,
            "POST",
            &format!("{}/{other}/cancel", harness.imports_uri()),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cancelled["status"], "cancelled");
    harness.run_import(&other).await;
    assert_eq!(harness.page_count().await, pages);
    let import = harness.detail(&harness.owner_cookie, &other).await;
    assert_eq!(import["report"]["unfilled_pages_trashed"], 0);
}

#[tokio::test]
async fn cancelling_an_import_waiting_for_a_retry_trashes_its_unfilled_pages_at_once() {
    let harness = Harness::new().await;
    let id = harness.scanned(&harness.owner_cookie).await;
    harness
        .start(
            &harness.owner_cookie,
            &id,
            json!({"selection": {"all": true}}),
        )
        .await;
    let shutdown = CancellationToken::new();
    let trigger = shutdown.clone();
    harness.fake.on_children(
        BETA,
        Box::new(move || {
            let trigger = trigger.clone();
            Box::pin(async move { trigger.cancel() })
        }),
    );
    let run = JobRun {
        cancel: shutdown,
        last_attempt: false,
    };
    assert!(
        harness
            .service
            .run_import(id.parse().unwrap(), &run)
            .await
            .is_err()
    );
    assert_eq!(harness.page_count().await, 8);
    // A member moves one of their pages under an unfilled page: that subtree is kept.
    let orphan = harness.orbit_id(&id, ORPHAN).await;
    let (status, own) = harness
        .call(
            &harness.owner_cookie,
            "POST",
            &format!("/api/v1/workspaces/{}/pages", harness.workspace_id),
            Some(json!({"parent_id": orphan, "title": "Mine"})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{own}");

    let (status, import) = harness
        .call(
            &harness.owner_cookie,
            "POST",
            &format!("{}/{id}/cancel", harness.imports_uri()),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(import["status"], "cancelled");
    assert_eq!(
        import["report"]["unfilled_pages_trashed"], 3,
        "Tasks and its two rows; Orphan holds a page of the member"
    );
    // Handbook, Alpha, In toggle, Beta (filled), Orphan and Mine stay.
    assert_eq!(harness.page_count().await, 6);
    let (status, _) = harness.page(&harness.owner_cookie, &orphan).await;
    assert_eq!(status, StatusCode::OK);

    // The job runs again later, sees the cancellation and changes nothing.
    harness.run_import(&id).await;
    assert_eq!(harness.page_count().await, 6);
    let import = harness.detail(&harness.owner_cookie, &id).await;
    assert_eq!(import["report"]["unfilled_pages_trashed"], 3);
}

#[tokio::test]
async fn import_resumes_after_a_crash_without_duplicates() {
    let harness = Harness::new().await;
    let id = harness.scanned(&harness.owner_cookie).await;
    harness
        .start(
            &harness.owner_cookie,
            &id,
            json!({"selection": {"all": true}}),
        )
        .await;
    let shutdown = CancellationToken::new();
    let trigger = shutdown.clone();
    harness.fake.on_children(
        BETA,
        Box::new(move || {
            let trigger = trigger.clone();
            Box::pin(async move { trigger.cancel() })
        }),
    );
    let run = JobRun {
        cancel: shutdown,
        last_attempt: false,
    };
    let result = harness.service.run_import(id.parse().unwrap(), &run).await;
    assert!(result.is_err(), "a shutdown leaves the job to be retried");
    let import = harness.detail(&harness.owner_cookie, &id).await;
    assert_eq!(import["status"], "importing");
    assert_eq!(
        import["progress"]["done"], 4,
        "Handbook, Alpha, In toggle and Beta are done"
    );
    assert!(harness.token_stored(&id).await);
    assert_eq!(harness.page_count().await, 8);

    // A crash between creating a page and recording it: the page exists, the item is pending.
    sqlx::query(
        "UPDATE notion_import_items SET status = 'pending' WHERE import_id = $1 AND notion_id = $2",
    )
    .bind(&id)
    .bind(ORPHAN)
    .execute(harness.database.pool())
    .await
    .unwrap();

    harness.run_import(&id).await;
    let import = harness.detail(&harness.owner_cookie, &id).await;
    assert_eq!(import["status"], "completed");
    assert_eq!(
        import["progress"],
        json!({"total": 8, "done": 8, "failed": 0})
    );
    assert_eq!(harness.page_count().await, 8, "no page is created twice");
    assert_eq!(
        harness.fake.hits(HANDBOOK),
        1,
        "finished pages are not fetched again"
    );
    assert_eq!(harness.fake.hits(ORPHAN), 1);
    assert!(!harness.token_stored(&id).await);
}

#[tokio::test]
async fn a_revoked_token_fails_the_import_and_page_errors_are_reported() {
    let harness = Harness::new().await;
    let id = harness.scanned(&harness.owner_cookie).await;
    harness
        .start(
            &harness.owner_cookie,
            &id,
            json!({"selection": {"all": true}}),
        )
        .await;
    // A page that disappeared from Notion is recorded; the import continues.
    sqlx::query(
        "UPDATE notion_import_items SET notion_id = $1 WHERE import_id = $2 AND notion_id = $3",
    )
    .bind("f0000000-0000-4000-8000-000000000001")
    .bind(&id)
    .bind(ORPHAN)
    .execute(harness.database.pool())
    .await
    .unwrap();
    harness.run_import(&id).await;
    let import = harness.detail(&harness.owner_cookie, &id).await;
    assert_eq!(import["status"], "completed");
    assert_eq!(import["progress"]["failed"], 1);
    assert_eq!(import["report"]["failures"][0]["title"], "Orphan");

    let second = harness.scanned(&harness.owner_cookie).await;
    harness
        .start(
            &harness.owner_cookie,
            &second,
            json!({"selection": {"all": true}}),
        )
        .await;
    let before = harness.page_count().await;
    harness.fake.revoked.store(true, Ordering::SeqCst);
    harness.run_import(&second).await;
    let import = harness.detail(&harness.owner_cookie, &second).await;
    assert_eq!(import["status"], "failed");
    assert!(import["error"].as_str().unwrap().contains("revoked"));
    assert!(!harness.token_stored(&second).await);
    // Pass 1 stops at the database (it reads Notion); the 4 pages it created before are empty
    // and go to the trash, the first import's stay.
    assert_eq!(import["report"]["unfilled_pages_trashed"], 4);
    assert_eq!(import["root_page_ids"], json!([]));
    assert_eq!(harness.page_count().await, before);
    let (status, trash) = harness
        .call(
            &harness.owner_cookie,
            "GET",
            &format!("/api/v1/workspaces/{}/pages/trash", harness.workspace_id),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        trash["items"].as_array().unwrap().len(),
        1,
        "one trash entry per top-level page (Handbook; Orphan was not created)"
    );
}

#[tokio::test]
async fn unstarted_scans_expire_after_a_day() {
    let harness = Harness::new().await;
    let id = harness.scanned(&harness.owner_cookie).await;
    sqlx::query("UPDATE notion_imports SET updated_at = $1 WHERE id = $2")
        .bind(TimestampMillis::now().as_millis() - 25 * 60 * 60 * 1000)
        .bind(&id)
        .execute(harness.database.pool())
        .await
        .unwrap();
    let import = harness.detail(&harness.owner_cookie, &id).await;
    assert_eq!(import["status"], "expired");
    assert!(!harness.token_stored(&id).await);
    let (status, _) = harness
        .start(
            &harness.owner_cookie,
            &id,
            json!({"selection": {"all": true}}),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn jobs_scan_and_import_through_the_worker() {
    let harness = Harness::new().await;
    let worker = register_jobs(
        Worker::new(
            JobStore::new((*harness.database).clone()),
            WorkerConfig::new(2)
                .unwrap()
                .with_poll_interval(Duration::from_millis(10)),
        ),
        harness.service.clone(),
    )
    .unwrap();
    let shutdown = CancellationToken::new();
    let running = tokio::spawn(worker.run(shutdown.clone()));

    let (status, import) = harness
        .call(
            &harness.owner_cookie,
            "POST",
            &harness.imports_uri(),
            Some(json!({"token": TOKEN})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let id = import["id"].as_str().unwrap().to_owned();
    wait_for(&harness, &id, "ready").await;
    let payload: String =
        sqlx::query_scalar("SELECT payload_json FROM jobs WHERE kind = 'notion.scan'")
            .fetch_one(harness.database.pool())
            .await
            .unwrap();
    assert!(
        !payload.contains(TOKEN),
        "job payloads carry only the import id"
    );
    harness
        .start(
            &harness.owner_cookie,
            &id,
            json!({"selection": {"all": true}}),
        )
        .await;
    wait_for(&harness, &id, "completed").await;
    assert_eq!(harness.page_count().await, 8);
    shutdown.cancel();
    running.await.unwrap().unwrap();
}

async fn wait_for(harness: &Harness, id: &str, status: &str) {
    for _ in 0..500 {
        if harness.detail(&harness.owner_cookie, id).await["status"] == status {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("import {id} never reached {status}");
}

async fn page_files_of(harness: &Harness, page_id: &str) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM page_files WHERE page_id = $1")
        .bind(page_id)
        .fetch_one(harness.database.pool())
        .await
        .unwrap()
}

#[tokio::test]
async fn scan_stops_listing_at_the_cap_and_marks_the_tree_truncated() {
    let harness = Harness::new().await;
    harness.fake.endless.store(true, Ordering::SeqCst);
    let id = harness.scanned(&harness.owner_cookie).await;
    let import = harness.detail_with_tree(&harness.owner_cookie, &id).await;
    assert_eq!(import["status"], "ready");
    assert_eq!(import["tree"]["truncated"], true);
    assert_eq!(import["tree"]["nodes"].as_array().unwrap().len(), 5_000);
    assert_eq!(
        harness.fake.hits("search"),
        55,
        "listing stops at 5,500 results instead of following the cursor forever"
    );
}

#[tokio::test]
async fn cancelling_a_scan_stops_its_listing_promptly() {
    let harness = Harness::new().await;
    harness.fake.endless.store(true, Ordering::SeqCst);
    let (status, import) = harness
        .call(
            &harness.owner_cookie,
            "POST",
            &harness.imports_uri(),
            Some(json!({"token": TOKEN})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let id = import["id"].as_str().unwrap().to_owned();
    let import_id: Id = id.parse().unwrap();
    let service = harness.service.clone();
    let (workspace_id, owner_id) = (harness.workspace_id.parse().unwrap(), harness.owner_id);
    harness.fake.on_children(
        "search:3",
        Box::new(move || {
            let service = service.clone();
            Box::pin(async move {
                service
                    .cancel(workspace_id, owner_id, import_id)
                    .await
                    .unwrap();
            })
        }),
    );
    harness
        .service
        .run_scan(import_id, &JobRun::default())
        .await
        .unwrap();
    assert_eq!(
        harness.fake.hits("search"),
        4,
        "no request after the cancel"
    );
    let import = harness.detail_with_tree(&harness.owner_cookie, &id).await;
    assert_eq!(import["status"], "cancelled");
    assert_eq!(import["tree"], Value::Null);
    assert!(!harness.token_stored(&id).await);

    // A shutdown mid-scan leaves the scan to resume.
    let (_, second) = harness
        .call(
            &harness.owner_cookie,
            "POST",
            &harness.imports_uri(),
            Some(json!({"token": TOKEN})),
        )
        .await;
    let second = second["id"].as_str().unwrap().to_owned();
    let shutdown = CancellationToken::new();
    let trigger = shutdown.clone();
    harness.fake.on_children(
        "search:2",
        Box::new(move || {
            let trigger = trigger.clone();
            Box::pin(async move { trigger.cancel() })
        }),
    );
    let run = JobRun {
        cancel: shutdown,
        last_attempt: true,
    };
    assert!(
        harness
            .service
            .run_scan(second.parse().unwrap(), &run)
            .await
            .is_err()
    );
    assert_eq!(
        harness.detail(&harness.owner_cookie, &second).await["status"],
        "scanning"
    );
    assert!(harness.token_stored(&second).await);
}

#[tokio::test]
async fn a_shutdown_on_the_last_attempt_does_not_fail_the_import() {
    let harness = Harness::new().await;
    let id = harness.scanned(&harness.owner_cookie).await;
    harness
        .start(
            &harness.owner_cookie,
            &id,
            json!({"selection": {"all": true}}),
        )
        .await;
    let shutdown = CancellationToken::new();
    let trigger = shutdown.clone();
    harness.fake.on_children(
        ALPHA,
        Box::new(move || {
            let trigger = trigger.clone();
            Box::pin(async move { trigger.cancel() })
        }),
    );
    let run = JobRun {
        cancel: shutdown,
        last_attempt: true,
    };
    assert!(
        harness
            .service
            .run_import(id.parse().unwrap(), &run)
            .await
            .is_err()
    );
    let import = harness.detail(&harness.owner_cookie, &id).await;
    assert_eq!(import["status"], "importing");
    assert_eq!(import["progress"]["done"], 2);
    assert!(harness.token_stored(&id).await);
    harness.run_import(&id).await;
    assert_eq!(
        harness.detail(&harness.owner_cookie, &id).await["status"],
        "completed"
    );
}

#[tokio::test]
async fn a_dead_import_job_fails_the_import_and_deletes_the_token_at_once() {
    let harness = Harness::new().await;
    let id = harness.scanned(&harness.owner_cookie).await;
    harness
        .start(
            &harness.owner_cookie,
            &id,
            json!({"selection": {"all": true}}),
        )
        .await;
    // Pages exist, some filled; then the job's lease expires on its final attempt (a crash).
    let shutdown = CancellationToken::new();
    let trigger = shutdown.clone();
    harness.fake.on_children(
        BETA,
        Box::new(move || {
            let trigger = trigger.clone();
            Box::pin(async move { trigger.cancel() })
        }),
    );
    let run = JobRun {
        cancel: shutdown,
        last_attempt: false,
    };
    assert!(
        harness
            .service
            .run_import(id.parse().unwrap(), &run)
            .await
            .is_err()
    );
    sqlx::query("UPDATE jobs SET state = 'succeeded' WHERE kind = 'notion.scan'")
        .execute(harness.database.pool())
        .await
        .unwrap();
    sqlx::query("UPDATE jobs SET state = 'dead', dead_at = 1 WHERE kind = 'notion.import'")
        .execute(harness.database.pool())
        .await
        .unwrap();
    assert!(harness.token_stored(&id).await);

    let import = harness.detail(&harness.owner_cookie, &id).await;
    assert_eq!(import["status"], "failed");
    assert!(!harness.token_stored(&id).await);
    assert_eq!(
        import["report"]["unfilled_pages_trashed"], 4,
        "Tasks, its rows and Orphan"
    );
    assert_eq!(harness.page_count().await, 4);
}

#[tokio::test]
async fn an_exhausted_job_fails_the_import_when_its_last_attempt_ends() {
    let harness = Harness::new().await;
    let worker = register_jobs(
        Worker::new(
            JobStore::new((*harness.database).clone()),
            WorkerConfig::new(2)
                .unwrap()
                .with_poll_interval(Duration::from_millis(10)),
        ),
        harness.service.clone(),
    )
    .unwrap();
    let id = harness.scanned(&harness.owner_cookie).await;
    harness
        .start(
            &harness.owner_cookie,
            &id,
            json!({"selection": {"all": true}}),
        )
        .await;
    // Four attempts are already spent; Notion is down for the fifth.
    sqlx::query("UPDATE jobs SET attempt_count = 4 WHERE kind = 'notion.import'")
        .execute(harness.database.pool())
        .await
        .unwrap();
    harness.fake.unavailable.store(true, Ordering::SeqCst);
    let shutdown = CancellationToken::new();
    let running = tokio::spawn(worker.run(shutdown.clone()));
    wait_for(&harness, &id, "failed").await;
    assert!(!harness.token_stored(&id).await);
    let import = harness.detail(&harness.owner_cookie, &id).await;
    assert!(
        import["error"].as_str().unwrap().contains("503"),
        "{import}"
    );
    // The handler fails the import before it returns; the worker records the dead job after.
    let mut live = -1_i64;
    for _ in 0..500 {
        live = sqlx::query_scalar(
            "SELECT COUNT(*) FROM jobs WHERE kind = 'notion.import' AND state IN ('queued', 'running')",
        )
        .fetch_one(harness.database.pool())
        .await
        .unwrap();
        if live == 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(live, 0);
    shutdown.cancel();
    running.await.unwrap().unwrap();
}

#[tokio::test]
async fn a_retried_page_does_not_attach_its_files_twice() {
    let harness = Harness::new().await;
    let id = harness.scanned(&harness.owner_cookie).await;
    harness
        .start(
            &harness.owner_cookie,
            &id,
            json!({"selection": {"all": true}}),
        )
        .await;
    let shutdown = CancellationToken::new();
    let trigger = shutdown.clone();
    harness.fake.on_children(
        ALPHA,
        Box::new(move || {
            let trigger = trigger.clone();
            Box::pin(async move { trigger.cancel() })
        }),
    );
    let run = JobRun {
        cancel: shutdown,
        last_attempt: false,
    };
    assert!(
        harness
            .service
            .run_import(id.parse().unwrap(), &run)
            .await
            .is_err()
    );
    let handbook = harness.orbit_id(&id, HANDBOOK).await;
    assert_eq!(
        page_files_of(&harness, &handbook).await,
        2,
        "cover and image"
    );
    // A crash after Handbook's files were attached but before it was marked done.
    sqlx::query(
        "UPDATE notion_import_items SET status = 'created' WHERE import_id = $1 AND notion_id = $2",
    )
    .bind(&id)
    .bind(HANDBOOK)
    .execute(harness.database.pool())
    .await
    .unwrap();

    harness.run_import(&id).await;
    let import = harness.detail(&harness.owner_cookie, &id).await;
    assert_eq!(import["status"], "completed");
    assert_eq!(harness.fake.hits(HANDBOOK), 2, "Handbook was filled again");
    assert_eq!(
        page_files_of(&harness, &handbook).await,
        2,
        "its files were reused"
    );
    assert_eq!(import["report"]["files_imported"], 2);
    let (_, page) = harness.page(&harness.owner_cookie, &handbook).await;
    let image = page["content"][3]["props"]["url"]
        .as_str()
        .unwrap()
        .to_owned();
    let (status, _) = harness
        .call_raw(&harness.owner_cookie, "GET", &image, None)
        .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn an_import_into_a_read_only_cloud_workspace_stops() {
    let harness = Harness::new().await;
    let id = harness.scanned(&harness.owner_cookie).await;
    harness
        .start(
            &harness.owner_cookie,
            &id,
            json!({"selection": {"all": true}}),
        )
        .await;
    let before = harness.page_count().await;
    // Not the exempt root workspace and past its trial, so without a subscription it is read-only.
    sqlx::query("UPDATE workspaces SET created_at = 0, billing_exempt = 0 WHERE id = $1")
        .bind(harness.workspace_id.to_string())
        .execute(harness.database.pool())
        .await
        .unwrap();
    let _billing = orbit_server::billing::Billing::new(
        orbit_platform::BillingConfig::new(
            orbit_platform::Secret::new("sk_test_unused"),
            orbit_platform::Secret::new("whsec_unused"),
            vec![],
        ),
        (*harness.database).clone(),
        "https://orbit.test",
    );
    harness.run_import(&id).await;
    let import = harness.detail(&harness.owner_cookie, &id).await;
    assert_eq!(import["status"], "failed");
    assert_eq!(
        import["error"],
        "The import stopped because the workspace is read-only: it has no active subscription."
    );
    assert!(!harness.token_stored(&id).await);
    assert_eq!(harness.page_count().await, before);
}

const HOME: &str = "a0000000-0000-4000-8000-000000000010";
const COVERAGE_DB: &str = "d0000000-0000-4000-8000-000000000002";
const COVERAGE_DS: &str = "d5000000-0000-4000-8000-000000000002";
const INLINE_DB: &str = "d0000000-0000-4000-8000-000000000003";
const MULTI_A: &str = "d5000000-0000-4000-8000-000000000004";
const MULTI_B: &str = "d5000000-0000-4000-8000-000000000005";
const COVERAGE_ROWS: [&str; 3] = [
    "c1000000-0000-4000-8000-000000000001",
    "c1000000-0000-4000-8000-000000000002",
    "c1000000-0000-4000-8000-000000000003",
];

/// A database detail with properties and values by name.
struct Imported {
    detail: Value,
}

impl Imported {
    fn property(&self, name: &str) -> &Value {
        self.detail["properties"]
            .as_array()
            .unwrap()
            .iter()
            .find(|property| property["name"] == name)
            .unwrap_or_else(|| panic!("no property {name}"))
    }

    fn names(&self) -> Vec<&str> {
        self.detail["properties"]
            .as_array()
            .unwrap()
            .iter()
            .map(|property| property["name"].as_str().unwrap())
            .collect()
    }

    fn option(&self, property: &str, name: &str) -> &Value {
        self.property(property)["config"]["options"]
            .as_array()
            .unwrap()
            .iter()
            .find(|option| option["name"] == name)
            .unwrap_or_else(|| panic!("no option {name}"))
    }

    fn value(&self, row: &str, property: &str) -> Value {
        let row = self.detail["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|candidate| candidate["id"] == row)
            .unwrap_or_else(|| panic!("no row {row}"));
        row["values"][self.property(property)["id"].as_str().unwrap()].clone()
    }

    fn views(&self) -> Vec<&str> {
        self.detail["views"]
            .as_array()
            .unwrap()
            .iter()
            .map(|view| view["name"].as_str().unwrap())
            .collect()
    }
}

impl Harness {
    async fn imported(&self, import_id: &str, notion_id: &str) -> Imported {
        Imported {
            detail: self
                .database_detail(&self.orbit_id(import_id, notion_id).await)
                .await,
        }
    }
}

#[tokio::test]
async fn databases_import_with_typed_properties_values_and_views() {
    let harness = Harness::with_databases().await;
    let id = harness.scanned(&harness.owner_cookie).await;
    let tree = harness.detail_with_tree(&harness.owner_cookie, &id).await;
    let node = |notion_id: &str| {
        tree["tree"]["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|node| node["notion_id"] == notion_id)
            .cloned()
            .unwrap_or_else(|| panic!("no node {notion_id}"))
    };
    assert_eq!(
        node(MULTI_A)["kind"],
        "database",
        "one node per data source"
    );
    assert_eq!(node(MULTI_B)["title"], "Beta source");
    assert_eq!(node(INLINE_DB)["parent_id"], HOME);
    let (status, started) = harness
        .start(
            &harness.owner_cookie,
            &id,
            json!({"selection": {"notion_ids": [HOME, COVERAGE_DB, MULTI_A, MULTI_B]},
                   "destination": {"private": true}}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{started}");
    harness.run_import(&id).await;
    let import = harness.detail(&harness.owner_cookie, &id).await;
    assert_eq!(import["status"], "completed", "{import}");
    assert_eq!(import["progress"]["failed"], 0, "{import}");
    let report = &import["report"];
    assert_eq!(report["databases_imported"], 4);
    assert_eq!(report["rows_truncated"], 0);
    assert_eq!(
        report["skipped"],
        json!({"property:button": 1, "property:place": 1, "property:verification": 1})
    );
    assert_eq!(
        report["lossy"],
        json!({
            "files_as_names": 1, "formula_values": 1, "invalid_values": 1, "number_format": 1,
            "people_as_text": 1, "relation_titles": 1, "rollup_values": 1,
            "unmatched_people": 26, "view_filter": 1, "view_sort": 1, "view_type:calendar": 1,
            "views_unavailable": 1
        })
    );

    let coverage = harness.imported(&id, COVERAGE_DB).await;
    assert_eq!(coverage.detail["database"]["title"], "Coverage");
    let names = coverage.names();
    assert_eq!(names.len(), 24, "{names:?}");
    assert_eq!(
        names[..4],
        ["Name", "Stage", "Notes", "Attachments"],
        "the first view's columns lead"
    );
    let types: Vec<(&str, &str, Value)> = [
        "Notes",
        "Estimate",
        "Budget",
        "Ratio",
        "Coins",
        "Stage",
        "Tags",
        "State",
        "Due",
        "Owner",
        "Done",
        "Link",
        "Email",
        "Phone",
        "Attachments",
        "Created",
        "Edited",
        "Creator",
        "Editor",
        "Double",
        "Sum",
        "Related",
        "Ticket",
    ]
    .into_iter()
    .map(|name| {
        let property = coverage.property(name);
        let number = property["config"]
            .get("format")
            .cloned()
            .unwrap_or(Value::Null);
        (name, property["type"].as_str().unwrap(), number)
    })
    .collect();
    assert_eq!(
        types,
        [
            ("Notes", "text", Value::Null),
            ("Estimate", "number", json!("number")),
            ("Budget", "number", json!("currency")),
            ("Ratio", "number", json!("percent")),
            ("Coins", "number", json!("number")),
            ("Stage", "select", Value::Null),
            ("Tags", "multi_select", Value::Null),
            ("State", "status", Value::Null),
            ("Due", "date", Value::Null),
            ("Owner", "person", Value::Null),
            ("Done", "checkbox", Value::Null),
            ("Link", "url", Value::Null),
            ("Email", "text", Value::Null),
            ("Phone", "text", Value::Null),
            ("Attachments", "text", Value::Null),
            ("Created", "date", Value::Null),
            ("Edited", "date", Value::Null),
            ("Creator", "person", Value::Null),
            ("Editor", "text", Value::Null),
            ("Double", "number", json!("number")),
            ("Sum", "number", json!("number")),
            ("Related", "text", Value::Null),
            ("Ticket", "text", Value::Null),
        ]
    );
    assert_eq!(coverage.property("Budget")["config"]["currency"], "USD");
    assert_eq!(coverage.option("Stage", "Idea")["color"], "gray");
    assert_eq!(coverage.option("Stage", "Building")["color"], "blue");
    assert_eq!(
        coverage.option("Tags", "odd")["color"],
        "default",
        "names are trimmed, unknown colors become default"
    );
    let groups: Vec<(&str, &str)> = coverage.property("State")["config"]["options"]
        .as_array()
        .unwrap()
        .iter()
        .map(|option| {
            (
                option["name"].as_str().unwrap(),
                option["group"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        groups,
        [
            ("Not started", "todo"),
            ("In progress", "in_progress"),
            ("In review", "in_progress"),
            ("Done", "done")
        ]
    );

    let owner = harness.owner_id.to_string();
    let rows: Vec<String> = {
        let mut rows = Vec::new();
        for notion_id in COVERAGE_ROWS {
            rows.push(harness.orbit_id(&id, notion_id).await);
        }
        rows
    };
    let order: Vec<&str> = coverage.detail["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["id"].as_str().unwrap())
        .collect();
    assert_eq!(order, rows, "rows keep Notion's order");
    let one = rows[0].as_str();
    let stage_idea = coverage.option("Stage", "Idea")["id"].clone();
    for (property, expected) in [
        ("Notes", json!("note one")),
        ("Estimate", json!(3.0)),
        ("Budget", json!(12.5)),
        ("Ratio", json!(0.25)),
        ("Coins", json!(0.001)),
        ("Stage", stage_idea.clone()),
        (
            "Tags",
            json!([
                coverage.option("Tags", "bug")["id"],
                coverage.option("Tags", "docs")["id"]
            ]),
        ),
        ("State", coverage.option("State", "In review")["id"].clone()),
        ("Due", json!({"start": "2026-11-09T13:00:00Z", "end": null})),
        ("Owner", json!([owner])),
        ("Done", json!(true)),
        ("Link", json!("https://example.com/1")),
        ("Email", json!("one@example.com")),
        ("Phone", json!("+36 1 234 567")),
        ("Attachments", json!("brief.pdf")),
        (
            "Created",
            json!({"start": "2026-11-01T10:00:00Z", "end": null}),
        ),
        (
            "Edited",
            json!({"start": "2026-11-02T11:30:00Z", "end": null}),
        ),
        ("Creator", json!([owner])),
        ("Editor", json!("Importer bot")),
        ("Double", json!(6.0)),
        ("Sum", json!(9.0)),
        ("Related", json!("Home")),
        ("Ticket", json!("ORB-1")),
    ] {
        assert_eq!(coverage.value(one, property), expected, "{property}");
    }
    let two = rows[1].as_str();
    assert_eq!(
        coverage.value(two, "Link"),
        Value::Null,
        "not a URL: left out"
    );
    assert_eq!(coverage.value(two, "Done"), Value::Null);
    assert_eq!(coverage.value(two, "Ticket"), json!("ORB-2"));
    let three = rows[2].as_str();
    assert_eq!(
        coverage.value(three, "Owner"),
        json!([owner]),
        "people beyond the first 25 come from the property-item endpoint"
    );
    let related = coverage.value(three, "Related");
    assert!(
        related.as_str().unwrap().ends_with(", Home"),
        "26 relations: {related}"
    );

    assert_eq!(coverage.views(), ["All", "Board", "Either", "Calendar"]);
    let all = &coverage.detail["views"][0]["config"];
    let id_of = |name: &str| coverage.property(name)["id"].clone();
    let board = &coverage.detail["views"][1];
    assert_eq!(board["type"], "board");
    assert_eq!(
        board["config"]["group_by"],
        id_of("Stage"),
        "the board groups by its Notion property"
    );
    assert_eq!(board["config"]["card_properties"], json!([id_of("Tags")]));
    assert_eq!(coverage.detail["views"][3]["type"], "table");
    assert_eq!(
        all["sorts"],
        json!([{"property_id": id_of("Estimate"), "direction": "desc"}])
    );
    assert_eq!(
        all["filter"],
        json!({"op": "and", "children": [
            {"property_id": id_of("Stage"), "operator": "is", "value": [stage_idea]},
            {"property_id": id_of("Done"), "operator": "is", "value": true},
            {"property_id": id_of("State"), "operator": "is",
             "value": [coverage.option("State", "In progress")["id"]]}
        ]})
    );
    assert_eq!(
        all["columns"][0],
        json!({"property_id": id_of("Name"), "width": 320, "hidden": false})
    );
    assert_eq!(all["columns"][1]["width"], 32, "widths are clamped");
    assert_eq!(all["columns"][2]["hidden"], true);
    assert_eq!(all["wrap_cells"], true);
    assert_eq!(
        coverage.detail["views"][2]["config"]["filter"],
        json!({"op": "or", "children": [
            {"property_id": id_of("Done"), "operator": "is", "value": true},
            {"op": "and", "children": [
                {"property_id": id_of("Estimate"), "operator": ">", "value": 2.0},
                {"property_id": id_of("Stage"), "operator": "is", "value": [stage_idea]}
            ]}
        ]}),
        "a two-level OR maps intact"
    );

    let (_, row_page) = harness.page(&harness.owner_cookie, one).await;
    assert_eq!(row_page["content"][0]["type"], "paragraph");
    assert!(text_of(&row_page["content"][0]).contains("Row body"));

    let home = harness.orbit_id(&id, HOME).await;
    let inline = harness.orbit_id(&id, INLINE_DB).await;
    let (_, home_page) = harness.page(&harness.owner_cookie, &home).await;
    let blocks: Vec<&str> = home_page["content"]
        .as_array()
        .unwrap()
        .iter()
        .map(|block| block["type"].as_str().unwrap())
        .collect();
    assert_eq!(
        blocks,
        ["paragraph", "database", "page", "paragraph"],
        "only the inline database shows its views; the full-page one is a page link"
    );
    assert_eq!(
        home_page["content"][1]["props"],
        json!({"databaseId": inline.as_str(), "linkId": ""})
    );
    let multi = harness.orbit_id(&id, MULTI_A).await;
    assert_eq!(
        home_page["content"][2]["props"],
        json!({"pageId": multi.as_str()}),
        "a database with several data sources links to the first"
    );
    let (_, inline_page) = harness.page(&harness.owner_cookie, &inline).await;
    assert_eq!(inline_page["parent_id"], home.as_str());
    let inline = harness.imported(&id, INLINE_DB).await;
    assert_eq!(
        inline.views(),
        ["Table"],
        "no views from Notion: a default table"
    );
    let inline_row = harness
        .orbit_id(&id, "c2000000-0000-4000-8000-000000000001")
        .await;
    assert_eq!(inline.value(&inline_row, "Count"), json!(7.0));

    let alpha = harness.imported(&id, MULTI_A).await;
    let beta = harness.imported(&id, MULTI_B).await;
    assert_eq!(alpha.detail["database"]["title"], "Alpha source");
    assert_eq!(beta.detail["database"]["title"], "Beta source");
    assert_eq!(alpha.views(), ["Only A"]);
    assert_eq!(beta.views(), ["Table"]);
    assert_eq!(alpha.detail["rows"][0]["title"], "From A");
    assert_eq!(beta.detail["rows"][0]["title"], "From B");
}

#[tokio::test]
async fn a_database_import_resumes_its_rows_after_a_crash() {
    let harness = Harness::with_databases().await;
    let id = harness.scanned(&harness.owner_cookie).await;
    harness
        .start(
            &harness.owner_cookie,
            &id,
            json!({"selection": {"notion_ids": [COVERAGE_DB]}}),
        )
        .await;
    let shutdown = CancellationToken::new();
    let trigger = shutdown.clone();
    harness.fake.on_children(
        &format!("query:{COVERAGE_DS}"),
        Box::new(move || {
            let trigger = trigger.clone();
            Box::pin(async move { trigger.cancel() })
        }),
    );
    let run = JobRun {
        cancel: shutdown,
        last_attempt: false,
    };
    assert!(
        harness
            .service
            .run_import(id.parse().unwrap(), &run)
            .await
            .is_err()
    );
    let coverage = harness.imported(&id, COVERAGE_DB).await;
    assert_eq!(
        coverage.detail["rows"],
        json!([]),
        "stopped after the database"
    );

    harness.run_import(&id).await;
    let import = harness.detail(&harness.owner_cookie, &id).await;
    assert_eq!(import["status"], "completed", "{import}");
    assert_eq!(import["report"]["databases_imported"], 1, "counted once");
    let coverage = harness.imported(&id, COVERAGE_DB).await;
    assert_eq!(
        coverage.names().len(),
        24,
        "the schema is not created twice"
    );
    assert_eq!(coverage.detail["rows"].as_array().unwrap().len(), 3);
    let one = harness.orbit_id(&id, COVERAGE_ROWS[0]).await;
    assert_eq!(
        coverage.value(&one, "Stage"),
        coverage.option("Stage", "Idea")["id"],
        "the resumed rows find the options of the first attempt"
    );
    assert_eq!(
        coverage.value(&one, "Owner"),
        json!([harness.owner_id.to_string()])
    );
    assert_eq!(harness.fake.hits(&format!("query:{COVERAGE_DS}")), 2);
}
