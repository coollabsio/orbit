//! Chat (`/chat`). Session-authenticated. Every write answers with the changed record and the
//! chat events it caused for the caller, so the web client updates its cache without a refetch.

pub(crate) mod conversations;
pub(crate) mod files;
pub(crate) mod messages;
pub(crate) mod state;

use std::sync::Arc;

use axum::extract::{Extension, Request};
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post, put};
use axum::{Json, Router};
use orbit_platform::{Id, RequestId, UploadService};
use serde::Serialize;
use tracing::Instrument;
use utoipa::ToSchema;

use crate::auth_routes::CookieMode;
use crate::live::{LiveHub, Recipients};
use crate::repositories::chat::{ChatError, ChatEvent, ChatRepository, Written};
use crate::repositories::identity::IdentityRepository;
use crate::task_routes::{ApiError, authenticate_session, validation};

#[derive(Clone)]
pub struct ChatState {
    pub(crate) identity: Arc<IdentityRepository>,
    pub(crate) chat: Arc<ChatRepository>,
    pub(crate) hub: LiveHub,
    pub(crate) cookie_mode: CookieMode,
}

impl ChatState {
    #[must_use]
    pub fn new(
        identity: Arc<IdentityRepository>,
        uploads: UploadService,
        cookie_mode: CookieMode,
    ) -> Self {
        Self {
            chat: Arc::new(ChatRepository::new(identity.database().clone(), uploads)),
            hub: LiveHub::of(identity.database()),
            identity,
            cookie_mode,
        }
    }

    /// Runs a write and sends its events to the live sockets. The hub's write lock is held
    /// from before the write's transaction until the events are out, so the events of a
    /// workspace are published in commit order. The caller runs in a task of its own
    /// ([`detached`]), so nothing stops this between the commit and the events.
    async fn publish<T>(
        &self,
        workspace_id: Id,
        actor_id: Id,
        write: impl Future<Output = Result<Written<T>, ChatError>>,
    ) -> Result<(Written<T>, u64), ChatError> {
        let _order = self.hub.write_lock(workspace_id).await;
        let written = write.await?;
        for emitted in &written.events {
            self.hub
                .publish(workspace_id, &emitted.recipients, TOPIC, &emitted.event);
        }
        // The number of the write's last event. A client that has seen it on its socket knows
        // that the response is not news, and that it must not put it over newer events.
        let seq = self.hub.seq(workspace_id);
        if !written.inbox.is_empty() {
            self.hub.signal(
                workspace_id,
                &Recipients::Users(written.inbox.clone()),
                "inbox",
                &serde_json::json!({ "type": "changed" }),
            );
        }
        // The write changed counters of other members. Their states are read here, for the
        // members who are connected only, so a send costs the same in a channel of any size.
        let others: Vec<Id> = self
            .hub
            .online(workspace_id)
            .into_iter()
            .filter(|user_id| *user_id != actor_id)
            .collect();
        if let Some(conversation_id) = written.counted {
            match self.chat.states_for(conversation_id, &others).await {
                Ok(states) => {
                    for (user_id, state) in states {
                        self.hub.publish(
                            workspace_id,
                            &Recipients::Users(vec![user_id]),
                            TOPIC,
                            &ChatEvent::StateChanged { state },
                        );
                    }
                }
                Err(error) => tracing::warn!(error = %error, "chat states for the live socket"),
            }
        }
        if let Some(root_id) = written.replied {
            match self.chat.thread_states_for(root_id, &others).await {
                Ok(states) => {
                    for (user_id, state) in states {
                        self.hub.publish(
                            workspace_id,
                            &Recipients::Users(vec![user_id]),
                            TOPIC,
                            &ChatEvent::ThreadChanged { state },
                        );
                    }
                }
                Err(error) => tracing::warn!(error = %error, "thread states for the live socket"),
            }
        }
        Ok((written, seq))
    }
}

/// The topic of chat events on the live socket.
const TOPIC: &str = "chat";

