use std::str::FromStr;
use std::time::Duration;

use chrono::{DateTime, Utc};
use cron::Schedule;
use serde_json::Value;
use sqlx::Row;
use thiserror::Error;

use super::{JobKind, JobPriority, JobStore};
use crate::{Id, ParseIdError, TimestampMillis};

#[derive(Debug, Error)]
pub enum ScheduleError {
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Id(#[from] ParseIdError),
    #[error("cron expressions must have exactly five fields")]
    CronFieldCount,
    #[error("invalid cron expression: {0}")]
    InvalidCron(String),
    #[error("fixed schedule intervals must be positive")]
    ZeroInterval,
    #[error("schedule timestamp is outside the supported UTC range")]
    TimestampRange,
    #[error("invalid stored schedule: {0}")]
    InvalidStoredSchedule(String),
}

#[derive(Clone)]
pub struct CronSchedule {
    expression: String,
    parsed: Schedule,
}

impl std::fmt::Debug for CronSchedule {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_tuple("CronSchedule")
            .field(&self.expression)
            .finish()
    }
}

impl CronSchedule {
    pub fn parse(expression: &str) -> Result<Self, ScheduleError> {
        if expression.split_whitespace().count() != 5 {
            return Err(ScheduleError::CronFieldCount);
        }
        let parsed = Schedule::from_str(&format!("0 {expression}"))
            .map_err(|error| ScheduleError::InvalidCron(error.to_string()))?;
        Ok(Self {
            expression: expression.to_owned(),
            parsed,
        })
    }

    pub fn next_after(&self, timestamp: TimestampMillis) -> Result<TimestampMillis, ScheduleError> {
        let date = DateTime::<Utc>::from_timestamp_millis(timestamp.as_millis())
            .ok_or(ScheduleError::TimestampRange)?;
        self.parsed
            .after(&date)
            .next()
            .map(|next| TimestampMillis::from_millis(next.timestamp_millis()))
            .ok_or(ScheduleError::TimestampRange)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CatchUpMode {
    #[default]
    Latest,
    Full,
}

impl CatchUpMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Latest => "latest",
            Self::Full => "full",
        }
    }

    fn parse(value: &str) -> Result<Self, ScheduleError> {
        match value {
            "latest" => Ok(Self::Latest),
            "full" => Ok(Self::Full),
            _ => Err(ScheduleError::InvalidStoredSchedule(value.to_owned())),
        }
    }
}

#[derive(Clone, Debug)]
enum Recurrence {
    Interval(Duration),
    Cron(Box<CronSchedule>),
}

#[derive(Clone, Debug)]
pub struct RecurringSchedule {
    pub id: Id,
    pub workspace_id: Option<String>,
    pub kind: JobKind,
    pub payload: Value,
    recurrence: Recurrence,
    pub next_run_at: TimestampMillis,
    pub catch_up: CatchUpMode,
    pub enabled: bool,
}

impl RecurringSchedule {
    #[must_use]
    pub fn interval(
        kind: JobKind,
        payload: Value,
        interval: Duration,
        next_run_at: TimestampMillis,
    ) -> Self {
        assert!(
            interval.as_millis() > 0,
            "fixed schedule intervals must be at least one millisecond"
        );
        Self {
            id: Id::new_v7(),
            workspace_id: None,
            kind,
            payload,
            recurrence: Recurrence::Interval(interval),
            next_run_at,
            catch_up: CatchUpMode::Latest,
            enabled: true,
        }
    }

    pub fn cron(
        kind: JobKind,
        payload: Value,
        expression: &str,
        next_run_at: TimestampMillis,
    ) -> Result<Self, ScheduleError> {
        Ok(Self {
            id: Id::new_v7(),
            workspace_id: None,
            kind,
            payload,
            recurrence: Recurrence::Cron(Box::new(CronSchedule::parse(expression)?)),
            next_run_at,
            catch_up: CatchUpMode::Latest,
            enabled: true,
        })
    }

    #[must_use]
    pub fn with_catch_up(mut self, catch_up: CatchUpMode) -> Self {
        self.catch_up = catch_up;
        self
    }

    #[must_use]
    pub fn with_workspace(mut self, workspace_id: impl Into<String>) -> Self {
        self.workspace_id = Some(workspace_id.into());
        self
    }

