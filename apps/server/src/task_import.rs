//! Pieces the task importers (Asana, Linear) share: the job kind and its failure model, the
//! request spacing of their API clients, dates, and the Orbit projects, statuses and labels
//! they create. Each importer keeps its own tables, SQL and source mapping.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use orbit_platform::{Database, Id, JobError, JobKind, Row, TimestampMillis};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use crate::billing::WorkspaceAccess;
use crate::notion::import::clamp;
use crate::repositories::billing;
use crate::repositories::tasks::{TaskError, TaskRepository};

/// Colors of the projects and labels an importer creates, in turn.
pub(crate) const PROJECT_COLORS: [&str; 6] = [
    "#5e6ad2", "#26b5ce", "#4cb782", "#f2c94c", "#f2994a", "#eb5757",
];
pub(crate) const LABEL_COLORS: [&str; 6] = [
    "#eb5757", "#f2994a", "#f2c94c", "#4cb782", "#26b5ce", "#bb87fc",
];

/// Both job kinds of an importer: two at a time on all servers and one per workspace (defaults;
/// Admin can change them), a few retries for outages of the source. Same as the Notion import.
#[must_use]
pub fn job_kind(name: &str) -> JobKind {
    JobKind::new(name)
        .with_concurrency_limit(2)
        .with_workspace_limit(1)
        .with_retry_policy(
            5,
            vec![
                Duration::from_secs(30),
                Duration::from_secs(2 * 60),
                Duration::from_secs(10 * 60),
                Duration::from_secs(30 * 60),
            ],
        )
        .with_sensitive_fields(["token"])
}

/// One job attempt: `cancel` fires on server shutdown (the job resumes later), and
/// `last_attempt` turns a transient source failure into a failed import instead of a retry.
#[derive(Clone, Debug, Default)]
pub struct JobRun {
    pub cancel: CancellationToken,
    pub last_attempt: bool,
}

/// Why one item could not be imported, and what that means for the import.
pub(crate) enum Failure {
    /// Server shutdown: the job is requeued and resumes; never fails the import.
    Interrupted,
    /// Record on the item and continue.
    Item(String),
    /// Stop the import (revoked token, read-only workspace, task limit).
    Fatal(String),
    /// The source is overloaded or unreachable: retry the job later.
    Transient(String),
}

impl From<sqlx::Error> for Failure {
    fn from(error: sqlx::Error) -> Self {
        Self::Transient(format!("database error: {error}"))
    }
}

impl From<TaskError> for Failure {
    fn from(error: TaskError) -> Self {
        match error {
            TaskError::Unavailable(error) => Self::from(error),
            TaskError::TrialLimit(reached) => Self::Fatal(format!(
                "The import stopped at the trial's limit. {}",
                reached.detail()
            )),
            TaskError::NotFound => {
                Self::Item("The project or task was deleted during the import.".to_owned())
            }
            other => Self::Item(format!("Orbit refused the task: {other}.")),
        }
    }
}

/// Shutdown: leave the job to resume later. Cancelled or failed elsewhere: done.
pub(crate) fn stopped(job: &JobRun) -> Result<(), Failure> {
    if job.cancel.is_cancelled() {
        Err(Failure::Interrupted)
    } else {
        Ok(())
    }
}

pub(crate) fn retryable(error: impl std::fmt::Display) -> JobError {
    JobError::Retryable(error.to_string())
}

pub(crate) fn parse_stored_id(value: String) -> Result<Id, Failure> {
    value
        .parse()
        .map_err(|_| Failure::Fatal("Stored import data is invalid.".to_owned()))
}

/// Orbit Cloud: a read-only workspace (past its trial, no subscription) takes no imports.
pub(crate) async fn ensure_writable(database: &Database, workspace_id: Id) -> Result<(), Failure> {
    match billing::write_access(database, workspace_id).await? {
        WorkspaceAccess::Full | WorkspaceAccess::Trial => Ok(()),
        WorkspaceAccess::ReadOnly => Err(Failure::Fatal(
            "The import stopped because the workspace is read-only: it has no active \
             subscription."
                .to_owned(),
        )),
    }
}

