//! Linear import API and jobs against a stateful fake Linear GraphQL server (axum).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Json;
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::State;
use axum::http::{HeaderMap, Method, Request, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use orbit_platform::{
    AttachmentMutationCoordinator, AuthenticatedUser, Id, LocalBlobStore, PasswordService, Row,
    TestDatabase, TimestampMillis, UploadLimits, UploadService,
};
use orbit_server::auth_routes::CookieMode;
use orbit_server::import_notice::{ImportNotices, ImportSource};
use orbit_server::linear::client::LinearClientConfig;
use orbit_server::linear::import::{JobRun, LinearImportService, LinearImportSettings};
use orbit_server::linear_import_routes::{LinearImportState, linear_import_router};
use orbit_server::mail::Mailer;
use orbit_server::repositories::attachments::AttachmentRepository;
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

const KEY: &str = "lin_api_good";
const PNG: &[u8] = b"\x89\x50\x4e\x47\x0d\x0a\x1a\x0a\x00\x00\x00\x0d\x49\x48\x44\x52\x00\x00\x00\x01\x00\x00\x00\x01\x08\x04\x00\x00\x00\xb5\x1c\x0c\x02\x00\x00\x00\x0b\x49\x44\x41\x54\x78\xda\x63\x64\xf8\x0f\x00\x01\x05\x01\x01\x27\x18\xe3\x66\x00\x00\x00\x00\x49\x45\x4e\x44\xae\x42\x60\x82";
/// A second file, not an image.
const NOTES: &[u8] = b"diagram notes";

/// Noon UTC of a date, as the import stores Linear due dates.
fn noon(days_since_epoch: i64) -> i64 {
    days_since_epoch * 86_400_000 + 43_200_000
}

#[derive(Default)]
struct Fake {
    base: String,
    revoked: AtomicBool,
    /// Answer the first `states` query with a RATELIMITED error.
    rate_limit_states: AtomicBool,
    /// Answer every `issues` query with a RATELIMITED error that asks for an hour's wait.
    rate_limit_issues: AtomicBool,
    /// Serve the Engineering issues in two pages, a sub-issue on the first and its parent on
    /// the second.
    paged: AtomicBool,
    /// Answer the second page of issues with a RATELIMITED error that asks for an hour's wait.
    rate_limit_page_two: AtomicBool,
    hits: Mutex<HashMap<String, usize>>,
    /// Cancelled when `/uploads/diagram` is requested (simulates a shutdown mid-import).
    interrupt: Mutex<Option<CancellationToken>>,
}

impl Fake {
    fn hits(&self, name: &str) -> usize {
        self.hits.lock().unwrap().get(name).copied().unwrap_or(0)
    }

    fn hit(&self, name: &str) {
        *self
            .hits
            .lock()
            .unwrap()
            .entry(name.to_owned())
            .or_default() += 1;
    }
}

fn page(nodes: Vec<Value>) -> Value {
    json!({"nodes": nodes, "pageInfo": {"hasNextPage": false, "endCursor": null}})
}

fn page_before(nodes: Vec<Value>, next: &str) -> Value {
    json!({"nodes": nodes, "pageInfo": {"hasNextPage": true, "endCursor": next}})
}

fn issue(id: &str, identifier: &str, title: &str, state: &str) -> Value {
    json!({
        "id": id, "identifier": identifier, "title": title, "description": null, "priority": 0,
        "url": format!("https://linear.app/acme/issue/{identifier}"), "dueDate": null,
        "completedAt": null, "canceledAt": null, "createdAt": "2024-05-01T08:00:00.000Z",
        "archivedAt": null, "estimate": null, "slaBreachesAt": null, "trashed": false,
        "state": {"id": state}, "assignee": null, "parent": null, "project": null, "cycle": null,
        "projectMilestone": null, "labels": page(Vec::new()), "relations": page(Vec::new()),
        "attachments": page(Vec::new()), "comments": page(Vec::new()),
    })
}

fn relation(id: &str, kind: &str, related: &str) -> Value {
    json!({"id": id, "type": kind, "relatedIssue": {"id": related}})
}

fn eng_issues(base: &str) -> Vec<Value> {
    let mut design = issue("i1", "ENG-1", "Design homepage", "s-review");
    design["description"] = json!(format!(
        "Make it **pop**\n\n![shot.png]({base}/uploads/shot)"
    ));
    design["priority"] = json!(2);
    design["dueDate"] = json!("1970-01-11");
    design["assignee"] = json!({"id": "u2", "name": "Member", "displayName": "member", "email": "MEMBER@example.com"});
    design["labels"] = page(vec![
        json!({"id": "l1", "name": "Bug", "parent": {"name": "Type"}}),
        json!({"id": "l2", "name": "design", "parent": null}),
    ]);
    design["project"] = json!({"name": "Launch"});
    design["cycle"] = json!({"number": 3, "name": "Sprint 3"});
    design["estimate"] = json!(3);
    design["attachments"] = page(vec![
        json!({"id": "a1", "title": "PR #12", "url": "https://github.com/acme/web/pull/12"}),
    ]);
    design["comments"] = page(vec![
        json!({"id": "c2", "body": format!("Thanks ![d]({base}/uploads/diagram)"),
               "createdAt": "2024-05-01T10:00:00.000Z", "user": null,
               "botActor": {"name": "GitHub"}, "externalUser": null, "parent": {"id": "c1"}}),
        json!({"id": "c1", "body": format!("Looks great, see [old]({base}/uploads/gone)"),
               "createdAt": "2024-05-01T09:30:00.000Z",
               "user": {"id": "u3", "name": "Grace Hopper", "displayName": "Grace", "email": "grace@elsewhere.example"},
               "botActor": null, "externalUser": null, "parent": null}),
    ]);
    design["relations"] = page(vec![
        relation("r1", "blocks", "i2"),
        relation("r2", "related", "i3"),
    ]);

    let mut ship = issue("i2", "ENG-2", "Ship it", "s-done");
    ship["priority"] = json!(1);
    ship["completedAt"] = json!("1970-01-01T00:01:40.000Z");
    ship["assignee"] = json!({"id": "u4", "name": "Ghost", "displayName": "ghost", "email": "ghost@elsewhere.example"});
    // The same pair as r1: Orbit keeps one relation per pair.
    ship["relations"] = page(vec![relation("r3", "related", "i1")]);

    let mut sub = issue("i3", "ENG-3", "Sub task", "s-todo");
    sub["parent"] = json!({"id": "i1", "identifier": "ENG-1"});
    // A task duplicates at most one other task.
    sub["relations"] = page(vec![
        relation("r4", "duplicate", "i4"),
        relation("r5", "duplicate", "i5"),
    ]);

    let mut original = issue("i4", "ENG-4", "Original", "s-backlog");
    original["relations"] = page(vec![
        relation("r6", "similar", "i1"),
        relation("r7", "blocks", "o1"),
    ]);

    let other = issue("i5", "ENG-5", "Another original", "s-dup");
    let mut trashed = issue("i6", "ENG-6", "Deleted", "s-todo");
    trashed["trashed"] = json!(true);
    vec![design, ship, sub, original, other, trashed]
}

fn ops_issues() -> Vec<Value> {
    let mut child = issue("o1", "OPS-1", "Ops child", "o-todo");
    child["parent"] = json!({"id": "i1", "identifier": "ENG-1"});
    vec![child]
}

fn graphql_error(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        Json(json!({"errors": [{"message": message, "extensions": {"code": code}}]})),
    )
        .into_response()
}

