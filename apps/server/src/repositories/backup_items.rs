//! Single items copied out of a backup snapshot into the live workspace: a page (with its
//! sub-pages if asked) or a task. The snapshot is only read ([`SnapshotReader`]); the live
//! database gets new rows with new ids and nothing else changes. Files are shared with the live
//! workspace when it still stores the same bytes; otherwise they are uploaded again from the
//! snapshot's `attachments/` directory or the live blob store (S3 keeps released objects for a
//! while). A file whose bytes are nowhere is left out and counted in `missing_files`.

use std::collections::{HashMap, HashSet, VecDeque};

use orbit_platform::{
    BackupError, BackupSnapshot, Database, Flag, Id, LocalBlobStore, Row, SnapshotReader,
    SnapshotRow, StagedUpload, TimestampMillis, UploadError, UploadService,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{Any, Transaction};
use thiserror::Error;
use utoipa::ToSchema;

use super::billing::{TrialCapacityError, TrialLimitReached, ensure_trial_capacity};
use super::pages::{PageSpace, Remap, content_text, renumber, sibling_ids};
use super::tasks::description_from_blocks;
use super::teamspaces::teamspace_exists;
use crate::audit::{self, AuditOutcome};
use crate::billing::TrialLimit;

/// At most this many items are listed.
const ITEM_LIMIT: usize = 200;
const RESTORED_SUFFIX: &str = " (restored)";
const AUDIT_ACTION: &str = "backup.item_restored";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum BackupItemKind {
    Page,
    Task,
}

/// A workspace in a backup snapshot.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct BackupWorkspace {
    pub id: String,
    pub name: String,
    /// The workspace still exists and is not in the trash: only such a workspace can receive a restore.
    pub live: bool,
}

/// A page or task in a backup snapshot.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct BackupItem {
    pub id: String,
    pub kind: BackupItemKind,
    pub title: String,
    /// Pages: the parent page's title. `null` at the top level and for tasks.
    #[schema(required = true)]
    pub parent_title: Option<String>,
    /// Tasks: the project's name.
    #[schema(required = true)]
    pub project_name: Option<String>,
    /// Tasks: the number in the project.
    #[schema(required = true)]
    pub number: Option<i64>,
    /// The item was in the trash when the backup was made.
    pub deleted: bool,
    /// Unix milliseconds.
    pub updated_at: i64,
    /// Pages: the sub-pages that `include_children` copies (one level). Always 0 for tasks.
    pub child_count: i64,
}

/// The copy a restore made.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct RestoredItem {
    pub kind: BackupItemKind,
    /// The id of the new page or task.
    pub id: String,
    pub title: String,
    /// The web route of the new item (`/docs/<id>`, `/tasks/<KEY-12>`).
    pub path: String,
    /// Files of the item whose bytes were neither in the live workspace, the snapshot nor the blob store.
    pub missing_files: i64,
}

#[derive(Debug, Error)]
pub enum BackupItemError {
    /// Not in the snapshot, or not live.
    #[error("workspace was not found")]
    WorkspaceNotFound,
    #[error("backup item was not found")]
    ItemNotFound,
    #[error("the task's project no longer exists")]
    ProjectMissing,
    #[error("the page's teamspace no longer exists")]
    TeamspaceMissing,
    #[error("the snapshot cannot be read: {0}")]
    Unreadable(#[from] BackupError),
    #[error("trial limit reached")]
    TrialLimit(TrialLimitReached),
    #[error(transparent)]
    Upload(UploadError),
    #[error("backup item repository is unavailable")]
    Database(#[from] sqlx::Error),
}

impl From<TrialCapacityError> for BackupItemError {
    fn from(error: TrialCapacityError) -> Self {
        match error {
            TrialCapacityError::Reached(reached) => Self::TrialLimit(reached),
            TrialCapacityError::Database(error) => Self::Database(error),
        }
    }
}

impl From<UploadError> for BackupItemError {
    fn from(error: UploadError) -> Self {
        match error {
            UploadError::Database(error) => Self::Database(error),
            error => Self::Upload(error),
        }
    }
}

fn unreadable(detail: impl Into<String>) -> BackupItemError {
    BackupItemError::Unreadable(BackupError::InvalidSnapshot(detail.into()))
}

/// Where a restore runs: the verified snapshot, its reader, and who asked.
pub struct RestoreSource<'a> {
    pub snapshot: &'a BackupSnapshot,
    pub reader: &'a SnapshotReader,
    pub workspace_id: Id,
    pub actor_id: Id,
    pub request_id: &'a str,
}

#[derive(Clone)]
pub struct BackupItemRepository {
    database: Database,
    uploads: UploadService,
}

impl BackupItemRepository {
    #[must_use]
    pub fn new(database: Database, uploads: UploadService) -> Self {
        Self { database, uploads }
    }

    /// The workspaces of the snapshot, by name.
    pub async fn workspaces(
        &self,
        reader: &SnapshotReader,
    ) -> Result<Vec<BackupWorkspace>, BackupItemError> {
        let rows = reader.rows("workspaces", &["id", "name"], |_| true).await?;
        let live: HashSet<String> =
            sqlx::query_scalar::<_, String>("SELECT id FROM workspaces WHERE deleted_at IS NULL")
                .fetch_all(self.database.pool())
                .await?
                .into_iter()
                .collect();
        let mut items: Vec<BackupWorkspace> = rows
            .iter()
            .filter_map(|row| {
                let id = row.get("id")?.to_owned();
                Some(BackupWorkspace {
                    live: live.contains(&id),
                    name: row.get("name").unwrap_or_default().to_owned(),
                    id,
                })
            })
            .collect();
        items.sort_by(|left, right| {
            left.name
                .to_lowercase()
                .cmp(&right.name.to_lowercase())
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(items)
    }

    /// Fails with [`BackupItemError::WorkspaceNotFound`] unless the workspace is in the snapshot
    /// and live.
    pub async fn require_workspace(
        &self,
        reader: &SnapshotReader,
        workspace_id: Id,
    ) -> Result<(), BackupItemError> {
        let id = workspace_id.to_string();
        let in_snapshot = !reader
            .rows("workspaces", &["id"], |row| row.get("id") == Some(&id))
            .await?
            .is_empty();
        let Flag(live) = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM workspaces WHERE id = $1 AND deleted_at IS NULL)",
        )
        .bind(&id)
        .fetch_one(self.database.pool())
        .await?;
        if in_snapshot && live {
            Ok(())
        } else {
            Err(BackupItemError::WorkspaceNotFound)
        }
    }

