//! Project milestones: named targets that tasks point at, ordered by `position`.

use orbit_platform::{Id, Row, TimestampMillis};
use serde::Serialize;
use sqlx::{Any, Transaction};
use utoipa::ToSchema;

use super::{
    TaskError, TaskRepository, check_version, is_unique_violation, parse_id, parse_version,
    plan_progress_sql, record_mutation, require_project, require_project_tx,
};

#[derive(Clone, Debug, Serialize, ToSchema)]
#[schema(as = TaskMilestone)]
pub struct MilestoneRecord {
    #[schema(value_type = String)]
    pub id: Id,
    #[schema(value_type = String)]
    pub workspace_id: Id,
    #[schema(value_type = String)]
    pub project_id: Id,
    pub name: String,
    #[schema(required = true)]
    pub description: Option<String>,
    #[schema(value_type = Option<String>, format = DateTime, required = true)]
    pub target_at: Option<TimestampMillis>,
    pub position: i64,
    pub version: u64,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: TimestampMillis,
    #[schema(value_type = String, format = DateTime)]
    pub updated_at: TimestampMillis,
    /// Live tasks in the milestone; cancelled and duplicate tasks are not counted.
    pub task_count: i64,
    /// Of `task_count`, the tasks in a completed status.
    pub completed_count: i64,
    /// The estimates of `task_count`'s tasks, summed.
    pub estimate_total: i64,
    /// The estimates of `completed_count`'s tasks, summed.
    pub estimate_completed: i64,
}

/// A milestone PATCH: `None` keeps a field; `Some(None)` clears a nullable one.
#[derive(Clone, Debug, Default)]
pub struct MilestoneChanges {
    pub name: Option<String>,
    pub description: Option<Option<String>>,
    pub target_at: Option<Option<TimestampMillis>>,
    pub position: Option<i64>,
}

fn select_sql(filter: &'static str) -> String {
    let (progress, join) = plan_progress_sql("task_milestones", "milestone_id");
    format!(
        "SELECT task_milestones.id, task_milestones.workspace_id, task_milestones.project_id, \
         task_milestones.name, task_milestones.description, task_milestones.target_at, \
         task_milestones.position, task_milestones.version, task_milestones.created_at, \
         task_milestones.updated_at, {progress} FROM task_milestones {join} \
         WHERE task_milestones.workspace_id = $1 AND task_milestones.project_id = $2 {filter} \
         GROUP BY task_milestones.id ORDER BY task_milestones.position, task_milestones.id"
    )
}

impl TaskRepository {
    /// Every milestone of the project, in `position` order.
    pub async fn milestones(
        &self,
        workspace_id: Id,
        project_id: Id,
        actor_id: Id,
    ) -> Result<Vec<MilestoneRecord>, TaskError> {
        require_project(self.database.pool(), workspace_id, project_id, actor_id).await?;
        sqlx::query(sqlx::AssertSqlSafe(select_sql("")))
            .bind(workspace_id.to_string())
            .bind(project_id.to_string())
            .fetch_all(self.database.pool())
            .await?
            .into_iter()
            .map(milestone_from_row)
            .collect()
    }

