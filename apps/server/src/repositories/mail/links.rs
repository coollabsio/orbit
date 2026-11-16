//! Large files sent as links (spec §5.9, U14). At release a draft file marked `as_link` gets one
//! `mail_file_links` row in the release transaction. The token is 32 random bytes (URL-safe
//! base64); only its SHA-256 is stored, so the database cannot tell anyone the URL. The message
//! gets a block with the file's name, size, expiry date and URL.
//!
//! The download (`GET {content origin}/mail/f/{token}`) is public. [`MailRepository::open_link`]
//! answers `NotFound` for an unknown, expired or revoked token alike, counts every try per client
//! address and every download per link (the `rate_limit_windows` table, so every server of a
//! database shares the counts), and counts the download. The bytes are the uploaded file's: upload
//! blobs are stored as they came (only mail's raw messages are sealed).

use orbit_platform::{Id, Row, TimestampMillis, generate_opaque_token};
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::{Any, Transaction};
use utoipa::ToSchema;

use super::domains::parse_id;
use super::{MailError, MailRepository};

/// Downloads of one link in a day.
pub const LINK_DAY_LIMIT: i64 = 1000;
/// Tries from one client address in a minute (known or unknown tokens).
pub const LINK_IP_MINUTE_LIMIT: i64 = 60;
const DAY_MINUTES: i64 = 24 * 60;
const DAY_MS: i64 = DAY_MINUTES * 60_000;
/// The longest token looked up (a real one has 43 characters).
const TOKEN_MAX: usize = 64;

/// A file link of a sent message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileLink {
    pub id: Id,
    pub filename: String,
    pub size: i64,
    pub expires_at: TimestampMillis,
    /// `{origin}/mail/f/{token}`; empty when the link was opened by its token.
    pub url: String,
}

/// A link as the sender of the message sees it.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct MailFileLinkView {
    #[schema(value_type = String)]
    pub id: Id,
    pub filename: String,
    pub size: i64,
    #[schema(value_type = String, format = DateTime)]
    pub expires_at: TimestampMillis,
    pub revoked: bool,
    pub downloads: i64,
}

/// A link made before the release transaction, so that its URL is in the signed message.
pub(super) struct PlannedLink {
    pub id: Id,
    pub blob_id: String,
    pub filename: String,
    pub size: i64,
    pub token: String,
    pub expires_at: TimestampMillis,
    pub url: String,
}

impl PlannedLink {
    pub(super) fn new(
        origin: &str,
        blob_id: String,
        filename: String,
        size: i64,
        expires_at: TimestampMillis,
    ) -> Self {
        let token = generate_opaque_token();
        Self {
            id: Id::new_v7(),
            url: format!("{}/mail/f/{token}", origin.trim_end_matches('/')),
            blob_id,
            filename,
            size,
            token,
            expires_at,
        }
    }
}

fn token_hash(token: &str) -> String {
    format!("{:x}", Sha256::digest(token.as_bytes()))
}

/// The expiry of a link made at `now` under a policy of `days`.
pub(super) fn link_expiry(now: TimestampMillis, days: i64) -> TimestampMillis {
    TimestampMillis::from_millis(now.as_millis() + days.clamp(1, 365) * DAY_MS)
}

/// `1.5 MB`, `300 KB`, `12 B` (decimal units).
pub(super) fn human_size(bytes: i64) -> String {
    let bytes = bytes.max(0);
    #[allow(clippy::cast_precision_loss)]
    let value = bytes as f64;
    if bytes >= 1_000_000_000 {
        format!("{:.1} GB", value / 1e9)
    } else if bytes >= 1_000_000 {
        format!("{:.1} MB", value / 1e6)
    } else if bytes >= 1000 {
        format!("{:.0} KB", value / 1e3)
    } else {
        format!("{bytes} B")
    }
}

