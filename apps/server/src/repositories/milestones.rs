//! Milestones (spec `2026-10-08-project-details-milestones-design.md`): the time-bound deliverable
//! of a project, its health updates, and the hidden Docs pages that hold the description of a
//! project or a milestone.

use orbit_domain::Actor;
use orbit_platform::{Id, TimestampMillis};
use serde::Serialize;
use sqlx::{Row, Sqlite, Transaction};
use utoipa::ToSchema;

use crate::push::NotificationKind;

use super::intake;
use super::tasks::{
    Page, TaskError, TaskRepository, check_version, optional_id, parse_id, parse_version,
    project_in_tx, record_mutation, require_access, require_access_tx, require_project,
    require_project_tx,
};

pub const MILESTONE_STATUSES: [&str; 4] = ["planned", "in_progress", "completed", "cancelled"];
pub const HEALTH_VALUES: [&str; 3] = ["on_track", "at_risk", "off_track"];

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct MilestoneRecord {
    #[schema(value_type = String)]
    pub id: Id,
    #[schema(value_type = String)]
    pub workspace_id: Id,
    #[schema(value_type = String)]
    pub project_id: Id,
    pub name: String,
    /// `planned`, `in_progress`, `completed` or `cancelled`; set by a person, never automatically.
    pub status: String,
    #[schema(value_type = Option<String>, format = DateTime, required = true)]
    pub start_at: Option<TimestampMillis>,
    #[schema(value_type = Option<String>, format = DateTime, required = true)]
    pub target_at: Option<TimestampMillis>,
    /// The hidden Docs page that holds the description; null until someone writes one.
    #[schema(value_type = Option<String>, required = true)]
    pub description_page_id: Option<Id>,
    pub position: i64,
    #[schema(value_type = Option<String>, format = DateTime, required = true)]
    pub completed_at: Option<TimestampMillis>,
    /// Live tasks of the milestone, without cancelled and duplicate ones.
    pub task_count: i64,
    /// Live tasks of the milestone in a completed status.
    pub task_done_count: i64,
    /// The health of the most recent update: `on_track`, `at_risk` or `off_track`.
    #[schema(required = true)]
    pub health: Option<String>,
    pub version: u64,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: TimestampMillis,
    #[schema(value_type = String, format = DateTime)]
    pub updated_at: TimestampMillis,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct MilestoneUpdateRecord {
    #[schema(value_type = String)]
    pub id: Id,
    #[schema(value_type = String)]
    pub workspace_id: Id,
    #[schema(value_type = String)]
    pub milestone_id: Id,
    #[schema(value_type = String)]
    pub author_id: Id,
    pub health: String,
    pub body: String,
    /// Whether the caller may edit this update.
    pub can_edit: bool,
    /// Whether the caller may delete this update.
    pub can_delete: bool,
    pub version: u64,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: TimestampMillis,
    #[schema(value_type = String, format = DateTime)]
    pub updated_at: TimestampMillis,
}

/// The id of an owned page.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct OwnedPageRecord {
    #[schema(value_type = String)]
    pub page_id: Id,
}

#[derive(Clone, Debug)]
pub struct CreateMilestone {
    pub name: String,
    pub status: String,
    pub start_at: Option<TimestampMillis>,
    pub target_at: Option<TimestampMillis>,
}

/// `None` keeps the stored value; `Some(None)` clears a date.
#[derive(Clone, Debug, Default)]
pub struct MilestoneChanges {
    pub name: Option<String>,
    pub status: Option<String>,
    pub start_at: Option<Option<TimestampMillis>>,
    pub target_at: Option<Option<TimestampMillis>>,
}

const MILESTONE_COLUMNS: &str = "milestones.id, milestones.workspace_id, milestones.project_id, \
     milestones.name, milestones.status, milestones.start_at, milestones.target_at, \
     milestones.description_page_id, milestones.position, milestones.completed_at, \
     milestones.version, milestones.created_at, milestones.updated_at, \
     (SELECT COUNT(*) FROM tasks JOIN task_statuses ON task_statuses.id = tasks.status_id \
      WHERE tasks.milestone_id = milestones.id AND tasks.deleted_at IS NULL \
      AND task_statuses.category NOT IN ('cancelled', 'duplicate', 'triage')) AS task_count, \
     (SELECT COUNT(*) FROM tasks JOIN task_statuses ON task_statuses.id = tasks.status_id \
      WHERE tasks.milestone_id = milestones.id AND tasks.deleted_at IS NULL \
      AND task_statuses.category = 'completed') AS task_done_count, \
     (SELECT health FROM milestone_updates WHERE milestone_updates.milestone_id = milestones.id \
      ORDER BY created_at DESC, id DESC LIMIT 1) AS health";

