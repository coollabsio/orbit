//! Custom workspace roles (issue #73). A custom role is a named list of extra permissions that an
//! ordinary member holds on top of the member role. Only the owner and admins manage roles
//! (`Permission::RolesManage`), and a role never carries a permission that
//! `Permission::is_grantable` refuses, so a role cannot give anyone the owner's powers.

use orbit_domain::{Permission, PermissionSet, WorkspaceRole};
use orbit_platform::{Id, Row, TimestampMillis};
use serde::Serialize;
use serde_json::json;
use sqlx::{Any, Transaction};
use utoipa::ToSchema;

use super::membership::parse_permissions;
use super::workspaces::{
    WorkspaceError, WorkspaceRepository, parse_id, require_actor, target_membership,
};
use crate::audit::{self, AuditOutcome};

const MAX_ROLES_PER_WORKSPACE: i64 = 50;
const MAX_NAME_CHARS: usize = 40;
const BUILT_IN_NAMES: [&str; 4] = ["owner", "admin", "member", "guest"];

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct WorkspaceRoleRecord {
    #[schema(value_type = String)]
    pub id: Id,
    pub name: String,
    /// What a member with this role may do beyond an ordinary member.
    pub permissions: Vec<Permission>,
    /// How many members hold the role.
    pub member_count: u64,
    pub version: u64,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: TimestampMillis,
    #[schema(value_type = String, format = DateTime)]
    pub updated_at: TimestampMillis,
}

const SELECT_ROLE: &str = "SELECT workspace_roles.id, workspace_roles.name, workspace_roles.permissions, \
     workspace_roles.version, workspace_roles.created_at, workspace_roles.updated_at, \
     CAST((SELECT COUNT(*) FROM memberships WHERE memberships.custom_role_id = workspace_roles.id \
     AND memberships.role = 'member') AS BIGINT) AS member_count \
     FROM workspace_roles WHERE workspace_roles.workspace_id = $1";

fn role_from_row(row: &sqlx::any::AnyRow) -> Result<WorkspaceRoleRecord, WorkspaceError> {
    let permissions =
        parse_permissions(row.get::<Option<String>, _>("permissions").as_deref()).to_vec();
    Ok(WorkspaceRoleRecord {
        id: parse_id(row.get("id"))?,
        name: row.get("name"),
        permissions,
        member_count: u64::try_from(row.get::<i64, _>("member_count")).unwrap_or(0),
        version: u64::try_from(row.get::<i64, _>("version"))
            .map_err(|_| WorkspaceError::Conflict)?,
        created_at: TimestampMillis::from_millis(row.get("created_at")),
        updated_at: TimestampMillis::from_millis(row.get("updated_at")),
    })
}

fn clean_name(name: &str) -> Result<String, WorkspaceError> {
    let name = name.trim();
    if name.is_empty()
        || name.chars().count() > MAX_NAME_CHARS
        || name.chars().any(char::is_control)
    {
        return Err(WorkspaceError::InvalidRole(
            "A role name has 1 to 40 characters.",
        ));
    }
    if BUILT_IN_NAMES.contains(&name.to_lowercase().as_str()) {
        return Err(WorkspaceError::InvalidRole(
            "Owner, Admin, Member and Guest are built-in roles.",
        ));
    }
    Ok(name.to_owned())
}

/// The permissions as stored: validated, without duplicates, in a fixed order.
fn clean_permissions(permissions: &[Permission]) -> Result<String, WorkspaceError> {
    if permissions
        .iter()
        .any(|permission| !permission.is_grantable())
    {
        return Err(WorkspaceError::InvalidRole(
            "A custom role cannot carry that permission.",
        ));
    }
    let names: Vec<&str> = PermissionSet::of(permissions.iter().copied())
        .to_vec()
        .into_iter()
        .map(Permission::as_str)
        .collect();
    Ok(json!(names).to_string())
}

