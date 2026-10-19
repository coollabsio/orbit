//! What co-editing needs to know about task descriptions: where a task's document comes from,
//! whether it is locked (a GitHub issue owns its text), and how its document is written back
//! (`tasks.description_json` and the markdown copy in `tasks.description`).

use orbit_platform::{Id, Row, TimestampMillis};
use serde_json::Value;
use sqlx::{Any, AnyPool, Transaction};

use super::hub::CollabError;
use super::markdown_import::markdown_to_blocks;
use crate::repositories::tasks::{TaskError, project_task_description};

/// The task's workspace and lock state; `None` when the task does not exist. A task linked to a
/// GitHub issue is locked: GitHub sync writes its text.
pub(super) async fn meta(pool: &AnyPool, id: Id) -> Result<Option<(Id, bool)>, CollabError> {
    let Some(row) = sqlx::query(
        "SELECT workspace_id, \
         EXISTS (SELECT 1 FROM github_issue_links WHERE github_issue_links.task_id = tasks.id) \
         AS locked FROM tasks WHERE id = $1",
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

/// The description's blocks: `description_json`, or the markdown of a task written before rich
/// text, converted (mentions of the workspace's members included).
pub(super) async fn content(
    pool: &AnyPool,
    id: Id,
    workspace_id: Id,
) -> Result<Vec<Value>, CollabError> {
    let row = sqlx::query("SELECT description, description_json FROM tasks WHERE id = $1")
        .bind(id.to_string())
        .fetch_optional(pool)
        .await?
        .ok_or(CollabError::NotFound)?;
    if let Some(blocks) = row
        .get::<Option<String>, _>("description_json")
        .and_then(|json| serde_json::from_str::<Vec<Value>>(&json).ok())
    {
        return Ok(blocks);
    }
    let members = members(pool, workspace_id).await?;
    Ok(markdown_to_blocks(&row.get::<String, _>("description"), &members))
}

/// The workspace's members as `(id, display name)`, for mentions in imported markdown.
pub(crate) async fn members<'e, E>(executor: E, workspace_id: Id) -> Result<Vec<(Id, String)>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = Any>,
{
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT users.id, users.display_name FROM memberships \
         JOIN users ON users.id = memberships.user_id WHERE memberships.workspace_id = $1",
    )
    .bind(workspace_id.to_string())
    .fetch_all(executor)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|(id, name)| Some((id.parse().ok()?, name)))
        .collect())
}

/// Writes the document's blocks to the task (see [`project_task_description`]).
pub(super) async fn project(
    tx: &mut Transaction<'_, Any>,
    id: Id,
    workspace_id: Id,
    blocks: &[Value],
    editor: Option<Id>,
    audit: bool,
    now: TimestampMillis,
) -> Result<bool, CollabError> {
    project_task_description(
        tx,
        id,
        workspace_id,
        blocks,
        editor,
        audit,
        &format!("collab-{id}"),
        now,
    )
    .await
    .map_err(|error| match error {
        TaskError::NotFound => CollabError::NotFound,
        TaskError::Unavailable(error) => CollabError::Unavailable(error),
        other => CollabError::Corrupt(other.to_string()),
    })
}
