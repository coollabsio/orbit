//! Cycles (spec `2026-10-08-cycles-estimates-design.md`): the repeating work periods of a project.
//! Orbit creates and closes them; a person changes the settings and can start or end one early.
//!
//! A cycle is the period `[starts_at, ends_at)`. It starts at 00:01 on its first day in the
//! project timezone and ends at 00:01 `weeks * 7` days later; after the cooldown comes the next
//! one. Every step of the job is idempotent.

use jiff::civil::Date;
use jiff::tz::TimeZone;
use jiff::{Timestamp, ToSpan};
use orbit_platform::{Id, TimestampMillis};
use serde::Serialize;
use serde_json::json;
use sqlx::{Row, Sqlite, Transaction};
use utoipa::ToSchema;

use super::tasks::{
    Page, TaskError, TaskRepository, check_version, parse_id, parse_version, record_mutation,
    require_access, require_project, require_project_tx,
};
use crate::audit::{self, AuditOutcome};

const CYCLES_ACCOUNT: &str = "Cycles";
/// Upper bound for the catch-up loop of one project in one job run (downtime of years).
const MAX_STEPS: usize = 1_000;

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct CycleSettingsRecord {
    #[schema(value_type = String)]
    pub project_id: Id,
    pub enabled: bool,
    /// Length of a cycle, 1 to 8.
    pub weeks: i64,
    /// 0 = Sunday ... 6 = Saturday.
    pub start_weekday: i64,
    /// Weeks between two cycles, 0 to 4.
    pub cooldown_weeks: i64,
    /// Future cycles that exist at each time, 1 to 15.
    pub cycles_ahead: i64,
    /// IANA name. A cycle starts and ends in this zone.
    pub timezone: String,
    /// A task with no cycle that moves to a started status gets the current cycle.
    pub auto_add_started: bool,
    /// A task with no cycle that moves to a completed status gets the current cycle.
    pub auto_add_completed: bool,
    /// What happens to an unstarted task with no cycle: `off`, `backlog` (it moves to the first
    /// backlog status) or `cycle` (it gets the current cycle).
    pub active_without_cycle: String,
    pub version: u64,
}

#[derive(Clone, Debug)]
pub struct CycleSettingsInput {
    pub enabled: bool,
    pub weeks: i64,
    pub start_weekday: i64,
    pub cooldown_weeks: i64,
    pub cycles_ahead: i64,
    pub timezone: String,
    pub auto_add_started: bool,
    pub auto_add_completed: bool,
    pub active_without_cycle: String,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct CycleRecord {
    #[schema(value_type = String)]
    pub id: Id,
    #[schema(value_type = String)]
    pub workspace_id: Id,
    #[schema(value_type = String)]
    pub project_id: Id,
    pub number: i64,
    /// Null: the default name "Cycle {number}".
    #[schema(required = true)]
    pub name: Option<String>,
    pub description: String,
    #[schema(value_type = String, format = DateTime)]
    pub starts_at: TimestampMillis,
    #[schema(value_type = String, format = DateTime)]
    pub ends_at: TimestampMillis,
    #[schema(value_type = Option<String>, format = DateTime, required = true)]
    pub completed_at: Option<TimestampMillis>,
    /// `current`, `future` or `completed`, at the time of the request.
    pub state: String,
    /// The scope: live tasks with no sub-issues, without cancelled and duplicate ones.
    pub scope_count: i64,
    pub scope_points: i64,
    pub started_count: i64,
    pub started_points: i64,
    pub done_count: i64,
    pub done_points: i64,
    /// Tasks of the scope that have no estimate; they count 0 points.
    pub unestimated_count: i64,
    pub version: u64,
}

/// `None` keeps the stored value.
#[derive(Clone, Debug, Default)]
pub struct CycleChanges {
    pub name: Option<Option<String>>,
    pub description: Option<String>,
    /// A future cycle only. The later future cycles move by the same amount as the end.
    pub starts_at: Option<TimestampMillis>,
    pub ends_at: Option<TimestampMillis>,
}

/// One day of a cycle, for the burndown.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct CycleDayRecord {
    /// The local date in the project timezone, `YYYY-MM-DD`.
    pub day: String,
    pub scope_count: i64,
    pub scope_points: i64,
    pub started_count: i64,
    pub started_points: i64,
    pub done_count: i64,
    pub done_points: i64,
}

/// What one run of the cycle job did.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CycleRun {
    pub created: usize,
    pub closed: usize,
    pub failed: usize,
}

// ---- schedule maths (pure) ----

/// 00:01 on `date` in `zone`. A time that a daylight saving change skips or repeats is resolved
/// the way `jiff` does by default (the later of the two for a gap, the first for a fold).
fn start_of_day(zone: &TimeZone, date: Date) -> Result<TimestampMillis, TaskError> {
    date.at(0, 1, 0, 0)
        .to_zoned(zone.clone())
        .map(|zoned| TimestampMillis::from_millis(zoned.timestamp().as_millisecond()))
        .map_err(|_| TaskError::Invalid { field: "timezone" })
}

fn local_date(zone: &TimeZone, at: TimestampMillis) -> Result<Date, TaskError> {
    Timestamp::from_millisecond(at.as_millis())
        .map(|timestamp| timestamp.to_zoned(zone.clone()).date())
        .map_err(|_| TaskError::Invalid { field: "timezone" })
}

fn add_days(date: Date, days: i64) -> Result<Date, TaskError> {
    date.checked_add(days.days())
        .map_err(|_| TaskError::Invalid { field: "starts_at" })
}

/// The most recent day with `weekday` (0 = Sunday), `today` included.
fn latest_weekday(today: Date, weekday: i64) -> Result<Date, TaskError> {
    let current = i64::from(today.weekday().to_sunday_zero_offset());
    add_days(today, -((current - weekday).rem_euclid(7)))
}

/// The period of a cycle that starts on `start` and is `weeks` long.
fn bounds(
    zone: &TimeZone,
    start: Date,
    weeks: i64,
) -> Result<(TimestampMillis, TimestampMillis), TaskError> {
    Ok((
        start_of_day(zone, start)?,
        start_of_day(zone, add_days(start, weeks * 7)?)?,
    ))
}

