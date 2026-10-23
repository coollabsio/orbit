//! Ingest (spec §4.4, §6, §9): a receipt becomes a parsed message in each recipient mailbox,
//! failed receipts are retried, and old trash is purged (spec §6.4).
//!
//! The raw message is read and parsed before the transaction; the message, its attachments, the
//! threads, the search rows and the receipt's `ingested` mark are written in one
//! `immediate_transaction()`. A re-run is safe: the message is unique for its receipt and each
//! mailbox gets the same bytes once (`mail_thread_messages (mailbox_id, raw_sha256)`), so a retry
//! after any failure adds nothing twice. Live events go out after the commit.

use std::borrow::Cow;

use orbit_mail::{
    AttachmentInfo, AuthOutcome, DmarcVerdict, Mailbox, ParseError, ParseLimits, ParsedMessage,
    parse,
};
use orbit_platform::{Id, Job, JobError, JobKind, JobStore, Row, TimestampMillis};
use serde_json::{Value, json};
use sqlx::{Any, Transaction};

use super::domains::{manager, parse_id};
use super::threads::{self, Incoming, is_inside, truncate};
use super::{MailError, MailRepository, mail_ingest_kind, open_blob, receipt_aad};
use crate::live::{LiveHub, NewEvent, Recipients};

/// The daily job that deletes old trash and the messages and receipts nothing uses any more.
pub const PURGE_JOB: &str = "mail.purge";
/// How long a thread stays in Trash (spec §6.4: 30 days).
pub const TRASH_RETENTION_MS: i64 = 30 * 24 * 60 * 60 * 1000;
/// The live socket topic of mail events.
pub const MAIL_TOPIC: &str = "mail";
/// The plain-text body kept for a message that did not parse (the raw message stays).
const FALLBACK_BODY_BYTES: usize = 1024 * 1024;
const SNIPPET_CHARS: usize = 200;
/// Kept of an ingest error on the receipt.
const ERROR_CHARS: usize = 1000;

/// `mail.purge`: one at a time on the cluster.
#[must_use]
pub fn mail_purge_kind() -> JobKind {
    JobKind::new(PURGE_JOB).with_concurrency_limit(1)
}

/// What an ingest did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IngestOutcome {
    /// The receipt is gone (its workspace was deleted or it was purged).
    Missing,
    /// An earlier run ingested it.
    AlreadyIngested,
    /// `deliveries` holds the mailboxes that got the message now: empty when every mailbox
    /// already had these bytes, or no recipient address is left.
    Ingested {
        message_id: Id,
        deliveries: Vec<Delivery>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delivery {
    pub mailbox_id: Id,
    pub thread_id: Id,
    pub new_thread: bool,
}

/// What a purge deleted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PurgeResult {
    pub threads: u64,
    pub messages: u64,
    pub receipts: u64,
}

struct Receipt {
    workspace_id: Id,
    storage_key: String,
    key_version: i64,
    raw_sha256: String,
    size: i64,
    auth: String,
    status: String,
    received_at: TimestampMillis,
}

/// The parsed message, or the raw text with a warning when it did not parse.
struct Content {
    parsed: ParsedMessage,
    warning: Option<&'static str>,
}

impl Content {
    fn of(raw: &[u8]) -> Self {
        match parse(raw, &ParseLimits::default()) {
            Ok(parsed) => Self {
                parsed,
                warning: None,
            },
            Err(error) => Self {
                parsed: plain_text(raw),
                warning: Some(match error {
                    ParseError::Limit(_) => "limit",
                    ParseError::Malformed => "parse_failed",
                }),
            },
        }
    }

    fn has_attachments(&self) -> bool {
        self.parsed
            .attachments
            .iter()
            .any(|attachment| !attachment.inline)
    }

    /// `In-Reply-To` and `References`, without duplicates.
    fn thread_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = Vec::new();
        for id in self
            .parsed
            .in_reply_to
            .iter()
            .chain(&self.parsed.references)
        {
            let id = clean(id).into_owned();
            if !id.is_empty() && !ids.contains(&id) {
                ids.push(id);
            }
        }
        ids
    }
}

