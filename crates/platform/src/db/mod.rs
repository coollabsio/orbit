mod builder;
mod migrate;
mod row;
mod test_db;

use std::any::Any;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use fs2::FileExt;
use sha2::{Digest, Sha256};
use sqlx::any::AnyPoolOptions;
use sqlx::postgres::{PgConnectOptions, PgConnection, PgPool, PgPoolOptions};
use sqlx::sqlite::{SqliteConnectOptions, SqliteConnection, SqliteJournalMode};
use sqlx::{AnyPool, Connection, Decode, Transaction, Type};
use thiserror::Error;

use crate::TimestampMillis;

pub use builder::{Separated, SqlBuilder};
pub(crate) use migrate::metadata_table_sql;
pub use migrate::{
    GuardedMigrationError, Migration, MigrationError, MigrationRunner, PendingMigration,
    run_guarded_migrations,
};
pub use row::{Column, Flag, NotBool, Row};
pub use test_db::{TestDatabase, TestDatabaseError};

/// Server URL (e.g. `postgres://orbit:orbit@127.0.0.1:15432/orbit`) that makes tests run on
/// Postgres; see [`Database::open`].
const TEST_POSTGRES_ENV: &str = "ORBIT_TEST_POSTGRES_URL";
/// Advisory lock held by the connection that owns a Postgres database, for the whole process.
const POSTGRES_OWNERSHIP_LOCK: &str = "SELECT pg_try_advisory_lock(8030594652542119799)";
/// True when the database orders and folds text like SQLite: code-point order (BINARY) and
/// Unicode case folding, also in the text search parser.
const POSTGRES_LOCALE_CHECK: &str = "SELECT 'B' < 'a' AND lower('Ü') = 'ü' AND to_tsvector('simple', 'Ü') = to_tsvector('simple', 'ü')";
/// How Orbit creates (or needs) a Postgres database: see `POSTGRES_LOCALE_CHECK`.
const POSTGRES_DATABASE_OPTIONS: &str = "TEMPLATE template0 ENCODING 'UTF8' \
    LOCALE_PROVIDER builtin BUILTIN_LOCALE 'C.UTF-8' LOCALE 'C.UTF-8'";
/// Transaction-scoped advisory lock taken by every write transaction on Postgres, so writers run
/// one at a time as under SQLite's `BEGIN IMMEDIATE`.
const POSTGRES_WRITE_LOCK: &str = "SELECT 1 FROM pg_advisory_xact_lock(8030594652542121842)";

/// Whether tests run on Postgres (`ORBIT_TEST_POSTGRES_URL` is set), for tests that only make
/// sense on SQLite (files, PRAGMAs, old SQLite migrations).
#[must_use]
pub fn testing_postgres() -> bool {
    test_postgres_url().is_some()
}

fn test_postgres_url() -> Option<String> {
    std::env::var(TEST_POSTGRES_ENV)
        .ok()
        .filter(|url| !url.is_empty())
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Backend {
    #[default]
    Sqlite,
    Postgres,
}

#[derive(Clone, Eq, PartialEq)]
pub struct DatabaseConfig {
    /// The SQLite database. With `postgres_url` it is not opened, but still anchors files that
    /// live next to the database (e.g. `app.key`).
    pub path: PathBuf,
    /// Opens this Postgres database instead of the SQLite file at `path`.
    pub postgres_url: Option<String>,
    pub max_connections: u32,
    pub busy_timeout: Duration,
}

impl DatabaseConfig {
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            postgres_url: None,
            max_connections: 5,
            busy_timeout: Duration::from_secs(5),
        }
    }

    /// The database of `[data]`: Postgres when `database_url` is set, else the SQLite file.
    #[must_use]
    pub fn for_data(data: &crate::DataConfig) -> Self {
        Self {
            postgres_url: data.database_url.clone(),
            ..Self::new(&data.database)
        }
    }
}

impl fmt::Debug for DatabaseConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DatabaseConfig")
            .field("path", &self.path)
            .field(
                "postgres_url",
                &self.postgres_url.as_deref().map(crate::redact_database_url),
            )
            .field("max_connections", &self.max_connections)
            .field("busy_timeout", &self.busy_timeout)
            .finish()
    }
}

#[derive(Clone, Debug)]
pub struct Database {
    inner: Arc<DatabaseInner>,
}

#[derive(Debug)]
struct DatabaseInner {
    pool: AnyPool,
    backend: Backend,
    /// The native pool of a Postgres database, for `COPY` (backups), which the Any driver lacks.
    postgres: Option<PgPool>,
    path: PathBuf,
    _ownership: Ownership,
    extensions: Extensions,
}

