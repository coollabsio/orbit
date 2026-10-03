use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use orbit_platform::{AuthenticatedUser, Id, PasswordService, TestDatabase, TimestampMillis};
use orbit_server::auth_routes::{AuthState, CookieMode, auth_router};
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::workspace_routes::{WorkspaceState, workspace_router};
use serde_json::{Value, json};
use tower::ServiceExt;

struct Fixture {
    database: TestDatabase,
    identity: Arc<IdentityRepository>,
    app: axum::Router,
    workspace_id: String,
    owner_id: Id,
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
        let owner_id: String = sqlx::query_scalar("SELECT id FROM users")
            .fetch_one(database.pool())
            .await
            .unwrap();
        let app = workspace_router(WorkspaceState::new(
            Arc::clone(&identity),
            "https://orbit.test".to_owned(),
            CookieMode::secure(),
        ))
        .merge(auth_router(AuthState::new(
            Arc::clone(&identity),
            CookieMode::secure(),
        )));
        Self {
            database,
            identity,
            app,
            workspace_id: setup.workspace_id.to_string(),
            owner_id: owner_id.parse().unwrap(),
            owner_cookie: format!("__Host-orbit_session={}", setup.session.token),
        }
    }

    /// A user with a session; a member of the workspace if `workspace_id` is given.
    async fn user(&self, email: &str, workspace_id: Option<&str>) -> (Id, String) {
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
        if let Some(workspace_id) = workspace_id {
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
        }
        let user = AuthenticatedUser {
            id,
            email: email.to_owned(),
            display_name: "Member".to_owned(),
        };
        let session = self.identity.create_session(&user, now).await.unwrap();
        (id, format!("__Host-orbit_session={}", session.token))
    }

    async fn send(
        &self,
        method: &str,
        uri: &str,
        cookie: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let request = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::COOKIE, cookie)
            .header(header::CONTENT_TYPE, "application/json")
            .body(body.map_or_else(Body::empty, |body| Body::from(body.to_string())))
            .unwrap();
        let response = self.app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    fn profile(&self, user_id: Id) -> String {
        format!(
            "/api/v1/workspaces/{}/profiles/{user_id}",
            self.workspace_id
        )
    }
}

#[tokio::test]
async fn profile_parts_are_checked_kept_when_absent_and_removed_when_empty() {
    let fixture = Fixture::new().await;
    let patch = |body: Value| {
        fixture.send(
            "PATCH",
            "/api/v1/auth/me",
            &fixture.owner_cookie,
            Some(body),
        )
    };

    let (status, me) = patch(json!({
        "display_name": "Owner",
        "title": "  Staff engineer ",
        "pronouns": "they/them",
        "timezone": "Europe/Budapest",
        "bio": "Writes parsers.",
        "phone": " +36 (30) 123-4567 ",
    }))
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(me["title"], "Staff engineer");
    assert_eq!(me["timezone"], "Europe/Budapest");
    assert_eq!(me["phone"], "+36 (30) 123-4567");

    // An older client sends the name only: the other parts stay. An empty text removes a part.
    let (_, me) = patch(json!({ "display_name": "Owner Two", "pronouns": "" })).await;
    assert_eq!(me["display_name"], "Owner Two");
    assert_eq!(me["title"], "Staff engineer");
    assert_eq!(me["bio"], "Writes parsers.");
    assert!(me["pronouns"].is_null());

    for invalid in [
        json!({ "display_name": "Owner", "title": "x".repeat(81) }),
        json!({ "display_name": "Owner", "bio": "x".repeat(501) }),
        json!({ "display_name": "Owner", "timezone": "Europe/Buda pest" }),
        json!({ "display_name": "Owner", "phone": "call me" }),
        json!({ "display_name": "Owner", "phone": "+-()" }),
        json!({ "display_name": "Owner", "phone": "1".repeat(33) }),
    ] {
        let (status, problem) = patch(invalid).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(problem["code"], "invalid_profile");
    }
    let (_, me) = fixture
        .send("GET", "/api/v1/auth/me", &fixture.owner_cookie, None)
        .await;
    assert_eq!(me["title"], "Staff engineer");
}

