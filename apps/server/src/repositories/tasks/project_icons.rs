//! A project's own icon image (issue #146). The image is an attachment blob of the project's workspace, so it goes
//! to S3 with all other files when S3 is on; reconcile deletes a replaced or removed one (BLOB_REFERENCE_COUNT).

use orbit_platform::{BlobStoreError, Id, Row, TimestampMillis, UploadError, UploadService};
use serde_json::json;
use thiserror::Error;

use super::{
    ProjectRecord, ProjectScope, TaskError, TaskRepository, project_in_tx, require_access_tx,
    require_manager,
};

/// The bytes of a project icon and their type (`image/png`, `image/jpeg` or `image/webp`).
pub struct ProjectIcon {
    pub mime_type: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Error)]
pub enum ProjectIconError {
    #[error(transparent)]
    Task(#[from] TaskError),
    #[error(transparent)]
    Upload(#[from] UploadError),
}

impl From<sqlx::Error> for ProjectIconError {
    fn from(error: sqlx::Error) -> Self {
        Self::Task(TaskError::Unavailable(error))
    }
}

/// The URL of a project's icon; `updated_at` is when it last changed (`None` while there is none), so a new image
/// is a new URL.
#[must_use]
pub fn project_icon_url(
    workspace_id: Id,
    project_id: Id,
    updated_at: Option<i64>,
) -> Option<String> {
    updated_at.map(|version| {
        format!("/api/v1/workspaces/{workspace_id}/projects/{project_id}/icon?v={version}")
    })
}

impl TaskRepository {
    /// Replaces the icon of a live project. Only a manager of the project may change it. Callers check the
    /// format and size first; `mime_type` is what they read from the bytes.
    #[allow(clippy::too_many_arguments)]
    pub async fn set_project_icon(
        &self,
        uploads: &UploadService,
        workspace_id: Id,
        project_id: Id,
        actor_id: Id,
        mime_type: &'static str,
        bytes: &[u8],
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<ProjectRecord, ProjectIconError> {
        let finalization = uploads.begin_finalization().await;
        // Checked before the bytes are stored, so a refused request stores nothing, and again in the transaction,
        // which decides.
        {
            let mut tx = self.database.pool().begin().await?;
            manageable_project(&mut tx, workspace_id, project_id, actor_id).await?;
        }
        let prepared = finalization.prepare_bytes(workspace_id, bytes).await?;
        let mut tx = self.database.immediate_transaction().await?;
        let project = manageable_project(&mut tx, workspace_id, project_id, actor_id).await?;
        let blob = finalization
            .finalize_bytes_in_transaction(&mut tx, &prepared)
            .await?;
        sqlx::query(
            "INSERT INTO project_icons (project_id, workspace_id, mime_type, blob_id, updated_at) \
             VALUES ($1, $2, $3, $4, $5) ON CONFLICT (project_id) DO UPDATE SET \
             mime_type = excluded.mime_type, blob_id = excluded.blob_id, updated_at = excluded.updated_at",
        )
        .bind(project_id.to_string())
        .bind(workspace_id.to_string())
        .bind(mime_type)
        .bind(blob.id.to_string())
        .bind(now.as_millis())
        .execute(&mut *tx)
        .await?;
        self.record_write(
            &mut tx,
            workspace_id,
            actor_id,
            ("project.updated", "project"),
            project_id,
            request_id,
            json!({ "changes": { "icon": { "to": "image" } } }),
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(ProjectRecord {
            icon_url: project_icon_url(workspace_id, project_id, Some(now.as_millis())),
            ..project
        })
    }

    /// Removes the icon of a live project; nothing changes when it has none.
    pub async fn remove_project_icon(
        &self,
        workspace_id: Id,
        project_id: Id,
        actor_id: Id,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<ProjectRecord, ProjectIconError> {
        let mut tx = self.database.immediate_transaction().await?;
        let project = manageable_project(&mut tx, workspace_id, project_id, actor_id).await?;
        let removed = sqlx::query("DELETE FROM project_icons WHERE project_id = $1")
            .bind(project_id.to_string())
            .execute(&mut *tx)
            .await?
            .rows_affected();
        if removed > 0 {
            self.record_write(
                &mut tx,
                workspace_id,
                actor_id,
                ("project.updated", "project"),
                project_id,
                request_id,
                json!({ "changes": { "icon": { "to": null } } }),
                now,
            )
            .await?;
        }
        tx.commit().await?;
        Ok(ProjectRecord {
            icon_url: None,
            ..project
        })
    }

    /// The icon of a project, for everyone who sees the project (trashed projects included, so the trash can show
    /// it). An icon whose file is in no storage tier (for example, deleted from the bucket by hand) is removed, so
    /// the project shows its colour until someone uploads a new one.
    pub async fn project_icon(
        &self,
        uploads: &UploadService,
        workspace_id: Id,
        project_id: Id,
        actor_id: Id,
    ) -> Result<Option<ProjectIcon>, ProjectIconError> {
        let actor = super::require_access(self.database.pool(), workspace_id, actor_id).await?;
        let row = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT i.mime_type, i.blob_id, b.storage_key FROM project_icons i \
             JOIN projects ON projects.id = i.project_id \
             LEFT JOIN attachment_blobs b ON b.id = i.blob_id \
             WHERE i.project_id = $1 AND i.workspace_id = $2 AND {}",
            ProjectScope::of(&actor).project("projects.id")
        )))
        .bind(project_id.to_string())
        .bind(workspace_id.to_string())
        .fetch_optional(self.database.pool())
        .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        if let Some(storage_key) = row.get::<Option<String>, _>("storage_key") {
            match uploads.read_blob(&storage_key).await {
                Ok(bytes) => {
                    return Ok(Some(ProjectIcon {
                        mime_type: row.get("mime_type"),
                        bytes,
                    }));
                }
                Err(UploadError::BlobStore(BlobStoreError::Io { source, .. }))
                    if source.kind() == std::io::ErrorKind::NotFound => {}
                // A bucket that is down is not a missing file: keep the icon.
                Err(error) => return Err(error.into()),
            }
        }
        tracing::warn!(%project_id, "project icon file is missing; removing the icon");
        // The same blob only: an icon uploaded meanwhile stays.
        sqlx::query("DELETE FROM project_icons WHERE project_id = $1 AND blob_id = $2")
            .bind(project_id.to_string())
            .bind(row.get::<String, _>("blob_id"))
            .execute(self.database.pool())
            .await?;
        Ok(None)
    }
}

/// The live project, when the actor sees it and manages it.
async fn manageable_project(
    tx: &mut sqlx::Transaction<'_, sqlx::Any>,
    workspace_id: Id,
    project_id: Id,
    actor_id: Id,
) -> Result<ProjectRecord, TaskError> {
    let actor = require_access_tx(tx, workspace_id, actor_id).await?;
    let project = project_in_tx(
        tx,
        ProjectScope::of(&actor),
        workspace_id,
        project_id,
        false,
    )
    .await?;
    require_manager(&actor, project_id)?;
    Ok(project)
}
