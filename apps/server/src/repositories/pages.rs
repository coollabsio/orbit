//! Docs pages: trees of block documents grouped into spaces. A page lives in a shared teamspace
//! (every member reads and edits it) or in one member's private space (only the owner sees it;
//! everyone else gets `NotFound`). A page's space is its root's; sub-pages always share it.
//! Content is one opaque JSON block array per page; `content_text` is extracted for search.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use orbit_platform::Row;
use orbit_platform::{Backend, Database, Flag, Id, TimestampMillis};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{Any, Transaction};
use thiserror::Error;
use utoipa::ToSchema;

use super::billing::{TrialCapacityError, TrialLimitReached, ensure_trial_capacity};
use super::databases::{
    DatabaseSchema, LinkCopy, NOT_TEMPLATE, RowCapacityError, block_links, carry_row_values,
    copy_database_data, copy_links, ensure_row_capacity, insert_schema, is_template,
    linked_databases, row_search_lines,
};
use super::page_files::{page_file_url, page_has_file, parse_page_file_url};
use super::page_mentions::notify_new_mentions;
use super::page_versions::{
    PageVersionKind, SaveOrigin, project_collab_content, snapshot_before_edit, store_version,
};
use super::tasks::{TaskError, record_mutation, require_access, require_access_tx};
use super::teamspaces::{default_teamspace, teamspace_exists};
use super::workspaces::WorkspaceError;
use crate::audit::{self, AuditOutcome};
use crate::billing::TrialLimit;
use crate::collab::markdown_merge::{MAX_BLOCKS, block_count, merge};
use crate::collab::sanitize::sanitize_blocks_for;
use crate::collab::{CollabError, CollabHub, CollabLock, DocKey, DocKind, LinkPolicy};
use crate::export::markdown::LinkResolver;

const TRASH_RETENTION_MILLIS: i64 = 30 * 24 * 60 * 60 * 1_000;
const SEARCH_LIMIT: i64 = 20;
/// At most this many words of a query are searched for.
const SEARCH_MAX_TERMS: usize = 16;
/// Words in a body snippet (FTS5 caps it at 64).
const SNIPPET_TOKENS: i64 = 24;
/// Highlight markers in FTS output; the index strips them from page text (migration 0027).
const MARK_OPEN: char = '\u{2}';
const MARK_CLOSE: char = '\u{3}';
const TITLE_MAX_CHARS: usize = 500;
/// Visits kept per member and workspace (the recent list shows at most `RECENT_MAX_LIMIT`).
const VISITS_KEPT: i64 = 50;
pub const RECENT_DEFAULT_LIMIT: usize = 10;
pub const RECENT_MAX_LIMIT: usize = 50;
const COPY_SUFFIX: &str = " (copy)";
const PAGE_COLUMNS: &str = "id, workspace_id, parent_id, teamspace_id, title, icon, cover_url, cover_position, \
     content_json, position, creator_id, updated_by, version, created_at, updated_at, deleted_at, \
     COALESCE((SELECT epoch FROM page_collab_docs WHERE page_collab_docs.page_id = pages.id), \
     (SELECT generation FROM page_collab_meta WHERE page_collab_meta.id = 1), '') AS collab_epoch, \
     full_width, locked_at, locked_by, kind, \
     (SELECT parent.id FROM pages AS parent WHERE parent.id = pages.parent_id AND parent.kind = 'database') AS database_id, \
     (SELECT display_name FROM users WHERE users.id = pages.updated_by) AS updated_by_name, \
     (SELECT display_name FROM users WHERE users.id = pages.locked_by) AS locked_by_name";
const SUMMARY_COLUMNS: &str = "pages.id AS id, pages.parent_id AS parent_id, pages.teamspace_id AS teamspace_id, \
     pages.title AS title, pages.icon AS icon, pages.position AS position, pages.version AS version, \
     pages.updated_at AS updated_at, pages.kind AS kind";
/// Teamspace pages plus the caller's private pages; binds the caller's user id.
/// `param` is the placeholder number of that bind in the final query.
pub(super) fn visible(param: usize) -> String {
    format!("(pages.teamspace_id IS NOT NULL OR pages.owner_id = ${param})")
}

/// `SELECT` of the strings in the JSON text array bound at placeholder `param`.
pub(super) fn json_text_array(backend: Backend, param: usize) -> String {
    match backend {
        Backend::Sqlite => format!("SELECT value FROM json_each(${param})"),
        Backend::Postgres => format!("SELECT jsonb_array_elements_text(${param}::jsonb)"),
    }
}

/// What a page holds: a block document, or a database of rows (see `repositories::databases`).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, serde::Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PageKind {
    #[default]
    Page,
    Database,
}

impl PageKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Page => "page",
            Self::Database => "database",
        }
    }

    pub(super) fn parse(value: &str) -> Result<Self, PageError> {
        match value {
            "page" => Ok(Self::Page),
            "database" => Ok(Self::Database),
            _ => Err(PageError::Corrupt),
        }
    }
}

/// Where a page lives. `Private` holds the owner's user id.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PageSpace {
    Teamspace(Id),
    Private(Id),
}

impl PageSpace {
    pub(super) fn teamspace_column(self) -> Option<String> {
        match self {
            Self::Teamspace(id) => Some(id.to_string()),
            Self::Private(_) => None,
        }
    }

    pub(super) fn owner_column(self) -> Option<String> {
        match self {
            Self::Teamspace(_) => None,
            Self::Private(owner) => Some(owner.to_string()),
        }
    }

    /// The teamspace, or `None` for a private space.
    #[must_use]
    pub fn teamspace_id(self) -> Option<Id> {
        match self {
            Self::Teamspace(id) => Some(id),
            Self::Private(_) => None,
        }
    }
}

/// The space a client asked for; both fields absent means "no preference".
#[derive(Clone, Copy, Debug, Default)]
pub struct SpaceRequest {
    pub teamspace_id: Option<Id>,
    pub private: Option<bool>,
}

/// A moved page (as it is after the move, or would be after a dry run).
#[derive(Clone, Debug, Serialize, ToSchema)]
#[schema(as = MovedPage)]
pub struct MovedPage {
    #[serde(flatten)]
    pub page: PageRecord,
    /// Property values the move deletes: the page left its database, or the new database has no
    /// property of the same name and type (or no such option) for them.
    pub dropped_values: u64,
}

/// A page with its content.
#[derive(Clone, Debug, Serialize, ToSchema)]
#[schema(as = Page)]
pub struct PageRecord {
    #[schema(value_type = String)]
    pub id: Id,
    #[schema(value_type = String)]
    pub workspace_id: Id,
    #[schema(value_type = Option<String>, required = true)]
    pub parent_id: Option<Id>,
    /// The page's teamspace; `null` for a page in the caller's private space.
    #[schema(value_type = Option<String>, required = true)]
    pub teamspace_id: Option<Id>,
    /// True when the page is in the caller's private space (`teamspace_id` is `null`).
    pub private: bool,
    /// A `database` page has no body; read it with `GET .../databases/{database_id}`.
    pub kind: PageKind,
    /// For a row of a database (a page whose parent is a database page): that database.
    #[schema(value_type = Option<String>, required = true)]
    pub database_id: Option<Id>,
    pub title: String,
    #[schema(required = true)]
    pub icon: Option<String>,
    #[schema(required = true)]
    pub cover_url: Option<String>,
    /// Cover focal point as `"x,y"` percentages.
    #[schema(required = true)]
    pub cover_position: Option<String>,
    /// BlockNote blocks; opaque to the server.
    pub content: Vec<Value>,
    pub position: i64,
    pub version: u64,
    #[schema(value_type = String)]
    pub creator_id: Id,
    #[schema(value_type = String)]
    pub updated_by: Id,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: TimestampMillis,
    #[schema(value_type = String, format = DateTime)]
    pub updated_at: TimestampMillis,
    #[schema(value_type = Option<String>, format = DateTime, required = true)]
    pub deleted_at: Option<TimestampMillis>,
    /// Identifies the page's collaborative document. Pass it as the `epoch` query parameter of
    /// the co-editing socket (`GET /api/v1/workspaces/{workspace_id}/pages/{page_id}/collab`); it
    /// changes when the stored document is reset (backup restore, converter change), and a
    /// socket opened with an older value is closed with 4409.
    pub collab_epoch: String,
    /// The editor column uses the whole pane width.
    pub full_width: bool,
    /// Set while the page is locked: title, icon, cover and content writes answer 423
    /// `page_locked` (REST, version restore, imports) and the co-editing socket ignores content
    /// updates. Moving, trashing, duplicating and `full_width` stay allowed.
    #[schema(value_type = Option<String>, format = DateTime, required = true)]
    pub locked_at: Option<TimestampMillis>,
    /// Who locked the page (`null` when unlocked, or when that user no longer exists).
    #[schema(required = true)]
    pub locked_by: Option<PageUser>,
    /// The last editor (`updated_by`) with their display name.
    pub updated_by_user: PageUser,
}

/// A user named on a page (last editor, locker).
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct PageUser {
    #[schema(value_type = String)]
    pub id: Id,
    pub display_name: String,
}

/// A page the caller opened recently.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct RecentPage {
    #[schema(value_type = String)]
    pub id: Id,
    #[schema(value_type = Option<String>, required = true)]
    pub parent_id: Option<Id>,
    /// `null` for a page in the caller's private space.
    #[schema(value_type = Option<String>, required = true)]
    pub teamspace_id: Option<Id>,
    /// True when the page is in the caller's private space.
    pub private: bool,
    pub title: String,
    #[schema(required = true)]
    pub icon: Option<String>,
    /// When the caller last opened the page.
    #[schema(value_type = String, format = DateTime)]
    pub visited_at: TimestampMillis,
}

/// The caller's recently opened pages in this workspace, most recent first.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct RecentPageList {
    pub items: Vec<RecentPage>,
}

/// Page metadata for the tree, without content.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct PageSummary {
    #[schema(value_type = String)]
    pub id: Id,
    #[schema(value_type = Option<String>, required = true)]
    pub parent_id: Option<Id>,
    /// `null` for a page in the caller's private space.
    #[schema(value_type = Option<String>, required = true)]
    pub teamspace_id: Option<Id>,
    /// True when the page is in the caller's private space.
    pub private: bool,
    pub kind: PageKind,
    pub title: String,
    #[schema(required = true)]
    pub icon: Option<String>,
    pub position: i64,
    pub version: u64,
    #[schema(value_type = String, format = DateTime)]
    pub updated_at: TimestampMillis,
}

/// A page trashed directly; its descendants trashed with it are restored together.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct TrashedPage {
    #[schema(value_type = String)]
    pub id: Id,
    #[schema(value_type = Option<String>, required = true)]
    pub parent_id: Option<Id>,
    /// `null` for a page in the caller's private space.
    #[schema(value_type = Option<String>, required = true)]
    pub teamspace_id: Option<Id>,
    /// True when the page is in the caller's private space.
    pub private: bool,
    /// Whether the caller may delete this page forever.
    pub can_purge: bool,
    pub title: String,
    #[schema(required = true)]
    pub icon: Option<String>,
    pub position: i64,
    pub version: u64,
    #[schema(value_type = String, format = DateTime)]
    pub updated_at: TimestampMillis,
    #[schema(value_type = String, format = DateTime)]
    pub deleted_at: TimestampMillis,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct PageSearchResult {
    #[schema(value_type = String)]
    pub id: Id,
    #[schema(value_type = Option<String>, required = true)]
    pub parent_id: Option<Id>,
    /// `null` for a page in the caller's private space.
    #[schema(value_type = Option<String>, required = true)]
    pub teamspace_id: Option<Id>,
    /// True when the page is in the caller's private space.
    pub private: bool,
    pub title: String,
    #[schema(required = true)]
    pub icon: Option<String>,
    /// Plain body text around the best match (about 24 words, `…` where text was cut), or the
    /// start of the body when only the title matched. A row found by a property value and not by
    /// its body shows that value: `Status: Reading`.
    pub snippet: String,
    /// Matched words in `snippet`.
    pub snippet_highlights: Vec<TextRange>,
    /// Matched words in `title`.
    pub title_highlights: Vec<TextRange>,
    /// For a row of a database: the database's title.
    #[schema(required = true)]
    pub database_title: Option<String>,
}

/// A span of a string in UTF-16 code units (JavaScript string indices): `start` inclusive, `end`
/// exclusive.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, ToSchema)]
pub struct TextRange {
    pub start: u32,
    pub end: u32,
}

/// Result of emptying the trash.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct PageTrashEmptied {
    /// Trash entries (pages trashed directly) deleted forever, sub-pages not counted.
    pub purged: u64,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct PageList {
    pub items: Vec<PageSummary>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct PageTrash {
    pub items: Vec<TrashedPage>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct PageSearch {
    pub items: Vec<PageSearchResult>,
}

/// One of the caller's favorite pages, with what the favorites list shows: rows of databases (and
/// their sub-pages) are not in the page tree. `position` is the index among the caller's visible
/// favorites.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct PageFavorite {
    #[schema(value_type = String)]
    pub page_id: Id,
    pub position: i64,
    pub title: String,
    #[schema(required = true)]
    pub icon: Option<String>,
    pub kind: PageKind,
    /// For a row of a database: that database.
    #[schema(value_type = Option<String>, required = true)]
    pub database_id: Option<Id>,
    /// For a row of a database: the database's title.
    #[schema(required = true)]
    pub database_title: Option<String>,
}

/// The caller's favorites in this workspace, ordered by position.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct PageFavoriteList {
    pub items: Vec<PageFavorite>,
}

#[derive(Clone, Debug)]
pub struct CreatePage {
    /// A database starts with a "Name" title property, a "Tags" multi-select and a "Table" view.
    pub kind: PageKind,
    pub parent_id: Option<Id>,
    pub space: SpaceRequest,
    pub title: String,
    pub icon: Option<String>,
    pub position: Option<i64>,
}

/// `None` leaves a field unchanged; `Some(None)` clears a nullable field.
#[derive(Clone, Debug, Default)]
pub struct PageChanges {
    pub title: Option<String>,
    pub icon: Option<Option<String>>,
    pub cover_url: Option<Option<String>>,
    pub cover_position: Option<Option<String>>,
    pub content: Option<Vec<Value>>,
    /// A layout setting, not an edit: allowed on locked pages, leaves `updated_by` alone.
    pub full_width: Option<bool>,
    /// How page history records the save (a user edit unless said otherwise).
    pub origin: SaveOrigin,
}

