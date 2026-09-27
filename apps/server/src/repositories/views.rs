//! Saved task views, per-user view favorites, and per-page view preferences.
//!
//! Personal views exist only for their owner; everyone else gets `NotFound`. Workspace views are
//! readable by every member and editable by their owner or a workspace owner/admin.

use orbit_platform::{Database, Id, TimestampMillis};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, Sqlite};
use utoipa::ToSchema;

use super::task_filter::{ViewState, parse_view_state, preset_filter, validate_view_state};
use super::tasks::{TaskError, parse_id, record_mutation};
use crate::audit::{self, AuditOutcome};

const NAME_MAX_CHARS: usize = 80;
const DESCRIPTION_MAX_CHARS: usize = 500;
const ICON_MAX_BYTES: usize = 64;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    Personal,
    Workspace,
}

impl Visibility {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Personal => "personal",
            Self::Workspace => "workspace",
        }
    }

    fn from_db(value: &str) -> Result<Self, TaskError> {
        match value {
            "personal" => Ok(Self::Personal),
            "workspace" => Ok(Self::Workspace),
            _ => Err(TaskError::Conflict),
        }
    }
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct ViewOwner {
    #[schema(value_type = String)]
    pub user_id: Id,
    pub display_name: String,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct SavedViewRecord {
    #[schema(value_type = String)]
    pub id: Id,
    #[schema(value_type = String)]
    pub workspace_id: Id,
    pub owner: ViewOwner,
    pub name: String,
    pub description: String,
    /// Icon name from the web app's icon set.
    #[schema(required = true)]
    pub icon: Option<String>,
    /// `#rrggbb`.
    #[schema(required = true)]
    pub color: Option<String>,
    pub visibility: Visibility,
    /// Null when the stored state no longer parses; `state_error` then says why.
    #[schema(required = true)]
    pub state: Option<ViewState>,
    #[schema(required = true)]
    pub state_error: Option<String>,
    pub version: i64,
    /// Whether the caller has favorited this view.
    pub is_favorite: bool,
    /// The caller's sidebar position for this view; null when not a favorite.
    #[schema(required = true)]
    pub favorite_position: Option<i64>,
    /// Whether the caller may PATCH/DELETE this view.
    pub can_edit: bool,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: TimestampMillis,
    #[schema(value_type = String, format = DateTime)]
    pub updated_at: TimestampMillis,
}

#[derive(Clone, Debug)]
pub struct ViewCreate {
    pub name: String,
    pub description: String,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub visibility: Visibility,
    pub state: ViewState,
}

#[derive(Clone, Debug)]
pub struct ViewUpdate {
    pub expected_version: i64,
    pub name: Option<String>,
    pub description: Option<String>,
    /// `None`: unchanged. `Some(None)`: clear.
    pub icon: Option<Option<String>>,
    /// `None`: unchanged. `Some(None)`: clear.
    pub color: Option<Option<String>>,
    pub visibility: Option<Visibility>,
    pub state: Option<ViewState>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct ViewPreferenceRecord {
    /// `all`, `project:<id>` or `preset:<name>`.
    pub page_key: String,
    /// Null when the stored state no longer parses; `state_error` then says why.
    #[schema(required = true)]
    pub state: Option<ViewState>,
    #[schema(required = true)]
    pub state_error: Option<String>,
    #[schema(value_type = String, format = DateTime)]
    pub updated_at: TimestampMillis,
}

#[derive(Clone)]
pub struct ViewRepository {
    database: Database,
}

impl ViewRepository {
    #[must_use]
    pub fn new(database: Database) -> Self {
        Self { database }
    }

    pub async fn list_views(
        &self,
        workspace_id: Id,
        actor_id: Id,
    ) -> Result<Vec<SavedViewRecord>, TaskError> {
        let caller = load_caller(self.database.pool(), workspace_id, actor_id).await?;
        let sql = visible_views_sql("ORDER BY saved_views.name COLLATE NOCASE, saved_views.id");
        sqlx::query(&sql)
            .bind(actor_id.to_string())
            .bind(workspace_id.to_string())
            .bind(actor_id.to_string())
            .fetch_all(self.database.pool())
            .await?
            .into_iter()
            .map(|row| view_from_row(row, &caller))
            .collect()
    }

    pub async fn get_view(
        &self,
        workspace_id: Id,
        actor_id: Id,
        view_id: Id,
    ) -> Result<SavedViewRecord, TaskError> {
        let caller = load_caller(self.database.pool(), workspace_id, actor_id).await?;
        find_view(self.database.pool(), &caller, workspace_id, view_id).await
    }

    pub async fn create_view(
        &self,
        workspace_id: Id,
        actor_id: Id,
        input: ViewCreate,
        request_id: &str,
    ) -> Result<SavedViewRecord, TaskError> {
        let name = clean_name(&input.name)?;
        let description = clean_description(input.description)?;
        let icon = clean_icon(input.icon)?;
        let color = clean_color(input.color)?;
        let state_json = encode_state(&input.state)?;
        let id = Id::new_v7();
        let now = TimestampMillis::now();
        let mut tx = self.database.immediate_transaction().await?;
        let caller = load_caller(&mut *tx, workspace_id, actor_id).await?;
        sqlx::query(
            "INSERT INTO saved_views (id, workspace_id, owner_user_id, name, description, icon, color, \
             visibility, state_json, version, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?)",
        )
        .bind(id.to_string())
        .bind(workspace_id.to_string())
        .bind(actor_id.to_string())
        .bind(name)
        .bind(description)
        .bind(icon)
        .bind(color)
        .bind(input.visibility.as_str())
        .bind(state_json)
        .bind(now.as_millis())
        .bind(now.as_millis())
        .execute(&mut *tx)
        .await?;
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "saved_view.created",
            "saved_view",
            id,
            request_id,
            now,
        )
        .await?;
        let record = find_view(&mut *tx, &caller, workspace_id, id).await?;
        tx.commit().await?;
        Ok(record)
    }

    pub async fn update_view(
        &self,
        workspace_id: Id,
        actor_id: Id,
        view_id: Id,
        input: ViewUpdate,
        request_id: &str,
    ) -> Result<SavedViewRecord, TaskError> {
        let name = input.name.as_deref().map(clean_name).transpose()?;
        let description = input.description.map(clean_description).transpose()?;
        let icon = input.icon.map(clean_icon).transpose()?;
        let color = input.color.map(clean_color).transpose()?;
        let state_json = input.state.as_ref().map(encode_state).transpose()?;
        let now = TimestampMillis::now();
        let mut tx = self.database.immediate_transaction().await?;
        let caller = load_caller(&mut *tx, workspace_id, actor_id).await?;
        let current = find_view(&mut *tx, &caller, workspace_id, view_id).await?;
        if !current.can_edit {
            return Err(TaskError::Forbidden);
        }
        let visibility = input.visibility.unwrap_or(current.visibility);
        if visibility != current.visibility && current.owner.user_id != actor_id {
            return Err(TaskError::Forbidden);
        }
        if input.expected_version != current.version {
            let record = serde_json::to_value(&current).map_err(|_| TaskError::Conflict)?;
            return Err(TaskError::VersionConflict {
                current: Box::new(record),
            });
        }
        let updated = sqlx::query(
            "UPDATE saved_views SET name = ?, description = ?, icon = ?, color = ?, visibility = ?, \
             state_json = COALESCE(?, state_json), version = version + 1, updated_at = ? \
             WHERE id = ? AND workspace_id = ? AND version = ?",
        )
        .bind(name.unwrap_or_else(|| current.name.clone()))
        .bind(description.unwrap_or_else(|| current.description.clone()))
        .bind(icon.unwrap_or_else(|| current.icon.clone()))
        .bind(color.unwrap_or_else(|| current.color.clone()))
        .bind(visibility.as_str())
        .bind(state_json)
        .bind(now.as_millis())
        .bind(view_id.to_string())
        .bind(workspace_id.to_string())
        .bind(current.version)
        .execute(&mut *tx)
        .await?;
        if updated.rows_affected() != 1 {
            return Err(TaskError::Conflict);
        }
        if current.visibility == Visibility::Workspace && visibility == Visibility::Personal {
            sqlx::query("DELETE FROM saved_view_favorites WHERE view_id = ? AND user_id <> ?")
                .bind(view_id.to_string())
                .bind(current.owner.user_id.to_string())
                .execute(&mut *tx)
                .await?;
        }
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "saved_view.updated",
            "saved_view",
            view_id,
            request_id,
            now,
        )
        .await?;
        let record = find_view(&mut *tx, &caller, workspace_id, view_id).await?;
        tx.commit().await?;
        Ok(record)
    }

    pub async fn delete_view(
        &self,
        workspace_id: Id,
        actor_id: Id,
        view_id: Id,
        request_id: &str,
    ) -> Result<(), TaskError> {
        let now = TimestampMillis::now();
        let mut tx = self.database.immediate_transaction().await?;
        let caller = load_caller(&mut *tx, workspace_id, actor_id).await?;
        let current = find_view(&mut *tx, &caller, workspace_id, view_id).await?;
        if !current.can_edit {
            return Err(TaskError::Forbidden);
        }
        // Favorites go with it through `ON DELETE CASCADE`.
        sqlx::query("DELETE FROM saved_views WHERE id = ? AND workspace_id = ?")
            .bind(view_id.to_string())
            .bind(workspace_id.to_string())
            .execute(&mut *tx)
            .await?;
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "saved_view.deleted",
            "saved_view",
            view_id,
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Idempotent: favoriting a favorite (or unfavoriting a non-favorite) changes nothing and is
    /// not broadcast. New favorites go to the end of the caller's list in this workspace.
    pub async fn set_favorite(
        &self,
        workspace_id: Id,
        actor_id: Id,
        view_id: Id,
        favorite: bool,
        request_id: &str,
    ) -> Result<(), TaskError> {
        let now = TimestampMillis::now();
        let mut tx = self.database.immediate_transaction().await?;
        let caller = load_caller(&mut *tx, workspace_id, actor_id).await?;
        let current = find_view(&mut *tx, &caller, workspace_id, view_id).await?;
        if current.is_favorite == favorite {
            return Ok(());
        }
        if favorite {
            sqlx::query(
                "INSERT INTO saved_view_favorites (view_id, user_id, position, created_at) \
                 SELECT ?, ?, COALESCE(MAX(saved_view_favorites.position) + 1, 0), ? \
                 FROM saved_view_favorites \
                 JOIN saved_views ON saved_views.id = saved_view_favorites.view_id \
                 WHERE saved_view_favorites.user_id = ? AND saved_views.workspace_id = ?",
            )
            .bind(view_id.to_string())
            .bind(actor_id.to_string())
            .bind(now.as_millis())
            .bind(actor_id.to_string())
            .bind(workspace_id.to_string())
            .execute(&mut *tx)
            .await?;
        } else {
            sqlx::query("DELETE FROM saved_view_favorites WHERE view_id = ? AND user_id = ?")
                .bind(view_id.to_string())
                .bind(actor_id.to_string())
                .execute(&mut *tx)
                .await?;
        }
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "saved_view.favorites_changed",
            "saved_view",
            view_id,
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// `view_ids` must be a permutation of the caller's visible favorites in this workspace.
    /// Skips the writes and the audit/broadcast when the requested order already matches
    /// (including `[]` for a user with no favorites).
    pub async fn reorder_favorites(
        &self,
        workspace_id: Id,
        actor_id: Id,
        view_ids: Vec<Id>,
        request_id: &str,
    ) -> Result<(), TaskError> {
        let invalid = || TaskError::Invalid { field: "view_ids" };
        let mut requested = view_ids.clone();
        requested.sort_unstable();
        if requested.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(invalid());
        }
        let now = TimestampMillis::now();
        let mut tx = self.database.immediate_transaction().await?;
        load_caller(&mut *tx, workspace_id, actor_id).await?;
        let ordered = sqlx::query_scalar::<_, String>(
            "SELECT saved_view_favorites.view_id FROM saved_view_favorites \
             JOIN saved_views ON saved_views.id = saved_view_favorites.view_id \
             WHERE saved_view_favorites.user_id = ? AND saved_views.workspace_id = ? \
             AND (saved_views.visibility = 'workspace' OR saved_views.owner_user_id = ?) \
             ORDER BY saved_view_favorites.position",
        )
        .bind(actor_id.to_string())
        .bind(workspace_id.to_string())
        .bind(actor_id.to_string())
        .fetch_all(&mut *tx)
        .await?
        .into_iter()
        .map(parse_id)
        .collect::<Result<Vec<_>, _>>()?;
        let mut current = ordered.clone();
        current.sort_unstable();
        if current != requested {
            return Err(invalid());
        }
        if ordered == view_ids {
            // Already in the requested order: nothing to write, nothing to broadcast.
            return Ok(());
        }
        for (position, view_id) in view_ids.iter().enumerate() {
            sqlx::query(
                "UPDATE saved_view_favorites SET position = ? WHERE view_id = ? AND user_id = ?",
            )
            .bind(i64::try_from(position).map_err(|_| invalid())?)
            .bind(view_id.to_string())
            .bind(actor_id.to_string())
            .execute(&mut *tx)
            .await?;
        }
        audit::record(
            &mut tx,
            workspace_id,
            Some(actor_id),
            "saved_view.favorites_changed",
            AuditOutcome::Success,
            "saved_view",
            None,
            request_id,
            json!({}),
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// The caller's saved state for one task page; `NotFound` when none was saved yet.
    pub async fn get_preference(
        &self,
        workspace_id: Id,
        actor_id: Id,
        page_key: &str,
    ) -> Result<ViewPreferenceRecord, TaskError> {
        check_page_key(page_key)?;
        load_caller(self.database.pool(), workspace_id, actor_id).await?;
        let row = sqlx::query(
            "SELECT state_json, updated_at FROM view_preferences \
             WHERE workspace_id = ? AND user_id = ? AND page_key = ?",
        )
        .bind(workspace_id.to_string())
        .bind(actor_id.to_string())
        .bind(page_key)
        .fetch_optional(self.database.pool())
        .await?
        .ok_or(TaskError::NotFound)?;
        let (state, state_error) = decode_state(row.get::<String, _>("state_json").as_str());
        Ok(ViewPreferenceRecord {
            page_key: page_key.to_owned(),
            state,
            state_error,
            updated_at: TimestampMillis::from_millis(row.get("updated_at")),
        })
    }

    /// Upserts the caller's state for one task page. Not audited and not broadcast.
    pub async fn put_preference(
        &self,
        workspace_id: Id,
        actor_id: Id,
        page_key: &str,
        state: ViewState,
    ) -> Result<ViewPreferenceRecord, TaskError> {
        check_page_key(page_key)?;
        let state_json = encode_state(&state)?;
        let now = TimestampMillis::now();
        let mut tx = self.database.immediate_transaction().await?;
        load_caller(&mut *tx, workspace_id, actor_id).await?;
        // Only live projects of this workspace get a row, so made-up ids cannot pile up rows.
        if let Some(project_id) = page_key.strip_prefix("project:") {
            let exists: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM projects \
                 WHERE id = ? AND workspace_id = ? AND deleted_at IS NULL)",
            )
            .bind(project_id)
            .bind(workspace_id.to_string())
            .fetch_one(&mut *tx)
            .await?;
            if !exists {
                return Err(TaskError::NotFound);
            }
        }
        sqlx::query(
            "INSERT INTO view_preferences (workspace_id, user_id, page_key, state_json, updated_at) \
             VALUES (?, ?, ?, ?, ?) \
             ON CONFLICT (workspace_id, user_id, page_key) \
             DO UPDATE SET state_json = excluded.state_json, updated_at = excluded.updated_at",
        )
        .bind(workspace_id.to_string())
        .bind(actor_id.to_string())
        .bind(page_key)
        .bind(state_json)
        .bind(now.as_millis())
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(ViewPreferenceRecord {
            page_key: page_key.to_owned(),
            state: Some(state),
            state_error: None,
            updated_at: now,
        })
    }
}

/// The acting member. `manager` is a workspace owner or admin.
struct Caller {
    id: Id,
    manager: bool,
}

/// Same membership rule as `require_access`: non-members and deleted workspaces are `NotFound`.
async fn load_caller<'e, E>(
    executor: E,
    workspace_id: Id,
    actor_id: Id,
) -> Result<Caller, TaskError>
where
    E: sqlx::Executor<'e, Database = Sqlite>,
{
    let role: String = sqlx::query_scalar(
        "SELECT memberships.role FROM memberships JOIN workspaces ON workspaces.id = memberships.workspace_id \
         WHERE memberships.workspace_id = ? AND memberships.user_id = ? AND workspaces.deleted_at IS NULL",
    )
    .bind(workspace_id.to_string())
    .bind(actor_id.to_string())
    .fetch_optional(executor)
    .await?
    .ok_or(TaskError::NotFound)?;
    Ok(Caller {
        id: actor_id,
        manager: role != "member",
    })
}

