//! The 0014 -> 0015 backfill. Seeds Markdown rows against a database stopped at
//! schema 13, then runs the real runner through 15 and asserts the conversion.
use orbit_platform::{Database, DatabaseConfig};
use orbit_server::migrations::migration_runner;

async fn database() -> (tempfile::TempDir, Database) {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::open(&DatabaseConfig::new(directory.path().join("orbit.sqlite")))
        .await
        .unwrap();
    (directory, database)
}

async fn seed(database: &Database) {
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
             INSERT INTO tasks (id, workspace_id, project_id, status_id, identifier_key, number, title, description, priority, position, creator_id, version, created_at, updated_at)\
             VALUES ('task-1', 'workspace-1', 'project-1', 'status-1', 'GEN', 1, 'Ship it',\
                 '# Plan\n\n- **bold** item\n- plain item\n\nping @Ada Lovelace', 'none', 0, 'user-1', 0, 1, 1);\
             INSERT INTO task_comments (id, workspace_id, task_id, author_id, body, version, created_at, updated_at)\
             VALUES ('comment-1', 'workspace-1', 'task-1', 'user-1', 'see [docs](https://example.com)', 0, 1, 1);\
             COMMIT;",
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn the_backfill_converts_markdown_before_the_columns_are_dropped() {
    let (_directory, database) = database().await;
    let runner = migration_runner();

    // Stop at 13: the Markdown columns still exist and the rich text ones do not.
    runner.run_through(&database, 13).await.unwrap();
    seed(&database).await;

    runner.run(&database).await.unwrap();

    let description_json: String = database
        .scalar("SELECT description_json FROM tasks WHERE id = 'task-1'")
        .await
        .unwrap();
    let document: serde_json::Value = serde_json::from_str(&description_json).unwrap();
    assert_eq!(orbit_domain::rich_text::validate(&document), Ok(()));
    assert_eq!(document["content"][0]["type"], "heading");
    assert_eq!(document["content"][0]["attrs"]["level"], 1);
    assert_eq!(document["content"][1]["type"], "bulletList");
    // Formatting survives the round trip rather than arriving as literal asterisks.
    assert_eq!(
        document["content"][1]["content"][0]["content"][0]["content"][0]["marks"][0]["type"],
        "bold"
    );

    let description_text: String = database
        .scalar("SELECT description_text FROM tasks WHERE id = 'task-1'")
        .await
        .unwrap();
    assert_eq!(
        description_text,
        "Plan\nbold item\nplain item\nping @Ada Lovelace"
    );

    let body_json: String = database
        .scalar("SELECT body_json FROM task_comments WHERE id = 'comment-1'")
        .await
        .unwrap();
    let comment: serde_json::Value = serde_json::from_str(&body_json).unwrap();
    assert_eq!(
        comment["content"][0]["content"][1]["marks"][0]["attrs"]["href"],
        "https://example.com"
    );

    // The one-way loss: an old @Name cannot become an id-based mention.
    assert!(orbit_domain::rich_text::extract_user_ids(&document).is_empty());

    // The Markdown source is still present. 0015_drop_markdown.sql exists but is deliberately
    // NOT registered yet: the repository still reads these columns, and a column must not be
    // dropped before the code stops using it. It is registered once the repository writes
    // documents instead.
    for (table, column) in [("tasks", "description"), ("task_comments", "body")] {
        let count: i64 = database
            .scalar(&format!(
                "SELECT COUNT(*) FROM pragma_table_info('{table}') WHERE name = '{column}'"
            ))
            .await
            .unwrap();
        assert_eq!(count, 1, "{table}.{column} is still read by the repository");
    }
}
