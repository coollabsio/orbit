use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use orbit_platform::{
    AuthenticatedUser, BackupService, Id, LocalBlobStore, NewAttachmentReference, PasswordService,
    TestDatabase, TimestampMillis, UploadLimits, UploadService,
};
use orbit_server::auth_routes::CookieMode;
use orbit_server::backup_item_routes::{BackupItemState, backup_item_router};
use orbit_server::page_file_routes::{PageFileState, page_file_router};
use orbit_server::page_routes::{PageState, page_router};
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::repositories::page_files::PageFileRepository;
use orbit_server::task_routes::{TaskState, task_router};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tower::ServiceExt;

const PNG: &[u8] = b"\x89\x50\x4e\x47\x0d\x0a\x1a\x0a\x00\x00\x00\x0d\x49\x48\x44\x52\x00\x00\x00\x01\x00\x00\x00\x01\x08\x04\x00\x00\x00\xb5\x1c\x0c\x02\x00\x00\x00\x0b\x49\x44\x41\x54\x78\xda\x63\x64\xf8\x0f\x00\x01\x05\x01\x01\x27\x18\xe3\x66\x00\x00\x00\x00\x49\x45\x4e\x44\xae\x42\x60\x82";

struct Fixture {
    root: tempfile::TempDir,
    database: TestDatabase,
    identity: Arc<IdentityRepository>,
    uploads: UploadService,
    backups: BackupService,
    app: axum::Router,
    workspace_id: String,
    project_id: String,
    owner_id: Id,
    owner_cookie: String,
}

