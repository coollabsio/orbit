//! Docs databases (P1): a page of kind `database` holds typed properties, saved table and board
//! views and rows. A row is a page whose parent is the database page, so rows get co-editing, comments,
//! history, trash and search as pages. A row's property values live in `page_property_values`
//! (a missing row is an empty value); the `title`, `created_*` and `last_edited_*` properties store
//! nothing and read the row's page columns. Access is the database page's: whoever sees the page
//! reads and writes it. The page lock freezes properties and views, not rows or values.

use std::collections::{BTreeMap, HashMap, HashSet};

use orbit_platform::Row;
use orbit_platform::{Database, Id, TimestampMillis};
use serde::Serialize;
use serde_json::{Value, json};
use sqlx::{Any, Transaction};
use thiserror::Error;
use utoipa::ToSchema;

use super::billing::{TrialCapacityError, TrialLimitReached, ensure_trial_capacity};
use super::pages::{PageError, PageKind, PageRecord, PageSpace, page_in_tx, renumber};
pub use super::task_filter::GroupOp;
use super::tasks::{TaskError, require_access_tx};
use crate::audit::{self, AuditOutcome};
use crate::billing::TrialLimit;
use crate::collab::CollabHub;

mod agent;
mod board;
mod buttons;
mod calendar;
mod contact;
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
mod query;
mod search;
mod templates;
mod timeline;
mod values;
mod view_config;
mod views;
pub use buttons::{ButtonAction, ButtonConfig, ButtonPress, CreatedRow, MAX_ACTIONS};
pub use calendar::CalendarMode;
pub use computed::{ComputeScope, RelatedPage, is_computed};
pub use files::{FileRef, MAX_FILES, first_image_url};
pub(super) use files::FileCopies;
pub use unique_id::UniqueIdConfig;
pub(super) use unique_id::renumber_copy;
pub use values::{CellValue, Conversion, DatePoint, DateValue, accepts_value, convert};
use values::{display_text, parse_value, scalar_from_text, valid_url};
pub use view_config::{
    Calculation, FilterGroup, FilterNode, FilterOperator, FilterRule, SortDirection, ViewColumn,
    ViewConfig, ViewSort, ViewType,
};
use views::{insert_view, raw_views, views_in_tx, write_view_config};
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
pub use search::row_search_lines;
use search::{SearchRows, refresh_search_text};
use templates::Children;
pub use templates::{DatabaseTemplate, NewTemplate, TemplateChanges};
pub use timeline::TimelineZoom;
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

    pub async fn create_row(
        &self,
        workspace_id: Id,
        database_id: Id,
        actor_id: Id,
        input: NewRow,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<DatabaseRow, DatabaseError> {
        self.insert_row(
            Id::new_v7(),
            workspace_id,
            database_id,
            actor_id,
            input,
            None,
            request_id,
            now,
        )
        .await
    }

    /// [`Self::create_row`] with a caller-chosen id, so a resumable job (the Notion import) can
    /// record the id before the row exists. Unique ID numbers in `input` are kept.
    #[allow(clippy::too_many_arguments)]
    pub async fn create_row_with_id(
        &self,
        row_id: Id,
        workspace_id: Id,
        database_id: Id,
        actor_id: Id,
        mut input: NewRow,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<DatabaseRow, DatabaseError> {
        let mut tx = self.database.immediate_transaction().await?;
        let properties = properties_in_tx(&mut tx, database_id).await?;
        let numbers = unique_id::take_imported(&properties, &mut input.values);
        let mut row = Self::insert_row_in_tx(
            &self.database,
            &mut tx,
            row_id,
            workspace_id,
            database_id,
            actor_id,
            input,
            None,
            request_id,
            now,
        )
        .await?;
        unique_id::keep_imported(
            &mut tx,
            workspace_id,
            database_id,
            &mut row,
            numbers,
            actor_id,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(row)
    }

    /// A new row, filled from `template` (its id and the day "today" means) when given.
    #[allow(clippy::too_many_arguments)]
    async fn insert_row(
        &self,
        row_id: Id,
        workspace_id: Id,
        database_id: Id,
        actor_id: Id,
        input: NewRow,
        template: Option<(Id, String)>,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<DatabaseRow, DatabaseError> {
        let mut tx = self.database.immediate_transaction().await?;
        let row = Self::insert_row_in_tx(
            &self.database,
            &mut tx,
            row_id,
            workspace_id,
            database_id,
            actor_id,
            input,
            template,
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(row)
    }

    /// [`Self::insert_row`] inside the caller's write transaction (a button's `add_row`).
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn insert_row_in_tx(
        database: &Database,
        tx: &mut Transaction<'_, Any>,
        row_id: Id,
        workspace_id: Id,
        database_id: Id,
        actor_id: Id,
        mut input: NewRow,
        template: Option<(Id, String)>,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<DatabaseRow, DatabaseError> {
        let page = open(tx, workspace_id, database_id, actor_id).await?;
        ensure_row_capacity(tx, database_id, 1).await?;
        ensure_trial_capacity(database, tx, workspace_id, TrialLimit::Pages, 1).await?;
        let mut properties = properties_in_tx(tx, database_id).await?;
        let fill = match template {
            Some((template_id, today)) => Some(
                templates::fill(
                    tx,
                    workspace_id,
                    database_id,
                    template_id,
                    actor_id,
                    &today,
                    &properties,
                )
                .await?,
            ),
            None => None,
        };
        if let Some(fill) = &fill {
            fill.apply(&mut input);
        }
        let mut values = parse_values(tx, workspace_id, &properties, &input.values).await?;
        let space = page.space(actor_id);
        let mut siblings =
            templates::row_ids(tx, workspace_id, space, database_id, row_id).await?;
        let index = placement_index(&siblings, input.placement)?;
        siblings.insert(index, row_id);
        let search_values: HashMap<Id, CellValue> = values
            .iter()
            .filter_map(|(id, value)| Some((*id, value.clone()?)))
            .collect();
        sqlx::query(
            "INSERT INTO pages (id, workspace_id, parent_id, teamspace_id, owner_id, title, icon, position, \
             creator_id, updated_by, version, created_at, updated_at, content_json, content_text, property_text) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, 0, $11, $12, '[]', '', $13)",
        )
        .bind(row_id.to_string())
        .bind(workspace_id.to_string())
        .bind(database_id.to_string())
        .bind(space.teamspace_column())
        .bind(space.owner_column())
        .bind(&input.title)
        .bind(&input.icon)
        .bind(index as i64)
        .bind(actor_id.to_string())
        .bind(actor_id.to_string())
        .bind(now.as_millis())
        .bind(now.as_millis())
        .bind(search::search_text(&properties, &search_values))
        .execute(&mut **tx)
        .await?;
        renumber(tx, &siblings).await?;
        if let Some(fill) = &fill {
            let copies = templates::copy_body(
                tx,
                workspace_id,
                fill.page_id,
                &fill.content,
                row_id,
                actor_id,
                now,
            )
            .await?;
            files::remap_values(&mut values, fill.page_id, &copies);
        }
        files::check_page_files(tx, workspace_id, row_id, &mut values).await?;
        let row = row_id.to_string();
        for (property_id, value) in &values {
            if let Some(CellValue::Pages(ids)) = value {
                relations::write_cell(tx, workspace_id, actor_id, row_id, *property_id, ids, now)
                    .await?;
                continue;
            }
            if let Some(value) = value {
                upsert_value(
                    tx,
                    workspace_id,
                    &row,
                    *property_id,
                    value,
                    actor_id,
                    now,
                )
                .await?;
            }
        }
        let numbered =
            unique_id::number_rows(tx, workspace_id, database_id, &mut properties, actor_id, now)
                .await?;
        if !numbered.is_empty() {
            let rows = if numbered == [row_id] {
                SearchRows::One(row_id)
            } else {
                SearchRows::All
            };
            refresh_search_text(tx, workspace_id, database_id, &properties, rows).await?;
        }
        record_with(
            tx,
            workspace_id,
            actor_id,
            "database.row_created",
            database_id,
            json!({ "row_id": row }),
            request_id,
            now,
        )
        .await?;
        let scope = ComputeScope::utc(workspace_id, actor_id);
        single_row(tx, &scope, database_id, row_id).await
    }

    /// Sets (or clears, with `null`) values of a row. Last write wins; the row's `updated_*`
    /// change, its version does not.
    #[allow(clippy::too_many_arguments)]
    pub async fn update_values(
        &self,
        workspace_id: Id,
        database_id: Id,
        row_id: Id,
        actor_id: Id,
        input: Vec<(Id, Value)>,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<DatabaseRow, DatabaseError> {
        let mut tx = self.database.immediate_transaction().await?;
        open(&mut tx, workspace_id, database_id, actor_id).await?;
        // A template's values are written here too (its "me" and "today" kept aside).
        let children = templates::child_kind(&mut tx, database_id, row_id)
            .await?
            .ok_or(DatabaseError::NotFound)?;
        let properties = properties_in_tx(&mut tx, database_id).await?;
        let (input, placeholders) = match children {
            Children::Templates => templates::split_placeholders(&properties, input),
            Children::Rows => (input, Vec::new()),
        };
        let mut values = parse_values(&mut tx, workspace_id, &properties, &input).await?;
        files::check_page_files(&mut tx, workspace_id, row_id, &mut values).await?;
        write_values(&mut tx, workspace_id, row_id, &values, actor_id, now).await?;
        templates::write_placeholders(&mut tx, row_id, placeholders, now).await?;
        relations::notify_dependents(
            &mut tx,
            workspace_id,
            actor_id,
            database_id,
            request_id,
            now,
        )
        .await?;
        refresh_search_text(
            &mut tx,
            workspace_id,
            database_id,
            &properties,
            SearchRows::One(row_id),
        )
        .await?;
        record_with(
            &mut tx,
            workspace_id,
            actor_id,
            "database.values_updated",
            database_id,
            json!({
                "row_id": row_id.to_string(),
                "property_ids": values.iter().map(|(id, _)| id.to_string()).collect::<Vec<_>>(),
            }),
            request_id,
            now,
        )
        .await?;
        let scope = ComputeScope::utc(workspace_id, actor_id);
        let row = match children {
            Children::Rows => single_row(&mut tx, &scope, database_id, row_id).await?,
            Children::Templates => {
                templates::template_row(&mut tx, &scope, database_id, row_id).await?
            }
        };
        tx.commit().await?;
        Ok(row)
    }

    /// Trashes live rows of the database, each with its sub-pages as its own trash batch (so each
    /// one restores on its own, back into the database). Ids that are not live rows of this
    /// database are skipped, so a retry converges.
    pub async fn trash_rows(
        &self,
        workspace_id: Id,
        database_id: Id,
        actor_id: Id,
        row_ids: &[Id],
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), DatabaseError> {
        let mut tx = self.database.immediate_transaction().await?;
        let page = open(&mut tx, workspace_id, database_id, actor_id).await?;
        let space = page.space(actor_id);
        let live: HashSet<Id> =
            templates::row_ids(&mut tx, workspace_id, space, database_id, database_id)
                .await?
                .into_iter()
                .collect();
        let mut trashed = 0;
        for row_id in row_ids.iter().filter(|id| live.contains(id)) {
            trashed += sqlx::query(
                "WITH RECURSIVE subtree(id) AS ( \
                     SELECT $1 \
                     UNION ALL \
                     SELECT pages.id FROM pages JOIN subtree ON pages.parent_id = subtree.id \
                     WHERE pages.deleted_at IS NULL \
                 ) \
                 UPDATE pages SET deleted_at = $2, trashed_with = $3, updated_by = $4, updated_at = $5, version = version + 1 \
                 WHERE workspace_id = $6 AND deleted_at IS NULL AND id IN (SELECT id FROM subtree)",
            )
            .bind(row_id.to_string())
            .bind(now.as_millis())
            .bind(row_id.to_string())
            .bind(actor_id.to_string())
            .bind(now.as_millis())
            .bind(workspace_id.to_string())
            .execute(&mut *tx)
            .await?
            .rows_affected();
        }
        if trashed == 0 {
            tx.rollback().await?;
            return Ok(());
        }
        let siblings =
            templates::row_ids(&mut tx, workspace_id, space, database_id, database_id).await?;
        renumber(&mut tx, &siblings).await?;
        record_with(
            &mut tx,
            workspace_id,
            actor_id,
            "database.rows_trashed",
            database_id,
            json!({ "pages": trashed }),
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        CollabHub::revalidate_database(&self.database, Some(workspace_id));
        Ok(())
    }

    /// Puts a row first (`None`) or right after another row (manual order), and writes `values`
    /// in the same transaction (a board card moved to another column).
    #[allow(clippy::too_many_arguments)]
    pub async fn move_row(
        &self,
        workspace_id: Id,
        database_id: Id,
        row_id: Id,
        actor_id: Id,
        after: Option<Id>,
        values: Vec<(Id, Value)>,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<DatabaseRow, DatabaseError> {
        let mut tx = self.database.immediate_transaction().await?;
        let page = open(&mut tx, workspace_id, database_id, actor_id).await?;
        require_row(&mut tx, database_id, row_id).await?;
        if !values.is_empty() {
            let properties = properties_in_tx(&mut tx, database_id).await?;
            let values = parse_values(&mut tx, workspace_id, &properties, &values).await?;
            write_values(&mut tx, workspace_id, row_id, &values, actor_id, now).await?;
            refresh_search_text(
                &mut tx,
                workspace_id,
                database_id,
                &properties,
                SearchRows::One(row_id),
            )
            .await?;
        }
        let mut siblings = templates::row_ids(
            &mut tx,
            workspace_id,
            page.space(actor_id),
            database_id,
            row_id,
        )
        .await?;
        let placement = after.map_or(RowPlacement::First, RowPlacement::After);
        let index = placement_index(&siblings, placement)?;
        siblings.insert(index, row_id);
        renumber(&mut tx, &siblings).await?;
        record_with(
            &mut tx,
            workspace_id,
            actor_id,
            "database.row_moved",
            database_id,
            json!({ "row_id": row_id.to_string() }),
            request_id,
            now,
        )
        .await?;
        let scope = ComputeScope::utc(workspace_id, actor_id);
        let row = single_row(&mut tx, &scope, database_id, row_id).await?;
        tx.commit().await?;
        Ok(row)
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

async fn require_row(
    tx: &mut Transaction<'_, Any>,
    database_id: Id,
    row_id: Id,
) -> Result<(), DatabaseError> {
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT 1 FROM pages WHERE id = $1 AND parent_id = $2 AND deleted_at IS NULL AND {NOT_TEMPLATE}"
    )))
        .bind(row_id.to_string())
        .bind(database_id.to_string())
        .fetch_optional(&mut **tx)
        .await?
        .map(|_| ())
        .ok_or(DatabaseError::NotFound)
}

fn insert_index(position: Option<i64>, len: usize) -> usize {
    position.map_or(len, |position| {
        usize::try_from(position.max(0)).unwrap_or(len).min(len)
    })
}

fn placement_index(siblings: &[Id], placement: RowPlacement) -> Result<usize, DatabaseError> {
    match placement {
        RowPlacement::Last => Ok(siblings.len()),
        RowPlacement::First => Ok(0),
        RowPlacement::After(after) => siblings
            .iter()
            .position(|id| *id == after)
            .map(|index| index + 1)
            .ok_or_else(|| DatabaseError::invalid("after_row_id")),
    }
}

/// Parses each value against its property (and person ids against the members), so the write
/// itself cannot fail on input. Errors name `values.<property id>`.
async fn parse_values(
    tx: &mut Transaction<'_, Any>,
    workspace_id: Id,
    properties: &[Property],
    input: &[(Id, Value)],
) -> Result<Vec<(Id, Option<CellValue>)>, DatabaseError> {
    let mut members: Option<HashSet<String>> = None;
    let mut values = Vec::with_capacity(input.len());
    for (property_id, raw) in input {
        let path = || DatabaseError::invalid(format!("values.{property_id}"));
        let property = properties
            .iter()
            .find(|property| property.id == *property_id)
            .ok_or_else(path)?;
        // Unique ID numbers are the server's.
        if matches!(property.kind, PropertyKind::UniqueId(_)) {
            return Err(path());
        }
        let value = parse_value(&property.kind, raw).map_err(|()| path())?;
        // An empty relation cell still replaces its visible links.
        let value = match (&property.kind, value) {
            (PropertyKind::Relation(_), None) => Some(CellValue::Pages(Vec::new())),
            (_, value) => value,
        };
        if let Some(CellValue::People(people)) = &value {
            if members.is_none() {
                members = Some(
                    sqlx::query_scalar::<_, String>(
                        "SELECT user_id FROM memberships WHERE workspace_id = $1 AND role <> 'guest'",
                    )
                    .bind(workspace_id.to_string())
                    .fetch_all(&mut **tx)
                    .await?
                    .into_iter()
                    .collect(),
                );
            }
            let members = members.as_ref().ok_or(DatabaseError::Corrupt)?;
            if !people.iter().all(|id| members.contains(&id.to_string())) {
                return Err(path());
            }
        }
        values.push((*property_id, value));
    }
    Ok(values)
}

/// Writes parsed values (`None` clears) and stamps the row's `updated_at` and `updated_by`.
async fn write_values(
    tx: &mut Transaction<'_, Any>,
    workspace_id: Id,
    row_id: Id,
    values: &[(Id, Option<CellValue>)],
    actor_id: Id,
    now: TimestampMillis,
) -> Result<(), DatabaseError> {
    let row = row_id.to_string();
    for (property_id, value) in values {
        match value {
            Some(CellValue::Pages(ids)) => {
                relations::write_cell(tx, workspace_id, actor_id, row_id, *property_id, ids, now)
                    .await?;
            }
            Some(value) => {
                upsert_value(tx, workspace_id, &row, *property_id, value, actor_id, now).await?;
            }
            None => delete_value(tx, &row, *property_id).await?,
        }
    }
    sqlx::query(
            "UPDATE pages SET updated_by = $1, updated_at = CASE WHEN updated_at < $2 THEN $3 ELSE updated_at + 1 END \
             WHERE id = $4",
        )
        .bind(actor_id.to_string())
        .bind(now.as_millis())
        .bind(now.as_millis())
        .bind(&row)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

async fn upsert_value(
    tx: &mut Transaction<'_, Any>,
    workspace_id: Id,
    row: &str,
    property_id: Id,
    value: &CellValue,
    actor_id: Id,
    now: TimestampMillis,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO page_property_values (page_id, property_id, workspace_id, value_json, updated_by, updated_at) \
         VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT (page_id, property_id) DO UPDATE \
         SET value_json = excluded.value_json, updated_by = excluded.updated_by, updated_at = excluded.updated_at",
    )
    .bind(row)
    .bind(property_id.to_string())
    .bind(workspace_id.to_string())
    .bind(value.to_json().to_string())
    .bind(actor_id.to_string())
    .bind(now.as_millis())
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Replaces a stored value without touching who changed it last (conversions, option removal).
async fn update_value(
    tx: &mut Transaction<'_, Any>,
    row: &str,
    property_id: Id,
    value: &CellValue,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE page_property_values SET value_json = $1 WHERE page_id = $2 AND property_id = $3",
    )
    .bind(value.to_json().to_string())
    .bind(row)
    .bind(property_id.to_string())
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn delete_value(
    tx: &mut Transaction<'_, Any>,
    row: &str,
    property_id: Id,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM page_property_values WHERE page_id = $1 AND property_id = $2")
        .bind(row)
        .bind(property_id.to_string())
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Live rows in manual order (one row with `only`), with their stored values.
async fn rows_in_tx(
    tx: &mut Transaction<'_, Any>,
    database_id: Id,
    only: Option<Id>,
) -> Result<Vec<DatabaseRow>, DatabaseError> {
    children_in_tx(tx, database_id, Children::Rows, only).await
}

/// Live rows or templates in manual order (one with `only`), with their stored values.
async fn children_in_tx(
    tx: &mut Transaction<'_, Any>,
    database_id: Id,
    children: Children,
    only: Option<Id>,
) -> Result<Vec<DatabaseRow>, DatabaseError> {
    let only = only.map(|id| id.to_string());
    let filter = children.filter();
    let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT id, title, icon, position, version, created_at, creator_id, updated_at, updated_by, \
         content_text <> '' AS has_content FROM pages \
         WHERE parent_id = $1 AND deleted_at IS NULL AND ($2 IS NULL OR id = $3) AND {filter} \
         ORDER BY position, id"
    )))
    .bind(database_id.to_string())
    .bind(&only)
    .bind(&only)
    .fetch_all(&mut **tx)
    .await?;
    let stored = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT page_property_values.page_id AS page_id, page_property_values.property_id AS property_id, \
         page_property_values.value_json AS value_json FROM page_property_values \
         JOIN pages ON pages.id = page_property_values.page_id \
         WHERE pages.parent_id = $1 AND pages.deleted_at IS NULL AND ($2 IS NULL OR pages.id = $3) \
         AND {filter}"
    )))
    .bind(database_id.to_string())
    .bind(&only)
    .bind(&only)
    .fetch_all(&mut **tx)
    .await?;
    let mut values: HashMap<String, BTreeMap<String, Value>> = HashMap::new();
    for row in stored {
        let value: Value = serde_json::from_str(&row.get::<String, _>("value_json"))
            .map_err(|_| DatabaseError::Corrupt)?;
        values
            .entry(row.get("page_id"))
            .or_default()
            .insert(row.get("property_id"), value);
    }
    rows.into_iter()
        .map(|row| {
            let id: String = row.get("id");
            Ok(DatabaseRow {
                id: parse_id(&id)?,
                title: row.get("title"),
                icon: row.get("icon"),
                position: row.get("position"),
                version: u64::try_from(row.get::<i64, _>("version"))
                    .map_err(|_| DatabaseError::Corrupt)?,
                created_at: TimestampMillis::from_millis(row.get("created_at")),
                created_by: parse_id(&row.get::<String, _>("creator_id"))?,
                updated_at: TimestampMillis::from_millis(row.get("updated_at")),
                updated_by: parse_id(&row.get::<String, _>("updated_by"))?,
                has_content: row.get("has_content"),
                values: values.remove(&id).unwrap_or_default(),
                hidden_links: BTreeMap::new(),
                preview: None,
            })
        })
        .collect()
}

/// One live row with its computed values (in UTC), as a write returns it.
async fn single_row(
    tx: &mut Transaction<'_, Any>,
    scope: &ComputeScope,
    database_id: Id,
    row_id: Id,
) -> Result<DatabaseRow, DatabaseError> {
    let mut rows = rows_in_tx(tx, database_id, Some(row_id)).await?;
    if rows.is_empty() {
        return Err(DatabaseError::NotFound);
    }
    let properties = properties_in_tx(tx, database_id).await?;
    computed::fill(tx, scope, database_id, &properties, &mut rows).await?;
    rows.pop().ok_or(DatabaseError::NotFound)
}

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
    properties: Vec<Property>,
    views: Vec<DatabaseView>,
    /// Rows to create with the database (a CSV import).
    rows: Option<import::ImportRows>,
}

impl DatabaseSchema {
    /// Rows created with the database.
    pub(super) fn row_count(&self) -> usize {
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
pub(super) async fn insert_schema(
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

fn page_error(error: DatabaseError) -> PageError {
    match error {
        DatabaseError::Unavailable(error) => PageError::Unavailable(error),
        _ => PageError::Corrupt,
    }
}

/// The database data of a set of pages, keyed by their original ids: the properties and views
/// of each database, and the values of each page.
#[derive(Default)]
pub(super) struct DatabaseData {
    pub properties: Vec<(Id, Property)>,
    pub views: Vec<(Id, DatabaseView)>,
    /// `(page, property, value_json)`.
    pub values: Vec<(Id, Id, String)>,
    /// The templates of the databases.
    pub templates: Vec<TemplateCopy>,
}

/// The live data of the databases in `databases` and the values of the pages in `pages`.
pub(super) async fn load_database_data(
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
pub(super) async fn write_database_data(
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
pub(super) async fn retain_fitting_values(
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
pub(super) async fn copy_database_data(
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
