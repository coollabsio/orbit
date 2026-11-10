//! Unique ID properties (spec P3 item 10). A database has at most one. Each row stores its number;
//! the property's `next` is the number the next row gets. Every path that makes rows calls
//! [`number_rows`] in its transaction, which numbers the rows without one in creation order. `next`
//! only grows, so a number is never given twice, also after a delete. Clients cannot write
//! numbers; only an import keeps the numbers it brings.

use orbit_platform::{Id, TimestampMillis};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{Any, Transaction};

use super::super::pages::PageError;
use super::search::{SearchRows, refresh_search_text};
use super::templates::NOT_TEMPLATE;
use super::{
    CellValue, DatabaseError, DatabaseRow, Property, PropertyKind, page_error, properties_in_tx,
    upsert_value,
};

const MAX_PREFIX_CHARS: usize = 10;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UniqueIdConfig {
    /// Shown before the number, as `PREFIX-12`.
    #[serde(default)]
    pub prefix: Option<String>,
    /// The number of the next row. Only the server changes it.
    #[serde(default = "first")]
    pub next: i64,
}

const fn first() -> i64 {
    1
}

impl Default for UniqueIdConfig {
    fn default() -> Self {
        Self {
            prefix: None,
            next: first(),
        }
    }
}

impl UniqueIdConfig {
    /// A stored or client config; an empty prefix is none.
    pub(super) fn parse(config: &Value) -> Result<Self, String> {
        let mut config: Self =
            serde_json::from_value(config.clone()).map_err(|_| "config".to_owned())?;
        config.prefix = config.prefix.filter(|prefix| !prefix.is_empty());
        if config.prefix.as_ref().is_some_and(|prefix| {
            prefix.chars().count() > MAX_PREFIX_CHARS
                || !prefix
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_')
        }) {
            return Err("config.prefix".to_owned());
        }
        if config.next < 1 {
            return Err("config.next".to_owned());
        }
        Ok(config)
    }
}

/// `PREFIX-12`, or `12` without a prefix.
pub(super) fn text(kind: &PropertyKind, number: i64) -> String {
    match kind {
        PropertyKind::UniqueId(UniqueIdConfig {
            prefix: Some(prefix),
            ..
        }) => format!("{prefix}-{number}"),
        _ => number.to_string(),
    }
}

/// `kind` as a client set it, with the stored counter of `current`.
pub(super) fn keep_next(current: &PropertyKind, kind: PropertyKind) -> PropertyKind {
    match (current, kind) {
        (PropertyKind::UniqueId(current), PropertyKind::UniqueId(config)) => {
            PropertyKind::UniqueId(UniqueIdConfig {
                next: current.next,
                ..config
            })
        }
        (_, kind) => kind,
    }
}

/// A new property of `kind` (counting from 1), unless the database has a unique ID already.
pub(super) fn fresh(
    properties: &[Property],
    kind: PropertyKind,
) -> Result<PropertyKind, DatabaseError> {
    match kind {
        PropertyKind::UniqueId(config) => {
            if properties
                .iter()
                .any(|property| matches!(property.kind, PropertyKind::UniqueId(_)))
            {
                return Err(DatabaseError::invalid("type"));
            }
            Ok(PropertyKind::UniqueId(UniqueIdConfig {
                next: first(),
                ..config
            }))
        }
        kind => Ok(kind),
    }
}

/// Takes the unique ID numbers out of an import's values; the import keeps them.
pub(super) fn take_imported(
    properties: &[Property],
    values: &mut Vec<(Id, Value)>,
) -> Vec<(Id, i64)> {
    let mut numbers = Vec::new();
    values.retain(|(property_id, raw)| {
        let unique = properties.iter().any(|property| {
            property.id == *property_id && matches!(property.kind, PropertyKind::UniqueId(_))
        });
        if unique && let Some(number) = raw.as_i64().filter(|number| *number > 0) {
            numbers.push((*property_id, number));
        }
        !unique
    });
    numbers
}

