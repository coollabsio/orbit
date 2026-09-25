use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use orbit_platform::{
    AttachmentMutationCoordinator, AuthenticatedUser, Id, LocalBlobStore, PasswordService,
    TestDatabase, TimestampMillis, UploadLimits, UploadService,
};
use orbit_server::auth_routes::CookieMode;
use orbit_server::page_file_routes::{PageFileState, page_file_router};
use orbit_server::page_routes::{PageState, page_router};
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::repositories::page_files::{PageFileError, PageFileRepository};
use orbit_server::repositories::workspaces::WorkspaceRepository;
use serde_json::{Value, json};
use tower::ServiceExt;

const PNG: &[u8] = b"\x89\x50\x4e\x47\x0d\x0a\x1a\x0a\x00\x00\x00\x0d\x49\x48\x44\x52\x00\x00\x00\x01\x00\x00\x00\x01\x08\x04\x00\x00\x00\xb5\x1c\x0c\x02\x00\x00\x00\x0b\x49\x44\x41\x54\x78\xda\x63\x64\xf8\x0f\x00\x01\x05\x01\x01\x27\x18\xe3\x66\x00\x00\x00\x00\x49\x45\x4e\x44\xae\x42\x60\x82";
const SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg"><script>alert(1)</script></svg>"#;
const HTML: &[u8] = b"<!doctype html><script>alert(1)</script>";
const DAY_MILLIS: i64 = 24 * 60 * 60 * 1_000;
const RETENTION_MILLIS: i64 = 30 * DAY_MILLIS;

struct Fixture {
    _root: tempfile::TempDir,
    database: TestDatabase,
    identity: Arc<IdentityRepository>,
    store: LocalBlobStore,
    uploads: UploadService,
    app: axum::Router,
    workspace_id: String,
    owner_id: Id,
    owner_cookie: String,
}

impl Fixture {
    async fn new(limits: UploadLimits) -> Self {
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
        let store = LocalBlobStore::new(root.path().join("attachments"));
        let uploads = UploadService::new(
            (*database).clone(),
            Arc::new(store.clone()),
            AttachmentMutationCoordinator::default(),
            limits,
        );
        let app = page_router(PageState::new(Arc::clone(&identity), CookieMode::secure())).merge(
            page_file_router(PageFileState::new(
                Arc::clone(&identity),
                uploads.clone(),
                CookieMode::secure(),
            )),
        );
        Self {
            _root: root,
            database,
            identity,
            store,
            uploads,
            app,
            workspace_id: setup.workspace_id.to_string(),
            owner_id: setup.user_id,
            owner_cookie: format!("__Host-orbit_session={}", setup.session.token),
        }
    }

    fn pages_uri(&self) -> String {
        format!("/api/v1/workspaces/{}/pages", self.workspace_id)
    }

    fn files_uri(&self, page_id: &str) -> String {
        format!("{}/{page_id}/files", self.pages_uri())
    }

    async fn call(
        &self,
        cookie: &str,
        method: &str,
        uri: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut builder = Request::builder().method(method).uri(uri);
        if !cookie.is_empty() {
            builder = builder.header(header::COOKIE, cookie);
        }
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
        let value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or(Value::Null)
        };
        (status, value)
    }

    async fn create_page(&self, cookie: &str, body: Value) -> Value {
        let (status, page) = self
            .call(cookie, "POST", &self.pages_uri(), Some(body))
            .await;
        assert_eq!(status, StatusCode::CREATED, "{page}");
        page
    }

    async fn upload(
        &self,
        cookie: &str,
        page_id: &str,
        name: &str,
        bytes: &[u8],
    ) -> (StatusCode, Value) {
        let response = self
            .app
            .clone()
            .oneshot(multipart_request(
                &self.files_uri(page_id),
                cookie,
                name,
                bytes,
            ))
            .await
            .unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    async fn download(&self, cookie: &str, url: &str) -> axum::response::Response {
        let mut builder = Request::builder().uri(url);
        if !cookie.is_empty() {
            builder = builder.header(header::COOKIE, cookie);
        }
        self.app
            .clone()
            .oneshot(builder.body(Body::empty()).unwrap())
            .await
            .unwrap()
    }

    async fn trash(&self, page_id: &str) {
        let (_, page) = self
            .call(
                &self.owner_cookie,
                "GET",
                &format!("{}/{page_id}", self.pages_uri()),
                None,
            )
            .await;
        let (status, body) = self
            .call(
                &self.owner_cookie,
                "DELETE",
                &format!(
                    "{}/{page_id}?expected_version={}",
                    self.pages_uri(),
                    page["version"]
                ),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    }

    async fn count(&self, sql: &str) -> i64 {
        sqlx::query_scalar(sql)
            .fetch_one(self.database.pool())
            .await
            .unwrap()
    }

    async fn add_member(&self, workspace_id: &str, email: &str) -> String {
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
        .bind(workspace_id)
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
        format!("__Host-orbit_session={}", session.token)
    }
}

fn multipart_request(uri: &str, cookie: &str, name: &str, bytes: &[u8]) -> Request<Body> {
    let boundary = "orbit-test-boundary";
    let mut body = Vec::new();
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        format!("Content-Disposition: form-data; name=\"file\"; filename=\"{name}\"\r\n\r\n")
            .as_bytes(),
    );
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    let mut builder = Request::builder().method("POST").uri(uri).header(
        header::CONTENT_TYPE,
        format!("multipart/form-data; boundary={boundary}"),
    );
    if !cookie.is_empty() {
        builder = builder.header(header::COOKIE, cookie);
    }
    builder.body(Body::from(body)).unwrap()
}

