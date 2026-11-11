//! Project setup writes for the token's approved projects: a project's name and color, its
//! statuses and its labels.
use orbit_platform::{Id, TimestampMillis};

use super::{Authorized, Result, TokenApiError, valid_color, valid_name};
use crate::repositories::api_tokens::ApiTokenScope;
use crate::repositories::tasks::{
    LabelRecord, ProjectAutomationPatch, ProjectRecord, StatusRecord, TaskError,
};
use crate::task_import::category_color;

/// The web app's neutral label color.
const DEFAULT_LABEL_COLOR: &str = "#95a2b3";

#[derive(Debug, Default)]
pub struct StatusPatch {
    pub name: Option<String>,
    pub description: Option<String>,
    pub color: Option<String>,
    pub category: Option<String>,
    pub position: Option<i64>,
}

impl Authorized<'_> {
    /// Renames or recolors an approved project; `None` keeps the value.
    pub async fn update_project(
        &self,
        project_id: Id,
        expected_version: u64,
        name: Option<String>,
        color: Option<String>,
    ) -> Result<ProjectRecord> {
        self.need(ApiTokenScope::TasksWrite)?;
        let p = self.principal;
        let current = self
            .api
            .tasks
            .approved_projects(p.workspace_id, p.actor_id, &p.project_ids)
            .await?
            .into_iter()
            .find(|project| project.id == project_id)
            .ok_or(TokenApiError::NotAvailable)?;
        let name = match name {
            Some(name) => valid_name(&name, 200, "Invalid project name")?,
            None => current.name,
        };
        let color = valid_color(color, &current.color)?;
        Ok(self
            .granted()
            .update_project(
                p.workspace_id,
                project_id,
                p.actor_id,
                name,
                current.key,
                color,
                ProjectAutomationPatch::default(),
                None,
                expected_version,
                self.request_id,
                TimestampMillis::now(),
            )
            .await?)
    }

    /// Adds a status to an approved project; its color follows the category when unset.
    pub async fn create_status(&self, project_id: Id, status: StatusPatch) -> Result<StatusRecord> {
        self.need(ApiTokenScope::TasksWrite)?;
        if !self.approved(project_id) {
            return Err(TokenApiError::NotAvailable);
        }
        let name = valid_name(
            status.name.as_deref().unwrap_or_default(),
            200,
            "Invalid status name",
        )?;
        let category = valid_category(status.category.as_deref().unwrap_or_default())?;
        let color = valid_color(status.color, category_color(&category))?;
        let description = valid_description(status.description.unwrap_or_default())?;
        let p = self.principal;
        Ok(self
            .granted()
            .create_status(
                p.workspace_id,
                project_id,
                p.actor_id,
                name,
                description,
                color,
                category,
                status.position,
                self.request_id,
                TimestampMillis::now(),
            )
            .await?)
    }

    /// Changes a status of an approved project; `None` keeps the value. A status cannot become
    /// or stop being the Duplicate status.
    pub async fn update_status(
        &self,
        project_id: Id,
        status_id: Id,
        expected_version: u64,
        patch: StatusPatch,
    ) -> Result<StatusRecord> {
        self.need(ApiTokenScope::TasksWrite)?;
        if !self.approved(project_id) {
            return Err(TokenApiError::NotAvailable);
        }
        let p = self.principal;
        let current = self
            .api
            .tasks
            .status(p.workspace_id, project_id, status_id)
            .await?;
        let name = match patch.name {
            Some(name) => valid_name(&name, 200, "Invalid status name")?,
            None => current.name,
        };
        let category = match patch.category {
            Some(category) => valid_category(&category)?,
            None => current.category,
        };
        let description = patch.description.map(valid_description).transpose()?;
        Ok(self
            .granted()
            .update_status(
                p.workspace_id,
                project_id,
                status_id,
                p.actor_id,
                name,
                description,
                valid_color(patch.color, &current.color)?,
                category,
                patch.position.unwrap_or(current.position),
                expected_version,
                self.request_id,
                TimestampMillis::now(),
            )
            .await?)
    }

    pub async fn create_label(
        &self,
        project_id: Id,
        name: &str,
        color: Option<String>,
    ) -> Result<LabelRecord> {
        self.need(ApiTokenScope::TasksWrite)?;
        if !self.approved(project_id) {
            return Err(TokenApiError::NotAvailable);
        }
        let name = valid_name(name, 100, "Invalid label name")?;
        let color = valid_color(color, DEFAULT_LABEL_COLOR)?;
        let p = self.principal;
        self.granted()
            .create_label(
                p.workspace_id,
                project_id,
                p.actor_id,
                name,
                color,
                self.request_id,
                TimestampMillis::now(),
            )
            .await
            .map_err(label_conflict)
    }

    /// Renames or recolors a label of an approved project; `None` keeps the value.
    pub async fn update_label(
        &self,
        project_id: Id,
        label_id: Id,
        expected_version: u64,
        name: Option<String>,
        color: Option<String>,
    ) -> Result<LabelRecord> {
        self.need(ApiTokenScope::TasksWrite)?;
        if !self.approved(project_id) {
            return Err(TokenApiError::NotAvailable);
        }
        let p = self.principal;
        let current = self
            .api
            .tasks
            .label(p.workspace_id, project_id, label_id)
            .await?;
        let name = match name {
            Some(name) => valid_name(&name, 100, "Invalid label name")?,
            None => current.name,
        };
        self.granted()
            .update_label(
                p.workspace_id,
                project_id,
                label_id,
                p.actor_id,
                name,
                valid_color(color, &current.color)?,
                expected_version,
                self.request_id,
                TimestampMillis::now(),
            )
            .await
            .map_err(label_conflict)
    }
}

/// The categories a client can give a status; Orbit makes the Duplicate status itself.
fn valid_category(category: &str) -> Result<String> {
    match category {
        "unstarted" | "started" | "completed" | "cancelled" => Ok(category.to_owned()),
        _ => Err(TokenApiError::Invalid(
            "A status category is unstarted, started, completed or cancelled",
        )),
    }
}

fn valid_description(description: String) -> Result<String> {
    if description.chars().count() > 20_000 {
        return Err(TokenApiError::Invalid(
            "A status description has at most 20,000 characters",
        ));
    }
    Ok(description)
}

fn label_conflict(error: TaskError) -> TokenApiError {
    match error {
        TaskError::Conflict => {
            TokenApiError::Invalid("The project already has a label with this name")
        }
        error => error.into(),
    }
}
