//! The SMTP listener's certificate from Admin → Mail (`mail_certificate`, one row): the ACME account, the active
//! certificate, the pending order and the last result. The private keys are sealed with the app key.

use instant_acme::AccountCredentials;
use orbit_platform::{Row, TimestampMillis};
use sqlx::any::AnyRow;

use super::{MailError, MailRepository};
use crate::audit::{self, AuditOutcome};
use crate::secret_box;

const ACCOUNT_AAD: &[u8] = b"mail-acme-account";
const KEY_AAD: &[u8] = b"mail-cert";

/// How the CA checked that Orbit controls the host name.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChallengeMethod {
    /// HTTP-01: the CA fetches `http://<host>/.well-known/acme-challenge/<token>`.
    Http,
    /// DNS-01: the admin adds a TXT record at `_acme-challenge.<host>`.
    Dns,
}

impl ChallengeMethod {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Dns => "dns",
        }
    }

    fn parse(value: &str) -> Result<Self, MailError> {
        match value {
            "http" => Ok(Self::Http),
            "dns" => Ok(Self::Dns),
            _ => Err(MailError::Invalid { field: "method" }),
        }
    }
}

/// An ACME order that waits for its challenge.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PendingOrder {
    Http {
        url: String,
        hostname: String,
        token: String,
        key_authorization: String,
        started_at: TimestampMillis,
    },
    Dns {
        url: String,
        hostname: String,
        /// `_acme-challenge.<host>`.
        name: String,
        /// The TXT value.
        value: String,
        started_at: TimestampMillis,
    },
}

impl PendingOrder {
    #[must_use]
    pub fn url(&self) -> &str {
        match self {
            Self::Http { url, .. } | Self::Dns { url, .. } => url,
        }
    }

    #[must_use]
    pub fn hostname(&self) -> &str {
        match self {
            Self::Http { hostname, .. } | Self::Dns { hostname, .. } => hostname,
        }
    }

    #[must_use]
    pub const fn method(&self) -> ChallengeMethod {
        match self {
            Self::Http { .. } => ChallengeMethod::Http,
            Self::Dns { .. } => ChallengeMethod::Dns,
        }
    }

    #[must_use]
    pub const fn started_at(&self) -> TimestampMillis {
        match self {
            Self::Http { started_at, .. } | Self::Dns { started_at, .. } => *started_at,
        }
    }
}

/// The active certificate, with its key opened.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IssuedCert {
    /// The chain, leaf first.
    pub cert_pem: String,
    pub key_pem: String,
    pub hostname: String,
    /// Unix seconds.
    pub not_before: i64,
    /// Unix seconds.
    pub not_after: i64,
    pub issued_at: TimestampMillis,
    pub method: ChallengeMethod,
    /// Bumped for each new certificate; servers reload when it changes.
    pub version: i64,
}

/// A certificate the CA just issued, to store.
pub struct NewCert<'a> {
    pub cert_pem: &'a str,
    pub key_pem: &'a str,
    pub hostname: &'a str,
    pub not_before: i64,
    pub not_after: i64,
    pub method: ChallengeMethod,
}

/// The ACME account and the directory it belongs to.
pub struct AcmeAccount {
    pub directory: String,
    pub credentials: AccountCredentials,
}

/// The whole row.
pub struct CertificateState {
    pub account: Option<AcmeAccount>,
    pub issued: Option<IssuedCert>,
    pub pending: Option<PendingOrder>,
    pub last_error: Option<String>,
    pub last_attempt_at: Option<TimestampMillis>,
}