/// Inserts the links of released message `outbound_id`.
pub(super) async fn create_links_for(
    transaction: &mut Transaction<'_, Any>,
    workspace_id: Id,
    outbound_id: Id,
    links: &[PlannedLink],
    now: TimestampMillis,
) -> Result<Vec<FileLink>, MailError> {
    let mut made = Vec::with_capacity(links.len());
    for link in links {
        sqlx::query(
            "INSERT INTO mail_file_links (id, workspace_id, outbound_id, blob_id, filename, size, \
             token_sha256, expires_at, created_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
        )
        .bind(link.id.to_string())
        .bind(workspace_id.to_string())
        .bind(outbound_id.to_string())
        .bind(&link.blob_id)
        .bind(&link.filename)
        .bind(link.size)
        .bind(token_hash(&link.token))
        .bind(link.expires_at.as_millis())
        .bind(now.as_millis())
        .execute(&mut **transaction)
        .await?;
        made.push(FileLink {
            id: link.id,
            filename: link.filename.clone(),
            size: link.size,
            expires_at: link.expires_at,
            url: link.url.clone(),
        });
    }
    Ok(made)
}

/// The links of the messages `message_ids` that `membership_id` sent, by message.
pub(super) async fn sender_links(
    transaction: &mut Transaction<'_, Any>,
    membership_id: Id,
    thread_id: Id,
) -> Result<Vec<(String, MailFileLinkView)>, MailError> {
    let rows = sqlx::query(
        "SELECT o.message_id, l.id, l.filename, l.size, l.expires_at, l.revoked_at, l.downloads \
         FROM mail_file_links l JOIN mail_outbound o ON o.id = l.outbound_id \
         JOIN mail_thread_messages tm ON tm.message_id = o.message_id \
         WHERE tm.thread_id = $1 AND o.membership_id = $2 ORDER BY l.created_at, l.id",
    )
    .bind(thread_id.to_string())
    .bind(membership_id.to_string())
    .fetch_all(&mut **transaction)
    .await?;
    rows.iter()
        .map(|row| {
            Ok((
                row.try_get("message_id")?,
                MailFileLinkView {
                    id: parse_id(&row.try_get::<String, _>("id")?)?,
                    filename: row.try_get("filename")?,
                    size: row.try_get("size")?,
                    expires_at: TimestampMillis::from_millis(row.try_get("expires_at")?),
                    revoked: row.try_get::<Option<i64>, _>("revoked_at")?.is_some(),
                    downloads: row.try_get("downloads")?,
                },
            ))
        })
        .collect()
}

impl MailRepository {
    /// Counts one hit on `bucket` in the window ending at `window_minute`; `false` over `limit`.
    async fn link_hit(
        &self,
        bucket: &str,
        window_minute: i64,
        limit: i64,
    ) -> Result<bool, MailError> {
        let hits: i64 = sqlx::query_scalar(
            "INSERT INTO rate_limit_windows (bucket, window_minute, hits) VALUES ($1, $2, 1) \
             ON CONFLICT (bucket, window_minute) DO UPDATE SET hits = rate_limit_windows.hits + 1 \
             RETURNING hits",
        )
        .bind(bucket)
        .bind(window_minute)
        .fetch_one(self.database.pool())
        .await?;
        Ok(hits <= limit)
    }

