use sha2::{Digest, Sha256};
use sqlx::Row;
use thiserror::Error;

use super::Database;
use crate::TimestampMillis;

const PLATFORM_SCHEMA: &str = include_str!("../../../../apps/server/migrations/0001_platform.sql");
const IDENTITY_SCHEMA: &str = include_str!("../../../../apps/server/migrations/0002_identity.sql");
const ATTACHMENT_REFERENCE_SCHEMA: &str =
    include_str!("../../../../apps/server/migrations/0003_attachment_references.sql");
const WORKSPACE_SCHEMA: &str =
    include_str!("../../../../apps/server/migrations/0004_workspaces.sql");
const WORKSPACE_INTEGRITY_SCHEMA: &str =
    include_str!("../../../../apps/server/migrations/0005_workspace_integrity.sql");
const WORKSPACE_RETENTION_SECURITY_SCHEMA: &str =
    include_str!("../../../../apps/server/migrations/0006_workspace_retention_security.sql");
const TASKS_SCHEMA: &str = include_str!("../../../../apps/server/migrations/0007_tasks.sql");

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Migration {
    pub version: i64,
    pub sql: &'static str,
    pub destructive: bool,
}

impl Migration {
    #[must_use]
    pub const fn new(version: i64, sql: &'static str, destructive: bool) -> Self {
        Self {
            version,
            sql,
            destructive,
        }
    }

    #[must_use]
    pub fn checksum(&self) -> String {
        format!("{:x}", Sha256::digest(self.sql.as_bytes()))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingMigration {
    pub version: i64,
    pub checksum: String,
    pub destructive: bool,
}

#[derive(Debug)]
pub struct MigrationRunner {
    app_version: String,
    migrations: Vec<Migration>,
}

#[derive(Debug, Error)]
pub enum MigrationError {
    #[error("migration version {version} appears more than once")]
    DuplicateVersion { version: i64 },
    #[error(
        "database schema version {database_version} is newer than binary schema version {binary_version}"
    )]
    SchemaNewer {
        database_version: i64,
        binary_version: i64,
    },
    #[error("migration {version} is not known to this binary")]
    UnknownMigration { version: i64 },
    #[error("migration {version} checksum mismatch: expected {expected}, found {actual}")]
    ChecksumMismatch {
        version: i64,
        expected: String,
        actual: String,
    },
    #[error("migration {version} failed: {source}")]
    Apply {
        version: i64,
        #[source]
        source: sqlx::Error,
    },
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

impl MigrationRunner {
    #[must_use]
    pub fn new(app_version: impl Into<String>, mut migrations: Vec<Migration>) -> Self {
        migrations.sort_by_key(|migration| migration.version);
        Self {
            app_version: app_version.into(),
            migrations,
        }
    }

    #[must_use]
    pub fn embedded(app_version: impl Into<String>) -> Self {
        Self::new(
            app_version,
            vec![
                Migration::new(1, PLATFORM_SCHEMA, false),
                Migration::new(2, IDENTITY_SCHEMA, false),
                Migration::new(3, ATTACHMENT_REFERENCE_SCHEMA, false),
                Migration::new(4, WORKSPACE_SCHEMA, false),
                Migration::new(5, WORKSPACE_INTEGRITY_SCHEMA, false),
                Migration::new(6, WORKSPACE_RETENTION_SECURITY_SCHEMA, false),
                Migration::new(7, TASKS_SCHEMA, false),
            ],
        )
    }

    pub async fn pending(
        &self,
        database: &Database,
    ) -> Result<Vec<PendingMigration>, MigrationError> {
        self.validate_catalog()?;
        self.ensure_metadata_table(database).await?;
        let applied = self.validate_applied(database).await?;

        Ok(self
            .migrations
            .iter()
            .filter(|migration| !applied.contains(&migration.version))
            .map(|migration| PendingMigration {
                version: migration.version,
                checksum: migration.checksum(),
                destructive: migration.destructive,
            })
            .collect())
    }

    fn validate_catalog(&self) -> Result<(), MigrationError> {
        for migrations in self.migrations.windows(2) {
            if migrations[0].version == migrations[1].version {
                return Err(MigrationError::DuplicateVersion {
                    version: migrations[0].version,
                });
            }
        }
        Ok(())
    }

    pub async fn run(&self, database: &Database) -> Result<(), MigrationError> {
        let pending = self.pending(database).await?;
        for pending_migration in pending {
            let migration = self
                .migrations
                .iter()
                .find(|migration| migration.version == pending_migration.version)
                .expect("pending migrations come from the runner's migration list");
            let mut transaction = database.transaction().await?;
            sqlx::raw_sql(migration.sql)
                .execute(&mut *transaction)
                .await
                .map_err(|source| MigrationError::Apply {
                    version: migration.version,
                    source,
                })?;
            sqlx::query(
                "INSERT INTO schema_migrations \
                 (version, checksum, applied_at, app_version) VALUES (?, ?, ?, ?)",
            )
            .bind(migration.version)
            .bind(&pending_migration.checksum)
            .bind(TimestampMillis::now().as_millis())
            .bind(&self.app_version)
            .execute(&mut *transaction)
            .await
            .map_err(|source| MigrationError::Apply {
                version: migration.version,
                source,
            })?;
            transaction
                .commit()
                .await
                .map_err(|source| MigrationError::Apply {
                    version: migration.version,
                    source,
                })?;
        }
        Ok(())
    }

    async fn ensure_metadata_table(&self, database: &Database) -> Result<(), sqlx::Error> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS schema_migrations (\
                version INTEGER PRIMARY KEY,\
                checksum TEXT NOT NULL,\
                applied_at INTEGER NOT NULL,\
                app_version TEXT NOT NULL\
            )",
        )
        .execute(database.pool())
        .await?;
        Ok(())
    }

    async fn validate_applied(&self, database: &Database) -> Result<Vec<i64>, MigrationError> {
        let rows = sqlx::query("SELECT version, checksum FROM schema_migrations ORDER BY version")
            .fetch_all(database.pool())
            .await?;
        let binary_version = self
            .migrations
            .last()
            .map_or(0, |migration| migration.version);
        let database_version = rows.last().map_or(0, |row| row.get("version"));
        if database_version > binary_version {
            return Err(MigrationError::SchemaNewer {
                database_version,
                binary_version,
            });
        }

        let mut applied = Vec::with_capacity(rows.len());
        for row in rows {
            let version = row.get::<i64, _>("version");
            let actual = row.get::<String, _>("checksum");
            let migration = self
                .migrations
                .iter()
                .find(|migration| migration.version == version)
                .ok_or(MigrationError::UnknownMigration { version })?;
            let expected = migration.checksum();
            if actual != expected {
                return Err(MigrationError::ChecksumMismatch {
                    version,
                    expected,
                    actual,
                });
            }
            applied.push(version);
        }
        Ok(applied)
    }
}
