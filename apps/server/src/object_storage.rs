//! Object storage (S3) for attachments and backups, as saved by the root user in Admin → Storage and by
//! workspaces for their own files in Settings → File storage: builds the buckets from the saved settings, applies
//! them while the server runs, moves blobs in the background and makes the automatic backups.

use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use orbit_platform::Row;
use orbit_platform::{
    BackupService, Database, Id, ObjectStorage, ObjectStorageState, S3Bucket, S3Config,
    TieredBlobStore, TimestampMillis, UploadError, UploadService, WorkspaceBucket,
};
use tokio_util::sync::CancellationToken;

use crate::repositories::environment_settings::EnvironmentSettings;
use crate::repositories::instance_settings::{
    BackupSchedule, BucketLocation, InstanceSettingsRepository, S3Settings, StorageSettings,
};
use crate::repositories::workspace_storage::{WorkspaceStorage, WorkspaceStorageRepository};
use crate::secret_box::decrypt_secret;

const HOUR_MILLIS: i64 = 60 * 60 * 1000;
/// The mover also runs this often without a settings change, to retry after an error.
const MOVE_INTERVAL: Duration = Duration::from_secs(10 * 60);
/// The backup job runs every hour; a backup is due a little before its period ends, so a job that runs a few
/// minutes early does not skip a period.
const SCHEDULE_SLACK_MILLIS: i64 = 10 * 60 * 1000;

/// Who picked the bucket: a workspace bucket may only be on the public internet (SSRF).
#[derive(Clone, Copy)]
pub enum Owner {
    Installation,
    Workspace,
}

/// `secret_access_key` in plain text, for a bucket that is not saved yet.
pub fn bucket(
    s3: &S3Settings,
    secret_access_key: String,
    owner: Owner,
) -> Result<S3Bucket, String> {
    let config = config(s3, secret_access_key);
    match owner {
        Owner::Installation => S3Bucket::new(config),
        Owner::Workspace => S3Bucket::new_public(config),
    }
    .map_err(|error| error.to_string())
}

fn config(s3: &S3Settings, secret_access_key: String) -> S3Config {
    S3Config {
        endpoint: s3.endpoint.clone(),
        region: s3.region.clone(),
        bucket: s3.bucket.clone(),
        prefix: s3.prefix.clone(),
        access_key_id: s3.access_key_id.clone(),
        secret_access_key,
        path_style: s3.path_style,
    }
}

/// Whether the two buckets can address the same objects. They must not: the mover would treat one object as two.
pub fn overlap(left: &S3Settings, right: &S3Settings) -> bool {
    config(left, String::new()).overlaps(&config(right, String::new()))
}

/// The saved bucket, with its secret decrypted under the app key.
pub fn saved_bucket(
    s3: &S3Settings,
    app_key: Option<&[u8; 32]>,
    owner: Owner,
) -> Result<S3Bucket, String> {
    let key = app_key.ok_or("the server has no app key to read the S3 secret with")?;
    let secret = decrypt_secret(key, &s3.secret_access_key)
        .map_err(|()| "the S3 secret cannot be decrypted with this server's app key".to_owned())?;
    bucket(s3, secret, owner)
}

/// The installation's storage settings and the buckets that workspaces connected.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SavedStorage {
    pub installation: StorageSettings,
    pub workspaces: Vec<WorkspaceStorage>,
}

pub async fn load_saved(database: &Database) -> Result<SavedStorage, sqlx::Error> {
    Ok(SavedStorage {
        installation: InstanceSettingsRepository::new(database.clone())
            .storage()
            .await?,
        workspaces: WorkspaceStorageRepository::new(database.clone())
            .all()
            .await?,
    })
}

