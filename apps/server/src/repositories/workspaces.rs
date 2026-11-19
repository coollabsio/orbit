use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use orbit_domain::{Actor, MembershipDenied, Permission, Policy, WorkspaceDefaults, WorkspaceRole};
use orbit_platform::{
    AttachmentMutationCoordinator, BLOB_REFERENCE_COUNT, BlobStore, BlobStoreError, Database, Flag,
    Id, IssuedSession, Job, JobError, JobKind, JobKindRegistrationError, JobStore, LocalBlobStore,
    RecurringSchedule, Row, ScheduleError, Scheduler, TimestampMillis, UploadError, UploadService,
    WORKSPACE_STORAGE_USED, Worker, WorkerConfig, WorkerError, generate_opaque_token,
    normalize_email,
};
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::{Any, AnyConnection, Transaction};
use thiserror::Error;
use tokio::sync::OwnedMutexGuard;
use tokio_util::sync::CancellationToken;
use utoipa::ToSchema;

use super::billing::{TrialCapacityError, TrialLimitReached, ensure_trial_capacity};
use super::chat::{
    ChatError, Emitted, MAX_GUEST_CHANNELS, join_invited_channels, join_public_channels,
    require_guest_channels,
};
use super::membership;
use super::page_versions;
use super::tasks::TaskError;
use super::teamspaces::insert_default_teamspace;
use crate::audit::{self, AuditEvent, AuditOutcome};
use crate::billing::TrialLimit;
use crate::live::LiveHub;

const INVITATION_LIFETIME_MILLIS: i64 = 7 * 24 * 60 * 60 * 1_000;
const WORKSPACE_TRASH_RETENTION_MILLIS: i64 = 30 * 24 * 60 * 60 * 1_000;
const SESSION_IDLE_RETENTION_MILLIS: i64 = 30 * 24 * 60 * 60 * 1_000;
const AUDIT_RETENTION_MILLIS: i64 = 365 * 24 * 60 * 60 * 1_000;
/// Items deleted per write transaction of the retention purge when one item takes several statements or
/// cascades: the transaction holds every writer of the cluster.
const PURGE_BATCH: i64 = 200;
/// The same for rows that go with one plain delete each.
const PURGE_ROW_BATCH: i64 = 2_000;

/// The one parameter of a batched retention delete.
#[derive(Clone, Copy)]
enum PurgeKey<'a> {
    Millis(i64),
    Id(&'a str),
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct WorkspaceRecord {
    #[schema(value_type = String)]
    pub id: Id,
    pub name: String,
    pub role: WorkspaceRole,
    /// What the caller's role may do in this workspace.
    pub permissions: Vec<Permission>,
    /// What the caller's custom role grants in single projects, beyond `permissions`.
    pub project_permissions: Vec<ProjectPermissions>,
    pub version: u64,
    #[schema(value_type = Option<String>, format = DateTime)]
    pub deleted_at: Option<TimestampMillis>,
}

/// The project-scoped permissions a member holds in one project.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct ProjectPermissions {
    #[schema(value_type = String)]
    pub project_id: Id,
    pub permissions: Vec<Permission>,
}

fn project_permissions(grants: Vec<(Id, Vec<Permission>)>) -> Vec<ProjectPermissions> {
    grants
        .into_iter()
        .map(|(project_id, permissions)| ProjectPermissions {
            project_id,
            permissions,
        })
        .collect()
}

/// What a profile popover shows of a member beside the member list's fields.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct MemberProfile {
    #[schema(value_type = String)]
    pub user_id: Id,
    /// The job title.
    pub title: Option<String>,
    pub pronouns: Option<String>,
    /// An IANA time zone name.
    pub timezone: Option<String>,
    pub bio: Option<String>,
    pub phone: Option<String>,
    /// The caller's private note about this member; nobody else can read it.
    pub note: Option<String>,
}

