//! Docs comments: threads on a page, anchored to text by BlockNote's `comment` mark inside the
//! collaborative document (the mark's `threadId` is a thread id here; the server never writes the
//! mark). A thread holds comments whose bodies are BlockNote blocks of the comment editor
//! (paragraphs with styled text, links and `mention` inline content).
//!
//! Access is the page's: teamspace pages for every member, private pages for their owner only,
//! trashed pages for nobody; everyone else gets `NotFound`, the same as for an unknown id. Anyone
//! who can see the page can start threads, reply, resolve and reopen; only authors edit or delete
//! their comments, and only a thread's creator deletes the whole thread.
//!
//! Mentions are read from the body (`mention` nodes with a `userId`). Only members who can see the
//! page count; newly mentioned ones (never the author) get a `page_comment_mentioned` notification.

use std::collections::{BTreeSet, HashMap};

use orbit_platform::{Database, Id, TimestampMillis};
use serde::Serialize;
use serde_json::{Map, Value, json};
use sqlx::{Row, Sqlite, Transaction};
use thiserror::Error;
use utoipa::ToSchema;

use super::pages::VISIBLE;
use crate::audit::{self, AuditOutcome};
use crate::collab::sanitize::normalize_link_href;

/// Blocks in one comment.
const MAX_BLOCKS: usize = 50;
/// Characters of a comment's plain text (mention names included).
const MAX_TEXT_CHARS: usize = 10_000;
/// Serialized size of a comment body.
const MAX_BODY_BYTES: usize = 64 * 1024;
/// Characters of the quoted text stored with a thread.
const MAX_QUOTE_CHARS: usize = 1_000;
/// Distinct users one comment may mention.
const MAX_MENTIONS: usize = 50;
const MAX_MENTION_NAME_CHARS: usize = 100;

/// One comment of a thread. A deleted comment keeps its place with an empty body.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct PageComment {
    #[schema(value_type = String)]
    pub id: Id,
    #[schema(value_type = String)]
    pub thread_id: Id,
    #[schema(value_type = String)]
    pub author_id: Id,
    /// BlockNote blocks (paragraphs; inline `text`, `link` and `mention {userId, name}`); `[]`
    /// once deleted.
    #[schema(value_type = Vec<Object>)]
    pub body: Value,
    /// Plain text of the body (mentions as `@name`).
    pub body_text: String,
    /// Members this comment mentions who can see the page.
    #[schema(value_type = Vec<String>)]
    pub mentioned_user_ids: Vec<Id>,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: TimestampMillis,
    #[schema(value_type = String, format = DateTime)]
    pub updated_at: TimestampMillis,
    #[schema(value_type = Option<String>, format = DateTime)]
    pub edited_at: Option<TimestampMillis>,
    #[schema(value_type = Option<String>, format = DateTime)]
    pub deleted_at: Option<TimestampMillis>,
}

/// A comment thread on a page, with its comments oldest first.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct PageThread {
    #[schema(value_type = String)]
    pub id: Id,
    #[schema(value_type = String)]
    pub page_id: Id,
    #[schema(value_type = String)]
    pub created_by: Id,
    /// The text selected when the thread was started.
    pub quote: String,
    pub resolved: bool,
    #[schema(value_type = Option<String>, format = DateTime)]
    pub resolved_at: Option<TimestampMillis>,
    #[schema(value_type = Option<String>)]
    pub resolved_by: Option<Id>,
    pub version: u64,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: TimestampMillis,
    #[schema(value_type = String, format = DateTime)]
    pub updated_at: TimestampMillis,
    pub comments: Vec<PageComment>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct PageThreadList {
    /// Open and resolved threads, oldest first.
    pub items: Vec<PageThread>,
}

