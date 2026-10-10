use std::time::Duration;

use orbit_platform::{
    Database, DatabaseConfig, DatabaseError, Id, Migration, MigrationError, MigrationRunner,
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
async fn github_schema_is_in_one_draft_migration() {
    let db = TestDatabase::new().await.unwrap();
    assert_eq!(
        db.scalar::<i64>("SELECT MAX(version) FROM schema_migrations")
            .await
            .unwrap(),
        53
    );
    assert_eq!(
        db.scalar::<i64>("SELECT COUNT(*) FROM pragma_table_info('github_issue_links') WHERE name IN ('kind', 'pull_state', 'sync_paused')")
            .await
            .unwrap(),
        3
    );
    assert_eq!(
        db.scalar::<i64>("SELECT COUNT(*) FROM pragma_table_info('github_app_registrations') WHERE name = 'public_origin'")
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        db.scalar::<i64>("SELECT COUNT(*) FROM pragma_table_info('github_app_registrations') WHERE name = 'project_id'")
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        db.scalar::<i64>(
            "SELECT COUNT(*) FROM pragma_table_info('tasks') WHERE name = 'source_url'"
        )
        .await
        .unwrap(),
        1
    );
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
            binary_version: 53
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

    assert!(matches!(error, DatabaseError::HardLinked { .. }));
}

#[cfg(unix)]
#[tokio::test]
async fn symlink_alias_in_another_directory_cannot_bypass_database_ownership() {
    let directory = tempfile::tempdir().unwrap();
    let other_directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("db.sqlite");
    let alias = other_directory.path().join("alias.sqlite");
    let _database = Database::open(&DatabaseConfig::new(&path)).await.unwrap();
    std::os::unix::fs::symlink(&path, &alias).unwrap();

    let error = Database::open(&DatabaseConfig::new(alias))
        .await
        .unwrap_err();

    assert!(matches!(error, DatabaseError::AlreadyOwned { .. }));
}

#[tokio::test]
async fn ownership_lock_does_not_lock_the_database_file() {
    // On macOS a lock on the database file itself blocks SQLite's own locks ("database is locked").
    use fs2::FileExt;

    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("db.sqlite");
    let _database = Database::open(&DatabaseConfig::new(&path)).await.unwrap();

    let database_file = std::fs::File::open(&path).unwrap();
    database_file.try_lock_exclusive().unwrap();
    assert!(directory.path().join("db.sqlite.lock").exists());
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

#[tokio::test]
async fn task_relations_migration_rebuilds_statuses_and_seeds_duplicate_statuses() {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::open(&DatabaseConfig::new(directory.path().join("db.sqlite")))
        .await
        .unwrap();
    MigrationRunner::embedded_through("test", 20)
        .run(&database)
        .await
        .unwrap();

    let [user, workspace, membership, live_project, trashed_project]: [Id; 5] =
        std::array::from_fn(|_| Id::new_v7());
    let [
        todo,
        done,
        trashed_todo,
        first_task,
        second_task,
        third_task,
    ]: [Id; 6] = std::array::from_fn(|_| Id::new_v7());
    let seed = format!(
        "INSERT INTO users (id, email, normalized_email, display_name, password_hash, created_at, updated_at)
         VALUES ('{user}', 'owner@example.com', 'owner@example.com', 'Owner', 'x', 1, 1);
         INSERT INTO workspaces (id, name, version, owner_membership_id, created_at, updated_at)
         VALUES ('{workspace}', 'Orbit', 0, '{membership}', 1, 1);
         INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, updated_at)
         VALUES ('{membership}', '{workspace}', '{user}', 'owner', 0, 1, 1);
         INSERT INTO projects (id, workspace_id, name, project_key, color, version, deleted_at, created_at, updated_at)
         VALUES ('{live_project}', '{workspace}', 'Live', 'LIVE', '#000000', 0, NULL, 1, 1),
                ('{trashed_project}', '{workspace}', 'Trashed', 'TRASH', '#000000', 0, 5, 1, 1);
         INSERT INTO task_statuses (id, workspace_id, project_id, name, description, color, category, position, version, created_at, updated_at)
         VALUES ('{todo}', '{workspace}', '{live_project}', 'Todo', '', '#ffffff', 'unstarted', 0, 3, 1, 2),
                ('{done}', '{workspace}', '{live_project}', 'Done', '', '#ffffff', 'completed', 7, 0, 1, 1),
                ('{trashed_todo}', '{workspace}', '{trashed_project}', 'Todo', '', '#ffffff', 'unstarted', 0, 0, 1, 1);
         INSERT INTO tasks (id, workspace_id, project_id, status_id, title, creator_id, created_at, updated_at)
         VALUES ('{first_task}', '{workspace}', '{live_project}', '{todo}', 'First', '{user}', 1, 1),
                ('{second_task}', '{workspace}', '{live_project}', '{done}', 'Second', '{user}', 1, 1),
                ('{third_task}', '{workspace}', '{live_project}', '{todo}', 'Third', '{user}', 1, 1);"
    );
    let mut transaction = database.transaction().await.unwrap();
    sqlx::raw_sql(&seed)
        .execute(&mut *transaction)
        .await
        .unwrap();
    transaction.commit().await.unwrap();

    MigrationRunner::embedded("test")
        .run(&database)
        .await
        .unwrap();

    assert_eq!(
        database
            .scalar::<i64>("SELECT COUNT(*) FROM pragma_foreign_key_check")
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        database
            .scalar::<String>("PRAGMA integrity_check")
            .await
            .unwrap(),
        "ok"
    );
    let statuses: Vec<(String, String, i64, i64)> = sqlx::query_as(
        "SELECT id, category, position, version FROM task_statuses WHERE project_id = ? ORDER BY position",
    )
    .bind(live_project.to_string())
    .fetch_all(database.pool())
    .await
    .unwrap();
    // The third is the Duplicate status of 0021; the fourth is the Triage status of 0046.
    assert_eq!(statuses.len(), 4);
    assert_eq!(statuses[3].1, "triage");
    assert_eq!(
        statuses[..2],
        [
            (todo.to_string(), "unstarted".to_owned(), 0, 3),
            (done.to_string(), "completed".to_owned(), 7, 0),
        ]
    );
    assert_eq!((statuses[2].1.as_str(), statuses[2].2), ("duplicate", 8));
    assert!(
        statuses[2].0.parse::<Id>().is_ok(),
        "seeded id {} must be a canonical UUIDv7",
        statuses[2].0
    );
    let (name, color, description): (String, String, String) =
        sqlx::query_as("SELECT name, color, description FROM task_statuses WHERE id = ?")
            .bind(&statuses[2].0)
            .fetch_one(database.pool())
            .await
            .unwrap();
    assert_eq!(
        (name.as_str(), color.as_str(), description.as_str()),
        ("Duplicate", "#8b8f98", "")
    );
    let trashed_position: i64 = sqlx::query_scalar(
        "SELECT position FROM task_statuses WHERE project_id = ? AND category = 'duplicate'",
    )
    .bind(trashed_project.to_string())
    .fetch_one(database.pool())
    .await
    .unwrap();
    assert_eq!(
        trashed_position, 1,
        "trashed projects get one too, so restore works"
    );

    // The rebuilt table keeps its triggers, FK enforcement and the new invariants.
    for (sql, message) in [
        (
            format!(
                "UPDATE task_statuses SET project_id = '{trashed_project}' WHERE id = '{todo}'"
            ),
            "task status scope is immutable",
        ),
        (
            format!("UPDATE task_statuses SET category = 'duplicate' WHERE id = '{todo}'"),
            "the duplicate status category is immutable",
        ),
        (
            format!(
                "INSERT INTO task_statuses (id, workspace_id, project_id, name, description, color, category, position, version, created_at, updated_at) \
                 VALUES ('{}', '{workspace}', '{live_project}', 'Again', '', '#ffffff', 'duplicate', 9, 0, 1, 1)",
                Id::new_v7()
            ),
            "UNIQUE constraint failed",
        ),
        (
            format!("DELETE FROM task_statuses WHERE id = '{todo}'"),
            "FOREIGN KEY constraint failed",
        ),
    ] {
        let error = database.execute(&sql).await.unwrap_err().to_string();
        assert!(error.contains(message), "{sql}: {error}");
    }

    let relation = |id: Id, source: Id, target: Id, kind: &str, previous: &str| {
        format!(
            "INSERT INTO task_relations (id, workspace_id, task_id, related_task_id, type, previous_status_id, created_by, created_at) \
             VALUES ('{id}', '{workspace}', '{source}', '{target}', '{kind}', {previous}, '{user}', 1)"
        )
    };
    database
        .execute(&relation(
            Id::new_v7(),
            first_task,
            second_task,
            "blocks",
            "NULL",
        ))
        .await
        .unwrap();
    let reverse = database
        .execute(&relation(
            Id::new_v7(),
            second_task,
            first_task,
            "blocks",
            "NULL",
        ))
        .await
        .unwrap_err()
        .to_string();
    assert!(reverse.contains("task_relations_pair"), "{reverse}");
    database
        .execute(&relation(
            Id::new_v7(),
            third_task,
            first_task,
            "duplicate",
            &format!("'{todo}'"),
        ))
        .await
        .unwrap();
    let chained = database
        .execute(&relation(
            Id::new_v7(),
            second_task,
            third_task,
            "duplicate",
            &format!("'{done}'"),
        ))
        .await
        .unwrap_err()
        .to_string();
    assert!(
        chained.contains("duplicate relations cannot chain"),
        "{chained}"
    );
}

#[tokio::test]
async fn pages_migration_scopes_parents_to_their_workspace() {
    // Pinned to 0022: later migrations add space columns every page insert must set.
    let directory = tempfile::tempdir().unwrap();
    let db = Database::open(&DatabaseConfig::new(directory.path().join("db.sqlite")))
        .await
        .unwrap();
    MigrationRunner::embedded_through("test", 22)
        .run(&db)
        .await
        .unwrap();
    assert_eq!(
        db.scalar::<i64>(
            "SELECT COUNT(*) FROM pragma_table_info('pages') WHERE name IN \
             ('content_json', 'content_text', 'cover_position', 'updated_by', 'trashed_with')"
        )
        .await
        .unwrap(),
        5
    );
    let [
        user,
        first,
        second,
        first_owner,
        second_owner,
        root,
        foreign,
    ]: [Id; 7] = std::array::from_fn(|_| Id::new_v7());
    db.execute(&format!(
        "BEGIN;
         INSERT INTO users (id, email, normalized_email, display_name, password_hash, created_at, updated_at)
         VALUES ('{user}', 'owner@example.com', 'owner@example.com', 'Owner', 'x', 1, 1);
         INSERT INTO workspaces (id, name, version, owner_membership_id, created_at, updated_at)
         VALUES ('{first}', 'First', 0, '{first_owner}', 1, 1),
                ('{second}', 'Second', 0, '{second_owner}', 1, 1);
         INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, updated_at)
         VALUES ('{first_owner}', '{first}', '{user}', 'owner', 0, 1, 1),
                ('{second_owner}', '{second}', '{user}', 'owner', 0, 1, 1);
         INSERT INTO pages (id, workspace_id, creator_id, updated_by, created_at, updated_at)
         VALUES ('{root}', '{first}', '{user}', '{user}', 1, 1),
                ('{foreign}', '{second}', '{user}', '{user}', 1, 1);
         COMMIT;"
    ))
    .await
    .unwrap();

    let cross_insert = db
        .execute(&format!(
            "INSERT INTO pages (id, workspace_id, parent_id, creator_id, updated_by, created_at, updated_at) \
             VALUES ('{}', '{second}', '{root}', '{user}', '{user}', 1, 1)",
            Id::new_v7()
        ))
        .await
        .unwrap_err()
        .to_string();
    assert!(
        cross_insert.contains("page parent must belong to the workspace"),
        "{cross_insert}"
    );
    let cross_move = db
        .execute(&format!(
            "UPDATE pages SET parent_id = '{root}' WHERE id = '{foreign}'"
        ))
        .await
        .unwrap_err()
        .to_string();
    assert!(
        cross_move.contains("page parent must belong to the workspace"),
        "{cross_move}"
    );
    let moved = db
        .execute(&format!(
            "UPDATE pages SET workspace_id = '{second}' WHERE id = '{root}'"
        ))
        .await
        .unwrap_err()
        .to_string();
    assert!(moved.contains("page workspace is immutable"), "{moved}");
}

