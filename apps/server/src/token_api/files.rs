//! Task and comment attachments, and doc files, for API tokens.
//!
//! The rules are the web app's: [`AttachmentRepository`] and [`PageFileRepository`] check the
//! member's access, stage and sniff the bytes, enforce the upload limits and the workspace's
//! storage quota, and store the blob before their write transaction. On top, a task must be in
//! an approved project. Uploads and deletes act as the token's user, so they need a personal
//! token; reads also work for a service account (as its other task reads do).
use orbit_platform::{
    BlobDownload, BlobStoreError, Id, StagedUpload, TimestampMillis, UploadError,
};
use serde::Serialize;
use tokio::io::AsyncReadExt;
use utoipa::ToSchema;

use super::{Authorized, Result, TokenApiError, page_limit};
use crate::repositories::api_tokens::ApiTokenScope;
use crate::repositories::attachments::{
    AttachmentCaller, AttachmentRepository, AttachmentRepositoryError, CreatedAttachment,
};
use crate::repositories::page_files::{PageFile, PageFileError, PageFileRepository};
use crate::repositories::tasks::Page;

/// The largest file that a download tool returns inline (as base64); larger files give only
/// their REST download path.
pub const MAX_INLINE_DOWNLOAD_BYTES: usize = 64 * 1024;

const SERVICE_ACCOUNT_FILES: &str = "A service-account token cannot upload or delete files: they are stored as a person's. Use a personal API token.";
const COMMENT_FILES_FORBIDDEN: &str =
    "Only the comment's author (or a workspace owner or admin) may change its attachments";
const QUOTA_DETAIL: &str = "This file does not fit in the workspace's storage quota. Delete files you no longer need (files in the trash count until the trash is emptied), or ask the administrator of this Orbit for a higher quota.";

/// A file attached to a task or one of its comments.
#[derive(Clone, Debug, Serialize, ToSchema)]
#[schema(as = PublicAttachment)]
pub struct TokenFile {
    #[schema(value_type = String)]
    pub id: Id,
    #[schema(value_type = String)]
    pub task_id: Id,
    /// The comment that holds the file; `null` for a file of the task itself.
    #[schema(value_type = Option<String>, required = true)]
    pub comment_id: Option<Id>,
    /// The member who uploaded it.
    #[schema(value_type = String)]
    pub owner_id: Id,
    pub file_name: String,
    /// Detected from the file's bytes, not taken from the client.
    pub media_type: String,
    pub byte_size: u64,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: TimestampMillis,
    /// An admin deleted the stored file: its download answers 410 with a "Deleted file" image.
    pub deleted: bool,
    /// `GET` this path of the public API (with the token) for the file's bytes.
    pub download_path: String,
}

/// A file of a doc: an image or file block, or the cover.
#[derive(Clone, Debug, Serialize, ToSchema)]
#[schema(as = PublicDocFile)]
pub struct TokenPageFile {
    #[schema(value_type = String)]
    pub id: Id,
    #[schema(value_type = String)]
    pub page_id: Id,
    /// The file's address inside Orbit: put it in the doc's Markdown (`![name](url)` for an
    /// image, `[name](url)` for a file) or use it as the doc's cover. Signed-in members open it.
    pub url: String,
    pub file_name: String,
    /// Detected from the file's bytes, not taken from the client.
    pub mime_type: String,
    pub size_bytes: u64,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: TimestampMillis,
    /// `GET` this path of the public API (with the token) for the file's bytes.
    pub download_path: String,
}

/// An open download, and its REST path for a client that wants to fetch it again.
pub struct FileDownload {
    pub blob: BlobDownload,
    pub download_path: String,
}

impl FileDownload {
    /// The bytes when the file has at most `cap` of them; `None` for a larger file.
    pub async fn read_capped(self, cap: usize) -> Result<Option<Vec<u8>>> {
        let mut bytes = Vec::new();
        let limit = u64::try_from(cap).unwrap_or(u64::MAX).saturating_add(1);
        self.blob
            .reader
            .take(limit)
            .read_to_end(&mut bytes)
            .await
            .map_err(|error| TokenApiError::Internal(error.to_string()))?;
        Ok((bytes.len() <= cap).then_some(bytes))
    }
}

/// Where an upload goes, checked before its bytes are read.
pub struct UploadTarget {
    task_id: Id,
    comment_id: Option<Id>,
}

