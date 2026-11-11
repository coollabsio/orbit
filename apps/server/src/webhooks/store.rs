//! The SQL of webhook subscriptions and deliveries. Callers own the transactions.

use orbit_platform::{Id, Row, TimestampMillis};
use serde::Serialize;
use sqlx::any::AnyRow;
use sqlx::{Any, Transaction};
use utoipa::ToSchema;

use super::WebhookEventType;

/// How long finished deliveries are kept.
const DELIVERY_RETENTION_MILLIS: i64 = 30 * 24 * 60 * 60 * 1000;
/// Deliveries the API lists, newest first.
const DELIVERY_PAGE: i64 = 50;

#[derive(Clone, Debug, Serialize, ToSchema)]
#[schema(as = WebhookSubscription)]
pub(crate) struct Subscription {
    #[schema(value_type = String)]
    pub id: Id,
    pub name: String,
    pub url: String,
    pub event_types: Vec<WebhookEventType>,
    #[schema(value_type = Vec<String>)]
    pub project_ids: Vec<Id>,
    pub enabled: bool,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: TimestampMillis,
    #[schema(value_type = String, format = DateTime)]
    pub updated_at: TimestampMillis,
}

/// `pending` until a 2xx answer (`succeeded`), the last retry (`failed`), or a cancel (the
/// subscription was turned off, or its project left the subscription or went to the trash).
#[derive(Clone, Debug, Serialize, ToSchema)]
#[schema(as = WebhookDelivery)]
pub(crate) struct Delivery {
    #[schema(value_type = String)]
    pub id: Id,
    /// The `webhook-id` header: the same on every retry and for every subscription.
    #[schema(value_type = String)]
    pub event_id: Id,
    pub event_type: WebhookEventType,
    /// The task's id.
    #[schema(value_type = String)]
    pub resource_id: Id,
    /// `pending`, `succeeded`, `failed` or `cancelled`.
    pub state: String,
    pub attempt_count: i64,
    /// The HTTP status of the last answer; null when there was none.
    pub last_status: Option<i64>,
    /// Why the last attempt failed: `http_status`, `timeout`, `unreachable`, `blocked_address`,
    /// `app_key_missing`, `secret_unreadable` or `failed`.
    pub last_error: Option<String>,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: TimestampMillis,
    #[schema(value_type = Option<String>, format = DateTime)]
    pub last_attempt_at: Option<TimestampMillis>,
    #[schema(value_type = Option<String>, format = DateTime)]
    pub delivered_at: Option<TimestampMillis>,
}

pub(crate) struct NewSubscription<'a> {
    pub name: &'a str,
    pub url: &'a str,
    pub event_types: &'a [WebhookEventType],
    pub project_ids: &'a [Id],
    pub enabled: bool,
}

pub(super) fn parse_id(value: &str) -> Result<Id, sqlx::Error> {
    value
        .parse()
        .map_err(|_| sqlx::Error::Protocol("invalid webhook identifier".to_owned()))
}

/// The event types stored as a JSON array; unknown ones are skipped.
pub(super) fn event_types(stored: &str) -> Vec<WebhookEventType> {
    serde_json::from_str::<Vec<serde_json::Value>>(stored)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|value| serde_json::from_value(value).ok())
        .collect()
}

/// Whether the subscription takes events of `project_id`'s tasks.
pub(super) async fn covers(
    transaction: &mut Transaction<'_, Any>,
    subscription_id: Id,
    project_id: Id,
) -> Result<bool, sqlx::Error> {
    let found: Option<String> = sqlx::query_scalar(
        "SELECT project_id FROM webhook_subscription_projects WHERE subscription_id = $1 AND project_id = $2",
    )
    .bind(subscription_id.to_string())
    .bind(project_id.to_string())
    .fetch_optional(&mut **transaction)
    .await?;
    Ok(found.is_some())
}

