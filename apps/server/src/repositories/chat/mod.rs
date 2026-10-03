//! Chat: channels, direct messages, threads, reactions, pins and each member's read state.
//!
//! Access: a public channel is readable by every workspace member and writable by its members;
//! a private channel or DM exists only for its members (everyone else gets `NotFound`).
//!
//! Counting: a member's unread count is the conversation's `message_count` minus the member's
//! `read_count`, so a send writes no row for the other members. The counters stay exact because
//! every change to them happens in the transaction of the message it belongs to.
//!
//! Chat writes are not audited: an audit row makes every open tab refetch the whole workspace.
//! Each write returns the chat events it caused, with their audience, for the caller's response
//! and for the live socket.

mod conversations;
mod files;
mod messages;
mod search;
mod state;

use std::collections::HashMap;

use orbit_domain::Actor;
use orbit_platform::{Database, Id, TimestampMillis, UploadError, UploadService};
use serde::{Deserialize, Serialize};
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, Sqlite, SqliteConnection, Transaction};
use thiserror::Error;
use utoipa::ToSchema;

use super::membership;
pub use crate::live::Recipients;

pub use conversations::{ChannelCreate, ChannelUpdate};
pub use messages::{MessageCursor, SendInput};
pub use search::{SearchHitRecord, SearchInput, SearchPage};
pub use state::{ConversationCursor, ReadSnapshot, StateUpdate, ThreadCursor};

/// Characters in one message body.
pub const MESSAGE_MAX_CHARS: usize = 4000;

#[derive(Debug, Error)]
pub enum ChatError {
    #[error("chat resource was not found")]
    NotFound,
    #[error("{0}")]
    Forbidden(&'static str),
    #[error("{0}")]
    Conflict(&'static str),
    #[error("chat input is invalid: {field}")]
    Invalid { field: &'static str },
    #[error("the message is too long")]
    TooLong,
    #[error(transparent)]
    Upload(#[from] UploadError),
    #[error("chat repository is unavailable")]
    Unavailable(#[from] sqlx::Error),
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConversationKind {
    Public,
    Private,
    Dm,
}

impl ConversationKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Private => "private",
            Self::Dm => "dm",
        }
    }

    fn from_db(value: &str) -> Result<Self, ChatError> {
        match value {
            "public" => Ok(Self::Public),
            "private" => Ok(Self::Private),
            "dm" => Ok(Self::Dm),
            _ => Err(corrupt("conversation kind")),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum NotifyLevel {
    All,
    Mentions,
    Muted,
}

impl NotifyLevel {
    const fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Mentions => "mentions",
            Self::Muted => "muted",
        }
    }

    fn from_db(value: &str) -> Result<Self, ChatError> {
        match value {
            "all" => Ok(Self::All),
            "mentions" => Ok(Self::Mentions),
            "muted" => Ok(Self::Muted),
            _ => Err(corrupt("notify level")),
        }
    }
}

/// `pin`, `join` and `leave` are one-line system rows; `author_id` is the member they are about.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum MessageKind {
    Message,
    Pin,
    Join,
    Leave,
}

impl MessageKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Message => "message",
            Self::Pin => "pin",
            Self::Join => "join",
            Self::Leave => "leave",
        }
    }