pub fn chat_router(state: ChatState) -> Router {
    files::file_router(state.clone()).merge(json_router(state))
}

fn json_router(state: ChatState) -> Router {
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
            "/api/v1/workspaces/{workspace_id}/chat/conversations/{conversation_id}/place",
            post(conversations::place_chat_channel),
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
            "/api/v1/workspaces/{workspace_id}/chat/search",
            get(messages::list_chat_search),
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
        .layer(middleware::from_fn(detached))
        .with_state(state)
}

/// Runs a write in a task of its own. axum drops the future of a request when the client
/// disconnects; a write that stopped after its commit would never publish its events. The
/// task holds the hub's write lock and runs to the end, whoever waits for it.
async fn detached(request: Request, next: Next) -> Response {
    if request.method() == Method::GET {
        return next.run(request).await;
    }
    let instance = request.uri().path().to_owned();
    let request_id = request
        .extensions()
        .get::<RequestId>()
        .cloned()
        .map(Extension);
    match tokio::spawn(next.run(request).in_current_span()).await {
        Ok(response) => response,
        Err(error) => {
            tracing::error!(error = %error, "chat write task");
            problem(
                ChatError::Unavailable(sqlx::Error::WorkerCrashed),
                &instance,
                request_id.as_ref(),
            )
            .into_response()
        }
    }
}

/// The changed record and the chat events that this write caused for the caller. The events
/// are the same records that the live socket sends; `seq` is the socket's number of the last
/// one, so a client that got a later event on its socket leaves these out.
#[derive(Serialize, ToSchema)]
pub(crate) struct ChatWrite<T> {
    result: T,
    events: Vec<ChatEvent>,
    seq: u64,
}

/// A write without a record of its own.
#[derive(Serialize, ToSchema)]
pub(crate) struct ChatEvents {
    events: Vec<ChatEvent>,
    seq: u64,
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

    async fn write<T>(
        &self,
        state: &ChatState,
        write: impl Future<Output = Result<Written<T>, ChatError>>,
    ) -> Result<Json<ChatWrite<T>>, ApiError> {
        let (written, seq) = state
            .publish(self.workspace_id, self.actor_id, write)
            .await
            .map_err(|error| self.problem(error))?;
        let events = written.events_for(self.actor_id);
        Ok(Json(ChatWrite {
            result: written.value,
            events,
            seq,
        }))
    }

    async fn events(
        &self,
        state: &ChatState,
        write: impl Future<Output = Result<Written<()>, ChatError>>,
    ) -> Result<Json<ChatEvents>, ApiError> {
        let (written, seq) = state
            .publish(self.workspace_id, self.actor_id, write)
            .await
            .map_err(|error| self.problem(error))?;
        Ok(Json(ChatEvents {
            events: written.events_for(self.actor_id),
            seq,
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
        ChatError::Upload(error) => return files::upload_problem(error, instance, request_id),
        ChatError::Unavailable(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            "Internal server error",
            "An unexpected error occurred. Use the request ID when contacting support.",
        ),
    };
    ApiError::new(status, code, title, detail, instance, request_id)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    use axum::body::Body;
    use tower::ServiceExt;

    use super::*;

    #[tokio::test]
    async fn a_write_ends_when_its_request_is_dropped() {
        let done = Arc::new(AtomicBool::new(false));
        let app = Router::new()
            .route(
                "/write",
                post({
                    let done = Arc::clone(&done);
                    || async move {
                        tokio::time::sleep(Duration::from_millis(60)).await;
                        done.store(true, Ordering::SeqCst);
                    }
                }),
            )
            .layer(middleware::from_fn(detached));
        let request = Request::post("/write").body(Body::empty()).unwrap();
        // The client goes away while the write runs.
        assert!(
            tokio::time::timeout(Duration::from_millis(20), app.oneshot(request))
                .await
                .is_err()
        );
        tokio::time::sleep(Duration::from_millis(120)).await;
        assert!(done.load(Ordering::SeqCst));
    }
}
