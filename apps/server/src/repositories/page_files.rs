//! Files attached to docs pages (editor images and files, uploaded covers). The bytes go through
//! the shared `UploadService` (size limits, content sniffing, per-workspace dedup, quarantine);
//! a `page_files` row is the page's reference to a blob. Access follows the page: a teamspace
//! page's files are readable by every member, a private page's only by its owner, and a trashed
//! page's by nobody.

use orbit_platform::{
    AuthorizedAttachment, BlobDownload, Database, Id, StagedUpload, TimestampMillis, UploadError,
    UploadService,
};
use serde::Serialize;
use serde_json::json;
use sqlx::{Row, Sqlite, Transaction};
use thiserror::Error;
use utoipa::ToSchema;

use super::pages::VISIBLE;
use crate::audit::{self, AuditOutcome};

/// A file attached to a page.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct PageFile {
    #[schema(value_type = String)]
    pub id: Id,
    #[schema(value_type = String)]
    pub page_id: Id,
    /// Same-origin download path (`/api/v1/workspaces/{workspace_id}/pages/{page_id}/files/{id}`);
    /// usable as an image `src`, a file link, or the page's `cover_url`.
    pub url: String,
    pub file_name: String,
    /// Detected from the file's bytes, not taken from the client.
    pub mime_type: String,
    pub size_bytes: u64,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: TimestampMillis,
}

