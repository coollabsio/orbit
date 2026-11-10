//! Property values in docs search (spec P2 item 4). `pages.property_text` holds a row's searchable
//! values, one line per property; the `page_search` triggers copy it into the `props` column of
//! the index. Every write that changes a value or a property's schema rewrites it in its own
//! transaction; `fill_search_text` computes the rows still `NULL` after migration 0083 or a
//! restore. Person names are not indexed (D-P2-6): a member's rename writes no value.

use std::collections::HashMap;
use std::collections::hash_map::Entry;

use orbit_platform::{Database, Id, Row};
use serde_json::Value;
use sqlx::{Any, Transaction};

use super::{
    CellValue, DatabaseError, DatabaseRepository, Property, PropertyKind, StatusGroup,
    display_text, parse_id, parse_value, properties_in_tx,
};

/// Rows one backfill transaction computes.
pub const SEARCH_TEXT_BATCH: i64 = 500;

/// A row's searchable values: `(property name, text)` per property, in property order. Text,
/// URL, number (as written), the option names of select, multi-select and status (a status
/// without a value is its default option, as clients show it) and dates as ISO 8601. Person,
/// checkbox and the computed types are left out. Line breaks in a value become spaces.
pub(super) fn search_lines<'a>(
    properties: &'a [Property],
    values: &HashMap<Id, CellValue>,
) -> Vec<(&'a str, String)> {
    properties
        .iter()
        .filter_map(|property| {
            let value = values.get(&property.id);
            let text = match (&property.kind, value) {
                (
                    PropertyKind::Text
                    | PropertyKind::Url
                    | PropertyKind::Email
                    | PropertyKind::Phone
                    | PropertyKind::UniqueId(_)
                    | PropertyKind::Number(_)
                    | PropertyKind::Select(_)
                    | PropertyKind::MultiSelect(_)
                    | PropertyKind::Status(_)
                    | PropertyKind::Date(_),
                    Some(value),
                ) => display_text(&property.kind, value),
                (PropertyKind::Status(options), None) => options
                    .iter()
                    .find(|option| option.group == StatusGroup::Todo)
                    .map(|option| option.name.clone())?,
                _ => return None,
            };
            // One line per property, so a matched line names its property.
            let text = text.replace(['\n', '\r'], " ");
            (!text.is_empty()).then_some((property.name.as_str(), text))
        })
        .collect()
}

/// `pages.property_text` of a row: its search lines' text, one per line.
pub(super) fn search_text(properties: &[Property], values: &HashMap<Id, CellValue>) -> String {
    search_lines(properties, values)
        .into_iter()
        .map(|(_, text)| text)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Which rows of a database to recompute.
#[derive(Clone, Copy)]
pub(super) enum SearchRows {
    /// Every row, trashed ones too: a schema change.
    All,
    One(Id),
    /// Rows not computed yet, at most a batch.
    Missing,
}

/// Rewrites `property_text` of rows of `database_id` from their stored values; a row whose text
/// did not change is not written (each write updates the search index). Returns how many rows
/// were computed.
pub(super) async fn refresh_search_text(
    tx: &mut Transaction<'_, Any>,
    workspace_id: Id,
    database_id: Id,
    properties: &[Property],
    rows: SearchRows,
) -> Result<usize, DatabaseError> {
    let scope = "FROM pages WHERE pages.workspace_id = $1 AND pages.parent_id = $2";
    let page_sql = match rows {
        SearchRows::All => {
            format!("SELECT pages.id AS id, pages.property_text AS property_text {scope}")
        }
        SearchRows::One(_) => {
            format!(
                "SELECT pages.id AS id, pages.property_text AS property_text {scope} AND pages.id = $3"
            )
        }
        SearchRows::Missing => format!(
            "SELECT pages.id AS id, pages.property_text AS property_text {scope} \
             AND pages.property_text IS NULL ORDER BY pages.id LIMIT {SEARCH_TEXT_BATCH}"
        ),
    };
    let query = sqlx::query(sqlx::AssertSqlSafe(page_sql))
        .bind(workspace_id.to_string())
        .bind(database_id.to_string());
    let query = match rows {
        SearchRows::One(id) => query.bind(id.to_string()),
        SearchRows::All | SearchRows::Missing => query,
    };
    let pages = query.fetch_all(&mut **tx).await?;
    let Some(last) = pages.last().map(|page| page.get::<String, _>("id")) else {
        return Ok(0);
    };
    // The values of exactly these rows: one row, all, or the batch (missing rows up to its last id).
    let values_filter = match rows {
        SearchRows::All => "",
        SearchRows::One(_) => " AND pages.id = $3",
        SearchRows::Missing => " AND pages.property_text IS NULL AND pages.id <= $3",
    };
    let query = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT page_property_values.page_id AS page_id, page_property_values.property_id AS property_id, \
         page_property_values.value_json AS value_json FROM page_property_values \
         JOIN pages ON pages.id = page_property_values.page_id \
         WHERE pages.workspace_id = $1 AND pages.parent_id = $2{values_filter}"
    )))
    .bind(workspace_id.to_string())
    .bind(database_id.to_string());
    let query = match rows {
        SearchRows::All => query,
        SearchRows::One(_) | SearchRows::Missing => query.bind(last),
    };
    let stored = query.fetch_all(&mut **tx).await?;
    let mut values: HashMap<String, HashMap<Id, CellValue>> = HashMap::new();
    for row in stored {
        let property_id = parse_id(&row.get::<String, _>("property_id"))?;
        let Some(property) = properties
            .iter()
            .find(|property| property.id == property_id)
        else {
            continue;
        };
        let raw: Value = serde_json::from_str(&row.get::<String, _>("value_json"))
            .map_err(|_| DatabaseError::Corrupt)?;
        if let Ok(Some(value)) = parse_value(&property.kind, &raw) {
            values
                .entry(row.get("page_id"))
                .or_default()
                .insert(property_id, value);
        }
    }
    let empty = HashMap::new();
    for page in &pages {
        let id: String = page.get("id");
        let text = search_text(properties, values.get(&id).unwrap_or(&empty));
        if page.get::<Option<String>, _>("property_text").as_deref() != Some(text.as_str()) {
            sqlx::query("UPDATE pages SET property_text = $1 WHERE id = $2")
                .bind(text)
                .bind(id)
                .execute(&mut **tx)
                .await?;
        }
    }
    Ok(pages.len())
}

