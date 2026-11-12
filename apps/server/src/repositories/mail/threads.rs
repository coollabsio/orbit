//! The threads of a mailbox (spec §6.3): which thread a new message joins, how it is filed, the
//! thread's counts and its search row. Everything runs in the ingest's transaction.

use std::collections::BTreeSet;

use orbit_platform::{Id, Row, SqlBuilder, TimestampMillis};
use serde_json::Value;
use sqlx::{Any, Transaction};

use super::MailError;
use super::domains::parse_id;

/// Bytes of the bodies of a thread kept in its search row (Global Constraints: 256 KB).
pub const SEARCH_BODY_BYTES: usize = 256 * 1024;
/// Candidate threads looked at for one message.
const CANDIDATE_THREADS: i64 = 20;
/// Messages read for a thread's participants and search row.
const THREAD_MESSAGES_READ: i64 = 1000;

/// What the threading needs of a new message.
pub(super) struct Incoming<'a> {
    pub message_id: Id,
    pub raw_sha256: &'a str,
    /// `In-Reply-To` and `References`, without duplicates.
    pub thread_ids: &'a [String],
    pub subject: &'a str,
    pub normalized_subject: &'a str,
    /// The `From` address (lower case).
    pub sender: Option<&'a str>,
    /// The `From` domain is one of the workspace's domains.
    pub sender_inside: bool,
    pub received_at: TimestampMillis,
    pub snippet: &'a str,
    pub has_attachments: bool,
    pub dmarc_failed: bool,
}

/// A message filed into a mailbox: its thread and the users to tell.
pub(super) struct Filed {
    pub thread_id: Id,
    pub new_thread: bool,
    pub users: Vec<Id>,
}

/// Files `message` into `mailbox_id`, once: `None` when the mailbox already has these bytes
/// (the dedupe key `(mailbox_id, raw_sha256)`), so a re-run changes nothing.
pub(super) async fn file(
    transaction: &mut Transaction<'_, Any>,
    workspace_id: Id,
    mailbox_id: Id,
    message: &Incoming<'_>,
    domains: &[String],
    now: TimestampMillis,
) -> Result<Option<Filed>, MailError> {
    let delivered: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM mail_thread_messages WHERE mailbox_id = $1 AND raw_sha256 = $2",
    )
    .bind(mailbox_id.to_string())
    .bind(message.raw_sha256)
    .fetch_optional(&mut **transaction)
    .await?;
    if delivered.is_some() {
        return Ok(None);
    }

    let found = find_thread(transaction, workspace_id, mailbox_id, message).await?;
    let (thread_id, new_thread, external) = match found {
        Some(thread_id) => {
            let first_inside = first_sender(transaction, thread_id)
                .await?
                .is_some_and(|address| is_inside(&address, domains));
            (thread_id, false, first_inside && !message.sender_inside)
        }
        None => (
            create_thread(transaction, workspace_id, mailbox_id, message, now).await?,
            true,
            false,
        ),
    };

    let inserted = sqlx::query(
        "INSERT INTO mail_thread_messages (thread_id, message_id, mailbox_id, raw_sha256, external, created_at) \
         VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT (mailbox_id, raw_sha256) DO NOTHING",
    )
    .bind(thread_id.to_string())
    .bind(message.message_id.to_string())
    .bind(mailbox_id.to_string())
    .bind(message.raw_sha256)
    .bind(i64::from(external))
    .bind(now.as_millis())
    .execute(&mut **transaction)
    .await?
    .rows_affected();
    if inserted == 0 {
        // Checked above inside this serialized transaction; kept as a guard.
        return Ok(None);
    }
    if !new_thread {
        add_to_thread(transaction, thread_id, message, now).await?;
    }
    index_thread(transaction, thread_id).await?;
    let users = mailbox_users(transaction, mailbox_id).await?;
    Ok(Some(Filed {
        thread_id,
        new_thread,
        users,
    }))
}

