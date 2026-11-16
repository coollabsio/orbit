//! Drafts (spec §7, D13): a member's unsent messages, private to the author. A reply or forward
//! draft belongs to a thread, and a member has one draft per thread (`UNIQUE (thread_id,
//! membership_id)`); a new message has no thread and any number of drafts.
//!
//! A draft carries a `version`: a change must name the version it was made from, and a stale one
//! is `Conflict("draft_conflict")`. The recipients are stored as plain addresses and are checked
//! loosely here (the editor saves while the member types); the send checks them strictly.
//!
//! The files of a forward point at the attachments of the received message
//! (`source_attachment_id`); their `blob_id` is the blob of the raw message, and the bytes of the
//! part are read from it when the message is built. No bytes are copied here.

use std::collections::{BTreeSet, HashMap};

use orbit_platform::{BlobReader, Id, Row, StagedUpload, TimestampMillis};
use serde::{Deserialize, Serialize};
use sqlx::any::AnyRow;
use sqlx::{Any, Transaction};

use super::domains::parse_id;
use super::search::people;
use super::signatures::check_sendable;
use super::{MailError, MailRepository};

const BODY_MAX_BYTES: usize = 2_000_000;
const SUBJECT_MAX_CHARS: usize = 998;
const RECIPIENTS_MAX: usize = 100;
const ADDRESS_MAX_CHARS: usize = 320;
const FILES_MAX: i64 = 50;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DraftMode {
    New,
    Reply,
    ReplyAll,
    Forward,
    ForwardAttached,
}

