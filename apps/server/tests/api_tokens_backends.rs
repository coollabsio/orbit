//! The SeaORM `ApiTokenRepository` on the migrated schema: SQLite by default, Postgres when
//! `ORBIT_TEST_POSTGRES_URL` is set (see `TestDatabase`).

use orbit_platform::{Id, TestDatabase, TimestampMillis};
use orbit_server::entities::{
    api_token_projects, audit_events, memberships, projects, users, workspaces,
};
use orbit_server::repositories::api_tokens::{ApiTokenError, ApiTokenRepository, ApiTokenScope};
use sea_orm::ActiveValue::{NotSet, Set};
use sea_orm::{DatabaseConnection, EntityTrait, PaginatorTrait, TransactionTrait};

struct Seed {
    owner: Id,
    outsider: Id,
    workspace: Id,
    projects: [Id; 2],
    foreign_project: Id,
}

async fn seed(connection: &DatabaseConnection, now: i64) -> Seed {
    let seed = Seed {
        owner: Id::new_v7(),
        outsider: Id::new_v7(),
        workspace: Id::new_v7(),
        projects: [Id::new_v7(), Id::new_v7()],
        foreign_project: Id::new_v7(),
    };
    let foreign_workspace = Id::new_v7();
    for (id, email) in [
        (seed.owner, "owner@example.com"),
        (seed.outsider, "outsider@example.com"),
    ] {
        users::Entity::insert(users::ActiveModel {
            id: Set(id.to_string()),
            email: Set(email.into()),
            normalized_email: Set(email.into()),
            display_name: Set(email.into()),
            password_hash: Set("x".into()),
            suspended_at: NotSet,
            created_at: Set(now),
            updated_at: Set(now),
        })
        .exec_without_returning(connection)
        .await
        .unwrap();
    }
    // A workspace and its owner membership reference each other (deferred foreign key), so both
    // go in one transaction.
    let transaction = connection.begin().await.unwrap();
    for (workspace, owner, name) in [
        (seed.workspace, seed.owner, "Orbit"),
        (foreign_workspace, seed.outsider, "Other"),
    ] {
        let membership = Id::new_v7().to_string();
        workspaces::Entity::insert(workspaces::ActiveModel {
            id: Set(workspace.to_string()),
            name: Set(name.into()),
            owner_membership_id: Set(Some(membership.clone())),
            deleted_at: NotSet,
            created_at: Set(now),
            updated_at: Set(now),
        })
        .exec_without_returning(&transaction)
        .await
        .unwrap();
        memberships::Entity::insert(memberships::ActiveModel {
            id: Set(membership),
            workspace_id: Set(workspace.to_string()),
            user_id: Set(owner.to_string()),
            role: Set("owner".into()),
            created_at: Set(now),
            updated_at: Set(now),
        })
        .exec_without_returning(&transaction)
        .await
        .unwrap();
    }
    transaction.commit().await.unwrap();
    for (offset, (id, workspace, key)) in [
        (seed.projects[0], seed.workspace, "ONE"),
        (seed.projects[1], seed.workspace, "TWO"),
        (seed.foreign_project, foreign_workspace, "FOR"),
    ]
    .into_iter()
    .enumerate()
    {
        projects::Entity::insert(projects::ActiveModel {
            id: Set(id.to_string()),
            workspace_id: Set(workspace.to_string()),
            name: Set(key.into()),
            project_key: Set(key.into()),
            color: Set("#ff0000".into()),
            deleted_at: NotSet,
            created_at: Set(now + offset as i64),
            updated_at: Set(now),
        })
        .exec_without_returning(connection)
        .await
        .unwrap();
    }
    seed
}

/// Create, list, authenticate and revoke tokens; the expected results are the same on every backend.
async fn scenario(connection: DatabaseConnection) {
    let now = TimestampMillis::now();
    let seed = seed(&connection, now.as_millis()).await;
    let tokens = ApiTokenRepository::with_connection(connection.clone());

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
        audit_events::Entity::find()
            .count(&connection)
            .await
            .unwrap(),
        4
    );

    // The schema trigger refuses a project from another workspace on both backends.
    let refused = api_token_projects::Entity::insert(api_token_projects::ActiveModel {
        token_id: Set(bot.api_token.id.to_string()),
        project_id: Set(seed.foreign_project.to_string()),
    })
    .exec_without_returning(&connection)
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
async fn api_tokens_through_the_orm() {
    let database = TestDatabase::new().await.unwrap();
    scenario(database.orm().clone()).await;
}
