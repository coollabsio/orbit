//! The instance settings that `ORBIT_*` variables set ([`InstanceConfig`]). They replace the values saved in Admin
//! on every read of [`InstanceSettingsRepository`], and Admin shows them read-only. They are not saved: when a
//! variable is removed, the saved value is used again. Only the location of a bucket that holds attachment files is
//! recorded (`files_s3_*`), so that a later bucket change cannot lose them.

use std::sync::Arc;

use orbit_platform::{Database, InstanceConfig, Row, S3Config};
use sqlx::{Any, Transaction};

use super::instance_settings::{
    BackupSchedule, BucketLocation, InstanceSettingsRepository, S3Settings, SmtpSettings,
};
use super::sign_in_providers::SavedSignInProvider;
use super::workspace_storage::WorkspaceStorageRepository;
use crate::object_storage::Owner;
use crate::secret_box::encrypt_secret;
use crate::sign_in_providers::Provider;

/// Secrets are encrypted under the app key, the same as saved ones, so the code that uses a setting does not
/// know where it came from.
#[derive(Debug, Default)]
pub struct EnvironmentSettings {
    pub registration_open: Option<bool>,
    pub smtp: Option<SmtpSettings>,
    pub s3: Option<S3Settings>,
    pub attachments_in_s3: Option<bool>,
    pub backups_in_s3: Option<bool>,
    pub backup_schedule: Option<BackupSchedule>,
    pub sign_in_providers: Vec<SavedSignInProvider>,
}

impl EnvironmentSettings {
    /// The settings of `database`; none when [`EnvironmentSettings::install`] did not run.
    #[must_use]
    pub fn of(database: &Database) -> Arc<Self> {
        database.existing_extension::<Self>().unwrap_or_default()
    }

    #[must_use]
    pub fn sign_in_provider(&self, provider: Provider) -> Option<&SavedSignInProvider> {
        self.sign_in_providers
            .iter()
            .find(|saved| saved.provider == provider)
    }

    /// The settings of `config`, with their secrets encrypted under `app_key`.
    pub fn from_config(config: &InstanceConfig, app_key: &[u8; 32]) -> Result<Self, String> {
        let seal = |secret: &str| {
            encrypt_secret(app_key, secret)
                .map_err(|()| "a secret could not be encrypted".to_owned())
        };
        let smtp = config
            .smtp
            .as_ref()
            .map(|smtp| -> Result<SmtpSettings, String> {
                if !crate::mail::valid_address(&smtp.from_address) {
                    return Err("ORBIT_SMTP_FROM must be one email address".to_owned());
                }
                Ok(SmtpSettings {
                    host: smtp.host.clone(),
                    port: smtp.port,
                    security: smtp.security,
                    username: smtp.username.clone(),
                    password: smtp
                        .password
                        .as_ref()
                        .map(|password| seal(password.expose()))
                        .transpose()?,
                    from_address: smtp.from_address.clone(),
                    from_name: smtp.from_name.clone(),
                })
            })
            .transpose()?;
        let s3 = config
            .s3
            .as_ref()
            .map(|s3| seal(&s3.secret_access_key).map(|secret| s3_settings(s3, secret)))
            .transpose()?;
        let mut sign_in_providers = Vec::new();
        for (provider, client) in [
            (Provider::Google, &config.google),
            (Provider::Github, &config.github),
        ] {
            if let Some(client) = client {
                sign_in_providers.push(SavedSignInProvider {
                    provider,
                    client_id: client.client_id.clone(),
                    client_secret: seal(client.client_secret.expose())?,
                });
            }
        }
        Ok(Self {
            registration_open: config.registration_open,
            smtp,
            s3,
            attachments_in_s3: config.attachments_in_s3,
            backups_in_s3: config.backups_in_s3,
            backup_schedule: config.backup_schedule,
            sign_in_providers,
        })
    }