const UPDATE_COLUMNS: &str =
    "id, workspace_id, milestone_id, author_id, health, body, version, created_at, updated_at";

impl TaskRepository {
    /// The milestones of one project, or of every live project of the workspace (the roadmap).
    pub async fn milestones(
        &self,
        workspace_id: Id,
        actor_id: Id,
        project_id: Option<Id>,
    ) -> Result<Page<MilestoneRecord>, TaskError> {
        let pool = self.database().pool();
        match project_id {
            Some(project_id) => require_project(pool, workspace_id, project_id, actor_id).await?,
            None => {
                require_access(pool, workspace_id, actor_id).await?;
            }
        }
        let items = sqlx::query(&format!(
            "SELECT {MILESTONE_COLUMNS} FROM milestones \
             JOIN projects ON projects.id = milestones.project_id \
             WHERE milestones.workspace_id = ? AND projects.deleted_at IS NULL \
             AND (? IS NULL OR milestones.project_id = ?) \
             ORDER BY milestones.project_id, milestones.position, milestones.id"
        ))
        .bind(workspace_id.to_string())
        .bind(project_id.map(|id| id.to_string()))
        .bind(project_id.map(|id| id.to_string()))
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(milestone_from_row)
        .collect::<Result<Vec<_>, _>>()?;
        Ok(Page {
            items,
            next_cursor: None,
        })
    }

