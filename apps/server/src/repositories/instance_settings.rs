//! Settings of the whole instance that the root user changes in Admin: open registration, the outgoing mail
//! server, object storage (S3) and automatic backups. One row (`instance_settings.id = 1`, created by migration
//! 0042). Background job limits have a row for each changed job kind (`job_limits`).

use orbit_platform::Row;
use orbit_platform::{Database, Id, JobKind, JobLimits, TimestampMillis, load_limit_overrides};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use utoipa::ToSchema;

use crate::audit::{self, AuditOutcome};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum SmtpSecurity {
    /// TLS from the first byte (usually port 465).
    Tls,
    /// Plain connection upgraded with STARTTLS (usually port 587). Refuses servers without STARTTLS.
    Starttls,
    /// No encryption. Only for a mail relay on the same host or private network.
    None,
}

impl SmtpSecurity {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Tls => "tls",
            Self::Starttls => "starttls",
            Self::None => "none",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "tls" => Some(Self::Tls),
            "starttls" => Some(Self::Starttls),
            "none" => Some(Self::None),
            _ => None,
        }
    }
}

/// The saved mail server. `password` is encrypted under the app key.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SmtpSettings {
    pub host: String,
    pub port: u16,
    pub security: SmtpSecurity,
    pub username: Option<String>,
    pub password: Option<Vec<u8>>,
    pub from_address: String,
    pub from_name: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstanceSettings {
    pub registration_open: bool,
    pub smtp: Option<SmtpSettings>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum BackupSchedule {
    #[default]
    Off,
    Hourly,
    Daily,
}

impl BackupSchedule {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Hourly => "hourly",
            Self::Daily => "daily",
        }
    }

    fn parse(value: &str) -> Self {
        match value {
            "hourly" => Self::Hourly,
            "daily" => Self::Daily,
            _ => Self::Off,
        }
    }
}

/// The saved bucket. `secret_access_key` is encrypted under the app key.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct S3Settings {
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    pub prefix: String,
    pub access_key_id: String,
    pub secret_access_key: Vec<u8>,
    pub path_style: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct StorageSettings {
    pub s3: Option<S3Settings>,
    pub attachments_in_s3: bool,
    pub backups_in_s3: bool,
    pub backup_schedule: BackupSchedule,
}

#[derive(Debug, Error)]
pub enum StorageSettingsError {
    #[error("save a bucket first")]
    NotConfigured,
    #[error("instance settings are unavailable")]
    Unavailable(#[from] sqlx::Error),
}

/// What a save does with the stored SMTP password.
pub enum PasswordChange {
    Keep,
    Clear,
    /// Already encrypted.
    Set(Vec<u8>),
}

#[derive(Debug, Error)]
pub enum InstanceSettingsError {
    #[error("registration needs a configured mail server")]
    EmailNotConfigured,
    #[error("instance settings are unavailable")]
    Unavailable(#[from] sqlx::Error),
}

/// The job kinds whose limits the root user can change in Admin: the work that customers start.
/// Maintenance kinds keep their limit of one.
#[must_use]
pub fn adjustable_job_kinds() -> [JobKind; 2] {
    [
        crate::notion::import::scan_job_kind(),
        crate::notion::import::import_job_kind(),
    ]
}

/// The limits in use for one adjustable job kind, and the code defaults.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JobKindLimits {
    pub kind: String,
    pub limits: JobLimits,
    pub defaults: JobLimits,
}

#[derive(Clone, Debug)]
pub struct InstanceSettingsRepository {
    database: Database,
}

impl InstanceSettingsRepository {
    #[must_use]
    pub fn new(database: Database) -> Self {
        Self { database }
    }