    fn from_db(value: &str) -> Result<Self, ChatError> {
        match value {
            "message" => Ok(Self::Message),
            "pin" => Ok(Self::Pin),
            "join" => Ok(Self::Join),
            "leave" => Ok(Self::Leave),
            _ => Err(corrupt("message kind")),
        }
    }
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct ConversationRecord {
    #[schema(value_type = String)]
    pub id: Id,
    pub kind: ConversationKind,
    /// Empty for a DM: its title is made from the other members' names.
    pub name: String,
    pub topic: String,
    #[schema(value_type = Option<String>, required = true)]
    pub category_id: Option<Id>,
    /// Order inside its category.
    pub position: i64,
    #[schema(value_type = Vec<String>)]
    pub member_ids: Vec<Id>,
    /// `#general`: every member is in it and it cannot be left, archived or made private.
    pub is_default: bool,
    /// A DM that was opened with its creator alone: their own place for notes. A DM whose other
    /// members left the workspace is not one.
    pub self_dm: bool,
    pub archived: bool,
    #[schema(value_type = String)]
    pub created_by: Id,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: TimestampMillis,
    #[schema(value_type = Option<String>, format = DateTime, required = true)]
    pub last_message_at: Option<TimestampMillis>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct CategoryRecord {
    #[schema(value_type = String)]
    pub id: Id,
    pub name: String,
    pub position: i64,
}

/// A file of a message.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct ChatFileRecord {
    #[schema(value_type = String)]
    pub id: Id,
    /// Same-origin download path; usable as an image `src` or a file link.
    pub url: String,
    pub file_name: String,
    /// Detected from the file's bytes, not taken from the client.
    pub mime_type: String,
    pub size_bytes: i64,
    /// Of an image, as the uploader's browser measured it.
    #[schema(required = true)]
    pub width: Option<i64>,
    #[schema(required = true)]
    pub height: Option<i64>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct ReactionRecord {
    pub emoji: String,
    #[schema(value_type = Vec<String>)]
    pub user_ids: Vec<Id>,
}

/// Who the body mentions, parsed from its tokens.
#[derive(Clone, Debug, Default, Serialize, ToSchema)]
pub struct MentionsRecord {
    #[schema(value_type = Vec<String>)]
    pub user_ids: Vec<Id>,
    pub channel: bool,
    pub here: bool,
}

/// The newest reply of a thread, shown under its root.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct ReplyPreviewRecord {
    #[schema(value_type = String)]
    pub author_id: Id,
    pub body: String,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: TimestampMillis,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct MessageRecord {
    /// UUIDv7: message ids sort by creation time, and every list orders by them.
    #[schema(value_type = String)]
    pub id: Id,
    #[schema(value_type = String)]
    pub conversation_id: Id,
    /// Set on a thread reply.
    #[schema(value_type = Option<String>, required = true)]
    pub thread_root_id: Option<Id>,
    pub kind: MessageKind,
    #[schema(value_type = String)]
    pub author_id: Id,
    /// Markdown source. Mentions are tokens: `<@user_id>`, `<#conversation_id>`, `<!channel>`,
    /// `<!here>`.
    pub body: String,
    pub mentions: MentionsRecord,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: TimestampMillis,
    #[schema(value_type = Option<String>, format = DateTime, required = true)]
    pub edited_at: Option<TimestampMillis>,
    /// A deleted root that still has replies stays in the list with an empty body.
    pub deleted: bool,
    pub attachments: Vec<ChatFileRecord>,
    pub reactions: Vec<ReactionRecord>,
    pub pinned: bool,
    /// A thread reply that also shows in the conversation's main list.
    pub also_in_channel: bool,
    /// Echo of the sender's nonce, so the sender can replace its optimistic row.
    #[schema(required = true)]
    pub nonce: Option<String>,
    /// Thread summary; meaningful on a root only.
    pub reply_count: i64,
    #[schema(value_type = Option<String>, format = DateTime, required = true)]
    pub last_reply_at: Option<TimestampMillis>,
    /// The first reply authors (at most five).
    #[schema(value_type = Vec<String>)]
    pub reply_user_ids: Vec<Id>,
    #[schema(required = true)]
    pub last_reply: Option<ReplyPreviewRecord>,
    #[serde(skip)]
    pub(crate) last_reply_id: Option<Id>,
}

impl MessageRecord {
    /// The message shows in the conversation's main list.
    fn in_main(&self) -> bool {
        self.thread_root_id.is_none() || self.also_in_channel
    }
}

/// The caller's state in one conversation.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct ConversationStateRecord {
    #[schema(value_type = String)]
    pub conversation_id: Id,
    #[schema(value_type = Option<String>, required = true)]
    pub last_read_message_id: Option<Id>,
    /// Unread messages in the main list, written by other members.
    pub unread_count: i64,
    /// Unread `@user` mentions, plus `@channel` and `@here` unless the conversation is muted.
    pub mention_count: i64,
    pub notify: NotifyLevel,
    pub favorite: bool,
}

/// The caller's state in one thread.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct ThreadStateRecord {
    #[schema(value_type = String)]
    pub root_id: Id,
    #[schema(value_type = String)]
    pub conversation_id: Id,
    pub following: bool,
    #[schema(value_type = Option<String>, required = true)]
    pub last_read_reply_id: Option<Id>,
    pub unread_replies: i64,
    pub mention_count: i64,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct FollowedThreadRecord {
    pub root: MessageRecord,
    #[schema(value_type = String)]
    pub conversation_id: Id,
    pub state: ThreadStateRecord,
    #[schema(required = true)]
    pub last_reply: Option<MessageRecord>,
}

/// Messages in ascending order. A null cursor means there is nothing more in that direction.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct MessagePage {
    pub items: Vec<MessageRecord>,
    #[schema(value_type = Option<String>, required = true)]
    pub before: Option<Id>,
    #[schema(value_type = Option<String>, required = true)]
    pub after: Option<Id>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct ThreadPage {
    pub items: Vec<MessageRecord>,
    #[schema(value_type = Option<String>, required = true)]
    pub before: Option<Id>,
    #[schema(value_type = Option<String>, required = true)]
    pub after: Option<Id>,
    pub root: MessageRecord,
    /// Null until the caller follows the thread or has read it.
    #[schema(required = true)]
    pub state: Option<ThreadStateRecord>,
}

/// One change, as the live socket sends it and as a write's response lists it.
#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(tag = "type")]
pub enum ChatEvent {
    #[serde(rename = "message.created")]
    MessageCreated { message: MessageRecord },
    /// Edits, reactions, pins and thread summary changes.
    #[serde(rename = "message.updated")]
    MessageUpdated { message: MessageRecord },
    #[serde(rename = "message.deleted")]
    MessageDeleted {
        #[schema(value_type = String)]
        conversation_id: Id,
        #[schema(value_type = String)]
        message_id: Id,
        #[schema(value_type = Option<String>, required = true)]
        thread_root_id: Option<Id>,
    },
    #[serde(rename = "conversation.changed")]
    ConversationChanged { conversation: ConversationRecord },
    /// The receiver lost access (left or was removed from a private channel).
    #[serde(rename = "conversation.removed")]
    ConversationRemoved {
        #[schema(value_type = String)]
        conversation_id: Id,
    },
    #[serde(rename = "categories.changed")]
    CategoriesChanged { categories: Vec<CategoryRecord> },
    #[serde(rename = "state.changed")]
    StateChanged { state: ConversationStateRecord },
    #[serde(rename = "thread.changed")]
    ThreadChanged { state: ThreadStateRecord },
}

/// Who an event is for, as the write that made it sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Audience {
    /// Every workspace member.
    Workspace,
    /// Everyone who can read the conversation: its members, and for a public channel every
    /// workspace member.
    Conversation(Id),
    /// Workspace members who are not in the conversation (it became private: it is gone
    /// for them).
    Outsiders(Id),
    User(Id),
}

