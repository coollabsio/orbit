use std::fs;

use orbit_platform::{
    BackupError, BackupKind, BackupService, Database, DatabaseConfig, MigrationRunner,
};
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

struct Fixture {
    _root: tempfile::TempDir,
    database: Database,
    service: BackupService,
}

impl Fixture {
    async fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let database = Database::open(&DatabaseConfig::new(root.path().join("orbit.sqlite")))
            .await
            .unwrap();
        MigrationRunner::embedded(env!("CARGO_PKG_VERSION"))
            .run(&database)
            .await
            .unwrap();
        database
            .execute("INSERT INTO installation_state (id, initialized) VALUES (1, 1)")
            .await
            .unwrap();

        let attachments = root.path().join("attachments");
        fs::create_dir_all(&attachments).unwrap();
        fs::write(attachments.join("record.txt"), b"attachment contents").unwrap();

        let service = BackupService::new(root.path().join("backups"), &attachments);
        Self {
            _root: root,
            database,
            service,
        }
    }
}

/// Data, attachments, identity sequences, triggers and collab epochs come back as they were.
#[tokio::test]
async fn postgres_restore_replaces_the_database_and_attachments() {
    if !orbit_platform::testing_postgres() {
        return; // SQLite restores are covered by the tests below.
    }
    let fixture = Fixture::new().await;
    fixture
        .database
        .execute(
            "INSERT INTO users (id, email, normalized_email, display_name, password_hash, \
             created_at, updated_at) VALUES ('kept', 'Kept@example.com', 'kept@example.com', \
             'Kept ü', 'x', 1, 1); \
             INSERT INTO workspaces (id, name, created_at, updated_at, owner_membership_id) \
             VALUES ('w', 'W', 1, 1, 'm'); \
             INSERT INTO memberships (id, workspace_id, user_id, role, created_at, updated_at) \
             VALUES ('m', 'w', 'kept', 'owner', 1, 1)",
        )
        .await
        .unwrap();
    let epoch: String = fixture
        .database
        .scalar("SELECT generation FROM page_collab_meta")
        .await
        .unwrap();
    let snapshot = fixture.service.create(&fixture.database).await.unwrap();
    assert_eq!(
        snapshot.manifest.database,
        orbit_platform::Backend::Postgres
    );
    assert!(
        snapshot
            .manifest
            .files
            .iter()
            .any(|file| file.path == "database/users.copy")
    );

    fixture
        .database
        .execute(
            "UPDATE users SET display_name = 'Changed' WHERE id = 'kept'; \
             INSERT INTO users (id, email, normalized_email, display_name, password_hash, \
             created_at, updated_at) VALUES ('later', 'l@example.com', 'l@example.com', 'L', 'x', 2, 2)",
        )
        .await
        .unwrap();
    let attachments = fixture._root.path().join("attachments");
    fs::write(attachments.join("record.txt"), b"changed").unwrap();
    fs::write(attachments.join("later.txt"), b"later").unwrap();

    fixture
        .service
        .restore_postgres(&snapshot.id, &fixture.database)
        .await
        .unwrap();

    let users: String = fixture
        .database
        .scalar("SELECT string_agg(id || ':' || display_name, ',' ORDER BY id) FROM users")
        .await
        .unwrap();
    assert_eq!(users, "kept:Kept ü");
    // The workspace owner is a deferred key, checked before the triggers come back.
    let owner: String = fixture
        .database
        .scalar("SELECT owner_membership_id FROM workspaces WHERE id = 'w'")
        .await
        .unwrap();
    assert_eq!(owner, "m");
    assert_eq!(
        fs::read(attachments.join("record.txt")).unwrap(),
        b"attachment contents"
    );
    assert!(!attachments.join("later.txt").exists());
    let initialized: i64 = fixture
        .database
        .scalar("SELECT initialized FROM installation_state WHERE id = 1")
        .await
        .unwrap();
    assert_eq!(initialized, 1);
    let migrations: i64 = fixture
        .database
        .scalar("SELECT COUNT(*) FROM schema_migrations WHERE version = 61")
        .await
        .unwrap();
    assert_eq!(migrations, 1);
    let disabled_triggers: i64 = fixture
        .database
        .scalar("SELECT COUNT(*) FROM pg_trigger WHERE NOT tgisinternal AND tgenabled <> 'O'")
        .await
        .unwrap();
    assert_eq!(disabled_triggers, 0);
    let next_id: i64 = fixture
        .database
        .scalar("SELECT nextval(pg_get_serial_sequence('instance_settings', 'id'))")
        .await
        .unwrap();
    assert_eq!(next_id, 2);
    let new_epoch: String = fixture
        .database
        .scalar("SELECT generation FROM page_collab_meta")
        .await
        .unwrap();
    assert_ne!(new_epoch, epoch);
}

