//! Each member's read state: cursors, unread and mention counters, notify level, favorites and
//! thread follows.

use std::collections::HashSet;

use orbit_platform::Id;
use orbit_platform::{Row, SqlBuilder};
use serde::Serialize;
use sqlx::AnyConnection;
use utoipa::ToSchema;

use super::messages::{mark_read_to, upsert_thread_member};
use super::{
    CONVERSATION_COLUMNS, ChatError, ChatEvent, ChatRepository, Conversation, ConversationKind,
    ConversationStateRecord, Events, MEMBER_COLUMNS, MessageRecord, NotifyLevel, Recipients,
    ThreadMember, ThreadStateRecord, Written, conversation_from_row, current_state, finish,
    id_list, in_ids, load_access, load_actor, load_message, load_message_access,
    load_thread_member, member_from_row, member_ids, parse_id, parse_optional_id, public_for,
    reachable_dm, thread_baseline,
};

/// Cursors that one `restore_read` call may put back.
const MAX_RESTORE: usize = 1000;

/// Conversations whose thread baselines go in one `unread_threads` query: two binds each, far
/// below the bind limits of SQLite and Postgres.
const BASELINE_CHUNK: usize = 500;

#[derive(Clone, Copy, Debug, Default)]
pub struct StateUpdate {
    pub notify: Option<NotifyLevel>,
    pub favorite: Option<bool>,
}

/// The states that "mark all as read" changed, as they were before, so the caller can undo it.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct ReadSnapshot {
    pub states: Vec<ConversationStateRecord>,
    pub threads: Vec<ThreadStateRecord>,
}

#[derive(Clone, Copy, Debug)]
pub struct ConversationCursor {
    pub conversation_id: Id,
    pub last_read_message_id: Option<Id>,
}

#[derive(Clone, Copy, Debug)]
pub struct ThreadCursor {
    pub root_id: Id,
    pub last_read_reply_id: Option<Id>,
}

impl ChatRepository {
    /// `NotFound` unless the caller is a member of the workspace.
    pub async fn require_member(&self, workspace_id: Id, actor_id: Id) -> Result<(), ChatError> {
        let mut conn = self.database.pool().acquire().await?;
        load_actor(&mut conn, workspace_id, actor_id).await?;
        Ok(())
    }