    pub async fn get(&self) -> Result<InstanceSettings, sqlx::Error> {
        let row = sqlx::query(
            "SELECT registration_open, smtp_host, smtp_port, smtp_security, smtp_username, smtp_password, \
             smtp_from_address, smtp_from_name FROM instance_settings WHERE id = 1",
        )
        .fetch_one(self.database.pool())
        .await?;
        let smtp = match (
            row.get::<Option<String>, _>("smtp_host"),
            row.get::<Option<i64>, _>("smtp_port"),
            row.get::<Option<String>, _>("smtp_security")
                .as_deref()
                .and_then(SmtpSecurity::parse),
            row.get::<Option<String>, _>("smtp_from_address"),
        ) {
            (Some(host), Some(port), Some(security), Some(from_address)) => Some(SmtpSettings {
                host,
                port: u16::try_from(port).unwrap_or(25),
                security,
                username: row.get("smtp_username"),
                password: row.get("smtp_password"),
                from_address,
                from_name: row.get("smtp_from_name"),
            }),
            _ => None,
        };
        Ok(InstanceSettings {
            registration_open: row.get::<i64, _>("registration_open") == 1,
            smtp,
        })
    }

    /// Opening registration needs a mail server: the sign-up link goes out by email.
    pub async fn set_registration_open(
        &self,
        actor_id: Id,
        open: bool,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), InstanceSettingsError> {
        let mut transaction = self.database.immediate_transaction().await?;
        if open {
            let configured = sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM instance_settings WHERE id = 1 AND smtp_host IS NOT NULL",
            )
            .fetch_one(&mut *transaction)
            .await?;
            if configured == 0 {
                return Err(InstanceSettingsError::EmailNotConfigured);
            }
        }
        sqlx::query(
            "UPDATE instance_settings SET registration_open = $1, updated_at = $2 WHERE id = 1",
        )
        .bind(i64::from(open))
        .bind(now.as_millis())
        .execute(&mut *transaction)
        .await?;
        audit::record_global(
            &mut transaction,
            Some(actor_id),
            if open {
                "instance.registration_opened"
            } else {
                "instance.registration_closed"
            },
            AuditOutcome::Success,
            "installation",
            None,
            request_id,
            serde_json::json!({}),
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn save_smtp(
        &self,
        actor_id: Id,
        smtp: SmtpSettings,
        password: PasswordChange,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), sqlx::Error> {
        let mut transaction = self.database.immediate_transaction().await?;
        let password = match password {
            PasswordChange::Keep => {
                sqlx::query_scalar::<_, Option<Vec<u8>>>(
                    "SELECT smtp_password FROM instance_settings WHERE id = 1",
                )
                .fetch_one(&mut *transaction)
                .await?
            }
            PasswordChange::Clear => None,
            PasswordChange::Set(sealed) => Some(sealed),
        };
        sqlx::query(
            "UPDATE instance_settings SET smtp_host = $1, smtp_port = $2, smtp_security = $3, smtp_username = $4, \
             smtp_password = $5, smtp_from_address = $6, smtp_from_name = $7, updated_at = $8 WHERE id = 1",
        )
        .bind(&smtp.host)
        .bind(i64::from(smtp.port))
        .bind(smtp.security.as_str())
        .bind(&smtp.username)
        .bind(password)
        .bind(&smtp.from_address)
        .bind(&smtp.from_name)
        .bind(now.as_millis())
        .execute(&mut *transaction)
        .await?;
        audit::record_global(
            &mut transaction,
            Some(actor_id),
            "instance.smtp_updated",
            AuditOutcome::Success,
            "installation",
            None,
            request_id,
            serde_json::json!({"host": smtp.host, "port": smtp.port}),
            now,
        )
        .await?;
        transaction.commit().await
    }

    /// Removes the mail server. Registration closes with it, since nobody could finish signing up.
    pub async fn clear_smtp(
        &self,
        actor_id: Id,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), sqlx::Error> {
        let mut transaction = self.database.immediate_transaction().await?;
        sqlx::query(
            "UPDATE instance_settings SET smtp_host = NULL, smtp_port = NULL, smtp_security = NULL, \
             smtp_username = NULL, smtp_password = NULL, smtp_from_address = NULL, smtp_from_name = NULL, \
             registration_open = 0, updated_at = $1 WHERE id = 1",
        )
        .bind(now.as_millis())
        .execute(&mut *transaction)
        .await?;
        audit::record_global(
            &mut transaction,
            Some(actor_id),
            "instance.smtp_removed",
            AuditOutcome::Success,
            "installation",
            None,
            request_id,
            serde_json::json!({}),
            now,
        )
        .await?;
        transaction.commit().await
    }

    pub async fn storage(&self) -> Result<StorageSettings, sqlx::Error> {
        let row = sqlx::query(
            "SELECT s3_endpoint, s3_region, s3_bucket, s3_prefix, s3_access_key_id, s3_secret_access_key, \
             s3_path_style, attachments_in_s3, backups_in_s3, backup_schedule FROM instance_settings WHERE id = 1",
        )
        .fetch_one(self.database.pool())
        .await?;
        let s3 = match (
            row.get::<Option<String>, _>("s3_endpoint"),
            row.get::<Option<String>, _>("s3_bucket"),
            row.get::<Option<String>, _>("s3_access_key_id"),
            row.get::<Option<Vec<u8>>, _>("s3_secret_access_key"),
        ) {
            (Some(endpoint), Some(bucket), Some(access_key_id), Some(secret_access_key)) => {
                Some(S3Settings {
                    endpoint,
                    region: row
                        .get::<Option<String>, _>("s3_region")
                        .unwrap_or_default(),
                    bucket,
                    prefix: row.get("s3_prefix"),
                    access_key_id,
                    secret_access_key,
                    path_style: row.get::<i64, _>("s3_path_style") == 1,
                })
            }
            _ => None,
        };
        Ok(StorageSettings {
            attachments_in_s3: s3.is_some() && row.get::<i64, _>("attachments_in_s3") == 1,
            backups_in_s3: s3.is_some() && row.get::<i64, _>("backups_in_s3") == 1,
            s3,
            backup_schedule: BackupSchedule::parse(&row.get::<String, _>("backup_schedule")),
        })
    }

    /// Saves the bucket. The caller has checked that Orbit can write to it.
    pub async fn save_s3(
        &self,
        actor_id: Id,
        s3: &S3Settings,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), sqlx::Error> {
        let mut transaction = self.database.immediate_transaction().await?;
        sqlx::query(
            "UPDATE instance_settings SET s3_endpoint = $1, s3_region = $2, s3_bucket = $3, s3_prefix = $4, \
             s3_access_key_id = $5, s3_secret_access_key = $6, s3_path_style = $7, updated_at = $8 WHERE id = 1",
        )
        .bind(&s3.endpoint)
        .bind(&s3.region)
        .bind(&s3.bucket)
        .bind(&s3.prefix)
        .bind(&s3.access_key_id)
        .bind(&s3.secret_access_key)
        .bind(i64::from(s3.path_style))
        .bind(now.as_millis())
        .execute(&mut *transaction)
        .await?;
        audit::record_global(
            &mut transaction,
            Some(actor_id),
            "instance.storage_updated",
            AuditOutcome::Success,
            "installation",
            None,
            request_id,
            serde_json::json!({"endpoint": s3.endpoint, "bucket": s3.bucket, "prefix": s3.prefix}),
            now,
        )
        .await?;
        transaction.commit().await
    }

    /// Removes the bucket and turns off everything that used it.
    pub async fn clear_s3(
        &self,
        actor_id: Id,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), sqlx::Error> {
        let mut transaction = self.database.immediate_transaction().await?;
        sqlx::query(
            "UPDATE instance_settings SET s3_endpoint = NULL, s3_region = NULL, s3_bucket = NULL, s3_prefix = '', \
             s3_access_key_id = NULL, s3_secret_access_key = NULL, s3_path_style = 0, attachments_in_s3 = 0, \
             backups_in_s3 = 0, updated_at = $1 WHERE id = 1",
        )
        .bind(now.as_millis())
        .execute(&mut *transaction)
        .await?;
        audit::record_global(
            &mut transaction,
            Some(actor_id),
            "instance.storage_removed",
            AuditOutcome::Success,
            "installation",
            None,
            request_id,
            serde_json::json!({}),
            now,
        )
        .await?;
        transaction.commit().await
    }

    /// Where attachments and backups go, and how often Orbit backs up by itself.
    pub async fn save_storage_options(
        &self,
        actor_id: Id,
        attachments_in_s3: bool,
        backups_in_s3: bool,
        backup_schedule: BackupSchedule,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), StorageSettingsError> {
        let mut transaction = self.database.immediate_transaction().await?;
        if attachments_in_s3 || backups_in_s3 {
            let configured = sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM instance_settings WHERE id = 1 AND s3_endpoint IS NOT NULL",
            )
            .fetch_one(&mut *transaction)
            .await?;
            if configured == 0 {
                return Err(StorageSettingsError::NotConfigured);
            }
        }
        sqlx::query(
            "UPDATE instance_settings SET attachments_in_s3 = $1, backups_in_s3 = $2, backup_schedule = $3, \
             updated_at = $4 WHERE id = 1",
        )
        .bind(i64::from(attachments_in_s3))
        .bind(i64::from(backups_in_s3))
        .bind(backup_schedule.as_str())
        .bind(now.as_millis())
        .execute(&mut *transaction)
        .await?;
        audit::record_global(
            &mut transaction,
            Some(actor_id),
            "instance.storage_options_updated",
            AuditOutcome::Success,
            "installation",
            None,
            request_id,
            serde_json::json!({
                "attachments_in_s3": attachments_in_s3,
                "backups_in_s3": backups_in_s3,
                "backup_schedule": backup_schedule.as_str(),
            }),
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn job_limits(&self) -> Result<Vec<JobKindLimits>, sqlx::Error> {
        let overrides = load_limit_overrides(self.database.pool()).await?;
        Ok(adjustable_job_kinds()
            .into_iter()
            .map(|kind| JobKindLimits {
                limits: overrides
                    .get(kind.name())
                    .copied()
                    .unwrap_or_else(|| kind.default_limits()),
                defaults: kind.default_limits(),
                kind: kind.name().to_owned(),
            })
            .collect())
    }

    /// Saves the limits of adjustable job kinds; limits equal to the defaults remove the kind's
    /// row, so later changes of the defaults reach it. The workers read them on their next claim.
    pub async fn save_job_limits(
        &self,
        actor_id: Id,
        limits: &[(JobKind, JobLimits)],
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), sqlx::Error> {
        let limit = |value: Option<u32>| value.map(i64::from);
        let mut transaction = self.database.immediate_transaction().await?;
        for (kind, kind_limits) in limits {
            if *kind_limits == kind.default_limits() {
                sqlx::query("DELETE FROM job_limits WHERE kind = $1")
                    .bind(kind.name())
                    .execute(&mut *transaction)
                    .await?;
            } else {
                sqlx::query(
                    "INSERT INTO job_limits (kind, concurrency, workspace_concurrency, updated_at) \
                     VALUES ($1, $2, $3, $4) ON CONFLICT (kind) DO UPDATE SET \
                     concurrency = excluded.concurrency, \
                     workspace_concurrency = excluded.workspace_concurrency, \
                     updated_at = excluded.updated_at",
                )
                .bind(kind.name())
                .bind(limit(kind_limits.concurrency))
                .bind(limit(kind_limits.workspace_concurrency))
                .bind(now.as_millis())
                .execute(&mut *transaction)
                .await?;
            }
        }
        audit::record_global(
            &mut transaction,
            Some(actor_id),
            "instance.job_limits_updated",
            AuditOutcome::Success,
            "installation",
            None,
            request_id,
            serde_json::json!({
                "kinds": limits.iter().map(|(kind, limits)| serde_json::json!({
                    "kind": kind.name(),
                    "concurrency": limits.concurrency,
                    "workspace_concurrency": limits.workspace_concurrency,
                })).collect::<Vec<_>>(),
            }),
            now,
        )
        .await?;
        transaction.commit().await
    }
}
