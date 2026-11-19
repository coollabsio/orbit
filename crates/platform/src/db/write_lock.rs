//! The write locks of a Postgres database, and the monitor that reports who holds them too long.
//!
//! Two levels: every write transaction takes the global lock, exclusive for a
//! [`WriteScope::Global`] writer and shared for a [`WriteScope::Workspace`] or
//! [`WriteScope::Jobs`] writer, which then takes the one lock of its workspace or of the job
//! queue. Always in this order, and never two second-level locks, so two writers cannot deadlock
//! on them.

use std::collections::HashMap;
use std::panic::Location;
use std::str::FromStr;
use std::time::Duration;

use sqlx::{Any, AnyPool, Transaction};
use tokio_util::sync::CancellationToken;

use super::{Backend, Database, Singleton};
use crate::Id;

/// Bigint advisory key of the global write lock.
const GLOBAL_KEY: i64 = 8_030_594_652_542_121_842;
/// First key of the two-int advisory locks of workspaces (the second is `hashtext(<id>)`).
/// Postgres keeps two-int keys apart from bigint keys, so these never collide with `GLOBAL_KEY`.
const WORKSPACE_NAMESPACE: i32 = 0x4F52_4257;
/// First key of the two-int advisory lock of the job queue (the second is 0).
const JOBS_NAMESPACE: i32 = 0x4F52_424A;
/// A writer waits for a lock at most as long as the pool waits for a connection, then fails with
/// a transient lock timeout (55P03): waiting writers hold pool connections, so an unlimited wait
/// behind one slow writer starves every reader.
const LOCK_TIMEOUT: &str = "SET LOCAL lock_timeout = '30s'";
/// Postgres cuts `application_name` to this many bytes.
const APPLICATION_NAME_BYTES: usize = 63;

const MONITOR_SINGLETON: &str = "write-lock-monitor";
const MONITOR_POLL: Duration = Duration::from_secs(1);
const MONITOR_SINGLETON_RETRY: Duration = Duration::from_secs(30);
const MONITOR_SLOW_SECS: f64 = 2.0;

/// What a write transaction may touch, and so which writers it excludes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WriteScope {
    /// Excludes every other writer of the database.
    Global,
    /// Excludes the writers of this workspace and every `Global` writer.
    Workspace(Id),
    /// Excludes the other writers of the job queue and every `Global` writer. For the job store
    /// only: a worker's poll must not stop the writers of the workspaces.
    Jobs,
}

/// `ORBIT_WRITE_LOCK`: whether a workspace or job queue writer takes only its own lock, or the
/// global lock like every writer did before those locks existed.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum WriteLockMode {
    #[default]
    Workspace,
    Global,
}

impl FromStr for WriteLockMode {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "workspace" => Ok(Self::Workspace),
            "global" => Ok(Self::Global),
            _ => Err(()),
        }
    }
}

/// The statements that open a Postgres write transaction: the lock timeout, the call site as
/// `application_name` (for `pg_stat_activity` and the monitor) and the locks of `scope`.
/// `SET LOCAL` ends with the transaction.
#[must_use]
pub fn write_lock_sql(scope: WriteScope, mode: WriteLockMode, caller: &Location<'_>) -> String {
    let tag = call_site_tag(caller);
    let locks = match (scope, mode) {
        (WriteScope::Workspace(workspace_id), WriteLockMode::Workspace) => format!(
            "SELECT 1 FROM pg_advisory_xact_lock_shared({GLOBAL_KEY}); \
             SELECT 1 FROM pg_advisory_xact_lock({WORKSPACE_NAMESPACE}, hashtext('{workspace_id}'))"
        ),
        (WriteScope::Jobs, WriteLockMode::Workspace) => format!(
            "SELECT 1 FROM pg_advisory_xact_lock_shared({GLOBAL_KEY}); \
             SELECT 1 FROM pg_advisory_xact_lock({JOBS_NAMESPACE}, 0)"
        ),
        (WriteScope::Global, _)
        | (WriteScope::Workspace(_) | WriteScope::Jobs, WriteLockMode::Global) => {
            format!("SELECT 1 FROM pg_advisory_xact_lock({GLOBAL_KEY})")
        }
    };
    format!("{LOCK_TIMEOUT}; SET LOCAL application_name = '{tag}'; {locks}")
}

