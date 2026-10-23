//! Received messages (`mail_receipts`, `mail_receipt_recipients`) and health test results, as
//! written by the intake (`crate::mail_intake`) inside its transaction.

use orbit_platform::{Id, JobKind, TimestampMillis};
use sqlx::{Any, Transaction};

use super::MailError;

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
