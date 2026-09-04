use std::fmt;

use orbit_domain::WorkspaceRole;
use orbit_platform::{Database, Id, TimestampMillis, generate_opaque_token, normalize_email};
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::{Row, Sqlite, Transaction};
use thiserror::Error;

use crate::audit::{self, AuditEvent};

const INVITATION_LIFETIME_MILLIS: i64 = 7 * 24 * 60 * 60 * 1_000;

#[derive(Clone, Debug, Serialize)]
pub struct WorkspaceRecord {
    pub id: Id,
    pub name: String,
    pub role: String,
    pub version: u64,
    pub deleted_at: Option<TimestampMillis>,
}

#[derive(Clone, Debug, Serialize)]
pub struct MemberRecord {
    pub id: Id,
    pub user_id: Id,
    pub email: String,
    pub display_name: String,
    pub role: String,
    pub version: u64,
    pub created_at: TimestampMillis,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InvitationDelivery {
    Manual,
    Smtp,
}

impl InvitationDelivery {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Smtp => "smtp",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct InvitationRecord {
    pub id: Id,
    pub workspace_id: Id,
    pub email: String,
    pub role: String,
    pub delivery: String,
    pub status: String,
    pub expires_at: TimestampMillis,
    pub created_at: TimestampMillis,
}

#[derive(Clone, Eq, PartialEq)]
pub struct IssuedInvitation {
    pub invitation: InvitationRecord,
    pub token: String,
}

impl fmt::Debug for IssuedInvitation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("IssuedInvitation")
            .field("invitation", &self.invitation)
            .field("token", &"[REDACTED]")
            .finish()
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct AcceptanceRecord {
    pub workspace_id: Id,
    pub membership_id: Id,
    pub created: bool,
    pub email_verified: bool,
}

#[derive(Debug, Error)]
pub enum WorkspaceError {
    #[error("workspace resource was not found")]
    NotFound,
    #[error("workspace action is not permitted")]
    Forbidden,
    #[error("workspace owner must transfer ownership first")]
    TransferRequired,
    #[error("workspace invitation is invalid or expired")]
    InvalidInvitation,
    #[error("workspace invitation belongs to another email address")]
    EmailMismatch,
    #[error("workspace operation conflicts with current state")]
    Conflict,
    #[error("workspace repository is unavailable")]
    Unavailable(#[from] sqlx::Error),
}

#[derive(Clone, Debug)]
pub struct WorkspaceRepository {
    database: Database,
}

impl WorkspaceRepository {
    #[must_use]
    pub fn new(database: Database) -> Self {
        Self { database }
    }

    pub async fn list_for_user(&self, user_id: Id) -> Result<Vec<WorkspaceRecord>, WorkspaceError> {
        let rows = sqlx::query(
            "SELECT workspaces.id, workspaces.name, workspaces.version, workspaces.deleted_at, \
             memberships.role FROM workspaces JOIN memberships ON memberships.workspace_id = workspaces.id \
             WHERE memberships.user_id = ? AND workspaces.deleted_at IS NULL ORDER BY workspaces.id",
        )
        .bind(user_id.to_string())
        .fetch_all(self.database.pool())
        .await?;
        rows.into_iter().map(workspace_from_row).collect()
    }

    pub async fn list_trash(&self, user_id: Id) -> Result<Vec<WorkspaceRecord>, WorkspaceError> {
        let rows = sqlx::query(
            "SELECT workspaces.id, workspaces.name, workspaces.version, workspaces.deleted_at, \
             memberships.role FROM workspaces JOIN memberships ON memberships.workspace_id = workspaces.id \
             WHERE memberships.user_id = ? AND memberships.role = 'owner' AND workspaces.deleted_at IS NOT NULL \
             ORDER BY workspaces.id DESC",
        )
        .bind(user_id.to_string())
        .fetch_all(self.database.pool())
        .await?;
        rows.into_iter().map(workspace_from_row).collect()
    }

