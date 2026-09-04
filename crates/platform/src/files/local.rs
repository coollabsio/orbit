use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;

use tokio::fs;
use uuid::Uuid;

use super::{
    BlobFuture, BlobObject, BlobReader, BlobStore, BlobStoreError, StagedUpload, StoredObject,
};

#[derive(Clone, Debug)]
pub struct LocalBlobStore {
    root: PathBuf,
}

impl LocalBlobStore {
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn path(&self, storage_key: &str) -> Result<PathBuf, BlobStoreError> {
        let relative = Path::new(storage_key);
        if relative.is_absolute()
            || relative
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err(BlobStoreError::UnsafeStorageKey(storage_key.to_owned()));
        }
        Ok(self.root.join(relative))
    }

    pub async fn blobs(&self) -> Result<Vec<BlobObject>, BlobStoreError> {
        list_files(&self.root, &self.root.join("blobs")).await
    }

    pub async fn temporary_files(&self) -> Result<Vec<BlobObject>, BlobStoreError> {
        list_files(&self.root, &self.root.join("temporary")).await
    }

    fn temporary_path_is_safe(&self, path: &Path) -> bool {
        path.parent() == Some(self.root.join("temporary").as_path())
            && path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("upload-"))
    }
}

impl BlobStore for LocalBlobStore {
    fn create_temporary(&self) -> BlobFuture<'_, PathBuf> {
        Box::pin(async move {
            let directory = self.root.join("temporary");
            fs::create_dir_all(&directory)
                .await
                .map_err(|source| io_error(&directory, source))?;
            let path = directory.join(format!("upload-{}", Uuid::now_v7()));
            fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&path)
                .await
                .map_err(|source| io_error(&path, source))?;
            Ok(path)
        })
    }

    fn install<'a>(&'a self, upload: &'a StagedUpload) -> BlobFuture<'a, StoredObject> {
        Box::pin(async move {
            if !self.temporary_path_is_safe(&upload.temporary_path) {
                return Err(BlobStoreError::UnsafeTemporaryPath(
                    upload.temporary_path.clone(),
                ));
            }
            let storage_key = format!(
                "blobs/{}/{}-{}",
                upload.workspace_id, upload.sha256, upload.size_bytes
            );
            let destination = self.path(&storage_key)?;
            let parent = destination
                .parent()
                .expect("generated blob paths always have a parent");
            fs::create_dir_all(parent)
                .await
                .map_err(|source| io_error(parent, source))?;

            let deduplicated = match fs::hard_link(&upload.temporary_path, &destination).await {
                Ok(()) => false,
                Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => true,
                Err(source) => return Err(io_error(&destination, source)),
            };
            remove_if_exists(&upload.temporary_path).await?;
            Ok(StoredObject {
                storage_key,
                deduplicated,
            })
        })
    }

    fn open<'a>(&'a self, storage_key: &'a str) -> BlobFuture<'a, BlobReader> {
        Box::pin(async move {
            let path = self.path(storage_key)?;
            let file = fs::File::open(&path)
                .await
                .map_err(|source| io_error(&path, source))?;
            Ok(Box::pin(file) as BlobReader)
        })
    }

    fn delete<'a>(&'a self, storage_key: &'a str) -> BlobFuture<'a, ()> {
        Box::pin(async move {
            let path = self.path(storage_key)?;
            remove_if_exists(&path).await
        })
    }

    fn delete_temporary<'a>(&'a self, path: &'a Path) -> BlobFuture<'a, ()> {
        Box::pin(async move {
            if !self.temporary_path_is_safe(path) {
                return Err(BlobStoreError::UnsafeTemporaryPath(path.to_owned()));
            }
            remove_if_exists(path).await
        })
    }

    fn blobs(&self) -> BlobFuture<'_, Vec<BlobObject>> {
        Box::pin(LocalBlobStore::blobs(self))
    }

    fn temporary_files(&self) -> BlobFuture<'_, Vec<BlobObject>> {
        Box::pin(LocalBlobStore::temporary_files(self))
    }
}

async fn remove_if_exists(path: &Path) -> Result<(), BlobStoreError> {
    match fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(io_error(path, source)),
    }
}

async fn list_files(root: &Path, directory: &Path) -> Result<Vec<BlobObject>, BlobStoreError> {
    if !fs::try_exists(directory)
        .await
        .map_err(|source| io_error(directory, source))?
    {
        return Ok(Vec::new());
    }

    let mut pending = vec![directory.to_owned()];
    let mut files = Vec::new();
    while let Some(current) = pending.pop() {
        let mut entries = fs::read_dir(&current)
            .await
            .map_err(|source| io_error(&current, source))?;
        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|source| io_error(&current, source))?
        {
            let file_type = entry
                .file_type()
                .await
                .map_err(|source| io_error(&entry.path(), source))?;
            if file_type.is_dir() {
                pending.push(entry.path());
            } else if file_type.is_file() {
                let metadata = entry
                    .metadata()
                    .await
                    .map_err(|source| io_error(&entry.path(), source))?;
                let modified_at_millis = metadata
                    .modified()
                    .unwrap_or(UNIX_EPOCH)
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis()
                    .try_into()
                    .unwrap_or(i64::MAX);
                let storage_key = entry
                    .path()
                    .strip_prefix(root)
                    .expect("walked entries remain under the store root")
                    .to_string_lossy()
                    .replace('\\', "/");
                files.push(BlobObject {
                    storage_key,
                    path: entry.path(),
                    modified_at_millis,
                });
            }
        }
    }
    files.sort_by(|left, right| left.storage_key.cmp(&right.storage_key));
    Ok(files)
}

fn io_error(path: &Path, source: std::io::Error) -> BlobStoreError {
    BlobStoreError::Io {
        path: path.to_owned(),
        source,
    }
}
