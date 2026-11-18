//! Reports of chat messages and blocks between users (App Store guideline 1.2).
//!
//! A report keeps a copy of the message body, so the chat managers who get it can read it after
//! an edit or a delete, and also when they cannot read the conversation (a DM). A block is the
//! blocker's own list: clients hide the blocked person's messages from it, and the blocked person
//! cannot open or write a DM with the blocker.

use orbit_domain::{Actor, Permission, ProjectGrants, WorkspaceRole};
use orbit_platform::{Flag, Id, Row, TimestampMillis};
use serde::{Deserialize, Serialize};
use sqlx::AnyConnection;
use sqlx::any::AnyRow;
use utoipa::ToSchema;

use super::messages::remove_message;
use super::{
    ChatError, ChatRepository, Events, MessageKind, Written, corrupt, finish, id_list, in_ids,
    load_actor, load_message, load_message_access, parse_id, parse_optional_id,
};
use crate::auth_routes::avatar_url;
use crate::repositories::membership::parse_permissions;

/// Characters in the note of a report.
pub const REPORT_NOTE_MAX_CHARS: usize = 500;
/// Reports in the managers' list, newest first.
const REPORT_LIST_LIMIT: i64 = 200;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
#[schema(as = ChatReportReason)]
pub enum ReportReason {
    Spam,
    Harassment,
    Inappropriate,
    Other,
}