#[tokio::test]
async fn page_spaces_migration_backfills_a_general_teamspace_per_workspace() {
    let directory = tempfile::tempdir().unwrap();
    let db = Database::open(&DatabaseConfig::new(directory.path().join("db.sqlite")))
        .await
        .unwrap();
    MigrationRunner::embedded_through("test", 22)
        .run(&db)
        .await
        .unwrap();
    let [
        user,
        first,
        second,
        first_owner,
        second_owner,
        root,
        child,
        trashed,
        foreign,
    ]: [Id; 9] = std::array::from_fn(|_| Id::new_v7());
    db.execute(&format!(
        "BEGIN;
         INSERT INTO users (id, email, normalized_email, display_name, password_hash, created_at, updated_at)
         VALUES ('{user}', 'owner@example.com', 'owner@example.com', 'Owner', 'x', 1, 1);
         INSERT INTO workspaces (id, name, version, owner_membership_id, created_at, updated_at, deleted_at)
         VALUES ('{first}', 'First', 0, '{first_owner}', 1, 1, NULL),
                ('{second}', 'Second', 0, '{second_owner}', 1, 1, 5);
         INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, updated_at)
         VALUES ('{first_owner}', '{first}', '{user}', 'owner', 0, 1, 1),
                ('{second_owner}', '{second}', '{user}', 'owner', 0, 1, 1);
         INSERT INTO pages (id, workspace_id, parent_id, creator_id, updated_by, created_at, updated_at, deleted_at, trashed_with)
         VALUES ('{root}', '{first}', NULL, '{user}', '{user}', 1, 1, NULL, NULL),
                ('{child}', '{first}', '{root}', '{user}', '{user}', 1, 1, NULL, NULL),
                ('{trashed}', '{first}', NULL, '{user}', '{user}', 1, 1, 9, '{trashed}'),
                ('{foreign}', '{second}', NULL, '{user}', '{user}', 1, 1, NULL, NULL);
         COMMIT;"
    ))
    .await
    .unwrap();

    MigrationRunner::embedded("test").run(&db).await.unwrap();

    assert_eq!(
        db.scalar::<i64>("SELECT COUNT(*) FROM pragma_foreign_key_check")
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        db.scalar::<String>("PRAGMA integrity_check").await.unwrap(),
        "ok"
    );
    let teamspaces: Vec<(String, String, String, i64, i64, Option<String>)> = sqlx::query_as(
        "SELECT id, workspace_id, name, position, version, created_by FROM teamspaces \
         ORDER BY workspace_id = ? DESC",
    )
    .bind(first.to_string())
    .fetch_all(db.pool())
    .await
    .unwrap();
    assert_eq!(teamspaces.len(), 2, "trashed workspaces get one too");
    for (teamspace, workspace) in teamspaces.iter().zip([first, second]) {
        assert!(
            teamspace.0.parse::<Id>().is_ok(),
            "backfilled id {} must be a canonical UUIDv7",
            teamspace.0
        );
        assert_eq!(teamspace.1, workspace.to_string());
        assert_eq!(
            (teamspace.2.as_str(), teamspace.3, teamspace.4),
            ("General", 0, 0)
        );
        assert_eq!(teamspace.5.as_deref(), Some(user.to_string().as_str()));
    }
    let general = teamspaces[0].0.clone();
    let foreign_general = teamspaces[1].0.clone();
    let pages: Vec<(String, Option<String>, Option<String>)> =
        sqlx::query_as("SELECT id, teamspace_id, owner_id FROM pages ORDER BY id")
            .fetch_all(db.pool())
            .await
            .unwrap();
    assert_eq!(pages.len(), 4);
    for (id, teamspace_id, owner_id) in &pages {
        let expected = if *id == foreign.to_string() {
            &foreign_general
        } else {
            &general
        };
        assert_eq!(teamspace_id.as_ref(), Some(expected), "{id}");
        assert_eq!(owner_id, &None);
    }

    let insert = |parent: Option<Id>, teamspace: Option<&str>, owner: Option<Id>| {
        let quote = |value: Option<String>| value.map_or("NULL".to_owned(), |v| format!("'{v}'"));
        format!(
            "INSERT INTO pages (id, workspace_id, parent_id, teamspace_id, owner_id, creator_id, updated_by, created_at, updated_at) \
             VALUES ('{}', '{first}', {}, {}, {}, '{user}', '{user}', 1, 1)",
            Id::new_v7(),
            quote(parent.map(|id| id.to_string())),
            quote(teamspace.map(str::to_owned)),
            quote(owner.map(|id| id.to_string())),
        )
    };
    for (sql, message) in [
        (
            insert(None, None, None),
            "page must belong to exactly one space",
        ),
        (
            insert(None, Some(&general), Some(user)),
            "page must belong to exactly one space",
        ),
        (
            insert(None, Some(&foreign_general), None),
            "page teamspace must belong to the workspace",
        ),
        (
            insert(Some(root), None, Some(user)),
            "page must share its parent's space",
        ),
        (
            format!(
                "UPDATE pages SET teamspace_id = NULL, owner_id = '{user}' WHERE id = '{child}'"
            ),
            "page must share its parent's space",
        ),
        (
            format!("UPDATE pages SET owner_id = '{user}' WHERE id = '{root}'"),
            "page must belong to exactly one space",
        ),
        (
            format!("UPDATE teamspaces SET workspace_id = '{second}' WHERE id = '{general}'"),
            "teamspace workspace is immutable",
        ),
    ] {
        let error = db.execute(&sql).await.unwrap_err().to_string();
        assert!(error.contains(message), "{sql}: {error}");
    }
    db.execute(&insert(None, None, Some(user))).await.unwrap();

    // Deleting a teamspace cascades to its pages, even a trashed parent with its child: the
    // parent_id SET NULL on the child must not trip the space triggers.
    db.execute(&format!(
        "UPDATE pages SET deleted_at = 10, trashed_with = '{root}' WHERE id IN ('{root}', '{child}')"
    ))
    .await
    .unwrap();
    db.execute(&format!("DELETE FROM teamspaces WHERE id = '{general}'"))
        .await
        .unwrap();
    let remaining: Vec<(String, Option<String>)> =
        sqlx::query_as("SELECT workspace_id, owner_id FROM pages ORDER BY workspace_id = ? DESC")
            .bind(first.to_string())
            .fetch_all(db.pool())
            .await
            .unwrap();
    assert_eq!(
        remaining,
        [
            (first.to_string(), Some(user.to_string())),
            (second.to_string(), None),
        ]
    );
}

#[tokio::test]
async fn page_favorites_migration_scopes_rows_and_cascades() {
    let directory = tempfile::tempdir().unwrap();
    let db = Database::open(&DatabaseConfig::new(directory.path().join("db.sqlite")))
        .await
        .unwrap();
    MigrationRunner::embedded_through("test", 23)
        .run(&db)
        .await
        .unwrap();
    let [
        user,
        first,
        second,
        first_owner,
        second_owner,
        page,
        other,
        foreign,
    ]: [Id; 8] = std::array::from_fn(|_| Id::new_v7());
    db.execute(&format!(
        "BEGIN;
         INSERT INTO users (id, email, normalized_email, display_name, password_hash, created_at, updated_at)
         VALUES ('{user}', 'owner@example.com', 'owner@example.com', 'Owner', 'x', 1, 1);
         INSERT INTO workspaces (id, name, version, owner_membership_id, created_at, updated_at, deleted_at)
         VALUES ('{first}', 'First', 0, '{first_owner}', 1, 1, NULL),
                ('{second}', 'Second', 0, '{second_owner}', 1, 1, NULL);
         INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, updated_at)
         VALUES ('{first_owner}', '{first}', '{user}', 'owner', 0, 1, 1),
                ('{second_owner}', '{second}', '{user}', 'owner', 0, 1, 1);
         INSERT INTO pages (id, workspace_id, parent_id, owner_id, creator_id, updated_by, created_at, updated_at)
         VALUES ('{page}', '{first}', NULL, '{user}', '{user}', '{user}', 1, 1),
                ('{other}', '{first}', NULL, '{user}', '{user}', '{user}', 1, 1),
                ('{foreign}', '{second}', NULL, '{user}', '{user}', '{user}', 1, 1);
         COMMIT;"
    ))
    .await
    .unwrap();

    MigrationRunner::embedded("test").run(&db).await.unwrap();

    let favorite = |workspace: Id, page: Id, position: i64| {
        format!(
            "INSERT INTO page_favorites (workspace_id, user_id, page_id, position, created_at) \
             VALUES ('{workspace}', '{user}', '{page}', {position}, 1)"
        )
    };
    db.execute(&favorite(first, page, 0)).await.unwrap();
    db.execute(&favorite(first, other, 1)).await.unwrap();
    db.execute(&favorite(second, foreign, 0)).await.unwrap();
    for (sql, message) in [
        (
            favorite(second, page, 1),
            "favorite page must belong to the workspace",
        ),
        (favorite(first, page, 2), "UNIQUE constraint failed"),
        (favorite(first, foreign, 2), "favorite page must belong"),
        (
            format!("UPDATE page_favorites SET page_id = '{foreign}' WHERE page_id = '{other}'"),
            "favorite identity is immutable",
        ),
        (
            format!("UPDATE page_favorites SET position = -1 WHERE page_id = '{page}'"),
            "CHECK constraint failed",
        ),
    ] {
        let error = db.execute(&sql).await.unwrap_err().to_string();
        assert!(error.contains(message), "{sql}: {error}");
    }

    // A hard-deleted page takes its favorites along; so does a deleted workspace.
    db.execute(&format!("DELETE FROM pages WHERE id = '{page}'"))
        .await
        .unwrap();
    db.execute(&format!("DELETE FROM workspaces WHERE id = '{second}'"))
        .await
        .unwrap();
    let remaining: Vec<(String,)> = sqlx::query_as("SELECT page_id FROM page_favorites")
        .fetch_all(db.pool())
        .await
        .unwrap();
    assert_eq!(remaining, [(other.to_string(),)]);
    assert_eq!(
        db.scalar::<i64>("SELECT COUNT(*) FROM pragma_foreign_key_check")
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        db.scalar::<String>("PRAGMA integrity_check").await.unwrap(),
        "ok"
    );
}

