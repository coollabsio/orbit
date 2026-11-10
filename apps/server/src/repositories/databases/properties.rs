//! A database's properties: the repository methods that create, change, convert and delete
//! them, and the queries that read and write them.

use std::collections::HashMap;

use orbit_platform::Row;
use orbit_platform::{Id, TimestampMillis};
use serde_json::Value;
use sqlx::{Any, Transaction};

use super::search::{SearchRows, refresh_search_text};
use super::{
    CellValue, ComputeScope, DatabaseError, DatabaseProperty, DatabaseRepository, MAX_PROPERTIES,
    NewProperty, Property, PropertyChanges, PropertyKind, PropertyType, PropertyUpdate, ViewColumn,
    Views, convert, copy_name, delete_value, insert_index, open, parse_id, parse_value, raw_views,
    record, unlocked, update_value, upsert_value, write_view_config,
};
use super::{formula, relations, unique_id};

impl DatabaseRepository {
    /// The database's properties in order (the schema without views and rows).
    pub async fn properties(
        &self,
        workspace_id: Id,
        database_id: Id,
        actor_id: Id,
    ) -> Result<Vec<Property>, DatabaseError> {
        let mut tx = self.database.transaction().await?;
        open(&mut tx, workspace_id, database_id, actor_id).await?;
        let properties = properties_in_tx(&mut tx, database_id).await?;
        tx.rollback().await?;
        Ok(properties)
    }

