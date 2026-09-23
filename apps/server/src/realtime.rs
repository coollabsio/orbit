//! Workspace invalidations. HTTP remains authoritative for resource data.
use std::time::{Duration, Instant};

use crate::task_routes::TaskState;
use axum::{
    Router,
    extract::{
        Path, Query, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::{HeaderMap, StatusCode, header::COOKIE},
    response::Response,
    routing::get,
};
use orbit_platform::{Database, Id, TimestampMillis};
use serde::Deserialize;

const RETENTION_MS: i64 = 7 * 24 * 60 * 60 * 1000;

pub fn router(state: TaskState) -> Router {
    Router::new()
        .route("/api/v1/workspaces/{workspace_id}/events", get(connect))
        .with_state(state)
}

#[derive(Deserialize)]
struct Cursor {
    after: Option<i64>,
}

async fn connect(
    State(state): State<TaskState>,
    Path(workspace): Path<Id>,
    Query(cursor): Query<Cursor>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Result<Response, StatusCode> {
    let name = format!("{}=", state.cookie_mode.session_cookie_name());
    let token = headers
        .get(COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| {
            v.split(';')
                .find_map(|part| part.trim().strip_prefix(&name))
        })
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let session = state
        .identity
        .authenticate_session(token, TimestampMillis::now())
        .await
        .map_err(|_| StatusCode::UNAUTHORIZED)?;
    let database = state.identity.database().clone();
    if !authorized(&database, workspace, session.id)
        .await
        .unwrap_or(false)
    {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(upgrade
        .max_message_size(1024)
        .max_frame_size(1024)
        .on_upgrade(move |socket| serve(socket, database, workspace, session.id, cursor.after)))
}

pub async fn authorized(
    database: &Database,
    workspace: Id,
    session: Id,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sessions s JOIN users u ON u.id=s.user_id JOIN memberships m ON m.user_id=u.id JOIN workspaces w ON w.id=m.workspace_id WHERE s.id=? AND w.id=? AND s.revoked_at IS NULL AND u.suspended_at IS NULL AND w.deleted_at IS NULL AND s.idle_expires_at>? AND s.absolute_expires_at>?)")
        .bind(session.to_string()).bind(workspace.to_string()).bind(TimestampMillis::now().as_millis()).bind(TimestampMillis::now().as_millis()).fetch_one(database.pool()).await
}

#[derive(Debug, serde::Serialize)]
pub struct Events {
    pub version: u8,
    pub kind: &'static str,
    // Strings avoid losing precision in browser JSON numbers.
    pub sequence: String,
    pub workspaces_changed: bool,
    pub profile_changed: bool,
}

pub async fn replay(
    database: &Database,
    workspace: Id,
    after: Option<i64>,
) -> Result<Events, sqlx::Error> {
    // One snapshot prevents a writer racing cursor validation and replay.
    let mut tx = database.pool().begin().await?;
    let latest: i64 = sqlx::query_scalar(
        "SELECT COALESCE((SELECT sequence FROM realtime_sequences WHERE workspace_id=?),0)",
    )
    .bind(workspace.to_string())
    .fetch_one(&mut *tx)
    .await?;
    let oldest: Option<i64> = sqlx::query_scalar(
        "SELECT MIN(sequence) FROM outbox_events WHERE workspace_id=? AND created_at>=?",
    )
    .bind(workspace.to_string())
    .bind(TimestampMillis::now().as_millis() - RETENTION_MS)
    .fetch_one(&mut *tx)
    .await?;
    let valid = after.is_some_and(|n| {
        n >= 0 && n <= latest && (n == latest || oldest.is_some_and(|first| n >= first - 1))
    });
    let (workspaces_changed, profile_changed) = if valid && after != Some(latest) {
        let (workspace, profile): (i64, i64) = sqlx::query_as(
            "SELECT COALESCE(MAX(a.resource_type IN ('workspace', 'membership')), 0), \
                    COALESCE(MAX(a.resource_type = 'user'), 0) \
             FROM outbox_events e JOIN audit_events a ON a.id = e.id \
             WHERE e.scope = ? AND e.sequence > ? AND e.sequence <= ?",
        )
        .bind(workspace.to_string())
        .bind(after.unwrap_or_default())
        .bind(latest)
        .fetch_one(&mut *tx)
        .await?;
        (workspace != 0, profile != 0)
    } else if valid {
        (false, false)
    } else {
        (true, true)
    };
    tx.commit().await?;
    // All events invalidate the same workspace cache; coalesce the ordered range.
    Ok(Events {
        version: 1,
        kind: if valid {
            "workspace.changed"
        } else {
            "resync_required"
        },
        sequence: latest.to_string(),
        workspaces_changed,
        profile_changed,
    })
}

async fn serve(
    mut socket: WebSocket,
    database: Database,
    workspace: Id,
    session: Id,
    mut after: Option<i64>,
) {
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    let mut heartbeat = tokio::time::interval(Duration::from_secs(20));
    let mut last_pong = Instant::now();
    loop {
        tokio::select! {
            _ = tick.tick() => {
                if !authorized(&database, workspace, session).await.unwrap_or(false) { break; }
                let Ok(event) = replay(&database, workspace, after).await else { break; };
                let sequence = event.sequence.parse::<i64>().ok();
                if after != sequence || event.kind == "resync_required" {
                    // Recheck after reading the batch, before releasing any notification.
                    if !authorized(&database, workspace, session).await.unwrap_or(false) { break; }
                    if !send(&mut socket, Message::Text(serde_json::to_string(&event).unwrap().into())).await { break; }
                    after = sequence;
                }
            }
            _ = heartbeat.tick() => {
                if last_pong.elapsed() > Duration::from_secs(60) { break; }
                if !send(&mut socket, Message::Ping(Vec::new().into())).await { break; }
            }
            message = socket.recv() => match message {
                Some(Ok(Message::Pong(_))) => last_pong = Instant::now(),
                Some(Ok(Message::Ping(_))) => {},
                _ => break,
            }
        }
    }
    let _ = send(&mut socket, Message::Close(None)).await;
}

async fn send(socket: &mut WebSocket, message: Message) -> bool {
    matches!(
        tokio::time::timeout(Duration::from_secs(5), socket.send(message)).await,
        Ok(Ok(()))
    )
}