impl DraftMode {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::New => "new",
            Self::Reply => "reply",
            Self::ReplyAll => "reply_all",
            Self::Forward => "forward",
            Self::ForwardAttached => "forward_attached",
        }
    }

    fn parse(value: &str) -> Result<Self, MailError> {
        Ok(match value {
            "new" => Self::New,
            "reply" => Self::Reply,
            "reply_all" => Self::ReplyAll,
            "forward" => Self::Forward,
            "forward_attached" => Self::ForwardAttached,
            _ => return Err(MailError::Invalid { field: "mode" }),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DraftFile {
    pub id: Id,
    pub filename: String,
    pub content_type: String,
    pub size: i64,
    pub content_id: Option<String>,
    /// Sent as a download link instead of an attachment.
    pub as_link: bool,
    /// The received attachment a forward copied this file from.
    pub source_attachment_id: Option<Id>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Draft {
    pub id: Id,
    pub mailbox_id: Id,
    pub from_address_id: Id,
    pub thread_id: Option<Id>,
    pub parent_message_id: Option<Id>,
    pub mode: DraftMode,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub subject: String,
    pub body_html: String,
    pub version: i64,
    pub files: Vec<DraftFile>,
    pub created_at: TimestampMillis,
    pub updated_at: TimestampMillis,
}

/// A new draft. A reply or forward names its `thread_id` (and optionally the message answered;
/// the newest of the thread when absent); a new message names its `from_address_id`. Fields that
/// are `None` start from the defaults of the mode.
#[derive(Clone, Debug)]
pub struct NewDraft {
    pub mode: DraftMode,
    pub thread_id: Option<Id>,
    pub parent_message_id: Option<Id>,
    pub from_address_id: Option<Id>,
    pub to: Option<Vec<String>>,
    pub cc: Option<Vec<String>>,
    pub bcc: Option<Vec<String>>,
    pub subject: Option<String>,
    pub body_html: Option<String>,
}

/// A change to a draft; `None` keeps the value. `version` is the one the change was made from.
#[derive(Clone, Debug, Default)]
pub struct DraftChange {
    pub version: i64,
    pub from_address_id: Option<Id>,
    pub mode: Option<DraftMode>,
    pub to: Option<Vec<String>>,
    pub cc: Option<Vec<String>>,
    pub bcc: Option<Vec<String>>,
    pub subject: Option<String>,
    pub body_html: Option<String>,
}

type Recipients = (
    Option<Vec<String>>,
    Option<Vec<String>>,
    Option<Vec<String>>,
);

fn list_json(list: &[String]) -> String {
    serde_json::to_string(list).unwrap_or_else(|_| "[]".to_owned())
}

fn list_of(json: &str) -> Vec<String> {
    serde_json::from_str(json).unwrap_or_default()
}

fn clean_list(list: &[String], field: &'static str) -> Result<Vec<String>, MailError> {
    if list.len() > RECIPIENTS_MAX {
        return Err(MailError::Invalid { field });
    }
    let mut cleaned = Vec::with_capacity(list.len());
    for entry in list {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        if entry.chars().count() > ADDRESS_MAX_CHARS || entry.chars().any(char::is_control) {
            return Err(MailError::Invalid { field });
        }
        cleaned.push(entry.to_owned());
    }
    Ok(cleaned)
}

fn clean_subject(subject: &str) -> Result<String, MailError> {
    let subject = subject.trim();
    if subject.chars().count() > SUBJECT_MAX_CHARS || subject.chars().any(char::is_control) {
        return Err(MailError::Invalid { field: "subject" });
    }
    Ok(subject.to_owned())
}

fn clean_body(body: &str) -> Result<String, MailError> {
    if body.len() > BODY_MAX_BYTES {
        return Err(MailError::Invalid { field: "body_html" });
    }
    Ok(orbit_mail::clean_outgoing_html(body))
}

/// `Re: x` / `Fwd: x`, without doubling a prefix the subject has already.
fn prefixed(subject: &str, mode: DraftMode) -> String {
    let subject = subject.trim();
    let lower = subject.to_ascii_lowercase();
    let (has, prefix) = match mode {
        DraftMode::New => return subject.to_owned(),
        DraftMode::Reply | DraftMode::ReplyAll => (lower.starts_with("re:"), "Re: "),
        DraftMode::Forward | DraftMode::ForwardAttached => (
            lower.starts_with("fwd:") || lower.starts_with("fw:"),
            "Fwd: ",
        ),
    };
    if has {
        subject.to_owned()
    } else {
        format!("{prefix}{subject}")
    }
}

/// The address with its `+tag` and case removed, to compare it with the addresses of a mailbox.
fn base_key(address: &str) -> String {
    let lower = address.trim().to_ascii_lowercase();
    match lower.rsplit_once('@') {
        Some((local, domain)) => format!("{}@{domain}", orbit_mail::base_local(local)),
        None => lower,
    }
}

/// Removes the addresses already in `seen` and the repeats (case-insensitive), keeping the order.
fn distinct(list: Vec<String>, seen: &mut BTreeSet<String>) -> Vec<String> {
    list.into_iter()
        .filter(|address| seen.insert(address.trim().to_ascii_lowercase()))
        .collect()
}

struct Parent {
    id: Id,
    from: Option<String>,
    reply_to: Vec<String>,
    to: Vec<String>,
    cc: Vec<String>,
    subject: String,
}

struct Defaults {
    to: Vec<String>,
    cc: Vec<String>,
    subject: String,
}

/// The recipients and the subject a mode starts with. `own` are the base keys of the mailbox's
/// addresses; they never appear as recipients.
fn defaults_for(mode: DraftMode, parent: &Parent, own: &BTreeSet<String>) -> Defaults {
    let from_self = parent
        .from
        .as_deref()
        .is_some_and(|from| own.contains(&base_key(from)));
    let target: Vec<String> = if from_self {
        parent.to.clone()
    } else if parent.reply_to.is_empty() {
        parent.from.iter().cloned().collect()
    } else {
        parent.reply_to.clone()
    };
    let (to, cc) = match mode {
        DraftMode::New | DraftMode::Forward | DraftMode::ForwardAttached => {
            (Vec::new(), Vec::new())
        }
        DraftMode::Reply => (target, Vec::new()),
        DraftMode::ReplyAll => {
            let mut to = target;
            if !from_self {
                to.extend(parent.to.iter().cloned());
            }
            (to, parent.cc.clone())
        }
    };
    let mut seen = BTreeSet::new();
    let strip = |list: Vec<String>| -> Vec<String> {
        list.into_iter()
            .filter(|address| !own.contains(&base_key(address)))
            .collect()
    };
    let to = distinct(strip(to), &mut seen);
    let cc = distinct(strip(cc), &mut seen);
    Defaults {
        to,
        cc,
        subject: prefixed(&parent.subject, mode),
    }
}

fn file(row: &AnyRow) -> Result<DraftFile, MailError> {
    Ok(DraftFile {
        id: parse_id(&row.try_get::<String, _>("id")?)?,
        filename: row.try_get("filename")?,
        content_type: row.try_get("content_type")?,
        size: row.try_get("size")?,
        content_id: row.try_get("content_id")?,
        as_link: row.try_get::<i64, _>("as_link")? != 0,
        source_attachment_id: row
            .try_get::<Option<String>, _>("source_attachment_id")?
            .map(|id| parse_id(&id))
            .transpose()?,
    })
}

fn draft(row: &AnyRow, files: Vec<DraftFile>) -> Result<Draft, MailError> {
    let optional = |name: &str| -> Result<Option<Id>, MailError> {
        row.try_get::<Option<String>, _>(name)?
            .map(|id| parse_id(&id))
            .transpose()
    };
    Ok(Draft {
        id: parse_id(&row.try_get::<String, _>("id")?)?,
        mailbox_id: parse_id(&row.try_get::<String, _>("mailbox_id")?)?,
        from_address_id: parse_id(&row.try_get::<String, _>("from_address_id")?)?,
        thread_id: optional("thread_id")?,
        parent_message_id: optional("parent_message_id")?,
        mode: DraftMode::parse(&row.try_get::<String, _>("mode")?)?,
        to: list_of(&row.try_get::<String, _>("to_addrs")?),
        cc: list_of(&row.try_get::<String, _>("cc_addrs")?),
        bcc: list_of(&row.try_get::<String, _>("bcc_addrs")?),
        subject: row.try_get("subject")?,
        body_html: row.try_get("body_html")?,
        version: row.try_get("version")?,
        files,
        created_at: TimestampMillis::from_millis(row.try_get("created_at")?),
        updated_at: TimestampMillis::from_millis(row.try_get("updated_at")?),
    })
}

const DRAFT_COLUMNS: &str = "id, mailbox_id, from_address_id, thread_id, parent_message_id, mode, \
     to_addrs, cc_addrs, bcc_addrs, subject, body_html, version, created_at, updated_at";
const FILE_COLUMNS: &str = "id, filename, content_type, size, content_id, as_link, \
     source_attachment_id";

/// The author's draft with its files; `NotFound` for another member's.
pub(super) async fn load_draft(
    transaction: &mut Transaction<'_, Any>,
    membership_id: Id,
    draft_id: Id,
) -> Result<Draft, MailError> {
    let row = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT {DRAFT_COLUMNS} FROM mail_drafts WHERE id = $1 AND membership_id = $2"
    )))
    .bind(draft_id.to_string())
    .bind(membership_id.to_string())
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(MailError::NotFound)?;
    let files = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT {FILE_COLUMNS} FROM mail_draft_files WHERE draft_id = $1 \
         ORDER BY created_at, id"
    )))
    .bind(draft_id.to_string())
    .fetch_all(&mut **transaction)
    .await?
    .iter()
    .map(file)
    .collect::<Result<_, _>>()?;
    draft(&row, files)
}

