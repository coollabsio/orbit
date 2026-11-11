//! Who sees a private project (#37). Whoever sees a project may read and change its member list,
//! as with private chat channels. The list only matters while the project is private; it is kept
//! when the project opens to the workspace again.

use orbit_platform::{Id, Row, TimestampMillis};
use serde::Serialize;
use serde_json::json;
use sqlx::{Any, Transaction};
use utoipa::ToSchema;

use super::membership;
use super::project_access::{self, ProjectScope};
use super::tasks::{TaskError, TaskRepository, parse_id, project_in_tx, require_access_tx};
use crate::audit::{self, AuditOutcome};
use crate::collab::CollabHub;

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct ProjectMemberRecord {
    #[schema(value_type = String)]
    pub project_id: Id,
    #[schema(value_type = String)]
    pub user_id: Id,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: TimestampMillis,
}

/// Adds `user_id` to the project; a no-op for someone already in it. Returns whether a row was
/// added.
pub(super) async fn insert_in_tx(
    tx: &mut Transaction<'_, Any>,
    workspace_id: Id,
    project_id: Id,
    user_id: Id,
    now: TimestampMillis,
) -> Result<bool, TaskError> {
    let inserted = sqlx::query(
        "INSERT INTO project_members (project_id, workspace_id, user_id, created_at) \
         VALUES ($1, $2, $3, $4) ON CONFLICT (project_id, user_id) DO NOTHING",
    )
    .bind(project_id.to_string())
    .bind(workspace_id.to_string())
    .bind(user_id.to_string())
    .bind(now.as_millis())
    .execute(&mut **tx)
    .await?;
    Ok(inserted.rows_affected() == 1)
}

async fn member_in_tx(
    tx: &mut Transaction<'_, Any>,
    project_id: Id,
    user_id: Id,
) -> Result<Option<ProjectMemberRecord>, TaskError> {
    sqlx::query(
        "SELECT project_id, user_id, created_at FROM project_members WHERE project_id = $1 AND user_id = $2",
    )
    .bind(project_id.to_string())
    .bind(user_id.to_string())
    .fetch_optional(&mut **tx)
    .await?
    .map(member_from_row)
    .transpose()
}

fn member_from_row(row: sqlx::any::AnyRow) -> Result<ProjectMemberRecord, TaskError> {
    Ok(ProjectMemberRecord {
        project_id: parse_id(row.get("project_id"))?,
        user_id: parse_id(row.get("user_id"))?,
        created_at: TimestampMillis::from_millis(row.get("created_at")),
    })
}

impl TaskRepository {
    /// The members of the live project `project_id`, oldest first. Owners and admins see the
    /// project without being listed.
    pub async fn project_members(
        &self,
        workspace_id: Id,
        project_id: Id,
        actor_id: Id,
    ) -> Result<Vec<ProjectMemberRecord>, TaskError> {
        let mut tx = self.database().pool().begin().await?;
        let scope = ProjectScope::of(require_access_tx(&mut tx, workspace_id, actor_id).await?);
        project_in_tx(&mut tx, scope, workspace_id, project_id, false).await?;
        sqlx::query(
            "SELECT project_id, user_id, created_at FROM project_members WHERE project_id = $1 \
             ORDER BY created_at, user_id",
        )
        .bind(project_id.to_string())
        .fetch_all(&mut *tx)
        .await?
        .into_iter()
        .map(member_from_row)
        .collect()
    }

    /// Adds an active workspace member (not a guest) to the project. Adding someone who is
    /// already in it returns their row and records nothing.
    pub async fn add_project_member(
        &self,
        workspace_id: Id,
        project_id: Id,
        actor_id: Id,
        user_id: Id,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<ProjectMemberRecord, TaskError> {
        let mut tx = self.database().immediate_transaction().await?;
        let scope = ProjectScope::of(require_access_tx(&mut tx, workspace_id, actor_id).await?);
        project_in_tx(&mut tx, scope, workspace_id, project_id, false).await?;
        // Guests and people outside the workspace see no project, listed or not.
        if membership::actor(&mut *tx, workspace_id, user_id)
            .await?
            .is_none()
        {
            return Err(TaskError::Invalid { field: "user_id" });
        }
        let inserted = insert_in_tx(&mut tx, workspace_id, project_id, user_id, now).await?;
        // Listed now, so this only fails for a suspended account (the insert rolls back).
        if !project_access::user_sees_project(&mut *tx, project_id, user_id).await? {
            return Err(TaskError::Invalid { field: "user_id" });
        }
        if inserted {
            audit::record(
                &mut tx,
                workspace_id,
                Some(actor_id),
                "project.member_added",
                AuditOutcome::Success,
                "project",
                Some(project_id),
                request_id,
                json!({ "user_id": user_id }),
                now,
            )
            .await?;
        }
        let member = member_in_tx(&mut tx, project_id, user_id)
            .await?
            .ok_or(TaskError::Conflict)?;
        tx.commit().await?;
        Ok(member)
    }

    /// Removes a member; someone who is not in the project is `NotFound`. A member may remove
    /// themselves and then no longer sees a private project.
    pub async fn remove_project_member(
        &self,
        workspace_id: Id,
        project_id: Id,
        actor_id: Id,
        user_id: Id,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), TaskError> {
        let mut tx = self.database().immediate_transaction().await?;
        let scope = ProjectScope::of(require_access_tx(&mut tx, workspace_id, actor_id).await?);
        project_in_tx(&mut tx, scope, workspace_id, project_id, false).await?;
        let removed =
            sqlx::query("DELETE FROM project_members WHERE project_id = $1 AND user_id = $2")
                .bind(project_id.to_string())
                .bind(user_id.to_string())
                .execute(&mut *tx)
                .await?;
        if removed.rows_affected() != 1 {
            return Err(TaskError::NotFound);
        }
        audit::record(
            &mut tx,
            workspace_id,
            Some(actor_id),
            "project.member_removed",
            AuditOutcome::Success,
            "project",
            Some(project_id),
            request_id,
            json!({ "user_id": user_id }),
            now,
        )
        .await?;
        tx.commit().await?;
        // Open task descriptions re-check who may read them.
        CollabHub::revalidate_database(self.database(), Some(workspace_id));
        Ok(())
    }
}
