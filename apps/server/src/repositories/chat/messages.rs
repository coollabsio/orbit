//! Messages, thread replies, reactions and pins, and the lists that read them.

use orbit_platform::{Id, TimestampMillis};
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqliteConnection};

use super::files::{MAX_MESSAGE_FILES, attach_files, copy_files};
use super::stickers::load_sticker;
use super::{
    Access, ChatError, ChatEvent, ChatRepository, ConversationKind, Events, FollowedThreadRecord,
    ForwardedRecord, MESSAGE_MAX_CHARS, MESSAGE_SELECT, MentionsRecord, MessageKind, MessagePage,
    MessageRecord, ReplyToRecord, StickerRecord, ThreadMember, ThreadPage, Written, current_state,
    finish, hydrate, id_list, load_access, load_actor, load_message, load_message_access,
    load_thread_member, mentioned_user_ids, parse_id, parse_optional_id,
};

const DEFAULT_PAGE: usize = 50;
const MAX_PAGE: usize = 100;
/// Lists that the client reads whole (threads, pins, followed threads).
const LIST_LIMIT: i64 = 100;
const FOLLOWED_LIMIT: i64 = 200;
const MAX_REACTIONS: usize = 20;
const NONCE_MAX_BYTES: usize = 64;
const EMOJI_MAX_BYTES: usize = 64;
/// Reply authors kept on the root for the summary row's avatars.
const REPLY_AUTHORS: usize = 5;

/// At most one of `before`, `after` and `around`. Without one: the newest page.
#[derive(Clone, Copy, Debug, Default)]
pub struct MessageCursor {
    /// Messages older than this id.
    pub before: Option<Id>,
    /// Messages newer than this id.
    pub after: Option<Id>,
    /// A window with this message in the middle.
    pub around: Option<Id>,
    pub limit: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct SendInput {
    pub conversation_id: Id,
    pub thread_root_id: Option<Id>,
    /// The message this one quotes: a message of the same conversation.
    pub reply_to_id: Option<Id>,
    /// May be blank when the message has files or a sticker.
    pub body: String,
    /// A sticker of the workspace.
    pub sticker_id: Option<Id>,
    /// Files the sender uploaded for this message.
    pub file_ids: Vec<Id>,
    /// Who is online now: `@here` notifies these members only.
    pub online: Vec<Id>,
    pub also_in_channel: bool,
    pub nonce: String,
}

#[derive(Clone, Debug)]
pub struct ForwardInput {
    /// The message to copy.
    pub message_id: Id,
    /// Where the copy goes: the main list of this conversation.
    pub conversation_id: Id,
    pub nonce: String,
}

/// Which list a page is read from.
#[derive(Clone, Copy)]
enum Scope {
    /// Roots, system rows and "also in channel" replies of a conversation.
    Main(Id),
    /// The replies of one root.
    Thread(Id),
}

impl Scope {
    const fn filter(self) -> &'static str {
        match self {
            Self::Main(_) => {
                "m.conversation_id = ? AND (m.thread_root_id IS NULL OR m.also_in_channel = 1)"
            }
            Self::Thread(_) => "m.thread_root_id = ?",
        }
    }

    fn key(self) -> String {
        match self {
            Self::Main(id) | Self::Thread(id) => id.to_string(),
        }
    }
}

impl ChatRepository {
    pub async fn list_messages(
        &self,
        workspace_id: Id,
        actor_id: Id,
        conversation_id: Id,
        cursor: MessageCursor,
    ) -> Result<MessagePage, ChatError> {
        let mut tx = self.database.pool().begin().await?;
        load_access(&mut tx, workspace_id, actor_id, conversation_id).await?;
        let (items, before, after) = page(&mut tx, Scope::Main(conversation_id), cursor).await?;
        Ok(MessagePage {
            items,
            before,
            after,
        })
    }

    pub async fn get_thread(
        &self,
        workspace_id: Id,
        actor_id: Id,
        root_id: Id,
        cursor: MessageCursor,
    ) -> Result<ThreadPage, ChatError> {
        let mut tx = self.database.pool().begin().await?;
        let (_, root) = load_message_access(&mut tx, workspace_id, actor_id, root_id).await?;
        if root.thread_root_id.is_some() {
            return Err(ChatError::NotFound);
        }
        let (items, before, after) = page(&mut tx, Scope::Thread(root_id), cursor).await?;
        let state = load_thread_member(&mut tx, root_id, actor_id)
            .await?
            .map(|member| member.state(&root));
        Ok(ThreadPage {
            items,
            before,
            after,
            root,
            state,
        })
    }

    /// Roots that have replies, newest reply first.
    pub async fn list_threads(
        &self,
        workspace_id: Id,
        actor_id: Id,
        conversation_id: Id,
    ) -> Result<Vec<MessageRecord>, ChatError> {
        let mut tx = self.database.pool().begin().await?;
        load_access(&mut tx, workspace_id, actor_id, conversation_id).await?;
        let rows = sqlx::query(&threads_sql())
            .bind(conversation_id.to_string())
            .bind(LIST_LIMIT)
            .fetch_all(&mut *tx)
            .await?;
        hydrate(&mut tx, &rows).await
    }

    /// Pinned messages, newest message first.
    pub async fn list_pins(
        &self,
        workspace_id: Id,
        actor_id: Id,
        conversation_id: Id,
    ) -> Result<Vec<MessageRecord>, ChatError> {
        let mut tx = self.database.pool().begin().await?;
        load_access(&mut tx, workspace_id, actor_id, conversation_id).await?;
        let rows = sqlx::query(&pins_sql())
            .bind(conversation_id.to_string())
            .bind(LIST_LIMIT)
            .fetch_all(&mut *tx)
            .await?;
        hydrate(&mut tx, &rows).await
    }

