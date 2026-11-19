//! The write lock hierarchy on Postgres, seen from two servers (two `Database` handles) that
//! share one database.

use std::pin::pin;
use std::time::Duration;

use orbit_platform::{Database, DatabaseConfig, Id, TestDatabase, WriteLockMode};
use sqlx::{Any, Connection, Transaction};
use tokio::time::timeout;

/// Long enough for an uncontended begin, short enough to keep the file fast.
const BLOCKED: Duration = Duration::from_millis(200);
const RELEASED: Duration = Duration::from_secs(5);

/// Two servers on a fresh Postgres database, dropped by [`Servers::close`].
struct Servers {
    first: Database,
    second: Database,
    admin: sqlx::PgConnection,
    name: String,
    _directory: tempfile::TempDir,
}

impl Servers {
    /// `None` without `ORBIT_TEST_POSTGRES_URL`.
    async fn open(mode: WriteLockMode) -> Option<Self> {
        let server = std::env::var("ORBIT_TEST_POSTGRES_URL")
            .ok()
            .filter(|url| !url.is_empty())?;
        let directory = tempfile::tempdir().unwrap();
        let name = format!("orbit_t_lock_{}", Id::new_v7().to_string().replace('-', ""));
        let mut admin = sqlx::PgConnection::connect(&server).await.unwrap();
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE DATABASE {name}")))
            .execute(&mut admin)
            .await
            .unwrap();
        let (base, _) = server.rsplit_once('/').unwrap();
        let config = DatabaseConfig {
            postgres_url: Some(format!("{base}/{name}")),
            write_lock: mode,
            ..DatabaseConfig::new(directory.path().join("orbit.sqlite"))
        }
        .shared();
        Some(Self {
            first: Database::open(&config).await.unwrap(),
            second: Database::open(&config).await.unwrap(),
            admin,
            name,
            _directory: directory,
        })
    }

    async fn close(self) {
        let Self {
            first,
            second,
            mut admin,
            name,
            ..
        } = self;
        drop((first, second));
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
            "DROP DATABASE {name} WITH (FORCE)"
        )))
        .execute(&mut admin)
        .await
        .unwrap();
    }
}

#[tokio::test]
async fn writers_of_two_workspaces_run_at_the_same_time() {
    let Some(servers) = Servers::open(WriteLockMode::Workspace).await else {
        return; // needs a Postgres server
    };
    let a = servers
        .first
        .workspace_transaction(Id::new_v7())
        .await
        .unwrap();

    let b = timeout(BLOCKED, servers.second.workspace_transaction(Id::new_v7()))
        .await
        .expect("workspace b does not wait for workspace a")
        .unwrap();

    drop((a, b));
    servers.close().await;
}

/// Asserts that `waiter` does not begin while `holder` is open, and begins once it commits.
async fn begins_after<'a>(
    holder: Transaction<'_, Any>,
    waiter: impl Future<Output = Result<Transaction<'a, Any>, sqlx::Error>>,
    what: &str,
) -> Transaction<'a, Any> {
    let mut waiter = pin!(waiter);
    assert!(
        timeout(BLOCKED, waiter.as_mut()).await.is_err(),
        "{what}: begins while the holder is open"
    );
    holder.commit().await.unwrap();
    timeout(RELEASED, waiter)
        .await
        .unwrap_or_else(|_| panic!("{what}: still waits after the holder committed"))
        .unwrap()
}

#[tokio::test]
async fn writers_of_one_workspace_take_turns() {
    let Some(servers) = Servers::open(WriteLockMode::Workspace).await else {
        return; // needs a Postgres server
    };
    let workspace = Id::new_v7();
    let a = servers
        .first
        .workspace_transaction(workspace)
        .await
        .unwrap();

    let b = begins_after(a, servers.second.workspace_transaction(workspace), "b").await;

    drop(b);
    servers.close().await;
}

#[tokio::test]
async fn a_global_writer_and_a_workspace_writer_exclude_each_other() {
    let Some(servers) = Servers::open(WriteLockMode::Workspace).await else {
        return; // needs a Postgres server
    };
    let workspace = servers
        .first
        .workspace_transaction(Id::new_v7())
        .await
        .unwrap();

    let global = begins_after(
        workspace,
        servers.second.immediate_transaction(),
        "global after workspace",
    )
    .await;
    let workspace = begins_after(
        global,
        servers.first.workspace_transaction(Id::new_v7()),
        "workspace after global",
    )
    .await;

    drop(workspace);
    servers.close().await;
}

