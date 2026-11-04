//! Thread lists and search (spec §8.3): the search operators, and the one query behind the
//! thread list, a thread, and search. The caller's `mailbox_members` row is a join of that
//! query, so a thread of a mailbox the caller does not read never reaches `LIMIT` (spec §10).
//! Pages are ordered newest first by (`last_message_at`, `id`) and continue after a cursor of
//! the same pair, so mail arriving meanwhile neither repeats nor skips a thread.

use std::collections::HashMap;

use chrono::NaiveDate;
use orbit_platform::{Id, Row, SqlBuilder, TimestampMillis};
use serde_json::Value;
use sqlx::{Any, Transaction};

use super::MailError;
use super::domains::parse_id;
use super::reading::{MailFolder, MailPerson, MailThreadSummary};
use crate::repositories::pages::{fts_query, pg_tsquery};

/// Senders shown for each thread of a list.
const LIST_SENDERS: usize = 5;

/// A search box query (spec §8.3): words plus `from:`, `to:`, `subject:`, `label:`, `in:`,
/// `is:unread`, `is:starred`, `has:attachment`, `before:` and `after:` (`YYYY-MM-DD`, UTC).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MailQuery {
    /// The words left once the operators are taken out, separated by one space.
    pub text: String,
    /// Part of a sender's name or address.
    pub from: Option<String>,
    /// Part of a `To` or `Cc` name or address.
    pub to: Option<String>,
    /// Part of the subject.
    pub subject: Option<String>,
    /// A label name (any case).
    pub label: Option<String>,
    /// Without `in:`, Spam and Trash are left out.
    pub folder: Option<MailFolder>,
    pub unread: bool,
    pub starred: bool,
    pub has_attachment: bool,
    /// Threads whose newest message arrived before this time (ms, the start of the day).
    pub before: Option<i64>,
    /// … on or after this time (ms, the start of the day).
    pub after: Option<i64>,
}

impl MailQuery {
    fn has_filter(&self) -> bool {
        self.from.is_some()
            || self.to.is_some()
            || self.subject.is_some()
            || self.label.is_some()
            || self.folder.is_some()
            || self.unread
            || self.starred
            || self.has_attachment
            || self.before.is_some()
            || self.after.is_some()
    }
}

