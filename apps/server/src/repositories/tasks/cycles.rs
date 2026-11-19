//! Project cycles: numbered, non-overlapping time boxes `[starts_at, ends_at)` that tasks point
//! at. Which cycle is current, next or previous follows from the dates; nothing is stored.

use orbit_platform::{Flag, Id, Row, TimestampMillis};
use serde::Serialize;
use serde_json::json;
use sqlx::{Any, Transaction};
use utoipa::ToSchema;

use super::{
    TaskError, TaskRepository, check_version, parse_id, parse_version, plan_progress_sql,
    record_mutation, require_project, require_project_tx,
};

#[derive(Clone, Debug, Serialize, ToSchema)]
#[schema(as = TaskCycle)]
pub struct CycleRecord {
    #[schema(value_type = String)]
    pub id: Id,
    #[schema(value_type = String)]
    pub workspace_id: Id,
    #[schema(value_type = String)]
    pub project_id: Id,
    /// 1 for the project's first cycle, then one more than the highest so far.
    pub number: i64,
    #[schema(required = true)]
    pub name: Option<String>,
    #[schema(value_type = String, format = DateTime)]
    pub starts_at: TimestampMillis,
    /// The end, exclusive: the next cycle may start at this instant.
    #[schema(value_type = String, format = DateTime)]
    pub ends_at: TimestampMillis,
    pub version: u64,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: TimestampMillis,
    #[schema(value_type = String, format = DateTime)]
    pub updated_at: TimestampMillis,
    /// Live tasks in the cycle; cancelled and duplicate tasks are not counted.
    pub task_count: i64,
    /// Of `task_count`, the tasks in a completed status.
    pub completed_count: i64,
    /// The estimates of `task_count`'s tasks, summed.
    pub estimate_total: i64,
    /// The estimates of `completed_count`'s tasks, summed.
    pub estimate_completed: i64,
    /// Live tasks that "move unfinished tasks" took out of this cycle and that are not back in it.
    /// They are not in `task_count`, so a past cycle still shows the work it did not finish.
    pub carried_over_count: i64,
}

/// A cycle PATCH: `None` keeps a field; `Some(None)` clears the name.
#[derive(Clone, Debug, Default)]
pub struct CycleChanges {
    pub name: Option<Option<String>>,
    pub starts_at: Option<TimestampMillis>,
    pub ends_at: Option<TimestampMillis>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[schema(as = TaskCycleMoveOutcome)]
pub struct CycleMoveOutcome {
    /// How many tasks moved.
    pub moved: i64,
    /// The cycle they moved to, with its new counts.
    pub to_cycle: CycleRecord,
}

fn select_sql(filter: &'static str) -> String {
    let (progress, join) = plan_progress_sql("task_cycles", "cycle_id");
    format!(
        "SELECT task_cycles.id, task_cycles.workspace_id, task_cycles.project_id, task_cycles.number, \
         task_cycles.name, task_cycles.starts_at, task_cycles.ends_at, task_cycles.version, \
         task_cycles.created_at, task_cycles.updated_at, {progress}, \
         (SELECT CAST(COUNT(*) AS BIGINT) FROM task_cycle_carryovers AS carry \
         JOIN tasks AS carry_task ON carry_task.id = carry.task_id \
         WHERE carry.cycle_id = task_cycles.id AND carry_task.deleted_at IS NULL \
         AND carry_task.cycle_id IS DISTINCT FROM task_cycles.id) AS carried_over_count \
         FROM task_cycles {join} \
         WHERE task_cycles.workspace_id = $1 AND task_cycles.project_id = $2 {filter} \
         GROUP BY task_cycles.id ORDER BY task_cycles.starts_at, task_cycles.id"
    )
}

impl TaskRepository {
    /// Every cycle of the project, by start date.
    pub async fn cycles(
        &self,
        workspace_id: Id,
        project_id: Id,
        actor_id: Id,
    ) -> Result<Vec<CycleRecord>, TaskError> {
        require_project(self.database.pool(), workspace_id, project_id, actor_id).await?;
        sqlx::query(sqlx::AssertSqlSafe(select_sql("")))
            .bind(workspace_id.to_string())
            .bind(project_id.to_string())
            .fetch_all(self.database.pool())
            .await?
            .into_iter()
            .map(cycle_from_row)
            .collect()
    }

