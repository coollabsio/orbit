use std::sync::Arc;

use axum::extract::{DefaultBodyLimit, Extension, Path, Query, State, WebSocketUpgrade};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use orbit_platform::{Id, RequestId, TimestampMillis};
use serde::{Deserialize, Deserializer};
use serde_json::Value;
use utoipa::{IntoParams, ToSchema};

use crate::auth_routes::CookieMode;
use crate::collab::socket::CollabQuery;
use crate::repositories::identity::IdentityRepository;
use crate::repositories::page_files::parse_page_file_url;
use crate::repositories::page_versions::{
    DEFAULT_VERSION_LIMIT, MAX_VERSION_LIMIT, PageVersion, PageVersionList,
};
use crate::repositories::pages::{
    CreatePage, PageChanges, PageError, PageFavorite, PageFavoriteList, PageList, PageRecord,
    PageRepository, PageSearch, PageTrash, PageTrashEmptied, SpaceRequest,
};
use crate::task_routes::{
    ApiError, ApiJson, ApiQuery, MutationQuery, RestoreBody, authenticate_session, bounded,
    deserialize_source_patch, http_url, request_id_value, validation,
};

/// Page requests carry a whole BlockNote document; cap them at 1 MiB.
const PAGE_BODY_LIMIT: usize = 1024 * 1024;

#[derive(Clone)]
pub struct PageState {
    identity: Arc<IdentityRepository>,
    pages: Arc<PageRepository>,
    cookie_mode: CookieMode,
}

impl PageState {
    #[must_use]
    pub fn new(identity: Arc<IdentityRepository>, cookie_mode: CookieMode) -> Self {
        Self {
            pages: Arc::new(PageRepository::new(identity.database().clone())),
            identity,
            cookie_mode,
        }
    }
}

