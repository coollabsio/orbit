//! Saved task views and the token user's view favorites.
//!
//! Views, and favorites even more, belong to a person: a service-account token, which acts as the
//! workspace owner, gets [`TokenApiError::Forbidden`]. A view whose filter names a project outside
//! the grant is hidden (it is not listed, and reads and writes answer not found), and a create or
//! update cannot name such a project. Reads need `tasks:read`, writes `tasks:write`.
use orbit_platform::Id;

use super::{Authorized, Result, TokenApiError};
use crate::repositories::api_tokens::ApiTokenScope;
use crate::repositories::task_filter::{FilterField, FilterGroup, FilterNode, ViewState};
use crate::repositories::tasks::TaskError;
use crate::repositories::views::{SavedViewRecord, ViewCreate, ViewRepository, ViewUpdate};

/// Upper bound for one favorites reorder, as the web route has.
const MAX_ORDER_IDS: usize = 500;

const SERVICE_ACCOUNT_VIEWS: &str = "A service-account token has no saved views or favorites: they belong to a person. Use a personal API token.";
const VIEW_PROJECT_OUTSIDE_GRANT: &str =
    "The view's filter names a project that this token is not approved for";

impl Authorized<'_> {
    /// The views the token user sees (their personal ones and every workspace view), by name.
    pub async fn saved_views(&self) -> Result<Vec<SavedViewRecord>> {
        self.view_scope(ApiTokenScope::TasksRead)?;
        let p = self.principal;
        let views = self
            .view_repository()
            .list_views(p.workspace_id, p.actor_id)
            .await?;
        Ok(views
            .into_iter()
            .filter(|view| self.view_in_grant(view.state.as_ref()))
            .collect())
    }

    pub async fn saved_view(&self, view_id: Id) -> Result<SavedViewRecord> {
        self.view_scope(ApiTokenScope::TasksRead)?;
        self.granted_view(view_id).await
    }

    pub async fn create_view(&self, input: ViewCreate) -> Result<SavedViewRecord> {
        self.view_scope(ApiTokenScope::TasksWrite)?;
        if !self.view_in_grant(Some(&input.state)) {
            return Err(TokenApiError::Invalid(VIEW_PROJECT_OUTSIDE_GRANT));
        }
        let p = self.principal;
        self.view_repository()
            .create_view(p.workspace_id, p.actor_id, input, self.request_id)
            .await
            .map_err(view_error)
    }

    /// Changes a view that the token user may edit (their own, or any workspace view for an owner
    /// or admin). Only the view's owner may change its visibility.
    pub async fn update_view(&self, view_id: Id, input: ViewUpdate) -> Result<SavedViewRecord> {
        self.view_scope(ApiTokenScope::TasksWrite)?;
        if !self.view_in_grant(input.state.as_ref()) {
            return Err(TokenApiError::Invalid(VIEW_PROJECT_OUTSIDE_GRANT));
        }
        self.granted_view(view_id).await?;
        let p = self.principal;
        self.view_repository()
            .update_view(p.workspace_id, p.actor_id, view_id, input, self.request_id)
            .await
            .map_err(view_error)
    }

    pub async fn delete_view(&self, view_id: Id) -> Result<()> {
        self.view_scope(ApiTokenScope::TasksWrite)?;
        self.granted_view(view_id).await?;
        let p = self.principal;
        self.view_repository()
            .delete_view(p.workspace_id, p.actor_id, view_id, self.request_id)
            .await
            .map_err(view_error)
    }

    /// Adds the view to the end of the token user's favorites, or takes it out. Idempotent.
    pub async fn set_view_favorite(&self, view_id: Id, favorite: bool) -> Result<()> {
        self.view_scope(ApiTokenScope::TasksWrite)?;
        self.granted_view(view_id).await?;
        let p = self.principal;
        Ok(self
            .view_repository()
            .set_favorite(
                p.workspace_id,
                p.actor_id,
                view_id,
                favorite,
                self.request_id,
            )
            .await?)
    }

    /// Reorders the token user's favorites. `view_ids` lists every favorite that [`Self::saved_views`]
    /// shows, in the new order; favorites hidden from the token keep their places.
    pub async fn reorder_view_favorites(&self, view_ids: Vec<Id>) -> Result<()> {
        self.view_scope(ApiTokenScope::TasksWrite)?;
        if view_ids.len() > MAX_ORDER_IDS {
            return Err(TokenApiError::Invalid("view_ids has at most 500 IDs"));
        }
        let p = self.principal;
        let repository = self.view_repository();
        let mut favorites: Vec<SavedViewRecord> = repository
            .list_views(p.workspace_id, p.actor_id)
            .await?
            .into_iter()
            .filter(|view| view.is_favorite)
            .collect();
        favorites.sort_by_key(|view| (view.favorite_position, view.id));
        let shown: Vec<Id> = favorites
            .iter()
            .filter(|view| self.view_in_grant(view.state.as_ref()))
            .map(|view| view.id)
            .collect();
        let mut requested = view_ids.clone();
        requested.sort_unstable();
        requested.dedup();
        let mut expected = shown;
        expected.sort_unstable();
        if requested.len() != view_ids.len() || requested != expected {
            return Err(TokenApiError::Invalid(
                "view_ids must list each favorite view (is_favorite in list_views) exactly once",
            ));
        }
        // The shown favorites take the new order in their own slots; hidden ones stay put.
        let mut next = view_ids.into_iter();
        let order: Vec<Id> = favorites
            .iter()
            .map(|view| {
                if self.view_in_grant(view.state.as_ref()) {
                    next.next().unwrap_or(view.id)
                } else {
                    view.id
                }
            })
            .collect();
        repository
            .reorder_favorites(p.workspace_id, p.actor_id, order, self.request_id)
            .await
            .map_err(|error| match error {
                // A favorite changed between the read and the write.
                TaskError::Invalid { .. } => TokenApiError::Invalid(
                    "The favorites changed meanwhile; list the views again and retry",
                ),
                error => error.into(),
            })
    }

    fn view_scope(&self, scope: ApiTokenScope) -> Result<()> {
        self.need(scope)?;
        match self.principal.service_account {
            Some(_) => Err(TokenApiError::Forbidden(SERVICE_ACCOUNT_VIEWS)),
            None => Ok(()),
        }
    }

    fn view_repository(&self) -> ViewRepository {
        ViewRepository::new(self.api.tasks.database().clone())
    }

    async fn granted_view(&self, view_id: Id) -> Result<SavedViewRecord> {
        let p = self.principal;
        let view = self
            .view_repository()
            .get_view(p.workspace_id, p.actor_id, view_id)
            .await?;
        if self.view_in_grant(view.state.as_ref()) {
            Ok(view)
        } else {
            Err(TokenApiError::NotAvailable)
        }
    }

    /// Whether every project that the filter names is approved. A state that no longer parses
    /// names none that the token could use.
    fn view_in_grant(&self, state: Option<&ViewState>) -> bool {
        let mut projects = Vec::new();
        if let Some(state) = state {
            filter_projects(&state.filter, &mut projects);
        }
        projects.iter().all(|raw| {
            raw.parse::<Id>()
                .is_ok_and(|project_id| self.approved(project_id))
        })
    }
}