    /// Adds the project's next-numbered cycle. `CycleOverlap` when its dates overlap another.
    #[allow(clippy::too_many_arguments)]
    pub async fn create_cycle(
        &self,
        workspace_id: Id,
        project_id: Id,
        actor_id: Id,
        name: Option<String>,
        starts_at: TimestampMillis,
        ends_at: TimestampMillis,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<CycleRecord, TaskError> {
        validate_dates(starts_at, ends_at)?;
        let id = Id::new_v7();
        let mut tx = self.database.immediate_transaction().await?;
        require_project_tx(&mut tx, workspace_id, project_id, actor_id).await?;
        reject_overlap(&mut tx, project_id, None, starts_at, ends_at).await?;
        sqlx::query(
            "INSERT INTO task_cycles (id, workspace_id, project_id, number, name, starts_at, ends_at, version, created_at, updated_at) \
             VALUES ($1, $2, $3, (SELECT COALESCE(MAX(number), 0) + 1 FROM task_cycles WHERE project_id = $4), \
             $5, $6, $7, 0, $8, $9)",
        )
        .bind(id.to_string())
        .bind(workspace_id.to_string())
        .bind(project_id.to_string())
        .bind(project_id.to_string())
        .bind(&name)
        .bind(starts_at.as_millis())
        .bind(ends_at.as_millis())
        .bind(now.as_millis())
        .bind(now.as_millis())
        .execute(&mut *tx)
        .await?;
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "cycle.created",
            "cycle",
            id,
            request_id,
            now,
        )
        .await?;
        let record = cycle_in_tx(&mut tx, workspace_id, project_id, id).await?;
        tx.commit().await?;
        Ok(record)
    }

