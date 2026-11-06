//! Asana import API and jobs against a stateful fake Asana server (axum).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Json;
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, Request, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use orbit_platform::{
    AttachmentMutationCoordinator, Id, LocalBlobStore, PasswordService, Row, TestDatabase,
    TimestampMillis, UploadLimits, UploadService,
};
use orbit_server::asana::client::AsanaClientConfig;
use orbit_server::asana::import::{AsanaImportService, AsanaImportSettings, JobRun};
use orbit_server::asana_import_routes::{AsanaImportState, asana_import_router};
use orbit_server::auth_routes::CookieMode;
use orbit_server::repositories::attachments::AttachmentRepository;
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

const TOKEN: &str = "2/1200/1234:good";
const PNG: &[u8] = b"\x89\x50\x4e\x47\x0d\x0a\x1a\x0a\x00\x00\x00\x0d\x49\x48\x44\x52\x00\x00\x00\x01\x00\x00\x00\x01\x08\x04\x00\x00\x00\xb5\x1c\x0c\x02\x00\x00\x00\x0b\x49\x44\x41\x54\x78\xda\x63\x64\xf8\x0f\x00\x01\x05\x01\x01\x27\x18\xe3\x66\x00\x00\x00\x00\x49\x45\x4e\x44\xae\x42\x60\x82";

/// Noon UTC of a date, as the import stores Asana dates.
fn noon(days_since_epoch: i64) -> i64 {
    days_since_epoch * 86_400_000 + 43_200_000
}

#[derive(Default)]
struct Fake {
    base: String,
    revoked: AtomicBool,
    hits: Mutex<HashMap<String, usize>>,
    /// Cancelled when `/tasks/401/stories` is requested (simulates a shutdown mid-import).
    interrupt: Mutex<Option<CancellationToken>>,
}

impl Fake {
    fn hits(&self, path: &str) -> usize {
        self.hits.lock().unwrap().get(path).copied().unwrap_or(0)
    }
}

fn task(gid: &str, name: &str, project: (&str, &str), section: Option<(&str, &str)>) -> Value {
    json!({
        "gid": gid, "name": name, "notes": "", "html_notes": "<body></body>", "completed": false,
        "completed_at": null, "due_on": null, "due_at": null, "start_on": null, "start_at": null,
        "permalink_url": format!("https://app.asana.com/0/{}/{gid}", project.0),
        "assignee": null, "tags": [], "custom_fields": [], "num_subtasks": 0,
        "memberships": [{"project": {"gid": project.0, "name": project.1},
                          "section": section.map(|(gid, name)| json!({"gid": gid, "name": name}))}]
    })
}

fn tasks(base: &str) -> HashMap<&'static str, Value> {
    let _ = base;
    let mut design = task(
        "400",
        "Design homepage",
        ("200", "Website Launch"),
        Some(("300", "To do")),
    );
    design["html_notes"] = json!("<body>Make it <strong>pop</strong></body>");
    design["assignee"] = json!({"gid": "7", "name": "Member", "email": "MEMBER@example.com"});
    design["tags"] = json!([{"gid": "800", "name": "design"}]);
    design["custom_fields"] = json!([
        {"gid": "1", "name": "Priority", "display_value": "High", "enum_value": {"gid": "2", "name": "High"}},
        {"gid": "3", "name": "Effort", "display_value": "3", "enum_value": null},
        {"gid": "4", "name": "Empty", "display_value": null, "enum_value": null}
    ]);
    design["memberships"]
        .as_array_mut()
        .unwrap()
        .push(json!({"project": {"gid": "201", "name": "Ops"}, "section": {"gid": "310", "name": "Backlog"}}));
    design["due_on"] = json!("1970-01-11");
    design["start_on"] = json!("1970-01-02");
    design["num_subtasks"] = json!(1);

    let mut ship = task(
        "401",
        "Ship it",
        ("200", "Website Launch"),
        Some(("301", "In Progress")),
    );
    ship["html_notes"] = Value::Null;
    ship["notes"] = json!("Plain notes");
    ship["completed"] = json!(true);
    ship["completed_at"] = json!("1970-01-01T00:01:40.000Z");
    ship["assignee"] = json!({"gid": "8", "name": "Ghost", "email": "ghost@elsewhere.example"});
    ship["tags"] = json!([{"gid": "801", "name": "Design"}]);
    // Start after the end: the start is dropped.
    ship["due_at"] = json!("1970-01-01T00:00:10.000Z");
    ship["start_at"] = json!("1970-01-01T00:00:20.000Z");

    let mut colors = task("402", "Pick colors", ("200", "Website Launch"), None);
    colors["memberships"] = json!([]);
    colors["completed"] = json!(true);

    let ops = task("410", "Ops task", ("201", "Ops"), Some(("310", "Backlog")));
    HashMap::from([
        ("400", design),
        ("401", ship),
        ("402", colors),
        ("410", ops),
    ])
}

