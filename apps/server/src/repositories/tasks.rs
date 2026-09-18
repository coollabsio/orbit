use orbit_domain::{DomainError, MAX_PARENT_DEPTH, StatusCategory, check_parent_edge, rich_text};
use orbit_platform::{Database, Id, TimestampMillis};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use serde_json::{Value, json};
use sqlx::{QueryBuilder, Row, Sqlite, Transaction};
use thiserror::Error;
use utoipa::ToSchema;

use crate::audit::{self, AuditOutcome};

const TRASH_RETENTION_MILLIS: i64 = 30 * 24 * 60 * 60 * 1_000;

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct ProjectRecord {
    #[schema(value_type = String)]
    pub id: Id,
    #[schema(value_type = String)]
    pub workspace_id: Id,
    pub name: String,
    pub key: String,
    pub color: String,
    pub version: u64,
    #[schema(value_type = Option<String>, format = DateTime)]
    pub deleted_at: Option<TimestampMillis>,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: TimestampMillis,
    #[schema(value_type = String, format = DateTime)]
    pub updated_at: TimestampMillis,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct StatusRecord {
    #[schema(value_type = String)]
    pub id: Id,
    #[schema(value_type = String)]
    pub workspace_id: Id,
    #[schema(value_type = String)]
    pub project_id: Id,
    pub name: String,
    pub description: String,
    pub color: String,
    pub category: String,
    pub position: i64,
    pub version: u64,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct LabelRecord {
    #[schema(value_type = String)]
    pub id: Id,
    #[schema(value_type = String)]
    pub workspace_id: Id,
    pub name: String,
    pub color: String,
    pub version: u64,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct TaskRecord {
    #[schema(value_type = String)]
    pub id: Id,
    #[schema(value_type = String)]
    pub workspace_id: Id,
    #[schema(value_type = String)]
    pub project_id: Id,
    #[schema(value_type = String)]
    pub status_id: Id,
    pub identifier: String,
    pub identifier_key: String,
    pub number: i64,
    pub title: String,
    #[schema(value_type = Object)]
    pub description_json: Value,
    pub description_text: String,
    pub priority: String,
    pub position: i64,
    #[schema(value_type = Option<String>)]
    pub creator_id: Option<Id>,
    #[schema(value_type = Option<String>)]
    pub creator_service_account_id: Option<Id>,
    pub creator_service_account_name: Option<String>,
    /// The task this one is a sub-issue of. Any depth, any project, same workspace.
    #[schema(value_type = Option<String>)]
    pub parent_id: Option<Id>,
    /// Live sub-issues of this task.
    pub sub_issue_total: i64,
    /// Live sub-issues in a completed or cancelled status.
    pub sub_issue_done: i64,
    /// Set when this task was marked as a duplicate of another. Nothing moved:
    /// both tasks stay fully readable.
    #[schema(value_type = Option<String>)]
    pub duplicate_of_task_id: Option<Id>,
    /// Tasks marked as duplicates of this one. Detail reads only; `[]` elsewhere.
    #[schema(value_type = Vec<String>)]
    pub duplicate_ids: Vec<Id>,
    /// Live tasks and comments whose rich text mentions this task, capped at 50.
    /// Detail reads only; `[]` elsewhere.
    pub referenced_by: Vec<TaskReferenceRecord>,
    #[schema(value_type = Vec<String>)]
    pub assignee_ids: Vec<Id>,
    #[schema(value_type = Vec<String>)]
    pub label_ids: Vec<Id>,
    #[schema(value_type = Option<String>, format = DateTime)]
    pub due_at: Option<TimestampMillis>,
    pub version: u64,
    #[schema(value_type = Option<String>, format = DateTime)]
    pub deleted_at: Option<TimestampMillis>,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: TimestampMillis,
    #[schema(value_type = String, format = DateTime)]
    pub updated_at: TimestampMillis,
}

/// A derived backlink: a task description or comment that mentions a task.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct TaskReferenceRecord {
    /// `task` or `comment`.
    pub source_type: String,
    /// The task id for a description, the comment id for a comment.
    #[schema(value_type = String)]
    pub source_id: Id,
    /// The task to navigate to: the source task itself, or a comment's task.
    #[schema(value_type = String)]
    pub source_task_id: Id,
    /// Identifier of `source_task_id`, so a backlink renders without a lookup.
    pub source_task_identifier: String,
    pub source_task_title: String,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct CommentRecord {
    #[schema(value_type = String)]
    pub id: Id,
    #[schema(value_type = String)]
    pub workspace_id: Id,
    #[schema(value_type = String)]
    pub task_id: Id,
    #[schema(value_type = String)]
    pub author_id: Id,
    #[schema(value_type = Option<String>)]
    pub parent_id: Option<Id>,
    #[schema(value_type = Object)]
    pub body_json: Value,
    pub body_text: String,
    pub version: u64,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: TimestampMillis,
    #[schema(value_type = String, format = DateTime)]
    pub updated_at: TimestampMillis,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct NotificationRecord {
    #[schema(value_type = String)]
    pub id: Id,
    #[schema(value_type = String)]
    pub workspace_id: Id,
    #[schema(value_type = String)]
    pub recipient_user_id: Id,
    #[schema(value_type = String)]
    pub actor_user_id: Id,
    pub kind: String,
    #[schema(value_type = String)]
    pub task_id: Id,
    #[schema(value_type = Option<String>)]
    pub comment_id: Option<Id>,
    #[schema(value_type = Option<String>, format = DateTime)]
    pub read_at: Option<TimestampMillis>,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: TimestampMillis,
}

#[derive(Clone, Debug)]
pub struct CreateTask {
    pub project_id: Id,
    pub status_id: Id,
    pub title: String,
    pub description_json: Value,
    pub priority: String,
    pub position: Option<i64>,
    pub assignee_ids: Vec<Id>,
    pub label_ids: Vec<Id>,
    pub due_at: Option<TimestampMillis>,
    pub parent_id: Option<Id>,
}

#[derive(Clone, Debug)]
pub struct DiscordTask {
    pub event_id: String,
    pub payload_hash: [u8; 32],
    pub project_id: Id,
    pub title: String,
    pub description: String,
}

#[derive(Clone, Debug, Default)]
pub struct TaskChanges {
    pub project_id: Option<Id>,
    pub status_id: Option<Id>,
    pub title: Option<String>,
    pub description_json: Option<Value>,
    pub priority: Option<String>,
    pub position: Option<i64>,
    pub assignee_ids: Option<Vec<Id>>,
    pub label_ids: Option<Vec<Id>>,
    pub due_at: Option<Option<TimestampMillis>>,
    /// Outer `Some` = field present, inner `None` = detach from the parent.
    pub parent_id: Option<Option<Id>>,
}

#[derive(Clone, Debug)]
pub struct TaskUpdate {
    pub id: Id,
    pub expected_version: u64,
    pub changes: TaskChanges,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TaskSort {
    Position,
    Priority,
    Title,
    CreatedAt,
    UpdatedAt,
    /// fts5 bm25 ranking. Requires a search term and is never cursor-paginated.
    Relevance,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SortOrder {
    Asc,
    Desc,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaskNesting {
    /// Flat list, sub-issues included. Default: preserves pre-0016 behaviour.
    All,
    /// Only tasks without a parent.
    Roots,
}

#[derive(Clone, Debug)]
pub struct TaskFilter {
    pub project_id: Option<Id>,
    pub status_id: Option<Id>,
    pub assignee_id: Option<Id>,
    pub label_id: Option<Id>,
    /// Exact `(identifier_key, number)` seek against the `tasks_identifier` unique index.
    pub identifier: Option<(String, i64)>,
    pub priority: Option<String>,
    pub search: Option<String>,
    pub view: Option<String>,
    /// Only the direct sub-issues of this task. Takes precedence over `nesting`.
    pub parent_id: Option<Id>,
    pub nesting: TaskNesting,
    pub sort: TaskSort,
    pub order: SortOrder,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Error)]
pub enum TaskError {
    #[error("task resource was not found")]
    NotFound,
    #[error("task input is invalid: {field}")]
    Invalid { field: &'static str },
    #[error("rich text document is invalid: {reason}")]
    InvalidDocument {
        field: &'static str,
        reason: &'static str,
    },
    #[error("task operation conflicts with current state")]
    Conflict,
    #[error("integration event conflicts with its original payload")]
    IntegrationConflict,
    #[error("restore conflicts with the current {field}")]
    RestoreConflict { field: &'static str },
    #[error("the supplied cursor is invalid")]
    InvalidCursor,
    #[error("stale version")]
    VersionConflict { current: Box<Value> },
    #[error("task repository is unavailable")]
    Unavailable(#[from] sqlx::Error),
}

#[derive(Clone)]
pub struct TaskRepository {
    database: Database,
}

impl TaskRepository {
    pub async fn task_activity(
        &self,
        workspace_id: Id,
        task_id: Id,
        actor_id: Id,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<Page<audit::AuditEvent>, TaskError> {
        require_access(self.database.pool(), workspace_id, actor_id).await?;
        self.get_task(workspace_id, task_id, actor_id).await?;
        let cursor = cursor
            .map(str::parse)
            .transpose()
            .map_err(|_| TaskError::InvalidCursor)?;
        let (items, next) = audit::list_resource(
            &self.database,
            workspace_id,
            "task",
            task_id,
            cursor,
            limit.clamp(1, 100),
        )
        .await?;
        Ok(Page {
            items,
            next_cursor: next.map(|id| id.to_string()),
        })
    }

    #[must_use]
    pub fn new(database: Database) -> Self {
        Self { database }
    }

    pub async fn projects(
        &self,
        workspace_id: Id,
        actor_id: Id,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<Page<ProjectRecord>, TaskError> {
        require_access(self.database.pool(), workspace_id, actor_id).await?;
        let fingerprint = format!("projects:{workspace_id}");
        let after = cursor_pair(cursor, &fingerprint)?;
        let mut query = QueryBuilder::<Sqlite>::new(
            "SELECT id, workspace_id, name, project_key, color, version, deleted_at, created_at, updated_at \
             FROM projects WHERE workspace_id = ",
        );
        query
            .push_bind(workspace_id.to_string())
            .push(" AND deleted_at IS NULL");
        if let Some((name, id)) = after {
            query
                .push(" AND (name > ")
                .push_bind(name.clone())
                .push(" OR (name = ")
                .push_bind(name)
                .push(" AND id > ")
                .push_bind(id.to_string())
                .push("))");
        }
        let limit = limit.clamp(1, 100);
        query
            .push(" ORDER BY name, id LIMIT ")
            .push_bind((limit + 1) as i64);
        let items = query
            .build()
            .fetch_all(self.database.pool())
            .await?
            .into_iter()
            .map(project_from_row)
            .collect::<Result<Vec<_>, _>>()?;
        finish_page(items, limit, &fingerprint, |project| {
            vec![project.name.clone(), project.id.to_string()]
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn create_project(
        &self,
        workspace_id: Id,
        actor_id: Id,
        name: String,
        key: String,
        color: String,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<ProjectRecord, TaskError> {
        let project_id = Id::new_v7();
        let mut tx = self.database.immediate_transaction().await?;
        require_access_tx(&mut tx, workspace_id, actor_id).await?;
        // A deleted project's key can be reclaimed while its tasks keep their numbers, so start
        // past anything already issued under this key or the identifier index will fire.
        let next_task_number: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(number), 0) + 1 FROM tasks \
             WHERE workspace_id = ? AND identifier_key = ?",
        )
        .bind(workspace_id.to_string())
        .bind(&key)
        .fetch_one(&mut *tx)
        .await?;
        let inserted = sqlx::query(
            "INSERT INTO projects (id, workspace_id, name, project_key, color, next_task_number, version, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, 0, ?, ?)",
        )
        .bind(project_id.to_string())
        .bind(workspace_id.to_string())
        .bind(&name)
        .bind(&key)
        .bind(&color)
        .bind(next_task_number)
        .bind(now.as_millis())
        .bind(now.as_millis())
        .execute(&mut *tx)
        .await;
        if is_unique_violation(&inserted) {
            return Err(TaskError::Conflict);
        }
        inserted?;
        insert_default_statuses(&mut tx, workspace_id, project_id, now).await?;
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "project.created",
            "project",
            project_id,
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(ProjectRecord {
            id: project_id,
            workspace_id,
            name,
            key,
            color,
            version: 0,
            deleted_at: None,
            created_at: now,
            updated_at: now,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update_project(
        &self,
        workspace_id: Id,
        project_id: Id,
        actor_id: Id,
        name: String,
        key: String,
        color: String,
        expected_version: u64,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<ProjectRecord, TaskError> {
        let mut tx = self.database.immediate_transaction().await?;
        require_access_tx(&mut tx, workspace_id, actor_id).await?;
        let current = project_in_tx(&mut tx, workspace_id, project_id, false).await?;
        check_version(expected_version, current.version, &current)?;
        let updated = sqlx::query(
            "UPDATE projects SET name = ?, project_key = ?, color = ?, version = version + 1, updated_at = ? \
             WHERE id = ? AND workspace_id = ? AND deleted_at IS NULL AND version = ?",
        )
        .bind(&name)
        .bind(&key)
        .bind(&color)
        .bind(now.as_millis())
        .bind(project_id.to_string())
        .bind(workspace_id.to_string())
        .bind(expected_version as i64)
        .execute(&mut *tx)
        .await;
        if is_unique_violation(&updated) {
            return Err(TaskError::Conflict);
        }
        if updated?.rows_affected() != 1 {
            return Err(TaskError::Conflict);
        }
        // Renaming a key rewrites the identifiers it issued, like Linear. Kept in this
        // transaction so a collision with numbers left by a deleted project rolls the rename back.
        if key != current.key {
            let rewritten = sqlx::query(
                "UPDATE tasks SET identifier_key = ? WHERE workspace_id = ? AND identifier_key = ?",
            )
            .bind(&key)
            .bind(workspace_id.to_string())
            .bind(&current.key)
            .execute(&mut *tx)
            .await;
            if is_unique_violation(&rewritten) {
                return Err(TaskError::Conflict);
            }
            rewritten?;
            sqlx::query(
                "UPDATE projects SET next_task_number = MAX(next_task_number, 1 + COALESCE(\
                    (SELECT MAX(number) FROM tasks WHERE workspace_id = ? AND identifier_key = ?), 0)) \
                 WHERE id = ? AND workspace_id = ?",
            )
            .bind(workspace_id.to_string())
            .bind(&key)
            .bind(project_id.to_string())
            .bind(workspace_id.to_string())
            .execute(&mut *tx)
            .await?;
        }
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "project.updated",
            "project",
            project_id,
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(ProjectRecord {
            name,
            key,
            color,
            version: current.version + 1,
            updated_at: now,
            ..current
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn delete_project(
        &self,
        workspace_id: Id,
        project_id: Id,
        actor_id: Id,
        expected_version: u64,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), TaskError> {
        let mut tx = self.database.immediate_transaction().await?;
        require_access_tx(&mut tx, workspace_id, actor_id).await?;
        let current = project_in_tx(&mut tx, workspace_id, project_id, false).await?;
        check_version(expected_version, current.version, &current)?;
        let tombstone = format!("__deleted__{project_id}");
        sqlx::query(
            "UPDATE projects SET restore_project_key = project_key, project_key = ?, deleted_at = ?, \
             version = version + 1, updated_at = ? WHERE id = ? AND workspace_id = ? AND deleted_at IS NULL AND version = ?",
        )
        .bind(tombstone)
        .bind(now.as_millis())
        .bind(now.as_millis())
        .bind(project_id.to_string())
        .bind(workspace_id.to_string())
        .bind(expected_version as i64)
        .execute(&mut *tx)
        .await?;
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "project.deleted",
            "project",
            project_id,
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn restore_project(
        &self,
        workspace_id: Id,
        project_id: Id,
        actor_id: Id,
        expected_version: u64,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<ProjectRecord, TaskError> {
        let mut tx = self.database.immediate_transaction().await?;
        require_access_tx(&mut tx, workspace_id, actor_id).await?;
        let current = project_in_tx(&mut tx, workspace_id, project_id, true).await?;
        if current.deleted_at.is_none_or(|deleted| {
            deleted.as_millis() <= now.as_millis().saturating_sub(TRASH_RETENTION_MILLIS)
        }) {
            return Err(TaskError::NotFound);
        }
        check_version(expected_version, current.version, &current)?;
        let restore_key: Option<String> = sqlx::query_scalar(
            "SELECT restore_project_key FROM projects WHERE id = ? AND workspace_id = ?",
        )
        .bind(project_id.to_string())
        .bind(workspace_id.to_string())
        .fetch_one(&mut *tx)
        .await?;
        let restore_key = restore_key.ok_or(TaskError::Conflict)?;
        let conflict: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM projects WHERE workspace_id = ? AND project_key = ? AND deleted_at IS NULL",
        )
        .bind(workspace_id.to_string())
        .bind(&restore_key)
        .fetch_one(&mut *tx)
        .await?;
        if conflict != 0 {
            return Err(TaskError::RestoreConflict { field: "key" });
        }
        sqlx::query(
            "UPDATE projects SET project_key = ?, restore_project_key = NULL, deleted_at = NULL, \
             version = version + 1, updated_at = ? WHERE id = ? AND workspace_id = ? AND version = ?",
        )
        .bind(&restore_key)
        .bind(now.as_millis())
        .bind(project_id.to_string())
        .bind(workspace_id.to_string())
        .bind(expected_version as i64)
        .execute(&mut *tx)
        .await?;
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "project.restored",
            "project",
            project_id,
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(ProjectRecord {
            key: restore_key,
            version: current.version + 1,
            deleted_at: None,
            updated_at: now,
            ..current
        })
    }

    pub async fn project_trash(
        &self,
        workspace_id: Id,
        actor_id: Id,
        cursor: Option<&str>,
        limit: usize,
        now: TimestampMillis,
    ) -> Result<Page<ProjectRecord>, TaskError> {
        require_access(self.database.pool(), workspace_id, actor_id).await?;
        let fingerprint = format!("project-trash:{workspace_id}");
        let after = cursor_i64_pair(cursor, &fingerprint)?;
        let mut query = QueryBuilder::<Sqlite>::new(
            "SELECT id, workspace_id, name, COALESCE(restore_project_key, project_key) AS project_key, color, version, deleted_at, created_at, updated_at \
             FROM projects WHERE workspace_id = ",
        );
        query
            .push_bind(workspace_id.to_string())
            .push(" AND deleted_at > ")
            .push_bind(now.as_millis().saturating_sub(TRASH_RETENTION_MILLIS));
        if let Some((deleted_at, id)) = after {
            query
                .push(" AND (deleted_at < ")
                .push_bind(deleted_at)
                .push(" OR (deleted_at = ")
                .push_bind(deleted_at)
                .push(" AND id < ")
                .push_bind(id.to_string())
                .push("))");
        }
        let limit = limit.clamp(1, 100);
        query
            .push(" ORDER BY deleted_at DESC, id DESC LIMIT ")
            .push_bind((limit + 1) as i64);
        let items = query
            .build()
            .fetch_all(self.database.pool())
            .await?
            .into_iter()
            .map(project_from_row)
            .collect::<Result<Vec<_>, _>>()?;
        finish_page(items, limit, &fingerprint, |project| {
            vec![
                project
                    .deleted_at
                    .map_or(0, TimestampMillis::as_millis)
                    .to_string(),
                project.id.to_string(),
            ]
        })
    }

    pub async fn statuses(
        &self,
        workspace_id: Id,
        project_id: Id,
        actor_id: Id,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<Page<StatusRecord>, TaskError> {
        require_project(self.database.pool(), workspace_id, project_id, actor_id).await?;
        let fingerprint = format!("statuses:{project_id}");
        let after = cursor_i64_pair(cursor, &fingerprint)?;
        let mut query = QueryBuilder::<Sqlite>::new(
            "SELECT id, workspace_id, project_id, name, description, color, category, position, version \
             FROM task_statuses WHERE workspace_id = ",
        );
        query
            .push_bind(workspace_id.to_string())
            .push(" AND project_id = ")
            .push_bind(project_id.to_string());
        if let Some((position, id)) = after {
            query
                .push(" AND (position > ")
                .push_bind(position)
                .push(" OR (position = ")
                .push_bind(position)
                .push(" AND id > ")
                .push_bind(id.to_string())
                .push("))");
        }
        let limit = limit.clamp(1, 100);
        query
            .push(" ORDER BY position, id LIMIT ")
            .push_bind((limit + 1) as i64);
        let items = query
            .build()
            .fetch_all(self.database.pool())
            .await?
            .into_iter()
            .map(status_from_row)
            .collect::<Result<Vec<_>, _>>()?;
        finish_page(items, limit, &fingerprint, |status| {
            vec![status.position.to_string(), status.id.to_string()]
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn create_status(
        &self,
        workspace_id: Id,
        project_id: Id,
        actor_id: Id,
        name: String,
        description: String,
        color: String,
        category: String,
        position: Option<i64>,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<StatusRecord, TaskError> {
        let id = Id::new_v7();
        let mut tx = self.database.immediate_transaction().await?;
        require_project_tx(&mut tx, workspace_id, project_id, actor_id).await?;
        let position = match position { Some(value) => value, None => sqlx::query_scalar::<_, i64>("SELECT COALESCE(MAX(position) + 1, 0) FROM task_statuses WHERE workspace_id = ? AND project_id = ?").bind(workspace_id.to_string()).bind(project_id.to_string()).fetch_one(&mut *tx).await? };
        sqlx::query("INSERT INTO task_statuses (id, workspace_id, project_id, name, description, color, category, position, version, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, 0, ?, ?)")
            .bind(id.to_string()).bind(workspace_id.to_string()).bind(project_id.to_string()).bind(&name).bind(&description).bind(&color).bind(&category).bind(position).bind(now.as_millis()).bind(now.as_millis()).execute(&mut *tx).await?;
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "status.created",
            "status",
            id,
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(StatusRecord {
            id,
            workspace_id,
            project_id,
            name,
            description,
            color,
            category,
            position,
            version: 0,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update_status(
        &self,
        workspace_id: Id,
        project_id: Id,
        status_id: Id,
        actor_id: Id,
        name: String,
        description: Option<String>,
        color: String,
        category: String,
        position: i64,
        expected_version: u64,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<StatusRecord, TaskError> {
        let mut tx = self.database.immediate_transaction().await?;
        require_project_tx(&mut tx, workspace_id, project_id, actor_id).await?;
        let current = status_in_tx(&mut tx, workspace_id, project_id, status_id).await?;
        check_version(expected_version, current.version, &current)?;
        let description = description.unwrap_or_else(|| current.description.clone());
        sqlx::query("UPDATE task_statuses SET name = ?, description = ?, color = ?, category = ?, position = ?, version = version + 1, updated_at = ? WHERE id = ? AND workspace_id = ? AND project_id = ? AND version = ?")
            .bind(&name).bind(&description).bind(&color).bind(&category).bind(position).bind(now.as_millis()).bind(status_id.to_string()).bind(workspace_id.to_string()).bind(project_id.to_string()).bind(expected_version as i64).execute(&mut *tx).await?;
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "status.updated",
            "status",
            status_id,
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(StatusRecord {
            name,
            description,
            color,
            category,
            position,
            version: current.version + 1,
            ..current
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn delete_status(
        &self,
        workspace_id: Id,
        project_id: Id,
        status_id: Id,
        actor_id: Id,
        expected_version: u64,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), TaskError> {
        let mut tx = self.database.immediate_transaction().await?;
        require_project_tx(&mut tx, workspace_id, project_id, actor_id).await?;
        let current = status_in_tx(&mut tx, workspace_id, project_id, status_id).await?;
        check_version(expected_version, current.version, &current)?;
        let tasks: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM tasks WHERE workspace_id = ? AND status_id = ?",
        )
        .bind(workspace_id.to_string())
        .bind(status_id.to_string())
        .fetch_one(&mut *tx)
        .await?;
        if tasks != 0 {
            return Err(TaskError::Conflict);
        }
        sqlx::query("DELETE FROM task_statuses WHERE id = ? AND workspace_id = ? AND project_id = ? AND version = ?").bind(status_id.to_string()).bind(workspace_id.to_string()).bind(project_id.to_string()).bind(expected_version as i64).execute(&mut *tx).await?;
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "status.deleted",
            "status",
            status_id,
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn reorder_statuses(
        &self,
        workspace_id: Id,
        project_id: Id,
        actor_id: Id,
        items: &[(Id, u64, i64)],
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<Vec<StatusRecord>, TaskError> {
        let mut tx = self.database.immediate_transaction().await?;
        require_project_tx(&mut tx, workspace_id, project_id, actor_id).await?;
        let mut result = Vec::with_capacity(items.len());
        for &(id, expected, position) in items {
            let current = status_in_tx(&mut tx, workspace_id, project_id, id).await?;
            check_version(expected, current.version, &current)?;
            sqlx::query("UPDATE task_statuses SET position = ?, version = version + 1, updated_at = ? WHERE id = ? AND workspace_id = ? AND project_id = ? AND version = ?").bind(position).bind(now.as_millis()).bind(id.to_string()).bind(workspace_id.to_string()).bind(project_id.to_string()).bind(expected as i64).execute(&mut *tx).await?;
            result.push(StatusRecord {
                position,
                version: current.version + 1,
                ..current
            });
        }
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "status.reordered",
            "project",
            project_id,
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(result)
    }

    pub async fn labels(
        &self,
        workspace_id: Id,
        actor_id: Id,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<Page<LabelRecord>, TaskError> {
        require_access(self.database.pool(), workspace_id, actor_id).await?;
        let fingerprint = format!("labels:{workspace_id}");
        let after = cursor_pair(cursor, &fingerprint)?;
        let mut query = QueryBuilder::<Sqlite>::new(
            "SELECT id, workspace_id, name, color, version FROM labels WHERE workspace_id = ",
        );
        query.push_bind(workspace_id.to_string());
        if let Some((name, id)) = after {
            query
                .push(" AND (name > ")
                .push_bind(name.clone())
                .push(" OR (name = ")
                .push_bind(name)
                .push(" AND id > ")
                .push_bind(id.to_string())
                .push("))");
        }
        let limit = limit.clamp(1, 100);
        query
            .push(" ORDER BY name, id LIMIT ")
            .push_bind((limit + 1) as i64);
        let items = query
            .build()
            .fetch_all(self.database.pool())
            .await?
            .into_iter()
            .map(label_from_row)
            .collect::<Result<Vec<_>, _>>()?;
        finish_page(items, limit, &fingerprint, |label| {
            vec![label.name.clone(), label.id.to_string()]
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn create_label(
        &self,
        workspace_id: Id,
        actor_id: Id,
        name: String,
        color: String,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<LabelRecord, TaskError> {
        let id = Id::new_v7();
        let mut tx = self.database.immediate_transaction().await?;
        require_access_tx(&mut tx, workspace_id, actor_id).await?;
        let inserted = sqlx::query("INSERT INTO labels (id, workspace_id, name, color, version, created_at, updated_at) VALUES (?, ?, ?, ?, 0, ?, ?)").bind(id.to_string()).bind(workspace_id.to_string()).bind(&name).bind(&color).bind(now.as_millis()).bind(now.as_millis()).execute(&mut *tx).await;
        if is_unique_violation(&inserted) {
            return Err(TaskError::Conflict);
        }
        inserted?;
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "label.created",
            "label",
            id,
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(LabelRecord {
            id,
            workspace_id,
            name,
            color,
            version: 0,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update_label(
        &self,
        workspace_id: Id,
        label_id: Id,
        actor_id: Id,
        name: String,
        color: String,
        expected_version: u64,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<LabelRecord, TaskError> {
        let mut tx = self.database.immediate_transaction().await?;
        require_access_tx(&mut tx, workspace_id, actor_id).await?;
        let current = label_in_tx(&mut tx, workspace_id, label_id).await?;
        check_version(expected_version, current.version, &current)?;
        let updated = sqlx::query("UPDATE labels SET name = ?, color = ?, version = version + 1, updated_at = ? WHERE id = ? AND workspace_id = ? AND version = ?").bind(&name).bind(&color).bind(now.as_millis()).bind(label_id.to_string()).bind(workspace_id.to_string()).bind(expected_version as i64).execute(&mut *tx).await;
        if is_unique_violation(&updated) {
            return Err(TaskError::Conflict);
        }
        updated?;
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "label.updated",
            "label",
            label_id,
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(LabelRecord {
            name,
            color,
            version: current.version + 1,
            ..current
        })
    }

    pub async fn delete_label(
        &self,
        workspace_id: Id,
        label_id: Id,
        actor_id: Id,
        expected_version: u64,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), TaskError> {
        let mut tx = self.database.immediate_transaction().await?;
        require_access_tx(&mut tx, workspace_id, actor_id).await?;
        let current = label_in_tx(&mut tx, workspace_id, label_id).await?;
        check_version(expected_version, current.version, &current)?;
        sqlx::query("DELETE FROM task_labels WHERE label_id = ?")
            .bind(label_id.to_string())
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM labels WHERE id = ? AND workspace_id = ? AND version = ?")
            .bind(label_id.to_string())
            .bind(workspace_id.to_string())
            .bind(expected_version as i64)
            .execute(&mut *tx)
            .await?;
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "label.deleted",
            "label",
            label_id,
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }
}

impl TaskRepository {
    pub async fn tasks(
        &self,
        workspace_id: Id,
        actor_id: Id,
        filter: &TaskFilter,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<Page<TaskRecord>, TaskError> {
        require_access(self.database.pool(), workspace_id, actor_id).await?;
        let fingerprint = task_fingerprint(workspace_id, filter);
        let after = cursor_pair(cursor, &fingerprint)?;
        let mut query = QueryBuilder::<Sqlite>::new(
            "SELECT tasks.id, tasks.workspace_id, tasks.project_id, tasks.status_id, tasks.identifier_key, tasks.number, tasks.title, \
             tasks.description_json, tasks.description_text, tasks.priority, tasks.position, tasks.creator_id, tasks.creator_service_account_id, (SELECT name FROM service_accounts WHERE id = tasks.creator_service_account_id) AS creator_service_account_name, tasks.due_at, tasks.version, \
             tasks.deleted_at, tasks.created_at, tasks.updated_at, tasks.parent_id FROM tasks \
             JOIN projects ON projects.id = tasks.project_id \
             WHERE tasks.workspace_id = ",
        );
        query.push_bind(workspace_id.to_string());
        query.push(" AND tasks.deleted_at IS NULL AND projects.deleted_at IS NULL");
        if let Some(project_id) = filter.project_id {
            query
                .push(" AND tasks.project_id = ")
                .push_bind(project_id.to_string());
        }
        if let Some(status_id) = filter.status_id {
            query
                .push(" AND tasks.status_id = ")
                .push_bind(status_id.to_string());
        }
        if let Some(parent_id) = filter.parent_id {
            query
                .push(" AND tasks.parent_id = ")
                .push_bind(parent_id.to_string());
        } else if filter.nesting == TaskNesting::Roots {
            query.push(" AND tasks.parent_id IS NULL");
        }
        if let Some(priority) = &filter.priority {
            query
                .push(" AND tasks.priority = ")
                .push_bind(priority.clone());
        }
        if filter.view.as_deref() == Some("mine") || filter.assignee_id.is_some() {
            let assignee_id = if filter.view.as_deref() == Some("mine") {
                actor_id
            } else {
                filter.assignee_id.unwrap()
            };
            query
                .push(" AND EXISTS (SELECT 1 FROM task_assignees JOIN memberships ON memberships.id = task_assignees.membership_id JOIN users ON users.id = task_assignees.user_id WHERE task_assignees.task_id = tasks.id AND memberships.workspace_id = tasks.workspace_id AND memberships.user_id = task_assignees.user_id AND users.suspended_at IS NULL AND task_assignees.user_id = ")
                .push_bind(assignee_id.to_string())
                .push(")");
        }
        if matches!(
            filter.view.as_deref(),
            Some("overdue") | Some("due_soon") | Some("current_week")
        ) {
            let day_ms = 86_400_000;
            let start_of_utc_day = (TimestampMillis::now().as_millis() / day_ms) * day_ms;
            query.push(
                " AND tasks.due_at IS NOT NULL AND EXISTS (SELECT 1 FROM task_statuses WHERE task_statuses.id = tasks.status_id AND task_statuses.category NOT IN ('completed', 'cancelled'))",
            );
            if filter.view.as_deref() == Some("overdue") {
                query
                    .push(" AND tasks.due_at < ")
                    .push_bind(start_of_utc_day);
            } else if filter.view.as_deref() == Some("due_soon") {
                query
                    .push(" AND tasks.due_at >= ")
                    .push_bind(start_of_utc_day)
                    .push(" AND tasks.due_at < ")
                    .push_bind(start_of_utc_day + 7 * day_ms);
            } else {
                let days_since_epoch = start_of_utc_day / day_ms;
                let monday = start_of_utc_day - (days_since_epoch + 3).rem_euclid(7) * day_ms;
                query
                    .push(" AND tasks.due_at >= ")
                    .push_bind(monday)
                    .push(" AND tasks.due_at < ")
                    .push_bind(monday + 7 * day_ms);
            }
        }
        if let Some(label_id) = filter.label_id {
            query
                .push(" AND EXISTS (SELECT 1 FROM task_labels WHERE task_labels.task_id = tasks.id AND task_labels.label_id = ")
                .push_bind(label_id.to_string())
                .push(")");
        }
        if let Some((identifier_key, number)) = &filter.identifier {
            query
                .push(" AND tasks.identifier_key = ")
                .push_bind(identifier_key.clone())
                .push(" AND tasks.number = ")
                .push_bind(*number);
        }
        if let Some(search) = &filter.search {
            match search_match_query(workspace_id, search) {
                Some(match_query) => {
                    query
                        .push(" AND tasks.id IN (SELECT task_id FROM task_search WHERE task_search MATCH ")
                        .push_bind(match_query)
                        .push(")");
                }
                None => {
                    query.push(" AND 0");
                }
            }
        }
        if let Some((raw_value, id)) = after {
            let operator = if filter.order == SortOrder::Asc {
                ">"
            } else {
                "<"
            };
            let column = task_sort_column(&filter.sort);
            query
                .push(" AND (")
                .push(column)
                .push(" ")
                .push(operator)
                .push(" ");
            push_cursor_value(&mut query, &filter.sort, &raw_value)?;
            query.push(" OR (").push(column).push(" = ");
            push_cursor_value(&mut query, &filter.sort, &raw_value)?;
            query
                .push(" AND tasks.id ")
                .push(operator)
                .push(" ")
                .push_bind(id.to_string())
                .push("))");
        }
        let direction = if filter.order == SortOrder::Asc {
            " ASC"
        } else {
            " DESC"
        };
        query
            .push(" ORDER BY ")
            .push(task_sort_column(&filter.sort))
            .push(direction)
            .push(", tasks.id")
            .push(direction);
        let limit = limit.clamp(1, 100);
        query.push(" LIMIT ").push_bind((limit + 1) as i64);
        let rows = query.build().fetch_all(self.database.pool()).await?;
        let mut tasks = Vec::new();
        for row in rows {
            tasks.push(task_from_row(self.database.pool(), row).await?);
        }
        let has_more = tasks.len() > limit;
        tasks.truncate(limit);
        apply_task_graph(
            &mut *self.database.pool().acquire().await?,
            workspace_id,
            &mut tasks,
        )
        .await?;
        let next_cursor = if has_more {
            tasks
                .last()
                .map(|task| encode_cursor(&fingerprint, task_cursor_key(task, filter)))
                .transpose()?
        } else {
            None
        };
        Ok(Page {
            items: tasks,
            next_cursor,
        })
    }

    pub async fn get_task(
        &self,
        workspace_id: Id,
        task_id: Id,
        actor_id: Id,
    ) -> Result<TaskRecord, TaskError> {
        require_access(self.database.pool(), workspace_id, actor_id).await?;
        let row = sqlx::query(
            "SELECT tasks.id, tasks.workspace_id, tasks.project_id, tasks.status_id, tasks.identifier_key, tasks.number, tasks.title, \
             tasks.description_json, tasks.description_text, tasks.priority, tasks.position, tasks.creator_id, tasks.creator_service_account_id, (SELECT name FROM service_accounts WHERE id = tasks.creator_service_account_id) AS creator_service_account_name, tasks.due_at, tasks.version, \
             tasks.deleted_at, tasks.created_at, tasks.updated_at, tasks.parent_id FROM tasks \
             JOIN projects ON projects.id = tasks.project_id WHERE tasks.id = ? AND tasks.workspace_id = ? \
             AND tasks.deleted_at IS NULL AND projects.deleted_at IS NULL",
        )
        .bind(task_id.to_string())
        .bind(workspace_id.to_string())
        .fetch_optional(self.database.pool())
        .await?
        .ok_or(TaskError::NotFound)?;
        let mut record = task_from_row(self.database.pool(), row).await?;
        let mut connection = self.database.pool().acquire().await?;
        apply_task_graph(
            &mut connection,
            workspace_id,
            std::slice::from_mut(&mut record),
        )
        .await?;
        // Detail-only: the canonical side of the duplicate relation.
        record.duplicate_ids = sqlx::query_scalar::<_, String>(
            "SELECT task_relations.source_task_id FROM task_relations \
             JOIN tasks ON tasks.id = task_relations.source_task_id \
             WHERE task_relations.workspace_id = ? AND task_relations.target_task_id = ? \
             AND task_relations.kind = 'duplicate_of' AND tasks.deleted_at IS NULL \
             ORDER BY task_relations.created_at, task_relations.source_task_id",
        )
        .bind(workspace_id.to_string())
        .bind(task_id.to_string())
        .fetch_all(&mut *connection)
        .await?
        .into_iter()
        .map(parse_id)
        .collect::<Result<_, _>>()?;
        // Detail-only backlinks: one capped query. A source whose task is in the
        // trash is hidden, and a comment on this task mentioning this task is
        // not a backlink.
        record.referenced_by = sqlx::query_as::<_, (String, String, String, String, i64, String)>(
            "SELECT task_references.source_type, task_references.source_id, source.id, \
             source.identifier_key, source.number, source.title \
             FROM task_references \
             LEFT JOIN task_comments ON task_references.source_type = 'comment' \
             AND task_comments.id = task_references.source_id \
             JOIN tasks AS source ON source.id = COALESCE(task_comments.task_id, task_references.source_id) \
             WHERE task_references.workspace_id = ? AND task_references.target_task_id = ? \
             AND source.deleted_at IS NULL AND source.id <> task_references.target_task_id \
             ORDER BY task_references.source_type DESC, source.identifier_key, source.number, task_references.source_id \
             LIMIT 50",
        )
        .bind(workspace_id.to_string())
        .bind(task_id.to_string())
        .fetch_all(&mut *connection)
        .await?
        .into_iter()
        .map(|(source_type, source_id, source_task_id, key, number, title)| {
            Ok(TaskReferenceRecord {
                source_type,
                source_id: parse_id(source_id)?,
                source_task_id: parse_id(source_task_id)?,
                source_task_identifier: format!("{key}-{number}"),
                source_task_title: title,
            })
        })
        .collect::<Result<Vec<_>, TaskError>>()?;
        Ok(record)
    }

    /// Batch identifier lookup for rich-text task chips: one request, never N+1. Each entry is an
    /// index seek on `tasks_identifier`; unknown identifiers are omitted so a document that
    /// references a deleted task still renders.
    pub async fn resolve_tasks(
        &self,
        workspace_id: Id,
        actor_id: Id,
        identifiers: &[(String, i64)],
    ) -> Result<Vec<TaskRecord>, TaskError> {
        require_access(self.database.pool(), workspace_id, actor_id).await?;
        let mut records = Vec::with_capacity(identifiers.len());
        for (identifier_key, number) in identifiers {
            let row = sqlx::query(
                "SELECT tasks.id, tasks.workspace_id, tasks.project_id, tasks.status_id, \
                 tasks.identifier_key, tasks.number, tasks.title, \
                 tasks.description_json, tasks.description_text, tasks.priority, tasks.position, tasks.creator_id, tasks.creator_service_account_id, (SELECT name FROM service_accounts WHERE id = tasks.creator_service_account_id) AS creator_service_account_name, tasks.due_at, tasks.version, \
                 tasks.deleted_at, tasks.created_at, tasks.updated_at, tasks.parent_id FROM tasks \
                 JOIN projects ON projects.id = tasks.project_id \
                 WHERE tasks.workspace_id = ? AND tasks.identifier_key = ? AND tasks.number = ? \
                 AND tasks.deleted_at IS NULL AND projects.deleted_at IS NULL",
            )
            .bind(workspace_id.to_string())
            .bind(identifier_key)
            .bind(number)
            .fetch_optional(self.database.pool())
            .await?;
            if let Some(row) = row {
                records.push(task_from_row(self.database.pool(), row).await?);
            }
        }
        apply_task_graph(
            &mut *self.database.pool().acquire().await?,
            workspace_id,
            &mut records,
        )
        .await?;
        Ok(records)
    }

    pub async fn create_task(
        &self,
        workspace_id: Id,
        actor_id: Id,
        input: CreateTask,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<TaskRecord, TaskError> {
        let id = Id::new_v7();
        let mut tx = self.database.immediate_transaction().await?;
        require_access_tx(&mut tx, workspace_id, actor_id).await?;
        validate_project_status(&mut tx, workspace_id, input.project_id, input.status_id).await?;
        validate_assignees(&mut tx, workspace_id, &input.assignee_ids).await?;
        validate_labels(&mut tx, workspace_id, &input.label_ids).await?;
        if let Some(parent_id) = input.parent_id {
            resolve_parent_tx(&mut tx, workspace_id, id, parent_id).await?;
        }
        // One statement inside the enclosing BEGIN IMMEDIATE, so the counter cannot race. The
        // WHERE clause matches at most the single primary-key row; None means the project is
        // missing or soft-deleted.
        let (identifier_key, number) =
            allocate_identifier(&mut tx, workspace_id, input.project_id).await?;
        let (description_json, description_text) =
            document_columns(&input.description_json, "description_json")?;
        let position = match input.position {
            Some(position) => position,
            None => sqlx::query_scalar::<_, i64>(
                "SELECT COALESCE(MAX(position) + 1, 0) FROM tasks WHERE workspace_id = ? AND project_id = ? AND status_id = ? AND deleted_at IS NULL",
            )
            .bind(workspace_id.to_string())
            .bind(input.project_id.to_string())
            .bind(input.status_id.to_string())
            .fetch_one(&mut *tx)
            .await?,
        };
        sqlx::query(
            "INSERT INTO tasks (id, workspace_id, project_id, status_id, identifier_key, number, title, description_json, description_text, priority, position, creator_id, due_at, parent_id, version, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, ?, ?)",
        )
        .bind(id.to_string())
        .bind(workspace_id.to_string())
        .bind(input.project_id.to_string())
        .bind(input.status_id.to_string())
        .bind(&identifier_key)
        .bind(number)
        .bind(&input.title)
        .bind(&description_json)
        .bind(&description_text)
        .bind(&input.priority)
        .bind(position)
        .bind(actor_id.to_string())
        .bind(input.due_at.map(TimestampMillis::as_millis))
        .bind(input.parent_id.map(|id| id.to_string()))
        .bind(now.as_millis())
        .bind(now.as_millis())
        .execute(&mut *tx)
        .await?;
        replace_assignees(&mut tx, id, &input.assignee_ids).await?;
        replace_labels(&mut tx, id, &input.label_ids).await?;
        rebuild_task_references(&mut tx, workspace_id, "task", id, &input.description_json).await?;
        sync_task_search(
            &mut tx,
            workspace_id,
            id,
            &format!("{identifier_key}-{number}"),
            &input.title,
            &description_text,
        )
        .await?;
        notify_users(
            &mut tx,
            workspace_id,
            actor_id,
            NotificationRequest {
                kind: "task_assigned",
                skip_unknown: false,
                task_id: id,
                comment_id: None,
                recipients: &input.assignee_ids,
            },
            now,
        )
        .await?;
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "task.created",
            "task",
            id,
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(TaskRecord {
            id,
            workspace_id,
            project_id: input.project_id,
            status_id: input.status_id,
            identifier: format!("{identifier_key}-{number}"),
            identifier_key,
            number,
            title: input.title,
            description_json: input.description_json,
            description_text,
            priority: input.priority,
            position,
            creator_id: Some(actor_id),
            creator_service_account_id: None,
            creator_service_account_name: None,
            parent_id: input.parent_id,
            sub_issue_total: 0,
            sub_issue_done: 0,
            duplicate_of_task_id: None,
            duplicate_ids: Vec::new(),
            referenced_by: Vec::new(),
            assignee_ids: input.assignee_ids,
            label_ids: input.label_ids,
            due_at: input.due_at,
            version: 0,
            deleted_at: None,
            created_at: now,
            updated_at: now,
        })
    }

    pub async fn create_discord_task(
        &self,
        workspace_id: Id,
        actor_id: Id,
        service_account_id: Option<Id>,
        input: DiscordTask,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(TaskRecord, bool), TaskError> {
        let mut tx = self.database.immediate_transaction().await?;
        let service_account_name = if let Some(service_account_id) = service_account_id {
            let name = sqlx::query_scalar::<_, String>(
                "SELECT name FROM service_accounts WHERE id = ? AND workspace_id = ? AND disabled_at IS NULL",
            )
            .bind(service_account_id.to_string())
            .bind(workspace_id.to_string())
            .fetch_optional(&mut *tx)
            .await?;
            Some(name.ok_or(TaskError::NotFound)?)
        } else {
            require_access_tx(&mut tx, workspace_id, actor_id).await?;
            None
        };

        if let Some(row) = sqlx::query(
            "SELECT integration_events.task_id, integration_events.payload_hash, tasks.deleted_at \
             FROM integration_events \
             JOIN tasks ON tasks.id = integration_events.task_id \
             WHERE integration_events.workspace_id = ? AND integration_events.provider = 'discord' AND integration_events.external_event_id = ?",
        )
        .bind(workspace_id.to_string())
        .bind(&input.event_id)
        .fetch_optional(&mut *tx)
        .await?
        {
            let task_id = parse_id(row.get("task_id"))?;
            if row.get::<Option<i64>, _>("deleted_at").is_some() {
                sqlx::query(
                    "DELETE FROM integration_events WHERE workspace_id = ? AND provider = 'discord' AND external_event_id = ?",
                )
                .bind(workspace_id.to_string())
                .bind(&input.event_id)
                .execute(&mut *tx)
                .await?;
            } else {
                if row.get::<Vec<u8>, _>("payload_hash") != input.payload_hash {
                    return Err(TaskError::IntegrationConflict);
                }
                let task = task_in_tx(&mut tx, workspace_id, task_id, false).await?;
                tx.commit().await?;
                return Ok((task, false));
            }
        }

        project_in_tx(&mut tx, workspace_id, input.project_id, false).await?;
        let status_id = sqlx::query_scalar::<_, String>(
            "SELECT id FROM task_statuses WHERE workspace_id = ? AND project_id = ? ORDER BY CASE category WHEN 'unstarted' THEN 0 ELSE 1 END, position, id LIMIT 1",
        )
        .bind(workspace_id.to_string())
        .bind(input.project_id.to_string())
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(TaskError::NotFound)
        .and_then(parse_id)?;

        let existing_label = sqlx::query_scalar::<_, String>(
            "SELECT id FROM labels WHERE workspace_id = ? AND lower(name) = 'discord' ORDER BY id LIMIT 1",
        )
        .bind(workspace_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        let label_id = if let Some(id) = existing_label {
            parse_id(id)?
        } else {
            let id = Id::new_v7();
            sqlx::query("INSERT INTO labels (id, workspace_id, name, color, version, created_at, updated_at) VALUES (?, ?, 'Discord', '#5865F2', 0, ?, ?)")
                .bind(id.to_string()).bind(workspace_id.to_string()).bind(now.as_millis()).bind(now.as_millis()).execute(&mut *tx).await?;
            record_principal_mutation(
                &mut tx,
                workspace_id,
                actor_id,
                service_account_id.zip(service_account_name.as_deref()),
                "label.created",
                "label",
                id,
                request_id,
                now,
            )
            .await?;
            id
        };

        let task_id = Id::new_v7();
        let position = sqlx::query_scalar::<_, i64>(
            "SELECT COALESCE(MAX(position) + 1, 0) FROM tasks WHERE workspace_id = ? AND project_id = ? AND status_id = ? AND deleted_at IS NULL",
        )
        .bind(workspace_id.to_string()).bind(input.project_id.to_string()).bind(status_id.to_string())
        .fetch_one(&mut *tx).await?;
        // Discord message text is Markdown, so it goes through the same converter the
        // 0020 backfill uses; the stored columns are derived exactly as on any write.
        let (identifier_key, number) =
            allocate_identifier(&mut tx, workspace_id, input.project_id).await?;
        let description = rich_text::markdown_to_document(&input.description);
        let (description_json, description_text) = document_columns(&description, "description")?;
        sqlx::query(
            "INSERT INTO tasks (id, workspace_id, project_id, status_id, identifier_key, number, title, description_json, description_text, priority, position, creator_id, creator_service_account_id, version, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'none', ?, ?, ?, 0, ?, ?)",
        )
        .bind(task_id.to_string()).bind(workspace_id.to_string()).bind(input.project_id.to_string()).bind(status_id.to_string())
        .bind(&identifier_key).bind(number)
        .bind(&input.title).bind(&description_json).bind(&description_text).bind(position).bind(actor_id.to_string()).bind(service_account_id.map(|id| id.to_string())).bind(now.as_millis()).bind(now.as_millis())
        .execute(&mut *tx).await?;
        rebuild_task_references(&mut tx, workspace_id, "task", task_id, &description).await?;
        sync_task_search(
            &mut tx,
            workspace_id,
            task_id,
            &format!("{identifier_key}-{number}"),
            &input.title,
            &description_text,
        )
        .await?;
        sqlx::query("INSERT INTO task_labels (task_id, label_id) VALUES (?, ?)")
            .bind(task_id.to_string())
            .bind(label_id.to_string())
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO integration_events (workspace_id, provider, external_event_id, payload_hash, task_id, created_at) VALUES (?, 'discord', ?, ?, ?, ?)")
            .bind(workspace_id.to_string()).bind(&input.event_id).bind(input.payload_hash.to_vec()).bind(task_id.to_string()).bind(now.as_millis())
            .execute(&mut *tx).await?;
        record_principal_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            service_account_id.zip(service_account_name.as_deref()),
            "task.created",
            "task",
            task_id,
            request_id,
            now,
        )
        .await?;
        let task = task_in_tx(&mut tx, workspace_id, task_id, false).await?;
        tx.commit().await?;
        Ok((task, true))
    }

    pub async fn update_task(
        &self,
        workspace_id: Id,
        actor_id: Id,
        update: &TaskUpdate,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<TaskRecord, TaskError> {
        let mut tx = self.database.immediate_transaction().await?;
        require_access_tx(&mut tx, workspace_id, actor_id).await?;
        let mut task = update_task_in_tx(&mut tx, workspace_id, actor_id, update, now).await?;
        apply_task_graph(&mut tx, workspace_id, std::slice::from_mut(&mut task)).await?;
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "task.updated",
            "task",
            update.id,
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(task)
    }

    pub async fn bulk_update_tasks(
        &self,
        workspace_id: Id,
        actor_id: Id,
        updates: &[TaskUpdate],
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<Vec<TaskRecord>, TaskError> {
        self.update_many_tasks(
            workspace_id,
            actor_id,
            updates,
            "task.bulk_updated",
            request_id,
            now,
        )
        .await
    }

    async fn update_many_tasks(
        &self,
        workspace_id: Id,
        actor_id: Id,
        updates: &[TaskUpdate],
        action: &str,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<Vec<TaskRecord>, TaskError> {
        let mut tx = self.database.immediate_transaction().await?;
        require_access_tx(&mut tx, workspace_id, actor_id).await?;
        let mut records = Vec::with_capacity(updates.len());
        for update in updates {
            records.push(update_task_in_tx(&mut tx, workspace_id, actor_id, update, now).await?);
        }
        apply_task_graph(&mut tx, workspace_id, &mut records).await?;
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            action,
            "workspace",
            workspace_id,
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(records)
    }

    pub async fn reorder_tasks(
        &self,
        workspace_id: Id,
        actor_id: Id,
        items: &[(Id, u64, i64)],
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<Vec<TaskRecord>, TaskError> {
        let updates = items
            .iter()
            .map(|&(id, expected_version, position)| TaskUpdate {
                id,
                expected_version,
                changes: TaskChanges {
                    position: Some(position),
                    ..TaskChanges::default()
                },
            })
            .collect::<Vec<_>>();
        self.update_many_tasks(
            workspace_id,
            actor_id,
            &updates,
            "task.reordered",
            request_id,
            now,
        )
        .await
    }

    pub async fn delete_task(
        &self,
        workspace_id: Id,
        task_id: Id,
        actor_id: Id,
        expected_version: u64,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), TaskError> {
        let mut tx = self.database.immediate_transaction().await?;
        require_access_tx(&mut tx, workspace_id, actor_id).await?;
        let current = task_in_tx(&mut tx, workspace_id, task_id, false).await?;
        check_version(expected_version, current.version, &current)?;
        sqlx::query("UPDATE tasks SET deleted_at = ?, version = version + 1, updated_at = ? WHERE id = ? AND workspace_id = ? AND deleted_at IS NULL AND version = ?")
            .bind(now.as_millis()).bind(now.as_millis()).bind(task_id.to_string()).bind(workspace_id.to_string()).bind(expected_version as i64).execute(&mut *tx).await?;
        // A trashed task must leave the index, or search would surface it.
        remove_task_search(&mut tx, task_id).await?;
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "task.deleted",
            "task",
            task_id,
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn restore_task(
        &self,
        workspace_id: Id,
        task_id: Id,
        actor_id: Id,
        expected_version: u64,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<TaskRecord, TaskError> {
        let mut tx = self.database.immediate_transaction().await?;
        require_access_tx(&mut tx, workspace_id, actor_id).await?;
        let current = task_in_tx(&mut tx, workspace_id, task_id, true).await?;
        if current.deleted_at.is_none_or(|deleted| {
            deleted.as_millis() <= now.as_millis().saturating_sub(TRASH_RETENTION_MILLIS)
        }) {
            return Err(TaskError::NotFound);
        }
        check_version(expected_version, current.version, &current)?;
        validate_project_status(&mut tx, workspace_id, current.project_id, current.status_id)
            .await?;
        sqlx::query("UPDATE tasks SET deleted_at = NULL, version = version + 1, updated_at = ? WHERE id = ? AND workspace_id = ? AND deleted_at IS NOT NULL AND version = ?")
            .bind(now.as_millis()).bind(task_id.to_string()).bind(workspace_id.to_string()).bind(expected_version as i64).execute(&mut *tx).await?;
        sync_task_search(
            &mut tx,
            workspace_id,
            task_id,
            &current.identifier,
            &current.title,
            &current.description_text,
        )
        .await?;
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "task.restored",
            "task",
            task_id,
            request_id,
            now,
        )
        .await?;
        let mut record = TaskRecord {
            version: current.version + 1,
            deleted_at: None,
            updated_at: now,
            ..current
        };
        apply_task_graph(&mut tx, workspace_id, std::slice::from_mut(&mut record)).await?;
        tx.commit().await?;
        Ok(record)
    }

    /// Linear-style "mark as duplicate": a relation row plus a move to the
    /// project's Cancelled-category status. NOTHING else moves, so it is fully
    /// reversible with `unmark_duplicate`.
    #[allow(clippy::too_many_arguments)]
    pub async fn mark_duplicate(
        &self,
        workspace_id: Id,
        task_id: Id,
        target_task_id: Id,
        actor_id: Id,
        expected_version: u64,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<TaskRecord, TaskError> {
        if task_id == target_task_id {
            return Err(TaskError::Invalid {
                field: "target_task_id",
            });
        }
        let mut tx = self.database.immediate_transaction().await?;
        require_access_tx(&mut tx, workspace_id, actor_id).await?;
        let current = task_in_tx(&mut tx, workspace_id, task_id, false).await?;
        check_version(expected_version, current.version, &current)?;
        let target = match task_in_tx(&mut tx, workspace_id, target_task_id, false).await {
            Err(TaskError::NotFound) => {
                return Err(TaskError::Invalid {
                    field: "target_task_id",
                });
            }
            other => other?,
        };
        // Two tasks marked as duplicates of each other would leave no canonical one.
        let target_is_duplicate_of_source: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM task_relations WHERE workspace_id = ? AND kind = 'duplicate_of' \
             AND source_task_id = ? AND target_task_id = ?",
        )
        .bind(workspace_id.to_string())
        .bind(target.id.to_string())
        .bind(task_id.to_string())
        .fetch_one(&mut *tx)
        .await?;
        if target_is_duplicate_of_source > 0 {
            return Err(TaskError::Invalid {
                field: "target_task_id",
            });
        }
        let current_category: String =
            sqlx::query_scalar("SELECT category FROM task_statuses WHERE id = ?")
                .bind(current.status_id.to_string())
                .fetch_one(&mut *tx)
                .await?;
        // Already cancelled stays where it is; otherwise the project's first
        // Cancelled-category status.
        let status_id = if current_category == "cancelled" {
            current.status_id
        } else {
            let cancelled: Option<String> = sqlx::query_scalar(
                "SELECT id FROM task_statuses WHERE workspace_id = ? AND project_id = ? \
                 AND category = 'cancelled' ORDER BY position, id LIMIT 1",
            )
            .bind(workspace_id.to_string())
            .bind(current.project_id.to_string())
            .fetch_optional(&mut *tx)
            .await?;
            let Some(cancelled) = cancelled else {
                return Err(TaskError::Invalid { field: "status_id" });
            };
            parse_id(cancelled)?
        };
        let inserted = sqlx::query(
            "INSERT INTO task_relations (id, workspace_id, kind, source_task_id, target_task_id, created_by, created_at) \
             VALUES (?, ?, 'duplicate_of', ?, ?, ?, ?)",
        )
        .bind(Id::new_v7().to_string())
        .bind(workspace_id.to_string())
        .bind(task_id.to_string())
        .bind(target_task_id.to_string())
        .bind(actor_id.to_string())
        .bind(now.as_millis())
        .execute(&mut *tx)
        .await;
        // task_relations_one_duplicate: a task is a duplicate of at most one task.
        if is_unique_violation(&inserted) {
            return Err(TaskError::Conflict);
        }
        inserted?;
        sqlx::query(
            "UPDATE tasks SET status_id = ?, version = version + 1, updated_at = ? \
             WHERE id = ? AND workspace_id = ? AND deleted_at IS NULL AND version = ?",
        )
        .bind(status_id.to_string())
        .bind(now.as_millis())
        .bind(task_id.to_string())
        .bind(workspace_id.to_string())
        .bind(expected_version as i64)
        .execute(&mut *tx)
        .await?;
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "task.duplicate_marked",
            "task",
            task_id,
            request_id,
            now,
        )
        .await?;
        let mut record = TaskRecord {
            status_id,
            version: current.version + 1,
            updated_at: now,
            ..current
        };
        apply_task_graph(&mut tx, workspace_id, std::slice::from_mut(&mut record)).await?;
        tx.commit().await?;
        Ok(record)
    }

    /// Removes the duplicate relation. Deliberately does NOT restore the
    /// previous status: the user picks one if they want it back.
    pub async fn unmark_duplicate(
        &self,
        workspace_id: Id,
        task_id: Id,
        actor_id: Id,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), TaskError> {
        let mut tx = self.database.immediate_transaction().await?;
        require_task_tx(&mut tx, workspace_id, task_id, actor_id).await?;
        let removed = sqlx::query(
            "DELETE FROM task_relations WHERE workspace_id = ? AND source_task_id = ? AND kind = 'duplicate_of'",
        )
        .bind(workspace_id.to_string())
        .bind(task_id.to_string())
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if removed == 0 {
            return Err(TaskError::NotFound);
        }
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "task.duplicate_unmarked",
            "task",
            task_id,
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn task_trash(
        &self,
        workspace_id: Id,
        actor_id: Id,
        cursor: Option<&str>,
        limit: usize,
        now: TimestampMillis,
    ) -> Result<Page<TaskRecord>, TaskError> {
        require_access(self.database.pool(), workspace_id, actor_id).await?;
        let fingerprint = format!("task-trash:{workspace_id}");
        let after = cursor_i64_pair(cursor, &fingerprint)?;
        let mut query = QueryBuilder::<Sqlite>::new(
            "SELECT tasks.id, tasks.workspace_id, tasks.project_id, tasks.status_id, tasks.identifier_key, tasks.number, tasks.title, tasks.description_json, tasks.description_text, tasks.priority, tasks.position, tasks.creator_id, tasks.creator_service_account_id, (SELECT name FROM service_accounts WHERE id = tasks.creator_service_account_id) AS creator_service_account_name, tasks.due_at, tasks.version, tasks.deleted_at, tasks.created_at, tasks.updated_at, tasks.parent_id \
             FROM tasks JOIN projects ON projects.id = tasks.project_id WHERE tasks.workspace_id = ",
        );
        query
            .push_bind(workspace_id.to_string())
            .push(" AND tasks.deleted_at > ")
            .push_bind(now.as_millis().saturating_sub(TRASH_RETENTION_MILLIS))
            .push(" AND projects.deleted_at IS NULL");
        if let Some((deleted_at, id)) = after {
            query
                .push(" AND (tasks.deleted_at < ")
                .push_bind(deleted_at)
                .push(" OR (tasks.deleted_at = ")
                .push_bind(deleted_at)
                .push(" AND tasks.id < ")
                .push_bind(id.to_string())
                .push("))");
        }
        let limit = limit.clamp(1, 100);
        query
            .push(" ORDER BY tasks.deleted_at DESC, tasks.id DESC LIMIT ")
            .push_bind((limit + 1) as i64);
        let rows = query.build().fetch_all(self.database.pool()).await?;
        let mut items = Vec::with_capacity(rows.len());
        for row in rows {
            items.push(task_from_row(self.database.pool(), row).await?);
        }
        finish_page(items, limit, &fingerprint, |task| {
            vec![
                task.deleted_at
                    .map_or(0, TimestampMillis::as_millis)
                    .to_string(),
                task.id.to_string(),
            ]
        })
    }

    pub async fn comments(
        &self,
        workspace_id: Id,
        task_id: Id,
        actor_id: Id,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<Page<CommentRecord>, TaskError> {
        self.get_task(workspace_id, task_id, actor_id).await?;
        let fingerprint = format!("comments:{task_id}");
        let after = cursor_i64_pair(cursor, &fingerprint)?;
        let mut query = QueryBuilder::<Sqlite>::new(
            "SELECT id, workspace_id, task_id, author_id, parent_id, body_json, body_text, version, created_at, updated_at \
             FROM task_comments WHERE workspace_id = ",
        );
        query
            .push_bind(workspace_id.to_string())
            .push(" AND task_id = ")
            .push_bind(task_id.to_string());
        if let Some((created_at, id)) = after {
            query
                .push(" AND (created_at > ")
                .push_bind(created_at)
                .push(" OR (created_at = ")
                .push_bind(created_at)
                .push(" AND id > ")
                .push_bind(id.to_string())
                .push("))");
        }
        let limit = limit.clamp(1, 100);
        query
            .push(" ORDER BY created_at, id LIMIT ")
            .push_bind((limit + 1) as i64);
        let items = query
            .build()
            .fetch_all(self.database.pool())
            .await?
            .into_iter()
            .map(comment_from_row)
            .collect::<Result<Vec<_>, _>>()?;
        finish_page(items, limit, &fingerprint, |comment| {
            vec![
                comment.created_at.as_millis().to_string(),
                comment.id.to_string(),
            ]
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn create_comment(
        &self,
        workspace_id: Id,
        task_id: Id,
        actor_id: Id,
        parent_id: Option<Id>,
        body_json: Value,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<CommentRecord, TaskError> {
        let id = Id::new_v7();
        let mut tx = self.database.immediate_transaction().await?;
        require_task_tx(&mut tx, workspace_id, task_id, actor_id).await?;
        if let Some(parent_id) = parent_id {
            let exists: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM task_comments WHERE id = ? AND workspace_id = ? AND task_id = ?").bind(parent_id.to_string()).bind(workspace_id.to_string()).bind(task_id.to_string()).fetch_one(&mut *tx).await?;
            if exists != 1 {
                return Err(TaskError::NotFound);
            }
        }
        if rich_text::is_empty(&body_json) {
            return Err(TaskError::Invalid { field: "body_json" });
        }
        let (body_column, body_text) = document_columns(&body_json, "body_json")?;
        // Derived from the stored document, so the highlighted text and the
        // notified people can never disagree.
        let recipients = rich_text::extract_user_ids(&body_json);
        sqlx::query("INSERT INTO task_comments (id, workspace_id, task_id, author_id, parent_id, body_json, body_text, version, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, 0, ?, ?)")
            .bind(id.to_string()).bind(workspace_id.to_string()).bind(task_id.to_string()).bind(actor_id.to_string()).bind(parent_id.map(|id| id.to_string())).bind(&body_column).bind(&body_text).bind(now.as_millis()).bind(now.as_millis()).execute(&mut *tx).await?;
        rebuild_task_references(&mut tx, workspace_id, "comment", id, &body_json).await?;
        notify_users(
            &mut tx,
            workspace_id,
            actor_id,
            NotificationRequest {
                kind: "comment_mentioned",
                task_id,
                comment_id: Some(id),
                recipients: &recipients,
                skip_unknown: true,
            },
            now,
        )
        .await?;
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "comment.created",
            "comment",
            id,
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(CommentRecord {
            id,
            workspace_id,
            task_id,
            author_id: actor_id,
            parent_id,
            body_json,
            body_text,
            version: 0,
            created_at: now,
            updated_at: now,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update_comment(
        &self,
        workspace_id: Id,
        task_id: Id,
        comment_id: Id,
        actor_id: Id,
        body_json: Value,
        expected_version: u64,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<CommentRecord, TaskError> {
        let mut tx = self.database.immediate_transaction().await?;
        require_task_tx(&mut tx, workspace_id, task_id, actor_id).await?;
        let current = comment_in_tx(&mut tx, workspace_id, task_id, comment_id).await?;
        check_version(expected_version, current.version, &current)?;
        if rich_text::is_empty(&body_json) {
            return Err(TaskError::Invalid { field: "body_json" });
        }
        let (body_column, body_text) = document_columns(&body_json, "body_json")?;
        sqlx::query("UPDATE task_comments SET body_json = ?, body_text = ?, version = version + 1, updated_at = ? WHERE id = ? AND workspace_id = ? AND task_id = ? AND version = ?")
            .bind(&body_column).bind(&body_text).bind(now.as_millis()).bind(comment_id.to_string()).bind(workspace_id.to_string()).bind(task_id.to_string()).bind(expected_version as i64).execute(&mut *tx).await?;
        rebuild_task_references(&mut tx, workspace_id, "comment", comment_id, &body_json).await?;
        // Editing in a new mention notifies that person. dedupe_key is
        // workspace:recipient:kind:comment, inserted with OR IGNORE, so anyone
        // already notified for this comment is not notified twice.
        let recipients = rich_text::extract_user_ids(&body_json);
        notify_users(
            &mut tx,
            workspace_id,
            actor_id,
            NotificationRequest {
                kind: "comment_mentioned",
                task_id,
                comment_id: Some(comment_id),
                recipients: &recipients,
                skip_unknown: true,
            },
            now,
        )
        .await?;
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "comment.updated",
            "comment",
            comment_id,
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(CommentRecord {
            body_json,
            body_text,
            version: current.version + 1,
            updated_at: now,
            ..current
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn delete_comment(
        &self,
        workspace_id: Id,
        task_id: Id,
        comment_id: Id,
        actor_id: Id,
        expected_version: u64,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), TaskError> {
        let mut tx = self.database.immediate_transaction().await?;
        require_task_tx(&mut tx, workspace_id, task_id, actor_id).await?;
        let current = comment_in_tx(&mut tx, workspace_id, task_id, comment_id).await?;
        check_version(expected_version, current.version, &current)?;
        sqlx::query("DELETE FROM task_comments WHERE id = ? AND workspace_id = ? AND task_id = ? AND version = ?").bind(comment_id.to_string()).bind(workspace_id.to_string()).bind(task_id.to_string()).bind(expected_version as i64).execute(&mut *tx).await?;
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "comment.deleted",
            "comment",
            comment_id,
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn notifications(
        &self,
        workspace_id: Id,
        actor_id: Id,
        unread_only: bool,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<Page<NotificationRecord>, TaskError> {
        require_access(self.database.pool(), workspace_id, actor_id).await?;
        let fingerprint = format!("notifications:{actor_id}:{}", unread_only);
        let after = cursor_i64_pair(cursor, &fingerprint)?;
        let mut query = QueryBuilder::<Sqlite>::new(
            "SELECT id, workspace_id, recipient_user_id, actor_user_id, kind, task_id, comment_id, read_at, created_at \
             FROM notifications WHERE workspace_id = ",
        );
        query
            .push_bind(workspace_id.to_string())
            .push(" AND recipient_user_id = ")
            .push_bind(actor_id.to_string());
        if unread_only {
            query.push(" AND read_at IS NULL");
        }
        if let Some((created_at, id)) = after {
            query
                .push(" AND (created_at < ")
                .push_bind(created_at)
                .push(" OR (created_at = ")
                .push_bind(created_at)
                .push(" AND id < ")
                .push_bind(id.to_string())
                .push("))");
        }
        let limit = limit.clamp(1, 100);
        query
            .push(" ORDER BY created_at DESC, id DESC LIMIT ")
            .push_bind((limit + 1) as i64);
        let items = query
            .build()
            .fetch_all(self.database.pool())
            .await?
            .into_iter()
            .map(notification_from_row)
            .collect::<Result<Vec<_>, _>>()?;
        finish_page(items, limit, &fingerprint, |notification| {
            vec![
                notification.created_at.as_millis().to_string(),
                notification.id.to_string(),
            ]
        })
    }

    pub async fn mark_notification_read(
        &self,
        workspace_id: Id,
        actor_id: Id,
        notification_id: Id,
        now: TimestampMillis,
    ) -> Result<NotificationRecord, TaskError> {
        require_access(self.database.pool(), workspace_id, actor_id).await?;
        let mut tx = self.database.immediate_transaction().await?;
        sqlx::query(
            "UPDATE notifications SET read_at = COALESCE(read_at, ?) \
             WHERE id = ? AND workspace_id = ? AND recipient_user_id = ?",
        )
        .bind(now.as_millis())
        .bind(notification_id.to_string())
        .bind(workspace_id.to_string())
        .bind(actor_id.to_string())
        .execute(&mut *tx)
        .await?;
        let row = sqlx::query(
            "SELECT id, workspace_id, recipient_user_id, actor_user_id, kind, task_id, comment_id, read_at, created_at \
             FROM notifications WHERE id = ? AND workspace_id = ? AND recipient_user_id = ?",
        )
        .bind(notification_id.to_string())
        .bind(workspace_id.to_string())
        .bind(actor_id.to_string())
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(TaskError::NotFound)?;
        tx.commit().await?;
        notification_from_row(row)
    }

    pub async fn mark_notifications_read(
        &self,
        workspace_id: Id,
        actor_id: Id,
        now: TimestampMillis,
    ) -> Result<u64, TaskError> {
        require_access(self.database.pool(), workspace_id, actor_id).await?;
        let changed = sqlx::query(
            "UPDATE notifications SET read_at = ? WHERE workspace_id = ? AND recipient_user_id = ? AND read_at IS NULL",
        )
        .bind(now.as_millis())
        .bind(workspace_id.to_string())
        .bind(actor_id.to_string())
        .execute(self.database.pool())
        .await?
        .rows_affected();
        Ok(changed)
    }
}

async fn update_task_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    actor_id: Id,
    update: &TaskUpdate,
    now: TimestampMillis,
) -> Result<TaskRecord, TaskError> {
    let current = task_in_tx(tx, workspace_id, update.id, false).await?;
    check_version(update.expected_version, current.version, &current)?;
    let project_id = update.changes.project_id.unwrap_or(current.project_id);
    let status_id = update.changes.status_id.unwrap_or(current.status_id);
    validate_project_status(tx, workspace_id, project_id, status_id).await?;
    if let Some(assignees) = &update.changes.assignee_ids {
        validate_assignees(tx, workspace_id, assignees).await?;
    }
    if let Some(labels) = &update.changes.label_ids {
        validate_labels(tx, workspace_id, labels).await?;
    }
    let parent_id = update.changes.parent_id.unwrap_or(current.parent_id);
    if let Some(parent_id) = parent_id
        && Some(parent_id) != current.parent_id
    {
        resolve_parent_tx(tx, workspace_id, update.id, parent_id).await?;
    }
    let title = update
        .changes
        .title
        .clone()
        .unwrap_or(current.title.clone());
    let description = update
        .changes
        .description_json
        .clone()
        .unwrap_or_else(|| current.description_json.clone());
    let (description_json, description_text) = document_columns(&description, "description_json")?;
    let priority = update
        .changes
        .priority
        .clone()
        .unwrap_or(current.priority.clone());
    let position = update.changes.position.unwrap_or(current.position);
    let due_at = update.changes.due_at.unwrap_or(current.due_at);
    sqlx::query("UPDATE tasks SET project_id = ?, status_id = ?, title = ?, description_json = ?, description_text = ?, priority = ?, position = ?, due_at = ?, parent_id = ?, version = version + 1, updated_at = ? WHERE id = ? AND workspace_id = ? AND deleted_at IS NULL AND version = ?")
        .bind(project_id.to_string()).bind(status_id.to_string()).bind(&title).bind(&description_json).bind(&description_text).bind(&priority).bind(position).bind(due_at.map(TimestampMillis::as_millis)).bind(parent_id.map(|id| id.to_string())).bind(now.as_millis()).bind(update.id.to_string()).bind(workspace_id.to_string()).bind(update.expected_version as i64).execute(&mut **tx).await?;
    if let Some(assignees) = &update.changes.assignee_ids {
        let added: Vec<Id> = assignees
            .iter()
            .copied()
            .filter(|id| !current.assignee_ids.contains(id))
            .collect();
        replace_assignees(tx, update.id, assignees).await?;
        notify_users(
            tx,
            workspace_id,
            actor_id,
            NotificationRequest {
                kind: "task_assigned",
                skip_unknown: false,
                task_id: update.id,
                comment_id: None,
                recipients: &added,
            },
            now,
        )
        .await?;
    }
    if let Some(labels) = &update.changes.label_ids {
        replace_labels(tx, update.id, labels).await?;
    }
    if update.changes.description_json.is_some() {
        rebuild_task_references(tx, workspace_id, "task", update.id, &description).await?;
    }
    // Unconditional: a title- or description-only edit must reindex too.
    sync_task_search(
        tx,
        workspace_id,
        update.id,
        &current.identifier,
        &title,
        &description_text,
    )
    .await?;
    Ok(TaskRecord {
        project_id,
        status_id,
        title,
        description_json: description,
        description_text,
        priority,
        position,
        due_at,
        parent_id,
        assignee_ids: update
            .changes
            .assignee_ids
            .clone()
            .unwrap_or(current.assignee_ids.clone()),
        label_ids: update
            .changes
            .label_ids
            .clone()
            .unwrap_or(current.label_ids.clone()),
        version: current.version + 1,
        updated_at: now,
        ..current
    })
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    version: u8,
    fingerprint: String,
    key: Vec<String>,
}

fn finish_page<T>(
    mut items: Vec<T>,
    limit: usize,
    fingerprint: &str,
    key: impl Fn(&T) -> Vec<String>,
) -> Result<Page<T>, TaskError> {
    let has_more = items.len() > limit;
    items.truncate(limit);
    let next_cursor = if has_more {
        items
            .last()
            .map(|last| encode_cursor(fingerprint, key(last)))
            .transpose()?
    } else {
        None
    };
    Ok(Page { items, next_cursor })
}

fn encode_cursor(fingerprint: &str, key: Vec<String>) -> Result<String, TaskError> {
    let bytes = serde_json::to_vec(&Cursor {
        version: 1,
        fingerprint: fingerprint.to_owned(),
        key,
    })
    .map_err(|_| TaskError::InvalidCursor)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn decode_cursor(value: &str) -> Result<Cursor, TaskError> {
    if !value.len().is_multiple_of(2) || value.len() > 8_192 {
        return Err(TaskError::InvalidCursor);
    }
    let bytes = (0..value.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&value[index..index + 2], 16))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| TaskError::InvalidCursor)?;
    serde_json::from_slice(&bytes).map_err(|_| TaskError::InvalidCursor)
}

fn validated_cursor(
    value: Option<&str>,
    fingerprint: &str,
) -> Result<Option<Vec<String>>, TaskError> {
    value
        .map(decode_cursor)
        .transpose()?
        .map(|cursor| {
            if cursor.version != 1 || cursor.fingerprint != fingerprint {
                Err(TaskError::InvalidCursor)
            } else {
                Ok(cursor.key)
            }
        })
        .transpose()
}

fn cursor_pair(value: Option<&str>, fingerprint: &str) -> Result<Option<(String, Id)>, TaskError> {
    let Some(key) = validated_cursor(value, fingerprint)? else {
        return Ok(None);
    };
    let [value, raw_id] = key.as_slice() else {
        return Err(TaskError::InvalidCursor);
    };
    let id = raw_id.parse().map_err(|_| TaskError::InvalidCursor)?;
    Ok(Some((value.clone(), id)))
}

fn cursor_i64_pair(value: Option<&str>, fingerprint: &str) -> Result<Option<(i64, Id)>, TaskError> {
    cursor_pair(value, fingerprint)?
        .map(|(value, id)| {
            value
                .parse()
                .map(|value| (value, id))
                .map_err(|_| TaskError::InvalidCursor)
        })
        .transpose()
}

fn task_fingerprint(workspace_id: Id, filter: &TaskFilter) -> String {
    format!(
        "tasks:w={workspace_id}:p={}:s={}:a={}:l={}:n={}:r={}:q={}:v={}:parent={}:nest={:?}:sort={:?}:order={:?}",
        filter
            .project_id
            .map_or_else(String::new, |id| id.to_string()),
        filter
            .status_id
            .map_or_else(String::new, |id| id.to_string()),
        filter
            .assignee_id
            .map_or_else(String::new, |id| id.to_string()),
        filter
            .label_id
            .map_or_else(String::new, |id| id.to_string()),
        filter
            .identifier
            .as_ref()
            .map_or_else(String::new, |(key, number)| format!("{key}-{number}")),
        filter.priority.as_deref().unwrap_or(""),
        filter.search.as_deref().unwrap_or(""),
        filter.view.as_deref().unwrap_or(""),
        filter
            .parent_id
            .map_or_else(String::new, |id| id.to_string()),
        filter.nesting,
        filter.sort,
        filter.order,
    )
}

fn task_cursor_key(task: &TaskRecord, filter: &TaskFilter) -> Vec<String> {
    let primary = match filter.sort {
        TaskSort::Position => task.position.to_string(),
        TaskSort::Priority => priority_rank(&task.priority).to_string(),
        TaskSort::Title => task.title.clone(),
        TaskSort::CreatedAt => task.created_at.as_millis().to_string(),
        TaskSort::UpdatedAt => task.updated_at.as_millis().to_string(),
        // Never reached: relevance queries return no cursor.
        TaskSort::Relevance => task.position.to_string(),
    };
    vec![primary, task.id.to_string()]
}

fn task_sort_column(sort: &TaskSort) -> &'static str {
    match sort {
        TaskSort::Position => "tasks.position",
        TaskSort::Priority => {
            "CASE tasks.priority WHEN 'urgent' THEN 0 WHEN 'high' THEN 1 WHEN 'medium' THEN 2 WHEN 'low' THEN 3 ELSE 4 END"
        }
        TaskSort::Title => "tasks.title",
        TaskSort::CreatedAt => "tasks.created_at",
        TaskSort::UpdatedAt => "tasks.updated_at",
        // Never reached: a relevance query short-circuits before the ORDER BY branch.
        TaskSort::Relevance => "tasks.position",
    }
}

fn push_cursor_value<'a>(
    query: &mut QueryBuilder<'a, Sqlite>,
    sort: &TaskSort,
    value: &str,
) -> Result<(), TaskError> {
    match sort {
        TaskSort::Title => {
            query.push_bind(value.to_owned());
        }
        TaskSort::Position
        | TaskSort::Priority
        | TaskSort::CreatedAt
        | TaskSort::UpdatedAt
        | TaskSort::Relevance => {
            query.push_bind(value.parse::<i64>().map_err(|_| TaskError::InvalidCursor)?);
        }
    }
    Ok(())
}

fn priority_rank(priority: &str) -> i64 {
    match priority {
        "urgent" => 0,
        "high" => 1,
        "medium" => 2,
        "low" => 3,
        _ => 4,
    }
}

/// Issues the next `KEY-n` for a project. One statement inside the caller's
/// BEGIN IMMEDIATE, so the counter cannot race. `NotFound` means the project is
/// missing or soft-deleted. Every task-creating path must go through this: the
/// `tasks_require_identifier_insert` trigger rejects a task without one.
async fn allocate_identifier(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    project_id: Id,
) -> Result<(String, i64), TaskError> {
    let allocation = sqlx::query(
        "UPDATE projects SET next_task_number = next_task_number + 1 \
         WHERE id = ? AND workspace_id = ? AND deleted_at IS NULL \
         RETURNING project_key, next_task_number - 1 AS number",
    )
    .bind(project_id.to_string())
    .bind(workspace_id.to_string())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(TaskError::NotFound)?;
    Ok((allocation.get("project_key"), allocation.get("number")))
}

/// Validates a client document and derives its stored columns. The server never
/// trusts a client-supplied text field: `description_text` / `body_text` and the
/// fts5 body always come from `extract_text` on the document that was stored.
fn document_columns(document: &Value, field: &'static str) -> Result<(String, String), TaskError> {
    rich_text::validate(document).map_err(|error| match error {
        DomainError::InvalidDocument { reason } => TaskError::InvalidDocument { field, reason },
        _ => TaskError::Invalid { field },
    })?;
    let text = rich_text::extract_text(document);
    let json = serde_json::to_string(document).map_err(|_| TaskError::InvalidDocument {
        field,
        reason: "document is not encodable",
    })?;
    Ok((json, text))
}

/// fts5 stores the hyphen-stripped UUID because unicode61 splits on hyphens; a
/// single token keeps `workspace_id:<value>` a cheap index lookup.
fn search_workspace_token(workspace_id: Id) -> String {
    workspace_id.to_string().replace('-', "")
}

/// Builds the fts5 MATCH expression. Every term is reduced to alphanumerics so a
/// user cannot inject fts5 operators, and the workspace is ANDed in so the index
/// never scans across workspaces. `None` means "no usable term" -> no results.
fn search_match_query(workspace_id: Id, search: &str) -> Option<String> {
    let terms: Vec<String> = search
        .split(|character: char| !character.is_alphanumeric())
        .filter(|term| !term.is_empty())
        .take(8)
        .map(str::to_lowercase)
        .collect();
    if terms.is_empty() {
        return None;
    }
    let clauses: Vec<String> = terms
        .iter()
        .map(|term| format!("(identifier:{term}* OR title:{term}* OR body:{term}*)"))
        .collect();
    Some(format!(
        "workspace_id:{} AND {}",
        search_workspace_token(workspace_id),
        clauses.join(" AND ")
    ))
}

/// Keeps `task_search` in step inside the caller's write transaction. The house
/// rule is that repositories own explicit SQL, so this is a delete + insert
/// rather than a SQL trigger.
async fn sync_task_search(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    task_id: Id,
    identifier: &str,
    title: &str,
    body: &str,
) -> Result<(), TaskError> {
    remove_task_search(tx, task_id).await?;
    sqlx::query(
        "INSERT INTO task_search (task_id, workspace_id, identifier, title, body) \
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(task_id.to_string())
    .bind(search_workspace_token(workspace_id))
    .bind(identifier)
    .bind(title)
    .bind(body)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn remove_task_search(
    tx: &mut Transaction<'_, Sqlite>,
    task_id: Id,
) -> Result<(), TaskError> {
    sqlx::query("DELETE FROM task_search WHERE task_id = ?")
        .bind(task_id.to_string())
        .execute(&mut **tx)
        .await?;
    Ok(())
}

async fn require_access(
    pool: &sqlx::SqlitePool,
    workspace_id: Id,
    actor_id: Id,
) -> Result<(), TaskError> {
    let exists: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM memberships JOIN workspaces ON workspaces.id = memberships.workspace_id \
         WHERE memberships.workspace_id = ? AND memberships.user_id = ? AND workspaces.deleted_at IS NULL",
    )
    .bind(workspace_id.to_string())
    .bind(actor_id.to_string())
    .fetch_one(pool)
    .await?;
    if exists == 1 {
        Ok(())
    } else {
        Err(TaskError::NotFound)
    }
}

async fn require_access_tx(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    actor_id: Id,
) -> Result<(), TaskError> {
    let exists: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM memberships JOIN workspaces ON workspaces.id = memberships.workspace_id \
         WHERE memberships.workspace_id = ? AND memberships.user_id = ? AND workspaces.deleted_at IS NULL",
    )
    .bind(workspace_id.to_string())
    .bind(actor_id.to_string())
    .fetch_one(&mut **tx)
    .await?;
    if exists == 1 {
        Ok(())
    } else {
        Err(TaskError::NotFound)
    }
}

async fn require_project(
    pool: &sqlx::SqlitePool,
    workspace_id: Id,
    project_id: Id,
    actor_id: Id,
) -> Result<(), TaskError> {
    require_access(pool, workspace_id, actor_id).await?;
    let exists: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM projects WHERE id = ? AND workspace_id = ? AND deleted_at IS NULL",
    )
    .bind(project_id.to_string())
    .bind(workspace_id.to_string())
    .fetch_one(pool)
    .await?;
    if exists == 1 {
        Ok(())
    } else {
        Err(TaskError::NotFound)
    }
}

async fn require_project_tx(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    project_id: Id,
    actor_id: Id,
) -> Result<(), TaskError> {
    require_access_tx(tx, workspace_id, actor_id).await?;
    let exists: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM projects WHERE id = ? AND workspace_id = ? AND deleted_at IS NULL",
    )
    .bind(project_id.to_string())
    .bind(workspace_id.to_string())
    .fetch_one(&mut **tx)
    .await?;
    if exists == 1 {
        Ok(())
    } else {
        Err(TaskError::NotFound)
    }
}

async fn require_task_tx(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    task_id: Id,
    actor_id: Id,
) -> Result<(), TaskError> {
    require_access_tx(tx, workspace_id, actor_id).await?;
    let exists: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tasks JOIN projects ON projects.id = tasks.project_id WHERE tasks.id = ? AND tasks.workspace_id = ? AND tasks.deleted_at IS NULL AND projects.deleted_at IS NULL")
        .bind(task_id.to_string()).bind(workspace_id.to_string()).fetch_one(&mut **tx).await?;
    if exists == 1 {
        Ok(())
    } else {
        Err(TaskError::NotFound)
    }
}

/// Bounded ancestor walk, closest first: the proposed parent, then its parent,
/// and so on, stopping at `MAX_PARENT_DEPTH` rows so a pathological chain can
/// never scan without limit.
const PARENT_WALK_SQL: &str = "WITH RECURSIVE ancestors(id, depth) AS ( \
     SELECT ?, 1 \
     UNION ALL \
     SELECT tasks.parent_id, ancestors.depth + 1 FROM tasks JOIN ancestors ON tasks.id = ancestors.id \
     WHERE tasks.workspace_id = ? AND tasks.parent_id IS NOT NULL AND ancestors.depth < ? \
 ) SELECT id FROM ancestors ORDER BY depth";

/// Bounded parent walk + domain rule. Runs inside the write transaction so the
/// graph cannot change between the check and the write.
async fn resolve_parent_tx(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    child_id: Id,
    parent_id: Id,
) -> Result<(), TaskError> {
    let exists: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tasks WHERE id = ? AND workspace_id = ? AND deleted_at IS NULL",
    )
    .bind(parent_id.to_string())
    .bind(workspace_id.to_string())
    .fetch_one(&mut **tx)
    .await?;
    if exists != 1 {
        return Err(TaskError::Invalid { field: "parent_id" });
    }
    let ancestors = sqlx::query_scalar::<_, String>(PARENT_WALK_SQL)
        .bind(parent_id.to_string())
        .bind(workspace_id.to_string())
        .bind(MAX_PARENT_DEPTH as i64)
        .fetch_all(&mut **tx)
        .await?
        .into_iter()
        .map(parse_id)
        .collect::<Result<Vec<_>, _>>()?;
    check_parent_edge(child_id, parent_id, &ancestors)
        .map_err(|_| TaskError::Invalid { field: "parent_id" })
}

async fn validate_project_status(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    project_id: Id,
    status_id: Id,
) -> Result<(), TaskError> {
    let exists: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM task_statuses JOIN projects ON projects.id = task_statuses.project_id \
         WHERE task_statuses.id = ? AND task_statuses.project_id = ? AND task_statuses.workspace_id = ? AND projects.deleted_at IS NULL",
    )
    .bind(status_id.to_string()).bind(project_id.to_string()).bind(workspace_id.to_string()).fetch_one(&mut **tx).await?;
    if exists == 1 {
        Ok(())
    } else {
        Err(TaskError::NotFound)
    }
}

async fn validate_assignees(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    ids: &[Id],
) -> Result<(), TaskError> {
    let mut unique = ids.to_vec();
    unique.sort_unstable();
    unique.dedup();
    if unique.len() != ids.len() {
        return Err(TaskError::Invalid {
            field: "assignee_ids",
        });
    }
    for id in ids {
        let exists: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM memberships JOIN users ON users.id = memberships.user_id WHERE memberships.workspace_id = ? AND memberships.user_id = ? AND users.suspended_at IS NULL")
            .bind(workspace_id.to_string()).bind(id.to_string()).fetch_one(&mut **tx).await?;
        if exists != 1 {
            return Err(TaskError::Invalid {
                field: "assignee_ids",
            });
        }
    }
    Ok(())
}

struct NotificationRequest<'a> {
    kind: &'a str,
    task_id: Id,
    comment_id: Option<Id>,
    recipients: &'a [Id],
    /// Assignees are a client-supplied list, so an unknown id is a caller error.
    /// Mention recipients are derived from the document, so an id that is no
    /// longer a member is simply skipped - the author cannot fix history.
    skip_unknown: bool,
}

async fn notify_users(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    actor_id: Id,
    request: NotificationRequest<'_>,
    now: TimestampMillis,
) -> Result<(), TaskError> {
    let field = if request.kind == "comment_mentioned" {
        "mentioned_user_ids"
    } else {
        "assignee_ids"
    };
    let mut unique = request.recipients.to_vec();
    unique.sort_unstable();
    unique.dedup();
    if !request.skip_unknown && unique.len() != request.recipients.len() {
        return Err(TaskError::Invalid { field });
    }
    for recipient in request
        .recipients
        .iter()
        .copied()
        .filter(|id| *id != actor_id)
    {
        let exists: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM memberships JOIN users ON users.id = memberships.user_id \
             WHERE memberships.workspace_id = ? AND memberships.user_id = ? AND users.suspended_at IS NULL",
        )
        .bind(workspace_id.to_string())
        .bind(recipient.to_string())
        .fetch_one(&mut **tx)
        .await?;
        if exists != 1 {
            if request.skip_unknown {
                continue;
            }
            return Err(TaskError::Invalid { field });
        }
        let dedupe_key = format!(
            "{workspace_id}:{recipient}:{}:{}",
            request.kind,
            request.comment_id.unwrap_or(request.task_id)
        );
        sqlx::query(
            "INSERT OR IGNORE INTO notifications \
             (id, workspace_id, recipient_user_id, actor_user_id, kind, task_id, comment_id, dedupe_key, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(Id::new_v7().to_string())
        .bind(workspace_id.to_string())
        .bind(recipient.to_string())
        .bind(actor_id.to_string())
        .bind(request.kind)
        .bind(request.task_id.to_string())
        .bind(request.comment_id.map(|id| id.to_string()))
        .bind(dedupe_key)
        .bind(now.as_millis())
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

async fn validate_labels(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    ids: &[Id],
) -> Result<(), TaskError> {
    let mut unique = ids.to_vec();
    unique.sort_unstable();
    unique.dedup();
    if unique.len() != ids.len() {
        return Err(TaskError::Invalid { field: "label_ids" });
    }
    for id in ids {
        let exists: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM labels WHERE workspace_id = ? AND id = ?")
                .bind(workspace_id.to_string())
                .bind(id.to_string())
                .fetch_one(&mut **tx)
                .await?;
        if exists != 1 {
            return Err(TaskError::Invalid { field: "label_ids" });
        }
    }
    Ok(())
}

async fn replace_assignees(
    tx: &mut Transaction<'_, Sqlite>,
    task_id: Id,
    ids: &[Id],
) -> Result<(), TaskError> {
    sqlx::query("DELETE FROM task_assignees WHERE task_id = ?")
        .bind(task_id.to_string())
        .execute(&mut **tx)
        .await?;
    for id in ids {
        let membership_id: String = sqlx::query_scalar(
            "SELECT id FROM memberships WHERE workspace_id = (SELECT workspace_id FROM tasks WHERE id = ?) AND user_id = ?",
        )
        .bind(task_id.to_string())
        .bind(id.to_string())
        .fetch_one(&mut **tx)
        .await?;
        sqlx::query(
            "INSERT INTO task_assignees (task_id, membership_id, user_id) VALUES (?, ?, ?)",
        )
        .bind(task_id.to_string())
        .bind(membership_id)
        .bind(id.to_string())
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

async fn replace_labels(
    tx: &mut Transaction<'_, Sqlite>,
    task_id: Id,
    ids: &[Id],
) -> Result<(), TaskError> {
    sqlx::query("DELETE FROM task_labels WHERE task_id = ?")
        .bind(task_id.to_string())
        .execute(&mut **tx)
        .await?;
    for id in ids {
        sqlx::query("INSERT INTO task_labels (task_id, label_id) VALUES (?, ?)")
            .bind(task_id.to_string())
            .bind(id.to_string())
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}

/// Replaces every reference row owned by this source with the task mentions in
/// its document. Derived, never hand-maintained.
///
/// DELIBERATELY writes no audit row: `audit_workspace_realtime` in
/// apps/server/migrations/0011_realtime.sql fires an outbox event on every
/// successful audit insert, so an audited rebuild would fan a second realtime
/// event out to every connected client on every debounced description save.
async fn rebuild_task_references(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    source_type: &str,
    source_id: Id,
    document: &Value,
) -> Result<(), TaskError> {
    sqlx::query("DELETE FROM task_references WHERE source_type = ? AND source_id = ?")
        .bind(source_type)
        .bind(source_id.to_string())
        .execute(&mut **tx)
        .await?;
    // extract_task_ids is already deduplicated, in document order.
    for target in rich_text::extract_task_ids(document) {
        if target == source_id {
            continue;
        }
        // The SELECT source silently drops unknown or foreign targets, so a
        // stale mention can never abort the user's save via the scope trigger.
        sqlx::query(
            "INSERT OR IGNORE INTO task_references (workspace_id, source_type, source_id, target_task_id) \
             SELECT ?, ?, ?, tasks.id FROM tasks WHERE tasks.id = ? AND tasks.workspace_id = ?",
        )
        .bind(workspace_id.to_string())
        .bind(source_type)
        .bind(source_id.to_string())
        .bind(target.to_string())
        .bind(workspace_id.to_string())
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

async fn insert_default_statuses(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    project_id: Id,
    now: TimestampMillis,
) -> Result<(), TaskError> {
    let statuses = [
        ("Backlog", "#8b8f98", StatusCategory::Unstarted),
        ("Todo", "#8b8f98", StatusCategory::Unstarted),
        ("In Progress", "#f2c94c", StatusCategory::Started),
        ("Done", "#4cb782", StatusCategory::Completed),
        ("Cancelled", "#8b8f98", StatusCategory::Cancelled),
    ];
    for (position, (name, color, category)) in statuses.into_iter().enumerate() {
        let category = match category {
            StatusCategory::Unstarted => "unstarted",
            StatusCategory::Started => "started",
            StatusCategory::Completed => "completed",
            StatusCategory::Cancelled => "cancelled",
        };
        sqlx::query("INSERT INTO task_statuses (id, workspace_id, project_id, name, description, color, category, position, version, created_at, updated_at) VALUES (?, ?, ?, ?, '', ?, ?, ?, 0, ?, ?)")
            .bind(Id::new_v7().to_string()).bind(workspace_id.to_string()).bind(project_id.to_string()).bind(name).bind(color).bind(category).bind(position as i64).bind(now.as_millis()).bind(now.as_millis()).execute(&mut **tx).await?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn record_mutation(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    actor_id: Id,
    action: &str,
    resource_type: &str,
    resource_id: Id,
    request_id: &str,
    now: TimestampMillis,
) -> Result<(), TaskError> {
    audit::record(
        tx,
        workspace_id,
        Some(actor_id),
        action,
        AuditOutcome::Success,
        resource_type,
        Some(resource_id),
        request_id,
        json!({}),
        now,
    )
    .await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn record_principal_mutation(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    actor_id: Id,
    service_account: Option<(Id, &str)>,
    action: &str,
    resource_type: &str,
    resource_id: Id,
    request_id: &str,
    now: TimestampMillis,
) -> Result<(), TaskError> {
    if let Some((service_account_id, service_account_name)) = service_account {
        audit::record(
            tx,
            workspace_id,
            None,
            action,
            AuditOutcome::Success,
            resource_type,
            Some(resource_id),
            request_id,
            json!({
                "actor_service_account_id": service_account_id,
                "actor_service_account_name": service_account_name,
            }),
            now,
        )
        .await?;
        Ok(())
    } else {
        record_mutation(
            tx,
            workspace_id,
            actor_id,
            action,
            resource_type,
            resource_id,
            request_id,
            now,
        )
        .await
    }
}

fn is_unique_violation(result: &Result<sqlx::sqlite::SqliteQueryResult, sqlx::Error>) -> bool {
    matches!(result, Err(sqlx::Error::Database(error)) if error.is_unique_violation())
}

fn check_version<T: Serialize>(expected: u64, current: u64, record: &T) -> Result<(), TaskError> {
    if expected == current {
        Ok(())
    } else {
        Err(TaskError::VersionConflict {
            current: Box::new(serde_json::to_value(record).map_err(|_| TaskError::Conflict)?),
        })
    }
}

async fn project_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    project_id: Id,
    deleted: bool,
) -> Result<ProjectRecord, TaskError> {
    let row = sqlx::query(
        "SELECT id, workspace_id, name, COALESCE(restore_project_key, project_key) AS project_key, color, version, deleted_at, created_at, updated_at \
         FROM projects WHERE id = ? AND workspace_id = ? AND ((? = 1 AND deleted_at IS NOT NULL) OR (? = 0 AND deleted_at IS NULL))",
    )
    .bind(project_id.to_string()).bind(workspace_id.to_string()).bind(i64::from(deleted)).bind(i64::from(deleted))
    .fetch_optional(&mut **tx).await?.ok_or(TaskError::NotFound)?;
    project_from_row(row)
}

async fn status_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    project_id: Id,
    status_id: Id,
) -> Result<StatusRecord, TaskError> {
    let row = sqlx::query("SELECT id, workspace_id, project_id, name, description, color, category, position, version FROM task_statuses WHERE id = ? AND workspace_id = ? AND project_id = ?")
        .bind(status_id.to_string()).bind(workspace_id.to_string()).bind(project_id.to_string()).fetch_optional(&mut **tx).await?.ok_or(TaskError::NotFound)?;
    status_from_row(row)
}

async fn label_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    label_id: Id,
) -> Result<LabelRecord, TaskError> {
    let row = sqlx::query("SELECT id, workspace_id, name, color, version FROM labels WHERE id = ? AND workspace_id = ?")
        .bind(label_id.to_string()).bind(workspace_id.to_string()).fetch_optional(&mut **tx).await?.ok_or(TaskError::NotFound)?;
    label_from_row(row)
}

async fn task_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    task_id: Id,
    deleted: bool,
) -> Result<TaskRecord, TaskError> {
    let row = sqlx::query(
        "SELECT tasks.id, tasks.workspace_id, tasks.project_id, tasks.status_id, tasks.identifier_key, tasks.number, tasks.title, tasks.description_json, tasks.description_text, tasks.priority, tasks.position, tasks.creator_id, tasks.creator_service_account_id, (SELECT name FROM service_accounts WHERE id = tasks.creator_service_account_id) AS creator_service_account_name, tasks.due_at, tasks.version, tasks.deleted_at, tasks.created_at, tasks.updated_at, tasks.parent_id \
         FROM tasks JOIN projects ON projects.id = tasks.project_id WHERE tasks.id = ? AND tasks.workspace_id = ? \
         AND ((? = 1 AND tasks.deleted_at IS NOT NULL) OR (? = 0 AND tasks.deleted_at IS NULL)) AND (? = 1 OR projects.deleted_at IS NULL)",
    )
    .bind(task_id.to_string()).bind(workspace_id.to_string()).bind(i64::from(deleted)).bind(i64::from(deleted)).bind(i64::from(deleted))
    .fetch_optional(&mut **tx).await?.ok_or(TaskError::NotFound)?;
    task_from_row_tx(tx, row).await
}

async fn comment_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    task_id: Id,
    comment_id: Id,
) -> Result<CommentRecord, TaskError> {
    let row = sqlx::query("SELECT id, workspace_id, task_id, author_id, parent_id, body_json, body_text, version, created_at, updated_at FROM task_comments WHERE id = ? AND workspace_id = ? AND task_id = ?")
        .bind(comment_id.to_string()).bind(workspace_id.to_string()).bind(task_id.to_string()).fetch_optional(&mut **tx).await?.ok_or(TaskError::NotFound)?;
    comment_from_row(row)
}

fn project_from_row(row: sqlx::sqlite::SqliteRow) -> Result<ProjectRecord, TaskError> {
    Ok(ProjectRecord {
        id: parse_id(row.get("id"))?,
        workspace_id: parse_id(row.get("workspace_id"))?,
        name: row.get("name"),
        key: row.get("project_key"),
        color: row.get("color"),
        version: parse_version(row.get("version"))?,
        deleted_at: row
            .get::<Option<i64>, _>("deleted_at")
            .map(TimestampMillis::from_millis),
        created_at: TimestampMillis::from_millis(row.get("created_at")),
        updated_at: TimestampMillis::from_millis(row.get("updated_at")),
    })
}

fn status_from_row(row: sqlx::sqlite::SqliteRow) -> Result<StatusRecord, TaskError> {
    Ok(StatusRecord {
        id: parse_id(row.get("id"))?,
        workspace_id: parse_id(row.get("workspace_id"))?,
        project_id: parse_id(row.get("project_id"))?,
        name: row.get("name"),
        description: row.get("description"),
        color: row.get("color"),
        category: row.get("category"),
        position: row.get("position"),
        version: parse_version(row.get("version"))?,
    })
}

fn label_from_row(row: sqlx::sqlite::SqliteRow) -> Result<LabelRecord, TaskError> {
    Ok(LabelRecord {
        id: parse_id(row.get("id"))?,
        workspace_id: parse_id(row.get("workspace_id"))?,
        name: row.get("name"),
        color: row.get("color"),
        version: parse_version(row.get("version"))?,
    })
}

/// One grouped aggregate over `tasks_parent_rollup` for a whole page of
/// parents: one placeholder per parent, never a correlated subquery per row.
fn sub_issue_rollup_sql(parent_count: usize) -> String {
    let placeholders = vec!["?"; parent_count].join(", ");
    format!(
        "SELECT tasks.parent_id AS parent_id, COUNT(*) AS total, \
         SUM(CASE WHEN task_statuses.category IN ('completed', 'cancelled') THEN 1 ELSE 0 END) AS done \
         FROM tasks JOIN task_statuses ON task_statuses.id = tasks.status_id \
         WHERE tasks.workspace_id = ? AND tasks.deleted_at IS NULL \
         AND tasks.parent_id IN ({placeholders}) GROUP BY tasks.parent_id"
    )
}

fn duplicate_of_sql(task_count: usize) -> String {
    let placeholders = vec!["?"; task_count].join(", ");
    format!(
        "SELECT source_task_id, target_task_id FROM task_relations \
         WHERE workspace_id = ? AND kind = 'duplicate_of' AND source_task_id IN ({placeholders})"
    )
}

/// The per-page graph summary: the sub-issue rollup and the duplicate marker.
/// Exactly two queries for the whole page, whatever its size. Never call this
/// per row.
async fn apply_task_graph(
    connection: &mut sqlx::SqliteConnection,
    workspace_id: Id,
    records: &mut [TaskRecord],
) -> Result<(), TaskError> {
    if records.is_empty() {
        return Ok(());
    }
    let sql = sub_issue_rollup_sql(records.len());
    let mut query = sqlx::query_as::<_, (String, i64, i64)>(&sql).bind(workspace_id.to_string());
    for record in records.iter() {
        query = query.bind(record.id.to_string());
    }
    let rollup: HashMap<String, (i64, i64)> = query
        .fetch_all(&mut *connection)
        .await?
        .into_iter()
        .map(|(parent, total, done)| (parent, (total, done)))
        .collect();

    let sql = duplicate_of_sql(records.len());
    let mut query = sqlx::query_as::<_, (String, String)>(&sql).bind(workspace_id.to_string());
    for record in records.iter() {
        query = query.bind(record.id.to_string());
    }
    let duplicate_of: HashMap<String, String> = query
        .fetch_all(&mut *connection)
        .await?
        .into_iter()
        .collect();

    for record in records.iter_mut() {
        let key = record.id.to_string();
        let (total, done) = rollup.get(&key).copied().unwrap_or((0, 0));
        record.sub_issue_total = total;
        record.sub_issue_done = done;
        record.duplicate_of_task_id = duplicate_of.get(&key).cloned().map(parse_id).transpose()?;
    }
    Ok(())
}

async fn task_from_row(
    pool: &sqlx::SqlitePool,
    row: sqlx::sqlite::SqliteRow,
) -> Result<TaskRecord, TaskError> {
    let id = parse_id(row.get("id"))?;
    let assignee_ids = sqlx::query_scalar::<_, String>(
        "SELECT task_assignees.user_id FROM task_assignees \
         JOIN memberships ON memberships.id = task_assignees.membership_id \
         JOIN users ON users.id = task_assignees.user_id \
         JOIN tasks ON tasks.id = task_assignees.task_id \
         WHERE task_assignees.task_id = ? AND memberships.workspace_id = tasks.workspace_id \
         AND memberships.user_id = task_assignees.user_id AND users.suspended_at IS NULL \
         ORDER BY task_assignees.user_id",
    )
    .bind(id.to_string())
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(parse_id)
    .collect::<Result<_, _>>()?;
    let label_ids = sqlx::query_scalar::<_, String>(
        "SELECT label_id FROM task_labels WHERE task_id = ? ORDER BY label_id",
    )
    .bind(id.to_string())
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(parse_id)
    .collect::<Result<_, _>>()?;
    task_record_from_row(row, id, assignee_ids, label_ids)
}

async fn task_from_row_tx(
    tx: &mut Transaction<'_, Sqlite>,
    row: sqlx::sqlite::SqliteRow,
) -> Result<TaskRecord, TaskError> {
    let id = parse_id(row.get("id"))?;
    let assignee_ids = sqlx::query_scalar::<_, String>(
        "SELECT task_assignees.user_id FROM task_assignees \
         JOIN memberships ON memberships.id = task_assignees.membership_id \
         JOIN users ON users.id = task_assignees.user_id \
         JOIN tasks ON tasks.id = task_assignees.task_id \
         WHERE task_assignees.task_id = ? AND memberships.workspace_id = tasks.workspace_id \
         AND memberships.user_id = task_assignees.user_id AND users.suspended_at IS NULL \
         ORDER BY task_assignees.user_id",
    )
    .bind(id.to_string())
    .fetch_all(&mut **tx)
    .await?
    .into_iter()
    .map(parse_id)
    .collect::<Result<_, _>>()?;
    let label_ids = sqlx::query_scalar::<_, String>(
        "SELECT label_id FROM task_labels WHERE task_id = ? ORDER BY label_id",
    )
    .bind(id.to_string())
    .fetch_all(&mut **tx)
    .await?
    .into_iter()
    .map(parse_id)
    .collect::<Result<_, _>>()?;
    task_record_from_row(row, id, assignee_ids, label_ids)
}

fn task_record_from_row(
    row: sqlx::sqlite::SqliteRow,
    id: Id,
    assignee_ids: Vec<Id>,
    label_ids: Vec<Id>,
) -> Result<TaskRecord, TaskError> {
    // ALTER TABLE cannot make these NOT NULL, so a row without them is corrupt rather than absent.
    let (identifier_key, number) = match (
        row.get::<Option<String>, _>("identifier_key"),
        row.get::<Option<i64>, _>("number"),
    ) {
        (Some(identifier_key), Some(number)) => (identifier_key, number),
        _ => return Err(TaskError::Conflict),
    };
    let creator_service_account_id = row
        .get::<Option<String>, _>("creator_service_account_id")
        .map(parse_id)
        .transpose()?;
    Ok(TaskRecord {
        id,
        workspace_id: parse_id(row.get("workspace_id"))?,
        project_id: parse_id(row.get("project_id"))?,
        status_id: parse_id(row.get("status_id"))?,
        identifier: format!("{identifier_key}-{number}"),
        identifier_key,
        number,
        title: row.get("title"),
        description_json: serde_json::from_str(&row.get::<String, _>("description_json"))
            .unwrap_or_else(|_| rich_text::empty_document()),
        description_text: row.get("description_text"),
        priority: row.get("priority"),
        position: row.get("position"),
        creator_id: if creator_service_account_id.is_some() {
            None
        } else {
            Some(parse_id(row.get("creator_id"))?)
        },
        creator_service_account_id,
        creator_service_account_name: row.get("creator_service_account_name"),
        parent_id: row
            .get::<Option<String>, _>("parent_id")
            .map(parse_id)
            .transpose()?,
        // Filled in by one batch pass per page; see `apply_task_graph`.
        sub_issue_total: 0,
        sub_issue_done: 0,
        duplicate_of_task_id: None,
        duplicate_ids: Vec::new(),
        referenced_by: Vec::new(),
        assignee_ids,
        label_ids,
        due_at: row
            .get::<Option<i64>, _>("due_at")
            .map(TimestampMillis::from_millis),
        version: parse_version(row.get("version"))?,
        deleted_at: row
            .get::<Option<i64>, _>("deleted_at")
            .map(TimestampMillis::from_millis),
        created_at: TimestampMillis::from_millis(row.get("created_at")),
        updated_at: TimestampMillis::from_millis(row.get("updated_at")),
    })
}

fn notification_from_row(row: sqlx::sqlite::SqliteRow) -> Result<NotificationRecord, TaskError> {
    Ok(NotificationRecord {
        id: parse_id(row.get("id"))?,
        workspace_id: parse_id(row.get("workspace_id"))?,
        recipient_user_id: parse_id(row.get("recipient_user_id"))?,
        actor_user_id: parse_id(row.get("actor_user_id"))?,
        kind: row.get("kind"),
        task_id: parse_id(row.get("task_id"))?,
        comment_id: row
            .get::<Option<String>, _>("comment_id")
            .map(parse_id)
            .transpose()?,
        read_at: row
            .get::<Option<i64>, _>("read_at")
            .map(TimestampMillis::from_millis),
        created_at: TimestampMillis::from_millis(row.get("created_at")),
    })
}

fn comment_from_row(row: sqlx::sqlite::SqliteRow) -> Result<CommentRecord, TaskError> {
    Ok(CommentRecord {
        id: parse_id(row.get("id"))?,
        workspace_id: parse_id(row.get("workspace_id"))?,
        task_id: parse_id(row.get("task_id"))?,
        author_id: parse_id(row.get("author_id"))?,
        parent_id: row
            .get::<Option<String>, _>("parent_id")
            .map(parse_id)
            .transpose()?,
        body_json: serde_json::from_str(&row.get::<String, _>("body_json"))
            .unwrap_or_else(|_| rich_text::empty_document()),
        body_text: row.get("body_text"),
        version: parse_version(row.get("version"))?,
        created_at: TimestampMillis::from_millis(row.get("created_at")),
        updated_at: TimestampMillis::from_millis(row.get("updated_at")),
    })
}

fn parse_id(value: String) -> Result<Id, TaskError> {
    value.parse().map_err(|_| TaskError::Conflict)
}
fn parse_version(value: i64) -> Result<u64, TaskError> {
    u64::try_from(value).map_err(|_| TaskError::Conflict)
}

#[cfg(test)]
mod tests {
    use super::sub_issue_rollup_sql;

    #[test]
    fn rollup_is_one_grouped_statement_with_one_placeholder_per_parent() {
        let sql = sub_issue_rollup_sql(3);
        assert_eq!(sql.matches(';').count(), 0, "one statement only");
        assert_eq!(sql.matches("SELECT").count(), 1, "no correlated subquery");
        assert!(sql.contains("GROUP BY tasks.parent_id"));
        assert!(sql.contains("IN (?, ?, ?)"));
        assert!(sql.contains("tasks.deleted_at IS NULL"));
    }
}
