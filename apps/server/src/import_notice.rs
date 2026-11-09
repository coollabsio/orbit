//! The email and inbox notification a user gets when a long import ends, for every importer.

use std::time::Duration;

use orbit_platform::{Database, Id, Row, TimestampMillis};

use crate::mail::{Mailer, templates};

/// An import that ran at least this long tells its user when it ends; a shorter one ends while
/// the user still watches its page.
pub const IMPORT_NOTICE_AFTER: Duration = Duration::from_secs(5 * 60);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImportSource {
    Asana,
    Linear,
    Trello,
    Notion,
    Obsidian,
}

impl ImportSource {
    /// `notifications.import_source`.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Asana => "asana",
            Self::Linear => "linear",
            Self::Trello => "trello",
            Self::Notion => "notion",
            Self::Obsidian => "obsidian",
        }
    }

    fn table(self) -> &'static str {
        match self {
            Self::Asana => "asana_imports",
            Self::Linear => "linear_imports",
            Self::Trello => "trello_imports",
            Self::Notion => "notion_imports",
            Self::Obsidian => "obsidian_imports",
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Asana => "Asana",
            Self::Linear => "Linear",
            Self::Trello => "Trello",
            Self::Notion => "Notion",
            Self::Obsidian => "Obsidian",
        }
    }

    fn unit(self) -> &'static str {
        match self {
            Self::Asana => "tasks",
            Self::Linear => "issues",
            Self::Trello => "cards",
            Self::Notion | Self::Obsidian => "pages",
        }
    }

    /// The import's page in the web app.
    fn result_path(self, import_id: Id, workspace_id: &str) -> String {
        let page = match self {
            Self::Asana => "tasks-import",
            Self::Linear => "tasks-import/linear",
            Self::Trello => "tasks-import/trello",
            Self::Notion => "docs/import",
            Self::Obsidian => "docs/import/obsidian",
        };
        format!("/{page}/{import_id}?workspace={workspace_id}")
    }
}

/// Tells a user that a long import ended. Each importer's job handler calls it after every run.
#[derive(Clone)]
pub struct ImportNotices {
    database: Database,
    mailer: Mailer,
}

impl ImportNotices {
    #[must_use]
    pub fn new(database: Database, mailer: Mailer) -> Self {
        Self { database, mailer }
    }

    /// After a run of an import job. A failed notice is logged: it never changes the job's result.
    pub async fn after_run(&self, source: ImportSource, import_id: Id) {
        if let Err(error) = self.notify(source, import_id, TimestampMillis::now()).await {
            tracing::warn!(source = source.key(), import_id = %import_id, %error, "import notice failed");
        }
    }

    /// When the import ended (completed or failed) at least [`IMPORT_NOTICE_AFTER`] after its
    /// start, adds one inbox notification for its user and emails them when a mail server is set.
    /// Returns whether it notified; a second call for the same import does nothing.
    pub async fn notify(
        &self,
        source: ImportSource,
        import_id: Id,
        now: TimestampMillis,
    ) -> Result<bool, sqlx::Error> {
        let Some(row) = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT i.workspace_id, i.user_id, i.status, i.started_at, i.done, i.failed, \
             u.email, w.name AS workspace_name FROM {} AS i \
             JOIN users AS u ON u.id = i.user_id JOIN workspaces AS w ON w.id = i.workspace_id \
             WHERE i.id = $1",
            source.table()
        )))
        .bind(import_id.to_string())
        .fetch_optional(self.database.pool())
        .await?
        else {
            return Ok(false);
        };
        let status: String = row.get("status");
        let completed = match status.as_str() {
            "completed" => true,
            "failed" => false,
            _ => return Ok(false),
        };
        let long = row
            .get::<Option<i64>, _>("started_at")
            .is_some_and(|started| {
                now.as_millis() - started >= IMPORT_NOTICE_AFTER.as_millis() as i64
            });
        if !long {
            return Ok(false);
        }
        let workspace_id: String = row.get("workspace_id");
        let user_id: String = row.get("user_id");
        // the user may have left the workspace meanwhile: then no notice
        let inserted = sqlx::query(
            "INSERT INTO notifications (id, workspace_id, recipient_user_id, actor_user_id, kind, \
             import_source, import_id, import_status, dedupe_key, created_at) \
             SELECT $1, $2, $3, $3, 'import_finished', $4, $5, $6, $7, $8 \
             WHERE EXISTS (SELECT 1 FROM memberships WHERE workspace_id = $2 AND user_id = $3) \
             ON CONFLICT DO NOTHING",
        )
        .bind(Id::new_v7().to_string())
        .bind(&workspace_id)
        .bind(&user_id)
        .bind(source.key())
        .bind(import_id.to_string())
        .bind(&status)
        .bind(format!("import_finished:{}:{import_id}", source.key()))
        .bind(now.as_millis())
        .execute(self.database.pool())
        .await?
        .rows_affected()
            == 1;
        if !inserted {
            return Ok(false);
        }
        if self.mailer.enabled().await? {
            let count = |column: &str| row.get::<i64, _>(column);
            // `done` counts finished items only: a run that failed early may already have made some
            let mut summary = if !completed && count("done") == 0 {
                "Open the import to see what it made before it stopped".to_owned()
            } else {
                format!("{} {} imported", thousands(count("done")), source.unit())
            };
            if count("failed") > 0 {
                summary.push_str(&format!(", {} failed", thousands(count("failed"))));
            }
            self.mailer.send_later(templates::import_finished(
                &row.get::<String, _>("email"),
                source.name(),
                completed,
                &row.get::<String, _>("workspace_name"),
                &summary,
                &self
                    .mailer
                    .link(&source.result_path(import_id, &workspace_id)),
            ));
        }
        Ok(true)
    }
}

/// `5600` → `5,600`.
fn thousands(value: i64) -> String {
    let digits = value.unsigned_abs().to_string();
    let mut out = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            out.push(',');
        }
        out.push(digit);
    }
    if value < 0 { format!("-{out}") } else { out }
}

#[cfg(test)]
mod tests {
    use super::thousands;

    #[test]
    fn groups_thousands() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(5_600), "5,600");
        assert_eq!(thousands(1_234_567), "1,234,567");
    }
}
