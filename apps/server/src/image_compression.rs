//! Makes large stored images smaller with [`shrink_image`], as uploads are now (long edge at most 2560 px, lossy
//! WebP). The running server does it from Admin → Storage, for files on the local disk and in S3;
//! `orbit attachments compress` does it offline for local files. Each blob keeps its id
//! ([`UploadService::replace_blob`]), so tasks, pages and chat messages keep their references.

use std::sync::{Arc, Mutex, PoisonError};

use orbit_platform::{Database, Row, SHRINK_MIN_BYTES, UploadError, UploadService, shrink_image};
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, ToSchema)]
pub struct CompressSummary {
    pub compressed: u64,
    pub bytes_before: u64,
    pub bytes_after: u64,
    /// Not a still JPEG, PNG or WebP, not readable, or not smaller after re-encoding.
    pub skipped: u64,
}

impl CompressSummary {
    #[must_use]
    pub fn report(&self, dry_run: bool) -> String {
        let verb = if dry_run {
            "would compress"
        } else {
            "compressed"
        };
        format!(
            "{verb} {} image(s): {} -> {}; skipped {}",
            self.compressed,
            megabytes(self.bytes_before),
            megabytes(self.bytes_after),
            self.skipped
        )
    }
}

fn megabytes(bytes: u64) -> String {
    #[allow(clippy::cast_precision_loss)]
    let value = bytes as f64 / (1024.0 * 1024.0);
    format!("{value:.1} MB")
}

/// Compresses every large image blob. With `dry_run`, only reports what would change. `progress` gets the
/// running totals after each blob.
pub async fn compress_images(
    database: &Database,
    uploads: &UploadService,
    dry_run: bool,
    progress: impl Fn(&CompressSummary),
) -> Result<CompressSummary, UploadError> {
    let blobs = sqlx::query(
        "SELECT id, storage_key FROM attachment_blobs WHERE byte_size >= $1 AND deleted_at IS NULL ORDER BY created_at, id",
    )
    .bind(i64::try_from(SHRINK_MIN_BYTES).unwrap_or(i64::MAX))
    .fetch_all(database.pool())
    .await?;
    let mut summary = CompressSummary::default();
    for blob in blobs {
        let id: String = blob.try_get("id")?;
        let storage_key: String = blob.try_get("storage_key")?;
        // Unreadable (for example, an S3 file while the command runs offline): skip it, do not stop.
        let original = match uploads.read_blob(&storage_key).await {
            Ok(bytes) => bytes,
            Err(error) => {
                tracing::debug!(%error, storage_key, "image compression skipped an unreadable blob");
                summary.skipped += 1;
                progress(&summary);
                continue;
            }
        };
        let original_size = original.len() as u64;
        let compressed = tokio::task::spawn_blocking(move || shrink_image(&original))
            .await
            .ok()
            .flatten();
        let changed = match &compressed {
            Some(_) if dry_run => true,
            Some(bytes) => {
                uploads
                    .replace_blob(&id, &storage_key, bytes, "webp")
                    .await?
            }
            None => false,
        };
        match compressed.filter(|_| changed) {
            Some(bytes) => {
                summary.compressed += 1;
                summary.bytes_before += original_size;
                summary.bytes_after += bytes.len() as u64;
            }
            None => summary.skipped += 1,
        }
        progress(&summary);
    }
    Ok(summary)
}

/// The state of the last compression started from Admin → Storage.
#[derive(Clone, Debug, Default, Serialize, ToSchema)]
pub struct CompressionStatus {
    pub running: bool,
    #[serde(flatten)]
    pub summary: CompressSummary,
    /// Why the last run stopped early.
    pub error: Option<String>,
}

/// Runs one compression at a time among the servers of the database, next to uploads, backups and the S3 mover.
/// The status is the one of this server's last run.
#[derive(Clone)]
pub struct ImageCompressor {
    database: Database,
    uploads: UploadService,
    status: Arc<Mutex<CompressionStatus>>,
}

impl ImageCompressor {
    #[must_use]
    pub fn new(database: Database, uploads: UploadService) -> Self {
        Self {
            database,
            uploads,
            status: Arc::default(),
        }
    }

    #[must_use]
    pub fn status(&self) -> CompressionStatus {
        self.lock().clone()
    }

    /// Starts a run in the background; does nothing while one is running.
    pub fn start(&self) {
        {
            let mut status = self.lock();
            if status.running {
                return;
            }
            *status = CompressionStatus {
                running: true,
                ..CompressionStatus::default()
            };
        }
        let compressor = self.clone();
        tokio::spawn(async move {
            let result = match compressor.database.try_singleton("image-compression").await {
                Ok(Some(_one)) => compress_images(
                    &compressor.database,
                    &compressor.uploads,
                    false,
                    |summary| compressor.lock().summary = summary.clone(),
                )
                .await
                .map_err(|error| error.to_string()),
                Ok(None) => Err("Another server is compressing images.".to_owned()),
                Err(error) => Err(error.to_string()),
            };
            let mut status = compressor.lock();
            status.running = false;
            match result {
                Ok(summary) => {
                    tracing::info!(report = summary.report(false), "image compression finished");
                    status.summary = summary;
                }
                Err(error) => {
                    tracing::warn!(%error, "image compression stopped");
                    status.error = Some(error.to_string());
                }
            }
        });
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, CompressionStatus> {
        self.status.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