    /// The threads the caller follows in conversations it can still read: unread first, then
    /// newest reply first.
    pub async fn list_followed_threads(
        &self,
        workspace_id: Id,
        actor_id: Id,
    ) -> Result<Vec<FollowedThreadRecord>, ChatError> {
        let mut tx = self.database.pool().begin().await?;
        load_actor(&mut tx, workspace_id, actor_id).await?;
        let follows = sqlx::query(
            "SELECT t.root_id, t.last_read_reply_id, t.read_reply_count, t.mention_count, \
             r.last_reply_id \
             FROM chat_thread_members t \
             JOIN chat_messages r ON r.id = t.root_id \
             JOIN chat_conversations c ON c.id = t.conversation_id \
             WHERE t.workspace_id = ? AND t.user_id = ? AND t.following = 1 \
             AND (c.kind = 'public' OR EXISTS (SELECT 1 FROM chat_members \
                  WHERE chat_members.conversation_id = c.id AND chat_members.user_id = t.user_id)) \
             ORDER BY (r.reply_count > t.read_reply_count) DESC, r.last_reply_id DESC, r.id DESC \
             LIMIT ?",
        )
        .bind(workspace_id.to_string())
        .bind(actor_id.to_string())
        .bind(FOLLOWED_LIMIT)
        .fetch_all(&mut *tx)
        .await?;
        if follows.is_empty() {
            return Ok(Vec::new());
        }
        let mut ids = Vec::with_capacity(follows.len() * 2);
        for row in &follows {
            ids.push(parse_id(row.get("root_id"))?);
            ids.extend(parse_optional_id(row.get("last_reply_id"))?);
        }
        let rows = sqlx::query(&format!(
            "{MESSAGE_SELECT} WHERE m.id IN (SELECT value FROM json_each(?))"
        ))
        .bind(id_list(ids))
        .fetch_all(&mut *tx)
        .await?;
        let messages = hydrate(&mut tx, &rows).await?;
        let find = |id: Id| messages.iter().find(|message| message.id == id).cloned();
        let mut threads = Vec::with_capacity(follows.len());
        for row in &follows {
            let Some(root) = find(parse_id(row.get("root_id"))?) else {
                continue;
            };
            let member = ThreadMember {
                following: true,
                last_read_reply_id: parse_optional_id(row.get("last_read_reply_id"))?,
                read_reply_count: row.get("read_reply_count"),
                mention_count: row.get("mention_count"),
            };
            threads.push(FollowedThreadRecord {
                conversation_id: root.conversation_id,
                state: member.state(&root),
                last_reply: root.last_reply_id.and_then(find),
                root,
            });
        }
        Ok(threads)
    }

    /// A send that is tried again with the same nonce returns the first message.
    pub async fn send_message(
        &self,
        workspace_id: Id,
        actor_id: Id,
        input: SendInput,
    ) -> Result<Written<MessageRecord>, ChatError> {
        let body = clean_body(
            &input.body,
            !input.file_ids.is_empty() || input.sticker_id.is_some(),
        )?;
        if input.file_ids.len() > MAX_MESSAGE_FILES {
            return Err(ChatError::Invalid { field: "file_ids" });
        }
        if input.nonce.is_empty() || input.nonce.len() > NONCE_MAX_BYTES {
            return Err(ChatError::Invalid { field: "nonce" });
        }
        let now = TimestampMillis::now();
        let mut tx = self.database.immediate_transaction().await?;
        let access = load_access(&mut tx, workspace_id, actor_id, input.conversation_id).await?;
        access.require_member()?;
        access.require_open()?;
        let conversation_id = access.conversation.id;
        let mut events = Events::default();

        if let Some(message) = sent_before(&mut tx, conversation_id, actor_id, &input.nonce).await?
        {
            events.user(
                actor_id,
                ChatEvent::MessageCreated {
                    message: message.clone(),
                },
            );
            return Ok(events.written(message));
        }

        let root = match input.thread_root_id {
            Some(root_id) => Some(
                load_message(&mut tx, root_id)
                    .await?
                    .filter(|root| {
                        root.conversation_id == conversation_id
                            && root.thread_root_id.is_none()
                            && root.kind == MessageKind::Message
                    })
                    .ok_or(ChatError::NotFound)?,
            ),
            None => None,
        };
        // A quote notifies nobody and counts nothing: it is only shown above the message.
        let reply_to = match input.reply_to_id {
            Some(target_id) => Some(
                load_message(&mut tx, target_id)
                    .await?
                    .filter(|target| {
                        target.conversation_id == conversation_id
                            && target.kind == MessageKind::Message
                    })
                    .as_ref()
                    .and_then(ReplyToRecord::of)
                    .ok_or(ChatError::NotFound)?,
            ),
            None => None,
        };
        let sticker = match input.sticker_id {
            Some(sticker_id) => Some(load_sticker(&mut tx, workspace_id, sticker_id).await?),
            None => None,
        };
        // `@channel` and `@here` do nothing in a thread.
        let broadcast = |token: &str| root.is_none() && body.contains(token);
        let mut message = insert_message(
            &mut tx,
            NewMessage {
                conversation_id,
                thread_root_id: root.as_ref().map(|root| root.id),
                kind: MessageKind::Message,
                author_id: actor_id,
                body: &body,
                mention_channel: broadcast("<!channel>"),
                mention_here: broadcast("<!here>"),
                also_in_channel: root.is_some() && input.also_in_channel,
                nonce: Some(&input.nonce),
                reply_to,
                forwarded: None,
                sticker_id: sticker.as_ref().map(|sticker| sticker.id),
                sticker,
                now,
            },
        )
        .await?;
        attach_files(&mut tx, &mut message, &input.file_ids).await?;
        let mentioned = members_among(
            &mut tx,
            conversation_id,
            &message.mentions.user_ids,
            actor_id,
        )
        .await?;
        insert_mention_rows(&mut tx, &message, &mentioned).await?;
        adjust_mentions(&mut tx, &message, &mentioned, 1).await?;
        events.conversation(
            conversation_id,
            ChatEvent::MessageCreated {
                message: message.clone(),
            },
        );

        if message.in_main() {
            events.counted = Some(conversation_id);
            sqlx::query(
                "UPDATE chat_conversations SET message_count = message_count + 1, last_message_at = ? \
                 WHERE id = ?",
            )
            .bind(now.as_millis())
            .bind(conversation_id.to_string())
            .execute(&mut *tx)
            .await?;
            if message.mentions.channel || message.mentions.here {
                adjust_broadcast(&mut tx, &message, 1).await?;
            }
            events.inbox =
                notify_mentions(&mut tx, &access, &message, &mentioned, &input.online).await?;
            // Sending a message reads the conversation up to it.
            if mark_read_to(&mut tx, conversation_id, actor_id, Some(message.id)).await? {
                events.inbox.push(actor_id);
            }
        }
        if let Some(root) = &root {
            events.replied = Some(root.id);
            after_reply(&mut tx, &access, root, &message, &mentioned).await?;
            let root = load_message(&mut tx, root.id)
                .await?
                .ok_or(ChatError::NotFound)?;
            // The other followers get their states from the live socket (`replied`).
            if let Some(member) = load_thread_member(&mut tx, root.id, actor_id).await? {
                events.user(
                    actor_id,
                    ChatEvent::ThreadChanged {
                        state: member.state(&root),
                    },
                );
            }
            events.conversation(conversation_id, ChatEvent::MessageUpdated { message: root });
        }
        if message.in_main()
            && let Some(state) =
                current_state(&mut tx, workspace_id, conversation_id, actor_id).await?
        {
            events.user(actor_id, ChatEvent::StateChanged { state });
        }
        finish(tx, events, message).await
    }

