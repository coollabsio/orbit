//! Saved task views and the token user's view favorites.
use rmcp::ErrorData;
use rmcp::model::{CallToolRequestParams, Tool};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{EmptyArgs, arguments, definition, parse_id, parse_ids, present, write_definition};
use crate::repositories::api_tokens::ApiTokenScope;
use crate::repositories::task_filter::{DisplayOptions, FilterGroup, ViewState};
use crate::repositories::views::{ViewCreate, ViewUpdate, Visibility};
use crate::token_api::{Authorized, TokenApiError};

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ViewArgs {
    /// A view id, from list_views.
    view_id: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct CreateViewArgs {
    /// 1 to 80 characters.
    name: String,
    /// Up to 500 characters.
    #[serde(default)]
    description: String,
    /// An icon name: ASCII letters, digits, `-` or `_`, up to 64 bytes.
    icon: Option<String>,
    /// `#rrggbb`.
    color: Option<String>,
    /// `personal` (only the token user sees it) or `workspace` (every member does).
    visibility: String,
    /// The filter and display options, in the shape that list_views shows:
    /// `{"filter": {"op": "and", "children": [{"field": "project", "operator": "is", "value":
    /// ["<project id>"]}]}, "display": {…}}`. Without `filter`, the view shows every task; without
    /// `display`, it has the web app's default display (copy one from list_views to change it).
    /// `project` conditions may name approved projects only.
    state: Value,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct UpdateViewArgs {
    view_id: String,
    /// The view's version, from list_views or get_view.
    expected_version: i64,
    name: Option<String>,
    description: Option<String>,
    /// `null` clears it.
    #[serde(default, deserialize_with = "present")]
    icon: Option<Option<String>>,
    /// `null` clears it.
    #[serde(default, deserialize_with = "present")]
    color: Option<Option<String>>,
    /// `personal` or `workspace`; only the view's owner may change it.
    visibility: Option<String>,
    /// The whole new state, as for create_view.
    state: Option<Value>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct FavoriteArgs {
    view_id: String,
    /// `true` adds the view to the end of the token user's favorites; `false` takes it out.
    favorite: bool,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ReorderArgs {
    /// Every view of list_views with is_favorite, each once, in the new order.
    view_ids: Vec<String>,
}

pub(super) fn scope(name: &str) -> Option<ApiTokenScope> {
    match name {
        "list_views" | "get_view" => Some(ApiTokenScope::TasksRead),
        "create_view"
        | "update_view"
        | "delete_view"
        | "set_view_favorite"
        | "reorder_view_favorites" => Some(ApiTokenScope::TasksWrite),
        _ => None,
    }
}

pub(super) fn tools() -> Vec<Tool> {
    vec![
        definition::<EmptyArgs>(
            "list_views",
            "The saved task views that the token user sees (their personal views and every workspace view), by name: each a task filter and display options (state), with is_favorite and favorite_position. Views whose filter names a project outside the approved ones are left out. Not for service-account tokens.",
        ),
        definition::<ViewArgs>("get_view", "A saved view from list_views."),
        write_definition::<CreateViewArgs>(
            "create_view",
            "Save a task view: a name and a state (filter and display options). Returns it as list_views does.",
            false,
        ),
        write_definition::<UpdateViewArgs>(
            "update_view",
            "Change a saved view: name, description, icon, color, visibility or state. Missing fields stay. The token user must own the view, or be a workspace owner or admin for a workspace view; only the owner may change visibility.",
            false,
        ),
        write_definition::<ViewArgs>(
            "delete_view",
            "Delete a saved view, for every member (it leaves their favorites too). Same rule as update_view.",
            true,
        ),
        write_definition::<FavoriteArgs>(
            "set_view_favorite",
            "Add a view to the token user's favorites (at the end) or take it out. Idempotent.",
            false,
        ),
        write_definition::<ReorderArgs>(
            "reorder_view_favorites",
            "Reorder the token user's favorite views: list every favorite of list_views once, in the new order.",
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
        "list_views" => {
            let run = async {
                let _: EmptyArgs = arguments(request)?;
                Ok(api
                    .saved_views()
                    .await
                    .map(|items| json!({ "items": items })))
            };
            run.await
        }
        "get_view" => {
            let run = async {
                let a: ViewArgs = arguments(request)?;
                Ok(api
                    .saved_view(parse_id(&a.view_id)?)
                    .await
                    .map(|view| json!(view)))
            };
            run.await
        }
        "create_view" => {
            let run = async {
                let a: CreateViewArgs = arguments(request)?;
                let input = ViewCreate {
                    name: a.name,
                    description: a.description,
                    icon: a.icon,
                    color: a.color,
                    visibility: visibility(&a.visibility)?,
                    state: view_state(a.state)?,
                };
                Ok(api.create_view(input).await.map(|view| json!(view)))
            };
            run.await
        }
        "update_view" => {
            let run = async {
                let a: UpdateViewArgs = arguments(request)?;
                let changes = ViewUpdate {
                    expected_version: a.expected_version,
                    name: a.name,
                    description: a.description,
                    icon: a.icon,
                    color: a.color,
                    visibility: a.visibility.as_deref().map(visibility).transpose()?,
                    state: a.state.map(view_state).transpose()?,
                };
                Ok(api
                    .update_view(parse_id(&a.view_id)?, changes)
                    .await
                    .map(|view| json!(view)))
            };
            run.await
        }
        "delete_view" => {
            let run = async {
                let a: ViewArgs = arguments(request)?;
                Ok(api
                    .delete_view(parse_id(&a.view_id)?)
                    .await
                    .map(|()| json!({ "deleted": true })))
            };
            run.await
        }
        "set_view_favorite" => {
            let run = async {
                let a: FavoriteArgs = arguments(request)?;
                Ok(api
                    .set_view_favorite(parse_id(&a.view_id)?, a.favorite)
                    .await
                    .map(|()| json!({ "favorite": a.favorite })))
            };
            run.await
        }
        "reorder_view_favorites" => {
            let run = async {
                let a: ReorderArgs = arguments(request)?;
                Ok(api
                    .reorder_view_favorites(parse_ids(&a.view_ids)?)
                    .await
                    .map(|()| json!({ "reordered": true })))
            };
            run.await
        }
        _ => return None,
    };
    Some(result)
}

fn visibility(raw: &str) -> Result<Visibility, ErrorData> {
    match raw {
        "personal" => Ok(Visibility::Personal),
        "workspace" => Ok(Visibility::Workspace),
        _ => Err(ErrorData::invalid_params(
            "visibility must be personal or workspace",
            None,
        )),
    }
}

/// A state where `filter` and `display` may be left out: an empty filter (every task) and the
/// web app's default display.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StateArg {
    #[serde(default)]
    filter: FilterGroup,
    #[serde(default)]
    display: DisplayOptions,
}

fn view_state(raw: Value) -> Result<ViewState, ErrorData> {
    serde_json::from_value::<StateArg>(raw)
        .map(|state| ViewState {
            filter: state.filter,
            display: state.display,
        })
        .map_err(|error| {
            ErrorData::invalid_params(
                format!("Invalid state ({error}); copy the shape of a view from list_views"),
                None,
            )
        })
}