async fn handle(
    State(fake): State<Arc<Fake>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let authorized = !fake.revoked.load(Ordering::SeqCst)
        && headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            == Some(KEY);
    if let Some(name) = uri.path().strip_prefix("/uploads/") {
        fake.hit(&format!("upload {name}"));
        if !authorized {
            return StatusCode::UNAUTHORIZED.into_response();
        }
        if name == "diagram"
            && let Some(token) = fake.interrupt.lock().unwrap().take()
        {
            token.cancel();
        }
        return match name {
            "shot" => ([(header::CONTENT_TYPE, "image/png")], PNG).into_response(),
            "diagram" => ([(header::CONTENT_TYPE, "text/plain")], NOTES).into_response(),
            _ => StatusCode::NOT_FOUND.into_response(),
        };
    }
    assert_eq!((method, uri.path()), (Method::POST, "/graphql"));
    if !authorized {
        return graphql_error(
            StatusCode::BAD_REQUEST,
            "AUTHENTICATION_ERROR",
            "Authentication required, not authenticated",
        );
    }
    let body: Value = serde_json::from_slice(&body).unwrap();
    let query = body["query"].as_str().unwrap_or_default();
    let id = body["variables"]["id"].as_str().unwrap_or_default();
    let data = if query.contains("viewer") {
        fake.hit("viewer");
        json!({"viewer": {"id": "u1", "name": "Ada Linear", "displayName": "ada", "email": "ada@example.com"}})
    } else if query.contains("organization") {
        json!({"organization": {"name": "Acme", "urlKey": "acme"}})
    } else if query.contains("teams(") {
        json!({"teams": page(vec![
            json!({"id": "t-ops", "key": "OPS", "name": "Ops", "archivedAt": null, "issueCount": 1}),
            json!({"id": "t-old", "key": "OLD", "name": "Archive", "archivedAt": "2024-01-01T00:00:00.000Z", "issueCount": 0}),
            json!({"id": "t-eng", "key": "ENG", "name": "Engineering", "archivedAt": null, "issueCount": 6}),
        ])})
    } else if query.contains("states(") {
        fake.hit(&format!("states {id}"));
        if fake.rate_limit_states.swap(false, Ordering::SeqCst) {
            let mut response = graphql_error(
                StatusCode::BAD_REQUEST,
                "RATELIMITED",
                "Rate limit exceeded",
            );
            response
                .headers_mut()
                .insert(header::RETRY_AFTER, "0".parse().unwrap());
            return response;
        }
        let states = match id {
            "t-eng" => vec![
                json!({"id": "s-done", "name": "Done", "type": "completed", "position": 4, "color": "#5e6ad2"}),
                json!({"id": "s-backlog", "name": "Backlog", "type": "backlog", "position": 0, "color": "#bec2c8"}),
                json!({"id": "s-todo", "name": "Todo", "type": "unstarted", "position": 1, "color": "#e2e2e2"}),
                json!({"id": "s-prog", "name": "In Progress", "type": "started", "position": 2, "color": "#f2c94c"}),
                json!({"id": "s-review", "name": "In Review", "type": "started", "position": 3, "color": "#0f783c"}),
                json!({"id": "s-cancel", "name": "Canceled", "type": "canceled", "position": 5, "color": "#95a2b3"}),
                json!({"id": "s-dup", "name": "Duplicate", "type": "canceled", "position": 6, "color": "#95a2b3"}),
            ],
            _ => vec![
                json!({"id": "o-todo", "name": "Todo", "type": "unstarted", "position": 0, "color": "nope"}),
            ],
        };
        json!({"team": {"states": page(states)}})
    } else if query.contains("issues(") {
        fake.hit(&format!("issues {id}"));
        if fake.rate_limit_issues.load(Ordering::SeqCst) {
            let mut response = graphql_error(
                StatusCode::BAD_REQUEST,
                "RATELIMITED",
                "Rate limit exceeded",
            );
            response
                .headers_mut()
                .insert(header::RETRY_AFTER, "3600".parse().unwrap());
            return response;
        }
        let after = body["variables"]["after"].as_str();
        fake.hit(&format!("issues {id} after {}", after.unwrap_or("start")));
        if id == "t-eng" && fake.paged.load(Ordering::SeqCst) {
            // i3 (a sub-issue of i1) and i2, i4 first; i1, i5 and the trashed i6 after
            let mut issues = eng_issues(&fake.base);
            let second: Vec<Value> = [0, 4, 5].map(|index| issues[index].clone()).to_vec();
            let first = vec![issues.remove(2), issues.remove(1), issues.remove(1)];
            let issues = match after {
                None => page_before(first, "p2"),
                Some(_) if fake.rate_limit_page_two.load(Ordering::SeqCst) => {
                    let mut response = graphql_error(
                        StatusCode::BAD_REQUEST,
                        "RATELIMITED",
                        "Rate limit exceeded",
                    );
                    response
                        .headers_mut()
                        .insert(header::RETRY_AFTER, "3600".parse().unwrap());
                    return response;
                }
                Some(_) => page(second),
            };
            return Json(json!({"data": {"team": {"issues": issues}}})).into_response();
        }
        let issues = match id {
            "t-eng" => eng_issues(&fake.base),
            _ => ops_issues(),
        };
        json!({"team": {"issues": page(issues)}})
    } else {
        return graphql_error(StatusCode::OK, "INVALID_INPUT", "Entity not found");
    };
    Json(json!({"data": data})).into_response()
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
    service: LinearImportService,
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
        let service = LinearImportService::new(
            (*database).clone(),
            AttachmentRepository::new((*database).clone(), uploads),
            LinearImportSettings {
                client: LinearClientConfig {
                    endpoint: format!("{}/graphql", fake.base),
                    requests_per_second: 1000.0,
                    max_attempts: 2,
                    initial_backoff: Duration::from_millis(5),
                    max_backoff: Duration::from_millis(20),
                    loopback_file_port: Some(port),
                    ..LinearClientConfig::default()
                },
                app_key: Some([7u8; 32]),
                max_file_bytes: 1024 * 1024,
            },
        );
        let app = linear_import_router(LinearImportState::new(
            Arc::clone(&identity),
            service.clone(),
            CookieMode::secure(),
        ));
        let member_id = Id::new_v7();
        let member_cookie = add_member(
            &database,
            &identity,
            &setup.workspace_id.to_string(),
            member_id,
        )
        .await;
        Self {
            _root: root,
            database,
            service,
            fake,
            app,
            workspace_id: setup.workspace_id.to_string(),
            cookie: format!("__Host-orbit_session={}", setup.session.token),
            member_id,
            member_cookie,
        }
    }

    fn imports_uri(&self) -> String {
        format!("/api/v1/workspaces/{}/imports/linear", self.workspace_id)
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

    /// Creates an import, runs its scan job and starts it with these teams.
    async fn started(&self, team_ids: &[&str]) -> String {
        let (status, import) = self
            .call("POST", &self.imports_uri(), Some(json!({"token": KEY})))
            .await;
        assert_eq!(status, StatusCode::CREATED, "{import}");
        assert_eq!(import["status"], "scanning");
        assert_eq!(import["linear_user_name"], "Ada Linear");
        let id = import["id"].as_str().unwrap().to_owned();
        self.service
            .run_scan(id.parse().unwrap(), &JobRun::default())
            .await
            .unwrap();
        let (status, started) = self
            .call(
                "POST",
                &format!("{}/{id}/start", self.imports_uri()),
                Some(json!({"team_ids": team_ids})),
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

    async fn token(&self, id: &str) -> Option<Vec<u8>> {
        sqlx::query_scalar("SELECT token_ciphertext FROM linear_imports WHERE id = $1")
            .bind(id)
            .fetch_one(self.database.pool())
            .await
            .unwrap()
    }

    async fn orbit_id(&self, id: &str, kind: &str, linear_id: &str) -> String {
        sqlx::query_scalar(
            "SELECT orbit_id FROM linear_import_items WHERE import_id = $1 AND kind = $2 AND linear_id = $3",
        )
        .bind(id)
        .bind(kind)
        .bind(linear_id)
        .fetch_one(self.database.pool())
        .await
        .unwrap()
    }

    /// The imported task as a JSON object of the fields the import sets.
    async fn task(&self, task_id: &str) -> Value {
        let row = sqlx::query(
            "SELECT tasks.title, task_statuses.name, task_statuses.category, tasks.priority, \
             tasks.parent_task_id, projects.project_key, tasks.source_url, tasks.due_at, \
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

    /// `(source title, target title, type)`; related pairs in title order.
    async fn relations(&self, id: &str) -> Vec<(String, String, String)> {
        let rows: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT source.title, target.title, task_relations.type FROM task_relations \
             JOIN tasks AS source ON source.id = task_relations.task_id \
             JOIN tasks AS target ON target.id = task_relations.related_task_id \
             WHERE source.project_id IN (SELECT orbit_id FROM linear_import_items WHERE import_id = $1)",
        )
        .bind(id)
        .fetch_all(self.database.pool())
        .await
        .unwrap();
        let mut rows: Vec<(String, String, String)> = rows
            .into_iter()
            .map(|(a, b, kind)| {
                if kind == "related" && b < a {
                    (b, a, kind)
                } else {
                    (a, b, kind)
                }
            })
            .collect();
        rows.sort();
        rows
    }
}

/// Adds a member (`member@example.com`) with a session; returns its cookie.
async fn add_member(
    database: &TestDatabase,
    identity: &IdentityRepository,
    workspace_id: &str,
    member_id: Id,
) -> String {
    let now = TimestampMillis::now().as_millis();
    sqlx::query(
        "INSERT INTO users (id, email, normalized_email, display_name, password_hash, created_at, updated_at) \
         VALUES ($1, 'member@example.com', 'member@example.com', 'Member', $2, $3, $4)",
    )
    .bind(member_id.to_string())
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
                email: "member@example.com".to_owned(),
                display_name: "Member".to_owned(),
            },
            TimestampMillis::now(),
        )
        .await
        .unwrap();
    format!("__Host-orbit_session={}", session.token)
}

#[tokio::test]
async fn imports_teams_with_states_issues_comments_files_and_relations() {
    let h = Harness::new().await;
    h.fake.rate_limit_states.store(true, Ordering::SeqCst);
    let id = h.started(&["t-eng", "t-ops"]).await;
    h.run(&id, &JobRun::default()).await;

    let import = h.view(&id).await;
    assert_eq!(import["status"], "completed", "{import}");
    assert_eq!(
        import["progress"],
        json!({"total": 6, "done": 6, "failed": 0, "expected": 7, "read": 6, "created": 6, "stage": null})
    );
    assert_eq!(import["waiting_until"], Value::Null);
    assert_eq!(
        import["report"],
        json!({"comments_imported": 2, "files_imported": 2, "missing_files": 1, "links_added": 1,
               "relations_linked": 4, "relations_skipped": 2, "unmatched_assignees": 1,
               "detached_sub_issues": 1, "failures": []})
    );
    assert!(h.token(&id).await.is_none(), "key wiped on completion");
    assert_eq!(
        h.fake.hits("states t-eng"),
        2,
        "the rate-limited query was retried"
    );

    // Projects in selection order, keyed by the team keys.
    let eng = h.orbit_id(&id, "team", "t-eng").await;
    let ops = h.orbit_id(&id, "team", "t-ops").await;
    assert_eq!(import["project_ids"], json!([eng, ops]));
    let projects: Vec<(String, String)> = sqlx::query_as(
        "SELECT name, project_key FROM projects WHERE id IN ($1, $2) ORDER BY created_at, id",
    )
    .bind(&eng)
    .bind(&ops)
    .fetch_all(h.database.pool())
    .await
    .unwrap();
    assert_eq!(
        projects,
        [
            ("Engineering".to_owned(), "ENG".to_owned()),
            ("Ops".to_owned(), "OPS".to_owned())
        ]
    );
    let statuses: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT name, category, color FROM task_statuses WHERE project_id = $1 ORDER BY position, id",
    )
    .bind(&eng)
    .fetch_all(h.database.pool())
    .await
    .unwrap();
    let statuses: Vec<(&str, &str)> = statuses
        .iter()
        .map(|(name, category, _)| (name.as_str(), category.as_str()))
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
            ("In Review", "started"),
        ],
        "same names (and Canceled) reuse the defaults; In Review is new"
    );
    let review_color: String = sqlx::query_scalar(
        "SELECT color FROM task_statuses WHERE project_id = $1 AND name = 'In Review'",
    )
    .bind(&eng)
    .fetch_one(h.database.pool())
    .await
    .unwrap();
    assert_eq!(review_color, "#0f783c");

    let imported = h
        .scalar(
            "SELECT COUNT(*) FROM tasks WHERE project_id IN (SELECT orbit_id FROM linear_import_items WHERE import_id = $1)",
            &id,
        )
        .await;
    assert_eq!(imported, 6, "the trashed issue is left out");

    let design = h.orbit_id(&id, "issue", "i1").await;
    let ship = h.orbit_id(&id, "issue", "i2").await;
    let sub = h.orbit_id(&id, "issue", "i3").await;
    let ops_child = h.orbit_id(&id, "issue", "o1").await;
    let files = h.files(&design).await;
    let names: Vec<(&str, i64)> = files
        .iter()
        .map(|(_, name, size)| (name.as_str(), *size))
        .collect();
    assert_eq!(
        names,
        [
            ("diagram", NOTES.len() as i64),
            ("shot.png", PNG.len() as i64)
        ]
    );
    let file_url = |name: &str| {
        let id = &files.iter().find(|file| file.1 == name).unwrap().0;
        format!(
            "/api/v1/workspaces/{}/tasks/{design}/attachments/{id}/download",
            h.workspace_id
        )
    };
    assert_eq!(
        h.task(&design).await,
        json!({
            "title": "Design homepage", "status": "In Review", "category": "started",
            "priority": "high", "parent": null, "project_key": "ENG",
            "source_url": "https://linear.app/acme/issue/ENG-1",
            "due_at": noon(10), "completed_at": null,
            "description": format!(
                "Make it **pop**\n\n![shot.png]({})\n\n**Linear**\n\n- Issue: ENG-1\n- Project: Launch\n- Cycle: 3 (Sprint 3)\n- Estimate: 3\n\n**Links**\n\n- [PR \\#12](<https://github.com/acme/web/pull/12>)",
                file_url("shot.png")
            ),
            "assignees": [h.member_id.to_string()], "labels": ["Type/Bug", "design"],
        })
    );
    let blocks: String = sqlx::query_scalar("SELECT description_json FROM tasks WHERE id = $1")
        .bind(&design)
        .fetch_one(h.database.pool())
        .await
        .unwrap();
    let blocks: Value = serde_json::from_str(&blocks).unwrap();
    let image = blocks
        .as_array()
        .unwrap()
        .iter()
        .find(|block| block["type"] == "image")
        .expect("the upload became an image block");
    assert_eq!(
        image["props"]["url"],
        file_url("shot.png"),
        "kept by the task's file policy"
    );
    assert_eq!(
        h.task(&ship).await,
        json!({
            "title": "Ship it", "status": "Done", "category": "completed", "priority": "urgent",
            "parent": null, "project_key": "ENG", "source_url": "https://linear.app/acme/issue/ENG-2",
            "due_at": null, "completed_at": 100_000,
            "description": "**Linear**\n\n- Issue: ENG-2\n- Assignee: Ghost",
            "assignees": [], "labels": [],
        })
    );
    let sub_task = h.task(&sub).await;
    assert_eq!(
        (&sub_task["parent"], &sub_task["status"]),
        (&json!(design), &json!("Duplicate")),
        "a sub-issue keeps its parent; a duplicate moves into Duplicate"
    );
    let child = h.task(&ops_child).await;
    assert_eq!(
        (
            &child["parent"],
            &child["project_key"],
            &child["status"],
            &child["description"]
        ),
        (
            &Value::Null,
            &json!("OPS"),
            &json!("Todo"),
            &json!("**Linear**\n\n- Issue: OPS-1\n- Parent: ENG-1")
        ),
        "a parent in another team is noted, not linked"
    );
    let other = h.task(&h.orbit_id(&id, "issue", "i5").await).await;
    assert_eq!(other["status"], "Cancelled", "Linear's Duplicate state");

    assert_eq!(
        h.comments(&design).await,
        [
            format!(
                "**Grace** · 2024-05-01 09:30 UTC\n\nLooks great, see [old]({}/uploads/gone)",
                h.fake.base
            ),
            format!(
                "**GitHub** · 2024-05-01 10:00 UTC · reply\n\nThanks ![d]({})",
                file_url("diagram")
            ),
        ]
    );
    assert_eq!(
        h.relations(&id).await,
        [
            (
                "Design homepage".to_owned(),
                "Ship it".to_owned(),
                "blocks".to_owned()
            ),
            (
                "Design homepage".to_owned(),
                "Sub task".to_owned(),
                "related".to_owned()
            ),
            (
                "Original".to_owned(),
                "Ops child".to_owned(),
                "blocks".to_owned()
            ),
            (
                "Sub task".to_owned(),
                "Original".to_owned(),
                "duplicate".to_owned()
            ),
        ]
    );

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
            &format!("linear-import-{id}"),
        )
        .await;
    assert_eq!(audited, 6);
}