/// Lowercase email → user id of the workspace's active members (assignees are matched by email).
pub(crate) async fn member_emails(
    database: &Database,
    workspace_id: Id,
) -> Result<HashMap<String, Id>, sqlx::Error> {
    Ok(sqlx::query(
        "SELECT users.id, users.normalized_email FROM memberships \
         JOIN users ON users.id = memberships.user_id \
         WHERE memberships.workspace_id = $1 AND users.suspended_at IS NULL",
    )
    .bind(workspace_id.to_string())
    .fetch_all(database.pool())
    .await?
    .into_iter()
    .filter_map(|member| {
        let id = member.get::<String, _>("id").parse().ok()?;
        Some((
            member.get::<String, _>("normalized_email").to_lowercase(),
            id,
        ))
    })
    .collect())
}

/// `(id, name, category)` of the project's statuses in board order.
pub(crate) async fn project_statuses(
    database: &Database,
    project_id: Id,
) -> Result<Vec<(Id, String, String)>, sqlx::Error> {
    Ok(sqlx::query(
        "SELECT id, name, category FROM task_statuses WHERE project_id = $1 ORDER BY position, id",
    )
    .bind(project_id.to_string())
    .fetch_all(database.pool())
    .await?
    .into_iter()
    .filter_map(|row| {
        Some((
            row.get::<String, _>("id").parse().ok()?,
            row.get("name"),
            row.get("category"),
        ))
    })
    .collect())
}

/// What the importer creates the Orbit project with.
pub(crate) struct NewProject<'a> {
    pub id: Id,
    pub workspace_id: Id,
    pub user_id: Id,
    pub name: &'a str,
    /// Used when the name is empty.
    pub fallback_name: &'a str,
    /// The preferred project key; a number is appended while it is taken.
    pub key_base: String,
    pub color: &'a str,
    pub request_id: &'a str,
}

/// Creates the project under its recorded id unless it exists (an earlier attempt made it).
pub(crate) async fn ensure_project(
    database: &Database,
    tasks: &TaskRepository,
    project: NewProject<'_>,
) -> Result<(), Failure> {
    let exists: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM projects WHERE id = $1")
        .bind(project.id.to_string())
        .fetch_one(database.pool())
        .await?;
    if exists > 0 {
        return Ok(());
    }
    let name = clamp(project.name.trim(), 200, 200);
    let name = if name.is_empty() {
        project.fallback_name.to_owned()
    } else {
        name
    };
    let base = project.key_base;
    for attempt in 1..=50 {
        let key = if attempt == 1 {
            base.clone()
        } else {
            let suffix = attempt.to_string();
            format!("{}{suffix}", &base[..base.len().min(20 - suffix.len())])
        };
        match tasks
            .create_project_with_id(
                project.id,
                project.workspace_id,
                project.user_id,
                name.clone(),
                key,
                project.color.to_owned(),
                project.request_id,
                TimestampMillis::now(),
            )
            .await
        {
            Ok(_) => return Ok(()),
            Err(TaskError::Conflict) => {}
            Err(TaskError::NotFound) => {
                return Err(Failure::Fatal(
                    "The importing member left the workspace.".to_owned(),
                ));
            }
            Err(other) => return Err(Failure::from(other)),
        }
    }
    Err(Failure::Item(
        "No free project key was found for this project.".to_owned(),
    ))
}

/// A project key from a project name: the initials of a multi-word name, else its first
/// letters; `fallback` when nothing usable is left.
pub(crate) fn project_key_base(name: &str, fallback: &str) -> String {
    let words: Vec<String> = name
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_ascii_uppercase)
        .collect();
    let key: String = if words.len() >= 2 {
        words
            .iter()
            .filter_map(|word| word.chars().next())
            .take(5)
            .collect()
    } else {
        words
            .first()
            .map(|word| word.chars().take(4).collect())
            .unwrap_or_default()
    };
    if key.is_empty() {
        fallback.to_owned()
    } else {
        key
    }
}