    /// Pages or tasks of the workspace in the snapshot whose title contains `query` (any case),
    /// newest first, at most [`ITEM_LIMIT`].
    pub async fn items(
        &self,
        reader: &SnapshotReader,
        workspace_id: Id,
        kind: BackupItemKind,
        query: Option<&str>,
    ) -> Result<Vec<BackupItem>, BackupItemError> {
        let needle = query
            .map(str::trim)
            .filter(|query| !query.is_empty())
            .map(str::to_lowercase);
        let matches = |title: &str| {
            needle
                .as_ref()
                .is_none_or(|needle| title.to_lowercase().contains(needle))
        };
        let mut items = match kind {
            BackupItemKind::Page => {
                let tree = PageTree::read(reader, workspace_id).await?;
                tree.nodes
                    .values()
                    .filter(|node| matches(&node.title))
                    .map(|node| BackupItem {
                        id: node.id.clone(),
                        kind,
                        title: node.title.clone(),
                        parent_title: node
                            .parent_id
                            .as_ref()
                            .and_then(|parent| tree.nodes.get(parent))
                            .map(|parent| parent.title.clone()),
                        project_name: None,
                        number: None,
                        deleted: node.deleted,
                        updated_at: node.updated_at,
                        child_count: tree.children(node).len() as i64,
                    })
                    .collect::<Vec<_>>()
            }
            BackupItemKind::Task => {
                let workspace = workspace_id.to_string();
                let projects: HashMap<String, (String, bool)> = reader
                    .rows(
                        "projects",
                        &["id", "workspace_id", "name", "deleted_at"],
                        |row| row.get("workspace_id") == Some(&workspace),
                    )
                    .await?
                    .into_iter()
                    .filter_map(|row| {
                        Some((
                            row.get("id")?.to_owned(),
                            (
                                row.get("name").unwrap_or_default().to_owned(),
                                row.get("deleted_at").is_some(),
                            ),
                        ))
                    })
                    .collect();
                let mut columns = vec![
                    "id",
                    "workspace_id",
                    "project_id",
                    "title",
                    "deleted_at",
                    "updated_at",
                ];
                columns.extend(optional_columns(reader, "tasks", &["number"]).await?);
                reader
                    .rows("tasks", &columns, |row| {
                        row.get("workspace_id") == Some(&workspace)
                            && matches(row.get("title").unwrap_or_default())
                    })
                    .await?
                    .into_iter()
                    .filter_map(|row| {
                        let project = row.get("project_id").and_then(|id| projects.get(id));
                        Some(BackupItem {
                            id: row.get("id")?.to_owned(),
                            kind,
                            title: row.get("title").unwrap_or_default().to_owned(),
                            parent_title: None,
                            project_name: project.map(|(name, _)| name.clone()),
                            number: row.get("number").and_then(|number| number.parse().ok()),
                            deleted: row.get("deleted_at").is_some()
                                || project.is_some_and(|(_, deleted)| *deleted),
                            updated_at: millis(row.get("updated_at")),
                            child_count: 0,
                        })
                    })
                    .collect()
            }
        };
        items.sort_by(|left, right| {
            right
                .updated_at
                .cmp(&left.updated_at)
                .then_with(|| right.id.cmp(&left.id))
        });
        items.truncate(ITEM_LIMIT);
        Ok(items)
    }