impl ReportReason {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Spam => "spam",
            Self::Harassment => "harassment",
            Self::Inappropriate => "inappropriate",
            Self::Other => "other",
        }
    }

    fn from_db(value: &str) -> Result<Self, ChatError> {
        match value {
            "spam" => Ok(Self::Spam),
            "harassment" => Ok(Self::Harassment),
            "inappropriate" => Ok(Self::Inappropriate),
            "other" => Ok(Self::Other),
            _ => Err(corrupt("report reason")),
        }
    }
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct ChatReportRecord {
    #[schema(value_type = String)]
    pub id: Id,
    #[schema(value_type = String)]
    pub workspace_id: Id,
    #[schema(value_type = String)]
    pub conversation_id: Id,
    /// Null once the message is gone for good.
    #[schema(value_type = Option<String>, required = true)]
    pub message_id: Option<Id>,
    #[schema(value_type = String)]
    pub reporter_id: Id,
    pub reporter_name: String,
    #[schema(value_type = String)]
    pub author_id: Id,
    pub author_name: String,
    pub reason: ReportReason,
    pub note: String,
    /// The message body when it was reported.
    pub body: String,
    /// The message was deleted after the report.
    pub message_deleted: bool,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: TimestampMillis,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct BlockedUserRecord {
    #[schema(value_type = String)]
    pub user_id: Id,
    pub display_name: String,
    pub avatar_url: Option<String>,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: TimestampMillis,
}

/// Who an email about a new report goes to: the installation admins.
pub struct ReportMail {
    pub emails: Vec<String>,
    pub workspace_name: String,
}

const REPORT_SELECT: &str = "SELECT r.id, r.workspace_id, r.conversation_id, r.message_id, \
     r.reporter_id, reporter.display_name AS reporter_name, r.author_id, \
     author.display_name AS author_name, r.reason, r.note, r.body, r.created_at, \
     CASE WHEN m.id IS NULL OR m.deleted_at IS NOT NULL THEN 1 ELSE 0 END AS message_deleted \
     FROM chat_message_reports r \
     JOIN users reporter ON reporter.id = r.reporter_id \
     JOIN users author ON author.id = r.author_id \
     LEFT JOIN chat_messages m ON m.id = r.message_id";

impl ChatRepository {
    /// Reports a message that the caller can read and did not send. The members who can manage
    /// chat get an inbox notification. Reporting a message again returns the first report; the
    /// flag tells whether this call made the report.
    pub async fn report_message(
        &self,
        workspace_id: Id,
        actor_id: Id,
        message_id: Id,
        reason: ReportReason,
        note: &str,
    ) -> Result<Written<(ChatReportRecord, bool)>, ChatError> {
        let note = note.trim();
        if note.chars().count() > REPORT_NOTE_MAX_CHARS {
            return Err(ChatError::Invalid { field: "note" });
        }
        let now = TimestampMillis::now();
        let mut tx = self.database.immediate_transaction().await?;
        let (_, message) = load_message_access(&mut tx, workspace_id, actor_id, message_id).await?;
        if message.deleted || message.kind != MessageKind::Message {
            return Err(ChatError::NotFound);
        }
        if message.sender_id() == actor_id {
            return Err(ChatError::Forbidden("You cannot report your own message."));
        }
        let existing: Option<String> = sqlx::query_scalar(
            "SELECT id FROM chat_message_reports WHERE message_id = $1 AND reporter_id = $2",
        )
        .bind(message_id.to_string())
        .bind(actor_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(existing) = existing {
            let record = load_report(&mut tx, parse_id(existing)?).await?;
            return Ok(Events::default().written((record, false)));
        }
        let report_id = Id::new_v7();
        sqlx::query(
            "INSERT INTO chat_message_reports (id, workspace_id, conversation_id, message_id, \
             reporter_id, author_id, reason, note, body, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
        )
        .bind(report_id.to_string())
        .bind(workspace_id.to_string())
        .bind(message.conversation_id.to_string())
        .bind(message_id.to_string())
        .bind(actor_id.to_string())
        .bind(message.author_id.to_string())
        .bind(reason.as_str())
        .bind(note)
        .bind(&message.body)
        .bind(now.as_millis())
        .execute(&mut *tx)
        .await?;
        let mut events = Events::default();
        for recipient in chat_managers(&mut tx, workspace_id).await? {
            if recipient == actor_id {
                continue;
            }
            sqlx::query(
                "INSERT INTO notifications (id, workspace_id, recipient_user_id, actor_user_id, \
                 kind, chat_report_id, dedupe_key, created_at) \
                 VALUES ($1, $2, $3, $4, 'chat_reported', $5, $6, $7) ON CONFLICT DO NOTHING",
            )
            .bind(Id::new_v7().to_string())
            .bind(workspace_id.to_string())
            .bind(recipient.to_string())
            .bind(actor_id.to_string())
            .bind(report_id.to_string())
            .bind(format!(
                "{workspace_id}:{recipient}:chat_reported:{report_id}"
            ))
            .bind(now.as_millis())
            .execute(&mut *tx)
            .await?;
            events.inbox.push(recipient);
        }
        let record = load_report(&mut tx, report_id).await?;
        finish(tx, events, (record, true)).await
    }

    /// The workspace's reports, newest first, for the members who can manage chat.
    pub async fn list_reports(
        &self,
        workspace_id: Id,
        actor_id: Id,
    ) -> Result<Vec<ChatReportRecord>, ChatError> {
        let mut conn = self.database.pool().acquire().await?;
        require_chat_manager(&mut conn, workspace_id, actor_id).await?;
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "{REPORT_SELECT} WHERE r.workspace_id = $1 ORDER BY r.created_at DESC, r.id DESC LIMIT $2"
        )))
        .bind(workspace_id.to_string())
        .bind(REPORT_LIST_LIMIT)
        .fetch_all(&mut *conn)
        .await?
        .iter()
        .map(report_from_row)
        .collect()
    }

    /// A chat manager deletes a reported message, also in a conversation that they cannot read.
    /// A message that is gone already changes nothing.
    pub async fn delete_reported_message(
        &self,
        workspace_id: Id,
        actor_id: Id,
        report_id: Id,
    ) -> Result<Written<()>, ChatError> {
        let now = TimestampMillis::now();
        let mut tx = self.database.immediate_transaction().await?;
        require_chat_manager(&mut tx, workspace_id, actor_id).await?;
        let message_id: Option<Option<String>> = sqlx::query_scalar(
            "SELECT message_id FROM chat_message_reports WHERE id = $1 AND workspace_id = $2",
        )
        .bind(report_id.to_string())
        .bind(workspace_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        let message_id = parse_optional_id(message_id.ok_or(ChatError::NotFound)?)?;
        let message = match message_id {
            Some(message_id) => load_message(&mut tx, message_id).await?,
            None => None,
        };
        let Some(message) = message.filter(|message| !message.deleted) else {
            return Ok(Events::default().written(()));
        };
        let events = remove_message(&mut tx, workspace_id, actor_id, &message, now).await?;
        finish(tx, events, ()).await
    }

    /// The installation admins to email about a new report, when the workspace exists.
    pub async fn report_mail(&self, workspace_id: Id) -> Result<ReportMail, ChatError> {
        let emails = sqlx::query_scalar(
            "SELECT email FROM users WHERE installation_admin = 1 AND suspended_at IS NULL \
             ORDER BY email",
        )
        .fetch_all(self.database.pool())
        .await?;
        let workspace_name = sqlx::query_scalar("SELECT name FROM workspaces WHERE id = $1")
            .bind(workspace_id.to_string())
            .fetch_optional(self.database.pool())
            .await?
            .ok_or(ChatError::NotFound)?;
        Ok(ReportMail {
            emails,
            workspace_name,
        })
    }

    /// The people the user blocked, newest first.
    pub async fn list_blocks(&self, user_id: Id) -> Result<Vec<BlockedUserRecord>, ChatError> {
        sqlx::query(
            "SELECT b.blocked_id, u.display_name, a.updated_at AS avatar_updated_at, b.created_at \
             FROM user_blocks b JOIN users u ON u.id = b.blocked_id \
             LEFT JOIN user_avatars a ON a.user_id = b.blocked_id \
             WHERE b.blocker_id = $1 ORDER BY b.created_at DESC, b.blocked_id",
        )
        .bind(user_id.to_string())
        .fetch_all(self.database.pool())
        .await?
        .iter()
        .map(|row| {
            let user_id = parse_id(row.get("blocked_id"))?;
            Ok(BlockedUserRecord {
                user_id,
                display_name: row.get("display_name"),
                avatar_url: avatar_url(user_id, row.get("avatar_updated_at")),
                created_at: TimestampMillis::from_millis(row.get("created_at")),
            })
        })
        .collect()
    }

    /// Blocks a user who shares a workspace with the caller. Other users do not exist for the
    /// caller. Blocking again changes nothing.
    pub async fn block_user(&self, user_id: Id, target_id: Id) -> Result<(), ChatError> {
        if user_id == target_id {
            return Err(ChatError::Invalid { field: "user_id" });
        }
        let mut tx = self.database.immediate_transaction().await?;
        let Flag(shared) = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM memberships own \
             JOIN memberships theirs ON theirs.workspace_id = own.workspace_id \
             JOIN workspaces w ON w.id = own.workspace_id AND w.deleted_at IS NULL \
             WHERE own.user_id = $1 AND theirs.user_id = $2)",
        )
        .bind(user_id.to_string())
        .bind(target_id.to_string())
        .fetch_one(&mut *tx)
        .await?;
        if !shared {
            return Err(ChatError::NotFound);
        }
        sqlx::query(
            "INSERT INTO user_blocks (blocker_id, blocked_id, created_at) VALUES ($1, $2, $3) \
             ON CONFLICT DO NOTHING",
        )
        .bind(user_id.to_string())
        .bind(target_id.to_string())
        .bind(TimestampMillis::now().as_millis())
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Removes a block. Removing one that does not exist changes nothing.
    pub async fn unblock_user(&self, user_id: Id, target_id: Id) -> Result<(), ChatError> {
        sqlx::query("DELETE FROM user_blocks WHERE blocker_id = $1 AND blocked_id = $2")
            .bind(user_id.to_string())
            .bind(target_id.to_string())
            .execute(self.database.pool())
            .await?;
        Ok(())
    }
}