/// The base keys of every address of the mailbox (its aliases too).
async fn own_keys(
    transaction: &mut Transaction<'_, Any>,
    mailbox_id: Id,
) -> Result<BTreeSet<String>, MailError> {
    let rows = sqlx::query(
        "SELECT a.local_part, d.name AS domain FROM mail_addresses a \
         JOIN mail_domains d ON d.id = a.domain_id WHERE a.mailbox_id = $1",
    )
    .bind(mailbox_id.to_string())
    .fetch_all(&mut **transaction)
    .await?;
    rows.iter()
        .map(|row| {
            Ok(base_key(&format!(
                "{}@{}",
                row.try_get::<String, _>("local_part")?,
                row.try_get::<String, _>("domain")?
            )))
        })
        .collect()
}

/// A message of the thread: the one named, else the newest. `NotFound` when the thread has none.
async fn load_parent(
    transaction: &mut Transaction<'_, Any>,
    thread_id: Id,
    message_id: Option<Id>,
) -> Result<Parent, MailError> {
    let mut sql = orbit_platform::SqlBuilder::new(
        "SELECT m.id, m.from_addr, m.reply_to, m.to_addrs, m.cc_addrs, m.subject \
         FROM mail_thread_messages tm JOIN mail_messages m ON m.id = tm.message_id \
         WHERE tm.thread_id = ",
    );
    sql.push_bind(thread_id.to_string());
    if let Some(message) = message_id {
        sql.push(" AND m.id = ");
        sql.push_bind(message.to_string());
    }
    sql.push(" ORDER BY m.received_at DESC, m.id DESC LIMIT 1");
    let row = sql
        .build()
        .fetch_optional(&mut **transaction)
        .await?
        .ok_or(MailError::NotFound)?;
    parent_of(&row)
}

fn parent_of(row: &AnyRow) -> Result<Parent, MailError> {
    let addresses = |column: &str| -> Result<Vec<String>, MailError> {
        Ok(people(&row.try_get::<String, _>(column)?)
            .into_iter()
            .map(|person| person.address)
            .collect())
    };
    Ok(Parent {
        id: parse_id(&row.try_get::<String, _>("id")?)?,
        from: row.try_get("from_addr")?,
        reply_to: addresses("reply_to")?,
        to: addresses("to_addrs")?,
        cc: addresses("cc_addrs")?,
        subject: row.try_get("subject")?,
    })
}

/// Makes the forwarded files of a draft match its mode: a forward holds the attachments of the
/// message answered, any other mode none.
async fn sync_forward_files(
    transaction: &mut Transaction<'_, Any>,
    workspace_id: Id,
    draft_id: Id,
    parent_id: Option<Id>,
    mode: DraftMode,
    now: TimestampMillis,
) -> Result<(), MailError> {
    sqlx::query(
        "DELETE FROM mail_draft_files WHERE draft_id = $1 AND source_attachment_id IS NOT NULL",
    )
    .bind(draft_id.to_string())
    .execute(&mut **transaction)
    .await?;
    let (Some(parent_id), DraftMode::Forward) = (parent_id, mode) else {
        return Ok(());
    };
    let blob: Option<String> = sqlx::query_scalar(
        "SELECT r.blob_id FROM mail_messages m JOIN mail_receipts r ON r.id = m.receipt_id \
         WHERE m.id = $1",
    )
    .bind(parent_id.to_string())
    .fetch_optional(&mut **transaction)
    .await?;
    let Some(blob) = blob else {
        return Ok(());
    };
    let attachments = sqlx::query(
        "SELECT id, filename, sniffed_type, size, content_id FROM mail_attachments \
         WHERE message_id = $1 AND disposition = 'attachment' ORDER BY part_id",
    )
    .bind(parent_id.to_string())
    .fetch_all(&mut **transaction)
    .await?;
    for attachment in &attachments {
        let name: Option<String> = attachment.try_get("filename")?;
        let name: String = name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .unwrap_or("attachment")
            .chars()
            .take(255)
            .collect();
        sqlx::query(
            "INSERT INTO mail_draft_files \
             (id, draft_id, workspace_id, blob_id, filename, content_type, size, content_id, \
              as_link, source_attachment_id, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 0, $9, $10)",
        )
        .bind(Id::new_v7().to_string())
        .bind(draft_id.to_string())
        .bind(workspace_id.to_string())
        .bind(&blob)
        .bind(&name)
        .bind(attachment.try_get::<String, _>("sniffed_type")?)
        .bind(attachment.try_get::<i64, _>("size")?)
        .bind(attachment.try_get::<Option<String>, _>("content_id")?)
        .bind(attachment.try_get::<String, _>("id")?)
        .bind(now.as_millis())
        .execute(&mut **transaction)
        .await?;
    }
    Ok(())
}

