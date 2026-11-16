//! Mail: domains, mailboxes, received messages (receipts), threads, labels and search
//! (migration 0095).
//!
//! A received message is stored as an ordinary blob, sealed with the workspace data key
//! ([`MailKeys`]); `mail_receipts.blob_id` references it, so the blob reconcile removes a blob
//! that no receipt uses. No HTML is stored: it is sanitized from the raw message on each view.

mod contacts;
mod content;
mod domains;
mod drafts;
mod events;
mod health;
mod ingest;
mod keys;
mod labels;
mod links;
mod limits;
mod mailboxes;
mod outbound;
mod providers;
mod reading;
mod receipts;
mod release;
mod routes;
mod search;
mod signatures;
mod suppressions;
mod threads;
mod vacation;

use orbit_platform::{Database, Id, TimestampMillis, UploadError, UploadService};
use sqlx::{Any, Transaction};
use thiserror::Error;

pub use contacts::{CONTACT_LIMIT, ContactHit, ContactKind};
pub use content::{MessageContent, MessagePart, PartContent};
pub use domains::{
    DAILY_CHECK_MS, DNS_CHECK_JOB, DkimKeyView, DkimPublic, DnsReport, Domain, DomainProvider,
    DomainSendRoute, DomainStatus, NotReadyReason, PENDING_CHECK_MS, PENDING_WINDOW_MS,
    RETIRING_KEY_MS, RecordStatus, SUSPENDED_GRACE_MS, SendReadiness, dkim_aad, dns_check_job,
    dns_check_kind,
};
pub use drafts::{Draft, DraftChange, DraftFile, DraftMode, NewDraft};
pub use events::{EventOutcome, PROVIDER_DEFER_GIVE_UP_MS};
pub use health::{HEALTH_TEST_TTL_MS, HealthTest, HealthTestResult, HealthTestStatus, TestDomain};
pub use ingest::{
    Delivery, IngestOutcome, MAIL_TOPIC, PURGE_JOB, PurgeResult, TRASH_RETENTION_MS,
    mail_purge_kind, run_ingest_job,
};
pub use keys::{MailKeys, open_blob, receipt_aad, seal_blob};
pub use labels::Label;
pub use links::{FileLink, MailFileLinkView};
pub use limits::{Limits, SEND_POLICY_JOB, SendPolicyView, SendTier, send_policy_kind};
pub(crate) use mailboxes::suspend_owned_mailboxes;
pub use mailboxes::{
    AddressKind, BulkResult, MailAddress, Mailbox, MailboxKind, MailboxStatus, NamePattern,
    NewMailbox, Proposal, RecipientLookup, local_part_for, lookup_recipient,
};
pub use outbound::{
    AttemptResult, BounceKind, Claim, DELAYED_AFTER_MS, DeferCause, GAVE_UP,
    LOCAL_DOMAIN_UNAVAILABLE, LOCAL_MAILBOX_SUSPENDED, LOCAL_TOO_LARGE, Outbound, OutboundDelivery,
    OutboundItem, OutboundView, RETRY_STEPS_MS, ROUTE_TOO_LARGE, RecipientRow, RecipientStatus,
    Recorded, SEND_JOB, SEND_TIMEOUT_MS, SENDING_LEASE_MS, ThreadAudience, UNREADABLE,
    mail_send_kind,
};
pub use providers::{
    CreatedProvider, MailProvider, NewProvider, NewRejection, ProviderChange, ProviderKind,
    ProviderScope, ProviderSecrets, ProviderSettings, REJECTED_RETENTION_MS, RejectReason,
    provider_aad,
};
pub use reading::{
    MailAttachmentView, MailAuthResult, MailAuthSummary, MailCounts, MailFolder, MailMessageView,
    MailOverview, MailPerson, MailThreadDetail, MailThreadPage, MailThreadSummary, MailboxUnread,
    THREAD_PAGE_DEFAULT, THREAD_PAGE_MAX, ThreadChange, ThreadListInput,
};
pub use receipts::{
    FailedReceipt, FailedReceipts, INGEST_JOB, ReceiptProblem, RejectedDelivery, STUCK_RECEIPT_MS,
    mail_ingest_kind,
};
pub(crate) use receipts::{
    NewReceipt, insert_receipt, insert_receipt_recipient, record_health_test,
};
pub use release::{Released, SEND_SWEEP_JOB, SEND_SWEEP_MS, mail_send_sweep_kind};
pub use routes::{
    HostClass, NewRoute, ProviderRouteInput, RouteChange, RouteSecrets, RouteSettings, RouteStep,
    RouteStepStatus, SendRoute, SendRouteKind, SmtpSecurity, all_private, all_public, classify_ip,
    resolve_host, route_aad,
};
pub use search::{MailQuery, ThreadCursor, parse_query};
pub use signatures::{FromAddress, Snippet, UNDO_SEND_SECONDS};
pub use suppressions::{
    BOUNCE_TOKEN_TTL_MS, SUPPRESSION_PAGE, Suppression, SuppressionPage, SuppressionReason,
    bounce_codes, is_bounce_local, suppresses, suppresses_response,
};
pub use threads::SEARCH_BODY_BYTES;
pub use vacation::{REPLY_INTERVAL_MS, Vacation, VacationChange};

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
    /// Valid input that Orbit refuses, with a stable code (a 422).
    #[error("{0}")]
    Unprocessable(&'static str),
    /// The message is over the send route's size limit (a 422 `message_too_large`).
    #[error("the message is over the size limit")]
    MessageTooLarge { limit: i64, size: i64 },
    /// The domain or mailbox cannot send yet (a 409 `send_not_ready`).
    #[error("the domain cannot send yet")]
    NotReady(NotReadyReason),
    #[error("the workspace is over its storage quota")]
    QuotaExceeded,
    /// Too many downloads of a file link (a 429).
    #[error("too many requests")]
    RateLimited,
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
    /// Orbit Cloud (`ORBIT_CLOUD=true`): a workspace starts on the `new` tier.
    cloud: bool,
    /// Where file links point (`{origin}/mail/f/{token}`): the content origin, else the public
    /// origin. Empty: a message with a link file is held (`link_origin_missing`).
    link_origin: String,
}