/// Where a doc upload goes, checked before its bytes are read.
pub struct DocUploadTarget {
    page_id: Id,
}

impl super::TokenApi {
    /// The largest upload request body: the server's request limit, or the default when the
    /// server has no file storage.
    #[must_use]
    pub fn upload_request_limit(&self) -> usize {
        let limits = self
            .uploads
            .as_ref()
            .map_or_else(orbit_platform::UploadLimits::default, |uploads| {
                uploads.limits()
            });
        usize::try_from(limits.max_request_bytes()).unwrap_or(usize::MAX)
    }
}

impl Authorized<'_> {
    /// The upload service for staging a request body; set when the server has file storage.
    pub fn uploads(&self) -> Result<&orbit_platform::UploadService> {
        self.api
            .uploads
            .as_ref()
            .ok_or_else(|| TokenApiError::Internal("upload service not configured".to_owned()))
    }

    /// The token user: owner of the files it uploads.
    pub fn uploader(&self) -> (Id, Id) {
        (self.principal.workspace_id, self.principal.actor_id)
    }

    /// The files of a task (`comment_id` unset) or of one of its comments, oldest first. Files
    /// pasted into a description or comment text are not listed.
    pub async fn attachments(
        &self,
        task_ref: &str,
        comment_id: Option<Id>,
        cursor: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Page<TokenFile>> {
        self.need(ApiTokenScope::TasksRead)?;
        let limit = page_limit(cursor, limit)?;
        let task = self.task(task_ref).await?;
        let p = self.principal;
        let list = self
            .attachment_repository()?
            .list(
                AttachmentCaller::Member(p.actor_id),
                p.workspace_id,
                task.id,
                comment_id,
                cursor,
                limit,
            )
            .await
            .map_err(attachment_error)?;
        Ok(Page {
            items: list
                .items
                .into_iter()
                .map(|file| self.token_file(file))
                .collect(),
            next_cursor: list.next_cursor,
        })
    }

    /// Checks that the token user may add a file to the task or comment. Call it before the
    /// upload body is read; [`Self::finish_upload`] checks again.
    pub async fn start_upload(
        &self,
        task_ref: &str,
        comment_id: Option<Id>,
    ) -> Result<UploadTarget> {
        self.file_writer(ApiTokenScope::TasksWrite)?;
        let task = self.task(task_ref).await?;
        let p = self.principal;
        let repository = self.attachment_repository()?;
        let caller = AttachmentCaller::Member(p.actor_id);
        match comment_id {
            Some(comment_id) => repository
                .require_comment_editor(caller, p.workspace_id, task.id, comment_id)
                .await
                .map_err(attachment_error)?,
            None => repository
                .authorize_task(caller, p.workspace_id, task.id)
                .await
                .map_err(attachment_error)?,
        }
        Ok(UploadTarget {
            task_id: task.id,
            comment_id,
        })
    }

    /// Turns a staged upload into an attachment of the target. The bytes go to the blob store
    /// before the write transaction. The caller discards `upload` when this fails.
    pub async fn finish_upload(
        &self,
        target: &UploadTarget,
        upload: &StagedUpload,
    ) -> Result<TokenFile> {
        self.file_writer(ApiTokenScope::TasksWrite)?;
        let p = self.principal;
        let file = self
            .attachment_repository()?
            .finalize(
                AttachmentCaller::Member(p.actor_id),
                p.workspace_id,
                target.task_id,
                target.comment_id,
                false,
                upload,
            )
            .await
            .map_err(attachment_error)?;
        Ok(self.token_file(file))
    }

    /// [`Self::start_upload`] and [`Self::finish_upload`] for bytes that the request carried
    /// (an MCP tool's base64 content).
    pub async fn upload_attachment(
        &self,
        task_ref: &str,
        comment_id: Option<Id>,
        file_name: &str,
        bytes: &[u8],
    ) -> Result<TokenFile> {
        let target = self.start_upload(task_ref, comment_id).await?;
        let (workspace_id, actor_id) = self.uploader();
        let uploads = self.uploads()?;
        // Boxed: staging (with image shrinking) and the finalization are large futures, and the
        // MCP dispatch around them is deep; unboxed they overflowed a worker thread's stack.
        let staged = Box::pin(uploads.stage(workspace_id, actor_id, file_name, bytes))
            .await
            .map_err(upload_error)?;
        let result = Box::pin(self.finish_upload(&target, &staged)).await;
        if result.is_err() {
            let _ = uploads.discard(&staged).await;
        }
        result
    }

    /// Deletes a file of a task or comment. A comment's files follow the comment's edit rule.
    pub async fn delete_attachment(
        &self,
        task_ref: &str,
        comment_id: Option<Id>,
        attachment_id: Id,
    ) -> Result<()> {
        self.file_writer(ApiTokenScope::TasksWrite)?;
        let task = self.task(task_ref).await?;
        let p = self.principal;
        self.attachment_repository()?
            .delete(
                AttachmentCaller::Member(p.actor_id),
                p.workspace_id,
                task.id,
                comment_id,
                attachment_id,
                self.request_id,
                TimestampMillis::now(),
            )
            .await
            .map_err(attachment_error)
    }

    pub async fn download_attachment(
        &self,
        task_ref: &str,
        comment_id: Option<Id>,
        attachment_id: Id,
    ) -> Result<FileDownload> {
        self.need(ApiTokenScope::TasksRead)?;
        let task = self.task(task_ref).await?;
        let p = self.principal;
        let blob = self
            .attachment_repository()?
            .download(
                AttachmentCaller::Member(p.actor_id),
                p.workspace_id,
                task.id,
                comment_id,
                attachment_id,
            )
            .await
            .map_err(attachment_error)?;
        Ok(FileDownload {
            blob,
            download_path: attachment_path(p.workspace_id, task.id, comment_id, attachment_id),
        })
    }

    /// Checks that the token user may add a file to the doc. Call it before the body is read.
    pub async fn start_doc_upload(&self, page_id: Id) -> Result<DocUploadTarget> {
        self.content(ApiTokenScope::DocsWrite, &self.api.pages)?;
        let p = self.principal;
        self.page_file_repository()?
            .authorize_upload(p.workspace_id, page_id, p.actor_id)
            .await
            .map_err(page_file_error)?;
        Ok(DocUploadTarget { page_id })
    }

    /// Turns a staged upload into a file of the doc; the caller discards `upload` on failure.
    pub async fn finish_doc_upload(
        &self,
        target: &DocUploadTarget,
        upload: &StagedUpload,
    ) -> Result<TokenPageFile> {
        self.content(ApiTokenScope::DocsWrite, &self.api.pages)?;
        let p = self.principal;
        let file = self
            .page_file_repository()?
            .finalize(
                p.workspace_id,
                target.page_id,
                p.actor_id,
                upload,
                self.request_id,
            )
            .await
            .map_err(page_file_error)?;
        Ok(self.token_page_file(file))
    }

    pub async fn upload_doc_file(
        &self,
        page_id: Id,
        file_name: &str,
        bytes: &[u8],
    ) -> Result<TokenPageFile> {
        let target = self.start_doc_upload(page_id).await?;
        let (workspace_id, actor_id) = self.uploader();
        let uploads = self.uploads()?;
        // Boxed for the stack, as in `upload_attachment`.
        let staged = Box::pin(uploads.stage(workspace_id, actor_id, file_name, bytes))
            .await
            .map_err(upload_error)?;
        let result = Box::pin(self.finish_doc_upload(&target, &staged)).await;
        if result.is_err() {
            let _ = uploads.discard(&staged).await;
        }
        result
    }

    pub async fn download_doc_file(&self, page_id: Id, file_id: Id) -> Result<FileDownload> {
        self.content(ApiTokenScope::DocsRead, &self.api.pages)?;
        let p = self.principal;
        let blob = self
            .page_file_repository()?
            .download(p.workspace_id, page_id, file_id, p.actor_id)
            .await
            .map_err(page_file_error)?;
        Ok(FileDownload {
            blob,
            download_path: doc_file_path(p.workspace_id, page_id, file_id),
        })
    }

    /// A file write with `scope`, by a personal token.
    fn file_writer(&self, scope: ApiTokenScope) -> Result<()> {
        self.need(scope)?;
        match self.principal.service_account {
            Some(_) => Err(TokenApiError::Forbidden(SERVICE_ACCOUNT_FILES)),
            None => Ok(()),
        }
    }

    fn attachment_repository(&self) -> Result<AttachmentRepository> {
        Ok(AttachmentRepository::new(
            self.api.tasks.database().clone(),
            self.uploads()?.clone(),
        ))
    }

    fn page_file_repository(&self) -> Result<PageFileRepository> {
        Ok(PageFileRepository::new(
            self.api.tasks.database().clone(),
            self.uploads()?.clone(),
        ))
    }

    fn token_file(&self, file: CreatedAttachment) -> TokenFile {
        TokenFile {
            download_path: attachment_path(
                file.workspace_id,
                file.task_id,
                file.comment_id,
                file.id,
            ),
            id: file.id,
            task_id: file.task_id,
            comment_id: file.comment_id,
            owner_id: file.owner_id,
            file_name: file.display_name,
            media_type: file.media_type,
            byte_size: file.byte_size,
            created_at: file.created_at,
            deleted: file.deleted,
        }
    }

    fn token_page_file(&self, file: PageFile) -> TokenPageFile {
        TokenPageFile {
            download_path: doc_file_path(self.principal.workspace_id, file.page_id, file.id),
            id: file.id,
            page_id: file.page_id,
            url: file.url,
            file_name: file.file_name,
            mime_type: file.mime_type,
            size_bytes: file.size_bytes,
            created_at: file.created_at,
        }
    }
}

