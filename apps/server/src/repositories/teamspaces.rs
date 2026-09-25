//! Docs teamspaces: shared page spaces inside a workspace. Every member sees, creates and renames
//! them; owners and admins delete them, and only while they hold no live pages. The default
//! teamspace is derived, never stored: the lowest `(position, id)` in the workspace.

use orbit_domain::WorkspaceRole;
use orbit_platform::{Database, Id, TimestampMillis};
use serde::Serialize;
use serde_json::Value;
use sqlx::{Row, Sqlite, Transaction};
use thiserror::Error;
use utoipa::ToSchema;

use super::tasks::{TaskError, record_mutation, require_access, require_access_tx};
use super::workspaces::{WorkspaceError, require_role};

/// Name of the teamspace every new workspace starts with.
pub const DEFAULT_TEAMSPACE_NAME: &str = "General";
const TEAMSPACE_COLUMNS: &str =
    "id, workspace_id, name, icon, position, version, created_at, updated_at";

#[derive(Clone, Debug, Serialize, ToSchema)]
#[schema(as = Teamspace)]
pub struct TeamspaceRecord {
    #[schema(value_type = String)]
    pub id: Id,
    #[schema(value_type = String)]
    pub workspace_id: Id,
    pub name: String,
    #[schema(required = true)]
    pub icon: Option<String>,
    pub position: i64,
    pub version: u64,
    /// True for the workspace's first teamspace by position; new root pages go there by default.
    pub is_default: bool,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: TimestampMillis,
    #[schema(value_type = String, format = DateTime)]
    pub updated_at: TimestampMillis,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct TeamspaceList {
    pub items: Vec<TeamspaceRecord>,
}

/// `None` leaves a field unchanged; `Some(None)` clears the icon.
#[derive(Clone, Debug, Default)]
pub struct TeamspaceChanges {
    pub name: Option<String>,
    pub icon: Option<Option<String>>,
}

#[derive(Debug, Error)]
pub enum TeamspaceError {
    #[error("teamspace was not found")]
    NotFound,
    #[error("only workspace owners and admins may delete teamspaces")]
    Forbidden,
    #[error("teamspace still has live pages")]
    NotEmpty,
    #[error("the last teamspace of a workspace cannot be deleted")]
    LastTeamspace,
    #[error("stale version")]
    VersionConflict { current: Box<Value> },
    #[error("stored teamspace data is invalid")]
    Corrupt,
    #[error("teamspace repository is unavailable")]
    Unavailable(#[from] sqlx::Error),
}

impl From<TaskError> for TeamspaceError {
    fn from(error: TaskError) -> Self {
        match error {
            TaskError::Unavailable(error) => Self::Unavailable(error),
            _ => Self::NotFound,
        }
    }
}

impl From<WorkspaceError> for TeamspaceError {
    fn from(error: WorkspaceError) -> Self {
        match error {
            WorkspaceError::Unavailable(error) => Self::Unavailable(error),
            WorkspaceError::NotFound => Self::NotFound,
            _ => Self::Corrupt,
        }
    }
}

#[derive(Clone)]
pub struct TeamspaceRepository {
    database: Database,
}

impl TeamspaceRepository {
    #[must_use]
    pub fn new(database: Database) -> Self {
        Self { database }
    }

    /// The workspace's teamspaces ordered by `position, id`; the first one is the default.
    pub async fn teamspaces(
        &self,
        workspace_id: Id,
        actor_id: Id,
    ) -> Result<TeamspaceList, TeamspaceError> {
        require_access(self.database.pool(), workspace_id, actor_id).await?;
        let rows = sqlx::query(&format!(
            "SELECT {TEAMSPACE_COLUMNS} FROM teamspaces WHERE workspace_id = ? ORDER BY position, id"
        ))
        .bind(workspace_id.to_string())
        .fetch_all(self.database.pool())
        .await?;
        let items = rows
            .into_iter()
            .enumerate()
            .map(|(index, row)| teamspace_from_row(row, index == 0))
            .collect::<Result<_, _>>()?;
        Ok(TeamspaceList { items })
    }

