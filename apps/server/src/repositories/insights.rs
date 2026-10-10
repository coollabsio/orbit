//! Insights (spec `2026-10-08-insights-design.md`): the numbers behind the project charts. No
//! table of its own: everything is calculated from the tasks as they are now.
//!
//! One set of counting rules for every chart, the same as the cycle totals: only tasks with no
//! sub-issues count; cancelled and duplicate tasks are not in the scope; trashed tasks and tasks
//! in triage are excluded; a task with no estimate counts 0 points.

use jiff::civil::Date;
use jiff::tz::TimeZone;
use jiff::{Timestamp, ToSpan};
use orbit_platform::{Id, TimestampMillis};
use serde::Serialize;
use sqlx::Row;
use utoipa::ToSchema;

use super::tasks::{TaskError, TaskRepository, require_project};

/// The counting rule, for a query `FROM tasks JOIN task_statuses ON task_statuses.id = tasks.status_id`.
const COUNTED: &str = "tasks.deleted_at IS NULL \
     AND task_statuses.category NOT IN ('cancelled', 'duplicate', 'triage') \
     AND NOT EXISTS (SELECT 1 FROM tasks AS child \
                     WHERE child.parent_task_id = tasks.id AND child.deleted_at IS NULL)";

const MAX_WEEKS: i64 = 104;

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct ThroughputWeek {
    /// The Monday of the week in the viewer's timezone, `YYYY-MM-DD`.
    pub week: String,
    pub created_count: i64,
    pub created_points: i64,
    pub completed_count: i64,
    pub completed_points: i64,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct BurnupWeek {
    /// The Monday of the week in the viewer's timezone, `YYYY-MM-DD`.
    pub week: String,
    /// Tasks of the milestone that existed at the end of the week.
    pub scope_count: i64,
    pub scope_points: i64,
    /// Of these, the tasks that were completed at the end of the week.
    pub done_count: i64,
    pub done_points: i64,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct OpenGroup {
    /// A status id, a user id, a priority or a label id; `none` for no assignee or no label.
    pub key: String,
    pub count: i64,
    pub points: i64,
}

/// The rows of a chart, with the number of counted tasks that have no estimate (0 points).
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct Insight<T> {
    pub items: Vec<T>,
    pub unestimated_count: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OpenBy {
    Status,
    Assignee,
    Priority,
    Label,
}

impl OpenBy {
    pub fn parse(value: &str) -> Result<Self, TaskError> {
        match value {
            "status" => Ok(Self::Status),
            "assignee" => Ok(Self::Assignee),
            "priority" => Ok(Self::Priority),
            "label" => Ok(Self::Label),
            _ => Err(TaskError::Invalid { field: "by" }),
        }
    }
}

fn zone(name: &str) -> Result<TimeZone, TaskError> {
    if name.len() > 64 {
        return Err(TaskError::Invalid { field: "tz" });
    }
    TimeZone::get(name).map_err(|_| TaskError::Invalid { field: "tz" })
}

/// The Monday of the week of `at` in `zone`.
fn week_of(zone: &TimeZone, at: i64) -> Result<Date, TaskError> {
    let date = Timestamp::from_millisecond(at)
        .map_err(|_| TaskError::Invalid { field: "tz" })?
        .to_zoned(zone.clone())
        .date();
    let since_monday = i64::from(date.weekday().to_monday_zero_offset());
    date.checked_sub(since_monday.days())
        .map_err(|_| TaskError::Invalid { field: "tz" })
}

/// The first instant after the week that starts on `monday`.
fn week_end(zone: &TimeZone, monday: Date) -> Result<i64, TaskError> {
    monday
        .checked_add(7.days())
        .and_then(|next| next.at(0, 0, 0, 0).to_zoned(zone.clone()))
        .map(|zoned| zoned.timestamp().as_millisecond())
        .map_err(|_| TaskError::Invalid { field: "tz" })
}

/// `count` Mondays that end with the week of `now`, oldest first.
fn weeks_until(zone: &TimeZone, now: i64, count: i64) -> Result<Vec<Date>, TaskError> {
    let last = week_of(zone, now)?;
    (0..count)
        .rev()
        .map(|back| {
            last.checked_sub((back * 7).days())
                .map_err(|_| TaskError::Invalid { field: "tz" })
        })
        .collect()
}

struct CountedTask {
    created_at: i64,
    /// Set while the task is in a completed status.
    completed_at: Option<i64>,
    estimate: Option<i64>,
}

impl TaskRepository {
    /// The counted tasks of a project, or of one milestone of it.
    async fn counted_tasks(
        &self,
        project_id: Id,
        milestone_id: Option<Id>,
    ) -> Result<Vec<CountedTask>, TaskError> {
        Ok(sqlx::query(&format!(
            "SELECT tasks.created_at, tasks.estimate, \
             CASE WHEN task_statuses.category = 'completed' \
                  THEN COALESCE(tasks.completed_at, tasks.updated_at) END AS completed_at \
             FROM tasks JOIN task_statuses ON task_statuses.id = tasks.status_id \
             WHERE tasks.project_id = ? AND (? IS NULL OR tasks.milestone_id = ?) AND {COUNTED}"
        ))
        .bind(project_id.to_string())
        .bind(milestone_id.map(|id| id.to_string()))
        .bind(milestone_id.map(|id| id.to_string()))
        .fetch_all(self.database().pool())
        .await?
        .into_iter()
        .map(|row| CountedTask {
            created_at: row.get("created_at"),
            completed_at: row.get("completed_at"),
            estimate: row.get("estimate"),
        })
        .collect())
    }

    /// Tasks created and tasks completed in each of the last `weeks` weeks.
    pub async fn insight_throughput(
        &self,
        workspace_id: Id,
        project_id: Id,
        actor_id: Id,
        timezone: &str,
        weeks: i64,
        now: TimestampMillis,
    ) -> Result<Insight<ThroughputWeek>, TaskError> {
        if !(1..=MAX_WEEKS).contains(&weeks) {
            return Err(TaskError::Invalid { field: "weeks" });
        }
        let zone = zone(timezone)?;
        require_project(self.database().pool(), workspace_id, project_id, actor_id).await?;
        let tasks = self.counted_tasks(project_id, None).await?;
        let mondays = weeks_until(&zone, now.as_millis(), weeks)?;
        let mut items: Vec<ThroughputWeek> = mondays
            .iter()
            .map(|monday| ThroughputWeek {
                week: monday.to_string(),
                created_count: 0,
                created_points: 0,
                completed_count: 0,
                completed_points: 0,
            })
            .collect();
        let index_of = |at: i64| -> Result<Option<usize>, TaskError> {
            let monday = week_of(&zone, at)?;
            Ok(mondays.iter().position(|week| *week == monday))
        };
        let mut unestimated_count = 0;
        for task in &tasks {
            let created = index_of(task.created_at)?;
            let completed = match task.completed_at {
                Some(at) => index_of(at)?,
                None => None,
            };
            if (created.is_some() || completed.is_some()) && task.estimate.is_none() {
                unestimated_count += 1;
            }
            if let Some(index) = created {
                items[index].created_count += 1;
                items[index].created_points += task.estimate.unwrap_or(0);
            }
            if let Some(index) = completed {
                items[index].completed_count += 1;
                items[index].completed_points += task.estimate.unwrap_or(0);
            }
        }
        Ok(Insight {
            items,
            unestimated_count,
        })
    }

    /// The scope and the done part of a milestone at the end of each week, from the week of its
    /// first task to this week. It uses the day a task was created, not the day it joined the
    /// milestone, and it does not show a task that left the milestone.
    pub async fn insight_burnup(
        &self,
        workspace_id: Id,
        project_id: Id,
        milestone_id: Id,
        actor_id: Id,
        timezone: &str,
        now: TimestampMillis,
    ) -> Result<Insight<BurnupWeek>, TaskError> {
        let zone = zone(timezone)?;
        require_project(self.database().pool(), workspace_id, project_id, actor_id).await?;
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM milestones WHERE id = ? AND project_id = ?)",
        )
        .bind(milestone_id.to_string())
        .bind(project_id.to_string())
        .fetch_one(self.database().pool())
        .await?;
        if !exists {
            return Err(TaskError::NotFound);
        }
        let tasks = self.counted_tasks(project_id, Some(milestone_id)).await?;
        let Some(first) = tasks.iter().map(|task| task.created_at).min() else {
            return Ok(Insight {
                items: Vec::new(),
                unestimated_count: 0,
            });
        };
        let mut mondays = Vec::new();
        let mut monday = week_of(&zone, first.min(now.as_millis()))?;
        let last = week_of(&zone, now.as_millis())?;
        while monday <= last {
            mondays.push(monday);
            monday = monday
                .checked_add(7.days())
                .map_err(|_| TaskError::Invalid { field: "tz" })?;
        }
        // A long-lived milestone shows its most recent weeks.
        let skip = mondays.len().saturating_sub(MAX_WEEKS as usize);
        let mut items = Vec::new();
        for monday in mondays.into_iter().skip(skip) {
            let end = week_end(&zone, monday)?;
            let mut week = BurnupWeek {
                week: monday.to_string(),
                scope_count: 0,
                scope_points: 0,
                done_count: 0,
                done_points: 0,
            };
            for task in tasks.iter().filter(|task| task.created_at < end) {
                week.scope_count += 1;
                week.scope_points += task.estimate.unwrap_or(0);
                if task.completed_at.is_some_and(|at| at < end) {
                    week.done_count += 1;
                    week.done_points += task.estimate.unwrap_or(0);
                }
            }
            items.push(week);
        }
        Ok(Insight {
            items,
            unestimated_count: tasks.iter().filter(|task| task.estimate.is_none()).count() as i64,
        })
    }

    /// The open tasks of a project (backlog, unstarted, started) by one dimension. A task with
    /// two assignees or two labels counts in each.
    pub async fn insight_open(
        &self,
        workspace_id: Id,
        project_id: Id,
        actor_id: Id,
        by: OpenBy,
    ) -> Result<Insight<OpenGroup>, TaskError> {
        require_project(self.database().pool(), workspace_id, project_id, actor_id).await?;
        let open = format!(
            "FROM tasks JOIN task_statuses ON task_statuses.id = tasks.status_id \
             WHERE tasks.project_id = ?1 AND {COUNTED} \
             AND task_statuses.category IN ('backlog', 'unstarted', 'started')"
        );
        // Suspended members and former members are not assignees (the same rule as the lists).
        let sql = match by {
            OpenBy::Status => format!(
                "SELECT tasks.status_id AS key, COUNT(*) AS count, COALESCE(SUM(tasks.estimate), 0) AS points \
                 {open} GROUP BY tasks.status_id ORDER BY MIN(task_statuses.position), key"
            ),
            OpenBy::Priority => format!(
                "SELECT tasks.priority AS key, COUNT(*) AS count, COALESCE(SUM(tasks.estimate), 0) AS points \
                 {open} GROUP BY tasks.priority ORDER BY CASE tasks.priority WHEN 'urgent' THEN 0 \
                 WHEN 'high' THEN 1 WHEN 'medium' THEN 2 WHEN 'low' THEN 3 ELSE 4 END"
            ),
            OpenBy::Assignee => format!(
                "WITH open_tasks AS (SELECT tasks.id, tasks.estimate {open}) \
                 SELECT COALESCE(task_assignees.user_id, 'none') AS key, COUNT(*) AS count, \
                 COALESCE(SUM(open_tasks.estimate), 0) AS points \
                 FROM open_tasks LEFT JOIN task_assignees ON task_assignees.task_id = open_tasks.id \
                 GROUP BY key ORDER BY count DESC, key"
            ),
            OpenBy::Label => format!(
                "WITH open_tasks AS (SELECT tasks.id, tasks.estimate {open}) \
                 SELECT COALESCE(task_labels.label_id, 'none') AS key, COUNT(*) AS count, \
                 COALESCE(SUM(open_tasks.estimate), 0) AS points \
                 FROM open_tasks LEFT JOIN task_labels ON task_labels.task_id = open_tasks.id \
                 GROUP BY key ORDER BY count DESC, key"
            ),
        };
        let items = sqlx::query(&sql)
            .bind(project_id.to_string())
            .fetch_all(self.database().pool())
            .await?
            .into_iter()
            .map(|row| OpenGroup {
                key: row.get("key"),
                count: row.get("count"),
                points: row.get("points"),
            })
            .collect();
        let unestimated_count: i64 = sqlx::query_scalar(&format!(
            "SELECT COUNT(*) {open} AND tasks.estimate IS NULL"
        ))
        .bind(project_id.to_string())
        .fetch_one(self.database().pool())
        .await?;
        Ok(Insight {
            items,
            unestimated_count,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;

    fn at(zone: &TimeZone, year: i16, month: i8, day: i8, hour: i8) -> i64 {
        date(year, month, day)
            .at(hour, 0, 0, 0)
            .to_zoned(zone.clone())
            .unwrap()
            .timestamp()
            .as_millisecond()
    }

    #[test]
    fn a_week_starts_on_monday_in_the_viewers_timezone() {
        let utc = zone("UTC").unwrap();
        let tokyo = zone("Asia/Tokyo").unwrap();
        let los_angeles = zone("America/Los_Angeles").unwrap();
        // Sunday 2026-10-04 20:00 UTC is Monday morning in Tokyo and Sunday noon in Los Angeles.
        let instant = at(&utc, 2026, 10, 4, 20);
        assert_eq!(week_of(&utc, instant).unwrap(), date(2026, 9, 28));
        assert_eq!(week_of(&tokyo, instant).unwrap(), date(2026, 10, 5));
        assert_eq!(week_of(&los_angeles, instant).unwrap(), date(2026, 9, 28));
        // The week ends at local midnight of the next Monday.
        assert_eq!(
            week_end(&tokyo, date(2026, 10, 5)).unwrap(),
            at(&tokyo, 2026, 10, 12, 0)
        );
    }

    #[test]
    fn the_last_weeks_end_with_this_week_and_have_no_gap() {
        let utc = zone("UTC").unwrap();
        let weeks = weeks_until(&utc, at(&utc, 2026, 10, 8, 12), 3).unwrap();
        assert_eq!(
            weeks,
            [date(2026, 9, 21), date(2026, 9, 28), date(2026, 10, 5)]
        );
        assert!(zone("Mars/Olympus").is_err());
        assert!(OpenBy::parse("status").is_ok());
        assert!(OpenBy::parse("colour").is_err());
    }
}
