//! Time logged on tasks (issue #95). An entry belongs to one person: everyone logs, edits and
//! deletes their own time, and `time.manage` covers the time of other members.

use orbit_platform::{Id, Row, SqlBuilder, TimestampMillis};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{Any, Transaction};
use utoipa::ToSchema;

use super::project_access::ProjectScope;
use super::tasks::{
    CommentContent, CommentRecord, ImportedComment, TaskError, TaskRepository, check_version,
    parse_id, require_access, require_task_tx, validate_member,
};

/// The longest single entry: one day.
pub const MAX_MINUTES: u32 = 24 * 60;
/// The longest note, in characters.
pub const MAX_NOTE_CHARS: usize = 10_000;

const COLUMNS: &str = "task_time_entries.id, task_time_entries.workspace_id, task_time_entries.task_id, \
     tasks.project_id, task_time_entries.user_id, task_time_entries.minutes, task_time_entries.worked_on, \
     task_time_entries.note, task_time_entries.billable, task_time_entries.created_by, \
     task_time_entries.comment_id, task_time_entries.version, task_time_entries.created_at, \
     task_time_entries.updated_at";

#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct TimeEntryRecord {
    #[schema(value_type = String)]
    pub id: Id,
    #[schema(value_type = String)]
    pub workspace_id: Id,
    #[schema(value_type = String)]
    pub task_id: Id,
    #[schema(value_type = String)]
    pub project_id: Id,
    /// The member who did the work.
    #[schema(value_type = String)]
    pub user_id: Id,
    /// Whole minutes, 1 to 1440.
    pub minutes: u32,
    /// The calendar day of the work, `YYYY-MM-DD`, in the person's timezone.
    pub worked_on: String,
    /// Markdown; empty when the entry has no note.
    pub note: String,
    pub billable: bool,
    /// The member who logged the entry; an owner or admin can log time for someone else.
    #[schema(value_type = String)]
    pub created_by: Id,
    /// The comment the time was logged with (`/spend`); `null` otherwise or once it is deleted.
    #[schema(value_type = Option<String>, required = true)]
    pub comment_id: Option<Id>,
    pub version: u64,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: TimestampMillis,
    #[schema(value_type = String, format = DateTime)]
    pub updated_at: TimestampMillis,
    /// Whether the caller may edit and delete this entry.
    pub can_edit: bool,
}

/// The time logged on a task, oldest day first, with the totals shown on the task.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct TaskTimeEntries {
    pub items: Vec<TimeEntryRecord>,
    /// Minutes logged on the task itself.
    pub total_minutes: i64,
    /// Minutes logged on its live sub-issues, at every depth, that the caller sees.
    pub sub_issues_minutes: i64,
}

/// A new entry. `user_id` defaults to the caller, `worked_on` to today in that person's
/// timezone (UTC when their profile has none).
#[derive(Clone, Debug, Default)]
pub struct NewTimeEntry {
    pub user_id: Option<Id>,
    pub minutes: u32,
    pub worked_on: Option<String>,
    pub note: String,
    pub billable: bool,
}

/// A new entry after its fields were checked.
struct Checked {
    user_id: Id,
    minutes: u32,
    worked_on: String,
    note: String,
    billable: bool,
    comment_id: Option<Id>,
}

/// The fields an edit changes; `None` keeps the current value.
#[derive(Clone, Debug, Default)]
pub struct TimeEntryChanges {
    pub user_id: Option<Id>,
    pub minutes: Option<u32>,
    pub worked_on: Option<String>,
    pub note: Option<String>,
    pub billable: Option<bool>,
}

/// `value` as a `YYYY-MM-DD` calendar date.
pub fn parse_worked_on(value: &str) -> Result<String, TaskError> {
    let invalid = TaskError::Invalid { field: "worked_on" };
    let date: jiff::civil::Date = value.trim().parse().map_err(|_| invalid)?;
    let text = date.to_string();
    if text.len() == 10 {
        Ok(text)
    } else {
        Err(TaskError::Invalid { field: "worked_on" })
    }
}

