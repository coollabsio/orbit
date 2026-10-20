mod fake_s3;
mod local;
mod s3;
mod tiered;
mod upload;

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;

use thiserror::Error;
use tokio::io::AsyncRead;

pub use fake_s3::start_fake_s3;
pub use local::LocalBlobStore;
pub use s3::{S3Bucket, S3Config, S3Error, S3Object};
pub use tiered::{
    BlobMoveStatus, ObjectStorage, ObjectStorageState, S3_DELETE_DELAY_MILLIS, TieredBlobStore,
};
pub use upload::{
    AuthorizedAttachment, BLOB_REFERENCE_COUNT, BlobDownload, ContentDisposition, DownloadMetadata,
    FinalizedAttachment, FinalizedBlob, INLINE_IMAGE_TYPES, NewAttachmentReference,
    ReconcileResult, StagedUpload, UploadError, UploadFinalization, UploadLimitError, UploadLimits,
    UploadService, WORKSPACE_STORAGE_USED,
};

pub type BlobFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, BlobStoreError>> + Send + 'a>>;
pub type BlobReader = Pin<Box<dyn AsyncRead + Send>>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredObject {
    pub storage_key: String,
    pub deduplicated: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlobObject {
    pub storage_key: String,
    /// Local file path; empty for objects in S3.
    pub path: PathBuf,
    pub modified_at_millis: i64,
    /// Reconcile deletes an object without a database record only after it stayed unchanged this long (at least
    /// the upload quarantine). S3 objects are kept longer than any backup, so restoring a backup finds its files.
    pub keep_untracked_millis: i64,
}

#[derive(Debug, Error)]
pub enum BlobStoreError {
    #[error("blob storage I/O failed at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("unsafe blob storage key {0}")]
    UnsafeStorageKey(String),
    #[error("temporary path is outside the blob store: {0}")]
    UnsafeTemporaryPath(PathBuf),
    #[error("staged upload contents changed after validation")]
    StagedContentChanged,
    #[error(transparent)]
    ObjectStorage(#[from] S3Error),
}

/// Blobs are deduplicated per workspace by content.
fn blob_storage_key(upload: &StagedUpload) -> String {
    format!(
        "blobs/{}/{}-{}",
        upload.workspace_id, upload.sha256, upload.size_bytes
    )
}

pub trait BlobStore: Send + Sync {
    fn create_temporary(&self) -> Result<PathBuf, BlobStoreError>;

    fn install<'a>(&'a self, upload: &'a StagedUpload) -> BlobFuture<'a, StoredObject>;

    fn open<'a>(&'a self, storage_key: &'a str) -> BlobFuture<'a, BlobReader>;

    /// Deletes the blob now.
    fn delete<'a>(&'a self, storage_key: &'a str) -> BlobFuture<'a, ()>;

    /// The database no longer references the blob. A store whose blobs are not in backups keeps it for a while
    /// (reconcile deletes it later as an untracked object), so restoring an older backup still finds it.
    fn release<'a>(&'a self, storage_key: &'a str) -> BlobFuture<'a, ()> {
        self.delete(storage_key)
    }

    fn delete_temporary<'a>(&'a self, path: &'a Path) -> BlobFuture<'a, ()>;

    fn blob_modified_at<'a>(&'a self, storage_key: &'a str) -> BlobFuture<'a, Option<i64>>;

    fn blobs(&self) -> BlobFuture<'_, Vec<BlobObject>>;

    fn temporary_files(&self) -> BlobFuture<'_, Vec<BlobObject>>;
}