async fn address_mailbox(
    transaction: &mut Transaction<'_, Any>,
    address_id: Id,
) -> Result<Id, MailError> {
    let found: Option<String> =
        sqlx::query_scalar("SELECT mailbox_id FROM mail_addresses WHERE id = $1")
            .bind(address_id.to_string())
            .fetch_optional(&mut **transaction)
            .await?;
    parse_id(&found.ok_or(MailError::NotFound)?)
}

impl MailRepository {
    /// A new draft of the member. For a reply or a forward (`thread_id`) the member must read the
    /// thread's mailbox; the recipients, the subject and (for a forward) the files start from the
    /// message answered, and the From is the address that received it (`reply_from`) or the
    /// mailbox's primary address. A member has one draft per thread: when there is one already,
    /// it is returned, and a different `mode` changes its mode (see `update_draft`); the other
    /// fields of `new` are then ignored. The flag is `true` when a draft was made. When another
    /// request made the thread's draft first, the result is `Conflict("draft_conflict")`.
    pub async fn create_draft(
        &self,
        membership_id: Id,
        workspace_id: Id,
        new: NewDraft,
    ) -> Result<(Draft, bool), MailError> {
        let to = new.to.as_deref().map(|l| clean_list(l, "to")).transpose()?;
        let cc = new.cc.as_deref().map(|l| clean_list(l, "cc")).transpose()?;
        let bcc = new
            .bcc
            .as_deref()
            .map(|l| clean_list(l, "bcc"))
            .transpose()?;
        let subject = new.subject.as_deref().map(clean_subject).transpose()?;
        let body = new.body_html.as_deref().map(clean_body).transpose()?;
        let Some(thread_id) = new.thread_id else {
            let created = self
                .create_new_draft(
                    membership_id,
                    workspace_id,
                    &new,
                    (to, cc, bcc),
                    subject,
                    body,
                )
                .await?;
            return Ok((created, true));
        };
        if new.mode == DraftMode::New {
            return Err(MailError::Invalid { field: "mode" });
        }
        let mailbox_id = self
            .thread_mailbox(workspace_id, membership_id, thread_id)
            .await?;
        let mut read = self.database.pool().begin().await?;
        let existing: Option<String> = sqlx::query_scalar(
            "SELECT id FROM mail_drafts WHERE thread_id = $1 AND membership_id = $2",
        )
        .bind(thread_id.to_string())
        .bind(membership_id.to_string())
        .fetch_optional(&mut *read)
        .await?;
        if let Some(existing) = existing {
            let sending: Option<String> = sqlx::query_scalar(
                "SELECT id FROM mail_outbound WHERE draft_id = $1 AND status = 'held'",
            )
            .bind(&existing)
            .fetch_optional(&mut *read)
            .await?;
            if let Some(outbound_id) = sending {
                return Err(MailError::DraftSending {
                    outbound_id: parse_id(&outbound_id)?,
                });
            }
            read.commit().await?;
            let existing = parse_id(&existing)?;
            let current = self.get_draft(membership_id, existing).await?;
            if current.mode == new.mode {
                return Ok((current, false));
            }
            let changed = self
                .update_draft(
                    membership_id,
                    existing,
                    DraftChange {
                        version: current.version,
                        mode: Some(new.mode),
                        ..DraftChange::default()
                    },
                )
                .await?;
            return Ok((changed, false));
        }
        let parent = load_parent(&mut read, thread_id, new.parent_message_id).await?;
        read.commit().await?;
        let from_address = match new.from_address_id {
            Some(address) => address,
            None => match self
                .reply_from(membership_id, parent.id, mailbox_id)
                .await?
            {
                Some(address) => address,
                None => self.primary_address(mailbox_id).await?,
            },
        };
        let now = self.database.database_now().await?;
        let mut transaction = self.database.immediate_transaction().await?;
        check_sendable(&mut transaction, workspace_id, membership_id, from_address).await?;
        if address_mailbox(&mut transaction, from_address).await? != mailbox_id {
            return Err(MailError::Invalid {
                field: "from_address_id",
            });
        }
        let taken: Option<String> = sqlx::query_scalar(
            "SELECT id FROM mail_drafts WHERE thread_id = $1 AND membership_id = $2",
        )
        .bind(thread_id.to_string())
        .bind(membership_id.to_string())
        .fetch_optional(&mut *transaction)
        .await?;
        if taken.is_some() {
            // Another request made the draft in between; the caller can ask again.
            return Err(MailError::Conflict("draft_conflict"));
        }
        let own = own_keys(&mut transaction, mailbox_id).await?;
        let defaults = defaults_for(new.mode, &parent, &own);
        let id = Id::new_v7();
        sqlx::query(
            "INSERT INTO mail_drafts \
             (id, workspace_id, membership_id, mailbox_id, from_address_id, thread_id, \
              parent_message_id, mode, to_addrs, cc_addrs, bcc_addrs, subject, body_html, \
              version, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, 1, $14, $14)",
        )
        .bind(id.to_string())
        .bind(workspace_id.to_string())
        .bind(membership_id.to_string())
        .bind(mailbox_id.to_string())
        .bind(from_address.to_string())
        .bind(thread_id.to_string())
        .bind(parent.id.to_string())
        .bind(new.mode.as_str())
        .bind(list_json(&to.unwrap_or(defaults.to)))
        .bind(list_json(&cc.unwrap_or(defaults.cc)))
        .bind(list_json(&bcc.unwrap_or_default()))
        .bind(subject.unwrap_or(defaults.subject))
        .bind(body.unwrap_or_default())
        .bind(now.as_millis())
        .execute(&mut *transaction)
        .await?;
        sync_forward_files(
            &mut transaction,
            workspace_id,
            id,
            Some(parent.id),
            new.mode,
            now,
        )
        .await?;
        let created = load_draft(&mut transaction, membership_id, id).await?;
        transaction.commit().await?;
        Ok((created, true))
    }