pub fn page_router(state: PageState) -> Router {
    Router::new()
        .route(
            "/api/v1/workspaces/{workspace_id}/pages",
            get(list_pages).post(create_page),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/pages/trash",
            get(list_page_trash),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/pages/trash/empty",
            post(empty_page_trash),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/pages/search",
            get(search_pages),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/pages/favorites",
            get(list_page_favorites),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/pages/favorites/{page_id}/move",
            post(move_page_favorite),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/pages/{page_id}",
            get(get_page).patch(update_page).delete(delete_page),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/pages/{page_id}/move",
            post(move_page),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/pages/{page_id}/restore",
            post(restore_page),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/pages/{page_id}/permanent",
            delete(purge_page),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/pages/{page_id}/duplicate",
            post(duplicate_page),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/pages/{page_id}/favorite",
            put(add_page_favorite).delete(remove_page_favorite),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/pages/{page_id}/versions",
            get(list_page_versions),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/pages/{page_id}/collab",
            get(collab_socket),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/pages/{page_id}/versions/{version_id}",
            get(get_page_version),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/pages/{page_id}/versions/{version_id}/restore",
            post(restore_page_version),
        )
        .layer(DefaultBodyLimit::max(PAGE_BODY_LIMIT))
        .with_state(state)
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct CreatePageBody {
    /// With a parent the page joins the parent's space; contradicting `teamspace_id` or
    /// `private` values fail validation.
    parent_id: Option<String>,
    /// Root pages only: the target teamspace. Omitted (and not `private`) uses the default
    /// teamspace.
    teamspace_id: Option<String>,
    /// Root pages only: `true` creates the page in the caller's private space.
    private: Option<bool>,
    /// Defaults to an empty title; clients show "Untitled".
    #[serde(default)]
    title: String,
    icon: Option<String>,
    /// Index among the new siblings; omitted appends the page last.
    position: Option<i64>,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct PageUpdateBody {
    expected_version: u64,
    title: Option<String>,
    /// Absent: unchanged. `null`: cleared.
    #[serde(default, deserialize_with = "deserialize_source_patch")]
    icon: Option<Option<String>>,
    /// Absent: unchanged. `null`: cleared. Otherwise an http(s) URL, or the `url` of a file
    /// uploaded to this same page (`/api/v1/workspaces/{workspace_id}/pages/{page_id}/files/{file_id}`).
    #[serde(default, deserialize_with = "deserialize_source_patch")]
    cover_url: Option<Option<String>>,
    /// Absent: unchanged. `null`: cleared. Otherwise `"x,y"` percentages from 0 to 100.
    #[serde(default, deserialize_with = "deserialize_source_patch")]
    cover_position: Option<Option<String>>,
    /// The whole BlockNote block array.
    #[schema(value_type = Option<Vec<serde_json::Value>>)]
    content: Option<Value>,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct MovePageBody {
    expected_version: u64,
    /// The new parent; `null` moves the page to a space root. The page and its whole subtree
    /// take the parent's space.
    #[serde(deserialize_with = "required_nullable")]
    #[schema(required = true)]
    parent_id: Option<String>,
    /// With `parent_id: null`: the target teamspace. Omitted keeps the page's current space.
    teamspace_id: Option<String>,
    /// With `parent_id: null`: `true` moves the page into the caller's private space, `false`
    /// out of it (to the default teamspace unless `teamspace_id` is given).
    private: Option<bool>,
    /// Index among the new siblings.
    position: i64,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct MoveFavoriteBody {
    /// Index among the caller's visible favorites; larger values move it last.
    position: i64,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct DuplicatePageBody {
    /// Also copy the page's live sub-pages (the whole subtree, in order). Defaults to `false`.
    #[serde(default)]
    include_children: bool,
}

#[derive(Deserialize, IntoParams, ToSchema)]
#[into_params(parameter_in = Query)]
#[serde(deny_unknown_fields)]
struct PageSearchQuery {
    /// Words matched against titles and body text, ignoring case and diacritics; every word must
    /// match, the last one as a prefix. Search syntax (quotes, `*`, `:`, `-`, `NEAR`, parentheses)
    /// is treated as plain text. At most 200 characters.
    q: String,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
#[serde(deny_unknown_fields)]
struct PageVersionQuery {
    /// `next_cursor` from the previous response, for older versions.
    cursor: Option<String>,
    /// Versions per response, 1 to 100 (default 50).
    limit: Option<usize>,
}

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/pages", params(("workspace_id" = String, Path)), responses((status = 200, body = PageList)))]
async fn list_pages(
    State(state): State<PageState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<PageList>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/pages");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    state
        .pages
        .pages(workspace_id, actor_id)
        .await
        .map(Json)
        .map_err(|error| page_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/pages", params(("workspace_id" = String, Path)), request_body = CreatePageBody, responses((status = 201, body = PageRecord)))]
async fn create_page(
    State(state): State<PageState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<CreatePageBody>,
) -> Result<Response, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/pages");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let input = CreatePage {
        parent_id: body
            .parent_id
            .map(|value| parse_id(&value, &instance, request_id.as_ref()))
            .transpose()?,
        space: space_request(
            body.teamspace_id,
            body.private,
            &instance,
            request_id.as_ref(),
        )?,
        title: title(body.title, &instance, request_id.as_ref())?,
        icon: body
            .icon
            .map(|value| icon(value, &instance, request_id.as_ref()))
            .transpose()?,
        position: body
            .position
            .map(|value| position(value, &instance, request_id.as_ref()))
            .transpose()?,
    };
    state
        .pages
        .create_page(
            workspace_id,
            actor_id,
            input,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map(|record| (StatusCode::CREATED, Json(record)).into_response())
        .map_err(|error| page_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/pages/{page_id}", params(("workspace_id" = String, Path), ("page_id" = String, Path)), responses((status = 200, body = PageRecord)))]
async fn get_page(
    State(state): State<PageState>,
    Path((workspace, page)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<PageRecord>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/pages/{page}");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let page_id = parse_id(&page, &instance, request_id.as_ref())?;
    state
        .pages
        .get_page(workspace_id, page_id, actor_id)
        .await
        .map(Json)
        .map_err(|error| page_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(patch, path = "/api/v1/workspaces/{workspace_id}/pages/{page_id}", params(("workspace_id" = String, Path), ("page_id" = String, Path)), request_body = PageUpdateBody, responses((status = 200, body = PageRecord)))]
async fn update_page(
    State(state): State<PageState>,
    Path((workspace, page)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<PageUpdateBody>,
) -> Result<Json<PageRecord>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/pages/{page}");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let page_id = parse_id(&page, &instance, request_id.as_ref())?;
    let request = request_id.as_ref();
    let changes = PageChanges {
        title: body
            .title
            .map(|value| title(value, &instance, request))
            .transpose()?,
        icon: body
            .icon
            .map(|value| {
                value
                    .map(|value| icon(value, &instance, request))
                    .transpose()
            })
            .transpose()?,
        cover_url: body
            .cover_url
            .map(|value| {
                value
                    .map(|value| cover_url(value, workspace_id, page_id, &instance, request))
                    .transpose()
            })
            .transpose()?,
        cover_position: body
            .cover_position
            .map(|value| {
                value
                    .map(|value| cover_position(value, &instance, request))
                    .transpose()
            })
            .transpose()?,
        content: body
            .content
            .map(|value| match value {
                Value::Array(blocks) => Ok(blocks),
                _ => Err(validation("content", &instance, request)),
            })
            .transpose()?,
        ..PageChanges::default()
    };
    state
        .pages
        .update_page(
            workspace_id,
            page_id,
            actor_id,
            body.expected_version,
            changes,
            request_id_value(request),
            TimestampMillis::now(),
        )
        .await
        .map(Json)
        .map_err(|error| page_problem(error, instance, request))
}

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/pages/{page_id}/move", params(("workspace_id" = String, Path), ("page_id" = String, Path)), request_body = MovePageBody, responses((status = 200, body = PageRecord)))]
async fn move_page(
    State(state): State<PageState>,
    Path((workspace, page)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<MovePageBody>,
) -> Result<Json<PageRecord>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/pages/{page}/move");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let page_id = parse_id(&page, &instance, request_id.as_ref())?;
    let parent_id = body
        .parent_id
        .map(|value| parse_id(&value, &instance, request_id.as_ref()))
        .transpose()?;
    let space = space_request(
        body.teamspace_id,
        body.private,
        &instance,
        request_id.as_ref(),
    )?;
    let position = position(body.position, &instance, request_id.as_ref())?;
    state
        .pages
        .move_page(
            workspace_id,
            page_id,
            actor_id,
            body.expected_version,
            parent_id,
            space,
            position,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map(Json)
        .map_err(|error| page_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(delete, path = "/api/v1/workspaces/{workspace_id}/pages/{page_id}", params(MutationQuery, ("workspace_id" = String, Path), ("page_id" = String, Path)), responses((status = 204)))]
async fn delete_page(
    State(state): State<PageState>,
    Path((workspace, page)): Path<(String, String)>,
    ApiQuery(query): ApiQuery<MutationQuery>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<StatusCode, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/pages/{page}");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let page_id = parse_id(&page, &instance, request_id.as_ref())?;
    state
        .pages
        .delete_page(
            workspace_id,
            page_id,
            actor_id,
            query.expected_version,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map(|()| StatusCode::NO_CONTENT)
        .map_err(|error| page_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/pages/{page_id}/restore", params(("workspace_id" = String, Path), ("page_id" = String, Path)), request_body = RestoreBody, responses((status = 200, body = PageRecord)))]
async fn restore_page(
    State(state): State<PageState>,
    Path((workspace, page)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<RestoreBody>,
) -> Result<Json<PageRecord>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/pages/{page}/restore");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let page_id = parse_id(&page, &instance, request_id.as_ref())?;
    state
        .pages
        .restore_page(
            workspace_id,
            page_id,
            actor_id,
            body.expected_version,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map(Json)
        .map_err(|error| page_problem(error, instance, request_id.as_ref()))
}

/// Deletes a page in the trash forever, with the sub-pages trashed along with it. Your own private
/// pages, or teamspace pages as a workspace owner or admin (members get 403).
#[utoipa::path(delete, path = "/api/v1/workspaces/{workspace_id}/pages/{page_id}/permanent", params(MutationQuery, ("workspace_id" = String, Path), ("page_id" = String, Path)), responses((status = 204)))]
async fn purge_page(
    State(state): State<PageState>,
    Path((workspace, page)): Path<(String, String)>,
    ApiQuery(query): ApiQuery<MutationQuery>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<StatusCode, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/pages/{page}/permanent");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let page_id = parse_id(&page, &instance, request_id.as_ref())?;
    state
        .pages
        .purge_page(
            workspace_id,
            page_id,
            actor_id,
            query.expected_version,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map(|()| StatusCode::NO_CONTENT)
        .map_err(|error| page_problem(error, instance, request_id.as_ref()))
}

/// Deletes forever every trashed page the caller may purge: their own private pages, plus teamspace
/// pages for workspace owners and admins.
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/pages/trash/empty", params(("workspace_id" = String, Path)), responses((status = 200, body = PageTrashEmptied)))]
async fn empty_page_trash(
    State(state): State<PageState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<PageTrashEmptied>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/pages/trash/empty");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    state
        .pages
        .empty_trash(
            workspace_id,
            actor_id,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map(Json)
        .map_err(|error| page_problem(error, instance, request_id.as_ref()))
}

/// Copies a page ("<title> (copy)", right after the original), optionally with its sub-pages.
/// Files are shared with the copies and links inside the copied pages point at the copies.
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/pages/{page_id}/duplicate", params(("workspace_id" = String, Path), ("page_id" = String, Path)), request_body = DuplicatePageBody, responses((status = 201, body = PageRecord)))]
async fn duplicate_page(
    State(state): State<PageState>,
    Path((workspace, page)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<DuplicatePageBody>,
) -> Result<Response, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/pages/{page}/duplicate");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let page_id = parse_id(&page, &instance, request_id.as_ref())?;
    state
        .pages
        .duplicate_page(
            workspace_id,
            page_id,
            actor_id,
            body.include_children,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map(|record| (StatusCode::CREATED, Json(record)).into_response())
        .map_err(|error| page_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/pages/trash", params(("workspace_id" = String, Path)), responses((status = 200, body = PageTrash)))]
async fn list_page_trash(
    State(state): State<PageState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<PageTrash>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/pages/trash");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    state
        .pages
        .page_trash(workspace_id, actor_id, TimestampMillis::now())
        .await
        .map(Json)
        .map_err(|error| page_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/pages/search", params(PageSearchQuery, ("workspace_id" = String, Path)), responses((status = 200, body = PageSearch)))]
async fn search_pages(
    State(state): State<PageState>,
    Path(workspace): Path<String>,
    ApiQuery(query): ApiQuery<PageSearchQuery>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<PageSearch>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/pages/search");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let q = bounded(query.q, 200, 800, "q", &instance, request_id.as_ref())?;
    state
        .pages
        .search_pages(workspace_id, actor_id, &q)
        .await
        .map(Json)
        .map_err(|error| page_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/pages/favorites", params(("workspace_id" = String, Path)), responses((status = 200, body = PageFavoriteList)))]
async fn list_page_favorites(
    State(state): State<PageState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<PageFavoriteList>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/pages/favorites");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    state
        .pages
        .favorites(workspace_id, actor_id)
        .await
        .map(Json)
        .map_err(|error| page_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(put, path = "/api/v1/workspaces/{workspace_id}/pages/{page_id}/favorite", params(("workspace_id" = String, Path), ("page_id" = String, Path)), responses((status = 200, body = PageFavorite)))]
async fn add_page_favorite(
    State(state): State<PageState>,
    Path((workspace, page)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<PageFavorite>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/pages/{page}/favorite");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let page_id = parse_id(&page, &instance, request_id.as_ref())?;
    state
        .pages
        .add_favorite(workspace_id, page_id, actor_id, TimestampMillis::now())
        .await
        .map(Json)
        .map_err(|error| page_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(delete, path = "/api/v1/workspaces/{workspace_id}/pages/{page_id}/favorite", params(("workspace_id" = String, Path), ("page_id" = String, Path)), responses((status = 204)))]
async fn remove_page_favorite(
    State(state): State<PageState>,
    Path((workspace, page)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<StatusCode, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/pages/{page}/favorite");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let page_id = parse_id(&page, &instance, request_id.as_ref())?;
    state
        .pages
        .remove_favorite(workspace_id, page_id, actor_id)
        .await
        .map(|()| StatusCode::NO_CONTENT)
        .map_err(|error| page_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/pages/favorites/{page_id}/move", params(("workspace_id" = String, Path), ("page_id" = String, Path)), request_body = MoveFavoriteBody, responses((status = 200, body = PageFavoriteList)))]
async fn move_page_favorite(
    State(state): State<PageState>,
    Path((workspace, page)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<MoveFavoriteBody>,
) -> Result<Json<PageFavoriteList>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/pages/favorites/{page}/move");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let page_id = parse_id(&page, &instance, request_id.as_ref())?;
    let position = position(body.position, &instance, request_id.as_ref())?;
    state
        .pages
        .move_favorite(workspace_id, page_id, actor_id, position)
        .await
        .map(Json)
        .map_err(|error| page_problem(error, instance, request_id.as_ref()))
}

/// The page's history, newest first (without content). Visible like the page itself.
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/pages/{page_id}/versions", params(PageVersionQuery, ("workspace_id" = String, Path), ("page_id" = String, Path)), responses((status = 200, body = PageVersionList)))]
async fn list_page_versions(
    State(state): State<PageState>,
    Path((workspace, page)): Path<(String, String)>,
    ApiQuery(query): ApiQuery<PageVersionQuery>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<PageVersionList>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/pages/{page}/versions");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let page_id = parse_id(&page, &instance, request_id.as_ref())?;
    let limit = query.limit.unwrap_or(DEFAULT_VERSION_LIMIT);
    if !(1..=MAX_VERSION_LIMIT).contains(&limit) {
        return Err(validation("limit", &instance, request_id.as_ref()));
    }
    state
        .pages
        .page_versions(
            workspace_id,
            page_id,
            actor_id,
            query.cursor.as_deref(),
            limit,
        )
        .await
        .map(Json)
        .map_err(|error| page_problem(error, instance, request_id.as_ref()))
}

/// One version of the page, with its content.
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/pages/{page_id}/versions/{version_id}", params(("workspace_id" = String, Path), ("page_id" = String, Path), ("version_id" = String, Path)), responses((status = 200, body = PageVersion)))]
async fn get_page_version(
    State(state): State<PageState>,
    Path((workspace, page, version)): Path<(String, String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<PageVersion>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/pages/{page}/versions/{version}");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let page_id = parse_id(&page, &instance, request_id.as_ref())?;
    let version_id = parse_version_id(&version, &instance, request_id.as_ref())?;
    state
        .pages
        .page_version(workspace_id, page_id, version_id, actor_id)
        .await
        .map(Json)
        .map_err(|error| page_problem(error, instance, request_id.as_ref()))
}

/// Puts a version's title, icon and content back on the page (cover and place stay). The current
/// state is kept as a `restore` version first. Audited `page.restored_version`.
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/pages/{page_id}/versions/{version_id}/restore", params(("workspace_id" = String, Path), ("page_id" = String, Path), ("version_id" = String, Path)), request_body = RestoreBody, responses((status = 200, body = PageRecord)))]
async fn restore_page_version(
    State(state): State<PageState>,
    Path((workspace, page, version)): Path<(String, String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<RestoreBody>,
) -> Result<Json<PageRecord>, ApiError> {
    let instance =
        format!("/api/v1/workspaces/{workspace}/pages/{page}/versions/{version}/restore");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let page_id = parse_id(&page, &instance, request_id.as_ref())?;
    let version_id = parse_version_id(&version, &instance, request_id.as_ref())?;
    state
        .pages
        .restore_page_version(
            workspace_id,
            page_id,
            version_id,
            actor_id,
            body.expected_version,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map(Json)
        .map_err(|error| page_problem(error, instance, request_id.as_ref()))
}

/// The page's real-time co-editing socket (y-sync over WebSocket, see `crate::collab::socket`).
async fn collab_socket(
    State(state): State<PageState>,
    Path((workspace, page)): Path<(String, String)>,
    Query(query): Query<CollabQuery>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    crate::collab::socket::upgrade(
        &state.identity,
        state.cookie_mode,
        state.identity.database(),
        &workspace,
        &page,
        query,
        &headers,
        upgrade,
    )
    .await
}

async fn scope(
    state: &PageState,
    headers: &HeaderMap,
    workspace: &str,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<(Id, Id), ApiError> {
    let session = authenticate_session(
        &state.identity,
        state.cookie_mode,
        headers,
        instance,
        request_id,
    )
    .await?;
    Ok((parse_id(workspace, instance, request_id)?, session.user.id))
}

fn parse_id(
    value: &str,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<Id, ApiError> {
    value
        .parse()
        .map_err(|_| not_found(instance.to_owned(), request_id))
}

/// An unparseable version id names no version of any page. The answer does not depend on the
/// page, so it reveals nothing about hidden pages (valid ids are checked page first).
fn parse_version_id(
    value: &str,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<Id, ApiError> {
    value
        .parse()
        .map_err(|_| version_not_found(instance.to_owned(), request_id))
}

/// An unparseable teamspace id cannot name a teamspace of this workspace.
fn space_request(
    teamspace_id: Option<String>,
    private: Option<bool>,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<SpaceRequest, ApiError> {
    Ok(SpaceRequest {
        teamspace_id: teamspace_id
            .map(|value| {
                value
                    .parse()
                    .map_err(|_| teamspace_not_found(instance.to_owned(), request_id))
            })
            .transpose()?,
        private,
    })
}

fn title(
    value: String,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<String, ApiError> {
    bounded(value, 500, 2000, "title", instance, request_id)
}

fn icon(
    value: String,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<String, ApiError> {
    bounded(value, 64, 256, "icon", instance, request_id)
}

fn position(
    value: i64,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<i64, ApiError> {
    if value < 0 {
        Err(validation("position", instance, request_id))
    } else {
        Ok(value)
    }
}

/// An http(s) URL, or the canonical download path of a file of this page (its existence is
/// checked by the repository).
fn cover_url(
    value: String,
    workspace_id: Id,
    page_id: Id,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<String, ApiError> {
    if !value.starts_with('/') {
        return http_url(value, "cover_url", instance, request_id);
    }
    match parse_page_file_url(&value) {
        Some((workspace, page, _)) if workspace == workspace_id && page == page_id => Ok(value),
        _ => Err(validation("cover_url", instance, request_id)),
    }
}

/// `"x,y"` where both are percentages from 0 to 100, e.g. `"50,30"` or `"12.5,100"`.
fn cover_position(
    value: String,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<String, ApiError> {
    let percent = |part: &str| {
        !part.is_empty()
            && part.len() <= 8
            && part
                .bytes()
                .all(|byte| byte.is_ascii_digit() || byte == b'.')
            && part.bytes().filter(|&byte| byte == b'.').count() <= 1
            && !part.starts_with('.')
            && !part.ends_with('.')
            && part
                .parse::<f64>()
                .is_ok_and(|number| (0.0..=100.0).contains(&number))
    };
    match value.split_once(',') {
        Some((x, y)) if percent(x) && percent(y) => Ok(value),
        _ => Err(validation("cover_position", instance, request_id)),
    }
}

fn required_nullable<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer)
}

fn not_found(instance: String, request_id: Option<&Extension<RequestId>>) -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        "page_not_found",
        "Page not found",
        "The requested page was not found.",
        instance,
        request_id,
    )
}

fn version_not_found(instance: String, request_id: Option<&Extension<RequestId>>) -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        "page_version_not_found",
        "Page version not found",
        "The requested page version was not found.",
        instance,
        request_id,
    )
}

fn teamspace_not_found(instance: String, request_id: Option<&Extension<RequestId>>) -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        "teamspace_not_found",
        "Teamspace not found",
        "The requested teamspace was not found.",
        instance,
        request_id,
    )
}

fn page_problem(
    error: PageError,
    instance: String,
    request_id: Option<&Extension<RequestId>>,
) -> ApiError {
    match error {
        PageError::NotFound => not_found(instance, request_id),
        PageError::VersionNotFound => version_not_found(instance, request_id),
        PageError::InvalidCursor => ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_cursor",
            "Invalid cursor",
            "The cursor does not match this collection query.",
            instance,
            request_id,
        ),
        PageError::TeamspaceNotFound => teamspace_not_found(instance, request_id),
        PageError::Invalid { field } => validation(field, &instance, request_id),
        PageError::VersionConflict { current } => {
            ApiError::version_conflict(*current, instance, request_id)
        }
        PageError::Forbidden => ApiError::new(
            StatusCode::FORBIDDEN,
            "workspace_action_forbidden",
            "Workspace action forbidden",
            "Only workspace owners and administrators may delete teamspace pages forever.",
            instance,
            request_id,
        ),
        PageError::NotTrashed => ApiError::new(
            StatusCode::CONFLICT,
            "page_not_trashed",
            "Page not in the trash",
            "Only pages in the trash can be deleted forever.",
            instance,
            request_id,
        ),
        PageError::Corrupt | PageError::Unavailable(_) => ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            "Internal server error",
            "An unexpected error occurred. Use the request ID when contacting support.",
            instance,
            request_id,
        ),
    }
}
