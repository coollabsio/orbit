//! Channels, direct messages, their members, and the shared categories of the channel list.

use std::collections::HashMap;

use orbit_domain::{Actor, Permission};
use orbit_platform::{Id, TimestampMillis};
use sqlx::{Row, SqliteConnection};

use super::messages::add_system_row;
use super::state::emit_state;
use super::{
    Access, CONVERSATION_COLUMNS, CategoryRecord, ChatError, ChatEvent, ChatRepository,
    ConversationKind, ConversationRecord, Events, MessageKind, NotifyLevel, Written,
    conversation_from_row, conversation_record, current_conversation_record, finish, id_list,
    load_access, load_actor, parse_id, workspace_member_ids,
};

const NAME_MAX_CHARS: usize = 80;
const TOPIC_MAX_CHARS: usize = 250;
/// Members of one group DM, with the caller.
const MAX_DM_MEMBERS: usize = 9;
/// Members that one call may add to a channel.
const MAX_ADDED_MEMBERS: usize = 200;

#[derive(Clone, Debug)]
pub struct ChannelCreate {
    pub name: String,
    pub topic: String,
    pub category_id: Option<Id>,
    /// `Public` or `Private`.
    pub kind: ConversationKind,
    /// Members besides the caller.
    pub member_ids: Vec<Id>,
}

#[derive(Clone, Debug, Default)]
pub struct ChannelUpdate {
    pub name: Option<String>,
    pub topic: Option<String>,
    /// `None`: unchanged. `Some(None)`: no category.
    pub category_id: Option<Option<Id>>,
    pub kind: Option<ConversationKind>,
}

impl ChatRepository {
    /// Conversations the caller can see: the ones it is in, and every public channel (to
    /// browse and join). Archived ones are left out.
    pub async fn list_conversations(
        &self,
        workspace_id: Id,
        actor_id: Id,
    ) -> Result<Vec<ConversationRecord>, ChatError> {
        const VISIBLE: &str = "c.workspace_id = ? AND c.archived_at IS NULL \
             AND (c.kind = 'public' OR EXISTS (SELECT 1 FROM chat_members \
                  WHERE chat_members.conversation_id = c.id AND chat_members.user_id = ?))";
        let mut tx = self.database.pool().begin().await?;
        load_actor(&mut tx, workspace_id, actor_id).await?;
        let mut members: HashMap<Id, Vec<Id>> = HashMap::new();
        for row in sqlx::query(&format!(
            "SELECT cm.conversation_id, cm.user_id FROM chat_members cm \
             JOIN chat_conversations c ON c.id = cm.conversation_id WHERE {VISIBLE} \
             ORDER BY cm.joined_at, cm.user_id"
        ))
        .bind(workspace_id.to_string())
        .bind(actor_id.to_string())
        .fetch_all(&mut *tx)
        .await?
        {
            members
                .entry(parse_id(row.get("conversation_id"))?)
                .or_default()
                .push(parse_id(row.get("user_id"))?);
        }
        sqlx::query(&format!(
            "SELECT {CONVERSATION_COLUMNS} FROM chat_conversations c WHERE {VISIBLE} \
             ORDER BY c.created_at, c.id"
        ))
        .bind(workspace_id.to_string())
        .bind(actor_id.to_string())
        .fetch_all(&mut *tx)
        .await?
        .iter()
        .map(|row| {
            let conversation = conversation_from_row(row)?;
            let members = members.remove(&conversation.id).unwrap_or_default();
            Ok(conversation_record(&conversation, members))
        })
        .collect()
    }

    pub async fn list_categories(
        &self,
        workspace_id: Id,
        actor_id: Id,
    ) -> Result<Vec<CategoryRecord>, ChatError> {
        let mut tx = self.database.pool().begin().await?;
        load_actor(&mut tx, workspace_id, actor_id).await?;
        categories(&mut tx, workspace_id).await
    }

