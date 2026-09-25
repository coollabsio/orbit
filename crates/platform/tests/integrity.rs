use std::fs::OpenOptions;
use std::io::{Seek, SeekFrom, Write};

use orbit_platform::{
    Database, DatabaseConfig, HealthCheck, HealthRegistry, IntegrityError, IntegrityService,
    MigrationRunner,
};

#[tokio::test]
async fn quick_and_full_checks_accept_a_healthy_database() {
    let database = orbit_platform::TestDatabase::new().await.unwrap();
    let integrity = IntegrityService::new(database.clone());

    integrity.quick().await.unwrap();
    integrity.full().await.unwrap();
}

#[tokio::test]
async fn full_check_accepts_updated_fts5_tables() {
    // SQLite 3.46.0's PRAGMA integrity_check reports "malformed inverted index" for an FTS5
    // table after two UPDATEs of a row; FTS5's own check (and every query) is fine.
    let database = orbit_platform::TestDatabase::new().await.unwrap();
    database
        .execute(
            "CREATE VIRTUAL TABLE probe_search USING fts5(title, body);\
             INSERT INTO probe_search (rowid, title, body) VALUES (1, 'a', 'x');\
             UPDATE probe_search SET body = 'x0' WHERE rowid = 1;\
             UPDATE probe_search SET body = 'x01' WHERE rowid = 1;",
        )
        .await
        .unwrap();
    IntegrityService::new(database.clone())
        .full()
        .await
        .unwrap();
    let stale: i64 = database
        .scalar("SELECT COUNT(*) FROM probe_search WHERE probe_search MATCH 'x0'")
        .await
        .unwrap();
    assert_eq!(stale, 0);
}

#[tokio::test]
async fn a_failed_quick_check_makes_readiness_fail() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("corrupt.sqlite");
    let database = Database::open(&DatabaseConfig::new(&path)).await.unwrap();
    MigrationRunner::embedded(env!("CARGO_PKG_VERSION"))
        .run(&database)
        .await
        .unwrap();
    database
        .execute(
            "CREATE TABLE corruption_probe (id INTEGER PRIMARY KEY, value TEXT);\
             INSERT INTO corruption_probe (value) VALUES (printf('%.*c', 10000, 'x'));\
             PRAGMA wal_checkpoint(TRUNCATE);",
        )
        .await
        .unwrap();
    drop(database);

    let mut file = OpenOptions::new().write(true).open(&path).unwrap();
    file.seek(SeekFrom::Start(4096)).unwrap();
    file.write_all(&[0; 4096]).unwrap();
    file.sync_all().unwrap();
    drop(file);

    let database = Database::open(&DatabaseConfig::new(&path)).await.unwrap();
    let integrity = IntegrityService::new(database);
    assert!(matches!(
        integrity.quick().await,
        Err(IntegrityError::Failed { .. })
    ));

    let mut health = HealthRegistry::new();
    integrity.register_readiness(&mut health);
    for check in [
        HealthCheck::Migrations,
        HealthCheck::WritableStorage,
        HealthCheck::Scheduler,
        HealthCheck::CriticalConfig,
    ] {
        health.register(check, || async { Ok(()) });
    }

    let readiness = health.readiness().await;
    assert!(!readiness.ready);
    assert!(!readiness.checks["database_integrity"].ready);
}

#[tokio::test]
async fn full_check_rejects_foreign_key_orphans() {
    let database = orbit_platform::TestDatabase::new().await.unwrap();
    database
        .execute(
            "CREATE TABLE integrity_parent (id INTEGER PRIMARY KEY);\
             CREATE TABLE integrity_child (\
                 id INTEGER PRIMARY KEY,\
                 parent_id INTEGER NOT NULL REFERENCES integrity_parent(id)\
             );\
             PRAGMA foreign_keys = OFF;\
             INSERT INTO integrity_child (id, parent_id) VALUES (1, 999);\
             PRAGMA foreign_keys = ON;",
        )
        .await
        .unwrap();

    let error = IntegrityService::new(Database::clone(&database))
        .full()
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        IntegrityError::Failed {
            check: "foreign_key_check",
            ..
        }
    ));
}
