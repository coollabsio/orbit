//! Each member's read state: cursors, unread and mention counters, notify level, favorites and
//! thread follows.

use orbit_platform::Id;
use serde::Serialize;
use sqlx::{Row, SqliteConnection};
use utoipa::ToSchema;

use super::messages::{mark_read_to, upsert_thread_member};
use super::{
    CONVERSATION_COLUMNS, ChatError, ChatEvent, ChatRepository, Conversation,
    ConversationStateRecord, Events, MEMBER_COLUMNS, MessageRecord, NotifyLevel, ThreadMember,
    ThreadStateRecord, Written, conversation_from_row, current_state, load_access, load_actor,
    load_message, load_message_access, load_thread_member, member_from_row, parse_id,
};

/// Cursors that one `restore_read` call may put back.
const MAX_RESTORE: usize = 1000;

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
    /// The caller's state in every conversation it is a member of. Read from the counters: no
    /// message is counted here.
    pub async fn list_states(
        &self,
        workspace_id: Id,
        actor_id: Id,
    ) -> Result<Vec<ConversationStateRecord>, ChatError> {
        let mut tx = self.database.pool().begin().await?;
        load_actor(&mut tx, workspace_id, actor_id).await?;
        sqlx::query(&format!(
            "SELECT {CONVERSATION_COLUMNS}, {MEMBER_COLUMNS} FROM chat_members m \
             JOIN chat_conversations c ON c.id = m.conversation_id \
             WHERE m.workspace_id = ? AND m.user_id = ? AND c.archived_at IS NULL"
        ))
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
        mark_read_to(
            &mut tx,
            conversation_id,
            actor_id,
            newest.or(member.last_read_message_id),
        )
        .await?;
        let state = emit_state(
            &mut tx,
            workspace_id,
            conversation_id,
            actor_id,
            &mut events,
        )
        .await?;
        tx.commit().await?;
        Ok(events.written(state))
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
                    "SELECT id FROM chat_messages WHERE thread_root_id = ? AND id < ? \
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
                    "SELECT id FROM chat_messages m WHERE m.conversation_id = ? \
                     AND (m.thread_root_id IS NULL OR m.also_in_channel = 1) AND m.id < ? \
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
        tx.commit().await?;
        Ok(events.written(state))
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
        tx.commit().await?;
        Ok(events.written(state))
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
        tx.commit().await?;
        Ok(events.written(state))
    }

    /// Reads every conversation and followed thread that has something unread. Returns their
    /// states as they were, for `restore_read`.
    pub async fn mark_all_read(
        &self,
        workspace_id: Id,
        actor_id: Id,
    ) -> Result<Written<ReadSnapshot>, ChatError> {
        let mut tx = self.database.immediate_transaction().await?;
        load_actor(&mut tx, workspace_id, actor_id).await?;
        let mut events = Events::default();

        let states = sqlx::query(&format!(
            "SELECT {CONVERSATION_COLUMNS}, {MEMBER_COLUMNS} FROM chat_members m \
             JOIN chat_conversations c ON c.id = m.conversation_id \
             WHERE m.workspace_id = ? AND m.user_id = ? AND c.archived_at IS NULL \
             AND c.message_count > m.read_count"
        ))
        .bind(workspace_id.to_string())
        .bind(actor_id.to_string())
        .fetch_all(&mut *tx)
        .await?
        .iter()
        .map(|row| Ok(member_from_row(row)?.state(&conversation_from_row(row)?)))
        .collect::<Result<Vec<_>, ChatError>>()?;
        for state in &states {
            let newest = newest_main_id(&mut tx, state.conversation_id).await?;
            mark_read_to(
                &mut tx,
                state.conversation_id,
                actor_id,
                newest.or(state.last_read_message_id),
            )
            .await?;
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
             WHERE t.workspace_id = ? AND t.user_id = ? AND t.following = 1 \
             AND r.reply_count > t.read_reply_count",
        )
        .bind(workspace_id.to_string())
        .bind(actor_id.to_string())
        .fetch_all(&mut *tx)
        .await?;
        let mut threads = Vec::with_capacity(roots.len());
        for root_id in roots {
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
        tx.commit().await?;
        Ok(events.written(ReadSnapshot { states, threads }))
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
        for cursor in states {
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
        for cursor in threads {
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
        tx.commit().await?;
        Ok(events.written(()))
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
            "UPDATE chat_members SET notify = ?, favorite = ? WHERE conversation_id = ? AND user_id = ?",
        )
        .bind(update.notify.unwrap_or(member.notify).as_str())
        .bind(update.favorite.unwrap_or(member.favorite))
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
        tx.commit().await?;
        Ok(events.written(state))
    }
}

/// A thread root that the caller can read.
async fn load_root(
    conn: &mut SqliteConnection,
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
    conn: &mut SqliteConnection,
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
    "SELECT id FROM chat_messages m WHERE m.conversation_id = ? \
     AND (m.thread_root_id IS NULL OR m.also_in_channel = 1) ORDER BY m.id DESC LIMIT 1"
}

/// The unread messages of a member after a cursor (or all of them), and how many of them
/// have `@channel` or `@here`. Binds: conversation, member, cursor.
pub(super) fn unread_count_sql(bounded: bool) -> String {
    let after = if bounded { "AND m.id > ?" } else { "" };
    format!(
        "SELECT COUNT(*) AS unread, COALESCE(SUM(m.mention_channel OR m.mention_here), 0) AS broadcasts \
         FROM chat_messages m WHERE m.conversation_id = ? \
         AND (m.thread_root_id IS NULL OR m.also_in_channel = 1) AND m.kind = 'message' \
         AND m.deleted_at IS NULL AND m.author_id <> ? {after}"
    )
}

/// The main-list messages after a cursor that mention a member. Binds: member, conversation,
/// cursor.
pub(super) fn mention_count_sql(bounded: bool) -> String {
    let after = if bounded { "AND message_id > ?" } else { "" };
    format!(
        "SELECT COUNT(*) FROM chat_message_mentions WHERE user_id = ? AND conversation_id = ? \
         AND in_main = 1 {after}"
    )
}

/// The caller's state after a change, as an event for the caller.
pub(super) async fn emit_state(
    conn: &mut SqliteConnection,
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
    conn: &mut SqliteConnection,
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

/// Puts a member's cursor at `cursor` and counts what is unread after it. This is the one place
/// that counts messages; its cost is the number of main-list rows after the cursor.
async fn set_cursor(
    conn: &mut SqliteConnection,
    conversation: &Conversation,
    user_id: Id,
    cursor: Option<Id>,
) -> Result<(), ChatError> {
    let sql = unread_count_sql(cursor.is_some());
    let mut query = sqlx::query(&sql)
        .bind(conversation.id.to_string())
        .bind(user_id.to_string());
    if let Some(cursor) = cursor {
        query = query.bind(cursor.to_string());
    }
    let row = query.fetch_one(&mut *conn).await?;
    let unread: i64 = row.get("unread");
    let broadcasts: i64 = row.get("broadcasts");

    let sql = mention_count_sql(cursor.is_some());
    let mut query = sqlx::query_scalar::<_, i64>(&sql)
        .bind(user_id.to_string())
        .bind(conversation.id.to_string());
    if let Some(cursor) = cursor {
        query = query.bind(cursor.to_string());
    }
    let mentions = query.fetch_one(&mut *conn).await?;

    sqlx::query(
        "UPDATE chat_members SET last_read_message_id = ?, mention_count = ?, broadcast_count = ?, \
         read_count = MAX((SELECT message_count FROM chat_conversations \
                           WHERE chat_conversations.id = chat_members.conversation_id) - ?, 0) \
         WHERE conversation_id = ? AND user_id = ?",
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
    conn: &mut SqliteConnection,
    workspace_id: Id,
    root: &MessageRecord,
    user_id: Id,
    cursor: Option<Id>,
    following: bool,
) -> Result<(), ChatError> {
    let after = |column: &str| cursor.map_or_else(String::new, |_| format!("AND {column} > ?"));
    let sql = format!(
        "SELECT COUNT(*) FROM chat_messages WHERE thread_root_id = ? AND author_id <> ? {}",
        after("id")
    );
    let mut query = sqlx::query_scalar::<_, i64>(&sql)
        .bind(root.id.to_string())
        .bind(user_id.to_string());
    if let Some(cursor) = cursor {
        query = query.bind(cursor.to_string());
    }
    let unread = query.fetch_one(&mut *conn).await?;

    let sql = format!(
        "SELECT COUNT(*) FROM chat_message_mentions WHERE user_id = ? AND conversation_id = ? \
         AND thread_root_id = ? {}",
        after("message_id")
    );
    let mut query = sqlx::query_scalar::<_, i64>(&sql)
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
