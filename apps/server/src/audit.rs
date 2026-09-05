use orbit_platform::{Database, Id, TimestampMillis};
use serde::Serialize;
use serde_json::Value;
use sqlx::{Row, Sqlite, Transaction};
use utoipa::ToSchema;

const MAX_METADATA_BYTES: usize = 2_048;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuditOutcome {
    Success,
    Failure,
}

impl AuditOutcome {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Failure => "failure",
        }
    }
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct AuditEvent {
    #[schema(value_type = String)]
    pub id: Id,
    #[schema(value_type = Option<String>)]
    pub workspace_id: Option<Id>,
    #[schema(value_type = Option<String>)]
    pub actor_id: Option<Id>,
    pub action: String,
    pub outcome: String,
    pub resource_type: String,
    #[schema(value_type = Option<String>)]
    pub resource_id: Option<Id>,
    pub request_id: String,
    pub metadata: Value,
    #[schema(value_type = String, format = DateTime)]
    pub occurred_at: TimestampMillis,
}

#[allow(clippy::too_many_arguments)]
pub async fn record(
    transaction: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    actor_id: Option<Id>,
    action: &str,
    outcome: AuditOutcome,
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
        outcome,
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
    outcome: AuditOutcome,
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
        outcome,
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
    outcome: AuditOutcome,
    resource_type: &str,
    resource_id: Option<Id>,
    request_id: &str,
    metadata: Value,
    now: TimestampMillis,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO audit_events (id, workspace_id, actor_id, action, outcome, resource_type, \
         resource_id, request_id, metadata_json, occurred_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(Id::new_v7().to_string())
    .bind(workspace_id.map(|id| id.to_string()))
    .bind(actor_id.map(|id| id.to_string()))
    .bind(action)
    .bind(outcome.as_str())
    .bind(resource_type)
    .bind(resource_id.map(|id| id.to_string()))
    .bind(bounded_text(request_id, 128))
    .bind(bounded_metadata(metadata))
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
    decode_page(rows, limit)
}

pub async fn list_global(
    database: &Database,
    workspace_id: Option<Id>,
    action: Option<&str>,
    cursor: Option<Id>,
    limit: usize,
) -> Result<(Vec<AuditEvent>, Option<Id>), sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, workspace_id, actor_id, action, outcome, resource_type, resource_id, \
         request_id, metadata_json, occurred_at FROM audit_events \
         WHERE (? IS NULL OR workspace_id = ?) AND (? IS NULL OR action = ?) \
         AND (? IS NULL OR id < ?) ORDER BY id DESC LIMIT ?",
    )
    .bind(workspace_id.map(|id| id.to_string()))
    .bind(workspace_id.map(|id| id.to_string()))
    .bind(action)
    .bind(action)
    .bind(cursor.map(|id| id.to_string()))
    .bind(cursor.map(|id| id.to_string()))
    .bind(i64::try_from(limit.saturating_add(1)).unwrap_or(101))
    .fetch_all(database.pool())
    .await?;
    decode_page(rows, limit)
}

pub async fn list_resource(
    database: &Database,
    workspace_id: Id,
    resource_type: &str,
    resource_id: Id,
    cursor: Option<Id>,
    limit: usize,
) -> Result<(Vec<AuditEvent>, Option<Id>), sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, workspace_id, actor_id, action, outcome, resource_type, resource_id, \
         request_id, metadata_json, occurred_at FROM audit_events \
         WHERE workspace_id = ? AND resource_type = ? AND resource_id = ? \
         AND (? IS NULL OR id < ?) ORDER BY id DESC LIMIT ?",
    )
    .bind(workspace_id.to_string())
    .bind(resource_type)
    .bind(resource_id.to_string())
    .bind(cursor.map(|id| id.to_string()))
    .bind(cursor.map(|id| id.to_string()))
    .bind(i64::try_from(limit.saturating_add(1)).unwrap_or(101))
    .fetch_all(database.pool())
    .await?;
    decode_page(rows, limit)
}

fn decode_page(
    rows: Vec<sqlx::sqlite::SqliteRow>,
    limit: usize,
) -> Result<(Vec<AuditEvent>, Option<Id>), sqlx::Error> {
    let has_more = rows.len() > limit;
    let events = rows
        .into_iter()
        .take(limit)
        .map(decode_event)
        .collect::<Result<Vec<_>, _>>()?;
    let next = has_more
        .then(|| events.last().map(|event| event.id))
        .flatten();
    Ok((events, next))
}

fn decode_event(row: sqlx::sqlite::SqliteRow) -> Result<AuditEvent, sqlx::Error> {
    let parse_id = |value: String| {
        value
            .parse()
            .map_err(|_| sqlx::Error::Protocol("invalid audit identifier".to_owned()))
    };
    Ok(AuditEvent {
        id: parse_id(row.try_get("id")?)?,
        workspace_id: row
            .try_get::<Option<String>, _>("workspace_id")?
            .map(parse_id)
            .transpose()?,
        actor_id: row
            .try_get::<Option<String>, _>("actor_id")?
            .map(parse_id)
            .transpose()?,
        action: row.try_get("action")?,
        outcome: row.try_get("outcome")?,
        resource_type: row.try_get("resource_type")?,
        resource_id: row
            .try_get::<Option<String>, _>("resource_id")?
            .map(parse_id)
            .transpose()?,
        request_id: row.try_get("request_id")?,
        metadata: serde_json::from_str(&row.try_get::<String, _>("metadata_json")?)
            .map_err(|_| sqlx::Error::Protocol("invalid audit metadata".to_owned()))?,
        occurred_at: TimestampMillis::from_millis(row.try_get("occurred_at")?),
    })
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
