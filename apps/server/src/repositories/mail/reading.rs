//! Reading and organising mail (spec §7 Threads, Search, Counts; §8.3): thread lists, a thread
//! with its messages, folder, read, star and label changes, unread counts and the client's
//! mailbox list.
//!
//! Only members of a mailbox (a `mailbox_members` row of the caller's membership) read its
//! threads; anyone else gets `NotFound`. A personal mailbox whose owner left has no member, so
//! nobody reads it here (admins see it in the management view). Changes write no audit rows (as
//! chat) and announce `thread.updated` to the mailbox members after the commit.

use std::collections::{BTreeSet, HashMap};

use orbit_domain::{Actor, Permission, WorkspaceRole};
use orbit_mail::{AuthOutcome, DmarcVerdict, Verdict};
use orbit_platform::{Backend, Id, Row, SqlBuilder, TimestampMillis};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{Any, Transaction};
use utoipa::ToSchema;

use super::domains::parse_id;
use super::ingest::MAIL_TOPIC;
use super::labels::{Label, load_labels};
use super::mailboxes::{Mailbox, Scope, load_mailboxes};
use super::search::{Folders, Selection, ThreadCursor, people, select_threads};
use super::threads::{is_inside, mailbox_users};
use super::{MailError, MailQuery, MailRepository};
use crate::live::{LiveHub, NewEvent, Recipients};

/// Threads in a page when the caller names no limit.
pub const THREAD_PAGE_DEFAULT: usize = 50;
/// The largest page, and the most threads one bulk change takes.
pub const THREAD_PAGE_MAX: usize = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum MailFolder {
    Inbox,
    Archive,
    Spam,
    /// Purged 30 days after it was trashed.
    Trash,
}

impl MailFolder {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Inbox => "inbox",
            Self::Archive => "archive",
            Self::Spam => "spam",
            Self::Trash => "trash",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "inbox" => Some(Self::Inbox),
            "archive" => Some(Self::Archive),
            "spam" => Some(Self::Spam),
            "trash" => Some(Self::Trash),
            _ => None,
        }
    }
}

/// A name and an address from a mail header.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct MailPerson {
    pub name: Option<String>,
    /// Lower case.
    pub address: String,
}

/// A row of the thread list, as the caller sees it (`unread` and `starred` are the caller's own).
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct MailThreadSummary {
    #[schema(value_type = String)]
    pub id: Id,
    #[schema(value_type = String)]
    pub mailbox_id: Id,
    pub subject: String,
    pub snippet: String,
    /// The distinct senders, first sender first (at most 5).
    pub from: Vec<MailPerson>,
    #[schema(value_type = String, format = DateTime)]
    pub last_message_at: TimestampMillis,
    pub message_count: i64,
    /// A message arrived after the caller last read the thread.
    pub unread: bool,
    pub starred: bool,
    /// Label ids, by label name.
    #[schema(value_type = Vec<String>)]
    pub labels: Vec<Id>,
    /// A message has an attachment that is not an inline image.
    pub has_attachments: bool,
    pub folder: MailFolder,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct MailThreadPage {
    pub items: Vec<MailThreadSummary>,
    /// Pass as `cursor` for the next page; null on the last one.
    #[schema(required = true)]
    pub cursor: Option<String>,
}

/// An SPF, DKIM or DMARC result.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum MailAuthResult {
    Pass,
    Fail,
    Softfail,
    Neutral,
    None,
    Temperror,
    Permerror,
}

impl From<Verdict> for MailAuthResult {
    fn from(verdict: Verdict) -> Self {
        match verdict {
            Verdict::Pass => Self::Pass,
            Verdict::Fail => Self::Fail,
            Verdict::SoftFail => Self::Softfail,
            Verdict::Neutral => Self::Neutral,
            Verdict::None => Self::None,
            Verdict::TempError => Self::Temperror,
            Verdict::PermError => Self::Permerror,
        }
    }
}

