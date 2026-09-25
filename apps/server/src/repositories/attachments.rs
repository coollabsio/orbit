use orbit_platform::{
    AuthorizedAttachment, BlobDownload, Database, Id, NewAttachmentReference, StagedUpload,
    TimestampMillis, UploadError, UploadService,
};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use thiserror::Error;

use crate::audit::{self, AuditOutcome};

use super::identity::AuthenticatedSession;
use super::tasks::CommentRecord;

const DAY_MILLIS: i64 = 24 * 60 * 60 * 1_000;

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

#[derive(Clone, Debug)]
pub struct AttachmentList {
    pub items: Vec<CreatedAttachment>,
    pub next_cursor: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct AttachmentCursor {
    version: u8,
    scope: String,
    created_at: i64,
    id: String,
}

#[derive(Debug, Error)]
pub enum AttachmentRepositoryError {
    #[error("attachment target is unavailable")]
    NotFound,
    #[error("attachment cursor is invalid")]
    InvalidCursor,
    #[error("attachment repository contains an invalid record")]
    InvalidRecord,
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

    pub async fn authorize_task(
        &self,
        session: &AuthenticatedSession,
        workspace_id: Id,
        task_id: Id,
    ) -> Result<(), AttachmentRepositoryError> {
        let now = self.database.database_now().await?;
        let access = task_access(
            self.database.pool(),
            session,
            workspace_id,
            task_id,
            now.as_millis(),
        )
        .await?;
        if access == 1 {
            Ok(())
        } else {
            Err(AttachmentRepositoryError::NotFound)
        }
    }

    pub async fn require_comment(
        &self,
        session: &AuthenticatedSession,
        workspace_id: Id,
        task_id: Id,
        comment_id: Id,
    ) -> Result<(), AttachmentRepositoryError> {
        self.authorize_task(session, workspace_id, task_id).await?;
        let exists: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM task_comments WHERE id = ? AND workspace_id = ? AND task_id = ?",
        )
        .bind(comment_id.to_string())
        .bind(workspace_id.to_string())
        .bind(task_id.to_string())
        .fetch_one(self.database.pool())
        .await?;
        if exists == 1 {
            Ok(())
        } else {
            Err(AttachmentRepositoryError::NotFound)
        }
    }