/// The project's label named `name`, created when missing; a label with the same name in
/// another case is reused.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn find_or_create_label(
    database: &Database,
    tasks: &TaskRepository,
    workspace_id: Id,
    project_id: Id,
    user_id: Id,
    name: &str,
    color: &str,
    request_id: &str,
) -> Result<Id, Failure> {
    match tasks
        .create_label(
            workspace_id,
            project_id,
            user_id,
            name.to_owned(),
            color.to_owned(),
            request_id,
            TimestampMillis::now(),
        )
        .await
    {
        Ok(label) => Ok(label.id),
        Err(TaskError::Conflict) => {
            let found: Option<String> = sqlx::query_scalar(
                "SELECT id FROM labels WHERE project_id = $1 AND lower(name) = lower($2)",
            )
            .bind(project_id.to_string())
            .bind(name)
            .fetch_optional(database.pool())
            .await?;
            found
                .and_then(|id| id.parse().ok())
                .ok_or_else(|| Failure::Item(format!("The label {name} could not be created.")))
        }
        Err(error) => Err(Failure::from(error)),
    }
}

/// A trimmed, clamped name, or `fallback` when it is empty.
pub(crate) fn name_or(name: Option<String>, fallback: &str) -> String {
    name.map(|name| clamp(name.trim(), 200, 800))
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| fallback.to_owned())
}

/// Spaces requests `interval` apart; `pause` pushes the next slot out for every caller.
pub(crate) struct RateLimiter {
    interval: Duration,
    next: Mutex<Instant>,
}

impl RateLimiter {
    /// `requests_per_second` that is not a positive number means no spacing.
    pub(crate) fn per_second(requests_per_second: f64) -> Self {
        let interval = if requests_per_second.is_finite() && requests_per_second > 0.0 {
            Duration::from_secs_f64(1.0 / requests_per_second)
        } else {
            Duration::ZERO
        };
        Self {
            interval,
            next: Mutex::new(Instant::now()),
        }
    }

    pub(crate) async fn acquire(&self) {
        let slot = {
            let mut next = self
                .next
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            let slot = (*next).max(Instant::now());
            *next = slot + self.interval;
            slot
        };
        tokio::time::sleep_until(slot).await;
    }

    pub(crate) fn pause(&self, wait: Duration) {
        let mut next = self
            .next
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let resume = Instant::now() + wait;
        if *next < resume {
            *next = resume;
        }
    }
}

/// Exponential backoff: `initial` doubled per attempt after the first, at most `max`.
pub(crate) fn backoff(initial: Duration, max: Duration, attempt: u32) -> Duration {
    let factor = 2u32.saturating_pow(attempt.saturating_sub(1).min(16));
    initial.saturating_mul(factor).min(max)
}

/// Parses a `Retry-After` number of seconds (fractions allowed, HTTP dates ignored).
pub(crate) fn parse_retry_after(raw: &str) -> Option<Duration> {
    let seconds: f64 = raw.trim().parse().ok()?;
    (seconds.is_finite() && seconds >= 0.0).then(|| Duration::from_secs_f64(seconds.min(86_400.0)))
}

pub(crate) fn is_transient(error: &reqwest::Error) -> bool {
    error.is_timeout() || error.is_connect() || error.is_request()
}

/// `YYYY-MM-DD` dates become 12:00 UTC of that day (Orbit due dates are instants), which falls
/// on the same calendar day from UTC-11 to UTC+11.
#[must_use]
pub fn parse_date_millis(value: &str) -> Option<i64> {
    let days = crate::repositories::task_filter::parse_ymd(value)?;
    Some(days * 86_400_000 + 12 * 3_600_000)
}