impl From<DmarcVerdict> for MailAuthResult {
    fn from(verdict: DmarcVerdict) -> Self {
        match verdict {
            DmarcVerdict::Pass => Self::Pass,
            DmarcVerdict::Fail { .. } => Self::Fail,
            DmarcVerdict::None => Self::None,
            DmarcVerdict::TempError => Self::Temperror,
        }
    }
}

/// What the checks found when the message arrived.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct MailAuthSummary {
    pub spf: MailAuthResult,
    pub dkim: MailAuthResult,
    /// `pass`, `fail`, `none` or `temperror`.
    pub dmarc: MailAuthResult,
    /// The `From` domain that DMARC proved (only when it passed).
    pub aligned_domain: Option<String>,
}

impl MailAuthSummary {
    fn of(auth: &str) -> Self {
        match serde_json::from_str::<AuthOutcome>(auth) {
            Ok(outcome) => Self {
                spf: outcome.spf.into(),
                dkim: outcome.dkim.into(),
                dmarc: outcome.dmarc.into(),
                aligned_domain: outcome.aligned_domain,
            },
            Err(_) => Self {
                spf: MailAuthResult::None,
                dkim: MailAuthResult::None,
                dmarc: MailAuthResult::None,
                aligned_domain: None,
            },
        }
    }
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct MailAttachmentView {
    #[schema(value_type = String)]
    pub id: Id,
    pub filename: Option<String>,
    /// The type detected from the bytes (served).
    pub content_type: String,
    pub size: i64,
    /// An inline part (an image in the HTML body).
    pub inline: bool,
    /// Without `<>`.
    pub content_id: Option<String>,
}

/// A message of a thread.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct MailMessageView {
    #[schema(value_type = String)]
    pub id: Id,
    pub from: Option<MailPerson>,
    /// The `Sender` address, when it differs from `From`.
    pub sender: Option<String>,
    pub reply_to: Vec<MailPerson>,
    pub to: Vec<MailPerson>,
    pub cc: Vec<MailPerson>,
    pub subject: String,
    /// The `Date` header, when it parsed.
    #[schema(value_type = Option<String>, format = DateTime)]
    pub sent_at: Option<TimestampMillis>,
    #[schema(value_type = String, format = DateTime)]
    pub received_at: TimestampMillis,
    pub body_text: String,
    /// The message has an HTML body (shown in the sandboxed frame from `view-url`).
    pub has_html: bool,
    pub attachments: Vec<MailAttachmentView>,
    pub auth: MailAuthSummary,
    /// The sender's domain is not a domain of the workspace: the **External** banner.
    pub from_external: bool,
    /// Came from outside the workspace into a thread that started inside it.
    pub external: bool,
    /// Remote images load at once: the caller chose it for this sender and DMARC passed.
    pub load_images_default: bool,
    /// The display name of the member whose name the sender uses with an address that is not
    /// theirs: the **Lookalike name** banner.
    pub lookalike_member: Option<String>,
    /// Why the message shows as plain text only (`parse_failed`, `limit`).
    pub warning: Option<String>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct MailThreadDetail {
    pub thread: MailThreadSummary,
    /// Oldest first.
    pub messages: Vec<MailMessageView>,
}

/// Unread threads of a mailbox in each folder, for the caller.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct MailboxUnread {
    #[schema(value_type = String)]
    pub mailbox_id: Id,
    pub inbox: i64,
    pub archive: i64,
    pub spam: i64,
    pub trash: i64,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct MailCounts {
    /// The mailboxes the caller reads, by name.
    pub mailboxes: Vec<MailboxUnread>,
}

/// What the mail app needs at start: the caller's mailboxes, the labels and whether the caller
/// manages mail.
#[derive(Clone, Debug)]
pub struct MailOverview {
    /// Each with its unread inbox threads.
    pub mailboxes: Vec<(Mailbox, i64)>,
    pub labels: Vec<Label>,
    pub can_manage: bool,
}

/// What the caller asks for in a thread list.
#[derive(Clone, Debug, Default)]
pub struct ThreadListInput {
    /// One mailbox; all the caller reads when `None`.
    pub mailbox_id: Option<Id>,
    /// Without a folder: the inbox, or with `label_id` or `starred` every folder but Spam and
    /// Trash.
    pub folder: Option<MailFolder>,
    pub label_id: Option<Id>,
    pub starred: bool,
    pub cursor: Option<String>,
    pub limit: Option<usize>,
}

/// A change to threads; `None` and empty lists keep the value.
#[derive(Clone, Debug, Default)]
pub struct ThreadChange {
    /// Moving to Trash keeps the folder to restore; moving out of it clears the trash time.
    pub folder: Option<MailFolder>,
    /// Back from Trash to the folder it came from (the inbox when unknown).
    pub restore: bool,
    pub read: Option<bool>,
    pub starred: Option<bool>,
    pub add_labels: Vec<Id>,
    pub remove_labels: Vec<Id>,
}

/// The caller as a member of the workspace.
struct Reader {
    membership_id: Id,
    actor: Actor,
}

async fn reader(
    transaction: &mut Transaction<'_, Any>,
    workspace_id: Id,
    user_id: Id,
) -> Result<Reader, MailError> {
    let row = sqlx::query(
        "SELECT ms.id, ms.role FROM memberships ms JOIN workspaces w ON w.id = ms.workspace_id \
         WHERE ms.workspace_id = $1 AND ms.user_id = $2 AND w.deleted_at IS NULL",
    )
    .bind(workspace_id.to_string())
    .bind(user_id.to_string())
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(MailError::NotFound)?;
    let role: String = row.try_get("role")?;
    Ok(Reader {
        membership_id: parse_id(&row.try_get::<String, _>("id")?)?,
        actor: Actor {
            user_id,
            role: WorkspaceRole::parse(&role).ok_or(MailError::NotFound)?,
        },
    })
}

/// `NotFound` unless the membership reads the mailbox of the workspace.
async fn check_mailbox(
    transaction: &mut Transaction<'_, Any>,
    workspace_id: Id,
    membership_id: Id,
    mailbox_id: Id,
) -> Result<(), MailError> {
    let found: Option<String> = sqlx::query_scalar(
        "SELECT mb.id FROM mailboxes mb JOIN mailbox_members mm ON mm.mailbox_id = mb.id \
         WHERE mb.id = $1 AND mb.workspace_id = $2 AND mm.membership_id = $3",
    )
    .bind(mailbox_id.to_string())
    .bind(workspace_id.to_string())
    .bind(membership_id.to_string())
    .fetch_optional(&mut **transaction)
    .await?;
    found.map(|_| ()).ok_or(MailError::NotFound)
}

fn page_limit(limit: Option<usize>) -> Result<usize, MailError> {
    match limit {
        None => Ok(THREAD_PAGE_DEFAULT),
        Some(limit) if (1..=THREAD_PAGE_MAX).contains(&limit) => Ok(limit),
        Some(_) => Err(MailError::Invalid { field: "limit" }),
    }
}

fn page(threads: Vec<MailThreadSummary>, more: bool) -> MailThreadPage {
    let cursor = threads.last().filter(|_| more).map(|thread| {
        ThreadCursor {
            last_message_at: thread.last_message_at.as_millis(),
            id: thread.id,
        }
        .encode()
    });
    MailThreadPage {
        items: threads,
        cursor,
    }
}

/// The member a display name imitates: `name` equals a member's display name (trimmed, any
/// case) and `address` is none of that member's addresses (login email, addresses of the
/// mailboxes they read).
async fn lookalike(
    transaction: &mut Transaction<'_, Any>,
    members: &[(Id, String, String)],
    name: &str,
    address: &str,
) -> Result<Option<String>, MailError> {
    let wanted = name.trim().to_lowercase();
    if wanted.is_empty() {
        return Ok(None);
    }
    let mut imitated = None;
    for (membership_id, display_name, email) in members {
        if display_name.trim().to_lowercase() != wanted {
            continue;
        }
        if email.eq_ignore_ascii_case(address) {
            return Ok(None);
        }
        let addresses: Vec<(String, String)> = sqlx::query_as(
            "SELECT a.local_part, d.name FROM mailbox_members mm \
             JOIN mail_addresses a ON a.mailbox_id = mm.mailbox_id \
             JOIN mail_domains d ON d.id = a.domain_id WHERE mm.membership_id = $1",
        )
        .bind(membership_id.to_string())
        .fetch_all(&mut **transaction)
        .await?;
        if addresses
            .iter()
            .any(|(local, domain)| format!("{local}@{domain}").eq_ignore_ascii_case(address))
        {
            return Ok(None);
        }
        imitated.get_or_insert_with(|| display_name.trim().to_owned());
    }
    Ok(imitated)
}

impl MailRepository {
    fn postgres(&self) -> bool {
        self.database.backend() == Backend::Postgres
    }

