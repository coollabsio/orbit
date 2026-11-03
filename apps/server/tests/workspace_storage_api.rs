//! A workspace's own S3 bucket (Settings → File storage). The in-memory S3 server listens on a local address,
//! which a workspace may not use, so the moves themselves are tested in `crates/platform/tests/object_storage.rs`.

use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use orbit_platform::{
    AttachmentMutationCoordinator, AuthenticatedUser, Id, LocalBlobStore, ObjectStorage,
    PasswordService, TestDatabase, TieredBlobStore, TimestampMillis,
};
use orbit_server::mail::Mailer;
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::workspace_routes::{WorkspaceState, workspace_router};
use serde_json::{Value, json};
use tower::ServiceExt;

struct Fixture {
    database: TestDatabase,
    identity: Arc<IdentityRepository>,
    _attachments: tempfile::TempDir,
    app: axum::Router,
    workspace_id: Id,
    owner: String,
}

async fn fixture() -> Fixture {
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
                email: "root@example.com".to_owned(),
                display_name: "Root".to_owned(),
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
    let attachments = tempfile::tempdir().unwrap();
    let storage = ObjectStorage::default();
    let store = TieredBlobStore::new(
        LocalBlobStore::new(attachments.path()),
        storage.clone(),
        AttachmentMutationCoordinator::default(),
    );
    let (mailer, _) = Mailer::recording((*database).clone(), "https://orbit.test");
    let app = workspace_router(
        WorkspaceState::new(
            Arc::clone(&identity),
            "https://orbit.test".to_owned(),
            orbit_server::auth_routes::CookieMode::secure(),
        )
        .with_mailer(mailer)
        .with_object_storage(storage, store),
    );
    Fixture {
        database,
        identity,
        _attachments: attachments,
        app,
        workspace_id: setup.workspace_id,
        owner: format!("__Host-orbit_session={}", setup.session.token),
    }
}

impl Fixture {
    fn uri(&self) -> String {
        format!("/api/v1/workspaces/{}/storage/bucket", self.workspace_id)
    }

    async fn call(&self, method: &str, cookie: &str, body: Option<Value>) -> (StatusCode, Value) {
        let request = Request::builder()
            .method(method)
            .uri(self.uri())
            .header(header::COOKIE, cookie);
        let request = match body {
            Some(body) => request
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string())),
            None => request.body(Body::empty()),
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

    async fn member(&self) -> String {
        let id = Id::new_v7();
        let now = TimestampMillis::now();
        sqlx::query(
            "INSERT INTO users (id, email, normalized_email, display_name, password_hash, created_at, updated_at) \
             VALUES ($1, 'member@example.com', 'member@example.com', 'Member', 'unused', $2, $2)",
        )
        .bind(id.to_string())
        .bind(now.as_millis())
        .execute(self.database.pool())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, updated_at) \
             VALUES ($1, $2, $3, 'member', 0, $4, $4)",
        )
        .bind(Id::new_v7().to_string())
        .bind(self.workspace_id.to_string())
        .bind(id.to_string())
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

fn s3_body(endpoint: &str) -> Value {
    json!({
        "endpoint": endpoint, "region": "", "bucket": "team-files", "prefix": "orbit",
        "access_key_id": "key", "secret_access_key": "secret", "path_style": true
    })
}

#[tokio::test]
async fn a_workspace_starts_on_the_installation_storage_and_members_cannot_see_the_bucket() {
    let fixture = fixture().await;
    let (status, view) = fixture.call("GET", &fixture.owner, None).await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(
        view,
        json!({"s3": null, "removing": false, "checking": false, "files_to_move": 0, "move_error": null})
    );

    let member = fixture.member().await;
    for (method, body) in [
        ("GET", None),
        ("PUT", Some(s3_body("https://s3.example.com"))),
        ("DELETE", None),
    ] {
        let (status, problem) = fixture.call(method, &member, body).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{method} {problem}");
        assert_eq!(problem["code"], "workspace_action_forbidden");
    }

    let (status, view) = fixture.call("DELETE", &fixture.owner, None).await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["s3"], Value::Null);
}

#[tokio::test]
async fn a_workspace_bucket_must_be_on_the_public_internet() {
    let fixture = fixture().await;
    for endpoint in [
        "http://s3.example.com",
        "https://127.0.0.1:9000",
        "https://10.0.0.5",
        "https://169.254.169.254",
        "https://[::1]",
    ] {
        let (status, problem) = fixture
            .call("PUT", &fixture.owner, Some(s3_body(endpoint)))
            .await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{endpoint} {problem}"
        );
        assert_eq!(problem["code"], "invalid_storage_settings");
    }
    // A host name that resolves to a local address is refused when Orbit connects.
    let (status, problem) = fixture
        .call("PUT", &fixture.owner, Some(s3_body("https://localhost")))
        .await;
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{problem}");
    assert_eq!(problem["code"], "storage_unreachable");
    let saved: i64 = fixture
        .database
        .scalar("SELECT COUNT(*) FROM workspace_storage")
        .await
        .unwrap();
    assert_eq!(saved, 0);
}

#[tokio::test]
async fn a_removed_bucket_stays_until_its_files_moved_back() {
    use orbit_server::repositories::instance_settings::S3Settings;
    use orbit_server::repositories::workspace_storage::WorkspaceStorageRepository;

    let fixture = fixture().await;
    let owner_id: Id = fixture
        .database
        .scalar::<String>("SELECT id FROM users WHERE email = 'root@example.com'")
        .await
        .unwrap()
        .parse()
        .unwrap();
    let repository = WorkspaceStorageRepository::new((*fixture.database).clone());
    let s3 = S3Settings {
        endpoint: "https://s3.example.com".to_owned(),
        region: "us-east-1".to_owned(),
        bucket: "team-files".to_owned(),
        prefix: "orbit".to_owned(),
        access_key_id: "key".to_owned(),
        secret_access_key: b"sealed".to_vec(),
        path_style: false,
    };
    let workspace = fixture.workspace_id;
    let id = workspace.to_string();
    let now = TimestampMillis::now();

    repository
        .save(workspace, owner_id, &s3, "r1", now)
        .await
        .unwrap();
    let saved = repository.all().await.unwrap();
    assert_eq!(saved.len(), 1);
    assert!(saved[0].active);
    assert_eq!(saved[0].s3, s3);
    assert!(!repository.forget_drained(&id).await.unwrap(), "in use");

    repository
        .remove(workspace, owner_id, "r2", now)
        .await
        .unwrap();
    assert!(!repository.all().await.unwrap()[0].active);
    repository
        .save(workspace, owner_id, &s3, "r3", now)
        .await
        .unwrap();
    assert!(repository.all().await.unwrap()[0].active, "saved again");

    repository
        .remove(workspace, owner_id, "r4", now)
        .await
        .unwrap();
    assert!(repository.forget_drained(&id).await.unwrap());
    assert!(repository.all().await.unwrap().is_empty());
    let audited: i64 = fixture
        .database
        .scalar(
            "SELECT COUNT(*) FROM audit_events WHERE action IN ('workspace.storage_updated', 'workspace.storage_removed')",
        )
        .await
        .unwrap();
    assert_eq!(audited, 4);
}
