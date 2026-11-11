//! The property types of a database and their configs (`PropertyKind`), with select and status
//! options.

use std::collections::{HashMap, HashSet};

use orbit_platform::Id;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use utoipa::ToSchema;

use super::{
    ButtonConfig, FormulaConfig, MAX_NAME_CHARS, MAX_OPTION_ID_CHARS, MAX_OPTIONS, RelationConfig,
    RollupConfig, UniqueIdConfig,
};

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
    Email,
    Phone,
    Files,
    UniqueId,
    Relation,
    Rollup,
    CreatedTime,
    CreatedBy,
    LastEditedTime,
    LastEditedBy,
    Button,
    Formula,
}

impl PropertyType {
    pub(super) const fn as_str(self) -> &'static str {
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
            Self::Email => "email",
            Self::Phone => "phone",
            Self::Files => "files",
            Self::UniqueId => "unique_id",
            Self::Relation => "relation",
            Self::Rollup => "rollup",
            Self::CreatedTime => "created_time",
            Self::CreatedBy => "created_by",
            Self::LastEditedTime => "last_edited_time",
            Self::LastEditedBy => "last_edited_by",
            Self::Button => "button",
            Self::Formula => "formula",
        }
    }

    pub(super) fn parse(value: &str) -> Option<Self> {
        serde_json::from_value(Value::String(value.to_owned())).ok()
    }

    /// Read from the row's page (or, for a button, nothing at all), never stored as a value (and
    /// never set through values).
    pub(super) const fn computed(self) -> bool {
        matches!(
            self,
            Self::Title
                | Self::Rollup
                | Self::CreatedTime
                | Self::CreatedBy
                | Self::LastEditedTime
                | Self::LastEditedBy
                | Self::Button
                | Self::Formula
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
    Email,
    Phone,
    Files,
    UniqueId(UniqueIdConfig),
    /// Links to rows of a database (spec P3 item 9), stored in `page_relations`.
    Relation(RelationConfig),
    /// Computed on read from the pages of a relation (spec P3 11.5).
    Rollup(RollupConfig),
    CreatedTime,
    CreatedBy,
    LastEditedTime,
    LastEditedBy,
    /// Stores no value: a press runs its actions (`buttons`).
    Button(ButtonConfig),
    /// Computed on every read (`formula`); stores no value.
    Formula(FormulaConfig),
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
            Self::Email => PropertyType::Email,
            Self::Phone => PropertyType::Phone,
            Self::Files => PropertyType::Files,
            Self::UniqueId(_) => PropertyType::UniqueId,
            Self::Relation(_) => PropertyType::Relation,
            Self::Rollup(_) => PropertyType::Rollup,
            Self::CreatedTime => PropertyType::CreatedTime,
            Self::CreatedBy => PropertyType::CreatedBy,
            Self::LastEditedTime => PropertyType::LastEditedTime,
            Self::LastEditedBy => PropertyType::LastEditedBy,
            Self::Button(_) => PropertyType::Button,
            Self::Formula(_) => PropertyType::Formula,
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
            PropertyType::Email => Self::Email,
            PropertyType::Phone => Self::Phone,
            PropertyType::Files => Self::Files,
            PropertyType::UniqueId => Self::UniqueId(UniqueIdConfig::default()),
            // Never reached: both need a config (`parse` refuses none, and a type change to
            // them is refused), so this only keeps the match whole.
            PropertyType::Relation | PropertyType::Rollup => Self::Text,
            PropertyType::CreatedTime => Self::CreatedTime,
            PropertyType::CreatedBy => Self::CreatedBy,
            PropertyType::LastEditedTime => Self::LastEditedTime,
            PropertyType::LastEditedBy => Self::LastEditedBy,
            PropertyType::Button => Self::Button(ButtonConfig::default()),
            PropertyType::Formula => Self::Formula(FormulaConfig::default()),
        }
    }

    /// Parses `config` for `property_type`; `None` gives the default. Errors name the field path.
    pub fn parse(property_type: PropertyType, config: Option<&Value>) -> Result<Self, String> {
        if let Some(kind) = relation_kind(property_type, config) {
            return kind;
        }
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
            PropertyType::UniqueId => Self::UniqueId(UniqueIdConfig::parse(config)?),
            PropertyType::Button => Self::Button(ButtonConfig::parse(config)?),
            PropertyType::Formula => Self::Formula(FormulaConfig::parse(config)?),
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
            Self::Relation(config) => json!(config),
            Self::Rollup(config) => json!(config),
            Self::UniqueId(config) => json!(config),
            Self::Button(config) => json!(config),
            Self::Formula(config) => config.to_json(),
            _ => json!({}),
        }
    }

    /// `(id, name)` of each option, in order; empty for types without options.
    pub(super) fn options(&self) -> Vec<(&str, &str)> {
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

    pub(super) fn has_option(&self, id: &str) -> bool {
        self.options().iter().any(|(option, _)| *option == id)
    }

    /// The same kind with a new id for each option, and the map of old id → new id.
    pub(super) fn with_new_option_ids(&self) -> (Self, HashMap<String, String>) {
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

/// The kind of a relation or rollup from its config (both need one); `None` for other types.
fn relation_kind(
    property_type: PropertyType,
    config: Option<&Value>,
) -> Option<Result<PropertyKind, String>> {
    fn from<T: serde::de::DeserializeOwned>(config: Option<&Value>) -> Result<T, String> {
        config
            .and_then(|config| serde_json::from_value(config.clone()).ok())
            .ok_or_else(|| "config".to_owned())
    }
    Some(match property_type {
        PropertyType::Relation => from(config).map(PropertyKind::Relation),
        PropertyType::Rollup => from(config).map(PropertyKind::Rollup),
        _ => return None,
    })
}

/// "<name> (copy)", within the name limit.
pub(super) fn copy_name(name: &str) -> String {
    const SUFFIX: &str = " (copy)";
    let kept: String = name
        .chars()
        .take(MAX_NAME_CHARS - SUFFIX.chars().count())
        .collect();
    format!("{kept}{SUFFIX}")
}

pub(super) fn default_status_options() -> Vec<StatusOption> {
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

pub(super) fn select_options(options: Vec<StatusOption>) -> Vec<SelectOption> {
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