/// The raw message as the text body (capped), with no headers parsed.
fn plain_text(raw: &[u8]) -> ParsedMessage {
    let mut body_text =
        String::from_utf8_lossy(&raw[..raw.len().min(FALLBACK_BODY_BYTES)]).replace('\0', "");
    truncate(&mut body_text, FALLBACK_BODY_BYTES);
    ParsedMessage {
        message_id: None,
        in_reply_to: Vec::new(),
        references: Vec::new(),
        from: None,
        sender: None,
        reply_to: Vec::new(),
        to: Vec::new(),
        cc: Vec::new(),
        subject: String::new(),
        normalized_subject: String::new(),
        sent_at: None,
        snippet: snippet(&body_text),
        body_text,
        html_part: None,
        attachments: Vec::new(),
    }
}

fn snippet(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(SNIPPET_CHARS)
        .collect::<String>()
        .trim_end()
        .to_owned()
}

/// Postgres text cannot hold NUL; mail text can.
fn clean(text: &str) -> Cow<'_, str> {
    if text.contains('\0') {
        Cow::Owned(text.replace('\0', ""))
    } else {
        Cow::Borrowed(text)
    }
}

fn clean_opt(text: Option<&str>) -> Option<String> {
    text.map(|text| clean(text).into_owned())
}

/// A JSON list of `{"name", "address"}`.
fn people_json(list: &[Mailbox]) -> String {
    Value::Array(
        list.iter()
            .map(|mailbox| {
                json!({
                    "name": clean_opt(mailbox.name.as_deref()),
                    "address": clean(&mailbox.address),
                })
            })
            .collect(),
    )
    .to_string()
}

impl MailRepository {
    /// Parses receipt `receipt_id` and delivers it to each recipient mailbox (spec §4.4). Safe to
    /// run again at any point; see the module docs.
    pub async fn ingest(
        &self,
        receipt_id: Id,
        now: TimestampMillis,
    ) -> Result<IngestOutcome, MailError> {
        let Some(receipt) = load_receipt(self.database.pool(), receipt_id).await? else {
            return Ok(IngestOutcome::Missing);
        };
        if receipt.status == "ingested" {
            return Ok(IngestOutcome::AlreadyIngested);
        }
        let key = self
            .keys
            .get(receipt.workspace_id, receipt.key_version)
            .await?;
        let sealed = self
            .uploads
            .read_blob(&receipt.storage_key)
            .await
            .map_err(MailError::Upload)?;
        let raw = open_blob(&key, &receipt_aad(receipt_id), &sealed)?;
        drop(sealed);
        let content = Content::of(&raw);
        drop(raw);
        let dmarc_failed = serde_json::from_str::<AuthOutcome>(&receipt.auth)
            .is_ok_and(|auth| matches!(auth.dmarc, DmarcVerdict::Fail { .. }));

        let workspace_id = receipt.workspace_id;
        let mut transaction = self.database.immediate_transaction().await?;
        // Another server may have finished it meanwhile.
        let status: Option<String> =
            sqlx::query_scalar("SELECT status FROM mail_receipts WHERE id = $1")
                .bind(receipt_id.to_string())
                .fetch_optional(&mut *transaction)
                .await?;
        match status.as_deref() {
            None => return Ok(IngestOutcome::Missing),
            Some("ingested") => return Ok(IngestOutcome::AlreadyIngested),
            Some(_) => {}
        }
        let message_id =
            insert_message(&mut transaction, receipt_id, &receipt, &content, now).await?;
        let domains: Vec<String> = sqlx::query_scalar(
            "SELECT name FROM mail_domains WHERE workspace_id = $1 \
             AND status IN ('verified', 'suspended')",
        )
        .bind(workspace_id.to_string())
        .fetch_all(&mut *transaction)
        .await?;
        let sender = content
            .parsed
            .from
            .as_ref()
            .map(|from| clean(&from.address).into_owned());
        let thread_ids = content.thread_ids();
        let subject = clean(&content.parsed.subject);
        let normalized_subject = clean(&content.parsed.normalized_subject);
        let snippet = clean(&content.parsed.snippet);
        let incoming = Incoming {
            message_id,
            raw_sha256: &receipt.raw_sha256,
            thread_ids: &thread_ids,
            subject: &subject,
            normalized_subject: &normalized_subject,
            sender: sender.as_deref(),
            sender_inside: sender
                .as_deref()
                .is_some_and(|address| is_inside(address, &domains)),
            received_at: receipt.received_at,
            snippet: &snippet,
            has_attachments: content.has_attachments(),
            dmarc_failed,
        };

        let mut deliveries = Vec::new();
        let mut events = Vec::new();
        for mailbox_id in recipient_mailboxes(&mut transaction, receipt_id, workspace_id).await? {
            let Some(filed) = threads::file(
                &mut transaction,
                workspace_id,
                mailbox_id,
                &incoming,
                &domains,
                now,
            )
            .await?
            else {
                continue;
            };
            if !filed.users.is_empty() {
                events.push(NewEvent {
                    recipients: Recipients::Users(filed.users),
                    topic: MAIL_TOPIC,
                    event: json!({
                        "type": "thread.updated",
                        "mailbox_id": mailbox_id.to_string(),
                        "thread_id": filed.thread_id.to_string(),
                    }),
                });
            }
            deliveries.push(Delivery {
                mailbox_id,
                thread_id: filed.thread_id,
                new_thread: filed.new_thread,
            });
        }
        sqlx::query(
            "UPDATE mail_receipts SET status = 'ingested', attempts = attempts + 1, error = NULL, \
             updated_at = $1 WHERE id = $2",
        )
        .bind(now.as_millis())
        .bind(receipt_id.to_string())
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;

        if !events.is_empty()
            && let Err(error) = LiveHub::of(&self.database)
                .publish(workspace_id, &events)
                .await
        {
            // The mail is stored; the clients see it on their next refresh.
            tracing::warn!(error = %error, "mail events for the live socket");
        }
        Ok(IngestOutcome::Ingested {
            message_id,
            deliveries,
        })
    }

