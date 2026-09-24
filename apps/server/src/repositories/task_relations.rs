//! Task relations. A duplicate relation is kept in sync with the project's system Duplicate
//! status: a task is in that status exactly while it has an outgoing duplicate relation.

use orbit_platform::{Id, TimestampMillis};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{Row, Sqlite, Transaction};
use utoipa::ToSchema;

use super::tasks::{
    TaskChanges, TaskError, TaskRepository, TaskUpdate, parse_id, record_mutation,
    require_access_tx, task_in_tx, update_task_in_tx,
};
use crate::audit::{self, AuditOutcome};

pub(super) const DUPLICATE: &str = "duplicate";

/// Audit metadata keeps titles short so events stay under the 2 KB cap in `audit.rs`.
const AUDIT_TITLE_CHARS: usize = 200;

/// Who changed a relation: a member, or GitHub acting through its service account.
#[derive(Clone, Copy)]
pub(super) struct RelationActor<'a> {
    pub(super) user_id: Id,
    pub(super) service_account: Option<(Id, &'a str)>,
}

pub(super) async fn status_category_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    status_id: Id,
) -> Result<String, TaskError> {
    sqlx::query_scalar("SELECT category FROM task_statuses WHERE id = ? AND workspace_id = ?")
        .bind(status_id.to_string())
        .bind(workspace_id.to_string())
        .fetch_optional(&mut **tx)
        .await?
        .ok_or(TaskError::NotFound)
}

pub(super) async fn duplicate_status_id_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    project_id: Id,
) -> Result<Id, TaskError> {
    let id: String = sqlx::query_scalar(
        "SELECT id FROM task_statuses WHERE workspace_id = ? AND project_id = ? AND category = 'duplicate'",
    )
    .bind(workspace_id.to_string())
    .bind(project_id.to_string())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(TaskError::NotFound)?;
    parse_id(id)
}

/// Where an unmarked duplicate goes: the status it had before it was marked, while that still
/// exists in `project_id`, otherwise the project's first unstarted (then first) status.
pub(super) async fn restore_status_id_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    task_id: Id,
    project_id: Id,
) -> Result<Id, TaskError> {
    let previous: Option<String> = sqlx::query_scalar(
        "SELECT task_statuses.id FROM task_relations \
         JOIN task_statuses ON task_statuses.id = task_relations.previous_status_id \
         WHERE task_relations.task_id = ? AND task_relations.type = 'duplicate' \
         AND task_statuses.workspace_id = ? AND task_statuses.project_id = ? \
         AND task_statuses.category <> 'duplicate'",
    )
    .bind(task_id.to_string())
    .bind(workspace_id.to_string())
    .bind(project_id.to_string())
    .fetch_optional(&mut **tx)
    .await?;
    let id = match previous {
        Some(id) => id,
        None => sqlx::query_scalar(
            "SELECT id FROM task_statuses WHERE workspace_id = ? AND project_id = ? AND category <> 'duplicate' \
             ORDER BY CASE category WHEN 'unstarted' THEN 0 ELSE 1 END, position, id LIMIT 1",
        )
        .bind(workspace_id.to_string())
        .bind(project_id.to_string())
        .fetch_optional(&mut **tx)
        .await?
        .ok_or(TaskError::Invalid {
            field: "duplicate_of_id",
        })?,
    };
    parse_id(id)
}

/// The title of a live task (neither it nor its project is in the trash), if it is one.
pub(super) async fn visible_task_title_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    task_id: Id,
) -> Result<Option<String>, TaskError> {
    Ok(sqlx::query_scalar(
        "SELECT tasks.title FROM tasks JOIN projects ON projects.id = tasks.project_id \
         WHERE tasks.id = ? AND tasks.workspace_id = ? AND tasks.deleted_at IS NULL AND projects.deleted_at IS NULL",
    )
    .bind(task_id.to_string())
    .bind(workspace_id.to_string())
    .fetch_optional(&mut **tx)
    .await?)
}