/// Refuses a DM with someone among `user_ids` who blocked `actor_id`.
pub(super) async fn require_not_blocked_by(
    conn: &mut AnyConnection,
    actor_id: Id,
    user_ids: &[Id],
) -> Result<(), ChatError> {
    let Flag(blocked) = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT EXISTS(SELECT 1 FROM user_blocks WHERE blocked_id = $1 AND blocker_id {})",
        in_ids(&*conn, 2)
    )))
    .bind(actor_id.to_string())
    .bind(id_list(user_ids.iter().copied()))
    .fetch_one(&mut *conn)
    .await?;
    if blocked {
        Err(ChatError::Forbidden(
            "You cannot send direct messages to this person.",
        ))
    } else {
        Ok(())
    }
}

/// Refuses a message in a DM where another member blocked `actor_id`.
pub(super) async fn require_dm_not_blocked(
    conn: &mut AnyConnection,
    conversation_id: Id,
    actor_id: Id,
) -> Result<(), ChatError> {
    let Flag(blocked) = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM chat_members m \
         JOIN user_blocks b ON b.blocker_id = m.user_id \
         WHERE m.conversation_id = $1 AND b.blocked_id = $2)",
    )
    .bind(conversation_id.to_string())
    .bind(actor_id.to_string())
    .fetch_one(&mut *conn)
    .await?;
    if blocked {
        Err(ChatError::Forbidden(
            "You cannot send direct messages to this person.",
        ))
    } else {
        Ok(())
    }
}