#[derive(Clone, Debug)]
pub struct Emitted {
    pub recipients: Recipients,
    pub event: ChatEvent,
}

/// The result of a write and the events it caused.
#[derive(Clone, Debug)]
pub struct Written<T> {
    pub value: T,
    pub events: Vec<Emitted>,
    /// A conversation whose unread or mention counters changed for members other than the
    /// caller. Their states are not in `events`: the live socket asks for the states of the
    /// members who are connected (`ChatRepository::states_for`).
    pub counted: Option<Id>,
    /// A thread root whose followers have a new unread count, in the same way
    /// (`ChatRepository::thread_states_for`).
    pub replied: Option<Id>,
    /// Members whose inbox changed: a mention made a notification, or a read marked one.
    pub inbox: Vec<Id>,
}

impl<T> Written<T> {
    /// The events the caller itself sees.
    #[must_use]
    pub fn events_for(&self, user_id: Id) -> Vec<ChatEvent> {
        self.events
            .iter()
            .filter(|emitted| emitted.recipients.includes(user_id))
            .map(|emitted| emitted.event.clone())
            .collect()
    }
}

#[derive(Default)]
struct Events {
    events: Vec<(Audience, ChatEvent)>,
    counted: Option<Id>,
    replied: Option<Id>,
    inbox: Vec<Id>,
}

impl Events {
    fn conversation(&mut self, conversation_id: Id, event: ChatEvent) {
        self.events
            .push((Audience::Conversation(conversation_id), event));
    }

    fn outsiders(&mut self, conversation_id: Id, event: ChatEvent) {
        self.events
            .push((Audience::Outsiders(conversation_id), event));
    }

