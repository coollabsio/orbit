//! Sub-issues (spec `2026-09-27-sub-issues-design.md`): parent validation and tree reads. Every
//! `_in_tx` function runs inside the caller's write transaction.

use std::collections::BTreeMap;

use orbit_platform::{Id, TimestampMillis};
use serde::Serialize;
use serde_json::json;
use sqlx::{Row, Sqlite, SqlitePool, Transaction};
use utoipa::ToSchema;

use super::tasks::{TaskError, TaskRef, parse_id};
use crate::audit::{self, AuditOutcome};

/// Upper bound for walks over `parent_task_id`. The repository never lets a cycle in; the bound
/// only keeps a corrupt database from looping.
const MAX_DEPTH: i64 = 1_000;

/// A task's parent is visible when it is live and its project is too. The rule behind the
/// `parent` field, the `ancestors` walk and the `hidden` sub-issue display; `parent` and
/// `parent_project` are the aliases it expects.
const VISIBLE_PARENT_FROM: &str =
    "tasks AS parent JOIN projects AS parent_project ON parent_project.id = parent.project_id";
const VISIBLE_PARENT_WHERE: &str =
    "parent.deleted_at IS NULL AND parent_project.deleted_at IS NULL";

/// Correlated subquery rendering the visible parent `parent_id_sql` as a `TaskRef` JSON object,
/// or NULL when there is none or it (or its project) is in the trash.
pub(super) fn parent_subquery(parent_id_sql: &str) -> String {
    format!(
        "(SELECT json_object('id', parent.id, 'project_id', parent.project_id, \
         'project_key', parent_project.project_key, 'title', parent.title) \
         FROM {VISIBLE_PARENT_FROM} WHERE parent.id = {parent_id_sql} AND {VISIBLE_PARENT_WHERE})"
    )
}

