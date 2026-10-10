//! Label groups: a label can belong to one group, and a task has not more than one label of a
//! group (the `task_labels_one_per_group` trigger replaces the other label on each write).

use orbit_platform::{Id, TimestampMillis};
use serde::Serialize;
use sqlx::{Row, Sqlite, Transaction};
use utoipa::ToSchema;

use super::tasks::{
    TaskError, TaskRepository, check_version, is_unique_violation, parse_id, parse_version,
    record_mutation, require_access, require_access_tx,
};

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct LabelGroupRecord {
    #[schema(value_type = String)]
    pub id: Id,
    #[schema(value_type = String)]
    pub workspace_id: Id,
    pub name: String,
    pub color: String,
    pub position: i64,
    pub version: u64,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct LabelGroupList {
    pub items: Vec<LabelGroupRecord>,
}

const COLUMNS: &str = "id, workspace_id, name, color, position, version";

impl TaskRepository {
    pub async fn label_groups(
        &self,
        workspace_id: Id,
        actor_id: Id,
    ) -> Result<LabelGroupList, TaskError> {
        require_access(self.database().pool(), workspace_id, actor_id).await?;
        let items = sqlx::query(&format!(
            "SELECT {COLUMNS} FROM label_groups WHERE workspace_id = ? ORDER BY position, name, id"
        ))
        .bind(workspace_id.to_string())
        .fetch_all(self.database().pool())
        .await?
        .into_iter()
        .map(group_from_row)
        .collect::<Result<_, _>>()?;
        Ok(LabelGroupList { items })
    }

    /// A new group goes last. A name that the workspace already uses is a `Conflict`.
    pub async fn create_label_group(
        &self,
        workspace_id: Id,
        actor_id: Id,
        name: String,
        color: String,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<LabelGroupRecord, TaskError> {
        let id = Id::new_v7();
        let mut tx = self.database().immediate_transaction().await?;
        require_access_tx(&mut tx, workspace_id, actor_id).await?;
        let position: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(position) + 1, 0) FROM label_groups WHERE workspace_id = ?",
        )
        .bind(workspace_id.to_string())
        .fetch_one(&mut *tx)
        .await?;
        let inserted = sqlx::query(
            "INSERT INTO label_groups (id, workspace_id, name, color, position, version, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, 0, ?, ?)",
        )
        .bind(id.to_string())
        .bind(workspace_id.to_string())
        .bind(&name)
        .bind(&color)
        .bind(position)
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
            "label_group.created",
            "label_group",
            id,
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(LabelGroupRecord {
            id,
            workspace_id,
            name,
            color,
            position,
            version: 0,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update_label_group(
        &self,
        workspace_id: Id,
        group_id: Id,
        actor_id: Id,
        name: String,
        color: String,
        expected_version: u64,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<LabelGroupRecord, TaskError> {
        let mut tx = self.database().immediate_transaction().await?;
        require_access_tx(&mut tx, workspace_id, actor_id).await?;
        let current = label_group_in_tx(&mut tx, workspace_id, group_id).await?;
        check_version(expected_version, current.version, &current)?;
        let updated = sqlx::query(
            "UPDATE label_groups SET name = ?, color = ?, version = version + 1, updated_at = ? \
             WHERE id = ? AND workspace_id = ? AND version = ?",
        )
        .bind(&name)
        .bind(&color)
        .bind(now.as_millis())
        .bind(group_id.to_string())
        .bind(workspace_id.to_string())
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
            "label_group.updated",
            "label_group",
            group_id,
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(LabelGroupRecord {
            name,
            color,
            version: current.version + 1,
            ..current
        })
    }

    /// Deletes the group; its labels stay, with no group (`ON DELETE SET NULL`).
    pub async fn delete_label_group(
        &self,
        workspace_id: Id,
        group_id: Id,
        actor_id: Id,
        expected_version: u64,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), TaskError> {
        let mut tx = self.database().immediate_transaction().await?;
        require_access_tx(&mut tx, workspace_id, actor_id).await?;
        let current = label_group_in_tx(&mut tx, workspace_id, group_id).await?;
        check_version(expected_version, current.version, &current)?;
        // Each label of the group changes, so clients with a stale label version must refresh.
        sqlx::query("UPDATE labels SET version = version + 1, updated_at = ? WHERE group_id = ?")
            .bind(now.as_millis())
            .bind(group_id.to_string())
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM label_groups WHERE id = ? AND workspace_id = ?")
            .bind(group_id.to_string())
            .bind(workspace_id.to_string())
            .execute(&mut *tx)
            .await?;
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "label_group.deleted",
            "label_group",
            group_id,
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }
}

pub(super) async fn label_group_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    group_id: Id,
) -> Result<LabelGroupRecord, TaskError> {
    let row = sqlx::query(&format!(
        "SELECT {COLUMNS} FROM label_groups WHERE id = ? AND workspace_id = ?"
    ))
    .bind(group_id.to_string())
    .bind(workspace_id.to_string())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(TaskError::NotFound)?;
    group_from_row(row)
}

/// The number of tasks that have `label_id` and also a different label of `group_id`: the tasks
/// that would break the rule if the label joined the group. Tasks in the trash count too, since
/// a restore brings them back.
pub(super) async fn join_conflicts_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    label_id: Id,
    group_id: Id,
) -> Result<i64, TaskError> {
    Ok(sqlx::query_scalar(
        "SELECT COUNT(DISTINCT mine.task_id) FROM task_labels AS mine \
         JOIN task_labels AS other ON other.task_id = mine.task_id AND other.label_id <> mine.label_id \
         JOIN labels ON labels.id = other.label_id \
         WHERE mine.label_id = ? AND labels.group_id = ?",
    )
    .bind(label_id.to_string())
    .bind(group_id.to_string())
    .fetch_one(&mut **tx)
    .await?)
}

fn group_from_row(row: sqlx::sqlite::SqliteRow) -> Result<LabelGroupRecord, TaskError> {
    Ok(LabelGroupRecord {
        id: parse_id(row.get("id"))?,
        workspace_id: parse_id(row.get("workspace_id"))?,
        name: row.get("name"),
        color: row.get("color"),
        position: row.get("position"),
        version: parse_version(row.get("version"))?,
    })
}