#[tokio::test]
async fn a_member_reads_profiles_of_the_workspace_and_a_note_is_its_authors_only() {
    let fixture = Fixture::new().await;
    let workspace = fixture.workspace_id.clone();
    let (ada, ada_cookie) = fixture.user("ada@example.com", Some(&workspace)).await;
    let (bob, bob_cookie) = fixture.user("bob@example.com", Some(&workspace)).await;
    let (stranger, stranger_cookie) = fixture.user("eve@example.com", None).await;
    fixture
        .send(
            "PATCH",
            "/api/v1/auth/me",
            &fixture.owner_cookie,
            Some(json!({ "display_name": "Owner", "title": "Founder", "phone": "555 0100" })),
        )
        .await;
    let owner = fixture.profile(fixture.owner_id);
    let note = format!("{owner}/note");

    let (status, profile) = fixture.send("GET", &owner, &ada_cookie, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(profile["title"], "Founder");
    assert_eq!(profile["phone"], "555 0100");
    assert!(profile["note"].is_null());

    // Ada's note: Ada reads it, Bob and the owner do not.
    let (status, _) = fixture
        .send(
            "PUT",
            &note,
            &ada_cookie,
            Some(json!({ "body": " Likes short reviews. " })),
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, profile) = fixture.send("GET", &owner, &ada_cookie, None).await;
    assert_eq!(profile["note"], "Likes short reviews.");
    let (_, profile) = fixture.send("GET", &owner, &bob_cookie, None).await;
    assert!(profile["note"].is_null());
    let (_, profile) = fixture
        .send("GET", &owner, &fixture.owner_cookie, None)
        .await;
    assert!(profile["note"].is_null());

    let (status, problem) = fixture
        .send(
            "PUT",
            &note,
            &ada_cookie,
            Some(json!({ "body": "x".repeat(1001) })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(problem["code"], "invalid_note");
    // An empty note removes it.
    fixture
        .send("PUT", &note, &ada_cookie, Some(json!({ "body": "" })))
        .await;
    let (_, profile) = fixture.send("GET", &owner, &ada_cookie, None).await;
    assert!(profile["note"].is_null());

    // Somebody outside the workspace reads nothing and writes nothing; neither does a member
    // about somebody outside it.
    let (status, _) = fixture.send("GET", &owner, &stranger_cookie, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = fixture
        .send(
            "PUT",
            &note,
            &stranger_cookie,
            Some(json!({ "body": "hi" })),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = fixture
        .send("GET", &fixture.profile(stranger), &ada_cookie, None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = fixture
        .send(
            "PUT",
            &format!("{}/note", fixture.profile(stranger)),
            &ada_cookie,
            Some(json!({ "body": "hi" })),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let _ = (ada, bob);
}

#[tokio::test]
async fn saves_of_different_parts_at_the_same_time_keep_both_parts() {
    let fixture = Fixture::new().await;
    let patch = |body: Value| {
        fixture.send(
            "PATCH",
            "/api/v1/auth/me",
            &fixture.owner_cookie,
            Some(body),
        )
    };
    // Each request names one part: neither may write the other part back to what it was.
    for round in 0..10 {
        let title = format!("Title {round}");
        let bio = format!("Bio {round}");
        let (first, second) = tokio::join!(
            patch(json!({ "display_name": "Owner", "title": title })),
            patch(json!({ "display_name": "Owner", "bio": bio })),
        );
        assert_eq!(first.0, StatusCode::OK);
        assert_eq!(second.0, StatusCode::OK);
        let (_, me) = fixture
            .send("GET", "/api/v1/auth/me", &fixture.owner_cookie, None)
            .await;
        assert_eq!(me["title"], title.as_str());
        assert_eq!(me["bio"], bio.as_str());
    }
}