/// Makes the running server use `saved`. A workspace bucket that cannot be read (the app key changed) is left
/// out, so its workspace uses the installation's storage instead of failing all others.
pub fn apply(
    storage: &ObjectStorage,
    saved: &SavedStorage,
    app_key: Option<&[u8; 32]>,
) -> Result<(), String> {
    let settings = &saved.installation;
    let bucket = settings
        .s3
        .as_ref()
        .map(|s3| saved_bucket(s3, app_key, Owner::Installation))
        .transpose()?;
    let mut workspace_buckets = std::collections::HashMap::new();
    for workspace in &saved.workspaces {
        match saved_bucket(&workspace.s3, app_key, Owner::Workspace) {
            Ok(bucket) => {
                workspace_buckets.insert(
                    workspace.workspace_id.to_string(),
                    WorkspaceBucket {
                        bucket,
                        active: workspace.active,
                    },
                );
            }
            Err(error) => {
                tracing::warn!(workspace_id = %workspace.workspace_id, %error, "a workspace bucket is not used");
            }
        }
    }
    storage.configure(ObjectStorageState {
        bucket,
        attachments: settings.attachments_in_s3,
        backups: settings.backups_in_s3,
        workspace_buckets,
    });
    Ok(())
}

/// A new storage with the bucket saved in Admin → Storage, for a command that runs without the server.
pub async fn saved_storage(
    database: &Database,
    app_key: Option<&[u8; 32]>,
) -> Result<ObjectStorage, String> {
    let saved = load_saved(database)
        .await
        .map_err(|error| error.to_string())?;
    let storage = ObjectStorage::default();
    apply(&storage, &saved, app_key)?;
    Ok(storage)
}

/// The storage the server uses and the settings it applied, so the server can apply what another server saved
/// ([`reload_saved`]).
struct Applied {
    storage: ObjectStorage,
    app_key: Option<[u8; 32]>,
    settings: Mutex<SavedStorage>,
}

/// Makes the running server use `settings` (as [`apply`]) and remembers them for [`reload_saved`].
pub fn apply_and_keep(
    database: &Database,
    storage: &ObjectStorage,
    settings: &SavedStorage,
    app_key: Option<&[u8; 32]>,
) -> Result<(), String> {
    apply(storage, settings, app_key)?;
    let applied = database.extension(|| Applied {
        storage: storage.clone(),
        app_key: app_key.copied(),
        settings: Mutex::new(settings.clone()),
    });
    *applied
        .settings
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = settings.clone();
    Ok(())
}

/// Whether this server keeps new attachments in S3 (`None` before it applied settings).
pub(crate) fn attachments_in_s3(database: &Database) -> Option<bool> {
    database
        .existing_extension::<Applied>()
        .map(|applied| applied.storage.current().attachments)
}

/// Applies the saved storage settings if they differ from the ones this server uses (another server saved
/// them). Does nothing before the server applied any.
pub async fn reload_saved(database: &Database) -> Result<(), String> {
    let Some(applied) = database.existing_extension::<Applied>() else {
        return Ok(());
    };
    let saved = load_saved(database)
        .await
        .map_err(|error| error.to_string())?;
    if *applied
        .settings
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        == saved
    {
        return Ok(());
    }
    apply_and_keep(database, &applied.storage, &saved, applied.app_key.as_ref())
}

/// Moves blobs to the active tier after every settings change, and every ten minutes to retry after errors. Each
/// pass first moves the images older versions kept in the database to the blob store, so they move with all other
/// files.
pub async fn run_mover(
    database: Database,
    uploads: UploadService,
    store: TieredBlobStore,
    storage: ObjectStorage,
    shutdown: CancellationToken,
) -> Result<(), String> {
    loop {
        match move_images_to_blobs(&database, &uploads).await {
            Ok(0) => {}
            Ok(moved) => {
                tracing::info!(moved, "moved images from the database to attachment blobs")
            }
            Err(error) => {
                tracing::warn!(%error, "moving images to attachment blobs failed; retrying later")
            }
        }
        match store.move_blobs().await {
            Ok(0) => {}
            Ok(moved) => tracing::info!(moved, "moved attachment blobs between disk and S3"),
            Err(error) => tracing::warn!(%error, "moving attachment blobs failed; retrying later"),
        }
        if let Err(error) = forget_drained_buckets(&database, &store, &storage).await {
            tracing::warn!(%error, "removing emptied workspace buckets failed; retrying later");
        }
        if let Err(error) = track_files_bucket(&database, &storage).await {
            tracing::warn!(%error, "recording the bucket of the attachment files failed; retrying later");
        }
        tokio::select! {
            () = shutdown.cancelled() => return Ok(()),
            () = storage.changed() => {}
            () = tokio::time::sleep(MOVE_INTERVAL) => {}
        }
    }
}