/// Records `action` on both tasks of a relation; each event describes the other task.
/// `task_id` is the relation's source (the duplicate, or the blocker). `direction` is relative
/// to the task the event is stored on: the source's event is `outgoing`, and the target's event
/// (the canonical, or the blocked task) is `incoming`.
#[allow(clippy::too_many_arguments)]
pub(super) async fn record_relation_audit(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    actor: RelationActor<'_>,
    action: &str,
    relation_type: &str,
    task_id: Id,
    related_task_id: Id,
    request_id: &str,
    now: TimestampMillis,
) -> Result<(), TaskError> {
    for (resource_id, other_id, direction) in [
        (task_id, related_task_id, "outgoing"),
        (related_task_id, task_id, "incoming"),
    ] {
        let (title, project_id): (String, String) =
            sqlx::query_as("SELECT title, project_id FROM tasks WHERE id = ?")
                .bind(other_id.to_string())
                .fetch_one(&mut **tx)
                .await?;
        let mut metadata = json!({
            "type": relation_type,
            "direction": direction,
            "related_task_id": other_id,
            "related_task_project_id": project_id,
            "related_task_title": title.chars().take(AUDIT_TITLE_CHARS).collect::<String>(),
        });
        let actor_id = match actor.service_account {
            Some((id, name)) => {
                metadata["actor_service_account_id"] = json!(id);
                metadata["actor_service_account_name"] = json!(name);
                None
            }
            None => Some(actor.user_id),
        };
        audit::record(
            tx,
            workspace_id,
            actor_id,
            action,
            AuditOutcome::Success,
            "task",
            Some(resource_id),
            request_id,
            metadata,
            now,
        )
        .await?;
    }
    Ok(())
}

/// Deletes `task_id`'s duplicate relation, if any, and audits it. The caller owns the task's
/// status change. Returns whether a relation existed.
pub(super) async fn delete_duplicate_relation_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    actor: RelationActor<'_>,
    task_id: Id,
    request_id: &str,
    now: TimestampMillis,
) -> Result<bool, TaskError> {
    let Some(row) = sqlx::query(
        "SELECT id, related_task_id FROM task_relations WHERE task_id = ? AND type = 'duplicate'",
    )
    .bind(task_id.to_string())
    .fetch_optional(&mut **tx)
    .await?
    else {
        return Ok(false);
    };
    let related_task_id = parse_id(row.get("related_task_id"))?;
    sqlx::query("DELETE FROM task_relations WHERE id = ?")
        .bind(row.get::<String, _>("id"))
        .execute(&mut **tx)
        .await?;
    record_relation_audit(
        tx,
        workspace_id,
        actor,
        "task.unmarked_duplicate",
        DUPLICATE,
        task_id,
        related_task_id,
        request_id,
        now,
    )
    .await?;
    Ok(true)
}