#[tokio::test]
async fn resumes_after_a_restart_without_duplicates() {
    let h = Harness::new().await;
    let id = h.started(&["t-eng", "t-ops"]).await;
    // Shutdown while the first issue's files are downloaded.
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
    assert!(import["progress"]["done"].as_u64().unwrap() < 6);

    // A crash right after Orbit rows were made, before their items said so.
    sqlx::query(
        "UPDATE linear_import_items SET state = 'pending' WHERE import_id = $1 AND linear_id IN ('t-ops', 'i2')",
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
        count("SELECT COUNT(*) FROM projects WHERE id IN (SELECT orbit_id FROM linear_import_items WHERE import_id = $1)").await,
        2
    );
    assert_eq!(
        count("SELECT COUNT(*) FROM tasks WHERE project_id IN (SELECT orbit_id FROM linear_import_items WHERE import_id = $1)").await,
        6
    );
    assert_eq!(
        count("SELECT COUNT(*) FROM task_statuses WHERE project_id IN (SELECT orbit_id FROM linear_import_items WHERE import_id = $1 AND kind = 'team')").await,
        7 + 6
    );
    let design = h.orbit_id(&id, "issue", "i1").await;
    assert_eq!(h.comments(&design).await.len(), 2);
    assert_eq!(h.files(&design).await.len(), 2);
    assert_eq!(h.relations(&id).await.len(), 4);
    assert_eq!(
        h.scalar(
            "SELECT COUNT(*) FROM labels WHERE project_id = $1",
            &h.orbit_id(&id, "team", "t-eng").await
        )
        .await,
        2
    );
    assert_eq!(
        h.fake.hits("issues t-ops"),
        2,
        "the reset team was listed again"
    );
    assert_eq!(h.fake.hits("issues t-eng"), 1);
    assert_eq!(import["report"]["files_imported"], 2);
}

