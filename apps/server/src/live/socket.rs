//! `GET /api/v1/workspaces/{workspace_id}/live?epoch=<epoch>&after=<seq>`: the live socket.
//!
//! Before the upgrade the platform layer requires an allowed `Origin`. A request without a
//! session, or from someone who is not a member of a live workspace, is upgraded and closed at
//! once with 4401 or 4403: a browser shows an HTTP status at the handshake as close code 1006,
//! which the client cannot tell from a network failure. After it the connection is checked
//! again every 15 seconds, so a removed member, a suspended account and an ended session lose
//! it.
//!
//! The only frame a client may send is `{"type":"typing","conversation_id":…,"thread_root_id":…}`.

use std::time::{Duration, Instant};

use axum::Router;
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use orbit_platform::Id;
use serde::Deserialize;
use serde_json::json;

use crate::auth_routes::request_session;
use crate::chat_routes::ChatState;
use crate::collab::close;
use crate::realtime::authorized;

const HEARTBEAT: Duration = Duration::from_secs(25);
const RECHECK: Duration = Duration::from_secs(15);
const SEND_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_FRAME_BYTES: usize = 1024;
/// Frames a client may send in one second. The web client sends one every 8 seconds for each
/// composer.
const FRAMES_PER_SECOND: u32 = 5;

pub fn live_router(state: ChatState) -> Router {
    Router::new()
        .route("/api/v1/workspaces/{workspace_id}/live", get(connect))
        .with_state(state)
}

#[derive(Deserialize)]
struct Resume {
    /// The `epoch` of the last `hello` the client got.
    epoch: Option<String>,
    /// The `seq` of the last event the client got.
    after: Option<u64>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum ClientFrame {
    Typing {
        conversation_id: Id,
        #[serde(default)]
        thread_root_id: Option<Id>,
    },
}

async fn connect(
    State(state): State<ChatState>,
    Path(workspace): Path<Id>,
    Query(resume): Query<Resume>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    let Some(session) = request_session(&state.identity, state.cookie_mode, &headers).await else {
        return refuse(upgrade, close::SESSION, "no session");
    };
    let database = state.identity.database().clone();
    match authorized(&database, workspace, session.id).await {
        Ok(true) => {}
        Ok(false) => return refuse(upgrade, close::FORBIDDEN, "no access"),
        // The database could not say: the client must try again, not give up.
        Err(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
    upgrade
        .max_message_size(MAX_FRAME_BYTES)
        .max_frame_size(MAX_FRAME_BYTES)
        .on_upgrade(move |socket| {
            serve(
                socket,
                state,
                workspace,
                session.id,
                session.user.id,
                resume,
            )
        })
}

/// Accepts the upgrade and closes the socket with `code`; nothing else is sent.
fn refuse(upgrade: WebSocketUpgrade, code: u16, reason: &'static str) -> Response {
    upgrade.on_upgrade(move |mut socket| async move {
        let frame = CloseFrame {
            code,
            reason: reason.into(),
        };
        let _ = send(&mut socket, Message::Close(Some(frame))).await;
    })
}

async fn serve(
    mut socket: WebSocket,
    state: ChatState,
    workspace_id: Id,
    session_id: Id,
    user_id: Id,
    resume: Resume,
) {
    let database = state.identity.database().clone();
    let mut attached =
        state
            .hub
            .connect(workspace_id, user_id, resume.epoch.as_deref(), resume.after);
    let connection_id = attached.id;
    let close = run(
        &mut socket,
        &state,
        &database,
        workspace_id,
        session_id,
        user_id,
        &mut attached,
    )
    .await;
    state.hub.disconnect(workspace_id, connection_id, user_id);
    let frame = close.map(|(code, reason)| CloseFrame {
        code,
        reason: reason.into(),
    });
    let _ = send(&mut socket, Message::Close(frame)).await;
}

/// Serves one connection until it ends; returns the close code to send, if any.
async fn run(
    socket: &mut WebSocket,
    state: &ChatState,
    database: &orbit_platform::Database,
    workspace_id: Id,
    session_id: Id,
    user_id: Id,
    attached: &mut super::hub::Attached,
) -> Option<(u16, &'static str)> {
    if !send_text(socket, attached.hello.clone()).await {
        return None;
    }
    match attached.replay.take() {
        Some(missed) => {
            for frame in missed {
                if !send_text(socket, frame.to_string()).await {
                    return None;
                }
            }
        }
        None => {
            if !send_text(socket, json!({ "type": "resync" }).to_string()).await {
                return None;
            }
        }
    }

    // The first tick of a plain interval is immediate; these start one period from now.
    let start = tokio::time::Instant::now();
    let mut heartbeat = tokio::time::interval_at(start + HEARTBEAT, HEARTBEAT);
    let mut recheck = tokio::time::interval_at(start + RECHECK, RECHECK);
    let mut allowance = FRAMES_PER_SECOND;
    let mut second = Instant::now();
    loop {
        tokio::select! {
            frame = attached.frames.recv() => match frame {
                Some(frame) => {
                    if !send_text(socket, frame.to_string()).await {
                        return None;
                    }
                }
                // The hub dropped this connection: it did not keep up.
                None => return Some((close::OVERLOADED, "too slow")),
            },
            _ = heartbeat.tick() => {
                if !send_text(socket, json!({ "type": "ping" }).to_string()).await {
                    return None;
                }
            }
            _ = recheck.tick() => {
                if !authorized(database, workspace_id, session_id).await.unwrap_or(false) {
                    return Some((close::SESSION, "session or access ended"));
                }
            }
            message = socket.recv() => match message {
                Some(Ok(Message::Text(text))) => {
                    if second.elapsed() >= Duration::from_secs(1) {
                        second = Instant::now();
                        allowance = FRAMES_PER_SECOND;
                    }
                    if allowance == 0 {
                        return Some((close::RATE_LIMITED, "too many frames"));
                    }
                    allowance -= 1;
                    let Ok(ClientFrame::Typing { conversation_id, thread_root_id }) =
                        serde_json::from_str(&text)
                    else {
                        return Some((close::BAD_MESSAGE, "unknown frame"));
                    };
                    // Someone who may not write in the conversation types for nobody.
                    if let Ok(recipients) = state
                        .chat
                        .typing_recipients(workspace_id, user_id, conversation_id)
                        .await
                    {
                        state.hub.signal(
                            workspace_id,
                            &recipients,
                            "chat",
                            &json!({
                                "type": "typing",
                                "conversation_id": conversation_id,
                                "thread_root_id": thread_root_id,
                                "user_id": user_id,
                            }),
                        );
                    }
                }
                Some(Ok(Message::Ping(_) | Message::Pong(_))) => {}
                _ => return None,
            }
        }
    }
}

async fn send_text(socket: &mut WebSocket, text: String) -> bool {
    send(socket, Message::Text(text.into())).await
}

async fn send(socket: &mut WebSocket, message: Message) -> bool {
    matches!(
        tokio::time::timeout(SEND_TIMEOUT, socket.send(message)).await,
        Ok(Ok(()))
    )
}