/// The thread of the mailbox that `message` replies to (spec §6.3): a thread with a message
/// whose `Message-ID` is in `In-Reply-To` or `References`, kept only when the normalized subject
/// matches or the sender is a participant (`From`, `To` or `Cc` of its messages). The thread
/// with the newest matching message wins. A message that failed DMARC joins only a spam thread,
/// so a forged reply cannot land in a thread of the inbox.
async fn find_thread(
    transaction: &mut Transaction<'_, Any>,
    workspace_id: Id,
    mailbox_id: Id,
    message: &Incoming<'_>,
) -> Result<Option<Id>, MailError> {
    if message.thread_ids.is_empty() {
        return Ok(None);
    }
    let mut sql = SqlBuilder::new(
        "SELECT t.id, t.normalized_subject, MAX(m.received_at) AS matched_at \
         FROM mail_messages m \
         JOIN mail_thread_messages tm ON tm.message_id = m.id \
         JOIN mail_threads t ON t.id = tm.thread_id \
         WHERE m.workspace_id = ",
    );
    sql.push_bind(workspace_id.to_string())
        .push(" AND tm.mailbox_id = ")
        .push_bind(mailbox_id.to_string())
        .push(" AND m.message_id_header IN (");
    let mut ids = sql.separated(", ");
    for id in message.thread_ids {
        ids.push_bind(id.clone());
    }
    sql.push(")");
    if message.dmarc_failed {
        sql.push(" AND t.folder = 'spam'");
    }
    sql.push(" GROUP BY t.id, t.normalized_subject ORDER BY matched_at DESC, t.id DESC LIMIT ")
        .push_bind(CANDIDATE_THREADS);
    let candidates = sql.build().fetch_all(&mut **transaction).await?;
    for row in candidates {
        let id: String = row.try_get("id")?;
        let thread_id = parse_id(&id)?;
        let subject: String = row.try_get("normalized_subject")?;
        if subject == message.normalized_subject {
            return Ok(Some(thread_id));
        }
        if let Some(sender) = message.sender
            && is_participant(transaction, thread_id, sender).await?
        {
            return Ok(Some(thread_id));
        }
    }
    Ok(None)
}