fn attachment_path(workspace_id: Id, task_id: Id, comment_id: Option<Id>, id: Id) -> String {
    let base = crate::public_api::BASE_PATH;
    match comment_id {
        Some(comment_id) => format!(
            "{base}/workspaces/{workspace_id}/tasks/{task_id}/comments/{comment_id}/attachments/{id}/download"
        ),
        None => {
            format!("{base}/workspaces/{workspace_id}/tasks/{task_id}/attachments/{id}/download")
        }
    }
}

fn doc_file_path(workspace_id: Id, page_id: Id, file_id: Id) -> String {
    format!(
        "{}/workspaces/{workspace_id}/docs/{page_id}/files/{file_id}",
        crate::public_api::BASE_PATH
    )
}

fn attachment_error(error: AttachmentRepositoryError) -> TokenApiError {
    match error {
        AttachmentRepositoryError::NotFound => TokenApiError::NotAvailable,
        AttachmentRepositoryError::Forbidden => TokenApiError::Forbidden(COMMENT_FILES_FORBIDDEN),
        AttachmentRepositoryError::InvalidCursor => TokenApiError::Invalid("Invalid cursor"),
        AttachmentRepositoryError::Upload(error) => upload_error(error),
        error @ (AttachmentRepositoryError::Database(_)
        | AttachmentRepositoryError::InvalidRecord) => TokenApiError::Internal(error.to_string()),
    }
}

