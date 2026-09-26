//! What a page export reads: the page (and, on request, its live sub-pages) the caller can see,
//! the titles of every page the caller can see (for page links), and the files of the exported
//! pages. Open collaborative documents are flushed first so `content_json` is current.

use std::collections::{HashMap, HashSet};

use orbit_platform::{Database, Id};
use serde_json::Value;
use sqlx::Row;
use thiserror::Error;

use super::pages::{VISIBLE, parse_id};
use super::tasks::{TaskError, require_access};
use crate::collab::CollabHub;

/// A page to export.
#[derive(Clone, Debug)]
pub struct ExportPage {
    pub id: Id,
    pub parent_id: Option<Id>,
    pub title: String,
    pub icon: Option<String>,
    pub cover_url: Option<String>,
    pub content: Vec<Value>,
}

/// A file attached to an exported page.
#[derive(Clone, Debug)]
pub struct ExportFile {
    pub page_id: Id,
    pub id: Id,
    pub file_name: String,
    pub mime_type: String,
    pub size_bytes: u64,
}

/// Everything an export needs, read at one point in time.
#[derive(Clone, Debug, Default)]
pub struct ExportSource {
    /// The root first, then its sub-pages depth-first in tree order.
    pub pages: Vec<ExportPage>,
    /// Title of every live page the caller can see in the workspace.
    pub titles: HashMap<Id, String>,
    /// Files of the exported pages by `(page_id, file_id)`.
    pub files: HashMap<(Id, Id), ExportFile>,
    /// Current display names of the workspace's members (for `@mentions`).
    pub members: HashMap<Id, String>,
}

#[derive(Debug, Error)]
pub enum PageExportError {
    /// Missing, trashed, another member's private page, or not a member of the workspace.
    #[error("page was not found")]
    NotFound,
    #[error("the export has more than {limit} pages")]
    TooManyPages { limit: usize },
    #[error("the export is larger than {limit} bytes")]
    TooLarge { limit: u64 },
    #[error("stored page data is invalid")]
    Corrupt,
    #[error("page export storage failed: {0}")]
    Io(String),
    #[error("page export repository is unavailable")]
    Unavailable(#[from] sqlx::Error),
}

impl From<TaskError> for PageExportError {
    fn from(error: TaskError) -> Self {
        match error {
            TaskError::Unavailable(error) => Self::Unavailable(error),
            _ => Self::NotFound,
        }
    }
}

#[derive(Clone)]
pub struct PageExportRepository {
    database: Database,
}

impl PageExportRepository {
    #[must_use]
    pub fn new(database: Database) -> Self {
        Self { database }
    }