/// Removes a blocks/related relation between two tasks, in either direction.
async fn delete_plain_pair_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    first: Id,
    second: Id,
) -> Result<(), TaskError> {
    sqlx::query(
        "DELETE FROM task_relations WHERE type <> 'duplicate' \
         AND ((task_id = ? AND related_task_id = ?) OR (task_id = ? AND related_task_id = ?))",
    )
    .bind(first.to_string())
    .bind(second.to_string())
    .bind(second.to_string())
    .bind(first.to_string())
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Makes `task_id` a duplicate of `target_id`. The caller has already moved the task into its
/// project's Duplicate status; `previous_status_id` is the status it had before. Duplicates of
/// `task_id` are re-pointed to `target_id` (without a version bump) so chains never form.
#[allow(clippy::too_many_arguments)]
pub(super) async fn mark_duplicate_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    actor: RelationActor<'_>,
    task_id: Id,
    previous_status_id: Id,
    target_id: Id,
    request_id: &str,
    now: TimestampMillis,
) -> Result<(), TaskError> {
    if target_id == task_id
        || visible_task_title_in_tx(tx, workspace_id, target_id)
            .await?
            .is_none()
    {
        return Err(TaskError::Invalid {
            field: "duplicate_of_id",
        });
    }
    let target_is_duplicate: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM task_relations WHERE task_id = ? AND type = 'duplicate')",
    )
    .bind(target_id.to_string())
    .fetch_one(&mut **tx)
    .await?;
    if target_is_duplicate {
        return Err(TaskError::Invalid {
            field: "duplicate_of_id",
        });
    }
    let existing = sqlx::query(
        "SELECT id, related_task_id FROM task_relations WHERE task_id = ? AND type = 'duplicate'",
    )
    .bind(task_id.to_string())
    .fetch_optional(&mut **tx)
    .await?;
    let existing_target = existing
        .as_ref()
        .map(|row| parse_id(row.get("related_task_id")))
        .transpose()?;
    if existing_target == Some(target_id) {
        return Ok(());
    }
    delete_plain_pair_in_tx(tx, task_id, target_id).await?;

    // Re-point this task's own duplicates first: the chain triggers reject a new duplicate
    // relation for a task that others still point at.
    let followers: Vec<String> = sqlx::query_scalar(
        "SELECT task_id FROM task_relations WHERE related_task_id = ? AND type = 'duplicate'",
    )
    .bind(task_id.to_string())
    .fetch_all(&mut **tx)
    .await?;
    for follower in followers {
        let follower = parse_id(follower)?;
        delete_plain_pair_in_tx(tx, follower, target_id).await?;
        sqlx::query(
            "UPDATE task_relations SET related_task_id = ? WHERE task_id = ? AND type = 'duplicate'",
        )
        .bind(target_id.to_string())
        .bind(follower.to_string())
        .execute(&mut **tx)
        .await?;
        record_relation_audit(
            tx,
            workspace_id,
            actor,
            "task.unmarked_duplicate",
            DUPLICATE,
            follower,
            task_id,
            request_id,
            now,
        )
        .await?;
        record_relation_audit(
            tx,
            workspace_id,
            actor,
            "task.marked_duplicate",
            DUPLICATE,
            follower,
            target_id,
            request_id,
            now,
        )
        .await?;
    }

    match (existing, existing_target) {
        (Some(row), Some(old_target)) => {
            // Re-point, keeping the original previous_status_id.
            sqlx::query("UPDATE task_relations SET related_task_id = ? WHERE id = ?")
                .bind(target_id.to_string())
                .bind(row.get::<String, _>("id"))
                .execute(&mut **tx)
                .await?;
            record_relation_audit(
                tx,
                workspace_id,
                actor,
                "task.unmarked_duplicate",
                DUPLICATE,
                task_id,
                old_target,
                request_id,
                now,
            )
            .await?;
        }
        _ => {
            sqlx::query(
                "INSERT INTO task_relations (id, workspace_id, task_id, related_task_id, type, previous_status_id, created_by, created_at) \
                 VALUES (?, ?, ?, ?, 'duplicate', ?, ?, ?)",
            )
            .bind(Id::new_v7().to_string())
            .bind(workspace_id.to_string())
            .bind(task_id.to_string())
            .bind(target_id.to_string())
            .bind(previous_status_id.to_string())
            .bind(actor.user_id.to_string())
            .bind(now.as_millis())
            .execute(&mut **tx)
            .await?;
        }
    }
    record_relation_audit(
        tx,
        workspace_id,
        actor,
        "task.marked_duplicate",
        DUPLICATE,
        task_id,
        target_id,
        request_id,
        now,
    )
    .await?;
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum TaskRelationType {
    Blocks,
    Related,
    Duplicate,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum TaskRelationDirection {
    Outgoing,
    Incoming,
}

/// Relation types a client can create. Duplicates go through `duplicate_of_id` on task updates.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum NewTaskRelationType {
    Blocks,
    BlockedBy,
    Related,
}

/// The other task of a relation, as seen from the task being viewed.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct RelatedTask {
    #[schema(value_type = String)]
    pub id: Id,
    #[schema(value_type = String)]
    pub project_id: Id,
    pub title: String,
    #[schema(value_type = String)]
    pub status_id: Id,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct TaskRelationRecord {
    #[schema(value_type = String)]
    pub id: Id,
    #[serde(rename = "type")]
    pub relation_type: TaskRelationType,
    /// Outgoing when the viewed task is the relation's source (the blocker or the duplicate).
    pub direction: TaskRelationDirection,
    pub task: RelatedTask,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: TimestampMillis,
}