    pub async fn create_property(
        &self,
        workspace_id: Id,
        database_id: Id,
        actor_id: Id,
        input: NewProperty,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<DatabaseProperty, DatabaseError> {
        let mut tx = self.database.immediate_transaction().await?;
        let page = open(&mut tx, workspace_id, database_id, actor_id).await?;
        unlocked(&page)?;
        let mut properties = properties_in_tx(&mut tx, database_id).await?;
        if properties.len() >= MAX_PROPERTIES {
            return Err(DatabaseError::PropertyLimit);
        }
        let (property, index, source) = match input {
            NewProperty::Fresh {
                name,
                property_type,
                config,
                position,
            } => {
                if property_type == PropertyType::Title {
                    return Err(DatabaseError::invalid("type"));
                }
                let kind = PropertyKind::parse(property_type, config.as_ref())
                    .map_err(DatabaseError::invalid)?;
                let kind = unique_id::fresh(&properties, kind)?;
                let property = Property {
                    id: Id::new_v7(),
                    name,
                    kind,
                    position: 0,
                };
                let mut property = property;
                let scope = ComputeScope::utc(workspace_id, actor_id);
                relations::check(&mut tx, &scope, database_id, &properties, &mut property).await?;
                (
                    formula::checked(&properties, property)?,
                    insert_index(position, properties.len()),
                    None,
                )
            }
            NewProperty::Duplicate { source, name } => {
                let at = properties
                    .iter()
                    .position(|property| property.id == source)
                    .ok_or_else(|| DatabaseError::invalid("duplicate_of"))?;
                let original = properties[at].clone();
                if matches!(
                    original.kind,
                    PropertyKind::Title | PropertyKind::UniqueId(_)
                ) {
                    return Err(DatabaseError::invalid("duplicate_of"));
                }
                let (kind, option_ids) = original.kind.with_new_option_ids();
                // A copied relation is one-way and starts without links.
                let kind = relations::copied_kind(&kind, &HashMap::new());
                let property = Property {
                    id: Id::new_v7(),
                    name: name.unwrap_or_else(|| copy_name(&original.name)),
                    kind,
                    position: 0,
                };
                (
                    formula::checked(&properties, property)?,
                    at + 1,
                    Some((original, option_ids)),
                )
            }
        };
        insert_property(&mut tx, workspace_id, database_id, &property, now).await?;
        properties.insert(index, property.clone());
        renumber_rows(&mut tx, "database_properties", &properties_ids(&properties)).await?;
        if let Some((original, option_ids)) = &source {
            for (row, value) in stored_values(&mut tx, original).await? {
                let value = value.with_option_ids(option_ids);
                upsert_value(
                    &mut tx,
                    workspace_id,
                    &row,
                    property.id,
                    &value,
                    actor_id,
                    now,
                )
                .await?;
            }
        }
        unique_id::number_rows(
            &mut tx,
            workspace_id,
            database_id,
            &mut properties,
            actor_id,
            now,
        )
        .await?;
        for mut view in raw_views(&mut tx, database_id, Views::All).await? {
            let columns = &mut view.config.columns;
            match source.as_ref().and_then(|(original, _)| {
                columns
                    .iter()
                    .position(|column| column.property_id == original.id)
            }) {
                Some(at) => {
                    let column = ViewColumn {
                        property_id: property.id,
                        ..columns[at].clone()
                    };
                    columns.insert(at + 1, column);
                }
                None => columns.push(property.column()),
            }
            write_view_config(&mut tx, view.id, &view.config, now).await?;
        }
        // A copy brings values, and a new status shows its default option on every row.
        refresh_search_text(
            &mut tx,
            workspace_id,
            database_id,
            &properties,
            SearchRows::All,
        )
        .await?;
        let scope = ComputeScope::utc(workspace_id, actor_id);
        properties[index].kind = relations::sync_pair(
            &mut tx,
            &scope,
            database_id,
            &page.title,
            &properties[index],
            None,
            request_id,
            now,
        )
        .await?;
        record(
            &mut tx,
            workspace_id,
            actor_id,
            "database.property_created",
            database_id,
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(DatabaseProperty {
            position: index as i64,
            ..properties[index].output()
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update_property(
        &self,
        workspace_id: Id,
        database_id: Id,
        property_id: Id,
        actor_id: Id,
        changes: PropertyChanges,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<PropertyUpdate, DatabaseError> {
        let mut tx = self.database.immediate_transaction().await?;
        let page = open(&mut tx, workspace_id, database_id, actor_id).await?;
        unlocked(&page)?;
        let mut properties = properties_in_tx(&mut tx, database_id).await?;
        let index = properties
            .iter()
            .position(|property| property.id == property_id)
            .ok_or(DatabaseError::PropertyNotFound)?;
        let current = properties[index].clone();
        let type_change = changes
            .property_type
            .filter(|property_type| *property_type != current.kind.property_type());
        let (mut converted, mut cleared) = (0, 0);
        let kind = match (type_change, &changes.config) {
            (Some(_), _) if current.kind == PropertyKind::Title => {
                return Err(DatabaseError::TitleProperty);
            }
            // Both need a config: create such a property instead.
            (Some(PropertyType::Relation | PropertyType::Rollup), _) => {
                return Err(DatabaseError::invalid("type"));
            }
            (Some(PropertyType::Title), _) => return Err(DatabaseError::invalid("type")),
            (Some(PropertyType::UniqueId), _)
                if properties
                    .iter()
                    .any(|property| matches!(property.kind, PropertyKind::UniqueId(_))) =>
            {
                return Err(DatabaseError::invalid("type"));
            }
            // A formula stores no values, so its expression can come with the type change.
            (Some(PropertyType::Formula), Some(config)) => {
                for (row, _) in stored_values(&mut tx, &current).await? {
                    cleared += 1;
                    delete_value(&mut tx, &row, property_id).await?;
                }
                PropertyKind::parse(PropertyType::Formula, Some(config))
                    .map_err(DatabaseError::invalid)?
            }
            // A type change derives the new config; set it in a second request.
            (Some(_), Some(_)) => return Err(DatabaseError::invalid("config")),
            (Some(new_type), None) => {
                relations::detach(&mut tx, &current, false).await?;
                let values = stored_values(&mut tx, &current).await?;
                let conversion = convert(&current.kind, new_type, values);
                for (row, value) in &conversion.values {
                    match value {
                        Some(value) => {
                            converted += 1;
                            update_value(&mut tx, row, property_id, value).await?;
                        }
                        None => {
                            cleared += 1;
                            delete_value(&mut tx, row, property_id).await?;
                        }
                    }
                }
                conversion.kind
            }
            (None, Some(config)) => {
                let kind = PropertyKind::parse(current.kind.property_type(), Some(config))
                    .map_err(DatabaseError::invalid)?;
                let kind = unique_id::keep_next(&current.kind, kind);
                let mut changed = Property {
                    kind,
                    ..current.clone()
                };
                let scope = ComputeScope::utc(workspace_id, actor_id);
                relations::check(&mut tx, &scope, database_id, &properties, &mut changed).await?;
                let kind = changed.kind;
                cleared = remove_missing_options(&mut tx, &current, &kind).await?;
                kind
            }
            (None, None) => current.kind.clone(),
        };
        let property = Property {
            name: changes.name.unwrap_or_else(|| current.name.clone()),
            kind,
            ..current
        };
        let others: Vec<Property> = properties
            .iter()
            .filter(|other| other.id != property_id)
            .cloned()
            .collect();
        let property = formula::checked(&others, property)?;
        sqlx::query(
            "UPDATE database_properties SET name = $1, type = $2, config_json = $3, updated_at = $4 WHERE id = $5",
        )
        .bind(&property.name)
        .bind(property.kind.property_type().as_str())
        .bind(property.kind.config_json().to_string())
        .bind(now.as_millis())
        .bind(property_id.to_string())
        .execute(&mut *tx)
        .await?;
        let property = match (&current.kind, type_change) {
            (PropertyKind::Relation(previous), None) if changes.config.is_some() => Property {
                kind: relations::sync_pair(
                    &mut tx,
                    &ComputeScope::utc(workspace_id, actor_id),
                    database_id,
                    &page.title,
                    &property,
                    Some(previous),
                    request_id,
                    now,
                )
                .await?,
                ..property
            },
            _ => property,
        };
        properties.remove(index);
        let index = insert_index(changes.position.or(Some(index as i64)), properties.len());
        properties.insert(index, property.clone());
        renumber_rows(&mut tx, "database_properties", &properties_ids(&properties)).await?;
        formula::refresh_types(&mut tx, &mut properties, now).await?;
        if type_change == Some(PropertyType::UniqueId) {
            unique_id::number_rows(
                &mut tx,
                workspace_id,
                database_id,
                &mut properties,
                actor_id,
                now,
            )
            .await?;
        }
        let update = PropertyUpdate {
            property: DatabaseProperty {
                position: index as i64,
                ..properties[index].output()
            },
            converted,
            cleared,
        };
        if changes.dry_run {
            // Nothing is kept, so nothing is audited and no realtime event goes out.
            tx.rollback().await?;
            return Ok(update);
        }
        // A rename of the property itself changes no searchable text.
        if type_change.is_some() || changes.config.is_some() {
            refresh_search_text(
                &mut tx,
                workspace_id,
                database_id,
                &properties,
                SearchRows::All,
            )
            .await?;
        }
        record(
            &mut tx,
            workspace_id,
            actor_id,
            "database.property_updated",
            database_id,
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(update)
    }

    pub async fn delete_property(
        &self,
        workspace_id: Id,
        database_id: Id,
        property_id: Id,
        actor_id: Id,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), DatabaseError> {
        let mut tx = self.database.immediate_transaction().await?;
        let page = open(&mut tx, workspace_id, database_id, actor_id).await?;
        unlocked(&page)?;
        let mut properties = properties_in_tx(&mut tx, database_id).await?;
        let index = properties
            .iter()
            .position(|property| property.id == property_id)
            .ok_or(DatabaseError::PropertyNotFound)?;
        if properties[index].kind == PropertyKind::Title {
            return Err(DatabaseError::TitleProperty);
        }
        relations::detach(&mut tx, &properties[index], true).await?;
        // Its values go by cascade.
        sqlx::query("DELETE FROM database_properties WHERE id = $1")
            .bind(property_id.to_string())
            .execute(&mut *tx)
            .await?;
        properties.remove(index);
        formula::refresh_types(&mut tx, &mut properties, now).await?;
        renumber_rows(&mut tx, "database_properties", &properties_ids(&properties)).await?;
        refresh_search_text(
            &mut tx,
            workspace_id,
            database_id,
            &properties,
            SearchRows::All,
        )
        .await?;
        for mut view in raw_views(&mut tx, database_id, Views::All).await? {
            let before = view.config.clone();
            view.config.repair(view.view_type, &properties);
            if view.config != before {
                write_view_config(&mut tx, view.id, &view.config, now).await?;
            }
        }
        record(
            &mut tx,
            workspace_id,
            actor_id,
            "database.property_deleted",
            database_id,
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }
}

/// Every stored value of a property (live and trashed rows), parsed under its kind.
async fn stored_values(
    tx: &mut Transaction<'_, Any>,
    property: &Property,
) -> Result<Vec<(String, CellValue)>, DatabaseError> {
    let rows = sqlx::query(
        "SELECT page_id, value_json FROM page_property_values WHERE property_id = $1 ORDER BY page_id",
    )
    .bind(property.id.to_string())
    .fetch_all(&mut **tx)
    .await?;
    rows.into_iter()
        .map(|row| {
            let raw: Value = serde_json::from_str(&row.get::<String, _>("value_json"))
                .map_err(|_| DatabaseError::Corrupt)?;
            match parse_value(&property.kind, &raw) {
                Ok(Some(value)) => Ok((row.get("page_id"), value)),
                _ => Err(DatabaseError::Corrupt),
            }
        })
        .collect()
}

/// Removes option ids that `next` no longer has from the values; returns how many values were
/// cleared.
async fn remove_missing_options(
    tx: &mut Transaction<'_, Any>,
    current: &Property,
    next: &PropertyKind,
) -> Result<u64, DatabaseError> {
    let removed = current
        .kind
        .options()
        .iter()
        .any(|(id, _)| !next.has_option(id));
    if !removed {
        return Ok(0);
    }
    let mut cleared = 0;
    for (row, value) in stored_values(tx, current).await? {
        let next_value = match &value {
            CellValue::Choice(id) => next.has_option(id).then(|| value.clone()),
            CellValue::Choices(ids) => {
                let kept: Vec<String> = ids
                    .iter()
                    .filter(|id| next.has_option(id))
                    .cloned()
                    .collect();
                (!kept.is_empty()).then_some(CellValue::Choices(kept))
            }
            _ => Some(value.clone()),
        };
        match next_value {
            Some(next_value) if next_value == value => {}
            Some(next_value) => update_value(tx, &row, current.id, &next_value).await?,
            None => {
                cleared += 1;
                delete_value(tx, &row, current.id).await?;
            }
        }
    }
    Ok(cleared)
}

pub(super) fn properties_ids(properties: &[Property]) -> Vec<Id> {
    properties.iter().map(|property| property.id).collect()
}

/// Sets `position` to the index for each id of a database's properties or views.
pub(super) async fn renumber_rows(
    tx: &mut Transaction<'_, Any>,
    table: &'static str,
    ids: &[Id],
) -> Result<(), sqlx::Error> {
    for (position, id) in ids.iter().enumerate() {
        let position = position as i64;
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE {table} SET position = $1 WHERE id = $2 AND position <> $3"
        )))
        .bind(position)
        .bind(id.to_string())
        .bind(position)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

pub(super) async fn properties_in_tx(
    tx: &mut Transaction<'_, Any>,
    database_id: Id,
) -> Result<Vec<Property>, DatabaseError> {
    let rows = sqlx::query(
        "SELECT id, name, type, config_json, position FROM database_properties \
         WHERE database_id = $1 ORDER BY position, id",
    )
    .bind(database_id.to_string())
    .fetch_all(&mut **tx)
    .await?;
    rows.into_iter()
        .map(|row| {
            Property::from_stored(
                &row.get::<String, _>("id"),
                row.get("name"),
                &row.get::<String, _>("type"),
                &row.get::<String, _>("config_json"),
                row.get("position"),
            )
        })
        .collect()
}

impl Property {
    /// A property from its `database_properties` columns.
    pub(in crate::repositories) fn from_stored(
        id: &str,
        name: String,
        property_type: &str,
        config_json: &str,
        position: i64,
    ) -> Result<Self, DatabaseError> {
        let property_type = PropertyType::parse(property_type).ok_or(DatabaseError::Corrupt)?;
        let config: Value =
            serde_json::from_str(config_json).map_err(|_| DatabaseError::Corrupt)?;
        Ok(Self {
            id: parse_id(id)?,
            name,
            kind: PropertyKind::parse(property_type, Some(&config))
                .map_err(|_| DatabaseError::Corrupt)?,
            position,
        })
    }
}

pub(super) async fn insert_property(
    tx: &mut Transaction<'_, Any>,
    workspace_id: Id,
    database_id: Id,
    property: &Property,
    now: TimestampMillis,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO database_properties (id, workspace_id, database_id, name, type, config_json, position, created_at, updated_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
    )
    .bind(property.id.to_string())
    .bind(workspace_id.to_string())
    .bind(database_id.to_string())
    .bind(&property.name)
    .bind(property.kind.property_type().as_str())
    .bind(property.kind.config_json().to_string())
    .bind(property.position)
    .bind(now.as_millis())
    .bind(now.as_millis())
    .execute(&mut **tx)
    .await?;
    Ok(())
}