    /// Records a failed ingest attempt; the last one (`final_attempt`) marks the receipt
    /// `failed`, shown with **Retry**. The raw message stays.
    pub async fn record_ingest_failure(
        &self,
        receipt_id: Id,
        error: &str,
        final_attempt: bool,
        now: TimestampMillis,
    ) -> Result<(), MailError> {
        let error: String = clean(error).chars().take(ERROR_CHARS).collect();
        sqlx::query(
            "UPDATE mail_receipts SET attempts = attempts + 1, error = $1, \
             status = CASE WHEN $2 = 1 THEN 'failed' ELSE status END, updated_at = $3 \
             WHERE id = $4 AND status <> 'ingested'",
        )
        .bind(error)
        .bind(i64::from(final_attempt))
        .bind(now.as_millis())
        .bind(receipt_id.to_string())
        .execute(self.database.pool())
        .await?;
        Ok(())
    }

    /// **Retry** of a failed receipt (owners and admins): back to `pending` with a new ingest job.
    pub async fn retry_receipt(
        &self,
        workspace_id: Id,
        actor_id: Id,
        receipt_id: Id,
        jobs: &JobStore,
    ) -> Result<(), MailError> {
        let now = self.database.database_now().await?;
        let mut transaction = self.database.immediate_transaction().await?;
        manager(&mut transaction, workspace_id, actor_id).await?;
        let status: Option<String> = sqlx::query_scalar(
            "SELECT status FROM mail_receipts WHERE id = $1 AND workspace_id = $2",
        )
        .bind(receipt_id.to_string())
        .bind(workspace_id.to_string())
        .fetch_optional(&mut *transaction)
        .await?;
        match status.as_deref() {
            None => return Err(MailError::NotFound),
            Some("failed") => {}
            Some(_) => return Err(MailError::Conflict("receipt_not_failed")),
        }
        sqlx::query(
            "UPDATE mail_receipts SET status = 'pending', attempts = 0, error = NULL, \
             updated_at = $1 WHERE id = $2",
        )
        .bind(now.as_millis())
        .bind(receipt_id.to_string())
        .execute(&mut *transaction)
        .await?;
        let job = Job::new(
            mail_ingest_kind(),
            json!({ "receipt_id": receipt_id.to_string() }),
            now,
        )
        .with_workspace(workspace_id.to_string());
        jobs.enqueue_in(&mut transaction, &job)
            .await
            .map_err(|error| match error {
                orbit_platform::JobStoreError::Database(error) => MailError::Unavailable(error),
                other => MailError::Unavailable(sqlx::Error::Protocol(other.to_string())),
            })?;
        transaction.commit().await?;
        Ok(())
    }