    /// The caller's state in every conversation it is a member of. Read from the counters: no
    /// message is counted here.
    pub async fn list_states(
        &self,
        workspace_id: Id,
        actor_id: Id,
    ) -> Result<Vec<ConversationStateRecord>, ChatError> {
        let mut tx = self.database.pool().begin().await?;
        load_actor(&mut tx, workspace_id, actor_id).await?;
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT {CONVERSATION_COLUMNS}, {MEMBER_COLUMNS} FROM chat_members m \
             JOIN chat_conversations c ON c.id = m.conversation_id \
             WHERE m.workspace_id = $1 AND m.user_id = $2 AND c.archived_at IS NULL AND {}",
            reachable_dm()
        )))
        .bind(workspace_id.to_string())
        .bind(actor_id.to_string())
        .fetch_all(&mut *tx)
        .await?
        .iter()
        .map(|row| Ok(member_from_row(row)?.state(&conversation_from_row(row)?)))
        .collect()
    }

    /// Reads the conversation up to its newest message. Writes nothing when it is read already,
    /// so a client that repeats the call costs one read.
    pub async fn mark_read(
        &self,
        workspace_id: Id,
        actor_id: Id,
        conversation_id: Id,
    ) -> Result<Written<ConversationStateRecord>, ChatError> {
        let mut tx = self.database.immediate_transaction().await?;
        let access = load_access(&mut tx, workspace_id, actor_id, conversation_id).await?;
        let member = access.require_member()?;
        let mut events = Events::default();
        let newest = newest_main_id(&mut tx, conversation_id).await?;
        let read = member.read_count == access.conversation.message_count
            && member.mention_count == 0
            && member.broadcast_count == 0
            && (newest.is_none() || member.last_read_message_id == newest);
        if read {
            return Ok(events.written(member.state(&access.conversation)));
        }
        if mark_read_to(
            &mut tx,
            conversation_id,
            actor_id,
            newest.or(member.last_read_message_id),
        )
        .await?
        {
            events.inbox.push(actor_id);
        }
        let state = emit_state(
            &mut tx,
            workspace_id,
            conversation_id,
            actor_id,
            &mut events,
        )
        .await?;
        finish(tx, events, state).await
    }

    /// Moves the read cursor to just before the message. For a thread reply it is the thread's
    /// cursor that moves, and the caller follows the thread so that it can show as unread.
    pub async fn mark_unread(
        &self,
        workspace_id: Id,
        actor_id: Id,
        message_id: Id,
    ) -> Result<Written<ConversationStateRecord>, ChatError> {
        let mut tx = self.database.immediate_transaction().await?;
        let (access, message) =
            load_message_access(&mut tx, workspace_id, actor_id, message_id).await?;
        access.require_member()?;
        let conversation_id = message.conversation_id;
        let mut events = Events::default();
        match message.thread_root_id {
            Some(root_id) if !message.also_in_channel => {
                let root = load_message(&mut tx, root_id)
                    .await?
                    .ok_or(ChatError::NotFound)?;
                let previous = sqlx::query_scalar::<_, String>(
                    "SELECT id FROM chat_messages WHERE thread_root_id = $1 AND id < $2 \
                     ORDER BY id DESC LIMIT 1",
                )
                .bind(root_id.to_string())
                .bind(message.id.to_string())
                .fetch_optional(&mut *tx)
                .await?
                .map(parse_id)
                .transpose()?;
                set_thread_cursor(&mut tx, workspace_id, &root, actor_id, previous, true).await?;
                emit_thread_state(&mut tx, &root, actor_id, &mut events).await?;
            }
            _ => {
                let previous = sqlx::query_scalar::<_, String>(
                    "SELECT id FROM chat_messages m WHERE m.conversation_id = $1 \
                     AND (m.thread_root_id IS NULL OR m.also_in_channel = 1) AND m.id < $2 \
                     ORDER BY m.id DESC LIMIT 1",
                )
                .bind(conversation_id.to_string())
                .bind(message.id.to_string())
                .fetch_optional(&mut *tx)
                .await?
                .map(parse_id)
                .transpose()?;
                set_cursor(&mut tx, &access.conversation, actor_id, previous).await?;
            }
        }
        let state = emit_state(
            &mut tx,
            workspace_id,
            conversation_id,
            actor_id,
            &mut events,
        )
        .await?;
        finish(tx, events, state).await
    }

    /// Reads a thread up to its newest reply. It does not make the caller follow the thread.
    pub async fn mark_thread_read(
        &self,
        workspace_id: Id,
        actor_id: Id,
        root_id: Id,
    ) -> Result<Written<ThreadStateRecord>, ChatError> {
        let mut tx = self.database.immediate_transaction().await?;
        let root = load_root(&mut tx, workspace_id, actor_id, root_id).await?;
        let following = load_thread_member(&mut tx, root_id, actor_id)
            .await?
            .is_some_and(|member| member.following);
        upsert_thread_member(
            &mut tx,
            workspace_id,
            &root,
            actor_id,
            &ThreadMember {
                following,
                last_read_reply_id: root.last_reply_id,
                read_reply_count: root.reply_count,
                mention_count: 0,
            },
        )
        .await?;
        let mut events = Events::default();
        let state = emit_thread_state(&mut tx, &root, actor_id, &mut events).await?;
        finish(tx, events, state).await
    }

    /// Following starts from now: older replies do not turn unread.
    pub async fn set_thread_follow(
        &self,
        workspace_id: Id,
        actor_id: Id,
        root_id: Id,
        following: bool,
    ) -> Result<Written<ThreadStateRecord>, ChatError> {
        let mut tx = self.database.immediate_transaction().await?;
        let root = load_root(&mut tx, workspace_id, actor_id, root_id).await?;
        let current = load_thread_member(&mut tx, root_id, actor_id).await?;
        let starts = following && !current.as_ref().is_some_and(|member| member.following);
        let member = match current {
            Some(member) if !starts => ThreadMember {
                following,
                ..member
            },
            _ => ThreadMember {
                following,
                last_read_reply_id: root.last_reply_id,
                read_reply_count: root.reply_count,
                mention_count: 0,
            },
        };
        upsert_thread_member(&mut tx, workspace_id, &root, actor_id, &member).await?;
        let mut events = Events::default();
        let state = emit_thread_state(&mut tx, &root, actor_id, &mut events).await?;
        finish(tx, events, state).await
    }

    /// Reads every conversation and thread that has something unread. Returns their states as
    /// they were, for `restore_read`. It reads at most `MAX_RESTORE` threads, so that the undo
    /// accepts the snapshot; the others stay unread for a next call.
    pub async fn mark_all_read(
        &self,
        workspace_id: Id,
        actor_id: Id,
    ) -> Result<Written<ReadSnapshot>, ChatError> {
        let mut tx = self.database.immediate_transaction().await?;
        load_actor(&mut tx, workspace_id, actor_id).await?;
        let mut events = Events::default();

        let states = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT {CONVERSATION_COLUMNS}, {MEMBER_COLUMNS} FROM chat_members m \
             JOIN chat_conversations c ON c.id = m.conversation_id \
             WHERE m.workspace_id = $1 AND m.user_id = $2 AND c.archived_at IS NULL \
             AND c.message_count > m.read_count AND {}",
            reachable_dm()
        )))
        .bind(workspace_id.to_string())
        .bind(actor_id.to_string())
        .fetch_all(&mut *tx)
        .await?
        .iter()
        .map(|row| Ok(member_from_row(row)?.state(&conversation_from_row(row)?)))
        .collect::<Result<Vec<_>, ChatError>>()?;
        for state in &states {
            let newest = newest_main_id(&mut tx, state.conversation_id).await?;
            if mark_read_to(
                &mut tx,
                state.conversation_id,
                actor_id,
                newest.or(state.last_read_message_id),
            )
            .await?
                && !events.inbox.contains(&actor_id)
            {
                events.inbox.push(actor_id);
            }
            emit_state(
                &mut tx,
                workspace_id,
                state.conversation_id,
                actor_id,
                &mut events,
            )
            .await?;
        }

        let roots = sqlx::query_scalar::<_, String>(
            "SELECT t.root_id FROM chat_thread_members t JOIN chat_messages r ON r.id = t.root_id \
             WHERE t.workspace_id = $1 AND t.user_id = $2 AND t.following = 1 \
             AND r.reply_count > t.read_reply_count",
        )
        .bind(workspace_id.to_string())
        .bind(actor_id.to_string())
        .fetch_all(&mut *tx)
        .await?;
        let mut threads = Vec::with_capacity(roots.len());
        for root_id in roots {
            if threads.len() >= MAX_RESTORE {
                break;
            }
            let root_id = parse_id(root_id)?;
            let (Some(root), Some(member)) = (
                load_message(&mut tx, root_id).await?,
                load_thread_member(&mut tx, root_id, actor_id).await?,
            ) else {
                continue;
            };
            threads.push(member.state(&root));
            upsert_thread_member(
                &mut tx,
                workspace_id,
                &root,
                actor_id,
                &ThreadMember {
                    following: true,
                    last_read_reply_id: root.last_reply_id,
                    read_reply_count: root.reply_count,
                    mention_count: 0,
                },
            )
            .await?;
            emit_thread_state(&mut tx, &root, actor_id, &mut events).await?;
        }
        // Threads the caller does not follow: those of conversations it has not muted. A thread
        // without a row gets one, and its snapshot cursor puts the same replies back to unread.
        for thread in unread_threads(&mut tx, workspace_id, actor_id).await? {
            if threads.len() >= MAX_RESTORE {
                break;
            }
            if thread.state.following {
                continue;
            }
            let Some(root) = load_message(&mut tx, thread.state.root_id).await? else {
                continue;
            };
            threads.push(thread.state);
            upsert_thread_member(
                &mut tx,
                workspace_id,
                &root,
                actor_id,
                &ThreadMember {
                    following: false,
                    last_read_reply_id: root.last_reply_id,
                    read_reply_count: root.reply_count,
                    mention_count: 0,
                },
            )
            .await?;
            emit_thread_state(&mut tx, &root, actor_id, &mut events).await?;
        }
        finish(tx, events, ReadSnapshot { states, threads }).await
    }

    /// Puts read cursors back (the undo of `mark_all_read`) and counts again from them. Cursors
    /// of conversations and threads that are gone, or that the caller left, are skipped.
    pub async fn restore_read(
        &self,
        workspace_id: Id,
        actor_id: Id,
        states: Vec<ConversationCursor>,
        threads: Vec<ThreadCursor>,
    ) -> Result<Written<()>, ChatError> {
        if states.len() > MAX_RESTORE {
            return Err(ChatError::Invalid { field: "states" });
        }
        if threads.len() > MAX_RESTORE {
            return Err(ChatError::Invalid { field: "threads" });
        }
        let mut tx = self.database.immediate_transaction().await?;
        load_actor(&mut tx, workspace_id, actor_id).await?;
        let mut events = Events::default();
        for cursor in last_of_each(states, |cursor| cursor.conversation_id) {
            let access =
                match load_access(&mut tx, workspace_id, actor_id, cursor.conversation_id).await {
                    Ok(access) if access.member.is_some() => access,
                    Ok(_) | Err(ChatError::NotFound) => continue,
                    Err(error) => return Err(error),
                };
            set_cursor(
                &mut tx,
                &access.conversation,
                actor_id,
                cursor.last_read_message_id,
            )
            .await?;
            emit_state(
                &mut tx,
                workspace_id,
                cursor.conversation_id,
                actor_id,
                &mut events,
            )
            .await?;
        }
        for cursor in last_of_each(threads, |cursor| cursor.root_id) {
            let root = match load_root(&mut tx, workspace_id, actor_id, cursor.root_id).await {
                Ok(root) => root,
                Err(ChatError::NotFound) => continue,
                Err(error) => return Err(error),
            };
            let Some(member) = load_thread_member(&mut tx, root.id, actor_id).await? else {
                continue;
            };
            set_thread_cursor(
                &mut tx,
                workspace_id,
                &root,
                actor_id,
                cursor.last_read_reply_id,
                member.following,
            )
            .await?;
            emit_thread_state(&mut tx, &root, actor_id, &mut events).await?;
        }
        finish(tx, events, ()).await
    }

    /// The notify level and the favorite flag. A change of the notify level needs no recount:
    /// `@channel` and `@here` have their own counter, which a muted conversation does not show.
    pub async fn update_state(
        &self,
        workspace_id: Id,
        actor_id: Id,
        conversation_id: Id,
        update: StateUpdate,
    ) -> Result<Written<ConversationStateRecord>, ChatError> {
        let mut tx = self.database.immediate_transaction().await?;
        let access = load_access(&mut tx, workspace_id, actor_id, conversation_id).await?;
        let member = access.require_member()?;
        sqlx::query(
            "UPDATE chat_members SET notify = $1, favorite = $2 WHERE conversation_id = $3 AND user_id = $4",
        )
        .bind(update.notify.unwrap_or(member.notify).as_str())
        .bind(i64::from(update.favorite.unwrap_or(member.favorite)))
        .bind(conversation_id.to_string())
        .bind(actor_id.to_string())
        .execute(&mut *tx)
        .await?;
        let mut events = Events::default();
        let state = emit_state(
            &mut tx,
            workspace_id,
            conversation_id,
            actor_id,
            &mut events,
        )
        .await?;
        finish(tx, events, state).await
    }
}