    pub async fn create(
        &self,
        actor_id: Id,
        name: String,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<WorkspaceRecord, WorkspaceError> {
        let id = Id::new_v7();
        let membership_id = Id::new_v7();
        let mut transaction = self.database.immediate_transaction().await?;
        sqlx::query(
            "INSERT INTO workspaces (id, name, version, created_at, updated_at) VALUES (?, ?, 0, ?, ?)",
        )
        .bind(id.to_string())
        .bind(&name)
        .bind(now.as_millis())
        .bind(now.as_millis())
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, updated_at) \
             VALUES (?, ?, ?, 'owner', 0, ?, ?)",
        )
        .bind(membership_id.to_string())
        .bind(id.to_string())
        .bind(actor_id.to_string())
        .bind(now.as_millis())
        .bind(now.as_millis())
        .execute(&mut *transaction)
        .await?;
        audit::record(
            &mut transaction,
            id,
            Some(actor_id),
            "workspace.created",
            "workspace",
            Some(id),
            request_id,
            json!({}),
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(WorkspaceRecord {
            id,
            name,
            role: "owner".to_owned(),
            version: 0,
            deleted_at: None,
        })
    }

    pub async fn get(
        &self,
        workspace_id: Id,
        actor_id: Id,
    ) -> Result<WorkspaceRecord, WorkspaceError> {
        let row = sqlx::query(
            "SELECT workspaces.id, workspaces.name, workspaces.version, workspaces.deleted_at, memberships.role \
             FROM workspaces JOIN memberships ON memberships.workspace_id = workspaces.id \
             WHERE workspaces.id = ? AND memberships.user_id = ? AND workspaces.deleted_at IS NULL",
        )
        .bind(workspace_id.to_string())
        .bind(actor_id.to_string())
        .fetch_optional(self.database.pool())
        .await?
        .ok_or(WorkspaceError::NotFound)?;
        workspace_from_row(row)
    }