async fn require_chat_manager(
    conn: &mut AnyConnection,
    workspace_id: Id,
    actor_id: Id,
) -> Result<Actor, ChatError> {
    let actor = load_actor(conn, workspace_id, actor_id).await?;
    if actor.can(Permission::ChatManage) {
        Ok(actor)
    } else {
        Err(ChatError::Forbidden(
            "Only workspace owners and admins can see reports.",
        ))
    }
}

/// The members who can act on a report: those whose role (or custom role) can manage chat.
async fn chat_managers(conn: &mut AnyConnection, workspace_id: Id) -> Result<Vec<Id>, ChatError> {
    let rows = sqlx::query(
        "SELECT m.user_id, m.role, r.permissions FROM memberships m \
         JOIN users u ON u.id = m.user_id AND u.suspended_at IS NULL \
         LEFT JOIN workspace_roles r ON r.id = m.custom_role_id \
         WHERE m.workspace_id = $1 ORDER BY m.user_id",
    )
    .bind(workspace_id.to_string())
    .fetch_all(&mut *conn)
    .await?;
    let mut managers = Vec::new();
    for row in &rows {
        let user_id = parse_id(row.get("user_id"))?;
        let Some(role) = WorkspaceRole::parse(&row.get::<String, _>("role")) else {
            continue;
        };
        let actor = Actor::new(user_id, role).with_custom_role(
            parse_permissions(row.get::<Option<String>, _>("permissions").as_deref()),
            ProjectGrants::default(),
        );
        if actor.can(Permission::ChatManage) {
            managers.push(user_id);
        }
    }
    Ok(managers)
}

async fn load_report(
    conn: &mut AnyConnection,
    report_id: Id,
) -> Result<ChatReportRecord, ChatError> {
    let row = sqlx::query(sqlx::AssertSqlSafe(format!(
        "{REPORT_SELECT} WHERE r.id = $1"
    )))
    .bind(report_id.to_string())
    .fetch_one(&mut *conn)
    .await?;
    report_from_row(&row)
}

fn report_from_row(row: &AnyRow) -> Result<ChatReportRecord, ChatError> {
    Ok(ChatReportRecord {
        id: parse_id(row.get("id"))?,
        workspace_id: parse_id(row.get("workspace_id"))?,
        conversation_id: parse_id(row.get("conversation_id"))?,
        message_id: parse_optional_id(row.get("message_id"))?,
        reporter_id: parse_id(row.get("reporter_id"))?,
        reporter_name: row.get("reporter_name"),
        author_id: parse_id(row.get("author_id"))?,
        author_name: row.get("author_name"),
        reason: ReportReason::from_db(&row.get::<String, _>("reason"))?,
        note: row.get("note"),
        body: row.get("body"),
        message_deleted: row.get::<i64, _>("message_deleted") == 1,
        created_at: TimestampMillis::from_millis(row.get("created_at")),
    })
}
