//! Docs databases (P1): a page of kind `database` holds typed properties, saved table and board
//! views and rows. A row is a page whose parent is the database page, so rows get co-editing, comments,
//! history, trash and search as pages. A row's property values live in `page_property_values`
//! (a missing row is an empty value); the `title`, `created_*` and `last_edited_*` properties store
//! nothing and read the row's page columns. Access is the database page's: whoever sees the page
//! reads and writes it. The page lock freezes properties and views, not rows or values.

use std::collections::BTreeMap;

use orbit_platform::{Database, Id, TimestampMillis};
use serde::Serialize;
use serde_json::{Value, json};
use sqlx::{Any, Transaction};
use thiserror::Error;
use utoipa::ToSchema;

use super::billing::{TrialCapacityError, TrialLimitReached};
use super::pages::{PageError, PageKind, PageRecord, page_in_tx};
pub use super::task_filter::GroupOp;
use super::tasks::{TaskError, require_access_tx};
use crate::audit::{self, AuditOutcome};

mod agent;
mod board;
mod buttons;
mod calendar;
mod contact;
mod copy;
mod files;
mod unique_id;
mod computed;
mod export;
mod forms;
mod formula;
mod gallery;
mod import;
mod links;
mod model;
mod moves;
mod properties;
mod relation_import;
mod relations;
mod rollups;
mod rows;
mod query;
mod schema;
mod search;
mod templates;
mod timeline;
mod values;
mod view_config;
mod views;
pub use buttons::{ButtonAction, ButtonConfig, ButtonPress, CreatedRow, MAX_ACTIONS};
pub use calendar::CalendarMode;
pub use computed::{ComputeScope, RelatedPage, is_computed};
pub(super) use copy::{
    DatabaseData, copy_database_data, retain_fitting_values, write_database_data,
};
pub use files::{FileRef, MAX_FILES, first_image_url};
pub(super) use files::FileCopies;
pub use unique_id::UniqueIdConfig;
pub(super) use unique_id::renumber_copy;
pub use agent::{AgentDatabase, AgentQuery, AgentRow, AgentRowPage, invalid_detail};
pub use forms::{FormAccess, FormConfig, FormQuestion, FormSubmission, MAX_SUBMISSIONS_PER_HOUR};
pub use formula::{
    FormulaConfig, FormulaPreview, FormulaProblem, FormulaSample, checked as check_formula,
};
pub use export::{ExportDatabase, ExportRow, cell_text, is_number};
pub use gallery::{CardPreview, CardSize, RowPreview};
pub use import::{
    CsvImport, ImportColumn, ImportColumnResult, ImportResult, ImportTarget, MAX_IMPORT_COLUMNS,
};
pub use links::{DatabaseLink, LinkedViews, MAX_LINK_VIEWS};
pub(super) use links::{LinkCopy, block_links, copy_links, linked_databases};
pub use model::{
    DateConfig, DateFormat, NumberConfig, NumberFormat, OptionColor, PropertyKind, PropertyType,
    SelectOption, StatusGroup, StatusOption, TimeFormat,
};
use model::{copy_name, default_status_options, select_options};
pub(super) use moves::carry_row_values;
use properties::{insert_property, properties_ids, properties_in_tx, renumber_rows};
pub use relation_import::{ImportedRelation, ImportedRollup, ImportedSide};
pub use relations::{MAX_LINKS, RelationConfig, RelationLimit};
pub use rollups::{RollupConfig, RollupFunction};
use rows::{
    children_in_tx, delete_value, insert_index, parse_values, require_row, rows_in_tx, single_row,
    update_value, upsert_value, write_values,
};
pub(super) use schema::insert_schema;
pub use schema::{DatabaseSchema, SchemaProperty, SchemaView};
pub use search::row_search_lines;
pub use templates::{DatabaseTemplate, NewTemplate, TemplateChanges};
pub use timeline::TimelineZoom;
pub use values::{CellValue, Conversion, DatePoint, DateValue, accepts_value, convert};
use values::{display_text, parse_value, scalar_from_text, valid_url};
pub use view_config::{
    Calculation, FilterGroup, FilterNode, FilterOperator, FilterRule, SortDirection, ViewColumn,
    ViewConfig, ViewSort, ViewType,
};
use views::{insert_view, raw_views, views_in_tx, write_view_config};
pub(super) use templates::{NOT_TEMPLATE, TemplateCopy, is_template};

