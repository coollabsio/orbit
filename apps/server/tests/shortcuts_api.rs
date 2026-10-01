use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use orbit_platform::{AuthenticatedUser, Id, PasswordService, TestDatabase, TimestampMillis};
use orbit_server::auth_routes::{AuthState, CookieMode, auth_router};
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use serde_json::{Map, Value, json};
use tower::ServiceExt;

const PATH: &str = "/api/v1/auth/shortcuts";

struct Fixture {
    database: TestDatabase,
    identity: Arc<IdentityRepository>,
    app: axum::Router,
    owner_cookie: String,
}

impl Fixture {
    async fn new() -> Self {
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
        let app = auth_router(AuthState::new(Arc::clone(&identity), CookieMode::secure()));
        Self {
            database,
            identity,
            app,
            owner_cookie: format!("__Host-orbit_session={}", setup.session.token),
        }
    }

    /// Adds a second user with a live session.
    async fn add_user(&self, email: &str) -> String {
        let id = Id::new_v7();
        let now = TimestampMillis::now();
        sqlx::query(
            "INSERT INTO users (id, email, normalized_email, display_name, password_hash, created_at, updated_at) \
             VALUES (?, ?, ?, 'Other', 'unused', ?, ?)",
        )
        .bind(id.to_string())
        .bind(email)
        .bind(email)
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
                    display_name: "Other".to_owned(),
                },
                now,
            )
            .await
            .unwrap();
        format!("__Host-orbit_session={}", session.token)
    }

    async fn call(
        &self,
        method: &str,
        cookie: Option<&str>,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut builder = Request::builder().method(method).uri(PATH);
        if let Some(cookie) = cookie {
            builder = builder.header(header::COOKIE, cookie);
        }
        let request = match body {
            Some(body) => builder
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
            None => builder.body(Body::empty()).unwrap(),
        };
        let response = self.app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, value)
    }

    async fn get(&self, cookie: &str) -> Value {
        let (status, body) = self.call("GET", Some(cookie), None).await;
        assert_eq!(status, StatusCode::OK);
        body
    }
}

#[tokio::test]
async fn a_user_without_overrides_gets_an_empty_map() {
    let fixture = Fixture::new().await;
    assert_eq!(
        fixture.get(&fixture.owner_cookie).await,
        json!({ "bindings": {} })
    );
}

#[tokio::test]
async fn put_replaces_the_whole_map_and_keeps_null_bindings() {
    let fixture = Fixture::new().await;
    let first = json!({ "bindings": { "task.setStatus": "Q", "nav.inbox": null } });
    let (status, body) = fixture
        .call("PUT", Some(&fixture.owner_cookie), Some(first.clone()))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, first);
    assert_eq!(fixture.get(&fixture.owner_cookie).await, first);

    let second = json!({ "bindings": { "task.create": "N" } });
    let (status, _) = fixture
        .call("PUT", Some(&fixture.owner_cookie), Some(second.clone()))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(fixture.get(&fixture.owner_cookie).await, second);
}

#[tokio::test]
async fn limits_are_enforced_and_a_refused_map_is_not_stored() {
    let fixture = Fixture::new().await;
    let cookie = Some(fixture.owner_cookie.as_str());
    let too_many: Map<String, Value> = (0..201)
        .map(|index| (format!("command.{index}"), json!("Q")))
        .collect();
    let cases = [
        json!({ "bindings": too_many }),
        json!({ "bindings": { "x".repeat(65): "Q" } }),
        json!({ "bindings": { "": "Q" } }),
        json!({ "bindings": { "task.create": "Q".repeat(33) } }),
        json!({ "bindings": { "task.create": "" } }),
    ];
    for case in cases {
        let (status, body) = fixture.call("PUT", cookie, Some(case)).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body["code"], "invalid_shortcuts");
    }
    // the limits themselves are allowed
    let at_limit: Map<String, Value> = (0..200)
        .map(|index| (format!("command.{index}"), json!("Q")))
        .collect();
    let (status, _) = fixture
        .call("PUT", cookie, Some(json!({ "bindings": at_limit })))
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = fixture
        .call(
            "PUT",
            cookie,
            Some(json!({ "bindings": { "x".repeat(64): "Q".repeat(32) } })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn a_body_of_the_wrong_shape_is_refused() {
    let fixture = Fixture::new().await;
    let cookie = Some(fixture.owner_cookie.as_str());
    for case in [
        json!({ "bindings": ["Q"] }),
        json!({ "bindings": { "task.create": 5 } }),
        json!({}),
    ] {
        let (status, _) = fixture.call("PUT", cookie, Some(case)).await;
        assert!(status.is_client_error(), "{status}");
    }
    assert_eq!(
        fixture.get(&fixture.owner_cookie).await,
        json!({ "bindings": {} })
    );
}

#[tokio::test]
async fn each_user_has_their_own_map() {
    let fixture = Fixture::new().await;
    let other = fixture.add_user("other@example.com").await;
    fixture
        .call(
            "PUT",
            Some(&fixture.owner_cookie),
            Some(json!({ "bindings": { "task.create": "N" } })),
        )
        .await;
    assert_eq!(fixture.get(&other).await, json!({ "bindings": {} }));
    fixture
        .call(
            "PUT",
            Some(&other),
            Some(json!({ "bindings": { "task.create": "M" } })),
        )
        .await;
    assert_eq!(
        fixture.get(&fixture.owner_cookie).await,
        json!({ "bindings": { "task.create": "N" } })
    );
}

#[tokio::test]
async fn shortcuts_need_a_session() {
    let fixture = Fixture::new().await;
    let (status, _) = fixture.call("GET", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = fixture
        .call("PUT", None, Some(json!({ "bindings": {} })))
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}
