//! Docs pages: trees of block documents grouped into spaces. A page lives in a shared teamspace
//! (every member reads and edits it) or in one member's private space (only the owner sees it;
//! everyone else gets `NotFound`). A page's space is its root's; sub-pages always share it.
//! Content is one opaque JSON block array per page; `content_text` is extracted for search.

use std::collections::HashMap;

use orbit_domain::WorkspaceRole;
use orbit_platform::{Database, Id, TimestampMillis};
use serde::Serialize;
use serde_json::{Value, json};
use sqlx::{Row, Sqlite, Transaction};
use thiserror::Error;
use utoipa::ToSchema;

use super::page_files::{page_file_url, page_has_file, parse_page_file_url};
use super::page_mentions::notify_new_mentions;
use super::page_versions::{PageVersionKind, SaveOrigin, snapshot_before_edit, store_version};
use super::tasks::{TaskError, record_mutation, require_access, require_access_tx};
use super::teamspaces::{default_teamspace, teamspace_exists};
use super::workspaces::{WorkspaceError, require_role};
use crate::audit::{self, AuditOutcome};
use crate::collab::{CollabError, CollabHub, CollabLock};

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
     full_width, locked_at, locked_by, \
     (SELECT display_name FROM users WHERE users.id = pages.updated_by) AS updated_by_name, \
     (SELECT display_name FROM users WHERE users.id = pages.locked_by) AS locked_by_name";
const SUMMARY_COLUMNS: &str = "pages.id AS id, pages.parent_id AS parent_id, pages.teamspace_id AS teamspace_id, \
     pages.title AS title, pages.icon AS icon, pages.position AS position, pages.version AS version, \
     pages.updated_at AS updated_at";
/// Teamspace pages plus the caller's private pages; binds the caller's user id.
pub(super) const VISIBLE: &str = "(pages.teamspace_id IS NOT NULL OR pages.owner_id = ?)";

/// Where a page lives. `Private` holds the owner's user id.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PageSpace {
    Teamspace(Id),
    Private(Id),
}

impl PageSpace {
    fn teamspace_column(self) -> Option<String> {
        match self {
            Self::Teamspace(id) => Some(id.to_string()),
            Self::Private(_) => None,
        }
    }

    fn owner_column(self) -> Option<String> {
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
    /// start of the body when only the title matched.
    pub snippet: String,
    /// Matched words in `snippet`.
    pub snippet_highlights: Vec<TextRange>,
    /// Matched words in `title`.
    pub title_highlights: Vec<TextRange>,
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

/// One of the caller's favorite pages. `position` is the index among the caller's visible
/// favorites.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct PageFavorite {
    #[schema(value_type = String)]
    pub page_id: Id,
    pub position: i64,
}

/// The caller's favorites in this workspace, ordered by position.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct PageFavoriteList {
    pub items: Vec<PageFavorite>,
}