/// A custom role, as a member's row names it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, ToSchema)]
pub struct CustomRoleRef {
    #[schema(value_type = String)]
    pub id: Id,
    pub name: String,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct MemberRecord {
    #[schema(value_type = String)]
    pub id: Id,
    #[schema(value_type = String)]
    pub user_id: Id,
    pub email: String,
    pub display_name: String,
    /// The member's profile picture; absent while they have none.
    pub avatar_url: Option<String>,
    pub role: WorkspaceRole,
    /// The custom role that adds permissions to a member's role.
    pub custom_role: Option<CustomRoleRef>,
    /// Whether the caller may change this member's role.
    pub can_change_role: bool,
    /// Whether the caller may remove this member (for the caller's own row: leave).
    pub can_remove: bool,
    /// Whether the caller may hand the workspace over to this member.
    pub can_transfer_ownership: bool,
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
    /// The private channels a guest invitation adds the guest to; empty for other roles.
    pub channels: Vec<String>,
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

/// How many bytes of files a workspace stores, and its quota.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct WorkspaceStorage {
    /// Each stored file once, however often it is attached. A deleted file stops counting about a day after its
    /// last reference goes (trashed tasks, pages and messages keep their files until the trash is emptied).
    pub used_bytes: u64,
    /// `null` is unlimited.
    pub quota_bytes: Option<u64>,
    /// One page of the files that use the most storage, largest first. Empty unless the caller may update the
    /// workspace.
    pub largest_files: Vec<StorageFile>,
    /// How many files the workspace stores; `0` unless the caller may update the workspace.
    pub file_count: u64,
}

/// One stored file (blob) of a workspace.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct StorageFile {
    /// For `DELETE /api/v1/workspaces/{workspace_id}/storage/files/{file_id}`.
    #[schema(value_type = String)]
    pub id: Id,
    pub byte_size: u64,
    /// `null` when the file is only used in private places, or no longer used (it stops counting soon).
    pub name: Option<String>,
    /// Where the file is attached; empty when nothing uses it any more.
    pub uses: Vec<StorageFileUse>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct StorageFileUse {
    pub kind: StorageFileUseKind,
    /// The task title, page title or channel name; `null` for emoji, stickers and private places.
    pub title: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum StorageFileUseKind {
    Task,
    Page,
    Channel,
    Emoji,
    Sticker,
    /// A personal page, a private channel, a direct message or an unsent chat upload. Its names stay hidden.
    Private,
}

/// How many files one page of `WorkspaceStorage::largest_files` lists at most.
pub const MAX_LARGEST_FILES: u32 = 500;

/// Every use of the largest blobs of workspace `$1`, largest blob first. Names in private places are `NULL`.
const LARGEST_FILE_USES: &str = "SELECT b.id AS blob_id, b.byte_size, u.kind, u.file_name, u.title \
     FROM attachment_blobs b LEFT JOIN ( \
       SELECT r.blob_id, 'task' AS kind, r.display_name AS file_name, t.title AS title \
         FROM attachment_references r JOIN tasks t ON t.id = r.task_id WHERE r.workspace_id = $1 \
       UNION ALL SELECT f.blob_id, CASE WHEN p.owner_id IS NULL THEN 'page' ELSE 'private' END, \
         CASE WHEN p.owner_id IS NULL THEN f.file_name END, CASE WHEN p.owner_id IS NULL THEN p.title END \
         FROM page_files f JOIN pages p ON p.id = f.page_id WHERE f.workspace_id = $1 \
       UNION ALL SELECT f.blob_id, CASE WHEN c.kind = 'public' THEN 'channel' ELSE 'private' END, \
         CASE WHEN c.kind = 'public' THEN f.file_name END, CASE WHEN c.kind = 'public' THEN c.name END \
         FROM chat_message_files f LEFT JOIN chat_conversations c ON c.id = f.conversation_id \
         WHERE f.workspace_id = $1 \
       UNION ALL SELECT blob_id, 'emoji', name, NULL FROM custom_emoji WHERE workspace_id = $1 AND blob_id IS NOT NULL \
       UNION ALL SELECT blob_id, 'sticker', name, NULL FROM custom_stickers WHERE workspace_id = $1 AND blob_id IS NOT NULL \
     ) u ON u.blob_id = b.id \
     WHERE b.id IN (SELECT id FROM attachment_blobs WHERE workspace_id = $1 AND deleted_at IS NULL \
       ORDER BY byte_size DESC, id LIMIT $2 OFFSET $3) \
     ORDER BY b.byte_size DESC, b.id, u.kind, u.title, u.file_name";

#[derive(Debug, Error)]
pub enum WorkspaceError {
    #[error("workspace resource was not found")]
    NotFound,
    #[error("workspace action is not permitted")]
    Forbidden,
    #[error("workspace owner must transfer ownership first")]
    TransferRequired,
    #[error("ownership transfer is not available with billing")]
    TransferUnavailable,
    #[error("workspace invitation is invalid or expired")]
    InvalidInvitation,
    #[error("workspace invitation belongs to another email address")]
    EmailMismatch,
    #[error("an existing global account must sign in to accept this invitation")]
    RegistrationRequiresSignIn,
    #[error("workspace operation conflicts with current state")]
    Conflict,
    /// A custom role or its assignment breaks a rule; the text says which.
    #[error("{0}")]
    InvalidRole(&'static str),
    #[error("a role with that name exists")]
    RoleNameTaken,
    /// A guest invitation needs open private channels that the inviter manages; other roles
    /// take none.
    #[error("the invitation's channels are invalid")]
    GuestChannels,
    #[error("stale version; current version is {current_version}")]
    VersionConflict { current_version: u64 },
    #[error("the account already owns {limit} workspaces, the most the instance allows")]
    OwnedWorkspaceLimit { limit: u32 },
    #[error("trial limit reached")]
    TrialLimit(TrialLimitReached),
    #[error("workspace repository is unavailable")]
    Unavailable(#[from] sqlx::Error),
    #[error("workspace attachment storage is unavailable")]
    Storage(#[from] BlobStoreError),
}

impl From<TrialCapacityError> for WorkspaceError {
    fn from(error: TrialCapacityError) -> Self {
        match error {
            TrialCapacityError::Reached(reached) => Self::TrialLimit(reached),
            TrialCapacityError::Database(error) => Self::Unavailable(error),
        }
    }
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
    pub(super) database: Database,
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
             memberships.role, workspace_roles.permissions AS role_permissions \
             FROM workspaces JOIN memberships ON memberships.workspace_id = workspaces.id \
             LEFT JOIN workspace_roles ON workspace_roles.id = memberships.custom_role_id \
             WHERE memberships.user_id = $1 AND workspaces.deleted_at IS NULL ORDER BY workspaces.id",
        )
        .bind(user_id.to_string())
        .fetch_all(self.database.pool())
        .await?;
        let mut workspaces = rows
            .into_iter()
            .map(workspace_from_row)
            .collect::<Result<Vec<_>, _>>()?;
        self.add_project_permissions(user_id, &mut workspaces)
            .await?;
        Ok(workspaces)
    }

    /// Fills in what the user's custom role grants in single projects (trashed ones too, so a
    /// project's manager can restore it).
    async fn add_project_permissions(
        &self,
        user_id: Id,
        workspaces: &mut [WorkspaceRecord],
    ) -> Result<(), WorkspaceError> {
        if workspaces
            .iter()
            .all(|workspace| workspace.role != WorkspaceRole::Member)
        {
            return Ok(());
        }
        let rows: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT memberships.workspace_id, grants.project_id, grants.permission \
             FROM memberships JOIN workspace_role_projects grants ON grants.role_id = memberships.custom_role_id \
             WHERE memberships.user_id = $1 AND memberships.role = 'member' \
             UNION SELECT projects.workspace_id, direct.project_id, direct.permission \
             FROM member_project_grants direct JOIN projects ON projects.id = direct.project_id \
             JOIN memberships ON memberships.workspace_id = projects.workspace_id AND memberships.user_id = direct.user_id \
             AND memberships.role = 'member' WHERE direct.user_id = $2 \
             ORDER BY 2, 3",
        )
        .bind(user_id.to_string())
        .bind(user_id.to_string())
        .fetch_all(self.database.pool())
        .await?;
        for (workspace_id, project_id, permission) in rows {
            let (Ok(workspace_id), Ok(project_id), Some(permission)) = (
                workspace_id.parse::<Id>(),
                project_id.parse::<Id>(),
                Permission::parse(&permission),
            ) else {
                continue;
            };
            let Some(workspace) = workspaces
                .iter_mut()
                .find(|workspace| workspace.id == workspace_id)
            else {
                continue;
            };
            match workspace
                .project_permissions
                .iter_mut()
                .find(|held| held.project_id == project_id)
            {
                Some(held) => held.permissions.push(permission),
                None => workspace.project_permissions.push(ProjectPermissions {
                    project_id,
                    permissions: vec![permission],
                }),
            }
        }
        Ok(())
    }

    pub async fn list_trash(&self, user_id: Id) -> Result<Vec<WorkspaceRecord>, WorkspaceError> {
        let rows = sqlx::query(
            "SELECT workspaces.id, workspaces.name, workspaces.version, workspaces.deleted_at, \
             memberships.role, NULL AS role_permissions \
             FROM workspaces JOIN memberships ON memberships.workspace_id = workspaces.id \
             WHERE memberships.user_id = $1 AND memberships.role = 'owner' AND workspaces.deleted_at IS NOT NULL \
             AND workspaces.deleted_at > $2 \
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
        ensure_may_own_another(&mut transaction, actor_id).await?;
        // The trial is the creator's: a workspace copies the account's trial start, so a new workspace never starts a
        // new trial. The root user's first workspace is exempt from billing (`crate::billing::access`).
        let trial_started_at = super::billing::account_trial_start(&mut transaction, actor_id)
            .await?
            .unwrap_or(now.as_millis());
        sqlx::query(
            "INSERT INTO workspaces (id, name, version, owner_membership_id, created_at, updated_at, \
             billing_exempt, trial_started_at) VALUES ($1, $2, 0, $3, $4, $5, CASE WHEN EXISTS (SELECT 1 FROM \
             installation_state WHERE id = 1 AND root_user_id = $6) AND NOT EXISTS (SELECT 1 FROM \
             workspaces WHERE billing_exempt = 1) THEN 1 ELSE 0 END, $7)",
        )
        .bind(id.to_string())
        .bind(&defaults.workspace.name)
        .bind(defaults.owner.id.to_string())
        .bind(now.as_millis())
        .bind(now.as_millis())
        .bind(actor_id.to_string())
        .bind(trial_started_at)
        .execute(&mut *transaction)
        .await?;
        // The first workspace that is not billing-exempt starts the account's trial.
        sqlx::query(
            "UPDATE users SET trial_started_at = $1 WHERE id = $2 AND trial_started_at IS NULL \
             AND NOT EXISTS (SELECT 1 FROM workspaces WHERE id = $3 AND billing_exempt = 1)",
        )
        .bind(trial_started_at)
        .bind(actor_id.to_string())
        .bind(id.to_string())
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, updated_at) \
             VALUES ($1, $2, $3, 'owner', 0, $4, $5)",
        )
        .bind(defaults.owner.id.to_string())
        .bind(id.to_string())
        .bind(actor_id.to_string())
        .bind(now.as_millis())
        .bind(now.as_millis())
        .execute(&mut *transaction)
        .await?;
        join_public_channels(&mut transaction, id, actor_id, now).await?;
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
            role: WorkspaceRole::Owner,
            permissions: Policy::permissions(WorkspaceRole::Owner),
            project_permissions: Vec::new(),
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
            "SELECT workspaces.id, workspaces.name, workspaces.version, workspaces.deleted_at, memberships.role, \
             workspace_roles.permissions AS role_permissions \
             FROM workspaces JOIN memberships ON memberships.workspace_id = workspaces.id \
             LEFT JOIN workspace_roles ON workspace_roles.id = memberships.custom_role_id \
             WHERE workspaces.id = $1 AND memberships.user_id = $2 AND workspaces.deleted_at IS NULL",
        )
        .bind(workspace_id.to_string())
        .bind(actor_id.to_string())
        .fetch_optional(self.database.pool())
        .await?
        .ok_or(WorkspaceError::NotFound)?;
        let mut workspaces = [workspace_from_row(row)?];
        self.add_project_permissions(actor_id, &mut workspaces)
            .await?;
        let [workspace] = workspaces;
        Ok(workspace)
    }