    pub async fn rename(
        &self,
        workspace_id: Id,
        actor_id: Id,
        name: String,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<WorkspaceRecord, WorkspaceError> {
        let mut transaction = self.database.immediate_transaction().await?;
        require_manager(&mut transaction, workspace_id, actor_id, false).await?;
        sqlx::query(
            "UPDATE workspaces SET name = ?, version = version + 1, updated_at = ? \
             WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(&name)
        .bind(now.as_millis())
        .bind(workspace_id.to_string())
        .execute(&mut *transaction)
        .await?;
        audit::record(
            &mut transaction,
            workspace_id,
            Some(actor_id),
            "workspace.updated",
            "workspace",
            Some(workspace_id),
            request_id,
            json!({}),
            now,
        )
        .await?;
        let version = sqlx::query_scalar::<_, i64>("SELECT version FROM workspaces WHERE id = ?")
            .bind(workspace_id.to_string())
            .fetch_one(&mut *transaction)
            .await?;
        transaction.commit().await?;
        Ok(WorkspaceRecord {
            id: workspace_id,
            name,
            role: role_name(
                require_role_from_db(self.database.pool(), workspace_id, actor_id).await?,
            ),
            version: u64::try_from(version).map_err(|_| WorkspaceError::Conflict)?,
            deleted_at: None,
        })
    }

    pub async fn members(
        &self,
        workspace_id: Id,
        actor_id: Id,
        cursor: Option<Id>,
        limit: usize,
    ) -> Result<(Vec<MemberRecord>, Option<Id>), WorkspaceError> {
        self.get(workspace_id, actor_id).await?;
        let rows = sqlx::query(
            "SELECT memberships.id, memberships.user_id, memberships.role, memberships.version, \
             memberships.created_at, users.email, users.display_name FROM memberships \
             JOIN users ON users.id = memberships.user_id WHERE memberships.workspace_id = ? \
             AND (? IS NULL OR memberships.id > ?) ORDER BY memberships.id LIMIT ?",
        )
        .bind(workspace_id.to_string())
        .bind(cursor.map(|id| id.to_string()))
        .bind(cursor.map(|id| id.to_string()))
        .bind(page_size(limit).saturating_add(1) as i64)
        .fetch_all(self.database.pool())
        .await?;
        page(rows, limit, member_from_row)
    }

    pub async fn invitations(
        &self,
        workspace_id: Id,
        actor_id: Id,
        cursor: Option<Id>,
        limit: usize,
        now: TimestampMillis,
    ) -> Result<(Vec<InvitationRecord>, Option<Id>), WorkspaceError> {
        let role = require_role_from_db(self.database.pool(), workspace_id, actor_id).await?;
        if role == WorkspaceRole::Member {
            return Err(WorkspaceError::Forbidden);
        }
        let rows = sqlx::query(
            "SELECT id, workspace_id, email, role, delivery, expires_at, accepted_at, revoked_at, \
             replaced_at, created_at FROM workspace_invitations WHERE workspace_id = ? \
             AND (? IS NULL OR id > ?) ORDER BY id LIMIT ?",
        )
        .bind(workspace_id.to_string())
        .bind(cursor.map(|id| id.to_string()))
        .bind(cursor.map(|id| id.to_string()))
        .bind(page_size(limit).saturating_add(1) as i64)
        .fetch_all(self.database.pool())
        .await?;
        page(rows, limit, |row| invitation_from_row(row, now))
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn invite(
        &self,
        workspace_id: Id,
        actor_id: Id,
        email: String,
        role: WorkspaceRole,
        delivery: InvitationDelivery,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<IssuedInvitation, WorkspaceError> {
        if role == WorkspaceRole::Owner {
            return Err(WorkspaceError::Forbidden);
        }
        let id = Id::new_v7();
        let token = generate_opaque_token();
        let normalized_email = normalize_email(&email);
        let expires_at = TimestampMillis::from_millis(
            now.as_millis().saturating_add(INVITATION_LIFETIME_MILLIS),
        );
        let mut transaction = self.database.immediate_transaction().await?;
        require_manager(&mut transaction, workspace_id, actor_id, false).await?;
        let already_member = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM memberships JOIN users ON users.id = memberships.user_id \
             WHERE memberships.workspace_id = ? AND users.normalized_email = ?",
        )
        .bind(workspace_id.to_string())
        .bind(&normalized_email)
        .fetch_one(&mut *transaction)
        .await?
            != 0;
        if already_member {
            return Err(WorkspaceError::Conflict);
        }
        sqlx::query(
            "UPDATE workspace_invitations SET replaced_at = ? WHERE workspace_id = ? \
             AND normalized_email = ? AND accepted_at IS NULL AND revoked_at IS NULL AND replaced_at IS NULL",
        )
        .bind(now.as_millis())
        .bind(workspace_id.to_string())
        .bind(&normalized_email)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "INSERT INTO workspace_invitations (id, workspace_id, email, normalized_email, role, \
             delivery, token_hash, invited_by, expires_at, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id.to_string())
        .bind(workspace_id.to_string())
        .bind(&email)
        .bind(normalized_email)
        .bind(role_name(role))
        .bind(delivery.as_str())
        .bind(token_hash(&token).to_vec())
        .bind(actor_id.to_string())
        .bind(expires_at.as_millis())
        .bind(now.as_millis())
        .execute(&mut *transaction)
        .await?;
        audit::record(
            &mut transaction,
            workspace_id,
            Some(actor_id),
            "invitation.created",
            "workspace_invitation",
            Some(id),
            request_id,
            json!({"delivery": delivery.as_str(), "role": role_name(role)}),
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(IssuedInvitation {
            invitation: InvitationRecord {
                id,
                workspace_id,
                email,
                role: role_name(role),
                delivery: delivery.as_str().to_owned(),
                status: "pending".to_owned(),
                expires_at,
                created_at: now,
            },
            token,
        })
    }

    pub async fn revoke_invitation(
        &self,
        workspace_id: Id,
        invitation_id: Id,
        actor_id: Id,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), WorkspaceError> {
        let mut transaction = self.database.immediate_transaction().await?;
        require_manager(&mut transaction, workspace_id, actor_id, false).await?;
        let changed = sqlx::query(
            "UPDATE workspace_invitations SET revoked_at = ? WHERE id = ? AND workspace_id = ? \
             AND accepted_at IS NULL AND revoked_at IS NULL AND replaced_at IS NULL",
        )
        .bind(now.as_millis())
        .bind(invitation_id.to_string())
        .bind(workspace_id.to_string())
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if changed == 0 {
            return Err(WorkspaceError::NotFound);
        }
        audit::record(
            &mut transaction,
            workspace_id,
            Some(actor_id),
            "invitation.revoked",
            "workspace_invitation",
            Some(invitation_id),
            request_id,
            json!({}),
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn accept_invitation(
        &self,
        token: &str,
        user_id: Id,
        user_email: &str,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<AcceptanceRecord, WorkspaceError> {
        let hash = token_hash(token);
        let mut transaction = self.database.immediate_transaction().await?;
        let row = sqlx::query(
            "SELECT id, workspace_id, normalized_email, role, delivery, expires_at, accepted_at, \
             revoked_at, replaced_at FROM workspace_invitations WHERE token_hash = ?",
        )
        .bind(hash.to_vec())
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(WorkspaceError::InvalidInvitation)?;
        if row.get::<i64, _>("expires_at") <= now.as_millis()
            || row.get::<Option<i64>, _>("accepted_at").is_some()
            || row.get::<Option<i64>, _>("revoked_at").is_some()
            || row.get::<Option<i64>, _>("replaced_at").is_some()
        {
            return Err(WorkspaceError::InvalidInvitation);
        }
        if row.get::<String, _>("normalized_email") != normalize_email(user_email) {
            return Err(WorkspaceError::EmailMismatch);
        }
        let workspace_id = parse_id(row.get("workspace_id"))?;
        let invitation_id = parse_id(row.get("id"))?;
        let role = row.get::<String, _>("role");
        let delivery = row.get::<String, _>("delivery");
        let existing = sqlx::query_scalar::<_, String>(
            "SELECT id FROM memberships WHERE workspace_id = ? AND user_id = ?",
        )
        .bind(workspace_id.to_string())
        .bind(user_id.to_string())
        .fetch_optional(&mut *transaction)
        .await?;
        let (membership_id, created) = if let Some(id) = existing {
            (parse_id(id)?, false)
        } else {
            let id = Id::new_v7();
            sqlx::query(
                "INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, updated_at) \
                 VALUES (?, ?, ?, ?, 0, ?, ?)",
            )
            .bind(id.to_string())
            .bind(workspace_id.to_string())
            .bind(user_id.to_string())
            .bind(&role)
            .bind(now.as_millis())
            .bind(now.as_millis())
            .execute(&mut *transaction)
            .await?;
            (id, true)
        };
        sqlx::query(
            "UPDATE workspace_invitations SET accepted_at = ? WHERE id = ? AND workspace_id = ?",
        )
        .bind(now.as_millis())
        .bind(invitation_id.to_string())
        .bind(workspace_id.to_string())
        .execute(&mut *transaction)
        .await?;
        let email_verified = delivery == "smtp";
        if email_verified {
            sqlx::query(
                "UPDATE users SET email_verified_at = COALESCE(email_verified_at, ?) WHERE id = ?",
            )
            .bind(now.as_millis())
            .bind(user_id.to_string())
            .execute(&mut *transaction)
            .await?;
        }
        audit::record(
            &mut transaction,
            workspace_id,
            Some(user_id),
            "invitation.accepted",
            "membership",
            Some(membership_id),
            request_id,
            json!({"delivery": delivery}),
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(AcceptanceRecord {
            workspace_id,
            membership_id,
            created,
            email_verified,
        })
    }

    pub async fn change_role(
        &self,
        workspace_id: Id,
        membership_id: Id,
        actor_id: Id,
        role: WorkspaceRole,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), WorkspaceError> {
        if role == WorkspaceRole::Owner {
            return Err(WorkspaceError::TransferRequired);
        }
        let mut transaction = self.database.immediate_transaction().await?;
        let actor_role = require_role(&mut transaction, workspace_id, actor_id, false).await?;
        if actor_role == WorkspaceRole::Member {
            return Err(WorkspaceError::Forbidden);
        }
        let target = target_role(&mut transaction, workspace_id, membership_id).await?;
        if target == WorkspaceRole::Owner {
            return if actor_role == WorkspaceRole::Owner {
                Err(WorkspaceError::TransferRequired)
            } else {
                Err(WorkspaceError::Forbidden)
            };
        }
        sqlx::query(
            "UPDATE memberships SET role = ?, version = version + 1, updated_at = ? \
             WHERE id = ? AND workspace_id = ?",
        )
        .bind(role_name(role))
        .bind(now.as_millis())
        .bind(membership_id.to_string())
        .bind(workspace_id.to_string())
        .execute(&mut *transaction)
        .await?;
        audit::record(
            &mut transaction,
            workspace_id,
            Some(actor_id),
            "membership.role_changed",
            "membership",
            Some(membership_id),
            request_id,
            json!({"role": role_name(role)}),
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn remove_member(
        &self,
        workspace_id: Id,
        membership_id: Id,
        actor_id: Id,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), WorkspaceError> {
        let mut transaction = self.database.immediate_transaction().await?;
        let actor_role = require_role(&mut transaction, workspace_id, actor_id, false).await?;
        let target = target_role(&mut transaction, workspace_id, membership_id).await?;
        if target == WorkspaceRole::Owner {
            return if actor_role == WorkspaceRole::Owner {
                Err(WorkspaceError::TransferRequired)
            } else {
                Err(WorkspaceError::Forbidden)
            };
        }
        if actor_role == WorkspaceRole::Member {
            let target_user = sqlx::query_scalar::<_, String>(
                "SELECT user_id FROM memberships WHERE id = ? AND workspace_id = ?",
            )
            .bind(membership_id.to_string())
            .bind(workspace_id.to_string())
            .fetch_one(&mut *transaction)
            .await?;
            if target_user != actor_id.to_string() {
                return Err(WorkspaceError::Forbidden);
            }
        }
        sqlx::query("DELETE FROM memberships WHERE id = ? AND workspace_id = ?")
            .bind(membership_id.to_string())
            .bind(workspace_id.to_string())
            .execute(&mut *transaction)
            .await?;
        audit::record(
            &mut transaction,
            workspace_id,
            Some(actor_id),
            "membership.removed",
            "membership",
            Some(membership_id),
            request_id,
            json!({}),
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn transfer_ownership(
        &self,
        workspace_id: Id,
        membership_id: Id,
        actor_id: Id,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), WorkspaceError> {
        let mut transaction = self.database.immediate_transaction().await?;
        let actor_membership = sqlx::query(
            "SELECT memberships.id, memberships.role FROM memberships JOIN workspaces ON workspaces.id = memberships.workspace_id \
             WHERE memberships.workspace_id = ? AND memberships.user_id = ? AND workspaces.deleted_at IS NULL",
        )
        .bind(workspace_id.to_string())
        .bind(actor_id.to_string())
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(WorkspaceError::NotFound)?;
        if actor_membership.get::<String, _>("role") != "owner" {
            return Err(WorkspaceError::Forbidden);
        }
        target_role(&mut transaction, workspace_id, membership_id).await?;
        let actor_membership_id = actor_membership.get::<String, _>("id");
        if actor_membership_id == membership_id.to_string() {
            return Ok(());
        }
        sqlx::query(
            "UPDATE memberships SET role = 'admin', version = version + 1, updated_at = ? \
             WHERE id = ? AND workspace_id = ? AND role = 'owner'",
        )
        .bind(now.as_millis())
        .bind(actor_membership_id)
        .bind(workspace_id.to_string())
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "UPDATE memberships SET role = 'owner', version = version + 1, updated_at = ? \
             WHERE id = ? AND workspace_id = ?",
        )
        .bind(now.as_millis())
        .bind(membership_id.to_string())
        .bind(workspace_id.to_string())
        .execute(&mut *transaction)
        .await?;
        audit::record(
            &mut transaction,
            workspace_id,
            Some(actor_id),
            "workspace.ownership_transferred",
            "membership",
            Some(membership_id),
            request_id,
            json!({}),
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn set_deleted(
        &self,
        workspace_id: Id,
        actor_id: Id,
        deleted: bool,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), WorkspaceError> {
        let mut transaction = self.database.immediate_transaction().await?;
        let role = require_role(&mut transaction, workspace_id, actor_id, !deleted).await?;
        if role != WorkspaceRole::Owner {
            return Err(WorkspaceError::Forbidden);
        }
        let changed = if deleted {
            sqlx::query(
                "UPDATE workspaces SET deleted_at = ?, version = version + 1, updated_at = ? \
                 WHERE id = ? AND deleted_at IS NULL",
            )
            .bind(now.as_millis())
            .bind(now.as_millis())
            .bind(workspace_id.to_string())
            .execute(&mut *transaction)
            .await?
            .rows_affected()
        } else {
            sqlx::query(
                "UPDATE workspaces SET deleted_at = NULL, version = version + 1, updated_at = ? \
                 WHERE id = ? AND deleted_at IS NOT NULL",
            )
            .bind(now.as_millis())
            .bind(workspace_id.to_string())
            .execute(&mut *transaction)
            .await?
            .rows_affected()
        };
        if changed == 0 {
            return Err(WorkspaceError::NotFound);
        }
        audit::record(
            &mut transaction,
            workspace_id,
            Some(actor_id),
            if deleted {
                "workspace.deleted"
            } else {
                "workspace.restored"
            },
            "workspace",
            Some(workspace_id),
            request_id,
            json!({}),
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn audit(
        &self,
        workspace_id: Id,
        actor_id: Id,
        cursor: Option<Id>,
        limit: usize,
    ) -> Result<(Vec<AuditEvent>, Option<Id>), WorkspaceError> {
        let role = require_role_from_db(self.database.pool(), workspace_id, actor_id).await?;
        if role == WorkspaceRole::Member {
            return Err(WorkspaceError::Forbidden);
        }
        Ok(audit::list(&self.database, workspace_id, cursor, page_size(limit)).await?)
    }
}

fn page<R, T>(
    rows: Vec<R>,
    requested_limit: usize,
    map: impl Fn(R) -> Result<T, WorkspaceError>,
) -> Result<(Vec<T>, Option<Id>), WorkspaceError>
where
    T: PageItem,
{
    let limit = page_size(requested_limit);
    let has_more = rows.len() > limit;
    let items = rows
        .into_iter()
        .take(limit)
        .map(map)
        .collect::<Result<Vec<_>, _>>()?;
    let next = has_more.then(|| items.last().map(PageItem::id)).flatten();
    Ok((items, next))
}

trait PageItem {
    fn id(&self) -> Id;
}

impl PageItem for MemberRecord {
    fn id(&self) -> Id {
        self.id
    }
}

impl PageItem for InvitationRecord {
    fn id(&self) -> Id {
        self.id
    }
}

fn page_size(limit: usize) -> usize {
    limit.clamp(1, 100)
}

fn workspace_from_row(row: sqlx::sqlite::SqliteRow) -> Result<WorkspaceRecord, WorkspaceError> {
    Ok(WorkspaceRecord {
        id: parse_id(row.get("id"))?,
        name: row.get("name"),
        role: row.get("role"),
        version: u64::try_from(row.get::<i64, _>("version"))
            .map_err(|_| WorkspaceError::Conflict)?,
        deleted_at: row
            .get::<Option<i64>, _>("deleted_at")
            .map(TimestampMillis::from_millis),
    })
}

fn member_from_row(row: sqlx::sqlite::SqliteRow) -> Result<MemberRecord, WorkspaceError> {
    Ok(MemberRecord {
        id: parse_id(row.get("id"))?,
        user_id: parse_id(row.get("user_id"))?,
        email: row.get("email"),
        display_name: row.get("display_name"),
        role: row.get("role"),
        version: u64::try_from(row.get::<i64, _>("version"))
            .map_err(|_| WorkspaceError::Conflict)?,
        created_at: TimestampMillis::from_millis(row.get("created_at")),
    })
}

fn invitation_from_row(
    row: sqlx::sqlite::SqliteRow,
    now: TimestampMillis,
) -> Result<InvitationRecord, WorkspaceError> {
    let expires_at = TimestampMillis::from_millis(row.get("expires_at"));
    let status = if row.get::<Option<i64>, _>("accepted_at").is_some() {
        "accepted"
    } else if row.get::<Option<i64>, _>("revoked_at").is_some() {
        "revoked"
    } else if row.get::<Option<i64>, _>("replaced_at").is_some() {
        "replaced"
    } else if expires_at <= now {
        "expired"
    } else {
        "pending"
    };
    Ok(InvitationRecord {
        id: parse_id(row.get("id"))?,
        workspace_id: parse_id(row.get("workspace_id"))?,
        email: row.get("email"),
        role: row.get("role"),
        delivery: row.get("delivery"),
        status: status.to_owned(),
        expires_at,
        created_at: TimestampMillis::from_millis(row.get("created_at")),
    })
}

async fn require_manager(
    transaction: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    actor_id: Id,
    deleted: bool,
) -> Result<(), WorkspaceError> {
    if require_role(transaction, workspace_id, actor_id, deleted).await? == WorkspaceRole::Member {
        return Err(WorkspaceError::Forbidden);
    }
    Ok(())
}

async fn require_role(
    transaction: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    actor_id: Id,
    deleted: bool,
) -> Result<WorkspaceRole, WorkspaceError> {
    let role = sqlx::query_scalar::<_, String>(
        "SELECT memberships.role FROM memberships JOIN workspaces ON workspaces.id = memberships.workspace_id \
         WHERE memberships.workspace_id = ? AND memberships.user_id = ? \
         AND ((? = 1 AND workspaces.deleted_at IS NOT NULL) OR (? = 0 AND workspaces.deleted_at IS NULL))",
    )
    .bind(workspace_id.to_string())
    .bind(actor_id.to_string())
    .bind(i64::from(deleted))
    .bind(i64::from(deleted))
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(WorkspaceError::NotFound)?;
    parse_role(&role)
}

async fn require_role_from_db(
    pool: &sqlx::SqlitePool,
    workspace_id: Id,
    actor_id: Id,
) -> Result<WorkspaceRole, WorkspaceError> {
    let role = sqlx::query_scalar::<_, String>(
        "SELECT memberships.role FROM memberships JOIN workspaces ON workspaces.id = memberships.workspace_id \
         WHERE memberships.workspace_id = ? AND memberships.user_id = ? AND workspaces.deleted_at IS NULL",
    )
    .bind(workspace_id.to_string())
    .bind(actor_id.to_string())
    .fetch_optional(pool)
    .await?
    .ok_or(WorkspaceError::NotFound)?;
    parse_role(&role)
}

async fn target_role(
    transaction: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    membership_id: Id,
) -> Result<WorkspaceRole, WorkspaceError> {
    let role = sqlx::query_scalar::<_, String>(
        "SELECT role FROM memberships WHERE id = ? AND workspace_id = ?",
    )
    .bind(membership_id.to_string())
    .bind(workspace_id.to_string())
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(WorkspaceError::NotFound)?;
    parse_role(&role)
}

fn parse_role(role: &str) -> Result<WorkspaceRole, WorkspaceError> {
    match role {
        "owner" => Ok(WorkspaceRole::Owner),
        "admin" => Ok(WorkspaceRole::Admin),
        "member" => Ok(WorkspaceRole::Member),
        _ => Err(WorkspaceError::Conflict),
    }
}

fn role_name(role: WorkspaceRole) -> String {
    match role {
        WorkspaceRole::Owner => "owner",
        WorkspaceRole::Admin => "admin",
        WorkspaceRole::Member => "member",
    }
    .to_owned()
}

fn parse_id(value: String) -> Result<Id, WorkspaceError> {
    value.parse().map_err(|_| WorkspaceError::Conflict)
}

fn token_hash(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}
