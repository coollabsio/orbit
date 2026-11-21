//! Project lifecycle and members, task bulk writes and reorders, cycle rollover and GitHub links for API tokens and MCP.
//!
//! A trashed project is in no token's `project_ids`, so the project trash and its restore and
//! permanent delete need a credential for all projects, as creating a project does.
use std::collections::HashSet;

use orbit_platform::{Id, TimestampMillis};
use serde::Serialize;
use utoipa::ToSchema;

use super::{
    Authorized, CycleView, Result, TaskPatch, TaskView, TokenApiError, cycle_views, page_limit,
    validate_task_input,
};
use crate::repositories::api_tokens::ApiTokenScope;
use crate::repositories::project_members::ProjectMemberRecord;
use crate::repositories::sub_issues::AutoClosed;
use crate::repositories::tasks::{
    GithubLink, MilestoneRecord, Page, ProjectRecord, StatusRecord, TaskChanges, TaskError,
    TaskRepository, TaskTrashEmptied, TaskUpdate, TokenActor, TrashedProject,
};

/// The most tasks, statuses or milestones one bulk write or reorder takes, as in the web app.
const MAX_ITEMS: usize = 100;

/// One task of a bulk update: the task (UUID or identifier), the `version` read and the patch.
#[derive(Debug, Default)]
pub struct BulkTaskUpdate {
    pub task: String,
    pub expected_version: u64,
    pub patch: TaskPatch,
}

/// The tasks after a bulk write, and the parents that the sub-issue automation closed.
#[derive(Debug, Serialize, ToSchema)]
#[schema(as = PublicBulkTasks)]
pub struct BulkTasks {
    pub items: Vec<TaskView>,
    /// Parent tasks closed because all their sub-issues are done, with their new status.
    #[schema(value_type = Vec<Object>)]
    pub auto_closed: Vec<AutoClosed>,
}

/// One item of a reorder: a task (UUID or identifier), status or milestone, the `version` read
/// and its new position (lower comes first).
#[derive(Debug)]
pub struct ReorderItem {
    pub id: String,
    pub expected_version: u64,
    pub position: i64,
}

/// What "move unfinished tasks" did: how many tasks moved, and the cycle they moved to.
#[derive(Debug, Serialize, ToSchema)]
#[schema(as = PublicCycleMove)]
pub struct CycleMove {
    pub moved: i64,
    #[schema(value_type = Object)]
    pub to_cycle: CycleView,
}