/// An RFC 3339 instant (`2024-05-01T09:30:00.000Z`, or with a `±HH:MM` offset) in epoch
/// milliseconds.
#[must_use]
pub fn parse_instant_millis(value: &str) -> Option<i64> {
    let value = value.trim();
    if !value.is_ascii() || value.len() < 20 || value.as_bytes()[10] != b'T' {
        return None;
    }
    let days = crate::repositories::task_filter::parse_ymd(&value[..10])?;
    let digits = |part: &str| -> Option<i64> {
        part.bytes()
            .all(|byte| byte.is_ascii_digit())
            .then(|| part.parse().ok())
            .flatten()
    };
    let rest = &value[11..];
    if rest.len() < 9 || &rest[2..3] != ":" || &rest[5..6] != ":" {
        return None;
    }
    let (hour, minute, second) = (
        digits(&rest[..2])?,
        digits(&rest[3..5])?,
        digits(&rest[6..8])?,
    );
    if hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let mut rest = &rest[8..];
    let mut millis = 0;
    if let Some(fraction) = rest.strip_prefix('.') {
        let end = fraction
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(fraction.len());
        if end == 0 {
            return None;
        }
        let padded = format!("{:0<3}", &fraction[..end.min(3)]);
        millis = digits(&padded)?;
        rest = &fraction[end..];
    }
    let offset_minutes = match rest {
        "Z" | "z" => 0,
        _ if rest.len() == 6 && &rest[3..4] == ":" => {
            let sign = match &rest[..1] {
                "+" => 1,
                "-" => -1,
                _ => return None,
            };
            sign * (digits(&rest[1..3])? * 60 + digits(&rest[4..6])?)
        }
        _ => return None,
    };
    Some(
        days * 86_400_000
            + (hour * 3_600 + minute * 60 + second - offset_minutes * 60) * 1_000
            + millis,
    )
}

/// `2024-05-01T09:30:00.000Z` → `2024-05-01 09:30 UTC`.
pub(crate) fn display_instant(value: &str) -> String {
    match value.get(..16) {
        Some(prefix) if value.ends_with('Z') && value.as_bytes().get(10) == Some(&b'T') => {
            format!("{} UTC", prefix.replacen('T', " ", 1))
        }
        _ => value.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_dates_and_instants() {
        assert_eq!(
            parse_date_millis("1970-01-02"),
            Some(86_400_000 + 43_200_000)
        );
        assert_eq!(parse_date_millis("2024-02-30"), None);
        assert_eq!(
            parse_instant_millis("1970-01-01T00:00:01.250Z"),
            Some(1_250)
        );
        assert_eq!(parse_instant_millis("1970-01-01T01:00:00+01:00"), Some(0));
        assert_eq!(parse_instant_millis("1970-01-01T00:00:00.5Z"), Some(500));
        assert_eq!(parse_instant_millis("1970-01-01 00:00:00Z"), None);
        assert_eq!(parse_instant_millis("1970-01-01T00:00:00"), None);
        assert_eq!(
            display_instant("2024-05-01T09:30:00.000Z"),
            "2024-05-01 09:30 UTC"
        );
    }

    #[test]
    fn derives_project_keys() {
        assert_eq!(project_key_base("Marketing launch 2025", "ASANA"), "ML2");
        assert_eq!(project_key_base("Roadmap", "ASANA"), "ROAD");
        assert_eq!(project_key_base("Équipe", "ASANA"), "QUIP");
        assert_eq!(project_key_base("日本", "ASANA"), "ASANA");
    }

    #[test]
    fn reads_retry_after_and_backs_off() {
        assert_eq!(parse_retry_after("2"), Some(Duration::from_secs(2)));
        assert_eq!(parse_retry_after("Wed, 21 Oct 2015 07:28:00 GMT"), None);
        let (initial, max) = (Duration::from_millis(500), Duration::from_secs(3));
        assert_eq!(backoff(initial, max, 1), Duration::from_millis(500));
        assert_eq!(backoff(initial, max, 3), Duration::from_secs(2));
        assert_eq!(backoff(initial, max, 9), Duration::from_secs(3));
    }
}
