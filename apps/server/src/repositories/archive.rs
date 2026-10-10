//! Task archive: closed work leaves the default lists without being deleted.
//!
//! The unit is a tree: a top-level task with all its live sub-issues. A tree is archived only
//! when each task of it is closed (completed, cancelled or duplicate), so a closed sub-issue
//! never goes away from below an open parent. Each task of an archived tree has the same
//! `archived_root_id`. A move to an open status restores the tree (trigger
//! `tasks_unarchive_on_reopen`).

use orbit_platform::{Database, Id, TimestampMillis};
use serde_json::json;
use sqlx::{Row, Sqlite, Transaction};

use super::tasks::{TaskError, TaskRepository, parse_id, record_mutation, require_access_tx};
use crate::audit::{self, AuditOutcome};

/// The period of `projects.auto_archive_months` counts a month as 30 days, as `show_completed`.
const MONTH_MS: i64 = 30 * 24 * 60 * 60 * 1000;
/// Sub-issue trees are shallow; the walk to the root stops here whatever the data says.
const MAX_DEPTH: usize = 64;

/// `tree(id)`: the task `?1` and all its live descendants.
const TREE: &str = "WITH RECURSIVE tree(id) AS ( \
     SELECT id FROM tasks WHERE id = ?1 \
     UNION ALL \
     SELECT tasks.id FROM tasks JOIN tree ON tasks.parent_task_id = tree.id \
     WHERE tasks.deleted_at IS NULL)";