impl MailRepository {
    /// The row, with the keys opened.
    pub async fn certificate_state(&self) -> Result<CertificateState, MailError> {
        let row = sqlx::query(
            "SELECT account_directory, account_sealed, cert_pem, key_sealed, cert_hostname, not_before, \
             not_after, issued_at, method, version, order_method, order_url, order_hostname, http_token, \
             http_key_authorization, dns_name, dns_value, order_started_at, last_error, last_attempt_at \
             FROM mail_certificate WHERE id = 1",
        )
        .fetch_one(self.database.pool())
        .await?;
        Ok(CertificateState {
            account: self.account_from_row(&row)?,
            issued: self.issued_from_row(&row)?,
            pending: pending_from_row(&row)?,
            last_error: row.try_get("last_error")?,
            last_attempt_at: row
                .try_get::<Option<i64>, _>("last_attempt_at")?
                .map(TimestampMillis::from_millis),
        })
    }

    /// The version of the active certificate (0: none yet). Cheap: each server polls it.
    pub async fn certificate_version(&self) -> Result<i64, MailError> {
        Ok(
            sqlx::query_scalar::<_, i64>("SELECT version FROM mail_certificate WHERE id = 1")
                .fetch_one(self.database.pool())
                .await?,
        )
    }

    /// The key authorization of the pending HTTP-01 order with `token`.
    pub async fn http_challenge(&self, token: &str) -> Result<Option<String>, MailError> {
        Ok(sqlx::query_scalar::<_, String>(
            "SELECT http_key_authorization FROM mail_certificate \
             WHERE id = 1 AND order_method = 'http' AND http_token = $1",
        )
        .bind(token)
        .fetch_optional(self.database.pool())
        .await?)
    }

