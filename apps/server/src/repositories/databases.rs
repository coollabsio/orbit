//! Docs databases (P1): a page of kind `database` holds typed properties, saved table and board
//! views and rows. A row is a page whose parent is the database page, so rows get co-editing, comments,
//! history, trash and search as pages. A row's property values live in `page_property_values`
//! (a missing row is an empty value); the `title`, `created_*` and `last_edited_*` properties store
//! nothing and read the row's page columns. Access is the database page's: whoever sees the page
//! reads and writes it. The page lock freezes properties and views, not rows or values.

use std::collections::{BTreeMap, HashMap, HashSet};

use orbit_platform::Row;
use orbit_platform::{Database, Id, TimestampMillis};
use serde::{Deserialize, Serialize};
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
mod relation_import;
mod relations;
mod rollups;
mod query;
mod search;
mod templates;
mod timeline;
mod values;
pub use buttons::{ButtonAction, ButtonConfig, ButtonPress, CreatedRow, MAX_ACTIONS};
pub use calendar::CalendarMode;
pub use computed::{ComputeScope, RelatedPage, is_computed};
pub use files::{FileRef, MAX_FILES, first_image_url};
pub(super) use files::FileCopies;
pub use unique_id::UniqueIdConfig;
pub(super) use unique_id::renumber_copy;
pub use values::{CellValue, Conversion, DatePoint, DateValue, accepts_value, convert};
use values::{display_text, parse_value, scalar_from_text, valid_url};
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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
#[schema(as = DatabaseViewType)]
pub enum ViewType {
    #[default]
    Table,
    Board,
    List,
    Calendar,
    Gallery,
    Timeline,
    Form,
}

