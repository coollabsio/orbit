//! Channels, DMs, their members, and categories.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, Uri};
use serde::Deserialize;
use utoipa::ToSchema;

use super::{Call, ChatEvents, ChatState, ChatWrite, RequestIdExtension};
use crate::repositories::chat::{
    CategoryRecord, ChannelCreate, ChannelUpdate, ConversationKind, ConversationRecord,
};
use crate::task_routes::{ApiError, ApiJson, deserialize_source_patch};

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ChatChannelCreateBody {
    /// Stored trimmed, in lower case, with `-` for spaces; at most 80 characters.
    name: String,
    /// At most 250 characters.
    #[serde(default)]
    #[schema(required = false)]
    topic: String,
    category_id: Option<String>,
    /// `public` or `private`.
    kind: ConversationKind,
    /// Workspace members to add besides the caller.
    #[serde(default)]
    #[schema(required = false)]
    member_ids: Vec<String>,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ChatChannelUpdateBody {
    name: Option<String>,
    topic: Option<String>,
    /// Absent: unchanged. `null`: no category.
    #[serde(default, deserialize_with = "deserialize_source_patch")]
    category_id: Option<Option<String>>,
    /// `public` or `private`.
    kind: Option<ConversationKind>,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ChatUserIdsBody {
    user_ids: Vec<String>,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ChatCategoryBody {
    /// 1–80 characters after trimming.
    name: String,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ChatMoveBody {
    category_id: String,
    /// The category to put it before; null for the end.
    before_id: Option<String>,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ChatPlaceBody {
    /// The category to put the channel in; null for the channels without a category.
    category_id: Option<String>,
    /// The channel of that category to put it before; null for the end.
    before_id: Option<String>,
}

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/chat/conversations", params(("workspace_id" = String, Path)), responses((status = 200, body = Vec<ConversationRecord>)))]
pub(crate) async fn list_chat_conversations(
    State(state): State<ChatState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<Json<Vec<ConversationRecord>>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    call.read(
        state
            .chat
            .list_conversations(call.workspace_id, call.actor_id)
            .await,
    )
}

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/chat/conversations", params(("workspace_id" = String, Path)), request_body = ChatChannelCreateBody, responses((status = 200, body = ChatWrite<ConversationRecord>)))]
pub(crate) async fn create_chat_channel(
    State(state): State<ChatState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiJson(body): ApiJson<ChatChannelCreateBody>,
) -> Result<Json<ChatWrite<ConversationRecord>>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let input = ChannelCreate {
        name: body.name,
        topic: body.topic,
        category_id: body
            .category_id
            .as_deref()
            .map(|id| call.body_id(id, "category_id"))
            .transpose()?,
        kind: body.kind,
        member_ids: call.body_ids(&body.member_ids, "member_ids")?,
    };
    call.write(
        &state,
        state
            .chat
            .create_channel(call.workspace_id, call.actor_id, input),
    )
    .await
}

#[utoipa::path(patch, path = "/api/v1/workspaces/{workspace_id}/chat/conversations/{conversation_id}", params(("workspace_id" = String, Path), ("conversation_id" = String, Path)), request_body = ChatChannelUpdateBody, responses((status = 200, body = ChatWrite<ConversationRecord>)))]
pub(crate) async fn update_chat_channel(
    State(state): State<ChatState>,
    Path((workspace, conversation)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiJson(body): ApiJson<ChatChannelUpdateBody>,
) -> Result<Json<ChatWrite<ConversationRecord>>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let conversation_id = call.id(&conversation)?;
    let category_id = match body.category_id {
        Some(Some(id)) => Some(Some(call.body_id(&id, "category_id")?)),
        Some(None) => Some(None),
        None => None,
    };
    let update = ChannelUpdate {
        name: body.name,
        topic: body.topic,
        category_id,
        kind: body.kind,
    };
    call.write(
        &state,
        state
            .chat
            .update_channel(call.workspace_id, call.actor_id, conversation_id, update),
    )
    .await
}

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/chat/conversations/{conversation_id}/archive", params(("workspace_id" = String, Path), ("conversation_id" = String, Path)), responses((status = 200, body = ChatWrite<ConversationRecord>)))]
pub(crate) async fn archive_chat_channel(
    State(state): State<ChatState>,
    Path((workspace, conversation)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<Json<ChatWrite<ConversationRecord>>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let conversation_id = call.id(&conversation)?;
    call.write(
        &state,
        state
            .chat
            .archive_channel(call.workspace_id, call.actor_id, conversation_id),
    )
    .await
}

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/chat/conversations/{conversation_id}/join", params(("workspace_id" = String, Path), ("conversation_id" = String, Path)), responses((status = 200, body = ChatWrite<ConversationRecord>)))]
pub(crate) async fn join_chat_channel(
    State(state): State<ChatState>,
    Path((workspace, conversation)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<Json<ChatWrite<ConversationRecord>>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let conversation_id = call.id(&conversation)?;
    call.write(
        &state,
        state
            .chat
            .join_channel(call.workspace_id, call.actor_id, conversation_id),
    )
    .await
}

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/chat/conversations/{conversation_id}/leave", params(("workspace_id" = String, Path), ("conversation_id" = String, Path)), responses((status = 200, body = ChatEvents)))]
pub(crate) async fn leave_chat_channel(
    State(state): State<ChatState>,
    Path((workspace, conversation)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<Json<ChatEvents>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let conversation_id = call.id(&conversation)?;
    call.events(
        &state,
        state
            .chat
            .leave_channel(call.workspace_id, call.actor_id, conversation_id),
    )
    .await
}

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/chat/conversations/{conversation_id}/members", params(("workspace_id" = String, Path), ("conversation_id" = String, Path)), request_body = ChatUserIdsBody, responses((status = 200, body = ChatWrite<ConversationRecord>)))]
pub(crate) async fn add_chat_members(
    State(state): State<ChatState>,
    Path((workspace, conversation)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiJson(body): ApiJson<ChatUserIdsBody>,
) -> Result<Json<ChatWrite<ConversationRecord>>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let conversation_id = call.id(&conversation)?;
    let user_ids = call.body_ids(&body.user_ids, "user_ids")?;
    call.write(
        &state,
        state
            .chat
            .add_channel_members(call.workspace_id, call.actor_id, conversation_id, user_ids),
    )
    .await
}

#[utoipa::path(delete, path = "/api/v1/workspaces/{workspace_id}/chat/conversations/{conversation_id}/members/{user_id}", params(("workspace_id" = String, Path), ("conversation_id" = String, Path), ("user_id" = String, Path)), responses((status = 200, body = ChatWrite<ConversationRecord>)))]
pub(crate) async fn remove_chat_member(
    State(state): State<ChatState>,
    Path((workspace, conversation, user)): Path<(String, String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<Json<ChatWrite<ConversationRecord>>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let conversation_id = call.id(&conversation)?;
    let user_id = call.id(&user)?;
    call.write(
        &state,
        state.chat.remove_channel_member(
            call.workspace_id,
            call.actor_id,
            conversation_id,
            user_id,
        ),
    )
    .await
}

/// Returns the DM of exactly these members and the caller, or makes it.
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/chat/dms", params(("workspace_id" = String, Path)), request_body = ChatUserIdsBody, responses((status = 200, body = ChatWrite<ConversationRecord>)))]
pub(crate) async fn open_chat_dm(
    State(state): State<ChatState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiJson(body): ApiJson<ChatUserIdsBody>,
) -> Result<Json<ChatWrite<ConversationRecord>>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let user_ids = call.body_ids(&body.user_ids, "user_ids")?;
    call.write(
        &state,
        state
            .chat
            .open_dm(call.workspace_id, call.actor_id, user_ids),
    )
    .await
}

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/chat/categories", params(("workspace_id" = String, Path)), responses((status = 200, body = Vec<CategoryRecord>)))]
pub(crate) async fn list_chat_categories(
    State(state): State<ChatState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<Json<Vec<CategoryRecord>>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    call.read(
        state
            .chat
            .list_categories(call.workspace_id, call.actor_id)
            .await,
    )
}

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/chat/categories", params(("workspace_id" = String, Path)), request_body = ChatCategoryBody, responses((status = 200, body = ChatWrite<CategoryRecord>)))]
pub(crate) async fn create_chat_category(
    State(state): State<ChatState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiJson(body): ApiJson<ChatCategoryBody>,
) -> Result<Json<ChatWrite<CategoryRecord>>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    call.write(
        &state,
        state
            .chat
            .create_category(call.workspace_id, call.actor_id, &body.name),
    )
    .await
}

#[utoipa::path(patch, path = "/api/v1/workspaces/{workspace_id}/chat/categories/{category_id}", params(("workspace_id" = String, Path), ("category_id" = String, Path)), request_body = ChatCategoryBody, responses((status = 200, body = ChatWrite<CategoryRecord>)))]
pub(crate) async fn rename_chat_category(
    State(state): State<ChatState>,
    Path((workspace, category)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiJson(body): ApiJson<ChatCategoryBody>,
) -> Result<Json<ChatWrite<CategoryRecord>>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let category_id = call.id(&category)?;
    call.write(
        &state,
        state
            .chat
            .rename_category(call.workspace_id, call.actor_id, category_id, &body.name),
    )
    .await
}

/// The category's channels move to the channels without a category.
#[utoipa::path(delete, path = "/api/v1/workspaces/{workspace_id}/chat/categories/{category_id}", params(("workspace_id" = String, Path), ("category_id" = String, Path)), responses((status = 200, body = ChatEvents)))]
pub(crate) async fn delete_chat_category(
    State(state): State<ChatState>,
    Path((workspace, category)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<Json<ChatEvents>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let category_id = call.id(&category)?;
    call.events(
        &state,
        state
            .chat
            .delete_category(call.workspace_id, call.actor_id, category_id),
    )
    .await
}

/// Puts a category before another category or at the end (drag and drop).
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/chat/move", params(("workspace_id" = String, Path)), request_body = ChatMoveBody, responses((status = 200, body = ChatEvents)))]
pub(crate) async fn move_chat_item(
    State(state): State<ChatState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiJson(body): ApiJson<ChatMoveBody>,
) -> Result<Json<ChatEvents>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let category_id = call.body_id(&body.category_id, "category_id")?;
    let before_id = body
        .before_id
        .as_deref()
        .map(|id| call.body_id(id, "before_id"))
        .transpose()?;
    call.events(
        &state,
        state
            .chat
            .place_category(call.workspace_id, call.actor_id, category_id, before_id),
    )
    .await
}

/// Puts a channel into a category, before another channel or at the end (drag and drop).
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/chat/conversations/{conversation_id}/place", params(("workspace_id" = String, Path), ("conversation_id" = String, Path)), request_body = ChatPlaceBody, responses((status = 200, body = ChatEvents)))]
pub(crate) async fn place_chat_channel(
    State(state): State<ChatState>,
    Path((workspace, conversation)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiJson(body): ApiJson<ChatPlaceBody>,
) -> Result<Json<ChatEvents>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let conversation_id = call.id(&conversation)?;
    let category_id = body
        .category_id
        .as_deref()
        .map(|id| call.body_id(id, "category_id"))
        .transpose()?;
    let before_id = body
        .before_id
        .as_deref()
        .map(|id| call.body_id(id, "before_id"))
        .transpose()?;
    call.events(
        &state,
        state.chat.place_channel(
            call.workspace_id,
            call.actor_id,
            conversation_id,
            category_id,
            before_id,
        ),
    )
    .await
}
