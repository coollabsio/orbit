mod migrate;
mod test_db;

use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use fs2::FileExt;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::{Decode, Sqlite, SqlitePool, Transaction, Type};
use thiserror::Error;

use crate::TimestampMillis;

pub use migrate::{Migration, MigrationError, MigrationRunner, PendingMigration};
pub use test_db::{TestDatabase, TestDatabaseError};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DatabaseConfig {
    pub path: PathBuf,
    pub max_connections: u32,
    pub busy_timeout: Duration,
}

impl DatabaseConfig {
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            max_connections: 5,
            busy_timeout: Duration::from_secs(5),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Database {
    inner: Arc<DatabaseInner>,
}

#[derive(Debug)]
struct DatabaseInner {
    pool: SqlitePool,
    path: PathBuf,
    _ownership_lock: File,
}

#[derive(Debug, Error)]
pub enum DatabaseError {
    #[error("failed to create database directory {path}: {source}")]
    CreateDirectory {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to open database ownership lock {path}: {source}")]
    OpenOwnershipLock {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("database is already owned by another Orbit process: {path}")]
    AlreadyOwned { path: PathBuf },
    #[error("failed to open SQLite database {path}: {source}")]
    OpenDatabase {
        path: PathBuf,
        #[source]
        source: sqlx::Error,
    },
}

impl Database {
    pub async fn open(config: &DatabaseConfig) -> Result<Self, DatabaseError> {
        if let Some(parent) = config.path.parent() {
            fs::create_dir_all(parent).map_err(|source| DatabaseError::CreateDirectory {
                path: parent.to_owned(),
                source,
            })?;
        }

        let ownership_lock = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&config.path)
            .map_err(|source| DatabaseError::OpenOwnershipLock {
                path: config.path.clone(),
                source,
            })?;
        ownership_lock
            .try_lock_exclusive()
            .map_err(|_| DatabaseError::AlreadyOwned {
                path: config.path.clone(),
            })?;

        let options = SqliteConnectOptions::new()
            .filename(&config.path)
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(config.busy_timeout);
        let pool = SqlitePoolOptions::new()
            .max_connections(config.max_connections)
            .connect_with(options)
            .await
            .map_err(|source| DatabaseError::OpenDatabase {
                path: config.path.clone(),
                source,
            })?;

        Ok(Self {
            inner: Arc::new(DatabaseInner {
                pool,
                path: config.path.clone(),
                _ownership_lock: ownership_lock,
            }),
        })
    }

    pub async fn execute(&self, query: &str) -> Result<u64, sqlx::Error> {
        Ok(sqlx::raw_sql(query)
            .execute(&self.inner.pool)
            .await?
            .rows_affected())
    }

    pub async fn scalar<T>(&self, query: &str) -> Result<T, sqlx::Error>
    where
        T: for<'row> Decode<'row, Sqlite> + Type<Sqlite> + Send + Unpin,
    {
        sqlx::query_scalar(query).fetch_one(&self.inner.pool).await
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.inner.path
    }

    pub async fn transaction(&self) -> Result<Transaction<'_, Sqlite>, sqlx::Error> {
        self.inner.pool.begin().await
    }

    pub(crate) async fn immediate_transaction(
        &self,
    ) -> Result<Transaction<'_, Sqlite>, sqlx::Error> {
        self.inner.pool.begin_with("BEGIN IMMEDIATE").await
    }

    /// Returns UTC wall time as observed by SQLite, at millisecond precision.
    pub async fn database_now(&self) -> Result<TimestampMillis, sqlx::Error> {
        let milliseconds = sqlx::query_scalar(
            "SELECT CAST(strftime('%s', 'now') AS INTEGER) * 1000 \
             + CAST(substr(strftime('%f', 'now'), 4, 3) AS INTEGER)",
        )
        .fetch_one(&self.inner.pool)
        .await?;
        Ok(TimestampMillis::from_millis(milliseconds))
    }

    pub(crate) fn pool(&self) -> &SqlitePool {
        &self.inner.pool
    }
}
