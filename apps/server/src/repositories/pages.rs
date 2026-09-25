//! Docs pages: trees of block documents grouped into spaces. A page lives in a shared teamspace
//! (every member reads and edits it) or in one member's private space (only the owner sees it;
//! everyone else gets `NotFound`). A page's space is its root's; sub-pages always share it.
//! Content is one opaque JSON block array per page; `content_text` is extracted for search.

use orbit_platform::{Database, Id, TimestampMillis};
use serde::Serialize;
use serde_json::Value;
use sqlx::{Row, Sqlite, Transaction};
use thiserror::Error;
use utoipa::ToSchema;

use super::page_files::{page_has_file, parse_page_file_url};
use super::tasks::{TaskError, escape_like, record_mutation, require_access, require_access_tx};
use super::teamspaces::{default_teamspace, teamspace_exists};

const TRASH_RETENTION_MILLIS: i64 = 30 * 24 * 60 * 60 * 1_000;
const SEARCH_LIMIT: i64 = 20;
const SNIPPET_CHARS: usize = 160;
/// How much text a snippet keeps before the first match.
const SNIPPET_LEAD_CHARS: usize = 40;
const PAGE_COLUMNS: &str = "id, workspace_id, parent_id, teamspace_id, title, icon, cover_url, cover_position, \
     content_json, position, creator_id, updated_by, version, created_at, updated_at, deleted_at";
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
    /// About 160 characters of body text around the first match, or from the start.
    pub snippet: String,
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
}

