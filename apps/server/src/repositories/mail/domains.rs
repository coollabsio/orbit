//! Mail domains (spec §6.1): the claim, the first DKIM key, catch-all, disable and enable, the
//! results of the DNS checks and the status they lead to.
//!
//! A name belongs to one workspace on the instance. A claim is refused when the name, a parent
//! or a subdomain of it belongs to another workspace; the same workspace can add subdomains.
//! A re-added domain is a new row: a new id, a new token and a new DKIM key, so nothing of an
//! earlier claim (addresses, catch-all, results) carries over.

use orbit_domain::{Actor, Permission};
use orbit_platform::{Id, Job, JobStore, Row, TimestampMillis};
use rand_core::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::any::AnyRow;
use sqlx::{Any, Transaction};
use utoipa::ToSchema;

use super::{MailError, MailRepository};
use crate::audit::{self, AuditOutcome};
use crate::repositories::membership;
use crate::secret_box;

/// Pending domains are checked this often for [`PENDING_WINDOW_MS`] after they were added.
pub const PENDING_CHECK_MS: i64 = 10 * 60 * 1000;
/// How long a new domain is checked every [`PENDING_CHECK_MS`]; then once a day, still pending.
pub const PENDING_WINDOW_MS: i64 = 72 * 60 * 60 * 1000;
/// Verified, suspended and older pending domains are checked once a day.
pub const DAILY_CHECK_MS: i64 = 24 * 60 * 60 * 1000;
/// A suspended domain receives mail for this long after `suspended_at`, then refuses it.
pub const SUSPENDED_GRACE_MS: i64 = 7 * 24 * 60 * 60 * 1000;
/// The schedule runs every 10 minutes; a check that ran a little less than an interval ago is
/// due, so a domain does not skip a run.
const SCHEDULE_SLACK_MS: i64 = 60 * 1000;
/// Longest record text kept in a report.
const FOUND_MAX_CHARS: usize = 512;
/// The job kind of the DNS checks.
pub const DNS_CHECK_JOB: &str = "mail.dns_check";
/// Audit requests of the DNS check job.
const DNS_CHECK_REQUEST: &str = "mail.dns_check";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
#[schema(as = MailDomainStatus)]
pub enum DomainStatus {
    /// Added; the TXT record with the token was not seen yet.
    Pending,
    /// The TXT record was seen; the domain receives mail.
    Verified,
    /// The TXT record of a verified domain is gone. Mail is accepted until the grace end.
    Suspended,
    /// Turned off by an admin: mail is refused.
    Disabled,
}

impl DomainStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Verified => "verified",
            Self::Suspended => "suspended",
            Self::Disabled => "disabled",
        }
    }

    fn parse(value: &str) -> Result<Self, MailError> {
        match value {
            "pending" => Ok(Self::Pending),
            "verified" => Ok(Self::Verified),
            "suspended" => Ok(Self::Suspended),
            "disabled" => Ok(Self::Disabled),
            _ => Err(MailError::Invalid { field: "status" }),
        }
    }
}

/// The result of one DNS record check.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
#[schema(as = MailRecordStatus)]
pub enum RecordStatus {
    /// The record is published as Orbit expects.
    Ok,
    /// No such record.
    Missing,
    /// A record is published with another value (`found`, shortened).
    Wrong { found: String },
    /// The lookup timed out or failed; nothing was concluded.
    Unknown,
}

impl RecordStatus {
    /// A [`Self::Wrong`] with `found` shortened to a size a report can keep.
    #[must_use]
    pub fn wrong(found: &str) -> Self {
        Self::Wrong {
            found: found.chars().take(FOUND_MAX_CHARS).collect(),
        }
    }
}

/// The last DNS check of a domain: one result for each record.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[schema(as = MailDnsReport)]
pub struct DnsReport {
    /// TXT `_orbit-verify.<domain>` with the token. Only this one decides the status.
    pub verify: RecordStatus,
    /// MX of the domain.
    pub mx: RecordStatus,
    /// TXT `<selector>._domainkey.<domain>` with the public key.
    pub dkim: RecordStatus,
    /// One SPF record (`v=spf1`) on the domain.
    pub spf: RecordStatus,
    /// TXT `_dmarc.<domain>`.
    pub dmarc: RecordStatus,
    /// The `p=` of the DMARC record: `none`, `quarantine` or `reject`.
    #[serde(default)]
    pub dmarc_policy: Option<String>,
}