/// Views the caller can see. Binds, in order: caller id, workspace id, caller id.
fn visible_views_sql(tail: &str) -> String {
    format!(
        "SELECT saved_views.id, saved_views.workspace_id, saved_views.owner_user_id, \
         users.display_name AS owner_display_name, saved_views.name, saved_views.description, \
         saved_views.icon, saved_views.color, saved_views.visibility, saved_views.state_json, \
         saved_views.version, saved_views.created_at, saved_views.updated_at, \
         saved_view_favorites.position AS favorite_position \
         FROM saved_views JOIN users ON users.id = saved_views.owner_user_id \
         LEFT JOIN saved_view_favorites ON saved_view_favorites.view_id = saved_views.id \
         AND saved_view_favorites.user_id = ? \
         WHERE saved_views.workspace_id = ? \
         AND (saved_views.visibility = 'workspace' OR saved_views.owner_user_id = ?) {tail}"
    )
}

async fn find_view<'e, E>(
    executor: E,
    caller: &Caller,
    workspace_id: Id,
    view_id: Id,
) -> Result<SavedViewRecord, TaskError>
where
    E: sqlx::Executor<'e, Database = Sqlite>,
{
    let sql = visible_views_sql("AND saved_views.id = ?");
    let row = sqlx::query(&sql)
        .bind(caller.id.to_string())
        .bind(workspace_id.to_string())
        .bind(caller.id.to_string())
        .bind(view_id.to_string())
        .fetch_optional(executor)
        .await?
        .ok_or(TaskError::NotFound)?;
    view_from_row(row, caller)
}

