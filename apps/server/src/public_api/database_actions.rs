//! Database row moves, row templates and formula previews under `/docs/{page_id}`, where the
//! page is a database.
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{patch, post};
use axum::{Json, Router};
use orbit_platform::Id;
use serde::Deserialize;
use serde_json::{Map, Value};
use utoipa::ToSchema;

use super::{Ctx, PublicApiState, problem_response};
use crate::repositories::api_tokens::ApiTokenScope;
use crate::repositories::databases::{AgentRow, AgentTemplate, FormulaPreview};
use crate::token_api::TokenApiError;

pub(super) fn routes() -> Router<PublicApiState> {
    Router::new()
        .route("/docs/{page_id}/rows/{row_id}/move", post(move_row))
        .route("/docs/{page_id}/database/templates", post(create_template))
        .route(
            "/docs/{page_id}/database/templates/{template_id}",
            patch(update_template).delete(delete_template),
        )
        .route(
            "/docs/{page_id}/database/formula/preview",
            post(preview_formula),
        )
}

#[derive(Deserialize, ToSchema)]
#[schema(as = PublicMoveDatabaseRowBody)]
#[serde(deny_unknown_fields)]
pub(super) struct MoveRowBody {
    /// The row to put this one right after; `null` moves it first. The order is the one that
    /// views without a sort show.
    #[schema(value_type = Option<String>, required = true)]
    after_row_id: Option<Id>,
    /// Values written with the move, by property name or id, as in `PATCH …/rows/{row_id}`
    /// (a board card dropped into another column sets its group value).
    #[serde(default)]
    #[schema(value_type = Object)]
    values: Map<String, Value>,
}

#[derive(Deserialize, ToSchema)]
#[schema(as = PublicCreateDatabaseTemplateBody)]
#[serde(deny_unknown_fields)]
pub(super) struct CreateTemplateBody {
    /// What "New" lists: 1 to 200 characters.
    name: String,
    /// A live row or template of the database whose title, icon, values and body the template
    /// copies; omitted makes an empty template. Edit its content as a doc (`PATCH /docs/{id}`).
    #[schema(value_type = Option<String>)]
    from_row_id: Option<Id>,
}

/// Send `name`, `position` or both.
#[derive(Deserialize, ToSchema)]
#[schema(as = PublicUpdateDatabaseTemplateBody)]
#[serde(deny_unknown_fields)]
pub(super) struct UpdateTemplateBody {
    /// 1 to 200 characters.
    name: Option<String>,
    /// The template's place in the "New" list (0 is first).
    position: Option<i64>,
}

#[derive(Deserialize, ToSchema)]
#[schema(as = PublicPreviewDatabaseFormulaBody)]
#[serde(deny_unknown_fields)]
pub(super) struct PreviewFormulaBody {
    /// The expression, as a formula property's `config.expression`; `prop("…")` takes a
    /// property name or id.
    expression: String,
    /// The formula property being edited, so it cannot read itself; omitted for a new one.
    #[schema(value_type = Option<String>)]
    property_id: Option<Id>,
    /// IANA zone for date functions; default `UTC`.
    tz: Option<String>,
}

