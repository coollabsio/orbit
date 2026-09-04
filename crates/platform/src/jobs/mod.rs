mod schedule;
mod store;
mod worker;

use std::collections::BTreeSet;
use std::time::Duration;

use serde_json::Value;

use crate::{Id, TimestampMillis};

pub use schedule::{CatchUpMode, CronSchedule, RecurringSchedule, ScheduleError, Scheduler};
pub use store::{Claim, ClaimSelection, JobQueue, JobStore, JobStoreError};
pub use worker::{JobContext, Worker, WorkerConfig, WorkerConfigError};

pub const DEFAULT_MAX_ATTEMPTS: u32 = 8;
pub const DEFAULT_LEASE: Duration = Duration::from_secs(5 * 60);
pub const DEFAULT_HEARTBEAT: Duration = Duration::from_secs(30);
pub const DEFAULT_RETRY_DELAYS: [Duration; 7] = [
    Duration::from_secs(10),
    Duration::from_secs(30),
    Duration::from_secs(2 * 60),
    Duration::from_secs(10 * 60),
    Duration::from_secs(60 * 60),
    Duration::from_secs(6 * 60 * 60),
    Duration::from_secs(24 * 60 * 60),
];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JobKind {
    name: String,
    sensitive_fields: BTreeSet<String>,
    max_attempts: u32,
    retry_delays: Vec<Duration>,
    lease: Duration,
    concurrency_limit: Option<usize>,
}

impl JobKind {
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            sensitive_fields: BTreeSet::new(),
            max_attempts: DEFAULT_MAX_ATTEMPTS,
            retry_delays: DEFAULT_RETRY_DELAYS.to_vec(),
            lease: DEFAULT_LEASE,
            concurrency_limit: None,
        }
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn with_sensitive_fields<I, S>(mut self, fields: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.sensitive_fields = fields.into_iter().map(Into::into).collect();
        self
    }

    #[must_use]
    pub fn with_retry_policy(mut self, max_attempts: u32, delays: Vec<Duration>) -> Self {
        assert!(max_attempts > 0, "job kinds require at least one attempt");
        self.max_attempts = max_attempts;
        self.retry_delays = delays;
        self
    }

    #[must_use]
    pub fn with_lease(mut self, lease: Duration) -> Self {
        assert!(!lease.is_zero(), "job leases cannot be disabled");
        self.lease = lease;
        self
    }

    #[must_use]
    pub fn with_concurrency_limit(mut self, limit: usize) -> Self {
        assert!(limit > 0, "job kind concurrency must be positive");
        self.concurrency_limit = Some(limit);
        self
    }

    #[must_use]
    pub fn redact_payload(&self, payload: &Value) -> Value {
        fn redact(value: &mut Value, fields: &BTreeSet<String>) {
            match value {
                Value::Object(values) => {
                    for (name, value) in values {
                        if fields.contains(name) {
                            *value = Value::String("[REDACTED]".to_owned());
                        } else {
                            redact(value, fields);
                        }
                    }
                }
                Value::Array(values) => {
                    for value in values {
                        redact(value, fields);
                    }
                }
                _ => {}
            }
        }

        let mut redacted = payload.clone();
        redact(&mut redacted, &self.sensitive_fields);
        redacted
    }

    pub(crate) fn max_attempts(&self) -> u32 {
        self.max_attempts
    }

    pub(crate) fn retry_delay(&self, failed_attempt: u32) -> Option<Duration> {
        self.retry_delays
            .get(failed_attempt.saturating_sub(1) as usize)
            .copied()
    }

    pub(crate) fn lease(&self) -> Duration {
        self.lease
    }

    pub(crate) fn concurrency_limit(&self) -> Option<usize> {
        self.concurrency_limit
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum JobPriority {
    Low,
    #[default]
    Normal,
    High,
    Critical,
}

impl JobPriority {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Normal => "normal",
            Self::High => "high",
            Self::Critical => "critical",
        }
    }

    pub(crate) fn parse(value: &str) -> Result<Self, JobStoreError> {
        match value {
            "low" => Ok(Self::Low),
            "normal" => Ok(Self::Normal),
            "high" => Ok(Self::High),
            "critical" => Ok(Self::Critical),
            _ => Err(JobStoreError::InvalidData(format!(
                "unknown job priority {value}"
            ))),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JobState {
    Queued,
    Running,
    Succeeded,
    Failed,
    Dead,
}

impl JobState {
    pub(crate) fn parse(value: &str) -> Result<Self, JobStoreError> {
        match value {
            "queued" => Ok(Self::Queued),
            "running" => Ok(Self::Running),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "dead" => Ok(Self::Dead),
            _ => Err(JobStoreError::InvalidData(format!(
                "unknown job state {value}"
            ))),
        }
    }
}

#[derive(Clone, PartialEq)]
pub struct Job {
    pub id: Id,
    pub workspace_id: Option<String>,
    pub schedule_id: Option<Id>,
    pub scheduled_for: Option<TimestampMillis>,
    pub manual_retry_of: Option<Id>,
    pub kind: JobKind,
    pub payload: Value,
    pub state: JobState,
    pub priority: JobPriority,
    pub attempt_count: u32,
    pub max_attempts: u32,
    pub available_at: TimestampMillis,
    pub created_at: TimestampMillis,
    pub updated_at: TimestampMillis,
}

impl std::fmt::Debug for Job {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Job")
            .field("id", &self.id)
            .field("workspace_id", &self.workspace_id)
            .field("schedule_id", &self.schedule_id)
            .field("scheduled_for", &self.scheduled_for)
            .field("manual_retry_of", &self.manual_retry_of)
            .field("kind", &self.kind)
            .field("payload", &"[REDACTED]")
            .field("state", &self.state)
            .field("priority", &self.priority)
            .field("attempt_count", &self.attempt_count)
            .field("max_attempts", &self.max_attempts)
            .field("available_at", &self.available_at)
            .field("created_at", &self.created_at)
            .field("updated_at", &self.updated_at)
            .finish()
    }
}

impl Job {
    #[must_use]
    pub fn new(kind: JobKind, payload: Value, available_at: TimestampMillis) -> Self {
        let max_attempts = kind.max_attempts();
        Self {
            id: Id::new_v7(),
            workspace_id: None,
            schedule_id: None,
            scheduled_for: None,
            manual_retry_of: None,
            kind,
            payload,
            state: JobState::Queued,
            priority: JobPriority::Normal,
            attempt_count: 0,
            max_attempts,
            available_at,
            created_at: available_at,
            updated_at: available_at,
        }
    }

    #[must_use]
    pub fn with_priority(mut self, priority: JobPriority) -> Self {
        self.priority = priority;
        self
    }

    #[must_use]
    pub fn with_workspace(mut self, workspace_id: impl Into<String>) -> Self {
        self.workspace_id = Some(workspace_id.into());
        self
    }

    #[must_use]
    pub fn diagnostic_payload(&self) -> Value {
        self.kind.redact_payload(&self.payload)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum JobError {
    #[error("{0}")]
    Retryable(String),
    #[error("{0}")]
    Permanent(String),
}
