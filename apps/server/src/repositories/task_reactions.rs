//! Emoji reactions on task comments. A reaction makes no notification and no audit row; the
//! other clients hear of it through a realtime event of the workspace.

use std::collections::HashMap;

use orbit_platform::{Id, TimestampMillis};
use serde::Serialize;
use sqlx::{Row, Sqlite, Transaction};
use utoipa::ToSchema;

use super::tasks::{CommentRecord, TaskError, TaskRepository, comment_in_tx, require_task_tx};

/// A comment can have this many different emoji, as a chat message.
const MAX_REACTIONS: usize = 20;
/// A Unicode emoji (with its modifiers) or `:name:` of a custom emoji.
const EMOJI_MAX_BYTES: usize = 64;

/// One emoji on a comment.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct CommentReaction {
    /// A Unicode emoji, or `:name:` for a custom emoji of the workspace.
    pub emoji: String,
    pub count: usize,
    /// The caller is one of them.
    pub reacted: bool,
    /// Who reacted, the first person first.
    #[schema(value_type = Vec<String>)]
    pub user_ids: Vec<Id>,
}

/// The reactions of the comments of a task, for `viewer`, each list in the order the emoji
/// were first used.
pub(super) async fn reactions_of_task<'e, E>(
    executor: E,
    task_id: Id,
    viewer: Id,
) -> Result<HashMap<Id, Vec<CommentReaction>>, TaskError>
where
    E: sqlx::Executor<'e, Database = Sqlite>,
{
    let rows = sqlx::query(
        "SELECT task_comment_reactions.comment_id, task_comment_reactions.emoji, \
         task_comment_reactions.user_id FROM task_comment_reactions \
         JOIN task_comments ON task_comments.id = task_comment_reactions.comment_id \
         WHERE task_comments.task_id = ? \
         ORDER BY task_comment_reactions.created_at, task_comment_reactions.user_id",
    )
    .bind(task_id.to_string())
    .fetch_all(executor)
    .await?;
    let mut reactions: HashMap<Id, Vec<CommentReaction>> = HashMap::new();
    for row in rows {
        let (Ok(comment_id), Ok(user_id)) = (
            row.get::<String, _>("comment_id").parse::<Id>(),
            row.get::<String, _>("user_id").parse::<Id>(),
        ) else {
            continue;
        };
        let emoji: String = row.get("emoji");
        let list = reactions.entry(comment_id).or_default();
        let index = match list.iter().position(|reaction| reaction.emoji == emoji) {
            Some(index) => index,
            None => {
                list.push(CommentReaction {
                    emoji,
                    count: 0,
                    reacted: false,
                    user_ids: Vec::new(),
                });
                list.len() - 1
            }
        };
        list[index].count += 1;
        list[index].reacted |= user_id == viewer;
        list[index].user_ids.push(user_id);
    }
    Ok(reactions)
}

/// Tells the realtime clients of the workspace that something changed, with no audit row:
/// what the `audit_workspace_realtime` trigger does for an audit event.
async fn signal_workspace_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    now: TimestampMillis,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO realtime_sequences (workspace_id, sequence) VALUES (?, 1) \
         ON CONFLICT (workspace_id) DO UPDATE SET sequence = sequence + 1",
    )
    .bind(workspace_id.to_string())
    .execute(&mut **tx)
    .await?;
    sqlx::query(
        "INSERT INTO outbox_events (id, scope, sequence, workspace_id, topic, payload_json, created_at) \
         SELECT ?, workspace_id, sequence, workspace_id, 'workspace.changed', '{}', ? \
         FROM realtime_sequences WHERE workspace_id = ?",
    )
    .bind(Id::new_v7().to_string())
    .bind(now.as_millis())
    .bind(workspace_id.to_string())
    .execute(&mut **tx)
    .await?;
    Ok(())
}

impl TaskRepository {
    /// Adds (`on`) or removes the caller's reaction to a comment. Both are idempotent.
    #[allow(clippy::too_many_arguments)]
    pub async fn set_comment_reaction(
        &self,
        workspace_id: Id,
        task_id: Id,
        comment_id: Id,
        actor_id: Id,
        emoji: &str,
        on: bool,
        now: TimestampMillis,
    ) -> Result<CommentRecord, TaskError> {
        if emoji.is_empty()
            || emoji.len() > EMOJI_MAX_BYTES
            || emoji
                .chars()
                .any(|char| char.is_whitespace() || char.is_control())
        {
            return Err(TaskError::Invalid { field: "emoji" });
        }
        let mut tx = self.database().immediate_transaction().await?;
        let actor = require_task_tx(&mut tx, workspace_id, task_id, actor_id).await?;
        let mut comment = comment_in_tx(&mut tx, workspace_id, task_id, comment_id, actor).await?;
        let known = reactions_of_task(&mut *tx, task_id, actor_id)
            .await?
            .remove(&comment_id)
            .unwrap_or_default();
        let changed = if on {
            if !known.iter().any(|reaction| reaction.emoji == emoji) && known.len() >= MAX_REACTIONS
            {
                return Err(TaskError::Conflict);
            }
            sqlx::query(
                "INSERT INTO task_comment_reactions (comment_id, user_id, emoji, created_at) \
                 VALUES (?, ?, ?, ?) ON CONFLICT DO NOTHING",
            )
            .bind(comment_id.to_string())
            .bind(actor_id.to_string())
            .bind(emoji)
            .bind(now.as_millis())
            .execute(&mut *tx)
            .await?
        } else {
            sqlx::query(
                "DELETE FROM task_comment_reactions WHERE comment_id = ? AND user_id = ? AND emoji = ?",
            )
            .bind(comment_id.to_string())
            .bind(actor_id.to_string())
            .bind(emoji)
            .execute(&mut *tx)
            .await?
        };
        if changed.rows_affected() > 0 {
            signal_workspace_in_tx(&mut tx, workspace_id, now).await?;
        }
        comment.reactions = reactions_of_task(&mut *tx, task_id, actor_id)
            .await?
            .remove(&comment_id)
            .unwrap_or_default();
        tx.commit().await?;
        Ok(comment)
    }
}
