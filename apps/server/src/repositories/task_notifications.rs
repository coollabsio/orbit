//! Task subscribers and the inbox notifications of a task.
//!
//! The inbox has one row for each (recipient, task): a new event updates the row, makes it
//! unread and brings it back from snooze or the archive. Everything here runs in the
//! transaction of the change that causes the event.

use orbit_platform::{Id, TimestampMillis};
use sqlx::{Sqlite, Transaction};

use super::tasks::{TaskError, TaskRepository, parse_id, require_task_tx};
use crate::push::NotificationKind;

/// One event of a task. `actor` gets no notification; `None` is GitHub.
#[derive(Clone, Copy)]
pub(super) struct TaskEvent {
    pub(super) workspace_id: Id,
    pub(super) actor: Option<Id>,
    pub(super) task_id: Id,
    pub(super) now: TimestampMillis,
}

/// Subscribes `users` to the task. `force` (an assignment or a mention) also subscribes a
/// person who unsubscribed; without it (the creator, a comment author) their choice stays.
pub(super) async fn subscribe_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    task_id: Id,
    users: &[Id],
    force: bool,
    now: TimestampMillis,
) -> Result<(), sqlx::Error> {
    let sql = if force {
        "INSERT INTO task_subscribers (task_id, user_id, subscribed, created_at, updated_at) \
         VALUES (?, ?, 1, ?, ?) ON CONFLICT (task_id, user_id) DO UPDATE SET subscribed = 1, \
         updated_at = excluded.updated_at WHERE task_subscribers.subscribed = 0"
    } else {
        "INSERT OR IGNORE INTO task_subscribers (task_id, user_id, subscribed, created_at, updated_at) \
         VALUES (?, ?, 1, ?, ?)"
    };
    for user in users {
        sqlx::query(sql)
            .bind(task_id.to_string())
            .bind(user.to_string())
            .bind(now.as_millis())
            .bind(now.as_millis())
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}

/// Writes the task's inbox row of each recipient except the actor. The callers make sure that
/// each recipient is a member of the workspace.
pub(super) async fn notify_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    event: TaskEvent,
    kind: NotificationKind,
    comment_id: Option<Id>,
    recipients: &[Id],
) -> Result<(), sqlx::Error> {
    // An unread assignment or mention stays what it is when an event for subscribers follows:
    // the row only moves up and comes back from snooze or the archive.
    let keep = if kind.is_direct() {
        "0"
    } else {
        "(notifications.read_at IS NULL AND notifications.kind IN ('task_assigned', 'comment_mentioned', 'task_mentioned'))"
    };
    let sql = format!(
        "INSERT INTO notifications (id, workspace_id, recipient_user_id, actor_user_id, kind, task_id, \
         comment_id, dedupe_key, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?) \
         ON CONFLICT (dedupe_key) DO UPDATE SET \
         kind = CASE WHEN {keep} THEN notifications.kind ELSE excluded.kind END, \
         actor_user_id = CASE WHEN {keep} THEN notifications.actor_user_id ELSE excluded.actor_user_id END, \
         comment_id = CASE WHEN {keep} THEN notifications.comment_id ELSE excluded.comment_id END, \
         pushed_at = CASE WHEN {keep} THEN notifications.pushed_at ELSE NULL END, \
         created_at = excluded.created_at, read_at = NULL, snoozed_until = NULL, archived_at = NULL"
    );
    for recipient in recipients
        .iter()
        .filter(|recipient| Some(**recipient) != event.actor)
    {
        sqlx::query(&sql)
            .bind(Id::new_v7().to_string())
            .bind(event.workspace_id.to_string())
            .bind(recipient.to_string())
            .bind(event.actor.map(|actor| actor.to_string()))
            .bind(kind.as_str())
            .bind(event.task_id.to_string())
            .bind(comment_id.map(|id| id.to_string()))
            .bind(format!(
                "{}:{recipient}:task:{}",
                event.workspace_id, event.task_id
            ))
            .bind(event.now.as_millis())
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}

/// Notifies the subscribers of the task, except `except` (the persons who got a more specific
/// notification of the same event). A subscriber who is suspended or left the workspace is
/// skipped.
pub(super) async fn notify_subscribers_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    event: TaskEvent,
    kind: NotificationKind,
    comment_id: Option<Id>,
    except: &[Id],
) -> Result<(), sqlx::Error> {
    let subscribers: Vec<String> = sqlx::query_scalar(
        "SELECT task_subscribers.user_id FROM task_subscribers \
         JOIN memberships ON memberships.user_id = task_subscribers.user_id \
         AND memberships.workspace_id = ? \
         JOIN users ON users.id = task_subscribers.user_id AND users.suspended_at IS NULL \
         WHERE task_subscribers.task_id = ? AND task_subscribers.subscribed = 1",
    )
    .bind(event.workspace_id.to_string())
    .bind(event.task_id.to_string())
    .fetch_all(&mut **tx)
    .await?;
    let recipients: Vec<Id> = subscribers
        .iter()
        .filter_map(|id| id.parse().ok())
        .filter(|id| !except.contains(id))
        .collect();
    notify_in_tx(tx, event, kind, comment_id, &recipients).await
}