fn list(data: Vec<Value>) -> Response {
    Json(json!({"data": data, "next_page": null})).into_response()
}

fn error(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({"errors": [{"message": message}]}))).into_response()
}

async fn handle(
    State(fake): State<Arc<Fake>>,
    uri: Uri,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Response {
    let path = uri.path().to_owned();
    if let Some(name) = path.strip_prefix("/files/") {
        let _ = name;
        return ([(header::CONTENT_TYPE, "image/png")], PNG).into_response();
    }
    let Some(path) = path.strip_prefix("/api/1.0") else {
        return error(StatusCode::NOT_FOUND, "unknown");
    };
    *fake
        .hits
        .lock()
        .unwrap()
        .entry(path.to_owned())
        .or_default() += 1;
    if fake.revoked.load(Ordering::SeqCst)
        || headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            != Some(&format!("Bearer {TOKEN}"))
    {
        return error(StatusCode::UNAUTHORIZED, "Not Authorized");
    }
    assert_eq!(
        query.get("limit").map(String::as_str).unwrap_or("100"),
        "100"
    );
    let all = tasks(&fake.base);
    let pick = |gids: &[&str]| -> Vec<Value> { gids.iter().map(|gid| all[gid].clone()).collect() };
    match path {
        "/users/me" => {
            Json(json!({"data": {"gid": "7", "name": "Ada Asana", "email": "ada@example.com"}}))
                .into_response()
        }
        "/workspaces" => list(vec![json!({"gid": "100", "name": "Acme"})]),
        "/projects" => {
            assert_eq!(query.get("workspace").map(String::as_str), Some("100"));
            list(vec![
                json!({"gid": "201", "name": "Ops", "archived": false, "team": {"gid": "900", "name": "Eng"}}),
                json!({"gid": "200", "name": "Website Launch", "archived": false, "team": {"gid": "900", "name": "Eng"}}),
                json!({"gid": "202", "name": "Old", "archived": true, "team": null}),
            ])
        }
        "/projects/200/sections" => list(vec![
            json!({"gid": "300", "name": "To do"}),
            json!({"gid": "301", "name": "In Progress"}),
            json!({"gid": "302", "name": "done"}),
        ]),
        "/projects/201/sections" => list(vec![json!({"gid": "310", "name": "Backlog"})]),
        "/projects/200/tasks" => list(pick(&["400", "401"])),
        "/projects/201/tasks" => list(pick(&["400", "410"])),
        "/tasks/400/subtasks" => list(pick(&["402"])),
        "/tasks/400/stories" => list(vec![
            json!({"type": "system", "resource_subtype": "assigned", "text": "assigned to Member",
                   "created_at": "2024-05-01T09:00:00.000Z", "created_by": {"gid": "9", "name": "Grace"}}),
            json!({"type": "comment", "resource_subtype": "comment_added", "text": "Looks great",
                   "html_text": "<body>Looks <em>great</em></body>",
                   "created_at": "2024-05-01T09:30:00.000Z", "created_by": {"gid": "9", "name": "Grace"}}),
        ]),
        "/tasks/401/stories" => {
            if let Some(token) = fake.interrupt.lock().unwrap().take() {
                token.cancel();
            }
            list(vec![
                json!({"type": "comment", "resource_subtype": "comment_added",
                             "text": "Shipped!\nThanks", "created_at": "2024-05-02T10:00:00.000Z",
                             "created_by": {"gid": "7", "name": "Ada Asana"}}),
            ])
        }
        "/tasks/402/stories" | "/tasks/410/stories" => list(Vec::new()),
        "/attachments" => match query.get("parent").map(String::as_str) {
            Some("400") => list(vec![
                json!({"gid": "500", "name": "mock.png", "resource_subtype": "asana", "size": PNG.len(),
                       "view_url": null, "permanent_url": "https://app.asana.com/app/asana/-/get_asset?asset_id=500"}),
                json!({"gid": "501", "name": "Spec", "resource_subtype": "gdrive", "size": null,
                       "view_url": "https://drive.google.com/file/d/1", "permanent_url": null}),
            ]),
            _ => list(Vec::new()),
        },
        "/attachments/500" => Json(json!({"data": {"gid": "500", "name": "mock.png",
            "resource_subtype": "asana", "size": PNG.len(),
            "download_url": format!("{}/files/mock.png?X-Amz-Expires=120", fake.base)}}))
        .into_response(),
        "/tasks/401/dependencies" => list(vec![json!({"gid": "400", "name": "Design homepage"})]),
        "/tasks/410/dependencies" => error(StatusCode::PAYMENT_REQUIRED, "Premium feature"),
        path if path.ends_with("/dependencies") => list(Vec::new()),
        _ => error(StatusCode::NOT_FOUND, "unknown"),
    }
}

async fn serve_fake() -> Arc<Fake> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let fake = Arc::new(Fake {
        base,
        ..Fake::default()
    });
    let router = Router::new().fallback(handle).with_state(Arc::clone(&fake));
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    fake
}