fn zone(name: &str) -> Result<TimeZone, TaskError> {
    TimeZone::get(name).map_err(|_| TaskError::Invalid { field: "timezone" })
}

fn validate_settings(input: &CycleSettingsInput) -> Result<(), TaskError> {
    let field = if !(1..=8).contains(&input.weeks) {
        "weeks"
    } else if !(0..=6).contains(&input.start_weekday) {
        "start_weekday"
    } else if !(0..=4).contains(&input.cooldown_weeks) {
        "cooldown_weeks"
    } else if !(1..=15).contains(&input.cycles_ahead) {
        "cycles_ahead"
    } else if !matches!(
        input.active_without_cycle.as_str(),
        "off" | "backlog" | "cycle"
    ) {
        "active_without_cycle"
    } else if input.timezone.len() > 64 || zone(&input.timezone).is_err() {
        "timezone"
    } else {
        return Ok(());
    };
    Err(TaskError::Invalid { field })
}

// ---- SQL ----

const SETTINGS_COLUMNS: &str = "project_id, enabled, weeks, start_weekday, cooldown_weeks, \
     cycles_ahead, timezone, auto_add_started, auto_add_completed, active_without_cycle, version";

/// The totals of every cycle, joined as `totals`. Scope: live tasks with no live sub-issue,
/// without cancelled and duplicate ones.
const TOTALS: &str = "SELECT tasks.cycle_id AS cycle_id, COUNT(*) AS scope_count, \
     COALESCE(SUM(tasks.estimate), 0) AS scope_points, \
     COALESCE(SUM(task_statuses.category = 'started'), 0) AS started_count, \
     COALESCE(SUM(CASE WHEN task_statuses.category = 'started' THEN tasks.estimate END), 0) AS started_points, \
     COALESCE(SUM(task_statuses.category = 'completed'), 0) AS done_count, \
     COALESCE(SUM(CASE WHEN task_statuses.category = 'completed' THEN tasks.estimate END), 0) AS done_points, \
     COALESCE(SUM(tasks.estimate IS NULL), 0) AS unestimated_count \
     FROM tasks JOIN task_statuses ON task_statuses.id = tasks.status_id \
     WHERE tasks.cycle_id IS NOT NULL AND tasks.deleted_at IS NULL \
     AND task_statuses.category NOT IN ('cancelled', 'duplicate') \
     AND NOT EXISTS (SELECT 1 FROM tasks AS child \
                     WHERE child.parent_task_id = tasks.id AND child.deleted_at IS NULL) \
     GROUP BY tasks.cycle_id";

fn cycle_select() -> String {
    format!(
        "SELECT cycles.id, cycles.workspace_id, cycles.project_id, cycles.number, cycles.name, \
         cycles.description, cycles.starts_at, cycles.ends_at, cycles.completed_at, cycles.version, \
         COALESCE(totals.scope_count, 0) AS scope_count, COALESCE(totals.scope_points, 0) AS scope_points, \
         COALESCE(totals.started_count, 0) AS started_count, COALESCE(totals.started_points, 0) AS started_points, \
         COALESCE(totals.done_count, 0) AS done_count, COALESCE(totals.done_points, 0) AS done_points, \
         COALESCE(totals.unestimated_count, 0) AS unestimated_count \
         FROM cycles LEFT JOIN ({TOTALS}) AS totals ON totals.cycle_id = cycles.id"
    )
}

impl TaskRepository {
    /// The cycle settings of a project; the defaults (cycles off) while it has never used cycles.
    pub async fn cycle_settings(
        &self,
        workspace_id: Id,
        project_id: Id,
        actor_id: Id,
    ) -> Result<CycleSettingsRecord, TaskError> {
        let pool = self.database().pool();
        require_project(pool, workspace_id, project_id, actor_id).await?;
        let row = sqlx::query(&format!(
            "SELECT {SETTINGS_COLUMNS} FROM project_cycle_settings WHERE project_id = ?"
        ))
        .bind(project_id.to_string())
        .fetch_optional(pool)
        .await?;
        match row {
            Some(row) => settings_from_row(row),
            None => Ok(default_settings(project_id)),
        }
    }