    /// Copies a page of the snapshot (with `include_children`, the sub-pages that were in the
    /// same state: live under a live page, or trashed together with a trashed one) as new pages.
    /// The root is titled "<title> (restored)"; sub-pages keep their titles and order.
    ///
    /// Placement: a teamspace page goes under its old parent when that parent is live, not in
    /// the trash and still in the same teamspace, else to the top level of its teamspace; when
    /// the teamspace is gone, the restore fails with [`BackupItemError::TeamspaceMissing`]. A
    /// private page goes back to its owner's private space (under its old parent on the same
    /// rule) only while the owner is still a member; it is never moved into a teamspace, so it
    /// fails the same way otherwise. Comments, versions, favorites and visits are not copied.
    pub async fn restore_page(
        &self,
        source: RestoreSource<'_>,
        page_id: Id,
        include_children: bool,
    ) -> Result<RestoredItem, BackupItemError> {
        let RestoreSource {
            snapshot,
            reader,
            workspace_id,
            actor_id,
            request_id,
        } = source;
        let tree = PageTree::read(reader, workspace_id).await?;
        let root = tree
            .nodes
            .get(&page_id.to_string())
            .ok_or(BackupItemError::ItemNotFound)?;
        // Parents first, siblings in order.
        let mut order = vec![root.id.clone()];
        if include_children {
            let mut queue = VecDeque::from([root]);
            while let Some(node) = queue.pop_front() {
                for child in tree.children(node) {
                    order.push(child.id.clone());
                    queue.push_back(child);
                }
            }
        }
        let wanted: HashSet<&str> = order.iter().map(String::as_str).collect();

        let mut columns = vec!["id", "workspace_id", "title", "content_json"];
        columns.extend(
            optional_columns(
                reader,
                "pages",
                &[
                    "icon",
                    "cover_url",
                    "cover_position",
                    "full_width",
                    "teamspace_id",
                    "owner_id",
                ],
            )
            .await?,
        );
        let workspace = workspace_id.to_string();
        let rows: HashMap<String, SnapshotRow> = reader
            .rows("pages", &columns, |row| {
                row.get("workspace_id") == Some(&workspace)
                    && row.get("id").is_some_and(|id| wanted.contains(id))
            })
            .await?
            .into_iter()
            .filter_map(|row| Some((row.get("id")?.to_owned(), row)))
            .collect();
        let mut pages = Vec::with_capacity(order.len());
        for id in &order {
            let row = rows
                .get(id)
                .ok_or_else(|| unreadable("a page row is missing"))?;
            let content =
                match serde_json::from_str::<Value>(row.get("content_json").unwrap_or("[]")) {
                    Ok(Value::Array(content)) => content,
                    _ => return Err(unreadable("a page has invalid content")),
                };
            pages.push(SnapshotPage {
                id: parse_snapshot_id(id)?,
                parent_id: tree.nodes[id]
                    .parent_id
                    .as_deref()
                    .map(parse_snapshot_id)
                    .transpose()?,
                title: row.get("title").unwrap_or_default().to_owned(),
                icon: row.get("icon").map(str::to_owned),
                cover_url: row.get("cover_url").map(str::to_owned),
                cover_position: row.get("cover_position").map(str::to_owned),
                full_width: row.get("full_width") == Some("1"),
                teamspace_id: row.get("teamspace_id").map(parse_snapshot_id).transpose()?,
                owner_id: row.get("owner_id").map(parse_snapshot_id).transpose()?,
                content,
            });
        }

        // Files of the copied pages, by (page, file).
        let files = if reader.columns("page_files").await?.is_some() {
            reader
                .rows(
                    "page_files",
                    &[
                        "id",
                        "workspace_id",
                        "page_id",
                        "blob_id",
                        "file_name",
                        "mime_type",
                    ],
                    |row| {
                        row.get("workspace_id") == Some(&workspace)
                            && row.get("page_id").is_some_and(|id| wanted.contains(id))
                    },
                )
                .await?
        } else {
            Vec::new()
        };
        let mut staging = BlobStaging::new(&self.uploads, workspace_id, actor_id);
        let result = match staging
            .prepare(&self.database, snapshot, reader, &files, "file_name")
            .await
        {
            Ok(()) => {
                let meta = source_meta(snapshot, workspace_id, actor_id, request_id);
                self.write_pages(&mut staging, &pages, &files, meta).await
            }
            Err(error) => Err(error),
        };
        if result.is_err() {
            staging.discard().await;
        }
        result
    }

