//! Chat reads (`chat:read`): conversations, messages, threads and search. Chat has no write
//! endpoint; a bot that writes uses a chat webhook.
use axum::extract::rejection::QueryRejection;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Response;
use orbit_platform::Id;
use serde::Deserialize;
use utoipa::IntoParams;

use super::{Ctx, List, PublicApiState, list, problem_response};
use crate::repositories::api_tokens::ApiTokenScope;
use crate::repositories::chat::{
    ConversationRecord, MessagePage, SearchInput, SearchPage, ThreadPage,
};
use crate::token_api::TokenApiError;

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
#[serde(deny_unknown_fields)]
pub(super) struct MessagesQuery {
    /// The `before` of the previous response: older messages. The newest messages when unset.
    #[param(value_type = Option<String>)]
    before: Option<Id>,
    /// 1 to 100.
    #[param(minimum = 1, maximum = 100)]
    limit: Option<usize>,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
#[serde(deny_unknown_fields)]
pub(super) struct SearchQuery {
    /// Every word must match; the last one matches as a prefix. Up to 500 characters; may be
    /// empty with a filter.
    #[serde(default)]
    q: String,
    /// Only this conversation.
    #[param(value_type = Option<String>)]
    conversation_id: Option<Id>,
    /// Only messages of this member.
    #[param(value_type = Option<String>)]
    author_id: Option<Id>,
    /// Only messages with a file.
    #[serde(default)]
    has_file: bool,
    /// The `cursor` of the previous page: older messages.
    #[param(value_type = Option<String>)]
    cursor: Option<Id>,
}

#[utoipa::path(
    get,
    path = "/api/public/v1/workspaces/{workspace_id}/chat/conversations",
    operation_id = "public_list_conversations",
    tag = "public-api",
    summary = "The chat conversations that the token's creator can see",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`.")),
    responses((status = 200, description = "Every visible conversation; `next_cursor` is `null`", body = List<ConversationRecord>)),
    security(("bearerAuth" = []))
)]
pub(super) async fn conversations(State(s): State<PublicApiState>, ctx: Ctx) -> Response {
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::ChatRead, &ctx.request_id)
            .await?;
        Ok(list::<_, ConversationRecord>(
            api.conversations().await?,
            None,
        ))
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    get,
    path = "/api/public/v1/workspaces/{workspace_id}/chat/conversations/{conversation_id}/messages",
    operation_id = "public_list_messages",
    tag = "public-api",
    summary = "A page of messages of a conversation, oldest first",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("conversation_id" = String, Path), MessagesQuery),
    responses((status = 200, description = "A page of messages; `before` is `null` when there is nothing older", body = MessagePage)),
    security(("bearerAuth" = []))
)]
pub(super) async fn messages(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, conversation)): Path<(String, String)>,
    query: Result<Query<MessagesQuery>, QueryRejection>,
) -> Response {
    let conversation = path_id!(ctx, conversation);
    let query = input!(ctx, ApiTokenScope::ChatRead, query);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::ChatRead, &ctx.request_id)
            .await?;
        api.messages(conversation, query.before, query.limit).await
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    get,
    path = "/api/public/v1/workspaces/{workspace_id}/chat/threads/{message_id}",
    operation_id = "public_get_thread",
    tag = "public-api",
    summary = "A message thread: its root message and a page of replies, oldest first",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("message_id" = String, Path, description = "The thread's root message."), MessagesQuery),
    responses((status = 200, description = "The thread", body = ThreadPage)),
    security(("bearerAuth" = []))
)]
pub(super) async fn thread(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, message)): Path<(String, String)>,
    query: Result<Query<MessagesQuery>, QueryRejection>,
) -> Response {
    let message = path_id!(ctx, message);
    let query = input!(ctx, ApiTokenScope::ChatRead, query);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::ChatRead, &ctx.request_id)
            .await?;
        api.thread(message, query.before, query.limit).await
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    get,
    path = "/api/public/v1/workspaces/{workspace_id}/chat/search",
    operation_id = "public_search_messages",
    tag = "public-api",
    summary = "Search messages in the visible conversations, newest first, 20 per page",
    description = "Without words, a filter is needed: the result is then the newest messages that pass it.",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), SearchQuery),
    responses((status = 200, description = "A page of hits; `cursor` is `null` on the last page", body = SearchPage)),
    security(("bearerAuth" = []))
)]
pub(super) async fn search(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    query: Result<Query<SearchQuery>, QueryRejection>,
) -> Response {
    let query = input!(ctx, ApiTokenScope::ChatRead, query);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::ChatRead, &ctx.request_id)
            .await?;
        api.search_messages(SearchInput {
            query: query.q,
            conversation_id: query.conversation_id,
            author_id: query.author_id,
            has_file: query.has_file,
            cursor: query.cursor,
        })
        .await
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}
