//! Custom task fields (issue #87): the workspace's field gallery, the fields each project shows,
//! and one value per task and field.

use std::collections::{BTreeSet, HashMap, HashSet};

use orbit_domain::{Actor, Permission};
use orbit_platform::{Id, Row, SqlBuilder, TimestampMillis};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{Any, Transaction};
use utoipa::ToSchema;

use crate::repositories::task_filter::{CustomFieldSpec, CustomFields};

use super::{
    TaskError, TaskRepository, check_version, is_unique_violation, parse_id, parse_version,
    require_access, require_access_tx, require_manager_tx, require_project,
    require_task_tx,
};

pub const MAX_FIELDS: i64 = 200;
pub const MAX_OPTIONS: usize = 100;
pub const MAX_TEXT_CHARS: usize = 2_000;
const MAX_URL_BYTES: usize = 2_048;
/// Larger numbers lose precision in browser JSON numbers.
const MAX_NUMBER: f64 = 1e15;
pub const TIME_TRACKING_EXISTS: &str = "The workspace already has a Time tracking field.";
pub const TIME_TRACKING_VALUE: &str =
    "A Time tracking field shows the time logged on the task. Log time instead of setting a value.";
pub const TIME_TRACKING_OFF: &str =
    "Time tracking is off in this project. Add the Time tracking field to the project.";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
#[schema(as = TaskFieldKind)]
pub enum TaskFieldKind {
    Text,
    Number,
    /// A calendar day, `YYYY-MM-DD`.
    Date,
    Select,
    MultiSelect,
    Checkbox,
    Url,
    /// The time logged on the task, in minutes. A workspace has at most one; it stores no
    /// values, and a project that shows it lets its tasks take new time entries.
    TimeTracking,
}