    /// `mail.purge` (spec §6.4): deletes threads trashed more than [`TRASH_RETENTION_MS`] ago,
    /// then the messages no thread holds, then the ingested receipts without a message. The blob
    /// reconcile removes their blobs.
    pub async fn purge(&self, now: TimestampMillis) -> Result<PurgeResult, MailError> {
        let mut transaction = self.database.immediate_transaction().await?;
        let threads = sqlx::query(
            "DELETE FROM mail_threads WHERE trashed_at IS NOT NULL AND trashed_at <= $1",
        )
        .bind(now.as_millis() - TRASH_RETENTION_MS)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        let messages = sqlx::query(
            "DELETE FROM mail_messages WHERE NOT EXISTS \
             (SELECT 1 FROM mail_thread_messages tm WHERE tm.message_id = mail_messages.id)",
        )
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        let receipts = sqlx::query(
            "DELETE FROM mail_receipts WHERE status = 'ingested' AND NOT EXISTS \
             (SELECT 1 FROM mail_messages m WHERE m.receipt_id = mail_receipts.id)",
        )
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        transaction.commit().await?;
        Ok(PurgeResult {
            threads,
            messages,
            receipts,
        })
    }
}

/// The `mail.ingest` handler: `attempt` of `max_attempts` (from the job's claim). Database and
/// blob store errors are retried; the last attempt, and any other error at once, marks the
/// receipt `failed`.
pub async fn run_ingest_job(
    repo: &MailRepository,
    payload: &Value,
    attempt: u32,
    max_attempts: u32,
) -> Result<(), JobError> {
    let receipt_id = payload
        .get("receipt_id")
        .and_then(Value::as_str)
        .and_then(|id| id.parse::<Id>().ok())
        .ok_or_else(|| JobError::Permanent("mail.ingest needs a receipt_id".to_owned()))?;
    let error = match repo.ingest(receipt_id, TimestampMillis::now()).await {
        Ok(_) => return Ok(()),
        Err(error) => error,
    };
    let retryable = matches!(error, MailError::Unavailable(_) | MailError::Upload(_));
    let text = match &error {
        MailError::Upload(error) => format!("the raw message blob could not be read: {error}"),
        other => other.to_string(),
    };
    let final_attempt = !retryable || attempt >= max_attempts;
    if let Err(record) = repo
        .record_ingest_failure(receipt_id, &text, final_attempt, TimestampMillis::now())
        .await
    {
        tracing::warn!(error = %record, receipt_id = %receipt_id, "recording a mail ingest failure");
    }
    tracing::warn!(error = %text, receipt_id = %receipt_id, attempt, "mail ingest failed");
    if retryable {
        Err(JobError::Retryable(text))
    } else {
        Err(JobError::Permanent(text))
    }
}

async fn load_receipt(pool: &sqlx::AnyPool, receipt_id: Id) -> Result<Option<Receipt>, MailError> {
    let Some(row) = sqlx::query(
        "SELECT r.workspace_id, b.storage_key, r.key_version, r.raw_sha256, r.size, r.auth, \
         r.status, r.received_at FROM mail_receipts r \
         JOIN attachment_blobs b ON b.id = r.blob_id WHERE r.id = $1",
    )
    .bind(receipt_id.to_string())
    .fetch_optional(pool)
    .await?
    else {
        return Ok(None);
    };
    let workspace_id: String = row.try_get("workspace_id")?;
    Ok(Some(Receipt {
        workspace_id: parse_id(&workspace_id)?,
        storage_key: row.try_get("storage_key")?,
        key_version: row.try_get("key_version")?,
        raw_sha256: row.try_get("raw_sha256")?,
        size: row.try_get("size")?,
        auth: row.try_get("auth")?,
        status: row.try_get("status")?,
        received_at: TimestampMillis::from_millis(row.try_get("received_at")?),
    }))
}

