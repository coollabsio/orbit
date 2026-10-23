//! Received messages (`mail_receipts`, `mail_receipt_recipients`) and health test results, as
//! written by the intake (`crate::mail_intake`) inside its transaction, and the list of receipts
//! that need **Retry**.

use orbit_platform::{Id, JobKind, Row, TimestampMillis};
use sqlx::{Any, Transaction};

use super::domains::{manager, parse_id};
use super::{MailError, MailRepository};

/// A `pending` receipt untouched this long without a waiting or running ingest job is stuck:
/// the server stopped during its last attempt, so no job will finish it.
pub const STUCK_RECEIPT_MS: i64 = 60 * 60 * 1000;
/// The most receipts [`MailRepository::failed_receipts`] lists.
const FAILED_LIST_MAX: i64 = 100;

/// `true` for a stuck receipt `r`; `$2` is the stuck cutoff. The ingest job's payload is
/// `{"receipt_id":"<id>"}` (as written by the intake and by retry).
pub(super) const STUCK_SQL: &str = "(r.status = 'pending' AND r.updated_at <= $2 AND NOT EXISTS \
     (SELECT 1 FROM jobs j WHERE j.kind = 'mail.ingest' AND j.workspace_id = r.workspace_id \
     AND j.state IN ('queued', 'running') \
     AND j.payload_json = '{\"receipt_id\":\"' || r.id || '\"}'))";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReceiptProblem {
    /// Every ingest attempt failed.
    Failed,
    /// Pending without an ingest job (see [`STUCK_RECEIPT_MS`]).
    Stuck,
}

/// A receipt that needs **Retry**.
#[derive(Clone, Debug)]
pub struct FailedReceipt {
    pub id: Id,
    pub problem: ReceiptProblem,
    pub mail_from: Option<String>,
    pub size: i64,
    pub error: Option<String>,
    pub attempts: i64,
    pub received_at: TimestampMillis,
    pub updated_at: TimestampMillis,
}

#[derive(Clone, Debug)]
pub struct FailedReceipts {
    /// All of them; `receipts` holds the newest 100.
    pub count: i64,
    pub receipts: Vec<FailedReceipt>,
}

/// The job that parses a receipt into messages and threads.
pub const INGEST_JOB: &str = "mail.ingest";

/// `mail.ingest`: four at a time on the cluster, two of them for one workspace while another one
/// waits; default retries.
#[must_use]
pub fn mail_ingest_kind() -> JobKind {
    JobKind::new(INGEST_JOB)
        .with_concurrency_limit(4)
        .with_workspace_limit(2)
}

/// A receipt row: the sealed raw message is blob `blob_id`, sealed with data key `key_version`.
pub(crate) struct NewReceipt<'a> {
    pub id: Id,
    pub workspace_id: Id,
    pub source: &'a str,
    pub blob_id: Id,
    pub key_version: i64,
    pub raw_sha256: &'a str,
    pub size: i64,
    pub mail_from: Option<&'a str>,
    pub peer_ip: String,
    pub helo: &'a str,
    pub tls: Option<String>,
    pub auth: String,
}

pub(crate) async fn insert_receipt(
    transaction: &mut Transaction<'_, Any>,
    receipt: &NewReceipt<'_>,
    now: TimestampMillis,
) -> Result<(), MailError> {
    sqlx::query(
        "INSERT INTO mail_receipts (id, workspace_id, source, blob_id, key_version, raw_sha256, size, \
         mail_from, peer_ip, helo, tls, auth, status, received_at, updated_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, 'pending', $13, $14)",
    )
    .bind(receipt.id.to_string())
    .bind(receipt.workspace_id.to_string())
    .bind(receipt.source)
    .bind(receipt.blob_id.to_string())
    .bind(receipt.key_version)
    .bind(receipt.raw_sha256)
    .bind(receipt.size)
    .bind(receipt.mail_from)
    .bind(&receipt.peer_ip)
    .bind(receipt.helo)
    .bind(receipt.tls.as_deref())
    .bind(&receipt.auth)
    .bind(now.as_millis())
    .bind(now.as_millis())
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

/// One `RCPT TO` of a receipt; the same forward-path twice is stored once.
pub(crate) async fn insert_receipt_recipient(
    transaction: &mut Transaction<'_, Any>,
    receipt_id: Id,
    address_id: Id,
    rcpt: &str,
) -> Result<(), MailError> {
    sqlx::query(
        "INSERT INTO mail_receipt_recipients (receipt_id, address_id, rcpt) VALUES ($1, $2, $3) \
         ON CONFLICT (receipt_id, rcpt) DO NOTHING",
    )
    .bind(receipt_id.to_string())
    .bind(address_id.to_string())
    .bind(rcpt)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

/// Stores what a health test message showed (`result` is JSON). A test that is gone (its
/// domain removed) is skipped.
pub(crate) async fn record_health_test(
    transaction: &mut Transaction<'_, Any>,
    test_id: Id,
    result: &str,
    now: TimestampMillis,
) -> Result<(), MailError> {
    sqlx::query("UPDATE mail_health_tests SET result = $1, received_at = $2 WHERE id = $3")
        .bind(result)
        .bind(now.as_millis())
        .bind(test_id.to_string())
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

impl MailRepository {
    /// Owners and admins: the failed and stuck receipts of the workspace, newest first.
    pub async fn failed_receipts(
        &self,
        workspace_id: Id,
        actor_id: Id,
    ) -> Result<FailedReceipts, MailError> {
        let now = self.database.database_now().await?;
        let mut transaction = self.database.pool().begin().await?;
        manager(&mut transaction, workspace_id, actor_id).await?;
        let filter = format!("r.workspace_id = $1 AND (r.status = 'failed' OR {STUCK_SQL})");
        let cutoff = now.as_millis() - STUCK_RECEIPT_MS;
        let count: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) FROM mail_receipts r WHERE {filter}"
        )))
        .bind(workspace_id.to_string())
        .bind(cutoff)
        .fetch_one(&mut *transaction)
        .await?;
        let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT r.id, r.status, r.mail_from, r.size, r.error, r.attempts, r.received_at, \
             r.updated_at FROM mail_receipts r WHERE {filter} \
             ORDER BY r.received_at DESC, r.id DESC LIMIT $3"
        )))
        .bind(workspace_id.to_string())
        .bind(cutoff)
        .bind(FAILED_LIST_MAX)
        .fetch_all(&mut *transaction)
        .await?;
        transaction.commit().await?;
        let receipts = rows
            .iter()
            .map(|row| {
                let status: String = row.try_get("status")?;
                Ok(FailedReceipt {
                    id: parse_id(&row.try_get::<String, _>("id")?)?,
                    problem: if status == "failed" {
                        ReceiptProblem::Failed
                    } else {
                        ReceiptProblem::Stuck
                    },
                    mail_from: row.try_get("mail_from")?,
                    size: row.try_get("size")?,
                    error: row.try_get("error")?,
                    attempts: row.try_get("attempts")?,
                    received_at: TimestampMillis::from_millis(row.try_get("received_at")?),
                    updated_at: TimestampMillis::from_millis(row.try_get("updated_at")?),
                })
            })
            .collect::<Result<_, MailError>>()?;
        Ok(FailedReceipts { count, receipts })
    }
}