#[derive(Debug, Error)]
pub enum PageCommentError {
    /// The page is missing, trashed or not visible to the caller (same answer as an unknown id).
    #[error("page was not found")]
    PageNotFound,
    /// The page is visible, but it has no such thread or comment.
    #[error("comment was not found")]
    NotFound,
    /// Someone else's comment (edit, delete) or thread (delete).
    #[error("comment action is not permitted")]
    Forbidden,
    #[error("comment input is invalid: {field}")]
    Invalid { field: &'static str },
    #[error("stored comment data is invalid")]
    Corrupt,
    #[error("comment repository is unavailable")]
    Unavailable(#[from] sqlx::Error),
}

/// A validated comment body.
#[derive(Clone, Debug)]
pub struct CommentBody {
    blocks: Vec<Value>,
    text: String,
    mentions: Vec<Id>,
}

impl CommentBody {
    /// Keeps paragraphs with text, links (safe hrefs only; others become plain text) and
    /// mentions; drops everything else. Fails when nothing readable remains or limits are hit.
    pub fn parse(value: Value) -> Result<Self, PageCommentError> {
        let invalid = || PageCommentError::Invalid { field: "body" };
        let Value::Array(blocks) = value else {
            return Err(invalid());
        };
        if blocks.is_empty() || blocks.len() > MAX_BLOCKS {
            return Err(invalid());
        }
        let mut clean = Vec::with_capacity(blocks.len());
        let mut lines = Vec::with_capacity(blocks.len());
        let mut mentions = BTreeSet::new();
        for block in &blocks {
            if block.get("type").and_then(Value::as_str) != Some("paragraph") {
                continue;
            }
            let mut content = Vec::new();
            let mut line = String::new();
            for item in block
                .get("content")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                clean_inline(item, &mut content, &mut line, &mut mentions)?;
            }
            lines.push(line);
            let mut props = Map::new();
            if let Some(source) = block.get("props").and_then(Value::as_object) {
                for key in ["textColor", "backgroundColor", "textAlignment"] {
                    if let Some(Value::String(value)) = source.get(key)
                        && value.len() <= 32
                    {
                        props.insert(key.to_owned(), Value::String(value.clone()));
                    }
                }
            }
            let mut out = Map::new();
            if let Some(Value::String(id)) = block.get("id")
                && id.len() <= 64
            {
                out.insert("id".to_owned(), Value::String(id.clone()));
            }
            out.insert("type".to_owned(), json!("paragraph"));
            out.insert("props".to_owned(), Value::Object(props));
            out.insert("content".to_owned(), Value::Array(content));
            out.insert("children".to_owned(), json!([]));
            clean.push(Value::Object(out));
        }
        let text = lines.join("\n").trim().to_owned();
        if text.is_empty() || text.chars().count() > MAX_TEXT_CHARS {
            return Err(invalid());
        }
        if mentions.len() > MAX_MENTIONS {
            return Err(PageCommentError::Invalid { field: "mentions" });
        }
        let blocks = clean;
        if serde_json::to_vec(&blocks).map_or(usize::MAX, |bytes| bytes.len()) > MAX_BODY_BYTES {
            return Err(invalid());
        }
        Ok(Self {
            blocks,
            text,
            mentions: mentions.into_iter().collect(),
        })
    }

    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Every user id the body mentions (before the visibility filter).
    #[must_use]
    pub fn mentions(&self) -> &[Id] {
        &self.mentions
    }
}

fn clean_styles(styles: Option<&Value>) -> Value {
    let mut out = Map::new();
    if let Some(styles) = styles.and_then(Value::as_object) {
        for (name, value) in styles {
            let known = matches!(
                name.as_str(),
                "bold" | "italic" | "underline" | "strike" | "code"
            );
            if known && value == &Value::Bool(true) {
                out.insert(name.clone(), Value::Bool(true));
            }
        }
    }
    Value::Object(out)
}

fn clean_text(item: &Value, line: &mut String) -> Option<Value> {
    let text = item.get("text").and_then(Value::as_str)?;
    if text.is_empty() {
        return None;
    }
    line.push_str(text);
    Some(json!({ "type": "text", "text": text, "styles": clean_styles(item.get("styles")) }))
}

fn clean_inline(
    item: &Value,
    content: &mut Vec<Value>,
    line: &mut String,
    mentions: &mut BTreeSet<Id>,
) -> Result<(), PageCommentError> {
    match item.get("type").and_then(Value::as_str) {
        Some("text") => content.extend(clean_text(item, line)),
        Some("link") => {
            let runs: Vec<Value> = item
                .get("content")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|run| clean_text(run, line))
                .collect();
            let href = item
                .get("href")
                .and_then(Value::as_str)
                .and_then(normalize_link_href);
            match href {
                Some(href) if !runs.is_empty() => {
                    content.push(json!({ "type": "link", "href": href, "content": runs }));
                }
                _ => content.extend(runs),
            }
        }
        Some("mention") => {
            let props = item.get("props");
            let user_id = props
                .and_then(|props| props.get("userId"))
                .and_then(Value::as_str)
                .and_then(|id| id.parse::<Id>().ok())
                .ok_or(PageCommentError::Invalid { field: "mentions" })?;
            let name: String = props
                .and_then(|props| props.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .chars()
                .take(MAX_MENTION_NAME_CHARS)
                .collect();
            line.push('@');
            line.push_str(&name);
            mentions.insert(user_id);
            content.push(json!({
                "type": "mention",
                "props": { "userId": user_id.to_string(), "name": name },
            }));
        }
        _ => {}
    }
    Ok(())
}

fn quote(value: &str) -> String {
    value.trim().chars().take(MAX_QUOTE_CHARS).collect()
}

#[derive(Clone)]
pub struct PageCommentRepository {
    database: Database,
}

impl PageCommentRepository {
    #[must_use]
    pub fn new(database: Database) -> Self {
        Self { database }
    }