    fn workspace(&mut self, event: ChatEvent) {
        self.events.push((Audience::Workspace, event));
    }

    fn user(&mut self, user_id: Id, event: ChatEvent) {
        self.events.push((Audience::User(user_id), event));
    }

    /// The result of a call that changed nothing, or nothing that another member sees: only
    /// events for single users are kept.
    fn written<T>(self, value: T) -> Written<T> {
        let events = self
            .events
            .into_iter()
            .filter_map(|(audience, event)| match audience {
                Audience::User(user_id) => Some(Emitted {
                    recipients: Recipients::Users(vec![user_id]),
                    event,
                }),
                _ => None,
            })
            .collect();
        Written {
            value,
            events,
            counted: None,
            replied: None,
            inbox: Vec::new(),
        }
    }
}

/// Commits a write. The audiences become user ids first, inside the transaction, so they are
/// the readers of each conversation as this write left them.
async fn finish<T>(
    mut tx: Transaction<'_, Sqlite>,
    events: Events,
    value: T,
) -> Result<Written<T>, ChatError> {
    let mut members: HashMap<Id, (bool, Vec<Id>)> = HashMap::new();
    let mut emitted = Vec::with_capacity(events.events.len());
    for (audience, event) in events.events {
        let recipients = match audience {
            Audience::Workspace => Recipients::Workspace,
            Audience::User(user_id) => Recipients::Users(vec![user_id]),
            Audience::Conversation(id) | Audience::Outsiders(id) => {
                let (public, users) = match members.get(&id) {
                    Some(readers) => readers,
                    None => {
                        let public: Option<bool> = sqlx::query_scalar(
                            "SELECT kind = 'public' FROM chat_conversations WHERE id = ?",
                        )
                        .bind(id.to_string())
                        .fetch_optional(&mut *tx)
                        .await?;
                        let users = member_ids(&mut tx, id).await?;
                        members
                            .entry(id)
                            .or_insert((public.unwrap_or(false), users))
                    }
                };
                match audience {
                    Audience::Outsiders(_) => Recipients::WorkspaceExcept(users.clone()),
                    _ if *public => Recipients::Workspace,
                    _ => Recipients::Users(users.clone()),
                }
            }
        };
        emitted.push(Emitted { recipients, event });
    }
    tx.commit().await?;
    Ok(Written {
        value,
        events: emitted,
        counted: events.counted,
        replied: events.replied,
        inbox: events.inbox,
    })
}

#[derive(Clone)]
pub struct ChatRepository {
    database: Database,
    uploads: UploadService,
}

impl ChatRepository {
    #[must_use]
    pub fn new(database: Database, uploads: UploadService) -> Self {
        Self { database, uploads }
    }

    #[must_use]
    pub fn uploads(&self) -> &UploadService {
        &self.uploads
    }
}

/// One `chat_conversations` row.
#[derive(Clone, Debug)]
struct Conversation {
    id: Id,
    workspace_id: Id,
    kind: ConversationKind,
    name: String,
    topic: String,
    category_id: Option<Id>,
    position: i64,
    is_default: bool,
    self_dm: bool,
    archived_at: Option<i64>,
    created_by: Id,
    created_at: i64,
    last_message_at: Option<i64>,
    message_count: i64,
}

const CONVERSATION_COLUMNS: &str = "c.id, c.workspace_id, c.kind, c.name, c.topic, c.category_id, \
     c.position, c.is_default, c.archived_at, c.created_by, c.created_at, c.last_message_at, \
     c.message_count, (c.kind = 'dm' AND instr(c.dm_key, ',') = 0) AS self_dm";

fn conversation_from_row(row: &SqliteRow) -> Result<Conversation, ChatError> {
    Ok(Conversation {
        id: parse_id(row.get("id"))?,
        workspace_id: parse_id(row.get("workspace_id"))?,
        kind: ConversationKind::from_db(row.get::<String, _>("kind").as_str())?,
        name: row.get("name"),
        topic: row.get("topic"),
        category_id: parse_optional_id(row.get("category_id"))?,
        position: row.get("position"),
        is_default: row.get("is_default"),
        self_dm: row.get("self_dm"),
        archived_at: row.get("archived_at"),
        created_by: parse_id(row.get("created_by"))?,
        created_at: row.get("created_at"),
        last_message_at: row.get("last_message_at"),
        message_count: row.get("message_count"),
    })
}

