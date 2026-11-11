//! Docs database endpoints (spec P3 14.3) under `/docs/{page_id}`, where the page is a database.
use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use orbit_platform::{Id, Problem};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use utoipa::ToSchema;

use super::{Ctx, PublicApiState, Responder, problem_response};
use crate::repositories::api_tokens::ApiTokenScope;
use crate::repositories::databases::{
    AgentDatabase, AgentPropertyChanges, AgentQuery, AgentRow, AgentRowPage, DatabaseError,
    DatabaseProperty, PropertyUpdate, invalid_detail,
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

/// See `create_database_property` in `docs/mcp.md` for the config shapes.
#[derive(Deserialize, ToSchema)]
#[schema(as = PublicCreateDatabasePropertyBody)]
#[serde(deny_unknown_fields)]
pub(super) struct CreatePropertyBody {
    /// 1 to 200 characters.
    name: String,
    /// Any type but `title`, such as `select`, `status`, `number` or `date`.
    #[serde(rename = "type")]
    property_type: String,
    /// The type's config. Select, multi-select and status take `{"options": [...]}` with option
    /// names or `{name, color, group}` objects. Omitted: the type's default.
    #[schema(value_type = Option<Object>)]
    config: Option<Value>,
    /// Index among the properties; omitted appends it.
    position: Option<i64>,
}

/// Send at least one of `name`, `type`, `config`, `add_options` or `position`.
#[derive(Deserialize, ToSchema)]
#[schema(as = PublicUpdateDatabasePropertyBody)]
#[serde(deny_unknown_fields)]
pub(super) struct UpdatePropertyBody {
    /// 1 to 200 characters.
    name: Option<String>,
    /// A new type converts the values and gives the type's default config.
    #[serde(rename = "type")]
    property_type: Option<String>,
    /// The whole config. Options left out are removed from every row; an option sent by name
    /// only keeps its id, color and group.
    #[schema(value_type = Option<Object>)]
    config: Option<Value>,
    /// Options to add after the current ones, by name or as `{name, color, group}`; names the
    /// property already has are skipped. Not with `config`.
    #[serde(default)]
    #[schema(value_type = Vec<Object>)]
    add_options: Vec<Value>,
    /// The new index among the properties.
    position: Option<i64>,
    /// Returns what the change would do without saving it.
    #[serde(default)]
    dry_run: bool,
}

impl Responder {
    pub(super) fn database_problem(&self, error: DatabaseError) -> Problem {
        let (status, code, title, detail) = match error {
            DatabaseError::Invalid { field } => {
                return Problem::validation(
                    &self.request_id,
                    &self.instance,
                    [(field.as_str(), invalid_detail(&field))],
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
            DatabaseError::PropertyLimit => (
                StatusCode::CONFLICT,
                "database_property_limit",
                "Database property limit",
                "A database holds at most 100 properties.",
            ),
            DatabaseError::TitleProperty => (
                StatusCode::CONFLICT,
                "database_title_property",
                "Title property",
                "The title property cannot be deleted or change its type.",
            ),
            DatabaseError::FormulaInvalid(problem) => {
                return Problem::validation(
                    &self.request_id,
                    &self.instance,
                    [("config.expression", problem.message.as_str())],
                );
            }
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

#[utoipa::path(
    post,
    path = "/api/public/v1/workspaces/{workspace_id}/docs/{page_id}/database/properties",
    operation_id = "public_create_database_property",
    tag = "public-api",
    summary = "Add a property to a database",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("page_id" = String, Path, description = "The database's page id.")),
    request_body = CreatePropertyBody,
    responses((status = 201, description = "The new property", body = DatabaseProperty)),
    security(("bearerAuth" = []))
)]
pub(super) async fn create_property(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, page)): Path<(String, String)>,
    body: Result<Json<CreatePropertyBody>, JsonRejection>,
) -> Response {
    let page = path_id!(ctx, page);
    let body = input!(ctx, ApiTokenScope::DocsWrite, body);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::DocsWrite, &ctx.request_id)
            .await?;
        api.create_database_property(
            page,
            &body.name,
            &body.property_type,
            body.config,
            body.position,
        )
        .await
    }
    .await;
    ctx.reply(StatusCode::CREATED, result)
}

#[utoipa::path(
    patch,
    path = "/api/public/v1/workspaces/{workspace_id}/docs/{page_id}/database/properties/{property}",
    operation_id = "public_update_database_property",
    tag = "public-api",
    summary = "Rename, retype, reconfigure or move a database property",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("page_id" = String, Path, description = "The database's page id."), ("property" = String, Path, description = "The property's name or id.")),
    request_body = UpdatePropertyBody,
    responses((status = 200, description = "The property, with the values the change converted or cleared", body = PropertyUpdate)),
    security(("bearerAuth" = []))
)]
pub(super) async fn update_property(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, page, property)): Path<(String, String, String)>,
    body: Result<Json<UpdatePropertyBody>, JsonRejection>,
) -> Response {
    let page = path_id!(ctx, page);
    let body = input!(ctx, ApiTokenScope::DocsWrite, body);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::DocsWrite, &ctx.request_id)
            .await?;
        let changes = AgentPropertyChanges {
            name: body.name,
            property_type: body.property_type,
            config: body.config,
            add_options: body.add_options,
            position: body.position,
            dry_run: body.dry_run,
        };
        api.update_database_property(page, &property, changes).await
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    delete,
    path = "/api/public/v1/workspaces/{workspace_id}/docs/{page_id}/database/properties/{property}",
    operation_id = "public_delete_database_property",
    tag = "public-api",
    summary = "Delete a database property with its values",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("page_id" = String, Path, description = "The database's page id."), ("property" = String, Path, description = "The property's name or id.")),
    responses((status = 204, description = "Deleted")),
    security(("bearerAuth" = []))
)]
pub(super) async fn delete_property(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, page, property)): Path<(String, String, String)>,
) -> Response {
    let page = path_id!(ctx, page);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::DocsWrite, &ctx.request_id)
            .await?;
        api.delete_database_property(page, &property).await
    }
    .await;
    match result {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => ctx.error(error),
    }
}