impl ViewType {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Table => "table",
            Self::Board => "board",
            Self::List => "list",
            Self::Calendar => "calendar",
            Self::Gallery => "gallery",
            Self::Timeline => "timeline",
            Self::Form => "form",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "table" => Some(Self::Table),
            "board" => Some(Self::Board),
            "list" => Some(Self::List),
            "calendar" => Some(Self::Calendar),
            "gallery" => Some(Self::Gallery),
            "timeline" => Some(Self::Timeline),
            "form" => Some(Self::Form),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
#[schema(as = DatabaseSortDirection)]
pub enum SortDirection {
    Asc,
    Desc,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
#[schema(as = DatabaseFilterOperator)]
pub enum FilterOperator {
    Equals,
    NotEquals,
    Contains,
    NotContains,
    StartsWith,
    EndsWith,
    IsEmpty,
    IsNotEmpty,
    #[serde(rename = "=")]
    Eq,
    #[serde(rename = "!=")]
    Ne,
    #[serde(rename = ">")]
    Gt,
    #[serde(rename = "<")]
    Lt,
    #[serde(rename = ">=")]
    Ge,
    #[serde(rename = "<=")]
    Le,
    Is,
    IsNot,
    Before,
    After,
    OnOrBefore,
    OnOrAfter,
}

/// What a filter rule's `value` holds, for a property type and operator.
#[derive(Clone, Copy)]
enum RuleValue {
    Nothing,
    Text,
    Number,
    Ids,
    /// User ids, where `"me"` stands for the viewer.
    People,
    Bool,
    Date,
}

/// The value a rule takes, or `None` when the operator does not apply to the type.
fn rule_value(property_type: PropertyType, operator: FilterOperator) -> Option<RuleValue> {
    use FilterOperator as Op;
    use PropertyType as Type;
    let empty = matches!(operator, Op::IsEmpty | Op::IsNotEmpty);
    Some(match property_type {
        Type::Title | Type::Text | Type::Url | Type::Email | Type::Phone => match operator {
            Op::Equals
            | Op::NotEquals
            | Op::Contains
            | Op::NotContains
            | Op::StartsWith
            | Op::EndsWith => RuleValue::Text,
            _ if empty => RuleValue::Nothing,
            _ => return None,
        },
        Type::Number | Type::UniqueId => match operator {
            Op::Eq | Op::Ne | Op::Gt | Op::Lt | Op::Ge | Op::Le => RuleValue::Number,
            _ if empty => RuleValue::Nothing,
            _ => return None,
        },
        Type::Select | Type::Status => match operator {
            Op::Is | Op::IsNot => RuleValue::Ids,
            _ if empty => RuleValue::Nothing,
            _ => return None,
        },
        Type::MultiSelect | Type::Person => match operator {
            Op::Contains | Op::NotContains if property_type == Type::Person => RuleValue::People,
            Op::Contains | Op::NotContains => RuleValue::Ids,
            _ if empty => RuleValue::Nothing,
            _ => return None,
        },
        Type::Relation => match operator {
            Op::Contains | Op::NotContains => RuleValue::Text,
            _ if empty => RuleValue::Nothing,
            _ => return None,
        },
        // By its result: a number, a date or a list of texts.
        Type::Rollup => match operator {
            Op::Contains | Op::NotContains => RuleValue::Text,
            Op::Eq | Op::Ne | Op::Gt | Op::Lt | Op::Ge | Op::Le => RuleValue::Number,
            Op::Is | Op::Before | Op::After | Op::OnOrBefore | Op::OnOrAfter => RuleValue::Date,
            _ if empty => RuleValue::Nothing,
            _ => return None,
        },
        Type::Checkbox => match operator {
            Op::Is => RuleValue::Bool,
            _ => return None,
        },
        Type::Files if empty => RuleValue::Nothing,
        Type::Files => return None,
        Type::Date | Type::CreatedTime | Type::LastEditedTime => match operator {
            Op::Is | Op::Before | Op::After | Op::OnOrBefore | Op::OnOrAfter => RuleValue::Date,
            _ if empty => RuleValue::Nothing,
            _ => return None,
        },
        Type::CreatedBy | Type::LastEditedBy => match operator {
            Op::Is | Op::IsNot => RuleValue::People,
            _ => return None,
        },
        // A list, person or page result (others filter as their result type).
        Type::Formula if empty => RuleValue::Nothing,
        Type::Button | Type::Formula => return None,
    })
}

/// A rule's value fits its kind; `null` always does (a half-made rule matches every row).
fn rule_value_fits(kind: RuleValue, value: &Value) -> bool {
    if value.is_null() {
        return true;
    }
    let ids = |value: &Value, me: bool| {
        value.as_array().is_some_and(|items| {
            items.len() <= MAX_CELL_ITEMS
                && items.iter().all(|item| {
                    item.as_str().is_some_and(|id| {
                        (me && id == "me")
                            || (!id.is_empty() && id.chars().count() <= MAX_OPTION_ID_CHARS)
                    })
                })
        })
    };
    match kind {
        RuleValue::Nothing => false,
        RuleValue::Text => value
            .as_str()
            .is_some_and(|text| text.chars().count() <= MAX_TEXT_CHARS),
        RuleValue::Number => value.as_f64().is_some_and(f64::is_finite),
        RuleValue::Ids => ids(value, false),
        RuleValue::People => value.as_str() == Some("me") || ids(value, true),
        RuleValue::Bool => value.is_boolean(),
        RuleValue::Date => value
            .as_str()
            .is_some_and(|text| RELATIVE_DATES.contains(&text) || DatePoint::parse(text).is_some()),
    }
}

/// A footer calculation of a table column (spec P2 4.3).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
#[schema(as = DatabaseCalculation)]
pub enum Calculation {
    CountAll,
    CountValues,
    CountUnique,
    CountEmpty,
    CountNotEmpty,
    PercentEmpty,
    PercentNotEmpty,
    Sum,
    Average,
    Median,
    Min,
    Max,
    Range,
    EarliestDate,
    LatestDate,
    DateRange,
    Checked,
    Unchecked,
    PercentChecked,
    PercentUnchecked,
}

impl Calculation {
    /// Whether the function applies to a property of `property_type`.
    const fn fits(self, property_type: PropertyType) -> bool {
        use Calculation as C;
        use PropertyType as Type;
        match self {
            C::CountAll => true,
            C::CountValues
            | C::CountUnique
            | C::CountEmpty
            | C::CountNotEmpty
            | C::PercentEmpty
            | C::PercentNotEmpty => !matches!(property_type, Type::Checkbox),
            C::Sum | C::Average | C::Median | C::Min | C::Max | C::Range => {
                matches!(property_type, Type::Number | Type::Rollup)
            }
            C::EarliestDate | C::LatestDate | C::DateRange => matches!(
                property_type,
                Type::Date | Type::CreatedTime | Type::LastEditedTime | Type::Rollup
            ),
            C::Checked | C::Unchecked | C::PercentChecked | C::PercentUnchecked => {
                matches!(property_type, Type::Checkbox)
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
#[schema(as = DatabaseViewColumn)]
pub struct ViewColumn {
    #[schema(value_type = String)]
    pub property_id: Id,
    /// 32 to 1,000 px.
    #[serde(default = "default_width")]
    pub width: u16,
    #[serde(default)]
    pub hidden: bool,
}

const fn default_width() -> u16 {
    DEFAULT_WIDTH
}

impl ViewColumn {
    /// A column with `width` kept in 32–1,000 px; without one, the default (wider for the title).
    #[must_use]
    pub fn new(property_id: Id, title: bool, width: Option<u64>, hidden: bool) -> Self {
        let width = match width {
            Some(width) => u16::try_from(width.clamp(u64::from(MIN_WIDTH), u64::from(MAX_WIDTH)))
                .unwrap_or(MAX_WIDTH),
            None if title => TITLE_WIDTH,
            None => DEFAULT_WIDTH,
        };
        Self {
            property_id,
            width,
            hidden,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
#[schema(as = DatabaseViewSort)]
pub struct ViewSort {
    #[schema(value_type = String)]
    pub property_id: Id,
    pub direction: SortDirection,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
#[schema(as = DatabaseFilterRule)]
pub struct FilterRule {
    #[schema(value_type = String)]
    pub property_id: Id,
    pub operator: FilterOperator,
    /// Depends on the property type and operator (spec 4.3); `null` matches every row.
    #[serde(default)]
    pub value: Value,
}

/// An AND/OR group of rules and groups (spec P2 5.2), as task filters. The root group is level 1;
/// groups nest 3 levels, hold at most 10 children, and a filter has at most 20 rules. An empty
/// group matches every row.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
#[schema(as = DatabaseFilterGroup)]
pub struct FilterGroup {
    pub op: GroupOp,
    pub children: Vec<FilterNode>,
}

/// A nested group (it has `op` and `children`) or a rule.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(untagged)]
#[schema(as = DatabaseFilterNode)]
pub enum FilterNode {
    #[schema(no_recursion)]
    Group(FilterGroup),
    Rule(FilterRule),
}

impl FilterGroup {
    /// The root group of `rules`, all of which must match.
    #[must_use]
    pub fn all(rules: Vec<FilterRule>) -> Self {
        Self {
            op: GroupOp::And,
            children: rules.into_iter().map(FilterNode::Rule).collect(),
        }
    }

    /// Keeps the rules `keep` accepts; a group left empty goes (a group with one child stays).
    /// Returns how many rules went.
    fn retain_rules(&mut self, keep: &mut impl FnMut(&FilterRule) -> bool) -> usize {
        let mut dropped = 0;
        self.children.retain_mut(|child| match child {
            FilterNode::Rule(rule) => {
                let kept = keep(rule);
                dropped += usize::from(!kept);
                kept
            }
            FilterNode::Group(group) => {
                dropped += group.retain_rules(keep);
                !group.children.is_empty()
            }
        });
        dropped
    }

    fn rules_mut(&mut self, change: &mut impl FnMut(&mut FilterRule)) {
        for child in &mut self.children {
            match child {
                FilterNode::Rule(rule) => change(rule),
                FilterNode::Group(group) => group.rules_mut(change),
            }
        }
    }

    /// Checks the limits and each rule against its property's type. `path` names this group,
    /// `level` is its depth (the root is 1) and `rules` counts the rules seen so far.
    fn check(
        &self,
        path: &str,
        level: usize,
        types: &HashMap<Id, PropertyType>,
        rules: &mut usize,
    ) -> Result<(), String> {
        if level > MAX_FILTER_LEVELS {
            return Err(path.to_owned());
        }
        if self.children.len() > MAX_GROUP_CHILDREN {
            return Err(format!("{path}.children"));
        }
        for (index, child) in self.children.iter().enumerate() {
            let path = format!("{path}.children[{index}]");
            match child {
                FilterNode::Group(group) => group.check(&path, level + 1, types, rules)?,
                FilterNode::Rule(rule) => {
                    *rules += 1;
                    if *rules > MAX_RULES {
                        return Err("config.filter".to_owned());
                    }
                    let Some(property_type) = types.get(&rule.property_id) else {
                        continue;
                    };
                    let Some(kind) = rule_value(*property_type, rule.operator) else {
                        return Err(format!("{path}.operator"));
                    };
                    if !rule_value_fits(kind, &rule.value) {
                        return Err(format!("{path}.value"));
                    }
                }
            }
        }
        Ok(())
    }
}

/// A view's saved layout. Rules, sorts, columns and cards that name a deleted property are dropped
/// when the view is read and saved. The `group_*`, `hidden_groups` and `card_properties` fields
/// belong to board views.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
#[schema(as = DatabaseViewConfig)]
pub struct ViewConfig {
    /// The order of the properties; properties missing here show at the end.
    #[serde(default)]
    pub columns: Vec<ViewColumn>,
    /// At most 10.
    #[serde(default)]
    pub sorts: Vec<ViewSort>,
    #[serde(default)]
    pub filter: FilterGroup,
    #[serde(default)]
    pub wrap_cells: bool,
    /// Form: the questions and texts (spec P3 18). Only form views have one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub form: Option<FormConfig>,
    /// Board: the `select`, `status`, `multi_select` or `person` property whose groups are the
    /// columns (a card with several values shows in each of their columns). A board without one
    /// falls back to the first status, else the first select property.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<String>)]
    pub group_by: Option<Id>,
    /// Board: group ids in column order (option ids, or user ids for a person), `null` for the
    /// "No <property>" column. Groups missing here follow in the property's order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub group_order: Vec<Option<String>>,
    /// Board: group ids (or `null`) of the columns listed under "Hidden groups".
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hidden_groups: Vec<Option<String>>,
    /// Board: the property whose groups are the lanes (rows of the same columns): a `select`,
    /// `status`, `multi_select`, `person` or `checkbox` property other than `group_by`. A
    /// checkbox's groups are `"true"` and `"false"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<String>)]
    pub sub_group_by: Option<Id>,
    /// Board: group ids in lane order, as `group_order`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sub_group_order: Vec<Option<String>>,
    /// Board: group ids (or `null`) of the hidden lanes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hidden_sub_groups: Vec<Option<String>>,
    /// Board: hides the columns and lanes that have no cards.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hide_empty_groups: bool,
    /// Board: the properties shown on each card, in order. The title always shows.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schema(value_type = Vec<String>)]
    pub card_properties: Vec<Id>,
    /// Table: the footer calculation of each property that has one. A function that does not fit
    /// the property's type (after a type change) is dropped.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    #[schema(value_type = HashMap<String, Calculation>)]
    pub calculations: BTreeMap<Id, Calculation>,
    /// The template that "New" and "+" use in this view (`null`: an empty row). A deleted
    /// template reads as `null`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<String>)]
    pub default_template_id: Option<Id>,
    /// Calendar: the `date`, `created_time` or `last_edited_time` property that places the rows.
    /// A calendar without one takes the first `date` property.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<String>)]
    pub date_property_id: Option<Id>,
    /// Calendar: a month grid (default) or one week.
    #[serde(default, skip_serializing_if = "CalendarMode::is_month")]
    pub calendar_mode: CalendarMode,
    /// Gallery: what a card shows above its title (default: the body's first image or text).
    #[serde(default, skip_serializing_if = "CardPreview::is_content")]
    pub card_preview: CardPreview,
    /// Gallery: the card width.
    #[serde(default, skip_serializing_if = "CardSize::is_medium")]
    pub card_size: CardSize,
    /// Gallery: the preview image fits its box instead of filling it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub fit_image: bool,
    /// Timeline: the `date` property where a bar starts. A timeline without one takes the first
    /// `date` property.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<String>)]
    pub start_property_id: Option<Id>,
    /// Timeline: the `date` property where a bar ends. Without one, a date range's end is the
    /// end and a single date is a one-day bar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<String>)]
    pub end_property_id: Option<Id>,
    /// Timeline: the time scale.
    #[serde(default, skip_serializing_if = "TimelineZoom::is_month")]
    pub zoom: TimelineZoom,
    /// Timeline: the table pane left of the bars shows (absent: `true`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show_table: Option<bool>,
    /// Timeline: the properties of the table pane, after the title.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schema(value_type = Vec<String>)]
    pub table_columns: Vec<Id>,
}

impl ViewConfig {
    /// Every property as a visible column, in property order.
    fn for_properties(properties: &[Property]) -> Self {
        Self {
            columns: properties.iter().map(Property::column).collect(),
            ..Self::default()
        }
    }

    /// Drops what names a property not in `properties`, repeats, and the board groups that do not
    /// fit their property (`board::repair`).
    fn repair(&mut self, view_type: ViewType, properties: &[Property]) {
        self.retain_known(properties);
        let kind = |id: Id| {
            properties
                .iter()
                .find(|property| property.id == id)
                .map(|property| &property.kind)
        };
        board::repair(self, view_type, properties);
        forms::repair(self, view_type, properties);
        calendar::repair(self, view_type, properties);
        gallery::repair(self, properties);
        timeline::repair(self, view_type, properties);
        let mut cards = HashSet::new();
        self.card_properties.retain(|id| {
            kind(*id).is_some_and(|kind| *kind != PropertyKind::Title) && cards.insert(*id)
        });
        self.calculations.retain(|id, calculation| {
            kind(*id).is_some_and(|kind| calculation.fits(kind.filter_type()))
        });
    }

    /// Drops what names a property not in `properties` and repeated columns and sorts.
    fn retain_known(&mut self, properties: &[Property]) {
        let known: HashSet<Id> = properties.iter().map(|property| property.id).collect();
        let mut columns = HashSet::new();
        self.columns.retain(|column| {
            known.contains(&column.property_id) && columns.insert(column.property_id)
        });
        let mut sorts = HashSet::new();
        self.sorts
            .retain(|sort| known.contains(&sort.property_id) && sorts.insert(sort.property_id));
        self.filter
            .retain_rules(&mut |rule| known.contains(&rule.property_id));
    }

    /// Checks limits, widths and each rule against its property's type, then drops what names
    /// unknown properties. Errors name the field path.
    pub fn validate(&mut self, view_type: ViewType, properties: &[Property]) -> Result<(), String> {
        if self.sorts.len() > MAX_SORTS {
            return Err("config.sorts".to_owned());
        }
        if self.columns.len() > MAX_PROPERTIES * 2 {
            return Err("config.columns".to_owned());
        }
        for (index, column) in self.columns.iter().enumerate() {
            if !(MIN_WIDTH..=MAX_WIDTH).contains(&column.width) {
                return Err(format!("config.columns[{index}].width"));
            }
        }
        if self.card_properties.len() > MAX_PROPERTIES {
            return Err("config.card_properties".to_owned());
        }
        board::check(self, properties)?;
        forms::check(self)?;
        calendar::check(self, properties)?;
        gallery::check(self, properties)?;
        timeline::check(self, properties)?;
        let types: HashMap<Id, PropertyType> = properties
            .iter()
            .map(|property| (property.id, property.kind.filter_type()))
            .collect();
        if self.calculations.len() > MAX_PROPERTIES {
            return Err("config.calculations".to_owned());
        }
        for (id, calculation) in &self.calculations {
            if types
                .get(id)
                .is_some_and(|property_type| !calculation.fits(*property_type))
            {
                return Err(format!("config.calculations.{id}"));
            }
        }
        self.filter.check("config.filter", 1, &types, &mut 0)?;
        self.repair(view_type, properties);
        Ok(())
    }

    /// Drops filter rules whose operator or value does not fit their property; returns how many.
    fn drop_unfit_rules(&mut self, properties: &[Property]) -> usize {
        let types: HashMap<Id, PropertyType> = properties
            .iter()
            .map(|property| (property.id, property.kind.filter_type()))
            .collect();
        self.filter.retain_rules(&mut |rule| {
            types.get(&rule.property_id).is_none_or(|property_type| {
                rule_value(*property_type, rule.operator)
                    .is_some_and(|kind| rule_value_fits(kind, &rule.value))
            })
        })
    }

    fn remap(&mut self, properties: &HashMap<Id, Id>) {
        forms::remap(self, properties);
        let map = |id: &mut Id| {
            if let Some(copy) = properties.get(id) {
                *id = *copy;
            }
        };
        self.columns
            .iter_mut()
            .for_each(|column| map(&mut column.property_id));
        self.sorts
            .iter_mut()
            .for_each(|sort| map(&mut sort.property_id));
        self.filter
            .rules_mut(&mut |rule| map(&mut rule.property_id));
        if let Some(group_by) = &mut self.group_by {
            map(group_by);
        }
        if let Some(date_property_id) = &mut self.date_property_id {
            map(date_property_id);
        }
        if let Some(sub_group_by) = &mut self.sub_group_by {
            map(sub_group_by);
        }
        if let CardPreview::Files(id) = &mut self.card_preview
            && let Ok(mut parsed) = id.parse::<Id>()
        {
            map(&mut parsed);
            *id = parsed.to_string();
        }
        for id in [&mut self.start_property_id, &mut self.end_property_id]
            .into_iter()
            .flatten()
        {
            map(id);
        }
        self.table_columns.iter_mut().for_each(map);
        self.card_properties.iter_mut().for_each(map);
        self.calculations = std::mem::take(&mut self.calculations)
            .into_iter()
            .map(|(mut id, calculation)| {
                map(&mut id);
                (id, calculation)
            })
            .collect();
    }
}

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

    /// The database's properties in order (the schema without views and rows).
    pub async fn properties(
        &self,
        workspace_id: Id,
        database_id: Id,
        actor_id: Id,
    ) -> Result<Vec<Property>, DatabaseError> {
        let mut tx = self.database.transaction().await?;
        open(&mut tx, workspace_id, database_id, actor_id).await?;
        let properties = properties_in_tx(&mut tx, database_id).await?;
        tx.rollback().await?;
        Ok(properties)
    }

    pub async fn create_property(
        &self,
        workspace_id: Id,
        database_id: Id,
        actor_id: Id,
        input: NewProperty,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<DatabaseProperty, DatabaseError> {
        let mut tx = self.database.immediate_transaction().await?;
        let page = open(&mut tx, workspace_id, database_id, actor_id).await?;
        unlocked(&page)?;
        let mut properties = properties_in_tx(&mut tx, database_id).await?;
        if properties.len() >= MAX_PROPERTIES {
            return Err(DatabaseError::PropertyLimit);
        }
        let (property, index, source) = match input {
            NewProperty::Fresh {
                name,
                property_type,
                config,
                position,
            } => {
                if property_type == PropertyType::Title {
                    return Err(DatabaseError::invalid("type"));
                }
                let kind = PropertyKind::parse(property_type, config.as_ref())
                    .map_err(DatabaseError::invalid)?;
                let kind = unique_id::fresh(&properties, kind)?;
                let property = Property {
                    id: Id::new_v7(),
                    name,
                    kind,
                    position: 0,
                };
                let mut property = property;
                let scope = ComputeScope::utc(workspace_id, actor_id);
                relations::check(&mut tx, &scope, database_id, &properties, &mut property).await?;
                (
                    formula::checked(&properties, property)?,
                    insert_index(position, properties.len()),
                    None,
                )
            }
            NewProperty::Duplicate { source, name } => {
                let at = properties
                    .iter()
                    .position(|property| property.id == source)
                    .ok_or_else(|| DatabaseError::invalid("duplicate_of"))?;
                let original = properties[at].clone();
                if matches!(original.kind, PropertyKind::Title | PropertyKind::UniqueId(_)) {
                    return Err(DatabaseError::invalid("duplicate_of"));
                }
                let (kind, option_ids) = original.kind.with_new_option_ids();
                // A copied relation is one-way and starts without links.
                let kind = relations::copied_kind(&kind, &HashMap::new());
                let property = Property {
                    id: Id::new_v7(),
                    name: name.unwrap_or_else(|| copy_name(&original.name)),
                    kind,
                    position: 0,
                };
                (
                    formula::checked(&properties, property)?,
                    at + 1,
                    Some((original, option_ids)),
                )
            }
        };
        insert_property(&mut tx, workspace_id, database_id, &property, now).await?;
        properties.insert(index, property.clone());
        renumber_rows(&mut tx, "database_properties", &properties_ids(&properties)).await?;
        if let Some((original, option_ids)) = &source {
            for (row, value) in stored_values(&mut tx, original).await? {
                let value = value.with_option_ids(option_ids);
                upsert_value(
                    &mut tx,
                    workspace_id,
                    &row,
                    property.id,
                    &value,
                    actor_id,
                    now,
                )
                .await?;
            }
        }
        unique_id::number_rows(&mut tx, workspace_id, database_id, &mut properties, actor_id, now)
            .await?;
        for mut view in raw_views(&mut tx, database_id, Views::All).await? {
            let columns = &mut view.config.columns;
            match source.as_ref().and_then(|(original, _)| {
                columns
                    .iter()
                    .position(|column| column.property_id == original.id)
            }) {
                Some(at) => {
                    let column = ViewColumn {
                        property_id: property.id,
                        ..columns[at].clone()
                    };
                    columns.insert(at + 1, column);
                }
                None => columns.push(property.column()),
            }
            write_view_config(&mut tx, view.id, &view.config, now).await?;
        }
        // A copy brings values, and a new status shows its default option on every row.
        refresh_search_text(
            &mut tx,
            workspace_id,
            database_id,
            &properties,
            SearchRows::All,
        )
        .await?;
        let scope = ComputeScope::utc(workspace_id, actor_id);
        properties[index].kind = relations::sync_pair(
            &mut tx,
            &scope,
            database_id,
            &page.title,
            &properties[index],
            None,
            request_id,
            now,
        )
        .await?;
        record(
            &mut tx,
            workspace_id,
            actor_id,
            "database.property_created",
            database_id,
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(DatabaseProperty {
            position: index as i64,
            ..properties[index].output()
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update_property(
        &self,
        workspace_id: Id,
        database_id: Id,
        property_id: Id,
        actor_id: Id,
        changes: PropertyChanges,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<PropertyUpdate, DatabaseError> {
        let mut tx = self.database.immediate_transaction().await?;
        let page = open(&mut tx, workspace_id, database_id, actor_id).await?;
        unlocked(&page)?;
        let mut properties = properties_in_tx(&mut tx, database_id).await?;
        let index = properties
            .iter()
            .position(|property| property.id == property_id)
            .ok_or(DatabaseError::PropertyNotFound)?;
        let current = properties[index].clone();
        let type_change = changes
            .property_type
            .filter(|property_type| *property_type != current.kind.property_type());
        let (mut converted, mut cleared) = (0, 0);
        let kind = match (type_change, &changes.config) {
            (Some(_), _) if current.kind == PropertyKind::Title => {
                return Err(DatabaseError::TitleProperty);
            }
            // Both need a config: create such a property instead.
            (Some(PropertyType::Relation | PropertyType::Rollup), _) => {
                return Err(DatabaseError::invalid("type"));
            }
            (Some(PropertyType::Title), _) => return Err(DatabaseError::invalid("type")),
            (Some(PropertyType::UniqueId), _)
                if properties
                    .iter()
                    .any(|property| matches!(property.kind, PropertyKind::UniqueId(_))) =>
            {
                return Err(DatabaseError::invalid("type"));
            }
            // A formula stores no values, so its expression can come with the type change.
            (Some(PropertyType::Formula), Some(config)) => {
                for (row, _) in stored_values(&mut tx, &current).await? {
                    cleared += 1;
                    delete_value(&mut tx, &row, property_id).await?;
                }
                PropertyKind::parse(PropertyType::Formula, Some(config))
                    .map_err(DatabaseError::invalid)?
            }
            // A type change derives the new config; set it in a second request.
            (Some(_), Some(_)) => return Err(DatabaseError::invalid("config")),
            (Some(new_type), None) => {
                relations::detach(&mut tx, &current, false).await?;
                let values = stored_values(&mut tx, &current).await?;
                let conversion = convert(&current.kind, new_type, values);
                for (row, value) in &conversion.values {
                    match value {
                        Some(value) => {
                            converted += 1;
                            update_value(&mut tx, row, property_id, value).await?;
                        }
                        None => {
                            cleared += 1;
                            delete_value(&mut tx, row, property_id).await?;
                        }
                    }
                }
                conversion.kind
            }
            (None, Some(config)) => {
                let kind = PropertyKind::parse(current.kind.property_type(), Some(config))
                    .map_err(DatabaseError::invalid)?;
                let kind = unique_id::keep_next(&current.kind, kind);
                let mut changed = Property {
                    kind,
                    ..current.clone()
                };
                let scope = ComputeScope::utc(workspace_id, actor_id);
                relations::check(&mut tx, &scope, database_id, &properties, &mut changed).await?;
                let kind = changed.kind;
                cleared = remove_missing_options(&mut tx, &current, &kind).await?;
                kind
            }
            (None, None) => current.kind.clone(),
        };
        let property = Property {
            name: changes.name.unwrap_or_else(|| current.name.clone()),
            kind,
            ..current
        };
        let others: Vec<Property> = properties
            .iter()
            .filter(|other| other.id != property_id)
            .cloned()
            .collect();
        let property = formula::checked(&others, property)?;
        sqlx::query(
            "UPDATE database_properties SET name = $1, type = $2, config_json = $3, updated_at = $4 WHERE id = $5",
        )
        .bind(&property.name)
        .bind(property.kind.property_type().as_str())
        .bind(property.kind.config_json().to_string())
        .bind(now.as_millis())
        .bind(property_id.to_string())
        .execute(&mut *tx)
        .await?;
        let property = match (&current.kind, type_change) {
            (PropertyKind::Relation(previous), None) if changes.config.is_some() => Property {
                kind: relations::sync_pair(
                    &mut tx,
                    &ComputeScope::utc(workspace_id, actor_id),
                    database_id,
                    &page.title,
                    &property,
                    Some(previous),
                    request_id,
                    now,
                )
                .await?,
                ..property
            },
            _ => property,
        };
        properties.remove(index);
        let index = insert_index(changes.position.or(Some(index as i64)), properties.len());
        properties.insert(index, property.clone());
        renumber_rows(&mut tx, "database_properties", &properties_ids(&properties)).await?;
        formula::refresh_types(&mut tx, &mut properties, now).await?;
        if type_change == Some(PropertyType::UniqueId) {
            unique_id::number_rows(&mut tx, workspace_id, database_id, &mut properties, actor_id, now)
                .await?;
        }
        let update = PropertyUpdate {
            property: DatabaseProperty {
                position: index as i64,
                ..properties[index].output()
            },
            converted,
            cleared,
        };
        if changes.dry_run {
            // Nothing is kept, so nothing is audited and no realtime event goes out.
            tx.rollback().await?;
            return Ok(update);
        }
        // A rename of the property itself changes no searchable text.
        if type_change.is_some() || changes.config.is_some() {
            refresh_search_text(
                &mut tx,
                workspace_id,
                database_id,
                &properties,
                SearchRows::All,
            )
            .await?;
        }
        record(
            &mut tx,
            workspace_id,
            actor_id,
            "database.property_updated",
            database_id,
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(update)
    }

    pub async fn delete_property(
        &self,
        workspace_id: Id,
        database_id: Id,
        property_id: Id,
        actor_id: Id,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), DatabaseError> {
        let mut tx = self.database.immediate_transaction().await?;
        let page = open(&mut tx, workspace_id, database_id, actor_id).await?;
        unlocked(&page)?;
        let mut properties = properties_in_tx(&mut tx, database_id).await?;
        let index = properties
            .iter()
            .position(|property| property.id == property_id)
            .ok_or(DatabaseError::PropertyNotFound)?;
        if properties[index].kind == PropertyKind::Title {
            return Err(DatabaseError::TitleProperty);
        }
        relations::detach(&mut tx, &properties[index], true).await?;
        // Its values go by cascade.
        sqlx::query("DELETE FROM database_properties WHERE id = $1")
            .bind(property_id.to_string())
            .execute(&mut *tx)
            .await?;
        properties.remove(index);
        formula::refresh_types(&mut tx, &mut properties, now).await?;
        renumber_rows(&mut tx, "database_properties", &properties_ids(&properties)).await?;
        refresh_search_text(
            &mut tx,
            workspace_id,
            database_id,
            &properties,
            SearchRows::All,
        )
        .await?;
        for mut view in raw_views(&mut tx, database_id, Views::All).await? {
            let before = view.config.clone();
            view.config.repair(view.view_type, &properties);
            if view.config != before {
                write_view_config(&mut tx, view.id, &view.config, now).await?;
            }
        }
        record(
            &mut tx,
            workspace_id,
            actor_id,
            "database.property_deleted",
            database_id,
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn create_view(
        &self,
        workspace_id: Id,
        database_id: Id,
        actor_id: Id,
        input: NewView,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<DatabaseView, DatabaseError> {
        let mut tx = self.database.immediate_transaction().await?;
        let set = writable_views(&mut tx, workspace_id, database_id, actor_id, input.link).await?;
        let properties = properties_in_tx(&mut tx, database_id).await?;
        let views = raw_views(&mut tx, database_id, set).await?;
        match set {
            Views::Link(_) if views.len() >= MAX_LINK_VIEWS => {
                return Err(DatabaseError::LinkViewLimit);
            }
            _ if views.len() >= MAX_VIEWS => return Err(DatabaseError::ViewLimit),
            _ => {}
        }
        let source = input
            .duplicate_of
            .map(|source| {
                views
                    .iter()
                    .find(|view| view.id == source)
                    .ok_or_else(|| DatabaseError::invalid("duplicate_of"))
            })
            .transpose()?;
        let view_type = source.map_or(input.view_type, |view| view.view_type);
        let mut config = match (input.config, source) {
            (Some(config), _) => config,
            (None, Some(source)) => source.config.clone(),
            (None, None) => ViewConfig::for_properties(&properties),
        };
        config
            .validate(view_type, &properties)
            .map_err(DatabaseError::invalid)?;
        board::drop_former_members(&mut tx, database_id, &properties, &mut [&mut config]).await?;
        templates::retain_default(&mut tx, database_id, &mut config).await?;
        let view = DatabaseView {
            id: Id::new_v7(),
            name: input.name,
            view_type,
            config,
            position: views.len() as i64,
        };
        insert_view(&mut tx, workspace_id, database_id, set.link(), &view, now).await?;
        record(
            &mut tx,
            workspace_id,
            actor_id,
            "database.view_created",
            database_id,
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(view)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update_view(
        &self,
        workspace_id: Id,
        database_id: Id,
        view_id: Id,
        actor_id: Id,
        changes: ViewChanges,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<DatabaseView, DatabaseError> {
        let mut tx = self.database.immediate_transaction().await?;
        let set =
            writable_views(&mut tx, workspace_id, database_id, actor_id, changes.link).await?;
        let properties = properties_in_tx(&mut tx, database_id).await?;
        let mut views = raw_views(&mut tx, database_id, set).await?;
        let index = views
            .iter()
            .position(|view| view.id == view_id)
            .ok_or(DatabaseError::ViewNotFound)?;
        let mut view = views.remove(index);
        if let Some(mut config) = changes.config {
            config
                .validate(view.view_type, &properties)
                .map_err(DatabaseError::invalid)?;
            board::drop_former_members(&mut tx, database_id, &properties, &mut [&mut config])
                .await?;
            templates::retain_default(&mut tx, database_id, &mut config).await?;
            view.config = config;
        }
        if let Some(name) = changes.name {
            view.name = name;
        }
        sqlx::query(
            "UPDATE database_views SET name = $1, config_json = $2, updated_at = $3 WHERE id = $4",
        )
        .bind(&view.name)
        .bind(serde_json::to_string(&view.config).map_err(|_| DatabaseError::Corrupt)?)
        .bind(now.as_millis())
        .bind(view_id.to_string())
        .execute(&mut *tx)
        .await?;
        let index = insert_index(changes.position.or(Some(index as i64)), views.len());
        view.position = index as i64;
        let mut ids: Vec<Id> = views.iter().map(|view| view.id).collect();
        ids.insert(index, view.id);
        renumber_rows(&mut tx, "database_views", &ids).await?;
        record(
            &mut tx,
            workspace_id,
            actor_id,
            "database.view_updated",
            database_id,
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        view.config.repair(view.view_type, &properties);
        Ok(view)
    }

    /// Deletes an own view (`link` `None`) or a view of a linked view block.
    #[allow(clippy::too_many_arguments)]
    pub async fn delete_view(
        &self,
        workspace_id: Id,
        database_id: Id,
        view_id: Id,
        link: Option<Id>,
        actor_id: Id,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), DatabaseError> {
        let mut tx = self.database.immediate_transaction().await?;
        let set = writable_views(&mut tx, workspace_id, database_id, actor_id, link).await?;
        let mut views = raw_views(&mut tx, database_id, set).await?;
        let index = views
            .iter()
            .position(|view| view.id == view_id)
            .ok_or(DatabaseError::ViewNotFound)?;
        if views.len() == 1 {
            return Err(DatabaseError::LastView);
        }
        sqlx::query("DELETE FROM database_views WHERE id = $1")
            .bind(view_id.to_string())
            .execute(&mut *tx)
            .await?;
        views.remove(index);
        let ids: Vec<Id> = views.iter().map(|view| view.id).collect();
        renumber_rows(&mut tx, "database_views", &ids).await?;
        record(
            &mut tx,
            workspace_id,
            actor_id,
            "database.view_deleted",
            database_id,
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(())
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

/// Every stored value of a property (live and trashed rows), parsed under its kind.
async fn stored_values(
    tx: &mut Transaction<'_, Any>,
    property: &Property,
) -> Result<Vec<(String, CellValue)>, DatabaseError> {
    let rows = sqlx::query(
        "SELECT page_id, value_json FROM page_property_values WHERE property_id = $1 ORDER BY page_id",
    )
    .bind(property.id.to_string())
    .fetch_all(&mut **tx)
    .await?;
    rows.into_iter()
        .map(|row| {
            let raw: Value = serde_json::from_str(&row.get::<String, _>("value_json"))
                .map_err(|_| DatabaseError::Corrupt)?;
            match parse_value(&property.kind, &raw) {
                Ok(Some(value)) => Ok((row.get("page_id"), value)),
                _ => Err(DatabaseError::Corrupt),
            }
        })
        .collect()
}

/// Removes option ids that `next` no longer has from the values; returns how many values were
/// cleared.
async fn remove_missing_options(
    tx: &mut Transaction<'_, Any>,
    current: &Property,
    next: &PropertyKind,
) -> Result<u64, DatabaseError> {
    let removed = current
        .kind
        .options()
        .iter()
        .any(|(id, _)| !next.has_option(id));
    if !removed {
        return Ok(0);
    }
    let mut cleared = 0;
    for (row, value) in stored_values(tx, current).await? {
        let next_value = match &value {
            CellValue::Choice(id) => next.has_option(id).then(|| value.clone()),
            CellValue::Choices(ids) => {
                let kept: Vec<String> = ids
                    .iter()
                    .filter(|id| next.has_option(id))
                    .cloned()
                    .collect();
                (!kept.is_empty()).then_some(CellValue::Choices(kept))
            }
            _ => Some(value.clone()),
        };
        match next_value {
            Some(next_value) if next_value == value => {}
            Some(next_value) => update_value(tx, &row, current.id, &next_value).await?,
            None => {
                cleared += 1;
                delete_value(tx, &row, current.id).await?;
            }
        }
    }
    Ok(cleared)
}

fn properties_ids(properties: &[Property]) -> Vec<Id> {
    properties.iter().map(|property| property.id).collect()
}

/// Sets `position` to the index for each id of a database's properties or views.
async fn renumber_rows(
    tx: &mut Transaction<'_, Any>,
    table: &'static str,
    ids: &[Id],
) -> Result<(), sqlx::Error> {
    for (position, id) in ids.iter().enumerate() {
        let position = position as i64;
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE {table} SET position = $1 WHERE id = $2 AND position <> $3"
        )))
        .bind(position)
        .bind(id.to_string())
        .bind(position)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

async fn properties_in_tx(
    tx: &mut Transaction<'_, Any>,
    database_id: Id,
) -> Result<Vec<Property>, DatabaseError> {
    let rows = sqlx::query(
        "SELECT id, name, type, config_json, position FROM database_properties \
         WHERE database_id = $1 ORDER BY position, id",
    )
    .bind(database_id.to_string())
    .fetch_all(&mut **tx)
    .await?;
    rows.into_iter()
        .map(|row| {
            Property::from_stored(
                &row.get::<String, _>("id"),
                row.get("name"),
                &row.get::<String, _>("type"),
                &row.get::<String, _>("config_json"),
                row.get("position"),
            )
        })
        .collect()
}

impl Property {
    /// A property from its `database_properties` columns.
    pub(super) fn from_stored(
        id: &str,
        name: String,
        property_type: &str,
        config_json: &str,
        position: i64,
    ) -> Result<Self, DatabaseError> {
        let property_type = PropertyType::parse(property_type).ok_or(DatabaseError::Corrupt)?;
        let config: Value =
            serde_json::from_str(config_json).map_err(|_| DatabaseError::Corrupt)?;
        Ok(Self {
            id: parse_id(id)?,
            name,
            kind: PropertyKind::parse(property_type, Some(&config))
                .map_err(|_| DatabaseError::Corrupt)?,
            position,
        })
    }
}

impl DatabaseView {
    /// A view from its `database_views` columns, config as saved.
    pub(super) fn from_stored(
        id: &str,
        name: String,
        view_type: &str,
        config_json: &str,
        position: i64,
    ) -> Result<Self, DatabaseError> {
        Ok(Self {
            id: parse_id(id)?,
            name,
            view_type: ViewType::parse(view_type).ok_or(DatabaseError::Corrupt)?,
            config: serde_json::from_str(config_json).map_err(|_| DatabaseError::Corrupt)?,
            position,
        })
    }
}

/// The stored views of `set`, configs as saved.
async fn raw_views(
    tx: &mut Transaction<'_, Any>,
    database_id: Id,
    set: Views,
) -> Result<Vec<DatabaseView>, DatabaseError> {
    let query = match set {
        Views::Own => sqlx::query(
            "SELECT id, name, type, config_json, position FROM database_views \
             WHERE database_id = $1 AND link_id IS NULL ORDER BY position, id",
        ),
        Views::Link(_) => sqlx::query(
            "SELECT id, name, type, config_json, position FROM database_views \
             WHERE database_id = $1 AND link_id = $2 ORDER BY position, id",
        ),
        Views::All => sqlx::query(
            "SELECT id, name, type, config_json, position FROM database_views \
             WHERE database_id = $1 ORDER BY position, id",
        ),
    };
    let query = query.bind(database_id.to_string());
    let query = match set {
        Views::Link(link_id) => query.bind(link_id.to_string()),
        Views::Own | Views::All => query,
    };
    let rows = query.fetch_all(&mut **tx).await?;
    rows.into_iter()
        .map(|row| {
            DatabaseView::from_stored(
                &row.get::<String, _>("id"),
                row.get("name"),
                &row.get::<String, _>("type"),
                &row.get::<String, _>("config_json"),
                row.get("position"),
            )
        })
        .collect()
}

/// The views as clients see them: repaired against the current properties.
async fn views_in_tx(
    tx: &mut Transaction<'_, Any>,
    database_id: Id,
    set: Views,
    properties: &[Property],
) -> Result<Vec<DatabaseView>, DatabaseError> {
    let mut views = raw_views(tx, database_id, set).await?;
    for view in &mut views {
        view.config.repair(view.view_type, properties);
    }
    let mut configs: Vec<_> = views.iter_mut().map(|view| &mut view.config).collect();
    board::drop_former_members(tx, database_id, properties, &mut configs).await?;
    Ok(views)
}

async fn write_view_config(
    tx: &mut Transaction<'_, Any>,
    view_id: Id,
    config: &ViewConfig,
    now: TimestampMillis,
) -> Result<(), DatabaseError> {
    sqlx::query("UPDATE database_views SET config_json = $1, updated_at = $2 WHERE id = $3")
        .bind(serde_json::to_string(config).map_err(|_| DatabaseError::Corrupt)?)
        .bind(now.as_millis())
        .bind(view_id.to_string())
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

async fn insert_property(
    tx: &mut Transaction<'_, Any>,
    workspace_id: Id,
    database_id: Id,
    property: &Property,
    now: TimestampMillis,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO database_properties (id, workspace_id, database_id, name, type, config_json, position, created_at, updated_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
    )
    .bind(property.id.to_string())
    .bind(workspace_id.to_string())
    .bind(database_id.to_string())
    .bind(&property.name)
    .bind(property.kind.property_type().as_str())
    .bind(property.kind.config_json().to_string())
    .bind(property.position)
    .bind(now.as_millis())
    .bind(now.as_millis())
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn insert_view(
    tx: &mut Transaction<'_, Any>,
    workspace_id: Id,
    database_id: Id,
    link: Option<Id>,
    view: &DatabaseView,
    now: TimestampMillis,
) -> Result<(), DatabaseError> {
    sqlx::query(
        "INSERT INTO database_views (id, workspace_id, database_id, link_id, name, type, config_json, position, created_at, updated_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
    )
    .bind(view.id.to_string())
    .bind(workspace_id.to_string())
    .bind(database_id.to_string())
    .bind(link.map(|link_id| link_id.to_string()))
    .bind(&view.name)
    .bind(view.view_type.as_str())
    .bind(serde_json::to_string(&view.config).map_err(|_| DatabaseError::Corrupt)?)
    .bind(view.position)
    .bind(now.as_millis())
    .bind(now.as_millis())
    .execute(&mut **tx)
    .await?;
    Ok(())
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
