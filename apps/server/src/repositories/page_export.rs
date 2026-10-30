//! What a page export reads: the page (and, on request, its live sub-pages) the caller can see,
//! the titles of every page the caller can see (for page links), and the files of the exported
//! pages. Open collaborative documents are flushed first so `content_json` is current.

use std::collections::{HashMap, HashSet};

use orbit_platform::Row;
use orbit_platform::{Database, Id};
use serde_json::Value;
use thiserror::Error;

use super::pages::{json_text_array, parse_id, visible};
use super::tasks::{TaskError, require_access};
use crate::collab::CollabHub;
use crate::export::names::{NameSet, STEM_MAX_CHARS, sanitize_segment};

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
    /// Where the bytes are in the blob store.
    pub storage_key: String,
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
    /// Archive folder of a root page (path segments); a root without one goes to the archive root.
    pub root_dirs: HashMap<Id, Vec<String>>,
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
        let ids: Vec<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "WITH RECURSIVE subtree(id) AS ( \
                 SELECT pages.id FROM pages WHERE pages.id = $1 AND pages.workspace_id = $2 \
                 AND pages.deleted_at IS NULL AND {visible3} \
                 UNION \
                 SELECT pages.id FROM pages JOIN subtree ON pages.parent_id = subtree.id \
                 WHERE $4 = 1 AND pages.workspace_id = $5 AND pages.deleted_at IS NULL AND {visible6} \
             ) \
             SELECT id FROM subtree LIMIT $7",
            visible3 = visible(3),
            visible6 = visible(6)
        )))
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
        let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT id, parent_id, title, icon, cover_url, content_json FROM pages \
             WHERE id IN ({id_list}) AND workspace_id = $2 \
             AND deleted_at IS NULL AND {visible} ORDER BY position, id",
            id_list = json_text_array(self.database.backend(), 1),
            visible = visible(3)
        )))
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

        let title_rows = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT id, title FROM pages WHERE workspace_id = $1 AND deleted_at IS NULL AND {visible}",
            visible = visible(2)
        )))
        .bind(workspace_id.to_string())
        .bind(actor_id.to_string())
        .fetch_all(pool)
        .await?;
        let mut titles = HashMap::with_capacity(title_rows.len());
        for row in title_rows {
            let id = parse_id(row.get("id")).map_err(|_| PageExportError::Corrupt)?;
            titles.insert(id, row.get::<String, _>("title"));
        }

        let files = self.load_files(workspace_id, &pages).await?;
        let members = self.load_members(workspace_id).await?;
        Ok(ExportSource {
            pages,
            titles,
            files,
            members,
            root_dirs: HashMap::new(),
        })
    }

    /// Every live page `actor_id` can see in the workspace, as one forest: teamspace pages in the
    /// folder `<root>/<Teamspace name>/`, the actor's private pages in `<root>/Private/`.
    pub async fn load_workspace(
        &self,
        workspace_id: Id,
        actor_id: Id,
        root: &str,
    ) -> Result<ExportSource, PageExportError> {
        let pool = self.database.pool();
        require_access(pool, workspace_id, actor_id).await?;
        let visible_pages = format!(
            "FROM pages WHERE workspace_id = $1 AND deleted_at IS NULL AND {visible}",
            visible = visible(2)
        );
        if let Some(hub) = CollabHub::existing(&self.database) {
            let ids: Vec<String> =
                sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT id {visible_pages}")))
                    .bind(workspace_id.to_string())
                    .bind(actor_id.to_string())
                    .fetch_all(pool)
                    .await?;
            for id in ids {
                if let Ok(id) = parse_id(id) {
                    hub.flush_page(id).await;
                }
            }
        }
        let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT id, parent_id, teamspace_id, title, icon, cover_url, content_json \
             {visible_pages} ORDER BY position, id"
        )))
        .bind(workspace_id.to_string())
        .bind(actor_id.to_string())
        .fetch_all(pool)
        .await?;
        let mut loaded = HashMap::with_capacity(rows.len());
        let mut spaces = HashMap::with_capacity(rows.len());
        let mut order = Vec::with_capacity(rows.len());
        for row in rows {
            let id = parse_id(row.get("id")).map_err(|_| PageExportError::Corrupt)?;
            let parent_id = row
                .get::<Option<String>, _>("parent_id")
                .map(parse_id)
                .transpose()
                .map_err(|_| PageExportError::Corrupt)?;
            let content: Vec<Value> =
                serde_json::from_str(row.get::<&str, _>("content_json")).unwrap_or_default();
            spaces.insert(id, row.get::<Option<String>, _>("teamspace_id"));
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

        let teamspaces = sqlx::query(
            "SELECT id, name FROM teamspaces WHERE workspace_id = $1 ORDER BY position, id",
        )
        .bind(workspace_id.to_string())
        .fetch_all(pool)
        .await?;
        let mut space_names = NameSet::default();
        let mut space_dirs: HashMap<Option<String>, Vec<String>> = HashMap::new();
        let mut space_order: Vec<Option<String>> = Vec::new();
        for row in teamspaces {
            let name: String = row.get("name");
            let id: String = row.get("id");
            let folder =
                space_names.claim(&sanitize_segment(&name, STEM_MAX_CHARS, "Teamspace"), &[""]);
            space_dirs.insert(Some(id.clone()), vec![root.to_owned(), folder]);
            space_order.push(Some(id));
        }
        let private = space_names.claim("Private", &[""]);
        space_dirs.insert(None, vec![root.to_owned(), private]);
        space_order.push(None);

        // Roots (no parent, or a parent the actor cannot see) by space, then depth-first.
        let mut children: HashMap<Id, Vec<Id>> = HashMap::new();
        let mut roots: HashMap<Option<String>, Vec<Id>> = HashMap::new();
        for id in &order {
            match loaded[id]
                .parent_id
                .filter(|parent| loaded.contains_key(parent))
            {
                Some(parent) => children.entry(parent).or_default().push(*id),
                None => roots.entry(spaces[id].clone()).or_default().push(*id),
            }
        }
        let mut pages = Vec::with_capacity(loaded.len());
        let mut root_dirs = HashMap::new();
        let mut seen = HashSet::new();
        for space in &space_order {
            for root_id in roots.get(space).into_iter().flatten() {
                if let Some(dir) = space_dirs.get(space) {
                    root_dirs.insert(*root_id, dir.clone());
                }
                let mut stack = vec![*root_id];
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
            }
        }
        let titles = pages
            .iter()
            .map(|page| (page.id, page.title.clone()))
            .collect();
        let files = self.load_files(workspace_id, &pages).await?;
        let members = self.load_members(workspace_id).await?;
        Ok(ExportSource {
            pages,
            titles,
            files,
            members,
            root_dirs,
        })
    }

    async fn load_files(
        &self,
        workspace_id: Id,
        pages: &[ExportPage],
    ) -> Result<HashMap<(Id, Id), ExportFile>, PageExportError> {
        let pool = self.database.pool();
        let exported: Vec<String> = pages.iter().map(|page| page.id.to_string()).collect();
        let exported = serde_json::to_string(&exported).map_err(|_| PageExportError::Corrupt)?;
        let file_rows = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT page_files.id, page_files.page_id, page_files.file_name, page_files.mime_type, \
             page_files.size_bytes, attachment_blobs.storage_key FROM page_files \
             JOIN attachment_blobs ON attachment_blobs.id = page_files.blob_id \
             WHERE page_files.workspace_id = $1 AND page_files.page_id IN ({ids})",
            ids = json_text_array(self.database.backend(), 2)
        )))
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
                    storage_key: row.get("storage_key"),
                },
            );
        }
        Ok(files)
    }

    async fn load_members(&self, workspace_id: Id) -> Result<HashMap<Id, String>, PageExportError> {
        let pool = self.database.pool();
        let member_rows = sqlx::query(
            "SELECT users.id, users.display_name FROM memberships \
             JOIN users ON users.id = memberships.user_id WHERE memberships.workspace_id = $1",
        )
        .bind(workspace_id.to_string())
        .fetch_all(pool)
        .await?;
        let mut members = HashMap::with_capacity(member_rows.len());
        for row in member_rows {
            let id = parse_id(row.get("id")).map_err(|_| PageExportError::Corrupt)?;
            members.insert(id, row.get::<String, _>("display_name"));
        }
        Ok(members)
    }
}