/// Keeps the record of the bucket that holds the attachment files in step with this server: recorded while
/// attachments go to it, forgotten once they are off and no file is left in it. Then the bucket can change at the
/// next start ([`EnvironmentSettings::check_bucket`]). Recording again on every pass also repairs a record that
/// another server cleared just before attachments were turned on.
///
/// [`EnvironmentSettings::check_bucket`]: crate::repositories::environment_settings::EnvironmentSettings::check_bucket
async fn track_files_bucket(database: &Database, storage: &ObjectStorage) -> Result<(), String> {
    let state = storage.current();
    let Some(bucket) = state.bucket else {
        return Ok(());
    };
    let current = bucket.config();
    let location = BucketLocation {
        endpoint: current.endpoint.clone(),
        bucket: current.bucket.clone(),
        prefix: current.prefix.clone(),
    };
    let repository = InstanceSettingsRepository::new(database.clone());
    let recorded = repository
        .files_bucket()
        .await
        .map_err(|error| error.to_string())?;
    if state.attachments {
        if recorded.as_ref() != Some(&location) {
            repository
                .keep_files_bucket(&location)
                .await
                .map_err(|error| error.to_string())?;
        }
        return Ok(());
    }
    if recorded.as_ref() != Some(&location)
        || bucket
            .any("blobs/")
            .await
            .map_err(|error| error.to_string())?
    {
        return Ok(());
    }
    // Attachments may be turned on again (on any server) after the check: the record then stays. With
    // `ORBIT_S3_ATTACHMENTS` set, nobody can turn them on, and the saved setting does not count.
    let unless_attachments_saved_on = EnvironmentSettings::of(database)
        .attachments_in_s3
        .is_none();
    repository
        .files_bucket_emptied(&location, unless_attachments_saved_on)
        .await
        .map_err(|error| error.to_string())?;
    Ok(())
}

/// Drops the removed workspace buckets that have no files left, on every server of the database.
async fn forget_drained_buckets(
    database: &Database,
    store: &TieredBlobStore,
    storage: &ObjectStorage,
) -> Result<(), String> {
    let repository = WorkspaceStorageRepository::new(database.clone());
    let mut forgot = false;
    let state = storage.current();
    for (workspace_id, own) in state.workspace_buckets {
        // Not checked since the removal yet, or files still wait to move back.
        let Some(status) = store.workspace_status(&workspace_id) else {
            continue;
        };
        if own.active || status.error.is_some() || status.remaining > 0 {
            continue;
        }
        // At the installation's location, its files are the installation's files.
        let shared = state
            .bucket
            .as_ref()
            .is_some_and(|bucket| bucket.config().same_location(own.bucket.config()));
        let empty = shared
            || !own
                .bucket
                .any(&format!("blobs/{workspace_id}/"))
                .await
                .map_err(|error| error.to_string())?;
        if empty {
            forgot |= repository
                .forget_drained(&workspace_id)
                .await
                .map_err(|error| error.to_string())?;
        }
    }
    if forgot {
        reload_saved(database).await?;
        crate::cluster::Cluster::of(database).send(crate::cluster::Message::StorageChanged);
    }
    Ok(())
}