    /// Every member may make a channel. The caller is its first member; a public channel
    /// gets every workspace member.
    pub async fn create_channel(
        &self,
        workspace_id: Id,
        actor_id: Id,
        input: ChannelCreate,
    ) -> Result<Written<ConversationRecord>, ChatError> {
        if input.kind == ConversationKind::Dm {
            return Err(ChatError::Invalid { field: "kind" });
        }
        let name = clean_name(&input.name)?;
        let topic = clean_topic(&input.topic)?;
        let mut members = vec![actor_id];
        members.extend(input.member_ids);
        members.sort_unstable();
        members.dedup();
        if members.len() > MAX_ADDED_MEMBERS {
            return Err(ChatError::Invalid {
                field: "member_ids",
            });
        }
        let now = TimestampMillis::now();
        let mut tx = self.database.immediate_transaction().await?;
        load_actor(&mut tx, workspace_id, actor_id).await?;
        require_free_name(&mut tx, workspace_id, &name, None).await?;
        require_category(&mut tx, workspace_id, input.category_id).await?;
        require_workspace_members(&mut tx, workspace_id, &members).await?;
        // Every workspace member is in a public channel, like the default one.
        if input.kind == ConversationKind::Public {
            members = workspace_member_ids(&mut tx, workspace_id).await?;
        }
        let id = Id::new_v7();
        let position = next_position(&mut tx, workspace_id, input.category_id).await?;
        sqlx::query(
            "INSERT INTO chat_conversations (id, workspace_id, kind, name, topic, category_id, position, \
             created_by, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id.to_string())
        .bind(workspace_id.to_string())
        .bind(input.kind.as_str())
        .bind(&name)
        .bind(&topic)
        .bind(input.category_id.map(|id| id.to_string()))
        .bind(position)
        .bind(actor_id.to_string())
        .bind(now.as_millis())
        .bind(now.as_millis())
        .execute(&mut *tx)
        .await?;
        add_members(&mut tx, id, &members, NotifyLevel::Mentions, now).await?;
        let events = created_events(&mut tx, workspace_id, id, &members).await?;
        let record = current_conversation_record(&mut tx, workspace_id, id).await?;
        finish(tx, events, record).await
    }

    /// The channel's creator and chat managers change its name, topic, category and kind.
    pub async fn update_channel(
        &self,
        workspace_id: Id,
        actor_id: Id,
        conversation_id: Id,
        update: ChannelUpdate,
    ) -> Result<Written<ConversationRecord>, ChatError> {
        let now = TimestampMillis::now();
        let mut tx = self.database.immediate_transaction().await?;
        let access = load_access(&mut tx, workspace_id, actor_id, conversation_id).await?;
        access.require_manage()?;
        let current = &access.conversation;
        let kind = update.kind.unwrap_or(current.kind);
        if kind == ConversationKind::Dm {
            return Err(ChatError::Invalid { field: "kind" });
        }
        if current.is_default && kind != ConversationKind::Public {
            return Err(ChatError::Forbidden(
                "The default channel cannot be private.",
            ));
        }
        let name = match &update.name {
            Some(name) => {
                let name = clean_name(name)?;
                require_free_name(&mut tx, workspace_id, &name, Some(conversation_id)).await?;
                name
            }
            None => current.name.clone(),
        };
        let topic = match &update.topic {
            Some(topic) => clean_topic(topic)?,
            None => current.topic.clone(),
        };
        let category_id = update.category_id.unwrap_or(current.category_id);
        // A channel that changes category goes to the end of the new one.
        let position = if category_id == current.category_id {
            current.position
        } else {
            require_category(&mut tx, workspace_id, category_id).await?;
            next_position(&mut tx, workspace_id, category_id).await?
        };
        sqlx::query(
            "UPDATE chat_conversations SET name = ?, topic = ?, kind = ?, category_id = ?, position = ?, \
             updated_at = ? WHERE id = ?",
        )
        .bind(name)
        .bind(topic)
        .bind(kind.as_str())
        .bind(category_id.map(|id| id.to_string()))
        .bind(position)
        .bind(now.as_millis())
        .bind(conversation_id.to_string())
        .execute(&mut *tx)
        .await?;
        // A channel that becomes public gets every workspace member, with everything read.
        let added = if current.kind == ConversationKind::Private && kind == ConversationKind::Public
        {
            let current_members = super::member_ids(&mut tx, conversation_id).await?;
            let added: Vec<Id> = workspace_member_ids(&mut tx, workspace_id)
                .await?
                .into_iter()
                .filter(|id| !current_members.contains(id))
                .collect();
            add_members(&mut tx, conversation_id, &added, NotifyLevel::Mentions, now).await?;
            added
        } else {
            Vec::new()
        };
        let (mut events, record) = changed(&mut tx, workspace_id, conversation_id).await?;
        for user_id in added {
            emit_state(&mut tx, workspace_id, conversation_id, user_id, &mut events).await?;
        }
        if current.kind == ConversationKind::Public && kind == ConversationKind::Private {
            // The channel is gone for everyone who is not in it.
            events.outsiders(
                conversation_id,
                ChatEvent::ConversationRemoved { conversation_id },
            );
        }
        finish(tx, events, record).await
    }

    /// An archived channel is read-only and leaves the channel list.
    pub async fn archive_channel(
        &self,
        workspace_id: Id,
        actor_id: Id,
        conversation_id: Id,
    ) -> Result<Written<ConversationRecord>, ChatError> {
        let now = TimestampMillis::now();
        let mut tx = self.database.immediate_transaction().await?;
        let access = load_access(&mut tx, workspace_id, actor_id, conversation_id).await?;
        access.require_manage()?;
        if access.conversation.is_default {
            return Err(ChatError::Forbidden(
                "The default channel cannot be archived.",
            ));
        }
        sqlx::query(
            "UPDATE chat_conversations SET archived_at = COALESCE(archived_at, ?), updated_at = ? WHERE id = ?",
        )
        .bind(now.as_millis())
        .bind(now.as_millis())
        .bind(conversation_id.to_string())
        .execute(&mut *tx)
        .await?;
        let (events, record) = changed(&mut tx, workspace_id, conversation_id).await?;
        finish(tx, events, record).await
    }

    /// Every workspace member may join a public channel.
    pub async fn join_channel(
        &self,
        workspace_id: Id,
        actor_id: Id,
        conversation_id: Id,
    ) -> Result<Written<ConversationRecord>, ChatError> {
        let now = TimestampMillis::now();
        let mut tx = self.database.immediate_transaction().await?;
        let access = load_access(&mut tx, workspace_id, actor_id, conversation_id).await?;
        if access.member.is_some() {
            let record =
                current_conversation_record(&mut tx, workspace_id, conversation_id).await?;
            return Ok(Events::default().written(record));
        }
        access.require_open()?;
        add_members(
            &mut tx,
            conversation_id,
            &[actor_id],
            NotifyLevel::Mentions,
            now,
        )
        .await?;
        let row =
            add_system_row(&mut tx, conversation_id, MessageKind::Join, actor_id, now).await?;
        let (mut events, record) = changed(&mut tx, workspace_id, conversation_id).await?;
        events.conversation(conversation_id, ChatEvent::MessageCreated { message: row });
        emit_state(
            &mut tx,
            workspace_id,
            conversation_id,
            actor_id,
            &mut events,
        )
        .await?;
        finish(tx, events, record).await
    }

    /// Nobody leaves a public channel (every workspace member is in it) or a DM.
    pub async fn leave_channel(
        &self,
        workspace_id: Id,
        actor_id: Id,
        conversation_id: Id,
    ) -> Result<Written<()>, ChatError> {
        let mut tx = self.database.immediate_transaction().await?;
        let access = load_access(&mut tx, workspace_id, actor_id, conversation_id).await?;
        access.require_member()?;
        require_leavable(&access)?;
        let events = remove_from(&mut tx, &access, actor_id).await?;
        finish(tx, events, ()).await
    }

    /// Members of a public channel add people to it; for a private channel only its creator
    /// and chat managers do.
    pub async fn add_channel_members(
        &self,
        workspace_id: Id,
        actor_id: Id,
        conversation_id: Id,
        user_ids: Vec<Id>,
    ) -> Result<Written<ConversationRecord>, ChatError> {
        let mut user_ids = user_ids;
        user_ids.sort_unstable();
        user_ids.dedup();
        if user_ids.len() > MAX_ADDED_MEMBERS {
            return Err(ChatError::Invalid { field: "user_ids" });
        }
        let now = TimestampMillis::now();
        let mut tx = self.database.immediate_transaction().await?;
        let access = load_access(&mut tx, workspace_id, actor_id, conversation_id).await?;
        access.require_member()?;
        match access.conversation.kind {
            ConversationKind::Dm => {
                return Err(ChatError::Forbidden(
                    "Start a new message to talk to more people.",
                ));
            }
            ConversationKind::Private => access.require_manage()?,
            ConversationKind::Public => {}
        }
        access.require_open()?;
        require_workspace_members(&mut tx, workspace_id, &user_ids).await?;
        let current = super::member_ids(&mut tx, conversation_id).await?;
        let added: Vec<Id> = user_ids
            .into_iter()
            .filter(|id| !current.contains(id))
            .collect();
        add_members(&mut tx, conversation_id, &added, NotifyLevel::Mentions, now).await?;
        let mut rows = Vec::with_capacity(added.len());
        for user_id in &added {
            rows.push(
                add_system_row(&mut tx, conversation_id, MessageKind::Join, *user_id, now).await?,
            );
        }
        let (mut events, record) = changed(&mut tx, workspace_id, conversation_id).await?;
        for row in rows {
            events.conversation(conversation_id, ChatEvent::MessageCreated { message: row });
        }
        for user_id in added {
            emit_state(&mut tx, workspace_id, conversation_id, user_id, &mut events).await?;
        }
        finish(tx, events, record).await
    }

    /// The channel's creator and chat managers remove a member of a private channel.
    pub async fn remove_channel_member(
        &self,
        workspace_id: Id,
        actor_id: Id,
        conversation_id: Id,
        user_id: Id,
    ) -> Result<Written<ConversationRecord>, ChatError> {
        let mut tx = self.database.immediate_transaction().await?;
        let access = load_access(&mut tx, workspace_id, actor_id, conversation_id).await?;
        access.require_manage()?;
        require_leavable(&access)?;
        let is_member = super::load_member(&mut tx, conversation_id, user_id)
            .await?
            .is_some();
        let events = if is_member {
            remove_from(&mut tx, &access, user_id).await?
        } else {
            Events::default()
        };
        let record = current_conversation_record(&mut tx, workspace_id, conversation_id).await?;
        finish(tx, events, record).await
    }

    /// The DM of exactly these members and the caller. One set of people has one DM.
    pub async fn open_dm(
        &self,
        workspace_id: Id,
        actor_id: Id,
        user_ids: Vec<Id>,
    ) -> Result<Written<ConversationRecord>, ChatError> {
        let mut members = user_ids;
        members.push(actor_id);
        members.sort_unstable();
        members.dedup();
        if members.len() > MAX_DM_MEMBERS {
            return Err(ChatError::Invalid { field: "user_ids" });
        }
        let dm_key = members
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let now = TimestampMillis::now();
        let mut tx = self.database.immediate_transaction().await?;
        load_actor(&mut tx, workspace_id, actor_id).await?;
        let existing: Option<String> = sqlx::query_scalar(
            "SELECT id FROM chat_conversations WHERE workspace_id = ? AND dm_key = ?",
        )
        .bind(workspace_id.to_string())
        .bind(&dm_key)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(existing) = existing {
            let id = parse_id(existing)?;
            // A member who left the workspace and came back lost the row; put it back.
            let present: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM chat_members WHERE conversation_id = ?")
                    .bind(id.to_string())
                    .fetch_one(&mut *tx)
                    .await?;
            if usize::try_from(present) == Ok(members.len())
                || require_workspace_members(&mut tx, workspace_id, &members)
                    .await
                    .is_err()
            {
                let record = current_conversation_record(&mut tx, workspace_id, id).await?;
                return Ok(Events::default().written(record));
            }
            add_members(&mut tx, id, &members, NotifyLevel::All, now).await?;
            let events = created_events(&mut tx, workspace_id, id, &members).await?;
            let record = current_conversation_record(&mut tx, workspace_id, id).await?;
            return finish(tx, events, record).await;
        }
        require_workspace_members(&mut tx, workspace_id, &members).await?;
        let id = Id::new_v7();
        sqlx::query(
            "INSERT INTO chat_conversations (id, workspace_id, kind, dm_key, created_by, created_at, updated_at) \
             VALUES (?, ?, 'dm', ?, ?, ?, ?)",
        )
        .bind(id.to_string())
        .bind(workspace_id.to_string())
        .bind(dm_key)
        .bind(actor_id.to_string())
        .bind(now.as_millis())
        .bind(now.as_millis())
        .execute(&mut *tx)
        .await?;
        add_members(&mut tx, id, &members, NotifyLevel::All, now).await?;
        let events = created_events(&mut tx, workspace_id, id, &members).await?;
        let record = current_conversation_record(&mut tx, workspace_id, id).await?;
        finish(tx, events, record).await
    }

    pub async fn create_category(
        &self,
        workspace_id: Id,
        actor_id: Id,
        name: &str,
    ) -> Result<Written<CategoryRecord>, ChatError> {
        let name = clean_category_name(name)?;
        let now = TimestampMillis::now();
        let mut tx = self.database.immediate_transaction().await?;
        require_manager(load_actor(&mut tx, workspace_id, actor_id).await?)?;
        let id = Id::new_v7();
        sqlx::query(
            "INSERT INTO chat_categories (id, workspace_id, name, position, created_at, updated_at) \
             SELECT ?, ?, ?, COALESCE(MAX(position) + 1, 0), ?, ? FROM chat_categories WHERE workspace_id = ?",
        )
        .bind(id.to_string())
        .bind(workspace_id.to_string())
        .bind(name)
        .bind(now.as_millis())
        .bind(now.as_millis())
        .bind(workspace_id.to_string())
        .execute(&mut *tx)
        .await?;
        let (events, category) = category_changed(&mut tx, workspace_id, id).await?;
        finish(tx, events, category).await
    }

    pub async fn rename_category(
        &self,
        workspace_id: Id,
        actor_id: Id,
        category_id: Id,
        name: &str,
    ) -> Result<Written<CategoryRecord>, ChatError> {
        let name = clean_category_name(name)?;
        let now = TimestampMillis::now();
        let mut tx = self.database.immediate_transaction().await?;
        require_manager(load_actor(&mut tx, workspace_id, actor_id).await?)?;
        sqlx::query(
            "UPDATE chat_categories SET name = ?, updated_at = ? WHERE id = ? AND workspace_id = ?",
        )
        .bind(name)
        .bind(now.as_millis())
        .bind(category_id.to_string())
        .bind(workspace_id.to_string())
        .execute(&mut *tx)
        .await?;
        let (events, category) = category_changed(&mut tx, workspace_id, category_id).await?;
        finish(tx, events, category).await
    }

    /// The category's channels move to the end of the channels without a category.
    pub async fn delete_category(
        &self,
        workspace_id: Id,
        actor_id: Id,
        category_id: Id,
    ) -> Result<Written<()>, ChatError> {
        let mut tx = self.database.immediate_transaction().await?;
        require_manager(load_actor(&mut tx, workspace_id, actor_id).await?)?;
        require_category(&mut tx, workspace_id, Some(category_id)).await?;
        let channels = sqlx::query(
            "SELECT id, archived_at FROM chat_conversations WHERE workspace_id = ? AND category_id = ? \
             ORDER BY position, id",
        )
        .bind(workspace_id.to_string())
        .bind(category_id.to_string())
        .fetch_all(&mut *tx)
        .await?;
        let mut position = next_position(&mut tx, workspace_id, None).await?;
        let mut events = Events::default();
        for row in channels {
            let id = parse_id(row.get("id"))?;
            sqlx::query(
                "UPDATE chat_conversations SET category_id = NULL, position = ? WHERE id = ?",
            )
            .bind(position)
            .bind(id.to_string())
            .execute(&mut *tx)
            .await?;
            position += 1;
            // An archived channel is in nobody's list; an event would put it back.
            if row.get::<Option<i64>, _>("archived_at").is_none() {
                let record = current_conversation_record(&mut tx, workspace_id, id).await?;
                events.conversation(
                    id,
                    ChatEvent::ConversationChanged {
                        conversation: record,
                    },
                );
            }
        }
        sqlx::query("DELETE FROM chat_categories WHERE id = ? AND workspace_id = ?")
            .bind(category_id.to_string())
            .bind(workspace_id.to_string())
            .execute(&mut *tx)
            .await?;
        events.workspace(ChatEvent::CategoriesChanged {
            categories: categories(&mut tx, workspace_id).await?,
        });
        finish(tx, events, ()).await
    }

    /// Puts a category before the category `before_id`, or at the end. The categories get new
    /// positions in order.
    pub async fn place_category(
        &self,
        workspace_id: Id,
        actor_id: Id,
        category_id: Id,
        before_id: Option<Id>,
    ) -> Result<Written<()>, ChatError> {
        let mut tx = self.database.immediate_transaction().await?;
        require_manager(load_actor(&mut tx, workspace_id, actor_id).await?)?;
        require_category(&mut tx, workspace_id, Some(category_id)).await?;
        if before_id == Some(category_id) {
            return Ok(Events::default().written(()));
        }
        let mut order = sqlx::query(
            "SELECT id FROM chat_categories WHERE workspace_id = ? AND id <> ? ORDER BY position, id",
        )
        .bind(workspace_id.to_string())
        .bind(category_id.to_string())
        .fetch_all(&mut *tx)
        .await?
        .iter()
        .map(|row| parse_id(row.get("id")))
        .collect::<Result<Vec<_>, ChatError>>()?;
        // A `before_id` that is not a category (the list of the caller was old) means the end.
        let at = before_id
            .and_then(|before| order.iter().position(|id| *id == before))
            .unwrap_or(order.len());
        order.insert(at, category_id);
        for (position, id) in (0_i64..).zip(order) {
            sqlx::query("UPDATE chat_categories SET position = ? WHERE id = ?")
                .bind(position)
                .bind(id.to_string())
                .execute(&mut *tx)
                .await?;
        }
        let mut events = Events::default();
        events.workspace(ChatEvent::CategoriesChanged {
            categories: categories(&mut tx, workspace_id).await?,
        });
        finish(tx, events, ()).await
    }

    /// Puts a channel into a category (`None`: the channels without one), before the channel
    /// `before_id` or at the end. The channels of that category get new positions in order.
    pub async fn place_channel(
        &self,
        workspace_id: Id,
        actor_id: Id,
        conversation_id: Id,
        category_id: Option<Id>,
        before_id: Option<Id>,
    ) -> Result<Written<()>, ChatError> {
        let mut tx = self.database.immediate_transaction().await?;
        require_manager(load_actor(&mut tx, workspace_id, actor_id).await?)?;
        let access = load_access(&mut tx, workspace_id, actor_id, conversation_id).await?;
        if access.conversation.kind == ConversationKind::Dm {
            return Err(ChatError::NotFound);
        }
        access.require_open()?;
        require_category(&mut tx, workspace_id, category_id).await?;
        if before_id == Some(conversation_id) {
            return Ok(Events::default().written(()));
        }
        let siblings = sqlx::query(
            "SELECT id, position FROM chat_conversations WHERE workspace_id = ? AND kind <> 'dm' \
             AND archived_at IS NULL AND category_id IS ? AND id <> ? ORDER BY position, id",
        )
        .bind(workspace_id.to_string())
        .bind(category_id.map(|id| id.to_string()))
        .bind(conversation_id.to_string())
        .fetch_all(&mut *tx)
        .await?
        .iter()
        .map(|row| Ok((parse_id(row.get("id"))?, row.get::<i64, _>("position"))))
        .collect::<Result<Vec<_>, ChatError>>()?;
        // A `before_id` that is not in the category (the list of the caller was old) means the end.
        let at = before_id
            .and_then(|before| siblings.iter().position(|(id, _)| *id == before))
            .unwrap_or(siblings.len());
        let mut order = siblings;
        order.insert(at, (conversation_id, -1));

        let mut events = Events::default();
        for (position, (id, old)) in (0_i64..).zip(order) {
            if old == position {
                continue;
            }
            sqlx::query("UPDATE chat_conversations SET position = ?, category_id = CASE WHEN id = ? THEN ? ELSE category_id END WHERE id = ?")
                .bind(position)
                .bind(conversation_id.to_string())
                .bind(category_id.map(|id| id.to_string()))
                .bind(id.to_string())
                .execute(&mut *tx)
                .await?;
            let record = current_conversation_record(&mut tx, workspace_id, id).await?;
            events.conversation(
                id,
                ChatEvent::ConversationChanged {
                    conversation: record,
                },
            );
        }
        finish(tx, events, ()).await
    }
}

/// Trimmed, lower case, `-` for runs of white space, at most 80 characters.
fn clean_name(name: &str) -> Result<String, ChatError> {
    let name: String = name
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-")
        .chars()
        .take(NAME_MAX_CHARS)
        .collect();
    if name.is_empty() {
        Err(ChatError::Invalid { field: "name" })
    } else {
        Ok(name)
    }
}

fn clean_topic(topic: &str) -> Result<String, ChatError> {
    let topic = topic.trim();
    if topic.chars().count() > TOPIC_MAX_CHARS {
        Err(ChatError::Invalid { field: "topic" })
    } else {
        Ok(topic.to_owned())
    }
}

fn clean_category_name(name: &str) -> Result<String, ChatError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > NAME_MAX_CHARS {
        Err(ChatError::Invalid { field: "name" })
    } else {
        Ok(name.to_owned())
    }
}

