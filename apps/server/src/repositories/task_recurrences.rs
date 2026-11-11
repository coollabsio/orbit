//! Recurring tasks. One `task_recurrences` row is the rule of a series; every occurrence links to
//! it. When an occurrence is closed (completed or cancelled), the sweep creates the next one in
//! the same transaction, copying the details of the closed one, and marks the closed one with
//! `recurrence_next_id`, so completing it again never creates a second successor.

mod schedule;

use jiff::Timestamp;
use jiff::civil::Weekday;
use jiff::tz::TimeZone;
use orbit_platform::{Database, Id, Row, TimestampMillis};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{Any, Transaction};
use utoipa::ToSchema;

pub use schedule::{Frequency, RecurrenceBase};
use schedule::{Schedule, WeekdaySet, next_due};

use super::billing::{TrialCapacityError, ensure_trial_capacity};
use super::project_access::{self, ProjectScope};
use super::sub_issues::AutomationActor;
use super::tasks::{
    CreateTask, NotificationRequest, TaskError, insert_task_rows_in_tx, notify_users, parse_id,
    task_in_tx,
};
use crate::audit::{self, AuditOutcome};
use crate::billing::TrialLimit;

/// How a task repeats, as clients read and write it. Shared by every occurrence of the series.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
#[schema(as = TaskRecurrence)]
pub struct TaskRecurrence {
    pub frequency: Frequency,
    /// Every `interval` days, weeks, months or years (1 to 365).
    #[schema(minimum = 1, maximum = 365)]
    pub interval: u16,
    /// Weekly rules from the due date only: the weekdays it falls on. Empty means the weekday of
    /// the due date the rule was set from.
    #[serde(default)]
    #[schema(required = true)]
    pub weekdays: Vec<RecurrenceWeekday>,
    /// `due`: a fixed calendar counted from the due date (missed dates are skipped). `completion`:
    /// the next due date is `interval` units after the day the task was completed.
    pub base: RecurrenceBase,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
#[schema(as = TaskRecurrenceWeekday)]
pub enum RecurrenceWeekday {
    Mon,
    Tue,
    Wed,
    Thu,
    Fri,
    Sat,
    Sun,
}

/// Monday first, as the weekdays of a rule are listed.
const WEEKDAYS: [(RecurrenceWeekday, Weekday); 7] = [
    (RecurrenceWeekday::Mon, Weekday::Monday),
    (RecurrenceWeekday::Tue, Weekday::Tuesday),
    (RecurrenceWeekday::Wed, Weekday::Wednesday),
    (RecurrenceWeekday::Thu, Weekday::Thursday),
    (RecurrenceWeekday::Fri, Weekday::Friday),
    (RecurrenceWeekday::Sat, Weekday::Saturday),
    (RecurrenceWeekday::Sun, Weekday::Sunday),
];

impl RecurrenceWeekday {
    fn civil(self) -> Weekday {
        WEEKDAYS[self as usize].1
    }
}

/// A checked rule, as stored (without the anchor and time zone the server adds).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecurrenceRule {
    frequency: Frequency,
    interval: u16,
    weekdays: WeekdaySet,
    base: RecurrenceBase,
}

impl TaskRecurrence {
    /// The rule this input stands for: `interval` is 1 to 365, and weekdays belong to weekly
    /// rules on a fixed calendar only.
    pub fn rule(&self) -> Result<RecurrenceRule, TaskError> {
        let weekdays = self
            .weekdays
            .iter()
            .fold(WeekdaySet::default(), |set, day| set.with(day.civil()));
        let takes_weekdays =
            self.frequency == Frequency::Weekly && self.base == RecurrenceBase::Due;
        if !(1..=365).contains(&self.interval) || (!weekdays.is_empty() && !takes_weekdays) {
            return Err(TaskError::Invalid {
                field: "recurrence",
            });
        }
        Ok(RecurrenceRule {
            frequency: self.frequency,
            interval: self.interval,
            weekdays,
            base: self.base,
        })
    }
}