/// What the live socket needs after a write, outside the write's transaction. The states are
/// absolute, so one that is read a moment later is still right.
impl ChatRepository {
    /// The states of `user_ids` in a conversation, for those who are members of it.
    pub async fn states_for(
        &self,
        conversation_id: Id,
        user_ids: &[Id],
    ) -> Result<Vec<(Id, ConversationStateRecord)>, ChatError> {
        if user_ids.is_empty() {
            return Ok(Vec::new());
        }
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT m.user_id, {CONVERSATION_COLUMNS}, {MEMBER_COLUMNS} FROM chat_members m \
             JOIN chat_conversations c ON c.id = m.conversation_id \
             WHERE m.conversation_id = $1 AND m.user_id {}",
            in_ids(&self.database, 2)
        )))
        .bind(conversation_id.to_string())
        .bind(id_list(user_ids.iter().copied()))
        .fetch_all(self.database.pool())
        .await?
        .iter()
        .map(|row| {
            Ok((
                parse_id(row.get("user_id"))?,
                member_from_row(row)?.state(&conversation_from_row(row)?),
            ))
        })
        .collect()
    }

    /// The thread states of those of `user_ids` who follow the thread and can read its conversation.
    pub async fn thread_states_for(
        &self,
        root_id: Id,
        user_ids: &[Id],
    ) -> Result<Vec<(Id, ThreadStateRecord)>, ChatError> {
        if user_ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut conn = self.database.pool().acquire().await?;
        let Some(root) = load_message(&mut conn, root_id).await? else {
            return Ok(Vec::new());
        };
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT user_id, last_read_reply_id, read_reply_count, mention_count \
             FROM chat_thread_members WHERE root_id = $1 AND following = 1 \
             AND user_id {} \
             AND EXISTS (SELECT 1 FROM chat_conversations c \
                         WHERE c.id = chat_thread_members.conversation_id \
                           AND ({} OR EXISTS ( \
                               SELECT 1 FROM chat_members m WHERE m.conversation_id = c.id \
                                 AND m.user_id = chat_thread_members.user_id)) \
                           AND {})",
            in_ids(&*conn, 2),
            public_for("chat_thread_members.user_id"),
            reachable_dm()
        )))
        .bind(root_id.to_string())
        .bind(id_list(user_ids.iter().copied()))
        .fetch_all(&mut *conn)
        .await?
        .iter()
        .map(|row| {
            let member = ThreadMember {
                following: true,
                last_read_reply_id: parse_optional_id(row.get("last_read_reply_id"))?,
                read_reply_count: row.get("read_reply_count"),
                mention_count: row.get("mention_count"),
            };
            Ok((parse_id(row.get("user_id"))?, member.state(&root)))
        })
        .collect()
    }

    /// Who sees that `actor_id` types in a conversation. Only a member may say so.
    pub async fn typing_recipients(
        &self,
        workspace_id: Id,
        actor_id: Id,
        conversation_id: Id,
    ) -> Result<Recipients, ChatError> {
        let mut conn = self.database.pool().acquire().await?;
        let access = load_access(&mut conn, workspace_id, actor_id, conversation_id).await?;
        access.require_member()?;
        access.require_open()?;
        if access.conversation.kind == ConversationKind::Public {
            Ok(Recipients::Workspace)
        } else {
            Ok(Recipients::Users(
                member_ids(&mut conn, conversation_id).await?,
            ))
        }
    }
}

