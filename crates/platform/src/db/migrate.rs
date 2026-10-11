use sha2::{Digest, Sha256};
use sqlx::Row;
use thiserror::Error;

use super::{Backend, Database};
use crate::{BackupError, BackupService, Config, TimestampMillis};

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
const TASK_INTEGRITY_SCHEMA: &str =
    include_str!("../../../../apps/server/migrations/0008_task_integrity.sql");
const ATTACHMENT_INTEGRITY_SCHEMA: &str =
    include_str!("../../../../apps/server/migrations/0009_attachment_integrity.sql");
const TASK_DUE_DATE_SCHEMA: &str =
    include_str!("../../../../apps/server/migrations/0010_task_due_dates.sql");

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

/// The Postgres schema starts at the SQLite schema of the same version, in one baseline.
const POSTGRES_BASELINE: &str =
    include_str!("../../../../apps/server/migrations/postgres/0046_baseline.sql");

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
        let mut runner = Self::new(
            app_version,
            vec![
                Migration::new(1, PLATFORM_SCHEMA, false),
                Migration::new(2, IDENTITY_SCHEMA, false),
                Migration::new(3, ATTACHMENT_REFERENCE_SCHEMA, false),
                Migration::new(4, WORKSPACE_SCHEMA, false),
                Migration::new(5, WORKSPACE_INTEGRITY_SCHEMA, false),
                Migration::new(6, WORKSPACE_RETENTION_SECURITY_SCHEMA, false),
                Migration::new(7, TASKS_SCHEMA, false),
                Migration::new(8, TASK_INTEGRITY_SCHEMA, false),
                Migration::new(9, ATTACHMENT_INTEGRITY_SCHEMA, false),
                Migration::new(10, TASK_DUE_DATE_SCHEMA, false),
                Migration::new(
                    11,
                    include_str!("../../../../apps/server/migrations/0011_realtime.sql"),
                    false,
                ),
                Migration::new(
                    12,
                    include_str!("../../../../apps/server/migrations/0012_notifications.sql"),
                    false,
                ),
                Migration::new(
                    13,
                    include_str!("../../../../apps/server/migrations/0013_api_tokens.sql"),
                    false,
                ),
                Migration::new(
                    14,
                    include_str!("../../../../apps/server/migrations/0014_api_token_projects.sql"),
                    false,
                ),
                Migration::new(
                    15,
                    include_str!("../../../../apps/server/migrations/0015_discord_integration.sql"),
                    false,
                ),
                Migration::new(
                    16,
                    include_str!(
                        "../../../../apps/server/migrations/0016_api_token_projects_many.sql"
                    ),
                    false,
                ),
                Migration::new(
                    17,
                    include_str!("../../../../apps/server/migrations/0017_service_accounts.sql"),
                    false,
                ),
                Migration::new(
                    18,
                    include_str!(
                        "../../../../apps/server/migrations/0018_api_token_expiration.sql"
                    ),
                    false,
                ),
                Migration::new(
                    19,
                    include_str!(
                        "../../../../apps/server/migrations/0019_task_due_date_ranges.sql"
                    ),
                    false,
                ),
                Migration::new(
                    20,
                    include_str!("../../../../apps/server/migrations/0020_github_integration.sql"),
                    false,
                ),
                Migration::new(
                    21,
                    include_str!("../../../../apps/server/migrations/0021_task_relations.sql"),
                    true,
                ),
                Migration::new(
                    22,
                    include_str!("../../../../apps/server/migrations/0022_pages.sql"),
                    false,
                ),
                Migration::new(
                    23,
                    include_str!("../../../../apps/server/migrations/0023_page_spaces.sql"),
                    false,
                ),
                Migration::new(
                    24,
                    include_str!("../../../../apps/server/migrations/0024_page_favorites.sql"),
                    false,
                ),
                Migration::new(
                    25,
                    include_str!("../../../../apps/server/migrations/0025_page_files.sql"),
                    false,
                ),
                Migration::new(
                    26,
                    include_str!("../../../../apps/server/migrations/0026_notion_imports.sql"),
                    false,
                ),
                Migration::new(
                    27,
                    include_str!("../../../../apps/server/migrations/0027_page_search_fts.sql"),
                    false,
                ),
                Migration::new(
                    28,
                    include_str!("../../../../apps/server/migrations/0028_page_versions.sql"),
                    false,
                ),
                Migration::new(
                    29,
                    include_str!("../../../../apps/server/migrations/0029_page_collab.sql"),
                    false,
                ),
                Migration::new(
                    30,
                    include_str!("../../../../apps/server/migrations/0030_page_comments.sql"),
                    false,
                ),
                Migration::new(
                    31,
                    include_str!("../../../../apps/server/migrations/0031_page_options.sql"),
                    false,
                ),
                Migration::new(
                    32,
                    include_str!("../../../../apps/server/migrations/0032_page_mentions.sql"),
                    false,
                ),
                Migration::new(
                    33,
                    include_str!("../../../../apps/server/migrations/0033_views.sql"),
                    false,
                ),
                Migration::new(
                    34,
                    include_str!("../../../../apps/server/migrations/0034_sub_issues.sql"),
                    false,
                ),
                Migration::new(
                    35,
                    include_str!("../../../../apps/server/migrations/0035_mcp_oauth.sql"),
                    false,
                ),
                Migration::new(
                    36,
                    include_str!("../../../../apps/server/migrations/0036_keyboard_shortcuts.sql"),
                    false,
                ),
                Migration::new(
                    37,
                    include_str!("../../../../apps/server/migrations/0037_chat.sql"),
                    false,
                ),
                Migration::new(
                    38,
                    include_str!("../../../../apps/server/migrations/0038_user_avatars.sql"),
                    false,
                ),
                Migration::new(
                    39,
                    include_str!("../../../../apps/server/migrations/0039_task_numbers.sql"),
                    false,
                ),
                Migration::new(
                    40,
                    include_str!(
                        "../../../../apps/server/migrations/0040_presence_profiles_push.sql"
                    ),
                    false,
                ),
                Migration::new(
                    41,
                    include_str!(
                        "../../../../apps/server/migrations/0041_chat_replies_forwards_custom_emoji.sql"
                    ),
                    false,
                ),
                Migration::new(
                    42,
                    include_str!(
                        "../../../../apps/server/migrations/0042_instance_settings_email.sql"
                    ),
                    false,
                ),
                Migration::new(
                    43,
                    include_str!("../../../../apps/server/migrations/0043_root_user.sql"),
                    false,
                ),
                Migration::new(
                    44,
                    include_str!(
                        "../../../../apps/server/migrations/0044_instance_settings_storage.sql"
                    ),
                    false,
                ),
                Migration::new(
                    45,
                    include_str!("../../../../apps/server/migrations/0045_two_factor.sql"),
                    false,
                ),
                Migration::new(
                    46,
                    include_str!(
                        "../../../../apps/server/migrations/0046_images_in_blob_store.sql"
                    ),
                    false,
                ),
            ],
        );
        runner.postgres = vec![Migration::new(46, POSTGRES_BASELINE, false)];
        runner
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
