//! The task view contract (`ViewState`): filter trees, display options, validation that reports
//! the JSON path of the first bad node, and the built-in preset trees.

use std::collections::{BTreeSet, HashMap};
use std::fmt;

use orbit_platform::SqlBuilder;
use orbit_platform::{Backend, Id, TimestampMillis};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use utoipa::ToSchema;

use super::project_access::ProjectScope;
use super::sub_issues;
use super::tasks::TaskFieldKind;

/// Groups may nest this many levels below the root group (the root is level 0).
pub const MAX_NESTING: usize = 3;
/// Conditions across the whole tree.
pub const MAX_CONDITIONS: usize = 50;
/// Values in one condition.
pub const MAX_VALUES: usize = 100;
/// Characters in a `text contains` value.
pub const MAX_TEXT_CHARS: usize = 200;
/// Largest `offset_days` of a relative date, in either direction.
pub const MAX_OFFSET_DAYS: i64 = 3_650;

const MAX_STATUS_NAME_CHARS: usize = 200;
const MAX_LABEL_NAME_CHARS: usize = 100;
const DAY_MS: i64 = 86_400_000;
const STATUS_CATEGORIES: [&str; 5] = [
    "unstarted",
    "started",
    "completed",
    "cancelled",
    "duplicate",
];
const DONE_CATEGORIES: [&str; 3] = ["completed", "cancelled", "duplicate"];
const RELATIVE_CYCLES: [&str; 3] = ["current", "next", "previous"];
const PRIORITIES: [&str; 5] = ["none", "low", "medium", "high", "urgent"];
/// Custom fields a view shows on rows and cards.
pub const MAX_DISPLAY_FIELDS: usize = 50;
const DISPLAY_KEYS: [&str; 11] = [
    "layout",
    "group_by",
    "sub_group_by",
    "order_by",
    "order_direction",
    "properties",
    "show_completed",
    "show_empty_groups",
    "sub_issues",
    "custom_fields",
    "order_field_id",
];

/// Filter and display options shared by saved views and per-page preferences.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ViewState {
    pub filter: FilterGroup,
    pub display: DisplayOptions,
}

/// An AND/OR group. An empty group matches every task.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct FilterGroup {
    pub op: GroupOp,
    pub children: Vec<FilterNode>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum GroupOp {
    #[default]
    And,
    Or,
}

