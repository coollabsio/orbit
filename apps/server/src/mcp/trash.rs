//! Task and doc trash tools: list, read and restore.
use rmcp::ErrorData;
use rmcp::model::{CallToolRequestParams, Tool};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{EmptyArgs, arguments, definition, parse_id, write_definition};
use crate::repositories::api_tokens::ApiTokenScope;
use crate::token_api::{Authorized, TokenApiError};

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ListTaskTrashArgs {
    /// The next_cursor of the previous page.
    cursor: Option<String>,
    /// Tasks per page, from 1 to 100. Default: 50. A page can hold fewer; follow next_cursor.
    limit: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct TrashedTaskArgs {
    /// A task in the trash, by UUID or identifier (`ENG-12`), from `list_task_trash`.
    task_id: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct RestoreTaskArgs {
    /// A task in the trash, by UUID or identifier (`ENG-12`), from `list_task_trash`.
    task_id: String,
    /// The `version` that you read.
    expected_version: u64,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct RestoreDocArgs {
    /// A doc in the trash, from `list_doc_trash`.
    page_id: String,
    /// The `version` that you read.
    expected_version: u64,
}

pub(super) fn scope(name: &str) -> Option<ApiTokenScope> {
    match name {
        "list_task_trash" | "get_trashed_task" => Some(ApiTokenScope::TasksRead),
        "restore_task" => Some(ApiTokenScope::TasksWrite),
        "list_doc_trash" => Some(ApiTokenScope::DocsRead),
        "restore_doc" => Some(ApiTokenScope::DocsWrite),
        _ => None,
    }
}

pub(super) fn tools() -> Vec<Tool> {
    vec![
        definition::<ListTaskTrashArgs>(
            "list_task_trash",
            "List the tasks in the trash of approved projects, last trashed first. Tasks stay in the trash for 30 days. Each task has deleted_at and trashed_descendant_count, the sub-issues that a restore brings back too.",
        ),
        definition::<TrashedTaskArgs>(
            "get_trashed_task",
            "Read a task in the trash, with its status, labels and the user who trashed it (deleted_by).",
        ),
        write_definition::<RestoreTaskArgs>(
            "restore_task",
            "Restore a task from the trash with the sub-issues trashed with it. A task whose parent is still in the trash comes back top-level. Returns the task as get_task does.",
            false,
        ),
        definition::<EmptyArgs>(
            "list_doc_trash",
            "List the docs in the trash that you can see, last trashed first. Docs trashed with a parent doc are not listed: restoring the parent brings them back. Docs stay in the trash for 30 days.",
        ),
        write_definition::<RestoreDocArgs>(
            "restore_doc",
            "Restore a doc from the trash with the sub-docs trashed with it. A row of a database that is in the trash cannot be restored before the database. Returns the doc as get_doc does.",
            false,
        ),
    ]
}

/// Runs a trash tool; `None` for another tool's name.
pub(super) async fn call(
    api: &Authorized<'_>,
    request: &CallToolRequestParams,
) -> Option<Result<Result<Value, TokenApiError>, ErrorData>> {
    let result = match request.name.as_ref() {
        "list_task_trash" => {
            let run = async {
                let a: ListTaskTrashArgs = arguments(request)?;
                Ok(api
                    .task_trash(a.cursor.as_deref(), a.limit)
                    .await
                    .map(|page| json!(page)))
            };
            run.await
        }
        "get_trashed_task" => {
            let run = async {
                let a: TrashedTaskArgs = arguments(request)?;
                Ok(api
                    .get_trashed_task(&a.task_id)
                    .await
                    .map(|task| json!(task)))
            };
            run.await
        }
        "restore_task" => {
            let run = async {
                let a: RestoreTaskArgs = arguments(request)?;
                Ok(api
                    .restore_task(&a.task_id, a.expected_version)
                    .await
                    .map(|task| json!(task)))
            };
            run.await
        }
        "list_doc_trash" => {
            let run = async {
                let _: EmptyArgs = arguments(request)?;
                Ok(api.doc_trash().await.map(|trash| json!(trash)))
            };
            run.await
        }
        "restore_doc" => {
            let run = async {
                let a: RestoreDocArgs = arguments(request)?;
                Ok(api
                    .restore_doc(parse_id(&a.page_id)?, a.expected_version)
                    .await
                    .map(|doc| json!(doc)))
            };
            run.await
        }
        _ => return None,
    };
    Some(result)
}