pub const MAX_ROWS: i64 = 5_000;
pub const MAX_PROPERTIES: usize = 100;
pub const MAX_VIEWS: usize = 50;
pub const MAX_OPTIONS: usize = 200;
pub const MAX_CELL_ITEMS: usize = 100;
pub const MAX_SORTS: usize = 10;
pub const MAX_RULES: usize = 20;
/// Filter group levels, the root group included.
pub const MAX_FILTER_LEVELS: usize = 3;
pub const MAX_GROUP_CHILDREN: usize = 10;
pub const MAX_TEXT_CHARS: usize = 10_000;
pub const MAX_URL_CHARS: usize = 2_000;
pub const MAX_NAME_CHARS: usize = 200;
pub const MAX_TRASH_ROWS: usize = 500;
const MAX_OPTION_ID_CHARS: usize = 64;
const MIN_WIDTH: u16 = 32;
const MAX_WIDTH: u16 = 1_000;
const DEFAULT_WIDTH: u16 = 200;
const TITLE_WIDTH: u16 = 280;
const RELATIVE_DATES: [&str; 7] = [
    "today",
    "tomorrow",
    "yesterday",
    "one_week_ago",
    "one_week_from_now",
    "one_month_ago",
    "one_month_from_now",
];

/// A property as stored.
#[derive(Clone, Debug)]
pub struct Property {
    pub id: Id,
    pub name: String,
    pub kind: PropertyKind,
    pub position: i64,
}

impl Property {
    fn column(&self) -> ViewColumn {
        ViewColumn {
            property_id: self.id,
            width: if self.kind == PropertyKind::Title {
                TITLE_WIDTH
            } else {
                DEFAULT_WIDTH
            },
            hidden: false,
        }
    }

