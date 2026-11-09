//! Trello import API and jobs against a stateful fake Trello REST server (axum).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Json;
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, Request, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use orbit_platform::{
    AttachmentMutationCoordinator, AuthenticatedUser, Id, LocalBlobStore, PasswordService, Row,
    TestDatabase, TimestampMillis, UploadLimits, UploadService,
};
use orbit_server::auth_routes::CookieMode;
use orbit_server::repositories::attachments::AttachmentRepository;
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::trello::client::TrelloClientConfig;
use orbit_server::trello::import::{JobRun, TrelloImportService, TrelloImportSettings};
use orbit_server::trello_import_routes::{TrelloImportState, trello_import_router};
use serde_json::{Value, json};
use tower::ServiceExt;

const KEY: &str = "trellokey1";
const TOKEN: &str = "trellotoken1";
const WEBSITE: &str = "b1b1b1b1b1b1b1b1b1b1b1b1";
const PERSONAL: &str = "b2b2b2b2b2b2b2b2b2b2b2b2";
const OLD_BOARD: &str = "b3b3b3b3b3b3b3b3b3b3b3b3";
const PNG: &[u8] = b"\x89\x50\x4e\x47\x0d\x0a\x1a\x0a\x00\x00\x00\x0d\x49\x48\x44\x52\x00\x00\x00\x01\x00\x00\x00\x01\x08\x04\x00\x00\x00\xb5\x1c\x0c\x02\x00\x00\x00\x0b\x49\x44\x41\x54\x78\xda\x63\x64\xf8\x0f\x00\x01\x05\x01\x01\x27\x18\xe3\x66\x00\x00\x00\x00\x49\x45\x4e\x44\xae\x42\x60\x82";

#[derive(Default)]
struct Fake {
    base: String,
    hits: Mutex<HashMap<String, usize>>,
    /// `Authorization` headers the upload route saw.
    upload_auth: Mutex<Vec<Option<String>>>,
}

impl Fake {
    fn hits(&self, name: &str) -> usize {
        self.hits.lock().unwrap().get(name).copied().unwrap_or(0)
    }

    fn upload_url(&self) -> String {
        format!("{}/1/cards/c1/attachments/a1/download/shot.png", self.base)
    }
}

fn authorization() -> String {
    format!("OAuth oauth_consumer_key=\"{KEY}\", oauth_token=\"{TOKEN}\"")
}

fn card(id: &str, name: &str, list: &str, pos: f64) -> Value {
    json!({
        "id": id, "name": name, "desc": "", "closed": false, "idList": list, "idLabels": [],
        "idMembers": [], "start": null, "due": null, "dueComplete": false, "pos": pos,
        "url": format!("https://trello.com/c/{id}/card"), "shortUrl": format!("https://trello.com/c/{id}"),
        "dateLastActivity": "1970-01-01T00:01:40.000Z", "attachments": [], "customFieldItems": [],
    })
}

fn website_cards(upload: &str) -> Vec<Value> {
    let mut design = card("c1", "Design homepage", "l-doing", 1.0);
    design["desc"] = json!(format!("Make it **pop**\n\n![shot.png]({upload})"));
    design["idLabels"] = json!(["lb1", "lb2", "lb3"]);
    design["idMembers"] = json!(["m1", "m2", "m3"]);
    design["start"] = json!("1970-01-02T00:00:00.000Z");
    design["due"] = json!("1970-01-11T00:00:00.000Z");
    design["dueComplete"] = json!(true);
    design["attachments"] = json!([
        {"id": "a1", "name": "shot.png", "url": upload, "isUpload": true, "bytes": PNG.len()},
        {"id": "a2", "name": "PR #12", "url": "https://github.com/acme/web/pull/12", "isUpload": false},
    ]);
    design["customFieldItems"] = json!([{"idCustomField": "f1", "value": {"text": "3d"}}]);

    let mut ship = card("c2", "Ship it", "l-shipped", 1.0);
    ship["idMembers"] = json!(["m1"]);
    ship["due"] = json!("1970-01-11T00:00:00.000Z");
    ship["dueComplete"] = json!(true);

    let on_archived_list = card("c3", "Old idea", "l-old", 1.0);
    let idea = card("c4", "Brainstorm", "l-ideas", 1.0);
    vec![design, ship, on_archived_list, idea]
}

