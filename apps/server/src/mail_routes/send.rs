//! Send, undo and the Scheduled and Outbox lists (spec §5.3, §7). Another member's draft or
//! message is `mail_not_found`.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, Uri};
use orbit_platform::{Id, TimestampMillis};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use super::drafts::{MailDraft, view as draft_view};
use super::{Call, MailState, RequestIdExtension};
use crate::repositories::mail::{MailError, Outbound, OutboundItem, OutboundView};
use crate::task_routes::{ApiError, ApiJson, ApiQuery};

#[derive(Serialize, ToSchema)]
pub(crate) struct MailOutbound {
    #[schema(value_type = String)]
    id: Id,
    /// `held`, `cancelled`, `released` or `done`.
    status: String,
    /// When the message goes (the end of the undo time, or the scheduled time).
    #[schema(value_type = String, format = DateTime)]
    release_at: TimestampMillis,
    scheduled: bool,
    /// Why a held message waits beyond its time: `rate_limit`.
    hold_reason: Option<String>,
}

fn outbound_view(outbound: Outbound) -> MailOutbound {
    MailOutbound {
        id: outbound.id,
        status: outbound.status,
        release_at: outbound.release_at,
        scheduled: outbound.scheduled,
        hold_reason: outbound.hold_reason,
    }
}

#[derive(Serialize, ToSchema)]
pub(crate) struct MailOutboundItem {
    #[serde(flatten)]
    outbound: MailOutbound,
    #[schema(value_type = String)]
    mailbox_id: Id,
    #[schema(value_type = Option<String>)]
    draft_id: Option<Id>,
    subject: String,
    to: Vec<String>,
}

fn item_view(item: OutboundItem) -> MailOutboundItem {
    MailOutboundItem {
        outbound: outbound_view(item.outbound),
        mailbox_id: item.mailbox_id,
        draft_id: item.draft_id,
        subject: item.subject,
        to: item.to,
    }
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct SendMailDraftBody {
    /// Send at this time instead of after the undo time. Must be in the future.
    #[schema(value_type = Option<String>, format = DateTime)]
    scheduled_at: Option<TimestampMillis>,
}

#[derive(Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub(crate) struct ListMailOutboundQuery {
    /// `scheduled` or `outbox`.
    view: String,
    /// Only the messages sent from this mailbox.
    #[param(value_type = Option<String>)]
    mailbox_id: Option<String>,
}

async fn membership(state: &MailState, call: &Call<'_>) -> Result<Id, ApiError> {
    state
        .mail
        .mail_membership(call.workspace_id, call.actor_id)
        .await
        .map_err(|error| call.problem(error))
}

async fn now(state: &MailState, call: &Call<'_>) -> Result<TimestampMillis, ApiError> {
    state
        .mail
        .database()
        .database_now()
        .await
        .map_err(|error| call.problem(error.into()))
}

/// Sends a draft: it is held for the member's undo time (or until `scheduled_at`) and goes then.
/// A second call returns the same message. A message over a send limit waits in the Outbox.
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/mail/drafts/{draft_id}/send", params(("workspace_id" = String, Path), ("draft_id" = String, Path)), request_body = SendMailDraftBody, responses((status = 200, body = MailOutbound)))]
pub(crate) async fn send_mail_draft(
    State(state): State<MailState>,
    Path((workspace, draft)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiJson(body): ApiJson<SendMailDraftBody>,
) -> Result<Json<MailOutbound>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let draft_id = call.id(&draft)?;
    let membership = membership(&state, &call).await?;
    let now = now(&state, &call).await?;
    let sent = state
        .mail
        .send_draft(membership, draft_id, body.scheduled_at, now)
        .await
        .map_err(|error| call.problem(error))?;
    Ok(Json(outbound_view(sent)))
}

/// Undoes a send or cancels a scheduled one before it goes, and returns the draft. A message that
/// went already is a 409 `already_sent`.
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/mail/outbound/{outbound_id}/cancel", params(("workspace_id" = String, Path), ("outbound_id" = String, Path)), responses((status = 200, body = MailDraft)))]
pub(crate) async fn cancel_mail_outbound(
    State(state): State<MailState>,
    Path((workspace, outbound)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<Json<MailDraft>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let outbound_id = call.id(&outbound)?;
    let membership = membership(&state, &call).await?;
    let now = now(&state, &call).await?;
    let draft = state
        .mail
        .cancel_outbound(membership, outbound_id, now)
        .await
        .map_err(|error| call.problem(error))?;
    Ok(Json(draft_view(draft)))
}

/// The caller's scheduled messages, or the Outbox (held for the undo time or a limit, and going).
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/mail/outbound", params(("workspace_id" = String, Path), ListMailOutboundQuery), responses((status = 200, body = Vec<MailOutboundItem>)))]
pub(crate) async fn list_mail_outbound(
    State(state): State<MailState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiQuery(query): ApiQuery<ListMailOutboundQuery>,
) -> Result<Json<Vec<MailOutboundItem>>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let membership = membership(&state, &call).await?;
    let view = match query.view.as_str() {
        "scheduled" => OutboundView::Scheduled,
        "outbox" => OutboundView::Outbox,
        _ => return Err(call.problem(MailError::Invalid { field: "view" })),
    };
    let mailbox_id = query
        .mailbox_id
        .as_deref()
        .map(|id| call.id(id))
        .transpose()?;
    let items = state
        .mail
        .list_outbound(membership, view, mailbox_id)
        .await
        .map_err(|error| call.problem(error))?;
    Ok(Json(items.into_iter().map(item_view).collect()))
}
