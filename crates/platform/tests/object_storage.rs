//! Attachments and backups in S3. The tests run against a small in-memory S3 server; set `ORBIT_TEST_S3_ENDPOINT`,
//! `ORBIT_TEST_S3_BUCKET`, `ORBIT_TEST_S3_ACCESS_KEY` and `ORBIT_TEST_S3_SECRET_KEY` to run them against a real
//! S3-compatible server (path-style URLs) instead.

use std::sync::Arc;

use chrono::Utc;
use orbit_platform::{
    AttachmentMutationCoordinator, BackupService, BlobStore, Database, DatabaseConfig, Id,
    LocalBlobStore, MigrationRunner, NewAttachmentReference, ObjectStorage, ObjectStorageState,
    S3Bucket, S3Config, TestDatabase, TieredBlobStore, UploadLimits, UploadService,
    WorkspaceBucket, start_fake_s3,
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
            ..Default::default()
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
        self.upload_to(Id::new_v7(), contents).await
    }

    async fn upload_to(&self, workspace_id: Id, contents: &'static [u8]) -> String {
        let staged = self
            .service
            .stage(workspace_id, Id::new_v7(), "file.txt", contents)
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
        ..Default::default()
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
        ..Default::default()
    });
    assert_eq!(fixture.store.move_blobs().await.unwrap(), 1);
    assert!(!fixture.in_bucket(&key).await);
    assert_eq!(fixture.local.blobs().await.unwrap().len(), 1);
    assert_eq!(fixture.read(&key).await, b"first on disk");
    assert!(!fixture.bucket.any("blobs/").await.unwrap());
}

fn with_workspace_bucket(
    fixture: &Fixture,
    workspace_id: Id,
    bucket: &S3Bucket,
    active: bool,
) -> ObjectStorageState {
    ObjectStorageState {
        bucket: Some(fixture.bucket.clone()),
        attachments: true,
        backups: false,
        workspace_buckets: [(
            workspace_id.to_string(),
            WorkspaceBucket {
                bucket: bucket.clone(),
                active,
            },
        )]
        .into(),
        ..ObjectStorageState::default()
    }
}

async fn holds(bucket: &S3Bucket, storage_key: &str) -> bool {
    bucket.last_modified(storage_key).await.unwrap().is_some()
}

#[tokio::test]
async fn a_workspace_bucket_takes_the_workspace_files_and_gives_them_back_when_removed() {
    let fixture = Fixture::new(true).await;
    let workspace = Id::new_v7();
    let other = fixture.upload(b"another workspace").await;
    let before = fixture.upload_to(workspace, b"before the bucket").await;
    let own = bucket().await;

    fixture
        .storage
        .configure(with_workspace_bucket(&fixture, workspace, &own, true));
    let after = fixture.upload_to(workspace, b"after the bucket").await;
    assert!(holds(&own, &after).await);
    assert!(!fixture.in_bucket(&after).await);
    assert_eq!(fixture.read(&before).await, b"before the bucket");

    assert_eq!(fixture.store.move_blobs().await.unwrap(), 1);
    assert!(holds(&own, &before).await);
    assert!(!fixture.in_bucket(&before).await);
    assert!(fixture.in_bucket(&other).await, "other workspaces stay");
    assert!(!holds(&own, &other).await);
    assert_eq!(
        fixture
            .store
            .workspace_status(&workspace.to_string())
            .unwrap()
            .remaining,
        0
    );
    assert_eq!(fixture.read(&before).await, b"before the bucket");
    assert_eq!(fixture.read(&other).await, b"another workspace");

    fixture
        .storage
        .configure(with_workspace_bucket(&fixture, workspace, &own, false));
    assert_eq!(fixture.read(&after).await, b"after the bucket");
    let removed = fixture.upload_to(workspace, b"after removal").await;
    assert!(fixture.in_bucket(&removed).await);
    assert!(!holds(&own, &removed).await);

    fixture.store.move_blobs().await.unwrap();
    assert!(!own.any("blobs/").await.unwrap());
    assert!(fixture.in_bucket(&before).await);
    assert!(fixture.in_bucket(&after).await);
    assert!(fixture.local.blobs().await.unwrap().is_empty());
    assert_eq!(fixture.read(&after).await, b"after the bucket");
}