    /// The member's draft of a thread, if any.
    pub async fn thread_draft(
        &self,
        membership_id: Id,
        thread_id: Id,
    ) -> Result<Option<Draft>, MailError> {
        let found: Option<String> = sqlx::query_scalar(
            "SELECT id FROM mail_drafts WHERE thread_id = $1 AND membership_id = $2",
        )
        .bind(thread_id.to_string())
        .bind(membership_id.to_string())
        .fetch_optional(self.database.pool())
        .await?;
        match found {
            Some(id) => Ok(Some(self.get_draft(membership_id, parse_id(&id)?).await?)),
            None => Ok(None),
        }
    }

    async fn create_new_draft(
        &self,
        membership_id: Id,
        workspace_id: Id,
        new: &NewDraft,
        (to, cc, bcc): Recipients,
        subject: Option<String>,
        body: Option<String>,
    ) -> Result<Draft, MailError> {
        if new.mode != DraftMode::New || new.parent_message_id.is_some() {
            return Err(MailError::Invalid { field: "mode" });
        }
        let from_address = new.from_address_id.ok_or(MailError::Invalid {
            field: "from_address_id",
        })?;
        let now = self.database.database_now().await?;
        let mut transaction = self.database.immediate_transaction().await?;
        check_sendable(&mut transaction, workspace_id, membership_id, from_address).await?;
        let mailbox_id = address_mailbox(&mut transaction, from_address).await?;
        let id = Id::new_v7();
        sqlx::query(
            "INSERT INTO mail_drafts \
             (id, workspace_id, membership_id, mailbox_id, from_address_id, mode, to_addrs, \
              cc_addrs, bcc_addrs, subject, body_html, version, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, 'new', $6, $7, $8, $9, $10, 1, $11, $11)",
        )
        .bind(id.to_string())
        .bind(workspace_id.to_string())
        .bind(membership_id.to_string())
        .bind(mailbox_id.to_string())
        .bind(from_address.to_string())
        .bind(list_json(&to.unwrap_or_default()))
        .bind(list_json(&cc.unwrap_or_default()))
        .bind(list_json(&bcc.unwrap_or_default()))
        .bind(subject.unwrap_or_default())
        .bind(body.unwrap_or_default())
        .bind(now.as_millis())
        .execute(&mut *transaction)
        .await?;
        let created = load_draft(&mut transaction, membership_id, id).await?;
        transaction.commit().await?;
        Ok(created)
    }

    async fn primary_address(&self, mailbox_id: Id) -> Result<Id, MailError> {
        let found: Option<String> = sqlx::query_scalar(
            "SELECT id FROM mail_addresses WHERE mailbox_id = $1 \
             ORDER BY CASE kind WHEN 'primary' THEN 0 ELSE 1 END, local_part LIMIT 1",
        )
        .bind(mailbox_id.to_string())
        .fetch_optional(self.database.pool())
        .await?;
        parse_id(&found.ok_or(MailError::NotFound)?)
    }

    /// The member's draft; `NotFound` for another member's.
    pub async fn get_draft(&self, membership_id: Id, draft_id: Id) -> Result<Draft, MailError> {
        let mut transaction = self.database.pool().begin().await?;
        let found = load_draft(&mut transaction, membership_id, draft_id).await?;
        transaction.commit().await?;
        Ok(found)
    }

    /// The member's drafts, the most recently changed first; with `mailbox`, only that mailbox's.
    pub async fn list_drafts(
        &self,
        membership_id: Id,
        mailbox: Option<Id>,
    ) -> Result<Vec<Draft>, MailError> {
        let mut transaction = self.database.pool().begin().await?;
        let mut sql = orbit_platform::SqlBuilder::new(format!(
            "SELECT {DRAFT_COLUMNS} FROM mail_drafts WHERE NOT EXISTS (SELECT 1 FROM mail_outbound o \
             WHERE o.draft_id = mail_drafts.id AND o.status = 'held') AND membership_id = "
        ));
        sql.push_bind(membership_id.to_string());
        if let Some(mailbox) = mailbox {
            sql.push(" AND mailbox_id = ");
            sql.push_bind(mailbox.to_string());
        }
        sql.push(" ORDER BY updated_at DESC, id DESC");
        let rows = sql.build().fetch_all(&mut *transaction).await?;
        let file_rows = sqlx::query(
            "SELECT f.id, f.draft_id, f.filename, f.content_type, f.size, f.content_id, \
             f.as_link, f.source_attachment_id FROM mail_draft_files f \
             JOIN mail_drafts d ON d.id = f.draft_id WHERE d.membership_id = $1 \
             ORDER BY f.created_at, f.id",
        )
        .bind(membership_id.to_string())
        .fetch_all(&mut *transaction)
        .await?;
        transaction.commit().await?;
        let mut files: HashMap<String, Vec<DraftFile>> = HashMap::new();
        for row in &file_rows {
            files
                .entry(row.try_get("draft_id")?)
                .or_default()
                .push(file(row)?);
        }
        rows.iter()
            .map(|row| {
                let id: String = row.try_get("id")?;
                draft(row, files.remove(&id).unwrap_or_default())
            })
            .collect()
    }

