//! The scan: lists the boards the member can see, grouped by Trello Workspace, with the number
//! of open cards on each.

use std::collections::HashMap;

use orbit_platform::{Database, Id};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use utoipa::ToSchema;

use super::client::{TrelloClient, TrelloError};
use crate::task_import::name_or;

/// Most boards one scan lists.
pub const MAX_BOARDS: usize = 1_000;

/// A Trello board in the scan tree; importing it makes one Orbit project.
#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct TrelloImportBoard {
    /// Use it in `board_ids` to import this board.
    pub id: String,
    pub name: String,
    /// The board is closed (archived in Trello).
    pub archived: bool,
    /// Open cards on open lists; `null` when Trello did not let the token count them.
    #[schema(required = true)]
    pub card_count: Option<u64>,
}

/// A Trello Workspace, or the member's boards outside any Workspace they belong to (`id`
/// `null`).
#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct TrelloImportWorkspace {
    #[schema(required = true)]
    pub id: Option<String>,
    pub name: String,
    /// Sorted by name; archived boards last.
    pub boards: Vec<TrelloImportBoard>,
}

/// What the key and token can see.
#[derive(Clone, Debug, Default, Deserialize, Serialize, ToSchema)]
pub struct TrelloImportTree {
    /// Sorted by name; the boards outside a Workspace last.
    pub workspaces: Vec<TrelloImportWorkspace>,
    /// The member sees more than 1,000 boards; only the first 1,000 are listed.
    pub truncated: bool,
}

impl TrelloImportTree {
    #[must_use]
    pub fn board(&self, id: &str) -> Option<&TrelloImportBoard> {
        self.workspaces
            .iter()
            .flat_map(|workspace| &workspace.boards)
            .find(|board| board.id == id)
    }
}

/// Lists the tree; `None` when the import left `scanning` (cancelled, expired) or `cancel`
/// fired (server shutdown).
pub async fn build_tree(
    client: &TrelloClient,
    database: &Database,
    import_id: Id,
    cancel: &CancellationToken,
) -> Result<Option<TrelloImportTree>, TrelloError> {
    let organizations: HashMap<String, String> = client
        .organizations()
        .await?
        .into_iter()
        .map(|organization| {
            let name = name_or(
                organization.display_name.or(organization.name),
                "Trello Workspace",
            );
            (organization.id, name)
        })
        .collect();
    let mut boards = client.boards().await?;
    let truncated = boards.len() > MAX_BOARDS;
    boards.truncate(MAX_BOARDS);
    let mut groups: HashMap<Option<String>, Vec<TrelloImportBoard>> = HashMap::new();
    for board in boards {
        if should_stop(database, import_id, cancel).await {
            return Ok(None);
        }
        let card_count = match client.open_lists_with_cards(&board.id).await {
            Ok(lists) => Some(lists.iter().map(|list| list.cards.len() as u64).sum()),
            Err(TrelloError::Forbidden { .. } | TrelloError::NotFound { .. }) => None,
            Err(error) => return Err(error),
        };
        let workspace = board
            .id_organization
            .filter(|id| organizations.contains_key(id));
        groups
            .entry(workspace)
            .or_default()
            .push(TrelloImportBoard {
                name: name_or(board.name, "Untitled board"),
                archived: board.closed,
                card_count,
                id: board.id,
            });
    }
    let mut workspaces: Vec<TrelloImportWorkspace> = groups
        .into_iter()
        .map(|(id, mut boards)| {
            boards.sort_by(|a, b| {
                (a.archived, a.name.to_lowercase()).cmp(&(b.archived, b.name.to_lowercase()))
            });
            let name = id
                .as_ref()
                .and_then(|id| organizations.get(id).cloned())
                .unwrap_or_else(|| "Other boards".to_owned());
            TrelloImportWorkspace { id, name, boards }
        })
        .collect();
    workspaces.sort_by(|a, b| {
        (a.id.is_none(), a.name.to_lowercase()).cmp(&(b.id.is_none(), b.name.to_lowercase()))
    });
    Ok(Some(TrelloImportTree {
        workspaces,
        truncated,
    }))
}

async fn should_stop(database: &Database, import_id: Id, cancel: &CancellationToken) -> bool {
    if cancel.is_cancelled() {
        return true;
    }
    match sqlx::query_scalar::<_, String>("SELECT status FROM trello_imports WHERE id = $1")
        .bind(import_id.to_string())
        .fetch_optional(database.pool())
        .await
    {
        Ok(status) => status.as_deref() != Some("scanning"),
        // A database hiccup is not a reason to stop; the next check sees it again.
        Err(_) => false,
    }
}