    /// Copies a message the caller can read into the main list of a conversation the caller can
    /// write to. The copy is the caller's message: it has the body and the files of the original
    /// as they are now, and says where they came from. It mentions nobody. Tried again with the
    /// same nonce, it returns the first copy.
    pub async fn forward_message(
        &self,
        workspace_id: Id,
        actor_id: Id,
        input: ForwardInput,
    ) -> Result<Written<MessageRecord>, ChatError> {
        if input.nonce.is_empty() || input.nonce.len() > NONCE_MAX_BYTES {
            return Err(ChatError::Invalid { field: "nonce" });
        }
        let now = TimestampMillis::now();
        let mut tx = self.database.immediate_transaction().await?;
        let access = load_access(&mut tx, workspace_id, actor_id, input.conversation_id).await?;
        access.require_member()?;
        access.require_open()?;
        let conversation_id = access.conversation.id;
        let mut events = Events::default();
        // The destination and the nonce first: a retry finds its copy after the original is gone.
        let earlier = sent_before(&mut tx, conversation_id, actor_id, &input.nonce).await?;
        let source =
            match load_message_access(&mut tx, workspace_id, actor_id, input.message_id).await {
                Ok((_, source)) if !source.deleted && source.kind == MessageKind::Message => {
                    Some(source)
                }
                Ok(_) | Err(ChatError::NotFound) => None,
                Err(error) => return Err(error),
            };
        // A forward of a forward names the first original, not the copy.
        let origin = source.as_ref().map(|source| {
            source.forwarded.unwrap_or(ForwardedRecord {
                message_id: source.id,
                conversation_id: source.conversation_id,
                author_id: source.author_id,
                created_at: source.created_at,
            })
        });
        if let Some(message) = earlier {
            // The nonce is this forward's only when the earlier message is a forward of the same
            // original; an original that is gone cannot be compared.
            let same = match (&message.forwarded, &origin) {
                (Some(forwarded), Some(origin)) => forwarded.message_id == origin.message_id,
                (Some(_), None) => true,
                (None, _) => false,
            };
            if !same {
                return Err(ChatError::Invalid { field: "nonce" });
            }
            events.user(
                actor_id,
                ChatEvent::MessageCreated {
                    message: message.clone(),
                },
            );
            return Ok(events.written(message));
        }
        let (Some(source), Some(origin)) = (source, origin) else {
            return Err(ChatError::NotFound);
        };

        let mut message = insert_message(
            &mut tx,
            NewMessage {
                conversation_id,
                thread_root_id: None,
                kind: MessageKind::Message,
                author_id: actor_id,
                body: &source.body,
                mention_channel: false,
                mention_here: false,
                also_in_channel: false,
                nonce: Some(&input.nonce),
                reply_to: None,
                forwarded: Some(origin),
                // The id of a deleted sticker is copied too.
                sticker_id: source.sticker_id,
                sticker: source.sticker.clone(),
                now,
            },
        )
        .await?;
        copy_files(&mut tx, &source, &mut message).await?;
        events.conversation(
            conversation_id,
            ChatEvent::MessageCreated {
                message: message.clone(),
            },
        );
        events.counted = Some(conversation_id);
        sqlx::query(
            "UPDATE chat_conversations SET message_count = message_count + 1, last_message_at = ? \
             WHERE id = ?",
        )
        .bind(now.as_millis())
        .bind(conversation_id.to_string())
        .execute(&mut *tx)
        .await?;
        // Sending a message reads the conversation up to it.
        if mark_read_to(&mut tx, conversation_id, actor_id, Some(message.id)).await? {
            events.inbox.push(actor_id);
        }
        if let Some(state) = current_state(&mut tx, workspace_id, conversation_id, actor_id).await?
        {
            events.user(actor_id, ChatEvent::StateChanged { state });
        }
        finish(tx, events, message).await
    }

    /// Only the author edits a message. A forward is a copy and is never edited. The new body is
    /// not blank, whatever the message has besides it; its files and its sticker stay.
    pub async fn edit_message(
        &self,
        workspace_id: Id,
        actor_id: Id,
        message_id: Id,
        body: &str,
    ) -> Result<Written<MessageRecord>, ChatError> {
        let body = clean_body(body, false)?;
        let now = TimestampMillis::now();
        let mut tx = self.database.immediate_transaction().await?;
        let (access, message) =
            load_message_access(&mut tx, workspace_id, actor_id, message_id).await?;
        if message.deleted || message.kind != MessageKind::Message {
            return Err(ChatError::NotFound);
        }
        if !access.actor.can_edit_chat_message(message.author_id) {
            return Err(ChatError::Forbidden("Only the author can edit a message."));
        }
        if message.forwarded.is_some() {
            return Err(ChatError::Forbidden(
                "A forwarded message cannot be edited.",
            ));
        }
        access.require_member()?;
        access.require_open()?;
        let conversation_id = message.conversation_id;

        // Members who gain or lose a mention get their unread mention counters corrected.
        let before = mention_row_users(&mut tx, message.id).await?;
        let after = members_among(
            &mut tx,
            conversation_id,
            &mentioned_user_ids(&body),
            message.author_id,
        )
        .await?;
        let removed: Vec<Id> = before
            .iter()
            .copied()
            .filter(|id| !after.contains(id))
            .collect();
        let added: Vec<Id> = after
            .iter()
            .copied()
            .filter(|id| !before.contains(id))
            .collect();
        adjust_mentions(&mut tx, &message, &removed, -1).await?;
        let removed_any = !removed.is_empty();
        sqlx::query(
            "DELETE FROM chat_message_mentions WHERE message_id = ? \
             AND user_id IN (SELECT value FROM json_each(?))",
        )
        .bind(message.id.to_string())
        .bind(id_list(removed))
        .execute(&mut *tx)
        .await?;
        insert_mention_rows(&mut tx, &message, &added).await?;
        adjust_mentions(&mut tx, &message, &added, 1).await?;

        let is_root = message.thread_root_id.is_none();
        let channel = is_root && body.contains("<!channel>");
        let here = is_root && body.contains("<!here>");
        let was_broadcast = message.mentions.channel || message.mentions.here;
        if was_broadcast != (channel || here) {
            adjust_broadcast(&mut tx, &message, if was_broadcast { -1 } else { 1 }).await?;
        }
        let mut events = Events::default();
        if removed_any || !added.is_empty() || was_broadcast != (channel || here) {
            events.counted = message.in_main().then_some(conversation_id);
            events.replied = message.thread_root_id;
        }
        sqlx::query(
            "UPDATE chat_messages SET body = ?, mention_channel = ?, mention_here = ?, edited_at = ? \
             WHERE id = ?",
        )
        .bind(&body)
        .bind(channel)
        .bind(here)
        .bind(now.as_millis())
        .bind(message.id.to_string())
        .execute(&mut *tx)
        .await?;

        let next = load_message(&mut tx, message.id)
            .await?
            .ok_or(ChatError::NotFound)?;
        events.conversation(
            conversation_id,
            ChatEvent::MessageUpdated {
                message: next.clone(),
            },
        );
        // The root shows a preview of its newest reply.
        if let Some(root_id) = message.thread_root_id
            && let Some(root) = load_message(&mut tx, root_id).await?
            && root.last_reply_id == Some(message.id)
        {
            events.conversation(conversation_id, ChatEvent::MessageUpdated { message: root });
        }
        finish(tx, events, next).await
    }

