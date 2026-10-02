//! Files of chat messages. The bytes go through the shared `UploadService` (size limits, content
//! sniffing, per-workspace dedup, quarantine); a `chat_message_files` row is a message's
//! reference to a blob. A file is uploaded first, and then it is its uploader's alone; the send
//! of the message attaches it. From then on it is readable by everyone who can read the
//! conversation.

use orbit_platform::{AuthorizedAttachment, BlobDownload, Id, StagedUpload, UploadError};
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqliteConnection};

use super::{
    ChatError, ChatFileRecord, ChatRepository, MESSAGE_SELECT, MessageRecord, hydrate, id_list,
    load_access, load_actor, parse_id,
};

/// Files of one message.
pub(super) const MAX_MESSAGE_FILES: usize = 10;
const LIST_LIMIT: i64 = 100;
const MAX_DIMENSION: u32 = 100_000;

const FILE_COLUMNS: &str =
    "id, workspace_id, message_id, file_name, mime_type, size_bytes, width, height";

fn file_from_row(row: &SqliteRow) -> Result<ChatFileRecord, ChatError> {
    let id = parse_id(row.get("id"))?;
    let workspace_id = parse_id(row.get("workspace_id"))?;
    Ok(ChatFileRecord {
        id,
        url: format!("/api/v1/workspaces/{workspace_id}/chat/files/{id}"),
        file_name: row.get("file_name"),
        mime_type: row.get("mime_type"),
        size_bytes: row.get("size_bytes"),
        width: row.get("width"),
        height: row.get("height"),
    })
}

/// Puts the files of each message into its record: one query for the whole list.
pub(super) async fn load_files(
    conn: &mut SqliteConnection,
    messages: &mut [MessageRecord],
) -> Result<(), ChatError> {
    let rows = sqlx::query(&format!(
        "SELECT {FILE_COLUMNS} FROM chat_message_files \
         WHERE message_id IN (SELECT value FROM json_each(?)) ORDER BY created_at, id"
    ))
    .bind(id_list(messages.iter().map(|message| message.id)))
    .fetch_all(&mut *conn)
    .await?;
    for row in rows {
        let message_id = parse_id(row.get("message_id"))?;
        if let Some(message) = messages.iter_mut().find(|message| message.id == message_id) {
            message.attachments.push(file_from_row(&row)?);
        }
    }
    Ok(())
}

/// Attaches uploaded files to a new message. Each must be the author's own upload in the
/// conversation's workspace that no message has yet.
pub(super) async fn attach_files(
    conn: &mut SqliteConnection,
    message: &mut MessageRecord,
    file_ids: &[Id],
) -> Result<(), ChatError> {
    if file_ids.is_empty() {
        return Ok(());
    }
    let attached = sqlx::query(
        "UPDATE chat_message_files SET message_id = ?, conversation_id = ? \
         WHERE id IN (SELECT value FROM json_each(?)) AND uploaded_by = ? AND message_id IS NULL \
         AND workspace_id = (SELECT workspace_id FROM chat_conversations WHERE id = ?)",
    )
    .bind(message.id.to_string())
    .bind(message.conversation_id.to_string())
    .bind(id_list(file_ids.iter().copied()))
    .bind(message.author_id.to_string())
    .bind(message.conversation_id.to_string())
    .execute(&mut *conn)
    .await?
    .rows_affected();
    if usize::try_from(attached) != Ok(file_ids.len()) {
        return Err(ChatError::Invalid { field: "file_ids" });
    }
    load_files(conn, std::slice::from_mut(message)).await
}

impl ChatRepository {
    /// Succeeds for a member of the live workspace. Run before reading an upload body.
    pub async fn authorize_upload(&self, workspace_id: Id, actor_id: Id) -> Result<(), ChatError> {
        let mut conn = self.database.pool().acquire().await?;
        load_actor(&mut conn, workspace_id, actor_id).await?;
        Ok(())
    }