    /// Saves the settings and applies them at once: future cycles that have no tasks are made
    /// again from the new settings; future cycles that have tasks keep their dates. Turning
    /// cycles off completes the current cycle (its open tasks lose their cycle) and deletes the
    /// future ones; completed cycles stay.
    #[allow(clippy::too_many_arguments)]
    pub async fn update_cycle_settings(
        &self,
        workspace_id: Id,
        project_id: Id,
        actor_id: Id,
        input: CycleSettingsInput,
        expected_version: u64,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<CycleSettingsRecord, TaskError> {
        validate_settings(&input)?;
        let mut tx = self.database().immediate_transaction().await?;
        require_project_tx(&mut tx, workspace_id, project_id, actor_id).await?;
        let current = settings_in_tx(&mut tx, project_id).await?;
        check_version(expected_version, current.version, &current)?;
        if input.active_without_cycle == "backlog" {
            let backlog: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT 1 FROM task_statuses WHERE project_id = ? AND category = 'backlog')",
            )
            .bind(project_id.to_string())
            .fetch_one(&mut *tx)
            .await?;
            if !backlog {
                return Err(TaskError::Invalid {
                    field: "active_without_cycle",
                });
            }
        }
        sqlx::query(
            "INSERT INTO project_cycle_settings (project_id, enabled, weeks, start_weekday, cooldown_weeks, \
             cycles_ahead, timezone, auto_add_started, auto_add_completed, active_without_cycle, version, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?) \
             ON CONFLICT(project_id) DO UPDATE SET enabled = excluded.enabled, weeks = excluded.weeks, \
             start_weekday = excluded.start_weekday, cooldown_weeks = excluded.cooldown_weeks, \
             cycles_ahead = excluded.cycles_ahead, timezone = excluded.timezone, \
             auto_add_started = excluded.auto_add_started, auto_add_completed = excluded.auto_add_completed, \
             active_without_cycle = excluded.active_without_cycle, version = version + 1, \
             updated_at = excluded.updated_at",
        )
        .bind(project_id.to_string())
        .bind(input.enabled)
        .bind(input.weeks)
        .bind(input.start_weekday)
        .bind(input.cooldown_weeks)
        .bind(input.cycles_ahead)
        .bind(&input.timezone)
        .bind(input.auto_add_started)
        .bind(input.auto_add_completed)
        .bind(&input.active_without_cycle)
        .bind(now.as_millis())
        .execute(&mut *tx)
        .await?;
        let settings = settings_in_tx(&mut tx, project_id).await?;
        if settings.enabled {
            // Future cycles with no tasks, later than every cycle that must keep its dates.
            sqlx::query(
                "DELETE FROM cycles WHERE project_id = ?1 AND starts_at > ?2 \
                 AND starts_at > COALESCE((SELECT MAX(kept.starts_at) FROM cycles AS kept \
                      WHERE kept.project_id = ?1 AND (kept.starts_at <= ?2 \
                      OR EXISTS (SELECT 1 FROM tasks WHERE tasks.cycle_id = kept.id))), 0)",
            )
            .bind(project_id.to_string())
            .bind(now.as_millis())
            .execute(&mut *tx)
            .await?;
            advance_in_tx(
                &mut tx,
                workspace_id,
                &settings,
                now,
                &mut CycleRun::default(),
            )
            .await?;
        } else if current.enabled {
            turn_off_in_tx(&mut tx, workspace_id, &settings, now).await?;
        }
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "project.cycle_settings_updated",
            "project",
            project_id,
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(settings)
    }

    /// Every cycle of a project with its totals, in time order.
    pub async fn cycles(
        &self,
        workspace_id: Id,
        project_id: Id,
        actor_id: Id,
        now: TimestampMillis,
    ) -> Result<Page<CycleRecord>, TaskError> {
        let pool = self.database().pool();
        require_project(pool, workspace_id, project_id, actor_id).await?;
        let items = sqlx::query(&format!(
            "{} WHERE cycles.project_id = ? ORDER BY cycles.starts_at, cycles.number",
            cycle_select()
        ))
        .bind(project_id.to_string())
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(|row| cycle_from_row(row, now))
        .collect::<Result<Vec<_>, _>>()?;
        Ok(Page {
            items,
            next_cursor: None,
        })
    }

    /// The current cycle of each live project that has one.
    pub async fn current_cycles(
        &self,
        workspace_id: Id,
        actor_id: Id,
        now: TimestampMillis,
    ) -> Result<Page<CycleRecord>, TaskError> {
        let pool = self.database().pool();
        require_access(pool, workspace_id, actor_id).await?;
        let items = sqlx::query(&format!(
            "{} JOIN projects ON projects.id = cycles.project_id \
             WHERE cycles.workspace_id = ? AND projects.deleted_at IS NULL \
             AND cycles.completed_at IS NULL AND cycles.starts_at <= ? AND cycles.ends_at > ? \
             ORDER BY cycles.project_id",
            cycle_select()
        ))
        .bind(workspace_id.to_string())
        .bind(now.as_millis())
        .bind(now.as_millis())
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(|row| cycle_from_row(row, now))
        .collect::<Result<Vec<_>, _>>()?;
        Ok(Page {
            items,
            next_cursor: None,
        })
    }

    /// The daily snapshots of a cycle, oldest first.
    pub async fn cycle_days(
        &self,
        workspace_id: Id,
        project_id: Id,
        cycle_id: Id,
        actor_id: Id,
    ) -> Result<Page<CycleDayRecord>, TaskError> {
        let pool = self.database().pool();
        require_project(pool, workspace_id, project_id, actor_id).await?;
        let items = sqlx::query(
            "SELECT cycle_days.day, scope_count, scope_points, started_count, started_points, done_count, \
             done_points FROM cycle_days JOIN cycles ON cycles.id = cycle_days.cycle_id \
             WHERE cycle_days.cycle_id = ? AND cycles.project_id = ? ORDER BY cycle_days.day",
        )
        .bind(cycle_id.to_string())
        .bind(project_id.to_string())
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(|row| CycleDayRecord {
            day: row.get("day"),
            scope_count: row.get("scope_count"),
            scope_points: row.get("scope_points"),
            started_count: row.get("started_count"),
            started_points: row.get("started_points"),
            done_count: row.get("done_count"),
            done_points: row.get("done_points"),
        })
        .collect();
        Ok(Page {
            items,
            next_cursor: None,
        })
    }

    /// Rename, describe, or move a cycle that is not completed. Only a future cycle can change
    /// its dates: it must not start before the cycle before it ends, and the later future cycles
    /// move by the same amount as its end.
    #[allow(clippy::too_many_arguments)]
    pub async fn update_cycle(
        &self,
        workspace_id: Id,
        project_id: Id,
        cycle_id: Id,
        actor_id: Id,
        changes: CycleChanges,
        expected_version: u64,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<CycleRecord, TaskError> {
        let mut tx = self.database().immediate_transaction().await?;
        require_project_tx(&mut tx, workspace_id, project_id, actor_id).await?;
        let current = cycle_in_tx(&mut tx, project_id, cycle_id, now).await?;
        check_version(expected_version, current.version, &current)?;
        if current.completed_at.is_some() {
            return Err(TaskError::Conflict);
        }
        let name = changes.name.unwrap_or(current.name.clone());
        let description = changes.description.unwrap_or(current.description.clone());
        let starts_at = changes.starts_at.unwrap_or(current.starts_at);
        let ends_at = changes.ends_at.unwrap_or(current.ends_at);
        let moved = starts_at != current.starts_at || ends_at != current.ends_at;
        if moved {
            if current.state != "future" || starts_at <= now {
                return Err(TaskError::Invalid { field: "starts_at" });
            }
            if ends_at <= starts_at {
                return Err(TaskError::Invalid { field: "ends_at" });
            }
            let previous_end: Option<i64> = sqlx::query_scalar(
                "SELECT MAX(ends_at) FROM cycles WHERE project_id = ? AND starts_at < ? AND id <> ?",
            )
            .bind(project_id.to_string())
            .bind(current.starts_at.as_millis())
            .bind(cycle_id.to_string())
            .fetch_one(&mut *tx)
            .await?;
            if previous_end.is_some_and(|end| starts_at.as_millis() < end) {
                return Err(TaskError::Invalid { field: "starts_at" });
            }
            shift_later_in_tx(
                &mut tx,
                project_id,
                current.starts_at,
                ends_at.as_millis() - current.ends_at.as_millis(),
                now,
            )
            .await?;
        }
        sqlx::query(
            "UPDATE cycles SET name = ?, description = ?, starts_at = ?, ends_at = ?, \
             version = version + 1, updated_at = ? WHERE id = ? AND version = ?",
        )
        .bind(&name)
        .bind(&description)
        .bind(starts_at.as_millis())
        .bind(ends_at.as_millis())
        .bind(now.as_millis())
        .bind(cycle_id.to_string())
        .bind(expected_version as i64)
        .execute(&mut *tx)
        .await?;
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "cycle.updated",
            "cycle",
            cycle_id,
            request_id,
            now,
        )
        .await?;
        let record = cycle_in_tx(&mut tx, project_id, cycle_id, now).await?;
        tx.commit().await?;
        Ok(record)
    }

    /// Start the next cycle today: the current cycle (if there is one) ends now and closes by the
    /// normal rules; this cycle starts now and keeps its length; the later cycles move with it.
    pub async fn start_cycle_today(
        &self,
        workspace_id: Id,
        project_id: Id,
        cycle_id: Id,
        actor_id: Id,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<CycleRecord, TaskError> {
        let mut tx = self.database().immediate_transaction().await?;
        require_project_tx(&mut tx, workspace_id, project_id, actor_id).await?;
        let settings = settings_in_tx(&mut tx, project_id).await?;
        let cycle = cycle_in_tx(&mut tx, project_id, cycle_id, now).await?;
        let next: Option<String> = sqlx::query_scalar(
            "SELECT id FROM cycles WHERE project_id = ? AND starts_at > ? ORDER BY starts_at LIMIT 1",
        )
        .bind(project_id.to_string())
        .bind(now.as_millis())
        .fetch_optional(&mut *tx)
        .await?;
        if !settings.enabled || next.as_deref() != Some(cycle_id.to_string().as_str()) {
            return Err(TaskError::Conflict);
        }
        let delta = now.as_millis() - cycle.starts_at.as_millis();
        // The current cycle ends at this instant, so the close below finds it.
        sqlx::query(
            "UPDATE cycles SET ends_at = ?, version = version + 1, updated_at = ? \
             WHERE project_id = ? AND completed_at IS NULL AND starts_at <= ? AND ends_at > ?",
        )
        .bind(now.as_millis())
        .bind(now.as_millis())
        .bind(project_id.to_string())
        .bind(now.as_millis())
        .bind(now.as_millis())
        .execute(&mut *tx)
        .await?;
        shift_later_in_tx(&mut tx, project_id, cycle.starts_at, delta, now).await?;
        sqlx::query(
            "UPDATE cycles SET starts_at = starts_at + ?, ends_at = ends_at + ?, \
             version = version + 1, updated_at = ? WHERE id = ?",
        )
        .bind(delta)
        .bind(delta)
        .bind(now.as_millis())
        .bind(cycle_id.to_string())
        .execute(&mut *tx)
        .await?;
        advance_in_tx(
            &mut tx,
            workspace_id,
            &settings,
            now,
            &mut CycleRun::default(),
        )
        .await?;
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "cycle.started_early",
            "cycle",
            cycle_id,
            request_id,
            now,
        )
        .await?;
        let record = cycle_in_tx(&mut tx, project_id, cycle_id, now).await?;
        tx.commit().await?;
        Ok(record)
    }

    /// End the current cycle today: it ends at the end of today in the project timezone (never
    /// after the start of the next cycle). The next cycle keeps its dates.
    pub async fn end_cycle_today(
        &self,
        workspace_id: Id,
        project_id: Id,
        cycle_id: Id,
        actor_id: Id,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<CycleRecord, TaskError> {
        let mut tx = self.database().immediate_transaction().await?;
        require_project_tx(&mut tx, workspace_id, project_id, actor_id).await?;
        let settings = settings_in_tx(&mut tx, project_id).await?;
        let cycle = cycle_in_tx(&mut tx, project_id, cycle_id, now).await?;
        if cycle.state != "current" {
            return Err(TaskError::Conflict);
        }
        let zone = zone(&settings.timezone)?;
        let tomorrow = start_of_day(&zone, add_days(local_date(&zone, now)?, 1)?)?;
        let next_start: Option<i64> = sqlx::query_scalar(
            "SELECT MIN(starts_at) FROM cycles WHERE project_id = ? AND starts_at > ?",
        )
        .bind(project_id.to_string())
        .bind(now.as_millis())
        .fetch_one(&mut *tx)
        .await?;
        let ends_at = [
            Some(tomorrow.as_millis()),
            next_start,
            Some(cycle.ends_at.as_millis()),
        ]
        .into_iter()
        .flatten()
        .min()
        .unwrap_or(tomorrow.as_millis());
        sqlx::query(
            "UPDATE cycles SET ends_at = ?, version = version + 1, updated_at = ? WHERE id = ?",
        )
        .bind(ends_at)
        .bind(now.as_millis())
        .bind(cycle_id.to_string())
        .execute(&mut *tx)
        .await?;
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "cycle.ended_early",
            "cycle",
            cycle_id,
            request_id,
            now,
        )
        .await?;
        let record = cycle_in_tx(&mut tx, project_id, cycle_id, now).await?;
        tx.commit().await?;
        Ok(record)
    }

    /// The cycle job: for each live project with cycles on, create the future cycles, close the
    /// ended ones (and move their open tasks), write the daily snapshot, and apply the option for
    /// active tasks with no cycle. Each project has its own transaction.
    pub async fn run_cycles(&self, now: TimestampMillis) -> Result<CycleRun, TaskError> {
        let projects: Vec<(String, String)> = sqlx::query_as(
            "SELECT projects.id, projects.workspace_id FROM project_cycle_settings \
             JOIN projects ON projects.id = project_cycle_settings.project_id \
             JOIN workspaces ON workspaces.id = projects.workspace_id \
             WHERE project_cycle_settings.enabled = 1 AND projects.deleted_at IS NULL \
             AND workspaces.deleted_at IS NULL ORDER BY projects.id",
        )
        .fetch_all(self.database().pool())
        .await?;
        let mut run = CycleRun::default();
        for (project_id, workspace_id) in projects {
            let outcome: Result<CycleRun, TaskError> = async {
                let mut tx = self.database().immediate_transaction().await?;
                let settings = settings_in_tx(&mut tx, parse_id(project_id)?).await?;
                let mut step = CycleRun::default();
                advance_in_tx(&mut tx, parse_id(workspace_id)?, &settings, now, &mut step).await?;
                tx.commit().await?;
                Ok(step)
            }
            .await;
            match outcome {
                Ok(step) => {
                    run.created += step.created;
                    run.closed += step.closed;
                }
                Err(_) => run.failed += 1,
            }
        }
        Ok(run)
    }
}

