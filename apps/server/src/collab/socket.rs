//! `GET /api/v1/workspaces/{workspace_id}/pages/{page_id}/collab?v=1&epoch=<collab_epoch>`:
//! the page's y-sync WebSocket (what y-websocket 3 speaks).
//!
//! Before the upgrade: the platform layer requires exactly one allowed `Origin` for any request
//! with `Upgrade` (403); then the session cookie (401), and workspace membership plus a live page
//! the user can see (404, the same answer as for an unknown id). After it: `v` must be `1`
//! (4426) and `epoch` must be the page's `collab_epoch` (4409). The connection is re-checked every
//! 15 s and whenever pages move, get trashed or members leave.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use axum::body::Bytes;
use axum::extract::ws::{CloseFrame, Message as WsMessage, WebSocket, WebSocketUpgrade};
use axum::http::{HeaderMap, StatusCode, header::COOKIE};
use axum::response::{IntoResponse, Response};
use orbit_platform::{Database, Id, TimestampMillis};
use serde::Deserialize;
use sqlx::{Row, SqlitePool};
use tokio::sync::broadcast;

use super::close;
use super::hub::{CollabHub, MessageKind, Outbound, Peer};
use crate::auth_routes::CookieMode;
use crate::repositories::identity::IdentityRepository;

/// Query parameters y-websocket appends (`params`).
#[derive(Debug, Default, Deserialize)]
pub struct CollabQuery {
    /// Protocol version; must be `1`.
    pub v: Option<String>,
    /// The page's `collab_epoch` the client's document belongs to.
    pub epoch: Option<String>,
}

/// Why a connection may not (or no longer) edit the page.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Denied {
    Session,
    Forbidden,
    Gone,
}

impl Denied {
    fn close(self) -> (u16, &'static str) {
        match self {
            Self::Session => (close::SESSION, "session ended"),
            Self::Forbidden => (close::FORBIDDEN, "no access"),
            Self::Gone => (close::GONE, "page not found"),
        }
    }
}

