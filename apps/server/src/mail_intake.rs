//! Intake (spec §4.2, §4.3): the only way a received message enters Orbit. The listener calls
//! [`MailIntake::accept`] and replies `250` only when it returns `Ok`.
//!
//! The raw message is sealed with each recipient workspace's data key (associated data
//! `mail-raw:<receipt_id>`) and installed as a blob (the file is synced, or S3 confirmed the PUT);
//! the blob row, the receipt with its recipients and the `mail.ingest` job are written in one
//! `immediate_transaction()` for all workspaces. Any failure rolls back every workspace, so the
//! reply is all or nothing; a file installed for a rolled-back receipt has no blob row, and the
//! blob reconcile removes it. A health test recipient gets no receipt: its result is recorded in
//! the same transaction and the bytes are dropped.

use std::net::IpAddr;
use std::sync::Arc;

use orbit_mail::{AuthOutcome, TlsInfo};
use orbit_platform::{Id, Job, JobStore, JobStoreError, UploadError};
use serde_json::json;
use sha2::{Digest, Sha256};
use thiserror::Error;

pub use crate::repositories::mail::{INGEST_JOB, ProviderKind, mail_ingest_kind};
use crate::repositories::mail::{
    MailError, MailRepository, NewReceipt, insert_receipt, insert_receipt_recipient, receipt_aad,
    record_health_test, seal_blob,
};

pub type ReceiptId = Id;

/// How a message arrived: Orbit's SMTP listener, or a provider's webhook (provider intake).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Smtp,
    Provider(ProviderKind),
}

impl Source {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Smtp => "smtp",
            Self::Provider(kind) => kind.as_str(),
        }
    }
}

/// A recipient the listener accepted at `RCPT TO` (from `RecipientLookup`). `rcpt` is the
/// address as sent, with its `+tag`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Recipient {
    Mailbox {
        workspace_id: Id,
        address_id: Id,
        rcpt: String,
    },
    HealthTest {
        test_id: Id,
        rcpt: String,
    },
}

/// The adapter contract (spec §4.2).
#[derive(Debug, Clone)]
pub struct InboundMessage {
    pub source: Source,
    /// The message as received (dot-unstuffed, CRLF line ends), with Orbit's own trace headers.
    pub raw: Vec<u8>,
    /// The length of the trace headers Orbit put in front of `raw`. The dedupe hash skips them, so
    /// a sender or provider that sends the same message again (a lost reply) gets one copy.
    pub trace_len: usize,
    /// `None` for the null sender `<>`.
    pub mail_from: Option<String>,
    pub recipients: Vec<Recipient>,
    pub peer_ip: IpAddr,
    pub helo: String,
    pub tls: Option<TlsInfo>,
    pub auth: AuthOutcome,
}

/// Why nothing was stored; the listener replies `451 4.3.0`, `452 4.2.2` and `552 5.3.4`.
#[derive(Debug, Error)]
pub enum IntakeError {
    /// The blob store, the database or the commit failed; the sender retries.
    #[error("the message could not be stored: {0}")]
    Temporary(String),
    #[error("a recipient workspace is over its storage quota")]
    QuotaExceeded,
    #[error("the message is over the size limit")]
    TooLarge,
}

impl From<MailError> for IntakeError {
    fn from(error: MailError) -> Self {
        match error {
            MailError::QuotaExceeded => Self::QuotaExceeded,
            other => Self::Temporary(other.to_string()),
        }
    }
}

impl From<UploadError> for IntakeError {
    fn from(error: UploadError) -> Self {
        MailError::from(error).into()
    }
}

impl From<sqlx::Error> for IntakeError {
    fn from(error: sqlx::Error) -> Self {
        Self::Temporary(error.to_string())
    }
}

impl From<JobStoreError> for IntakeError {
    fn from(error: JobStoreError) -> Self {
        Self::Temporary(error.to_string())
    }
}

impl From<serde_json::Error> for IntakeError {
    fn from(error: serde_json::Error) -> Self {
        Self::Temporary(error.to_string())
    }
}

#[derive(Clone)]
pub struct MailIntake {
    repo: Arc<MailRepository>,
    max_message_bytes: u64,
}