/// Whether every id is a live project of the workspace.
pub(super) async fn projects_exist(
    transaction: &mut Transaction<'_, Any>,
    workspace_id: Id,
    project_ids: &[Id],
) -> Result<bool, sqlx::Error> {
    for project_id in project_ids {
        let found: Option<String> = sqlx::query_scalar(
            "SELECT id FROM projects WHERE id = $1 AND workspace_id = $2 AND deleted_at IS NULL",
        )
        .bind(project_id.to_string())
        .bind(workspace_id.to_string())
        .fetch_optional(&mut **transaction)
        .await?;
        if found.is_none() {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(super) async fn list(
    transaction: &mut Transaction<'_, Any>,
    workspace_id: Id,
) -> Result<Vec<Subscription>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, name, url, event_types, enabled, created_at, updated_at FROM webhook_subscriptions \
         WHERE workspace_id = $1 ORDER BY created_at, id",
    )
    .bind(workspace_id.to_string())
    .fetch_all(&mut **transaction)
    .await?;
    let mut subscriptions = Vec::with_capacity(rows.len());
    for row in rows {
        subscriptions.push(subscription_from_row(transaction, &row).await?);
    }
    Ok(subscriptions)
}

pub(super) async fn get(
    transaction: &mut Transaction<'_, Any>,
    workspace_id: Id,
    subscription_id: Id,
) -> Result<Option<Subscription>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT id, name, url, event_types, enabled, created_at, updated_at FROM webhook_subscriptions \
         WHERE id = $1 AND workspace_id = $2",
    )
    .bind(subscription_id.to_string())
    .bind(workspace_id.to_string())
    .fetch_optional(&mut **transaction)
    .await?;
    match row {
        Some(row) => Ok(Some(subscription_from_row(transaction, &row).await?)),
        None => Ok(None),
    }
}

async fn subscription_from_row(
    transaction: &mut Transaction<'_, Any>,
    row: &AnyRow,
) -> Result<Subscription, sqlx::Error> {
    let id = parse_id(&row.get::<String, _>("id"))?;
    let project_ids = sqlx::query_scalar::<_, String>(
        "SELECT project_id FROM webhook_subscription_projects WHERE subscription_id = $1 ORDER BY project_id",
    )
    .bind(id.to_string())
    .fetch_all(&mut **transaction)
    .await?
    .iter()
    .map(|value| parse_id(value))
    .collect::<Result<_, _>>()?;
    Ok(Subscription {
        id,
        name: row.get("name"),
        url: row.get("url"),
        event_types: event_types(&row.get::<String, _>("event_types")),
        project_ids,
        enabled: row.get("enabled"),
        created_at: TimestampMillis::from_millis(row.get("created_at")),
        updated_at: TimestampMillis::from_millis(row.get("updated_at")),
    })
}

pub(super) async fn insert(
    transaction: &mut Transaction<'_, Any>,
    workspace_id: Id,
    created_by: Id,
    input: &NewSubscription<'_>,
    sealed_secret: &[u8],
    now: TimestampMillis,
) -> Result<Id, sqlx::Error> {
    let id = Id::new_v7();
    sqlx::query(
        "INSERT INTO webhook_subscriptions (id, workspace_id, name, url, signing_secret, event_types, \
         enabled, created_by, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
    )
    .bind(id.to_string())
    .bind(workspace_id.to_string())
    .bind(input.name)
    .bind(input.url)
    .bind(sealed_secret.to_vec())
    .bind(stored_event_types(input.event_types))
    .bind(i64::from(input.enabled))
    .bind(created_by.to_string())
    .bind(now.as_millis())
    .bind(now.as_millis())
    .execute(&mut **transaction)
    .await?;
    set_projects(transaction, id, input.project_ids).await?;
    Ok(id)
}

pub(super) async fn update(
    transaction: &mut Transaction<'_, Any>,
    subscription_id: Id,
    input: &NewSubscription<'_>,
    now: TimestampMillis,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE webhook_subscriptions SET name = $1, url = $2, event_types = $3, enabled = $4, \
         updated_at = $5 WHERE id = $6",
    )
    .bind(input.name)
    .bind(input.url)
    .bind(stored_event_types(input.event_types))
    .bind(i64::from(input.enabled))
    .bind(now.as_millis())
    .bind(subscription_id.to_string())
    .execute(&mut **transaction)
    .await?;
    sqlx::query("DELETE FROM webhook_subscription_projects WHERE subscription_id = $1")
        .bind(subscription_id.to_string())
        .execute(&mut **transaction)
        .await?;
    set_projects(transaction, subscription_id, input.project_ids).await?;
    if !input.enabled {
        sqlx::query(
            "UPDATE webhook_deliveries SET state = 'cancelled', updated_at = $1 \
             WHERE subscription_id = $2 AND state = 'pending'",
        )
        .bind(now.as_millis())
        .bind(subscription_id.to_string())
        .execute(&mut **transaction)
        .await?;
    }
    Ok(())
}