impl MailRepository {
    #[must_use]
    pub fn new(database: Database, uploads: UploadService, app_key: [u8; 32]) -> Self {
        Self {
            keys: MailKeys::new(database.clone(), app_key),
            database,
            uploads,
            app_key,
            cloud: false,
            link_origin: String::new(),
        }
    }

    /// Sets the origin of file links.
    #[must_use]
    pub fn with_link_origin(mut self, origin: impl Into<String>) -> Self {
        self.link_origin = origin.into().trim_end_matches('/').to_owned();
        self
    }

    /// Sets whether this is Orbit Cloud: a workspace without a send policy is then `new`, else
    /// `standard` (spec §5.8).
    #[must_use]
    pub const fn with_cloud(mut self, cloud: bool) -> Self {
        self.cloud = cloud;
        self
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

    /// The app key, for keys derived from it (mail view URLs).
    pub(crate) fn app_key(&self) -> &[u8; 32] {
        &self.app_key
    }

    /// The key of the bounce tokens in envelope senders (`bounce+{token}@domain`, spec §5.7):
    /// HMAC-SHA256 of the label `orbit-bounce-token-v1` with the app key. Sending makes tokens
    /// with it and the bounce handler reads them with it.
    #[must_use]
    pub fn bounce_key(&self) -> [u8; 32] {
        use hmac::{Hmac, Mac};
        let mut mac = Hmac::<sha2::Sha256>::new_from_slice(&self.app_key)
            .expect("HMAC takes a key of any length");
        mac.update(b"orbit-bounce-token-v1");
        mac.finalize().into_bytes().into()
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