struct Harness {
    _root: tempfile::TempDir,
    database: TestDatabase,
    service: AsanaImportService,
    fake: Arc<Fake>,
    app: Router,
    workspace_id: String,
    owner_id: Id,
    cookie: String,
    member_id: Id,
}

impl Harness {
    async fn new() -> Self {
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
        let service = AsanaImportService::new(
            (*database).clone(),
            AttachmentRepository::new((*database).clone(), uploads),
            AsanaImportSettings {
                client: AsanaClientConfig {
                    base_url: format!("{}/api/1.0", fake.base),
                    requests_per_second: 1000.0,
                    max_attempts: 2,
                    initial_backoff: Duration::from_millis(5),
                    max_backoff: Duration::from_millis(20),
                    loopback_file_port: Some(port),
                    ..AsanaClientConfig::default()
                },
                app_key: Some([7u8; 32]),
                max_file_bytes: 1024 * 1024,
            },
        );
        let app = asana_import_router(AsanaImportState::new(
            Arc::clone(&identity),
            service.clone(),
            CookieMode::secure(),
        ));
        let harness = Self {
            _root: root,
            database,
            service,
            fake,
            app,
            workspace_id: setup.workspace_id.to_string(),
            owner_id: setup.user_id,
            cookie: format!("__Host-orbit_session={}", setup.session.token),
            member_id: Id::new_v7(),
        };
        harness.add_member().await;
        harness
    }

