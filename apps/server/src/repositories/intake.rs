//! Intake (spec `2026-10-08-intake-triage-templates-recurring-design.md`): the triage actions, the
//! task payload that templates and recurring tasks store, and the recurring job.

use orbit_platform::{Id, TimestampMillis};
use serde::{Deserialize, Serialize};
use sqlx::{Row, Sqlite, Transaction};
use utoipa::ToSchema;

use super::sub_issues::AutoClosed;
use super::task_notifications::{self, TaskEvent};
use super::task_relations;
use super::tasks::{
    CreateTask, Page, TaskChanges, TaskError, TaskRepository, TaskUpdate, TaskUpdateOutcome,
    check_version, create_task_in_tx, is_unique_violation, optional_id, parse_id, parse_version,
    record_mutation, require_access_tx, require_project, require_project_tx, task_in_tx,
};
use crate::push::NotificationKind;

const DAY_MS: i64 = 86_400_000;
const PRIORITIES: [&str; 5] = ["none", "low", "medium", "high", "urgent"];
const MAX_SUB_ISSUES: usize = 50;
const MAX_DUE_OFFSET_DAYS: i64 = 3_650;
const RECURRING_ACCOUNT: &str = "Recurring";

/// The fields of a task as a template or a recurring task stores them. References are ids as
/// text: each one is checked again when the payload is used, and one that no longer exists is
/// dropped (a missing status becomes the default status of the project).
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TaskPayload {
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub status_id: Option<String>,
    /// `none` (the default), `low`, `medium`, `high` or `urgent`.
    #[serde(default = "no_priority")]
    pub priority: String,
    #[serde(default)]
    pub label_ids: Vec<String>,
    #[serde(default)]
    pub assignee_ids: Vec<String>,
    #[serde(default)]
    pub milestone_id: Option<String>,
    /// The due date, in days after the day the task is created.
    #[serde(default)]
    pub due_offset_days: Option<i64>,
    /// Points. A payload holds no cycle: the "tasks with no cycle" options of the project decide.
    #[serde(default)]
    pub estimate: Option<i64>,
    /// Sub-issues, one level deep: a sub-issue has no `sub_issues` of its own.
    #[serde(default)]
    #[schema(no_recursion)]
    pub sub_issues: Vec<TaskPayload>,
}

fn no_priority() -> String {
    "none".to_owned()
}

impl TaskPayload {
    /// The shape rules. References are not checked here: they are checked at each use.
    pub fn validate(&self) -> Result<(), TaskError> {
        self.validate_fields()?;
        if self.sub_issues.len() > MAX_SUB_ISSUES {
            return Err(TaskError::Invalid {
                field: "sub_issues",
            });
        }
        for sub_issue in &self.sub_issues {
            if !sub_issue.sub_issues.is_empty() {
                return Err(TaskError::Invalid {
                    field: "sub_issues",
                });
            }
            sub_issue.validate_fields()?;
        }
        Ok(())
    }