/// The values of every `project` condition in the group and its subgroups.
fn filter_projects<'a>(group: &'a FilterGroup, out: &mut Vec<&'a str>) {
    for node in &group.children {
        match node {
            FilterNode::Group(group) => filter_projects(group, out),
            FilterNode::Condition(condition) if condition.field == FilterField::Project => {
                match &condition.value {
                    serde_json::Value::Array(values) => {
                        out.extend(values.iter().map(|value| value.as_str().unwrap_or("")));
                    }
                    serde_json::Value::String(value) => out.push(value),
                    serde_json::Value::Null => {}
                    // Not an ID: the token cannot have approved it.
                    _ => out.push(""),
                }
            }
            FilterNode::Condition(_) => {}
        }
    }
}

/// The repository's refusals with what the agent can do about them.
fn view_error(error: TaskError) -> TokenApiError {
    match error {
        TaskError::Forbidden => TokenApiError::Forbidden(
            "Only the view's owner, or a workspace owner or admin for a workspace view, may change it; only its owner may change its visibility",
        ),
        TaskError::Invalid { field: "name" } => {
            TokenApiError::Invalid("A view name has 1 to 80 characters")
        }
        TaskError::Invalid {
            field: "description",
        } => TokenApiError::Invalid("A view description has at most 500 characters"),
        TaskError::Invalid { field: "icon" } => TokenApiError::Invalid(
            "icon is an icon name: ASCII letters, digits, - or _, up to 64 bytes",
        ),
        TaskError::Invalid { field: "color" } => TokenApiError::Invalid("color must be #rrggbb"),
        error => error.into(),
    }
}