fn view_from_row(row: SqliteRow, caller: &Caller) -> Result<SavedViewRecord, TaskError> {
    let owner_id = parse_id(row.get("owner_user_id"))?;
    let visibility = Visibility::from_db(row.get::<String, _>("visibility").as_str())?;
    let (state, state_error) = decode_state(row.get::<String, _>("state_json").as_str());
    let favorite_position: Option<i64> = row.get("favorite_position");
    Ok(SavedViewRecord {
        id: parse_id(row.get("id"))?,
        workspace_id: parse_id(row.get("workspace_id"))?,
        owner: ViewOwner {
            user_id: owner_id,
            display_name: row.get("owner_display_name"),
        },
        name: row.get("name"),
        description: row.get("description"),
        icon: row.get("icon"),
        color: row.get("color"),
        visibility,
        state,
        state_error,
        version: row.get("version"),
        is_favorite: favorite_position.is_some(),
        favorite_position,
        can_edit: owner_id == caller.id || (visibility == Visibility::Workspace && caller.manager),
        created_at: TimestampMillis::from_millis(row.get("created_at")),
        updated_at: TimestampMillis::from_millis(row.get("updated_at")),
    })
}

/// Stored state that no longer parses is reported, not fatal: the client falls back to defaults.
fn decode_state(json: &str) -> (Option<ViewState>, Option<String>) {
    match parse_view_state(json) {
        Ok(state) => (Some(state), None),
        Err(error) if error.path.is_empty() => (None, Some(error.message.to_owned())),
        Err(error) => (None, Some(error.to_string())),
    }
}