/// A task's cycle must be a cycle of the task's project that is not completed.
pub(super) async fn validate_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    project_id: Id,
    cycle_id: Id,
) -> Result<(), TaskError> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM cycles WHERE id = ? AND project_id = ? AND completed_at IS NULL)",
    )
    .bind(cycle_id.to_string())
    .bind(project_id.to_string())
    .fetch_one(&mut **tx)
    .await?;
    if exists {
        Ok(())
    } else {
        Err(TaskError::Invalid { field: "cycle_id" })
    }
}

/// The "tasks with no cycle" options, in the status change path: called after a task of
/// `project_id` moved to `status_id`. Does nothing for a task that has a cycle or a project
/// with cycles off.
pub(super) async fn apply_options_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    task_id: Id,
    project_id: Id,
    status_id: Id,
    now: TimestampMillis,
) -> Result<(), TaskError> {
    let settings = settings_in_tx(tx, project_id).await?;
    if !settings.enabled {
        return Ok(());
    }
    let row = sqlx::query(
        "SELECT tasks.cycle_id, task_statuses.category FROM tasks \
         JOIN task_statuses ON task_statuses.id = ? WHERE tasks.id = ?",
    )
    .bind(status_id.to_string())
    .bind(task_id.to_string())
    .fetch_one(&mut **tx)
    .await?;
    if row.get::<Option<String>, _>("cycle_id").is_some() {
        return Ok(());
    }
    let category: String = row.get("category");
    let target = match category.as_str() {
        "started" if settings.auto_add_started => {
            current_or(tx, project_id, now, Fallback::Next).await?
        }
        "completed" if settings.auto_add_completed => {
            current_or(tx, project_id, now, Fallback::Previous).await?
        }
        "unstarted" if settings.active_without_cycle == "cycle" => {
            current_or(tx, project_id, now, Fallback::Next).await?
        }
        "unstarted" if settings.active_without_cycle == "backlog" => {
            sqlx::query(
                "UPDATE tasks SET status_id = (SELECT id FROM task_statuses WHERE project_id = ?1 \
                 AND category = 'backlog' ORDER BY position, id LIMIT 1) \
                 WHERE id = ?2 AND EXISTS (SELECT 1 FROM task_statuses WHERE project_id = ?1 AND category = 'backlog')",
            )
            .bind(project_id.to_string())
            .bind(task_id.to_string())
            .execute(&mut **tx)
            .await?;
            None
        }
        _ => None,
    };
    if let Some(cycle_id) = target {
        sqlx::query("UPDATE tasks SET cycle_id = ? WHERE id = ?")
            .bind(cycle_id)
            .bind(task_id.to_string())
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum Fallback {
    /// During a cooldown: the next cycle.
    Next,
    /// During a cooldown: the cycle before it.
    Previous,
}

/// The current cycle of the project, or the fallback while it has none (a cooldown).
async fn current_or(
    tx: &mut Transaction<'_, Sqlite>,
    project_id: Id,
    now: TimestampMillis,
    fallback: Fallback,
) -> Result<Option<String>, TaskError> {
    let current: Option<String> = sqlx::query_scalar(
        "SELECT id FROM cycles WHERE project_id = ? AND completed_at IS NULL AND starts_at <= ? \
         AND ends_at > ? ORDER BY starts_at LIMIT 1",
    )
    .bind(project_id.to_string())
    .bind(now.as_millis())
    .bind(now.as_millis())
    .fetch_optional(&mut **tx)
    .await?;
    if current.is_some() {
        return Ok(current);
    }
    let sql = match fallback {
        Fallback::Next => {
            "SELECT id FROM cycles WHERE project_id = ? AND starts_at > ? ORDER BY starts_at LIMIT 1"
        }
        Fallback::Previous => {
            "SELECT id FROM cycles WHERE project_id = ? AND ends_at <= ? ORDER BY ends_at DESC LIMIT 1"
        }
    };
    Ok(sqlx::query_scalar(sql)
        .bind(project_id.to_string())
        .bind(now.as_millis())
        .fetch_optional(&mut **tx)
        .await?)
}

/// The four steps of the job for one project, until nothing is left to do.
async fn advance_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    settings: &CycleSettingsRecord,
    now: TimestampMillis,
    run: &mut CycleRun,
) -> Result<(), TaskError> {
    let zone = zone(&settings.timezone)?;
    let project = settings.project_id.to_string();
    for _ in 0..MAX_STEPS {
        ensure_ahead_in_tx(tx, workspace_id, settings, &zone, now, run).await?;
        let overdue: Option<String> = sqlx::query_scalar(
            "SELECT id FROM cycles WHERE project_id = ? AND completed_at IS NULL AND ends_at <= ? \
             ORDER BY starts_at LIMIT 1",
        )
        .bind(&project)
        .bind(now.as_millis())
        .fetch_optional(&mut **tx)
        .await?;
        let Some(cycle_id) = overdue else { break };
        close_in_tx(tx, workspace_id, &zone, &cycle_id, true, now).await?;
        run.closed += 1;
    }
    // The snapshot of today for the current cycle.
    let current: Option<String> = sqlx::query_scalar(
        "SELECT id FROM cycles WHERE project_id = ? AND completed_at IS NULL AND starts_at <= ? AND ends_at > ?",
    )
    .bind(&project)
    .bind(now.as_millis())
    .bind(now.as_millis())
    .fetch_optional(&mut **tx)
    .await?;
    if let Some(cycle_id) = &current {
        snapshot_in_tx(tx, cycle_id, &local_date(&zone, now)?.to_string()).await?;
    }
    // Active tasks with no cycle (tasks created with none; the status path covers the others).
    match settings.active_without_cycle.as_str() {
        "cycle" => {
            if let Some(cycle_id) = current_or(tx, settings.project_id, now, Fallback::Next).await?
            {
                sqlx::query(
                    "UPDATE tasks SET cycle_id = ?1 WHERE project_id = ?2 AND cycle_id IS NULL \
                     AND deleted_at IS NULL AND status_id IN \
                     (SELECT id FROM task_statuses WHERE project_id = ?2 AND category = 'unstarted')",
                )
                .bind(cycle_id)
                .bind(&project)
                .execute(&mut **tx)
                .await?;
            }
        }
        "backlog" => {
            sqlx::query(
                "UPDATE tasks SET status_id = (SELECT id FROM task_statuses WHERE project_id = ?1 \
                 AND category = 'backlog' ORDER BY position, id LIMIT 1) \
                 WHERE project_id = ?1 AND cycle_id IS NULL AND deleted_at IS NULL AND status_id IN \
                 (SELECT id FROM task_statuses WHERE project_id = ?1 AND category = 'unstarted') \
                 AND EXISTS (SELECT 1 FROM task_statuses WHERE project_id = ?1 AND category = 'backlog')",
            )
            .bind(&project)
            .execute(&mut **tx)
            .await?;
        }
        _ => {}
    }
    Ok(())
}

/// Creates cycles until `cycles_ahead` future cycles exist. A project with no cycle gets a first
/// one that starts on the most recent start weekday, so it has a current cycle at once.
async fn ensure_ahead_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    settings: &CycleSettingsRecord,
    zone: &TimeZone,
    now: TimestampMillis,
    run: &mut CycleRun,
) -> Result<(), TaskError> {
    let project = settings.project_id.to_string();
    for _ in 0..MAX_STEPS {
        let last: Option<(i64, i64)> = sqlx::query_as(
            "SELECT ends_at, number FROM cycles WHERE project_id = ? ORDER BY starts_at DESC LIMIT 1",
        )
        .bind(&project)
        .fetch_optional(&mut **tx)
        .await?;
        let start = match last {
            None => latest_weekday(local_date(zone, now)?, settings.start_weekday)?,
            Some((ends_at, _)) => {
                let future: i64 = sqlx::query_scalar(
                    "SELECT COUNT(*) FROM cycles WHERE project_id = ? AND starts_at > ?",
                )
                .bind(&project)
                .bind(now.as_millis())
                .fetch_one(&mut **tx)
                .await?;
                if future >= settings.cycles_ahead {
                    return Ok(());
                }
                add_days(
                    local_date(zone, TimestampMillis::from_millis(ends_at))?,
                    settings.cooldown_weeks * 7,
                )?
            }
        };
        let (starts_at, ends_at) = bounds(zone, start, settings.weeks)?;
        let number: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(number), 0) + 1 FROM cycles WHERE project_id = ?",
        )
        .bind(&project)
        .fetch_one(&mut **tx)
        .await?;
        sqlx::query(
            "INSERT INTO cycles (id, workspace_id, project_id, number, starts_at, ends_at, version, \
             created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, 0, ?, ?)",
        )
        .bind(Id::new_v7().to_string())
        .bind(workspace_id.to_string())
        .bind(&project)
        .bind(number)
        .bind(starts_at.as_millis())
        .bind(ends_at.as_millis())
        .bind(now.as_millis())
        .bind(now.as_millis())
        .execute(&mut **tx)
        .await?;
        run.created += 1;
    }
    Ok(())
}

