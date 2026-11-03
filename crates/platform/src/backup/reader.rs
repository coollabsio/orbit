//! Reads rows out of a verified snapshot without restoring it: an SQLite `database.sqlite`
//! opened read-only, or the `COPY` files of a Postgres snapshot. Columns are picked by name and
//! every value comes back as text, so callers do not depend on the column order or the types of
//! the snapshot's schema version. Rows stream through a filter; only the kept rows are held.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use futures_util::TryStreamExt;
use serde::Deserialize;
use sqlx::sqlite::{SqliteConnectOptions, SqliteConnection};
use sqlx::{ConnectOptions, Row};
use tokio::io::{AsyncBufReadExt, BufReader};

use super::{BackupError, BackupSnapshot, DATABASE_FILE, io_error, postgres};
use crate::Backend;

const TABLES_FILE: &str = "tables.json";

/// One row of a snapshot table: the selected columns, by name.
#[derive(Clone, Debug)]
pub struct SnapshotRow {
    columns: Arc<[String]>,
    values: Vec<Option<String>>,
}

impl SnapshotRow {
    /// The text of `column`; `None` for SQL `NULL` or a column that was not selected.
    #[must_use]
    pub fn get(&self, column: &str) -> Option<&str> {
        let index = self.columns.iter().position(|name| name == column)?;
        self.values[index].as_deref()
    }
}

#[derive(Debug, Deserialize)]
struct TableCopy {
    table: String,
    columns: Vec<String>,
}

#[derive(Debug)]
enum Source {
    Sqlite(PathBuf),
    /// The `database/` directory and the columns of each table, in file order.
    Postgres {
        directory: PathBuf,
        tables: HashMap<String, Vec<String>>,
    },
}

#[derive(Debug)]
pub struct SnapshotReader {
    source: Source,
}

impl SnapshotReader {
    /// A reader of `snapshot`, which the caller has verified. Nothing in the snapshot is written.
    pub fn open(snapshot: &BackupSnapshot) -> Result<Self, BackupError> {
        let source = match snapshot.manifest.database {
            Backend::Sqlite => {
                let path = snapshot.path.join(DATABASE_FILE);
                if !path.is_file() {
                    return Err(BackupError::InvalidSnapshot(
                        "the snapshot has no SQLite database".to_owned(),
                    ));
                }
                Source::Sqlite(path)
            }
            Backend::Postgres => {
                let directory = snapshot.path.join(postgres::DATABASE_DIRECTORY);
                let path = directory.join(TABLES_FILE);
                let contents = std::fs::read(&path).map_err(|source| io_error(&path, source))?;
                let copies: Vec<TableCopy> = serde_json::from_slice(&contents)
                    .map_err(|source| BackupError::Manifest { path, source })?;
                Source::Postgres {
                    directory,
                    tables: copies
                        .into_iter()
                        .map(|copy| (copy.table, copy.columns))
                        .collect(),
                }
            }
        };
        Ok(Self { source })
    }

    /// The columns of `table`, or `None` when the snapshot has no such table.
    pub async fn columns(&self, table: &str) -> Result<Option<Vec<String>>, BackupError> {
        match &self.source {
            Source::Postgres { tables, .. } => Ok(tables.get(table).cloned()),
            Source::Sqlite(path) => {
                let mut connection = connect(path).await?;
                let columns: Vec<String> =
                    sqlx::query_scalar("SELECT name FROM pragma_table_info($1) ORDER BY cid")
                        .bind(table)
                        .fetch_all(&mut connection)
                        .await
                        .map_err(BackupError::SnapshotRead)?;
                Ok((!columns.is_empty()).then_some(columns))
            }
        }
    }

    /// The rows of `table` that `keep` accepts, with `columns` selected by name. A table or
    /// column the snapshot does not have is [`BackupError::InvalidSnapshot`]; check optional
    /// columns with [`Self::columns`] first.
    pub async fn rows<F>(
        &self,
        table: &str,
        columns: &[&str],
        mut keep: F,
    ) -> Result<Vec<SnapshotRow>, BackupError>
    where
        F: FnMut(&SnapshotRow) -> bool,
    {
        let available = self.columns(table).await?.ok_or_else(|| {
            BackupError::InvalidSnapshot(format!("the snapshot has no table {table}"))
        })?;
        let mut indexes = Vec::with_capacity(columns.len());
        for column in columns {
            let index = available
                .iter()
                .position(|name| name == column)
                .ok_or_else(|| {
                    BackupError::InvalidSnapshot(format!(
                        "table {table} of the snapshot has no column {column}"
                    ))
                })?;
            indexes.push(index);
        }
        let names: Arc<[String]> = columns.iter().map(|&column| column.to_owned()).collect();
        let mut kept = Vec::new();
        match &self.source {
            Source::Sqlite(path) => {
                let mut connection = connect(path).await?;
                let sql = format!(
                    "SELECT {} FROM {}",
                    columns
                        .iter()
                        .map(|column| format!("CAST({} AS TEXT)", quote(column)))
                        .collect::<Vec<_>>()
                        .join(", "),
                    quote(table)
                );
                let mut stream = sqlx::query(sqlx::AssertSqlSafe(sql)).fetch(&mut connection);
                while let Some(row) = stream.try_next().await.map_err(BackupError::SnapshotRead)? {
                    let values = (0..columns.len())
                        .map(|index| row.try_get::<Option<String>, _>(index))
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(BackupError::SnapshotRead)?;
                    let row = SnapshotRow {
                        columns: Arc::clone(&names),
                        values,
                    };
                    if keep(&row) {
                        kept.push(row);
                    }
                }
            }
            Source::Postgres { directory, .. } => {
                let file = postgres::open_table(directory, table).await?;
                let mut lines = BufReader::new(file);
                let mut line = Vec::new();
                loop {
                    line.clear();
                    let read = lines
                        .read_until(b'\n', &mut line)
                        .await
                        .map_err(|source| io_error(directory.join(table), source))?;
                    if read == 0 {
                        break;
                    }
                    if line.last() == Some(&b'\n') {
                        line.pop();
                    }
                    // The end-of-data marker of older `COPY` output.
                    if line == b"\\." {
                        break;
                    }
                    let fields: Vec<&[u8]> = line.split(|&byte| byte == b'\t').collect();
                    if fields.len() != available.len() {
                        return Err(BackupError::InvalidSnapshot(format!(
                            "a row of table {table} has {} fields, expected {}",
                            fields.len(),
                            available.len()
                        )));
                    }
                    let row = SnapshotRow {
                        columns: Arc::clone(&names),
                        values: indexes
                            .iter()
                            .map(|&index| copy_field(fields[index]))
                            .collect(),
                    };
                    if keep(&row) {
                        kept.push(row);
                    }
                }
            }
        }
        Ok(kept)
    }
}

