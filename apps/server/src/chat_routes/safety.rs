//! Reports of chat messages and the caller's blocked people (App Store guideline 1.2).

use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, Uri};
use orbit_platform::Id;
use serde::Deserialize;
use utoipa::ToSchema;

use super::{Call, ChatEvents, ChatState, RequestIdExtension, problem};
use crate::mail::templates;
use crate::repositories::chat::{BlockedUserRecord, ChatError, ChatReportRecord, ReportReason};
use crate::task_routes::{ApiError, ApiJson, authenticate_session};

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ChatReportBody {
    reason: ReportReason,
    /// At most 500 characters.
    #[serde(default)]
    note: String,
}

/// Reports a message that the caller can read and did not send. Workspace owners and admins get
/// an inbox notification, and the installation admins an email when a mail server is set.
/// Reporting a message again returns the first report.
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/chat/messages/{message_id}/report", params(("workspace_id" = String, Path), ("message_id" = String, Path)), request_body = ChatReportBody, responses((status = 200, body = ChatReportRecord)))]
pub(crate) async fn report_chat_message(
    State(state): State<ChatState>,
    Path((workspace, message)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiJson(body): ApiJson<ChatReportBody>,
) -> Result<Json<ChatReportRecord>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let message_id = call.id(&message)?;
    let (written, _) = state
        .publish(
            call.workspace_id,
            call.actor_id,
            state.chat.report_message(
                call.workspace_id,
                call.actor_id,
                message_id,
                body.reason,
                &body.note,
            ),
        )
        .await
        .map_err(|error| call.problem(error))?;
    let (report, created) = written.value;
    if created {
        email_admins(&state, &report).await;
    }
    Ok(Json(report))
}

/// The email is a courtesy on top of the inbox: a failure is logged, the report stays.
async fn email_admins(state: &ChatState, report: &ChatReportRecord) {
    let Some(mailer) = &state.mailer else {
        return;
    };
    match mailer.enabled().await {
        Ok(true) => {}
        Ok(false) => return,
        Err(error) => {
            tracing::warn!(%error, "mail settings for a chat report");
            return;
        }
    }
    let mail = match state.chat.report_mail(report.workspace_id).await {
        Ok(mail) => mail,
        Err(error) => {
            tracing::warn!(%error, "recipients of a chat report email");
            return;
        }
    };
    let url = mailer.link("/settings/reports");
    for email in &mail.emails {
        mailer.send_later(templates::chat_report(
            email,
            &mail.workspace_name,
            report.reason.as_str(),
            &report.body,
            &url,
        ));
    }
}

/// The workspace's reports, newest first (at most 200). Only workspace owners and admins.
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/chat/reports", params(("workspace_id" = String, Path)), responses((status = 200, body = Vec<ChatReportRecord>)))]
pub(crate) async fn list_chat_reports(
    State(state): State<ChatState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<Json<Vec<ChatReportRecord>>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    call.read(
        state
            .chat
            .list_reports(call.workspace_id, call.actor_id)
            .await,
    )
}

/// Deletes the reported message, also in a conversation that the caller cannot read. Only
/// workspace owners and admins. A message that is gone already changes nothing.
#[utoipa::path(delete, path = "/api/v1/workspaces/{workspace_id}/chat/reports/{report_id}/message", params(("workspace_id" = String, Path), ("report_id" = String, Path)), responses((status = 200, body = ChatEvents)))]
pub(crate) async fn delete_reported_chat_message(
    State(state): State<ChatState>,
    Path((workspace, report)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<Json<ChatEvents>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let report_id = call.id(&report)?;
    call.events(
        &state,
        state
            .chat
            .delete_reported_message(call.workspace_id, call.actor_id, report_id),
    )
    .await
}

/// The caller's session, for the calls that belong to no workspace.
async fn session_user(
    state: &ChatState,
    headers: &HeaderMap,
    instance: &str,
    request_id: &RequestIdExtension,
) -> Result<Id, ApiError> {
    Ok(authenticate_session(
        &state.identity,
        state.cookie_mode,
        headers,
        instance,
        request_id.as_ref(),
    )
    .await?
    .user
    .id)
}

/// The people the caller blocked, newest first. Chat hides their messages for the caller.
#[utoipa::path(get, path = "/api/v1/chat/blocks", responses((status = 200, body = Vec<BlockedUserRecord>)))]
pub(crate) async fn list_chat_blocks(
    State(state): State<ChatState>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<Json<Vec<BlockedUserRecord>>, ApiError> {
    let instance = uri.path();
    let user_id = session_user(&state, &headers, instance, &request_id).await?;
    state
        .chat
        .list_blocks(user_id)
        .await
        .map(Json)
        .map_err(|error| problem(error, instance, request_id.as_ref()))
}

/// Blocks a person who shares a workspace with the caller: chat hides their messages for the
/// caller, and they cannot open or write a DM with the caller. Blocking again changes nothing.
#[utoipa::path(put, path = "/api/v1/chat/blocks/{user_id}", params(("user_id" = String, Path)), responses((status = 204)))]
pub(crate) async fn block_chat_user(
    State(state): State<ChatState>,
    Path(target): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<StatusCode, ApiError> {
    let instance = uri.path();
    let user_id = session_user(&state, &headers, instance, &request_id).await?;
    let fail = |error| problem(error, instance, request_id.as_ref());
    let target_id: Id = target.parse().map_err(|_| fail(ChatError::NotFound))?;
    state
        .chat
        .block_user(user_id, target_id)
        .await
        .map_err(fail)?;
    Ok(StatusCode::NO_CONTENT)
}

/// Removes a block. Removing one that does not exist changes nothing.
#[utoipa::path(delete, path = "/api/v1/chat/blocks/{user_id}", params(("user_id" = String, Path)), responses((status = 204)))]
pub(crate) async fn unblock_chat_user(
    State(state): State<ChatState>,
    Path(target): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<StatusCode, ApiError> {
    let instance = uri.path();
    let user_id = session_user(&state, &headers, instance, &request_id).await?;
    let fail = |error| problem(error, instance, request_id.as_ref());
    let target_id: Id = target.parse().map_err(|_| fail(ChatError::NotFound))?;
    state
        .chat
        .unblock_user(user_id, target_id)
        .await
        .map_err(fail)?;
    Ok(StatusCode::NO_CONTENT)
}