/// Whether `task_id` has an open blocker other than `except`: a live task in a live project
/// that blocks it and is not completed, cancelled or a duplicate (`blocked` of a task record).
async fn blocked_by_other_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    task_id: Id,
    except: Id,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM task_relations \
         JOIN tasks AS blocker ON blocker.id = task_relations.task_id \
         JOIN projects ON projects.id = blocker.project_id \
         JOIN task_statuses ON task_statuses.id = blocker.status_id \
         WHERE task_relations.related_task_id = ? AND task_relations.type = 'blocks' \
         AND blocker.id <> ? AND blocker.deleted_at IS NULL AND projects.deleted_at IS NULL \
         AND task_statuses.category NOT IN ('completed', 'cancelled', 'duplicate'))",
    )
    .bind(task_id.to_string())
    .bind(except.to_string())
    .fetch_one(&mut **tx)
    .await
}

async fn is_open_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    status_id: Id,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT category NOT IN ('completed', 'cancelled', 'duplicate') FROM task_statuses WHERE id = ?",
    )
    .bind(status_id.to_string())
    .fetch_one(&mut **tx)
    .await
}

/// `blocker` started (`blocks`) or stopped to be an open blocker of `blocked_id`. The
/// subscribers of `blocked_id` hear of it only when its `blocked` value changed, which is
/// when it has no other open blocker.
async fn blocker_changed_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    blocker: TaskEvent,
    blocked_id: Id,
    blocks: bool,
) -> Result<(), sqlx::Error> {
    if blocked_by_other_in_tx(tx, blocked_id, blocker.task_id).await? {
        return Ok(());
    }
    let kind = if blocks {
        NotificationKind::TaskBlocked
    } else {
        NotificationKind::TaskUnblocked
    };
    let event = TaskEvent {
        task_id: blocked_id,
        ..blocker
    };
    notify_subscribers_in_tx(tx, event, kind, None, &[]).await
}

/// A "blocks" relation from `blocker` to `blocked_id` was added (`added`) or removed. Call it
/// after the write.
pub(super) async fn blocks_relation_changed_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    blocker: TaskEvent,
    blocked_id: Id,
    added: bool,
) -> Result<(), sqlx::Error> {
    let open: Option<bool> = sqlx::query_scalar(
        "SELECT task_statuses.category NOT IN ('completed', 'cancelled', 'duplicate') FROM tasks \
         JOIN task_statuses ON task_statuses.id = tasks.status_id WHERE tasks.id = ?",
    )
    .bind(blocker.task_id.to_string())
    .fetch_optional(&mut **tx)
    .await?;
    if open == Some(true) {
        blocker_changed_in_tx(tx, blocker, blocked_id, added).await?;
    }
    Ok(())
}

