//! Settings of the whole instance that the root user changes in Admin: open registration, the outgoing mail
//! server, object storage (S3), automatic backups and the limits of a shared installation. One row
//! (`instance_settings.id = 1`, created by migration 0042). Background job limits have a row for each changed job
//! kind (`job_limits`).

use orbit_platform::Row;
pub use orbit_platform::{BackupSchedule, SmtpSecurity};
use orbit_platform::{Database, Id, JobKind, JobLimits, TimestampMillis, load_limit_overrides};
use thiserror::Error;

use super::environment_settings::EnvironmentSettings;
use crate::audit::{self, AuditOutcome};

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

/// Where a bucket's objects are: two buckets at one location share their objects.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BucketLocation {
    pub endpoint: String,
    pub bucket: String,
    pub prefix: String,
}

impl BucketLocation {
    #[must_use]
    pub fn of(s3: &S3Settings) -> Self {
        Self {
            endpoint: s3.endpoint.clone(),
            bucket: s3.bucket.clone(),
            prefix: s3.prefix.clone(),
        }
    }
}

impl std::fmt::Display for BucketLocation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "bucket {} at {} with prefix {:?}",
            self.bucket, self.endpoint, self.prefix
        )
    }
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

/// Limits for a shared installation (Orbit Cloud). `None` is unlimited (stored as 0).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InstanceLimits {
    /// Workspaces (not in the trash) one account may own. Instance admins are not limited.
    pub max_owned_workspaces: Option<u32>,
    /// Bytes of files one workspace may store.
    pub workspace_storage_quota_bytes: Option<u64>,
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
    #[error("Orbit Cloud needs a mail server")]
    SmtpRequired,
    #[error("instance settings are unavailable")]
    Unavailable(#[from] sqlx::Error),
}

/// The job kinds whose limits the root user can change in Admin: the work that customers start.
/// Maintenance kinds keep their limit of one.
#[must_use]
pub fn adjustable_job_kinds() -> [JobKind; 11] {
    [
        crate::notion::import::scan_job_kind(),
        crate::notion::import::import_job_kind(),
        crate::asana::import::scan_job_kind(),
        crate::asana::import::import_job_kind(),
        crate::linear::import::scan_job_kind(),
        crate::linear::import::import_job_kind(),
        crate::trello::import::scan_job_kind(),
        crate::trello::import::import_job_kind(),
        crate::obsidian::import::scan_job_kind(),
        crate::obsidian::import::import_job_kind(),
        crate::workspace_export::job_kind(),
    ]
}

/// The limits in use for one adjustable job kind, and the code defaults.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JobKindLimits {
    pub kind: String,
    pub limits: JobLimits,
    pub defaults: JobLimits,
}

/// Sign-up needs a way to prove the email: an emailed link, or a sign-in provider that verified it.
/// Orbit Cloud needs the mail server even with a provider.
pub(crate) fn can_confirm_sign_up(database: &Database, smtp: bool, sign_in_provider: bool) -> bool {
    smtp || (sign_in_provider && !super::billing::enabled(database))
}