impl DnsReport {
    /// Nothing known yet.
    #[must_use]
    pub const fn unknown() -> Self {
        Self {
            verify: RecordStatus::Unknown,
            mx: RecordStatus::Unknown,
            dkim: RecordStatus::Unknown,
            spf: RecordStatus::Unknown,
            dmarc: RecordStatus::Unknown,
            dmarc_policy: None,
        }
    }
}

/// The DKIM key the setup page shows (the newest pending or active key).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DkimPublic {
    pub selector: String,
    /// Base64 DER (SPKI), the `p=` of the record.
    pub public_key: String,
    pub status: String,
}

#[derive(Clone, Debug)]
pub struct Domain {
    pub id: Id,
    pub workspace_id: Id,
    /// Normalized: lower case, IDNA A-labels, no trailing dot.
    pub name: String,
    pub status: DomainStatus,
    pub verify_token: String,
    pub verified_at: Option<TimestampMillis>,
    pub last_verified_at: Option<TimestampMillis>,
    pub suspended_at: Option<TimestampMillis>,
    pub catch_all_mailbox_id: Option<Id>,
    pub report: Option<DnsReport>,
    pub checked_at: Option<TimestampMillis>,
    pub created_at: TimestampMillis,
    pub updated_at: TimestampMillis,
    pub dkim: Option<DkimPublic>,
    /// Addresses on the domain; the domain can only be removed without any.
    pub address_count: i64,
}

impl Domain {
    /// When a suspended domain stops receiving mail.
    #[must_use]
    pub fn grace_ends_at(&self) -> Option<TimestampMillis> {
        if self.status != DomainStatus::Suspended {
            return None;
        }
        self.suspended_at
            .map(|at| TimestampMillis::from_millis(at.as_millis() + SUSPENDED_GRACE_MS))
    }
}

const DOMAIN_COLUMNS: &str = "d.id, d.workspace_id, d.name, d.status, d.verify_token, d.verified_at, \
     d.last_verified_at, d.suspended_at, d.catch_all_mailbox_id, d.dns_report, d.checked_at, \
     d.created_at, d.updated_at, \
     (SELECT COUNT(*) FROM mail_addresses a WHERE a.domain_id = d.id) AS address_count, \
     k.selector AS dkim_selector, k.public_key AS dkim_public_key, k.status AS dkim_status";

const DOMAIN_FROM: &str = "mail_domains d LEFT JOIN mail_dkim_keys k ON k.id = (\
     SELECT k2.id FROM mail_dkim_keys k2 WHERE k2.domain_id = d.id \
     AND k2.status IN ('pending', 'active') ORDER BY k2.created_at DESC, k2.id DESC LIMIT 1)";

pub(super) fn parse_id(value: &str) -> Result<Id, MailError> {
    value.parse().map_err(|_| MailError::NotFound)
}

fn millis(row: &AnyRow, column: &str) -> Result<Option<TimestampMillis>, MailError> {
    Ok(row
        .try_get::<Option<i64>, _>(column)?
        .map(TimestampMillis::from_millis))
}

fn domain(row: &AnyRow) -> Result<Domain, MailError> {
    let report: Option<String> = row.try_get("dns_report")?;
    let selector: Option<String> = row.try_get("dkim_selector")?;
    let dkim = match selector {
        Some(selector) => Some(DkimPublic {
            selector,
            public_key: row.try_get("dkim_public_key")?,
            status: row.try_get("dkim_status")?,
        }),
        None => None,
    };
    let catch_all: Option<String> = row.try_get("catch_all_mailbox_id")?;
    Ok(Domain {
        id: parse_id(&row.try_get::<String, _>("id")?)?,
        workspace_id: parse_id(&row.try_get::<String, _>("workspace_id")?)?,
        name: row.try_get("name")?,
        status: DomainStatus::parse(&row.try_get::<String, _>("status")?)?,
        verify_token: row.try_get("verify_token")?,
        verified_at: millis(row, "verified_at")?,
        last_verified_at: millis(row, "last_verified_at")?,
        suspended_at: millis(row, "suspended_at")?,
        catch_all_mailbox_id: catch_all.as_deref().map(parse_id).transpose()?,
        // A report this server cannot read is shown as not checked.
        report: report.and_then(|text| serde_json::from_str(&text).ok()),
        checked_at: millis(row, "checked_at")?,
        created_at: TimestampMillis::from_millis(row.try_get("created_at")?),
        updated_at: TimestampMillis::from_millis(row.try_get("updated_at")?),
        dkim,
        address_count: row.try_get("address_count")?,
    })
}