/// `orbit:<file>:<line>`, safe to put in a SQL string literal: only `[A-Za-z0-9_./:-]`, and the
/// end of the path when the name is too long.
fn call_site_tag(caller: &Location<'_>) -> String {
    const PREFIX: &str = "orbit:";
    let line = format!(":{}", caller.line());
    let file: String = caller
        .file()
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || "_./:-".contains(*character))
        .collect();
    let room = APPLICATION_NAME_BYTES - PREFIX.len() - line.len();
    let file = &file[file.len().saturating_sub(room)..];
    format!("{PREFIX}{file}{line}")
}

/// Begins a write transaction of `scope` on `pool`. For code that has the pool of a [`Database`]
/// but not the `Database`; other code calls [`Database::immediate_transaction`],
/// [`Database::workspace_transaction`] or [`Database::jobs_transaction`].
pub async fn begin_write_transaction(
    pool: &AnyPool,
    backend: Backend,
    mode: WriteLockMode,
    scope: WriteScope,
    caller: &'static Location<'static>,
) -> Result<Transaction<'static, Any>, sqlx::Error> {
    match backend {
        // SQLite has one writer for the whole file.
        Backend::Sqlite => pool.begin_with("BEGIN IMMEDIATE").await,
        Backend::Postgres => {
            let mut transaction = pool.begin().await?;
            // Built from constants, a typed `Id` and a filtered call site only.
            sqlx::raw_sql(sqlx::AssertSqlSafe(write_lock_sql(scope, mode, caller)))
                .execute(&mut *transaction)
                .await?;
            Ok(transaction)
        }
    }
}

/// One row per session that holds or waits for a write lock. A bigint advisory key shows in
/// `pg_locks` as `objsubid` 1 with its high half in `classid` and its low half in `objid`; a
/// two-int key as `objsubid` 2 with the first key in `classid`.
const MONITOR_QUERY: &str = "\
    SELECT a.pid, \
        (EXTRACT(EPOCH FROM a.xact_start) * 1000000)::bigint, \
        EXTRACT(EPOCH FROM clock_timestamp() - a.xact_start)::float8, \
        CASE WHEN bool_or(l.granted AND l.objsubid = 2 AND l.classid = $4::bigint::oid) THEN 'jobs' \
             WHEN bool_or(l.granted AND l.objsubid = 2) THEN 'workspace' \
             WHEN bool_or(l.granted AND l.mode = 'ShareLock') THEN 'shared' \
             ELSE 'exclusive' END, \
        bool_or(l.granted), \
        bool_or(NOT l.granted), \
        COALESCE(a.application_name, ''), \
        COALESCE(a.state, ''), \
        COALESCE(left(a.query, 120), '') \
    FROM pg_locks l \
    JOIN pg_stat_activity a ON a.pid = l.pid \
    WHERE l.locktype = 'advisory' \
        AND l.database = (SELECT oid FROM pg_database WHERE datname = current_database()) \
        AND ((l.objsubid = 1 AND l.classid = $1::bigint::oid AND l.objid = $2::bigint::oid) \
            OR (l.objsubid = 2 AND l.classid IN ($3::bigint::oid, $4::bigint::oid))) \
        AND a.xact_start IS NOT NULL \
    GROUP BY a.pid, a.xact_start, a.application_name, a.state, a.query";

type MonitorRow = (i32, i64, f64, String, bool, bool, String, String, String);

/// A slow write transaction that the monitor reported, kept to report its end.
struct Reported {
    application_name: String,
    mode: String,
    held_secs: f64,
}

/// Logs each write transaction that holds a write lock for more than two seconds, once, and its
/// duration when it ends. One server of the cluster runs it (the others wait for the singleton);
/// it ends at `shutdown` and never fails: an error is logged and the monitor starts again.
///
/// `held_secs` is the age of the transaction, so it includes the time the transaction waited for
/// its own locks. The final duration is the last one seen, at most one poll short.
pub async fn run_write_lock_monitor(database: Database, shutdown: CancellationToken) {
    if database.backend() != Backend::Postgres {
        return shutdown.cancelled().await;
    }
    loop {
        let watch = async {
            match database.try_singleton(MONITOR_SINGLETON).await {
                Ok(Some(singleton)) => watch_write_locks(singleton).await,
                Ok(None) => Ok(()),
                Err(error) => Err(error),
            }
        };
        tokio::select! {
            () = shutdown.cancelled() => return,
            result = watch => {
                if let Err(error) = result {
                    tracing::warn!(%error, "write lock monitor stopped; it will start again");
                }
            }
        }
        tokio::select! {
            () = shutdown.cancelled() => return,
            () = tokio::time::sleep(MONITOR_SINGLETON_RETRY) => {}
        }
    }
}