/// New content of a page as Markdown (the token API's doc writes). It is merged onto the page's
/// current document (see [`crate::collab::markdown_merge`]): what it leaves unchanged keeps
/// everything Markdown cannot show, and a live document gets only the changed parts.
#[derive(Clone)]
pub struct MarkdownContent {
    pub markdown: String,
    /// `(user id, display name)` of the workspace's members, for new `@Name` mentions.
    pub members: Vec<(Id, String)>,
    /// How the client's read of the page wrote links (sub-page titles); it must be the same, so
    /// unchanged blocks are recognized.
    pub links: Arc<dyn LinkResolver + Send + Sync>,
}

impl fmt::Debug for MarkdownContent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MarkdownContent")
            .field("markdown", &self.markdown)
            .field("members", &self.members.len())
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Error)]
pub enum PageError {
    #[error("page was not found")]
    NotFound,
    /// The page is visible, but it has no such version.
    #[error("page version was not found")]
    VersionNotFound,
    #[error("page version cursor is invalid")]
    InvalidCursor,
    #[error("teamspace was not found")]
    TeamspaceNotFound,
    #[error("page input is invalid: {field}")]
    Invalid { field: &'static str },
    #[error("stale version")]
    VersionConflict { current: Box<Value> },
    /// Deleting teamspace pages forever needs a workspace owner or admin.
    #[error("page action is not permitted")]
    Forbidden,
    /// Moving teamspace pages created by others into a private space needs a workspace owner
    /// or admin.
    #[error("page action is not permitted")]
    MoveToPrivateForbidden,
    /// Only pages in the trash (trashed directly) can be deleted forever.
    #[error("page is not in the trash")]
    NotTrashed,
    /// The page is locked: its title, icon, cover and content cannot change.
    #[error("page is locked")]
    Locked,
    /// A database page cannot be created or moved directly under a database (become a row).
    #[error("a database cannot be a row")]
    DatabaseInDatabase,
    /// Restoring a row whose database is in the trash (or gone): a row cannot live outside its
    /// database, so the database comes back first.
    #[error("the row's database is in the trash")]
    DatabaseTrashed,
    /// Restoring or duplicating a row of a database that holds the most live rows allowed.
    #[error("database holds the most rows allowed")]
    RowLimit,
    #[error("stored page data is invalid")]
    Corrupt,
    /// Orbit Cloud: the trial workspace holds as many pages as the trial allows.
    #[error("trial limit reached")]
    TrialLimit(TrialLimitReached),
    #[error("page repository is unavailable")]
    Unavailable(#[from] sqlx::Error),
    /// Co-editing is at its memory budget, or another server reset the document: try again.
    #[error("the page's co-editing document is busy")]
    Busy,
    /// A Markdown write onto a page with more than [`MAX_BLOCKS`] blocks, or that would give it
    /// more.
    #[error("page content has too many blocks")]
    TooLarge,
}

impl From<CollabError> for PageError {
    fn from(error: CollabError) -> Self {
        match error {
            CollabError::NotFound => Self::NotFound,
            CollabError::Unavailable(error) => Self::Unavailable(error),
            // the memory budget is full, or another server reset the document while this write waited
            CollabError::Overloaded | CollabError::Reset => Self::Busy,
            CollabError::Corrupt(_) => Self::Corrupt,
        }
    }
}

impl From<RowCapacityError> for PageError {
    fn from(error: RowCapacityError) -> Self {
        match error {
            RowCapacityError::Full => Self::RowLimit,
            RowCapacityError::Database(error) => Self::Unavailable(error),
        }
    }
}

impl From<WorkspaceError> for PageError {
    fn from(error: WorkspaceError) -> Self {
        match error {
            WorkspaceError::Unavailable(error) => Self::Unavailable(error),
            WorkspaceError::NotFound => Self::NotFound,
            _ => Self::Corrupt,
        }
    }
}

impl From<TrialCapacityError> for PageError {
    fn from(error: TrialCapacityError) -> Self {
        match error {
            TrialCapacityError::Reached(reached) => Self::TrialLimit(reached),
            TrialCapacityError::Database(error) => Self::Unavailable(error),
        }
    }
}

impl From<TaskError> for PageError {
    fn from(error: TaskError) -> Self {
        match error {
            TaskError::Unavailable(error) => Self::Unavailable(error),
            _ => Self::NotFound,
        }
    }
}

#[derive(Clone)]
pub struct PageRepository {
    pub(super) database: Database,
}

impl PageRepository {
    #[must_use]
    pub fn new(database: Database) -> Self {
        Self { database }
    }

    /// All live pages the caller can see, without database rows and their sub-pages, grouped by
    /// space (teamspaces by position, then the caller's private space) and ordered
    /// `parent_id, position, id` within a space.
    pub async fn pages(&self, workspace_id: Id, actor_id: Id) -> Result<PageList, PageError> {
        require_access(self.database.pool(), workspace_id, actor_id).await?;
        // A database can hold 5,000 rows; the tree would be slow and full.
        let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
            "WITH RECURSIVE in_database(id) AS ( \
                 SELECT row_page.id FROM pages AS row_page JOIN pages AS db ON db.id = row_page.parent_id \
                 WHERE db.workspace_id = $1 AND db.kind = 'database' \
                 UNION ALL \
                 SELECT child.id FROM pages AS child JOIN in_database ON child.parent_id = in_database.id \
             ) \
             SELECT {SUMMARY_COLUMNS} FROM pages \
             LEFT JOIN teamspaces ON teamspaces.id = pages.teamspace_id \
             WHERE pages.workspace_id = $1 AND pages.deleted_at IS NULL AND {visible} \
             AND pages.id NOT IN (SELECT id FROM in_database) \
             ORDER BY pages.teamspace_id IS NULL, teamspaces.position, teamspaces.id, \
             pages.parent_id NULLS FIRST, pages.position, pages.id",
            visible = visible(2)
        )))
        .bind(workspace_id.to_string())
        .bind(actor_id.to_string())
        .fetch_all(self.database.pool())
        .await?;
        Ok(PageList {
            items: rows
                .into_iter()
                .map(summary_from_row)
                .collect::<Result<_, _>>()?,
        })
    }

    /// The live sub-pages of a live page the caller can see, at any depth, parents first: what
    /// trashing, "Duplicate with sub-pages" and "Markdown with sub-pages" take along. As in the
    /// tree, a database counts but its rows (its content) and their sub-pages do not, so a
    /// database has none. Rows and their sub-pages are not in the tree; this is their source.
    pub async fn descendants(
        &self,
        workspace_id: Id,
        page_id: Id,
        actor_id: Id,
    ) -> Result<PageList, PageError> {
        self.get_visible(workspace_id, page_id, actor_id).await?;
        let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
            "WITH RECURSIVE subtree(id, kind, depth) AS ( \
                 SELECT child.id, child.kind, 1 FROM pages AS child \
                 JOIN pages AS root ON root.id = child.parent_id \
                 WHERE root.id = $1 AND root.kind <> 'database' AND child.deleted_at IS NULL \
                 UNION ALL \
                 SELECT pages.id, pages.kind, subtree.depth + 1 FROM pages \
                 JOIN subtree ON pages.parent_id = subtree.id \
                 WHERE pages.deleted_at IS NULL AND subtree.kind <> 'database' \
             ) \
             SELECT {SUMMARY_COLUMNS} FROM pages JOIN subtree ON subtree.id = pages.id \
             WHERE pages.workspace_id = $2 AND {visible} \
             ORDER BY subtree.depth, pages.parent_id, pages.position, pages.id",
            visible = visible(3)
        )))
        .bind(page_id.to_string())
        .bind(workspace_id.to_string())
        .bind(actor_id.to_string())
        .fetch_all(self.database.pool())
        .await?;
        Ok(PageList {
            items: rows
                .into_iter()
                .map(summary_from_row)
                .collect::<Result<_, _>>()?,
        })
    }

    pub async fn get_page(
        &self,
        workspace_id: Id,
        page_id: Id,
        actor_id: Id,
    ) -> Result<PageRecord, PageError> {
        require_access(self.database.pool(), workspace_id, actor_id).await?;
        // Edits of an open document reach `content_json` within seconds; reads see them now.
        if let Some(hub) = CollabHub::existing(&self.database) {
            hub.flush_page(page_id).await;
        }
        let row = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT {PAGE_COLUMNS} FROM pages WHERE id = $1 AND workspace_id = $2 AND deleted_at IS NULL \
             AND {visible}", visible = visible(3)
        )))
        .bind(page_id.to_string())
        .bind(workspace_id.to_string())
        .bind(actor_id.to_string())
        .fetch_optional(self.database.pool())
        .await?
        .ok_or(PageError::NotFound)?;
        page_from_row(row)
    }

    pub async fn create_page(
        &self,
        workspace_id: Id,
        actor_id: Id,
        input: CreatePage,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<PageRecord, PageError> {
        self.create_page_with_id(Id::new_v7(), workspace_id, actor_id, input, request_id, now)
            .await
    }

    /// [`Self::create_page`] with a caller-chosen id, so a resumable job (the Notion import)
    /// can record the id before the page exists and find the page again after a restart.
    pub async fn create_page_with_id(
        &self,
        id: Id,
        workspace_id: Id,
        actor_id: Id,
        input: CreatePage,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<PageRecord, PageError> {
        self.insert_page(
            id,
            workspace_id,
            actor_id,
            input,
            &[],
            None,
            request_id,
            now,
        )
        .await
    }

    /// A database page (`input.kind` is ignored) with `schema` instead of the starter schema,
    /// with a caller-chosen id as [`Self::create_page_with_id`].
    #[allow(clippy::too_many_arguments)]
    pub async fn create_database_with_id(
        &self,
        id: Id,
        workspace_id: Id,
        actor_id: Id,
        input: CreatePage,
        schema: &DatabaseSchema,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<PageRecord, PageError> {
        let input = CreatePage {
            kind: PageKind::Database,
            ..input
        };
        self.insert_page(
            id,
            workspace_id,
            actor_id,
            input,
            &[],
            Some(schema),
            request_id,
            now,
        )
        .await
    }

    /// [`Self::create_page`] with its first content, in the same transaction (one audit event).
    /// Mentioned members are notified as for an edit.
    pub async fn create_page_with_content(
        &self,
        workspace_id: Id,
        actor_id: Id,
        input: CreatePage,
        content: &[Value],
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<PageRecord, PageError> {
        let id = Id::new_v7();
        self.insert_page(
            id,
            workspace_id,
            actor_id,
            input,
            content,
            None,
            request_id,
            now,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn insert_page(
        &self,
        id: Id,
        workspace_id: Id,
        actor_id: Id,
        input: CreatePage,
        content: &[Value],
        schema: Option<&DatabaseSchema>,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<PageRecord, PageError> {
        let content = sanitize_page_blocks(content, workspace_id, id);
        let mut tx = self.database.immediate_transaction().await?;
        require_access_tx(&mut tx, workspace_id, actor_id).await?;
        let imported_rows = schema.map_or(0, DatabaseSchema::row_count) as i64;
        ensure_trial_capacity(
            &self.database,
            &mut tx,
            workspace_id,
            TrialLimit::Pages,
            1 + imported_rows,
        )
        .await?;
        let space = resolve_space(
            &mut tx,
            workspace_id,
            actor_id,
            input.parent_id,
            input.space,
            None,
        )
        .await?;
        // A page created under a database is a row with no values.
        let row_of = match input.parent_id {
            Some(parent_id) if is_database(&mut tx, parent_id).await? => Some(parent_id),
            _ => None,
        };
        if let Some(database_id) = row_of {
            if input.kind == PageKind::Database {
                return Err(PageError::DatabaseInDatabase);
            }
            ensure_row_capacity(&mut tx, database_id, 1).await?;
        }
        let mut siblings = sibling_ids(&mut tx, workspace_id, space, input.parent_id, id).await?;
        let index = insert_index(input.position, siblings.len());
        siblings.insert(index, id);
        let position = index as i64;
        sqlx::query(
            "INSERT INTO pages (id, workspace_id, parent_id, teamspace_id, owner_id, title, icon, position, creator_id, updated_by, version, created_at, updated_at, content_json, content_text, kind) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, 0, $11, $12, $13, $14, $15)",
        )
        .bind(id.to_string())
        .bind(workspace_id.to_string())
        .bind(input.parent_id.map(|parent| parent.to_string()))
        .bind(space.teamspace_column())
        .bind(space.owner_column())
        .bind(&input.title)
        .bind(&input.icon)
        .bind(position)
        .bind(actor_id.to_string())
        .bind(actor_id.to_string())
        .bind(now.as_millis())
        .bind(now.as_millis())
        .bind(serde_json::to_string(&content).map_err(|_| PageError::Corrupt)?)
        .bind(content_text(&content))
        .bind(input.kind.as_str())
        .execute(&mut *tx)
        .await?;
        if input.kind == PageKind::Database {
            insert_schema(&mut tx, workspace_id, id, space, actor_id, schema, now).await?;
        }
        carry_row_values(
            &mut tx,
            workspace_id,
            id,
            None,
            row_of,
            actor_id,
            request_id,
            now,
        )
        .await?;
        renumber(&mut tx, &siblings).await?;
        notify_new_mentions(&mut tx, id, &[], &content, Some(actor_id), request_id, now).await?;
        let collab_epoch = collab_generation(&mut tx).await?;
        let updated_by_user = user_in_tx(&mut tx, actor_id).await?;
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "page.created",
            "page",
            id,
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(PageRecord {
            id,
            workspace_id,
            parent_id: input.parent_id,
            teamspace_id: space.teamspace_id(),
            private: space.teamspace_id().is_none(),
            kind: input.kind,
            database_id: row_of,
            title: input.title,
            icon: input.icon,
            cover_url: None,
            cover_position: None,
            content,
            position,
            version: 0,
            creator_id: actor_id,
            updated_by: actor_id,
            created_at: now,
            updated_at: now,
            deleted_at: None,
            collab_epoch,
            full_width: false,
            locked_at: None,
            locked_by: None,
            updated_by_user,
        })
    }

    /// The space a new page under `parent_id` (or at the root of `space`) would land in, with the
    /// same checks as [`Self::create_page`]: the parent must be live and visible to the actor,
    /// and `space` must not contradict it.
    pub async fn resolve_destination(
        &self,
        workspace_id: Id,
        actor_id: Id,
        parent_id: Option<Id>,
        space: SpaceRequest,
    ) -> Result<PageSpace, PageError> {
        let mut tx = self.database.pool().begin().await?;
        require_access_tx(&mut tx, workspace_id, actor_id).await?;
        let space = resolve_space(&mut tx, workspace_id, actor_id, parent_id, space, None).await?;
        tx.rollback().await?;
        Ok(space)
    }

    /// Moves the live, visible children `ordered` of `parent_id` to the front of its child list
    /// in that order (other children keep their relative order after them). Positions are
    /// renumbered without bumping versions, like a move's sibling renumbering.
    pub async fn order_children(
        &self,
        workspace_id: Id,
        actor_id: Id,
        parent_id: Id,
        ordered: &[Id],
    ) -> Result<(), PageError> {
        let mut tx = self.database.immediate_transaction().await?;
        require_access_tx(&mut tx, workspace_id, actor_id).await?;
        let space = live_page_space(&mut tx, workspace_id, parent_id, actor_id)
            .await?
            .ok_or(PageError::NotFound)?;
        let siblings =
            sibling_ids(&mut tx, workspace_id, space, Some(parent_id), parent_id).await?;
        let mut next: Vec<Id> = Vec::with_capacity(siblings.len());
        for id in ordered {
            if siblings.contains(id) && !next.contains(id) {
                next.push(*id);
            }
        }
        let rest: Vec<Id> = siblings
            .iter()
            .copied()
            .filter(|id| !next.contains(id))
            .collect();
        next.extend(rest);
        renumber(&mut tx, &next).await?;
        tx.commit().await?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update_page(
        &self,
        workspace_id: Id,
        page_id: Id,
        actor_id: Id,
        expected_version: u64,
        changes: PageChanges,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<PageRecord, PageError> {
        self.write_page(
            workspace_id,
            page_id,
            actor_id,
            Expected::Version(expected_version),
            changes,
            None,
            request_id,
            now,
        )
        .await
    }

    /// [`Self::update_page`] that checks the page's [`revision`] instead of its version, so it
    /// also conflicts with co-edits made after the caller's read (the token API's doc writes).
    /// The check runs after the open document's pending edits are stored.
    #[allow(clippy::too_many_arguments)]
    pub async fn update_page_at_revision(
        &self,
        workspace_id: Id,
        page_id: Id,
        actor_id: Id,
        expected_revision: u64,
        changes: PageChanges,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<PageRecord, PageError> {
        self.write_page(
            workspace_id,
            page_id,
            actor_id,
            Expected::Revision(expected_revision),
            changes,
            None,
            request_id,
            now,
        )
        .await
    }

    /// [`Self::update_page_at_revision`] with the content as Markdown, merged onto the page's
    /// current document after the revision check. A write that changes nothing leaves the
    /// content and the live document alone.
    #[allow(clippy::too_many_arguments)]
    pub async fn update_page_markdown_at_revision(
        &self,
        workspace_id: Id,
        page_id: Id,
        actor_id: Id,
        expected_revision: u64,
        changes: PageChanges,
        markdown: MarkdownContent,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<PageRecord, PageError> {
        self.write_page(
            workspace_id,
            page_id,
            actor_id,
            Expected::Revision(expected_revision),
            PageChanges {
                content: None,
                ..changes
            },
            Some(&markdown),
            request_id,
            now,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn write_page(
        &self,
        workspace_id: Id,
        page_id: Id,
        actor_id: Id,
        expected: Expected,
        changes: PageChanges,
        markdown: Option<&MarkdownContent>,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<PageRecord, PageError> {
        let mut changes = changes;
        if let Some(content) = &mut changes.content {
            *content = sanitize_page_blocks(content, workspace_id, page_id);
        }
        // A content write replaces the live collaborative document too: take its room first
        // (never while a transaction is open), after a cheap visibility check.
        let mut collab = if changes.content.is_some() || markdown.is_some() {
            self.get_visible(workspace_id, page_id, actor_id).await?;
            Some(
                CollabHub::of(&self.database)
                    .write(DocKey::page(page_id))
                    .await?,
            )
        } else {
            None
        };
        // Markdown is merged (and a live document changed in memory) before the write
        // transaction, on a blocking thread: it can take a while, and the transaction holds
        // every writer. The room keeps this server's editors out meanwhile; the transaction
        // then checks the revision again, and that no other server changed the document.
        let mut reconciled = false;
        if let Some(markdown) = markdown
            && let Some(mut room) = collab.take()
        {
            let mut read = self.database.transaction().await?;
            require_access_tx(&mut read, workspace_id, actor_id).await?;
            let current = page_in_tx(&mut read, workspace_id, page_id, actor_id, false).await?;
            read.rollback().await?;
            if current.locked_at.is_some() {
                return Err(PageError::Locked);
            }
            expected.check(&current)?;
            let base = match room.current_blocks().await? {
                Some(blocks) => sanitize_page_blocks(&blocks, workspace_id, page_id),
                None => current.content,
            };
            if block_count(&base) > MAX_BLOCKS {
                return Err(PageError::TooLarge);
            }
            let markdown = markdown.clone();
            let (room, merged) = tokio::task::spawn_blocking(move || {
                let merged = merge(
                    &base,
                    &markdown.markdown,
                    &markdown.members,
                    markdown.links.as_ref(),
                );
                let content = sanitize_page_blocks(&merged, workspace_id, page_id);
                let result = if block_count(&content) > MAX_BLOCKS {
                    Err(PageError::TooLarge)
                } else {
                    room.prepare(&content)
                        .map_err(PageError::from)
                        .map(|()| match room.blocks() {
                            Some(blocks) => {
                                (sanitize_page_blocks(&blocks, workspace_id, page_id), true)
                            }
                            None => (content, false),
                        })
                };
                (room, result)
            })
            .await
            .map_err(|_| PageError::Corrupt)?;
            collab = Some(room);
            let (content, live) = merged?;
            reconciled = live;
            changes.content = Some(content);
        }
        let mut tx = self.database.immediate_transaction().await?;
        require_access_tx(&mut tx, workspace_id, actor_id).await?;
        let current = page_in_tx(&mut tx, workspace_id, page_id, actor_id, false).await?;
        let edit = changes.title.is_some()
            || changes.icon.is_some()
            || changes.cover_url.is_some()
            || changes.cover_position.is_some()
            || changes.content.is_some()
            || markdown.is_some();
        if edit && current.locked_at.is_some() {
            return Err(PageError::Locked);
        }
        if current.kind == PageKind::Database && (changes.content.is_some() || markdown.is_some()) {
            return Err(PageError::Invalid { field: "content" });
        }
        expected.check(&current)?;
        if reconciled
            && let Some(collab) = collab.as_mut()
            && !collab.store(&mut tx, actor_id, now).await?
        {
            return Err(expected.conflict(&current));
        }
        if let Some(Some(url)) = &changes.cover_url
            && url.starts_with('/')
        {
            // An uploaded cover must be a file of this very page.
            let valid = match parse_page_file_url(url) {
                Some((workspace, page, file)) if workspace == workspace_id && page == page_id => {
                    page_has_file(&mut tx, workspace_id, page_id, file).await?
                }
                _ => false,
            };
            if !valid {
                return Err(PageError::Invalid { field: "cover_url" });
            }
        }
        let title = changes.title.unwrap_or_else(|| current.title.clone());
        let icon = changes.icon.unwrap_or_else(|| current.icon.clone());
        let cover_url = changes
            .cover_url
            .unwrap_or_else(|| current.cover_url.clone());
        let cover_position = changes
            .cover_position
            .unwrap_or_else(|| current.cover_position.clone());
        let full_width = changes.full_width.unwrap_or(current.full_width);
        let content_changed = changes
            .content
            .as_ref()
            .is_some_and(|content| *content != current.content);
        // A token API write that changes nothing keeps the version, so other clients that hold
        // it can still write; it is not audited either.
        if matches!(expected, Expected::Revision(_))
            && !content_changed
            && title == current.title
            && icon == current.icon
            && cover_url == current.cover_url
            && cover_position == current.cover_position
            && full_width == current.full_width
        {
            tx.commit().await?;
            if let Some(collab) = collab {
                collab.commit();
            }
            return Ok(current);
        }
        if changes.origin == SaveOrigin::Edit && (content_changed || title != current.title) {
            snapshot_before_edit(&mut tx, &current, now).await?;
        }
        let (content_json, text) = match &changes.content {
            Some(content) if content_changed => (
                Some(serde_json::to_string(content).map_err(|_| PageError::Corrupt)?),
                Some(content_text(content)),
            ),
            _ => (None, None),
        };
        // A layout-only change (full width) is not an edit: the last editor and time stay.
        let (updated_by, updated_at) = if edit {
            (actor_id, now)
        } else {
            (current.updated_by, current.updated_at)
        };
        sqlx::query(
            "UPDATE pages SET title = $1, icon = $2, cover_url = $3, cover_position = $4, \
             content_json = COALESCE($5, content_json), content_text = COALESCE($6, content_text), \
             full_width = $7, updated_by = $8, updated_at = $9, version = version + 1 \
             WHERE id = $10 AND workspace_id = $11 AND deleted_at IS NULL AND version = $12",
        )
        .bind(&title)
        .bind(&icon)
        .bind(&cover_url)
        .bind(&cover_position)
        .bind(content_json)
        .bind(text)
        .bind(i64::from(full_width))
        .bind(updated_by.to_string())
        .bind(updated_at.as_millis())
        .bind(page_id.to_string())
        .bind(workspace_id.to_string())
        .bind(current.version as i64)
        .execute(&mut *tx)
        .await?;
        if let (Some(collab), Some(content)) = (collab.as_mut(), &changes.content)
            && content_changed
            && !reconciled
        {
            collab.replace(&mut tx, content, actor_id, now).await?;
        }
        if changes.origin == SaveOrigin::Edit
            && content_changed
            && let Some(content) = &changes.content
        {
            notify_new_mentions(
                &mut tx,
                page_id,
                &current.content,
                content,
                Some(actor_id),
                request_id,
                now,
            )
            .await?;
        }
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "page.updated",
            "page",
            page_id,
            request_id,
            now,
        )
        .await?;
        let updated_by_user = if edit {
            user_in_tx(&mut tx, actor_id).await?
        } else {
            current.updated_by_user.clone()
        };
        let updated = PageRecord {
            title,
            icon,
            cover_url,
            cover_position,
            content: changes.content.unwrap_or(current.content),
            version: current.version + 1,
            full_width,
            updated_by,
            updated_by_user,
            updated_at,
            ..current
        };
        if changes.origin == SaveOrigin::Import {
            store_version(&mut tx, &updated, PageVersionKind::Import, actor_id, now).await?;
        }
        tx.commit().await?;
        if let Some(collab) = collab {
            collab.commit();
        }
        Ok(updated)
    }

    /// Fails with `NotFound` unless the page is live and visible to a member.
    pub(super) async fn get_visible(
        &self,
        workspace_id: Id,
        page_id: Id,
        actor_id: Id,
    ) -> Result<(), PageError> {
        require_access(self.database.pool(), workspace_id, actor_id).await?;
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT 1 FROM pages WHERE id = $1 AND workspace_id = $2 AND deleted_at IS NULL AND {visible}", visible = visible(3)
        )))
        .bind(page_id.to_string())
        .bind(workspace_id.to_string())
        .bind(actor_id.to_string())
        .fetch_optional(self.database.pool())
        .await?
        .map(|_| ())
        .ok_or(PageError::NotFound)
    }

    /// Moves a page under `parent_id` (root when `None`) at index `position` among its new
    /// siblings. With a parent the page joins the parent's space; at the root it goes to the
    /// requested space, or stays in its own. The whole subtree follows a space change. Sibling
    /// positions are renumbered without bumping their versions, so a reorder never makes a
    /// concurrent content save conflict. A page that enters a database becomes its last row; a
    /// row that changes database keeps only the values the new one can hold (see
    /// [`carry_row_values`]). `dry_run` does all of it, then rolls back.
    #[allow(clippy::too_many_arguments)]
    pub async fn move_page(
        &self,
        workspace_id: Id,
        page_id: Id,
        actor_id: Id,
        expected_version: u64,
        parent_id: Option<Id>,
        space: SpaceRequest,
        position: i64,
        dry_run: bool,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<MovedPage, PageError> {
        let mut tx = self.database.immediate_transaction().await?;
        let actor = require_access_tx(&mut tx, workspace_id, actor_id).await?;
        let current = page_in_tx(&mut tx, workspace_id, page_id, actor_id, false).await?;
        check_version(expected_version, current.version, &current)?;
        // A row template stays a hidden page of its database.
        if parent_id == Some(page_id) || is_template(&mut tx, page_id).await? {
            return Err(PageError::Invalid { field: "parent_id" });
        }
        let database_id = match parent_id {
            Some(parent_id) if is_database(&mut tx, parent_id).await? => Some(parent_id),
            _ => None,
        };
        let joins_database = database_id.is_some() && database_id != current.database_id;
        if let Some(database_id) = database_id {
            if current.kind == PageKind::Database {
                return Err(PageError::DatabaseInDatabase);
            }
            if joins_database {
                ensure_row_capacity(&mut tx, database_id, 1).await?;
            }
        }
        let current_space = current.space(actor_id);
        let space = resolve_space(
            &mut tx,
            workspace_id,
            actor_id,
            parent_id,
            space,
            Some(current_space),
        )
        .await?;
        let to_private = matches!(
            (current_space, space),
            (PageSpace::Teamspace(_), PageSpace::Private(_))
        );
        if to_private
            && !actor
                .can_move_pages_to_private(created_whole_subtree(&mut tx, page_id, actor_id).await?)
        {
            return Err(PageError::MoveToPrivateForbidden);
        }
        if let Some(parent_id) = parent_id
            && is_ancestor(&mut tx, page_id, parent_id).await?
        {
            return Err(PageError::Invalid { field: "parent_id" });
        }
        let old_siblings = sibling_ids(
            &mut tx,
            workspace_id,
            current_space,
            current.parent_id,
            page_id,
        )
        .await?;
        renumber(&mut tx, &old_siblings).await?;
        let mut siblings = sibling_ids(&mut tx, workspace_id, space, parent_id, page_id).await?;
        // A new row goes last in the database's manual order.
        let index = insert_index((!joins_database).then_some(position), siblings.len());
        siblings.insert(index, page_id);
        sqlx::query(
            "UPDATE pages SET parent_id = $1, teamspace_id = $2, owner_id = $3, position = $4, \
             updated_by = $5, updated_at = $6, version = version + 1 \
             WHERE id = $7 AND workspace_id = $8 AND deleted_at IS NULL AND version = $9",
        )
        .bind(parent_id.map(|parent| parent.to_string()))
        .bind(space.teamspace_column())
        .bind(space.owner_column())
        .bind(index as i64)
        .bind(actor_id.to_string())
        .bind(now.as_millis())
        .bind(page_id.to_string())
        .bind(workspace_id.to_string())
        .bind(expected_version as i64)
        .execute(&mut *tx)
        .await?;
        if space != current_space {
            move_descendants(&mut tx, page_id, space).await?;
        }
        renumber(&mut tx, &siblings).await?;
        let dropped_values = carry_row_values(
            &mut tx,
            workspace_id,
            page_id,
            current.database_id,
            database_id,
            actor_id,
            request_id,
            now,
        )
        .await?;
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "page.moved",
            "page",
            page_id,
            request_id,
            now,
        )
        .await?;
        if let (true, Some(teamspace_id)) = (to_private, current.teamspace_id) {
            audit::record(
                &mut tx,
                workspace_id,
                Some(actor_id),
                "page.moved_to_private",
                AuditOutcome::Success,
                "page",
                Some(page_id),
                request_id,
                json!({"from_teamspace_id": teamspace_id.to_string()}),
                now,
            )
            .await?;
        }
        let updated_by_user = user_in_tx(&mut tx, actor_id).await?;
        let page = PageRecord {
            parent_id,
            teamspace_id: space.teamspace_id(),
            private: space.teamspace_id().is_none(),
            database_id,
            position: index as i64,
            version: current.version + 1,
            updated_by: actor_id,
            updated_by_user,
            updated_at: now,
            ..current
        };
        if dry_run {
            // Nothing is kept, so nothing is audited and no realtime event goes out.
            tx.rollback().await?;
        } else {
            tx.commit().await?;
            // A page (and its subtree) moved into someone's private space closes other editors.
            CollabHub::revalidate_database(&self.database, Some(workspace_id));
        }
        Ok(MovedPage {
            page,
            dropped_values,
        })
    }

    /// Trashes the page and its live subtree as one batch (`trashed_with` = this page).
    #[allow(clippy::too_many_arguments)]
    pub async fn delete_page(
        &self,
        workspace_id: Id,
        page_id: Id,
        actor_id: Id,
        expected_version: u64,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), PageError> {
        let mut tx = self.database.immediate_transaction().await?;
        require_access_tx(&mut tx, workspace_id, actor_id).await?;
        let current = page_in_tx(&mut tx, workspace_id, page_id, actor_id, false).await?;
        check_version(expected_version, current.version, &current)?;
        sqlx::query(
            "WITH RECURSIVE subtree(id) AS ( \
                 SELECT $1 \
                 UNION ALL \
                 SELECT pages.id FROM pages JOIN subtree ON pages.parent_id = subtree.id \
                 WHERE pages.deleted_at IS NULL \
             ) \
             UPDATE pages SET deleted_at = $2, trashed_with = $3, updated_by = $4, updated_at = $5, version = version + 1 \
             WHERE workspace_id = $6 AND deleted_at IS NULL AND id IN (SELECT id FROM subtree)",
        )
        .bind(page_id.to_string())
        .bind(now.as_millis())
        .bind(page_id.to_string())
        .bind(actor_id.to_string())
        .bind(now.as_millis())
        .bind(workspace_id.to_string())
        .execute(&mut *tx)
        .await?;
        let siblings = sibling_ids(
            &mut tx,
            workspace_id,
            current.space(actor_id),
            current.parent_id,
            page_id,
        )
        .await?;
        renumber(&mut tx, &siblings).await?;
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "page.deleted",
            "page",
            page_id,
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        CollabHub::revalidate_database(&self.database, Some(workspace_id));
        // The trash already committed: a failed cleanup only leaves "Missing page" cards behind.
        if let Err(error) = self
            .unlink_trashed(workspace_id, page_id, actor_id, request_id, now)
            .await
        {
            tracing::warn!(error = %error, "removing links to a trashed page");
        }
        Ok(())
    }

    /// Removes the `page` blocks that link to the batch trashed with `page_id` from every live
    /// page of the workspace, in storage and in open editors.
    async fn unlink_trashed(
        &self,
        workspace_id: Id,
        page_id: Id,
        actor_id: Id,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), PageError> {
        let trashed: Vec<String> = sqlx::query_scalar(
            "SELECT id FROM pages WHERE workspace_id = $1 AND trashed_with = $2 AND deleted_at = $3",
        )
        .bind(workspace_id.to_string())
        .bind(page_id.to_string())
        .bind(now.as_millis())
        .fetch_all(self.database.pool())
        .await?;
        let hub = CollabHub::of(&self.database);
        // Pending edits are projected first, so `content_json` shows links added seconds ago.
        hub.flush_workspace(workspace_id).await;
        let mut linking = std::collections::BTreeSet::new();
        for id in &trashed {
            let contains = match self.database.backend() {
                Backend::Sqlite => "instr(content_json, $2) > 0",
                Backend::Postgres => "strpos(content_json, $2) > 0",
            };
            let rows: Vec<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
                "SELECT id FROM pages WHERE workspace_id = $1 AND deleted_at IS NULL AND {contains}"
            )))
            .bind(workspace_id.to_string())
            .bind(format!("\"pageId\":\"{id}\""))
            .fetch_all(self.database.pool())
            .await?;
            linking.extend(rows);
        }
        for id in linking {
            let Ok(linking_id) = id.parse::<Id>() else {
                continue;
            };
            // The room is taken before the transaction (see `CollabHub`).
            let mut collab = hub.write(DocKey::page(linking_id)).await?;
            let mut tx = self.database.immediate_transaction().await?;
            let Some(current) = page_by_id_in_tx(&mut tx, linking_id).await? else {
                continue;
            };
            let content = without_page_blocks(&current.content, &trashed);
            if content == current.content {
                continue;
            }
            project_collab_content(&mut tx, linking_id, &content, None, false, request_id, now)
                .await?;
            collab.replace(&mut tx, &content, actor_id, now).await?;
            tx.commit().await?;
            collab.commit();
        }
        Ok(())
    }

    /// Restores the page and the descendants trashed in the same batch. A page whose parent is
    /// no longer live goes to the root of its own space.
    #[allow(clippy::too_many_arguments)]
    pub async fn restore_page(
        &self,
        workspace_id: Id,
        page_id: Id,
        actor_id: Id,
        expected_version: u64,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<PageRecord, PageError> {
        let mut tx = self.database.immediate_transaction().await?;
        require_access_tx(&mut tx, workspace_id, actor_id).await?;
        let current = page_in_tx(&mut tx, workspace_id, page_id, actor_id, true).await?;
        if current.deleted_at.is_none_or(|deleted| {
            deleted.as_millis() <= now.as_millis().saturating_sub(TRASH_RETENTION_MILLIS)
        }) {
            return Err(PageError::NotFound);
        }
        check_version(expected_version, current.version, &current)?;
        let batch: String =
            sqlx::query_scalar("SELECT COALESCE(trashed_with, id) FROM pages WHERE id = $1")
                .bind(page_id.to_string())
                .fetch_one(&mut *tx)
                .await?;
        let restored: i64 = sqlx::query_scalar(
            "WITH RECURSIVE subtree(id) AS ( \
                 SELECT $1 \
                 UNION ALL \
                 SELECT pages.id FROM pages JOIN subtree ON pages.parent_id = subtree.id \
                 WHERE pages.deleted_at IS NOT NULL AND pages.trashed_with = $2 \
             ) \
             SELECT CAST(COUNT(*) AS BIGINT) FROM pages \
             WHERE workspace_id = $3 AND deleted_at IS NOT NULL AND id IN (SELECT id FROM subtree)",
        )
        .bind(page_id.to_string())
        .bind(&batch)
        .bind(workspace_id.to_string())
        .fetch_one(&mut *tx)
        .await?;
        ensure_trial_capacity(
            &self.database,
            &mut tx,
            workspace_id,
            TrialLimit::Pages,
            restored,
        )
        .await?;
        let space = current.space(actor_id);
        let parent_id = match current.parent_id {
            Some(parent_id)
                if live_page_space(&mut tx, workspace_id, parent_id, actor_id).await?
                    == Some(space) =>
            {
                Some(parent_id)
            }
            Some(parent_id) if is_database(&mut tx, parent_id).await? => {
                return Err(PageError::DatabaseTrashed);
            }
            _ => None,
        };
        if current.database_id.is_some()
            && let Some(database_id) = parent_id
        {
            ensure_row_capacity(&mut tx, database_id, 1).await?;
        }
        let mut siblings = sibling_ids(&mut tx, workspace_id, space, parent_id, page_id).await?;
        let index = insert_index(Some(current.position), siblings.len());
        siblings.insert(index, page_id);
        sqlx::query(
            "WITH RECURSIVE subtree(id) AS ( \
                 SELECT $1 \
                 UNION ALL \
                 SELECT pages.id FROM pages JOIN subtree ON pages.parent_id = subtree.id \
                 WHERE pages.deleted_at IS NOT NULL AND pages.trashed_with = $2 \
             ) \
             UPDATE pages SET deleted_at = NULL, trashed_with = NULL, updated_by = $3, updated_at = $4, version = version + 1 \
             WHERE workspace_id = $5 AND deleted_at IS NOT NULL AND id IN (SELECT id FROM subtree)",
        )
        .bind(page_id.to_string())
        .bind(&batch)
        .bind(actor_id.to_string())
        .bind(now.as_millis())
        .bind(workspace_id.to_string())
        .execute(&mut *tx)
        .await?;
        sqlx::query("UPDATE pages SET parent_id = $1, position = $2 WHERE id = $3")
            .bind(parent_id.map(|parent| parent.to_string()))
            .bind(index as i64)
            .bind(page_id.to_string())
            .execute(&mut *tx)
            .await?;
        renumber(&mut tx, &siblings).await?;
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "page.restored",
            "page",
            page_id,
            request_id,
            now,
        )
        .await?;
        let updated_by_user = user_in_tx(&mut tx, actor_id).await?;
        tx.commit().await?;
        Ok(PageRecord {
            parent_id,
            position: index as i64,
            version: current.version + 1,
            updated_by: actor_id,
            updated_by_user,
            updated_at: now,
            deleted_at: None,
            ..current
        })
    }

    /// Pages trashed directly (not along with an ancestor), newest first.
    pub async fn page_trash(
        &self,
        workspace_id: Id,
        actor_id: Id,
        now: TimestampMillis,
    ) -> Result<PageTrash, PageError> {
        let actor = require_access(self.database.pool(), workspace_id, actor_id).await?;
        let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT {SUMMARY_COLUMNS}, pages.deleted_at AS deleted_at FROM pages \
             WHERE pages.workspace_id = $1 AND pages.deleted_at > $2 AND pages.trashed_with = pages.id \
             AND {visible} ORDER BY pages.deleted_at DESC, pages.id DESC", visible = visible(3)
        )))
        .bind(workspace_id.to_string())
        .bind(now.as_millis().saturating_sub(TRASH_RETENTION_MILLIS))
        .bind(actor_id.to_string())
        .fetch_all(self.database.pool())
        .await?;
        let items = rows
            .into_iter()
            .map(|row| {
                let deleted_at = TimestampMillis::from_millis(row.get("deleted_at"));
                let summary = summary_from_row(row)?;
                Ok(TrashedPage {
                    id: summary.id,
                    parent_id: summary.parent_id,
                    teamspace_id: summary.teamspace_id,
                    private: summary.private,
                    can_purge: actor.can_purge_page(summary.private),
                    title: summary.title,
                    icon: summary.icon,
                    position: summary.position,
                    version: summary.version,
                    updated_at: summary.updated_at,
                    deleted_at,
                })
            })
            .collect::<Result<_, PageError>>()?;
        Ok(PageTrash { items })
    }

    /// Deletes a page in the trash forever, with the sub-pages trashed along with it. Only pages
    /// trashed directly qualify (`NotTrashed` otherwise, and for live pages). A private page can
    /// only be seen, and so purged, by its owner; teamspace pages need a workspace owner or admin.
    /// Its files, favorites and search entry go with it (cascades and triggers).
    #[allow(clippy::too_many_arguments)]
    pub async fn purge_page(
        &self,
        workspace_id: Id,
        page_id: Id,
        actor_id: Id,
        expected_version: u64,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), PageError> {
        let mut tx = self.database.immediate_transaction().await?;
        let actor = require_access_tx(&mut tx, workspace_id, actor_id).await?;
        let row = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT deleted_at IS NOT NULL AND trashed_with IS NOT DISTINCT FROM id AS in_trash FROM pages \
             WHERE id = $1 AND workspace_id = $2 AND {visible}",
            visible = visible(3)
        )))
        .bind(page_id.to_string())
        .bind(workspace_id.to_string())
        .bind(actor_id.to_string())
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(PageError::NotFound)?;
        if !row.get::<bool, _>("in_trash") {
            return Err(PageError::NotTrashed);
        }
        let current = page_in_tx(&mut tx, workspace_id, page_id, actor_id, true).await?;
        if !actor.can_purge_page(current.teamspace_id.is_none()) {
            return Err(PageError::Forbidden);
        }
        check_version(expected_version, current.version, &current)?;
        let purged = purge_batch(&mut tx, page_id).await?;
        audit::record(
            &mut tx,
            workspace_id,
            Some(actor_id),
            "page.purged",
            AuditOutcome::Success,
            "page",
            Some(page_id),
            request_id,
            json!({"pages": purged}),
            now,
        )
        .await?;
        tx.commit().await?;
        CollabHub::revalidate_database(&self.database, Some(workspace_id));
        Ok(())
    }

    /// Deletes forever every trashed page the caller may purge: their own private pages, plus
    /// teamspace pages for workspace owners and admins. Members' teamspace trash is left alone.
    pub async fn empty_trash(
        &self,
        workspace_id: Id,
        actor_id: Id,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<PageTrashEmptied, PageError> {
        let mut tx = self.database.immediate_transaction().await?;
        let actor = require_access_tx(&mut tx, workspace_id, actor_id).await?;
        let roots = sqlx::query_scalar::<_, String>(
            "SELECT id FROM pages WHERE workspace_id = $1 AND deleted_at IS NOT NULL \
             AND trashed_with = id AND (owner_id = $2 OR ($3 AND teamspace_id IS NOT NULL)) \
             ORDER BY deleted_at, id",
        )
        .bind(workspace_id.to_string())
        .bind(actor_id.to_string())
        .bind(actor.can_purge_page(false))
        .fetch_all(&mut *tx)
        .await?
        .into_iter()
        .map(parse_id)
        .collect::<Result<Vec<_>, _>>()?;
        for root in &roots {
            let purged = purge_batch(&mut tx, *root).await?;
            audit::record(
                &mut tx,
                workspace_id,
                Some(actor_id),
                "page.purged",
                AuditOutcome::Success,
                "page",
                Some(*root),
                request_id,
                json!({"pages": purged}),
                now,
            )
            .await?;
        }
        tx.commit().await?;
        CollabHub::revalidate_database(&self.database, Some(workspace_id));
        Ok(PageTrashEmptied {
            purged: roots.len() as u64,
        })
    }

    /// Copies a live page the caller can see (with `include_children`, its whole live subtree in
    /// order) into the same space, the copy placed right after the original. The new root is
    /// titled "<title> (copy)". Files are shared, not copied: each copy gets its own
    /// `page_files` rows for the same blobs, and file URLs in content and covers, `page` blocks
    /// and `/docs/<id>` links that point inside the copied pages are rewritten to the copies.
    pub async fn duplicate_page(
        &self,
        workspace_id: Id,
        page_id: Id,
        actor_id: Id,
        include_children: bool,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<PageRecord, PageError> {
        // Open documents first write their latest edits into `content_json`, which is copied.
        if let Some(hub) = CollabHub::existing(&self.database) {
            hub.flush_workspace(workspace_id).await;
        }
        let mut tx = self.database.immediate_transaction().await?;
        require_access_tx(&mut tx, workspace_id, actor_id).await?;
        let source = page_in_tx(&mut tx, workspace_id, page_id, actor_id, false).await?;
        let collab_epoch = collab_generation(&mut tx).await?;
        let actor = user_in_tx(&mut tx, actor_id).await?;
        let space = source.space(actor_id);
        // A copied row is one more row of the same database; its sub-pages are not rows.
        if let Some(database_id) = source.database_id {
            ensure_row_capacity(&mut tx, database_id, 1).await?;
        }
        // A database's rows are its content: they are always copied.
        let include_children = include_children || source.kind == PageKind::Database;
        let mut originals = vec![source];
        if include_children {
            let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
                "WITH RECURSIVE subtree(page_id, depth) AS ( \
                     SELECT $1, 0 \
                     UNION ALL \
                     SELECT pages.id, subtree.depth + 1 FROM pages \
                     JOIN subtree ON pages.parent_id = subtree.page_id WHERE pages.deleted_at IS NULL \
                 ) \
                 SELECT {PAGE_COLUMNS} FROM pages JOIN subtree ON subtree.page_id = pages.id \
                 WHERE subtree.depth > 0 ORDER BY subtree.depth, pages.parent_id, pages.position, pages.id"
            )))
            .bind(page_id.to_string())
            .fetch_all(&mut *tx)
            .await?;
            for row in rows {
                originals.push(page_from_row(row)?);
            }
        }
        let copies = i64::try_from(originals.len()).unwrap_or(i64::MAX);
        ensure_trial_capacity(
            &self.database,
            &mut tx,
            workspace_id,
            TrialLimit::Pages,
            copies,
        )
        .await?;
        let pages: HashMap<Id, Id> = originals
            .iter()
            .map(|page| (page.id, Id::new_v7()))
            .collect();
        let mut files = HashMap::new();
        let mut file_rows = Vec::new();
        for page in &originals {
            let rows = sqlx::query(
                "SELECT id, blob_id, file_name, mime_type, size_bytes FROM page_files \
                 WHERE page_id = $1 AND workspace_id = $2 ORDER BY created_at, id",
            )
            .bind(page.id.to_string())
            .bind(workspace_id.to_string())
            .fetch_all(&mut *tx)
            .await?;
            for row in rows {
                let file_id = parse_id(row.get("id"))?;
                let copy = (pages[&page.id], Id::new_v7());
                files.insert((page.id, file_id), copy);
                file_rows.push((copy, row));
            }
        }
        // Each linked view block gets its own link, with copies of the link's views.
        let mut link_copies = Vec::new();
        for original in &originals {
            let found = block_links(&original.content);
            for (link_id, database_id) in linked_databases(&mut tx, workspace_id, &found).await? {
                if !link_copies
                    .iter()
                    .any(|copy: &LinkCopy| copy.link_id == link_id)
                {
                    link_copies.push(LinkCopy {
                        link_id,
                        copy_id: Id::new_v7(),
                        database_id,
                        host_page_id: pages[&original.id],
                    });
                }
            }
        }
        let links: HashMap<Id, Id> = link_copies
            .iter()
            .map(|copy| (copy.link_id, copy.copy_id))
            .collect();
        let remap = Remap {
            workspace_id,
            pages: &pages,
            files: &files,
            links: &links,
        };

        let mut child_positions: HashMap<Id, i64> = HashMap::new();
        let mut root = None;
        for (index, original) in originals.iter().enumerate() {
            let id = pages[&original.id];
            let (parent_id, title, position) = if index == 0 {
                (original.parent_id, copy_title(&original.title), 0)
            } else {
                let parent = original
                    .parent_id
                    .and_then(|parent| pages.get(&parent).copied());
                let parent = parent.ok_or(PageError::Corrupt)?;
                let next = child_positions.entry(parent).or_default();
                *next += 1;
                (Some(parent), original.title.clone(), *next - 1)
            };
            let mut content = Value::Array(original.content.clone());
            remap.value(&mut content, None);
            let Value::Array(content) = content else {
                return Err(PageError::Corrupt);
            };
            let cover_url = original
                .cover_url
                .as_ref()
                .map(|url| remap.url(url).unwrap_or_else(|| url.clone()));
            sqlx::query(
                "INSERT INTO pages (id, workspace_id, parent_id, teamspace_id, owner_id, title, icon, \
                 cover_url, cover_position, content_json, content_text, position, creator_id, updated_by, \
                 version, created_at, updated_at, full_width, kind) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, 0, $15, $16, $17, $18)",
            )
            .bind(id.to_string())
            .bind(workspace_id.to_string())
            .bind(parent_id.map(|parent| parent.to_string()))
            .bind(space.teamspace_column())
            .bind(space.owner_column())
            .bind(&title)
            .bind(&original.icon)
            .bind(&cover_url)
            .bind(&original.cover_position)
            .bind(serde_json::to_string(&content).map_err(|_| PageError::Corrupt)?)
            .bind(content_text(&content))
            .bind(position)
            .bind(actor_id.to_string())
            .bind(actor_id.to_string())
            .bind(now.as_millis())
            .bind(now.as_millis())
            .bind(i64::from(original.full_width))
            .bind(original.kind.as_str())
            .execute(&mut *tx)
            .await?;
            audit::record(
                &mut tx,
                workspace_id,
                Some(actor_id),
                "page.duplicated",
                AuditOutcome::Success,
                "page",
                Some(id),
                request_id,
                json!({"source_page_id": original.id}),
                now,
            )
            .await?;
            if index == 0 {
                root = Some(PageRecord {
                    id,
                    workspace_id,
                    parent_id,
                    teamspace_id: space.teamspace_id(),
                    private: space.teamspace_id().is_none(),
                    kind: original.kind,
                    database_id: original.database_id,
                    title,
                    icon: original.icon.clone(),
                    cover_url,
                    cover_position: original.cover_position.clone(),
                    content,
                    position,
                    version: 0,
                    creator_id: actor_id,
                    updated_by: actor_id,
                    created_at: now,
                    updated_at: now,
                    deleted_at: None,
                    collab_epoch: collab_epoch.clone(),
                    // A copy keeps the layout but is never locked.
                    full_width: original.full_width,
                    locked_at: None,
                    locked_by: None,
                    updated_by_user: actor.clone(),
                });
            }
        }
        for ((page, file), row) in file_rows {
            sqlx::query(
                "INSERT INTO page_files (id, workspace_id, page_id, blob_id, file_name, mime_type, \
                 size_bytes, uploaded_by, created_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
            )
            .bind(file.to_string())
            .bind(workspace_id.to_string())
            .bind(page.to_string())
            .bind(row.get::<String, _>("blob_id"))
            .bind(row.get::<String, _>("file_name"))
            .bind(row.get::<String, _>("mime_type"))
            .bind(row.get::<i64, _>("size_bytes"))
            .bind(actor_id.to_string())
            .bind(now.as_millis())
            .execute(&mut *tx)
            .await?;
        }

        let databases: Vec<Id> = originals
            .iter()
            .filter(|page| page.kind == PageKind::Database)
            .map(|page| page.id)
            .collect();
        let property_ids =
            copy_database_data(&mut tx, workspace_id, &databases, &pages, actor_id, now).await?;
        copy_links(
            &mut tx,
            workspace_id,
            &link_copies,
            &pages,
            &property_ids,
            actor_id,
            now,
        )
        .await?;

        // The copy goes right after the original among the original's siblings.
        let mut root = root.ok_or(PageError::Corrupt)?;
        let mut siblings =
            sibling_ids(&mut tx, workspace_id, space, root.parent_id, root.id).await?;
        let index = siblings
            .iter()
            .position(|id| *id == page_id)
            .map_or(siblings.len(), |index| index + 1);
        siblings.insert(index, root.id);
        renumber(&mut tx, &siblings).await?;
        root.position = index as i64;
        tx.commit().await?;
        Ok(root)
    }

    /// Full-text search (FTS5, migrations 0027 and 0083) over the titles, body text and property
    /// values of live pages the caller can see. Every word must match (the last one as a prefix);
    /// case and diacritics are ignored. Ranked by BM25 with title matches weighted 10x and
    /// property values 2x body matches.
    pub async fn search_pages(
        &self,
        workspace_id: Id,
        actor_id: Id,
        query: &str,
    ) -> Result<PageSearch, PageError> {
        require_access(self.database.pool(), workspace_id, actor_id).await?;
        let postgres = self.database.backend() == Backend::Postgres;
        let expression = if postgres {
            pg_tsquery(query)
        } else {
            fts_query(query)
        };
        let Some(expression) = expression else {
            return Ok(PageSearch { items: Vec::new() });
        };
        let sql = if postgres {
            format!(
                "SELECT pages.id AS id, pages.parent_id AS parent_id, pages.teamspace_id AS teamspace_id, \
                 pages.title AS title, pages.icon AS icon, \
                 ts_headline('orbit_search', page_search.title, q.query, \
                 'HighlightAll=true, StartSel=' || chr(2) || ', StopSel=' || chr(3)) AS marked_title, \
                 ts_headline('orbit_search', page_search.body, q.query, \
                 'MaxWords=' || $1 || ', MinWords=' || greatest($1 / 2, 1) \
                 || ', ShortWord=0, StartSel=' || chr(2) || ', StopSel=' || chr(3)) AS marked_snippet, \
                 page_search.body AS body, \
                 ts_headline('orbit_search', coalesce(page_search.props, ''), q.query, \
                 'HighlightAll=true, StartSel=' || chr(2) || ', StopSel=' || chr(3)) AS marked_props, \
                 parent.title AS database_title \
                 FROM page_search \
                 CROSS JOIN LATERAL (SELECT to_tsquery('orbit_search', $2) AS query) AS q \
                 JOIN page_search_rows ON page_search_rows.id = page_search.rowid \
                 JOIN pages ON pages.id = page_search_rows.page_id \
                 LEFT JOIN pages AS parent ON parent.id = pages.parent_id AND parent.kind = 'database' \
                 WHERE page_search.search_vector @@ q.query \
                 AND pages.workspace_id = $3 AND pages.deleted_at IS NULL AND {visible} AND {NOT_TEMPLATE} \
                 ORDER BY ts_rank('{{0, 0.2, 0.1, 1}}', page_search.search_vector, q.query) DESC, \
                 pages.updated_at DESC, pages.id DESC LIMIT $5",
                visible = visible(4)
            )
        } else {
            format!(
                "SELECT pages.id AS id, pages.parent_id AS parent_id, pages.teamspace_id AS teamspace_id, \
             pages.title AS title, pages.icon AS icon, \
             highlight(page_search, 0, char(2), char(3)) AS marked_title, \
             snippet(page_search, 1, char(2), char(3), '…', $1) AS marked_snippet, \
             highlight(page_search, 2, char(2), char(3)) AS marked_props, \
             parent.title AS database_title \
             FROM page_search \
             JOIN page_search_rows ON page_search_rows.id = page_search.rowid \
             JOIN pages ON pages.id = page_search_rows.page_id \
             LEFT JOIN pages AS parent ON parent.id = pages.parent_id AND parent.kind = 'database' \
             WHERE page_search MATCH $2 AND pages.workspace_id = $3 AND pages.deleted_at IS NULL \
             AND {visible} AND {NOT_TEMPLATE} \
             ORDER BY bm25(page_search, 10.0, 1.0, 2.0), pages.updated_at DESC, pages.id DESC LIMIT $5",
                visible = visible(4)
            )
        };
        let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(SNIPPET_TOKENS)
            .bind(&expression)
            .bind(workspace_id.to_string())
            .bind(actor_id.to_string())
            .bind(SEARCH_LIMIT)
            .fetch_all(self.database.pool())
            .await?;
        // A row found by a value and not by its body shows that value, named by its property.
        let mut property_hits = Vec::new();
        for row in &rows {
            let marked_props: String = row.get("marked_props");
            let marked_snippet: String = row.get("marked_snippet");
            if !marked_snippet.contains(MARK_OPEN) && marked_props.contains(MARK_OPEN) {
                property_hits.push(parse_id(row.get("id"))?);
            }
        }
        let property_lines = row_search_lines(&self.database, &property_hits)
            .await
            .map_err(|_| PageError::Corrupt)?;
        let items = rows
            .into_iter()
            .map(|row| {
                let teamspace_id = parse_optional_id(row.get("teamspace_id"))?;
                let id = parse_id(row.get("id"))?;
                let title: String = row.get("title");
                let (marked_title, title_highlights) =
                    split_marks(&row.get::<String, _>("marked_title"));
                let mut marked_snippet: String = row.get("marked_snippet");
                if postgres {
                    marked_snippet = with_ellipses(marked_snippet, &row.get::<String, _>("body"));
                }
                let (mut snippet, mut snippet_highlights) = split_marks(&marked_snippet);
                if let Some(lines) = property_lines.get(&id)
                    && let Some((value, snippet_value)) =
                        property_snippet(&row.get::<String, _>("marked_props"), lines)
                {
                    (snippet, snippet_highlights) = (value, snippet_value);
                }
                Ok(PageSearchResult {
                    id,
                    parent_id: parse_optional_id(row.get("parent_id"))?,
                    teamspace_id,
                    private: teamspace_id.is_none(),
                    // The index copy drops the marker characters; its ranges only fit an equal title.
                    title_highlights: if marked_title == title {
                        title_highlights
                    } else {
                        Vec::new()
                    },
                    title,
                    icon: row.get("icon"),
                    snippet,
                    snippet_highlights,
                    database_title: row.get("database_title"),
                })
            })
            .collect::<Result<_, PageError>>()?;
        Ok(PageSearch { items })
    }
}