fn page_file_error(error: PageFileError) -> TokenApiError {
    match error {
        PageFileError::NotFound => TokenApiError::NotAvailable,
        PageFileError::Upload(error) => upload_error(error),
        PageFileError::Database(error) => TokenApiError::Internal(error.to_string()),
    }
}

/// The upload service's refusals, as the web routes answer them.
pub fn upload_error(error: UploadError) -> TokenApiError {
    match error {
        UploadError::FileTooLarge { .. } | UploadError::RequestTooLarge { .. } => too_large(),
        UploadError::StorageQuotaExceeded { .. } => TokenApiError::TooLarge {
            code: "storage_quota_exceeded",
            detail: QUOTA_DETAIL,
        },
        UploadError::InvalidDisplayName => TokenApiError::Invalid(
            "The file name is invalid: 1 to 255 bytes, without control characters",
        ),
        UploadError::BlobStore(BlobStoreError::Io { source, .. })
            if source.kind() == std::io::ErrorKind::FileTooLarge =>
        {
            too_large()
        }
        UploadError::BlobStore(BlobStoreError::Io { source, .. })
            if source.kind() == std::io::ErrorKind::InvalidData =>
        {
            TokenApiError::Invalid("The upload body is invalid")
        }
        UploadError::Unauthorized => TokenApiError::NotAvailable,
        error => TokenApiError::Internal(error.to_string()),
    }
}

pub fn too_large() -> TokenApiError {
    TokenApiError::TooLarge {
        code: "upload_too_large",
        detail: "The file is larger than this Orbit's upload limit",
    }
}