/// Moving the files of a workspace bucket that is the installation's bucket deleted them (issue #54).
#[tokio::test]
async fn a_workspace_bucket_at_the_installation_location_keeps_the_files() {
    let fixture = Fixture::new(true).await;
    let workspace = Id::new_v7();
    let key = fixture.upload_to(workspace, b"only copy").await;
    let own = S3Bucket::new(fixture.bucket.config().clone()).unwrap();

    fixture
        .storage
        .configure(with_workspace_bucket(&fixture, workspace, &own, true));
    fixture.store.move_blobs().await.unwrap();
    assert!(fixture.in_bucket(&key).await);
    assert_eq!(fixture.read(&key).await, b"only copy");
    let status = fixture
        .store
        .workspace_status(&workspace.to_string())
        .unwrap();
    assert_eq!(status.remaining, 0);

    fixture
        .storage
        .configure(with_workspace_bucket(&fixture, workspace, &own, false));
    fixture.store.move_blobs().await.unwrap();
    assert!(fixture.in_bucket(&key).await, "the installation keeps it");
    assert!(fixture.local.blobs().await.unwrap().is_empty());
    let status = fixture
        .store
        .workspace_status(&workspace.to_string())
        .unwrap();
    assert_eq!(status.remaining, 0);
}

/// Two endpoint names for one bucket pass the location check; the mover still keeps the file.
#[tokio::test]
async fn a_workspace_bucket_under_another_endpoint_name_keeps_the_files() {
    let fixture = Fixture::new(true).await;
    let workspace = Id::new_v7();
    let key = fixture.upload_to(workspace, b"only copy").await;
    let config = fixture.bucket.config().clone();
    let own = S3Bucket::new(S3Config {
        endpoint: config.endpoint.replace("127.0.0.1", "localhost"),
        ..config
    })
    .unwrap();

    fixture
        .storage
        .configure(with_workspace_bucket(&fixture, workspace, &own, true));
    fixture.store.move_blobs().await.unwrap();
    assert!(holds(&own, &key).await);
    assert_eq!(fixture.read(&key).await, b"only copy");
}

#[tokio::test]
async fn existing_files_on_this_server_move_to_a_workspace_bucket_and_back() {
    let fixture = Fixture::new(false).await;
    let workspace = Id::new_v7();
    let id = workspace.to_string();
    let first = fixture.upload_to(workspace, b"first").await;
    let second = fixture.upload_to(workspace, b"second").await;
    let other = fixture.upload(b"another workspace").await;
    let own = bucket().await;
    let state = |active| ObjectStorageState {
        workspace_buckets: [(
            id.clone(),
            WorkspaceBucket {
                bucket: own.clone(),
                active,
            },
        )]
        .into(),
        ..Default::default()
    };

    fixture.storage.configure(state(true));
    assert_eq!(fixture.store.workspace_status(&id), None, "not counted yet");
    assert_eq!(fixture.store.move_blobs().await.unwrap(), 2);
    assert_eq!(fixture.store.workspace_status(&id).unwrap().remaining, 0);
    let local: Vec<_> = fixture
        .local
        .blobs()
        .await
        .unwrap()
        .into_iter()
        .map(|object| object.storage_key)
        .collect();
    assert_eq!(local, std::slice::from_ref(&other));
    assert!(holds(&own, &first).await && holds(&own, &second).await);
    assert_eq!(fixture.read(&second).await, b"second");

    fixture.storage.configure(state(false));
    assert_eq!(
        fixture.store.workspace_status(&id),
        None,
        "not counted since the removal"
    );
    assert_eq!(fixture.store.move_blobs().await.unwrap(), 2);
    assert_eq!(fixture.store.workspace_status(&id).unwrap().remaining, 0);
    assert!(!own.any("blobs/").await.unwrap());
    assert_eq!(fixture.local.blobs().await.unwrap().len(), 3);
    assert_eq!(fixture.read(&first).await, b"first");
}

