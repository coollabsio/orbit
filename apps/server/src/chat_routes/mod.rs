//! Chat (`/chat`). Session-authenticated. Every write answers with the changed record and the
//! chat events it caused for the caller, so the web client updates its cache without a refetch.

pub(crate) mod conversations;
pub(crate) mod messages;
pub(crate) mod state;

use std::sync::Arc;

use axum::extract::Extension;
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::routing::{delete, get, patch, post, put};
use axum::{Json, Router};
use orbit_platform::{Id, RequestId};
use serde::Serialize;
use utoipa::ToSchema;

use crate::auth_routes::CookieMode;
use crate::repositories::chat::{ChatError, ChatEvent, ChatRepository, Written};
use crate::repositories::identity::IdentityRepository;
use crate::task_routes::{ApiError, authenticate_session, validation};

#[derive(Clone)]
pub struct ChatState {
    identity: Arc<IdentityRepository>,
    chat: Arc<ChatRepository>,
    cookie_mode: CookieMode,
}

impl ChatState {
    #[must_use]
    pub fn new(identity: Arc<IdentityRepository>, cookie_mode: CookieMode) -> Self {
        Self {
            chat: Arc::new(ChatRepository::new(identity.database().clone())),
            identity,
            cookie_mode,
        }
    }
}

pub fn chat_router(state: ChatState) -> Router {
    Router::new()
        .route(
            "/api/v1/workspaces/{workspace_id}/chat/conversations",
            get(conversations::list_chat_conversations).post(conversations::create_chat_channel),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/chat/conversations/{conversation_id}",
            patch(conversations::update_chat_channel),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/chat/conversations/{conversation_id}/archive",
            post(conversations::archive_chat_channel),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/chat/conversations/{conversation_id}/join",
            post(conversations::join_chat_channel),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/chat/conversations/{conversation_id}/leave",
            post(conversations::leave_chat_channel),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/chat/conversations/{conversation_id}/members",
            post(conversations::add_chat_members),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/chat/conversations/{conversation_id}/members/{user_id}",
            delete(conversations::remove_chat_member),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/chat/dms",
            post(conversations::open_chat_dm),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/chat/categories",
            get(conversations::list_chat_categories).post(conversations::create_chat_category),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/chat/categories/{category_id}",
            patch(conversations::rename_chat_category).delete(conversations::delete_chat_category),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/chat/move",
            post(conversations::move_chat_item),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/chat/conversations/{conversation_id}/messages",
            get(messages::list_chat_messages).post(messages::send_chat_message),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/chat/messages/{message_id}",
            patch(messages::edit_chat_message).delete(messages::delete_chat_message),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/chat/messages/{message_id}/reactions/{emoji}",
            put(messages::add_chat_reaction).delete(messages::remove_chat_reaction),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/chat/messages/{message_id}/pin",
            put(messages::pin_chat_message),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/chat/conversations/{conversation_id}/pins",
            get(messages::list_chat_pins),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/chat/conversations/{conversation_id}/threads",
            get(messages::list_chat_threads),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/chat/threads",
            get(messages::list_followed_chat_threads),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/chat/threads/{root_id}",
            get(messages::get_chat_thread),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/chat/states",
            get(state::list_chat_states),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/chat/conversations/{conversation_id}/state",
            patch(state::update_chat_state),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/chat/conversations/{conversation_id}/read",
            post(state::read_chat_conversation),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/chat/messages/{message_id}/unread",
            post(state::mark_chat_message_unread),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/chat/threads/{root_id}/read",
            post(state::read_chat_thread),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/chat/threads/{root_id}/follow",
            put(state::follow_chat_thread),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/chat/read-all",
            post(state::read_all_chat),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/chat/read-restore",
            post(state::restore_chat_read),
        )
        .with_state(state)
}

/// The changed record and the chat events that this write caused for the caller. The events
/// are the same records that the live socket sends.
#[derive(Serialize, ToSchema)]
pub(crate) struct ChatWrite<T> {
    result: T,
    events: Vec<ChatEvent>,
}

/// A write without a record of its own.
#[derive(Serialize, ToSchema)]
pub(crate) struct ChatEvents {
    events: Vec<ChatEvent>,
}

type RequestIdExtension = Option<Extension<RequestId>>;

/// One authenticated request: who calls, in which workspace, and how to report an error.
struct Call<'a> {
    workspace_id: Id,
    actor_id: Id,
    instance: String,
    request_id: Option<&'a Extension<RequestId>>,
}

impl<'a> Call<'a> {
    async fn enter(
        state: &ChatState,
        headers: &HeaderMap,
        uri: &Uri,
        workspace: &str,
        request_id: &'a RequestIdExtension,
    ) -> Result<Self, ApiError> {
        let instance = uri.path().to_owned();
        let request_id = request_id.as_ref();
        let session = authenticate_session(
            &state.identity,
            state.cookie_mode,
            headers,
            &instance,
            request_id,
        )
        .await?;
        let workspace_id = workspace
            .parse()
            .map_err(|_| problem(ChatError::NotFound, &instance, request_id))?;
        Ok(Self {
            workspace_id,
            actor_id: session.user.id,
            instance,
            request_id,
        })
    }

    /// An id from the path. One that does not parse names nothing.
    fn id(&self, value: &str) -> Result<Id, ApiError> {
        value.parse().map_err(|_| self.problem(ChatError::NotFound))
    }

    /// An id from the body.
    fn body_id(&self, value: &str, field: &'static str) -> Result<Id, ApiError> {
        value
            .parse()
            .map_err(|_| self.problem(ChatError::Invalid { field }))
    }

    fn body_ids(&self, values: &[String], field: &'static str) -> Result<Vec<Id>, ApiError> {
        values
            .iter()
            .map(|value| self.body_id(value, field))
            .collect()
    }

    fn problem(&self, error: ChatError) -> ApiError {
        problem(error, &self.instance, self.request_id)
    }

    fn read<T>(&self, result: Result<T, ChatError>) -> Result<Json<T>, ApiError> {
        result.map(Json).map_err(|error| self.problem(error))
    }

    fn write<T>(
        &self,
        result: Result<Written<T>, ChatError>,
    ) -> Result<Json<ChatWrite<T>>, ApiError> {
        let written = result.map_err(|error| self.problem(error))?;
        let events = written.events_for(self.actor_id);
        Ok(Json(ChatWrite {
            result: written.value,
            events,
        }))
    }

    fn events(&self, result: Result<Written<()>, ChatError>) -> Result<Json<ChatEvents>, ApiError> {
        let written = result.map_err(|error| self.problem(error))?;
        Ok(Json(ChatEvents {
            events: written.events_for(self.actor_id),
        }))
    }
}

fn problem(
    error: ChatError,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> ApiError {
    let (status, code, title, detail) = match error {
        ChatError::NotFound => (
            StatusCode::NOT_FOUND,
            "chat_not_found",
            "Not found",
            "This conversation or message is not available.",
        ),
        ChatError::Forbidden(detail) => (
            StatusCode::FORBIDDEN,
            "chat_forbidden",
            "Action forbidden",
            detail,
        ),
        ChatError::Conflict(detail) => (StatusCode::CONFLICT, "chat_conflict", "Conflict", detail),
        ChatError::Invalid { field } => return validation(field, instance, request_id),
        ChatError::TooLong => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "chat_message_too_long",
            "Message too long",
            "A message can have at most 4000 characters.",
        ),
        ChatError::Unavailable(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            "Internal server error",
            "An unexpected error occurred. Use the request ID when contacting support.",
        ),
    };
    ApiError::new(status, code, title, detail, instance, request_id)
}
