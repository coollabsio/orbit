//! Database data copied with pages: the properties, views, templates and values of a set of
//! pages, loaded for a duplicate or a backup and written back under new ids.

use std::collections::HashMap;

use orbit_platform::Row;
use orbit_platform::{Id, TimestampMillis};
use serde_json::Value;
use sqlx::{Any, Transaction};

use super::{
    DatabaseView, FileCopies, Property, PropertyKind, TemplateCopy, Views, insert_property,
    insert_view, page_error, parse_id, parse_value, properties_in_tx, raw_views,
};
use super::{files, relations, search, templates};
use crate::repositories::pages::PageError;

/// The database data of a set of pages, keyed by their original ids: the properties and views
/// of each database, and the values of each page.
#[derive(Default)]
pub(in crate::repositories) struct DatabaseData {
    pub properties: Vec<(Id, Property)>,
    pub views: Vec<(Id, DatabaseView)>,
    /// `(page, property, value_json)`.
    pub values: Vec<(Id, Id, String)>,
    /// The templates of the databases.
    pub templates: Vec<TemplateCopy>,
}

/// The live data of the databases in `databases` and the values of the pages in `pages`.
pub(in crate::repositories) async fn load_database_data(
    tx: &mut Transaction<'_, Any>,
    databases: &[Id],
    pages: impl Iterator<Item = Id>,
) -> Result<DatabaseData, PageError> {
    let mut data = DatabaseData {
        templates: templates::load_copies(tx, databases)
            .await
            .map_err(page_error)?,
        ..DatabaseData::default()
    };
    for database_id in databases {
        for property in properties_in_tx(tx, *database_id)
            .await
            .map_err(page_error)?
        {
            data.properties.push((*database_id, property));
        }
        for view in raw_views(tx, *database_id, Views::Own)
            .await
            .map_err(page_error)?
        {
            data.views.push((*database_id, view));
        }
    }
    for page_id in pages {
        let rows = sqlx::query(
            "SELECT property_id, value_json FROM page_property_values WHERE page_id = $1 ORDER BY property_id",
        )
        .bind(page_id.to_string())
        .fetch_all(&mut **tx)
        .await?;
        for row in rows {
            let property_id = parse_id(&row.get::<String, _>("property_id")).map_err(page_error)?;
            data.values
                .push((page_id, property_id, row.get::<String, _>("value_json")));
        }
    }
    Ok(data)
}

/// Writes `data` under the copies in `pages` (original id → copy id): properties and views get
/// new ids, views point at the copied properties, and values move to the copied property. A
/// value of a property that was not copied (a duplicated row stays in its database) keeps its
/// property.
pub(in crate::repositories) async fn write_database_data(
    tx: &mut Transaction<'_, Any>,
    workspace_id: Id,
    data: DatabaseData,
    pages: &HashMap<Id, Id>,
    files: &FileCopies,
    actor_id: Id,
    now: TimestampMillis,
) -> Result<HashMap<Id, Id>, PageError> {
    let property_ids: HashMap<Id, Id> = data
        .properties
        .iter()
        .map(|(_, property)| (property.id, Id::new_v7()))
        .collect();
    for (database_id, property) in &data.properties {
        let copy = Property {
            id: property_ids[&property.id],
            kind: relations::copied_kind(&property.kind, &property_ids),
            ..property.clone()
        };
        insert_property(tx, workspace_id, pages[database_id], &copy, now).await?;
    }
    relations::copy_links(
        tx,
        workspace_id,
        &data.properties,
        &property_ids,
        pages,
        now,
    )
    .await
    .map_err(page_error)?;
    templates::write_copies(tx, workspace_id, &data.templates, pages, &property_ids, now)
        .await
        .map_err(page_error)?;
    for (database_id, mut view) in data.views {
        view.config.remap(&property_ids);
        templates::remap_default(&mut view.config, pages);
        view.id = Id::new_v7();
        insert_view(tx, workspace_id, pages[&database_id], None, &view, now)
            .await
            .map_err(page_error)?;
    }
    for (page_id, property_id, value_json) in data.values {
        let Some(value_json) = files::remap_stored(value_json, page_id, files) else {
            continue;
        };
        sqlx::query(
            "INSERT INTO page_property_values (page_id, property_id, workspace_id, value_json, updated_by, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(pages[&page_id].to_string())
        .bind(property_ids.get(&property_id).unwrap_or(&property_id).to_string())
        .bind(workspace_id.to_string())
        .bind(value_json)
        .bind(actor_id.to_string())
        .bind(now.as_millis())
        .execute(&mut **tx)
        .await?;
    }
    search::refresh_rows(tx, pages.values().copied())
        .await
        .map_err(page_error)?;
    Ok(property_ids)
}

/// Keeps only the values of `page_id` that fit `database_id`'s live properties: the property
/// still exists and the stored value parses against its current type (empty ones go too).
pub(in crate::repositories) async fn retain_fitting_values(
    tx: &mut Transaction<'_, Any>,
    database_id: Id,
    page_id: Id,
    values: &mut Vec<(Id, Id, String)>,
) -> Result<(), PageError> {
    let properties = properties_in_tx(tx, database_id)
        .await
        .map_err(page_error)?;
    values.retain(|(page, property_id, value_json)| {
        *page != page_id
            || properties
                .iter()
                .find(|property| property.id == *property_id)
                .is_some_and(|property| {
                    // A restored row gets a new number.
                    !matches!(property.kind, PropertyKind::UniqueId(_))
                        && serde_json::from_str::<Value>(value_json).is_ok_and(|raw| {
                            matches!(parse_value(&property.kind, &raw), Ok(Some(_)))
                        })
                })
    });
    Ok(())
}

/// Copies database data for a page duplicate: the properties and views of each database in
/// `databases` and the values of every copied page (`pages`: original id → copy id). Returns
/// the copied property ids (original id → copy id).
pub(in crate::repositories) async fn copy_database_data(
    tx: &mut Transaction<'_, Any>,
    workspace_id: Id,
    databases: &[Id],
    pages: &HashMap<Id, Id>,
    files: &FileCopies,
    actor_id: Id,
    now: TimestampMillis,
) -> Result<HashMap<Id, Id>, PageError> {
    let data = load_database_data(tx, databases, pages.keys().copied()).await?;
    write_database_data(tx, workspace_id, data, pages, files, actor_id, now).await
}
