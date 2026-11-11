//! The schema a new database starts with: its properties, views and imported rows.

use std::collections::HashSet;

use orbit_platform::{Id, TimestampMillis};
use serde_json::Value;
use sqlx::{Any, Transaction};

use super::{
    DatabaseError, DatabaseView, MAX_NAME_CHARS, MAX_PROPERTIES, MAX_VIEWS, Property, PropertyKind,
    PropertyType, ViewConfig, ViewType, import, insert_property, insert_view, page_error,
};
use crate::repositories::pages::{PageError, PageSpace};

/// A property of a [`DatabaseSchema`], with a caller-chosen id.
pub struct SchemaProperty {
    pub id: Id,
    pub name: String,
    pub property_type: PropertyType,
    /// The config as clients send it (`config` of `POST …/properties`).
    pub config: Value,
}

/// A view of a [`DatabaseSchema`]. Empty `columns` show every property.
pub struct SchemaView {
    pub name: String,
    pub view_type: ViewType,
    pub config: ViewConfig,
}

/// The properties and views a new database starts with, checked by the same rules as the
/// property and view routes.
pub struct DatabaseSchema {
    pub(super) properties: Vec<Property>,
    pub(super) views: Vec<DatabaseView>,
    /// Rows to create with the database (a CSV import).
    pub(super) rows: Option<import::ImportRows>,
}

impl DatabaseSchema {
    /// Rows created with the database.
    pub(in crate::repositories) fn row_count(&self) -> usize {
        self.rows.as_ref().map_or(0, import::ImportRows::len)
    }

    /// Checks names, configs, limits and exactly one `title` property. Filter rules that do not
    /// fit their property are dropped (a view of another app can hold rules Orbit has no operator
    /// for); the second value counts them. Errors name the field path.
    pub fn new(
        properties: Vec<SchemaProperty>,
        views: Vec<SchemaView>,
    ) -> Result<(Self, usize), DatabaseError> {
        if properties.is_empty() || properties.len() > MAX_PROPERTIES {
            return Err(DatabaseError::invalid("properties"));
        }
        if views.is_empty() || views.len() > MAX_VIEWS {
            return Err(DatabaseError::invalid("views"));
        }
        let mut ids = HashSet::new();
        let properties = properties
            .into_iter()
            .enumerate()
            .map(|(index, input)| {
                let path =
                    |field: &str| DatabaseError::invalid(format!("properties[{index}]{field}"));
                let name = input.name.trim().to_owned();
                if name.is_empty() || name.chars().count() > MAX_NAME_CHARS {
                    return Err(path(".name"));
                }
                if !ids.insert(input.id) {
                    return Err(path(".id"));
                }
                let kind = PropertyKind::parse(input.property_type, Some(&input.config))
                    .map_err(|field| path(&format!(".{field}")))?;
                Ok(Property {
                    id: input.id,
                    name,
                    kind,
                    position: index as i64,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        if properties
            .iter()
            .filter(|property| property.kind == PropertyKind::Title)
            .count()
            != 1
        {
            return Err(DatabaseError::invalid("properties"));
        }
        let mut dropped = 0;
        let views = views
            .into_iter()
            .enumerate()
            .map(|(index, input)| {
                let name = input.name.trim().to_owned();
                if name.is_empty() || name.chars().count() > MAX_NAME_CHARS {
                    return Err(DatabaseError::invalid(format!("views[{index}].name")));
                }
                let mut config = input.config;
                if config.columns.is_empty() {
                    config.columns = ViewConfig::for_properties(&properties).columns;
                }
                dropped += config.drop_unfit_rules(&properties);
                config
                    .validate(input.view_type, &properties)
                    .map_err(|field| DatabaseError::invalid(format!("views[{index}].{field}")))?;
                Ok(DatabaseView {
                    id: Id::new_v7(),
                    name,
                    view_type: input.view_type,
                    config,
                    position: index as i64,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok((
            Self {
                properties,
                views,
                rows: None,
            },
            dropped,
        ))
    }

    /// A new database's schema: a "Name" title, a "Tags" multi-select and one "Table" view.
    fn starter() -> Self {
        let properties: Vec<Property> = [
            ("Name", PropertyKind::Title),
            ("Tags", PropertyKind::MultiSelect(Vec::new())),
        ]
        .into_iter()
        .enumerate()
        .map(|(position, (name, kind))| Property {
            id: Id::new_v7(),
            name: name.to_owned(),
            kind,
            position: position as i64,
        })
        .collect();
        let views = vec![DatabaseView {
            id: Id::new_v7(),
            name: "Table".to_owned(),
            view_type: ViewType::Table,
            config: ViewConfig::for_properties(&properties),
            position: 0,
        }];
        Self {
            properties,
            views,
            rows: None,
        }
    }
}

/// Writes a new database's schema; `None` is the starter schema of `POST /pages`.
pub(in crate::repositories) async fn insert_schema(
    tx: &mut Transaction<'_, Any>,
    workspace_id: Id,
    database_id: Id,
    space: PageSpace,
    actor_id: Id,
    schema: Option<&DatabaseSchema>,
    now: TimestampMillis,
) -> Result<(), PageError> {
    let starter;
    let schema = match schema {
        Some(schema) => schema,
        None => {
            starter = DatabaseSchema::starter();
            &starter
        }
    };
    for property in &schema.properties {
        insert_property(tx, workspace_id, database_id, property, now).await?;
    }
    for view in &schema.views {
        insert_view(tx, workspace_id, database_id, None, view, now)
            .await
            .map_err(page_error)?;
    }
    if let Some(rows) = &schema.rows {
        import::insert_rows(tx, workspace_id, database_id, space, actor_id, rows, now).await?;
    }
    Ok(())
}
