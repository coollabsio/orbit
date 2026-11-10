//! Relation properties (spec P3 item 9, decision D-P3-5): links from rows of one database to rows
//! of a target database (or the same one). Each link is one `page_relations` row, stored under
//! the pair's primary property only. The synced property of a two-way pair (in the target
//! database) reads the same rows from the other side: `property_id` = the primary property and
//! `related_page_id` = its row. Nothing is ever copied between the two sides.
//!
//! Reads ([`fill`]) list only related pages the reader can read and count the others in
//! `hidden_links`; trashed pages are left out until a restore. Writes ([`write_cell`]) replace
//! the cell's visible links and keep the hidden ones.

use std::collections::{BTreeMap, HashMap, HashSet};

use orbit_platform::Row;
use orbit_platform::{Id, TimestampMillis};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{Any, Transaction};

use super::computed::{ComputeScope, Computed, RelatedPage};
use super::rollups::{self, RollupConfig};
use super::templates::NOT_TEMPLATE;
use super::{
    DatabaseError, DatabaseRow, MAX_CELL_ITEMS, MAX_NAME_CHARS, MAX_PROPERTIES, Property,
    PropertyKind, Views, insert_property, open, parse_id, properties_ids, properties_in_tx,
    raw_views, record_with, renumber_rows, unlocked, write_view_config,
};

/// Links one relation cell may hold for a write, on either side (spec 19).
pub const MAX_LINKS: usize = MAX_CELL_ITEMS;

/// Pages the reader can read: teamspace pages and the reader's private pages (binds the reader).
fn readable(param: usize) -> String {
    format!("(pages.teamspace_id IS NOT NULL OR pages.owner_id = ${param})")
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationLimit {
    #[default]
    None,
    One,
}

/// The config of a relation property. Stored as `{database_id, limit, synced_property_id,
/// primary}`; a client sends `{database_id, limit, two_way}`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelationConfig {
    /// The target database.
    pub database_id: Id,
    #[serde(default)]
    pub limit: RelationLimit,
    /// The other property of a two-way pair, in the target database.
    #[serde(default)]
    pub synced_property_id: Option<Id>,
    /// The links of the pair are stored under this property (a one-way relation is primary).
    #[serde(default = "primary")]
    pub primary: bool,
    /// Input only: "Show on <target database>". Absent keeps the pairing as it is.
    #[serde(default, skip_serializing)]
    pub two_way: Option<bool>,
}

const fn primary() -> bool {
    true
}

impl RelationConfig {
    fn stored(&self) -> Self {
        Self {
            two_way: None,
            ..self.clone()
        }
    }

    /// The `page_relations` key of this property's cells: `(property_id, the cell's row column,
    /// the related page column)`; `None` for a synced property whose primary is gone.
    fn side(&self, property_id: Id) -> Option<(Id, &'static str, &'static str)> {
        if self.primary {
            Some((property_id, "page_id", "related_page_id"))
        } else {
            self.synced_property_id
                .map(|primary| (primary, "related_page_id", "page_id"))
        }
    }
}

/// Parses a relation value: page ids, unique, at most 100 (one with `limit: one`). `[]` and
/// `null` are empty.
pub(super) fn parse_ids(config: &RelationConfig, raw: &Value) -> Result<Option<Vec<Id>>, ()> {
    let list = match raw {
        Value::Null => return Ok(None),
        Value::Array(list) => list,
        _ => return Err(()),
    };
    let max = match config.limit {
        RelationLimit::One => 1,
        RelationLimit::None => MAX_LINKS,
    };
    if list.len() > max {
        return Err(());
    }
    let mut ids = Vec::with_capacity(list.len());
    for item in list {
        let id: Id = item.as_str().ok_or(())?.parse().map_err(|_| ())?;
        if ids.contains(&id) {
            return Err(());
        }
        ids.push(id);
    }
    Ok((!ids.is_empty()).then_some(ids))
}