/// Polls on the singleton's own connection: when that connection is lost, so is the singleton,
/// and the poll fails instead of running next to another server's monitor.
async fn watch_write_locks(mut singleton: Singleton) -> Result<(), sqlx::Error> {
    let Some(connection) = singleton.0.as_mut() else {
        return Ok(());
    };
    sqlx::query("SET application_name = 'orbit:write-lock-monitor'")
        .execute(&mut **connection)
        .await?;
    let mut reported: HashMap<(i32, i64), Reported> = HashMap::new();
    loop {
        let rows: Vec<MonitorRow> = sqlx::query_as(MONITOR_QUERY)
            .bind(GLOBAL_KEY >> 32)
            .bind(GLOBAL_KEY & 0xFFFF_FFFF)
            .bind(i64::from(WORKSPACE_NAMESPACE))
            .bind(i64::from(JOBS_NAMESPACE))
            .fetch_all(&mut **connection)
            .await?;
        let waiters = rows.iter().filter(|row| row.5).count();
        let mut holders = Vec::new();
        for (pid, started, held_secs, mode, holds, _, application_name, state, query) in rows {
            if !holds {
                continue;
            }
            let key = (pid, started);
            holders.push(key);
            if let Some(known) = reported.get_mut(&key) {
                known.held_secs = held_secs;
            } else if held_secs >= MONITOR_SLOW_SECS {
                tracing::warn!(
                    application_name,
                    held_secs,
                    mode,
                    waiters,
                    state,
                    query,
                    pid,
                    "slow write transaction holds a write lock"
                );
                reported.insert(
                    key,
                    Reported {
                        application_name,
                        mode,
                        held_secs,
                    },
                );
            }
        }
        reported.retain(|key, known| {
            let holds = holders.contains(key);
            if !holds {
                tracing::warn!(
                    application_name = known.application_name,
                    held_secs = known.held_secs,
                    mode = known.mode,
                    pid = key.0,
                    "slow write transaction ended"
                );
            }
            holds
        });
        tokio::time::sleep(MONITOR_POLL).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_lock_sql_takes_the_locks_of_the_scope_in_order() {
        let caller = Location::caller();
        let tag = format!("orbit:{}:{}", caller.file(), caller.line());
        let prefix =
            format!("SET LOCAL lock_timeout = '30s'; SET LOCAL application_name = '{tag}'; ");
        let global = format!("{prefix}SELECT 1 FROM pg_advisory_xact_lock(8030594652542121842)");
        let workspace: Id = "018f4e4a-0000-7000-8000-000000000001".parse().unwrap();

        assert_eq!(
            write_lock_sql(WriteScope::Global, WriteLockMode::Workspace, caller),
            global
        );
        assert_eq!(
            write_lock_sql(
                WriteScope::Workspace(workspace),
                WriteLockMode::Global,
                caller
            ),
            global
        );
        assert_eq!(
            write_lock_sql(WriteScope::Jobs, WriteLockMode::Global, caller),
            global
        );
        assert_eq!(
            write_lock_sql(WriteScope::Jobs, WriteLockMode::Workspace, caller),
            format!(
                "{prefix}SELECT 1 FROM pg_advisory_xact_lock_shared(8030594652542121842); \
                 SELECT 1 FROM pg_advisory_xact_lock(1330790986, 0)"
            )
        );
        assert_eq!(
            write_lock_sql(
                WriteScope::Workspace(workspace),
                WriteLockMode::Workspace,
                caller
            ),
            format!(
                "{prefix}SELECT 1 FROM pg_advisory_xact_lock_shared(8030594652542121842); \
                 SELECT 1 FROM pg_advisory_xact_lock(1330790999, \
                 hashtext('018f4e4a-0000-7000-8000-000000000001'))"
            )
        );
    }
}