#[utoipa::path(
    post,
    path = "/api/public/v1/workspaces/{workspace_id}/docs/{page_id}/rows/{row_id}/move",
    operation_id = "public_move_database_row",
    tag = "public-api",
    summary = "Move a row in a database's order",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("page_id" = String, Path, description = "The database's page id."), ("row_id" = String, Path)),
    request_body = MoveRowBody,
    responses((status = 200, description = "The row", body = AgentRow)),
    security(("bearerAuth" = []))
)]
pub(super) async fn move_row(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, page, row)): Path<(String, String, String)>,
    body: Result<Json<MoveRowBody>, JsonRejection>,
) -> Response {
    let page = path_id!(ctx, page);
    let row = path_id!(ctx, row);
    let body = input!(ctx, ApiTokenScope::DocsWrite, body);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::DocsWrite, &ctx.request_id)
            .await?;
        api.move_database_row(page, row, body.after_row_id, body.values)
            .await
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    post,
    path = "/api/public/v1/workspaces/{workspace_id}/docs/{page_id}/database/templates",
    operation_id = "public_create_database_template",
    tag = "public-api",
    summary = "Add a row template to a database",
    description = "The template is a page: read and edit its title, values and body with the doc and row endpoints. `POST …/rows` with `template_id` fills a new row from it.",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("page_id" = String, Path, description = "The database's page id.")),
    request_body = CreateTemplateBody,
    responses((status = 201, description = "The new template", body = AgentTemplate)),
    security(("bearerAuth" = []))
)]
pub(super) async fn create_template(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, page)): Path<(String, String)>,
    body: Result<Json<CreateTemplateBody>, JsonRejection>,
) -> Response {
    let page = path_id!(ctx, page);
    let body = input!(ctx, ApiTokenScope::DocsWrite, body);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::DocsWrite, &ctx.request_id)
            .await?;
        api.create_database_template(page, &body.name, body.from_row_id)
            .await
    }
    .await;
    ctx.reply(StatusCode::CREATED, result)
}

#[utoipa::path(
    patch,
    path = "/api/public/v1/workspaces/{workspace_id}/docs/{page_id}/database/templates/{template_id}",
    operation_id = "public_update_database_template",
    tag = "public-api",
    summary = "Rename a row template or move it in the list",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("page_id" = String, Path, description = "The database's page id."), ("template_id" = String, Path)),
    request_body = UpdateTemplateBody,
    responses((status = 200, description = "The template", body = AgentTemplate)),
    security(("bearerAuth" = []))
)]
pub(super) async fn update_template(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, page, template)): Path<(String, String, String)>,
    body: Result<Json<UpdateTemplateBody>, JsonRejection>,
) -> Response {
    let page = path_id!(ctx, page);
    let template = path_id!(ctx, template);
    let body = input!(ctx, ApiTokenScope::DocsWrite, body);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::DocsWrite, &ctx.request_id)
            .await?;
        api.update_database_template(page, template, body.name, body.position)
            .await
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    delete,
    path = "/api/public/v1/workspaces/{workspace_id}/docs/{page_id}/database/templates/{template_id}",
    operation_id = "public_delete_database_template",
    tag = "public-api",
    summary = "Delete a row template",
    description = "Rows made from it stay. Templates have no version, as in the web app.",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("page_id" = String, Path, description = "The database's page id."), ("template_id" = String, Path)),
    responses((status = 204, description = "The template is deleted")),
    security(("bearerAuth" = []))
)]
pub(super) async fn delete_template(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, page, template)): Path<(String, String, String)>,
) -> Response {
    let page = path_id!(ctx, page);
    let template = path_id!(ctx, template);
    if !ctx.principal.allows(ApiTokenScope::DocsWrite) {
        return ctx.error(TokenApiError::MissingScope);
    }
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::DocsWrite, &ctx.request_id)
            .await?;
        api.delete_database_template(page, template).await
    }
    .await;
    match result {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => ctx.error(error),
    }
}

#[utoipa::path(
    post,
    path = "/api/public/v1/workspaces/{workspace_id}/docs/{page_id}/database/formula/preview",
    operation_id = "public_preview_database_formula",
    tag = "public-api",
    summary = "Try a formula on a database's first rows",
    description = "Returns the result type, or the error with its position, and the values on the first rows. Writes nothing: set the formula with `PATCH …/properties/{property}`.",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("page_id" = String, Path, description = "The database's page id.")),
    request_body = PreviewFormulaBody,
    responses((status = 200, description = "The preview", body = FormulaPreview)),
    security(("bearerAuth" = []))
)]
pub(super) async fn preview_formula(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, page)): Path<(String, String)>,
    body: Result<Json<PreviewFormulaBody>, JsonRejection>,
) -> Response {
    let page = path_id!(ctx, page);
    let body = input!(ctx, ApiTokenScope::DocsRead, body);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::DocsRead, &ctx.request_id)
            .await?;
        api.preview_database_formula(page, &body.expression, body.property_id, body.tz.as_deref())
            .await
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}