#[tokio::test]
async fn page_files_migration_scopes_rows_and_releases_blobs() {
    let directory = tempfile::tempdir().unwrap();
    let db = Database::open(&DatabaseConfig::new(directory.path().join("db.sqlite")))
        .await
        .unwrap();
    MigrationRunner::embedded_through("test", 24)
        .run(&db)
        .await
        .unwrap();
    let [
        user,
        first,
        second,
        first_owner,
        second_owner,
        teamspace,
        page,
        private,
        foreign,
        blob,
        foreign_blob,
    ]: [Id; 11] = std::array::from_fn(|_| Id::new_v7());
    db.execute(&format!(
        "BEGIN;
         INSERT INTO users (id, email, normalized_email, display_name, password_hash, created_at, updated_at)
         VALUES ('{user}', 'owner@example.com', 'owner@example.com', 'Owner', 'x', 1, 1);
         INSERT INTO workspaces (id, name, version, owner_membership_id, created_at, updated_at, deleted_at)
         VALUES ('{first}', 'First', 0, '{first_owner}', 1, 1, NULL),
                ('{second}', 'Second', 0, '{second_owner}', 1, 1, NULL);
         INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, updated_at)
         VALUES ('{first_owner}', '{first}', '{user}', 'owner', 0, 1, 1),
                ('{second_owner}', '{second}', '{user}', 'owner', 0, 1, 1);
         INSERT INTO teamspaces (id, workspace_id, name, created_at, updated_at)
         VALUES ('{teamspace}', '{first}', 'Team', 1, 1);
         INSERT INTO pages (id, workspace_id, parent_id, teamspace_id, owner_id, creator_id, updated_by, created_at, updated_at)
         VALUES ('{page}', '{first}', NULL, '{teamspace}', NULL, '{user}', '{user}', 1, 1),
                ('{private}', '{first}', NULL, NULL, '{user}', '{user}', '{user}', 1, 1),
                ('{foreign}', '{second}', NULL, NULL, '{user}', '{user}', '{user}', 1, 1);
         INSERT INTO attachment_blobs (id, workspace_id, sha256, byte_size, storage_key, created_at, quarantine_until)
         VALUES ('{blob}', '{first}', 'a', 1, 'k1', 1, 0),
                ('{foreign_blob}', '{second}', 'b', 1, 'k2', 1, 0);
         COMMIT;"
    ))
    .await
    .unwrap();

    MigrationRunner::embedded("test").run(&db).await.unwrap();

    let file = |workspace: Id, page: Id, blob: Id, name: &str| {
        format!(
            "INSERT INTO page_files (id, workspace_id, page_id, blob_id, file_name, mime_type, \
             size_bytes, uploaded_by, created_at) \
             VALUES ('{}', '{workspace}', '{page}', '{blob}', '{name}', 'image/png', 1, '{user}', 1)",
            Id::new_v7()
        )
    };
    db.execute(&file(first, page, blob, "a.png")).await.unwrap();
    db.execute(&file(first, private, blob, "b.png"))
        .await
        .unwrap();
    for (sql, message) in [
        (
            file(first, foreign, blob, "x.png"),
            "page file scope mismatch",
        ),
        (
            file(first, page, foreign_blob, "x.png"),
            "page file scope mismatch",
        ),
        (file(first, page, blob, ""), "CHECK constraint failed"),
        (
            format!("UPDATE page_files SET page_id = '{private}' WHERE page_id = '{page}'"),
            "page file scope is immutable",
        ),
        (
            format!("DELETE FROM attachment_blobs WHERE id = '{blob}'"),
            "FOREIGN KEY constraint failed",
        ),
    ] {
        let error = db.execute(&sql).await.unwrap_err().to_string();
        assert!(error.contains(message), "{sql}: {error}");
    }

    // A hard-deleted page takes its rows along and quarantines the blob for a day.
    db.execute(&format!("DELETE FROM pages WHERE id = '{page}'"))
        .await
        .unwrap();
    assert_eq!(
        db.scalar::<i64>("SELECT COUNT(*) FROM page_files")
            .await
            .unwrap(),
        1
    );
    assert!(
        db.scalar::<i64>(&format!(
            "SELECT quarantine_until FROM attachment_blobs WHERE id = '{blob}'"
        ))
        .await
        .unwrap()
            > orbit_platform::TimestampMillis::now().as_millis() + 23 * 60 * 60 * 1_000
    );
    // The workspace cascade removes the rest; the blob is then deletable.
    db.execute(&format!("DELETE FROM workspaces WHERE id = '{first}'"))
        .await
        .unwrap();
    db.execute(&format!("DELETE FROM attachment_blobs WHERE id = '{blob}'"))
        .await
        .unwrap();
    assert_eq!(
        db.scalar::<i64>("SELECT COUNT(*) FROM page_files")
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        db.scalar::<i64>("SELECT COUNT(*) FROM pragma_foreign_key_check")
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        db.scalar::<String>("PRAGMA integrity_check").await.unwrap(),
        "ok"
    );
}

#[tokio::test]
async fn page_search_migration_backfills_and_tracks_pages() {
    let directory = tempfile::tempdir().unwrap();
    let db = Database::open(&DatabaseConfig::new(directory.path().join("db.sqlite")))
        .await
        .unwrap();
    MigrationRunner::embedded_through("test", 26)
        .run(&db)
        .await
        .unwrap();
    let [user, workspace, owner, teamspace, live, trashed, private]: [Id; 7] =
        std::array::from_fn(|_| Id::new_v7());
    db.execute(&format!(
        "BEGIN;
         INSERT INTO users (id, email, normalized_email, display_name, password_hash, created_at, updated_at)
         VALUES ('{user}', 'owner@example.com', 'owner@example.com', 'Owner', 'x', 1, 1);
         INSERT INTO workspaces (id, name, version, owner_membership_id, created_at, updated_at)
         VALUES ('{workspace}', 'First', 0, '{owner}', 1, 1);
         INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, updated_at)
         VALUES ('{owner}', '{workspace}', '{user}', 'owner', 0, 1, 1);
         INSERT INTO teamspaces (id, workspace_id, name, created_at, updated_at)
         VALUES ('{teamspace}', '{workspace}', 'General', 1, 1);
         INSERT INTO pages (id, workspace_id, teamspace_id, owner_id, title, content_text, creator_id, updated_by, created_at, updated_at, deleted_at, trashed_with)
         VALUES ('{live}', '{workspace}', '{teamspace}', NULL, 'Über plan', 'Café notes', '{user}', '{user}', 1, 1, NULL, NULL),
                ('{trashed}', '{workspace}', '{teamspace}', NULL, 'Old', 'uber archive', '{user}', '{user}', 2, 2, 5, '{trashed}'),
                ('{private}', '{workspace}', NULL, '{user}', 'Diary', 'secret', '{user}', '{user}', 3, 3, NULL, NULL);
         COMMIT;"
    ))
    .await
    .unwrap();

    MigrationRunner::embedded("test").run(&db).await.unwrap();

    let matches = |query: &'static str| {
        let db = &db;
        async move {
            let mut ids: Vec<String> = sqlx::query_scalar(
                "SELECT page_search_rows.page_id FROM page_search \
                 JOIN page_search_rows ON page_search_rows.id = page_search.rowid \
                 WHERE page_search MATCH ?",
            )
            .bind(query)
            .fetch_all(db.pool())
            .await
            .unwrap();
            ids.sort();
            ids
        }
    };
    // Every page is indexed (trashed ones too; queries filter them), diacritics folded.
    assert_eq!(
        db.scalar::<i64>("SELECT COUNT(*) FROM page_search")
            .await
            .unwrap(),
        3
    );
    let mut uber = vec![live.to_string(), trashed.to_string()];
    uber.sort();
    assert_eq!(matches("\"uber\"").await, uber);
    assert_eq!(matches("\"cafe\"").await, [live.to_string()]);
    assert_eq!(matches("\"pla\"*").await, [live.to_string()]);

    // Triggers follow inserts, title/body updates and deletes.
    let added = Id::new_v7();
    db.execute(&format!(
        "INSERT INTO pages (id, workspace_id, teamspace_id, title, content_text, creator_id, updated_by, created_at, updated_at) \
         VALUES ('{added}', '{workspace}', '{teamspace}', 'Zebra', '', '{user}', '{user}', 4, 4)"
    ))
    .await
    .unwrap();
    assert_eq!(matches("\"zebra\"").await, [added.to_string()]);
    db.execute(&format!(
        "UPDATE pages SET title = 'Giraffe', content_text = 'tall' WHERE id = '{added}'"
    ))
    .await
    .unwrap();
    assert!(matches("\"zebra\"").await.is_empty());
    assert_eq!(matches("\"tall\"").await, [added.to_string()]);
    db.execute(&format!("DELETE FROM pages WHERE id = '{added}'"))
        .await
        .unwrap();
    assert!(matches("\"giraffe\"").await.is_empty());
    // The workspace purge deletes every page explicitly; the index empties with it.
    db.execute(&format!(
        "DELETE FROM pages WHERE workspace_id = '{workspace}'"
    ))
    .await
    .unwrap();
    assert_eq!(
        db.scalar::<i64>("SELECT COUNT(*) FROM page_search")
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        db.scalar::<i64>("SELECT COUNT(*) FROM page_search_rows")
            .await
            .unwrap(),
        0
    );
    db.execute("INSERT INTO page_search (page_search) VALUES ('integrity-check')")
        .await
        .unwrap();
    assert_eq!(
        db.scalar::<String>("PRAGMA integrity_check").await.unwrap(),
        "ok"
    );
}