    async fn write_pages(
        &self,
        staging: &mut BlobStaging<'_>,
        pages: &[SnapshotPage],
        files: &[SnapshotRow],
        meta: Meta<'_>,
    ) -> Result<RestoredItem, BackupItemError> {
        let Meta {
            backup_id,
            workspace_id,
            actor_id,
            request_id,
        } = meta;
        let root = &pages[0];
        let finalization = self.uploads.begin_finalization().await;
        let now = self.database.database_now().await?;
        let mut tx = self.database.immediate_transaction().await?;
        require_live_workspace(&mut tx, workspace_id).await?;
        ensure_trial_capacity(
            &self.database,
            &mut tx,
            workspace_id,
            TrialLimit::Pages,
            i64::try_from(pages.len()).unwrap_or(i64::MAX),
        )
        .await?;
        let space = match (root.teamspace_id, root.owner_id) {
            (Some(teamspace), _) if teamspace_exists(&mut tx, workspace_id, teamspace).await? => {
                PageSpace::Teamspace(teamspace)
            }
            (None, Some(owner)) if is_member(&mut tx, workspace_id, owner).await? => {
                PageSpace::Private(owner)
            }
            _ => return Err(BackupItemError::TeamspaceMissing),
        };
        let parent_id = match root.parent_id {
            Some(parent) if live_page_in(&mut tx, workspace_id, parent, space).await? => {
                Some(parent)
            }
            _ => None,
        };

        let blobs = staging.resolve(&finalization, &mut tx, now).await?;
        let page_ids: HashMap<Id, Id> = pages.iter().map(|page| (page.id, Id::new_v7())).collect();
        let mut file_ids = HashMap::new();
        let mut file_rows = Vec::new();
        for file in files {
            let (Some(old_page), Some(old_file)) = (
                file.get("page_id").and_then(|id| id.parse::<Id>().ok()),
                file.get("id").and_then(|id| id.parse::<Id>().ok()),
            ) else {
                continue;
            };
            let Some(Some(blob)) = file.get("blob_id").and_then(|id| blobs.get(id)) else {
                continue;
            };
            let copy = (page_ids[&old_page], Id::new_v7());
            file_ids.insert((old_page, old_file), copy);
            file_rows.push((copy, file, blob));
        }
        let remap = Remap {
            workspace_id,
            pages: &page_ids,
            files: &file_ids,
        };

        let mut child_positions: HashMap<Id, i64> = HashMap::new();
        let new_root = page_ids[&root.id];
        let title = format!("{}{RESTORED_SUFFIX}", root.title);
        for (index, page) in pages.iter().enumerate() {
            let id = page_ids[&page.id];
            let (parent, title, position) = if index == 0 {
                (parent_id, title.clone(), 0)
            } else {
                let parent = page
                    .parent_id
                    .and_then(|parent| page_ids.get(&parent).copied())
                    .ok_or_else(|| unreadable("a sub-page has no copied parent"))?;
                let next = child_positions.entry(parent).or_default();
                *next += 1;
                (Some(parent), page.title.clone(), *next - 1)
            };
            let mut content = Value::Array(page.content.clone());
            remap.value(&mut content, None);
            let Value::Array(content) = content else {
                return Err(unreadable("a page has invalid content"));
            };
            let cover_url = page
                .cover_url
                .as_ref()
                .map(|url| remap.url(url).unwrap_or_else(|| url.clone()));
            sqlx::query(
                "INSERT INTO pages (id, workspace_id, parent_id, teamspace_id, owner_id, title, icon, \
                 cover_url, cover_position, content_json, content_text, position, creator_id, updated_by, \
                 version, created_at, updated_at, full_width) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, 0, $15, $16, $17)",
            )
            .bind(id.to_string())
            .bind(workspace_id.to_string())
            .bind(parent.map(|parent| parent.to_string()))
            .bind(space.teamspace_column())
            .bind(space.owner_column())
            .bind(&title)
            .bind(&page.icon)
            .bind(&cover_url)
            .bind(&page.cover_position)
            .bind(Value::Array(content.clone()).to_string())
            .bind(content_text(&content))
            .bind(position)
            .bind(actor_id.to_string())
            .bind(actor_id.to_string())
            .bind(now.as_millis())
            .bind(now.as_millis())
            .bind(i64::from(page.full_width))
            .execute(&mut *tx)
            .await?;
        }
        for ((page, file), row, blob) in &file_rows {
            sqlx::query(
                "INSERT INTO page_files (id, workspace_id, page_id, blob_id, file_name, mime_type, \
                 size_bytes, uploaded_by, created_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
            )
            .bind(file.to_string())
            .bind(workspace_id.to_string())
            .bind(page.to_string())
            .bind(blob.id.to_string())
            .bind(file_name(row.get("file_name")))
            .bind(blob.media_type(row.get("mime_type")))
            .bind(blob.size)
            .bind(actor_id.to_string())
            .bind(now.as_millis())
            .execute(&mut *tx)
            .await?;
        }
        let mut siblings = sibling_ids(&mut tx, workspace_id, space, parent_id, new_root)
            .await
            .map_err(page_error)?;
        siblings.push(new_root);
        renumber(&mut tx, &siblings).await.map_err(page_error)?;
        let missing_files = i64::try_from(files.len() - file_rows.len()).unwrap_or(i64::MAX);
        audit::record(
            &mut tx,
            workspace_id,
            Some(actor_id),
            AUDIT_ACTION,
            AuditOutcome::Success,
            "page",
            Some(new_root),
            request_id,
            json!({
                "backup_id": backup_id,
                "kind": "page",
                "source_id": root.id,
                "id": new_root,
                "pages": pages.len(),
                "missing_files": missing_files,
            }),
            now,
        )
        .await?;
        tx.commit().await?;
        drop(finalization);
        Ok(RestoredItem {
            kind: BackupItemKind::Page,
            id: new_root.to_string(),
            title,
            path: format!("/docs/{new_root}"),
            missing_files,
        })
    }

    /// Copies a task of the snapshot as a new task (new number) of its project, which must be
    /// live. Title "<title> (restored)"; description, priority and due dates as in the snapshot;
    /// the status and labels of the same names in the live project (else its first status, and
    /// no such label); assignees who are still members; its own files (not those of comments).
    /// Comments, relations, sub-tasks and activity are not copied.
    pub async fn restore_task(
        &self,
        source: RestoreSource<'_>,
        task_id: Id,
    ) -> Result<RestoredItem, BackupItemError> {
        let RestoreSource {
            snapshot,
            reader,
            workspace_id,
            actor_id,
            request_id,
        } = source;
        let workspace = workspace_id.to_string();
        let id = task_id.to_string();
        let mut columns = vec![
            "id",
            "workspace_id",
            "project_id",
            "status_id",
            "title",
            "description",
            "priority",
        ];
        columns.extend(
            optional_columns(
                reader,
                "tasks",
                &["due_at", "due_start_at", "description_json"],
            )
            .await?,
        );
        let task = reader
            .rows("tasks", &columns, |row| {
                row.get("id") == Some(&id) && row.get("workspace_id") == Some(&workspace)
            })
            .await?
            .into_iter()
            .next()
            .ok_or(BackupItemError::ItemNotFound)?;
        let status_id = task.get("status_id").unwrap_or_default().to_owned();
        let status_name = reader
            .rows("task_statuses", &["id", "name"], |row| {
                row.get("id") == Some(&status_id)
            })
            .await?
            .first()
            .and_then(|row| row.get("name").map(str::to_owned));
        let label_ids: HashSet<String> = reader
            .rows("task_labels", &["task_id", "label_id"], |row| {
                row.get("task_id") == Some(&id)
            })
            .await?
            .iter()
            .filter_map(|row| row.get("label_id").map(str::to_owned))
            .collect();
        let label_names: Vec<String> = if label_ids.is_empty() {
            Vec::new()
        } else {
            reader
                .rows("labels", &["id", "name"], |row| {
                    row.get("id").is_some_and(|id| label_ids.contains(id))
                })
                .await?
                .iter()
                .filter_map(|row| row.get("name").map(str::to_owned))
                .collect()
        };
        let assignees: Vec<String> = reader
            .rows("task_assignees", &["task_id", "user_id"], |row| {
                row.get("task_id") == Some(&id)
            })
            .await?
            .iter()
            .filter_map(|row| row.get("user_id").map(str::to_owned))
            .collect();
        let mut reference_columns = vec![
            "id",
            "workspace_id",
            "task_id",
            "comment_id",
            "blob_id",
            "display_name",
            "media_type",
        ];
        reference_columns
            .extend(optional_columns(reader, "attachment_references", &["inline"]).await?);
        let references = reader
            .rows("attachment_references", &reference_columns, |row| {
                row.get("task_id") == Some(&id)
                    && row.get("workspace_id") == Some(&workspace)
                    && row.get("comment_id").is_none()
            })
            .await?;
        let snapshot_task = SnapshotTask {
            id: task_id,
            project_id: parse_snapshot_id(task.get("project_id").unwrap_or_default())?,
            title: task.get("title").unwrap_or_default().to_owned(),
            description: task.get("description").unwrap_or_default().to_owned(),
            description_json: task.get("description_json").map(str::to_owned),
            priority: task.get("priority").unwrap_or("none").to_owned(),
            due_at: task.get("due_at").and_then(|value| value.parse().ok()),
            due_start_at: task
                .get("due_start_at")
                .and_then(|value| value.parse().ok()),
            status_name,
            label_names,
            assignees,
        };

        let mut staging = BlobStaging::new(&self.uploads, workspace_id, actor_id);
        let result = match staging
            .prepare(
                &self.database,
                snapshot,
                reader,
                &references,
                "display_name",
            )
            .await
        {
            Ok(()) => {
                self.write_task(
                    &mut staging,
                    &snapshot_task,
                    &references,
                    source_meta(snapshot, workspace_id, actor_id, request_id),
                )
                .await
            }
            Err(error) => Err(error),
        };
        if result.is_err() {
            staging.discard().await;
        }
        result
    }