    pub async fn list(
        &self,
        session: &AuthenticatedSession,
        workspace_id: Id,
        task_id: Id,
        comment_id: Option<Id>,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<AttachmentList, AttachmentRepositoryError> {
        let scope = attachment_scope(workspace_id, task_id, comment_id);
        let after = cursor
            .map(|encoded| decode_cursor(encoded, &scope))
            .transpose()?;
        let limit = limit.clamp(1, 100);
        let fetch_limit =
            i64::try_from(limit + 1).map_err(|_| AttachmentRepositoryError::InvalidRecord)?;
        let comment = comment_id.map(|id| id.to_string());
        let now = self.database.database_now().await?;
        let mut transaction = self.database.immediate_transaction().await?;
        if task_access(
            &mut *transaction,
            session,
            workspace_id,
            task_id,
            now.as_millis(),
        )
        .await?
            != 1
        {
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
        let rows = if let Some((created_at, id)) = after {
            sqlx::query(
                "SELECT id, workspace_id, task_id, comment_id, owner_id, display_name, media_type, \
                 byte_size, created_at FROM attachment_references WHERE workspace_id = ? AND task_id = ? \
                 AND comment_id IS ? AND (created_at > ? OR (created_at = ? AND id > ?)) \
                 ORDER BY created_at, id LIMIT ?",
            )
            .bind(workspace_id.to_string())
            .bind(task_id.to_string())
            .bind(&comment)
            .bind(created_at)
            .bind(created_at)
            .bind(id)
            .bind(fetch_limit)
            .fetch_all(&mut *transaction)
            .await?
        } else {
            sqlx::query(
                "SELECT id, workspace_id, task_id, comment_id, owner_id, display_name, media_type, \
                 byte_size, created_at FROM attachment_references WHERE workspace_id = ? AND task_id = ? \
                 AND comment_id IS ? ORDER BY created_at, id LIMIT ?",
            )
            .bind(workspace_id.to_string())
            .bind(task_id.to_string())
            .bind(&comment)
            .bind(fetch_limit)
            .fetch_all(&mut *transaction)
            .await?
        };
        transaction.commit().await?;
        let mut items = rows
            .into_iter()
            .map(attachment_from_row)
            .collect::<Result<Vec<_>, _>>()?;
        let has_more = items.len() > limit;
        items.truncate(limit);
        let next_cursor = if has_more {
            items
                .last()
                .map(|record| {
                    encode_cursor(&AttachmentCursor {
                        version: 1,
                        scope,
                        created_at: record.created_at.as_millis(),
                        id: record.id.to_string(),
                    })
                })
                .transpose()?
        } else {
            None
        };
        Ok(AttachmentList { items, next_cursor })
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn delete(
        &self,
        session: &AuthenticatedSession,
        workspace_id: Id,
        task_id: Id,
        comment_id: Option<Id>,
        attachment_id: Id,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), AttachmentRepositoryError> {
        let mut transaction = self.database.immediate_transaction().await?;
        if task_access(
            &mut *transaction,
            session,
            workspace_id,
            task_id,
            now.as_millis(),
        )
        .await?
            != 1
        {
            return Err(AttachmentRepositoryError::NotFound);
        }
        let row = sqlx::query(
            "SELECT blob_id FROM attachment_references WHERE id = ? AND workspace_id = ? \
             AND task_id = ? AND comment_id IS ?",
        )
        .bind(attachment_id.to_string())
        .bind(workspace_id.to_string())
        .bind(task_id.to_string())
        .bind(comment_id.map(|id| id.to_string()))
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(AttachmentRepositoryError::NotFound)?;
        let blob_id: String = row.try_get("blob_id")?;
        sqlx::query("DELETE FROM attachment_references WHERE id = ?")
            .bind(attachment_id.to_string())
            .execute(&mut *transaction)
            .await?;
        sqlx::query(
            "UPDATE attachment_blobs SET quarantine_until = MAX(quarantine_until, ?) \
             WHERE id = ? AND NOT EXISTS (SELECT 1 FROM attachment_references WHERE blob_id = ?) \
             AND NOT EXISTS (SELECT 1 FROM page_files WHERE blob_id = ?)",
        )
        .bind(now.as_millis().saturating_add(DAY_MILLIS))
        .bind(&blob_id)
        .bind(&blob_id)
        .bind(&blob_id)
        .execute(&mut *transaction)
        .await?;
        audit::record(
            &mut transaction,
            workspace_id,
            Some(session.user.id),
            "attachment.deleted",
            AuditOutcome::Success,
            "attachment",
            Some(attachment_id),
            request_id,
            serde_json::json!({"task_id": task_id, "comment_id": comment_id}),
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn download(
        &self,
        session: &AuthenticatedSession,
        workspace_id: Id,
        task_id: Id,
        comment_id: Option<Id>,
        attachment_id: Id,
    ) -> Result<BlobDownload, AttachmentRepositoryError> {
        let repository = self.clone();
        let session = session.clone();
        self.uploads
            .download(move || async move {
                repository
                    .authorized_attachment(
                        &session,
                        workspace_id,
                        task_id,
                        comment_id,
                        attachment_id,
                    )
                    .await
                    .map_err(|error| match error {
                        AttachmentRepositoryError::NotFound => UploadError::Unauthorized,
                        AttachmentRepositoryError::Database(error) => UploadError::Database(error),
                        AttachmentRepositoryError::Upload(error) => error,
                        AttachmentRepositoryError::InvalidCursor
                        | AttachmentRepositoryError::InvalidRecord => UploadError::Unauthorized,
                    })
            })
            .await
            .map_err(AttachmentRepositoryError::Upload)
    }

    async fn authorized_attachment(
        &self,
        session: &AuthenticatedSession,
        workspace_id: Id,
        task_id: Id,
        comment_id: Option<Id>,
        attachment_id: Id,
    ) -> Result<AuthorizedAttachment, AttachmentRepositoryError> {
        let now = self.database.database_now().await?;
        let row = sqlx::query(
            "SELECT attachment_blobs.storage_key, attachment_references.media_type, \
             attachment_references.display_name FROM attachment_references JOIN attachment_blobs \
             ON attachment_blobs.id = attachment_references.blob_id \
             AND attachment_blobs.workspace_id = attachment_references.workspace_id \
             JOIN tasks ON tasks.id = attachment_references.task_id \
             AND tasks.workspace_id = attachment_references.workspace_id \
             JOIN projects ON projects.id = tasks.project_id \
             JOIN workspaces ON workspaces.id = tasks.workspace_id \
             JOIN memberships ON memberships.workspace_id = tasks.workspace_id \
             AND memberships.user_id = ? JOIN users ON users.id = memberships.user_id \
             JOIN sessions ON sessions.id = ? AND sessions.user_id = users.id \
             WHERE attachment_references.id = ? AND attachment_references.workspace_id = ? \
             AND attachment_references.task_id = ? AND attachment_references.comment_id IS ? \
             AND sessions.revoked_at IS NULL AND sessions.idle_expires_at > ? \
             AND sessions.absolute_expires_at > ? AND tasks.deleted_at IS NULL \
             AND projects.deleted_at IS NULL AND workspaces.deleted_at IS NULL \
             AND users.suspended_at IS NULL",
        )
        .bind(session.user.id.to_string())
        .bind(session.id.to_string())
        .bind(attachment_id.to_string())
        .bind(workspace_id.to_string())
        .bind(task_id.to_string())
        .bind(comment_id.map(|id| id.to_string()))
        .bind(now.as_millis())
        .bind(now.as_millis())
        .fetch_optional(self.database.pool())
        .await?
        .ok_or(AttachmentRepositoryError::NotFound)?;
        Ok(AuthorizedAttachment {
            storage_key: row.try_get("storage_key")?,
            media_type: row.try_get("media_type")?,
            display_name: row.try_get("display_name")?,
        })
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
        let access = task_access(
            &mut *transaction,
            session,
            workspace_id,
            task_id,
            now_millis,
        )
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

async fn task_access<'e, E>(
    executor: E,
    session: &AuthenticatedSession,
    workspace_id: Id,
    task_id: Id,
    now_millis: i64,
) -> Result<i64, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    sqlx::query_scalar(
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
    .fetch_one(executor)
    .await
}

fn attachment_scope(workspace_id: Id, task_id: Id, comment_id: Option<Id>) -> String {
    format!(
        "{workspace_id}:{task_id}:{}",
        comment_id.map_or_else(|| "task".to_owned(), |id| id.to_string())
    )
}

fn decode_cursor(
    encoded: &str,
    expected_scope: &str,
) -> Result<(i64, String), AttachmentRepositoryError> {
    if !encoded.len().is_multiple_of(2) || encoded.len() > 8_192 {
        return Err(AttachmentRepositoryError::InvalidCursor);
    }
    let bytes = (0..encoded.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&encoded[index..index + 2], 16).ok())
        .collect::<Option<Vec<_>>>()
        .ok_or(AttachmentRepositoryError::InvalidCursor)?;
    let cursor: AttachmentCursor =
        serde_json::from_slice(&bytes).map_err(|_| AttachmentRepositoryError::InvalidCursor)?;
    if cursor.version != 1 || cursor.scope != expected_scope || cursor.id.parse::<Id>().is_err() {
        return Err(AttachmentRepositoryError::InvalidCursor);
    }
    Ok((cursor.created_at, cursor.id))
}

fn encode_cursor(cursor: &AttachmentCursor) -> Result<String, AttachmentRepositoryError> {
    let bytes = serde_json::to_vec(cursor).map_err(|_| AttachmentRepositoryError::InvalidRecord)?;
    Ok(bytes
        .into_iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn attachment_from_row(
    row: sqlx::sqlite::SqliteRow,
) -> Result<CreatedAttachment, AttachmentRepositoryError> {
    Ok(CreatedAttachment {
        id: parse_db_id(row.try_get("id")?)?,
        workspace_id: parse_db_id(row.try_get("workspace_id")?)?,
        task_id: parse_db_id(row.try_get("task_id")?)?,
        comment_id: row
            .try_get::<Option<String>, _>("comment_id")?
            .map(parse_db_id)
            .transpose()?,
        owner_id: parse_db_id(row.try_get("owner_id")?)?,
        display_name: row.try_get("display_name")?,
        media_type: row.try_get("media_type")?,
        byte_size: u64::try_from(row.try_get::<i64, _>("byte_size")?)
            .map_err(|_| AttachmentRepositoryError::InvalidRecord)?,
        created_at: TimestampMillis::from_millis(row.try_get("created_at")?),
    })
}

fn parse_db_id(value: String) -> Result<Id, AttachmentRepositoryError> {
    value
        .parse()
        .map_err(|_| AttachmentRepositoryError::InvalidRecord)
}
