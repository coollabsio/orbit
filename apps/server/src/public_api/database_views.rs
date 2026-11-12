//! Docs database creation, views and linked views under `/databases` and `/docs/{page_id}`.
use axum::Json;
use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use orbit_platform::Id;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use super::{Ctx, PublicApiState, problem_response};
use crate::repositories::api_tokens::ApiTokenScope;
use crate::repositories::databases::{
    AgentDatabase, AgentPropertySpec, AgentViewChanges, AgentViewDetail, AgentViewSpec,
    DatabaseError,
};
use crate::token_api::{LinkedDatabase, NewDatabase, TokenApiError};

#[derive(Deserialize, ToSchema)]
#[schema(as = PublicCreateDatabaseBody)]
#[serde(deny_unknown_fields)]
pub(super) struct CreateDatabaseBody {
    /// The database goes under this doc, in its space.
    #[schema(value_type = Option<String>)]
    parent_id: Option<Id>,
    /// Root databases only: the teamspace of the database.
    #[schema(value_type = Option<String>)]
    teamspace_id: Option<Id>,
    /// Root databases only: `true` puts the database in the token creator's private space.
    private: Option<bool>,
    /// Up to 500 characters.
    #[serde(default)]
    title: String,
    /// Up to 64 characters.
    icon: Option<String>,
    /// The properties after the title property "Name", up to 99. Omitted: a "Tags" multi-select.
    /// Relations, rollups, formulas, buttons and unique IDs come afterwards, with
    /// `POST …/database/properties`.
    properties: Option<Vec<AgentPropertySpec>>,
    /// 1 to 50 views. Omitted: one "Table" view. A view's filter, sorts, columns and group name
    /// the properties above and their options.
    views: Option<Vec<AgentViewSpec>>,
}

/// Which views a request reads or changes.
#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
#[serde(deny_unknown_fields)]
pub(super) struct LinkQuery {
    /// The `link_id` of a linked view block of this database. Omitted: the database's own views.
    #[param(value_type = Option<String>)]
    link_id: Option<Id>,
}

#[derive(Deserialize, ToSchema)]
#[schema(as = PublicCreateDatabaseLinkBody)]
#[serde(deny_unknown_fields)]
pub(super) struct CreateLinkBody {
    /// The doc that shows the database. The block goes at the end of the doc.
    #[schema(value_type = String)]
    host_page_id: Id,
    /// An own view of the database whose type and config the block's first view copies.
    /// Omitted: the first view.
    #[schema(value_type = Option<String>)]
    from_view_id: Option<Id>,
}

#[derive(Serialize, ToSchema)]
#[schema(as = PublicDatabaseViews)]
pub(super) struct Views {
    items: Vec<AgentViewDetail>,
}