/// A normalized name with at least two labels.
fn clean_domain(name: &str) -> Result<String, MailError> {
    let name = orbit_mail::normalize_domain(name.trim())
        .map_err(|_| MailError::Invalid { field: "name" })?;
    if name.contains('.') {
        Ok(name)
    } else {
        Err(MailError::Invalid { field: "name" })
    }
}

/// `b.example.com`, `example.com` and `com` for `a.b.example.com`.
fn parents(name: &str) -> Vec<&str> {
    name.match_indices('.')
        .map(|(index, _)| &name[index + 1..])
        .collect()
}

/// 32 random bytes, RFC 4648 base32 in lower case without padding (52 characters).
fn new_token() -> String {
    const ALPHABET: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";
    let mut bytes = [0_u8; 32];
    rand_core::OsRng.fill_bytes(&mut bytes);
    let mut token = String::with_capacity(52);
    let mut buffer: u32 = 0;
    let mut bits = 0;
    for byte in bytes {
        buffer = (buffer << 8) | u32::from(byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            token.push(char::from(ALPHABET[((buffer >> bits) & 31) as usize]));
        }
    }
    if bits > 0 {
        token.push(char::from(ALPHABET[((buffer << (5 - bits)) & 31) as usize]));
    }
    token
}

/// The UTC year of `at`, for the DKIM selector.
fn utc_year(at: TimestampMillis) -> i32 {
    // Howard Hinnant's civil-from-days.
    let days = at.as_millis().div_euclid(86_400_000);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let year = yoe + era * 400 + i64::from(mp >= 10);
    i32::try_from(year).unwrap_or(i32::MAX)
}

/// The associated data of a sealed DKIM private key.
#[must_use]
pub fn dkim_aad(domain_id: Id, selector: &str) -> String {
    format!("dkim:{domain_id}:{selector}")
}

/// The job that checks one domain now.
#[must_use]
pub fn dns_check_job(domain_id: Id, workspace_id: Id, now: TimestampMillis) -> Job {
    Job::new(
        dns_check_kind(),
        json!({ "domain_id": domain_id.to_string() }),
        now,
    )
    .with_workspace(workspace_id.to_string())
}

/// `mail.dns_check`: one check at a time on the cluster (the scheduled run and **Check now**).
#[must_use]
pub fn dns_check_kind() -> orbit_platform::JobKind {
    orbit_platform::JobKind::new(DNS_CHECK_JOB).with_concurrency_limit(1)
}

/// The caller as an owner or admin of the workspace: `Forbidden("mail_forbidden")` for other
/// members, `NotFound` for outsiders.
pub(super) async fn manager(
    transaction: &mut Transaction<'_, Any>,
    workspace_id: Id,
    actor_id: Id,
) -> Result<Actor, MailError> {
    let actor = membership::actor(&mut **transaction, workspace_id, actor_id)
        .await?
        .ok_or(MailError::NotFound)?;
    if actor.can(Permission::MailManage) {
        Ok(actor)
    } else {
        Err(MailError::Forbidden("mail_forbidden"))
    }
}

async fn load<'e, E>(executor: E, workspace_id: Id, domain_id: Id) -> Result<Domain, MailError>
where
    E: sqlx::Executor<'e, Database = Any>,
{
    let row = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT {DOMAIN_COLUMNS} FROM {DOMAIN_FROM} WHERE d.id = $1 AND d.workspace_id = $2"
    )))
    .bind(domain_id.to_string())
    .bind(workspace_id.to_string())
    .fetch_optional(executor)
    .await?
    .ok_or(MailError::NotFound)?;
    domain(&row)
}

