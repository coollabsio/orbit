//! Stored first responses for the public API `Idempotency-Key` header, per API token.
use orbit_platform::{Id, Row, TimestampMillis};

use super::api_tokens::{ApiTokenError, ApiTokenRepository};

/// How long a key keeps its response.
const KEEP_MILLIS: i64 = 24 * 60 * 60 * 1000;
/// How long a claim without a response blocks a retry; a server that died while it ran leaves one.
const RUNNING_MILLIS: i64 = 5 * 60 * 1000;

#[derive(Debug, Eq, PartialEq)]
pub enum IdempotencyClaim {
    /// The key is new: run the request, then `finish` or `release` the key.
    Claimed,
    /// An earlier request with this key and body finished: answer with its response.
    Replay { status: u16, response: String },
    /// An earlier request used this key with another body or path.
    Mismatch,
    /// An earlier request with this key still runs.
    Running,
}

impl ApiTokenRepository {
    pub async fn claim_idempotency_key(
        &self,
        token_id: Id,
        key: &str,
        request_hash: &str,
        now: TimestampMillis,
    ) -> Result<IdempotencyClaim, ApiTokenError> {
        let now = now.as_millis();
        let mut transaction = self.database.immediate_transaction().await?;
        sqlx::query(
            "DELETE FROM api_idempotency_keys WHERE created_at < $1 \
             OR (response IS NULL AND created_at < $2 AND token_id = $3 AND idempotency_key = $4)",
        )
        .bind(now - KEEP_MILLIS)
        .bind(now - RUNNING_MILLIS)
        .bind(token_id.to_string())
        .bind(key)
        .execute(&mut *transaction)
        .await?;
        let inserted = sqlx::query(
            "INSERT INTO api_idempotency_keys (token_id, idempotency_key, request_hash, created_at) \
             VALUES ($1, $2, $3, $4) ON CONFLICT DO NOTHING",
        )
        .bind(token_id.to_string())
        .bind(key)
        .bind(request_hash)
        .bind(now)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        let claim = if inserted == 1 {
            IdempotencyClaim::Claimed
        } else {
            let row = sqlx::query(
                "SELECT request_hash, status, response FROM api_idempotency_keys \
                 WHERE token_id = $1 AND idempotency_key = $2",
            )
            .bind(token_id.to_string())
            .bind(key)
            .fetch_one(&mut *transaction)
            .await?;
            let stored: Option<String> = row.get("response");
            let status: Option<i64> = row.get("status");
            if row.get::<String, _>("request_hash") != request_hash {
                IdempotencyClaim::Mismatch
            } else if let (Some(response), Some(status)) = (stored, status) {
                IdempotencyClaim::Replay {
                    status: u16::try_from(status).unwrap_or(200),
                    response,
                }
            } else {
                IdempotencyClaim::Running
            }
        };
        transaction.commit().await?;
        Ok(claim)
    }

    /// Keeps the response of the request that holds the claim.
    pub async fn finish_idempotency_key(
        &self,
        token_id: Id,
        key: &str,
        status: u16,
        response: &str,
    ) -> Result<(), ApiTokenError> {
        sqlx::query(
            "UPDATE api_idempotency_keys SET status = $1, response = $2 \
             WHERE token_id = $3 AND idempotency_key = $4",
        )
        .bind(i64::from(status))
        .bind(response)
        .bind(token_id.to_string())
        .bind(key)
        .execute(self.database.pool())
        .await?;
        Ok(())
    }

    /// Drops the claim of a request that made nothing, so a retry runs again.
    pub async fn release_idempotency_key(
        &self,
        token_id: Id,
        key: &str,
    ) -> Result<(), ApiTokenError> {
        sqlx::query(
            "DELETE FROM api_idempotency_keys \
             WHERE token_id = $1 AND idempotency_key = $2 AND response IS NULL",
        )
        .bind(token_id.to_string())
        .bind(key)
        .execute(self.database.pool())
        .await?;
        Ok(())
    }
}