fn check_minutes(minutes: u32) -> Result<u32, TaskError> {
    if (1..=MAX_MINUTES).contains(&minutes) {
        Ok(minutes)
    } else {
        Err(TaskError::Invalid { field: "minutes" })
    }
}

fn check_note(note: &str) -> Result<String, TaskError> {
    let note = note.trim();
    if note.chars().count() > MAX_NOTE_CHARS {
        return Err(TaskError::Invalid { field: "note" });
    }
    Ok(note.to_owned())
}

impl TaskRepository {
    /// Every entry of a task the caller sees, with the task's and its sub-issues' totals.
    pub async fn time_entries(
        &self,
        workspace_id: Id,
        task_id: Id,
        actor_id: Id,
    ) -> Result<TaskTimeEntries, TaskError> {
        let actor = require_access(self.database().pool(), workspace_id, actor_id).await?;
        self.get_task(workspace_id, task_id, actor_id).await?;
        let items = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT {COLUMNS} FROM task_time_entries JOIN tasks ON tasks.id = task_time_entries.task_id \
             WHERE task_time_entries.workspace_id = $1 AND task_time_entries.task_id = $2 \
             ORDER BY task_time_entries.worked_on, task_time_entries.created_at, task_time_entries.id"
        )))
        .bind(workspace_id.to_string())
        .bind(task_id.to_string())
        .fetch_all(self.database().pool())
        .await?
        .into_iter()
        .map(|row| entry_from_row(&row, &actor))
        .collect::<Result<Vec<_>, _>>()?;
        let total_minutes = items.iter().map(|entry| i64::from(entry.minutes)).sum();
        let mut query = SqlBuilder::new(
            "WITH RECURSIVE sub(id) AS (SELECT id FROM tasks WHERE parent_task_id = ",
        );
        query
            .push_bind(task_id.to_string())
            .push(" AND workspace_id = ")
            .push_bind(workspace_id.to_string())
            .push(
                " AND deleted_at IS NULL UNION SELECT tasks.id FROM tasks JOIN sub ON tasks.parent_task_id = sub.id \
                 WHERE tasks.deleted_at IS NULL) \
                 SELECT CAST(COALESCE(SUM(task_time_entries.minutes), 0) AS BIGINT) AS minutes FROM task_time_entries \
                 JOIN tasks ON tasks.id = task_time_entries.task_id \
                 WHERE task_time_entries.task_id IN (SELECT id FROM sub) AND ",
            )
            .push(ProjectScope::of(&actor).project("tasks.project_id"));
        if let Some(grant) = self.granted_projects() {
            query.push(" AND tasks.project_id IN ('', ");
            let mut ids = query.separated(", ");
            for id in grant {
                ids.push_bind(id.to_string());
            }
            ids.push_unseparated(")");
        }
        let sub_issues_minutes: i64 = query
            .build()
            .fetch_one(self.database().pool())
            .await?
            .get("minutes");
        Ok(TaskTimeEntries {
            items,
            total_minutes,
            sub_issues_minutes,
        })
    }

    /// One entry, when the caller sees its task.
    pub async fn time_entry(
        &self,
        workspace_id: Id,
        entry_id: Id,
        actor_id: Id,
    ) -> Result<TimeEntryRecord, TaskError> {
        let mut tx = self.database().pool().begin().await?;
        let (entry, _) = self
            .entry_in_tx(&mut tx, workspace_id, entry_id, actor_id)
            .await?;
        Ok(entry)
    }

    pub async fn create_time_entry(
        &self,
        workspace_id: Id,
        task_id: Id,
        actor_id: Id,
        entry: NewTimeEntry,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<TimeEntryRecord, TaskError> {
        let minutes = check_minutes(entry.minutes)?;
        let note = check_note(&entry.note)?;
        let user_id = entry.user_id.unwrap_or(actor_id);
        let worked_on = match entry.worked_on.as_deref() {
            Some(value) => parse_worked_on(value)?,
            None => self.today_of(user_id, now).await?,
        };
        let mut tx = self.database().workspace_transaction(workspace_id).await?;
        let record = self
            .insert_entry_in_tx(
                &mut tx,
                workspace_id,
                task_id,
                actor_id,
                Checked {
                    user_id,
                    minutes,
                    worked_on,
                    note,
                    billable: entry.billable,
                    comment_id: None,
                },
                request_id,
                now,
            )
            .await?;
        tx.commit().await?;
        Ok(record)
    }

    /// Writes a comment and the time it logs (`/spend` in the composer) in one transaction. The
    /// entry is the caller's, and its note is a copy of the comment's Markdown.
    #[allow(clippy::too_many_arguments)]
    pub async fn create_comment_with_time(
        &self,
        workspace_id: Id,
        task_id: Id,
        actor_id: Id,
        parent_id: Option<Id>,
        content: CommentContent,
        minutes: u32,
        worked_on: Option<String>,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(CommentRecord, TimeEntryRecord), TaskError> {
        let minutes = check_minutes(minutes)?;
        let worked_on = match worked_on.as_deref() {
            Some(value) => parse_worked_on(value)?,
            None => self.today_of(actor_id, now).await?,
        };
        let mut tx = self.database().workspace_transaction(workspace_id).await?;
        let comment = self
            .insert_comment_in_tx(
                &mut tx,
                workspace_id,
                task_id,
                actor_id,
                parent_id,
                content,
                ImportedComment::default(),
                request_id,
                now,
            )
            .await?;
        let note: String = comment.body.trim().chars().take(MAX_NOTE_CHARS).collect();
        let entry = self
            .insert_entry_in_tx(
                &mut tx,
                workspace_id,
                task_id,
                actor_id,
                Checked {
                    user_id: actor_id,
                    minutes,
                    worked_on,
                    note,
                    billable: false,
                    comment_id: Some(comment.id),
                },
                request_id,
                now,
            )
            .await?;
        tx.commit().await?;
        Ok((comment, entry))
    }

    /// Checks that the actor may log `entry` on the live task, then writes it with its audit event.
    #[allow(clippy::too_many_arguments)]
    async fn insert_entry_in_tx(
        &self,
        tx: &mut Transaction<'_, Any>,
        workspace_id: Id,
        task_id: Id,
        actor_id: Id,
        entry: Checked,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<TimeEntryRecord, TaskError> {
        let actor = require_task_tx(tx, workspace_id, task_id, actor_id).await?;
        let project_id = parse_id(
            sqlx::query_scalar::<_, String>(
                "SELECT project_id FROM tasks WHERE id = $1 AND workspace_id = $2",
            )
            .bind(task_id.to_string())
            .bind(workspace_id.to_string())
            .fetch_one(&mut **tx)
            .await?,
        )?;
        self.require_granted_project(project_id)?;
        let user_id = entry.user_id;
        if !actor.can_manage_time_of(project_id, user_id) {
            return Err(TaskError::Forbidden);
        }
        validate_member(tx, workspace_id, user_id, "user_id").await?;
        let id = Id::new_v7();
        sqlx::query(
            "INSERT INTO task_time_entries (id, workspace_id, task_id, user_id, minutes, worked_on, note, billable, created_by, comment_id, version, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, 0, $11, $11)",
        )
        .bind(id.to_string())
        .bind(workspace_id.to_string())
        .bind(task_id.to_string())
        .bind(user_id.to_string())
        .bind(i64::from(entry.minutes))
        .bind(&entry.worked_on)
        .bind(&entry.note)
        .bind(i64::from(entry.billable))
        .bind(actor_id.to_string())
        .bind(entry.comment_id.map(|id| id.to_string()))
        .bind(now.as_millis())
        .execute(&mut **tx)
        .await?;
        let record = TimeEntryRecord {
            id,
            workspace_id,
            task_id,
            project_id,
            user_id,
            minutes: entry.minutes,
            worked_on: entry.worked_on,
            note: entry.note,
            billable: entry.billable,
            created_by: actor_id,
            comment_id: entry.comment_id,
            version: 0,
            created_at: now,
            updated_at: now,
            can_edit: true,
        };
        self.record_write(
            tx,
            workspace_id,
            actor_id,
            ("time_entry.created", "time_entry"),
            id,
            request_id,
            audit_metadata(&record),
            now,
        )
        .await?;
        Ok(record)
    }

    /// Changes an entry. `expected_version`, when given, must be the entry's version.
    #[allow(clippy::too_many_arguments)]
    pub async fn update_time_entry(
        &self,
        workspace_id: Id,
        entry_id: Id,
        actor_id: Id,
        changes: TimeEntryChanges,
        expected_version: Option<u64>,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<TimeEntryRecord, TaskError> {
        let minutes = changes.minutes.map(check_minutes).transpose()?;
        let worked_on = changes
            .worked_on
            .as_deref()
            .map(parse_worked_on)
            .transpose()?;
        let note = changes.note.as_deref().map(check_note).transpose()?;
        let mut tx = self.database().workspace_transaction(workspace_id).await?;
        let (current, actor) = self
            .entry_in_tx(&mut tx, workspace_id, entry_id, actor_id)
            .await?;
        if !current.can_edit {
            return Err(TaskError::Forbidden);
        }
        if let Some(expected) = expected_version {
            check_version(expected, current.version, &current)?;
        }
        if let Some(user_id) = changes.user_id.filter(|id| *id != current.user_id) {
            if !actor.can_manage_time_of(current.project_id, user_id) {
                return Err(TaskError::Forbidden);
            }
            validate_member(&mut tx, workspace_id, user_id, "user_id").await?;
        }
        let record = TimeEntryRecord {
            user_id: changes.user_id.unwrap_or(current.user_id),
            minutes: minutes.unwrap_or(current.minutes),
            worked_on: worked_on.unwrap_or_else(|| current.worked_on.clone()),
            note: note.unwrap_or_else(|| current.note.clone()),
            billable: changes.billable.unwrap_or(current.billable),
            version: current.version + 1,
            updated_at: now,
            can_edit: actor.can_manage_time_of(
                current.project_id,
                changes.user_id.unwrap_or(current.user_id),
            ),
            ..current.clone()
        };
        let updated = sqlx::query(
            "UPDATE task_time_entries SET user_id = $1, minutes = $2, worked_on = $3, note = $4, billable = $5, \
             version = version + 1, updated_at = $6 WHERE id = $7 AND workspace_id = $8 AND version = $9",
        )
        .bind(record.user_id.to_string())
        .bind(i64::from(record.minutes))
        .bind(&record.worked_on)
        .bind(&record.note)
        .bind(i64::from(record.billable))
        .bind(now.as_millis())
        .bind(entry_id.to_string())
        .bind(workspace_id.to_string())
        .bind(current.version as i64)
        .execute(&mut *tx)
        .await?;
        if updated.rows_affected() != 1 {
            return Err(TaskError::Conflict);
        }
        let mut metadata = audit_metadata(&record);
        metadata["previous"] = json!({
            "user_id": current.user_id,
            "minutes": current.minutes,
            "worked_on": current.worked_on,
            "billable": current.billable,
        });
        self.record_write(
            &mut tx,
            workspace_id,
            actor_id,
            ("time_entry.updated", "time_entry"),
            entry_id,
            request_id,
            metadata,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(record)
    }

    pub async fn delete_time_entry(
        &self,
        workspace_id: Id,
        entry_id: Id,
        actor_id: Id,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), TaskError> {
        let mut tx = self.database().workspace_transaction(workspace_id).await?;
        let (current, _) = self
            .entry_in_tx(&mut tx, workspace_id, entry_id, actor_id)
            .await?;
        if !current.can_edit {
            return Err(TaskError::Forbidden);
        }
        sqlx::query("DELETE FROM task_time_entries WHERE id = $1 AND workspace_id = $2")
            .bind(entry_id.to_string())
            .bind(workspace_id.to_string())
            .execute(&mut *tx)
            .await?;
        self.record_write(
            &mut tx,
            workspace_id,
            actor_id,
            ("time_entry.deleted", "time_entry"),
            entry_id,
            request_id,
            audit_metadata(&current),
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// The calendar day `now` falls on in the timezone of `user_id`'s profile, else in UTC.
    async fn today_of(&self, user_id: Id, now: TimestampMillis) -> Result<String, TaskError> {
        let zone: Option<String> = sqlx::query_scalar("SELECT timezone FROM users WHERE id = $1")
            .bind(user_id.to_string())
            .fetch_optional(self.database().pool())
            .await?
            .flatten();
        let zone = zone
            .and_then(|name| jiff::tz::TimeZone::get(&name).ok())
            .unwrap_or(jiff::tz::TimeZone::UTC);
        let instant = jiff::Timestamp::from_millisecond(now.as_millis())
            .map_err(|_| TaskError::Invalid { field: "worked_on" })?;
        Ok(instant.to_zoned(zone).date().to_string())
    }

    /// The entry and the acting member, when the caller sees the entry's live task.
    async fn entry_in_tx(
        &self,
        tx: &mut Transaction<'_, Any>,
        workspace_id: Id,
        entry_id: Id,
        actor_id: Id,
    ) -> Result<(TimeEntryRecord, orbit_domain::Actor), TaskError> {
        let row = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT {COLUMNS} FROM task_time_entries JOIN tasks ON tasks.id = task_time_entries.task_id \
             WHERE task_time_entries.id = $1 AND task_time_entries.workspace_id = $2"
        )))
        .bind(entry_id.to_string())
        .bind(workspace_id.to_string())
        .fetch_optional(&mut **tx)
        .await?
        .ok_or(TaskError::NotFound)?;
        let task_id = parse_id(row.get("task_id"))?;
        let actor = require_task_tx(tx, workspace_id, task_id, actor_id).await?;
        let entry = entry_from_row(&row, &actor)?;
        self.require_granted_project(entry.project_id)?;
        Ok((entry, actor))
    }
}

/// Audit metadata: what was logged, never the note (it can be long; the audit row caps at 2 KB).
fn audit_metadata(entry: &TimeEntryRecord) -> serde_json::Value {
    json!({
        "task_id": entry.task_id,
        "project_id": entry.project_id,
        "user_id": entry.user_id,
        "minutes": entry.minutes,
        "worked_on": entry.worked_on,
        "billable": entry.billable,
    })
}

fn entry_from_row(
    row: &sqlx::any::AnyRow,
    actor: &orbit_domain::Actor,
) -> Result<TimeEntryRecord, TaskError> {
    let project_id = parse_id(row.get("project_id"))?;
    let user_id = parse_id(row.get("user_id"))?;
    Ok(TimeEntryRecord {
        id: parse_id(row.get("id"))?,
        workspace_id: parse_id(row.get("workspace_id"))?,
        task_id: parse_id(row.get("task_id"))?,
        project_id,
        user_id,
        minutes: u32::try_from(row.get::<i64, _>("minutes")).map_err(|_| TaskError::Conflict)?,
        worked_on: row.get("worked_on"),
        note: row.get("note"),
        billable: row.get("billable"),
        created_by: parse_id(row.get("created_by"))?,
        comment_id: row
            .get::<Option<String>, _>("comment_id")
            .map(parse_id)
            .transpose()?,
        version: u64::try_from(row.get::<i64, _>("version")).map_err(|_| TaskError::Conflict)?,
        created_at: TimestampMillis::from_millis(row.get("created_at")),
        updated_at: TimestampMillis::from_millis(row.get("updated_at")),
        can_edit: actor.can_manage_time_of(project_id, user_id),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worked_on_is_a_calendar_date() {
        assert_eq!(parse_worked_on(" 2026-11-10 ").unwrap(), "2026-11-10");
        for bad in [
            "2026-02-30",
            "10/11/2026",
            "2026-11-10T10:00:00Z",
            "",
            "20261-01-01",
        ] {
            assert!(parse_worked_on(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn minutes_stay_within_a_day() {
        assert!(check_minutes(0).is_err());
        assert_eq!(check_minutes(1).unwrap(), 1);
        assert_eq!(check_minutes(1440).unwrap(), 1440);
        assert!(check_minutes(1441).is_err());
    }
}
