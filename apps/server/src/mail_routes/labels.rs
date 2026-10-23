//! Mail labels of the workspace: members list them, owners and admins change them.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, Uri};
use orbit_platform::{Id, TimestampMillis};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::{Call, MailState, RequestIdExtension};
use crate::repositories::mail::Label;
use crate::task_routes::{ApiError, ApiJson};

#[derive(Serialize, ToSchema)]
pub(crate) struct MailLabel {
    #[schema(value_type = String)]
    id: Id,
    name: String,
    /// `#rrggbb`, lower case.
    color: String,
    #[schema(value_type = String, format = DateTime)]
    created_at: TimestampMillis,
    #[schema(value_type = String, format = DateTime)]
    updated_at: TimestampMillis,
}

pub(super) fn view(label: Label) -> MailLabel {
    MailLabel {
        id: label.id,
        name: label.name,
        color: label.color,
        created_at: label.created_at,
        updated_at: label.updated_at,
    }
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct CreateMailLabelBody {
    /// 1 to 80 characters, unique in the workspace whatever the case.
    name: String,
    /// `#rrggbb`.
    color: String,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct UpdateMailLabelBody {
    name: Option<String>,
    color: Option<String>,
}

/// The mail labels of the workspace by name, for any member.
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/mail/labels", params(("workspace_id" = String, Path)), responses((status = 200, body = Vec<MailLabel>)))]
pub(crate) async fn list_mail_labels(
    State(state): State<MailState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<Json<Vec<MailLabel>>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let labels = state
        .mail
        .list_labels(call.workspace_id, call.actor_id)
        .await
        .map_err(|error| call.problem(error))?;
    Ok(Json(labels.into_iter().map(view).collect()))
}

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/mail/labels", params(("workspace_id" = String, Path)), request_body = CreateMailLabelBody, responses((status = 201, body = MailLabel)))]
pub(crate) async fn create_mail_label(
    State(state): State<MailState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiJson(body): ApiJson<CreateMailLabelBody>,
) -> Result<(StatusCode, Json<MailLabel>), ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let label = state
        .mail
        .create_label(
            call.workspace_id,
            call.actor_id,
            &body.name,
            &body.color,
            call.request_id(),
        )
        .await
        .map_err(|error| call.problem(error))?;
    Ok((StatusCode::CREATED, Json(view(label))))
}

/// Renames a label or changes its color.
#[utoipa::path(patch, path = "/api/v1/workspaces/{workspace_id}/mail/labels/{label_id}", params(("workspace_id" = String, Path), ("label_id" = String, Path)), request_body = UpdateMailLabelBody, responses((status = 200, body = MailLabel)))]
pub(crate) async fn update_mail_label(
    State(state): State<MailState>,
    Path((workspace, label)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiJson(body): ApiJson<UpdateMailLabelBody>,
) -> Result<Json<MailLabel>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let label_id = call.id(&label)?;
    let label = state
        .mail
        .update_label(
            call.workspace_id,
            call.actor_id,
            label_id,
            body.name.as_deref(),
            body.color.as_deref(),
            call.request_id(),
        )
        .await
        .map_err(|error| call.problem(error))?;
    Ok(Json(view(label)))
}

/// Deletes a label; threads lose it.
#[utoipa::path(delete, path = "/api/v1/workspaces/{workspace_id}/mail/labels/{label_id}", params(("workspace_id" = String, Path), ("label_id" = String, Path)), responses((status = 204)))]
pub(crate) async fn delete_mail_label(
    State(state): State<MailState>,
    Path((workspace, label)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<StatusCode, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let label_id = call.id(&label)?;
    state
        .mail
        .delete_label(
            call.workspace_id,
            call.actor_id,
            label_id,
            call.request_id(),
        )
        .await
        .map_err(|error| call.problem(error))?;
    Ok(StatusCode::NO_CONTENT)
}