/// The caller's own `chat_members` row.
#[derive(Clone, Debug)]
struct Member {
    notify: NotifyLevel,
    favorite: bool,
    last_read_message_id: Option<Id>,
    read_count: i64,
    mention_count: i64,
    broadcast_count: i64,
}

impl Member {
    fn state(&self, conversation: &Conversation) -> ConversationStateRecord {
        let broadcasts = if self.notify == NotifyLevel::Muted {
            0
        } else {
            self.broadcast_count
        };
        ConversationStateRecord {
            conversation_id: conversation.id,
            last_read_message_id: self.last_read_message_id,
            unread_count: (conversation.message_count - self.read_count).max(0),
            mention_count: self.mention_count + broadcasts,
            notify: self.notify,
            favorite: self.favorite,
        }
    }
}

const MEMBER_COLUMNS: &str = "m.notify, m.favorite, m.last_read_message_id, m.read_count, \
     m.mention_count, m.broadcast_count";

fn member_from_row(row: &SqliteRow) -> Result<Member, ChatError> {
    Ok(Member {
        notify: NotifyLevel::from_db(row.get::<String, _>("notify").as_str())?,
        favorite: row.get("favorite"),
        last_read_message_id: parse_optional_id(row.get("last_read_message_id"))?,
        read_count: row.get("read_count"),
        mention_count: row.get("mention_count"),
        broadcast_count: row.get("broadcast_count"),
    })
}

/// The caller in one conversation that it can read.
struct Access {
    actor: Actor,
    conversation: Conversation,
    member: Option<Member>,
}

impl Access {
    fn require_member(&self) -> Result<&Member, ChatError> {
        self.member
            .as_ref()
            .ok_or(ChatError::Forbidden("Join the channel first."))
    }

    fn require_open(&self) -> Result<(), ChatError> {
        if self.conversation.archived_at.is_some() {
            Err(ChatError::Forbidden("This channel is archived."))
        } else {
            Ok(())
        }
    }

    /// The creator and chat managers; never for a DM.
    fn require_manage(&self) -> Result<(), ChatError> {
        if self.conversation.kind != ConversationKind::Dm
            && self
                .actor
                .can_manage_chat_channel(self.conversation.created_by)
        {
            Ok(())
        } else {
            Err(ChatError::Forbidden(
                "Only the channel's creator and workspace owners and admins can do this.",
            ))
        }
    }
}

/// Non-members of the workspace and trashed workspaces are `NotFound`.
async fn load_actor(
    conn: &mut SqliteConnection,
    workspace_id: Id,
    user_id: Id,
) -> Result<Actor, ChatError> {
    membership::actor(&mut *conn, workspace_id, user_id)
        .await?
        .ok_or(ChatError::NotFound)
}

async fn load_conversation(
    conn: &mut SqliteConnection,
    workspace_id: Id,
    conversation_id: Id,
) -> Result<Option<Conversation>, ChatError> {
    sqlx::query(&format!(
        "SELECT {CONVERSATION_COLUMNS} FROM chat_conversations c WHERE c.id = ? AND c.workspace_id = ?"
    ))
    .bind(conversation_id.to_string())
    .bind(workspace_id.to_string())
    .fetch_optional(&mut *conn)
    .await?
    .as_ref()
    .map(conversation_from_row)
    .transpose()
}

async fn load_member(
    conn: &mut SqliteConnection,
    conversation_id: Id,
    user_id: Id,
) -> Result<Option<Member>, ChatError> {
    sqlx::query(&format!(
        "SELECT {MEMBER_COLUMNS} FROM chat_members m WHERE m.conversation_id = ? AND m.user_id = ?"
    ))
    .bind(conversation_id.to_string())
    .bind(user_id.to_string())
    .fetch_optional(&mut *conn)
    .await?
    .as_ref()
    .map(member_from_row)
    .transpose()
}