    pub async fn create_milestone(
        &self,
        workspace_id: Id,
        project_id: Id,
        actor_id: Id,
        input: CreateMilestone,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<MilestoneRecord, TaskError> {
        validate_status(&input.status)?;
        validate_dates(input.start_at, input.target_at)?;
        let id = Id::new_v7();
        let mut tx = self.database().immediate_transaction().await?;
        require_project_tx(&mut tx, workspace_id, project_id, actor_id).await?;
        sqlx::query(
            "INSERT INTO milestones (id, workspace_id, project_id, name, status, start_at, target_at, \
             position, completed_at, version, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, \
             (SELECT COALESCE(MAX(position) + 1, 0) FROM milestones WHERE project_id = ?), ?, 0, ?, ?)",
        )
        .bind(id.to_string())
        .bind(workspace_id.to_string())
        .bind(project_id.to_string())
        .bind(&input.name)
        .bind(&input.status)
        .bind(input.start_at.map(TimestampMillis::as_millis))
        .bind(input.target_at.map(TimestampMillis::as_millis))
        .bind(project_id.to_string())
        .bind((input.status == "completed").then_some(now.as_millis()))
        .bind(now.as_millis())
        .bind(now.as_millis())
        .execute(&mut *tx)
        .await?;
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "milestone.created",
            "milestone",
            id,
            request_id,
            now,
        )
        .await?;
        let record = milestone_in_tx(&mut tx, workspace_id, project_id, id).await?;
        tx.commit().await?;
        Ok(record)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update_milestone(
        &self,
        workspace_id: Id,
        project_id: Id,
        milestone_id: Id,
        actor_id: Id,
        changes: MilestoneChanges,
        expected_version: u64,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<MilestoneRecord, TaskError> {
        let mut tx = self.database().immediate_transaction().await?;
        require_project_tx(&mut tx, workspace_id, project_id, actor_id).await?;
        let current = milestone_in_tx(&mut tx, workspace_id, project_id, milestone_id).await?;
        check_version(expected_version, current.version, &current)?;
        let name = changes.name.unwrap_or(current.name);
        let status = changes.status.unwrap_or(current.status.clone());
        validate_status(&status)?;
        let start_at = changes.start_at.unwrap_or(current.start_at);
        let target_at = changes.target_at.unwrap_or(current.target_at);
        validate_dates(start_at, target_at)?;
        // The completion time is the time of the move to `completed`; any other status clears it.
        let completed_at = match (status.as_str(), current.status.as_str()) {
            ("completed", "completed") => current.completed_at,
            ("completed", _) => Some(now),
            _ => None,
        };
        let updated = sqlx::query(
            "UPDATE milestones SET name = ?, status = ?, start_at = ?, target_at = ?, completed_at = ?, \
             version = version + 1, updated_at = ? WHERE id = ? AND project_id = ? AND version = ?",
        )
        .bind(&name)
        .bind(&status)
        .bind(start_at.map(TimestampMillis::as_millis))
        .bind(target_at.map(TimestampMillis::as_millis))
        .bind(completed_at.map(TimestampMillis::as_millis))
        .bind(now.as_millis())
        .bind(milestone_id.to_string())
        .bind(project_id.to_string())
        .bind(expected_version as i64)
        .execute(&mut *tx)
        .await?;
        if updated.rows_affected() != 1 {
            return Err(TaskError::Conflict);
        }
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "milestone.updated",
            "milestone",
            milestone_id,
            request_id,
            now,
        )
        .await?;
        let record = milestone_in_tx(&mut tx, workspace_id, project_id, milestone_id).await?;
        tx.commit().await?;
        Ok(record)
    }

    /// Milestones have no trash. The tasks stay and lose the milestone (FK `SET NULL`); the
    /// description page and the updates go with it.
    #[allow(clippy::too_many_arguments)]
    pub async fn delete_milestone(
        &self,
        workspace_id: Id,
        project_id: Id,
        milestone_id: Id,
        actor_id: Id,
        expected_version: u64,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), TaskError> {
        let mut tx = self.database().immediate_transaction().await?;
        require_project_tx(&mut tx, workspace_id, project_id, actor_id).await?;
        let current = milestone_in_tx(&mut tx, workspace_id, project_id, milestone_id).await?;
        check_version(expected_version, current.version, &current)?;
        sqlx::query("DELETE FROM milestones WHERE id = ? AND project_id = ?")
            .bind(milestone_id.to_string())
            .bind(project_id.to_string())
            .execute(&mut *tx)
            .await?;
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "milestone.deleted",
            "milestone",
            milestone_id,
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// The overview page of a project; created on the first call.
    pub async fn project_overview_page(
        &self,
        workspace_id: Id,
        project_id: Id,
        actor_id: Id,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<OwnedPageRecord, TaskError> {
        let mut tx = self.database().immediate_transaction().await?;
        require_access_tx(&mut tx, workspace_id, actor_id).await?;
        let project = project_in_tx(&mut tx, workspace_id, project_id, false).await?;
        if let Some(page_id) = project.overview_page_id {
            return Ok(OwnedPageRecord { page_id });
        }
        let page_id = insert_owned_page(&mut tx, workspace_id, actor_id, "project", now).await?;
        // The description is not a versioned field of the project: its version stays.
        sqlx::query("UPDATE projects SET overview_page_id = ? WHERE id = ?")
            .bind(page_id.to_string())
            .bind(project_id.to_string())
            .execute(&mut *tx)
            .await?;
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "project.overview_created",
            "project",
            project_id,
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(OwnedPageRecord { page_id })
    }

    /// The description page of a milestone; created on the first call.
    pub async fn milestone_description_page(
        &self,
        workspace_id: Id,
        project_id: Id,
        milestone_id: Id,
        actor_id: Id,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<OwnedPageRecord, TaskError> {
        let mut tx = self.database().immediate_transaction().await?;
        require_project_tx(&mut tx, workspace_id, project_id, actor_id).await?;
        let milestone = milestone_in_tx(&mut tx, workspace_id, project_id, milestone_id).await?;
        if let Some(page_id) = milestone.description_page_id {
            return Ok(OwnedPageRecord { page_id });
        }
        let page_id = insert_owned_page(&mut tx, workspace_id, actor_id, "milestone", now).await?;
        sqlx::query("UPDATE milestones SET description_page_id = ? WHERE id = ?")
            .bind(page_id.to_string())
            .bind(milestone_id.to_string())
            .execute(&mut *tx)
            .await?;
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "milestone.description_created",
            "milestone",
            milestone_id,
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(OwnedPageRecord { page_id })
    }

    /// The update feed of a milestone, newest first.
    pub async fn milestone_updates(
        &self,
        workspace_id: Id,
        project_id: Id,
        milestone_id: Id,
        actor_id: Id,
    ) -> Result<Page<MilestoneUpdateRecord>, TaskError> {
        let mut tx = self.database().transaction().await?;
        let actor = require_access_tx(&mut tx, workspace_id, actor_id).await?;
        milestone_in_tx(&mut tx, workspace_id, project_id, milestone_id).await?;
        let items = sqlx::query(&format!(
            "SELECT {UPDATE_COLUMNS} FROM milestone_updates WHERE milestone_id = ? \
             ORDER BY created_at DESC, id DESC"
        ))
        .bind(milestone_id.to_string())
        .fetch_all(&mut *tx)
        .await?
        .into_iter()
        .map(|row| update_from_row(row, actor))
        .collect::<Result<Vec<_>, _>>()?;
        Ok(Page {
            items,
            next_cursor: None,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn create_milestone_update(
        &self,
        workspace_id: Id,
        project_id: Id,
        milestone_id: Id,
        actor_id: Id,
        health: String,
        body: String,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<MilestoneUpdateRecord, TaskError> {
        validate_health(&health)?;
        let id = Id::new_v7();
        let mut tx = self.database().immediate_transaction().await?;
        let actor = require_access_tx(&mut tx, workspace_id, actor_id).await?;
        milestone_in_tx(&mut tx, workspace_id, project_id, milestone_id).await?;
        sqlx::query(
            "INSERT INTO milestone_updates (id, workspace_id, milestone_id, author_id, health, body, \
             version, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, 0, ?, ?)",
        )
        .bind(id.to_string())
        .bind(workspace_id.to_string())
        .bind(milestone_id.to_string())
        .bind(actor_id.to_string())
        .bind(&health)
        .bind(&body)
        .bind(now.as_millis())
        .bind(now.as_millis())
        .execute(&mut *tx)
        .await?;
        // One inbox row for each update, to the project lead and members, not to the author.
        for recipient in intake::project_people_in_tx(&mut tx, project_id)
            .await?
            .into_iter()
            .filter(|recipient| *recipient != actor_id)
        {
            sqlx::query(
                "INSERT INTO notifications (id, workspace_id, recipient_user_id, actor_user_id, kind, \
                 milestone_id, dedupe_key, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(Id::new_v7().to_string())
            .bind(workspace_id.to_string())
            .bind(recipient.to_string())
            .bind(actor_id.to_string())
            .bind(NotificationKind::MilestoneUpdatePosted.as_str())
            .bind(milestone_id.to_string())
            .bind(format!("{workspace_id}:{recipient}:milestone_update:{id}"))
            .bind(now.as_millis())
            .execute(&mut *tx)
            .await?;
        }
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "milestone_update.created",
            "milestone_update",
            id,
            request_id,
            now,
        )
        .await?;
        let record = update_in_tx(&mut tx, milestone_id, id, actor).await?;
        tx.commit().await?;
        Ok(record)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update_milestone_update(
        &self,
        workspace_id: Id,
        project_id: Id,
        milestone_id: Id,
        update_id: Id,
        actor_id: Id,
        health: String,
        body: String,
        expected_version: u64,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<MilestoneUpdateRecord, TaskError> {
        validate_health(&health)?;
        let mut tx = self.database().immediate_transaction().await?;
        let actor = require_access_tx(&mut tx, workspace_id, actor_id).await?;
        milestone_in_tx(&mut tx, workspace_id, project_id, milestone_id).await?;
        let current = update_in_tx(&mut tx, milestone_id, update_id, actor).await?;
        if !current.can_edit {
            return Err(TaskError::Forbidden);
        }
        check_version(expected_version, current.version, &current)?;
        sqlx::query(
            "UPDATE milestone_updates SET health = ?, body = ?, version = version + 1, updated_at = ? \
             WHERE id = ? AND version = ?",
        )
        .bind(&health)
        .bind(&body)
        .bind(now.as_millis())
        .bind(update_id.to_string())
        .bind(expected_version as i64)
        .execute(&mut *tx)
        .await?;
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "milestone_update.updated",
            "milestone_update",
            update_id,
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(MilestoneUpdateRecord {
            health,
            body,
            version: current.version + 1,
            updated_at: now,
            ..current
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn delete_milestone_update(
        &self,
        workspace_id: Id,
        project_id: Id,
        milestone_id: Id,
        update_id: Id,
        actor_id: Id,
        expected_version: u64,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), TaskError> {
        let mut tx = self.database().immediate_transaction().await?;
        let actor = require_access_tx(&mut tx, workspace_id, actor_id).await?;
        milestone_in_tx(&mut tx, workspace_id, project_id, milestone_id).await?;
        let current = update_in_tx(&mut tx, milestone_id, update_id, actor).await?;
        if !current.can_delete {
            return Err(TaskError::Forbidden);
        }
        check_version(expected_version, current.version, &current)?;
        sqlx::query("DELETE FROM milestone_updates WHERE id = ?")
            .bind(update_id.to_string())
            .execute(&mut *tx)
            .await?;
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "milestone_update.deleted",
            "milestone_update",
            update_id,
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }
}

/// A task's milestone must be a milestone of the task's project.
pub(super) async fn validate_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    project_id: Id,
    milestone_id: Id,
) -> Result<(), TaskError> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM milestones WHERE id = ? AND project_id = ?)",
    )
    .bind(milestone_id.to_string())
    .bind(project_id.to_string())
    .fetch_one(&mut **tx)
    .await?;
    if exists {
        Ok(())
    } else {
        Err(TaskError::Invalid {
            field: "milestone_id",
        })
    }
}

fn validate_status(status: &str) -> Result<(), TaskError> {
    if MILESTONE_STATUSES.contains(&status) {
        Ok(())
    } else {
        Err(TaskError::Invalid { field: "status" })
    }
}

fn validate_health(health: &str) -> Result<(), TaskError> {
    if HEALTH_VALUES.contains(&health) {
        Ok(())
    } else {
        Err(TaskError::Invalid { field: "health" })
    }
}

fn validate_dates(
    start_at: Option<TimestampMillis>,
    target_at: Option<TimestampMillis>,
) -> Result<(), TaskError> {
    match (start_at, target_at) {
        (Some(start), Some(target)) if target < start => {
            Err(TaskError::Invalid { field: "target_at" })
        }
        _ => Ok(()),
    }
}

/// A page with an owner kind has no space, so no Docs list returns it (see migration 0045).
async fn insert_owned_page(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    actor_id: Id,
    owner_kind: &str,
    now: TimestampMillis,
) -> Result<Id, TaskError> {
    let page_id = Id::new_v7();
    sqlx::query(
        "INSERT INTO pages (id, workspace_id, creator_id, updated_by, owner_kind, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(page_id.to_string())
    .bind(workspace_id.to_string())
    .bind(actor_id.to_string())
    .bind(actor_id.to_string())
    .bind(owner_kind)
    .bind(now.as_millis())
    .bind(now.as_millis())
    .execute(&mut **tx)
    .await?;
    Ok(page_id)
}

async fn milestone_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    project_id: Id,
    milestone_id: Id,
) -> Result<MilestoneRecord, TaskError> {
    let row = sqlx::query(&format!(
        "SELECT {MILESTONE_COLUMNS} FROM milestones \
         JOIN projects ON projects.id = milestones.project_id \
         WHERE milestones.id = ? AND milestones.workspace_id = ? AND milestones.project_id = ? \
         AND projects.deleted_at IS NULL"
    ))
    .bind(milestone_id.to_string())
    .bind(workspace_id.to_string())
    .bind(project_id.to_string())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(TaskError::NotFound)?;
    milestone_from_row(row)
}

async fn update_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    milestone_id: Id,
    update_id: Id,
    actor: Actor,
) -> Result<MilestoneUpdateRecord, TaskError> {
    let row = sqlx::query(&format!(
        "SELECT {UPDATE_COLUMNS} FROM milestone_updates WHERE id = ? AND milestone_id = ?"
    ))
    .bind(update_id.to_string())
    .bind(milestone_id.to_string())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(TaskError::NotFound)?;
    update_from_row(row, actor)
}

fn timestamp(row: &sqlx::sqlite::SqliteRow, column: &str) -> Option<TimestampMillis> {
    row.get::<Option<i64>, _>(column)
        .map(TimestampMillis::from_millis)
}

fn milestone_from_row(row: sqlx::sqlite::SqliteRow) -> Result<MilestoneRecord, TaskError> {
    Ok(MilestoneRecord {
        id: parse_id(row.get("id"))?,
        workspace_id: parse_id(row.get("workspace_id"))?,
        project_id: parse_id(row.get("project_id"))?,
        name: row.get("name"),
        status: row.get("status"),
        start_at: timestamp(&row, "start_at"),
        target_at: timestamp(&row, "target_at"),
        description_page_id: optional_id(row.get("description_page_id"))?,
        position: row.get("position"),
        completed_at: timestamp(&row, "completed_at"),
        task_count: row.get("task_count"),
        task_done_count: row.get("task_done_count"),
        health: row.get("health"),
        version: parse_version(row.get("version"))?,
        created_at: TimestampMillis::from_millis(row.get("created_at")),
        updated_at: TimestampMillis::from_millis(row.get("updated_at")),
    })
}

fn update_from_row(
    row: sqlx::sqlite::SqliteRow,
    actor: Actor,
) -> Result<MilestoneUpdateRecord, TaskError> {
    let author_id = parse_id(row.get("author_id"))?;
    Ok(MilestoneUpdateRecord {
        id: parse_id(row.get("id"))?,
        workspace_id: parse_id(row.get("workspace_id"))?,
        milestone_id: parse_id(row.get("milestone_id"))?,
        author_id,
        health: row.get("health"),
        body: row.get("body"),
        // The author, or a person who may moderate comments.
        can_edit: actor.can_delete_comment(author_id),
        can_delete: actor.can_delete_comment(author_id),
        version: parse_version(row.get("version"))?,
        created_at: TimestampMillis::from_millis(row.get("created_at")),
        updated_at: TimestampMillis::from_millis(row.get("updated_at")),
    })
}
