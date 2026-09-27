//! Page history (migration 0028): snapshots of a page's title, icon and content.
//!
//! Rules:
//! - An edit that changes the title or the content stores the page's *previous* state as an
//!   `auto` version, in the same transaction as the save, when the newest version of the page
//!   is at least [`SNAPSHOT_INTERVAL_MILLIS`] old. A page without versions counts from its
//!   creation, so a new page's first minutes of typing leave no near-empty versions behind.
//! - Restoring a version stores the current state first (`restore`), then replaces the
//!   page's content through [`replace_content`].
//! - A Notion import stores the imported state (`import`).
//! - Blank states (no title, only empty paragraphs) and states equal to the newest version are
//!   never stored.
//! - Retention ([`thin_versions`], run by the `workspace.retention` job): everything from the
//!   last 30 days, then the newest version of each UTC day up to a year, and always the newest
//!   20 of a page.
//! - Duplicates start without history; versions go with their page.
//!
//! Versions are visible exactly like their page: a live page the caller can see.

use orbit_platform::{Id, TimestampMillis};
use serde::Serialize;
use serde_json::{Value, json};
use sqlx::{Row, Sqlite, Transaction};
use utoipa::ToSchema;

use super::page_mentions::notify_new_mentions;
use super::pages::{
    PageError, PageRecord, PageRepository, VISIBLE, check_version, content_text, page_by_id_in_tx,
    page_in_tx, parse_id, user_in_tx,
};
use super::tasks::{record_mutation, require_access, require_access_tx};
use crate::audit::{self, AuditOutcome};
use crate::collab::{CollabHub, CollabWrite};

/// Minimum age of a page's newest version before an edit stores another one.
pub const SNAPSHOT_INTERVAL_MILLIS: i64 = 10 * 60 * 1_000;
const DAY_MILLIS: i64 = 24 * 60 * 60 * 1_000;
/// Versions younger than this are all kept.
const KEEP_ALL_MILLIS: i64 = 30 * DAY_MILLIS;
/// Versions older than this are dropped (unless among the newest [`KEEP_NEWEST`]).
const KEEP_DAILY_MILLIS: i64 = 365 * DAY_MILLIS;
/// The newest versions of a page that retention never removes.
const KEEP_NEWEST: i64 = 20;
pub const DEFAULT_VERSION_LIMIT: usize = 50;
pub const MAX_VERSION_LIMIT: usize = 100;

const SUMMARY_COLUMNS: &str = "page_versions.id AS id, page_versions.page_id AS page_id, \
     page_versions.kind AS kind, page_versions.title AS title, page_versions.icon AS icon, \
     page_versions.created_at AS created_at, page_versions.created_by AS created_by, \
     users.display_name AS display_name";

/// Where a page save comes from; decides how page history records it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SaveOrigin {
    /// A user edit: the previous state may become an `auto` version.
    #[default]
    Edit,
    /// A Notion import filling the page: the imported state becomes an `import` version.
    Import,
}

/// Why a version was stored.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PageVersionKind {
    /// The state before an edit (at most one per 10 minutes of editing).
    Auto,
    /// The state right before a version was restored.
    Restore,
    /// The content a Notion import wrote.
    Import,
}

impl PageVersionKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Restore => "restore",
            Self::Import => "import",
        }
    }

    fn parse(value: &str) -> Result<Self, PageError> {
        match value {
            "auto" => Ok(Self::Auto),
            "restore" => Ok(Self::Restore),
            "import" => Ok(Self::Import),
            _ => Err(PageError::Corrupt),
        }
    }
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct PageVersionAuthor {
    #[schema(value_type = String)]
    pub id: Id,
    pub display_name: String,
}

/// A stored version without its content.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct PageVersionSummary {
    #[schema(value_type = String)]
    pub id: Id,
    #[schema(value_type = String)]
    pub page_id: Id,
    pub kind: PageVersionKind,
    pub title: String,
    #[schema(required = true)]
    pub icon: Option<String>,
    /// When the version was stored; the page looked like this at that moment.
    #[schema(value_type = String, format = DateTime)]
    pub created_at: TimestampMillis,
    /// Who last edited this state (the importer for `import`); `null` when the user is gone.
    #[schema(required = true)]
    pub created_by: Option<PageVersionAuthor>,
}