/// Closes a cycle: the last snapshot, `completed_at`, then its open tasks move. A task in an
/// unstarted or started status goes to the next cycle (`roll`), a task in the backlog or in
/// triage loses its cycle, a closed task stays. No open task stays in a closed cycle.
async fn close_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    zone: &TimeZone,
    cycle_id: &str,
    roll: bool,
    now: TimestampMillis,
) -> Result<(), TaskError> {
    let (project_id, ends_at): (String, i64) =
        sqlx::query_as("SELECT project_id, ends_at FROM cycles WHERE id = ?")
            .bind(cycle_id)
            .fetch_one(&mut **tx)
            .await?;
    let closed_at = ends_at.min(now.as_millis());
    // The last moment of the cycle belongs to its last day.
    let last_day = local_date(zone, TimestampMillis::from_millis(closed_at - 60_000))?;
    snapshot_in_tx(tx, cycle_id, &last_day.to_string()).await?;
    sqlx::query(
        "UPDATE cycles SET completed_at = ?, version = version + 1, updated_at = ? WHERE id = ?",
    )
    .bind(closed_at)
    .bind(now.as_millis())
    .bind(cycle_id)
    .execute(&mut **tx)
    .await?;
    let next: Option<String> = if roll {
        sqlx::query_scalar(
            "SELECT id FROM cycles WHERE project_id = ? AND completed_at IS NULL AND starts_at >= ? \
             ORDER BY starts_at LIMIT 1",
        )
        .bind(&project_id)
        .bind(ends_at)
        .fetch_optional(&mut **tx)
        .await?
    } else {
        None
    };
    let open: Vec<(String, String)> = sqlx::query_as(
        "SELECT tasks.id, task_statuses.category FROM tasks \
         JOIN task_statuses ON task_statuses.id = tasks.status_id \
         WHERE tasks.cycle_id = ? AND task_statuses.category NOT IN ('completed', 'cancelled', 'duplicate')",
    )
    .bind(cycle_id)
    .fetch_all(&mut **tx)
    .await?;
    if open.is_empty() {
        return Ok(());
    }
    let account = service_account_in_tx(tx, workspace_id, now).await?;
    for (task_id, category) in open {
        let target = match category.as_str() {
            "unstarted" | "started" => next.clone(),
            _ => None,
        };
        sqlx::query(
            "UPDATE tasks SET cycle_id = ?, version = version + 1, updated_at = ? WHERE id = ?",
        )
        .bind(&target)
        .bind(now.as_millis())
        .bind(&task_id)
        .execute(&mut **tx)
        .await?;
        // One row for each task, so its activity feed shows the move. No notification.
        audit::record(
            tx,
            workspace_id,
            None,
            "task.cycle_changed",
            AuditOutcome::Success,
            "task",
            Some(parse_id(task_id)?),
            &format!("cycles:{cycle_id}"),
            json!({
                "from": cycle_id,
                "to": target,
                "actor_service_account_id": account,
                "actor_service_account_name": CYCLES_ACCOUNT,
            }),
            now,
        )
        .await?;
    }
    Ok(())
}