fn id_of(value: &Value) -> &str {
    value["id"].as_str().unwrap()
}

async fn body_bytes(response: axum::response::Response) -> Vec<u8> {
    to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap()
        .to_vec()
}

#[tokio::test]
async fn uploads_an_image_and_serves_it_inline_with_safety_headers() {
    let fixture = Fixture::new(UploadLimits::default()).await;
    let owner = fixture.owner_cookie.clone();
    let page = fixture
        .create_page(&owner, json!({"title": "Pictures"}))
        .await;

    let (unauthenticated, _) = fixture.upload("", id_of(&page), "a.png", PNG).await;
    assert_eq!(unauthenticated, StatusCode::UNAUTHORIZED);
    assert!(fixture.store.temporary_files().await.unwrap().is_empty());

    let (status, file) = fixture
        .upload(&owner, id_of(&page), "folder/photo.png", PNG)
        .await;
    assert_eq!(status, StatusCode::CREATED, "{file}");
    let url = file["url"].as_str().unwrap();
    assert_eq!(
        url,
        format!("{}/{}", fixture.files_uri(id_of(&page)), id_of(&file))
    );
    assert_eq!(file["page_id"], page["id"]);
    assert_eq!(file["file_name"], "photo.png");
    assert_eq!(file["mime_type"], "image/png");
    assert_eq!(file["size_bytes"], PNG.len());
    assert!(file["created_at"].is_string());

    let response = fixture.download(&owner, url).await;
    assert_eq!(response.status(), StatusCode::OK);
    let headers = response.headers().clone();
    assert_eq!(headers[header::CONTENT_TYPE], "image/png");
    assert_eq!(
        headers[header::CONTENT_DISPOSITION],
        "inline; filename=\"photo.png\""
    );
    assert_eq!(headers["x-content-type-options"], "nosniff");
    assert_eq!(body_bytes(response).await, PNG);

    // Same bytes on another upload share the blob.
    fixture.upload(&owner, id_of(&page), "again.png", PNG).await;
    assert_eq!(
        fixture.count("SELECT COUNT(*) FROM attachment_blobs").await,
        1
    );
    assert_eq!(fixture.count("SELECT COUNT(*) FROM page_files").await, 2);
    assert_eq!(
        fixture
            .count("SELECT COUNT(*) FROM audit_events WHERE action = 'page.file_added'")
            .await,
        2
    );

    let (unknown, _) = fixture
        .call(
            &owner,
            "GET",
            &format!("{}/{}", fixture.files_uri(id_of(&page)), Id::new_v7()),
            None,
        )
        .await;
    assert_eq!(unknown, StatusCode::NOT_FOUND);
    let (malformed, _) = fixture
        .call(
            &owner,
            "GET",
            &format!("{}/not-an-id", fixture.files_uri(id_of(&page))),
            None,
        )
        .await;
    assert_eq!(malformed, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn active_content_and_unknown_types_download_as_attachments() {
    let fixture = Fixture::new(UploadLimits::default()).await;
    let owner = fixture.owner_cookie.clone();
    let page = fixture.create_page(&owner, json!({"title": "Files"})).await;
    for (name, bytes, mime) in [
        ("logo.png", SVG, "image/svg+xml"),
        ("page.html", HTML, "text/html"),
        ("notes.bin", &b"plain bytes"[..], "application/octet-stream"),
    ] {
        let (status, file) = fixture.upload(&owner, id_of(&page), name, bytes).await;
        assert_eq!(status, StatusCode::CREATED, "{file}");
        assert_eq!(file["mime_type"], mime);
        let response = fixture
            .download(&owner, file["url"].as_str().unwrap())
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CONTENT_TYPE], mime);
        assert_eq!(
            response.headers()[header::CONTENT_DISPOSITION],
            format!("attachment; filename=\"{name}\"")
        );
        assert_eq!(response.headers()["x-content-type-options"], "nosniff");
    }
}

