//! Per-user favourites of a workspace, for the sidebar: tasks, saved views, projects and
//! milestones in one ordered list. The `favorites` table has no foreign key on the item; reads drop the rows whose item is
//! gone, in the trash, or not visible to the caller.

use orbit_platform::{Database, Id, TimestampMillis};
use serde::Serialize;
use sqlx::{Row, Sqlite, Transaction};
use utoipa::ToSchema;

use super::membership;
use super::tasks::{TaskError, parse_id, record_mutation};

/// The kinds the server accepts.
const KINDS: [&str; 4] = ["task", "view", "project", "milestone"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FavoriteKey {
    pub kind: String,
    pub target_id: Id,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct FavoriteRecord {
    /// `task`, `view`, `project` or `milestone`.
    pub kind: String,
    #[schema(value_type = String)]
    pub target_id: Id,
    /// The task title, or the name of the view, project or milestone.
    pub title: String,
    /// The task identifier (`ENG-12`), or the project key of a project or milestone; null for
    /// a view.
    #[schema(required = true)]
    pub identifier: Option<String>,
    /// `#rrggbb`: the colour of the project (of the task or milestone), or of the view.
    #[schema(required = true)]
    pub color: Option<String>,
    /// The icon name of a view; null for a task.
    #[schema(required = true)]
    pub icon: Option<String>,
    /// The web app path of the item, such as `/tasks/ENG-12`, `/views/<id>` or
    /// `/tasks/projects/<id>`.
    pub path: String,
    pub position: i64,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct FavoriteList {
    pub items: Vec<FavoriteRecord>,
}

/// The caller's favourites that still resolve, in sidebar order: one SELECT for each kind, with
/// the item's title, colour and path. A row whose item is gone, in the trash or not visible to
/// the caller is not in the result. Binds: user id, workspace id.
const RESOLVED: &str = "SELECT favorites.kind, favorites.target_id, favorites.position, favorites.created_at, \
     tasks.title AS title, projects.color AS color, NULL AS icon, \
     CASE WHEN tasks.number IS NULL THEN NULL ELSE projects.project_key || '-' || tasks.number END AS identifier, \
     '/tasks/' || CASE WHEN tasks.number IS NULL THEN tasks.id ELSE projects.project_key || '-' || tasks.number END AS path \
     FROM favorites \
     JOIN tasks ON tasks.id = favorites.target_id AND tasks.workspace_id = favorites.workspace_id \
     JOIN projects ON projects.id = tasks.project_id \
     WHERE favorites.kind = 'task' AND favorites.user_id = ?1 AND favorites.workspace_id = ?2 \
     AND tasks.deleted_at IS NULL AND projects.deleted_at IS NULL \
     UNION ALL \
     SELECT favorites.kind, favorites.target_id, favorites.position, favorites.created_at, \
     saved_views.name, saved_views.color, saved_views.icon, NULL, '/views/' || saved_views.id \
     FROM favorites \
     JOIN saved_views ON saved_views.id = favorites.target_id \
     AND saved_views.workspace_id = favorites.workspace_id \
     WHERE favorites.kind = 'view' AND favorites.user_id = ?1 AND favorites.workspace_id = ?2 \
     AND (saved_views.visibility = 'workspace' OR saved_views.owner_user_id = ?1) \
     UNION ALL \
     SELECT favorites.kind, favorites.target_id, favorites.position, favorites.created_at, \
     projects.name, projects.color, NULL, projects.project_key, '/tasks/projects/' || projects.id \
     FROM favorites \
     JOIN projects ON projects.id = favorites.target_id AND projects.workspace_id = favorites.workspace_id \
     WHERE favorites.kind = 'project' AND favorites.user_id = ?1 AND favorites.workspace_id = ?2 \
     AND projects.deleted_at IS NULL \
     UNION ALL \
     SELECT favorites.kind, favorites.target_id, favorites.position, favorites.created_at, \
     milestones.name, projects.color, NULL, projects.project_key, \
     '/tasks/projects/' || projects.id || '/milestones/' || milestones.id \
     FROM favorites \
     JOIN milestones ON milestones.id = favorites.target_id \
     AND milestones.workspace_id = favorites.workspace_id \
     JOIN projects ON projects.id = milestones.project_id \
     WHERE favorites.kind = 'milestone' AND favorites.user_id = ?1 AND favorites.workspace_id = ?2 \
     AND projects.deleted_at IS NULL \
     ORDER BY 3, 4, 2";

#[derive(Clone)]
pub struct FavoriteRepository {
    database: Database,
}

impl FavoriteRepository {
    #[must_use]
    pub fn new(database: Database) -> Self {
        Self { database }
    }

    pub async fn list(&self, workspace_id: Id, actor_id: Id) -> Result<FavoriteList, TaskError> {
        let mut tx = self.database.pool().begin().await?;
        require_member(&mut tx, workspace_id, actor_id).await?;
        let items = resolved(&mut tx, workspace_id, actor_id).await?;
        Ok(FavoriteList { items })
    }

    /// Idempotent: adding a favourite (or removing a non-favourite) again changes nothing and is
    /// not broadcast. A new favourite goes to the end of the caller's list in this workspace.
    /// An item the caller cannot see is `NotFound`.
    pub async fn set(
        &self,
        workspace_id: Id,
        actor_id: Id,
        key: &FavoriteKey,
        favorite: bool,
        request_id: &str,
    ) -> Result<(), TaskError> {
        if !KINDS.contains(&key.kind.as_str()) {
            return Err(TaskError::Invalid { field: "kind" });
        }
        let now = TimestampMillis::now();
        let mut tx = self.database.immediate_transaction().await?;
        require_member(&mut tx, workspace_id, actor_id).await?;
        let visible = match key.kind.as_str() {
            "task" => {
                "SELECT COUNT(*) FROM tasks JOIN projects ON projects.id = tasks.project_id \
                 WHERE tasks.id = ?1 AND tasks.workspace_id = ?2 AND ?3 IS NOT NULL \
                 AND tasks.deleted_at IS NULL AND projects.deleted_at IS NULL"
            }
            "project" => {
                "SELECT COUNT(*) FROM projects WHERE id = ?1 AND workspace_id = ?2 \
                 AND ?3 IS NOT NULL AND deleted_at IS NULL"
            }
            "milestone" => {
                "SELECT COUNT(*) FROM milestones JOIN projects ON projects.id = milestones.project_id \
                 WHERE milestones.id = ?1 AND milestones.workspace_id = ?2 AND ?3 IS NOT NULL \
                 AND projects.deleted_at IS NULL"
            }
            _ => {
                "SELECT COUNT(*) FROM saved_views WHERE id = ?1 AND workspace_id = ?2 \
                 AND (visibility = 'workspace' OR owner_user_id = ?3)"
            }
        };
        let found: i64 = sqlx::query_scalar(visible)
            .bind(key.target_id.to_string())
            .bind(workspace_id.to_string())
            .bind(actor_id.to_string())
            .fetch_one(&mut *tx)
            .await?;
        if found != 1 {
            return Err(TaskError::NotFound);
        }
        let changed = if favorite {
            sqlx::query(
                "INSERT INTO favorites (workspace_id, user_id, kind, target_id, position, created_at) \
                 SELECT ?1, ?2, ?3, ?4, COALESCE(MAX(position) + 1, 0), ?5 FROM favorites \
                 WHERE user_id = ?2 AND workspace_id = ?1 \
                 ON CONFLICT (user_id, kind, target_id) DO NOTHING",
            )
            .bind(workspace_id.to_string())
            .bind(actor_id.to_string())
            .bind(&key.kind)
            .bind(key.target_id.to_string())
            .bind(now.as_millis())
            .execute(&mut *tx)
            .await?
        } else {
            sqlx::query("DELETE FROM favorites WHERE user_id = ? AND kind = ? AND target_id = ?")
                .bind(actor_id.to_string())
                .bind(&key.kind)
                .bind(key.target_id.to_string())
                .execute(&mut *tx)
                .await?
        };
        if changed.rows_affected() == 0 {
            return Ok(());
        }
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "favorite.changed",
            "favorite",
            key.target_id,
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// `keys` must be a permutation of the caller's resolved favourites in this workspace.
    /// Skips the writes and the broadcast when the order already matches.
    pub async fn reorder(
        &self,
        workspace_id: Id,
        actor_id: Id,
        keys: Vec<FavoriteKey>,
        request_id: &str,
    ) -> Result<(), TaskError> {
        let invalid = || TaskError::Invalid { field: "items" };
        let now = TimestampMillis::now();
        let mut tx = self.database.immediate_transaction().await?;
        require_member(&mut tx, workspace_id, actor_id).await?;
        let current: Vec<FavoriteKey> = resolved(&mut tx, workspace_id, actor_id)
            .await?
            .into_iter()
            .map(|item| FavoriteKey {
                kind: item.kind,
                target_id: item.target_id,
            })
            .collect();
        if current.len() != keys.len()
            || current.iter().any(|key| !keys.contains(key))
            || keys.iter().any(|key| !current.contains(key))
        {
            return Err(invalid());
        }
        if current == keys {
            return Ok(());
        }
        for (position, key) in keys.iter().enumerate() {
            sqlx::query(
                "UPDATE favorites SET position = ? WHERE user_id = ? AND kind = ? AND target_id = ?",
            )
            .bind(i64::try_from(position).map_err(|_| invalid())?)
            .bind(actor_id.to_string())
            .bind(&key.kind)
            .bind(key.target_id.to_string())
            .execute(&mut *tx)
            .await?;
        }
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "favorite.changed",
            "favorite",
            actor_id,
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }
}

/// Same membership rule as the task routes: non-members and deleted workspaces are `NotFound`.
async fn require_member(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    actor_id: Id,
) -> Result<(), TaskError> {
    membership::actor(&mut **tx, workspace_id, actor_id)
        .await?
        .map(|_| ())
        .ok_or(TaskError::NotFound)
}

async fn resolved(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    actor_id: Id,
) -> Result<Vec<FavoriteRecord>, TaskError> {
    sqlx::query(RESOLVED)
        .bind(actor_id.to_string())
        .bind(workspace_id.to_string())
        .fetch_all(&mut **tx)
        .await?
        .into_iter()
        .map(|row| {
            let kind: String = row.get("kind");
            let target_id = parse_id(row.get("target_id"))?;
            Ok(FavoriteRecord {
                kind,
                target_id,
                title: row.get("title"),
                identifier: row.get("identifier"),
                color: row.get("color"),
                icon: row.get("icon"),
                path: row.get("path"),
                position: row.get("position"),
            })
        })
        .collect()
}