    /// The thread list (spec §7): one mailbox the caller reads, or all of them.
    pub async fn list_threads(
        &self,
        workspace_id: Id,
        user_id: Id,
        input: ThreadListInput,
    ) -> Result<MailThreadPage, MailError> {
        let limit = page_limit(input.limit)?;
        let cursor = input
            .cursor
            .as_deref()
            .map(ThreadCursor::parse)
            .transpose()?;
        let mut transaction = self.database.pool().begin().await?;
        let reader = reader(&mut transaction, workspace_id, user_id).await?;
        if let Some(mailbox_id) = input.mailbox_id {
            check_mailbox(
                &mut transaction,
                workspace_id,
                reader.membership_id,
                mailbox_id,
            )
            .await?;
        }
        let folders = match input.folder {
            Some(folder) => Folders::One(folder),
            None if input.label_id.is_some() || input.starred => Folders::Kept,
            None => Folders::One(MailFolder::Inbox),
        };
        let mut selection = Selection::new(folders);
        selection.mailbox_id = input.mailbox_id;
        selection.label_id = input.label_id;
        selection.query.starred = input.starred;
        let (threads, more) = select_threads(
            &mut transaction,
            self.postgres(),
            workspace_id,
            reader.membership_id,
            &selection,
            cursor,
            limit,
        )
        .await?;
        transaction.commit().await?;
        Ok(page(threads, more))
    }

