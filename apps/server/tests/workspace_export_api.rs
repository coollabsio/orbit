//! Full workspace export through the whole app: start, the `workspace.export` job, download and
//! unzip, permissions, a read-only (billing) workspace, and retention.

use std::collections::BTreeSet;
use std::io::{Cursor, Read};
use std::net::SocketAddr;
use std::time::Duration;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode, header};
use orbit_platform::{
    AuthenticatedUser, BillingConfig, Config, Database, EnvironmentMode, Id, PasswordService, Plan,
    Secret, TimestampMillis,
};
use orbit_server::app::App;
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::workspace_export::WorkspaceExportService;
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

const COOKIE: &str = "orbit_session_dev";
const ORIGIN: &str = "http://127.0.0.1";
const PNG: &[u8] = b"\x89\x50\x4e\x47\x0d\x0a\x1a\x0a\x00\x00\x00\x0d\x49\x48\x44\x52\x00\x00\x00\x01\x00\x00\x00\x01\x08\x04\x00\x00\x00\xb5\x1c\x0c\x02\x00\x00\x00\x0b\x49\x44\x41\x54\x78\xda\x63\x64\xf8\x0f\x00\x01\x05\x01\x01\x27\x18\xe3\x66\x00\x00\x00\x00\x49\x45\x4e\x44\xae\x42\x60\x82";
const DAY_MILLIS: i64 = 24 * 60 * 60 * 1000;

struct Harness {
    root: TempDir,
    /// `None` once [`Harness::serve`] runs it.
    app: Option<App>,
    exports: WorkspaceExportService,
    router: Router,
    database: Database,
    identity: IdentityRepository,
    owner: String,
    workspace_id: String,
}

impl Harness {
    /// `billing`: Orbit Cloud with the read-only guard (Stripe is never called here).
    async fn new(billing: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut config = Config {
            environment: EnvironmentMode::Development,
            app_key: Some(orbit_platform::AppKey::new([1; 32])),
            ..Config::default()
        };
        config.data.database = root.path().join("data/orbit.sqlite");
        config.data.attachments = root.path().join("data/attachments");
        config.data.backups = root.path().join("backups");
        config.http.public_origin = ORIGIN.to_owned();
        if billing {
            config.billing = Some(BillingConfig::new(
                Secret::new("sk_test_export"),
                Secret::new("whsec_export"),
                vec![Plan {
                    key: "team".to_owned(),
                    price_id: "price_team".to_owned(),
                }],
            ));
        }
        let app = App::build(config).await.unwrap();
        let database = app.database().clone();
        let identity = IdentityRepository::new(database.clone());
        let now = TimestampMillis::now();
        identity
            .store_setup_token(
                "export-setup",
                TimestampMillis::from_millis(now.as_millis() + 60_000),
            )
            .await
            .unwrap();
        let setup = identity
            .complete_setup(
                SetupRequest {
                    token: "export-setup".to_owned(),
                    email: "owner@example.com".to_owned(),
                    display_name: "Owner".to_owned(),
                    password_hash: PasswordService::default()
                        .hash("correct horse battery")
                        .unwrap(),
                    workspace_name: "Acme".to_owned(),
                    project_name: "General".to_owned(),
                },
                now,
            )
            .await
            .unwrap();
        Self {
            root,
            router: app.router(),
            exports: app.workspace_exports().clone(),
            app: Some(app),
            database,
            identity,
            owner: setup.session.token,
            workspace_id: setup.workspace_id.to_string(),
        }
    }

    fn workspace(&self, rest: &str) -> String {
        format!("/api/v1/workspaces/{}{rest}", self.workspace_id)
    }