    /// Reads the page (and with `include_children` its live, visible subtree) for `actor_id`.
    /// More than `max_pages` pages fail with `TooManyPages` before any content is read.
    pub async fn load(
        &self,
        workspace_id: Id,
        page_id: Id,
        actor_id: Id,
        include_children: bool,
        max_pages: usize,
    ) -> Result<ExportSource, PageExportError> {
        let pool = self.database.pool();
        require_access(pool, workspace_id, actor_id).await?;
        // Trashed pages end the walk (their sub-pages went to the trash with them); sub-pages
        // share their root's space, so the visibility check below is a second line of defense.
        let limit = i64::try_from(max_pages)
            .unwrap_or(i64::MAX)
            .saturating_add(1);
        let ids: Vec<String> = sqlx::query_scalar(&format!(
            "WITH RECURSIVE subtree(id) AS ( \
                 SELECT pages.id FROM pages WHERE pages.id = ? AND pages.workspace_id = ? \
                 AND pages.deleted_at IS NULL AND {VISIBLE} \
                 UNION \
                 SELECT pages.id FROM pages JOIN subtree ON pages.parent_id = subtree.id \
                 WHERE ? = 1 AND pages.workspace_id = ? AND pages.deleted_at IS NULL AND {VISIBLE} \
             ) \
             SELECT id FROM subtree LIMIT ?"
        ))
        .bind(page_id.to_string())
        .bind(workspace_id.to_string())
        .bind(actor_id.to_string())
        .bind(i64::from(include_children))
        .bind(workspace_id.to_string())
        .bind(actor_id.to_string())
        .bind(limit)
        .fetch_all(pool)
        .await?;
        if ids.is_empty() {
            return Err(PageExportError::NotFound);
        }
        if ids.len() > max_pages {
            return Err(PageExportError::TooManyPages { limit: max_pages });
        }
        let ids: Vec<Id> = ids
            .into_iter()
            .map(parse_id)
            .collect::<Result<_, _>>()
            .map_err(|_| PageExportError::Corrupt)?;

        // Edits in open documents reach `content_json` within seconds; the export sees them now.
        if let Some(hub) = CollabHub::existing(&self.database) {
            for id in &ids {
                hub.flush_page(*id).await;
            }
        }

        let id_list = serde_json::to_string(&ids.iter().map(Id::to_string).collect::<Vec<_>>())
            .map_err(|_| PageExportError::Corrupt)?;
        let rows = sqlx::query(&format!(
            "SELECT id, parent_id, title, icon, cover_url, content_json FROM pages \
             WHERE id IN (SELECT value FROM json_each(?)) AND workspace_id = ? \
             AND deleted_at IS NULL AND {VISIBLE} ORDER BY position, id"
        ))
        .bind(&id_list)
        .bind(workspace_id.to_string())
        .bind(actor_id.to_string())
        .fetch_all(pool)
        .await?;
        let mut loaded = HashMap::new();
        let mut order = Vec::new();
        for row in rows {
            let id = parse_id(row.get("id")).map_err(|_| PageExportError::Corrupt)?;
            let parent_id = row
                .get::<Option<String>, _>("parent_id")
                .map(parse_id)
                .transpose()
                .map_err(|_| PageExportError::Corrupt)?;
            let content: Vec<Value> =
                serde_json::from_str(row.get::<&str, _>("content_json")).unwrap_or_default();
            order.push(id);
            loaded.insert(
                id,
                ExportPage {
                    id,
                    parent_id,
                    title: row.get("title"),
                    icon: row.get("icon"),
                    cover_url: row.get("cover_url"),
                    content,
                },
            );
        }
        // Depth-first from the root in sibling order; pages trashed since the walk drop out
        // together with their sub-pages.
        let mut children: HashMap<Id, Vec<Id>> = HashMap::new();
        for id in &order {
            if *id == page_id {
                continue;
            }
            if let Some(parent) = loaded[id].parent_id {
                children.entry(parent).or_default().push(*id);
            }
        }
        if !loaded.contains_key(&page_id) {
            return Err(PageExportError::NotFound);
        }
        let mut pages = Vec::with_capacity(loaded.len());
        let mut seen = HashSet::new();
        let mut stack = vec![page_id];
        while let Some(id) = stack.pop() {
            if !seen.insert(id) {
                continue;
            }
            if let Some(page) = loaded.remove(&id) {
                pages.push(page);
            }
            if let Some(kids) = children.get(&id) {
                stack.extend(kids.iter().rev().copied());
            }
        }

        let title_rows = sqlx::query(&format!(
            "SELECT id, title FROM pages WHERE workspace_id = ? AND deleted_at IS NULL AND {VISIBLE}"
        ))
        .bind(workspace_id.to_string())
        .bind(actor_id.to_string())
        .fetch_all(pool)
        .await?;
        let mut titles = HashMap::with_capacity(title_rows.len());
        for row in title_rows {
            let id = parse_id(row.get("id")).map_err(|_| PageExportError::Corrupt)?;
            titles.insert(id, row.get::<String, _>("title"));
        }

        let exported: Vec<String> = pages.iter().map(|page| page.id.to_string()).collect();
        let exported = serde_json::to_string(&exported).map_err(|_| PageExportError::Corrupt)?;
        let file_rows = sqlx::query(
            "SELECT id, page_id, file_name, mime_type, size_bytes FROM page_files \
             WHERE workspace_id = ? AND page_id IN (SELECT value FROM json_each(?))",
        )
        .bind(workspace_id.to_string())
        .bind(&exported)
        .fetch_all(pool)
        .await?;
        let mut files = HashMap::with_capacity(file_rows.len());
        for row in file_rows {
            let id = parse_id(row.get("id")).map_err(|_| PageExportError::Corrupt)?;
            let page_id = parse_id(row.get("page_id")).map_err(|_| PageExportError::Corrupt)?;
            files.insert(
                (page_id, id),
                ExportFile {
                    page_id,
                    id,
                    file_name: row.get("file_name"),
                    mime_type: row.get("mime_type"),
                    size_bytes: u64::try_from(row.get::<i64, _>("size_bytes")).unwrap_or(0),
                },
            );
        }
        let member_rows = sqlx::query(
            "SELECT users.id, users.display_name FROM memberships \
             JOIN users ON users.id = memberships.user_id WHERE memberships.workspace_id = ?",
        )
        .bind(workspace_id.to_string())
        .fetch_all(pool)
        .await?;
        let mut members = HashMap::with_capacity(member_rows.len());
        for row in member_rows {
            let id = parse_id(row.get("id")).map_err(|_| PageExportError::Corrupt)?;
            members.insert(id, row.get::<String, _>("display_name"));
        }
        Ok(ExportSource {
            pages,
            titles,
            files,
            members,
        })
    }
}