fn require_manager(actor: Actor) -> Result<(), ChatError> {
    if actor.can(Permission::ChatManage) {
        Ok(())
    } else {
        Err(ChatError::Forbidden(
            "Only workspace owners and admins can change categories and the channel order.",
        ))
    }
}

fn require_leavable(access: &Access) -> Result<(), ChatError> {
    if access.conversation.is_default {
        Err(ChatError::Forbidden(
            "Nobody can leave the default channel.",
        ))
    } else if access.conversation.kind == ConversationKind::Dm {
        Err(ChatError::Forbidden("A direct message cannot be left."))
    } else if access.conversation.kind == ConversationKind::Public {
        Err(ChatError::Forbidden(
            "Every workspace member is in a public channel; only a private channel can be left.",
        ))
    } else {
        Ok(())
    }
}

/// Two live channels of a workspace cannot share a name.
async fn require_free_name(
    conn: &mut SqliteConnection,
    workspace_id: Id,
    name: &str,
    except: Option<Id>,
) -> Result<(), ChatError> {
    let taken: Option<String> = sqlx::query_scalar(
        "SELECT id FROM chat_conversations WHERE workspace_id = ? AND name = ? AND kind <> 'dm' \
         AND archived_at IS NULL",
    )
    .bind(workspace_id.to_string())
    .bind(name)
    .fetch_optional(&mut *conn)
    .await?;
    match taken.map(parse_id).transpose()? {
        Some(id) if Some(id) != except => Err(ChatError::Conflict(
            "A channel with this name already exists.",
        )),
        _ => Ok(()),
    }
}