fn encode_state(state: &ViewState) -> Result<String, TaskError> {
    validate_view_state(state)?;
    serde_json::to_string(state).map_err(|_| TaskError::Invalid { field: "state" })
}

/// `all` | `project:<uuidv7>` | `preset:(mine|overdue|due_soon|current_week|my_week)`.
fn check_page_key(page_key: &str) -> Result<(), TaskError> {
    let valid = match page_key.split_once(':') {
        None => page_key == "all",
        Some(("project", id)) => id.parse::<Id>().is_ok(),
        Some(("preset", preset)) => preset_filter(preset).is_some(),
        Some(_) => false,
    };
    if valid {
        Ok(())
    } else {
        Err(TaskError::Invalid { field: "page_key" })
    }
}

fn clean_name(name: &str) -> Result<String, TaskError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > NAME_MAX_CHARS {
        Err(TaskError::Invalid { field: "name" })
    } else {
        Ok(name.to_owned())
    }
}

fn clean_description(description: String) -> Result<String, TaskError> {
    if description.chars().count() > DESCRIPTION_MAX_CHARS {
        Err(TaskError::Invalid {
            field: "description",
        })
    } else {
        Ok(description)
    }
}

fn clean_icon(icon: Option<String>) -> Result<Option<String>, TaskError> {
    icon.map(|icon| {
        let valid = !icon.is_empty()
            && icon.len() <= ICON_MAX_BYTES
            && icon
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_');
        if valid {
            Ok(icon)
        } else {
            Err(TaskError::Invalid { field: "icon" })
        }
    })
    .transpose()
}

fn clean_color(color: Option<String>) -> Result<Option<String>, TaskError> {
    color
        .map(|color| {
            if color.len() == 7
                && color.starts_with('#')
                && color[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
            {
                Ok(color.to_ascii_lowercase())
            } else {
                Err(TaskError::Invalid { field: "color" })
            }
        })
        .transpose()
}
