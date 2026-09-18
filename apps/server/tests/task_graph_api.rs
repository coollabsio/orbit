use orbit_platform::{Id, TestDatabase};

#[tokio::test]
async fn migration_0016_adds_parent_column_indexes_and_graph_tables() {
    let database = TestDatabase::new().await.unwrap();

    let columns: Vec<String> = sqlx::query_scalar("SELECT name FROM pragma_table_info('tasks')")
        .fetch_all(database.pool())
        .await
        .unwrap();
    assert!(columns.contains(&"parent_id".to_owned()));

    let objects: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE name IN (\
         'tasks_parent', 'tasks_parent_rollup', 'task_relations', 'task_references', \
         'task_relations_one_duplicate', 'task_relations_target', 'task_references_target', \
         'tasks_validate_parent_insert', 'tasks_validate_parent_update', \
         'task_relations_validate_scope_insert', 'task_references_validate_scope_insert', \
         'tasks_cleanup_references', 'task_comments_cleanup_references') \
         ORDER BY name",
    )
    .fetch_all(database.pool())
    .await
    .unwrap();
    assert_eq!(objects.len(), 13, "missing schema objects: {objects:?}");

    let rollup: String =
        sqlx::query_scalar("SELECT sql FROM sqlite_master WHERE name = 'tasks_parent_rollup'")
            .fetch_one(database.pool())
            .await
            .unwrap();
    assert!(rollup.contains("WHERE deleted_at IS NULL AND parent_id IS NOT NULL"));
}

/// Seeds a workspace with one owner, one project and one status. Returns
/// `(workspace_id, project_id, status_id, user_id)`.
async fn seed_workspace(database: &TestDatabase, key: &str) -> (String, String, String, String) {
    let workspace_id = Id::new_v7().to_string();
    let project_id = Id::new_v7().to_string();
    let status_id = Id::new_v7().to_string();
    let user_id = Id::new_v7().to_string();
    let membership_id = Id::new_v7().to_string();
    let email = format!("{}@example.com", key.to_lowercase());
    database
        .execute(&format!(
            "BEGIN;\
             INSERT INTO users (id, email, normalized_email, display_name, password_hash, created_at, updated_at)\
             VALUES ('{user_id}', '{email}', '{email}', '{key}', 'unused', 1, 1);\
             INSERT INTO workspaces (id, name, version, owner_membership_id, created_at, updated_at)\
             VALUES ('{workspace_id}', '{key}', 0, '{membership_id}', 1, 1);\
             INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, updated_at)\
             VALUES ('{membership_id}', '{workspace_id}', '{user_id}', 'owner', 0, 1, 1);\
             INSERT INTO projects (id, workspace_id, name, project_key, color, version, created_at, updated_at)\
             VALUES ('{project_id}', '{workspace_id}', '{key}', '{key}', '#5e6ad2', 0, 1, 1);\
             INSERT INTO task_statuses (id, workspace_id, project_id, name, description, color, category, position, version, created_at, updated_at)\
             VALUES ('{status_id}', '{workspace_id}', '{project_id}', 'Todo', '', '#8b8f98', 'unstarted', 0, 0, 1, 1);\
             COMMIT;"
        ))
        .await
        .unwrap();
    (workspace_id, project_id, status_id, user_id)
}

async fn seed_task(
    database: &TestDatabase,
    scope: &(String, String, String, String),
    key: &str,
    number: i64,
) -> String {
    let id = Id::new_v7().to_string();
    sqlx::query(
        "INSERT INTO tasks (id, workspace_id, project_id, status_id, identifier_key, number, title, \
         priority, position, creator_id, version, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, 'none', 0, ?, 0, 1, 1)",
    )
    .bind(&id)
    .bind(&scope.0)
    .bind(&scope.1)
    .bind(&scope.2)
    .bind(key)
    .bind(number)
    .bind(format!("{key}-{number}"))
    .bind(&scope.3)
    .execute(database.pool())
    .await
    .unwrap();
    id
}

#[tokio::test]
async fn parent_trigger_rejects_cross_workspace_and_self_parenting() {
    let database = TestDatabase::new().await.unwrap();
    let first = seed_workspace(&database, "ALP").await;
    let second = seed_workspace(&database, "BET").await;
    let child = seed_task(&database, &first, "ALP", 1).await;
    let foreign = seed_task(&database, &second, "BET", 1).await;

    let cross = sqlx::query("UPDATE tasks SET parent_id = ? WHERE id = ?")
        .bind(&foreign)
        .bind(&child)
        .execute(database.pool())
        .await;
    assert!(cross.is_err(), "cross-workspace parent must abort");

    let itself = sqlx::query("UPDATE tasks SET parent_id = ? WHERE id = ?")
        .bind(&child)
        .bind(&child)
        .execute(database.pool())
        .await;
    assert!(itself.is_err(), "self-parenting must abort");

    let sibling = seed_task(&database, &first, "ALP", 2).await;
    sqlx::query("UPDATE tasks SET parent_id = ? WHERE id = ?")
        .bind(&sibling)
        .bind(&child)
        .execute(database.pool())
        .await
        .expect("same-workspace parent is allowed");

    // a relation or a reference cannot reach across workspaces either
    let relation = sqlx::query(
        "INSERT INTO task_relations (id, workspace_id, kind, source_task_id, target_task_id, created_by, created_at) \
         VALUES (?, ?, 'duplicate_of', ?, ?, ?, 1)",
    )
    .bind(Id::new_v7().to_string())
    .bind(&first.0)
    .bind(&child)
    .bind(&foreign)
    .bind(&first.3)
    .execute(database.pool())
    .await;
    assert!(relation.is_err(), "cross-workspace relation must abort");

    let reference = sqlx::query(
        "INSERT INTO task_references (workspace_id, source_type, source_id, target_task_id) \
         VALUES (?, 'task', ?, ?)",
    )
    .bind(&first.0)
    .bind(&child)
    .bind(&foreign)
    .execute(database.pool())
    .await;
    assert!(reference.is_err(), "cross-workspace reference must abort");
}
