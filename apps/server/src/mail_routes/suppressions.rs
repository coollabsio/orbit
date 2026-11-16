//! The suppression list (managers) and **Retry** of a recipient that was not delivered
//! (spec §5.7).

use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, Uri};
use orbit_platform::{Id, TimestampMillis};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use super::{Call, MailState, RequestIdExtension};
use crate::repositories::mail::{BounceKind, RecipientStatus, Suppression};
use crate::task_routes::{ApiError, ApiQuery};

#[derive(Serialize, ToSchema)]
pub(crate) struct MailSuppression {
    /// Lower case.
    address: String,
    /// `hard_bounce`, `complaint` or `manual`.
    reason: String,
    /// The bounce's response or the complaint.
    detail: Option<String>,
    /// The sent recipient that bounced.
    #[schema(value_type = Option<String>)]
    recipient_id: Option<Id>,
    #[schema(value_type = String, format = DateTime)]
    created_at: TimestampMillis,
}

fn suppression_view(entry: Suppression) -> MailSuppression {
    MailSuppression {
        address: entry.address,
        reason: entry.reason.as_str().to_owned(),
        detail: entry.detail,
        recipient_id: entry.recipient_id,
        created_at: entry.created_at,
    }
}

#[derive(Serialize, ToSchema)]
pub(crate) struct MailSuppressionPage {
    items: Vec<MailSuppression>,
    /// Pass as `cursor` for the next page; null on the last one.
    cursor: Option<String>,
}

#[derive(Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub(crate) struct ListMailSuppressionsQuery {
    /// Only addresses that contain this text.
    q: Option<String>,
    cursor: Option<String>,
}

/// The delivery status of one recipient of a sent message.
#[derive(Serialize, ToSchema)]
pub(crate) struct MailOutboundRecipient {
    #[schema(value_type = String)]
    id: Id,
    address: String,
    /// `to`, `cc` or `bcc`.
    kind: String,
    /// `queued`, `sending`, `delivered`, `deferred`, `bounced`, `suppressed` or `cancelled`.
    status: String,
    attempts: i64,
    #[schema(value_type = Option<String>, format = DateTime)]
    next_attempt_at: Option<TimestampMillis>,
    last_response: Option<String>,
    /// Deferred for more than 4 hours.
    delayed: bool,
    /// Why a bounced recipient bounced: `hard`, `rejected`, `gave_up`, `route` or `unreadable`.
    bounce_kind: Option<String>,
}

fn recipient_view(status: RecipientStatus) -> MailOutboundRecipient {
    MailOutboundRecipient {
        id: status.id,
        address: status.address,
        kind: status.kind,
        status: status.status,
        attempts: status.attempts,
        next_attempt_at: status.next_attempt_at,
        last_response: status.last_response,
        delayed: status.delayed,
        bounce_kind: status
            .bounce_kind
            .map(|kind| BounceKind::as_str(kind).to_owned()),
    }
}

async fn now(state: &MailState, call: &Call<'_>) -> Result<TimestampMillis, ApiError> {
    state
        .mail
        .database()
        .database_now()
        .await
        .map_err(|error| call.problem(error.into()))
}

/// The workspace's suppression list, newest first (owners and admins).
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/mail/suppressions", params(("workspace_id" = String, Path), ListMailSuppressionsQuery), responses((status = 200, body = MailSuppressionPage)))]
pub(crate) async fn list_mail_suppressions(
    State(state): State<MailState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiQuery(query): ApiQuery<ListMailSuppressionsQuery>,
) -> Result<Json<MailSuppressionPage>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let page = state
        .mail
        .list_suppressions(
            call.workspace_id,
            call.actor_id,
            query.q.as_deref(),
            query.cursor.as_deref(),
        )
        .await
        .map_err(|error| call.problem(error))?;
    Ok(Json(MailSuppressionPage {
        items: page.items.into_iter().map(suppression_view).collect(),
        cursor: page.cursor,
    }))
}

/// Removes an address from the suppression list (owners and admins); mail to it goes again.
#[utoipa::path(delete, path = "/api/v1/workspaces/{workspace_id}/mail/suppressions/{address}", params(("workspace_id" = String, Path), ("address" = String, Path)), responses((status = 204)))]
pub(crate) async fn delete_mail_suppression(
    State(state): State<MailState>,
    Path((workspace, address)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<StatusCode, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let now = now(&state, &call).await?;
    state
        .mail
        .remove_suppression(
            call.workspace_id,
            call.actor_id,
            &address,
            call.request_id(),
            now,
        )
        .await
        .map_err(|error| call.problem(error))?;
    Ok(StatusCode::NO_CONTENT)
}

/// **Retry** of a recipient that was not delivered (bounced or suppressed), by the message's
/// author or a member of its mailbox: the address leaves the suppression list and the recipient
/// is sent again.
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/mail/outbound/{outbound_id}/recipients/{recipient_id}/retry", params(("workspace_id" = String, Path), ("outbound_id" = String, Path), ("recipient_id" = String, Path)), responses((status = 200, body = MailOutboundRecipient)))]
pub(crate) async fn retry_mail_recipient(
    State(state): State<MailState>,
    Path((workspace, outbound, recipient)): Path<(String, String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<Json<MailOutboundRecipient>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let outbound_id = call.id(&outbound)?;
    let recipient_id = call.id(&recipient)?;
    let membership = state
        .mail
        .mail_membership(call.workspace_id, call.actor_id)
        .await
        .map_err(|error| call.problem(error))?;
    let now = now(&state, &call).await?;
    let status = state
        .mail
        .retry_recipient(
            call.workspace_id,
            membership,
            outbound_id,
            recipient_id,
            call.request_id(),
            now,
        )
        .await
        .map_err(|error| call.problem(error))?;
    Ok(Json(recipient_view(status)))
}