#[tokio::test]
async fn page_versions_migration_scopes_rows_and_cascades() {
    let directory = tempfile::tempdir().unwrap();
    let db = Database::open(&DatabaseConfig::new(directory.path().join("db.sqlite")))
        .await
        .unwrap();
    MigrationRunner::embedded_through("test", 27)
        .run(&db)
        .await
        .unwrap();
    let [
        user,
        editor,
        workspace,
        other,
        owner,
        other_owner,
        teamspace,
        page,
        doomed,
    ]: [Id; 9] = std::array::from_fn(|_| Id::new_v7());
    db.execute(&format!(
        "BEGIN;
         INSERT INTO users (id, email, normalized_email, display_name, password_hash, created_at, updated_at)
         VALUES ('{user}', 'owner@example.com', 'owner@example.com', 'Owner', 'x', 1, 1),
                ('{editor}', 'editor@example.com', 'editor@example.com', 'Editor', 'x', 1, 1);
         INSERT INTO workspaces (id, name, version, owner_membership_id, created_at, updated_at)
         VALUES ('{workspace}', 'First', 0, '{owner}', 1, 1),
                ('{other}', 'Second', 0, '{other_owner}', 1, 1);
         INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, updated_at)
         VALUES ('{owner}', '{workspace}', '{user}', 'owner', 0, 1, 1),
                ('{other_owner}', '{other}', '{user}', 'owner', 0, 1, 1);
         INSERT INTO teamspaces (id, workspace_id, name, created_at, updated_at)
         VALUES ('{teamspace}', '{workspace}', 'General', 1, 1);
         INSERT INTO pages (id, workspace_id, teamspace_id, title, creator_id, updated_by, created_at, updated_at)
         VALUES ('{page}', '{workspace}', '{teamspace}', 'Plan', '{user}', '{user}', 1, 1),
                ('{doomed}', '{workspace}', '{teamspace}', 'Doomed', '{user}', '{user}', 1, 1);
         COMMIT;"
    ))
    .await
    .unwrap();

    MigrationRunner::embedded("test").run(&db).await.unwrap();

    let insert = |id: Id, workspace_id: Id, page_id: Id, kind: &'static str, by: Id| {
        let db = &db;
        async move {
            db.execute(&format!(
                "INSERT INTO page_versions (id, workspace_id, page_id, title, icon, content_json, kind, created_by, created_at) \
                 VALUES ('{id}', '{workspace_id}', '{page_id}', 'Plan', NULL, '[]', '{kind}', '{by}', 5)"
            ))
            .await
        }
    };
    let [kept, by_editor, gone]: [Id; 3] = std::array::from_fn(|_| Id::new_v7());
    insert(kept, workspace, page, "auto", user).await.unwrap();
    insert(by_editor, workspace, page, "restore", editor)
        .await
        .unwrap();
    insert(gone, workspace, doomed, "import", user)
        .await
        .unwrap();
    // A version belongs to its page's workspace, has a known kind and keeps its scope.
    assert!(
        insert(Id::new_v7(), other, page, "auto", user)
            .await
            .is_err()
    );
    assert!(
        insert(Id::new_v7(), workspace, page, "manual", user)
            .await
            .is_err()
    );
    assert!(
        db.execute(&format!(
            "UPDATE page_versions SET page_id = '{doomed}' WHERE id = '{kept}'"
        ))
        .await
        .is_err()
    );

    // Versions go with their page and lose a deleted author.
    db.execute(&format!("DELETE FROM pages WHERE id = '{doomed}'"))
        .await
        .unwrap();
    db.execute(&format!("DELETE FROM users WHERE id = '{editor}'"))
        .await
        .unwrap();
    assert_eq!(
        db.scalar::<i64>("SELECT COUNT(*) FROM page_versions")
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        db.scalar::<Option<String>>(&format!(
            "SELECT created_by FROM page_versions WHERE id = '{by_editor}'"
        ))
        .await
        .unwrap(),
        None
    );
    // The workspace purge deletes pages, then teamspaces, then the workspace.
    db.execute(&format!(
        "DELETE FROM pages WHERE workspace_id = '{workspace}';
         DELETE FROM teamspaces WHERE workspace_id = '{workspace}';
         DELETE FROM workspaces WHERE id = '{workspace}';"
    ))
    .await
    .unwrap();
    assert_eq!(
        db.scalar::<i64>("SELECT COUNT(*) FROM page_versions")
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        db.scalar::<i64>("SELECT COUNT(*) FROM pragma_foreign_key_check")
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        db.scalar::<String>("PRAGMA integrity_check").await.unwrap(),
        "ok"
    );
}

#[tokio::test]
async fn page_options_migration_adds_lock_width_and_scoped_visits() {
    let directory = tempfile::tempdir().unwrap();
    let db = Database::open(&DatabaseConfig::new(directory.path().join("db.sqlite")))
        .await
        .unwrap();
    MigrationRunner::embedded_through("test", 30)
        .run(&db)
        .await
        .unwrap();
    let [
        user,
        other_user,
        first,
        second,
        first_owner,
        second_owner,
        page,
        other,
        foreign,
    ]: [Id; 9] = std::array::from_fn(|_| Id::new_v7());
    db.execute(&format!(
        "BEGIN;
         INSERT INTO users (id, email, normalized_email, display_name, password_hash, created_at, updated_at)
         VALUES ('{user}', 'owner@example.com', 'owner@example.com', 'Owner', 'x', 1, 1),
                ('{other_user}', 'other@example.com', 'other@example.com', 'Other', 'x', 1, 1);
         INSERT INTO workspaces (id, name, version, owner_membership_id, created_at, updated_at, deleted_at)
         VALUES ('{first}', 'First', 0, '{first_owner}', 1, 1, NULL),
                ('{second}', 'Second', 0, '{second_owner}', 1, 1, NULL);
         INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, updated_at)
         VALUES ('{first_owner}', '{first}', '{user}', 'owner', 0, 1, 1),
                ('{second_owner}', '{second}', '{user}', 'owner', 0, 1, 1);
         INSERT INTO pages (id, workspace_id, parent_id, owner_id, creator_id, updated_by, created_at, updated_at)
         VALUES ('{page}', '{first}', NULL, '{user}', '{user}', '{user}', 1, 1),
                ('{other}', '{first}', NULL, '{user}', '{user}', '{user}', 1, 1),
                ('{foreign}', '{second}', NULL, '{user}', '{user}', '{user}', 1, 1);
         COMMIT;"
    ))
    .await
    .unwrap();

    MigrationRunner::embedded("test").run(&db).await.unwrap();

    // Existing pages are unlocked and not full width.
    assert_eq!(
        db.scalar::<i64>(
            "SELECT COUNT(*) FROM pages WHERE full_width = 0 AND locked_at IS NULL AND locked_by IS NULL"
        )
        .await
        .unwrap(),
        3
    );
    db.execute(&format!(
        "UPDATE pages SET full_width = 1, locked_at = 5, locked_by = '{other_user}' WHERE id = '{other}'"
    ))
    .await
    .unwrap();
    let visit = |workspace: Id, page: Id, at: i64| {
        format!(
            "INSERT INTO page_visits (workspace_id, user_id, page_id, visited_at) \
             VALUES ('{workspace}', '{user}', '{page}', {at})"
        )
    };
    db.execute(&visit(first, page, 1)).await.unwrap();
    db.execute(&visit(first, other, 2)).await.unwrap();
    db.execute(&visit(second, foreign, 3)).await.unwrap();
    for (sql, message) in [
        (
            visit(second, page, 4),
            "visited page must belong to the workspace",
        ),
        (visit(first, page, 4), "UNIQUE constraint failed"),
        (
            format!("UPDATE page_visits SET page_id = '{foreign}' WHERE page_id = '{other}'"),
            "page visit identity is immutable",
        ),
        (
            format!("UPDATE pages SET full_width = 2 WHERE id = '{page}'"),
            "CHECK constraint failed",
        ),
        (
            format!("UPDATE pages SET locked_by = '{user}' WHERE id = '{page}'"),
            "CHECK constraint failed",
        ),
        (
            format!("UPDATE pages SET locked_at = 1, locked_by = 'nobody' WHERE id = '{page}'"),
            "FOREIGN KEY constraint failed",
        ),
    ] {
        let error = db.execute(&sql).await.unwrap_err().to_string();
        assert!(error.contains(message), "{sql}: {error}");
    }

    // A deleted locker leaves the page locked by nobody in particular.
    db.execute(&format!("DELETE FROM users WHERE id = '{other_user}'"))
        .await
        .unwrap();
    assert_eq!(
        db.scalar::<i64>(&format!(
            "SELECT COUNT(*) FROM pages WHERE id = '{other}' AND locked_at = 5 AND locked_by IS NULL"
        ))
        .await
        .unwrap(),
        1
    );
    // A hard-deleted page takes its visits along; so does a deleted workspace.
    db.execute(&format!("DELETE FROM pages WHERE id = '{page}'"))
        .await
        .unwrap();
    db.execute(&format!("DELETE FROM workspaces WHERE id = '{second}'"))
        .await
        .unwrap();
    let remaining: Vec<(String,)> = sqlx::query_as("SELECT page_id FROM page_visits")
        .fetch_all(db.pool())
        .await
        .unwrap();
    assert_eq!(remaining, [(other.to_string(),)]);
    assert_eq!(
        db.scalar::<i64>("SELECT COUNT(*) FROM pragma_foreign_key_check")
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        db.scalar::<String>("PRAGMA integrity_check").await.unwrap(),
        "ok"
    );
}

#[tokio::test]
async fn page_mentions_migration_keeps_notifications_and_adds_the_kind() {
    let directory = tempfile::tempdir().unwrap();
    let db = Database::open(&DatabaseConfig::new(directory.path().join("db.sqlite")))
        .await
        .unwrap();
    MigrationRunner::embedded_through("test", 31)
        .run(&db)
        .await
        .unwrap();
    let [
        user,
        other_user,
        workspace,
        owner,
        member,
        page,
        private,
        thread,
        comment,
        note,
    ]: [Id; 10] = std::array::from_fn(|_| Id::new_v7());
    db.execute(&format!(
        "BEGIN;
         INSERT INTO users (id, email, normalized_email, display_name, password_hash, created_at, updated_at)
         VALUES ('{user}', 'owner@example.com', 'owner@example.com', 'Owner', 'x', 1, 1),
                ('{other_user}', 'other@example.com', 'other@example.com', 'Other', 'x', 1, 1);
         INSERT INTO workspaces (id, name, version, owner_membership_id, created_at, updated_at, deleted_at)
         VALUES ('{workspace}', 'First', 0, '{owner}', 1, 1, NULL);
         INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, updated_at)
         VALUES ('{owner}', '{workspace}', '{user}', 'owner', 0, 1, 1),
                ('{member}', '{workspace}', '{other_user}', 'member', 0, 1, 1);
         INSERT INTO pages (id, workspace_id, parent_id, owner_id, creator_id, updated_by, created_at, updated_at)
         VALUES ('{page}', '{workspace}', NULL, '{user}', '{user}', '{user}', 1, 1),
                ('{private}', '{workspace}', NULL, '{user}', '{user}', '{user}', 1, 1);
         INSERT INTO page_threads (id, workspace_id, page_id, created_by, created_at, updated_at)
         VALUES ('{thread}', '{workspace}', '{page}', '{user}', 1, 1);
         INSERT INTO page_comments (id, workspace_id, page_id, thread_id, author_id, created_at, updated_at)
         VALUES ('{comment}', '{workspace}', '{page}', '{thread}', '{user}', 1, 1);
         INSERT INTO notifications (id, workspace_id, recipient_user_id, actor_user_id, kind, page_id,
                                    page_thread_id, page_comment_id, dedupe_key, read_at, created_at)
         VALUES ('{note}', '{workspace}', '{user}', '{other_user}', 'page_comment_mentioned', '{page}',
                 '{thread}', '{comment}', 'kept', 7, 5);
         COMMIT;"
    ))
    .await
    .unwrap();

    MigrationRunner::embedded("test").run(&db).await.unwrap();

    assert_eq!(
        db.scalar::<String>(&format!(
            "SELECT kind || ':' || dedupe_key || ':' || read_at || ':' || created_at \
             || ':' || COALESCE(page_block_id, '-') FROM notifications WHERE id = '{note}'"
        ))
        .await
        .unwrap(),
        "page_comment_mentioned:kept:7:5:-"
    );
    let insert = |id: &str, recipient: Id, page: Id, extra: &str, block: &str| {
        format!(
            "INSERT INTO notifications (id, workspace_id, recipient_user_id, actor_user_id, kind, \
             page_id, page_thread_id, page_block_id, dedupe_key, created_at) VALUES ('{id}', \
             '{workspace}', '{recipient}', '{other_user}', 'page_mentioned', '{page}', {extra}, \
             {block}, '{id}', 9)"
        )
    };
    db.execute(&insert("ok", user, page, "NULL", "'b1'"))
        .await
        .unwrap();
    for (sql, message) in [
        // The page is its owner's private page: nobody else may be notified about it.
        (
            insert("private", other_user, private, "NULL", "NULL"),
            "notification must belong to the workspace task, a page the recipient can see",
        ),
        (
            insert("long", user, page, "NULL", &format!("'{}'", "x".repeat(65))),
            "CHECK constraint failed",
        ),
    ] {
        let error = db.execute(&sql).await.unwrap_err().to_string();
        assert!(error.contains(message), "{sql}: {error}");
    }
    assert_eq!(
        db.scalar::<String>("PRAGMA integrity_check").await.unwrap(),
        "ok"
    );
}