impl Authorized<'_> {
    /// Updates up to 100 tasks of approved projects in one transaction: all or none. Each
    /// `expected_version` is the task's `version` as read.
    pub async fn bulk_update_tasks(&self, items: Vec<BulkTaskUpdate>) -> Result<BulkTasks> {
        self.need(ApiTokenScope::TasksWrite)?;
        if items.is_empty() || items.len() > MAX_ITEMS {
            return Err(TokenApiError::Invalid("Send 1 to 100 updates"));
        }
        let mut updates = Vec::with_capacity(items.len());
        for item in items {
            let patch = item.patch;
            validate_task_input(
                patch.title.as_deref(),
                patch.description.as_deref(),
                patch.priority.as_deref(),
            )?;
            let recurrence = patch
                .recurrence
                .map(|value| value.map(|recurrence| recurrence.rule()).transpose())
                .transpose()?;
            if let Some(Some(original)) = patch.duplicate_of_id
                && self.task(&original.to_string()).await.is_err()
            {
                return Err(TaskError::Invalid {
                    field: "duplicate_of_id",
                }
                .into());
            }
            updates.push(TaskUpdate {
                id: self.task(&item.task).await?.id,
                expected_version: item.expected_version,
                changes: TaskChanges {
                    title: patch.title,
                    description: patch.description,
                    status_id: patch.status_id,
                    priority: patch.priority,
                    assignee_ids: patch.assignee_ids,
                    label_ids: patch.label_ids,
                    due_start_at: patch.due_start_at,
                    due_at: patch.due_at,
                    parent_task_id: patch.parent_task_id,
                    recurrence,
                    milestone_id: patch.milestone_id,
                    cycle_id: patch.cycle_id,
                    estimate: patch.estimate,
                    duplicate_of_id: patch.duplicate_of_id,
                    ..Default::default()
                },
            });
        }
        distinct(updates.iter().map(|update| update.id))?;
        let p = self.principal;
        let outcome = self
            .granted()
            .bulk_update_tasks(
                p.workspace_id,
                p.actor_id,
                &updates,
                self.request_id,
                TimestampMillis::now(),
            )
            .await?;
        Ok(BulkTasks {
            items: self.views(outcome.tasks).await?,
            auto_closed: outcome.auto_closed,
        })
    }

    /// Sets the board position of up to 100 tasks of approved projects in one transaction.
    pub async fn reorder_tasks(&self, items: Vec<ReorderItem>) -> Result<Vec<TaskView>> {
        self.need(ApiTokenScope::TasksWrite)?;
        let mut moves = Vec::with_capacity(items.len());
        for item in reorder_items(items)? {
            moves.push((
                self.task(&item.id).await?.id,
                item.expected_version,
                item.position,
            ));
        }
        distinct(moves.iter().map(|(id, _, _)| *id))?;
        let p = self.principal;
        let tasks = self
            .granted()
            .reorder_tasks(
                p.workspace_id,
                p.actor_id,
                &moves,
                self.request_id,
                TimestampMillis::now(),
            )
            .await?;
        self.views(tasks).await
    }

    /// The GitHub issues and pull requests linked to a task of an approved project.
    pub async fn github_links(&self, task_ref: &str) -> Result<Vec<GithubLink>> {
        self.need(ApiTokenScope::TasksRead)?;
        let task = self.task(task_ref).await?;
        Ok(self
            .api
            .tasks
            .github_links(self.principal.workspace_id, task.id)
            .await?)
    }

    /// Deletes forever every task and project in the trash. The trash spans the whole
    /// workspace, so it takes a credential for all projects of a workspace owner or admin.
    pub async fn empty_task_trash(&self) -> Result<TaskTrashEmptied> {
        self.need(ApiTokenScope::TasksWrite)?;
        self.all_projects("Emptying the trash needs a token for all projects")?;
        let p = self.principal;
        self.ungranted()
            .empty_trash(
                p.workspace_id,
                p.actor_id,
                self.request_id,
                TimestampMillis::now(),
            )
            .await
            .map_err(|error| match error {
                TaskError::Forbidden => TokenApiError::Forbidden(
                    "Only workspace owners and administrators may empty the trash",
                ),
                error => error.into(),
            })
    }

    /// Moves an approved project with its tasks to the trash for 30 days.
    pub async fn delete_project(&self, project_id: Id, expected_version: u64) -> Result<()> {
        self.approved_write(project_id)?;
        let p = self.principal;
        self.granted()
            .delete_project(
                p.workspace_id,
                project_id,
                p.actor_id,
                expected_version,
                self.request_id,
                TimestampMillis::now(),
            )
            .await
            .map_err(manage_forbidden)
    }

    /// The projects in the trash that the token's creator sees, last trashed first.
    pub async fn project_trash(
        &self,
        cursor: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Page<ProjectRecord>> {
        self.need(ApiTokenScope::TasksRead)?;
        let limit = page_limit(cursor, limit)?;
        self.all_projects("The project trash needs a token for all projects")?;
        let p = self.principal;
        Ok(self
            .ungranted()
            .project_trash(
                p.workspace_id,
                p.actor_id,
                cursor,
                limit,
                TimestampMillis::now(),
            )
            .await?)
    }

    /// A project in the trash, with its statuses and who trashed it.
    pub async fn trashed_project(&self, project_id: Id) -> Result<TrashedProject> {
        self.need(ApiTokenScope::TasksRead)?;
        self.all_projects("The project trash needs a token for all projects")?;
        let p = self.principal;
        Ok(self
            .ungranted()
            .trashed_project(
                p.workspace_id,
                project_id,
                p.actor_id,
                TimestampMillis::now(),
            )
            .await?)
    }

    /// Restores a trashed project with its tasks; the token covers it from the next request on.
    pub async fn restore_project(
        &self,
        project_id: Id,
        expected_version: u64,
    ) -> Result<ProjectRecord> {
        self.need(ApiTokenScope::TasksWrite)?;
        self.all_projects("Restoring a project needs a token for all projects")?;
        let p = self.principal;
        self.ungranted()
            .restore_project(
                p.workspace_id,
                project_id,
                p.actor_id,
                expected_version,
                self.request_id,
                TimestampMillis::now(),
            )
            .await
            .map_err(|error| match error {
                TaskError::RestoreConflict { .. } => TokenApiError::Conflict(
                    "A live project uses this project's key: change that key first",
                ),
                error => manage_forbidden(error),
            })
    }

    /// Deletes a trashed project forever with all its tasks. Takes a workspace owner or admin.
    pub async fn purge_project(&self, project_id: Id) -> Result<()> {
        self.need(ApiTokenScope::TasksWrite)?;
        self.all_projects("Deleting a project forever needs a token for all projects")?;
        let p = self.principal;
        self.ungranted()
            .purge_project(
                p.workspace_id,
                project_id,
                p.actor_id,
                self.request_id,
                TimestampMillis::now(),
            )
            .await
            .map_err(|error| match error {
                TaskError::Forbidden => TokenApiError::Forbidden(
                    "Only workspace owners and administrators may delete projects forever",
                ),
                TaskError::ProjectNotTrashed => TokenApiError::Conflict(
                    "Only projects in the trash can be deleted forever: delete the project first",
                ),
                error => error.into(),
            })
    }

    /// The members of an approved project. They matter while the project is private; owners
    /// and admins see it without being listed.
    pub async fn project_members(&self, project_id: Id) -> Result<Vec<ProjectMemberRecord>> {
        self.need(ApiTokenScope::TasksRead)?;
        if !self.approved(project_id) {
            return Err(TokenApiError::NotAvailable);
        }
        let p = self.principal;
        Ok(self
            .api
            .tasks
            .project_members(p.workspace_id, project_id, p.actor_id)
            .await?)
    }

    /// Adds a workspace member (not a guest) to an approved project; adding one again changes
    /// nothing.
    pub async fn add_project_member(
        &self,
        project_id: Id,
        user_id: Id,
    ) -> Result<ProjectMemberRecord> {
        self.approved_write(project_id)?;
        let p = self.principal;
        self.api
            .tasks
            .add_project_member(
                p.workspace_id,
                project_id,
                p.actor_id,
                user_id,
                self.request_id,
                TimestampMillis::now(),
            )
            .await
            .map_err(|error| match error {
                TaskError::Invalid { field: "user_id" } => TokenApiError::Invalid(
                    "user_id must be a workspace member who is not a guest (see list_members)",
                ),
                error => manage_forbidden(error),
            })
    }

    pub async fn remove_project_member(&self, project_id: Id, user_id: Id) -> Result<()> {
        self.approved_write(project_id)?;
        let p = self.principal;
        self.api
            .tasks
            .remove_project_member(
                p.workspace_id,
                project_id,
                p.actor_id,
                user_id,
                self.request_id,
                TimestampMillis::now(),
            )
            .await
            .map_err(manage_forbidden)
    }

    /// Sets the board order of statuses of an approved project; returns the moved statuses.
    pub async fn reorder_statuses(
        &self,
        project_id: Id,
        items: Vec<ReorderItem>,
    ) -> Result<Vec<StatusRecord>> {
        self.approved_write(project_id)?;
        let items = id_items(items)?;
        let p = self.principal;
        self.granted()
            .reorder_statuses(
                p.workspace_id,
                project_id,
                p.actor_id,
                &items,
                self.request_id,
                TimestampMillis::now(),
            )
            .await
            .map_err(manage_forbidden)
    }

    /// Sets the order of milestones of an approved project; returns the project's milestones.
    pub async fn reorder_milestones(
        &self,
        project_id: Id,
        items: Vec<ReorderItem>,
    ) -> Result<Vec<MilestoneRecord>> {
        self.approved_write(project_id)?;
        let items = id_items(items)?;
        let p = self.principal;
        Ok(self
            .granted()
            .reorder_milestones(
                p.workspace_id,
                project_id,
                p.actor_id,
                &items,
                self.request_id,
                TimestampMillis::now(),
            )
            .await?)
    }

    /// Moves the cycle's unfinished tasks to the project's next cycle.
    pub async fn move_unfinished_cycle_tasks(
        &self,
        project_id: Id,
        cycle_id: Id,
    ) -> Result<CycleMove> {
        self.approved_write(project_id)?;
        let p = self.principal;
        let now = TimestampMillis::now();
        let outcome = self
            .granted()
            .move_unfinished_tasks(
                p.workspace_id,
                project_id,
                cycle_id,
                p.actor_id,
                self.request_id,
                now,
            )
            .await
            .map_err(|error| match error {
                TaskError::NoNextCycle => TokenApiError::Conflict(
                    "The project has no cycle after this one: create the next cycle first",
                ),
                error => error.into(),
            })?;
        let cycles = self
            .api
            .tasks
            .cycles(p.workspace_id, project_id, p.actor_id)
            .await?;
        let to_cycle = cycle_views(cycles, now)
            .into_iter()
            .find(|view| view.cycle.id == outcome.to_cycle.id)
            .unwrap_or(CycleView {
                cycle: outcome.to_cycle,
                relative: None,
            });
        Ok(CycleMove {
            moved: outcome.moved,
            to_cycle,
        })
    }

    /// A task write in an approved project.
    fn approved_write(&self, project_id: Id) -> Result<()> {
        self.need(ApiTokenScope::TasksWrite)?;
        if self.approved(project_id) {
            Ok(())
        } else {
            Err(TokenApiError::NotAvailable)
        }
    }

    fn all_projects(&self, refusal: &'static str) -> Result<()> {
        if self.principal.all_projects {
            Ok(())
        } else {
            Err(TokenApiError::Forbidden(refusal))
        }
    }

    /// The task repository without the project grant, for a credential for all projects that
    /// works on trashed projects (never in the grant); writes are still the token's.
    fn ungranted(&self) -> TaskRepository {
        self.api
            .tasks
            .as_ref()
            .clone()
            .with_token(TokenActor {
                token_id: self.principal.token_id,
                service_account: self
                    .principal
                    .service_account
                    .as_ref()
                    .map(|account| (account.id, account.name.clone())),
            })
            .with_revisions()
    }
}