#[tokio::test]
async fn a_snapshot_restores_only_into_its_own_backend() {
    let fixture = Fixture::new().await;
    let snapshot = fixture.service.create(&fixture.database).await.unwrap();

    let error = if orbit_platform::testing_postgres() {
        fixture
            .service
            .restore(&snapshot.id, &fixture._root.path().join("other.sqlite"))
            .await
    } else {
        fixture
            .service
            .restore_postgres(&snapshot.id, &fixture.database)
            .await
    }
    .unwrap_err();
    assert!(matches!(error, BackupError::WrongBackend { .. }), "{error}");
}

fn checksum(contents: &[u8]) -> String {
    format!("{:x}", Sha256::digest(contents))
}

#[tokio::test]
async fn snapshot_manifest_checksums_the_database_and_attachment() {
    if orbit_platform::testing_postgres() {
        return; // Checks the SQLite file of a snapshot.
    }
    let fixture = Fixture::new().await;

    let snapshot = fixture.service.create(&fixture.database).await.unwrap();

    assert_eq!(snapshot.manifest.kind, BackupKind::Snapshot);
    assert_eq!(snapshot.manifest.schema_version, 61);
    assert_eq!(
        snapshot.manifest.application_version,
        env!("CARGO_PKG_VERSION")
    );
    let database = snapshot
        .manifest
        .files
        .iter()
        .find(|file| file.path == "database.sqlite")
        .unwrap();
    assert_eq!(
        database.sha256,
        checksum(&fs::read(snapshot.path.join(&database.path)).unwrap())
    );
    let attachment = snapshot
        .manifest
        .files
        .iter()
        .find(|file| file.path == "attachments/record.txt")
        .unwrap();
    assert_eq!(attachment.sha256, checksum(b"attachment contents"));
}

