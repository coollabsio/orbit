//! Formula preview (spec P2/P3 15.10): `POST …/databases/{database_id}/formula/preview` checks an
//! expression as the editor types it and computes it on the first rows. Formulas run only on the
//! server (D-P3-3), so this is how clients see a formula before they save it.

use axum::extract::{Extension, Path, State};
use axum::http::HeaderMap;
use axum::routing::post;
use axum::{Json, Router};
use orbit_platform::RequestId;
use serde::Deserialize;
use utoipa::{IntoParams, ToSchema};

use super::{BASE, DatabaseState, database_problem, scope};
use crate::repositories::databases::{ComputeScope, FormulaPreview};
use crate::task_routes::{ApiError, ApiJson, ApiQuery, validation};

pub(super) fn routes() -> Router<DatabaseState> {
    Router::new().route(
        &format!("{BASE}/formula/preview"),
        post(preview_database_formula),
    )
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
#[schema(as = DatabaseFormulaPreviewBody)]
pub(crate) struct FormulaPreviewBody {
    /// The expression; `prop("…")` takes a property name or id.
    expression: String,
    /// The formula property being edited (so it cannot read itself); omitted for a new one.
    property_id: Option<String>,
}

#[derive(Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub(crate) struct FormulaPreviewQuery {
    /// The reader's IANA time zone for date functions; default `UTC`.
    tz: Option<String>,
}

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/databases/{database_id}/formula/preview", params(("workspace_id" = String, Path), ("database_id" = String, Path), FormulaPreviewQuery), request_body = FormulaPreviewBody, responses((status = 200, body = FormulaPreview)))]
pub(crate) async fn preview_database_formula(
    State(state): State<DatabaseState>,
    Path((workspace, database)): Path<(String, String)>,
    ApiQuery(query): ApiQuery<FormulaPreviewQuery>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<FormulaPreviewBody>,
) -> Result<Json<FormulaPreview>, ApiError> {
    let request = request_id.as_ref();
    let instance = format!("/api/v1/workspaces/{workspace}/databases/{database}/formula/preview");
    let (workspace_id, database_id, actor_id) =
        scope(&state, &headers, &workspace, &database, &instance, request).await?;
    let compute = ComputeScope::with_tz(workspace_id, actor_id, query.tz.as_deref())
        .ok_or_else(|| validation("tz", &instance, request))?;
    let property_id = body
        .property_id
        .map(|id| {
            id.parse()
                .map_err(|_| validation("property_id", &instance, request))
        })
        .transpose()?;
    state
        .databases
        .preview_formula(&compute, database_id, property_id, &body.expression)
        .await
        .map(Json)
        .map_err(|error| database_problem(error, instance, request))
}