    async fn add_member(&self) {
        let now = TimestampMillis::now().as_millis();
        sqlx::query(
            "INSERT INTO users (id, email, normalized_email, display_name, password_hash, created_at, updated_at) \
             VALUES ($1, 'member@example.com', 'member@example.com', 'Member', 'unused', $2, $3)",
        )
        .bind(self.member_id.to_string())
        .bind(now)
        .bind(now)
        .execute(self.database.pool())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, updated_at) \
             VALUES ($1, $2, $3, 'member', 0, $4, $5)",
        )
        .bind(Id::new_v7().to_string())
        .bind(&self.workspace_id)
        .bind(self.member_id.to_string())
        .bind(now)
        .bind(now)
        .execute(self.database.pool())
        .await
        .unwrap();
    }

    fn imports_uri(&self) -> String {
        format!("/api/v1/workspaces/{}/imports/asana", self.workspace_id)
    }

    async fn call(&self, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::COOKIE, &self.cookie);
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
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    /// Creates an import, runs its scan job and starts it with these projects.
    async fn started(&self, project_gids: &[&str]) -> String {
        let (status, import) = self
            .call("POST", &self.imports_uri(), Some(json!({"token": TOKEN})))
            .await;
        assert_eq!(status, StatusCode::CREATED, "{import}");
        assert_eq!(import["status"], "scanning");
        assert_eq!(import["asana_user_name"], "Ada Asana");
        let id = import["id"].as_str().unwrap().to_owned();
        self.service
            .run_scan(id.parse().unwrap(), &JobRun::default())
            .await
            .unwrap();
        let (status, ready) = self
            .call(
                "GET",
                &format!("{}/{id}?include=tree", self.imports_uri()),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(ready["status"], "ready");
        let (status, started) = self
            .call(
                "POST",
                &format!("{}/{id}/start", self.imports_uri()),
                Some(json!({"project_gids": project_gids})),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{started}");
        assert_eq!(started["status"], "queued");
        id
    }

    async fn run(&self, id: &str, run: &JobRun) {
        self.service.run_import(id.parse().unwrap(), run).await.ok();
    }

    async fn view(&self, id: &str) -> Value {
        self.call("GET", &format!("{}/{id}", self.imports_uri()), None)
            .await
            .1
    }

    async fn scalar(&self, sql: &str, bind: &str) -> i64 {
        sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_owned()))
            .bind(bind.to_owned())
            .fetch_one(self.database.pool())
            .await
            .unwrap()
    }

    async fn orbit_id(&self, id: &str, kind: &str, gid: &str) -> String {
        sqlx::query_scalar(
            "SELECT orbit_id FROM asana_import_items WHERE import_id = $1 AND kind = $2 AND asana_gid = $3",
        )
        .bind(id)
        .bind(kind)
        .bind(gid)
        .fetch_one(self.database.pool())
        .await
        .unwrap()
    }

    /// The imported task as a JSON object of the fields the import sets.
    async fn task(&self, task_id: &str) -> Value {
        let row = sqlx::query(
            "SELECT tasks.title, task_statuses.name, task_statuses.category, tasks.priority, \
             tasks.parent_task_id, projects.project_key, tasks.source_url, tasks.due_start_at, \
             tasks.due_at, tasks.completed_at, tasks.description FROM tasks \
             JOIN task_statuses ON task_statuses.id = tasks.status_id \
             JOIN projects ON projects.id = tasks.project_id WHERE tasks.id = $1",
        )
        .bind(task_id)
        .fetch_one(self.database.pool())
        .await
        .unwrap();
        let assignees: Vec<String> =
            sqlx::query_scalar("SELECT user_id FROM task_assignees WHERE task_id = $1")
                .bind(task_id)
                .fetch_all(self.database.pool())
                .await
                .unwrap();
        let labels: Vec<String> = sqlx::query_scalar(
            "SELECT labels.name FROM task_labels JOIN labels ON labels.id = task_labels.label_id \
             WHERE task_labels.task_id = $1 ORDER BY labels.name",
        )
        .bind(task_id)
        .fetch_all(self.database.pool())
        .await
        .unwrap();
        json!({
            "title": row.get::<String, _>("title"),
            "status": row.get::<String, _>("name"),
            "category": row.get::<String, _>("category"),
            "priority": row.get::<String, _>("priority"),
            "parent": row.get::<Option<String>, _>("parent_task_id"),
            "project_key": row.get::<String, _>("project_key"),
            "source_url": row.get::<Option<String>, _>("source_url"),
            "due_start_at": row.get::<Option<i64>, _>("due_start_at"),
            "due_at": row.get::<Option<i64>, _>("due_at"),
            "completed_at": row.get::<Option<i64>, _>("completed_at"),
            "description": row.get::<String, _>("description"),
            "assignees": assignees,
            "labels": labels,
        })
    }

    async fn comments(&self, task_id: &str) -> Vec<String> {
        sqlx::query_scalar(
            "SELECT body FROM task_comments WHERE task_id = $1 ORDER BY created_at, id",
        )
        .bind(task_id)
        .fetch_all(self.database.pool())
        .await
        .unwrap()
    }
}

