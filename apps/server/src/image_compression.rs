//! Makes large stored images smaller, the same way the web client does before an upload (long edge at most
//! 2560 px, lossy WebP). The running server does it from Admin → Storage, for files on the local disk and in S3;
//! `orbit attachments compress` does it offline for local files. Each blob keeps its id
//! ([`UploadService::replace_blob`]), so tasks, pages and chat messages keep their references.

use std::io::Cursor;
use std::sync::{Arc, Mutex, PoisonError};

use image::imageops::FilterType;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader};
use orbit_platform::{Database, Row, UploadError, UploadService};
use serde::Serialize;
use utoipa::ToSchema;

/// The same limits as the web client (`apps/web/src/lib/shrinkImage.ts`).
pub const MAX_IMAGE_EDGE: u32 = 2560;
pub const SHRINK_MIN_BYTES: i64 = 512 * 1024;
const QUALITY: f32 = 85.0;

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

/// Re-encodes `bytes` as a WebP of at most `MAX_IMAGE_EDGE` px. `None` when it is not a still JPEG, PNG or WebP,
/// cannot be decoded, or would not get smaller.
fn compress(bytes: &[u8]) -> Option<Vec<u8>> {
    let format = image::guess_format(bytes).ok()?;
    if !matches!(
        format,
        ImageFormat::Jpeg | ImageFormat::Png | ImageFormat::WebP
    ) || is_animated(bytes)
    {
        return None;
    }
    let mut decoder = ImageReader::with_format(Cursor::new(bytes), format)
        .into_decoder()
        .ok()?;
    // Browsers show a photo turned as its EXIF orientation says; the WebP has no EXIF, so turn the pixels.
    let orientation = decoder.orientation().ok()?;
    let mut image = DynamicImage::from_decoder(decoder).ok()?;
    image.apply_orientation(orientation);
    let fits = image.width().max(image.height()) <= MAX_IMAGE_EDGE;
    // A WebP that fits is already what this produces: encoding it again only loses quality.
    if fits && format == ImageFormat::WebP {
        return None;
    }
    if !fits {
        image = image.resize(MAX_IMAGE_EDGE, MAX_IMAGE_EDGE, FilterType::Lanczos3);
    }
    let rgba = image.to_rgba8();
    let encoded = webp::Encoder::from_rgba(&rgba, rgba.width(), rgba.height()).encode(QUALITY);
    (encoded.len() < bytes.len()).then(|| encoded.to_vec())
}

/// Animated PNG (an `acTL` chunk) and animated WebP (an `ANIM` chunk) would lose all frames but the first. Both
/// chunks come before the image data, near the start of the file.
fn is_animated(bytes: &[u8]) -> bool {
    let header = &bytes[..bytes.len().min(4096)];
    header
        .windows(4)
        .any(|chunk| chunk == b"acTL" || chunk == b"ANIM")
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
        "SELECT id, storage_key FROM attachment_blobs WHERE byte_size >= $1 ORDER BY created_at, id",
    )
    .bind(SHRINK_MIN_BYTES)
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
        let compressed = tokio::task::spawn_blocking(move || compress(&original))
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

/// Runs one compression at a time in the serving process, next to uploads, backups and the S3 mover.
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
            let result = compress_images(
                &compressor.database,
                &compressor.uploads,
                false,
                |summary| compressor.lock().summary = summary.clone(),
            )
            .await;
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

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgba};

    fn noisy_png(width: u32, height: u32) -> Vec<u8> {
        let image = ImageBuffer::from_fn(width, height, |x, y| {
            Rgba([
                (x * 7 % 256) as u8,
                (y * 13 % 256) as u8,
                ((x ^ y) % 256) as u8,
                255,
            ])
        });
        let mut bytes = Vec::new();
        DynamicImage::ImageRgba8(image)
            .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
            .unwrap();
        bytes
    }

    #[test]
    fn compress_scales_large_images_to_webp() {
        let compressed = compress(&noisy_png(3000, 1500)).unwrap();
        let image = image::load_from_memory(&compressed).unwrap();
        assert_eq!((image.width(), image.height()), (2560, 1280));
        assert_eq!(image::guess_format(&compressed).unwrap(), ImageFormat::WebP);
        // A second run leaves the result alone.
        assert!(compress(&compressed).is_none());
    }

    #[test]
    fn compress_skips_animations_and_other_formats() {
        let mut animated = noisy_png(64, 64);
        animated.splice(
            33..33,
            b"\0\0\0\x08acTL\0\0\0\x02\0\0\0\0\0\0\0\0".iter().copied(),
        );
        assert!(compress(&animated).is_none());
        assert!(compress(b"GIF89a not really").is_none());
        assert!(compress(b"%PDF-1.7").is_none());
    }
}
