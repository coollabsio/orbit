//! Attachments and backups in S3. The tests run against a small in-memory S3 server; set `ORBIT_TEST_S3_ENDPOINT`,
//! `ORBIT_TEST_S3_BUCKET`, `ORBIT_TEST_S3_ACCESS_KEY` and `ORBIT_TEST_S3_SECRET_KEY` to run them against a real
//! S3-compatible server (path-style URLs) instead.

use std::sync::Arc;

use chrono::Utc;
use orbit_platform::{
    AttachmentMutationCoordinator, BackupService, BlobStore, Database, DatabaseConfig, Id,
    LocalBlobStore, MigrationRunner, NewAttachmentReference, ObjectStorage, ObjectStorageState,
    S3Bucket, S3Config, TestDatabase, TieredBlobStore, UploadLimits, UploadService, start_fake_s3,
};
use tokio::io::AsyncReadExt;

const DAY_MILLIS: i64 = 24 * 60 * 60 * 1000;

/// A bucket under a fresh prefix, so runs against a real server do not see each other's objects.
async fn bucket() -> S3Bucket {
    let prefix = format!("orbit-test-{}", Id::new_v7());
    let config = match std::env::var("ORBIT_TEST_S3_ENDPOINT") {
        Ok(endpoint) => S3Config {
            endpoint,
            region: "us-east-1".to_owned(),
            bucket: std::env::var("ORBIT_TEST_S3_BUCKET").unwrap(),
            prefix,
            access_key_id: std::env::var("ORBIT_TEST_S3_ACCESS_KEY").unwrap(),
            secret_access_key: std::env::var("ORBIT_TEST_S3_SECRET_KEY").unwrap(),
            path_style: true,
        },
        Err(_) => S3Config {
            prefix,
            ..start_fake_s3().await
        },
    };
    S3Bucket::new(config).unwrap()
}

struct Fixture {
    _root: tempfile::TempDir,
    database: TestDatabase,
    local: LocalBlobStore,
    bucket: S3Bucket,
    storage: ObjectStorage,
    store: TieredBlobStore,
    service: UploadService,
}

impl Fixture {
    async fn new(attachments_in_s3: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let database = TestDatabase::new().await.unwrap();
        let local = LocalBlobStore::new(root.path().join("attachments"));
        let bucket = bucket().await;
        let storage = ObjectStorage::default();
        storage.configure(ObjectStorageState {
            bucket: Some(bucket.clone()),
            attachments: attachments_in_s3,
            backups: false,
        });
        let mutations = AttachmentMutationCoordinator::default();
        let store = TieredBlobStore::new(local.clone(), storage.clone(), mutations.clone());
        let service = UploadService::new(
            (*database).clone(),
            Arc::new(store.clone()),
            mutations,
            UploadLimits::default(),
        );
        Self {
            _root: root,
            database,
            local,
            bucket,
            storage,
            store,
            service,
        }
    }

    async fn upload(&self, contents: &'static [u8]) -> String {
        let staged = self
            .service
            .stage(Id::new_v7(), Id::new_v7(), "file.txt", contents)
            .await
            .unwrap();
        self.service
            .finalize(&staged, NewAttachmentReference::new(Id::new_v7(), None))
            .await
            .unwrap()
            .storage_key
    }

    async fn read(&self, storage_key: &str) -> Vec<u8> {
        let mut bytes = Vec::new();
        self.store
            .open(storage_key)
            .await
            .unwrap()
            .read_to_end(&mut bytes)
            .await
            .unwrap();
        bytes
    }

    async fn in_bucket(&self, storage_key: &str) -> bool {
        self.bucket
            .last_modified(storage_key)
            .await
            .unwrap()
            .is_some()
    }
}

#[tokio::test]
async fn s3_attachments_are_stored_read_and_kept_after_release_until_the_delete_delay() {
    let fixture = Fixture::new(true).await;
    let key = fixture.upload(b"in the bucket").await;
    assert!(fixture.in_bucket(&key).await);
    assert!(fixture.local.blobs().await.unwrap().is_empty());
    assert!(fixture.local.temporary_files().await.unwrap().is_empty());
    assert_eq!(fixture.read(&key).await, b"in the bucket");

    fixture
        .database
        .execute("DELETE FROM attachment_references")
        .await
        .unwrap();
    let now = Utc::now().timestamp_millis();
    let released = fixture
        .service
        .reconcile(now + 2 * DAY_MILLIS)
        .await
        .unwrap();
    assert_eq!(released.deleted_blobs, 1);
    assert_eq!(released.deleted_untracked_files, 0);
    assert!(
        fixture.in_bucket(&key).await,
        "a restored backup may still need the file"
    );

    let later = fixture
        .service
        .reconcile(now + 39 * DAY_MILLIS)
        .await
        .unwrap();
    assert_eq!(later.deleted_untracked_files, 0);
    let expired = fixture
        .service
        .reconcile(now + 41 * DAY_MILLIS)
        .await
        .unwrap();
    assert_eq!(expired.deleted_untracked_files, 1);
    assert!(!fixture.in_bucket(&key).await);
}