/// A read-only connection that never writes to the file (no journal, no WAL recovery).
async fn connect(path: &std::path::Path) -> Result<SqliteConnection, BackupError> {
    SqliteConnectOptions::new()
        .filename(path)
        .read_only(true)
        .immutable(true)
        .connect()
        .await
        .map_err(BackupError::SnapshotRead)
}

fn quote(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

/// One field of Postgres `COPY ... TO STDOUT` text output: `\N` is `NULL`; backslash escapes
/// `\b \f \n \r \t \v \\`, octal `\NNN` and hex `\xHH` give bytes; any other escaped character
/// stands for itself. Invalid UTF-8 is replaced, as the server writes UTF-8.
#[must_use]
pub(crate) fn copy_field(field: &[u8]) -> Option<String> {
    if field == b"\\N" {
        return None;
    }
    let mut bytes = Vec::with_capacity(field.len());
    let mut index = 0;
    while index < field.len() {
        let byte = field[index];
        index += 1;
        if byte != b'\\' || index == field.len() {
            bytes.push(byte);
            continue;
        }
        let escaped = field[index];
        index += 1;
        match escaped {
            b'b' => bytes.push(0x08),
            b'f' => bytes.push(0x0c),
            b'n' => bytes.push(b'\n'),
            b'r' => bytes.push(b'\r'),
            b't' => bytes.push(b'\t'),
            b'v' => bytes.push(0x0b),
            b'0'..=b'7' => {
                let mut value = u32::from(escaped - b'0');
                for _ in 0..2 {
                    match field.get(index) {
                        Some(digit @ b'0'..=b'7') => {
                            value = value * 8 + u32::from(digit - b'0');
                            index += 1;
                        }
                        _ => break,
                    }
                }
                bytes.push((value & 0xff) as u8);
            }
            b'x' => {
                let mut value = 0_u32;
                let mut digits = 0;
                while digits < 2 {
                    match field
                        .get(index)
                        .and_then(|digit| (*digit as char).to_digit(16))
                    {
                        Some(digit) => {
                            value = value * 16 + digit;
                            index += 1;
                            digits += 1;
                        }
                        None => break,
                    }
                }
                if digits == 0 {
                    bytes.push(b'x');
                } else {
                    bytes.push(value as u8);
                }
            }
            other => bytes.push(other),
        }
    }
    Some(match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(error) => String::from_utf8_lossy(error.as_bytes()).into_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::copy_field;

    #[test]
    fn null_and_empty_strings_differ() {
        assert_eq!(copy_field(b"\\N"), None);
        assert_eq!(copy_field(b""), Some(String::new()));
        assert_eq!(copy_field(b"\\\\N"), Some("\\N".to_owned()));
    }

    #[test]
    fn backslash_escapes_decode() {
        assert_eq!(
            copy_field(b"a\\tb\\nc\\rd\\\\e\\bf\\fg\\vh"),
            Some("a\tb\nc\rd\\e\u{8}f\u{c}g\u{b}h".to_owned())
        );
        assert_eq!(copy_field(b"\\\""), Some("\"".to_owned()));
    }

    #[test]
    fn octal_and_hex_escapes_give_bytes() {
        assert_eq!(copy_field(b"\\101\\x42\\7"), Some("AB\u{7}".to_owned()));
        // UTF-8 written as escaped bytes: "é" is C3 A9.
        assert_eq!(copy_field(b"\\303\\251"), Some("é".to_owned()));
        assert_eq!(copy_field(b"\\xc3\\xA9"), Some("é".to_owned()));
        assert_eq!(copy_field(b"\\xg"), Some("xg".to_owned()));
    }

    #[test]
    fn plain_text_passes_through() {
        assert_eq!(
            copy_field("Grüße {\"a\":1}".as_bytes()),
            Some("Grüße {\"a\":1}".to_owned())
        );
        assert_eq!(copy_field(b"trailing\\"), Some("trailing\\".to_owned()));
    }
}