    /// Opens a link for a download: the link and the file's bytes. `NotFound` for an unknown,
    /// expired or revoked token (the same answer for all), `RateLimited` over the limits.
    /// `client` is the requester's address (any text; its rate bucket).
    pub async fn open_link(
        &self,
        token: &str,
        client: &str,
        now: TimestampMillis,
    ) -> Result<(FileLink, Vec<u8>), MailError> {
        let minute = now.as_millis().div_euclid(60_000);
        if !self
            .link_hit(
                &format!("mailfile:ip:{client}"),
                minute,
                LINK_IP_MINUTE_LIMIT,
            )
            .await?
        {
            return Err(MailError::RateLimited);
        }
        if token.is_empty() || token.len() > TOKEN_MAX {
            return Err(MailError::NotFound);
        }
        // The unique index finds the row by the hash of the token: nothing compares the token
        // itself, so the lookup does not leak it by timing.
        let row = sqlx::query(
            "SELECT l.id, l.filename, l.size, l.expires_at, l.revoked_at, b.storage_key \
             FROM mail_file_links l JOIN attachment_blobs b ON b.id = l.blob_id \
             WHERE l.token_sha256 = $1",
        )
        .bind(token_hash(token))
        .fetch_optional(self.database.pool())
        .await?
        .ok_or(MailError::NotFound)?;
        let expires_at: i64 = row.try_get("expires_at")?;
        if row.try_get::<Option<i64>, _>("revoked_at")?.is_some() || expires_at <= now.as_millis() {
            return Err(MailError::NotFound);
        }
        let id = parse_id(&row.try_get::<String, _>("id")?)?;
        let day = now.as_millis().div_euclid(DAY_MS);
        // The window row outlives the minute sweep of the limiter: it ends when the day does.
        if !self
            .link_hit(
                &format!("mailfile:link:{id}:{day}"),
                (day + 1) * DAY_MINUTES,
                LINK_DAY_LIMIT,
            )
            .await?
        {
            return Err(MailError::RateLimited);
        }
        let counted = sqlx::query(
            "UPDATE mail_file_links SET downloads = downloads + 1 \
             WHERE id = $1 AND revoked_at IS NULL AND expires_at > $2",
        )
        .bind(id.to_string())
        .bind(now.as_millis())
        .execute(self.database.pool())
        .await?
        .rows_affected();
        if counted != 1 {
            return Err(MailError::NotFound);
        }
        let bytes = self
            .uploads
            .read_blob(&row.try_get::<String, _>("storage_key")?)
            .await?;
        Ok((
            FileLink {
                id,
                filename: row.try_get("filename")?,
                size: row.try_get("size")?,
                expires_at: TimestampMillis::from_millis(expires_at),
                url: String::new(),
            },
            bytes,
        ))
    }

    /// Revokes a link of a message the member sent (`NotFound` for anyone else, and for a link
    /// that is not of this message and thread). Revoking twice keeps the first time.
    pub async fn revoke_link(
        &self,
        membership_id: Id,
        thread_id: Id,
        message_id: Id,
        link_id: Id,
        now: TimestampMillis,
    ) -> Result<MailFileLinkView, MailError> {
        let mut transaction = self.database.immediate_transaction().await?;
        let row = sqlx::query(
            "SELECT l.id FROM mail_file_links l JOIN mail_outbound o ON o.id = l.outbound_id \
             JOIN mail_thread_messages tm ON tm.message_id = o.message_id \
             WHERE l.id = $1 AND o.membership_id = $2 AND o.message_id = $3 AND tm.thread_id = $4",
        )
        .bind(link_id.to_string())
        .bind(membership_id.to_string())
        .bind(message_id.to_string())
        .bind(thread_id.to_string())
        .fetch_optional(&mut *transaction)
        .await?;
        if row.is_none() {
            return Err(MailError::NotFound);
        }
        sqlx::query(
            "UPDATE mail_file_links SET revoked_at = $1 WHERE id = $2 AND revoked_at IS NULL",
        )
        .bind(now.as_millis())
        .bind(link_id.to_string())
        .execute(&mut *transaction)
        .await?;
        let row = sqlx::query(
            "SELECT id, filename, size, expires_at, revoked_at, downloads FROM mail_file_links \
             WHERE id = $1",
        )
        .bind(link_id.to_string())
        .fetch_one(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(MailFileLinkView {
            id: link_id,
            filename: row.try_get("filename")?,
            size: row.try_get("size")?,
            expires_at: TimestampMillis::from_millis(row.try_get("expires_at")?),
            revoked: row.try_get::<Option<i64>, _>("revoked_at")?.is_some(),
            downloads: row.try_get("downloads")?,
        })
    }
}