fn reorder_items(items: Vec<ReorderItem>) -> Result<Vec<ReorderItem>> {
    if items.is_empty() || items.len() > MAX_ITEMS {
        return Err(TokenApiError::Invalid("Send 1 to 100 items"));
    }
    Ok(items)
}

/// Status and milestone reorder items: their ids, each once.
fn id_items(items: Vec<ReorderItem>) -> Result<Vec<(Id, u64, i64)>> {
    let items = reorder_items(items)?
        .into_iter()
        .map(|item| {
            item.id
                .parse()
                .map(|id| (id, item.expected_version, item.position))
                .map_err(|_| TokenApiError::Invalid("Invalid identifier in items"))
        })
        .collect::<Result<Vec<_>>>()?;
    distinct(items.iter().map(|(id, _, _)| *id))?;
    Ok(items)
}

fn distinct(ids: impl Iterator<Item = Id>) -> Result<()> {
    let mut seen = HashSet::new();
    for id in ids {
        if !seen.insert(id) {
            return Err(TokenApiError::Invalid("Each item must name a different id"));
        }
    }
    Ok(())
}

/// The project is already approved, so a refusal is the manage permission.
fn manage_forbidden(error: TaskError) -> TokenApiError {
    match error {
        TaskError::Forbidden => {
            TokenApiError::Forbidden("This needs a member who may manage the project")
        }
        error => error.into(),
    }
}
