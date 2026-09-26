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
        32
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
            binary_version: 32
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
    assert_eq!(statuses.len(), 3);
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
            "notification must belong to the workspace task or a page the recipient can see",
        ),
        (
            insert("thread", user, page, &format!("'{thread}'"), "NULL"),
            "CHECK constraint failed",
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