    /// Changes a draft of the member. `Conflict("draft_conflict")` when `change.version` is not
    /// the current one. A new From must pass the same send-as check as on create (and, for a
    /// thread draft, be an address of the thread's mailbox, D13).
    ///
    /// A different `mode` (between reply, reply all and the forwards of a thread draft) gives the
    /// recipients (together) and the subject (on its own) of the new mode only while the member
    /// has not edited them (they still equal what the old mode made); the forwarded files follow the mode.
    pub async fn update_draft(
        &self,
        membership_id: Id,
        draft_id: Id,
        change: DraftChange,
    ) -> Result<Draft, MailError> {
        let to = change
            .to
            .as_deref()
            .map(|l| clean_list(l, "to"))
            .transpose()?;
        let cc = change
            .cc
            .as_deref()
            .map(|l| clean_list(l, "cc"))
            .transpose()?;
        let bcc = change
            .bcc
            .as_deref()
            .map(|l| clean_list(l, "bcc"))
            .transpose()?;
        let subject = change.subject.as_deref().map(clean_subject).transpose()?;
        let body = change.body_html.as_deref().map(clean_body).transpose()?;
        let now = self.database.database_now().await?;
        let mut transaction = self.database.immediate_transaction().await?;
        let current = load_draft(&mut transaction, membership_id, draft_id).await?;
        ensure_editable(&mut transaction, membership_id, draft_id).await?;
        if current.version != change.version {
            return Err(MailError::Conflict("draft_conflict"));
        }
        let workspace: String =
            sqlx::query_scalar("SELECT workspace_id FROM mail_drafts WHERE id = $1")
                .bind(draft_id.to_string())
                .fetch_one(&mut *transaction)
                .await?;
        let workspace_id = parse_id(&workspace)?;
        let mut mailbox_id = current.mailbox_id;
        let mut from_address = current.from_address_id;
        if let Some(address) = change.from_address_id
            && address != current.from_address_id
        {
            check_sendable(&mut transaction, workspace_id, membership_id, address).await?;
            let address_mailbox = address_mailbox(&mut transaction, address).await?;
            if current.thread_id.is_some() && address_mailbox != current.mailbox_id {
                return Err(MailError::Invalid {
                    field: "from_address_id",
                });
            }
            mailbox_id = address_mailbox;
            from_address = address;
        }
        let mut mode = current.mode;
        let mut new_to = current.to.clone();
        let mut new_cc = current.cc.clone();
        let mut new_subject = current.subject.clone();
        let mut sync_files = false;
        if let Some(requested) = change.mode
            && requested != current.mode
        {
            if current.thread_id.is_none() || requested == DraftMode::New {
                return Err(MailError::Invalid { field: "mode" });
            }
            if let (Some(thread_id), Some(parent_id)) =
                (current.thread_id, current.parent_message_id)
            {
                let parent = load_parent(&mut transaction, thread_id, Some(parent_id)).await?;
                let own = own_keys(&mut transaction, current.mailbox_id).await?;
                let old = defaults_for(current.mode, &parent, &own);
                let fresh = defaults_for(requested, &parent, &own);
                // The recipients move together; the subject on its own.
                if current.to == old.to && current.cc == old.cc {
                    new_to = fresh.to;
                    new_cc = fresh.cc;
                }
                if current.subject == old.subject {
                    new_subject = fresh.subject;
                }
            }
            mode = requested;
            // A draft whose parent is gone keeps its files: nothing to copy them from.
            sync_files = current.parent_message_id.is_some();
        }
        let result = sqlx::query(
            "UPDATE mail_drafts SET mailbox_id = $1, from_address_id = $2, mode = $3, \
             to_addrs = $4, cc_addrs = $5, bcc_addrs = $6, subject = $7, body_html = $8, \
             version = version + 1, updated_at = $9 \
             WHERE id = $10 AND membership_id = $11 AND version = $12",
        )
        .bind(mailbox_id.to_string())
        .bind(from_address.to_string())
        .bind(mode.as_str())
        .bind(list_json(&to.unwrap_or(new_to)))
        .bind(list_json(&cc.unwrap_or(new_cc)))
        .bind(list_json(&bcc.unwrap_or(current.bcc)))
        .bind(subject.unwrap_or(new_subject))
        .bind(body.unwrap_or(current.body_html))
        .bind(now.as_millis())
        .bind(draft_id.to_string())
        .bind(membership_id.to_string())
        .bind(change.version)
        .execute(&mut *transaction)
        .await?;
        if result.rows_affected() != 1 {
            return Err(MailError::Conflict("draft_conflict"));
        }
        if sync_files {
            sync_forward_files(
                &mut transaction,
                workspace_id,
                draft_id,
                current.parent_message_id,
                mode,
                now,
            )
            .await?;
        }
        let updated = load_draft(&mut transaction, membership_id, draft_id).await?;
        transaction.commit().await?;
        Ok(updated)
    }