    async fn write_task(
        &self,
        staging: &mut BlobStaging<'_>,
        task: &SnapshotTask,
        references: &[SnapshotRow],
        meta: Meta<'_>,
    ) -> Result<RestoredItem, BackupItemError> {
        let Meta {
            backup_id,
            workspace_id,
            actor_id,
            request_id,
        } = meta;
        let finalization = self.uploads.begin_finalization().await;
        let now = self.database.database_now().await?;
        let mut tx = self.database.immediate_transaction().await?;
        require_live_workspace(&mut tx, workspace_id).await?;
        let Flag(project_live) = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM projects WHERE id = $1 AND workspace_id = $2 AND deleted_at IS NULL)",
        )
        .bind(task.project_id.to_string())
        .bind(workspace_id.to_string())
        .fetch_one(&mut *tx)
        .await?;
        if !project_live {
            return Err(BackupItemError::ProjectMissing);
        }
        ensure_trial_capacity(&self.database, &mut tx, workspace_id, TrialLimit::Tasks, 1).await?;

        // A duplicate status needs a relation to the original, which is not restored.
        let statuses = sqlx::query(
            "SELECT id, name FROM task_statuses WHERE workspace_id = $1 AND project_id = $2 \
             AND category <> 'duplicate' ORDER BY position, id",
        )
        .bind(workspace_id.to_string())
        .bind(task.project_id.to_string())
        .fetch_all(&mut *tx)
        .await?;
        let wanted_status = task.status_name.as_deref().map(str::to_lowercase);
        let status_id: String = statuses
            .iter()
            .find(|row| Some(row.get::<String, _>("name").to_lowercase()) == wanted_status)
            .or_else(|| statuses.first())
            .map(|row| row.get("id"))
            .ok_or_else(|| unreadable("the project has no status"))?;
        let wanted_labels: HashSet<String> = task
            .label_names
            .iter()
            .map(|name| name.to_lowercase())
            .collect();
        let label_ids: Vec<String> = sqlx::query(
            "SELECT id, name FROM labels WHERE workspace_id = $1 AND project_id = $2 ORDER BY name, id",
        )
        .bind(workspace_id.to_string())
        .bind(task.project_id.to_string())
        .fetch_all(&mut *tx)
        .await?
        .into_iter()
        .filter(|row| wanted_labels.contains(&row.get::<String, _>("name").to_lowercase()))
        .map(|row| row.get("id"))
        .collect();
        let memberships: HashMap<String, String> =
            sqlx::query("SELECT id, user_id FROM memberships WHERE workspace_id = $1")
                .bind(workspace_id.to_string())
                .fetch_all(&mut *tx)
                .await?
                .into_iter()
                .map(|row| (row.get("user_id"), row.get("id")))
                .collect();

