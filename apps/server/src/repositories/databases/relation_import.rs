//! Relations and rollups from a Notion import (spec P3 11.9). The import plans every Notion
//! relation as a text property of related titles, and every rollup as static values; once all
//! databases exist, these methods turn the ones whose target came with the import into live
//! relation and rollup properties. Each call is one transaction and does nothing when its new
//! property already exists, so a resumed import converges.

use std::collections::HashMap;

use orbit_platform::{Id, TimestampMillis};
use serde_json::json;
use sqlx::{Any, Transaction};

use super::relations::{RelationConfig, RelationLimit};
use super::rollups::{self, RollupConfig};
use super::{
    CellValue, DatabaseError, DatabaseRepository, MAX_NAME_CHARS, MAX_PROPERTIES, Property,
    PropertyKind, Views, delete_value, insert_property, properties_ids, properties_in_tx,
    raw_views, record_with, renumber_rows, upsert_value, write_view_config,
};

/// One side of an imported relation: the planned text property it replaces.
pub struct ImportedSide {
    pub database_id: Id,
    /// The text property of related titles that the plan made for the Notion property.
    pub text_property_id: Id,
    /// The id of the relation property to create (derived, so a retry finds it).
    pub relation_id: Id,
    pub name: String,
    /// Titles of related pages outside the import, per row: the text property keeps only
    /// these, as "<name> (titles)". All `None`: the text property goes.
    pub outside: Vec<(Id, Option<String>)>,
}

pub struct ImportedRelation {
    pub primary: ImportedSide,
    /// The other side of a Notion `dual_property` (in the target database).
    pub synced: Option<ImportedSide>,
    /// The target database of the primary side.
    pub target_id: Id,
    /// Links of the primary side: row → related rows of the target, in Notion's order.
    pub links: Vec<(Id, Vec<Id>)>,
}

pub struct ImportedRollup {
    pub database_id: Id,
    /// The property of static values that the plan made for the Notion rollup.
    pub static_property_id: Id,
    pub rollup_id: Id,
    pub config: RollupConfig,
}

