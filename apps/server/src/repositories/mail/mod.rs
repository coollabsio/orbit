//! Mail: domains, mailboxes, received messages (receipts), threads, labels and search
//! (migration 0056).
//!
//! A received message is stored as an ordinary blob, sealed with the workspace data key
//! ([`MailKeys`]); `mail_receipts.blob_id` references it, so the blob reconcile removes a blob
//! that no receipt uses. No HTML is stored: it is sanitized from the raw message on each view.

mod domains;
mod ingest;
mod keys;
mod labels;
mod mailboxes;
mod reading;
mod receipts;
mod search;
mod threads;

use orbit_platform::{Database, Id, TimestampMillis, UploadError, UploadService};
use sqlx::{Any, Transaction};
use thiserror::Error;

pub use domains::{
    DAILY_CHECK_MS, DNS_CHECK_JOB, DkimPublic, DnsReport, Domain, DomainStatus, PENDING_CHECK_MS,
    PENDING_WINDOW_MS, RecordStatus, SUSPENDED_GRACE_MS, dkim_aad, dns_check_job, dns_check_kind,
};
pub use ingest::{
    Delivery, IngestOutcome, MAIL_TOPIC, PURGE_JOB, PurgeResult, TRASH_RETENTION_MS,
    mail_purge_kind, run_ingest_job,
};
pub use keys::{MailKeys, open_blob, receipt_aad, seal_blob};
pub use labels::Label;
pub(crate) use mailboxes::suspend_owned_mailboxes;
pub use mailboxes::{
    AddressKind, BulkResult, MailAddress, Mailbox, MailboxKind, MailboxStatus, NamePattern,
    NewMailbox, Proposal, RecipientLookup, local_part_for, lookup_recipient,
};
pub use reading::{
    MailAttachmentView, MailAuthResult, MailAuthSummary, MailCounts, MailFolder, MailMessageView,
    MailOverview, MailPerson, MailThreadDetail, MailThreadPage, MailThreadSummary, MailboxUnread,
    THREAD_PAGE_DEFAULT, THREAD_PAGE_MAX, ThreadChange, ThreadListInput,
};
pub use receipts::{INGEST_JOB, mail_ingest_kind};
pub(crate) use receipts::{
    NewReceipt, insert_receipt, insert_receipt_recipient, record_health_test,
};
pub use search::{MailQuery, ThreadCursor, parse_query};
pub use threads::SEARCH_BODY_BYTES;

#[derive(Debug, Error)]
pub enum MailError {
    #[error("mail resource was not found")]
    NotFound,
    #[error("{0}")]
    Forbidden(&'static str),
    #[error("{0}")]
    Conflict(&'static str),
    #[error("mail input is invalid: {field}")]
    Invalid { field: &'static str },
    #[error("the workspace is over its storage quota")]
    QuotaExceeded,
    /// A sealed value did not open: another app key, other associated data or altered bytes.
    #[error("mail data could not be decrypted")]
    Crypto,
    #[error(transparent)]
    Upload(UploadError),
    #[error("mail repository is unavailable")]
    Unavailable(#[from] sqlx::Error),
}

impl From<UploadError> for MailError {
    fn from(error: UploadError) -> Self {
        match error {
            UploadError::StorageQuotaExceeded { .. } => Self::QuotaExceeded,
            other => Self::Upload(other),
        }
    }
}

pub struct MailRepository {
    database: Database,
    uploads: UploadService,
    keys: MailKeys,
    /// Seals DKIM private keys (associated data [`dkim_aad`]).
    app_key: [u8; 32],
}

impl MailRepository {
    #[must_use]
    pub fn new(database: Database, uploads: UploadService, app_key: [u8; 32]) -> Self {
        Self {
            keys: MailKeys::new(database.clone(), app_key),
            database,
            uploads,
            app_key,
        }
    }

    #[must_use]
    pub fn database(&self) -> &Database {
        &self.database
    }

    #[must_use]
    pub fn uploads(&self) -> &UploadService {
        &self.uploads
    }

    #[must_use]
    pub fn keys(&self) -> &MailKeys {
        &self.keys
    }
}

/// A successful mail change in the audit log.
#[allow(clippy::too_many_arguments)]
pub(super) async fn record_mail_audit(
    transaction: &mut Transaction<'_, Any>,
    workspace_id: Id,
    actor_id: Id,
    action: &str,
    resource_type: &str,
    resource_id: Id,
    request_id: &str,
    metadata: serde_json::Value,
    now: TimestampMillis,
) -> Result<(), MailError> {
    crate::audit::record(
        transaction,
        workspace_id,
        Some(actor_id),
        action,
        crate::audit::AuditOutcome::Success,
        resource_type,
        Some(resource_id),
        request_id,
        metadata,
        now,
    )
    .await?;
    Ok(())
}
