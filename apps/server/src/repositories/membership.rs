//! Turns a user into an acting workspace member (`Actor`), which carries the role every
//! permission rule needs. Checks that only ask "is this user a member" together with something
//! else in one query (a live session, a visible page, an OAuth grant) keep their own SQL.

use orbit_domain::{Actor, WorkspaceRole};
use orbit_platform::Id;
use sqlx::Sqlite;

/// The caller as a member of a live workspace, or `None` for outsiders and trashed workspaces.
/// Repositories call this inside their own transaction, so the rule holds for every caller
/// (HTTP sessions, API tokens, MCP, webhooks) and cannot go stale before the write.
pub(crate) async fn actor<'e, E>(
    executor: E,
    workspace_id: Id,
    user_id: Id,
) -> Result<Option<Actor>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = Sqlite>,
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
    E: sqlx::Executor<'e, Database = Sqlite>,
{
    let role: Option<String> = sqlx::query_scalar(
        "SELECT memberships.role FROM memberships JOIN workspaces ON workspaces.id = memberships.workspace_id \
         WHERE memberships.workspace_id = ? AND memberships.user_id = ? \
         AND (workspaces.deleted_at IS NOT NULL) = ?",
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
