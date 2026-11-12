//! Project setup writes: a project's name and color, its statuses and its labels. An import
//! from another tool makes the source's workflow with these before it writes tasks.
use axum::Json;
use axum::extract::Path;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use axum::response::Response;
use serde::Deserialize;
use utoipa::ToSchema;

use super::{Ctx, LabelOut, ProjectOut, PublicApiState, StatusOut, problem_response};
use crate::repositories::api_tokens::ApiTokenScope;
use crate::token_api::{StatusPatch, TokenApiError};
use orbit_platform::Id;

/// A missing field keeps its value.
#[derive(Deserialize, ToSchema)]
#[schema(as = PublicUpdateProjectBody)]
#[serde(deny_unknown_fields)]
pub(super) struct UpdateProjectBody {
    /// The `version` that you read.
    expected_version: u64,
    /// 1 to 200 characters.
    name: Option<String>,
    /// `#rrggbb`.
    color: Option<String>,
}

#[derive(Deserialize, ToSchema)]
#[schema(as = PublicCreateStatusBody)]
#[serde(deny_unknown_fields)]
pub(super) struct CreateStatusBody {
    /// 1 to 200 characters.
    name: String,
    /// `unstarted`, `started`, `completed` or `cancelled`. Tasks in a `completed` or
    /// `cancelled` status are done.
    category: String,
    /// `#rrggbb`; a gray, yellow or green that matches the category when unset.
    color: Option<String>,
    /// Up to 20,000 characters.
    description: Option<String>,
    /// The place in the board order (0 is first); after the last status when unset.
    position: Option<i64>,
}

/// A missing field keeps its value. The project's Duplicate status cannot change its category,
/// and no other status can take it.
#[derive(Deserialize, ToSchema)]
#[schema(as = PublicUpdateStatusBody)]
#[serde(deny_unknown_fields)]
pub(super) struct UpdateStatusBody {
    /// The `version` that you read.
    expected_version: u64,
    /// 1 to 200 characters.
    name: Option<String>,
    /// `unstarted`, `started`, `completed` or `cancelled`.
    category: Option<String>,
    /// `#rrggbb`.
    color: Option<String>,
    /// Up to 20,000 characters.
    description: Option<String>,
    position: Option<i64>,
}

#[derive(Deserialize, ToSchema)]
#[schema(as = PublicCreateLabelBody)]
#[serde(deny_unknown_fields)]
pub(super) struct CreateLabelBody {
    /// 1 to 100 characters, unique in the project.
    name: String,
    /// `#rrggbb`; gray when unset.
    color: Option<String>,
}

/// A missing field keeps its value.
#[derive(Deserialize, ToSchema)]
#[schema(as = PublicUpdateLabelBody)]
#[serde(deny_unknown_fields)]
pub(super) struct UpdateLabelBody {
    /// The `version` that you read.
    expected_version: u64,
    /// 1 to 100 characters, unique in the project.
    name: Option<String>,
    /// `#rrggbb`.
    color: Option<String>,
}

