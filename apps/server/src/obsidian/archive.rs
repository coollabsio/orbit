//! Where an uploaded vault waits for its jobs: like a workspace export, in the workspace's own
//! bucket, the installation bucket (when attachments go there), or the server's disk, so any
//! server of the cluster can run the scan and the import.

use std::path::{Path, PathBuf};

use orbit_platform::{Id, ObjectStorage, S3Bucket};

/// Where an archive is stored (`obsidian_imports.archive_location`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Location {
    Local,
    Bucket,
    WorkspaceBucket,
}

impl Location {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Bucket => "bucket",
            Self::WorkspaceBucket => "workspace_bucket",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "local" => Some(Self::Local),
            "bucket" => Some(Self::Bucket),
            "workspace_bucket" => Some(Self::WorkspaceBucket),
            _ => None,
        }
    }
}

/// An archive opened for a job: the stored file itself (disk) or a downloaded copy, which is
/// deleted when this is dropped.
pub struct LocalArchive {
    pub path: PathBuf,
    temporary: bool,
}

impl Drop for LocalArchive {
    fn drop(&mut self) {
        if self.temporary {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

#[derive(Clone)]
pub struct ArchiveStore {
    storage: ObjectStorage,
    /// Holds `imports/…` for archives on the server's disk.
    local_root: PathBuf,
}

fn key(workspace_id: Id, import_id: Id) -> String {
    format!("imports/{workspace_id}/{import_id}.zip")
}

impl ArchiveStore {
    pub fn new(storage: ObjectStorage, local_root: impl Into<PathBuf>) -> Self {
        Self {
            storage,
            local_root: local_root.into(),
        }
    }

    /// A path on this server's disk for an upload in progress.
    pub async fn partial_path(&self, workspace_id: Id, import_id: Id) -> std::io::Result<PathBuf> {
        let path = self
            .local_root
            .join(format!("{}.partial", key(workspace_id, import_id)));
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        Ok(path)
    }

    /// Moves a finished upload to the tier that new attachment blobs of the workspace go to.
    pub async fn store(
        &self,
        workspace_id: Id,
        import_id: Id,
        file: &Path,
    ) -> Result<Location, String> {
        let key = key(workspace_id, import_id);
        let state = self.storage.current();
        let (location, bucket) = match state
            .workspace_buckets
            .get(&workspace_id.to_string())
            .filter(|own| own.active)
        {
            Some(own) => (Location::WorkspaceBucket, Some(own.bucket.clone())),
            None => match state.bucket.filter(|_| state.attachments) {
                Some(bucket) => (Location::Bucket, Some(bucket)),
                None => (Location::Local, None),
            },
        };
        match bucket {
            Some(bucket) => {
                let stored = bucket.put_file(&key, file).await;
                let _ = tokio::fs::remove_file(file).await;
                stored.map_err(|error| error.to_string())?;
            }
            None => tokio::fs::rename(file, self.local_root.join(&key))
                .await
                .map_err(|error| error.to_string())?,
        }
        Ok(location)
    }

    fn bucket(&self, workspace_id: Id, location: Location) -> Option<S3Bucket> {
        let state = self.storage.current();
        match location {
            Location::Local => None,
            Location::Bucket => state.bucket,
            Location::WorkspaceBucket => state
                .workspace_buckets
                .get(&workspace_id.to_string())
                .map(|own| own.bucket.clone()),
        }
    }

    /// The archive as a local file, or `None` when it is gone.
    pub async fn open(
        &self,
        workspace_id: Id,
        import_id: Id,
        location: Location,
    ) -> Result<Option<LocalArchive>, String> {
        let key = key(workspace_id, import_id);
        if location == Location::Local {
            let path = self.local_root.join(&key);
            return Ok(tokio::fs::try_exists(&path)
                .await
                .unwrap_or(false)
                .then_some(LocalArchive {
                    path,
                    temporary: false,
                }));
        }
        let Some(bucket) = self.bucket(workspace_id, location) else {
            return Ok(None);
        };
        let path = self
            .local_root
            .join("imports/tmp")
            .join(format!("{import_id}-{}.zip", Id::new_v7()));
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|error| error.to_string())?;
        }
        let copy = LocalArchive {
            path,
            temporary: true,
        };
        let found = bucket
            .get_to_file(&key, &copy.path)
            .await
            .map_err(|error| error.to_string())?;
        Ok(found.then_some(copy))
    }

    pub async fn delete(
        &self,
        workspace_id: Id,
        import_id: Id,
        location: Location,
    ) -> Result<(), String> {
        let key = key(workspace_id, import_id);
        if location == Location::Local {
            return match tokio::fs::remove_file(self.local_root.join(key)).await {
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                    Err(error.to_string())
                }
                _ => Ok(()),
            };
        }
        match self.bucket(workspace_id, location) {
            Some(bucket) => bucket.delete(&key).await.map_err(|error| error.to_string()),
            // The bucket was removed from the settings; its objects are out of Orbit's reach.
            None => Ok(()),
        }
    }
}
