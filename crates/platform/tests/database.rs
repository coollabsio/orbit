use std::time::Duration;

use orbit_platform::{
    Database, DatabaseConfig, DatabaseError, Migration, MigrationError, MigrationRunner,
    TestDatabase,
};

const OWNERSHIP_CHILD_PATH: &str = "ORBIT_TEST_OWNERSHIP_CHILD_PATH";
static PROCESS_OWNERSHIP_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[tokio::test]
async fn opens_sqlite_with_required_pragmas() {
    let db = TestDatabase::new().await.unwrap();

    assert_eq!(
        db.scalar::<String>("PRAGMA journal_mode")
            .await
            .unwrap()
            .to_lowercase(),
        "wal"
    );
    assert_eq!(db.scalar::<i64>("PRAGMA foreign_keys").await.unwrap(), 1);
    assert_eq!(
        db.scalar::<i64>("PRAGMA busy_timeout").await.unwrap(),
        5_000
    );
}

#[tokio::test]
async fn creates_the_platform_schema() {
    let db = TestDatabase::new().await.unwrap();

    for expected in [
        "schema_migrations",
        "installation_state",
        "jobs",
        "job_attempts",
        "schedules",
        "audit_events",
        "outbox_events",
        "attachment_blobs",
        "pending_uploads",
    ] {
        let count = db
            .scalar::<i64>(&format!(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = '{expected}'"
            ))
            .await
            .unwrap();
        assert_eq!(count, 1, "missing {expected}");
    }
}

#[tokio::test]
async fn platform_schema_supports_durable_job_and_schedule_semantics() {
    let db = TestDatabase::new().await.unwrap();
    db.execute(
        "INSERT INTO schedules (\
            id, workspace_id, job_kind, payload_json, schedule, next_run_at,\
            catch_up_mode, consecutive_failures, enabled, updated_at\
         ) VALUES ('schedule-1', 'workspace-1', 'cleanup', '{}', '0 * * * *', 100,\
             'latest', 0, 1, 1);\
         INSERT INTO jobs (\
            id, kind, payload_json, state, priority, attempt_count, max_attempts,\
            available_at, claim_token, dead_at, incident_hold, created_at, updated_at\
         ) VALUES ('dead-1', 'cleanup', '{}', 'dead', 'critical', 8, 8, 1,\
             'claim-1', 1, 1, 1, 1);\
         INSERT INTO jobs (\
            id, kind, payload_json, state, priority, attempt_count, max_attempts,\
            available_at, manual_retry_of, created_at, updated_at\
         ) VALUES ('retry-1', 'cleanup', '{}', 'queued', 'normal', 0, 8, 1,\
             'dead-1', 1, 1);\
         INSERT INTO jobs (\
            id, kind, payload_json, state, priority, attempt_count, max_attempts,\
            available_at, schedule_id, scheduled_for, created_at, updated_at\
         ) VALUES ('run-1', 'cleanup', '{}', 'queued', 'low', 0, 8, 100,\
             'schedule-1', 100, 1, 1);",
    )
    .await
    .unwrap();

    let duplicate_occurrence = db
        .execute(
            "INSERT INTO jobs (\
                id, kind, payload_json, state, priority, attempt_count, max_attempts,\
                available_at, schedule_id, scheduled_for, created_at, updated_at\
             ) VALUES ('run-2', 'cleanup', '{}', 'queued', 'low', 0, 8, 100,\
                 'schedule-1', 100, 1, 1);",
        )
        .await;

    assert!(duplicate_occurrence.is_err());
}

#[tokio::test]
async fn audit_events_store_outcomes_and_request_ids() {
    let db = TestDatabase::new().await.unwrap();

    db.execute(
        "INSERT INTO audit_events (\
            id, actor_id, action, outcome, resource_type, resource_id, request_id,\
            metadata_json, occurred_at\
         ) VALUES ('audit-1', 'user-1', 'session.revoke', 'succeeded', 'session',\
             'session-1', 'request-1', '{}', 1)",
    )
    .await
    .unwrap();

    assert_eq!(
        db.scalar::<String>("SELECT request_id FROM audit_events WHERE id = 'audit-1'")
            .await
            .unwrap(),
        "request-1"
    );
}

#[tokio::test]
async fn rejects_a_changed_applied_migration() {
    let db = TestDatabase::new().await.unwrap();
    db.execute("UPDATE schema_migrations SET checksum = 'changed' WHERE version = 1")
        .await
        .unwrap();

    let error = MigrationRunner::embedded(env!("CARGO_PKG_VERSION"))
        .run(&db)
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        MigrationError::ChecksumMismatch { version: 1, .. }
    ));
}

#[tokio::test]
async fn rejects_a_schema_newer_than_the_binary() {
    let db = TestDatabase::new().await.unwrap();
    db.execute(
        "INSERT INTO schema_migrations (version, checksum, applied_at, app_version) \
         VALUES (999, 'future', 0, 'future')",
    )
    .await
    .unwrap();

    let error = MigrationRunner::embedded(env!("CARGO_PKG_VERSION"))
        .run(&db)
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        MigrationError::SchemaNewer {
            database_version: 999,
            binary_version: 4
        }
    ));
}