/// Favorites are the caller's own ordered shortcuts. They are not audited and emit no realtime
/// event. Only favorites of live pages the caller can see are listed; the rest keep their row, so
/// a restored page (or one moved back out of someone else's private space) returns in place.
impl PageRepository {
    pub async fn favorites(
        &self,
        workspace_id: Id,
        actor_id: Id,
    ) -> Result<PageFavoriteList, PageError> {
        require_access(self.database.pool(), workspace_id, actor_id).await?;
        let mut connection = self.database.pool().acquire().await?;
        let items = visible_favorites(&mut connection, workspace_id, actor_id).await?;
        Ok(PageFavoriteList { items })
    }

    /// Adds the page as the caller's last favorite; an existing favorite is left in place.
    pub async fn add_favorite(
        &self,
        workspace_id: Id,
        page_id: Id,
        actor_id: Id,
        now: TimestampMillis,
    ) -> Result<PageFavorite, PageError> {
        let mut tx = self.database.immediate_transaction().await?;
        require_access_tx(&mut tx, workspace_id, actor_id).await?;
        page_in_tx(&mut tx, workspace_id, page_id, actor_id, false).await?;
        let mut order = favorite_order(&mut tx, workspace_id, actor_id).await?;
        if !order.contains(&page_id) {
            sqlx::query(
                "INSERT INTO page_favorites (workspace_id, user_id, page_id, position, created_at) \
                 VALUES ($1, $2, $3, $4, $5)",
            )
            .bind(workspace_id.to_string())
            .bind(actor_id.to_string())
            .bind(page_id.to_string())
            .bind(order.len() as i64)
            .bind(now.as_millis())
            .execute(&mut *tx)
            .await?;
            order.push(page_id);
            renumber_favorites(&mut tx, actor_id, &order).await?;
        }
        let visible = visible_favorites(&mut tx, workspace_id, actor_id).await?;
        tx.commit().await?;
        visible
            .into_iter()
            .find(|favorite| favorite.page_id == page_id)
            .ok_or(PageError::NotFound)
    }

