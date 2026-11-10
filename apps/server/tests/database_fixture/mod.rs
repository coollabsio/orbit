//! The HTTP fixture of the database API tests: a workspace with an owner, the page and database
//! routers, and helpers to build databases.
#![allow(dead_code)]

use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use orbit_platform::{AuthenticatedUser, Id, PasswordService, TestDatabase, TimestampMillis};
use orbit_server::auth_routes::CookieMode;
use orbit_server::database_routes::{DatabaseState, database_router};
use orbit_server::page_routes::{PageState, page_router};
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use serde_json::{Value, json};

use tower::ServiceExt;

pub struct Fixture {
    pub database: TestDatabase,
    pub identity: Arc<IdentityRepository>,
    pub app: axum::Router,
    pub workspace_id: String,
    pub cookie: String,
    pub owner_id: Id,
}

impl Fixture {
    pub async fn new() -> Self {
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
        let app = page_router(PageState::new(Arc::clone(&identity), CookieMode::secure())).merge(
            database_router(DatabaseState::new(
                Arc::clone(&identity),
                CookieMode::secure(),
            )),
        );
        Self {
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
        let response = self.app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        };
        (status, value)
    }

    pub async fn ok(&self, method: &str, path: &str, body: Option<Value>) -> Value {
        let (status, value) = self.call(method, path, body).await;
        assert!(status.is_success(), "{method} {path}: {status} {value}");
        value
    }

    pub async fn create_page(&self, body: Value) -> Value {
        self.ok("POST", "pages", Some(body)).await
    }

    /// A new database; returns its id.
    pub async fn create_database(&self, title: &str) -> String {
        let page = self
            .create_page(json!({"kind": "database", "title": title}))
            .await;
        id_of(&page).to_owned()
    }

    pub async fn database(&self, id: &str) -> Value {
        self.ok("GET", &format!("databases/{id}"), None).await
    }

    pub async fn add_property(&self, database: &str, body: Value) -> Value {
        self.ok(
            "POST",
            &format!("databases/{database}/properties"),
            Some(body),
        )
        .await
    }

    pub async fn add_row(&self, database: &str, body: Value) -> Value {
        self.ok("POST", &format!("databases/{database}/rows"), Some(body))
            .await
    }

    pub async fn set_values(
        &self,
        database: &str,
        row: &str,
        values: Value,
    ) -> (StatusCode, Value) {
        self.call(
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
        let version = self.page(id).await["version"].as_u64().unwrap();
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
}

impl Fixture {
    /// A new property of `database`; returns its id.
    pub async fn property_id(
        &self,
        database: &str,
        name: &str,
        kind: &str,
        config: Option<Value>,
    ) -> String {
        id_of(
            &self
                .add_property(
                    database,
                    json!({"name": name, "type": kind, "config": config}),
                )
                .await,
        )
        .to_owned()
    }

    pub async fn add_view(
        &self,
        database: &str,
        name: &str,
        kind: &str,
        config: Option<Value>,
    ) -> Value {
        self.ok(
            "POST",
            &format!("databases/{database}/views"),
            Some(json!({"name": name, "type": kind, "config": config})),
        )
        .await
    }

    pub async fn set_content(&self, page: &str, content: Value) {
        let version = self.page(page).await["version"].as_u64().unwrap();
        self.ok(
            "PATCH",
            &format!("pages/{page}"),
            Some(json!({"expected_version": version, "content": content})),
        )
        .await;
    }

    pub async fn count(&self, sql: &str, id: &str) -> i64 {
        sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_owned()))
            .bind(id)
            .fetch_one(self.database.pool())
            .await
            .unwrap()
    }

    /// Inserts `count` live rows straight into `db`, bypassing the API.
    pub async fn seed_rows(&self, db: &str, count: i64) {
        let teamspace: String = self
            .database
            .scalar(&format!("SELECT teamspace_id FROM pages WHERE id = '{db}'"))
            .await
            .unwrap();
        let now = TimestampMillis::now().as_millis();
        let mut tx = self.database.pool().begin().await.unwrap();
        for position in 0..count {
            sqlx::query(
                "INSERT INTO pages (id, workspace_id, parent_id, teamspace_id, title, position, creator_id, updated_by, created_at, updated_at) \
                 VALUES ($1, $2, $3, $4, 'Row', $5, $6, $7, $8, $9)",
            )
            .bind(Id::new_v7().to_string())
            .bind(&self.workspace_id)
            .bind(db)
            .bind(&teamspace)
            .bind(position)
            .bind(self.owner_id.to_string())
            .bind(self.owner_id.to_string())
            .bind(now)
            .bind(now)
            .execute(&mut *tx)
            .await
            .unwrap();
        }
        tx.commit().await.unwrap();
    }
}

/// The code of an error body with its status.
pub fn problem((status, body): &(StatusCode, Value)) -> (StatusCode, Option<&str>) {
    (*status, body["code"].as_str())
}

pub fn id_of(value: &Value) -> &str {
    value["id"].as_str().unwrap()
}

pub fn property<'a>(database: &'a Value, name: &str) -> &'a Value {
    database["properties"]
        .as_array()
        .unwrap()
        .iter()
        .find(|property| property["name"] == name)
        .unwrap_or_else(|| panic!("no property {name}"))
}

pub fn names(items: &Value) -> Vec<&str> {
    items
        .as_array()
        .unwrap()
        .iter()
        .map(|item| {
            item["name"]
                .as_str()
                .unwrap_or_else(|| item["title"].as_str().unwrap())
        })
        .collect()
}

pub fn option_id(property: &Value, name: &str) -> String {
    property["config"]["options"]
        .as_array()
        .unwrap()
        .iter()
        .find(|option| option["name"] == name)
        .unwrap_or_else(|| panic!("no option {name}"))["id"]
        .as_str()
        .unwrap()
        .to_owned()
}