#[tokio::test]
async fn rejects_an_incompatible_preexisting_schema() {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::open(&DatabaseConfig::new(directory.path().join("db.sqlite")))
        .await
        .unwrap();
    database
        .execute(
            "CREATE TABLE jobs (\
                id TEXT PRIMARY KEY,\
                state TEXT NOT NULL,\
                available_at INTEGER NOT NULL,\
                lease_expires_at INTEGER\
            )",
        )
        .await
        .unwrap();

    let result = MigrationRunner::embedded(env!("CARGO_PKG_VERSION"))
        .run(&database)
        .await;

    assert!(result.is_err());
    assert_eq!(
        database
            .scalar::<i64>("SELECT COUNT(*) FROM schema_migrations")
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn rejects_duplicate_migration_versions_before_touching_the_schema() {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::open(&DatabaseConfig::new(directory.path().join("db.sqlite")))
        .await
        .unwrap();
    let runner = MigrationRunner::new(
        "test",
        vec![
            Migration::new(1, "CREATE TABLE first (id INTEGER);", false),
            Migration::new(1, "CREATE TABLE second (id INTEGER);", false),
        ],
    );

    let error = runner.run(&database).await.unwrap_err();

    assert!(matches!(
        error,
        MigrationError::DuplicateVersion { version: 1 }
    ));
    assert_eq!(
        database
            .scalar::<i64>(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' \
                 AND name = 'schema_migrations'"
            )
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn rolls_back_a_failing_transactional_migration() {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::open(&DatabaseConfig::new(directory.path().join("db.sqlite")))
        .await
        .unwrap();
    let migration = Migration::new(
        1,
        "CREATE TABLE rollback_probe (id INTEGER PRIMARY KEY);\
         INSERT INTO table_that_does_not_exist (id) VALUES (1);",
        false,
    );

    assert!(
        MigrationRunner::new("test", vec![migration])
            .run(&database)
            .await
            .is_err()
    );
    let table_count: i64 = database
        .scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'rollback_probe'",
        )
        .await
        .unwrap();
    assert_eq!(table_count, 0);
}

#[tokio::test]
async fn temporary_databases_use_isolated_paths_and_contents() {
    let first = TestDatabase::new().await.unwrap();
    let second = TestDatabase::new().await.unwrap();

    assert_ne!(first.path(), second.path());
    first
        .execute("INSERT INTO installation_state (id, initialized) VALUES (1, 1)")
        .await
        .unwrap();
    assert_eq!(
        second
            .scalar::<i64>("SELECT COUNT(*) FROM installation_state")
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn one_process_owns_a_database_path() {
    let directory = tempfile::tempdir().unwrap();
    let config = DatabaseConfig::new(directory.path().join("db.sqlite"));
    let _first = Database::open(&config).await.unwrap();

    let error = Database::open(&config).await.unwrap_err();

    assert!(matches!(error, DatabaseError::AlreadyOwned { .. }));
}

#[tokio::test]
async fn cloned_database_handles_keep_the_ownership_lock() {
    let _process_guard = PROCESS_OWNERSHIP_TEST_LOCK.lock().await;
    let directory = tempfile::tempdir().unwrap();
    let config = DatabaseConfig::new(directory.path().join("db.sqlite"));
    let database = Database::open(&config).await.unwrap();
    let remaining_handle = database.clone();
    drop(database);

    let error = Database::open(&config).await.unwrap_err();

    assert!(matches!(error, DatabaseError::AlreadyOwned { .. }));
    drop(remaining_handle);
    Database::open(&config).await.unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn symlink_alias_cannot_bypass_database_ownership() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("db.sqlite");
    let alias = directory.path().join("alias.sqlite");
    let _database = Database::open(&DatabaseConfig::new(&path)).await.unwrap();
    std::os::unix::fs::symlink(&path, &alias).unwrap();

    let error = Database::open(&DatabaseConfig::new(alias))
        .await
        .unwrap_err();

    assert!(matches!(error, DatabaseError::AlreadyOwned { .. }));
}

#[tokio::test]
async fn hard_link_alias_cannot_bypass_database_ownership() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("db.sqlite");
    let alias = directory.path().join("alias.sqlite");
    let _database = Database::open(&DatabaseConfig::new(&path)).await.unwrap();
    std::fs::hard_link(&path, &alias).unwrap();

    let error = Database::open(&DatabaseConfig::new(alias))
        .await
        .unwrap_err();

    assert!(matches!(error, DatabaseError::AlreadyOwned { .. }));
}

#[cfg(unix)]
#[tokio::test]
async fn another_process_cannot_own_the_database_through_an_alias() {
    if let Some(path) = std::env::var_os(OWNERSHIP_CHILD_PATH) {
        let error = Database::open(&DatabaseConfig::new(path))
            .await
            .unwrap_err();
        assert!(matches!(error, DatabaseError::AlreadyOwned { .. }));
        return;
    }

    let _process_guard = PROCESS_OWNERSHIP_TEST_LOCK.lock().await;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("db.sqlite");
    let alias = directory.path().join("alias.sqlite");
    let _database = Database::open(&DatabaseConfig::new(&path)).await.unwrap();
    std::os::unix::fs::symlink(&path, &alias).unwrap();

    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "another_process_cannot_own_the_database_through_an_alias",
            "--nocapture",
        ])
        .env(OWNERSHIP_CHILD_PATH, alias)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "child process failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test]
async fn exposes_pending_destructive_migrations_without_running_backups() {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::open(&DatabaseConfig {
        path: directory.path().join("db.sqlite"),
        max_connections: 1,
        busy_timeout: Duration::from_secs(1),
    })
    .await
    .unwrap();
    let runner = MigrationRunner::new(
        "test",
        vec![Migration::new(
            1,
            "CREATE TABLE example (id INTEGER);",
            true,
        )],
    );

    let pending = runner.pending(&database).await.unwrap();

    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].version, 1);
    assert!(pending[0].destructive);
}