impl DatabaseRepository {
    /// Turns a planned text property (and, for a two-way pair, the one of the target) into a
    /// relation with its links. Returns whether it changed anything.
    pub async fn import_relation(
        &self,
        workspace_id: Id,
        actor_id: Id,
        relation: ImportedRelation,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<bool, DatabaseError> {
        let mut tx = self.database.immediate_transaction().await?;
        if property_exists(&mut tx, relation.primary.relation_id).await? {
            tx.rollback().await?;
            return Ok(false);
        }
        let source = relation.primary.database_id;
        let primary = Property {
            id: relation.primary.relation_id,
            name: relation.primary.name.clone(),
            kind: PropertyKind::Relation(RelationConfig {
                database_id: relation.target_id,
                limit: RelationLimit::None,
                synced_property_id: relation.synced.as_ref().map(|side| side.relation_id),
                primary: true,
                two_way: None,
            }),
            position: 0,
        };
        if !replace(
            &mut tx,
            workspace_id,
            &relation.primary,
            primary,
            actor_id,
            now,
        )
        .await?
        {
            tx.rollback().await?;
            return Ok(false);
        }
        if let Some(side) = &relation.synced {
            let synced = Property {
                id: side.relation_id,
                name: side.name.clone(),
                kind: PropertyKind::Relation(RelationConfig {
                    database_id: source,
                    limit: RelationLimit::None,
                    synced_property_id: Some(relation.primary.relation_id),
                    primary: false,
                    two_way: None,
                }),
                position: 0,
            };
            replace(&mut tx, workspace_id, side, synced, actor_id, now).await?;
        }
        for (row, pages) in &relation.links {
            for (position, page) in pages.iter().enumerate() {
                sqlx::query(
                    "INSERT INTO page_relations (property_id, page_id, related_page_id, workspace_id, position, created_at) \
                     SELECT $1, $2, $3, $4, $5, $6 WHERE EXISTS (SELECT 1 FROM pages WHERE id = $7 AND parent_id = $8) \
                     ON CONFLICT (property_id, page_id, related_page_id) DO NOTHING",
                )
                .bind(relation.primary.relation_id.to_string())
                .bind(row.to_string())
                .bind(page.to_string())
                .bind(workspace_id.to_string())
                .bind(position as i64)
                .bind(now.as_millis())
                .bind(page.to_string())
                .bind(relation.target_id.to_string())
                .execute(&mut *tx)
                .await?;
            }
        }
        let mut changed = vec![source];
        changed.extend(relation.synced.as_ref().map(|side| side.database_id));
        changed.dedup();
        for database_id in changed {
            record_with(
                &mut tx,
                workspace_id,
                actor_id,
                "database.property_created",
                database_id,
                json!({}),
                request_id,
                now,
            )
            .await?;
        }
        tx.commit().await?;
        Ok(true)
    }

    /// Turns a property of static rollup values into a live rollup when its config checks
    /// (spec 11.5); otherwise the static values stay. Returns whether it changed anything.
    pub async fn import_rollup(
        &self,
        workspace_id: Id,
        actor_id: Id,
        rollup: ImportedRollup,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<bool, DatabaseError> {
        let mut tx = self.database.immediate_transaction().await?;
        if property_exists(&mut tx, rollup.rollup_id).await? {
            tx.rollback().await?;
            return Ok(false);
        }
        let properties = properties_in_tx(&mut tx, rollup.database_id).await?;
        let Some(planned) = properties
            .iter()
            .find(|property| property.id == rollup.static_property_id)
        else {
            tx.rollback().await?;
            return Ok(false);
        };
        if rollups::check(&mut tx, rollup.database_id, &properties, &rollup.config)
            .await
            .is_err()
        {
            tx.rollback().await?;
            return Ok(false);
        }
        let side = ImportedSide {
            database_id: rollup.database_id,
            text_property_id: planned.id,
            relation_id: rollup.rollup_id,
            name: planned.name.clone(),
            outside: Vec::new(),
        };
        let live = Property {
            id: rollup.rollup_id,
            name: planned.name.clone(),
            kind: PropertyKind::Rollup(rollup.config),
            position: 0,
        };
        replace(&mut tx, workspace_id, &side, live, actor_id, now).await?;
        record_with(
            &mut tx,
            workspace_id,
            actor_id,
            "database.property_created",
            rollup.database_id,
            json!({}),
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(true)
    }
}

async fn property_exists(
    tx: &mut Transaction<'_, Any>,
    property_id: Id,
) -> Result<bool, DatabaseError> {
    Ok(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM database_properties WHERE id = $1")
            .bind(property_id.to_string())
            .fetch_one(&mut **tx)
            .await?
            > 0,
    )
}

/// Puts `property` where the planned property `side.text_property_id` is (its place in the
/// property order and in every view). The planned property goes, unless some rows keep text in
/// it (`side.outside`): then it is renamed "<name> (titles)" and holds only that text. Returns
/// `false` when the planned property is gone or the database is full.
async fn replace(
    tx: &mut Transaction<'_, Any>,
    workspace_id: Id,
    side: &ImportedSide,
    mut property: Property,
    actor_id: Id,
    now: TimestampMillis,
) -> Result<bool, DatabaseError> {
    let database_id = side.database_id;
    let mut properties = properties_in_tx(tx, database_id).await?;
    let Some(index) = properties
        .iter()
        .position(|candidate| candidate.id == side.text_property_id)
    else {
        return Ok(false);
    };
    let keep = side.outside.iter().any(|(_, text)| text.is_some());
    if keep && properties.len() >= MAX_PROPERTIES {
        return Ok(false);
    }
    let planned = properties[index].clone();
    if keep {
        let name: String = format!("{} (titles)", planned.name)
            .chars()
            .take(MAX_NAME_CHARS)
            .collect();
        sqlx::query("UPDATE database_properties SET name = $1, updated_at = $2 WHERE id = $3")
            .bind(&name)
            .bind(now.as_millis())
            .bind(planned.id.to_string())
            .execute(&mut **tx)
            .await?;
        let texts: HashMap<Id, &Option<String>> = side
            .outside
            .iter()
            .map(|(row, text)| (*row, text))
            .collect();
        for (row, text) in texts {
            match text {
                Some(text) => {
                    let value = CellValue::Text(text.clone());
                    upsert_value(
                        tx,
                        workspace_id,
                        &row.to_string(),
                        planned.id,
                        &value,
                        actor_id,
                        now,
                    )
                    .await?;
                }
                None => delete_value(tx, &row.to_string(), planned.id).await?,
            }
        }
    } else {
        sqlx::query("DELETE FROM database_properties WHERE id = $1")
            .bind(planned.id.to_string())
            .execute(&mut **tx)
            .await?;
        properties.remove(index);
    }
    property.position = index as i64;
    insert_property(tx, workspace_id, database_id, &property, now).await?;
    properties.insert(index, property.clone());
    renumber_rows(tx, "database_properties", &properties_ids(&properties)).await?;
    for mut view in raw_views(tx, database_id, Views::All).await? {
        let columns = &mut view.config.columns;
        let at = columns
            .iter()
            .position(|column| column.property_id == planned.id);
        let column = match at {
            Some(at) => super::ViewColumn {
                property_id: property.id,
                ..columns[at].clone()
            },
            None => property.column(),
        };
        match at {
            Some(at) if !keep => columns[at] = column,
            Some(at) => columns.insert(at, column),
            None => columns.push(column),
        }
        view.config.repair(view.view_type, &properties);
        write_view_config(tx, view.id, &view.config, now).await?;
    }
    Ok(true)
}