    /// For imports: the project's cycle numbered `number` with exactly these dates, else a new
    /// one with that number and these dates. `None` when that number is taken by a cycle with
    /// other dates (a hand-made one), or the number or dates are invalid or overlap another
    /// cycle; then nothing is written. Every check runs before the insert, so the transaction
    /// never sees a failed statement (which aborts it on Postgres).
    #[allow(clippy::too_many_arguments)]
    pub async fn find_or_create_cycle(
        &self,
        workspace_id: Id,
        project_id: Id,
        actor_id: Id,
        number: i64,
        name: Option<String>,
        starts_at: TimestampMillis,
        ends_at: TimestampMillis,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<Option<Id>, TaskError> {
        let mut tx = self.database.immediate_transaction().await?;
        require_project_tx(&mut tx, workspace_id, project_id, actor_id).await?;
        let existing: Option<(String, i64, i64)> = sqlx::query_as(
            "SELECT id, starts_at, ends_at FROM task_cycles WHERE project_id = $1 AND number = $2",
        )
        .bind(project_id.to_string())
        .bind(number)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some((id, existing_starts_at, existing_ends_at)) = existing {
            let same_dates = existing_starts_at == starts_at.as_millis()
                && existing_ends_at == ends_at.as_millis();
            return if same_dates {
                parse_id(id).map(Some)
            } else {
                Ok(None)
            };
        }
        if number < 1 || validate_dates(starts_at, ends_at).is_err() {
            return Ok(None);
        }
        match reject_overlap(&mut tx, project_id, None, starts_at, ends_at).await {
            Err(TaskError::CycleOverlap) => return Ok(None),
            result => result?,
        }
        let id = Id::new_v7();
        sqlx::query(
            "INSERT INTO task_cycles (id, workspace_id, project_id, number, name, starts_at, ends_at, version, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, 0, $8, $9)",
        )
        .bind(id.to_string())
        .bind(workspace_id.to_string())
        .bind(project_id.to_string())
        .bind(number)
        .bind(&name)
        .bind(starts_at.as_millis())
        .bind(ends_at.as_millis())
        .bind(now.as_millis())
        .bind(now.as_millis())
        .execute(&mut *tx)
        .await?;
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "cycle.created",
            "cycle",
            id,
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(Some(id))
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update_cycle(
        &self,
        workspace_id: Id,
        project_id: Id,
        cycle_id: Id,
        actor_id: Id,
        changes: CycleChanges,
        expected_version: u64,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<CycleRecord, TaskError> {
        let mut tx = self.database.immediate_transaction().await?;
        require_project_tx(&mut tx, workspace_id, project_id, actor_id).await?;
        let current = cycle_in_tx(&mut tx, workspace_id, project_id, cycle_id).await?;
        check_version(expected_version, current.version, &current)?;
        let starts_at = changes.starts_at.unwrap_or(current.starts_at);
        let ends_at = changes.ends_at.unwrap_or(current.ends_at);
        validate_dates(starts_at, ends_at)?;
        reject_overlap(&mut tx, project_id, Some(cycle_id), starts_at, ends_at).await?;
        sqlx::query(
            "UPDATE task_cycles SET name = $1, starts_at = $2, ends_at = $3, version = version + 1, \
             updated_at = $4 WHERE id = $5 AND project_id = $6 AND version = $7",
        )
        .bind(changes.name.unwrap_or(current.name))
        .bind(starts_at.as_millis())
        .bind(ends_at.as_millis())
        .bind(now.as_millis())
        .bind(cycle_id.to_string())
        .bind(project_id.to_string())
        .bind(expected_version as i64)
        .execute(&mut *tx)
        .await?;
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "cycle.updated",
            "cycle",
            cycle_id,
            request_id,
            now,
        )
        .await?;
        let record = cycle_in_tx(&mut tx, workspace_id, project_id, cycle_id).await?;
        tx.commit().await?;
        Ok(record)
    }

    /// Deletes the cycle; its tasks keep going without one (`ON DELETE SET NULL`). Numbers are
    /// not reused unless the deleted cycle had the highest.
    #[allow(clippy::too_many_arguments)]
    pub async fn delete_cycle(
        &self,
        workspace_id: Id,
        project_id: Id,
        cycle_id: Id,
        actor_id: Id,
        expected_version: u64,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), TaskError> {
        let mut tx = self.database.immediate_transaction().await?;
        require_project_tx(&mut tx, workspace_id, project_id, actor_id).await?;
        let current = cycle_in_tx(&mut tx, workspace_id, project_id, cycle_id).await?;
        check_version(expected_version, current.version, &current)?;
        sqlx::query("DELETE FROM task_cycles WHERE id = $1 AND project_id = $2 AND version = $3")
            .bind(cycle_id.to_string())
            .bind(project_id.to_string())
            .bind(expected_version as i64)
            .execute(&mut *tx)
            .await?;
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "cycle.deleted",
            "cycle",
            cycle_id,
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Moves the cycle's live tasks that are not completed, cancelled or duplicates to the next
    /// cycle (the first that starts at or after this one's end). Each moved task gets a new
    /// version, a `task.updated` activity entry and a `task_cycle_carryovers` row, which keeps it in
    /// this cycle's `carried_over_count`. `NoNextCycle` when there is none.
    pub async fn move_unfinished_tasks(
        &self,
        workspace_id: Id,
        project_id: Id,
        cycle_id: Id,
        actor_id: Id,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<CycleMoveOutcome, TaskError> {
        let mut tx = self.database.immediate_transaction().await?;
        require_project_tx(&mut tx, workspace_id, project_id, actor_id).await?;
        let current = cycle_in_tx(&mut tx, workspace_id, project_id, cycle_id).await?;
        let next_id = sqlx::query_scalar::<_, String>(
            "SELECT id FROM task_cycles WHERE project_id = $1 AND starts_at >= $2 \
             ORDER BY starts_at, id LIMIT 1",
        )
        .bind(project_id.to_string())
        .bind(current.ends_at.as_millis())
        .fetch_optional(&mut *tx)
        .await?
        .map(parse_id)
        .transpose()?
        .ok_or(TaskError::NoNextCycle)?;
        let moved = sqlx::query_scalar::<_, String>(
            "UPDATE tasks SET cycle_id = $1, version = version + 1, updated_at = $2 \
             WHERE workspace_id = $3 AND cycle_id = $4 AND deleted_at IS NULL \
             AND status_id IN (SELECT id FROM task_statuses WHERE project_id = $5 \
             AND category NOT IN ('completed', 'cancelled', 'duplicate')) RETURNING id",
        )
        .bind(next_id.to_string())
        .bind(now.as_millis())
        .bind(workspace_id.to_string())
        .bind(cycle_id.to_string())
        .bind(project_id.to_string())
        .fetch_all(&mut *tx)
        .await?;
        let changes = json!({ "changes": { "cycle": { "from": cycle_id, "to": next_id } } });
        for task_id in &moved {
            sqlx::query(
                "INSERT INTO task_cycle_carryovers (cycle_id, task_id, carried_at) VALUES ($1, $2, $3) \
                 ON CONFLICT (cycle_id, task_id) DO NOTHING",
            )
            .bind(cycle_id.to_string())
            .bind(task_id.as_str())
            .bind(now.as_millis())
            .execute(&mut *tx)
            .await?;
            self.record_write(
                &mut tx,
                workspace_id,
                actor_id,
                ("task.updated", "task"),
                parse_id(task_id.clone())?,
                request_id,
                changes.clone(),
                now,
            )
            .await?;
        }
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "cycle.unfinished_moved",
            "cycle",
            cycle_id,
            request_id,
            now,
        )
        .await?;
        let to_cycle = cycle_in_tx(&mut tx, workspace_id, project_id, next_id).await?;
        tx.commit().await?;
        Ok(CycleMoveOutcome {
            moved: moved.len() as i64,
            to_cycle,
        })
    }
}

fn validate_dates(starts_at: TimestampMillis, ends_at: TimestampMillis) -> Result<(), TaskError> {
    if ends_at.as_millis() > starts_at.as_millis() {
        Ok(())
    } else {
        Err(TaskError::Invalid { field: "ends_at" })
    }
}

/// `[starts_at, ends_at)` must not overlap another cycle of the project (touching is fine).
async fn reject_overlap(
    tx: &mut Transaction<'_, Any>,
    project_id: Id,
    except: Option<Id>,
    starts_at: TimestampMillis,
    ends_at: TimestampMillis,
) -> Result<(), TaskError> {
    let Flag(overlaps) = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM task_cycles WHERE project_id = $1 \
         AND id IS DISTINCT FROM $2 AND starts_at < $3 AND ends_at > $4)",
    )
    .bind(project_id.to_string())
    .bind(except.map(|id| id.to_string()))
    .bind(ends_at.as_millis())
    .bind(starts_at.as_millis())
    .fetch_one(&mut **tx)
    .await?;
    if overlaps {
        Err(TaskError::CycleOverlap)
    } else {
        Ok(())
    }
}

async fn cycle_in_tx(
    tx: &mut Transaction<'_, Any>,
    workspace_id: Id,
    project_id: Id,
    cycle_id: Id,
) -> Result<CycleRecord, TaskError> {
    let row = sqlx::query(sqlx::AssertSqlSafe(select_sql("AND task_cycles.id = $3")))
        .bind(workspace_id.to_string())
        .bind(project_id.to_string())
        .bind(cycle_id.to_string())
        .fetch_optional(&mut **tx)
        .await?
        .ok_or(TaskError::NotFound)?;
    cycle_from_row(row)
}

fn cycle_from_row(row: sqlx::any::AnyRow) -> Result<CycleRecord, TaskError> {
    Ok(CycleRecord {
        id: parse_id(row.get("id"))?,
        workspace_id: parse_id(row.get("workspace_id"))?,
        project_id: parse_id(row.get("project_id"))?,
        number: row.get("number"),
        name: row.get("name"),
        starts_at: TimestampMillis::from_millis(row.get("starts_at")),
        ends_at: TimestampMillis::from_millis(row.get("ends_at")),
        version: parse_version(row.get("version"))?,
        created_at: TimestampMillis::from_millis(row.get("created_at")),
        updated_at: TimestampMillis::from_millis(row.get("updated_at")),
        task_count: row.get("task_count"),
        completed_count: row.get("completed_count"),
        estimate_total: row.get("estimate_total"),
        estimate_completed: row.get("estimate_completed"),
        carried_over_count: row.get("carried_over_count"),
    })
}
