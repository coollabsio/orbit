use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use orbit_platform::{
    AttachmentMutationCoordinator, BlobFuture, BlobObject, BlobReader, BlobStore,
    ContentDisposition, Id, LocalBlobStore, StagedUpload, StoredObject, TestDatabase, UploadError,
    UploadLimitError, UploadLimits, UploadService,
};
use tokio::io::{AsyncRead, AsyncReadExt, ReadBuf};

struct Fixture {
    _root: tempfile::TempDir,
    database: TestDatabase,
    store: LocalBlobStore,
    service: UploadService,
}

impl Fixture {
    async fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let database = TestDatabase::new().await.unwrap();
        let store = LocalBlobStore::new(root.path().join("attachments"));
        let service = UploadService::new(
            (*database).clone(),
            Arc::new(store.clone()),
            AttachmentMutationCoordinator::default(),
            UploadLimits::default(),
        );
        Self {
            _root: root,
            database,
            store,
            service,
        }
    }
}

#[tokio::test]
async fn defaults_accept_25_mib_and_reject_larger_files_without_reading_the_rest() {
    let fixture = Fixture::new().await;
    let limits = UploadLimits::default();
    assert_eq!(limits.max_file_bytes(), 25 * 1024 * 1024);
    assert_eq!(limits.max_request_bytes(), 100 * 1024 * 1024);

    let staged = fixture
        .service
        .stage(
            Id::new_v7(),
            Id::new_v7(),
            "archive.bin",
            tokio::io::repeat(0x5a).take(limits.max_file_bytes()),
        )
        .await
        .unwrap();
    assert_eq!(staged.size_bytes, 25 * 1024 * 1024);

    let oversized = CountingReader::new(40 * 1024 * 1024);
    let bytes_read = oversized.bytes_read.clone();
    let error = fixture
        .service
        .stage(Id::new_v7(), Id::new_v7(), "too-big.bin", oversized)
        .await
        .unwrap_err();
    assert!(matches!(error, UploadError::FileTooLarge { .. }));
    assert!(*bytes_read.lock().unwrap() < 40 * 1024 * 1024);
    assert_eq!(fixture.store.temporary_files().await.unwrap().len(), 1);
}

#[tokio::test]
async fn request_budget_rejects_before_creating_a_temporary_file() {
    let fixture = Fixture::new().await;
    let error = fixture
        .service
        .stage_in_request(
            Id::new_v7(),
            Id::new_v7(),
            "another.bin",
            UploadLimits::default().max_request_bytes(),
            tokio::io::empty(),
        )
        .await
        .unwrap_err();

    assert!(matches!(error, UploadError::RequestTooLarge { .. }));
    assert!(fixture.store.temporary_files().await.unwrap().is_empty());
}

#[test]
fn upload_limits_reject_values_that_cannot_be_stored_safely() {
    assert!(matches!(
        UploadLimits::new(0, 1),
        Err(UploadLimitError::Zero)
    ));
    assert!(matches!(
        UploadLimits::new(2, 1),
        Err(UploadLimitError::FileExceedsRequest)
    ));
    assert!(matches!(
        UploadLimits::new(i64::MAX as u64 + 1, i64::MAX as u64 + 1),
        Err(UploadLimitError::PlatformBound)
    ));
}

#[tokio::test]
async fn stage_sniffs_bytes_and_never_uses_a_client_path_as_a_filesystem_path() {
    let fixture = Fixture::new().await;
    let png = fixture
        .service
        .stage(
            Id::new_v7(),
            Id::new_v7(),
            r"C:\fake\folder\picture.png",
            &b"\x89PNG\r\n\x1a\ncontent"[..],
        )
        .await
        .unwrap();
    assert_eq!(png.detected_media_type, "image/png");
    assert_eq!(png.display_name, "picture.png");
    assert_eq!(png.sha256.len(), 64);
    assert!(png.temporary_path.starts_with(fixture.store.root()));

    let svg = fixture
        .service
        .stage(
            Id::new_v7(),
            Id::new_v7(),
            "diagram.svg",
            &b"  <?xml version='1.0'?><svg xmlns='http://www.w3.org/2000/svg'></svg>"[..],
        )
        .await
        .unwrap();
    assert_eq!(svg.detected_media_type, "image/svg+xml");
}

