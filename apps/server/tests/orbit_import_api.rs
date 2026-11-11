//! Orbit import service behavior that needs no second Orbit instance.

use std::sync::Arc;

use orbit_platform::{
    AttachmentMutationCoordinator, Id, LocalBlobStore, ObjectStorage, PasswordService,
    TestDatabase, TimestampMillis, UploadLimits, UploadService,
};
use orbit_server::orbit_import::{OrbitImportService, OrbitImportSettings, OrbitImportStatus};
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};

const DAY_MS: i64 = 24 * 60 * 60 * 1000;

async fn insert_import(
    database: &TestDatabase,
    workspace_id: Id,
    user_id: Id,
    status: &str,
    updated_at: i64,
) -> Id {
    let id = Id::new_v7();
    sqlx::query(
        "INSERT INTO orbit_imports (id, workspace_id, user_id, status, source_kind, created_at, updated_at) \
         VALUES ($1, $2, $3, $4, 'upload', $5, $6)",
    )
    .bind(id.to_string())
    .bind(workspace_id.to_string())
    .bind(user_id.to_string())
    .bind(status)
    .bind(updated_at)
    .bind(updated_at)
    .execute(database.pool())
    .await
    .unwrap();
    id
}

async fn status(database: &TestDatabase, id: Id) -> String {
    sqlx::query_scalar("SELECT status FROM orbit_imports WHERE id = $1")
        .bind(id.to_string())
        .fetch_one(database.pool())
        .await
        .unwrap()
}

#[tokio::test]
async fn polling_expires_only_the_members_own_stale_imports() {
    let root = tempfile::tempdir().unwrap();
    let database = TestDatabase::new().await.unwrap();
    let identity = IdentityRepository::new((*database).clone());
    let now = TimestampMillis::now().as_millis();
    identity
        .store_setup_token(
            "operator-secret",
            TimestampMillis::from_millis(now + 60_000),
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
            TimestampMillis::now(),
        )
        .await
        .unwrap();
    let other_id = Id::new_v7();
    sqlx::query(
        "INSERT INTO users (id, email, normalized_email, display_name, password_hash, created_at, updated_at) \
         VALUES ($1, 'other@example.com', 'other@example.com', 'Other', 'x', $2, $3)",
    )
    .bind(other_id.to_string())
    .bind(now)
    .bind(now)
    .execute(database.pool())
    .await
    .unwrap();
    let uploads = UploadService::new(
        (*database).clone(),
        Arc::new(LocalBlobStore::new(root.path().join("attachments"))),
        AttachmentMutationCoordinator::default(),
        UploadLimits::new(1024 * 1024, 4 * 1024 * 1024).unwrap(),
    );
    let service = OrbitImportService::new(
        (*database).clone(),
        uploads,
        ObjectStorage::default(),
        root.path().join("backups"),
        OrbitImportSettings {
            app_key: Some([7u8; 32]),
            max_file_bytes: 1024 * 1024,
            max_archive_bytes: 64 * 1024,
            allow_local: false,
        },
    );
    let (workspace, owner) = (setup.workspace_id, setup.user_id);
    let stale = now - 2 * DAY_MS;
    let own_ready = insert_import(&database, workspace, owner, "ready", stale).await;
    let own_stalled = insert_import(&database, workspace, owner, "importing", stale).await;
    let own_dead = insert_import(&database, workspace, owner, "queued", now).await;
    sqlx::query(
        "INSERT INTO jobs (id, kind, payload_json, state, max_attempts, available_at, created_at, updated_at) \
         VALUES ($1, 'orbit.import', $2, 'dead', 3, $3, $4, $5)",
    )
    .bind(Id::new_v7().to_string())
    .bind(format!("{{\"import_id\":\"{own_dead}\"}}"))
    .bind(now)
    .bind(now)
    .bind(now)
    .execute(database.pool())
    .await
    .unwrap();
    let others_ready = insert_import(&database, workspace, other_id, "ready", stale).await;

    let polled = service
        .get(workspace, owner, own_ready, false)
        .await
        .unwrap();
    assert_eq!(polled.status, OrbitImportStatus::Expired);
    assert_eq!(status(&database, own_stalled).await, "failed");
    assert_eq!(status(&database, own_dead).await, "failed");
    assert_eq!(status(&database, others_ready).await, "ready");

    service.list(workspace, owner).await.unwrap();
    assert_eq!(status(&database, others_ready).await, "ready");

    service.expire_stale(TimestampMillis::now()).await.unwrap();
    assert_eq!(status(&database, others_ready).await, "expired");
}