/// What keeps other Orbit processes from opening the database while this one runs.
#[derive(Debug)]
enum Ownership {
    /// The `<database>.lock` file next to the SQLite database.
    File { _lock: File },
    /// The connection that holds the ownership advisory lock, and in tests on Postgres also the
    /// lock file of the SQLite path the database stands in for.
    Postgres {
        _connection: Mutex<PgConnection>,
        _file: Option<File>,
    },
}

/// Process-wide services bound to one open database (at most one value per type), e.g. the
/// server's in-memory co-editing hub, which must be shared by every repository of the database.
#[derive(Default)]
struct Extensions(Mutex<Vec<Arc<dyn Any + Send + Sync>>>);

impl fmt::Debug for Extensions {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Extensions")
    }
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
    #[error("database file has more than one hard link, which SQLite does not support: {path}")]
    HardLinked { path: PathBuf },
    #[error("failed to open SQLite database {path}: {source}")]
    OpenDatabase {
        path: PathBuf,
        #[source]
        source: sqlx::Error,
    },
    /// `url` has its password redacted.
    #[error("failed to open Postgres database {url}: {source}")]
    OpenPostgres {
        url: String,
        #[source]
        source: sqlx::Error,
    },
    /// `url` has its password redacted.
    #[error(
        "Postgres database {url} does not order and fold text like Orbit needs; create it with \
         CREATE DATABASE <name> TEMPLATE template0 ENCODING 'UTF8' LOCALE_PROVIDER builtin \
         BUILTIN_LOCALE 'C.UTF-8' LOCALE 'C.UTF-8'"
    )]
    PostgresLocale { url: String },
}

/// Takes the lock that makes this process the only owner of the database at `path`, creating an
/// empty database file if none exists. The lock is a separate `<database>.lock` file next to the
/// resolved database path: on macOS a `flock` on the database file itself also blocks SQLite's own
/// `fcntl` locks, so SQLite fails with "database is locked" (code 5). Symlink aliases resolve to the
/// same lock file; hard links are refused, because SQLite does not support them.
pub fn lock_database_ownership(path: &Path) -> Result<File, DatabaseError> {
    let open_error = |source| DatabaseError::OpenOwnershipLock {
        path: path.to_owned(),
        source,
    };
    // An existing file is not opened: closing a file descriptor drops this process's `fcntl`
    // locks on that file, including the locks of SQLite connections that are open on it.
    let database_path = match fs::canonicalize(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(false)
                .open(path)
                .map_err(open_error)?;
            fs::canonicalize(path)
        }
        result => result,
    }
    .map_err(open_error)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;

        if fs::metadata(&database_path).map_err(open_error)?.nlink() > 1 {
            return Err(DatabaseError::HardLinked {
                path: path.to_owned(),
            });
        }
    }

    let mut lock_path = database_path.into_os_string();
    lock_path.push(".lock");
    let lock_path = PathBuf::from(lock_path);
    let ownership_lock = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(|source| DatabaseError::OpenOwnershipLock {
            path: lock_path,
            source,
        })?;
    ownership_lock
        .try_lock_exclusive()
        .map_err(|_| DatabaseError::AlreadyOwned {
            path: path.to_owned(),
        })?;
    Ok(ownership_lock)
}

impl Database {
    /// Opens the SQLite database at `config.path`, or the Postgres database of
    /// `config.postgres_url`.
    ///
    /// Test-only switch: when `ORBIT_TEST_POSTGRES_URL` names a Postgres server and the config is
    /// SQLite-only, the SQLite path is mapped to its own database on that server,
    /// `orbit_t_<first 20 hex digits of sha256(absolute path)>`, so every existing test runs on
    /// Postgres unchanged. A path that did not exist before this open gets a fresh database; an
    /// existing one keeps its data, like a reopened SQLite file.
    pub async fn open(config: &DatabaseConfig) -> Result<Self, DatabaseError> {
        sqlx::any::install_default_drivers();
        if let Some(parent) = config.path.parent() {
            fs::create_dir_all(parent).map_err(|source| DatabaseError::CreateDirectory {
                path: parent.to_owned(),
                source,
            })?;
        }

        if let Some(url) = &config.postgres_url {
            return Self::open_postgres(config, url, None).await;
        }
        if let Some(server) = test_postgres_url() {
            let existed = config.path.exists();
            let ownership_lock = lock_database_ownership(&config.path)?;
            let url = create_test_database(&server, &config.path, !existed)
                .await
                .map_err(|source| DatabaseError::OpenPostgres {
                    url: crate::redact_database_url(&server),
                    source,
                })?;
            return Self::open_postgres(config, &url, Some(ownership_lock)).await;
        }
        Self::open_sqlite(config).await
    }