fn frequency_name(frequency: Frequency) -> &'static str {
    match frequency {
        Frequency::Daily => "daily",
        Frequency::Weekly => "weekly",
        Frequency::Monthly => "monthly",
        Frequency::Yearly => "yearly",
    }
}

fn base_name(base: RecurrenceBase) -> &'static str {
    match base {
        RecurrenceBase::Due => "due",
        RecurrenceBase::Completion => "completion",
    }
}

fn parse_rule(frequency: &str, interval: i64, weekdays: i64, base: &str) -> Option<RecurrenceRule> {
    Some(RecurrenceRule {
        frequency: match frequency {
            "daily" => Frequency::Daily,
            "weekly" => Frequency::Weekly,
            "monthly" => Frequency::Monthly,
            "yearly" => Frequency::Yearly,
            _ => return None,
        },
        interval: u16::try_from(interval).ok()?,
        weekdays: WeekdaySet::from_bits(weekdays),
        base: match base {
            "due" => RecurrenceBase::Due,
            "completion" => RecurrenceBase::Completion,
            _ => return None,
        },
    })
}

impl From<RecurrenceRule> for TaskRecurrence {
    fn from(rule: RecurrenceRule) -> Self {
        Self {
            frequency: rule.frequency,
            interval: rule.interval,
            weekdays: WEEKDAYS
                .iter()
                .filter(|(_, day)| rule.weekdays.contains(*day))
                .map(|(name, _)| *name)
                .collect(),
            base: rule.base,
        }
    }
}

/// The `json_object` a task query selects as `recurrence_json` (see `from_json`).
pub(super) fn json_subquery(open: &str, close: &str) -> String {
    format!(
        "(SELECT {open}'frequency', task_recurrences.frequency, 'interval', task_recurrences.interval_count, \
         'weekdays', task_recurrences.weekdays, 'base', task_recurrences.base{close} \
         FROM task_recurrences WHERE task_recurrences.id = tasks.recurrence_id)"
    )
}

/// Decodes `json_subquery`'s column; `None` when the task does not repeat.
pub(super) fn from_json(json: Option<String>) -> Option<TaskRecurrence> {
    #[derive(Deserialize)]
    struct Stored {
        frequency: String,
        interval: i64,
        weekdays: i64,
        base: String,
    }
    let stored: Stored = serde_json::from_str(&json?).ok()?;
    parse_rule(
        &stored.frequency,
        stored.interval,
        stored.weekdays,
        &stored.base,
    )
    .map(Into::into)
}

/// Sets, replaces (`Some`) or removes (`None`) the rule of `task_id`'s series. A new rule is
/// anchored on `due_at` (else now) in the actor's time zone; a replaced one moves its anchor to
/// `due_at` when the task has one. Removing the rule stops every occurrence of the series.
pub(super) async fn set_in_tx(
    tx: &mut Transaction<'_, Any>,
    workspace_id: Id,
    task_id: Id,
    actor_id: Id,
    rule: Option<RecurrenceRule>,
    due_at: Option<TimestampMillis>,
    now: TimestampMillis,
) -> Result<(), TaskError> {
    let current: Option<String> =
        sqlx::query_scalar("SELECT recurrence_id FROM tasks WHERE id = $1")
            .bind(task_id.to_string())
            .fetch_one(&mut **tx)
            .await?;
    match (rule, current) {
        (None, None) => {}
        (None, Some(recurrence_id)) => {
            sqlx::query("DELETE FROM task_recurrences WHERE id = $1 AND workspace_id = $2")
                .bind(recurrence_id)
                .bind(workspace_id.to_string())
                .execute(&mut **tx)
                .await?;
        }
        (Some(rule), Some(recurrence_id)) => {
            sqlx::query(
                "UPDATE task_recurrences SET frequency = $1, interval_count = $2, weekdays = $3, base = $4, \
                 anchor_at = COALESCE($5, anchor_at), updated_at = $6 WHERE id = $7 AND workspace_id = $8",
            )
            .bind(frequency_name(rule.frequency))
            .bind(i64::from(rule.interval))
            .bind(rule.weekdays.bits())
            .bind(base_name(rule.base))
            .bind(due_at.map(TimestampMillis::as_millis))
            .bind(now.as_millis())
            .bind(recurrence_id)
            .bind(workspace_id.to_string())
            .execute(&mut **tx)
            .await?;
        }
        (Some(rule), None) => {
            let timezone: Option<String> =
                sqlx::query_scalar("SELECT timezone FROM users WHERE id = $1")
                    .bind(actor_id.to_string())
                    .fetch_optional(&mut **tx)
                    .await?
                    .flatten();
            let timezone = timezone
                .filter(|name| TimeZone::get(name).is_ok())
                .unwrap_or_else(|| "UTC".to_owned());
            let id = Id::new_v7();
            sqlx::query(
                "INSERT INTO task_recurrences (id, workspace_id, frequency, interval_count, weekdays, base, anchor_at, timezone, created_at, updated_at) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
            )
            .bind(id.to_string())
            .bind(workspace_id.to_string())
            .bind(frequency_name(rule.frequency))
            .bind(i64::from(rule.interval))
            .bind(rule.weekdays.bits())
            .bind(base_name(rule.base))
            .bind(due_at.unwrap_or(now).as_millis())
            .bind(timezone)
            .bind(now.as_millis())
            .bind(now.as_millis())
            .execute(&mut **tx)
            .await?;
            sqlx::query("UPDATE tasks SET recurrence_id = $1 WHERE id = $2")
                .bind(id.to_string())
                .bind(task_id.to_string())
                .execute(&mut **tx)
                .await?;
        }
    }
    Ok(())
}