/// Refuses `name` when it, a parent or a subdomain belongs to a workspace other than
/// `workspace_id`, or when `workspace_id` has it already. `except` leaves one row out (a move).
async fn check_claim(
    transaction: &mut Transaction<'_, Any>,
    workspace_id: Id,
    name: &str,
    except: Option<Id>,
) -> Result<(), MailError> {
    let mut candidates = vec![name];
    candidates.extend(parents(name));
    let mut sql = orbit_platform::SqlBuilder::new(
        "SELECT workspace_id, name FROM mail_domains WHERE (name LIKE ",
    );
    // Normalized names hold only `a-z 0-9 - .`, so `%` and `_` cannot occur in them.
    sql.push_bind(format!("%.{name}"));
    for candidate in candidates {
        sql.push(" OR name = ");
        sql.push_bind(candidate.to_owned());
    }
    sql.push(")");
    if let Some(except) = except {
        sql.push(" AND id <> ");
        sql.push_bind(except.to_string());
    }
    let rows = sql.build().fetch_all(&mut **transaction).await?;
    for row in &rows {
        let owner: String = row.try_get("workspace_id")?;
        let found: String = row.try_get("name")?;
        if owner != workspace_id.to_string() {
            return Err(MailError::Conflict("domain_claimed"));
        }
        if found == name {
            return Err(MailError::Conflict("domain_exists"));
        }
    }
    Ok(())
}

/// The caller manages mail, the instance allows mail domains and `name` is free to claim.
/// Returns the normalized name.
async fn claim_checks(
    transaction: &mut Transaction<'_, Any>,
    workspace_id: Id,
    actor_id: Id,
    name: &str,
) -> Result<String, MailError> {
    manager(transaction, workspace_id, actor_id).await?;
    let enabled: i64 =
        sqlx::query_scalar("SELECT mail_domains_enabled FROM instance_settings WHERE id = 1")
            .fetch_optional(&mut **transaction)
            .await?
            .unwrap_or(0);
    if enabled != 1 {
        return Err(MailError::Forbidden("mail_domains_disabled"));
    }
    let name = clean_domain(name)?;
    check_claim(transaction, workspace_id, &name, None).await?;
    Ok(name)
}

#[allow(clippy::too_many_arguments)]
async fn record_audit(
    transaction: &mut Transaction<'_, Any>,
    workspace_id: Id,
    actor_id: Option<Id>,
    action: &str,
    domain_id: Id,
    request_id: &str,
    metadata: serde_json::Value,
    now: TimestampMillis,
) -> Result<(), MailError> {
    audit::record(
        transaction,
        workspace_id,
        actor_id,
        action,
        AuditOutcome::Success,
        "mail_domain",
        Some(domain_id),
        request_id,
        metadata,
        now,
    )
    .await?;
    Ok(())
}

impl MailRepository {
    /// Claims `name` for the workspace with a new token and a first DKIM key (`pending`).
    pub async fn create_domain(
        &self,
        workspace_id: Id,
        actor_id: Id,
        name: &str,
        request_id: &str,
    ) -> Result<Domain, MailError> {
        // RSA key generation takes a while: it runs off the runtime and outside the writer
        // transaction, and only for a claim that passes the checks (they run again below).
        let mut transaction = self.database.pool().begin().await?;
        let name = claim_checks(&mut transaction, workspace_id, actor_id, name).await?;
        transaction.rollback().await?;
        let key = tokio::task::spawn_blocking(orbit_mail::generate_dkim_key)
            .await
            .map_err(|_| MailError::Crypto)?;
        let now = self.database.database_now().await?;
        let mut transaction = self.database.immediate_transaction().await?;
        claim_checks(&mut transaction, workspace_id, actor_id, &name).await?;

        let domain_id = Id::new_v7();
        sqlx::query(
            "INSERT INTO mail_domains (id, workspace_id, name, status, verify_token, created_at, \
             updated_at) VALUES ($1, $2, $3, 'pending', $4, $5, $5)",
        )
        .bind(domain_id.to_string())
        .bind(workspace_id.to_string())
        .bind(&name)
        .bind(new_token())
        .bind(now.as_millis())
        .execute(&mut *transaction)
        .await?;

        let selector = orbit_mail::new_selector(utc_year(now), 0);
        let sealed = secret_box::seal(
            &self.app_key,
            dkim_aad(domain_id, &selector).as_bytes(),
            &key.private_pkcs8_der,
        )
        .map_err(|()| MailError::Crypto)?;
        let public_key = {
            use base64::Engine;
            base64::engine::general_purpose::STANDARD.encode(&key.public_der)
        };
        sqlx::query(
            "INSERT INTO mail_dkim_keys (id, domain_id, selector, public_key, private_sealed, \
             status, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, 'pending', $6, $6)",
        )
        .bind(Id::new_v7().to_string())
        .bind(domain_id.to_string())
        .bind(&selector)
        .bind(public_key)
        .bind(sealed)
        .bind(now.as_millis())
        .execute(&mut *transaction)
        .await?;

        record_audit(
            &mut transaction,
            workspace_id,
            Some(actor_id),
            "mail.domain_created",
            domain_id,
            request_id,
            json!({ "name": name }),
            now,
        )
        .await?;
        let created = load(&mut *transaction, workspace_id, domain_id).await?;
        transaction.commit().await?;
        Ok(created)
    }