/// A thread root that the caller can read.
async fn load_root(
    conn: &mut AnyConnection,
    workspace_id: Id,
    actor_id: Id,
    root_id: Id,
) -> Result<MessageRecord, ChatError> {
    let (_, root) = load_message_access(conn, workspace_id, actor_id, root_id).await?;
    if root.thread_root_id.is_some() {
        return Err(ChatError::NotFound);
    }
    Ok(root)
}

pub(super) async fn newest_main_id(
    conn: &mut AnyConnection,
    conversation_id: Id,
) -> Result<Option<Id>, ChatError> {
    sqlx::query_scalar::<_, String>(newest_main_sql())
        .bind(conversation_id.to_string())
        .fetch_optional(&mut *conn)
        .await?
        .map(parse_id)
        .transpose()
}

pub(super) const fn newest_main_sql() -> &'static str {
    "SELECT id FROM chat_messages m WHERE m.conversation_id = $1 \
     AND (m.thread_root_id IS NULL OR m.also_in_channel = 1) ORDER BY m.id DESC LIMIT 1"
}

/// The unread messages of a member after a cursor (or all of them), and how many of them
/// have `@channel` or `@here`. Binds: conversation, member, cursor.
pub(super) fn unread_count_sql(bounded: bool) -> String {
    let after = if bounded { "AND m.id > $3" } else { "" };
    format!(
        "SELECT COUNT(*) AS unread, CAST(COALESCE(SUM(CASE WHEN m.mention_channel <> 0 OR m.mention_here <> 0 THEN 1 ELSE 0 END), 0) AS BIGINT) AS broadcasts \
         FROM chat_messages m WHERE m.conversation_id = $1 \
         AND (m.thread_root_id IS NULL OR m.also_in_channel = 1) AND m.kind = 'message' \
         AND m.deleted_at IS NULL AND (m.author_id <> $2 OR m.webhook_id IS NOT NULL) {after}"
    )
}

