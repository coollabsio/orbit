//! Mail content for the reading pane (spec §6.5, §7 Content): the raw message and its parts for
//! the HTML view, attachment downloads and the "Always load images from this sender" choice.
//!
//! The same read rule as threads: the caller's membership must be a member of a mailbox that
//! holds the message; anyone else gets `NotFound`. The raw message is opened from its sealed blob
//! on each call (no HTML or part is stored in clear).

use orbit_platform::{Id, Row, TimestampMillis};

use super::domains::parse_id;
use super::reading::reader;
use super::{MailError, MailRepository};

/// A message as the view document needs it.
#[derive(Debug)]
pub struct MessageContent {
    /// The raw message, decrypted.
    pub raw: Vec<u8>,
    /// mail-parser's part id of the HTML body, when there is one.
    pub html_part: Option<u32>,
    pub body_text: String,
    pub subject: String,
    pub parts: Vec<MessagePart>,
}

/// An attachment or inline part of a message.
#[derive(Clone, Debug)]
pub struct MessagePart {
    pub id: Id,
    pub part_id: u32,
    pub filename: Option<String>,
    /// The type detected from the bytes (`application/octet-stream` when it is none of the
    /// allowed image types).
    pub sniffed_type: String,
    /// Without angle brackets.
    pub content_id: Option<String>,
    pub inline: bool,
}

/// The decoded bytes of one part.
#[derive(Debug)]
pub struct PartContent {
    pub part: MessagePart,
    pub bytes: Vec<u8>,
}

/// The shortest and longest sender address kept as a preference (as the column check).
const ADDRESS_BYTES: std::ops::RangeInclusive<usize> = 3..=320;

impl MailRepository {
    /// `NotFound` unless `user_id` reads a mailbox of the workspace that holds `message_id`.
    pub async fn check_message_reader(
        &self,
        workspace_id: Id,
        user_id: Id,
        message_id: Id,
    ) -> Result<(), MailError> {
        self.readable_message(workspace_id, user_id, message_id)
            .await
            .map(|_| ())
    }

    /// The message with its raw bytes and parts, for a reader of it.
    pub async fn message_content(
        &self,
        workspace_id: Id,
        user_id: Id,
        message_id: Id,
    ) -> Result<MessageContent, MailError> {
        let row = self
            .readable_message(workspace_id, user_id, message_id)
            .await?;
        let parts = self.message_parts(message_id, None).await?;
        let raw = self.read_raw(row.receipt_id).await?;
        Ok(MessageContent {
            raw,
            html_part: row.html_part,
            body_text: row.body_text,
            subject: row.subject,
            parts,
        })
    }

    /// The raw message (`.eml`) and its subject, for a reader of it.
    pub async fn message_raw(
        &self,
        workspace_id: Id,
        user_id: Id,
        message_id: Id,
    ) -> Result<(Vec<u8>, String), MailError> {
        let row = self
            .readable_message(workspace_id, user_id, message_id)
            .await?;
        Ok((self.read_raw(row.receipt_id).await?, row.subject))
    }

    /// The decoded bytes of attachment `attachment_id`, for a reader of its message. With
    /// `message_id`, the attachment must belong to that message.
    pub async fn attachment_content(
        &self,
        workspace_id: Id,
        user_id: Id,
        attachment_id: Id,
        message_id: Option<Id>,
    ) -> Result<PartContent, MailError> {
        let owner: Option<String> =
            sqlx::query_scalar("SELECT message_id FROM mail_attachments WHERE id = $1")
                .bind(attachment_id.to_string())
                .fetch_optional(self.database.pool())
                .await?;
        let owner = parse_id(&owner.ok_or(MailError::NotFound)?)?;
        if message_id.is_some_and(|message_id| message_id != owner) {
            return Err(MailError::NotFound);
        }
        let row = self.readable_message(workspace_id, user_id, owner).await?;
        let part = self
            .message_parts(owner, Some(attachment_id))
            .await?
            .pop()
            .ok_or(MailError::NotFound)?;
        let raw = self.read_raw(row.receipt_id).await?;
        let bytes = orbit_mail::part_bytes(&raw, part.part_id).ok_or(MailError::NotFound)?;
        Ok(PartContent { part, bytes })
    }

