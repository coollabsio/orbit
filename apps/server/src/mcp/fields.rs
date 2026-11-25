//! Custom task fields (#87): list the gallery or a project's fields, read and set a task's
//! values, add a field and choose a project's fields. Renaming a field, replacing its options and
//! deleting it are not tools: they change or delete values in every project that shows the field,
//! so they stay with people (the web app) and the REST API.
use rmcp::ErrorData;
use rmcp::model::{CallToolRequestParams, Tool};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{arguments, definition, parse_id, parse_ids, write_definition};
use crate::repositories::api_tokens::ApiTokenScope;
use crate::repositories::tasks::{
    TaskFieldKind, TaskFieldOptionInput, TaskFieldRecord, TaskFieldValue,
};
use crate::token_api::{Authorized, TokenApiError};

/// What `set_task_field_value` takes per kind.
const SET_VALUE: &str = "Set or clear a task's value of a custom field that the task's project shows (see list_task_fields with project_id). Value by the field's kind: text and url take a string (url: http or https); number takes a JSON number; date takes \"YYYY-MM-DD\"; select takes one option id of the field (from list_task_fields, not the option name); multi_select takes a list of option ids; checkbox takes true. null clears any value (so do \"\", false and []). A time_tracking field takes no value: use log_time. Returns the task's values as get_task_field_values does.";

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ListFieldsArgs {
    /// An approved project: list the fields it shows, in its order (only these take values on
    /// its tasks). Without it, list the workspace's whole field gallery.
    project_id: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct TaskValuesArgs {
    /// Task id, or its identifier such as ENG-12.
    task_id: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SetValueArgs {
    /// Task id, or its identifier such as ENG-12.
    task_id: String,
    /// A field that the task's project shows (list_task_fields with the project_id).
    field_id: String,
    /// A string for text and url, a number for number, "YYYY-MM-DD" for date, an option id for
    /// select, a list of option ids for multi_select, true for checkbox; null clears it.
    value: Value,
}

#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum FieldKind {
    Text,
    Number,
    Date,
    Select,
    MultiSelect,
    Checkbox,
    Url,
    TimeTracking,
}

impl From<FieldKind> for TaskFieldKind {
    fn from(kind: FieldKind) -> Self {
        match kind {
            FieldKind::Text => Self::Text,
            FieldKind::Number => Self::Number,
            FieldKind::Date => Self::Date,
            FieldKind::Select => Self::Select,
            FieldKind::MultiSelect => Self::MultiSelect,
            FieldKind::Checkbox => Self::Checkbox,
            FieldKind::Url => Self::Url,
            FieldKind::TimeTracking => Self::TimeTracking,
        }
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct OptionArgs {
    /// 1 to 100 characters, unique in the field.
    name: String,
    /// `#rrggbb`.
    color: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct CreateFieldArgs {
    /// 1 to 100 characters, unique in the workspace (case-insensitive).
    name: String,
    /// The kind never changes after creation.
    kind: FieldKind,
    /// The choices of a select or multi_select field, up to 100; leave it out for other kinds.
    #[serde(default)]
    options: Vec<OptionArgs>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ProjectFieldsArgs {
    project_id: String,
    /// Every gallery field the project shows, in order; a field left out leaves the project
    /// (tasks keep its values). Read the current list with list_task_fields first.
    field_ids: Vec<String>,
}

pub(super) fn scope(name: &str) -> Option<ApiTokenScope> {
    match name {
        "list_task_fields" | "get_task_field_values" => Some(ApiTokenScope::TasksRead),
        "set_task_field_value" | "create_task_field" | "set_project_task_fields" => {
            Some(ApiTokenScope::TasksWrite)
        }
        _ => None,
    }
}

pub(super) fn tools() -> Vec<Tool> {
    vec![
        definition::<ListFieldsArgs>(
            "list_task_fields",
            "List custom task fields: with project_id, the fields that approved project shows, in order (only these take values on its tasks); without it, the workspace's whole gallery by name. Each field has id, name, kind (text, number, date, select, multi_select, checkbox, url, time_tracking), options (id, name, color; for select and multi_select) and version. A project that shows the time_tracking field takes log_time; its value on a task is the task's logged minutes.",
        ),
        definition::<TaskValuesArgs>(
            "get_task_field_values",
            "Read a task's custom field values: one item per field with a value (field_id, name, kind, value, and option_names for select and multi_select). Fields without a value are left out.",
        ),
        write_definition::<SetValueArgs>("set_task_field_value", SET_VALUE, false),
        write_definition::<CreateFieldArgs>(
            "create_task_field",
            "Add a custom field to the workspace's gallery. No project shows it until set_project_task_fields adds it. Check list_task_fields first: names are unique in the workspace, and a workspace has at most one time_tracking field. Returns the field.",
            false,
        ),
        write_definition::<ProjectFieldsArgs>(
            "set_project_task_fields",
            "Set which gallery fields an approved project shows and their order. Send the whole list: a field left out leaves the project (tasks keep its values, hidden). Needs a member who may manage the project. Returns the project's fields.",
            true,
        ),
    ]
}

/// Runs a tool of this module; `None` for another tool's name.
pub(super) async fn call(
    api: &Authorized<'_>,
    request: &CallToolRequestParams,
) -> Option<Result<Result<Value, TokenApiError>, ErrorData>> {
    let result = match request.name.as_ref() {
        "list_task_fields" => {
            let run = async {
                let a: ListFieldsArgs = arguments(request)?;
                let project = a.project_id.as_deref().map(parse_id).transpose()?;
                Ok(match project {
                    Some(project) => api.project_task_fields(project).await,
                    None => api.task_fields().await,
                }
                .map(|items| json!({ "items": items })))
            };
            run.await
        }
        "get_task_field_values" => {
            let run = async {
                let a: TaskValuesArgs = arguments(request)?;
                Ok(named_values(api, api.task_field_values(&a.task_id).await).await)
            };
            run.await
        }
        "set_task_field_value" => {
            let run = async {
                let a: SetValueArgs = arguments(request)?;
                let field = parse_id(&a.field_id)?;
                let values = api.set_task_field_value(&a.task_id, field, a.value).await;
                Ok(named_values(api, values).await)
            };
            run.await
        }
        "create_task_field" => {
            let run = async {
                let a: CreateFieldArgs = arguments(request)?;
                let options = a
                    .options
                    .into_iter()
                    .map(|option| TaskFieldOptionInput {
                        id: None,
                        name: option.name,
                        color: option.color,
                    })
                    .collect();
                Ok(api
                    .create_task_field(&a.name, a.kind.into(), options)
                    .await
                    .map(|field| json!(field)))
            };
            run.await
        }
        "set_project_task_fields" => {
            let run = async {
                let a: ProjectFieldsArgs = arguments(request)?;
                let project = parse_id(&a.project_id)?;
                let fields = parse_ids(&a.field_ids)?;
                Ok(api
                    .set_project_task_fields(project, &fields)
                    .await
                    .map(|items| json!({ "items": items })))
            };
            run.await
        }
        _ => return None,
    };
    Some(result)
}

/// The values with each field's name and kind, and the option names of select values, so the
/// model does not need a second call to read them.
async fn named_values(
    api: &Authorized<'_>,
    values: Result<Vec<TaskFieldValue>, TokenApiError>,
) -> Result<Value, TokenApiError> {
    let values = values?;
    let gallery = api.value_fields().await?;
    let items: Vec<Value> = values
        .into_iter()
        .map(|value| {
            let field = gallery.iter().find(|field| field.id == value.field_id);
            let mut item = json!({
                "field_id": value.field_id,
                "name": field.map(|field| field.name.as_str()),
                "kind": field.map(|field| field.kind),
                "value": value.value,
            });
            if let Some(names) = field.and_then(|field| option_names(field, &item["value"])) {
                item["option_names"] = names;
            }
            item
        })
        .collect();
    Ok(json!({ "items": items }))
}

fn option_names(field: &TaskFieldRecord, value: &Value) -> Option<Value> {
    let name = |id: &Value| {
        field
            .options
            .iter()
            .find(|option| Some(option.id.as_str()) == id.as_str())
            .map(|option| option.name.clone())
    };
    match field.kind {
        TaskFieldKind::Select => name(value).map(|name| json!([name])),
        TaskFieldKind::MultiSelect => value
            .as_array()
            .map(|ids| json!(ids.iter().filter_map(name).collect::<Vec<_>>())),
        _ => None,
    }
}