/// The main-list messages after a cursor that mention a member. Binds: member, conversation,
/// cursor.
pub(super) fn mention_count_sql(bounded: bool) -> String {
    let after = if bounded { "AND message_id > $3" } else { "" };
    format!(
        "SELECT COUNT(*) FROM chat_message_mentions WHERE user_id = $1 AND conversation_id = $2 \
         AND in_main = 1 {after}"
    )
}

/// The caller's state after a change, as an event for the caller.
pub(super) async fn emit_state(
    conn: &mut AnyConnection,
    workspace_id: Id,
    conversation_id: Id,
    user_id: Id,
    events: &mut Events,
) -> Result<ConversationStateRecord, ChatError> {
    let state = current_state(conn, workspace_id, conversation_id, user_id)
        .await?
        .ok_or(ChatError::NotFound)?;
    events.user(
        user_id,
        ChatEvent::StateChanged {
            state: state.clone(),
        },
    );
    Ok(state)
}

async fn emit_thread_state(
    conn: &mut AnyConnection,
    root: &MessageRecord,
    user_id: Id,
    events: &mut Events,
) -> Result<ThreadStateRecord, ChatError> {
    let state = load_thread_member(conn, root.id, user_id)
        .await?
        .ok_or(ChatError::NotFound)?
        .state(root);
    events.user(
        user_id,
        ChatEvent::ThreadChanged {
            state: state.clone(),
        },
    );
    Ok(state)
}

