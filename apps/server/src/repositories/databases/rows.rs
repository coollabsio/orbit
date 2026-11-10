//! A database's rows: the repository methods that create, change, trash and move them, and the
//! queries that read rows and write their values.

use std::collections::{BTreeMap, HashMap, HashSet};

use orbit_platform::Row;
use orbit_platform::{Database, Id, TimestampMillis};
use serde_json::{Value, json};
use sqlx::{Any, Transaction};

use super::search::{SearchRows, refresh_search_text};
use super::templates::Children;
use super::{
    CellValue, ComputeScope, DatabaseError, DatabaseRepository, DatabaseRow, NOT_TEMPLATE, NewRow,
    Property, PropertyKind, RowPlacement, ensure_row_capacity, open, parse_id, parse_value,
    properties_in_tx, record_with,
};
use super::{computed, files, relations, search, templates, unique_id};
use crate::billing::TrialLimit;
use crate::collab::CollabHub;
use crate::repositories::billing::ensure_trial_capacity;
use crate::repositories::pages::renumber;

impl DatabaseRepository {
    pub async fn create_row(
        &self,
        workspace_id: Id,
        database_id: Id,
        actor_id: Id,
        input: NewRow,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<DatabaseRow, DatabaseError> {
        self.insert_row(
            Id::new_v7(),
            workspace_id,
            database_id,
            actor_id,
            input,
            None,
            request_id,
            now,
        )
        .await
    }

    /// [`Self::create_row`] with a caller-chosen id, so a resumable job (the Notion import) can
    /// record the id before the row exists. Unique ID numbers in `input` are kept.
    #[allow(clippy::too_many_arguments)]
    pub async fn create_row_with_id(
        &self,
        row_id: Id,
        workspace_id: Id,
        database_id: Id,
        actor_id: Id,
        mut input: NewRow,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<DatabaseRow, DatabaseError> {
        let mut tx = self.database.immediate_transaction().await?;
        let properties = properties_in_tx(&mut tx, database_id).await?;
        let numbers = unique_id::take_imported(&properties, &mut input.values);
        let mut row = Self::insert_row_in_tx(
            &self.database,
            &mut tx,
            row_id,
            workspace_id,
            database_id,
            actor_id,
            input,
            None,
            request_id,
            now,
        )
        .await?;
        unique_id::keep_imported(
            &mut tx,
            workspace_id,
            database_id,
            &mut row,
            numbers,
            actor_id,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(row)
    }

    /// A new row, filled from `template` (its id and the day "today" means) when given.
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn insert_row(
        &self,
        row_id: Id,
        workspace_id: Id,
        database_id: Id,
        actor_id: Id,
        input: NewRow,
        template: Option<(Id, String)>,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<DatabaseRow, DatabaseError> {
        let mut tx = self.database.immediate_transaction().await?;
        let row = Self::insert_row_in_tx(
            &self.database,
            &mut tx,
            row_id,
            workspace_id,
            database_id,
            actor_id,
            input,
            template,
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(row)
    }

    /// [`Self::insert_row`] inside the caller's write transaction (a button's `add_row`).
    #[allow(clippy::too_many_arguments)]
    pub(in crate::repositories) async fn insert_row_in_tx(
        database: &Database,
        tx: &mut Transaction<'_, Any>,
        row_id: Id,
        workspace_id: Id,
        database_id: Id,
        actor_id: Id,
        mut input: NewRow,
        template: Option<(Id, String)>,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<DatabaseRow, DatabaseError> {
        let page = open(tx, workspace_id, database_id, actor_id).await?;
        ensure_row_capacity(tx, database_id, 1).await?;
        ensure_trial_capacity(database, tx, workspace_id, TrialLimit::Pages, 1).await?;
        let mut properties = properties_in_tx(tx, database_id).await?;
        let fill = match template {
            Some((template_id, today)) => Some(
                templates::fill(
                    tx,
                    workspace_id,
                    database_id,
                    template_id,
                    actor_id,
                    &today,
                    &properties,
                )
                .await?,
            ),
            None => None,
        };
        if let Some(fill) = &fill {
            fill.apply(&mut input);
        }
        let mut values = parse_values(tx, workspace_id, &properties, &input.values).await?;
        let space = page.space(actor_id);
        let mut siblings = templates::row_ids(tx, workspace_id, space, database_id, row_id).await?;
        let index = placement_index(&siblings, input.placement)?;
        siblings.insert(index, row_id);
        let search_values: HashMap<Id, CellValue> = values
            .iter()
            .filter_map(|(id, value)| Some((*id, value.clone()?)))
            .collect();
        sqlx::query(
            "INSERT INTO pages (id, workspace_id, parent_id, teamspace_id, owner_id, title, icon, position, \
             creator_id, updated_by, version, created_at, updated_at, content_json, content_text, property_text) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, 0, $11, $12, '[]', '', $13)",
        )
        .bind(row_id.to_string())
        .bind(workspace_id.to_string())
        .bind(database_id.to_string())
        .bind(space.teamspace_column())
        .bind(space.owner_column())
        .bind(&input.title)
        .bind(&input.icon)
        .bind(index as i64)
        .bind(actor_id.to_string())
        .bind(actor_id.to_string())
        .bind(now.as_millis())
        .bind(now.as_millis())
        .bind(search::search_text(&properties, &search_values))
        .execute(&mut **tx)
        .await?;
        renumber(tx, &siblings).await?;
        if let Some(fill) = &fill {
            let copies = templates::copy_into_row(
                database,
                tx,
                workspace_id,
                fill,
                row_id,
                space,
                actor_id,
                request_id,
                now,
            )
            .await?;
            files::remap_values(&mut values, fill.page_id, &copies);
        }
        files::check_page_files(tx, workspace_id, row_id, &mut values).await?;
        let row = row_id.to_string();
        for (property_id, value) in &values {
            if let Some(CellValue::Pages(ids)) = value {
                relations::write_cell(tx, workspace_id, actor_id, row_id, *property_id, ids, now)
                    .await?;
                continue;
            }
            if let Some(value) = value {
                upsert_value(tx, workspace_id, &row, *property_id, value, actor_id, now).await?;
            }
        }
        let numbered = unique_id::number_rows(
            tx,
            workspace_id,
            database_id,
            &mut properties,
            actor_id,
            now,
        )
        .await?;
        if !numbered.is_empty() {
            let rows = if numbered == [row_id] {
                SearchRows::One(row_id)
            } else {
                SearchRows::All
            };
            refresh_search_text(tx, workspace_id, database_id, &properties, rows).await?;
        }
        record_with(
            tx,
            workspace_id,
            actor_id,
            "database.row_created",
            database_id,
            json!({ "row_id": row }),
            request_id,
            now,
        )
        .await?;
        let scope = ComputeScope::utc(workspace_id, actor_id);
        single_row(tx, &scope, database_id, row_id).await
    }

    /// Sets (or clears, with `null`) values of a row. Last write wins; the row's `updated_*`
    /// change, its version does not.
    #[allow(clippy::too_many_arguments)]
    pub async fn update_values(
        &self,
        workspace_id: Id,
        database_id: Id,
        row_id: Id,
        actor_id: Id,
        input: Vec<(Id, Value)>,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<DatabaseRow, DatabaseError> {
        let mut tx = self.database.immediate_transaction().await?;
        open(&mut tx, workspace_id, database_id, actor_id).await?;
        // A template's values are written here too (its "me" and "today" kept aside).
        let children = templates::child_kind(&mut tx, database_id, row_id)
            .await?
            .ok_or(DatabaseError::NotFound)?;
        let properties = properties_in_tx(&mut tx, database_id).await?;
        let (input, placeholders) = match children {
            Children::Templates => templates::split_placeholders(&properties, input),
            Children::Rows => (input, Vec::new()),
        };
        let mut values = parse_values(&mut tx, workspace_id, &properties, &input).await?;
        files::check_page_files(&mut tx, workspace_id, row_id, &mut values).await?;
        write_values(&mut tx, workspace_id, row_id, &values, actor_id, now).await?;
        templates::write_placeholders(&mut tx, row_id, placeholders, now).await?;
        relations::notify_dependents(
            &mut tx,
            workspace_id,
            actor_id,
            database_id,
            request_id,
            now,
        )
        .await?;
        refresh_search_text(
            &mut tx,
            workspace_id,
            database_id,
            &properties,
            SearchRows::One(row_id),
        )
        .await?;
        record_with(
            &mut tx,
            workspace_id,
            actor_id,
            "database.values_updated",
            database_id,
            json!({
                "row_id": row_id.to_string(),
                "property_ids": values.iter().map(|(id, _)| id.to_string()).collect::<Vec<_>>(),
            }),
            request_id,
            now,
        )
        .await?;
        let scope = ComputeScope::utc(workspace_id, actor_id);
        let row = match children {
            Children::Rows => single_row(&mut tx, &scope, database_id, row_id).await?,
            Children::Templates => {
                templates::template_row(&mut tx, &scope, database_id, row_id).await?
            }
        };
        tx.commit().await?;
        Ok(row)
    }

    /// Trashes live rows of the database, each with its sub-pages as its own trash batch (so each
    /// one restores on its own, back into the database). Ids that are not live rows of this
    /// database are skipped, so a retry converges.
    pub async fn trash_rows(
        &self,
        workspace_id: Id,
        database_id: Id,
        actor_id: Id,
        row_ids: &[Id],
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), DatabaseError> {
        let mut tx = self.database.immediate_transaction().await?;
        let page = open(&mut tx, workspace_id, database_id, actor_id).await?;
        let space = page.space(actor_id);
        let live: HashSet<Id> =
            templates::row_ids(&mut tx, workspace_id, space, database_id, database_id)
                .await?
                .into_iter()
                .collect();
        let mut trashed = 0;
        for row_id in row_ids.iter().filter(|id| live.contains(id)) {
            trashed += sqlx::query(
                "WITH RECURSIVE subtree(id) AS ( \
                     SELECT $1 \
                     UNION ALL \
                     SELECT pages.id FROM pages JOIN subtree ON pages.parent_id = subtree.id \
                     WHERE pages.deleted_at IS NULL \
                 ) \
                 UPDATE pages SET deleted_at = $2, trashed_with = $3, updated_by = $4, updated_at = $5, version = version + 1 \
                 WHERE workspace_id = $6 AND deleted_at IS NULL AND id IN (SELECT id FROM subtree)",
            )
            .bind(row_id.to_string())
            .bind(now.as_millis())
            .bind(row_id.to_string())
            .bind(actor_id.to_string())
            .bind(now.as_millis())
            .bind(workspace_id.to_string())
            .execute(&mut *tx)
            .await?
            .rows_affected();
        }
        if trashed == 0 {
            tx.rollback().await?;
            return Ok(());
        }
        let siblings =
            templates::row_ids(&mut tx, workspace_id, space, database_id, database_id).await?;
        renumber(&mut tx, &siblings).await?;
        record_with(
            &mut tx,
            workspace_id,
            actor_id,
            "database.rows_trashed",
            database_id,
            json!({ "pages": trashed }),
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        CollabHub::revalidate_database(&self.database, Some(workspace_id));
        Ok(())
    }

    /// Puts a row first (`None`) or right after another row (manual order), and writes `values`
    /// in the same transaction (a board card moved to another column).
    #[allow(clippy::too_many_arguments)]
    pub async fn move_row(
        &self,
        workspace_id: Id,
        database_id: Id,
        row_id: Id,
        actor_id: Id,
        after: Option<Id>,
        values: Vec<(Id, Value)>,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<DatabaseRow, DatabaseError> {
        let mut tx = self.database.immediate_transaction().await?;
        let page = open(&mut tx, workspace_id, database_id, actor_id).await?;
        require_row(&mut tx, database_id, row_id).await?;
        if !values.is_empty() {
            let properties = properties_in_tx(&mut tx, database_id).await?;
            let values = parse_values(&mut tx, workspace_id, &properties, &values).await?;
            write_values(&mut tx, workspace_id, row_id, &values, actor_id, now).await?;
            refresh_search_text(
                &mut tx,
                workspace_id,
                database_id,
                &properties,
                SearchRows::One(row_id),
            )
            .await?;
        }
        let mut siblings = templates::row_ids(
            &mut tx,
            workspace_id,
            page.space(actor_id),
            database_id,
            row_id,
        )
        .await?;
        let placement = after.map_or(RowPlacement::First, RowPlacement::After);
        let index = placement_index(&siblings, placement)?;
        siblings.insert(index, row_id);
        renumber(&mut tx, &siblings).await?;
        record_with(
            &mut tx,
            workspace_id,
            actor_id,
            "database.row_moved",
            database_id,
            json!({ "row_id": row_id.to_string() }),
            request_id,
            now,
        )
        .await?;
        let scope = ComputeScope::utc(workspace_id, actor_id);
        let row = single_row(&mut tx, &scope, database_id, row_id).await?;
        tx.commit().await?;
        Ok(row)
    }
}

pub(super) async fn require_row(
    tx: &mut Transaction<'_, Any>,
    database_id: Id,
    row_id: Id,
) -> Result<(), DatabaseError> {
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT 1 FROM pages WHERE id = $1 AND parent_id = $2 AND deleted_at IS NULL AND {NOT_TEMPLATE}"
    )))
        .bind(row_id.to_string())
        .bind(database_id.to_string())
        .fetch_optional(&mut **tx)
        .await?
        .map(|_| ())
        .ok_or(DatabaseError::NotFound)
}

pub(super) fn insert_index(position: Option<i64>, len: usize) -> usize {
    position.map_or(len, |position| {
        usize::try_from(position.max(0)).unwrap_or(len).min(len)
    })
}

fn placement_index(siblings: &[Id], placement: RowPlacement) -> Result<usize, DatabaseError> {
    match placement {
        RowPlacement::Last => Ok(siblings.len()),
        RowPlacement::First => Ok(0),
        RowPlacement::After(after) => siblings
            .iter()
            .position(|id| *id == after)
            .map(|index| index + 1)
            .ok_or_else(|| DatabaseError::invalid("after_row_id")),
    }
}

/// Parses each value against its property (and person ids against the members), so the write
/// itself cannot fail on input. Errors name `values.<property id>`.
pub(super) async fn parse_values(
    tx: &mut Transaction<'_, Any>,
    workspace_id: Id,
    properties: &[Property],
    input: &[(Id, Value)],
) -> Result<Vec<(Id, Option<CellValue>)>, DatabaseError> {
    let mut members: Option<HashSet<String>> = None;
    let mut values = Vec::with_capacity(input.len());
    for (property_id, raw) in input {
        let path = || DatabaseError::invalid(format!("values.{property_id}"));
        let property = properties
            .iter()
            .find(|property| property.id == *property_id)
            .ok_or_else(path)?;
        // Unique ID numbers are the server's.
        if matches!(property.kind, PropertyKind::UniqueId(_)) {
            return Err(path());
        }
        let value = parse_value(&property.kind, raw).map_err(|()| path())?;
        // An empty relation cell still replaces its visible links.
        let value = match (&property.kind, value) {
            (PropertyKind::Relation(_), None) => Some(CellValue::Pages(Vec::new())),
            (_, value) => value,
        };
        if let Some(CellValue::People(people)) = &value {
            if members.is_none() {
                members = Some(
                    sqlx::query_scalar::<_, String>(
                        "SELECT user_id FROM memberships WHERE workspace_id = $1 AND role <> 'guest'",
                    )
                    .bind(workspace_id.to_string())
                    .fetch_all(&mut **tx)
                    .await?
                    .into_iter()
                    .collect(),
                );
            }
            let members = members.as_ref().ok_or(DatabaseError::Corrupt)?;
            if !people.iter().all(|id| members.contains(&id.to_string())) {
                return Err(path());
            }
        }
        values.push((*property_id, value));
    }
    Ok(values)
}

/// Writes parsed values (`None` clears) and stamps the row's `updated_at` and `updated_by`.
pub(super) async fn write_values(
    tx: &mut Transaction<'_, Any>,
    workspace_id: Id,
    row_id: Id,
    values: &[(Id, Option<CellValue>)],
    actor_id: Id,
    now: TimestampMillis,
) -> Result<(), DatabaseError> {
    let row = row_id.to_string();
    for (property_id, value) in values {
        match value {
            Some(CellValue::Pages(ids)) => {
                relations::write_cell(tx, workspace_id, actor_id, row_id, *property_id, ids, now)
                    .await?;
            }
            Some(value) => {
                upsert_value(tx, workspace_id, &row, *property_id, value, actor_id, now).await?;
            }
            None => delete_value(tx, &row, *property_id).await?,
        }
    }
    sqlx::query(
            "UPDATE pages SET updated_by = $1, updated_at = CASE WHEN updated_at < $2 THEN $3 ELSE updated_at + 1 END \
             WHERE id = $4",
        )
        .bind(actor_id.to_string())
        .bind(now.as_millis())
        .bind(now.as_millis())
        .bind(&row)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

pub(super) async fn upsert_value(
    tx: &mut Transaction<'_, Any>,
    workspace_id: Id,
    row: &str,
    property_id: Id,
    value: &CellValue,
    actor_id: Id,
    now: TimestampMillis,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO page_property_values (page_id, property_id, workspace_id, value_json, updated_by, updated_at) \
         VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT (page_id, property_id) DO UPDATE \
         SET value_json = excluded.value_json, updated_by = excluded.updated_by, updated_at = excluded.updated_at",
    )
    .bind(row)
    .bind(property_id.to_string())
    .bind(workspace_id.to_string())
    .bind(value.to_json().to_string())
    .bind(actor_id.to_string())
    .bind(now.as_millis())
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Replaces a stored value without touching who changed it last (conversions, option removal).
pub(super) async fn update_value(
    tx: &mut Transaction<'_, Any>,
    row: &str,
    property_id: Id,
    value: &CellValue,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE page_property_values SET value_json = $1 WHERE page_id = $2 AND property_id = $3",
    )
    .bind(value.to_json().to_string())
    .bind(row)
    .bind(property_id.to_string())
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub(super) async fn delete_value(
    tx: &mut Transaction<'_, Any>,
    row: &str,
    property_id: Id,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM page_property_values WHERE page_id = $1 AND property_id = $2")
        .bind(row)
        .bind(property_id.to_string())
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Live rows in manual order (one row with `only`), with their stored values.
pub(super) async fn rows_in_tx(
    tx: &mut Transaction<'_, Any>,
    database_id: Id,
    only: Option<Id>,
) -> Result<Vec<DatabaseRow>, DatabaseError> {
    children_in_tx(tx, database_id, Children::Rows, only).await
}

/// Live rows or templates in manual order (one with `only`), with their stored values.
pub(super) async fn children_in_tx(
    tx: &mut Transaction<'_, Any>,
    database_id: Id,
    children: Children,
    only: Option<Id>,
) -> Result<Vec<DatabaseRow>, DatabaseError> {
    let only = only.map(|id| id.to_string());
    let filter = children.filter();
    let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT id, title, icon, position, version, created_at, creator_id, updated_at, updated_by, \
         content_text <> '' AS has_content FROM pages \
         WHERE parent_id = $1 AND deleted_at IS NULL AND ($2 IS NULL OR id = $3) AND {filter} \
         ORDER BY position, id"
    )))
    .bind(database_id.to_string())
    .bind(&only)
    .bind(&only)
    .fetch_all(&mut **tx)
    .await?;
    let stored = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT page_property_values.page_id AS page_id, page_property_values.property_id AS property_id, \
         page_property_values.value_json AS value_json FROM page_property_values \
         JOIN pages ON pages.id = page_property_values.page_id \
         WHERE pages.parent_id = $1 AND pages.deleted_at IS NULL AND ($2 IS NULL OR pages.id = $3) \
         AND {filter}"
    )))
    .bind(database_id.to_string())
    .bind(&only)
    .bind(&only)
    .fetch_all(&mut **tx)
    .await?;
    let mut values: HashMap<String, BTreeMap<String, Value>> = HashMap::new();
    for row in stored {
        let value: Value = serde_json::from_str(&row.get::<String, _>("value_json"))
            .map_err(|_| DatabaseError::Corrupt)?;
        values
            .entry(row.get("page_id"))
            .or_default()
            .insert(row.get("property_id"), value);
    }
    rows.into_iter()
        .map(|row| {
            let id: String = row.get("id");
            Ok(DatabaseRow {
                id: parse_id(&id)?,
                title: row.get("title"),
                icon: row.get("icon"),
                position: row.get("position"),
                version: u64::try_from(row.get::<i64, _>("version"))
                    .map_err(|_| DatabaseError::Corrupt)?,
                created_at: TimestampMillis::from_millis(row.get("created_at")),
                created_by: parse_id(&row.get::<String, _>("creator_id"))?,
                updated_at: TimestampMillis::from_millis(row.get("updated_at")),
                updated_by: parse_id(&row.get::<String, _>("updated_by"))?,
                has_content: row.get("has_content"),
                values: values.remove(&id).unwrap_or_default(),
                hidden_links: BTreeMap::new(),
                preview: None,
            })
        })
        .collect()
}

/// One live row with its computed values (in UTC), as a write returns it.
pub(super) async fn single_row(
    tx: &mut Transaction<'_, Any>,
    scope: &ComputeScope,
    database_id: Id,
    row_id: Id,
) -> Result<DatabaseRow, DatabaseError> {
    let mut rows = rows_in_tx(tx, database_id, Some(row_id)).await?;
    if rows.is_empty() {
        return Err(DatabaseError::NotFound);
    }
    let properties = properties_in_tx(tx, database_id).await?;
    computed::fill(tx, scope, database_id, &properties, &mut rows).await?;
    rows.pop().ok_or(DatabaseError::NotFound)
}
