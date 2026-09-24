use orbit_domain::{DEFAULT_STATUSES, StatusCategory};
use orbit_platform::{Database, Id, TimestampMillis};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{QueryBuilder, Row, Sqlite, Transaction};
use thiserror::Error;
use utoipa::ToSchema;

use super::task_relations::{self, RelationActor};
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
    pub title: String,
    pub description: String,
    pub source_url: Option<String>,
    pub priority: String,
    pub position: i64,
    #[schema(value_type = Option<String>)]
    pub creator_id: Option<Id>,
    #[schema(value_type = Option<String>)]
    pub creator_service_account_id: Option<Id>,
    pub creator_service_account_name: Option<String>,
    #[schema(value_type = Vec<String>)]
    pub assignee_ids: Vec<Id>,
    #[schema(value_type = Vec<String>)]
    pub label_ids: Vec<Id>,
    #[schema(value_type = Option<String>, format = DateTime)]
    pub due_start_at: Option<TimestampMillis>,
    #[schema(value_type = Option<String>, format = DateTime)]
    pub due_at: Option<TimestampMillis>,
    pub version: u64,
    #[schema(value_type = Option<String>, format = DateTime)]
    pub deleted_at: Option<TimestampMillis>,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: TimestampMillis,
    #[schema(value_type = String, format = DateTime)]
    pub updated_at: TimestampMillis,
    /// The task this one duplicates; null when it is not a duplicate or its target is in the trash.
    #[schema(required = true)]
    pub duplicate_of: Option<TaskRef>,
    /// True while at least one live task that is not completed, cancelled or a duplicate blocks it.
    pub blocked: bool,
}

/// A task reference small enough to embed; clients build the display identifier themselves.
#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct TaskRef {
    #[schema(value_type = String)]
    pub id: Id,
    #[schema(value_type = String)]
    pub project_id: Id,
    pub title: String,
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
    pub body: String,
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
    pub description: String,
    pub source_url: Option<String>,
    pub priority: String,
    pub position: Option<i64>,
    pub assignee_ids: Vec<Id>,
    pub label_ids: Vec<Id>,
    pub due_start_at: Option<TimestampMillis>,
    pub due_at: Option<TimestampMillis>,
}

#[derive(Clone, Debug)]
pub struct DiscordTask {
    pub source_url: String,
    pub event_id: String,
    pub payload_hash: [u8; 32],
    pub project_id: Id,
    pub title: String,
    pub description: String,
}

#[derive(Clone, Debug)]
pub struct GithubWorkItem {
    pub repository: String,
    pub number: i64,
    pub project_id: Id,
    pub title: String,
    pub description: String,
    pub kind: &'static str,
    pub state: &'static str,
    pub state_changed: bool,
}

#[derive(Clone, Debug, Default)]
pub struct TaskChanges {
    pub project_id: Option<Id>,
    pub status_id: Option<Id>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub source_url: Option<Option<String>>,
    pub priority: Option<String>,
    pub position: Option<i64>,
    pub assignee_ids: Option<Vec<Id>>,
    pub label_ids: Option<Vec<Id>>,
    pub due_start_at: Option<Option<TimestampMillis>>,
    pub due_at: Option<Option<TimestampMillis>>,
    /// `Some(Some(id))` marks the task as a duplicate of `id`; `Some(None)` unmarks it.
    pub duplicate_of_id: Option<Option<Id>>,
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
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SortOrder {
    Asc,
    Desc,
}

#[derive(Clone, Debug)]
pub struct TaskFilter {
    pub project_id: Option<Id>,
    pub status_id: Option<Id>,
    pub assignee_id: Option<Id>,
    pub unassigned: bool,
    pub label_id: Option<Id>,
    pub priority: Option<String>,
    pub search: Option<String>,
    pub view: Option<String>,
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
    #[error("task operation conflicts with current state")]
    Conflict,
    #[error("GitHub controls this task's title and description")]
    GithubContentReadOnly,
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
    pub async fn reconcile_github_attribution(
        &self,
        now: TimestampMillis,
    ) -> Result<(), TaskError> {
        let workspaces = sqlx::query("SELECT DISTINCT workspace_id FROM github_issue_links")
            .fetch_all(self.database.pool())
            .await?;
        for row in workspaces {
            let workspace_id = parse_id(row.get("workspace_id"))?;
            let mut tx = self.database.immediate_transaction().await?;
            let owner: Option<String> = sqlx::query_scalar(
                "SELECT user_id FROM memberships WHERE workspace_id = ? AND role = 'owner' LIMIT 1",
            )
            .bind(workspace_id.to_string())
            .fetch_optional(&mut *tx)
            .await?;
            let Some(owner) = owner else { continue };
            let (service_account_id, service_account_name) =
                github_service_account_in_tx(&mut tx, workspace_id, parse_id(owner)?, now).await?;
            sqlx::query("UPDATE tasks SET creator_service_account_id = ? WHERE workspace_id = ? AND creator_service_account_id IS NULL AND id IN (SELECT task_id FROM github_issue_links WHERE workspace_id = ?)")
                .bind(service_account_id.to_string()).bind(workspace_id.to_string())
                .bind(workspace_id.to_string()).execute(&mut *tx).await?;
            sqlx::query("UPDATE audit_events SET actor_id = NULL, metadata_json = json_set(metadata_json, '$.actor_service_account_id', ?, '$.actor_service_account_name', ?) WHERE workspace_id = ? AND resource_type = 'task' AND resource_id IN (SELECT task_id FROM github_issue_links WHERE workspace_id = ?) AND actor_id IS NOT NULL AND action IN ('task.created', 'github.work_item.link.updated', 'github.issue.link.updated')")
                .bind(service_account_id.to_string()).bind(&service_account_name)
                .bind(workspace_id.to_string()).bind(workspace_id.to_string())
                .execute(&mut *tx).await?;
            tx.commit().await?;
        }
        Ok(())
    }

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

