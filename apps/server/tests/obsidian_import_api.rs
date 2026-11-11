//! Obsidian import API and jobs: upload a vault zip, scan it, import it into Docs.

use std::io::{Cursor, Write};
use std::sync::Arc;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use orbit_platform::{
    AttachmentMutationCoordinator, AuthenticatedUser, Id, LocalBlobStore, ObjectStorage,
    PasswordService, TestDatabase, TimestampMillis, UploadLimits, UploadService,
};
use orbit_server::auth_routes::CookieMode;
use orbit_server::obsidian::import::{JobRun, ObsidianImportService, ObsidianImportSettings};
use orbit_server::obsidian_import_routes::{ObsidianImportState, obsidian_import_router};
use orbit_server::page_routes::{PageState, page_router};
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::repositories::page_files::PageFileRepository;
use serde_json::{Value, json};
use tower::ServiceExt;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

const PNG: &[u8] = b"\x89\x50\x4e\x47\x0d\x0a\x1a\x0a\x00\x00\x00\x0d\x49\x48\x44\x52\x00\x00\x00\x01\x00\x00\x00\x01\x08\x04\x00\x00\x00\xb5\x1c\x0c\x02\x00\x00\x00\x0b\x49\x44\x41\x54\x78\xda\x63\x64\xf8\x0f\x00\x01\x05\x01\x01\x27\x18\xe3\x66\x00\x00\x00\x00\x49\x45\x4e\x44\xae\x42\x60\x82";
const BOUNDARY: &str = "orbit-test-boundary";