    async fn open_sqlite(config: &DatabaseConfig) -> Result<Self, DatabaseError> {
        let ownership_lock = lock_database_ownership(&config.path)?;
        let open_error = |source| DatabaseError::OpenDatabase {
            path: config.path.clone(),
            source,
        };

        let options = SqliteConnectOptions::new()
            .filename(&config.path)
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(config.busy_timeout);
        // The Any driver only takes a URL. A native connection creates the file and switches it to
        // WAL (a persistent mode); the per-connection settings are applied after connecting.
        SqliteConnection::connect_with(&options)
            .await
            .map_err(open_error)?
            .close()
            .await
            .map_err(open_error)?;
        let absolute =
            fs::canonicalize(&config.path).map_err(|source| DatabaseError::OpenOwnershipLock {
                path: config.path.clone(),
                source,
            })?;
        let url = format!("sqlite://{}?mode=rwc", percent_encode_path(&absolute));
        let busy_timeout = config.busy_timeout.as_millis();
        let pool = AnyPoolOptions::new()
            .max_connections(config.max_connections)
            .after_connect(move |connection, _| {
                Box::pin(async move {
                    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
                        "PRAGMA foreign_keys = ON; PRAGMA busy_timeout = {busy_timeout};"
                    )))
                    .execute(connection)
                    .await?;
                    Ok(())
                })
            })
            .connect(&url)
            .await
            .map_err(open_error)?;

        Ok(Self {
            inner: Arc::new(DatabaseInner {
                pool,
                backend: Backend::Sqlite,
                postgres: None,
                path: config.path.clone(),
                _ownership: Ownership::File {
                    _lock: ownership_lock,
                },
                extensions: Extensions::default(),
            }),
        })
    }

    async fn open_postgres(
        config: &DatabaseConfig,
        url: &str,
        ownership_file: Option<File>,
    ) -> Result<Self, DatabaseError> {
        let open_error = |source| DatabaseError::OpenPostgres {
            url: crate::redact_database_url(url),
            source,
        };
        let options = PgConnectOptions::from_str(url).map_err(open_error)?;

        // A closed owner's session can take a moment to end on the server, so a reopen retries
        // briefly before it reports the database as owned.
        let mut connection = PgConnection::connect_with(&options)
            .await
            .map_err(open_error)?;
        let mut attempts = 0;
        loop {
            let owned: bool = sqlx::query_scalar(POSTGRES_OWNERSHIP_LOCK)
                .fetch_one(&mut connection)
                .await
                .map_err(open_error)?;
            if owned {
                break;
            }
            attempts += 1;
            if attempts == 20 {
                return Err(DatabaseError::AlreadyOwned {
                    path: config.path.clone(),
                });
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let locale_ok: bool = sqlx::query_scalar(POSTGRES_LOCALE_CHECK)
            .fetch_one(&mut connection)
            .await
            .map_err(open_error)?;
        if !locale_ok {
            return Err(DatabaseError::PostgresLocale {
                url: crate::redact_database_url(url),
            });
        }

        let postgres = PgPoolOptions::new()
            .max_connections(config.max_connections)
            .connect_with(options)
            .await
            .map_err(open_error)?;
        let pool = AnyPoolOptions::new()
            .max_connections(config.max_connections)
            .connect(url)
            .await
            .map_err(open_error)?;

        Ok(Self {
            inner: Arc::new(DatabaseInner {
                pool,
                backend: Backend::Postgres,
                postgres: Some(postgres),
                path: config.path.clone(),
                _ownership: Ownership::Postgres {
                    _connection: Mutex::new(connection),
                    _file: ownership_file,
                },
                extensions: Extensions::default(),
            }),
        })
    }

    #[must_use]
    pub fn backend(&self) -> Backend {
        self.inner.backend
    }

    pub async fn execute(&self, query: &str) -> Result<u64, sqlx::Error> {
        // Callers are operator tools and tests that pass trusted SQL.
        Ok(sqlx::raw_sql(sqlx::AssertSqlSafe(query.to_owned()))
            .execute(&self.inner.pool)
            .await?
            .rows_affected())
    }

    pub async fn scalar<T>(&self, query: &str) -> Result<T, sqlx::Error>
    where
        T: for<'row> Decode<'row, sqlx::Any> + Type<sqlx::Any> + Send + Unpin,
    {
        sqlx::query_scalar(sqlx::AssertSqlSafe(query.to_owned()))
            .fetch_one(&self.inner.pool)
            .await
    }

    /// The SQLite database path; on Postgres the configured path that files such as `app.key`
    /// live next to.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.inner.path
    }

    pub async fn transaction(&self) -> Result<Transaction<'_, sqlx::Any>, sqlx::Error> {
        self.inner.pool.begin().await
    }

    /// A write transaction. Writers run one at a time on both backends: SQLite takes the write
    /// lock with `BEGIN IMMEDIATE`, Postgres a transaction-scoped advisory lock, so
    /// read-then-write logic sees no concurrent writer.
    pub async fn immediate_transaction(&self) -> Result<Transaction<'_, sqlx::Any>, sqlx::Error> {
        match self.inner.backend {
            Backend::Sqlite => self.inner.pool.begin_with("BEGIN IMMEDIATE").await,
            Backend::Postgres => {
                let mut transaction = self.inner.pool.begin().await?;
                sqlx::query(POSTGRES_WRITE_LOCK)
                    .execute(&mut *transaction)
                    .await?;
                Ok(transaction)
            }
        }
    }

    /// Returns UTC wall time as observed by the database, at millisecond precision.
    pub async fn database_now(&self) -> Result<TimestampMillis, sqlx::Error> {
        let query = match self.inner.backend {
            Backend::Sqlite => {
                "SELECT CAST(strftime('%s', 'now') AS INTEGER) * 1000 \
                 + CAST(substr(strftime('%f', 'now'), 4, 3) AS INTEGER)"
            }
            Backend::Postgres => "SELECT (EXTRACT(EPOCH FROM clock_timestamp()) * 1000)::BIGINT",
        };
        let milliseconds = sqlx::query_scalar(query)
            .fetch_one(&self.inner.pool)
            .await?;
        Ok(TimestampMillis::from_millis(milliseconds))
    }

    pub fn pool(&self) -> &AnyPool {
        &self.inner.pool
    }

    pub(crate) fn postgres_pool(&self) -> Option<&PgPool> {
        self.inner.postgres.as_ref()
    }

    /// The database's extension of type `T`, created by `init` on first use. Extensions live as
    /// long as the database (they must not hold a `Database` clone, or it is never closed).
    pub fn extension<T: Any + Send + Sync>(&self, init: impl FnOnce() -> T) -> Arc<T> {
        let mut extensions = self
            .inner
            .extensions
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(existing) = extensions
            .iter()
            .find_map(|extension| Arc::clone(extension).downcast::<T>().ok())
        {
            return existing;
        }
        let created = Arc::new(init());
        extensions.push(Arc::clone(&created) as Arc<dyn Any + Send + Sync>);
        created
    }

    /// The database's extension of type `T`, if one was created.
    #[must_use]
    pub fn existing_extension<T: Any + Send + Sync>(&self) -> Option<Arc<T>> {
        self.inner
            .extensions
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .find_map(|extension| Arc::clone(extension).downcast::<T>().ok())
    }
}

