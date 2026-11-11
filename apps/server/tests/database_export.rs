//! Page export of databases (P1 9, P2 9.2): a database page is `<title>.md` with a table of its
//! first view and `<title>.csv` with every row and property; row pages start with their values;
//! a `database` block writes the table under its link. `UPDATE_EXPORT_GOLDENS=1` rewrites the
//! goldens in `tests/fixtures/export/`.

use std::io::{Cursor, Read};
use std::path::Path;
use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use orbit_platform::{
    AttachmentMutationCoordinator, LocalBlobStore, PasswordService, TestDatabase, TimestampMillis,
    UploadLimits, UploadService,
};
use orbit_server::auth_routes::CookieMode;
use orbit_server::database_routes::{DatabaseState, database_router};
use orbit_server::export_routes::{ExportState, export_router};
use orbit_server::page_routes::{PageState, page_router};
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use serde_json::{Value, json};
use tower::ServiceExt;

struct Fixture {
    _root: tempfile::TempDir,
    _database: TestDatabase,
    app: axum::Router,
    workspace_id: String,
    cookie: String,
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
        let uploads = UploadService::new(
            (*database).clone(),
            Arc::new(LocalBlobStore::new(root.path().join("attachments"))),
            AttachmentMutationCoordinator::default(),
            UploadLimits::default(),
        );
        let app = page_router(PageState::new(Arc::clone(&identity), CookieMode::secure()))
            .merge(database_router(DatabaseState::new(
                Arc::clone(&identity),
                CookieMode::secure(),
            )))
            .merge(export_router(ExportState::new(
                Arc::clone(&identity),
                uploads,
                CookieMode::secure(),
                "https://orbit.example/".to_owned(),
            )));
        Self {
            _root: root,
            _database: database,
            app,
            workspace_id: setup.workspace_id.to_string(),
            cookie: format!("__Host-orbit_session={}", setup.session.token),
        }
    }

    async fn ok(&self, method: &str, path: &str, body: Option<Value>) -> Value {
        let builder = Request::builder()
            .method(method)
            .uri(format!("/api/v1/workspaces/{}/{path}", self.workspace_id))
            .header(header::COOKIE, &self.cookie);
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
        let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        assert!(status.is_success(), "{method} {path}: {status} {value}");
        value
    }

    async fn export(&self, page_id: &str, children: bool) -> Vec<(String, String)> {
        let request = Request::builder()
            .uri(format!(
                "/api/v1/workspaces/{}/pages/{page_id}/export?format=markdown&children={children}",
                self.workspace_id
            ))
            .header(header::COOKIE, &self.cookie)
            .body(Body::empty())
            .unwrap();
        let response = self.app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes.to_vec())).unwrap();
        (0..archive.len())
            .map(|index| {
                let mut entry = archive.by_index(index).unwrap();
                let mut text = String::new();
                entry.read_to_string(&mut text).unwrap();
                (entry.name().unwrap().into_owned(), text)
            })
            .collect()
    }
}