    /// Removes the page from the caller's favorites; removing a non-favorite is a no-op.
    pub async fn remove_favorite(
        &self,
        workspace_id: Id,
        page_id: Id,
        actor_id: Id,
    ) -> Result<(), PageError> {
        let mut tx = self.database.immediate_transaction().await?;
        require_access_tx(&mut tx, workspace_id, actor_id).await?;
        page_in_tx(&mut tx, workspace_id, page_id, actor_id, false).await?;
        let removed = sqlx::query(
            "DELETE FROM page_favorites WHERE workspace_id = $1 AND user_id = $2 AND page_id = $3",
        )
        .bind(workspace_id.to_string())
        .bind(actor_id.to_string())
        .bind(page_id.to_string())
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if removed > 0 {
            let order = favorite_order(&mut tx, workspace_id, actor_id).await?;
            renumber_favorites(&mut tx, actor_id, &order).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Moves a favorite to index `position` among the caller's visible favorites (clamped to the
    /// end). Hidden favorites keep their place relative to their visible neighbours.
    pub async fn move_favorite(
        &self,
        workspace_id: Id,
        page_id: Id,
        actor_id: Id,
        position: i64,
    ) -> Result<PageFavoriteList, PageError> {
        let mut tx = self.database.immediate_transaction().await?;
        require_access_tx(&mut tx, workspace_id, actor_id).await?;
        page_in_tx(&mut tx, workspace_id, page_id, actor_id, false).await?;
        let mut order = favorite_order(&mut tx, workspace_id, actor_id).await?;
        let Some(current) = order.iter().position(|id| *id == page_id) else {
            return Err(PageError::NotFound);
        };
        order.remove(current);
        let visible = visible_favorites(&mut tx, workspace_id, actor_id)
            .await?
            .into_iter()
            .map(|favorite| favorite.page_id)
            .filter(|id| *id != page_id)
            .collect::<Vec<_>>();
        let index = insert_index(Some(position), visible.len());
        let at = match visible.get(index) {
            Some(next) => order
                .iter()
                .position(|id| id == next)
                .unwrap_or(order.len()),
            None => visible
                .last()
                .and_then(|last| order.iter().position(|id| id == last))
                .map_or(order.len(), |last| last + 1),
        };
        order.insert(at, page_id);
        renumber_favorites(&mut tx, actor_id, &order).await?;
        let items = visible_favorites(&mut tx, workspace_id, actor_id).await?;
        tx.commit().await?;
        Ok(PageFavoriteList { items })
    }
}

/// Page lock: any member who can see the page may lock or unlock it (audited `page.locked` /
/// `page.unlocked`). The lock is enforced on every content path (see `PageRecord::locked_at`).
impl PageRepository {
    /// Locks (`locked`) or unlocks the page. Setting the state the page already has changes
    /// nothing (same version, no audit). A change bumps the version and closes the page's
    /// co-editing sockets with 4423 so every editor reloads the lock state.
    pub async fn set_lock(
        &self,
        workspace_id: Id,
        page_id: Id,
        actor_id: Id,
        locked: bool,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<PageRecord, PageError> {
        self.get_visible(workspace_id, page_id, actor_id).await?;
        // The room is held across the transaction, so no update lands between commit and flag.
        let room: CollabLock = CollabHub::of(&self.database)
            .lock_change(DocKey::page(page_id))
            .await?;
        let mut tx = self.database.immediate_transaction().await?;
        require_access_tx(&mut tx, workspace_id, actor_id).await?;
        let current = page_in_tx(&mut tx, workspace_id, page_id, actor_id, false).await?;
        if current.locked_at.is_some() == locked {
            tx.rollback().await?;
            return Ok(current);
        }
        let (locked_at, locked_by) = if locked {
            (Some(now), Some(user_in_tx(&mut tx, actor_id).await?))
        } else {
            (None, None)
        };
        sqlx::query(
            "UPDATE pages SET locked_at = $1, locked_by = $2, version = version + 1 \
             WHERE id = $3 AND workspace_id = $4 AND deleted_at IS NULL",
        )
        .bind(locked_at.map(TimestampMillis::as_millis))
        .bind(locked_by.as_ref().map(|user| user.id.to_string()))
        .bind(page_id.to_string())
        .bind(workspace_id.to_string())
        .execute(&mut *tx)
        .await?;
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            if locked {
                "page.locked"
            } else {
                "page.unlocked"
            },
            "page",
            page_id,
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        room.commit(locked);
        Ok(PageRecord {
            version: current.version + 1,
            locked_at,
            locked_by,
            ..current
        })
    }
}

/// Recent pages: when each member last opened a page (`POST .../visit`, sent by the web app when
/// a page is opened, not by background reads). Private to the member, not audited, no realtime
/// event. Only live pages the member can see now are listed, so a visited page that later moved
/// into someone else's private space never shows up.
impl PageRepository {
    pub async fn record_visit(
        &self,
        workspace_id: Id,
        page_id: Id,
        actor_id: Id,
        now: TimestampMillis,
    ) -> Result<(), PageError> {
        let mut tx = self.database.immediate_transaction().await?;
        require_access_tx(&mut tx, workspace_id, actor_id).await?;
        page_in_tx(&mut tx, workspace_id, page_id, actor_id, false).await?;
        sqlx::query(
            "INSERT INTO page_visits (workspace_id, user_id, page_id, visited_at) VALUES ($1, $2, $3, $4) \
             ON CONFLICT (user_id, page_id) DO UPDATE SET visited_at = CASE \
             WHEN excluded.visited_at > page_visits.visited_at THEN excluded.visited_at \
             ELSE page_visits.visited_at END",
        )
        .bind(workspace_id.to_string())
        .bind(actor_id.to_string())
        .bind(page_id.to_string())
        .bind(now.as_millis())
        .execute(&mut *tx)
        .await?;
        // Keep the newest few per member and workspace.
        sqlx::query(
            "DELETE FROM page_visits WHERE workspace_id = $1 AND user_id = $2 AND page_id NOT IN ( \
                 SELECT page_id FROM page_visits WHERE workspace_id = $1 AND user_id = $2 \
                 ORDER BY visited_at DESC, page_id LIMIT $3)",
        )
        .bind(workspace_id.to_string())
        .bind(actor_id.to_string())
        .bind(VISITS_KEPT)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// The caller's most recently opened live, visible pages, newest first.
    pub async fn recent_pages(
        &self,
        workspace_id: Id,
        actor_id: Id,
        limit: usize,
    ) -> Result<RecentPageList, PageError> {
        require_access(self.database.pool(), workspace_id, actor_id).await?;
        let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT pages.id AS id, pages.parent_id AS parent_id, pages.teamspace_id AS teamspace_id, \
             pages.title AS title, pages.icon AS icon, page_visits.visited_at AS visited_at \
             FROM page_visits JOIN pages ON pages.id = page_visits.page_id \
             WHERE page_visits.workspace_id = $1 AND page_visits.user_id = $2 \
             AND pages.workspace_id = page_visits.workspace_id AND pages.deleted_at IS NULL AND {visible} \
             AND {NOT_TEMPLATE} ORDER BY page_visits.visited_at DESC, pages.id LIMIT $4", visible = visible(3)
        )))
        .bind(workspace_id.to_string())
        .bind(actor_id.to_string())
        .bind(actor_id.to_string())
        .bind(i64::try_from(limit).unwrap_or(i64::MAX))
        .fetch_all(self.database.pool())
        .await?;
        let items = rows
            .into_iter()
            .map(|row| {
                let teamspace_id = parse_optional_id(row.get("teamspace_id"))?;
                Ok(RecentPage {
                    id: parse_id(row.get("id"))?,
                    parent_id: parse_optional_id(row.get("parent_id"))?,
                    teamspace_id,
                    private: teamspace_id.is_none(),
                    title: row.get("title"),
                    icon: row.get("icon"),
                    visited_at: TimestampMillis::from_millis(row.get("visited_at")),
                })
            })
            .collect::<Result<_, PageError>>()?;
        Ok(RecentPageList { items })
    }
}

/// Every favorite of the caller in the workspace (hidden ones too), in order.
async fn favorite_order(
    tx: &mut Transaction<'_, Any>,
    workspace_id: Id,
    actor_id: Id,
) -> Result<Vec<Id>, PageError> {
    sqlx::query_scalar::<_, String>(
        "SELECT page_id FROM page_favorites WHERE workspace_id = $1 AND user_id = $2 \
         ORDER BY position, page_id",
    )
    .bind(workspace_id.to_string())
    .bind(actor_id.to_string())
    .fetch_all(&mut **tx)
    .await?
    .into_iter()
    .map(parse_id)
    .collect()
}

/// The caller's favorites of live pages they can see, in order.
async fn visible_favorites(
    connection: &mut sqlx::AnyConnection,
    workspace_id: Id,
    actor_id: Id,
) -> Result<Vec<PageFavorite>, PageError> {
    let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT page_favorites.page_id AS page_id, pages.title AS title, pages.icon AS icon, \
         pages.kind AS kind, parent.id AS database_id, parent.title AS database_title \
         FROM page_favorites JOIN pages ON pages.id = page_favorites.page_id \
         LEFT JOIN pages AS parent ON parent.id = pages.parent_id AND parent.kind = 'database' \
         WHERE page_favorites.workspace_id = $1 AND page_favorites.user_id = $2 \
         AND pages.workspace_id = page_favorites.workspace_id AND pages.deleted_at IS NULL \
         AND {visible} AND {NOT_TEMPLATE} ORDER BY page_favorites.position, page_favorites.page_id",
        visible = visible(3)
    )))
    .bind(workspace_id.to_string())
    .bind(actor_id.to_string())
    .bind(actor_id.to_string())
    .fetch_all(connection)
    .await?;
    rows.into_iter()
        .enumerate()
        .map(|(position, row)| {
            Ok(PageFavorite {
                page_id: parse_id(row.get("page_id"))?,
                position: position as i64,
                title: row.get("title"),
                icon: row.get("icon"),
                kind: PageKind::parse(row.get("kind"))?,
                database_id: parse_optional_id(row.get("database_id"))?,
                database_title: row.get("database_title"),
            })
        })
        .collect()
}

