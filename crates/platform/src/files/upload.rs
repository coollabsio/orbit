use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;

use sha2::{Digest, Sha256};
use sqlx::{Any, Row, Transaction};
use thiserror::Error;
use tokio::fs;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};

use super::image_shrink::{SHRINK_MIN_BYTES, shrink_image};
use super::{BlobReader, BlobStore, BlobStoreError};
use crate::{
    AttachmentMutationCoordinator, AttachmentOperationGuard, Database, Id, TimestampMillis,
};

const HOUR_MILLIS: i64 = 60 * 60 * 1000;
/// Uploads shrink at most this many images at a time: each decoded image can take hundreds of MB.
static SHRINKING: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);
const QUARANTINE_MILLIS: i64 = 24 * HOUR_MILLIS;
const SNIFF_BYTES: usize = 8 * 1024;
/// References to one blob (bind its id twice): task/comment attachments, docs page files, chat
/// files, profile pictures, custom emoji, stickers and received mail. The second parameter is used
/// for all of the newer tables.
pub const BLOB_REFERENCE_COUNT: &str = "SELECT (SELECT COUNT(*) FROM attachment_references WHERE blob_id = $1) \
     + (SELECT COUNT(*) FROM page_files WHERE blob_id = $2) \
     + (SELECT COUNT(*) FROM chat_message_files WHERE blob_id = $2) \
     + (SELECT COUNT(*) FROM user_avatars WHERE blob_id = $2) \
     + (SELECT COUNT(*) FROM custom_emoji WHERE blob_id = $2) \
     + (SELECT COUNT(*) FROM custom_stickers WHERE blob_id = $2) \
     + (SELECT COUNT(*) FROM mail_receipts WHERE blob_id = $2)";