/// A stored version with its content.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct PageVersion {
    #[schema(value_type = String)]
    pub id: Id,
    #[schema(value_type = String)]
    pub page_id: Id,
    pub kind: PageVersionKind,
    pub title: String,
    #[schema(required = true)]
    pub icon: Option<String>,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: TimestampMillis,
    #[schema(required = true)]
    pub created_by: Option<PageVersionAuthor>,
    /// BlockNote blocks as they were; opaque to the server.
    pub content: Vec<Value>,
}

/// A page's versions, newest first.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct PageVersionList {
    pub items: Vec<PageVersionSummary>,
    /// Pass as `cursor` for the next (older) versions; `null` on the last page.
    #[schema(required = true)]
    pub next_cursor: Option<String>,
}

impl PageRepository {
    /// Versions of a live page the caller can see, newest first. `cursor` is a previous
    /// response's `next_cursor`.
    pub async fn page_versions(
        &self,
        workspace_id: Id,
        page_id: Id,
        actor_id: Id,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<PageVersionList, PageError> {
        let pool = self.database.pool();
        require_access(pool, workspace_id, actor_id).await?;
        let mut tx = pool.begin().await?;
        require_visible_page(&mut tx, workspace_id, page_id, actor_id).await?;
        let (before_at, before_id) = match cursor {
            None => (i64::MAX, String::new()),
            Some(cursor) => {
                let cursor_id = cursor.parse::<Id>().map_err(|_| PageError::InvalidCursor)?;
                let at: i64 = sqlx::query_scalar(
                    "SELECT created_at FROM page_versions WHERE id = ? AND page_id = ?",
                )
                .bind(cursor_id.to_string())
                .bind(page_id.to_string())
                .fetch_optional(&mut *tx)
                .await?
                .ok_or(PageError::InvalidCursor)?;
                (at, cursor_id.to_string())
            }
        };
        let limit = limit.clamp(1, MAX_VERSION_LIMIT);
        let rows = sqlx::query(&format!(
            "SELECT {SUMMARY_COLUMNS} FROM page_versions \
             LEFT JOIN users ON users.id = page_versions.created_by \
             WHERE page_versions.page_id = ? AND page_versions.workspace_id = ? \
             AND (page_versions.created_at, page_versions.id) < (?, ?) \
             ORDER BY page_versions.created_at DESC, page_versions.id DESC LIMIT ?"
        ))
        .bind(page_id.to_string())
        .bind(workspace_id.to_string())
        .bind(before_at)
        .bind(before_id)
        .bind(limit as i64 + 1)
        .fetch_all(&mut *tx)
        .await?;
        tx.rollback().await?;
        let mut items = rows
            .into_iter()
            .map(|row| summary_from_row(&row))
            .collect::<Result<Vec<_>, _>>()?;
        let next_cursor = if items.len() > limit {
            items.truncate(limit);
            items.last().map(|item| item.id.to_string())
        } else {
            None
        };
        Ok(PageVersionList { items, next_cursor })
    }

    /// One version, with content, of a live page the caller can see.
    pub async fn page_version(
        &self,
        workspace_id: Id,
        page_id: Id,
        version_id: Id,
        actor_id: Id,
    ) -> Result<PageVersion, PageError> {
        let pool = self.database.pool();
        require_access(pool, workspace_id, actor_id).await?;
        let mut tx = pool.begin().await?;
        require_visible_page(&mut tx, workspace_id, page_id, actor_id).await?;
        let version = version_in_tx(&mut tx, workspace_id, page_id, version_id).await?;
        tx.rollback().await?;
        Ok(version)
    }

    /// Restores a version's title, icon and content onto the page (cover and place stay). The
    /// current state is stored as a `restore` version first, so the restore can be undone.
    #[allow(clippy::too_many_arguments)]
    pub async fn restore_page_version(
        &self,
        workspace_id: Id,
        page_id: Id,
        version_id: Id,
        actor_id: Id,
        expected_version: u64,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<PageRecord, PageError> {
        // The live document's latest edits are projected first, so the `restore` version holds
        // them; the room stays locked until the replacement is broadcast.
        let mut collab = CollabHub::of(&self.database).write(page_id).await?;
        let mut tx = self.database.immediate_transaction().await?;
        require_access_tx(&mut tx, workspace_id, actor_id).await?;
        let current = page_in_tx(&mut tx, workspace_id, page_id, actor_id, false).await?;
        let version = version_in_tx(&mut tx, workspace_id, page_id, version_id).await?;
        if current.locked_at.is_some() {
            return Err(PageError::Locked);
        }
        check_version(expected_version, current.version, &current)?;
        store_version(
            &mut tx,
            &current,
            PageVersionKind::Restore,
            current.updated_by,
            now,
        )
        .await?;
        replace_content(
            &mut tx,
            ContentReplacement {
                workspace_id,
                page_id,
                actor_id,
                expected_version,
                title: &version.title,
                icon: version.icon.as_deref(),
                content: &version.content,
            },
            &mut collab,
            now,
        )
        .await?;
        audit::record(
            &mut tx,
            workspace_id,
            Some(actor_id),
            "page.restored_version",
            AuditOutcome::Success,
            "page",
            Some(page_id),
            request_id,
            json!({"version_id": version_id}),
            now,
        )
        .await?;
        let updated_by_user = user_in_tx(&mut tx, actor_id).await?;
        tx.commit().await?;
        collab.commit();
        Ok(PageRecord {
            title: version.title,
            icon: version.icon,
            content: version.content,
            version: current.version + 1,
            updated_by: actor_id,
            updated_by_user,
            updated_at: now,
            ..current
        })
    }
}

/// A whole-document replacement written by the server (not typed by a user).
pub struct ContentReplacement<'a> {
    pub workspace_id: Id,
    pub page_id: Id,
    pub actor_id: Id,
    /// The page's version the replacement is based on; the update only applies on it.
    pub expected_version: u64,
    pub title: &'a str,
    pub icon: Option<&'a str>,
    pub content: &'a [Value],
}

/// THE way to replace a live page's whole document on the server: title, icon, content and the
/// search text, bumping the page version. Version restore goes through here; API content
/// writes and imports go through `PageRepository::update_page`, which does the same.
///
/// `collab` is the page's locked collaborative document ([`CollabHub::write`], taken before the
/// transaction began): the new content is applied to it as an ordinary Yjs transaction and
/// logged in `tx`, and connected editors switch to it live once the caller commits and calls
/// [`CollabWrite::commit`].
pub async fn replace_content(
    tx: &mut Transaction<'_, Sqlite>,
    replacement: ContentReplacement<'_>,
    collab: &mut CollabWrite,
    now: TimestampMillis,
) -> Result<(), PageError> {
    let content_json =
        serde_json::to_string(replacement.content).map_err(|_| PageError::Corrupt)?;
    let updated = sqlx::query(
        "UPDATE pages SET title = ?, icon = ?, content_json = ?, content_text = ?, \
         updated_by = ?, updated_at = ?, version = version + 1 \
         WHERE id = ? AND workspace_id = ? AND deleted_at IS NULL AND version = ?",
    )
    .bind(replacement.title)
    .bind(replacement.icon)
    .bind(content_json)
    .bind(content_text(replacement.content))
    .bind(replacement.actor_id.to_string())
    .bind(now.as_millis())
    .bind(replacement.page_id.to_string())
    .bind(replacement.workspace_id.to_string())
    .bind(replacement.expected_version as i64)
    .execute(&mut **tx)
    .await?
    .rows_affected();
    if updated != 1 {
        return Err(PageError::NotFound);
    }
    collab
        .replace(tx, replacement.content, replacement.actor_id, now)
        .await?;
    Ok(())
}

/// THE write path of co-editing: stores the JSON projection of a page's collaborative document
/// as `content_json` / `content_text`. Applies the history rule of edits (the previous state
/// becomes an `auto` version when the newest one is 10 minutes old), sets `updated_by` (the last
/// editor, when known) and `updated_at`, and does not bump `pages.version` (the metadata
/// concurrency token). With `audit`, records `page.updated` (audit + realtime outbox); the hub
/// asks for that at most once per page per 10 minutes. Returns whether the content changed.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn project_collab_content(
    tx: &mut Transaction<'_, Sqlite>,
    page_id: Id,
    content: &[Value],
    editor: Option<Id>,
    audit: bool,
    request_id: &str,
    now: TimestampMillis,
) -> Result<bool, PageError> {
    let current = page_by_id_in_tx(tx, page_id)
        .await?
        .ok_or(PageError::NotFound)?;
    if current.content == content {
        return Ok(false);
    }
    snapshot_before_edit(tx, &current, now).await?;
    sqlx::query(
        "UPDATE pages SET content_json = ?, content_text = ?, updated_by = COALESCE(?, updated_by), \
         updated_at = ? WHERE id = ?",
    )
    .bind(serde_json::to_string(content).map_err(|_| PageError::Corrupt)?)
    .bind(content_text(content))
    .bind(editor.map(|id| id.to_string()))
    .bind(now.as_millis())
    .bind(page_id.to_string())
    .execute(&mut **tx)
    .await?;
    // Members the editor just mentioned hear about it (never for re-projected or moved mentions).
    notify_new_mentions(
        tx,
        page_id,
        &current.content,
        content,
        editor,
        request_id,
        now,
    )
    .await?;
    if audit && let Some(editor) = editor {
        record_mutation(
            tx,
            current.workspace_id,
            editor,
            "page.updated",
            "page",
            page_id,
            request_id,
            now,
        )
        .await?;
    }
    Ok(true)
}