    fn validate_fields(&self) -> Result<(), TaskError> {
        let title = self.title.trim();
        if title.is_empty() || title.chars().count() > 500 {
            return Err(TaskError::Invalid { field: "title" });
        }
        if self.description.len() > 100_000 {
            return Err(TaskError::Invalid {
                field: "description",
            });
        }
        if !PRIORITIES.contains(&self.priority.as_str()) {
            return Err(TaskError::Invalid { field: "priority" });
        }
        if self
            .due_offset_days
            .is_some_and(|days| !(0..=MAX_DUE_OFFSET_DAYS).contains(&days))
        {
            return Err(TaskError::Invalid {
                field: "due_offset_days",
            });
        }
        let ids = self
            .status_id
            .iter()
            .chain(&self.milestone_id)
            .chain(&self.label_ids)
            .chain(&self.assignee_ids);
        for id in ids {
            if id.parse::<Id>().is_err() {
                return Err(TaskError::Invalid { field: "payload" });
            }
        }
        if self
            .estimate
            .is_some_and(|points| !(0..=10_000).contains(&points))
        {
            return Err(TaskError::Invalid { field: "estimate" });
        }
        if self.label_ids.len() > 100 || self.assignee_ids.len() > 100 {
            return Err(TaskError::Invalid { field: "payload" });
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct TemplateRecord {
    #[schema(value_type = String)]
    pub id: Id,
    #[schema(value_type = String)]
    pub workspace_id: Id,
    #[schema(value_type = String)]
    pub project_id: Id,
    pub name: String,
    pub payload: TaskPayload,
    pub position: i64,
    pub version: u64,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: TimestampMillis,
    #[schema(value_type = String, format = DateTime)]
    pub updated_at: TimestampMillis,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct RecurringTaskRecord {
    #[schema(value_type = String)]
    pub id: Id,
    #[schema(value_type = String)]
    pub workspace_id: Id,
    #[schema(value_type = String)]
    pub project_id: Id,
    pub payload: TaskPayload,
    /// `schedule`: at fixed times from `anchor_at`. `after_completion`: one interval after the
    /// last created task is closed.
    pub mode: String,
    pub every_count: i64,
    /// `day`, `week` or `month`.
    pub every_unit: String,
    #[schema(value_type = String, format = DateTime)]
    pub anchor_at: TimestampMillis,
    /// Null while an `after_completion` routine waits for its last task.
    #[schema(value_type = Option<String>, format = DateTime, required = true)]
    pub next_run_at: Option<TimestampMillis>,
    #[schema(value_type = Option<String>, required = true)]
    pub last_task_id: Option<Id>,
    pub paused: bool,
    pub version: u64,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: TimestampMillis,
    #[schema(value_type = String, format = DateTime)]
    pub updated_at: TimestampMillis,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecurringMode {
    Schedule,
    AfterCompletion,
}

impl RecurringMode {
    pub fn parse(value: &str) -> Result<Self, TaskError> {
        match value {
            "schedule" => Ok(Self::Schedule),
            "after_completion" => Ok(Self::AfterCompletion),
            _ => Err(TaskError::Invalid { field: "mode" }),
        }
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::Schedule => "schedule",
            Self::AfterCompletion => "after_completion",
        }
    }
}

/// "Every `count` `unit`s". Month intervals keep the day of the month and use the last day when
/// the month is shorter. All times are UTC.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Interval {
    pub count: i64,
    pub unit: IntervalUnit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IntervalUnit {
    Day,
    Week,
    Month,
}

impl Interval {
    pub fn parse(count: i64, unit: &str) -> Result<Self, TaskError> {
        if !(1..=1_000).contains(&count) {
            return Err(TaskError::Invalid {
                field: "every_count",
            });
        }
        let unit = match unit {
            "day" => IntervalUnit::Day,
            "week" => IntervalUnit::Week,
            "month" => IntervalUnit::Month,
            _ => {
                return Err(TaskError::Invalid {
                    field: "every_unit",
                });
            }
        };
        Ok(Self { count, unit })
    }

    const fn unit_str(self) -> &'static str {
        match self.unit {
            IntervalUnit::Day => "day",
            IntervalUnit::Week => "week",
            IntervalUnit::Month => "month",
        }
    }

    /// `from` plus `steps` intervals. Always computed from `from` itself, so a schedule that
    /// starts on the 31st returns to the 31st after a shorter month.
    #[must_use]
    pub fn add(self, from: TimestampMillis, steps: i64) -> TimestampMillis {
        let from = from.as_millis();
        let millis = match self.unit {
            IntervalUnit::Day => from + steps * self.count * DAY_MS,
            IntervalUnit::Week => from + steps * self.count * 7 * DAY_MS,
            IntervalUnit::Month => add_months(from, steps * self.count),
        };
        TimestampMillis::from_millis(millis)
    }

    /// The first time of the schedule `anchor, anchor + 1, anchor + 2, ...` that is after `now`
    /// (the anchor itself while it is still in the future). One result for any length of
    /// downtime: missed runs are not replayed.
    #[must_use]
    pub fn next_after(self, anchor: TimestampMillis, now: TimestampMillis) -> TimestampMillis {
        if anchor > now {
            return anchor;
        }
        let elapsed = now.as_millis() - anchor.as_millis();
        // A lower bound for the number of whole intervals; a month is at most 31 days.
        let step_ms = match self.unit {
            IntervalUnit::Day => self.count * DAY_MS,
            IntervalUnit::Week => self.count * 7 * DAY_MS,
            IntervalUnit::Month => self.count * 31 * DAY_MS,
        };
        let mut steps = elapsed / step_ms;
        loop {
            let candidate = self.add(anchor, steps);
            if candidate > now {
                return candidate;
            }
            steps += 1;
        }
    }
}

/// Days from 1970-01-01 to a civil date (proleptic Gregorian; Howard Hinnant's algorithm).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year.rem_euclid(400);
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

fn days_in_month(year: i64, month: i64) -> i64 {
    let (next_year, next_month) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    days_from_civil(next_year, next_month, 1) - days_from_civil(year, month, 1)
}

fn add_months(millis: i64, months: i64) -> i64 {
    let days = millis.div_euclid(DAY_MS);
    let time_of_day = millis.rem_euclid(DAY_MS);
    let (year, month, day) = civil_from_days(days);
    let total = year * 12 + (month - 1) + months;
    let (year, month) = (total.div_euclid(12), total.rem_euclid(12) + 1);
    let day = day.min(days_in_month(year, month));
    days_from_civil(year, month, day) * DAY_MS + time_of_day
}

#[derive(Clone, Debug)]
pub struct CreateRecurringTask {
    pub payload: TaskPayload,
    pub mode: String,
    pub every_count: i64,
    pub every_unit: String,
    /// The first run; now when absent.
    pub starts_at: Option<TimestampMillis>,
}

/// `None` keeps the stored value.
#[derive(Clone, Debug, Default)]
pub struct RecurringTaskChanges {
    pub payload: Option<TaskPayload>,
    pub mode: Option<String>,
    pub every_count: Option<i64>,
    pub every_unit: Option<String>,
    pub starts_at: Option<TimestampMillis>,
    pub paused: Option<bool>,
}

#[derive(Clone, Debug)]
pub enum TriageAction {
    /// To `status_id`, or to the first backlog status (else the first unstarted one).
    Accept { status_id: Option<Id> },
    /// To the first cancelled status, with an optional comment.
    Decline { comment: Option<String> },
}

/// What one run of the recurring job did.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RecurringRun {
    pub created: usize,
    pub failed: usize,
}

const TEMPLATE_COLUMNS: &str =
    "id, workspace_id, project_id, name, payload_json, position, version, created_at, updated_at";
const RECURRING_COLUMNS: &str = "id, workspace_id, project_id, payload_json, mode, every_count, \
     every_unit, anchor_at, next_run_at, last_task_id, paused, version, created_at, updated_at";

impl TaskRepository {
    /// Accept or decline a task that waits in triage. Each is a normal status change.
    #[allow(clippy::too_many_arguments)]
    pub async fn triage_task(
        &self,
        workspace_id: Id,
        task_id: Id,
        actor_id: Id,
        action: TriageAction,
        expected_version: u64,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<TaskUpdateOutcome, TaskError> {
        let status_id = {
            let mut tx = self.database().transaction().await?;
            require_access_tx(&mut tx, workspace_id, actor_id).await?;
            let task = task_in_tx(&mut tx, workspace_id, task_id, false).await?;
            if task_relations::status_category_in_tx(&mut tx, workspace_id, task.status_id).await?
                != "triage"
            {
                return Err(TaskError::Invalid { field: "status_id" });
            }
            let target: Option<String> = match &action {
                TriageAction::Accept {
                    status_id: Some(status_id),
                } => Some(status_id.to_string()),
                TriageAction::Accept { status_id: None } => sqlx::query_scalar(
                    "SELECT id FROM task_statuses WHERE project_id = ? AND category IN ('backlog', 'unstarted') \
                     ORDER BY CASE category WHEN 'backlog' THEN 0 ELSE 1 END, position, id LIMIT 1",
                )
                .bind(task.project_id.to_string())
                .fetch_optional(&mut *tx)
                .await?,
                TriageAction::Decline { .. } => sqlx::query_scalar(
                    "SELECT id FROM task_statuses WHERE project_id = ? AND category = 'cancelled' \
                     ORDER BY position, id LIMIT 1",
                )
                .bind(task.project_id.to_string())
                .fetch_optional(&mut *tx)
                .await?,
            };
            let target = parse_id(target.ok_or(TaskError::Invalid { field: "status_id" })?)?;
            let category =
                task_relations::status_category_in_tx(&mut tx, workspace_id, target).await?;
            if category == "triage" || category == task_relations::DUPLICATE {
                return Err(TaskError::Invalid { field: "status_id" });
            }
            target
        };
        let outcome = self
            .update_task(
                workspace_id,
                actor_id,
                &TaskUpdate {
                    id: task_id,
                    expected_version,
                    changes: TaskChanges {
                        status_id: Some(status_id),
                        ..TaskChanges::default()
                    },
                },
                request_id,
                now,
            )
            .await?;
        if let TriageAction::Decline {
            comment: Some(comment),
        } = action
        {
            self.create_comment(
                workspace_id,
                task_id,
                actor_id,
                None,
                comment,
                request_id,
                now,
            )
            .await?;
        }
        Ok(outcome)
    }

    /// Creates a task and its sub-issues from a payload, all or nothing.
    pub async fn create_task_from_payload(
        &self,
        workspace_id: Id,
        actor_id: Id,
        project_id: Id,
        payload: &TaskPayload,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<TaskUpdateOutcome, TaskError> {
        payload.validate()?;
        let mut tx = self.database().immediate_transaction().await?;
        require_project_tx(&mut tx, workspace_id, project_id, actor_id).await?;
        let mut auto_closed = Vec::new();
        let id = create_from_payload_in_tx(
            &mut tx,
            workspace_id,
            project_id,
            actor_id,
            None,
            payload,
            request_id,
            now,
            &mut auto_closed,
        )
        .await?;
        let task = task_in_tx(&mut tx, workspace_id, id, false).await?;
        tx.commit().await?;
        Ok(TaskUpdateOutcome { task, auto_closed })
    }

    pub async fn templates(
        &self,
        workspace_id: Id,
        project_id: Id,
        actor_id: Id,
    ) -> Result<Page<TemplateRecord>, TaskError> {
        let pool = self.database().pool();
        require_project(pool, workspace_id, project_id, actor_id).await?;
        let items = sqlx::query(&format!(
            "SELECT {TEMPLATE_COLUMNS} FROM task_templates WHERE project_id = ? ORDER BY position, id"
        ))
        .bind(project_id.to_string())
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(template_from_row)
        .collect::<Result<Vec<_>, _>>()?;
        Ok(Page {
            items,
            next_cursor: None,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn create_template(
        &self,
        workspace_id: Id,
        project_id: Id,
        actor_id: Id,
        name: String,
        payload: TaskPayload,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<TemplateRecord, TaskError> {
        payload.validate()?;
        let id = Id::new_v7();
        let mut tx = self.database().immediate_transaction().await?;
        require_project_tx(&mut tx, workspace_id, project_id, actor_id).await?;
        let inserted = sqlx::query(
            "INSERT INTO task_templates (id, workspace_id, project_id, name, payload_json, position, \
             version, created_by, created_at, updated_at) VALUES (?, ?, ?, ?, ?, \
             (SELECT COALESCE(MAX(position) + 1, 0) FROM task_templates WHERE project_id = ?), 0, ?, ?, ?)",
        )
        .bind(id.to_string())
        .bind(workspace_id.to_string())
        .bind(project_id.to_string())
        .bind(&name)
        .bind(encode(&payload)?)
        .bind(project_id.to_string())
        .bind(actor_id.to_string())
        .bind(now.as_millis())
        .bind(now.as_millis())
        .execute(&mut *tx)
        .await;
        if is_unique_violation(&inserted) {
            return Err(TaskError::Conflict);
        }
        inserted?;
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "task_template.created",
            "task_template",
            id,
            request_id,
            now,
        )
        .await?;
        let record = template_in_tx(&mut tx, project_id, id).await?;
        tx.commit().await?;
        Ok(record)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update_template(
        &self,
        workspace_id: Id,
        project_id: Id,
        template_id: Id,
        actor_id: Id,
        name: String,
        payload: TaskPayload,
        expected_version: u64,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<TemplateRecord, TaskError> {
        payload.validate()?;
        let mut tx = self.database().immediate_transaction().await?;
        require_project_tx(&mut tx, workspace_id, project_id, actor_id).await?;
        let current = template_in_tx(&mut tx, project_id, template_id).await?;
        check_version(expected_version, current.version, &current)?;
        let updated = sqlx::query(
            "UPDATE task_templates SET name = ?, payload_json = ?, version = version + 1, updated_at = ? \
             WHERE id = ? AND version = ?",
        )
        .bind(&name)
        .bind(encode(&payload)?)
        .bind(now.as_millis())
        .bind(template_id.to_string())
        .bind(expected_version as i64)
        .execute(&mut *tx)
        .await;
        if is_unique_violation(&updated) {
            return Err(TaskError::Conflict);
        }
        updated?;
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "task_template.updated",
            "task_template",
            template_id,
            request_id,
            now,
        )
        .await?;
        let record = template_in_tx(&mut tx, project_id, template_id).await?;
        tx.commit().await?;
        Ok(record)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn delete_template(
        &self,
        workspace_id: Id,
        project_id: Id,
        template_id: Id,
        actor_id: Id,
        expected_version: u64,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), TaskError> {
        let mut tx = self.database().immediate_transaction().await?;
        require_project_tx(&mut tx, workspace_id, project_id, actor_id).await?;
        let current = template_in_tx(&mut tx, project_id, template_id).await?;
        check_version(expected_version, current.version, &current)?;
        sqlx::query("DELETE FROM task_templates WHERE id = ?")
            .bind(template_id.to_string())
            .execute(&mut *tx)
            .await?;
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "task_template.deleted",
            "task_template",
            template_id,
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn recurring_tasks(
        &self,
        workspace_id: Id,
        project_id: Id,
        actor_id: Id,
    ) -> Result<Page<RecurringTaskRecord>, TaskError> {
        let pool = self.database().pool();
        require_project(pool, workspace_id, project_id, actor_id).await?;
        let items = sqlx::query(&format!(
            "SELECT {RECURRING_COLUMNS} FROM recurring_tasks WHERE project_id = ? ORDER BY created_at, id"
        ))
        .bind(project_id.to_string())
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(recurring_from_row)
        .collect::<Result<Vec<_>, _>>()?;
        Ok(Page {
            items,
            next_cursor: None,
        })
    }

    pub async fn create_recurring_task(
        &self,
        workspace_id: Id,
        project_id: Id,
        actor_id: Id,
        input: CreateRecurringTask,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<RecurringTaskRecord, TaskError> {
        input.payload.validate()?;
        let mode = RecurringMode::parse(&input.mode)?;
        let interval = Interval::parse(input.every_count, &input.every_unit)?;
        let anchor = input.starts_at.unwrap_or(now);
        let id = Id::new_v7();
        let mut tx = self.database().immediate_transaction().await?;
        require_project_tx(&mut tx, workspace_id, project_id, actor_id).await?;
        // The first task is created at the anchor in both modes.
        sqlx::query(
            "INSERT INTO recurring_tasks (id, workspace_id, project_id, payload_json, mode, every_count, \
             every_unit, anchor_at, next_run_at, paused, version, created_by, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 0, 0, ?, ?, ?)",
        )
        .bind(id.to_string())
        .bind(workspace_id.to_string())
        .bind(project_id.to_string())
        .bind(encode(&input.payload)?)
        .bind(mode.as_str())
        .bind(interval.count)
        .bind(interval.unit_str())
        .bind(anchor.as_millis())
        .bind(anchor.as_millis())
        .bind(actor_id.to_string())
        .bind(now.as_millis())
        .bind(now.as_millis())
        .execute(&mut *tx)
        .await?;
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "recurring_task.created",
            "recurring_task",
            id,
            request_id,
            now,
        )
        .await?;
        let record = recurring_in_tx(&mut tx, project_id, id).await?;
        tx.commit().await?;
        Ok(record)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update_recurring_task(
        &self,
        workspace_id: Id,
        project_id: Id,
        recurring_id: Id,
        actor_id: Id,
        changes: RecurringTaskChanges,
        expected_version: u64,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<RecurringTaskRecord, TaskError> {
        let mut tx = self.database().immediate_transaction().await?;
        require_project_tx(&mut tx, workspace_id, project_id, actor_id).await?;
        let current = recurring_in_tx(&mut tx, project_id, recurring_id).await?;
        check_version(expected_version, current.version, &current)?;
        let payload = changes.payload.unwrap_or(current.payload);
        payload.validate()?;
        let mode = RecurringMode::parse(changes.mode.as_deref().unwrap_or(&current.mode))?;
        let interval = Interval::parse(
            changes.every_count.unwrap_or(current.every_count),
            changes.every_unit.as_deref().unwrap_or(&current.every_unit),
        )?;
        let anchor = changes.starts_at.unwrap_or(current.anchor_at);
        let paused = changes.paused.unwrap_or(current.paused);
        let rescheduled = mode.as_str() != current.mode
            || interval.count != current.every_count
            || interval.unit_str() != current.every_unit
            || anchor != current.anchor_at
            || (current.paused && !paused);
        // A changed schedule starts again from the anchor; a resumed one skips what it missed.
        let next_run_at = if !rescheduled {
            current.next_run_at
        } else {
            match mode {
                RecurringMode::Schedule => Some(interval.next_after(anchor, now)),
                RecurringMode::AfterCompletion => {
                    if last_task_is_open(&mut tx, current.last_task_id).await? {
                        None
                    } else {
                        Some(current.next_run_at.unwrap_or(now).max(anchor))
                    }
                }
            }
        };
        sqlx::query(
            "UPDATE recurring_tasks SET payload_json = ?, mode = ?, every_count = ?, every_unit = ?, \
             anchor_at = ?, next_run_at = ?, paused = ?, version = version + 1, updated_at = ? \
             WHERE id = ? AND version = ?",
        )
        .bind(encode(&payload)?)
        .bind(mode.as_str())
        .bind(interval.count)
        .bind(interval.unit_str())
        .bind(anchor.as_millis())
        .bind(next_run_at.map(TimestampMillis::as_millis))
        .bind(paused)
        .bind(now.as_millis())
        .bind(recurring_id.to_string())
        .bind(expected_version as i64)
        .execute(&mut *tx)
        .await?;
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "recurring_task.updated",
            "recurring_task",
            recurring_id,
            request_id,
            now,
        )
        .await?;
        let record = recurring_in_tx(&mut tx, project_id, recurring_id).await?;
        tx.commit().await?;
        Ok(record)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn delete_recurring_task(
        &self,
        workspace_id: Id,
        project_id: Id,
        recurring_id: Id,
        actor_id: Id,
        expected_version: u64,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), TaskError> {
        let mut tx = self.database().immediate_transaction().await?;
        require_project_tx(&mut tx, workspace_id, project_id, actor_id).await?;
        let current = recurring_in_tx(&mut tx, project_id, recurring_id).await?;
        check_version(expected_version, current.version, &current)?;
        sqlx::query("DELETE FROM recurring_tasks WHERE id = ?")
            .bind(recurring_id.to_string())
            .execute(&mut *tx)
            .await?;
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "recurring_task.deleted",
            "recurring_task",
            recurring_id,
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// The recurring job: creates one task for each routine that is due, in a live project of a
    /// live workspace. Each routine has its own transaction, so one bad routine stops no other.
    pub async fn run_recurring_tasks(
        &self,
        now: TimestampMillis,
    ) -> Result<RecurringRun, TaskError> {
        self.wake_after_completion(now).await?;
        let due: Vec<String> = sqlx::query_scalar(
            "SELECT recurring_tasks.id FROM recurring_tasks \
             JOIN projects ON projects.id = recurring_tasks.project_id \
             JOIN workspaces ON workspaces.id = recurring_tasks.workspace_id \
             WHERE recurring_tasks.paused = 0 AND recurring_tasks.next_run_at <= ? \
             AND projects.deleted_at IS NULL AND workspaces.deleted_at IS NULL \
             ORDER BY recurring_tasks.next_run_at, recurring_tasks.id",
        )
        .bind(now.as_millis())
        .fetch_all(self.database().pool())
        .await?;
        let mut run = RecurringRun::default();
        for id in due {
            match self.run_one_recurring_task(parse_id(id)?, now).await {
                Ok(true) => run.created += 1,
                Ok(false) => {}
                Err(_) => run.failed += 1,
            }
        }
        Ok(run)
    }

    /// `after_completion`: a routine whose last task is closed, in the trash or gone runs again
    /// one interval after that moment. The job finds these itself, so every way a task can close
    /// (a status change, the sub-issue automation, GitHub sync, the trash) follows one rule.
    async fn wake_after_completion(&self, now: TimestampMillis) -> Result<(), TaskError> {
        let rows = sqlx::query(
            "SELECT recurring_tasks.id, recurring_tasks.every_count, recurring_tasks.every_unit, \
             COALESCE(tasks.deleted_at, tasks.completed_at) AS closed_at \
             FROM recurring_tasks LEFT JOIN tasks ON tasks.id = recurring_tasks.last_task_id \
             WHERE recurring_tasks.mode = 'after_completion' AND recurring_tasks.next_run_at IS NULL \
             AND (tasks.id IS NULL OR tasks.deleted_at IS NOT NULL OR tasks.completed_at IS NOT NULL)",
        )
        .fetch_all(self.database().pool())
        .await?;
        for row in rows {
            let interval = Interval::parse(row.get("every_count"), row.get("every_unit"))?;
            let closed_at = row
                .get::<Option<i64>, _>("closed_at")
                .map_or(now, TimestampMillis::from_millis);
            sqlx::query(
                "UPDATE recurring_tasks SET next_run_at = ? WHERE id = ? AND next_run_at IS NULL",
            )
            .bind(interval.add(closed_at, 1).as_millis())
            .bind(row.get::<String, _>("id"))
            .execute(self.database().pool())
            .await?;
        }
        Ok(())
    }

    async fn run_one_recurring_task(
        &self,
        recurring_id: Id,
        now: TimestampMillis,
    ) -> Result<bool, TaskError> {
        let mut tx = self.database().immediate_transaction().await?;
        let row = sqlx::query(&format!(
            "SELECT {RECURRING_COLUMNS}, created_by FROM recurring_tasks WHERE id = ? AND paused = 0 \
             AND next_run_at <= ?"
        ))
        .bind(recurring_id.to_string())
        .bind(now.as_millis())
        .fetch_optional(&mut *tx)
        .await?;
        // Paused, deleted or already run since the list was read.
        let Some(row) = row else { return Ok(false) };
        let created_by: Option<String> = row.get("created_by");
        let routine = recurring_from_row(row)?;
        // The task is attributed to the member who made the routine while that person is still an
        // active member, else to the workspace owner; the creator shown is the service account.
        let actor: Option<String> = sqlx::query_scalar(
            "SELECT memberships.user_id FROM memberships JOIN users ON users.id = memberships.user_id \
             WHERE memberships.workspace_id = ? AND users.suspended_at IS NULL \
             AND (memberships.user_id = ? OR memberships.role = 'owner') \
             ORDER BY (memberships.user_id = ?) DESC LIMIT 1",
        )
        .bind(routine.workspace_id.to_string())
        .bind(&created_by)
        .bind(&created_by)
        .fetch_optional(&mut *tx)
        .await?;
        let actor_id = parse_id(actor.ok_or(TaskError::NotFound)?)?;
        let account = service_account_in_tx(&mut tx, routine.workspace_id, actor_id, now).await?;
        let request_id = format!("recurring:{recurring_id}");
        let mut auto_closed = Vec::new();
        let task_id = create_from_payload_in_tx(
            &mut tx,
            routine.workspace_id,
            routine.project_id,
            actor_id,
            Some((account, RECURRING_ACCOUNT)),
            &routine.payload,
            &request_id,
            now,
            &mut auto_closed,
        )
        .await?;
        let interval = Interval::parse(routine.every_count, &routine.every_unit)?;
        let next_run_at = match RecurringMode::parse(&routine.mode)? {
            RecurringMode::Schedule => Some(interval.next_after(routine.anchor_at, now)),
            RecurringMode::AfterCompletion => None,
        };
        sqlx::query(
            "UPDATE recurring_tasks SET last_task_id = ?, next_run_at = ?, updated_at = ? WHERE id = ?",
        )
        .bind(task_id.to_string())
        .bind(next_run_at.map(TimestampMillis::as_millis))
        .bind(now.as_millis())
        .bind(recurring_id.to_string())
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(true)
    }
}

/// A task entered the triage queue: notifies the project lead and members (not the actor). A
/// project with no lead and no members notifies nobody. `actor` is absent for an integration.
pub(super) async fn notify_triage_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    project_id: Id,
    task_id: Id,
    actor: Option<Id>,
    now: TimestampMillis,
) -> Result<(), TaskError> {
    let recipients = project_people_in_tx(tx, project_id).await?;
    task_notifications::notify_in_tx(
        tx,
        TaskEvent {
            workspace_id,
            actor,
            task_id,
            now,
        },
        NotificationKind::TaskTriageNew,
        None,
        &recipients,
    )
    .await?;
    Ok(())
}

/// The lead and the members of a project who are active members of the workspace.
pub(super) async fn project_people_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    project_id: Id,
) -> Result<Vec<Id>, TaskError> {
    sqlx::query_scalar::<_, String>(
        "SELECT people.user_id FROM ( \
             SELECT lead_user_id AS user_id FROM projects WHERE id = ?1 AND lead_user_id IS NOT NULL \
             UNION SELECT user_id FROM project_members WHERE project_id = ?1) AS people \
         JOIN projects ON projects.id = ?1 \
         JOIN memberships ON memberships.user_id = people.user_id \
              AND memberships.workspace_id = projects.workspace_id \
         JOIN users ON users.id = people.user_id AND users.suspended_at IS NULL \
         ORDER BY people.user_id",
    )
    .bind(project_id.to_string())
    .fetch_all(&mut **tx)
    .await?
    .into_iter()
    .map(parse_id)
    .collect()
}

/// The "Recurring" service account of the workspace, created on first use (as "GitHub" is).
async fn service_account_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    actor_id: Id,
    now: TimestampMillis,
) -> Result<Id, TaskError> {
    if let Some(id) = sqlx::query_scalar::<_, String>(
        "SELECT id FROM service_accounts WHERE workspace_id = ? AND lower(name) = lower(?) AND disabled_at IS NULL",
    )
    .bind(workspace_id.to_string())
    .bind(RECURRING_ACCOUNT)
    .fetch_optional(&mut **tx)
    .await?
    {
        return parse_id(id);
    }
    let id = Id::new_v7();
    sqlx::query(
        "INSERT INTO service_accounts (id, workspace_id, name, created_by, created_at) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(id.to_string())
    .bind(workspace_id.to_string())
    .bind(RECURRING_ACCOUNT)
    .bind(actor_id.to_string())
    .bind(now.as_millis())
    .execute(&mut **tx)
    .await?;
    Ok(id)
}

async fn last_task_is_open(
    tx: &mut Transaction<'_, Sqlite>,
    last_task_id: Option<Id>,
) -> Result<bool, TaskError> {
    let Some(task_id) = last_task_id else {
        return Ok(false);
    };
    Ok(sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM tasks WHERE id = ? AND deleted_at IS NULL AND completed_at IS NULL)",
    )
    .bind(task_id.to_string())
    .fetch_one(&mut **tx)
    .await?)
}

/// Creates the task of `payload` and its sub-issues inside the caller's transaction.
#[allow(clippy::too_many_arguments)]
async fn create_from_payload_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    project_id: Id,
    actor_id: Id,
    service_account: Option<(Id, &str)>,
    payload: &TaskPayload,
    request_id: &str,
    now: TimestampMillis,
    auto_closed: &mut Vec<AutoClosed>,
) -> Result<Id, TaskError> {
    let input = resolve_payload(tx, workspace_id, project_id, payload, None, now).await?;
    let parent_id = create_task_in_tx(
        tx,
        workspace_id,
        actor_id,
        service_account,
        &input,
        request_id,
        now,
        auto_closed,
    )
    .await?;
    for sub_issue in &payload.sub_issues {
        let input = resolve_payload(
            tx,
            workspace_id,
            project_id,
            sub_issue,
            Some(parent_id),
            now,
        )
        .await?;
        create_task_in_tx(
            tx,
            workspace_id,
            actor_id,
            service_account,
            &input,
            request_id,
            now,
            auto_closed,
        )
        .await?;
    }
    Ok(parent_id)
}

/// Checks every reference of the payload again. A status, label, member or milestone that no
/// longer exists is dropped; a missing status becomes the default status of the project.
async fn resolve_payload(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    project_id: Id,
    payload: &TaskPayload,
    parent_task_id: Option<Id>,
    now: TimestampMillis,
) -> Result<CreateTask, TaskError> {
    let stored_status: Option<String> = match &payload.status_id {
        Some(status_id) => sqlx::query_scalar(
            "SELECT id FROM task_statuses WHERE id = ? AND project_id = ? AND category <> 'duplicate'",
        )
        .bind(status_id)
        .bind(project_id.to_string())
        .fetch_optional(&mut **tx)
        .await?,
        None => None,
    };
    let status_id = match stored_status {
        Some(status_id) => parse_id(status_id)?,
        None => task_relations::default_status_id_in_tx(tx, workspace_id, project_id)
            .await?
            .ok_or(TaskError::NotFound)?,
    };
    let mut label_ids = Vec::new();
    for label_id in &payload.label_ids {
        let found: Option<String> =
            sqlx::query_scalar("SELECT id FROM labels WHERE id = ? AND workspace_id = ?")
                .bind(label_id)
                .bind(workspace_id.to_string())
                .fetch_optional(&mut **tx)
                .await?;
        if let Some(id) = found.map(parse_id).transpose()?
            && !label_ids.contains(&id)
        {
            label_ids.push(id);
        }
    }
    let mut assignee_ids = Vec::new();
    for user_id in &payload.assignee_ids {
        let found: Option<String> = sqlx::query_scalar(
            "SELECT memberships.user_id FROM memberships JOIN users ON users.id = memberships.user_id \
             WHERE memberships.workspace_id = ? AND memberships.user_id = ? AND users.suspended_at IS NULL",
        )
        .bind(workspace_id.to_string())
        .bind(user_id)
        .fetch_optional(&mut **tx)
        .await?;
        if let Some(id) = found.map(parse_id).transpose()?
            && !assignee_ids.contains(&id)
        {
            assignee_ids.push(id);
        }
    }
    let milestone_id: Option<String> = match &payload.milestone_id {
        Some(milestone_id) => {
            sqlx::query_scalar("SELECT id FROM milestones WHERE id = ? AND project_id = ?")
                .bind(milestone_id)
                .bind(project_id.to_string())
                .fetch_optional(&mut **tx)
                .await?
        }
        None => None,
    };
    Ok(CreateTask {
        project_id,
        status_id,
        title: payload.title.trim().to_owned(),
        description: payload.description.clone(),
        source_url: None,
        priority: payload.priority.clone(),
        position: None,
        assignee_ids,
        label_ids,
        due_start_at: None,
        due_at: payload
            .due_offset_days
            .map(|days| TimestampMillis::from_millis(now.as_millis() + days * DAY_MS)),
        parent_task_id,
        milestone_id: optional_id(milestone_id)?,
        cycle_id: None,
        estimate: payload.estimate,
    })
}

fn encode(payload: &TaskPayload) -> Result<String, TaskError> {
    serde_json::to_string(payload).map_err(|_| TaskError::Invalid { field: "payload" })
}

fn decode(json: &str) -> Result<TaskPayload, TaskError> {
    serde_json::from_str(json).map_err(|_| TaskError::Conflict)
}

async fn template_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    project_id: Id,
    template_id: Id,
) -> Result<TemplateRecord, TaskError> {
    let row = sqlx::query(&format!(
        "SELECT {TEMPLATE_COLUMNS} FROM task_templates WHERE id = ? AND project_id = ?"
    ))
    .bind(template_id.to_string())
    .bind(project_id.to_string())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(TaskError::NotFound)?;
    template_from_row(row)
}

async fn recurring_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    project_id: Id,
    recurring_id: Id,
) -> Result<RecurringTaskRecord, TaskError> {
    let row = sqlx::query(&format!(
        "SELECT {RECURRING_COLUMNS} FROM recurring_tasks WHERE id = ? AND project_id = ?"
    ))
    .bind(recurring_id.to_string())
    .bind(project_id.to_string())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(TaskError::NotFound)?;
    recurring_from_row(row)
}

fn template_from_row(row: sqlx::sqlite::SqliteRow) -> Result<TemplateRecord, TaskError> {
    Ok(TemplateRecord {
        id: parse_id(row.get("id"))?,
        workspace_id: parse_id(row.get("workspace_id"))?,
        project_id: parse_id(row.get("project_id"))?,
        name: row.get("name"),
        payload: decode(row.get("payload_json"))?,
        position: row.get("position"),
        version: parse_version(row.get("version"))?,
        created_at: TimestampMillis::from_millis(row.get("created_at")),
        updated_at: TimestampMillis::from_millis(row.get("updated_at")),
    })
}

fn recurring_from_row(row: sqlx::sqlite::SqliteRow) -> Result<RecurringTaskRecord, TaskError> {
    Ok(RecurringTaskRecord {
        id: parse_id(row.get("id"))?,
        workspace_id: parse_id(row.get("workspace_id"))?,
        project_id: parse_id(row.get("project_id"))?,
        payload: decode(row.get("payload_json"))?,
        mode: row.get("mode"),
        every_count: row.get("every_count"),
        every_unit: row.get("every_unit"),
        anchor_at: TimestampMillis::from_millis(row.get("anchor_at")),
        next_run_at: row
            .get::<Option<i64>, _>("next_run_at")
            .map(TimestampMillis::from_millis),
        last_task_id: optional_id(row.get("last_task_id"))?,
        paused: row.get("paused"),
        version: parse_version(row.get("version"))?,
        created_at: TimestampMillis::from_millis(row.get("created_at")),
        updated_at: TimestampMillis::from_millis(row.get("updated_at")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn utc(year: i64, month: i64, day: i64) -> i64 {
        // Only for the dates of these tests: computed by `days_from_civil` below.
        year * 10_000 + month * 100 + day
    }

    fn at(date: i64) -> TimestampMillis {
        let (year, month, day) = (date / 10_000, date / 100 % 100, date % 100);
        TimestampMillis::from_millis(days_from_civil(year, month, day) * DAY_MS + 9 * 3_600_000)
    }

    fn date_of(time: TimestampMillis) -> i64 {
        let (year, month, day) = civil_from_days(time.as_millis().div_euclid(DAY_MS));
        utc(year, month, day)
    }

    #[test]
    fn civil_dates_round_trip() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(days_from_civil(2024, 2, 29)), (2024, 2, 29));
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2026, 2), 28);
        assert_eq!(days_in_month(2026, 12), 31);
    }

    #[test]
    fn month_intervals_keep_the_day_and_use_the_last_day_of_a_shorter_month() {
        let monthly = Interval::parse(1, "month").unwrap();
        let anchor = at(utc(2026, 1, 31));
        assert_eq!(date_of(monthly.add(anchor, 1)), utc(2026, 2, 28));
        assert_eq!(date_of(monthly.add(anchor, 2)), utc(2026, 3, 31));
        assert_eq!(date_of(monthly.add(anchor, 12)), utc(2027, 1, 31));
        // The time of day stays.
        assert_eq!(
            monthly.add(anchor, 1).as_millis().rem_euclid(DAY_MS),
            9 * 3_600_000
        );
    }

    #[test]
    fn a_schedule_does_not_drift_and_skips_missed_runs() {
        let weekly = Interval::parse(1, "week").unwrap();
        let anchor = at(utc(2026, 10, 5));
        // Before the anchor: the anchor itself.
        assert_eq!(weekly.next_after(anchor, at(utc(2026, 10, 1))), anchor);
        // A run that happens late keeps the schedule on the anchor's weekday and time.
        let late = TimestampMillis::from_millis(anchor.as_millis() + 3 * 3_600_000);
        assert_eq!(weekly.next_after(anchor, late), at(utc(2026, 10, 12)));
        // After three weeks of downtime: one next run, not three.
        assert_eq!(
            weekly.next_after(anchor, at(utc(2026, 10, 27))),
            TimestampMillis::from_millis(at(utc(2026, 11, 2)).as_millis())
        );
        let monthly = Interval::parse(2, "month").unwrap();
        assert_eq!(
            date_of(monthly.next_after(at(utc(2026, 1, 31)), at(utc(2026, 9, 1)))),
            utc(2026, 9, 30)
        );
    }

    #[test]
    fn a_payload_has_one_level_of_sub_issues_and_valid_fields() {
        let task = |title: &str| TaskPayload {
            title: title.to_owned(),
            priority: "none".to_owned(),
            ..TaskPayload::default()
        };
        let mut payload = task("Release");
        payload.sub_issues = vec![task("Changelog")];
        assert!(payload.validate().is_ok());
        payload.sub_issues[0].sub_issues = vec![task("Too deep")];
        assert!(payload.validate().is_err());
        assert!(task(" ").validate().is_err());
        let mut bad = task("x");
        bad.priority = "soon".to_owned();
        assert!(bad.validate().is_err());
        bad = task("x");
        bad.label_ids = vec!["not-an-id".to_owned()];
        assert!(bad.validate().is_err());
        bad = task("x");
        bad.due_offset_days = Some(-1);
        assert!(bad.validate().is_err());
    }
}
