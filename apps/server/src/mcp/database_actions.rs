//! Database row moves, row templates and formula previews over [`crate::token_api`], with the
//! scopes of docs.
use rmcp::ErrorData;
use rmcp::model::{CallToolRequestParams, Tool};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::{arguments, definition, parse_id, write_definition};
use crate::repositories::api_tokens::ApiTokenScope;
use crate::token_api::{Authorized, TokenApiError};

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct MoveRowArgs {
    /// The database's page id.
    database_id: String,
    row_id: String,
    /// The row to put this one right after; `null` moves it first. The order is the one that
    /// views without a sort show (query_database without sorts).
    after_row_id: Option<String>,
    /// Values written with the move, by property name or id, as in update_database_row.
    #[serde(default)]
    values: Map<String, Value>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct CreateTemplateArgs {
    database_id: String,
    /// What the "New" menu lists: 1 to 200 characters.
    name: String,
    /// A row or template of the database whose title, icon, values and body the template
    /// copies; omitted makes an empty template.
    from_row_id: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct UpdateTemplateArgs {
    database_id: String,
    /// A template id from get_database's `templates`.
    template_id: String,
    /// 1 to 200 characters.
    name: Option<String>,
    /// The template's place in the "New" menu (0 is first).
    position: Option<i64>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct DeleteTemplateArgs {
    database_id: String,
    /// A template id from get_database's `templates`.
    template_id: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct PreviewFormulaArgs {
    database_id: String,
    /// The formula expression; `prop("…")` takes a property name or id.
    expression: String,
    /// The formula property being edited, so it cannot read itself; omitted for a new one.
    property_id: Option<String>,
    /// IANA zone for date functions such as `Europe/Budapest`; default UTC.
    tz: Option<String>,
}

pub(super) fn scope(name: &str) -> Option<ApiTokenScope> {
    match name {
        "preview_database_formula" => Some(ApiTokenScope::DocsRead),
        "move_database_row"
        | "create_database_template"
        | "update_database_template"
        | "delete_database_template" => Some(ApiTokenScope::DocsWrite),
        _ => None,
    }
}

pub(super) fn tools() -> Vec<Tool> {
    vec![
        write_definition::<MoveRowArgs>(
            "move_database_row",
            "Move a row of a visible database right after another row (or first), optionally writing values with the move. Returns the row as query_database does.",
            false,
        ),
        write_definition::<CreateTemplateArgs>(
            "create_database_template",
            "Add a row template to a visible database, empty or copied from a row. The template is a page: change its title and body with update_doc and its values with update_database_row. create_database_row with template_id fills a row from it.",
            false,
        ),
        write_definition::<UpdateTemplateArgs>(
            "update_database_template",
            "Rename a row template of a database or move it in the New menu.",
            false,
        ),
        write_definition::<DeleteTemplateArgs>(
            "delete_database_template",
            "Delete a row template of a database. Rows made from it stay.",
            true,
        ),
        definition::<PreviewFormulaArgs>(
            "preview_database_formula",
            "Try a formula expression on a visible database without saving it: returns its result type (or the error and where it is) and its values on the first rows. Set it with create_database_property or update_database_property.",
        ),
    ]
}

/// Runs a tool of this module; `None` for another tool's name.
pub(super) async fn call(
    api: &Authorized<'_>,
    request: &CallToolRequestParams,
) -> Option<Result<Result<Value, TokenApiError>, ErrorData>> {
    let result = match request.name.as_ref() {
        "move_database_row" => {
            let run = async {
                let a: MoveRowArgs = arguments(request)?;
                let after = a.after_row_id.as_deref().map(parse_id).transpose()?;
                Ok(api
                    .move_database_row(
                        parse_id(&a.database_id)?,
                        parse_id(&a.row_id)?,
                        after,
                        a.values,
                    )
                    .await
                    .map(|row| json!(row)))
            };
            run.await
        }
        "create_database_template" => {
            let run = async {
                let a: CreateTemplateArgs = arguments(request)?;
                let from = a.from_row_id.as_deref().map(parse_id).transpose()?;
                Ok(api
                    .create_database_template(parse_id(&a.database_id)?, &a.name, from)
                    .await
                    .map(|template| json!(template)))
            };
            run.await
        }
        "update_database_template" => {
            let run = async {
                let a: UpdateTemplateArgs = arguments(request)?;
                Ok(api
                    .update_database_template(
                        parse_id(&a.database_id)?,
                        parse_id(&a.template_id)?,
                        a.name,
                        a.position,
                    )
                    .await
                    .map(|template| json!(template)))
            };
            run.await
        }
        "delete_database_template" => {
            let run = async {
                let a: DeleteTemplateArgs = arguments(request)?;
                Ok(api
                    .delete_database_template(parse_id(&a.database_id)?, parse_id(&a.template_id)?)
                    .await
                    .map(|()| json!({ "deleted": true })))
            };
            run.await
        }
        "preview_database_formula" => {
            let run = async {
                let a: PreviewFormulaArgs = arguments(request)?;
                let property = a.property_id.as_deref().map(parse_id).transpose()?;
                Ok(api
                    .preview_database_formula(
                        parse_id(&a.database_id)?,
                        &a.expression,
                        property,
                        a.tz.as_deref(),
                    )
                    .await
                    .map(|preview| json!(preview)))
            };
            run.await
        }
        _ => return None,
    };
    Some(result)
}