    /// From now on, every read of the instance settings of `database` uses these settings.
    pub fn install(self, database: &Database) {
        database.extension(|| self);
    }

    /// Refuses a bucket that would lose files, before [`EnvironmentSettings::install`]: another bucket than the
    /// one that may still hold the attachment files, or a bucket that a workspace uses. When the recorded bucket
    /// is the one saved in Admin, Orbit looks into it: an empty one is forgotten.
    pub async fn check_bucket(
        &self,
        database: &Database,
        app_key: &[u8; 32],
    ) -> Result<(), String> {
        let unavailable =
            |error: sqlx::Error| format!("the saved storage settings cannot be read: {error}");
        let repository = InstanceSettingsRepository::new(database.clone());
        let saved = repository.storage().await.map_err(unavailable)?;
        if let Some(s3) = &self.s3 {
            let workspaces = WorkspaceStorageRepository::new(database.clone())
                .all()
                .await
                .map_err(unavailable)?;
            if let Some(workspace) = workspaces
                .iter()
                .find(|workspace| crate::object_storage::overlap(&workspace.s3, s3))
            {
                return Err(format!(
                    "workspace {} keeps its files in the ORBIT_S3_* bucket under this prefix, or a prefix \
                     inside or around it: use another bucket or ORBIT_S3_PREFIX",
                    workspace.workspace_id
                ));
            }
        }
        let Some(recorded) = repository.files_bucket().await.map_err(unavailable)? else {
            return Ok(());
        };
        let used = self.s3.as_ref().or(saved.s3.as_ref());
        if used.is_some_and(|s3| BucketLocation::of(s3) == recorded) {
            return Ok(());
        }
        let refuse = |reason: &str| {
            format!(
                "the attachment files may still be in the {recorded}, and Orbit would use {used} now \
                 ({reason}). Start Orbit with that bucket and ORBIT_S3_ATTACHMENTS=false (or S3 for \
                 attachments turned off in Admin → Storage), and wait until Admin → Storage shows no files left \
                 to move. If the files in that bucket are no longer needed, stop Orbit and run `orbit \
                 attachments forget-bucket --yes`",
                used = used.map_or("no bucket".to_owned(), |s3| BucketLocation::of(s3)
                    .to_string()),
            )
        };
        let Some(old) = saved
            .s3
            .as_ref()
            .filter(|s3| BucketLocation::of(s3) == recorded)
        else {
            return Err(refuse("Orbit has no credentials to check that bucket"));
        };
        let has_files =
            crate::object_storage::saved_bucket(old, Some(app_key), Owner::Installation)?
                .any("blobs/")
                .await
                .map_err(|error| {
                    refuse(&format!(
                        "the bucket could not be checked: {}",
                        error.summary()
                    ))
                })?;
        if has_files {
            return Err(refuse("files are still in it"));
        }
        repository
            .files_bucket_emptied(&recorded, false)
            .await
            .map_err(unavailable)?;
        Ok(())
    }
}

fn s3_settings(s3: &S3Config, secret_access_key: Vec<u8>) -> S3Settings {
    S3Settings {
        endpoint: s3.endpoint.clone(),
        region: s3.region.clone(),
        bucket: s3.bucket.clone(),
        prefix: s3.prefix.clone(),
        access_key_id: s3.access_key_id.clone(),
        secret_access_key,
        path_style: s3.path_style,
    }
}

/// Whether registration is open, read in the caller's transaction unless `ORBIT_OPEN_REGISTRATION` sets it.
pub(crate) async fn registration_open(
    database: &Database,
    transaction: &mut Transaction<'_, Any>,
) -> Result<bool, sqlx::Error> {
    if let Some(open) = EnvironmentSettings::of(database).registration_open {
        return Ok(open);
    }
    let row = sqlx::query("SELECT registration_open FROM instance_settings WHERE id = 1")
        .fetch_one(&mut **transaction)
        .await?;
    Ok(row.get::<i64, _>("registration_open") == 1)
}