    pub async fn save_acme_account(
        &self,
        directory: &str,
        credentials: &AccountCredentials,
    ) -> Result<(), MailError> {
        let json = serde_json::to_vec(credentials).map_err(|_| MailError::Crypto)?;
        let sealed =
            secret_box::seal(&self.app_key, ACCOUNT_AAD, &json).map_err(|()| MailError::Crypto)?;
        let mut transaction = self.database.immediate_transaction().await?;
        sqlx::query(
            "UPDATE mail_certificate SET account_directory = $1, account_sealed = $2 WHERE id = 1",
        )
        .bind(directory)
        .bind(sealed)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    /// Replaces the pending order (an older one is dropped: its token stops answering).
    pub async fn set_pending_order(&self, order: &PendingOrder) -> Result<(), MailError> {
        let (token, key_authorization, name, value) = match order {
            PendingOrder::Http {
                token,
                key_authorization,
                ..
            } => (
                Some(token.as_str()),
                Some(key_authorization.as_str()),
                None,
                None,
            ),
            PendingOrder::Dns { name, value, .. } => {
                (None, None, Some(name.as_str()), Some(value.as_str()))
            }
        };
        let started_at = order.started_at().as_millis();
        let mut transaction = self.database.immediate_transaction().await?;
        sqlx::query(
            "UPDATE mail_certificate SET order_method = $1, order_url = $2, order_hostname = $3, \
             http_token = $4, http_key_authorization = $5, dns_name = $6, dns_value = $7, \
             order_started_at = $8, last_error = NULL, last_attempt_at = $8 WHERE id = 1",
        )
        .bind(order.method().as_str())
        .bind(order.url())
        .bind(order.hostname())
        .bind(token)
        .bind(key_authorization)
        .bind(name)
        .bind(value)
        .bind(started_at)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    /// Drops the pending order; with `error`, records why it failed.
    pub async fn end_pending_order(
        &self,
        error: Option<&str>,
        now: TimestampMillis,
    ) -> Result<(), MailError> {
        let mut transaction = self.database.immediate_transaction().await?;
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE mail_certificate SET {CLEAR_ORDER}, last_error = $1, \
             last_attempt_at = COALESCE($2, last_attempt_at) WHERE id = 1"
        )))
        .bind(error)
        .bind(error.map(|_| now.as_millis()))
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    /// Stores a new certificate, ends the pending order and bumps the version; returns the new version.
    pub async fn store_certificate(
        &self,
        cert: &NewCert<'_>,
        actor_id: Option<orbit_platform::Id>,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<i64, MailError> {
        let key_sealed = secret_box::seal(&self.app_key, KEY_AAD, cert.key_pem.as_bytes())
            .map_err(|()| MailError::Crypto)?;
        let mut transaction = self.database.immediate_transaction().await?;
        let version = sqlx::query_scalar::<_, i64>(sqlx::AssertSqlSafe(format!(
            "UPDATE mail_certificate SET cert_pem = $1, key_sealed = $2, cert_hostname = $3, \
             not_before = $4, not_after = $5, issued_at = $6, method = $7, version = version + 1, \
             {CLEAR_ORDER}, last_error = NULL, last_attempt_at = $6 WHERE id = 1 RETURNING version"
        )))
        .bind(cert.cert_pem)
        .bind(key_sealed)
        .bind(cert.hostname)
        .bind(cert.not_before)
        .bind(cert.not_after)
        .bind(now.as_millis())
        .bind(cert.method.as_str())
        .fetch_one(&mut *transaction)
        .await?;
        audit::record_global(
            &mut transaction,
            actor_id,
            "instance.mail_certificate_issued",
            AuditOutcome::Success,
            "installation",
            None,
            request_id,
            serde_json::json!({
                "hostname": cert.hostname,
                "method": cert.method.as_str(),
                "not_after": cert.not_after,
            }),
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(version)
    }

    fn account_from_row(&self, row: &AnyRow) -> Result<Option<AcmeAccount>, MailError> {
        let directory: Option<String> = row.try_get("account_directory")?;
        let sealed: Option<Vec<u8>> = row.try_get("account_sealed")?;
        let (Some(directory), Some(sealed)) = (directory, sealed) else {
            return Ok(None);
        };
        let json = secret_box::open(&self.app_key, ACCOUNT_AAD, &sealed)
            .map_err(|()| MailError::Crypto)?;
        let credentials = serde_json::from_slice(&json).map_err(|_| MailError::Crypto)?;
        Ok(Some(AcmeAccount {
            directory,
            credentials,
        }))
    }

    fn issued_from_row(&self, row: &AnyRow) -> Result<Option<IssuedCert>, MailError> {
        let cert_pem: Option<String> = row.try_get("cert_pem")?;
        let Some(cert_pem) = cert_pem else {
            return Ok(None);
        };
        let sealed: Vec<u8> = row.try_get("key_sealed")?;
        let key =
            secret_box::open(&self.app_key, KEY_AAD, &sealed).map_err(|()| MailError::Crypto)?;
        let method: String = row.try_get("method")?;
        Ok(Some(IssuedCert {
            cert_pem,
            key_pem: String::from_utf8(key).map_err(|_| MailError::Crypto)?,
            hostname: row.try_get("cert_hostname")?,
            not_before: row.try_get("not_before")?,
            not_after: row.try_get("not_after")?,
            issued_at: TimestampMillis::from_millis(row.try_get("issued_at")?),
            method: ChallengeMethod::parse(&method)?,
            version: row.try_get("version")?,
        }))
    }
}

const CLEAR_ORDER: &str = "order_method = NULL, order_url = NULL, order_hostname = NULL, \
    http_token = NULL, http_key_authorization = NULL, dns_name = NULL, dns_value = NULL, \
    order_started_at = NULL";

fn pending_from_row(row: &AnyRow) -> Result<Option<PendingOrder>, MailError> {
    let method: Option<String> = row.try_get("order_method")?;
    let Some(method) = method else {
        return Ok(None);
    };
    let url: String = row.try_get("order_url")?;
    let hostname: String = row.try_get("order_hostname")?;
    let started_at = TimestampMillis::from_millis(row.try_get("order_started_at")?);
    Ok(Some(match ChallengeMethod::parse(&method)? {
        ChallengeMethod::Http => PendingOrder::Http {
            url,
            hostname,
            token: row.try_get("http_token")?,
            key_authorization: row.try_get("http_key_authorization")?,
            started_at,
        },
        ChallengeMethod::Dns => PendingOrder::Dns {
            url,
            hostname,
            name: row.try_get("dns_name")?,
            value: row.try_get("dns_value")?,
            started_at,
        },
    }))
}