#[tokio::test]
async fn imports_projects_with_full_task_data() {
    let h = Harness::new().await;
    let id = h.started(&["200", "201"]).await;
    h.run(&id, &JobRun::default()).await;

    let import = h.view(&id).await;
    assert_eq!(import["status"], "completed", "{import}");
    assert_eq!(
        import["progress"],
        json!({"total": 4, "done": 4, "failed": 0})
    );
    let mut report = import["report"].clone();
    report.as_object_mut().unwrap().remove("failures");
    assert_eq!(
        report,
        json!({"comments_imported": 2, "files_imported": 1, "missing_files": 0, "links_added": 1,
               "dependencies_linked": 1, "unmatched_assignees": 1, "skipped_subtasks": 0})
    );
    assert_eq!(import["report"]["failures"], json!([]));
    let token: Option<Vec<u8>> =
        sqlx::query_scalar("SELECT token_ciphertext FROM asana_imports WHERE id = $1")
            .bind(&id)
            .fetch_one(h.database.pool())
            .await
            .unwrap();
    assert!(token.is_none(), "token wiped on completion");

    // Projects, in selection order, with keys from their names and statuses from sections.
    let website = h.orbit_id(&id, "project", "200").await;
    let ops = h.orbit_id(&id, "project", "201").await;
    assert_eq!(import["project_ids"], json!([website, ops]));
    let projects: Vec<(String, String)> = sqlx::query_as(
        "SELECT name, project_key FROM projects WHERE id IN ($1, $2) ORDER BY created_at, id",
    )
    .bind(&website)
    .bind(&ops)
    .fetch_all(h.database.pool())
    .await
    .unwrap();
    assert_eq!(
        projects,
        [
            ("Website Launch".to_owned(), "WL".to_owned()),
            ("Ops".to_owned(), "OPS".to_owned())
        ]
    );
    let statuses: Vec<(String, String)> = sqlx::query_as(
        "SELECT name, category FROM task_statuses WHERE project_id = $1 ORDER BY position, id",
    )
    .bind(&website)
    .fetch_all(h.database.pool())
    .await
    .unwrap();
    let statuses: Vec<(&str, &str)> = statuses
        .iter()
        .map(|(name, category)| (name.as_str(), category.as_str()))
        .collect();
    assert_eq!(
        statuses,
        [
            ("Backlog", "unstarted"),
            ("Todo", "unstarted"),
            ("In Progress", "started"),
            ("Done", "completed"),
            ("Cancelled", "cancelled"),
            ("Duplicate", "duplicate"),
            ("To do", "unstarted"),
        ],
        "In Progress and done reuse the defaults; To do is new"
    );

    // Each Asana task once, the one in two projects under the first.
    let imported = h
        .scalar(
            "SELECT COUNT(*) FROM tasks WHERE project_id IN (SELECT orbit_id FROM asana_import_items WHERE import_id = $1)",
            &id,
        )
        .await;
    assert_eq!(imported, 4);

    let design = h.orbit_id(&id, "task", "400").await;
    let ship = h.orbit_id(&id, "task", "401").await;
    let colors = h.orbit_id(&id, "task", "402").await;
    let ops_task = h.orbit_id(&id, "task", "410").await;
    assert_eq!(
        h.task(&design).await,
        json!({
            "title": "Design homepage", "status": "To do", "category": "unstarted",
            "priority": "high", "parent": null, "project_key": "WL",
            "source_url": "https://app.asana.com/0/200/400",
            "due_start_at": noon(1), "due_at": noon(10), "completed_at": null,
            "description": "Make it **pop**\n\nAlso in Asana projects: Ops\n\n**Fields**\n\n- Effort: 3\n\n**Attachments**\n\n- [Spec](https://drive.google.com/file/d/1)\n",
            "assignees": [h.member_id.to_string()], "labels": ["design"],
        })
    );
    assert_eq!(
        h.task(&ship).await,
        json!({
            "title": "Ship it", "status": "Done", "category": "completed", "priority": "none",
            "parent": null, "project_key": "WL", "source_url": "https://app.asana.com/0/200/401",
            "due_start_at": null, "due_at": 10_000, "completed_at": 100_000,
            "description": "Plain notes\n\nAsana assignee: Ghost",
            "assignees": [], "labels": ["design"],
        })
    );
    let colors_task = h.task(&colors).await;
    assert_eq!(
        (
            &colors_task["parent"],
            &colors_task["status"],
            &colors_task["project_key"]
        ),
        (&json!(design), &json!("Done"), &json!("WL")),
        "a completed subtask keeps its parent open"
    );
    let ops_view = h.task(&ops_task).await;
    assert_eq!(
        (&ops_view["status"], &ops_view["project_key"]),
        (&json!("Backlog"), &json!("OPS"))
    );

    assert_eq!(
        h.comments(&design).await,
        ["**Grace** · 2024-05-01 09:30 UTC\n\nLooks *great*\n"]
    );
    assert_eq!(
        h.comments(&ship).await,
        ["**Ada Asana** · 2024-05-02 10:00 UTC\n\nShipped!\n\nThanks\n"]
    );
    let authors = h
        .scalar(
            "SELECT COUNT(DISTINCT author_id) FROM task_comments WHERE task_id IN (SELECT orbit_id FROM asana_import_items WHERE import_id = $1)",
            &id,
        )
        .await;
    assert_eq!(authors, 1, "comments are written by the importer");

    let relation: (String, String, String) =
        sqlx::query_as("SELECT task_id, related_task_id, type FROM task_relations")
            .fetch_one(h.database.pool())
            .await
            .unwrap();
    assert_eq!(
        relation,
        (design.clone(), ship.clone(), "blocks".to_owned())
    );

    let files: Vec<(String, i64)> = sqlx::query_as(
        "SELECT display_name, byte_size FROM attachment_references WHERE task_id = $1",
    )
    .bind(&design)
    .fetch_all(h.database.pool())
    .await
    .unwrap();
    assert_eq!(files, [("mock.png".to_owned(), PNG.len() as i64)]);

    let notifications = h
        .scalar(
            "SELECT COUNT(*) FROM notifications WHERE workspace_id = $1",
            &h.workspace_id,
        )
        .await;
    assert_eq!(notifications, 0, "imports notify nobody");
    let audited = h
        .scalar(
            "SELECT COUNT(*) FROM audit_events WHERE action = 'task.created' AND request_id LIKE $1",
            &format!("asana-import-{id}"),
        )
        .await;
    assert_eq!(audited, 4);
    let _ = h.owner_id;
}

