use std::collections::HashSet;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use sha2::{Digest, Sha256};
use sqlx::{Row, SqliteConnection};
use thiserror::Error;
use tokio::fs;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};

use super::{BlobReader, BlobStore, BlobStoreError};
use crate::{AttachmentMutationCoordinator, Database, Id};

const HOUR_MILLIS: i64 = 60 * 60 * 1000;
const QUARANTINE_MILLIS: i64 = 24 * HOUR_MILLIS;
const SNIFF_BYTES: usize = 8 * 1024;

#[derive(Clone, Copy)]
enum ReferenceKey {
    BlobId,
    StorageKey,
}

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
    pub temporary_path: PathBuf,
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
}

pub struct BlobDownload {
    pub reader: BlobReader,
    pub metadata: DownloadMetadata,
}

pub type FinalizeFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, UploadError>> + Send + 'a>>;

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
    #[error("attachment display name is empty, too long, or contains control characters")]
    InvalidDisplayName,
    #[error("staged upload is not in a finalizable state")]
    InvalidState,
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
    operations: Arc<tokio::sync::Mutex<()>>,
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
            operations: Arc::new(tokio::sync::Mutex::new(())),
        }
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

    pub async fn stage_in_request<R>(
        &self,
        workspace_id: Id,
        owner_id: Id,
        display_name: &str,
        request_bytes_received: u64,
        mut reader: R,
    ) -> Result<StagedUpload, UploadError>
    where
        R: AsyncRead + Unpin,
    {
        let display_name = safe_display_name(display_name)?;
        if request_bytes_received >= self.limits.max_request_bytes {
            return Err(UploadError::RequestTooLarge {
                limit: self.limits.max_request_bytes,
            });
        }

        let _mutation = self.mutations.begin().await;
        let temporary_path = self.store.create_temporary().await?;
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

            let id = Id::new_v7();
            let now = self.database.database_now().await?.as_millis();
            let sha256 = format!("{:x}", digest.finalize());
            let detected_media_type = detect_media_type(&sniffed).to_owned();
            let size_i64 = i64::try_from(size_bytes).map_err(|_| UploadError::SizeOverflow)?;
            sqlx::query(
                "INSERT INTO pending_uploads (\
                    id, workspace_id, user_id, temporary_path, original_name, media_type,\
                    byte_size, sha256, state, created_at, expires_at\
                 ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'staged', ?, ?)",
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

    pub async fn finalize(&self, upload: &StagedUpload) -> Result<FinalizedBlob, UploadError> {
        let (blob, ()) = self.finalize_with(upload, no_attachment_write).await?;
        Ok(blob)
    }

    pub async fn finalize_with<T, F>(
        &self,
        upload: &StagedUpload,
        write_attachment: F,
    ) -> Result<(FinalizedBlob, T), UploadError>
    where
        F: for<'a> FnOnce(&'a mut SqliteConnection, &'a FinalizedBlob) -> FinalizeFuture<'a, T>,
    {
        let _operation = self.operations.lock().await;
        let _mutation = self.mutations.begin().await;
        let size_i64 = i64::try_from(upload.size_bytes).map_err(|_| UploadError::SizeOverflow)?;
        let valid: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pending_uploads \
             WHERE id = ? AND workspace_id = ? AND user_id = ? AND temporary_path = ? \
               AND original_name = ? AND media_type = ? AND byte_size = ? AND sha256 = ? \
               AND state = 'staged'",
        )
        .bind(upload.id.to_string())
        .bind(upload.workspace_id.to_string())
        .bind(upload.owner_id.to_string())
        .bind(upload.temporary_path.to_string_lossy().as_ref())
        .bind(&upload.display_name)
        .bind(&upload.detected_media_type)
        .bind(size_i64)
        .bind(&upload.sha256)
        .fetch_one(self.database.pool())
        .await?;
        if valid != 1 {
            return Err(UploadError::InvalidState);
        }

        let now = self.database.database_now().await?.as_millis();
        let quarantine_until = now.saturating_add(QUARANTINE_MILLIS);
        let proposed_id = Id::new_v7();
        let mut transaction = self.database.immediate_transaction().await?;
        let stored = self.store.install(upload).await?;

        sqlx::query(
            "INSERT OR IGNORE INTO attachment_blobs (\
                id, workspace_id, sha256, byte_size, storage_key, created_at, quarantine_until\
             ) VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(proposed_id.to_string())
        .bind(upload.workspace_id.to_string())
        .bind(&upload.sha256)
        .bind(size_i64)
        .bind(&stored.storage_key)
        .bind(now)
        .bind(quarantine_until)
        .execute(&mut *transaction)
        .await?;

        let row = sqlx::query(
            "SELECT id, storage_key, quarantine_until FROM attachment_blobs \
             WHERE workspace_id = ? AND sha256 = ? AND byte_size = ?",
        )
        .bind(upload.workspace_id.to_string())
        .bind(&upload.sha256)
        .bind(size_i64)
        .fetch_one(&mut *transaction)
        .await?;
        let id_text: String = row.try_get("id")?;
        let id = id_text.parse().map_err(|_| UploadError::InvalidState)?;
        let storage_key: String = row.try_get("storage_key")?;
        let quarantine_until: i64 = row.try_get("quarantine_until")?;
        let blob = FinalizedBlob {
            id,
            workspace_id: upload.workspace_id,
            sha256: upload.sha256.clone(),
            size_bytes: upload.size_bytes,
            storage_key,
            quarantine_until,
        };

        let attachment = write_attachment(&mut transaction, &blob).await?;

        let updated = sqlx::query(
            "UPDATE pending_uploads SET state = 'complete', completed_at = ? \
             WHERE id = ? AND workspace_id = ? AND user_id = ? AND state = 'staged' \
               AND sha256 = ? AND byte_size = ?",
        )
        .bind(now)
        .bind(upload.id.to_string())
        .bind(upload.workspace_id.to_string())
        .bind(upload.owner_id.to_string())
        .bind(&upload.sha256)
        .bind(size_i64)
        .execute(&mut *transaction)
        .await?;
        if updated.rows_affected() != 1 {
            return Err(UploadError::InvalidState);
        }
        transaction.commit().await?;

        Ok((blob, attachment))
    }

    pub async fn reconcile(&self, now_millis: i64) -> Result<ReconcileResult, UploadError> {
        let _operation = self.operations.lock().await;
        let _mutation = self.mutations.begin().await;
        let mut result = ReconcileResult::default();

        let expired = sqlx::query(
            "SELECT id, temporary_path FROM pending_uploads \
             WHERE state IN ('receiving', 'staged') AND expires_at <= ?",
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
                 WHERE id = ? AND state IN ('receiving', 'staged')",
            )
            .bind(id)
            .execute(self.database.pool())
            .await?
            .rows_affected();
        }

        let reference_sources = self.reference_sources().await?;
        let quarantined =
            sqlx::query("SELECT id, storage_key FROM attachment_blobs WHERE quarantine_until <= ?")
                .bind(now_millis)
                .fetch_all(self.database.pool())
                .await?;
        for row in quarantined {
            let id: String = row.try_get("id")?;
            let storage_key: String = row.try_get("storage_key")?;
            let mut transaction = self.database.immediate_transaction().await?;
            let mut references = 0_i64;
            for (table, column, key) in &reference_sources {
                let query = format!("SELECT COUNT(*) FROM {table} WHERE {column} = ?");
                let value = match key {
                    ReferenceKey::BlobId => &id,
                    ReferenceKey::StorageKey => &storage_key,
                };
                references += sqlx::query_scalar::<_, i64>(&query)
                    .bind(value)
                    .fetch_one(&mut *transaction)
                    .await?;
            }
            if references == 0 {
                self.store.delete(&storage_key).await?;
                result.deleted_blobs += sqlx::query(
                    "DELETE FROM attachment_blobs WHERE id = ? AND quarantine_until <= ?",
                )
                .bind(&id)
                .bind(now_millis)
                .execute(&mut *transaction)
                .await?
                .rows_affected();
            }
            transaction.commit().await?;
        }

        let quarantine_cutoff = now_millis.saturating_sub(QUARANTINE_MILLIS);
        for object in self.store.blobs().await? {
            if object.modified_at_millis > quarantine_cutoff {
                continue;
            }
            let mut transaction = self.database.immediate_transaction().await?;
            let tracked: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM attachment_blobs WHERE storage_key = ?")
                    .bind(&object.storage_key)
                    .fetch_one(&mut *transaction)
                    .await?;
            if tracked == 0 {
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
        let disposition = if matches!(
            media_type,
            "image/jpeg" | "image/png" | "image/gif" | "image/webp"
        ) {
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
            content_type: media_type.to_owned(),
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
        let metadata = Self::download_metadata(&attachment.media_type, &attachment.display_name);
        let reader = self.store.open(&attachment.storage_key).await?;
        Ok(BlobDownload { reader, metadata })
    }

    async fn reference_sources(
        &self,
    ) -> Result<Vec<(&'static str, &'static str, ReferenceKey)>, sqlx::Error> {
        let mut sources = Vec::new();
        for table in ["attachments", "attachment_refs"] {
            let exists: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?",
            )
            .bind(table)
            .fetch_one(self.database.pool())
            .await?;
            if exists == 1 {
                let pragma = format!("PRAGMA table_info({table})");
                let columns = sqlx::query(&pragma).fetch_all(self.database.pool()).await?;
                let names: HashSet<String> = columns
                    .iter()
                    .map(|column| column.get::<String, _>("name"))
                    .collect();
                if names.contains("blob_id") {
                    sources.push((table, "blob_id", ReferenceKey::BlobId));
                } else if names.contains("storage_key") {
                    sources.push((table, "storage_key", ReferenceKey::StorageKey));
                }
            }
        }
        Ok(sources)
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

fn detect_media_type(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        "image/jpeg"
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        "image/gif"
    } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        "image/webp"
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

fn no_attachment_write<'a>(
    _connection: &'a mut SqliteConnection,
    _blob: &'a FinalizedBlob,
) -> FinalizeFuture<'a, ()> {
    Box::pin(async { Ok(()) })
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
