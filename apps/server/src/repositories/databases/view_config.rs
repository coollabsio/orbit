//! View configs: view types, columns, sorts, filters and calculations, checked against the
//! properties and repaired when they change.

use std::collections::{BTreeMap, HashMap, HashSet};

use orbit_platform::Id;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;

use super::{
    CalendarMode, CardPreview, CardSize, DEFAULT_WIDTH, DatePoint, FormConfig, GroupOp,
    MAX_CELL_ITEMS, MAX_FILTER_LEVELS, MAX_GROUP_CHILDREN, MAX_OPTION_ID_CHARS, MAX_PROPERTIES,
    MAX_RULES, MAX_SORTS, MAX_TEXT_CHARS, MAX_WIDTH, MIN_WIDTH, Property, PropertyKind,
    PropertyType, RELATIVE_DATES, TITLE_WIDTH, TimelineZoom,
};
use super::{board, calendar, forms, gallery, timeline};

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
    pub(super) const fn as_str(self) -> &'static str {
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

    pub(super) fn parse(value: &str) -> Option<Self> {
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
/// As `sortable` in `@orbit/database` `meta.ts`: a button has no value, and relations do not sort.
fn sortable(kind: &PropertyKind) -> bool {
    !matches!(kind, PropertyKind::Button(_) | PropertyKind::Relation(_))
}

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

    /// An old flat filter holds up to [`MAX_RULES`] children, more than a group may: the overflow
    /// moves into a nested group with the same operator, which matches the same rows.
    fn nest_overflow(&mut self) {
        if !(MAX_GROUP_CHILDREN + 1..=MAX_RULES).contains(&self.children.len()) {
            return;
        }
        let mut overflow = Self {
            op: self.op,
            children: self.children.split_off(MAX_GROUP_CHILDREN - 1),
        };
        overflow.nest_overflow();
        self.children.push(FilterNode::Group(overflow));
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
    pub(super) fn check(
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
    pub(super) fn for_properties(properties: &[Property]) -> Self {
        Self {
            columns: properties.iter().map(Property::column).collect(),
            ..Self::default()
        }
    }

    /// Drops what names a property not in `properties`, repeats, sorts by a type that does not
    /// sort, and the board groups that do not fit their property (`board::repair`).
    pub(super) fn repair(&mut self, view_type: ViewType, properties: &[Property]) {
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
        self.sorts
            .retain(|sort| kind(sort.property_id).is_some_and(sortable));
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
        self.filter.nest_overflow();
    }

    /// Checks limits, widths and each rule against its property's type, then drops what names
    /// unknown properties. Errors name the field path.
    pub fn validate(&mut self, view_type: ViewType, properties: &[Property]) -> Result<(), String> {
        if self.sorts.len() > MAX_SORTS {
            return Err("config.sorts".to_owned());
        }
        for (index, sort) in self.sorts.iter().enumerate() {
            let kind = properties
                .iter()
                .find(|property| property.id == sort.property_id)
                .map(|property| &property.kind);
            if kind.is_some_and(|kind| !sortable(kind)) {
                return Err(format!("config.sorts[{index}].property_id"));
            }
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
        self.filter.nest_overflow();
        self.filter.check("config.filter", 1, &types, &mut 0)?;
        self.repair(view_type, properties);
        Ok(())
    }

    /// Drops filter rules whose operator or value does not fit their property; returns how many.
    pub(super) fn drop_unfit_rules(&mut self, properties: &[Property]) -> usize {
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

    pub(super) fn remap(&mut self, properties: &HashMap<Id, Id>) {
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