    #[must_use]
    pub fn database(&self) -> &Database {
        &self.database
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
        let inserted = sqlx::query(
            "INSERT INTO projects (id, workspace_id, name, project_key, color, version, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, 0, ?, ?)",
        )
        .bind(project_id.to_string())
        .bind(workspace_id.to_string())
        .bind(&name)
        .bind(&key)
        .bind(&color)
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
        if category == StatusCategory::Duplicate.as_str() {
            return Err(TaskError::Invalid { field: "category" });
        }
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
        let duplicate = StatusCategory::Duplicate.as_str();
        if (current.category == duplicate) != (category == duplicate) {
            return Err(TaskError::Invalid { field: "category" });
        }
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
        if current.category == StatusCategory::Duplicate.as_str() {
            return Err(TaskError::Invalid { field: "status_id" });
        }
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
        let mut query = QueryBuilder::<Sqlite>::new(format!(
            "SELECT {} FROM tasks JOIN projects ON projects.id = tasks.project_id WHERE tasks.workspace_id = ",
            task_columns()
        ));
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
        if let Some(priority) = &filter.priority {
            query
                .push(" AND tasks.priority = ")
                .push_bind(priority.clone());
        }
        if matches!(filter.view.as_deref(), Some("mine") | Some("my_week"))
            || filter.assignee_id.is_some()
        {
            let assignee_id = if matches!(filter.view.as_deref(), Some("mine") | Some("my_week")) {
                actor_id
            } else {
                filter.assignee_id.unwrap()
            };
            query
                .push(" AND EXISTS (SELECT 1 FROM task_assignees JOIN memberships ON memberships.id = task_assignees.membership_id JOIN users ON users.id = task_assignees.user_id WHERE task_assignees.task_id = tasks.id AND memberships.workspace_id = tasks.workspace_id AND memberships.user_id = task_assignees.user_id AND users.suspended_at IS NULL AND task_assignees.user_id = ")
                .push_bind(assignee_id.to_string())
                .push(")");
        }
        if filter.unassigned {
            query.push(" AND NOT EXISTS (SELECT 1 FROM task_assignees JOIN memberships ON memberships.id = task_assignees.membership_id JOIN users ON users.id = task_assignees.user_id WHERE task_assignees.task_id = tasks.id AND memberships.workspace_id = tasks.workspace_id AND memberships.user_id = task_assignees.user_id AND users.suspended_at IS NULL)");
        }
        if matches!(
            filter.view.as_deref(),
            Some("overdue") | Some("due_soon") | Some("current_week") | Some("my_week")
        ) {
            let day_ms = 86_400_000;
            let start_of_utc_day = (TimestampMillis::now().as_millis() / day_ms) * day_ms;
            query.push(
                " AND tasks.due_at IS NOT NULL AND EXISTS (SELECT 1 FROM task_statuses WHERE task_statuses.id = tasks.status_id AND task_statuses.category NOT IN ('completed', 'cancelled', 'duplicate'))",
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
        if let Some(search) = &filter.search {
            let pattern = format!("%{}%", escape_like(&search.to_lowercase()));
            query
                .push(" AND (LOWER(tasks.title) LIKE ")
                .push_bind(pattern.clone())
                .push(" ESCAPE '\\' OR LOWER(tasks.description) LIKE ")
                .push_bind(pattern)
                .push(" ESCAPE '\\')");
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
        let sql = format!(
            "SELECT {} FROM tasks JOIN projects ON projects.id = tasks.project_id WHERE tasks.id = ? AND tasks.workspace_id = ? \
             AND tasks.deleted_at IS NULL AND projects.deleted_at IS NULL",
            task_columns()
        );
        let row = sqlx::query(&sql)
            .bind(task_id.to_string())
            .bind(workspace_id.to_string())
            .fetch_optional(self.database.pool())
            .await?
            .ok_or(TaskError::NotFound)?;
        task_from_row(self.database.pool(), row).await
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
        if input
            .due_start_at
            .is_some_and(|start| input.due_at.is_none_or(|end| start > end))
        {
            return Err(TaskError::Invalid {
                field: "due_start_at",
            });
        }
        let mut tx = self.database.immediate_transaction().await?;
        require_access_tx(&mut tx, workspace_id, actor_id).await?;
        validate_project_status(&mut tx, workspace_id, input.project_id, input.status_id).await?;
        if task_relations::status_category_in_tx(&mut tx, workspace_id, input.status_id).await?
            == task_relations::DUPLICATE
        {
            return Err(TaskError::Invalid { field: "status_id" });
        }
        validate_assignees(&mut tx, workspace_id, &input.assignee_ids).await?;
        validate_labels(&mut tx, workspace_id, &input.label_ids).await?;
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
            "INSERT INTO tasks (id, workspace_id, project_id, status_id, title, description, source_url, priority, position, creator_id, due_start_at, due_at, version, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, ?, ?)",
        )
        .bind(id.to_string())
        .bind(workspace_id.to_string())
        .bind(input.project_id.to_string())
        .bind(input.status_id.to_string())
        .bind(&input.title)
        .bind(&input.description)
        .bind(&input.source_url)
        .bind(&input.priority)
        .bind(position)
        .bind(actor_id.to_string())
        .bind(input.due_start_at.map(TimestampMillis::as_millis))
        .bind(input.due_at.map(TimestampMillis::as_millis))
        .bind(now.as_millis())
        .bind(now.as_millis())
        .execute(&mut *tx)
        .await?;
        replace_assignees(&mut tx, id, &input.assignee_ids).await?;
        replace_labels(&mut tx, id, &input.label_ids).await?;
        notify_users(
            &mut tx,
            workspace_id,
            actor_id,
            NotificationRequest {
                kind: "task_assigned",
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
            title: input.title,
            description: input.description,
            source_url: input.source_url,
            priority: input.priority,
            position,
            creator_id: Some(actor_id),
            creator_service_account_id: None,
            creator_service_account_name: None,
            assignee_ids: input.assignee_ids,
            label_ids: input.label_ids,
            due_start_at: input.due_start_at,
            due_at: input.due_at,
            version: 0,
            deleted_at: None,
            created_at: now,
            updated_at: now,
            duplicate_of: None,
            blocked: false,
        })
    }

    pub async fn sync_github_work_item(
        &self,
        workspace_id: Id,
        actor_id: Id,
        issue: GithubWorkItem,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<Id, TaskError> {
        let mut tx = self.database.immediate_transaction().await?;
        require_access_tx(&mut tx, workspace_id, actor_id).await?;
        let (service_account_id, service_account_name) =
            github_service_account_in_tx(&mut tx, workspace_id, actor_id, now).await?;
        project_in_tx(&mut tx, workspace_id, issue.project_id, false).await?;
        if issue.kind == "pull_request" {
            sqlx::query("DELETE FROM github_pull_links WHERE workspace_id = ? AND repository = ? AND pull_number = ?")
                .bind(workspace_id.to_string()).bind(&issue.repository).bind(issue.number)
                .execute(&mut *tx).await?;
        }
        let path = if issue.kind == "pull_request" {
            "pull"
        } else {
            "issues"
        };
        let issue_url = format!(
            "https://github.com/{}/{path}/{}",
            issue.repository, issue.number
        );
        let status_category = match (issue.kind, issue.state) {
            ("pull_request", "merged") | ("issue", "closed") => "completed",
            ("pull_request", "closed") => "cancelled",
            _ => "unstarted",
        };
        let existing = sqlx::query(
            "SELECT github_issue_links.task_id, github_issue_links.kind, tasks.project_id, tasks.status_id, tasks.deleted_at FROM github_issue_links JOIN tasks ON tasks.id = github_issue_links.task_id WHERE github_issue_links.workspace_id = ? AND repository = ? AND issue_number = ?",
        ).bind(workspace_id.to_string()).bind(&issue.repository).bind(issue.number).fetch_optional(&mut *tx).await?;
        if let Some(row) = &existing {
            let was_deleted = row.get::<Option<i64>, _>("deleted_at").is_some();
            let task_id = parse_id(row.get("task_id"))?;
            if parse_id(row.get("project_id"))? != issue.project_id
                || row.get::<String, _>("kind") != issue.kind
            {
                return Err(TaskError::IntegrationConflict);
            }
            let current_status_id: String = row.get("status_id");
            let status_id: String = if issue.state_changed || was_deleted {
                sqlx::query_scalar(
                        "SELECT id FROM task_statuses WHERE workspace_id = ? AND project_id = ? AND category = ? ORDER BY position, id LIMIT 1",
                    ).bind(workspace_id.to_string()).bind(row.get::<String, _>("project_id"))
                        .bind(status_category).fetch_optional(&mut *tx).await?
                        .unwrap_or_else(|| current_status_id.clone())
            } else {
                current_status_id.clone()
            };
            let leaves_duplicate = current_status_id != status_id
                && task_relations::status_category_in_tx(
                    &mut tx,
                    workspace_id,
                    parse_id(current_status_id)?,
                )
                .await?
                    == task_relations::DUPLICATE;
            let changed = sqlx::query(
                    "UPDATE tasks SET title = ?, description = ?, source_url = ?, status_id = ?, deleted_at = NULL, version = version + 1, updated_at = ? WHERE id = ? AND workspace_id = ? AND (deleted_at IS NOT NULL OR title != ? OR description != ? OR source_url IS NOT ? OR status_id != ?)",
                ).bind(&issue.title).bind(&issue.description).bind(&issue_url).bind(&status_id).bind(now.as_millis())
                    .bind(task_id.to_string()).bind(workspace_id.to_string())
                    .bind(&issue.title).bind(&issue.description).bind(&issue_url).bind(&status_id).execute(&mut *tx).await?;
            if leaves_duplicate {
                task_relations::delete_duplicate_relation_in_tx(
                    &mut tx,
                    workspace_id,
                    RelationActor {
                        user_id: actor_id,
                        service_account: Some((service_account_id, &service_account_name)),
                    },
                    task_id,
                    request_id,
                    now,
                )
                .await?;
            }
            if changed.rows_affected() > 0 {
                record_principal_mutation(
                    &mut tx,
                    workspace_id,
                    actor_id,
                    Some((service_account_id, &service_account_name)),
                    if was_deleted {
                        "task.restored"
                    } else {
                        "task.updated"
                    },
                    "task",
                    task_id,
                    request_id,
                    now,
                )
                .await?;
            }
            let resumed = sqlx::query("UPDATE github_issue_links SET sync_paused = 0, pull_state = ? WHERE task_id = ? AND (sync_paused != 0 OR pull_state IS NOT ?)")
                    .bind(issue.state)
                    .bind(task_id.to_string())
                    .bind(issue.state)
                    .execute(&mut *tx)
                    .await?;
            if resumed.rows_affected() > 0 && changed.rows_affected() == 0 {
                record_principal_mutation(
                    &mut tx,
                    workspace_id,
                    actor_id,
                    Some((service_account_id, &service_account_name)),
                    "github.work_item.link.updated",
                    "task",
                    task_id,
                    request_id,
                    now,
                )
                .await?;
            }
            tx.commit().await?;
            return Ok(task_id);
        }
        let status_id: String = sqlx::query_scalar(
            "SELECT id FROM task_statuses WHERE workspace_id = ? AND project_id = ? AND category <> 'duplicate' ORDER BY CASE WHEN category = ? THEN 0 WHEN category = 'unstarted' THEN 1 ELSE 2 END, position, id LIMIT 1",
        ).bind(workspace_id.to_string()).bind(issue.project_id.to_string())
            .bind(status_category)
            .fetch_optional(&mut *tx).await?.ok_or(TaskError::NotFound)?;
        let label_id = if let Some(id) = sqlx::query_scalar::<_, String>(
            "SELECT id FROM labels WHERE workspace_id = ? AND lower(name) = 'github' ORDER BY id LIMIT 1",
        ).bind(workspace_id.to_string()).fetch_optional(&mut *tx).await? {
            id
        } else {
            let id = Id::new_v7();
            sqlx::query("INSERT INTO labels (id, workspace_id, name, color, version, created_at, updated_at) VALUES (?, ?, 'GitHub', '#6e7681', 0, ?, ?)")
                .bind(id.to_string()).bind(workspace_id.to_string()).bind(now.as_millis()).bind(now.as_millis()).execute(&mut *tx).await?;
            record_principal_mutation(&mut tx, workspace_id, actor_id,
                Some((service_account_id, &service_account_name)),
                "label.created", "label", id, request_id, now).await?;
            id.to_string()
        };
        let task_id = Id::new_v7();
        let position: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(position) + 1, 0) FROM tasks WHERE workspace_id = ? AND project_id = ? AND status_id = ? AND deleted_at IS NULL",
        ).bind(workspace_id.to_string()).bind(issue.project_id.to_string()).bind(&status_id).fetch_one(&mut *tx).await?;
        sqlx::query("INSERT INTO tasks (id, workspace_id, project_id, status_id, title, description, source_url, priority, position, creator_id, creator_service_account_id, version, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, 'none', ?, ?, ?, 0, ?, ?)")
            .bind(task_id.to_string()).bind(workspace_id.to_string()).bind(issue.project_id.to_string())
            .bind(&status_id).bind(&issue.title).bind(&issue.description).bind(&issue_url).bind(position)
            .bind(actor_id.to_string()).bind(service_account_id.to_string())
            .bind(now.as_millis()).bind(now.as_millis()).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO task_labels (task_id, label_id) VALUES (?, ?)")
            .bind(task_id.to_string())
            .bind(&label_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO github_issue_links (workspace_id, repository, issue_number, task_id, kind, pull_state) VALUES (?, ?, ?, ?, ?, ?)")
            .bind(workspace_id.to_string()).bind(&issue.repository).bind(issue.number)
            .bind(task_id.to_string()).bind(issue.kind).bind(issue.state).execute(&mut *tx).await?;
        record_principal_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            Some((service_account_id, &service_account_name)),
            "task.created",
            "task",
            task_id,
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(task_id)
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
        let status_id =
            task_relations::default_status_id_in_tx(&mut tx, workspace_id, input.project_id)
                .await?
                .ok_or(TaskError::NotFound)?;

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
        sqlx::query(
            "INSERT INTO tasks (id, workspace_id, project_id, status_id, title, description, source_url, priority, position, creator_id, creator_service_account_id, version, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, 'none', ?, ?, ?, 0, ?, ?)",
        )
        .bind(task_id.to_string()).bind(workspace_id.to_string()).bind(input.project_id.to_string()).bind(status_id.to_string())
        .bind(&input.title).bind(&input.description).bind(&input.source_url).bind(position).bind(actor_id.to_string()).bind(service_account_id.map(|id| id.to_string())).bind(now.as_millis()).bind(now.as_millis())
        .execute(&mut *tx).await?;
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
        let task =
            update_task_in_tx(&mut tx, workspace_id, actor_id, update, request_id, now).await?;
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
            records.push(
                update_task_in_tx(&mut tx, workspace_id, actor_id, update, request_id, now).await?,
            );
        }
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
        tx.commit().await?;
        Ok(TaskRecord {
            version: current.version + 1,
            deleted_at: None,
            updated_at: now,
            ..current
        })
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
        let mut query = QueryBuilder::<Sqlite>::new(format!(
            "SELECT {} FROM tasks JOIN projects ON projects.id = tasks.project_id WHERE tasks.workspace_id = ",
            task_columns()
        ));
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
            "SELECT id, workspace_id, task_id, author_id, parent_id, body, version, created_at, updated_at \
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
        body: String,
        mentioned_user_ids: Vec<Id>,
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
        sqlx::query("INSERT INTO task_comments (id, workspace_id, task_id, author_id, parent_id, body, version, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, 0, ?, ?)")
            .bind(id.to_string()).bind(workspace_id.to_string()).bind(task_id.to_string()).bind(actor_id.to_string()).bind(parent_id.map(|id| id.to_string())).bind(&body).bind(now.as_millis()).bind(now.as_millis()).execute(&mut *tx).await?;
        notify_users(
            &mut tx,
            workspace_id,
            actor_id,
            NotificationRequest {
                kind: "comment_mentioned",
                task_id,
                comment_id: Some(id),
                recipients: &mentioned_user_ids,
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
            body,
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
        body: String,
        expected_version: u64,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<CommentRecord, TaskError> {
        let mut tx = self.database.immediate_transaction().await?;
        require_task_tx(&mut tx, workspace_id, task_id, actor_id).await?;
        let current = comment_in_tx(&mut tx, workspace_id, task_id, comment_id).await?;
        check_version(expected_version, current.version, &current)?;
        sqlx::query("UPDATE task_comments SET body = ?, version = version + 1, updated_at = ? WHERE id = ? AND workspace_id = ? AND task_id = ? AND version = ?")
            .bind(&body).bind(now.as_millis()).bind(comment_id.to_string()).bind(workspace_id.to_string()).bind(task_id.to_string()).bind(expected_version as i64).execute(&mut *tx).await?;
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
            body,
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

pub(super) async fn update_task_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    actor_id: Id,
    update: &TaskUpdate,
    request_id: &str,
    now: TimestampMillis,
) -> Result<TaskRecord, TaskError> {
    let current = task_in_tx(tx, workspace_id, update.id, false).await?;
    check_version(update.expected_version, current.version, &current)?;
    if update.changes.duplicate_of_id.is_some() && update.changes.status_id.is_some() {
        return Err(TaskError::Invalid {
            field: "duplicate_of_id",
        });
    }
    if update.changes.title.is_some() || update.changes.description.is_some() {
        let linked: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM github_issue_links WHERE workspace_id = ? AND task_id = ?)")
            .bind(workspace_id.to_string())
            .bind(update.id.to_string())
            .fetch_one(&mut **tx)
            .await?;
        if linked {
            return Err(TaskError::GithubContentReadOnly);
        }
    }
    let project_id = update.changes.project_id.unwrap_or(current.project_id);
    // Only these changes can move the task into or out of the Duplicate status.
    let duplicate_may_change = update.changes.duplicate_of_id.is_some()
        || update.changes.status_id.is_some()
        || project_id != current.project_id;
    let was_duplicate = duplicate_may_change
        && task_relations::status_category_in_tx(tx, workspace_id, current.status_id).await?
            == task_relations::DUPLICATE;
    // The Duplicate status is entered only through duplicate_of_id and follows project moves.
    let status_id = match update.changes.duplicate_of_id {
        Some(Some(_)) => {
            task_relations::duplicate_status_id_in_tx(tx, workspace_id, project_id).await?
        }
        Some(None) if was_duplicate => {
            task_relations::restore_status_id_in_tx(tx, workspace_id, update.id, project_id).await?
        }
        Some(None) => current.status_id,
        None => match update.changes.status_id {
            Some(status_id) if status_id != current.status_id => {
                if task_relations::status_category_in_tx(tx, workspace_id, status_id).await?
                    == task_relations::DUPLICATE
                {
                    return Err(TaskError::Invalid { field: "status_id" });
                }
                status_id
            }
            Some(status_id) => status_id,
            None if was_duplicate && project_id != current.project_id => {
                task_relations::duplicate_status_id_in_tx(tx, workspace_id, project_id).await?
            }
            None => current.status_id,
        },
    };
    validate_project_status(tx, workspace_id, project_id, status_id).await?;
    if let Some(assignees) = &update.changes.assignee_ids {
        validate_assignees(tx, workspace_id, assignees).await?;
    }
    if let Some(labels) = &update.changes.label_ids {
        validate_labels(tx, workspace_id, labels).await?;
    }
    let title = update
        .changes
        .title
        .clone()
        .unwrap_or(current.title.clone());
    let description = update
        .changes
        .description
        .clone()
        .unwrap_or(current.description.clone());
    let source_url = update
        .changes
        .source_url
        .clone()
        .unwrap_or(current.source_url.clone());
    let priority = update
        .changes
        .priority
        .clone()
        .unwrap_or(current.priority.clone());
    let position = update.changes.position.unwrap_or(current.position);
    let due_at = update.changes.due_at.unwrap_or(current.due_at);
    let due_start_at =
        if matches!(update.changes.due_at, Some(None)) && update.changes.due_start_at.is_none() {
            None
        } else {
            update.changes.due_start_at.unwrap_or(current.due_start_at)
        };
    if due_start_at.is_some_and(|start| due_at.is_none_or(|end| start > end)) {
        return Err(TaskError::Invalid {
            field: "due_start_at",
        });
    }
    sqlx::query("UPDATE tasks SET project_id = ?, status_id = ?, title = ?, description = ?, source_url = ?, priority = ?, position = ?, due_start_at = ?, due_at = ?, version = version + 1, updated_at = ? WHERE id = ? AND workspace_id = ? AND deleted_at IS NULL AND version = ?")
        .bind(project_id.to_string()).bind(status_id.to_string()).bind(&title).bind(&description).bind(&source_url).bind(&priority).bind(position).bind(due_start_at.map(TimestampMillis::as_millis)).bind(due_at.map(TimestampMillis::as_millis)).bind(now.as_millis()).bind(update.id.to_string()).bind(workspace_id.to_string()).bind(update.expected_version as i64).execute(&mut **tx).await?;
    let actor = RelationActor {
        user_id: actor_id,
        service_account: None,
    };
    match update.changes.duplicate_of_id {
        Some(Some(target_id)) => {
            task_relations::mark_duplicate_in_tx(
                tx,
                workspace_id,
                actor,
                update.id,
                current.status_id,
                target_id,
                request_id,
                now,
            )
            .await?;
        }
        Some(None) => {
            task_relations::delete_duplicate_relation_in_tx(
                tx,
                workspace_id,
                actor,
                update.id,
                request_id,
                now,
            )
            .await?;
        }
        None if was_duplicate
            && status_id != current.status_id
            && update.changes.status_id.is_some() =>
        {
            task_relations::delete_duplicate_relation_in_tx(
                tx,
                workspace_id,
                actor,
                update.id,
                request_id,
                now,
            )
            .await?;
        }
        None => {}
    }
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
    let duplicate_of = if duplicate_may_change {
        duplicate_of_in_tx(tx, update.id).await?
    } else {
        current.duplicate_of.clone()
    };
    Ok(TaskRecord {
        project_id,
        status_id,
        title,
        description,
        source_url,
        priority,
        position,
        due_start_at,
        due_at,
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
        duplicate_of,
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
        "tasks:w={workspace_id}:p={}:s={}:a={}:u={}:l={}:r={}:q={}:v={}:sort={:?}:order={:?}",
        filter
            .project_id
            .map_or_else(String::new, |id| id.to_string()),
        filter
            .status_id
            .map_or_else(String::new, |id| id.to_string()),
        filter
            .assignee_id
            .map_or_else(String::new, |id| id.to_string()),
        filter.unassigned,
        filter
            .label_id
            .map_or_else(String::new, |id| id.to_string()),
        filter.priority.as_deref().unwrap_or(""),
        filter.search.as_deref().unwrap_or(""),
        filter.view.as_deref().unwrap_or(""),
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
        TaskSort::Position | TaskSort::Priority | TaskSort::CreatedAt | TaskSort::UpdatedAt => {
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

fn escape_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
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

pub(super) async fn require_access_tx(
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
    if unique.len() != request.recipients.len() {
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

async fn insert_default_statuses(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    project_id: Id,
    now: TimestampMillis,
) -> Result<(), TaskError> {
    for (position, (name, color, category)) in DEFAULT_STATUSES.into_iter().enumerate() {
        sqlx::query("INSERT INTO task_statuses (id, workspace_id, project_id, name, description, color, category, position, version, created_at, updated_at) VALUES (?, ?, ?, ?, '', ?, ?, ?, 0, ?, ?)")
            .bind(Id::new_v7().to_string()).bind(workspace_id.to_string()).bind(project_id.to_string()).bind(name).bind(color).bind(category.as_str()).bind(position as i64).bind(now.as_millis()).bind(now.as_millis()).execute(&mut **tx).await?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn record_mutation(
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

async fn github_service_account_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    actor_id: Id,
    now: TimestampMillis,
) -> Result<(Id, String), TaskError> {
    if let Some((id, name)) = sqlx::query_as::<_, (String, String)>(
        "SELECT id, name FROM service_accounts WHERE workspace_id = ? AND lower(name) = 'github' AND disabled_at IS NULL",
    ).bind(workspace_id.to_string()).fetch_optional(&mut **tx).await? {
        return Ok((parse_id(id)?, name));
    }
    let id = Id::new_v7();
    sqlx::query("INSERT INTO service_accounts (id, workspace_id, name, created_by, created_at) VALUES (?, ?, 'GitHub', ?, ?)")
        .bind(id.to_string()).bind(workspace_id.to_string()).bind(actor_id.to_string())
        .bind(now.as_millis()).execute(&mut **tx).await?;
    Ok((id, "GitHub".to_owned()))
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

pub(super) async fn task_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    task_id: Id,
    deleted: bool,
) -> Result<TaskRecord, TaskError> {
    let sql = format!(
        "SELECT {} FROM tasks JOIN projects ON projects.id = tasks.project_id WHERE tasks.id = ? AND tasks.workspace_id = ? \
         AND ((? = 1 AND tasks.deleted_at IS NOT NULL) OR (? = 0 AND tasks.deleted_at IS NULL)) AND (? = 1 OR projects.deleted_at IS NULL)",
        task_columns()
    );
    let row = sqlx::query(&sql)
        .bind(task_id.to_string())
        .bind(workspace_id.to_string())
        .bind(i64::from(deleted))
        .bind(i64::from(deleted))
        .bind(i64::from(deleted))
        .fetch_optional(&mut **tx)
        .await?
        .ok_or(TaskError::NotFound)?;
    task_from_row_tx(tx, row).await
}

async fn comment_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    task_id: Id,
    comment_id: Id,
) -> Result<CommentRecord, TaskError> {
    let row = sqlx::query("SELECT id, workspace_id, task_id, author_id, parent_id, body, version, created_at, updated_at FROM task_comments WHERE id = ? AND workspace_id = ? AND task_id = ?")
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

/// Correlated subquery rendering the live duplicate target of `task_id_sql` as a JSON object,
/// or NULL when the task is not a duplicate or its target (or target's project) is in the trash.
fn duplicate_of_subquery(task_id_sql: &str) -> String {
    format!(
        "(SELECT json_object('id', canonical.id, 'project_id', canonical.project_id, 'title', canonical.title) \
         FROM task_relations AS duplicate_relation \
         JOIN tasks AS canonical ON canonical.id = duplicate_relation.related_task_id \
         JOIN projects AS canonical_project ON canonical_project.id = canonical.project_id \
         WHERE duplicate_relation.task_id = {task_id_sql} AND duplicate_relation.type = 'duplicate' \
         AND canonical.deleted_at IS NULL AND canonical_project.deleted_at IS NULL)"
    )
}

/// The column list every `TaskRecord` query selects (decoded by `task_record_from_row`).
fn task_columns() -> String {
    format!(
        "tasks.id, tasks.workspace_id, tasks.project_id, tasks.status_id, tasks.title, \
         tasks.description, tasks.source_url, tasks.priority, tasks.position, tasks.creator_id, \
         tasks.creator_service_account_id, \
         (SELECT name FROM service_accounts WHERE id = tasks.creator_service_account_id) AS creator_service_account_name, \
         tasks.due_start_at, tasks.due_at, tasks.version, tasks.deleted_at, tasks.created_at, tasks.updated_at, \
         {} AS duplicate_of_json, \
         EXISTS (SELECT 1 FROM task_relations AS blocker_relation \
                 JOIN tasks AS blocker ON blocker.id = blocker_relation.task_id \
                 JOIN projects AS blocker_project ON blocker_project.id = blocker.project_id \
                 JOIN task_statuses AS blocker_status ON blocker_status.id = blocker.status_id \
                 WHERE blocker_relation.related_task_id = tasks.id AND blocker_relation.type = 'blocks' \
                 AND blocker.deleted_at IS NULL AND blocker_project.deleted_at IS NULL \
                 AND blocker_status.category NOT IN ('completed', 'cancelled', 'duplicate')) AS blocked",
        duplicate_of_subquery("tasks.id")
    )
}

fn parse_task_ref(value: Option<String>) -> Result<Option<TaskRef>, TaskError> {
    value
        .map(|json| serde_json::from_str(&json).map_err(|_| TaskError::Conflict))
        .transpose()
}

async fn duplicate_of_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    task_id: Id,
) -> Result<Option<TaskRef>, TaskError> {
    let sql = format!("SELECT {}", duplicate_of_subquery("?"));
    let value: Option<String> = sqlx::query_scalar(&sql)
        .bind(task_id.to_string())
        .fetch_one(&mut **tx)
        .await?;
    parse_task_ref(value)
}

fn task_record_from_row(
    row: sqlx::sqlite::SqliteRow,
    id: Id,
    assignee_ids: Vec<Id>,
    label_ids: Vec<Id>,
) -> Result<TaskRecord, TaskError> {
    let creator_service_account_id = row
        .get::<Option<String>, _>("creator_service_account_id")
        .map(parse_id)
        .transpose()?;
    Ok(TaskRecord {
        id,
        workspace_id: parse_id(row.get("workspace_id"))?,
        project_id: parse_id(row.get("project_id"))?,
        status_id: parse_id(row.get("status_id"))?,
        title: row.get("title"),
        description: row.get("description"),
        source_url: row.get("source_url"),
        priority: row.get("priority"),
        position: row.get("position"),
        creator_id: if creator_service_account_id.is_some() {
            None
        } else {
            Some(parse_id(row.get("creator_id"))?)
        },
        creator_service_account_id,
        creator_service_account_name: row.get("creator_service_account_name"),
        assignee_ids,
        label_ids,
        due_start_at: row
            .get::<Option<i64>, _>("due_start_at")
            .map(TimestampMillis::from_millis),
        due_at: row
            .get::<Option<i64>, _>("due_at")
            .map(TimestampMillis::from_millis),
        version: parse_version(row.get("version"))?,
        deleted_at: row
            .get::<Option<i64>, _>("deleted_at")
            .map(TimestampMillis::from_millis),
        created_at: TimestampMillis::from_millis(row.get("created_at")),
        updated_at: TimestampMillis::from_millis(row.get("updated_at")),
        duplicate_of: parse_task_ref(row.get("duplicate_of_json"))?,
        blocked: row.get::<bool, _>("blocked"),
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
        body: row.get("body"),
        version: parse_version(row.get("version"))?,
        created_at: TimestampMillis::from_millis(row.get("created_at")),
        updated_at: TimestampMillis::from_millis(row.get("updated_at")),
    })
}

pub(super) fn parse_id(value: String) -> Result<Id, TaskError> {
    value.parse().map_err(|_| TaskError::Conflict)
}
fn parse_version(value: i64) -> Result<u64, TaskError> {
    u64::try_from(value).map_err(|_| TaskError::Conflict)
}
