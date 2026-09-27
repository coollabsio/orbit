use std::io::{Cursor, Read};
use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use orbit_platform::{
    AttachmentMutationCoordinator, AuthenticatedUser, Id, LocalBlobStore, PasswordService,
    TestDatabase, TimestampMillis, UploadLimits, UploadService,
};
use orbit_server::auth_routes::CookieMode;
use orbit_server::export::ExportLimits;
use orbit_server::export_routes::{ExportState, export_router};
use orbit_server::page_file_routes::{PageFileState, page_file_router};
use orbit_server::page_routes::{PageState, page_router};
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::repositories::workspaces::WorkspaceRepository;
use serde_json::{Value, json};
use tower::ServiceExt;

const PNG: &[u8] = b"\x89\x50\x4e\x47\x0d\x0a\x1a\x0a\x00\x00\x00\x0d\x49\x48\x44\x52\x00\x00\x00\x01\x00\x00\x00\x01\x08\x04\x00\x00\x00\xb5\x1c\x0c\x02\x00\x00\x00\x0b\x49\x44\x41\x54\x78\xda\x63\x64\xf8\x0f\x00\x01\x05\x01\x01\x27\x18\xe3\x66\x00\x00\x00\x00\x49\x45\x4e\x44\xae\x42\x60\x82";
const ORIGIN: &str = "https://orbit.example";

struct Fixture {
    _root: tempfile::TempDir,
    database: TestDatabase,
    identity: Arc<IdentityRepository>,
    app: axum::Router,
    workspace_id: String,
    owner_id: Id,
    owner: String,
}

impl Fixture {
    async fn new() -> Self {
        Self::with_limits(ExportLimits::default()).await
    }

    async fn with_limits(limits: ExportLimits) -> Self {
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
            UploadLimits::default(),
        );
        let app = page_router(PageState::new(Arc::clone(&identity), CookieMode::secure()))
            .merge(page_file_router(PageFileState::new(
                Arc::clone(&identity),
                uploads.clone(),
                CookieMode::secure(),
            )))
            .merge(export_router(
                ExportState::new(
                    Arc::clone(&identity),
                    uploads,
                    CookieMode::secure(),
                    format!("{ORIGIN}/"),
                )
                .with_limits(limits),
            ));
        Self {
            _root: root,
            database,
            identity,
            app,
            workspace_id: setup.workspace_id.to_string(),
            owner_id: setup.user_id,
            owner: format!("__Host-orbit_session={}", setup.session.token),
        }
    }

    fn pages_uri(&self) -> String {
        format!("/api/v1/workspaces/{}/pages", self.workspace_id)
    }

    fn export_uri(&self, page_id: &str, children: bool) -> String {
        format!(
            "{}/{page_id}/export?format=markdown&children={children}",
            self.pages_uri()
        )
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
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn create(&self, cookie: &str, body: Value) -> String {
        let (status, page) = self
            .call(cookie, "POST", &self.pages_uri(), Some(body))
            .await;
        assert_eq!(status, StatusCode::CREATED, "{page}");
        page["id"].as_str().unwrap().to_owned()
    }

    async fn set_content(&self, cookie: &str, page_id: &str, content: Value) {
        let uri = format!("{}/{page_id}", self.pages_uri());
        let (_, page) = self.call(cookie, "GET", &uri, None).await;
        let (status, body) = self
            .call(
                cookie,
                "PATCH",
                &uri,
                Some(json!({"expected_version": page["version"], "content": content})),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }

    async fn trash(&self, cookie: &str, page_id: &str) {
        let uri = format!("{}/{page_id}", self.pages_uri());
        let (_, page) = self.call(cookie, "GET", &uri, None).await;
        let (status, body) = self
            .call(
                cookie,
                "DELETE",
                &format!("{uri}?expected_version={}", page["version"]),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    }

    async fn upload(&self, cookie: &str, page_id: &str, name: &str, bytes: &[u8]) -> String {
        let boundary = "orbit-test-boundary";
        let mut body = Vec::new();
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        body.extend_from_slice(
            format!("Content-Disposition: form-data; name=\"file\"; filename=\"{name}\"\r\n\r\n")
                .as_bytes(),
        );
        body.extend_from_slice(bytes);
        body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
        let request = Request::builder()
            .method("POST")
            .uri(format!("{}/{page_id}/files", self.pages_uri()))
            .header(
                header::CONTENT_TYPE,
                format!("multipart/form-data; boundary={boundary}"),
            )
            .header(header::COOKIE, cookie)
            .body(Body::from(body))
            .unwrap();
        let response = self.app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let file: Value = serde_json::from_slice(&bytes).unwrap();
        file["url"].as_str().unwrap().to_owned()
    }

    async fn export(&self, cookie: &str, page_id: &str, children: bool) -> Export {
        let request = Request::builder()
            .uri(self.export_uri(page_id, children))
            .header(header::COOKIE, cookie)
            .body(Body::empty())
            .unwrap();
        let response = self.app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(
            status,
            StatusCode::OK,
            "{}",
            String::from_utf8_lossy(&bytes)
        );
        assert_eq!(headers[header::CONTENT_TYPE], "application/zip");
        assert_eq!(
            headers[header::CONTENT_LENGTH],
            bytes.len().to_string().as_str()
        );
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes.to_vec())).unwrap();
        let mut files = Vec::new();
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).unwrap();
            let mut content = Vec::new();
            entry.read_to_end(&mut content).unwrap();
            files.push((entry.name().to_owned(), content));
        }
        Export {
            disposition: headers[header::CONTENT_DISPOSITION]
                .to_str()
                .unwrap()
                .to_owned(),
            files,
        }
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