/// A private channel or DM that the caller is not in does not exist for the caller.
async fn load_access(
    conn: &mut SqliteConnection,
    workspace_id: Id,
    user_id: Id,
    conversation_id: Id,
) -> Result<Access, ChatError> {
    let actor = load_actor(conn, workspace_id, user_id).await?;
    let conversation = load_conversation(conn, workspace_id, conversation_id)
        .await?
        .ok_or(ChatError::NotFound)?;
    let member = load_member(conn, conversation_id, user_id).await?;
    if member.is_none() && conversation.kind != ConversationKind::Public {
        return Err(ChatError::NotFound);
    }
    Ok(Access {
        actor,
        conversation,
        member,
    })
}

async fn member_ids(
    conn: &mut SqliteConnection,
    conversation_id: Id,
) -> Result<Vec<Id>, ChatError> {
    sqlx::query_scalar::<_, String>(
        "SELECT user_id FROM chat_members WHERE conversation_id = ? ORDER BY joined_at, user_id",
    )
    .bind(conversation_id.to_string())
    .fetch_all(&mut *conn)
    .await?
    .into_iter()
    .map(parse_id)
    .collect()
}

/// Puts a new workspace member into every live public channel, with everything read, the way
/// the `chat_default_channel_for_member` trigger does for `#general`. Every insert into
/// `memberships` calls this in the same transaction, so a public channel always has every
/// workspace member. Channels the member is in already stay as they are.
pub async fn join_public_channels(
    conn: &mut SqliteConnection,
    workspace_id: Id,
    user_id: Id,
    now: TimestampMillis,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO chat_members (conversation_id, user_id, workspace_id, notify, \
         last_read_message_id, read_count, joined_at) \
         SELECT c.id, ?, c.workspace_id, 'mentions', \
                (SELECT id FROM chat_messages WHERE conversation_id = c.id ORDER BY id DESC LIMIT 1), \
                c.message_count, ? \
         FROM chat_conversations c \
         WHERE c.workspace_id = ? AND c.kind = 'public' AND c.archived_at IS NULL \
         ON CONFLICT (conversation_id, user_id) DO NOTHING",
    )
    .bind(user_id.to_string())
    .bind(now.as_millis())
    .bind(workspace_id.to_string())
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Every member of the workspace.
async fn workspace_member_ids(
    conn: &mut SqliteConnection,
    workspace_id: Id,
) -> Result<Vec<Id>, ChatError> {
    sqlx::query_scalar::<_, String>(
        "SELECT user_id FROM memberships WHERE workspace_id = ? ORDER BY created_at, user_id",
    )
    .bind(workspace_id.to_string())
    .fetch_all(&mut *conn)
    .await?
    .into_iter()
    .map(parse_id)
    .collect()
}

fn conversation_record(conversation: &Conversation, member_ids: Vec<Id>) -> ConversationRecord {
    ConversationRecord {
        id: conversation.id,
        kind: conversation.kind,
        name: conversation.name.clone(),
        topic: conversation.topic.clone(),
        category_id: conversation.category_id,
        position: conversation.position,
        member_ids,
        is_default: conversation.is_default,
        self_dm: conversation.self_dm,
        archived: conversation.archived_at.is_some(),
        created_by: conversation.created_by,
        created_at: TimestampMillis::from_millis(conversation.created_at),
        last_message_at: conversation
            .last_message_at
            .map(TimestampMillis::from_millis),
    }
}

/// The conversation as it is now, with its members.
async fn current_conversation_record(
    conn: &mut SqliteConnection,
    workspace_id: Id,
    conversation_id: Id,
) -> Result<ConversationRecord, ChatError> {
    let conversation = load_conversation(conn, workspace_id, conversation_id)
        .await?
        .ok_or(ChatError::NotFound)?;
    let members = member_ids(conn, conversation_id).await?;
    Ok(conversation_record(&conversation, members))
}

/// The caller's state as it is now; `None` when the caller is not a member.
async fn current_state(
    conn: &mut SqliteConnection,
    workspace_id: Id,
    conversation_id: Id,
    user_id: Id,
) -> Result<Option<ConversationStateRecord>, ChatError> {
    let Some(conversation) = load_conversation(conn, workspace_id, conversation_id).await? else {
        return Ok(None);
    };
    Ok(load_member(conn, conversation_id, user_id)
        .await?
        .map(|member| member.state(&conversation)))
}

