//! Workspace data keys (`mail_keys`). Each workspace has a random 32-byte key, sealed with the
//! app key and bound to the workspace (associated data `mail-key:<workspace_id>`), so a sealed key
//! copied to another workspace does not open. Raw messages are sealed with the data key and bound
//! to their receipt (`mail-raw:<receipt_id>`).

use orbit_platform::{Database, Id, Row, TimestampMillis};
use rand_core::RngCore;
use sqlx::{Any, Transaction};

use super::MailError;
use crate::secret_box;

pub struct MailKeys {
    database: Database,
    app_key: [u8; 32],
}

impl MailKeys {
    #[must_use]
    pub fn new(database: Database, app_key: [u8; 32]) -> Self {
        Self { database, app_key }
    }

    /// The newest data key of the workspace and its version; the first call makes version 1.
    /// `transaction` is an `immediate_transaction()`, so two servers cannot both create one.
    pub async fn current(
        &self,
        transaction: &mut Transaction<'_, Any>,
        workspace_id: Id,
    ) -> Result<(i64, [u8; 32]), MailError> {
        if let Some(found) = self.newest(transaction, workspace_id).await? {
            return Ok(found);
        }
        let mut key = [0_u8; 32];
        rand_core::OsRng.fill_bytes(&mut key);
        let sealed = secret_box::seal(&self.app_key, key_aad(workspace_id).as_bytes(), &key)
            .map_err(|()| MailError::Crypto)?;
        let inserted = sqlx::query(
            "INSERT INTO mail_keys (workspace_id, version, key_sealed, created_at) \
             VALUES ($1, 1, $2, $3) ON CONFLICT (workspace_id, version) DO NOTHING",
        )
        .bind(workspace_id.to_string())
        .bind(sealed)
        .bind(TimestampMillis::now().as_millis())
        .execute(&mut **transaction)
        .await?
        .rows_affected();
        if inserted == 1 {
            return Ok((1, key));
        }
        self.newest(transaction, workspace_id)
            .await?
            .ok_or(MailError::NotFound)
    }

    /// Data key `version` of the workspace, to open an older receipt.
    pub async fn get(&self, workspace_id: Id, version: i64) -> Result<[u8; 32], MailError> {
        let sealed: Vec<u8> = sqlx::query_scalar(
            "SELECT key_sealed FROM mail_keys WHERE workspace_id = $1 AND version = $2",
        )
        .bind(workspace_id.to_string())
        .bind(version)
        .fetch_optional(self.database.pool())
        .await?
        .ok_or(MailError::NotFound)?;
        self.open_key(workspace_id, &sealed)
    }

    async fn newest(
        &self,
        transaction: &mut Transaction<'_, Any>,
        workspace_id: Id,
    ) -> Result<Option<(i64, [u8; 32])>, MailError> {
        let Some(row) = sqlx::query(
            "SELECT version, key_sealed FROM mail_keys WHERE workspace_id = $1 \
             ORDER BY version DESC LIMIT 1",
        )
        .bind(workspace_id.to_string())
        .fetch_optional(&mut **transaction)
        .await?
        else {
            return Ok(None);
        };
        let version: i64 = row.try_get("version")?;
        let sealed: Vec<u8> = row.try_get("key_sealed")?;
        Ok(Some((version, self.open_key(workspace_id, &sealed)?)))
    }

    fn open_key(&self, workspace_id: Id, sealed: &[u8]) -> Result<[u8; 32], MailError> {
        let key = secret_box::open(&self.app_key, key_aad(workspace_id).as_bytes(), sealed)
            .map_err(|()| MailError::Crypto)?;
        <[u8; 32]>::try_from(key.as_slice()).map_err(|_| MailError::Crypto)
    }
}

fn key_aad(workspace_id: Id) -> String {
    format!("mail-key:{workspace_id}")
}

/// The associated data of a receipt's raw message.
#[must_use]
pub fn receipt_aad(receipt_id: Id) -> String {
    format!("mail-raw:{receipt_id}")
}

/// Seals a raw message with a workspace data key (ChaCha20-Poly1305, fresh nonce), bound to `aad`.
pub fn seal_blob(data_key: &[u8; 32], aad: &str, raw: &[u8]) -> Result<Vec<u8>, MailError> {
    secret_box::seal(data_key, aad.as_bytes(), raw).map_err(|()| MailError::Crypto)
}

/// Opens a value from [`seal_blob`]; fails for another key, other `aad` or altered bytes.
pub fn open_blob(data_key: &[u8; 32], aad: &str, sealed: &[u8]) -> Result<Vec<u8>, MailError> {
    secret_box::open(data_key, aad.as_bytes(), sealed).map_err(|()| MailError::Crypto)
}
