//! Messages, threads, reactions and pins.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, Uri};
use serde::Deserialize;
use utoipa::{IntoParams, ToSchema};

use super::{Call, ChatEvents, ChatState, ChatWrite, RequestIdExtension};
use crate::repositories::chat::{
    ChatError, FollowedThreadRecord, MessageCursor, MessagePage, MessageRecord, SendInput,
    ThreadPage,
};
use crate::task_routes::{ApiError, ApiJson, ApiQuery};

/// At most one of `before`, `after` and `around`. Without one: the newest page.
#[derive(Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub(crate) struct ChatMessageQuery {
    /// Messages older than this message id.
    before: Option<String>,
    /// Messages newer than this message id.
    after: Option<String>,
    /// A window with this message in the middle.
    around: Option<String>,
    /// 1–100; default 50.
    limit: Option<usize>,
}

impl ChatMessageQuery {
    fn cursor(&self, call: &Call<'_>) -> Result<MessageCursor, ApiError> {
        let given = [&self.before, &self.after, &self.around]
            .into_iter()
            .flatten()
            .count();
        if given > 1 {
            return Err(call.problem(ChatError::Invalid { field: "cursor" }));
        }
        let id = |value: &Option<String>| {
            value
                .as_deref()
                .map(|value| call.body_id(value, "cursor"))
                .transpose()
        };
        Ok(MessageCursor {
            before: id(&self.before)?,
            after: id(&self.after)?,
            around: id(&self.around)?,
            limit: self.limit,
        })
    }
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ChatSendBody {
    /// Markdown source, at most 4000 characters. Mentions are tokens: `<@user_id>`,
    /// `<#conversation_id>`, `<!channel>`, `<!here>`.
    body: String,
    /// The root message, for a thread reply.
    thread_root_id: Option<String>,
    /// A thread reply that also shows in the conversation.
    #[serde(default)]
    #[schema(required = false)]
    also_in_channel: bool,
    /// Made by the caller (1–64 bytes). A send that is tried again with the same nonce returns
    /// the first message.
    nonce: String,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ChatEditBody {
    body: String,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ChatPinBody {
    pinned: bool,
}

/// The main list of a conversation: root messages, system rows and "also in channel" replies.
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/chat/conversations/{conversation_id}/messages", params(ChatMessageQuery, ("workspace_id" = String, Path), ("conversation_id" = String, Path)), responses((status = 200, body = MessagePage)))]
pub(crate) async fn list_chat_messages(
    State(state): State<ChatState>,
    Path((workspace, conversation)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiQuery(query): ApiQuery<ChatMessageQuery>,
) -> Result<Json<MessagePage>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let conversation_id = call.id(&conversation)?;
    let cursor = query.cursor(&call)?;
    call.read(
        state
            .chat
            .list_messages(call.workspace_id, call.actor_id, conversation_id, cursor)
            .await,
    )
}

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/chat/conversations/{conversation_id}/messages", params(("workspace_id" = String, Path), ("conversation_id" = String, Path)), request_body = ChatSendBody, responses((status = 200, body = ChatWrite<MessageRecord>)))]
pub(crate) async fn send_chat_message(
    State(state): State<ChatState>,
    Path((workspace, conversation)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiJson(body): ApiJson<ChatSendBody>,
) -> Result<Json<ChatWrite<MessageRecord>>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let input = SendInput {
        conversation_id: call.id(&conversation)?,
        thread_root_id: body
            .thread_root_id
            .as_deref()
            .map(|id| call.body_id(id, "thread_root_id"))
            .transpose()?,
        body: body.body,
        also_in_channel: body.also_in_channel,
        nonce: body.nonce,
    };
    call.write(
        state
            .chat
            .send_message(call.workspace_id, call.actor_id, input)
            .await,
    )
}

#[utoipa::path(patch, path = "/api/v1/workspaces/{workspace_id}/chat/messages/{message_id}", params(("workspace_id" = String, Path), ("message_id" = String, Path)), request_body = ChatEditBody, responses((status = 200, body = ChatWrite<MessageRecord>)))]
pub(crate) async fn edit_chat_message(
    State(state): State<ChatState>,
    Path((workspace, message)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiJson(body): ApiJson<ChatEditBody>,
) -> Result<Json<ChatWrite<MessageRecord>>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let message_id = call.id(&message)?;
    call.write(
        state
            .chat
            .edit_message(call.workspace_id, call.actor_id, message_id, &body.body)
            .await,
    )
}

/// A root that has replies stays in the list with an empty body; every other message is removed.
#[utoipa::path(delete, path = "/api/v1/workspaces/{workspace_id}/chat/messages/{message_id}", params(("workspace_id" = String, Path), ("message_id" = String, Path)), responses((status = 200, body = ChatEvents)))]
pub(crate) async fn delete_chat_message(
    State(state): State<ChatState>,
    Path((workspace, message)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<Json<ChatEvents>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let message_id = call.id(&message)?;
    call.events(
        state
            .chat
            .delete_message(call.workspace_id, call.actor_id, message_id)
            .await,
    )
}

#[utoipa::path(put, path = "/api/v1/workspaces/{workspace_id}/chat/messages/{message_id}/reactions/{emoji}", params(("workspace_id" = String, Path), ("message_id" = String, Path), ("emoji" = String, Path)), responses((status = 200, body = ChatWrite<MessageRecord>)))]
pub(crate) async fn add_chat_reaction(
    State(state): State<ChatState>,
    Path((workspace, message, emoji)): Path<(String, String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<Json<ChatWrite<MessageRecord>>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let message_id = call.id(&message)?;
    call.write(
        state
            .chat
            .set_reaction(call.workspace_id, call.actor_id, message_id, &emoji, true)
            .await,
    )
}

#[utoipa::path(delete, path = "/api/v1/workspaces/{workspace_id}/chat/messages/{message_id}/reactions/{emoji}", params(("workspace_id" = String, Path), ("message_id" = String, Path), ("emoji" = String, Path)), responses((status = 200, body = ChatWrite<MessageRecord>)))]
pub(crate) async fn remove_chat_reaction(
    State(state): State<ChatState>,
    Path((workspace, message, emoji)): Path<(String, String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<Json<ChatWrite<MessageRecord>>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let message_id = call.id(&message)?;
    call.write(
        state
            .chat
            .set_reaction(call.workspace_id, call.actor_id, message_id, &emoji, false)
            .await,
    )
}

/// Pinning adds a system row to the conversation.
#[utoipa::path(put, path = "/api/v1/workspaces/{workspace_id}/chat/messages/{message_id}/pin", params(("workspace_id" = String, Path), ("message_id" = String, Path)), request_body = ChatPinBody, responses((status = 200, body = ChatWrite<MessageRecord>)))]
pub(crate) async fn pin_chat_message(
    State(state): State<ChatState>,
    Path((workspace, message)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiJson(body): ApiJson<ChatPinBody>,
) -> Result<Json<ChatWrite<MessageRecord>>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let message_id = call.id(&message)?;
    call.write(
        state
            .chat
            .set_pinned(call.workspace_id, call.actor_id, message_id, body.pinned)
            .await,
    )
}

/// Pinned messages, newest message first (at most 100).
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/chat/conversations/{conversation_id}/pins", params(("workspace_id" = String, Path), ("conversation_id" = String, Path)), responses((status = 200, body = Vec<MessageRecord>)))]
pub(crate) async fn list_chat_pins(
    State(state): State<ChatState>,
    Path((workspace, conversation)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<Json<Vec<MessageRecord>>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let conversation_id = call.id(&conversation)?;
    call.read(
        state
            .chat
            .list_pins(call.workspace_id, call.actor_id, conversation_id)
            .await,
    )
}

/// Root messages that have replies, newest reply first (at most 100).
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/chat/conversations/{conversation_id}/threads", params(("workspace_id" = String, Path), ("conversation_id" = String, Path)), responses((status = 200, body = Vec<MessageRecord>)))]
pub(crate) async fn list_chat_threads(
    State(state): State<ChatState>,
    Path((workspace, conversation)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<Json<Vec<MessageRecord>>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let conversation_id = call.id(&conversation)?;
    call.read(
        state
            .chat
            .list_threads(call.workspace_id, call.actor_id, conversation_id)
            .await,
    )
}

/// The threads the caller follows: unread first, then newest reply first (at most 200).
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/chat/threads", params(("workspace_id" = String, Path)), responses((status = 200, body = Vec<FollowedThreadRecord>)))]
pub(crate) async fn list_followed_chat_threads(
    State(state): State<ChatState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<Json<Vec<FollowedThreadRecord>>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    call.read(
        state
            .chat
            .list_followed_threads(call.workspace_id, call.actor_id)
            .await,
    )
}

/// A root message, a page of its replies and the caller's state in the thread.
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/chat/threads/{root_id}", params(ChatMessageQuery, ("workspace_id" = String, Path), ("root_id" = String, Path)), responses((status = 200, body = ThreadPage)))]
pub(crate) async fn get_chat_thread(
    State(state): State<ChatState>,
    Path((workspace, root)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiQuery(query): ApiQuery<ChatMessageQuery>,
) -> Result<Json<ThreadPage>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let root_id = call.id(&root)?;
    let cursor = query.cursor(&call)?;
    call.read(
        state
            .chat
            .get_thread(call.workspace_id, call.actor_id, root_id, cursor)
            .await,
    )
}