/// Numbers the rows of `database_id` that have no number (trashed ones too), by `created_at`
/// then id, from the larger of `next` and the highest stored number + 1. Updates the property in
/// `properties` and returns the numbered rows (their search text is stale).
pub(super) async fn number_rows(
    tx: &mut Transaction<'_, Any>,
    workspace_id: Id,
    database_id: Id,
    properties: &mut [Property],
    actor_id: Id,
    now: TimestampMillis,
) -> Result<Vec<Id>, DatabaseError> {
    let Some(property) = properties
        .iter_mut()
        .find(|property| matches!(property.kind, PropertyKind::UniqueId(_)))
    else {
        return Ok(Vec::new());
    };
    let PropertyKind::UniqueId(config) = &mut property.kind else {
        return Ok(Vec::new());
    };
    let rows: Vec<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT pages.id FROM pages WHERE pages.workspace_id = $1 AND pages.parent_id = $2 \
         AND {NOT_TEMPLATE} AND NOT EXISTS (SELECT 1 FROM page_property_values \
         WHERE page_property_values.page_id = pages.id AND page_property_values.property_id = $3) \
         ORDER BY pages.created_at, pages.id"
    )))
    .bind(workspace_id.to_string())
    .bind(database_id.to_string())
    .bind(property.id.to_string())
    .fetch_all(&mut **tx)
    .await?;
    let highest: Option<i64> = sqlx::query_scalar(
        "SELECT CAST(MAX(CAST(value_json AS BIGINT)) AS BIGINT) FROM page_property_values \
         WHERE property_id = $1",
    )
    .bind(property.id.to_string())
    .fetch_one(&mut **tx)
    .await?;
    let start = config.next.max(highest.map_or(1, |highest| highest + 1));
    let mut numbered = Vec::with_capacity(rows.len());
    let mut next = start;
    for row in rows {
        upsert_value(
            tx,
            workspace_id,
            &row,
            property.id,
            &CellValue::UniqueId(next),
            actor_id,
            now,
        )
        .await?;
        numbered.push(row.parse().map_err(|_| DatabaseError::Corrupt)?);
        next += 1;
    }
    if next != config.next {
        config.next = next;
        sqlx::query("UPDATE database_properties SET config_json = $1 WHERE id = $2")
            .bind(json!(config).to_string())
            .bind(property.id.to_string())
            .execute(&mut **tx)
            .await?;
    }
    Ok(numbered)
}

/// Drops the number of `page_id` (a copy or a template is not the row it came from); a row gets
/// a new one from [`number_rows`].
pub(in crate::repositories) async fn clear(
    tx: &mut Transaction<'_, Any>,
    database_id: Id,
    page_id: Id,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "DELETE FROM page_property_values WHERE page_id = $1 AND property_id IN \
         (SELECT id FROM database_properties WHERE database_id = $2 AND type = 'unique_id')",
    )
    .bind(page_id.to_string())
    .bind(database_id.to_string())
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Keeps the numbers an import brought for `row` (just made, so its own number was never seen):
/// the value becomes the import's, and `next` stays above it.
pub(super) async fn keep_imported(
    tx: &mut Transaction<'_, Any>,
    workspace_id: Id,
    database_id: Id,
    row: &mut DatabaseRow,
    numbers: Vec<(Id, i64)>,
    actor_id: Id,
    now: TimestampMillis,
) -> Result<(), DatabaseError> {
    let Some((property_id, number)) = numbers.into_iter().next() else {
        return Ok(());
    };
    let mut properties = properties_in_tx(tx, database_id).await?;
    let Some(PropertyKind::UniqueId(config)) = properties
        .iter_mut()
        .find(|property| property.id == property_id)
        .map(|property| &mut property.kind)
    else {
        return Ok(());
    };
    let given = row
        .values
        .get(&property_id.to_string())
        .and_then(Value::as_i64)
        .unwrap_or(config.next);
    config.next = given.max(number + 1);
    let config = json!(config).to_string();
    sqlx::query("UPDATE database_properties SET config_json = $1 WHERE id = $2")
        .bind(config)
        .bind(property_id.to_string())
        .execute(&mut **tx)
        .await?;
    upsert_value(
        tx,
        workspace_id,
        &row.id.to_string(),
        property_id,
        &CellValue::UniqueId(number),
        actor_id,
        now,
    )
    .await?;
    row.values.insert(property_id.to_string(), json!(number));
    refresh_search_text(
        tx,
        workspace_id,
        database_id,
        &properties,
        SearchRows::One(row.id),
    )
    .await?;
    Ok(())
}

/// A copy of a row (a duplicate, a restore) stays in `database_id` as a new row: it gets a new
/// number instead of the copied one.
pub(in crate::repositories) async fn renumber_copy(
    tx: &mut Transaction<'_, Any>,
    workspace_id: Id,
    database_id: Id,
    page_id: Id,
    actor_id: Id,
    now: TimestampMillis,
) -> Result<(), PageError> {
    clear(tx, database_id, page_id).await?;
    let mut properties = properties_in_tx(tx, database_id)
        .await
        .map_err(page_error)?;
    if !number_rows(
        tx,
        workspace_id,
        database_id,
        &mut properties,
        actor_id,
        now,
    )
    .await
    .map_err(page_error)?
    .is_empty()
    {
        refresh_search_text(tx, workspace_id, database_id, &properties, SearchRows::One(page_id))
            .await
            .map_err(page_error)?;
    }
    Ok(())
}