#[tokio::test]
async fn finalize_deduplicates_only_inside_a_workspace_and_atomically_installs_the_staged_file() {
    let fixture = Fixture::new().await;
    let workspace_a = Id::new_v7();
    let workspace_b = Id::new_v7();
    let owner = Id::new_v7();

    let first = fixture
        .service
        .stage(workspace_a, owner, "first.txt", &b"same bytes"[..])
        .await
        .unwrap();
    let first_temp = first.temporary_path.clone();
    let first_blob = fixture.service.finalize(&first).await.unwrap();
    assert!(!first_temp.exists());
    assert!(
        fixture
            .store
            .path(&first_blob.storage_key)
            .unwrap()
            .exists()
    );

    let duplicate = fixture
        .service
        .stage(workspace_a, owner, "second.txt", &b"same bytes"[..])
        .await
        .unwrap();
    let duplicate_blob = fixture.service.finalize(&duplicate).await.unwrap();
    assert_eq!(first_blob.id, duplicate_blob.id);
    assert_eq!(first_blob.storage_key, duplicate_blob.storage_key);

    let isolated = fixture
        .service
        .stage(workspace_b, owner, "third.txt", &b"same bytes"[..])
        .await
        .unwrap();
    let isolated_blob = fixture.service.finalize(&isolated).await.unwrap();
    assert_ne!(first_blob.id, isolated_blob.id);
    assert_ne!(first_blob.storage_key, isolated_blob.storage_key);
    assert_eq!(fixture.store.blobs().await.unwrap().len(), 2);
}

#[tokio::test]
async fn concurrent_same_workspace_finalization_converges_on_one_blob() {
    let fixture = Fixture::new().await;
    let workspace = Id::new_v7();
    let first = fixture
        .service
        .stage(workspace, Id::new_v7(), "one.bin", &b"parallel"[..])
        .await
        .unwrap();
    let second = fixture
        .service
        .stage(workspace, Id::new_v7(), "two.bin", &b"parallel"[..])
        .await
        .unwrap();
    let other_service = UploadService::new(
        (*fixture.database).clone(),
        Arc::new(fixture.store.clone()),
        AttachmentMutationCoordinator::default(),
        UploadLimits::default(),
    );

    let (first_result, second_result) = tokio::join!(
        fixture.service.finalize(&first),
        other_service.finalize(&second)
    );
    let first_blob = first_result.unwrap();
    let second_blob = second_result.unwrap();
    assert_eq!(first_blob.id, second_blob.id);
    assert_eq!(fixture.store.blobs().await.unwrap().len(), 1);
}

#[tokio::test]
async fn finalize_rejects_tampered_staged_metadata_before_moving_bytes() {
    let fixture = Fixture::new().await;
    let mut staged = fixture
        .service
        .stage(Id::new_v7(), Id::new_v7(), "safe.txt", &b"original"[..])
        .await
        .unwrap();
    let temporary_path = staged.temporary_path.clone();
    staged.sha256 = "0".repeat(64);

    assert!(matches!(
        fixture.service.finalize(&staged).await,
        Err(UploadError::InvalidState)
    ));
    assert!(temporary_path.exists());
    assert!(fixture.store.blobs().await.unwrap().is_empty());
}

#[tokio::test]
async fn failed_attachment_write_rolls_back_blob_metadata_and_leaves_quarantined_bytes() {
    let fixture = Fixture::new().await;
    let staged = fixture
        .service
        .stage(Id::new_v7(), Id::new_v7(), "rollback.txt", &b"orphan"[..])
        .await
        .unwrap();

    let result = fixture
        .service
        .finalize_with(&staged, |transaction, _blob| {
            Box::pin(async move {
                sqlx::query("INSERT INTO missing_attachment_table DEFAULT VALUES")
                    .execute(&mut *transaction)
                    .await?;
                Ok(())
            })
        })
        .await;
    assert!(matches!(result, Err(UploadError::Database(_))));
    assert_eq!(
        fixture
            .database
            .scalar::<i64>("SELECT COUNT(*) FROM attachment_blobs")
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        fixture
            .database
            .scalar::<String>(&format!(
                "SELECT state FROM pending_uploads WHERE id = '{}'",
                staged.id
            ))
            .await
            .unwrap(),
        "staged"
    );
    assert_eq!(fixture.store.blobs().await.unwrap().len(), 1);
}

