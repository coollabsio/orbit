//! Postgres snapshots: the rows of every table, written with `COPY` inside one `REPEATABLE READ`
//! transaction (so all tables show the same moment), and restored into a schema rebuilt from the
//! migrations of the snapshot's version. Each table file is zstd-compressed; snapshots written
//! before that hold plain `<table>.copy` files, which restore still reads. Postgres runs DDL in transactions, so a restore either
//! replaces the whole database or changes nothing.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use async_compression::tokio::{bufread::ZstdDecoder, write::ZstdEncoder};
use futures_util::TryStreamExt;
use serde::{Deserialize, Serialize};
use sqlx::postgres::{PgPool, Postgres};
use sqlx::{Row, Transaction};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio_util::sync::CancellationToken;

use super::crypto::{DataKey, Opener, Sealer};
use super::{BackupError, check_cancelled, io_error};
use crate::{Backend, Migration, db::metadata_table_sql};

/// The snapshot directory with one `<table>.copy.zst` file per table and [`TABLES_FILE`].
pub(super) const DATABASE_DIRECTORY: &str = "database";
const TABLES_FILE: &str = "tables.json";

/// The columns written for a table: every column except generated ones, which Postgres computes.
#[derive(Debug, Deserialize, Serialize)]
struct TableCopy {
    table: String,
    columns: Vec<String>,
}

/// The path of a file of this directory in the snapshot, the associated data of its encryption.
fn snapshot_path(name: &str) -> String {
    format!("{DATABASE_DIRECTORY}/{name}")
}

/// With `key`, every file is encrypted as it is written.
pub(super) async fn dump(
    pool: &PgPool,
    directory: &Path,
    key: Option<&DataKey>,
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
        let name = format!("{}.copy.zst", table.table);
        let path = directory.join(&name);
        let mut file = tokio::fs::File::create(&path)
            .await
            .map_err(|source| io_error(&path, source))?;
        // compressed first, then encrypted: the zstd output is drained after each chunk
        let mut encoder = ZstdEncoder::new(Vec::new());
        let mut sealer = key.map(|key| key.sealer(&snapshot_path(&name)));
        let mut sealed = Vec::new();
        let mut rows = transaction
            .copy_out_raw(&format!(
                "COPY {} ({}) TO STDOUT",
                quote(&table.table),
                column_list(&table.columns)
            ))
            .await
            .map_err(BackupError::Postgres)?;
        while let Some(chunk) = rows.try_next().await.map_err(BackupError::Postgres)? {
            encoder
                .write_all(&chunk)
                .await
                .map_err(|source| io_error(&path, source))?;
            let compressed = std::mem::take(encoder.get_mut());
            write_sealed(&mut file, &path, sealer.as_mut(), &compressed, &mut sealed).await?;
        }
        drop(rows);
        encoder
            .shutdown()
            .await
            .map_err(|source| io_error(&path, source))?;
        let rest = std::mem::take(encoder.get_mut());
        write_sealed(&mut file, &path, sealer.as_mut(), &rest, &mut sealed).await?;
        if let Some(sealer) = sealer {
            sealed.clear();
            sealer.finish(&mut sealed)?;
            file.write_all(&sealed)
                .await
                .map_err(|source| io_error(&path, source))?;
        }
        file.sync_all()
            .await
            .map_err(|source| io_error(&path, source))?;
    }
    transaction.commit().await.map_err(BackupError::Postgres)?;

    let path = directory.join(TABLES_FILE);
    let mut contents =
        serde_json::to_vec_pretty(&tables).map_err(|source| BackupError::Manifest {
            path: path.clone(),
            source,
        })?;
    if let Some(key) = key {
        let mut sealer = key.sealer(&snapshot_path(TABLES_FILE));
        let mut sealed = Vec::new();
        sealer.update(&contents, &mut sealed)?;
        sealer.finish(&mut sealed)?;
        contents = sealed;
    }
    tokio::fs::write(&path, contents)
        .await
        .map_err(|source| io_error(&path, source))
}

/// Replaces every table in `pool` with the snapshot in `directory` and returns the transaction
/// uncommitted, so the caller can switch the attachments before it commits.
/// The schema version that added `task_collab_docs` (migration 0052).
const TASK_COLLAB_SCHEMA_VERSION: i64 = 52;

