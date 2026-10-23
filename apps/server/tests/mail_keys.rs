//! Workspace mail data keys (`mail_keys`) and the sealing of raw messages.

use std::sync::Arc;

use orbit_platform::{
    AttachmentMutationCoordinator, Id, LocalBlobStore, PasswordService, TestDatabase,
    TimestampMillis, UploadLimits, UploadService,
};
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::repositories::mail::{
    MailError, MailKeys, MailRepository, open_blob, receipt_aad, seal_blob,
};
use orbit_server::repositories::workspaces::WorkspaceRepository;

const APP_KEY: [u8; 32] = [7; 32];

async fn two_workspaces() -> (tempfile::TempDir, TestDatabase, UploadService, Id, Id) {
    let database = TestDatabase::new().await.unwrap();
    let identity = IdentityRepository::new((*database).clone());
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
    let second = WorkspaceRepository::new((*database).clone())
        .create(setup.user_id, "Second".to_owned(), "request", now)
        .await
        .unwrap();
    let root = tempfile::tempdir().unwrap();
    let uploads = UploadService::new(
        (*database).clone(),
        Arc::new(LocalBlobStore::new(root.path().join("attachments"))),
        AttachmentMutationCoordinator::default(),
        UploadLimits::default(),
    );
    (root, database, uploads, setup.workspace_id, second.id)
}

#[tokio::test]
async fn current_creates_one_key_per_workspace() {
    let (_root, database, uploads, first, second) = two_workspaces().await;
    let repository = MailRepository::new((*database).clone(), uploads, APP_KEY);
    let keys = repository.keys();

    let mut tx = database.immediate_transaction().await.unwrap();
    let (version, key) = keys.current(&mut tx, first).await.unwrap();
    assert_eq!(version, 1);
    assert_eq!(keys.current(&mut tx, first).await.unwrap(), (1, key));
    tx.commit().await.unwrap();

    let mut tx = database.immediate_transaction().await.unwrap();
    assert_eq!(keys.current(&mut tx, first).await.unwrap(), (1, key));
    let (second_version, second_key) = keys.current(&mut tx, second).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(second_version, 1);
    assert_ne!(second_key, key);
    assert_eq!(
        database
            .scalar::<i64>("SELECT COUNT(*) FROM mail_keys")
            .await
            .unwrap(),
        2
    );

    assert_eq!(keys.get(first, 1).await.unwrap(), key);
    assert!(matches!(keys.get(first, 2).await, Err(MailError::NotFound)));

    // The key is stored sealed, never in plain text.
    let sealed: Vec<u8> = sqlx::query_scalar(
        "SELECT key_sealed FROM mail_keys WHERE workspace_id = $1 AND version = 1",
    )
    .bind(first.to_string())
    .fetch_one(database.pool())
    .await
    .unwrap();
    assert!(!sealed.windows(32).any(|window| window == key));

    // Another app key cannot open it.
    let other_app = MailKeys::new((*database).clone(), [8; 32]);
    assert!(matches!(
        other_app.get(first, 1).await,
        Err(MailError::Crypto)
    ));

    // A sealed key moved to another workspace does not open there.
    sqlx::query("UPDATE mail_keys SET key_sealed = $1 WHERE workspace_id = $2")
        .bind(&sealed)
        .bind(second.to_string())
        .execute(database.pool())
        .await
        .unwrap();
    assert!(matches!(keys.get(second, 1).await, Err(MailError::Crypto)));
}

#[test]
fn a_sealed_raw_message_opens_only_for_its_receipt() {
    let key = [3; 32];
    let receipt = Id::new_v7();
    let raw = b"From: ann@sender.test\r\n\r\nHello\r\n";
    let sealed = seal_blob(&key, &receipt_aad(receipt), raw).unwrap();
    assert_ne!(&sealed[..], &raw[..]);

    assert_eq!(
        open_blob(&key, &receipt_aad(receipt), &sealed).unwrap(),
        raw
    );
    assert!(matches!(
        open_blob(&key, &receipt_aad(Id::new_v7()), &sealed),
        Err(MailError::Crypto)
    ));
    assert!(matches!(
        open_blob(&[4; 32], &receipt_aad(receipt), &sealed),
        Err(MailError::Crypto)
    ));
    assert_eq!(receipt_aad(receipt), format!("mail-raw:{receipt}"));
}