#[derive(Debug, Error)]
pub enum PageError {
    #[error("page was not found")]
    NotFound,
    #[error("teamspace was not found")]
    TeamspaceNotFound,
    #[error("page input is invalid: {field}")]
    Invalid { field: &'static str },
    #[error("stale version")]
    VersionConflict { current: Box<Value> },
    #[error("stored page data is invalid")]
    Corrupt,
    #[error("page repository is unavailable")]
    Unavailable(#[from] sqlx::Error),
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
    database: Database,
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
        let mut tx = self.database.immediate_transaction().await?;
        require_access_tx(&mut tx, workspace_id, actor_id).await?;
        let current = page_in_tx(&mut tx, workspace_id, page_id, actor_id, false).await?;
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
        let (content_json, text) = match &changes.content {
            Some(content) => (
                Some(serde_json::to_string(content).map_err(|_| PageError::Corrupt)?),
                Some(content_text(content)),
            ),
            None => (None, None),
        };
        sqlx::query(
            "UPDATE pages SET title = ?, icon = ?, cover_url = ?, cover_position = ?, \
             content_json = COALESCE(?, content_json), content_text = COALESCE(?, content_text), \
             updated_by = ?, updated_at = ?, version = version + 1 \
             WHERE id = ? AND workspace_id = ? AND deleted_at IS NULL AND version = ?",
        )
        .bind(&title)
        .bind(&icon)
        .bind(&cover_url)
        .bind(&cover_position)
        .bind(content_json)
        .bind(text)
        .bind(actor_id.to_string())
        .bind(now.as_millis())
        .bind(page_id.to_string())
        .bind(workspace_id.to_string())
        .bind(expected_version as i64)
        .execute(&mut *tx)
        .await?;
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
        tx.commit().await?;
        Ok(PageRecord {
            title,
            icon,
            cover_url,
            cover_position,
            content: changes.content.unwrap_or(current.content),
            version: current.version + 1,
            updated_by: actor_id,
            updated_at: now,
            ..current
        })
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
        tx.commit().await?;
        Ok(PageRecord {
            parent_id,
            teamspace_id: space.teamspace_id(),
            private: space.teamspace_id().is_none(),
            position: index as i64,
            version: current.version + 1,
            updated_by: actor_id,
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
        tx.commit().await?;
        Ok(PageRecord {
            parent_id,
            position: index as i64,
            version: current.version + 1,
            updated_by: actor_id,
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

    /// Case-insensitive substring search over live page titles and body text.
    pub async fn search_pages(
        &self,
        workspace_id: Id,
        actor_id: Id,
        query: &str,
    ) -> Result<PageSearch, PageError> {
        require_access(self.database.pool(), workspace_id, actor_id).await?;
        let query = query.trim();
        if query.is_empty() {
            return Ok(PageSearch { items: Vec::new() });
        }
        let pattern = format!("%{}%", escape_like(&query.to_lowercase()));
        let rows = sqlx::query(&format!(
            "SELECT id, parent_id, teamspace_id, title, icon, content_text FROM pages \
             WHERE workspace_id = ? AND deleted_at IS NULL AND {VISIBLE} \
             AND (LOWER(title) LIKE ? ESCAPE '\\' OR LOWER(content_text) LIKE ? ESCAPE '\\') \
             ORDER BY (LOWER(title) LIKE ? ESCAPE '\\') DESC, updated_at DESC, id DESC LIMIT ?",
        ))
        .bind(workspace_id.to_string())
        .bind(actor_id.to_string())
        .bind(&pattern)
        .bind(&pattern)
        .bind(&pattern)
        .bind(SEARCH_LIMIT)
        .fetch_all(self.database.pool())
        .await?;
        let items = rows
            .into_iter()
            .map(|row| {
                let text: String = row.get("content_text");
                let teamspace_id = parse_optional_id(row.get("teamspace_id"))?;
                Ok(PageSearchResult {
                    id: parse_id(row.get("id"))?,
                    parent_id: parse_optional_id(row.get("parent_id"))?,
                    teamspace_id,
                    private: teamspace_id.is_none(),
                    title: row.get("title"),
                    icon: row.get("icon"),
                    snippet: snippet(&text, query),
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

/// Collects every string under a `"text"` key, in document order, joined with spaces.
#[must_use]
pub fn content_text(content: &[Value]) -> String {
    let mut parts = Vec::new();
    for block in content {
        collect_text(block, &mut parts);
    }
    parts.join(" ")
}

fn collect_text<'a>(value: &'a Value, parts: &mut Vec<&'a str>) {
    match value {
        Value::Array(items) => {
            for item in items {
                collect_text(item, parts);
            }
        }
        Value::Object(map) => {
            // Object keys are sorted, so walk a block's own text before its children.
            if let Some(Value::String(text)) = map.get("text")
                && !text.is_empty()
            {
                parts.push(text);
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

fn snippet(text: &str, query: &str) -> String {
    let chars = text.chars().collect::<Vec<_>>();
    let folded = chars.iter().map(|&c| fold(c)).collect::<Vec<_>>();
    let needle = query.chars().map(fold).collect::<Vec<_>>();
    let start = if needle.is_empty() {
        0
    } else {
        folded
            .windows(needle.len())
            .position(|window| window == needle.as_slice())
            .map_or(0, |index| index.saturating_sub(SNIPPET_LEAD_CHARS))
    };
    let end = (start + SNIPPET_CHARS).min(chars.len());
    let mut snippet = String::new();
    if start > 0 {
        snippet.push('…');
    }
    snippet.extend(&chars[start..end]);
    if end < chars.len() {
        snippet.push('…');
    }
    snippet
}

fn fold(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
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

async fn page_in_tx(
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

fn check_version(expected: u64, current: u64, record: &PageRecord) -> Result<(), PageError> {
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
    })
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

fn parse_id(value: String) -> Result<Id, PageError> {
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

    use super::{content_text, snippet};

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
    }

    #[test]
    fn snippets_center_on_the_first_match() {
        let text = format!("{} Needle {}", "a".repeat(100), "b".repeat(300));
        let result = snippet(&text, "needle");
        assert!(result.starts_with('…'));
        assert!(result.ends_with('…'));
        assert!(result.contains("Needle"));
        assert_eq!(result.chars().count(), 160 + 2);
        assert_eq!(snippet("short body", "title only"), "short body");
    }
}
