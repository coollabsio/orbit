mod local;
mod upload;

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;

use thiserror::Error;
use tokio::io::AsyncRead;

pub use local::LocalBlobStore;
pub use upload::{
    AuthorizedAttachment, BlobDownload, ContentDisposition, DownloadMetadata, FinalizedBlob,
    NewAttachmentReference, ReconcileResult, StagedUpload, UploadError, UploadLimitError,
    UploadLimits, UploadService,
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
    pub path: PathBuf,
    pub modified_at_millis: i64,
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
}

pub trait BlobStore: Send + Sync {
    fn create_temporary(&self) -> Result<PathBuf, BlobStoreError>;

    fn install<'a>(&'a self, upload: &'a StagedUpload) -> BlobFuture<'a, StoredObject>;

    fn open<'a>(&'a self, storage_key: &'a str) -> BlobFuture<'a, BlobReader>;

    fn delete<'a>(&'a self, storage_key: &'a str) -> BlobFuture<'a, ()>;

    fn delete_temporary<'a>(&'a self, path: &'a Path) -> BlobFuture<'a, ()>;

    fn blob_modified_at<'a>(&'a self, storage_key: &'a str) -> BlobFuture<'a, Option<i64>>;

    fn blobs(&self) -> BlobFuture<'_, Vec<BlobObject>>;

    fn temporary_files(&self) -> BlobFuture<'_, Vec<BlobObject>>;
}