pub(super) async fn restore(
    pool: &PgPool,
    directory: &Path,
    schema_version: i64,
    migrations: &[Migration],
    key: Option<&DataKey>,
) -> Result<Transaction<'static, Postgres>, BackupError> {
    let path = directory.join(TABLES_FILE);
    let mut contents = std::fs::read(&path).map_err(|source| io_error(&path, source))?;
    if let Some(key) = key {
        let mut opener = key.opener(&snapshot_path(TABLES_FILE));
        let mut opened = Vec::new();
        opener.update(&contents, &mut opened)?;
        opener.finish(&mut opened)?;
        contents = opened;
    }
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
        let file = open_table(directory, &copy.table, key).await?;
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
         UPDATE page_collab_docs SET epoch = md5(random()::text);",
    )
    .execute(&mut *transaction)
    .await
    .map_err(BackupError::CollabEpochs)?;
    // Task descriptions are co-edited since schema 52; an older snapshot has no such table.
    if schema_version >= TASK_COLLAB_SCHEMA_VERSION {
        sqlx::raw_sql("UPDATE task_collab_docs SET epoch = md5(random()::text)")
            .execute(&mut *transaction)
            .await
            .map_err(BackupError::CollabEpochs)?;
    }
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

/// Writes `bytes` to `file`, sealed when the snapshot is encrypted.
async fn write_sealed(
    file: &mut tokio::fs::File,
    path: &Path,
    sealer: Option<&mut Sealer>,
    bytes: &[u8],
    sealed: &mut Vec<u8>,
) -> Result<(), BackupError> {
    let output = match sealer {
        Some(sealer) => {
            sealed.clear();
            sealer.update(bytes, sealed)?;
            &sealed[..]
        }
        None => bytes,
    };
    file.write_all(output)
        .await
        .map_err(|source| io_error(path, source))
}

/// The ciphertext of `file` decrypted as it is read, a piece at a time: a table is never held in
/// memory.
fn decrypting(file: tokio::fs::File, opener: Opener) -> impl AsyncRead + Unpin + Send {
    struct Reading {
        file: tokio::fs::File,
        opener: Opener,
        buffer: Vec<u8>,
    }
    let to_io = |error: BackupError| std::io::Error::other(error.to_string());
    let start = Some(Reading {
        file,
        opener,
        buffer: vec![0_u8; 256 * 1024],
    });
    let stream =
        futures_util::stream::try_unfold(start, move |state: Option<Reading>| async move {
            let Some(mut reading) = state else {
                return Ok::<_, std::io::Error>(None);
            };
            loop {
                let read = reading.file.read(&mut reading.buffer).await?;
                let mut opened = Vec::new();
                if read == 0 {
                    reading.opener.finish(&mut opened).map_err(to_io)?;
                    return Ok(Some((tokio_util::bytes::Bytes::from(opened), None)));
                }
                reading
                    .opener
                    .update(&reading.buffer[..read], &mut opened)
                    .map_err(to_io)?;
                if !opened.is_empty() {
                    return Ok(Some((
                        tokio_util::bytes::Bytes::from(opened),
                        Some(reading),
                    )));
                }
            }
        });
    tokio_util::io::StreamReader::new(Box::pin(stream))
}

/// The rows of `table`, decrypted (with `key`) and decompressed: `<table>.copy.zst`, or
/// `<table>.copy` in older snapshots.
pub(super) async fn open_table(
    directory: &Path,
    table: &str,
    key: Option<&DataKey>,
) -> Result<Box<dyn AsyncRead + Unpin + Send>, BackupError> {
    let compressed = directory.join(format!("{table}.copy.zst"));
    let (path, zstd) = if tokio::fs::try_exists(&compressed)
        .await
        .map_err(|source| io_error(&compressed, source))?
    {
        (compressed, true)
    } else {
        (directory.join(format!("{table}.copy")), false)
    };
    let file = tokio::fs::File::open(&path)
        .await
        .map_err(|source| io_error(&path, source))?;
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let plain: Box<dyn AsyncRead + Unpin + Send> = match key {
        Some(key) => Box::new(decrypting(file, key.opener(&snapshot_path(&name)))),
        None => Box::new(file),
    };
    Ok(if zstd {
        Box::new(ZstdDecoder::new(BufReader::new(plain)))
    } else {
        plain
    })
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