#[tokio::test]
async fn resumes_after_a_restart_without_duplicates() {
    let h = Harness::new().await;
    let id = h.started(&["200", "201"]).await;
    // Shutdown while the second task is being filled.
    let token = CancellationToken::new();
    *h.fake.interrupt.lock().unwrap() = Some(token.clone());
    h.run(
        &id,
        &JobRun {
            cancel: token,
            last_attempt: false,
        },
    )
    .await;
    let import = h.view(&id).await;
    assert_eq!(import["status"], "importing");
    assert!(import["progress"]["done"].as_u64().unwrap() < 4);

    // A crash right after an Orbit row was made, before its item said so.
    sqlx::query(
        "UPDATE asana_import_items SET state = 'pending' WHERE import_id = $1 AND asana_gid IN ('200', '410')",
    )
    .bind(&id)
    .execute(h.database.pool())
    .await
    .unwrap();
    sqlx::query(
        "UPDATE asana_import_items SET state = 'created' WHERE import_id = $1 AND asana_gid = '400'",
    )
    .bind(&id)
    .execute(h.database.pool())
    .await
    .unwrap();
    h.run(&id, &JobRun::default()).await;

    let import = h.view(&id).await;
    assert_eq!(import["status"], "completed", "{import}");
    let count = |sql: &'static str| h.scalar(sql, &id);
    assert_eq!(
        count("SELECT COUNT(*) FROM projects WHERE id IN (SELECT orbit_id FROM asana_import_items WHERE import_id = $1)").await,
        2
    );
    assert_eq!(
        count("SELECT COUNT(*) FROM tasks WHERE project_id IN (SELECT orbit_id FROM asana_import_items WHERE import_id = $1)").await,
        4
    );
    assert_eq!(
        count("SELECT COUNT(*) FROM task_statuses WHERE project_id IN (SELECT orbit_id FROM asana_import_items WHERE import_id = $1 AND kind = 'project')").await,
        7 + 6
    );
    let design = h.orbit_id(&id, "task", "400").await;
    assert_eq!(h.comments(&design).await.len(), 1);
    assert_eq!(
        h.scalar(
            "SELECT COUNT(*) FROM attachment_references WHERE task_id = $1",
            &design
        )
        .await,
        1
    );
    assert_eq!(
        h.scalar(
            "SELECT COUNT(*) FROM labels WHERE project_id = $1",
            &h.orbit_id(&id, "project", "200").await
        )
        .await,
        1
    );
    assert_eq!(
        h.fake.hits("/projects/200/tasks"),
        2,
        "the reset project was listed again"
    );
}

