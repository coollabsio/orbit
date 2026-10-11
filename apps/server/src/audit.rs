use orbit_platform::{Database, Id, Row, TimestampMillis};
use serde::Serialize;
use serde_json::Value;
use sqlx::{Any, Transaction};
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
    /// The actor's current display name, `null` for system events and deleted accounts.
    pub actor_name: Option<String>,
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
    transaction: &mut Transaction<'_, Any>,
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
    transaction: &mut Transaction<'_, Any>,
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
    transaction: &mut Transaction<'_, Any>,
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
         resource_id, request_id, metadata_json, occurred_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
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

/// As `record`, inside a SeaORM transaction on any backend (spike: used by `api_tokens`).
#[allow(clippy::too_many_arguments)]
pub async fn record_orm(
    connection: &impl sea_orm::ConnectionTrait,
    workspace_id: Id,
    actor_id: Option<Id>,
    action: &str,
    outcome: AuditOutcome,
    resource_type: &str,
    resource_id: Option<Id>,
    request_id: &str,
    metadata: Value,
    now: TimestampMillis,
) -> Result<(), sea_orm::DbErr> {
    use sea_orm::{ActiveValue::Set, EntityTrait};

    use crate::entities::audit_events;

    audit_events::Entity::insert(audit_events::ActiveModel {
        id: Set(Id::new_v7().to_string()),
        workspace_id: Set(Some(workspace_id.to_string())),
        actor_id: Set(actor_id.map(|id| id.to_string())),
        action: Set(action.to_owned()),
        outcome: Set(outcome.as_str().to_owned()),
        resource_type: Set(resource_type.to_owned()),
        resource_id: Set(resource_id.map(|id| id.to_string())),
        request_id: Set(bounded_text(request_id, 128)),
        metadata_json: Set(bounded_metadata(metadata)),
        occurred_at: Set(now.as_millis()),
    })
    .exec_without_returning(connection)
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
        "SELECT audit_events.id, workspace_id, actor_id, users.display_name AS actor_name, action, outcome, \
         resource_type, resource_id, request_id, metadata_json, occurred_at FROM audit_events \
         LEFT JOIN users ON users.id = audit_events.actor_id \
         WHERE workspace_id = $1 AND ($2 IS NULL OR audit_events.id < $3) ORDER BY audit_events.id DESC LIMIT $4",
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
        "SELECT audit_events.id, workspace_id, actor_id, users.display_name AS actor_name, action, outcome, \
         resource_type, resource_id, request_id, metadata_json, occurred_at FROM audit_events \
         LEFT JOIN users ON users.id = audit_events.actor_id \
         WHERE ($1 IS NULL OR workspace_id = $2) AND ($3 IS NULL OR action = $4) \
         AND ($5 IS NULL OR audit_events.id < $6) ORDER BY audit_events.id DESC LIMIT $7",
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
        "SELECT audit_events.id, workspace_id, actor_id, users.display_name AS actor_name, action, outcome, \
         resource_type, resource_id, request_id, metadata_json, occurred_at FROM audit_events \
         LEFT JOIN users ON users.id = audit_events.actor_id \
         WHERE workspace_id = $1 AND resource_type = $2 AND resource_id = $3 \
         AND ($4 IS NULL OR audit_events.id < $5) ORDER BY audit_events.id DESC LIMIT $6",
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
    rows: Vec<sqlx::any::AnyRow>,
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

fn decode_event(row: sqlx::any::AnyRow) -> Result<AuditEvent, sqlx::Error> {
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
        actor_name: row.try_get("actor_name")?,
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