/// Percent-encodes a path for a `sqlite://` URL; sqlx decodes it again (spaces, `?`, `#`, `%`
/// and non-ASCII characters survive).
fn percent_encode_path(path: &Path) -> String {
    let mut encoded = String::new();
    for &byte in path.as_os_str().as_encoded_bytes() {
        if byte.is_ascii_alphanumeric() || b"/-._~".contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

/// The test database that stands in for the SQLite file at `path` (see [`Database::open`]).
pub(crate) fn test_postgres_database(path: &Path) -> Option<(String, String)> {
    let server = test_postgres_url()?;
    let absolute = fs::canonicalize(path).ok()?;
    let digest = format!(
        "{:x}",
        Sha256::digest(absolute.as_os_str().as_encoded_bytes())
    );
    Some((server, format!("orbit_t_{}", &digest[..20])))
}

/// Creates (if `fresh`, recreates) the test database for `path` and returns its URL.
async fn create_test_database(
    server: &str,
    path: &Path,
    fresh: bool,
) -> Result<String, sqlx::Error> {
    let (_, name) = test_postgres_database(path).ok_or_else(|| {
        sqlx::Error::Configuration(format!("cannot resolve {}", path.display()).into())
    })?;
    let mut connection = PgConnection::connect(server).await?;
    if fresh {
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
            "DROP DATABASE IF EXISTS {name} WITH (FORCE)"
        )))
        .execute(&mut connection)
        .await?;
    }
    let created = sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "CREATE DATABASE {name} {POSTGRES_DATABASE_OPTIONS}"
    )))
    .execute(&mut connection)
    .await;
    match created {
        Err(sqlx::Error::Database(error)) if error.code().as_deref() == Some("42P04") => {}
        result => {
            result?;
        }
    }
    connection.close().await?;

    let mut url =
        url::Url::parse(server).map_err(|error| sqlx::Error::Configuration(error.into()))?;
    url.set_path(&name);
    Ok(url.into())
}

/// Drops the test database of `path`, if tests run on Postgres.
pub(crate) async fn drop_test_database(path: &Path) -> Result<(), sqlx::Error> {
    let Some((server, name)) = test_postgres_database(path) else {
        return Ok(());
    };
    let mut connection = PgConnection::connect(&server).await?;
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "DROP DATABASE IF EXISTS {name} WITH (FORCE)"
    )))
    .execute(&mut connection)
    .await?;
    connection.close().await
}
