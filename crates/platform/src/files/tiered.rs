//! Attachment storage on the local disk and, when the root user turns it on, in an S3 bucket. A workspace can
//! connect its own bucket: its blobs (`blobs/{workspace_id}/…`) then go there instead.
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
    /// The buckets that workspaces connected for their own attachment files, by workspace id.
    pub workspace_buckets: HashMap<String, WorkspaceBucket>,
}

#[derive(Clone, Debug)]
pub struct WorkspaceBucket {
    pub bucket: S3Bucket,
    /// New blobs of the workspace go to the bucket and its other blobs move there. Off: the workspace removed
    /// the bucket, and its blobs move back out of it.
    pub active: bool,
}

/// A place where a blob can be.
#[derive(Clone, Copy)]
enum Tier<'a> {
    Local,
    Bucket(&'a S3Bucket),
}

impl ObjectStorageState {
    /// The bucket that the workspace of `storage_key` connected.
    fn workspace_bucket(&self, storage_key: &str) -> Option<&WorkspaceBucket> {
        let (workspace_id, _) = storage_key.strip_prefix("blobs/")?.split_once('/')?;
        self.workspace_buckets.get(workspace_id)
    }

    fn in_active_workspace_bucket(&self, storage_key: &str) -> bool {
        self.workspace_bucket(storage_key)
            .is_some_and(|own| own.active)
    }

    /// Where the blob can be, the tier that new blobs go to first.
    fn tiers(&self, storage_key: &str) -> Vec<Tier<'_>> {
        let mut tiers = match &self.bucket {
            None => vec![Tier::Local],
            Some(bucket) if self.attachments => vec![Tier::Bucket(bucket), Tier::Local],
            Some(bucket) => vec![Tier::Local, Tier::Bucket(bucket)],
        };
        match self.workspace_bucket(storage_key) {
            Some(own) if own.active => tiers.insert(0, Tier::Bucket(&own.bucket)),
            Some(own) => tiers.push(Tier::Bucket(&own.bucket)),
            None => {}
        }
        tiers
    }
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

#[derive(Clone, Debug, Default)]
struct MoveStatuses {
    installation: BlobMoveStatus,
    workspaces: HashMap<String, WorkspaceMove>,
}

/// The move of one workspace's blobs, for the bucket state (connected or removed) it was found for.
#[derive(Clone, Debug)]
struct WorkspaceMove {
    active: bool,
    status: BlobMoveStatus,
}

#[derive(Clone, Debug)]
pub struct TieredBlobStore {
    local: LocalBlobStore,
    storage: ObjectStorage,
    mutations: AttachmentMutationCoordinator,
    status: Arc<Mutex<MoveStatuses>>,
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

    /// The move between this server's disk and the installation's bucket.
    #[must_use]
    pub fn status(&self) -> BlobMoveStatus {
        self.statuses().installation
    }

    /// The move into (or, after it was removed, out of) the workspace's own bucket. `None` until a pass checked
    /// the workspace's files for its current bucket state, so right after a save the files to move are not known
    /// yet.
    #[must_use]
    pub fn workspace_status(&self, workspace_id: &str) -> Option<BlobMoveStatus> {
        let active = self
            .storage
            .current()
            .workspace_buckets
            .get(workspace_id)?
            .active;
        self.statuses()
            .workspaces
            .remove(workspace_id)
            .filter(|found| found.active == active)
            .map(|found| found.status)
    }

    fn update_workspace_status(
        &self,
        workspace_id: &str,
        active: bool,
        update: impl FnOnce(&mut BlobMoveStatus),
    ) {
        let mut statuses = self.lock_status();
        let found = statuses
            .workspaces
            .entry(workspace_id.to_owned())
            .or_insert_with(|| WorkspaceMove {
                active,
                status: BlobMoveStatus::default(),
            });
        if found.active != active {
            *found = WorkspaceMove {
                active,
                status: BlobMoveStatus::default(),
            };
        }
        update(&mut found.status);
    }

