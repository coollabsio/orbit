//! Turns a user into an acting workspace member (`Actor`), which carries the role every
//! permission rule needs. Checks that only ask "is this user a member" together with something
//! else in one query (a live session, a visible page, an OAuth grant) keep their own SQL.

use orbit_domain::{Actor, Permission, PermissionSet, WorkspaceRole};
use orbit_platform::Id;
use sqlx::Any;

/// The caller as a member of a live workspace, or `None` for outsiders, guests and trashed
/// workspaces. Repositories call this inside their own transaction, so the rule holds for every
/// caller (HTTP sessions, API tokens, MCP, webhooks) and cannot go stale before the write.
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
    Ok(any_actor(executor, workspace_id, user_id, trashed)
        .await?
        .filter(|actor| !actor.is_guest()))
}

/// As `actor`, but guests too: only chat calls it, and chat checks each conversation itself.
pub(crate) async fn chat_actor<'e, E>(
    executor: E,
    workspace_id: Id,
    user_id: Id,
) -> Result<Option<Actor>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = Any>,
{
    any_actor(executor, workspace_id, user_id, false).await
}

async fn any_actor<'e, E>(
    executor: E,
    workspace_id: Id,
    user_id: Id,
    trashed: bool,
) -> Result<Option<Actor>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = Any>,
{
    let row: Option<(String, Option<String>)> = sqlx::query_as(
        "SELECT memberships.role, workspace_roles.permissions \
         FROM memberships JOIN workspaces ON workspaces.id = memberships.workspace_id \
         LEFT JOIN workspace_roles ON workspace_roles.id = memberships.custom_role_id \
         WHERE memberships.workspace_id = $1 AND memberships.user_id = $2 \
         AND (CASE WHEN workspaces.deleted_at IS NULL THEN 0 ELSE 1 END) = $3",
    )
    .bind(workspace_id.to_string())
    .bind(user_id.to_string())
    .bind(i64::from(trashed))
    .fetch_optional(executor)
    .await?;
    // The schema only stores known roles; an unknown one grants nothing.
    Ok(row.and_then(|(role, permissions)| {
        WorkspaceRole::parse(&role).map(|role| {
            Actor::new(user_id, role).with_custom_role(parse_permissions(permissions.as_deref()))
        })
    }))
}

/// The permissions that a custom role stores (a JSON array of names). Unknown names and names
/// that a custom role may not carry add nothing.
pub(crate) fn parse_permissions(stored: Option<&str>) -> PermissionSet {
    let names: Vec<String> = stored
        .and_then(|stored| serde_json::from_str(stored).ok())
        .unwrap_or_default();
    PermissionSet::of(names.iter().filter_map(|name| Permission::parse(name)))
}