/// Splits `q` into words, keeping `op:"two words"` (and `"two words"`) together.
fn tokens(q: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for character in q.chars() {
        if character == '"' {
            quoted = !quoted;
            current.push(character);
        } else if character.is_whitespace() && !quoted {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
        } else {
            current.push(character);
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

/// The start of a `YYYY-MM-DD` day (UTC) in milliseconds.
fn day_start(value: &str) -> Option<i64> {
    let date = NaiveDate::parse_from_str(value, "%Y-%m-%d").ok()?;
    Some(date.and_hms_opt(0, 0, 0)?.and_utc().timestamp_millis())
}

/// Applies one `operator:value`; `false` when the operator or the value is unknown.
fn apply(query: &mut MailQuery, operator: &str, value: String) -> bool {
    match operator.to_ascii_lowercase().as_str() {
        "from" => query.from = Some(value),
        "to" => query.to = Some(value),
        "subject" => query.subject = Some(value),
        "label" => query.label = Some(value),
        "in" => match MailFolder::parse(&value.to_ascii_lowercase()) {
            Some(folder) => query.folder = Some(folder),
            None => return false,
        },
        "is" => match value.to_ascii_lowercase().as_str() {
            "unread" => query.unread = true,
            "starred" => query.starred = true,
            _ => return false,
        },
        "has" if value.eq_ignore_ascii_case("attachment") => query.has_attachment = true,
        "before" => match day_start(&value) {
            Some(at) => query.before = Some(at),
            None => return false,
        },
        "after" => match day_start(&value) {
            Some(at) => query.after = Some(at),
            None => return false,
        },
        _ => return false,
    }
    true
}

/// Parses a search box query. An unknown operator or a value that does not fit stays a word.
#[must_use]
pub fn parse_query(q: &str) -> MailQuery {
    let mut query = MailQuery::default();
    let mut words = Vec::new();
    for token in tokens(q) {
        let Some((operator, raw)) = token.split_once(':') else {
            words.push(token);
            continue;
        };
        let value = raw.trim_matches('"').trim().to_owned();
        let known = !value.is_empty() && apply(&mut query, operator, value);
        if !known {
            words.push(token);
        }
    }
    query.text = words.join(" ");
    query
}

/// Which folders a selection reads.
#[derive(Clone, Copy, Debug)]
pub(super) enum Folders {
    /// Every folder (one thread, or threads named by id).
    Any,
    One(MailFolder),
    /// Inbox and Archive: everything but Spam and Trash.
    Kept,
}

/// The threads one query reads, always among the mailboxes the caller is a member of.
pub(super) struct Selection {
    pub mailbox_id: Option<Id>,
    pub thread_ids: Option<Vec<Id>>,
    pub folders: Folders,
    pub label_id: Option<Id>,
    pub query: MailQuery,
    /// The full-text expression for the backend (from `query.text`).
    pub expression: Option<String>,
}

impl Selection {
    pub(super) fn new(folders: Folders) -> Self {
        Self {
            mailbox_id: None,
            thread_ids: None,
            folders,
            label_id: None,
            query: MailQuery::default(),
            expression: None,
        }
    }

    /// The selection of a search box query; `None` when it names nothing to search for.
    pub(super) fn search(query: MailQuery, postgres: bool) -> Option<Self> {
        let expression = if postgres {
            pg_tsquery(&query.text)
        } else {
            fts_query(&query.text)
        };
        if expression.is_none() && !query.has_filter() {
            return None;
        }
        Some(Self {
            folders: query.folder.map_or(Folders::Kept, Folders::One),
            expression,
            query,
            ..Self::new(Folders::Kept)
        })
    }
}

/// Where a page starts: after the thread with this time and id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThreadCursor {
    pub last_message_at: i64,
    pub id: Id,
}

impl ThreadCursor {
    /// `<last_message_at>_<id>`, as [`ThreadCursor::encode`] writes it.
    pub fn parse(value: &str) -> Result<Self, MailError> {
        let invalid = MailError::Invalid { field: "cursor" };
        let (at, id) = value.split_once('_').ok_or(invalid)?;
        Ok(Self {
            last_message_at: at
                .parse()
                .map_err(|_| MailError::Invalid { field: "cursor" })?,
            id: id
                .parse()
                .map_err(|_| MailError::Invalid { field: "cursor" })?,
        })
    }

    #[must_use]
    pub fn encode(&self) -> String {
        format!("{}_{}", self.last_message_at, self.id)
    }
}

/// `value` as a `LIKE` pattern matching it anywhere, lower case, with `%`, `_` and `\` taken
/// literally (`ESCAPE '\'`).
fn contains_pattern(value: &str) -> String {
    let mut pattern = String::from("%");
    for character in value.to_lowercase().chars() {
        if matches!(character, '%' | '_' | '\\') {
            pattern.push('\\');
        }
        pattern.push(character);
    }
    pattern.push('%');
    pattern
}

/// The threads of `selection` that `membership_id` reads, newest first: at most `limit`, after
/// `cursor`. Returns the threads and whether more follow.
#[allow(clippy::too_many_arguments)]
pub(super) async fn select_threads(
    transaction: &mut Transaction<'_, Any>,
    postgres: bool,
    workspace_id: Id,
    membership_id: Id,
    selection: &Selection,
    cursor: Option<ThreadCursor>,
    limit: usize,
) -> Result<(Vec<MailThreadSummary>, bool), MailError> {
    let mut sql = SqlBuilder::new(
        "SELECT t.id, t.mailbox_id, t.subject, t.snippet, t.last_message_at, t.message_count, \
         t.has_attachments, t.folder, \
         (s.last_read_at IS NULL OR s.last_read_at < t.last_message_at) AS unread, \
         COALESCE(s.starred, 0) AS starred \
         FROM mail_threads t \
         JOIN mailbox_members mm ON mm.mailbox_id = t.mailbox_id AND mm.membership_id = ",
    );
    sql.push_bind(membership_id.to_string())
        .push(" LEFT JOIN mail_thread_user_state s ON s.thread_id = t.id AND s.membership_id = ")
        .push_bind(membership_id.to_string())
        .push(" WHERE t.workspace_id = ")
        .push_bind(workspace_id.to_string());
    if let Some(mailbox_id) = selection.mailbox_id {
        sql.push(" AND t.mailbox_id = ")
            .push_bind(mailbox_id.to_string());
    }
    if let Some(ids) = &selection.thread_ids {
        if ids.is_empty() {
            return Ok((Vec::new(), false));
        }
        sql.push(" AND t.id IN (");
        let mut list = sql.separated(", ");
        for id in ids {
            list.push_bind(id.to_string());
        }
        sql.push(")");
    }
    match selection.folders {
        Folders::Any => {}
        Folders::One(folder) => {
            sql.push(" AND t.folder = ").push_bind(folder.as_str());
        }
        Folders::Kept => {
            sql.push(" AND t.folder IN ('inbox', 'archive')");
        }
    }
    if let Some(label_id) = selection.label_id {
        sql.push(
            " AND EXISTS (SELECT 1 FROM mail_thread_labels tl WHERE tl.thread_id = t.id \
             AND tl.label_id = ",
        )
        .push_bind(label_id.to_string())
        .push(")");
    }
    let query = &selection.query;
    if query.starred {
        sql.push(" AND s.starred = 1");
    }
    if query.unread {
        sql.push(" AND (s.last_read_at IS NULL OR s.last_read_at < t.last_message_at)");
    }
    if query.has_attachment {
        sql.push(" AND t.has_attachments = 1");
    }
    if let Some(from) = &query.from {
        let pattern = contains_pattern(from);
        sql.push(
            " AND EXISTS (SELECT 1 FROM mail_thread_messages tm \
             JOIN mail_messages m ON m.id = tm.message_id WHERE tm.thread_id = t.id \
             AND (lower(m.from_addr) LIKE ",
        )
        .push_bind(pattern.clone())
        .push(" ESCAPE '\\' OR lower(m.from_name) LIKE ")
        .push_bind(pattern)
        .push(" ESCAPE '\\'))");
    }
    if let Some(to) = &query.to {
        let pattern = contains_pattern(to);
        sql.push(
            " AND EXISTS (SELECT 1 FROM mail_thread_messages tm \
             JOIN mail_messages m ON m.id = tm.message_id WHERE tm.thread_id = t.id \
             AND (lower(m.to_addrs) LIKE ",
        )
        .push_bind(pattern.clone())
        .push(" ESCAPE '\\' OR lower(m.cc_addrs) LIKE ")
        .push_bind(pattern)
        .push(" ESCAPE '\\'))");
    }
    if let Some(subject) = &query.subject {
        sql.push(" AND lower(t.subject) LIKE ")
            .push_bind(contains_pattern(subject))
            .push(" ESCAPE '\\'");
    }
    if let Some(label) = &query.label {
        sql.push(
            " AND EXISTS (SELECT 1 FROM mail_thread_labels tl JOIN mail_labels l \
             ON l.id = tl.label_id WHERE tl.thread_id = t.id AND lower(l.name) = ",
        )
        .push_bind(label.to_lowercase())
        .push(")");
    }
    if let Some(before) = query.before {
        sql.push(" AND t.last_message_at < ").push_bind(before);
    }
    if let Some(after) = query.after {
        sql.push(" AND t.last_message_at >= ").push_bind(after);
    }
    if let Some(expression) = &selection.expression {
        if postgres {
            sql.push(
                " AND EXISTS (SELECT 1 FROM mail_search ms WHERE ms.rowid = t.row_id \
                 AND ms.search_vector @@ to_tsquery('orbit_search', ",
            )
            .push_bind(expression.clone())
            .push("))");
        } else {
            sql.push(" AND t.row_id IN (SELECT rowid FROM mail_search WHERE mail_search MATCH ")
                .push_bind(expression.clone())
                .push(")");
        }
    }
    if let Some(cursor) = cursor {
        sql.push(" AND (t.last_message_at < ")
            .push_bind(cursor.last_message_at)
            .push(" OR (t.last_message_at = ")
            .push_bind(cursor.last_message_at)
            .push(" AND t.id < ")
            .push_bind(cursor.id.to_string())
            .push("))");
    }
    sql.push(" ORDER BY t.last_message_at DESC, t.id DESC LIMIT ")
        .push_bind(i64::try_from(limit + 1).unwrap_or(i64::MAX));
    let mut rows = sql.build().fetch_all(&mut **transaction).await?;
    let more = rows.len() > limit;
    rows.truncate(limit);

    let mut threads = Vec::with_capacity(rows.len());
    for row in &rows {
        let folder: String = row.try_get("folder")?;
        threads.push(MailThreadSummary {
            id: parse_id(&row.try_get::<String, _>("id")?)?,
            mailbox_id: parse_id(&row.try_get::<String, _>("mailbox_id")?)?,
            subject: row.try_get("subject")?,
            snippet: row.try_get("snippet")?,
            from: Vec::new(),
            last_message_at: TimestampMillis::from_millis(row.try_get("last_message_at")?),
            message_count: row.try_get("message_count")?,
            unread: row.try_get("unread")?,
            starred: row.try_get("starred")?,
            labels: Vec::new(),
            has_attachments: row.try_get("has_attachments")?,
            folder: MailFolder::parse(&folder).ok_or(MailError::Invalid { field: "folder" })?,
        });
    }
    hydrate(transaction, &mut threads).await?;
    Ok((threads, more))
}

/// Adds the senders and labels to `threads`.
async fn hydrate(
    transaction: &mut Transaction<'_, Any>,
    threads: &mut [MailThreadSummary],
) -> Result<(), MailError> {
    if threads.is_empty() {
        return Ok(());
    }
    let mut senders = SqlBuilder::new(
        "SELECT tm.thread_id, m.from_name, m.from_addr FROM mail_thread_messages tm \
         JOIN mail_messages m ON m.id = tm.message_id WHERE tm.thread_id IN (",
    );
    let mut list = senders.separated(", ");
    for thread in threads.iter() {
        list.push_bind(thread.id.to_string());
    }
    senders.push(") ORDER BY m.received_at, m.id");
    let mut from: HashMap<String, Vec<MailPerson>> = HashMap::new();
    for row in senders.build().fetch_all(&mut **transaction).await? {
        let Some(address) = row.try_get::<Option<String>, _>("from_addr")? else {
            continue;
        };
        let people = from.entry(row.try_get("thread_id")?).or_default();
        if people.len() < LIST_SENDERS && !people.iter().any(|person| person.address == address) {
            people.push(MailPerson {
                name: row.try_get("from_name")?,
                address,
            });
        }
    }
    let mut labels = SqlBuilder::new(
        "SELECT tl.thread_id, tl.label_id FROM mail_thread_labels tl \
         JOIN mail_labels l ON l.id = tl.label_id WHERE tl.thread_id IN (",
    );
    let mut list = labels.separated(", ");
    for thread in threads.iter() {
        list.push_bind(thread.id.to_string());
    }
    labels.push(") ORDER BY lower(l.name), l.id");
    let mut by_thread: HashMap<String, Vec<Id>> = HashMap::new();
    for row in labels.build().fetch_all(&mut **transaction).await? {
        by_thread
            .entry(row.try_get("thread_id")?)
            .or_default()
            .push(parse_id(&row.try_get::<String, _>("label_id")?)?);
    }
    for thread in threads {
        let key = thread.id.to_string();
        thread.from = from.remove(&key).unwrap_or_default();
        thread.labels = by_thread.remove(&key).unwrap_or_default();
    }
    Ok(())
}

/// `[{"name", "address"}]` as people.
pub(super) fn people(list: &str) -> Vec<MailPerson> {
    let Ok(Value::Array(entries)) = serde_json::from_str::<Value>(list) else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|entry| {
            Some(MailPerson {
                name: entry.get("name").and_then(Value::as_str).map(str::to_owned),
                address: entry.get("address")?.as_str()?.to_owned(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{ThreadCursor, contains_pattern, parse_query};

    #[test]
    fn like_patterns_take_wildcards_literally() {
        assert_eq!(contains_pattern("A_b%c\\"), "%a\\_b\\%c\\\\%");
    }

    #[test]
    fn quoted_words_stay_together() {
        let query = parse_query("subject:\"big news\" \"exact words\"");
        assert_eq!(query.subject.as_deref(), Some("big news"));
        assert_eq!(query.text, "\"exact words\"");
        // An empty value is a word.
        assert_eq!(parse_query("from: x").text, "from: x");
    }

    #[test]
    fn cursors_round_trip() {
        let cursor = ThreadCursor {
            last_message_at: 42,
            id: orbit_platform::Id::new_v7(),
        };
        assert_eq!(ThreadCursor::parse(&cursor.encode()).unwrap(), cursor);
        assert!(ThreadCursor::parse("42").is_err());
        assert!(ThreadCursor::parse("x_y").is_err());
    }
}