/// Relations of a viewer task (bind order: viewer, workspace, viewer, viewer). Only relations
/// whose other task and project are live are returned.
const RELATION_SELECT: &str = "SELECT task_relations.id, task_relations.type, task_relations.task_id, \
     task_relations.created_at, other.id AS other_id, other.project_id AS other_project_id, \
     other.title AS other_title, other.status_id AS other_status_id \
     FROM task_relations \
     JOIN tasks AS other ON other.id = CASE WHEN task_relations.task_id = ? \
         THEN task_relations.related_task_id ELSE task_relations.task_id END \
     JOIN projects AS other_project ON other_project.id = other.project_id \
     WHERE task_relations.workspace_id = ? \
     AND (task_relations.task_id = ? OR task_relations.related_task_id = ?) \
     AND other.deleted_at IS NULL AND other_project.deleted_at IS NULL";

fn relation_from_row(
    row: &sqlx::sqlite::SqliteRow,
    viewer_id: Id,
) -> Result<TaskRelationRecord, TaskError> {
    let relation_type = match row.get::<String, _>("type").as_str() {
        "blocks" => TaskRelationType::Blocks,
        "related" => TaskRelationType::Related,
        "duplicate" => TaskRelationType::Duplicate,
        _ => return Err(TaskError::Conflict),
    };
    Ok(TaskRelationRecord {
        id: parse_id(row.get("id"))?,
        relation_type,
        direction: if parse_id(row.get("task_id"))? == viewer_id {
            TaskRelationDirection::Outgoing
        } else {
            TaskRelationDirection::Incoming
        },
        task: RelatedTask {
            id: parse_id(row.get("other_id"))?,
            project_id: parse_id(row.get("other_project_id"))?,
            title: row.get("other_title"),
            status_id: parse_id(row.get("other_status_id"))?,
        },
        created_at: TimestampMillis::from_millis(row.get("created_at")),
    })
}

async fn relation_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    viewer_id: Id,
    relation_id: Id,
) -> Result<TaskRelationRecord, TaskError> {
    let sql = format!("{RELATION_SELECT} AND task_relations.id = ?");
    let row = sqlx::query(&sql)
        .bind(viewer_id.to_string())
        .bind(workspace_id.to_string())
        .bind(viewer_id.to_string())
        .bind(viewer_id.to_string())
        .bind(relation_id.to_string())
        .fetch_optional(&mut **tx)
        .await?
        .ok_or(TaskError::NotFound)?;
    relation_from_row(&row, viewer_id)
}