#[utoipa::path(
    patch,
    path = "/api/public/v1/workspaces/{workspace_id}/projects/{project_id}",
    operation_id = "public_update_project",
    tag = "public-api",
    summary = "Rename or recolor a project",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("project_id" = String, Path)),
    request_body = UpdateProjectBody,
    responses((status = 200, description = "The project", body = ProjectOut)),
    security(("bearerAuth" = []))
)]
pub(super) async fn update_project(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, project)): Path<(String, String)>,
    body: Result<Json<UpdateProjectBody>, JsonRejection>,
) -> Response {
    let project = path_id!(ctx, project);
    let body = input!(ctx, ApiTokenScope::TasksWrite, body);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::TasksWrite, &ctx.request_id)
            .await?;
        api.update_project(project, body.expected_version, body.name, body.color)
            .await
            .map(ProjectOut::from)
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    post,
    path = "/api/public/v1/workspaces/{workspace_id}/projects/{project_id}/statuses",
    operation_id = "public_create_status",
    tag = "public-api",
    summary = "Add a status to a project",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("project_id" = String, Path)),
    request_body = CreateStatusBody,
    responses((status = 201, description = "The new status", body = StatusOut)),
    security(("bearerAuth" = []))
)]
pub(super) async fn create_status(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, project)): Path<(String, String)>,
    body: Result<Json<CreateStatusBody>, JsonRejection>,
) -> Response {
    let project = path_id!(ctx, project);
    let body = input!(ctx, ApiTokenScope::TasksWrite, body);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::TasksWrite, &ctx.request_id)
            .await?;
        let status = StatusPatch {
            name: Some(body.name),
            category: Some(body.category),
            color: body.color,
            description: body.description,
            position: body.position,
        };
        api.create_status(project, status)
            .await
            .map(StatusOut::from)
    }
    .await;
    ctx.reply(StatusCode::CREATED, result)
}

#[utoipa::path(
    patch,
    path = "/api/public/v1/workspaces/{workspace_id}/projects/{project_id}/statuses/{status_id}",
    operation_id = "public_update_status",
    tag = "public-api",
    summary = "Change a status of a project",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("project_id" = String, Path), ("status_id" = String, Path)),
    request_body = UpdateStatusBody,
    responses((status = 200, description = "The status", body = StatusOut)),
    security(("bearerAuth" = []))
)]
pub(super) async fn update_status(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, project, status)): Path<(String, String, String)>,
    body: Result<Json<UpdateStatusBody>, JsonRejection>,
) -> Response {
    let project = path_id!(ctx, project);
    let status = path_id!(ctx, status);
    let body = input!(ctx, ApiTokenScope::TasksWrite, body);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::TasksWrite, &ctx.request_id)
            .await?;
        let patch = StatusPatch {
            name: body.name,
            category: body.category,
            color: body.color,
            description: body.description,
            position: body.position,
        };
        api.update_status(project, status, body.expected_version, patch)
            .await
            .map(StatusOut::from)
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    post,
    path = "/api/public/v1/workspaces/{workspace_id}/projects/{project_id}/labels",
    operation_id = "public_create_label",
    tag = "public-api",
    summary = "Add a label to a project",
    description = "A name that the project already has answers 400.",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("project_id" = String, Path)),
    request_body = CreateLabelBody,
    responses((status = 201, description = "The new label", body = LabelOut)),
    security(("bearerAuth" = []))
)]
pub(super) async fn create_label(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, project)): Path<(String, String)>,
    body: Result<Json<CreateLabelBody>, JsonRejection>,
) -> Response {
    let project = path_id!(ctx, project);
    let body = input!(ctx, ApiTokenScope::TasksWrite, body);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::TasksWrite, &ctx.request_id)
            .await?;
        api.create_label(project, &body.name, body.color)
            .await
            .map(LabelOut::from)
    }
    .await;
    ctx.reply(StatusCode::CREATED, result)
}

#[utoipa::path(
    patch,
    path = "/api/public/v1/workspaces/{workspace_id}/projects/{project_id}/labels/{label_id}",
    operation_id = "public_update_label",
    tag = "public-api",
    summary = "Rename or recolor a label",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("project_id" = String, Path), ("label_id" = String, Path)),
    request_body = UpdateLabelBody,
    responses((status = 200, description = "The label", body = LabelOut)),
    security(("bearerAuth" = []))
)]
pub(super) async fn update_label(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, project, label)): Path<(String, String, String)>,
    body: Result<Json<UpdateLabelBody>, JsonRejection>,
) -> Response {
    let project = path_id!(ctx, project);
    let label = path_id!(ctx, label);
    let body = input!(ctx, ApiTokenScope::TasksWrite, body);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::TasksWrite, &ctx.request_id)
            .await?;
        api.update_label(project, label, body.expected_version, body.name, body.color)
            .await
            .map(LabelOut::from)
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}
