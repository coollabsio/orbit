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
        self.run("integrity_check").await?;
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
