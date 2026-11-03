//! The S3 bucket a workspace connected for its own attachment files (Settings → File storage).

use orbit_domain::Permission;
use orbit_platform::{Database, Id, Row, TimestampMillis};
use thiserror::Error;

use crate::audit::{self, AuditOutcome};
use crate::repositories::instance_settings::S3Settings;
use crate::repositories::membership;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceStorage {
    pub workspace_id: Id,
    /// `secret_access_key` is encrypted under the app key.
    pub s3: S3Settings,
    /// Off: the workspace removed the bucket and its files move back to the installation's storage.
    pub active: bool,
}

#[derive(Debug, Error)]
pub enum WorkspaceStorageError {
    #[error("workspace not found")]
    NotFound,
    #[error("the role cannot manage the workspace's storage")]
    Forbidden,
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

#[derive(Clone)]
pub struct WorkspaceStorageRepository {
    database: Database,
}

const COLUMNS: &str = "workspace_id, endpoint, region, bucket, prefix, access_key_id, secret_access_key, path_style, \
                       active";

fn from_row(row: &sqlx::any::AnyRow) -> Result<WorkspaceStorage, sqlx::Error> {
    Ok(WorkspaceStorage {
        workspace_id: row
            .try_get::<String, _>("workspace_id")?
            .parse()
            .map_err(|error| sqlx::Error::Decode(Box::new(error)))?,
        s3: S3Settings {
            endpoint: row.try_get("endpoint")?,
            region: row.try_get("region")?,
            bucket: row.try_get("bucket")?,
            prefix: row.try_get("prefix")?,
            access_key_id: row.try_get("access_key_id")?,
            secret_access_key: row.try_get("secret_access_key")?,
            path_style: row.try_get::<i64, _>("path_style")? == 1,
        },
        active: row.try_get::<i64, _>("active")? == 1,
    })
}

impl WorkspaceStorageRepository {
    #[must_use]
    pub const fn new(database: Database) -> Self {
        Self { database }
    }

    /// Every connected bucket, removed ones included until their files moved back.
    pub async fn all(&self) -> Result<Vec<WorkspaceStorage>, sqlx::Error> {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT {COLUMNS} FROM workspace_storage ORDER BY workspace_id"
        )))
        .fetch_all(self.database.pool())
        .await?
        .iter()
        .map(from_row)
        .collect()
    }

    /// The workspace's bucket, for a member who may manage it.
    pub async fn get(
        &self,
        workspace_id: Id,
        user_id: Id,
    ) -> Result<Option<WorkspaceStorage>, WorkspaceStorageError> {
        let pool = self.database.pool();
        require_manager(pool, workspace_id, user_id).await?;
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT {COLUMNS} FROM workspace_storage WHERE workspace_id = $1"
        )))
        .bind(workspace_id.to_string())
        .fetch_optional(pool)
        .await?
        .as_ref()
        .map(from_row)
        .transpose()
        .map_err(Into::into)
    }

    /// Saves the bucket and makes it the workspace's storage. The caller has checked that Orbit can write to it.
    pub async fn save(
        &self,
        workspace_id: Id,
        user_id: Id,
        s3: &S3Settings,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), WorkspaceStorageError> {
        let mut transaction = self.database.immediate_transaction().await?;
        require_manager(&mut *transaction, workspace_id, user_id).await?;
        sqlx::query(
            "INSERT INTO workspace_storage (workspace_id, endpoint, region, bucket, prefix, access_key_id, \
             secret_access_key, path_style, active, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 1, $9) \
             ON CONFLICT (workspace_id) DO UPDATE SET endpoint = excluded.endpoint, region = excluded.region, \
             bucket = excluded.bucket, prefix = excluded.prefix, access_key_id = excluded.access_key_id, \
             secret_access_key = excluded.secret_access_key, path_style = excluded.path_style, active = 1, \
             updated_at = excluded.updated_at",
        )
        .bind(workspace_id.to_string())
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
        audit::record(
            &mut transaction,
            workspace_id,
            Some(user_id),
            "workspace.storage_updated",
            AuditOutcome::Success,
            "workspace",
            Some(workspace_id),
            request_id,
            serde_json::json!({"endpoint": s3.endpoint, "bucket": s3.bucket, "prefix": s3.prefix}),
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    /// Stops using the bucket: new files go to the installation's storage and the files in the bucket move back
    /// ([`Self::forget_drained`] then drops it). Nothing to do without a bucket.
    pub async fn remove(
        &self,
        workspace_id: Id,
        user_id: Id,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), WorkspaceStorageError> {
        let mut transaction = self.database.immediate_transaction().await?;
        require_manager(&mut *transaction, workspace_id, user_id).await?;
        let removed = sqlx::query(
            "UPDATE workspace_storage SET active = 0, updated_at = $1 WHERE workspace_id = $2 AND active = 1",
        )
        .bind(now.as_millis())
        .bind(workspace_id.to_string())
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if removed > 0 {
            audit::record(
                &mut transaction,
                workspace_id,
                Some(user_id),
                "workspace.storage_removed",
                AuditOutcome::Success,
                "workspace",
                Some(workspace_id),
                request_id,
                serde_json::json!({}),
                now,
            )
            .await?;
        }
        transaction.commit().await?;
        Ok(())
    }

    /// Drops a removed bucket whose files all moved back. Returns whether it did.
    pub async fn forget_drained(&self, workspace_id: &str) -> Result<bool, sqlx::Error> {
        let mut transaction = self.database.immediate_transaction().await?;
        let deleted =
            sqlx::query("DELETE FROM workspace_storage WHERE workspace_id = $1 AND active = 0")
                .bind(workspace_id)
                .execute(&mut *transaction)
                .await?
                .rows_affected();
        transaction.commit().await?;
        Ok(deleted > 0)
    }
}

async fn require_manager<'e, E>(
    executor: E,
    workspace_id: Id,
    user_id: Id,
) -> Result<(), WorkspaceStorageError>
where
    E: sqlx::Executor<'e, Database = sqlx::Any>,
{
    let actor = membership::actor(executor, workspace_id, user_id)
        .await?
        .ok_or(WorkspaceStorageError::NotFound)?;
    if actor.can(Permission::StorageManage) {
        Ok(())
    } else {
        Err(WorkspaceStorageError::Forbidden)
    }
}
