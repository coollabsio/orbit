//! A database already on `main` (schema 18: API tokens, Discord, service accounts)
//! upgrading through this branch's 19-22. This is the path every existing install
//! takes, so it is tested with real pre-existing rows rather than fresh ones.
use orbit_platform::{Database, DatabaseConfig};
use orbit_server::migrations::migration_runner;

#[tokio::test]
async fn a_main_schema_database_upgrades_through_the_task_features() {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::open(&DatabaseConfig::new(directory.path().join("orbit.sqlite")))
        .await
        .unwrap();
    let runner = migration_runner();

    // Exactly main's schema: no identifier, document or graph columns yet.
    runner.run_through(&database, 18).await.unwrap();
    database
        .execute(
            "BEGIN;\
             INSERT INTO users (id, email, normalized_email, display_name, password_hash, created_at, updated_at)\
             VALUES ('user-1', 'a@example.com', 'a@example.com', 'A', 'unused', 1, 1);\
             INSERT INTO workspaces (id, name, version, owner_membership_id, created_at, updated_at)\
             VALUES ('workspace-1', 'Orbit', 0, 'membership-1', 1, 1);\
             INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, updated_at)\
             VALUES ('membership-1', 'workspace-1', 'user-1', 'owner', 0, 1, 1);\
             INSERT INTO projects (id, workspace_id, name, project_key, color, version, created_at, updated_at)\
             VALUES ('project-1', 'workspace-1', 'General', 'GEN', '#5e6ad2', 0, 1, 1);\
             INSERT INTO task_statuses (id, workspace_id, project_id, name, description, color, category, position, version, created_at, updated_at)\
             VALUES ('status-1', 'workspace-1', 'project-1', 'Todo', '', '#8b8f98', 'unstarted', 0, 0, 1, 1);\
             INSERT INTO tasks (id, workspace_id, project_id, status_id, title, description, priority, position, creator_id, version, created_at, updated_at)\
             VALUES ('task-b', 'workspace-1', 'project-1', 'status-1', 'Second', 'line one\nline two', 'none', 1, 'user-1', 0, 20, 20),\
                    ('task-a', 'workspace-1', 'project-1', 'status-1', 'First', '**bold** start', 'none', 0, 'user-1', 0, 10, 10);\
             INSERT INTO task_comments (id, workspace_id, task_id, author_id, body, version, created_at, updated_at)\
             VALUES ('comment-1', 'workspace-1', 'task-a', 'user-1', 'see [docs](https://example.com)', 0, 30, 30);\
             COMMIT;",
        )
        .await
        .unwrap();

    runner.run(&database).await.unwrap();

    assert_eq!(
        database
            .scalar::<i64>("SELECT MAX(version) FROM schema_migrations")
            .await
            .unwrap(),
        22
    );
    // Existing tasks are numbered by creation time, and the project counter continues after them.
    assert_eq!(
        database
            .scalar::<String>(
                "SELECT identifier_key || '-' || number FROM tasks WHERE id = 'task-a'"
            )
            .await
            .unwrap(),
        "GEN-1"
    );
    assert_eq!(
        database
            .scalar::<String>(
                "SELECT identifier_key || '-' || number FROM tasks WHERE id = 'task-b'"
            )
            .await
            .unwrap(),
        "GEN-2"
    );
    assert_eq!(
        database
            .scalar::<i64>("SELECT next_task_number FROM projects WHERE id = 'project-1'")
            .await
            .unwrap(),
        3
    );
    // Markdown became documents, and a lone newline stayed a line break.
    let first: serde_json::Value = serde_json::from_str(
        &database
            .scalar::<String>("SELECT description_json FROM tasks WHERE id = 'task-a'")
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        first["content"][0]["content"][0]["marks"][0]["type"],
        "bold"
    );
    assert_eq!(
        database
            .scalar::<String>("SELECT description_text FROM tasks WHERE id = 'task-b'")
            .await
            .unwrap(),
        "line one\nline two"
    );
    assert!(
        database
            .scalar::<String>("SELECT body_json FROM task_comments WHERE id = 'comment-1'")
            .await
            .unwrap()
            .contains("https://example.com")
    );
    // Pre-existing tasks must be in the search index, or search silently misses them
    // until each one happens to be edited.
    assert_eq!(
        database
            .scalar::<i64>("SELECT COUNT(*) FROM task_search")
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        database
            .scalar::<String>("SELECT task_id FROM task_search WHERE task_search MATCH 'body:bold'")
            .await
            .unwrap(),
        "task-a"
    );
    // The old Markdown columns are gone.
    assert_eq!(
        database
            .scalar::<i64>(
                "SELECT COUNT(*) FROM pragma_table_info('tasks') WHERE name = 'description'"
            )
            .await
            .unwrap(),
        0
    );
}