/// A thread with unread replies, for the Unreads view and "mark all as read".
pub(super) struct UnreadThread {
    pub state: ThreadStateRecord,
    pub last_reply_id: Option<Id>,
}

/// The threads with unread replies, followed or not, in the open conversations the caller is a
/// member of and has not muted, newest reply first. A thread the caller has no state in counts
/// the replies after its thread baseline (`thread_baseline`).
pub(super) async fn unread_threads(
    conn: &mut AnyConnection,
    workspace_id: Id,
    actor_id: Id,
) -> Result<Vec<UnreadThread>, ChatError> {
    let scope = format!(
        "m.workspace_id = $1 AND m.user_id = $2 AND m.notify <> 'muted' \
         AND c.archived_at IS NULL AND {}",
        reachable_dm()
    );
    let mut threads = Vec::new();
    let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT t.root_id, t.conversation_id, t.following, t.last_read_reply_id, t.mention_count, \
         r.reply_count - t.read_reply_count AS unread, r.last_reply_id \
         FROM chat_members m JOIN chat_conversations c ON c.id = m.conversation_id \
         JOIN chat_thread_members t ON t.conversation_id = m.conversation_id AND t.user_id = m.user_id \
         JOIN chat_messages r ON r.id = t.root_id \
         WHERE {scope} AND r.reply_count > t.read_reply_count"
    )))
    .bind(workspace_id.to_string())
    .bind(actor_id.to_string())
    .fetch_all(&mut *conn)
    .await?;
    for row in &rows {
        let following: bool = row.get("following");
        threads.push(UnreadThread {
            state: ThreadStateRecord {
                root_id: parse_id(row.get("root_id"))?,
                conversation_id: parse_id(row.get("conversation_id"))?,
                following,
                last_read_reply_id: parse_optional_id(row.get("last_read_reply_id"))?,
                unread_replies: row.get("unread"),
                mention_count: if following {
                    row.get("mention_count")
                } else {
                    0
                },
            },
            last_reply_id: parse_optional_id(row.get("last_reply_id"))?,
        });
    }

    let members = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT m.conversation_id, m.joined_at, m.threads_read_to FROM chat_members m \
         JOIN chat_conversations c ON c.id = m.conversation_id WHERE {scope}"
    )))
    .bind(workspace_id.to_string())
    .bind(actor_id.to_string())
    .fetch_all(&mut *conn)
    .await?;
    // One query for each chunk of conversations, not one for each conversation: the baselines go
    // in as a `VALUES` table. `chat_messages_threads` (partial: `reply_count > 0`) finds the roots
    // with a reply after the baseline; `chat_messages_thread` serves the two subqueries.
    for chunk in members.chunks(BASELINE_CHUNK) {
        let mut sql = SqlBuilder::new("WITH b (conversation_id, baseline) AS (VALUES ");
        for (index, member) in chunk.iter().enumerate() {
            let conversation_id: String = member.get("conversation_id");
            let baseline = thread_baseline(member.get("joined_at"), member.get("threads_read_to"));
            sql.push(if index == 0 { "(" } else { ", (" })
                .push_bind(conversation_id)
                .push(", ")
                .push_bind(baseline)
                .push(")");
        }
        sql.push(
            ") SELECT r.id, r.conversation_id, r.last_reply_id, \
             (SELECT MAX(x.id) FROM chat_messages x WHERE x.thread_root_id = r.id \
              AND x.id <= b.baseline) AS cursor, \
             (SELECT COUNT(*) FROM chat_messages x WHERE x.thread_root_id = r.id \
              AND x.id > b.baseline AND x.author_id <> ",
        )
        .push_bind(actor_id.to_string())
        .push(
            ") AS unread \
             FROM b JOIN chat_messages r ON r.conversation_id = b.conversation_id \
             WHERE r.reply_count > 0 AND r.last_reply_id > b.baseline \
             AND r.thread_root_id IS NULL AND NOT EXISTS (SELECT 1 FROM chat_thread_members t \
             WHERE t.root_id = r.id AND t.user_id = ",
        )
        .push_bind(actor_id.to_string())
        .push(")");
        for row in &sql.build().fetch_all(&mut *conn).await? {
            let unread: i64 = row.get("unread");
            if unread == 0 {
                continue;
            }
            threads.push(UnreadThread {
                state: ThreadStateRecord {
                    root_id: parse_id(row.get("id"))?,
                    conversation_id: parse_id(row.get("conversation_id"))?,
                    following: false,
                    last_read_reply_id: parse_optional_id(row.get("cursor"))?,
                    unread_replies: unread,
                    mention_count: 0,
                },
                last_reply_id: parse_optional_id(row.get("last_reply_id"))?,
            });
        }
    }
    threads.sort_by_key(|thread| std::cmp::Reverse(thread.last_reply_id));
    Ok(threads)
}