async fn renumber_favorites(
    tx: &mut Transaction<'_, Any>,
    actor_id: Id,
    ids: &[Id],
) -> Result<(), PageError> {
    for (position, id) in ids.iter().enumerate() {
        let position = position as i64;
        sqlx::query(
            "UPDATE page_favorites SET position = $1 WHERE user_id = $2 AND page_id = $3 \
             AND position <> $4",
        )
        .bind(position)
        .bind(actor_id.to_string())
        .bind(id.to_string())
        .bind(position)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

/// Collects every string under a `"text"` key, in document order, joined with spaces; a mention
/// contributes `@<name>` (so search finds who a page mentions).
#[must_use]
/// Blocks from a REST write or a stored version, cleaned the way the live document cleans them, so
/// `content_json` never holds a link or file URL that the editor would drop.
pub(crate) fn sanitize_page_blocks(content: &[Value], workspace_id: Id, page_id: Id) -> Vec<Value> {
    let policy = LinkPolicy {
        kind: DocKind::Page,
        workspace_id,
        doc_id: page_id,
    };
    sanitize_blocks_for(content, &policy)
}

pub fn content_text(content: &[Value]) -> String {
    let mut parts = Vec::new();
    for block in content {
        collect_text(block, &mut parts);
    }
    parts.join(" ")
}

fn collect_text<'a>(value: &'a Value, parts: &mut Vec<std::borrow::Cow<'a, str>>) {
    match value {
        Value::Array(items) => {
            for item in items {
                collect_text(item, parts);
            }
        }
        Value::Object(map) if map.get("type").and_then(Value::as_str) == Some("mention") => {
            if let Some(name) = map
                .get("props")
                .and_then(|props| props.get("name"))
                .and_then(Value::as_str)
                .filter(|name| !name.trim().is_empty())
            {
                parts.push(format!("@{name}").into());
            }
        }
        Value::Object(map) => {
            // Object keys are sorted, so walk a block's own text before its children.
            if let Some(Value::String(text)) = map.get("text")
                && !text.is_empty()
            {
                parts.push(text.as_str().into());
            }
            if let Some(content) = map.get("content") {
                collect_text(content, parts);
            }
            for (key, value) in map {
                if !matches!(key.as_str(), "text" | "content" | "children") {
                    collect_text(value, parts);
                }
            }
            if let Some(children) = map.get("children") {
                collect_text(children, parts);
            }
        }
        _ => {}
    }
}

/// Builds an FTS5 query from user input without exposing FTS syntax: every whitespace-separated
/// word becomes a quoted string (so `AND`, `NEAR`, `col:`, `-`, `*`, `^` and parentheses are plain
/// text), words without a letter or digit are dropped (the tokenizer would drop them too), the
/// last word matches as a prefix, and all words must match. `None` when nothing is searchable.
pub(crate) fn fts_query(input: &str) -> Option<String> {
    let terms = input
        .split_whitespace()
        // `"` separates tokens anyway; inside the quoted string it would end it.
        .map(|term| term.replace('"', " "))
        .filter(|term| term.chars().any(char::is_alphanumeric))
        .take(SEARCH_MAX_TERMS)
        .collect::<Vec<_>>();
    let last = terms.len().checked_sub(1)?;
    Some(
        terms
            .iter()
            .enumerate()
            .map(|(index, term)| {
                if index == last {
                    format!("\"{term}\"*")
                } else {
                    format!("\"{term}\"")
                }
            })
            .collect::<Vec<_>>()
            .join(" "),
    )
}

/// Postgres counterpart of [`fts_query`]: a `to_tsquery('orbit_search', …)` string. Every
/// whitespace-separated term is split into alphanumeric runs like the unicode61 tokenizer; the
/// runs of one term form a phrase, terms are ANDed, and the last run is a prefix. The pieces are
/// purely alphanumeric, so they cannot inject tsquery syntax. `None` when nothing is searchable.
pub(crate) fn pg_tsquery(input: &str) -> Option<String> {
    let terms: Vec<Vec<String>> = input
        .split_whitespace()
        .map(|term| {
            term.split(|c: char| !c.is_alphanumeric())
                .filter(|part| !part.is_empty())
                .map(str::to_lowercase)
                .collect::<Vec<_>>()
        })
        .filter(|parts| !parts.is_empty())
        .take(SEARCH_MAX_TERMS)
        .collect();
    let last = terms.len().checked_sub(1)?;
    Some(
        terms
            .iter()
            .enumerate()
            .map(|(index, parts)| {
                let phrase = parts.join(" <-> ");
                if index == last {
                    format!("{phrase}:*")
                } else {
                    phrase
                }
            })
            .collect::<Vec<_>>()
            .join(" & "),
    )
}

/// Adds `…` where a Postgres `ts_headline` excerpt cuts `body`, like FTS5 `snippet()`.
fn with_ellipses(marked: String, body: &str) -> String {
    let plain: String = marked
        .chars()
        .filter(|c| !matches!(c, '\u{2}' | '\u{3}'))
        .collect();
    if plain.is_empty() {
        return marked;
    }
    let start = if body.starts_with(&plain) { "" } else { "…" };
    let end = if body.ends_with(&plain) { "" } else { "…" };
    format!("{start}{marked}{end}")
}

/// The first property value with a match in a row's marked `props` index text, as `Name: value`
/// with the matched words' ranges. `lines` are the row's search lines, which name the property of
/// each line of the index text.
fn property_snippet(
    marked_props: &str,
    lines: &[(String, String)],
) -> Option<(String, Vec<TextRange>)> {
    let marked = marked_props.lines().find(|line| line.contains(MARK_OPEN))?;
    let (value, ranges) = split_marks(marked);
    let (name, _) = lines.iter().find(|(_, text)| *text == value)?;
    let prefix = format!("{name}: ");
    let shift = prefix.encode_utf16().count() as u32;
    let ranges = ranges
        .into_iter()
        .map(|range| TextRange {
            start: range.start + shift,
            end: range.end + shift,
        })
        .collect();
    Some((format!("{prefix}{value}"), ranges))
}

/// Removes the highlight markers from FTS output and returns the plain text with the marked
/// spans as UTF-16 ranges.
pub(crate) fn split_marks(marked: &str) -> (String, Vec<TextRange>) {
    let mut text = String::with_capacity(marked.len());
    let mut ranges = Vec::new();
    let mut offset = 0_u32;
    let mut open = None;
    for c in marked.chars() {
        match c {
            MARK_OPEN => open = Some(offset),
            MARK_CLOSE => {
                if let Some(start) = open.take()
                    && offset > start
                {
                    ranges.push(TextRange { start, end: offset });
                }
            }
            c => {
                text.push(c);
                offset += c.len_utf16() as u32;
            }
        }
    }
    (text, ranges)
}

/// Hard-deletes the trashed page `root_id` and the pages trashed along with it; returns how many
/// pages went. Sub-pages trashed on their own earlier keep their own trash entry (their
/// `parent_id` is cleared by the foreign key), like restore treats them.
async fn purge_batch(tx: &mut Transaction<'_, Any>, root_id: Id) -> Result<u64, PageError> {
    let ids = sqlx::query_scalar::<_, String>(
        "WITH RECURSIVE subtree(id) AS ( \
             SELECT $1 \
             UNION ALL \
             SELECT pages.id FROM pages JOIN subtree ON pages.parent_id = subtree.id \
             WHERE pages.deleted_at IS NOT NULL AND pages.trashed_with = $2 \
         ) \
         SELECT id FROM subtree",
    )
    .bind(root_id.to_string())
    .bind(root_id.to_string())
    .fetch_all(&mut **tx)
    .await?;
    let mut purged = 0;
    // Children first, so no row of the batch is left pointing at a deleted parent mid-way.
    for id in ids.iter().rev() {
        purged += sqlx::query("DELETE FROM pages WHERE id = $1 AND deleted_at IS NOT NULL")
            .bind(id)
            .execute(&mut **tx)
            .await?
            .rows_affected();
    }
    Ok(purged)
}

/// "<title> (copy)", keeping the result within the title limit; "Untitled (copy)" when empty.
fn copy_title(title: &str) -> String {
    let base = if title.trim().is_empty() {
        "Untitled"
    } else {
        title
    };
    let keep = TITLE_MAX_CHARS - COPY_SUFFIX.chars().count();
    let mut copy = base.chars().take(keep).collect::<String>();
    copy.push_str(COPY_SUFFIX);
    copy
}

/// Rewrites references into a duplicated set of pages to their copies.
pub(super) struct Remap<'a> {
    pub(super) workspace_id: Id,
    /// Original page id → copy id.
    pub(super) pages: &'a HashMap<Id, Id>,
    /// (original page id, original file id) → (copy page id, copy file id).
    pub(super) files: &'a HashMap<(Id, Id), (Id, Id)>,
    /// Original `database_links` id → copy id.
    pub(super) links: &'a HashMap<Id, Id>,
}