        let blobs = staging.resolve(&finalization, &mut tx, now).await?;
        let new_id = Id::new_v7();
        let mut copies = Vec::new();
        for reference in references {
            let Some(old) = reference.get("id").and_then(|id| id.parse::<Id>().ok()) else {
                continue;
            };
            let Some(Some(blob)) = reference.get("blob_id").and_then(|id| blobs.get(id)) else {
                continue;
            };
            copies.push((old, Id::new_v7(), reference, blob));
        }
        // File URLs in the description point at the task's own attachments.
        let rewrite = |text: &str| {
            copies
                .iter()
                .fold(text.to_owned(), |text, (old, new, _, _)| {
                    text.replace(
                        &attachment_path(workspace_id, task.id, *old),
                        &attachment_path(workspace_id, new_id, *new),
                    )
                })
        };
        // The markdown copy stays as it was; the blocks are checked for the new task.
        let description = rewrite(&task.description);
        let description_json = match &task.description_json {
            Some(json) => {
                let Ok(Value::Array(blocks)) = serde_json::from_str::<Value>(&rewrite(json)) else {
                    return Err(unreadable("a task has an invalid description"));
                };
                let (_, blocks) = description_from_blocks(workspace_id, new_id, &blocks);
                Some(Value::Array(blocks).to_string())
            }
            None => None,
        };
        let priority =
            if ["none", "low", "medium", "high", "urgent"].contains(&task.priority.as_str()) {
                task.priority.as_str()
            } else {
                "none"
            };
        let position: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(position) + 1, 0) FROM tasks WHERE workspace_id = $1 AND project_id = $2 \
             AND status_id = $3 AND deleted_at IS NULL",
        )
        .bind(workspace_id.to_string())
        .bind(task.project_id.to_string())
        .bind(&status_id)
        .fetch_one(&mut *tx)
        .await?;
        let title = format!("{}{RESTORED_SUFFIX}", task.title);
        // A range end before its start would fail the task form; keep only the end then.
        let due_start_at = task
            .due_start_at
            .filter(|start| task.due_at.is_some_and(|end| *start <= end));
        sqlx::query(
            "INSERT INTO tasks (id, workspace_id, project_id, status_id, title, description, priority, \
             position, creator_id, due_start_at, due_at, version, created_at, updated_at, description_json) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, 0, $12, $13, $14)",
        )
        .bind(new_id.to_string())
        .bind(workspace_id.to_string())
        .bind(task.project_id.to_string())
        .bind(&status_id)
        .bind(&title)
        .bind(&description)
        .bind(priority)
        .bind(position)
        .bind(actor_id.to_string())
        .bind(due_start_at)
        .bind(task.due_at)
        .bind(now.as_millis())
        .bind(now.as_millis())
        .bind(description_json)
        .execute(&mut *tx)
        .await?;
        for user_id in &task.assignees {
            let Some(membership_id) = memberships.get(user_id) else {
                continue;
            };
            sqlx::query(
                "INSERT INTO task_assignees (task_id, membership_id, user_id) VALUES ($1, $2, $3)",
            )
            .bind(new_id.to_string())
            .bind(membership_id)
            .bind(user_id)
            .execute(&mut *tx)
            .await?;
        }
        for label_id in &label_ids {
            sqlx::query("INSERT INTO task_labels (task_id, label_id) VALUES ($1, $2)")
                .bind(new_id.to_string())
                .bind(label_id)
                .execute(&mut *tx)
                .await?;
        }
        for (_, id, reference, blob) in &copies {
            sqlx::query(
                "INSERT INTO attachment_references (id, workspace_id, task_id, comment_id, owner_id, \
                 blob_id, display_name, media_type, byte_size, created_at, inline) \
                 VALUES ($1, $2, $3, NULL, $4, $5, $6, $7, $8, $9, $10)",
            )
            .bind(id.to_string())
            .bind(workspace_id.to_string())
            .bind(new_id.to_string())
            .bind(actor_id.to_string())
            .bind(blob.id.to_string())
            .bind(file_name(reference.get("display_name")))
            .bind(blob.media_type(reference.get("media_type")))
            .bind(blob.size)
            .bind(now.as_millis())
            .bind(i64::from(reference.get("inline") == Some("1")))
            .execute(&mut *tx)
            .await?;
        }
        let missing_files = i64::try_from(references.len() - copies.len()).unwrap_or(i64::MAX);
        audit::record(
            &mut tx,
            workspace_id,
            Some(actor_id),
            AUDIT_ACTION,
            AuditOutcome::Success,
            "task",
            Some(new_id),
            request_id,
            json!({
                "backup_id": backup_id,
                "kind": "task",
                "source_id": task.id,
                "id": new_id,
                "missing_files": missing_files,
            }),
            now,
        )
        .await?;
        let row = sqlx::query(
            "SELECT tasks.number AS number, projects.project_key AS project_key FROM tasks \
             JOIN projects ON projects.id = tasks.project_id WHERE tasks.id = $1",
        )
        .bind(new_id.to_string())
        .fetch_one(&mut *tx)
        .await?;
        let number: Option<i64> = row.get("number");
        let project_key: String = row.get("project_key");
        tx.commit().await?;
        drop(finalization);
        let slug = match number {
            Some(number) if number > 0 => format!("{project_key}-{number}"),
            _ => new_id.to_string(),
        };
        Ok(RestoredItem {
            kind: BackupItemKind::Task,
            id: new_id.to_string(),
            title,
            path: format!("/tasks/{slug}"),
            missing_files,
        })
    }
}

struct Meta<'a> {
    backup_id: &'a str,
    workspace_id: Id,
    actor_id: Id,
    request_id: &'a str,
}

fn source_meta<'a>(
    snapshot: &'a BackupSnapshot,
    workspace_id: Id,
    actor_id: Id,
    request_id: &'a str,
) -> Meta<'a> {
    Meta {
        backup_id: &snapshot.id,
        workspace_id,
        actor_id,
        request_id,
    }
}

struct SnapshotPage {
    id: Id,
    parent_id: Option<Id>,
    title: String,
    icon: Option<String>,
    cover_url: Option<String>,
    cover_position: Option<String>,
    full_width: bool,
    teamspace_id: Option<Id>,
    owner_id: Option<Id>,
    content: Vec<Value>,
}

struct SnapshotTask {
    id: Id,
    project_id: Id,
    title: String,
    description: String,
    description_json: Option<String>,
    priority: String,
    due_at: Option<i64>,
    due_start_at: Option<i64>,
    status_name: Option<String>,
    label_names: Vec<String>,
    assignees: Vec<String>,
}

struct PageNode {
    id: String,
    parent_id: Option<String>,
    title: String,
    deleted: bool,
    /// The page whose delete trashed this one.
    trashed_with: Option<String>,
    position: i64,
    updated_at: i64,
}

/// The pages of one workspace in the snapshot, without their content.
struct PageTree {
    nodes: HashMap<String, PageNode>,
    children: HashMap<String, Vec<String>>,
}

