use orbit_platform::{
    Database, Id, NewAttachmentReference, StagedUpload, TimestampMillis, UploadError, UploadService,
};
use thiserror::Error;

use crate::audit::{self, AuditOutcome};

use super::identity::AuthenticatedSession;
use super::tasks::CommentRecord;

#[derive(Clone)]
pub struct AttachmentRepository {
    database: Database,
    uploads: UploadService,
}

#[derive(Clone, Debug)]
pub struct CreatedAttachment {
    pub id: Id,
    pub workspace_id: Id,
    pub task_id: Id,
    pub comment_id: Option<Id>,
    pub owner_id: Id,
    pub display_name: String,
    pub media_type: String,
    pub byte_size: u64,
    pub created_at: TimestampMillis,
}

#[derive(Debug, Error)]
pub enum AttachmentRepositoryError {
    #[error("attachment target is unavailable")]
    NotFound,
    #[error("attachment repository is unavailable")]
    Database(#[from] sqlx::Error),
    #[error(transparent)]
    Upload(#[from] UploadError),
}

impl AttachmentRepository {
    #[must_use]
    pub fn new(database: Database, uploads: UploadService) -> Self {
        Self { database, uploads }
    }

    pub async fn finalize(
        &self,
        session: &AuthenticatedSession,
        workspace_id: Id,
        task_id: Id,
        comment_id: Option<Id>,
        upload: &StagedUpload,
    ) -> Result<CreatedAttachment, AttachmentRepositoryError> {
        let (attachment, _) = self
            .finalize_inner(
                session,
                workspace_id,
                task_id,
                comment_id,
                false,
                "",
                upload,
            )
            .await?;
        Ok(attachment)
    }

    pub async fn finalize_attachment_comment(
        &self,
        session: &AuthenticatedSession,
        workspace_id: Id,
        task_id: Id,
        request_id: &str,
        upload: &StagedUpload,
    ) -> Result<(CreatedAttachment, CommentRecord), AttachmentRepositoryError> {
        let (attachment, comment) = self
            .finalize_inner(
                session,
                workspace_id,
                task_id,
                None,
                true,
                request_id,
                upload,
            )
            .await?;
        Ok((attachment, comment.expect("comment creation requested")))
    }

    #[allow(clippy::too_many_arguments)]
    async fn finalize_inner(
        &self,
        session: &AuthenticatedSession,
        workspace_id: Id,
        task_id: Id,
        comment_id: Option<Id>,
        create_comment: bool,
        request_id: &str,
        upload: &StagedUpload,
    ) -> Result<(CreatedAttachment, Option<CommentRecord>), AttachmentRepositoryError> {
        if upload.workspace_id != workspace_id || upload.owner_id != session.user.id {
            return Err(AttachmentRepositoryError::NotFound);
        }
        let finalization = self.uploads.begin_finalization().await;
        let mut transaction = self.database.immediate_transaction().await?;
        let now_millis: i64 = sqlx::query_scalar(
            "SELECT CAST(strftime('%s', 'now') AS INTEGER) * 1000 \
             + CAST(substr(strftime('%f', 'now'), 4, 3) AS INTEGER)",
        )
        .fetch_one(&mut *transaction)
        .await?;
        let now = TimestampMillis::from_millis(now_millis);
        let access: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sessions JOIN users ON users.id = sessions.user_id \
             JOIN memberships ON memberships.user_id = users.id AND memberships.workspace_id = ? \
             JOIN tasks ON tasks.workspace_id = memberships.workspace_id AND tasks.id = ? \
             JOIN projects ON projects.id = tasks.project_id \
             JOIN workspaces ON workspaces.id = tasks.workspace_id \
             WHERE sessions.id = ? AND sessions.user_id = ? AND sessions.revoked_at IS NULL \
             AND sessions.idle_expires_at > ? AND sessions.absolute_expires_at > ? \
             AND users.suspended_at IS NULL AND tasks.deleted_at IS NULL \
             AND projects.deleted_at IS NULL AND workspaces.deleted_at IS NULL",
        )
        .bind(workspace_id.to_string())
        .bind(task_id.to_string())
        .bind(session.id.to_string())
        .bind(session.user.id.to_string())
        .bind(now_millis)
        .bind(now_millis)
        .fetch_one(&mut *transaction)
        .await?;
        if access != 1 {
            return Err(AttachmentRepositoryError::NotFound);
        }
        if let Some(comment_id) = comment_id {
            let exists: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM task_comments WHERE id = ? AND workspace_id = ? AND task_id = ?",
            )
            .bind(comment_id.to_string())
            .bind(workspace_id.to_string())
            .bind(task_id.to_string())
            .fetch_one(&mut *transaction)
            .await?;
            if exists != 1 {
                return Err(AttachmentRepositoryError::NotFound);
            }
        }

        let comment = if create_comment {
            let id = Id::new_v7();
            sqlx::query(
                "INSERT INTO task_comments (id, workspace_id, task_id, author_id, parent_id, body, \
                 version, created_at, updated_at) VALUES (?, ?, ?, ?, NULL, '', 0, ?, ?)",
            )
            .bind(id.to_string())
            .bind(workspace_id.to_string())
            .bind(task_id.to_string())
            .bind(session.user.id.to_string())
            .bind(now_millis)
            .bind(now_millis)
            .execute(&mut *transaction)
            .await?;
            audit::record(
                &mut transaction,
                workspace_id,
                Some(session.user.id),
                "comment.created",
                AuditOutcome::Success,
                "comment",
                Some(id),
                request_id,
                serde_json::json!({}),
                now,
            )
            .await?;
            Some(CommentRecord {
                id,
                workspace_id,
                task_id,
                author_id: session.user.id,
                parent_id: None,
                body: String::new(),
                version: 0,
                created_at: now,
                updated_at: now,
            })
        } else {
            None
        };
        let target_comment_id = comment.as_ref().map(|comment| comment.id).or(comment_id);
        let reference = NewAttachmentReference::new(task_id, target_comment_id);
        let finalized = finalization
            .finalize_in_transaction(&mut transaction, upload, reference, now)
            .await?;
        transaction.commit().await?;

        Ok((
            CreatedAttachment {
                id: finalized.reference_id,
                workspace_id,
                task_id,
                comment_id: target_comment_id,
                owner_id: session.user.id,
                display_name: upload.display_name.clone(),
                media_type: upload.detected_media_type.clone(),
                byte_size: upload.size_bytes,
                created_at: finalized.created_at,
            },
            comment,
        ))
    }
}
