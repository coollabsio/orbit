//! Docs database endpoints (spec P3 14.3) under `/docs/{page_id}`, where the page is a database.
use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use orbit_platform::{Id, Problem};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use utoipa::ToSchema;

use super::{Ctx, PublicApiState, Responder, problem_response};
use crate::repositories::api_tokens::ApiTokenScope;
use crate::repositories::databases::{
    AgentDatabase, AgentQuery, AgentRow, AgentRowPage, DatabaseError,
};
use crate::token_api::{DatabaseRowPatch, NewDatabaseRow, TokenApiError};

/// See `query_database` in `docs/mcp.md` for the filter, sort and value shapes.
#[derive(Deserialize, ToSchema)]
#[schema(as = PublicQueryDatabaseBody)]
#[serde(deny_unknown_fields)]
pub(super) struct QueryDatabaseBody {
    /// An AND/OR group of rules, as a view's filter; `property_id` takes a property name or id
    /// and `value` option names, user ids, emails or `"me"`.
    #[schema(value_type = Option<Object>)]
    filter: Option<Value>,
    /// Up to 10 `{property_id, direction}` with names or ids.
    #[schema(value_type = Option<Vec<Object>>)]
    sorts: Option<Value>,
    /// Starts from this view's filter and sorts: `filter` must match too, `sorts` replace them.
    #[schema(value_type = Option<String>)]
    view_id: Option<Id>,
    /// 1 to 100; default 50.
    limit: Option<usize>,
    /// The `next_cursor` of the previous page, with the same query (else `422`).
    cursor: Option<String>,
    /// IANA zone for relative dates and computed values; default `UTC`.
    tz: Option<String>,
}

#[derive(Deserialize, ToSchema)]
#[schema(as = PublicCreateDatabaseRowBody)]
#[serde(deny_unknown_fields)]
pub(super) struct CreateRowBody {
    /// Up to 500 characters.
    #[serde(default)]
    title: String,
    /// Up to 50 values by property name or id.
    #[serde(default)]
    #[schema(value_type = Object)]
    values: Map<String, Value>,
    /// The body as GitHub-flavored Markdown; with a template, it replaces the template's body.
    #[serde(default)]
    markdown: String,
    /// A template of the database that fills the title, values and body.
    #[schema(value_type = Option<String>)]
    template_id: Option<Id>,
}

/// Send `values`, `title` or both. Last write wins: no version.
#[derive(Deserialize, ToSchema)]
#[schema(as = PublicUpdateDatabaseRowBody)]
#[serde(deny_unknown_fields)]
pub(super) struct UpdateRowBody {
    /// Up to 50 values by property name or id; `null` clears a value.
    #[serde(default)]
    #[schema(value_type = Object)]
    values: Map<String, Value>,
    /// Up to 500 characters.
    title: Option<String>,
}

#[derive(Deserialize, ToSchema)]
#[schema(as = PublicTrashDatabaseRowsBody)]
#[serde(deny_unknown_fields)]
pub(super) struct TrashRowsBody {
    /// 1 to 100 row ids; ids that are not live rows of the database are skipped.
    #[schema(value_type = Vec<String>)]
    row_ids: Vec<Id>,
}

#[derive(Serialize, ToSchema)]
#[schema(as = PublicTrashedDatabaseRows)]
pub(super) struct Trashed {
    /// Rows moved to the trash.
    trashed: u64,
}

impl Responder {
    pub(super) fn database_problem(&self, error: DatabaseError) -> Problem {
        let (status, code, title, detail) = match error {
            DatabaseError::Invalid { field } => {
                return Problem::validation(
                    &self.request_id,
                    &self.instance,
                    [(
                        field,
                        "The value is invalid for this database: an unknown or shared property name, an unknown option or member, or a value of the wrong type.",
                    )],
                );
            }
            DatabaseError::RowLimit => (
                StatusCode::CONFLICT,
                "database_row_limit",
                "Database row limit",
                "A database holds at most 5,000 rows.",
            ),
            DatabaseError::Locked => (
                StatusCode::LOCKED,
                "page_locked",
                "Doc locked",
                "The database is locked.",
            ),
            _ => return self.not_found(),
        };
        self.problem(status, code, title, detail)
    }
}

