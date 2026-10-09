//! Attachment storage on the local disk and, when the root user turns it on, in an S3 bucket.
//!
//! New blobs go to the active tier; reads try both, so files keep working while the background mover
//! ([`TieredBlobStore::move_blobs`]) moves them from the other tier. Uploads are always staged on the local disk.
//! S3 objects are not in backups, so a released S3 blob is kept for [`S3_DELETE_DELAY_MILLIS`]: restoring any
//! kept backup still finds the files its database references.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError, RwLock};

use tokio::sync::Notify;
use uuid::Uuid;

use super::{
    BlobFuture, BlobObject, BlobReader, BlobStore, BlobStoreError, LocalBlobStore, S3Bucket,
    StagedUpload, StoredObject, blob_storage_key,
};
use crate::AttachmentMutationCoordinator;

/// Longer than backup retention (the last day, 7 daily + 4 weekly snapshots).
pub const S3_DELETE_DELAY_MILLIS: i64 = 40 * 24 * 60 * 60 * 1000;

/// Where attachments and backups are stored. Shared by the blob store and the backup service; saving the
/// storage settings swaps it at run time.
#[derive(Clone, Debug, Default)]
pub struct ObjectStorage {
    state: Arc<RwLock<ObjectStorageState>>,
    changed: Arc<Notify>,
}

#[derive(Clone, Debug, Default)]
pub struct ObjectStorageState {
    /// The saved bucket. It stays readable when neither switch is on, until no blob is left in it.
    pub bucket: Option<S3Bucket>,
    /// New attachment blobs go to S3.
    pub attachments: bool,
    /// New backups are uploaded to S3.
    pub backups: bool,
}

impl ObjectStorage {
    pub fn configure(&self, state: ObjectStorageState) {
        *self.state.write().unwrap_or_else(PoisonError::into_inner) = state;
        self.changed.notify_one();
    }

    #[must_use]
    pub fn current(&self) -> ObjectStorageState {
        self.state
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Waits for the next [`configure`](Self::configure) call (or returns at once if one happened unseen).
    pub async fn changed(&self) {
        self.changed.notified().await;
    }

    #[must_use]
    pub fn bucket(&self) -> Option<S3Bucket> {
        self.current().bucket
    }

    /// The bucket that new backups go to.
    #[must_use]
    pub fn backup_bucket(&self) -> Option<S3Bucket> {
        let state = self.current();
        state.bucket.filter(|_| state.backups)
    }
}

/// Progress of the background move between the local disk and S3.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BlobMoveStatus {
    /// Blobs still waiting to move to the active tier.
    pub remaining: usize,
    /// The error that stopped the last pass; it is retried on the next pass.
    pub error: Option<String>,
}

#[derive(Clone, Debug)]
pub struct TieredBlobStore {
    local: LocalBlobStore,
    storage: ObjectStorage,
    mutations: AttachmentMutationCoordinator,
    status: Arc<Mutex<BlobMoveStatus>>,
}

impl TieredBlobStore {
    #[must_use]
    pub fn new(
        local: LocalBlobStore,
        storage: ObjectStorage,
        mutations: AttachmentMutationCoordinator,
    ) -> Self {
        Self {
            local,
            storage,
            mutations,
            status: Arc::default(),
        }
    }

    #[must_use]
    pub fn status(&self) -> BlobMoveStatus {
        self.status
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// One pass of the background move: blobs on the inactive tier are copied to the active tier and removed
    /// from the inactive one. Returns how many blobs moved.
    pub async fn move_blobs(&self) -> Result<usize, BlobStoreError> {
        let result = self.move_pass().await;
        let mut status = self.status.lock().unwrap_or_else(PoisonError::into_inner);
        status.error = result.as_ref().err().map(ToString::to_string);
        result
    }

    async fn move_pass(&self) -> Result<usize, BlobStoreError> {
        let state = self.storage.current();
        let Some(bucket) = state.bucket else {
            self.set_remaining(0);
            return Ok(0);
        };
        if state.attachments {
            let objects = self.local.blobs().await?;
            self.set_remaining(objects.len());
            for (index, object) in objects.iter().enumerate() {
                let _mutation = self.mutations.begin().await;
                if bucket.last_modified(&object.storage_key).await?.is_none() {
                    match bucket.put_file(&object.storage_key, &object.path).await {
                        // Deleted by reconcile meanwhile.
                        Err(super::S3Error::Io { source, .. })
                            if source.kind() == std::io::ErrorKind::NotFound => {}
                        result => result?,
                    }
                }
                self.local.delete(&object.storage_key).await?;
                self.set_remaining(objects.len() - index - 1);
            }
            Ok(objects.len())
        } else {
            let objects = bucket.list("blobs/").await?;
            self.set_remaining(objects.len());
            for (index, object) in objects.iter().enumerate() {
                let _mutation = self.mutations.begin().await;
                self.download(&bucket, &object.key).await?;
                bucket.delete(&object.key).await?;
                self.set_remaining(objects.len() - index - 1);
            }
            Ok(objects.len())
        }
    }

    async fn download(&self, bucket: &S3Bucket, storage_key: &str) -> Result<(), BlobStoreError> {
        let destination = self.local.path(storage_key)?;
        if tokio::fs::try_exists(&destination)
            .await
            .map_err(|source| io_error(&destination, source))?
        {
            return Ok(());
        }
        let parent = destination
            .parent()
            .expect("blob paths always have a parent");
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|source| io_error(parent, source))?;
        let partial = parent.join(format!(".download-{}", Uuid::now_v7()));
        let result = async {
            if bucket.get_to_file(storage_key, &partial).await? {
                tokio::fs::rename(&partial, &destination)
                    .await
                    .map_err(|source| io_error(&destination, source))?;
            }
            Ok(())
        }
        .await;
        if result.is_err() {
            let _ = tokio::fs::remove_file(&partial).await;
        }
        result
    }

