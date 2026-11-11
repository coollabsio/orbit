//! Gallery views (spec P3 16.1): the card preview and size, and the preview of each row.

use std::collections::HashMap;

use orbit_platform::{Id, Row};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{Any, Transaction};
use utoipa::ToSchema;

use super::templates::Children;
use super::{
    DatabaseError, DatabaseRow, DatabaseView, Property, PropertyKind, ViewConfig, ViewType,
};

/// What a gallery card shows above its title.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
#[schema(as = DatabaseCardPreview)]
pub enum CardPreview {
    None,
    /// The row page's cover.
    Cover,
    /// The first image of the row page's body, else the start of its text.
    #[default]
    Content,
    /// The first image of a `files` property, by its id.
    Files(String),
}

impl CardPreview {
    pub(super) fn is_content(&self) -> bool {
        *self == Self::Content
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
#[schema(as = DatabaseCardSize)]
pub enum CardSize {
    Small,
    #[default]
    Medium,
    Large,
}

impl CardSize {
    pub(super) fn is_medium(&self) -> bool {
        *self == Self::Medium
    }
}

/// What a gallery card can show of a row page. Sent only when a view of the response is a gallery.
#[derive(Clone, Debug, Default, PartialEq, Serialize, ToSchema)]
#[schema(as = DatabaseRowPreview)]
pub struct RowPreview {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover_position: Option<String>,
    /// The first image of the body.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_url: Option<String>,
    /// The first 300 characters of the body text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

const PREVIEW_CHARS: i64 = 300;

const fn holds_files(property: &Property) -> bool {
    matches!(property.kind, PropertyKind::Files)
}

/// The gallery part of `ViewConfig::repair`: a files preview whose property is gone or holds no
/// files shows the body instead.
pub(super) fn repair(config: &mut ViewConfig, properties: &[Property]) {
    if let CardPreview::Files(id) = &config.card_preview
        && !properties
            .iter()
            .any(|property| property.id.to_string() == *id && holds_files(property))
    {
        config.card_preview = CardPreview::Content;
    }
}

/// Checks the gallery fields a client sends; errors name the field path.
pub(super) fn check(config: &ViewConfig, properties: &[Property]) -> Result<(), String> {
    if let CardPreview::Files(id) = &config.card_preview {
        let unfit = properties
            .iter()
            .find(|property| property.id.to_string() == *id)
            .is_some_and(|property| !holds_files(property));
        if unfit {
            return Err("config.card_preview".to_owned());
        }
    }
    Ok(())
}

/// Sets `preview` on every row when one of `views` is a gallery that shows a cover or the body.
pub(super) async fn fill(
    tx: &mut Transaction<'_, Any>,
    database_id: Id,
    views: &[DatabaseView],
    rows: &mut [DatabaseRow],
) -> Result<(), DatabaseError> {
    let wanted = views.iter().any(|view| {
        view.view_type == ViewType::Gallery
            && matches!(
                view.config.card_preview,
                CardPreview::Cover | CardPreview::Content
            )
    });
    if !wanted || rows.is_empty() {
        return Ok(());
    }
    let filter = Children::Rows.filter();
    let found = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT id, cover_url, cover_position, content_json, substr(content_text, 1, {PREVIEW_CHARS}) AS excerpt \
         FROM pages WHERE parent_id = $1 AND deleted_at IS NULL AND {filter}"
    )))
    .bind(database_id.to_string())
    .fetch_all(&mut **tx)
    .await?;
    let mut previews: HashMap<String, RowPreview> = found
        .into_iter()
        .map(|row| {
            let content: Option<String> = row.get("content_json");
            let excerpt: Option<String> = row.get("excerpt");
            let preview = RowPreview {
                cover_url: row.get("cover_url"),
                cover_position: row.get("cover_position"),
                image_url: content
                    .and_then(|json| serde_json::from_str::<Value>(&json).ok())
                    .and_then(|blocks| first_image(&blocks)),
                text: excerpt
                    .map(|text| text.trim().to_owned())
                    .filter(|text| !text.is_empty()),
            };
            (row.get::<String, _>("id"), preview)
        })
        .collect();
    for row in rows {
        row.preview = previews.remove(&row.id.to_string());
    }
    Ok(())
}

/// The URL of the first `image` block, depth first, in a BlockNote document.
fn first_image(blocks: &Value) -> Option<String> {
    blocks.as_array()?.iter().find_map(|block| {
        let url = (block.get("type")? == "image")
            .then(|| block.get("props")?.get("url")?.as_str())
            .flatten()
            .filter(|url| !url.is_empty());
        url.map(str::to_owned)
            .or_else(|| block.get("children").and_then(first_image))
    })
}
