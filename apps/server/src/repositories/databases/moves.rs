//! Pages moving into and out of a database (spec P2 item 8): what happens to a page's property
//! values when its database changes.

use orbit_platform::Row;
use orbit_platform::{Id, TimestampMillis};
use serde_json::{Value, json};
use sqlx::{Any, Transaction};

use super::search::{SearchRows, refresh_search_text};
use super::{
    CellValue, DatabaseError, Property, PropertyKind, page_error, parse_id, parse_value,
    properties_in_tx, record_with, upsert_value,
};
use crate::repositories::pages::PageError;

/// Inside the move transaction, after the page has its new parent: carries the values of
/// `page_id` from database `from` to database `to` (`None`: not a row). A value moves when `to`
/// has a property with the same name (case ignored) and type whose options hold it (by id or
/// name); every other value is deleted. Refreshes the row's search text (`NULL` outside a
/// database) and records a realtime event on each database. Returns how many values were deleted.
#[allow(clippy::too_many_arguments)]
pub(in crate::repositories) async fn carry_row_values(
    tx: &mut Transaction<'_, Any>,
    workspace_id: Id,
    page_id: Id,
    from: Option<Id>,
    to: Option<Id>,
    actor_id: Id,
    request_id: &str,
    now: TimestampMillis,
) -> Result<u64, PageError> {
    if from == to {
        return Ok(0);
    }
    move_values(
        tx,
        workspace_id,
        page_id,
        from,
        to,
        actor_id,
        request_id,
        now,
    )
    .await
    .map_err(page_error)
}

#[allow(clippy::too_many_arguments)]
async fn move_values(
    tx: &mut Transaction<'_, Any>,
    workspace_id: Id,
    page_id: Id,
    from: Option<Id>,
    to: Option<Id>,
    actor_id: Id,
    request_id: &str,
    now: TimestampMillis,
) -> Result<u64, DatabaseError> {
    let source = match from {
        Some(database_id) => properties_in_tx(tx, database_id).await?,
        None => Vec::new(),
    };
    let target = match to {
        Some(database_id) => properties_in_tx(tx, database_id).await?,
        None => Vec::new(),
    };
    let stored = sqlx::query(
        "SELECT property_id, value_json FROM page_property_values WHERE page_id = $1 ORDER BY property_id",
    )
    .bind(page_id.to_string())
    .fetch_all(&mut **tx)
    .await?;
    let mut kept = Vec::new();
    for row in &stored {
        let property_id = parse_id(&row.get::<String, _>("property_id"))?;
        let raw: Value = serde_json::from_str(&row.get::<String, _>("value_json"))
            .map_err(|_| DatabaseError::Corrupt)?;
        let carried = source
            .iter()
            .find(|property| property.id == property_id)
            .and_then(|property| carry(property, &raw, &target));
        kept.extend(carried);
    }
    sqlx::query("DELETE FROM page_property_values WHERE page_id = $1")
        .bind(page_id.to_string())
        .execute(&mut **tx)
        .await?;
    let row = page_id.to_string();
    for (property_id, value) in &kept {
        upsert_value(tx, workspace_id, &row, *property_id, value, actor_id, now).await?;
    }
    match to {
        Some(database_id) => {
            refresh_search_text(
                tx,
                workspace_id,
                database_id,
                &target,
                SearchRows::One(page_id),
            )
            .await?;
        }
        None => {
            sqlx::query("UPDATE pages SET property_text = NULL WHERE id = $1")
                .bind(&row)
                .execute(&mut **tx)
                .await?;
        }
    }
    for (database_id, action) in [(from, "database.row_removed"), (to, "database.row_added")] {
        if let Some(database_id) = database_id {
            record_with(
                tx,
                workspace_id,
                actor_id,
                action,
                database_id,
                json!({ "row_id": row }),
                request_id,
                now,
            )
            .await?;
        }
    }
    Ok((stored.len() - kept.len()) as u64)
}

/// The value `raw` of `property` as a value of the matching property of `target`, if it has one.
fn carry(property: &Property, raw: &Value, target: &[Property]) -> Option<(Id, CellValue)> {
    let value = parse_value(&property.kind, raw).ok()??;
    let name = property.name.to_lowercase();
    let into = target.iter().find(|candidate| {
        candidate.kind.property_type() == property.kind.property_type()
            && candidate.name.to_lowercase() == name
    })?;
    let option = |id: &String| option_in(&property.kind, &into.kind, id);
    let value = match value {
        CellValue::Choice(id) => CellValue::Choice(option(&id)?),
        CellValue::Choices(ids) => {
            let ids: Vec<String> = ids.iter().filter_map(option).collect();
            if ids.is_empty() {
                return None;
            }
            CellValue::Choices(ids)
        }
        other => other,
    };
    let value = parse_value(&into.kind, &value.to_json()).ok()??;
    Some((into.id, value))
}

/// The option of `to` for option `id` of `from`: the same id, else the same name.
fn option_in(from: &PropertyKind, to: &PropertyKind, id: &str) -> Option<String> {
    let options = to.options();
    if let Some((same, _)) = options.iter().find(|(option, _)| *option == id) {
        return Some((*same).to_owned());
    }
    let name = from
        .options()
        .into_iter()
        .find(|(option, _)| *option == id)?
        .1;
    options
        .into_iter()
        .find(|(_, option)| *option == name)
        .map(|(option, _)| option.to_owned())
}