#[tokio::test]
async fn verify_and_restore_reject_a_newer_schema_before_touching_the_target() {
    let fixture = Fixture::new().await;
    let snapshot = fixture.service.create(&fixture.database).await.unwrap();
    let mut manifest = snapshot.manifest;
    manifest.schema_version = 999;
    fs::write(
        snapshot.path.join("manifest.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();

    assert!(matches!(
        fixture.service.verify(&snapshot.id).await,
        Err(BackupError::UnsupportedSchema { .. })
    ));

    let target_database = fixture._root.path().join("target.sqlite");
    let target_attachments = fixture._root.path().join("target-attachments");
    fs::write(&target_database, b"unchanged database").unwrap();
    fs::create_dir_all(&target_attachments).unwrap();
    fs::write(target_attachments.join("old.txt"), b"unchanged attachment").unwrap();
    let before = fs::read(&target_database).unwrap();

    assert!(matches!(
        fixture
            .service
            .restore_to(&snapshot.id, &target_database, &target_attachments)
            .await,
        Err(BackupError::UnsupportedSchema { .. })
    ));
    assert_eq!(fs::read(&target_database).unwrap(), before);
    assert_eq!(
        fs::read(target_attachments.join("old.txt")).unwrap(),
        b"unchanged attachment"
    );
}

#[tokio::test]
async fn verify_rejects_a_damaged_attachment() {
    let fixture = Fixture::new().await;
    let snapshot = fixture.service.create(&fixture.database).await.unwrap();
    fs::write(snapshot.path.join("attachments/record.txt"), b"damaged").unwrap();

    let error = fixture.service.verify(&snapshot.id).await.unwrap_err();

    assert!(matches!(error, BackupError::ChecksumMismatch { .. }));
}

#[tokio::test]
async fn restore_refuses_a_database_with_a_held_ownership_lock() {
    if orbit_platform::testing_postgres() {
        return; // SQLite file restore.
    }
    let fixture = Fixture::new().await;
    let snapshot = fixture.service.create(&fixture.database).await.unwrap();

    let error = fixture
        .service
        .restore(&snapshot.id, fixture.database.path())
        .await
        .unwrap_err();

    assert!(matches!(error, BackupError::RestoreTargetOwned { .. }));
}

#[tokio::test]
async fn restore_rotates_collaborative_document_epochs() {
    if orbit_platform::testing_postgres() {
        return; // SQLite file restore; Postgres: postgres_restore_replaces_the_database_and_attachments.
    }
    // Clients still hold post-backup document state; a new generation makes them reset.
    let fixture = Fixture::new().await;
    let before: String = fixture
        .database
        .scalar("SELECT generation FROM page_collab_meta WHERE id = 1")
        .await
        .unwrap();
    let snapshot = fixture.service.create(&fixture.database).await.unwrap();
    let restored_database = fixture._root.path().join("restored.sqlite");
    let restored_attachments = fixture._root.path().join("restored-attachments");
    fixture
        .service
        .restore_to(&snapshot.id, &restored_database, &restored_attachments)
        .await
        .unwrap();
    let database = Database::open(&DatabaseConfig::new(&restored_database))
        .await
        .unwrap();
    let after: String = database
        .scalar("SELECT generation FROM page_collab_meta WHERE id = 1")
        .await
        .unwrap();
    assert_ne!(before, after);
    assert_eq!(after.len(), 32);
}

#[tokio::test]
async fn restore_recovers_the_database_record_and_attachment() {
    if orbit_platform::testing_postgres() {
        return; // SQLite file restore; Postgres: postgres_restore_replaces_the_database_and_attachments.
    }
    let fixture = Fixture::new().await;
    let snapshot = fixture.service.create(&fixture.database).await.unwrap();
    let restored_database = fixture._root.path().join("restored.sqlite");
    let restored_attachments = fixture._root.path().join("restored-attachments");

    fixture
        .service
        .restore_to(&snapshot.id, &restored_database, &restored_attachments)
        .await
        .unwrap();

    let database = Database::open(&DatabaseConfig::new(&restored_database))
        .await
        .unwrap();
    assert_eq!(
        database
            .scalar::<i64>("SELECT initialized FROM installation_state WHERE id = 1")
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        fs::read(restored_attachments.join("record.txt")).unwrap(),
        b"attachment contents"
    );
}

#[tokio::test]
async fn restore_rejects_a_database_nested_in_the_attachment_target() {
    if orbit_platform::testing_postgres() {
        return; // SQLite file restore.
    }
    let fixture = Fixture::new().await;
    let snapshot = fixture.service.create(&fixture.database).await.unwrap();
    let restored_attachments = fixture._root.path().join("overlapping-restore");
    let restored_database = restored_attachments.join("orbit.sqlite");

    let error = fixture
        .service
        .restore_to(&snapshot.id, &restored_database, &restored_attachments)
        .await
        .unwrap_err();

    assert!(matches!(error, BackupError::RestorePathOverlap { .. }));
    assert!(!restored_attachments.exists());
}

#[tokio::test]
async fn restore_removes_sqlite_sidecars_for_any_database_extension() {
    if orbit_platform::testing_postgres() {
        return; // SQLite file restore.
    }
    let fixture = Fixture::new().await;
    let snapshot = fixture.service.create(&fixture.database).await.unwrap();
    let restored_database = fixture._root.path().join("restored.db");
    let restored_attachments = fixture._root.path().join("restored-attachments");
    let wal = fixture._root.path().join("restored.db-wal");
    let shm = fixture._root.path().join("restored.db-shm");
    fs::write(&wal, b"stale wal").unwrap();
    fs::write(&shm, b"stale shm").unwrap();

    fixture
        .service
        .restore_to(&snapshot.id, &restored_database, &restored_attachments)
        .await
        .unwrap();

    assert!(!wal.exists());
    assert!(!shm.exists());
}

#[tokio::test]
async fn backup_ids_cannot_escape_the_backup_root() {
    let fixture = Fixture::new().await;
    let snapshot = fixture.service.create(&fixture.database).await.unwrap();
    let outside = fixture._root.path().join("outside");
    copy_dir(&snapshot.path, &outside);

    let result = fixture.service.verify("../../outside").await;

    assert!(matches!(result, Err(BackupError::NotFound { .. })));
}

#[tokio::test]
async fn manifest_file_paths_cannot_escape_the_snapshot() {
    let fixture = Fixture::new().await;
    let snapshot = fixture.service.create(&fixture.database).await.unwrap();
    let mut manifest = snapshot.manifest;
    let outside = snapshot
        .path
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("outside.sqlite");
    fs::copy(snapshot.path.join(&manifest.files[0].path), &outside).unwrap();
    manifest.files[0].path = "../../outside.sqlite".to_owned();
    fs::write(
        snapshot.path.join("manifest.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();

    let result = fixture.service.verify(&snapshot.id).await;

    assert!(result.is_err());
}

#[tokio::test]
async fn verify_rejects_a_manifest_that_omits_a_snapshot_file() {
    let fixture = Fixture::new().await;
    let snapshot = fixture.service.create(&fixture.database).await.unwrap();
    let mut manifest = snapshot.manifest;
    manifest
        .files
        .retain(|file| file.path != "attachments/record.txt");
    fs::write(
        snapshot.path.join("manifest.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();

    let result = fixture.service.verify(&snapshot.id).await;

    assert!(result.is_err());
}

fn copy_dir(source: &std::path::Path, destination: &std::path::Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

#[tokio::test]
async fn pre_migration_backups_are_listed_separately() {
    let fixture = Fixture::new().await;

    let snapshot = fixture.service.create(&fixture.database).await.unwrap();
    let migration = fixture
        .service
        .create_pre_migration(&fixture.database)
        .await
        .unwrap();

    assert_eq!(fixture.service.list().await.unwrap(), vec![snapshot]);
    assert_eq!(
        fixture.service.list_pre_migration().await.unwrap(),
        vec![migration]
    );
}

#[tokio::test]
async fn same_day_snapshots_are_all_kept() {
    let fixture = Fixture::new().await;

    let first = fixture.service.create(&fixture.database).await.unwrap();
    let second = fixture.service.create(&fixture.database).await.unwrap();
    let third = fixture.service.create(&fixture.database).await.unwrap();

    let ids: Vec<_> = fixture
        .service
        .list()
        .await
        .unwrap()
        .into_iter()
        .map(|snapshot| snapshot.id)
        .collect();
    assert_eq!(ids, vec![third.id, second.id, first.id]);
}

#[tokio::test]
async fn backup_pauses_attachment_mutations_at_the_coordination_guard() {
    let fixture = Fixture::new().await;
    let coordinator = fixture.service.attachment_mutations();
    let mutation = coordinator.begin().await;
    let service = fixture.service.clone();
    let database = fixture.database.clone();

    let backup = tokio::spawn(async move { service.create(&database).await });
    tokio::task::yield_now().await;
    assert!(!backup.is_finished());

    drop(mutation);
    backup.await.unwrap().unwrap();
}

#[tokio::test]
async fn a_cancelled_backup_stops_before_snapshot_io() {
    let fixture = Fixture::new().await;
    let cancellation = CancellationToken::new();
    cancellation.cancel();

    let error = fixture
        .service
        .create_cancellable(&fixture.database, cancellation)
        .await
        .unwrap_err();

    assert!(matches!(error, BackupError::Cancelled));
    assert!(fixture.service.list().await.unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancellation_stops_active_attachment_copy_and_verification() {
    let fixture = Fixture::new().await;
    let attachments = fixture._root.path().join("attachments");
    for index in 0..4_000 {
        fs::write(attachments.join(format!("{index:04}.txt")), b"content").unwrap();
    }
    let cancellation = CancellationToken::new();
    let service = fixture.service.clone();
    let database = fixture.database.clone();
    let backup_cancellation = cancellation.clone();
    let backup = tokio::spawn(async move {
        service
            .create_cancellable(&database, backup_cancellation)
            .await
    });

    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let snapshot_root = fixture._root.path().join("backups/snapshots");
            let copying = fs::read_dir(snapshot_root).is_ok_and(|entries| {
                entries.filter_map(Result::ok).any(|entry| {
                    entry.file_name().to_string_lossy().starts_with('.')
                        && fs::read_dir(entry.path().join("attachments"))
                            .is_ok_and(|mut files| files.next().is_some())
                })
            });
            if copying {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("backup entered cancellable attachment work");
    cancellation.cancel();

    let error = tokio::time::timeout(std::time::Duration::from_secs(2), backup)
        .await
        .expect("active backup observes cancellation")
        .unwrap()
        .unwrap_err();
    assert!(matches!(error, BackupError::Cancelled));
    assert!(fixture.service.list().await.unwrap().is_empty());
}

#[cfg(unix)]
#[tokio::test]
async fn rejects_a_backup_root_nested_under_attachments_before_copying() {
    let fixture = Fixture::new().await;
    let attachments = fixture._root.path().join("overlap-attachments");
    fs::create_dir_all(&attachments).unwrap();
    std::os::unix::fs::symlink("missing", attachments.join("0-unsupported")).unwrap();
    let backup_root = attachments.join("backups");
    let service = BackupService::new(&backup_root, &attachments);

    let error = service.create(&fixture.database).await.unwrap_err();

    assert!(matches!(error, BackupError::PathOverlap { .. }));
    assert!(!backup_root.exists());
}

#[tokio::test]
async fn rejects_attachments_nested_under_the_backup_root() {
    let fixture = Fixture::new().await;
    let backup_root = fixture._root.path().join("outer-backups");
    let attachments = backup_root.join("live-attachments");
    let service = BackupService::new(&backup_root, &attachments);

    let error = service.create(&fixture.database).await.unwrap_err();

    assert!(matches!(error, BackupError::PathOverlap { .. }));
    assert!(!backup_root.exists());
}

#[cfg(unix)]
#[tokio::test]
async fn failed_snapshot_creation_removes_its_hidden_temporary_directory() {
    let fixture = Fixture::new().await;
    let attachments = fixture._root.path().join("bad-attachments");
    fs::create_dir_all(&attachments).unwrap();
    std::os::unix::fs::symlink("missing", attachments.join("unsupported")).unwrap();
    let backup_root = fixture._root.path().join("cleanup-backups");
    let service = BackupService::new(&backup_root, &attachments);

    assert!(service.create(&fixture.database).await.is_err());

    let snapshots = backup_root.join("snapshots");
    assert!(
        !snapshots.exists() || fs::read_dir(snapshots).unwrap().next().is_none(),
        "failed snapshots must not leave hidden temporary trees"
    );
}