impl PageTree {
    async fn read(reader: &SnapshotReader, workspace_id: Id) -> Result<Self, BackupItemError> {
        let workspace = workspace_id.to_string();
        let mut columns = vec![
            "id",
            "workspace_id",
            "parent_id",
            "title",
            "deleted_at",
            "updated_at",
        ];
        columns.extend(optional_columns(reader, "pages", &["trashed_with", "position"]).await?);
        let rows = reader
            .rows("pages", &columns, |row| {
                row.get("workspace_id") == Some(&workspace)
            })
            .await?;
        let mut nodes = HashMap::with_capacity(rows.len());
        let mut children: HashMap<String, Vec<String>> = HashMap::new();
        for row in rows {
            let Some(id) = row.get("id").map(str::to_owned) else {
                continue;
            };
            let parent_id = row.get("parent_id").map(str::to_owned);
            if let Some(parent) = &parent_id {
                children.entry(parent.clone()).or_default().push(id.clone());
            }
            nodes.insert(
                id.clone(),
                PageNode {
                    id,
                    parent_id,
                    title: row.get("title").unwrap_or_default().to_owned(),
                    deleted: row.get("deleted_at").is_some(),
                    trashed_with: row.get("trashed_with").map(str::to_owned),
                    position: millis(row.get("position")),
                    updated_at: millis(row.get("updated_at")),
                },
            );
        }
        for list in children.values_mut() {
            list.sort_by(|left, right| {
                let position = |id: &String| nodes.get(id).map_or(0, |node| node.position);
                position(left)
                    .cmp(&position(right))
                    .then_with(|| left.cmp(right))
            });
        }
        Ok(Self { nodes, children })
    }

    /// The direct sub-pages a restore of `node` copies: live ones under a live page, the ones
    /// trashed together with it under a trashed page.
    fn children(&self, node: &PageNode) -> Vec<&PageNode> {
        self.children
            .get(&node.id)
            .into_iter()
            .flatten()
            .filter_map(|id| self.nodes.get(id))
            .filter(|child| {
                child.deleted == node.deleted
                    && (!node.deleted || child.trashed_with == node.trashed_with)
            })
            .collect()
    }
}

/// A file's blob in the live workspace: an existing one, or one installed from staged bytes.
struct LiveBlob {
    id: Id,
    size: i64,
    /// The media type detected from re-uploaded bytes; `None` for a shared blob.
    detected: Option<String>,
}

impl LiveBlob {
    fn media_type(&self, recorded: Option<&str>) -> String {
        self.detected
            .clone()
            .or_else(|| recorded.map(str::to_owned))
            .unwrap_or_else(|| "application/octet-stream".to_owned())
    }
}

struct SnapshotBlob {
    sha256: String,
    size: i64,
    staged: Option<StagedUpload>,
}

/// The blobs a restore needs, by their id in the snapshot. Bytes that the live workspace does
/// not store are staged before the write transaction, like an upload.
struct BlobStaging<'a> {
    uploads: &'a UploadService,
    workspace_id: Id,
    actor_id: Id,
    blobs: HashMap<String, SnapshotBlob>,
}

impl<'a> BlobStaging<'a> {
    fn new(uploads: &'a UploadService, workspace_id: Id, actor_id: Id) -> Self {
        Self {
            uploads,
            workspace_id,
            actor_id,
            blobs: HashMap::new(),
        }
    }

    /// Reads the blobs of `files` (rows with `blob_id` and a file name in `name_column`) from the
    /// snapshot and stages the bytes of each one the live workspace does not store.
    async fn prepare(
        &mut self,
        database: &Database,
        snapshot: &BackupSnapshot,
        reader: &SnapshotReader,
        files: &[SnapshotRow],
        name_column: &str,
    ) -> Result<(), BackupItemError> {
        let names: HashMap<&str, &str> = files
            .iter()
            .filter_map(|file| Some((file.get("blob_id")?, file.get(name_column)?)))
            .collect();
        if names.is_empty() {
            return Ok(());
        }
        let rows = reader
            .rows(
                "attachment_blobs",
                &["id", "sha256", "byte_size", "storage_key"],
                |row| row.get("id").is_some_and(|id| names.contains_key(id)),
            )
            .await?;
        let snapshot_store = LocalBlobStore::new(snapshot.path.join("attachments"));
        for row in rows {
            let (Some(id), Some(sha256), Some(size)) = (
                row.get("id"),
                row.get("sha256"),
                row.get("byte_size")
                    .and_then(|size| size.parse::<i64>().ok()),
            ) else {
                continue;
            };
            let mut blob = SnapshotBlob {
                sha256: sha256.to_owned(),
                size,
                staged: None,
            };
            if live_blob(database.pool(), self.workspace_id, sha256, size)
                .await?
                .is_none()
                && let Some(bytes) = self
                    .bytes(&snapshot_store, row.get("storage_key").unwrap_or_default())
                    .await
            {
                match self
                    .uploads
                    .stage(
                        self.workspace_id,
                        self.actor_id,
                        file_name(names.get(id).copied()).as_str(),
                        bytes.as_slice(),
                    )
                    .await
                {
                    Ok(staged) => blob.staged = Some(staged),
                    // Over the configured file size, or an unusable name: such a file is left out.
                    Err(
                        UploadError::FileTooLarge { .. }
                        | UploadError::RequestTooLarge { .. }
                        | UploadError::InvalidDisplayName,
                    ) => {}
                    Err(error) => return Err(error.into()),
                }
            }
            self.blobs.insert(id.to_owned(), blob);
        }
        Ok(())
    }