    /// Records whether the caller always loads remote images of mail from `address` (the reading
    /// pane offers it only when DMARC passed; [`super::MailMessageView`]'s `load_images_default`
    /// also needs DMARC to pass). Returns the stored address (trimmed, lower case).
    pub async fn set_sender_pref(
        &self,
        workspace_id: Id,
        user_id: Id,
        address: &str,
        load_images: bool,
        now: TimestampMillis,
    ) -> Result<String, MailError> {
        let address = address.trim().to_lowercase();
        let valid = ADDRESS_BYTES.contains(&address.len())
            && address
                .split_once('@')
                .is_some_and(|(local, domain)| !local.is_empty() && !domain.is_empty())
            && !address.chars().any(|c| c.is_whitespace() || c.is_control());
        if !valid {
            return Err(MailError::Invalid { field: "address" });
        }
        let mut transaction = self.database.immediate_transaction().await?;
        let reader = reader(&mut transaction, workspace_id, user_id).await?;
        sqlx::query(
            "INSERT INTO mail_sender_prefs (membership_id, address, load_images, updated_at) \
             VALUES ($1, $2, $3, $4) ON CONFLICT (membership_id, address) \
             DO UPDATE SET load_images = excluded.load_images, updated_at = excluded.updated_at",
        )
        .bind(reader.membership_id.to_string())
        .bind(&address)
        .bind(i64::from(load_images))
        .bind(now.as_millis())
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(address)
    }

    async fn readable_message(
        &self,
        workspace_id: Id,
        user_id: Id,
        message_id: Id,
    ) -> Result<MessageRow, MailError> {
        let mut transaction = self.database.pool().begin().await?;
        let reader = reader(&mut transaction, workspace_id, user_id).await?;
        let row = sqlx::query(
            "SELECT m.receipt_id, m.html_part, m.body_text, m.subject FROM mail_messages m \
             WHERE m.id = $1 AND m.workspace_id = $2 AND EXISTS (\
             SELECT 1 FROM mail_thread_messages tm \
             JOIN mailbox_members mm ON mm.mailbox_id = tm.mailbox_id \
             WHERE tm.message_id = m.id AND mm.membership_id = $3)",
        )
        .bind(message_id.to_string())
        .bind(workspace_id.to_string())
        .bind(reader.membership_id.to_string())
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(MailError::NotFound)?;
        let message = MessageRow {
            receipt_id: parse_id(&row.try_get::<String, _>("receipt_id")?)?,
            html_part: row
                .try_get::<Option<i64>, _>("html_part")?
                .and_then(|part| u32::try_from(part).ok()),
            body_text: row.try_get("body_text")?,
            subject: row.try_get("subject")?,
        };
        transaction.commit().await?;
        Ok(message)
    }

    async fn message_parts(
        &self,
        message_id: Id,
        attachment_id: Option<Id>,
    ) -> Result<Vec<MessagePart>, MailError> {
        let rows = sqlx::query(
            "SELECT id, part_id, filename, sniffed_type, content_id, disposition \
             FROM mail_attachments WHERE message_id = $1 AND ($2 = '' OR id = $3) ORDER BY part_id",
        )
        .bind(message_id.to_string())
        .bind(attachment_id.map(|id| id.to_string()).unwrap_or_default())
        .bind(attachment_id.map(|id| id.to_string()).unwrap_or_default())
        .fetch_all(self.database.pool())
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok(MessagePart {
                    id: parse_id(&row.try_get::<String, _>("id")?)?,
                    part_id: u32::try_from(row.try_get::<i64, _>("part_id")?)
                        .map_err(|_| MailError::NotFound)?,
                    filename: row.try_get("filename")?,
                    sniffed_type: row.try_get("sniffed_type")?,
                    content_id: row.try_get("content_id")?,
                    inline: row.try_get::<String, _>("disposition")? == "inline",
                })
            })
            .collect()
    }
}

struct MessageRow {
    receipt_id: Id,
    html_part: Option<u32>,
    body_text: String,
    subject: String,
}
