//! Files properties (spec P3 item 10). A value lists files of the row page itself: clients upload
//! through the page file route of the row, then write `[{file_id, name}]`. A write must name
//! files of the same row, and takes each name from its `page_files` row. A copy of a row (a
//! duplicate, a template fill, a restore) points its value at the copied files.

use std::collections::{HashMap, HashSet};

use orbit_platform::{INLINE_IMAGE_TYPES, Id, Row};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{Any, Transaction};
use utoipa::ToSchema;

use super::super::page_files::page_file_url;
use super::{CellValue, DatabaseError};

/// Files in one cell.
pub const MAX_FILES: usize = 20;
const MAX_FILE_NAME_CHARS: usize = 255;

/// A file of a files value: a `page_files` row of the row page.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
#[schema(as = DatabaseFileRef)]
pub struct FileRef {
    #[schema(value_type = String)]
    pub file_id: Id,
    pub name: String,
}

/// (original page, original file) → (copy page, copy file), as a page duplicate records it.
pub(in crate::repositories) type FileCopies = HashMap<(Id, Id), (Id, Id)>;

pub(super) fn parse(raw: &Value) -> Result<Option<CellValue>, ()> {
    let files: Vec<FileRef> = serde_json::from_value(raw.clone()).map_err(|_| ())?;
    let mut seen = HashSet::new();
    if files.len() > MAX_FILES
        || !files.iter().all(|file| {
            seen.insert(file.file_id)
                && !file.name.is_empty()
                && file.name.chars().count() <= MAX_FILE_NAME_CHARS
        })
    {
        return Err(());
    }
    Ok((!files.is_empty()).then_some(CellValue::Files(files)))
}

/// Checks that every files value names files of `page_id`, and sets each name to the stored one.
pub(super) async fn check_page_files(
    tx: &mut Transaction<'_, Any>,
    workspace_id: Id,
    page_id: Id,
    values: &mut [(Id, Option<CellValue>)],
) -> Result<(), DatabaseError> {
    if !values
        .iter()
        .any(|(_, value)| matches!(value, Some(CellValue::Files(_))))
    {
        return Ok(());
    }
    let stored: HashMap<String, String> = sqlx::query(
        "SELECT id, file_name FROM page_files WHERE page_id = $1 AND workspace_id = $2",
    )
    .bind(page_id.to_string())
    .bind(workspace_id.to_string())
    .fetch_all(&mut **tx)
    .await?
    .into_iter()
    .map(|row| (row.get("id"), row.get("file_name")))
    .collect();
    for (property_id, value) in values {
        let Some(CellValue::Files(files)) = value else {
            continue;
        };
        for file in files {
            file.name = stored
                .get(&file.file_id.to_string())
                .cloned()
                .ok_or_else(|| DatabaseError::invalid(format!("values.{property_id}")))?;
        }
    }
    Ok(())
}

/// Points the files values of a copy of `source` at the copied files; a file that was not
/// copied leaves the value.
pub(super) fn remap_values(
    values: &mut [(Id, Option<CellValue>)],
    source: Id,
    copies: &FileCopies,
) {
    for (_, value) in values {
        if let Some(CellValue::Files(files)) = value {
            remap(files, source, copies);
            if files.is_empty() {
                *value = None;
            }
        }
    }
}

/// [`remap_values`] for a stored `value_json` of `source`, whatever its property type: only a
/// files value has objects with a `file_id`. `None`: the value is now empty.
pub(super) fn remap_stored(value_json: String, source: Id, copies: &FileCopies) -> Option<String> {
    let Ok(mut files) = serde_json::from_str::<Vec<FileRef>>(&value_json) else {
        return Some(value_json);
    };
    remap(&mut files, source, copies);
    (!files.is_empty()).then(|| serde_json::to_string(&files).unwrap_or(value_json))
}

/// [`remap_stored`] for every stored value of `page_id`, a copy of `source`.
pub(super) async fn remap_page(
    tx: &mut Transaction<'_, Any>,
    page_id: Id,
    source: Id,
    copies: &FileCopies,
) -> Result<(), DatabaseError> {
    let stored =
        sqlx::query("SELECT property_id, value_json FROM page_property_values WHERE page_id = $1")
            .bind(page_id.to_string())
            .fetch_all(&mut **tx)
            .await?;
    for row in stored {
        let property_id: String = row.get("property_id");
        let value_json: String = row.get("value_json");
        match remap_stored(value_json.clone(), source, copies) {
            Some(remapped) if remapped == value_json => {}
            Some(remapped) => {
                sqlx::query(
                    "UPDATE page_property_values SET value_json = $1 WHERE page_id = $2 AND property_id = $3",
                )
                .bind(remapped)
                .bind(page_id.to_string())
                .bind(&property_id)
                .execute(&mut **tx)
                .await?;
            }
            None => {
                sqlx::query(
                    "DELETE FROM page_property_values WHERE page_id = $1 AND property_id = $2",
                )
                .bind(page_id.to_string())
                .bind(&property_id)
                .execute(&mut **tx)
                .await?;
            }
        }
    }
    Ok(())
}

fn remap(files: &mut Vec<FileRef>, source: Id, copies: &FileCopies) {
    files.retain_mut(|file| match copies.get(&(source, file.file_id)) {
        Some((_, copy)) => {
            file.file_id = *copy;
            true
        }
        None => false,
    });
}

/// The download URL of the first image (by detected type) in a row's files value, for card
/// covers (the gallery view). `None` without an image.
pub async fn first_image_url(
    tx: &mut Transaction<'_, Any>,
    workspace_id: Id,
    row_id: Id,
    value: &CellValue,
) -> Result<Option<String>, sqlx::Error> {
    let CellValue::Files(files) = value else {
        return Ok(None);
    };
    let images: HashSet<String> = sqlx::query(
        "SELECT id, mime_type FROM page_files WHERE page_id = $1 AND workspace_id = $2",
    )
    .bind(row_id.to_string())
    .bind(workspace_id.to_string())
    .fetch_all(&mut **tx)
    .await?
    .into_iter()
    .filter(|row| INLINE_IMAGE_TYPES.contains(&row.get::<String, _>("mime_type").as_str()))
    .map(|row| row.get("id"))
    .collect();
    Ok(files
        .iter()
        .find(|file| images.contains(&file.file_id.to_string()))
        .map(|file| page_file_url(workspace_id, row_id, file.file_id)))
}