    /// Appends a teamspace after the last one.
    pub async fn create_teamspace(
        &self,
        workspace_id: Id,
        actor_id: Id,
        name: String,
        icon: Option<String>,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<TeamspaceRecord, TeamspaceError> {
        let id = Id::new_v7();
        let mut tx = self.database.immediate_transaction().await?;
        require_access_tx(&mut tx, workspace_id, actor_id).await?;
        let position: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(position) + 1, 0) FROM teamspaces WHERE workspace_id = ?",
        )
        .bind(workspace_id.to_string())
        .fetch_one(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO teamspaces (id, workspace_id, name, icon, position, version, created_by, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, 0, ?, ?, ?)",
        )
        .bind(id.to_string())
        .bind(workspace_id.to_string())
        .bind(&name)
        .bind(&icon)
        .bind(position)
        .bind(actor_id.to_string())
        .bind(now.as_millis())
        .bind(now.as_millis())
        .execute(&mut *tx)
        .await?;
        let is_default = default_teamspace(&mut tx, workspace_id).await? == Some(id);
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "teamspace.created",
            "teamspace",
            id,
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(TeamspaceRecord {
            id,
            workspace_id,
            name,
            icon,
            position,
            version: 0,
            is_default,
            created_at: now,
            updated_at: now,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update_teamspace(
        &self,
        workspace_id: Id,
        teamspace_id: Id,
        actor_id: Id,
        expected_version: u64,
        changes: TeamspaceChanges,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<TeamspaceRecord, TeamspaceError> {
        let mut tx = self.database.immediate_transaction().await?;
        require_access_tx(&mut tx, workspace_id, actor_id).await?;
        let current = teamspace_in_tx(&mut tx, workspace_id, teamspace_id).await?;
        check_version(expected_version, &current)?;
        let name = changes.name.unwrap_or_else(|| current.name.clone());
        let icon = changes.icon.unwrap_or_else(|| current.icon.clone());
        sqlx::query(
            "UPDATE teamspaces SET name = ?, icon = ?, updated_at = ?, version = version + 1 \
             WHERE id = ? AND workspace_id = ? AND version = ?",
        )
        .bind(&name)
        .bind(&icon)
        .bind(now.as_millis())
        .bind(teamspace_id.to_string())
        .bind(workspace_id.to_string())
        .bind(expected_version as i64)
        .execute(&mut *tx)
        .await?;
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "teamspace.updated",
            "teamspace",
            teamspace_id,
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(TeamspaceRecord {
            name,
            icon,
            version: current.version + 1,
            updated_at: now,
            ..current
        })
    }

    /// Deletes an empty teamspace together with its trashed pages. Requires owner or admin.
    pub async fn delete_teamspace(
        &self,
        workspace_id: Id,
        teamspace_id: Id,
        actor_id: Id,
        expected_version: u64,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), TeamspaceError> {
        let mut tx = self.database.immediate_transaction().await?;
        if require_role(&mut tx, workspace_id, actor_id, false).await? == WorkspaceRole::Member {
            return Err(TeamspaceError::Forbidden);
        }
        let current = teamspace_in_tx(&mut tx, workspace_id, teamspace_id).await?;
        check_version(expected_version, &current)?;
        let (teamspaces, live_pages): (i64, i64) = sqlx::query_as(
            "SELECT (SELECT COUNT(*) FROM teamspaces WHERE workspace_id = ?), \
             (SELECT COUNT(*) FROM pages WHERE teamspace_id = ? AND deleted_at IS NULL)",
        )
        .bind(workspace_id.to_string())
        .bind(teamspace_id.to_string())
        .fetch_one(&mut *tx)
        .await?;
        if teamspaces <= 1 {
            return Err(TeamspaceError::LastTeamspace);
        }
        if live_pages > 0 {
            return Err(TeamspaceError::NotEmpty);
        }
        // The trashed pages go explicitly, in one statement (pages.parent_id is ON DELETE SET
        // NULL, so the order inside it does not matter), before the teamspace row itself.
        sqlx::query("DELETE FROM pages WHERE teamspace_id = ?")
            .bind(teamspace_id.to_string())
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM teamspaces WHERE id = ? AND workspace_id = ?")
            .bind(teamspace_id.to_string())
            .bind(workspace_id.to_string())
            .execute(&mut *tx)
            .await?;
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "teamspace.deleted",
            "teamspace",
            teamspace_id,
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }
}

/// Creates the default "General" teamspace of a new workspace, inside its creating transaction.
pub(super) async fn insert_default_teamspace(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    created_by: Id,
    now: TimestampMillis,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO teamspaces (id, workspace_id, name, icon, position, version, created_by, created_at, updated_at) \
         VALUES (?, ?, ?, NULL, 0, 0, ?, ?, ?)",
    )
    .bind(Id::new_v7().to_string())
    .bind(workspace_id.to_string())
    .bind(DEFAULT_TEAMSPACE_NAME)
    .bind(created_by.to_string())
    .bind(now.as_millis())
    .bind(now.as_millis())
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// The workspace's default teamspace: the lowest `(position, id)`.
pub(super) async fn default_teamspace(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
) -> Result<Option<Id>, sqlx::Error> {
    sqlx::query_scalar::<_, String>(
        "SELECT id FROM teamspaces WHERE workspace_id = ? ORDER BY position, id LIMIT 1",
    )
    .bind(workspace_id.to_string())
    .fetch_optional(&mut **tx)
    .await?
    .map(|id| {
        id.parse()
            .map_err(|error| sqlx::Error::Decode(Box::new(error)))
    })
    .transpose()
}

pub(super) async fn teamspace_exists(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    teamspace_id: Id,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM teamspaces WHERE id = ? AND workspace_id = ?)")
        .bind(teamspace_id.to_string())
        .bind(workspace_id.to_string())
        .fetch_one(&mut **tx)
        .await
}

async fn teamspace_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    teamspace_id: Id,
) -> Result<TeamspaceRecord, TeamspaceError> {
    let row = sqlx::query(&format!(
        "SELECT {TEAMSPACE_COLUMNS} FROM teamspaces WHERE id = ? AND workspace_id = ?"
    ))
    .bind(teamspace_id.to_string())
    .bind(workspace_id.to_string())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(TeamspaceError::NotFound)?;
    let is_default = default_teamspace(tx, workspace_id).await? == Some(teamspace_id);
    teamspace_from_row(row, is_default)
}

fn check_version(expected: u64, record: &TeamspaceRecord) -> Result<(), TeamspaceError> {
    if expected == record.version {
        Ok(())
    } else {
        Err(TeamspaceError::VersionConflict {
            current: Box::new(serde_json::to_value(record).map_err(|_| TeamspaceError::Corrupt)?),
        })
    }
}

fn teamspace_from_row(
    row: sqlx::sqlite::SqliteRow,
    is_default: bool,
) -> Result<TeamspaceRecord, TeamspaceError> {
    Ok(TeamspaceRecord {
        id: parse_id(row.get("id"))?,
        workspace_id: parse_id(row.get("workspace_id"))?,
        name: row.get("name"),
        icon: row.get("icon"),
        position: row.get("position"),
        version: u64::try_from(row.get::<i64, _>("version"))
            .map_err(|_| TeamspaceError::Corrupt)?,
        is_default,
        created_at: TimestampMillis::from_millis(row.get("created_at")),
        updated_at: TimestampMillis::from_millis(row.get("updated_at")),
    })
}

fn parse_id(value: String) -> Result<Id, TeamspaceError> {
    value.parse().map_err(|_| TeamspaceError::Corrupt)
}