    /// The domains of the workspace by name. Owners and admins only.
    pub async fn list_domains(
        &self,
        workspace_id: Id,
        actor_id: Id,
    ) -> Result<Vec<Domain>, MailError> {
        let mut transaction = self.database.pool().begin().await?;
        manager(&mut transaction, workspace_id, actor_id).await?;
        let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT {DOMAIN_COLUMNS} FROM {DOMAIN_FROM} WHERE d.workspace_id = $1 ORDER BY d.name"
        )))
        .bind(workspace_id.to_string())
        .fetch_all(&mut *transaction)
        .await?;
        transaction.commit().await?;
        rows.iter().map(domain).collect()
    }

    pub async fn get_domain(
        &self,
        workspace_id: Id,
        actor_id: Id,
        domain_id: Id,
    ) -> Result<Domain, MailError> {
        let mut transaction = self.database.pool().begin().await?;
        manager(&mut transaction, workspace_id, actor_id).await?;
        let found = load(&mut *transaction, workspace_id, domain_id).await?;
        transaction.commit().await?;
        Ok(found)
    }

    /// Removes a domain without addresses (`Conflict("domain_has_mailboxes")` otherwise). Its
    /// DKIM keys go with it; mail it received stays in the workspace's mailboxes.
    pub async fn delete_domain(
        &self,
        workspace_id: Id,
        actor_id: Id,
        domain_id: Id,
        request_id: &str,
    ) -> Result<(), MailError> {
        let now = self.database.database_now().await?;
        let mut transaction = self.database.immediate_transaction().await?;
        manager(&mut transaction, workspace_id, actor_id).await?;
        let found = load(&mut *transaction, workspace_id, domain_id).await?;
        if found.address_count > 0 {
            return Err(MailError::Conflict("domain_has_mailboxes"));
        }
        sqlx::query("DELETE FROM mail_domains WHERE id = $1")
            .bind(domain_id.to_string())
            .execute(&mut *transaction)
            .await?;
        record_audit(
            &mut transaction,
            workspace_id,
            Some(actor_id),
            "mail.domain_deleted",
            domain_id,
            request_id,
            json!({ "name": found.name }),
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    /// Sets or clears the mailbox that receives mail for unknown addresses of the domain.
    pub async fn set_catch_all(
        &self,
        workspace_id: Id,
        actor_id: Id,
        domain_id: Id,
        mailbox_id: Option<Id>,
        request_id: &str,
    ) -> Result<Domain, MailError> {
        let now = self.database.database_now().await?;
        let mut transaction = self.database.immediate_transaction().await?;
        manager(&mut transaction, workspace_id, actor_id).await?;
        load(&mut *transaction, workspace_id, domain_id).await?;
        if let Some(mailbox_id) = mailbox_id {
            let found: Option<String> =
                sqlx::query_scalar("SELECT id FROM mailboxes WHERE id = $1 AND workspace_id = $2")
                    .bind(mailbox_id.to_string())
                    .bind(workspace_id.to_string())
                    .fetch_optional(&mut *transaction)
                    .await?;
            if found.is_none() {
                return Err(MailError::Invalid {
                    field: "mailbox_id",
                });
            }
        }
        sqlx::query(
            "UPDATE mail_domains SET catch_all_mailbox_id = $1, updated_at = $2 WHERE id = $3",
        )
        .bind(mailbox_id.map(|id| id.to_string()))
        .bind(now.as_millis())
        .bind(domain_id.to_string())
        .execute(&mut *transaction)
        .await?;
        record_audit(
            &mut transaction,
            workspace_id,
            Some(actor_id),
            "mail.domain_updated",
            domain_id,
            request_id,
            json!({ "catch_all_mailbox_id": mailbox_id.map(|id| id.to_string()) }),
            now,
        )
        .await?;
        let updated = load(&mut *transaction, workspace_id, domain_id).await?;
        transaction.commit().await?;
        Ok(updated)
    }

    /// Disables a domain (mail is refused), or enables it again. Enabling restores the status
    /// the dates say: `suspended` while `suspended_at` is set, else `verified` once verified,
    /// else `pending`.
    pub async fn set_domain_enabled(
        &self,
        workspace_id: Id,
        actor_id: Id,
        domain_id: Id,
        enabled: bool,
        request_id: &str,
    ) -> Result<Domain, MailError> {
        let now = self.database.database_now().await?;
        let mut transaction = self.database.immediate_transaction().await?;
        manager(&mut transaction, workspace_id, actor_id).await?;
        let found = load(&mut *transaction, workspace_id, domain_id).await?;
        let status = if !enabled {
            DomainStatus::Disabled
        } else if found.status != DomainStatus::Disabled {
            found.status
        } else if found.suspended_at.is_some() {
            DomainStatus::Suspended
        } else if found.verified_at.is_some() {
            DomainStatus::Verified
        } else {
            DomainStatus::Pending
        };
        if status != found.status {
            sqlx::query("UPDATE mail_domains SET status = $1, updated_at = $2 WHERE id = $3")
                .bind(status.as_str())
                .bind(now.as_millis())
                .bind(domain_id.to_string())
                .execute(&mut *transaction)
                .await?;
            record_audit(
                &mut transaction,
                workspace_id,
                Some(actor_id),
                "mail.domain_updated",
                domain_id,
                request_id,
                json!({ "from": found.status.as_str(), "status": status.as_str() }),
                now,
            )
            .await?;
        }
        let updated = load(&mut *transaction, workspace_id, domain_id).await?;
        transaction.commit().await?;
        Ok(updated)
    }

    /// Queues a DNS check of the domain (**Check now**). Returns `false` when one is waiting
    /// already, so repeated clicks do not pile up jobs.
    pub async fn request_dns_check(
        &self,
        workspace_id: Id,
        actor_id: Id,
        domain_id: Id,
        jobs: &JobStore,
    ) -> Result<bool, MailError> {
        let now = self.database.database_now().await?;
        let mut transaction = self.database.immediate_transaction().await?;
        manager(&mut transaction, workspace_id, actor_id).await?;
        load(&mut *transaction, workspace_id, domain_id).await?;
        let job = dns_check_job(domain_id, workspace_id, now);
        let payload = serde_json::to_string(&job.payload).map_err(|_| MailError::Crypto)?;
        let waiting: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM jobs WHERE kind = $1 AND payload_json = $2 \
             AND state IN ('queued', 'running')",
        )
        .bind(DNS_CHECK_JOB)
        .bind(payload)
        .fetch_one(&mut *transaction)
        .await?;
        if waiting > 0 {
            return Ok(false);
        }
        jobs.enqueue_in(&mut transaction, &job)
            .await
            .map_err(|error| match error {
                orbit_platform::JobStoreError::Database(error) => MailError::Unavailable(error),
                other => MailError::Unavailable(sqlx::Error::Protocol(other.to_string())),
            })?;
        transaction.commit().await?;
        Ok(true)
    }

    /// The domain for the DNS check, whatever its workspace.
    pub async fn domain_for_check(&self, domain_id: Id) -> Result<Option<Domain>, MailError> {
        let row = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT {DOMAIN_COLUMNS} FROM {DOMAIN_FROM} WHERE d.id = $1"
        )))
        .bind(domain_id.to_string())
        .fetch_optional(self.database.pool())
        .await?;
        row.as_ref().map(domain).transpose()
    }

    /// Stores a check result and applies the state machine:
    /// pending + verify ok → verified; verified + verify missing or wrong → suspended;
    /// suspended + verify ok → verified. An unknown verify result (a timeout) changes neither
    /// the status nor `checked_at`, so the next run tries again. A disabled domain keeps its
    /// status. Returns the status after the check, `None` for a domain that is gone.
    pub async fn record_dns_report(
        &self,
        domain_id: Id,
        report: DnsReport,
        now: TimestampMillis,
    ) -> Result<Option<DomainStatus>, MailError> {
        let mut transaction = self.database.immediate_transaction().await?;
        let Some(row) = sqlx::query(
            "SELECT workspace_id, status, verified_at, last_verified_at, suspended_at, checked_at \
             FROM mail_domains WHERE id = $1",
        )
        .bind(domain_id.to_string())
        .fetch_optional(&mut *transaction)
        .await?
        else {
            return Ok(None);
        };
        let workspace_id = parse_id(&row.try_get::<String, _>("workspace_id")?)?;
        let status = DomainStatus::parse(&row.try_get::<String, _>("status")?)?;
        let verified_at: Option<i64> = row.try_get("verified_at")?;
        let mut last_verified_at: Option<i64> = row.try_get("last_verified_at")?;
        let mut suspended_at: Option<i64> = row.try_get("suspended_at")?;
        let mut checked_at: Option<i64> = row.try_get("checked_at")?;
        let verify_ok = report.verify == RecordStatus::Ok;
        let verify_failed = matches!(
            report.verify,
            RecordStatus::Missing | RecordStatus::Wrong { .. }
        );
        let next = match status {
            DomainStatus::Pending | DomainStatus::Suspended if verify_ok => {
                suspended_at = None;
                DomainStatus::Verified
            }
            DomainStatus::Verified if verify_failed => {
                suspended_at = Some(now.as_millis());
                DomainStatus::Suspended
            }
            other => other,
        };
        if verify_ok {
            last_verified_at = Some(now.as_millis());
        }
        if report.verify != RecordStatus::Unknown {
            checked_at = Some(now.as_millis());
        }
        let first_verified_at = match verified_at {
            None if next == DomainStatus::Verified => Some(now.as_millis()),
            other => other,
        };
        let report_json = serde_json::to_string(&report).map_err(|_| MailError::Crypto)?;
        sqlx::query(
            "UPDATE mail_domains SET status = $1, dns_report = $2, verified_at = $3, \
             last_verified_at = $4, suspended_at = $5, checked_at = $6, updated_at = $7 \
             WHERE id = $8",
        )
        .bind(next.as_str())
        .bind(report_json)
        .bind(first_verified_at)
        .bind(last_verified_at)
        .bind(suspended_at)
        .bind(checked_at)
        .bind(now.as_millis())
        .bind(domain_id.to_string())
        .execute(&mut *transaction)
        .await?;
        if next != status {
            record_audit(
                &mut transaction,
                workspace_id,
                None,
                "mail.domain_updated",
                domain_id,
                DNS_CHECK_REQUEST,
                json!({
                    "from": status.as_str(),
                    "status": next.as_str(),
                    "first_verification": verified_at.is_none() && next == DomainStatus::Verified,
                }),
                now,
            )
            .await?;
        }
        transaction.commit().await?;
        Ok(Some(next))
    }

    /// Domains the scheduled check should look at: pending domains younger than 72 hours every
    /// 10 minutes, all other enabled domains once a day.
    pub async fn domains_due(&self, now: TimestampMillis) -> Result<Vec<Id>, MailError> {
        let now = now.as_millis();
        let ids: Vec<String> = sqlx::query_scalar(
            "SELECT id FROM mail_domains WHERE status <> 'disabled' AND (checked_at IS NULL OR \
             checked_at <= CASE WHEN status = 'pending' AND created_at > $1 THEN $2 ELSE $3 END) \
             ORDER BY checked_at, id",
        )
        .bind(now - PENDING_WINDOW_MS)
        .bind(now - PENDING_CHECK_MS + SCHEDULE_SLACK_MS)
        .bind(now - DAILY_CHECK_MS + SCHEDULE_SLACK_MS)
        .fetch_all(self.database.pool())
        .await?;
        ids.iter().map(|id| parse_id(id)).collect()
    }

    /// Moves a domain to another workspace (instance admins). Only a domain without addresses
    /// moves; its catch-all is cleared, as that mailbox stays behind.
    pub async fn move_domain(
        &self,
        name: &str,
        target_workspace_id: Id,
        actor_id: Id,
        request_id: &str,
    ) -> Result<Domain, MailError> {
        let name = clean_domain(name).map_err(|_| MailError::NotFound)?;
        let now = self.database.database_now().await?;
        let mut transaction = self.database.immediate_transaction().await?;
        let row = sqlx::query("SELECT id, workspace_id FROM mail_domains WHERE name = $1")
            .bind(&name)
            .fetch_optional(&mut *transaction)
            .await?
            .ok_or(MailError::NotFound)?;
        let domain_id = parse_id(&row.try_get::<String, _>("id")?)?;
        let source_workspace_id = parse_id(&row.try_get::<String, _>("workspace_id")?)?;
        let target: Option<String> =
            sqlx::query_scalar("SELECT id FROM workspaces WHERE id = $1 AND deleted_at IS NULL")
                .bind(target_workspace_id.to_string())
                .fetch_optional(&mut *transaction)
                .await?;
        if target.is_none() {
            return Err(MailError::Invalid {
                field: "workspace_id",
            });
        }
        let found = load(&mut *transaction, source_workspace_id, domain_id).await?;
        if source_workspace_id == target_workspace_id {
            transaction.commit().await?;
            return Ok(found);
        }
        if found.address_count > 0 {
            return Err(MailError::Conflict("domain_has_mailboxes"));
        }
        check_claim(
            &mut transaction,
            target_workspace_id,
            &name,
            Some(domain_id),
        )
        .await?;
        sqlx::query(
            "UPDATE mail_domains SET workspace_id = $1, catch_all_mailbox_id = NULL, \
             updated_at = $2 WHERE id = $3",
        )
        .bind(target_workspace_id.to_string())
        .bind(now.as_millis())
        .bind(domain_id.to_string())
        .execute(&mut *transaction)
        .await?;
        let metadata = json!({
            "name": name,
            "from_workspace_id": source_workspace_id.to_string(),
            "to_workspace_id": target_workspace_id.to_string(),
        });
        for workspace_id in [source_workspace_id, target_workspace_id] {
            record_audit(
                &mut transaction,
                workspace_id,
                Some(actor_id),
                "mail.domain_updated",
                domain_id,
                request_id,
                metadata.clone(),
                now,
            )
            .await?;
        }
        let moved = load(&mut *transaction, target_workspace_id, domain_id).await?;
        transaction.commit().await?;
        Ok(moved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_52_base32_characters() {
        let token = new_token();
        assert_eq!(token.len(), 52);
        assert!(
            token
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || (b'2'..=b'7').contains(&byte))
        );
        assert_ne!(token, new_token());
    }

    #[test]
    fn parents_of_a_name() {
        assert_eq!(
            parents("a.b.example.com"),
            ["b.example.com", "example.com", "com"]
        );
        assert!(parents("localhost").is_empty());
    }

    #[test]
    fn utc_years() {
        assert_eq!(utc_year(TimestampMillis::from_millis(0)), 1970);
        // 2026-10-23T00:00:00Z and 2024-02-29T12:00:00Z, 2025-12-31T23:59:59Z.
        assert_eq!(
            utc_year(TimestampMillis::from_millis(1_792_713_600_000)),
            2026
        );
        assert_eq!(
            utc_year(TimestampMillis::from_millis(1_709_208_000_000)),
            2024
        );
        assert_eq!(
            utc_year(TimestampMillis::from_millis(1_767_225_599_000)),
            2025
        );
    }
}
