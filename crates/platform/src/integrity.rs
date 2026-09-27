use sqlx::{query, query_scalar};
use thiserror::Error;

use crate::{Database, HealthCheck, HealthRegistry};

#[derive(Clone, Debug)]
pub struct IntegrityService {
    database: Database,
}

#[derive(Debug, Error)]
pub enum IntegrityError {
    #[error("SQLite {check} failed: {details}")]
    Failed {
        check: &'static str,
        details: String,
    },
}

impl IntegrityService {
    #[must_use]
    pub fn new(database: Database) -> Self {
        Self { database }
    }

    pub async fn quick(&self) -> Result<(), IntegrityError> {
        self.run("quick_check").await
    }

    pub async fn full(&self) -> Result<(), IntegrityError> {
        self.integrity_check().await?;
        let violations = query("PRAGMA foreign_key_check")
            .fetch_all(self.database.pool())
            .await
            .map_err(|error| IntegrityError::Failed {
                check: "foreign_key_check",
                details: error.to_string(),
            })?;
        if violations.is_empty() {
            return Ok(());
        }
        Err(IntegrityError::Failed {
            check: "foreign_key_check",
            details: format!("{} violation(s)", violations.len()),
        })
    }

    pub fn register_readiness(&self, registry: &mut HealthRegistry) {
        let service = self.clone();
        registry.register(HealthCheck::DatabaseIntegrity, move || {
            let service = service.clone();
            async move { service.quick().await.map_err(|error| error.to_string()) }
        });
    }

    /// `PRAGMA integrity_check`, except that FTS5 index findings are confirmed with FTS5's own
    /// `integrity-check` command: the bundled SQLite 3.46.0 reports "malformed inverted index"
    /// for any FTS5 table whose rows were UPDATEd twice (or rebuilt), while the index is fine
    /// and FTS5's check passes. A really damaged FTS5 index fails that command.
    async fn integrity_check(&self) -> Result<(), IntegrityError> {
        let results: Vec<String> = query_scalar("PRAGMA integrity_check")
            .fetch_all(self.database.pool())
            .await
            .map_err(|error| IntegrityError::Failed {
                check: "integrity_check",
                details: error.to_string(),
            })?;
        if results.len() == 1 && results[0].eq_ignore_ascii_case("ok") {
            return Ok(());
        }
        let mut remaining = Vec::new();
        for line in &results {
            match fts5_table(line) {
                Some((schema, table)) => {
                    let command = format!(
                        "INSERT INTO \"{schema}\".\"{table}\"(\"{table}\") VALUES ('integrity-check')"
                    );
                    if let Err(error) = query(&command).execute(self.database.pool()).await {
                        remaining.push(format!("{line} ({error})"));
                    }
                }
                None => remaining.push(line.clone()),
            }
        }
        if remaining.is_empty() {
            return Ok(());
        }
        Err(IntegrityError::Failed {
            check: "integrity_check",
            details: remaining.join("; "),
        })
    }

    async fn run(&self, check: &'static str) -> Result<(), IntegrityError> {
        let query = format!("PRAGMA {check}");
        let results: Vec<String> = query_scalar(&query)
            .fetch_all(self.database.pool())
            .await
            .map_err(|error| IntegrityError::Failed {
                check,
                details: error.to_string(),
            })?;
        if results.len() == 1 && results[0].eq_ignore_ascii_case("ok") {
            return Ok(());
        }
        Err(IntegrityError::Failed {
            check,
            details: results.join("; "),
        })
    }
}

/// `(schema, table)` of an FTS5 index finding of `PRAGMA integrity_check`; only plain identifiers.
fn fts5_table(line: &str) -> Option<(&str, &str)> {
    let name = line.strip_prefix("malformed inverted index for FTS5 table ")?;
    let (schema, table) = name.split_once('.')?;
    let plain = |part: &str| {
        !part.is_empty()
            && part
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    };
    (plain(schema) && plain(table)).then_some((schema, table))
}