/// Inserts the receipt's message (once: a re-run reads the existing one) and, the first time,
/// its attachments. Returns the message id.
async fn insert_message(
    transaction: &mut Transaction<'_, Any>,
    receipt_id: Id,
    receipt: &Receipt,
    content: &Content,
    now: TimestampMillis,
) -> Result<Id, MailError> {
    let parsed = &content.parsed;
    let message_id = Id::new_v7();
    let from = parsed.from.as_ref();
    let inserted = sqlx::query(
        "INSERT INTO mail_messages (id, workspace_id, receipt_id, raw_sha256, message_id_header, \
         in_reply_to, reference_ids, from_name, from_addr, sender, reply_to, to_addrs, cc_addrs, \
         subject, sent_at, received_at, snippet, body_text, has_attachments, size, html_part, warning, \
         created_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, \
         $17, $18, $19, $20, $21, $22, $23) ON CONFLICT (receipt_id) DO NOTHING",
    )
    .bind(message_id.to_string())
    .bind(receipt.workspace_id.to_string())
    .bind(receipt_id.to_string())
    .bind(&receipt.raw_sha256)
    .bind(clean_opt(parsed.message_id.as_deref()))
    .bind(clean_opt(parsed.in_reply_to.first().map(String::as_str)))
    .bind(
        Value::Array(
            parsed
                .references
                .iter()
                .map(|id| Value::String(clean(id).into_owned()))
                .collect(),
        )
        .to_string(),
    )
    .bind(clean_opt(from.and_then(|from| from.name.as_deref())))
    .bind(clean_opt(from.map(|from| from.address.as_str())))
    .bind(clean_opt(
        parsed.sender.as_ref().map(|sender| sender.address.as_str()),
    ))
    .bind(people_json(&parsed.reply_to))
    .bind(people_json(&parsed.to))
    .bind(people_json(&parsed.cc))
    .bind(clean(&parsed.subject).into_owned())
    .bind(parsed.sent_at.and_then(|seconds| seconds.checked_mul(1000)))
    .bind(receipt.received_at.as_millis())
    .bind(clean(&parsed.snippet).into_owned())
    .bind(clean(&parsed.body_text).into_owned())
    .bind(i64::from(content.has_attachments()))
    .bind(receipt.size)
    .bind(parsed.html_part.map(i64::from))
    .bind(content.warning)
    .bind(now.as_millis())
    .execute(&mut **transaction)
    .await?
    .rows_affected();
    if inserted == 0 {
        let existing: String =
            sqlx::query_scalar("SELECT id FROM mail_messages WHERE receipt_id = $1")
                .bind(receipt_id.to_string())
                .fetch_one(&mut **transaction)
                .await?;
        return parse_id(&existing);
    }
    for attachment in &parsed.attachments {
        insert_attachment(transaction, message_id, attachment).await?;
    }
    Ok(message_id)
}

async fn insert_attachment(
    transaction: &mut Transaction<'_, Any>,
    message_id: Id,
    attachment: &AttachmentInfo,
) -> Result<(), MailError> {
    sqlx::query(
        "INSERT INTO mail_attachments (id, message_id, filename, declared_type, sniffed_type, size, \
         part_id, content_id, disposition) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) \
         ON CONFLICT (message_id, part_id) DO NOTHING",
    )
    .bind(Id::new_v7().to_string())
    .bind(message_id.to_string())
    .bind(clean_opt(attachment.filename.as_deref()))
    .bind(clean(&attachment.declared_type).into_owned())
    .bind(
        attachment
            .sniffed_type
            .as_deref()
            .unwrap_or("application/octet-stream"),
    )
    .bind(i64::try_from(attachment.size).unwrap_or(i64::MAX))
    .bind(i64::from(attachment.part_id))
    .bind(clean_opt(attachment.content_id.as_deref()))
    .bind(if attachment.inline {
        "inline"
    } else {
        "attachment"
    })
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

/// The mailboxes of the receipt's recipients that still exist in its workspace, each once (two
/// addresses of one mailbox in `RCPT TO` deliver once). A removed address is skipped.
async fn recipient_mailboxes(
    transaction: &mut Transaction<'_, Any>,
    receipt_id: Id,
    workspace_id: Id,
) -> Result<Vec<Id>, MailError> {
    let ids: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT a.mailbox_id FROM mail_receipt_recipients r \
         JOIN mail_addresses a ON a.id = r.address_id \
         JOIN mailboxes mb ON mb.id = a.mailbox_id \
         WHERE r.receipt_id = $1 AND mb.workspace_id = $2 ORDER BY a.mailbox_id",
    )
    .bind(receipt_id.to_string())
    .bind(workspace_id.to_string())
    .fetch_all(&mut **transaction)
    .await?;
    ids.iter().map(|id| parse_id(id)).collect()
}
