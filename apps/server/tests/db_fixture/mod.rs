//! A workspace with an owner, served by the page, page file and database routes, for the P3
//! database item tests (`database_relations`, `database_fields`, `database_calendar`).
#![allow(dead_code)]

use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use orbit_platform::{
    AttachmentMutationCoordinator, AuthenticatedUser, Id, LocalBlobStore, PasswordService,
    TestDatabase, TimestampMillis, UploadLimits, UploadService,
};
use orbit_server::auth_routes::CookieMode;
use orbit_server::database_routes::{DatabaseState, database_router};
use orbit_server::page_file_routes::{PageFileState, page_file_router};
use orbit_server::page_routes::{PageState, page_router};
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use serde_json::{Value, json};
use tower::ServiceExt;

pub const PNG: &[u8] = b"\x89\x50\x4e\x47\x0d\x0a\x1a\x0a\x00\x00\x00\x0d\x49\x48\x44\x52\x00\x00\x00\x01\x00\x00\x00\x01\x08\x04\x00\x00\x00\xb5\x1c\x0c\x02\x00\x00\x00\x0b\x49\x44\x41\x54\x78\xda\x63\x64\xf8\x0f\x00\x01\x05\x01\x01\x27\x18\xe3\x66\x00\x00\x00\x00\x49\x45\x4e\x44\xae\x42\x60\x82";

pub struct Fixture {
    _root: tempfile::TempDir,
    pub database: TestDatabase,
    identity: Arc<IdentityRepository>,
    app: axum::Router,
    pub workspace_id: String,
    pub cookie: String,
    pub owner_id: Id,
}

impl Fixture {
    pub async fn new() -> Self {
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
            .merge(page_file_router(PageFileState::new(
                Arc::clone(&identity),
                uploads,
                CookieMode::secure(),
            )));
        Self {
            _root: root,
            database,
            identity,
            app,
            workspace_id: setup.workspace_id.to_string(),
            cookie: format!("__Host-orbit_session={}", setup.session.token),
            owner_id: setup.user_id,
        }
    }

    pub fn uri(&self, path: &str) -> String {
        format!("/api/v1/workspaces/{}/{path}", self.workspace_id)
    }

    pub async fn call(&self, method: &str, path: &str, body: Option<Value>) -> (StatusCode, Value) {
        self.call_as(&self.cookie, method, path, body).await
    }