/// Called by `update_page` before it writes an edit that changes the title or content:
/// stores `current` (the state being replaced) as an `auto` version when the page's newest
/// version, or the page itself when it has none, is at least 10 minutes old.
pub(super) async fn snapshot_before_edit(
    tx: &mut Transaction<'_, Sqlite>,
    current: &PageRecord,
    now: TimestampMillis,
) -> Result<(), PageError> {
    let newest: Option<i64> =
        sqlx::query_scalar("SELECT MAX(created_at) FROM page_versions WHERE page_id = ?")
            .bind(current.id.to_string())
            .fetch_one(&mut **tx)
            .await?;
    let since = newest.unwrap_or(current.created_at.as_millis());
    if now.as_millis().saturating_sub(since) < SNAPSHOT_INTERVAL_MILLIS {
        return Ok(());
    }
    store_version(tx, current, PageVersionKind::Auto, current.updated_by, now).await?;
    Ok(())
}

/// Stores `state` as a version unless it is blank or equals the page's newest version.
/// Returns whether a row was written.
pub(super) async fn store_version(
    tx: &mut Transaction<'_, Sqlite>,
    state: &PageRecord,
    kind: PageVersionKind,
    created_by: Id,
    now: TimestampMillis,
) -> Result<bool, PageError> {
    if is_blank(&state.title, &state.content) {
        return Ok(false);
    }
    let newest = sqlx::query(
        "SELECT title, icon, content_json FROM page_versions WHERE page_id = ? \
         ORDER BY created_at DESC, id DESC LIMIT 1",
    )
    .bind(state.id.to_string())
    .fetch_optional(&mut **tx)
    .await?;
    if let Some(newest) = newest {
        let content: Vec<Value> = serde_json::from_str(&newest.get::<String, _>("content_json"))
            .map_err(|_| PageError::Corrupt)?;
        if newest.get::<String, _>("title") == state.title
            && newest.get::<Option<String>, _>("icon") == state.icon
            && content == state.content
        {
            return Ok(false);
        }
    }
    sqlx::query(
        "INSERT INTO page_versions (id, workspace_id, page_id, title, icon, content_json, kind, \
         created_by, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(Id::new_v7().to_string())
    .bind(state.workspace_id.to_string())
    .bind(state.id.to_string())
    .bind(&state.title)
    .bind(&state.icon)
    .bind(serde_json::to_string(&state.content).map_err(|_| PageError::Corrupt)?)
    .bind(kind.as_str())
    .bind(created_by.to_string())
    .bind(now.as_millis())
    .execute(&mut **tx)
    .await?;
    Ok(true)
}

/// Retention for page history; returns how many versions were deleted. Per page: every version
/// younger than 30 days, the newest version of each UTC day up to a year, and the newest 20
/// whatever their age.
pub(crate) async fn thin_versions(
    tx: &mut Transaction<'_, Sqlite>,
    now: TimestampMillis,
) -> Result<u64, sqlx::Error> {
    Ok(sqlx::query(
        "DELETE FROM page_versions WHERE id IN ( \
             SELECT id FROM ( \
                 SELECT id, created_at, \
                 ROW_NUMBER() OVER (PARTITION BY page_id ORDER BY created_at DESC, id DESC) AS recent, \
                 ROW_NUMBER() OVER (PARTITION BY page_id, created_at / ? \
                     ORDER BY created_at DESC, id DESC) AS in_day \
                 FROM page_versions \
             ) WHERE recent > ? AND created_at <= ? AND (created_at <= ? OR in_day > 1) \
         )",
    )
    .bind(DAY_MILLIS)
    .bind(KEEP_NEWEST)
    .bind(now.as_millis().saturating_sub(KEEP_ALL_MILLIS))
    .bind(now.as_millis().saturating_sub(KEEP_DAILY_MILLIS))
    .execute(&mut **tx)
    .await?
    .rows_affected())
}

/// No title and nothing but empty paragraphs: not worth a version.
fn is_blank(title: &str, content: &[Value]) -> bool {
    let empty = |value: Option<&Value>| match value {
        None | Some(Value::Null) => true,
        Some(Value::Array(items)) => items.is_empty(),
        Some(_) => false,
    };
    title.trim().is_empty()
        && content.iter().all(|block| {
            block.get("type").and_then(Value::as_str) == Some("paragraph")
                && empty(block.get("content"))
                && empty(block.get("children"))
        })
}

async fn require_visible_page(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    page_id: Id,
    actor_id: Id,
) -> Result<(), PageError> {
    sqlx::query(&format!(
        "SELECT 1 FROM pages WHERE id = ? AND workspace_id = ? AND deleted_at IS NULL AND {VISIBLE}"
    ))
    .bind(page_id.to_string())
    .bind(workspace_id.to_string())
    .bind(actor_id.to_string())
    .fetch_optional(&mut **tx)
    .await?
    .map(|_| ())
    .ok_or(PageError::NotFound)
}

async fn version_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    page_id: Id,
    version_id: Id,
) -> Result<PageVersion, PageError> {
    let row = sqlx::query(&format!(
        "SELECT {SUMMARY_COLUMNS}, page_versions.content_json AS content_json FROM page_versions \
         LEFT JOIN users ON users.id = page_versions.created_by \
         WHERE page_versions.id = ? AND page_versions.page_id = ? AND page_versions.workspace_id = ?"
    ))
    .bind(version_id.to_string())
    .bind(page_id.to_string())
    .bind(workspace_id.to_string())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(PageError::VersionNotFound)?;
    let summary = summary_from_row(&row)?;
    let content = serde_json::from_str(&row.get::<String, _>("content_json"))
        .map_err(|_| PageError::Corrupt)?;
    Ok(PageVersion {
        id: summary.id,
        page_id: summary.page_id,
        kind: summary.kind,
        title: summary.title,
        icon: summary.icon,
        created_at: summary.created_at,
        created_by: summary.created_by,
        content,
    })
}