    /// The author and chat managers delete a message. A root that has replies stays in the list
    /// with an empty body; every other message is removed.
    pub async fn delete_message(
        &self,
        workspace_id: Id,
        actor_id: Id,
        message_id: Id,
    ) -> Result<Written<()>, ChatError> {
        let now = TimestampMillis::now();
        let mut tx = self.database.immediate_transaction().await?;
        let (access, message) =
            load_message_access(&mut tx, workspace_id, actor_id, message_id).await?;
        if message.deleted || message.kind != MessageKind::Message {
            return Err(ChatError::NotFound);
        }
        if !access.actor.can_delete_chat_message(message.author_id) {
            return Err(ChatError::Forbidden(
                "Only the author and workspace owners and admins can delete a message.",
            ));
        }
        let conversation_id = message.conversation_id;
        let mut events = Events::default();
        let mentioned = mention_row_users(&mut tx, message.id).await?;
        adjust_mentions(&mut tx, &message, &mentioned, -1).await?;
        if message.in_main() {
            events.counted = Some(conversation_id);
            uncount_main(&mut tx, &message).await?;
        }
        events.replied = message.thread_root_id;

        if message.thread_root_id.is_none() && message.reply_count > 0 {
            sqlx::query("DELETE FROM notifications WHERE chat_message_id = ?")
                .bind(message.id.to_string())
                .execute(&mut *tx)
                .await?;
            for table in [
                "chat_message_mentions",
                "chat_reactions",
                "chat_message_files",
            ] {
                sqlx::query(&format!("DELETE FROM {table} WHERE message_id = ?"))
                    .bind(message.id.to_string())
                    .execute(&mut *tx)
                    .await?;
            }
            sqlx::query(
                "UPDATE chat_messages SET body = '', deleted_at = ?, pinned_at = NULL, pinned_by = NULL, \
                 mention_channel = 0, mention_here = 0, sticker_id = NULL WHERE id = ?",
            )
            .bind(now.as_millis())
            .bind(message.id.to_string())
            .execute(&mut *tx)
            .await?;
            let next = load_message(&mut tx, message.id)
                .await?
                .ok_or(ChatError::NotFound)?;
            events.conversation(conversation_id, ChatEvent::MessageUpdated { message: next });
        } else {
            if message.thread_root_id.is_some() {
                sqlx::query(
                    "UPDATE chat_thread_members SET read_reply_count = MAX(read_reply_count - 1, 0) \
                     WHERE root_id = ? AND (user_id = ? OR last_read_reply_id >= ?)",
                )
                .bind(message.thread_root_id.map(|id| id.to_string()))
                .bind(message.author_id.to_string())
                .bind(message.id.to_string())
                .execute(&mut *tx)
                .await?;
            }
            delete_row(&mut tx, &message, &mut events).await?;
            if let Some(root_id) = message.thread_root_id {
                after_reply_removed(&mut tx, root_id, actor_id, &mut events).await?;
            }
        }
        if let Some(state) = current_state(&mut tx, workspace_id, conversation_id, actor_id).await?
        {
            events.user(actor_id, ChatEvent::StateChanged { state });
        }
        finish(tx, events, ()).await
    }

    /// Adds (`on`) or removes the caller's reaction. Both are idempotent.
    pub async fn set_reaction(
        &self,
        workspace_id: Id,
        actor_id: Id,
        message_id: Id,
        emoji: &str,
        on: bool,
    ) -> Result<Written<MessageRecord>, ChatError> {
        if emoji.is_empty()
            || emoji.len() > EMOJI_MAX_BYTES
            || emoji
                .chars()
                .any(|char| char.is_whitespace() || char.is_control())
        {
            return Err(ChatError::Invalid { field: "emoji" });
        }
        let now = TimestampMillis::now();
        let mut tx = self.database.immediate_transaction().await?;
        let (access, message) =
            load_message_access(&mut tx, workspace_id, actor_id, message_id).await?;
        access.require_member()?;
        access.require_open()?;
        if message.deleted || message.kind != MessageKind::Message {
            return Err(ChatError::NotFound);
        }
        if on {
            let known = message
                .reactions
                .iter()
                .any(|reaction| reaction.emoji == emoji);
            if !known && message.reactions.len() >= MAX_REACTIONS {
                return Err(ChatError::Conflict(
                    "This message has the maximum number of different reactions.",
                ));
            }
            sqlx::query(
                "INSERT INTO chat_reactions (message_id, emoji, user_id, created_at) VALUES (?, ?, ?, ?) \
                 ON CONFLICT DO NOTHING",
            )
            .bind(message.id.to_string())
            .bind(emoji)
            .bind(actor_id.to_string())
            .bind(now.as_millis())
            .execute(&mut *tx)
            .await?;
        } else {
            sqlx::query(
                "DELETE FROM chat_reactions WHERE message_id = ? AND emoji = ? AND user_id = ?",
            )
            .bind(message.id.to_string())
            .bind(emoji)
            .bind(actor_id.to_string())
            .execute(&mut *tx)
            .await?;
        }
        let next = load_message(&mut tx, message.id)
            .await?
            .ok_or(ChatError::NotFound)?;
        let mut events = Events::default();
        events.conversation(
            message.conversation_id,
            ChatEvent::MessageUpdated {
                message: next.clone(),
            },
        );
        finish(tx, events, next).await
    }

    /// Pinning adds a system row to the conversation; unpinning does not.
    pub async fn set_pinned(
        &self,
        workspace_id: Id,
        actor_id: Id,
        message_id: Id,
        pinned: bool,
    ) -> Result<Written<MessageRecord>, ChatError> {
        let now = TimestampMillis::now();
        let mut tx = self.database.immediate_transaction().await?;
        let (access, message) =
            load_message_access(&mut tx, workspace_id, actor_id, message_id).await?;
        access.require_member()?;
        if message.deleted || message.kind != MessageKind::Message {
            return Err(ChatError::NotFound);
        }
        let mut events = Events::default();
        if message.pinned == pinned {
            return Ok(events.written(message));
        }
        access.require_open()?;
        sqlx::query("UPDATE chat_messages SET pinned_at = ?, pinned_by = ? WHERE id = ?")
            .bind(pinned.then_some(now.as_millis()))
            .bind(pinned.then(|| actor_id.to_string()))
            .bind(message.id.to_string())
            .execute(&mut *tx)
            .await?;
        let next = load_message(&mut tx, message.id)
            .await?
            .ok_or(ChatError::NotFound)?;
        let conversation_id = message.conversation_id;
        events.conversation(
            conversation_id,
            ChatEvent::MessageUpdated {
                message: next.clone(),
            },
        );
        if pinned {
            let row =
                add_system_row(&mut tx, conversation_id, MessageKind::Pin, actor_id, now).await?;
            events.conversation(conversation_id, ChatEvent::MessageCreated { message: row });
            if let Some(state) =
                current_state(&mut tx, workspace_id, conversation_id, actor_id).await?
            {
                events.user(actor_id, ChatEvent::StateChanged { state });
            }
        }
        finish(tx, events, next).await
    }
}