#[tokio::test]
async fn cancelling_stage_removes_its_partial_temporary_file() {
    let fixture = Fixture::new().await;
    let service = fixture.service.clone();
    let task = tokio::spawn(async move {
        service
            .stage(Id::new_v7(), Id::new_v7(), "cancelled.bin", PendingReader)
            .await
    });

    for _ in 0..100 {
        if !fixture.store.temporary_files().await.unwrap().is_empty() {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert_eq!(fixture.store.temporary_files().await.unwrap().len(), 1);
    task.abort();
    let _ = task.await;
    for _ in 0..100 {
        if fixture.store.temporary_files().await.unwrap().is_empty() {
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!("cancelled upload left a temporary file");
}

#[tokio::test]
async fn cancelling_while_cleanup_is_waiting_still_removes_the_temporary_file() {
    let root = tempfile::tempdir().unwrap();
    let database = TestDatabase::new().await.unwrap();
    let local = LocalBlobStore::new(root.path().join("attachments"));
    let delete_started = Arc::new(tokio::sync::Notify::new());
    let delete_resume = Arc::new(tokio::sync::Semaphore::new(0));
    let store = PausingDeleteStore {
        inner: local.clone(),
        delete_started: delete_started.clone(),
        delete_resume: delete_resume.clone(),
    };
    let service = UploadService::new(
        (*database).clone(),
        Arc::new(store),
        AttachmentMutationCoordinator::default(),
        UploadLimits::new(1, 10).unwrap(),
    );
    let task = tokio::spawn(async move {
        service
            .stage(Id::new_v7(), Id::new_v7(), "too-big.bin", &b"xx"[..])
            .await
    });

    delete_started.notified().await;
    task.abort();
    let _ = task.await;
    delete_resume.add_permits(1);
    for _ in 0..100 {
        if local.temporary_files().await.unwrap().is_empty() {
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!("cancellation during cleanup left a temporary file");
}

#[tokio::test]
async fn reconciliation_waits_24_hours_and_queries_references_before_deleting() {
    let fixture = Fixture::new().await;
    let staged = fixture
        .service
        .stage(
            Id::new_v7(),
            Id::new_v7(),
            "kept.txt",
            &b"kept until unreferenced"[..],
        )
        .await
        .unwrap();
    let blob = fixture.service.finalize(&staged).await.unwrap();
    let now = fixture.database.database_now().await.unwrap().as_millis();

    fixture
        .database
        .execute("CREATE TABLE attachments (id TEXT PRIMARY KEY, storage_key TEXT NOT NULL);")
        .await
        .unwrap();
    fixture
        .database
        .execute(&format!(
            "INSERT INTO attachments (id, storage_key) VALUES ('attachment-1', '{}')",
            blob.storage_key
        ))
        .await
        .unwrap();

    let before = fixture
        .service
        .reconcile(now + 23 * 60 * 60 * 1000)
        .await
        .unwrap();
    assert_eq!(before.deleted_blobs, 0);

    let referenced = fixture
        .service
        .reconcile(now + 25 * 60 * 60 * 1000)
        .await
        .unwrap();
    assert_eq!(referenced.deleted_blobs, 0);
    assert!(fixture.store.path(&blob.storage_key).unwrap().exists());

    fixture
        .database
        .execute("DELETE FROM attachments")
        .await
        .unwrap();
    let deleted = fixture
        .service
        .reconcile(now + 25 * 60 * 60 * 1000)
        .await
        .unwrap();
    assert_eq!(deleted.deleted_blobs, 1);
    assert!(!fixture.store.path(&blob.storage_key).unwrap().exists());
}

#[tokio::test]
async fn reconciliation_expires_pending_uploads_and_removes_their_temporary_files() {
    let fixture = Fixture::new().await;
    let staged = fixture
        .service
        .stage(Id::new_v7(), Id::new_v7(), "stale.txt", &b"stale"[..])
        .await
        .unwrap();
    fixture
        .database
        .execute(&format!(
            "UPDATE pending_uploads SET expires_at = 0 WHERE id = '{}'",
            staged.id
        ))
        .await
        .unwrap();

    let result = fixture.service.reconcile(1).await.unwrap();
    assert_eq!(result.expired_uploads, 1);
    assert!(!staged.temporary_path.exists());
    assert_eq!(
        fixture
            .database
            .scalar::<String>(&format!(
                "SELECT state FROM pending_uploads WHERE id = '{}'",
                staged.id
            ))
            .await
            .unwrap(),
        "failed"
    );
}

#[tokio::test]
async fn reconciliation_removes_aged_files_that_have_no_database_record() {
    let fixture = Fixture::new().await;
    let now = fixture.database.database_now().await.unwrap().as_millis();

    let temporary = fixture
        .service
        .stage(
            Id::new_v7(),
            Id::new_v7(),
            "abandoned.tmp",
            &b"temporary orphan"[..],
        )
        .await
        .unwrap();
    fixture
        .database
        .execute(&format!(
            "DELETE FROM pending_uploads WHERE id = '{}'",
            temporary.id
        ))
        .await
        .unwrap();

    let finalized = fixture
        .service
        .stage(
            Id::new_v7(),
            Id::new_v7(),
            "finalized.tmp",
            &b"final orphan"[..],
        )
        .await
        .unwrap();
    BlobStore::install(&fixture.store, &finalized)
        .await
        .unwrap();
    fixture
        .database
        .execute(&format!(
            "DELETE FROM pending_uploads WHERE id = '{}'",
            finalized.id
        ))
        .await
        .unwrap();

    let result = fixture
        .service
        .reconcile(now + 25 * 60 * 60 * 1000)
        .await
        .unwrap();
    assert_eq!(result.deleted_untracked_files, 1);
    assert!(!temporary.temporary_path.exists());
    assert!(fixture.store.blobs().await.unwrap().is_empty());
}

#[tokio::test]
async fn reconciliation_rechecks_an_orphan_that_is_finalized_after_the_file_scan() {
    let root = tempfile::tempdir().unwrap();
    let database = TestDatabase::new().await.unwrap();
    let local = LocalBlobStore::new(root.path().join("attachments"));
    let listed = Arc::new(tokio::sync::Notify::new());
    let resume = Arc::new(tokio::sync::Notify::new());
    let pausing = PausingBlobStore {
        inner: local.clone(),
        listed: listed.clone(),
        resume: resume.clone(),
    };
    let coordinator = AttachmentMutationCoordinator::default();
    let finalizer = UploadService::new(
        (*database).clone(),
        Arc::new(local.clone()),
        coordinator.clone(),
        UploadLimits::default(),
    );
    let reconciler = UploadService::new(
        (*database).clone(),
        Arc::new(pausing),
        coordinator,
        UploadLimits::default(),
    );

    let workspace = Id::new_v7();
    let orphan = finalizer
        .stage(workspace, Id::new_v7(), "old.bin", &b"shared"[..])
        .await
        .unwrap();
    BlobStore::install(&local, &orphan).await.unwrap();
    database
        .execute(&format!(
            "DELETE FROM pending_uploads WHERE id = '{}'",
            orphan.id
        ))
        .await
        .unwrap();
    let fresh = finalizer
        .stage(workspace, Id::new_v7(), "new.bin", &b"shared"[..])
        .await
        .unwrap();
    let now = database.database_now().await.unwrap().as_millis();
    database
        .execute(&format!(
            "UPDATE pending_uploads SET expires_at = {} WHERE id = '{}'",
            now + 48 * 60 * 60 * 1000,
            fresh.id
        ))
        .await
        .unwrap();

    let reconcile_task =
        tokio::spawn(async move { reconciler.reconcile(now + 25 * 60 * 60 * 1000).await });
    listed.notified().await;
    let blob = finalizer.finalize(&fresh).await.unwrap();
    resume.notify_one();
    reconcile_task.await.unwrap().unwrap();

    assert!(local.path(&blob.storage_key).unwrap().exists());
}

#[tokio::test]
async fn download_resolves_authorization_before_trying_to_open_a_blob() {
    let fixture = Fixture::new().await;
    let error = fixture
        .service
        .download(|| async { Err(UploadError::Unauthorized) })
        .await
        .err()
        .unwrap();

    assert!(matches!(error, UploadError::Unauthorized));
}

#[test]
fn download_metadata_allows_only_safe_images_inline_and_always_uses_nosniff() {
    let png = UploadService::download_metadata("image/png", "photo.png");
    assert_eq!(png.disposition, ContentDisposition::Inline);
    assert_eq!(png.x_content_type_options, "nosniff");

    for media_type in ["image/svg+xml", "text/html", "application/pdf"] {
        let download = UploadService::download_metadata(media_type, "report\".svg");
        assert_eq!(download.disposition, ContentDisposition::Attachment);
        assert_eq!(download.x_content_type_options, "nosniff");
        assert!(!download.content_disposition.contains('\r'));
        assert!(!download.content_disposition.contains('\n'));
    }
}

#[tokio::test]
async fn sniffed_media_types_drive_safe_inline_disposition() {
    let fixture = Fixture::new().await;
    let cases: [(&str, &[u8], ContentDisposition); 6] = [
        ("photo.jpg", b"\xff\xd8\xffdata", ContentDisposition::Inline),
        ("image.gif", b"GIF89adata", ContentDisposition::Inline),
        (
            "image.webp",
            b"RIFF\x04\x00\x00\x00WEBPdata",
            ContentDisposition::Inline,
        ),
        (
            "image.png",
            b"\x89PNG\r\n\x1a\ndata",
            ContentDisposition::Inline,
        ),
        (
            "active.svg",
            b"<svg xmlns='http://www.w3.org/2000/svg'></svg>",
            ContentDisposition::Attachment,
        ),
        (
            "active.html",
            b"<!doctype html><script>alert(1)</script>",
            ContentDisposition::Attachment,
        ),
    ];

    for (name, bytes, expected) in cases {
        let staged = fixture
            .service
            .stage(Id::new_v7(), Id::new_v7(), name, bytes)
            .await
            .unwrap();
        assert_eq!(
            UploadService::download_metadata(&staged.detected_media_type, &staged.display_name)
                .disposition,
            expected,
            "{name}"
        );
    }
}

#[derive(Clone)]
struct CountingReader {
    remaining: usize,
    bytes_read: Arc<std::sync::Mutex<usize>>,
}

impl CountingReader {
    fn new(bytes: usize) -> Self {
        Self {
            remaining: bytes,
            bytes_read: Arc::new(std::sync::Mutex::new(0)),
        }
    }
}

impl AsyncRead for CountingReader {
    fn poll_read(
        mut self: Pin<&mut Self>,
        _context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let amount = self.remaining.min(buffer.remaining());
        buffer.initialize_unfilled_to(amount).fill(0x61);
        buffer.advance(amount);
        self.remaining -= amount;
        *self.bytes_read.lock().unwrap() += amount;
        Poll::Ready(Ok(()))
    }
}

struct PendingReader;

impl AsyncRead for PendingReader {
    fn poll_read(
        self: Pin<&mut Self>,
        _context: &mut Context<'_>,
        _buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Poll::Pending
    }
}

#[derive(Clone)]
struct PausingBlobStore {
    inner: LocalBlobStore,
    listed: Arc<tokio::sync::Notify>,
    resume: Arc<tokio::sync::Notify>,
}

impl BlobStore for PausingBlobStore {
    fn create_temporary(&self) -> BlobFuture<'_, std::path::PathBuf> {
        BlobStore::create_temporary(&self.inner)
    }

    fn install<'a>(&'a self, upload: &'a StagedUpload) -> BlobFuture<'a, StoredObject> {
        BlobStore::install(&self.inner, upload)
    }

    fn open<'a>(&'a self, storage_key: &'a str) -> BlobFuture<'a, BlobReader> {
        BlobStore::open(&self.inner, storage_key)
    }

    fn delete<'a>(&'a self, storage_key: &'a str) -> BlobFuture<'a, ()> {
        BlobStore::delete(&self.inner, storage_key)
    }

    fn delete_temporary<'a>(&'a self, path: &'a std::path::Path) -> BlobFuture<'a, ()> {
        BlobStore::delete_temporary(&self.inner, path)
    }

    fn blobs(&self) -> BlobFuture<'_, Vec<BlobObject>> {
        Box::pin(async move {
            let blobs = self.inner.blobs().await?;
            self.listed.notify_one();
            self.resume.notified().await;
            Ok(blobs)
        })
    }

    fn temporary_files(&self) -> BlobFuture<'_, Vec<BlobObject>> {
        BlobStore::temporary_files(&self.inner)
    }
}

#[derive(Clone)]
struct PausingDeleteStore {
    inner: LocalBlobStore,
    delete_started: Arc<tokio::sync::Notify>,
    delete_resume: Arc<tokio::sync::Semaphore>,
}

impl BlobStore for PausingDeleteStore {
    fn create_temporary(&self) -> BlobFuture<'_, std::path::PathBuf> {
        BlobStore::create_temporary(&self.inner)
    }

    fn install<'a>(&'a self, upload: &'a StagedUpload) -> BlobFuture<'a, StoredObject> {
        BlobStore::install(&self.inner, upload)
    }

    fn open<'a>(&'a self, storage_key: &'a str) -> BlobFuture<'a, BlobReader> {
        BlobStore::open(&self.inner, storage_key)
    }

    fn delete<'a>(&'a self, storage_key: &'a str) -> BlobFuture<'a, ()> {
        BlobStore::delete(&self.inner, storage_key)
    }

    fn delete_temporary<'a>(&'a self, path: &'a std::path::Path) -> BlobFuture<'a, ()> {
        Box::pin(async move {
            self.delete_started.notify_one();
            let permit = self.delete_resume.acquire().await.expect("semaphore open");
            permit.forget();
            BlobStore::delete_temporary(&self.inner, path).await
        })
    }

    fn blobs(&self) -> BlobFuture<'_, Vec<BlobObject>> {
        BlobStore::blobs(&self.inner)
    }

    fn temporary_files(&self) -> BlobFuture<'_, Vec<BlobObject>> {
        BlobStore::temporary_files(&self.inner)
    }
}