async fn set_projects(
    transaction: &mut Transaction<'_, Any>,
    subscription_id: Id,
    project_ids: &[Id],
) -> Result<(), sqlx::Error> {
    for project_id in project_ids {
        sqlx::query(
            "INSERT INTO webhook_subscription_projects (subscription_id, project_id) VALUES ($1, $2) \
             ON CONFLICT DO NOTHING",
        )
        .bind(subscription_id.to_string())
        .bind(project_id.to_string())
        .execute(&mut **transaction)
        .await?;
    }
    Ok(())
}

fn stored_event_types(event_types: &[WebhookEventType]) -> String {
    serde_json::to_string(event_types).unwrap_or_else(|_| "[]".to_owned())
}

pub(super) async fn set_secret(
    transaction: &mut Transaction<'_, Any>,
    subscription_id: Id,
    sealed_secret: &[u8],
    now: TimestampMillis,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE webhook_subscriptions SET signing_secret = $1, updated_at = $2 WHERE id = $3",
    )
    .bind(sealed_secret.to_vec())
    .bind(now.as_millis())
    .bind(subscription_id.to_string())
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

pub(super) async fn delete(
    transaction: &mut Transaction<'_, Any>,
    subscription_id: Id,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM webhook_subscriptions WHERE id = $1")
        .bind(subscription_id.to_string())
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

const DELIVERY_COLUMNS: &str = "id, event_id, event_type, resource_id, state, attempt_count, \
    last_status, last_error, created_at, last_attempt_at, delivered_at";

pub(super) async fn deliveries(
    transaction: &mut Transaction<'_, Any>,
    subscription_id: Id,
) -> Result<Vec<Delivery>, sqlx::Error> {
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT {DELIVERY_COLUMNS} FROM webhook_deliveries WHERE subscription_id = $1 \
         ORDER BY created_at DESC, id DESC LIMIT $2"
    )))
    .bind(subscription_id.to_string())
    .bind(DELIVERY_PAGE)
    .fetch_all(&mut **transaction)
    .await?
    .iter()
    .map(delivery_from_row)
    .collect()
}

pub(super) async fn delivery(
    transaction: &mut Transaction<'_, Any>,
    subscription_id: Id,
    delivery_id: Id,
) -> Result<Option<Delivery>, sqlx::Error> {
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT {DELIVERY_COLUMNS} FROM webhook_deliveries WHERE id = $1 AND subscription_id = $2"
    )))
    .bind(delivery_id.to_string())
    .bind(subscription_id.to_string())
    .fetch_optional(&mut **transaction)
    .await?
    .as_ref()
    .map(delivery_from_row)
    .transpose()
}

/// Sets a failed or cancelled delivery back to pending with no attempts. The caller queues its job.
pub(super) async fn reset_delivery(
    transaction: &mut Transaction<'_, Any>,
    delivery_id: Id,
    now: TimestampMillis,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE webhook_deliveries SET state = 'pending', attempt_count = 0, last_status = NULL, \
         last_error = NULL, delivered_at = NULL, updated_at = $1 WHERE id = $2",
    )
    .bind(now.as_millis())
    .bind(delivery_id.to_string())
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

/// Removes the subscription's finished deliveries older than 30 days.
pub(super) async fn prune(
    database: &orbit_platform::Database,
    subscription_id: Id,
    now: TimestampMillis,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "DELETE FROM webhook_deliveries WHERE subscription_id = $1 AND state <> 'pending' AND created_at < $2",
    )
    .bind(subscription_id.to_string())
    .bind(now.as_millis().saturating_sub(DELIVERY_RETENTION_MILLIS))
    .execute(database.pool())
    .await?;
    Ok(())
}

fn delivery_from_row(row: &AnyRow) -> Result<Delivery, sqlx::Error> {
    let time = |column: &str| {
        row.get::<Option<i64>, _>(column)
            .map(TimestampMillis::from_millis)
    };
    let event_type: String = row.get("event_type");
    Ok(Delivery {
        id: parse_id(&row.get::<String, _>("id"))?,
        event_id: parse_id(&row.get::<String, _>("event_id"))?,
        event_type: serde_json::from_value(serde_json::Value::String(event_type))
            .map_err(|_| sqlx::Error::Protocol("invalid webhook event type".to_owned()))?,
        resource_id: parse_id(&row.get::<String, _>("resource_id"))?,
        state: row.get("state"),
        attempt_count: row.get("attempt_count"),
        last_status: row.get("last_status"),
        last_error: row.get("last_error"),
        created_at: TimestampMillis::from_millis(row.get("created_at")),
        last_attempt_at: time("last_attempt_at"),
        delivered_at: time("delivered_at"),
    })
}