    /// Adds a milestone after the project's others. A name the project already has is a
    /// `Conflict`.
    #[allow(clippy::too_many_arguments)]
    pub async fn create_milestone(
        &self,
        workspace_id: Id,
        project_id: Id,
        actor_id: Id,
        name: String,
        description: Option<String>,
        target_at: Option<TimestampMillis>,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<MilestoneRecord, TaskError> {
        let id = Id::new_v7();
        let mut tx = self.database.immediate_transaction().await?;
        require_project_tx(&mut tx, workspace_id, project_id, actor_id).await?;
        let inserted = sqlx::query(
            "INSERT INTO task_milestones (id, workspace_id, project_id, name, description, target_at, position, version, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6, \
             (SELECT COALESCE(MAX(position) + 1, 0) FROM task_milestones WHERE project_id = $7), 0, $8, $9)",
        )
        .bind(id.to_string())
        .bind(workspace_id.to_string())
        .bind(project_id.to_string())
        .bind(&name)
        .bind(&description)
        .bind(target_at.map(TimestampMillis::as_millis))
        .bind(project_id.to_string())
        .bind(now.as_millis())
        .bind(now.as_millis())
        .execute(&mut *tx)
        .await;
        if is_unique_violation(&inserted) {
            return Err(TaskError::Conflict);
        }
        inserted?;
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "milestone.created",
            "milestone",
            id,
            request_id,
            now,
        )
        .await?;
        let record = milestone_in_tx(&mut tx, workspace_id, project_id, id).await?;
        tx.commit().await?;
        Ok(record)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update_milestone(
        &self,
        workspace_id: Id,
        project_id: Id,
        milestone_id: Id,
        actor_id: Id,
        changes: MilestoneChanges,
        expected_version: u64,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<MilestoneRecord, TaskError> {
        let mut tx = self.database.immediate_transaction().await?;
        require_project_tx(&mut tx, workspace_id, project_id, actor_id).await?;
        let current = milestone_in_tx(&mut tx, workspace_id, project_id, milestone_id).await?;
        check_version(expected_version, current.version, &current)?;
        let updated = sqlx::query(
            "UPDATE task_milestones SET name = $1, description = $2, target_at = $3, position = $4, \
             version = version + 1, updated_at = $5 WHERE id = $6 AND project_id = $7 AND version = $8",
        )
        .bind(changes.name.unwrap_or(current.name))
        .bind(changes.description.unwrap_or(current.description))
        .bind(
            changes
                .target_at
                .unwrap_or(current.target_at)
                .map(TimestampMillis::as_millis),
        )
        .bind(changes.position.unwrap_or(current.position))
        .bind(now.as_millis())
        .bind(milestone_id.to_string())
        .bind(project_id.to_string())
        .bind(expected_version as i64)
        .execute(&mut *tx)
        .await;
        if is_unique_violation(&updated) {
            return Err(TaskError::Conflict);
        }
        updated?;
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "milestone.updated",
            "milestone",
            milestone_id,
            request_id,
            now,
        )
        .await?;
        let record = milestone_in_tx(&mut tx, workspace_id, project_id, milestone_id).await?;
        tx.commit().await?;
        Ok(record)
    }

    /// Deletes the milestone; its tasks keep going without one (`ON DELETE SET NULL`).
    #[allow(clippy::too_many_arguments)]
    pub async fn delete_milestone(
        &self,
        workspace_id: Id,
        project_id: Id,
        milestone_id: Id,
        actor_id: Id,
        expected_version: u64,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), TaskError> {
        let mut tx = self.database.immediate_transaction().await?;
        require_project_tx(&mut tx, workspace_id, project_id, actor_id).await?;
        let current = milestone_in_tx(&mut tx, workspace_id, project_id, milestone_id).await?;
        check_version(expected_version, current.version, &current)?;
        sqlx::query(
            "DELETE FROM task_milestones WHERE id = $1 AND project_id = $2 AND version = $3",
        )
        .bind(milestone_id.to_string())
        .bind(project_id.to_string())
        .bind(expected_version as i64)
        .execute(&mut *tx)
        .await?;
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "milestone.deleted",
            "milestone",
            milestone_id,
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Sets each `(id, expected_version, position)`; returns the project's milestones in the new
    /// order.
    pub async fn reorder_milestones(
        &self,
        workspace_id: Id,
        project_id: Id,
        actor_id: Id,
        items: &[(Id, u64, i64)],
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<Vec<MilestoneRecord>, TaskError> {
        let mut tx = self.database.immediate_transaction().await?;
        require_project_tx(&mut tx, workspace_id, project_id, actor_id).await?;
        for &(id, expected, position) in items {
            let current = milestone_in_tx(&mut tx, workspace_id, project_id, id).await?;
            check_version(expected, current.version, &current)?;
            sqlx::query(
                "UPDATE task_milestones SET position = $1, version = version + 1, updated_at = $2 \
                 WHERE id = $3 AND project_id = $4 AND version = $5",
            )
            .bind(position)
            .bind(now.as_millis())
            .bind(id.to_string())
            .bind(project_id.to_string())
            .bind(expected as i64)
            .execute(&mut *tx)
            .await?;
        }
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "milestone.reordered",
            "project",
            project_id,
            request_id,
            now,
        )
        .await?;
        let records = sqlx::query(sqlx::AssertSqlSafe(select_sql("")))
            .bind(workspace_id.to_string())
            .bind(project_id.to_string())
            .fetch_all(&mut *tx)
            .await?
            .into_iter()
            .map(milestone_from_row)
            .collect::<Result<Vec<_>, _>>()?;
        tx.commit().await?;
        Ok(records)
    }
}

/// The milestone of `project_id` with the name of `milestone_id` (case-insensitive, the exact
/// name first), for a task that moves there; `None` when the project has no such milestone.
pub(super) async fn matching_milestone_in_tx(
    tx: &mut Transaction<'_, Any>,
    milestone_id: Id,
    project_id: Id,
) -> Result<Option<Id>, TaskError> {
    sqlx::query_scalar::<_, String>(
        "SELECT target.id FROM task_milestones AS source \
         JOIN task_milestones AS target ON lower(trim(target.name)) = lower(trim(source.name)) \
         AND target.workspace_id = source.workspace_id \
         WHERE source.id = $1 AND target.project_id = $2 \
         ORDER BY CASE WHEN target.name = source.name THEN 0 ELSE 1 END, target.id LIMIT 1",
    )
    .bind(milestone_id.to_string())
    .bind(project_id.to_string())
    .fetch_optional(&mut **tx)
    .await?
    .map(parse_id)
    .transpose()
}

async fn milestone_in_tx(
    tx: &mut Transaction<'_, Any>,
    workspace_id: Id,
    project_id: Id,
    milestone_id: Id,
) -> Result<MilestoneRecord, TaskError> {
    let row = sqlx::query(sqlx::AssertSqlSafe(select_sql(
        "AND task_milestones.id = $3",
    )))
    .bind(workspace_id.to_string())
    .bind(project_id.to_string())
    .bind(milestone_id.to_string())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(TaskError::NotFound)?;
    milestone_from_row(row)
}

fn milestone_from_row(row: sqlx::any::AnyRow) -> Result<MilestoneRecord, TaskError> {
    Ok(MilestoneRecord {
        id: parse_id(row.get("id"))?,
        workspace_id: parse_id(row.get("workspace_id"))?,
        project_id: parse_id(row.get("project_id"))?,
        name: row.get("name"),
        description: row.get("description"),
        target_at: row
            .get::<Option<i64>, _>("target_at")
            .map(TimestampMillis::from_millis),
        position: row.get("position"),
        version: parse_version(row.get("version"))?,
        created_at: TimestampMillis::from_millis(row.get("created_at")),
        updated_at: TimestampMillis::from_millis(row.get("updated_at")),
        task_count: row.get("task_count"),
        completed_count: row.get("completed_count"),
        estimate_total: row.get("estimate_total"),
        estimate_completed: row.get("estimate_completed"),
    })
}
