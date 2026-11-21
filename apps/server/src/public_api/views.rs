//! Saved task views and the token user's view favorites. Personal tokens only: a
//! service-account token gets 403. A view whose filter names a project outside the grant is
//! hidden (404), and a write cannot name such a project (400).
use axum::Json;
use axum::Router;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, put};
use orbit_platform::{Id, TimestampMillis};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::{Ctx, List, PublicApiState, present, problem_response};
use crate::repositories::api_tokens::ApiTokenScope;
use crate::repositories::task_filter::ViewState;
use crate::repositories::views::{SavedViewRecord, ViewCreate, ViewUpdate, Visibility};
use crate::token_api::{MemberView, TokenApiError};

pub(super) fn routes() -> Router<PublicApiState> {
    Router::new()
        .route("/views", get(views).post(create_view))
        .route(
            "/views/{view_id}",
            get(get_view).patch(update_view).delete(delete_view),
        )
        .route(
            "/views/{view_id}/favorite",
            put(favorite_view).delete(unfavorite_view),
        )
        .route("/view-favorites/order", put(reorder_view_favorites))
}

/// A saved view: a named task filter with display options.
#[derive(Serialize, ToSchema)]
#[schema(as = PublicView)]
pub(super) struct ViewOut {
    #[schema(value_type = String)]
    id: Id,
    owner: MemberView,
    name: String,
    description: String,
    /// An icon name from the web app's icon set.
    #[schema(required = true)]
    icon: Option<String>,
    /// `#rrggbb`.
    #[schema(required = true)]
    color: Option<String>,
    /// `personal` (only its owner sees it) or `workspace` (every member does).
    visibility: Visibility,
    /// The filter and display options; `null` when the stored state no longer parses
    /// (`state_error` says why).
    #[schema(required = true)]
    state: Option<ViewState>,
    #[schema(required = true)]
    state_error: Option<String>,
    /// Send it back as `expected_version`.
    version: i64,
    /// Whether the token user has favorited the view.
    is_favorite: bool,
    /// The view's place in the token user's favorites; `null` when not a favorite.
    #[schema(required = true)]
    favorite_position: Option<i64>,
    /// Whether the token user may change or delete the view.
    can_edit: bool,
    /// Whether the token user may change its visibility (the owner only).
    can_change_visibility: bool,
    #[schema(value_type = String, format = DateTime)]
    created_at: TimestampMillis,
    #[schema(value_type = String, format = DateTime)]
    updated_at: TimestampMillis,
}

impl From<SavedViewRecord> for ViewOut {
    fn from(view: SavedViewRecord) -> Self {
        Self {
            id: view.id,
            owner: MemberView {
                user_id: view.owner.user_id,
                display_name: view.owner.display_name,
            },
            name: view.name,
            description: view.description,
            icon: view.icon,
            color: view.color,
            visibility: view.visibility,
            state: view.state,
            state_error: view.state_error,
            version: view.version,
            is_favorite: view.is_favorite,
            favorite_position: view.favorite_position,
            can_edit: view.can_edit,
            can_change_visibility: view.can_change_visibility,
            created_at: view.created_at,
            updated_at: view.updated_at,
        }
    }
}

#[derive(Deserialize, ToSchema)]
#[schema(as = PublicCreateViewBody)]
#[serde(deny_unknown_fields)]
pub(super) struct CreateViewBody {
    /// 1 to 80 characters after trimming.
    name: String,
    /// Up to 500 characters.
    #[serde(default)]
    #[schema(required = false)]
    description: String,
    /// An icon name: ASCII letters, digits, `-` or `_`, up to 64 bytes.
    icon: Option<String>,
    /// `#rrggbb`.
    color: Option<String>,
    visibility: Visibility,
    /// The filter and display options, as `GET /views` shows them. `project` conditions may
    /// name approved projects only.
    state: ViewState,
}

/// A missing field keeps its value.
#[derive(Deserialize, ToSchema)]
#[schema(as = PublicUpdateViewBody)]
#[serde(deny_unknown_fields)]
pub(super) struct UpdateViewBody {
    /// The `version` that you read.
    expected_version: i64,
    name: Option<String>,
    description: Option<String>,
    /// `null` clears it.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = Option<String>)]
    icon: Option<Option<String>>,
    /// `null` clears it.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = Option<String>)]
    color: Option<Option<String>>,
    /// Only the view's owner may change it.
    visibility: Option<Visibility>,
    state: Option<ViewState>,
}

#[derive(Deserialize, ToSchema)]
#[schema(as = PublicViewFavoritesOrderBody)]
#[serde(deny_unknown_fields)]
pub(super) struct FavoritesOrderBody {
    /// Every view of `GET /views` with `is_favorite`, each once, in the new order.
    #[schema(value_type = Vec<String>)]
    view_ids: Vec<Id>,
}

fn no_content(ctx: &Ctx, result: Result<(), TokenApiError>) -> Response {
    match result {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => ctx.error(error),
    }
}