/// Upgrades an authorized request to the page's collaboration socket.
#[allow(clippy::too_many_arguments)]
pub async fn upgrade(
    identity: &IdentityRepository,
    cookie_mode: CookieMode,
    database: &Database,
    workspace: &str,
    page: &str,
    query: CollabQuery,
    headers: &HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    let name = format!("{}=", cookie_mode.session_cookie_name());
    let Some(token) = headers
        .get(COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| {
            value
                .split(';')
                .find_map(|part| part.trim().strip_prefix(&name))
        })
        .filter(|token| !token.is_empty())
    else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let Ok(session) = identity
        .authenticate_session(token, TimestampMillis::now())
        .await
    else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let (Ok(workspace_id), Ok(page_id)) = (workspace.parse::<Id>(), page.parse::<Id>()) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let pool = database.pool().clone();
    match check_access(&pool, workspace_id, page_id, session.id, session.user.id).await {
        Ok(None) => {}
        // Not a member, someone else's private page, trashed or unknown: all look the same.
        Ok(Some(Denied::Forbidden | Denied::Gone)) => {
            return StatusCode::NOT_FOUND.into_response();
        }
        Ok(Some(Denied::Session)) => return StatusCode::UNAUTHORIZED.into_response(),
        Err(error) => {
            tracing::warn!(error = %error, "co-editing access check failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    }
    let hub = CollabHub::of(database);
    let limit = hub.config().max_frame_bytes;
    let connection = Connection {
        hub,
        pool,
        workspace_id,
        page_id,
        session_id: session.id,
        peer: Peer {
            user_id: session.user.id,
            name: session.user.display_name,
        },
        query,
    };
    upgrade
        // Slightly above the limit, so an oversized frame is answered with 4413 instead of a
        // protocol error.
        .max_message_size(limit + 64 * 1024)
        .max_frame_size(limit + 64 * 1024)
        .on_upgrade(move |socket| connection.serve(socket))
        .into_response()
}

struct Connection {
    hub: CollabHub,
    pool: SqlitePool,
    workspace_id: Id,
    page_id: Id,
    session_id: Id,
    peer: Peer,
    query: CollabQuery,
}

/// Messages per second, with one second of burst.
struct TokenBucket {
    capacity: f64,
    tokens: f64,
    last: Instant,
}

impl TokenBucket {
    fn new(per_second: u32) -> Self {
        let capacity = f64::from(per_second.max(1));
        Self {
            capacity,
            tokens: capacity,
            last: Instant::now(),
        }
    }

    fn take(&mut self) -> bool {
        let now = Instant::now();
        let refill = now.duration_since(self.last).as_secs_f64() * self.capacity;
        self.tokens = (self.tokens + refill).min(self.capacity);
        self.last = now;
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

impl Connection {
    async fn serve(self, mut socket: WebSocket) {
        if self.query.v.as_deref() != Some(super::PROTOCOL_VERSION) {
            send_close(&mut socket, close::PROTOCOL, "unsupported protocol version").await;
            return;
        }
        let joined = match self.hub.join(self.page_id, &self.peer).await {
            Ok(joined) => joined,
            Err((code, reason)) => {
                send_close(&mut socket, code, reason).await;
                return;
            }
        };
        let room = joined.room.clone();
        let connection = joined.connection;
        let (code, reason) = self.run(&mut socket, joined).await;
        self.hub.leave(&room, connection, &self.peer).await;
        send_close(&mut socket, code, reason).await;
    }

    /// The connection loop; returns how to close.
    async fn run(&self, socket: &mut WebSocket, joined: super::hub::Joined) -> (u16, &'static str) {
        if self.query.epoch.as_deref() != Some(joined.epoch.as_str()) {
            return (close::RESET, "stale document");
        }
        let config = self.hub.config().clone();
        let super::hub::Joined {
            room,
            connection,
            mut rx,
            greeting,
            ..
        } = joined;
        for frame in greeting {
            if !send(socket, WsMessage::Binary(Bytes::from(frame))).await {
                return (1000, "");
            }
        }
        let mut sync_bucket = TokenBucket::new(config.sync_messages_per_second);
        let mut awareness_bucket = TokenBucket::new(config.awareness_messages_per_second);
        let mut owned_clients = HashSet::new();
        let mut heartbeat = tokio::time::interval(config.ping_interval);
        heartbeat.tick().await;
        let mut recheck = tokio::time::interval(config.recheck_interval);
        recheck.tick().await;
        let mut last_pong = Instant::now();
        loop {
            tokio::select! {
                incoming = socket.recv() => {
                    let data = match incoming {
                        Some(Ok(WsMessage::Binary(data))) => data,
                        Some(Ok(WsMessage::Pong(_))) => { last_pong = Instant::now(); continue; }
                        Some(Ok(WsMessage::Ping(_) | WsMessage::Text(_))) => continue,
                        Some(Ok(WsMessage::Close(_))) | None => return (1000, ""),
                        Some(Err(_)) => return (close::TOO_LARGE, "frame too large or invalid"),
                    };
                    if data.len() > config.max_frame_bytes {
                        return (close::TOO_LARGE, "frame too large");
                    }
                    let mut budget = |kind: MessageKind| match kind {
                        MessageKind::Sync => sync_bucket.take(),
                        MessageKind::Awareness => awareness_bucket.take(),
                    };
                    let handled = self
                        .hub
                        .handle(&room, connection, &self.peer, &data, &mut owned_clients, &mut budget)
                        .await;
                    for reply in handled.replies {
                        if !send(socket, WsMessage::Binary(Bytes::from(reply))).await {
                            return (1000, "");
                        }
                    }
                    if let Some(close) = handled.close {
                        return close;
                    }
                }
                outbound = rx.recv() => match outbound {
                    Ok(Outbound::Frame { except, data }) => {
                        if except != Some(connection) && !send(socket, WsMessage::Binary(data)).await {
                            return (1000, "");
                        }
                    }
                    Ok(Outbound::Close { code, reason }) => return (code, reason),
                    Ok(Outbound::Revalidate) => {
                        if let Some(denied) = self.denied().await {
                            return denied.close();
                        }
                    }
                    // Too slow to keep up: the client resyncs on reconnect.
                    Err(broadcast::error::RecvError::Lagged(_)) => return (close::OVERLOADED, "lagged"),
                    Err(broadcast::error::RecvError::Closed) => return (close::RESTART, "room closed"),
                },
                _ = heartbeat.tick() => {
                    if last_pong.elapsed() > config.ping_interval * 3 {
                        return (1001, "no pong");
                    }
                    if !send(socket, WsMessage::Ping(Bytes::new())).await {
                        return (1000, "");
                    }
                }
                _ = recheck.tick() => {
                    if let Some(denied) = self.denied().await {
                        return denied.close();
                    }
                }
            }
        }
    }

    async fn denied(&self) -> Option<Denied> {
        // A database hiccup is not a reason to drop an editor; the next check decides.
        check_access(
            &self.pool,
            self.workspace_id,
            self.page_id,
            self.session_id,
            self.peer.user_id,
        )
        .await
        .unwrap_or_default()
    }
}

/// `None` when the session is valid, the user is a member of the workspace and the page is live
/// and visible to them.
async fn check_access(
    pool: &SqlitePool,
    workspace_id: Id,
    page_id: Id,
    session_id: Id,
    user_id: Id,
) -> Result<Option<Denied>, sqlx::Error> {
    let now = TimestampMillis::now().as_millis();
    let row = sqlx::query(
        "SELECT \
         EXISTS(SELECT 1 FROM sessions JOIN users ON users.id = sessions.user_id \
                WHERE sessions.id = ?1 AND sessions.revoked_at IS NULL AND users.suspended_at IS NULL \
                AND sessions.idle_expires_at > ?5 AND sessions.absolute_expires_at > ?5) AS session_ok, \
         EXISTS(SELECT 1 FROM memberships JOIN workspaces ON workspaces.id = memberships.workspace_id \
                WHERE memberships.user_id = ?2 AND memberships.workspace_id = ?3 \
                AND workspaces.deleted_at IS NULL) AS member, \
         (SELECT CASE WHEN deleted_at IS NOT NULL THEN 2 \
                      WHEN teamspace_id IS NULL AND owner_id IS NOT ?2 THEN 1 ELSE 0 END \
          FROM pages WHERE id = ?4 AND workspace_id = ?3) AS page_state",
    )
    .bind(session_id.to_string())
    .bind(user_id.to_string())
    .bind(workspace_id.to_string())
    .bind(page_id.to_string())
    .bind(now)
    .fetch_one(pool)
    .await?;
    Ok(if !row.get::<bool, _>("session_ok") {
        Some(Denied::Session)
    } else if !row.get::<bool, _>("member") {
        Some(Denied::Forbidden)
    } else {
        match row.get::<Option<i64>, _>("page_state") {
            Some(0) => None,
            Some(1) => Some(Denied::Forbidden),
            _ => Some(Denied::Gone),
        }
    })
}

async fn send(socket: &mut WebSocket, message: WsMessage) -> bool {
    matches!(
        tokio::time::timeout(Duration::from_secs(10), socket.send(message)).await,
        Ok(Ok(()))
    )
}

async fn send_close(socket: &mut WebSocket, code: u16, reason: &'static str) {
    let _ = tokio::time::timeout(
        Duration::from_secs(2),
        socket.send(WsMessage::Close(Some(CloseFrame {
            code,
            reason: reason.into(),
        }))),
    )
    .await;
}
