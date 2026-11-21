//! The GitHub issues and pull requests linked to a task.
use orbit_platform::{Id, Row};
use serde::Serialize;
use utoipa::ToSchema;

use super::{TaskError, TaskRepository};

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct GithubLink {
    /// `issue` or `pull_request`.
    pub kind: String,
    pub title: String,
    pub url: String,
    pub state: String,
    /// `true` for a synced issue or its pull request; `false` for a pull request that only
    /// names the task.
    pub source: bool,
}

impl TaskRepository {
    /// The links of `task_id`. The caller checks first that the actor sees the task.
    pub async fn github_links(
        &self,
        workspace_id: Id,
        task_id: Id,
    ) -> Result<Vec<GithubLink>, TaskError> {
        let rows = sqlx::query("SELECT kind, repository || '#' || issue_number AS title, 'https://github.com/' || repository || CASE kind WHEN 'pull_request' THEN '/pull/' ELSE '/issues/' END || issue_number AS url, CASE WHEN sync_paused = 1 THEN 'paused' WHEN kind = 'pull_request' THEN pull_state ELSE 'active' END AS state, 1 AS source FROM github_issue_links WHERE workspace_id = $1 AND task_id = $2 UNION ALL SELECT 'pull_request' AS kind, title, url, state, 0 AS source FROM github_pull_links WHERE workspace_id = $3 AND task_id = $4 ORDER BY kind, title")
            .bind(workspace_id.to_string()).bind(task_id.to_string())
            .bind(workspace_id.to_string()).bind(task_id.to_string())
            .fetch_all(self.database().pool()).await?;
        Ok(rows
            .into_iter()
            .map(|row| GithubLink {
                kind: row.get("kind"),
                title: row.get("title"),
                url: row.get("url"),
                state: row.get("state"),
                source: row.get::<i64, _>("source") != 0,
            })
            .collect())
    }
}