#[tokio::test]
async fn views_migration_backfills_completed_at_and_creates_view_tables() {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::open(&DatabaseConfig::new(directory.path().join("db.sqlite")))
        .await
        .unwrap();
    MigrationRunner::embedded_through("test", 32)
        .run(&database)
        .await
        .unwrap();

    let [user, workspace, membership, project]: [Id; 4] = std::array::from_fn(|_| Id::new_v7());
    let [todo, done, cancelled, duplicate]: [Id; 4] = std::array::from_fn(|_| Id::new_v7());
    let [open_task, done_task, cancelled_task, duplicate_task]: [Id; 4] =
        std::array::from_fn(|_| Id::new_v7());
    let seed = format!(
        "INSERT INTO users (id, email, normalized_email, display_name, password_hash, created_at, updated_at)
         VALUES ('{user}', 'owner@example.com', 'owner@example.com', 'Owner', 'x', 1, 1);
         INSERT INTO workspaces (id, name, version, owner_membership_id, created_at, updated_at)
         VALUES ('{workspace}', 'Orbit', 0, '{membership}', 1, 1);
         INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, updated_at)
         VALUES ('{membership}', '{workspace}', '{user}', 'owner', 0, 1, 1);
         INSERT INTO projects (id, workspace_id, name, project_key, color, version, deleted_at, created_at, updated_at)
         VALUES ('{project}', '{workspace}', 'Live', 'LIVE', '#000000', 0, NULL, 1, 1);
         INSERT INTO task_statuses (id, workspace_id, project_id, name, description, color, category, position, version, created_at, updated_at)
         VALUES ('{todo}', '{workspace}', '{project}', 'Todo', '', '#ffffff', 'unstarted', 0, 0, 1, 1),
                ('{done}', '{workspace}', '{project}', 'Done', '', '#ffffff', 'completed', 1, 0, 1, 1),
                ('{cancelled}', '{workspace}', '{project}', 'Cancelled', '', '#ffffff', 'cancelled', 2, 0, 1, 1),
                ('{duplicate}', '{workspace}', '{project}', 'Duplicate', '', '#ffffff', 'duplicate', 3, 0, 1, 1);
         INSERT INTO tasks (id, workspace_id, project_id, status_id, title, creator_id, created_at, updated_at)
         VALUES ('{open_task}', '{workspace}', '{project}', '{todo}', 'Open', '{user}', 1, 10),
                ('{done_task}', '{workspace}', '{project}', '{done}', 'Done', '{user}', 1, 20),
                ('{cancelled_task}', '{workspace}', '{project}', '{cancelled}', 'Cancelled', '{user}', 1, 30),
                ('{duplicate_task}', '{workspace}', '{project}', '{duplicate}', 'Duplicate', '{user}', 1, 40);"
    );
    let mut transaction = database.transaction().await.unwrap();
    sqlx::raw_sql(&seed)
        .execute(&mut *transaction)
        .await
        .unwrap();
    transaction.commit().await.unwrap();

    MigrationRunner::embedded("test")
        .run(&database)
        .await
        .unwrap();

    let completed: Vec<(String, Option<i64>)> =
        sqlx::query_as("SELECT title, completed_at FROM tasks ORDER BY updated_at")
            .fetch_all(database.pool())
            .await
            .unwrap();
    assert_eq!(
        completed,
        [
            ("Open".to_owned(), None),
            ("Done".to_owned(), Some(20)),
            ("Cancelled".to_owned(), Some(30)),
            ("Duplicate".to_owned(), Some(40)),
        ]
    );
    for table in ["saved_views", "favorites", "view_preferences"] {
        assert_eq!(
            database
                .scalar::<i64>(&format!(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = '{table}'"
                ))
                .await
                .unwrap(),
            1,
            "missing {table}"
        );
    }
    assert_eq!(
        database
            .scalar::<i64>("SELECT COUNT(*) FROM pragma_foreign_key_check")
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        database
            .scalar::<String>("PRAGMA integrity_check")
            .await
            .unwrap(),
        "ok"
    );
    // The name check trims before measuring.
    let error = database
        .execute(&format!(
            "INSERT INTO saved_views (id, workspace_id, owner_user_id, name, visibility, state_json, created_at, updated_at) \
             VALUES ('{}', '{workspace}', '{user}', '   ', 'personal', '{{}}', 1, 1)",
            Id::new_v7()
        ))
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("CHECK constraint failed"), "{error}");
}

#[tokio::test]
async fn sub_issues_migration_links_parents_in_one_workspace_and_adds_project_flags() {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::open(&DatabaseConfig::new(directory.path().join("db.sqlite")))
        .await
        .unwrap();
    MigrationRunner::embedded_through("test", 33)
        .run(&database)
        .await
        .unwrap();

    let [user, workspace, membership, project]: [Id; 4] = std::array::from_fn(|_| Id::new_v7());
    let [other_workspace, other_membership, other_project]: [Id; 3] =
        std::array::from_fn(|_| Id::new_v7());
    let [todo, other_todo, parent, child, foreign]: [Id; 5] = std::array::from_fn(|_| Id::new_v7());
    let seed = format!(
        "INSERT INTO users (id, email, normalized_email, display_name, password_hash, created_at, updated_at)
         VALUES ('{user}', 'owner@example.com', 'owner@example.com', 'Owner', 'x', 1, 1);
         INSERT INTO workspaces (id, name, version, owner_membership_id, created_at, updated_at)
         VALUES ('{workspace}', 'Orbit', 0, '{membership}', 1, 1),
                ('{other_workspace}', 'Other', 0, '{other_membership}', 1, 1);
         INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, updated_at)
         VALUES ('{membership}', '{workspace}', '{user}', 'owner', 0, 1, 1),
                ('{other_membership}', '{other_workspace}', '{user}', 'owner', 0, 1, 1);
         INSERT INTO projects (id, workspace_id, name, project_key, color, version, deleted_at, created_at, updated_at)
         VALUES ('{project}', '{workspace}', 'Live', 'LIVE', '#000000', 0, NULL, 1, 1),
                ('{other_project}', '{other_workspace}', 'Other', 'OTH', '#000000', 0, NULL, 1, 1);
         INSERT INTO task_statuses (id, workspace_id, project_id, name, description, color, category, position, version, created_at, updated_at)
         VALUES ('{todo}', '{workspace}', '{project}', 'Todo', '', '#ffffff', 'unstarted', 0, 0, 1, 1),
                ('{other_todo}', '{other_workspace}', '{other_project}', 'Todo', '', '#ffffff', 'unstarted', 0, 0, 1, 1);
         INSERT INTO tasks (id, workspace_id, project_id, status_id, title, creator_id, created_at, updated_at)
         VALUES ('{parent}', '{workspace}', '{project}', '{todo}', 'Parent', '{user}', 1, 1),
                ('{child}', '{workspace}', '{project}', '{todo}', 'Child', '{user}', 1, 1),
                ('{foreign}', '{other_workspace}', '{other_project}', '{other_todo}', 'Foreign', '{user}', 1, 1);"
    );
    let mut transaction = database.transaction().await.unwrap();
    sqlx::raw_sql(&seed)
        .execute(&mut *transaction)
        .await
        .unwrap();
    transaction.commit().await.unwrap();

    MigrationRunner::embedded("test")
        .run(&database)
        .await
        .unwrap();

    // Existing projects start with both automations on; existing tasks are top-level.
    assert_eq!(
        database
            .scalar::<i64>(&format!(
                "SELECT auto_close_parent + auto_close_sub_issues FROM projects WHERE id = '{project}'"
            ))
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        database
            .scalar::<i64>("SELECT COUNT(*) FROM tasks WHERE parent_task_id IS NOT NULL")
            .await
            .unwrap(),
        0
    );

    database
        .execute(&format!(
            "UPDATE tasks SET parent_task_id = '{parent}' WHERE id = '{child}'"
        ))
        .await
        .unwrap();
    for statement in [
        format!("UPDATE tasks SET parent_task_id = id WHERE id = '{parent}'"),
        format!("UPDATE tasks SET parent_task_id = '{foreign}' WHERE id = '{parent}'"),
        format!(
            "INSERT INTO tasks (id, workspace_id, project_id, status_id, title, creator_id, parent_task_id, created_at, updated_at) \
             VALUES ('{}', '{workspace}', '{project}', '{todo}', 'Stray', '{user}', '{foreign}', 1, 1)",
            Id::new_v7()
        ),
    ] {
        let error = database.execute(&statement).await.unwrap_err().to_string();
        assert!(
            error.contains("task parent must be another task in the same workspace"),
            "{statement}: {error}"
        );
    }
    let error = database
        .execute(&format!(
            "UPDATE projects SET auto_close_parent = 2 WHERE id = '{project}'"
        ))
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("CHECK constraint failed"), "{error}");

    // Hard-deleting a parent (retention purge) detaches its children; the SET NULL update
    // must not trip the parent triggers.
    database
        .execute(&format!("DELETE FROM tasks WHERE id = '{parent}'"))
        .await
        .unwrap();
    assert_eq!(
        database
            .scalar::<i64>(&format!(
                "SELECT COUNT(*) FROM tasks WHERE id = '{child}' AND parent_task_id IS NULL"
            ))
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        database
            .scalar::<i64>("SELECT COUNT(*) FROM pragma_foreign_key_check")
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        database
            .scalar::<String>("PRAGMA integrity_check")
            .await
            .unwrap(),
        "ok"
    );
}