#[derive(Clone, Debug)]
pub struct CreatePage {
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
    /// Only pages in the trash (trashed directly) can be deleted forever.
    #[error("page is not in the trash")]
    NotTrashed,
    /// The page is locked: its title, icon, cover and content cannot change.
    #[error("page is locked")]
    Locked,
    #[error("stored page data is invalid")]
    Corrupt,
    #[error("page repository is unavailable")]
    Unavailable(#[from] sqlx::Error),
}

impl From<CollabError> for PageError {
    fn from(error: CollabError) -> Self {
        match error {
            CollabError::NotFound => Self::NotFound,
            CollabError::Unavailable(error) => Self::Unavailable(error),
            CollabError::Overloaded | CollabError::Corrupt(_) => Self::Corrupt,
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

    /// All live pages the caller can see, grouped by space (teamspaces by position, then the
    /// caller's private space) and ordered `parent_id, position, id` within a space.
    pub async fn pages(&self, workspace_id: Id, actor_id: Id) -> Result<PageList, PageError> {
        require_access(self.database.pool(), workspace_id, actor_id).await?;
        let rows = sqlx::query(&format!(
            "SELECT {SUMMARY_COLUMNS} FROM pages \
             LEFT JOIN teamspaces ON teamspaces.id = pages.teamspace_id \
             WHERE pages.workspace_id = ? AND pages.deleted_at IS NULL AND {VISIBLE} \
             ORDER BY pages.teamspace_id IS NULL, teamspaces.position, teamspaces.id, \
             pages.parent_id, pages.position, pages.id"
        ))
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
        let row = sqlx::query(&format!(
            "SELECT {PAGE_COLUMNS} FROM pages WHERE id = ? AND workspace_id = ? AND deleted_at IS NULL \
             AND {VISIBLE}"
        ))
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
        let mut tx = self.database.immediate_transaction().await?;
        require_access_tx(&mut tx, workspace_id, actor_id).await?;
        let space = resolve_space(
            &mut tx,
            workspace_id,
            actor_id,
            input.parent_id,
            input.space,
            None,
        )
        .await?;
        let mut siblings = sibling_ids(&mut tx, workspace_id, space, input.parent_id, id).await?;
        let index = insert_index(input.position, siblings.len());
        siblings.insert(index, id);
        let position = index as i64;
        sqlx::query(
            "INSERT INTO pages (id, workspace_id, parent_id, teamspace_id, owner_id, title, icon, position, creator_id, updated_by, version, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, ?, ?)",
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
        .execute(&mut *tx)
        .await?;
        renumber(&mut tx, &siblings).await?;
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
            title: input.title,
            icon: input.icon,
            cover_url: None,
            cover_position: None,
            content: Vec::new(),
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
        // A content write replaces the live collaborative document too: take its room first
        // (never while a transaction is open), after a cheap visibility check.
        let mut collab = match &changes.content {
            Some(_) => {
                self.get_visible(workspace_id, page_id, actor_id).await?;
                Some(CollabHub::of(&self.database).write(page_id).await?)
            }
            None => None,
        };
        let mut tx = self.database.immediate_transaction().await?;
        require_access_tx(&mut tx, workspace_id, actor_id).await?;
        let current = page_in_tx(&mut tx, workspace_id, page_id, actor_id, false).await?;
        let edit = changes.title.is_some()
            || changes.icon.is_some()
            || changes.cover_url.is_some()
            || changes.cover_position.is_some()
            || changes.content.is_some();
        if edit && current.locked_at.is_some() {
            return Err(PageError::Locked);
        }
        check_version(expected_version, current.version, &current)?;
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
        if changes.origin == SaveOrigin::Edit && (content_changed || title != current.title) {
            snapshot_before_edit(&mut tx, &current, now).await?;
        }
        let (content_json, text) = match &changes.content {
            Some(content) => (
                Some(serde_json::to_string(content).map_err(|_| PageError::Corrupt)?),
                Some(content_text(content)),
            ),
            None => (None, None),
        };
        // A layout-only change (full width) is not an edit: the last editor and time stay.
        let (updated_by, updated_at) = if edit {
            (actor_id, now)
        } else {
            (current.updated_by, current.updated_at)
        };
        sqlx::query(
            "UPDATE pages SET title = ?, icon = ?, cover_url = ?, cover_position = ?, \
             content_json = COALESCE(?, content_json), content_text = COALESCE(?, content_text), \
             full_width = ?, updated_by = ?, updated_at = ?, version = version + 1 \
             WHERE id = ? AND workspace_id = ? AND deleted_at IS NULL AND version = ?",
        )
        .bind(&title)
        .bind(&icon)
        .bind(&cover_url)
        .bind(&cover_position)
        .bind(content_json)
        .bind(text)
        .bind(full_width)
        .bind(updated_by.to_string())
        .bind(updated_at.as_millis())
        .bind(page_id.to_string())
        .bind(workspace_id.to_string())
        .bind(expected_version as i64)
        .execute(&mut *tx)
        .await?;
        if let (Some(collab), Some(content)) = (collab.as_mut(), &changes.content)
            && content_changed
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
    async fn get_visible(
        &self,
        workspace_id: Id,
        page_id: Id,
        actor_id: Id,
    ) -> Result<(), PageError> {
        require_access(self.database.pool(), workspace_id, actor_id).await?;
        sqlx::query(&format!(
            "SELECT 1 FROM pages WHERE id = ? AND workspace_id = ? AND deleted_at IS NULL AND {VISIBLE}"
        ))
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
    /// concurrent content save conflict.
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
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<PageRecord, PageError> {
        let mut tx = self.database.immediate_transaction().await?;
        require_access_tx(&mut tx, workspace_id, actor_id).await?;
        let current = page_in_tx(&mut tx, workspace_id, page_id, actor_id, false).await?;
        check_version(expected_version, current.version, &current)?;
        if parent_id == Some(page_id) {
            return Err(PageError::Invalid { field: "parent_id" });
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
        let index = insert_index(Some(position), siblings.len());
        siblings.insert(index, page_id);
        sqlx::query(
            "UPDATE pages SET parent_id = ?, teamspace_id = ?, owner_id = ?, position = ?, \
             updated_by = ?, updated_at = ?, version = version + 1 \
             WHERE id = ? AND workspace_id = ? AND deleted_at IS NULL AND version = ?",
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
        let updated_by_user = user_in_tx(&mut tx, actor_id).await?;
        tx.commit().await?;
        // A page (and its subtree) moved into someone's private space closes other editors.
        CollabHub::revalidate_database(&self.database, Some(workspace_id));
        Ok(PageRecord {
            parent_id,
            teamspace_id: space.teamspace_id(),
            private: space.teamspace_id().is_none(),
            position: index as i64,
            version: current.version + 1,
            updated_by: actor_id,
            updated_by_user,
            updated_at: now,
            ..current
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
                 SELECT ? \
                 UNION ALL \
                 SELECT pages.id FROM pages JOIN subtree ON pages.parent_id = subtree.id \
                 WHERE pages.deleted_at IS NULL \
             ) \
             UPDATE pages SET deleted_at = ?, trashed_with = ?, updated_by = ?, updated_at = ?, version = version + 1 \
             WHERE workspace_id = ? AND deleted_at IS NULL AND id IN (SELECT id FROM subtree)",
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
            sqlx::query_scalar("SELECT COALESCE(trashed_with, id) FROM pages WHERE id = ?")
                .bind(page_id.to_string())
                .fetch_one(&mut *tx)
                .await?;
        let space = current.space(actor_id);
        let parent_id = match current.parent_id {
            Some(parent_id)
                if live_page_space(&mut tx, workspace_id, parent_id, actor_id).await?
                    == Some(space) =>
            {
                Some(parent_id)
            }
            _ => None,
        };
        let mut siblings = sibling_ids(&mut tx, workspace_id, space, parent_id, page_id).await?;
        let index = insert_index(Some(current.position), siblings.len());
        siblings.insert(index, page_id);
        sqlx::query(
            "WITH RECURSIVE subtree(id) AS ( \
                 SELECT ? \
                 UNION ALL \
                 SELECT pages.id FROM pages JOIN subtree ON pages.parent_id = subtree.id \
                 WHERE pages.deleted_at IS NOT NULL AND pages.trashed_with = ? \
             ) \
             UPDATE pages SET deleted_at = NULL, trashed_with = NULL, updated_by = ?, updated_at = ?, version = version + 1 \
             WHERE workspace_id = ? AND deleted_at IS NOT NULL AND id IN (SELECT id FROM subtree)",
        )
        .bind(page_id.to_string())
        .bind(&batch)
        .bind(actor_id.to_string())
        .bind(now.as_millis())
        .bind(workspace_id.to_string())
        .execute(&mut *tx)
        .await?;
        sqlx::query("UPDATE pages SET parent_id = ?, position = ? WHERE id = ?")
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
        require_access(self.database.pool(), workspace_id, actor_id).await?;
        let rows = sqlx::query(&format!(
            "SELECT {SUMMARY_COLUMNS}, pages.deleted_at AS deleted_at FROM pages \
             WHERE pages.workspace_id = ? AND pages.deleted_at > ? AND pages.trashed_with = pages.id \
             AND {VISIBLE} ORDER BY pages.deleted_at DESC, pages.id DESC"
        ))
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
        let role = require_role(&mut tx, workspace_id, actor_id, false).await?;
        let row = sqlx::query(&format!(
            "SELECT deleted_at IS NOT NULL AND trashed_with IS id AS in_trash FROM pages \
             WHERE id = ? AND workspace_id = ? AND {VISIBLE}"
        ))
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
        if current.teamspace_id.is_some() && role == WorkspaceRole::Member {
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
        let role = require_role(&mut tx, workspace_id, actor_id, false).await?;
        let roots = sqlx::query_scalar::<_, String>(
            "SELECT id FROM pages WHERE workspace_id = ? AND deleted_at IS NOT NULL \
             AND trashed_with = id AND (owner_id = ? OR (? AND teamspace_id IS NOT NULL)) \
             ORDER BY deleted_at, id",
        )
        .bind(workspace_id.to_string())
        .bind(actor_id.to_string())
        .bind(role != WorkspaceRole::Member)
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
        let mut originals = vec![source];
        if include_children {
            let rows = sqlx::query(&format!(
                "WITH RECURSIVE subtree(page_id, depth) AS ( \
                     SELECT ?, 0 \
                     UNION ALL \
                     SELECT pages.id, subtree.depth + 1 FROM pages \
                     JOIN subtree ON pages.parent_id = subtree.page_id WHERE pages.deleted_at IS NULL \
                 ) \
                 SELECT {PAGE_COLUMNS} FROM pages JOIN subtree ON subtree.page_id = pages.id \
                 WHERE subtree.depth > 0 ORDER BY subtree.depth, pages.parent_id, pages.position, pages.id"
            ))
            .bind(page_id.to_string())
            .fetch_all(&mut *tx)
            .await?;
            for row in rows {
                originals.push(page_from_row(row)?);
            }
        }
        let pages: HashMap<Id, Id> = originals
            .iter()
            .map(|page| (page.id, Id::new_v7()))
            .collect();
        let mut files = HashMap::new();
        let mut file_rows = Vec::new();
        for page in &originals {
            let rows = sqlx::query(
                "SELECT id, blob_id, file_name, mime_type, size_bytes FROM page_files \
                 WHERE page_id = ? AND workspace_id = ? ORDER BY created_at, id",
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
        let remap = Remap {
            workspace_id,
            pages: &pages,
            files: &files,
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
                 version, created_at, updated_at, full_width) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, ?, ?, ?)",
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
            .bind(original.full_width)
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
                 size_bytes, uploaded_by, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
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

    /// Full-text search (FTS5, migration 0027) over the titles and body text of live pages the
    /// caller can see. Every word must match (the last one as a prefix); case and diacritics are
    /// ignored. Ranked by BM25 with title matches weighted 10x body matches.
    pub async fn search_pages(
        &self,
        workspace_id: Id,
        actor_id: Id,
        query: &str,
    ) -> Result<PageSearch, PageError> {
        require_access(self.database.pool(), workspace_id, actor_id).await?;
        let Some(expression) = fts_query(query) else {
            return Ok(PageSearch { items: Vec::new() });
        };
        let rows = sqlx::query(&format!(
            "SELECT pages.id AS id, pages.parent_id AS parent_id, pages.teamspace_id AS teamspace_id, \
             pages.title AS title, pages.icon AS icon, \
             highlight(page_search, 0, char(2), char(3)) AS marked_title, \
             snippet(page_search, 1, char(2), char(3), '…', ?) AS marked_snippet \
             FROM page_search \
             JOIN page_search_rows ON page_search_rows.id = page_search.rowid \
             JOIN pages ON pages.id = page_search_rows.page_id \
             WHERE page_search MATCH ? AND pages.workspace_id = ? AND pages.deleted_at IS NULL \
             AND {VISIBLE} \
             ORDER BY bm25(page_search, 10.0, 1.0), pages.updated_at DESC, pages.id DESC LIMIT ?",
        ))
        .bind(SNIPPET_TOKENS)
        .bind(&expression)
        .bind(workspace_id.to_string())
        .bind(actor_id.to_string())
        .bind(SEARCH_LIMIT)
        .fetch_all(self.database.pool())
        .await?;
        let items = rows
            .into_iter()
            .map(|row| {
                let teamspace_id = parse_optional_id(row.get("teamspace_id"))?;
                let title: String = row.get("title");
                let (marked_title, title_highlights) =
                    split_marks(&row.get::<String, _>("marked_title"));
                let (snippet, snippet_highlights) =
                    split_marks(&row.get::<String, _>("marked_snippet"));
                Ok(PageSearchResult {
                    id: parse_id(row.get("id"))?,
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
        let ids = visible_favorite_ids(&mut connection, workspace_id, actor_id).await?;
        Ok(favorite_list(ids))
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
                 VALUES (?, ?, ?, ?, ?)",
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
        let visible = visible_favorite_ids(&mut tx, workspace_id, actor_id).await?;
        tx.commit().await?;
        let position = visible
            .iter()
            .position(|id| *id == page_id)
            .ok_or(PageError::NotFound)?;
        Ok(PageFavorite {
            page_id,
            position: position as i64,
        })
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
            "DELETE FROM page_favorites WHERE workspace_id = ? AND user_id = ? AND page_id = ?",
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
        let visible = visible_favorite_ids(&mut tx, workspace_id, actor_id)
            .await?
            .into_iter()
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
        let ids = visible_favorite_ids(&mut tx, workspace_id, actor_id).await?;
        tx.commit().await?;
        Ok(favorite_list(ids))
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
        let room: CollabLock = CollabHub::of(&self.database).lock_change(page_id).await?;
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
            "UPDATE pages SET locked_at = ?, locked_by = ?, version = version + 1 \
             WHERE id = ? AND workspace_id = ? AND deleted_at IS NULL",
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
            "INSERT INTO page_visits (workspace_id, user_id, page_id, visited_at) VALUES (?, ?, ?, ?) \
             ON CONFLICT (user_id, page_id) DO UPDATE SET visited_at = MAX(visited_at, excluded.visited_at)",
        )
        .bind(workspace_id.to_string())
        .bind(actor_id.to_string())
        .bind(page_id.to_string())
        .bind(now.as_millis())
        .execute(&mut *tx)
        .await?;
        // Keep the newest few per member and workspace.
        sqlx::query(
            "DELETE FROM page_visits WHERE workspace_id = ?1 AND user_id = ?2 AND page_id NOT IN ( \
                 SELECT page_id FROM page_visits WHERE workspace_id = ?1 AND user_id = ?2 \
                 ORDER BY visited_at DESC, page_id LIMIT ?3)",
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
        let rows = sqlx::query(&format!(
            "SELECT pages.id AS id, pages.parent_id AS parent_id, pages.teamspace_id AS teamspace_id, \
             pages.title AS title, pages.icon AS icon, page_visits.visited_at AS visited_at \
             FROM page_visits JOIN pages ON pages.id = page_visits.page_id \
             WHERE page_visits.workspace_id = ? AND page_visits.user_id = ? \
             AND pages.workspace_id = page_visits.workspace_id AND pages.deleted_at IS NULL AND {VISIBLE} \
             ORDER BY page_visits.visited_at DESC, pages.id LIMIT ?"
        ))
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

fn favorite_list(ids: Vec<Id>) -> PageFavoriteList {
    PageFavoriteList {
        items: ids
            .into_iter()
            .enumerate()
            .map(|(position, page_id)| PageFavorite {
                page_id,
                position: position as i64,
            })
            .collect(),
    }
}

/// Every favorite of the caller in the workspace (hidden ones too), in order.
async fn favorite_order(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    actor_id: Id,
) -> Result<Vec<Id>, PageError> {
    sqlx::query_scalar::<_, String>(
        "SELECT page_id FROM page_favorites WHERE workspace_id = ? AND user_id = ? \
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
async fn visible_favorite_ids(
    connection: &mut sqlx::SqliteConnection,
    workspace_id: Id,
    actor_id: Id,
) -> Result<Vec<Id>, PageError> {
    sqlx::query_scalar::<_, String>(&format!(
        "SELECT page_favorites.page_id FROM page_favorites \
         JOIN pages ON pages.id = page_favorites.page_id \
         WHERE page_favorites.workspace_id = ? AND page_favorites.user_id = ? \
         AND pages.workspace_id = page_favorites.workspace_id AND pages.deleted_at IS NULL \
         AND {VISIBLE} ORDER BY page_favorites.position, page_favorites.page_id"
    ))
    .bind(workspace_id.to_string())
    .bind(actor_id.to_string())
    .bind(actor_id.to_string())
    .fetch_all(connection)
    .await?
    .into_iter()
    .map(parse_id)
    .collect()
}

async fn renumber_favorites(
    tx: &mut Transaction<'_, Sqlite>,
    actor_id: Id,
    ids: &[Id],
) -> Result<(), PageError> {
    for (position, id) in ids.iter().enumerate() {
        let position = position as i64;
        sqlx::query(
            "UPDATE page_favorites SET position = ? WHERE user_id = ? AND page_id = ? \
             AND position <> ?",
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
fn fts_query(input: &str) -> Option<String> {
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

/// Removes the highlight markers from FTS output and returns the plain text with the marked
/// spans as UTF-16 ranges.
fn split_marks(marked: &str) -> (String, Vec<TextRange>) {
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
async fn purge_batch(tx: &mut Transaction<'_, Sqlite>, root_id: Id) -> Result<u64, PageError> {
    let ids = sqlx::query_scalar::<_, String>(
        "WITH RECURSIVE subtree(id) AS ( \
             SELECT ? \
             UNION ALL \
             SELECT pages.id FROM pages JOIN subtree ON pages.parent_id = subtree.id \
             WHERE pages.deleted_at IS NOT NULL AND pages.trashed_with = ? \
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
        purged += sqlx::query("DELETE FROM pages WHERE id = ? AND deleted_at IS NOT NULL")
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
struct Remap<'a> {
    workspace_id: Id,
    /// Original page id → copy id.
    pages: &'a HashMap<Id, Id>,
    /// (original page id, original file id) → (copy page id, copy file id).
    files: &'a HashMap<(Id, Id), (Id, Id)>,
}

impl Remap<'_> {
    /// Walks block JSON: `pageId` props of `page` blocks, page-file URLs (image/file `url`
    /// props and anything else holding one) and `/docs/<id>` link targets. Text is left alone.
    fn value(&self, value: &mut Value, key: Option<&str>) {
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
                Some("pageId") => {
                    if let Some(copy) = text.parse::<Id>().ok().and_then(|id| self.pages.get(&id)) {
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
    fn url(&self, url: &str) -> Option<String> {
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

fn insert_index(position: Option<i64>, len: usize) -> usize {
    position.map_or(len, |position| {
        usize::try_from(position.max(0)).unwrap_or(len).min(len)
    })
}

/// Live siblings in `space` under `parent_id` (the space's root when `None`), excluding
/// `exclude`, in order.
async fn sibling_ids(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    space: PageSpace,
    parent_id: Option<Id>,
    exclude: Id,
) -> Result<Vec<Id>, PageError> {
    sqlx::query_scalar::<_, String>(
        "SELECT id FROM pages WHERE workspace_id = ? AND teamspace_id IS ? AND owner_id IS ? \
         AND parent_id IS ? AND deleted_at IS NULL AND id <> ? ORDER BY position, id",
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

async fn renumber(tx: &mut Transaction<'_, Sqlite>, ids: &[Id]) -> Result<(), PageError> {
    for (position, id) in ids.iter().enumerate() {
        let position = position as i64;
        sqlx::query("UPDATE pages SET position = ? WHERE id = ? AND position <> ?")
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
    tx: &mut Transaction<'_, Sqlite>,
    ancestor: Id,
    page_id: Id,
) -> Result<bool, PageError> {
    Ok(sqlx::query_scalar(
        "WITH RECURSIVE ancestors(id) AS ( \
             SELECT parent_id FROM pages WHERE id = ? \
             UNION \
             SELECT pages.parent_id FROM pages JOIN ancestors ON pages.id = ancestors.id \
         ) \
         SELECT EXISTS(SELECT 1 FROM ancestors WHERE id = ?)",
    )
    .bind(page_id.to_string())
    .bind(ancestor.to_string())
    .fetch_one(&mut **tx)
    .await?)
}

/// The space of a live page the caller can see; `None` when it is missing, trashed, or someone
/// else's private page.
async fn live_page_space(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    page_id: Id,
    actor_id: Id,
) -> Result<Option<PageSpace>, PageError> {
    let row = sqlx::query(&format!(
        "SELECT teamspace_id FROM pages WHERE id = ? AND workspace_id = ? AND deleted_at IS NULL \
         AND {VISIBLE}"
    ))
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
    tx: &mut Transaction<'_, Sqlite>,
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

/// Moves every descendant (trashed ones too, so a later restore stays consistent) into `space`,
/// one depth level at a time so each row's parent is already in the new space when the
/// `pages_parent_space_update` trigger checks it.
async fn move_descendants(
    tx: &mut Transaction<'_, Sqlite>,
    page_id: Id,
    space: PageSpace,
) -> Result<(), PageError> {
    for depth in 1_i64.. {
        let moved = sqlx::query(
            "WITH RECURSIVE subtree(id, depth) AS ( \
                 SELECT ?, 0 \
                 UNION ALL \
                 SELECT pages.id, subtree.depth + 1 FROM pages JOIN subtree ON pages.parent_id = subtree.id \
                 WHERE subtree.depth < ? \
             ) \
             UPDATE pages SET teamspace_id = ?, owner_id = ? \
             WHERE id IN (SELECT id FROM subtree WHERE depth = ?)",
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
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    page_id: Id,
    actor_id: Id,
    deleted: bool,
) -> Result<PageRecord, PageError> {
    let row = sqlx::query(&format!(
        "SELECT {PAGE_COLUMNS} FROM pages WHERE id = ? AND workspace_id = ? AND {VISIBLE} \
         AND ((? = 1 AND deleted_at IS NOT NULL) OR (? = 0 AND deleted_at IS NULL))"
    ))
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

impl PageRecord {
    /// The page's space; a private page is always the caller's, since no one else can load it.
    fn space(&self, actor_id: Id) -> PageSpace {
        self.teamspace_id
            .map_or(PageSpace::Private(actor_id), PageSpace::Teamspace)
    }
}

fn page_from_row(row: sqlx::sqlite::SqliteRow) -> Result<PageRecord, PageError> {
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
    tx: &mut Transaction<'_, Sqlite>,
    user_id: Id,
) -> Result<PageUser, PageError> {
    let display_name: String = sqlx::query_scalar("SELECT display_name FROM users WHERE id = ?")
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
async fn collab_generation(tx: &mut Transaction<'_, Sqlite>) -> Result<String, PageError> {
    Ok(
        sqlx::query_scalar("SELECT generation FROM page_collab_meta WHERE id = 1")
            .fetch_one(&mut **tx)
            .await?,
    )
}

/// A page by id in any state (live or trashed), without a visibility check.
pub(super) async fn page_by_id_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    page_id: Id,
) -> Result<Option<PageRecord>, PageError> {
    sqlx::query(&format!("SELECT {PAGE_COLUMNS} FROM pages WHERE id = ?"))
        .bind(page_id.to_string())
        .fetch_optional(&mut **tx)
        .await?
        .map(page_from_row)
        .transpose()
}

fn summary_from_row(row: sqlx::sqlite::SqliteRow) -> Result<PageSummary, PageError> {
    let teamspace_id = parse_optional_id(row.get("teamspace_id"))?;
    Ok(PageSummary {
        id: parse_id(row.get("id"))?,
        parent_id: parse_optional_id(row.get("parent_id"))?,
        teamspace_id,
        private: teamspace_id.is_none(),
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

    use super::{SEARCH_MAX_TERMS, TextRange, content_text, fts_query, split_marks};

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