/// Condition true when `parent_id_sql` names a visible parent (false for NULL).
pub(super) fn has_visible_parent_sql(parent_id_sql: &str) -> String {
    format!(
        "EXISTS (SELECT 1 FROM {VISIBLE_PARENT_FROM} \
         WHERE parent.id = {parent_id_sql} AND {VISIBLE_PARENT_WHERE})"
    )
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

/// Visible ancestors of `task_id`, root first (`GET /tasks/{id}`). The walk stops at the first
/// hidden ancestor, as the `parent` field does, so the chain always ends at the task's parent.
pub(super) async fn ancestors(pool: &SqlitePool, task_id: Id) -> Result<Vec<TaskRef>, TaskError> {
    let sql = format!(
        "WITH RECURSIVE chain(id, project_id, project_key, title, parent_task_id, depth) AS ( \
             SELECT parent.id, parent.project_id, parent_project.project_key, parent.title, \
             parent.parent_task_id, 1 FROM tasks AS child, {VISIBLE_PARENT_FROM} \
             WHERE child.id = ? AND parent.id = child.parent_task_id AND {VISIBLE_PARENT_WHERE} \
             UNION ALL \
             SELECT parent.id, parent.project_id, parent_project.project_key, parent.title, \
             parent.parent_task_id, chain.depth + 1 FROM chain, {VISIBLE_PARENT_FROM} \
             WHERE parent.id = chain.parent_task_id AND {VISIBLE_PARENT_WHERE} AND chain.depth < ? \
         ) \
         SELECT id, project_id, project_key, title FROM chain ORDER BY depth DESC"
    );
    let rows = sqlx::query(&sql)
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

/// The recursive "every descendant of `root`" CTE shared by `live_descendants` and
/// `trashed_with` (the auto-close cascade reuses `live_descendants` instead of duplicating the
/// walk). `predicate` is a SQL fragment tested against `tasks.deleted_at` in
/// both the seed and recursive branches, e.g. `"IS NULL"` or `"= ?"`.
pub(super) fn descendants_sql(predicate: &str) -> String {
    format!(
        "WITH RECURSIVE subtree(id) AS ( \
             SELECT id FROM tasks WHERE parent_task_id = ? AND deleted_at {predicate} \
             UNION \
             SELECT tasks.id FROM tasks JOIN subtree ON tasks.parent_task_id = subtree.id \
             WHERE tasks.deleted_at {predicate} \
         ) \
         SELECT id FROM subtree ORDER BY id"
    )
}

/// Live descendants of `root` at every depth (not `root` itself).
pub(super) async fn live_descendants<'e, E>(executor: E, root: Id) -> Result<Vec<Id>, TaskError>
where
    E: sqlx::Executor<'e, Database = Sqlite>,
{
    sqlx::query_scalar::<_, String>(&descendants_sql("IS NULL"))
        .bind(root.to_string())
        .fetch_all(executor)
        .await?
        .into_iter()
        .map(parse_id)
        .collect()
}

/// Descendants of `root` trashed together with it: the walk only passes through tasks whose
/// `deleted_at` equals `deleted_at`, so a subtree trashed earlier on its own is left out.
pub(super) async fn trashed_with<'e, E>(
    executor: E,
    root: Id,
    deleted_at: i64,
) -> Result<Vec<Id>, TaskError>
where
    E: sqlx::Executor<'e, Database = Sqlite>,
{
    sqlx::query_scalar::<_, String>(&descendants_sql("= ?"))
        .bind(root.to_string())
        .bind(deleted_at)
        .bind(deleted_at)
        .fetch_all(executor)
        .await?
        .into_iter()
        .map(parse_id)
        .collect()
}

/// `trashed_with(root, root.deleted_at).len()` for every root in `roots` in one query (the
/// trash page's "+N sub-issues"). Roots with no such descendants are absent from the map.
pub(super) async fn trashed_with_counts(
    pool: &SqlitePool,
    roots: &[Id],
) -> Result<BTreeMap<Id, i64>, TaskError> {
    if roots.is_empty() {
        return Ok(BTreeMap::new());
    }
    let mut query = sqlx::QueryBuilder::<Sqlite>::new(
        "WITH RECURSIVE subtree(root, id, deleted_at) AS ( \
             SELECT root.id, tasks.id, tasks.deleted_at FROM tasks \
             JOIN tasks AS root ON root.id = tasks.parent_task_id AND root.deleted_at = tasks.deleted_at \
             WHERE root.id IN (",
    );
    let mut ids = query.separated(", ");
    for root in roots {
        ids.push_bind(root.to_string());
    }
    query.push(
        ") \
         UNION \
         SELECT subtree.root, tasks.id, tasks.deleted_at FROM tasks \
         JOIN subtree ON tasks.parent_task_id = subtree.id AND tasks.deleted_at = subtree.deleted_at \
         ) \
         SELECT root, COUNT(*) AS count FROM subtree GROUP BY root",
    );
    query
        .build()
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(|row| Ok((parse_id(row.get("root"))?, row.get("count"))))
        .collect()
}

/// Whether `task_id` itself is out of the trash. Restore (and GitHub's un-trash) detach a task
/// only from a parent that is still in the trash (spec §5); a parent whose project is in the
/// trash stays linked, like the children of any task in a trashed project, and shows again when
/// that project is restored. `validate_parent_in_tx` is stricter because it gates picking a new
/// parent, which must be visible.
pub(super) async fn task_is_live_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    task_id: Id,
) -> Result<bool, TaskError> {
    Ok(sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM tasks WHERE id = ? AND deleted_at IS NULL)",
    )
    .bind(task_id.to_string())
    .fetch_one(&mut **tx)
    .await?)
}

/// A task the automation changed in the same request (`auto_closed` in PATCH and bulk responses).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct AutoClosed {
    #[schema(value_type = String)]
    pub id: Id,
    #[schema(value_type = String)]
    pub status_id: Id,
}

/// Who and when, for the audit rows the automation writes. `service_account` is set when an
/// integration (GitHub sync) made the triggering change; its audit rows then carry no user actor.
#[derive(Clone, Copy)]
pub(super) struct AutomationActor<'a> {
    pub(super) workspace_id: Id,
    pub(super) actor_id: Id,
    pub(super) service_account: Option<(Id, &'a str)>,
    pub(super) request_id: &'a str,
    pub(super) now: TimestampMillis,
}

/// The fields the automation compares before and after a write.
#[derive(Clone, Debug)]
pub(super) struct TaskSnapshot {
    category: String,
    pub(super) parent_task_id: Option<Id>,
    project_id: Id,
}

/// Closed for the automation: the codebase's done categories (`completed`, `cancelled`,
/// `duplicate`).
fn is_closed(category: &str) -> bool {
    matches!(category, "completed" | "cancelled" | "duplicate")
}

pub(super) async fn snapshot_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    task_id: Id,
) -> Result<Option<TaskSnapshot>, TaskError> {
    let Some(row) = sqlx::query(
        "SELECT tasks.project_id, tasks.parent_task_id, task_statuses.category FROM tasks \
         JOIN task_statuses ON task_statuses.id = tasks.status_id WHERE tasks.id = ?",
    )
    .bind(task_id.to_string())
    .fetch_optional(&mut **tx)
    .await?
    else {
        return Ok(None);
    };
    Ok(Some(TaskSnapshot {
        category: row.get("category"),
        parent_task_id: row
            .get::<Option<String>, _>("parent_task_id")
            .map(parse_id)
            .transpose()?,
        project_id: parse_id(row.get("project_id"))?,
    }))
}