    fn set_remaining(&self, remaining: usize) {
        self.status
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remaining = remaining;
    }

    async fn install_in(
        &self,
        bucket: &S3Bucket,
        upload: &StagedUpload,
    ) -> Result<StoredObject, BlobStoreError> {
        let storage_key = blob_storage_key(upload);
        let copy = self.local.verified_copy(upload).await?;
        // Copying an existing object onto itself refreshes its modified time, like a local deduplication.
        let deduplicated = bucket.touch(&storage_key).await?;
        if !deduplicated {
            bucket.put_file(&storage_key, copy.path()).await?;
        }
        copy.remove().await?;
        self.local.delete_temporary(&upload.temporary_path).await?;
        Ok(StoredObject {
            storage_key,
            deduplicated,
        })
    }
}

impl BlobStore for TieredBlobStore {
    fn create_temporary(&self) -> Result<PathBuf, BlobStoreError> {
        self.local.create_temporary()
    }

    fn install<'a>(&'a self, upload: &'a StagedUpload) -> BlobFuture<'a, StoredObject> {
        Box::pin(async move {
            let state = self.storage.current();
            match state.bucket.filter(|_| state.attachments) {
                Some(bucket) => self.install_in(&bucket, upload).await,
                None => self.local.install(upload).await,
            }
        })
    }

    fn open<'a>(&'a self, storage_key: &'a str) -> BlobFuture<'a, BlobReader> {
        Box::pin(async move {
            let state = self.storage.current();
            let open_local = || async {
                match self.local.open(storage_key).await {
                    Err(BlobStoreError::Io { source, .. })
                        if source.kind() == std::io::ErrorKind::NotFound =>
                    {
                        Ok(None)
                    }
                    result => result.map(Some),
                }
            };
            let reader = match &state.bucket {
                None => open_local().await?,
                Some(bucket) if state.attachments => match bucket.get(storage_key).await? {
                    Some(reader) => Some(reader),
                    None => open_local().await?,
                },
                Some(bucket) => match open_local().await? {
                    Some(reader) => Some(reader),
                    None => bucket.get(storage_key).await?,
                },
            };
            reader.ok_or_else(|| {
                io_error(
                    Path::new(storage_key),
                    std::io::Error::from(std::io::ErrorKind::NotFound),
                )
            })
        })
    }

    fn delete<'a>(&'a self, storage_key: &'a str) -> BlobFuture<'a, ()> {
        Box::pin(async move {
            self.local.delete(storage_key).await?;
            if let Some(bucket) = self.storage.bucket() {
                bucket.delete(storage_key).await?;
            }
            Ok(())
        })
    }

    fn release<'a>(&'a self, storage_key: &'a str) -> BlobFuture<'a, ()> {
        Box::pin(async move {
            self.local.delete(storage_key).await?;
            if let Some(bucket) = self.storage.bucket() {
                // Restarts the delete delay; reconcile deletes the untracked object after it.
                bucket.touch(storage_key).await?;
            }
            Ok(())
        })
    }

    fn delete_temporary<'a>(&'a self, path: &'a Path) -> BlobFuture<'a, ()> {
        self.local.delete_temporary(path)
    }

    fn blob_modified_at<'a>(&'a self, storage_key: &'a str) -> BlobFuture<'a, Option<i64>> {
        Box::pin(async move {
            let local = self.local.blob_modified_at(storage_key).await?;
            let remote = match self.storage.bucket() {
                Some(bucket) => bucket.last_modified(storage_key).await?,
                None => None,
            };
            Ok(local.max(remote))
        })
    }

    fn blobs(&self) -> BlobFuture<'_, Vec<BlobObject>> {
        Box::pin(async move {
            let mut objects: HashMap<String, BlobObject> = self
                .local
                .blobs()
                .await?
                .into_iter()
                .map(|object| (object.storage_key.clone(), object))
                .collect();
            if let Some(bucket) = self.storage.bucket() {
                for object in bucket.list("blobs/").await? {
                    objects
                        .entry(object.key.clone())
                        .and_modify(|local| {
                            local.modified_at_millis =
                                local.modified_at_millis.max(object.last_modified_millis);
                            local.keep_untracked_millis = S3_DELETE_DELAY_MILLIS;
                        })
                        .or_insert(BlobObject {
                            storage_key: object.key,
                            path: PathBuf::new(),
                            modified_at_millis: object.last_modified_millis,
                            keep_untracked_millis: S3_DELETE_DELAY_MILLIS,
                        });
                }
            }
            let mut objects: Vec<_> = objects.into_values().collect();
            objects.sort_by(|left, right| left.storage_key.cmp(&right.storage_key));
            Ok(objects)
        })
    }

    fn temporary_files(&self) -> BlobFuture<'_, Vec<BlobObject>> {
        BlobStore::temporary_files(&self.local)
    }
}

fn io_error(path: &Path, source: std::io::Error) -> BlobStoreError {
    BlobStoreError::Io {
        path: path.to_owned(),
        source,
    }
}