impl TaskRepository {
    pub async fn task_relations(
        &self,
        workspace_id: Id,
        task_id: Id,
        actor_id: Id,
    ) -> Result<Vec<TaskRelationRecord>, TaskError> {
        self.get_task(workspace_id, task_id, actor_id).await?;
        let sql =
            format!("{RELATION_SELECT} ORDER BY task_relations.created_at, task_relations.id");
        sqlx::query(&sql)
            .bind(task_id.to_string())
            .bind(workspace_id.to_string())
            .bind(task_id.to_string())
            .bind(task_id.to_string())
            .fetch_all(self.database().pool())
            .await?
            .iter()
            .map(|row| relation_from_row(row, task_id))
            .collect()
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn add_task_relation(
        &self,
        workspace_id: Id,
        task_id: Id,
        actor_id: Id,
        kind: NewTaskRelationType,
        other_id: Id,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<TaskRelationRecord, TaskError> {
        let mut tx = self.database().immediate_transaction().await?;
        require_access_tx(&mut tx, workspace_id, actor_id).await?;
        if visible_task_title_in_tx(&mut tx, workspace_id, task_id)
            .await?
            .is_none()
        {
            return Err(TaskError::NotFound);
        }
        if other_id == task_id
            || visible_task_title_in_tx(&mut tx, workspace_id, other_id)
                .await?
                .is_none()
        {
            return Err(TaskError::Invalid { field: "task_id" });
        }
        let (source, target, relation_type) = match kind {
            NewTaskRelationType::Blocks => (task_id, other_id, "blocks"),
            NewTaskRelationType::BlockedBy => (other_id, task_id, "blocks"),
            NewTaskRelationType::Related if task_id.to_string() < other_id.to_string() => {
                (task_id, other_id, "related")
            }
            NewTaskRelationType::Related => (other_id, task_id, "related"),
        };
        let actor = RelationActor {
            user_id: actor_id,
            service_account: None,
        };
        let existing = sqlx::query(
            "SELECT id, task_id, related_task_id, type FROM task_relations \
             WHERE (task_id = ? AND related_task_id = ?) OR (task_id = ? AND related_task_id = ?)",
        )
        .bind(source.to_string())
        .bind(target.to_string())
        .bind(target.to_string())
        .bind(source.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(row) = existing {
            let existing_id = parse_id(row.get("id"))?;
            let existing_type: String = row.get("type");
            let existing_source = parse_id(row.get("task_id"))?;
            let existing_target = parse_id(row.get("related_task_id"))?;
            let reversed_blocks =
                existing_type == "blocks" && relation_type == "blocks" && existing_source != source;
            if existing_type == DUPLICATE || reversed_blocks {
                return Err(TaskError::Conflict);
            }
            if existing_type == relation_type && existing_source == source {
                let record = relation_in_tx(&mut tx, workspace_id, task_id, existing_id).await?;
                tx.commit().await?;
                return Ok(record);
            }
            sqlx::query("DELETE FROM task_relations WHERE id = ?")
                .bind(existing_id.to_string())
                .execute(&mut *tx)
                .await?;
            record_relation_audit(
                &mut tx,
                workspace_id,
                actor,
                "task.relation_removed",
                &existing_type,
                existing_source,
                existing_target,
                request_id,
                now,
            )
            .await?;
        }
        let id = Id::new_v7();
        sqlx::query(
            "INSERT INTO task_relations (id, workspace_id, task_id, related_task_id, type, previous_status_id, created_by, created_at) \
             VALUES (?, ?, ?, ?, ?, NULL, ?, ?)",
        )
        .bind(id.to_string())
        .bind(workspace_id.to_string())
        .bind(source.to_string())
        .bind(target.to_string())
        .bind(relation_type)
        .bind(actor_id.to_string())
        .bind(now.as_millis())
        .execute(&mut *tx)
        .await?;
        record_relation_audit(
            &mut tx,
            workspace_id,
            actor,
            "task.relation_added",
            relation_type,
            source,
            target,
            request_id,
            now,
        )
        .await?;
        let record = relation_in_tx(&mut tx, workspace_id, task_id, id).await?;
        tx.commit().await?;
        Ok(record)
    }

    /// Deletes a relation seen from `task_id`. Deleting a duplicate relation runs the unmark
    /// path, so the duplicate also leaves the Duplicate status.
    pub async fn remove_task_relation(
        &self,
        workspace_id: Id,
        task_id: Id,
        relation_id: Id,
        actor_id: Id,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), TaskError> {
        let mut tx = self.database().immediate_transaction().await?;
        require_access_tx(&mut tx, workspace_id, actor_id).await?;
        if visible_task_title_in_tx(&mut tx, workspace_id, task_id)
            .await?
            .is_none()
        {
            return Err(TaskError::NotFound);
        }
        let relation = relation_in_tx(&mut tx, workspace_id, task_id, relation_id).await?;
        let (source, target) = match relation.direction {
            TaskRelationDirection::Outgoing => (task_id, relation.task.id),
            TaskRelationDirection::Incoming => (relation.task.id, task_id),
        };
        if relation.relation_type == TaskRelationType::Duplicate {
            let duplicate = task_in_tx(&mut tx, workspace_id, source, false).await?;
            let update = TaskUpdate {
                id: source,
                expected_version: duplicate.version,
                changes: TaskChanges {
                    duplicate_of_id: Some(None),
                    ..TaskChanges::default()
                },
            };
            update_task_in_tx(&mut tx, workspace_id, actor_id, &update, request_id, now).await?;
            record_mutation(
                &mut tx,
                workspace_id,
                actor_id,
                "task.updated",
                "task",
                source,
                request_id,
                now,
            )
            .await?;
        } else {
            sqlx::query("DELETE FROM task_relations WHERE id = ?")
                .bind(relation_id.to_string())
                .execute(&mut *tx)
                .await?;
            let relation_type = match relation.relation_type {
                TaskRelationType::Blocks => "blocks",
                _ => "related",
            };
            record_relation_audit(
                &mut tx,
                workspace_id,
                RelationActor {
                    user_id: actor_id,
                    service_account: None,
                },
                "task.relation_removed",
                relation_type,
                source,
                target,
                request_id,
                now,
            )
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }
}
