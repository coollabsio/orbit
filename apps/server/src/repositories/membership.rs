//! Turns a user into an acting workspace member (`Actor`), which carries the role every
//! permission rule needs. Checks that only ask "is this user a member" together with something
//! else in one query (a live session, a visible page, an OAuth grant) keep their own SQL.

use orbit_domain::{Actor, WorkspaceRole};
use orbit_platform::Id;
use sqlx::Any;

/// The caller as a member of a live workspace, or `None` for outsiders and trashed workspaces.
/// Repositories call this inside their own transaction, so the rule holds for every caller
/// (HTTP sessions, API tokens, MCP, webhooks) and cannot go stale before the write.
pub(crate) async fn actor<'e, E>(
    executor: E,
    workspace_id: Id,
    user_id: Id,
) -> Result<Option<Actor>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = Any>,
{
    actor_in(executor, workspace_id, user_id, false).await
}

/// As `actor`, for a workspace in the trash when `trashed` is set.
pub(crate) async fn actor_in<'e, E>(
    executor: E,
    workspace_id: Id,
    user_id: Id,
    trashed: bool,
) -> Result<Option<Actor>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = Any>,
{
    let role: Option<String> = sqlx::query_scalar(
        "SELECT memberships.role FROM memberships JOIN workspaces ON workspaces.id = memberships.workspace_id \
         WHERE memberships.workspace_id = $1 AND memberships.user_id = $2 \
         AND (workspaces.deleted_at IS NOT NULL) = $3",
    )
    .bind(workspace_id.to_string())
    .bind(user_id.to_string())
    .bind(trashed)
    .fetch_optional(executor)
    .await?;
    // The schema only stores known roles; an unknown one grants nothing.
    Ok(role
        .as_deref()
        .and_then(WorkspaceRole::parse)
        .map(|role| Actor { user_id, role }))
}

/// As `actor`, on any SeaORM connection or transaction (spike: used by `api_tokens`).
pub(crate) async fn actor_orm(
    connection: &impl sea_orm::ConnectionTrait,
    workspace_id: Id,
    user_id: Id,
) -> Result<Option<Actor>, sea_orm::DbErr> {
    use sea_orm::{ColumnTrait, EntityTrait, JoinType, QueryFilter, QuerySelect, RelationTrait};

    use crate::entities::{memberships, workspaces};

    let role: Option<String> = memberships::Entity::find()
        .select_only()
        .column(memberships::Column::Role)
        .join(JoinType::InnerJoin, memberships::Relation::Workspace.def())
        .filter(memberships::Column::WorkspaceId.eq(workspace_id.to_string()))
        .filter(memberships::Column::UserId.eq(user_id.to_string()))
        .filter(workspaces::Column::DeletedAt.is_null())
        .into_tuple()
        .one(connection)
        .await?;
    Ok(role
        .as_deref()
        .and_then(WorkspaceRole::parse)
        .map(|role| Actor { user_id, role }))
}
