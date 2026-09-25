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
use axum::extract::{Path, State};
use axum::http::{HeaderMap, Request, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use orbit_platform::{
    AttachmentMutationCoordinator, AuthenticatedUser, Id, JobStore, LocalBlobStore,
    PasswordService, TestDatabase, TimestampMillis, UploadLimits, UploadService, Worker,
    WorkerConfig,
};
use orbit_server::auth_routes::CookieMode;
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

async fn serve_fake() -> Arc<Fake> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let fake = Arc::new(build_fake(base));
    let router = Router::new()
        .route("/v1/users/me", get(me))
        .route("/v1/search", post(search))
        .route("/v1/pages/{id}", get(retrieve_page))
        .route("/v1/blocks/{id}", get(retrieve_block))
        .route("/v1/blocks/{id}/children", get(block_children))
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
        "Status": {"id": "st", "type": "select", "select": {"name": "Done", "color": "green"}}
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
    Fake {
        base,
        pages: pages
            .into_iter()
            .map(|page| (page["id"].as_str().unwrap().to_owned(), page))
            .collect(),
        children,
        blocks,
        search,
        ..Fake::default()
    }
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
        Self::with_key(Some([7u8; 32])).await
    }

    async fn with_key(app_key: Option<[u8; 32]>) -> Self {
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
        let fake = serve_fake().await;
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
             VALUES (?, ?, ?, 'Member', 'unused', ?, ?)",
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
             VALUES (?, ?, ?, 'member', 0, ?, ?)",
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

    async fn run_import(&self, id: &str) {
        self.service
            .run_import(id.parse().unwrap(), &JobRun::default())
            .await
            .unwrap();
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
            "SELECT token_ciphertext FROM notion_imports WHERE id = ?",
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
            "SELECT page_id FROM notion_import_items WHERE import_id = ? AND notion_id = ?",
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

    async fn children_titles(&self, parent: &str) -> Vec<String> {
        sqlx::query_scalar("SELECT title FROM pages WHERE parent_id = ? ORDER BY position, id")
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
    let rows: Vec<&Value> = tasks_page["content"]
        .as_array()
        .unwrap()
        .iter()
        .map(|block| &block["props"]["pageId"])
        .collect();
    assert_eq!(rows.len(), 2);
    let row_one = harness.orbit_id(&id, ROW_ONE).await;
    assert_eq!(rows[0], row_one.as_str());
    let (_, row_page) = harness.page(&harness.owner_cookie, &row_one).await;
    assert_eq!(
        row_page["content"][0]["type"], "table",
        "row properties come first"
    );
    assert!(text_of(&row_page["content"][0]).contains("Done"));

    let report = &import["report"];
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
        "UPDATE notion_import_items SET status = 'pending' WHERE import_id = ? AND notion_id = ?",
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
        "UPDATE notion_import_items SET notion_id = ? WHERE import_id = ? AND notion_id = ?",
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
    // Pass 1 created all 8 pages, none was filled: all go to the trash, the first import's stay.
    assert_eq!(import["report"]["unfilled_pages_trashed"], 8);
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
        2,
        "one trash entry per top-level page (Handbook, Orphan)"
    );
}

#[tokio::test]
async fn unstarted_scans_expire_after_a_day() {
    let harness = Harness::new().await;
    let id = harness.scanned(&harness.owner_cookie).await;
    sqlx::query("UPDATE notion_imports SET updated_at = ? WHERE id = ?")
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
    sqlx::query_scalar("SELECT COUNT(*) FROM page_files WHERE page_id = ?")
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
    let live: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM jobs WHERE kind = 'notion.import' AND state IN ('queued', 'running')",
    )
    .fetch_one(harness.database.pool())
    .await
    .unwrap();
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
        "UPDATE notion_import_items SET status = 'created' WHERE import_id = ? AND notion_id = ?",
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