#[tokio::test]
async fn the_job_queue_has_its_own_lock_under_the_global_one() {
    let Some(servers) = Servers::open(WriteLockMode::Workspace).await else {
        return; // needs a Postgres server
    };
    let jobs = servers.first.jobs_transaction().await.unwrap();

    let workspace = timeout(BLOCKED, servers.second.workspace_transaction(Id::new_v7()))
        .await
        .expect("a workspace writer does not wait for the job queue")
        .unwrap();
    workspace.commit().await.unwrap();
    let jobs = begins_after(jobs, servers.second.jobs_transaction(), "jobs after jobs").await;
    let global = begins_after(
        jobs,
        servers.first.immediate_transaction(),
        "global after jobs",
    )
    .await;

    drop(global);
    servers.close().await;
}

#[tokio::test]
async fn mode_global_serializes_every_writer() {
    let Some(servers) = Servers::open(WriteLockMode::Global).await else {
        return; // needs a Postgres server
    };
    let a = servers
        .first
        .workspace_transaction(Id::new_v7())
        .await
        .unwrap();

    let b = begins_after(
        a,
        servers.second.workspace_transaction(Id::new_v7()),
        "workspace b after workspace a",
    )
    .await;
    let jobs = begins_after(
        b,
        servers.first.jobs_transaction(),
        "jobs after workspace b",
    )
    .await;

    drop(jobs);
    servers.close().await;
}

/// Without the workspace lock, two writers read the same value and one increment is lost.
#[tokio::test]
async fn workspace_writers_on_two_servers_lose_no_update() {
    const WRITERS: i64 = 20;
    let Some(servers) = Servers::open(WriteLockMode::Workspace).await else {
        return; // needs a Postgres server
    };
    servers
        .first
        .execute("CREATE TABLE write_lock_counter (id BIGINT PRIMARY KEY, value BIGINT NOT NULL)")
        .await
        .unwrap();
    servers
        .first
        .execute("INSERT INTO write_lock_counter (id, value) VALUES (1, 0)")
        .await
        .unwrap();
    let workspace = Id::new_v7();

    let writers = (0..WRITERS).map(|index| {
        let database = if index % 2 == 0 {
            servers.first.clone()
        } else {
            servers.second.clone()
        };
        tokio::spawn(async move {
            let mut transaction = database.workspace_transaction(workspace).await?;
            let value: i64 =
                sqlx::query_scalar("SELECT value FROM write_lock_counter WHERE id = 1")
                    .fetch_one(&mut *transaction)
                    .await?;
            tokio::time::sleep(Duration::from_millis(1)).await;
            sqlx::query("UPDATE write_lock_counter SET value = $1 WHERE id = 1")
                .bind(value + 1)
                .execute(&mut *transaction)
                .await?;
            transaction.commit().await
        })
    });
    for result in futures_util::future::join_all(writers).await {
        result.unwrap().unwrap();
    }

    let value: i64 = sqlx::query_scalar("SELECT value FROM write_lock_counter WHERE id = 1")
        .fetch_one(servers.first.pool())
        .await
        .unwrap();
    assert_eq!(value, WRITERS);
    servers.close().await;
}

#[tokio::test]
async fn a_write_transaction_names_its_call_site() {
    let Some(servers) = Servers::open(WriteLockMode::Workspace).await else {
        return; // needs a Postgres server
    };
    let global = servers.first.immediate_transaction().await.unwrap();
    let global = application_name(global).await;
    let workspace = servers
        .first
        .workspace_transaction(Id::new_v7())
        .await
        .unwrap();
    let workspace = application_name(workspace).await;
    let jobs = servers.first.jobs_transaction().await.unwrap();
    let jobs = application_name(jobs).await;

    for name in [global, workspace, jobs] {
        assert!(
            name.starts_with("orbit:") && name.contains("tests/write_lock.rs:"),
            "{name}"
        );
    }
    servers.close().await;
}

async fn application_name(mut transaction: Transaction<'_, Any>) -> String {
    let name = sqlx::query_scalar("SELECT current_setting('application_name')")
        .fetch_one(&mut *transaction)
        .await
        .unwrap();
    transaction.commit().await.unwrap();
    name
}

/// SQLite has one writer for the whole file, so two workspaces still take turns.
#[tokio::test]
async fn sqlite_serializes_writers_of_two_workspaces() {
    if orbit_platform::testing_postgres() {
        return; // SQLite's file-wide write lock
    }
    let database = TestDatabase::new().await.unwrap();
    let a = database.workspace_transaction(Id::new_v7()).await.unwrap();

    let b = begins_after(
        a,
        database.workspace_transaction(Id::new_v7()),
        "workspace b after workspace a",
    )
    .await;
    drop(b);
}