impl Fixture {
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
        let attachments = root.path().join("attachments");
        let backups = BackupService::new(root.path().join("backups"), &attachments);
        let uploads = UploadService::new(
            (*database).clone(),
            Arc::new(LocalBlobStore::new(&attachments)),
            backups.attachment_mutations(),
            UploadLimits::default(),
        );
        let cookie_mode = CookieMode::secure();
        let app = page_router(PageState::new(Arc::clone(&identity), cookie_mode))
            .merge(page_file_router(PageFileState::new(
                Arc::clone(&identity),
                uploads.clone(),
                cookie_mode,
            )))
            .merge(task_router(TaskState::new(
                Arc::clone(&identity),
                cookie_mode,
            )))
            .merge(backup_item_router(BackupItemState::new(
                Arc::clone(&identity),
                Some(backups.clone()),
                uploads.clone(),
                cookie_mode,
            )));
        Self {
            root,
            database,
            identity,
            uploads,
            backups,
            app,
            workspace_id: setup.workspace_id.to_string(),
            project_id: setup.project_id.to_string(),
            owner_id: setup.user_id,
            owner_cookie: format!("__Host-orbit_session={}", setup.session.token),
        }
    }

    async fn request(
        &self,
        cookie: &str,
        method: &str,
        uri: &str,
        body: Option<Value>,
    ) -> (StatusCode, Vec<u8>) {
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
        let response = self.app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (status, bytes.to_vec())
    }

    async fn call(&self, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
        self.call_as(&self.owner_cookie, method, uri, body).await
    }

    async fn call_as(
        &self,
        cookie: &str,
        method: &str,
        uri: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let (status, bytes) = self.request(cookie, method, uri, body).await;
        let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, value)
    }

    fn pages_uri(&self) -> String {
        format!("/api/v1/workspaces/{}/pages", self.workspace_id)
    }

    fn backup_uri(&self, backup: &str) -> String {
        format!(
            "/api/v1/admin/backups/{backup}/workspaces/{}",
            self.workspace_id
        )
    }

    async fn create_page(&self, title: &str, parent: Option<&str>) -> String {
        let (status, page) = self
            .call(
                "POST",
                &self.pages_uri(),
                Some(json!({"title": title, "parent_id": parent})),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{page}");
        page["id"].as_str().unwrap().to_owned()
    }

    async fn page(&self, id: &str) -> Value {
        let (status, page) = self
            .call("GET", &format!("{}/{id}", self.pages_uri()), None)
            .await;
        assert_eq!(status, StatusCode::OK, "{page}");
        page
    }

    async fn update_page(&self, id: &str, changes: Value) {
        let mut body = changes;
        body["expected_version"] = self.page(id).await["version"].clone();
        let (status, page) = self
            .call("PATCH", &format!("{}/{id}", self.pages_uri()), Some(body))
            .await;
        assert_eq!(status, StatusCode::OK, "{page}");
    }

    async fn attach(&self, page: &str, name: &str, bytes: &[u8]) -> String {
        PageFileRepository::new((*self.database).clone(), self.uploads.clone())
            .create_from_bytes(
                self.workspace_id.parse().unwrap(),
                page.parse().unwrap(),
                self.owner_id,
                name,
                bytes,
                "test",
            )
            .await
            .unwrap()
            .url
    }

    async fn trash(&self, id: &str) {
        let version = self.page(id).await["version"].clone();
        let (status, body) = self
            .call(
                "DELETE",
                &format!("{}/{id}?expected_version={version}", self.pages_uri()),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    }

    async fn purge(&self, id: &str) {
        self.trash(id).await;
        let version: i64 = sqlx::query_scalar("SELECT version FROM pages WHERE id = $1")
            .bind(id)
            .fetch_one(self.database.pool())
            .await
            .unwrap();
        let (status, body) = self
            .call(
                "DELETE",
                &format!(
                    "{}/{id}/permanent?expected_version={version}",
                    self.pages_uri()
                ),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    }

    async fn status_id(&self, project: &str, name: &str) -> String {
        sqlx::query_scalar("SELECT id FROM task_statuses WHERE project_id = $1 AND name = $2")
            .bind(project)
            .bind(name)
            .fetch_one(self.database.pool())
            .await
            .unwrap()
    }

    async fn create_task(&self, project: &str, body: Value) -> Value {
        let mut body = body;
        body["project_id"] = json!(project);
        if body.get("status_id").is_none() {
            body["status_id"] = json!(self.status_id(project, "Todo").await);
        }
        let (status, task) = self
            .call(
                "POST",
                &format!("/api/v1/workspaces/{}/tasks", self.workspace_id),
                Some(body),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{task}");
        task
    }

    async fn backup(&self) -> String {
        self.backups.create(&self.database).await.unwrap().id
    }

    async fn restore(&self, backup: &str, body: Value) -> (StatusCode, Value) {
        self.call(
            "POST",
            &format!("{}/restore", self.backup_uri(backup)),
            Some(body),
        )
        .await
    }

    async fn add_member(&self, email: &str, admin: bool) -> (Id, String) {
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
             VALUES ($1, $2, $3, $4, 0, $5, $6)",
        )
        .bind(Id::new_v7().to_string())
        .bind(&self.workspace_id)
        .bind(id.to_string())
        .bind(if admin { "admin" } else { "member" })
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

    async fn audit_count(&self, resource: &str) -> i64 {
        sqlx::query_scalar(
            "SELECT COUNT(*) FROM audit_events WHERE action = 'backup.item_restored' AND resource_id = $1",
        )
        .bind(resource)
        .fetch_one(self.database.pool())
        .await
        .unwrap()
    }
}

fn find<'a>(items: &'a Value, title: &str) -> &'a Value {
    items["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["title"] == title)
        .unwrap_or_else(|| panic!("{title} is not in {items}"))
}

fn titles(items: &Value) -> Vec<String> {
    let mut titles: Vec<String> = items["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["title"].as_str().unwrap().to_owned())
        .collect();
    titles.sort();
    titles
}

/// The id in a page file URL `/api/v1/workspaces/{ws}/pages/{page}/files/{file}`: `(page, file)`.
fn file_parts(url: &str) -> (String, String) {
    let parts: Vec<&str> = url.split('/').collect();
    (parts[6].to_owned(), parts[8].to_owned())
}

#[tokio::test]
async fn lists_the_workspaces_and_items_of_a_backup() {
    let fixture = Fixture::new().await;
    let alpha = fixture.create_page("Alpha plan", None).await;
    fixture.create_page("Beta notes", Some(&alpha)).await;
    let gamma = fixture.create_page("Gamma old", None).await;
    fixture.trash(&gamma).await;
    fixture
        .create_task(&fixture.project_id, json!({"title": "Fix login"}))
        .await;
    let backup = fixture.backup().await;
    fixture.create_page("Delta later", None).await;

    let (status, workspaces) = fixture
        .call(
            "GET",
            &format!("/api/v1/admin/backups/{backup}/workspaces"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{workspaces}");
    assert_eq!(
        workspaces["items"],
        json!([{"id": fixture.workspace_id, "name": "Orbit", "live": true}])
    );

    let items_uri = format!("{}/items", fixture.backup_uri(&backup));
    let (status, pages) = fixture
        .call("GET", &format!("{items_uri}?kind=page"), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{pages}");
    assert_eq!(titles(&pages), ["Alpha plan", "Beta notes", "Gamma old"]);
    let alpha_item = find(&pages, "Alpha plan");
    assert_eq!(alpha_item["kind"], "page");
    assert_eq!(alpha_item["id"], alpha.as_str());
    assert_eq!(alpha_item["child_count"], 1);
    assert_eq!(alpha_item["parent_title"], Value::Null);
    assert_eq!(alpha_item["deleted"], false);
    assert!(alpha_item["updated_at"].as_i64().unwrap() > 0);
    assert_eq!(find(&pages, "Beta notes")["parent_title"], "Alpha plan");
    assert_eq!(find(&pages, "Gamma old")["deleted"], true);

    let (_, filtered) = fixture
        .call("GET", &format!("{items_uri}?kind=page&q=ALPHA"), None)
        .await;
    assert_eq!(titles(&filtered), ["Alpha plan"]);

    let (status, tasks) = fixture
        .call("GET", &format!("{items_uri}?kind=task&q=login"), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{tasks}");
    assert_eq!(titles(&tasks), ["Fix login"]);
    let task = find(&tasks, "Fix login");
    assert_eq!(task["kind"], "task");
    assert_eq!(task["project_name"], "General");
    assert_eq!(task["number"], 1);
    assert_eq!(task["child_count"], 0);
    assert_eq!(task["parent_title"], Value::Null);

    let (status, missing_kind) = fixture.call("GET", &items_uri, None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{missing_kind}");
    let unknown_workspace = format!(
        "/api/v1/admin/backups/{backup}/workspaces/{}/items?kind=page",
        Id::new_v7()
    );
    let (status, body) = fixture.call("GET", &unknown_workspace, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "workspace_not_found");
    let unknown_backup = format!("/api/v1/admin/backups/{}/workspaces", Id::new_v7());
    let (status, body) = fixture.call("GET", &unknown_backup, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "backup_not_found");
    let (status, body) = fixture
        .restore(
            &backup,
            json!({"kind": "page", "id": Id::new_v7().to_string(), "include_children": false}),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "item_not_found");
}

#[tokio::test]
async fn only_installation_admins_may_browse_or_restore() {
    let fixture = Fixture::new().await;
    let page = fixture.create_page("Secret", None).await;
    let backup = fixture.backup().await;
    let (_, admin_cookie) = fixture.add_member("admin@example.com", true).await;
    for (method, uri, body) in [
        (
            "GET",
            format!("/api/v1/admin/backups/{backup}/workspaces"),
            None,
        ),
        (
            "GET",
            format!("{}/items?kind=page", fixture.backup_uri(&backup)),
            None,
        ),
        (
            "POST",
            format!("{}/restore", fixture.backup_uri(&backup)),
            Some(json!({"kind": "page", "id": page, "include_children": false})),
        ),
    ] {
        let (status, problem) = fixture
            .call_as(&admin_cookie, method, &uri, body.clone())
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{uri}");
        assert_eq!(problem["code"], "installation_admin_required");
        let (status, _) = fixture.call_as("", method, &uri, body).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{uri}");
    }
    let pages: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pages")
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    assert_eq!(pages, 1);
}

#[tokio::test]
async fn a_page_with_a_sub_page_and_a_file_restores_as_a_copy_under_its_live_parent() {
    let fixture = Fixture::new().await;
    let parent = fixture.create_page("Parent", None).await;
    let root = fixture.create_page("Root", Some(&parent)).await;
    let child = fixture.create_page("Child", Some(&root)).await;
    let file_url = fixture.attach(&root, "pixel.png", PNG).await;
    let content = json!([
        {"type": "image", "props": {"url": file_url}},
        {"type": "paragraph", "content": [
            {"type": "link", "href": format!("/docs/{child}"), "content": [{"type": "text", "text": "child", "styles": {}}]}
        ]},
        {"type": "paragraph", "content": [{"type": "text", "text": "old words", "styles": {}}]}
    ]);
    fixture
        .update_page(
            &root,
            json!({"content": content, "cover_url": file_url, "icon": "📘"}),
        )
        .await;
    let backup = fixture.backup().await;
    fixture
        .update_page(&root, json!({"title": "Root changed", "content": []}))
        .await;

    let (status, restored) = fixture
        .restore(
            &backup,
            json!({"kind": "page", "id": root, "include_children": true}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{restored}");
    let new_root = restored["id"].as_str().unwrap().to_owned();
    assert_ne!(new_root, root);
    assert_eq!(restored["kind"], "page");
    assert_eq!(restored["title"], "Root (restored)");
    assert_eq!(restored["path"], format!("/docs/{new_root}"));
    assert_eq!(restored["missing_files"], 0);

    let copy = fixture.page(&new_root).await;
    assert_eq!(copy["title"], "Root (restored)");
    assert_eq!(copy["parent_id"], parent.as_str());
    assert_eq!(copy["icon"], "📘");
    let new_file_url = copy["content"][0]["props"]["url"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_ne!(new_file_url, file_url);
    let (file_page, _) = file_parts(&new_file_url);
    assert_eq!(file_page, new_root);
    assert_eq!(copy["cover_url"], new_file_url.as_str());
    assert_eq!(copy["content"][2]["content"][0]["text"], "old words");

    let (status, pages) = fixture.call("GET", &fixture.pages_uri(), None).await;
    assert_eq!(status, StatusCode::OK);
    let new_child = pages["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|page| page["parent_id"] == new_root.as_str())
        .unwrap();
    assert_eq!(new_child["title"], "Child");
    let new_child = new_child["id"].as_str().unwrap();
    assert_ne!(new_child, child);
    assert_eq!(
        copy["content"][1]["content"][0]["href"],
        format!("/docs/{new_child}")
    );

    let (status, bytes) = fixture
        .request(&fixture.owner_cookie, "GET", &new_file_url, None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(bytes, PNG);
    // The copy shares the live blob.
    let blobs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM attachment_blobs")
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    assert_eq!(blobs, 1);

    let original = fixture.page(&root).await;
    assert_eq!(original["title"], "Root changed");
    assert_eq!(original["content"], json!([]));
    assert_eq!(fixture.page(&child).await["parent_id"], root.as_str());
    assert_eq!(fixture.audit_count(&new_root).await, 1);
}

#[tokio::test]
async fn a_page_purged_after_the_backup_restores_with_its_file_from_the_snapshot() {
    let fixture = Fixture::new().await;
    let parent = fixture.create_page("Folder", None).await;
    let page = fixture.create_page("Gone", Some(&parent)).await;
    let file_url = fixture.attach(&page, "pixel.png", PNG).await;
    fixture
        .update_page(
            &page,
            json!({"content": [{"type": "image", "props": {"url": file_url}}]}),
        )
        .await;
    let backup = fixture.backup().await;
    fixture.purge(&page).await;
    fixture.purge(&parent).await;
    // Reconcile removed the unused blob from the live store too.
    let storage_key: String = sqlx::query_scalar("SELECT storage_key FROM attachment_blobs")
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    sqlx::query("DELETE FROM attachment_blobs")
        .execute(fixture.database.pool())
        .await
        .unwrap();
    std::fs::remove_file(fixture.root.path().join("attachments").join(&storage_key)).unwrap();

    let (status, items) = fixture
        .call(
            "GET",
            &format!("{}/items?kind=page&q=gone", fixture.backup_uri(&backup)),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(titles(&items), ["Gone"]);

    let (status, restored) = fixture
        .restore(
            &backup,
            json!({"kind": "page", "id": page, "include_children": false}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{restored}");
    assert_eq!(restored["missing_files"], 0);
    let copy = fixture.page(restored["id"].as_str().unwrap()).await;
    // The old parent is gone: the copy goes to the top level of the teamspace.
    assert_eq!(copy["parent_id"], Value::Null);
    assert!(copy["teamspace_id"].is_string());
    let new_file_url = copy["content"][0]["props"]["url"].as_str().unwrap();
    let (status, bytes) = fixture
        .request(&fixture.owner_cookie, "GET", new_file_url, None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(bytes, PNG);
}

#[tokio::test]
async fn a_file_whose_bytes_are_nowhere_is_left_out_and_counted() {
    let fixture = Fixture::new().await;
    let page = fixture.create_page("Lossy", None).await;
    fixture.attach(&page, "pixel.png", PNG).await;
    let backup = fixture.backup().await;
    let snapshot = fixture.backups.verify(&backup).await.unwrap();
    let storage_key: String = sqlx::query_scalar("SELECT storage_key FROM attachment_blobs")
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    sqlx::query("UPDATE attachment_blobs SET deleted_at = 1, sha256 = 'deleted:x', storage_key = 'deleted:x'")
        .execute(fixture.database.pool())
        .await
        .unwrap();
    std::fs::remove_file(fixture.root.path().join("attachments").join(&storage_key)).unwrap();
    // The snapshot was verified above; the cache does not check its files again.
    let _ = fixture.backups.verified_snapshot(&backup).await.unwrap();
    std::fs::remove_file(snapshot.path.join("attachments").join(&storage_key)).unwrap();

    let (status, restored) = fixture
        .restore(
            &backup,
            json!({"kind": "page", "id": page, "include_children": false}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{restored}");
    assert_eq!(restored["missing_files"], 1);
    let files: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM page_files WHERE page_id = $1")
        .bind(restored["id"].as_str().unwrap())
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    assert_eq!(files, 0);
}

#[tokio::test]
async fn a_task_restores_into_its_live_project_with_status_labels_and_files_by_name() {
    let fixture = Fixture::new().await;
    let workspace_id: Id = fixture.workspace_id.parse().unwrap();
    let (member, _) = fixture.add_member("member@example.com", false).await;
    let labels_uri = format!(
        "/api/v1/workspaces/{}/projects/{}/labels",
        fixture.workspace_id, fixture.project_id
    );
    let (status, bug) = fixture
        .call(
            "POST",
            &labels_uri,
            Some(json!({"name": "bug", "color": "#ff0000"})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{bug}");
    let (_, other) = fixture
        .call(
            "POST",
            &labels_uri,
            Some(json!({"name": "other", "color": "#00ff00"})),
        )
        .await;
    let task = fixture
        .create_task(
            &fixture.project_id,
            json!({
                "title": "Broken sign-in",
                "description": "Steps to reproduce",
                "priority": "high",
                "status_id": fixture.status_id(&fixture.project_id, "In Progress").await,
                "label_ids": [bug["id"]],
                "assignee_ids": [fixture.owner_id, member],
                "due_at": "2026-12-01T00:00:00Z"
            }),
        )
        .await;
    let task_id: Id = task["id"].as_str().unwrap().parse().unwrap();
    let staged = fixture
        .uploads
        .stage(workspace_id, fixture.owner_id, "log.png", PNG)
        .await
        .unwrap();
    let reference = NewAttachmentReference::new(task_id, None);
    let file_url = format!(
        "/api/v1/workspaces/{workspace_id}/tasks/{task_id}/attachments/{}/download",
        reference.id
    );
    fixture.uploads.finalize(&staged, reference).await.unwrap();
    let (status, task) = fixture
        .call(
            "PATCH",
            &format!("/api/v1/workspaces/{}/tasks/{task_id}", fixture.workspace_id),
            Some(json!({
                "expected_version": task["version"],
                "description_json": [
                    {"type": "paragraph", "content": [{"type": "text", "text": "Steps to reproduce", "styles": {}}]},
                    {"type": "image", "props": {"url": file_url}}
                ]
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{task}");
    let backup = fixture.backup().await;
    // After the backup: the member leaves, the label is recreated (new id, same name).
    sqlx::query("DELETE FROM memberships WHERE user_id = $1")
        .bind(member.to_string())
        .execute(fixture.database.pool())
        .await
        .unwrap();
    sqlx::query("DELETE FROM task_labels")
        .execute(fixture.database.pool())
        .await
        .unwrap();
    sqlx::query("DELETE FROM labels WHERE id = $1")
        .bind(bug["id"].as_str().unwrap())
        .execute(fixture.database.pool())
        .await
        .unwrap();
    let (_, bug) = fixture
        .call(
            "POST",
            &labels_uri,
            Some(json!({"name": "Bug", "color": "#ff0000"})),
        )
        .await;

    let (status, restored) = fixture
        .restore(
            &backup,
            json!({"kind": "task", "id": task_id.to_string(), "include_children": false}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{restored}");
    assert_eq!(restored["kind"], "task");
    assert_eq!(restored["title"], "Broken sign-in (restored)");
    assert_eq!(restored["missing_files"], 0);
    let new_id = restored["id"].as_str().unwrap();
    let key: String = sqlx::query_scalar("SELECT project_key FROM projects WHERE id = $1")
        .bind(&fixture.project_id)
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    assert_eq!(restored["path"], format!("/tasks/{key}-2"));

    let (status, copy) = fixture
        .call(
            "GET",
            &format!("/api/v1/workspaces/{}/tasks/{new_id}", fixture.workspace_id),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{copy}");
    assert_eq!(copy["title"], "Broken sign-in (restored)");
    assert_eq!(copy["number"], 2);
    assert_eq!(copy["priority"], "high");
    let (reference_id,): (String,) =
        sqlx::query_as("SELECT id FROM attachment_references WHERE task_id = $1")
            .bind(new_id)
            .fetch_one(fixture.database.pool())
            .await
            .unwrap();
    let new_file_url = format!(
        "/api/v1/workspaces/{workspace_id}/tasks/{new_id}/attachments/{reference_id}/download"
    );
    assert_eq!(
        copy["description_json"][0]["content"][0]["text"],
        "Steps to reproduce"
    );
    assert_eq!(
        copy["description_json"][1]["props"]["url"],
        new_file_url.as_str()
    );
    let description = copy["description"].as_str().unwrap();
    assert!(
        description.starts_with("Steps to reproduce"),
        "{description}"
    );
    assert!(description.contains(&new_file_url), "{description}");
    assert!(!description.contains(&task_id.to_string()), "{description}");
    assert_eq!(
        copy["status_id"],
        fixture
            .status_id(&fixture.project_id, "In Progress")
            .await
            .as_str()
    );
    assert_eq!(copy["label_ids"], json!([bug["id"]]));
    assert_ne!(copy["label_ids"], json!([other["id"]]));
    assert_eq!(copy["assignee_ids"], json!([fixture.owner_id.to_string()]));
    assert_eq!(copy["due_at"], task["due_at"]);
    let (blob, original_blob): (String, String) = sqlx::query_as(
        "SELECT copy.blob_id, original.blob_id FROM attachment_references copy \
         JOIN attachment_references original ON original.task_id = $2 WHERE copy.task_id = $1",
    )
    .bind(new_id)
    .bind(task_id.to_string())
    .fetch_one(fixture.database.pool())
    .await
    .unwrap();
    assert_eq!(blob, original_blob);
    assert_eq!(fixture.audit_count(new_id).await, 1);
}

#[tokio::test]
async fn a_task_whose_project_is_gone_is_refused() {
    let fixture = Fixture::new().await;
    let (status, project) = fixture
        .call(
            "POST",
            &format!("/api/v1/workspaces/{}/projects", fixture.workspace_id),
            Some(json!({"name": "Temp", "key": "TMP", "color": "#abcdef"})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{project}");
    let project_id = project["id"].as_str().unwrap();
    let task = fixture
        .create_task(project_id, json!({"title": "Temporary"}))
        .await;
    let backup = fixture.backup().await;
    sqlx::query("UPDATE projects SET deleted_at = $1 WHERE id = $2")
        .bind(TimestampMillis::now().as_millis())
        .bind(project_id)
        .execute(fixture.database.pool())
        .await
        .unwrap();
    let tasks_before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tasks")
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();

    let (status, problem) = fixture
        .restore(
            &backup,
            json!({"kind": "task", "id": task["id"], "include_children": false}),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{problem}");
    assert_eq!(problem["code"], "project_missing");
    let tasks_after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tasks")
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    assert_eq!(tasks_after, tasks_before);
}

#[tokio::test]
async fn postgres_snapshots_are_read_compressed_and_as_legacy_plain_copy_files() {
    if !orbit_platform::testing_postgres() {
        // Only Postgres snapshots hold `COPY` files.
        return;
    }
    let fixture = Fixture::new().await;
    let page = fixture
        .create_page("Tab\there \\ \"quoted\"\nline", None)
        .await;
    let content = json!([{"type": "paragraph", "content": [{"type": "text", "text": "é\t\\N", "styles": {}}]}]);
    fixture
        .update_page(&page, json!({"content": content}))
        .await;

    let compressed = fixture.backup().await;
    let snapshot = fixture.backups.verify(&compressed).await.unwrap();
    assert!(snapshot.path.join("database/pages.copy.zst").is_file());
    let legacy = fixture.backup().await;
    let snapshot = fixture.backups.verify(&legacy).await.unwrap();
    let mut manifest = snapshot.manifest.clone();
    for file in &mut manifest.files {
        let Some(plain) = file.path.strip_suffix(".zst") else {
            continue;
        };
        let path = snapshot.path.join(&file.path);
        let contents = zstd::decode_all(std::fs::File::open(&path).unwrap()).unwrap();
        std::fs::remove_file(&path).unwrap();
        std::fs::write(snapshot.path.join(plain), &contents).unwrap();
        file.path = plain.to_owned();
        file.sha256 = format!("{:x}", Sha256::digest(&contents));
        file.byte_size = contents.len() as u64;
    }
    std::fs::write(
        snapshot.path.join("manifest.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    assert!(snapshot.path.join("database/pages.copy").is_file());

    for backup in [compressed, legacy] {
        let (status, items) = fixture
            .call(
                "GET",
                &format!("{}/items?kind=page", fixture.backup_uri(&backup)),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{items}");
        assert_eq!(titles(&items), ["Tab\there \\ \"quoted\"\nline"]);
        let (status, restored) = fixture
            .restore(
                &backup,
                json!({"kind": "page", "id": page, "include_children": false}),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{restored}");
        let copy = fixture.page(restored["id"].as_str().unwrap()).await;
        assert_eq!(copy["title"], "Tab\there \\ \"quoted\"\nline (restored)");
        assert_eq!(copy["content"], content);
    }
}
