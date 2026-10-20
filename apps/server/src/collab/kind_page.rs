//! What co-editing needs to know about pages: where a page's document comes from, whether it is
//! locked, and how its document is written back (`pages.content_json`).

use orbit_platform::{Id, Row, TimestampMillis};
use serde_json::Value;
use sqlx::{Any, AnyConnection, AnyPool, Transaction};

use super::hub::CollabError;
use crate::repositories::page_versions::project_collab_content;
use crate::repositories::pages::PageError;

/// The page's workspace and lock state; `None` when the page does not exist.
pub(super) async fn meta(pool: &AnyPool, id: Id) -> Result<Option<(Id, bool)>, CollabError> {
    let Some(row) = sqlx::query(
        "SELECT workspace_id, locked_at IS NOT NULL AS locked FROM pages WHERE id = $1",
    )
    .bind(id.to_string())
    .fetch_optional(pool)
    .await?
    else {
        return Ok(None);
    };
    let workspace_id = row
        .get::<String, _>("workspace_id")
        .parse()
        .map_err(|_| CollabError::Corrupt("invalid workspace id".to_owned()))?;
    Ok(Some((workspace_id, row.get("locked"))))
}

/// The page's stored content (`pages.content_json`), the source of a new document.
pub(super) async fn content(
    connection: &mut AnyConnection,
    id: Id,
) -> Result<Vec<Value>, CollabError> {
    let content: String = sqlx::query_scalar("SELECT content_json FROM pages WHERE id = $1")
        .bind(id.to_string())
        .fetch_optional(&mut *connection)
        .await?
        .ok_or(CollabError::NotFound)?;
    Ok(serde_json::from_str(&content).unwrap_or_default())
}

/// Writes the document's blocks to the page (see [`project_collab_content`]).
pub(super) async fn project(
    tx: &mut Transaction<'_, Any>,
    id: Id,
    blocks: &[Value],
    editor: Option<Id>,
    audit: bool,
    now: TimestampMillis,
) -> Result<bool, CollabError> {
    project_collab_content(tx, id, blocks, editor, audit, &format!("collab-{id}"), now)
        .await
        .map_err(|error| match error {
            PageError::NotFound => CollabError::NotFound,
            PageError::Unavailable(error) => CollabError::Unavailable(error),
            other => CollabError::Corrupt(other.to_string()),
        })
}
