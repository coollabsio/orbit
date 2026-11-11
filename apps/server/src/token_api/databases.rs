//! Docs databases for API tokens and MCP (spec P3 14.3): read the schema, query rows, and add,
//! change and trash rows as the token's creator. The scopes are those of docs.
use std::collections::BTreeMap;

use orbit_platform::{Id, TimestampMillis};
use serde_json::{Map, Value};

use super::{Authorized, DocPatch, Result, TokenApiError, hidden_page, validate_doc_input};
use crate::collab::markdown_merge::{MAX_BLOCKS, block_count};
use crate::repositories::api_tokens::ApiTokenScope;
use crate::repositories::databases::{
    AgentDatabase, AgentPropertyChanges, AgentQuery, AgentRow, AgentRowPage, DatabaseError,
    DatabaseProperty, DatabaseRepository, NewRow, PropertyUpdate, RowPlacement,
};
use crate::repositories::pages::{PageChanges, PageError, PageRecord, revision};

impl From<DatabaseError> for TokenApiError {
    fn from(error: DatabaseError) -> Self {
        match error {
            DatabaseError::NotFound => Self::NotAvailable,
            DatabaseError::Unavailable(_) | DatabaseError::Corrupt => Self::Internal,
            DatabaseError::TrialLimit(reached) => Self::Page(PageError::TrialLimit(reached)),
            error => Self::Database(error),
        }
    }
}

#[derive(Debug, Default)]
pub struct NewDatabaseRow {
    pub title: String,
    /// By property name or id, as agents write them.
    pub values: Map<String, Value>,
    /// The row's body; with a template, it replaces the template's body.
    pub markdown: String,
    pub template_id: Option<Id>,
}

#[derive(Debug, Default)]
pub struct DatabaseRowPatch {
    pub title: Option<String>,
    pub values: Map<String, Value>,
}

