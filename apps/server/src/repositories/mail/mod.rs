//! Mail: domains, mailboxes, received messages (receipts), threads, labels and search
//! (migration 0056).
//!
//! A received message is stored as an ordinary blob, sealed with the workspace data key
//! ([`MailKeys`]); `mail_receipts.blob_id` references it, so the blob reconcile removes a blob
//! that no receipt uses. No HTML is stored: it is sanitized from the raw message on each view.

mod keys;

use orbit_platform::{Database, UploadError, UploadService};
use thiserror::Error;

pub use keys::{MailKeys, open_blob, receipt_aad, seal_blob};

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
}

impl MailRepository {
    #[must_use]
    pub fn new(database: Database, uploads: UploadService, app_key: [u8; 32]) -> Self {
        Self {
            keys: MailKeys::new(database.clone(), app_key),
            database,
            uploads,
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