    pub async fn call_as(
        &self,
        cookie: &str,
        method: &str,
        path: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let builder = Request::builder()
            .method(method)
            .uri(self.uri(path))
            .header(header::COOKIE, cookie);
        let request = match body {
            Some(value) => builder
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(value.to_string())),
            None => builder.body(Body::empty()),
        }
        .unwrap();
        self.send(request).await
    }

    async fn send(&self, request: Request<Body>) -> (StatusCode, Value) {
        let response = self.app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or_else(|_| json!(bytes.len()))
        };
        (status, value)
    }

    pub async fn ok(&self, method: &str, path: &str, body: Option<Value>) -> Value {
        self.ok_as(&self.cookie, method, path, body).await
    }

    pub async fn ok_as(
        &self,
        cookie: &str,
        method: &str,
        path: &str,
        body: Option<Value>,
    ) -> Value {
        let (status, value) = self.call_as(cookie, method, path, body).await;
        assert!(status.is_success(), "{method} {path}: {status} {value}");
        value
    }

    /// A new teamspace database; returns its id.
    pub async fn create_database(&self, title: &str) -> String {
        self.create_database_as(&self.cookie, json!({"kind": "database", "title": title}))
            .await
    }

    pub async fn create_database_as(&self, cookie: &str, body: Value) -> String {
        id_of(&self.ok_as(cookie, "POST", "pages", Some(body)).await).to_owned()
    }

    pub async fn database(&self, id: &str) -> Value {
        self.ok("GET", &format!("databases/{id}"), None).await
    }

    pub async fn database_as(&self, cookie: &str, id: &str) -> Value {
        self.ok_as(cookie, "GET", &format!("databases/{id}"), None)
            .await
    }

    /// Adds a property; returns it.
    pub async fn add_property(&self, database: &str, body: Value) -> Value {
        self.add_property_as(&self.cookie, database, body).await
    }

    pub async fn add_property_as(&self, cookie: &str, database: &str, body: Value) -> Value {
        self.ok_as(
            cookie,
            "POST",
            &format!("databases/{database}/properties"),
            Some(body),
        )
        .await
    }

    /// Adds a row titled `title`; returns its id.
    pub async fn add_row(&self, database: &str, title: &str) -> String {
        self.add_row_with(database, json!({"title": title})).await
    }

    pub async fn add_row_with(&self, database: &str, body: Value) -> String {
        id_of(
            &self
                .ok("POST", &format!("databases/{database}/rows"), Some(body))
                .await,
        )
        .to_owned()
    }

    pub async fn set_values(
        &self,
        database: &str,
        row: &str,
        values: Value,
    ) -> (StatusCode, Value) {
        self.set_values_as(&self.cookie, database, row, values)
            .await
    }

    pub async fn set_values_as(
        &self,
        cookie: &str,
        database: &str,
        row: &str,
        values: Value,
    ) -> (StatusCode, Value) {
        self.call_as(
            cookie,
            "PATCH",
            &format!("databases/{database}/rows/{row}/values"),
            Some(values),
        )
        .await
    }

    pub async fn page(&self, id: &str) -> Value {
        self.ok("GET", &format!("pages/{id}"), None).await
    }

    pub async fn trash_page(&self, id: &str) {
        let version = self.page(id).await["version"].clone();
        self.ok(
            "DELETE",
            &format!("pages/{id}?expected_version={version}"),
            None,
        )
        .await;
    }

    pub async fn restore_page(&self, id: &str) {
        let (_, trash) = self.call("GET", "pages/trash", None).await;
        let version = trash["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["id"] == id)
            .expect("in the trash")["version"]
            .clone();
        self.ok(
            "POST",
            &format!("pages/{id}/restore"),
            Some(json!({"expected_version": version})),
        )
        .await;
    }

    /// Trashes the page, then deletes it forever.
    pub async fn purge_page(&self, id: &str) {
        self.trash_page(id).await;
        let (_, trash) = self.call("GET", "pages/trash", None).await;
        let version = trash["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["id"] == id)
            .expect("in the trash")["version"]
            .clone();
        self.ok(
            "DELETE",
            &format!("pages/{id}/permanent?expected_version={version}"),
            None,
        )
        .await;
    }

    /// Uploads `bytes` as a file of the page; returns the file id.
    pub async fn upload(&self, page_id: &str, name: &str, bytes: &[u8]) -> String {
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
            .uri(self.uri(&format!("pages/{page_id}/files")))
            .header(header::COOKIE, &self.cookie)
            .header(
                header::CONTENT_TYPE,
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(Body::from(body))
            .unwrap();
        let (status, file) = self.send(request).await;
        assert!(status.is_success(), "upload: {status} {file}");
        id_of(&file).to_owned()
    }

    /// A workspace member; returns its id and session cookie.
    pub async fn add_member(&self, email: &str) -> (Id, String) {
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
                    email: email.to_owned(),
                    display_name: "Member".to_owned(),
                },
                now,
            )
            .await
            .unwrap();
        (id, format!("__Host-orbit_session={}", session.token))
    }

    /// The audit actions recorded on the database `id`, oldest first.
    pub async fn audit_actions(&self, id: &str) -> Vec<String> {
        sqlx::query_scalar(
            "SELECT action FROM audit_events WHERE workspace_id = $1 AND resource_type = 'database' \
             AND resource_id = $2 ORDER BY occurred_at, id",
        )
        .bind(&self.workspace_id)
        .bind(id)
        .fetch_all(self.database.pool())
        .await
        .unwrap()
    }
}

pub fn id_of(value: &Value) -> &str {
    value["id"]
        .as_str()
        .unwrap_or_else(|| panic!("no id in {value}"))
}

pub fn property<'a>(database: &'a Value, name: &str) -> &'a Value {
    database["properties"]
        .as_array()
        .unwrap()
        .iter()
        .find(|property| property["name"] == name)
        .unwrap_or_else(|| panic!("no property {name}"))
}

/// The row `row` of a database detail.
pub fn row<'a>(database: &'a Value, row: &str) -> &'a Value {
    database["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|candidate| candidate["id"] == row)
        .unwrap_or_else(|| panic!("no row {row}"))
}

/// The value of `property` (an id) in row `row` of a database detail.
pub fn value(database: &Value, row_id: &str, property: &str) -> Value {
    row(database, row_id)["values"][property].clone()
}

pub fn titles(database: &Value) -> Vec<String> {
    database["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["title"].as_str().unwrap().to_owned())
        .collect()
}