/// Cycles go off: the current cycle is completed and its open tasks lose their cycle; the future
/// cycles are deleted (their tasks lose the cycle through the foreign key).
async fn turn_off_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    settings: &CycleSettingsRecord,
    now: TimestampMillis,
) -> Result<(), TaskError> {
    let zone = zone(&settings.timezone)?;
    let project = settings.project_id.to_string();
    sqlx::query("DELETE FROM cycles WHERE project_id = ? AND starts_at > ?")
        .bind(&project)
        .bind(now.as_millis())
        .execute(&mut **tx)
        .await?;
    let open: Vec<String> = sqlx::query_scalar(
        "SELECT id FROM cycles WHERE project_id = ? AND completed_at IS NULL ORDER BY starts_at",
    )
    .bind(&project)
    .fetch_all(&mut **tx)
    .await?;
    for cycle_id in open {
        sqlx::query("UPDATE cycles SET ends_at = MIN(ends_at, MAX(?, starts_at + 1)) WHERE id = ?")
            .bind(now.as_millis())
            .bind(&cycle_id)
            .execute(&mut **tx)
            .await?;
        close_in_tx(tx, workspace_id, &zone, &cycle_id, false, now).await?;
    }
    Ok(())
}

/// Moves every cycle that starts after `after` by `delta` milliseconds.
async fn shift_later_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    project_id: Id,
    after: TimestampMillis,
    delta: i64,
    now: TimestampMillis,
) -> Result<(), TaskError> {
    if delta == 0 {
        return Ok(());
    }
    sqlx::query(
        "UPDATE cycles SET starts_at = starts_at + ?, ends_at = ends_at + ?, version = version + 1, \
         updated_at = ? WHERE project_id = ? AND starts_at > ? AND completed_at IS NULL",
    )
    .bind(delta)
    .bind(delta)
    .bind(now.as_millis())
    .bind(project_id.to_string())
    .bind(after.as_millis())
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Writes (or writes again) the snapshot of `day` from the totals of the cycle at this moment.
async fn snapshot_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    cycle_id: &str,
    day: &str,
) -> Result<(), TaskError> {
    sqlx::query(&format!(
        "INSERT INTO cycle_days (cycle_id, day, scope_count, scope_points, started_count, started_points, \
         done_count, done_points) \
         SELECT ?1, ?2, COALESCE(totals.scope_count, 0), COALESCE(totals.scope_points, 0), \
         COALESCE(totals.started_count, 0), COALESCE(totals.started_points, 0), \
         COALESCE(totals.done_count, 0), COALESCE(totals.done_points, 0) \
         FROM (SELECT ?1 AS id) AS cycle LEFT JOIN ({TOTALS}) AS totals ON totals.cycle_id = cycle.id \
         WHERE true \
         ON CONFLICT(cycle_id, day) DO UPDATE SET scope_count = excluded.scope_count, \
         scope_points = excluded.scope_points, started_count = excluded.started_count, \
         started_points = excluded.started_points, done_count = excluded.done_count, \
         done_points = excluded.done_points"
    ))
    .bind(cycle_id)
    .bind(day)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// The "Cycles" service account of the workspace, created on first use (as "GitHub" is). It is
/// made in the name of the workspace owner.
async fn service_account_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    now: TimestampMillis,
) -> Result<String, TaskError> {
    if let Some(id) = sqlx::query_scalar::<_, String>(
        "SELECT id FROM service_accounts WHERE workspace_id = ? AND lower(name) = lower(?) AND disabled_at IS NULL",
    )
    .bind(workspace_id.to_string())
    .bind(CYCLES_ACCOUNT)
    .fetch_optional(&mut **tx)
    .await?
    {
        return Ok(id);
    }
    let owner: String = sqlx::query_scalar(
        "SELECT user_id FROM memberships WHERE workspace_id = ? AND role = 'owner' LIMIT 1",
    )
    .bind(workspace_id.to_string())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(TaskError::NotFound)?;
    let id = Id::new_v7().to_string();
    sqlx::query(
        "INSERT INTO service_accounts (id, workspace_id, name, created_by, created_at) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(workspace_id.to_string())
    .bind(CYCLES_ACCOUNT)
    .bind(owner)
    .bind(now.as_millis())
    .execute(&mut **tx)
    .await?;
    Ok(id)
}

fn default_settings(project_id: Id) -> CycleSettingsRecord {
    CycleSettingsRecord {
        project_id,
        enabled: false,
        weeks: 2,
        start_weekday: 1,
        cooldown_weeks: 0,
        cycles_ahead: 2,
        timezone: "UTC".to_owned(),
        auto_add_started: false,
        auto_add_completed: false,
        active_without_cycle: "off".to_owned(),
        version: 0,
    }
}

async fn settings_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    project_id: Id,
) -> Result<CycleSettingsRecord, TaskError> {
    let row = sqlx::query(&format!(
        "SELECT {SETTINGS_COLUMNS} FROM project_cycle_settings WHERE project_id = ?"
    ))
    .bind(project_id.to_string())
    .fetch_optional(&mut **tx)
    .await?;
    match row {
        Some(row) => settings_from_row(row),
        None => Ok(default_settings(project_id)),
    }
}