/// Creates the next occurrence of every closed occurrence of `actor.workspace_id` that has none
/// yet. Run inside each write transaction that can close a task, after the sub-issue automation
/// (an auto-closed parent can repeat too). Idempotent: whichever writer closed the task, the
/// next sweep converges on one successor per closed occurrence.
pub(super) async fn spawn_next_in_tx(
    database: &Database,
    tx: &mut Transaction<'_, Any>,
    actor: AutomationActor<'_>,
) -> Result<(), TaskError> {
    let pending = sqlx::query(
        "SELECT tasks.id, tasks.recurrence_id, tasks.completed_at, task_recurrences.frequency, task_recurrences.interval_count, \
         task_recurrences.weekdays, task_recurrences.base, task_recurrences.anchor_at, task_recurrences.timezone \
         FROM tasks \
         JOIN task_recurrences ON task_recurrences.id = tasks.recurrence_id \
         JOIN task_statuses ON task_statuses.id = tasks.status_id \
         JOIN projects ON projects.id = tasks.project_id \
         WHERE tasks.workspace_id = $1 AND tasks.recurrence_id IS NOT NULL AND tasks.recurrence_next_id IS NULL \
         AND tasks.deleted_at IS NULL AND projects.deleted_at IS NULL AND tasks.completed_at IS NOT NULL \
         AND task_statuses.category IN ('completed', 'cancelled') \
         ORDER BY tasks.id",
    )
    .bind(actor.workspace_id.to_string())
    .fetch_all(&mut **tx)
    .await?;
    for row in pending {
        let Some(rule) = parse_rule(
            &row.get::<String, _>("frequency"),
            row.get("interval_count"),
            row.get("weekdays"),
            &row.get::<String, _>("base"),
        ) else {
            continue;
        };
        let Some(anchor) = Timestamp::from_millisecond(row.get("anchor_at")).ok() else {
            continue;
        };
        let schedule = Schedule {
            frequency: rule.frequency,
            interval: rule.interval,
            weekdays: rule.weekdays,
            base: rule.base,
            anchor,
            timezone: TimeZone::get(&row.get::<String, _>("timezone")).unwrap_or(TimeZone::UTC),
        };
        let closed_id = parse_id(row.get("id"))?;
        let closed =
            task_in_tx(tx, ProjectScope::All, actor.workspace_id, closed_id, false).await?;
        let Some(completed_at) = Timestamp::from_millisecond(row.get("completed_at")).ok() else {
            continue;
        };
        let prev_due = closed
            .due_at
            .and_then(|due| Timestamp::from_millisecond(due.as_millis()).ok());
        let due_at = next_due(&schedule, prev_due, completed_at)
            .map(|due| TimestampMillis::from_millis(due.as_millisecond()));
        // The new occurrence keeps the length of a date range.
        let due_start_at = match (closed.due_start_at, closed.due_at, due_at) {
            (Some(start), Some(end), Some(next)) => Some(TimestampMillis::from_millis(
                next.as_millis() - (end.as_millis() - start.as_millis()),
            )),
            _ => None,
        };
        // An open status only: a successor created closed would repeat on every later write.
        let status_id: Option<String> = sqlx::query_scalar(
            "SELECT id FROM task_statuses WHERE workspace_id = $1 AND project_id = $2 \
             AND category IN ('unstarted', 'started') \
             ORDER BY CASE category WHEN 'unstarted' THEN 0 ELSE 1 END, position, id LIMIT 1",
        )
        .bind(actor.workspace_id.to_string())
        .bind(closed.project_id.to_string())
        .fetch_optional(&mut **tx)
        .await?;
        let Some(status_id) = status_id.map(parse_id).transpose()? else {
            continue;
        };
        // A full trial leaves the occurrence pending; the series continues once there is room.
        match ensure_trial_capacity(database, tx, actor.workspace_id, TrialLimit::Tasks, 1).await {
            Ok(()) => {}
            Err(TrialCapacityError::Reached(_)) => continue,
            Err(TrialCapacityError::Database(error)) => return Err(error.into()),
        }
        let mut assignee_ids = Vec::with_capacity(closed.assignee_ids.len());
        for id in &closed.assignee_ids {
            let assignable: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM memberships WHERE workspace_id = $1 AND user_id = $2 AND role <> 'guest'",
            )
            .bind(actor.workspace_id.to_string())
            .bind(id.to_string())
            .fetch_one(&mut **tx)
            .await?;
            // Nobody is assigned to a task of a project they do not see.
            if assignable == 1
                && project_access::user_sees_project(&mut **tx, closed.project_id, *id).await?
            {
                assignee_ids.push(*id);
            }
        }
        let input = CreateTask {
            project_id: closed.project_id,
            status_id,
            title: closed.title.clone(),
            description: closed.description.clone(),
            description_json: closed.description_json.clone(),
            source_url: None,
            priority: closed.priority.clone(),
            position: None,
            assignee_ids,
            label_ids: closed.label_ids.clone(),
            due_start_at,
            due_at,
            parent_task_id: closed.parent.as_ref().map(|parent| parent.id),
            milestone_id: closed.milestone_id,
            // a cycle is a time box: the next occurrence is due later, so it starts without one
            cycle_id: None,
            estimate: closed.estimate,
        };
        let next_id = Id::new_v7();
        insert_task_rows_in_tx(
            tx,
            next_id,
            actor.workspace_id,
            actor.actor_id,
            actor.service_account.map(|(id, _)| id),
            &input,
            None,
            Some(&row.get::<String, _>("recurrence_id")),
            actor.now,
        )
        .await?;
        sqlx::query("UPDATE tasks SET recurrence_next_id = $1 WHERE id = $2")
            .bind(next_id.to_string())
            .bind(closed_id.to_string())
            .execute(&mut **tx)
            .await?;
        let mut metadata = json!({ "recurrence_of": closed_id });
        let user = match actor.service_account {
            Some((id, name)) => {
                metadata["actor_service_account_id"] = json!(id);
                metadata["actor_service_account_name"] = json!(name);
                None
            }
            None => Some(actor.actor_id),
        };
        audit::record(
            tx,
            actor.workspace_id,
            user,
            "task.created",
            AuditOutcome::Success,
            "task",
            Some(next_id),
            actor.request_id,
            metadata,
            actor.now,
        )
        .await?;
        if actor.service_account.is_none() {
            notify_users(
                tx,
                actor.workspace_id,
                actor.actor_id,
                NotificationRequest {
                    kind: "task_assigned",
                    task_id: next_id,
                    comment_id: None,
                    recipients: &input.assignee_ids,
                },
                actor.now,
            )
            .await?;
        }
    }
    Ok(())
}