struct Export {
    disposition: String,
    files: Vec<(String, Vec<u8>)>,
}

impl Export {
    fn names(&self) -> Vec<&str> {
        self.files.iter().map(|(name, _)| name.as_str()).collect()
    }

    fn text(&self, name: &str) -> String {
        let (_, bytes) = self
            .files
            .iter()
            .find(|(entry, _)| entry == name)
            .unwrap_or_else(|| panic!("{name} not in {:?}", self.names()));
        String::from_utf8(bytes.clone()).unwrap()
    }

    fn bytes(&self, name: &str) -> &[u8] {
        &self
            .files
            .iter()
            .find(|(entry, _)| entry == name)
            .unwrap()
            .1
    }
}

fn paragraph_link(href: &str, text: &str) -> Value {
    json!({"type": "paragraph", "content": [{"type": "link", "href": href, "content": [{"type": "text", "text": text, "styles": {}}]}]})
}

#[tokio::test]
async fn exports_a_subtree_as_markdown_folders_with_assets_and_relative_links() {
    let fixture = Fixture::new().await;
    let owner = fixture.owner.clone();
    let root = fixture
        .create(&owner, json!({"title": "Plan: Q1/Q2"}))
        .await;
    let notes = fixture
        .create(&owner, json!({"title": "Notes", "parent_id": root}))
        .await;
    let notes_again = fixture
        .create(&owner, json!({"title": "notes", "parent_id": root}))
        .await;
    let assets_page = fixture
        .create(&owner, json!({"title": "assets", "parent_id": root}))
        .await;
    let deep = fixture
        .create(
            &owner,
            json!({"title": "../../etc/passwd", "parent_id": notes}),
        )
        .await;
    let trashed = fixture
        .create(&owner, json!({"title": "Old", "parent_id": root}))
        .await;
    let outside = fixture.create(&owner, json!({"title": "Elsewhere"})).await;
    fixture.trash(&owner, &trashed).await;

    let image = fixture.upload(&owner, &root, "shot one.png", PNG).await;
    let other_file = fixture.upload(&owner, &outside, "x.png", PNG).await;
    let unused = fixture.upload(&owner, &root, "unused.png", PNG).await;
    assert_ne!(image, unused);
    fixture
        .set_content(
            &owner,
            &root,
            json!([
                {"type": "heading", "props": {"level": 2}, "content": "Intro"},
                {"type": "image", "props": {"url": image, "name": "shot one.png"}},
                {"type": "file", "props": {"url": image, "name": "again"}},
                {"type": "page", "props": {"pageId": notes}},
                {"type": "page", "props": {"pageId": outside}},
                {"type": "page", "props": {"pageId": trashed}},
                paragraph_link(&format!("/docs/{deep}"), "deep"),
                paragraph_link(&format!("/docs/{outside}"), "out"),
                paragraph_link(&other_file, "their file"),
                paragraph_link("https://example.com/x", "web"),
            ]),
        )
        .await;
    fixture
        .set_content(&owner, &deep, json!([paragraph_link(&format!("/docs/{root}"), "up"), {"type": "image", "props": {"url": image}}]))
        .await;

    let export = fixture.export(&owner, &root, true).await;
    assert_eq!(
        export.names(),
        vec![
            "Plan- Q1-Q2.md",
            "Plan- Q1-Q2/Notes.md",
            "Plan- Q1-Q2/Notes/-..-etc-passwd.md",
            "Plan- Q1-Q2/notes (2).md",
            "Plan- Q1-Q2/assets (2).md",
            "Plan- Q1-Q2/assets/shot one.png",
        ]
    );
    for name in export.names() {
        assert!(!name.starts_with('/'));
        assert!(
            name.split('/')
                .all(|segment| segment != ".." && segment != "." && !segment.is_empty())
        );
    }
    assert!(
        export.disposition.contains("filename=\"Plan- Q1-Q2.zip\""),
        "{}",
        export.disposition
    );
    assert_eq!(export.bytes("Plan- Q1-Q2/assets/shot one.png"), PNG);

    let root_md = export.text("Plan- Q1-Q2.md");
    assert!(
        root_md.starts_with("---\ntitle: \"Plan: Q1/Q2\"\nexported_at: \""),
        "{root_md}"
    );
    assert!(
        root_md.contains("\n# Plan: Q1/Q2\n\n## Intro\n"),
        "{root_md}"
    );
    assert!(
        root_md.contains("![shot one.png](Plan-%20Q1-Q2/assets/shot%20one.png)"),
        "{root_md}"
    );
    assert!(
        root_md.contains("[again](Plan-%20Q1-Q2/assets/shot%20one.png)"),
        "{root_md}"
    );
    assert!(
        root_md.contains("[Notes](Plan-%20Q1-Q2/Notes.md)"),
        "{root_md}"
    );
    assert!(
        root_md.contains(&format!("[Elsewhere]({ORIGIN}/docs/{outside})")),
        "{root_md}"
    );
    assert!(root_md.contains("*Missing page*"), "{root_md}");
    assert!(
        root_md.contains("[deep](Plan-%20Q1-Q2/Notes/-..-etc-passwd.md)"),
        "{root_md}"
    );
    assert!(
        root_md.contains(&format!("[out]({ORIGIN}/docs/{outside})")),
        "{root_md}"
    );
    assert!(
        root_md.contains(&format!("[their file]({ORIGIN}{other_file})")),
        "{root_md}"
    );
    assert!(
        root_md.contains("[web](https://example.com/x)"),
        "{root_md}"
    );
    let deep_md = export.text("Plan- Q1-Q2/Notes/-..-etc-passwd.md");
    assert!(
        deep_md.contains("[up](../../Plan-%20Q1-Q2.md)"),
        "{deep_md}"
    );
    assert!(
        deep_md.contains("![](../assets/shot%20one.png)"),
        "{deep_md}"
    );
    let _ = (notes_again, assets_page);

    // Without children: one page, links to sub-pages leave the export.
    let single = fixture.export(&owner, &root, false).await;
    assert_eq!(
        single.names(),
        vec!["Plan- Q1-Q2.md", "Plan- Q1-Q2/assets/shot one.png"]
    );
    let single_md = single.text("Plan- Q1-Q2.md");
    assert!(
        single_md.contains(&format!("[Notes]({ORIGIN}/docs/{notes})")),
        "{single_md}"
    );
    assert!(
        single_md.contains(&format!("[deep]({ORIGIN}/docs/{deep})")),
        "{single_md}"
    );
}