    fn statuses(&self) -> MoveStatuses {
        self.status
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// One pass of the background move: blobs on an inactive tier are copied to the active tier and removed
    /// from the inactive one. Each workspace bucket moves on its own, so one that fails does not stop the others.
    /// Returns how many blobs moved; the error is the installation's.
    pub async fn move_blobs(&self) -> Result<usize, BlobStoreError> {
        let state = self.storage.current();
        let mut moved = 0;
        let local = if state.workspace_buckets.values().any(|own| own.active) {
            self.local.blobs().await?
        } else {
            Vec::new()
        };
        for (workspace_id, own) in &state.workspace_buckets {
            let result = self
                .move_workspace_pass(&state, workspace_id, own, &local)
                .await;
            if let Err(error) = &result {
                tracing::warn!(%workspace_id, %error, "moving files of a workspace bucket failed; retrying later");
            }
            moved += result.as_ref().copied().unwrap_or(0);
            self.update_workspace_status(workspace_id, own.active, |status| {
                status.error = result.err().map(|error| error.to_string());
            });
        }
        self.lock_status()
            .workspaces
            .retain(|workspace_id, _| state.workspace_buckets.contains_key(workspace_id));
        let result = self.move_pass(&state).await;
        self.lock_status().installation.error = result.as_ref().err().map(ToString::to_string);
        result.map(|installation| installation + moved)
    }

    fn lock_status(&self) -> std::sync::MutexGuard<'_, MoveStatuses> {
        self.status.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Moves the workspace's blobs into its active bucket, from this server's disk (`local`, all local blobs)
    /// and the installation's bucket; or out of a removed bucket to this server's disk (the installation pass
    /// then moves them on to its bucket).
    async fn move_workspace_pass(
        &self,
        state: &ObjectStorageState,
        workspace_id: &str,
        own: &WorkspaceBucket,
        local: &[BlobObject],
    ) -> Result<usize, BlobStoreError> {
        let prefix = format!("blobs/{workspace_id}/");
        let set_remaining = |remaining| {
            self.update_workspace_status(workspace_id, own.active, |status| {
                status.remaining = remaining;
            });
        };
        if !own.active {
            let objects = own.bucket.list(&prefix).await?;
            set_remaining(objects.len());
            for (index, object) in objects.iter().enumerate() {
                let _mutation = self.mutations.begin().await;
                self.download(&own.bucket, &object.key).await?;
                own.bucket.delete(&object.key).await?;
                set_remaining(objects.len() - index - 1);
            }
            return Ok(objects.len());
        }
        let local: Vec<_> = local
            .iter()
            .filter(|object| object.storage_key.starts_with(&prefix))
            .collect();
        let remote = match &state.bucket {
            Some(bucket) => bucket.list(&prefix).await?,
            None => Vec::new(),
        };
        let total = local.len() + remote.len();
        set_remaining(total);
        for (index, object) in local.iter().enumerate() {
            let _mutation = self.mutations.begin().await;
            if own
                .bucket
                .last_modified(&object.storage_key)
                .await?
                .is_none()
            {
                match own.bucket.put_file(&object.storage_key, &object.path).await {
                    // Deleted by reconcile meanwhile.
                    Err(super::S3Error::Io { source, .. })
                        if source.kind() == std::io::ErrorKind::NotFound => {}
                    result => result?,
                }
            }
            self.local.delete(&object.storage_key).await?;
            set_remaining(total - index - 1);
        }
        if let Some(bucket) = &state.bucket {
            for (index, object) in remote.iter().enumerate() {
                let _mutation = self.mutations.begin().await;
                if own.bucket.last_modified(&object.key).await?.is_none() {
                    self.copy_object(bucket, &own.bucket, &object.key).await?;
                }
                bucket.delete(&object.key).await?;
                set_remaining(remote.len() - index - 1);
            }
        }
        Ok(total)
    }

    /// Copies an object through a temporary file on this server (the buckets can be on different servers).
    async fn copy_object(
        &self,
        from: &S3Bucket,
        to: &S3Bucket,
        storage_key: &str,
    ) -> Result<(), BlobStoreError> {
        let temporary = self.local.create_temporary()?;
        let result = async {
            if from.get_to_file(storage_key, &temporary).await? {
                to.put_file(storage_key, &temporary).await?;
            }
            Ok(())
        }
        .await;
        let _ = tokio::fs::remove_file(&temporary).await;
        result
    }

    /// The installation's own move: blobs of workspaces with an active bucket are left to their pass.
    async fn move_pass(&self, state: &ObjectStorageState) -> Result<usize, BlobStoreError> {
        let Some(bucket) = &state.bucket else {
            self.set_remaining(0);
            return Ok(0);
        };
        let installation = |key: &str| !state.in_active_workspace_bucket(key);
        if state.attachments {
            let mut objects = self.local.blobs().await?;
            objects.retain(|object| installation(&object.storage_key));
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
            let mut objects = bucket.list("blobs/").await?;
            objects.retain(|object| installation(&object.key));
            self.set_remaining(objects.len());
            for (index, object) in objects.iter().enumerate() {
                let _mutation = self.mutations.begin().await;
                self.download(bucket, &object.key).await?;
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
        self.lock_status().installation.remaining = remaining;
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
            let own = state
                .workspace_buckets
                .get(&upload.workspace_id.to_string())
                .filter(|own| own.active);
            match own
                .map(|own| &own.bucket)
                .or(state.bucket.as_ref().filter(|_| state.attachments))
            {
                Some(bucket) => self.install_in(bucket, upload).await,
                None => self.local.install(upload).await,
            }
        })
    }

    fn open<'a>(&'a self, storage_key: &'a str) -> BlobFuture<'a, BlobReader> {
        Box::pin(async move {
            let state = self.storage.current();
            // A tier that fails (a bucket that is down) does not hide a copy on the next one.
            let mut failure = None;
            for tier in state.tiers(storage_key) {
                let reader = match tier {
                    Tier::Local => match self.local.open(storage_key).await {
                        Err(BlobStoreError::Io { source, .. })
                            if source.kind() == std::io::ErrorKind::NotFound =>
                        {
                            Ok(None)
                        }
                        result => result.map(Some),
                    },
                    Tier::Bucket(bucket) => bucket.get(storage_key).await.map_err(Into::into),
                };
                match reader {
                    Ok(Some(reader)) => return Ok(reader),
                    Ok(None) => {}
                    Err(error) => failure = failure.or(Some(error)),
                }
            }
            Err(failure.unwrap_or_else(|| {
                io_error(
                    Path::new(storage_key),
                    std::io::Error::from(std::io::ErrorKind::NotFound),
                )
            }))
        })
    }

    fn delete<'a>(&'a self, storage_key: &'a str) -> BlobFuture<'a, ()> {
        Box::pin(async move {
            let state = self.storage.current();
            for tier in state.tiers(storage_key) {
                match tier {
                    Tier::Local => self.local.delete(storage_key).await?,
                    Tier::Bucket(bucket) => bucket.delete(storage_key).await?,
                }
            }
            Ok(())
        })
    }

    fn release<'a>(&'a self, storage_key: &'a str) -> BlobFuture<'a, ()> {
        Box::pin(async move {
            let state = self.storage.current();
            self.local.delete(storage_key).await?;
            if let Some(bucket) = &state.bucket {
                // Restarts the delete delay; reconcile deletes the untracked object after it.
                bucket.touch(storage_key).await?;
            }
            if let Some(own) = state.workspace_bucket(storage_key) {
                // A workspace bucket that fails (revoked keys) must not stop the cleanup of all other files; the
                // object is untracked now, so reconcile deletes it once the bucket works again.
                if let Err(error) = own.bucket.touch(storage_key).await {
                    tracing::warn!(storage_key, %error, "a released file stays in a workspace bucket");
                }
            }
            Ok(())
        })
    }

    fn delete_temporary<'a>(&'a self, path: &'a Path) -> BlobFuture<'a, ()> {
        self.local.delete_temporary(path)
    }

    fn blob_modified_at<'a>(&'a self, storage_key: &'a str) -> BlobFuture<'a, Option<i64>> {
        Box::pin(async move {
            let state = self.storage.current();
            let local = self.local.blob_modified_at(storage_key).await?;
            let remote = match &state.bucket {
                Some(bucket) => bucket.last_modified(storage_key).await?,
                None => None,
            };
            let own = match state.workspace_bucket(storage_key) {
                Some(own) => own
                    .bucket
                    .last_modified(storage_key)
                    .await
                    .inspect_err(|error| {
                        tracing::warn!(storage_key, %error, "a workspace bucket cannot be read");
                    })
                    .unwrap_or(None),
                None => None,
            };
            Ok(local.max(remote).max(own))
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
            let state = self.storage.current();
            let mut remote = match &state.bucket {
                Some(bucket) => bucket.list("blobs/").await?,
                None => Vec::new(),
            };
            for (workspace_id, own) in &state.workspace_buckets {
                // Skipped while it fails: its objects are then only not deleted yet.
                match own.bucket.list(&format!("blobs/{workspace_id}/")).await {
                    Ok(objects) => remote.extend(objects),
                    Err(error) => {
                        tracing::warn!(%workspace_id, %error, "a workspace bucket cannot be listed");
                    }
                }
            }
            for object in remote {
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