impl Remap<'_> {
    /// Walks block JSON: `pageId` props of `page` blocks, `databaseId` and `linkId` props of
    /// `database` blocks, page-file URLs (image/file `url` props and anything else holding one)
    /// and `/docs/<id>` link targets. Text is left alone.
    pub(super) fn value(&self, value: &mut Value, key: Option<&str>) {
        match value {
            Value::Array(items) => {
                for item in items {
                    self.value(item, None);
                }
            }
            Value::Object(map) => {
                for (key, item) in map.iter_mut() {
                    self.value(item, Some(key));
                }
            }
            Value::String(text) => match key {
                Some("text") => {}
                Some("pageId" | "databaseId") => {
                    if let Some(copy) = text.parse::<Id>().ok().and_then(|id| self.pages.get(&id)) {
                        *text = copy.to_string();
                    }
                }
                Some("linkId") => {
                    if let Some(copy) = text.parse::<Id>().ok().and_then(|id| self.links.get(&id)) {
                        *text = copy.to_string();
                    }
                }
                _ => {
                    if let Some(url) = self.url(text) {
                        *text = url;
                    }
                }
            },
            _ => {}
        }
    }

    /// The copy's URL for a file of a copied page or a `/docs/<id>` link to a copied page.
    pub(super) fn url(&self, url: &str) -> Option<String> {
        if let Some((workspace, page, file)) = parse_page_file_url(url) {
            if workspace != self.workspace_id {
                return None;
            }
            let (page, file) = self.files.get(&(page, file))?;
            return Some(page_file_url(workspace, *page, *file));
        }
        let id = url.strip_prefix("/docs/")?.parse::<Id>().ok()?;
        self.pages.get(&id).map(|copy| format!("/docs/{copy}"))
    }
}