impl MailIntake {
    /// Stores blobs through the repository's `UploadService`; `max_message_bytes` is
    /// `MailConfig::max_message_bytes`.
    #[must_use]
    pub fn new(repo: Arc<MailRepository>, max_message_bytes: u64) -> Self {
        Self {
            repo,
            max_message_bytes,
        }
    }

    /// Stores the message durably: one receipt for each recipient workspace (in the order the
    /// workspaces first appear), each with its own sealed blob and `mail.ingest` job. Returns the
    /// receipt ids (empty when only health tests were addressed).
    pub async fn accept(&self, message: InboundMessage) -> Result<Vec<ReceiptId>, IntakeError> {
        if u64::try_from(message.raw.len()).map_or(true, |size| size > self.max_message_bytes) {
            return Err(IntakeError::TooLarge);
        }
        let (workspaces, tests) = group(&message.recipients);
        if workspaces.is_empty() && tests.is_empty() {
            return Ok(Vec::new());
        }
        let size = i64::try_from(message.raw.len()).map_err(|_| IntakeError::TooLarge)?;
        let received = message.raw.get(message.trace_len..).unwrap_or(&message.raw);
        let raw_sha256 = format!("{:x}", Sha256::digest(received));
        let tls = message
            .tls
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        let auth = serde_json::to_string(&message.auth)?;
        let peer_ip = message.peer_ip.to_string();

        let database = self.repo.database();
        let finalization = self.repo.uploads().begin_finalization().await;
        let now = database.database_now().await?;
        let mut transaction = database.immediate_transaction().await?;
        let mut receipts = Vec::with_capacity(workspaces.len());
        for (workspace_id, recipients) in workspaces {
            let receipt_id = Id::new_v7();
            let (key_version, key) = self
                .repo
                .keys()
                .current(&mut transaction, workspace_id)
                .await?;
            let sealed = seal_blob(&key, &receipt_aad(receipt_id), &message.raw)?;
            let blob = finalization
                .finalize_bytes_in_transaction(&mut transaction, workspace_id, &sealed, now)
                .await?;
            insert_receipt(
                &mut transaction,
                &NewReceipt {
                    id: receipt_id,
                    workspace_id,
                    source: message.source.as_str(),
                    blob_id: blob.id,
                    key_version,
                    raw_sha256: &raw_sha256,
                    size,
                    mail_from: message.mail_from.as_deref(),
                    peer_ip: peer_ip.clone(),
                    helo: &message.helo,
                    tls: tls.clone(),
                    auth: auth.clone(),
                },
                now,
            )
            .await?;
            for (address_id, rcpt) in recipients {
                insert_receipt_recipient(&mut transaction, receipt_id, address_id, rcpt).await?;
            }
            let job = Job::new(
                mail_ingest_kind(),
                json!({ "receipt_id": receipt_id.to_string() }),
                now,
            )
            .with_workspace(workspace_id.to_string());
            JobStore::enqueue_in(&mut transaction, &job).await?;
            receipts.push(receipt_id);
        }
        if !tests.is_empty() {
            let result = json!({
                "peer_ip": peer_ip,
                "helo": message.helo,
                "mail_from": message.mail_from,
                "size": size,
                "tls": message.tls,
                "auth": message.auth,
            })
            .to_string();
            for test_id in tests {
                record_health_test(&mut transaction, test_id, &result, now).await?;
            }
        }
        transaction.commit().await?;
        drop(finalization);
        Ok(receipts)
    }
}

/// Mailbox recipients by workspace (first-appearance order) and the health test ids.
#[allow(clippy::type_complexity)]
fn group(recipients: &[Recipient]) -> (Vec<(Id, Vec<(Id, &str)>)>, Vec<Id>) {
    let mut workspaces: Vec<(Id, Vec<(Id, &str)>)> = Vec::new();
    let mut tests = Vec::new();
    for recipient in recipients {
        match recipient {
            Recipient::Mailbox {
                workspace_id,
                address_id,
                rcpt,
            } => {
                let entry = (*address_id, rcpt.as_str());
                match workspaces.iter_mut().find(|(id, _)| id == workspace_id) {
                    Some((_, list)) => list.push(entry),
                    None => workspaces.push((*workspace_id, vec![entry])),
                }
            }
            Recipient::HealthTest { test_id, .. } => {
                if !tests.contains(test_id) {
                    tests.push(*test_id);
                }
            }
        }
    }
    (workspaces, tests)
}
