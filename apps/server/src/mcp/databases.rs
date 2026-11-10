//! Database tools (spec P3 14.3) over [`crate::token_api`], with the scopes of docs.
use rmcp::ErrorData;
use rmcp::model::{CallToolRequestParams, Tool};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::{arguments, definition, parse_id, write_definition};
use crate::repositories::api_tokens::ApiTokenScope;
use crate::repositories::databases::AgentQuery;
use crate::token_api::{Authorized, DatabaseRowPatch, NewDatabaseRow, TokenApiError};

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct DatabaseArgs {
    /// The database's page id (`list_docs` lists databases with `kind: "database"`).
    database_id: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct QueryArgs {
    database_id: String,
    /// An AND/OR group, at most 3 levels, 10 children per group and 20 rules:
    /// `{"op": "and", "children": [{"property_id": "Status", "operator": "is", "value":
    /// ["Done"]}, {"op": "or", "children": [...]}]}`. `property_id` takes a property name or
    /// id; select, status and multi-select values take option names or ids, person values
    /// user ids, emails or "me". Dates take `YYYY-MM-DD`, a UTC time, or `today`, `tomorrow`,
    /// `yesterday`, `one_week_ago`, `one_week_from_now`, `one_month_ago`, `one_month_from_now`.
    filter: Option<Value>,
    /// Up to 10 sorts, first one first: `[{"property_id": "Due", "direction": "desc"}]`.
    sorts: Option<Value>,
    /// Start from this view's filter and sorts. `filter` must match too; `sorts` replace the
    /// view's.
    view_id: Option<String>,
    /// Rows per page, from 1 to 100. Default: 50.
    limit: Option<usize>,
    /// The next_cursor of the previous page, with the same query.
    cursor: Option<String>,
    /// IANA time zone for relative dates and computed values, such as `Europe/Budapest`.
    /// Default: UTC.
    tz: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct CreateRowArgs {
    database_id: String,
    #[serde(default)]
    title: String,
    /// Up to 50 values by property name or id (see `update_database_row`).
    #[serde(default)]
    values: Map<String, Value>,
    /// The row's body as Markdown. With a template it replaces the template's body.
    #[serde(default)]
    markdown: String,
    /// A template of the database (`get_database` lists them) that fills the title, values
    /// and body; `title` and `values` sent here win.
    template_id: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct UpdateRowArgs {
    database_id: String,
    row_id: String,
    /// Up to 50 values by property name or id; `null` clears one. Text, URL and number take
    /// their value (a percent stores 0.5 for 50%), a checkbox true or false, a select or status
    /// an option name or id, a multi-select a list of them, a person a list of user ids or
    /// emails, a date `YYYY-MM-DD`, a UTC time, `start/end`, or `{"start", "end"}`. Options
    /// must exist: values never add options.
    #[serde(default)]
    values: Map<String, Value>,
    title: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct TrashRowsArgs {
    database_id: String,
    /// 1 to 100 row ids. Ids that are not live rows of the database are skipped.
    row_ids: Vec<String>,
}

pub(super) fn scope(name: &str) -> Option<ApiTokenScope> {
    match name {
        "get_database" | "query_database" => Some(ApiTokenScope::DocsRead),
        "create_database_row" | "update_database_row" | "trash_database_rows" => {
            Some(ApiTokenScope::DocsWrite)
        }
        _ => None,
    }
}

pub(super) fn tools() -> Vec<Tool> {
    vec![
        definition::<DatabaseArgs>(
            "get_database",
            "Read a visible database's schema: its properties (with option names), views, templates and row count.",
        ),
        definition::<QueryArgs>(
            "query_database",
            "Query a visible database's rows with a filter and sorts, 50 per page by default. Rows show values by property name, options by name and people as {id, name}.",
        ),
        write_definition::<CreateRowArgs>(
            "create_database_row",
            "Add a row to a visible database, with values by property name or id and an optional Markdown body or template.",
            false,
        ),
        write_definition::<UpdateRowArgs>(
            "update_database_row",
            "Change values or the title of a database row. Last write wins; no version is needed.",
            true,
        ),
        write_definition::<TrashRowsArgs>(
            "trash_database_rows",
            "Move database rows to the trash for 30 days.",
            true,
        ),
    ]
}

/// Runs a database tool; `None` for another tool's name.
pub(super) async fn call(
    api: &Authorized<'_>,
    request: &CallToolRequestParams,
) -> Option<Result<Result<Value, TokenApiError>, ErrorData>> {
    let result = match request.name.as_ref() {
        "get_database" => {
            let run = async {
                let a: DatabaseArgs = arguments(request)?;
                Ok(api
                    .get_database(parse_id(&a.database_id)?)
                    .await
                    .map(|d| json!(d)))
            };
            run.await
        }
        "query_database" => {
            let run = async {
                let a: QueryArgs = arguments(request)?;
                let query = AgentQuery {
                    filter: a.filter,
                    sorts: a.sorts,
                    view_id: a.view_id.as_deref().map(parse_id).transpose()?,
                    limit: a.limit,
                    cursor: a.cursor,
                    tz: a.tz,
                };
                Ok(api
                    .query_database(parse_id(&a.database_id)?, query)
                    .await
                    .map(|page| json!(page)))
            };
            run.await
        }
        "create_database_row" => {
            let run = async {
                let a: CreateRowArgs = arguments(request)?;
                let input = NewDatabaseRow {
                    title: a.title,
                    values: a.values,
                    markdown: a.markdown,
                    template_id: a.template_id.as_deref().map(parse_id).transpose()?,
                };
                Ok(api
                    .create_database_row(parse_id(&a.database_id)?, input)
                    .await
                    .map(|row| json!(row)))
            };
            run.await
        }
        "update_database_row" => {
            let run = async {
                let a: UpdateRowArgs = arguments(request)?;
                let patch = DatabaseRowPatch {
                    title: a.title,
                    values: a.values,
                };
                Ok(api
                    .update_database_row(parse_id(&a.database_id)?, parse_id(&a.row_id)?, patch)
                    .await
                    .map(|row| json!(row)))
            };
            run.await
        }
        "trash_database_rows" => {
            let run = async {
                let a: TrashRowsArgs = arguments(request)?;
                let row_ids = a
                    .row_ids
                    .iter()
                    .map(|id| parse_id(id))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(api
                    .trash_database_rows(parse_id(&a.database_id)?, &row_ids)
                    .await
                    .map(|trashed| json!({ "trashed": trashed })))
            };
            run.await
        }
        _ => return None,
    };
    Some(result)
}
