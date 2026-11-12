//! Linked view blocks: a `database` block with a `linkId` shows an existing database with views of
//! its own (`database_links`, `database_views.link_id`). Reading them needs read access to the
//! database; changing them needs the host page (visible to the caller and not locked), not the
//! database. The editor never deletes a link: a deleted block can come back with undo or from
//! page history, and the host page's permanent delete removes its links (cascade).

use std::collections::HashMap;

use orbit_platform::{Id, TimestampMillis};
use serde::Serialize;
use serde_json::{Value, json};
use sqlx::{Any, Transaction};
use utoipa::ToSchema;

use super::{
    DatabaseError, DatabaseRepository, DatabaseView, Views, insert_view, open, page_error,
    parse_id, properties_in_tx, raw_views, record_with, unlocked,
};
use crate::repositories::pages::{PageError, page_in_tx};

/// The most views one linked view block keeps.
pub const MAX_LINK_VIEWS: usize = 20;

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct DatabaseLink {
    #[schema(value_type = String)]
    pub id: Id,
    #[schema(value_type = String)]
    pub database_id: Id,
    /// The page whose block shows the link.
    #[schema(value_type = String)]
    pub host_page_id: Id,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: TimestampMillis,
}

/// A new link and its views.
#[derive(Clone, Debug, Serialize, ToSchema)]
#[schema(as = DatabaseLinkedViews)]
pub struct LinkedViews {
    pub link: DatabaseLink,
    pub views: Vec<DatabaseView>,
}