#[tokio::test]
async fn chat_migration_gives_every_workspace_member_the_default_channel() {
    let directory = tempfile::tempdir().unwrap();
    let db = Database::open(&DatabaseConfig::new(directory.path().join("db.sqlite")))
        .await
        .unwrap();
    MigrationRunner::embedded_through("test", 36)
        .run(&db)
        .await
        .unwrap();
    let [
        owner,
        member,
        late,
        first,
        first_owner,
        first_member,
        second,
        second_owner,
        second_late,
    ]: [Id; 9] = std::array::from_fn(|_| Id::new_v7());
    db.execute(&format!(
        "BEGIN;
         INSERT INTO users (id, email, normalized_email, display_name, password_hash, created_at, updated_at)
         VALUES ('{owner}', 'owner@example.com', 'owner@example.com', 'Owner', 'x', 1, 1),
                ('{member}', 'member@example.com', 'member@example.com', 'Member', 'x', 1, 1),
                ('{late}', 'late@example.com', 'late@example.com', 'Late', 'x', 1, 1);
         INSERT INTO workspaces (id, name, version, owner_membership_id, created_at, updated_at, deleted_at)
         VALUES ('{first}', 'First', 0, '{first_owner}', 1, 1, NULL);
         INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, updated_at)
         VALUES ('{first_owner}', '{first}', '{owner}', 'owner', 0, 1, 1),
                ('{first_member}', '{first}', '{member}', 'member', 0, 1, 1);
         COMMIT;"
    ))
    .await
    .unwrap();

    MigrationRunner::embedded("test").run(&db).await.unwrap();

    // The workspace that existed before the migration.
    let members = |workspace: Id| {
        format!(
            "SELECT COUNT(*) FROM chat_members JOIN chat_conversations \
             ON chat_conversations.id = chat_members.conversation_id \
             WHERE chat_conversations.workspace_id = '{workspace}' AND chat_conversations.is_default = 1 \
             AND chat_conversations.name = 'general' AND chat_conversations.kind = 'public'"
        )
    };
    assert_eq!(db.scalar::<i64>(&members(first)).await.unwrap(), 2);

    // A workspace made afterwards: its first member makes the channel, later members join it.
    db.execute(&format!(
        "BEGIN;
         INSERT INTO workspaces (id, name, version, owner_membership_id, created_at, updated_at, deleted_at)
         VALUES ('{second}', 'Second', 0, '{second_owner}', 1700000000000, 1700000000000, NULL);
         INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, updated_at)
         VALUES ('{second_owner}', '{second}', '{owner}', 'owner', 0, 1700000000000, 1700000000000);
         INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, updated_at)
         VALUES ('{second_late}', '{second}', '{late}', 'member', 0, 1700000000001, 1700000000001);
         COMMIT;"
    ))
    .await
    .unwrap();
    assert_eq!(db.scalar::<i64>(&members(second)).await.unwrap(), 2);
    let ids: Vec<(String,)> = sqlx::query_as("SELECT id FROM chat_conversations")
        .fetch_all(db.pool())
        .await
        .unwrap();
    assert_eq!(ids.len(), 2);
    for (id,) in ids {
        id.parse::<Id>()
            .expect("the channel id made in SQL is a UUIDv7");
    }

    // Leaving the workspace leaves its conversations; a deleted workspace takes chat along.
    db.execute(&format!(
        "DELETE FROM memberships WHERE id = '{second_late}'"
    ))
    .await
    .unwrap();
    assert_eq!(db.scalar::<i64>(&members(second)).await.unwrap(), 1);
    db.execute(&format!("DELETE FROM workspaces WHERE id = '{first}'"))
        .await
        .unwrap();
    assert_eq!(
        db.scalar::<i64>("SELECT COUNT(*) FROM chat_conversations")
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        db.scalar::<i64>("SELECT COUNT(*) FROM pragma_foreign_key_check")
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        db.scalar::<String>("PRAGMA integrity_check").await.unwrap(),
        "ok"
    );
}

#[tokio::test]
async fn task_numbers_migration_backfills_allocates_and_never_reuses() {
    let directory = tempfile::tempdir().unwrap();
    let db = Database::open(&DatabaseConfig::new(directory.path().join("db.sqlite")))
        .await
        .unwrap();
    MigrationRunner::embedded_through("test", 37)
        .run(&db)
        .await
        .unwrap();
    let [user, workspace, membership, eng, ops, eng_todo, ops_todo]: [Id; 7] =
        std::array::from_fn(|_| Id::new_v7());
    let [first, second, third, other]: [Id; 4] = std::array::from_fn(|_| Id::new_v7());
    db.execute(&format!(
        "BEGIN;
         INSERT INTO users (id, email, normalized_email, display_name, password_hash, created_at, updated_at)
         VALUES ('{user}', 'owner@example.com', 'owner@example.com', 'Owner', 'x', 1, 1);
         INSERT INTO workspaces (id, name, version, owner_membership_id, created_at, updated_at)
         VALUES ('{workspace}', 'Orbit', 0, '{membership}', 1, 1);
         INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, updated_at)
         VALUES ('{membership}', '{workspace}', '{user}', 'owner', 0, 1, 1);
         INSERT INTO projects (id, workspace_id, name, project_key, color, version, deleted_at, created_at, updated_at)
         VALUES ('{eng}', '{workspace}', 'Engineering', 'ENG', '#000000', 0, NULL, 1, 1),
                ('{ops}', '{workspace}', 'Operations', 'OPS', '#000000', 0, NULL, 1, 1);
         INSERT INTO task_statuses (id, workspace_id, project_id, name, description, color, category, position, version, created_at, updated_at)
         VALUES ('{eng_todo}', '{workspace}', '{eng}', 'Todo', '', '#ffffff', 'unstarted', 0, 0, 1, 1),
                ('{ops_todo}', '{workspace}', '{ops}', 'Todo', '', '#ffffff', 'unstarted', 0, 0, 1, 1);
         INSERT INTO tasks (id, workspace_id, project_id, status_id, title, creator_id, created_at, updated_at)
         VALUES ('{second}', '{workspace}', '{eng}', '{eng_todo}', 'Second', '{user}', 20, 20),
                ('{first}', '{workspace}', '{eng}', '{eng_todo}', 'First', '{user}', 10, 10),
                ('{third}', '{workspace}', '{eng}', '{eng_todo}', 'Third', '{user}', 30, 30),
                ('{other}', '{workspace}', '{ops}', '{ops_todo}', 'Other', '{user}', 5, 5);
         COMMIT;"
    ))
    .await
    .unwrap();

    MigrationRunner::embedded("test").run(&db).await.unwrap();

    let number = |task: Id| format!("SELECT number FROM tasks WHERE id = '{task}'");
    let counter = |project: Id| format!("SELECT task_counter FROM projects WHERE id = '{project}'");
    // Backfill: creation order within each project.
    assert_eq!(db.scalar::<i64>(&number(first)).await.unwrap(), 1);
    assert_eq!(db.scalar::<i64>(&number(second)).await.unwrap(), 2);
    assert_eq!(db.scalar::<i64>(&number(third)).await.unwrap(), 3);
    assert_eq!(db.scalar::<i64>(&number(other)).await.unwrap(), 1);
    assert_eq!(db.scalar::<i64>(&counter(eng)).await.unwrap(), 3);
    assert_eq!(db.scalar::<i64>(&counter(ops)).await.unwrap(), 1);

    // A delete never frees its number.
    db.execute(&format!("DELETE FROM tasks WHERE id = '{third}'"))
        .await
        .unwrap();
    let fourth = Id::new_v7();
    db.execute(&format!(
        "INSERT INTO tasks (id, workspace_id, project_id, status_id, title, creator_id, created_at, updated_at) \
         VALUES ('{fourth}', '{workspace}', '{eng}', '{eng_todo}', 'Fourth', '{user}', 40, 40)"
    ))
    .await
    .unwrap();
    assert_eq!(db.scalar::<i64>(&number(fourth)).await.unwrap(), 4);

    // A move takes the target project's next number, even when the old one is taken there.
    db.execute(&format!(
        "UPDATE tasks SET project_id = '{ops}', status_id = '{ops_todo}', number = NULL WHERE id = '{first}'"
    ))
    .await
    .unwrap();
    assert_eq!(db.scalar::<i64>(&number(first)).await.unwrap(), 2);
    assert_eq!(db.scalar::<i64>(&counter(ops)).await.unwrap(), 2);
    assert_eq!(db.scalar::<i64>(&counter(eng)).await.unwrap(), 4);
    // The old identifier stays behind as an alias, though the same UPDATE cleared `number`.
    let aliases = |task: Id| {
        format!(
            "SELECT COALESCE(GROUP_CONCAT(project_key || '-' || number, ','), '') FROM ( \
             SELECT projects.project_key, task_number_aliases.number FROM task_number_aliases \
             JOIN projects ON projects.id = task_number_aliases.project_id \
             WHERE task_number_aliases.task_id = '{task}' \
             ORDER BY projects.project_key, task_number_aliases.number)"
        )
    };
    assert_eq!(db.scalar::<String>(&aliases(first)).await.unwrap(), "ENG-1");
    // Moving back takes a new number; both earlier identifiers remain aliases.
    db.execute(&format!(
        "UPDATE tasks SET project_id = '{eng}', status_id = '{eng_todo}', number = NULL WHERE id = '{first}'"
    ))
    .await
    .unwrap();
    assert_eq!(db.scalar::<i64>(&number(first)).await.unwrap(), 5);
    assert_eq!(
        db.scalar::<String>(&aliases(first)).await.unwrap(),
        "ENG-1,OPS-2"
    );
    // Rewriting the same project keeps the number and leaves no alias.
    db.execute(&format!(
        "UPDATE tasks SET project_id = '{eng}' WHERE id = '{second}'"
    ))
    .await
    .unwrap();
    assert_eq!(db.scalar::<i64>(&number(second)).await.unwrap(), 2);
    assert_eq!(db.scalar::<String>(&aliases(second)).await.unwrap(), "");

    // An explicit number (an import) moves the counter forward only.
    let imported = Id::new_v7();
    db.execute(&format!(
        "INSERT INTO tasks (id, workspace_id, project_id, status_id, title, creator_id, number, created_at, updated_at) \
         VALUES ('{imported}', '{workspace}', '{eng}', '{eng_todo}', 'Imported', '{user}', 10, 50, 50)"
    ))
    .await
    .unwrap();
    assert_eq!(db.scalar::<i64>(&counter(eng)).await.unwrap(), 10);
    // Aliases go with their task.
    db.execute(&format!("DELETE FROM tasks WHERE id = '{first}'"))
        .await
        .unwrap();
    assert_eq!(
        db.scalar::<i64>("SELECT COUNT(*) FROM task_number_aliases")
            .await
            .unwrap(),
        0
    );
    let error = db
        .execute(&format!(
            "UPDATE tasks SET number = 2 WHERE id = '{fourth}'"
        ))
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("UNIQUE constraint failed"), "{error}");

    assert_eq!(
        db.scalar::<i64>("SELECT COUNT(*) FROM pragma_foreign_key_check")
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        db.scalar::<String>("PRAGMA integrity_check").await.unwrap(),
        "ok"
    );
}

/// id, project, name, description, color, category, position, version, created_at, updated_at.
type StatusRow = (
    String,
    String,
    String,
    String,
    String,
    String,
    i64,
    i64,
    i64,
    i64,
);