    pub async fn delete_draft(&self, membership_id: Id, draft_id: Id) -> Result<(), MailError> {
        let mut transaction = self.database.immediate_transaction().await?;
        load_draft(&mut transaction, membership_id, draft_id).await?;
        ensure_editable(&mut transaction, membership_id, draft_id).await?;
        let result = sqlx::query("DELETE FROM mail_drafts WHERE id = $1 AND membership_id = $2")
            .bind(draft_id.to_string())
            .bind(membership_id.to_string())
            .execute(&mut *transaction)
            .await?;
        if result.rows_affected() == 0 {
            return Err(MailError::NotFound);
        }
        transaction.commit().await?;
        Ok(())
    }

    /// `NotFound` unless the draft is the member's; for a route that reads an upload next.
    pub async fn authorize_draft_file(
        &self,
        membership_id: Id,
        draft_id: Id,
    ) -> Result<(), MailError> {
        self.get_draft(membership_id, draft_id).await.map(|_| ())
    }

    /// Attaches a staged upload to the member's draft. The caller discards the upload when this
    /// fails. The upload's size limit (`ORBIT_UPLOAD_MAX_FILE_BYTES`) was checked while staging.
    pub async fn add_draft_file(
        &self,
        membership_id: Id,
        workspace_id: Id,
        draft_id: Id,
        upload: &StagedUpload,
        content_id: Option<&str>,
    ) -> Result<DraftFile, MailError> {
        let finalization = self.uploads.begin_finalization().await;
        let now = self.database.database_now().await?;
        let mut transaction = self.database.immediate_transaction().await?;
        let found: Option<String> = sqlx::query_scalar(
            "SELECT id FROM mail_drafts WHERE id = $1 AND membership_id = $2 AND workspace_id = $3",
        )
        .bind(draft_id.to_string())
        .bind(membership_id.to_string())
        .bind(workspace_id.to_string())
        .fetch_optional(&mut *transaction)
        .await?;
        found.ok_or(MailError::NotFound)?;
        ensure_editable(&mut transaction, membership_id, draft_id).await?;
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM mail_draft_files WHERE draft_id = $1")
                .bind(draft_id.to_string())
                .fetch_one(&mut *transaction)
                .await?;
        if count >= FILES_MAX {
            return Err(MailError::Unprocessable("draft_files_limit"));
        }
        let blob = finalization
            .finalize_blob_in_transaction(&mut transaction, upload, now)
            .await?;
        let id = Id::new_v7();
        let size =
            i64::try_from(upload.size_bytes).map_err(|_| MailError::Invalid { field: "file" })?;
        sqlx::query(
            "INSERT INTO mail_draft_files \
             (id, draft_id, workspace_id, blob_id, filename, content_type, size, content_id, \
              as_link, created_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 0, $9)",
        )
        .bind(id.to_string())
        .bind(draft_id.to_string())
        .bind(workspace_id.to_string())
        .bind(blob.id.to_string())
        .bind(&upload.display_name)
        .bind(&upload.detected_media_type)
        .bind(size)
        .bind(content_id)
        .bind(now.as_millis())
        .execute(&mut *transaction)
        .await?;
        touch(&mut transaction, draft_id, now).await?;
        transaction.commit().await?;
        Ok(DraftFile {
            id,
            filename: upload.display_name.clone(),
            content_type: upload.detected_media_type.clone(),
            size,
            content_id: content_id.map(str::to_owned),
            as_link: false,
            source_attachment_id: None,
        })
    }

    /// The stored bytes of an uploaded file of the member's draft, for the editor preview.
    /// `NotFound` for another member's draft, a file that is not on it and a forwarded file: that
    /// one points at the received message's sealed raw blob, which is never served as it is.
    pub async fn open_draft_file(
        &self,
        membership_id: Id,
        draft_id: Id,
        file_id: Id,
    ) -> Result<(DraftFile, BlobReader), MailError> {
        let row = sqlx::query(
            "SELECT f.id, f.filename, f.content_type, f.size, f.content_id, f.as_link, \
             f.source_attachment_id, b.storage_key \
             FROM mail_draft_files f \
             JOIN mail_drafts d ON d.id = f.draft_id \
             JOIN attachment_blobs b ON b.id = f.blob_id \
             WHERE f.id = $1 AND f.draft_id = $2 AND d.membership_id = $3 \
             AND f.source_attachment_id IS NULL AND b.deleted_at IS NULL",
        )
        .bind(file_id.to_string())
        .bind(draft_id.to_string())
        .bind(membership_id.to_string())
        .fetch_optional(self.database.pool())
        .await?
        .ok_or(MailError::NotFound)?;
        let reader = match self
            .uploads
            .open_blob(&row.try_get::<String, _>("storage_key")?)
            .await
        {
            Ok(reader) => reader,
            Err(error) if super::release::blob_missing(&error) => return Err(MailError::NotFound),
            Err(error) => return Err(error.into()),
        };
        Ok((file(&row)?, reader))
    }

