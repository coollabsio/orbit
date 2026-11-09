//! The scan: lists the workspaces of the token's user and the projects in each, with their team.

use orbit_platform::{Database, Id};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use utoipa::ToSchema;

use super::client::{AsanaClient, AsanaError};
use crate::notion::import::clamp;
use crate::task_import::name_or;

/// Most projects one scan lists.
pub const MAX_PROJECTS: usize = 5_000;

/// An Asana project in the scan tree.
#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct AsanaImportProject {
    /// Use it in `project_gids` to import this project.
    pub gid: String,
    pub name: String,
    pub archived: bool,
    /// `null` in workspaces without teams (Asana teams exist only in organizations).
    #[schema(required = true)]
    pub team_gid: Option<String>,
    #[schema(required = true)]
    pub team_name: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct AsanaImportWorkspace {
    pub gid: String,
    pub name: String,
    /// Sorted by team, then by name.
    pub projects: Vec<AsanaImportProject>,
}

/// What the token can see.
#[derive(Clone, Debug, Default, Deserialize, Serialize, ToSchema)]
pub struct AsanaImportTree {
    pub workspaces: Vec<AsanaImportWorkspace>,
    /// The token sees more than 5,000 projects; only the first 5,000 are listed.
    pub truncated: bool,
}

impl AsanaImportTree {
    #[must_use]
    pub fn project(&self, gid: &str) -> Option<&AsanaImportProject> {
        self.workspaces
            .iter()
            .flat_map(|workspace| &workspace.projects)
            .find(|project| project.gid == gid)
    }
}

/// Lists the tree; `None` when the import left `scanning` (cancelled, expired) or `cancel`
/// fired (server shutdown).
pub async fn build_tree(
    client: &AsanaClient,
    database: &Database,
    import_id: Id,
    cancel: &CancellationToken,
) -> Result<Option<AsanaImportTree>, AsanaError> {
    let mut tree = AsanaImportTree::default();
    let mut listed = 0;
    for workspace in client.workspaces().await? {
        if should_stop(database, import_id, cancel).await {
            return Ok(None);
        }
        let room = MAX_PROJECTS - listed;
        if room == 0 {
            tree.truncated = true;
            break;
        }
        // One more than fits tells whether the list was cut.
        let mut projects = client.projects(&workspace.gid, room + 1).await?;
        if projects.len() > room {
            projects.truncate(room);
            tree.truncated = true;
        }
        listed += projects.len();
        let mut projects: Vec<AsanaImportProject> = projects
            .into_iter()
            .map(|project| AsanaImportProject {
                gid: project.gid,
                name: name_or(project.name, "Untitled project"),
                archived: project.archived,
                team_gid: project.team.as_ref().map(|team| team.gid.clone()),
                team_name: project
                    .team
                    .and_then(|team| team.name)
                    .map(|name| clamp(&name, 200, 800)),
            })
            .collect();
        projects.sort_by(|a, b| {
            (
                a.team_name.as_deref().map(str::to_lowercase),
                a.name.to_lowercase(),
            )
                .cmp(&(
                    b.team_name.as_deref().map(str::to_lowercase),
                    b.name.to_lowercase(),
                ))
        });
        tree.workspaces.push(AsanaImportWorkspace {
            gid: workspace.gid,
            name: name_or(workspace.name, "Asana workspace"),
            projects,
        });
    }
    Ok(Some(tree))
}

async fn should_stop(database: &Database, import_id: Id, cancel: &CancellationToken) -> bool {
    if cancel.is_cancelled() {
        return true;
    }
    match sqlx::query_scalar::<_, String>("SELECT status FROM asana_imports WHERE id = $1")
        .bind(import_id.to_string())
        .fetch_optional(database.pool())
        .await
    {
        Ok(status) => status.as_deref() != Some("scanning"),
        // A database hiccup is not a reason to stop; the next check sees it again.
        Err(_) => false,
    }
}
