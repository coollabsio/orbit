use std::ops::Deref;
use std::path::Path;

use tempfile::TempDir;
use thiserror::Error;

use super::{
    Database, DatabaseConfig, DatabaseError, MigrationError, MigrationRunner, drop_test_database,
    testing_postgres,
};

#[derive(Debug)]
pub struct TestDatabase {
    database: Database,
    _directory: TempDir,
}

#[derive(Debug, Error)]
pub enum TestDatabaseError {
    #[error("failed to create temporary database directory: {0}")]
    TemporaryDirectory(#[source] std::io::Error),
    #[error(transparent)]
    Database(#[from] DatabaseError),
    #[error(transparent)]
    Migration(#[from] MigrationError),
}

impl TestDatabase {
    pub async fn new() -> Result<Self, TestDatabaseError> {
        let directory = tempfile::tempdir().map_err(TestDatabaseError::TemporaryDirectory)?;
        let database =
            Database::open(&DatabaseConfig::new(directory.path().join("orbit.sqlite"))).await?;
        MigrationRunner::embedded(env!("CARGO_PKG_VERSION"))
            .run(&database)
            .await?;

        Ok(Self {
            database,
            _directory: directory,
        })
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        self.database.path()
    }
}

/// On Postgres the stand-in database is dropped with the temporary directory, so test runs do
/// not pile up databases on the server. Runs on its own thread and runtime, because `drop` may be
/// called inside or outside of a runtime.
impl Drop for TestDatabase {
    fn drop(&mut self) {
        if !testing_postgres() {
            return;
        }
        let path = self.database.path().to_owned();
        let dropped = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(sqlx::Error::Io)?;
            runtime.block_on(drop_test_database(&path))
        })
        .join();
        if let Ok(Err(error)) = dropped {
            tracing::warn!(%error, "failed to drop the Postgres test database");
        }
    }
}

impl Deref for TestDatabase {
    type Target = Database;

    fn deref(&self) -> &Self::Target {
        &self.database
    }
}