impl DatabaseRepository {
    /// Links the database to a block of `host_page_id`, with one view: a copy of `from_view`
    /// (an own view; by default the first one).
    #[allow(clippy::too_many_arguments)]
    pub async fn create_link(
        &self,
        workspace_id: Id,
        database_id: Id,
        actor_id: Id,
        host_page_id: Id,
        from_view: Option<Id>,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<LinkedViews, DatabaseError> {
        let mut tx = self.database.immediate_transaction().await?;
        open(&mut tx, workspace_id, database_id, actor_id).await?;
        let host = page_in_tx(&mut tx, workspace_id, host_page_id, actor_id, false)
            .await
            .map_err(|error| match error {
                PageError::NotFound => DatabaseError::invalid("host_page_id"),
                error => error.into(),
            })?;
        unlocked(&host)?;
        let own = raw_views(&mut tx, database_id, Views::Own).await?;
        let source = match from_view {
            Some(view_id) => own
                .into_iter()
                .find(|view| view.id == view_id)
                .ok_or_else(|| DatabaseError::invalid("from_view_id"))?,
            None => own.into_iter().next().ok_or(DatabaseError::Corrupt)?,
        };
        let properties = properties_in_tx(&mut tx, database_id).await?;
        let link = DatabaseLink {
            id: Id::new_v7(),
            database_id,
            host_page_id,
            created_at: now,
        };
        insert_link(&mut tx, workspace_id, &link, Some(actor_id)).await?;
        let mut view = DatabaseView {
            id: Id::new_v7(),
            position: 0,
            ..source
        };
        insert_view(
            &mut tx,
            workspace_id,
            database_id,
            Some(link.id),
            &view,
            now,
        )
        .await?;
        record_with(
            &mut tx,
            workspace_id,
            actor_id,
            "database.link_created",
            database_id,
            json!({"link_id": link.id, "host_page_id": host_page_id}),
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        view.config.repair(view.view_type, &properties);
        Ok(LinkedViews {
            link,
            views: vec![view],
        })
    }

    /// The page whose block shows `link_id`, for a caller that can read the database.
    pub async fn link_host(
        &self,
        workspace_id: Id,
        database_id: Id,
        link_id: Id,
        actor_id: Id,
    ) -> Result<Id, DatabaseError> {
        let mut tx = self.database.transaction().await?;
        open(&mut tx, workspace_id, database_id, actor_id).await?;
        let host = require_link(&mut tx, database_id, link_id).await;
        tx.rollback().await?;
        host
    }

    /// Deletes a link and its views (cleanup tools; the editor never calls it).
    pub async fn delete_link(
        &self,
        workspace_id: Id,
        database_id: Id,
        link_id: Id,
        actor_id: Id,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), DatabaseError> {
        let mut tx = self.database.immediate_transaction().await?;
        super::writable_views(&mut tx, workspace_id, database_id, actor_id, Some(link_id)).await?;
        sqlx::query("DELETE FROM database_links WHERE id = $1")
            .bind(link_id.to_string())
            .execute(&mut *tx)
            .await?;
        record_with(
            &mut tx,
            workspace_id,
            actor_id,
            "database.link_deleted",
            database_id,
            json!({"link_id": link_id}),
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }
}

/// The host page of `link_id`, a link of `database_id`.
pub(super) async fn require_link(
    tx: &mut Transaction<'_, Any>,
    database_id: Id,
    link_id: Id,
) -> Result<Id, DatabaseError> {
    let host: Option<String> = sqlx::query_scalar(
        "SELECT host_page_id FROM database_links WHERE id = $1 AND database_id = $2",
    )
    .bind(link_id.to_string())
    .bind(database_id.to_string())
    .fetch_optional(&mut **tx)
    .await?;
    host.as_deref()
        .map(parse_id)
        .transpose()?
        .ok_or(DatabaseError::LinkNotFound)
}

async fn insert_link(
    tx: &mut Transaction<'_, Any>,
    workspace_id: Id,
    link: &DatabaseLink,
    created_by: Option<Id>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO database_links (id, workspace_id, database_id, host_page_id, created_by, created_at) \
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(link.id.to_string())
    .bind(workspace_id.to_string())
    .bind(link.database_id.to_string())
    .bind(link.host_page_id.to_string())
    .bind(created_by.map(|id| id.to_string()))
    .bind(link.created_at.as_millis())
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// The `database` block that shows `link_id` of `database_id`, for the end of a document.
#[must_use]
pub fn link_block(database_id: Id, link_id: Id) -> Value {
    json!({
        "id": Id::new_v7().to_string(),
        "type": "database",
        "props": {"databaseId": database_id.to_string(), "linkId": link_id.to_string()},
        "children": [],
    })
}

/// `blocks` without the `database` blocks that show `link_id`, nested blocks included.
#[must_use]
pub fn without_link_blocks(blocks: &[Value], link_id: Id) -> Vec<Value> {
    let link = link_id.to_string();
    blocks
        .iter()
        .filter(|block| !(block["type"] == "database" && block["props"]["linkId"] == link))
        .map(|block| {
            let mut block = block.clone();
            if let Some(children) = block["children"].as_array() {
                block["children"] = Value::Array(without_link_blocks(children, link_id));
            }
            block
        })
        .collect()
}

/// The `linkId`s of the `database` blocks in `blocks`, nested blocks included, first seen first.
pub(in crate::repositories) fn block_links(blocks: &[Value]) -> Vec<Id> {
    let mut found = Vec::new();
    let mut stack: Vec<&Value> = blocks.iter().rev().collect();
    while let Some(block) = stack.pop() {
        if block["type"] == "database"
            && let Some(link_id) = block["props"]["linkId"]
                .as_str()
                .and_then(|id| id.parse().ok())
            && !found.contains(&link_id)
        {
            found.push(link_id);
        }
        if let Some(children) = block["children"].as_array() {
            stack.extend(children.iter().rev());
        }
    }
    found
}

/// The database of each link in `links` that exists in the workspace (link id → database id).
pub(in crate::repositories) async fn linked_databases(
    tx: &mut Transaction<'_, Any>,
    workspace_id: Id,
    links: &[Id],
) -> Result<HashMap<Id, Id>, PageError> {
    let mut found = HashMap::new();
    for link_id in links {
        let database: Option<String> = sqlx::query_scalar(
            "SELECT database_id FROM database_links WHERE id = $1 AND workspace_id = $2",
        )
        .bind(link_id.to_string())
        .bind(workspace_id.to_string())
        .fetch_optional(&mut **tx)
        .await?;
        if let Some(database) = database {
            found.insert(*link_id, parse_id(&database).map_err(page_error)?);
        }
    }
    Ok(found)
}

/// A link to copy with a duplicated page.
pub(in crate::repositories) struct LinkCopy {
    pub link_id: Id,
    pub copy_id: Id,
    /// The original link's database (a copied database is looked up in `pages`).
    pub database_id: Id,
    /// The copy of the page whose block shows the link.
    pub host_page_id: Id,
}

/// Writes each copy with copies of its link's views. A link to a copied database (`pages`:
/// original id → copy id) shows the copy, its views pointing at the copied properties.
pub(in crate::repositories) async fn copy_links(
    tx: &mut Transaction<'_, Any>,
    workspace_id: Id,
    copies: &[LinkCopy],
    pages: &HashMap<Id, Id>,
    property_ids: &HashMap<Id, Id>,
    actor_id: Id,
    now: TimestampMillis,
) -> Result<(), PageError> {
    for copy in copies {
        let database_id = pages
            .get(&copy.database_id)
            .copied()
            .unwrap_or(copy.database_id);
        let link = DatabaseLink {
            id: copy.copy_id,
            database_id,
            host_page_id: copy.host_page_id,
            created_at: now,
        };
        insert_link(tx, workspace_id, &link, Some(actor_id)).await?;
        let views = raw_views(tx, copy.database_id, Views::Link(copy.link_id))
            .await
            .map_err(page_error)?;
        for mut view in views {
            view.config.remap(property_ids);
            view.id = Id::new_v7();
            insert_view(tx, workspace_id, database_id, Some(link.id), &view, now)
                .await
                .map_err(page_error)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_link_blocks_drops_nested_blocks_of_that_link_only() {
        let (gone, kept) = (Id::new_v7(), Id::new_v7());
        let database = Id::new_v7();
        let blocks = vec![
            link_block(database, gone),
            json!({"type": "paragraph", "children": [link_block(database, gone), link_block(database, kept)]}),
            link_block(database, kept),
        ];
        let left = without_link_blocks(&blocks, gone);
        assert_eq!(block_links(&left), vec![kept]);
        assert_eq!(left.len(), 2);
        assert_eq!(left[0]["children"].as_array().map(Vec::len), Some(1));
    }

    #[test]
    fn block_links_finds_nested_links_once_in_document_order() {
        let first = Id::new_v7();
        let second = Id::new_v7();
        let blocks = vec![
            json!({"type": "database", "props": {"databaseId": "d", "linkId": first.to_string()}}),
            json!({"type": "database", "props": {"databaseId": "d", "linkId": ""}}),
            json!({"type": "paragraph", "children": [
                {"type": "database", "props": {"linkId": second.to_string()}},
                {"type": "page", "props": {"linkId": Id::new_v7().to_string()}},
            ]}),
            json!({"type": "database", "props": {"linkId": first.to_string()}}),
        ];
        assert_eq!(block_links(&blocks), vec![first, second]);
    }
}