fn summary_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<PageVersionSummary, PageError> {
    let created_by = match (
        row.get::<Option<String>, _>("created_by"),
        row.get::<Option<String>, _>("display_name"),
    ) {
        (Some(id), Some(display_name)) => Some(PageVersionAuthor {
            id: parse_id(id)?,
            display_name,
        }),
        _ => None,
    };
    Ok(PageVersionSummary {
        id: parse_id(row.get("id"))?,
        page_id: parse_id(row.get("page_id"))?,
        kind: PageVersionKind::parse(&row.get::<String, _>("kind"))?,
        title: row.get("title"),
        icon: row.get("icon"),
        created_at: TimestampMillis::from_millis(row.get("created_at")),
        created_by,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::is_blank;

    #[test]
    fn blank_means_no_title_and_only_empty_paragraphs() {
        assert!(is_blank("", &[]));
        assert!(is_blank(
            "  ",
            &[json!({"type": "paragraph", "content": [], "children": []})]
        ));
        assert!(!is_blank("Plan", &[]));
        assert!(!is_blank(
            "",
            &[json!({"type": "paragraph", "content": [{"type": "text", "text": "x"}]})]
        ));
        assert!(!is_blank(
            "",
            &[json!({"type": "image", "props": {"url": "https://example.com/a.png"}})]
        ));
    }
}