    /// Search (spec §8.3) over the mailboxes the caller reads; an empty page when `q` names
    /// nothing to look for.
    pub async fn search_threads(
        &self,
        workspace_id: Id,
        user_id: Id,
        query: MailQuery,
        cursor: Option<&str>,
        limit: Option<usize>,
    ) -> Result<MailThreadPage, MailError> {
        let limit = page_limit(limit)?;
        let cursor = cursor.map(ThreadCursor::parse).transpose()?;
        let mut transaction = self.database.pool().begin().await?;
        let reader = reader(&mut transaction, workspace_id, user_id).await?;
        let Some(selection) = Selection::search(query, self.postgres()) else {
            return Ok(page(Vec::new(), false));
        };
        let (threads, more) = select_threads(
            &mut transaction,
            self.postgres(),
            workspace_id,
            reader.membership_id,
            &selection,
            cursor,
            limit,
        )
        .await?;
        transaction.commit().await?;
        Ok(page(threads, more))
    }

    /// A thread with its messages, oldest first.
    pub async fn get_thread(
        &self,
        workspace_id: Id,
        user_id: Id,
        thread_id: Id,
    ) -> Result<MailThreadDetail, MailError> {
        let mut transaction = self.database.pool().begin().await?;
        let reader = reader(&mut transaction, workspace_id, user_id).await?;
        let mut selection = Selection::new(Folders::Any);
        selection.thread_ids = Some(vec![thread_id]);
        let thread = select_threads(
            &mut transaction,
            self.postgres(),
            workspace_id,
            reader.membership_id,
            &selection,
            None,
            1,
        )
        .await?
        .0
        .pop()
        .ok_or(MailError::NotFound)?;

        let rows = sqlx::query(
            "SELECT m.id, m.from_name, m.from_addr, m.sender, m.reply_to, m.to_addrs, m.cc_addrs, \
             m.subject, m.sent_at, m.received_at, m.body_text, m.html_part, m.warning, \
             tm.external, r.auth FROM mail_thread_messages tm \
             JOIN mail_messages m ON m.id = tm.message_id \
             JOIN mail_receipts r ON r.id = m.receipt_id \
             WHERE tm.thread_id = $1 ORDER BY m.received_at, m.id",
        )
        .bind(thread_id.to_string())
        .fetch_all(&mut *transaction)
        .await?;
        let mut attachments: HashMap<String, Vec<MailAttachmentView>> = HashMap::new();
        for row in sqlx::query(
            "SELECT a.id, a.message_id, a.filename, a.sniffed_type, a.size, a.content_id, \
             a.disposition FROM mail_attachments a \
             JOIN mail_thread_messages tm ON tm.message_id = a.message_id \
             WHERE tm.thread_id = $1 ORDER BY a.part_id",
        )
        .bind(thread_id.to_string())
        .fetch_all(&mut *transaction)
        .await?
        {
            let disposition: String = row.try_get("disposition")?;
            attachments
                .entry(row.try_get("message_id")?)
                .or_default()
                .push(MailAttachmentView {
                    id: parse_id(&row.try_get::<String, _>("id")?)?,
                    filename: row.try_get("filename")?,
                    content_type: row.try_get("sniffed_type")?,
                    size: row.try_get("size")?,
                    inline: disposition == "inline",
                    content_id: row.try_get("content_id")?,
                });
        }
        let domains: Vec<String> = sqlx::query_scalar(
            "SELECT name FROM mail_domains WHERE workspace_id = $1 \
             AND status IN ('verified', 'suspended')",
        )
        .bind(workspace_id.to_string())
        .fetch_all(&mut *transaction)
        .await?;
        let senders: BTreeSet<String> = rows
            .iter()
            .filter_map(|row| row.try_get::<Option<String>, _>("from_addr").ok().flatten())
            .collect();
        let mut load_images = BTreeSet::new();
        if !senders.is_empty() {
            let mut prefs = SqlBuilder::new(
                "SELECT address FROM mail_sender_prefs WHERE load_images = 1 AND membership_id = ",
            );
            prefs
                .push_bind(reader.membership_id.to_string())
                .push(" AND address IN (");
            let mut list = prefs.separated(", ");
            for sender in &senders {
                list.push_bind(sender.clone());
            }
            prefs.push(")");
            for row in prefs.build().fetch_all(&mut *transaction).await? {
                load_images.insert(row.try_get::<String, _>("address")?);
            }
        }
        let members: Vec<(Id, String, String)> = sqlx::query_as::<_, (String, String, String)>(
            "SELECT ms.id, u.display_name, u.email FROM memberships ms \
             JOIN users u ON u.id = ms.user_id WHERE ms.workspace_id = $1 ORDER BY ms.created_at, ms.id",
        )
        .bind(workspace_id.to_string())
        .fetch_all(&mut *transaction)
        .await?
        .into_iter()
        .map(|(id, name, email)| Ok((parse_id(&id)?, name, email)))
        .collect::<Result<_, MailError>>()?;

        let mut messages = Vec::with_capacity(rows.len());
        for row in &rows {
            let id: String = row.try_get("id")?;
            let name: Option<String> = row.try_get("from_name")?;
            let address: Option<String> = row.try_get("from_addr")?;
            let auth = MailAuthSummary::of(&row.try_get::<String, _>("auth")?);
            let lookalike_member = match (&name, &address) {
                (Some(name), Some(address)) => {
                    lookalike(&mut transaction, &members, name, address).await?
                }
                _ => None,
            };
            let sender: Option<String> = row.try_get("sender")?;
            messages.push(MailMessageView {
                from_external: !address
                    .as_deref()
                    .is_some_and(|address| is_inside(address, &domains)),
                load_images_default: auth.dmarc == MailAuthResult::Pass
                    && address
                        .as_ref()
                        .is_some_and(|address| load_images.contains(address)),
                sender: sender.filter(|sender| Some(sender) != address.as_ref()),
                from: address.map(|address| MailPerson { name, address }),
                reply_to: people(&row.try_get::<String, _>("reply_to")?),
                to: people(&row.try_get::<String, _>("to_addrs")?),
                cc: people(&row.try_get::<String, _>("cc_addrs")?),
                subject: row.try_get("subject")?,
                sent_at: row
                    .try_get::<Option<i64>, _>("sent_at")?
                    .map(TimestampMillis::from_millis),
                received_at: TimestampMillis::from_millis(row.try_get("received_at")?),
                body_text: row.try_get("body_text")?,
                has_html: row.try_get::<Option<i64>, _>("html_part")?.is_some(),
                attachments: attachments.remove(&id).unwrap_or_default(),
                auth,
                external: row.try_get("external")?,
                lookalike_member,
                warning: row.try_get("warning")?,
                id: parse_id(&id)?,
            });
        }
        transaction.commit().await?;
        Ok(MailThreadDetail { thread, messages })
    }

