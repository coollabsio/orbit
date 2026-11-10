//! Row template routes (spec P2 item 5): `…/databases/{database_id}/templates`. A template's
//! title, icon, body and values are edited through the page routes and `PATCH
//! …/rows/{template_id}/values`, so its editor is the row page.

use axum::Json;
use axum::extract::{Extension, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use orbit_platform::{RequestId, TimestampMillis};
use serde::Deserialize;
use utoipa::ToSchema;

use super::{DatabaseState, child_id, database_problem, name, position, scope};
use crate::repositories::databases::{
    DatabaseError, DatabaseTemplate, NewTemplate, TemplateChanges,
};
use crate::task_routes::{ApiError, ApiJson, request_id_value, validation};

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct CreateDatabaseTemplateBody {
    /// 1 to 200 characters.
    name: String,
    /// A row (or template) of this database whose title, icon, values and body the template
    /// copies; omitted: an empty template.
    from_row_id: Option<String>,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct UpdateDatabaseTemplateBody {
    name: Option<String>,
    /// Index among the templates.
    position: Option<i64>,
}

/// Adds a row template, a hidden page of the database that is not a row. At most 20 per
/// database (409 `database_template_limit`).
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/databases/{database_id}/templates", params(("workspace_id" = String, Path), ("database_id" = String, Path)), request_body = CreateDatabaseTemplateBody, responses((status = 201, body = DatabaseTemplate)))]
pub(super) async fn create_database_template(
    State(state): State<DatabaseState>,
    Path((workspace, database)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<CreateDatabaseTemplateBody>,
) -> Result<Response, ApiError> {
    let request = request_id.as_ref();
    let instance = format!("/api/v1/workspaces/{workspace}/databases/{database}/templates");
    let (workspace_id, database_id, actor_id) =
        scope(&state, &headers, &workspace, &database, &instance, request).await?;
    let from = body
        .from_row_id
        .map(|id| {
            id.parse()
                .map_err(|_| validation("from_row_id", &instance, request))
        })
        .transpose()?;
    let input = NewTemplate {
        name: name(body.name, &instance, request)?,
        from,
    };
    state
        .databases
        .create_template(
            workspace_id,
            database_id,
            actor_id,
            input,
            request_id_value(request),
            TimestampMillis::now(),
        )
        .await
        .map(|template| (StatusCode::CREATED, Json(template)).into_response())
        .map_err(|error| database_problem(error, instance, request))
}

/// Renames a template or moves it among the templates.
#[utoipa::path(patch, path = "/api/v1/workspaces/{workspace_id}/databases/{database_id}/templates/{template_id}", params(("workspace_id" = String, Path), ("database_id" = String, Path), ("template_id" = String, Path)), request_body = UpdateDatabaseTemplateBody, responses((status = 200, body = DatabaseTemplate)))]
pub(super) async fn update_database_template(
    State(state): State<DatabaseState>,
    Path((workspace, database, template)): Path<(String, String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<UpdateDatabaseTemplateBody>,
) -> Result<Json<DatabaseTemplate>, ApiError> {
    let request = request_id.as_ref();
    let instance =
        format!("/api/v1/workspaces/{workspace}/databases/{database}/templates/{template}");
    let (workspace_id, database_id, actor_id) =
        scope(&state, &headers, &workspace, &database, &instance, request).await?;
    let template_id = child_id(
        &template,
        DatabaseError::TemplateNotFound,
        &instance,
        request,
    )?;
    let changes = TemplateChanges {
        name: body
            .name
            .map(|value| name(value, &instance, request))
            .transpose()?,
        position: body
            .position
            .map(|value| position(value, &instance, request))
            .transpose()?,
    };
    state
        .databases
        .update_template(
            workspace_id,
            database_id,
            template_id,
            actor_id,
            changes,
            request_id_value(request),
            TimestampMillis::now(),
        )
        .await
        .map(Json)
        .map_err(|error| database_problem(error, instance, request))
}

/// Deletes a template and its page for good (no trash). Views that used it as their default
/// template go back to an empty row.
#[utoipa::path(delete, path = "/api/v1/workspaces/{workspace_id}/databases/{database_id}/templates/{template_id}", params(("workspace_id" = String, Path), ("database_id" = String, Path), ("template_id" = String, Path)), responses((status = 204)))]
pub(super) async fn delete_database_template(
    State(state): State<DatabaseState>,
    Path((workspace, database, template)): Path<(String, String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<StatusCode, ApiError> {
    let request = request_id.as_ref();
    let instance =
        format!("/api/v1/workspaces/{workspace}/databases/{database}/templates/{template}");
    let (workspace_id, database_id, actor_id) =
        scope(&state, &headers, &workspace, &database, &instance, request).await?;
    let template_id = child_id(
        &template,
        DatabaseError::TemplateNotFound,
        &instance,
        request,
    )?;
    state
        .databases
        .delete_template(
            workspace_id,
            database_id,
            template_id,
            actor_id,
            request_id_value(request),
            TimestampMillis::now(),
        )
        .await
        .map(|()| StatusCode::NO_CONTENT)
        .map_err(|error| database_problem(error, instance, request))
}
