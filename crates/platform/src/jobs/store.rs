use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;
use std::time::Duration;

use sha2::{Digest, Sha256};
use sqlx::any::AnyRow;
use sqlx::{Any, Executor, Row, Transaction};
use thiserror::Error;

use super::{
    Job, JobError, JobKind, JobKindRegistrationError, JobKindRegistry, JobLimits, JobPriority,
    JobState,
};
use crate::{Database, Flag, Id, ParseIdError, TimestampMillis};

const DEAD_RETENTION_MILLIS: i64 = 30 * 24 * 60 * 60 * 1_000;

#[derive(Debug, Error)]
pub enum JobStoreError {
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Id(#[from] ParseIdError),
    #[error("invalid durable job data: {0}")]
    InvalidData(String),
    #[error("job {0} is not dead and cannot be retried")]
    NotDead(Id),
    #[error("job {0} does not exist")]
    NotFound(Id),
    #[error(transparent)]
    KindPolicy(#[from] JobKindRegistrationError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClaimSelection {
    Any,
    Critical,
    NonCritical,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Claim {
    pub job_id: Id,
    pub attempt: u32,
    pub claim_token: String,
    pub lease_expires_at: TimestampMillis,
    lease_duration: Duration,
}

#[derive(Clone, Debug)]
pub struct JobStore {
    database: Database,
    lease_owner: String,
    kinds: JobKindRegistry,
}

impl JobStore {
    #[must_use]
    pub fn new(database: Database) -> Self {
        Self::with_registry(database, JobKindRegistry::new())
    }

    #[must_use]
    pub fn with_registry(database: Database, kinds: JobKindRegistry) -> Self {
        Self {
            database,
            lease_owner: Id::new_v7().to_string(),
            kinds,
        }
    }

    pub fn register_kind(&self, kind: JobKind) -> Result<(), JobKindRegistrationError> {
        self.kinds.register(kind)
    }

    pub(crate) fn resolve_kind(&self, name: &str) -> JobKind {
        self.kinds.resolve(name)
    }

    pub async fn enqueue(&self, job: &Job) -> Result<Id, JobStoreError> {
        self.register_kind(job.kind.clone())?;
        insert_job(self.database.pool(), job).await?;
        Ok(job.id)
    }

    /// Queues `job` inside `transaction`, so it exists only if the transaction commits. The
    /// worker that runs the job registers its kind.
    pub async fn enqueue_in(
        transaction: &mut Transaction<'_, Any>,
        job: &Job,
    ) -> Result<Id, JobStoreError> {
        insert_job(&mut **transaction, job).await?;
        Ok(job.id)
    }

    pub async fn get(&self, id: Id) -> Result<Option<Job>, JobStoreError> {
        let row = sqlx::query(
            "SELECT id, workspace_id, schedule_id, scheduled_for, manual_retry_of, kind, \
             payload_json, state, priority, attempt_count, max_attempts, available_at, \
             created_at, updated_at FROM jobs WHERE id = $1",
        )
        .bind(id.to_string())
        .fetch_optional(self.database.pool())
        .await?;
        row.map(|row| decode_job(&row, &self.kinds)).transpose()
    }

    pub async fn claim(
        &self,
        now: TimestampMillis,
        selection: ClaimSelection,
    ) -> Result<Option<Claim>, JobStoreError> {
        let mut transaction = self.database.immediate_transaction().await?;
        let rows = sqlx::query(
            "SELECT id, state, attempt_count, max_attempts, claim_token, kind, workspace_id FROM jobs \
             WHERE ((state = 'queued' AND available_at <= $1) \
                    OR (state = 'running' AND lease_expires_at <= $2)) \
               AND ($3 = 'any' OR ($4 = 'critical' AND priority = 'critical') \
                    OR ($5 = 'noncritical' AND priority != 'critical')) \
             ORDER BY CASE priority WHEN 'critical' THEN 0 WHEN 'high' THEN 1 \
                       WHEN 'normal' THEN 2 ELSE 3 END, available_at, created_at, id",
        )
        .bind(now.as_millis())
        .bind(now.as_millis())
        .bind(selection_name(selection))
        .bind(selection_name(selection))
        .bind(selection_name(selection))
        .fetch_all(&mut *transaction)
        .await?;
        // Read on every claim, so a change in Admin reaches every server without a restart.
        let overrides = load_limit_overrides(&mut *transaction).await?;
        // A kind at its limit (no workspace) or a workspace at its limit, known for this claim.
        let mut blocked = BTreeSet::<(String, Option<String>)>::new();

        for row in rows {
            let kind: String = row.try_get("kind")?;
            let workspace_id: Option<String> = row.try_get("workspace_id")?;
            if blocked.contains(&(kind.clone(), None))
                || blocked.contains(&(kind.clone(), workspace_id.clone()))
            {
                continue;
            }
            let policy = self.kinds.resolve(&kind);
            let limits = overrides
                .get(&kind)
                .copied()
                .unwrap_or_else(|| policy.default_limits());
            // The claim transaction is serialized, so the counts include the jobs of every
            // server of the database.
            if let Some(limit) = limits.concurrency
                && count_running(&mut transaction, &kind, None, now).await? >= i64::from(limit)
            {
                blocked.insert((kind, None));
                continue;
            }
            if let (Some(limit), Some(workspace)) =
                (limits.workspace_concurrency, workspace_id.as_deref())
                && count_running(&mut transaction, &kind, Some(workspace), now).await?
                    >= i64::from(limit)
                && another_workspace_waits(&mut transaction, &kind, workspace, limit, now).await?
            {
                blocked.insert((kind, workspace_id));
                continue;
            }
            if let Some(claim) = self
                .claim_row(&mut transaction, &row, now, policy.lease())
                .await?
            {
                transaction.commit().await?;
                return Ok(Some(claim));
            }
        }

        transaction.commit().await?;
        Ok(None)
    }

    pub async fn database_now(&self) -> Result<TimestampMillis, JobStoreError> {
        Ok(self.database.database_now().await?)
    }

    pub async fn claim_at_database_time(
        &self,
        selection: ClaimSelection,
    ) -> Result<Option<Claim>, JobStoreError> {
        let now = self.database_now().await?;
        self.claim(now, selection).await
    }

    async fn claim_row(
        &self,
        transaction: &mut Transaction<'_, Any>,
        row: &AnyRow,
        now: TimestampMillis,
        lease: Duration,
    ) -> Result<Option<Claim>, JobStoreError> {
        let id: String = row.try_get("id")?;
        let state: String = row.try_get("state")?;
        let previous_attempt: i64 = row.try_get("attempt_count")?;
        let max_attempts: i64 = row.try_get("max_attempts")?;
        let old_token: Option<String> = row.try_get("claim_token")?;
        if state == "running" {
            sqlx::query(
                "UPDATE job_attempts SET finished_at = $1, outcome = 'lease_expired', \
                 error = 'job lease expired' WHERE job_id = $2 AND attempt = $3 \
                 AND finished_at IS NULL",
            )
            .bind(now.as_millis())
            .bind(&id)
            .bind(previous_attempt)
            .execute(&mut **transaction)
            .await?;
            if previous_attempt >= max_attempts {
                sqlx::query(
                    "UPDATE jobs SET state = 'dead', lease_owner = NULL, claim_token = NULL, \
                     lease_expires_at = NULL, last_error = 'job lease expired', dead_at = $1, \
                     updated_at = $2 WHERE id = $3 AND state = 'running' AND claim_token = $4",
                )
                .bind(now.as_millis())
                .bind(now.as_millis())
                .bind(&id)
                .bind(old_token)
                .execute(&mut **transaction)
                .await?;
                return Ok(None);
            }
        }

        let attempt = previous_attempt + 1;
        let claim_token = Id::new_v7().to_string();
        let lease_expires_at = add_duration(now, lease);
        let update = if state == "running" {
            sqlx::query(
                "UPDATE jobs SET state = 'running', attempt_count = $1, lease_owner = $2, \
                 claim_token = $3, lease_expires_at = $4, updated_at = $5 \
                 WHERE id = $6 AND state = 'running' AND claim_token = $7 \
                 AND lease_expires_at <= $8",
            )
            .bind(attempt)
            .bind(&self.lease_owner)
            .bind(&claim_token)
            .bind(lease_expires_at.as_millis())
            .bind(now.as_millis())
            .bind(&id)
            .bind(old_token)
            .bind(now.as_millis())
            .execute(&mut **transaction)
            .await?
        } else {
            sqlx::query(
                "UPDATE jobs SET state = 'running', attempt_count = $1, lease_owner = $2, \
                 claim_token = $3, lease_expires_at = $4, updated_at = $5 \
                 WHERE id = $6 AND state = 'queued' AND available_at <= $7",
            )
            .bind(attempt)
            .bind(&self.lease_owner)
            .bind(&claim_token)
            .bind(lease_expires_at.as_millis())
            .bind(now.as_millis())
            .bind(&id)
            .bind(now.as_millis())
            .execute(&mut **transaction)
            .await?
        };
        if update.rows_affected() == 0 {
            return Ok(None);
        }
        sqlx::query("INSERT INTO job_attempts (job_id, attempt, started_at) VALUES ($1, $2, $3)")
            .bind(&id)
            .bind(attempt)
            .bind(now.as_millis())
            .execute(&mut **transaction)
            .await?;

        Ok(Some(Claim {
            job_id: Id::from_str(&id)?,
            attempt: u32::try_from(attempt)
                .map_err(|_| JobStoreError::InvalidData("attempt count overflow".to_owned()))?,
            claim_token,
            lease_expires_at,
            lease_duration: lease,
        }))
    }

    pub async fn heartbeat(
        &self,
        claim: &Claim,
        now: TimestampMillis,
    ) -> Result<bool, JobStoreError> {
        self.heartbeat_with_lease(claim, now, claim.lease_duration)
            .await
    }

    pub(crate) async fn heartbeat_with_lease(
        &self,
        claim: &Claim,
        now: TimestampMillis,
        lease: Duration,
    ) -> Result<bool, JobStoreError> {
        let result = sqlx::query(
            "UPDATE jobs SET lease_expires_at = $1, updated_at = $2 \
             WHERE id = $3 AND state = 'running' AND attempt_count = $4 AND claim_token = $5",
        )
        .bind(add_duration(now, lease).as_millis())
        .bind(now.as_millis())
        .bind(claim.job_id.to_string())
        .bind(i64::from(claim.attempt))
        .bind(&claim.claim_token)
        .execute(self.database.pool())
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn complete(
        &self,
        claim: &Claim,
        now: TimestampMillis,
    ) -> Result<bool, JobStoreError> {
        let mut transaction = self.database.immediate_transaction().await?;
        let result = sqlx::query(
            "UPDATE jobs SET state = 'succeeded', lease_owner = NULL, claim_token = NULL, \
             lease_expires_at = NULL, updated_at = $1 \
             WHERE id = $2 AND state = 'running' AND attempt_count = $3 AND claim_token = $4",
        )
        .bind(now.as_millis())
        .bind(claim.job_id.to_string())
        .bind(i64::from(claim.attempt))
        .bind(&claim.claim_token)
        .execute(&mut *transaction)
        .await?;
        if result.rows_affected() == 1 {
            finish_attempt(&mut transaction, claim, now, "succeeded", None).await?;
        }
        transaction.commit().await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn fail(
        &self,
        claim: &Claim,
        error: JobError,
        now: TimestampMillis,
    ) -> Result<bool, JobStoreError> {
        let mut transaction = self.database.immediate_transaction().await?;
        let row = sqlx::query(
            "SELECT max_attempts, kind FROM jobs WHERE id = $1 AND state = 'running' \
             AND attempt_count = $2 AND claim_token = $3",
        )
        .bind(claim.job_id.to_string())
        .bind(i64::from(claim.attempt))
        .bind(&claim.claim_token)
        .fetch_optional(&mut *transaction)
        .await?;
        let Some(row) = row else {
            transaction.commit().await?;
            return Ok(false);
        };
        let max_attempts: i64 = row.try_get("max_attempts")?;
        let kind = self.kinds.resolve(row.try_get("kind")?);

        let message = error.to_string();
        let retry_delay = match error {
            JobError::Retryable(_) if i64::from(claim.attempt) < max_attempts => {
                kind.retry_delay(claim.attempt)
            }
            JobError::Retryable(_) | JobError::Permanent(_) => None,
        };
        let (state, available_at, dead_at, outcome) = match retry_delay {
            Some(base) => {
                let delay = jittered_delay(base, &claim.claim_token, claim.attempt);
                (
                    "queued",
                    add_duration(now, delay).as_millis(),
                    None,
                    "retryable",
                )
            }
            None => ("dead", now.as_millis(), Some(now.as_millis()), "dead"),
        };
        let result = sqlx::query(
            "UPDATE jobs SET state = $1, available_at = $2, lease_owner = NULL, \
             claim_token = NULL, lease_expires_at = NULL, last_error = $3, dead_at = $4, \
             updated_at = $5 WHERE id = $6 AND state = 'running' AND attempt_count = $7 \
             AND claim_token = $8",
        )
        .bind(state)
        .bind(available_at)
        .bind(&message)
        .bind(dead_at)
        .bind(now.as_millis())
        .bind(claim.job_id.to_string())
        .bind(i64::from(claim.attempt))
        .bind(&claim.claim_token)
        .execute(&mut *transaction)
        .await?;
        if result.rows_affected() == 1 {
            finish_attempt(&mut transaction, claim, now, outcome, Some(&message)).await?;
        }
        transaction.commit().await?;
        Ok(result.rows_affected() == 1)
    }

    /// Puts a running job back in the queue without spending its attempt, for work interrupted
    /// by a graceful shutdown: the attempt row is removed and `attempt_count` goes back down, so
    /// the next claim reuses the attempt number. Returns `false` for a stale claim.
    pub async fn release(
        &self,
        claim: &Claim,
        now: TimestampMillis,
    ) -> Result<bool, JobStoreError> {
        let mut transaction = self.database.immediate_transaction().await?;
        let result = sqlx::query(
            "UPDATE jobs SET state = 'queued', attempt_count = attempt_count - 1, \
             available_at = $1, lease_owner = NULL, claim_token = NULL, lease_expires_at = NULL, \
             updated_at = $2 WHERE id = $3 AND state = 'running' AND attempt_count = $4 \
             AND claim_token = $5",
        )
        .bind(now.as_millis())
        .bind(now.as_millis())
        .bind(claim.job_id.to_string())
        .bind(i64::from(claim.attempt))
        .bind(&claim.claim_token)
        .execute(&mut *transaction)
        .await?;
        if result.rows_affected() == 1 {
            sqlx::query("DELETE FROM job_attempts WHERE job_id = $1 AND attempt = $2")
                .bind(claim.job_id.to_string())
                .bind(i64::from(claim.attempt))
                .execute(&mut *transaction)
                .await?;
        }
        transaction.commit().await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn manual_retry(
        &self,
        original_id: Id,
        now: TimestampMillis,
    ) -> Result<Id, JobStoreError> {
        let original = self
            .get(original_id)
            .await?
            .ok_or(JobStoreError::NotFound(original_id))?;
        if original.state != JobState::Dead {
            return Err(JobStoreError::NotDead(original_id));
        }
        let mut retry =
            Job::new(original.kind, original.payload, now).with_priority(original.priority);
        retry.workspace_id = original.workspace_id;
        retry.manual_retry_of = Some(original_id);
        retry.max_attempts = original.max_attempts;
        self.enqueue(&retry).await
    }

    pub async fn purge_dead(&self, now: TimestampMillis) -> Result<u64, JobStoreError> {
        let cutoff = now.as_millis().saturating_sub(DEAD_RETENTION_MILLIS);
        Ok(sqlx::query(
            "DELETE FROM jobs WHERE state = 'dead' AND dead_at <= $1 AND incident_hold = 0",
        )
        .bind(cutoff)
        .execute(self.database.pool())
        .await?
        .rows_affected())
    }

    pub(crate) fn database(&self) -> &Database {
        &self.database
    }
}

#[derive(Clone, Debug)]
pub struct JobQueue {
    store: JobStore,
}

impl JobQueue {
    #[must_use]
    pub fn new(store: JobStore) -> Self {
        Self { store }
    }

    pub async fn enqueue(&self, job: Job) -> Result<Id, JobStoreError> {
        self.store.enqueue(&job).await
    }
}

async fn insert_job<'e>(
    executor: impl Executor<'e, Database = Any>,
    job: &Job,
) -> Result<u64, JobStoreError> {
    Ok(sqlx::query(
        "INSERT INTO jobs (id, workspace_id, schedule_id, scheduled_for, \
         manual_retry_of, kind, payload_json, state, priority, attempt_count, max_attempts, \
         available_at, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7, 'queued', $8, 0, $9, $10, $11, $12) \
         ON CONFLICT DO NOTHING",
    )
    .bind(job.id.to_string())
    .bind(&job.workspace_id)
    .bind(job.schedule_id.map(|id| id.to_string()))
    .bind(job.scheduled_for.map(TimestampMillis::as_millis))
    .bind(job.manual_retry_of.map(|id| id.to_string()))
    .bind(job.kind.name())
    .bind(serde_json::to_string(&job.payload)?)
    .bind(job.priority.as_str())
    .bind(i64::from(job.max_attempts))
    .bind(job.available_at.as_millis())
    .bind(job.created_at.as_millis())
    .bind(job.updated_at.as_millis())
    .execute(executor)
    .await?
    .rows_affected())
}

fn decode_job(row: &AnyRow, kinds: &JobKindRegistry) -> Result<Job, JobStoreError> {
    fn optional_id(value: Option<String>) -> Result<Option<Id>, ParseIdError> {
        value.map(|value| Id::from_str(&value)).transpose()
    }

    let attempt_count: i64 = row.try_get("attempt_count")?;
    let max_attempts: i64 = row.try_get("max_attempts")?;
    Ok(Job {
        id: Id::from_str(row.try_get::<&str, _>("id")?)?,
        workspace_id: row.try_get("workspace_id")?,
        schedule_id: optional_id(row.try_get("schedule_id")?)?,
        scheduled_for: row
            .try_get::<Option<i64>, _>("scheduled_for")?
            .map(TimestampMillis::from_millis),
        manual_retry_of: optional_id(row.try_get("manual_retry_of")?)?,
        kind: kinds.resolve(row.try_get("kind")?),
        payload: serde_json::from_str(row.try_get::<&str, _>("payload_json")?)?,
        state: JobState::parse(row.try_get("state")?)?,
        priority: JobPriority::parse(row.try_get("priority")?)?,
        attempt_count: u32::try_from(attempt_count)
            .map_err(|_| JobStoreError::InvalidData("attempt count is negative".to_owned()))?,
        max_attempts: u32::try_from(max_attempts)
            .map_err(|_| JobStoreError::InvalidData("max attempts is invalid".to_owned()))?,
        available_at: TimestampMillis::from_millis(row.try_get("available_at")?),
        created_at: TimestampMillis::from_millis(row.try_get("created_at")?),
        updated_at: TimestampMillis::from_millis(row.try_get("updated_at")?),
    })
}

async fn finish_attempt(
    transaction: &mut Transaction<'_, Any>,
    claim: &Claim,
    now: TimestampMillis,
    outcome: &str,
    error: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE job_attempts SET finished_at = $1, outcome = $2, error = $3 \
         WHERE job_id = $4 AND attempt = $5 AND finished_at IS NULL",
    )
    .bind(now.as_millis())
    .bind(outcome)
    .bind(error)
    .bind(claim.job_id.to_string())
    .bind(i64::from(claim.attempt))
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

/// The `job_limits` rows: the limits the root user set in Admin, by job kind.
pub async fn load_limit_overrides<'e>(
    executor: impl Executor<'e, Database = Any>,
) -> Result<BTreeMap<String, JobLimits>, sqlx::Error> {
    let rows = sqlx::query("SELECT kind, concurrency, workspace_concurrency FROM job_limits")
        .fetch_all(executor)
        .await?;
    let limit = |value: Option<i64>| value.and_then(|value| u32::try_from(value).ok());
    rows.iter()
        .map(|row| {
            Ok((
                row.try_get("kind")?,
                JobLimits {
                    concurrency: limit(row.try_get("concurrency")?),
                    workspace_concurrency: limit(row.try_get("workspace_concurrency")?),
                },
            ))
        })
        .collect()
}

async fn count_running(
    transaction: &mut Transaction<'_, Any>,
    kind: &str,
    workspace_id: Option<&str>,
    now: TimestampMillis,
) -> Result<i64, sqlx::Error> {
    match workspace_id {
        None => {
            sqlx::query_scalar(
                "SELECT COUNT(*) FROM jobs WHERE kind = $1 AND state = 'running' \
                 AND lease_expires_at > $2",
            )
            .bind(kind)
            .bind(now.as_millis())
            .fetch_one(&mut **transaction)
            .await
        }
        Some(workspace_id) => {
            sqlx::query_scalar(
                "SELECT COUNT(*) FROM jobs WHERE kind = $1 AND state = 'running' \
                 AND lease_expires_at > $2 AND workspace_id = $3",
            )
            .bind(kind)
            .bind(now.as_millis())
            .bind(workspace_id)
            .fetch_one(&mut **transaction)
            .await
        }
    }
}

/// Whether another workspace below its limit has a job of `kind` ready to run.
async fn another_workspace_waits(
    transaction: &mut Transaction<'_, Any>,
    kind: &str,
    workspace_id: &str,
    limit: u32,
    now: TimestampMillis,
) -> Result<bool, sqlx::Error> {
    let Flag(waits) = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM jobs waiting WHERE waiting.kind = $1 \
         AND waiting.state = 'queued' AND waiting.available_at <= $2 \
         AND waiting.workspace_id IS NOT NULL AND waiting.workspace_id != $3 \
         AND (SELECT COUNT(*) FROM jobs running WHERE running.kind = $4 \
              AND running.workspace_id = waiting.workspace_id AND running.state = 'running' \
              AND running.lease_expires_at > $5) < $6)",
    )
    .bind(kind)
    .bind(now.as_millis())
    .bind(workspace_id)
    .bind(kind)
    .bind(now.as_millis())
    .bind(i64::from(limit))
    .fetch_one(&mut **transaction)
    .await?;
    Ok(waits)
}

fn selection_name(selection: ClaimSelection) -> &'static str {
    match selection {
        ClaimSelection::Any => "any",
        ClaimSelection::Critical => "critical",
        ClaimSelection::NonCritical => "noncritical",
    }
}

pub(crate) fn add_duration(timestamp: TimestampMillis, duration: Duration) -> TimestampMillis {
    let millis = i64::try_from(duration.as_millis()).unwrap_or(i64::MAX);
    TimestampMillis::from_millis(timestamp.as_millis().saturating_add(millis))
}

fn jittered_delay(base: Duration, token: &str, attempt: u32) -> Duration {
    let mut digest = Sha256::new();
    digest.update(token.as_bytes());
    digest.update(attempt.to_le_bytes());
    let bytes: [u8; 8] = digest.finalize()[..8].try_into().expect("eight bytes");
    let basis_points = u64::from_le_bytes(bytes) % 2_001;
    let base_millis = u64::try_from(base.as_millis()).unwrap_or(u64::MAX);
    Duration::from_millis(
        base_millis.saturating_add(base_millis.saturating_mul(basis_points) / 10_000),
    )
}
