//! Teamspaces and task activity.
//!
//! Activity metadata can name other tasks and projects (relations, duplicates, parents,
//! auto-close sources, project moves). The token sees only its approved projects, so a
//! reference into another project is blanked: the id and project id become `null`, a related
//! title is removed, and `<field>_hidden: true` says that something was there.
use orbit_platform::{Id, TimestampMillis};
use serde::Serialize;
use serde_json::{Map, Value};
use utoipa::ToSchema;

use super::{Authorized, Result, TokenApiError, page_limit};
use crate::repositories::api_tokens::ApiTokenScope;
use crate::repositories::tasks::Page;
use crate::repositories::teamspaces::{TeamspaceError, TeamspaceRepository};

/// A teamspace as `create_doc` and `move_doc` see it.
#[derive(Debug, Serialize, ToSchema)]
#[schema(as = PublicTeamspace)]
pub struct TeamspaceView {
    #[schema(value_type = String)]
    pub id: Id,
    pub name: String,
    #[schema(required = true)]
    pub icon: Option<String>,
    /// New root docs go here when `teamspace_id` is omitted.
    pub is_default: bool,
    /// The linked project, `null` when the teamspace holds global docs or its project is not
    /// approved for the token.
    #[schema(value_type = Option<String>, required = true)]
    pub project_id: Option<Id>,
    /// True when the teamspace holds global docs (no linked project).
    pub global: bool,
    /// Send it as `expected_version` to change, move or delete the teamspace.
    pub version: u64,
}

#[derive(Debug, Serialize, ToSchema)]
#[schema(as = PublicTaskActivity)]
pub struct TaskActivity {
    #[schema(value_type = String)]
    pub id: Id,
    pub action: String,
    /// `null` for system events and service accounts (see `actor_service_account_name`).
    #[schema(value_type = Option<String>, required = true)]
    pub actor_id: Option<Id>,
    #[schema(required = true)]
    pub actor_name: Option<String>,
    /// Depends on `action`. A task or project outside the approved projects is `null`, with
    /// `<field>_hidden: true`.
    #[schema(value_type = Object)]
    pub metadata: Value,
    #[schema(value_type = String, format = DateTime)]
    pub occurred_at: TimestampMillis,
}

impl From<TeamspaceError> for TokenApiError {
    fn from(error: TeamspaceError) -> Self {
        match error {
            TeamspaceError::NotFound | TeamspaceError::Forbidden => Self::NotAvailable,
            error => Self::Internal(error.to_string()),
        }
    }
}

/// Pairs of (task field, its project field) in activity metadata.
const TASK_REFERENCES: [(&str, &str); 4] = [
    ("related_task_id", "related_task_project_id"),
    ("from", "from_project_id"),
    ("to", "to_project_id"),
    ("source_task_id", "source_project_id"),
];

impl Authorized<'_> {
    /// The workspace's teamspaces the connected person sees, ordered by position.
    pub async fn list_teamspaces(&self) -> Result<Vec<TeamspaceView>> {
        self.content(ApiTokenScope::DocsRead, &self.api.pages)?;
        let p = self.principal;
        let list = TeamspaceRepository::new(self.api.tasks.database().clone())
            .teamspaces(p.workspace_id, p.actor_id)
            .await?;
        Ok(list
            .items
            .into_iter()
            .map(|teamspace| TeamspaceView {
                id: teamspace.id,
                name: teamspace.name,
                icon: teamspace.icon,
                is_default: teamspace.is_default,
                project_id: teamspace.project_id.filter(|id| self.approved(*id)),
                global: teamspace.project_id.is_none(),
                version: teamspace.version,
            })
            .collect())
    }

    /// A task's history, newest first.
    pub async fn task_activity(
        &self,
        task_ref: &str,
        cursor: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Page<TaskActivity>> {
        self.need(ApiTokenScope::TasksRead)?;
        let limit = page_limit(cursor, limit)?;
        let task = self.task(task_ref).await?;
        let p = self.principal;
        let page = self
            .api
            .tasks
            .task_activity(p.workspace_id, task.id, p.actor_id, cursor, limit)
            .await?;
        Ok(Page {
            items: page
                .items
                .into_iter()
                .map(|event| TaskActivity {
                    id: event.id,
                    action: event.action,
                    actor_id: event.actor_id,
                    actor_name: event.actor_name,
                    metadata: self.visible_metadata(event.metadata),
                    occurred_at: event.occurred_at,
                })
                .collect(),
            next_cursor: page.next_cursor,
        })
    }

    /// Blanks references to tasks and projects outside the approved projects, and drops the
    /// token id of the writer.
    fn visible_metadata(&self, metadata: Value) -> Value {
        let Value::Object(mut fields) = metadata else {
            return metadata;
        };
        fields.remove("api_token_id");
        for (task_field, project_field) in TASK_REFERENCES {
            if !self.visible_project(&fields, project_field) {
                fields.insert(task_field.into(), Value::Null);
                fields.insert(project_field.into(), Value::Null);
                if task_field == "related_task_id" {
                    fields.remove("related_task_title");
                }
                fields.insert(format!("{task_field}_hidden"), Value::Bool(true));
            }
        }
        if let Some(Value::Object(project)) = fields
            .get_mut("changes")
            .and_then(|changes| changes.get_mut("project"))
        {
            for side in ["from", "to"] {
                if !self.visible_project(project, side) {
                    project.insert(side.into(), Value::Null);
                    project.insert(format!("{side}_hidden"), Value::Bool(true));
                }
            }
        }
        Value::Object(fields)
    }

    /// False only for a field naming a project that is not approved.
    fn visible_project(&self, fields: &Map<String, Value>, field: &str) -> bool {
        match fields.get(field) {
            None | Some(Value::Null) => true,
            Some(value) => value
                .as_str()
                .and_then(|raw| raw.parse::<Id>().ok())
                .is_some_and(|id| self.approved(id)),
        }
    }
}