/// Computes `property_text` of each page in `pages` that is a row (others are left alone): pages
/// that got their values by copy (duplicate, restore of a backup item).
pub(super) async fn refresh_rows(
    tx: &mut Transaction<'_, Any>,
    pages: impl Iterator<Item = Id>,
) -> Result<(), DatabaseError> {
    let mut properties: HashMap<Id, Vec<Property>> = HashMap::new();
    for page_id in pages {
        let Some(row) = sqlx::query(
            "SELECT pages.workspace_id AS workspace_id, parent.id AS database_id FROM pages \
             JOIN pages AS parent ON parent.id = pages.parent_id AND parent.kind = 'database' \
             WHERE pages.id = $1",
        )
        .bind(page_id.to_string())
        .fetch_optional(&mut **tx)
        .await?
        else {
            continue;
        };
        let workspace_id = parse_id(&row.get::<String, _>("workspace_id"))?;
        let database_id = parse_id(&row.get::<String, _>("database_id"))?;
        let properties = match properties.entry(database_id) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => entry.insert(properties_in_tx(tx, database_id).await?),
        };
        refresh_search_text(
            tx,
            workspace_id,
            database_id,
            properties,
            SearchRows::One(page_id),
        )
        .await?;
    }
    Ok(())
}

impl DatabaseRepository {
    /// Computes `property_text` of up to a batch of rows that have none, all of one database, in
    /// one short transaction. Returns how many rows it computed: 0 when every row has one, so a
    /// caller repeats until then. Idempotent: a computed row is never picked again.
    pub async fn fill_search_text(&self) -> Result<usize, DatabaseError> {
        let mut tx = self.database.immediate_transaction().await?;
        let next = sqlx::query(
            "SELECT pages.workspace_id AS workspace_id, pages.parent_id AS database_id FROM pages \
             JOIN pages AS parent ON parent.id = pages.parent_id AND parent.kind = 'database' \
             WHERE pages.property_text IS NULL LIMIT 1",
        )
        .fetch_optional(&mut *tx)
        .await?;
        let Some(next) = next else {
            return Ok(0);
        };
        let workspace_id = parse_id(&next.get::<String, _>("workspace_id"))?;
        let database_id = parse_id(&next.get::<String, _>("database_id"))?;
        let properties = properties_in_tx(&mut tx, database_id).await?;
        let filled = refresh_search_text(
            &mut tx,
            workspace_id,
            database_id,
            &properties,
            SearchRows::Missing,
        )
        .await?;
        tx.commit().await?;
        Ok(filled)
    }
}

/// The search lines of each row in `rows` (non-rows have none), for result snippets.
pub async fn row_search_lines(
    database: &Database,
    rows: &[Id],
) -> Result<HashMap<Id, Vec<(String, String)>>, DatabaseError> {
    let mut lines = HashMap::new();
    if rows.is_empty() {
        return Ok(lines);
    }
    let mut tx = database.transaction().await?;
    let mut properties: HashMap<String, Vec<Property>> = HashMap::new();
    for row_id in rows {
        let Some(database_id) = sqlx::query_scalar::<_, String>(
            "SELECT parent.id FROM pages JOIN pages AS parent ON parent.id = pages.parent_id \
             AND parent.kind = 'database' WHERE pages.id = $1",
        )
        .bind(row_id.to_string())
        .fetch_optional(&mut *tx)
        .await?
        else {
            continue;
        };
        let properties = match properties.entry(database_id) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => {
                let id = parse_id(entry.key())?;
                entry.insert(properties_in_tx(&mut tx, id).await?)
            }
        };
        let mut values = HashMap::new();
        for row in sqlx::query(
            "SELECT property_id, value_json FROM page_property_values WHERE page_id = $1",
        )
        .bind(row_id.to_string())
        .fetch_all(&mut *tx)
        .await?
        {
            let property_id = parse_id(&row.get::<String, _>("property_id"))?;
            let Some(property) = properties
                .iter()
                .find(|property| property.id == property_id)
            else {
                continue;
            };
            let raw: Value = serde_json::from_str(&row.get::<String, _>("value_json"))
                .map_err(|_| DatabaseError::Corrupt)?;
            if let Ok(Some(value)) = parse_value(&property.kind, &raw) {
                values.insert(property_id, value);
            }
        }
        lines.insert(
            *row_id,
            search_lines(properties, &values)
                .into_iter()
                .map(|(name, text)| (name.to_owned(), text))
                .collect(),
        );
    }
    tx.commit().await?;
    Ok(lines)
}
