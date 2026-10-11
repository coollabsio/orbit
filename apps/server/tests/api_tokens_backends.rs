//! The `ApiTokenRepository` on the migrated schema: SQLite by default, Postgres when
//! `ORBIT_TEST_POSTGRES_URL` is set (see `TestDatabase`).

use orbit_platform::{Database, Id, TestDatabase, TimestampMillis};
use orbit_server::repositories::api_tokens::{ApiTokenError, ApiTokenRepository, ApiTokenScope};

struct Seed {
    owner: Id,
    outsider: Id,
    workspace: Id,
    projects: [Id; 2],
    foreign_project: Id,
}

async fn seed(database: &Database, now: i64) -> Seed {
    let seed = Seed {
        owner: Id::new_v7(),
        outsider: Id::new_v7(),
        workspace: Id::new_v7(),
        projects: [Id::new_v7(), Id::new_v7()],
        foreign_project: Id::new_v7(),
    };
    let foreign_workspace = Id::new_v7();
    let mut sql = String::new();
    for (id, email) in [
        (seed.owner, "owner@example.com"),
        (seed.outsider, "outsider@example.com"),
    ] {
        sql.push_str(&format!(
            "INSERT INTO users (id, email, normalized_email, display_name, password_hash, \
             created_at, updated_at) VALUES ('{id}', '{email}', '{email}', '{email}', 'x', {now}, {now});"
        ));
    }
    // A workspace and its owner membership reference each other (deferred foreign key), so both
    // go in one transaction.
    for (workspace, owner, name) in [
        (seed.workspace, seed.owner, "Orbit"),
        (foreign_workspace, seed.outsider, "Other"),
    ] {
        let membership = Id::new_v7();
        sql.push_str(&format!(
            "INSERT INTO workspaces (id, name, owner_membership_id, created_at, updated_at) \
             VALUES ('{workspace}', '{name}', '{membership}', {now}, {now}); \
             INSERT INTO memberships (id, workspace_id, user_id, role, created_at, updated_at) \
             VALUES ('{membership}', '{workspace}', '{owner}', 'owner', {now}, {now});"
        ));
    }
    for (offset, (id, workspace, key)) in [
        (seed.projects[0], seed.workspace, "ONE"),
        (seed.projects[1], seed.workspace, "TWO"),
        (seed.foreign_project, foreign_workspace, "FOR"),
    ]
    .into_iter()
    .enumerate()
    {
        sql.push_str(&format!(
            "INSERT INTO projects (id, workspace_id, name, project_key, color, created_at, updated_at) \
             VALUES ('{id}', '{workspace}', '{key}', '{key}', '#ff0000', {}, {now});",
            now + offset as i64
        ));
    }
    let mut transaction = database.immediate_transaction().await.unwrap();
    sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
        .execute(&mut *transaction)
        .await
        .unwrap();
    transaction.commit().await.unwrap();
    seed
}

/// Create, list, authenticate and revoke tokens; the expected results are the same on every backend.
async fn scenario(database: &Database) {
    let now = TimestampMillis::now();
    let seed = seed(database, now.as_millis()).await;
    let tokens = ApiTokenRepository::new(database.clone());

    let reader = tokens
        .create(
            seed.workspace,
            seed.owner,
            "Reader".into(),
            seed.projects.to_vec(),
            true,
            false,
            false,
            None,
            "req-1",
            now,
        )
        .await
        .unwrap();
    let principal = tokens
        .authenticate(&reader.token, ApiTokenScope::Read, now)
        .await
        .unwrap();
    assert_eq!(principal.workspace_id, seed.workspace);
    assert_eq!(principal.creator_id, seed.owner);
    assert_eq!(principal.project_ids, seed.projects.to_vec());
    assert!(matches!(
        tokens
            .authenticate(&reader.token, ApiTokenScope::Write, now)
            .await,
        Err(ApiTokenError::Forbidden)
    ));

    // Service accounts are matched by name without case.
    let bot = tokens
        .create(
            seed.workspace,
            seed.owner,
            "Bot".into(),
            vec![seed.projects[1]],
            true,
            true,
            true,
            Some(TimestampMillis::from_millis(now.as_millis() + 60_000)),
            "req-2",
            now,
        )
        .await
        .unwrap();
    let bot_again = tokens
        .create(
            seed.workspace,
            seed.owner,
            "bot".into(),
            vec![seed.projects[1]],
            true,
            false,
            true,
            None,
            "req-3",
            now,
        )
        .await
        .unwrap();
    assert!(bot.api_token.service_account_id.is_some());
    assert_eq!(
        bot.api_token.service_account_id,
        bot_again.api_token.service_account_id
    );
    let bot_principal = tokens
        .authenticate(&bot.token, ApiTokenScope::Write, now)
        .await
        .unwrap();
    assert_eq!(
        bot_principal.service_account_id,
        bot.api_token.service_account_id
    );
    let expired = TimestampMillis::from_millis(now.as_millis() + 120_000);
    assert!(matches!(
        tokens
            .authenticate(&bot.token, ApiTokenScope::Read, expired)
            .await,
        Err(ApiTokenError::NotFound)
    ));

    tokens
        .revoke(
            seed.workspace,
            reader.api_token.id,
            seed.owner,
            "req-4",
            now,
        )
        .await
        .unwrap();
    assert!(matches!(
        tokens
            .authenticate(&reader.token, ApiTokenScope::Read, now)
            .await,
        Err(ApiTokenError::NotFound)
    ));
    assert!(matches!(
        tokens
            .revoke(
                seed.workspace,
                reader.api_token.id,
                seed.owner,
                "req-5",
                now
            )
            .await,
        Err(ApiTokenError::NotFound)
    ));

    let listed = tokens.list(seed.workspace, seed.owner).await.unwrap();
    assert_eq!(listed.len(), 2);
    assert!(
        listed
            .iter()
            .all(|token| token.service_account_name.as_deref() == Some("Bot"))
    );
    assert_eq!(listed[0].scopes, ["read"]);
    assert_eq!(listed[1].scopes, ["read", "write"]);
    assert_eq!(listed[1].last_used_at, Some(now));
    assert!(matches!(
        tokens.list(seed.workspace, seed.outsider).await,
        Err(ApiTokenError::NotFound)
    ));
    assert_eq!(
        database
            .scalar::<i64>("SELECT COUNT(*) FROM audit_events")
            .await
            .unwrap(),
        4
    );

    // The schema trigger refuses a project from another workspace on both backends.
    let refused = database
        .execute(&format!(
            "INSERT INTO api_token_projects (token_id, project_id) VALUES ('{}', '{}')",
            bot.api_token.id, seed.foreign_project
        ))
        .await
        .unwrap_err();
    assert!(
        refused
            .to_string()
            .contains("api token project must be active and belong to the workspace"),
        "{refused}"
    );
}

#[tokio::test]
async fn api_tokens_on_the_configured_backend() {
    let database = TestDatabase::new().await.unwrap();
    scenario(&database).await;
}
