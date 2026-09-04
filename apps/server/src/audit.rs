use orbit_platform::{Database, Id, TimestampMillis};
use serde::Serialize;
use serde_json::Value;
use sqlx::{Row, Sqlite, Transaction};

const RETENTION_MILLIS: i64 = 365 * 24 * 60 * 60 * 1_000;
const MAX_METADATA_BYTES: usize = 2_048;

#[derive(Clone, Debug, Serialize)]
pub struct AuditEvent {
    pub id: Id,
    pub workspace_id: Id,
    pub actor_id: Option<Id>,
    pub action: String,
    pub outcome: String,
    pub resource_type: String,
    pub resource_id: Option<Id>,
    pub request_id: String,
    pub metadata: Value,
    pub occurred_at: TimestampMillis,
}

#[allow(clippy::too_many_arguments)]
pub async fn record(
    transaction: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    actor_id: Option<Id>,
    action: &str,
    resource_type: &str,
    resource_id: Option<Id>,
    request_id: &str,
    metadata: Value,
    now: TimestampMillis,
) -> Result<(), sqlx::Error> {
    record_scoped(
        transaction,
        Some(workspace_id),
        actor_id,
        action,
        resource_type,
        resource_id,
        request_id,
        metadata,
        now,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn record_global(
    transaction: &mut Transaction<'_, Sqlite>,
    actor_id: Option<Id>,
    action: &str,
    resource_type: &str,
    resource_id: Option<Id>,
    request_id: &str,
    metadata: Value,
    now: TimestampMillis,
) -> Result<(), sqlx::Error> {
    record_scoped(
        transaction,
        None,
        actor_id,
        action,
        resource_type,
        resource_id,
        request_id,
        metadata,
        now,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn record_scoped(
    transaction: &mut Transaction<'_, Sqlite>,
    workspace_id: Option<Id>,
    actor_id: Option<Id>,
    action: &str,
    resource_type: &str,
    resource_id: Option<Id>,
    request_id: &str,
    metadata: Value,
    now: TimestampMillis,
) -> Result<(), sqlx::Error> {
    let metadata = bounded_metadata(metadata);
    sqlx::query("DELETE FROM audit_events WHERE occurred_at < ?")
        .bind(now.as_millis().saturating_sub(RETENTION_MILLIS))
        .execute(&mut **transaction)
        .await?;
    sqlx::query(
        "INSERT INTO audit_events (id, workspace_id, actor_id, action, outcome, resource_type, \
         resource_id, request_id, metadata_json, occurred_at) VALUES (?, ?, ?, ?, 'success', ?, ?, ?, ?, ?)",
    )
    .bind(Id::new_v7().to_string())
    .bind(workspace_id.map(|id| id.to_string()))
    .bind(actor_id.map(|id| id.to_string()))
    .bind(action)
    .bind(resource_type)
    .bind(resource_id.map(|id| id.to_string()))
    .bind(bounded_text(request_id, 128))
    .bind(metadata)
    .bind(now.as_millis())
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

pub async fn list(
    database: &Database,
    workspace_id: Id,
    cursor: Option<Id>,
    limit: usize,
) -> Result<(Vec<AuditEvent>, Option<Id>), sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, workspace_id, actor_id, action, outcome, resource_type, resource_id, \
         request_id, metadata_json, occurred_at FROM audit_events \
         WHERE workspace_id = ? AND (? IS NULL OR id < ?) ORDER BY id DESC LIMIT ?",
    )
    .bind(workspace_id.to_string())
    .bind(cursor.map(|id| id.to_string()))
    .bind(cursor.map(|id| id.to_string()))
    .bind(i64::try_from(limit.saturating_add(1)).unwrap_or(101))
    .fetch_all(database.pool())
    .await?;
    let has_more = rows.len() > limit;
    let mut events = rows
        .into_iter()
        .take(limit)
        .filter_map(|row| {
            Some(AuditEvent {
                id: row.get::<String, _>("id").parse().ok()?,
                workspace_id: row.get::<String, _>("workspace_id").parse().ok()?,
                actor_id: row
                    .get::<Option<String>, _>("actor_id")
                    .and_then(|id| id.parse().ok()),
                action: row.get("action"),
                outcome: row.get("outcome"),
                resource_type: row.get("resource_type"),
                resource_id: row
                    .get::<Option<String>, _>("resource_id")
                    .and_then(|id| id.parse().ok()),
                request_id: row.get("request_id"),
                metadata: serde_json::from_str(&row.get::<String, _>("metadata_json")).ok()?,
                occurred_at: TimestampMillis::from_millis(row.get("occurred_at")),
            })
        })
        .collect::<Vec<_>>();
    let next = has_more
        .then(|| events.last().map(|event| event.id))
        .flatten();
    Ok((std::mem::take(&mut events), next))
}

fn bounded_metadata(metadata: Value) -> String {
    let serialized = serde_json::to_string(&metadata).unwrap_or_else(|_| "{}".to_owned());
    if serialized.len() <= MAX_METADATA_BYTES {
        serialized
    } else {
        "{\"truncated\":true}".to_owned()
    }
}

fn bounded_text(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}