/// Every table that references blobs by `blob_id`.
const BLOB_REFERENCE_TABLES: [&str; 7] = [
    "attachment_references",
    "page_files",
    "chat_message_files",
    "user_avatars",
    "custom_emoji",
    "custom_stickers",
    "mail_receipts",
];
/// Raster image types that are served inline. Video, audio and plain text are inline too
/// (`serves_inline`). SVG, HTML and PDF stay attachments: a same-origin viewer could run their content.
pub const INLINE_IMAGE_TYPES: [&str; 5] = [
    "image/jpeg",
    "image/png",
    "image/gif",
    "image/webp",
    "image/avif",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UploadLimits {
    max_file_bytes: u64,
    max_request_bytes: u64,
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum UploadLimitError {
    #[error("upload limits must be greater than zero")]
    Zero,
    #[error("the per-file limit must not exceed the per-request limit")]
    FileExceedsRequest,
    #[error("upload limits exceed platform-safe integer bounds")]
    PlatformBound,
}

impl UploadLimits {
    pub fn new(max_file_bytes: u64, max_request_bytes: u64) -> Result<Self, UploadLimitError> {
        if max_file_bytes == 0 || max_request_bytes == 0 {
            return Err(UploadLimitError::Zero);
        }
        if max_file_bytes > max_request_bytes {
            return Err(UploadLimitError::FileExceedsRequest);
        }
        if max_request_bytes > i64::MAX as u64 {
            return Err(UploadLimitError::PlatformBound);
        }
        Ok(Self {
            max_file_bytes,
            max_request_bytes,
        })
    }

    #[must_use]
    pub const fn max_file_bytes(self) -> u64 {
        self.max_file_bytes
    }

    #[must_use]
    pub const fn max_request_bytes(self) -> u64 {
        self.max_request_bytes
    }
}

impl Default for UploadLimits {
    fn default() -> Self {
        Self::new(25 * 1024 * 1024, 100 * 1024 * 1024).expect("default upload limits are valid")
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StagedUpload {
    pub id: Id,
    pub workspace_id: Id,
    pub owner_id: Id,
    pub sha256: String,
    pub size_bytes: u64,
    pub detected_media_type: String,
    pub display_name: String,
    pub(super) temporary_path: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewAttachmentReference {
    pub id: Id,
    pub task_id: Id,
    pub comment_id: Option<Id>,
    /// A file placed in a task description or comment text: not listed with the attachments.
    pub inline: bool,
}

impl NewAttachmentReference {
    #[must_use]
    pub fn new(task_id: Id, comment_id: Option<Id>) -> Self {
        Self {
            id: Id::new_v7(),
            task_id,
            comment_id,
            inline: false,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FinalizedAttachment {
    pub blob: FinalizedBlob,
    pub reference_id: Id,
    pub created_at: TimestampMillis,
}

pub struct UploadFinalization<'a> {
    service: &'a UploadService,
    _operation: AttachmentOperationGuard,
}

/// A staged upload whose bytes are in the blob store and whose `attachment_blobs` row is committed with a fresh
/// quarantine, so reconcile on any server keeps both. Only [`UploadFinalization::prepare`] makes one, in its own
/// short transactions: the store (S3) is never called inside the caller's write transaction.
#[derive(Debug)]
pub struct PreparedUpload {
    upload: StagedUpload,
    claimed: ClaimedBlob,
}

/// [`PreparedUpload`] for bytes without a pending upload. Only [`UploadFinalization::prepare_bytes`] and
/// [`UploadFinalization::prepare_unmetered_bytes`] make one.
#[derive(Debug)]
pub struct PreparedBlob {
    claimed: ClaimedBlob,
}

/// A committed `attachment_blobs` row from [`UploadService::claim_blob`].
#[derive(Debug)]
struct ClaimedBlob {
    blob: FinalizedBlob,
    /// The claim added the row; otherwise the workspace already had a row for these bytes.
    created: bool,
}

/// What [`UploadService::claim_blob`] checks before it records the blob row.
#[derive(Clone, Copy)]
enum Claim {
    /// A staged pending upload, within the workspace's storage quota.
    PendingUpload,
    MeteredBytes,
    UnmeteredBytes,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FinalizedBlob {
    pub id: Id,
    pub workspace_id: Id,
    pub sha256: String,
    pub size_bytes: u64,
    pub storage_key: String,
    pub quarantine_until: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContentDisposition {
    Inline,
    Attachment,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DownloadMetadata {
    pub content_type: String,
    pub disposition: ContentDisposition,
    pub content_disposition: String,
    pub x_content_type_options: &'static str,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthorizedAttachment {
    pub storage_key: String,
    pub media_type: String,
    pub display_name: String,
    /// An admin deleted the blob (`attachment_blobs.deleted_at`); there are no bytes to serve.
    pub deleted: bool,
}

pub struct BlobDownload {
    pub reader: BlobReader,
    pub metadata: DownloadMetadata,
    /// The blob was deleted: `reader` is a "Deleted file" image, to send with status 410.
    pub deleted: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ReconcileResult {
    pub expired_uploads: u64,
    pub deleted_blobs: u64,
    pub deleted_untracked_files: u64,
}

#[derive(Debug, Error)]
pub enum UploadError {
    #[error("file exceeds the configured {limit}-byte limit")]
    FileTooLarge { limit: u64 },
    #[error("request exceeds the configured {limit}-byte limit")]
    RequestTooLarge { limit: u64 },
    #[error("the file would take the workspace over its {limit}-byte storage quota")]
    StorageQuotaExceeded { limit: u64 },
    #[error("attachment display name is empty, too long, or contains control characters")]
    InvalidDisplayName,
    #[error("staged upload is not in a finalizable state")]
    InvalidState,
    #[error("staged upload contents changed after validation")]
    StagedContentChanged,
    #[error("attachment access was denied")]
    Unauthorized,
    #[error(transparent)]
    BlobStore(#[from] BlobStoreError),
    #[error("attachment database operation failed: {0}")]
    Database(#[from] sqlx::Error),
    #[error("attachment size exceeds platform integer bounds")]
    SizeOverflow,
}

#[derive(Clone)]
pub struct UploadService {
    database: Database,
    store: Arc<dyn BlobStore>,
    mutations: AttachmentMutationCoordinator,
    limits: UploadLimits,
}

impl UploadFinalization<'_> {
    /// Records the blob of a staged upload (within the workspace's storage quota) and writes its bytes to the
    /// store. Call it before the write transaction that references the blob: it opens its own short transaction,
    /// and the store can be slow. When no reference follows, reconcile removes the blob after the quarantine.
    pub async fn prepare(&self, upload: &StagedUpload) -> Result<PreparedUpload, UploadError> {
        let claimed = self
            .service
            .claim_blob(upload, Claim::PendingUpload)
            .await?;
        match self.service.store.install(upload).await {
            Ok(_) => Ok(PreparedUpload {
                upload: upload.clone(),
                claimed,
            }),
            Err(error) => {
                self.service.unclaim(&claimed).await;
                Err(match error {
                    BlobStoreError::StagedContentChanged => UploadError::StagedContentChanged,
                    error => error.into(),
                })
            }
        }
    }

    /// [`Self::prepare`] for bytes the server already checked (an emoji, a sticker, a received mail), as a blob of
    /// `workspace_id` within its storage quota, without a pending upload.
    pub async fn prepare_bytes(
        &self,
        workspace_id: Id,
        bytes: &[u8],
    ) -> Result<PreparedBlob, UploadError> {
        let (_, claimed) = self
            .service
            .install_claimed_bytes(workspace_id, bytes, Claim::MeteredBytes)
            .await?;
        Ok(PreparedBlob { claimed })
    }

    /// [`Self::prepare_bytes`] without the storage quota: for a profile picture (grouped under the user id, not a
    /// workspace) and for images that already were in the database before they move to the store.
    pub async fn prepare_unmetered_bytes(
        &self,
        scope_id: Id,
        bytes: &[u8],
    ) -> Result<PreparedBlob, UploadError> {
        let (_, claimed) = self
            .service
            .install_claimed_bytes(scope_id, bytes, Claim::UnmeteredBytes)
            .await?;
        Ok(PreparedBlob { claimed })
    }

    /// Gives up a prepared blob that gets no reference, so its bytes stop counting toward the workspace's storage
    /// now and not only after the quarantine. For callers that fail often with bytes nobody uploads again (a
    /// refused mail that its sender retries). Does nothing when the workspace had the blob before, or something
    /// references it. Best effort: reconcile removes what stays.
    pub async fn abandon(&self, prepared: &PreparedBlob) {
        self.service.unclaim(&prepared.claimed).await;
    }

    /// [`Self::finalize_blob_in_transaction`] plus the task attachment reference.
    pub async fn finalize_in_transaction(
        &self,
        transaction: &mut Transaction<'_, Any>,
        prepared: &PreparedUpload,
        attachment: NewAttachmentReference,
        now: TimestampMillis,
    ) -> Result<FinalizedAttachment, UploadError> {
        let blob = self
            .finalize_blob_in_transaction(transaction, prepared, now)
            .await?;
        let upload = &prepared.upload;
        let size_i64 = i64::try_from(upload.size_bytes).map_err(|_| UploadError::SizeOverflow)?;
        sqlx::query(
            "INSERT INTO attachment_references (\
                id, workspace_id, task_id, comment_id, owner_id, blob_id, display_name, media_type,\
                byte_size, created_at, inline\
             ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
        )
        .bind(attachment.id.to_string())
        .bind(upload.workspace_id.to_string())
        .bind(attachment.task_id.to_string())
        .bind(attachment.comment_id.map(|id| id.to_string()))
        .bind(upload.owner_id.to_string())
        .bind(blob.id.to_string())
        .bind(&upload.display_name)
        .bind(&upload.detected_media_type)
        .bind(size_i64)
        .bind(now.as_millis())
        .bind(i64::from(attachment.inline))
        .execute(&mut **transaction)
        .await?;

        Ok(FinalizedAttachment {
            blob,
            reference_id: attachment.id,
            created_at: now,
        })
    }

    /// Completes the pending upload of a prepared blob, without creating a reference. The caller must insert its
    /// own reference row (e.g. a page file) in the same transaction, or the blob becomes reclaimable after
    /// quarantine. Fails with [`UploadError::InvalidState`] when the upload is no longer staged, or the blob was
    /// deleted or replaced since [`Self::prepare`].
    pub async fn finalize_blob_in_transaction(
        &self,
        transaction: &mut Transaction<'_, Any>,
        prepared: &PreparedUpload,
        now: TimestampMillis,
    ) -> Result<FinalizedBlob, UploadError> {
        let upload = &prepared.upload;
        require_staged(transaction, upload, now).await?;
        require_claimed(transaction, &prepared.claimed.blob).await?;
        let size_i64 = i64::try_from(upload.size_bytes).map_err(|_| UploadError::SizeOverflow)?;
        let updated = sqlx::query(
            "UPDATE pending_uploads SET state = 'complete', completed_at = $1 \
             WHERE id = $2 AND workspace_id = $3 AND user_id = $4 AND state = 'staged' \
               AND sha256 = $5 AND byte_size = $6",
        )
        .bind(now.as_millis())
        .bind(upload.id.to_string())
        .bind(upload.workspace_id.to_string())
        .bind(upload.owner_id.to_string())
        .bind(&upload.sha256)
        .bind(size_i64)
        .execute(&mut **transaction)
        .await?;
        if updated.rows_affected() != 1 {
            return Err(UploadError::InvalidState);
        }

        Ok(prepared.claimed.blob.clone())
    }

    /// The blob of [`Self::prepare_bytes`] or [`Self::prepare_unmetered_bytes`], checked inside the caller's write
    /// transaction. As with [`Self::finalize_blob_in_transaction`], the caller inserts its reference in the same
    /// transaction.
    pub async fn finalize_bytes_in_transaction(
        &self,
        transaction: &mut Transaction<'_, Any>,
        prepared: &PreparedBlob,
    ) -> Result<FinalizedBlob, UploadError> {
        require_claimed(transaction, &prepared.claimed.blob).await?;
        Ok(prepared.claimed.blob.clone())
    }
}

async fn require_staged(
    transaction: &mut Transaction<'_, Any>,
    upload: &StagedUpload,
    now: TimestampMillis,
) -> Result<(), UploadError> {
    let size_i64 = i64::try_from(upload.size_bytes).map_err(|_| UploadError::SizeOverflow)?;
    let valid: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pending_uploads \
         WHERE id = $1 AND workspace_id = $2 AND user_id = $3 AND temporary_path = $4 \
           AND original_name = $5 AND media_type = $6 AND byte_size = $7 AND sha256 = $8 \
           AND state = 'staged' AND expires_at > $9",
    )
    .bind(upload.id.to_string())
    .bind(upload.workspace_id.to_string())
    .bind(upload.owner_id.to_string())
    .bind(upload.temporary_path.to_string_lossy().as_ref())
    .bind(&upload.display_name)
    .bind(&upload.detected_media_type)
    .bind(size_i64)
    .bind(&upload.sha256)
    .bind(now.as_millis())
    .fetch_one(&mut **transaction)
    .await?;
    if valid != 1 {
        return Err(UploadError::InvalidState);
    }
    Ok(())
}

/// The prepared blob's row still holds the prepared bytes: reconcile did not reclaim it, an admin did not delete it
/// and [`UploadService::replace_blob`] did not change it (each of those changes the row's hash or removes the row).
async fn require_claimed(
    transaction: &mut Transaction<'_, Any>,
    blob: &FinalizedBlob,
) -> Result<(), UploadError> {
    let size_i64 = i64::try_from(blob.size_bytes).map_err(|_| UploadError::SizeOverflow)?;
    let claimed: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM attachment_blobs \
         WHERE id = $1 AND workspace_id = $2 AND sha256 = $3 AND byte_size = $4",
    )
    .bind(blob.id.to_string())
    .bind(blob.workspace_id.to_string())
    .bind(&blob.sha256)
    .bind(size_i64)
    .fetch_one(&mut **transaction)
    .await?;
    if claimed != 1 {
        return Err(UploadError::InvalidState);
    }
    Ok(())
}

/// Bytes of files a workspace stores: each blob once, however many references it has. A blob stops counting
/// when reconcile deletes it, a day after its last reference went (see [`QUARANTINE_MILLIS`]), or at once when an
/// admin deletes it ([`UploadService::delete_blob`]).
pub const WORKSPACE_STORAGE_USED: &str = "SELECT CAST(COALESCE(SUM(byte_size), 0) AS BIGINT) FROM attachment_blobs WHERE workspace_id = $1 AND deleted_at IS NULL";

/// Served (with status 410) in place of a blob an admin deleted, so an image of it, or a link to it, says so.
const DELETED_FILE_SVG: &str = concat!(
    r##"<svg xmlns="http://www.w3.org/2000/svg" width="320" height="80" viewBox="0 0 320 80">"##,
    r##"<rect x="0.5" y="0.5" width="319" height="79" rx="8" fill="#8881" stroke="#8886"/>"##,
    r##"<text x="160" y="46" text-anchor="middle" font-family="system-ui,sans-serif" font-size="15" fill="#888">"##,
    "Deleted file</text></svg>"
);

/// Refuses a new blob that would take the workspace over `instance_settings.workspace_storage_quota_bytes` (0 is
/// unlimited). Bytes the workspace already stores cost nothing, as blobs are deduplicated per workspace. Runs in
/// the serialized write transaction that records the blob, before the bytes go to the store, so concurrent uploads
/// on any server cannot pass the quota together.
async fn ensure_storage_quota(
    transaction: &mut Transaction<'_, Any>,
    workspace_id: Id,
    sha256: &str,
    size: i64,
) -> Result<(), UploadError> {
    let row = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT workspace_storage_quota_bytes AS quota, \
         (SELECT COUNT(*) FROM attachment_blobs WHERE workspace_id = $1 AND sha256 = $2 AND byte_size = $3) AS stored, \
         ({WORKSPACE_STORAGE_USED}) AS used \
         FROM instance_settings WHERE id = 1"
    )))
    .bind(workspace_id.to_string())
    .bind(sha256)
    .bind(size)
    .fetch_one(&mut **transaction)
    .await?;
    let quota: i64 = row.try_get("quota")?;
    let stored: i64 = row.try_get("stored")?;
    let used: i64 = row.try_get("used")?;
    if quota > 0 && stored == 0 && used.saturating_add(size) > quota {
        return Err(UploadError::StorageQuotaExceeded {
            limit: u64::try_from(quota).unwrap_or(u64::MAX),
        });
    }
    Ok(())
}

/// Adds the `attachment_blobs` row of a blob that is about to be installed, or finds the row of the same bytes.
/// Either way the row's quarantine then ends no sooner than a day from `now`, so reconcile on any server neither
/// deletes the row nor releases its object while the bytes are written and the reference is committed.
async fn record_blob(
    transaction: &mut Transaction<'_, Any>,
    upload: &StagedUpload,
    now: TimestampMillis,
) -> Result<ClaimedBlob, UploadError> {
    let size_i64 = i64::try_from(upload.size_bytes).map_err(|_| UploadError::SizeOverflow)?;
    let quarantine_until = now.as_millis().saturating_add(QUARANTINE_MILLIS);
    let new_id = Id::new_v7();
    sqlx::query(
        "INSERT INTO attachment_blobs (\
            id, workspace_id, sha256, byte_size, storage_key, created_at, quarantine_until\
         ) VALUES ($1, $2, $3, $4, $5, $6, $7) \
         ON CONFLICT (workspace_id, sha256, byte_size) DO UPDATE SET quarantine_until = CASE \
            WHEN attachment_blobs.quarantine_until > excluded.quarantine_until \
            THEN attachment_blobs.quarantine_until ELSE excluded.quarantine_until END",
    )
    .bind(new_id.to_string())
    .bind(upload.workspace_id.to_string())
    .bind(&upload.sha256)
    .bind(size_i64)
    .bind(super::blob_storage_key(upload))
    .bind(now.as_millis())
    .bind(quarantine_until)
    .execute(&mut **transaction)
    .await?;

    let row = sqlx::query(
        "SELECT id, storage_key, quarantine_until FROM attachment_blobs \
         WHERE workspace_id = $1 AND sha256 = $2 AND byte_size = $3",
    )
    .bind(upload.workspace_id.to_string())
    .bind(&upload.sha256)
    .bind(size_i64)
    .fetch_one(&mut **transaction)
    .await?;
    let id_text: String = row.try_get("id")?;
    let id: Id = id_text.parse().map_err(|_| UploadError::InvalidState)?;
    Ok(ClaimedBlob {
        blob: FinalizedBlob {
            id,
            workspace_id: upload.workspace_id,
            sha256: upload.sha256.clone(),
            size_bytes: upload.size_bytes,
            storage_key: row.try_get("storage_key")?,
            quarantine_until: row.try_get("quarantine_until")?,
        },
        created: id == new_id,
    })
}

impl UploadService {
    #[must_use]
    pub fn new(
        database: Database,
        store: Arc<dyn BlobStore>,
        mutations: AttachmentMutationCoordinator,
        limits: UploadLimits,
    ) -> Self {
        Self {
            database,
            store,
            mutations,
            limits,
        }
    }

    #[must_use]
    pub const fn limits(&self) -> UploadLimits {
        self.limits
    }

    pub async fn stage<R>(
        &self,
        workspace_id: Id,
        owner_id: Id,
        display_name: &str,
        reader: R,
    ) -> Result<StagedUpload, UploadError>
    where
        R: AsyncRead + Unpin,
    {
        self.stage_in_request(workspace_id, owner_id, display_name, 0, reader)
            .await
    }

    /// [`Self::stage`] that keeps the exact bytes, without [`shrink_image`]: for a restore, which must give back
    /// the files of the backup.
    pub async fn stage_original<R>(
        &self,
        workspace_id: Id,
        owner_id: Id,
        display_name: &str,
        reader: R,
    ) -> Result<StagedUpload, UploadError>
    where
        R: AsyncRead + Unpin,
    {
        self.stage_with(workspace_id, owner_id, display_name, 0, reader, false)
            .await
    }

    /// Stages one file of a request. A large JPEG, PNG or WebP is stored as a smaller WebP ([`shrink_image`]),
    /// as the web client does, so a client that did not shrink it does not store the full size.
    pub async fn stage_in_request<R>(
        &self,
        workspace_id: Id,
        owner_id: Id,
        display_name: &str,
        request_bytes_received: u64,
        reader: R,
    ) -> Result<StagedUpload, UploadError>
    where
        R: AsyncRead + Unpin,
    {
        self.stage_with(
            workspace_id,
            owner_id,
            display_name,
            request_bytes_received,
            reader,
            true,
        )
        .await
    }

    async fn stage_with<R>(
        &self,
        workspace_id: Id,
        owner_id: Id,
        display_name: &str,
        request_bytes_received: u64,
        mut reader: R,
        shrink: bool,
    ) -> Result<StagedUpload, UploadError>
    where
        R: AsyncRead + Unpin,
    {
        let mut display_name = safe_display_name(display_name)?;
        if request_bytes_received >= self.limits.max_request_bytes {
            return Err(UploadError::RequestTooLarge {
                limit: self.limits.max_request_bytes,
            });
        }

        let _mutation = self.mutations.begin().await;
        let temporary_path = self.store.create_temporary()?;
        let mut temporary = TemporaryFile::new(self.store.clone(), temporary_path.clone());
        let result = async {
            let mut output = fs::OpenOptions::new()
                .write(true)
                .truncate(true)
                .open(&temporary_path)
                .await
                .map_err(|source| BlobStoreError::Io {
                    path: temporary_path.clone(),
                    source,
                })?;
            let mut digest = Sha256::new();
            let mut size_bytes = 0_u64;
            let mut sniffed = Vec::with_capacity(SNIFF_BYTES);
            let mut buffer = [0_u8; 64 * 1024];

            loop {
                let count =
                    reader
                        .read(&mut buffer)
                        .await
                        .map_err(|source| BlobStoreError::Io {
                            path: temporary_path.clone(),
                            source,
                        })?;
                if count == 0 {
                    break;
                }
                size_bytes = size_bytes
                    .checked_add(count as u64)
                    .ok_or(UploadError::SizeOverflow)?;
                if size_bytes > self.limits.max_file_bytes {
                    return Err(UploadError::FileTooLarge {
                        limit: self.limits.max_file_bytes,
                    });
                }
                if request_bytes_received.saturating_add(size_bytes) > self.limits.max_request_bytes
                {
                    return Err(UploadError::RequestTooLarge {
                        limit: self.limits.max_request_bytes,
                    });
                }
                let sniff_count = count.min(SNIFF_BYTES.saturating_sub(sniffed.len()));
                sniffed.extend_from_slice(&buffer[..sniff_count]);
                digest.update(&buffer[..count]);
                output
                    .write_all(&buffer[..count])
                    .await
                    .map_err(|source| BlobStoreError::Io {
                        path: temporary_path.clone(),
                        source,
                    })?;
            }
            output.flush().await.map_err(|source| BlobStoreError::Io {
                path: temporary_path.clone(),
                source,
            })?;

            let mut sha256 = format!("{:x}", digest.finalize());
            let mut detected_media_type = detect_media_type(&sniffed).to_owned();
            if shrink
                && let Some(smaller) =
                    shrink_staged(&temporary_path, &detected_media_type, size_bytes).await?
            {
                fs::write(&temporary_path, &smaller)
                    .await
                    .map_err(|source| BlobStoreError::Io {
                        path: temporary_path.clone(),
                        source,
                    })?;
                sha256 = format!("{:x}", Sha256::digest(&smaller));
                size_bytes = smaller.len() as u64;
                detected_media_type = detect_media_type(&smaller).to_owned();
                display_name = with_extension(&display_name, "webp");
            }

            let id = Id::new_v7();
            let now = self.database.database_now().await?.as_millis();
            let size_i64 = i64::try_from(size_bytes).map_err(|_| UploadError::SizeOverflow)?;
            sqlx::query(
                "INSERT INTO pending_uploads (\
                    id, workspace_id, user_id, temporary_path, original_name, media_type,\
                    byte_size, sha256, state, created_at, expires_at\
                 ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 'staged', $9, $10)",
            )
            .bind(id.to_string())
            .bind(workspace_id.to_string())
            .bind(owner_id.to_string())
            .bind(temporary_path.to_string_lossy().as_ref())
            .bind(&display_name)
            .bind(&detected_media_type)
            .bind(size_i64)
            .bind(&sha256)
            .bind(now)
            .bind(now.saturating_add(QUARANTINE_MILLIS))
            .execute(self.database.pool())
            .await?;

            Ok(StagedUpload {
                id,
                workspace_id,
                owner_id,
                sha256,
                size_bytes,
                detected_media_type,
                display_name,
                temporary_path: temporary_path.clone(),
            })
        }
        .await;

        if result.is_ok() {
            temporary.disarm();
        } else {
            temporary.cleanup().await;
        }
        result
    }

    /// Holds upload/reconciliation coordination while a server-owned transaction performs
    /// domain authorization and attachment finalization.
    pub async fn begin_finalization(&self) -> UploadFinalization<'_> {
        UploadFinalization {
            service: self,
            _operation: self.mutations.begin_operation().await,
        }
    }

    pub async fn finalize(
        &self,
        upload: &StagedUpload,
        attachment: NewAttachmentReference,
    ) -> Result<FinalizedBlob, UploadError> {
        let finalization = self.begin_finalization().await;
        let prepared = finalization.prepare(upload).await?;
        let now = self.database.database_now().await?;
        let mut transaction = self.database.immediate_transaction().await?;
        let finalized = finalization
            .finalize_in_transaction(&mut transaction, &prepared, attachment, now)
            .await?;
        transaction.commit().await?;
        Ok(finalized.blob)
    }

    /// Abandons a staged upload that will not be finalized, removing its temporary bytes now.
    pub async fn discard(&self, upload: &StagedUpload) -> Result<(), UploadError> {
        let _operation = self.mutations.begin_operation().await;
        self.store.delete_temporary(&upload.temporary_path).await?;
        sqlx::query(
            "UPDATE pending_uploads SET state = 'failed' WHERE id = $1 AND workspace_id = $2 \
             AND user_id = $3 AND state = 'staged'",
        )
        .bind(upload.id.to_string())
        .bind(upload.workspace_id.to_string())
        .bind(upload.owner_id.to_string())
        .execute(self.database.pool())
        .await?;
        Ok(())
    }

    /// Deletes a blob of `workspace_id` for an admin, keeping its references: tasks, pages and chat messages that
    /// use it then get the "Deleted file" image, and it stops counting toward the workspace's storage at once. Its
    /// row gets a `deleted:<id>` hash and key, so the same bytes can be uploaded again as a new blob, and reconcile
    /// removes the stored object as untracked (after the usual quarantine, which also covers a concurrent upload of
    /// the same bytes). Returns `false` when there is no such live blob, or a profile picture, custom emoji or
    /// sticker uses it (those are deleted where they are managed).
    pub async fn delete_blob(&self, workspace_id: Id, blob_id: Id) -> Result<bool, UploadError> {
        let _operation = self.mutations.begin_operation().await;
        let now = self.database.database_now().await?;
        let tombstone = format!("deleted:{blob_id}");
        let mut transaction = self.database.immediate_transaction().await?;
        let deleted = sqlx::query(
            "UPDATE attachment_blobs SET deleted_at = $1, sha256 = $2, storage_key = $2 \
             WHERE id = $3 AND workspace_id = $4 AND deleted_at IS NULL \
             AND NOT EXISTS (SELECT 1 FROM user_avatars WHERE blob_id = $3) \
             AND NOT EXISTS (SELECT 1 FROM custom_emoji WHERE blob_id = $3) \
             AND NOT EXISTS (SELECT 1 FROM custom_stickers WHERE blob_id = $3)",
        )
        .bind(now.as_millis())
        .bind(&tombstone)
        .bind(blob_id.to_string())
        .bind(workspace_id.to_string())
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        transaction.commit().await?;
        Ok(deleted == 1)
    }

    /// The stored bytes of a blob, from the local disk or S3.
    pub async fn read_blob(&self, storage_key: &str) -> Result<Vec<u8>, UploadError> {
        let mut reader = self.store.open(storage_key).await?;
        let mut bytes = Vec::new();
        reader
            .read_to_end(&mut bytes)
            .await
            .map_err(|source| BlobStoreError::Io {
                path: PathBuf::from(storage_key),
                source,
            })?;
        Ok(bytes)
    }

    /// Replaces a blob's bytes with a re-encoded version (for example a smaller image). The blob keeps its id, so
    /// every task, page and chat reference stays; their media type, size and file extension follow the new bytes.
    /// Returns `false` and changes nothing when the blob changed since `expected_key` was read, or the workspace
    /// already stores these exact bytes. The old file is released like an unreferenced blob.
    pub async fn replace_blob(
        &self,
        blob_id: &str,
        expected_key: &str,
        bytes: &[u8],
        extension: &str,
    ) -> Result<bool, UploadError> {
        let _operation = self.mutations.begin_operation().await;
        let Some(row) = sqlx::query(
            "SELECT workspace_id FROM attachment_blobs WHERE id = $1 AND storage_key = $2",
        )
        .bind(blob_id)
        .bind(expected_key)
        .fetch_optional(self.database.pool())
        .await?
        else {
            return Ok(false);
        };
        let workspace_id: String = row.try_get("workspace_id")?;
        let sha256 = format!("{:x}", Sha256::digest(bytes));
        let size_bytes = u64::try_from(bytes.len()).map_err(|_| UploadError::SizeOverflow)?;
        let size_i64 = i64::try_from(size_bytes).map_err(|_| UploadError::SizeOverflow)?;
        let duplicate: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM attachment_blobs WHERE workspace_id = $1 AND sha256 = $2 AND byte_size = $3",
        )
        .bind(&workspace_id)
        .bind(&sha256)
        .bind(size_i64)
        .fetch_one(self.database.pool())
        .await?;
        if duplicate > 0 {
            return Ok(false);
        }

        let workspace_id = workspace_id
            .parse()
            .map_err(|_| UploadError::InvalidState)?;
        // The new bytes get a blob row of their own while they are written, so reconcile on another server does
        // not delete their object as untracked. The transaction below hands the object over to `blob_id`.
        let (staged, claimed) = self
            .install_claimed_bytes(workspace_id, bytes, Claim::UnmeteredBytes)
            .await?;
        let storage_key = super::blob_storage_key(&staged);
        let media_type = staged.detected_media_type.as_str();
        let claimed_id = claimed.blob.id.to_string();

        // An early return rolls back: the claimed row then stays, and reconcile removes it after its quarantine.
        let mut transaction = self.database.immediate_transaction().await?;
        let claimed_references: i64 = sqlx::query_scalar(BLOB_REFERENCE_COUNT)
            .bind(&claimed_id)
            .bind(&claimed_id)
            .fetch_one(&mut *transaction)
            .await?;
        if claimed_references > 0 {
            // An upload of the same bytes took the claimed row meanwhile.
            return Ok(false);
        }
        sqlx::query("DELETE FROM attachment_blobs WHERE id = $1")
            .bind(&claimed_id)
            .execute(&mut *transaction)
            .await?;
        let updated = sqlx::query(
            "UPDATE attachment_blobs SET sha256 = $1, byte_size = $2, storage_key = $3 \
             WHERE id = $4 AND storage_key = $5",
        )
        .bind(&sha256)
        .bind(size_i64)
        .bind(&storage_key)
        .bind(blob_id)
        .bind(expected_key)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if updated != 1 {
            return Ok(false);
        }
        for (table, name, media, size) in [
            (
                "attachment_references",
                "display_name",
                "media_type",
                "byte_size",
            ),
            ("page_files", "file_name", "mime_type", "size_bytes"),
            ("chat_message_files", "file_name", "mime_type", "size_bytes"),
        ] {
            let references = sqlx::query(sqlx::AssertSqlSafe(format!(
                "SELECT id, {name} AS name FROM {table} WHERE blob_id = $1"
            )))
            .bind(blob_id)
            .fetch_all(&mut *transaction)
            .await?;
            for reference in references {
                let old_name: String = reference.try_get("name")?;
                sqlx::query(sqlx::AssertSqlSafe(format!(
                    "UPDATE {table} SET {name} = $1, {media} = $2, {size} = $3 WHERE id = $4"
                )))
                .bind(with_extension(&old_name, extension))
                .bind(media_type)
                .bind(size_i64)
                .bind(reference.try_get::<String, _>("id")?)
                .execute(&mut *transaction)
                .await?;
            }
        }
        for table in ["user_avatars", "custom_emoji", "custom_stickers"] {
            sqlx::query(sqlx::AssertSqlSafe(format!(
                "UPDATE {table} SET mime_type = $1 WHERE blob_id = $2"
            )))
            .bind(media_type)
            .bind(blob_id)
            .execute(&mut *transaction)
            .await?;
        }
        transaction.commit().await?;
        self.store.release(expected_key).await?;
        Ok(true)
    }

    /// Records `bytes` as a blob of `scope_id` and writes them to the store: the row first, in its own short
    /// transaction ([`Self::claim_blob`]), then the store with no database lock held.
    async fn install_claimed_bytes(
        &self,
        scope_id: Id,
        bytes: &[u8],
        claim: Claim,
    ) -> Result<(StagedUpload, ClaimedBlob), UploadError> {
        let size_bytes = u64::try_from(bytes.len()).map_err(|_| UploadError::SizeOverflow)?;
        let temporary_path = self.store.create_temporary()?;
        let mut temporary = TemporaryFile::new(self.store.clone(), temporary_path.clone());
        fs::write(&temporary_path, bytes)
            .await
            .map_err(|source| BlobStoreError::Io {
                path: temporary_path.clone(),
                source,
            })?;
        let staged = StagedUpload {
            id: Id::new_v7(),
            workspace_id: scope_id,
            owner_id: Id::new_v7(),
            sha256: format!("{:x}", Sha256::digest(bytes)),
            size_bytes,
            detected_media_type: detect_media_type(bytes).to_owned(),
            display_name: String::new(),
            temporary_path,
        };
        let claimed = self.claim_blob(&staged, claim).await?;
        if let Err(error) = self.store.install(&staged).await {
            self.unclaim(&claimed).await;
            return Err(error.into());
        }
        temporary.disarm();
        Ok((staged, claimed))
    }

    /// Removes the row of a claim that added it, unless something references the blob, so bytes that never got
    /// a reference stop counting toward the workspace's storage at once. Safe against an upload of the same bytes
    /// that found the row meanwhile, on any server: its write transaction either committed a reference before
    /// this one, and the row stays, or finds the row gone ([`require_claimed`]) and fails without a reference. Best
    /// effort: on an error the row stays, and reconcile removes it after its quarantine.
    async fn unclaim(&self, claimed: &ClaimedBlob) {
        if !claimed.created {
            return;
        }
        let id = claimed.blob.id.to_string();
        let removed = async {
            let mut transaction = self.database.immediate_transaction().await?;
            let references: i64 = sqlx::query_scalar(BLOB_REFERENCE_COUNT)
                .bind(&id)
                .bind(&id)
                .fetch_one(&mut *transaction)
                .await?;
            if references == 0 {
                sqlx::query("DELETE FROM attachment_blobs WHERE id = $1")
                    .bind(&id)
                    .execute(&mut *transaction)
                    .await?;
            }
            transaction.commit().await
        }
        .await;
        if let Err(error) = removed {
            tracing::debug!(%error, blob_id = %id, "an unused blob row stays until reconcile removes it");
        }
    }

    /// Commits the blob row of `upload` before its bytes go to the store, in a short write transaction that calls
    /// no store. The upload's object is tracked from then on, and the row's quarantine is fresh, so reconcile on
    /// any server leaves both alone while the bytes are written with no database lock held. A failure after this
    /// leaves a row without references, which reconcile removes after the quarantine.
    async fn claim_blob(
        &self,
        upload: &StagedUpload,
        claim: Claim,
    ) -> Result<ClaimedBlob, UploadError> {
        let size_i64 = i64::try_from(upload.size_bytes).map_err(|_| UploadError::SizeOverflow)?;
        let now = self.database.database_now().await?;
        let mut transaction = self.database.immediate_transaction().await?;
        if matches!(claim, Claim::PendingUpload) {
            require_staged(&mut transaction, upload, now).await?;
        }
        if !matches!(claim, Claim::UnmeteredBytes) {
            ensure_storage_quota(
                &mut transaction,
                upload.workspace_id,
                &upload.sha256,
                size_i64,
            )
            .await?;
        }
        let claimed = record_blob(&mut transaction, upload, now).await?;
        transaction.commit().await?;
        Ok(claimed)
    }

    pub async fn reconcile(&self, now_millis: i64) -> Result<ReconcileResult, UploadError> {
        let _operation = self.mutations.begin_operation().await;
        let mut result = ReconcileResult::default();

        // Fail closed: without every reference table, no blob can be proven unreferenced.
        for table in BLOB_REFERENCE_TABLES {
            sqlx::query(sqlx::AssertSqlSafe(format!(
                "SELECT blob_id FROM {table} WHERE 1 = 0"
            )))
            .fetch_all(self.database.pool())
            .await?;
        }

        let expired = sqlx::query(
            "SELECT id, temporary_path FROM pending_uploads \
             WHERE state IN ('receiving', 'staged') AND expires_at <= $1",
        )
        .bind(now_millis)
        .fetch_all(self.database.pool())
        .await?;
        for row in expired {
            let id: String = row.try_get("id")?;
            let path = PathBuf::from(row.try_get::<String, _>("temporary_path")?);
            self.store.delete_temporary(&path).await?;
            result.expired_uploads += sqlx::query(
                "UPDATE pending_uploads SET state = 'failed' \
                 WHERE id = $1 AND state IN ('receiving', 'staged')",
            )
            .bind(id)
            .execute(self.database.pool())
            .await?
            .rows_affected();
        }

        let quarantine_cutoff = now_millis.saturating_sub(QUARANTINE_MILLIS);
        let mut blob_inventory: HashMap<String, _> = self
            .store
            .blobs()
            .await?
            .into_iter()
            .map(|object| (object.storage_key.clone(), object))
            .collect();
        let quarantined = sqlx::query(
            "SELECT id, storage_key, deleted_at FROM attachment_blobs WHERE quarantine_until <= $1",
        )
        .bind(now_millis)
        .fetch_all(self.database.pool())
        .await?;
        for row in quarantined {
            let id: String = row.try_get("id")?;
            let storage_key: String = row.try_get("storage_key")?;
            let deleted = row.try_get::<Option<i64>, _>("deleted_at")?.is_some();
            blob_inventory.remove(&storage_key);
            // The checks before the write transaction only save work and keep the store out of it: the
            // transaction decides again.
            let references: i64 = sqlx::query_scalar(BLOB_REFERENCE_COUNT)
                .bind(&id)
                .bind(&id)
                .fetch_one(self.database.pool())
                .await?;
            if references > 0 {
                continue;
            }
            // A deleted blob has no object: it went when the blob was deleted (reconcile removed it as untracked).
            if !deleted {
                let recently_published = self
                    .store
                    .blob_modified_at(&storage_key)
                    .await?
                    .is_some_and(|modified_at| modified_at > quarantine_cutoff);
                if recently_published {
                    continue;
                }
            }
            let mut transaction = self.database.immediate_transaction().await?;
            let references: i64 = sqlx::query_scalar(BLOB_REFERENCE_COUNT)
                .bind(&id)
                .bind(&id)
                .fetch_one(&mut *transaction)
                .await?;
            let removed = if references == 0 {
                // An upload that claimed the row meanwhile moved its quarantine, so the row stays.
                sqlx::query(
                    "DELETE FROM attachment_blobs \
                     WHERE id = $1 AND quarantine_until <= $2 AND storage_key = $3",
                )
                .bind(&id)
                .bind(now_millis)
                .bind(&storage_key)
                .execute(&mut *transaction)
                .await?
                .rows_affected()
            } else {
                0
            };
            transaction.commit().await?;
            if removed == 0 {
                continue;
            }
            result.deleted_blobs += removed;
            if !deleted {
                // Only after the row is gone, and outside the transaction. An upload of the same bytes on another
                // server may already have a new row for this key: in S3 a release only refreshes the object, and
                // without S3 there is one server, where the operation lock keeps uploads out of a reconcile.
                self.store.release(&storage_key).await?;
            }
        }

        for object in blob_inventory.into_values() {
            let untracked_cutoff =
                now_millis.saturating_sub(QUARANTINE_MILLIS.max(object.keep_untracked_millis));
            if object.modified_at_millis > untracked_cutoff {
                continue;
            }
            const TRACKED: &str = "SELECT COUNT(*) FROM attachment_blobs WHERE storage_key = $1";
            let tracked: i64 = sqlx::query_scalar(TRACKED)
                .bind(&object.storage_key)
                .fetch_one(self.database.pool())
                .await?;
            if tracked > 0 {
                continue;
            }
            let recently_published = self
                .store
                .blob_modified_at(&object.storage_key)
                .await?
                .is_some_and(|modified_at| modified_at > untracked_cutoff);
            if recently_published {
                continue;
            }
            let mut transaction = self.database.immediate_transaction().await?;
            let tracked: i64 = sqlx::query_scalar(TRACKED)
                .bind(&object.storage_key)
                .fetch_one(&mut *transaction)
                .await?;
            if tracked == 0 {
                // The one store call inside a write transaction. An upload commits its blob row (a serialized
                // write) before it writes or refreshes the object. With the write lock held from the check above
                // to the end of the delete, no upload on any server can adopt this object in between; a delete
                // after the lock is released could remove an object that an upload just adopted.
                self.store.delete(&object.storage_key).await?;
                result.deleted_untracked_files += 1;
            }
            transaction.commit().await?;
        }
        for object in self.store.temporary_files().await? {
            if object.modified_at_millis <= quarantine_cutoff {
                self.store.delete_temporary(&object.path).await?;
            }
        }

        Ok(result)
    }

    pub fn download_metadata(media_type: &str, display_name: &str) -> DownloadMetadata {
        // Sniffing keeps unknown bytes as `application/octet-stream`, and `nosniff` then stops a
        // `<video>` or `<audio>` from playing. The stored name's extension picks a player type.
        // PDF stays `application/pdf` and an attachment: the editor shows it from a blob, not this URL.
        let content_type = presented_media_type(media_type, display_name);
        let disposition = if serves_inline(&content_type) {
            ContentDisposition::Inline
        } else {
            ContentDisposition::Attachment
        };
        let filename = header_filename(display_name);
        let kind = match disposition {
            ContentDisposition::Inline => "inline",
            ContentDisposition::Attachment => "attachment",
        };
        DownloadMetadata {
            content_type,
            disposition,
            content_disposition: format!("{kind}; filename=\"{filename}\""),
            x_content_type_options: "nosniff",
        }
    }

    pub async fn download<F, Fut>(&self, authorize: F) -> Result<BlobDownload, UploadError>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<AuthorizedAttachment, UploadError>>,
    {
        let attachment = authorize().await?;
        if attachment.deleted {
            return Ok(BlobDownload {
                reader: Box::pin(std::io::Cursor::new(DELETED_FILE_SVG.as_bytes())),
                metadata: DownloadMetadata {
                    content_type: "image/svg+xml".to_owned(),
                    disposition: ContentDisposition::Inline,
                    content_disposition: "inline; filename=\"deleted-file.svg\"".to_owned(),
                    x_content_type_options: "nosniff",
                },
                deleted: true,
            });
        }
        let metadata = Self::download_metadata(&attachment.media_type, &attachment.display_name);
        let reader = self.store.open(&attachment.storage_key).await?;
        Ok(BlobDownload {
            reader,
            metadata,
            deleted: false,
        })
    }
}

fn safe_display_name(value: &str) -> Result<String, UploadError> {
    if value.chars().any(char::is_control) {
        return Err(UploadError::InvalidDisplayName);
    }
    let name = value.rsplit(['/', '\\']).next().unwrap_or_default().trim();
    if name.is_empty() || name.len() > 255 {
        return Err(UploadError::InvalidDisplayName);
    }
    Ok(name.to_owned())
}

/// The staged file as a smaller WebP, when it is a large JPEG, PNG or WebP that [`shrink_image`] can make smaller.
async fn shrink_staged(
    path: &std::path::Path,
    media_type: &str,
    size_bytes: u64,
) -> Result<Option<Vec<u8>>, UploadError> {
    if size_bytes < SHRINK_MIN_BYTES
        || !matches!(media_type, "image/jpeg" | "image/png" | "image/webp")
    {
        return Ok(None);
    }
    let bytes = fs::read(path).await.map_err(|source| BlobStoreError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let _permit = SHRINKING.acquire().await;
    Ok(tokio::task::spawn_blocking(move || shrink_image(&bytes))
        .await
        .ok()
        .flatten())
}

/// `photo.png` -> `photo.webp`. Keeps the name when the result would be longer than a display name may be.
fn with_extension(name: &str, extension: &str) -> String {
    let stem = match name.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem,
        _ => name,
    };
    let renamed = format!("{stem}.{extension}");
    if renamed.len() > 255 {
        name.to_owned()
    } else {
        renamed
    }
}

fn header_filename(value: &str) -> String {
    value
        .chars()
        .take(255)
        .map(|character| match character {
            '"' | '\\' => '_',
            character if character.is_control() => '_',
            character if character.is_ascii() => character,
            _ => '_',
        })
        .collect()
}

/// `octet-stream` plus a known extension becomes a player type. Any other stored type is kept
/// (a sniffed `text/html` must not become `text/plain` because the name says `.txt`).
fn presented_media_type(media_type: &str, display_name: &str) -> String {
    if media_type != "application/octet-stream" {
        return media_type.to_owned();
    }
    let Some(extension) = file_extension(display_name) else {
        return media_type.to_owned();
    };
    match extension.as_str() {
        "mp4" | "m4v" => "video/mp4",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        "ogv" => "video/ogg",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" | "oga" => "audio/ogg",
        "m4a" | "aac" => "audio/mp4",
        "flac" => "audio/flac",
        "opus" => "audio/opus",
        "txt" | "md" | "markdown" | "csv" | "json" | "log" | "yaml" | "yml" | "tsv" | "xml" => {
            "text/plain; charset=utf-8"
        }
        _ => return media_type.to_owned(),
    }
    .to_owned()
}

fn file_extension(name: &str) -> Option<String> {
    let extension = name.rsplit_once('.')?.1;
    if extension.is_empty()
        || extension.len() > 8
        || !extension.bytes().all(|byte| byte.is_ascii_alphanumeric())
    {
        return None;
    }
    Some(extension.to_ascii_lowercase())
}

/// Images, video, audio and plain text can be shown in the page. HTML, SVG and PDF cannot.
fn serves_inline(media_type: &str) -> bool {
    INLINE_IMAGE_TYPES.contains(&media_type)
        || media_type.starts_with("video/")
        || media_type.starts_with("audio/")
        || media_type.starts_with("text/plain")
}

pub(super) fn detect_media_type(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        "image/jpeg"
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        "image/gif"
    } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        "image/webp"
    } else if is_avif(bytes) {
        "image/avif"
    } else if bytes.starts_with(b"%PDF-") {
        "application/pdf"
    } else {
        let text = String::from_utf8_lossy(bytes);
        let normalized = text.trim_start().to_ascii_lowercase();
        if normalized.starts_with("<svg")
            || (normalized.starts_with("<?xml") && normalized.contains("<svg"))
        {
            "image/svg+xml"
        } else if normalized.starts_with("<!doctype html") || normalized.starts_with("<html") {
            "text/html"
        } else {
            "application/octet-stream"
        }
    }
}

/// An ISO-BMFF `ftyp` box whose major or a compatible brand is `avif`/`avis`.
fn is_avif(bytes: &[u8]) -> bool {
    if bytes.len() < 16 || &bytes[4..8] != b"ftyp" {
        return false;
    }
    let box_size = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
    let end = box_size.clamp(16, 256).min(bytes.len());
    let brand = |offset: usize| matches!(&bytes[offset..offset + 4], b"avif" | b"avis");
    brand(8) || (16..end.saturating_sub(3)).step_by(4).any(brand)
}

struct TemporaryFile {
    store: Arc<dyn BlobStore>,
    path: Option<PathBuf>,
}

impl TemporaryFile {
    fn new(store: Arc<dyn BlobStore>, path: PathBuf) -> Self {
        Self {
            store,
            path: Some(path),
        }
    }

    fn disarm(&mut self) {
        self.path = None;
    }

    async fn cleanup(&mut self) {
        if let Some(path) = self.path.clone()
            && self.store.delete_temporary(&path).await.is_ok()
        {
            self.path = None;
        }
    }
}

impl Drop for TemporaryFile {
    fn drop(&mut self) {
        let Some(path) = self.path.take() else {
            return;
        };
        let store = self.store.clone();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                let _ = store.delete_temporary(&path).await;
            });
        }
    }
}