impl TaskFieldKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Number => "number",
            Self::Date => "date",
            Self::Select => "select",
            Self::MultiSelect => "multi_select",
            Self::Checkbox => "checkbox",
            Self::Url => "url",
            Self::TimeTracking => "time_tracking",
        }
    }

    fn parse(value: &str) -> Result<Self, TaskError> {
        serde_json::from_value(Value::String(value.to_owned())).map_err(|_| TaskError::Conflict)
    }

    pub fn has_options(self) -> bool {
        matches!(self, Self::Select | Self::MultiSelect)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
#[schema(as = TaskFieldOption)]
pub struct TaskFieldOption {
    pub id: String,
    pub name: String,
    /// `#rrggbb`.
    pub color: String,
}

/// An option of a create or update body: an `id` of the field's option keeps that option (and the
/// values that point at it); no `id` adds a new option.
#[derive(Clone, Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
#[schema(as = TaskFieldOptionInput)]
pub struct TaskFieldOptionInput {
    pub id: Option<String>,
    pub name: String,
    pub color: String,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[schema(as = TaskField)]
pub struct TaskFieldRecord {
    #[schema(value_type = String)]
    pub id: Id,
    #[schema(value_type = String)]
    pub workspace_id: Id,
    pub name: String,
    pub kind: TaskFieldKind,
    /// The choices of a `select` or `multi_select` field; empty for other kinds.
    pub options: Vec<TaskFieldOption>,
    #[schema(value_type = Option<String>, required = true)]
    pub created_by: Option<Id>,
    /// Projects that show the field.
    pub project_count: i64,
    /// The caller may rename, change or delete the field (its creator, or an owner or admin).
    pub can_manage: bool,
    pub version: u64,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: TimestampMillis,
    #[schema(value_type = String, format = DateTime)]
    pub updated_at: TimestampMillis,
}

/// A task's value of one field. `value` is a string (`text`, `url`, `YYYY-MM-DD` for `date`, the
/// option id for `select`), a number, `true` (`checkbox`) or option ids (`multi_select`).
#[derive(Clone, Debug, Serialize, ToSchema)]
#[schema(as = TaskFieldValue)]
pub struct TaskFieldValue {
    #[schema(value_type = String)]
    pub field_id: Id,
    pub value: Value,
}

const FIELD_COLUMNS: &str = "task_fields.id, task_fields.workspace_id, task_fields.name, \
     task_fields.kind, task_fields.options_json, task_fields.created_by, task_fields.version, \
     task_fields.created_at, task_fields.updated_at, \
     (SELECT COUNT(*) FROM project_task_fields WHERE project_task_fields.field_id = task_fields.id) AS project_count";

impl TaskRepository {
    /// The workspace's field gallery, by name.
    pub async fn task_fields(
        &self,
        workspace_id: Id,
        actor_id: Id,
    ) -> Result<Vec<TaskFieldRecord>, TaskError> {
        let actor = require_access(self.database.pool(), workspace_id, actor_id).await?;
        // a guest only reaches chat
        if actor.is_guest() {
            return Err(TaskError::Forbidden);
        }
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT {FIELD_COLUMNS} FROM task_fields WHERE workspace_id = $1 \
             ORDER BY lower(task_fields.name), task_fields.id"
        )))
        .bind(workspace_id.to_string())
        .fetch_all(self.database.pool())
        .await?
        .into_iter()
        .map(|row| field_from_row(row, &actor))
        .collect()
    }

    /// Adds a field to the gallery. Any member but a guest may; a name the workspace already has
    /// (case-insensitive) is a `Conflict`.
    pub async fn create_task_field(
        &self,
        workspace_id: Id,
        actor_id: Id,
        name: String,
        kind: TaskFieldKind,
        options: Vec<TaskFieldOptionInput>,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<TaskFieldRecord, TaskError> {
        let options = checked_options(kind, options, &[])?;
        let id = Id::new_v7();
        let mut tx = self.database.workspace_transaction(workspace_id).await?;
        let actor = require_access_tx(&mut tx, workspace_id, actor_id).await?;
        if actor.is_guest() {
            return Err(TaskError::Forbidden);
        }
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM task_fields WHERE workspace_id = $1")
            .bind(workspace_id.to_string())
            .fetch_one(&mut *tx)
            .await?;
        if count >= MAX_FIELDS {
            return Err(TaskError::Invalid { field: "fields" });
        }
        if kind == TaskFieldKind::TimeTracking && time_tracking_field(&mut tx, workspace_id).await?.is_some() {
            return Err(TaskError::Refused(TIME_TRACKING_EXISTS));
        }
        let inserted = sqlx::query(
            "INSERT INTO task_fields (id, workspace_id, name, kind, options_json, created_by, version, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6, 0, $7, $8)",
        )
        .bind(id.to_string())
        .bind(workspace_id.to_string())
        .bind(&name)
        .bind(kind.as_str())
        .bind(options_json(&options))
        .bind(actor_id.to_string())
        .bind(now.as_millis())
        .bind(now.as_millis())
        .execute(&mut *tx)
        .await;
        if is_unique_violation(&inserted) {
            return Err(TaskError::Conflict);
        }
        inserted?;
        self.record_write(&mut tx, workspace_id, actor_id, ("task_field.created", "task_field"), id, request_id, json!({}), now).await?;
        let record = field_in_tx(&mut tx, workspace_id, id, &actor).await?;
        tx.commit().await?;
        Ok(record)
    }

    /// Renames the field or replaces its options; the kind never changes. Values that point at a
    /// removed option stop showing it.
    #[allow(clippy::too_many_arguments)]
    pub async fn update_task_field(
        &self,
        workspace_id: Id,
        field_id: Id,
        actor_id: Id,
        name: Option<String>,
        options: Option<Vec<TaskFieldOptionInput>>,
        expected_version: u64,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<TaskFieldRecord, TaskError> {
        let mut tx = self.database.workspace_transaction(workspace_id).await?;
        let actor = require_access_tx(&mut tx, workspace_id, actor_id).await?;
        let current = field_in_tx(&mut tx, workspace_id, field_id, &actor).await?;
        if !current.can_manage {
            return Err(TaskError::Forbidden);
        }
        check_version(expected_version, current.version, &current)?;
        let options = match options {
            Some(options) => checked_options(current.kind, options, &current.options)?,
            None => current.options,
        };
        let updated = sqlx::query(
            "UPDATE task_fields SET name = $1, options_json = $2, version = version + 1, updated_at = $3 \
             WHERE id = $4 AND workspace_id = $5 AND version = $6",
        )
        .bind(name.unwrap_or(current.name))
        .bind(options_json(&options))
        .bind(now.as_millis())
        .bind(field_id.to_string())
        .bind(workspace_id.to_string())
        .bind(expected_version as i64)
        .execute(&mut *tx)
        .await;
        if is_unique_violation(&updated) {
            return Err(TaskError::Conflict);
        }
        updated?;
        self.record_write(&mut tx, workspace_id, actor_id, ("task_field.updated", "task_field"), field_id, request_id, json!({}), now).await?;
        let record = field_in_tx(&mut tx, workspace_id, field_id, &actor).await?;
        tx.commit().await?;
        Ok(record)
    }

    /// Deletes the field, its place in every project and every task's value of it.
    pub async fn delete_task_field(
        &self,
        workspace_id: Id,
        field_id: Id,
        actor_id: Id,
        expected_version: u64,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), TaskError> {
        let mut tx = self.database.workspace_transaction(workspace_id).await?;
        let actor = require_access_tx(&mut tx, workspace_id, actor_id).await?;
        let current = field_in_tx(&mut tx, workspace_id, field_id, &actor).await?;
        if !current.can_manage {
            return Err(TaskError::Forbidden);
        }
        check_version(expected_version, current.version, &current)?;
        sqlx::query("DELETE FROM task_fields WHERE id = $1 AND workspace_id = $2 AND version = $3")
            .bind(field_id.to_string())
            .bind(workspace_id.to_string())
            .bind(expected_version as i64)
            .execute(&mut *tx)
            .await?;
        self.record_write(&mut tx, workspace_id, actor_id, ("task_field.deleted", "task_field"), field_id, request_id, json!({}), now).await?;
        tx.commit().await?;
        Ok(())
    }

    /// The fields the project shows, in its order.
    pub async fn project_task_fields(
        &self,
        workspace_id: Id,
        project_id: Id,
        actor_id: Id,
    ) -> Result<Vec<TaskFieldRecord>, TaskError> {
        require_project(self.database.pool(), workspace_id, project_id, actor_id).await?;
        let actor = require_access(self.database.pool(), workspace_id, actor_id).await?;
        sqlx::query(sqlx::AssertSqlSafe(project_fields_sql()))
            .bind(workspace_id.to_string())
            .bind(project_id.to_string())
            .fetch_all(self.database.pool())
            .await?
            .into_iter()
            .map(|row| field_from_row(row, &actor))
            .collect()
    }

    /// Sets the fields the project shows and their order (`field_ids`, gallery fields of the
    /// workspace). Only a project manager may.
    pub async fn set_project_task_fields(
        &self,
        workspace_id: Id,
        project_id: Id,
        actor_id: Id,
        field_ids: &[Id],
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<Vec<TaskFieldRecord>, TaskError> {
        if field_ids.len() > MAX_FIELDS as usize
            || field_ids.iter().collect::<HashSet<_>>().len() != field_ids.len()
        {
            return Err(TaskError::Invalid { field: "field_ids" });
        }
        let mut tx = self.database.workspace_transaction(workspace_id).await?;
        require_manager_tx(&mut tx, workspace_id, project_id, actor_id).await?;
        let actor = require_access_tx(&mut tx, workspace_id, actor_id).await?;
        if !field_ids.is_empty() {
            let mut query = SqlBuilder::new("SELECT COUNT(*) FROM task_fields WHERE workspace_id = ");
            query.push_bind(workspace_id.to_string()).push(" AND id IN (");
            let mut ids = query.separated(", ");
            for id in field_ids {
                ids.push_bind(id.to_string());
            }
            query.push(")");
            let found: i64 = query.build().fetch_one(&mut *tx).await?.get(0);
            if found != field_ids.len() as i64 {
                return Err(TaskError::Invalid { field: "field_ids" });
            }
        }
        sqlx::query("DELETE FROM project_task_fields WHERE project_id = $1")
            .bind(project_id.to_string())
            .execute(&mut *tx)
            .await?;
        if !field_ids.is_empty() {
            let mut insert = SqlBuilder::new(
                "INSERT INTO project_task_fields (project_id, field_id, position) VALUES ",
            );
            for (position, id) in field_ids.iter().enumerate() {
                if position > 0 {
                    insert.push(", ");
                }
                insert.push("(").push_bind(project_id.to_string()).push(", ");
                insert.push_bind(id.to_string()).push(", ");
                insert.push_bind(position as i64).push(")");
            }
            insert.build().execute(&mut *tx).await?;
        }
        self.record_write(&mut tx, workspace_id, actor_id, ("project.fields_updated", "project"), project_id, request_id, json!({}), now).await?;
        let records = sqlx::query(sqlx::AssertSqlSafe(project_fields_sql()))
            .bind(workspace_id.to_string())
            .bind(project_id.to_string())
            .fetch_all(&mut *tx)
            .await?
            .into_iter()
            .map(|row| field_from_row(row, &actor))
            .collect::<Result<Vec<_>, _>>()?;
        tx.commit().await?;
        Ok(records)
    }

    /// The task's values of every gallery field, also of fields its project does not show.
    pub async fn task_field_values(
        &self,
        workspace_id: Id,
        task_id: Id,
        actor_id: Id,
    ) -> Result<Vec<TaskFieldValue>, TaskError> {
        let mut tx = self.database.pool().begin().await?;
        require_task_tx(&mut tx, workspace_id, task_id, actor_id).await?;
        let values = values_in_tx(&mut tx, task_id).await?;
        tx.commit().await?;
        Ok(values)
    }

    /// Sets (or with `null`, clears) the task's value of a field that its project shows; returns
    /// all the task's values.
    #[allow(clippy::too_many_arguments)]
    pub async fn set_task_field_value(
        &self,
        workspace_id: Id,
        task_id: Id,
        field_id: Id,
        actor_id: Id,
        value: Value,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<Vec<TaskFieldValue>, TaskError> {
        let mut tx = self.database.workspace_transaction(workspace_id).await?;
        let actor = require_task_tx(&mut tx, workspace_id, task_id, actor_id).await?;
        let shown: Option<String> = sqlx::query_scalar(
            "SELECT task_fields.id FROM task_fields \
             JOIN project_task_fields ON project_task_fields.field_id = task_fields.id \
             JOIN tasks ON tasks.project_id = project_task_fields.project_id \
             WHERE task_fields.id = $1 AND task_fields.workspace_id = $2 AND tasks.id = $3",
        )
        .bind(field_id.to_string())
        .bind(workspace_id.to_string())
        .bind(task_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        if shown.is_none() {
            return Err(TaskError::NotFound);
        }
        let field = field_in_tx(&mut tx, workspace_id, field_id, &actor).await?;
        match checked_value(field.kind, &field.options, value)? {
            Some(value) => {
                sqlx::query(
                    "INSERT INTO task_field_values (task_id, field_id, value_json, updated_at) VALUES ($1, $2, $3, $4) \
                     ON CONFLICT (task_id, field_id) DO UPDATE SET value_json = excluded.value_json, updated_at = excluded.updated_at",
                )
                .bind(task_id.to_string())
                .bind(field_id.to_string())
                .bind(value.to_string())
                .bind(now.as_millis())
                .execute(&mut *tx)
                .await?;
            }
            None => {
                sqlx::query("DELETE FROM task_field_values WHERE task_id = $1 AND field_id = $2")
                    .bind(task_id.to_string())
                    .bind(field_id.to_string())
                    .execute(&mut *tx)
                    .await?;
            }
        }
        // As every task write: a service-account token is the actor, not the member behind it.
        self.record_write(
            &mut tx,
            workspace_id,
            actor_id,
            ("task.field_changed", "task"),
            task_id,
            request_id,
            json!({ "field_id": field_id.to_string(), "field_name": field.name }),
            now,
        )
        .await?;
        let values = values_in_tx(&mut tx, task_id).await?;
        tx.commit().await?;
        Ok(values)
    }
}

/// An option of a field that an import brings: a trimmed name of 1 to 100 characters and a
/// `#rrggbb` color (the importer checks both).
#[derive(Clone, Debug)]
pub struct ImportedOption {
    pub name: String,
    pub color: String,
}

/// Suffixes " (2)" to " (N)" an imported field may take when its name is taken by a field of
/// another kind.
const MAX_NAME_SUFFIX: usize = 50;

impl TaskRepository {
    /// The gallery field an import uses for a source field: `linked` (the field an earlier
    /// import made) when it still exists with the same kind, else the field with the same name
    /// (case-insensitive) and kind, else a new field named `name`, or `name (2)`, `name (3)`, …
    /// when a field of another kind holds the name. Options the field lacks (by name,
    /// case-insensitive) are appended up to [`MAX_OPTIONS`]. `None` when the gallery is full or
    /// no free name was found. One short write transaction.
    #[allow(clippy::too_many_arguments)]
    pub async fn import_task_field(
        &self,
        workspace_id: Id,
        actor_id: Id,
        linked: Option<Id>,
        name: &str,
        kind: TaskFieldKind,
        options: &[ImportedOption],
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<Option<TaskFieldRecord>, TaskError> {
        let options: &[ImportedOption] = if kind.has_options() { options } else { &[] };
        let mut tx = self.database.workspace_transaction(workspace_id).await?;
        let actor = require_access_tx(&mut tx, workspace_id, actor_id).await?;
        if actor.is_guest() {
            return Err(TaskError::Forbidden);
        }
        let gallery: Vec<(String, String, String)> =
            sqlx::query("SELECT id, name, kind FROM task_fields WHERE workspace_id = $1")
                .bind(workspace_id.to_string())
                .fetch_all(&mut *tx)
                .await?
                .into_iter()
                .map(|row| (row.get("id"), row.get("name"), row.get("kind")))
                .collect();
        let same_kind = |(_, _, field_kind): &&(String, String, String)| field_kind == kind.as_str();
        let existing = if kind == TaskFieldKind::TimeTracking {
            // The workspace's only one, whatever its name.
            gallery.iter().find(same_kind)
        } else {
            linked
                .and_then(|linked| {
                    gallery
                        .iter()
                        .filter(same_kind)
                        .find(|(id, _, _)| *id == linked.to_string())
                })
                .or_else(|| {
                    gallery
                        .iter()
                        .filter(same_kind)
                        .find(|(_, field_name, _)| field_name.to_lowercase() == name.to_lowercase())
                })
        }
        .map(|(id, _, _)| id.clone());
        let field_id = match existing {
            Some(id) => {
                let field_id = parse_id(id)?;
                let current = field_in_tx(&mut tx, workspace_id, field_id, &actor).await?;
                let mut merged = current.options.clone();
                for option in options {
                    if merged.len() >= MAX_OPTIONS {
                        break;
                    }
                    if !merged.iter().any(|known| known.name.to_lowercase() == option.name.to_lowercase()) {
                        merged.push(TaskFieldOption {
                            id: Id::new_v7().to_string(),
                            name: option.name.clone(),
                            color: option.color.clone(),
                        });
                    }
                }
                if merged.len() != current.options.len() {
                    sqlx::query(
                        "UPDATE task_fields SET options_json = $1, version = version + 1, updated_at = $2 \
                         WHERE id = $3 AND workspace_id = $4",
                    )
                    .bind(options_json(&merged))
                    .bind(now.as_millis())
                    .bind(field_id.to_string())
                    .bind(workspace_id.to_string())
                    .execute(&mut *tx)
                    .await?;
                    self.record_write(&mut tx, workspace_id, actor_id, ("task_field.updated", "task_field"), field_id, request_id, json!({}), now).await?;
                }
                field_id
            }
            None => {
                if gallery.len() as i64 >= MAX_FIELDS {
                    return Ok(None);
                }
                let taken: HashSet<String> =
                    gallery.iter().map(|(_, field_name, _)| field_name.to_lowercase()).collect();
                let free = std::iter::once(name.to_owned())
                    .chain((2..=MAX_NAME_SUFFIX).map(|number| {
                        let suffix = format!(" ({number})");
                        let base: String =
                            name.chars().take(100 - suffix.chars().count()).collect();
                        format!("{}{suffix}", base.trim_end())
                    }))
                    .find(|candidate| !taken.contains(&candidate.to_lowercase()));
                let Some(free) = free else {
                    return Ok(None);
                };
                let inputs = options
                    .iter()
                    .take(MAX_OPTIONS)
                    .map(|option| TaskFieldOptionInput {
                        id: None,
                        name: option.name.clone(),
                        color: option.color.clone(),
                    })
                    .collect();
                let options = checked_options(kind, inputs, &[])?;
                let field_id = Id::new_v7();
                sqlx::query(
                    "INSERT INTO task_fields (id, workspace_id, name, kind, options_json, created_by, version, created_at, updated_at) \
                     VALUES ($1, $2, $3, $4, $5, $6, 0, $7, $8)",
                )
                .bind(field_id.to_string())
                .bind(workspace_id.to_string())
                .bind(&free)
                .bind(kind.as_str())
                .bind(options_json(&options))
                .bind(actor_id.to_string())
                .bind(now.as_millis())
                .bind(now.as_millis())
                .execute(&mut *tx)
                .await?;
                self.record_write(&mut tx, workspace_id, actor_id, ("task_field.created", "task_field"), field_id, request_id, json!({}), now).await?;
                field_id
            }
        };
        let record = field_in_tx(&mut tx, workspace_id, field_id, &actor).await?;
        tx.commit().await?;
        Ok(Some(record))
    }

    /// Turns time tracking on in the project (for imported time entries and the development
    /// seed): adds the workspace's time tracking field to it, after creating the field when the
    /// workspace has none. Does nothing more when it is on already, or when the gallery is full.
    pub async fn turn_on_time_tracking(
        &self,
        workspace_id: Id,
        project_id: Id,
        actor_id: Id,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), TaskError> {
        let field = self
            .import_task_field(workspace_id, actor_id, None, "Time tracking", TaskFieldKind::TimeTracking, &[], request_id, now)
            .await?;
        if let Some(field) = field {
            self.import_project_task_fields(workspace_id, project_id, &[field.id]).await?;
        }
        Ok(())
    }

    /// Appends the gallery fields among `field_ids` that the project does not show yet, in
    /// order, after its own (an import never reorders or removes a project's fields).
    pub async fn import_project_task_fields(
        &self,
        workspace_id: Id,
        project_id: Id,
        field_ids: &[Id],
    ) -> Result<(), TaskError> {
        if field_ids.is_empty() {
            return Ok(());
        }
        let mut tx = self.database.workspace_transaction(workspace_id).await?;
        let shown: Vec<String> = sqlx::query_scalar(
            "SELECT project_task_fields.field_id FROM project_task_fields \
             JOIN projects ON projects.id = project_task_fields.project_id \
             WHERE project_task_fields.project_id = $1 AND projects.workspace_id = $2",
        )
        .bind(project_id.to_string())
        .bind(workspace_id.to_string())
        .fetch_all(&mut *tx)
        .await?;
        let position: Option<i64> =
            sqlx::query_scalar("SELECT MAX(position) FROM project_task_fields WHERE project_id = $1")
                .bind(project_id.to_string())
                .fetch_one(&mut *tx)
                .await?;
        let mut seen: HashSet<String> = shown.into_iter().collect();
        let added: Vec<Id> = field_ids
            .iter()
            .filter(|id| seen.insert(id.to_string()))
            .take(MAX_FIELDS as usize)
            .copied()
            .collect();
        if added.is_empty() {
            return Ok(());
        }
        let start = position.map_or(0, |position| position + 1);
        // Only fields of this workspace, in a project of this workspace.
        let mut insert = SqlBuilder::new(
            "INSERT INTO project_task_fields (project_id, field_id, position) \
             SELECT projects.id, task_fields.id, added.position FROM (",
        );
        for (offset, id) in added.iter().enumerate() {
            insert.push(if offset == 0 { "SELECT " } else { " UNION ALL SELECT " });
            insert.push("CAST(").push_bind(id.to_string()).push(" AS TEXT) AS field_id, CAST(");
            insert.push_bind(start + offset as i64).push(" AS BIGINT) AS position");
        }
        insert.push(") added JOIN task_fields ON task_fields.id = added.field_id AND task_fields.workspace_id = ");
        insert.push_bind(workspace_id.to_string());
        insert.push(" JOIN projects ON projects.id = ").push_bind(project_id.to_string());
        insert.push(" AND projects.workspace_id = ").push_bind(workspace_id.to_string());
        // `WHERE` keeps SQLite from reading `ON CONFLICT` as part of the join.
        insert.push(" WHERE 1 = 1 ON CONFLICT (project_id, field_id) DO NOTHING");
        insert.build().execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Stores an imported task's values (each already checked with [`checked_value`] against
    /// its field); a value the task already has stays. At most [`MAX_FIELDS`] values, in one
    /// statement.
    pub async fn import_task_field_values(
        &self,
        workspace_id: Id,
        task_id: Id,
        values: &[(Id, Value)],
        now: TimestampMillis,
    ) -> Result<(), TaskError> {
        let values = &values[..values.len().min(MAX_FIELDS as usize)];
        if values.is_empty() {
            return Ok(());
        }
        let mut tx = self.database.workspace_transaction(workspace_id).await?;
        // Only fields of this workspace, for a task of this workspace.
        let mut insert = SqlBuilder::new(
            "INSERT INTO task_field_values (task_id, field_id, value_json, updated_at) \
             SELECT tasks.id, task_fields.id, added.value_json, added.updated_at FROM (",
        );
        for (offset, (field_id, value)) in values.iter().enumerate() {
            insert.push(if offset == 0 { "SELECT " } else { " UNION ALL SELECT " });
            insert.push("CAST(").push_bind(field_id.to_string()).push(" AS TEXT) AS field_id, CAST(");
            insert.push_bind(value.to_string()).push(" AS TEXT) AS value_json, CAST(");
            insert.push_bind(now.as_millis()).push(" AS BIGINT) AS updated_at");
        }
        insert.push(") added JOIN task_fields ON task_fields.id = added.field_id AND task_fields.workspace_id = ");
        insert.push_bind(workspace_id.to_string());
        insert.push(" JOIN tasks ON tasks.id = ").push_bind(task_id.to_string());
        insert.push(" AND tasks.workspace_id = ").push_bind(workspace_id.to_string());
        insert.push(" WHERE 1 = 1 ON CONFLICT (task_id, field_id) DO NOTHING");
        insert.build().execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }
}

fn project_fields_sql() -> String {
    format!(
        "SELECT {FIELD_COLUMNS} FROM task_fields \
         JOIN project_task_fields ON project_task_fields.field_id = task_fields.id \
         WHERE task_fields.workspace_id = $1 AND project_task_fields.project_id = $2 \
         ORDER BY project_task_fields.position, task_fields.id"
    )
}

async fn field_in_tx(
    tx: &mut Transaction<'_, Any>,
    workspace_id: Id,
    field_id: Id,
    actor: &Actor,
) -> Result<TaskFieldRecord, TaskError> {
    let row = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT {FIELD_COLUMNS} FROM task_fields WHERE task_fields.id = $1 AND task_fields.workspace_id = $2"
    )))
    .bind(field_id.to_string())
    .bind(workspace_id.to_string())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(TaskError::NotFound)?;
    field_from_row(row, actor)
}

/// The task's values; options removed from their field are dropped.
async fn values_in_tx(
    tx: &mut Transaction<'_, Any>,
    task_id: Id,
) -> Result<Vec<TaskFieldValue>, TaskError> {
    Ok(values_by_task(&mut **tx, &[task_id])
        .await?
        .remove(&task_id)
        .unwrap_or_default())
}

/// The workspace's time tracking field.
async fn time_tracking_field(
    tx: &mut Transaction<'_, Any>,
    workspace_id: Id,
) -> Result<Option<String>, TaskError> {
    Ok(sqlx::query_scalar("SELECT id FROM task_fields WHERE workspace_id = $1 AND kind = 'time_tracking'")
        .bind(workspace_id.to_string())
        .fetch_optional(&mut **tx)
        .await?)
}

/// Whether the project shows the time tracking field, so its tasks take new time entries.
pub(in crate::repositories) async fn time_tracking_on(
    tx: &mut Transaction<'_, Any>,
    project_id: Id,
) -> Result<bool, TaskError> {
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM project_task_fields \
         JOIN task_fields ON task_fields.id = project_task_fields.field_id \
         WHERE project_task_fields.project_id = $1 AND task_fields.kind = 'time_tracking'",
    )
    .bind(project_id.to_string())
    .fetch_one(&mut **tx)
    .await?;
    Ok(count > 0)
}

/// The values of each of `task_ids` (at most a page of tasks) in one query, by field id;
/// options removed from their field are dropped. Tasks without values are missing. The time
/// tracking field's value is the task's own logged minutes, also when its project does not
/// show the field (as a stored value stays when its task moves).
pub(in crate::repositories) async fn values_by_task<'e, E>(
    executor: E,
    task_ids: &[Id],
) -> Result<HashMap<Id, Vec<TaskFieldValue>>, TaskError>
where
    E: sqlx::Executor<'e, Database = Any>,
{
    let mut by_task: HashMap<Id, Vec<TaskFieldValue>> = HashMap::new();
    if task_ids.is_empty() {
        return Ok(by_task);
    }
    let mut query = SqlBuilder::new(
        "SELECT task_field_values.task_id AS task_id, task_field_values.field_id AS field_id, \
         task_field_values.value_json AS value_json, task_fields.kind AS kind, task_fields.options_json AS options_json \
         FROM task_field_values JOIN task_fields ON task_fields.id = task_field_values.field_id \
         WHERE task_field_values.task_id IN (",
    );
    let mut ids = query.separated(", ");
    for id in task_ids {
        ids.push_bind(id.to_string());
    }
    query.push(
        ") UNION ALL SELECT task_time_entries.task_id, task_fields.id, \
         CAST(CAST(SUM(task_time_entries.minutes) AS BIGINT) AS TEXT), task_fields.kind, task_fields.options_json \
         FROM task_time_entries JOIN task_fields ON task_fields.workspace_id = task_time_entries.workspace_id \
         AND task_fields.kind = 'time_tracking' WHERE task_time_entries.task_id IN (",
    );
    let mut ids = query.separated(", ");
    for id in task_ids {
        ids.push_bind(id.to_string());
    }
    query.push(
        ") GROUP BY task_time_entries.task_id, task_fields.id, task_fields.kind, task_fields.options_json \
         ORDER BY task_id, field_id",
    );
    let rows = query.build().fetch_all(executor).await?;
    let mut options_by_field: HashMap<String, Vec<TaskFieldOption>> = HashMap::new();
    for row in rows {
        let field_id: String = row.get("field_id");
        let kind = TaskFieldKind::parse(row.get::<String, _>("kind").as_str())?;
        if !options_by_field.contains_key(&field_id) {
            let options = parse_options(&row.get::<String, _>("options_json"))?;
            options_by_field.insert(field_id.clone(), options);
        }
        let options = &options_by_field[&field_id];
        let value: Value = serde_json::from_str(&row.get::<String, _>("value_json"))
            .map_err(|_| TaskError::Conflict)?;
        if let Some(value) = live_value(kind, options, value) {
            by_task
                .entry(parse_id(row.get("task_id"))?)
                .or_default()
                .push(TaskFieldValue {
                    field_id: parse_id(field_id)?,
                    value,
                });
        }
    }
    Ok(by_task)
}

/// The kind and option order of the workspace's fields among `field_ids`, for filters and
/// sorting. Ids that are not (or no longer) fields of the workspace are left out, and so is the
/// time tracking field: it has no stored value to filter or sort by.
pub(in crate::repositories) async fn custom_field_specs(
    pool: &sqlx::AnyPool,
    workspace_id: Id,
    field_ids: &BTreeSet<Id>,
) -> Result<CustomFields, TaskError> {
    let mut specs = CustomFields::new();
    if field_ids.is_empty() {
        return Ok(specs);
    }
    let mut query =
        SqlBuilder::new("SELECT id, kind, options_json FROM task_fields WHERE workspace_id = ");
    query
        .push_bind(workspace_id.to_string())
        .push(" AND kind <> 'time_tracking' AND id IN (");
    let mut ids = query.separated(", ");
    for id in field_ids {
        ids.push_bind(id.to_string());
    }
    query.push(")");
    for row in query.build().fetch_all(pool).await? {
        specs.insert(
            parse_id(row.get("id"))?,
            CustomFieldSpec {
                kind: TaskFieldKind::parse(row.get::<String, _>("kind").as_str())?,
                option_ids: parse_options(&row.get::<String, _>("options_json"))?
                    .into_iter()
                    .map(|option| option.id)
                    .collect(),
            },
        );
    }
    Ok(specs)
}

fn field_from_row(row: sqlx::any::AnyRow, actor: &Actor) -> Result<TaskFieldRecord, TaskError> {
    let created_by: Option<String> = row.get("created_by");
    let created_by = created_by.map(parse_id).transpose()?;
    Ok(TaskFieldRecord {
        id: parse_id(row.get("id"))?,
        workspace_id: parse_id(row.get("workspace_id"))?,
        name: row.get("name"),
        kind: TaskFieldKind::parse(row.get::<String, _>("kind").as_str())?,
        options: parse_options(&row.get::<String, _>("options_json"))?,
        can_manage: can_manage(actor, created_by),
        created_by,
        project_count: row.get("project_count"),
        version: parse_version(row.get("version"))?,
        created_at: TimestampMillis::from_millis(row.get("created_at")),
        updated_at: TimestampMillis::from_millis(row.get("updated_at")),
    })
}

fn can_manage(actor: &Actor, created_by: Option<Id>) -> bool {
    actor.can(Permission::WorkspaceUpdate) || (!actor.is_guest() && created_by == Some(actor.user_id))
}

fn parse_options(json: &str) -> Result<Vec<TaskFieldOption>, TaskError> {
    serde_json::from_str(json).map_err(|_| TaskError::Conflict)
}

fn options_json(options: &[TaskFieldOption]) -> String {
    serde_json::to_string(options).unwrap_or_else(|_| "[]".to_owned())
}

/// Trimmed, unique (case-insensitive) option names of 1 to 100 characters and `#rrggbb` colors. An
/// input id of an existing option keeps it; any other input gets a new id.
pub fn checked_options(
    kind: TaskFieldKind,
    inputs: Vec<TaskFieldOptionInput>,
    existing: &[TaskFieldOption],
) -> Result<Vec<TaskFieldOption>, TaskError> {
    let invalid = TaskError::Invalid { field: "options" };
    if !kind.has_options() {
        return if inputs.is_empty() { Ok(Vec::new()) } else { Err(invalid) };
    }
    if inputs.len() > MAX_OPTIONS {
        return Err(invalid);
    }
    let mut names = HashSet::new();
    let mut ids = HashSet::new();
    let mut options = Vec::with_capacity(inputs.len());
    for input in inputs {
        let name = input.name.trim().to_owned();
        let color = input.color.to_ascii_lowercase();
        let valid_color = color.len() == 7
            && color.starts_with('#')
            && color[1..].bytes().all(|byte| byte.is_ascii_hexdigit());
        if name.is_empty() || name.chars().count() > 100 || !valid_color || !names.insert(name.to_lowercase()) {
            return Err(invalid);
        }
        let id = input
            .id
            .filter(|id| existing.iter().any(|option| &option.id == id))
            .unwrap_or_else(|| Id::new_v7().to_string());
        if !ids.insert(id.clone()) {
            return Err(invalid);
        }
        options.push(TaskFieldOption { id, name, color });
    }
    Ok(options)
}

/// The value to store for `value`, or `None` to clear: `null`, an empty text, `false` and an
/// empty option list all clear.
pub fn checked_value(
    kind: TaskFieldKind,
    options: &[TaskFieldOption],
    value: Value,
) -> Result<Option<Value>, TaskError> {
    let invalid = TaskError::Invalid { field: "value" };
    let known = |id: &str| options.iter().any(|option| option.id == id);
    match (kind, value) {
        (TaskFieldKind::TimeTracking, _) => Err(TaskError::Refused(TIME_TRACKING_VALUE)),
        (_, Value::Null) => Ok(None),
        (TaskFieldKind::Text, Value::String(text)) => {
            let text = text.trim();
            if text.chars().count() > MAX_TEXT_CHARS {
                Err(invalid)
            } else {
                Ok((!text.is_empty()).then(|| Value::String(text.to_owned())))
            }
        }
        (TaskFieldKind::Number, Value::Number(number)) => match number.as_f64() {
            Some(n) if n.is_finite() && n.abs() <= MAX_NUMBER => Ok(Some(Value::Number(number))),
            _ => Err(invalid),
        },
        (TaskFieldKind::Date, Value::String(date)) => {
            if date.len() == 10 && chrono::NaiveDate::parse_from_str(&date, "%Y-%m-%d").is_ok() {
                Ok(Some(Value::String(date)))
            } else {
                Err(invalid)
            }
        }
        (TaskFieldKind::Select, Value::String(id)) if known(&id) => Ok(Some(Value::String(id))),
        (TaskFieldKind::MultiSelect, Value::Array(items)) => {
            let mut ids: Vec<String> = Vec::with_capacity(items.len());
            for item in items {
                match item {
                    Value::String(id) if known(&id) => {
                        if !ids.contains(&id) {
                            ids.push(id);
                        }
                    }
                    _ => return Err(invalid),
                }
            }
            Ok((!ids.is_empty()).then(|| json!(ids)))
        }
        (TaskFieldKind::Checkbox, Value::Bool(checked)) => Ok(checked.then_some(Value::Bool(true))),
        (TaskFieldKind::Url, Value::String(url)) => {
            let url = url.trim();
            if url.is_empty() {
                return Ok(None);
            }
            let valid = url.len() <= MAX_URL_BYTES
                && !url.chars().any(char::is_whitespace)
                && reqwest::Url::parse(url).is_ok_and(|parsed| {
                    matches!(parsed.scheme(), "https" | "http") && parsed.host_str().is_some()
                });
            if valid { Ok(Some(Value::String(url.to_owned()))) } else { Err(invalid) }
        }
        _ => Err(invalid),
    }
}

/// A stored value as it reads now: option ids removed from the field are dropped.
pub fn live_value(kind: TaskFieldKind, options: &[TaskFieldOption], value: Value) -> Option<Value> {
    let known = |id: &str| options.iter().any(|option| option.id == id);
    match (kind, value) {
        (TaskFieldKind::Select, Value::String(id)) => known(&id).then_some(Value::String(id)),
        (TaskFieldKind::MultiSelect, Value::Array(items)) => {
            let items: Vec<Value> = items
                .into_iter()
                .filter(|item| item.as_str().is_some_and(known))
                .collect();
            (!items.is_empty()).then_some(Value::Array(items))
        }
        (_, value) => Some(value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn option(id: &str) -> TaskFieldOption {
        TaskFieldOption { id: id.into(), name: id.into(), color: "#ff0000".into() }
    }

    #[test]
    fn checks_values_by_kind() {
        let options = [option("a"), option("b")];
        let check = |kind, value| checked_value(kind, &options, value);
        assert_eq!(check(TaskFieldKind::Text, json!("  hi ")).unwrap(), Some(json!("hi")));
        assert_eq!(check(TaskFieldKind::Text, json!("   ")).unwrap(), None);
        assert_eq!(check(TaskFieldKind::Number, json!(12.5)).unwrap(), Some(json!(12.5)));
        assert!(check(TaskFieldKind::Number, json!(1e16)).is_err());
        assert!(check(TaskFieldKind::Number, json!("12")).is_err());
        assert_eq!(check(TaskFieldKind::Date, json!("2026-02-28")).unwrap(), Some(json!("2026-02-28")));
        assert!(check(TaskFieldKind::Date, json!("2026-02-30")).is_err());
        assert!(check(TaskFieldKind::Date, json!("2026-2-3")).is_err());
        assert_eq!(check(TaskFieldKind::Select, json!("a")).unwrap(), Some(json!("a")));
        assert!(check(TaskFieldKind::Select, json!("c")).is_err());
        assert_eq!(check(TaskFieldKind::MultiSelect, json!(["b", "a", "b"])).unwrap(), Some(json!(["b", "a"])));
        assert_eq!(check(TaskFieldKind::MultiSelect, json!([])).unwrap(), None);
        assert!(check(TaskFieldKind::MultiSelect, json!(["a", "c"])).is_err());
        assert_eq!(check(TaskFieldKind::Checkbox, json!(false)).unwrap(), None);
        assert_eq!(check(TaskFieldKind::Checkbox, json!(true)).unwrap(), Some(json!(true)));
        assert_eq!(check(TaskFieldKind::Url, json!("https://x.io/a")).unwrap(), Some(json!("https://x.io/a")));
        assert!(check(TaskFieldKind::Url, json!("javascript:alert(1)")).is_err());
        assert_eq!(check(TaskFieldKind::Url, Value::Null).unwrap(), None);
        assert!(matches!(check(TaskFieldKind::TimeTracking, json!(30)), Err(TaskError::Refused(_))));
        assert!(matches!(check(TaskFieldKind::TimeTracking, Value::Null), Err(TaskError::Refused(_))));
    }

    #[test]
    fn keeps_known_option_ids_and_rejects_duplicates() {
        let input = |id: Option<&str>, name: &str| TaskFieldOptionInput {
            id: id.map(str::to_owned),
            name: name.into(),
            color: "#00FF00".into(),
        };
        let existing = [option("keep")];
        let options = checked_options(
            TaskFieldKind::Select,
            vec![input(Some("keep"), "Kept"), input(Some("forged"), " New ")],
            &existing,
        )
        .unwrap();
        assert_eq!(options[0].id, "keep");
        assert_ne!(options[1].id, "forged");
        assert_eq!(options[1].name, "New");
        assert_eq!(options[1].color, "#00ff00");
        assert!(checked_options(TaskFieldKind::Select, vec![input(None, "A"), input(None, "a")], &[]).is_err());
        assert!(checked_options(TaskFieldKind::Text, vec![input(None, "A")], &[]).is_err());
    }

    #[test]
    fn drops_removed_options_on_read() {
        let options = [option("a")];
        assert_eq!(live_value(TaskFieldKind::Select, &options, json!("gone")), None);
        assert_eq!(live_value(TaskFieldKind::MultiSelect, &options, json!(["gone", "a"])), Some(json!(["a"])));
    }
}
