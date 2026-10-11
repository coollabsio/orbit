//! Workspace invalidations. HTTP remains authoritative for resource data.
use std::time::{Duration, Instant};

use crate::auth_routes::request_session;
use crate::task_routes::TaskState;
use axum::{
    Router,
    extract::{
        Path, Query, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::{HeaderMap, StatusCode},
    response::Response,
    routing::get,
};
use orbit_platform::{Database, Flag, Id, TimestampMillis};
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
    let session = request_session(&state.identity, state.cookie_mode, &headers)
        .await
        .ok_or(StatusCode::UNAUTHORIZED)?;
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
    let Flag(found) = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sessions s JOIN users u ON u.id=s.user_id JOIN memberships m ON m.user_id=u.id JOIN workspaces w ON w.id=m.workspace_id WHERE s.id=$1 AND w.id=$2 AND s.revoked_at IS NULL AND u.suspended_at IS NULL AND w.deleted_at IS NULL AND s.idle_expires_at>$3 AND s.absolute_expires_at>$4)")
        .bind(session.to_string()).bind(workspace.to_string()).bind(TimestampMillis::now().as_millis()).bind(TimestampMillis::now().as_millis()).fetch_one(database.pool()).await?;
    Ok(found)
}

#[derive(Debug, serde::Serialize)]
pub struct Events {
    pub version: u8,
    pub kind: &'static str,
    // Strings avoid losing precision in browser JSON numbers.
    pub sequence: String,
    pub workspaces_changed: bool,
    pub profile_changed: bool,
    /// Every change in the range is a saved-view write, so clients refresh only their views.
    pub views_only: bool,
}

pub async fn replay(
    database: &Database,
    workspace: Id,
    after: Option<i64>,
) -> Result<Events, sqlx::Error> {
    // One snapshot prevents a writer racing cursor validation and replay.
    let mut tx = database.pool().begin().await?;
    let latest: i64 = sqlx::query_scalar(
        "SELECT COALESCE((SELECT sequence FROM realtime_sequences WHERE workspace_id=$1),0)",
    )
    .bind(workspace.to_string())
    .fetch_one(&mut *tx)
    .await?;
    let oldest: Option<i64> = sqlx::query_scalar(
        "SELECT MIN(sequence) FROM outbox_events WHERE workspace_id=$1 AND created_at>=$2",
    )
    .bind(workspace.to_string())
    .bind(TimestampMillis::now().as_millis() - RETENTION_MS)
    .fetch_one(&mut *tx)
    .await?;
    let valid = after.is_some_and(|n| {
        n >= 0 && n <= latest && (n == latest || oldest.is_some_and(|first| n >= first - 1))
    });
    let (workspaces_changed, profile_changed, views_only) = if valid && after != Some(latest) {
        // LEFT JOIN: an event without its audit row counts as "not a view change"
        let (workspace, profile, views): (i64, i64, i64) = sqlx::query_as(
            "SELECT COALESCE(MAX(CASE WHEN a.resource_type IN ('workspace', 'membership') \
                                      THEN 1 ELSE 0 END), 0), \
                    COALESCE(MAX(CASE WHEN a.resource_type = 'user' THEN 1 ELSE 0 END), 0), \
                    COALESCE(MIN(CASE WHEN a.resource_type = 'saved_view' THEN 1 ELSE 0 END), 0) \
             FROM outbox_events e LEFT JOIN audit_events a ON a.id = e.id \
             WHERE e.scope = $1 AND e.sequence > $2 AND e.sequence <= $3",
        )
        .bind(workspace.to_string())
        .bind(after.unwrap_or_default())
        .bind(latest)
        .fetch_one(&mut *tx)
        .await?;
        (workspace != 0, profile != 0, views != 0)
    } else if valid {
        (false, false, false)
    } else {
        (true, true, false)
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
        views_only,
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