fn zip(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in files {
        writer
            .start_file(*name, SimpleFileOptions::default())
            .unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn vault() -> Vec<u8> {
    zip(&[
        ("My Vault/.obsidian/app.json", b"{}"),
        (
            "My Vault/Projects/Projects.md",
            b"---\ntags: [x]\n---\nAll projects. See [[Plan|the plan]].",
        ),
        (
            "My Vault/Projects/Plan.md",
            b"> [!warning] Careful\n> Body\n\n![[diagram.png]]\n\n[[Inbox#Today]] [[Nowhere]] ==hot==",
        ),
        ("My Vault/Projects/diagram.png", PNG),
        ("My Vault/Inbox.md", b"# Inbox\n%%secret%%Done."),
        ("My Vault/unused.png", PNG),
    ])
}

fn multipart(name: &str, bytes: &[u8]) -> Vec<u8> {
    let mut body = format!(
        "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{name}\"\r\n\
         Content-Type: application/zip\r\n\r\n"
    )
    .into_bytes();
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{BOUNDARY}--\r\n").as_bytes());
    body
}

struct Harness {
    _root: tempfile::TempDir,
    database: TestDatabase,
    identity: Arc<IdentityRepository>,
    service: ObsidianImportService,
    app: Router,
    workspace_id: String,
    owner_cookie: String,
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
        let service = ObsidianImportService::new(
            (*database).clone(),
            PageFileRepository::new((*database).clone(), uploads),
            ObjectStorage::default(),
            root.path().join("backups"),
            ObsidianImportSettings {
                max_archive_bytes: 64 * 1024,
                max_file_bytes: 1024 * 1024,
            },
        );
        let app = obsidian_import_router(ObsidianImportState::new(
            Arc::clone(&identity),
            service.clone(),
            CookieMode::secure(),
        ))
        .merge(page_router(PageState::new(
            Arc::clone(&identity),
            CookieMode::secure(),
        )));
        Self {
            _root: root,
            database,
            identity,
            service,
            app,
            workspace_id: setup.workspace_id.to_string(),
            owner_cookie: format!("__Host-orbit_session={}", setup.session.token),
        }
    }

    fn imports_uri(&self) -> String {
        format!("/api/v1/workspaces/{}/imports/obsidian", self.workspace_id)
    }

    async fn send(&self, request: Request<Body>) -> (StatusCode, Value) {
        let response = self.app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn call(
        &self,
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
            None => builder
                .header(header::CONTENT_LENGTH, 0)
                .body(Body::empty()),
        }
        .unwrap();
        self.send(request).await
    }

    async fn upload(&self, cookie: &str, name: &str, bytes: &[u8]) -> (StatusCode, Value) {
        let request = Request::builder()
            .method("POST")
            .uri(self.imports_uri())
            .header(header::COOKIE, cookie)
            .header(
                header::CONTENT_TYPE,
                format!("multipart/form-data; boundary={BOUNDARY}"),
            )
            .body(Body::from(multipart(name, bytes)))
            .unwrap();
        self.send(request).await
    }

    /// Uploads a zip and runs its scan job directly.
    async fn scanned(&self, bytes: &[u8]) -> (String, Value) {
        let (status, import) = self.upload(&self.owner_cookie, "vault.zip", bytes).await;
        assert_eq!(status, StatusCode::CREATED, "{import}");
        assert_eq!(import["status"], "scanning");
        let id = import["id"].as_str().unwrap().to_owned();
        self.service
            .run_scan(id.parse().unwrap(), &JobRun::default())
            .await
            .unwrap();
        let detail = self.detail(&id, true).await;
        (id, detail)
    }

    async fn detail(&self, id: &str, tree: bool) -> Value {
        let query = if tree { "?include=tree" } else { "" };
        let (status, import) = self
            .call(
                &self.owner_cookie,
                "GET",
                &format!("{}/{id}{query}", self.imports_uri()),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{import}");
        import
    }

    /// Runs the import's job as the worker does: the job row shows it running meanwhile.
    async fn run_import(&self, id: &str) {
        let set_state = |from: &'static str, to: &'static str| {
            sqlx::query(
                "UPDATE jobs SET state = $1 WHERE kind = 'obsidian.import' AND state = $2 \
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

    async fn archive_location(&self, id: &str) -> Option<String> {
        sqlx::query_scalar("SELECT archive_location FROM obsidian_imports WHERE id = $1")
            .bind(id)
            .fetch_one(self.database.pool())
            .await
            .unwrap()
    }

    async fn page_id(&self, import_id: &str, path: &str) -> String {
        sqlx::query_scalar(
            "SELECT page_id FROM obsidian_import_items WHERE import_id = $1 AND path = $2",
        )
        .bind(import_id)
        .bind(path)
        .fetch_one(self.database.pool())
        .await
        .unwrap()
    }

    async fn page(&self, page_id: &str) -> Value {
        let (status, page) = self
            .call(
                &self.owner_cookie,
                "GET",
                &format!("/api/v1/workspaces/{}/pages/{page_id}", self.workspace_id),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{page}");
        page
    }

    async fn member_cookie(&self) -> String {
        let id = Id::new_v7();
        let now = TimestampMillis::now();
        sqlx::query(
            "INSERT INTO users (id, email, normalized_email, display_name, password_hash, created_at, updated_at) \
             VALUES ($1, 'member@example.com', 'member@example.com', 'Member', 'unused', $2, $3)",
        )
        .bind(id.to_string())
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
                    email: "member@example.com".to_owned(),
                    display_name: "Member".to_owned(),
                },
                now,
            )
            .await
            .unwrap();
        format!("__Host-orbit_session={}", session.token)
    }
}

#[tokio::test]
async fn imports_a_vault_into_private_pages() {
    let harness = Harness::new().await;
    let (id, ready) = harness.scanned(&vault()).await;
    assert_eq!(ready["status"], "ready", "{ready}");
    assert_eq!(ready["file_name"], "vault.zip");
    let tree = &ready["tree"];
    let nodes: Vec<(&str, &str)> = tree["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| {
            (
                node["path"].as_str().unwrap(),
                node["kind"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        nodes,
        [
            ("Projects", "folder"),
            ("Projects/Plan.md", "note"),
            ("Inbox.md", "note")
        ]
    );
    assert_eq!(tree["attachments"], 1);
    assert_eq!(tree["unresolved_links"], 1);
    assert_eq!(tree["skipped"]["system"], 1);
    assert_eq!(tree["skipped"]["unreferenced"], 1);

    let (status, started) = harness
        .call(
            &harness.owner_cookie,
            "POST",
            &format!("{}/{id}/start", harness.imports_uri()),
            Some(json!({"selection": {"all": true}, "destination": {"private": true}})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{started}");
    assert_eq!(started["status"], "queued");
    harness.run_import(&id).await;

    let import = harness.detail(&id, false).await;
    assert_eq!(import["status"], "completed", "{import}");
    assert_eq!(
        import["progress"],
        json!({"total": 3, "done": 3, "failed": 0})
    );
    assert_eq!(import["report"]["files_imported"], 1);
    assert_eq!(import["report"]["unresolved_links"], 1);
    assert_eq!(import["report"]["skipped_files"]["unreferenced"], 1);
    assert_eq!(import["root_page_ids"].as_array().unwrap().len(), 2);
    assert_eq!(harness.archive_location(&id).await, None, "archive deleted");

    let projects = harness.page(&harness.page_id(&id, "Projects").await).await;
    let plan_id = harness.page_id(&id, "Projects/Plan.md").await;
    let inbox_id = harness.page_id(&id, "Inbox.md").await;
    assert_eq!(projects["title"], "Projects");
    let folder_content = projects["content"].to_string();
    assert!(folder_content.contains("All projects."), "{folder_content}");
    assert!(!folder_content.contains("tags"), "frontmatter dropped");
    assert!(
        folder_content.contains(&format!("\"href\":\"/docs/{plan_id}\"")),
        "{folder_content}"
    );

    let plan = harness.page(&plan_id).await;
    assert_eq!(plan["title"], "Plan");
    let blocks = plan["content"].as_array().unwrap();
    assert_eq!(blocks[0]["type"], "callout");
    assert_eq!(blocks[0]["props"]["emoji"], "⚠️");
    assert_eq!(blocks[1]["type"], "image");
    assert!(
        blocks[1]["props"]["url"]
            .as_str()
            .unwrap()
            .contains(&format!("/pages/{plan_id}/files/")),
        "{}",
        blocks[1]
    );
    let last = blocks[2].to_string();
    assert!(last.contains(&format!("/docs/{inbox_id}")), "{last}");
    assert!(last.contains("Inbox > Today"), "{last}");
    assert!(last.contains("Nowhere"), "{last}");
    assert!(last.contains("\"backgroundColor\":\"yellow\""), "{last}");

    let inbox = harness.page(&inbox_id).await;
    let content = inbox["content"].to_string();
    assert!(
        content.contains("Done.") && !content.contains("secret"),
        "{content}"
    );
}

#[tokio::test]
async fn refuses_unsafe_archives_with_a_clear_error() {
    let harness = Harness::new().await;
    let (id, failed) = harness
        .scanned(&zip(&[("ok.md", b"ok"), ("../evil.md", b"x")]))
        .await;
    assert_eq!(failed["status"], "failed");
    assert_eq!(
        failed["error"],
        "The archive contains an unsafe path: ../evil.md"
    );
    assert_eq!(harness.archive_location(&id).await, None, "archive deleted");

    let (status, problem) = harness
        .upload(&harness.owner_cookie, "notes.txt", b"plain text")
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(problem["code"], "obsidian_archive_invalid");

    let (status, problem) = harness
        .upload(&harness.owner_cookie, "big.zip", &vec![b'P'; 128 * 1024])
        .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE, "{problem}");
}

#[tokio::test]
async fn imports_are_private_to_their_creator_and_cancel_deletes_the_archive() {
    let harness = Harness::new().await;
    let (id, ready) = harness.scanned(&vault()).await;
    assert_eq!(ready["status"], "ready");
    assert_eq!(
        harness.archive_location(&id).await.as_deref(),
        Some("local")
    );

    let member = harness.member_cookie().await;
    let (status, _) = harness
        .call(
            &member,
            "GET",
            &format!("{}/{id}", harness.imports_uri()),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, cancelled) = harness
        .call(
            &harness.owner_cookie,
            "POST",
            &format!("{}/{id}/cancel", harness.imports_uri()),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{cancelled}");
    assert_eq!(cancelled["status"], "cancelled");
    assert_eq!(harness.archive_location(&id).await, None);

    let (status, problem) = harness
        .call(
            &harness.owner_cookie,
            "POST",
            &format!("{}/{id}/start", harness.imports_uri()),
            Some(json!({"selection": {"all": true}})),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(problem["code"], "obsidian_import_state_conflict");
}