impl TaskRepository {
    /// Archives the trees of `task_ids`. Each tree must be closed (`ArchiveOpenTree` if not);
    /// nothing is archived when one of them is open. Returns the number of tasks archived.
    pub async fn archive_tasks(
        &self,
        workspace_id: Id,
        actor_id: Id,
        task_ids: &[Id],
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<u64, TaskError> {
        let mut tx = self.database().immediate_transaction().await?;
        require_access_tx(&mut tx, workspace_id, actor_id).await?;
        let mut roots = Vec::new();
        for task_id in task_ids {
            let root = root_in_tx(&mut tx, workspace_id, *task_id).await?;
            if !roots.contains(&root) {
                roots.push(root);
            }
        }
        let mut archived = 0;
        for root in roots {
            let open: i64 = sqlx::query_scalar(&format!(
                "{TREE} SELECT COUNT(*) FROM tree JOIN tasks ON tasks.id = tree.id \
                 JOIN task_statuses ON task_statuses.id = tasks.status_id \
                 WHERE task_statuses.category NOT IN ('completed', 'cancelled', 'duplicate')"
            ))
            .bind(root.to_string())
            .fetch_one(&mut *tx)
            .await?;
            if open > 0 {
                return Err(TaskError::ArchiveOpenTree);
            }
            let count = archive_tree_in_tx(&mut tx, root, now).await?;
            if count > 0 {
                archived += count;
                record_mutation(
                    &mut tx,
                    workspace_id,
                    actor_id,
                    "task.archived",
                    "task",
                    root,
                    request_id,
                    now,
                )
                .await?;
            }
        }
        tx.commit().await?;
        Ok(archived)
    }

    /// Restores the archived trees of `task_ids`; a task that is not archived is left as it is.
    /// Returns the number of tasks restored.
    pub async fn unarchive_tasks(
        &self,
        workspace_id: Id,
        actor_id: Id,
        task_ids: &[Id],
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<u64, TaskError> {
        let mut tx = self.database().immediate_transaction().await?;
        require_access_tx(&mut tx, workspace_id, actor_id).await?;
        let mut restored = 0;
        for task_id in task_ids {
            let row = sqlx::query(
                "SELECT tasks.archived_root_id FROM tasks \
                 JOIN projects ON projects.id = tasks.project_id \
                 WHERE tasks.id = ? AND tasks.workspace_id = ? \
                 AND tasks.deleted_at IS NULL AND projects.deleted_at IS NULL",
            )
            .bind(task_id.to_string())
            .bind(workspace_id.to_string())
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(TaskError::NotFound)?;
            let Some(root) = row.get::<Option<String>, _>("archived_root_id") else {
                continue;
            };
            let count = sqlx::query(
                "UPDATE tasks SET archived_at = NULL, archived_root_id = NULL, version = version + 1 \
                 WHERE archived_root_id = ?",
            )
            .bind(&root)
            .execute(&mut *tx)
            .await?
            .rows_affected();
            restored += count;
            record_mutation(
                &mut tx,
                workspace_id,
                actor_id,
                "task.unarchived",
                "task",
                parse_id(root)?,
                request_id,
                now,
            )
            .await?;
        }
        tx.commit().await?;
        Ok(restored)
    }
}

/// The daily job: archives each closed tree whose most recent close time is older than the
/// period of the project of its top-level task. Returns the number of tasks archived.
pub async fn run_auto_archive(
    database: &Database,
    now: TimestampMillis,
) -> Result<u64, sqlx::Error> {
    let mut tx = database.immediate_transaction().await?;
    // A tree starts at a task with no live parent. `updated_at` stands in for a close time that
    // the data does not have.
    let due = sqlx::query(
        "WITH RECURSIVE tree(root_id, id) AS ( \
         SELECT tasks.id, tasks.id FROM tasks \
         JOIN projects ON projects.id = tasks.project_id \
         WHERE tasks.deleted_at IS NULL AND tasks.archived_at IS NULL \
         AND projects.deleted_at IS NULL AND projects.auto_archive_months IS NOT NULL \
         AND (tasks.parent_task_id IS NULL OR NOT EXISTS ( \
              SELECT 1 FROM tasks AS parent \
              WHERE parent.id = tasks.parent_task_id AND parent.deleted_at IS NULL)) \
         UNION ALL \
         SELECT tree.root_id, child.id FROM tasks AS child \
         JOIN tree ON child.parent_task_id = tree.id WHERE child.deleted_at IS NULL) \
         SELECT tree.root_id AS root_id, root.workspace_id AS workspace_id FROM tree \
         JOIN tasks ON tasks.id = tree.id \
         JOIN task_statuses ON task_statuses.id = tasks.status_id \
         JOIN tasks AS root ON root.id = tree.root_id \
         JOIN projects ON projects.id = root.project_id \
         GROUP BY tree.root_id \
         HAVING MIN(task_statuses.category IN ('completed', 'cancelled', 'duplicate')) = 1 \
         AND MAX(COALESCE(tasks.completed_at, tasks.updated_at)) <= ?1 - projects.auto_archive_months * ?2",
    )
    .bind(now.as_millis())
    .bind(MONTH_MS)
    .fetch_all(&mut *tx)
    .await?;
    let mut archived = 0;
    let mut by_workspace: Vec<(String, u64)> = Vec::new();
    for row in due {
        let root: String = row.get("root_id");
        let workspace: String = row.get("workspace_id");
        let count = sqlx::query(&format!(
            "{TREE} UPDATE tasks SET archived_at = ?2, archived_root_id = ?1, version = version + 1 \
             WHERE id IN (SELECT id FROM tree) AND archived_at IS NULL"
        ))
        .bind(&root)
        .bind(now.as_millis())
        .execute(&mut *tx)
        .await?
        .rows_affected();
        archived += count;
        match by_workspace.iter_mut().find(|(id, _)| *id == workspace) {
            Some((_, total)) => *total += count,
            None => by_workspace.push((workspace, count)),
        }
    }
    // One event for each workspace, so that open clients refresh their lists.
    for (workspace, count) in by_workspace {
        let Ok(workspace_id) = workspace.parse::<Id>() else {
            continue;
        };
        audit::record(
            &mut tx,
            workspace_id,
            None,
            "task.auto_archived",
            AuditOutcome::Success,
            "task",
            None,
            "auto-archive",
            json!({ "count": count }),
            now,
        )
        .await?;
    }
    tx.commit().await?;
    Ok(archived)
}

/// The top-level task of the tree of `task_id`: the last task on the walk through live parents.
/// A task that is not a live task of the workspace is `NotFound`.
async fn root_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    task_id: Id,
) -> Result<Id, TaskError> {
    let exists: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tasks JOIN projects ON projects.id = tasks.project_id \
         WHERE tasks.id = ? AND tasks.workspace_id = ? \
         AND tasks.deleted_at IS NULL AND projects.deleted_at IS NULL",
    )
    .bind(task_id.to_string())
    .bind(workspace_id.to_string())
    .fetch_one(&mut **tx)
    .await?;
    if exists != 1 {
        return Err(TaskError::NotFound);
    }
    let mut root = task_id;
    for _ in 0..MAX_DEPTH {
        let parent: Option<String> = sqlx::query_scalar(
            "SELECT parent.id FROM tasks AS child \
             JOIN tasks AS parent ON parent.id = child.parent_task_id \
             WHERE child.id = ? AND parent.deleted_at IS NULL",
        )
        .bind(root.to_string())
        .fetch_optional(&mut **tx)
        .await?;
        match parent {
            Some(parent) => root = parse_id(parent)?,
            None => break,
        }
    }
    Ok(root)
}

async fn archive_tree_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    root: Id,
    now: TimestampMillis,
) -> Result<u64, TaskError> {
    Ok(sqlx::query(&format!(
        "{TREE} UPDATE tasks SET archived_at = ?2, archived_root_id = ?1, version = version + 1 \
         WHERE id IN (SELECT id FROM tree) AND archived_at IS NULL"
    ))
    .bind(root.to_string())
    .bind(now.as_millis())
    .execute(&mut **tx)
    .await?
    .rows_affected())
}