#[derive(Debug, Error)]
pub enum PageFileError {
    /// The page or file is missing, trashed, or not visible to the caller.
    #[error("page file target was not found")]
    NotFound,
    #[error(transparent)]
    Upload(#[from] UploadError),
    #[error("page file repository is unavailable")]
    Database(#[from] sqlx::Error),
}

/// The same-origin download path of a page file.
#[must_use]
pub fn page_file_url(workspace_id: Id, page_id: Id, file_id: Id) -> String {
    format!("/api/v1/workspaces/{workspace_id}/pages/{page_id}/files/{file_id}")
}

/// Parses a canonical page file path back into `(workspace_id, page_id, file_id)`. Anything that
/// is not exactly what [`page_file_url`] produces (query, fragment, other casing, extra
/// segments) is rejected.
#[must_use]
pub fn parse_page_file_url(url: &str) -> Option<(Id, Id, Id)> {
    let rest = url.strip_prefix("/api/v1/workspaces/")?;
    let mut parts = rest.split('/');
    let workspace_id: Id = parts.next()?.parse().ok()?;
    if parts.next()? != "pages" {
        return None;
    }
    let page_id: Id = parts.next()?.parse().ok()?;
    if parts.next()? != "files" {
        return None;
    }
    let file_id: Id = parts.next()?.parse().ok()?;
    if parts.next().is_some() || page_file_url(workspace_id, page_id, file_id) != url {
        return None;
    }
    Some((workspace_id, page_id, file_id))
}

#[derive(Clone)]
pub struct PageFileRepository {
    database: Database,
    uploads: UploadService,
}

impl PageFileRepository {
    #[must_use]
    pub fn new(database: Database, uploads: UploadService) -> Self {
        Self { database, uploads }
    }

    #[must_use]
    pub fn uploads(&self) -> &UploadService {
        &self.uploads
    }

    /// Succeeds when `actor_id` may attach files to the page: a member of the live workspace and
    /// the page is live and visible to them. Run before reading an upload body.
    pub async fn authorize_upload(
        &self,
        workspace_id: Id,
        page_id: Id,
        actor_id: Id,
    ) -> Result<(), PageFileError> {
        let mut connection = self.database.pool().acquire().await?;
        require_live_page(&mut connection, workspace_id, page_id, actor_id).await
    }

    /// Turns a staged upload into a file of the page (re-checking access in the same
    /// transaction) and records `page.file_added`. The caller discards the staged upload when
    /// this fails.
    pub async fn finalize(
        &self,
        workspace_id: Id,
        page_id: Id,
        actor_id: Id,
        upload: &StagedUpload,
        request_id: &str,
    ) -> Result<PageFile, PageFileError> {
        if upload.workspace_id != workspace_id || upload.owner_id != actor_id {
            return Err(PageFileError::NotFound);
        }
        let finalization = self.uploads.begin_finalization().await;
        let now = self.database.database_now().await?;
        let mut tx = self.database.immediate_transaction().await?;
        require_live_page(&mut tx, workspace_id, page_id, actor_id).await?;
        let blob = finalization
            .finalize_blob_in_transaction(&mut tx, upload, now)
            .await?;
        let id = Id::new_v7();
        let size = i64::try_from(upload.size_bytes).map_err(|_| UploadError::SizeOverflow)?;
        sqlx::query(
            "INSERT INTO page_files (id, workspace_id, page_id, blob_id, file_name, mime_type, \
             size_bytes, uploaded_by, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id.to_string())
        .bind(workspace_id.to_string())
        .bind(page_id.to_string())
        .bind(blob.id.to_string())
        .bind(&upload.display_name)
        .bind(&upload.detected_media_type)
        .bind(size)
        .bind(actor_id.to_string())
        .bind(now.as_millis())
        .execute(&mut *tx)
        .await?;
        audit::record(
            &mut tx,
            workspace_id,
            Some(actor_id),
            "page.file_added",
            AuditOutcome::Success,
            "page",
            Some(page_id),
            request_id,
            json!({"file_id": id}),
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(PageFile {
            id,
            page_id,
            url: page_file_url(workspace_id, page_id, id),
            file_name: upload.display_name.clone(),
            mime_type: upload.detected_media_type.clone(),
            size_bytes: upload.size_bytes,
            created_at: now,
        })
    }

    /// Attaches bytes the server already holds (e.g. a file the Notion importer downloaded) to a
    /// page, acting as `actor_id`. Goes through the same path as an HTTP upload: the configured
    /// size limit, file-name validation, content sniffing (the stored `mime_type` is detected,
    /// never declared), blob dedup, and the page access check. Records `page.file_added`.
    pub async fn create_from_bytes(
        &self,
        workspace_id: Id,
        page_id: Id,
        actor_id: Id,
        file_name: &str,
        bytes: &[u8],
        request_id: &str,
    ) -> Result<PageFile, PageFileError> {
        self.authorize_upload(workspace_id, page_id, actor_id)
            .await?;
        let staged = self
            .uploads
            .stage(workspace_id, actor_id, file_name, bytes)
            .await?;
        match self
            .finalize(workspace_id, page_id, actor_id, &staged, request_id)
            .await
        {
            Ok(file) => Ok(file),
            Err(error) => {
                let _ = self.uploads.discard(&staged).await;
                Err(error)
            }
        }
    }

    /// Opens a page file for `actor_id` with the shared download safety metadata.
    pub async fn download(
        &self,
        workspace_id: Id,
        page_id: Id,
        file_id: Id,
        actor_id: Id,
    ) -> Result<BlobDownload, PageFileError> {
        let database = self.database.clone();
        self.uploads
            .download(move || async move {
                authorized_file(&database, workspace_id, page_id, file_id, actor_id)
                    .await
                    .map_err(|error| match error {
                        PageFileError::Database(error) => UploadError::Database(error),
                        PageFileError::Upload(error) => error,
                        PageFileError::NotFound => UploadError::Unauthorized,
                    })
            })
            .await
            .map_err(|error| match error {
                UploadError::Unauthorized => PageFileError::NotFound,
                error => PageFileError::Upload(error),
            })
    }
}

/// True when the page has a file with this id (used to validate uploaded covers).
pub(super) async fn page_has_file(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    page_id: Id,
    file_id: Id,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM page_files WHERE id = ? AND page_id = ? AND workspace_id = ?)",
    )
    .bind(file_id.to_string())
    .bind(page_id.to_string())
    .bind(workspace_id.to_string())
    .fetch_one(&mut **tx)
    .await
}

async fn require_live_page(
    connection: &mut sqlx::SqliteConnection,
    workspace_id: Id,
    page_id: Id,
    actor_id: Id,
) -> Result<(), PageFileError> {
    let allowed: bool = sqlx::query_scalar(&format!(
        "SELECT EXISTS(SELECT 1 FROM pages \
         JOIN workspaces ON workspaces.id = pages.workspace_id \
         JOIN memberships ON memberships.workspace_id = pages.workspace_id AND memberships.user_id = ? \
         WHERE pages.id = ? AND pages.workspace_id = ? AND pages.deleted_at IS NULL \
         AND workspaces.deleted_at IS NULL AND {VISIBLE})"
    ))
    .bind(actor_id.to_string())
    .bind(page_id.to_string())
    .bind(workspace_id.to_string())
    .bind(actor_id.to_string())
    .fetch_one(connection)
    .await?;
    if allowed {
        Ok(())
    } else {
        Err(PageFileError::NotFound)
    }
}

async fn authorized_file(
    database: &Database,
    workspace_id: Id,
    page_id: Id,
    file_id: Id,
    actor_id: Id,
) -> Result<AuthorizedAttachment, PageFileError> {
    let row = sqlx::query(&format!(
        "SELECT attachment_blobs.storage_key AS storage_key, page_files.mime_type AS mime_type, \
         page_files.file_name AS file_name FROM page_files \
         JOIN attachment_blobs ON attachment_blobs.id = page_files.blob_id \
         AND attachment_blobs.workspace_id = page_files.workspace_id \
         JOIN pages ON pages.id = page_files.page_id AND pages.workspace_id = page_files.workspace_id \
         JOIN workspaces ON workspaces.id = page_files.workspace_id \
         JOIN memberships ON memberships.workspace_id = page_files.workspace_id AND memberships.user_id = ? \
         WHERE page_files.id = ? AND page_files.page_id = ? AND page_files.workspace_id = ? \
         AND pages.deleted_at IS NULL AND workspaces.deleted_at IS NULL AND {VISIBLE}"
    ))
    .bind(actor_id.to_string())
    .bind(file_id.to_string())
    .bind(page_id.to_string())
    .bind(workspace_id.to_string())
    .bind(actor_id.to_string())
    .fetch_optional(database.pool())
    .await?
    .ok_or(PageFileError::NotFound)?;
    Ok(AuthorizedAttachment {
        storage_key: row.try_get("storage_key")?,
        media_type: row.try_get("mime_type")?,
        display_name: row.try_get("file_name")?,
    })
}

#[cfg(test)]
mod tests {
    use orbit_platform::Id;

    use super::{page_file_url, parse_page_file_url};

    #[test]
    fn page_file_urls_round_trip_strictly() {
        let [workspace, page, file]: [Id; 3] = std::array::from_fn(|_| Id::new_v7());
        let url = page_file_url(workspace, page, file);
        assert_eq!(parse_page_file_url(&url), Some((workspace, page, file)));
        for bad in [
            format!("{url}?x=1"),
            format!("{url}/"),
            format!("{url}#a"),
            url.to_uppercase(),
            format!("https://example.com{url}"),
            format!("/api/v1/workspaces/{workspace}/tasks/{page}/files/{file}"),
            format!("/api/v1/workspaces/{workspace}/pages/{page}/files/not-an-id"),
            format!("//api/v1/workspaces/{workspace}/pages/{page}/files/{file}"),
        ] {
            assert_eq!(parse_page_file_url(&bad), None, "{bad}");
        }
    }
}