    /// Lists `file_limit` of the largest files (at most [`MAX_LARGEST_FILES`]), after skipping `file_offset`.
    pub async fn storage(
        &self,
        workspace_id: Id,
        actor_id: Id,
        file_limit: u32,
        file_offset: u32,
    ) -> Result<WorkspaceStorage, WorkspaceError> {
        let workspace = self.get(workspace_id, actor_id).await?;
        if workspace.role == WorkspaceRole::Guest {
            return Err(WorkspaceError::NotFound);
        }
        let row = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT ({WORKSPACE_STORAGE_USED}) AS used, workspace_storage_quota_bytes AS quota, \
             (SELECT COUNT(*) FROM attachment_blobs WHERE workspace_id = $1 AND deleted_at IS NULL) AS files \
             FROM instance_settings WHERE id = 1"
        )))
        .bind(workspace_id.to_string())
        .fetch_one(self.database.pool())
        .await?;
        let admin = workspace.permissions.contains(&Permission::WorkspaceUpdate);
        let largest_files = if admin {
            self.largest_files(workspace_id, file_limit.min(MAX_LARGEST_FILES), file_offset)
                .await?
        } else {
            Vec::new()
        };
        let file_count = if admin {
            u64::try_from(row.get::<i64, _>("files")).unwrap_or(0)
        } else {
            0
        };
        Ok(WorkspaceStorage {
            used_bytes: u64::try_from(row.get::<i64, _>("used")).unwrap_or(0),
            quota_bytes: u64::try_from(row.get::<i64, _>("quota"))
                .ok()
                .filter(|quota| *quota > 0),
            largest_files,
            file_count,
        })
    }

    async fn largest_files(
        &self,
        workspace_id: Id,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<StorageFile>, WorkspaceError> {
        let rows = sqlx::query(LARGEST_FILE_USES)
            .bind(workspace_id.to_string())
            .bind(i64::from(limit))
            .bind(i64::from(offset))
            .fetch_all(self.database.pool())
            .await?;
        let mut files: Vec<(String, StorageFile)> = Vec::new();
        for row in rows {
            let blob_id: String = row.get("blob_id");
            if files.last().is_none_or(|(id, _)| *id != blob_id) {
                let file = StorageFile {
                    id: blob_id.parse().map_err(|_| WorkspaceError::Conflict)?,
                    byte_size: u64::try_from(row.get::<i64, _>("byte_size")).unwrap_or(0),
                    name: None,
                    uses: Vec::new(),
                };
                files.push((blob_id, file));
            }
            let Some((_, file)) = files.last_mut() else {
                continue;
            };
            let kind = match row.get::<Option<String>, _>("kind").as_deref() {
                Some("task") => StorageFileUseKind::Task,
                Some("page") => StorageFileUseKind::Page,
                Some("channel") => StorageFileUseKind::Channel,
                Some("emoji") => StorageFileUseKind::Emoji,
                Some("sticker") => StorageFileUseKind::Sticker,
                Some(_) => StorageFileUseKind::Private,
                None => continue,
            };
            if file.name.is_none() {
                file.name = row.get("file_name");
            }
            let used = StorageFileUse {
                kind,
                title: row.get("title"),
            };
            if !file.uses.contains(&used) {
                file.uses.push(used);
            }
        }
        Ok(files.into_iter().map(|(_, file)| file).collect())
    }

    /// Deletes a stored file of the workspace for an admin (see [`UploadService::delete_blob`]): the tasks, pages
    /// and chat messages that use it keep their reference and show "Deleted file".
    pub async fn delete_storage_file(
        &self,
        uploads: &UploadService,
        workspace_id: Id,
        actor_id: Id,
        file_id: Id,
        request_id: &str,
    ) -> Result<(), WorkspaceError> {
        let workspace = self.get(workspace_id, actor_id).await?;
        if !workspace.permissions.contains(&Permission::WorkspaceUpdate) {
            return Err(WorkspaceError::Forbidden);
        }
        let deleted =
            uploads
                .delete_blob(workspace_id, file_id)
                .await
                .map_err(|error| match error {
                    UploadError::Database(error) => WorkspaceError::Unavailable(error),
                    UploadError::BlobStore(error) => WorkspaceError::Storage(error),
                    _ => WorkspaceError::Conflict,
                })?;
        if !deleted {
            return Err(WorkspaceError::NotFound);
        }
        let now = self.database.database_now().await?;
        let mut transaction = self.database.immediate_transaction().await?;
        audit::record(
            &mut transaction,
            workspace_id,
            Some(actor_id),
            "storage_file.deleted",
            AuditOutcome::Success,
            "storage_file",
            Some(file_id),
            request_id,
            json!({}),
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(())
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
        let actor = require_actor(&mut *transaction, workspace_id, actor_id).await?;
        if !actor.can(Permission::WorkspaceUpdate) {
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
            "SELECT version FROM workspaces WHERE id = $1 AND deleted_at IS NULL",
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
            "UPDATE workspaces SET name = $1, version = version + 1, updated_at = $2 \
             WHERE id = $3 AND deleted_at IS NULL AND version = $4",
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
            role: actor.role,
            permissions: actor.permissions(),
            project_permissions: project_permissions(actor.project_permissions()),
            version,
            deleted_at: None,
        })
    }

    /// The workspace's members. A guest gets itself and the people it shares a conversation
    /// with, without their email address.
    pub async fn members(
        &self,
        workspace_id: Id,
        actor_id: Id,
        cursor: Option<Id>,
        limit: usize,
    ) -> Result<(Vec<MemberRecord>, Option<Id>), WorkspaceError> {
        let actor = require_chat_actor(self.database.pool(), workspace_id, actor_id).await?;
        let rows = sqlx::query(
            "SELECT memberships.id, memberships.user_id, memberships.role, memberships.version, \
             memberships.created_at, users.email, users.display_name, users.suspended_at, \
             user_avatars.updated_at AS avatar_updated_at, \
             workspace_roles.id AS custom_role_id, workspace_roles.name AS custom_role_name \
             FROM memberships \
             JOIN users ON users.id = memberships.user_id \
             LEFT JOIN workspace_roles ON workspace_roles.id = memberships.custom_role_id \
             LEFT JOIN user_avatars ON user_avatars.user_id = memberships.user_id \
             WHERE memberships.workspace_id = $1 \
             AND ($2 IS NULL OR memberships.id > $3) \
             AND ($5 = 0 OR memberships.user_id = $6 OR EXISTS ( \
                 SELECT 1 FROM chat_members own JOIN chat_members theirs \
                 ON theirs.conversation_id = own.conversation_id \
                 WHERE own.workspace_id = $1 AND own.user_id = $6 \
                 AND theirs.user_id = memberships.user_id)) \
             ORDER BY memberships.id LIMIT $4",
        )
        .bind(workspace_id.to_string())
        .bind(cursor.map(|id| id.to_string()))
        .bind(cursor.map(|id| id.to_string()))
        .bind(page_size(limit).saturating_add(1) as i64)
        .bind(i64::from(actor.is_guest()))
        .bind(actor_id.to_string())
        .fetch_all(self.database.pool())
        .await?;
        let transfer_available = !crate::repositories::billing::enabled(&self.database);
        page(rows, limit, |row| {
            member_from_row(row, &actor, transfer_available)
        })
    }

    /// The profile of a member of the workspace, with the caller's private note about them.
    pub async fn profile(
        &self,
        workspace_id: Id,
        actor_id: Id,
        user_id: Id,
    ) -> Result<MemberProfile, WorkspaceError> {
        let mut connection = self.database.pool().acquire().await?;
        let actor = require_chat_actor(&mut *connection, workspace_id, actor_id).await?;
        require_visible(&mut connection, workspace_id, actor, user_id).await?;
        let row = sqlx::query(
            "SELECT users.title, users.pronouns, users.timezone, users.bio, users.phone, \
             user_notes.body AS note \
             FROM memberships JOIN users ON users.id = memberships.user_id \
             LEFT JOIN user_notes ON user_notes.subject_id = users.id AND user_notes.author_id = $1 \
             WHERE memberships.workspace_id = $2 AND memberships.user_id = $3",
        )
        .bind(actor_id.to_string())
        .bind(workspace_id.to_string())
        .bind(user_id.to_string())
        .fetch_optional(&mut *connection)
        .await?
        .ok_or(WorkspaceError::NotFound)?;
        Ok(MemberProfile {
            user_id,
            title: row.get("title"),
            pronouns: row.get("pronouns"),
            timezone: row.get("timezone"),
            bio: row.get("bio"),
            phone: row.get("phone"),
            note: row.get("note"),
        })
    }

    /// Replaces the caller's private note about a member; an empty note removes it. Callers
    /// check the length first.
    pub async fn set_note(
        &self,
        workspace_id: Id,
        actor_id: Id,
        user_id: Id,
        body: &str,
        now: TimestampMillis,
    ) -> Result<(), WorkspaceError> {
        let mut transaction = self.database.immediate_transaction().await?;
        let actor = require_chat_actor(&mut *transaction, workspace_id, actor_id).await?;
        require_chat_actor(&mut *transaction, workspace_id, user_id).await?;
        require_visible(&mut transaction, workspace_id, actor, user_id).await?;
        if body.is_empty() {
            sqlx::query("DELETE FROM user_notes WHERE author_id = $1 AND subject_id = $2")
                .bind(actor_id.to_string())
                .bind(user_id.to_string())
                .execute(&mut *transaction)
                .await?;
        } else {
            sqlx::query(
                "INSERT INTO user_notes (author_id, subject_id, body, updated_at) \
                 VALUES ($1, $2, $3, $4) ON CONFLICT (author_id, subject_id) DO UPDATE SET \
                 body = excluded.body, updated_at = excluded.updated_at",
            )
            .bind(actor_id.to_string())
            .bind(user_id.to_string())
            .bind(body)
            .bind(now.as_millis())
            .execute(&mut *transaction)
            .await?;
        }
        transaction.commit().await?;
        Ok(())
    }

    pub async fn invitations(
        &self,
        workspace_id: Id,
        actor_id: Id,
        cursor: Option<Id>,
        limit: usize,
        now: TimestampMillis,
    ) -> Result<(Vec<InvitationRecord>, Option<Id>), WorkspaceError> {
        let actor = require_actor(self.database.pool(), workspace_id, actor_id).await?;
        if !actor.can(Permission::MembersManage) {
            return Err(WorkspaceError::Forbidden);
        }
        let rows = sqlx::query(
            "SELECT id, workspace_id, email, role, delivery, expires_at, accepted_at, revoked_at, \
             replaced_at, created_at FROM workspace_invitations WHERE workspace_id = $1 \
             AND ($2 IS NULL OR id > $3) ORDER BY id LIMIT $4",
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
    /// Invites `email` with `role`. A guest invitation names the private channels the guest
    /// joins (`conversation_ids`); other roles name none.
    pub async fn invite(
        &self,
        workspace_id: Id,
        actor_id: Id,
        email: String,
        role: WorkspaceRole,
        conversation_ids: &[Id],
        delivery: InvitationDelivery,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<IssuedInvitation, WorkspaceError> {
        let mut conversation_ids = conversation_ids.to_vec();
        conversation_ids.sort_unstable();
        conversation_ids.dedup();
        if (role == WorkspaceRole::Guest) == conversation_ids.is_empty()
            || conversation_ids.len() > MAX_GUEST_CHANNELS
        {
            return Err(WorkspaceError::GuestChannels);
        }
        let id = Id::new_v7();
        let token = generate_opaque_token();
        let normalized_email = normalize_email(&email);
        let expires_at = TimestampMillis::from_millis(
            now.as_millis().saturating_add(INVITATION_LIFETIME_MILLIS),
        );
        let mut transaction = self.database.immediate_transaction().await?;
        let actor = require_actor(&mut *transaction, workspace_id, actor_id).await?;
        if let Err(denied) = actor.invite(role) {
            let reason = match denied {
                MembershipDenied::RoleForbidden => "role_forbidden",
                _ => "owner_role_forbidden",
            };
            audit::record(
                &mut transaction,
                workspace_id,
                Some(actor_id),
                "invitation.create_denied",
                AuditOutcome::Failure,
                "workspace",
                Some(workspace_id),
                request_id,
                json!({"reason": reason}),
                now,
            )
            .await?;
            transaction.commit().await?;
            return Err(WorkspaceError::Forbidden);
        }
        let already_member = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM memberships JOIN users ON users.id = memberships.user_id \
             WHERE memberships.workspace_id = $1 AND users.normalized_email = $2",
        )
        .bind(workspace_id.to_string())
        .bind(&normalized_email)
        .fetch_one(&mut *transaction)
        .await?
            != 0;
        if already_member {
            return Err(WorkspaceError::Conflict);
        }
        if role == WorkspaceRole::Guest {
            require_guest_channels(&mut transaction, workspace_id, actor, &conversation_ids)
                .await
                .map_err(from_chat)?;
        }
        sqlx::query(
            "UPDATE workspace_invitations SET replaced_at = $1 WHERE workspace_id = $2 \
             AND normalized_email = $3 AND accepted_at IS NULL AND revoked_at IS NULL AND replaced_at IS NULL",
        )
        .bind(now.as_millis())
        .bind(workspace_id.to_string())
        .bind(&normalized_email)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "INSERT INTO workspace_invitations (id, workspace_id, email, normalized_email, role, \
             delivery, token_hash, invited_by, expires_at, created_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
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
        for conversation_id in &conversation_ids {
            sqlx::query(
                "INSERT INTO workspace_invitation_conversations (invitation_id, conversation_id) \
                 VALUES ($1, $2)",
            )
            .bind(id.to_string())
            .bind(conversation_id.to_string())
            .execute(&mut *transaction)
            .await?;
        }
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
        match require_permission(
            &mut transaction,
            workspace_id,
            actor_id,
            Permission::MembersManage,
        )
        .await
        {
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
            "UPDATE workspace_invitations SET revoked_at = $1 WHERE id = $2 AND workspace_id = $3 \
             AND accepted_at IS NULL AND revoked_at IS NULL AND replaced_at IS NULL AND expires_at > $4",
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

    /// The live socket's write lock of the invitation's workspace (none for an unknown token),
    /// held from before an acceptance's transaction until its chat events are out, as
    /// `ChatState::publish` does, so the events go out in commit order.
    async fn invitation_write_lock(
        &self,
        hash: &[u8],
    ) -> Result<Option<OwnedMutexGuard<()>>, WorkspaceError> {
        let workspace_id: Option<String> = sqlx::query_scalar(
            "SELECT workspace_id FROM workspace_invitations WHERE token_hash = $1",
        )
        .bind(hash.to_vec())
        .fetch_optional(self.database.pool())
        .await?;
        Ok(match workspace_id {
            Some(id) => Some(LiveHub::of(&self.database).write_lock(parse_id(id)?).await),
            None => None,
        })
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
        let _order = self.invitation_write_lock(&hash).await?;
        let mut transaction = self.database.immediate_transaction().await?;
        let row = sqlx::query(
            "SELECT id, workspace_id, normalized_email, role, delivery, expires_at, accepted_at, \
             revoked_at, replaced_at FROM workspace_invitations WHERE token_hash = $1",
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
            "SELECT id FROM memberships WHERE workspace_id = $1 AND user_id = $2",
        )
        .bind(workspace_id.to_string())
        .bind(user_id.to_string())
        .fetch_optional(&mut *transaction)
        .await?;
        let mut chat_events = Vec::new();
        let (membership_id, created) = if let Some(id) = existing {
            (parse_id(id)?, false)
        } else {
            let id = Id::new_v7();
            sqlx::query(
                "INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, updated_at) \
                 VALUES ($1, $2, $3, $4, 0, $5, $6)",
            )
            .bind(id.to_string())
            .bind(workspace_id.to_string())
            .bind(user_id.to_string())
            .bind(&role)
            .bind(now.as_millis())
            .bind(now.as_millis())
            .execute(&mut *transaction)
            .await?;
            join_public_channels(&mut transaction, workspace_id, user_id, now).await?;
            chat_events =
                join_invited_channels(&mut transaction, workspace_id, invitation_id, user_id, now)
                    .await
                    .map_err(from_chat)?;
            (id, true)
        };
        sqlx::query(
            "UPDATE workspace_invitations SET accepted_at = $1 WHERE id = $2 AND workspace_id = $3",
        )
        .bind(now.as_millis())
        .bind(invitation_id.to_string())
        .bind(workspace_id.to_string())
        .execute(&mut *transaction)
        .await?;
        let email_verified = delivery == "smtp";
        if email_verified {
            sqlx::query(
                "UPDATE users SET email_verified_at = COALESCE(email_verified_at, $1) WHERE id = $2",
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
        publish_chat(&self.database, workspace_id, &chat_events).await;
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
             revoked_at, replaced_at FROM workspace_invitations WHERE token_hash = $1",
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
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM users WHERE normalized_email = $1")
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
             email_verified_at, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
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
             VALUES ($1, $2, $3, $4, 0, $5, $6)",
        )
        .bind(membership_id.to_string())
        .bind(workspace_id.to_string())
        .bind(user_id.to_string())
        .bind(role)
        .bind(now.as_millis())
        .bind(now.as_millis())
        .execute(&mut *transaction)
        .await?;
        join_public_channels(&mut transaction, workspace_id, user_id, now).await?;
        let chat_events =
            join_invited_channels(&mut transaction, workspace_id, invitation_id, user_id, now)
                .await
                .map_err(from_chat)?;
        sqlx::query(
            "UPDATE workspace_invitations SET accepted_at = $1 WHERE id = $2 AND workspace_id = $3",
        )
        .bind(now.as_millis())
        .bind(invitation_id.to_string())
        .bind(workspace_id.to_string())
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "INSERT INTO sessions (id, token_hash, user_id, created_at, last_activity_at, \
             idle_expires_at, absolute_expires_at) VALUES ($1, $2, $3, $4, $5, $6, $7)",
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
        publish_chat(&self.database, workspace_id, &chat_events).await;
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
            "SELECT invitations.id, invitations.email, workspaces.name AS workspace_name \
             FROM workspace_invitations AS invitations \
             JOIN workspaces ON workspaces.id = invitations.workspace_id \
             WHERE invitations.token_hash = $1 AND invitations.expires_at > $2 \
             AND invitations.accepted_at IS NULL AND invitations.revoked_at IS NULL \
             AND invitations.replaced_at IS NULL AND workspaces.deleted_at IS NULL",
        )
        .bind(token_hash(token).to_vec())
        .bind(now.as_millis())
        .fetch_optional(self.database.pool())
        .await?
        .ok_or(WorkspaceError::InvalidInvitation)?;
        let channels = sqlx::query_scalar(
            "SELECT c.name FROM workspace_invitation_conversations i \
             JOIN chat_conversations c ON c.id = i.conversation_id \
             WHERE i.invitation_id = $1 AND c.kind = 'private' AND c.archived_at IS NULL \
             ORDER BY c.name",
        )
        .bind(row.get::<String, _>("id"))
        .fetch_all(self.database.pool())
        .await?;
        Ok(InvitationPreview {
            email: row.get("email"),
            workspace_name: row.get("workspace_name"),
            channels,
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
             FROM workspace_invitations WHERE token_hash = $1",
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
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM users WHERE normalized_email = $1")
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
             VALUES ('invitation.registration_invalid', $1, 1, $2) \
             ON CONFLICT(kind, bucket_started_at) DO UPDATE SET \
             attempt_count = security_probe_summaries.attempt_count + 1, last_attempt_at = excluded.last_attempt_at",
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
        let actor = require_actor(&mut *transaction, workspace_id, actor_id).await?;
        // The actor's own rule is checked against an ordinary member first, so a caller who may
        // not change roles learns nothing about the target, and the denial is audited even for
        // an unknown membership. The real target is then checked after the version.
        let checked = match actor.change_member_role(WorkspaceRole::Member, role) {
            Err(denied) => Err(denied),
            Ok(()) => {
                let target =
                    target_membership(&mut transaction, workspace_id, membership_id).await?;
                if target.version != expected_version {
                    return Err(WorkspaceError::VersionConflict {
                        current_version: target.version,
                    });
                }
                actor.change_member_role(target.role, role).map(|()| target)
            }
        };
        let target = match checked {
            Ok(target) => target,
            Err(denied) => {
                let reason = match denied {
                    MembershipDenied::RoleForbidden => "role_forbidden",
                    MembershipDenied::OwnerProtected => "owner_protected",
                    MembershipDenied::TransferRequired if role == WorkspaceRole::Owner => {
                        "ownership_transfer_required"
                    }
                    MembershipDenied::TransferRequired => "owner_protected",
                    MembershipDenied::GuestDowngrade => "guest_downgrade",
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
                    json!({"reason": reason}),
                    now,
                )
                .await?;
                transaction.commit().await?;
                return Err(denied.into());
            }
        };
        // Guests do not count as members, so a guest who becomes a member takes a trial seat.
        if target.role == WorkspaceRole::Guest && role != WorkspaceRole::Guest {
            ensure_trial_capacity(
                &self.database,
                &mut transaction,
                workspace_id,
                TrialLimit::Members,
                1,
            )
            .await?;
        }
        sqlx::query(
            "UPDATE memberships SET role = $1, \
             custom_role_id = CASE WHEN $1 = role THEN custom_role_id ELSE NULL END, \
             version = version + 1, updated_at = $2 \
             WHERE id = $3 AND workspace_id = $4 AND version = $5",
        )
        .bind(role_name(role))
        .bind(now.as_millis())
        .bind(membership_id.to_string())
        .bind(workspace_id.to_string())
        .bind(expected_version as i64)
        .execute(&mut *transaction)
        .await?;
        // A guest who becomes a member joins #general and every public channel. Every other
        // member is in them already, so for them this changes nothing.
        join_public_channels(&mut transaction, workspace_id, target.user_id, now).await?;
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
        // A guest may leave too; the policy lets it remove nobody else.
        let actor = require_chat_actor(&mut *transaction, workspace_id, actor_id).await?;
        let target = target_membership(&mut transaction, workspace_id, membership_id).await?;
        if target.version != expected_version {
            return Err(WorkspaceError::VersionConflict {
                current_version: target.version,
            });
        }
        if let Err(denied) = actor.remove_member(target.user_id, target.role) {
            let reason = match denied {
                MembershipDenied::RoleForbidden => "role_forbidden",
                _ => "owner_protected",
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
                json!({"reason": reason}),
                now,
            )
            .await?;
            transaction.commit().await?;
            return Err(denied.into());
        }
        // The member's personal mailbox stays for the admins, suspended: the delete below sets
        // its owner to NULL and removes its only member.
        crate::repositories::mail::suspend_owned_mailboxes(
            &mut transaction,
            workspace_id,
            membership_id,
            actor_id,
            request_id,
            now,
        )
        .await?;
        sqlx::query("DELETE FROM memberships WHERE id = $1 AND workspace_id = $2 AND version = $3")
            .bind(membership_id.to_string())
            .bind(workspace_id.to_string())
            .bind(expected_version as i64)
            .execute(&mut *transaction)
            .await?;
        // A removed member's own credentials end here, so they do not work again if the person rejoins.
        // Service-account tokens belong to the workspace and stay.
        sqlx::query(
            "UPDATE api_tokens SET revoked_at = $1 WHERE workspace_id = $2 AND created_by = $3 \
             AND service_account_id IS NULL AND revoked_at IS NULL",
        )
        .bind(now.as_millis())
        .bind(workspace_id.to_string())
        .bind(target.user_id.to_string())
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "UPDATE oauth_grants SET revoked_at = $1 WHERE workspace_id = $2 AND user_id = $3 \
             AND revoked_at IS NULL",
        )
        .bind(now.as_millis())
        .bind(workspace_id.to_string())
        .bind(target.user_id.to_string())
        .execute(&mut *transaction)
        .await?;
        crate::workspace_export::expire_for_user(
            &mut transaction,
            Some(workspace_id),
            target.user_id,
            now,
        )
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
        // Billing belongs to the owner, so Orbit Cloud cannot hand a workspace over yet.
        if crate::repositories::billing::enabled(&self.database) {
            return Err(WorkspaceError::TransferUnavailable);
        }
        let mut transaction = self.database.immediate_transaction().await?;
        let actor = require_actor(&mut *transaction, workspace_id, actor_id).await?;
        if !actor.can(Permission::WorkspaceTransfer) {
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
            "SELECT version FROM workspaces WHERE id = $1 AND deleted_at IS NULL",
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
        let target = target_membership(&mut transaction, workspace_id, membership_id).await?;
        if target.version != membership_version {
            return Err(WorkspaceError::VersionConflict {
                current_version: target.version,
            });
        }
        // Handing the workspace to yourself changes nothing.
        if target.user_id == actor_id {
            return Ok(());
        }
        ensure_may_own_another(&mut transaction, target.user_id).await?;
        sqlx::query(
            "UPDATE workspaces SET owner_membership_id = $1, updated_at = $2 WHERE id = $3 AND version = $4",
        )
            .bind(membership_id.to_string())
            .bind(now.as_millis())
            .bind(workspace_id.to_string())
            .bind(expected_version as i64)
            .execute(&mut *transaction)
            .await?;
        // The owner holds every permission, so a custom role would only linger.
        sqlx::query("UPDATE memberships SET custom_role_id = NULL WHERE id = $1")
            .bind(membership_id.to_string())
            .execute(&mut *transaction)
            .await?;
        sqlx::query(
            "UPDATE workspaces SET version = version + 1, updated_at = $1 WHERE id = $2 AND version = $3",
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
        let actor = membership::actor_in(&mut *transaction, workspace_id, actor_id, !deleted)
            .await?
            .ok_or(WorkspaceError::NotFound)?;
        if !actor.can(Permission::WorkspaceDelete) {
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
            sqlx::query_scalar::<_, i64>("SELECT version FROM workspaces WHERE id = $1")
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
                "SELECT deleted_at FROM workspaces WHERE id = $1 AND deleted_at IS NOT NULL",
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
            let owner_id = sqlx::query_scalar::<_, String>(
                "SELECT user_id FROM memberships WHERE workspace_id = $1 AND role = 'owner'",
            )
            .bind(workspace_id.to_string())
            .fetch_one(&mut *transaction)
            .await?
            .parse()
            .map_err(|_| WorkspaceError::Conflict)?;
            ensure_may_own_another(&mut transaction, owner_id).await?;
        }
        let changed = if deleted {
            sqlx::query(
                "UPDATE workspaces SET deleted_at = $1, version = version + 1, updated_at = $2 \
                 WHERE id = $3 AND deleted_at IS NULL AND version = $4",
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
                "UPDATE workspaces SET deleted_at = NULL, version = version + 1, updated_at = $1 \
                 WHERE id = $2 AND deleted_at IS NOT NULL AND version = $3",
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
        // Restoring does not resume billing: the owner subscribes again.
        if deleted {
            crate::repositories::billing::cancel_on_delete(
                &self.database,
                &mut transaction,
                workspace_id,
                now,
            )
            .await?;
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
        let actor = require_actor(self.database.pool(), workspace_id, actor_id).await?;
        if !actor.can(Permission::AuditView) {
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
        {
            let _mutation = self.attachment_mutations.begin().await;
            let now = self.database.database_now().await?;
            self.purge_retention(now).await?;
        }
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
        let now = self
            .database
            .database_now()
            .await
            .map_err(WorkspaceError::from)?;
        scheduler
            .ensure(&RecurringSchedule::interval(
                JobKind::new("workspace.retention").with_concurrency_limit(1),
                json!({}),
                cadence,
                now,
            ))
            .await?;
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

    /// Runs as many short write transactions, so none holds the write lock of the whole cluster for long.
    /// Everything it deletes is expired, and a restore refuses an expired item, so each transaction leaves a
    /// valid state and a run that stops midway is finished by the next one.
    pub async fn purge_retention(
        &self,
        now: TimestampMillis,
    ) -> Result<MaintenanceSummary, WorkspaceError> {
        let mut summary = MaintenanceSummary {
            workspaces_purged: 0,
            audit_events_purged: 0,
            attachment_references_purged: 0,
            attachment_blobs_purged: 0,
            pending_uploads_purged: 0,
            files_purged: 0,
            occurred_at: now,
        };
        let outcome = self.purge_expired(now, &mut summary).await;
        let purged_any = summary.workspaces_purged
            + summary.audit_events_purged
            + summary.attachment_references_purged
            + summary.attachment_blobs_purged
            + summary.pending_uploads_purged
            != 0;
        match outcome {
            Ok(()) => {
                self.record_maintenance(&summary).await?;
                Ok(summary)
            }
            Err(error) => {
                // What a failed run deleted stays deleted, so its counts are recorded too.
                if purged_any && let Err(error) = self.record_maintenance(&summary).await {
                    tracing::warn!(%error, "the counts of a failed retention run were not recorded");
                }
                Err(error)
            }
        }
    }

    async fn purge_expired(
        &self,
        now: TimestampMillis,
        summary: &mut MaintenanceSummary,
    ) -> Result<(), WorkspaceError> {
        const LIVE_WORKSPACES: &str = "SELECT id FROM workspaces WHERE deleted_at IS NULL";
        let trash_cutoff = now
            .as_millis()
            .saturating_sub(WORKSPACE_TRASH_RETENTION_MILLIS);
        // Chat files that were uploaded and never sent with a message.
        self.delete_in_batches(
            "chat_message_files",
            "message_id IS NULL AND created_at <= $1",
            PurgeKey::Millis(now.as_millis().saturating_sub(24 * 60 * 60 * 1000)),
            PURGE_ROW_BATCH,
        )
        .await?;
        // Sessions not used for 30 days, revoked or not. Their push subscriptions cascade.
        self.delete_in_batches(
            "sessions",
            "last_activity_at <= $1",
            PurgeKey::Millis(
                now.as_millis()
                    .saturating_sub(SESSION_IDLE_RETENTION_MILLIS),
            ),
            PURGE_ROW_BATCH,
        )
        .await?;
        let expired = sqlx::query_scalar::<_, String>(
            "SELECT id FROM workspaces WHERE deleted_at IS NOT NULL AND deleted_at <= $1 \
             ORDER BY deleted_at, id",
        )
        .bind(trash_cutoff)
        .fetch_all(self.database.pool())
        .await?;
        loop {
            let mut transaction = self.database.immediate_transaction().await?;
            let task_ids = sqlx::query_scalar::<_, String>(
                "SELECT tasks.id FROM tasks LEFT JOIN projects ON projects.id = tasks.project_id \
                 JOIN workspaces ON workspaces.id = tasks.workspace_id \
                 WHERE workspaces.deleted_at IS NULL AND (tasks.deleted_at <= $1 OR projects.deleted_at <= $2) \
                 ORDER BY tasks.id LIMIT $3",
            )
            .bind(trash_cutoff)
            .bind(trash_cutoff)
            .bind(PURGE_BATCH)
            .fetch_all(&mut *transaction)
            .await?;
            if task_ids.is_empty() {
                break;
            }
            for task_id in &task_ids {
                let (references, blobs) =
                    super::tasks::purge_task_in_tx(&mut transaction, task_id, now)
                        .await
                        .map_err(|error| match error {
                            TaskError::Unavailable(error) => WorkspaceError::Unavailable(error),
                            _ => WorkspaceError::Conflict,
                        })?;
                summary.attachment_references_purged += references;
                summary.attachment_blobs_purged += blobs;
            }
            transaction.commit().await?;
        }
        // A trashed subtree shares one deleted_at, so it expires together; pages.parent_id is
        // ON DELETE SET NULL, so the delete order does not matter, also across batches.
        self.delete_in_batches(
            "pages",
            &format!("deleted_at <= $1 AND workspace_id IN ({LIVE_WORKSPACES})"),
            PurgeKey::Millis(trash_cutoff),
            PURGE_BATCH,
        )
        .await?;
        // Page history: all of the last 30 days, then one per day up to a year, newest 20 always.
        let mut transaction = self.database.immediate_transaction().await?;
        page_versions::thin_versions(&mut transaction, now).await?;
        transaction.commit().await?;
        loop {
            let mut transaction = self.database.immediate_transaction().await?;
            let project_ids = sqlx::query_scalar::<_, String>(sqlx::AssertSqlSafe(format!(
                "SELECT id FROM projects WHERE deleted_at <= $1 AND workspace_id IN ({LIVE_WORKSPACES}) \
                 ORDER BY id LIMIT $2"
            )))
            .bind(trash_cutoff)
            .bind(PURGE_BATCH)
            .fetch_all(&mut *transaction)
            .await?;
            if project_ids.is_empty() {
                break;
            }
            for project_id in &project_ids {
                // API token grants hold their projects with ON DELETE RESTRICT.
                for sql in [
                    "DELETE FROM api_token_projects WHERE project_id = $1",
                    "UPDATE api_tokens SET project_id = NULL WHERE project_id = $1",
                    "DELETE FROM projects WHERE id = $1",
                ] {
                    sqlx::query(sql)
                        .bind(project_id)
                        .execute(&mut *transaction)
                        .await?;
                }
            }
            transaction.commit().await?;
        }
        for workspace_id in expired {
            let of_workspace = PurgeKey::Id(&workspace_id);
            summary.attachment_references_purged += self
                .delete_in_batches(
                    "attachment_references",
                    "workspace_id = $1",
                    of_workspace,
                    PURGE_ROW_BATCH,
                )
                .await?;
            // Page files, chat files, mail receipts and the files of outgoing mail hold their
            // blobs with ON DELETE RESTRICT, so they go before the blobs. A receipt takes its
            // parsed messages along.
            for (table, batch) in [
                ("page_files", PURGE_ROW_BATCH),
                ("chat_message_files", PURGE_ROW_BATCH),
                ("mail_receipts", PURGE_BATCH),
                ("mail_file_links", PURGE_ROW_BATCH),
                ("mail_draft_files", PURGE_ROW_BATCH),
            ] {
                summary.attachment_references_purged += self
                    .delete_in_batches(table, "workspace_id = $1", of_workspace, batch)
                    .await?;
            }
            // A blob that something still references stays, so the batches go on from the last id.
            let mut after = String::new();
            loop {
                let mut transaction = self.database.immediate_transaction().await?;
                let blob_rows = sqlx::query(
                    "SELECT id, storage_key FROM attachment_blobs \
                     WHERE workspace_id = $1 AND id > $2 ORDER BY id LIMIT $3",
                )
                .bind(&workspace_id)
                .bind(&after)
                .bind(PURGE_BATCH)
                .fetch_all(&mut *transaction)
                .await?;
                let Some(last) = blob_rows.last() else {
                    break;
                };
                after = last.get("id");
                for row in &blob_rows {
                    let blob_id: String = row.get("id");
                    let references: i64 = sqlx::query_scalar(BLOB_REFERENCE_COUNT)
                        .bind(&blob_id)
                        .bind(&blob_id)
                        .fetch_one(&mut *transaction)
                        .await?;
                    if references == 0 {
                        sqlx::query(
                            "INSERT INTO attachment_file_deletions \
                             (id, path_kind, path, created_at) VALUES ($1, 'blob', $2, $3) \
                             ON CONFLICT DO NOTHING",
                        )
                        .bind(Id::new_v7().to_string())
                        .bind(row.get::<String, _>("storage_key"))
                        .bind(now.as_millis())
                        .execute(&mut *transaction)
                        .await?;
                        summary.attachment_blobs_purged +=
                            sqlx::query("DELETE FROM attachment_blobs WHERE id = $1")
                                .bind(blob_id)
                                .execute(&mut *transaction)
                                .await?
                                .rows_affected();
                    }
                }
                transaction.commit().await?;
            }
            loop {
                let mut transaction = self.database.immediate_transaction().await?;
                let pending = sqlx::query(
                    "SELECT id, temporary_path FROM pending_uploads WHERE workspace_id = $1 \
                     ORDER BY id LIMIT $2",
                )
                .bind(&workspace_id)
                .bind(PURGE_BATCH)
                .fetch_all(&mut *transaction)
                .await?;
                if pending.is_empty() {
                    break;
                }
                for row in &pending {
                    sqlx::query(
                        "INSERT INTO attachment_file_deletions \
                         (id, path_kind, path, created_at) VALUES ($1, 'temporary', $2, $3) \
                         ON CONFLICT DO NOTHING",
                    )
                    .bind(Id::new_v7().to_string())
                    .bind(row.get::<String, _>("temporary_path"))
                    .bind(now.as_millis())
                    .execute(&mut *transaction)
                    .await?;
                    summary.pending_uploads_purged +=
                        sqlx::query("DELETE FROM pending_uploads WHERE id = $1")
                            .bind(row.get::<String, _>("id"))
                            .execute(&mut *transaction)
                            .await?
                            .rows_affected();
                }
                transaction.commit().await?;
            }
            // Tasks go before the workspace cascade: tasks.status_id is ON DELETE RESTRICT, and
            // SQLite may cascade into task_statuses first (it was rebuilt after tasks in 0021).
            // Pages before teamspaces (pages.teamspace_id cascades), teamspaces before the
            // workspace, so no cascade order is left to SQLite.
            for table in ["tasks", "pages"] {
                self.delete_in_batches(table, "workspace_id = $1", of_workspace, PURGE_BATCH)
                    .await?;
            }
            let mut transaction = self.database.immediate_transaction().await?;
            sqlx::query("DELETE FROM teamspaces WHERE workspace_id = $1")
                .bind(&workspace_id)
                .execute(&mut *transaction)
                .await?;
            summary.workspaces_purged += sqlx::query("DELETE FROM workspaces WHERE id = $1")
                .bind(&workspace_id)
                .execute(&mut *transaction)
                .await?
                .rows_affected();
            transaction.commit().await?;
        }
        self.delete_in_batches(
            "outbox_events",
            "created_at < $1",
            PurgeKey::Millis(now.as_millis() - 7 * 24 * 60 * 60 * 1000),
            PURGE_ROW_BATCH,
        )
        .await?;
        let audit_cutoff = now.as_millis().saturating_sub(AUDIT_RETENTION_MILLIS);
        summary.audit_events_purged += self
            .delete_in_batches(
                "audit_events",
                "occurred_at < $1",
                PurgeKey::Millis(audit_cutoff),
                PURGE_ROW_BATCH,
            )
            .await?;
        let mut transaction = self.database.immediate_transaction().await?;
        sqlx::query("DELETE FROM security_probe_summaries WHERE last_attempt_at < $1")
            .bind(audit_cutoff)
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        Ok(())
    }

    /// Deletes the rows of `table` that match `predicate` (its one parameter is `key`), `batch` rows per write
    /// transaction; returns how many rows went.
    async fn delete_in_batches(
        &self,
        table: &str,
        predicate: &str,
        key: PurgeKey<'_>,
        batch: i64,
    ) -> Result<u64, WorkspaceError> {
        let sql = format!(
            "DELETE FROM {table} WHERE id IN (SELECT id FROM {table} WHERE {predicate} LIMIT $2)"
        );
        let mut deleted = 0_u64;
        loop {
            let mut transaction = self.database.immediate_transaction().await?;
            let query = sqlx::query(sqlx::AssertSqlSafe(sql.clone()));
            let query = match key {
                PurgeKey::Millis(millis) => query.bind(millis),
                PurgeKey::Id(id) => query.bind(id.to_owned()),
            };
            let rows = query
                .bind(batch)
                .execute(&mut *transaction)
                .await?
                .rows_affected();
            transaction.commit().await?;
            deleted += rows;
            if rows < batch as u64 {
                return Ok(deleted);
            }
        }
    }

    async fn record_maintenance(&self, summary: &MaintenanceSummary) -> Result<(), WorkspaceError> {
        let mut transaction = self.database.immediate_transaction().await?;
        sqlx::query(
            "INSERT INTO maintenance_summaries (id, kind, workspaces_purged, audit_events_purged, \
             attachment_references_purged, attachment_blobs_purged, pending_uploads_purged, files_purged, occurred_at) \
             VALUES ($1, 'workspace.retention', $2, $3, $4, $5, $6, 0, $7)",
        )
        .bind(Id::new_v7().to_string())
        .bind(summary.workspaces_purged as i64)
        .bind(summary.audit_events_purged as i64)
        .bind(summary.attachment_references_purged as i64)
        .bind(summary.attachment_blobs_purged as i64)
        .bind(summary.pending_uploads_purged as i64)
        .bind(summary.occurred_at.as_millis())
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    /// Calls the store with no write transaction open: a release waits on S3, up to a minute per bucket, and
    /// the write lock is one for the whole cluster. The queue row goes first, in a short transaction that also
    /// reads whether the file is in use again; the store is called after the commit.
    ///
    /// An upload of the same bytes can claim the storage key between that commit and the release. In S3 that
    /// is harmless: a release only refreshes the object there. A store without S3 deletes the file, so each row
    /// holds the blob operation lock that uploads also take (a store without S3 has one server).
    async fn purge_attachment_files(&self) -> Result<u64, WorkspaceError> {
        let store = &self.blob_store;
        let rows = sqlx::query(
            "SELECT id, path_kind, path, created_at FROM attachment_file_deletions ORDER BY created_at, id",
        )
        .fetch_all(self.database.pool())
        .await?;
        let mut purged = 0_u64;
        let mut failure = None;
        for row in rows {
            let id: String = row.get("id");
            let path: String = row.get("path");
            let path_kind: String = row.get("path_kind");
            let live_sql = match path_kind.as_str() {
                "blob" => "SELECT COUNT(*) FROM attachment_blobs WHERE storage_key = $1",
                "temporary" => {
                    "SELECT COUNT(*) FROM pending_uploads WHERE temporary_path = $1 \
                     AND state IN ('receiving', 'staged')"
                }
                _ => continue,
            };
            let _operation = self.attachment_mutations.begin_operation().await;
            let mut transaction = self.database.immediate_transaction().await?;
            let live: i64 = sqlx::query_scalar(live_sql)
                .bind(&path)
                .fetch_one(&mut *transaction)
                .await?;
            let removed = sqlx::query("DELETE FROM attachment_file_deletions WHERE id = $1")
                .bind(&id)
                .execute(&mut *transaction)
                .await?
                .rows_affected();
            transaction.commit().await?;
            if removed == 0 {
                continue;
            }
            if live == 0 {
                let released = match path_kind.as_str() {
                    "blob" => store.release(&path).await,
                    _ => store.delete_temporary(std::path::Path::new(&path)).await,
                };
                if let Err(error) = released {
                    // The row goes back, so the next run tries again. If that fails too, the file is untracked
                    // and reconcile deletes it.
                    let created_at: i64 = row.get("created_at");
                    if let Err(error) = self
                        .requeue_file_deletion(&id, &path_kind, &path, created_at)
                        .await
                    {
                        tracing::warn!(%error, path, "a file deletion was not queued again");
                    }
                    failure = Some(error);
                    break;
                }
            }
            purged += removed;
        }
        if purged != 0 {
            sqlx::query(
                "UPDATE maintenance_summaries SET files_purged = files_purged + $1 \
                 WHERE id = (SELECT id FROM maintenance_summaries WHERE kind = 'workspace.retention' \
                 ORDER BY occurred_at DESC, id DESC LIMIT 1)",
            )
            .bind(purged as i64)
            .execute(self.database.pool())
            .await?;
        }
        match failure {
            Some(error) => Err(error.into()),
            None => Ok(purged),
        }
    }

    async fn requeue_file_deletion(
        &self,
        id: &str,
        path_kind: &str,
        path: &str,
        created_at: i64,
    ) -> Result<(), WorkspaceError> {
        let mut transaction = self.database.immediate_transaction().await?;
        sqlx::query(
            "INSERT INTO attachment_file_deletions (id, path_kind, path, created_at) \
             VALUES ($1, $2, $3, $4) ON CONFLICT DO NOTHING",
        )
        .bind(id)
        .bind(path_kind)
        .bind(path)
        .bind(created_at)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(())
    }
}

async fn insert_default_project(
    transaction: &mut Transaction<'_, Any>,
    defaults: &WorkspaceDefaults,
    now: TimestampMillis,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO projects (id, workspace_id, name, project_key, color, version, created_at, updated_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
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
             category, position, version, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
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

/// Refuses to make `user_id` the owner of one more workspace when they already own (outside the trash) as many as
/// `instance_settings.max_owned_workspaces` allows. Instance admins are not limited. Call it in the serialized
/// write transaction that adds the ownership, so concurrent requests on any server see each other's workspaces.
async fn ensure_may_own_another(
    transaction: &mut Transaction<'_, Any>,
    user_id: Id,
) -> Result<(), WorkspaceError> {
    let row = sqlx::query(
        "SELECT max_owned_workspaces, \
         (SELECT COUNT(*) FROM users WHERE id = $1 AND installation_admin = 1 AND suspended_at IS NULL) AS admin, \
         (SELECT COUNT(*) FROM memberships JOIN workspaces ON workspaces.id = memberships.workspace_id \
          WHERE memberships.user_id = $1 AND memberships.role = 'owner' AND workspaces.deleted_at IS NULL) AS owned \
         FROM instance_settings WHERE id = 1",
    )
    .bind(user_id.to_string())
    .fetch_one(&mut **transaction)
    .await?;
    let limit: i64 = row.get("max_owned_workspaces");
    if limit > 0 && row.get::<i64, _>("admin") == 0 && row.get::<i64, _>("owned") >= limit {
        return Err(WorkspaceError::OwnedWorkspaceLimit {
            limit: u32::try_from(limit).unwrap_or(u32::MAX),
        });
    }
    Ok(())
}

fn workspace_from_row(row: sqlx::any::AnyRow) -> Result<WorkspaceRecord, WorkspaceError> {
    let role = parse_role(&row.get::<String, _>("role"))?;
    let permissions = Policy::permissions_with(
        role,
        membership::parse_permissions(row.get::<Option<String>, _>("role_permissions").as_deref()),
    );
    Ok(WorkspaceRecord {
        id: parse_id(row.get("id"))?,
        name: row.get("name"),
        role,
        permissions,
        project_permissions: Vec::new(),
        version: u64::try_from(row.get::<i64, _>("version"))
            .map_err(|_| WorkspaceError::Conflict)?,
        deleted_at: row
            .get::<Option<i64>, _>("deleted_at")
            .map(TimestampMillis::from_millis),
    })
}

fn member_from_row(
    row: sqlx::any::AnyRow,
    actor: &Actor,
    transfer_available: bool,
) -> Result<MemberRecord, WorkspaceError> {
    let user_id = parse_id(row.get("user_id"))?;
    let role = parse_role(&row.get::<String, _>("role"))?;
    Ok(MemberRecord {
        id: parse_id(row.get("id"))?,
        user_id,
        // A guest does not learn the addresses of the people in its channels.
        email: if actor.is_guest() && user_id != actor.user_id {
            String::new()
        } else {
            row.get("email")
        },
        display_name: row.get("display_name"),
        avatar_url: crate::auth_routes::avatar_url(user_id, row.get("avatar_updated_at")),
        role,
        custom_role: custom_role_ref(&row, role)?,
        can_change_role: actor
            .change_member_role(role, WorkspaceRole::Member)
            .is_ok(),
        can_remove: actor.remove_member(user_id, role).is_ok(),
        can_transfer_ownership: transfer_available && actor.can_transfer_ownership_to(user_id),
        version: u64::try_from(row.get::<i64, _>("version"))
            .map_err(|_| WorkspaceError::Conflict)?,
        created_at: TimestampMillis::from_millis(row.get("created_at")),
        suspended_at: row
            .get::<Option<i64>, _>("suspended_at")
            .map(TimestampMillis::from_millis),
    })
}

/// The custom role of a row that selects `custom_role_id` and `custom_role_name`. Only an
/// ordinary member holds one.
fn custom_role_ref(
    row: &sqlx::any::AnyRow,
    role: WorkspaceRole,
) -> Result<Option<CustomRoleRef>, WorkspaceError> {
    let id = row.get::<Option<String>, _>("custom_role_id");
    let name = row.get::<Option<String>, _>("custom_role_name");
    match (id, name) {
        (Some(id), Some(name)) if role == WorkspaceRole::Member => Ok(Some(CustomRoleRef {
            id: parse_id(id)?,
            name,
        })),
        _ => Ok(None),
    }
}

fn invitation_from_row(
    row: sqlx::any::AnyRow,
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

/// The acting member, or `NotFound` for outsiders and trashed workspaces.
pub(super) async fn require_actor<'e, E>(
    executor: E,
    workspace_id: Id,
    actor_id: Id,
) -> Result<Actor, WorkspaceError>
where
    E: sqlx::Executor<'e, Database = Any>,
{
    membership::actor(executor, workspace_id, actor_id)
        .await?
        .ok_or(WorkspaceError::NotFound)
}

/// As `require_actor`, but guests too: for the people directory, which filters for them.
async fn require_chat_actor<'e, E>(
    executor: E,
    workspace_id: Id,
    actor_id: Id,
) -> Result<Actor, WorkspaceError>
where
    E: sqlx::Executor<'e, Database = Any>,
{
    membership::chat_actor(executor, workspace_id, actor_id)
        .await?
        .ok_or(WorkspaceError::NotFound)
}

/// A guest sees itself and the people it shares a conversation with; nobody else exists for it.
async fn require_visible(
    connection: &mut AnyConnection,
    workspace_id: Id,
    actor: Actor,
    user_id: Id,
) -> Result<(), WorkspaceError> {
    if !actor.is_guest() || actor.user_id == user_id {
        return Ok(());
    }
    let Flag(shared) = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM chat_members own JOIN chat_members theirs \
         ON theirs.conversation_id = own.conversation_id \
         WHERE own.workspace_id = $1 AND own.user_id = $2 AND theirs.user_id = $3)",
    )
    .bind(workspace_id.to_string())
    .bind(actor.user_id.to_string())
    .bind(user_id.to_string())
    .fetch_one(&mut *connection)
    .await?;
    if shared {
        Ok(())
    } else {
        Err(WorkspaceError::NotFound)
    }
}

pub(super) async fn require_permission(
    transaction: &mut Transaction<'_, Any>,
    workspace_id: Id,
    actor_id: Id,
    permission: Permission,
) -> Result<(), WorkspaceError> {
    if !require_actor(&mut **transaction, workspace_id, actor_id)
        .await?
        .can(permission)
    {
        return Err(WorkspaceError::Forbidden);
    }
    Ok(())
}

/// Sends the chat events of a committed acceptance to the live sockets. A failure only costs
/// the sockets a resync, as in `ChatState::publish`.
async fn publish_chat(database: &Database, workspace_id: Id, events: &[Emitted]) {
    if events.is_empty() {
        return;
    }
    let events: Vec<_> = events.iter().map(Emitted::live).collect();
    if let Err(error) = LiveHub::of(database).publish(workspace_id, &events).await {
        tracing::warn!(error = %error, "chat events of an accepted invitation");
    }
}

fn from_chat(error: ChatError) -> WorkspaceError {
    match error {
        ChatError::Invalid { .. } | ChatError::NotFound => WorkspaceError::GuestChannels,
        ChatError::Forbidden(_) => WorkspaceError::Forbidden,
        ChatError::Unavailable(error) => WorkspaceError::Unavailable(error),
        _ => WorkspaceError::Conflict,
    }
}

impl From<MembershipDenied> for WorkspaceError {
    fn from(denied: MembershipDenied) -> Self {
        match denied {
            MembershipDenied::TransferRequired => Self::TransferRequired,
            MembershipDenied::RoleForbidden
            | MembershipDenied::OwnerProtected
            | MembershipDenied::GuestDowngrade => Self::Forbidden,
        }
    }
}

pub(super) struct TargetMembership {
    pub(super) user_id: Id,
    pub(super) role: WorkspaceRole,
    pub(super) version: u64,
}

pub(super) async fn target_membership(
    transaction: &mut Transaction<'_, Any>,
    workspace_id: Id,
    membership_id: Id,
) -> Result<TargetMembership, WorkspaceError> {
    let row = sqlx::query(
        "SELECT user_id, role, version FROM memberships WHERE id = $1 AND workspace_id = $2",
    )
    .bind(membership_id.to_string())
    .bind(workspace_id.to_string())
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(WorkspaceError::NotFound)?;
    Ok(TargetMembership {
        user_id: parse_id(row.get("user_id"))?,
        role: parse_role(&row.get::<String, _>("role"))?,
        version: u64::try_from(row.get::<i64, _>("version"))
            .map_err(|_| WorkspaceError::Conflict)?,
    })
}

fn parse_role(role: &str) -> Result<WorkspaceRole, WorkspaceError> {
    WorkspaceRole::parse(role).ok_or(WorkspaceError::Conflict)
}

fn role_name(role: WorkspaceRole) -> String {
    role.as_str().to_owned()
}

pub(super) fn parse_id(value: String) -> Result<Id, WorkspaceError> {
    value.parse().map_err(|_| WorkspaceError::Conflict)
}

fn token_hash(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}