    fn encoded_recurrence(&self) -> String {
        match &self.recurrence {
            Recurrence::Interval(interval) => format!("interval:{}", interval.as_millis()),
            Recurrence::Cron(cron) => format!("cron:{}", cron.expression),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Scheduler {
    store: JobStore,
}

impl Scheduler {
    #[must_use]
    pub fn new(store: JobStore) -> Self {
        Self { store }
    }

    pub async fn upsert(&self, schedule: &RecurringSchedule) -> Result<(), ScheduleError> {
        self.store.register_kind(schedule.kind.clone());
        sqlx::query(
            "INSERT INTO schedules (id, workspace_id, job_kind, payload_json, schedule, \
             next_run_at, catch_up_mode, consecutive_failures, enabled, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, 0, ?, ?) \
             ON CONFLICT(id) DO UPDATE SET workspace_id = excluded.workspace_id, \
             job_kind = excluded.job_kind, payload_json = excluded.payload_json, \
             schedule = excluded.schedule, next_run_at = excluded.next_run_at, \
             catch_up_mode = excluded.catch_up_mode, enabled = excluded.enabled, \
             updated_at = excluded.updated_at",
        )
        .bind(schedule.id.to_string())
        .bind(&schedule.workspace_id)
        .bind(schedule.kind.name())
        .bind(serde_json::to_string(&schedule.payload)?)
        .bind(schedule.encoded_recurrence())
        .bind(schedule.next_run_at.as_millis())
        .bind(schedule.catch_up.as_str())
        .bind(schedule.enabled)
        .bind(TimestampMillis::now().as_millis())
        .execute(self.store.database().pool())
        .await?;
        Ok(())
    }

    pub async fn materialize_due(&self, now: TimestampMillis) -> Result<Vec<Id>, ScheduleError> {
        let rows = sqlx::query(
            "SELECT id, workspace_id, job_kind, payload_json, schedule, next_run_at, \
             catch_up_mode FROM schedules WHERE enabled = 1 AND next_run_at <= ? \
             ORDER BY next_run_at, id",
        )
        .bind(now.as_millis())
        .fetch_all(self.store.database().pool())
        .await?;
        let mut created = Vec::new();
        for row in rows {
            let schedule = decode_schedule(&row, &self.store)?;
            let (occurrences, next_run_at) = due_occurrences(&schedule, now)?;
            let mut transaction = self.store.database().transaction().await?;
            for occurrence in occurrences {
                let job_id = Id::new_v7();
                let result = sqlx::query(
                    "INSERT OR IGNORE INTO jobs (id, workspace_id, schedule_id, scheduled_for, \
                     kind, payload_json, state, priority, attempt_count, max_attempts, \
                     available_at, created_at, updated_at) \
                     VALUES (?, ?, ?, ?, ?, ?, 'queued', ?, 0, ?, ?, ?, ?)",
                )
                .bind(job_id.to_string())
                .bind(&schedule.workspace_id)
                .bind(schedule.id.to_string())
                .bind(occurrence.as_millis())
                .bind(schedule.kind.name())
                .bind(serde_json::to_string(&schedule.payload)?)
                .bind(JobPriority::Normal.as_str())
                .bind(schedule.kind.max_attempts())
                .bind(occurrence.as_millis())
                .bind(now.as_millis())
                .bind(now.as_millis())
                .execute(&mut *transaction)
                .await?;
                if result.rows_affected() == 1 {
                    created.push(job_id);
                }
            }
            sqlx::query(
                "UPDATE schedules SET next_run_at = ?, last_materialized_at = ?, updated_at = ? \
                 WHERE id = ?",
            )
            .bind(next_run_at.as_millis())
            .bind(now.as_millis())
            .bind(now.as_millis())
            .bind(schedule.id.to_string())
            .execute(&mut *transaction)
            .await?;
            transaction.commit().await?;
        }
        Ok(created)
    }
}

fn decode_schedule(
    row: &sqlx::sqlite::SqliteRow,
    store: &JobStore,
) -> Result<RecurringSchedule, ScheduleError> {
    let encoded: String = row.try_get("schedule")?;
    let recurrence = if let Some(milliseconds) = encoded.strip_prefix("interval:") {
        let milliseconds = milliseconds
            .parse::<u64>()
            .map_err(|_| ScheduleError::InvalidStoredSchedule(encoded.clone()))?;
        if milliseconds == 0 {
            return Err(ScheduleError::ZeroInterval);
        }
        Recurrence::Interval(Duration::from_millis(milliseconds))
    } else if let Some(expression) = encoded.strip_prefix("cron:") {
        Recurrence::Cron(Box::new(CronSchedule::parse(expression)?))
    } else {
        return Err(ScheduleError::InvalidStoredSchedule(encoded));
    };
    Ok(RecurringSchedule {
        id: Id::from_str(row.try_get::<&str, _>("id")?)?,
        workspace_id: row.try_get("workspace_id")?,
        kind: store.resolve_kind(row.try_get("job_kind")?),
        payload: serde_json::from_str(row.try_get::<&str, _>("payload_json")?)?,
        recurrence,
        next_run_at: TimestampMillis::from_millis(row.try_get("next_run_at")?),
        catch_up: CatchUpMode::parse(row.try_get("catch_up_mode")?)?,
        enabled: true,
    })
}

fn due_occurrences(
    schedule: &RecurringSchedule,
    now: TimestampMillis,
) -> Result<(Vec<TimestampMillis>, TimestampMillis), ScheduleError> {
    if schedule.catch_up == CatchUpMode::Latest
        && let Recurrence::Interval(interval) = &schedule.recurrence
    {
        let interval =
            i64::try_from(interval.as_millis()).map_err(|_| ScheduleError::TimestampRange)?;
        let elapsed = now.as_millis() - schedule.next_run_at.as_millis();
        let latest = schedule
            .next_run_at
            .as_millis()
            .checked_add((elapsed / interval) * interval)
            .ok_or(ScheduleError::TimestampRange)?;
        let next = latest
            .checked_add(interval)
            .ok_or(ScheduleError::TimestampRange)?;
        return Ok((
            vec![TimestampMillis::from_millis(latest)],
            TimestampMillis::from_millis(next),
        ));
    }

    let mut occurrences = Vec::new();
    let mut current = schedule.next_run_at;
    while current <= now {
        match schedule.catch_up {
            CatchUpMode::Latest => {
                occurrences.clear();
                occurrences.push(current);
            }
            CatchUpMode::Full => occurrences.push(current),
        }
        current = match &schedule.recurrence {
            Recurrence::Interval(interval) => {
                let millis = i64::try_from(interval.as_millis())
                    .map_err(|_| ScheduleError::TimestampRange)?;
                TimestampMillis::from_millis(
                    current
                        .as_millis()
                        .checked_add(millis)
                        .ok_or(ScheduleError::TimestampRange)?,
                )
            }
            Recurrence::Cron(cron) => cron.next_after(current)?,
        };
    }
    Ok((occurrences, current))
}
