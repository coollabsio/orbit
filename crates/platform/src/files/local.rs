use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use sha2::{Digest, Sha256};
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use uuid::Uuid;

use super::upload::detect_media_type;
use super::{
    BlobFuture, BlobObject, BlobReader, BlobStore, BlobStoreError, StagedUpload, StoredObject,
};

const SNIFF_BYTES: usize = 8 * 1024;

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
    fn create_temporary(&self) -> Result<PathBuf, BlobStoreError> {
        let directory = self.root.join("temporary");
        std::fs::create_dir_all(&directory).map_err(|source| io_error(&directory, source))?;
        let path = directory.join(format!("upload-{}", Uuid::now_v7()));
        std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
            .map_err(|source| io_error(&path, source))?;
        Ok(path)
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

            // Publish only the bytes that were hashed below. Copying to a private inode before
            // linking removes the inspect-then-publish race on the caller-visible staging file.
            let publishing_path = parent.join(format!(".publish-{}", Uuid::now_v7()));
            let (publishing, mut output) = PublishingFile::create(publishing_path)?;
            copy_and_verify(upload, publishing.path(), &mut output).await?;
            drop(output);

            let file = std::fs::OpenOptions::new()
                .write(true)
                .open(publishing.path())
                .map_err(|source| io_error(publishing.path(), source))?;
            file.set_modified(SystemTime::now())
                .map_err(|source| io_error(publishing.path(), source))?;
            drop(file);

            let deduplicated = match fs::hard_link(publishing.path(), &destination).await {
                Ok(()) => false,
                Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => {
                    let file = std::fs::OpenOptions::new()
                        .write(true)
                        .open(&destination)
                        .map_err(|source| io_error(&destination, source))?;
                    file.set_modified(SystemTime::now())
                        .map_err(|source| io_error(&destination, source))?;
                    true
                }
                Err(source) => return Err(io_error(&destination, source)),
            };
            publishing.remove().await?;
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

    fn blob_modified_at<'a>(&'a self, storage_key: &'a str) -> BlobFuture<'a, Option<i64>> {
        Box::pin(async move {
            let path = self.path(storage_key)?;
            let metadata = match fs::metadata(&path).await {
                Ok(metadata) => metadata,
                Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(source) => return Err(io_error(&path, source)),
            };
            let modified_at_millis = metadata
                .modified()
                .unwrap_or(UNIX_EPOCH)
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
                .try_into()
                .unwrap_or(i64::MAX);
            Ok(Some(modified_at_millis))
        })
    }

    fn blobs(&self) -> BlobFuture<'_, Vec<BlobObject>> {
        Box::pin(LocalBlobStore::blobs(self))
    }

    fn temporary_files(&self) -> BlobFuture<'_, Vec<BlobObject>> {
        Box::pin(LocalBlobStore::temporary_files(self))
    }
}

async fn copy_and_verify(
    upload: &StagedUpload,
    destination: &Path,
    output: &mut fs::File,
) -> Result<(), BlobStoreError> {
    let mut input = fs::File::open(&upload.temporary_path)
        .await
        .map_err(|source| io_error(&upload.temporary_path, source))?;
    let mut digest = Sha256::new();
    let mut size_bytes = 0_u64;
    let mut sniffed = Vec::with_capacity(SNIFF_BYTES);
    let mut buffer = [0_u8; 64 * 1024];

    loop {
        let count = input
            .read(&mut buffer)
            .await
            .map_err(|source| io_error(&upload.temporary_path, source))?;
        if count == 0 {
            break;
        }
        size_bytes = size_bytes
            .checked_add(count as u64)
            .ok_or(BlobStoreError::StagedContentChanged)?;
        let sniff_count = count.min(SNIFF_BYTES.saturating_sub(sniffed.len()));
        sniffed.extend_from_slice(&buffer[..sniff_count]);
        digest.update(&buffer[..count]);
        output
            .write_all(&buffer[..count])
            .await
            .map_err(|source| io_error(destination, source))?;
    }
    output
        .flush()
        .await
        .map_err(|source| io_error(destination, source))?;
    output
        .sync_all()
        .await
        .map_err(|source| io_error(destination, source))?;

    let hash = format!("{:x}", digest.finalize());
    if size_bytes != upload.size_bytes
        || hash != upload.sha256
        || detect_media_type(&sniffed) != upload.detected_media_type
    {
        return Err(BlobStoreError::StagedContentChanged);
    }
    Ok(())
}

struct PublishingFile {
    path: Option<PathBuf>,
}

impl PublishingFile {
    fn create(path: PathBuf) -> Result<(Self, fs::File), BlobStoreError> {
        let file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
            .map_err(|source| io_error(&path, source))?;
        Ok((Self { path: Some(path) }, fs::File::from_std(file)))
    }

    fn path(&self) -> &Path {
        self.path
            .as_deref()
            .expect("publishing path is still owned")
    }

    async fn remove(mut self) -> Result<(), BlobStoreError> {
        remove_if_exists(self.path()).await?;
        self.path = None;
        Ok(())
    }
}

impl Drop for PublishingFile {
    fn drop(&mut self) {
        if let Some(path) = self.path.take() {
            let _ = std::fs::remove_file(path);
        }
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

#[cfg(test)]
mod tests {
    use std::future::pending;

    use super::PublishingFile;

    #[tokio::test]
    async fn cancelling_a_publication_removes_its_private_copy() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(".publish-test");
        let task_path = path.clone();
        let task = tokio::spawn(async move {
            let (_publishing, _output) = PublishingFile::create(task_path).unwrap();
            pending::<()>().await;
        });

        while !path.exists() {
            tokio::task::yield_now().await;
        }
        task.abort();
        task.await.unwrap_err();

        assert!(!path.exists());
    }
}