/// Puts the relation values of `rows` into their `values` (visible page ids, in order) and
/// `hidden_links`, and returns the visible related pages.
pub(super) async fn fill(
    tx: &mut Transaction<'_, Any>,
    scope: &ComputeScope,
    properties: &[Property],
    rows: &mut [DatabaseRow],
) -> Result<Computed, DatabaseError> {
    let mut computed = Computed::default();
    let only = match rows {
        [row] => Some(row.id.to_string()),
        _ => None,
    };
    let index: HashMap<String, usize> = rows
        .iter()
        .enumerate()
        .map(|(at, row)| (row.id.to_string(), at))
        .collect();
    for property in properties {
        let PropertyKind::Relation(config) = &property.kind else {
            continue;
        };
        let Some((key, mine, other)) = config.side(property.id) else {
            continue;
        };
        let order = if config.primary {
            "page_relations.position, page_relations.created_at, pages.id"
        } else {
            "page_relations.created_at, pages.id"
        };
        let links = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT page_relations.{mine} AS row_id, pages.id AS page_id, pages.title AS title, \
             pages.icon AS icon, CASE WHEN {readable} THEN 1 ELSE 0 END AS readable \
             FROM page_relations JOIN pages ON pages.id = page_relations.{other} \
             WHERE page_relations.property_id = $1 AND pages.parent_id = $3 \
             AND pages.deleted_at IS NULL AND ($4 IS NULL OR page_relations.{mine} = $5) \
             ORDER BY {order}",
            readable = readable(2),
        )))
        .bind(key.to_string())
        .bind(scope.actor_id.to_string())
        .bind(config.database_id.to_string())
        .bind(&only)
        .bind(&only)
        .fetch_all(&mut **tx)
        .await?;
        let mut cells: HashMap<usize, (Vec<Value>, u64)> = HashMap::new();
        for link in links {
            let Some(at) = index.get(&link.get::<String, _>("row_id")) else {
                continue;
            };
            let page: String = link.get("page_id");
            let cell = cells.entry(*at).or_default();
            if link.get::<i64, _>("readable") == 1 {
                cell.0.push(Value::String(page.clone()));
                computed
                    .related_pages
                    .entry(page)
                    .or_insert_with(|| RelatedPage {
                        title: link.get("title"),
                        icon: link.get("icon"),
                        database_id: config.database_id,
                    });
            } else {
                cell.1 += 1;
            }
        }
        let key = property.id.to_string();
        for (at, (ids, hidden)) in cells {
            let row = &mut rows[at];
            if !ids.is_empty() {
                row.values.insert(key.clone(), Value::Array(ids));
            }
            if hidden > 0 {
                row.hidden_links.insert(key.clone(), hidden);
            }
        }
    }
    Ok(computed)
}

/// Rollups of `rows` (relations already filled); see `rollups::fill`.
pub(super) async fn fill_rollups(
    tx: &mut Transaction<'_, Any>,
    scope: &ComputeScope,
    properties: &[Property],
    rows: &mut [DatabaseRow],
    depth: u8,
) -> Result<(), DatabaseError> {
    rollups::fill(tx, scope, properties, rows, depth).await
}

/// Live rows of `database_id` that `actor_id` can read.
async fn readable_rows(
    tx: &mut Transaction<'_, Any>,
    database_id: Id,
    actor_id: Id,
) -> Result<HashSet<String>, DatabaseError> {
    Ok(sqlx::query_scalar::<_, String>(sqlx::AssertSqlSafe(format!(
        "SELECT id FROM pages WHERE parent_id = $1 AND deleted_at IS NULL AND {readable} \
         AND {NOT_TEMPLATE}",
        readable = readable(2),
    )))
    .bind(database_id.to_string())
    .bind(actor_id.to_string())
    .fetch_all(&mut **tx)
    .await?
    .into_iter()
    .collect())
}

