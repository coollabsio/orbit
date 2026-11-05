//! The scan: lists the Linear teams the key's user can see, with their issue counts.

use orbit_platform::{Database, Id};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use utoipa::ToSchema;

use super::client::{LinearClient, LinearError};
use crate::notion::import::clamp;
use crate::task_import::name_or;

/// Most teams one scan lists.
pub const MAX_TEAMS: usize = 1_000;

/// A Linear team in the scan tree; importing it makes one Orbit project.
#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct LinearImportTeam {
    /// Use it in `team_ids` to import this team.
    pub id: String,
    /// The team's issue key prefix, e.g. `ENG`.
    pub key: String,
    pub name: String,
    /// Issues in the team, archived ones included.
    pub issue_count: u64,
    pub archived: bool,
}

/// What the key can see.
#[derive(Clone, Debug, Default, Deserialize, Serialize, ToSchema)]
pub struct LinearImportTree {
    /// The Linear workspace (organization) name.
    pub organization: String,
    /// Sorted by name; archived teams last.
    pub teams: Vec<LinearImportTeam>,
    /// The key sees more than 1,000 teams; only the first 1,000 are listed.
    pub truncated: bool,
}

impl LinearImportTree {
    #[must_use]
    pub fn team(&self, id: &str) -> Option<&LinearImportTeam> {
        self.teams.iter().find(|team| team.id == id)
    }
}

/// Lists the tree; `None` when the import left `scanning` (cancelled, expired) or `cancel`
/// fired (server shutdown).
pub async fn build_tree(
    client: &LinearClient,
    database: &Database,
    import_id: Id,
    cancel: &CancellationToken,
) -> Result<Option<LinearImportTree>, LinearError> {
    let organization = match client.organization().await {
        Ok(organization) => name_or(organization.name, "Linear workspace"),
        // Some keys cannot read the organization; the name is only a label.
        Err(LinearError::Forbidden { .. }) => "Linear workspace".to_owned(),
        Err(error) => return Err(error),
    };
    if should_stop(database, import_id, cancel).await {
        return Ok(None);
    }
    // One more than fits tells whether the list was cut.
    let mut teams = client.teams(MAX_TEAMS + 1).await?;
    let truncated = teams.len() > MAX_TEAMS;
    teams.truncate(MAX_TEAMS);
    let mut teams: Vec<LinearImportTeam> = teams
        .into_iter()
        .map(|team| LinearImportTeam {
            key: team
                .key
                .map(|key| clamp(key.trim(), 20, 80))
                .unwrap_or_default(),
            name: name_or(team.name, "Untitled team"),
            issue_count: team.issue_count,
            archived: team.archived_at.is_some(),
            id: team.id,
        })
        .collect();
    teams.sort_by(|a, b| {
        (a.archived, a.name.to_lowercase()).cmp(&(b.archived, b.name.to_lowercase()))
    });
    Ok(Some(LinearImportTree {
        organization,
        teams,
        truncated,
    }))
}

async fn should_stop(database: &Database, import_id: Id, cancel: &CancellationToken) -> bool {
    if cancel.is_cancelled() {
        return true;
    }
    match sqlx::query_scalar::<_, String>("SELECT status FROM linear_imports WHERE id = $1")
        .bind(import_id.to_string())
        .fetch_optional(database.pool())
        .await
    {
        Ok(status) => status.as_deref() != Some("scanning"),
        // A database hiccup is not a reason to stop; the next check sees it again.
        Err(_) => false,
    }
}
