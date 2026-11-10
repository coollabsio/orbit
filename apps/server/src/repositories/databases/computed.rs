//! The computed-value layer (spec P2/P3 section 2.3, decision D-P3-1): values the server derives
//! each time it reads a database and never stores. Relations (item 9) read their links from
//! `page_relations`; rollups (item 9), formulas (item 13) and later computed types derive from
//! the row's other values.
//!
//! API, for every read path and every computed property type:
//!
//! - [`ComputeScope`]: who reads (the workspace and the actor, whose access decides which related
//!   pages show) and the viewer's time zone (`GET …?tz=<IANA zone>`, default `UTC`) for date
//!   functions such as `today()`. Writes compute with [`ComputeScope::utc`].
//! - [`fill`]`(tx, scope, database_id, properties, rows)`: puts every computed value into
//!   `row.values`, in the same JSON shape as a stored value of its type, sets
//!   `row.hidden_links`, and returns the [`Computed`] side data of the response
//!   (`related_pages`). `DatabaseRepository::get` and `single_row` call it; a new read path that
//!   returns rows must call it too, or computed cells read as empty.
//! - Passes run in a fixed order, and a pass sees the results of the earlier ones in
//!   `row.values`:
//!   1. relations (`relations::fill`): link ids per cell, only pages the actor can read;
//!   2. rollups (`relations::fill_rollups`): computes the related rows of the target database
//!      one level deeper first, so a rollup over a formula or a rollup works one level deep;
//!   3. formulas (item 13): add the pass at the marked place in [`fill_at`]. It gets every value
//!      of the row, relations and rollups included, and `scope.tz`.
//! - [`is_computed`]: kinds whose values are never in `page_property_values` and never written
//!   through `PATCH …/values` as stored values. A new computed type adds its kind there.
//!
//! A computed value that fails (a formula's runtime error) is written as the type's error
//! shape, never as a request error: one bad cell must not fail the whole read.

use std::collections::BTreeMap;

use orbit_platform::Id;
use serde::Serialize;
use sqlx::{Any, Transaction};
use utoipa::ToSchema;

use super::{DatabaseError, DatabaseRow, Property, PropertyKind, relations};

/// How deep a read follows rollups into other databases: rollups of the read's own rows (depth
/// 0) and of their target rows (1) are computed; at depth 2 only relations and formulas are.
/// Saving a rollup refuses a deeper chain (the "one level deep" rule of spec 11.5).
const MAX_ROLLUP_DEPTH: u8 = 2;

const MAX_TZ_CHARS: usize = 64;

/// Who reads and in which time zone.
#[derive(Clone, Debug)]
pub struct ComputeScope {
    pub workspace_id: Id,
    pub actor_id: Id,
    /// An IANA zone name as the client sent it (`Europe/Budapest`); `UTC` by default.
    pub tz: String,
}

impl ComputeScope {
    #[must_use]
    pub fn utc(workspace_id: Id, actor_id: Id) -> Self {
        Self {
            workspace_id,
            actor_id,
            tz: "UTC".to_owned(),
        }
    }

    /// The scope of a read with the client's `?tz=`; `None` for a name that cannot be a zone.
    #[must_use]
    pub fn with_tz(workspace_id: Id, actor_id: Id, tz: Option<&str>) -> Option<Self> {
        let Some(tz) = tz else {
            return Some(Self::utc(workspace_id, actor_id));
        };
        let valid = !tz.is_empty()
            && tz.len() <= MAX_TZ_CHARS
            && tz
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_+-/".contains(&byte));
        valid.then(|| Self {
            workspace_id,
            actor_id,
            tz: tz.to_owned(),
        })
    }
}

/// A page that a relation cell of the response links to, and the reader can read.
#[derive(Clone, Debug, Serialize, ToSchema)]
#[schema(as = DatabaseRelatedPage)]
pub struct RelatedPage {
    pub title: String,
    #[schema(required = true)]
    pub icon: Option<String>,
    /// The database the page is a row of.
    #[schema(value_type = String)]
    pub database_id: Id,
}

/// What a read returns next to its rows.
#[derive(Debug, Default)]
pub struct Computed {
    /// Every related page the response's relation cells show, by page id.
    pub related_pages: BTreeMap<String, RelatedPage>,
}

impl Computed {
    pub(super) fn merge(&mut self, other: Computed) {
        self.related_pages.extend(other.related_pages);
    }
}

/// Kinds that have no stored value: computed on read, or (relation) stored in their own table.
#[must_use]
pub fn is_computed(kind: &PropertyKind) -> bool {
    matches!(kind, PropertyKind::Relation(_) | PropertyKind::Rollup(_))
}

/// Puts the computed values of `rows` (rows of `database_id`) into their `values`.
pub(super) async fn fill(
    tx: &mut Transaction<'_, Any>,
    scope: &ComputeScope,
    database_id: Id,
    properties: &[Property],
    rows: &mut [DatabaseRow],
) -> Result<Computed, DatabaseError> {
    fill_at(tx, scope, database_id, properties, rows, 0).await
}

/// [`fill`] for rows of another database that a computed value of the read reads (the targets of
/// a rollup), one level deeper. Its related pages are not part of the response; rollups read
/// their titles.
pub(super) async fn fill_nested(
    tx: &mut Transaction<'_, Any>,
    scope: &ComputeScope,
    database_id: Id,
    properties: &[Property],
    rows: &mut [DatabaseRow],
    depth: u8,
) -> Result<Computed, DatabaseError> {
    Box::pin(fill_at(tx, scope, database_id, properties, rows, depth + 1)).await
}

async fn fill_at(
    tx: &mut Transaction<'_, Any>,
    scope: &ComputeScope,
    database_id: Id,
    properties: &[Property],
    rows: &mut [DatabaseRow],
    depth: u8,
) -> Result<Computed, DatabaseError> {
    let mut computed = Computed::default();
    if rows.is_empty()
        || !properties
            .iter()
            .any(|property| is_computed(&property.kind))
    {
        return Ok(computed);
    }
    let _ = database_id;
    computed.merge(relations::fill(tx, scope, properties, rows).await?);
    if depth < MAX_ROLLUP_DEPTH {
        relations::fill_rollups(tx, scope, properties, rows, depth).await?;
    }
    // 3. Formulas (item 13) go here: a pure pass over each row's `values`.
    Ok(computed)
}
