//! Project lifecycle and members, task bulk writes and reorders, cycle rollover and GitHub links for API tokens and MCP.
use rmcp::ErrorData;
use rmcp::model::{CallToolRequestParams, Tool};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    EmptyArgs, UpdateArgs, arguments, definition, parse_due_change, parse_id, parse_ids,
    write_definition,
};
use crate::repositories::api_tokens::ApiTokenScope;
use crate::token_api::{Authorized, BulkTaskUpdate, ReorderItem, TaskPatch, TokenApiError};

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct BulkUpdateArgs {
    /// 1 to 100 updates, each of a different task, with the fields of update_task. All are
    /// written, or none.
    updates: Vec<UpdateArgs>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ReorderItemArgs {
    /// The task (UUID or identifier such as ENG-12), status or milestone.
    id: String,
    /// Its version, from the list tool.
    expected_version: u64,
    /// The new position; lower comes first.
    position: i64,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ReorderTasksArgs {
    /// 1 to 100 tasks, each once.
    items: Vec<ReorderItemArgs>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ProjectReorderArgs {
    project_id: String,
    /// 1 to 100 items of the project, each once.
    items: Vec<ReorderItemArgs>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct TaskArgs {
    /// A task UUID or identifier such as ENG-12.
    task_id: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ProjectArgs {
    project_id: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ProjectVersionArgs {
    project_id: String,
    /// The project's version, from list_projects (or list_project_trash for a restore).
    expected_version: u64,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ProjectTrashArgs {
    /// The next_cursor of the previous page.
    cursor: Option<String>,
    /// Projects per page, from 1 to 100. Default: 50.
    limit: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ProjectMemberArgs {
    project_id: String,
    /// A workspace member who is not a guest, from list_members.
    user_id: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct CycleArgs {
    project_id: String,
    /// The cycle whose unfinished tasks move, from list_cycles.
    cycle_id: String,
}

pub(super) fn scope(name: &str) -> Option<ApiTokenScope> {
    match name {
        "list_task_github_links"
        | "list_project_trash"
        | "get_trashed_project"
        | "list_project_members" => Some(ApiTokenScope::TasksRead),
        "bulk_update_tasks"
        | "reorder_tasks"
        | "empty_task_trash"
        | "delete_project"
        | "restore_project"
        | "purge_project"
        | "add_project_member"
        | "remove_project_member"
        | "reorder_task_statuses"
        | "reorder_milestones"
        | "move_unfinished_cycle_tasks" => Some(ApiTokenScope::TasksWrite),
        _ => None,
    }
}

pub(super) fn tools() -> Vec<Tool> {
    vec![
        write_definition::<BulkUpdateArgs>(
            "bulk_update_tasks",
            "Update up to 100 tasks of approved projects at once; each update takes the arguments of update_task. All are written or none: one stale expected_version fails the whole call. Returns the tasks and the parents that the sub-issue automation closed.",
            false,
        ),
        write_definition::<ReorderTasksArgs>(
            "reorder_tasks",
            "Set the board position of up to 100 tasks of approved projects (lower comes first). expected_version is each task's version from list_tasks or get_task. Returns the moved tasks.",
            false,
        ),
        definition::<TaskArgs>(
            "list_task_github_links",
            "The GitHub issues and pull requests linked to a task of an approved project.",
        ),
        write_definition::<EmptyArgs>(
            "empty_task_trash",
            "Delete every task and project in the workspace's trash forever. Needs a connection for all projects of a workspace owner or administrator. Cannot be undone.",
            true,
        ),
        write_definition::<ProjectVersionArgs>(
            "delete_project",
            "Move an approved project with its tasks to the trash for 30 days (restore_project brings it back). Needs a member who may manage the project.",
            true,
        ),
        definition::<ProjectTrashArgs>(
            "list_project_trash",
            "The projects in the trash, last trashed first, with their version for restore_project. Needs a connection for all projects.",
        ),
        definition::<ProjectArgs>(
            "get_trashed_project",
            "A project in the trash, with its statuses and who trashed it. Needs a connection for all projects.",
        ),
        write_definition::<ProjectVersionArgs>(
            "restore_project",
            "Restore a project from the trash with its tasks; expected_version comes from list_project_trash. Needs a connection for all projects and a member who may manage the project.",
            false,
        ),
        write_definition::<ProjectArgs>(
            "purge_project",
            "Delete a project that is in the trash forever, with all its tasks. Needs a connection for all projects of a workspace owner or administrator. Cannot be undone.",
            true,
        ),
        definition::<ProjectArgs>(
            "list_project_members",
            "The members of an approved project, oldest first. Only they (and workspace owners and admins, who are not listed) see a private project.",
        ),
        write_definition::<ProjectMemberArgs>(
            "add_project_member",
            "Add a workspace member (user_id from list_members) to an approved project; adding one again changes nothing. Needs a member who may manage the project.",
            false,
        ),
        write_definition::<ProjectMemberArgs>(
            "remove_project_member",
            "Remove a member from an approved project; they no longer see it while it is private. Needs a member who may manage the project.",
            true,
        ),
        write_definition::<ProjectReorderArgs>(
            "reorder_task_statuses",
            "Set the board order of statuses of an approved project (ids and versions from list_task_statuses; lower comes first). Needs a member who may manage the project. Returns the moved statuses.",
            false,
        ),
        write_definition::<ProjectReorderArgs>(
            "reorder_milestones",
            "Set the order of milestones of an approved project (ids and versions from list_milestones; lower comes first). Returns the project's milestones.",
            false,
        ),
        write_definition::<CycleArgs>(
            "move_unfinished_cycle_tasks",
            "Move the unfinished tasks of a cycle of an approved project to the project's next cycle (create it first with create_cycle). Returns how many moved and the next cycle.",
            false,
        ),
    ]
}

/// Runs a tool of this module; `None` for another tool's name.
pub(super) async fn call(
    api: &Authorized<'_>,
    request: &CallToolRequestParams,
) -> Option<Result<Result<Value, TokenApiError>, ErrorData>> {
    let result = match request.name.as_ref() {
        "bulk_update_tasks" => {
            let run = async {
                let a: BulkUpdateArgs = arguments(request)?;
                let mut updates = Vec::with_capacity(a.updates.len());
                for item in a.updates {
                    match bulk_item(api, item).await? {
                        Ok(update) => updates.push(update),
                        Err(error) => return Ok(Err(error)),
                    }
                }
                Ok(api.bulk_update_tasks(updates).await.map(|out| json!(out)))
            };
            run.await
        }
        "reorder_tasks" => {
            let run = async {
                let a: ReorderTasksArgs = arguments(request)?;
                Ok(api
                    .reorder_tasks(items(a.items))
                    .await
                    .map(|tasks| json!({ "items": tasks })))
            };
            run.await
        }
        "list_task_github_links" => {
            let run = async {
                let a: TaskArgs = arguments(request)?;
                Ok(api
                    .github_links(&a.task_id)
                    .await
                    .map(|links| json!({ "items": links })))
            };
            run.await
        }
        "empty_task_trash" => {
            let run = async {
                let _: EmptyArgs = arguments(request)?;
                Ok(api.empty_task_trash().await.map(|out| json!(out)))
            };
            run.await
        }
        "delete_project" => {
            let run = async {
                let a: ProjectVersionArgs = arguments(request)?;
                Ok(api
                    .delete_project(parse_id(&a.project_id)?, a.expected_version)
                    .await
                    .map(|()| json!({ "deleted": true })))
            };
            run.await
        }
        "list_project_trash" => {
            let run = async {
                let a: ProjectTrashArgs = arguments(request)?;
                Ok(api
                    .project_trash(a.cursor.as_deref(), a.limit)
                    .await
                    .map(|page| json!(page)))
            };
            run.await
        }
        "get_trashed_project" => {
            let run = async {
                let a: ProjectArgs = arguments(request)?;
                Ok(api
                    .trashed_project(parse_id(&a.project_id)?)
                    .await
                    .map(|project| json!(project)))
            };
            run.await
        }
        "restore_project" => {
            let run = async {
                let a: ProjectVersionArgs = arguments(request)?;
                Ok(api
                    .restore_project(parse_id(&a.project_id)?, a.expected_version)
                    .await
                    .map(|project| json!(project)))
            };
            run.await
        }
        "purge_project" => {
            let run = async {
                let a: ProjectArgs = arguments(request)?;
                Ok(api
                    .purge_project(parse_id(&a.project_id)?)
                    .await
                    .map(|()| json!({ "deleted": true })))
            };
            run.await
        }
        "list_project_members" => {
            let run = async {
                let a: ProjectArgs = arguments(request)?;
                Ok(api
                    .project_members(parse_id(&a.project_id)?)
                    .await
                    .map(|members| json!({ "items": members })))
            };
            run.await
        }
        "add_project_member" => {
            let run = async {
                let a: ProjectMemberArgs = arguments(request)?;
                Ok(api
                    .add_project_member(parse_id(&a.project_id)?, parse_id(&a.user_id)?)
                    .await
                    .map(|member| json!(member)))
            };
            run.await
        }
        "remove_project_member" => {
            let run = async {
                let a: ProjectMemberArgs = arguments(request)?;
                Ok(api
                    .remove_project_member(parse_id(&a.project_id)?, parse_id(&a.user_id)?)
                    .await
                    .map(|()| json!({ "removed": true })))
            };
            run.await
        }
        "reorder_task_statuses" => {
            let run = async {
                let a: ProjectReorderArgs = arguments(request)?;
                Ok(api
                    .reorder_statuses(parse_id(&a.project_id)?, items(a.items))
                    .await
                    .map(|statuses| json!({ "items": statuses })))
            };
            run.await
        }
        "reorder_milestones" => {
            let run = async {
                let a: ProjectReorderArgs = arguments(request)?;
                Ok(api
                    .reorder_milestones(parse_id(&a.project_id)?, items(a.items))
                    .await
                    .map(|milestones| json!({ "items": milestones })))
            };
            run.await
        }
        "move_unfinished_cycle_tasks" => {
            let run = async {
                let a: CycleArgs = arguments(request)?;
                Ok(api
                    .move_unfinished_cycle_tasks(parse_id(&a.project_id)?, parse_id(&a.cycle_id)?)
                    .await
                    .map(|outcome| json!(outcome)))
            };
            run.await
        }
        _ => return None,
    };
    Some(result)
}

fn items(items: Vec<ReorderItemArgs>) -> Vec<ReorderItem> {
    items
        .into_iter()
        .map(|item| ReorderItem {
            id: item.id,
            expected_version: item.expected_version,
            position: item.position,
        })
        .collect()
}

/// An update_task argument set as a bulk item: argument errors are protocol errors, a task
/// reference that names no task is a tool result.
async fn bulk_item(
    api: &Authorized<'_>,
    a: UpdateArgs,
) -> Result<Result<BulkTaskUpdate, TokenApiError>, ErrorData> {
    let status_id = a.status_id.as_deref().map(parse_id).transpose()?;
    let assignee_ids = a.assignee_ids.as_deref().map(parse_ids).transpose()?;
    let label_ids = a.label_ids.as_deref().map(parse_ids).transpose()?;
    let plan_id = |id: Option<Option<String>>| {
        id.map(|id| id.as_deref().map(parse_id).transpose())
            .transpose()
    };
    let (milestone_id, cycle_id) = (plan_id(a.milestone_id)?, plan_id(a.cycle_id)?);
    let due_at = parse_due_change(a.due_at, "due_at")?;
    let due_start_at = parse_due_change(a.due_start_at, "due_start_at")?;
    Ok(async {
        let parent_task_id = match a.parent_task_id {
            Some(parent) => Some(api.optional_task_id(parent.as_deref()).await?),
            None => None,
        };
        let duplicate_of_id = match a.duplicate_of_id {
            Some(original) => Some(api.optional_task_id(original.as_deref()).await?),
            None => None,
        };
        Ok(BulkTaskUpdate {
            task: a.task_id,
            expected_version: a.expected_version,
            patch: TaskPatch {
                title: a.title,
                description: a.description,
                status_id,
                priority: a.priority,
                parent_task_id,
                assignee_ids,
                label_ids,
                recurrence: a.recurrence,
                milestone_id,
                cycle_id,
                estimate: a.estimate,
                duplicate_of_id,
                due_at,
                due_start_at,
            },
        })
    }
    .await)
}
