//! Form view route (spec P3 item 16): `POST …/databases/{database_id}/forms/{view_id}/submit`
//! adds a row from a member's answers. The form's layout is the view's config, changed with the
//! view routes.

use std::collections::BTreeMap;

use axum::extract::{Extension, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use orbit_platform::{RequestId, TimestampMillis};
use serde::Deserialize;
use serde_json::Value;
use utoipa::ToSchema;

use super::{BASE, DatabaseState, child_id, database_problem, scope};
use crate::repositories::databases::{DatabaseError, DatabaseRow, FormSubmission, MAX_PROPERTIES};
use crate::task_routes::{ApiError, ApiJson, request_id_value, validation};

pub(super) fn routes() -> Router<DatabaseState> {
    Router::new().route(
        &format!("{BASE}/forms/{{view_id}}/submit"),
        post(submit_database_form),
    )
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
#[schema(as = SubmitDatabaseFormBody)]
pub(super) struct SubmitDatabaseFormBody {
    /// Answers by property id, in the shape of row values; the title's answer is its text.
    /// Leave out (or send `null`) what the member skipped.
    answers: BTreeMap<String, Value>,
}

/// Submits a form view: a new row created by the caller, who needs only read access to the
/// database. Required questions left empty and answers to properties the form does not ask are
/// 422 `answers.<property_id>`; a closed form is 409 `database_form_closed`; more than 30
/// submissions an hour in the workspace are 429 `database_form_rate_limited`.
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/databases/{database_id}/forms/{view_id}/submit", params(("workspace_id" = String, Path), ("database_id" = String, Path), ("view_id" = String, Path)), request_body = SubmitDatabaseFormBody, responses((status = 201, body = DatabaseRow)))]
pub(super) async fn submit_database_form(
    State(state): State<DatabaseState>,
    Path((workspace, database, view)): Path<(String, String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<SubmitDatabaseFormBody>,
) -> Result<Response, ApiError> {
    let request = request_id.as_ref();
    let instance =
        format!("/api/v1/workspaces/{workspace}/databases/{database}/forms/{view}/submit");
    let (workspace_id, database_id, actor_id) =
        scope(&state, &headers, &workspace, &database, &instance, request).await?;
    let view_id = child_id(&view, DatabaseError::ViewNotFound, &instance, request)?;
    if body.answers.len() > MAX_PROPERTIES {
        return Err(validation("answers", &instance, request));
    }
    let answers = body
        .answers
        .into_iter()
        .map(|(key, value)| match key.parse() {
            Ok(id) => Ok((id, value)),
            Err(_) => {
                Err(validation("answers", &instance, request).with_detail(format!("answers.{key}")))
            }
        })
        .collect::<Result<_, _>>()?;
    let submission = FormSubmission { view_id, answers };
    state
        .databases
        .submit_form(
            workspace_id,
            database_id,
            actor_id,
            &submission,
            request_id_value(request),
            TimestampMillis::now(),
        )
        .await
        .map(|row| (StatusCode::CREATED, Json(row)).into_response())
        .map_err(|error| database_problem(error, instance, request))
}
