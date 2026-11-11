//! Cell values: dates, parsing and checking a raw value against its property, display text and
//! type conversion.

use std::collections::{HashMap, HashSet};

use orbit_platform::Id;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    FileRef, MAX_CELL_ITEMS, MAX_NAME_CHARS, MAX_OPTIONS, MAX_TEXT_CHARS, MAX_URL_CHARS,
    OptionColor, PropertyKind, PropertyType, StatusGroup, StatusOption, default_status_options,
    select_options,
};
use super::{contact, files, relations, unique_id};

/// A date (`2026-11-09`) or a UTC time (`2026-11-09T14:00:00Z`), stored normalized.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum DatePoint {
    Day(String),
    Moment(String),
}

impl DatePoint {
    /// Accepts `YYYY-MM-DD`, `YYYY-MM-DDTHH:MM[:SS[.fraction]]Z` and RFC 3339 times with an offset
    /// (`2026-11-10T14:00:00+01:00`), which it stores as the UTC time.
    pub(super) fn parse(value: &str) -> Option<Self> {
        let (day, time) = match value.split_once('T') {
            Some((day, time)) => (day, Some(time)),
            None => (value, None),
        };
        valid_day(day)?;
        let Some(time) = time else {
            return Some(Self::Day(day.to_owned()));
        };
        let Some(time) = time.strip_suffix('Z') else {
            if !time.contains(['+', '-']) {
                return None;
            }
            let moment = value.parse::<jiff::Timestamp>().ok()?;
            return Self::parse(&moment.strftime("%Y-%m-%dT%H:%M:%SZ").to_string());
        };
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

    pub(super) fn as_str(&self) -> &str {
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

    pub(super) fn from_text(text: &str) -> Option<Self> {
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
    /// Related page ids of a relation cell (stored in `page_relations`, never as a value).
    Pages(Vec<Id>),
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
    /// Files of the row page, unique, at most 20.
    Files(Vec<FileRef>),
    /// A unique ID number (the prefix is the property's).
    UniqueId(i64),
}

impl CellValue {
    #[must_use]
    pub fn to_json(&self) -> Value {
        match self {
            Self::Pages(ids) => json!(ids),
            Self::Text(text) | Self::Choice(text) | Self::Url(text) => json!(text),
            Self::Number(number) => json!(number),
            Self::Choices(ids) => json!(ids),
            Self::Date(date) => json!({
                "start": date.start.as_str(),
                "end": date.end.as_ref().map(DatePoint::as_str),
            }),
            Self::People(ids) => json!(ids),
            Self::Checked => json!(true),
            Self::Files(files) => json!(files),
            Self::UniqueId(number) => json!(number),
        }
    }

    /// The value with its option ids replaced by `ids` (old id → new id).
    pub(super) fn with_option_ids(self, ids: &HashMap<String, String>) -> Self {
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
pub(super) fn parse_value(kind: &PropertyKind, raw: &Value) -> Result<Option<CellValue>, ()> {
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
        PropertyKind::Email => contact::parse(raw, contact::valid_email)?,
        PropertyKind::Phone => contact::parse(raw, contact::valid_phone)?,
        PropertyKind::Files => files::parse(raw)?,
        PropertyKind::UniqueId(_) => Some(CellValue::UniqueId(
            raw.as_i64().filter(|number| *number > 0).ok_or(())?,
        )),
        PropertyKind::Relation(config) => relations::parse_ids(config, raw)?.map(CellValue::Pages),
        PropertyKind::Rollup(_) => return Err(()),
        PropertyKind::Title
        | PropertyKind::CreatedTime
        | PropertyKind::CreatedBy
        | PropertyKind::LastEditedTime
        | PropertyKind::LastEditedBy
        | PropertyKind::Button(_)
        | PropertyKind::Formula(_) => return Err(()),
    };
    Ok(value)
}

/// Whether `raw` is a valid value for a property of `kind` (person ids are checked for shape
/// only; row writes also check that they are members).
#[must_use]
pub fn accepts_value(kind: &PropertyKind, raw: &Value) -> bool {
    parse_value(kind, raw).is_ok()
}

pub(super) fn valid_url(url: &str) -> bool {
    url.chars().count() <= MAX_URL_CHARS
        && !url.chars().any(char::is_whitespace)
        && reqwest::Url::parse(url).is_ok_and(|parsed| {
            matches!(parsed.scheme(), "https" | "http") && parsed.host_str().is_some()
        })
}

/// The display text of a value (the `any → text` conversion).
pub(super) fn display_text(kind: &PropertyKind, value: &CellValue) -> String {
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
        // Titles need the reader's access: exports use `relations::titles`.
        CellValue::Pages(_) => String::new(),
        CellValue::Files(files) => files
            .iter()
            .map(|file| file.name.as_str())
            .collect::<Vec<_>>()
            .join(", "),
        CellValue::UniqueId(number) => unique_id::text(kind, *number),
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
    if old_type == PropertyType::Person
        || matches!(
            new_type,
            PropertyType::Person | PropertyType::Files | PropertyType::UniqueId
        )
        || new_type.computed()
    {
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
/// The `text → type` rule of spec 4.2 for a type without options (`None` for the others).
pub(super) fn scalar_from_text(new_type: PropertyType, text: &str) -> Option<CellValue> {
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
        PropertyType::Email => contact::valid_email(text).then(|| CellValue::Text(text.to_owned())),
        PropertyType::Phone => contact::valid_phone(text).then(|| CellValue::Text(text.to_owned())),
        PropertyType::Checkbox => ["true", "yes", "1", "x"]
            .iter()
            .any(|truthy| text.eq_ignore_ascii_case(truthy))
            .then_some(CellValue::Checked),
        PropertyType::Date => DateValue::from_text(text).map(CellValue::Date),
        _ => None,
    }
}

fn from_text(new_type: PropertyType, texts: Vec<(String, String)>) -> Conversion {
    let kind = PropertyKind::default_for(new_type);
    let parse = |text: &str| scalar_from_text(new_type, text);
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