#[utoipa::path(
    get,
    path = "/api/public/v1/workspaces/{workspace_id}/docs/{page_id}/database",
    operation_id = "public_get_database",
    tag = "public-api",
    summary = "A database's properties, views and templates",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("page_id" = String, Path, description = "The database's page id.")),
    responses((status = 200, description = "The database", body = AgentDatabase)),
    security(("bearerAuth" = []))
)]
pub(super) async fn database(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, page)): Path<(String, String)>,
) -> Response {
    let page = path_id!(ctx, page);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::DocsRead, &ctx.request_id)
            .await?;
        api.get_database(page).await
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    post,
    path = "/api/public/v1/workspaces/{workspace_id}/docs/{page_id}/database/query",
    operation_id = "public_query_database",
    tag = "public-api",
    summary = "Rows of a database that match a filter, sorted",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("page_id" = String, Path, description = "The database's page id.")),
    request_body = QueryDatabaseBody,
    responses((status = 200, description = "A page of rows", body = AgentRowPage)),
    security(("bearerAuth" = []))
)]
pub(super) async fn query_database(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, page)): Path<(String, String)>,
    body: Result<Json<QueryDatabaseBody>, JsonRejection>,
) -> Response {
    let page = path_id!(ctx, page);
    let body = input!(ctx, ApiTokenScope::DocsRead, body);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::DocsRead, &ctx.request_id)
            .await?;
        let query = AgentQuery {
            filter: body.filter,
            sorts: body.sorts,
            view_id: body.view_id,
            limit: body.limit,
            cursor: body.cursor,
            tz: body.tz,
        };
        api.query_database(page, query).await
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    post,
    path = "/api/public/v1/workspaces/{workspace_id}/docs/{page_id}/rows",
    operation_id = "public_create_database_row",
    tag = "public-api",
    summary = "Add a row to a database",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("page_id" = String, Path, description = "The database's page id.")),
    request_body = CreateRowBody,
    responses((status = 201, description = "The new row", body = AgentRow)),
    security(("bearerAuth" = []))
)]
pub(super) async fn create_row(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, page)): Path<(String, String)>,
    body: Result<Json<CreateRowBody>, JsonRejection>,
) -> Response {
    let page = path_id!(ctx, page);
    let body = input!(ctx, ApiTokenScope::DocsWrite, body);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::DocsWrite, &ctx.request_id)
            .await?;
        let input = NewDatabaseRow {
            title: body.title,
            values: body.values,
            markdown: body.markdown,
            template_id: body.template_id,
        };
        api.create_database_row(page, input).await
    }
    .await;
    ctx.reply(StatusCode::CREATED, result)
}

#[utoipa::path(
    patch,
    path = "/api/public/v1/workspaces/{workspace_id}/docs/{page_id}/rows/{row_id}",
    operation_id = "public_update_database_row",
    tag = "public-api",
    summary = "Change a row's values or title",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("page_id" = String, Path, description = "The database's page id."), ("row_id" = String, Path)),
    request_body = UpdateRowBody,
    responses((status = 200, description = "The row", body = AgentRow)),
    security(("bearerAuth" = []))
)]
pub(super) async fn update_row(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, page, row)): Path<(String, String, String)>,
    body: Result<Json<UpdateRowBody>, JsonRejection>,
) -> Response {
    let page = path_id!(ctx, page);
    let row = path_id!(ctx, row);
    let body = input!(ctx, ApiTokenScope::DocsWrite, body);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::DocsWrite, &ctx.request_id)
            .await?;
        let patch = DatabaseRowPatch {
            title: body.title,
            values: body.values,
        };
        api.update_database_row(page, row, patch).await
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    post,
    path = "/api/public/v1/workspaces/{workspace_id}/docs/{page_id}/rows/trash",
    operation_id = "public_trash_database_rows",
    tag = "public-api",
    summary = "Move rows of a database to the trash",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("page_id" = String, Path, description = "The database's page id.")),
    request_body = TrashRowsBody,
    responses((status = 200, description = "How many rows went to the trash", body = Trashed)),
    security(("bearerAuth" = []))
)]
pub(super) async fn trash_rows(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, page)): Path<(String, String)>,
    body: Result<Json<TrashRowsBody>, JsonRejection>,
) -> Response {
    let page = path_id!(ctx, page);
    let body = input!(ctx, ApiTokenScope::DocsWrite, body);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::DocsWrite, &ctx.request_id)
            .await?;
        api.trash_database_rows(page, &body.row_ids)
            .await
            .map(|trashed| Trashed { trashed })
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}
