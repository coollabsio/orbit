//! Custom task fields (#87): the workspace's field gallery, the fields each project shows, and
//! the tasks' values.
//!
//! The gallery is workspace data, as in Settings → Task fields: every token with `tasks:read`
//! lists all of it, also one limited to some projects. `project_count` then counts only the
//! approved projects, so a limited token does not learn how other projects use a field. A
//! project's fields and a task's values follow the grant like any other task data.
//!
//! Adding a field changes no project, so any `tasks:write` token may. Changing or deleting one
//! changes (or deletes) the values in every project that shows it, so it needs a token for all
//! projects, besides the creator rules of the repository.
use std::collections::{HashMap, HashSet};

use orbit_platform::{Id, TimestampMillis};
use serde_json::Value;

use super::{Authorized, Result, TokenApiError, valid_name};
use crate::repositories::api_tokens::ApiTokenScope;
use crate::repositories::tasks::{
    TaskError, TaskFieldKind, TaskFieldOptionInput, TaskFieldRecord, TaskFieldValue,
};

const FIELD_NAME: &str = "A field name has 1 to 100 characters";
pub const VALUE_DETAIL: &str = "value does not fit the field's kind: a string for text and url \
    (http or https, up to 2,048 bytes), a number for number (up to 1e15), YYYY-MM-DD for date, \
    an option id of the field for select, a list of option ids for multi_select, true for \
    checkbox; null clears any value";
const OPTIONS_DETAIL: &str = "options are only for select and multi_select fields: up to 100, \
    each with a unique name of 1 to 100 characters and a #rrggbb color";
const FIELD_IDS_DETAIL: &str =
    "field_ids must be distinct field ids of the workspace's gallery, at most 200";
const NOT_SHOWN_DETAIL: &str =
    "field_id must be a field that the task's project shows (see its project's fields)";
const MANAGE_DETAIL: &str =
    "Only the field's creator or a workspace owner or admin may change or delete it";
const GUEST_DETAIL: &str = "Guests cannot add task fields";
const PROJECT_DETAIL: &str = "Setting a project's fields needs a member who may manage the project";
const ALL_PROJECTS_DETAIL: &str = "Changing or deleting a gallery field changes the values of \
    every project that shows it, so it needs a token for all projects";