fn check_golden(name: &str, actual: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/export")
        .join(name);
    if std::env::var_os("UPDATE_EXPORT_GOLDENS").is_some() {
        std::fs::write(&path, actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_else(|_| {
        panic!(
            "missing golden {}; run with UPDATE_EXPORT_GOLDENS=1",
            path.display()
        )
    });
    assert_eq!(actual, expected, "golden {name} differs");
}

/// The text with the export time and the given volatile strings replaced by stable names.
fn normalize(text: &str, replacements: &[(String, String)]) -> String {
    let mut text: String = text
        .lines()
        .map(|line| {
            if line.starts_with("exported_at: ") {
                "exported_at: \"<now>\""
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    if !text.ends_with('\n') {
        text.push('\n');
    }
    for (from, to) in replacements {
        text = text.replace(from.as_str(), to);
    }
    text
}

fn file<'a>(files: &'a [(String, String)], name: &str) -> &'a str {
    &files
        .iter()
        .find(|(path, _)| path == name)
        .unwrap_or_else(|| {
            panic!(
                "{name} in {:?}",
                files.iter().map(|f| &f.0).collect::<Vec<_>>()
            )
        })
        .1
}

#[tokio::test]
async fn databases_export_as_markdown_tables_csv_and_row_property_lists() {
    let fixture = Fixture::new().await;
    let page = fixture
        .ok(
            "POST",
            "pages",
            Some(json!({"kind": "database", "title": "Tasks | 2026"})),
        )
        .await;
    let id = page["id"].as_str().unwrap().to_owned();
    let property = |name: &str, kind: &str, config: Value| {
        let fixture = &fixture;
        let id = id.clone();
        let mut body = json!({"name": name, "type": kind});
        if !config.is_null() {
            body["config"] = config;
        }
        async move {
            fixture
                .ok("POST", &format!("databases/{id}/properties"), Some(body))
                .await
        }
    };
    let notes = property("Notes", "text", Value::Null).await;
    let score = property("Score", "number", json!({"format": "percent"})).await;
    let kind = property(
        "Kind",
        "select",
        json!({"options": [{"name": "Bug"}, {"name": "=Feature"}]}),
    )
    .await;
    let status = property("Status", "status", Value::Null).await;
    let due = property("Due", "date", Value::Null).await;
    let owner = property("Owner", "person", Value::Null).await;
    let done = property("Done", "checkbox", Value::Null).await;
    let link = property("Link", "url", Value::Null).await;
    property("Created", "created_time", Value::Null).await;
    property("Created by", "created_by", Value::Null).await;
    let detail = fixture.ok("GET", &format!("databases/{id}"), None).await;
    let tags = detail["properties"]
        .as_array()
        .unwrap()
        .iter()
        .find(|property| property["name"] == "Tags")
        .unwrap()
        .clone();

    // The first view hides "Notes", so the table leaves it out; the CSV has every property.
    let view = &detail["views"][0];
    let mut config = view["config"].clone();
    for column in config["columns"].as_array_mut().unwrap() {
        if column["property_id"] == notes["id"] {
            column["hidden"] = json!(true);
        }
    }
    fixture
        .ok(
            "PATCH",
            &format!("databases/{id}/views/{}", view["id"].as_str().unwrap()),
            Some(json!({"config": config})),
        )
        .await;

    let option =
        |property: &Value, index: usize| property["config"]["options"][index]["id"].clone();
    let first = fixture
        .ok(
            "POST",
            &format!("databases/{id}/rows"),
            Some(json!({"title": "Fix | login", "values": {
                notes["id"].as_str().unwrap(): "=HYPERLINK(\"x\")\nsecond line",
                score["id"].as_str().unwrap(): -0.125,
                kind["id"].as_str().unwrap(): option(&kind, 1),
                status["id"].as_str().unwrap(): option(&status, 2),
                due["id"].as_str().unwrap(): {"start": "2026-11-09", "end": "2026-11-12"},
                done["id"].as_str().unwrap(): true,
                link["id"].as_str().unwrap(): "https://example.com/a?b=c,d",
            }})),
        )
        .await;
    let session = fixture.ok("GET", &format!("databases/{id}"), None).await;
    let creator = session["rows"][0]["created_by"].clone();
    fixture
        .ok(
            "PATCH",
            &format!(
                "databases/{id}/rows/{}/values",
                first["id"].as_str().unwrap()
            ),
            Some(json!({owner["id"].as_str().unwrap(): [creator]})),
        )
        .await;
    let second = fixture
        .ok(
            "POST",
            &format!("databases/{id}/rows"),
            Some(json!({"title": "", "values": {
                tags["id"].as_str().unwrap(): [],
            }})),
        )
        .await;

    let host = fixture
        .ok("POST", "pages", Some(json!({"title": "Weekly notes"})))
        .await;
    let host_id = host["id"].as_str().unwrap().to_owned();
    fixture
        .ok(
            "PATCH",
            &format!("pages/{host_id}"),
            Some(json!({"expected_version": host["version"], "content": [
                {"type": "paragraph", "content": [{"type": "text", "text": "Open work:", "styles": {}}]},
                {"type": "database", "props": {"databaseId": id, "linkId": ""}},
            ]})),
        )
        .await;

    let detail = fixture.ok("GET", &format!("databases/{id}"), None).await;
    let mut replacements = vec![(id.clone(), "<database>".to_owned())];
    for (index, row) in detail["rows"].as_array().unwrap().iter().enumerate() {
        replacements.push((
            row["id"].as_str().unwrap().to_owned(),
            format!("<row-{index}>"),
        ));
        for field in ["created_at", "updated_at"] {
            replacements.push((row[field].as_str().unwrap().to_owned(), "<time>".to_owned()));
        }
    }
    assert_eq!(second["title"], "");

    let files = fixture.export(&id, true).await;
    let mut names: Vec<&str> = files.iter().map(|(name, _)| name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            "Tasks - 2026.csv",
            "Tasks - 2026.md",
            "Tasks - 2026/Fix - login.md",
            "Tasks - 2026/Untitled.md",
        ]
    );
    let csv = file(&files, "Tasks - 2026.csv");
    assert!(csv.starts_with('\u{feff}') && csv.contains("\r\n"));
    check_golden("database.csv", &normalize(csv, &replacements));
    check_golden(
        "database_page.md",
        &normalize(file(&files, "Tasks - 2026.md"), &replacements),
    );
    check_golden(
        "database_row.md",
        &normalize(file(&files, "Tasks - 2026/Fix - login.md"), &replacements),
    );

    let files = fixture.export(&host_id, false).await;
    check_golden(
        "database_block.md",
        &normalize(file(&files, "Weekly notes.md"), &replacements),
    );
}