#[tokio::test]
async fn a_revoked_token_fails_the_import_and_keeps_what_was_imported() {
    let h = Harness::new().await;
    let id = h.started(&["200"]).await;
    h.fake.revoked.store(true, Ordering::SeqCst);
    h.run(&id, &JobRun::default()).await;
    let import = h.view(&id).await;
    assert_eq!(import["status"], "failed");
    assert_eq!(
        import["error"],
        "The Asana token was revoked or is no longer valid."
    );
    assert_eq!(
        import["project_ids"].as_array().unwrap().len(),
        1,
        "the project stays"
    );
    let token: Option<Vec<u8>> =
        sqlx::query_scalar("SELECT token_ciphertext FROM asana_imports WHERE id = $1")
            .bind(&id)
            .fetch_one(h.database.pool())
            .await
            .unwrap();
    assert!(token.is_none());
}

#[tokio::test]
async fn rejects_invalid_tokens_and_selections() {
    let h = Harness::new().await;
    let (status, problem) = h
        .call("POST", &h.imports_uri(), Some(json!({"token": "2/wrong"})))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(problem["code"], "asana_token_invalid");

    let (status, import) = h
        .call("POST", &h.imports_uri(), Some(json!({"token": TOKEN})))
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let id = import["id"].as_str().unwrap().to_owned();
    let (status, problem) = h
        .call(
            "POST",
            &format!("{}/{id}/start", h.imports_uri()),
            Some(json!({"project_gids": ["200"]})),
        )
        .await;
    assert_eq!(
        (status, problem["code"].as_str()),
        (StatusCode::CONFLICT, Some("asana_import_state_conflict"))
    );
    h.service
        .run_scan(id.parse().unwrap(), &JobRun::default())
        .await
        .unwrap();
    let (_, tree) = h
        .call(
            "GET",
            &format!("{}/{id}?include=tree", h.imports_uri()),
            None,
        )
        .await;
    assert_eq!(
        tree["tree"],
        json!({"workspaces": [{"gid": "100", "name": "Acme", "projects": [
            {"gid": "202", "name": "Old", "archived": true, "team_gid": null, "team_name": null},
            {"gid": "201", "name": "Ops", "archived": false, "team_gid": "900", "team_name": "Eng"},
            {"gid": "200", "name": "Website Launch", "archived": false, "team_gid": "900", "team_name": "Eng"}
        ]}], "truncated": false})
    );
    for body in [
        json!({"project_gids": []}),
        json!({"project_gids": ["999"]}),
    ] {
        let (status, problem) = h
            .call(
                "POST",
                &format!("{}/{id}/start", h.imports_uri()),
                Some(body),
            )
            .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{problem}");
        assert_eq!(problem["code"], "validation_failed");
    }
    let (status, cancelled) = h
        .call("POST", &format!("{}/{id}/cancel", h.imports_uri()), None)
        .await;
    assert_eq!(
        (status, cancelled["status"].as_str()),
        (StatusCode::OK, Some("cancelled"))
    );
    let (status, _) = h
        .call(
            "GET",
            &format!("{}/{}", h.imports_uri(), Id::new_v7()),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