/// The id of the project `task_id` belongs to, even when the task or its project is in the
/// trash (audit rows use it to build identifiers of hidden tasks).
pub(super) async fn project_id_of_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    task_id: Option<Id>,
) -> Result<Option<Id>, TaskError> {
    let Some(task_id) = task_id else {
        return Ok(None);
    };
    sqlx::query_scalar::<_, String>("SELECT project_id FROM tasks WHERE id = ?")
        .bind(task_id.to_string())
        .fetch_optional(&mut **tx)
        .await?
        .map(parse_id)
        .transpose()
}

/// Runs rules B then A for `task_id`, which the caller's own write changed from `before` to
/// `after` (spec §4). `after` must be read right after that write and before any automation
/// runs, so a task the automation closed never looks like it was closed by the caller (rule B
/// only follows the caller's writes, whatever order a bulk request lists them in).
pub(super) async fn run_automation_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    actor: AutomationActor<'_>,
    task_id: Id,
    before: &TaskSnapshot,
    after: &TaskSnapshot,
    auto_closed: &mut Vec<AutoClosed>,
) -> Result<(), TaskError> {
    let became_closed = !is_closed(&before.category) && is_closed(&after.category);
    // A child moving between closed categories into `completed` (cancelled -> completed) can
    // now satisfy rule A's "at least one completed" clause.
    let became_completed = before.category != "completed" && after.category == "completed";
    if became_closed
        && matches!(after.category.as_str(), "completed" | "cancelled")
        && closes_sub_issues_in_tx(tx, after.project_id).await?
    {
        close_descendants_in_tx(tx, actor, task_id, &after.category, auto_closed).await?;
    }
    // A closed task moved under a new parent can complete it too (rule A).
    let joined_closed = before.parent_task_id != after.parent_task_id && is_closed(&after.category);
    if (became_closed || became_completed || joined_closed)
        && let Some(parent_id) = after.parent_task_id
    {
        close_ancestors_in_tx(tx, actor, parent_id, task_id, auto_closed).await?;
    }
    if before.parent_task_id != after.parent_task_id
        && let Some(old_parent) = before.parent_task_id
    {
        close_ancestors_in_tx(tx, actor, old_parent, task_id, auto_closed).await?;
    }
    Ok(())
}

async fn closes_sub_issues_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    project_id: Id,
) -> Result<bool, TaskError> {
    Ok(
        sqlx::query_scalar::<_, bool>("SELECT auto_close_sub_issues FROM projects WHERE id = ?")
            .bind(project_id.to_string())
            .fetch_optional(&mut **tx)
            .await?
            .unwrap_or(false),
    )
}

/// The project's first status of `category` (lowest position, then id), if it has one.
async fn first_status_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    project_id: Id,
    category: &str,
) -> Result<Option<Id>, TaskError> {
    sqlx::query_scalar::<_, String>(
        "SELECT id FROM task_statuses WHERE project_id = ? AND category = ? ORDER BY position, id LIMIT 1",
    )
    .bind(project_id.to_string())
    .bind(category)
    .fetch_optional(&mut **tx)
    .await?
    .map(parse_id)
    .transpose()
}

/// Rule B: every open live descendant of `root` (at every depth, walking through closed ones)
/// moves to its own project's first status of `category` (`completed` or `cancelled`); tasks in
/// trashed projects and projects without such a status are skipped.
async fn close_descendants_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    actor: AutomationActor<'_>,
    root: Id,
    category: &str,
    auto_closed: &mut Vec<AutoClosed>,
) -> Result<(), TaskError> {
    let mut targets: BTreeMap<Id, Option<Id>> = BTreeMap::new();
    for id in live_descendants(&mut **tx, root).await? {
        let Some(row) = sqlx::query(
            "SELECT tasks.project_id, tasks.status_id FROM tasks \
             JOIN projects ON projects.id = tasks.project_id \
             JOIN task_statuses ON task_statuses.id = tasks.status_id \
             WHERE tasks.id = ? AND projects.deleted_at IS NULL \
             AND task_statuses.category NOT IN ('completed', 'cancelled', 'duplicate')",
        )
        .bind(id.to_string())
        .fetch_optional(&mut **tx)
        .await?
        else {
            continue;
        };
        let project_id = parse_id(row.get("project_id"))?;
        let from = parse_id(row.get("status_id"))?;
        let target = match targets.get(&project_id) {
            Some(target) => *target,
            None => {
                let target = first_status_in_tx(tx, project_id, category).await?;
                targets.insert(project_id, target);
                target
            }
        };
        if let Some(to) = target {
            close_in_tx(tx, actor, id, from, to, root, "parent_closed", auto_closed).await?;
        }
    }
    Ok(())
}