#[tokio::test]
async fn the_mover_moves_blobs_to_s3_and_back_while_reads_keep_working() {
    let fixture = Fixture::new(false).await;
    let key = fixture.upload(b"first on disk").await;
    assert_eq!(fixture.local.blobs().await.unwrap().len(), 1);
    assert!(!fixture.in_bucket(&key).await);

    fixture.storage.configure(ObjectStorageState {
        bucket: Some(fixture.bucket.clone()),
        attachments: true,
        backups: false,
    });
    assert_eq!(fixture.read(&key).await, b"first on disk");
    assert_eq!(fixture.store.move_blobs().await.unwrap(), 1);
    assert!(fixture.local.blobs().await.unwrap().is_empty());
    assert!(fixture.in_bucket(&key).await);
    assert_eq!(fixture.read(&key).await, b"first on disk");
    assert_eq!(fixture.store.status().remaining, 0);

    fixture.storage.configure(ObjectStorageState {
        bucket: Some(fixture.bucket.clone()),
        attachments: false,
        backups: false,
    });
    assert_eq!(fixture.store.move_blobs().await.unwrap(), 1);
    assert!(!fixture.in_bucket(&key).await);
    assert_eq!(fixture.local.blobs().await.unwrap().len(), 1);
    assert_eq!(fixture.read(&key).await, b"first on disk");
    assert!(!fixture.bucket.any("blobs/").await.unwrap());
}

#[tokio::test]
async fn a_duplicate_upload_to_s3_refreshes_the_existing_object() {
    let fixture = Fixture::new(true).await;
    let first = fixture.upload(b"same").await;
    let before = fixture.bucket.last_modified(&first).await.unwrap().unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    let staged = fixture
        .service
        .stage(
            first.split('/').nth(1).unwrap().parse().unwrap(),
            Id::new_v7(),
            "again.txt",
            &b"same"[..],
        )
        .await
        .unwrap();
    let second = fixture
        .service
        .finalize(&staged, NewAttachmentReference::new(Id::new_v7(), None))
        .await
        .unwrap();
    assert_eq!(second.storage_key, first);
    let after = fixture.bucket.last_modified(&first).await.unwrap().unwrap();
    assert!(after > before, "deduplication restarts the quarantine");
}

#[tokio::test]
async fn backups_go_to_s3_and_restore_from_it() {
    if orbit_platform::testing_postgres() {
        return; // file snapshots are SQLite-only
    }
    let root = tempfile::tempdir().unwrap();
    let database = Database::open(&DatabaseConfig::new(root.path().join("orbit.sqlite")))
        .await
        .unwrap();
    MigrationRunner::embedded(env!("CARGO_PKG_VERSION"))
        .run(&database)
        .await
        .unwrap();
    let attachments = root.path().join("attachments");
    std::fs::create_dir_all(&attachments).unwrap();
    std::fs::write(attachments.join("record.txt"), b"attachment contents").unwrap();
    let storage = ObjectStorage::default();
    storage.configure(ObjectStorageState {
        bucket: Some(bucket().await),
        attachments: false,
        backups: true,
    });
    let backups_root = root.path().join("backups");
    let service = BackupService::new(&backups_root, &attachments).with_object_storage(storage);

    let snapshot = service.create(&database).await.unwrap();
    assert!(snapshot.in_object_storage);
    assert!(!backups_root.join("snapshots").join(&snapshot.id).exists());
    let listed = service.list().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert!(listed[0].in_object_storage);
    assert_eq!(listed[0].manifest, snapshot.manifest);

    let verified = service.verify(&snapshot.id).await.unwrap();
    assert!(verified.in_object_storage);
    drop(database);
    let target = root.path().join("restored");
    service
        .restore_to(
            &snapshot.id,
            &target.join("orbit.sqlite"),
            &target.join("attachments"),
        )
        .await
        .unwrap();
    assert_eq!(
        std::fs::read(target.join("attachments/record.txt")).unwrap(),
        b"attachment contents"
    );
    assert!(target.join("orbit.sqlite").exists());

    service.delete(&snapshot.id).await.unwrap();
    assert!(service.list().await.unwrap().is_empty());
    assert!(!backups_root.join("downloads").join(&snapshot.id).exists());
    assert!(matches!(
        service.delete(&snapshot.id).await,
        Err(orbit_platform::BackupError::NotFound { .. })
    ));
}