/// `content` without the `page` blocks (at any depth) that link to one of `page_ids`.
fn without_page_blocks(content: &[Value], page_ids: &[String]) -> Vec<Value> {
    content
        .iter()
        .filter(|block| {
            let links = block["type"] == "page"
                && block["props"]["pageId"]
                    .as_str()
                    .is_some_and(|id| page_ids.iter().any(|trashed| trashed == id));
            !links
        })
        .map(|block| {
            let mut block = block.clone();
            if let Some(children) = block.get_mut("children").and_then(Value::as_array_mut) {
                *children = without_page_blocks(children, page_ids);
            }
            block
        })
        .collect()
}

fn insert_index(position: Option<i64>, len: usize) -> usize {
    position.map_or(len, |position| {
        usize::try_from(position.max(0)).unwrap_or(len).min(len)
    })
}

/// Live siblings in `space` under `parent_id` (the space's root when `None`), excluding
/// `exclude`, in order.
pub(super) async fn sibling_ids(
    tx: &mut Transaction<'_, Any>,
    workspace_id: Id,
    space: PageSpace,
    parent_id: Option<Id>,
    exclude: Id,
) -> Result<Vec<Id>, PageError> {
    sqlx::query_scalar::<_, String>(
        "SELECT id FROM pages WHERE workspace_id = $1 AND teamspace_id IS NOT DISTINCT FROM $2 \
         AND owner_id IS NOT DISTINCT FROM $3 AND parent_id IS NOT DISTINCT FROM $4 AND deleted_at IS NULL AND id <> $5 ORDER BY position, id",
    )
    .bind(workspace_id.to_string())
    .bind(space.teamspace_column())
    .bind(space.owner_column())
    .bind(parent_id.map(|parent| parent.to_string()))
    .bind(exclude.to_string())
    .fetch_all(&mut **tx)
    .await?
    .into_iter()
    .map(parse_id)
    .collect()
}

pub(super) async fn renumber(tx: &mut Transaction<'_, Any>, ids: &[Id]) -> Result<(), PageError> {
    for (position, id) in ids.iter().enumerate() {
        let position = position as i64;
        sqlx::query("UPDATE pages SET position = $1 WHERE id = $2 AND position <> $3")
            .bind(position)
            .bind(id.to_string())
            .bind(position)
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}

/// True when `ancestor` is on the parent chain of `page_id`.
async fn is_ancestor(
    tx: &mut Transaction<'_, Any>,
    ancestor: Id,
    page_id: Id,
) -> Result<bool, PageError> {
    let Flag(found) = sqlx::query_scalar(
        "WITH RECURSIVE ancestors(id) AS ( \
             SELECT parent_id FROM pages WHERE id = $1 \
             UNION \
             SELECT pages.parent_id FROM pages JOIN ancestors ON pages.id = ancestors.id \
         ) \
         SELECT EXISTS(SELECT 1 FROM ancestors WHERE id = $2)",
    )
    .bind(page_id.to_string())
    .bind(ancestor.to_string())
    .fetch_one(&mut **tx)
    .await?;
    Ok(found)
}

/// True when `page_id` is a database page (in any state).
pub(super) async fn is_database(
    tx: &mut Transaction<'_, Any>,
    page_id: Id,
) -> Result<bool, PageError> {
    let Flag(database) = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM pages WHERE id = $1 AND kind = 'database')",
    )
    .bind(page_id.to_string())
    .fetch_one(&mut **tx)
    .await?;
    Ok(database)
}

/// The space of a live page the caller can see; `None` when it is missing, trashed, or someone
/// else's private page.
async fn live_page_space(
    tx: &mut Transaction<'_, Any>,
    workspace_id: Id,
    page_id: Id,
    actor_id: Id,
) -> Result<Option<PageSpace>, PageError> {
    let row = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT teamspace_id FROM pages WHERE id = $1 AND workspace_id = $2 AND deleted_at IS NULL \
         AND {visible}",
        visible = visible(3)
    )))
    .bind(page_id.to_string())
    .bind(workspace_id.to_string())
    .bind(actor_id.to_string())
    .fetch_optional(&mut **tx)
    .await?;
    row.map(|row| {
        Ok(match parse_optional_id(row.get("teamspace_id"))? {
            Some(teamspace_id) => PageSpace::Teamspace(teamspace_id),
            None => PageSpace::Private(actor_id),
        })
    })
    .transpose()
}

/// Applies the space rules: under a parent the page takes the parent's space (a contradicting
/// request is invalid); at the root it takes the requested space, else `fallback`, else the
/// workspace's default teamspace.
async fn resolve_space(
    tx: &mut Transaction<'_, Any>,
    workspace_id: Id,
    actor_id: Id,
    parent_id: Option<Id>,
    request: SpaceRequest,
    fallback: Option<PageSpace>,
) -> Result<PageSpace, PageError> {
    if request.private == Some(true) && request.teamspace_id.is_some() {
        return Err(PageError::Invalid {
            field: "teamspace_id",
        });
    }
    if let Some(parent_id) = parent_id {
        let space = live_page_space(tx, workspace_id, parent_id, actor_id)
            .await?
            .ok_or(PageError::NotFound)?;
        if request
            .private
            .is_some_and(|private| private != matches!(space, PageSpace::Private(_)))
        {
            return Err(PageError::Invalid { field: "private" });
        }
        if request
            .teamspace_id
            .is_some_and(|teamspace_id| space != PageSpace::Teamspace(teamspace_id))
        {
            return Err(PageError::Invalid {
                field: "teamspace_id",
            });
        }
        return Ok(space);
    }
    if request.private == Some(true) {
        return Ok(PageSpace::Private(actor_id));
    }
    if let Some(teamspace_id) = request.teamspace_id {
        return if teamspace_exists(tx, workspace_id, teamspace_id).await? {
            Ok(PageSpace::Teamspace(teamspace_id))
        } else {
            Err(PageError::TeamspaceNotFound)
        };
    }
    match fallback {
        // `private: false` asks to leave the private space.
        Some(PageSpace::Private(_)) if request.private == Some(false) => {}
        Some(space) => return Ok(space),
        None => {}
    }
    default_teamspace(tx, workspace_id)
        .await?
        .map(PageSpace::Teamspace)
        .ok_or(PageError::TeamspaceNotFound)
}