/// Rule A for `parent_id`, then upward: an open parent whose live direct children are all closed
/// with at least one completed moves to its project's first completed status.
pub(super) async fn close_ancestors_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    actor: AutomationActor<'_>,
    parent_id: Id,
    source_id: Id,
    auto_closed: &mut Vec<AutoClosed>,
) -> Result<(), TaskError> {
    let (mut parent_id, mut source_id) = (parent_id, source_id);
    for _ in 0..MAX_DEPTH {
        let Some(row) = sqlx::query(
            "SELECT tasks.status_id, tasks.project_id, tasks.parent_task_id, task_statuses.category, \
             projects.auto_close_parent FROM tasks \
             JOIN projects ON projects.id = tasks.project_id \
             JOIN task_statuses ON task_statuses.id = tasks.status_id \
             WHERE tasks.id = ? AND tasks.deleted_at IS NULL AND projects.deleted_at IS NULL",
        )
        .bind(parent_id.to_string())
        .fetch_optional(&mut **tx)
        .await?
        else {
            return Ok(());
        };
        let category: String = row.get("category");
        if !row.get::<bool, _>("auto_close_parent") || is_closed(&category) {
            return Ok(());
        }
        let (total, open, completed): (i64, i64, i64) = sqlx::query_as(
            "SELECT COUNT(*), \
             COALESCE(SUM(task_statuses.category NOT IN ('completed', 'cancelled', 'duplicate')), 0), \
             COALESCE(SUM(task_statuses.category = 'completed'), 0) \
             FROM tasks JOIN projects ON projects.id = tasks.project_id \
             JOIN task_statuses ON task_statuses.id = tasks.status_id \
             WHERE tasks.parent_task_id = ? AND tasks.deleted_at IS NULL AND projects.deleted_at IS NULL",
        )
        .bind(parent_id.to_string())
        .fetch_one(&mut **tx)
        .await?;
        if total == 0 || open > 0 || completed == 0 {
            return Ok(());
        }
        let project_id = parse_id(row.get("project_id"))?;
        let Some(target) = first_status_in_tx(tx, project_id, "completed").await? else {
            return Ok(());
        };
        let from = parse_id(row.get("status_id"))?;
        close_in_tx(
            tx,
            actor,
            parent_id,
            from,
            target,
            source_id,
            "sub_issues_done",
            auto_closed,
        )
        .await?;
        match row
            .get::<Option<String>, _>("parent_task_id")
            .map(parse_id)
            .transpose()?
        {
            Some(next) => {
                source_id = parent_id;
                parent_id = next;
            }
            None => return Ok(()),
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn close_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    actor: AutomationActor<'_>,
    task_id: Id,
    from: Id,
    to: Id,
    source_id: Id,
    reason: &str,
    auto_closed: &mut Vec<AutoClosed>,
) -> Result<(), TaskError> {
    // updated_at drives the completed_at trigger from 0033.
    sqlx::query(
        "UPDATE tasks SET status_id = ?, version = version + 1, updated_at = ? WHERE id = ? AND deleted_at IS NULL",
    )
    .bind(to.to_string())
    .bind(actor.now.as_millis())
    .bind(task_id.to_string())
    .execute(&mut **tx)
    .await?;
    // The source's project lets the activity feed build its identifier (ORB-91C0).
    let source_project_id: String = sqlx::query_scalar("SELECT project_id FROM tasks WHERE id = ?")
        .bind(source_id.to_string())
        .fetch_one(&mut **tx)
        .await?;
    let mut metadata = json!({
        "source_task_id": source_id,
        "source_project_id": source_project_id,
        "from_status_id": from,
        "to_status_id": to,
        "reason": reason,
    });
    // Same shape as the other integration audits (`record_relation_audit`).
    let actor_id = match actor.service_account {
        Some((id, name)) => {
            metadata["actor_service_account_id"] = json!(id);
            metadata["actor_service_account_name"] = json!(name);
            None
        }
        None => Some(actor.actor_id),
    };
    audit::record(
        tx,
        actor.workspace_id,
        actor_id,
        "task.auto_closed",
        AuditOutcome::Success,
        "task",
        Some(task_id),
        actor.request_id,
        metadata,
        actor.now,
    )
    .await?;
    auto_closed.push(AutoClosed {
        id: task_id,
        status_id: to,
    });
    Ok(())
}
