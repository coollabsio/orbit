//! Task and doc trash tools: list, read and restore.
//!
//! A task's `version` here is its [`revision`], as in [`TaskView`], and a doc's `version` is its
//! revision, as in [`super::DocView`]: the repositories check the stored version, so a restore
//! checks the revision first and then sends the stored version on.
use orbit_platform::{Id, TimestampMillis};
use serde::Serialize;
use serde_json::json;
use utoipa::ToSchema;

use super::{Authorized, DocView, Result, TaskView, hidden_page, page_limit};
use crate::repositories::api_tokens::ApiTokenScope;
use crate::repositories::pages::{PageError, PageTrash, revision};
use crate::repositories::tasks::{LabelRecord, Page, StatusRecord, TaskError, TaskRecord};

/// A task in the trash, in the public form of [`TaskView`] (no references outside the grant).
#[derive(Clone, Debug, Serialize, ToSchema)]
#[schema(as = PublicTrashedTask)]
pub struct TrashedTaskView {
    #[serde(flatten)]
    pub task: TaskView,
    #[schema(value_type = Option<String>, format = DateTime, required = true)]
    pub deleted_at: Option<TimestampMillis>,
    /// The sub-issues trashed with it, which a restore brings back too (list only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trashed_descendant_count: Option<i64>,
    /// `status`, `labels` and `deleted_by`: a single trashed task only, not the list.
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = TrashedTaskPreview)]
    pub preview: Option<TrashedTaskPreview>,
}

/// What `get_trashed_task` adds: the task's own status and labels, and who trashed it.
#[derive(Clone, Debug, Serialize, ToSchema)]
#[schema(as = PublicTrashedTaskPreview)]
pub struct TrashedTaskPreview {
    #[schema(required = false)]
    pub status: StatusRecord,
    #[schema(required = false)]
    pub labels: Vec<LabelRecord>,
    /// The user who moved the task to the trash; `null` for a service account or when unknown.
    #[schema(value_type = Option<String>)]
    pub deleted_by: Option<Id>,
}

impl Authorized<'_> {
    /// Tasks trashed in approved projects, last trashed first. The repository reads the whole
    /// workspace's trash, so a page can hold fewer than `limit` items; follow `next_cursor`.
    pub async fn task_trash(
        &self,
        cursor: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Page<TrashedTaskView>> {
        self.need(ApiTokenScope::TasksRead)?;
        let limit = page_limit(cursor, limit)?;
        let p = self.principal;
        let page = self
            .granted()
            .task_trash(
                p.workspace_id,
                p.actor_id,
                cursor,
                limit,
                TimestampMillis::now(),
            )
            .await?;
        let tasks: Vec<TaskRecord> = page
            .items
            .into_iter()
            .filter(|task| self.approved(task.project_id))
            .collect();
        let extra: Vec<_> = tasks
            .iter()
            .map(|task| (task.deleted_at, task.trashed_descendant_count))
            .collect();
        let items = self
            .views(tasks)
            .await?
            .into_iter()
            .zip(extra)
            .map(|(task, (deleted_at, count))| TrashedTaskView {
                task,
                deleted_at,
                trashed_descendant_count: count,
                preview: None,
            })
            .collect();
        Ok(Page {
            items,
            next_cursor: page.next_cursor,
        })
    }

    pub async fn get_trashed_task(&self, task_ref: &str) -> Result<TrashedTaskView> {
        self.need(ApiTokenScope::TasksRead)?;
        let (task, preview) = self.trashed(task_ref).await?;
        let deleted_at = task.deleted_at;
        Ok(TrashedTaskView {
            task: self.view(task).await?,
            deleted_at,
            trashed_descendant_count: None,
            preview: Some(preview),
        })
    }

    /// Restores a trashed task with the sub-issues trashed with it.
    pub async fn restore_task(&self, task_ref: &str, expected_version: u64) -> Result<TaskView> {
        self.need(ApiTokenScope::TasksWrite)?;
        let (current, _) = self.trashed(task_ref).await?;
        if revision(current.version, current.updated_at) != expected_version {
            let current = json!(self.view(current).await?);
            return Err(TaskError::VersionConflict {
                current: Box::new(current),
            }
            .into());
        }
        let p = self.principal;
        let task = self
            .granted()
            .restore_task(
                p.workspace_id,
                current.id,
                p.actor_id,
                current.version,
                self.request_id,
                TimestampMillis::now(),
            )
            .await?;
        self.view(task).await
    }

    /// A task trashed itself in an approved, live project, by UUID or identifier. A task that
    /// is only in a trashed project is not found: restoring the project brings it back.
    async fn trashed(&self, task_ref: &str) -> Result<(TaskRecord, TrashedTaskPreview)> {
        let p = self.principal;
        let id = self.task_id(task_ref).await?;
        let trashed = self
            .granted()
            .trashed_task(p.workspace_id, id, p.actor_id, TimestampMillis::now())
            .await?;
        if trashed.task.deleted_at.is_none() || !self.approved(trashed.task.project_id) {
            return Err(TaskError::NotFound.into());
        }
        Ok((
            trashed.task,
            TrashedTaskPreview {
                status: trashed.status,
                labels: trashed.labels,
                deleted_by: trashed.deleted_by,
            },
        ))
    }

    /// The docs trashed directly (not with a parent) that the person can see.
    pub async fn doc_trash(&self) -> Result<PageTrash> {
        let pages = self.content(ApiTokenScope::DocsRead, &self.api.pages)?;
        let p = self.principal;
        let mut trash = pages
            .page_trash(p.workspace_id, p.actor_id, TimestampMillis::now())
            .await
            .map_err(hidden_page)?;
        for page in &mut trash.items {
            page.version = revision(page.version, page.updated_at);
        }
        Ok(trash)
    }

    /// Restores a trashed doc with the sub-docs trashed with it.
    pub async fn restore_doc(&self, page_id: Id, expected_version: u64) -> Result<DocView> {
        let pages = self.content(ApiTokenScope::DocsWrite, &self.api.pages)?;
        let p = self.principal;
        let now = TimestampMillis::now();
        let mut current = pages
            .trashed_page(p.workspace_id, page_id, p.actor_id, now)
            .await
            .map_err(hidden_page)?
            .page;
        let current_revision = revision(current.version, current.updated_at);
        if current_revision != expected_version {
            current.version = current_revision;
            return Err(PageError::VersionConflict {
                current: Box::new(json!(current)),
            }
            .into());
        }
        let page = pages
            .restore_page(
                p.workspace_id,
                page_id,
                p.actor_id,
                current.version,
                self.request_id,
                now,
            )
            .await?;
        self.doc_view(pages, page).await
    }
}
