//! Button press route (spec P3 item 15): `POST …/rows/{row_id}/press/{property_id}`.

use std::collections::HashMap;
use std::sync::Mutex;

use axum::Json;
use axum::extract::{Extension, Path, State};
use axum::http::{HeaderMap, StatusCode};
use orbit_platform::{Id, RequestId, TimestampMillis};
use serde::Deserialize;
use utoipa::ToSchema;

use super::{DatabaseState, child_id, database_problem, scope};
use crate::repositories::databases::{ButtonPress, DatabaseError};
use crate::task_routes::{ApiError, ApiJson, request_id_value};

const PRESSES_PER_SECOND: u32 = 10;
/// Users the limiter remembers before it forgets those of past seconds.
const MAX_TRACKED_USERS: usize = 10_000;

/// Presses per user in the current second. Kept in memory, so each server of a cluster counts
/// on its own: a press is a short write that `immediate_transaction` already serializes, and
/// this limit only stops a client from flooding it.
#[derive(Default)]
pub(super) struct PressLimiter {
    seconds: Mutex<HashMap<Id, (i64, u32)>>,
}

impl PressLimiter {
    fn admit(&self, user_id: Id, now: TimestampMillis) -> bool {
        let second = now.as_millis().div_euclid(1_000);
        let mut seconds = self
            .seconds
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if seconds.len() >= MAX_TRACKED_USERS {
            seconds.retain(|_, (at, _)| *at == second);
        }
        let entry = seconds.entry(user_id).or_insert((second, 0));
        if entry.0 != second {
            *entry = (second, 0);
        }
        entry.1 += 1;
        entry.1 <= PRESSES_PER_SECOND
    }
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct PressDatabaseButtonBody {
    /// The caller's local day (`YYYY-MM-DD`) that `"today"` values take; default: the UTC day.
    today: Option<String>,
}

/// Runs the button's server actions (`set_values`, `add_row`) in one transaction and returns
/// the pressed row, the rows made and the URL to open. A failed action rolls back every one
/// (422 names `actions[i]`). Needs access to each `add_row` target (else 404). At most 10
/// presses per user per second (429 `database_button_rate_limit`).
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/databases/{database_id}/rows/{row_id}/press/{property_id}", params(("workspace_id" = String, Path), ("database_id" = String, Path), ("row_id" = String, Path), ("property_id" = String, Path)), request_body = PressDatabaseButtonBody, responses((status = 200, body = ButtonPress)))]
pub(super) async fn press_database_button(
    State(state): State<DatabaseState>,
    Path((workspace, database, row, property)): Path<(String, String, String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<PressDatabaseButtonBody>,
) -> Result<Json<ButtonPress>, ApiError> {
    let request = request_id.as_ref();
    let instance = format!(
        "/api/v1/workspaces/{workspace}/databases/{database}/rows/{row}/press/{property}"
    );
    let (workspace_id, database_id, actor_id) =
        scope(&state, &headers, &workspace, &database, &instance, request).await?;
    let row_id = child_id(&row, DatabaseError::NotFound, &instance, request)?;
    let property_id = child_id(&property, DatabaseError::PropertyNotFound, &instance, request)?;
    let now = TimestampMillis::now();
    if !state.presses.admit(actor_id, now) {
        return Err(ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "database_button_rate_limit",
            "Too many button presses",
            "A user presses buttons at most 10 times per second. Try again in a moment.",
            instance,
            request,
        ));
    }
    state
        .databases
        .press_button(
            workspace_id,
            database_id,
            row_id,
            property_id,
            actor_id,
            body.today,
            request_id_value(request),
            now,
        )
        .await
        .map(Json)
        .map_err(|error| database_problem(error, instance, request))
}