const MESSAGE_SELECT: &str = "SELECT m.id, m.conversation_id, m.thread_root_id, m.kind, m.author_id, \
     m.body, m.mention_channel, m.mention_here, m.also_in_channel, m.nonce, m.pinned_at, \
     m.edited_at, m.deleted_at, m.created_at, m.reply_count, m.reply_user_ids, m.last_reply_id, \
     l.author_id AS last_author_id, l.body AS last_body, l.created_at AS last_created_at \
     FROM chat_messages m LEFT JOIN chat_messages l ON l.id = m.last_reply_id";

fn message_from_row(row: &SqliteRow) -> Result<MessageRecord, ChatError> {
    let body: String = row.get("body");
    let reply_user_ids: Vec<String> =
        serde_json::from_str(row.get::<String, _>("reply_user_ids").as_str())
            .map_err(|_| corrupt("reply authors"))?;
    let last_reply = match row.get::<Option<String>, _>("last_author_id") {
        Some(author_id) => Some(ReplyPreviewRecord {
            author_id: parse_id(author_id)?,
            body: row.get("last_body"),
            created_at: TimestampMillis::from_millis(row.get("last_created_at")),
        }),
        None => None,
    };
    Ok(MessageRecord {
        id: parse_id(row.get("id"))?,
        conversation_id: parse_id(row.get("conversation_id"))?,
        thread_root_id: parse_optional_id(row.get("thread_root_id"))?,
        kind: MessageKind::from_db(row.get::<String, _>("kind").as_str())?,
        author_id: parse_id(row.get("author_id"))?,
        mentions: MentionsRecord {
            user_ids: mentioned_user_ids(&body),
            channel: row.get("mention_channel"),
            here: row.get("mention_here"),
        },
        body,
        created_at: TimestampMillis::from_millis(row.get("created_at")),
        edited_at: row
            .get::<Option<i64>, _>("edited_at")
            .map(TimestampMillis::from_millis),
        deleted: row.get::<Option<i64>, _>("deleted_at").is_some(),
        attachments: Vec::new(),
        reactions: Vec::new(),
        pinned: row.get::<Option<i64>, _>("pinned_at").is_some(),
        also_in_channel: row.get("also_in_channel"),
        nonce: row.get("nonce"),
        reply_count: row.get("reply_count"),
        last_reply_at: last_reply.as_ref().map(|reply| reply.created_at),
        reply_user_ids: reply_user_ids
            .into_iter()
            .map(parse_id)
            .collect::<Result<_, _>>()?,
        last_reply,
        last_reply_id: parse_optional_id(row.get("last_reply_id"))?,
    })
}

/// Rows to records, with their reactions and files: two more queries for the whole list.
async fn hydrate(
    conn: &mut SqliteConnection,
    rows: &[SqliteRow],
) -> Result<Vec<MessageRecord>, ChatError> {
    let mut messages = rows
        .iter()
        .map(message_from_row)
        .collect::<Result<Vec<_>, _>>()?;
    if messages.is_empty() {
        return Ok(messages);
    }
    let reactions = sqlx::query(
        "SELECT message_id, emoji, user_id FROM chat_reactions \
         WHERE message_id IN (SELECT value FROM json_each(?)) \
         ORDER BY created_at, emoji, user_id",
    )
    .bind(id_list(messages.iter().map(|message| message.id)))
    .fetch_all(&mut *conn)
    .await?;
    for row in reactions {
        let message_id = parse_id(row.get("message_id"))?;
        let emoji: String = row.get("emoji");
        let user_id = parse_id(row.get("user_id"))?;
        let Some(message) = messages.iter_mut().find(|message| message.id == message_id) else {
            continue;
        };
        match message
            .reactions
            .iter_mut()
            .find(|reaction| reaction.emoji == emoji)
        {
            Some(reaction) => reaction.user_ids.push(user_id),
            None => message.reactions.push(ReactionRecord {
                emoji,
                user_ids: vec![user_id],
            }),
        }
    }
    files::load_files(conn, &mut messages).await?;
    Ok(messages)
}

async fn load_message(
    conn: &mut SqliteConnection,
    message_id: Id,
) -> Result<Option<MessageRecord>, ChatError> {
    let rows = sqlx::query(&format!("{MESSAGE_SELECT} WHERE m.id = ?"))
        .bind(message_id.to_string())
        .fetch_all(&mut *conn)
        .await?;
    Ok(hydrate(conn, &rows).await?.pop())
}