#[tokio::test]
async fn uploads_over_the_size_limit_or_without_one_file_are_rejected() {
    let fixture = Fixture::new(UploadLimits::new(32, 4096).unwrap()).await;
    let owner = fixture.owner_cookie.clone();
    let page = fixture.create_page(&owner, json!({"title": "Big"})).await;
    let (status, problem) = fixture.upload(&owner, id_of(&page), "big.png", PNG).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE, "{problem}");
    assert_eq!(problem["code"], "upload_too_large");
    assert_eq!(fixture.count("SELECT COUNT(*) FROM page_files").await, 0);
    assert!(fixture.store.blobs().await.unwrap().is_empty());

    let (empty, problem) = fixture
        .call(
            &owner,
            "POST",
            &fixture.files_uri(id_of(&page)),
            Some(json!({})),
        )
        .await;
    assert_eq!(empty, StatusCode::BAD_REQUEST, "{problem}");
    assert_eq!(problem["code"], "invalid_multipart");
}

#[tokio::test]
async fn files_follow_page_visibility_workspace_and_trash() {
    let fixture = Fixture::new(UploadLimits::default()).await;
    let owner = fixture.owner_cookie.clone();
    let member = fixture
        .add_member(&fixture.workspace_id, "member@example.com")
        .await;
    let shared = fixture
        .create_page(&owner, json!({"title": "Shared"}))
        .await;
    let private = fixture
        .create_page(&owner, json!({"private": true, "title": "Diary"}))
        .await;
    let (_, shared_file) = fixture.upload(&owner, id_of(&shared), "s.png", PNG).await;
    let (status, private_file) = fixture.upload(&owner, id_of(&private), "p.png", PNG).await;
    assert_eq!(status, StatusCode::CREATED);
    let shared_url = shared_file["url"].as_str().unwrap();
    let private_url = private_file["url"].as_str().unwrap();

    // Teamspace pages: every member reads and uploads.
    let response = fixture.download(&member, shared_url).await;
    assert_eq!(response.status(), StatusCode::OK);
    let (status, _) = fixture.upload(&member, id_of(&shared), "m.png", PNG).await;
    assert_eq!(status, StatusCode::CREATED);

    // Someone else's private page: 404 for upload and download, and no staged bytes remain.
    let (status, problem) = fixture.upload(&member, id_of(&private), "x.png", PNG).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(problem["code"], "page_file_not_found");
    assert!(fixture.store.temporary_files().await.unwrap().is_empty());
    assert_eq!(
        fixture.download(&member, private_url).await.status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        fixture.download("", private_url).await.status(),
        StatusCode::UNAUTHORIZED
    );

    // A page moved into the owner's private space takes its files along.
    let (_, current) = fixture
        .call(
            &owner,
            "GET",
            &format!("{}/{}", fixture.pages_uri(), id_of(&shared)),
            None,
        )
        .await;
    let (status, _) = fixture
        .call(
            &owner,
            "POST",
            &format!("{}/{}/move", fixture.pages_uri(), id_of(&shared)),
            Some(json!({"expected_version": current["version"], "parent_id": null, "private": true, "position": 0})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        fixture.download(&member, shared_url).await.status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        fixture.download(&owner, shared_url).await.status(),
        StatusCode::OK
    );

    // Another workspace (the owner belongs to both): the file is not reachable through it.
    let other = WorkspaceRepository::new((*fixture.database).clone())
        .create(
            fixture.owner_id,
            "Other".to_owned(),
            "files-test",
            TimestampMillis::now(),
        )
        .await
        .unwrap()
        .id;
    let cross = private_url.replace(&fixture.workspace_id, &other.to_string());
    assert_eq!(
        fixture.download(&owner, &cross).await.status(),
        StatusCode::NOT_FOUND
    );
    let outsider = fixture
        .add_member(&other.to_string(), "outsider@example.com")
        .await;
    assert_eq!(
        fixture.download(&outsider, private_url).await.status(),
        StatusCode::NOT_FOUND
    );
    let (status, _) = fixture
        .upload(&outsider, id_of(&private), "o.png", PNG)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Trashed pages hide their files and refuse uploads until restored.
    fixture.trash(id_of(&private)).await;
    assert_eq!(
        fixture.download(&owner, private_url).await.status(),
        StatusCode::NOT_FOUND
    );
    let (status, _) = fixture.upload(&owner, id_of(&private), "t.png", PNG).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let version: i64 = sqlx::query_scalar("SELECT version FROM pages WHERE id = ?")
        .bind(id_of(&private))
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    let (status, _) = fixture
        .call(
            &owner,
            "POST",
            &format!("{}/{}/restore", fixture.pages_uri(), id_of(&private)),
            Some(json!({"expected_version": version})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        fixture.download(&owner, private_url).await.status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn purged_pages_release_their_blobs_for_reconciliation() {
    let fixture = Fixture::new(UploadLimits::default()).await;
    let owner = fixture.owner_cookie.clone();
    let first = fixture.create_page(&owner, json!({"title": "First"})).await;
    let second = fixture
        .create_page(&owner, json!({"title": "Second"}))
        .await;
    fixture.upload(&owner, id_of(&first), "a.png", PNG).await;
    fixture.upload(&owner, id_of(&second), "b.png", PNG).await;
    fixture
        .upload(&owner, id_of(&first), "only-first.svg", SVG)
        .await;
    assert_eq!(fixture.store.blobs().await.unwrap().len(), 2);

    let workspaces = WorkspaceRepository::new((*fixture.database).clone());
    let expire = |page: &Value| {
        sqlx::query("UPDATE pages SET deleted_at = ? WHERE id = ?")
            .bind(TimestampMillis::now().as_millis() - RETENTION_MILLIS - 1_000)
            .bind(id_of(page).to_owned())
    };
    fixture.trash(id_of(&first)).await;
    expire(&first)
        .execute(fixture.database.pool())
        .await
        .unwrap();
    workspaces
        .purge_retention(TimestampMillis::now())
        .await
        .unwrap();
    assert_eq!(fixture.count("SELECT COUNT(*) FROM page_files").await, 1);

    // The PNG is still used by the second page; the SVG is not and goes after quarantine.
    let later = TimestampMillis::now().as_millis() + 2 * DAY_MILLIS;
    let result = fixture.uploads.reconcile(later).await.unwrap();
    assert_eq!(result.deleted_blobs, 1);
    assert_eq!(fixture.store.blobs().await.unwrap().len(), 1);
    let (_, remaining) = fixture
        .call(
            &owner,
            "GET",
            &format!("{}/{}", fixture.pages_uri(), id_of(&second)),
            None,
        )
        .await;
    assert_eq!(remaining["title"], "Second");

    // Purging the workspace removes the remaining rows and blobs without tripping the
    // page_files -> attachment_blobs RESTRICT.
    sqlx::query("UPDATE workspaces SET deleted_at = ? WHERE id = ?")
        .bind(TimestampMillis::now().as_millis() - RETENTION_MILLIS - 1_000)
        .bind(&fixture.workspace_id)
        .execute(fixture.database.pool())
        .await
        .unwrap();
    let summary = workspaces
        .purge_retention(TimestampMillis::now())
        .await
        .unwrap();
    assert_eq!(summary.workspaces_purged, 1);
    assert_eq!(summary.attachment_blobs_purged, 1);
    assert_eq!(fixture.count("SELECT COUNT(*) FROM page_files").await, 0);
    assert_eq!(
        fixture.count("SELECT COUNT(*) FROM attachment_blobs").await,
        0
    );
    assert_eq!(
        fixture
            .count("SELECT COUNT(*) FROM pragma_foreign_key_check")
            .await,
        0
    );
}

#[tokio::test]
async fn covers_accept_only_files_of_the_same_page() {
    let fixture = Fixture::new(UploadLimits::default()).await;
    let owner = fixture.owner_cookie.clone();
    let page = fixture.create_page(&owner, json!({"title": "Cover"})).await;
    let other = fixture.create_page(&owner, json!({"title": "Other"})).await;
    let (_, file) = fixture.upload(&owner, id_of(&page), "c.png", PNG).await;
    let (_, other_file) = fixture.upload(&owner, id_of(&other), "o.png", PNG).await;
    let url = file["url"].as_str().unwrap().to_owned();
    let page_uri = format!("{}/{}", fixture.pages_uri(), id_of(&page));

    let foreign_workspace = url.replace(&fixture.workspace_id, &Id::new_v7().to_string());
    let missing_file = format!("{}/{}", fixture.files_uri(id_of(&page)), Id::new_v7());
    for bad in [
        other_file["url"].as_str().unwrap().to_owned(),
        missing_file,
        foreign_workspace,
        format!("{url}?download=1"),
        format!("{url}/"),
        url.to_uppercase(),
        "/api/v1/auth/me".to_owned(),
        "javascript:alert(1)".to_owned(),
        "data:image/png;base64,AAAA".to_owned(),
        "//evil.example/x.png".to_owned(),
    ] {
        let (status, problem) = fixture
            .call(
                &owner,
                "PATCH",
                &page_uri,
                Some(json!({"expected_version": 0, "cover_url": bad})),
            )
            .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{bad}: {problem}");
    }
    let (status, saved) = fixture
        .call(
            &owner,
            "PATCH",
            &page_uri,
            Some(json!({"expected_version": 0, "cover_url": url})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["cover_url"], url);
    let (status, saved) = fixture
        .call(
            &owner,
            "PATCH",
            &page_uri,
            Some(json!({"expected_version": 1, "cover_url": "https://example.com/c.png"})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
}

#[tokio::test]
async fn server_side_bytes_go_through_the_same_checks() {
    let fixture = Fixture::new(UploadLimits::new(1024, 4096).unwrap()).await;
    let owner = fixture.owner_cookie.clone();
    let repository = PageFileRepository::new((*fixture.database).clone(), fixture.uploads.clone());
    let workspace_id: Id = fixture.workspace_id.parse().unwrap();
    let page = fixture
        .create_page(&owner, json!({"title": "Import"}))
        .await;
    let page_id: Id = id_of(&page).parse().unwrap();

    let file = repository
        .create_from_bytes(
            workspace_id,
            page_id,
            fixture.owner_id,
            "notion.png",
            PNG,
            "import",
        )
        .await
        .unwrap();
    assert_eq!(file.mime_type, "image/png");
    assert_eq!(file.page_id, page_id);
    let response = fixture.download(&owner, &file.url).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_bytes(response).await, PNG);

    let too_large = repository
        .create_from_bytes(
            workspace_id,
            page_id,
            fixture.owner_id,
            "big.bin",
            &[0; 2048],
            "import",
        )
        .await;
    assert!(matches!(
        too_large,
        Err(PageFileError::Upload(
            orbit_platform::UploadError::FileTooLarge { .. }
        ))
    ));
    let bad_name = repository
        .create_from_bytes(workspace_id, page_id, fixture.owner_id, "", PNG, "import")
        .await;
    assert!(matches!(bad_name, Err(PageFileError::Upload(_))));

    let member = Id::new_v7();
    let private = fixture
        .create_page(&owner, json!({"private": true, "title": "Mine"}))
        .await;
    let denied = repository
        .create_from_bytes(
            workspace_id,
            id_of(&private).parse().unwrap(),
            member,
            "x.png",
            PNG,
            "import",
        )
        .await;
    assert!(matches!(denied, Err(PageFileError::NotFound)));
    assert_eq!(fixture.count("SELECT COUNT(*) FROM page_files").await, 1);
    assert!(fixture.store.temporary_files().await.unwrap().is_empty());
}

#[tokio::test]
async fn duplicates_share_files_and_point_at_their_own_copies() {
    let fixture = Fixture::new(UploadLimits::default()).await;
    let owner = fixture.owner_cookie.clone();
    let parent = fixture
        .create_page(&owner, json!({"title": "Parent"}))
        .await;
    let child = fixture
        .create_page(
            &owner,
            json!({"parent_id": id_of(&parent), "title": "Child"}),
        )
        .await;
    let outside = fixture
        .create_page(&owner, json!({"title": "Outside"}))
        .await;
    let (_, cover) = fixture
        .upload(&owner, id_of(&parent), "cover.png", PNG)
        .await;
    let (_, child_file) = fixture.upload(&owner, id_of(&child), "doc.svg", SVG).await;
    let (_, outside_file) = fixture.upload(&owner, id_of(&outside), "o.png", PNG).await;
    let url = |file: &Value| file["url"].as_str().unwrap().to_owned();
    let page_uri = |page: &Value| format!("{}/{}", fixture.pages_uri(), id_of(page));
    let (status, _) = fixture
        .call(
            &owner,
            "PATCH",
            &page_uri(&parent),
            Some(json!({
                "expected_version": 0,
                "cover_url": url(&cover),
                "content": [
                    {"id": "i", "type": "image", "props": {"url": url(&cover), "name": "cover.png"}, "children": []},
                    {"id": "f", "type": "file", "props": {"url": url(&child_file)}, "children": []},
                    {"id": "o", "type": "image", "props": {"url": url(&outside_file)}, "children": []}
                ]
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = fixture
        .call(
            &owner,
            "PATCH",
            &page_uri(&child),
            Some(json!({
                "expected_version": 0,
                "content": [{"id": "f", "type": "file", "props": {"url": url(&child_file)}, "children": []}]
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let blobs = fixture.store.blobs().await.unwrap().len();

    let (status, copy) = fixture
        .call(
            &owner,
            "POST",
            &format!("{}/duplicate", page_uri(&parent)),
            Some(json!({"include_children": true})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{copy}");
    // New rows for the copied pages' files, the same blobs, no new bytes.
    assert_eq!(fixture.count("SELECT COUNT(*) FROM page_files").await, 5);
    assert_eq!(
        fixture
            .count("SELECT COUNT(DISTINCT blob_id) FROM page_files")
            .await,
        2
    );
    assert_eq!(fixture.store.blobs().await.unwrap().len(), blobs);
    let (child_copy,): (String,) = sqlx::query_as("SELECT id FROM pages WHERE parent_id = ?")
        .bind(id_of(&copy))
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    let copy_cover = copy["cover_url"].as_str().unwrap().to_owned();
    assert!(
        copy_cover.starts_with(&fixture.files_uri(id_of(&copy))),
        "{copy_cover}"
    );
    assert_ne!(copy_cover, url(&cover));
    let content = &copy["content"];
    assert_eq!(content[0]["props"]["url"], copy_cover);
    assert_eq!(content[0]["props"]["name"], "cover.png");
    let copied_child_file = content[1]["props"]["url"].as_str().unwrap().to_owned();
    assert!(copied_child_file.starts_with(&fixture.files_uri(&child_copy)));
    // A file of a page outside the copied set keeps pointing at that page.
    assert_eq!(content[2]["props"]["url"], url(&outside_file));
    let (_, child_page) = fixture
        .call(
            &owner,
            "GET",
            &format!("{}/{child_copy}", fixture.pages_uri()),
            None,
        )
        .await;
    assert_eq!(child_page["content"][0]["props"]["url"], copied_child_file);
    let response = fixture.download(&owner, &copy_cover).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_bytes(response).await, PNG);

    // Deleting the copy forever keeps the originals' files; the blobs stay referenced.
    fixture.trash(id_of(&copy)).await;
    let (status, body) = fixture
        .call(
            &owner,
            "DELETE",
            &format!("{}/permanent?expected_version=1", page_uri(&copy)),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    assert_eq!(fixture.count("SELECT COUNT(*) FROM page_files").await, 3);
    let response = fixture.download(&owner, &url(&cover)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let later = TimestampMillis::now().as_millis() + 2 * DAY_MILLIS;
    assert_eq!(
        fixture
            .uploads
            .reconcile(later)
            .await
            .unwrap()
            .deleted_blobs,
        0
    );
}

#[tokio::test]
async fn delete_forever_releases_page_files_for_reconciliation() {
    let fixture = Fixture::new(UploadLimits::default()).await;
    let owner = fixture.owner_cookie.clone();
    let page = fixture
        .create_page(&owner, json!({"title": "Doomed"}))
        .await;
    let (_, file) = fixture.upload(&owner, id_of(&page), "only.svg", SVG).await;
    fixture.trash(id_of(&page)).await;
    let (status, body) = fixture
        .call(
            &owner,
            "POST",
            &format!("{}/trash/empty", fixture.pages_uri()),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["purged"], 1);
    assert_eq!(fixture.count("SELECT COUNT(*) FROM page_files").await, 0);
    assert_eq!(
        fixture
            .download(&owner, file["url"].as_str().unwrap())
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    // The blob is quarantined for a day, then reclaimed.
    let now = TimestampMillis::now().as_millis();
    assert_eq!(
        fixture.uploads.reconcile(now).await.unwrap().deleted_blobs,
        0
    );
    let later = now + 2 * DAY_MILLIS;
    assert_eq!(
        fixture
            .uploads
            .reconcile(later)
            .await
            .unwrap()
            .deleted_blobs,
        1
    );
    assert!(fixture.store.blobs().await.unwrap().is_empty());
}
