//! Converts the legacy Markdown in `tasks.description` and `task_comments.body`
//! into rich text documents. Runs as a data migration between schema versions 14
//! and 15, so it reads the Markdown columns before version 15 drops them.
//!
//! Old `@Name` mentions survive as plain text. The old mention ids were never
//! persisted - `mentioned_user_ids` only fired a notification at write time and
//! was then discarded - so there is nothing to convert them to.
use orbit_domain::rich_text;
use orbit_platform::{DataMigration, DataMigrationFuture, Database};
use sqlx::Row;

pub const RICH_TEXT_BACKFILL: DataMigration = DataMigration {
    after_version: 14,
    run: backfill,
};

fn backfill(database: &Database) -> DataMigrationFuture<'_> {
    Box::pin(async move {
        convert(
            database,
            "SELECT id, description FROM tasks WHERE description_json = ''",
            "UPDATE tasks SET description_json = ?, description_text = ? WHERE id = ?",
        )
        .await?;
        convert(
            database,
            "SELECT id, body FROM task_comments WHERE body_json = ''",
            "UPDATE task_comments SET body_json = ?, body_text = ? WHERE id = ?",
        )
        .await
    })
}

async fn convert(database: &Database, select: &str, update: &str) -> Result<(), String> {
    let rows = sqlx::query(select)
        .fetch_all(database.pool())
        .await
        .map_err(|error| error.to_string())?;
    for row in rows {
        let id: String = row.get(0);
        let markdown: String = row.get(1);
        let document = rich_text::markdown_to_document(&markdown);
        let text = rich_text::extract_text(&document);
        let json = serde_json::to_string(&document).map_err(|error| error.to_string())?;
        sqlx::query(update)
            .bind(json)
            .bind(text)
            .bind(id)
            .execute(database.pool())
            .await
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}