#[utoipa::path(
    post,
    path = "/api/public/v1/workspaces/{workspace_id}/databases",
    operation_id = "public_create_database",
    tag = "public-api",
    summary = "Create a database with its properties and views",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`.")),
    request_body = CreateDatabaseBody,
    responses((status = 201, description = "The new database", body = AgentDatabase)),
    security(("bearerAuth" = []))
)]
pub(super) async fn create_database(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    body: Result<Json<CreateDatabaseBody>, JsonRejection>,
) -> Response {
    let body = input!(ctx, ApiTokenScope::DocsWrite, body);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::DocsWrite, &ctx.request_id)
            .await?;
        api.create_database(NewDatabase {
            parent_id: body.parent_id,
            teamspace_id: body.teamspace_id,
            private: body.private,
            title: body.title,
            icon: body.icon,
            properties: body.properties,
            views: body.views,
        })
        .await
    }
    .await;
    ctx.reply(StatusCode::CREATED, result)
}

#[utoipa::path(
    get,
    path = "/api/public/v1/workspaces/{workspace_id}/docs/{page_id}/database/views",
    operation_id = "public_list_database_views",
    tag = "public-api",
    summary = "The views of a database or of a linked view block, with their filters, sorts and columns",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("page_id" = String, Path, description = "The database's page id."), LinkQuery),
    responses((status = 200, description = "The views in order", body = Views)),
    security(("bearerAuth" = []))
)]
pub(super) async fn views(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, page)): Path<(String, String)>,
    query: Result<Query<LinkQuery>, QueryRejection>,
) -> Response {
    let page = path_id!(ctx, page);
    let query = input!(ctx, ApiTokenScope::DocsRead, query);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::DocsRead, &ctx.request_id)
            .await?;
        api.database_views(page, query.link_id)
            .await
            .map(|items| Views { items })
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    get,
    path = "/api/public/v1/workspaces/{workspace_id}/docs/{page_id}/database/views/{view_id}",
    operation_id = "public_get_database_view",
    tag = "public-api",
    summary = "One view of a database or of a linked view block",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("page_id" = String, Path, description = "The database's page id."), ("view_id" = String, Path), LinkQuery),
    responses((status = 200, description = "The view", body = AgentViewDetail)),
    security(("bearerAuth" = []))
)]
pub(super) async fn view(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, page, view)): Path<(String, String, String)>,
    query: Result<Query<LinkQuery>, QueryRejection>,
) -> Response {
    let page = path_id!(ctx, page);
    let view = path_id!(ctx, view);
    let query = input!(ctx, ApiTokenScope::DocsRead, query);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::DocsRead, &ctx.request_id)
            .await?;
        api.database_views(page, query.link_id)
            .await?
            .into_iter()
            .find(|candidate| candidate.id == view)
            .ok_or_else(|| DatabaseError::ViewNotFound.into())
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    post,
    path = "/api/public/v1/workspaces/{workspace_id}/docs/{page_id}/database/views",
    operation_id = "public_create_database_view",
    tag = "public-api",
    summary = "Add a view to a database or to a linked view block",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("page_id" = String, Path, description = "The database's page id."), LinkQuery),
    request_body = AgentViewSpec,
    responses((status = 201, description = "The new view", body = AgentViewDetail)),
    security(("bearerAuth" = []))
)]
pub(super) async fn create_view(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, page)): Path<(String, String)>,
    query: Result<Query<LinkQuery>, QueryRejection>,
    body: Result<Json<AgentViewSpec>, JsonRejection>,
) -> Response {
    let page = path_id!(ctx, page);
    let query = input!(ctx, ApiTokenScope::DocsWrite, query);
    let body = input!(ctx, ApiTokenScope::DocsWrite, body);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::DocsWrite, &ctx.request_id)
            .await?;
        api.create_database_view(page, query.link_id, body).await
    }
    .await;
    ctx.reply(StatusCode::CREATED, result)
}

#[utoipa::path(
    patch,
    path = "/api/public/v1/workspaces/{workspace_id}/docs/{page_id}/database/views/{view_id}",
    operation_id = "public_update_database_view",
    tag = "public-api",
    summary = "Rename, move or reconfigure a view",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("page_id" = String, Path, description = "The database's page id."), ("view_id" = String, Path), LinkQuery),
    request_body = AgentViewChanges,
    responses((status = 200, description = "The view", body = AgentViewDetail)),
    security(("bearerAuth" = []))
)]
pub(super) async fn update_view(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, page, view)): Path<(String, String, String)>,
    query: Result<Query<LinkQuery>, QueryRejection>,
    body: Result<Json<AgentViewChanges>, JsonRejection>,
) -> Response {
    let page = path_id!(ctx, page);
    let view = path_id!(ctx, view);
    let query = input!(ctx, ApiTokenScope::DocsWrite, query);
    let body = input!(ctx, ApiTokenScope::DocsWrite, body);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::DocsWrite, &ctx.request_id)
            .await?;
        api.update_database_view(page, view, query.link_id, body)
            .await
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    delete,
    path = "/api/public/v1/workspaces/{workspace_id}/docs/{page_id}/database/views/{view_id}",
    operation_id = "public_delete_database_view",
    tag = "public-api",
    summary = "Delete a view; the last view stays",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("page_id" = String, Path, description = "The database's page id."), ("view_id" = String, Path), LinkQuery),
    responses((status = 204, description = "Deleted")),
    security(("bearerAuth" = []))
)]
pub(super) async fn delete_view(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, page, view)): Path<(String, String, String)>,
    query: Result<Query<LinkQuery>, QueryRejection>,
) -> Response {
    let page = path_id!(ctx, page);
    let view = path_id!(ctx, view);
    let query = input!(ctx, ApiTokenScope::DocsWrite, query);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::DocsWrite, &ctx.request_id)
            .await?;
        api.delete_database_view(page, view, query.link_id).await
    }
    .await;
    match result {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => ctx.error(error),
    }
}

#[utoipa::path(
    post,
    path = "/api/public/v1/workspaces/{workspace_id}/docs/{page_id}/database/links",
    operation_id = "public_create_database_link",
    tag = "public-api",
    summary = "Show a database in a doc as a linked view block",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("page_id" = String, Path, description = "The database's page id.")),
    request_body = CreateLinkBody,
    responses((status = 201, description = "The linked view block with its first view", body = LinkedDatabase)),
    security(("bearerAuth" = []))
)]
pub(super) async fn create_link(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, page)): Path<(String, String)>,
    body: Result<Json<CreateLinkBody>, JsonRejection>,
) -> Response {
    let page = path_id!(ctx, page);
    let body = input!(ctx, ApiTokenScope::DocsWrite, body);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::DocsWrite, &ctx.request_id)
            .await?;
        api.create_database_link(page, body.host_page_id, body.from_view_id)
            .await
    }
    .await;
    ctx.reply(StatusCode::CREATED, result)
}

#[utoipa::path(
    delete,
    path = "/api/public/v1/workspaces/{workspace_id}/docs/{page_id}/database/links/{link_id}",
    operation_id = "public_delete_database_link",
    tag = "public-api",
    summary = "Remove a linked view block from its doc, with its views",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("page_id" = String, Path, description = "The database's page id."), ("link_id" = String, Path)),
    responses((status = 204, description = "Deleted")),
    security(("bearerAuth" = []))
)]
pub(super) async fn delete_link(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, page, link)): Path<(String, String, String)>,
) -> Response {
    let page = path_id!(ctx, page);
    let link = path_id!(ctx, link);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::DocsWrite, &ctx.request_id)
            .await?;
        api.delete_database_link(page, link).await
    }
    .await;
    match result {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => ctx.error(error),
    }
}