async fn require_category(
    conn: &mut SqliteConnection,
    workspace_id: Id,
    category_id: Option<Id>,
) -> Result<(), ChatError> {
    let Some(category_id) = category_id else {
        return Ok(());
    };
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM chat_categories WHERE id = ? AND workspace_id = ?)",
    )
    .bind(category_id.to_string())
    .bind(workspace_id.to_string())
    .fetch_one(&mut *conn)
    .await?;
    if exists {
        Ok(())
    } else {
        Err(ChatError::NotFound)
    }
}

/// Every id must be a member of the workspace. `user_ids` has no duplicates.
async fn require_workspace_members(
    conn: &mut SqliteConnection,
    workspace_id: Id,
    user_ids: &[Id],
) -> Result<(), ChatError> {
    let found: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM memberships WHERE workspace_id = ? \
         AND user_id IN (SELECT value FROM json_each(?))",
    )
    .bind(workspace_id.to_string())
    .bind(id_list(user_ids.iter().copied()))
    .fetch_one(&mut *conn)
    .await?;
    if usize::try_from(found) == Ok(user_ids.len()) {
        Ok(())
    } else {
        Err(ChatError::NotFound)
    }
}

/// The position after the last channel of a category (or of the channels without one).
async fn next_position(
    conn: &mut SqliteConnection,
    workspace_id: Id,
    category_id: Option<Id>,
) -> Result<i64, ChatError> {
    Ok(sqlx::query_scalar(
        "SELECT COALESCE(MAX(position) + 1, 0) FROM chat_conversations \
         WHERE workspace_id = ? AND kind <> 'dm' AND category_id IS ?",
    )
    .bind(workspace_id.to_string())
    .bind(category_id.map(|id| id.to_string()))
    .fetch_one(&mut *conn)
    .await?)
}