    pub async fn remove_draft_file(
        &self,
        membership_id: Id,
        draft_id: Id,
        file_id: Id,
    ) -> Result<(), MailError> {
        let now = self.database.database_now().await?;
        let mut transaction = self.database.immediate_transaction().await?;
        ensure_editable(&mut transaction, membership_id, draft_id).await?;
        let result = sqlx::query(
            "DELETE FROM mail_draft_files WHERE id = $1 AND draft_id = $2 \
             AND draft_id IN (SELECT id FROM mail_drafts WHERE id = $2 AND membership_id = $3)",
        )
        .bind(file_id.to_string())
        .bind(draft_id.to_string())
        .bind(membership_id.to_string())
        .execute(&mut *transaction)
        .await?;
        if result.rows_affected() == 0 {
            return Err(MailError::NotFound);
        }
        touch(&mut transaction, draft_id, now).await?;
        transaction.commit().await?;
        Ok(())
    }

    /// Sends a file as a download link instead of an attachment, or back.
    pub async fn set_file_as_link(
        &self,
        membership_id: Id,
        draft_id: Id,
        file_id: Id,
        as_link: bool,
    ) -> Result<DraftFile, MailError> {
        let now = self.database.database_now().await?;
        let mut transaction = self.database.immediate_transaction().await?;
        ensure_editable(&mut transaction, membership_id, draft_id).await?;
        if as_link {
            // A forwarded file is a part of a received message, not an Orbit upload: it has no
            // file of its own to link to (it stays an attachment).
            let found: Option<(Option<String>, Option<String>)> = sqlx::query_as(
                "SELECT source_attachment_id, content_id FROM mail_draft_files \
                 WHERE id = $1 AND draft_id = $2",
            )
            .bind(file_id.to_string())
            .bind(draft_id.to_string())
            .fetch_optional(&mut *transaction)
            .await?;
            match found {
                Some((Some(_), _)) => return Err(MailError::Unprocessable("link_forwarded")),
                // An inline image is part of the body: it must travel with the message.
                Some((None, Some(id))) if !id.trim().is_empty() => {
                    return Err(MailError::Unprocessable("link_inline"));
                }
                _ => {}
            }
        }
        let result = sqlx::query(
            "UPDATE mail_draft_files SET as_link = $1 WHERE id = $2 AND draft_id = $3 \
             AND draft_id IN (SELECT id FROM mail_drafts WHERE id = $3 AND membership_id = $4)",
        )
        .bind(i64::from(as_link))
        .bind(file_id.to_string())
        .bind(draft_id.to_string())
        .bind(membership_id.to_string())
        .execute(&mut *transaction)
        .await?;
        if result.rows_affected() == 0 {
            return Err(MailError::NotFound);
        }
        touch(&mut transaction, draft_id, now).await?;
        let row = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT {FILE_COLUMNS} FROM mail_draft_files WHERE id = $1"
        )))
        .bind(file_id.to_string())
        .fetch_one(&mut *transaction)
        .await?;
        transaction.commit().await?;
        file(&row)
    }
}

/// `Conflict("draft_sending")` while the draft is held for sending: it must be cancelled (undone)
/// first, so a held message cannot skip the checks of the send.
async fn ensure_editable(
    transaction: &mut Transaction<'_, Any>,
    membership_id: Id,
    draft_id: Id,
) -> Result<(), MailError> {
    // Ownership first: another member's draft is `NotFound`, held or not.
    let owned: Option<String> =
        sqlx::query_scalar("SELECT id FROM mail_drafts WHERE id = $1 AND membership_id = $2")
            .bind(draft_id.to_string())
            .bind(membership_id.to_string())
            .fetch_optional(&mut **transaction)
            .await?;
    owned.ok_or(MailError::NotFound)?;
    let held: Option<String> =
        sqlx::query_scalar("SELECT id FROM mail_outbound WHERE draft_id = $1 AND status = 'held'")
            .bind(draft_id.to_string())
            .fetch_optional(&mut **transaction)
            .await?;
    if held.is_some() {
        return Err(MailError::Conflict("draft_sending"));
    }
    Ok(())
}

/// A change of the files moves the draft up the list without a new version (the editor's text
/// is unchanged, so a file change must not make its next save a conflict).
async fn touch(
    transaction: &mut Transaction<'_, Any>,
    draft_id: Id,
    now: TimestampMillis,
) -> Result<(), MailError> {
    sqlx::query("UPDATE mail_drafts SET updated_at = $1 WHERE id = $2")
        .bind(now.as_millis())
        .bind(draft_id.to_string())
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{DraftMode, base_key, prefixed};

    #[test]
    fn prefix_is_not_doubled() {
        assert_eq!(prefixed("Hello", DraftMode::Reply), "Re: Hello");
        assert_eq!(prefixed("RE: Hello", DraftMode::ReplyAll), "RE: Hello");
        assert_eq!(prefixed("Fw: Hello", DraftMode::Forward), "Fw: Hello");
        assert_eq!(prefixed("Re: Hello", DraftMode::Forward), "Fwd: Re: Hello");
    }

    #[test]
    fn base_key_drops_tag_and_case() {
        assert_eq!(base_key("Support+News@One.test"), "support@one.test");
    }
}