#[tokio::test]
async fn a_revoked_key_fails_the_import_and_keeps_what_was_imported() {
    let h = Harness::new().await;
    let id = h.started(&["t-eng"]).await;
    h.fake.revoked.store(true, Ordering::SeqCst);
    h.run(&id, &JobRun::default()).await;
    let import = h.view(&id).await;
    assert_eq!(import["status"], "failed");
    assert_eq!(
        import["error"],
        "The Linear API key was revoked or is no longer valid."
    );
    assert_eq!(
        import["project_ids"].as_array().unwrap().len(),
        1,
        "the project stays"
    );
    assert!(h.token(&id).await.is_none());
}

#[tokio::test]
async fn validates_keys_and_selections_and_keeps_imports_private() {
    let h = Harness::new().await;
    let (status, problem) = h
        .call(
            "POST",
            &h.imports_uri(),
            Some(json!({"token": "lin_api_wrong"})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(problem["code"], "linear_token_invalid");

    let (status, import) = h
        .call("POST", &h.imports_uri(), Some(json!({"token": KEY})))
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let id = import["id"].as_str().unwrap().to_owned();
    let (status, problem) = h
        .call(
            "POST",
            &format!("{}/{id}/start", h.imports_uri()),
            Some(json!({"team_ids": ["t-eng"]})),
        )
        .await;
    assert_eq!(
        (status, problem["code"].as_str()),
        (StatusCode::CONFLICT, Some("linear_import_state_conflict"))
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
        json!({"organization": "Acme", "teams": [
            {"id": "t-eng", "key": "ENG", "name": "Engineering", "issue_count": 6, "archived": false},
            {"id": "t-ops", "key": "OPS", "name": "Ops", "issue_count": 1, "archived": false},
            {"id": "t-old", "key": "OLD", "name": "Archive", "issue_count": 0, "archived": true}
        ], "truncated": false})
    );
    let (_, light) = h
        .call("GET", &format!("{}/{id}", h.imports_uri()), None)
        .await;
    assert_eq!(light["tree"], Value::Null, "polling leaves the tree out");

    // Another member (the import's creator is the owner) sees nothing.
    let (status, _) = h
        .call_as(
            &h.member_cookie,
            "GET",
            &format!("{}/{id}", h.imports_uri()),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, list) = h
        .call_as(&h.member_cookie, "GET", &h.imports_uri(), None)
        .await;
    assert_eq!((status, list["items"].clone()), (StatusCode::OK, json!([])));
    let (status, _) = h
        .call_as(
            &h.member_cookie,
            "POST",
            &format!("{}/{id}/cancel", h.imports_uri()),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (_, list) = h.call("GET", &h.imports_uri(), None).await;
    assert_eq!(list["items"].as_array().unwrap().len(), 1);

    for body in [json!({"team_ids": []}), json!({"team_ids": ["t-missing"]})] {
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
    assert!(h.token(&id).await.is_some());
    let (status, cancelled) = h
        .call("POST", &format!("{}/{id}/cancel", h.imports_uri()), None)
        .await;
    assert_eq!(
        (status, cancelled["status"].as_str()),
        (StatusCode::OK, Some("cancelled"))
    );
    assert!(h.token(&id).await.is_none(), "cancel wipes the key");
    let (status, _) = h
        .call("POST", &format!("{}/{id}/cancel", h.imports_uri()), None)
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn a_cancelled_import_stops_and_keeps_what_it_made() {
    let h = Harness::new().await;
    let id = h.started(&["t-eng", "t-ops"]).await;
    let (status, _) = h
        .call("POST", &format!("{}/{id}/cancel", h.imports_uri()), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    h.run(&id, &JobRun::default()).await;
    let import = h.view(&id).await;
    assert_eq!(import["status"], "cancelled");
    assert_eq!(import["project_ids"], json!([]));
    assert_eq!(
        h.fake.hits("issues t-eng"),
        0,
        "the job did nothing after the cancel"
    );
}

#[tokio::test]
async fn a_long_rate_limit_queues_the_next_run_after_it_without_failing() {
    let h = Harness::new().await;
    h.fake.rate_limit_issues.store(true, Ordering::SeqCst);
    let id = h.started(&["t-eng"]).await;
    let before = TimestampMillis::now().as_millis();
    h.service
        .run_import(id.parse().unwrap(), &JobRun::default())
        .await
        .expect("the run ends without a job error");

    let import = h.view(&id).await;
    assert_eq!(import["status"], "importing", "{import}");
    assert_eq!(import["error"], Value::Null);
    assert_eq!(import["progress"]["stage"], "reading");
    assert_eq!(import["progress"]["expected"], 6);
    assert!(import["waiting_until"].is_string(), "{import}");
    let waiting_until = h
        .scalar(
            "SELECT waiting_until FROM linear_imports WHERE id = $1",
            &id,
        )
        .await;
    assert!(
        (before + 3_590_000..=before + 3_700_000).contains(&waiting_until),
        "waits for Linear's hour: {import}"
    );
    // the job `start` queued is never claimed here (the test calls the handler directly)
    let later: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM jobs WHERE kind = 'linear.import' AND state = 'queued' AND available_at >= $1",
    )
    .bind(before + 3_590_000)
    .fetch_one(h.database.pool())
    .await
    .unwrap();
    assert_eq!(later, 1, "the next run is queued after the wait");

    h.fake.rate_limit_issues.store(false, Ordering::SeqCst);
    h.run(&id, &JobRun::default()).await;
    let import = h.view(&id).await;
    assert_eq!(import["status"], "completed", "{import}");
    assert_eq!(import["waiting_until"], Value::Null);
    assert_eq!(import["progress"]["stage"], Value::Null);
    assert_eq!(
        import["progress"]["done"], import["progress"]["total"],
        "{import}"
    );
}

#[tokio::test]
async fn reads_issues_page_by_page_and_goes_on_from_the_stored_page_after_a_long_rate_limit() {
    let h = Harness::new().await;
    h.fake.paged.store(true, Ordering::SeqCst);
    h.fake.rate_limit_page_two.store(true, Ordering::SeqCst);
    let id = h.started(&["t-eng"]).await;
    h.run(&id, &JobRun::default()).await;

    let import = h.view(&id).await;
    assert_eq!(import["status"], "importing", "{import}");
    assert_eq!(
        import["progress"]["total"], 3,
        "the first page is stored: {import}"
    );
    assert_eq!(
        import["progress"]["done"], 2,
        "its tasks are imported at once, and ENG-3 waits for its parent: {import}"
    );
    assert!(import["waiting_until"].is_string(), "{import}");

    h.fake.rate_limit_page_two.store(false, Ordering::SeqCst);
    h.run(&id, &JobRun::default()).await;
    let import = h.view(&id).await;
    assert_eq!(import["status"], "completed", "{import}");
    assert_eq!(import["progress"]["total"], 5, "{import}");
    assert_eq!(import["progress"]["done"], 5, "{import}");
    assert_eq!(
        h.fake.hits("issues t-eng after start"),
        1,
        "the second run reads on from the stored cursor"
    );
    let parent = h.orbit_id(&id, "issue", "i1").await;
    let sub = h.task(&h.orbit_id(&id, "issue", "i3").await).await;
    assert_eq!(sub["parent"], parent, "ENG-3 is under ENG-1: {sub}");
}

#[tokio::test]
async fn a_cancelled_import_shows_no_stage_or_wait() {
    let h = Harness::new().await;
    h.fake.rate_limit_issues.store(true, Ordering::SeqCst);
    let id = h.started(&["t-eng"]).await;
    h.run(&id, &JobRun::default()).await;
    let waiting = h.view(&id).await;
    assert_eq!(waiting["progress"]["stage"], "reading", "{waiting}");
    assert!(waiting["waiting_until"].is_string(), "{waiting}");

    let (status, cancelled) = h
        .call("POST", &format!("{}/{id}/cancel", h.imports_uri()), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cancelled["status"], "cancelled");
    assert_eq!(
        cancelled["progress"]["stage"],
        Value::Null,
        "an ended import has no stage"
    );
    assert_eq!(
        cancelled["waiting_until"],
        Value::Null,
        "and waits for nothing"
    );
}

#[tokio::test]
async fn a_long_import_notifies_its_user_once_by_inbox_and_email() {
    let h = Harness::new().await;
    let (mailer, outbox) = Mailer::recording((*h.database).clone(), "https://orbit.example");
    let notices = ImportNotices::new((*h.database).clone(), mailer);
    let id = h.started(&["t-eng"]).await;
    let import_id: Id = id.parse().unwrap();
    h.run(&id, &JobRun::default()).await;
    assert_eq!(h.view(&id).await["status"], "completed");
    let notify = || notices.notify(ImportSource::Linear, import_id, TimestampMillis::now());
    assert!(
        !notify().await.unwrap(),
        "an import that just started notifies nobody"
    );

    sqlx::query("UPDATE linear_imports SET started_at = started_at - 360000 WHERE id = $1")
        .bind(&id)
        .execute(h.database.pool())
        .await
        .unwrap();
    assert!(
        notify().await.unwrap(),
        "a six-minute import notifies its user"
    );
    assert!(!notify().await.unwrap(), "only once");

    let rows: Vec<(String, String, String, String, String)> = sqlx::query_as(
        "SELECT kind, import_source, import_id, import_status, workspace_id FROM notifications \
         WHERE kind = 'import_finished'",
    )
    .fetch_all(h.database.pool())
    .await
    .unwrap();
    assert_eq!(
        rows,
        [(
            "import_finished".to_owned(),
            "linear".to_owned(),
            id.clone(),
            "completed".to_owned(),
            h.workspace_id.clone()
        )]
    );
    // the email is sent in the background
    for _ in 0..100 {
        if !outbox.lock().unwrap().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let sent = outbox.lock().unwrap().clone();
    assert_eq!(sent.len(), 1, "one email");
    assert_eq!(sent[0].to, "owner@example.com");
    assert_eq!(sent[0].subject, "Your Linear import is complete");
    assert!(
        sent[0].body.contains(": 5 issues imported.")
            && sent[0].body.contains(&format!(
                "https://orbit.example/tasks-import/linear/{id}?workspace={}",
                h.workspace_id
            )),
        "{}",
        sent[0].body
    );
}

#[tokio::test]
async fn a_cancelled_import_notifies_nobody() {
    let h = Harness::new().await;
    let (mailer, outbox) = Mailer::recording((*h.database).clone(), "https://orbit.example");
    let notices = ImportNotices::new((*h.database).clone(), mailer);
    let id = h.started(&["t-eng"]).await;
    let (status, _) = h
        .call("POST", &format!("{}/{id}/cancel", h.imports_uri()), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    sqlx::query("UPDATE linear_imports SET started_at = started_at - 360000 WHERE id = $1")
        .bind(&id)
        .execute(h.database.pool())
        .await
        .unwrap();
    assert!(
        !notices
            .notify(
                ImportSource::Linear,
                id.parse().unwrap(),
                TimestampMillis::now()
            )
            .await
            .unwrap()
    );
    assert!(outbox.lock().unwrap().is_empty());
}

#[tokio::test]
async fn mapping_an_imported_person_moves_their_tasks_in_a_job() {
    use orbit_server::import_people::{run_assign_job, set_member};
    let h = Harness::new().await;
    let id = h.started(&["t-eng", "t-ops"]).await;
    h.run(&id, &JobRun::default()).await;
    let workspace: Id = h.workspace_id.parse().unwrap();
    let text = |sql: &'static str, bind: String| {
        let pool = h.database.pool().clone();
        async move {
            sqlx::query_scalar::<_, String>(sql)
                .bind(bind)
                .fetch_one(&pool)
                .await
                .unwrap()
        }
    };
    let owner: Id = text(
        "SELECT id FROM users WHERE email = $1",
        "owner@example.com".into(),
    )
    .await
    .parse()
    .unwrap();
    let person: Id = text(
        "SELECT id FROM import_people WHERE workspace_id = $1",
        h.workspace_id.clone(),
    )
    .await
    .parse()
    .unwrap();
    let task = text(
        "SELECT task_id FROM import_person_tasks WHERE person_id = $1",
        person.to_string(),
    )
    .await;
    let member = h.member_id.to_string();
    let assigned = || async {
        h.scalar(
            &format!(
                "SELECT COUNT(*) FROM task_assignees WHERE user_id = '{member}' AND task_id = $1"
            ),
            &task,
        )
        .await
    };
    let assigning = || async {
        h.scalar(
            "SELECT assigning FROM import_people WHERE id = $1",
            &person.to_string(),
        )
        .await
    };
    let jobs = || async {
        sqlx::query_scalar::<_, String>(
            "SELECT payload_json FROM jobs WHERE kind = 'import_people.assign' ORDER BY created_at, id",
        )
        .fetch_all(h.database.pool())
        .await
        .unwrap()
        .iter()
        .map(|json| serde_json::from_str::<Value>(json).unwrap())
        .collect::<Vec<_>>()
    };

    let mapped = set_member(
        &h.database,
        workspace,
        owner,
        person,
        Some(h.member_id),
        "map-1",
    )
    .await
    .unwrap();
    assert!(
        mapped.assigning,
        "the person shows the work until the job ends"
    );
    assert_eq!(mapped.user_id, Some(h.member_id));
    assert_eq!(assigned().await, 0, "the request itself moves no task");
    run_assign_job(&h.database, &jobs().await[0], false)
        .await
        .unwrap();
    assert_eq!(assigned().await, 1);
    assert_eq!(assigning().await, 0);

    // unmap, then map again before the unmap's job runs: the older job stops and leaves the mark
    set_member(&h.database, workspace, owner, person, None, "unmap")
        .await
        .unwrap();
    set_member(
        &h.database,
        workspace,
        owner,
        person,
        Some(h.member_id),
        "map-2",
    )
    .await
    .unwrap();
    let queued = jobs().await;
    run_assign_job(&h.database, &queued[1], false)
        .await
        .unwrap();
    assert_eq!(assigned().await, 1, "the superseded unmap moved nothing");
    assert_eq!(
        assigning().await,
        1,
        "only the latest pick's job ends the work"
    );
    run_assign_job(&h.database, &queued[2], false)
        .await
        .unwrap();
    assert_eq!(assigned().await, 1);
    assert_eq!(assigning().await, 0);
}
