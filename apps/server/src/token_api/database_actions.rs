//! Database row moves, row templates and formula previews for API tokens and MCP. They act as
//! the token's creator with the docs scopes, as the other database writes do.
use orbit_platform::{Id, TimestampMillis};
use serde_json::{Map, Value};

use super::{Authorized, Result, TokenApiError, valid_name};
use crate::repositories::api_tokens::ApiTokenScope;
use crate::repositories::databases::{
    AgentRow, AgentTemplate, ComputeScope, DatabaseRepository, FormulaPreview, NewTemplate,
    TemplateChanges,
};

/// Characters of a formula expression that a preview reads.
const MAX_EXPRESSION_CHARS: usize = 10_000;

impl Authorized<'_> {
    /// Moves a live row right after `after_row_id` (first when `None`), and writes `values`
    /// (by property name or id) with the move, as a board drop sets the group value.
    pub async fn move_database_row(
        &self,
        database_id: Id,
        row_id: Id,
        after_row_id: Option<Id>,
        values: Map<String, Value>,
    ) -> Result<AgentRow> {
        let databases = self.databases(ApiTokenScope::DocsWrite)?;
        if after_row_id == Some(row_id) {
            return Err(TokenApiError::Invalid(
                "after_row_id must be another row of the database",
            ));
        }
        let p = self.principal;
        let input = if values.is_empty() {
            Vec::new()
        } else {
            databases
                .agent_values(p.workspace_id, database_id, p.actor_id, &values)
                .await?
        };
        let ids: Vec<Id> = input.iter().map(|(id, _)| *id).collect();
        databases
            .move_row(
                p.workspace_id,
                database_id,
                row_id,
                p.actor_id,
                after_row_id,
                input,
                self.request_id,
                TimestampMillis::now(),
            )
            .await
            .map_err(|error| DatabaseRepository::agent_value_error(error, &values, &ids))?;
        let origin = &self.api.web_origin;
        Ok(databases
            .agent_row(p.workspace_id, database_id, row_id, p.actor_id, origin)
            .await?)
    }

    /// Adds a row template named `name`, empty or a copy of the row or template `from`.
    pub async fn create_database_template(
        &self,
        database_id: Id,
        name: &str,
        from: Option<Id>,
    ) -> Result<AgentTemplate> {
        let databases = self.databases(ApiTokenScope::DocsWrite)?;
        let name = valid_name(name, 200, "A template name has 1 to 200 characters")?;
        let p = self.principal;
        let template = databases
            .create_template(
                p.workspace_id,
                database_id,
                p.actor_id,
                NewTemplate { name, from },
                self.request_id,
                TimestampMillis::now(),
            )
            .await?;
        Ok(AgentTemplate {
            id: template.row.id,
            name: template.name,
        })
    }

    /// Renames a template or moves it in the "New" list (0 is first).
    pub async fn update_database_template(
        &self,
        database_id: Id,
        template_id: Id,
        name: Option<String>,
        position: Option<i64>,
    ) -> Result<AgentTemplate> {
        let databases = self.databases(ApiTokenScope::DocsWrite)?;
        if name.is_none() && position.is_none() {
            return Err(TokenApiError::Invalid("Send name or position"));
        }
        if position.is_some_and(|position| position < 0) {
            return Err(TokenApiError::Invalid("position must be 0 or more"));
        }
        let changes = TemplateChanges {
            name: name
                .map(|name| valid_name(&name, 200, "A template name has 1 to 200 characters"))
                .transpose()?,
            position,
        };
        let p = self.principal;
        let template = databases
            .update_template(
                p.workspace_id,
                database_id,
                template_id,
                p.actor_id,
                changes,
                self.request_id,
                TimestampMillis::now(),
            )
            .await?;
        Ok(AgentTemplate {
            id: template.row.id,
            name: template.name,
        })
    }

    /// Deletes a template and its page; rows made from it stay.
    pub async fn delete_database_template(&self, database_id: Id, template_id: Id) -> Result<()> {
        let databases = self.databases(ApiTokenScope::DocsWrite)?;
        let p = self.principal;
        Ok(databases
            .delete_template(
                p.workspace_id,
                database_id,
                template_id,
                p.actor_id,
                self.request_id,
                TimestampMillis::now(),
            )
            .await?)
    }

    /// The result type (or error) of `expression` as a formula, and its values on the first
    /// rows. `property_id` is the formula property being edited, so it cannot read itself.
    /// Writes nothing.
    pub async fn preview_database_formula(
        &self,
        database_id: Id,
        expression: &str,
        property_id: Option<Id>,
        tz: Option<&str>,
    ) -> Result<FormulaPreview> {
        let databases = self.databases(ApiTokenScope::DocsRead)?;
        if expression.trim().is_empty() || expression.chars().count() > MAX_EXPRESSION_CHARS {
            return Err(TokenApiError::Invalid(
                "expression has 1 to 10,000 characters",
            ));
        }
        let p = self.principal;
        let scope = ComputeScope::with_tz(p.workspace_id, p.actor_id, tz).ok_or(
            TokenApiError::Invalid("tz must be an IANA time zone name such as Europe/Budapest"),
        )?;
        Ok(databases
            .preview_formula(&scope, database_id, property_id, expression)
            .await?)
    }
}
