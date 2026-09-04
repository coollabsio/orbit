use std::ops::Deref;
use std::path::Path;

use tempfile::TempDir;
use thiserror::Error;

use super::{Database, DatabaseConfig, DatabaseError, MigrationError, MigrationRunner};

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

impl Deref for TestDatabase {
    type Target = Database;

    fn deref(&self) -> &Self::Target {
        &self.database
    }
}