#[tokio::test]
async fn a_workspace_bucket_that_fails_does_not_stop_the_cleanup() {
    let fixture = Fixture::new(false).await;
    let workspace = Id::new_v7();
    let unreachable = S3Bucket::new(S3Config {
        endpoint: "http://127.0.0.1:9".to_owned(),
        ..fixture.bucket.config().clone()
    })
    .unwrap();
    let key = fixture.upload(b"unused").await;
    fixture.storage.configure(ObjectStorageState {
        bucket: Some(fixture.bucket.clone()),
        workspace_buckets: [(
            workspace.to_string(),
            WorkspaceBucket {
                bucket: unreachable,
                active: true,
            },
        )]
        .into(),
        ..Default::default()
    });
    fixture
        .database
        .execute("DELETE FROM attachment_references")
        .await
        .unwrap();

    let result = fixture
        .service
        .reconcile(Utc::now().timestamp_millis() + 2 * DAY_MILLIS)
        .await
        .unwrap();
    assert_eq!(result.deleted_blobs, 1);
    assert!(
        fixture
            .local
            .blobs()
            .await
            .unwrap()
            .iter()
            .all(|object| object.storage_key != key)
    );

    fixture.storage.configure(ObjectStorageState {
        bucket: Some(fixture.bucket.clone()),
        ..Default::default()
    });
    let waiting = fixture.upload_to(workspace, b"waits for the bucket").await;
    fixture.storage.configure(ObjectStorageState {
        bucket: Some(fixture.bucket.clone()),
        workspace_buckets: [(
            workspace.to_string(),
            WorkspaceBucket {
                bucket: S3Bucket::new(S3Config {
                    endpoint: "http://127.0.0.1:9".to_owned(),
                    ..fixture.bucket.config().clone()
                })
                .unwrap(),
                active: true,
            },
        )]
        .into(),
        ..Default::default()
    });
    fixture.store.move_blobs().await.unwrap();
    let status = fixture
        .store
        .workspace_status(&workspace.to_string())
        .unwrap();
    assert!(status.error.is_some());
    assert_eq!(status.remaining, 1);
    assert_eq!(fixture.read(&waiting).await, b"waits for the bucket");
}

#[test]
fn a_workspace_bucket_must_be_public_https() {
    let config = |endpoint: &str| S3Config {
        endpoint: endpoint.to_owned(),
        region: String::new(),
        bucket: "files".to_owned(),
        prefix: String::new(),
        access_key_id: "key".to_owned(),
        secret_access_key: "secret".to_owned(),
        path_style: true,
    };
    for endpoint in [
        "http://s3.example.com",
        "https://127.0.0.1",
        "https://10.0.0.2:9000",
        "https://169.254.169.254",
        "https://[::1]",
    ] {
        assert!(
            S3Bucket::new_public(config(endpoint)).is_err(),
            "{endpoint}"
        );
    }
    assert!(S3Bucket::new_public(config("https://s3.example.com")).is_ok());
    assert!(S3Bucket::new(config("http://127.0.0.1:9000")).is_ok());
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
        ..Default::default()
    });
    let backups_root = root.path().join("backups");
    let service = BackupService::new(&backups_root, &attachments)
        .with_object_storage(storage)
        .with_app_key(Some([5; 32]));

    let snapshot = service.create(&database).await.unwrap();
    assert!(snapshot.in_object_storage);
    // The encrypted files go to the bucket as they are and come back through the download cache.
    assert!(snapshot.manifest.encryption.is_some());
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

#[tokio::test]
async fn deleting_a_backup_that_is_local_and_in_s3_deletes_both_copies() {
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
    let bucket = bucket().await;
    let storage = ObjectStorage::default();
    storage.configure(ObjectStorageState {
        bucket: Some(bucket.clone()),
        attachments: false,
        backups: true,
        ..Default::default()
    });
    let backups_root = root.path().join("backups");
    let service = BackupService::new(&backups_root, &attachments).with_object_storage(storage);
    let snapshot = service.create(&database).await.unwrap();
    // A failed local remove after the upload leaves the same snapshot on this server too.
    service.verify(&snapshot.id).await.unwrap();
    let local = backups_root.join("snapshots").join(&snapshot.id);
    copy_dir(&backups_root.join("downloads").join(&snapshot.id), &local);

    service.delete(&snapshot.id).await.unwrap();
    assert!(!local.exists());
    assert!(bucket.list("backups/").await.unwrap().is_empty());
    assert!(service.list().await.unwrap().is_empty());
}

fn copy_dir(from: &std::path::Path, to: &std::path::Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}
