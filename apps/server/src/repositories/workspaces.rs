use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use orbit_domain::{WorkspaceDefaults, WorkspaceRole};
use orbit_platform::{
    AttachmentMutationCoordinator, BLOB_REFERENCE_COUNT, BlobStore, BlobStoreError, Database, Id,
    IssuedSession, Job, JobError, JobKind, JobKindRegistrationError, JobStore, LocalBlobStore,
    RecurringSchedule, ScheduleError, Scheduler, TimestampMillis, Worker, WorkerConfig,
    WorkerError, generate_opaque_token, normalize_email,
};
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::{Row, Sqlite, Transaction};
use thiserror::Error;
use tokio_util::sync::CancellationToken;
use utoipa::ToSchema;

use super::page_versions;
use super::task_relations;
use super::tasks::TaskError;
use super::teamspaces::insert_default_teamspace;
use crate::audit::{self, AuditEvent, AuditOutcome};

const INVITATION_LIFETIME_MILLIS: i64 = 7 * 24 * 60 * 60 * 1_000;
const WORKSPACE_TRASH_RETENTION_MILLIS: i64 = 30 * 24 * 60 * 60 * 1_000;
const AUDIT_RETENTION_MILLIS: i64 = 365 * 24 * 60 * 60 * 1_000;

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct WorkspaceRecord {
    #[schema(value_type = String)]
    pub id: Id,
    pub name: String,
    pub role: String,
    pub version: u64,
    #[schema(value_type = Option<String>, format = DateTime)]
    pub deleted_at: Option<TimestampMillis>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct MemberRecord {
    #[schema(value_type = String)]
    pub id: Id,
    #[schema(value_type = String)]
    pub user_id: Id,
    pub email: String,
    pub display_name: String,
    pub role: String,
    pub version: u64,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: TimestampMillis,
    /// Set while the user's account is suspended (they cannot sign in or be notified).
    #[schema(value_type = Option<String>, format = DateTime)]
    pub suspended_at: Option<TimestampMillis>,
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, ToSchema)]
pub struct InvitationRecord {
    #[schema(value_type = String)]
    pub id: Id,
    #[schema(value_type = String)]
    pub workspace_id: Id,
    pub email: String,
    pub role: String,
    pub delivery: String,
    pub status: String,
    #[schema(value_type = String, format = DateTime)]
    pub expires_at: TimestampMillis,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: TimestampMillis,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct InvitationPreview {
    pub email: String,
    pub workspace_name: String,
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

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct AcceptanceRecord {
    #[schema(value_type = String)]
    pub workspace_id: Id,
    #[schema(value_type = String)]
    pub membership_id: Id,
    pub created: bool,
    pub email_verified: bool,
}

pub struct RegisteredAcceptance {
    pub acceptance: AcceptanceRecord,
    pub session: IssuedSession,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, ToSchema)]
pub struct MaintenanceSummary {
    pub workspaces_purged: u64,
    pub audit_events_purged: u64,
    pub attachment_references_purged: u64,
    pub attachment_blobs_purged: u64,
    pub pending_uploads_purged: u64,
    pub files_purged: u64,
    #[schema(value_type = String, format = DateTime)]
    pub occurred_at: TimestampMillis,
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
    #[error("an existing global account must sign in to accept this invitation")]
    RegistrationRequiresSignIn,
    #[error("workspace operation conflicts with current state")]
    Conflict,
    #[error("stale version; current version is {current_version}")]
    VersionConflict { current_version: u64 },
    #[error("workspace repository is unavailable")]
    Unavailable(#[from] sqlx::Error),
    #[error("workspace attachment storage is unavailable")]
    Storage(#[from] BlobStoreError),
}

#[derive(Debug, Error)]
pub enum RetentionServiceError {
    #[error(transparent)]
    Workspace(#[from] WorkspaceError),
    #[error(transparent)]
    Schedule(#[from] ScheduleError),
    #[error(transparent)]
    Worker(#[from] WorkerError),
    #[error(transparent)]
    Kind(#[from] JobKindRegistrationError),
    #[error(transparent)]
    Join(#[from] tokio::task::JoinError),
    #[error("retention worker stopped unexpectedly")]
    WorkerStopped,
}

#[derive(Clone)]
pub struct WorkspaceRepository {
    database: Database,
    blob_store: Arc<dyn BlobStore>,
    attachment_mutations: AttachmentMutationCoordinator,
}

impl WorkspaceRepository {
    #[must_use]
    pub fn new(database: Database) -> Self {
        Self {
            database,
            blob_store: Arc::new(LocalBlobStore::new("attachments")),
            attachment_mutations: AttachmentMutationCoordinator::default(),
        }
    }

    #[must_use]
    pub fn with_blob_store(database: Database, blob_store: Arc<dyn BlobStore>) -> Self {
        Self::with_blob_store_and_mutations(
            database,
            blob_store,
            AttachmentMutationCoordinator::default(),
        )
    }

    #[must_use]
    pub fn with_blob_store_and_mutations(
        database: Database,
        blob_store: Arc<dyn BlobStore>,
        attachment_mutations: AttachmentMutationCoordinator,
    ) -> Self {
        Self {
            database,
            blob_store,
            attachment_mutations,
        }
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
             AND workspaces.deleted_at > ? \
             ORDER BY workspaces.id DESC",
        )
        .bind(user_id.to_string())
        .bind(self.database.database_now().await?.as_millis().saturating_sub(WORKSPACE_TRASH_RETENTION_MILLIS))
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
        let defaults = WorkspaceDefaults::new(actor_id, name, "General");
        let id = defaults.workspace.id;
        let mut transaction = self.database.immediate_transaction().await?;
        sqlx::query(
            "INSERT INTO workspaces (id, name, version, owner_membership_id, created_at, updated_at) \
             VALUES (?, ?, 0, ?, ?, ?)",
        )
        .bind(id.to_string())
        .bind(&defaults.workspace.name)
        .bind(defaults.owner.id.to_string())
        .bind(now.as_millis())
        .bind(now.as_millis())
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, updated_at) \
             VALUES (?, ?, ?, 'owner', 0, ?, ?)",
        )
        .bind(defaults.owner.id.to_string())
        .bind(id.to_string())
        .bind(actor_id.to_string())
        .bind(now.as_millis())
        .bind(now.as_millis())
        .execute(&mut *transaction)
        .await?;
        insert_default_project(&mut transaction, &defaults, now).await?;
        insert_default_teamspace(&mut transaction, id, actor_id, now).await?;
        audit::record(
            &mut transaction,
            id,
            Some(actor_id),
            "workspace.created",
            AuditOutcome::Success,
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
            name: defaults.workspace.name,
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
        expected_version: u64,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<WorkspaceRecord, WorkspaceError> {
        let mut transaction = self.database.immediate_transaction().await?;
        let actor_role = require_role(&mut transaction, workspace_id, actor_id, false).await?;
        if actor_role == WorkspaceRole::Member {
            audit::record(
                &mut transaction,
                workspace_id,
                Some(actor_id),
                "workspace.update_denied",
                AuditOutcome::Failure,
                "workspace",
                Some(workspace_id),
                request_id,
                json!({"reason": "role_forbidden"}),
                now,
            )
            .await?;
            transaction.commit().await?;
            return Err(WorkspaceError::Forbidden);
        }
        let current_version = sqlx::query_scalar::<_, i64>(
            "SELECT version FROM workspaces WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(workspace_id.to_string())
        .fetch_one(&mut *transaction)
        .await?;
        let current_version =
            u64::try_from(current_version).map_err(|_| WorkspaceError::Conflict)?;
        if current_version != expected_version {
            return Err(WorkspaceError::VersionConflict { current_version });
        }
        sqlx::query(
            "UPDATE workspaces SET name = ?, version = version + 1, updated_at = ? \
             WHERE id = ? AND deleted_at IS NULL AND version = ?",
        )
        .bind(&name)
        .bind(now.as_millis())
        .bind(workspace_id.to_string())
        .bind(expected_version as i64)
        .execute(&mut *transaction)
        .await?;
        audit::record(
            &mut transaction,
            workspace_id,
            Some(actor_id),
            "workspace.updated",
            AuditOutcome::Success,
            "workspace",
            Some(workspace_id),
            request_id,
            json!({}),
            now,
        )
        .await?;
        let version = current_version + 1;
        transaction.commit().await?;
        Ok(WorkspaceRecord {
            id: workspace_id,
            name,
            role: role_name(actor_role),
            version,
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
             memberships.created_at, users.email, users.display_name, users.suspended_at \
             FROM memberships \
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
        let id = Id::new_v7();
        let token = generate_opaque_token();
        let normalized_email = normalize_email(&email);
        let expires_at = TimestampMillis::from_millis(
            now.as_millis().saturating_add(INVITATION_LIFETIME_MILLIS),
        );
        let mut transaction = self.database.immediate_transaction().await?;
        match require_manager(&mut transaction, workspace_id, actor_id, false).await {
            Ok(_) => {}
            Err(WorkspaceError::Forbidden) => {
                audit::record(
                    &mut transaction,
                    workspace_id,
                    Some(actor_id),
                    "invitation.create_denied",
                    AuditOutcome::Failure,
                    "workspace",
                    Some(workspace_id),
                    request_id,
                    json!({"reason":"role_forbidden"}),
                    now,
                )
                .await?;
                transaction.commit().await?;
                return Err(WorkspaceError::Forbidden);
            }
            Err(error) => return Err(error),
        }
        if role == WorkspaceRole::Owner {
            audit::record(
                &mut transaction,
                workspace_id,
                Some(actor_id),
                "invitation.create_denied",
                AuditOutcome::Failure,
                "workspace",
                Some(workspace_id),
                request_id,
                json!({"reason":"owner_role_forbidden"}),
                now,
            )
            .await?;
            transaction.commit().await?;
            return Err(WorkspaceError::Forbidden);
        }
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
            AuditOutcome::Success,
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
        match require_manager(&mut transaction, workspace_id, actor_id, false).await {
            Ok(_) => {}
            Err(WorkspaceError::Forbidden) => {
                audit::record(
                    &mut transaction,
                    workspace_id,
                    Some(actor_id),
                    "invitation.revoke_denied",
                    AuditOutcome::Failure,
                    "workspace_invitation",
                    Some(invitation_id),
                    request_id,
                    json!({"reason":"role_forbidden"}),
                    now,
                )
                .await?;
                transaction.commit().await?;
                return Err(WorkspaceError::Forbidden);
            }
            Err(error) => return Err(error),
        }
        let changed = sqlx::query(
            "UPDATE workspace_invitations SET revoked_at = ? WHERE id = ? AND workspace_id = ? \
             AND accepted_at IS NULL AND revoked_at IS NULL AND replaced_at IS NULL AND expires_at > ?",
        )
        .bind(now.as_millis())
        .bind(invitation_id.to_string())
        .bind(workspace_id.to_string())
        .bind(now.as_millis())
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
            AuditOutcome::Success,
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
        .await?;
        let Some(row) = row else {
            audit::record_global(
                &mut transaction,
                Some(user_id),
                "invitation.accept_failed",
                AuditOutcome::Failure,
                "workspace_invitation",
                None,
                request_id,
                json!({"reason": "invalid"}),
                now,
            )
            .await?;
            transaction.commit().await?;
            return Err(WorkspaceError::InvalidInvitation);
        };
        let workspace_id = parse_id(row.get("workspace_id"))?;
        let invitation_id = parse_id(row.get("id"))?;
        if row.get::<i64, _>("expires_at") <= now.as_millis()
            || row.get::<Option<i64>, _>("accepted_at").is_some()
            || row.get::<Option<i64>, _>("revoked_at").is_some()
            || row.get::<Option<i64>, _>("replaced_at").is_some()
        {
            audit::record(
                &mut transaction,
                workspace_id,
                Some(user_id),
                "invitation.accept_failed",
                AuditOutcome::Failure,
                "workspace_invitation",
                Some(invitation_id),
                request_id,
                json!({"reason": "inactive"}),
                now,
            )
            .await?;
            transaction.commit().await?;
            return Err(WorkspaceError::InvalidInvitation);
        }
        if row.get::<String, _>("normalized_email") != normalize_email(user_email) {
            audit::record(
                &mut transaction,
                workspace_id,
                Some(user_id),
                "invitation.accept_failed",
                AuditOutcome::Failure,
                "workspace_invitation",
                Some(invitation_id),
                request_id,
                json!({"reason": "email_mismatch"}),
                now,
            )
            .await?;
            transaction.commit().await?;
            return Err(WorkspaceError::EmailMismatch);
        }
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
            AuditOutcome::Success,
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

    #[allow(clippy::too_many_arguments)]
    pub async fn register_invited_account(
        &self,
        token: &str,
        email: String,
        display_name: String,
        password_hash: String,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<RegisteredAcceptance, WorkspaceError> {
        const DAY: i64 = 24 * 60 * 60 * 1_000;
        let hash = token_hash(token);
        let normalized_email = normalize_email(&email);
        let user_id = Id::new_v7();
        let membership_id = Id::new_v7();
        let session_id = Id::new_v7();
        let session_token = generate_opaque_token();
        let idle_expires_at = TimestampMillis::from_millis(now.as_millis() + 30 * DAY);
        let absolute_expires_at = TimestampMillis::from_millis(now.as_millis() + 90 * DAY);
        let mut transaction = self.database.immediate_transaction().await?;
        let row = sqlx::query(
            "SELECT id, workspace_id, normalized_email, role, delivery, expires_at, accepted_at, \
             revoked_at, replaced_at FROM workspace_invitations WHERE token_hash = ?",
        )
        .bind(hash.to_vec())
        .fetch_optional(&mut *transaction)
        .await?;
        let Some(row) = row else {
            audit::record_global(
                &mut transaction,
                None,
                "invitation.registration_failed",
                AuditOutcome::Failure,
                "workspace_invitation",
                None,
                request_id,
                json!({"reason": "invalid"}),
                now,
            )
            .await?;
            transaction.commit().await?;
            return Err(WorkspaceError::InvalidInvitation);
        };
        let invitation_id = parse_id(row.get("id"))?;
        let workspace_id = parse_id(row.get("workspace_id"))?;
        if row.get::<i64, _>("expires_at") <= now.as_millis()
            || row.get::<Option<i64>, _>("accepted_at").is_some()
            || row.get::<Option<i64>, _>("revoked_at").is_some()
            || row.get::<Option<i64>, _>("replaced_at").is_some()
        {
            audit::record(
                &mut transaction,
                workspace_id,
                None,
                "invitation.registration_failed",
                AuditOutcome::Failure,
                "workspace_invitation",
                Some(invitation_id),
                request_id,
                json!({"reason": "inactive"}),
                now,
            )
            .await?;
            transaction.commit().await?;
            return Err(WorkspaceError::InvalidInvitation);
        }
        if row.get::<String, _>("normalized_email") != normalized_email {
            audit::record(
                &mut transaction,
                workspace_id,
                None,
                "invitation.registration_failed",
                AuditOutcome::Failure,
                "workspace_invitation",
                Some(invitation_id),
                request_id,
                json!({"reason": "email_mismatch"}),
                now,
            )
            .await?;
            transaction.commit().await?;
            return Err(WorkspaceError::EmailMismatch);
        }
        let existing =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM users WHERE normalized_email = ?")
                .bind(&normalized_email)
                .fetch_one(&mut *transaction)
                .await?
                != 0;
        if existing {
            return Err(WorkspaceError::RegistrationRequiresSignIn);
        }
        let role = row.get::<String, _>("role");
        let delivery = row.get::<String, _>("delivery");
        sqlx::query(
            "INSERT INTO users (id, email, normalized_email, display_name, password_hash, \
             email_verified_at, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(user_id.to_string())
        .bind(email)
        .bind(normalized_email)
        .bind(display_name)
        .bind(password_hash)
        .bind((delivery == "smtp").then_some(now.as_millis()))
        .bind(now.as_millis())
        .bind(now.as_millis())
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, updated_at) \
             VALUES (?, ?, ?, ?, 0, ?, ?)",
        )
        .bind(membership_id.to_string())
        .bind(workspace_id.to_string())
        .bind(user_id.to_string())
        .bind(role)
        .bind(now.as_millis())
        .bind(now.as_millis())
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "UPDATE workspace_invitations SET accepted_at = ? WHERE id = ? AND workspace_id = ?",
        )
        .bind(now.as_millis())
        .bind(invitation_id.to_string())
        .bind(workspace_id.to_string())
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "INSERT INTO sessions (id, token_hash, user_id, created_at, last_activity_at, \
             idle_expires_at, absolute_expires_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(session_id.to_string())
        .bind(token_hash(&session_token).to_vec())
        .bind(user_id.to_string())
        .bind(now.as_millis())
        .bind(now.as_millis())
        .bind(idle_expires_at.as_millis())
        .bind(absolute_expires_at.as_millis())
        .execute(&mut *transaction)
        .await?;
        audit::record(
            &mut transaction,
            workspace_id,
            Some(user_id),
            "invitation.accepted",
            AuditOutcome::Success,
            "membership",
            Some(membership_id),
            request_id,
            json!({"delivery": delivery, "new_account": true}),
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(RegisteredAcceptance {
            acceptance: AcceptanceRecord {
                workspace_id,
                membership_id,
                created: true,
                email_verified: delivery == "smtp",
            },
            session: IssuedSession {
                id: session_id,
                token: session_token,
                idle_expires_at,
                absolute_expires_at,
            },
        })
    }

    pub async fn preview_invitation(
        &self,
        token: &str,
        now: TimestampMillis,
    ) -> Result<InvitationPreview, WorkspaceError> {
        let row = sqlx::query(
            "SELECT invitations.email, workspaces.name AS workspace_name \
             FROM workspace_invitations AS invitations \
             JOIN workspaces ON workspaces.id = invitations.workspace_id \
             WHERE invitations.token_hash = ? AND invitations.expires_at > ? \
             AND invitations.accepted_at IS NULL AND invitations.revoked_at IS NULL \
             AND invitations.replaced_at IS NULL AND workspaces.deleted_at IS NULL",
        )
        .bind(token_hash(token).to_vec())
        .bind(now.as_millis())
        .fetch_optional(self.database.pool())
        .await?
        .ok_or(WorkspaceError::InvalidInvitation)?;
        Ok(InvitationPreview {
            email: row.get("email"),
            workspace_name: row.get("workspace_name"),
        })
    }

    pub async fn validate_invited_registration(
        &self,
        token: &str,
        email: &str,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), WorkspaceError> {
        let row = sqlx::query(
            "SELECT id, workspace_id, normalized_email, expires_at, accepted_at, revoked_at, replaced_at \
             FROM workspace_invitations WHERE token_hash = ?",
        )
        .bind(token_hash(token).to_vec())
        .fetch_optional(self.database.pool())
        .await?;
        let Some(row) = row else {
            self.summarize_invalid_invitation_probe(now).await?;
            return Err(WorkspaceError::InvalidInvitation);
        };
        let invitation_id = parse_id(row.get("id"))?;
        let workspace_id = parse_id(row.get("workspace_id"))?;
        if row.get::<i64, _>("expires_at") <= now.as_millis()
            || row.get::<Option<i64>, _>("accepted_at").is_some()
            || row.get::<Option<i64>, _>("revoked_at").is_some()
            || row.get::<Option<i64>, _>("replaced_at").is_some()
        {
            self.record_registration_failure(
                workspace_id,
                invitation_id,
                "inactive",
                request_id,
                now,
            )
            .await?;
            return Err(WorkspaceError::InvalidInvitation);
        }
        let normalized_email = normalize_email(email);
        if row.get::<String, _>("normalized_email") != normalized_email {
            self.record_registration_failure(
                workspace_id,
                invitation_id,
                "email_mismatch",
                request_id,
                now,
            )
            .await?;
            return Err(WorkspaceError::EmailMismatch);
        }
        let existing =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM users WHERE normalized_email = ?")
                .bind(normalized_email)
                .fetch_one(self.database.pool())
                .await?;
        if existing != 0 {
            return Err(WorkspaceError::RegistrationRequiresSignIn);
        }
        Ok(())
    }

    async fn record_registration_failure(
        &self,
        workspace_id: Id,
        invitation_id: Id,
        reason: &str,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), WorkspaceError> {
        let mut transaction = self.database.immediate_transaction().await?;
        audit::record(
            &mut transaction,
            workspace_id,
            None,
            "invitation.registration_failed",
            AuditOutcome::Failure,
            "workspace_invitation",
            Some(invitation_id),
            request_id,
            json!({"reason": reason}),
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    async fn summarize_invalid_invitation_probe(
        &self,
        now: TimestampMillis,
    ) -> Result<(), WorkspaceError> {
        const BUCKET_MILLIS: i64 = 5 * 60 * 1_000;
        let bucket = now.as_millis() - now.as_millis().rem_euclid(BUCKET_MILLIS);
        sqlx::query(
            "INSERT INTO security_probe_summaries (kind, bucket_started_at, attempt_count, last_attempt_at) \
             VALUES ('invitation.registration_invalid', ?, 1, ?) \
             ON CONFLICT(kind, bucket_started_at) DO UPDATE SET \
             attempt_count = attempt_count + 1, last_attempt_at = excluded.last_attempt_at",
        )
        .bind(bucket)
        .bind(now.as_millis())
        .execute(self.database.pool())
        .await?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn change_role(
        &self,
        workspace_id: Id,
        membership_id: Id,
        actor_id: Id,
        role: WorkspaceRole,
        expected_version: u64,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), WorkspaceError> {
        let mut transaction = self.database.immediate_transaction().await?;
        let actor_role = require_role(&mut transaction, workspace_id, actor_id, false).await?;
        if role == WorkspaceRole::Owner {
            audit::record(
                &mut transaction,
                workspace_id,
                Some(actor_id),
                "membership.change_denied",
                AuditOutcome::Failure,
                "membership",
                Some(membership_id),
                request_id,
                json!({"reason":"ownership_transfer_required"}),
                now,
            )
            .await?;
            transaction.commit().await?;
            return Err(WorkspaceError::TransferRequired);
        }
        if actor_role == WorkspaceRole::Member {
            audit::record(
                &mut transaction,
                workspace_id,
                Some(actor_id),
                "membership.change_denied",
                AuditOutcome::Failure,
                "membership",
                Some(membership_id),
                request_id,
                json!({"reason": "role_forbidden"}),
                now,
            )
            .await?;
            transaction.commit().await?;
            return Err(WorkspaceError::Forbidden);
        }
        let (target, current_version) =
            target_role_and_version(&mut transaction, workspace_id, membership_id).await?;
        if current_version != expected_version {
            return Err(WorkspaceError::VersionConflict { current_version });
        }
        if target == WorkspaceRole::Owner {
            let error = if actor_role == WorkspaceRole::Owner {
                WorkspaceError::TransferRequired
            } else {
                WorkspaceError::Forbidden
            };
            audit::record(
                &mut transaction,
                workspace_id,
                Some(actor_id),
                "membership.change_denied",
                AuditOutcome::Failure,
                "membership",
                Some(membership_id),
                request_id,
                json!({"reason": "owner_protected"}),
                now,
            )
            .await?;
            transaction.commit().await?;
            return Err(error);
        }
        sqlx::query(
            "UPDATE memberships SET role = ?, version = version + 1, updated_at = ? \
             WHERE id = ? AND workspace_id = ? AND version = ?",
        )
        .bind(role_name(role))
        .bind(now.as_millis())
        .bind(membership_id.to_string())
        .bind(workspace_id.to_string())
        .bind(expected_version as i64)
        .execute(&mut *transaction)
        .await?;
        audit::record(
            &mut transaction,
            workspace_id,
            Some(actor_id),
            "membership.role_changed",
            AuditOutcome::Success,
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
        expected_version: u64,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), WorkspaceError> {
        let result = self
            .remove_member_unchecked(
                workspace_id,
                membership_id,
                actor_id,
                expected_version,
                request_id,
                now,
            )
            .await;
        // Open co-editing sockets re-check their access right away.
        crate::collab::CollabHub::revalidate_database(&self.database, Some(workspace_id));
        result
    }

    #[allow(clippy::too_many_arguments)]
    async fn remove_member_unchecked(
        &self,
        workspace_id: Id,
        membership_id: Id,
        actor_id: Id,
        expected_version: u64,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), WorkspaceError> {
        let mut transaction = self.database.immediate_transaction().await?;
        let actor_role = require_role(&mut transaction, workspace_id, actor_id, false).await?;
        let (target, current_version) =
            target_role_and_version(&mut transaction, workspace_id, membership_id).await?;
        if current_version != expected_version {
            return Err(WorkspaceError::VersionConflict { current_version });
        }
        if target == WorkspaceRole::Owner {
            let error = if actor_role == WorkspaceRole::Owner {
                WorkspaceError::TransferRequired
            } else {
                WorkspaceError::Forbidden
            };
            audit::record(
                &mut transaction,
                workspace_id,
                Some(actor_id),
                "membership.remove_denied",
                AuditOutcome::Failure,
                "membership",
                Some(membership_id),
                request_id,
                json!({"reason": "owner_protected"}),
                now,
            )
            .await?;
            transaction.commit().await?;
            return Err(error);
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
                audit::record(
                    &mut transaction,
                    workspace_id,
                    Some(actor_id),
                    "membership.remove_denied",
                    AuditOutcome::Failure,
                    "membership",
                    Some(membership_id),
                    request_id,
                    json!({"reason": "role_forbidden"}),
                    now,
                )
                .await?;
                transaction.commit().await?;
                return Err(WorkspaceError::Forbidden);
            }
        }
        sqlx::query("DELETE FROM memberships WHERE id = ? AND workspace_id = ? AND version = ?")
            .bind(membership_id.to_string())
            .bind(workspace_id.to_string())
            .bind(expected_version as i64)
            .execute(&mut *transaction)
            .await?;
        audit::record(
            &mut transaction,
            workspace_id,
            Some(actor_id),
            "membership.removed",
            AuditOutcome::Success,
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

    #[allow(clippy::too_many_arguments)]
    pub async fn transfer_ownership(
        &self,
        workspace_id: Id,
        membership_id: Id,
        actor_id: Id,
        expected_version: u64,
        membership_version: u64,
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
            audit::record(
                &mut transaction,
                workspace_id,
                Some(actor_id),
                "workspace.transfer_denied",
                AuditOutcome::Failure,
                "workspace",
                Some(workspace_id),
                request_id,
                json!({"reason": "owner_required"}),
                now,
            )
            .await?;
            transaction.commit().await?;
            return Err(WorkspaceError::Forbidden);
        }
        let workspace_version = sqlx::query_scalar::<_, i64>(
            "SELECT version FROM workspaces WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(workspace_id.to_string())
        .fetch_one(&mut *transaction)
        .await?;
        let workspace_version =
            u64::try_from(workspace_version).map_err(|_| WorkspaceError::Conflict)?;
        if workspace_version != expected_version {
            return Err(WorkspaceError::VersionConflict {
                current_version: workspace_version,
            });
        }
        let (_, target_version) =
            target_role_and_version(&mut transaction, workspace_id, membership_id).await?;
        if target_version != membership_version {
            return Err(WorkspaceError::VersionConflict {
                current_version: target_version,
            });
        }
        let actor_membership_id = actor_membership.get::<String, _>("id");
        if actor_membership_id == membership_id.to_string() {
            return Ok(());
        }
        sqlx::query(
            "UPDATE workspaces SET owner_membership_id = ?, updated_at = ? WHERE id = ? AND version = ?",
        )
            .bind(membership_id.to_string())
            .bind(now.as_millis())
            .bind(workspace_id.to_string())
            .bind(expected_version as i64)
            .execute(&mut *transaction)
            .await?;
        sqlx::query(
            "UPDATE workspaces SET version = version + 1, updated_at = ? WHERE id = ? AND version = ?",
        )
        .bind(now.as_millis())
        .bind(workspace_id.to_string())
        .bind(expected_version as i64)
        .execute(&mut *transaction)
        .await?;
        audit::record(
            &mut transaction,
            workspace_id,
            Some(actor_id),
            "workspace.ownership_transferred",
            AuditOutcome::Success,
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
        expected_version: u64,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), WorkspaceError> {
        let result = self
            .set_deleted_unchecked(
                workspace_id,
                actor_id,
                deleted,
                expected_version,
                request_id,
                now,
            )
            .await;
        // Open co-editing sockets re-check their access right away.
        crate::collab::CollabHub::revalidate_database(&self.database, Some(workspace_id));
        result
    }

    #[allow(clippy::too_many_arguments)]
    async fn set_deleted_unchecked(
        &self,
        workspace_id: Id,
        actor_id: Id,
        deleted: bool,
        expected_version: u64,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), WorkspaceError> {
        let mut transaction = self.database.immediate_transaction().await?;
        let role = require_role(&mut transaction, workspace_id, actor_id, !deleted).await?;
        if role != WorkspaceRole::Owner {
            audit::record(
                &mut transaction,
                workspace_id,
                Some(actor_id),
                if deleted {
                    "workspace.delete_denied"
                } else {
                    "workspace.restore_denied"
                },
                AuditOutcome::Failure,
                "workspace",
                Some(workspace_id),
                request_id,
                json!({"reason":"owner_required"}),
                now,
            )
            .await?;
            transaction.commit().await?;
            return Err(WorkspaceError::Forbidden);
        }
        let current_version =
            sqlx::query_scalar::<_, i64>("SELECT version FROM workspaces WHERE id = ?")
                .bind(workspace_id.to_string())
                .fetch_one(&mut *transaction)
                .await?;
        let current_version =
            u64::try_from(current_version).map_err(|_| WorkspaceError::Conflict)?;
        if current_version != expected_version {
            return Err(WorkspaceError::VersionConflict { current_version });
        }
        if !deleted {
            let deleted_at = sqlx::query_scalar::<_, i64>(
                "SELECT deleted_at FROM workspaces WHERE id = ? AND deleted_at IS NOT NULL",
            )
            .bind(workspace_id.to_string())
            .fetch_one(&mut *transaction)
            .await?;
            if deleted_at
                <= now
                    .as_millis()
                    .saturating_sub(WORKSPACE_TRASH_RETENTION_MILLIS)
            {
                return Err(WorkspaceError::NotFound);
            }
        }
        let changed = if deleted {
            sqlx::query(
                "UPDATE workspaces SET deleted_at = ?, version = version + 1, updated_at = ? \
                 WHERE id = ? AND deleted_at IS NULL AND version = ?",
            )
            .bind(now.as_millis())
            .bind(now.as_millis())
            .bind(workspace_id.to_string())
            .bind(expected_version as i64)
            .execute(&mut *transaction)
            .await?
            .rows_affected()
        } else {
            sqlx::query(
                "UPDATE workspaces SET deleted_at = NULL, version = version + 1, updated_at = ? \
                 WHERE id = ? AND deleted_at IS NOT NULL AND version = ?",
            )
            .bind(now.as_millis())
            .bind(workspace_id.to_string())
            .bind(expected_version as i64)
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
            AuditOutcome::Success,
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

    pub async fn global_audit(
        &self,
        workspace_id: Option<Id>,
        action: Option<&str>,
        cursor: Option<Id>,
        limit: usize,
    ) -> Result<(Vec<AuditEvent>, Option<Id>), WorkspaceError> {
        Ok(audit::list_global(
            &self.database,
            workspace_id,
            action,
            cursor,
            page_size(limit),
        )
        .await?)
    }

    pub async fn enqueue_retention(&self, now: TimestampMillis) -> Result<Id, WorkspaceError> {
        let job = Job::new(
            JobKind::new("workspace.retention").with_concurrency_limit(1),
            json!({}),
            now,
        );
        JobStore::new(self.database.clone())
            .enqueue(&job)
            .await
            .map_err(|error| WorkspaceError::Unavailable(sqlx::Error::Protocol(error.to_string())))
    }

    pub fn retention_worker(
        &self,
        config: WorkerConfig,
    ) -> Result<Worker, JobKindRegistrationError> {
        let repository = self.clone();
        Worker::new(JobStore::new(self.database.clone()), config).with_handler(
            JobKind::new("workspace.retention").with_concurrency_limit(1),
            move |_| {
                let repository = repository.clone();
                async move {
                    repository
                        .run_retention_maintenance()
                        .await
                        .map_err(|_| JobError::Retryable("retention maintenance failed".to_owned()))
                }
            },
        )
    }

    pub(crate) async fn run_retention_maintenance(&self) -> Result<(), WorkspaceError> {
        let _mutation = self.attachment_mutations.begin().await;
        let now = self.database.database_now().await?;
        self.purge_retention(now).await?;
        self.purge_attachment_files().await?;
        Ok(())
    }

    pub async fn run_retention_service(
        &self,
        shutdown: CancellationToken,
        cadence: Duration,
        worker_config: WorkerConfig,
    ) -> Result<(), RetentionServiceError> {
        let store = JobStore::new(self.database.clone());
        let scheduler = Scheduler::new(store);
        let exists = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM schedules WHERE job_kind = 'workspace.retention'",
        )
        .fetch_one(self.database.pool())
        .await
        .map_err(WorkspaceError::from)?;
        if exists == 0 {
            let now = self
                .database
                .database_now()
                .await
                .map_err(WorkspaceError::from)?;
            scheduler
                .upsert(&RecurringSchedule::interval(
                    JobKind::new("workspace.retention").with_concurrency_limit(1),
                    json!({}),
                    cadence,
                    now,
                ))
                .await?;
        }
        let worker = self.retention_worker(worker_config)?;
        let worker_shutdown = shutdown.clone();
        let mut worker_task = tokio::spawn(async move { worker.run(worker_shutdown).await });
        let scheduler_shutdown = shutdown.clone();
        let scheduler_task = async {
            loop {
                if scheduler_shutdown.is_cancelled() {
                    break Ok::<(), RetentionServiceError>(());
                }
                let now = self
                    .database
                    .database_now()
                    .await
                    .map_err(WorkspaceError::from)?;
                scheduler.materialize_due(now).await?;
                tokio::select! {
                    () = scheduler_shutdown.cancelled() => break Ok(()),
                    () = tokio::time::sleep(cadence.min(Duration::from_secs(60))) => {}
                }
            }
        };
        tokio::pin!(scheduler_task);

        tokio::select! {
            worker_result = &mut worker_task => {
                let shutdown_was_requested = shutdown.is_cancelled();
                shutdown.cancel();
                worker_result??;
                if shutdown_was_requested {
                    Ok(())
                } else {
                    Err(RetentionServiceError::WorkerStopped)
                }
            }
            scheduler_result = &mut scheduler_task => {
                shutdown.cancel();
                let worker_result = worker_task.await;
                scheduler_result?;
                worker_result??;
                Ok(())
            }
        }
    }

    pub async fn run_production_retention_service(
        &self,
        shutdown: CancellationToken,
    ) -> Result<(), RetentionServiceError> {
        self.run_retention_service(
            shutdown,
            Duration::from_secs(24 * 60 * 60),
            WorkerConfig::default(),
        )
        .await
    }

    pub async fn purge_retention(
        &self,
        now: TimestampMillis,
    ) -> Result<MaintenanceSummary, WorkspaceError> {
        let mut transaction = self.database.immediate_transaction().await?;
        let expired = sqlx::query_scalar::<_, String>(
            "SELECT id FROM workspaces WHERE deleted_at IS NOT NULL AND deleted_at <= ? \
             ORDER BY deleted_at, id",
        )
        .bind(
            now.as_millis()
                .saturating_sub(WORKSPACE_TRASH_RETENTION_MILLIS),
        )
        .fetch_all(&mut *transaction)
        .await?;
        let mut workspaces_purged = 0_u64;
        let mut attachment_references_purged = 0_u64;
        let mut attachment_blobs_purged = 0_u64;
        let mut pending_uploads_purged = 0_u64;
        let task_ids = sqlx::query_scalar::<_, String>(
            "SELECT tasks.id FROM tasks LEFT JOIN projects ON projects.id = tasks.project_id \
             JOIN workspaces ON workspaces.id = tasks.workspace_id \
             WHERE workspaces.deleted_at IS NULL AND (tasks.deleted_at <= ? OR projects.deleted_at <= ?) \
             ORDER BY tasks.id",
        )
        .bind(
            now.as_millis()
                .saturating_sub(WORKSPACE_TRASH_RETENTION_MILLIS),
        )
        .bind(
            now.as_millis()
                .saturating_sub(WORKSPACE_TRASH_RETENTION_MILLIS),
        )
        .fetch_all(&mut *transaction)
        .await?;
        for task_id in task_ids {
            let blobs = sqlx::query(
                "SELECT DISTINCT attachment_blobs.id, attachment_blobs.storage_key \
                 FROM attachment_references JOIN attachment_blobs ON attachment_blobs.id = attachment_references.blob_id \
                 WHERE attachment_references.task_id = ?",
            )
            .bind(&task_id)
            .fetch_all(&mut *transaction)
            .await?;
            attachment_references_purged +=
                sqlx::query("DELETE FROM attachment_references WHERE task_id = ?")
                    .bind(&task_id)
                    .execute(&mut *transaction)
                    .await?
                    .rows_affected();
            task_relations::release_duplicates_in_tx(&mut transaction, &task_id, now)
                .await
                .map_err(|error| match error {
                    TaskError::Unavailable(error) => WorkspaceError::Unavailable(error),
                    _ => WorkspaceError::Conflict,
                })?;
            sqlx::query("DELETE FROM tasks WHERE id = ?")
                .bind(&task_id)
                .execute(&mut *transaction)
                .await?;
            for blob in blobs {
                let blob_id: String = blob.get("id");
                let references: i64 = sqlx::query_scalar(BLOB_REFERENCE_COUNT)
                    .bind(&blob_id)
                    .bind(&blob_id)
                    .fetch_one(&mut *transaction)
                    .await?;
                if references == 0 {
                    sqlx::query(
                        "INSERT OR IGNORE INTO attachment_file_deletions \
                         (id, path_kind, path, created_at) VALUES (?, 'blob', ?, ?)",
                    )
                    .bind(Id::new_v7().to_string())
                    .bind(blob.get::<String, _>("storage_key"))
                    .bind(now.as_millis())
                    .execute(&mut *transaction)
                    .await?;
                    attachment_blobs_purged +=
                        sqlx::query("DELETE FROM attachment_blobs WHERE id = ?")
                            .bind(blob_id)
                            .execute(&mut *transaction)
                            .await?
                            .rows_affected();
                }
            }
        }
        // A trashed subtree shares one deleted_at, so it expires together; pages.parent_id is
        // ON DELETE SET NULL, so the delete order inside the batch does not matter.
        sqlx::query(
            "DELETE FROM pages WHERE deleted_at <= ? AND workspace_id IN \
             (SELECT id FROM workspaces WHERE deleted_at IS NULL)",
        )
        .bind(
            now.as_millis()
                .saturating_sub(WORKSPACE_TRASH_RETENTION_MILLIS),
        )
        .execute(&mut *transaction)
        .await?;
        // Page history: all of the last 30 days, then one per day up to a year, newest 20 always.
        page_versions::thin_versions(&mut transaction, now).await?;
        sqlx::query(
            "DELETE FROM projects WHERE deleted_at <= ? AND workspace_id IN \
             (SELECT id FROM workspaces WHERE deleted_at IS NULL)",
        )
        .bind(
            now.as_millis()
                .saturating_sub(WORKSPACE_TRASH_RETENTION_MILLIS),
        )
        .execute(&mut *transaction)
        .await?;
        for workspace_id in expired {
            let blob_rows =
                sqlx::query("SELECT id, storage_key FROM attachment_blobs WHERE workspace_id = ?")
                    .bind(&workspace_id)
                    .fetch_all(&mut *transaction)
                    .await?;
            let pending_paths = sqlx::query_scalar::<_, String>(
                "SELECT temporary_path FROM pending_uploads WHERE workspace_id = ?",
            )
            .bind(&workspace_id)
            .fetch_all(&mut *transaction)
            .await?;
            attachment_references_purged +=
                sqlx::query("DELETE FROM attachment_references WHERE workspace_id = ?")
                    .bind(&workspace_id)
                    .execute(&mut *transaction)
                    .await?
                    .rows_affected();
            // Page files hold their blobs with ON DELETE RESTRICT, so they go before the blobs.
            attachment_references_purged +=
                sqlx::query("DELETE FROM page_files WHERE workspace_id = ?")
                    .bind(&workspace_id)
                    .execute(&mut *transaction)
                    .await?
                    .rows_affected();
            for row in blob_rows {
                let blob_id: String = row.get("id");
                let references: i64 = sqlx::query_scalar(BLOB_REFERENCE_COUNT)
                    .bind(&blob_id)
                    .bind(&blob_id)
                    .fetch_one(&mut *transaction)
                    .await?;
                if references == 0 {
                    sqlx::query(
                        "INSERT OR IGNORE INTO attachment_file_deletions \
                         (id, path_kind, path, created_at) VALUES (?, 'blob', ?, ?)",
                    )
                    .bind(Id::new_v7().to_string())
                    .bind(row.get::<String, _>("storage_key"))
                    .bind(now.as_millis())
                    .execute(&mut *transaction)
                    .await?;
                    attachment_blobs_purged +=
                        sqlx::query("DELETE FROM attachment_blobs WHERE id = ?")
                            .bind(blob_id)
                            .execute(&mut *transaction)
                            .await?
                            .rows_affected();
                }
            }
            for path in pending_paths {
                sqlx::query(
                    "INSERT OR IGNORE INTO attachment_file_deletions \
                     (id, path_kind, path, created_at) VALUES (?, 'temporary', ?, ?)",
                )
                .bind(Id::new_v7().to_string())
                .bind(path)
                .bind(now.as_millis())
                .execute(&mut *transaction)
                .await?;
            }
            pending_uploads_purged +=
                sqlx::query("DELETE FROM pending_uploads WHERE workspace_id = ?")
                    .bind(&workspace_id)
                    .execute(&mut *transaction)
                    .await?
                    .rows_affected();
            // Tasks go before the workspace cascade: tasks.status_id is ON DELETE RESTRICT, and
            // SQLite may cascade into task_statuses first (it was rebuilt after tasks in 0021).
            sqlx::query("DELETE FROM tasks WHERE workspace_id = ?")
                .bind(&workspace_id)
                .execute(&mut *transaction)
                .await?;
            // Pages before teamspaces (pages.teamspace_id cascades), teamspaces before the
            // workspace, so no cascade order is left to SQLite.
            sqlx::query("DELETE FROM pages WHERE workspace_id = ?")
                .bind(&workspace_id)
                .execute(&mut *transaction)
                .await?;
            sqlx::query("DELETE FROM teamspaces WHERE workspace_id = ?")
                .bind(&workspace_id)
                .execute(&mut *transaction)
                .await?;
            workspaces_purged += sqlx::query("DELETE FROM workspaces WHERE id = ?")
                .bind(workspace_id)
                .execute(&mut *transaction)
                .await?
                .rows_affected();
        }
        sqlx::query("DELETE FROM outbox_events WHERE created_at < ?")
            .bind(now.as_millis() - 7 * 24 * 60 * 60 * 1000)
            .execute(&mut *transaction)
            .await?;
        let audit_events_purged = sqlx::query("DELETE FROM audit_events WHERE occurred_at < ?")
            .bind(now.as_millis().saturating_sub(AUDIT_RETENTION_MILLIS))
            .execute(&mut *transaction)
            .await?
            .rows_affected();
        sqlx::query("DELETE FROM security_probe_summaries WHERE last_attempt_at < ?")
            .bind(now.as_millis().saturating_sub(AUDIT_RETENTION_MILLIS))
            .execute(&mut *transaction)
            .await?;
        sqlx::query(
            "INSERT INTO maintenance_summaries (id, kind, workspaces_purged, audit_events_purged, \
             attachment_references_purged, attachment_blobs_purged, pending_uploads_purged, files_purged, occurred_at) \
             VALUES (?, 'workspace.retention', ?, ?, ?, ?, ?, 0, ?)",
        )
        .bind(Id::new_v7().to_string())
        .bind(workspaces_purged as i64)
        .bind(audit_events_purged as i64)
        .bind(attachment_references_purged as i64)
        .bind(attachment_blobs_purged as i64)
        .bind(pending_uploads_purged as i64)
        .bind(now.as_millis())
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(MaintenanceSummary {
            workspaces_purged,
            audit_events_purged,
            attachment_references_purged,
            attachment_blobs_purged,
            pending_uploads_purged,
            files_purged: 0,
            occurred_at: now,
        })
    }

    async fn purge_attachment_files(&self) -> Result<u64, WorkspaceError> {
        let store = &self.blob_store;
        let rows = sqlx::query(
            "SELECT id, path_kind, path FROM attachment_file_deletions ORDER BY created_at, id",
        )
        .fetch_all(self.database.pool())
        .await?;
        let mut purged = 0_u64;
        for row in rows {
            let id: String = row.get("id");
            let path: String = row.get("path");
            let mut transaction = self.database.immediate_transaction().await?;
            match row.get::<String, _>("path_kind").as_str() {
                "blob" => {
                    let live: i64 = sqlx::query_scalar(
                        "SELECT COUNT(*) FROM attachment_blobs WHERE storage_key = ?",
                    )
                    .bind(&path)
                    .fetch_one(&mut *transaction)
                    .await?;
                    if live == 0 {
                        store.delete(&path).await?;
                    }
                }
                "temporary" => {
                    let live: i64 = sqlx::query_scalar(
                        "SELECT COUNT(*) FROM pending_uploads WHERE temporary_path = ? \
                         AND state IN ('receiving', 'staged')",
                    )
                    .bind(&path)
                    .fetch_one(&mut *transaction)
                    .await?;
                    if live == 0 {
                        store.delete_temporary(std::path::Path::new(&path)).await?;
                    }
                }
                _ => continue,
            }
            purged += sqlx::query("DELETE FROM attachment_file_deletions WHERE id = ?")
                .bind(id)
                .execute(&mut *transaction)
                .await?
                .rows_affected();
            transaction.commit().await?;
        }
        if purged != 0 {
            sqlx::query(
                "UPDATE maintenance_summaries SET files_purged = files_purged + ? \
                 WHERE id = (SELECT id FROM maintenance_summaries WHERE kind = 'workspace.retention' \
                 ORDER BY occurred_at DESC, id DESC LIMIT 1)",
            )
            .bind(purged as i64)
            .execute(self.database.pool())
            .await?;
        }
        Ok(purged)
    }
}

async fn insert_default_project(
    transaction: &mut Transaction<'_, Sqlite>,
    defaults: &WorkspaceDefaults,
    now: TimestampMillis,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO projects (id, workspace_id, name, project_key, color, version, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(defaults.project.id.to_string())
    .bind(defaults.workspace.id.to_string())
    .bind(&defaults.project.name)
    .bind(&defaults.project.key)
    .bind(&defaults.project.color)
    .bind(defaults.project.version as i64)
    .bind(now.as_millis())
    .bind(now.as_millis())
    .execute(&mut **transaction)
    .await?;
    for status in &defaults.statuses {
        sqlx::query(
            "INSERT INTO task_statuses (id, workspace_id, project_id, name, description, color, \
             category, position, version, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(status.id.to_string())
        .bind(status.workspace_id.to_string())
        .bind(status.project_id.to_string())
        .bind(&status.name)
        .bind(&status.description)
        .bind(&status.color)
        .bind(status.category.as_str())
        .bind(status.position)
        .bind(status.version as i64)
        .bind(now.as_millis())
        .bind(now.as_millis())
        .execute(&mut **transaction)
        .await?;
    }
    Ok(())
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
        suspended_at: row
            .get::<Option<i64>, _>("suspended_at")
            .map(TimestampMillis::from_millis),
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

pub(super) async fn require_role(
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

async fn target_role_and_version(
    transaction: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    membership_id: Id,
) -> Result<(WorkspaceRole, u64), WorkspaceError> {
    let row =
        sqlx::query("SELECT role, version FROM memberships WHERE id = ? AND workspace_id = ?")
            .bind(membership_id.to_string())
            .bind(workspace_id.to_string())
            .fetch_optional(&mut **transaction)
            .await?
            .ok_or(WorkspaceError::NotFound)?;
    let role = parse_role(&row.get::<String, _>("role"))?;
    let version =
        u64::try_from(row.get::<i64, _>("version")).map_err(|_| WorkspaceError::Conflict)?;
    Ok((role, version))
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