#[tokio::test]
async fn intake_migration_rebuilds_statuses_and_keeps_every_referring_row() {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::open(&DatabaseConfig::new(directory.path().join("db.sqlite")))
        .await
        .unwrap();
    MigrationRunner::embedded_through("test", 50)
        .run(&database)
        .await
        .unwrap();

    let [user, workspace, membership, live_project, trashed_project]: [Id; 5] =
        std::array::from_fn(|_| Id::new_v7());
    let [todo, done, duplicate, trashed_todo]: [Id; 4] = std::array::from_fn(|_| Id::new_v7());
    let [first_task, second_task, duplicate_task, relation]: [Id; 4] =
        std::array::from_fn(|_| Id::new_v7());
    let seed = format!(
        "INSERT INTO users (id, email, normalized_email, display_name, password_hash, created_at, updated_at)
         VALUES ('{user}', 'owner@example.com', 'owner@example.com', 'Owner', 'x', 1, 1);
         INSERT INTO workspaces (id, name, version, owner_membership_id, created_at, updated_at)
         VALUES ('{workspace}', 'Orbit', 0, '{membership}', 1, 1);
         INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, updated_at)
         VALUES ('{membership}', '{workspace}', '{user}', 'owner', 0, 1, 1);
         INSERT INTO projects (id, workspace_id, name, project_key, color, version, deleted_at, created_at, updated_at)
         VALUES ('{live_project}', '{workspace}', 'Live', 'LIVE', '#000000', 0, NULL, 1, 1),
                ('{trashed_project}', '{workspace}', 'Trashed', 'TRASH', '#000000', 0, 5, 1, 1);
         INSERT INTO task_statuses (id, workspace_id, project_id, name, description, color, category, position, version, created_at, updated_at)
         VALUES ('{todo}', '{workspace}', '{live_project}', 'Todo', 'Next up', '#ffffff', 'unstarted', 0, 3, 1, 2),
                ('{done}', '{workspace}', '{live_project}', 'Done', '', '#00ff00', 'completed', 7, 0, 1, 1),
                ('{duplicate}', '{workspace}', '{live_project}', 'Duplicate', '', '#8b8f98', 'duplicate', 8, 1, 1, 1),
                ('{trashed_todo}', '{workspace}', '{trashed_project}', 'Todo', '', '#ffffff', 'unstarted', 0, 0, 1, 1);
         INSERT INTO tasks (id, workspace_id, project_id, status_id, title, creator_id, created_at, updated_at)
         VALUES ('{first_task}', '{workspace}', '{live_project}', '{todo}', 'First', '{user}', 1, 1),
                ('{second_task}', '{workspace}', '{live_project}', '{done}', 'Second', '{user}', 1, 1),
                ('{duplicate_task}', '{workspace}', '{live_project}', '{duplicate}', 'Copy', '{user}', 1, 1);
         INSERT INTO task_relations (id, workspace_id, task_id, related_task_id, type, previous_status_id, created_by, created_at)
         VALUES ('{relation}', '{workspace}', '{duplicate_task}', '{first_task}', 'duplicate', '{todo}', '{user}', 1);
         INSERT INTO project_pr_automation (project_id, event, status_id)
         VALUES ('{live_project}', 'open', '{todo}'), ('{live_project}', 'merged', '{done}'),
                ('{live_project}', 'draft', NULL);"
    );
    let mut transaction = database.transaction().await.unwrap();
    sqlx::raw_sql(&seed)
        .execute(&mut *transaction)
        .await
        .unwrap();
    transaction.commit().await.unwrap();
    let before: Vec<StatusRow> = sqlx::query_as(
        "SELECT id, project_id, name, description, color, category, position, version, created_at, updated_at \
         FROM task_statuses ORDER BY id",
    )
    .fetch_all(database.pool())
    .await
    .unwrap();
    assert_eq!(before.len(), 4);

    MigrationRunner::embedded("test")
        .run(&database)
        .await
        .unwrap();

    assert_eq!(
        database
            .scalar::<i64>("SELECT COUNT(*) FROM pragma_foreign_key_check")
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        database
            .scalar::<String>("PRAGMA integrity_check")
            .await
            .unwrap(),
        "ok"
    );

    // Every status row is the same row as before.
    let after: Vec<StatusRow> = sqlx::query_as(
        "SELECT id, project_id, name, description, color, category, position, version, created_at, updated_at \
         FROM task_statuses WHERE category <> 'triage' ORDER BY id",
    )
    .fetch_all(database.pool())
    .await
    .unwrap();
    assert_eq!(after, before);

    // Every task keeps its status.
    let tasks: Vec<(String, String)> =
        sqlx::query_as("SELECT id, status_id FROM tasks ORDER BY title")
            .fetch_all(database.pool())
            .await
            .unwrap();
    assert_eq!(
        tasks,
        [
            (duplicate_task.to_string(), duplicate.to_string()),
            (first_task.to_string(), todo.to_string()),
            (second_task.to_string(), done.to_string()),
        ]
    );

    // The DROP of the rebuild runs ON DELETE SET NULL on task_relations.previous_status_id; the
    // migration writes the value back.
    let previous: Option<String> =
        sqlx::query_scalar("SELECT previous_status_id FROM task_relations WHERE id = ?")
            .bind(relation.to_string())
            .fetch_one(database.pool())
            .await
            .unwrap();
    assert_eq!(previous, Some(todo.to_string()));

    // The DROP also runs ON DELETE CASCADE on project_pr_automation.status_id; the migration puts
    // the rows back, and a rule with no status is not touched.
    let automation: Vec<(String, Option<String>)> = sqlx::query_as(
        "SELECT event, status_id FROM project_pr_automation WHERE project_id = ? ORDER BY event",
    )
    .bind(live_project.to_string())
    .fetch_all(database.pool())
    .await
    .unwrap();
    assert_eq!(
        automation,
        [
            ("draft".to_owned(), None),
            ("merged".to_owned(), Some(done.to_string())),
            ("open".to_owned(), Some(todo.to_string())),
        ]
    );

    // Each project, also a trashed one, has one Triage status after its last position.
    let triage: Vec<(String, String, String, i64)> = sqlx::query_as(
        "SELECT id, project_id, name, position FROM task_statuses WHERE category = 'triage' ORDER BY position",
    )
    .fetch_all(database.pool())
    .await
    .unwrap();
    assert_eq!(triage.len(), 2);
    assert_eq!(
        (triage[0].1.as_str(), triage[0].2.as_str(), triage[0].3),
        (trashed_project.to_string().as_str(), "Triage", 1)
    );
    assert_eq!(
        (triage[1].1.as_str(), triage[1].3),
        (live_project.to_string().as_str(), 9)
    );
    assert!(triage.iter().all(|row| row.0.parse::<Id>().is_ok()));
    assert_eq!(
        database
            .scalar::<i64>("SELECT COUNT(*) FROM projects WHERE triage_enabled <> 0")
            .await
            .unwrap(),
        0
    );

    // The rebuilt table keeps its triggers, FK enforcement and the new invariants.
    for (sql, message) in [
        (
            format!(
                "UPDATE task_statuses SET project_id = '{trashed_project}' WHERE id = '{todo}'"
            ),
            "task status scope is immutable",
        ),
        (
            format!("UPDATE task_statuses SET category = 'duplicate' WHERE id = '{todo}'"),
            "the duplicate status category is immutable",
        ),
        (
            format!("UPDATE task_statuses SET category = 'triage' WHERE id = '{todo}'"),
            "the triage status category is immutable",
        ),
        (
            format!(
                "UPDATE task_statuses SET category = 'backlog' WHERE id = '{}'",
                triage[1].0
            ),
            "the triage status category is immutable",
        ),
        (
            format!(
                "INSERT INTO task_statuses (id, workspace_id, project_id, name, description, color, category, position, version, created_at, updated_at) \
                 VALUES ('{}', '{workspace}', '{live_project}', 'Again', '', '#ffffff', 'triage', 20, 0, 1, 1)",
                Id::new_v7()
            ),
            "UNIQUE constraint failed",
        ),
        (
            format!(
                "INSERT INTO task_statuses (id, workspace_id, project_id, name, description, color, category, position, version, created_at, updated_at) \
                 VALUES ('{}', '{workspace}', '{live_project}', 'Bad', '', '#ffffff', 'someday', 21, 0, 1, 1)",
                Id::new_v7()
            ),
            "CHECK constraint failed",
        ),
        (
            format!("DELETE FROM task_statuses WHERE id = '{todo}'"),
            "FOREIGN KEY constraint failed",
        ),
    ] {
        let error = database.execute(&sql).await.unwrap_err().to_string();
        assert!(error.contains(message), "{sql}: {error}");
    }
    // The new category is accepted.
    database
        .execute(&format!(
            "UPDATE task_statuses SET category = 'backlog' WHERE id = '{todo}'"
        ))
        .await
        .unwrap();
}

#[tokio::test]
async fn task_subscribers_migration_merges_task_rows_moves_prefs_and_backfills() {
    let directory = tempfile::tempdir().unwrap();
    let db = Database::open(&DatabaseConfig::new(directory.path().join("db.sqlite")))
        .await
        .unwrap();
    MigrationRunner::embedded_through("test", 44)
        .run(&db)
        .await
        .unwrap();
    let [
        owner,
        member,
        author,
        workspace,
        owner_m,
        member_m,
        author_m,
    ]: [Id; 7] = std::array::from_fn(|_| Id::new_v7());
    let [project, status, task, other, trashed, comment, page]: [Id; 7] =
        std::array::from_fn(|_| Id::new_v7());
    db.execute(&format!(
        "BEGIN;
         INSERT INTO users (id, email, normalized_email, display_name, password_hash, created_at, updated_at)
         VALUES ('{owner}', 'owner@example.com', 'owner@example.com', 'Owner', 'x', 1, 1),
                ('{member}', 'member@example.com', 'member@example.com', 'Member', 'x', 1, 1),
                ('{author}', 'author@example.com', 'author@example.com', 'Author', 'x', 1, 1);
         INSERT INTO workspaces (id, name, version, owner_membership_id, created_at, updated_at)
         VALUES ('{workspace}', 'Orbit', 0, '{owner_m}', 1, 1);
         INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, updated_at)
         VALUES ('{owner_m}', '{workspace}', '{owner}', 'owner', 0, 1, 1),
                ('{member_m}', '{workspace}', '{member}', 'member', 0, 1, 1),
                ('{author_m}', '{workspace}', '{author}', 'member', 0, 1, 1);
         INSERT INTO projects (id, workspace_id, name, project_key, color, version, deleted_at, created_at, updated_at)
         VALUES ('{project}', '{workspace}', 'Engineering', 'ENG', '#000000', 0, NULL, 1, 1);
         INSERT INTO task_statuses (id, workspace_id, project_id, name, description, color, category, position, version, created_at, updated_at)
         VALUES ('{status}', '{workspace}', '{project}', 'Todo', '', '#ffffff', 'unstarted', 0, 0, 1, 1);
         INSERT INTO tasks (id, workspace_id, project_id, status_id, title, creator_id, deleted_at, created_at, updated_at)
         VALUES ('{task}', '{workspace}', '{project}', '{status}', 'Task', '{owner}', NULL, 10, 10),
                ('{other}', '{workspace}', '{project}', '{status}', 'Other', '{owner}', NULL, 11, 11),
                ('{trashed}', '{workspace}', '{project}', '{status}', 'Trashed', '{owner}', 12, 12, 12);
         INSERT INTO task_assignees (task_id, membership_id, user_id)
         VALUES ('{task}', '{member_m}', '{member}'), ('{trashed}', '{member_m}', '{member}');
         INSERT INTO task_comments (id, workspace_id, task_id, author_id, parent_id, body, version, created_at, updated_at)
         VALUES ('{comment}', '{workspace}', '{task}', '{author}', NULL, 'hi', 0, 20, 20);
         INSERT INTO pages (id, workspace_id, parent_id, owner_id, creator_id, updated_by, created_at, updated_at)
         VALUES ('{page}', '{workspace}', NULL, '{member}', '{owner}', '{owner}', 1, 1);
         INSERT INTO notifications (id, workspace_id, recipient_user_id, actor_user_id, kind, task_id,
                                    comment_id, page_id, dedupe_key, read_at, created_at, pushed_at)
         VALUES ('n1', '{workspace}', '{member}', '{owner}', 'task_assigned', '{task}', NULL, NULL, 'k1', NULL, 30, 30),
                ('n2', '{workspace}', '{member}', '{author}', 'comment_mentioned', '{task}', '{comment}', NULL, 'k2', 45, 40, 41),
                ('n3', '{workspace}', '{member}', '{owner}', 'task_assigned', '{other}', NULL, NULL, 'k3', 50, 35, 35),
                ('n4', '{workspace}', '{owner}', '{author}', 'comment_mentioned', '{task}', '{comment}', NULL, 'k4', 60, 40, 40),
                ('n5', '{workspace}', '{member}', '{owner}', 'page_mentioned', NULL, NULL, '{page}', 'k5', NULL, 33, 33);
         INSERT INTO notification_prefs (user_id, direct_messages, chat_mentions, thread_replies,
                                         channel_messages, task_assigned, mentions, updated_at)
         VALUES ('{member}', 1, 0, 1, 1, 1, 0, 70), ('{owner}', 1, 1, 1, 1, 1, 1, 71);
         COMMIT;"
    ))
    .await
    .unwrap();

    MigrationRunner::embedded("test").run(&db).await.unwrap();

    // One row for each (recipient, task): the latest one, unread because `n1` was.
    let rows = "SELECT COALESCE(GROUP_CONCAT(line, ' '), '') FROM (SELECT id || '|' || kind || '|' \
                || COALESCE(comment_id, '-') || '|' || COALESCE(read_at, '-') || '|' || created_at \
                || '|' || COALESCE(pushed_at, '-') AS line FROM notifications ORDER BY id)";
    assert_eq!(
        db.scalar::<String>(rows).await.unwrap(),
        format!(
            "n2|comment_mentioned|{comment}|-|40|41 n3|task_assigned|-|50|35|35 \
             n4|comment_mentioned|{comment}|60|40|40 n5|page_mentioned|-|-|33|33"
        )
    );
    assert_eq!(
        db.scalar::<String>("SELECT dedupe_key FROM notifications WHERE id = 'n2'")
            .await
            .unwrap(),
        format!("{workspace}:{member}:task:{task}")
    );
    assert_eq!(
        db.scalar::<String>("SELECT dedupe_key FROM notifications WHERE id = 'n5'")
            .await
            .unwrap(),
        "k5"
    );

    // A kind that this migration does not know, without an actor.
    db.execute(&format!(
        "INSERT INTO notifications (id, workspace_id, recipient_user_id, actor_user_id, kind, task_id, \
         dedupe_key, created_at) VALUES ('n6', '{workspace}', '{author}', NULL, 'a_later_kind', \
         '{task}', 'k6', 80)"
    ))
    .await
    .unwrap();
    // Not more than one target family.
    let error = db
        .execute(&format!(
            "INSERT INTO notifications (id, workspace_id, recipient_user_id, kind, task_id, page_id, \
             dedupe_key, created_at) VALUES ('n7', '{workspace}', '{member}', 'x', '{task}', \
             '{page}', 'k7', 80)"
        ))
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("CHECK constraint failed"), "{error}");
    // A deleted comment leaves the task's row.
    db.execute(&format!("DELETE FROM task_comments WHERE id = '{comment}'"))
        .await
        .unwrap();
    assert_eq!(
        db.scalar::<i64>(
            "SELECT COUNT(*) FROM notifications WHERE id IN ('n2', 'n4') AND comment_id IS NULL"
        )
        .await
        .unwrap(),
        2
    );

    // Only what was off is kept.
    assert_eq!(
        db.scalar::<String>(&format!(
            "SELECT GROUP_CONCAT(category || '=' || enabled || '@' || updated_at, ',') FROM ( \
             SELECT * FROM notification_pref_overrides WHERE user_id = '{member}' ORDER BY category)"
        ))
        .await
        .unwrap(),
        "chat_mentions=0@70,mentions=0@70"
    );
    assert_eq!(
        db.scalar::<i64>("SELECT COUNT(*) FROM notification_pref_overrides")
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        db.scalar::<i64>("SELECT COUNT(*) FROM sqlite_master WHERE name = 'notification_prefs'")
            .await
            .unwrap(),
        0
    );

    // Creator, assignee and comment author of the live tasks.
    let subscribers = |task: Id| {
        format!(
            "SELECT COALESCE(GROUP_CONCAT(display_name, ','), '') FROM (SELECT users.display_name \
             FROM task_subscribers JOIN users ON users.id = task_subscribers.user_id \
             WHERE task_id = '{task}' AND subscribed = 1 ORDER BY users.display_name)"
        )
    };
    assert_eq!(
        db.scalar::<String>(&subscribers(task)).await.unwrap(),
        "Author,Member,Owner"
    );
    assert_eq!(
        db.scalar::<String>(&subscribers(other)).await.unwrap(),
        "Owner"
    );
    assert_eq!(
        db.scalar::<String>(&subscribers(trashed)).await.unwrap(),
        ""
    );
    assert_eq!(
        db.scalar::<String>("PRAGMA integrity_check").await.unwrap(),
        "ok"
    );
}