    /// The bytes of a blob: from the snapshot's attachments, else from the live store (local or
    /// S3, which keeps released objects for a while). `None` when neither has them.
    async fn bytes(&self, snapshot_store: &LocalBlobStore, storage_key: &str) -> Option<Vec<u8>> {
        if let Ok(path) = snapshot_store.path(storage_key)
            && let Ok(bytes) = tokio::fs::read(&path).await
        {
            return Some(bytes);
        }
        self.uploads.read_blob(storage_key).await.ok()
    }

    /// The live blob of each snapshot blob, inside the write transaction: the workspace's blob of
    /// the same bytes, or the staged bytes installed now. `None` for a file that is left out.
    async fn resolve(
        &mut self,
        finalization: &orbit_platform::UploadFinalization<'_>,
        tx: &mut Transaction<'_, Any>,
        now: TimestampMillis,
    ) -> Result<HashMap<String, Option<LiveBlob>>, BackupItemError> {
        let mut resolved = HashMap::with_capacity(self.blobs.len());
        for (id, blob) in &self.blobs {
            let live =
                match live_blob(&mut **tx, self.workspace_id, &blob.sha256, blob.size).await? {
                    Some(live) => Some(LiveBlob {
                        id: live,
                        size: blob.size,
                        detected: None,
                    }),
                    None => match &blob.staged {
                        Some(staged) => {
                            let installed = finalization
                                .finalize_blob_in_transaction(tx, staged, now)
                                .await?;
                            Some(LiveBlob {
                                id: installed.id,
                                size: blob.size,
                                detected: Some(staged.detected_media_type.clone()),
                            })
                        }
                        None => None,
                    },
                };
            resolved.insert(id.clone(), live);
        }
        Ok(resolved)
    }

    /// Abandons staged bytes after a failed restore. A committed restore completed them.
    async fn discard(&mut self) {
        for blob in self.blobs.values_mut() {
            if let Some(staged) = blob.staged.take() {
                let _ = self.uploads.discard(&staged).await;
            }
        }
    }
}

/// The workspace's live blob of these bytes.
async fn live_blob<'e, E>(
    executor: E,
    workspace_id: Id,
    sha256: &str,
    size: i64,
) -> Result<Option<Id>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = Any>,
{
    sqlx::query_scalar::<_, String>(
        "SELECT id FROM attachment_blobs WHERE workspace_id = $1 AND sha256 = $2 AND byte_size = $3 \
         AND deleted_at IS NULL",
    )
    .bind(workspace_id.to_string())
    .bind(sha256)
    .bind(size)
    .fetch_optional(executor)
    .await?
    .map(|id| {
        id.parse()
            .map_err(|error| sqlx::Error::Decode(Box::new(error)))
    })
    .transpose()
}

async fn require_live_workspace(
    tx: &mut Transaction<'_, Any>,
    workspace_id: Id,
) -> Result<(), BackupItemError> {
    let Flag(live) = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM workspaces WHERE id = $1 AND deleted_at IS NULL)",
    )
    .bind(workspace_id.to_string())
    .fetch_one(&mut **tx)
    .await?;
    if live {
        Ok(())
    } else {
        Err(BackupItemError::WorkspaceNotFound)
    }
}

async fn is_member(
    tx: &mut Transaction<'_, Any>,
    workspace_id: Id,
    user_id: Id,
) -> Result<bool, sqlx::Error> {
    let Flag(member) = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM memberships WHERE workspace_id = $1 AND user_id = $2)",
    )
    .bind(workspace_id.to_string())
    .bind(user_id.to_string())
    .fetch_one(&mut **tx)
    .await?;
    Ok(member)
}

/// True when `page_id` is a live page (not in the trash) of `space`.
async fn live_page_in(
    tx: &mut Transaction<'_, Any>,
    workspace_id: Id,
    page_id: Id,
    space: PageSpace,
) -> Result<bool, sqlx::Error> {
    let Flag(live) = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM pages WHERE id = $1 AND workspace_id = $2 AND deleted_at IS NULL \
         AND teamspace_id IS NOT DISTINCT FROM $3 AND owner_id IS NOT DISTINCT FROM $4)",
    )
    .bind(page_id.to_string())
    .bind(workspace_id.to_string())
    .bind(space.teamspace_column())
    .bind(space.owner_column())
    .fetch_one(&mut **tx)
    .await?;
    Ok(live)
}

/// The ones of `wanted` that the snapshot's `table` has (older schemas lack some).
async fn optional_columns<'a>(
    reader: &SnapshotReader,
    table: &str,
    wanted: &[&'a str],
) -> Result<Vec<&'a str>, BackupItemError> {
    let columns = reader
        .columns(table)
        .await?
        .ok_or_else(|| unreadable(format!("the snapshot has no table {table}")))?;
    Ok(wanted
        .iter()
        .copied()
        .filter(|column| columns.iter().any(|name| name == column))
        .collect())
}

fn page_error(error: super::pages::PageError) -> BackupItemError {
    match error {
        super::pages::PageError::Unavailable(error) => BackupItemError::Database(error),
        _ => unreadable("a page id is invalid"),
    }
}

fn parse_snapshot_id(value: &str) -> Result<Id, BackupItemError> {
    value
        .parse()
        .map_err(|_| unreadable(format!("{value} is not a valid id")))
}

fn millis(value: Option<&str>) -> i64 {
    value.and_then(|value| value.parse().ok()).unwrap_or(0)
}

/// A file name the upload path accepts.
fn file_name(name: Option<&str>) -> String {
    match name.map(str::trim) {
        Some(name) if !name.is_empty() => name.to_owned(),
        _ => "file".to_owned(),
    }
}

/// The path of a task attachment (its `/download` URL starts with it).
fn attachment_path(workspace_id: Id, task_id: Id, attachment_id: Id) -> String {
    format!("/api/v1/workspaces/{workspace_id}/tasks/{task_id}/attachments/{attachment_id}/")
}