/// Adds members who start with everything read. Members that are in already stay as they are.
async fn add_members(
    conn: &mut SqliteConnection,
    conversation_id: Id,
    user_ids: &[Id],
    notify: NotifyLevel,
    now: TimestampMillis,
) -> Result<(), ChatError> {
    if user_ids.is_empty() {
        return Ok(());
    }
    sqlx::query(
        "INSERT INTO chat_members (conversation_id, user_id, workspace_id, notify, \
         last_read_message_id, read_count, joined_at) \
         SELECT c.id, users.value, c.workspace_id, ?, \
                (SELECT id FROM chat_messages WHERE conversation_id = c.id ORDER BY id DESC LIMIT 1), \
                c.message_count, ? \
         FROM chat_conversations c, json_each(?) AS users WHERE c.id = ? \
         ON CONFLICT (conversation_id, user_id) DO NOTHING",
    )
    .bind(notify.as_str())
    .bind(now.as_millis())
    .bind(id_list(user_ids.iter().copied()))
    .bind(conversation_id.to_string())
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// The events of a new conversation: the conversation, and each member's state in it.
async fn created_events(
    conn: &mut SqliteConnection,
    workspace_id: Id,
    conversation_id: Id,
    members: &[Id],
) -> Result<Events, ChatError> {
    let (mut events, _) = changed(conn, workspace_id, conversation_id).await?;
    for user_id in members {
        emit_state(conn, workspace_id, conversation_id, *user_id, &mut events).await?;
    }
    Ok(events)
}

/// The conversation as it is now, as an event for everyone who can read it.
async fn changed(
    conn: &mut SqliteConnection,
    workspace_id: Id,
    conversation_id: Id,
) -> Result<(Events, ConversationRecord), ChatError> {
    let record = current_conversation_record(conn, workspace_id, conversation_id).await?;
    let mut events = Events::default();
    events.conversation(
        conversation_id,
        ChatEvent::ConversationChanged {
            conversation: record.clone(),
        },
    );
    Ok((events, record))
}

/// Takes `user_id` out of a channel, with a `leave` row. A private channel is gone for that
/// member, so its thread states go too.
async fn remove_from(
    conn: &mut SqliteConnection,
    access: &Access,
    user_id: Id,
) -> Result<Events, ChatError> {
    let conversation = &access.conversation;
    let row = add_system_row(
        conn,
        conversation.id,
        MessageKind::Leave,
        user_id,
        TimestampMillis::now(),
    )
    .await?;
    sqlx::query("DELETE FROM chat_members WHERE conversation_id = ? AND user_id = ?")
        .bind(conversation.id.to_string())
        .bind(user_id.to_string())
        .execute(&mut *conn)
        .await?;
    let (mut events, _) = changed(conn, conversation.workspace_id, conversation.id).await?;
    events.conversation(conversation.id, ChatEvent::MessageCreated { message: row });
    if conversation.kind != ConversationKind::Public {
        sqlx::query("DELETE FROM chat_thread_members WHERE conversation_id = ? AND user_id = ?")
            .bind(conversation.id.to_string())
            .bind(user_id.to_string())
            .execute(&mut *conn)
            .await?;
        events.user(
            user_id,
            ChatEvent::ConversationRemoved {
                conversation_id: conversation.id,
            },
        );
    }
    Ok(events)
}

async fn categories(
    conn: &mut SqliteConnection,
    workspace_id: Id,
) -> Result<Vec<CategoryRecord>, ChatError> {
    sqlx::query(
        "SELECT id, name, position FROM chat_categories WHERE workspace_id = ? ORDER BY position, id",
    )
    .bind(workspace_id.to_string())
    .fetch_all(&mut *conn)
    .await?
    .iter()
    .map(|row| {
        Ok(CategoryRecord {
            id: parse_id(row.get("id"))?,
            name: row.get("name"),
            position: row.get("position"),
        })
    })
    .collect()
}

/// The category list as an event for the workspace, and the category that changed.
async fn category_changed(
    conn: &mut SqliteConnection,
    workspace_id: Id,
    category_id: Id,
) -> Result<(Events, CategoryRecord), ChatError> {
    let categories = categories(conn, workspace_id).await?;
    let category = categories
        .iter()
        .find(|category| category.id == category_id)
        .cloned()
        .ok_or(ChatError::NotFound)?;
    let mut events = Events::default();
    events.workspace(ChatEvent::CategoriesChanged { categories });
    Ok((events, category))
}