#[tokio::test]
async fn find_and_organise_migration_indexes_tasks_and_moves_view_favorites() {
    let directory = tempfile::tempdir().unwrap();
    let db = Database::open(&DatabaseConfig::new(directory.path().join("db.sqlite")))
        .await
        .unwrap();
    MigrationRunner::embedded_through("test", 52)
        .run(&db)
        .await
        .unwrap();
    let [
        user,
        workspace,
        membership,
        project,
        status,
        plain,
        commented,
    ]: [Id; 7] = std::array::from_fn(|_| Id::new_v7());
    let [first, second]: [Id; 2] = std::array::from_fn(|_| Id::new_v7());
    db.execute(&format!(
        "BEGIN;
         INSERT INTO users (id, email, normalized_email, display_name, password_hash, created_at, updated_at)
         VALUES ('{user}', 'owner@example.com', 'owner@example.com', 'Owner', 'x', 1, 1);
         INSERT INTO workspaces (id, name, version, owner_membership_id, created_at, updated_at)
         VALUES ('{workspace}', 'Orbit', 0, '{membership}', 1, 1);
         INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, updated_at)
         VALUES ('{membership}', '{workspace}', '{user}', 'owner', 0, 1, 1);
         INSERT INTO projects (id, workspace_id, name, project_key, color, version, deleted_at, created_at, updated_at)
         VALUES ('{project}', '{workspace}', 'Engineering', 'ENG', '#000000', 0, NULL, 1, 1);
         INSERT INTO task_statuses (id, workspace_id, project_id, name, description, color, category, position, version, created_at, updated_at)
         VALUES ('{status}', '{workspace}', '{project}', 'Todo', '', '#ffffff', 'unstarted', 0, 0, 1, 1);
         INSERT INTO tasks (id, workspace_id, project_id, status_id, title, description, creator_id, created_at, updated_at)
         VALUES ('{plain}', '{workspace}', '{project}', '{status}', 'Über plan', 'Café notes', '{user}', 1, 1),
                ('{commented}', '{workspace}', '{project}', '{status}', 'Deploy', '', '{user}', 2, 2);
         INSERT INTO task_comments (id, workspace_id, task_id, author_id, body, created_at, updated_at)
         VALUES ('{first}', '{workspace}', '{commented}', '{user}', 'the rollback failed', 3, 3),
                ('{second}', '{workspace}', '{commented}', '{user}', 'retry tomorrow', 4, 4);
         COMMIT;"
    ))
    .await
    .unwrap();

    // A view favourite under the old table moves to `favorites`.
    let view = Id::new_v7();
    db.execute(&format!(
        "INSERT INTO saved_views (id, workspace_id, owner_user_id, name, visibility, state_json, created_at, updated_at)
         VALUES ('{view}', '{workspace}', '{user}', 'Mine', 'personal', '{{}}', 1, 1);
         INSERT INTO saved_view_favorites (view_id, user_id, position, created_at) VALUES ('{view}', '{user}', 4, 9);"
    ))
    .await
    .unwrap();

    MigrationRunner::embedded("test").run(&db).await.unwrap();

    assert_eq!(
        sqlx::query_as::<_, (String, String, String, String, i64, i64)>(
            "SELECT workspace_id, user_id, kind, target_id, position, created_at FROM favorites"
        )
        .fetch_all(db.pool())
        .await
        .unwrap(),
        [(
            workspace.to_string(),
            user.to_string(),
            "view".to_owned(),
            view.to_string(),
            4,
            9
        )]
    );
    assert_eq!(
        db.scalar::<i64>("SELECT COUNT(*) FROM sqlite_master WHERE name = 'saved_view_favorites'")
            .await
            .unwrap(),
        0
    );

    let matches = |query: &'static str| {
        let db = &db;
        async move {
            sqlx::query_scalar::<_, String>(
                "SELECT task_search_rows.task_id FROM task_search \
                 JOIN task_search_rows ON task_search_rows.id = task_search.rowid \
                 WHERE task_search MATCH ?",
            )
            .bind(query)
            .fetch_all(db.pool())
            .await
            .unwrap()
        }
    };
    // Backfill: titles, descriptions and the joined comments, diacritics folded.
    assert_eq!(matches("\"uber\"").await, [plain.to_string()]);
    assert_eq!(matches("\"cafe\"*").await, [plain.to_string()]);
    assert_eq!(
        matches("comments : \"rollback\" \"retry\"").await,
        [commented.to_string()]
    );

    // Task triggers: insert, repeated updates, delete.
    let added = Id::new_v7();
    db.execute(&format!(
        "INSERT INTO tasks (id, workspace_id, project_id, status_id, title, creator_id, created_at, updated_at) \
         VALUES ('{added}', '{workspace}', '{project}', '{status}', 'Zebra', '{user}', 5, 5)"
    ))
    .await
    .unwrap();
    assert_eq!(matches("\"zebra\"").await, [added.to_string()]);
    for title in ["Giraffe", "Okapi", "Tapir"] {
        db.execute(&format!(
            "UPDATE tasks SET title = '{title}', description = 'tall' WHERE id = '{added}'"
        ))
        .await
        .unwrap();
    }
    assert!(matches("\"zebra\"").await.is_empty());
    assert!(matches("\"okapi\"").await.is_empty());
    assert_eq!(matches("\"tapir\" \"tall\"").await, [added.to_string()]);

    // Comment triggers: insert, edit, delete.
    let third = Id::new_v7();
    db.execute(&format!(
        "INSERT INTO task_comments (id, workspace_id, task_id, author_id, body, created_at, updated_at) \
         VALUES ('{third}', '{workspace}', '{added}', '{user}', 'needs a ladder', 6, 6)"
    ))
    .await
    .unwrap();
    assert_eq!(matches("\"ladder\"").await, [added.to_string()]);
    db.execute(&format!(
        "UPDATE task_comments SET body = 'needs a crane' WHERE id = '{third}'"
    ))
    .await
    .unwrap();
    assert!(matches("\"ladder\"").await.is_empty());
    assert_eq!(matches("\"crane\"").await, [added.to_string()]);
    db.execute(&format!("DELETE FROM task_comments WHERE id = '{first}'"))
        .await
        .unwrap();
    assert!(matches("\"rollback\"").await.is_empty());
    assert_eq!(matches("\"retry\"").await, [commented.to_string()]);

    // FTS5's own check passes after repeated updates. `PRAGMA integrity_check` of the bundled
    // SQLite 3.46.0 can report a false "malformed inverted index" here; `IntegrityService::full`
    // (the scheduled job) confirms such a finding with FTS5's check and must accept the table.
    db.execute("INSERT INTO task_search (task_search) VALUES ('integrity-check')")
        .await
        .unwrap();
    orbit_platform::IntegrityService::new(db.clone())
        .full()
        .await
        .unwrap();

    // A task delete cascades to its comments; the index rows go with it.
    db.execute(&format!(
        "DELETE FROM tasks WHERE workspace_id = '{workspace}'"
    ))
    .await
    .unwrap();
    assert_eq!(
        db.scalar::<i64>("SELECT COUNT(*) FROM task_search")
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        db.scalar::<i64>("SELECT COUNT(*) FROM task_search_rows")
            .await
            .unwrap(),
        0
    );
    db.execute("INSERT INTO task_search (task_search) VALUES ('integrity-check')")
        .await
        .unwrap();
}