/// Replaces the visible links of one relation cell (`row_id`, `property_id`) with `ids`, in
/// order. Each id must be a live row of the target database that the writer can read; links to
/// pages the writer cannot read (and to trashed pages) stay. A synced cell with more than
/// [`MAX_LINKS`] links is read-only ("Edit from <other database>").
pub(super) async fn write_cell(
    tx: &mut Transaction<'_, Any>,
    workspace_id: Id,
    actor_id: Id,
    row_id: Id,
    property_id: Id,
    ids: &[Id],
    now: TimestampMillis,
) -> Result<(), DatabaseError> {
    let invalid = || DatabaseError::invalid(format!("values.{property_id}"));
    let (_, property) = property_by_id(tx, property_id).await?.ok_or_else(invalid)?;
    let PropertyKind::Relation(config) = &property.kind else {
        return Err(invalid());
    };
    let (key, mine, other) = config.side(property_id).ok_or_else(invalid)?;
    let allowed = readable_rows(tx, config.database_id, actor_id).await?;
    if ids.iter().any(|id| !allowed.contains(&id.to_string())) {
        return Err(invalid());
    }
    let current: Vec<(String, i64)> = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT {other} AS page_id, position FROM page_relations \
         WHERE property_id = $1 AND {mine} = $2 ORDER BY position"
    )))
    .bind(key.to_string())
    .bind(row_id.to_string())
    .fetch_all(&mut **tx)
    .await?
    .into_iter()
    .map(|row| (row.get("page_id"), row.get("position")))
    .collect();
    let visible: Vec<&str> = current
        .iter()
        .map(|(page, _)| page.as_str())
        .filter(|page| allowed.contains(*page))
        .collect();
    if !config.primary && visible.len() > MAX_LINKS {
        return Err(DatabaseError::invalid(format!(
            "values.{property_id}.too_many_links"
        )));
    }
    let wanted: HashSet<String> = ids.iter().map(ToString::to_string).collect();
    let row = row_id.to_string();
    for page in visible.iter().filter(|page| !wanted.contains(**page)) {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "DELETE FROM page_relations WHERE property_id = $1 AND {mine} = $2 AND {other} = $3"
        )))
        .bind(key.to_string())
        .bind(&row)
        .bind(*page)
        .execute(&mut **tx)
        .await?;
    }
    let existing: HashSet<&str> = current.iter().map(|(page, _)| page.as_str()).collect();
    for (at, id) in ids.iter().enumerate() {
        let page = id.to_string();
        let (page_id, related_page_id) = if config.primary {
            (row.as_str(), page.as_str())
        } else {
            (page.as_str(), row.as_str())
        };
        if existing.contains(page.as_str()) {
            if config.primary {
                sqlx::query(
                    "UPDATE page_relations SET position = $1 \
                     WHERE property_id = $2 AND page_id = $3 AND related_page_id = $4",
                )
                .bind(at as i64)
                .bind(key.to_string())
                .bind(page_id)
                .bind(related_page_id)
                .execute(&mut **tx)
                .await?;
            }
            continue;
        }
        // The reverse side of a new link goes last in the primary cell of the other row.
        let position = if config.primary {
            at as i64
        } else {
            sqlx::query_scalar::<_, i64>(
                "SELECT COALESCE(MAX(position) + 1, 0) FROM page_relations \
                 WHERE property_id = $1 AND page_id = $2",
            )
            .bind(key.to_string())
            .bind(page_id)
            .fetch_one(&mut **tx)
            .await?
        };
        sqlx::query(
            "INSERT INTO page_relations (property_id, page_id, related_page_id, workspace_id, position, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT (property_id, page_id, related_page_id) DO NOTHING",
        )
        .bind(key.to_string())
        .bind(page_id)
        .bind(related_page_id)
        .bind(workspace_id.to_string())
        .bind(position)
        .bind(now.as_millis())
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

async fn property_by_id(
    tx: &mut Transaction<'_, Any>,
    property_id: Id,
) -> Result<Option<(Id, Property)>, DatabaseError> {
    let row = sqlx::query(
        "SELECT id, database_id, name, type, config_json, position FROM database_properties WHERE id = $1",
    )
    .bind(property_id.to_string())
    .fetch_optional(&mut **tx)
    .await?;
    row.map(|row| {
        Ok((
            parse_id(&row.get::<String, _>("database_id"))?,
            Property::from_stored(
                &row.get::<String, _>("id"),
                row.get("name"),
                &row.get::<String, _>("type"),
                &row.get::<String, _>("config_json"),
                row.get("position"),
            )?,
        ))
    })
    .transpose()
}

/// Checks the config of a new or changed relation or rollup property of `database_id` before it
/// is written, and sets the stored shape: a relation needs read access to its target (two-way
/// needs an unlocked target), a rollup a relation of this database and a target property it can
/// read one level deep.
pub(super) async fn check(
    tx: &mut Transaction<'_, Any>,
    scope: &ComputeScope,
    database_id: Id,
    properties: &[Property],
    property: &mut Property,
) -> Result<(), DatabaseError> {
    match &mut property.kind {
        PropertyKind::Relation(config) => {
            let target = open(tx, scope.workspace_id, config.database_id, scope.actor_id)
                .await
                .map_err(|_| DatabaseError::invalid("config.database_id"))?;
            if config.two_way == Some(true) && config.synced_property_id.is_none() {
                unlocked(&target)?;
            }
            Ok(())
        }
        PropertyKind::Rollup(config) => rollups::check(tx, database_id, properties, config).await,
        _ => Ok(()),
    }
}

/// After a relation property is inserted or its config changed: creates the synced property
/// when `two_way` turned on, deletes it when it turned off, and stores the config in its stored
/// shape. `previous` is the config before a change. Returns the stored kind.
#[allow(clippy::too_many_arguments)]
pub(super) async fn sync_pair(
    tx: &mut Transaction<'_, Any>,
    scope: &ComputeScope,
    database_id: Id,
    database_title: &str,
    property: &Property,
    previous: Option<&RelationConfig>,
    request_id: &str,
    now: TimestampMillis,
) -> Result<PropertyKind, DatabaseError> {
    let PropertyKind::Relation(config) = &property.kind else {
        return Ok(property.kind.clone());
    };
    let mut stored = config.stored();
    if let Some(previous) = previous {
        if !previous.primary && previous.database_id != config.database_id {
            return Err(DatabaseError::invalid("config.database_id"));
        }
        if previous.database_id != config.database_id {
            // A new target: the old links and the old pair go.
            drop_property_links(tx, property.id, previous).await?;
            if let Some(synced) = previous.synced_property_id {
                delete_synced(tx, scope, synced, request_id, now).await?;
            }
            stored.synced_property_id = None;
            stored.primary = true;
        } else {
            stored.synced_property_id = previous.synced_property_id;
            stored.primary = previous.primary;
        }
    }
    match config.two_way {
        Some(true) if stored.synced_property_id.is_none() => {
            let synced = create_synced(
                tx,
                scope,
                database_id,
                database_title,
                property.id,
                config.database_id,
                request_id,
                now,
            )
            .await?;
            stored.synced_property_id = Some(synced);
            stored.primary = true;
        }
        Some(false) => {
            if let Some(synced) = stored.synced_property_id {
                // The pair is switched off on its primary side.
                if !stored.primary {
                    return Err(DatabaseError::invalid("config.two_way"));
                }
                delete_synced(tx, scope, synced, request_id, now).await?;
                stored.synced_property_id = None;
            }
        }
        _ => {}
    }
    let kind = PropertyKind::Relation(stored);
    sqlx::query("UPDATE database_properties SET config_json = $1 WHERE id = $2")
        .bind(kind.config_json().to_string())
        .bind(property.id.to_string())
        .execute(&mut **tx)
        .await?;
    Ok(kind)
}

/// Creates the synced property of `primary_id` in `target` (named after the source database,
/// made unique), with a column in each of the target's views. Returns its id.
#[allow(clippy::too_many_arguments)]
async fn create_synced(
    tx: &mut Transaction<'_, Any>,
    scope: &ComputeScope,
    source: Id,
    source_title: &str,
    primary_id: Id,
    target: Id,
    request_id: &str,
    now: TimestampMillis,
) -> Result<Id, DatabaseError> {
    let mut properties = properties_in_tx(tx, target).await?;
    if properties.len() >= MAX_PROPERTIES {
        return Err(DatabaseError::PropertyLimit);
    }
    let base: String = match source_title.trim() {
        "" => "Related".to_owned(),
        title => title.chars().take(MAX_NAME_CHARS - 8).collect(),
    };
    let taken: HashSet<String> = properties
        .iter()
        .map(|property| property.name.to_lowercase())
        .collect();
    let name = (1..)
        .map(|n| {
            if n == 1 {
                base.clone()
            } else {
                format!("{base} {n}")
            }
        })
        .find(|name| !taken.contains(&name.to_lowercase()))
        .unwrap_or(base);
    let synced = Property {
        id: Id::new_v7(),
        name,
        kind: PropertyKind::Relation(RelationConfig {
            database_id: source,
            limit: RelationLimit::None,
            synced_property_id: Some(primary_id),
            primary: false,
            two_way: None,
        }),
        position: properties.len() as i64,
    };
    insert_property(tx, scope.workspace_id, target, &synced, now).await?;
    properties.push(synced.clone());
    renumber_rows(tx, "database_properties", &properties_ids(&properties)).await?;
    for mut view in raw_views(tx, target, Views::All).await? {
        view.config.columns.push(synced.column());
        write_view_config(tx, view.id, &view.config, now).await?;
    }
    if target != source {
        record_with(
            tx,
            scope.workspace_id,
            scope.actor_id,
            "database.property_created",
            target,
            json!({}),
            request_id,
            now,
        )
        .await?;
    }
    Ok(synced.id)
}

/// Deletes the synced property `synced` (its links stay under the primary).
async fn delete_synced(
    tx: &mut Transaction<'_, Any>,
    scope: &ComputeScope,
    synced: Id,
    request_id: &str,
    now: TimestampMillis,
) -> Result<(), DatabaseError> {
    let Some((target, _)) = property_by_id(tx, synced).await? else {
        return Ok(());
    };
    sqlx::query("DELETE FROM database_properties WHERE id = $1")
        .bind(synced.to_string())
        .execute(&mut **tx)
        .await?;
    let properties = properties_in_tx(tx, target).await?;
    renumber_rows(tx, "database_properties", &properties_ids(&properties)).await?;
    for mut view in raw_views(tx, target, Views::All).await? {
        let before = view.config.clone();
        view.config.repair(view.view_type, &properties);
        if view.config != before {
            write_view_config(tx, view.id, &view.config, now).await?;
        }
    }
    record_with(
        tx,
        scope.workspace_id,
        scope.actor_id,
        "database.property_deleted",
        target,
        json!({}),
        request_id,
        now,
    )
    .await?;
    Ok(())
}

/// Makes `property_id` (the other side of a pair) one-way and primary; with `links_from`, the
/// links of that primary property move to it first (swapped).
async fn unpair(
    tx: &mut Transaction<'_, Any>,
    property_id: Id,
    links_from: Option<Id>,
) -> Result<(), DatabaseError> {
    let Some((_, property)) = property_by_id(tx, property_id).await? else {
        return Ok(());
    };
    let PropertyKind::Relation(config) = property.kind else {
        return Ok(());
    };
    if let Some(primary) = links_from {
        sqlx::query(
            "INSERT INTO page_relations (property_id, page_id, related_page_id, workspace_id, position, created_at) \
             SELECT $1, related_page_id, page_id, workspace_id, created_at, created_at FROM page_relations \
             WHERE property_id = $2 ON CONFLICT (property_id, page_id, related_page_id) DO NOTHING",
        )
        .bind(property_id.to_string())
        .bind(primary.to_string())
        .execute(&mut **tx)
        .await?;
        sqlx::query("DELETE FROM page_relations WHERE property_id = $1")
            .bind(primary.to_string())
            .execute(&mut **tx)
            .await?;
    }
    let kind = PropertyKind::Relation(RelationConfig {
        synced_property_id: None,
        primary: true,
        ..config
    });
    sqlx::query("UPDATE database_properties SET config_json = $1 WHERE id = $2")
        .bind(kind.config_json().to_string())
        .bind(property_id.to_string())
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Deletes the links that `property_id` stores (a primary property; a synced one stores none).
async fn drop_property_links(
    tx: &mut Transaction<'_, Any>,
    property_id: Id,
    config: &RelationConfig,
) -> Result<(), DatabaseError> {
    if config.primary {
        sqlx::query("DELETE FROM page_relations WHERE property_id = $1")
            .bind(property_id.to_string())
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}

/// Before a relation property is deleted, or changes to another type (which clears its
/// values): a primary with a synced property hands its links to it (the synced property
/// becomes primary, spec 11.3) on delete, and drops them on a type change; a synced property
/// leaves the primary one-way with its links.
pub(super) async fn detach(
    tx: &mut Transaction<'_, Any>,
    property: &Property,
    deleting: bool,
) -> Result<(), DatabaseError> {
    let PropertyKind::Relation(config) = &property.kind else {
        return Ok(());
    };
    match (config.primary, config.synced_property_id) {
        (true, Some(synced)) if deleting => unpair(tx, synced, Some(property.id)).await,
        (true, Some(synced)) => {
            drop_property_links(tx, property.id, config).await?;
            unpair(tx, synced, None).await
        }
        (true, None) => drop_property_links(tx, property.id, config).await,
        (false, Some(primary)) => unpair(tx, primary, None).await,
        (false, None) => Ok(()),
    }
}

/// A row left its database (spec 11.3, item 8): its links go on both sides.
pub(in crate::repositories) async fn drop_row_links(
    tx: &mut Transaction<'_, Any>,
    page_id: Id,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM page_relations WHERE page_id = $1 OR related_page_id = $2")
        .bind(page_id.to_string())
        .bind(page_id.to_string())
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// The kind of a copied property in a duplicated database: relations keep their target and
/// become one-way (spec 11.3), rollups follow the copied relation.
pub(super) fn copied_kind(kind: &PropertyKind, ids: &HashMap<Id, Id>) -> PropertyKind {
    match kind {
        PropertyKind::Relation(config) => PropertyKind::Relation(RelationConfig {
            synced_property_id: None,
            primary: true,
            ..config.stored()
        }),
        PropertyKind::Rollup(config) => PropertyKind::Rollup(RollupConfig {
            relation_property_id: ids
                .get(&config.relation_property_id)
                .copied()
                .unwrap_or(config.relation_property_id),
            ..config.clone()
        }),
        other => other.clone(),
    }
}

/// Copies the links of copied relation properties (`ids`: old property id → new) for the
/// copied rows (`pages`: old page id → new). Each copy stores its links itself (one-way).
pub(super) async fn copy_links(
    tx: &mut Transaction<'_, Any>,
    workspace_id: Id,
    properties: &[(Id, Property)],
    ids: &HashMap<Id, Id>,
    pages: &HashMap<Id, Id>,
    now: TimestampMillis,
) -> Result<(), DatabaseError> {
    for (_, property) in properties {
        let PropertyKind::Relation(config) = &property.kind else {
            continue;
        };
        let (Some((key, mine, other)), Some(copy)) =
            (config.side(property.id), ids.get(&property.id))
        else {
            continue;
        };
        let links = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT {mine} AS row_id, {other} AS page_id, position FROM page_relations WHERE property_id = $1"
        )))
        .bind(key.to_string())
        .fetch_all(&mut **tx)
        .await?;
        for link in links {
            let Some(row) = pages.get(&parse_id(&link.get::<String, _>("row_id"))?) else {
                continue;
            };
            sqlx::query(
                "INSERT INTO page_relations (property_id, page_id, related_page_id, workspace_id, position, created_at) \
                 VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT (property_id, page_id, related_page_id) DO NOTHING",
            )
            .bind(copy.to_string())
            .bind(row.to_string())
            .bind(link.get::<String, _>("page_id"))
            .bind(workspace_id.to_string())
            .bind(link.get::<i64, _>("position"))
            .bind(now.as_millis())
            .execute(&mut **tx)
            .await?;
        }
    }
    Ok(())
}

/// After a value write in `database_id`: one realtime event for every other database whose
/// relations target it (their cells, rollups and related titles read its rows).
pub(super) async fn notify_dependents(
    tx: &mut Transaction<'_, Any>,
    workspace_id: Id,
    actor_id: Id,
    database_id: Id,
    request_id: &str,
    now: TimestampMillis,
) -> Result<(), DatabaseError> {
    let needle = format!("%\"database_id\":\"{database_id}\"%");
    let dependents = sqlx::query_scalar::<_, String>(
        "SELECT DISTINCT database_id FROM database_properties \
         WHERE workspace_id = $1 AND type = 'relation' AND config_json LIKE $2 AND database_id <> $3",
    )
    .bind(workspace_id.to_string())
    .bind(needle)
    .bind(database_id.to_string())
    .fetch_all(&mut **tx)
    .await?;
    for dependent in dependents {
        record_with(
            tx,
            workspace_id,
            actor_id,
            "database.related_changed",
            parse_id(&dependent)?,
            json!({ "database_id": database_id.to_string() }),
            request_id,
            now,
        )
        .await?;
    }
    Ok(())
}

/// The titles of related pages of one relation cell, for exports: visible pages only.
pub(super) fn titles(value: Option<&Value>, related: &BTreeMap<String, RelatedPage>) -> String {
    value
        .and_then(Value::as_array)
        .map(|ids| {
            ids.iter()
                .filter_map(Value::as_str)
                .filter_map(|id| related.get(id))
                .map(|page| page.title.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default()
}