/// The status of the task changed from `from` to `to`. Call it after the write. The
/// subscribers except `except` get `task_status_changed`; when the task closed or reopened,
/// the subscribers of the tasks that it blocks get `task_unblocked` or `task_blocked`.
pub(super) async fn status_changed_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    event: TaskEvent,
    from: Id,
    to: Id,
    except: &[Id],
) -> Result<(), sqlx::Error> {
    if from == to {
        return Ok(());
    }
    notify_subscribers_in_tx(tx, event, NotificationKind::TaskStatusChanged, None, except).await?;
    let was_open = is_open_in_tx(tx, from).await?;
    let is_open = is_open_in_tx(tx, to).await?;
    if was_open == is_open {
        return Ok(());
    }
    let blocked: Vec<String> = sqlx::query_scalar(
        "SELECT task_relations.related_task_id FROM task_relations \
         JOIN tasks ON tasks.id = task_relations.related_task_id \
         JOIN projects ON projects.id = tasks.project_id \
         WHERE task_relations.task_id = ? AND task_relations.type = 'blocks' \
         AND tasks.deleted_at IS NULL AND projects.deleted_at IS NULL",
    )
    .bind(event.task_id.to_string())
    .fetch_all(&mut **tx)
    .await?;
    for blocked_id in blocked.iter().filter_map(|id| id.parse().ok()) {
        blocker_changed_in_tx(tx, event, blocked_id, is_open).await?;
    }
    Ok(())
}

/// What `PATCH notifications/{id}` changes; `None` keeps the stored value.
#[derive(Clone, Copy, Debug, Default)]
pub struct NotificationPatch {
    pub read: Option<bool>,
    /// `Some(None)` ends a snooze.
    pub snoozed_until: Option<Option<TimestampMillis>>,
    pub archived: Option<bool>,
}

impl TaskRepository {
    /// The persons who get the later events of the task.
    pub async fn task_subscribers(
        &self,
        workspace_id: Id,
        task_id: Id,
        actor_id: Id,
    ) -> Result<Vec<Id>, TaskError> {
        let mut tx = self.database().immediate_transaction().await?;
        require_task_tx(&mut tx, workspace_id, task_id, actor_id).await?;
        let ids = subscriber_ids_in_tx(&mut tx, task_id).await?;
        tx.commit().await?;
        Ok(ids)
    }

    /// Subscribes the caller to the task or records that they unsubscribed.
    pub async fn set_task_subscription(
        &self,
        workspace_id: Id,
        task_id: Id,
        actor_id: Id,
        subscribed: bool,
        now: TimestampMillis,
    ) -> Result<Vec<Id>, TaskError> {
        let mut tx = self.database().immediate_transaction().await?;
        require_task_tx(&mut tx, workspace_id, task_id, actor_id).await?;
        sqlx::query(
            "INSERT INTO task_subscribers (task_id, user_id, subscribed, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?) ON CONFLICT (task_id, user_id) DO UPDATE SET \
             subscribed = excluded.subscribed, updated_at = excluded.updated_at",
        )
        .bind(task_id.to_string())
        .bind(actor_id.to_string())
        .bind(subscribed)
        .bind(now.as_millis())
        .bind(now.as_millis())
        .execute(&mut *tx)
        .await?;
        let ids = subscriber_ids_in_tx(&mut tx, task_id).await?;
        tx.commit().await?;
        Ok(ids)
    }
}

async fn subscriber_ids_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    task_id: Id,
) -> Result<Vec<Id>, TaskError> {
    sqlx::query_scalar::<_, String>(
        "SELECT user_id FROM task_subscribers WHERE task_id = ? AND subscribed = 1 \
         ORDER BY created_at, user_id",
    )
    .bind(task_id.to_string())
    .fetch_all(&mut **tx)
    .await?
    .into_iter()
    .map(parse_id)
    .collect()
}
