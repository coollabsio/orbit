//! The numbered events of the live socket, in `live_events`: one sequence per workspace for every
//! server of the database, and the newest [`BUFFERED_EVENTS`] for clients that reconnect.

use std::sync::Arc;

use orbit_platform::{Database, Id, Row};
use serde_json::{Value, json};
use sqlx::AnyPool;

use super::hub::Recipients;

/// Events kept for each workspace, for clients that reconnect.
pub(super) const BUFFERED_EVENTS: i64 = 1000;

/// An event to number and keep.
pub struct NewEvent {
    pub recipients: Recipients,
    pub topic: &'static str,
    pub event: Value,
}

/// A kept event.
pub(super) struct Stored {
    pub seq: u64,
    pub recipients: Recipients,
    pub frame: Arc<str>,
}

/// Numbers `events` after the newest of the workspace and keeps them. Returns the number of the
/// last one; with no events, the newest number.
pub(super) async fn append(
    database: &Database,
    workspace_id: Id,
    events: &[NewEvent],
) -> Result<u64, sqlx::Error> {
    let workspace = workspace_id.to_string();
    let mut transaction = database.workspace_transaction(workspace_id).await?;
    let mut seq: i64 = sqlx::query_scalar(
        "SELECT CAST(COALESCE(MAX(seq), 0) AS BIGINT) FROM live_events WHERE workspace_id = $1",
    )
    .bind(&workspace)
    .fetch_one(&mut *transaction)
    .await?;
    if events.is_empty() {
        return Ok(u64::try_from(seq).unwrap_or(0));
    }
    for event in events {
        seq += 1;
        let frame = json!({ "seq": seq, "topic": event.topic, "event": event.event });
        let recipients = serde_json::to_string(&event.recipients)
            .map_err(|error| sqlx::Error::Encode(Box::new(error)))?;
        sqlx::query(
            "INSERT INTO live_events (workspace_id, seq, recipients_json, frame) \
             VALUES ($1, $2, $3, $4)",
        )
        .bind(&workspace)
        .bind(seq)
        .bind(recipients)
        .bind(frame.to_string())
        .execute(&mut *transaction)
        .await?;
    }
    sqlx::query("DELETE FROM live_events WHERE workspace_id = $1 AND seq <= $2")
        .bind(&workspace)
        .bind(seq - BUFFERED_EVENTS)
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;
    Ok(u64::try_from(seq).unwrap_or(0))
}

/// The events of a workspace after `after`, oldest first.
pub(super) async fn after(
    pool: &AnyPool,
    workspace_id: Id,
    after: u64,
) -> Result<Vec<Stored>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT seq, recipients_json, frame FROM live_events \
         WHERE workspace_id = $1 AND seq > $2 ORDER BY seq",
    )
    .bind(workspace_id.to_string())
    .bind(i64::try_from(after).unwrap_or(i64::MAX))
    .fetch_all(pool)
    .await?;
    rows.into_iter()
        .map(|row| {
            Ok(Stored {
                seq: u64::try_from(row.get::<i64, _>("seq")).unwrap_or(0),
                recipients: serde_json::from_str(&row.get::<String, _>("recipients_json"))
                    .map_err(|error| sqlx::Error::Decode(Box::new(error)))?,
                frame: row.get::<String, _>("frame").into(),
            })
        })
        .collect()
}

/// The numbers of the oldest and the newest kept event of a workspace (0 and 0 with none).
pub(super) async fn range(pool: &AnyPool, workspace_id: Id) -> Result<(u64, u64), sqlx::Error> {
    let row = sqlx::query(
        "SELECT CAST(COALESCE(MIN(seq), 0) AS BIGINT) AS oldest, \
         CAST(COALESCE(MAX(seq), 0) AS BIGINT) AS newest FROM live_events WHERE workspace_id = $1",
    )
    .bind(workspace_id.to_string())
    .fetch_one(pool)
    .await?;
    Ok((
        u64::try_from(row.get::<i64, _>("oldest")).unwrap_or(0),
        u64::try_from(row.get::<i64, _>("newest")).unwrap_or(0),
    ))
}

/// The epoch of the event numbers: a restored database gets a new one (it rotates the co-editing
/// generation), so a client cannot replay numbers that were given again.
pub(super) async fn epoch(pool: &AnyPool) -> Result<String, sqlx::Error> {
    sqlx::query_scalar("SELECT generation FROM page_collab_meta WHERE id = 1")
        .fetch_one(pool)
        .await
}