/// The stored body: at most 4000 characters, not blank (unless the message has files or a
/// sticker), and without the two control characters that the search index uses as highlight
/// markers.
fn clean_body(body: &str, may_be_blank: bool) -> Result<String, ChatError> {
    if body.chars().count() > MESSAGE_MAX_CHARS {
        return Err(ChatError::TooLong);
    }
    if body.trim().is_empty() && !may_be_blank {
        return Err(ChatError::Invalid { field: "body" });
    }
    Ok(body.replace(['\u{2}', '\u{3}'], ""))
}

fn threads_sql() -> String {
    format!(
        "{MESSAGE_SELECT} WHERE m.conversation_id = ? AND m.reply_count > 0 \
         ORDER BY m.last_reply_id DESC LIMIT ?"
    )
}

fn pins_sql() -> String {
    format!(
        "{MESSAGE_SELECT} WHERE m.conversation_id = ? AND m.pinned_at IS NOT NULL \
         ORDER BY m.id DESC LIMIT ?"
    )
}

fn older_sql(scope: Scope, bounded: bool) -> String {
    let bound = if bounded { "AND m.id < ?" } else { "" };
    format!(
        "{MESSAGE_SELECT} WHERE {} {bound} ORDER BY m.id DESC LIMIT ?",
        scope.filter()
    )
}

fn newer_sql(scope: Scope, inclusive: bool) -> String {
    let compare = if inclusive { ">=" } else { ">" };
    format!(
        "{MESSAGE_SELECT} WHERE {} AND m.id {compare} ? ORDER BY m.id LIMIT ?",
        scope.filter()
    )
}

/// One page in ascending order, with the cursors of the pages next to it.
async fn page(
    conn: &mut SqliteConnection,
    scope: Scope,
    cursor: MessageCursor,
) -> Result<(Vec<MessageRecord>, Option<Id>, Option<Id>), ChatError> {
    let limit = cursor.limit.unwrap_or(DEFAULT_PAGE).clamp(1, MAX_PAGE);
    let (rows, has_older, has_newer) = if let Some(around) = cursor.around {
        // Half a page before the message, the rest from it on; a short first half gives its
        // room to the second.
        let (mut rows, has_older) = older(conn, scope, Some(around), limit / 2).await?;
        let (newer, has_newer) = newer(conn, scope, around, true, limit - rows.len()).await?;
        rows.extend(newer);
        (rows, has_older, has_newer)
    } else if let Some(before) = cursor.before {
        let (rows, has_older) = older(conn, scope, Some(before), limit).await?;
        let has_newer = !rows.is_empty();
        (rows, has_older, has_newer)
    } else if let Some(after) = cursor.after {
        let (rows, has_newer) = newer(conn, scope, after, false, limit).await?;
        let has_older = !rows.is_empty();
        (rows, has_older, has_newer)
    } else {
        let (rows, has_older) = older(conn, scope, None, limit).await?;
        (rows, has_older, false)
    };
    let items = hydrate(conn, &rows).await?;
    let before = items
        .first()
        .filter(|_| has_older)
        .map(|message| message.id);
    let after = items.last().filter(|_| has_newer).map(|message| message.id);
    Ok((items, before, after))
}

/// Up to `take` rows below `bound` in ascending order, and whether older rows exist.
async fn older(
    conn: &mut SqliteConnection,
    scope: Scope,
    bound: Option<Id>,
    take: usize,
) -> Result<(Vec<SqliteRow>, bool), ChatError> {
    let sql = older_sql(scope, bound.is_some());
    let mut query = sqlx::query(&sql).bind(scope.key());
    if let Some(bound) = bound {
        query = query.bind(bound.to_string());
    }
    let mut rows = query.bind(take as i64 + 1).fetch_all(&mut *conn).await?;
    let more = rows.len() > take;
    rows.truncate(take);
    rows.reverse();
    Ok((rows, more))
}

/// Up to `take` rows above `bound` (or from it on) in ascending order, and whether newer rows exist.
async fn newer(
    conn: &mut SqliteConnection,
    scope: Scope,
    bound: Id,
    inclusive: bool,
    take: usize,
) -> Result<(Vec<SqliteRow>, bool), ChatError> {
    let sql = newer_sql(scope, inclusive);
    let mut rows = sqlx::query(&sql)
        .bind(scope.key())
        .bind(bound.to_string())
        .bind(take as i64 + 1)
        .fetch_all(&mut *conn)
        .await?;
    let more = rows.len() > take;
    rows.truncate(take);
    Ok((rows, more))
}

/// The message that `author_id` sent to the conversation with this nonce, if there is one.
async fn sent_before(
    conn: &mut SqliteConnection,
    conversation_id: Id,
    author_id: Id,
    nonce: &str,
) -> Result<Option<MessageRecord>, ChatError> {
    let existing: Option<String> = sqlx::query_scalar(
        "SELECT id FROM chat_messages WHERE conversation_id = ? AND author_id = ? AND nonce = ?",
    )
    .bind(conversation_id.to_string())
    .bind(author_id.to_string())
    .bind(nonce)
    .fetch_optional(&mut *conn)
    .await?;
    match existing {
        Some(id) => Ok(Some(
            load_message(conn, parse_id(id)?)
                .await?
                .ok_or(ChatError::NotFound)?,
        )),
        None => Ok(None),
    }
}

struct NewMessage<'a> {
    conversation_id: Id,
    thread_root_id: Option<Id>,
    kind: MessageKind,
    author_id: Id,
    body: &'a str,
    mention_channel: bool,
    mention_here: bool,
    also_in_channel: bool,
    nonce: Option<&'a str>,
    reply_to: Option<ReplyToRecord>,
    /// Set on a forward. Its body mentions nobody.
    forwarded: Option<ForwardedRecord>,
    /// The id alone on a forward of a message whose sticker is deleted.
    sticker_id: Option<Id>,
    sticker: Option<StickerRecord>,
    now: TimestampMillis,
}

