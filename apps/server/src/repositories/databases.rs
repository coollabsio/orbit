//! Docs databases (P1): a page of kind `database` holds typed properties, saved table views and
//! rows. A row is a page whose parent is the database page, so rows get co-editing, comments,
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
use super::pages::{PageError, PageKind, PageRecord, page_in_tx, renumber, sibling_ids};
use super::tasks::{TaskError, require_access_tx};
use crate::audit::{self, AuditOutcome};
use crate::billing::TrialLimit;
use crate::collab::CollabHub;

pub const MAX_ROWS: i64 = 5_000;
pub const MAX_PROPERTIES: usize = 100;
pub const MAX_VIEWS: usize = 50;
pub const MAX_OPTIONS: usize = 200;
pub const MAX_CELL_ITEMS: usize = 100;
pub const MAX_SORTS: usize = 10;
pub const MAX_RULES: usize = 20;
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
#[schema(as = DatabasePropertyType)]
pub enum PropertyType {
    Title,
    Text,
    Number,
    Select,
    MultiSelect,
    Status,
    Date,
    Person,
    Checkbox,
    Url,
    CreatedTime,
    CreatedBy,
    LastEditedTime,
    LastEditedBy,
}

impl PropertyType {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Title => "title",
            Self::Text => "text",
            Self::Number => "number",
            Self::Select => "select",
            Self::MultiSelect => "multi_select",
            Self::Status => "status",
            Self::Date => "date",
            Self::Person => "person",
            Self::Checkbox => "checkbox",
            Self::Url => "url",
            Self::CreatedTime => "created_time",
            Self::CreatedBy => "created_by",
            Self::LastEditedTime => "last_edited_time",
            Self::LastEditedBy => "last_edited_by",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        serde_json::from_value(Value::String(value.to_owned())).ok()
    }

    /// Read from the row's page, never stored as a value (and never set through values).
    const fn computed(self) -> bool {
        matches!(
            self,
            Self::Title
                | Self::CreatedTime
                | Self::CreatedBy
                | Self::LastEditedTime
                | Self::LastEditedBy
        )
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum OptionColor {
    #[default]
    Default,
    Gray,
    Brown,
    Orange,
    Yellow,
    Green,
    Blue,
    Purple,
    Pink,
    Red,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum StatusGroup {
    #[default]
    Todo,
    InProgress,
    Done,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NumberFormat {
    #[default]
    Number,
    NumberWithCommas,
    Percent,
    Currency,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DateFormat {
    Relative,
    #[default]
    Short,
    Iso,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum TimeFormat {
    #[serde(rename = "12h")]
    TwelveHour,
    #[default]
    #[serde(rename = "24h")]
    TwentyFourHour,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NumberConfig {
    #[serde(default)]
    pub format: NumberFormat,
    /// ISO 4217 code, for the `currency` format.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DateConfig {
    #[serde(default)]
    pub date_format: DateFormat,
    #[serde(default)]
    pub time_format: TimeFormat,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SelectOption {
    pub id: String,
    pub name: String,
    pub color: OptionColor,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StatusOption {
    pub id: String,
    pub name: String,
    pub color: OptionColor,
    pub group: StatusGroup,
}

/// A property's type with its config: a number cannot carry options, a select cannot carry a
/// number format.
#[derive(Clone, Debug, PartialEq)]
pub enum PropertyKind {
    Title,
    Text,
    Number(NumberConfig),
    Select(Vec<SelectOption>),
    MultiSelect(Vec<SelectOption>),
    /// Never empty, with at least one `todo` option (the value of a row with no value).
    Status(Vec<StatusOption>),
    Date(DateConfig),
    Person,
    Checkbox,
    Url,
    CreatedTime,
    CreatedBy,
    LastEditedTime,
    LastEditedBy,
}

/// An option as clients send it: a missing id gets a new one.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OptionInput {
    id: Option<String>,
    name: String,
    #[serde(default)]
    color: OptionColor,
    group: Option<StatusGroup>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OptionsInput {
    #[serde(default)]
    options: Vec<OptionInput>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyConfig {}

impl PropertyKind {
    #[must_use]
    pub fn property_type(&self) -> PropertyType {
        match self {
            Self::Title => PropertyType::Title,
            Self::Text => PropertyType::Text,
            Self::Number(_) => PropertyType::Number,
            Self::Select(_) => PropertyType::Select,
            Self::MultiSelect(_) => PropertyType::MultiSelect,
            Self::Status(_) => PropertyType::Status,
            Self::Date(_) => PropertyType::Date,
            Self::Person => PropertyType::Person,
            Self::Checkbox => PropertyType::Checkbox,
            Self::Url => PropertyType::Url,
            Self::CreatedTime => PropertyType::CreatedTime,
            Self::CreatedBy => PropertyType::CreatedBy,
            Self::LastEditedTime => PropertyType::LastEditedTime,
            Self::LastEditedBy => PropertyType::LastEditedBy,
        }
    }

    /// The config a new property of `property_type` gets when the client sends none.
    #[must_use]
    pub fn default_for(property_type: PropertyType) -> Self {
        match property_type {
            PropertyType::Title => Self::Title,
            PropertyType::Text => Self::Text,
            PropertyType::Number => Self::Number(NumberConfig::default()),
            PropertyType::Select => Self::Select(Vec::new()),
            PropertyType::MultiSelect => Self::MultiSelect(Vec::new()),
            PropertyType::Status => Self::Status(default_status_options()),
            PropertyType::Date => Self::Date(DateConfig::default()),
            PropertyType::Person => Self::Person,
            PropertyType::Checkbox => Self::Checkbox,
            PropertyType::Url => Self::Url,
            PropertyType::CreatedTime => Self::CreatedTime,
            PropertyType::CreatedBy => Self::CreatedBy,
            PropertyType::LastEditedTime => Self::LastEditedTime,
            PropertyType::LastEditedBy => Self::LastEditedBy,
        }
    }

    /// Parses `config` for `property_type`; `None` gives the default. Errors name the field path.
    pub fn parse(property_type: PropertyType, config: Option<&Value>) -> Result<Self, String> {
        let Some(config) = config else {
            return Ok(Self::default_for(property_type));
        };
        let invalid = |_| "config".to_owned();
        Ok(match property_type {
            PropertyType::Number => {
                let config: NumberConfig =
                    serde_json::from_value(config.clone()).map_err(invalid)?;
                if config.currency.as_ref().is_some_and(|code| {
                    code.len() != 3 || !code.bytes().all(|byte| byte.is_ascii_uppercase())
                }) {
                    return Err("config.currency".to_owned());
                }
                Self::Number(config)
            }
            PropertyType::Date => {
                Self::Date(serde_json::from_value(config.clone()).map_err(invalid)?)
            }
            PropertyType::Select | PropertyType::MultiSelect | PropertyType::Status => {
                let input: OptionsInput =
                    serde_json::from_value(config.clone()).map_err(invalid)?;
                let options = parse_options(input.options, property_type == PropertyType::Status)?;
                match property_type {
                    PropertyType::Select => Self::Select(select_options(options)),
                    PropertyType::MultiSelect => Self::MultiSelect(select_options(options)),
                    _ => {
                        if !options
                            .iter()
                            .any(|option| option.group == StatusGroup::Todo)
                        {
                            return Err("config.options".to_owned());
                        }
                        Self::Status(options)
                    }
                }
            }
            _ => {
                let _: EmptyConfig = serde_json::from_value(config.clone()).map_err(invalid)?;
                Self::default_for(property_type)
            }
        })
    }

    #[must_use]
    pub fn config_json(&self) -> Value {
        let options = |options: &[SelectOption]| json!({ "options": options });
        match self {
            Self::Number(config) => json!(config),
            Self::Date(config) => json!(config),
            Self::Select(list) | Self::MultiSelect(list) => options(list),
            Self::Status(list) => json!({ "options": list }),
            _ => json!({}),
        }
    }

    /// `(id, name)` of each option, in order; empty for types without options.
    fn options(&self) -> Vec<(&str, &str)> {
        match self {
            Self::Select(list) | Self::MultiSelect(list) => list
                .iter()
                .map(|option| (option.id.as_str(), option.name.as_str()))
                .collect(),
            Self::Status(list) => list
                .iter()
                .map(|option| (option.id.as_str(), option.name.as_str()))
                .collect(),
            _ => Vec::new(),
        }
    }

    fn has_option(&self, id: &str) -> bool {
        self.options().iter().any(|(option, _)| *option == id)
    }

    /// The same kind with a new id for each option, and the map of old id → new id.
    fn with_new_option_ids(&self) -> (Self, HashMap<String, String>) {
        let mut ids = HashMap::new();
        let mut fresh = |id: &str| {
            let next = Id::new_v7().to_string();
            ids.insert(id.to_owned(), next.clone());
            next
        };
        let mut select = |option: &SelectOption| SelectOption {
            id: fresh(&option.id),
            ..option.clone()
        };
        let kind = match self {
            Self::Select(list) => Self::Select(list.iter().map(&mut select).collect()),
            Self::MultiSelect(list) => Self::MultiSelect(list.iter().map(&mut select).collect()),
            Self::Status(list) => Self::Status(
                list.iter()
                    .map(|option| StatusOption {
                        id: fresh(&option.id),
                        ..option.clone()
                    })
                    .collect(),
            ),
            other => other.clone(),
        };
        (kind, ids)
    }
}

/// "<name> (copy)", within the name limit.
fn copy_name(name: &str) -> String {
    const SUFFIX: &str = " (copy)";
    let kept: String = name
        .chars()
        .take(MAX_NAME_CHARS - SUFFIX.chars().count())
        .collect();
    format!("{kept}{SUFFIX}")
}

fn default_status_options() -> Vec<StatusOption> {
    [
        ("Not started", OptionColor::Default, StatusGroup::Todo),
        ("In progress", OptionColor::Blue, StatusGroup::InProgress),
        ("Done", OptionColor::Green, StatusGroup::Done),
    ]
    .into_iter()
    .map(|(name, color, group)| StatusOption {
        id: Id::new_v7().to_string(),
        name: name.to_owned(),
        color,
        group,
    })
    .collect()
}

fn select_options(options: Vec<StatusOption>) -> Vec<SelectOption> {
    options
        .into_iter()
        .map(|option| SelectOption {
            id: option.id,
            name: option.name,
            color: option.color,
        })
        .collect()
}

/// Validates options (ids unique and short, names 1–200 characters, at most 200). `group` is only
/// allowed for a status; without one an option is `todo`.
fn parse_options(input: Vec<OptionInput>, status: bool) -> Result<Vec<StatusOption>, String> {
    if input.len() > MAX_OPTIONS {
        return Err("config.options".to_owned());
    }
    let mut seen = HashSet::new();
    input
        .into_iter()
        .enumerate()
        .map(|(index, option)| {
            let path = |field: &str| format!("config.options[{index}].{field}");
            let id = option.id.unwrap_or_else(|| Id::new_v7().to_string());
            if id.is_empty()
                || id.chars().count() > MAX_OPTION_ID_CHARS
                || !id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
                || !seen.insert(id.clone())
            {
                return Err(path("id"));
            }
            let name = option.name.trim().to_owned();
            if name.is_empty() || name.chars().count() > MAX_NAME_CHARS {
                return Err(path("name"));
            }
            if option.group.is_some() && !status {
                return Err(path("group"));
            }
            Ok(StatusOption {
                id,
                name,
                color: option.color,
                group: option.group.unwrap_or_default(),
            })
        })
        .collect()
}

/// A date (`2026-11-09`) or a UTC time (`2026-11-09T14:00:00Z`), stored normalized.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum DatePoint {
    Day(String),
    Moment(String),
}

impl DatePoint {
    /// Accepts `YYYY-MM-DD` and `YYYY-MM-DDTHH:MM[:SS[.fraction]]Z`.
    fn parse(value: &str) -> Option<Self> {
        let (day, time) = match value.split_once('T') {
            Some((day, time)) => (day, Some(time)),
            None => (value, None),
        };
        valid_day(day)?;
        let Some(time) = time else {
            return Some(Self::Day(day.to_owned()));
        };
        let time = time.strip_suffix('Z')?;
        let (time, fraction) = time.split_once('.').unwrap_or((time, "0"));
        if fraction.is_empty() || !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        let parts: Vec<&str> = time.split(':').collect();
        let number = |part: &str, max: u32| {
            (part.len() == 2 && part.bytes().all(|byte| byte.is_ascii_digit()))
                .then(|| part.parse::<u32>().ok())
                .flatten()
                .filter(|value| *value <= max)
        };
        let (hour, minute, second) = match parts.as_slice() {
            [hour, minute] => (number(hour, 23)?, number(minute, 59)?, 0),
            [hour, minute, second] => (number(hour, 23)?, number(minute, 59)?, number(second, 59)?),
            _ => return None,
        };
        Some(Self::Moment(format!(
            "{day}T{hour:02}:{minute:02}:{second:02}Z"
        )))
    }

    fn as_str(&self) -> &str {
        match self {
            Self::Day(value) | Self::Moment(value) => value,
        }
    }
}

fn valid_day(day: &str) -> Option<()> {
    let bytes = day.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    let digits = |range: std::ops::Range<usize>| {
        let part = &day[range];
        part.bytes()
            .all(|byte| byte.is_ascii_digit())
            .then(|| part.parse::<u32>().ok())
            .flatten()
    };
    let (year, month, date) = (digits(0..4)?, digits(5..7)?, digits(8..10)?);
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return None,
    };
    (1..=days).contains(&date).then_some(())
}

/// A date or a range; both ends are dates or both are times, and the end is not before the start.
#[derive(Clone, Debug, PartialEq)]
pub struct DateValue {
    pub start: DatePoint,
    pub end: Option<DatePoint>,
}

impl DateValue {
    fn new(start: DatePoint, end: Option<DatePoint>) -> Option<Self> {
        if let Some(end) = &end
            && (std::mem::discriminant(end) != std::mem::discriminant(&start) || *end < start)
        {
            return None;
        }
        Some(Self { start, end })
    }

    /// `start` or the ISO 8601 interval `start/end`.
    fn text(&self) -> String {
        match &self.end {
            Some(end) => format!("{}/{}", self.start.as_str(), end.as_str()),
            None => self.start.as_str().to_owned(),
        }
    }

    fn from_text(text: &str) -> Option<Self> {
        match text.split_once('/') {
            Some((start, end)) => Self::new(
                DatePoint::parse(start.trim())?,
                Some(DatePoint::parse(end.trim())?),
            ),
            None => Self::new(DatePoint::parse(text)?, None),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DateInput {
    start: String,
    #[serde(default)]
    end: Option<String>,
}

/// A stored (non-empty) value. Which variant a property holds follows from its kind.
#[derive(Clone, Debug, PartialEq)]
pub enum CellValue {
    Text(String),
    Number(f64),
    /// A `select` or `status` option id.
    Choice(String),
    /// `multi_select` option ids, unique, at most 100.
    Choices(Vec<String>),
    Date(DateValue),
    /// Workspace members, unique, at most 100.
    People(Vec<Id>),
    /// A ticked checkbox (an unticked one stores nothing).
    Checked,
    Url(String),
}

impl CellValue {
    #[must_use]
    pub fn to_json(&self) -> Value {
        match self {
            Self::Text(text) | Self::Choice(text) | Self::Url(text) => json!(text),
            Self::Number(number) => json!(number),
            Self::Choices(ids) => json!(ids),
            Self::Date(date) => json!({
                "start": date.start.as_str(),
                "end": date.end.as_ref().map(DatePoint::as_str),
            }),
            Self::People(ids) => json!(ids),
            Self::Checked => json!(true),
        }
    }

    /// The value with its option ids replaced by `ids` (old id → new id).
    fn with_option_ids(self, ids: &HashMap<String, String>) -> Self {
        let swap = |id: String| ids.get(&id).cloned().unwrap_or(id);
        match self {
            Self::Choice(id) => Self::Choice(swap(id)),
            Self::Choices(list) => Self::Choices(list.into_iter().map(swap).collect()),
            other => other,
        }
    }
}

/// Parses a value sent for (or stored under) a property of `kind`. `Ok(None)` clears the value:
/// `null`, `""`, `[]` and `false` are empty. `Err(())` is invalid input. Person ids are only
/// checked for shape here; membership is checked by the caller.
fn parse_value(kind: &PropertyKind, raw: &Value) -> Result<Option<CellValue>, ()> {
    if raw.is_null() {
        return Ok(None);
    }
    let unique_strings = |raw: &Value| -> Result<Vec<String>, ()> {
        let items = raw.as_array().ok_or(())?;
        if items.len() > MAX_CELL_ITEMS {
            return Err(());
        }
        let mut seen = HashSet::new();
        items
            .iter()
            .map(|item| {
                let item = item.as_str().ok_or(())?;
                seen.insert(item).then(|| item.to_owned()).ok_or(())
            })
            .collect()
    };
    let value = match kind {
        PropertyKind::Text => {
            let text = raw.as_str().ok_or(())?;
            if text.chars().count() > MAX_TEXT_CHARS {
                return Err(());
            }
            (!text.is_empty()).then(|| CellValue::Text(text.to_owned()))
        }
        PropertyKind::Number(_) => {
            let number = raw.as_f64().filter(|number| number.is_finite()).ok_or(())?;
            Some(CellValue::Number(number))
        }
        PropertyKind::Select(_) | PropertyKind::Status(_) => {
            let id = raw.as_str().ok_or(())?;
            if !kind.has_option(id) {
                return Err(());
            }
            Some(CellValue::Choice(id.to_owned()))
        }
        PropertyKind::MultiSelect(_) => {
            let ids = unique_strings(raw)?;
            if !ids.iter().all(|id| kind.has_option(id)) {
                return Err(());
            }
            (!ids.is_empty()).then_some(CellValue::Choices(ids))
        }
        PropertyKind::Date(_) => {
            let input: DateInput = serde_json::from_value(raw.clone()).map_err(|_| ())?;
            let start = DatePoint::parse(&input.start).ok_or(())?;
            let end = input
                .end
                .map(|end| DatePoint::parse(&end).ok_or(()))
                .transpose()?;
            Some(CellValue::Date(DateValue::new(start, end).ok_or(())?))
        }
        PropertyKind::Person => {
            let ids = unique_strings(raw)?
                .iter()
                .map(|id| id.parse::<Id>().map_err(|_| ()))
                .collect::<Result<Vec<_>, _>>()?;
            (!ids.is_empty()).then_some(CellValue::People(ids))
        }
        PropertyKind::Checkbox => raw.as_bool().ok_or(())?.then_some(CellValue::Checked),
        PropertyKind::Url => {
            let url = raw.as_str().ok_or(())?;
            if url.is_empty() {
                None
            } else if valid_url(url) {
                Some(CellValue::Url(url.to_owned()))
            } else {
                return Err(());
            }
        }
        PropertyKind::Title
        | PropertyKind::CreatedTime
        | PropertyKind::CreatedBy
        | PropertyKind::LastEditedTime
        | PropertyKind::LastEditedBy => return Err(()),
    };
    Ok(value)
}

/// Whether `raw` is a valid value for a property of `kind` (person ids are checked for shape
/// only; row writes also check that they are members).
#[must_use]
pub fn accepts_value(kind: &PropertyKind, raw: &Value) -> bool {
    parse_value(kind, raw).is_ok()
}

fn valid_url(url: &str) -> bool {
    url.chars().count() <= MAX_URL_CHARS
        && !url.chars().any(char::is_whitespace)
        && reqwest::Url::parse(url).is_ok_and(|parsed| {
            matches!(parsed.scheme(), "https" | "http") && parsed.host_str().is_some()
        })
}

/// The display text of a value (the `any → text` conversion).
fn display_text(kind: &PropertyKind, value: &CellValue) -> String {
    let name = |id: &str| {
        kind.options()
            .into_iter()
            .find(|(option, _)| *option == id)
            .map_or_else(String::new, |(_, name)| name.to_owned())
    };
    match value {
        CellValue::Text(text) | CellValue::Url(text) => text.clone(),
        CellValue::Number(number) => number.to_string(),
        CellValue::Choice(id) => name(id),
        CellValue::Choices(ids) => ids.iter().map(|id| name(id)).collect::<Vec<_>>().join(", "),
        CellValue::Date(date) => date.text(),
        CellValue::People(ids) => ids
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", "),
        CellValue::Checked => "true".to_owned(),
    }
}

/// Result of a type change: the new kind and, for each converted row, its new value (`None`
/// clears it).
pub struct Conversion {
    pub kind: PropertyKind,
    pub values: Vec<(String, Option<CellValue>)>,
}

/// Converts the values of a property of `old` kind to `new_type` (spec 4.2). Selects, multi-selects
/// and statuses keep their options; person and computed types clear everything; any other change
/// goes through the display text.
#[must_use]
pub fn convert(
    old: &PropertyKind,
    new_type: PropertyType,
    values: Vec<(String, CellValue)>,
) -> Conversion {
    let old_type = old.property_type();
    if old_type == PropertyType::Person || new_type == PropertyType::Person || new_type.computed() {
        return Conversion {
            kind: PropertyKind::default_for(new_type),
            values: values.into_iter().map(|(row, _)| (row, None)).collect(),
        };
    }
    let option_type = |kind: PropertyType| {
        matches!(
            kind,
            PropertyType::Select | PropertyType::MultiSelect | PropertyType::Status
        )
    };
    if option_type(old_type) && option_type(new_type) {
        let options: Vec<StatusOption> = match old {
            PropertyKind::Select(list) | PropertyKind::MultiSelect(list) => list
                .iter()
                .map(|option| StatusOption {
                    id: option.id.clone(),
                    name: option.name.clone(),
                    color: option.color,
                    group: StatusGroup::Todo,
                })
                .collect(),
            PropertyKind::Status(list) => list.clone(),
            _ => Vec::new(),
        };
        let kind = options_kind(new_type, options);
        let values = values
            .into_iter()
            .map(|(row, value)| {
                let ids = match value {
                    CellValue::Choice(id) => vec![id],
                    CellValue::Choices(ids) => ids,
                    _ => Vec::new(),
                };
                let value = if new_type == PropertyType::MultiSelect {
                    (!ids.is_empty()).then_some(CellValue::Choices(ids))
                } else {
                    ids.into_iter().next().map(CellValue::Choice)
                };
                (row, value)
            })
            .collect();
        return Conversion { kind, values };
    }
    let texts: Vec<(String, String)> = values
        .into_iter()
        .map(|(row, value)| {
            let text = display_text(old, &value);
            (row, text)
        })
        .collect();
    from_text(new_type, texts)
}

fn options_kind(property_type: PropertyType, options: Vec<StatusOption>) -> PropertyKind {
    match property_type {
        PropertyType::Select => PropertyKind::Select(select_options(options)),
        PropertyType::MultiSelect => PropertyKind::MultiSelect(select_options(options)),
        _ if options
            .iter()
            .any(|option| option.group == StatusGroup::Todo) =>
        {
            PropertyKind::Status(options)
        }
        _ => {
            let mut options = options;
            let mut defaults = default_status_options();
            options.insert(0, defaults.remove(0));
            PropertyKind::Status(options)
        }
    }
}

/// The `text → type` conversions of spec 4.2.
fn from_text(new_type: PropertyType, texts: Vec<(String, String)>) -> Conversion {
    let kind = PropertyKind::default_for(new_type);
    let parse = |text: &str| -> Option<CellValue> {
        let text = text.trim();
        match new_type {
            PropertyType::Text => {
                let text: String = text.chars().take(MAX_TEXT_CHARS).collect();
                (!text.is_empty()).then_some(CellValue::Text(text))
            }
            PropertyType::Number => text
                .parse::<f64>()
                .ok()
                .filter(|number| number.is_finite())
                .map(CellValue::Number),
            PropertyType::Url => valid_url(text).then(|| CellValue::Url(text.to_owned())),
            PropertyType::Checkbox => ["true", "yes", "1", "x"]
                .iter()
                .any(|truthy| text.eq_ignore_ascii_case(truthy))
                .then_some(CellValue::Checked),
            PropertyType::Date => DateValue::from_text(text).map(CellValue::Date),
            _ => None,
        }
    };
    if !matches!(
        new_type,
        PropertyType::Select | PropertyType::MultiSelect | PropertyType::Status
    ) {
        return Conversion {
            values: texts
                .into_iter()
                .map(|(row, text)| {
                    let value = parse(&text);
                    (row, value)
                })
                .collect(),
            kind,
        };
    }
    // Options from the distinct values, in first-seen order; values past 200 options clear.
    let multi = new_type == PropertyType::MultiSelect;
    let mut options: Vec<StatusOption> = Vec::new();
    let mut ids: HashMap<String, String> = HashMap::new();
    let mut values = Vec::with_capacity(texts.len());
    for (row, text) in texts {
        let names: Vec<String> = if multi {
            text.split(',').map(|name| name.trim().to_owned()).collect()
        } else {
            vec![text.trim().to_owned()]
        };
        let mut chosen = Vec::new();
        for name in names {
            let name: String = name.chars().take(MAX_NAME_CHARS).collect();
            if name.is_empty() {
                continue;
            }
            let id = match ids.get(&name) {
                Some(id) => id.clone(),
                None if options.len() < MAX_OPTIONS => {
                    let id = Id::new_v7().to_string();
                    options.push(StatusOption {
                        id: id.clone(),
                        name: name.clone(),
                        color: OptionColor::Default,
                        group: StatusGroup::Todo,
                    });
                    ids.insert(name, id.clone());
                    id
                }
                None => continue,
            };
            if !chosen.contains(&id) && chosen.len() < MAX_CELL_ITEMS {
                chosen.push(id);
            }
        }
        let value = if multi {
            (!chosen.is_empty()).then_some(CellValue::Choices(chosen))
        } else {
            chosen.into_iter().next().map(CellValue::Choice)
        };
        values.push((row, value));
    }
    let kind = if options.is_empty() {
        kind
    } else {
        options_kind(new_type, options)
    };
    Conversion { kind, values }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
#[schema(as = DatabaseViewType)]
pub enum ViewType {
    #[default]
    Table,
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
        Type::Title | Type::Text | Type::Url => match operator {
            Op::Equals
            | Op::NotEquals
            | Op::Contains
            | Op::NotContains
            | Op::StartsWith
            | Op::EndsWith => RuleValue::Text,
            _ if empty => RuleValue::Nothing,
            _ => return None,
        },
        Type::Number => match operator {
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
        Type::Checkbox => match operator {
            Op::Is => RuleValue::Bool,
            _ => return None,
        },
        Type::Date | Type::CreatedTime | Type::LastEditedTime => match operator {
            Op::Is | Op::Before | Op::After | Op::OnOrBefore | Op::OnOrAfter => RuleValue::Date,
            _ if empty => RuleValue::Nothing,
            _ => return None,
        },
        Type::CreatedBy | Type::LastEditedBy => match operator {
            Op::Is | Op::IsNot => RuleValue::People,
            _ => return None,
        },
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

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
#[schema(as = DatabaseViewFilter)]
pub struct ViewFilter {
    /// All rules must match (AND). At most 20.
    #[serde(default)]
    pub rules: Vec<FilterRule>,
}

/// A table view's saved layout. Rules, sorts and columns that name a deleted property are
/// dropped when the view is read and saved.
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
    pub filter: ViewFilter,
    #[serde(default)]
    pub wrap_cells: bool,
}

impl ViewConfig {
    /// Every property as a visible column, in property order.
    fn for_properties(properties: &[Property]) -> Self {
        Self {
            columns: properties.iter().map(Property::column).collect(),
            ..Self::default()
        }
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
            .rules
            .retain(|rule| known.contains(&rule.property_id));
    }

    /// Checks limits, widths and each rule against its property's type, then drops what names
    /// unknown properties. Errors name the field path.
    pub fn validate(&mut self, properties: &[Property]) -> Result<(), String> {
        if self.sorts.len() > MAX_SORTS {
            return Err("config.sorts".to_owned());
        }
        if self.filter.rules.len() > MAX_RULES {
            return Err("config.filter.rules".to_owned());
        }
        if self.columns.len() > MAX_PROPERTIES * 2 {
            return Err("config.columns".to_owned());
        }
        for (index, column) in self.columns.iter().enumerate() {
            if !(MIN_WIDTH..=MAX_WIDTH).contains(&column.width) {
                return Err(format!("config.columns[{index}].width"));
            }
        }
        let types: HashMap<Id, PropertyType> = properties
            .iter()
            .map(|property| (property.id, property.kind.property_type()))
            .collect();
        for (index, rule) in self.filter.rules.iter().enumerate() {
            let Some(property_type) = types.get(&rule.property_id) else {
                continue;
            };
            let Some(kind) = rule_value(*property_type, rule.operator) else {
                return Err(format!("config.filter.rules[{index}].operator"));
            };
            if !rule_value_fits(kind, &rule.value) {
                return Err(format!("config.filter.rules[{index}].value"));
            }
        }
        self.retain_known(properties);
        Ok(())
    }

    /// Drops filter rules whose operator or value does not fit their property; returns how many.
    fn drop_unfit_rules(&mut self, properties: &[Property]) -> usize {
        let types: HashMap<Id, PropertyType> = properties
            .iter()
            .map(|property| (property.id, property.kind.property_type()))
            .collect();
        let before = self.filter.rules.len();
        self.filter.rules.retain(|rule| {
            types.get(&rule.property_id).is_none_or(|property_type| {
                rule_value(*property_type, rule.operator)
                    .is_some_and(|kind| rule_value_fits(kind, &rule.value))
            })
        });
        before - self.filter.rules.len()
    }

    fn remap(&mut self, properties: &HashMap<Id, Id>) {
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
            .rules
            .iter_mut()
            .for_each(|rule| map(&mut rule.property_id));
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
    /// Stored values by property id; a missing property is empty.
    pub values: BTreeMap<String, Value>,
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
    pub config: Option<ViewConfig>,
    pub duplicate_of: Option<Id>,
}

#[derive(Default)]
pub struct ViewChanges {
    pub name: Option<String>,
    pub config: Option<ViewConfig>,
    pub position: Option<i64>,
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
    #[error("a database keeps at least one view")]
    LastView,
    /// The title property cannot be deleted or change its type.
    #[error("the title property cannot change")]
    TitleProperty,
    #[error("trial limit reached")]
    TrialLimit(TrialLimitReached),
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

#[derive(Clone)]
pub struct DatabaseRepository {
    database: Database,
}

impl DatabaseRepository {
    #[must_use]
    pub fn new(database: Database) -> Self {
        Self { database }
    }

    pub async fn get(
        &self,
        workspace_id: Id,
        database_id: Id,
        actor_id: Id,
    ) -> Result<DatabaseDetail, DatabaseError> {
        let mut tx = self.database.transaction().await?;
        let page = open(&mut tx, workspace_id, database_id, actor_id).await?;
        let properties = properties_in_tx(&mut tx, database_id).await?;
        let views = views_in_tx(&mut tx, database_id, &properties).await?;
        let rows = rows_in_tx(&mut tx, database_id, None).await?;
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
                let property = Property {
                    id: Id::new_v7(),
                    name,
                    kind,
                    position: 0,
                };
                (property, insert_index(position, properties.len()), None)
            }
            NewProperty::Duplicate { source, name } => {
                let at = properties
                    .iter()
                    .position(|property| property.id == source)
                    .ok_or_else(|| DatabaseError::invalid("duplicate_of"))?;
                let original = properties[at].clone();
                if original.kind == PropertyKind::Title {
                    return Err(DatabaseError::invalid("duplicate_of"));
                }
                let (kind, option_ids) = original.kind.with_new_option_ids();
                let property = Property {
                    id: Id::new_v7(),
                    name: name.unwrap_or_else(|| copy_name(&original.name)),
                    kind,
                    position: 0,
                };
                (property, at + 1, Some((original, option_ids)))
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
        for mut view in raw_views(&mut tx, database_id).await? {
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
            ..property.output()
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
            (Some(PropertyType::Title), _) => return Err(DatabaseError::invalid("type")),
            // A type change derives the new config; set it in a second request.
            (Some(_), Some(_)) => return Err(DatabaseError::invalid("config")),
            (Some(new_type), None) => {
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
        properties.remove(index);
        let index = insert_index(changes.position.or(Some(index as i64)), properties.len());
        properties.insert(index, property.clone());
        renumber_rows(&mut tx, "database_properties", &properties_ids(&properties)).await?;
        let update = PropertyUpdate {
            property: DatabaseProperty {
                position: index as i64,
                ..property.output()
            },
            converted,
            cleared,
        };
        if changes.dry_run {
            // Nothing is kept, so nothing is audited and no realtime event goes out.
            tx.rollback().await?;
            return Ok(update);
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
        // Its values go by cascade.
        sqlx::query("DELETE FROM database_properties WHERE id = $1")
            .bind(property_id.to_string())
            .execute(&mut *tx)
            .await?;
        properties.remove(index);
        renumber_rows(&mut tx, "database_properties", &properties_ids(&properties)).await?;
        for mut view in raw_views(&mut tx, database_id).await? {
            let before = view.config.clone();
            view.config.retain_known(&properties);
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
        let page = open(&mut tx, workspace_id, database_id, actor_id).await?;
        unlocked(&page)?;
        let properties = properties_in_tx(&mut tx, database_id).await?;
        let views = raw_views(&mut tx, database_id).await?;
        if views.len() >= MAX_VIEWS {
            return Err(DatabaseError::ViewLimit);
        }
        let mut config = match (input.config, input.duplicate_of) {
            (Some(config), _) => config,
            (None, Some(source)) => views
                .iter()
                .find(|view| view.id == source)
                .map(|view| view.config.clone())
                .ok_or_else(|| DatabaseError::invalid("duplicate_of"))?,
            (None, None) => ViewConfig::for_properties(&properties),
        };
        config
            .validate(&properties)
            .map_err(DatabaseError::invalid)?;
        let view = DatabaseView {
            id: Id::new_v7(),
            name: input.name,
            view_type: ViewType::Table,
            config,
            position: views.len() as i64,
        };
        insert_view(&mut tx, workspace_id, database_id, &view, now).await?;
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
        let page = open(&mut tx, workspace_id, database_id, actor_id).await?;
        unlocked(&page)?;
        let properties = properties_in_tx(&mut tx, database_id).await?;
        let mut views = raw_views(&mut tx, database_id).await?;
        let index = views
            .iter()
            .position(|view| view.id == view_id)
            .ok_or(DatabaseError::ViewNotFound)?;
        let mut view = views.remove(index);
        if let Some(mut config) = changes.config {
            config
                .validate(&properties)
                .map_err(DatabaseError::invalid)?;
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
        view.config.retain_known(&properties);
        Ok(view)
    }

    pub async fn delete_view(
        &self,
        workspace_id: Id,
        database_id: Id,
        view_id: Id,
        actor_id: Id,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), DatabaseError> {
        let mut tx = self.database.immediate_transaction().await?;
        let page = open(&mut tx, workspace_id, database_id, actor_id).await?;
        unlocked(&page)?;
        let mut views = raw_views(&mut tx, database_id).await?;
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
        self.create_row_with_id(
            Id::new_v7(),
            workspace_id,
            database_id,
            actor_id,
            input,
            request_id,
            now,
        )
        .await
    }

    /// [`Self::create_row`] with a caller-chosen id, so a resumable job (the Notion import) can
    /// record the id before the row exists.
    #[allow(clippy::too_many_arguments)]
    pub async fn create_row_with_id(
        &self,
        row_id: Id,
        workspace_id: Id,
        database_id: Id,
        actor_id: Id,
        input: NewRow,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<DatabaseRow, DatabaseError> {
        let mut tx = self.database.immediate_transaction().await?;
        let page = open(&mut tx, workspace_id, database_id, actor_id).await?;
        let live: i64 = sqlx::query_scalar(
            "SELECT CAST(COUNT(*) AS BIGINT) FROM pages WHERE parent_id = $1 AND deleted_at IS NULL",
        )
        .bind(database_id.to_string())
        .fetch_one(&mut *tx)
        .await?;
        if live >= MAX_ROWS {
            return Err(DatabaseError::RowLimit);
        }
        ensure_trial_capacity(&self.database, &mut tx, workspace_id, TrialLimit::Pages, 1).await?;
        let properties = properties_in_tx(&mut tx, database_id).await?;
        let values = parse_values(&mut tx, workspace_id, &properties, &input.values).await?;
        let space = page.space(actor_id);
        let mut siblings =
            sibling_ids(&mut tx, workspace_id, space, Some(database_id), row_id).await?;
        let index = placement_index(&siblings, input.placement)?;
        siblings.insert(index, row_id);
        sqlx::query(
            "INSERT INTO pages (id, workspace_id, parent_id, teamspace_id, owner_id, title, icon, position, \
             creator_id, updated_by, version, created_at, updated_at, content_json, content_text) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, 0, $11, $12, '[]', '')",
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
        .execute(&mut *tx)
        .await?;
        renumber(&mut tx, &siblings).await?;
        let row = row_id.to_string();
        for (property_id, value) in &values {
            if let Some(value) = value {
                upsert_value(
                    &mut tx,
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
        record_with(
            &mut tx,
            workspace_id,
            actor_id,
            "database.row_created",
            database_id,
            json!({ "row_id": row }),
            request_id,
            now,
        )
        .await?;
        let row = single_row(&mut tx, database_id, row_id).await?;
        tx.commit().await?;
        Ok(row)
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
        require_row(&mut tx, database_id, row_id).await?;
        let properties = properties_in_tx(&mut tx, database_id).await?;
        let values = parse_values(&mut tx, workspace_id, &properties, &input).await?;
        let row = row_id.to_string();
        for (property_id, value) in &values {
            match value {
                Some(value) => {
                    upsert_value(
                        &mut tx,
                        workspace_id,
                        &row,
                        *property_id,
                        value,
                        actor_id,
                        now,
                    )
                    .await?;
                }
                None => delete_value(&mut tx, &row, *property_id).await?,
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
        .execute(&mut *tx)
        .await?;
        record_with(
            &mut tx,
            workspace_id,
            actor_id,
            "database.values_updated",
            database_id,
            json!({
                "row_id": row,
                "property_ids": values.iter().map(|(id, _)| id.to_string()).collect::<Vec<_>>(),
            }),
            request_id,
            now,
        )
        .await?;
        let row = single_row(&mut tx, database_id, row_id).await?;
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
            sibling_ids(&mut tx, workspace_id, space, Some(database_id), database_id)
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
            sibling_ids(&mut tx, workspace_id, space, Some(database_id), database_id).await?;
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

    /// Puts a row first (`None`) or right after another row (manual order).
    #[allow(clippy::too_many_arguments)]
    pub async fn move_row(
        &self,
        workspace_id: Id,
        database_id: Id,
        row_id: Id,
        actor_id: Id,
        after: Option<Id>,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<DatabaseRow, DatabaseError> {
        let mut tx = self.database.immediate_transaction().await?;
        let page = open(&mut tx, workspace_id, database_id, actor_id).await?;
        require_row(&mut tx, database_id, row_id).await?;
        let mut siblings = sibling_ids(
            &mut tx,
            workspace_id,
            page.space(actor_id),
            Some(database_id),
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
        let row = single_row(&mut tx, database_id, row_id).await?;
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
    sqlx::query("SELECT 1 FROM pages WHERE id = $1 AND parent_id = $2 AND deleted_at IS NULL")
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
        let value = parse_value(&property.kind, raw).map_err(|()| path())?;
        if let Some(CellValue::People(people)) = &value {
            if members.is_none() {
                members = Some(
                    sqlx::query_scalar::<_, String>(
                        "SELECT user_id FROM memberships WHERE workspace_id = $1",
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
            let property_type =
                PropertyType::parse(&row.get::<String, _>("type")).ok_or(DatabaseError::Corrupt)?;
            let config: Value = serde_json::from_str(&row.get::<String, _>("config_json"))
                .map_err(|_| DatabaseError::Corrupt)?;
            Ok(Property {
                id: parse_id(&row.get::<String, _>("id"))?,
                name: row.get("name"),
                kind: PropertyKind::parse(property_type, Some(&config))
                    .map_err(|_| DatabaseError::Corrupt)?,
                position: row.get("position"),
            })
        })
        .collect()
}

/// The stored views, configs as saved.
async fn raw_views(
    tx: &mut Transaction<'_, Any>,
    database_id: Id,
) -> Result<Vec<DatabaseView>, DatabaseError> {
    let rows = sqlx::query(
        "SELECT id, name, config_json, position FROM database_views \
         WHERE database_id = $1 ORDER BY position, id",
    )
    .bind(database_id.to_string())
    .fetch_all(&mut **tx)
    .await?;
    rows.into_iter()
        .map(|row| {
            Ok(DatabaseView {
                id: parse_id(&row.get::<String, _>("id"))?,
                name: row.get("name"),
                view_type: ViewType::Table,
                config: serde_json::from_str(&row.get::<String, _>("config_json"))
                    .map_err(|_| DatabaseError::Corrupt)?,
                position: row.get("position"),
            })
        })
        .collect()
}

/// The views as clients see them: without what names deleted properties.
async fn views_in_tx(
    tx: &mut Transaction<'_, Any>,
    database_id: Id,
    properties: &[Property],
) -> Result<Vec<DatabaseView>, DatabaseError> {
    let mut views = raw_views(tx, database_id).await?;
    for view in &mut views {
        view.config.retain_known(properties);
    }
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
    let only = only.map(|id| id.to_string());
    let rows = sqlx::query(
        "SELECT id, title, icon, position, version, created_at, creator_id, updated_at, updated_by, \
         content_text <> '' AS has_content FROM pages \
         WHERE parent_id = $1 AND deleted_at IS NULL AND ($2 IS NULL OR id = $3) ORDER BY position, id",
    )
    .bind(database_id.to_string())
    .bind(&only)
    .bind(&only)
    .fetch_all(&mut **tx)
    .await?;
    let stored = sqlx::query(
        "SELECT page_property_values.page_id AS page_id, page_property_values.property_id AS property_id, \
         page_property_values.value_json AS value_json FROM page_property_values \
         JOIN pages ON pages.id = page_property_values.page_id \
         WHERE pages.parent_id = $1 AND pages.deleted_at IS NULL AND ($2 IS NULL OR pages.id = $3)",
    )
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
            })
        })
        .collect()
}

async fn single_row(
    tx: &mut Transaction<'_, Any>,
    database_id: Id,
    row_id: Id,
) -> Result<DatabaseRow, DatabaseError> {
    rows_in_tx(tx, database_id, Some(row_id))
        .await?
        .pop()
        .ok_or(DatabaseError::NotFound)
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
    view: &DatabaseView,
    now: TimestampMillis,
) -> Result<(), DatabaseError> {
    sqlx::query(
        "INSERT INTO database_views (id, workspace_id, database_id, name, type, config_json, position, created_at, updated_at) \
         VALUES ($1, $2, $3, $4, 'table', $5, $6, $7, $8)",
    )
    .bind(view.id.to_string())
    .bind(workspace_id.to_string())
    .bind(database_id.to_string())
    .bind(&view.name)
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

/// A table view of a [`DatabaseSchema`]. Empty `columns` show every property.
pub struct SchemaView {
    pub name: String,
    pub config: ViewConfig,
}

/// The properties and views a new database starts with, checked by the same rules as the
/// property and view routes.
pub struct DatabaseSchema {
    properties: Vec<Property>,
    views: Vec<DatabaseView>,
}

impl DatabaseSchema {
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
                    .validate(&properties)
                    .map_err(|field| DatabaseError::invalid(format!("views[{index}].{field}")))?;
                Ok(DatabaseView {
                    id: Id::new_v7(),
                    name,
                    view_type: ViewType::Table,
                    config,
                    position: index as i64,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok((Self { properties, views }, dropped))
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
        Self { properties, views }
    }
}

/// Writes a new database's schema; `None` is the starter schema of `POST /pages`.
pub(super) async fn insert_schema(
    tx: &mut Transaction<'_, Any>,
    workspace_id: Id,
    database_id: Id,
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
        insert_view(tx, workspace_id, database_id, view, now)
            .await
            .map_err(page_error)?;
    }
    Ok(())
}

fn page_error(error: DatabaseError) -> PageError {
    match error {
        DatabaseError::Unavailable(error) => PageError::Unavailable(error),
        _ => PageError::Corrupt,
    }
}

/// Copies database data for a page duplicate: the properties and views of each database in
/// `databases` (with new ids; views point at the copied properties) and the values of every
/// copied page (`pages`: original id → copy id). A value of a property that was not copied (a
/// duplicated row stays in its database) keeps its property.
pub(super) async fn copy_database_data(
    tx: &mut Transaction<'_, Any>,
    workspace_id: Id,
    databases: &[Id],
    pages: &HashMap<Id, Id>,
    actor_id: Id,
    now: TimestampMillis,
) -> Result<(), PageError> {
    let mut property_ids: HashMap<Id, Id> = HashMap::new();
    for database_id in databases {
        let copy = pages[database_id];
        for property in properties_in_tx(tx, *database_id)
            .await
            .map_err(page_error)?
        {
            let id = Id::new_v7();
            property_ids.insert(property.id, id);
            insert_property(tx, workspace_id, copy, &Property { id, ..property }, now).await?;
        }
        for mut view in raw_views(tx, *database_id).await.map_err(page_error)? {
            view.config.remap(&property_ids);
            view.id = Id::new_v7();
            insert_view(tx, workspace_id, copy, &view, now)
                .await
                .map_err(page_error)?;
        }
    }
    for (original, copy) in pages {
        let rows = sqlx::query(
            "SELECT property_id, value_json FROM page_property_values WHERE page_id = $1 ORDER BY property_id",
        )
        .bind(original.to_string())
        .fetch_all(&mut **tx)
        .await?;
        for row in rows {
            let property_id = parse_id(&row.get::<String, _>("property_id")).map_err(page_error)?;
            sqlx::query(
                "INSERT INTO page_property_values (page_id, property_id, workspace_id, value_json, updated_by, updated_at) \
                 VALUES ($1, $2, $3, $4, $5, $6)",
            )
            .bind(copy.to_string())
            .bind(property_ids.get(&property_id).unwrap_or(&property_id).to_string())
            .bind(workspace_id.to_string())
            .bind(row.get::<String, _>("value_json"))
            .bind(actor_id.to_string())
            .bind(now.as_millis())
            .execute(&mut **tx)
            .await?;
        }
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        CellValue, DatePoint, DateValue, PropertyKind, PropertyType, convert, parse_value,
    };

    fn select(names: &[(&str, &str)]) -> PropertyKind {
        PropertyKind::parse(
            PropertyType::Select,
            Some(&json!({"options": names.iter().map(|(id, name)| json!({"id": id, "name": name})).collect::<Vec<_>>()})),
        )
        .unwrap()
    }

    #[test]
    fn dates_parse_days_and_utc_times() {
        assert_eq!(
            DatePoint::parse("2028-02-29"),
            Some(DatePoint::Day("2028-02-29".into()))
        );
        assert_eq!(DatePoint::parse("2026-02-29"), None);
        assert_eq!(
            DatePoint::parse("2026-11-09T14:05Z"),
            Some(DatePoint::Moment("2026-11-09T14:05:00Z".into()))
        );
        assert_eq!(
            DatePoint::parse("2026-11-09T14:05:07.250Z"),
            Some(DatePoint::Moment("2026-11-09T14:05:07Z".into()))
        );
        for bad in [
            "2026-13-01",
            "2026-11-9",
            "2026-11-09T24:00Z",
            "2026-11-09T14:00",
            "x",
        ] {
            assert_eq!(DatePoint::parse(bad), None, "{bad}");
        }
        assert!(DateValue::from_text("2026-11-10/2026-11-09").is_none());
        assert!(DateValue::from_text("2026-11-09/2026-11-09T10:00Z").is_none());
    }

    #[test]
    fn checkbox_false_and_empty_text_clear() {
        assert_eq!(
            parse_value(&PropertyKind::Checkbox, &json!(false)),
            Ok(None)
        );
        assert_eq!(
            parse_value(&PropertyKind::Checkbox, &json!(true)),
            Ok(Some(CellValue::Checked))
        );
        assert_eq!(parse_value(&PropertyKind::Text, &json!("")), Ok(None));
        assert_eq!(parse_value(&PropertyKind::Text, &json!(3)), Err(()));
    }

    #[test]
    fn multi_to_single_keeps_the_first_option() {
        let multi = PropertyKind::parse(
            PropertyType::MultiSelect,
            Some(&json!({"options": [{"id": "a", "name": "A"}, {"id": "b", "name": "B"}]})),
        )
        .unwrap();
        let conversion = convert(
            &multi,
            PropertyType::Select,
            vec![(
                "row".into(),
                CellValue::Choices(vec!["b".into(), "a".into()]),
            )],
        );
        assert_eq!(conversion.kind, select(&[("a", "A"), ("b", "B")]));
        assert_eq!(
            conversion.values,
            [("row".to_owned(), Some(CellValue::Choice("b".into())))]
        );
    }
}