    /// Changes one thread the caller reads; `NotFound` for any other.
    pub async fn update_thread(
        &self,
        workspace_id: Id,
        user_id: Id,
        thread_id: Id,
        change: &ThreadChange,
    ) -> Result<MailThreadSummary, MailError> {
        self.update_threads(workspace_id, user_id, &[thread_id], change)
            .await?
            .pop()
            .ok_or(MailError::NotFound)
    }

    /// Changes up to [`THREAD_PAGE_MAX`] threads at once; threads the caller does not read are
    /// left alone. Returns the changed threads, newest first.
    pub async fn update_threads(
        &self,
        workspace_id: Id,
        user_id: Id,
        thread_ids: &[Id],
        change: &ThreadChange,
    ) -> Result<Vec<MailThreadSummary>, MailError> {
        if thread_ids.is_empty() || thread_ids.len() > THREAD_PAGE_MAX {
            return Err(MailError::Invalid { field: "ids" });
        }
        if change.restore && change.folder.is_some() {
            return Err(MailError::Invalid { field: "restore" });
        }
        let now = self.database.database_now().await?;
        let mut transaction = self.database.immediate_transaction().await?;
        let reader = reader(&mut transaction, workspace_id, user_id).await?;
        if !change.add_labels.is_empty() {
            let wanted: BTreeSet<Id> = change.add_labels.iter().copied().collect();
            let mut sql = SqlBuilder::new("SELECT id FROM mail_labels WHERE workspace_id = ");
            sql.push_bind(workspace_id.to_string()).push(" AND id IN (");
            let mut list = sql.separated(", ");
            for id in &wanted {
                list.push_bind(id.to_string());
            }
            sql.push(")");
            if sql.build().fetch_all(&mut *transaction).await?.len() != wanted.len() {
                return Err(MailError::Invalid {
                    field: "add_labels",
                });
            }
        }

        let mut changed = Vec::new();
        let mut mailboxes = BTreeSet::new();
        for thread_id in thread_ids.iter().collect::<BTreeSet<_>>() {
            let Some(row) = sqlx::query(
                "SELECT t.mailbox_id, t.folder, t.last_message_at FROM mail_threads t \
                 JOIN mailbox_members mm ON mm.mailbox_id = t.mailbox_id AND mm.membership_id = $1 \
                 WHERE t.id = $2 AND t.workspace_id = $3",
            )
            .bind(reader.membership_id.to_string())
            .bind(thread_id.to_string())
            .bind(workspace_id.to_string())
            .fetch_optional(&mut *transaction)
            .await?
            else {
                continue;
            };
            let folder: String = row.try_get("folder")?;
            let last_message_at: i64 = row.try_get("last_message_at")?;
            let id = thread_id.to_string();
            move_thread(&mut transaction, &id, &folder, change, now).await?;
            if let Some(read) = change.read {
                sqlx::query(
                    "INSERT INTO mail_thread_user_state (thread_id, membership_id, last_read_at, starred) \
                     VALUES ($1, $2, $3, 0) ON CONFLICT (thread_id, membership_id) \
                     DO UPDATE SET last_read_at = excluded.last_read_at",
                )
                .bind(&id)
                .bind(reader.membership_id.to_string())
                .bind(read.then_some(last_message_at))
                .execute(&mut *transaction)
                .await?;
            }
            if let Some(starred) = change.starred {
                sqlx::query(
                    "INSERT INTO mail_thread_user_state (thread_id, membership_id, last_read_at, starred) \
                     VALUES ($1, $2, NULL, $3) ON CONFLICT (thread_id, membership_id) \
                     DO UPDATE SET starred = excluded.starred",
                )
                .bind(&id)
                .bind(reader.membership_id.to_string())
                .bind(i64::from(starred))
                .execute(&mut *transaction)
                .await?;
            }
            for label_id in &change.add_labels {
                sqlx::query(
                    "INSERT INTO mail_thread_labels (thread_id, label_id) VALUES ($1, $2) \
                     ON CONFLICT (thread_id, label_id) DO NOTHING",
                )
                .bind(&id)
                .bind(label_id.to_string())
                .execute(&mut *transaction)
                .await?;
            }
            for label_id in &change.remove_labels {
                sqlx::query(
                    "DELETE FROM mail_thread_labels WHERE thread_id = $1 AND label_id = $2",
                )
                .bind(&id)
                .bind(label_id.to_string())
                .execute(&mut *transaction)
                .await?;
            }
            mailboxes.insert(row.try_get::<String, _>("mailbox_id")?);
            changed.push(*thread_id);
        }

        let mut selection = Selection::new(Folders::Any);
        selection.thread_ids = Some(changed);
        let (threads, _) = select_threads(
            &mut transaction,
            self.postgres(),
            workspace_id,
            reader.membership_id,
            &selection,
            None,
            THREAD_PAGE_MAX,
        )
        .await?;
        let mut users = HashMap::new();
        for mailbox_id in &mailboxes {
            let mailbox_id = parse_id(mailbox_id)?;
            users.insert(
                mailbox_id,
                mailbox_users(&mut transaction, mailbox_id).await?,
            );
        }
        transaction.commit().await?;

        let events: Vec<NewEvent> = threads
            .iter()
            .filter_map(|thread| {
                let users = users.get(&thread.mailbox_id)?.clone();
                (!users.is_empty()).then(|| NewEvent {
                    recipients: Recipients::Users(users),
                    topic: MAIL_TOPIC,
                    event: json!({
                        "type": "thread.updated",
                        "mailbox_id": thread.mailbox_id.to_string(),
                        "thread_id": thread.id.to_string(),
                    }),
                })
            })
            .collect();
        if !events.is_empty()
            && let Err(error) = LiveHub::of(&self.database)
                .publish(workspace_id, &events)
                .await
        {
            // The change is stored; the clients see it on their next refresh.
            tracing::warn!(error = %error, "mail events for the live socket");
        }
        Ok(threads)
    }