fn settings_from_row(row: sqlx::sqlite::SqliteRow) -> Result<CycleSettingsRecord, TaskError> {
    Ok(CycleSettingsRecord {
        project_id: parse_id(row.get("project_id"))?,
        enabled: row.get("enabled"),
        weeks: row.get("weeks"),
        start_weekday: row.get("start_weekday"),
        cooldown_weeks: row.get("cooldown_weeks"),
        cycles_ahead: row.get("cycles_ahead"),
        timezone: row.get("timezone"),
        auto_add_started: row.get("auto_add_started"),
        auto_add_completed: row.get("auto_add_completed"),
        active_without_cycle: row.get("active_without_cycle"),
        version: parse_version(row.get("version"))?,
    })
}

async fn cycle_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    project_id: Id,
    cycle_id: Id,
    now: TimestampMillis,
) -> Result<CycleRecord, TaskError> {
    let row = sqlx::query(&format!(
        "{} WHERE cycles.id = ? AND cycles.project_id = ?",
        cycle_select()
    ))
    .bind(cycle_id.to_string())
    .bind(project_id.to_string())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(TaskError::NotFound)?;
    cycle_from_row(row, now)
}

fn cycle_from_row(
    row: sqlx::sqlite::SqliteRow,
    now: TimestampMillis,
) -> Result<CycleRecord, TaskError> {
    let starts_at = TimestampMillis::from_millis(row.get("starts_at"));
    let ends_at = TimestampMillis::from_millis(row.get("ends_at"));
    let completed_at = row
        .get::<Option<i64>, _>("completed_at")
        .map(TimestampMillis::from_millis);
    let state = if completed_at.is_some() || ends_at <= now {
        "completed"
    } else if starts_at > now {
        "future"
    } else {
        "current"
    };
    Ok(CycleRecord {
        id: parse_id(row.get("id"))?,
        workspace_id: parse_id(row.get("workspace_id"))?,
        project_id: parse_id(row.get("project_id"))?,
        number: row.get("number"),
        name: row.get("name"),
        description: row.get("description"),
        starts_at,
        ends_at,
        completed_at,
        state: state.to_owned(),
        scope_count: row.get("scope_count"),
        scope_points: row.get("scope_points"),
        started_count: row.get("started_count"),
        started_points: row.get("started_points"),
        done_count: row.get("done_count"),
        done_points: row.get("done_points"),
        unestimated_count: row.get("unestimated_count"),
        version: parse_version(row.get("version"))?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;

    fn local(zone: &TimeZone, at: TimestampMillis) -> String {
        Timestamp::from_millisecond(at.as_millis())
            .unwrap()
            .to_zoned(zone.clone())
            .datetime()
            .to_string()
    }

    #[test]
    fn the_first_cycle_starts_on_the_most_recent_start_weekday() {
        // 2026-10-08 is a Thursday.
        let thursday = date(2026, 10, 8);
        assert_eq!(latest_weekday(thursday, 4).unwrap(), thursday);
        assert_eq!(latest_weekday(thursday, 1).unwrap(), date(2026, 10, 5));
        assert_eq!(latest_weekday(thursday, 0).unwrap(), date(2026, 10, 4));
        assert_eq!(latest_weekday(thursday, 5).unwrap(), date(2026, 10, 2));
    }

    #[test]
    fn a_cycle_starts_and_ends_at_one_minute_past_midnight_in_the_project_timezone() {
        let berlin = zone("Europe/Berlin").unwrap();
        let (starts_at, ends_at) = bounds(&berlin, date(2026, 10, 5), 2).unwrap();
        assert_eq!(local(&berlin, starts_at), "2026-10-05T00:01:00");
        assert_eq!(local(&berlin, ends_at), "2026-10-19T00:01:00");
        // The same wall time is a different instant in a different zone.
        let tokyo = zone("Asia/Tokyo").unwrap();
        let (tokyo_start, _) = bounds(&tokyo, date(2026, 10, 5), 2).unwrap();
        assert_eq!(
            starts_at.as_millis() - tokyo_start.as_millis(),
            7 * 3_600_000
        );
        assert_eq!(local_date(&berlin, starts_at).unwrap(), date(2026, 10, 5));
    }

    #[test]
    fn a_cycle_across_a_daylight_saving_change_keeps_its_wall_time() {
        // Europe/Berlin leaves summer time on 2026-10-25: that week has one more hour.
        let berlin = zone("Europe/Berlin").unwrap();
        let (starts_at, ends_at) = bounds(&berlin, date(2026, 10, 19), 1).unwrap();
        assert_eq!(local(&berlin, ends_at), "2026-10-26T00:01:00");
        assert_eq!(
            ends_at.as_millis() - starts_at.as_millis(),
            7 * 86_400_000 + 3_600_000
        );
        // And the week of 2026-03-29 has one hour less.
        let (starts_at, ends_at) = bounds(&berlin, date(2026, 3, 23), 1).unwrap();
        assert_eq!(
            ends_at.as_millis() - starts_at.as_millis(),
            7 * 86_400_000 - 3_600_000
        );
    }

    #[test]
    fn settings_are_checked() {
        let valid = CycleSettingsInput {
            enabled: true,
            weeks: 2,
            start_weekday: 1,
            cooldown_weeks: 0,
            cycles_ahead: 2,
            timezone: "Europe/Berlin".to_owned(),
            auto_add_started: false,
            auto_add_completed: false,
            active_without_cycle: "off".to_owned(),
        };
        assert!(validate_settings(&valid).is_ok());
        for bad in [
            CycleSettingsInput {
                weeks: 9,
                ..valid.clone()
            },
            CycleSettingsInput {
                start_weekday: 7,
                ..valid.clone()
            },
            CycleSettingsInput {
                cooldown_weeks: 5,
                ..valid.clone()
            },
            CycleSettingsInput {
                cycles_ahead: 0,
                ..valid.clone()
            },
            CycleSettingsInput {
                timezone: "Mars/Olympus".to_owned(),
                ..valid.clone()
            },
            CycleSettingsInput {
                active_without_cycle: "later".to_owned(),
                ..valid.clone()
            },
        ] {
            assert!(validate_settings(&bad).is_err());
        }
    }
}