/// Whether `address` is in `From`, `To` or `Cc` of a message of the thread.
async fn is_participant(
    transaction: &mut Transaction<'_, Any>,
    thread_id: Id,
    address: &str,
) -> Result<bool, MailError> {
    let rows = sqlx::query(
        "SELECT m.from_addr, m.to_addrs, m.cc_addrs FROM mail_thread_messages tm \
         JOIN mail_messages m ON m.id = tm.message_id WHERE tm.thread_id = $1 LIMIT $2",
    )
    .bind(thread_id.to_string())
    .bind(THREAD_MESSAGES_READ)
    .fetch_all(&mut **transaction)
    .await?;
    for row in rows {
        let from: Option<String> = row.try_get("from_addr")?;
        if from.as_deref() == Some(address) {
            return Ok(true);
        }
        for column in ["to_addrs", "cc_addrs"] {
            let list: String = row.try_get(column)?;
            if addresses(&list).any(|found| found == address) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// The `From` address of the thread's first message.
async fn first_sender(
    transaction: &mut Transaction<'_, Any>,
    thread_id: Id,
) -> Result<Option<String>, MailError> {
    let address: Option<Option<String>> = sqlx::query_scalar(
        "SELECT m.from_addr FROM mail_thread_messages tm JOIN mail_messages m ON m.id = tm.message_id \
         WHERE tm.thread_id = $1 ORDER BY m.received_at, m.id LIMIT 1",
    )
    .bind(thread_id.to_string())
    .fetch_optional(&mut **transaction)
    .await?;
    Ok(address.flatten())
}

/// A new thread holding `message`: in Spam when DMARC failed, else in the inbox.
async fn create_thread(
    transaction: &mut Transaction<'_, Any>,
    workspace_id: Id,
    mailbox_id: Id,
    message: &Incoming<'_>,
    now: TimestampMillis,
) -> Result<Id, MailError> {
    let thread_id = Id::new_v7();
    let (folder, spam_reason) = if message.dmarc_failed {
        ("spam", Some("dmarc_fail"))
    } else {
        ("inbox", None)
    };
    sqlx::query(
        "INSERT INTO mail_threads (id, workspace_id, mailbox_id, subject, normalized_subject, folder, \
         spam_reason, last_message_at, message_count, has_attachments, snippet, created_at, updated_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 1, $9, $10, $11, $12)",
    )
    .bind(thread_id.to_string())
    .bind(workspace_id.to_string())
    .bind(mailbox_id.to_string())
    .bind(message.subject)
    .bind(message.normalized_subject)
    .bind(folder)
    .bind(spam_reason)
    .bind(message.received_at.as_millis())
    .bind(i64::from(message.has_attachments))
    .bind(message.snippet)
    .bind(now.as_millis())
    .bind(now.as_millis())
    .execute(&mut **transaction)
    .await?;
    Ok(thread_id)
}

/// Counts, snippet and time of a thread that got `message`. A new message brings an archived
/// thread back to the inbox; a thread in Spam or Trash stays there.
async fn add_to_thread(
    transaction: &mut Transaction<'_, Any>,
    thread_id: Id,
    message: &Incoming<'_>,
    now: TimestampMillis,
) -> Result<(), MailError> {
    // Every right-hand side reads the row as it was before this update.
    sqlx::query(
        "UPDATE mail_threads SET message_count = message_count + 1, \
         snippet = CASE WHEN $1 >= last_message_at THEN $2 ELSE snippet END, \
         last_message_at = CASE WHEN $3 > last_message_at THEN $4 ELSE last_message_at END, \
         has_attachments = CASE WHEN $5 = 1 THEN 1 ELSE has_attachments END, \
         folder = CASE WHEN folder = 'archive' THEN 'inbox' ELSE folder END, \
         updated_at = $6 WHERE id = $7",
    )
    .bind(message.received_at.as_millis())
    .bind(message.snippet)
    .bind(message.received_at.as_millis())
    .bind(message.received_at.as_millis())
    .bind(i64::from(message.has_attachments))
    .bind(now.as_millis())
    .bind(thread_id.to_string())
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

/// Replaces the thread's search row: its subject, the names and addresses of the people, and
/// the first [`SEARCH_BODY_BYTES`] of the bodies, oldest message first. The same statements work
/// on the SQLite FTS5 table and its Postgres twin.
async fn index_thread(
    transaction: &mut Transaction<'_, Any>,
    thread_id: Id,
) -> Result<(), MailError> {
    let thread = sqlx::query("SELECT row_id, subject FROM mail_threads WHERE id = $1")
        .bind(thread_id.to_string())
        .fetch_one(&mut **transaction)
        .await?;
    let row_id: i64 = thread.try_get("row_id")?;
    let subject: String = thread.try_get("subject")?;
    let rows = sqlx::query(
        "SELECT m.from_name, m.from_addr, m.to_addrs, m.cc_addrs, \
         substr(m.body_text, 1, CAST($1 AS INTEGER)) AS body FROM mail_thread_messages tm \
         JOIN mail_messages m ON m.id = tm.message_id WHERE tm.thread_id = $2 \
         ORDER BY m.received_at, m.id LIMIT $3",
    )
    .bind(i64::try_from(SEARCH_BODY_BYTES).unwrap_or(i64::MAX))
    .bind(thread_id.to_string())
    .bind(THREAD_MESSAGES_READ)
    .fetch_all(&mut **transaction)
    .await?;
    let mut people = BTreeSet::new();
    let mut body = String::new();
    for row in rows {
        let name: Option<String> = row.try_get("from_name")?;
        let address: Option<String> = row.try_get("from_addr")?;
        people.extend(name);
        people.extend(address);
        for column in ["to_addrs", "cc_addrs"] {
            let list: String = row.try_get(column)?;
            for person in people_of(&list) {
                people.extend(person.into_iter().flatten());
            }
        }
        if body.len() < SEARCH_BODY_BYTES {
            let text: String = row.try_get("body")?;
            if !body.is_empty() {
                body.push('\n');
            }
            body.push_str(&text);
            truncate(&mut body, SEARCH_BODY_BYTES);
        }
    }
    let people = people.into_iter().collect::<Vec<_>>().join(" ");
    sqlx::query("DELETE FROM mail_search WHERE rowid = $1")
        .bind(row_id)
        .execute(&mut **transaction)
        .await?;
    sqlx::query("INSERT INTO mail_search (rowid, subject, people, body) VALUES ($1, $2, $3, $4)")
        .bind(row_id)
        .bind(subject)
        .bind(people)
        .bind(body)
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

/// The users who read the mailbox (its members' users).
pub(super) async fn mailbox_users(
    transaction: &mut Transaction<'_, Any>,
    mailbox_id: Id,
) -> Result<Vec<Id>, MailError> {
    let users: Vec<String> = sqlx::query_scalar(
        "SELECT ms.user_id FROM mailbox_members mm JOIN memberships ms ON ms.id = mm.membership_id \
         WHERE mm.mailbox_id = $1 ORDER BY ms.user_id",
    )
    .bind(mailbox_id.to_string())
    .fetch_all(&mut **transaction)
    .await?;
    users.iter().map(|id| parse_id(id)).collect()
}

/// Whether the domain of `address` is one of `domains`.
pub(super) fn is_inside(address: &str, domains: &[String]) -> bool {
    address
        .rsplit_once('@')
        .is_some_and(|(_, domain)| domains.iter().any(|name| name.eq_ignore_ascii_case(domain)))
}

/// The addresses of a JSON list of `{"name", "address"}`.
fn addresses(list: &str) -> impl Iterator<Item = String> {
    people_of(list).filter_map(|[_, address]| address)
}

/// `[name, address]` of each entry of a JSON list of `{"name", "address"}`.
fn people_of(list: &str) -> impl Iterator<Item = [Option<String>; 2]> {
    let entries = match serde_json::from_str::<Value>(list) {
        Ok(Value::Array(entries)) => entries,
        _ => Vec::new(),
    };
    entries.into_iter().map(|entry| {
        let text = |key: &str| entry.get(key).and_then(Value::as_str).map(str::to_owned);
        [text("name"), text("address")]
    })
}

/// Cuts `text` to at most `bytes` bytes, at a character boundary.
pub(super) fn truncate(text: &mut String, bytes: usize) {
    if text.len() > bytes {
        let mut end = bytes;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
}
