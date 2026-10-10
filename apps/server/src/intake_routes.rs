//! Intake routes (spec `2026-10-08-intake-triage-templates-recurring-design.md`): the triage
//! actions, task templates, recurring tasks, and task creation from a payload. Part of the task
//! router.

use axum::extract::{Extension, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use orbit_platform::{RequestId, TimestampMillis};
use serde::Deserialize;
use utoipa::ToSchema;

use crate::repositories::intake::{
    CreateRecurringTask, RecurringTaskChanges, RecurringTaskRecord, TaskPayload, TemplateRecord,
    TriageAction,
};
use crate::repositories::tasks::Page;
use crate::task_routes::{
    ApiError, ApiJson, ApiQuery, MutationQuery, TaskState, TaskUpdateResponse, message,
    optional_id, parse_id, request_id_value, scope, task_problem, text, validation,
};

pub(crate) fn routes() -> Router<TaskState> {
    Router::new()
        .route(
            "/api/v1/workspaces/{workspace_id}/tasks/from-payload",
            post(create_task_from_payload),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/tasks/{task_id}/triage",
            post(triage_task),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/projects/{project_id}/templates",
            get(list_templates).post(create_template),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/projects/{project_id}/templates/{template_id}",
            patch(update_template).delete(delete_template),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/projects/{project_id}/recurring-tasks",
            get(list_recurring_tasks).post(create_recurring_task),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/projects/{project_id}/recurring-tasks/{recurring_id}",
            patch(update_recurring_task).delete(delete_recurring_task),
        )
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct TriageBody {
    expected_version: u64,
    /// `accept` or `decline`.
    action: String,
    /// `accept` only: the status the task moves to. Absent: the first backlog status, else the
    /// first unstarted one.
    status_id: Option<String>,
    /// `decline` only: a comment that says why.
    comment: Option<String>,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct FromPayloadBody {
    project_id: String,
    payload: TaskPayload,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct TemplateBody {
    name: String,
    payload: TaskPayload,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct TemplateUpdateBody {
    expected_version: u64,
    name: String,
    payload: TaskPayload,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct RecurringTaskBody {
    payload: TaskPayload,
    /// `schedule` or `after_completion`.
    mode: String,
    every_count: i64,
    /// `day`, `week` or `month`.
    every_unit: String,
    /// The first run; now when absent.
    #[schema(value_type = Option<String>, format = DateTime)]
    starts_at: Option<TimestampMillis>,
}

/// Absent fields are unchanged.
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct RecurringTaskUpdateBody {
    expected_version: u64,
    payload: Option<TaskPayload>,
    mode: Option<String>,
    every_count: Option<i64>,
    every_unit: Option<String>,
    #[schema(value_type = Option<String>, format = DateTime)]
    starts_at: Option<TimestampMillis>,
    paused: Option<bool>,
}

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/tasks/{task_id}/triage", params(("workspace_id" = String, Path), ("task_id" = String, Path)), request_body = TriageBody, responses((status = 200, body = TaskUpdateResponse)))]
pub(crate) async fn triage_task(
    State(state): State<TaskState>,
    Path((workspace, task)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<TriageBody>,
) -> Result<Json<TaskUpdateResponse>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/tasks/{task}/triage");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let task_id = parse_id(&task, &instance, request_id.as_ref())?;
    let action = match body.action.as_str() {
        "accept" => TriageAction::Accept {
            status_id: optional_id(body.status_id, &instance, request_id.as_ref())?,
        },
        "decline" => TriageAction::Decline {
            comment: body
                .comment
                .filter(|comment| !comment.trim().is_empty())
                .map(|comment| {
                    message(
                        comment,
                        50_000,
                        50_000,
                        "comment",
                        &instance,
                        request_id.as_ref(),
                    )
                })
                .transpose()?,
        },
        _ => return Err(validation("action", &instance, request_id.as_ref())),
    };
    state
        .tasks
        .triage_task(
            workspace_id,
            task_id,
            actor_id,
            action,
            body.expected_version,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map(|outcome| {
            Json(TaskUpdateResponse {
                task: outcome.task,
                auto_closed: outcome.auto_closed,
            })
        })
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/tasks/from-payload", params(("workspace_id" = String, Path)), request_body = FromPayloadBody, responses((status = 201, body = TaskUpdateResponse)))]
pub(crate) async fn create_task_from_payload(
    State(state): State<TaskState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<FromPayloadBody>,
) -> Result<Response, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/tasks/from-payload");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let project_id = parse_id(&body.project_id, &instance, request_id.as_ref())?;
    state
        .tasks
        .create_task_from_payload(
            workspace_id,
            actor_id,
            project_id,
            &body.payload,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map(|outcome| {
            let body = TaskUpdateResponse {
                task: outcome.task,
                auto_closed: outcome.auto_closed,
            };
            (StatusCode::CREATED, Json(body)).into_response()
        })
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/projects/{project_id}/templates", params(("workspace_id" = String, Path), ("project_id" = String, Path)), responses((status = 200, body = Page<TemplateRecord>)))]
pub(crate) async fn list_templates(
    State(state): State<TaskState>,
    Path((workspace, project)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<Page<TemplateRecord>>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/projects/{project}/templates");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let project_id = parse_id(&project, &instance, request_id.as_ref())?;
    state
        .tasks
        .templates(workspace_id, project_id, actor_id)
        .await
        .map(Json)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/projects/{project_id}/templates", params(("workspace_id" = String, Path), ("project_id" = String, Path)), request_body = TemplateBody, responses((status = 201, body = TemplateRecord)))]
pub(crate) async fn create_template(
    State(state): State<TaskState>,
    Path((workspace, project)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<TemplateBody>,
) -> Result<Response, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/projects/{project}/templates");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let project_id = parse_id(&project, &instance, request_id.as_ref())?;
    let name = text(body.name, 200, 200, "name", &instance, request_id.as_ref())?;
    state
        .tasks
        .create_template(
            workspace_id,
            project_id,
            actor_id,
            name,
            body.payload,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map(|record| (StatusCode::CREATED, Json(record)).into_response())
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(patch, path = "/api/v1/workspaces/{workspace_id}/projects/{project_id}/templates/{template_id}", params(("workspace_id" = String, Path), ("project_id" = String, Path), ("template_id" = String, Path)), request_body = TemplateUpdateBody, responses((status = 200, body = TemplateRecord)))]
pub(crate) async fn update_template(
    State(state): State<TaskState>,
    Path((workspace, project, template)): Path<(String, String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<TemplateUpdateBody>,
) -> Result<Json<TemplateRecord>, ApiError> {
    let instance =
        format!("/api/v1/workspaces/{workspace}/projects/{project}/templates/{template}");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let project_id = parse_id(&project, &instance, request_id.as_ref())?;
    let template_id = parse_id(&template, &instance, request_id.as_ref())?;
    let name = text(body.name, 200, 200, "name", &instance, request_id.as_ref())?;
    state
        .tasks
        .update_template(
            workspace_id,
            project_id,
            template_id,
            actor_id,
            name,
            body.payload,
            body.expected_version,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map(Json)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(delete, path = "/api/v1/workspaces/{workspace_id}/projects/{project_id}/templates/{template_id}", params(MutationQuery, ("workspace_id" = String, Path), ("project_id" = String, Path), ("template_id" = String, Path)), responses((status = 204)))]
pub(crate) async fn delete_template(
    State(state): State<TaskState>,
    Path((workspace, project, template)): Path<(String, String, String)>,
    ApiQuery(query): ApiQuery<MutationQuery>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<StatusCode, ApiError> {
    let instance =
        format!("/api/v1/workspaces/{workspace}/projects/{project}/templates/{template}");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let project_id = parse_id(&project, &instance, request_id.as_ref())?;
    let template_id = parse_id(&template, &instance, request_id.as_ref())?;
    state
        .tasks
        .delete_template(
            workspace_id,
            project_id,
            template_id,
            actor_id,
            query.expected_version,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map(|()| StatusCode::NO_CONTENT)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/projects/{project_id}/recurring-tasks", params(("workspace_id" = String, Path), ("project_id" = String, Path)), responses((status = 200, body = Page<RecurringTaskRecord>)))]
pub(crate) async fn list_recurring_tasks(
    State(state): State<TaskState>,
    Path((workspace, project)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<Page<RecurringTaskRecord>>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/projects/{project}/recurring-tasks");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let project_id = parse_id(&project, &instance, request_id.as_ref())?;
    state
        .tasks
        .recurring_tasks(workspace_id, project_id, actor_id)
        .await
        .map(Json)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/projects/{project_id}/recurring-tasks", params(("workspace_id" = String, Path), ("project_id" = String, Path)), request_body = RecurringTaskBody, responses((status = 201, body = RecurringTaskRecord)))]
pub(crate) async fn create_recurring_task(
    State(state): State<TaskState>,
    Path((workspace, project)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<RecurringTaskBody>,
) -> Result<Response, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/projects/{project}/recurring-tasks");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let project_id = parse_id(&project, &instance, request_id.as_ref())?;
    state
        .tasks
        .create_recurring_task(
            workspace_id,
            project_id,
            actor_id,
            CreateRecurringTask {
                payload: body.payload,
                mode: body.mode,
                every_count: body.every_count,
                every_unit: body.every_unit,
                starts_at: body.starts_at,
            },
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map(|record| (StatusCode::CREATED, Json(record)).into_response())
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(patch, path = "/api/v1/workspaces/{workspace_id}/projects/{project_id}/recurring-tasks/{recurring_id}", params(("workspace_id" = String, Path), ("project_id" = String, Path), ("recurring_id" = String, Path)), request_body = RecurringTaskUpdateBody, responses((status = 200, body = RecurringTaskRecord)))]
pub(crate) async fn update_recurring_task(
    State(state): State<TaskState>,
    Path((workspace, project, recurring)): Path<(String, String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<RecurringTaskUpdateBody>,
) -> Result<Json<RecurringTaskRecord>, ApiError> {
    let instance =
        format!("/api/v1/workspaces/{workspace}/projects/{project}/recurring-tasks/{recurring}");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let project_id = parse_id(&project, &instance, request_id.as_ref())?;
    let recurring_id = parse_id(&recurring, &instance, request_id.as_ref())?;
    state
        .tasks
        .update_recurring_task(
            workspace_id,
            project_id,
            recurring_id,
            actor_id,
            RecurringTaskChanges {
                payload: body.payload,
                mode: body.mode,
                every_count: body.every_count,
                every_unit: body.every_unit,
                starts_at: body.starts_at,
                paused: body.paused,
            },
            body.expected_version,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map(Json)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}

#[utoipa::path(delete, path = "/api/v1/workspaces/{workspace_id}/projects/{project_id}/recurring-tasks/{recurring_id}", params(MutationQuery, ("workspace_id" = String, Path), ("project_id" = String, Path), ("recurring_id" = String, Path)), responses((status = 204)))]
pub(crate) async fn delete_recurring_task(
    State(state): State<TaskState>,
    Path((workspace, project, recurring)): Path<(String, String, String)>,
    ApiQuery(query): ApiQuery<MutationQuery>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<StatusCode, ApiError> {
    let instance =
        format!("/api/v1/workspaces/{workspace}/projects/{project}/recurring-tasks/{recurring}");
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, &instance, request_id.as_ref()).await?;
    let project_id = parse_id(&project, &instance, request_id.as_ref())?;
    let recurring_id = parse_id(&recurring, &instance, request_id.as_ref())?;
    state
        .tasks
        .delete_recurring_task(
            workspace_id,
            project_id,
            recurring_id,
            actor_id,
            query.expected_version,
            request_id_value(request_id.as_ref()),
            TimestampMillis::now(),
        )
        .await
        .map(|()| StatusCode::NO_CONTENT)
        .map_err(|error| task_problem(error, instance, request_id.as_ref()))
}