impl Authorized<'_> {
    /// The gallery, by name. `project_count` counts approved projects only.
    pub async fn task_fields(&self) -> Result<Vec<TaskFieldRecord>> {
        self.need(ApiTokenScope::TasksRead)?;
        let p = self.principal;
        let fields = self
            .api
            .tasks
            .task_fields(p.workspace_id, p.actor_id)
            .await?;
        self.with_approved_counts(fields).await
    }

    pub async fn create_task_field(
        &self,
        name: &str,
        kind: TaskFieldKind,
        options: Vec<TaskFieldOptionInput>,
    ) -> Result<TaskFieldRecord> {
        self.need(ApiTokenScope::TasksWrite)?;
        let name = valid_name(name, 100, FIELD_NAME)?;
        let p = self.principal;
        let field = self
            .granted()
            .create_task_field(
                p.workspace_id,
                p.actor_id,
                name,
                kind,
                options,
                self.request_id,
                TimestampMillis::now(),
            )
            .await
            .map_err(|error| match error {
                TaskError::Forbidden => TokenApiError::Forbidden(GUEST_DETAIL),
                error => field_error(error),
            })?;
        Ok(self.with_approved_counts(vec![field]).await?.remove(0))
    }

    /// Renames the field or replaces its options (send an option's `id` to keep it).
    pub async fn update_task_field(
        &self,
        field_id: Id,
        expected_version: u64,
        name: Option<String>,
        options: Option<Vec<TaskFieldOptionInput>>,
    ) -> Result<TaskFieldRecord> {
        self.need(ApiTokenScope::TasksWrite)?;
        self.all_projects_for_gallery()?;
        let name = name
            .map(|name| valid_name(&name, 100, FIELD_NAME))
            .transpose()?;
        let p = self.principal;
        let field = self
            .granted()
            .update_task_field(
                p.workspace_id,
                field_id,
                p.actor_id,
                name,
                options,
                expected_version,
                self.request_id,
                TimestampMillis::now(),
            )
            .await
            .map_err(field_error)?;
        Ok(self.with_approved_counts(vec![field]).await?.remove(0))
    }

    /// Deletes the field and every task's value of it.
    pub async fn delete_task_field(&self, field_id: Id, expected_version: u64) -> Result<()> {
        self.need(ApiTokenScope::TasksWrite)?;
        self.all_projects_for_gallery()?;
        let p = self.principal;
        self.granted()
            .delete_task_field(
                p.workspace_id,
                field_id,
                p.actor_id,
                expected_version,
                self.request_id,
                TimestampMillis::now(),
            )
            .await
            .map_err(field_error)
    }

    /// The fields an approved project shows, in its order.
    pub async fn project_task_fields(&self, project_id: Id) -> Result<Vec<TaskFieldRecord>> {
        self.need(ApiTokenScope::TasksRead)?;
        if !self.approved(project_id) {
            return Err(TokenApiError::NotAvailable);
        }
        let p = self.principal;
        let fields = self
            .api
            .tasks
            .project_task_fields(p.workspace_id, project_id, p.actor_id)
            .await?;
        self.with_approved_counts(fields).await
    }

    /// Sets the fields an approved project shows and their order. Values of a removed field stay.
    pub async fn set_project_task_fields(
        &self,
        project_id: Id,
        field_ids: &[Id],
    ) -> Result<Vec<TaskFieldRecord>> {
        self.need(ApiTokenScope::TasksWrite)?;
        if !self.approved(project_id) {
            return Err(TokenApiError::NotAvailable);
        }
        let p = self.principal;
        let fields = self
            .granted()
            .set_project_task_fields(
                p.workspace_id,
                project_id,
                p.actor_id,
                field_ids,
                self.request_id,
                TimestampMillis::now(),
            )
            .await
            .map_err(|error| match error {
                TaskError::Forbidden => TokenApiError::Forbidden(PROJECT_DETAIL),
                error => field_error(error),
            })?;
        self.with_approved_counts(fields).await
    }

    /// The task's values of every gallery field, also of fields its project does not show.
    pub async fn task_field_values(&self, task_ref: &str) -> Result<Vec<TaskFieldValue>> {
        self.need(ApiTokenScope::TasksRead)?;
        let task = self.task(task_ref).await?;
        let p = self.principal;
        self.api
            .tasks
            .task_field_values(p.workspace_id, task.id, p.actor_id)
            .await
            .map_err(TokenApiError::from)
    }

    /// Sets (with `null`, clears) the task's value of a field that its project shows; returns
    /// all the task's values.
    pub async fn set_task_field_value(
        &self,
        task_ref: &str,
        field_id: Id,
        value: Value,
    ) -> Result<Vec<TaskFieldValue>> {
        self.need(ApiTokenScope::TasksWrite)?;
        // The task (and so its project) must be approved; the repository checks the rest.
        let task = self.task(task_ref).await?;
        let p = self.principal;
        self.granted()
            .set_task_field_value(
                p.workspace_id,
                task.id,
                field_id,
                p.actor_id,
                value,
                self.request_id,
                TimestampMillis::now(),
            )
            .await
            .map_err(|error| match error {
                // The task was found above: the field is missing or its project does not show it.
                TaskError::NotFound => TokenApiError::Invalid(NOT_SHOWN_DETAIL),
                error => field_error(error),
            })
    }

    /// The gallery, to name the fields of values the caller read or wrote (a write is authorized
    /// for `tasks:write` only); empty for a credential without `tasks:read`.
    pub async fn value_fields(&self) -> Result<Vec<TaskFieldRecord>> {
        if !self.principal.allows(ApiTokenScope::TasksRead) {
            return Ok(Vec::new());
        }
        let p = self.principal;
        Ok(self
            .api
            .tasks
            .task_fields(p.workspace_id, p.actor_id)
            .await?)
    }

    fn all_projects_for_gallery(&self) -> Result<()> {
        if self.principal.all_projects {
            Ok(())
        } else {
            Err(TokenApiError::Forbidden(ALL_PROJECTS_DETAIL))
        }
    }

    /// `fields` with `project_count` limited to the approved projects that show each field.
    async fn with_approved_counts(
        &self,
        mut fields: Vec<TaskFieldRecord>,
    ) -> Result<Vec<TaskFieldRecord>> {
        let approved: HashSet<String> = self
            .principal
            .project_ids
            .iter()
            .map(ToString::to_string)
            .collect();
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT project_task_fields.field_id, project_task_fields.project_id \
             FROM project_task_fields JOIN task_fields ON task_fields.id = project_task_fields.field_id \
             WHERE task_fields.workspace_id = $1",
        )
        .bind(self.principal.workspace_id.to_string())
        .fetch_all(self.api.tasks.database().pool())
        .await?;
        let mut counts: HashMap<String, i64> = HashMap::new();
        for (field_id, project_id) in rows {
            if approved.contains(&project_id) {
                *counts.entry(field_id).or_default() += 1;
            }
        }
        for field in &mut fields {
            field.project_count = counts.get(&field.id.to_string()).copied().unwrap_or(0);
        }
        Ok(fields)
    }
}

fn field_error(error: TaskError) -> TokenApiError {
    match error {
        TaskError::Forbidden => TokenApiError::Forbidden(MANAGE_DETAIL),
        TaskError::Conflict => {
            TokenApiError::Conflict("The workspace already has a field with this name")
        }
        TaskError::Invalid { field: "value" } => TokenApiError::Invalid(VALUE_DETAIL),
        TaskError::Invalid { field: "options" } => TokenApiError::Invalid(OPTIONS_DETAIL),
        TaskError::Invalid { field: "field_ids" } => TokenApiError::Invalid(FIELD_IDS_DETAIL),
        TaskError::Invalid { field: "fields" } => {
            TokenApiError::Conflict("The workspace has the most fields allowed (200)")
        }
        error => error.into(),
    }
}