    /// Open and resolved threads of a page the caller can see.
    pub async fn threads(
        &self,
        workspace_id: Id,
        page_id: Id,
        actor_id: Id,
    ) -> Result<PageThreadList, PageCommentError> {
        let mut tx = self.database.pool().begin().await?;
        visible_page(&mut tx, workspace_id, page_id, actor_id).await?;
        let items = load_threads(&mut tx, page_id, None).await?;
        tx.commit().await?;
        Ok(PageThreadList { items })
    }

    /// Starts a thread with its first comment.
    #[allow(clippy::too_many_arguments)]
    pub async fn create_thread(
        &self,
        workspace_id: Id,
        page_id: Id,
        actor_id: Id,
        body: CommentBody,
        quote_text: &str,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<PageThread, PageCommentError> {
        let mut tx = self.database.immediate_transaction().await?;
        let page = visible_page(&mut tx, workspace_id, page_id, actor_id).await?;
        let thread_id = Id::new_v7();
        sqlx::query(
            "INSERT INTO page_threads (id, workspace_id, page_id, created_by, quote, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(thread_id.to_string())
        .bind(workspace_id.to_string())
        .bind(page_id.to_string())
        .bind(actor_id.to_string())
        .bind(quote(quote_text))
        .bind(now.as_millis())
        .bind(now.as_millis())
        .execute(&mut *tx)
        .await?;
        insert_comment(&mut tx, &page, thread_id, actor_id, &body, now).await?;
        record(
            &mut tx,
            &page,
            actor_id,
            "page.thread_created",
            "page_thread",
            thread_id,
            request_id,
            now,
        )
        .await?;
        let thread = load_thread(&mut tx, page_id, thread_id).await?;
        tx.commit().await?;
        Ok(thread)
    }

    /// Replies in a thread (resolved threads accept replies too).
    #[allow(clippy::too_many_arguments)]
    pub async fn add_comment(
        &self,
        workspace_id: Id,
        page_id: Id,
        thread_id: Id,
        actor_id: Id,
        body: CommentBody,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<PageComment, PageCommentError> {
        let mut tx = self.database.immediate_transaction().await?;
        let page = visible_page(&mut tx, workspace_id, page_id, actor_id).await?;
        require_thread(&mut tx, page_id, thread_id).await?;
        let comment_id = insert_comment(&mut tx, &page, thread_id, actor_id, &body, now).await?;
        touch_thread(&mut tx, thread_id, now).await?;
        record(
            &mut tx,
            &page,
            actor_id,
            "page.comment_created",
            "page_comment",
            comment_id,
            request_id,
            now,
        )
        .await?;
        let comment = load_comment(&mut tx, page_id, comment_id).await?;
        tx.commit().await?;
        Ok(comment)
    }

    /// Replaces the body of the caller's own comment; newly mentioned members are notified.
    #[allow(clippy::too_many_arguments)]
    pub async fn update_comment(
        &self,
        workspace_id: Id,
        page_id: Id,
        thread_id: Id,
        comment_id: Id,
        actor_id: Id,
        body: CommentBody,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<PageComment, PageCommentError> {
        let mut tx = self.database.immediate_transaction().await?;
        let page = visible_page(&mut tx, workspace_id, page_id, actor_id).await?;
        require_thread(&mut tx, page_id, thread_id).await?;
        require_own_comment(&mut tx, thread_id, comment_id, actor_id).await?;
        let body_json =
            serde_json::to_string(&body.blocks).map_err(|_| PageCommentError::Corrupt)?;
        sqlx::query(
            "UPDATE page_comments SET body_json = ?, body_text = ?, version = version + 1, \
             updated_at = ?, edited_at = ? WHERE id = ?",
        )
        .bind(body_json)
        .bind(&body.text)
        .bind(now.as_millis())
        .bind(now.as_millis())
        .bind(comment_id.to_string())
        .execute(&mut *tx)
        .await?;
        replace_mentions(&mut tx, &page, thread_id, comment_id, actor_id, &body, now).await?;
        touch_thread(&mut tx, thread_id, now).await?;
        record(
            &mut tx,
            &page,
            actor_id,
            "page.comment_updated",
            "page_comment",
            comment_id,
            request_id,
            now,
        )
        .await?;
        let comment = load_comment(&mut tx, page_id, comment_id).await?;
        tx.commit().await?;
        Ok(comment)
    }

    /// Deletes the caller's own comment: its body, mentions and notifications go; the row stays
    /// as a placeholder. When no live comment is left, the whole thread goes.
    #[allow(clippy::too_many_arguments)]
    pub async fn delete_comment(
        &self,
        workspace_id: Id,
        page_id: Id,
        thread_id: Id,
        comment_id: Id,
        actor_id: Id,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), PageCommentError> {
        let mut tx = self.database.immediate_transaction().await?;
        let page = visible_page(&mut tx, workspace_id, page_id, actor_id).await?;
        require_thread(&mut tx, page_id, thread_id).await?;
        require_own_comment(&mut tx, thread_id, comment_id, actor_id).await?;
        sqlx::query(
            "UPDATE page_comments SET body_json = '[]', body_text = '', version = version + 1, \
             updated_at = ?, deleted_at = ? WHERE id = ?",
        )
        .bind(now.as_millis())
        .bind(now.as_millis())
        .bind(comment_id.to_string())
        .execute(&mut *tx)
        .await?;
        sqlx::query("DELETE FROM page_comment_mentions WHERE comment_id = ?")
            .bind(comment_id.to_string())
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM notifications WHERE page_comment_id = ?")
            .bind(comment_id.to_string())
            .execute(&mut *tx)
            .await?;
        let live: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM page_comments WHERE thread_id = ? AND deleted_at IS NULL",
        )
        .bind(thread_id.to_string())
        .fetch_one(&mut *tx)
        .await?;
        if live == 0 {
            sqlx::query("DELETE FROM page_threads WHERE id = ?")
                .bind(thread_id.to_string())
                .execute(&mut *tx)
                .await?;
        } else {
            touch_thread(&mut tx, thread_id, now).await?;
        }
        record(
            &mut tx,
            &page,
            actor_id,
            "page.comment_deleted",
            "page_comment",
            comment_id,
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Deletes a whole thread (its creator only), comments and notifications included.
    pub async fn delete_thread(
        &self,
        workspace_id: Id,
        page_id: Id,
        thread_id: Id,
        actor_id: Id,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), PageCommentError> {
        let mut tx = self.database.immediate_transaction().await?;
        let page = visible_page(&mut tx, workspace_id, page_id, actor_id).await?;
        let created_by = require_thread(&mut tx, page_id, thread_id).await?;
        if created_by != actor_id {
            return Err(PageCommentError::Forbidden);
        }
        sqlx::query("DELETE FROM page_threads WHERE id = ?")
            .bind(thread_id.to_string())
            .execute(&mut *tx)
            .await?;
        record(
            &mut tx,
            &page,
            actor_id,
            "page.thread_deleted",
            "page_thread",
            thread_id,
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Resolves (`true`) or reopens (`false`) a thread; a no-op when it already is.
    #[allow(clippy::too_many_arguments)]
    pub async fn set_resolved(
        &self,
        workspace_id: Id,
        page_id: Id,
        thread_id: Id,
        actor_id: Id,
        resolved: bool,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<PageThread, PageCommentError> {
        let mut tx = self.database.immediate_transaction().await?;
        let page = visible_page(&mut tx, workspace_id, page_id, actor_id).await?;
        require_thread(&mut tx, page_id, thread_id).await?;
        let changed = if resolved {
            sqlx::query(
                "UPDATE page_threads SET resolved_at = MAX(?, created_at), resolved_by = ?, \
                 version = version + 1, updated_at = ? WHERE id = ? AND resolved_at IS NULL",
            )
            .bind(now.as_millis())
            .bind(actor_id.to_string())
            .bind(now.as_millis())
            .bind(thread_id.to_string())
            .execute(&mut *tx)
            .await?
        } else {
            sqlx::query(
                "UPDATE page_threads SET resolved_at = NULL, resolved_by = NULL, \
                 version = version + 1, updated_at = ? WHERE id = ? AND resolved_at IS NOT NULL",
            )
            .bind(now.as_millis())
            .bind(thread_id.to_string())
            .execute(&mut *tx)
            .await?
        }
        .rows_affected();
        if changed > 0 {
            record(
                &mut tx,
                &page,
                actor_id,
                if resolved {
                    "page.thread_resolved"
                } else {
                    "page.thread_reopened"
                },
                "page_thread",
                thread_id,
                request_id,
                now,
            )
            .await?;
        }
        let thread = load_thread(&mut tx, page_id, thread_id).await?;
        tx.commit().await?;
        Ok(thread)
    }
}

/// The live, visible page a comment action targets.
struct VisiblePage {
    workspace_id: Id,
    id: Id,
    /// `Some(owner)` for a private page.
    owner_id: Option<Id>,
}

async fn visible_page(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    page_id: Id,
    actor_id: Id,
) -> Result<VisiblePage, PageCommentError> {
    let row = sqlx::query(&format!(
        "SELECT pages.owner_id AS owner_id FROM pages \
         JOIN workspaces ON workspaces.id = pages.workspace_id \
         JOIN memberships ON memberships.workspace_id = pages.workspace_id AND memberships.user_id = ? \
         WHERE pages.id = ? AND pages.workspace_id = ? AND pages.deleted_at IS NULL \
         AND workspaces.deleted_at IS NULL AND {VISIBLE}"
    ))
    .bind(actor_id.to_string())
    .bind(page_id.to_string())
    .bind(workspace_id.to_string())
    .bind(actor_id.to_string())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(PageCommentError::PageNotFound)?;
    Ok(VisiblePage {
        workspace_id,
        id: page_id,
        owner_id: parse_optional(row.get("owner_id"))?,
    })
}

/// The thread's creator, when the thread belongs to the page.
async fn require_thread(
    tx: &mut Transaction<'_, Sqlite>,
    page_id: Id,
    thread_id: Id,
) -> Result<Id, PageCommentError> {
    let created_by: Option<String> =
        sqlx::query_scalar("SELECT created_by FROM page_threads WHERE id = ? AND page_id = ?")
            .bind(thread_id.to_string())
            .bind(page_id.to_string())
            .fetch_optional(&mut **tx)
            .await?;
    parse(created_by.ok_or(PageCommentError::NotFound)?)
}

async fn require_own_comment(
    tx: &mut Transaction<'_, Sqlite>,
    thread_id: Id,
    comment_id: Id,
    actor_id: Id,
) -> Result<(), PageCommentError> {
    let author: Option<String> = sqlx::query_scalar(
        "SELECT author_id FROM page_comments WHERE id = ? AND thread_id = ? AND deleted_at IS NULL",
    )
    .bind(comment_id.to_string())
    .bind(thread_id.to_string())
    .fetch_optional(&mut **tx)
    .await?;
    if parse(author.ok_or(PageCommentError::NotFound)?)? == actor_id {
        Ok(())
    } else {
        Err(PageCommentError::Forbidden)
    }
}

async fn touch_thread(
    tx: &mut Transaction<'_, Sqlite>,
    thread_id: Id,
    now: TimestampMillis,
) -> Result<(), PageCommentError> {
    sqlx::query("UPDATE page_threads SET version = version + 1, updated_at = ? WHERE id = ?")
        .bind(now.as_millis())
        .bind(thread_id.to_string())
        .execute(&mut **tx)
        .await?;
    Ok(())
}

async fn insert_comment(
    tx: &mut Transaction<'_, Sqlite>,
    page: &VisiblePage,
    thread_id: Id,
    actor_id: Id,
    body: &CommentBody,
    now: TimestampMillis,
) -> Result<Id, PageCommentError> {
    let comment_id = Id::new_v7();
    let body_json = serde_json::to_string(&body.blocks).map_err(|_| PageCommentError::Corrupt)?;
    sqlx::query(
        "INSERT INTO page_comments (id, workspace_id, page_id, thread_id, author_id, body_json, \
         body_text, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(comment_id.to_string())
    .bind(page.workspace_id.to_string())
    .bind(page.id.to_string())
    .bind(thread_id.to_string())
    .bind(actor_id.to_string())
    .bind(body_json)
    .bind(&body.text)
    .bind(now.as_millis())
    .bind(now.as_millis())
    .execute(&mut **tx)
    .await?;
    replace_mentions(tx, page, thread_id, comment_id, actor_id, body, now).await?;
    Ok(comment_id)
}

/// Members of the workspace who can see the page, out of `candidates`: every active member for a
/// teamspace page, only the owner for a private page.
async fn can_see(
    tx: &mut Transaction<'_, Sqlite>,
    page: &VisiblePage,
    candidates: &[Id],
) -> Result<Vec<Id>, PageCommentError> {
    let mut allowed = Vec::new();
    for user_id in candidates {
        if page.owner_id.is_some_and(|owner| owner != *user_id) {
            continue;
        }
        let member: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM memberships JOIN users ON users.id = memberships.user_id \
             WHERE memberships.workspace_id = ? AND memberships.user_id = ? AND users.suspended_at IS NULL)",
        )
        .bind(page.workspace_id.to_string())
        .bind(user_id.to_string())
        .fetch_one(&mut **tx)
        .await?;
        if member {
            allowed.push(*user_id);
        }
    }
    Ok(allowed)
}

/// Stores the comment's visible mentions and notifies the ones it did not mention before.
async fn replace_mentions(
    tx: &mut Transaction<'_, Sqlite>,
    page: &VisiblePage,
    thread_id: Id,
    comment_id: Id,
    actor_id: Id,
    body: &CommentBody,
    now: TimestampMillis,
) -> Result<(), PageCommentError> {
    let previous: BTreeSet<Id> = sqlx::query_scalar::<_, String>(
        "SELECT user_id FROM page_comment_mentions WHERE comment_id = ?",
    )
    .bind(comment_id.to_string())
    .fetch_all(&mut **tx)
    .await?
    .into_iter()
    .map(parse)
    .collect::<Result<_, _>>()?;
    let mentioned = can_see(tx, page, &body.mentions).await?;
    sqlx::query("DELETE FROM page_comment_mentions WHERE comment_id = ?")
        .bind(comment_id.to_string())
        .execute(&mut **tx)
        .await?;
    for user_id in &mentioned {
        sqlx::query("INSERT INTO page_comment_mentions (comment_id, user_id) VALUES (?, ?)")
            .bind(comment_id.to_string())
            .bind(user_id.to_string())
            .execute(&mut **tx)
            .await?;
    }
    for recipient in mentioned
        .iter()
        .filter(|id| **id != actor_id && !previous.contains(id))
    {
        sqlx::query(
            "INSERT OR IGNORE INTO notifications \
             (id, workspace_id, recipient_user_id, actor_user_id, kind, page_id, page_thread_id, \
              page_comment_id, dedupe_key, created_at) \
             VALUES (?, ?, ?, ?, 'page_comment_mentioned', ?, ?, ?, ?, ?)",
        )
        .bind(Id::new_v7().to_string())
        .bind(page.workspace_id.to_string())
        .bind(recipient.to_string())
        .bind(actor_id.to_string())
        .bind(page.id.to_string())
        .bind(thread_id.to_string())
        .bind(comment_id.to_string())
        .bind(format!(
            "{}:{recipient}:page_comment_mentioned:{comment_id}",
            page.workspace_id
        ))
        .bind(now.as_millis())
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn record(
    tx: &mut Transaction<'_, Sqlite>,
    page: &VisiblePage,
    actor_id: Id,
    action: &str,
    resource_type: &str,
    resource_id: Id,
    request_id: &str,
    now: TimestampMillis,
) -> Result<(), PageCommentError> {
    audit::record(
        tx,
        page.workspace_id,
        Some(actor_id),
        action,
        AuditOutcome::Success,
        resource_type,
        Some(resource_id),
        request_id,
        json!({ "page_id": page.id.to_string() }),
        now,
    )
    .await?;
    Ok(())
}

async fn load_thread(
    tx: &mut Transaction<'_, Sqlite>,
    page_id: Id,
    thread_id: Id,
) -> Result<PageThread, PageCommentError> {
    load_threads(tx, page_id, Some(thread_id))
        .await?
        .into_iter()
        .next()
        .ok_or(PageCommentError::NotFound)
}

async fn load_comment(
    tx: &mut Transaction<'_, Sqlite>,
    page_id: Id,
    comment_id: Id,
) -> Result<PageComment, PageCommentError> {
    let sql = format!("{COMMENT_SELECT} WHERE page_comments.id = ? AND page_comments.page_id = ?");
    let row = sqlx::query(&sql)
        .bind(comment_id.to_string())
        .bind(page_id.to_string())
        .fetch_optional(&mut **tx)
        .await?
        .ok_or(PageCommentError::NotFound)?;
    let mentions = mentions_of(tx, page_id, Some(comment_id)).await?;
    comment_from_row(row, &mentions)
}

const COMMENT_SELECT: &str = "SELECT page_comments.id AS id, page_comments.thread_id AS thread_id, \
     page_comments.author_id AS author_id, page_comments.body_json AS body_json, \
     page_comments.body_text AS body_text, page_comments.created_at AS created_at, \
     page_comments.updated_at AS updated_at, page_comments.edited_at AS edited_at, \
     page_comments.deleted_at AS deleted_at FROM page_comments";

async fn load_threads(
    tx: &mut Transaction<'_, Sqlite>,
    page_id: Id,
    only: Option<Id>,
) -> Result<Vec<PageThread>, PageCommentError> {
    let only = only.map(|id| id.to_string());
    let thread_sql = format!(
        "SELECT id, page_id, created_by, quote, resolved_at, resolved_by, version, created_at, \
         updated_at FROM page_threads WHERE page_id = ?{} ORDER BY created_at, id",
        if only.is_some() { " AND id = ?" } else { "" }
    );
    let mut query = sqlx::query(&thread_sql).bind(page_id.to_string());
    if let Some(thread_id) = &only {
        query = query.bind(thread_id);
    }
    let threads = query.fetch_all(&mut **tx).await?;
    let comment_sql = format!(
        "{COMMENT_SELECT} WHERE page_comments.page_id = ?{} \
         ORDER BY page_comments.created_at, page_comments.id",
        if only.is_some() {
            " AND page_comments.thread_id = ?"
        } else {
            ""
        }
    );
    let mut query = sqlx::query(&comment_sql).bind(page_id.to_string());
    if let Some(thread_id) = &only {
        query = query.bind(thread_id);
    }
    let rows = query.fetch_all(&mut **tx).await?;
    let mentions = mentions_of(tx, page_id, None).await?;
    let mut comments: HashMap<Id, Vec<PageComment>> = HashMap::new();
    for row in rows {
        let comment = comment_from_row(row, &mentions)?;
        comments.entry(comment.thread_id).or_default().push(comment);
    }
    threads
        .into_iter()
        .map(|row| {
            let id = parse(row.get("id"))?;
            let resolved_at = row
                .get::<Option<i64>, _>("resolved_at")
                .map(TimestampMillis::from_millis);
            Ok(PageThread {
                id,
                page_id: parse(row.get("page_id"))?,
                created_by: parse(row.get("created_by"))?,
                quote: row.get("quote"),
                resolved: resolved_at.is_some(),
                resolved_at,
                resolved_by: parse_optional(row.get("resolved_by"))?,
                version: u64::try_from(row.get::<i64, _>("version"))
                    .map_err(|_| PageCommentError::Corrupt)?,
                created_at: TimestampMillis::from_millis(row.get("created_at")),
                updated_at: TimestampMillis::from_millis(row.get("updated_at")),
                comments: comments.remove(&id).unwrap_or_default(),
            })
        })
        .collect()
}

/// Mentions of the page's comments (or of one comment), by comment.
async fn mentions_of(
    tx: &mut Transaction<'_, Sqlite>,
    page_id: Id,
    comment_id: Option<Id>,
) -> Result<HashMap<Id, Vec<Id>>, PageCommentError> {
    let rows = sqlx::query_as::<_, (String, String)>(
        "SELECT page_comment_mentions.comment_id, page_comment_mentions.user_id \
         FROM page_comment_mentions JOIN page_comments ON page_comments.id = page_comment_mentions.comment_id \
         WHERE page_comments.page_id = ? AND (? IS NULL OR page_comments.id = ?) \
         ORDER BY page_comment_mentions.comment_id, page_comment_mentions.user_id",
    )
    .bind(page_id.to_string())
    .bind(comment_id.map(|id| id.to_string()))
    .bind(comment_id.map(|id| id.to_string()))
    .fetch_all(&mut **tx)
    .await?;
    let mut out: HashMap<Id, Vec<Id>> = HashMap::new();
    for (comment_id, user_id) in rows {
        out.entry(parse(comment_id)?)
            .or_default()
            .push(parse(user_id)?);
    }
    Ok(out)
}

fn comment_from_row(
    row: sqlx::sqlite::SqliteRow,
    mentions: &HashMap<Id, Vec<Id>>,
) -> Result<PageComment, PageCommentError> {
    let id = parse(row.get("id"))?;
    let body: Value =
        serde_json::from_str(row.get("body_json")).map_err(|_| PageCommentError::Corrupt)?;
    Ok(PageComment {
        id,
        thread_id: parse(row.get("thread_id"))?,
        author_id: parse(row.get("author_id"))?,
        body,
        body_text: row.get("body_text"),
        mentioned_user_ids: mentions.get(&id).cloned().unwrap_or_default(),
        created_at: TimestampMillis::from_millis(row.get("created_at")),
        updated_at: TimestampMillis::from_millis(row.get("updated_at")),
        edited_at: row
            .get::<Option<i64>, _>("edited_at")
            .map(TimestampMillis::from_millis),
        deleted_at: row
            .get::<Option<i64>, _>("deleted_at")
            .map(TimestampMillis::from_millis),
    })
}

fn parse(value: String) -> Result<Id, PageCommentError> {
    value.parse().map_err(|_| PageCommentError::Corrupt)
}

fn parse_optional(value: Option<String>) -> Result<Option<Id>, PageCommentError> {
    value.map(parse).transpose()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bodies_keep_paragraph_text_links_and_mentions_only() {
        let user = Id::new_v7();
        let body = CommentBody::parse(json!([
            {
                "id": "a",
                "type": "paragraph",
                "props": { "textColor": "default", "evil": "<x>" },
                "content": [
                    { "type": "text", "text": "Hi ", "styles": { "bold": true, "textColor": "red" } },
                    { "type": "mention", "props": { "userId": user.to_string(), "name": "Ann" } },
                    { "type": "link", "href": "javascript:alert(1)", "content": [{ "type": "text", "text": " x", "styles": {} }] },
                    { "type": "link", "href": "https://example.com", "content": [{ "type": "text", "text": " y", "styles": {} }] },
                    { "type": "image", "props": {} }
                ],
                "children": [{ "type": "paragraph", "content": "nested" }]
            },
            { "type": "heading", "content": [{ "type": "text", "text": "dropped" }] }
        ]))
        .unwrap();
        assert_eq!(body.text(), "Hi @Ann x y");
        assert_eq!(body.mentions(), &[user]);
        assert_eq!(body.blocks.len(), 1);
        let content = body.blocks[0]["content"].as_array().unwrap();
        assert_eq!(content[0]["styles"], json!({ "bold": true }));
        assert_eq!(
            content[2],
            json!({ "type": "text", "text": " x", "styles": {} })
        );
        assert_eq!(content[3]["href"], "https://example.com");
        assert_eq!(body.blocks[0]["props"], json!({ "textColor": "default" }));
        assert_eq!(body.blocks[0]["children"], json!([]));
    }

    #[test]
    fn empty_or_malformed_bodies_are_invalid() {
        for value in [
            json!([]),
            json!({}),
            json!([{ "type": "paragraph", "content": [] }]),
            json!([{ "type": "paragraph", "content": [{ "type": "text", "text": "   " }] }]),
            json!([{ "type": "paragraph", "content": [{ "type": "mention", "props": { "userId": "nope" } }] }]),
        ] {
            assert!(CommentBody::parse(value).is_err());
        }
    }
}
