use std::str::FromStr;
use std::time::Duration;

use sha2::{Digest, Sha256};
use sqlx::{Row, Sqlite, Transaction};
use thiserror::Error;

use super::{Job, JobError, JobKind, JobKindRegistry, JobPriority, JobState};
use crate::{Database, Id, ParseIdError, TimestampMillis};

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

    pub fn register_kind(&self, kind: JobKind) {
        self.kinds.register(kind);
    }

    pub(crate) fn resolve_kind(&self, name: &str) -> JobKind {
        self.kinds.resolve(name)
    }

    pub async fn enqueue(&self, job: &Job) -> Result<Id, JobStoreError> {
        self.register_kind(job.kind.clone());
        insert_job(&self.database, job).await?;
        Ok(job.id)
    }

    pub async fn get(&self, id: Id) -> Result<Option<Job>, JobStoreError> {
        let row = sqlx::query(
            "SELECT id, workspace_id, schedule_id, scheduled_for, manual_retry_of, kind, \
             payload_json, state, priority, attempt_count, max_attempts, available_at, \
             created_at, updated_at FROM jobs WHERE id = ?",
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
        self.claim_excluding(now, selection, &[]).await
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

    pub(crate) async fn claim_excluding_at_database_time(
        &self,
        selection: ClaimSelection,
        excluded_kinds: &[String],
    ) -> Result<Option<Claim>, JobStoreError> {
        let now = self.database_now().await?;
        self.claim_excluding(now, selection, excluded_kinds).await
    }

    pub(crate) async fn claim_excluding(
        &self,
        now: TimestampMillis,
        selection: ClaimSelection,
        excluded_kinds: &[String],
    ) -> Result<Option<Claim>, JobStoreError> {
        let mut transaction = self.database.immediate_transaction().await?;
        let rows = sqlx::query(
            "SELECT id, state, attempt_count, max_attempts, claim_token, kind FROM jobs \
             WHERE ((state = 'queued' AND available_at <= ?) \
                    OR (state = 'running' AND lease_expires_at <= ?)) \
               AND (? = 'any' OR (? = 'critical' AND priority = 'critical') \
                    OR (? = 'noncritical' AND priority != 'critical')) \
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

        for row in rows {
            let kind: String = row.try_get("kind")?;
            if excluded_kinds.contains(&kind) {
                continue;
            }
            let policy = self.kinds.resolve(&kind);
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

    async fn claim_row(
        &self,
        transaction: &mut Transaction<'_, Sqlite>,
        row: &sqlx::sqlite::SqliteRow,
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
                "UPDATE job_attempts SET finished_at = ?, outcome = 'lease_expired', \
                 error = 'job lease expired' WHERE job_id = ? AND attempt = ? \
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
                     lease_expires_at = NULL, last_error = 'job lease expired', dead_at = ?, \
                     updated_at = ? WHERE id = ? AND state = 'running' AND claim_token = ?",
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
                "UPDATE jobs SET state = 'running', attempt_count = ?, lease_owner = ?, \
                 claim_token = ?, lease_expires_at = ?, updated_at = ? \
                 WHERE id = ? AND state = 'running' AND claim_token = ? \
                 AND lease_expires_at <= ?",
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
                "UPDATE jobs SET state = 'running', attempt_count = ?, lease_owner = ?, \
                 claim_token = ?, lease_expires_at = ?, updated_at = ? \
                 WHERE id = ? AND state = 'queued' AND available_at <= ?",
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
        sqlx::query("INSERT INTO job_attempts (job_id, attempt, started_at) VALUES (?, ?, ?)")
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
            "UPDATE jobs SET lease_expires_at = ?, updated_at = ? \
             WHERE id = ? AND state = 'running' AND attempt_count = ? AND claim_token = ?",
        )
        .bind(add_duration(now, lease).as_millis())
        .bind(now.as_millis())
        .bind(claim.job_id.to_string())
        .bind(claim.attempt)
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
        let mut transaction = self.database.transaction().await?;
        let result = sqlx::query(
            "UPDATE jobs SET state = 'succeeded', lease_owner = NULL, claim_token = NULL, \
             lease_expires_at = NULL, updated_at = ? \
             WHERE id = ? AND state = 'running' AND attempt_count = ? AND claim_token = ?",
        )
        .bind(now.as_millis())
        .bind(claim.job_id.to_string())
        .bind(claim.attempt)
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
        let mut transaction = self.database.transaction().await?;
        let row = sqlx::query(
            "SELECT max_attempts, kind FROM jobs WHERE id = ? AND state = 'running' \
             AND attempt_count = ? AND claim_token = ?",
        )
        .bind(claim.job_id.to_string())
        .bind(claim.attempt)
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
            "UPDATE jobs SET state = ?, available_at = ?, lease_owner = NULL, \
             claim_token = NULL, lease_expires_at = NULL, last_error = ?, dead_at = ?, \
             updated_at = ? WHERE id = ? AND state = 'running' AND attempt_count = ? \
             AND claim_token = ?",
        )
        .bind(state)
        .bind(available_at)
        .bind(&message)
        .bind(dead_at)
        .bind(now.as_millis())
        .bind(claim.job_id.to_string())
        .bind(claim.attempt)
        .bind(&claim.claim_token)
        .execute(&mut *transaction)
        .await?;
        if result.rows_affected() == 1 {
            finish_attempt(&mut transaction, claim, now, outcome, Some(&message)).await?;
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
            "DELETE FROM jobs WHERE state = 'dead' AND dead_at <= ? AND incident_hold = 0",
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

pub(crate) async fn insert_job(database: &Database, job: &Job) -> Result<u64, JobStoreError> {
    Ok(sqlx::query(
        "INSERT OR IGNORE INTO jobs (id, workspace_id, schedule_id, scheduled_for, \
         manual_retry_of, kind, payload_json, state, priority, attempt_count, max_attempts, \
         available_at, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, 'queued', ?, 0, ?, ?, ?, ?)",
    )
    .bind(job.id.to_string())
    .bind(&job.workspace_id)
    .bind(job.schedule_id.map(|id| id.to_string()))
    .bind(job.scheduled_for.map(TimestampMillis::as_millis))
    .bind(job.manual_retry_of.map(|id| id.to_string()))
    .bind(job.kind.name())
    .bind(serde_json::to_string(&job.payload)?)
    .bind(job.priority.as_str())
    .bind(job.max_attempts)
    .bind(job.available_at.as_millis())
    .bind(job.created_at.as_millis())
    .bind(job.updated_at.as_millis())
    .execute(database.pool())
    .await?
    .rows_affected())
}

fn decode_job(
    row: &sqlx::sqlite::SqliteRow,
    kinds: &JobKindRegistry,
) -> Result<Job, JobStoreError> {
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
    transaction: &mut Transaction<'_, Sqlite>,
    claim: &Claim,
    now: TimestampMillis,
    outcome: &str,
    error: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE job_attempts SET finished_at = ?, outcome = ?, error = ? \
         WHERE job_id = ? AND attempt = ? AND finished_at IS NULL",
    )
    .bind(now.as_millis())
    .bind(outcome)
    .bind(error)
    .bind(claim.job_id.to_string())
    .bind(claim.attempt)
    .execute(&mut **transaction)
    .await?;
    Ok(())
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