/// Whether `actor_id` created the page and every descendant, trashed ones too (they move along).
async fn created_whole_subtree(
    tx: &mut Transaction<'_, Any>,
    page_id: Id,
    actor_id: Id,
) -> Result<bool, PageError> {
    let others: i64 = sqlx::query_scalar(
        "WITH RECURSIVE subtree(id) AS ( \
             SELECT $1 \
             UNION ALL \
             SELECT pages.id FROM pages JOIN subtree ON pages.parent_id = subtree.id \
         ) \
         SELECT CAST(COUNT(*) AS BIGINT) FROM pages \
         WHERE id IN (SELECT id FROM subtree) AND creator_id != $2",
    )
    .bind(page_id.to_string())
    .bind(actor_id.to_string())
    .fetch_one(&mut **tx)
    .await?;
    Ok(others == 0)
}

/// Moves every descendant (trashed ones too, so a later restore stays consistent) into `space`,
/// one depth level at a time so each row's parent is already in the new space when the
/// `pages_parent_space_update` trigger checks it.
async fn move_descendants(
    tx: &mut Transaction<'_, Any>,
    page_id: Id,
    space: PageSpace,
) -> Result<(), PageError> {
    for depth in 1_i64.. {
        let moved = sqlx::query(
            "WITH RECURSIVE subtree(id, depth) AS ( \
                 SELECT $1, 0 \
                 UNION ALL \
                 SELECT pages.id, subtree.depth + 1 FROM pages JOIN subtree ON pages.parent_id = subtree.id \
                 WHERE subtree.depth < $2 \
             ) \
             UPDATE pages SET teamspace_id = $3, owner_id = $4 \
             WHERE id IN (SELECT id FROM subtree WHERE depth = $5)",
        )
        .bind(page_id.to_string())
        .bind(depth)
        .bind(space.teamspace_column())
        .bind(space.owner_column())
        .bind(depth)
        .execute(&mut **tx)
        .await?
        .rows_affected();
        if moved == 0 {
            break;
        }
    }
    Ok(())
}

pub(super) async fn page_in_tx(
    tx: &mut Transaction<'_, Any>,
    workspace_id: Id,
    page_id: Id,
    actor_id: Id,
    deleted: bool,
) -> Result<PageRecord, PageError> {
    let row = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT {PAGE_COLUMNS} FROM pages WHERE id = $1 AND workspace_id = $2 AND {visible} \
         AND (($4 = 1 AND deleted_at IS NOT NULL) OR ($5 = 0 AND deleted_at IS NULL))",
        visible = visible(3)
    )))
    .bind(page_id.to_string())
    .bind(workspace_id.to_string())
    .bind(actor_id.to_string())
    .bind(i64::from(deleted))
    .bind(i64::from(deleted))
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(PageError::NotFound)?;
    page_from_row(row)
}

pub(super) fn check_version(
    expected: u64,
    current: u64,
    record: &PageRecord,
) -> Result<(), PageError> {
    if expected == current {
        Ok(())
    } else {
        Err(PageError::VersionConflict {
            current: Box::new(serde_json::to_value(record).map_err(|_| PageError::Corrupt)?),
        })
    }
}

/// A concurrency token that changes with every change of a record, co-edits included.
/// Co-edits do not bump `version` (each title save of an open editor would conflict), but every
/// co-edit that reaches the stored content moves `updated_at` forward (see
/// `project_collab_content`), so the pair covers both. Below 2^53, so JavaScript reads it exactly.
#[must_use]
pub fn revision(version: u64, updated_at: TimestampMillis) -> u64 {
    let digest = Sha256::digest(format!("{version}:{}", updated_at.as_millis()));
    let mut first = [0; 8];
    first.copy_from_slice(&digest[..8]);
    u64::from_be_bytes(first) >> 11
}

/// What a write must match to show that its caller saw the current page.
#[derive(Clone, Copy)]
enum Expected {
    Version(u64),
    /// The page's [`revision`].
    Revision(u64),
}

impl Expected {
    fn check(self, current: &PageRecord) -> Result<(), PageError> {
        let seen = match self {
            Self::Version(version) => version == current.version,
            Self::Revision(expected) => expected == revision(current.version, current.updated_at),
        };
        if seen {
            Ok(())
        } else {
            Err(self.conflict(current))
        }
    }

    /// The conflict of a write whose caller did not see `current`.
    fn conflict(self, current: &PageRecord) -> PageError {
        let Ok(mut record) = serde_json::to_value(current) else {
            return PageError::Corrupt;
        };
        if let Self::Revision(_) = self {
            // The conflict names the value the caller sends next.
            record["version"] = json!(revision(current.version, current.updated_at));
        }
        PageError::VersionConflict {
            current: Box::new(record),
        }
    }
}

impl PageRecord {
    /// The page's space; a private page is always the caller's, since no one else can load it.
    pub(super) fn space(&self, actor_id: Id) -> PageSpace {
        self.teamspace_id
            .map_or(PageSpace::Private(actor_id), PageSpace::Teamspace)
    }
}

fn page_from_row(row: sqlx::any::AnyRow) -> Result<PageRecord, PageError> {
    let content: String = row.get("content_json");
    let teamspace_id = parse_optional_id(row.get("teamspace_id"))?;
    Ok(PageRecord {
        id: parse_id(row.get("id"))?,
        workspace_id: parse_id(row.get("workspace_id"))?,
        parent_id: parse_optional_id(row.get("parent_id"))?,
        teamspace_id,
        private: teamspace_id.is_none(),
        title: row.get("title"),
        icon: row.get("icon"),
        cover_url: row.get("cover_url"),
        cover_position: row.get("cover_position"),
        content: serde_json::from_str(&content).map_err(|_| PageError::Corrupt)?,
        position: row.get("position"),
        version: parse_version(row.get("version"))?,
        creator_id: parse_id(row.get("creator_id"))?,
        updated_by: parse_id(row.get("updated_by"))?,
        created_at: TimestampMillis::from_millis(row.get("created_at")),
        updated_at: TimestampMillis::from_millis(row.get("updated_at")),
        deleted_at: row
            .get::<Option<i64>, _>("deleted_at")
            .map(TimestampMillis::from_millis),
        collab_epoch: row.get("collab_epoch"),
        full_width: row.get("full_width"),
        kind: PageKind::parse(&row.get::<String, _>("kind"))?,
        database_id: parse_optional_id(row.get("database_id"))?,
        locked_at: row
            .get::<Option<i64>, _>("locked_at")
            .map(TimestampMillis::from_millis),
        locked_by: match (
            parse_optional_id(row.get("locked_by"))?,
            row.get::<Option<String>, _>("locked_by_name"),
        ) {
            (Some(id), Some(display_name)) => Some(PageUser { id, display_name }),
            _ => None,
        },
        updated_by_user: PageUser {
            id: parse_id(row.get("updated_by"))?,
            display_name: row
                .get::<Option<String>, _>("updated_by_name")
                .unwrap_or_default(),
        },
    })
}

/// The user's id and display name.
pub(super) async fn user_in_tx(
    tx: &mut Transaction<'_, Any>,
    user_id: Id,
) -> Result<PageUser, PageError> {
    let display_name: String = sqlx::query_scalar("SELECT display_name FROM users WHERE id = $1")
        .bind(user_id.to_string())
        .fetch_optional(&mut **tx)
        .await?
        .ok_or(PageError::Corrupt)?;
    Ok(PageUser {
        id: user_id,
        display_name,
    })
}

/// The epoch reported for pages without a collaborative document yet.
async fn collab_generation(tx: &mut Transaction<'_, Any>) -> Result<String, PageError> {
    Ok(
        sqlx::query_scalar("SELECT generation FROM page_collab_meta WHERE id = 1")
            .fetch_one(&mut **tx)
            .await?,
    )
}

/// A page by id in any state (live or trashed), without a visibility check.
pub(super) async fn page_by_id_in_tx(
    tx: &mut Transaction<'_, Any>,
    page_id: Id,
) -> Result<Option<PageRecord>, PageError> {
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT {PAGE_COLUMNS} FROM pages WHERE id = $1"
    )))
    .bind(page_id.to_string())
    .fetch_optional(&mut **tx)
    .await?
    .map(page_from_row)
    .transpose()
}

fn summary_from_row(row: sqlx::any::AnyRow) -> Result<PageSummary, PageError> {
    let teamspace_id = parse_optional_id(row.get("teamspace_id"))?;
    Ok(PageSummary {
        id: parse_id(row.get("id"))?,
        parent_id: parse_optional_id(row.get("parent_id"))?,
        teamspace_id,
        private: teamspace_id.is_none(),
        kind: PageKind::parse(&row.get::<String, _>("kind"))?,
        title: row.get("title"),
        icon: row.get("icon"),
        position: row.get("position"),
        version: parse_version(row.get("version"))?,
        updated_at: TimestampMillis::from_millis(row.get("updated_at")),
    })
}

pub(super) fn parse_id(value: String) -> Result<Id, PageError> {
    value.parse().map_err(|_| PageError::Corrupt)
}

fn parse_optional_id(value: Option<String>) -> Result<Option<Id>, PageError> {
    value.map(parse_id).transpose()
}

fn parse_version(value: i64) -> Result<u64, PageError> {
    u64::try_from(value).map_err(|_| PageError::Corrupt)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        SEARCH_MAX_TERMS, TextRange, content_text, fts_query, pg_tsquery, split_marks,
        with_ellipses,
    };

    #[test]
    fn content_text_walks_blocks_in_document_order() {
        let content = [
            json!({
                "id": "a",
                "type": "heading",
                "props": {"level": 1, "textColor": "default"},
                "content": [{"type": "text", "text": "Launch", "styles": {}}],
                "children": [{
                    "id": "b",
                    "type": "paragraph",
                    "content": [
                        {"type": "text", "text": "plan", "styles": {"bold": true}},
                        {"type": "link", "href": "https://example.com", "content": [{"type": "text", "text": "docs"}]}
                    ],
                    "children": []
                }]
            }),
            json!({"type": "table", "content": {"type": "tableContent", "rows": [{"cells": [[{"type": "text", "text": "cell"}]]}]}}),
            json!({"type": "paragraph", "content": [{"type": "text", "text": ""}]}),
        ];
        assert_eq!(content_text(&content), "Launch plan docs cell");
        let mentioned = vec![json!({"type": "paragraph", "content": [
            {"type": "text", "text": "Ask", "styles": {}},
            {"type": "mention", "props": {"userId": "0199a0b0-0000-7000-8000-0000000000a1", "name": "Ann Lee"}},
            {"type": "mention", "props": {"userId": "0199a0b0-0000-7000-8000-0000000000a2", "name": ""}}
        ]})];
        assert_eq!(content_text(&mentioned), "Ask @Ann Lee");
    }

    #[test]
    fn fts_queries_quote_every_word_and_prefix_the_last() {
        assert_eq!(
            fts_query("  launch  pla "),
            Some("\"launch\" \"pla\"*".to_owned())
        );
        assert_eq!(
            fts_query("a\"b NEAR(x y) title:foo -bar ^c"),
            Some("\"a b\" \"NEAR(x\" \"y)\" \"title:foo\" \"-bar\" \"^c\"*".to_owned())
        );
        assert_eq!(fts_query("über"), Some("\"über\"*".to_owned()));
        for blank in ["", "   ", "* - : \" ( ) %", "🙂"] {
            assert_eq!(fts_query(blank), None, "{blank}");
        }
        assert_eq!(
            fts_query(&"w ".repeat(40)).unwrap().matches('"').count(),
            SEARCH_MAX_TERMS * 2
        );
    }

    #[test]
    fn pg_tsqueries_split_words_like_unicode61_and_prefix_the_last() {
        assert_eq!(
            pg_tsquery("foo-bar laun"),
            Some("foo <-> bar & laun:*".to_owned())
        );
        assert_eq!(
            pg_tsquery("user@Example.com"),
            Some("user <-> example <-> com:*".to_owned())
        );
        assert_eq!(
            pg_tsquery("a\"b NEAR(x) title:foo !c"),
            Some("a <-> b & near <-> x & title <-> foo & c:*".to_owned())
        );
        for blank in ["", "   ", "* - : \" ( ) % & | !", "🙂"] {
            assert_eq!(pg_tsquery(blank), None, "{blank}");
        }
        assert_eq!(
            pg_tsquery(&"w ".repeat(40)).unwrap().matches('w').count(),
            SEARCH_MAX_TERMS
        );
    }

    #[test]
    fn excerpts_get_ellipses_where_they_cut_the_body() {
        let body = "one two three four";
        assert_eq!(
            with_ellipses("two \u{2}three\u{3}".into(), body),
            "…two \u{2}three\u{3}…"
        );
        assert_eq!(with_ellipses("one two".into(), body), "one two…");
        assert_eq!(with_ellipses("three four".into(), body), "…three four");
        assert_eq!(with_ellipses(body.into(), body), body);
        assert_eq!(with_ellipses(String::new(), body), "");
    }

    #[test]
    fn marks_become_utf16_ranges() {
        let (text, ranges) = split_marks("…a \u{2}Über\u{3} 🙂 \u{2}plan\u{3}s\u{2}\u{3}");
        assert_eq!(text, "…a Über 🙂 plans");
        assert_eq!(
            ranges,
            [
                TextRange { start: 3, end: 7 },
                // The emoji is two UTF-16 code units.
                TextRange { start: 11, end: 15 },
            ]
        );
        assert_eq!(split_marks("plain"), ("plain".to_owned(), Vec::new()));
    }
}