#[tokio::test]
async fn download_names_are_ascii_with_an_utf8_variant() {
    let fixture = Fixture::new().await;
    let page = fixture
        .create(&fixture.owner, json!({"title": "Café \"notes\" 🚀"}))
        .await;
    let export = fixture.export(&fixture.owner, &page, false).await;
    assert_eq!(
        export.disposition,
        "attachment; filename=\"Caf_ -notes- _.zip\"; filename*=UTF-8''Caf%C3%A9%20-notes-%20%F0%9F%9A%80.zip"
    );
    assert_eq!(export.names(), vec!["Café -notes- 🚀.md"]);
    let untitled = fixture.create(&fixture.owner, json!({"title": "  "})).await;
    let export = fixture.export(&fixture.owner, &untitled, false).await;
    assert_eq!(export.names(), vec!["Untitled.md"]);
    assert!(export.text("Untitled.md").contains("\n# Untitled\n"));
}

#[tokio::test]
async fn exports_follow_page_visibility() {
    let fixture = Fixture::new().await;
    let owner = fixture.owner.clone();
    let member = fixture
        .add_member(&fixture.workspace_id, "member@example.com")
        .await;
    let private = fixture
        .create(&owner, json!({"title": "Secret", "private": true}))
        .await;
    let private_child = fixture
        .create(
            &owner,
            json!({"title": "Secret child", "parent_id": private}),
        )
        .await;
    let shared = fixture.create(&owner, json!({"title": "Shared"})).await;
    let shared_child = fixture
        .create(
            &owner,
            json!({"title": "Shared child", "parent_id": shared}),
        )
        .await;
    fixture
        .set_content(
            &owner,
            &shared,
            json!([{"type": "page", "props": {"pageId": private}}]),
        )
        .await;

    // The owner exports their private subtree.
    let export = fixture.export(&owner, &private, true).await;
    assert_eq!(export.names(), vec!["Secret.md", "Secret/Secret child.md"]);

    // A member gets the same 404 as for an unknown page, with or without children.
    let unknown = Id::new_v7().to_string();
    for (page, children) in [
        (&private, false),
        (&private, true),
        (&private_child, true),
        (&unknown, false),
    ] {
        let (status, body) = fixture
            .call(&member, "GET", &fixture.export_uri(page, children), None)
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
        assert_eq!(body["code"], "page_not_found");
    }
    // A teamspace export never names another member's private page.
    let export = fixture.export(&member, &shared, true).await;
    assert_eq!(export.names(), vec!["Shared.md", "Shared/Shared child.md"]);
    let text = export.text("Shared.md");
    assert!(
        text.contains("*Missing page*") && !text.contains("Secret"),
        "{text}"
    );
    let _ = shared_child;

    // Trashed pages, other workspaces, outsiders and anonymous callers.
    let trashed = fixture.create(&owner, json!({"title": "Gone"})).await;
    fixture.trash(&owner, &trashed).await;
    let (status, _) = fixture
        .call(&owner, "GET", &fixture.export_uri(&trashed, false), None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let other = WorkspaceRepository::new((*fixture.database).clone())
        .create(
            fixture.owner_id,
            "Other".to_owned(),
            "export-test",
            TimestampMillis::now(),
        )
        .await
        .unwrap()
        .id
        .to_string();
    let outsider = fixture.add_member(&other, "outsider@example.com").await;
    let (status, _) = fixture
        .call(&outsider, "GET", &fixture.export_uri(&shared, false), None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = fixture
        .call(
            &owner,
            "GET",
            &format!("/api/v1/workspaces/{other}/pages/{shared}/export?format=markdown"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = fixture
        .call("", "GET", &fixture.export_uri(&shared, false), None)
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = fixture
        .call(
            &owner,
            "GET",
            &format!("{}/{shared}/export?format=pdf", fixture.pages_uri()),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn exports_over_the_limits_are_refused() {
    let fixture = Fixture::with_limits(ExportLimits {
        max_pages: 2,
        max_bytes: 400,
    })
    .await;
    let owner = fixture.owner.clone();
    let root = fixture.create(&owner, json!({"title": "Root"})).await;
    fixture
        .create(&owner, json!({"title": "One", "parent_id": root}))
        .await;
    fixture
        .create(&owner, json!({"title": "Two", "parent_id": root}))
        .await;
    let (status, body) = fixture
        .call(&owner, "GET", &fixture.export_uri(&root, true), None)
        .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(body["code"], "export_too_many_pages");
    // The page alone fits.
    fixture.export(&owner, &root, false).await;

    let image = fixture
        .upload(&owner, &root, "big.png", &[PNG, &[0_u8; 512]].concat())
        .await;
    fixture
        .set_content(
            &owner,
            &root,
            json!([{"type": "image", "props": {"url": image}}]),
        )
        .await;
    let (status, body) = fixture
        .call(&owner, "GET", &fixture.export_uri(&root, false), None)
        .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(body["code"], "export_too_large");
}
