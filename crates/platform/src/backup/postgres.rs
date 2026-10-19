//! Postgres snapshots: the rows of every table, written with `COPY` inside one `REPEATABLE READ`
//! transaction (so all tables show the same moment), and restored into a schema rebuilt from the
//! migrations of the snapshot's version. Postgres runs DDL in transactions, so a restore either
//! replaces the whole database or changes nothing.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use futures_util::TryStreamExt;
use serde::{Deserialize, Serialize};
use sqlx::postgres::{PgPool, Postgres};
use sqlx::{Row, Transaction};
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;

use super::{BackupError, check_cancelled, io_error};
use crate::{Backend, Migration, db::metadata_table_sql};

/// The snapshot directory with one `<table>.copy` file per table and [`TABLES_FILE`].
pub(super) const DATABASE_DIRECTORY: &str = "database";
const TABLES_FILE: &str = "tables.json";

/// The columns written for a table: every column except generated ones, which Postgres computes.
#[derive(Debug, Deserialize, Serialize)]
struct TableCopy {
    table: String,
    columns: Vec<String>,
}

pub(super) async fn dump(
    pool: &PgPool,
    directory: &Path,
    cancellation: &CancellationToken,
) -> Result<(), BackupError> {
    super::create_dir_all(directory)?;
    let mut transaction = pool
        .begin_with("BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .await
        .map_err(BackupError::Postgres)?;
    let tables = tables(&mut transaction).await?;
    for table in &tables {
        check_cancelled(cancellation)?;
        let path = directory.join(format!("{}.copy", table.table));
        let mut file = tokio::fs::File::create(&path)
            .await
            .map_err(|source| io_error(&path, source))?;
        let mut rows = transaction
            .copy_out_raw(&format!(
                "COPY {} ({}) TO STDOUT",
                quote(&table.table),
                column_list(&table.columns)
            ))
            .await
            .map_err(BackupError::Postgres)?;
        while let Some(chunk) = rows.try_next().await.map_err(BackupError::Postgres)? {
            file.write_all(&chunk)
                .await
                .map_err(|source| io_error(&path, source))?;
        }
        drop(rows);
        file.sync_all()
            .await
            .map_err(|source| io_error(&path, source))?;
    }
    transaction.commit().await.map_err(BackupError::Postgres)?;

    let path = directory.join(TABLES_FILE);
    let contents = serde_json::to_vec_pretty(&tables).map_err(|source| BackupError::Manifest {
        path: path.clone(),
        source,
    })?;
    tokio::fs::write(&path, contents)
        .await
        .map_err(|source| io_error(&path, source))
}

/// Replaces every table in `pool` with the snapshot in `directory` and returns the transaction
/// uncommitted, so the caller can switch the attachments before it commits.
pub(super) async fn restore(
    pool: &PgPool,
    directory: &Path,
    schema_version: i64,
    migrations: &[Migration],
) -> Result<Transaction<'static, Postgres>, BackupError> {
    let path = directory.join(TABLES_FILE);
    let contents = std::fs::read(&path).map_err(|source| io_error(&path, source))?;
    let copies: Vec<TableCopy> =
        serde_json::from_slice(&contents).map_err(|source| BackupError::Manifest {
            path: path.clone(),
            source,
        })?;

    let mut transaction = pool.begin().await.map_err(BackupError::Postgres)?;
    sqlx::raw_sql("DROP SCHEMA public CASCADE; CREATE SCHEMA public;")
        .execute(&mut *transaction)
        .await
        .map_err(BackupError::Postgres)?;
    sqlx::raw_sql(sqlx::AssertSqlSafe(metadata_table_sql(Backend::Postgres)))
        .execute(&mut *transaction)
        .await
        .map_err(BackupError::Postgres)?;
    // The migration rows themselves come back with the `schema_migrations` table.
    for migration in migrations
        .iter()
        .filter(|migration| migration.version <= schema_version)
    {
        sqlx::raw_sql(migration.sql)
            .execute(&mut *transaction)
            .await
            .map_err(BackupError::Postgres)?;
    }

    let schema: BTreeMap<String, TableCopy> = tables(&mut transaction)
        .await?
        .into_iter()
        .map(|table| (table.table.clone(), table))
        .collect();
    for copy in &copies {
        if schema.get(&copy.table).map(|table| &table.columns) != Some(&copy.columns) {
            return Err(BackupError::InvalidSnapshot(format!(
                "table {} does not match the schema of version {schema_version}",
                copy.table
            )));
        }
    }

    // The migrations seed rows (instance settings, the collab generation) that the snapshot has.
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "TRUNCATE {}",
        schema
            .keys()
            .map(|table| quote(table))
            .collect::<Vec<_>>()
            .join(", ")
    )))
    .execute(&mut *transaction)
    .await
    .map_err(BackupError::Postgres)?;

    // User triggers would rewrite the copied rows (versions, search rows, events); foreign keys
    // stay on, so tables are loaded parents first.
    let order = load_order(&mut transaction, &copies).await?;
    set_user_triggers(&mut transaction, &order, "DISABLE").await?;
    for copy in &order {
        let path = directory.join(format!("{}.copy", copy.table));
        let file = tokio::fs::File::open(&path)
            .await
            .map_err(|source| io_error(&path, source))?;
        let mut sink = transaction
            .copy_in_raw(&format!(
                "COPY {} ({}) FROM STDIN",
                quote(&copy.table),
                column_list(&copy.columns)
            ))
            .await
            .map_err(BackupError::Postgres)?;
        sink.read_from(file).await.map_err(BackupError::Postgres)?;
        sink.finish().await.map_err(BackupError::Postgres)?;
    }
    // Deferred keys (the workspace owner) are checked now: a table with pending checks cannot
    // get its triggers back.
    sqlx::raw_sql("SET CONSTRAINTS ALL IMMEDIATE")
        .execute(&mut *transaction)
        .await
        .map_err(BackupError::Postgres)?;
    set_user_triggers(&mut transaction, &order, "ENABLE").await?;

    let identities = sqlx::query(
        "SELECT c.relname AS table_name, a.attname AS column_name FROM pg_attribute a \
         JOIN pg_class c ON c.oid = a.attrelid JOIN pg_namespace n ON n.oid = c.relnamespace \
         WHERE n.nspname = 'public' AND a.attidentity <> '' AND NOT a.attisdropped",
    )
    .fetch_all(&mut *transaction)
    .await
    .map_err(BackupError::Postgres)?;
    for row in identities {
        let (table, column): (String, String) = (row.get("table_name"), row.get("column_name"));
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT setval(pg_get_serial_sequence($1, $2), COALESCE(MAX({column}), 0) + 1, false) \
             FROM {table}",
            column = quote(&column),
            table = quote(&table),
        )))
        .bind(quote(&table))
        .bind(&column)
        .execute(&mut *transaction)
        .await
        .map_err(BackupError::Postgres)?;
    }

    // Like the SQLite restore: clients still hold newer collaborative documents, so every
    // document gets a new epoch and they reset instead of pushing them back.
    sqlx::raw_sql(
        "UPDATE page_collab_meta SET generation = md5(random()::text); \
         UPDATE page_collab_docs SET epoch = md5(random()::text); \
         UPDATE task_collab_docs SET epoch = md5(random()::text);",
    )
    .execute(&mut *transaction)
    .await
    .map_err(BackupError::CollabEpochs)?;
    Ok(transaction)
}