fn comment(id: &str, card: &str, author: &str, date: &str, text: &str) -> Value {
    json!({"id": id, "date": date, "data": {"text": text, "card": {"id": card}},
           "memberCreator": {"id": format!("mc-{id}"), "fullName": author, "username": "x"}})
}

async fn handle(
    State(fake): State<Arc<Fake>>,
    uri: Uri,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Response {
    let auth = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let path = uri.path().to_owned();
    let filter = query.get("filter").map(String::as_str).unwrap_or_default();
    let name = if path.ends_with("/lists") {
        format!("{path}?filter={filter}")
    } else {
        path.clone()
    };
    *fake.hits.lock().unwrap().entry(name).or_default() += 1;
    if path == "/1/cards/c1/attachments/a1/download/shot.png" {
        fake.upload_auth.lock().unwrap().push(auth.clone());
        if auth != Some(authorization()) {
            return StatusCode::UNAUTHORIZED.into_response();
        }
        return ([(header::CONTENT_TYPE, "image/png")], PNG).into_response();
    }
    if auth != Some(authorization()) {
        let message = if auth
            .as_deref()
            .is_some_and(|auth| auth.contains(&format!("oauth_consumer_key=\"{KEY}\"")))
        {
            "invalid token"
        } else {
            "invalid key"
        };
        return (StatusCode::UNAUTHORIZED, message).into_response();
    }
    let Some(rest) = path.strip_prefix("/1/") else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let (board, resource) = match rest.strip_prefix("boards/") {
        Some(rest) => rest.split_once('/').unwrap_or((rest, "")),
        None => ("", rest),
    };
    let body = match (board, resource, filter) {
        ("", "members/me", _) => {
            json!({"id": "m0", "fullName": "Ada Trello", "username": "ada"})
        }
        ("", "members/me/organizations", _) => {
            json!([{"id": "o1", "displayName": "Acme", "name": "acme"}])
        }
        ("", "members/me/boards", _) => json!([
            {"id": WEBSITE, "name": "Website Launch", "closed": false, "idOrganization": "o1"},
            {"id": PERSONAL, "name": "Personal", "closed": false, "idOrganization": null},
            {"id": OLD_BOARD, "name": "Old Board", "closed": true, "idOrganization": "o1"},
        ]),
        (OLD_BOARD, _, _) => {
            return (StatusCode::FORBIDDEN, "unauthorized permission requested").into_response();
        }
        (WEBSITE, "lists", "open") => json!([
            {"id": "l-ideas", "cards": [{"id": "c4"}]},
            {"id": "l-doing", "cards": [{"id": "c1"}]},
            {"id": "l-shipped", "cards": [{"id": "c2"}]},
        ]),
        (WEBSITE, "lists", "all") => json!([
            {"id": "l-doing", "name": "Doing", "closed": false, "pos": 2},
            {"id": "l-old", "name": "Old", "closed": true, "pos": 4},
            {"id": "l-ideas", "name": "Ideas", "closed": false, "pos": "1"},
            {"id": "l-shipped", "name": "Shipped", "closed": false, "pos": 3},
        ]),
        (WEBSITE, "labels", _) => json!([
            {"id": "lb1", "name": "Bug", "color": "red_dark"},
            {"id": "lb2", "name": "", "color": "green"},
            {"id": "lb3", "name": "", "color": null},
        ]),
        (WEBSITE, "members", _) => json!([
            {"id": "m1", "fullName": "Member", "username": "member"},
            {"id": "m2", "fullName": "Sam Twin", "username": "sam"},
            {"id": "m3", "fullName": null, "username": "ghost"},
        ]),
        (WEBSITE, "customFields", _) => {
            json!([{"id": "f1", "name": "Estimate", "type": "text", "options": null}])
        }
        (WEBSITE, "cards/open", _) => Value::Array(website_cards(&fake.upload_url())),
        (WEBSITE, "cards/closed", _) => json!([{"id": "x1"}, {"id": "x2"}]),
        (WEBSITE, "checklists", _) => json!([
            {"id": "k1", "name": "Launch", "idCard": "c1", "pos": 1, "checkItems": [
                {"name": "Ship it", "state": "incomplete", "pos": 2},
                {"name": "Write post", "state": "complete", "pos": 1},
            ]},
        ]),
        // Newest first, as Trello lists actions.
        (WEBSITE, "actions", _) => json!([
            comment(
                "a3",
                "c3",
                "Grace Hopper",
                "2024-05-02T08:00:00.000Z",
                "on an archived list"
            ),
            comment(
                "a2",
                "c1",
                "Grace Hopper",
                "2024-05-01T10:00:00.000Z",
                "Thanks!"
            ),
            comment(
                "a1",
                "c1",
                "Ada Lovelace",
                "2024-05-01T09:30:00.000Z",
                "Looks **great**"
            ),
        ]),
        (PERSONAL, "lists", "open") => json!([{"id": "l-todo", "cards": [{"id": "c5"}]}]),
        (PERSONAL, "lists", "all") => {
            json!([{"id": "l-todo", "name": "Todo", "closed": false, "pos": 1}])
        }
        (PERSONAL, "members", _) => json!([{"id": "m1", "fullName": "Member"}]),
        (PERSONAL, "customFields", _) => {
            return (StatusCode::FORBIDDEN, "Power-Up disabled").into_response();
        }
        (PERSONAL, "cards/open", _) => json!([card("c5", "Buy milk", "l-todo", 1.0)]),
        (PERSONAL, "labels" | "cards/closed" | "checklists" | "actions", _) => json!([]),
        _ => return (StatusCode::NOT_FOUND, "model not found").into_response(),
    };
    Json(body).into_response()
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
    service: TrelloImportService,
    fake: Arc<Fake>,
    app: Router,
    workspace_id: String,
    cookie: String,
    member_id: Id,
    member_cookie: String,
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
        let service = TrelloImportService::new(
            (*database).clone(),
            AttachmentRepository::new((*database).clone(), uploads),
            TrelloImportSettings {
                client: TrelloClientConfig {
                    base_url: format!("{}/1", fake.base),
                    requests_per_second: 1000.0,
                    max_attempts: 2,
                    initial_backoff: Duration::from_millis(5),
                    max_backoff: Duration::from_millis(20),
                    loopback_file_port: Some(port),
                    ..TrelloClientConfig::default()
                },
                app_key: Some([7u8; 32]),
                max_file_bytes: 1024 * 1024,
            },
        );
        let app = trello_import_router(TrelloImportState::new(
            Arc::clone(&identity),
            service.clone(),
            CookieMode::secure(),
        ));
        let workspace_id = setup.workspace_id.to_string();
        let (member_id, member_cookie) = add_member(
            &database,
            &identity,
            &workspace_id,
            "member@example.com",
            "Member",
        )
        .await;
        // Two members share a name, so a Trello member of that name matches neither.
        for email in ["sam1@example.com", "sam2@example.com"] {
            add_member(&database, &identity, &workspace_id, email, "Sam Twin").await;
        }
        Self {
            _root: root,
            database,
            service,
            fake,
            app,
            workspace_id,
            cookie: format!("__Host-orbit_session={}", setup.session.token),
            member_id,
            member_cookie,
        }
    }

    fn imports_uri(&self) -> String {
        format!("/api/v1/workspaces/{}/imports/trello", self.workspace_id)
    }

    async fn call(&self, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
        self.call_as(&self.cookie, method, uri, body).await
    }

    async fn call_as(
        &self,
        cookie: &str,
        method: &str,
        uri: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
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
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    /// Creates an import and runs its scan job.
    async fn scanned(&self) -> String {
        let (status, import) = self
            .call(
                "POST",
                &self.imports_uri(),
                Some(json!({"key": KEY, "token": TOKEN})),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{import}");
        assert_eq!(import["status"], "scanning");
        assert_eq!(import["trello_user_name"], "Ada Trello");
        let id = import["id"].as_str().unwrap().to_owned();
        self.service
            .run_scan(id.parse().unwrap(), &JobRun::default())
            .await
            .unwrap();
        id
    }

    /// Creates an import, runs its scan job and starts it with these boards.
    async fn started(&self, board_ids: &[&str]) -> String {
        let id = self.scanned().await;
        let (status, started) = self
            .call(
                "POST",
                &format!("{}/{id}/start", self.imports_uri()),
                Some(json!({"board_ids": board_ids})),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{started}");
        assert_eq!(started["status"], "queued");
        id
    }

    async fn view(&self, id: &str) -> Value {
        self.call("GET", &format!("{}/{id}", self.imports_uri()), None)
            .await
            .1
    }

    /// `(key_ciphertext IS NULL, token_ciphertext IS NULL)`.
    async fn credentials_wiped(&self, id: &str) -> (bool, bool) {
        let row = sqlx::query(
            "SELECT key_ciphertext, token_ciphertext FROM trello_imports WHERE id = $1",
        )
        .bind(id)
        .fetch_one(self.database.pool())
        .await
        .unwrap();
        (
            row.get::<Option<Vec<u8>>, _>("key_ciphertext").is_none(),
            row.get::<Option<Vec<u8>>, _>("token_ciphertext").is_none(),
        )
    }

    async fn orbit_id(&self, id: &str, kind: &str, trello_id: &str) -> String {
        sqlx::query_scalar(
            "SELECT orbit_id FROM trello_import_items WHERE import_id = $1 AND kind = $2 AND trello_id = $3",
        )
        .bind(id)
        .bind(kind)
        .bind(trello_id)
        .fetch_one(self.database.pool())
        .await
        .unwrap()
    }

    async fn statuses(&self, project_id: &str) -> Vec<(String, String)> {
        sqlx::query_as(
            "SELECT name, category FROM task_statuses WHERE project_id = $1 ORDER BY position, id",
        )
        .bind(project_id)
        .fetch_all(self.database.pool())
        .await
        .unwrap()
    }

    async fn tasks_in(&self, project_id: &str) -> Vec<String> {
        sqlx::query_scalar("SELECT title FROM tasks WHERE project_id = $1 ORDER BY title")
            .bind(project_id)
            .fetch_all(self.database.pool())
            .await
            .unwrap()
    }

    /// The imported task as a JSON object of the fields the import sets.
    async fn task(&self, task_id: &str) -> Value {
        let row = sqlx::query(
            "SELECT tasks.title, task_statuses.name, task_statuses.category, tasks.priority, \
             projects.project_key, tasks.source_url, tasks.due_start_at, tasks.due_at, \
             tasks.completed_at, tasks.description FROM tasks \
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
        let labels: Vec<(String, String)> = sqlx::query_as(
            "SELECT labels.name, labels.color FROM task_labels \
             JOIN labels ON labels.id = task_labels.label_id \
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

    async fn files(&self, task_id: &str) -> Vec<(String, String, i64)> {
        sqlx::query_as(
            "SELECT id, display_name, byte_size FROM attachment_references WHERE task_id = $1 \
             ORDER BY display_name",
        )
        .bind(task_id)
        .fetch_all(self.database.pool())
        .await
        .unwrap()
    }
}

/// Adds a workspace member with a session; returns its id and cookie.
async fn add_member(
    database: &TestDatabase,
    identity: &IdentityRepository,
    workspace_id: &str,
    email: &str,
    name: &str,
) -> (Id, String) {
    let member_id = Id::new_v7();
    let now = TimestampMillis::now().as_millis();
    sqlx::query(
        "INSERT INTO users (id, email, normalized_email, display_name, password_hash, created_at, updated_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(member_id.to_string())
    .bind(email)
    .bind(email)
    .bind(name)
    .bind(PasswordService::default().hash("member password!").unwrap())
    .bind(now)
    .bind(now)
    .execute(database.pool())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, updated_at) \
         VALUES ($1, $2, $3, 'member', 0, $4, $5)",
    )
    .bind(Id::new_v7().to_string())
    .bind(workspace_id)
    .bind(member_id.to_string())
    .bind(now)
    .bind(now)
    .execute(database.pool())
    .await
    .unwrap();
    let session = identity
        .create_session(
            &AuthenticatedUser {
                id: member_id,
                email: email.to_owned(),
                display_name: name.to_owned(),
            },
            TimestampMillis::now(),
        )
        .await
        .unwrap();
    (member_id, format!("__Host-orbit_session={}", session.token))
}

#[tokio::test]
async fn rejects_a_bad_key_or_token_with_trellos_reason() {
    let h = Harness::new().await;
    let (status, problem) = h
        .call(
            "POST",
            &h.imports_uri(),
            Some(json!({"key": "wrongkey", "token": TOKEN})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(problem["code"], "trello_key_invalid");
    assert_eq!(
        problem["detail"],
        "Trello rejected this API key (invalid key). Copy the key again from the Trello Auth tab of your Power-Up."
    );

    let (status, problem) = h
        .call(
            "POST",
            &h.imports_uri(),
            Some(json!({"key": KEY, "token": "wrongtoken"})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(problem["code"], "trello_token_invalid");
    assert_eq!(
        problem["detail"],
        "Trello rejected this token (invalid token). Generate a new token for this API key and paste it."
    );

    let (status, problem) = h
        .call(
            "POST",
            &h.imports_uri(),
            Some(json!({"key": "has space", "token": TOKEN})),
        )
        .await;
    assert_eq!(
        (status, problem["code"].as_str()),
        (StatusCode::UNPROCESSABLE_ENTITY, Some("trello_key_invalid"))
    );
    assert_eq!(
        h.fake.hits("/1/members/me"),
        2,
        "the format check needs no request"
    );
    let (_, list) = h.call("GET", &h.imports_uri(), None).await;
    assert_eq!(list["items"], json!([]), "nothing was stored");
}

#[tokio::test]
async fn imports_boards_with_lists_labels_checklists_comments_and_files() {
    let h = Harness::new().await;
    let id = h.started(&[WEBSITE, PERSONAL]).await;
    h.service
        .run_import(id.parse().unwrap(), &JobRun::default())
        .await
        .unwrap();

    let import = h.view(&id).await;
    assert_eq!(import["status"], "completed", "{import}");
    assert_eq!(
        import["progress"],
        json!({"total": 4, "done": 4, "failed": 0})
    );
    assert_eq!(
        import["report"],
        json!({"comments_imported": 2, "files_imported": 1, "missing_files": 0,
               "refused_files": 0, "links_added": 1, "checklists_added": 1,
               "unmatched_members": 2, "archived_lists_skipped": 1,
               "archived_cards_skipped": 3, "failures": []})
    );
    assert_eq!(
        h.credentials_wiped(&id).await,
        (true, true),
        "key and token wiped on completion"
    );

    let website = h.orbit_id(&id, "board", WEBSITE).await;
    let personal = h.orbit_id(&id, "board", PERSONAL).await;
    assert_eq!(import["project_ids"], json!([website, personal]));
    let projects: Vec<(String, String)> = sqlx::query_as(
        "SELECT name, project_key FROM projects WHERE id IN ($1, $2) ORDER BY created_at, id",
    )
    .bind(&website)
    .bind(&personal)
    .fetch_all(h.database.pool())
    .await
    .unwrap();
    assert_eq!(
        projects,
        [
            ("Website Launch".to_owned(), "WL".to_owned()),
            ("Personal".to_owned(), "PERS".to_owned())
        ]
    );
    let statuses = |pairs: &[(&str, &str)]| -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(name, category)| ((*name).to_owned(), (*category).to_owned()))
            .collect()
    };
    let defaults = [
        ("Backlog", "unstarted"),
        ("Todo", "unstarted"),
        ("In Progress", "started"),
        ("Done", "completed"),
        ("Cancelled", "cancelled"),
        ("Duplicate", "duplicate"),
    ];
    assert_eq!(
        h.statuses(&website).await,
        statuses(
            &[
                &defaults[..],
                &[
                    ("Ideas", "unstarted"),
                    ("Doing", "started"),
                    ("Shipped", "completed")
                ]
            ]
            .concat()
        ),
        "one status per open list in list order; the archived list is left out"
    );
    assert_eq!(
        h.statuses(&personal).await,
        statuses(&defaults),
        "a list named like a default status reuses it"
    );
    assert_eq!(
        h.tasks_in(&website).await,
        ["Brainstorm", "Design homepage", "Ship it"],
        "archived cards and cards on archived lists are left out"
    );
    assert_eq!(h.tasks_in(&personal).await, ["Buy milk"]);

    let design = h.orbit_id(&id, "card", "c1").await;
    let files = h.files(&design).await;
    assert_eq!(
        files
            .iter()
            .map(|(_, name, size)| (name.as_str(), *size))
            .collect::<Vec<_>>(),
        [("shot.png", PNG.len() as i64)]
    );
    assert_eq!(
        *h.fake.upload_auth.lock().unwrap(),
        [Some(authorization())],
        "downloaded once with the credentials"
    );
    let file_url = format!(
        "/api/v1/workspaces/{}/tasks/{design}/attachments/{}/download",
        h.workspace_id, files[0].0
    );
    assert_eq!(
        h.task(&design).await,
        json!({
            "title": "Design homepage", "status": "Doing", "category": "started",
            "priority": "none", "project_key": "WL", "source_url": "https://trello.com/c/c1",
            "due_start_at": 86_400_000, "due_at": 864_000_000, "completed_at": null,
            "description": format!(
                "Make it **pop**\n\n![shot.png]({file_url})\n\n**Launch**\n\n- [x] Write post\n- [ ] Ship it\n\n**Trello**\n\n- Members: Sam Twin, ghost\n- Due date: marked complete\n- Estimate: 3d\n\n**Links**\n\n- [PR \\#12](<https://github.com/acme/web/pull/12>)"
            ),
            "assignees": [h.member_id.to_string()],
            "labels": [["Bug", "#f87168"], ["green", "#4bce97"]],
        })
    );
    let blocks: String = sqlx::query_scalar("SELECT description_json FROM tasks WHERE id = $1")
        .bind(&design)
        .fetch_one(h.database.pool())
        .await
        .unwrap();
    let blocks: Value = serde_json::from_str(&blocks).unwrap();
    let checked: Vec<&Value> = blocks
        .as_array()
        .unwrap()
        .iter()
        .filter(|block| block["type"] == "checkListItem")
        .map(|block| &block["props"]["checked"])
        .collect();
    assert_eq!(
        checked,
        [&json!(true), &json!(false)],
        "the checklist became checklist blocks"
    );
    assert_eq!(
        h.comments(&design).await,
        [
            "**Ada Lovelace** · 2024-05-01 09:30 UTC\n\nLooks **great**",
            "**Grace Hopper** · 2024-05-01 10:00 UTC\n\nThanks!",
        ],
        "oldest first, each under its author"
    );

    assert_eq!(
        h.task(&h.orbit_id(&id, "card", "c2").await).await,
        json!({
            "title": "Ship it", "status": "Shipped", "category": "completed",
            "priority": "none", "project_key": "WL", "source_url": "https://trello.com/c/c2",
            "due_start_at": null, "due_at": 864_000_000, "completed_at": 100_000,
            "description": "", "assignees": [h.member_id.to_string()], "labels": [],
        }),
        "a completed list needs no due-complete note"
    );
    let milk = h.task(&h.orbit_id(&id, "card", "c5").await).await;
    assert_eq!(
        (&milk["status"], &milk["category"], &milk["project_key"]),
        (&json!("Todo"), &json!("unstarted"), &json!("PERS"))
    );

    let notifications: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM notifications WHERE workspace_id = $1")
            .bind(&h.workspace_id)
            .fetch_one(h.database.pool())
            .await
            .unwrap();
    assert_eq!(notifications, 0, "imports notify nobody");
}

#[tokio::test]
async fn lists_the_boards_by_workspace_and_keeps_imports_private() {
    let h = Harness::new().await;
    let id = h.scanned().await;
    let (_, scanned) = h
        .call(
            "GET",
            &format!("{}/{id}?include=tree", h.imports_uri()),
            None,
        )
        .await;
    assert_eq!(scanned["status"], "ready");
    assert_eq!(
        scanned["tree"],
        json!({"workspaces": [
            {"id": "o1", "name": "Acme", "boards": [
                {"id": WEBSITE, "name": "Website Launch", "archived": false, "card_count": 3},
                {"id": OLD_BOARD, "name": "Old Board", "archived": true, "card_count": null},
            ]},
            {"id": null, "name": "Other boards", "boards": [
                {"id": PERSONAL, "name": "Personal", "archived": false, "card_count": 1},
            ]},
        ], "truncated": false})
    );
    let (_, light) = h
        .call("GET", &format!("{}/{id}", h.imports_uri()), None)
        .await;
    assert_eq!(light["tree"], Value::Null, "polling leaves the tree out");

    let detail = format!("{}/{id}", h.imports_uri());
    for (method, uri, body) in [
        ("GET", detail.clone(), None),
        (
            "POST",
            format!("{detail}/start"),
            Some(json!({"board_ids": [WEBSITE]})),
        ),
        ("POST", format!("{detail}/cancel"), None),
    ] {
        let (status, problem) = h.call_as(&h.member_cookie, method, &uri, body).await;
        assert_eq!(
            (status, problem["code"].as_str()),
            (StatusCode::NOT_FOUND, Some("trello_import_not_found")),
            "{method} {uri}"
        );
    }
    let (status, list) = h
        .call_as(&h.member_cookie, "GET", &h.imports_uri(), None)
        .await;
    assert_eq!((status, list["items"].clone()), (StatusCode::OK, json!([])));
    let (_, list) = h.call("GET", &h.imports_uri(), None).await;
    assert_eq!(list["items"].as_array().unwrap().len(), 1);

    for body in [
        json!({"board_ids": []}),
        json!({"board_ids": ["ffffffffffffffffffffffff"]}),
    ] {
        let (status, problem) = h.call("POST", &format!("{detail}/start"), Some(body)).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{problem}");
        assert_eq!(problem["code"], "validation_failed");
    }

    assert_eq!(h.credentials_wiped(&id).await, (false, false));
    let (status, cancelled) = h.call("POST", &format!("{detail}/cancel"), None).await;
    assert_eq!(
        (status, cancelled["status"].as_str()),
        (StatusCode::OK, Some("cancelled"))
    );
    assert_eq!(
        h.credentials_wiped(&id).await,
        (true, true),
        "cancel wipes the key and token"
    );
    let (status, problem) = h.call("POST", &format!("{detail}/cancel"), None).await;
    assert_eq!(
        (status, problem["code"].as_str()),
        (StatusCode::CONFLICT, Some("trello_import_state_conflict"))
    );
}

#[tokio::test]
async fn start_waits_for_the_scan() {
    let h = Harness::new().await;
    let (status, import) = h
        .call(
            "POST",
            &h.imports_uri(),
            Some(json!({"key": KEY, "token": TOKEN})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{import}");
    let id = import["id"].as_str().unwrap();
    let (status, problem) = h
        .call(
            "POST",
            &format!("{}/{id}/start", h.imports_uri()),
            Some(json!({"board_ids": [WEBSITE]})),
        )
        .await;
    assert_eq!(
        (status, problem["code"].as_str()),
        (StatusCode::CONFLICT, Some("trello_import_state_conflict"))
    );
    let (status, problem) = h
        .call(
            "POST",
            &h.imports_uri(),
            Some(json!({"key": KEY, "token": TOKEN})),
        )
        .await;
    assert_eq!(
        (status, problem["code"].as_str()),
        (StatusCode::CONFLICT, Some("import_in_progress"))
    );
}

#[tokio::test]
async fn a_cancelled_import_stops_and_wipes_the_credentials() {
    let h = Harness::new().await;
    let id = h.started(&[WEBSITE, PERSONAL]).await;
    let (status, cancelled) = h
        .call("POST", &format!("{}/{id}/cancel", h.imports_uri()), None)
        .await;
    assert_eq!(
        (status, cancelled["status"].as_str()),
        (StatusCode::OK, Some("cancelled"))
    );
    h.service
        .run_import(id.parse().unwrap(), &JobRun::default())
        .await
        .unwrap();
    let import = h.view(&id).await;
    assert_eq!(import["status"], "cancelled");
    assert_eq!(import["project_ids"], json!([]));
    assert_eq!(h.credentials_wiped(&id).await, (true, true));
    assert_eq!(
        h.fake
            .hits(&format!("/1/boards/{WEBSITE}/lists?filter=all")),
        0,
        "the job did nothing after the cancel"
    );
}