/// A nested group or a condition; groups carry `op` and `children`.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, ToSchema)]
#[serde(untagged)]
pub enum FilterNode {
    #[schema(no_recursion)]
    Group(FilterGroup),
    Condition(Condition),
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Condition {
    pub field: FilterField,
    /// The task field (`/task-fields`) of a `custom_field` condition; omitted for other fields.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<String>)]
    pub field_id: Option<Id>,
    pub operator: FilterOperator,
    /// An array of strings, a date value, a pair of date values, or a string, depending on
    /// `field` and `operator`. Omitted for `is_empty` and `is_not_empty`.
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub value: Value,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum FilterField {
    Status,
    StatusCategory,
    Assignee,
    Creator,
    Label,
    Priority,
    Project,
    DueDate,
    CreatedAt,
    UpdatedAt,
    Text,
    Parent,
    SubIssues,
    /// Milestone ids.
    Milestone,
    /// Cycle ids, or `current`, `next` and `previous`: the cycle of the task's project that runs
    /// now, the first that starts after now, and the last that ended before now.
    Cycle,
    Estimate,
    /// A task field named by `field_id`. Operators and values by the field's kind: text and url
    /// `contains` (string); number `is`, `less_than`, `greater_than` (number) and `between`
    /// (`[min, max]`, inclusive); date `before`, `after`, `between` (date values); select `is` and
    /// `is_not` (option ids); multi_select `includes_any`, `includes_all`, `excludes` (option
    /// ids); checkbox `is` (boolean); every kind but checkbox `is_empty` and `is_not_empty`. The
    /// stored value counts, also when the task's project does not show the field. A deleted
    /// field has no values.
    CustomField,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum FilterOperator {
    Is,
    IsNot,
    IsEmpty,
    IsNotEmpty,
    IncludesAny,
    IncludesAll,
    Excludes,
    Before,
    After,
    Between,
    Contains,
    /// Custom number fields only.
    LessThan,
    /// Custom number fields only.
    GreaterThan,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct DisplayOptions {
    pub layout: Layout,
    pub group_by: GroupBy,
    pub sub_group_by: GroupBy,
    pub order_by: OrderBy,
    pub order_direction: OrderDirection,
    pub properties: Vec<TaskProperty>,
    pub show_completed: ShowCompleted,
    pub show_empty_groups: bool,
    /// Optional in stored and received states (default `nested`).
    #[serde(default)]
    pub sub_issues: SubIssuesDisplay,
    /// Task fields shown on rows and cards after `properties`, in order (optional, default none).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schema(value_type = Vec<String>)]
    pub custom_fields: Vec<Id>,
    /// The task field of `order_by: custom_field`; omitted for every other ordering.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<String>)]
    pub order_field_id: Option<Id>,
}

impl Default for DisplayOptions {
    fn default() -> Self {
        Self {
            layout: Layout::List,
            group_by: GroupBy::Status,
            sub_group_by: GroupBy::None,
            order_by: OrderBy::Manual,
            order_direction: OrderDirection::Asc,
            properties: vec![
                TaskProperty::Id,
                TaskProperty::Status,
                TaskProperty::Assignee,
                TaskProperty::Priority,
                TaskProperty::Project,
                TaskProperty::DueDate,
                TaskProperty::Labels,
                TaskProperty::SubIssueProgress,
            ],
            show_completed: ShowCompleted::All,
            show_empty_groups: false,
            sub_issues: SubIssuesDisplay::Nested,
            custom_fields: Vec::new(),
            order_field_id: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Layout {
    List,
    Board,
    Timeline,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum GroupBy {
    Status,
    Assignee,
    Priority,
    Project,
    Label,
    Milestone,
    Cycle,
    None,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum OrderBy {
    Manual,
    Priority,
    Created,
    Updated,
    Title,
    DueDate,
    /// Unestimated tasks last in both directions.
    Estimate,
    /// The value of the task field `order_field_id`; tasks without a value last in both
    /// directions. Select options sort in the field's option order; checkboxes unchecked first.
    CustomField,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum OrderDirection {
    Asc,
    Desc,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum TaskProperty {
    Id,
    Status,
    Assignee,
    Priority,
    Project,
    DueDate,
    Labels,
    Created,
    Updated,
    SubIssueProgress,
    Milestone,
    Cycle,
    Estimate,
}

/// Which tasks in a done category (completed, cancelled, duplicate) stay visible.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ShowCompleted {
    All,
    PastWeek,
    PastMonth,
    None,
}

/// How a view shows sub-issues. `nested` and `flat` only change the client; `hidden` lists
/// top-level tasks only (`POST /tasks/query` `sub_issues`).
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SubIssuesDisplay {
    #[default]
    Nested,
    Flat,
    Hidden,
}

/// The first invalid node: `path` is a JSON path such as `filter.children[2].value`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FilterError {
    pub path: String,
    pub message: &'static str,
}

impl fmt::Display for FilterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.path, self.message)
    }
}

impl std::error::Error for FilterError {}

fn invalid(path: impl Into<String>, message: &'static str) -> FilterError {
    FilterError {
        path: path.into(),
        message,
    }
}

/// A parsed date operand. Days count from 1970-01-01 in UTC.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DateValue {
    Absolute {
        day: i64,
    },
    /// An exact moment, in epoch milliseconds. The public API sends it for a time (not a day).
    Instant {
        millis: i64,
    },
    Relative {
        anchor: RelativeDay,
        offset_days: i64,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelativeDay {
    Today,
    StartOfWeek,
    EndOfWeek,
}

/// Parses and validates a filter received as JSON. Paths start with `filter`.
pub fn parse_filter(value: &Value) -> Result<FilterGroup, FilterError> {
    let group = parse_group(value, "filter", 0)?;
    validate_filter(&group)?;
    Ok(group)
}

/// Parses and validates stored or received view state JSON.
pub fn parse_view_state(json: &str) -> Result<ViewState, FilterError> {
    let value: Value = serde_json::from_str(json).map_err(|_| invalid("", "is not valid JSON"))?;
    parse_view_state_value(&value)
}

/// Like [`parse_view_state`], for an already parsed JSON value.
pub fn parse_view_state_value(value: &Value) -> Result<ViewState, FilterError> {
    let Some(object) = value.as_object() else {
        return Err(invalid("", "must be an object with filter and display"));
    };
    if let Some(key) = object
        .keys()
        .find(|key| !matches!(key.as_str(), "filter" | "display"))
    {
        return Err(invalid(key.clone(), "is not a view state field"));
    }
    let filter = object
        .get("filter")
        .ok_or_else(|| invalid("filter", "is required"))?;
    let display = object
        .get("display")
        .ok_or_else(|| invalid("display", "is required"))?;
    let state = ViewState {
        filter: parse_group(filter, "filter", 0)?,
        display: parse_display(display)?,
    };
    validate_view_state(&state)?;
    Ok(state)
}

fn parse_group(value: &Value, path: &str, nesting: usize) -> Result<FilterGroup, FilterError> {
    if nesting > MAX_NESTING {
        return Err(invalid(path, "filter groups are nested too deeply"));
    }
    let Some(object) = value.as_object() else {
        return Err(invalid(path, "must be a filter group"));
    };
    if let Some(key) = object
        .keys()
        .find(|key| !matches!(key.as_str(), "op" | "children"))
    {
        return Err(invalid(
            format!("{path}.{key}"),
            "is not a filter group field",
        ));
    }
    let op = required::<GroupOp>(object, path, "op", "must be and or or")?;
    let Some(children) = object.get("children").and_then(Value::as_array) else {
        return Err(invalid(format!("{path}.children"), "must be an array"));
    };
    let mut nodes = Vec::with_capacity(children.len().min(MAX_CONDITIONS + 1));
    for (index, child) in children.iter().enumerate() {
        let child_path = format!("{path}.children[{index}]");
        let is_group = child
            .as_object()
            .is_some_and(|node| node.contains_key("op") || node.contains_key("children"));
        nodes.push(if is_group {
            FilterNode::Group(parse_group(child, &child_path, nesting + 1)?)
        } else {
            FilterNode::Condition(parse_condition(child, &child_path)?)
        });
    }
    Ok(FilterGroup {
        op,
        children: nodes,
    })
}

fn parse_condition(value: &Value, path: &str) -> Result<Condition, FilterError> {
    let Some(object) = value.as_object() else {
        return Err(invalid(path, "must be a condition or a filter group"));
    };
    if let Some(key) = object
        .keys()
        .find(|key| !matches!(key.as_str(), "field" | "field_id" | "operator" | "value"))
    {
        return Err(invalid(format!("{path}.{key}"), "is not a condition field"));
    }
    let field_id = match object.get("field_id") {
        None | Some(Value::Null) => None,
        Some(id) => Some(
            id.as_str()
                .and_then(|id| id.parse::<Id>().ok())
                .ok_or_else(|| invalid(format!("{path}.field_id"), "must be a task field id"))?,
        ),
    };
    Ok(Condition {
        field: required(object, path, "field", "is not a known filter field")?,
        field_id,
        operator: required(object, path, "operator", "is not a known filter operator")?,
        value: object.get("value").cloned().unwrap_or(Value::Null),
    })
}

fn parse_display(value: &Value) -> Result<DisplayOptions, FilterError> {
    let Some(object) = value.as_object() else {
        return Err(invalid("display", "must be an object"));
    };
    if let Some(key) = object
        .keys()
        .find(|key| !DISPLAY_KEYS.contains(&key.as_str()))
    {
        return Err(invalid(format!("display.{key}"), "is not a display option"));
    }
    Ok(DisplayOptions {
        layout: required(object, "display", "layout", "is not a supported layout")?,
        group_by: required(object, "display", "group_by", "is not a supported grouping")?,
        sub_group_by: required(
            object,
            "display",
            "sub_group_by",
            "is not a supported grouping",
        )?,
        order_by: required(object, "display", "order_by", "is not a supported ordering")?,
        order_direction: required(object, "display", "order_direction", "must be asc or desc")?,
        properties: required(
            object,
            "display",
            "properties",
            "must be a list of task properties",
        )?,
        show_completed: required(
            object,
            "display",
            "show_completed",
            "must be all, past_week, past_month or none",
        )?,
        show_empty_groups: required(object, "display", "show_empty_groups", "must be a boolean")?,
        sub_issues: optional(
            object,
            "display",
            "sub_issues",
            "must be nested, flat or hidden",
        )?,
        custom_fields: optional(
            object,
            "display",
            "custom_fields",
            "must be a list of task field ids",
        )?,
        order_field_id: optional(
            object,
            "display",
            "order_field_id",
            "must be a task field id",
        )?,
    })
}

fn required<T: DeserializeOwned>(
    object: &Map<String, Value>,
    path: &str,
    key: &str,
    message: &'static str,
) -> Result<T, FilterError> {
    let key_path = format!("{path}.{key}");
    let value = object
        .get(key)
        .ok_or_else(|| invalid(key_path.clone(), "is required"))?;
    T::deserialize(value).map_err(|_| invalid(key_path, message))
}

/// Like [`required`], but a missing key takes the type's default (keys added after v1).
fn optional<T: DeserializeOwned + Default>(
    object: &Map<String, Value>,
    path: &str,
    key: &str,
    message: &'static str,
) -> Result<T, FilterError> {
    match object.get(key) {
        None => Ok(T::default()),
        Some(value) => T::deserialize(value).map_err(|_| invalid(format!("{path}.{key}"), message)),
    }
}

/// Checks limits and field/operator/value compatibility. Paths start with `filter`.
pub fn validate_filter(group: &FilterGroup) -> Result<(), FilterError> {
    let mut conditions = 0;
    validate_group(group, "filter", 0, &mut conditions)
}

/// Validates the filter, then the display rules. Paths start with `filter` or `display`.
pub fn validate_view_state(state: &ViewState) -> Result<(), FilterError> {
    validate_filter(&state.filter)?;
    validate_display(&state.display)
}

fn validate_group(
    group: &FilterGroup,
    path: &str,
    nesting: usize,
    conditions: &mut usize,
) -> Result<(), FilterError> {
    if nesting > MAX_NESTING {
        return Err(invalid(path, "filter groups are nested too deeply"));
    }
    for (index, child) in group.children.iter().enumerate() {
        let child_path = format!("{path}.children[{index}]");
        match child {
            FilterNode::Group(inner) => {
                validate_group(inner, &child_path, nesting + 1, conditions)?
            }
            FilterNode::Condition(condition) => {
                *conditions += 1;
                if *conditions > MAX_CONDITIONS {
                    return Err(invalid(
                        child_path,
                        "a filter can have at most 50 conditions",
                    ));
                }
                validate_condition(condition, &child_path)?;
            }
        }
    }
    Ok(())
}

fn allowed_operators(field: FilterField) -> &'static [FilterOperator] {
    use FilterOperator as O;
    match field {
        FilterField::Status
        | FilterField::StatusCategory
        | FilterField::Creator
        | FilterField::Priority
        | FilterField::Project
        | FilterField::Parent => &[O::Is, O::IsNot],
        FilterField::Assignee | FilterField::Milestone | FilterField::Cycle => {
            &[O::Is, O::IsNot, O::IsEmpty, O::IsNotEmpty]
        }
        FilterField::Estimate => &[O::IsEmpty, O::IsNotEmpty],
        FilterField::Label => &[
            O::IncludesAny,
            O::IncludesAll,
            O::Excludes,
            O::IsEmpty,
            O::IsNotEmpty,
        ],
        FilterField::DueDate => &[O::Before, O::After, O::Between, O::IsEmpty, O::IsNotEmpty],
        FilterField::CreatedAt | FilterField::UpdatedAt => &[O::Before, O::After],
        FilterField::Text => &[O::Contains],
        FilterField::SubIssues => &[O::Is],
        FilterField::CustomField => &[
            O::Is,
            O::IsNot,
            O::IsEmpty,
            O::IsNotEmpty,
            O::IncludesAny,
            O::IncludesAll,
            O::Excludes,
            O::Before,
            O::After,
            O::Between,
            O::Contains,
            O::LessThan,
            O::GreaterThan,
        ],
    }
}

fn validate_condition(condition: &Condition, path: &str) -> Result<(), FilterError> {
    if condition.field == FilterField::CustomField {
        if condition.field_id.is_none() {
            return Err(invalid(format!("{path}.field_id"), "is required"));
        }
        return validate_custom_any_kind(condition, path);
    }
    if condition.field_id.is_some() {
        return Err(invalid(
            format!("{path}.field_id"),
            "is only for custom_field conditions",
        ));
    }
    if !allowed_operators(condition.field).contains(&condition.operator) {
        return Err(invalid(
            format!("{path}.operator"),
            "is not supported for this field",
        ));
    }
    let value_path = format!("{path}.value");
    let value = &condition.value;
    match condition.operator {
        FilterOperator::IsEmpty | FilterOperator::IsNotEmpty => {
            if value.is_null() {
                Ok(())
            } else {
                Err(invalid(value_path, "must be omitted for this operator"))
            }
        }
        FilterOperator::Before | FilterOperator::After => {
            parse_date_value(value, &value_path).map(|_| ())
        }
        FilterOperator::Between => parse_date_range(value, &value_path).map(|_| ()),
        FilterOperator::Contains => validate_text(value, &value_path),
        FilterOperator::Is
        | FilterOperator::IsNot
        | FilterOperator::IncludesAny
        | FilterOperator::IncludesAll
        | FilterOperator::Excludes => validate_values(condition.field, value, &value_path),
        FilterOperator::LessThan | FilterOperator::GreaterThan => Err(invalid(
            format!("{path}.operator"),
            "is not supported for this field",
        )),
    }
}

const CUSTOM_KINDS: [TaskFieldKind; 7] = [
    TaskFieldKind::Text,
    TaskFieldKind::Number,
    TaskFieldKind::Date,
    TaskFieldKind::Select,
    TaskFieldKind::MultiSelect,
    TaskFieldKind::Checkbox,
    TaskFieldKind::Url,
];

/// The operators of a `custom_field` condition on a field of `kind`.
pub fn custom_operators(kind: TaskFieldKind) -> &'static [FilterOperator] {
    use FilterOperator as O;
    match kind {
        TaskFieldKind::Text | TaskFieldKind::Url => &[O::Contains, O::IsEmpty, O::IsNotEmpty],
        TaskFieldKind::Number => &[
            O::Is,
            O::LessThan,
            O::GreaterThan,
            O::Between,
            O::IsEmpty,
            O::IsNotEmpty,
        ],
        TaskFieldKind::Date => &[O::Before, O::After, O::Between, O::IsEmpty, O::IsNotEmpty],
        TaskFieldKind::Select => &[O::Is, O::IsNot, O::IsEmpty, O::IsNotEmpty],
        TaskFieldKind::MultiSelect => &[
            O::IncludesAny,
            O::IncludesAll,
            O::Excludes,
            O::IsEmpty,
            O::IsNotEmpty,
        ],
        TaskFieldKind::Checkbox => &[O::Is],
        // `custom_field_specs` leaves it out: it has no stored value.
        TaskFieldKind::TimeTracking => &[],
    }
}

/// Without the field's kind (a view is saved without reading its fields): the condition must
/// fit at least one kind. [`validate_custom_fields`] checks the real kind at query time.
fn validate_custom_any_kind(condition: &Condition, path: &str) -> Result<(), FilterError> {
    let mut first_error = None;
    for kind in CUSTOM_KINDS {
        if !custom_operators(kind).contains(&condition.operator) {
            continue;
        }
        match validate_custom_condition(kind, condition, path) {
            Ok(()) => return Ok(()),
            Err(error) => {
                first_error.get_or_insert(error);
            }
        }
    }
    Err(first_error.unwrap_or_else(|| {
        invalid(
            format!("{path}.operator"),
            "is not supported for this field",
        )
    }))
}

fn validate_custom_condition(
    kind: TaskFieldKind,
    condition: &Condition,
    path: &str,
) -> Result<(), FilterError> {
    use FilterOperator as O;
    if !custom_operators(kind).contains(&condition.operator) {
        return Err(invalid(
            format!("{path}.operator"),
            "is not supported for this field",
        ));
    }
    let value_path = format!("{path}.value");
    let value = &condition.value;
    match (kind, condition.operator) {
        (_, O::IsEmpty | O::IsNotEmpty) => {
            if value.is_null() {
                Ok(())
            } else {
                Err(invalid(value_path, "must be omitted for this operator"))
            }
        }
        (TaskFieldKind::Text | TaskFieldKind::Url, _) => validate_text(value, &value_path),
        (TaskFieldKind::Number, O::Between) => match value.as_array().map(Vec::as_slice) {
            Some([low, high]) if low.is_number() && high.is_number() => Ok(()),
            _ => Err(invalid(value_path, "must be a pair of numbers")),
        },
        (TaskFieldKind::Number, _) => {
            if value.is_number() {
                Ok(())
            } else {
                Err(invalid(value_path, "must be a number"))
            }
        }
        (TaskFieldKind::Date, O::Between) => parse_date_range(value, &value_path).map(|_| ()),
        (TaskFieldKind::Date, _) => parse_date_value(value, &value_path).map(|_| ()),
        (TaskFieldKind::Select | TaskFieldKind::MultiSelect, _) => {
            validate_option_ids(value, &value_path)
        }
        (TaskFieldKind::Checkbox, _) => {
            if value.is_boolean() {
                Ok(())
            } else {
                Err(invalid(value_path, "must be true or false"))
            }
        }
        (TaskFieldKind::TimeTracking, _) => Err(invalid(
            format!("{path}.operator"),
            "is not supported for this field",
        )),
    }
}

fn validate_option_ids(value: &Value, path: &str) -> Result<(), FilterError> {
    let Some(items) = value.as_array() else {
        return Err(invalid(path, "must be an array of option ids"));
    };
    if items.is_empty() {
        return Err(invalid(path, "must contain at least one value"));
    }
    if items.len() > MAX_VALUES {
        return Err(invalid(path, "must contain at most 100 values"));
    }
    for (index, item) in items.iter().enumerate() {
        if !item.as_str().is_some_and(is_id) {
            return Err(invalid(format!("{path}[{index}]"), "must be an option id"));
        }
    }
    Ok(())
}

/// A task field as filters and sorting read it: its kind and its options' ids, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CustomFieldSpec {
    pub kind: TaskFieldKind,
    pub option_ids: Vec<String>,
}

/// The workspace's task fields that a query names, by id. A missing id is a deleted field.
pub type CustomFields = HashMap<Id, CustomFieldSpec>;

/// The task fields that `custom_field` conditions of the tree name.
pub fn custom_field_ids(group: &FilterGroup) -> BTreeSet<Id> {
    fn walk(group: &FilterGroup, ids: &mut BTreeSet<Id>) {
        for child in &group.children {
            match child {
                FilterNode::Group(inner) => walk(inner, ids),
                FilterNode::Condition(condition) => {
                    if let Some(id) = condition.field_id {
                        ids.insert(id);
                    }
                }
            }
        }
    }
    let mut ids = BTreeSet::new();
    walk(group, &mut ids);
    ids
}

/// Checks each `custom_field` condition against its field's real kind. Conditions on deleted
/// fields pass (they match as if no task had a value).
pub fn validate_custom_fields(
    group: &FilterGroup,
    fields: &CustomFields,
) -> Result<(), FilterError> {
    fn walk(group: &FilterGroup, path: &str, fields: &CustomFields) -> Result<(), FilterError> {
        for (index, child) in group.children.iter().enumerate() {
            let child_path = format!("{path}.children[{index}]");
            match child {
                FilterNode::Group(inner) => walk(inner, &child_path, fields)?,
                FilterNode::Condition(condition) => {
                    if let Some(spec) = condition.field_id.and_then(|id| fields.get(&id)) {
                        validate_custom_condition(spec.kind, condition, &child_path)?;
                    }
                }
            }
        }
        Ok(())
    }
    walk(group, "filter", fields)
}

fn validate_text(value: &Value, path: &str) -> Result<(), FilterError> {
    let Some(text) = value.as_str() else {
        return Err(invalid(path, "must be a string"));
    };
    if text.is_empty() {
        return Err(invalid(path, "must not be empty"));
    }
    if text.chars().count() > MAX_TEXT_CHARS {
        return Err(invalid(path, "must be at most 200 characters"));
    }
    Ok(())
}

fn validate_values(field: FilterField, value: &Value, path: &str) -> Result<(), FilterError> {
    let Some(items) = value.as_array() else {
        return Err(invalid(path, "must be an array of strings"));
    };
    if items.is_empty() {
        return Err(invalid(path, "must contain at least one value"));
    }
    if items.len() > MAX_VALUES {
        return Err(invalid(path, "must contain at most 100 values"));
    }
    for (index, item) in items.iter().enumerate() {
        let item_path = format!("{path}[{index}]");
        let Some(text) = item.as_str() else {
            return Err(invalid(item_path, "must be a string"));
        };
        let valid = match field {
            FilterField::Status => valid_status_value(text),
            FilterField::StatusCategory => STATUS_CATEGORIES.contains(&text),
            FilterField::Priority => PRIORITIES.contains(&text),
            FilterField::Assignee | FilterField::Creator => text == "me" || is_id(text),
            FilterField::Label => valid_label_value(text),
            FilterField::Project | FilterField::Milestone => is_id(text),
            FilterField::Cycle => RELATIVE_CYCLES.contains(&text) || is_id(text),
            FilterField::DueDate
            | FilterField::CreatedAt
            | FilterField::UpdatedAt
            | FilterField::Text
            | FilterField::Estimate
            | FilterField::CustomField => false,
            FilterField::Parent => text == "none" || is_id(text),
            FilterField::SubIssues => matches!(text, "has" | "none"),
        };
        if !valid {
            return Err(invalid(item_path, "is not a valid value for this field"));
        }
    }
    Ok(())
}

fn is_id(value: &str) -> bool {
    value.parse::<Id>().is_ok()
}

/// A label key (its name) or a label id (views saved before labels were per project).
fn valid_label_value(value: &str) -> bool {
    !value.trim().is_empty() && value.chars().count() <= MAX_LABEL_NAME_CHARS
}

/// A `category:name` status key, or a bare status id (legacy `GET /tasks?status_id=`).
fn valid_status_value(value: &str) -> bool {
    match value.split_once(':') {
        Some((category, name)) => {
            STATUS_CATEGORIES.contains(&category)
                && !name.trim().is_empty()
                && name.chars().count() <= MAX_STATUS_NAME_CHARS
        }
        None => is_id(value),
    }
}

fn validate_display(display: &DisplayOptions) -> Result<(), FilterError> {
    if display.sub_group_by != GroupBy::None && display.sub_group_by == display.group_by {
        return Err(invalid("display.sub_group_by", "must differ from group_by"));
    }
    for (index, property) in display.properties.iter().enumerate() {
        if display.properties[..index].contains(property) {
            return Err(invalid(
                format!("display.properties[{index}]"),
                "is listed more than once",
            ));
        }
    }
    if display.custom_fields.len() > MAX_DISPLAY_FIELDS {
        return Err(invalid(
            "display.custom_fields",
            "must contain at most 50 fields",
        ));
    }
    for (index, field) in display.custom_fields.iter().enumerate() {
        if display.custom_fields[..index].contains(field) {
            return Err(invalid(
                format!("display.custom_fields[{index}]"),
                "is listed more than once",
            ));
        }
    }
    match (display.order_by, display.order_field_id) {
        (OrderBy::CustomField, None) => Err(invalid(
            "display.order_field_id",
            "is required when order_by is custom_field",
        )),
        (OrderBy::CustomField, Some(_)) | (_, None) => Ok(()),
        (_, Some(_)) => Err(invalid(
            "display.order_field_id",
            "must be omitted unless order_by is custom_field",
        )),
    }
}

/// Parses `{ "absolute": "YYYY-MM-DD" }`, `{ "instant": epoch_ms }` or
/// `{ "relative": …, "offset_days"?: n }`.
pub fn parse_date_value(value: &Value, path: &str) -> Result<DateValue, FilterError> {
    let Some(object) = value.as_object() else {
        return Err(invalid(path, "must be a date value"));
    };
    if let Some(instant) = object.get("instant") {
        if let Some(key) = object.keys().find(|key| key.as_str() != "instant") {
            return Err(invalid(
                format!("{path}.{key}"),
                "cannot be combined with instant",
            ));
        }
        let millis = instant
            .as_i64()
            .ok_or_else(|| invalid(format!("{path}.instant"), "must be epoch milliseconds"))?;
        return Ok(DateValue::Instant { millis });
    }
    if let Some(absolute) = object.get("absolute") {
        if let Some(key) = object.keys().find(|key| key.as_str() != "absolute") {
            return Err(invalid(
                format!("{path}.{key}"),
                "cannot be combined with absolute",
            ));
        }
        let day = absolute
            .as_str()
            .and_then(parse_ymd)
            .ok_or_else(|| invalid(format!("{path}.absolute"), "must be a YYYY-MM-DD date"))?;
        return Ok(DateValue::Absolute { day });
    }
    let Some(relative) = object.get("relative") else {
        return Err(invalid(path, "must have absolute or relative"));
    };
    if let Some(key) = object
        .keys()
        .find(|key| !matches!(key.as_str(), "relative" | "offset_days"))
    {
        return Err(invalid(
            format!("{path}.{key}"),
            "is not a date value field",
        ));
    }
    let anchor = match relative.as_str() {
        Some("today") => RelativeDay::Today,
        Some("start_of_week") => RelativeDay::StartOfWeek,
        Some("end_of_week") => RelativeDay::EndOfWeek,
        _ => {
            return Err(invalid(
                format!("{path}.relative"),
                "must be today, start_of_week or end_of_week",
            ));
        }
    };
    let offset_days = match object.get("offset_days") {
        None | Some(Value::Null) => 0,
        Some(offset) => offset
            .as_i64()
            .filter(|days| (-MAX_OFFSET_DAYS..=MAX_OFFSET_DAYS).contains(days))
            .ok_or_else(|| {
                invalid(
                    format!("{path}.offset_days"),
                    "must be a whole number between -3650 and 3650",
                )
            })?,
    };
    Ok(DateValue::Relative {
        anchor,
        offset_days,
    })
}

fn parse_date_range(value: &Value, path: &str) -> Result<(DateValue, DateValue), FilterError> {
    match value.as_array().map(Vec::as_slice) {
        Some([start, end]) => Ok((
            parse_date_value(start, &format!("{path}[0]"))?,
            parse_date_value(end, &format!("{path}[1]"))?,
        )),
        _ => Err(invalid(path, "must be a pair of date values")),
    }
}

/// The UTC day (days since 1970-01-01) a date value names at `now`. Weeks start on Monday.
pub fn resolve_day(date: DateValue, now: TimestampMillis) -> i64 {
    match date {
        DateValue::Absolute { day } => day,
        DateValue::Instant { millis } => millis.div_euclid(DAY_MS),
        DateValue::Relative {
            anchor,
            offset_days,
        } => {
            let today = now.as_millis().div_euclid(DAY_MS);
            // 1970-01-01 was a Thursday, so (day + 3) mod 7 is 0 on Mondays.
            let monday = today - (today + 3).rem_euclid(7);
            let base = match anchor {
                RelativeDay::Today => today,
                RelativeDay::StartOfWeek => monday,
                RelativeDay::EndOfWeek => monday + 6,
            };
            base + offset_days
        }
    }
}

pub(crate) fn parse_ymd(text: &str) -> Option<i64> {
    let bytes = text.as_bytes();
    if !text.is_ascii() || bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    let number = |start: usize, end: usize| -> Option<i64> {
        let part = &text[start..end];
        if part.bytes().all(|byte| byte.is_ascii_digit()) {
            part.parse().ok()
        } else {
            None
        }
    };
    let (year, month, day) = (number(0, 4)?, number(5, 7)?, number(8, 10)?);
    if year < 1 || !(1..=12).contains(&month) || !(1..=days_in_month(year, month)).contains(&day) {
        return None;
    }
    Some(days_from_civil(year, month, day))
}

fn days_in_month(year: i64, month: i64) -> i64 {
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's algorithm).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month_from_march = (month + 9) % 12;
    let day_of_year = (153 * month_from_march + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// The fixed filter of a built-in task page (`/tasks?view=…`), or `None` for unknown names.
pub fn preset_filter(view: &str) -> Option<FilterGroup> {
    let this_week = || {
        condition(
            FilterField::DueDate,
            FilterOperator::Between,
            json!([{ "relative": "start_of_week" }, { "relative": "end_of_week" }]),
        )
    };
    let not_done = || {
        condition(
            FilterField::StatusCategory,
            FilterOperator::IsNot,
            json!(DONE_CATEGORIES),
        )
    };
    let mine = || condition(FilterField::Assignee, FilterOperator::Is, json!(["me"]));
    let children = match view {
        "mine" => vec![mine()],
        "overdue" => vec![
            condition(
                FilterField::DueDate,
                FilterOperator::Before,
                json!({ "relative": "today" }),
            ),
            not_done(),
        ],
        "due_soon" => vec![
            condition(
                FilterField::DueDate,
                FilterOperator::Between,
                json!([{ "relative": "today" }, { "relative": "today", "offset_days": 6 }]),
            ),
            not_done(),
        ],
        "current_week" => vec![this_week(), not_done()],
        "my_week" => vec![this_week(), not_done(), mine()],
        _ => return None,
    };
    Some(FilterGroup {
        op: GroupOp::And,
        children,
    })
}

/// ANDs groups into one root: AND groups are spliced in, non-empty OR groups become children,
/// and empty OR groups (which match everything) are dropped.
pub fn and_groups(groups: Vec<FilterGroup>) -> FilterGroup {
    let mut children = Vec::new();
    for group in groups {
        match group.op {
            GroupOp::And => children.extend(group.children),
            GroupOp::Or if group.children.is_empty() => {}
            GroupOp::Or => children.push(FilterNode::Group(group)),
        }
    }
    FilterGroup {
        op: GroupOp::And,
        children,
    }
}

fn condition(field: FilterField, operator: FilterOperator, value: Value) -> FilterNode {
    FilterNode::Condition(Condition {
        field,
        field_id: None,
        operator,
        value,
    })
}

/// Request context for compiling a filter: `me` is `actor_id`, and relative dates and the
/// `show_completed` window resolve against `now` (UTC).
#[derive(Clone, Copy, Debug)]
pub struct FilterContext<'a> {
    /// The projects the reader sees: parents and sub-issues outside it do not count.
    pub(crate) scope: ProjectScope,
    pub actor_id: Id,
    pub now: TimestampMillis,
    /// Text fields are JSON in `task_field_values.value_json`; reading the text differs.
    pub backend: Backend,
    /// The fields that `custom_field` conditions name ([`custom_field_ids`]).
    pub custom_fields: &'a CustomFields,
}

/// An assignee that is still an active (member, not suspended) user, as in `unassigned`.
const ACTIVE_ASSIGNEE: &str = "SELECT 1 FROM task_assignees \
     JOIN memberships ON memberships.id = task_assignees.membership_id \
     JOIN users ON users.id = task_assignees.user_id \
     WHERE task_assignees.task_id = tasks.id AND memberships.workspace_id = tasks.workspace_id \
     AND memberships.user_id = task_assignees.user_id AND users.suspended_at IS NULL";
const TASK_LABEL: &str = "SELECT 1 FROM task_labels WHERE task_labels.task_id = tasks.id";
const TASK_STATUS: &str = "SELECT 1 FROM task_statuses WHERE task_statuses.id = tasks.status_id";
const DONE_STATUS: &str = "EXISTS (SELECT 1 FROM task_statuses \
     WHERE task_statuses.id = tasks.status_id \
     AND task_statuses.category IN ('completed', 'cancelled', 'duplicate'))";
/// A live direct child of the outer task that the reader sees, counted like
/// `TaskRecord.sub_issue_count`.
fn live_child_sql(scope: ProjectScope) -> String {
    format!(
        "SELECT 1 FROM tasks AS child \
         JOIN projects AS child_project ON child_project.id = child.project_id \
         WHERE child.parent_task_id = tasks.id AND child.deleted_at IS NULL \
         AND child_project.deleted_at IS NULL AND {}",
        scope.project("child.project_id")
    )
}

/// Appends ` AND (<compiled tree>)` to `query`. Every value is bound. The tree must already be
/// validated; an unexpected node fails closed (`0 = 1`).
pub fn push_filter(query: &mut SqlBuilder, group: &FilterGroup, ctx: &FilterContext) {
    query.push(" AND (");
    push_group(query, group, ctx);
    query.push(")");
}

/// Appends the `show_completed` restriction; `All` adds nothing.
pub fn push_show_completed(query: &mut SqlBuilder, show: ShowCompleted, now: TimestampMillis) {
    let window = match show {
        ShowCompleted::All => return,
        ShowCompleted::None => {
            query.push(" AND NOT ").push(DONE_STATUS);
            return;
        }
        ShowCompleted::PastWeek => 7 * DAY_MS,
        ShowCompleted::PastMonth => 30 * DAY_MS,
    };
    query
        .push(" AND (NOT ")
        .push(DONE_STATUS)
        .push(" OR tasks.completed_at >= ")
        .push_bind(now.as_millis() - window)
        .push(")");
}

fn push_group(query: &mut SqlBuilder, group: &FilterGroup, ctx: &FilterContext) {
    if group.children.is_empty() {
        query.push("1 = 1");
        return;
    }
    let joiner = match group.op {
        GroupOp::And => " AND ",
        GroupOp::Or => " OR ",
    };
    for (index, child) in group.children.iter().enumerate() {
        if index > 0 {
            query.push(joiner);
        }
        query.push("(");
        match child {
            FilterNode::Group(inner) => push_group(query, inner, ctx),
            FilterNode::Condition(condition) => push_condition(query, condition, ctx),
        }
        query.push(")");
    }
}

fn push_condition(query: &mut SqlBuilder, condition: &Condition, ctx: &FilterContext) {
    use FilterField as F;
    use FilterOperator as O;
    let negated = matches!(condition.operator, O::IsNot | O::Excludes);
    let value = &condition.value;
    match (condition.field, condition.operator) {
        (F::Status, O::Is | O::IsNot) => push_status(query, value, negated),
        (F::StatusCategory, O::Is | O::IsNot) => push_exists(
            query,
            negated,
            TASK_STATUS,
            " AND task_statuses.category IN ",
            values(value, ctx, false),
        ),
        (F::Assignee, O::Is | O::IsNot) => push_exists(
            query,
            negated,
            ACTIVE_ASSIGNEE,
            " AND task_assignees.user_id IN ",
            values(value, ctx, true),
        ),
        (F::Assignee, O::IsEmpty) => {
            query.push("NOT EXISTS (").push(ACTIVE_ASSIGNEE).push(")");
        }
        (F::Assignee, O::IsNotEmpty) => {
            query.push("EXISTS (").push(ACTIVE_ASSIGNEE).push(")");
        }
        (F::Creator, O::Is | O::IsNot) => {
            push_column_in(query, "tasks.creator_id", negated, values(value, ctx, true));
        }
        (F::Label, O::IncludesAny | O::Excludes) => {
            push_labels(query, values(value, ctx, false), false, negated);
        }
        (F::Label, O::IncludesAll) => push_labels(query, values(value, ctx, false), true, false),
        (F::Label, O::IsEmpty) => {
            query.push("NOT EXISTS (").push(TASK_LABEL).push(")");
        }
        (F::Label, O::IsNotEmpty) => {
            query.push("EXISTS (").push(TASK_LABEL).push(")");
        }
        (F::Priority, O::Is | O::IsNot) => {
            push_column_in(query, "tasks.priority", negated, values(value, ctx, false));
        }
        (F::Project, O::Is | O::IsNot) => {
            push_column_in(
                query,
                "tasks.project_id",
                negated,
                values(value, ctx, false),
            );
        }
        (F::DueDate, O::IsEmpty) => {
            query.push("tasks.due_at IS NULL");
        }
        (F::DueDate, O::IsNotEmpty) => {
            query.push("tasks.due_at IS NOT NULL");
        }
        (F::DueDate, O::Before | O::After | O::Between) => {
            push_date(query, "tasks.due_at", condition, ctx.now);
        }
        (F::CreatedAt, O::Before | O::After) => {
            push_date(query, "tasks.created_at", condition, ctx.now);
        }
        (F::UpdatedAt, O::Before | O::After) => {
            push_date(query, "tasks.updated_at", condition, ctx.now);
        }
        (F::Text, O::Contains) => push_text(query, value),
        (F::Parent, O::Is | O::IsNot) => {
            push_parent(query, ctx.scope, values(value, ctx, false), negated)
        }
        (F::SubIssues, O::Is) => push_sub_issues(query, ctx.scope, values(value, ctx, false)),
        (F::Milestone, O::Is | O::IsNot) => {
            push_column_in(
                query,
                "tasks.milestone_id",
                negated,
                values(value, ctx, false),
            );
        }
        (F::Milestone, O::IsEmpty) => {
            query.push("tasks.milestone_id IS NULL");
        }
        (F::Milestone, O::IsNotEmpty) => {
            query.push("tasks.milestone_id IS NOT NULL");
        }
        (F::Cycle, O::Is | O::IsNot) => {
            push_cycle(query, values(value, ctx, false), negated, ctx.now);
        }
        (F::Cycle, O::IsEmpty) => {
            query.push("tasks.cycle_id IS NULL");
        }
        (F::Cycle, O::IsNotEmpty) => {
            query.push("tasks.cycle_id IS NOT NULL");
        }
        (F::Estimate, O::IsEmpty) => {
            query.push("tasks.estimate IS NULL");
        }
        (F::Estimate, O::IsNotEmpty) => {
            query.push("tasks.estimate IS NOT NULL");
        }
        (F::CustomField, _) => push_custom(query, condition, ctx),
        _ => {
            query.push("0 = 1");
        }
    }
}

/// The condition's string values, deduplicated; `me` becomes the caller when `resolve_me`.
fn values(value: &Value, ctx: &FilterContext, resolve_me: bool) -> Vec<String> {
    let unique: BTreeSet<String> = value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(|item| {
            if resolve_me && item == "me" {
                ctx.actor_id.to_string()
            } else {
                item.to_owned()
            }
        })
        .collect();
    unique.into_iter().collect()
}

/// Pushes `(?, ?, …)`.
fn push_list(query: &mut SqlBuilder, values: Vec<String>) {
    let mut list = query.separated(", ");
    list.push_unseparated("(");
    for value in values {
        list.push_bind(value);
    }
    list.push_unseparated(")");
}

fn push_exists(
    query: &mut SqlBuilder,
    negated: bool,
    subquery: &'static str,
    column_in: &'static str,
    values: Vec<String>,
) {
    if values.is_empty() {
        query.push(if negated { "1 = 1" } else { "0 = 1" });
        return;
    }
    query
        .push(if negated { "NOT EXISTS (" } else { "EXISTS (" })
        .push(subquery)
        .push(column_in);
    push_list(query, values);
    query.push(")");
}

fn push_column_in(
    query: &mut SqlBuilder,
    column: &'static str,
    negated: bool,
    values: Vec<String>,
) {
    if values.is_empty() {
        query.push(if negated { "1 = 1" } else { "0 = 1" });
        return;
    }
    if negated {
        query
            .push("(")
            .push(column)
            .push(" IS NULL OR ")
            .push(column)
            .push(" NOT IN ");
        push_list(query, values);
        query.push(")");
    } else {
        query.push(column).push(" IN ");
        push_list(query, values);
    }
}

/// `parent is [ids…, "none"]`, matched against the visible parent (the `parent` field): a task
/// whose parent is hidden has parent `none`. The positive expression is never NULL (the EXISTS
/// is false for a NULL parent), so `is_not [id]` keeps top-level tasks.
fn push_parent(query: &mut SqlBuilder, scope: ProjectScope, values: Vec<String>, negated: bool) {
    let none = values.iter().any(|value| value == "none");
    let ids: Vec<String> = values.into_iter().filter(|value| value != "none").collect();
    if !none && ids.is_empty() {
        query.push(if negated { "1 = 1" } else { "0 = 1" });
        return;
    }
    let visible_parent = sub_issues::has_visible_parent_sql(scope, "tasks.parent_task_id");
    query.push(if negated { "NOT (" } else { "(" });
    if none {
        query.push("NOT ").push(&visible_parent);
        if !ids.is_empty() {
            query.push(" OR ");
        }
    }
    if !ids.is_empty() {
        query
            .push("(")
            .push(&visible_parent)
            .push(" AND tasks.parent_task_id IN ");
        push_list(query, ids);
        query.push(")");
    }
    query.push(")");
}

/// `cycle is [ids…, current, next, previous]`. Relative values name a cycle of the task's own
/// project at `now`; cycles cover `[starts_at, ends_at)`. Every alternative is an `IN` or an
/// `EXISTS` on a non-null `cycle_id`, so the expression is never NULL and `is_not` keeps tasks
/// without a cycle.
fn push_cycle(query: &mut SqlBuilder, values: Vec<String>, negated: bool, now: TimestampMillis) {
    let (relative, ids): (Vec<String>, Vec<String>) = values
        .into_iter()
        .partition(|value| RELATIVE_CYCLES.contains(&value.as_str()));
    if relative.is_empty() && ids.is_empty() {
        query.push(if negated { "1 = 1" } else { "0 = 1" });
        return;
    }
    let now = now.as_millis();
    query.push(if negated {
        "(tasks.cycle_id IS NULL OR NOT ("
    } else {
        "(tasks.cycle_id IS NOT NULL AND ("
    });
    let mut first = true;
    let mut or = |query: &mut SqlBuilder| {
        if !std::mem::take(&mut first) {
            query.push(" OR ");
        }
    };
    if !ids.is_empty() {
        or(query);
        query.push("tasks.cycle_id IN ");
        push_list(query, ids);
    }
    for value in relative {
        or(query);
        match value.as_str() {
            "current" => {
                query
                    .push(
                        "EXISTS (SELECT 1 FROM task_cycles AS cycle WHERE cycle.id = tasks.cycle_id                          AND cycle.starts_at <= ",
                    )
                    .push_bind(now)
                    .push(" AND cycle.ends_at > ")
                    .push_bind(now)
                    .push(")");
            }
            "next" => {
                query
                    .push(
                        "EXISTS (SELECT 1 FROM task_cycles AS cycle WHERE cycle.id = tasks.cycle_id                          AND cycle.id = (SELECT later.id FROM task_cycles AS later                          WHERE later.project_id = tasks.project_id AND later.starts_at > ",
                    )
                    .push_bind(now)
                    .push(" ORDER BY later.starts_at, later.id LIMIT 1))");
            }
            _ => {
                query
                    .push(
                        "EXISTS (SELECT 1 FROM task_cycles AS cycle WHERE cycle.id = tasks.cycle_id                          AND cycle.id = (SELECT earlier.id FROM task_cycles AS earlier                          WHERE earlier.project_id = tasks.project_id AND earlier.ends_at <= ",
                    )
                    .push_bind(now)
                    .push(" ORDER BY earlier.ends_at DESC, earlier.id DESC LIMIT 1))");
            }
        }
    }
    query.push("))");
}

/// `sub_issues is [has|none]`: whether the task has live direct children.
fn push_sub_issues(query: &mut SqlBuilder, scope: ProjectScope, values: Vec<String>) {
    let live_child = live_child_sql(scope);
    let has = values.iter().any(|value| value == "has");
    let none = values.iter().any(|value| value == "none");
    match (has, none) {
        (true, true) => {
            query.push("1 = 1");
        }
        (true, false) => {
            query.push("EXISTS (").push(&live_child).push(")");
        }
        (false, true) => {
            query.push("NOT EXISTS (").push(&live_child).push(")");
        }
        (false, false) => {
            query.push("0 = 1");
        }
    }
}

/// Labels belong to one project, so label conditions match by name (trimmed, case-insensitive)
/// and find the same name in every project. A value is a label key (the name, as the web's
/// `labelKeyOf`) or a label id that stands for its name, as in views saved before labels were
/// per project. `all` needs every value on the task, else any one.
fn push_labels(query: &mut SqlBuilder, values: Vec<String>, all: bool, negated: bool) {
    if values.is_empty() {
        query.push(if negated { "1 = 1" } else { "0 = 1" });
        return;
    }
    query.push(if negated { "NOT (" } else { "(" });
    for (index, value) in values.into_iter().enumerate() {
        if index > 0 {
            query.push(if all { " AND " } else { " OR " });
        }
        query.push(
            "EXISTS (SELECT 1 FROM task_labels JOIN labels ON labels.id = task_labels.label_id \
             WHERE task_labels.task_id = tasks.id AND lower(trim(labels.name)) = ",
        );
        if value.parse::<Id>().is_ok() {
            // An id from before labels were per project keeps the name it had then (its copy may
            // be renamed or gone since); a newer one is read from its label. An unknown id has no
            // name, so it matches nothing.
            query
                .push(
                    "COALESCE((SELECT lower(trim(alias.name)) FROM label_name_aliases AS alias \
                     WHERE alias.workspace_id = tasks.workspace_id AND alias.label_id = ",
                )
                .push_bind(value.clone())
                .push(
                    "), (SELECT lower(trim(selected.name)) FROM labels AS selected \
                     WHERE selected.workspace_id = tasks.workspace_id AND selected.id = ",
                )
                .push_bind(value)
                .push("))");
        } else {
            query
                .push("lower(trim(")
                .push_bind(value.trim().to_owned())
                .push("))");
        }
        query.push(")");
    }
    query.push(")");
}

/// Status values are `category:name` keys (names compared lower-cased and trimmed, like the
/// web's `statusKeyOf`; SQLite `lower()` folds ASCII only) or bare status ids.
fn push_status(query: &mut SqlBuilder, value: &Value, negated: bool) {
    let mut keys = BTreeSet::new();
    let mut ids = BTreeSet::new();
    for item in value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        match item.split_once(':') {
            Some((category, name)) => {
                keys.insert(format!("{category}:{}", name.trim().to_ascii_lowercase()));
            }
            None => {
                ids.insert(item.to_owned());
            }
        }
    }
    if keys.is_empty() && ids.is_empty() {
        query.push(if negated { "1 = 1" } else { "0 = 1" });
        return;
    }
    // Bare ids compare `tasks.status_id` directly so the legacy `status_id=` filter keeps
    // using the index.
    query.push(if negated { "NOT (" } else { "(" });
    let has_ids = !ids.is_empty();
    if !keys.is_empty() {
        query
            .push("EXISTS (")
            .push(TASK_STATUS)
            .push(" AND (task_statuses.category || ':' || lower(trim(task_statuses.name))) IN ");
        push_list(query, keys.into_iter().collect());
        query.push(")");
        if has_ids {
            query.push(" OR ");
        }
    }
    if has_ids {
        query.push("tasks.status_id IN ");
        push_list(query, ids.into_iter().collect());
    }
    query.push(")");
}

/// `before D`: `< D`; `after D`: `>= D + 1 day`; `between [a, b]`: `>= a AND < b + 1 day`.
/// An instant is exact: `before T`: `< T`; `after T`: `> T`; as `between` end: `<= T`.
fn push_date(
    query: &mut SqlBuilder,
    column: &'static str,
    condition: &Condition,
    now: TimestampMillis,
) {
    let start_of = |day: i64| day.saturating_mul(DAY_MS);
    // The first moment of a day-or-instant operand, and the first moment after it.
    let first = |date: DateValue| match date {
        DateValue::Instant { millis } => millis,
        date => start_of(resolve_day(date, now)),
    };
    let after = |date: DateValue| match date {
        DateValue::Instant { millis } => millis.saturating_add(1),
        date => start_of(resolve_day(date, now) + 1),
    };
    let bounds = match condition.operator {
        FilterOperator::Before => parse_date_value(&condition.value, "value")
            .ok()
            .map(|date| (None, Some(first(date)))),
        FilterOperator::After => parse_date_value(&condition.value, "value")
            .ok()
            .map(|date| (Some(after(date)), None)),
        FilterOperator::Between => parse_date_range(&condition.value, "value")
            .ok()
            .map(|(start, end)| (Some(first(start)), Some(after(end)))),
        _ => None,
    };
    match bounds {
        Some((Some(from), Some(until))) => {
            query
                .push(column)
                .push(" >= ")
                .push_bind(from)
                .push(" AND ")
                .push(column)
                .push(" < ")
                .push_bind(until);
        }
        Some((Some(from), None)) => {
            query.push(column).push(" >= ").push_bind(from);
        }
        Some((None, Some(until))) => {
            query.push(column).push(" < ").push_bind(until);
        }
        Some((None, None)) | None => {
            query.push("0 = 1");
        }
    }
}

fn push_text(query: &mut SqlBuilder, value: &Value) {
    let Some(text) = value.as_str().filter(|text| !text.is_empty()) else {
        query.push("0 = 1");
        return;
    };
    let pattern = format!("%{}%", escape_like(&text.to_lowercase()));
    query
        .push("LOWER(tasks.title) LIKE ")
        .push_bind(pattern.clone())
        .push(" ESCAPE '\\' OR LOWER(tasks.description) LIKE ")
        .push_bind(pattern)
        .push(" ESCAPE '\\'");
    // An identifier (`ENG-12`, any case) finds that task; a bare number every task with it.
    let trimmed = text.trim();
    if let Some((key, number)) = super::tasks::parse_task_identifier(trimmed) {
        query
            .push(" OR (tasks.number = ")
            .push_bind(number)
            .push(" AND tasks.project_id IN (SELECT id FROM projects WHERE workspace_id = tasks.workspace_id AND project_key = ")
            .push_bind(key)
            .push("))");
    } else if let Some(number) = trimmed
        .strip_prefix('#')
        .unwrap_or(trimmed)
        .parse::<i64>()
        .ok()
        .filter(|number| *number > 0)
    {
        query.push(" OR tasks.number = ").push_bind(number);
    }
}

/// `task_field_values` row of the outer task for one field (the field id is bound next).
const FIELD_VALUE: &str = "SELECT 1 FROM task_field_values AS field_value \
     WHERE field_value.task_id = tasks.id AND field_value.field_id = ";
/// A number value: its JSON text is a plain number, so both databases cast it.
const FIELD_NUMBER: &str = "CAST(field_value.value_json AS DOUBLE PRECISION)";

/// The text of a JSON string value (`text`, `url`) in `column`.
fn json_text_sql(backend: Backend, column: &str) -> String {
    match backend {
        Backend::Sqlite => format!("json_extract({column}, '$')"),
        Backend::Postgres => format!("(CAST({column} AS jsonb) #>> '{{}}')"),
    }
}

/// A value as `value_json` stores it (serde_json's canonical form, as `set_task_field_value`
/// writes it).
fn json_string(text: &str) -> String {
    Value::String(text.to_owned()).to_string()
}

/// `[NOT] EXISTS (value row of the field AND (predicate))`.
fn push_field_value_exists(
    query: &mut SqlBuilder,
    field_id: Id,
    negated: bool,
    predicate: impl FnOnce(&mut SqlBuilder),
) {
    query
        .push(if negated { "NOT EXISTS (" } else { "EXISTS (" })
        .push(FIELD_VALUE)
        .push_bind(field_id.to_string())
        .push(" AND (");
    predicate(query);
    query.push("))");
}

/// The value in `column` holds the option: a select value is the option id, a multi-select value
/// an array of ids (ids never contain `"`, so the quoted id is a safe substring).
fn push_option_match(query: &mut SqlBuilder, kind: TaskFieldKind, column: &str, option_id: &str) {
    if kind == TaskFieldKind::Select {
        query
            .push(column)
            .push(" = ")
            .push_bind(json_string(option_id));
    } else {
        query
            .push(column)
            .push(" LIKE ")
            .push_bind(format!("%{}%", escape_like(&json_string(option_id))))
            .push(" ESCAPE '\\'");
    }
}

/// Any (or with `all`, every) one of `option_ids`; no ids matches nothing.
fn push_options(
    query: &mut SqlBuilder,
    kind: TaskFieldKind,
    column: &str,
    option_ids: &[String],
    all: bool,
) {
    if option_ids.is_empty() {
        query.push("0 = 1");
        return;
    }
    query.push("(");
    for (index, id) in option_ids.iter().enumerate() {
        if index > 0 {
            query.push(if all { " AND " } else { " OR " });
        }
        push_option_match(query, kind, column, id);
    }
    query.push(")");
}

/// `YYYY-MM-DD` of a day counted from 1970-01-01.
fn ymd(day: i64) -> Option<String> {
    let days_from_ce = i32::try_from(day.checked_add(719_163)?).ok()?;
    chrono::NaiveDate::from_num_days_from_ce_opt(days_from_ce)
        .map(|date| date.format("%Y-%m-%d").to_string())
}

/// A `custom_field` condition. Values of options removed from the field do not count (as they
/// do not show). Dates compare as their quoted `"YYYY-MM-DD"` JSON text, which sorts by day.
fn push_custom(query: &mut SqlBuilder, condition: &Condition, ctx: &FilterContext) {
    use FilterOperator as O;
    use TaskFieldKind as K;
    let operator = condition.operator;
    let value = &condition.value;
    let Some(field_id) = condition.field_id else {
        query.push("0 = 1");
        return;
    };
    let Some(spec) = ctx.custom_fields.get(&field_id) else {
        // A deleted field: no task has a value.
        let matches_empty = matches!(operator, O::IsEmpty | O::IsNot | O::Excludes)
            || (operator == O::Is && value == &Value::Bool(false));
        query.push(if matches_empty { "1 = 1" } else { "0 = 1" });
        return;
    };
    let column = "field_value.value_json";
    let live = |requested: &Value| -> Vec<String> {
        let wanted: BTreeSet<&str> = requested
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        spec.option_ids
            .iter()
            .filter(|id| wanted.contains(id.as_str()))
            .cloned()
            .collect()
    };
    match (spec.kind, operator) {
        (kind, O::IsEmpty | O::IsNotEmpty) => {
            push_field_value_exists(query, field_id, operator == O::IsEmpty, |query| {
                if matches!(kind, K::Select | K::MultiSelect) {
                    push_options(query, kind, column, &spec.option_ids, false);
                } else {
                    query.push("1 = 1");
                }
            });
        }
        (K::Text | K::Url, O::Contains) => {
            let Some(text) = value.as_str().filter(|text| !text.is_empty()) else {
                query.push("0 = 1");
                return;
            };
            let pattern = format!("%{}%", escape_like(&text.to_lowercase()));
            let text_sql = json_text_sql(ctx.backend, column);
            push_field_value_exists(query, field_id, false, |query| {
                query
                    .push("lower(")
                    .push(&text_sql)
                    .push(") LIKE ")
                    .push_bind(pattern)
                    .push(" ESCAPE '\\'");
            });
        }
        (K::Number, O::Is | O::LessThan | O::GreaterThan) => {
            let Some(number) = value.as_f64() else {
                query.push("0 = 1");
                return;
            };
            let comparison = match operator {
                O::LessThan => " < ",
                O::GreaterThan => " > ",
                _ => " = ",
            };
            push_field_value_exists(query, field_id, false, |query| {
                query.push(FIELD_NUMBER).push(comparison).push_bind(number);
            });
        }
        (K::Number, O::Between) => {
            let bounds = value.as_array().and_then(|pair| match pair.as_slice() {
                [low, high] => Some((low.as_f64()?, high.as_f64()?)),
                _ => None,
            });
            let Some((low, high)) = bounds else {
                query.push("0 = 1");
                return;
            };
            push_field_value_exists(query, field_id, false, |query| {
                query
                    .push(FIELD_NUMBER)
                    .push(" >= ")
                    .push_bind(low)
                    .push(" AND ")
                    .push(FIELD_NUMBER)
                    .push(" <= ")
                    .push_bind(high);
            });
        }
        (K::Date, O::Before | O::After | O::Between) => {
            let day =
                |date: DateValue| ymd(resolve_day(date, ctx.now)).map(|day| json_string(&day));
            // (lower bound inclusive, upper bound inclusive) or strict bounds for before/after
            let bounds = match operator {
                O::Before => parse_date_value(value, "value")
                    .ok()
                    .and_then(day)
                    .map(|until| vec![(" < ", until)]),
                O::After => parse_date_value(value, "value")
                    .ok()
                    .and_then(day)
                    .map(|from| vec![(" > ", from)]),
                _ => parse_date_range(value, "value")
                    .ok()
                    .and_then(|(start, end)| {
                        Some(vec![(" >= ", day(start)?), (" <= ", day(end)?)])
                    }),
            };
            let Some(bounds) = bounds else {
                query.push("0 = 1");
                return;
            };
            push_field_value_exists(query, field_id, false, |query| {
                for (index, (comparison, bound)) in bounds.into_iter().enumerate() {
                    if index > 0 {
                        query.push(" AND ");
                    }
                    query.push(column).push(comparison).push_bind(bound);
                }
            });
        }
        (K::Select, O::Is | O::IsNot) | (K::MultiSelect, O::IncludesAny | O::Excludes) => {
            let negated = matches!(operator, O::IsNot | O::Excludes);
            let ids = live(value);
            if ids.is_empty() {
                query.push(if negated { "1 = 1" } else { "0 = 1" });
                return;
            }
            push_field_value_exists(query, field_id, negated, |query| {
                push_options(query, spec.kind, column, &ids, false);
            });
        }
        (K::MultiSelect, O::IncludesAll) => {
            let ids = live(value);
            let requested = values(value, ctx, false);
            if ids.is_empty() || ids.len() != requested.len() {
                // a removed option is on no task
                query.push("0 = 1");
                return;
            }
            push_field_value_exists(query, field_id, false, |query| {
                push_options(query, spec.kind, column, &ids, true);
            });
        }
        (K::Checkbox, O::Is) => {
            let checked = value.as_bool().unwrap_or(false);
            push_field_value_exists(query, field_id, !checked, |query| {
                query.push("1 = 1");
            });
        }
        _ => {
            query.push("0 = 1");
        }
    }
}

/// The SQL type of a custom field's sort key ([`push_custom_sort_key`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortKeyType {
    Real,
    Integer,
    Text,
}

pub fn custom_sort_key_type(spec: Option<&CustomFieldSpec>) -> SortKeyType {
    match spec.map(|spec| spec.kind) {
        Some(TaskFieldKind::Number) => SortKeyType::Real,
        Some(TaskFieldKind::Date | TaskFieldKind::Text | TaskFieldKind::Url) => SortKeyType::Text,
        Some(
            TaskFieldKind::Select
            | TaskFieldKind::MultiSelect
            | TaskFieldKind::Checkbox
            | TaskFieldKind::TimeTracking,
        )
        | None => SortKeyType::Integer,
    }
}

/// The outer task's sort key for a field, NULL without a value: the number; the quoted
/// `"YYYY-MM-DD"` date; the lower-cased text; the position of the first (in the field's order)
/// live option; 1 checked / 0 unchecked. A deleted field (`spec` None) is NULL for every task.
pub fn push_custom_sort_key(
    query: &mut SqlBuilder,
    field_id: Id,
    spec: Option<&CustomFieldSpec>,
    backend: Backend,
) {
    let Some(spec) = spec else {
        query.push("CAST(NULL AS BIGINT)");
        return;
    };
    let from = "FROM task_field_values AS sort_value \
         WHERE sort_value.task_id = tasks.id AND sort_value.field_id = ";
    let column = "sort_value.value_json";
    match spec.kind {
        TaskFieldKind::Checkbox => {
            query
                .push("CAST(CASE WHEN EXISTS (SELECT 1 ")
                .push(from)
                .push_bind(field_id.to_string())
                .push(") THEN 1 ELSE 0 END AS BIGINT)");
        }
        TaskFieldKind::TimeTracking => {
            query.push("CAST(NULL AS BIGINT)");
        }
        TaskFieldKind::Select | TaskFieldKind::MultiSelect if spec.option_ids.is_empty() => {
            query.push("CAST(NULL AS BIGINT)");
        }
        TaskFieldKind::Select | TaskFieldKind::MultiSelect => {
            query.push("(SELECT CAST(CASE");
            for (position, option_id) in spec.option_ids.iter().enumerate() {
                query.push(" WHEN ");
                push_option_match(query, spec.kind, column, option_id);
                query.push(" THEN ").push(&position.to_string());
            }
            query
                .push(" END AS BIGINT) ")
                .push(from)
                .push_bind(field_id.to_string())
                .push(")");
        }
        TaskFieldKind::Number => {
            query
                .push("(SELECT CAST(sort_value.value_json AS DOUBLE PRECISION) ")
                .push(from)
                .push_bind(field_id.to_string())
                .push(")");
        }
        TaskFieldKind::Date => {
            query
                .push("(SELECT sort_value.value_json ")
                .push(from)
                .push_bind(field_id.to_string())
                .push(")");
        }
        TaskFieldKind::Text | TaskFieldKind::Url => {
            query
                .push("(SELECT lower(")
                .push(&json_text_sql(backend, column))
                .push(") ")
                .push(from)
                .push_bind(field_id.to_string())
                .push(")");
        }
    }
}

pub(crate) fn escape_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// A canonical lowercase UUIDv7.
    const ID: &str = "0190f5b4-7c1e-7a3b-8c4d-5e6f7a8b9c0d";

    fn condition(field: &str, operator: &str, value: Value) -> Value {
        json!({ "field": field, "operator": operator, "value": value })
    }

    fn tree(children: Vec<Value>) -> Value {
        json!({ "op": "and", "children": children })
    }

    fn path_of(value: Value) -> String {
        parse_filter(&value).unwrap_err().path
    }

    #[test]
    fn serde_tells_groups_from_conditions() {
        let group: FilterGroup = serde_json::from_value(json!({
            "op": "or",
            "children": [
                { "op": "and", "children": [] },
                { "field": "priority", "operator": "is", "value": ["high"] },
                { "field": "assignee", "operator": "is_empty" }
            ]
        }))
        .unwrap();
        assert_eq!(group.op, GroupOp::Or);
        assert!(matches!(group.children[0], FilterNode::Group(_)));
        assert_eq!(
            group.children[1],
            FilterNode::Condition(Condition {
                field: FilterField::Priority,
                field_id: None,
                operator: FilterOperator::Is,
                value: json!(["high"]),
            })
        );
        assert_eq!(
            group.children[2],
            FilterNode::Condition(Condition {
                field: FilterField::Assignee,
                field_id: None,
                operator: FilterOperator::IsEmpty,
                value: Value::Null,
            })
        );
        assert_eq!(
            serde_json::to_value(&group.children[2]).unwrap(),
            json!({ "field": "assignee", "operator": "is_empty" })
        );
        for bad in [
            json!({ "op": "and", "children": [], "field": "priority" }),
            json!({ "field": "priority", "operator": "is", "value": ["high"], "extra": 1 }),
            json!({ "field": "story_points", "operator": "is", "value": ["1"] }),
        ] {
            assert!(
                serde_json::from_value::<FilterNode>(bad.clone()).is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn parse_filter_reports_the_path_of_the_first_bad_node() {
        let ok = condition("priority", "is", json!(["high"]));
        assert_eq!(
            path_of(tree(vec![
                ok.clone(),
                ok.clone(),
                condition("priority", "is", json!(["critical"])),
            ])),
            "filter.children[2].value[0]"
        );
        assert_eq!(
            path_of(tree(vec![condition("priority", "is", json!("high"))])),
            "filter.children[0].value"
        );
        assert_eq!(
            path_of(tree(vec![condition("story_points", "is", json!(["1"]))])),
            "filter.children[0].field"
        );
        assert_eq!(
            path_of(tree(vec![condition("priority", "contains", json!("x"))])),
            "filter.children[0].operator"
        );
        assert_eq!(
            path_of(tree(vec![condition("priority", "sounds_like", json!("x"))])),
            "filter.children[0].operator"
        );
        assert_eq!(
            path_of(json!({ "op": "and", "children": [], "mode": "all" })),
            "filter.mode"
        );
        assert_eq!(path_of(json!({ "op": "xor", "children": [] })), "filter.op");
        assert_eq!(path_of(json!({ "op": "and" })), "filter.children");
        assert_eq!(path_of(json!(["not", "a", "group"])), "filter");
        assert_eq!(
            path_of(tree(vec![
                ok.clone(),
                json!({ "op": "or", "children": [condition("project", "is", json!(["nope"]))] }),
            ])),
            "filter.children[1].children[0].value[0]"
        );
        assert_eq!(
            path_of(tree(vec![
                json!({ "field": "text", "operator": "contains", "value": "x", "negate": true })
            ])),
            "filter.children[0].negate"
        );
    }

    #[test]
    fn limits_are_enforced() {
        let leaf = condition("priority", "is", json!(["high"]));
        let nest = |levels: usize| {
            let mut node = tree(vec![leaf.clone()]);
            for _ in 0..levels {
                node = tree(vec![node]);
            }
            node
        };
        assert!(parse_filter(&nest(MAX_NESTING)).is_ok());
        assert_eq!(
            path_of(nest(MAX_NESTING + 1)),
            "filter.children[0].children[0].children[0].children[0]"
        );

        assert!(parse_filter(&tree(vec![leaf.clone(); MAX_CONDITIONS])).is_ok());
        assert_eq!(
            path_of(tree(vec![leaf.clone(); MAX_CONDITIONS + 1])),
            "filter.children[50]"
        );
        // Conditions are counted across the whole tree.
        assert_eq!(
            path_of(tree(vec![
                tree(vec![leaf.clone(); 30]),
                tree(vec![leaf.clone(); 21]),
            ])),
            "filter.children[1].children[20]"
        );

        let ids: Vec<String> = (0..MAX_VALUES).map(|_| Id::new_v7().to_string()).collect();
        assert!(parse_filter(&tree(vec![condition("label", "includes_any", json!(ids))])).is_ok());
        let mut too_many = ids.clone();
        too_many.push(Id::new_v7().to_string());
        assert_eq!(
            path_of(tree(vec![condition(
                "label",
                "includes_any",
                json!(too_many)
            )])),
            "filter.children[0].value"
        );
        assert_eq!(
            path_of(tree(vec![condition("label", "includes_any", json!([]))])),
            "filter.children[0].value"
        );

        assert!(
            parse_filter(&tree(vec![condition(
                "text",
                "contains",
                json!("é".repeat(MAX_TEXT_CHARS))
            )]))
            .is_ok()
        );
        assert_eq!(
            path_of(tree(vec![condition(
                "text",
                "contains",
                json!("a".repeat(MAX_TEXT_CHARS + 1))
            )])),
            "filter.children[0].value"
        );
        assert_eq!(
            path_of(tree(vec![condition("text", "contains", json!(""))])),
            "filter.children[0].value"
        );
    }

    #[test]
    fn values_must_fit_the_field_and_operator() {
        let valid = [
            condition("status", "is", json!(["started:in progress", ID])),
            condition(
                "status_category",
                "is_not",
                json!(["completed", "cancelled", "duplicate"]),
            ),
            condition("assignee", "is", json!(["me", ID])),
            condition("assignee", "is_empty", Value::Null),
            condition("creator", "is_not", json!(["me"])),
            condition("label", "includes_all", json!([ID])),
            condition("label", "is_not_empty", Value::Null),
            condition("priority", "is", json!(["none", "urgent"])),
            condition("project", "is", json!([ID])),
            condition(
                "due_date",
                "between",
                json!([{ "relative": "today" }, { "relative": "today", "offset_days": 6 }]),
            ),
            condition("due_date", "before", json!({ "absolute": "2024-02-29" })),
            condition(
                "created_at",
                "after",
                json!({ "relative": "start_of_week", "offset_days": -7 }),
            ),
            condition("updated_at", "before", json!({ "relative": "end_of_week" })),
            condition("text", "contains", json!("  spaced  ")),
            condition("parent", "is", json!(["none", ID])),
            condition("parent", "is_not", json!([ID])),
            condition("sub_issues", "is", json!(["has", "none"])),
        ];
        for node in valid {
            assert!(parse_filter(&tree(vec![node.clone()])).is_ok(), "{node}");
        }

        for (node, path) in [
            (
                condition("status", "is", json!(["done"])),
                "filter.children[0].value[0]",
            ),
            (
                condition("status", "is", json!(["archived:old"])),
                "filter.children[0].value[0]",
            ),
            (
                condition("status", "is", json!(["started: "])),
                "filter.children[0].value[0]",
            ),
            (
                condition("status_category", "is", json!(["open"])),
                "filter.children[0].value[0]",
            ),
            (
                condition("project", "is", json!(["me"])),
                "filter.children[0].value[0]",
            ),
            (
                condition(
                    "project",
                    "is",
                    json!(["0190F5B4-7C1E-7A3B-8C4D-5E6F7A8B9C0D"]),
                ),
                "filter.children[0].value[0]",
            ),
            (
                condition("label", "includes_any", json!(["  "])),
                "filter.children[0].value[0]",
            ),
            (
                condition("assignee", "is", json!([42])),
                "filter.children[0].value[0]",
            ),
            (
                condition("assignee", "is_empty", json!(["me"])),
                "filter.children[0].value",
            ),
            (
                condition("due_date", "before", json!({ "absolute": "2026-02-29" })),
                "filter.children[0].value.absolute",
            ),
            (
                condition("due_date", "before", json!({ "absolute": "2026-9-1" })),
                "filter.children[0].value.absolute",
            ),
            (
                condition("due_date", "before", json!({ "relative": "yesterday" })),
                "filter.children[0].value.relative",
            ),
            (
                condition(
                    "due_date",
                    "before",
                    json!({ "relative": "today", "offset_days": 1.5 }),
                ),
                "filter.children[0].value.offset_days",
            ),
            (
                condition(
                    "due_date",
                    "before",
                    json!({ "relative": "today", "offset_days": 4000 }),
                ),
                "filter.children[0].value.offset_days",
            ),
            (
                condition(
                    "due_date",
                    "before",
                    json!({ "relative": "today", "offset_days": i64::MIN }),
                ),
                "filter.children[0].value.offset_days",
            ),
            (
                condition(
                    "due_date",
                    "before",
                    json!({ "relative": "today", "at": "noon" }),
                ),
                "filter.children[0].value.at",
            ),
            (
                condition(
                    "due_date",
                    "before",
                    json!({ "absolute": "2026-09-01", "relative": "today" }),
                ),
                "filter.children[0].value.relative",
            ),
            (
                condition("due_date", "before", json!("2026-09-01")),
                "filter.children[0].value",
            ),
            (
                condition("due_date", "between", json!([{ "relative": "today" }])),
                "filter.children[0].value",
            ),
            (
                condition(
                    "due_date",
                    "between",
                    json!([{ "relative": "today" }, { "absolute": "tomorrow" }]),
                ),
                "filter.children[0].value[1].absolute",
            ),
            (
                condition("created_at", "is_empty", Value::Null),
                "filter.children[0].operator",
            ),
            (
                condition("text", "contains", json!(["x"])),
                "filter.children[0].value",
            ),
            (
                condition("parent", "is", json!(["me"])),
                "filter.children[0].value[0]",
            ),
            (
                condition("parent", "is_empty", Value::Null),
                "filter.children[0].operator",
            ),
            (
                condition("sub_issues", "is_not", json!(["has"])),
                "filter.children[0].operator",
            ),
            (
                condition("sub_issues", "is", json!(["some"])),
                "filter.children[0].value[0]",
            ),
        ] {
            assert_eq!(path_of(tree(vec![node.clone()])), path, "{node}");
        }
    }

    #[test]
    fn view_state_round_trips_and_validates_display_rules() {
        let state = ViewState::default();
        let text = serde_json::to_string(&state).unwrap();
        assert_eq!(parse_view_state(&text).unwrap(), state);
        assert_eq!(
            serde_json::to_value(&state.display).unwrap(),
            json!({
                "layout": "list",
                "group_by": "status",
                "sub_group_by": "none",
                "order_by": "manual",
                "order_direction": "asc",
                "properties": ["id", "status", "assignee", "priority", "project", "due_date", "labels", "sub_issue_progress"],
                "show_completed": "all",
                "show_empty_groups": false,
                "sub_issues": "nested"
            })
        );

        let with_display = |patch: Value| {
            let mut value = serde_json::to_value(ViewState::default()).unwrap();
            for (key, item) in patch.as_object().unwrap() {
                value["display"][key.as_str()] = item.clone();
            }
            value
        };
        let error = |value: Value| parse_view_state(&value.to_string()).unwrap_err().path;
        assert_eq!(
            error(with_display(
                json!({ "group_by": "priority", "sub_group_by": "priority" })
            )),
            "display.sub_group_by"
        );
        assert!(
            parse_view_state(
                &with_display(json!({ "group_by": "none", "sub_group_by": "none" })).to_string()
            )
            .is_ok()
        );
        assert!(
            parse_view_state(
                &with_display(json!({ "layout": "board", "group_by": "none" })).to_string()
            )
            .is_ok()
        );
        assert_eq!(
            error(with_display(
                json!({ "properties": ["id", "status", "id"] })
            )),
            "display.properties[2]"
        );
        assert_eq!(
            error(with_display(json!({ "layout": "gallery" }))),
            "display.layout"
        );
        assert_eq!(
            error(with_display(json!({ "density": "compact" }))),
            "display.density"
        );
        let mut missing = serde_json::to_value(ViewState::default()).unwrap();
        missing["display"]
            .as_object_mut()
            .unwrap()
            .remove("order_by");
        assert_eq!(error(missing), "display.order_by");
        let mut extra = serde_json::to_value(ViewState::default()).unwrap();
        extra["sort"] = json!("title");
        assert_eq!(error(extra), "sort");
        assert_eq!(
            error(json!({
                "filter": { "op": "and", "children": [condition("priority", "is", json!(["meh"]))] },
                "display": ViewState::default().display
            })),
            "filter.children[0].value[0]"
        );
        assert_eq!(parse_view_state("{not json").unwrap_err().path, "");

        let mut typed = ViewState::default();
        typed.display.sub_group_by = GroupBy::Status;
        assert_eq!(
            validate_view_state(&typed).unwrap_err().path,
            "display.sub_group_by"
        );

        let mut legacy = serde_json::to_value(ViewState::default()).unwrap();
        legacy["display"]
            .as_object_mut()
            .unwrap()
            .remove("sub_issues");
        assert_eq!(
            parse_view_state(&legacy.to_string())
                .unwrap()
                .display
                .sub_issues,
            SubIssuesDisplay::Nested
        );
        assert_eq!(
            error(with_display(json!({ "sub_issues": "tree" }))),
            "display.sub_issues"
        );
    }

    #[test]
    fn presets_are_valid_fixed_trees() {
        for name in ["mine", "overdue", "due_soon", "current_week", "my_week"] {
            let preset = preset_filter(name).unwrap();
            assert_eq!(preset.op, GroupOp::And);
            validate_filter(&preset).unwrap();
        }
        assert_eq!(preset_filter("someday"), None);
        let not_done = json!({
            "field": "status_category",
            "operator": "is_not",
            "value": ["completed", "cancelled", "duplicate"]
        });
        let this_week = json!({
            "field": "due_date",
            "operator": "between",
            "value": [{ "relative": "start_of_week" }, { "relative": "end_of_week" }]
        });
        let mine = json!({ "field": "assignee", "operator": "is", "value": ["me"] });
        for (name, children) in [
            ("mine", json!([mine.clone()])),
            (
                "overdue",
                json!([
                    { "field": "due_date", "operator": "before", "value": { "relative": "today" } },
                    not_done.clone()
                ]),
            ),
            (
                "due_soon",
                json!([
                    {
                        "field": "due_date",
                        "operator": "between",
                        "value": [{ "relative": "today" }, { "relative": "today", "offset_days": 6 }]
                    },
                    not_done.clone()
                ]),
            ),
            ("current_week", json!([this_week.clone(), not_done.clone()])),
            (
                "my_week",
                json!([this_week.clone(), not_done.clone(), mine.clone()]),
            ),
        ] {
            assert_eq!(
                serde_json::to_value(preset_filter(name).unwrap()).unwrap(),
                json!({ "op": "and", "children": children }),
                "{name}"
            );
        }
    }

    #[test]
    fn and_groups_flattens_and_roots_and_keeps_or_groups() {
        let leaf = |priority: &str| {
            FilterNode::Condition(Condition {
                field: FilterField::Priority,
                field_id: None,
                operator: FilterOperator::Is,
                value: json!([priority]),
            })
        };
        let or_group = FilterGroup {
            op: GroupOp::Or,
            children: vec![leaf("low"), leaf("high")],
        };
        let combined = and_groups(vec![
            preset_filter("mine").unwrap(),
            FilterGroup {
                op: GroupOp::And,
                children: vec![leaf("urgent")],
            },
            FilterGroup {
                op: GroupOp::Or,
                children: vec![],
            },
            or_group.clone(),
        ]);
        assert_eq!(combined.op, GroupOp::And);
        assert_eq!(combined.children.len(), 3);
        assert_eq!(combined.children[1], leaf("urgent"));
        assert_eq!(combined.children[2], FilterNode::Group(or_group));
        validate_filter(&combined).unwrap();
        assert_eq!(and_groups(vec![]), FilterGroup::default());
    }

    #[test]
    fn relative_days_resolve_in_utc_with_monday_weeks() {
        const DAY: i64 = 86_400_000;
        // 2026-09-24 is day 20_720 since the Unix epoch, a Thursday.
        let thursday_last_ms = TimestampMillis::from_millis(20_721 * DAY - 1);
        let friday_first_ms = TimestampMillis::from_millis(20_721 * DAY);
        let today = DateValue::Relative {
            anchor: RelativeDay::Today,
            offset_days: 0,
        };
        assert_eq!(resolve_day(today, thursday_last_ms), 20_720);
        assert_eq!(resolve_day(today, friday_first_ms), 20_721);
        assert_eq!(
            resolve_day(
                DateValue::Relative {
                    anchor: RelativeDay::Today,
                    offset_days: -3
                },
                thursday_last_ms
            ),
            20_717
        );

        let start = DateValue::Relative {
            anchor: RelativeDay::StartOfWeek,
            offset_days: 0,
        };
        let end = DateValue::Relative {
            anchor: RelativeDay::EndOfWeek,
            offset_days: 0,
        };
        let sunday_last_ms = TimestampMillis::from_millis(20_724 * DAY - 1);
        let monday_first_ms = TimestampMillis::from_millis(20_724 * DAY);
        assert_eq!(resolve_day(start, sunday_last_ms), 20_717);
        assert_eq!(resolve_day(end, sunday_last_ms), 20_723);
        assert_eq!(resolve_day(start, monday_first_ms), 20_724);
        assert_eq!(resolve_day(end, monday_first_ms), 20_730);

        for (text, day) in [
            ("1970-01-01", 0),
            ("2024-02-29", 19_782),
            ("2026-09-24", 20_720),
        ] {
            assert_eq!(
                parse_date_value(&json!({ "absolute": text }), "value").unwrap(),
                DateValue::Absolute { day },
                "{text}"
            );
            assert_eq!(
                resolve_day(DateValue::Absolute { day }, monday_first_ms),
                day
            );
        }
    }
}

#[cfg(test)]
mod custom_field_tests {
    use serde_json::json;

    use super::*;

    const FIELD: &str = "0190f5b4-7c1e-7a3b-8c4d-5e6f7a8b9c0d";

    fn display(extra: Value) -> Value {
        let mut display = serde_json::to_value(DisplayOptions::default()).unwrap();
        for (key, value) in extra.as_object().unwrap() {
            display[key] = value.clone();
        }
        json!({ "filter": { "op": "and", "children": [] }, "display": display })
    }

    #[test]
    fn views_remember_shown_fields_and_the_field_sort() {
        let state = display(json!({
            "custom_fields": [FIELD], "order_by": "custom_field", "order_field_id": FIELD
        }));
        let parsed = parse_view_state_value(&state).unwrap();
        assert_eq!(
            parsed.display.custom_fields,
            vec![FIELD.parse::<Id>().unwrap()]
        );
        assert_eq!(serde_json::to_value(&parsed).unwrap(), state);
        // older states without the keys still parse, and serialize without them
        let plain = parse_view_state_value(&display(json!({}))).unwrap();
        assert!(
            !serde_json::to_string(&plain)
                .unwrap()
                .contains("custom_fields")
        );
        let error = |extra: Value| parse_view_state_value(&display(extra)).unwrap_err().path;
        assert_eq!(
            error(json!({ "order_by": "custom_field" })),
            "display.order_field_id"
        );
        assert_eq!(
            error(json!({ "order_field_id": FIELD })),
            "display.order_field_id"
        );
        assert_eq!(
            error(json!({ "custom_fields": [FIELD, FIELD] })),
            "display.custom_fields[1]"
        );
        assert_eq!(
            error(json!({ "custom_fields": ["x"] })),
            "display.custom_fields"
        );
    }

    #[test]
    fn custom_conditions_need_a_shape_of_some_kind_until_the_kind_is_known() {
        let condition = |operator: &str, value: Value| {
            json!({ "op": "and", "children": [
                { "field": "custom_field", "field_id": FIELD, "operator": operator, "value": value }
            ] })
        };
        assert!(parse_filter(&condition("is", json!(true))).is_ok());
        assert!(parse_filter(&condition("is", json!(3))).is_ok());
        assert!(parse_filter(&condition("is", json!([FIELD]))).is_ok());
        assert!(parse_filter(&condition("between", json!([1, 2]))).is_ok());
        assert!(
            parse_filter(&condition(
                "between",
                json!([{ "absolute": "2026-01-01" }, { "relative": "today" }])
            ))
            .is_ok()
        );
        assert!(parse_filter(&condition("is", json!("x"))).is_err());
        assert!(parse_filter(&condition("less_than", json!("3"))).is_err());
        let group = parse_filter(&condition("is", json!(3))).unwrap();
        let id: Id = FIELD.parse().unwrap();
        assert_eq!(
            custom_field_ids(&group).into_iter().collect::<Vec<_>>(),
            vec![id]
        );
        let fields = CustomFields::from([(
            id,
            CustomFieldSpec {
                kind: TaskFieldKind::Checkbox,
                option_ids: Vec::new(),
            },
        )]);
        assert_eq!(
            validate_custom_fields(&group, &fields).unwrap_err().path,
            "filter.children[0].value"
        );
        assert!(validate_custom_fields(&group, &CustomFields::new()).is_ok());
    }

    #[test]
    fn days_format_as_iso_dates() {
        assert_eq!(ymd(0).as_deref(), Some("1970-01-01"));
        assert_eq!(
            parse_ymd("2026-02-28").and_then(ymd).as_deref(),
            Some("2026-02-28")
        );
    }
}

#[cfg(test)]
mod sql_tests {
    use orbit_platform::SqlBuilder;
    use serde_json::json;

    use super::*;

    fn compile(tree: Value) -> String {
        let group = parse_filter(&tree).unwrap();
        let mut query = SqlBuilder::new("SELECT tasks.id FROM tasks WHERE 1 = 1");
        push_filter(
            &mut query,
            &group,
            &FilterContext {
                scope: ProjectScope::All,
                actor_id: Id::new_v7(),
                now: TimestampMillis::from_millis(0),
                backend: Backend::Sqlite,
                custom_fields: &CustomFields::new(),
            },
        );
        query.sql().as_str().to_owned()
    }

    #[test]
    fn values_are_bound_never_interpolated() {
        let sql = compile(json!({ "op": "and", "children": [
            { "field": "text", "operator": "contains", "value": "x' OR 1=1 --" },
            { "field": "status", "operator": "is", "value": ["started:it's"] }
        ] }));
        assert!(!sql.contains("1=1"), "{sql}");
        assert!(!sql.contains("it's"), "{sql}");
        assert_eq!(sql.matches('$').count(), 3, "{sql}");
    }

    #[test]
    fn empty_groups_match_everything_and_children_are_parenthesised() {
        assert_eq!(
            compile(json!({ "op": "and", "children": [] })),
            "SELECT tasks.id FROM tasks WHERE 1 = 1 AND (1 = 1)"
        );
        assert_eq!(
            compile(json!({ "op": "or", "children": [
                { "field": "priority", "operator": "is", "value": ["high"] },
                { "op": "and", "children": [] }
            ] })),
            "SELECT tasks.id FROM tasks WHERE 1 = 1 AND ((tasks.priority IN ($1)) OR (1 = 1))"
        );
    }

    #[test]
    fn show_completed_all_adds_nothing() {
        let mut query = SqlBuilder::new("SELECT 1");
        push_show_completed(
            &mut query,
            ShowCompleted::All,
            TimestampMillis::from_millis(0),
        );
        assert_eq!(query.sql().as_str(), "SELECT 1");
        push_show_completed(
            &mut query,
            ShowCompleted::PastWeek,
            TimestampMillis::from_millis(0),
        );
        assert!(
            query.sql().as_str().contains("tasks.completed_at >= $1"),
            "{}",
            query.sql().as_str()
        );
    }

    #[test]
    fn parent_filters_never_compare_null() {
        let sql = compile(json!({ "op": "and", "children": [
            { "field": "parent", "operator": "is_not", "value": ["0190f5b4-7c1e-7a3b-8c4d-5e6f7a8b9c0d", "none"] }
        ] }));
        let visible = sub_issues::has_visible_parent_sql(ProjectScope::All, "tasks.parent_task_id");
        assert!(
            sql.contains(&format!(
                "NOT (NOT {visible} OR ({visible} AND tasks.parent_task_id IN ($1)))"
            )),
            "{sql}"
        );
    }
}