/// The tables of the `public` schema with their non-generated columns, by name.
async fn tables(
    transaction: &mut Transaction<'static, Postgres>,
) -> Result<Vec<TableCopy>, BackupError> {
    let rows = sqlx::query(
        "SELECT c.relname AS table_name, a.attname AS column_name FROM pg_class c \
         JOIN pg_namespace n ON n.oid = c.relnamespace JOIN pg_attribute a ON a.attrelid = c.oid \
         WHERE n.nspname = 'public' AND c.relkind = 'r' AND a.attnum > 0 \
           AND NOT a.attisdropped AND a.attgenerated = '' \
         ORDER BY c.relname, a.attnum",
    )
    .fetch_all(&mut **transaction)
    .await
    .map_err(BackupError::Postgres)?;
    let mut tables: Vec<TableCopy> = Vec::new();
    for row in rows {
        let (table, column): (String, String) = (row.get("table_name"), row.get("column_name"));
        match tables.last_mut() {
            Some(last) if last.table == table => last.columns.push(column),
            _ => tables.push(TableCopy {
                table,
                columns: vec![column],
            }),
        }
    }
    Ok(tables)
}

/// `copies` with every table after the tables its immediate foreign keys reference. Deferred
/// keys (the workspace owner cycle) are checked at commit and self-references at the end of the
/// `COPY`, so neither orders tables.
async fn load_order<'a>(
    transaction: &mut Transaction<'static, Postgres>,
    copies: &'a [TableCopy],
) -> Result<Vec<&'a TableCopy>, BackupError> {
    let edges = sqlx::query(
        "SELECT c.relname AS child, p.relname AS parent FROM pg_constraint k \
         JOIN pg_class c ON c.oid = k.conrelid JOIN pg_class p ON p.oid = k.confrelid \
         JOIN pg_namespace n ON n.oid = c.relnamespace \
         WHERE n.nspname = 'public' AND k.contype = 'f' AND NOT k.condeferrable \
           AND k.conrelid <> k.confrelid",
    )
    .fetch_all(&mut **transaction)
    .await
    .map_err(BackupError::Postgres)?;
    let mut parents: BTreeMap<&str, BTreeSet<String>> = copies
        .iter()
        .map(|copy| (copy.table.as_str(), BTreeSet::new()))
        .collect();
    for edge in edges {
        let child: String = edge.get("child");
        if let Some(set) = parents.get_mut(child.as_str()) {
            set.insert(edge.get("parent"));
        }
    }
    let mut order = Vec::with_capacity(copies.len());
    let mut loaded = BTreeSet::new();
    while order.len() < copies.len() {
        let ready: Vec<&TableCopy> = copies
            .iter()
            .filter(|copy| !loaded.contains(copy.table.as_str()))
            .filter(|copy| {
                parents[copy.table.as_str()].iter().all(|parent| {
                    loaded.contains(parent.as_str()) || !parents.contains_key(parent.as_str())
                })
            })
            .collect();
        if ready.is_empty() {
            return Err(BackupError::InvalidSnapshot(
                "tables reference each other through immediate foreign keys".to_owned(),
            ));
        }
        for copy in ready {
            loaded.insert(copy.table.as_str());
            order.push(copy);
        }
    }
    Ok(order)
}

/// `action` is `ENABLE` or `DISABLE`.
async fn set_user_triggers(
    transaction: &mut Transaction<'static, Postgres>,
    tables: &[&TableCopy],
    action: &str,
) -> Result<(), BackupError> {
    for table in tables {
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
            "ALTER TABLE {} {action} TRIGGER USER",
            quote(&table.table)
        )))
        .execute(&mut **transaction)
        .await
        .map_err(BackupError::Postgres)?;
    }
    Ok(())
}

fn quote(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

fn column_list(columns: &[String]) -> String {
    columns
        .iter()
        .map(|column| quote(column))
        .collect::<Vec<_>>()
        .join(", ")
}
