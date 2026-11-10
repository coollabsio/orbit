//! What a page export reads of a database (P1 9, P2 9.2): its properties, the first view of its
//! own views and of each linked view block, and its live rows with their values as text.

use std::collections::{HashMap, HashSet};

use orbit_platform::{Id, TimestampMillis};
use serde_json::Value;

use super::{
    CellValue, DatabaseError, DatabaseRepository, DatabaseView, Property, PropertyKind,
    StatusGroup, ViewType, Views, display_text, parse_value, properties_in_tx, rows_in_tx,
    views_in_tx,
};

/// A database to export.
#[derive(Clone, Debug)]
pub struct ExportDatabase {
    pub properties: Vec<Property>,
    /// The first of the database's own views.
    pub view: Option<DatabaseView>,
    /// The first view of each requested linked view block.
    pub link_views: HashMap<Id, DatabaseView>,
    /// Live rows in manual order.
    pub rows: Vec<ExportRow>,
}

#[derive(Clone, Debug)]
pub struct ExportRow {
    pub id: Id,
    pub title: String,
    pub created_at: TimestampMillis,
    pub created_by: Id,
    pub updated_at: TimestampMillis,
    pub updated_by: Id,
    pub values: HashMap<Id, CellValue>,
}

impl ExportDatabase {
    /// The properties a view shows, in order: a table's visible columns (properties the config
    /// does not name yet come last, as clients show them), a board's or a calendar's title and card
    /// properties.
    #[must_use]
    pub fn view_properties(&self, view: &DatabaseView) -> Vec<&Property> {
        let by_id: HashMap<Id, &Property> = self
            .properties
            .iter()
            .map(|property| (property.id, property))
            .collect();
        if matches!(view.view_type, ViewType::Board | ViewType::Calendar) {
            let title = self
                .properties
                .iter()
                .find(|property| property.kind == PropertyKind::Title);
            return title
                .into_iter()
                .chain(
                    view.config
                        .card_properties
                        .iter()
                        .filter_map(|id| by_id.get(id).copied()),
                )
                .collect();
        }
        let named: HashSet<Id> = view
            .config
            .columns
            .iter()
            .map(|column| column.property_id)
            .collect();
        view.config
            .columns
            .iter()
            .filter(|column| !column.hidden)
            .filter_map(|column| by_id.get(&column.property_id).copied())
            .chain(
                self.properties
                    .iter()
                    .filter(|property| !named.contains(&property.id)),
            )
            .collect()
    }
}

/// A value as an export writes it (spec P2 9.2): the display text in a fixed format for machines.
/// Numbers as stored, dates as ISO 8601 (`start/end` for a range), option and member names joined
/// with ", ", a checkbox as `true` or `false`; a status without a value is its first `todo`
/// option, and the computed properties read the row's page.
#[must_use]
pub fn cell_text(property: &Property, row: &ExportRow, members: &HashMap<Id, String>) -> String {
    let names = |ids: &[Id]| {
        ids.iter()
            .filter_map(|id| members.get(id).map(String::as_str))
            .filter(|name| !name.is_empty())
            .collect::<Vec<_>>()
            .join(", ")
    };
    match &property.kind {
        PropertyKind::Title => return row.title.clone(),
        PropertyKind::CreatedTime => return row.created_at.to_string(),
        PropertyKind::LastEditedTime => return row.updated_at.to_string(),
        PropertyKind::CreatedBy => return names(&[row.created_by]),
        PropertyKind::LastEditedBy => return names(&[row.updated_by]),
        PropertyKind::Checkbox => {
            return if row.values.contains_key(&property.id) {
                "true".to_owned()
            } else {
                "false".to_owned()
            };
        }
        _ => {}
    }
    match (row.values.get(&property.id), &property.kind) {
        (Some(CellValue::People(ids)), _) => names(ids),
        (Some(value), kind) => display_text(kind, value),
        (None, PropertyKind::Status(options)) => options
            .iter()
            .find(|option| option.group == StatusGroup::Todo)
            .map(|option| option.name.clone())
            .unwrap_or_default(),
        (None, _) => String::new(),
    }
}

/// Whether a property holds numbers (export cells of numbers are never guarded).
#[must_use]
pub fn is_number(property: &Property) -> bool {
    matches!(property.kind, PropertyKind::Number(_))
}

impl DatabaseRepository {
    /// The export data of each database in `wanted` (database id → linked view blocks whose first
    /// view is needed). The caller checked that the actor sees them; ids that are not live
    /// databases are left out.
    pub async fn export_data(
        &self,
        wanted: &HashMap<Id, HashSet<Id>>,
    ) -> Result<HashMap<Id, ExportDatabase>, DatabaseError> {
        let mut tx = self.database.transaction().await?;
        let mut databases = HashMap::with_capacity(wanted.len());
        for (database_id, links) in wanted {
            let live: Option<i64> = sqlx::query_scalar(
                "SELECT 1 FROM pages WHERE id = $1 AND kind = 'database' AND deleted_at IS NULL",
            )
            .bind(database_id.to_string())
            .fetch_optional(&mut *tx)
            .await?;
            if live.is_none() {
                continue;
            }
            let properties = properties_in_tx(&mut tx, *database_id).await?;
            let first =
                |views: Vec<DatabaseView>| views.into_iter().min_by_key(|view| view.position);
            let view = first(views_in_tx(&mut tx, *database_id, Views::Own, &properties).await?);
            let mut link_views = HashMap::new();
            for link in links {
                if let Some(view) = first(
                    views_in_tx(&mut tx, *database_id, Views::Link(*link), &properties).await?,
                ) {
                    link_views.insert(*link, view);
                }
            }
            let rows = rows_in_tx(&mut tx, *database_id, None)
                .await?
                .into_iter()
                .map(|row| ExportRow {
                    values: stored(&properties, &row.values),
                    id: row.id,
                    title: row.title,
                    created_at: row.created_at,
                    created_by: row.created_by,
                    updated_at: row.updated_at,
                    updated_by: row.updated_by,
                })
                .collect();
            databases.insert(
                *database_id,
                ExportDatabase {
                    properties,
                    view,
                    link_views,
                    rows,
                },
            );
        }
        tx.rollback().await?;
        Ok(databases)
    }
}

/// Stored values parsed under their properties; values that no longer fit are left out.
fn stored(
    properties: &[Property],
    values: &std::collections::BTreeMap<String, Value>,
) -> HashMap<Id, CellValue> {
    properties
        .iter()
        .filter_map(|property| {
            let raw = values.get(&property.id.to_string())?;
            let value = parse_value(&property.kind, raw).ok().flatten()?;
            Some((property.id, value))
        })
        .collect()
}