    /// Turns a staged upload into a chat file that waits for its message. `width` and `height`
    /// are the uploader's measure of an image. The caller discards the staged upload when this
    /// fails.
    pub async fn finalize_file(
        &self,
        workspace_id: Id,
        actor_id: Id,
        upload: &StagedUpload,
        width: Option<u32>,
        height: Option<u32>,
    ) -> Result<ChatFileRecord, ChatError> {
        if upload.workspace_id != workspace_id || upload.owner_id != actor_id {
            return Err(ChatError::NotFound);
        }
        let dimension =
            |value: Option<u32>| value.filter(|value| (1..=MAX_DIMENSION).contains(value));
        let finalization = self.uploads.begin_finalization().await;
        let now = self.database.database_now().await?;
        let mut tx = self.database.immediate_transaction().await?;
        load_actor(&mut tx, workspace_id, actor_id).await?;
        let blob = finalization
            .finalize_blob_in_transaction(&mut tx, upload, now)
            .await?;
        let id = Id::new_v7();
        let size = i64::try_from(upload.size_bytes).map_err(|_| UploadError::SizeOverflow)?;
        sqlx::query(
            "INSERT INTO chat_message_files (id, workspace_id, blob_id, file_name, mime_type, size_bytes, \
             width, height, uploaded_by, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id.to_string())
        .bind(workspace_id.to_string())
        .bind(blob.id.to_string())
        .bind(&upload.display_name)
        .bind(&upload.detected_media_type)
        .bind(size)
        .bind(dimension(width))
        .bind(dimension(height))
        .bind(actor_id.to_string())
        .bind(now.as_millis())
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(ChatFileRecord {
            id,
            url: format!("/api/v1/workspaces/{workspace_id}/chat/files/{id}"),
            file_name: upload.display_name.clone(),
            mime_type: upload.detected_media_type.clone(),
            size_bytes: size,
            width: dimension(width).map(i64::from),
            height: dimension(height).map(i64::from),
        })
    }

    /// Opens a chat file for `actor_id`: its uploader while it waits for its message, then
    /// everyone who can read the conversation.
    pub async fn download_file(
        &self,
        workspace_id: Id,
        actor_id: Id,
        file_id: Id,
    ) -> Result<BlobDownload, ChatError> {
        let database = self.database.clone();
        self.uploads
            .download(move || async move {
                let row = sqlx::query(
                    "SELECT b.storage_key, f.mime_type, f.file_name FROM chat_message_files f \
                     JOIN attachment_blobs b ON b.id = f.blob_id AND b.workspace_id = f.workspace_id \
                     JOIN workspaces w ON w.id = f.workspace_id AND w.deleted_at IS NULL \
                     JOIN memberships me ON me.workspace_id = f.workspace_id AND me.user_id = ? \
                     LEFT JOIN chat_conversations c ON c.id = f.conversation_id \
                     WHERE f.id = ? AND f.workspace_id = ? AND ( \
                         (f.message_id IS NULL AND f.uploaded_by = me.user_id) \
                         OR c.kind = 'public' \
                         OR EXISTS (SELECT 1 FROM chat_members cm \
                                    WHERE cm.conversation_id = c.id AND cm.user_id = me.user_id))",
                )
                .bind(actor_id.to_string())
                .bind(file_id.to_string())
                .bind(workspace_id.to_string())
                .fetch_optional(database.pool())
                .await?
                .ok_or(UploadError::Unauthorized)?;
                Ok(AuthorizedAttachment {
                    storage_key: row.try_get("storage_key")?,
                    media_type: row.try_get("mime_type")?,
                    display_name: row.try_get("file_name")?,
                })
            })
            .await
            .map_err(|error| match error {
                UploadError::Unauthorized => ChatError::NotFound,
                error => ChatError::Upload(error),
            })
    }

    /// The messages of a conversation that have files, newest first.
    pub async fn list_files(
        &self,
        workspace_id: Id,
        actor_id: Id,
        conversation_id: Id,
    ) -> Result<Vec<MessageRecord>, ChatError> {
        let mut tx = self.database.pool().begin().await?;
        load_access(&mut tx, workspace_id, actor_id, conversation_id).await?;
        let rows = sqlx::query(&format!(
            "{MESSAGE_SELECT} WHERE m.id IN (SELECT DISTINCT message_id FROM chat_message_files \
             WHERE conversation_id = ? ORDER BY message_id DESC LIMIT ?) ORDER BY m.id DESC"
        ))
        .bind(conversation_id.to_string())
        .bind(LIST_LIMIT)
        .fetch_all(&mut *tx)
        .await?;
        hydrate(&mut tx, &rows).await
    }
}
