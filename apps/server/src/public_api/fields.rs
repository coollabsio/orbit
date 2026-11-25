//! Custom task fields (#87): the field gallery, a project's fields and a task's values. The rules
//! (what a token limited to some projects may see and change) are in `token_api::fields`.
use axum::Json;
use axum::Router;
use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, put};
use orbit_platform::Id;
use serde::Deserialize;
use serde_json::Value;
use utoipa::ToSchema;

use super::{Ctx, ExpectedVersionQuery, List, PublicApiState, problem_response};
use crate::repositories::api_tokens::ApiTokenScope;
use crate::repositories::tasks::{
    TaskFieldKind, TaskFieldOptionInput, TaskFieldRecord, TaskFieldValue,
};
use crate::token_api::TokenApiError;

pub(super) fn routes() -> Router<PublicApiState> {
    Router::new()
        .route("/task-fields", get(task_fields).post(create_task_field))
        .route(
            "/task-fields/{field_id}",
            patch(update_task_field).delete(delete_task_field),
        )
        .route(
            "/projects/{project_id}/fields",
            get(project_task_fields).put(set_project_task_fields),
        )
        .route("/tasks/{task}/fields", get(task_field_values))
        .route("/tasks/{task}/fields/{field_id}", put(set_task_field_value))
}

#[derive(Deserialize, ToSchema)]
#[schema(as = PublicCreateTaskFieldBody)]
#[serde(deny_unknown_fields)]
pub(super) struct CreateTaskFieldBody {
    /// 1 to 100 characters, unique in the workspace (case-insensitive).
    name: String,
    /// It never changes after creation.
    kind: TaskFieldKind,
    /// The choices of a `select` or `multi_select` field (up to 100, each with a unique name and
    /// a `#rrggbb` color); leave it out for other kinds. Each option gets an `id`.
    #[serde(default)]
    #[schema(required = false)]
    options: Vec<TaskFieldOptionInput>,
}

/// A missing field keeps its value.
#[derive(Deserialize, ToSchema)]
#[schema(as = PublicUpdateTaskFieldBody)]
#[serde(deny_unknown_fields)]
pub(super) struct UpdateTaskFieldBody {
    /// The `version` that you read.
    expected_version: u64,
    /// 1 to 100 characters, unique in the workspace (case-insensitive).
    name: Option<String>,
    /// Replaces every option. Send an option's `id` to keep it and the values that use it; an
    /// option without an `id` is new. Values of a removed option stop showing it.
    options: Option<Vec<TaskFieldOptionInput>>,
}

#[derive(Deserialize, ToSchema)]
#[schema(as = PublicProjectTaskFieldsBody)]
#[serde(deny_unknown_fields)]
pub(super) struct ProjectTaskFieldsBody {
    /// Every gallery field that the project shows, in order; a field left out is removed from
    /// the project (its values stay). `[]` removes all.
    #[schema(value_type = Vec<String>)]
    field_ids: Vec<Id>,
}

#[derive(Deserialize, ToSchema)]
#[schema(as = PublicTaskFieldValueBody)]
#[serde(deny_unknown_fields)]
pub(super) struct TaskFieldValueBody {
    /// By the field's kind: a string for `text` and `url`, a number for `number`, `YYYY-MM-DD`
    /// for `date`, an option `id` for `select`, a list of option ids for `multi_select`, `true`
    /// for `checkbox`. `null`, `""`, `false` and `[]` clear the value.
    #[schema(value_type = Value)]
    value: Value,
}

fn no_content(ctx: &Ctx, result: Result<(), TokenApiError>) -> Response {
    match result {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => ctx.error(error),
    }
}

#[utoipa::path(
    get,
    path = "/api/public/v1/workspaces/{workspace_id}/task-fields",
    operation_id = "public_list_task_fields",
    tag = "public-api",
    summary = "The workspace's task field gallery, by name",
    description = "Every custom task field of the workspace, also for a token limited to some projects. `project_count` counts only the token's approved projects that show the field.",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`.")),
    responses((status = 200, description = "Every field; `next_cursor` is `null`", body = List<TaskFieldRecord>)),
    security(("bearerAuth" = []))
)]
pub(super) async fn task_fields(State(s): State<PublicApiState>, ctx: Ctx) -> Response {
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::TasksRead, &ctx.request_id)
            .await?;
        let fields = api.task_fields().await?;
        Ok(super::list::<_, TaskFieldRecord>(fields, None))
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    post,
    path = "/api/public/v1/workspaces/{workspace_id}/task-fields",
    operation_id = "public_create_task_field",
    tag = "public-api",
    summary = "Add a field to the gallery",
    description = "No project shows the new field until `PUT …/projects/{project_id}/fields` adds it. A name that the workspace already has answers `409`. Not for guests.",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`.")),
    request_body = CreateTaskFieldBody,
    responses((status = 201, description = "The new field", body = TaskFieldRecord)),
    security(("bearerAuth" = []))
)]
pub(super) async fn create_task_field(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    body: Result<Json<CreateTaskFieldBody>, JsonRejection>,
) -> Response {
    let body = input!(ctx, ApiTokenScope::TasksWrite, body);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::TasksWrite, &ctx.request_id)
            .await?;
        api.create_task_field(&body.name, body.kind, body.options)
            .await
    }
    .await;
    ctx.reply(StatusCode::CREATED, result)
}

