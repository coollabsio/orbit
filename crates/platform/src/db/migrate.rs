use sha2::{Digest, Sha256};
use sqlx::Row;
use thiserror::Error;

use super::{Backend, Database};
use crate::{BackupError, BackupService, Config, TimestampMillis};

/// The catalog that `build.rs` makes from `apps/server/migrations/`.
mod catalog {
    include!(concat!(env!("OUT_DIR"), "/migrations.rs"));
}

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

/// Runs the migration catalog of the database's backend.
#[derive(Debug)]
pub struct MigrationRunner {
    app_version: String,
    sqlite: Vec<Migration>,
    postgres: Vec<Migration>,
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

#[derive(Debug, Error)]
pub enum GuardedMigrationError {
    #[error(transparent)]
    Migration(#[from] MigrationError),
    #[error("pre-migration backup failed: {0}")]
    Backup(#[from] BackupError),
}

pub async fn run_guarded_migrations(
    config: &Config,
    database: &Database,
    runner: &MigrationRunner,
) -> Result<usize, GuardedMigrationError> {
    // Servers that start together apply each migration (and its backup) once.
    let lock = database
        .lock_migrations()
        .await
        .map_err(MigrationError::from)?;
    let result = run_pending_migrations(config, database, runner).await;
    lock.release().await.map_err(MigrationError::from)?;
    result
}

async fn run_pending_migrations(
    config: &Config,
    database: &Database,
    runner: &MigrationRunner,
) -> Result<usize, GuardedMigrationError> {
    let pending = runner.pending(database).await?;
    if pending.iter().any(|migration| migration.destructive) {
        let service = BackupService::new(&config.data.backups, &config.data.attachments);
        let backup = service.create_pre_migration(database).await?;
        service.verify(&backup.id).await?;
    }
    let count = pending.len();
    runner.run(database).await?;
    Ok(count)
}

impl MigrationRunner {
    /// A runner with the same `migrations` for both backends.
    #[must_use]
    pub fn new(app_version: impl Into<String>, mut migrations: Vec<Migration>) -> Self {
        migrations.sort_by_key(|migration| migration.version);
        Self {
            app_version: app_version.into(),
            sqlite: migrations.clone(),
            postgres: migrations,
        }
    }

    #[must_use]
    pub fn embedded(app_version: impl Into<String>) -> Self {
        let migrations = |entries: &[(i64, &'static str, bool)]| {
            entries
                .iter()
                .map(|&(version, sql, destructive)| Migration::new(version, sql, destructive))
                .collect::<Vec<_>>()
        };
        Self {
            app_version: app_version.into(),
            sqlite: migrations(catalog::SQLITE),
            postgres: migrations(catalog::POSTGRES),
        }
    }

    /// The newest migration version that this binary knows.
    #[must_use]
    pub fn latest_version(&self) -> i64 {
        self.sqlite.last().map_or(0, |migration| migration.version)
    }

    /// The embedded catalog up to and including `last_version`, for testing a migration
    /// against data written under the schema that precedes it.
    #[must_use]
    pub fn embedded_through(app_version: impl Into<String>, last_version: i64) -> Self {
        let mut runner = Self::embedded(app_version);
        runner
            .sqlite
            .retain(|migration| migration.version <= last_version);
        runner
            .postgres
            .retain(|migration| migration.version <= last_version);
        runner
    }

    /// The Postgres catalog, which a Postgres backup restore replays before it loads the rows.
    pub(crate) fn postgres_migrations(&self) -> &[Migration] {
        &self.postgres
    }

    fn migrations(&self, database: &Database) -> &[Migration] {
        match database.backend() {
            Backend::Sqlite => &self.sqlite,
            Backend::Postgres => &self.postgres,
        }
    }

    pub async fn pending(
        &self,
        database: &Database,
    ) -> Result<Vec<PendingMigration>, MigrationError> {
        let migrations = self.migrations(database);
        validate_catalog(migrations)?;
        ensure_metadata_table(database).await?;
        let applied = validate_applied(database, migrations).await?;

        Ok(migrations
            .iter()
            .filter(|migration| !applied.contains(&migration.version))
            .map(|migration| PendingMigration {
                version: migration.version,
                checksum: migration.checksum(),
                destructive: migration.destructive,
            })
            .collect())
    }

    pub async fn run(&self, database: &Database) -> Result<(), MigrationError> {
        let pending = self.pending(database).await?;
        for pending_migration in pending {
            let migration = self
                .migrations(database)
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
                 (version, checksum, applied_at, app_version) VALUES ($1, $2, $3, $4)",
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
}

fn validate_catalog(migrations: &[Migration]) -> Result<(), MigrationError> {
    for pair in migrations.windows(2) {
        if pair[0].version == pair[1].version {
            return Err(MigrationError::DuplicateVersion {
                version: pair[0].version,
            });
        }
    }
    Ok(())
}

async fn ensure_metadata_table(database: &Database) -> Result<(), sqlx::Error> {
    sqlx::query(sqlx::AssertSqlSafe(metadata_table_sql(database.backend())))
        .execute(database.pool())
        .await?;
    Ok(())
}

pub(crate) fn metadata_table_sql(backend: Backend) -> String {
    let integer = match backend {
        Backend::Sqlite => "INTEGER",
        Backend::Postgres => "BIGINT",
    };
    format!(
        "CREATE TABLE IF NOT EXISTS schema_migrations (\
            version {integer} PRIMARY KEY,\
            checksum TEXT NOT NULL,\
            applied_at {integer} NOT NULL,\
            app_version TEXT NOT NULL\
        )"
    )
}

async fn validate_applied(
    database: &Database,
    migrations: &[Migration],
) -> Result<Vec<i64>, MigrationError> {
    let rows = sqlx::query("SELECT version, checksum FROM schema_migrations ORDER BY version")
        .fetch_all(database.pool())
        .await?;
    let binary_version = migrations.last().map_or(0, |migration| migration.version);
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
        let migration = migrations
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