/// The id is made inside the write transaction, so id order is commit order and a page read
/// with `after` cannot miss a message.
async fn insert_message(
    conn: &mut SqliteConnection,
    new: NewMessage<'_>,
) -> Result<MessageRecord, ChatError> {
    let id = Id::new_v7();
    sqlx::query(
        "INSERT INTO chat_messages (id, conversation_id, thread_root_id, kind, author_id, body, \
         mention_channel, mention_here, also_in_channel, nonce, reply_to_id, forward_of_id, \
         forward_conversation_id, forward_author_id, forward_created_at, sticker_id, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(id.to_string())
    .bind(new.conversation_id.to_string())
    .bind(new.thread_root_id.map(|id| id.to_string()))
    .bind(new.kind.as_str())
    .bind(new.author_id.to_string())
    .bind(new.body)
    .bind(new.mention_channel)
    .bind(new.mention_here)
    .bind(new.also_in_channel)
    .bind(new.nonce)
    .bind(new.reply_to.as_ref().map(|target| target.id.to_string()))
    .bind(new.forwarded.map(|origin| origin.message_id.to_string()))
    .bind(
        new.forwarded
            .map(|origin| origin.conversation_id.to_string()),
    )
    .bind(new.forwarded.map(|origin| origin.author_id.to_string()))
    .bind(new.forwarded.map(|origin| origin.created_at.as_millis()))
    .bind(new.sticker_id.map(|id| id.to_string()))
    .bind(new.now.as_millis())
    .execute(&mut *conn)
    .await?;
    Ok(MessageRecord {
        id,
        conversation_id: new.conversation_id,
        thread_root_id: new.thread_root_id,
        kind: new.kind,
        author_id: new.author_id,
        body: new.body.to_owned(),
        mentions: MentionsRecord {
            user_ids: if new.forwarded.is_some() {
                Vec::new()
            } else {
                mentioned_user_ids(new.body)
            },
            channel: new.mention_channel,
            here: new.mention_here,
        },
        created_at: new.now,
        edited_at: None,
        deleted: false,
        attachments: Vec::new(),
        reactions: Vec::new(),
        pinned: false,
        also_in_channel: new.also_in_channel,
        nonce: new.nonce.map(str::to_owned),
        reply_count: 0,
        last_reply_at: None,
        reply_user_ids: Vec::new(),
        last_reply: None,
        reply_to_id: new.reply_to.as_ref().map(|target| target.id),
        reply_to: new.reply_to,
        forwarded: new.forwarded,
        sticker_id: new.sticker_id,
        sticker: new.sticker,
        last_reply_id: None,
    })
}

/// A `pin`, `join` or `leave` row about `author_id`. System rows are never unread, and the row
/// reads the conversation for the member it is about.
pub(super) async fn add_system_row(
    conn: &mut SqliteConnection,
    conversation_id: Id,
    kind: MessageKind,
    author_id: Id,
    now: TimestampMillis,
) -> Result<MessageRecord, ChatError> {
    let message = insert_message(
        conn,
        NewMessage {
            conversation_id,
            thread_root_id: None,
            kind,
            author_id,
            body: "",
            mention_channel: false,
            mention_here: false,
            also_in_channel: false,
            nonce: None,
            reply_to: None,
            forwarded: None,
            sticker_id: None,
            sticker: None,
            now,
        },
    )
    .await?;
    sqlx::query("UPDATE chat_conversations SET last_message_at = ? WHERE id = ?")
        .bind(now.as_millis())
        .bind(conversation_id.to_string())
        .execute(&mut *conn)
        .await?;
    mark_read_to(conn, conversation_id, author_id, Some(message.id)).await?;
    Ok(message)
}

/// Moves a member's cursor to the newest message: nothing is unread after it.
/// Returns whether that also marked inbox notifications of chat mentions as read.
pub(super) async fn mark_read_to(
    conn: &mut SqliteConnection,
    conversation_id: Id,
    user_id: Id,
    message_id: Option<Id>,
) -> Result<bool, ChatError> {
    sqlx::query(
        "UPDATE chat_members SET last_read_message_id = ?, mention_count = 0, broadcast_count = 0, \
         read_count = (SELECT message_count FROM chat_conversations \
                       WHERE chat_conversations.id = chat_members.conversation_id) \
         WHERE conversation_id = ? AND user_id = ?",
    )
    .bind(message_id.map(|id| id.to_string()))
    .bind(conversation_id.to_string())
    .bind(user_id.to_string())
    .execute(&mut *conn)
    .await?;
    let marked = sqlx::query(
        "UPDATE notifications SET read_at = ? \
         WHERE chat_conversation_id = ? AND recipient_user_id = ? AND read_at IS NULL",
    )
    .bind(TimestampMillis::now().as_millis())
    .bind(conversation_id.to_string())
    .bind(user_id.to_string())
    .execute(&mut *conn)
    .await?
    .rows_affected();
    Ok(marked > 0)
}

/// Inbox notifications for a message in the main list of a channel: for the members it
/// mentions, and with `@channel` for every member who has not muted the channel (`@here`: of
/// those, the members who are online). Returns who was notified.
async fn notify_mentions(
    conn: &mut SqliteConnection,
    access: &Access,
    message: &MessageRecord,
    mentioned: &[Id],
    online: &[Id],
) -> Result<Vec<Id>, ChatError> {
    let conversation = &access.conversation;
    if conversation.kind == ConversationKind::Dm || message.thread_root_id.is_some() {
        return Ok(Vec::new());
    }
    let mut recipients = mentioned.to_vec();
    if message.mentions.channel || message.mentions.here {
        let listening = sqlx::query_scalar::<_, String>(
            "SELECT user_id FROM chat_members WHERE conversation_id = ? AND user_id <> ? \
             AND notify <> 'muted'",
        )
        .bind(conversation.id.to_string())
        .bind(message.author_id.to_string())
        .fetch_all(&mut *conn)
        .await?;
        for user_id in listening {
            let user_id = parse_id(user_id)?;
            if message.mentions.channel || online.contains(&user_id) {
                recipients.push(user_id);
            }
        }
    }
    recipients.sort_unstable();
    recipients.dedup();
    for recipient in &recipients {
        sqlx::query(
            "INSERT OR IGNORE INTO notifications (id, workspace_id, recipient_user_id, actor_user_id, \
             kind, chat_conversation_id, chat_message_id, dedupe_key, created_at) \
             VALUES (?, ?, ?, ?, 'chat_mentioned', ?, ?, ?, ?)",
        )
        .bind(Id::new_v7().to_string())
        .bind(conversation.workspace_id.to_string())
        .bind(recipient.to_string())
        .bind(message.author_id.to_string())
        .bind(conversation.id.to_string())
        .bind(message.id.to_string())
        .bind(format!(
            "{}:{recipient}:chat_mentioned:{}",
            conversation.workspace_id, message.id
        ))
        .bind(message.created_at.as_millis())
        .execute(&mut *conn)
        .await?;
    }
    Ok(recipients)
}

/// The members of the conversation among `user_ids`, without `except` (nobody mentions themselves).
async fn members_among(
    conn: &mut SqliteConnection,
    conversation_id: Id,
    user_ids: &[Id],
    except: Id,
) -> Result<Vec<Id>, ChatError> {
    if user_ids.is_empty() {
        return Ok(Vec::new());
    }
    sqlx::query_scalar::<_, String>(
        "SELECT user_id FROM chat_members WHERE conversation_id = ? AND user_id <> ? \
         AND user_id IN (SELECT value FROM json_each(?))",
    )
    .bind(conversation_id.to_string())
    .bind(except.to_string())
    .bind(id_list(user_ids.iter().copied()))
    .fetch_all(&mut *conn)
    .await?
    .into_iter()
    .map(parse_id)
    .collect()
}

async fn mention_row_users(
    conn: &mut SqliteConnection,
    message_id: Id,
) -> Result<Vec<Id>, ChatError> {
    sqlx::query_scalar::<_, String>(
        "SELECT user_id FROM chat_message_mentions WHERE message_id = ?",
    )
    .bind(message_id.to_string())
    .fetch_all(&mut *conn)
    .await?
    .into_iter()
    .map(parse_id)
    .collect()
}

async fn insert_mention_rows(
    conn: &mut SqliteConnection,
    message: &MessageRecord,
    user_ids: &[Id],
) -> Result<(), ChatError> {
    if user_ids.is_empty() {
        return Ok(());
    }
    sqlx::query(
        "INSERT INTO chat_message_mentions (message_id, user_id, conversation_id, thread_root_id, in_main) \
         SELECT ?, value, ?, ?, ? FROM json_each(?)",
    )
    .bind(message.id.to_string())
    .bind(message.conversation_id.to_string())
    .bind(message.thread_root_id.map(|id| id.to_string()))
    .bind(message.in_main())
    .bind(id_list(user_ids.iter().copied()))
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Adds `delta` to the unread mention counters of `user_ids`: in the conversation when the
/// message is in the main list, and in the thread for the members who follow it. Only for
/// members whose cursor is before the message.
async fn adjust_mentions(
    conn: &mut SqliteConnection,
    message: &MessageRecord,
    user_ids: &[Id],
    delta: i64,
) -> Result<(), ChatError> {
    if user_ids.is_empty() {
        return Ok(());
    }
    let users = id_list(user_ids.iter().copied());
    if message.in_main() {
        sqlx::query(
            "UPDATE chat_members SET mention_count = MAX(mention_count + ?, 0) \
             WHERE conversation_id = ? AND user_id IN (SELECT value FROM json_each(?)) \
             AND (last_read_message_id IS NULL OR last_read_message_id < ?)",
        )
        .bind(delta)
        .bind(message.conversation_id.to_string())
        .bind(&users)
        .bind(message.id.to_string())
        .execute(&mut *conn)
        .await?;
    }
    if let Some(root_id) = message.thread_root_id {
        sqlx::query(
            "UPDATE chat_thread_members SET mention_count = MAX(mention_count + ?, 0) \
             WHERE root_id = ? AND following = 1 AND user_id IN (SELECT value FROM json_each(?)) \
             AND (last_read_reply_id IS NULL OR last_read_reply_id < ?)",
        )
        .bind(delta)
        .bind(root_id.to_string())
        .bind(&users)
        .bind(message.id.to_string())
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

/// Adds `delta` to the `@channel`/`@here` counter of every member but the author for whom the
/// message is unread. The caller decides whether the message has (or had) such a mention.
async fn adjust_broadcast(
    conn: &mut SqliteConnection,
    message: &MessageRecord,
    delta: i64,
) -> Result<(), ChatError> {
    if !message.in_main() {
        return Ok(());
    }
    sqlx::query(
        "UPDATE chat_members SET broadcast_count = MAX(broadcast_count + ?, 0) \
         WHERE conversation_id = ? AND user_id <> ? \
         AND (last_read_message_id IS NULL OR last_read_message_id < ?)",
    )
    .bind(delta)
    .bind(message.conversation_id.to_string())
    .bind(message.author_id.to_string())
    .bind(message.id.to_string())
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Takes a main-list message out of the counters: the conversation's total, and `read_count`
/// of the members for whom it was not unread (its author, and everyone who read past it).
async fn uncount_main(
    conn: &mut SqliteConnection,
    message: &MessageRecord,
) -> Result<(), ChatError> {
    sqlx::query(
        "UPDATE chat_conversations SET message_count = MAX(message_count - 1, 0) WHERE id = ?",
    )
    .bind(message.conversation_id.to_string())
    .execute(&mut *conn)
    .await?;
    sqlx::query(
        "UPDATE chat_members SET read_count = MAX(read_count - 1, 0) \
         WHERE conversation_id = ? AND (user_id = ? OR last_read_message_id >= ?)",
    )
    .bind(message.conversation_id.to_string())
    .bind(message.author_id.to_string())
    .bind(message.id.to_string())
    .execute(&mut *conn)
    .await?;
    if message.mentions.channel || message.mentions.here {
        adjust_broadcast(conn, message, -1).await?;
    }
    Ok(())
}

async fn delete_row(
    conn: &mut SqliteConnection,
    message: &MessageRecord,
    events: &mut Events,
) -> Result<(), ChatError> {
    // Reactions, mention rows and thread states go with it through `ON DELETE CASCADE`.
    sqlx::query("DELETE FROM chat_messages WHERE id = ?")
        .bind(message.id.to_string())
        .execute(&mut *conn)
        .await?;
    events.conversation(
        message.conversation_id,
        ChatEvent::MessageDeleted {
            conversation_id: message.conversation_id,
            message_id: message.id,
            thread_root_id: message.thread_root_id,
        },
    );
    Ok(())
}

/// The thread's side of a new reply: the root's summary, and who follows the thread now.
async fn after_reply(
    conn: &mut SqliteConnection,
    access: &Access,
    root: &MessageRecord,
    reply: &MessageRecord,
    mentioned: &[Id],
) -> Result<(), ChatError> {
    let conversation = &access.conversation;
    let reply_count = root.reply_count + 1;
    let mut authors = root.reply_user_ids.clone();
    if !authors.contains(&reply.author_id) && authors.len() < REPLY_AUTHORS {
        authors.push(reply.author_id);
    }
    sqlx::query(
        "UPDATE chat_messages SET reply_count = ?, last_reply_id = ?, reply_user_ids = ? WHERE id = ?",
    )
    .bind(reply_count)
    .bind(reply.id.to_string())
    .bind(id_list(authors))
    .bind(root.id.to_string())
    .execute(&mut *conn)
    .await?;

    // The author follows, and has read the thread up to the reply.
    upsert_thread_member(
        conn,
        conversation.workspace_id,
        root,
        reply.author_id,
        &ThreadMember {
            following: true,
            last_read_reply_id: Some(reply.id),
            read_reply_count: reply_count,
            mention_count: 0,
        },
    )
    .await?;

    // Members who start to follow with this reply: mentioned in it, or (if they have no
    // thread state yet) the root's author, members the root mentions, and both sides of a
    // 1:1 DM. The reply is their first unread one.
    let mut automatic = vec![root.author_id];
    automatic.extend(mention_row_users(conn, root.id).await?);
    if conversation.kind == ConversationKind::Dm {
        let members = super::member_ids(conn, conversation.id).await?;
        if members.len() <= 2 {
            automatic.extend(members);
        }
    }
    let mut candidates = mentioned.to_vec();
    candidates.extend(members_among(conn, conversation.id, &automatic, reply.author_id).await?);
    candidates.sort_unstable();
    candidates.dedup();
    for user_id in candidates {
        let is_mentioned = mentioned.contains(&user_id);
        let starts = match load_thread_member(conn, root.id, user_id).await? {
            // Already following: `adjust_mentions` counted the mention.
            Some(member) if member.following => false,
            Some(_) => is_mentioned,
            None => true,
        };
        if starts {
            upsert_thread_member(
                conn,
                conversation.workspace_id,
                root,
                user_id,
                &ThreadMember {
                    following: true,
                    last_read_reply_id: root.last_reply_id,
                    read_reply_count: reply_count - 1,
                    mention_count: i64::from(is_mentioned),
                },
            )
            .await?;
        }
    }
    Ok(())
}

/// Writes a member's state in a thread.
pub(super) async fn upsert_thread_member(
    conn: &mut SqliteConnection,
    workspace_id: Id,
    root: &MessageRecord,
    user_id: Id,
    member: &ThreadMember,
) -> Result<(), ChatError> {
    sqlx::query(
        "INSERT INTO chat_thread_members (root_id, user_id, workspace_id, conversation_id, following, \
         last_read_reply_id, read_reply_count, mention_count) VALUES (?, ?, ?, ?, ?, ?, ?, ?) \
         ON CONFLICT (root_id, user_id) DO UPDATE SET following = excluded.following, \
         last_read_reply_id = excluded.last_read_reply_id, \
         read_reply_count = excluded.read_reply_count, mention_count = excluded.mention_count",
    )
    .bind(root.id.to_string())
    .bind(user_id.to_string())
    .bind(workspace_id.to_string())
    .bind(root.conversation_id.to_string())
    .bind(member.following)
    .bind(member.last_read_reply_id.map(|id| id.to_string()))
    .bind(member.read_reply_count)
    .bind(member.mention_count)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// The root's summary after one of its replies was removed. A deleted root goes with its last
/// reply.
async fn after_reply_removed(
    conn: &mut SqliteConnection,
    root_id: Id,
    actor_id: Id,
    events: &mut Events,
) -> Result<(), ChatError> {
    let Some(root) = load_message(conn, root_id).await? else {
        return Ok(());
    };
    let last_reply_id: Option<String> = sqlx::query_scalar(
        "SELECT id FROM chat_messages WHERE thread_root_id = ? ORDER BY id DESC LIMIT 1",
    )
    .bind(root_id.to_string())
    .fetch_optional(&mut *conn)
    .await?;
    if last_reply_id.is_none() && root.deleted {
        return delete_row(conn, &root, events).await;
    }
    let authors = sqlx::query_scalar::<_, String>(
        "SELECT author_id FROM chat_messages WHERE thread_root_id = ? \
         GROUP BY author_id ORDER BY MIN(id) LIMIT ?",
    )
    .bind(root_id.to_string())
    .bind(REPLY_AUTHORS as i64)
    .fetch_all(&mut *conn)
    .await?;
    sqlx::query(
        "UPDATE chat_messages SET reply_count = MAX(reply_count - 1, 0), last_reply_id = ?, \
         reply_user_ids = ? WHERE id = ?",
    )
    .bind(last_reply_id)
    .bind(serde_json::to_string(&authors).expect("a list of strings serializes"))
    .bind(root_id.to_string())
    .execute(&mut *conn)
    .await?;
    let root = load_message(conn, root_id)
        .await?
        .ok_or(ChatError::NotFound)?;
    if let Some(member) = load_thread_member(conn, root_id, actor_id).await? {
        events.user(
            actor_id,
            ChatEvent::ThreadChanged {
                state: member.state(&root),
            },
        );
    }
    events.conversation(
        root.conversation_id,
        ChatEvent::MessageUpdated { message: root },
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use orbit_platform::TestDatabase;
    use sqlx::Row;

    use super::super::state::{mention_count_sql, newest_main_sql, unread_count_sql};
    use super::*;

    /// The steps of SQLite's plan for a query.
    async fn plan(database: &TestDatabase, sql: &str) -> Vec<String> {
        let explain = format!("EXPLAIN QUERY PLAN {sql}");
        let mut query = sqlx::query(&explain);
        for _ in 0..sql.matches('?').count() {
            query = query.bind("x");
        }
        query
            .fetch_all(database.pool())
            .await
            .unwrap()
            .iter()
            .map(|row| row.get::<String, _>("detail"))
            .collect()
    }

    /// Every list and count that runs for each page or each click must be a range read on its
    /// index: no table scan and no sort, however many messages there are.
    #[tokio::test]
    async fn message_queries_read_one_index_without_a_scan_or_a_sort() {
        let database = TestDatabase::new().await.unwrap();
        let main = Scope::Main(Id::new_v7());
        let thread = Scope::Thread(Id::new_v7());
        let conversation = "USING INDEX chat_messages_conversation (conversation_id=?";
        let replies = "USING INDEX chat_messages_thread (thread_root_id=?";
        for (name, sql, index) in [
            ("newest page", older_sql(main, false), conversation),
            ("older page", older_sql(main, true), conversation),
            ("newer page", newer_sql(main, false), conversation),
            ("thread, newest page", older_sql(thread, false), replies),
            ("thread, older page", older_sql(thread, true), replies),
            ("thread, newer page", newer_sql(thread, true), replies),
            (
                "threads of a conversation",
                threads_sql(),
                "USING INDEX chat_messages_threads (conversation_id=?",
            ),
            (
                "pins",
                pins_sql(),
                "USING INDEX chat_messages_pins (conversation_id=?",
            ),
            (
                "newest main-list id",
                newest_main_sql().to_owned(),
                conversation,
            ),
            ("unread count", unread_count_sql(true), conversation),
            (
                "mention count",
                mention_count_sql(true),
                "USING INDEX chat_message_mentions_user (user_id=? AND conversation_id=? AND message_id>?",
            ),
        ] {
            let steps = plan(&database, &sql).await;
            assert!(
                steps.iter().any(|step| step.contains(index)),
                "{name} does not use its index: {steps:?}"
            );
            assert!(
                !steps
                    .iter()
                    .any(|step| step.starts_with("SCAN") || step.contains("TEMP B-TREE")),
                "{name} scans or sorts: {steps:?}"
            );
        }
    }
}