#[derive(Clone, Debug)]
pub struct InstanceSettingsRepository {
    pub(super) database: Database,
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
        let environment = EnvironmentSettings::of(&self.database);
        Ok(InstanceSettings {
            registration_open: environment
                .registration_open
                .unwrap_or(row.get::<i64, _>("registration_open") == 1),
            smtp: environment.smtp.clone().or(smtp),
        })
    }

    /// Opening registration needs a way to confirm the email of a new account (see [`can_confirm_sign_up`]).
    pub async fn set_registration_open(
        &self,
        actor_id: Id,
        open: bool,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), InstanceSettingsError> {
        let mut transaction = self.database.immediate_transaction().await?;
        let environment = EnvironmentSettings::of(&self.database);
        let billing = super::billing::enabled(&self.database);
        let set_by_environment = can_confirm_sign_up(
            &self.database,
            environment.smtp.is_some(),
            !environment.sign_in_providers.is_empty(),
        );
        if open && !set_by_environment {
            let configured = sqlx::query_scalar::<_, i64>(
                if billing {
                    "SELECT COUNT(*) FROM instance_settings WHERE id = 1 AND smtp_host IS NOT NULL"
                } else {
                    "SELECT (SELECT COUNT(*) FROM instance_settings WHERE id = 1 AND smtp_host IS NOT NULL) \
                     + (SELECT COUNT(*) FROM sign_in_providers)"
                },
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

    /// Mail is on for this installation (off until the root user turns it on). Off hides the mail
    /// app and refuses the workspace mail API; mail for domains added before still arrives.
    pub async fn mail_enabled(&self) -> Result<bool, sqlx::Error> {
        let enabled =
            sqlx::query_scalar::<_, i64>("SELECT mail_enabled FROM instance_settings WHERE id = 1")
                .fetch_one(self.database.pool())
                .await?;
        Ok(enabled == 1)
    }

    pub async fn set_mail_enabled(
        &self,
        actor_id: Id,
        enabled: bool,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), sqlx::Error> {
        self.set_mail_switches(actor_id, Some(enabled), None, request_id, now)
            .await
    }

    /// Sets the given mail switches together, in one transaction with one audit event.
    pub async fn set_mail_switches(
        &self,
        actor_id: Id,
        mail_enabled: Option<bool>,
        workspace_providers_enabled: Option<bool>,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), sqlx::Error> {
        let mut transaction = self.database.immediate_transaction().await?;
        let mut changed = serde_json::Map::new();
        for (column, field, value) in [
            ("mail_enabled", "mail_enabled", mail_enabled),
            (
                "mail_workspace_providers_enabled",
                "workspace_providers_enabled",
                workspace_providers_enabled,
            ),
        ] {
            let Some(enabled) = value else { continue };
            sqlx::query(sqlx::AssertSqlSafe(format!(
                "UPDATE instance_settings SET {column} = $1, updated_at = $2 WHERE id = 1"
            )))
            .bind(i64::from(enabled))
            .bind(now.as_millis())
            .execute(&mut *transaction)
            .await?;
            changed.insert(field.to_owned(), enabled.into());
        }
        if changed.is_empty() {
            return Ok(());
        }
        audit::record_global(
            &mut transaction,
            Some(actor_id),
            "instance.mail_updated",
            AuditOutcome::Success,
            "installation",
            None,
            request_id,
            serde_json::Value::Object(changed),
            now,
        )
        .await?;
        transaction.commit().await
    }

    /// Whether workspace admins may set up their own mail providers (on by default; Orbit Cloud
    /// turns it off). Off stops new ones only: existing workspace providers keep receiving.
    pub async fn mail_workspace_providers_enabled(&self) -> Result<bool, sqlx::Error> {
        let enabled = sqlx::query_scalar::<_, i64>(
            "SELECT mail_workspace_providers_enabled FROM instance_settings WHERE id = 1",
        )
        .fetch_one(self.database.pool())
        .await?;
        Ok(enabled == 1)
    }

    pub async fn set_mail_workspace_providers_enabled(
        &self,
        actor_id: Id,
        enabled: bool,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), sqlx::Error> {
        self.set_mail_switches(actor_id, None, Some(enabled), request_id, now)
            .await
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
    /// Orbit Cloud refuses: it always needs one.
    pub async fn clear_smtp(
        &self,
        actor_id: Id,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), InstanceSettingsError> {
        if super::billing::enabled(&self.database) {
            return Err(InstanceSettingsError::SmtpRequired);
        }
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
        transaction.commit().await?;
        Ok(())
    }

    pub async fn limits(&self) -> Result<InstanceLimits, sqlx::Error> {
        let row = sqlx::query(
            "SELECT max_owned_workspaces, workspace_storage_quota_bytes FROM instance_settings WHERE id = 1",
        )
        .fetch_one(self.database.pool())
        .await?;
        Ok(InstanceLimits {
            max_owned_workspaces: u32::try_from(row.get::<i64, _>("max_owned_workspaces"))
                .ok()
                .filter(|limit| *limit > 0),
            workspace_storage_quota_bytes: u64::try_from(
                row.get::<i64, _>("workspace_storage_quota_bytes"),
            )
            .ok()
            .filter(|limit| *limit > 0),
        })
    }

    pub async fn save_limits(
        &self,
        actor_id: Id,
        limits: InstanceLimits,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), sqlx::Error> {
        let max_owned_workspaces = i64::from(limits.max_owned_workspaces.unwrap_or(0));
        let storage_quota = limits
            .workspace_storage_quota_bytes
            .map_or(0, |bytes| i64::try_from(bytes).unwrap_or(i64::MAX));
        let mut transaction = self.database.immediate_transaction().await?;
        sqlx::query(
            "UPDATE instance_settings SET max_owned_workspaces = $1, workspace_storage_quota_bytes = $2, \
             updated_at = $3 WHERE id = 1",
        )
        .bind(max_owned_workspaces)
        .bind(storage_quota)
        .bind(now.as_millis())
        .execute(&mut *transaction)
        .await?;
        audit::record_global(
            &mut transaction,
            Some(actor_id),
            "instance.limits_updated",
            AuditOutcome::Success,
            "installation",
            None,
            request_id,
            serde_json::json!({
                "max_owned_workspaces": max_owned_workspaces,
                "workspace_storage_quota_bytes": storage_quota,
            }),
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
        let environment = EnvironmentSettings::of(&self.database);
        let s3 = environment.s3.clone().or(s3);
        Ok(StorageSettings {
            attachments_in_s3: s3.is_some()
                && environment
                    .attachments_in_s3
                    .unwrap_or(row.get::<i64, _>("attachments_in_s3") == 1),
            backups_in_s3: s3.is_some()
                && environment
                    .backups_in_s3
                    .unwrap_or(row.get::<i64, _>("backups_in_s3") == 1),
            s3,
            backup_schedule: environment.backup_schedule.unwrap_or_else(|| {
                BackupSchedule::parse(&row.get::<String, _>("backup_schedule")).unwrap_or_default()
            }),
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
        // The caller found no attachment files left in a bucket at another location.
        sqlx::query(
            "UPDATE instance_settings SET files_s3_endpoint = NULL, files_s3_bucket = NULL, files_s3_prefix = NULL \
             WHERE id = 1 AND NOT (files_s3_endpoint = $1 AND files_s3_bucket = $2 AND files_s3_prefix = $3)",
        )
        .bind(&s3.endpoint)
        .bind(&s3.bucket)
        .bind(&s3.prefix)
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
             backups_in_s3 = 0, files_s3_endpoint = NULL, files_s3_bucket = NULL, files_s3_prefix = NULL, \
             updated_at = $1 WHERE id = 1",
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
        if (attachments_in_s3 || backups_in_s3)
            && EnvironmentSettings::of(&self.database).s3.is_none()
        {
            let configured = sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM instance_settings WHERE id = 1 AND s3_endpoint IS NOT NULL",
            )
            .fetch_one(&mut *transaction)
            .await?;
            if configured == 0 {
                return Err(StorageSettingsError::NotConfigured);
            }
        }
        // The saved value of a setting from the environment stays, for when its variable is removed.
        let environment = EnvironmentSettings::of(&self.database);
        let saved = |set: bool, value: i64| (!set).then_some(value);
        sqlx::query(
            "UPDATE instance_settings SET attachments_in_s3 = COALESCE($1, attachments_in_s3), \
             backups_in_s3 = COALESCE($2, backups_in_s3), backup_schedule = COALESCE($3, backup_schedule), \
             updated_at = $4 WHERE id = 1",
        )
        .bind(saved(environment.attachments_in_s3.is_some(), i64::from(attachments_in_s3)))
        .bind(saved(environment.backups_in_s3.is_some(), i64::from(backups_in_s3)))
        .bind(
            environment
                .backup_schedule
                .is_none()
                .then_some(backup_schedule.as_str()),
        )
        .bind(now.as_millis())
        .execute(&mut *transaction)
        .await?;
        if attachments_in_s3 {
            match &environment.s3 {
                Some(s3) => {
                    record_files_bucket(&mut transaction, &BucketLocation::of(s3)).await?;
                }
                None => {
                    sqlx::query(
                        "UPDATE instance_settings SET files_s3_endpoint = s3_endpoint, files_s3_bucket = s3_bucket, \
                         files_s3_prefix = s3_prefix WHERE id = 1",
                    )
                    .execute(&mut *transaction)
                    .await?;
                }
            }
        }
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

    /// The bucket that may hold the installation's attachment files (migration `20261111122359_files_bucket`).
    pub async fn files_bucket(&self) -> Result<Option<BucketLocation>, sqlx::Error> {
        let row = sqlx::query(
            "SELECT files_s3_endpoint, files_s3_bucket, files_s3_prefix FROM instance_settings WHERE id = 1",
        )
        .fetch_one(self.database.pool())
        .await?;
        Ok(
            match (
                row.get::<Option<String>, _>("files_s3_endpoint"),
                row.get::<Option<String>, _>("files_s3_bucket"),
            ) {
                (Some(endpoint), Some(bucket)) => Some(BucketLocation {
                    endpoint,
                    bucket,
                    prefix: row
                        .get::<Option<String>, _>("files_s3_prefix")
                        .unwrap_or_default(),
                }),
                _ => None,
            },
        )
    }

    /// Attachments use the bucket at `location`: Orbit keeps it until the mover empties it.
    pub async fn keep_files_bucket(&self, location: &BucketLocation) -> Result<(), sqlx::Error> {
        let mut transaction = self.database.immediate_transaction().await?;
        record_files_bucket(&mut transaction, location).await?;
        transaction.commit().await
    }

    /// No attachment files are left in the bucket at `location`. False when another bucket is recorded, or when
    /// `unless_attachments_saved_on` and the saved setting sends attachments to S3 again: turning them on records
    /// the bucket in the same transaction, so this one statement cannot clear a record that new uploads need.
    pub async fn files_bucket_emptied(
        &self,
        location: &BucketLocation,
        unless_attachments_saved_on: bool,
    ) -> Result<bool, sqlx::Error> {
        let cleared = sqlx::query(
            "UPDATE instance_settings SET files_s3_endpoint = NULL, files_s3_bucket = NULL, files_s3_prefix = NULL \
             WHERE id = 1 AND files_s3_endpoint = $1 AND files_s3_bucket = $2 AND files_s3_prefix = $3 \
             AND ($4 = 0 OR attachments_in_s3 = 0)",
        )
        .bind(&location.endpoint)
        .bind(&location.bucket)
        .bind(&location.prefix)
        .bind(i64::from(unless_attachments_saved_on))
        .execute(self.database.pool())
        .await?
        .rows_affected();
        Ok(cleared > 0)
    }

    /// `orbit attachments forget-bucket`: the operator says the recorded bucket no longer matters. Files still in
    /// it are not used again. Returns the bucket it forgot.
    pub async fn forget_files_bucket(
        &self,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<Option<BucketLocation>, sqlx::Error> {
        let Some(location) = self.files_bucket().await? else {
            return Ok(None);
        };
        let mut transaction = self.database.immediate_transaction().await?;
        sqlx::query(
            "UPDATE instance_settings SET files_s3_endpoint = NULL, files_s3_bucket = NULL, files_s3_prefix = NULL \
             WHERE id = 1",
        )
        .execute(&mut *transaction)
        .await?;
        audit::record_global(
            &mut transaction,
            None,
            "instance.files_bucket_forgotten",
            AuditOutcome::Success,
            "installation",
            None,
            request_id,
            serde_json::json!({
                "endpoint": location.endpoint,
                "bucket": location.bucket,
                "prefix": location.prefix,
            }),
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(Some(location))
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

async fn record_files_bucket(
    transaction: &mut sqlx::Transaction<'_, sqlx::Any>,
    location: &BucketLocation,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE instance_settings SET files_s3_endpoint = $1, files_s3_bucket = $2, files_s3_prefix = $3 \
         WHERE id = 1",
    )
    .bind(&location.endpoint)
    .bind(&location.bucket)
    .bind(&location.prefix)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}
