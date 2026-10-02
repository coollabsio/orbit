//! The caller's read state: cursors, notify level, favorites and thread follows.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, Uri};
use serde::Deserialize;
use utoipa::ToSchema;

use super::{Call, ChatEvents, ChatState, ChatWrite, RequestIdExtension};
use crate::repositories::chat::{
    ConversationCursor, ConversationStateRecord, NotifyLevel, ReadSnapshot, StateUpdate,
    ThreadCursor, ThreadStateRecord,
};
use crate::task_routes::{ApiError, ApiJson};

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ChatStateBody {
    notify: Option<NotifyLevel>,
    favorite: Option<bool>,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ChatFollowBody {
    following: bool,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ChatConversationCursorBody {
    conversation_id: String,
    last_read_message_id: Option<String>,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ChatThreadCursorBody {
    root_id: String,
    last_read_reply_id: Option<String>,
}

/// The read cursors to put back: the `states` and `threads` that `read-all` returned.
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ChatRestoreBody {
    states: Vec<ChatConversationCursorBody>,
    threads: Vec<ChatThreadCursorBody>,
}

/// The caller's state in every conversation it is a member of.
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/chat/states", params(("workspace_id" = String, Path)), responses((status = 200, body = Vec<ConversationStateRecord>)))]
pub(crate) async fn list_chat_states(
    State(state): State<ChatState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<Json<Vec<ConversationStateRecord>>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    call.read(
        state
            .chat
            .list_states(call.workspace_id, call.actor_id)
            .await,
    )
}

#[utoipa::path(patch, path = "/api/v1/workspaces/{workspace_id}/chat/conversations/{conversation_id}/state", params(("workspace_id" = String, Path), ("conversation_id" = String, Path)), request_body = ChatStateBody, responses((status = 200, body = ChatWrite<ConversationStateRecord>)))]
pub(crate) async fn update_chat_state(
    State(state): State<ChatState>,
    Path((workspace, conversation)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiJson(body): ApiJson<ChatStateBody>,
) -> Result<Json<ChatWrite<ConversationStateRecord>>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let conversation_id = call.id(&conversation)?;
    let update = StateUpdate {
        notify: body.notify,
        favorite: body.favorite,
    };
    call.write(
        &state,
        state
            .chat
            .update_state(call.workspace_id, call.actor_id, conversation_id, update),
    )
    .await
}

/// Reads the conversation up to its newest message.
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/chat/conversations/{conversation_id}/read", params(("workspace_id" = String, Path), ("conversation_id" = String, Path)), responses((status = 200, body = ChatWrite<ConversationStateRecord>)))]
pub(crate) async fn read_chat_conversation(
    State(state): State<ChatState>,
    Path((workspace, conversation)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<Json<ChatWrite<ConversationStateRecord>>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let conversation_id = call.id(&conversation)?;
    call.write(
        &state,
        state
            .chat
            .mark_read(call.workspace_id, call.actor_id, conversation_id),
    )
    .await
}

/// Moves the read cursor to just before the message. For a thread reply the thread's cursor
/// moves and the caller follows the thread.
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/chat/messages/{message_id}/unread", params(("workspace_id" = String, Path), ("message_id" = String, Path)), responses((status = 200, body = ChatWrite<ConversationStateRecord>)))]
pub(crate) async fn mark_chat_message_unread(
    State(state): State<ChatState>,
    Path((workspace, message)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<Json<ChatWrite<ConversationStateRecord>>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let message_id = call.id(&message)?;
    call.write(
        &state,
        state
            .chat
            .mark_unread(call.workspace_id, call.actor_id, message_id),
    )
    .await
}

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/chat/threads/{root_id}/read", params(("workspace_id" = String, Path), ("root_id" = String, Path)), responses((status = 200, body = ChatWrite<ThreadStateRecord>)))]
pub(crate) async fn read_chat_thread(
    State(state): State<ChatState>,
    Path((workspace, root)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<Json<ChatWrite<ThreadStateRecord>>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let root_id = call.id(&root)?;
    call.write(
        &state,
        state
            .chat
            .mark_thread_read(call.workspace_id, call.actor_id, root_id),
    )
    .await
}

/// Following starts from now: older replies do not turn unread.
#[utoipa::path(put, path = "/api/v1/workspaces/{workspace_id}/chat/threads/{root_id}/follow", params(("workspace_id" = String, Path), ("root_id" = String, Path)), request_body = ChatFollowBody, responses((status = 200, body = ChatWrite<ThreadStateRecord>)))]
pub(crate) async fn follow_chat_thread(
    State(state): State<ChatState>,
    Path((workspace, root)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiJson(body): ApiJson<ChatFollowBody>,
) -> Result<Json<ChatWrite<ThreadStateRecord>>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let root_id = call.id(&root)?;
    call.write(
        &state,
        state
            .chat
            .set_thread_follow(call.workspace_id, call.actor_id, root_id, body.following),
    )
    .await
}

/// Reads every conversation and followed thread. `result` holds the states as they were, for
/// `read-restore`.
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/chat/read-all", params(("workspace_id" = String, Path)), responses((status = 200, body = ChatWrite<ReadSnapshot>)))]
pub(crate) async fn read_all_chat(
    State(state): State<ChatState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<Json<ChatWrite<ReadSnapshot>>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    call.write(
        &state,
        state.chat.mark_all_read(call.workspace_id, call.actor_id),
    )
    .await
}

/// Puts read cursors back and counts again from them: the undo of `read-all`.
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/chat/read-restore", params(("workspace_id" = String, Path)), request_body = ChatRestoreBody, responses((status = 200, body = ChatEvents)))]
pub(crate) async fn restore_chat_read(
    State(state): State<ChatState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiJson(body): ApiJson<ChatRestoreBody>,
) -> Result<Json<ChatEvents>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let optional = |value: &Option<String>, field| {
        value
            .as_deref()
            .map(|value| call.body_id(value, field))
            .transpose()
    };
    let states = body
        .states
        .iter()
        .map(|cursor| {
            Ok(ConversationCursor {
                conversation_id: call.body_id(&cursor.conversation_id, "states")?,
                last_read_message_id: optional(&cursor.last_read_message_id, "states")?,
            })
        })
        .collect::<Result<Vec<_>, ApiError>>()?;
    let threads = body
        .threads
        .iter()
        .map(|cursor| {
            Ok(ThreadCursor {
                root_id: call.body_id(&cursor.root_id, "threads")?,
                last_read_reply_id: optional(&cursor.last_read_reply_id, "threads")?,
            })
        })
        .collect::<Result<Vec<_>, ApiError>>()?;
    call.events(
        &state,
        state
            .chat
            .restore_read(call.workspace_id, call.actor_id, states, threads),
    )
    .await
}