    fn request(&self, session: &str, method: &str, path: &str) -> axum::http::request::Builder {
        Request::builder()
            .method(method)
            .uri(path)
            .header(header::ORIGIN, ORIGIN)
            .header(header::COOKIE, format!("{COOKIE}={session}"))
            .extension(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 40_000))))
    }

    async fn send(&self, request: Request<Body>) -> (StatusCode, Vec<u8>, header::HeaderMap) {
        let response = self.router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (status, bytes.to_vec(), headers)
    }

    async fn call(
        &self,
        session: &str,
        method: &str,
        path: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let builder = self.request(session, method, path);
        let request = match body {
            Some(body) => builder
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string())),
            None => builder.body(Body::empty()),
        }
        .unwrap();
        let (status, bytes, _) = self.send(request).await;
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn ok(&self, session: &str, method: &str, path: &str, body: Option<Value>) -> Value {
        let (status, value) = self.call(session, method, path, body).await;
        assert!(status.is_success(), "{method} {path}: {status} {value}");
        value
    }

    async fn scalar(&self, sql: &'static str) -> String {
        sqlx::query_scalar(sql)
            .bind(&self.workspace_id)
            .fetch_one(self.database.pool())
            .await
            .unwrap()
    }

    /// A signed-in user with `role` in the workspace (`None`: not a member); returns the session.
    async fn add_user(&self, email: &str, name: &str, role: Option<&str>) -> String {
        let id = Id::new_v7();
        let now = TimestampMillis::now();
        sqlx::query(
            "INSERT INTO users (id, email, normalized_email, display_name, password_hash, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, 'unused', $5, $6)",
        )
        .bind(id.to_string())
        .bind(email)
        .bind(email)
        .bind(name)
        .bind(now.as_millis())
        .bind(now.as_millis())
        .execute(self.database.pool())
        .await
        .unwrap();
        if let Some(role) = role {
            sqlx::query(
                "INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, updated_at) \
                 VALUES ($1, $2, $3, $4, 0, $5, $6)",
            )
            .bind(Id::new_v7().to_string())
            .bind(&self.workspace_id)
            .bind(id.to_string())
            .bind(role)
            .bind(now.as_millis())
            .bind(now.as_millis())
            .execute(self.database.pool())
            .await
            .unwrap();
        }
        self.identity
            .create_session(
                &AuthenticatedUser {
                    id,
                    email: email.to_owned(),
                    display_name: name.to_owned(),
                },
                now,
            )
            .await
            .unwrap()
            .token
    }

    async fn create_task(&self, title: &str) -> String {
        let project = self
            .scalar("SELECT id FROM projects WHERE workspace_id = $1")
            .await;
        let status: String = sqlx::query_scalar(
            "SELECT id FROM task_statuses WHERE project_id = $1 ORDER BY position LIMIT 1",
        )
        .bind(&project)
        .fetch_one(self.database.pool())
        .await
        .unwrap();
        let task = self
            .ok(
                &self.owner,
                "POST",
                &self.workspace("/tasks"),
                Some(json!({"project_id": project, "status_id": status, "title": title})),
            )
            .await;
        task["task"]["id"]
            .as_str()
            .or_else(|| task["id"].as_str())
            .unwrap()
            .to_owned()
    }

    async fn attach(&self, task_id: &str, name: &str, bytes: &[u8]) {
        let boundary = "export-test-boundary";
        let mut body = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{name}\"\r\n\r\n"
        )
        .into_bytes();
        body.extend_from_slice(bytes);
        body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
        let request = self
            .request(
                &self.owner,
                "POST",
                &self.workspace(&format!("/tasks/{task_id}/attachments")),
            )
            .header(
                header::CONTENT_TYPE,
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(Body::from(body))
            .unwrap();
        let (status, bytes, _) = self.send(request).await;
        assert!(
            status.is_success(),
            "{status} {}",
            String::from_utf8_lossy(&bytes)
        );
    }

    async fn create_page(&self, session: &str, body: Value) -> String {
        let page = self
            .ok(session, "POST", &self.workspace("/pages"), Some(body))
            .await;
        page["id"].as_str().unwrap().to_owned()
    }

    async fn create_channel(&self, session: &str, name: &str, kind: &str) -> String {
        let written = self
            .ok(
                session,
                "POST",
                &self.workspace("/chat/conversations"),
                Some(json!({"name": name, "kind": kind, "member_ids": []})),
            )
            .await;
        written["result"]["id"].as_str().unwrap().to_owned()
    }

    async fn post_message(&self, conversation: &str, body: Value) -> String {
        let written = self
            .ok(
                &self.owner,
                "POST",
                &self.workspace(&format!("/chat/conversations/{conversation}/messages")),
                Some(body),
            )
            .await;
        written["result"]["id"].as_str().unwrap().to_owned()
    }

    async fn start_export(&self) -> String {
        let (status, export) = self
            .call(&self.owner, "POST", &self.workspace("/exports"), None)
            .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{export}");
        assert_eq!(export["status"], "queued");
        export["id"].as_str().unwrap().to_owned()
    }

    /// Runs the export's job as the worker does.
    async fn run_export(&self, id: &str) {
        self.exports
            .run(id.parse().unwrap(), &CancellationToken::new())
            .await
            .unwrap();
    }

    /// Serves the app (its background worker runs jobs); the future stops it.
    async fn serve(&mut self) -> impl std::future::Future<Output = ()> + use<> {
        let app = self.app.take().unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let shutdown = CancellationToken::new();
        let token = shutdown.clone();
        let task = tokio::spawn(async move { app.serve(listener, token).await.unwrap() });
        async move {
            shutdown.cancel();
            task.await.unwrap();
        }
    }

    async fn download(&self, session: &str, id: &str) -> (StatusCode, Vec<u8>, header::HeaderMap) {
        let request = self
            .request(
                session,
                "GET",
                &self.workspace(&format!("/exports/{id}/download")),
            )
            .body(Body::empty())
            .unwrap();
        self.send(request).await
    }
}