#[utoipa::path(
    get,
    path = "/api/public/v1/workspaces/{workspace_id}/views",
    operation_id = "public_list_views",
    tag = "public-api",
    summary = "The saved views that the token user sees, by name",
    description = "The user's personal views and every workspace view, without those whose filter names a project outside the grant. Not for service-account tokens (`403`).",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`.")),
    responses((status = 200, description = "Every view; `next_cursor` is `null`", body = List<ViewOut>)),
    security(("bearerAuth" = []))
)]
pub(super) async fn views(State(s): State<PublicApiState>, ctx: Ctx) -> Response {
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::TasksRead, &ctx.request_id)
            .await?;
        let views = api.saved_views().await?;
        Ok(super::list::<_, ViewOut>(views, None))
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    post,
    path = "/api/public/v1/workspaces/{workspace_id}/views",
    operation_id = "public_create_view",
    tag = "public-api",
    summary = "Save a view",
    description = "Not for service-account tokens (`403`).",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`.")),
    request_body = CreateViewBody,
    responses((status = 201, description = "The new view", body = ViewOut)),
    security(("bearerAuth" = []))
)]
pub(super) async fn create_view(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    body: Result<Json<CreateViewBody>, JsonRejection>,
) -> Response {
    let body = input!(ctx, ApiTokenScope::TasksWrite, body);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::TasksWrite, &ctx.request_id)
            .await?;
        api.create_view(ViewCreate {
            name: body.name,
            description: body.description,
            icon: body.icon,
            color: body.color,
            visibility: body.visibility,
            state: body.state,
        })
        .await
        .map(ViewOut::from)
    }
    .await;
    ctx.reply(StatusCode::CREATED, result)
}

#[utoipa::path(
    get,
    path = "/api/public/v1/workspaces/{workspace_id}/views/{view_id}",
    operation_id = "public_get_view",
    tag = "public-api",
    summary = "A saved view",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("view_id" = String, Path)),
    responses((status = 200, description = "The view", body = ViewOut)),
    security(("bearerAuth" = []))
)]
pub(super) async fn get_view(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, view)): Path<(String, String)>,
) -> Response {
    let view = path_id!(ctx, view);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::TasksRead, &ctx.request_id)
            .await?;
        api.saved_view(view).await.map(ViewOut::from)
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    patch,
    path = "/api/public/v1/workspaces/{workspace_id}/views/{view_id}",
    operation_id = "public_update_view",
    tag = "public-api",
    summary = "Change a saved view",
    description = "The token user must own the view, or be a workspace owner or admin for a workspace view (`403` otherwise); only the owner may change `visibility`.",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("view_id" = String, Path)),
    request_body = UpdateViewBody,
    responses((status = 200, description = "The view", body = ViewOut)),
    security(("bearerAuth" = []))
)]
pub(super) async fn update_view(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, view)): Path<(String, String)>,
    body: Result<Json<UpdateViewBody>, JsonRejection>,
) -> Response {
    let view = path_id!(ctx, view);
    let body = input!(ctx, ApiTokenScope::TasksWrite, body);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::TasksWrite, &ctx.request_id)
            .await?;
        let changes = ViewUpdate {
            expected_version: body.expected_version,
            name: body.name,
            description: body.description,
            icon: body.icon,
            color: body.color,
            visibility: body.visibility,
            state: body.state,
        };
        api.update_view(view, changes).await.map(ViewOut::from)
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    delete,
    path = "/api/public/v1/workspaces/{workspace_id}/views/{view_id}",
    operation_id = "public_delete_view",
    tag = "public-api",
    summary = "Delete a saved view",
    description = "Takes it out of every member's favorites. Same edit rule as `PATCH`.",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("view_id" = String, Path)),
    responses((status = 204, description = "The view is deleted")),
    security(("bearerAuth" = []))
)]
pub(super) async fn delete_view(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, view)): Path<(String, String)>,
) -> Response {
    let view = path_id!(ctx, view);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::TasksWrite, &ctx.request_id)
            .await?;
        api.delete_view(view).await
    }
    .await;
    no_content(&ctx, result)
}

#[utoipa::path(
    put,
    path = "/api/public/v1/workspaces/{workspace_id}/views/{view_id}/favorite",
    operation_id = "public_favorite_view",
    tag = "public-api",
    summary = "Add a view to the end of the token user's favorites",
    description = "A favorite stays one.",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("view_id" = String, Path)),
    responses((status = 204, description = "The view is a favorite")),
    security(("bearerAuth" = []))
)]
pub(super) async fn favorite_view(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, view)): Path<(String, String)>,
) -> Response {
    set_favorite(s, ctx, view, true).await
}

#[utoipa::path(
    delete,
    path = "/api/public/v1/workspaces/{workspace_id}/views/{view_id}/favorite",
    operation_id = "public_unfavorite_view",
    tag = "public-api",
    summary = "Take a view out of the token user's favorites",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("view_id" = String, Path)),
    responses((status = 204, description = "The view is not a favorite")),
    security(("bearerAuth" = []))
)]
pub(super) async fn unfavorite_view(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, view)): Path<(String, String)>,
) -> Response {
    set_favorite(s, ctx, view, false).await
}

async fn set_favorite(s: PublicApiState, ctx: Ctx, view: String, favorite: bool) -> Response {
    let view = path_id!(ctx, view);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::TasksWrite, &ctx.request_id)
            .await?;
        api.set_view_favorite(view, favorite).await
    }
    .await;
    no_content(&ctx, result)
}

#[utoipa::path(
    put,
    path = "/api/public/v1/workspaces/{workspace_id}/view-favorites/order",
    operation_id = "public_reorder_view_favorites",
    tag = "public-api",
    summary = "Reorder the token user's favorite views",
    description = "`view_ids` lists every favorite of `GET /views`, each once; favorites hidden from the token keep their places.",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`.")),
    request_body = FavoritesOrderBody,
    responses((status = 204, description = "The favorites are in the new order")),
    security(("bearerAuth" = []))
)]
pub(super) async fn reorder_view_favorites(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    body: Result<Json<FavoritesOrderBody>, JsonRejection>,
) -> Response {
    let body = input!(ctx, ApiTokenScope::TasksWrite, body);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::TasksWrite, &ctx.request_id)
            .await?;
        api.reorder_view_favorites(body.view_ids).await
    }
    .await;
    no_content(&ctx, result)
}
