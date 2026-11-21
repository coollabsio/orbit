//! Doc search, recents, favorites, copies, locks, version restores, permanent deletes, teamspace
//! writes and doc comment thread edits. Every call acts as the token's creator, so a
//! service-account token gets 404.
use axum::Json;
use axum::Router;
use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post, put};
use orbit_platform::Id;
use serde::Deserialize;
use utoipa::{IntoParams, ToSchema};

use super::{Ctx, ExpectedVersionQuery, List, PublicApiState, present, problem_response};
use crate::repositories::api_tokens::ApiTokenScope;
use crate::repositories::pages::{
    PageFavorite, PageSearchResult, PageSummary, PageTrashEmptied, RecentPage,
};
use crate::token_api::{
    DocComment, DocThread, DocView, TeamspacePatch, TeamspaceView, TokenApiError,
};

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
#[serde(deny_unknown_fields)]
pub(super) struct SearchQuery {
    /// Words matched against titles, text and row values, ignoring case and diacritics: 1 to 200
    /// characters. Search syntax is plain text.
    q: String,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
#[serde(deny_unknown_fields)]
pub(super) struct RecentQuery {
    /// 1 to 50; 10 when unset.
    #[param(minimum = 1, maximum = 50)]
    limit: Option<usize>,
}

#[derive(Deserialize, ToSchema)]
#[schema(as = PublicMoveFavoriteDocBody)]
#[serde(deny_unknown_fields)]
pub(super) struct MoveFavoriteBody {
    /// Index among the favorites, from 0; past the last one moves it last.
    position: i64,
}

#[derive(Deserialize, ToSchema)]
#[schema(as = PublicDuplicateDocBody)]
#[serde(deny_unknown_fields)]
pub(super) struct DuplicateDocBody {
    /// Also copy the doc's live sub-docs. Default `false`.
    #[serde(default)]
    include_children: bool,
}

#[derive(Deserialize, ToSchema)]
#[schema(as = PublicLockDocBody)]
#[serde(deny_unknown_fields)]
pub(super) struct LockDocBody {
    /// `true` locks the doc, `false` unlocks it.
    locked: bool,
}

#[derive(Deserialize, ToSchema)]
#[schema(as = PublicRestoreDocVersionBody)]
#[serde(deny_unknown_fields)]
pub(super) struct RestoreDocVersionBody {
    /// The doc's `version` that you read (not the version's id).
    expected_version: u64,
}

#[derive(Deserialize, ToSchema)]
#[schema(as = PublicCreateTeamspaceBody)]
#[serde(deny_unknown_fields)]
pub(super) struct CreateTeamspaceBody {
    /// 1 to 100 characters.
    name: String,
    /// An emoji, up to 64 characters.
    icon: Option<String>,
}

/// A missing field keeps its value.
#[derive(Deserialize, ToSchema)]
#[schema(as = PublicUpdateTeamspaceBody)]
#[serde(deny_unknown_fields)]
pub(super) struct UpdateTeamspaceBody {
    /// The `version` that you read in `GET /teamspaces`.
    expected_version: u64,
    /// 1 to 100 characters.
    name: Option<String>,
    /// An emoji, up to 64 characters; `null` clears it.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = Option<String>)]
    icon: Option<Option<String>>,
    /// The project whose docs the teamspace holds, approved for the token; `null` makes it a
    /// teamspace for global docs.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = Option<String>)]
    project_id: Option<Option<Id>>,
}

#[derive(Deserialize, ToSchema)]
#[schema(as = PublicMoveTeamspaceBody)]
#[serde(deny_unknown_fields)]
pub(super) struct MoveTeamspaceBody {
    /// The `version` that you read in `GET /teamspaces`.
    expected_version: u64,
    /// Index among the teamspaces, from 0 (the default teamspace); past the last one moves it last.
    position: i64,
}

#[derive(Deserialize, ToSchema)]
#[schema(as = PublicUpdateDocCommentBody)]
#[serde(deny_unknown_fields)]
pub(super) struct UpdateDocCommentBody {
    /// The new comment as Markdown, 1 to 100,000 bytes; `@Name` mentions a member.
    body: String,
}

fn no_content(ctx: &Ctx, result: Result<(), TokenApiError>) -> Response {
    match result {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => ctx.error(error),
    }
}

pub(super) fn routes() -> Router<PublicApiState> {
    Router::new()
        .route("/docs/search", get(search_docs))
        .route("/docs/recent", get(recent_docs))
        .route("/docs/favorites", get(favorite_docs))
        .route(
            "/docs/{page_id}/favorite",
            put(add_favorite_doc).delete(remove_favorite_doc),
        )
        .route("/docs/favorites/{page_id}/move", post(move_favorite_doc))
        .route("/docs/{page_id}/descendants", get(doc_descendants))
        .route("/docs/{page_id}/duplicate", post(duplicate_doc))
        .route("/docs/{page_id}/lock", post(lock_doc))
        .route(
            "/docs/{page_id}/versions/{version_id}/restore",
            post(restore_doc_version),
        )
        .route("/docs/{page_id}/permanent", delete(purge_doc))
        .route("/docs/trash/empty", post(empty_doc_trash))
        .route("/teamspaces", post(create_teamspace))
        .route(
            "/teamspaces/{teamspace_id}",
            patch(update_teamspace).delete(delete_teamspace),
        )
        .route("/teamspaces/{teamspace_id}/move", post(move_teamspace))
        .route(
            "/docs/{page_id}/threads/{thread_id}/comments/{comment_id}",
            patch(update_doc_comment).delete(delete_doc_comment),
        )
        .route(
            "/docs/{page_id}/threads/{thread_id}/resolve",
            post(resolve_doc_thread),
        )
        .route(
            "/docs/{page_id}/threads/{thread_id}/reopen",
            post(reopen_doc_thread),
        )
        .route(
            "/docs/{page_id}/threads/{thread_id}",
            delete(delete_doc_thread),
        )
}

#[utoipa::path(
    get,
    path = "/api/public/v1/workspaces/{workspace_id}/docs/search",
    operation_id = "public_search_docs",
    tag = "public-api",
    summary = "Search the docs that the token's creator can see",
    description = "Every word must match the title, the text or a database row's values, the last word as a prefix; best matches first. Send an `id` to `GET /docs/{page_id}`.",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), SearchQuery),
    responses((status = 200, description = "The matching docs, best first; `next_cursor` is `null`", body = List<PageSearchResult>)),
    security(("bearerAuth" = []))
)]
pub(super) async fn search_docs(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    query: Result<Query<SearchQuery>, QueryRejection>,
) -> Response {
    let query = input!(ctx, ApiTokenScope::DocsRead, query);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::DocsRead, &ctx.request_id)
            .await?;
        Ok(List {
            items: api.search_docs(&query.q).await?.items,
            next_cursor: None,
        })
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    get,
    path = "/api/public/v1/workspaces/{workspace_id}/docs/recent",
    operation_id = "public_list_recent_docs",
    tag = "public-api",
    summary = "The docs that the token's creator opened last",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), RecentQuery),
    responses((status = 200, description = "Most recent first; `next_cursor` is `null`", body = List<RecentPage>)),
    security(("bearerAuth" = []))
)]
pub(super) async fn recent_docs(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    query: Result<Query<RecentQuery>, QueryRejection>,
) -> Response {
    let query = input!(ctx, ApiTokenScope::DocsRead, query);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::DocsRead, &ctx.request_id)
            .await?;
        Ok(List {
            items: api.recent_docs(query.limit).await?.items,
            next_cursor: None,
        })
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    get,
    path = "/api/public/v1/workspaces/{workspace_id}/docs/favorites",
    operation_id = "public_list_favorite_docs",
    tag = "public-api",
    summary = "The favorite docs of the token's creator, in order",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`.")),
    responses((status = 200, description = "Every favorite by `position`; `next_cursor` is `null`", body = List<PageFavorite>)),
    security(("bearerAuth" = []))
)]
pub(super) async fn favorite_docs(State(s): State<PublicApiState>, ctx: Ctx) -> Response {
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::DocsRead, &ctx.request_id)
            .await?;
        Ok(List {
            items: api.favorite_docs().await?.items,
            next_cursor: None,
        })
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    put,
    path = "/api/public/v1/workspaces/{workspace_id}/docs/{page_id}/favorite",
    operation_id = "public_add_favorite_doc",
    tag = "public-api",
    summary = "Add a doc to the favorites",
    description = "The doc goes last; a favorite stays where it is.",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("page_id" = String, Path)),
    responses((status = 200, description = "The favorite", body = PageFavorite)),
    security(("bearerAuth" = []))
)]
pub(super) async fn add_favorite_doc(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, page_id)): Path<(String, String)>,
) -> Response {
    let page_id = path_id!(ctx, page_id);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::DocsWrite, &ctx.request_id)
            .await?;
        api.add_favorite_doc(page_id).await
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    delete,
    path = "/api/public/v1/workspaces/{workspace_id}/docs/{page_id}/favorite",
    operation_id = "public_remove_favorite_doc",
    tag = "public-api",
    summary = "Remove a doc from the favorites",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("page_id" = String, Path)),
    responses((status = 204, description = "The doc is no longer a favorite")),
    security(("bearerAuth" = []))
)]
pub(super) async fn remove_favorite_doc(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, page_id)): Path<(String, String)>,
) -> Response {
    let page_id = path_id!(ctx, page_id);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::DocsWrite, &ctx.request_id)
            .await?;
        api.remove_favorite_doc(page_id).await
    }
    .await;
    no_content(&ctx, result)
}

#[utoipa::path(
    post,
    path = "/api/public/v1/workspaces/{workspace_id}/docs/favorites/{page_id}/move",
    operation_id = "public_move_favorite_doc",
    tag = "public-api",
    summary = "Move a favorite doc to another place",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("page_id" = String, Path)),
    request_body = MoveFavoriteBody,
    responses((status = 200, description = "Every favorite in the new order; `next_cursor` is `null`", body = List<PageFavorite>)),
    security(("bearerAuth" = []))
)]
pub(super) async fn move_favorite_doc(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, page_id)): Path<(String, String)>,
    body: Result<Json<MoveFavoriteBody>, JsonRejection>,
) -> Response {
    let page_id = path_id!(ctx, page_id);
    let body = input!(ctx, ApiTokenScope::DocsWrite, body);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::DocsWrite, &ctx.request_id)
            .await?;
        Ok(List {
            items: api.move_favorite_doc(page_id, body.position).await?.items,
            next_cursor: None,
        })
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    get,
    path = "/api/public/v1/workspaces/{workspace_id}/docs/{page_id}/descendants",
    operation_id = "public_list_doc_descendants",
    tag = "public-api",
    summary = "Every live sub-doc of a doc, at any depth",
    description = "Ordered by `parent_id` and `position`. A database's rows are not sub-docs: query the database.",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("page_id" = String, Path)),
    responses((status = 200, description = "The sub-docs; `next_cursor` is `null`", body = List<PageSummary>)),
    security(("bearerAuth" = []))
)]
pub(super) async fn doc_descendants(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, page_id)): Path<(String, String)>,
) -> Response {
    let page_id = path_id!(ctx, page_id);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::DocsRead, &ctx.request_id)
            .await?;
        Ok(List {
            items: api.doc_descendants(page_id).await?.items,
            next_cursor: None,
        })
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    post,
    path = "/api/public/v1/workspaces/{workspace_id}/docs/{page_id}/duplicate",
    operation_id = "public_duplicate_doc",
    tag = "public-api",
    summary = "Copy a doc",
    description = "The copy, titled \"<title> (copy)\", goes right after the doc in the same space. Files are shared, links inside the copied docs point to the copies.",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("page_id" = String, Path)),
    request_body = DuplicateDocBody,
    responses((status = 201, description = "The copy", body = DocView)),
    security(("bearerAuth" = []))
)]
pub(super) async fn duplicate_doc(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, page_id)): Path<(String, String)>,
    body: Result<Json<DuplicateDocBody>, JsonRejection>,
) -> Response {
    let page_id = path_id!(ctx, page_id);
    let body = input!(ctx, ApiTokenScope::DocsWrite, body);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::DocsWrite, &ctx.request_id)
            .await?;
        api.duplicate_doc(page_id, body.include_children).await
    }
    .await;
    ctx.reply(StatusCode::CREATED, result)
}

#[utoipa::path(
    post,
    path = "/api/public/v1/workspaces/{workspace_id}/docs/{page_id}/lock",
    operation_id = "public_lock_doc",
    tag = "public-api",
    summary = "Lock or unlock a doc",
    description = "A locked doc's title, icon and content cannot change (`423`). Any member who sees the doc may lock and unlock it.",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("page_id" = String, Path)),
    request_body = LockDocBody,
    responses((status = 200, description = "The doc", body = DocView)),
    security(("bearerAuth" = []))
)]
pub(super) async fn lock_doc(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, page_id)): Path<(String, String)>,
    body: Result<Json<LockDocBody>, JsonRejection>,
) -> Response {
    let page_id = path_id!(ctx, page_id);
    let body = input!(ctx, ApiTokenScope::DocsWrite, body);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::DocsWrite, &ctx.request_id)
            .await?;
        api.set_doc_lock(page_id, body.locked).await
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    post,
    path = "/api/public/v1/workspaces/{workspace_id}/docs/{page_id}/versions/{version_id}/restore",
    operation_id = "public_restore_doc_version",
    tag = "public-api",
    summary = "Restore a saved version of a doc",
    description = "The doc's content becomes the version's; the current content is saved as a version first. Not on a locked doc.",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("page_id" = String, Path), ("version_id" = String, Path)),
    request_body = RestoreDocVersionBody,
    responses((status = 200, description = "The doc", body = DocView)),
    security(("bearerAuth" = []))
)]
pub(super) async fn restore_doc_version(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, page_id, version_id)): Path<(String, String, String)>,
    body: Result<Json<RestoreDocVersionBody>, JsonRejection>,
) -> Response {
    let page_id = path_id!(ctx, page_id);
    let version_id = path_id!(ctx, version_id);
    let body = input!(ctx, ApiTokenScope::DocsWrite, body);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::DocsWrite, &ctx.request_id)
            .await?;
        api.restore_doc_version(page_id, version_id, body.expected_version)
            .await
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    delete,
    path = "/api/public/v1/workspaces/{workspace_id}/docs/{page_id}/permanent",
    operation_id = "public_purge_doc",
    tag = "public-api",
    summary = "Delete a doc in the trash forever, with its sub-docs",
    description = "Only a doc trashed itself (see `GET /docs/trash`), else `409`. Teamspace docs need a workspace owner or admin (`403`); members delete their own private docs.",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("page_id" = String, Path), ExpectedVersionQuery),
    responses((status = 204, description = "The doc is deleted forever")),
    security(("bearerAuth" = []))
)]
pub(super) async fn purge_doc(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, page_id)): Path<(String, String)>,
    query: Result<Query<ExpectedVersionQuery>, QueryRejection>,
) -> Response {
    let page_id = path_id!(ctx, page_id);
    let query = input!(ctx, ApiTokenScope::DocsWrite, query);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::DocsWrite, &ctx.request_id)
            .await?;
        api.purge_doc(page_id, query.expected_version).await
    }
    .await;
    no_content(&ctx, result)
}

#[utoipa::path(
    post,
    path = "/api/public/v1/workspaces/{workspace_id}/docs/trash/empty",
    operation_id = "public_empty_doc_trash",
    tag = "public-api",
    summary = "Delete forever every doc in the trash that the token's creator may delete",
    description = "Their own private docs, and teamspace docs for workspace owners and admins; other trashed docs stay.",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`.")),
    responses((status = 200, description = "How many trash entries were deleted", body = PageTrashEmptied)),
    security(("bearerAuth" = []))
)]
pub(super) async fn empty_doc_trash(State(s): State<PublicApiState>, ctx: Ctx) -> Response {
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::DocsWrite, &ctx.request_id)
            .await?;
        api.empty_doc_trash().await
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    post,
    path = "/api/public/v1/workspaces/{workspace_id}/teamspaces",
    operation_id = "public_create_teamspace",
    tag = "public-api",
    summary = "Add a teamspace after the others",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`.")),
    request_body = CreateTeamspaceBody,
    responses((status = 201, description = "The new teamspace", body = TeamspaceView)),
    security(("bearerAuth" = []))
)]
pub(super) async fn create_teamspace(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    body: Result<Json<CreateTeamspaceBody>, JsonRejection>,
) -> Response {
    let body = input!(ctx, ApiTokenScope::DocsWrite, body);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::DocsWrite, &ctx.request_id)
            .await?;
        api.create_teamspace(&body.name, body.icon).await
    }
    .await;
    ctx.reply(StatusCode::CREATED, result)
}

#[utoipa::path(
    patch,
    path = "/api/public/v1/workspaces/{workspace_id}/teamspaces/{teamspace_id}",
    operation_id = "public_update_teamspace",
    tag = "public-api",
    summary = "Rename a teamspace, change its icon or link it to a project",
    description = "A linked project must be approved for the token, so the token also needs a task scope. A project has at most one teamspace (`409`).",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("teamspace_id" = String, Path)),
    request_body = UpdateTeamspaceBody,
    responses((status = 200, description = "The teamspace", body = TeamspaceView)),
    security(("bearerAuth" = []))
)]
pub(super) async fn update_teamspace(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, teamspace_id)): Path<(String, String)>,
    body: Result<Json<UpdateTeamspaceBody>, JsonRejection>,
) -> Response {
    let teamspace_id = path_id!(ctx, teamspace_id);
    let body = input!(ctx, ApiTokenScope::DocsWrite, body);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::DocsWrite, &ctx.request_id)
            .await?;
        api.update_teamspace(
            teamspace_id,
            body.expected_version,
            TeamspacePatch {
                name: body.name,
                icon: body.icon,
                project_id: body.project_id,
            },
        )
        .await
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    delete,
    path = "/api/public/v1/workspaces/{workspace_id}/teamspaces/{teamspace_id}",
    operation_id = "public_delete_teamspace",
    tag = "public-api",
    summary = "Delete an empty teamspace",
    description = "Workspace owners and admins only (`403`). A teamspace with docs, or the last one, answers `409`.",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("teamspace_id" = String, Path), ExpectedVersionQuery),
    responses((status = 204, description = "The teamspace is deleted")),
    security(("bearerAuth" = []))
)]
pub(super) async fn delete_teamspace(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, teamspace_id)): Path<(String, String)>,
    query: Result<Query<ExpectedVersionQuery>, QueryRejection>,
) -> Response {
    let teamspace_id = path_id!(ctx, teamspace_id);
    let query = input!(ctx, ApiTokenScope::DocsWrite, query);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::DocsWrite, &ctx.request_id)
            .await?;
        api.delete_teamspace(teamspace_id, query.expected_version)
            .await
    }
    .await;
    no_content(&ctx, result)
}

#[utoipa::path(
    post,
    path = "/api/public/v1/workspaces/{workspace_id}/teamspaces/{teamspace_id}/move",
    operation_id = "public_move_teamspace",
    tag = "public-api",
    summary = "Move a teamspace to another place",
    description = "Position 0 makes it the default teamspace, where new root docs go.",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("teamspace_id" = String, Path)),
    request_body = MoveTeamspaceBody,
    responses((status = 200, description = "Every teamspace in the new order; `next_cursor` is `null`", body = List<TeamspaceView>)),
    security(("bearerAuth" = []))
)]
pub(super) async fn move_teamspace(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, teamspace_id)): Path<(String, String)>,
    body: Result<Json<MoveTeamspaceBody>, JsonRejection>,
) -> Response {
    let teamspace_id = path_id!(ctx, teamspace_id);
    let body = input!(ctx, ApiTokenScope::DocsWrite, body);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::DocsWrite, &ctx.request_id)
            .await?;
        Ok(List {
            items: api
                .move_teamspace(teamspace_id, body.expected_version, body.position)
                .await?,
            next_cursor: None,
        })
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    patch,
    path = "/api/public/v1/workspaces/{workspace_id}/docs/{page_id}/threads/{thread_id}/comments/{comment_id}",
    operation_id = "public_update_doc_comment",
    tag = "public-api",
    summary = "Edit a doc comment of the token's creator",
    description = "Only the comment's author may edit it (`403`).",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("page_id" = String, Path), ("thread_id" = String, Path), ("comment_id" = String, Path)),
    request_body = UpdateDocCommentBody,
    responses((status = 200, description = "The comment", body = DocComment)),
    security(("bearerAuth" = []))
)]
pub(super) async fn update_doc_comment(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, page_id, thread_id, comment_id)): Path<(String, String, String, String)>,
    body: Result<Json<UpdateDocCommentBody>, JsonRejection>,
) -> Response {
    let page_id = path_id!(ctx, page_id);
    let thread_id = path_id!(ctx, thread_id);
    let comment_id = path_id!(ctx, comment_id);
    let body = input!(ctx, ApiTokenScope::DocsWrite, body);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::DocsWrite, &ctx.request_id)
            .await?;
        api.update_doc_comment(page_id, thread_id, comment_id, &body.body)
            .await
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    delete,
    path = "/api/public/v1/workspaces/{workspace_id}/docs/{page_id}/threads/{thread_id}/comments/{comment_id}",
    operation_id = "public_delete_doc_comment",
    tag = "public-api",
    summary = "Delete a doc comment of the token's creator",
    description = "Only the comment's author may delete it (`403`). It keeps its place in the thread, empty.",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("page_id" = String, Path), ("thread_id" = String, Path), ("comment_id" = String, Path)),
    responses((status = 204, description = "The comment is deleted")),
    security(("bearerAuth" = []))
)]
pub(super) async fn delete_doc_comment(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, page_id, thread_id, comment_id)): Path<(String, String, String, String)>,
) -> Response {
    let page_id = path_id!(ctx, page_id);
    let thread_id = path_id!(ctx, thread_id);
    let comment_id = path_id!(ctx, comment_id);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::DocsWrite, &ctx.request_id)
            .await?;
        api.delete_doc_comment(page_id, thread_id, comment_id).await
    }
    .await;
    no_content(&ctx, result)
}

#[utoipa::path(
    post,
    path = "/api/public/v1/workspaces/{workspace_id}/docs/{page_id}/threads/{thread_id}/resolve",
    operation_id = "public_resolve_doc_thread",
    tag = "public-api",
    summary = "Resolve a doc comment thread",
    description = "A resolved thread stays resolved.",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("page_id" = String, Path), ("thread_id" = String, Path)),
    responses((status = 200, description = "The thread", body = DocThread)),
    security(("bearerAuth" = []))
)]
pub(super) async fn resolve_doc_thread(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, page_id, thread_id)): Path<(String, String, String)>,
) -> Response {
    let page_id = path_id!(ctx, page_id);
    let thread_id = path_id!(ctx, thread_id);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::DocsWrite, &ctx.request_id)
            .await?;
        api.set_doc_thread_resolved(page_id, thread_id, true).await
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    post,
    path = "/api/public/v1/workspaces/{workspace_id}/docs/{page_id}/threads/{thread_id}/reopen",
    operation_id = "public_reopen_doc_thread",
    tag = "public-api",
    summary = "Reopen a resolved doc comment thread",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("page_id" = String, Path), ("thread_id" = String, Path)),
    responses((status = 200, description = "The thread", body = DocThread)),
    security(("bearerAuth" = []))
)]
pub(super) async fn reopen_doc_thread(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, page_id, thread_id)): Path<(String, String, String)>,
) -> Response {
    let page_id = path_id!(ctx, page_id);
    let thread_id = path_id!(ctx, thread_id);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::DocsWrite, &ctx.request_id)
            .await?;
        api.set_doc_thread_resolved(page_id, thread_id, false).await
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    delete,
    path = "/api/public/v1/workspaces/{workspace_id}/docs/{page_id}/threads/{thread_id}",
    operation_id = "public_delete_doc_thread",
    tag = "public-api",
    summary = "Delete a doc comment thread with its comments",
    description = "Its creator may delete it while nobody else commented in it (else `409`); a member who may moderate comments may delete any thread (else `403`).",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("page_id" = String, Path), ("thread_id" = String, Path)),
    responses((status = 204, description = "The thread is deleted")),
    security(("bearerAuth" = []))
)]
pub(super) async fn delete_doc_thread(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, page_id, thread_id)): Path<(String, String, String)>,
) -> Response {
    let page_id = path_id!(ctx, page_id);
    let thread_id = path_id!(ctx, thread_id);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::DocsWrite, &ctx.request_id)
            .await?;
        api.delete_doc_thread(page_id, thread_id).await
    }
    .await;
    no_content(&ctx, result)
}