/// Moves the profile pictures, custom emoji and stickers that still keep their bytes in the database (saved before
/// migration 0046) to the attachment blob store, which puts them in S3 when attachments go there. Returns how many
/// images moved.
pub async fn move_images_to_blobs(
    database: &Database,
    uploads: &UploadService,
) -> Result<u64, UploadError> {
    let mut moved = 0;
    // The table, its key and the id its blobs are grouped under (a profile picture has no workspace).
    for (table, key, scope) in [
        ("user_avatars", "user_id", "user_id"),
        ("custom_emoji", "id", "workspace_id"),
        ("custom_stickers", "id", "workspace_id"),
    ] {
        let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT {key} AS key, {scope} AS scope FROM {table} WHERE bytes IS NOT NULL"
        )))
        .fetch_all(database.pool())
        .await?;
        for row in rows {
            let row_key: String = row.try_get("key")?;
            let scope: Id = row
                .try_get::<String, _>("scope")?
                .parse()
                .map_err(|_| UploadError::InvalidState)?;
            let finalization = uploads.begin_finalization().await;
            let now = database.database_now().await?;
            let mut transaction = database.immediate_transaction().await?;
            // Replaced or deleted since the list was read.
            let Some(bytes) = sqlx::query_scalar::<_, Vec<u8>>(sqlx::AssertSqlSafe(format!(
                "SELECT bytes FROM {table} WHERE {key} = $1 AND bytes IS NOT NULL"
            )))
            .bind(&row_key)
            .fetch_optional(&mut *transaction)
            .await?
            else {
                continue;
            };
            let blob = finalization
                .finalize_unmetered_bytes_in_transaction(&mut transaction, scope, &bytes, now)
                .await?;
            sqlx::query(sqlx::AssertSqlSafe(format!(
                "UPDATE {table} SET bytes = NULL, blob_id = $1 WHERE {key} = $2"
            )))
            .bind(blob.id.to_string())
            .bind(&row_key)
            .execute(&mut *transaction)
            .await?;
            transaction.commit().await?;
            moved += 1;
        }
    }
    Ok(moved)
}

/// Runs every hour: creates a backup when the newest one is older than the saved schedule's period.
pub async fn run_scheduled_backup(
    database: &Database,
    backups: &BackupService,
) -> Result<(), String> {
    let schedule = InstanceSettingsRepository::new(database.clone())
        .storage()
        .await
        .map_err(|error| error.to_string())?
        .backup_schedule;
    let period = match schedule {
        BackupSchedule::Off => return Ok(()),
        BackupSchedule::Hourly => HOUR_MILLIS,
        BackupSchedule::Daily => 24 * HOUR_MILLIS,
    };
    let newest = backups
        .list()
        .await
        .map_err(|error| error.to_string())?
        .first()
        .map(|snapshot| snapshot.manifest.created_at);
    let now = TimestampMillis::now().as_millis();
    if newest.is_some_and(|created_at| now - created_at < period - SCHEDULE_SLACK_MILLIS) {
        return Ok(());
    }
    backups
        .create(database)
        .await
        .map(drop)
        .map_err(|error| error.to_string())
}

/// The bucket for `orbit backup …` from `ORBIT_BACKUP_S3_ENDPOINT`, `_BUCKET`, `_ACCESS_KEY_ID`,
/// `_SECRET_ACCESS_KEY` and optional `_REGION`, `_PREFIX`, `_PATH_STYLE=true`. The command line does not read the
/// saved settings: the database is locked while the server runs, and after a lost server only the bucket is left.
pub fn from_env() -> Result<ObjectStorage, String> {
    let storage = ObjectStorage::default();
    let variable = |name: &str| std::env::var(format!("ORBIT_BACKUP_S3_{name}")).ok();
    let Some(endpoint) = variable("ENDPOINT") else {
        return Ok(storage);
    };
    let required = |name: &str| variable(name).ok_or(format!("ORBIT_BACKUP_S3_{name} is not set"));
    let bucket = S3Bucket::new(S3Config {
        endpoint,
        region: variable("REGION").unwrap_or_default(),
        bucket: required("BUCKET")?,
        prefix: variable("PREFIX").unwrap_or_default(),
        access_key_id: required("ACCESS_KEY_ID")?,
        secret_access_key: required("SECRET_ACCESS_KEY")?,
        path_style: variable("PATH_STYLE").is_some_and(|value| value == "true" || value == "1"),
    })
    .map_err(|error| error.to_string())?;
    storage.configure(ObjectStorageState {
        bucket: Some(bucket),
        attachments: false,
        backups: true,
        ..ObjectStorageState::default()
    });
    Ok(storage)
}