impl Authorized<'_> {
    fn databases(&self, scope: ApiTokenScope) -> Result<DatabaseRepository> {
        self.content(scope, &self.api.pages)?;
        Ok(DatabaseRepository::new(self.api.tasks.database().clone()))
    }

    pub async fn get_database(&self, database_id: Id) -> Result<AgentDatabase> {
        let databases = self.databases(ApiTokenScope::DocsRead)?;
        let p = self.principal;
        Ok(databases
            .agent_database(
                p.workspace_id,
                database_id,
                p.actor_id,
                &self.api.web_origin,
            )
            .await?)
    }

    pub async fn query_database(&self, database_id: Id, query: AgentQuery) -> Result<AgentRowPage> {
        let databases = self.databases(ApiTokenScope::DocsRead)?;
        if query
            .cursor
            .as_ref()
            .is_some_and(|cursor| cursor.len() > 200)
        {
            return Err(DatabaseError::Invalid {
                field: "cursor".to_owned(),
            }
            .into());
        }
        let p = self.principal;
        Ok(databases
            .agent_query(
                p.workspace_id,
                database_id,
                p.actor_id,
                query,
                &self.api.web_origin,
            )
            .await?)
    }

    /// Adds a row, last in the manual order. The body is written after the row, so a body that
    /// fails leaves the row without it.
    pub async fn create_database_row(
        &self,
        database_id: Id,
        input: NewDatabaseRow,
    ) -> Result<AgentRow> {
        let databases = self.databases(ApiTokenScope::DocsWrite)?;
        validate_doc_input(Some(&input.title), None, Some(&input.markdown))?;
        let markdown = input.markdown.trim();
        if !markdown.is_empty() && block_count(&self.blocks(markdown).await?) > MAX_BLOCKS {
            return Err(PageError::TooLarge.into());
        }
        let p = self.principal;
        let values = databases
            .agent_values(p.workspace_id, database_id, p.actor_id, &input.values)
            .await?;
        let ids: Vec<Id> = values.iter().map(|(id, _)| *id).collect();
        let named = |error| DatabaseRepository::agent_value_error(error, &input.values, &ids);
        let row = NewRow {
            title: input.title,
            icon: None,
            values,
            placement: RowPlacement::Last,
        };
        let now = TimestampMillis::now();
        let row = match input.template_id {
            Some(template_id) => databases
                .create_row_from_template(
                    p.workspace_id,
                    database_id,
                    p.actor_id,
                    row,
                    template_id,
                    None,
                    self.request_id,
                    now,
                )
                .await
                .map_err(named)?,
            None => databases
                .create_row(
                    p.workspace_id,
                    database_id,
                    p.actor_id,
                    row,
                    self.request_id,
                    now,
                )
                .await
                .map_err(named)?,
        };
        if !markdown.is_empty() {
            let page = self.row_page(row.id).await?;
            let patch = DocPatch {
                markdown: Some(input.markdown),
                ..DocPatch::default()
            };
            self.update_doc(row.id, revision(page.version, page.updated_at), patch)
                .await?;
        }
        Ok(databases
            .agent_row(
                p.workspace_id,
                database_id,
                row.id,
                p.actor_id,
                &self.api.web_origin,
            )
            .await?)
    }

    /// Writes values (last write wins, as in the web app) and then the title.
    pub async fn update_database_row(
        &self,
        database_id: Id,
        row_id: Id,
        patch: DatabaseRowPatch,
    ) -> Result<AgentRow> {
        let databases = self.databases(ApiTokenScope::DocsWrite)?;
        if patch.values.is_empty() && patch.title.is_none() {
            return Err(TokenApiError::Invalid("Send values or a title"));
        }
        validate_doc_input(patch.title.as_deref(), None, None)?;
        let p = self.principal;
        let origin = &self.api.web_origin;
        databases
            .agent_row(p.workspace_id, database_id, row_id, p.actor_id, origin)
            .await?;
        if !patch.values.is_empty() {
            let values = databases
                .agent_values(p.workspace_id, database_id, p.actor_id, &patch.values)
                .await?;
            let ids: Vec<Id> = values.iter().map(|(id, _)| *id).collect();
            databases
                .update_values(
                    p.workspace_id,
                    database_id,
                    row_id,
                    p.actor_id,
                    values,
                    self.request_id,
                    TimestampMillis::now(),
                )
                .await
                .map_err(|error| {
                    DatabaseRepository::agent_value_error(error, &patch.values, &ids)
                })?;
        }
        if let Some(title) = patch.title {
            let pages = self.content(ApiTokenScope::DocsWrite, &self.api.pages)?;
            let page = self.row_page(row_id).await?;
            pages
                .update_page_at_revision(
                    p.workspace_id,
                    row_id,
                    p.actor_id,
                    revision(page.version, page.updated_at),
                    PageChanges {
                        title: Some(title),
                        ..PageChanges::default()
                    },
                    self.request_id,
                    TimestampMillis::now(),
                )
                .await?;
        }
        Ok(databases
            .agent_row(p.workspace_id, database_id, row_id, p.actor_id, origin)
            .await?)
    }

    /// Moves live rows of the database to the trash; returns how many.
    pub async fn trash_database_rows(&self, database_id: Id, row_ids: &[Id]) -> Result<u64> {
        let databases = self.databases(ApiTokenScope::DocsWrite)?;
        let p = self.principal;
        Ok(databases
            .agent_trash_rows(
                p.workspace_id,
                database_id,
                p.actor_id,
                row_ids,
                self.request_id,
                TimestampMillis::now(),
            )
            .await?)
    }

    /// Adds a property (any type but `title`); options may be plain names.
    pub async fn create_database_property(
        &self,
        database_id: Id,
        name: &str,
        property_type: &str,
        config: Option<Value>,
        position: Option<i64>,
    ) -> Result<DatabaseProperty> {
        let databases = self.databases(ApiTokenScope::DocsWrite)?;
        let p = self.principal;
        Ok(databases
            .agent_create_property(
                p.workspace_id,
                database_id,
                p.actor_id,
                name,
                property_type,
                config,
                position,
                self.request_id,
                TimestampMillis::now(),
            )
            .await?)
    }

    /// Renames, retypes, reconfigures or moves the property `key` names (its name or id).
    pub async fn update_database_property(
        &self,
        database_id: Id,
        key: &str,
        changes: AgentPropertyChanges,
    ) -> Result<PropertyUpdate> {
        let databases = self.databases(ApiTokenScope::DocsWrite)?;
        let p = self.principal;
        Ok(databases
            .agent_update_property(
                p.workspace_id,
                database_id,
                p.actor_id,
                key,
                changes,
                self.request_id,
                TimestampMillis::now(),
            )
            .await?)
    }

    /// Deletes the property `key` names with its values; returns its id.
    pub async fn delete_database_property(&self, database_id: Id, key: &str) -> Result<Id> {
        let databases = self.databases(ApiTokenScope::DocsWrite)?;
        let p = self.principal;
        Ok(databases
            .agent_delete_property(
                p.workspace_id,
                database_id,
                p.actor_id,
                key,
                self.request_id,
                TimestampMillis::now(),
            )
            .await?)
    }

    /// The values of `page` when it is a row of a database, as `query_database` shows them.
    pub(super) async fn row_values(
        &self,
        page: &PageRecord,
    ) -> Result<Option<BTreeMap<String, Value>>> {
        let Some(database_id) = page.database_id else {
            return Ok(None);
        };
        let p = self.principal;
        let databases = DatabaseRepository::new(self.api.tasks.database().clone());
        match databases
            .agent_row(
                p.workspace_id,
                database_id,
                page.id,
                p.actor_id,
                &self.api.web_origin,
            )
            .await
        {
            Ok(row) => Ok(Some(row.values)),
            Err(DatabaseError::Unavailable(_) | DatabaseError::Corrupt) => {
                Err(TokenApiError::Internal)
            }
            // A template of the database, or a database the reader cannot see.
            Err(_) => Ok(None),
        }
    }

    async fn row_page(&self, row_id: Id) -> Result<PageRecord> {
        let pages = self.content(ApiTokenScope::DocsWrite, &self.api.pages)?;
        let p = self.principal;
        pages
            .get_page(p.workspace_id, row_id, p.actor_id)
            .await
            .map_err(hidden_page)
    }
}