struct Archive(zip::ZipArchive<Cursor<Vec<u8>>>);

impl Archive {
    fn new(bytes: Vec<u8>) -> Self {
        Self(zip::ZipArchive::new(Cursor::new(bytes)).unwrap())
    }

    fn names(&self) -> BTreeSet<String> {
        self.0.file_names().map(str::to_owned).collect()
    }

    fn bytes(&mut self, name: &str) -> Vec<u8> {
        let mut file = self
            .0
            .by_name(name)
            .unwrap_or_else(|_| panic!("{name} missing"));
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).unwrap();
        bytes
    }

    fn text(&mut self, name: &str) -> String {
        String::from_utf8(self.bytes(name)).unwrap()
    }

    fn json(&mut self, name: &str) -> Value {
        serde_json::from_slice(&self.bytes(name)).unwrap()
    }
}

#[tokio::test]
async fn an_owner_exports_tasks_pages_chat_and_files_and_downloads_one_zip() {
    let harness = Harness::new(false).await;
    let member = harness
        .add_user("mia@example.com", "Mia Member", Some("member"))
        .await;
    let stranger = harness.add_user("sam@example.com", "Sam", None).await;

    let task_id = harness.create_task("Ship the export").await;
    harness
        .ok(
            &harness.owner,
            "POST",
            &harness.workspace(&format!("/tasks/{task_id}/comments")),
            Some(json!({"body": "Looks **good**"})),
        )
        .await;
    harness.attach(&task_id, "pixel.png", PNG).await;
    let task_key = harness
        .scalar(
            "SELECT projects.project_key || '-' || CAST(tasks.number AS TEXT) FROM tasks \
             JOIN projects ON projects.id = tasks.project_id WHERE tasks.workspace_id = $1",
        )
        .await;

    let handbook = harness
        .create_page(&harness.owner, json!({"title": "Handbook"}))
        .await;
    harness
        .create_page(
            &harness.owner,
            json!({"title": "Onboarding", "parent_id": handbook}),
        )
        .await;
    harness
        .create_page(
            &harness.owner,
            json!({"title": "My notes", "private": true}),
        )
        .await;
    harness
        .create_page(&member, json!({"title": "Mia's secret", "private": true}))
        .await;
    let teamspace = harness
        .scalar("SELECT name FROM teamspaces WHERE workspace_id = $1")
        .await;

    let launch = harness
        .create_channel(&harness.owner, "launch", "public")
        .await;
    let root = harness
        .post_message(&launch, json!({"body": "Hello launch", "nonce": "n1"}))
        .await;
    harness
        .post_message(
            &launch,
            json!({"body": "First reply", "thread_root_id": root, "nonce": "n2"}),
        )
        .await;
    harness.create_channel(&member, "mia-only", "private").await;

    let (status, problem) = harness
        .call(&member, "POST", &harness.workspace("/exports"), None)
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{problem}");
    assert_eq!(problem["code"], "workspace_action_forbidden");
    let (status, problem) = harness
        .call(&stranger, "POST", &harness.workspace("/exports"), None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{problem}");

    let export_id = harness.start_export().await;
    let (status, problem) = harness
        .call(&harness.owner, "POST", &harness.workspace("/exports"), None)
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{problem}");
    assert_eq!(problem["code"], "export_in_progress");
    let (status, _, _) = harness.download(&harness.owner, &export_id).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "not ready yet");

    harness.run_export(&export_id).await;
    let list = harness
        .ok(&harness.owner, "GET", &harness.workspace("/exports"), None)
        .await;
    let items = list["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["id"], export_id.as_str());
    assert_eq!(items[0]["status"], "ready");
    assert_eq!(items[0]["requested_by_name"], "Owner");
    let created = items[0]["created_at"].as_str().unwrap().to_owned();
    let expires = items[0]["expires_at"].as_str().unwrap();
    let completed = items[0]["completed_at"].as_str().unwrap();
    assert!(created.as_str() <= completed && completed < expires);

    let (status, _) = harness
        .call(&member, "GET", &harness.workspace("/exports"), None)
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _, _) = harness.download(&member, &export_id).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _, _) = harness.download(&stranger, &export_id).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, bytes, headers) = harness.download(&harness.owner, &export_id).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::CONTENT_TYPE], "application/zip");
    assert_eq!(headers[header::CACHE_CONTROL], "no-store");
    assert_eq!(
        headers[header::CONTENT_DISPOSITION],
        format!(
            "attachment; filename=\"Acme export {}.zip\"; filename*=UTF-8''Acme%20export%20{}.zip",
            &created[..10],
            &created[..10]
        )
        .as_str()
    );
    assert_eq!(items[0]["size_bytes"].as_u64().unwrap(), bytes.len() as u64);

    let mut archive = Archive::new(bytes);
    let names = archive.names();
    let task_file = format!("tasks/{task_key}.json");
    let attachment_path = {
        let task = archive.json(&task_file);
        assert_eq!(task["title"], "Ship the export");
        assert_eq!(task["key"], task_key.as_str());
        assert_eq!(task["deleted"], false);
        assert_eq!(task["creator"]["name"], "Owner");
        assert_eq!(task["comments"].as_array().unwrap().len(), 1);
        assert_eq!(task["comments"][0]["body"], "Looks **good**");
        assert_eq!(task["comments"][0]["author"]["name"], "Owner");
        assert_eq!(task["attachments"][0]["name"], "pixel.png");
        assert_eq!(task["attachments"][0]["media_type"], "image/png");
        assert_eq!(task["attachments"][0]["size_bytes"], PNG.len());
        let actions: Vec<&str> = task["activity"]
            .as_array()
            .unwrap()
            .iter()
            .map(|event| event["action"].as_str().unwrap())
            .collect();
        assert!(actions.contains(&"task.created"), "{actions:?}");
        task["attachments"][0]["path"].as_str().unwrap().to_owned()
    };
    assert!(attachment_path.starts_with("files/") && attachment_path.ends_with("/pixel.png"));
    assert_eq!(archive.bytes(&attachment_path), PNG);

    let workspace = archive.json("workspace.json");
    assert_eq!(workspace["name"], "Acme");
    assert_eq!(workspace["exported_by"]["name"], "Owner");
    let members: Vec<(String, String, String)> = workspace["members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|member| {
            (
                member["name"].as_str().unwrap().to_owned(),
                member["email"].as_str().unwrap().to_owned(),
                member["role"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    assert_eq!(
        members,
        vec![
            (
                "Owner".to_owned(),
                "owner@example.com".to_owned(),
                "owner".to_owned()
            ),
            (
                "Mia Member".to_owned(),
                "mia@example.com".to_owned(),
                "member".to_owned()
            ),
        ]
    );
    assert_eq!(workspace["teamspaces"][0]["name"], teamspace.as_str());
    assert_eq!(workspace["projects"][0]["name"], "General");
    assert!(
        !workspace["projects"][0]["statuses"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    let handbook_md = archive.text(&format!("pages/{teamspace}/Handbook.md"));
    assert!(handbook_md.contains("# Handbook"), "{handbook_md}");
    assert!(names.contains(&format!("pages/{teamspace}/Handbook/Onboarding.md")));
    assert!(names.contains("pages/Private/My notes.md"));
    assert!(
        !names.iter().any(|name| name.contains("secret")),
        "another member's private page: {names:?}"
    );

    let channel = archive.json("chat/launch.json");
    assert_eq!(channel["kind"], "public");
    let messages = channel["messages"].as_array().unwrap();
    let hello = messages
        .iter()
        .find(|message| message["body"] == "Hello launch")
        .unwrap();
    assert_eq!(hello["author"]["name"], "Owner");
    assert_eq!(hello["replies"][0]["body"], "First reply");
    assert!(
        !names.iter().any(|name| name.contains("mia-only")),
        "a private channel the exporter is not in: {names:?}"
    );
    assert!(
        archive
            .text("README.txt")
            .contains("tasks/<KEY>-<number>.json")
    );

    let actions: Vec<String> = sqlx::query_scalar(
        "SELECT action FROM audit_events WHERE resource_type = 'workspace_export' \
         ORDER BY occurred_at, id",
    )
    .fetch_all(harness.database.pool())
    .await
    .unwrap();
    assert_eq!(
        actions,
        vec!["workspace.export_created", "workspace.export_downloaded"]
    );
}

#[tokio::test]
async fn a_read_only_workspace_still_exports_through_the_background_job() {
    let mut harness = Harness::new(true).await;
    sqlx::query("UPDATE workspaces SET billing_exempt = 0, created_at = $1 WHERE id = $2")
        .bind(TimestampMillis::now().as_millis() - 30 * DAY_MILLIS)
        .bind(&harness.workspace_id)
        .execute(harness.database.pool())
        .await
        .unwrap();
    let (status, problem) = harness
        .call(
            &harness.owner,
            "POST",
            &harness.workspace("/pages"),
            Some(json!({"title": "Blocked"})),
        )
        .await;
    assert_eq!(status, StatusCode::PAYMENT_REQUIRED, "{problem}");
    assert_eq!(problem["code"], "workspace_read_only");

    let export_id = harness.start_export().await;
    let stop = harness.serve().await;
    let mut ready = false;
    for _ in 0..300 {
        let list = harness
            .ok(&harness.owner, "GET", &harness.workspace("/exports"), None)
            .await;
        if list["items"][0]["status"] == "ready" {
            ready = true;
            break;
        }
        assert_ne!(list["items"][0]["status"], "failed", "{list}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(ready, "the worker finished the export job");
    let (status, bytes, _) = harness.download(&harness.owner, &export_id).await;
    assert_eq!(status, StatusCode::OK);
    let mut archive = Archive::new(bytes);
    assert_eq!(archive.json("workspace.json")["name"], "Acme");
    let job: String = sqlx::query_scalar("SELECT state FROM jobs WHERE kind = 'workspace.export'")
        .fetch_one(harness.database.pool())
        .await
        .unwrap();
    assert_eq!(job, "succeeded");
    stop.await;
}

#[tokio::test]
async fn retention_deletes_an_expired_export_and_its_archive() {
    let harness = Harness::new(false).await;
    let export_id = harness.start_export().await;
    harness.run_export(&export_id).await;
    let archive = harness.root.path().join(format!(
        "backups/exports/{}/{export_id}.zip",
        harness.workspace_id
    ));
    assert!(
        archive.is_file(),
        "stored on the local disk under the backups directory"
    );
    let (status, _, _) = harness.download(&harness.owner, &export_id).await;
    assert_eq!(status, StatusCode::OK);

    let app = harness.app.as_ref().unwrap();
    app.run_retention_maintenance().await.unwrap();
    assert!(archive.is_file(), "an unexpired export stays");

    sqlx::query("UPDATE workspace_exports SET expires_at = $1 WHERE id = $2")
        .bind(TimestampMillis::now().as_millis() - 1)
        .bind(&export_id)
        .execute(harness.database.pool())
        .await
        .unwrap();
    let (status, _, _) = harness.download(&harness.owner, &export_id).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "expired exports are not served"
    );
    app.run_retention_maintenance().await.unwrap();
    assert!(!archive.exists());
    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM workspace_exports")
        .fetch_one(harness.database.pool())
        .await
        .unwrap();
    assert_eq!(rows, 0);
    let list = harness
        .ok(&harness.owner, "GET", &harness.workspace("/exports"), None)
        .await;
    assert_eq!(list, json!({"items": []}));
    // A new export can start once the old one is gone.
    harness.start_export().await;
}
