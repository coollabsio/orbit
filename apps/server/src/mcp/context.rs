//! Teamspaces and task activity.
use rmcp::ErrorData;
use rmcp::model::{CallToolRequestParams, Tool};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{arguments, definition};
use crate::repositories::api_tokens::ApiTokenScope;
use crate::token_api::{Authorized, TokenApiError};

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct NoArgs {}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct TaskActivityArgs {
    /// Task id, or its identifier such as ENG-12.
    task_id: String,
    /// Continue from the next_cursor of the previous response.
    cursor: Option<String>,
    /// Maximum number of events, from 1 to 100. Default: 50.
    limit: Option<usize>,
}

pub(super) fn scope(name: &str) -> Option<ApiTokenScope> {
    match name {
        "list_teamspaces" => Some(ApiTokenScope::DocsRead),
        "list_task_activity" => Some(ApiTokenScope::TasksRead),
        _ => None,
    }
}

pub(super) fn tools() -> Vec<Tool> {
    vec![
        definition::<NoArgs>(
            "list_teamspaces",
            "List the teamspaces the connected user sees, first one (the default) first. Pass an id as teamspace_id to create_doc or move_doc. project_id is the linked project, null for global docs or a project that is not approved.",
        ),
        definition::<TaskActivityArgs>(
            "list_task_activity",
            "List a task's history, newest first: who did what and when. metadata depends on the action; references to tasks and projects that are not approved are null with a <field>_hidden flag.",
        ),
    ]
}

/// Runs a tool of this module; `None` for another tool's name.
pub(super) async fn call(
    api: &Authorized<'_>,
    request: &CallToolRequestParams,
) -> Option<Result<Result<Value, TokenApiError>, ErrorData>> {
    let result = match request.name.as_ref() {
        "list_teamspaces" => {
            let run = async {
                let _: NoArgs = arguments(request)?;
                Ok(api
                    .list_teamspaces()
                    .await
                    .map(|items| json!({ "items": items })))
            };
            run.await
        }
        "list_task_activity" => {
            let run = async {
                let a: TaskActivityArgs = arguments(request)?;
                Ok(api
                    .task_activity(&a.task_id, a.cursor.as_deref(), a.limit)
                    .await
                    .map(|page| json!({ "items": page.items, "next_cursor": page.next_cursor })))
            };
            run.await
        }
        _ => return None,
    };
    Some(result)
}
