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

const APP_KEY: [u8; 32] = [7; 32];
const OTHER_KEY: [u8; 32] = [9; 32];

impl Fixture {
    async fn new() -> Self {
        Self::with_key(None).await
    }

    /// A fixture whose service encrypts new backups with `key`.
    async fn with_key(key: Option<[u8; 32]>) -> Self {
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

        let service =
            BackupService::new(root.path().join("backups"), &attachments).with_app_key(key);
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
    let fixture = Fixture::with_key(Some(APP_KEY)).await;
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
            .any(|file| file.path == "database/users.copy.zst")
    );
    // Every file of the snapshot is encrypted, the table list too.
    assert!(snapshot.manifest.encryption.is_some());
    let users = fs::read(snapshot.path.join("database/users.copy.zst")).unwrap();
    assert!(!contains(&users, b"kept@example.com"));
    let tables = fs::read(snapshot.path.join("database/tables.json")).unwrap();
    assert!(serde_json::from_slice::<serde_json::Value>(&tables).is_err());
    assert!(!contains(&tables, b"normalized_email"));

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
        .scalar("SELECT COUNT(*) FROM schema_migrations WHERE version = 95")
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
async fn postgres_restore_reads_uncompressed_table_files_of_older_snapshots() {
    if !orbit_platform::testing_postgres() {
        return;
    }
    let fixture = Fixture::new().await;
    fixture
        .database
        .execute(
            "INSERT INTO users (id, email, normalized_email, display_name, password_hash, \
             created_at, updated_at) VALUES ('kept', 'k@example.com', 'k@example.com', 'Kept', 'x', 1, 1)",
        )
        .await
        .unwrap();
    let snapshot = fixture.service.create(&fixture.database).await.unwrap();
    let mut manifest = snapshot.manifest;
    for file in &mut manifest.files {
        let Some(plain) = file.path.strip_suffix(".zst") else {
            continue;
        };
        let compressed = snapshot.path.join(&file.path);
        let contents = zstd::decode_all(fs::File::open(&compressed).unwrap()).unwrap();
        fs::remove_file(&compressed).unwrap();
        fs::write(snapshot.path.join(plain), &contents).unwrap();
        file.path = plain.to_owned();
        file.sha256 = checksum(&contents);
        file.byte_size = contents.len() as u64;
    }
    fs::write(
        snapshot.path.join("manifest.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    fixture.database.execute("DELETE FROM users").await.unwrap();

    fixture
        .service
        .restore_postgres(&snapshot.id, &fixture.database)
        .await
        .unwrap();

    let users: String = fixture
        .database
        .scalar("SELECT string_agg(id, ',' ORDER BY id) FROM users")
        .await
        .unwrap();
    assert_eq!(users, "kept");
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
    assert_eq!(snapshot.manifest.schema_version, 95);
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
async fn snapshot_leaves_out_upload_staging_files() {
    let fixture = Fixture::new().await;
    let staging = fixture._root.path().join("attachments/temporary");
    fs::create_dir_all(&staging).unwrap();
    fs::write(staging.join("upload-in-progress"), b"half an upload").unwrap();

    let snapshot = fixture.service.create(&fixture.database).await.unwrap();

    let attachments: Vec<_> = snapshot
        .manifest
        .files
        .iter()
        .map(|file| file.path.as_str())
        .filter(|path| path.starts_with("attachments/"))
        .collect();
    assert_eq!(attachments, ["attachments/record.txt"]);
    assert!(!snapshot.path.join("attachments/temporary").exists());
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

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

/// Restores `id` into a fresh SQLite target, or into the fixture's Postgres database.
async fn restore_into(
    fixture: &Fixture,
    service: &BackupService,
    id: &str,
) -> Result<(), BackupError> {
    if orbit_platform::testing_postgres() {
        service.restore_postgres(id, &fixture.database).await
    } else {
        service
            .restore_to(
                id,
                &fixture._root.path().join("restored.sqlite"),
                &fixture._root.path().join("restored-attachments"),
            )
            .await
    }
}

/// The restored `installation_state.initialized`, from the target `restore_into` used.
async fn restored_initialized(fixture: &Fixture) -> i64 {
    let query = "SELECT initialized FROM installation_state WHERE id = 1";
    if orbit_platform::testing_postgres() {
        fixture.database.scalar(query).await.unwrap()
    } else {
        Database::open(&DatabaseConfig::new(
            fixture._root.path().join("restored.sqlite"),
        ))
        .await
        .unwrap()
        .scalar(query)
        .await
        .unwrap()
    }
}

#[tokio::test]
async fn an_encrypted_snapshot_hides_its_files_and_restores_with_the_key() {
    if orbit_platform::testing_postgres() {
        return; // Reads the SQLite file; Postgres: postgres_restore_replaces_the_database_and_attachments.
    }
    let fixture = Fixture::with_key(Some(APP_KEY)).await;
    let snapshot = fixture.service.create(&fixture.database).await.unwrap();

    let encryption = snapshot.manifest.encryption.as_ref().unwrap();
    assert_eq!(encryption.algorithm, "chacha20poly1305-stream");
    let database = fs::read(snapshot.path.join("database.sqlite")).unwrap();
    assert!(!database.starts_with(b"SQLite format 3"));
    let attachment = fs::read(snapshot.path.join("attachments/record.txt")).unwrap();
    assert!(!contains(&attachment, b"attachment contents"));
    // The checksums cover the stored (encrypted) bytes, so verify needs no key.
    let file = snapshot
        .manifest
        .files
        .iter()
        .find(|file| file.path == "attachments/record.txt")
        .unwrap();
    assert_eq!(file.sha256, checksum(&attachment));
    BackupService::new(
        fixture._root.path().join("backups"),
        fixture._root.path().join("attachments"),
    )
    .verify(&snapshot.id)
    .await
    .unwrap();
    let manifest = fs::read_to_string(snapshot.path.join("manifest.json")).unwrap();
    assert!(manifest.contains("key_sealed"));

    restore_into(&fixture, &fixture.service, &snapshot.id)
        .await
        .unwrap();
    assert_eq!(restored_initialized(&fixture).await, 1);
    assert_eq!(
        fs::read(fixture._root.path().join("restored-attachments/record.txt")).unwrap(),
        b"attachment contents"
    );
}

#[tokio::test]
async fn an_encrypted_snapshot_round_trips_large_and_empty_files() {
    if orbit_platform::testing_postgres() {
        return; // Attachment files only; the Postgres COPY files are covered above.
    }
    let fixture = Fixture::with_key(Some(APP_KEY)).await;
    let attachments = fixture._root.path().join("attachments");
    let large: Vec<u8> = (0..(200 * 1024 + 17))
        .map(|index| (index % 251) as u8)
        .collect();
    fs::write(attachments.join("large.bin"), &large).unwrap();
    fs::write(attachments.join("exact.bin"), vec![1_u8; 64 * 1024]).unwrap();
    fs::write(attachments.join("empty.bin"), b"").unwrap();
    let snapshot = fixture.service.create(&fixture.database).await.unwrap();

    restore_into(&fixture, &fixture.service, &snapshot.id)
        .await
        .unwrap();
    let restored = fixture._root.path().join("restored-attachments");
    assert_eq!(fs::read(restored.join("large.bin")).unwrap(), large);
    assert_eq!(
        fs::read(restored.join("exact.bin")).unwrap(),
        vec![1_u8; 64 * 1024]
    );
    assert_eq!(fs::read(restored.join("empty.bin")).unwrap(), b"");
}

#[tokio::test]
async fn an_unencrypted_backup_still_restores() {
    let fixture = Fixture::new().await;
    let snapshot = fixture.service.create(&fixture.database).await.unwrap();
    assert!(snapshot.manifest.encryption.is_none());
    // Backups from before encryption have no such field at all.
    let manifest = fs::read_to_string(snapshot.path.join("manifest.json")).unwrap();
    assert!(!manifest.contains("encryption"));

    let keyed = BackupService::new(
        fixture._root.path().join("backups"),
        fixture._root.path().join("attachments"),
    )
    .with_app_key(Some(APP_KEY));
    restore_into(&fixture, &keyed, &snapshot.id).await.unwrap();
    assert_eq!(restored_initialized(&fixture).await, 1);
}

#[tokio::test]
async fn an_encrypted_backup_needs_the_app_key() {
    let fixture = Fixture::with_key(Some(APP_KEY)).await;
    let snapshot = fixture.service.create(&fixture.database).await.unwrap();
    let keyless = BackupService::new(
        fixture._root.path().join("backups"),
        fixture._root.path().join("attachments"),
    );

    let error = restore_into(&fixture, &keyless, &snapshot.id)
        .await
        .unwrap_err();

    assert!(matches!(error, BackupError::KeyRequired { .. }), "{error}");
}

#[tokio::test]
async fn a_different_app_key_is_refused_before_the_target_changes() {
    let fixture = Fixture::with_key(Some(APP_KEY)).await;
    let snapshot = fixture.service.create(&fixture.database).await.unwrap();
    let attachments = fixture._root.path().join("attachments");
    fs::write(attachments.join("record.txt"), b"changed after the backup").unwrap();
    fixture
        .database
        .execute("UPDATE installation_state SET initialized = 0 WHERE id = 1")
        .await
        .unwrap();
    let target_database = fixture._root.path().join("target.sqlite");
    let target_attachments = fixture._root.path().join("target-attachments");
    fs::write(&target_database, b"unchanged database").unwrap();
    fs::create_dir_all(&target_attachments).unwrap();
    fs::write(target_attachments.join("old.txt"), b"unchanged attachment").unwrap();
    let wrong = BackupService::new(fixture._root.path().join("backups"), &attachments)
        .with_app_key(Some(OTHER_KEY));

    let error = if orbit_platform::testing_postgres() {
        wrong
            .restore_postgres(&snapshot.id, &fixture.database)
            .await
    } else {
        wrong
            .restore_to(&snapshot.id, &target_database, &target_attachments)
            .await
    }
    .unwrap_err();

    assert!(matches!(error, BackupError::KeyMismatch { .. }), "{error}");
    assert_eq!(fs::read(&target_database).unwrap(), b"unchanged database");
    assert_eq!(
        fs::read(target_attachments.join("old.txt")).unwrap(),
        b"unchanged attachment"
    );
    assert_eq!(
        fs::read(attachments.join("record.txt")).unwrap(),
        b"changed after the backup"
    );
    let initialized: i64 = fixture
        .database
        .scalar("SELECT initialized FROM installation_state WHERE id = 1")
        .await
        .unwrap();
    assert_eq!(initialized, 0);
}

#[tokio::test]
async fn a_tampered_sealed_key_is_refused() {
    let fixture = Fixture::with_key(Some(APP_KEY)).await;
    let snapshot = fixture.service.create(&fixture.database).await.unwrap();
    // The sealed key belongs to its backup id: copying it into another manifest fails to open.
    let other = fixture.service.create(&fixture.database).await.unwrap();
    let mut manifest = other.manifest.clone();
    manifest.encryption = snapshot.manifest.encryption.clone();
    fs::write(
        other.path.join("manifest.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();

    let error = restore_into(&fixture, &fixture.service, &other.id)
        .await
        .unwrap_err();

    assert!(matches!(error, BackupError::KeyMismatch { .. }), "{error}");
}
