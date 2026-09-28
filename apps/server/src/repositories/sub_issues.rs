//! Sub-issues (spec `2026-09-27-sub-issues-design.md`): parent validation and tree reads. Every
//! `_in_tx` function runs inside the caller's write transaction.

use orbit_platform::Id;
use sqlx::{Row, Sqlite, SqlitePool, Transaction};

use super::tasks::{TaskError, TaskRef, parse_id, parse_task_ref};

/// Upper bound for walks over `parent_task_id`. The repository never lets a cycle in; the bound
/// only keeps a corrupt database from looping.
const MAX_DEPTH: i64 = 1_000;

/// Correlated subquery rendering the visible parent `parent_id_sql` as a `TaskRef` JSON object,
/// or NULL when there is none or it (or its project) is in the trash.
pub(super) fn parent_subquery(parent_id_sql: &str) -> String {
    format!(
        "(SELECT json_object('id', parent.id, 'project_id', parent.project_id, \
         'project_key', parent_project.project_key, 'title', parent.title) \
         FROM tasks AS parent JOIN projects AS parent_project ON parent_project.id = parent.project_id \
         WHERE parent.id = {parent_id_sql} AND parent.deleted_at IS NULL \
         AND parent_project.deleted_at IS NULL)"
    )
}

pub(super) async fn parent_ref_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    parent_id: Option<Id>,
) -> Result<Option<TaskRef>, TaskError> {
    let Some(parent_id) = parent_id else {
        return Ok(None);
    };
    let sql = format!("SELECT {}", parent_subquery("?"));
    let value: Option<String> = sqlx::query_scalar(&sql)
        .bind(parent_id.to_string())
        .fetch_one(&mut **tx)
        .await?;
    parse_task_ref(value)
}

/// `task_id` is `None` for a task being created (it cannot be anyone's ancestor yet).
/// Self-parent and any parent that has `task_id` among its ancestors are `ParentCycle`; a parent
/// that is missing, in another workspace, in the trash or in a trashed project is `ParentInvalid`.
pub(super) async fn validate_parent_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    task_id: Option<Id>,
    parent_id: Id,
) -> Result<(), TaskError> {
    if task_id == Some(parent_id) {
        return Err(TaskError::ParentCycle);
    }
    let live: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM tasks JOIN projects ON projects.id = tasks.project_id \
         WHERE tasks.id = ? AND tasks.workspace_id = ? AND tasks.deleted_at IS NULL \
         AND projects.deleted_at IS NULL)",
    )
    .bind(parent_id.to_string())
    .bind(workspace_id.to_string())
    .fetch_one(&mut **tx)
    .await?;
    if !live {
        return Err(TaskError::ParentInvalid);
    }
    if let Some(task_id) = task_id {
        // Walk up from the new parent; UNION (not UNION ALL) stops on repeated ids.
        let cycle: bool = sqlx::query_scalar(
            "WITH RECURSIVE chain(id) AS ( \
                 SELECT ? \
                 UNION \
                 SELECT tasks.parent_task_id FROM tasks JOIN chain ON tasks.id = chain.id \
                 WHERE tasks.parent_task_id IS NOT NULL \
             ) \
             SELECT EXISTS (SELECT 1 FROM chain WHERE id = ?)",
        )
        .bind(parent_id.to_string())
        .bind(task_id.to_string())
        .fetch_one(&mut **tx)
        .await?;
        if cycle {
            return Err(TaskError::ParentCycle);
        }
    }
    Ok(())
}

/// Visible ancestors of `task_id`, root first (`GET /tasks/{id}`).
pub(super) async fn ancestors(pool: &SqlitePool, task_id: Id) -> Result<Vec<TaskRef>, TaskError> {
    let rows = sqlx::query(
        "WITH RECURSIVE chain(id, depth) AS ( \
             SELECT parent_task_id, 1 FROM tasks WHERE id = ? AND parent_task_id IS NOT NULL \
             UNION ALL \
             SELECT tasks.parent_task_id, chain.depth + 1 FROM tasks JOIN chain ON tasks.id = chain.id \
             WHERE tasks.parent_task_id IS NOT NULL AND chain.depth < ? \
         ) \
         SELECT tasks.id, tasks.project_id, projects.project_key, tasks.title FROM chain \
         JOIN tasks ON tasks.id = chain.id JOIN projects ON projects.id = tasks.project_id \
         WHERE tasks.deleted_at IS NULL AND projects.deleted_at IS NULL \
         ORDER BY chain.depth DESC",
    )
    .bind(task_id.to_string())
    .bind(MAX_DEPTH)
    .fetch_all(pool)
    .await?;
    rows.into_iter()
        .map(|row| {
            Ok(TaskRef {
                id: parse_id(row.get("id"))?,
                project_id: parse_id(row.get("project_id"))?,
                project_key: row.get("project_key"),
                title: row.get("title"),
            })
        })
        .collect()
}