/// The last entry of each key, in the order of the list. A restore counts a conversation (or a
/// thread) once, however often the request names it.
fn last_of_each<T>(entries: Vec<T>, key: impl Fn(&T) -> Id) -> Vec<T> {
    let mut seen = HashSet::new();
    let mut kept: Vec<T> = entries
        .into_iter()
        .rev()
        .filter(|entry| seen.insert(key(entry)))
        .collect();
    kept.reverse();
    kept
}

/// Puts a member's cursor at `cursor` and counts what is unread after it. This is the one place
/// that counts messages; its cost is the number of main-list rows after the cursor.
async fn set_cursor(
    conn: &mut AnyConnection,
    conversation: &Conversation,
    user_id: Id,
    cursor: Option<Id>,
) -> Result<(), ChatError> {
    let sql = unread_count_sql(cursor.is_some());
    let mut query = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(conversation.id.to_string())
        .bind(user_id.to_string());
    if let Some(cursor) = cursor {
        query = query.bind(cursor.to_string());
    }
    let row = query.fetch_one(&mut *conn).await?;
    let unread: i64 = row.get("unread");
    let broadcasts: i64 = row.get("broadcasts");

    let sql = mention_count_sql(cursor.is_some());
    let mut query = sqlx::query_scalar::<_, i64>(sqlx::AssertSqlSafe(sql))
        .bind(user_id.to_string())
        .bind(conversation.id.to_string());
    if let Some(cursor) = cursor {
        query = query.bind(cursor.to_string());
    }
    let mentions = query.fetch_one(&mut *conn).await?;

    sqlx::query(
        "UPDATE chat_members SET last_read_message_id = $1, mention_count = $2, broadcast_count = $3, \
         read_count = (SELECT CASE WHEN message_count > $4 THEN message_count - $4 ELSE 0 END \
                       FROM chat_conversations WHERE chat_conversations.id = chat_members.conversation_id) \
         WHERE conversation_id = $5 AND user_id = $6",
    )
    .bind(cursor.map(|id| id.to_string()))
    .bind(mentions)
    .bind(broadcasts)
    .bind(unread)
    .bind(conversation.id.to_string())
    .bind(user_id.to_string())
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// The thread form of `set_cursor`.
async fn set_thread_cursor(
    conn: &mut AnyConnection,
    workspace_id: Id,
    root: &MessageRecord,
    user_id: Id,
    cursor: Option<Id>,
    following: bool,
) -> Result<(), ChatError> {
    let after = |column: &str, n: usize| {
        cursor.map_or_else(String::new, |_| format!("AND {column} > ${n}"))
    };
    let sql = format!(
        "SELECT COUNT(*) FROM chat_messages WHERE thread_root_id = $1 AND author_id <> $2 {}",
        after("id", 3)
    );
    let mut query = sqlx::query_scalar::<_, i64>(sqlx::AssertSqlSafe(sql))
        .bind(root.id.to_string())
        .bind(user_id.to_string());
    if let Some(cursor) = cursor {
        query = query.bind(cursor.to_string());
    }
    let unread = query.fetch_one(&mut *conn).await?;

    let sql = format!(
        "SELECT COUNT(*) FROM chat_message_mentions WHERE user_id = $1 AND conversation_id = $2 \
         AND thread_root_id = $3 {}",
        after("message_id", 4)
    );
    let mut query = sqlx::query_scalar::<_, i64>(sqlx::AssertSqlSafe(sql))
        .bind(user_id.to_string())
        .bind(root.conversation_id.to_string())
        .bind(root.id.to_string());
    if let Some(cursor) = cursor {
        query = query.bind(cursor.to_string());
    }
    let mentions = query.fetch_one(&mut *conn).await?;

    upsert_thread_member(
        conn,
        workspace_id,
        root,
        user_id,
        &ThreadMember {
            following,
            last_read_reply_id: cursor,
            read_reply_count: (root.reply_count - unread).max(0),
            mention_count: mentions,
        },
    )
    .await
}