    fn output(&self) -> DatabaseProperty {
        DatabaseProperty {
            id: self.id,
            name: self.name.clone(),
            property_type: self.kind.property_type(),
            config: self.kind.config_json(),
            position: self.position,
        }
    }
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct DatabaseProperty {
    #[schema(value_type = String)]
    pub id: Id,
    pub name: String,
    #[serde(rename = "type")]
    pub property_type: PropertyType,
    /// By type: `number` `{format, currency?}`; `select` and `multi_select` `{options: [{id, name,
    /// color}]}`; `status` the same with each option's `group`; `date` `{date_format,
    /// time_format}`; `{}` otherwise.
    #[schema(value_type = Object)]
    pub config: Value,
    pub position: i64,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct DatabaseView {
    #[schema(value_type = String)]
    pub id: Id,
    pub name: String,
    #[serde(rename = "type")]
    pub view_type: ViewType,
    pub config: ViewConfig,
    pub position: i64,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct DatabaseRow {
    #[schema(value_type = String)]
    pub id: Id,
    pub title: String,
    #[schema(required = true)]
    pub icon: Option<String>,
    /// Manual order among the rows.
    pub position: i64,
    /// The row page's version (`expected_version` of its title and icon edits).
    pub version: u64,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: TimestampMillis,
    #[schema(value_type = String)]
    pub created_by: Id,
    #[schema(value_type = String, format = DateTime)]
    pub updated_at: TimestampMillis,
    #[schema(value_type = String)]
    pub updated_by: Id,
    /// The row page has body text.
    pub has_content: bool,
    /// Values by property id, stored and computed (relations, rollups); a missing property is
    /// empty.
    pub values: BTreeMap<String, Value>,
    /// Per relation property id: links of the cell to pages the reader cannot read (they are
    /// not in `values`). Absent: none.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub hidden_links: BTreeMap<String, u64>,
    /// What a gallery card shows of the row page; only when a view read with it is a gallery.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<RowPreview>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct DatabaseHead {
    #[schema(value_type = String)]
    pub id: Id,
    pub title: String,
    #[schema(required = true)]
    pub icon: Option<String>,
    /// The page lock: properties and views are read-only, rows and values stay writable.
    pub locked: bool,
    pub row_count: u64,
}

/// A whole database: its properties, views and live rows.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct DatabaseDetail {
    pub database: DatabaseHead,
    pub properties: Vec<DatabaseProperty>,
    pub views: Vec<DatabaseView>,
    pub rows: Vec<DatabaseRow>,
    /// Row templates in order (at most 20); they are not rows. Absent: none.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub templates: Vec<DatabaseTemplate>,
    /// Every page that a relation value of `rows` links to, by page id. Absent: none.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub related_pages: BTreeMap<String, RelatedPage>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[schema(as = DatabasePropertyUpdate)]
pub struct PropertyUpdate {
    pub property: DatabaseProperty,
    /// Values a type change converted.
    pub converted: u64,
    /// Values cleared by a type change or by removed options.
    pub cleared: u64,
}

pub enum NewProperty {
    Fresh {
        name: String,
        property_type: PropertyType,
        config: Option<Value>,
        position: Option<i64>,
    },
    /// A copy of another property with its config (new option ids) and values, placed right after
    /// it; `name` defaults to "<name> (copy)".
    Duplicate { source: Id, name: Option<String> },
}

#[derive(Default)]
pub struct PropertyChanges {
    pub name: Option<String>,
    pub property_type: Option<PropertyType>,
    pub config: Option<Value>,
    pub position: Option<i64>,
    /// Runs the change and returns its result without writing anything.
    pub dry_run: bool,
}

pub struct NewView {
    pub name: String,
    /// Ignored with `duplicate_of`: a copy keeps its source's type.
    pub view_type: ViewType,
    pub config: Option<ViewConfig>,
    pub duplicate_of: Option<Id>,
    /// A view of this linked view block; `None` is an own view of the database.
    pub link: Option<Id>,
}

#[derive(Default)]
pub struct ViewChanges {
    pub name: Option<String>,
    pub config: Option<ViewConfig>,
    pub position: Option<i64>,
    /// The view belongs to this linked view block.
    pub link: Option<Id>,
}

/// Where a row goes among the rows: last, first, or right after another row.
#[derive(Clone, Copy, Debug)]
pub enum RowPlacement {
    Last,
    First,
    After(Id),
}

pub struct NewRow {
    pub title: String,
    pub icon: Option<String>,
    pub values: Vec<(Id, Value)>,
    pub placement: RowPlacement,
}

#[derive(Debug, Error)]
pub enum DatabaseError {
    /// The database (or the row) is missing, trashed, not a database, or invisible to the caller.
    #[error("database was not found")]
    NotFound,
    #[error("database property was not found")]
    PropertyNotFound,
    #[error("database view was not found")]
    ViewNotFound,
    /// The linked view block's link is missing or belongs to another database.
    #[error("database link was not found")]
    LinkNotFound,
    #[error("database template was not found")]
    TemplateNotFound,
    #[error("database holds the most templates allowed")]
    TemplateLimit,
    #[error("database input is invalid: {field}")]
    Invalid { field: String },
    #[error("database is locked")]
    Locked,
    #[error("database holds the most rows allowed")]
    RowLimit,
    #[error("database holds the most properties allowed")]
    PropertyLimit,
    #[error("database holds the most views allowed")]
    ViewLimit,
    #[error("linked view holds the most views allowed")]
    LinkViewLimit,
    #[error("a database keeps at least one view")]
    LastView,
    /// The form view takes no answers (`access: closed`).
    #[error("database form is closed")]
    FormClosed,
    #[error("too many form submissions")]
    FormRateLimit,
    /// The title property cannot be deleted or change its type.
    #[error("the title property cannot change")]
    TitleProperty,
    #[error("trial limit reached")]
    TrialLimit(TrialLimitReached),
    /// A formula that does not parse or type check (422 `formula_invalid`).
    #[error("database formula is invalid: {}", .0.message)]
    FormulaInvalid(FormulaProblem),
    #[error("stored database data is invalid")]
    Corrupt,
    #[error("database repository is unavailable")]
    Unavailable(#[from] sqlx::Error),
}

impl DatabaseError {
    fn invalid(field: impl Into<String>) -> Self {
        Self::Invalid {
            field: field.into(),
        }
    }
}

impl From<PageError> for DatabaseError {
    fn from(error: PageError) -> Self {
        match error {
            PageError::Unavailable(error) => Self::Unavailable(error),
            PageError::TrialLimit(reached) => Self::TrialLimit(reached),
            PageError::NotFound => Self::NotFound,
            _ => Self::Corrupt,
        }
    }
}

impl From<TaskError> for DatabaseError {
    fn from(error: TaskError) -> Self {
        match error {
            TaskError::Unavailable(error) => Self::Unavailable(error),
            _ => Self::NotFound,
        }
    }
}

impl From<TrialCapacityError> for DatabaseError {
    fn from(error: TrialCapacityError) -> Self {
        match error {
            TrialCapacityError::Reached(reached) => Self::TrialLimit(reached),
            TrialCapacityError::Database(error) => Self::Unavailable(error),
        }
    }
}

impl From<RowCapacityError> for DatabaseError {
    fn from(error: RowCapacityError) -> Self {
        match error {
            RowCapacityError::Full => Self::RowLimit,
            RowCapacityError::Database(error) => Self::Unavailable(error),
        }
    }
}

#[derive(Debug, Error)]
pub enum RowCapacityError {
    #[error("database holds the most rows allowed")]
    Full,
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

/// Inside the write transaction that makes `adding` rows of `database_id` live (create, restore,
/// duplicate, backup restore): refuses when the database would then hold more than
/// [`MAX_ROWS`] live rows. Writers serialize on `immediate_transaction`, so two of them cannot
/// pass the cap together.
pub(super) async fn ensure_row_capacity(
    tx: &mut Transaction<'_, Any>,
    database_id: Id,
    adding: i64,
) -> Result<(), RowCapacityError> {
    let live: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT CAST(COUNT(*) AS BIGINT) FROM pages WHERE parent_id = $1 AND deleted_at IS NULL \
             AND {NOT_TEMPLATE}"
    )))
    .bind(database_id.to_string())
    .fetch_one(&mut **tx)
    .await?;
    if live.saturating_add(adding) > MAX_ROWS {
        return Err(RowCapacityError::Full);
    }
    Ok(())
}

#[derive(Clone)]
pub struct DatabaseRepository {
    database: Database,
}

impl DatabaseRepository {
    #[must_use]
    pub fn new(database: Database) -> Self {
        Self { database }
    }

    /// The database with its own views, or with the views of `link` (a linked view block), with
    /// computed values in UTC.
    pub async fn get(
        &self,
        workspace_id: Id,
        database_id: Id,
        actor_id: Id,
        link: Option<Id>,
    ) -> Result<DatabaseDetail, DatabaseError> {
        let scope = ComputeScope::utc(workspace_id, actor_id);
        self.get_with(&scope, database_id, link).await
    }

    /// [`Self::get`] with the reader's scope (its time zone for computed values).
    pub async fn get_with(
        &self,
        scope: &ComputeScope,
        database_id: Id,
        link: Option<Id>,
    ) -> Result<DatabaseDetail, DatabaseError> {
        let (workspace_id, actor_id) = (scope.workspace_id, scope.actor_id);
        let mut tx = self.database.transaction().await?;
        let page = open(&mut tx, workspace_id, database_id, actor_id).await?;
        let set = match link {
            Some(link_id) => {
                links::require_link(&mut tx, database_id, link_id).await?;
                Views::Link(link_id)
            }
            None => Views::Own,
        };
        let properties = properties_in_tx(&mut tx, database_id).await?;
        let mut views = views_in_tx(&mut tx, database_id, set, &properties).await?;
        let mut rows = rows_in_tx(&mut tx, database_id, None).await?;
        let mut computed =
            computed::fill(&mut tx, scope, database_id, &properties, &mut rows).await?;
        gallery::fill(&mut tx, database_id, &views, &mut rows).await?;
        let (templates, template_computed) =
            templates::templates_in_tx(&mut tx, scope, database_id, &properties, None).await?;
        computed.merge(template_computed);
        templates::repair_defaults(&mut views, &templates);
        tx.rollback().await?;
        Ok(DatabaseDetail {
            database: DatabaseHead {
                id: page.id,
                title: page.title,
                icon: page.icon,
                locked: page.locked_at.is_some(),
                row_count: rows.len() as u64,
            },
            properties: properties.iter().map(Property::output).collect(),
            views,
            rows,
            templates,
            related_pages: computed.related_pages,
        })
    }
}

/// The live database page the caller can see; anything else is `NotFound`.
async fn open(
    tx: &mut Transaction<'_, Any>,
    workspace_id: Id,
    database_id: Id,
    actor_id: Id,
) -> Result<PageRecord, DatabaseError> {
    require_access_tx(tx, workspace_id, actor_id).await?;
    let page = page_in_tx(tx, workspace_id, database_id, actor_id, false).await?;
    if page.kind == PageKind::Database {
        Ok(page)
    } else {
        Err(DatabaseError::NotFound)
    }
}

/// A set of views of one database.
#[derive(Clone, Copy, Debug)]
enum Views {
    /// The database's own views (its full page and inline blocks).
    Own,
    /// The views of one linked view block.
    Link(Id),
    /// Both, for changes every view follows (a new or deleted property).
    All,
}

impl Views {
    fn link(self) -> Option<Id> {
        match self {
            Self::Link(link_id) => Some(link_id),
            Self::Own | Self::All => None,
        }
    }
}

/// The views a caller who reads the database may change: its own views when the database is
/// not locked, or a link's views when its host page is visible to the caller and not locked.
async fn writable_views(
    tx: &mut Transaction<'_, Any>,
    workspace_id: Id,
    database_id: Id,
    actor_id: Id,
    link: Option<Id>,
) -> Result<Views, DatabaseError> {
    let page = open(tx, workspace_id, database_id, actor_id).await?;
    match link {
        None => {
            unlocked(&page)?;
            Ok(Views::Own)
        }
        Some(link_id) => {
            let host_id = links::require_link(tx, database_id, link_id).await?;
            let host = page_in_tx(tx, workspace_id, host_id, actor_id, false).await?;
            unlocked(&host)?;
            Ok(Views::Link(link_id))
        }
    }
}

/// Properties and views of a locked database are read-only.
fn unlocked(page: &PageRecord) -> Result<(), DatabaseError> {
    if page.locked_at.is_some() {
        Err(DatabaseError::Locked)
    } else {
        Ok(())
    }
}

fn page_error(error: DatabaseError) -> PageError {
    match error {
        DatabaseError::Unavailable(error) => PageError::Unavailable(error),
        _ => PageError::Corrupt,
    }
}

async fn record(
    tx: &mut Transaction<'_, Any>,
    workspace_id: Id,
    actor_id: Id,
    action: &str,
    database_id: Id,
    request_id: &str,
    now: TimestampMillis,
) -> Result<(), sqlx::Error> {
    record_with(
        tx,
        workspace_id,
        actor_id,
        action,
        database_id,
        json!({}),
        request_id,
        now,
    )
    .await
}

/// One audit event (and so one realtime event) of resource type `database` per write.
#[allow(clippy::too_many_arguments)]
async fn record_with(
    tx: &mut Transaction<'_, Any>,
    workspace_id: Id,
    actor_id: Id,
    action: &str,
    database_id: Id,
    metadata: Value,
    request_id: &str,
    now: TimestampMillis,
) -> Result<(), sqlx::Error> {
    audit::record(
        tx,
        workspace_id,
        Some(actor_id),
        action,
        AuditOutcome::Success,
        "database",
        Some(database_id),
        request_id,
        metadata,
        now,
    )
    .await
}

fn parse_id(value: &str) -> Result<Id, DatabaseError> {
    value.parse().map_err(|_| DatabaseError::Corrupt)
}