#[utoipa::path(
    patch,
    path = "/api/public/v1/workspaces/{workspace_id}/task-fields/{field_id}",
    operation_id = "public_update_task_field",
    tag = "public-api",
    summary = "Rename a gallery field or replace its options",
    description = "The kind never changes. Needs a token for all projects (`403` otherwise), and the field's creator or a workspace owner or admin (`403`).",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("field_id" = String, Path)),
    request_body = UpdateTaskFieldBody,
    responses((status = 200, description = "The field", body = TaskFieldRecord)),
    security(("bearerAuth" = []))
)]
pub(super) async fn update_task_field(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, field)): Path<(String, String)>,
    body: Result<Json<UpdateTaskFieldBody>, JsonRejection>,
) -> Response {
    let field = path_id!(ctx, field);
    let body = input!(ctx, ApiTokenScope::TasksWrite, body);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::TasksWrite, &ctx.request_id)
            .await?;
        api.update_task_field(field, body.expected_version, body.name, body.options)
            .await
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    delete,
    path = "/api/public/v1/workspaces/{workspace_id}/task-fields/{field_id}",
    operation_id = "public_delete_task_field",
    tag = "public-api",
    summary = "Delete a gallery field",
    description = "Removes it from every project and deletes every task's value of it. Needs a token for all projects (`403` otherwise), and the field's creator or a workspace owner or admin (`403`).",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("field_id" = String, Path), ExpectedVersionQuery),
    responses((status = 204, description = "The field is deleted")),
    security(("bearerAuth" = []))
)]
pub(super) async fn delete_task_field(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, field)): Path<(String, String)>,
    query: Result<Query<ExpectedVersionQuery>, QueryRejection>,
) -> Response {
    let field = path_id!(ctx, field);
    let query = input!(ctx, ApiTokenScope::TasksWrite, query);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::TasksWrite, &ctx.request_id)
            .await?;
        api.delete_task_field(field, query.expected_version).await
    }
    .await;
    no_content(&ctx, result)
}

#[utoipa::path(
    get,
    path = "/api/public/v1/workspaces/{workspace_id}/projects/{project_id}/fields",
    operation_id = "public_list_project_task_fields",
    tag = "public-api",
    summary = "The fields that a project shows, in its order",
    description = "A task's value can be set only for these fields.",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("project_id" = String, Path)),
    responses((status = 200, description = "Every field of the project; `next_cursor` is `null`", body = List<TaskFieldRecord>)),
    security(("bearerAuth" = []))
)]
pub(super) async fn project_task_fields(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, project)): Path<(String, String)>,
) -> Response {
    let project = path_id!(ctx, project);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::TasksRead, &ctx.request_id)
            .await?;
        let fields = api.project_task_fields(project).await?;
        Ok(super::list::<_, TaskFieldRecord>(fields, None))
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    put,
    path = "/api/public/v1/workspaces/{workspace_id}/projects/{project_id}/fields",
    operation_id = "public_set_project_task_fields",
    tag = "public-api",
    summary = "Set the fields that a project shows and their order",
    description = "Tasks keep their values of a field that the project stops showing. The token's creator must be allowed to manage the project (`403`).",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("project_id" = String, Path)),
    request_body = ProjectTaskFieldsBody,
    responses((status = 200, description = "The project's fields, in order; `next_cursor` is `null`", body = List<TaskFieldRecord>)),
    security(("bearerAuth" = []))
)]
pub(super) async fn set_project_task_fields(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, project)): Path<(String, String)>,
    body: Result<Json<ProjectTaskFieldsBody>, JsonRejection>,
) -> Response {
    let project = path_id!(ctx, project);
    let body = input!(ctx, ApiTokenScope::TasksWrite, body);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::TasksWrite, &ctx.request_id)
            .await?;
        let fields = api
            .set_project_task_fields(project, &body.field_ids)
            .await?;
        Ok(super::list::<_, TaskFieldRecord>(fields, None))
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    get,
    path = "/api/public/v1/workspaces/{workspace_id}/tasks/{task}/fields",
    operation_id = "public_list_task_field_values",
    tag = "public-api",
    summary = "A task's custom field values",
    description = "One item per field with a value, also of fields that the task's project no longer shows. A field without a value is left out.",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("task" = String, Path, description = "The task's UUID or identifier, such as `ENG-12`.")),
    responses((status = 200, description = "The values; `next_cursor` is `null`", body = List<TaskFieldValue>)),
    security(("bearerAuth" = []))
)]
pub(super) async fn task_field_values(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, task)): Path<(String, String)>,
) -> Response {
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::TasksRead, &ctx.request_id)
            .await?;
        let values = api.task_field_values(&task).await?;
        Ok(super::list::<_, TaskFieldValue>(values, None))
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    put,
    path = "/api/public/v1/workspaces/{workspace_id}/tasks/{task}/fields/{field_id}",
    operation_id = "public_set_task_field_value",
    tag = "public-api",
    summary = "Set or clear a task's value of a field",
    description = "The field must be one that the task's project shows (`400` otherwise). A value that does not fit the field's kind answers `400`.",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("task" = String, Path, description = "The task's UUID or identifier, such as `ENG-12`."), ("field_id" = String, Path)),
    request_body = TaskFieldValueBody,
    responses((status = 200, description = "All the task's values; `next_cursor` is `null`", body = List<TaskFieldValue>)),
    security(("bearerAuth" = []))
)]
pub(super) async fn set_task_field_value(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, task, field)): Path<(String, String, String)>,
    body: Result<Json<TaskFieldValueBody>, JsonRejection>,
) -> Response {
    let field = path_id!(ctx, field);
    let body = input!(ctx, ApiTokenScope::TasksWrite, body);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::TasksWrite, &ctx.request_id)
            .await?;
        let values = api.set_task_field_value(&task, field, body.value).await?;
        Ok(super::list::<_, TaskFieldValue>(values, None))
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}