/// A message together with the caller's access to its conversation.
async fn load_message_access(
    conn: &mut SqliteConnection,
    workspace_id: Id,
    user_id: Id,
    message_id: Id,
) -> Result<(Access, MessageRecord), ChatError> {
    // The actor first, so an outsider learns nothing about message ids.
    load_actor(conn, workspace_id, user_id).await?;
    let message = load_message(conn, message_id)
        .await?
        .ok_or(ChatError::NotFound)?;
    let access = load_access(conn, workspace_id, user_id, message.conversation_id).await?;
    Ok((access, message))
}

/// The caller's `chat_thread_members` row.
#[derive(Clone, Debug)]
struct ThreadMember {
    following: bool,
    last_read_reply_id: Option<Id>,
    read_reply_count: i64,
    mention_count: i64,
}

impl ThreadMember {
    /// A thread that is not followed has nothing unread.
    fn state(&self, root: &MessageRecord) -> ThreadStateRecord {
        ThreadStateRecord {
            root_id: root.id,
            conversation_id: root.conversation_id,
            following: self.following,
            last_read_reply_id: self.last_read_reply_id,
            unread_replies: if self.following {
                (root.reply_count - self.read_reply_count).max(0)
            } else {
                0
            },
            mention_count: if self.following {
                self.mention_count
            } else {
                0
            },
        }
    }
}

async fn load_thread_member(
    conn: &mut SqliteConnection,
    root_id: Id,
    user_id: Id,
) -> Result<Option<ThreadMember>, ChatError> {
    let row = sqlx::query(
        "SELECT following, last_read_reply_id, read_reply_count, mention_count \
         FROM chat_thread_members WHERE root_id = ? AND user_id = ?",
    )
    .bind(root_id.to_string())
    .bind(user_id.to_string())
    .fetch_optional(&mut *conn)
    .await?;
    row.map(|row| {
        Ok(ThreadMember {
            following: row.get("following"),
            last_read_reply_id: parse_optional_id(row.get("last_read_reply_id"))?,
            read_reply_count: row.get("read_reply_count"),
            mention_count: row.get("mention_count"),
        })
    })
    .transpose()
}

/// The users a body mentions with `<@id>`, in order, each once. Tokens that are not ids are text.
fn mentioned_user_ids(body: &str) -> Vec<Id> {
    let mut ids = Vec::new();
    let mut rest = body;
    while let Some(start) = rest.find("<@") {
        rest = &rest[start + 2..];
        let Some(end) = rest.find('>') else { break };
        if let Ok(id) = rest[..end].parse::<Id>()
            && !ids.contains(&id)
        {
            ids.push(id);
        }
    }
    ids
}

/// A JSON array of ids, for `IN (SELECT value FROM json_each(?))`.
fn id_list(ids: impl IntoIterator<Item = Id>) -> String {
    let ids: Vec<String> = ids.into_iter().map(|id| id.to_string()).collect();
    serde_json::to_string(&ids).expect("a list of strings serializes")
}

fn parse_id(value: String) -> Result<Id, ChatError> {
    value.parse().map_err(|_| corrupt("id"))
}

fn parse_optional_id(value: Option<String>) -> Result<Option<Id>, ChatError> {
    value.map(parse_id).transpose()
}

/// A stored value that the schema should have made impossible.
fn corrupt(what: &'static str) -> ChatError {
    ChatError::Unavailable(sqlx::Error::Decode(
        format!("stored chat {what} is invalid").into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mention_tokens_give_each_user_once_and_skip_other_tokens() {
        let first = Id::new_v7();
        let second = Id::new_v7();
        let body = format!(
            "<@{first}> and <@{second}>, again <@{first}>, <@not-an-id>, <#{second}> <!channel> <@"
        );
        assert_eq!(mentioned_user_ids(&body), [first, second]);
        assert!(mentioned_user_ids("plain text").is_empty());
    }

    #[test]
    fn ids_made_in_one_millisecond_keep_their_order() {
        let ids: Vec<Id> = (0..10_000).map(|_| Id::new_v7()).collect();
        assert!(ids.windows(2).all(|pair| pair[0] < pair[1]));
        // The text form sorts the same way, which is what SQLite compares.
        assert!(
            ids.windows(2)
                .all(|pair| pair[0].to_string() < pair[1].to_string())
        );
    }
}