    /// Unread threads in each folder of each mailbox the caller reads.
    pub async fn counts(&self, workspace_id: Id, user_id: Id) -> Result<MailCounts, MailError> {
        let mut transaction = self.database.pool().begin().await?;
        let reader = reader(&mut transaction, workspace_id, user_id).await?;
        let mailboxes = unread_counts(&mut transaction, workspace_id, reader.membership_id).await?;
        transaction.commit().await?;
        Ok(MailCounts { mailboxes })
    }

    /// The caller's mailboxes with their unread inbox threads, the labels, and whether the caller
    /// manages mail.
    pub async fn overview(&self, workspace_id: Id, user_id: Id) -> Result<MailOverview, MailError> {
        let mut transaction = self.database.pool().begin().await?;
        let reader = reader(&mut transaction, workspace_id, user_id).await?;
        let counts = unread_counts(&mut transaction, workspace_id, reader.membership_id).await?;
        let mailboxes = load_mailboxes(&mut transaction, workspace_id, &Scope::MemberOf(user_id))
            .await?
            .into_iter()
            .map(|mailbox| {
                let unread = counts
                    .iter()
                    .find(|count| count.mailbox_id == mailbox.id)
                    .map_or(0, |count| count.inbox);
                (mailbox, unread)
            })
            .collect();
        let labels = load_labels(&mut transaction, workspace_id).await?;
        transaction.commit().await?;
        Ok(MailOverview {
            mailboxes,
            labels,
            can_manage: reader.actor.can(Permission::MailManage),
        })
    }
}

/// Applies the folder part of `change` to a thread now in `folder`.
async fn move_thread(
    transaction: &mut Transaction<'_, Any>,
    thread_id: &str,
    folder: &str,
    change: &ThreadChange,
    now: TimestampMillis,
) -> Result<(), MailError> {
    let target = match change.folder {
        Some(target) => target,
        None if change.restore && folder == "trash" => {
            sqlx::query(
                "UPDATE mail_threads SET folder = COALESCE(restore_folder, 'inbox'), \
                 trashed_at = NULL, restore_folder = NULL, updated_at = $1 WHERE id = $2",
            )
            .bind(now.as_millis())
            .bind(thread_id)
            .execute(&mut **transaction)
            .await?;
            return Ok(());
        }
        None => return Ok(()),
    };
    if target.as_str() == folder {
        return Ok(());
    }
    let query = match target {
        // Trash keeps the folder it came from (and a spam reason) for Restore.
        MailFolder::Trash => sqlx::query(
            "UPDATE mail_threads SET folder = 'trash', trashed_at = $1, restore_folder = folder, \
             updated_at = $2 WHERE id = $3",
        )
        .bind(now.as_millis()),
        MailFolder::Spam => sqlx::query(
            "UPDATE mail_threads SET folder = $1, trashed_at = NULL, restore_folder = NULL, \
             updated_at = $2 WHERE id = $3",
        )
        .bind(target.as_str()),
        MailFolder::Inbox | MailFolder::Archive => sqlx::query(
            "UPDATE mail_threads SET folder = $1, trashed_at = NULL, restore_folder = NULL, \
             spam_reason = NULL, updated_at = $2 WHERE id = $3",
        )
        .bind(target.as_str()),
    };
    query
        .bind(now.as_millis())
        .bind(thread_id)
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

async fn unread_counts(
    transaction: &mut Transaction<'_, Any>,
    workspace_id: Id,
    membership_id: Id,
) -> Result<Vec<MailboxUnread>, MailError> {
    let mailboxes: Vec<String> = sqlx::query_scalar(
        "SELECT mb.id FROM mailboxes mb JOIN mailbox_members mm ON mm.mailbox_id = mb.id \
         WHERE mb.workspace_id = $1 AND mm.membership_id = $2 ORDER BY lower(mb.name), mb.id",
    )
    .bind(workspace_id.to_string())
    .bind(membership_id.to_string())
    .fetch_all(&mut **transaction)
    .await?;
    let rows = sqlx::query(
        "SELECT t.mailbox_id, t.folder, CAST(COUNT(*) AS BIGINT) AS unread FROM mail_threads t \
         JOIN mailbox_members mm ON mm.mailbox_id = t.mailbox_id AND mm.membership_id = $1 \
         LEFT JOIN mail_thread_user_state s ON s.thread_id = t.id AND s.membership_id = $2 \
         WHERE t.workspace_id = $3 AND (s.last_read_at IS NULL OR s.last_read_at < t.last_message_at) \
         GROUP BY t.mailbox_id, t.folder",
    )
    .bind(membership_id.to_string())
    .bind(membership_id.to_string())
    .bind(workspace_id.to_string())
    .fetch_all(&mut **transaction)
    .await?;
    let mut counts: HashMap<(String, String), i64> = HashMap::new();
    for row in rows {
        counts.insert(
            (row.try_get("mailbox_id")?, row.try_get("folder")?),
            row.try_get("unread")?,
        );
    }
    mailboxes
        .into_iter()
        .map(|mailbox_id| {
            let count = |folder: &str| {
                counts
                    .get(&(mailbox_id.clone(), folder.to_owned()))
                    .copied()
                    .unwrap_or(0)
            };
            Ok(MailboxUnread {
                inbox: count("inbox"),
                archive: count("archive"),
                spam: count("spam"),
                trash: count("trash"),
                mailbox_id: parse_id(&mailbox_id)?,
            })
        })
        .collect()
}