impl WorkspaceRepository {
    /// Every custom role of the workspace, for any member: the members list names them.
    pub async fn list_roles(
        &self,
        workspace_id: Id,
        actor_id: Id,
    ) -> Result<Vec<WorkspaceRoleRecord>, WorkspaceError> {
        require_actor(self.database.pool(), workspace_id, actor_id).await?;
        let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
            "{SELECT_ROLE} ORDER BY lower(workspace_roles.name), workspace_roles.id"
        )))
        .bind(workspace_id.to_string())
        .fetch_all(self.database.pool())
        .await?;
        rows.iter().map(role_from_row).collect()
    }

    pub async fn create_role(
        &self,
        workspace_id: Id,
        actor_id: Id,
        name: &str,
        permissions: &[Permission],
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<WorkspaceRoleRecord, WorkspaceError> {
        let mut transaction = self.database.immediate_transaction().await?;
        if !may_manage_roles(&mut transaction, workspace_id, actor_id, request_id, now).await? {
            transaction.commit().await?;
            return Err(WorkspaceError::Forbidden);
        }
        let name = clean_name(name)?;
        let stored = clean_permissions(permissions)?;
        let count: i64 = sqlx::query_scalar(
            "SELECT CAST(COUNT(*) AS BIGINT) FROM workspace_roles WHERE workspace_id = $1",
        )
        .bind(workspace_id.to_string())
        .fetch_one(&mut *transaction)
        .await?;
        if count >= MAX_ROLES_PER_WORKSPACE {
            return Err(WorkspaceError::InvalidRole(
                "A workspace has at most 50 custom roles.",
            ));
        }
        ensure_name_free(&mut transaction, workspace_id, &name, None).await?;
        let id = Id::new_v7();
        sqlx::query(
            "INSERT INTO workspace_roles (id, workspace_id, name, permissions, version, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, 0, $5, $5)",
        )
        .bind(id.to_string())
        .bind(workspace_id.to_string())
        .bind(&name)
        .bind(&stored)
        .bind(now.as_millis())
        .execute(&mut *transaction)
        .await?;
        audit::record(
            &mut transaction,
            workspace_id,
            Some(actor_id),
            "role.created",
            AuditOutcome::Success,
            "workspace_role",
            Some(id),
            request_id,
            json!({"name": name, "permissions": stored_names(&stored)}),
            now,
        )
        .await?;
        let record = load_role(&mut transaction, workspace_id, id).await?;
        transaction.commit().await?;
        Ok(record)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update_role(
        &self,
        workspace_id: Id,
        actor_id: Id,
        role_id: Id,
        name: Option<&str>,
        permissions: Option<&[Permission]>,
        expected_version: u64,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<WorkspaceRoleRecord, WorkspaceError> {
        let mut transaction = self.database.immediate_transaction().await?;
        if !may_manage_roles(&mut transaction, workspace_id, actor_id, request_id, now).await? {
            transaction.commit().await?;
            return Err(WorkspaceError::Forbidden);
        }
        let current = load_role(&mut transaction, workspace_id, role_id).await?;
        if current.version != expected_version {
            return Err(WorkspaceError::VersionConflict {
                current_version: current.version,
            });
        }
        let name = match name {
            Some(name) => clean_name(name)?,
            None => current.name.clone(),
        };
        let stored = match permissions {
            Some(permissions) => clean_permissions(permissions)?,
            None => clean_permissions(&current.permissions)?,
        };
        ensure_name_free(&mut transaction, workspace_id, &name, Some(role_id)).await?;
        sqlx::query(
            "UPDATE workspace_roles SET name = $1, permissions = $2, version = version + 1, updated_at = $3 \
             WHERE id = $4 AND workspace_id = $5",
        )
        .bind(&name)
        .bind(&stored)
        .bind(now.as_millis())
        .bind(role_id.to_string())
        .bind(workspace_id.to_string())
        .execute(&mut *transaction)
        .await?;
        audit::record(
            &mut transaction,
            workspace_id,
            Some(actor_id),
            "role.updated",
            AuditOutcome::Success,
            "workspace_role",
            Some(role_id),
            request_id,
            json!({"name": name, "permissions": stored_names(&stored)}),
            now,
        )
        .await?;
        let record = load_role(&mut transaction, workspace_id, role_id).await?;
        transaction.commit().await?;
        Ok(record)
    }

    /// Deletes the role; its members become ordinary members.
    pub async fn delete_role(
        &self,
        workspace_id: Id,
        actor_id: Id,
        role_id: Id,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), WorkspaceError> {
        let mut transaction = self.database.immediate_transaction().await?;
        if !may_manage_roles(&mut transaction, workspace_id, actor_id, request_id, now).await? {
            transaction.commit().await?;
            return Err(WorkspaceError::Forbidden);
        }
        let role = load_role(&mut transaction, workspace_id, role_id).await?;
        sqlx::query("DELETE FROM workspace_roles WHERE id = $1 AND workspace_id = $2")
            .bind(role_id.to_string())
            .bind(workspace_id.to_string())
            .execute(&mut *transaction)
            .await?;
        audit::record(
            &mut transaction,
            workspace_id,
            Some(actor_id),
            "role.deleted",
            AuditOutcome::Success,
            "workspace_role",
            Some(role_id),
            request_id,
            json!({"name": role.name, "members": role.member_count}),
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    /// Gives an ordinary member a custom role, or takes it away (`role_id` is `None`).
    #[allow(clippy::too_many_arguments)]
    pub async fn set_member_custom_role(
        &self,
        workspace_id: Id,
        actor_id: Id,
        membership_id: Id,
        role_id: Option<Id>,
        expected_version: u64,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), WorkspaceError> {
        let mut transaction = self.database.immediate_transaction().await?;
        if !may_manage_roles(&mut transaction, workspace_id, actor_id, request_id, now).await? {
            transaction.commit().await?;
            return Err(WorkspaceError::Forbidden);
        }
        let target = target_membership(&mut transaction, workspace_id, membership_id).await?;
        if target.version != expected_version {
            return Err(WorkspaceError::VersionConflict {
                current_version: target.version,
            });
        }
        if target.role != WorkspaceRole::Member {
            return Err(WorkspaceError::InvalidRole(
                "Only an ordinary member holds a custom role.",
            ));
        }
        if let Some(role_id) = role_id {
            load_role(&mut transaction, workspace_id, role_id).await?;
        }
        sqlx::query(
            "UPDATE memberships SET custom_role_id = $1, version = version + 1, updated_at = $2 \
             WHERE id = $3 AND workspace_id = $4",
        )
        .bind(role_id.map(|id| id.to_string()))
        .bind(now.as_millis())
        .bind(membership_id.to_string())
        .bind(workspace_id.to_string())
        .execute(&mut *transaction)
        .await?;
        audit::record(
            &mut transaction,
            workspace_id,
            Some(actor_id),
            "membership.custom_role_changed",
            AuditOutcome::Success,
            "membership",
            Some(membership_id),
            request_id,
            json!({"role_id": role_id.map(|id| id.to_string())}),
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }
}

fn stored_names(stored: &str) -> serde_json::Value {
    serde_json::from_str(stored).unwrap_or_default()
}

/// Whether the acting member manages roles. If not, the denial is audited, and the caller
/// commits it and answers `Forbidden`.
async fn may_manage_roles(
    transaction: &mut Transaction<'_, Any>,
    workspace_id: Id,
    actor_id: Id,
    request_id: &str,
    now: TimestampMillis,
) -> Result<bool, WorkspaceError> {
    let actor = require_actor(&mut **transaction, workspace_id, actor_id).await?;
    if actor.can(Permission::RolesManage) {
        return Ok(true);
    }
    audit::record(
        transaction,
        workspace_id,
        Some(actor_id),
        "role.manage_denied",
        AuditOutcome::Failure,
        "workspace_role",
        None,
        request_id,
        json!({"reason": "role_forbidden"}),
        now,
    )
    .await?;
    Ok(false)
}

async fn load_role(
    transaction: &mut Transaction<'_, Any>,
    workspace_id: Id,
    role_id: Id,
) -> Result<WorkspaceRoleRecord, WorkspaceError> {
    let row = sqlx::query(sqlx::AssertSqlSafe(format!(
        "{SELECT_ROLE} AND workspace_roles.id = $2"
    )))
    .bind(workspace_id.to_string())
    .bind(role_id.to_string())
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(WorkspaceError::NotFound)?;
    role_from_row(&row)
}

async fn ensure_name_free(
    transaction: &mut Transaction<'_, Any>,
    workspace_id: Id,
    name: &str,
    except: Option<Id>,
) -> Result<(), WorkspaceError> {
    let taken: i64 = sqlx::query_scalar(
        "SELECT CAST(COUNT(*) AS BIGINT) FROM workspace_roles WHERE workspace_id = $1 \
         AND lower(name) = lower($2) AND ($3 IS NULL OR id <> $4)",
    )
    .bind(workspace_id.to_string())
    .bind(name)
    .bind(except.map(|id| id.to_string()))
    .bind(except.map(|id| id.to_string()))
    .fetch_one(&mut **transaction)
    .await?;
    if taken > 0 {
        return Err(WorkspaceError::RoleNameTaken);
    }
    Ok(())
}
