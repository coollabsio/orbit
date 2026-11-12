//! Test addresses of the Admin health card (spec §8.1): `check-<random>@<verified domain>`,
//! valid for one hour. The intake records what a test message showed (peer, TLS, SPF/DKIM/DMARC)
//! and delivers it to no mailbox. Instance admins only; the route checks it.

use orbit_mail::{AuthOutcome, TlsInfo};
use orbit_platform::{Id, Row, TimestampMillis};
use rand_core::RngCore;
use serde::Deserialize;

use super::domains::parse_id;
use super::{MailError, MailRepository};

/// How long a test address takes mail (spec: 1 hour).
pub const HEALTH_TEST_TTL_MS: i64 = 60 * 60 * 1000;
/// Old test addresses are deleted when a new one is made this long after they expired.
const HEALTH_TEST_KEPT_MS: i64 = 24 * 60 * 60 * 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HealthTestStatus {
    Waiting,
    Received,
    Expired,
}

/// What the test message showed: the JSON the intake writes.
#[derive(Clone, Debug, Deserialize)]
pub struct HealthTestResult {
    pub peer_ip: String,
    pub helo: String,
    pub mail_from: Option<String>,
    pub size: u64,
    pub tls: Option<TlsInfo>,
    pub auth: AuthOutcome,
}

#[derive(Clone, Debug)]
pub struct HealthTest {
    pub id: Id,
    pub domain_id: Id,
    /// `check-…@domain`.
    pub address: String,
    pub expires_at: TimestampMillis,
    pub received_at: Option<TimestampMillis>,
    pub status: HealthTestStatus,
    /// `None` until a message arrived (or when the stored result does not decode).
    pub result: Option<HealthTestResult>,
}

/// A verified domain a test address can use.
#[derive(Clone, Debug)]
pub struct TestDomain {
    pub id: Id,
    pub name: String,
    pub workspace_id: Id,
}

/// `check-` and 8 random base32 characters (lower case).
fn test_local_part() -> String {
    const ALPHABET: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";
    let mut bytes = [0_u8; 8];
    rand_core::OsRng.fill_bytes(&mut bytes);
    let random: String = bytes
        .iter()
        .map(|byte| char::from(ALPHABET[usize::from(byte & 31)]))
        .collect();
    format!("check-{random}")
}

impl MailRepository {
    /// The verified domains of all workspaces, by name.
    pub async fn test_domains(&self) -> Result<Vec<TestDomain>, MailError> {
        let rows = sqlx::query(
            "SELECT id, name, workspace_id FROM mail_domains WHERE status = 'verified' \
             ORDER BY name",
        )
        .fetch_all(self.database.pool())
        .await?;
        rows.iter()
            .map(|row| {
                Ok(TestDomain {
                    id: parse_id(&row.try_get::<String, _>("id")?)?,
                    name: row.try_get("name")?,
                    workspace_id: parse_id(&row.try_get::<String, _>("workspace_id")?)?,
                })
            })
            .collect()
    }

    /// A new test address on verified domain `domain_id` (any workspace's), valid for
    /// [`HEALTH_TEST_TTL_MS`]. `domain_not_verified` for a pending, suspended or disabled domain.
    pub async fn create_health_test(&self, domain_id: Id) -> Result<HealthTest, MailError> {
        let now = self.database.database_now().await?;
        let mut transaction = self.database.immediate_transaction().await?;
        let status: Option<String> =
            sqlx::query_scalar("SELECT status FROM mail_domains WHERE id = $1")
                .bind(domain_id.to_string())
                .fetch_optional(&mut *transaction)
                .await?;
        match status.as_deref() {
            None => return Err(MailError::NotFound),
            Some("verified") => {}
            Some(_) => return Err(MailError::Conflict("domain_not_verified")),
        }
        sqlx::query("DELETE FROM mail_health_tests WHERE expires_at <= $1")
            .bind(now.as_millis() - HEALTH_TEST_KEPT_MS)
            .execute(&mut *transaction)
            .await?;
        let id = Id::new_v7();
        sqlx::query(
            "INSERT INTO mail_health_tests (id, domain_id, address_local, expires_at, created_at) \
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(id.to_string())
        .bind(domain_id.to_string())
        .bind(test_local_part())
        .bind(now.as_millis() + HEALTH_TEST_TTL_MS)
        .bind(now.as_millis())
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        self.health_test(id).await
    }

    pub async fn health_test(&self, id: Id) -> Result<HealthTest, MailError> {
        let now = self.database.database_now().await?;
        let row = sqlx::query(
            "SELECT t.id, t.domain_id, t.address_local, d.name, t.expires_at, t.received_at, \
             t.result FROM mail_health_tests t JOIN mail_domains d ON d.id = t.domain_id \
             WHERE t.id = $1",
        )
        .bind(id.to_string())
        .fetch_optional(self.database.pool())
        .await?
        .ok_or(MailError::NotFound)?;
        let local: String = row.try_get("address_local")?;
        let domain: String = row.try_get("name")?;
        let expires_at = TimestampMillis::from_millis(row.try_get("expires_at")?);
        let received_at = row
            .try_get::<Option<i64>, _>("received_at")?
            .map(TimestampMillis::from_millis);
        let result = row
            .try_get::<Option<String>, _>("result")?
            .and_then(|result| serde_json::from_str(&result).ok());
        let status = if received_at.is_some() {
            HealthTestStatus::Received
        } else if expires_at <= now {
            HealthTestStatus::Expired
        } else {
            HealthTestStatus::Waiting
        };
        Ok(HealthTest {
            id: parse_id(&row.try_get::<String, _>("id")?)?,
            domain_id: parse_id(&row.try_get::<String, _>("domain_id")?)?,
            address: format!("{local}@{domain}"),
            expires_at,
            received_at,
            status,
            result,
        })
    }
}
